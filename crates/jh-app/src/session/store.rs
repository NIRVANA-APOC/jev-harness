//! 会话历史与恢复：默认落在 `~/.jev-harness/sessions/<run>/`。
//!
//! 每个会话一个目录，四份文件各司其职：
//!
//! | 文件 | 内容 | 谁写 |
//! | --- | --- | --- |
//! | `session.jsonl` | 完整会话日志（用户 / 模型 / 门控 / 审批 / 工具 / 错误） | [`super::log::SessionLog`] |
//! | `conversation.jsonl` | **模型可见的对话项**（`jh_openai::InputItem` 原样），恢复会话靠它 | [`ConversationLog`] |
//! | `meta.json` | 摘要：时间、工作区、模型、轮数、首末句话 | 会话开始与结束 |
//! | `evidence.jsonl` | 语义状态证据（既有） | `JsonlEvidenceLog` |
//!
//! 根目录取 `$JEV_HARNESS_HOME`，没有就用 `$HOME/.jev-harness`（`$HOME` 也取不到时**显式失败**，
//! 不去猜一个「大概能用」的路径）。
//!
//! **恢复的边界**：`/resume` 接回的是**对话上下文**（模型看到的东西）与界面回放；语义状态
//! （判断节点、依赖图、失效传播）与授权白名单不跨进程恢复——它们依赖进程内的运行时对象，
//! 1.0 不做重放（`docs/session-log.md` 登记为未做项）。

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::exp::unix_ms;

/// 覆盖默认根目录的环境变量（多环境与单测用；不是凭据，写进文档没问题）。
pub const HOME_ENV: &str = "JEV_HARNESS_HOME";

/// 会话目录下的文件布局。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionPaths {
    /// 会话目录。
    pub dir: PathBuf,
    /// 摘要文件路径。
    pub meta: PathBuf,
    /// 会话日志文件路径。
    pub log: PathBuf,
    /// 对话记录文件路径。
    pub conversation: PathBuf,
    /// 证据日志文件路径。
    pub evidence: PathBuf,
}

/// 一次会话的摘要。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionMeta {
    /// 会话标识（= 运行标识，也是目录名）。
    pub id: String,
    /// 会话开始时刻。
    pub created_at_unix_ms: u64,
    /// 最近一次更新时间。
    pub updated_at_unix_ms: u64,
    /// 工作区根路径。
    pub workspace: String,
    /// 模型标识。
    pub model: String,
    /// 门控判定器名。
    pub judge: String,
    /// 用户提交次数。
    pub turns: usize,
    /// 首条用户消息（截 80 字）：列表里认人用。
    pub first_message: String,
    /// 最近一条用户消息（截 80 字）。
    pub last_message: String,
}

/// 默认根目录：`$JEV_HARNESS_HOME` 或 `$HOME/.jev-harness`。
pub fn default_root() -> Result<PathBuf, Error> {
    if let Some(root) = std::env::var_os(HOME_ENV).filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(root));
    }
    let home = std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            Error::Judge(jh_core::Error::Unavailable(format!(
                "取不到会话历史目录：请设置 {HOME_ENV} 或 HOME"
            )))
        })?;
    Ok(PathBuf::from(home).join(".jev-harness"))
}

/// 会话列表的读回：读得出的摘要 + 读不出来的（id, 原因）。
///
/// 分开装是为了**不静默丢弃**：一个坏 `meta.json` 不该让会话「凭空消失」，
/// 调用方要把 `skipped` 如实上屏。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionList {
    /// 读得出的会话摘要（按最近更新倒序）。
    pub metas: Vec<SessionMeta>,
    /// 读不出摘要的会话：`(会话 ID, 原因)`。
    pub skipped: Vec<(String, String)>,
}

/// 会话历史的读写入口。
#[derive(Debug, Clone)]
pub struct SessionStore {
    root: PathBuf,
}

impl SessionStore {
    /// 指定根目录。
    pub fn at(root: impl Into<PathBuf>) -> Self {
        SessionStore { root: root.into() }
    }

    /// 默认根目录（`~/.jev-harness`）。
    pub fn open_default() -> Result<Self, Error> {
        Ok(SessionStore::at(default_root()?))
    }

    /// 历史仓库根目录。
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 会话目录下的文件路径（不建目录）。
    pub fn paths(&self, id: &str) -> SessionPaths {
        let dir = self.root.join("sessions").join(id);
        SessionPaths {
            meta: dir.join("meta.json"),
            log: dir.join("session.jsonl"),
            conversation: dir.join("conversation.jsonl"),
            evidence: dir.join("evidence.jsonl"),
            dir,
        }
    }

