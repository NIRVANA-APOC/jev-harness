//! 不可变的原始事件与证据记录，以及两种简单持久化（内存 / JSONL）。
//!
//! 证据是运行时唯一的一手材料：工具输出、环境观测、操作者输入都先落成一条记录，判断节点再从这些
//! 记录里取输入（`docs/RESEARCH_GOALS.md` §5.2）。两条性质是后面所有失效机制的前提：
//!
//! - **记录不可变**：一条记录一旦追加就不再改动，`id` 与 `content_hash` 由日志补齐（调用方给不出，
//!   也就伪造不了）；「撤回」不改写记录，而是另外追加一条撤回标记；
//! - **读取要留痕**：日志能回答「某对象当前仍有效的版本是什么」「这一版上有没有被撤回的记录」，
//!   判断节点的输入投影（[`InputProjection`](crate::InputProjection)）就按这些答案登记与校验。
//!
//! 持久化只有两种：内存与 JSONL。后者一行一条记录、追加即刷新，够 1.0 的实验用；不引入数据库、
//! 不做 schema 迁移（`docs/roadmap-1.0.md` §4 第 1 行只要求「简单持久化即可」）。

use std::collections::BTreeSet;
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::Error;
use crate::hash::ContentHash;

/// 证据指向的对象标识：文件、配置、日志、外部系统实体……
///
/// 一个对象可以有多个版本，同一版本上也可以有多条记录（例如对同一版文件的两条工具输出）；
/// 失效传播按「对象 + 读到的版本」判定，见 [`EvidenceLog::current_version`]。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ObjectId(pub String);

impl ObjectId {
    /// 借出标识文本。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for ObjectId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl From<String> for ObjectId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl fmt::Display for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// 对象版本：记录所描述的对象在那一刻的版本（文件修订号、配置世代号等）。
///
/// 由调用方给出：本 crate 不读时钟、不生成版本号。对象变了就用更大的版本号追加一条新记录，
/// 「当前版本」即仍有效记录里最大的那个版本号。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ObjectVersion(pub u64);

impl fmt::Display for ObjectVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 证据记录的标识：由日志按追加顺序分配，从 1 开始。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EvidenceId(pub u64);

impl fmt::Display for EvidenceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "evidence-{}", self.0)
    }
}

/// 一条不可变的证据记录。
///
/// 字段对应研究目标 §5.2「不可变原始事件与证据记录，保留工具输出、来源、时间、对象版本和内容摘要
/// 哈希」：来源、时间、对象版本是调用方给出的原始事实，`content` 是工具输出原文，
/// [`Evidence::content_hash`] 是内容的稳定摘要（非加密，用途见 [`ContentHash`]）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    /// 日志分配的标识。
    pub id: EvidenceId,
    /// 来源：工具名、通道名或「操作者」等，由调用方给出。
    pub source: String,
    /// 记录时间，Unix 毫秒；由调用方给出。
    pub at_unix_ms: u64,
    /// 这条记录描述的对象。
    pub object: ObjectId,
    /// 这条记录所述对象的版本。
    pub version: ObjectVersion,
    /// 原始内容：工具输出原文或结构化结果。
    pub content: Value,
    /// 内容的摘要哈希（非加密，只用于变更检测与审计标识）。
    pub content_hash: ContentHash,
}

/// 追加一条证据时要给出的字段。
///
/// [`Evidence::id`] 与 [`Evidence::content_hash`] 由日志在追加时补齐，所以它们不在这里：
/// 调用方无法指定一个与自己内容不符的摘要。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewEvidence {
    /// 来源。
    pub source: String,
    /// 记录时间，Unix 毫秒。
    pub at_unix_ms: u64,
    /// 记录描述的对象。
    pub object: ObjectId,
    /// 记录所述对象的版本。
    pub version: ObjectVersion,
    /// 原始内容。
    pub content: Value,
}

