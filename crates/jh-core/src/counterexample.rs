//! 反例：一次失败的原始上下文，以及反例日志的两种简单持久化（内存 / JSONL）。
//!
//! 研究目标 §6.2 第 1、2 步——从**验证失败、预期效果落空或矛盾证据**中提取候选反例，并**恢复当时的
//! 原始证据、问题版本、候选动作和控制规则**。这个模块只负责把那次失败的现场如实留下来：
//!
//! - **不判定归因**：§6.1 的四类可能（原问题回答错误、原始事实缺失、状态表示缺少区分维度、环境或
//!   工具随机）记在 [`Counterexample::attributions`] 里，可多选、可留空（空集 = 还没提出归因）。
//!   本 crate 只要求可记录可查；判定归因属于慢回路里人的或生成模型的事（§6.2 第 3 步）；
//! - **不改写历史**：反例只追加，标识由日志分配；后来补上的认识另记一条，不回头改旧记录
//!   （与 [`Evidence`](crate::Evidence) 同一个口径）；
//! - **没有撤回动作**：失败候选与已处理的反例都留在日志里，供报告与人工复核。
//!
//! 持久化只有两种：内存（[`MemoryCounterexampleLog`]）与 JSONL（[`JsonlCounterexampleLog`]，
//! 一行一条记录、追加即刷新），与证据日志同一种复杂度；文件路径由调用方决定。
//!
//! # 与问题库的关系
//!
//! 反例是「失败 → 修订问题集合」这条慢回路的输入（§6.2 第 1 步），从反例生成候选问题由 `jh-app`
//! 编排（§6.2 第 3–5 步）。本 crate 不在两者之间强制存一条边：反例与候选问题各有稳定标识
//! （[`CounterexampleId`] 与 [`QuestionVersion`](crate::QuestionVersion)），溯源由调用方在生成步骤里记。

use std::collections::BTreeSet;
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::evidence::EvidenceId;
use crate::graph::ActionId;
use crate::question::{QuestionId, QuestionVersion};

/// 反例记录的标识：由日志按追加顺序分配，从 1 开始。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CounterexampleId(pub u64);

impl fmt::Display for CounterexampleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "counterexample-{}", self.0)
    }
}

/// 失败来源：§6.2 第 1 步的三条提取入口。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureSource {
    /// 验证失败：独立验收没通过（如任务族的隐藏检查）。
    VerificationFailed,
    /// 预期效果落空：动作发出了，但没有拿到预期效果。
    ExpectedEffectMissed,
    /// 矛盾证据：新证据与已有判断互相矛盾。
    ContradictoryEvidence,
}

/// 四类可能归因（§6.1）：只记录，不判定。
///
/// 四种可能**允许并存**（同一个失败既可能是答错、也可能是当时就没那份材料），所以记录用的是集合：
/// 空集表示还没提出归因，多于一项表示还没收敛到一条。哪一类成立由人工或生成模型在 §6.2 第 3 步判定。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Attribution {
    /// 原问题回答错误：问题本身能区分这件事，但当时答错了。
    QuestionAnsweredWrongly,
    /// 原始事实缺失：判断需要的原始材料当时不在证据里。
    RawFactMissing,
    /// 状态表示缺少区分维度：现有问题集合表达不出这个差异。
    StateMissesDimension,
    /// 环境或工具存在随机性：同一份材料下的结果本来就不稳定。
    EnvironmentRandomness,
}

/// 一个反例：一次失败，以及当时的上下文。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Counterexample {
    /// 日志分配的标识。
    pub id: CounterexampleId,
    /// 失败来源。
    pub source: FailureSource,
    /// 记录时间，Unix 毫秒；由调用方给出（本 crate 不读时钟）。
    pub at_unix_ms: u64,
    /// 当时在用的问题。
    pub question: QuestionId,
    /// 当时在用的问题版本。
    pub question_version: QuestionVersion,
    /// 当时的原始证据：这次判断读到的那些记录（按调用方给出的顺序）。
    ///
    /// **可以为空**——「当时一条材料都没有」本身就是一种反例（原始事实缺失的候选）。
    pub evidence: Vec<EvidenceId>,
    /// 当时的候选动作：这次失败里在起作用、或本该发出的那个行动。
    pub action: Option<ActionId>,
    /// 当时的控制规则：把答案折成行动的那条规则（如 `cause_primary>=0.6 → rollback-cache`）。
    pub control_rule: Option<String>,
    /// 候选归因（§6.1 的四类）；空集表示还没提出归因。
    pub attributions: BTreeSet<Attribution>,
    /// 说明：这次失败是什么。写给报告与人工复核，可以是空串。
    pub note: String,
}