    /// 开一次会话：建目录 + 写初始摘要。
    pub fn start(
        &self,
        id: &str,
        workspace: &str,
        model: &str,
        judge: &str,
    ) -> Result<SessionPaths, Error> {
        let paths = self.paths(id);
        std::fs::create_dir_all(&paths.dir)?;
        let now = unix_ms();
        self.write_meta(&SessionMeta {
            id: id.to_owned(),
            created_at_unix_ms: now,
            updated_at_unix_ms: now,
            workspace: workspace.to_owned(),
            model: model.to_owned(),
            judge: judge.to_owned(),
            turns: 0,
            first_message: String::new(),
            last_message: String::new(),
        })?;
        Ok(paths)
    }

    /// 写摘要（覆盖写：摘要是当前状态的快照，不是追加流）。
    pub fn write_meta(&self, meta: &SessionMeta) -> Result<(), Error> {
        let path = self.paths(&meta.id).meta;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(meta)?;
        std::fs::write(&path, json)?;
        Ok(())
    }

    /// 读一个会话的摘要。
    pub fn read_meta(&self, id: &str) -> Result<SessionMeta, Error> {
        let path = self.paths(id).meta;
        // 路径与原因都留在消息里（`io::Error` 自己不带路径），但**别套判断器的前缀**：
        // 这是本地文件读不出来，不是判定器不可用（真调里踩过：`/resume` 报「判断器不可用：
        // 会话摘要不可读」）。
        let text = std::fs::read_to_string(&path).map_err(|error| {
            Error::Io(std::io::Error::new(
                error.kind(),
                format!("会话摘要不可读（{}）：{error}", path.display()),
            ))
        })?;
        serde_json::from_str(&text).map_err(|error| {
            Error::Io(std::io::Error::other(format!(
                "会话摘要不是合法 JSON（{}）：{error}",
                path.display()
            )))
        })
    }

    /// 列出全部会话摘要，按最近更新倒序。
    ///
    /// 读不出摘要的会话**不静默丢弃**：连同原因一起返回（`skipped`），由调用方如实上屏——
    /// 一个坏 `meta.json` 不该让会话「凭空消失」（评审 r1 的发现）。
    pub fn list(&self) -> Result<SessionList, Error> {
        let sessions = self.root.join("sessions");
        let entries = match std::fs::read_dir(&sessions) {
            Ok(entries) => entries,
            // 还没有任何会话：空列表，不是错误。
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(SessionList::default());
            }
            Err(error) => return Err(error.into()),
        };
        let mut metas = Vec::new();
        let mut skipped = Vec::new();
        for entry in entries.filter_map(Result::ok) {
            if !entry.path().is_dir() {
                continue;
            }
            let id = entry.file_name().to_string_lossy().into_owned();
            match self.read_meta(&id) {
                Ok(meta) => metas.push(meta),
                Err(error) => skipped.push((id, error.to_string())),
            }
        }
        metas.sort_by_key(|meta| std::cmp::Reverse(meta.updated_at_unix_ms));
        Ok(SessionList { metas, skipped })
    }

    /// 读回一个会话的对话项（恢复用）。
    pub fn read_conversation(&self, id: &str) -> Result<Vec<jh_openai::InputItem>, Error> {
        ConversationLog::read(&self.paths(id).conversation)
    }
}

/// 对话项的追加写：模型看到什么，这里就写什么（恢复时原样喂回去）。
#[derive(Debug)]
pub struct ConversationLog {
    file: File,
    path: PathBuf,
}

impl ConversationLog {
    /// 打开（必要时建父目录），追加写。
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, Error> {
        let path = path.into();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Self { file, path })
    }

    /// 追加一条对话项并立即 flush。
    pub fn append(&mut self, item: &jh_openai::InputItem) -> Result<(), Error> {
        let line = serde_json::to_string(item)?;
        self.file.write_all(line.as_bytes())?;
        self.file.write_all(b"\n")?;
        self.file.flush()?;
        Ok(())
    }

    /// 文件路径（上屏与回报里用）。
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 读回对话项；空行跳过，坏行显式失败（宁可报错，也不要带着残缺上下文继续对话）。
    pub fn read(path: &Path) -> Result<Vec<jh_openai::InputItem>, Error> {
        let file = File::open(path).map_err(|error| {
            Error::Io(std::io::Error::new(
                error.kind(),
                format!("对话记录不可读（{}）：{error}", path.display()),
            ))
        })?;
        let mut items = Vec::new();
        for (index, line) in BufReader::new(file).lines().enumerate() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let item = serde_json::from_str(&line).map_err(|error| {
                Error::Io(std::io::Error::other(format!(
                    "对话记录第 {} 行不是合法对话项：{error}",
                    index + 1
                )))
            })?;
            items.push(item);
        }
        Ok(items)
    }
}

/// 会话列表里一行的摘要（**不含** id：选择列表把 id 单独列一列）。
pub fn label(meta: &SessionMeta) -> String {
    let when = crate::session::view::at_utc(meta.updated_at_unix_ms);
    let head = if meta.first_message.is_empty() {
        "（空会话）".to_owned()
    } else {
        meta.first_message.clone()
    };
    format!("{} 轮 · {} · {} · {}", meta.turns, meta.model, when, head)
}