/// 证据日志：追加写、不可变的记录集合。
///
/// 两个实现：[`MemoryEvidenceLog`]（内存）与 [`JsonlEvidenceLog`]（一行一条的追加写文件）。
/// 撤回不改动已有记录，只标记它不再可用；因此日志永远只增长，任何时刻都能重放出「当时看到了什么」。
///
/// 追加与撤回都要求 `&mut self`：证据是运行时的公开事实，改动它必须经过
/// [`SemanticState`](crate::SemanticState)，才能连带做失效传播。只读查询随处可用。
pub trait EvidenceLog {
    /// 追加一条证据，返回带 `id` 与内容摘要的完整记录。
    fn append(&mut self, evidence: NewEvidence) -> Result<Evidence, Error>;

    /// 撤回一条证据。已撤回过的再撤回是幂等的；撤回不存在的证据显式失败。
    fn retract(&mut self, id: EvidenceId) -> Result<(), Error>;

    /// 按标识取证据（含已撤回的）。
    fn evidence(&self, id: EvidenceId) -> Option<&Evidence>;

    /// 这条证据是否已被撤回。
    fn is_retracted(&self, id: EvidenceId) -> bool;

    /// 按追加顺序列出全部证据（含已撤回的）。
    fn entries(&self) -> &[Evidence];

    /// 某对象当前仍有效的版本：仍有效记录里最大的版本号；该对象全部记录都已撤回时为 `None`。
    fn current_version(&self, object: &ObjectId) -> Option<ObjectVersion> {
        self.entries()
            .iter()
            .filter(|entry| entry.object == *object && !self.is_retracted(entry.id))
            .map(|entry| entry.version)
            .max()
    }

    /// 某对象在指定版本上是否有被撤回的记录。
    fn retracted_at(&self, object: &ObjectId, version: ObjectVersion) -> bool {
        self.entries().iter().any(|entry| {
            entry.object == *object && entry.version == version && self.is_retracted(entry.id)
        })
    }

    /// 某对象当前版本上仍有效的记录。
    ///
    /// 把这些记录装进请求 state 之后，该对象的输入投影读到的版本就是
    /// [`Self::current_version`] 给出的那个。
    fn current_evidence(&self, object: &ObjectId) -> Vec<&Evidence> {
        let Some(version) = self.current_version(object) else {
            return Vec::new();
        };
        self.entries()
            .iter()
            .filter(|entry| {
                entry.object == *object && entry.version == version && !self.is_retracted(entry.id)
            })
            .collect()
    }
}

/// 内存实现：不落盘，进程退出即消失；测试与一次性的运行用它。
#[derive(Debug, Clone, Default)]
pub struct MemoryEvidenceLog {
    entries: Vec<Evidence>,
    retracted: BTreeSet<EvidenceId>,
}

impl MemoryEvidenceLog {
    fn next_id(&self) -> EvidenceId {
        EvidenceId(self.entries.last().map_or(1, |last| last.id.0 + 1))
    }

    fn build(&self, evidence: NewEvidence) -> Evidence {
        Evidence {
            id: self.next_id(),
            content_hash: ContentHash::of_json(&evidence.content),
            source: evidence.source,
            at_unix_ms: evidence.at_unix_ms,
            object: evidence.object,
            version: evidence.version,
            content: evidence.content,
        }
    }

    fn push(&mut self, entry: Evidence) {
        self.entries.push(entry);
    }

    fn ensure_known(&self, id: EvidenceId) -> Result<(), Error> {
        if self.evidence(id).is_none() {
            return Err(Error::UnknownEvidence(id));
        }
        Ok(())
    }

    /// 重放一行日志（[`JsonlEvidenceLog::open`] 与测试用它）。
    fn replay(&mut self, row: LogRow) -> Result<(), Error> {
        match row {
            LogRow::Evidence(entry) => self.push(entry),
            LogRow::Retraction { evidence } => {
                self.ensure_known(evidence)?;
                self.retracted.insert(evidence);
            }
        }
        Ok(())
    }
}

impl EvidenceLog for MemoryEvidenceLog {
    fn append(&mut self, evidence: NewEvidence) -> Result<Evidence, Error> {
        let entry = self.build(evidence);
        self.push(entry.clone());
        Ok(entry)
    }

    fn retract(&mut self, id: EvidenceId) -> Result<(), Error> {
        self.ensure_known(id)?;
        self.retracted.insert(id);
        Ok(())
    }

    fn evidence(&self, id: EvidenceId) -> Option<&Evidence> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    fn is_retracted(&self, id: EvidenceId) -> bool {
        self.retracted.contains(&id)
    }