/// 追加一条反例时要给出的字段。
///
/// [`Counterexample::id`] 由日志在追加时补齐：调用方给不出，也就伪造不了「第几条反例」。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewCounterexample {
    /// 失败来源。
    pub source: FailureSource,
    /// 记录时间，Unix 毫秒。
    pub at_unix_ms: u64,
    /// 当时在用的问题。
    pub question: QuestionId,
    /// 当时在用的问题版本。
    pub question_version: QuestionVersion,
    /// 当时的原始证据。
    pub evidence: Vec<EvidenceId>,
    /// 当时的候选动作。
    pub action: Option<ActionId>,
    /// 当时的控制规则。
    pub control_rule: Option<String>,
    /// 候选归因。
    pub attributions: BTreeSet<Attribution>,
    /// 说明。
    pub note: String,
}

/// 反例日志：追加写、按标识可查的记录集合。
///
/// 两个实现：[`MemoryCounterexampleLog`]（内存）与 [`JsonlCounterexampleLog`]（一行一条的追加写
/// 文件）。反例既不改写也不撤回，所以日志永远只增长，任何时刻都能重放出「当时失败成什么样」
/// ——这是 §6.2 第 2 步「恢复当时现场」的材料来源。
pub trait CounterexampleLog {
    /// 追加一条反例，返回带 `id` 的完整记录。
    fn append(&mut self, counterexample: NewCounterexample) -> Result<Counterexample, Error>;

    /// 按标识取反例。
    fn counterexample(&self, id: CounterexampleId) -> Option<&Counterexample>;

    /// 按追加顺序列出全部反例。
    fn entries(&self) -> &[Counterexample];

    /// 某个问题上的反例（任一版本，按追加顺序）：生成候选问题时回看失败现场用。
    fn for_question(&self, question: &QuestionId) -> Vec<&Counterexample> {
        self.entries()
            .iter()
            .filter(|entry| entry.question == *question)
            .collect()
    }
}

/// 内存实现：不落盘，进程退出即消失；测试与一次性的运行用它。
#[derive(Debug, Clone, Default)]
pub struct MemoryCounterexampleLog {
    entries: Vec<Counterexample>,
}

impl MemoryCounterexampleLog {
    fn next_id(&self) -> CounterexampleId {
        CounterexampleId(self.entries.last().map_or(1, |last| last.id.0 + 1))
    }

    fn build(&self, counterexample: NewCounterexample) -> Counterexample {
        Counterexample {
            id: self.next_id(),
            source: counterexample.source,
            at_unix_ms: counterexample.at_unix_ms,
            question: counterexample.question,
            question_version: counterexample.question_version,
            evidence: counterexample.evidence,
            action: counterexample.action,
            control_rule: counterexample.control_rule,
            attributions: counterexample.attributions,
            note: counterexample.note,
        }
    }

    fn push(&mut self, entry: Counterexample) {
        self.entries.push(entry);
    }
}

impl CounterexampleLog for MemoryCounterexampleLog {
    fn append(&mut self, counterexample: NewCounterexample) -> Result<Counterexample, Error> {
        let entry = self.build(counterexample);
        self.push(entry.clone());
        Ok(entry)
    }

    fn counterexample(&self, id: CounterexampleId) -> Option<&Counterexample> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    fn entries(&self) -> &[Counterexample] {
        &self.entries
    }
}

/// JSONL 实现：一行一条反例，追加写、打开时重放。
///
/// 追加后立即 flush，保证进程退出时已写入的行不丢（不做 fsync 级别的耐久性承诺）。打开时按行重放：
/// 文件不存在就新建，空行跳过，其余行不是合法 JSON 或字段不全都显式失败，不做「跳过坏行继续」的兜底
/// （与 [`JsonlEvidenceLog`](crate::JsonlEvidenceLog) 同一套规则）。
#[derive(Debug)]
pub struct JsonlCounterexampleLog {
    path: PathBuf,
    file: File,
    memory: MemoryCounterexampleLog,
}