/// 一行文本（id + 摘要）：文本回执用。
pub fn describe(meta: &SessionMeta) -> String {
    format!("{} · {}", meta.id, label(meta))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::test_support::TempDir;

    fn item(text: &str) -> jh_openai::InputItem {
        jh_openai::InputItem::user_text(text)
    }

    #[test]
    fn a_session_round_trips_through_the_store() {
        let dir = TempDir::new("store");
        let store = SessionStore::at(dir.join("home"));

        let paths = store
            .start("run-1", "/tmp/ws", "deepseek-flash", "jev")
            .expect("开会话");
        assert!(paths.dir.is_dir());

        let mut log = ConversationLog::open(&paths.conversation).expect("开对话记录");
        log.append(&item("第一句")).expect("写对话");
        log.append(&jh_openai::InputItem::assistant_text("回一句"))
            .expect("写对话");

        store
            .write_meta(&SessionMeta {
                id: "run-1".to_owned(),
                created_at_unix_ms: 1,
                updated_at_unix_ms: 2,
                workspace: "/tmp/ws".to_owned(),
                model: "deepseek-flash".to_owned(),
                judge: "jev".to_owned(),
                turns: 1,
                first_message: "第一句".to_owned(),
                last_message: "第一句".to_owned(),
            })
            .expect("写摘要");

        let list = store.list().expect("列会话");
        assert!(list.skipped.is_empty(), "{:?}", list.skipped);
        assert_eq!(list.metas.len(), 1);
        assert_eq!(list.metas[0].id, "run-1");
        assert_eq!(list.metas[0].turns, 1);

        let restored = store.read_conversation("run-1").expect("读对话");
        assert_eq!(
            restored,
            vec![
                item("第一句"),
                jh_openai::InputItem::assistant_text("回一句")
            ]
        );
    }

    #[test]
    fn listing_is_newest_first_and_empty_without_any_session() {
        let dir = TempDir::new("store-list");
        let store = SessionStore::at(dir.join("home"));
        let empty = store.list().expect("没有会话时是空列表");
        assert!(empty.metas.is_empty());
        assert!(empty.skipped.is_empty());

        for (id, updated) in [("old", 1u64), ("new", 3), ("mid", 2)] {
            store.start(id, "/w", "m", "j").expect("开会话");
            let mut meta = store.read_meta(id).expect("读摘要");
            meta.updated_at_unix_ms = updated;
            store.write_meta(&meta).expect("写摘要");
        }
        let ids: Vec<String> = store
            .list()
            .expect("列会话")
            .metas
            .into_iter()
            .map(|meta| meta.id)
            .collect();
        assert_eq!(ids, vec!["new", "mid", "old"]);
    }

    /// 坏 `meta.json` 的会话不能被静默丢掉：要连同原因一起回报。
    #[test]
    fn a_session_with_a_broken_summary_is_reported_not_dropped() {
        let dir = TempDir::new("store-broken-meta");
        let store = SessionStore::at(dir.join("home"));
        store.start("good", "/w", "m", "j").expect("好会话");
        let bad = store.paths("bad");
        std::fs::create_dir_all(&bad.dir).expect("建目录");
        std::fs::write(&bad.meta, "{ 不是 JSON").expect("写坏摘要");

        let list = store.list().expect("列会话");
        assert_eq!(list.metas.len(), 1, "好会话照常列出");
        assert_eq!(list.skipped.len(), 1, "坏会话要报出来：{:?}", list.skipped);
        assert_eq!(list.skipped[0].0, "bad");
        assert!(!list.skipped[0].1.is_empty(), "要带原因");
    }

    /// 本地文件问题不许报成「判断器不可用」——真调里踩过，前缀会把人带偏。
    #[test]
    fn local_file_failures_do_not_blame_the_judge() {
        let dir = TempDir::new("store-local-error");
        let store = SessionStore::at(dir.join("home"));
        let missing = store.read_meta("没有这个会话").expect_err("读不到要报错");
        let text = missing.to_string();
        assert!(text.contains("本地读写失败"), "{text}");
        assert!(
            !text.contains("判断器"),
            "本地文件问题不该甩锅判断器：{text}"
        );
        assert!(text.contains("会话摘要不可读"), "{text}");
    }

    #[test]
    fn a_broken_conversation_line_fails_loudly() {
        let dir = TempDir::new("store-broken");
        let path = dir.join("conversation.jsonl");
        std::fs::write(&path, "{\"type\":\"message\"}\n").expect("写坏行");
        let error = ConversationLog::read(&path).expect_err("坏行要显式失败");
        assert!(error.to_string().contains("对话记录第 1 行"), "{error}");
    }
}