    fn entries(&self) -> &[Evidence] {
        &self.entries
    }
}

/// JSONL 日志的一行。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LogRow {
    /// 一条证据。
    Evidence(Evidence),
    /// 一条撤回标记。
    Retraction {
        /// 被撤回的证据。
        evidence: EvidenceId,
    },
}

/// JSONL 实现：一行一条记录，追加写、打开时重放。
///
/// 追加后立即 flush，保证进程退出时已写入的行不丢（不做 fsync 级别的耐久性承诺）。
/// 打开时按行重放：文件不存在就新建，行不是合法 JSON 或撤回了一条不存在的证据都显式失败，
/// 不做「跳过坏行继续」的兜底。
#[derive(Debug)]
pub struct JsonlEvidenceLog {
    path: PathBuf,
    file: File,
    memory: MemoryEvidenceLog,
}

impl JsonlEvidenceLog {
    /// 打开（或新建）日志文件并重放已有行。
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        let path = path.as_ref().to_path_buf();
        let mut memory = MemoryEvidenceLog::default();
        if path.exists() {
            let reader = BufReader::new(File::open(&path).map_err(|error| {
                Error::Storage(format!("{} 无法打开：{error}", path.display()))
            })?);
            for (index, line) in reader.lines().enumerate() {
                let line = line.map_err(|error| {
                    Error::Storage(format!("{} 无法读取：{error}", path.display()))
                })?;
                if line.trim().is_empty() {
                    continue;
                }
                let row: LogRow = serde_json::from_str(&line).map_err(|error| {
                    Error::Storage(format!(
                        "{} 第 {} 行不是合法记录：{error}",
                        path.display(),
                        index + 1
                    ))
                })?;
                memory.replay(row)?;
            }
        }

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|error| Error::Storage(format!("{} 无法写入：{error}", path.display())))?;
        Ok(Self { path, file, memory })
    }

    /// 日志文件路径。
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn write_row(&mut self, row: &LogRow) -> Result<(), Error> {
        let mut line = serde_json::to_string(row)
            .map_err(|error| Error::Storage(format!("记录无法序列化：{error}")))?;
        line.push('\n');
        self.file
            .write_all(line.as_bytes())
            .and_then(|()| self.file.flush())
            .map_err(|error| Error::Storage(format!("{} 无法写入：{error}", self.path.display())))
    }
}

impl EvidenceLog for JsonlEvidenceLog {
    fn append(&mut self, evidence: NewEvidence) -> Result<Evidence, Error> {
        // 先落盘再进内存：写失败时内存里也不会多出一条「不存在的记录」。
        let entry = self.memory.build(evidence);
        self.write_row(&LogRow::Evidence(entry.clone()))?;
        self.memory.push(entry.clone());
        Ok(entry)
    }

    fn retract(&mut self, id: EvidenceId) -> Result<(), Error> {
        self.memory.ensure_known(id)?;
        if self.memory.is_retracted(id) {
            return Ok(());
        }
        self.write_row(&LogRow::Retraction { evidence: id })?;
        self.memory.retracted.insert(id);
        Ok(())
    }

    fn evidence(&self, id: EvidenceId) -> Option<&Evidence> {
        self.memory.evidence(id)
    }

    fn is_retracted(&self, id: EvidenceId) -> bool {
        self.memory.is_retracted(id)
    }