impl JsonlCounterexampleLog {
    /// 打开（或新建）日志文件并重放已有行。
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        let path = path.as_ref().to_path_buf();
        let mut memory = MemoryCounterexampleLog::default();
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
                let entry: Counterexample = serde_json::from_str(&line).map_err(|error| {
                    Error::Storage(format!(
                        "{} 第 {} 行不是合法记录：{error}",
                        path.display(),
                        index + 1
                    ))
                })?;
                memory.push(entry);
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

    fn write_row(&mut self, entry: &Counterexample) -> Result<(), Error> {
        let mut line = serde_json::to_string(entry)
            .map_err(|error| Error::Storage(format!("记录无法序列化：{error}")))?;
        line.push('\n');
        self.file
            .write_all(line.as_bytes())
            .and_then(|()| self.file.flush())
            .map_err(|error| Error::Storage(format!("{} 无法写入：{error}", self.path.display())))
    }
}

impl CounterexampleLog for JsonlCounterexampleLog {
    fn append(&mut self, counterexample: NewCounterexample) -> Result<Counterexample, Error> {
        // 先落盘再进内存：写失败时内存里也不会多出一条「不存在的记录」。
        let entry = self.memory.build(counterexample);
        self.write_row(&entry)?;
        self.memory.push(entry.clone());
        Ok(entry)
    }

    fn counterexample(&self, id: CounterexampleId) -> Option<&Counterexample> {
        self.memory.counterexample(id)
    }