    fn entries(&self) -> &[Evidence] {
        self.memory.entries()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn evidence(object: &str, version: u64, content: Value) -> NewEvidence {
        NewEvidence {
            source: "tool:read_config".to_owned(),
            at_unix_ms: 1_700_000_000_000 + version,
            object: ObjectId::from(object),
            version: ObjectVersion(version),
            content,
        }
    }

    #[test]
    fn append_assigns_ids_and_content_hashes() {
        let mut log = MemoryEvidenceLog::default();
        let first = log
            .append(evidence("cache.conf", 1, json!({"cache": "off"})))
            .expect("内存日志不会失败");
        let second = log
            .append(evidence("cache.conf", 1, json!({"cache": "off"})))
            .expect("内存日志不会失败");

        assert_eq!(first.id, EvidenceId(1));
        assert_eq!(second.id, EvidenceId(2));
        assert_eq!(first.content_hash, second.content_hash);
        assert_eq!(first.content, json!({"cache": "off"}));
        assert_eq!(log.entries().len(), 2);
        assert_eq!(log.evidence(EvidenceId(2)), Some(&second));
        assert_eq!(log.evidence(EvidenceId(7)), None);
    }

    #[test]
    fn retracting_keeps_the_record_and_moves_the_current_version_back() {
        let mut log = MemoryEvidenceLog::default();
        let first = log
            .append(evidence("cache.conf", 1, json!({"cache": "off"})))
            .unwrap();
        let second = log
            .append(evidence("cache.conf", 2, json!({"cache": "on"})))
            .unwrap();
        let object = ObjectId::from("cache.conf");

        assert_eq!(log.current_version(&object), Some(ObjectVersion(2)));

        log.retract(second.id).expect("撤回已存在的证据");
        // 撤回不改写记录：两条都还在，只有第二条不再有效。
        assert_eq!(log.entries().len(), 2);
        assert!(log.is_retracted(second.id));
        assert!(!log.is_retracted(first.id));
        assert_eq!(log.current_version(&object), Some(ObjectVersion(1)));
        assert_eq!(log.current_evidence(&object), vec![&first]);
        assert!(log.retracted_at(&object, ObjectVersion(2)));
        assert!(!log.retracted_at(&object, ObjectVersion(1)));

        // 幂等：再撤回一次不改变任何东西。
        log.retract(second.id).expect("重复撤回是幂等的");
        assert_eq!(log.current_version(&object), Some(ObjectVersion(1)));
    }

    #[test]
    fn retracting_unknown_evidence_fails_explicitly() {
        let mut log = MemoryEvidenceLog::default();
        let error = log
            .retract(EvidenceId(3))
            .expect_err("撤回不存在的证据必须失败");
        assert!(
            matches!(error, Error::UnknownEvidence(EvidenceId(3))),
            "{error}"
        );
    }

    #[test]
    fn jsonl_log_replays_evidence_and_retractions() {
        let dir = std::env::temp_dir().join(format!("jh-core-evidence-{}", std::process::id()));
        let path = dir.join("evidence.jsonl");
        std::fs::create_dir_all(&dir).unwrap();
        let _ = std::fs::remove_file(&path);

        let (first, second) = {
            let mut log = JsonlEvidenceLog::open(&path).expect("新建日志");
            let first = log
                .append(evidence("cache.conf", 1, json!({"cache": "off"})))
                .unwrap();
            let second = log
                .append(evidence("cache.conf", 2, json!({"cache": "on"})))
                .unwrap();
            log.retract(second.id).expect("撤回");
            // 幂等：不该再写第二行撤回记录。
            log.retract(second.id).expect("撤回");
            assert_eq!(log.path(), path.as_path());
            (first, second)
        };

        let rows: Vec<LogRow> = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(rows.len(), 3, "两条证据 + 一条撤回：{rows:?}");
        assert_eq!(rows[0], LogRow::Evidence(first.clone()));

        let mut replayed = JsonlEvidenceLog::open(&path).expect("重放已存在的日志");
        assert_eq!(
            replayed.entries().to_vec(),
            vec![first.clone(), second.clone()]
        );
        assert!(replayed.is_retracted(second.id));
        assert_eq!(
            replayed.current_version(&ObjectId::from("cache.conf")),
            Some(ObjectVersion(1))
        );

        // 重放之后再追加：id 接着往下排。
        let third = replayed
            .append(evidence("cache.conf", 3, json!({"cache": "on"})))
            .expect("重放后仍可追加");
        assert_eq!(third.id, EvidenceId(3));

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn jsonl_log_fails_on_a_corrupt_line() {
        let dir = std::env::temp_dir().join(format!("jh-core-corrupt-{}", std::process::id()));
        let path = dir.join("evidence.jsonl");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, "{\"kind\":\"nonsense\"}\n").unwrap();

        let error = JsonlEvidenceLog::open(&path).expect_err("坏行必须显式失败");
        assert!(matches!(error, Error::Storage(_)), "{error}");
        assert!(error.to_string().contains("第 1 行"), "{error}");

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }
}