    fn entries(&self) -> &[Counterexample] {
        self.memory.entries()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn counterexample(question: &str, note: &str) -> NewCounterexample {
        NewCounterexample {
            source: FailureSource::VerificationFailed,
            at_unix_ms: 1_700_000_000_000,
            question: QuestionId::from(question),
            question_version: QuestionVersion(2),
            evidence: vec![EvidenceId(1), EvidenceId(2)],
            action: Some(ActionId::from("rollback-cache")),
            control_rule: Some("cause_primary>=0.6 → rollback-cache".to_owned()),
            attributions: BTreeSet::from([
                Attribution::StateMissesDimension,
                Attribution::RawFactMissing,
            ]),
            note: note.to_owned(),
        }
    }

    #[test]
    fn append_assigns_ids_and_keeps_the_failure_scene() {
        let mut log = MemoryCounterexampleLog::default();
        let first = log
            .append(counterexample(
                "cause_primary",
                "验收没过：把时钟偏差判成证书过期",
            ))
            .expect("内存日志不会失败");
        let second = log
            .append(counterexample("metrics_within_baseline", "预期效果落空"))
            .expect("内存日志不会失败");

        assert_eq!(first.id, CounterexampleId(1));
        assert_eq!(second.id, CounterexampleId(2));
        assert_eq!(first.question_version, QuestionVersion(2));
        assert_eq!(first.evidence, vec![EvidenceId(1), EvidenceId(2)]);
        assert_eq!(
            first.action.as_ref().map(ActionId::as_str),
            Some("rollback-cache")
        );
        assert_eq!(
            first.control_rule.as_deref(),
            Some("cause_primary>=0.6 → rollback-cache")
        );
        // 四类归因可多选、可留空：这里选了「状态表示缺维度」与「原始事实缺失」两项。
        assert_eq!(
            first.attributions,
            BTreeSet::from([
                Attribution::RawFactMissing,
                Attribution::StateMissesDimension
            ])
        );
        assert_eq!(log.counterexample(CounterexampleId(2)), Some(&second));
        assert_eq!(log.counterexample(CounterexampleId(7)), None);
        assert_eq!(log.entries().len(), 2);
    }

    #[test]
    fn a_counterexample_may_carry_no_evidence_no_action_and_no_attribution() {
        let mut log = MemoryCounterexampleLog::default();
        let entry = log
            .append(NewCounterexample {
                source: FailureSource::ContradictoryEvidence,
                at_unix_ms: 1_700_000_000_001,
                question: QuestionId::from("premise_holds"),
                question_version: QuestionVersion(1),
                // 当时一条材料都没取到，也没有候选动作：这正是「原始事实缺失」的候选现场。
                evidence: Vec::new(),
                action: None,
                control_rule: None,
                attributions: BTreeSet::new(),
                note: String::new(),
            })
            .expect("内存日志不会失败");

        assert!(entry.evidence.is_empty());
        assert!(entry.action.is_none());
        assert!(entry.attributions.is_empty());

        let wire = serde_json::to_value(&entry).unwrap();
        assert_eq!(wire["source"], json!("contradictory_evidence"));
        assert_eq!(wire["id"], json!(1));
        assert_eq!(wire["note"], json!(""));
        let round_tripped: Counterexample = serde_json::from_value(wire).unwrap();
        assert_eq!(round_tripped, entry);
    }

    #[test]
    fn counterexamples_round_trip_on_the_wire_with_their_evidence_and_attributions() {
        let mut log = MemoryCounterexampleLog::default();
        let entry = log
            .append(counterexample("cause_primary", "把时钟偏差判成证书过期"))
            .expect("内存日志不会失败");

        let wire = serde_json::to_value(&entry).unwrap();
        assert_eq!(wire["source"], json!("verification_failed"));
        assert_eq!(wire["question"], json!("cause_primary"));
        assert_eq!(wire["question_version"], json!(2));
        assert_eq!(wire["evidence"], json!([1, 2]));
        assert_eq!(wire["action"], json!("rollback-cache"));
        assert_eq!(
            wire["attributions"],
            json!(["raw_fact_missing", "state_misses_dimension"])
        );
        assert_eq!(
            serde_json::from_value::<Counterexample>(wire).unwrap(),
            entry
        );
    }

    #[test]
    fn for_question_filters_by_question_in_append_order() {
        let mut log = MemoryCounterexampleLog::default();
        log.append(counterexample("cause_primary", "第一条"))
            .unwrap();
        log.append(counterexample("cause_primary", "第二条"))
            .unwrap();
        log.append(counterexample("premise_holds", "别的问题"))
            .unwrap();

        let found = log.for_question(&QuestionId::from("cause_primary"));
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].note, "第一条");
        assert_eq!(found[1].note, "第二条");
        assert!(
            log.for_question(&QuestionId::from("never_recorded"))
                .is_empty()
        );
    }

    #[test]
    fn jsonl_log_replays_its_rows_and_continues_the_numbering() {
        let dir =
            std::env::temp_dir().join(format!("jh-core-counterexample-{}", std::process::id()));
        let path = dir.join("counterexamples.jsonl");
        std::fs::create_dir_all(&dir).unwrap();
        let _ = std::fs::remove_file(&path);

        let (first, second) = {
            let mut log = JsonlCounterexampleLog::open(&path).expect("新建日志");
            let first = log
                .append(counterexample("cause_primary", "第一条"))
                .unwrap();
            let second = log
                .append(counterexample("cause_primary", "第二条"))
                .unwrap();
            assert_eq!(log.path(), path.as_path());
            (first, second)
        };

        let mut replayed = JsonlCounterexampleLog::open(&path).expect("重放已存在的日志");
        assert_eq!(replayed.entries(), &[first.clone(), second.clone()]);
        assert_eq!(replayed.counterexample(CounterexampleId(1)), Some(&first));
        assert_eq!(
            replayed
                .for_question(&QuestionId::from("cause_primary"))
                .len(),
            2
        );

        // 重放之后再追加：id 接着往下排。
        let third = replayed
            .append(counterexample("premise_holds", "第三条"))
            .expect("重放后仍可追加");
        assert_eq!(third.id, CounterexampleId(3));

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn jsonl_log_fails_on_a_corrupt_line() {
        let dir = std::env::temp_dir().join(format!(
            "jh-core-counterexample-corrupt-{}",
            std::process::id()
        ));
        let path = dir.join("counterexamples.jsonl");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, "{\"source\":\"nonsense\"}\n").unwrap();

        let error = JsonlCounterexampleLog::open(&path).expect_err("坏行必须显式失败");
        assert!(matches!(error, Error::Storage(_)), "{error}");
        assert!(error.to_string().contains("第 1 行"), "{error}");

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }
}
