//! 调用记录落盘：`var/` 下的 JSONL，一行一次判断调用。
//!
//! 每行带 `kind` 判别标签，两种形态共用一套字段位置：
//!
//! - `judgment`：判断成功，`record` 是 [`JudgmentRecord`]——来源标签、执行者版本、`request_id`、
//!   `usage`、延迟与逐题答案都在里面（`docs/RESEARCH_GOALS.md` §5.2）；
//! - `failure`：判断失败，记下声明的来源标签、延迟与错误文本；**没有** `record`，
//!   也不写任何答案——失败就是失败，不用空答案顶替。
//!
//! 真实与模拟靠 `source` 标签区分（`real_api` / `llm` / `rule` / `replay` / `simulated_truth`），
//! 文件里没有任何凭据：key 只在环境变量里。

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use jh_core::{JudgeSource, JudgmentRecord, QuestionId, QuestionVersion, SnapshotId};
use serde::{Deserialize, Serialize};

use crate::error::Error;

/// 调用记录的一行。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CallEntry {
    /// 判断成功：整条 [`JudgmentRecord`] 原样落盘。
    Judgment {
        /// 本次运行标识。
        run: String,
        /// 样本标识。
        case: String,
        /// 判断记录。
        record: Box<JudgmentRecord>,
    },
    /// 判断失败：只有来源、延迟与错误。
    Failure {
        /// 本次运行标识。
        run: String,
        /// 样本标识。
        case: String,
        /// 该判断器声明的来源标签（失败也标明是谁失败）。
        source: JudgeSource,
        /// 本次调用耗时，毫秒（失败调用同样计时，不把失败样本排除在延迟统计外）。
        latency_ms: u64,
        /// 判断器的错误文本；通道侧的请求 id 若在错误里，会随文本一起留下。
        error: String,
    },
}

impl CallEntry {
    /// 本次运行标识。
    pub fn run(&self) -> &str {
        match self {
            Self::Judgment { run, .. } | Self::Failure { run, .. } => run,
        }
    }

    /// 样本标识。
    pub fn case(&self) -> &str {
        match self {
            Self::Judgment { case, .. } | Self::Failure { case, .. } => case,
        }
    }
}

/// 追加写的调用记录文件。
pub struct CallLog {
    file: File,
}

impl CallLog {
    /// 打开（必要时创建）调用记录文件；父目录一并创建。
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, Error> {
        let path = path.into();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Self { file })
    }

    /// 追加一行调用记录并立即 flush：中断也不会丢已完成的调用。
    pub fn append(&mut self, entry: &CallEntry) -> Result<(), Error> {
        let line = serde_json::to_string(entry)?;
        self.file.write_all(line.as_bytes())?;
        self.file.write_all(b"\n")?;
        self.file.flush()?;
        Ok(())
    }
}

/// 读回调用记录文件（回放与复核用）；空行跳过，坏行显式失败。
pub fn read_entries(path: &Path) -> Result<Vec<CallEntry>, Error> {
    let text = std::fs::read_to_string(path).map_err(|error| {
        Error::Judge(jh_core::Error::Unavailable(format!(
            "调用记录不可读（{}）：{error}",
            path.display()
        )))
    })?;

    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| {
            serde_json::from_str(line).map_err(|error| {
                Error::Judge(jh_core::Error::InvalidResponse(format!(
                    "调用记录第 {} 行无法解析：{error}",
                    index + 1
                )))
            })
        })
        .collect()
}

/// 回放索引：`(快照, 问题, 问题版本)` → 记录下来的答案与当时的执行者版本。
///
/// 复用条件是三者全等（`docs/RESEARCH_GOALS.md` §8.4）：快照标识绑定输入材料，
/// 问题版本绑定问题含义，执行者版本随答案一起保留——回放不重新判定，只在条件全等时复用。
pub type ReplayIndex =
    BTreeMap<(SnapshotId, QuestionId, QuestionVersion), (String, jh_core::Answer)>;

/// 从调用记录里挑出可回放的判断，按出现顺序索引；同一键后出现的记录覆盖先前的。
pub fn replay_index(entries: &[CallEntry]) -> ReplayIndex {
    let mut index = ReplayIndex::new();
    for entry in entries {
        let CallEntry::Judgment { record, .. } = entry else {
            continue;
        };
        for (question, judgment) in &record.judgments {
            index.insert(
                (
                    record.snapshot.clone(),
                    question.clone(),
                    judgment.question_version,
                ),
                (record.version.clone(), judgment.answer.clone()),
            );
        }
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;
    use jh_core::{Answer, NoulAnswer, NoulQuestion, Question, QuestionSpec, Questions, State};
    use std::time::Duration;

    fn unique_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jh-app-calls-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).expect("临时目录应能创建");
        dir
    }

    fn record() -> JudgmentRecord {
        let request = jh_core::JudgeRequest {
            snapshot: SnapshotId::from("case-001"),
            state: State::from("取消确认落库"),
            questions: Questions::from([(
                QuestionId::from("needs_new_observation"),
                QuestionSpec {
                    version: QuestionVersion(2),
                    question: Question::Noul(NoulQuestion {
                        instructions: "？".to_owned(),
                        criteria: None,
                    }),
                },
            )]),
        };
        let response = jh_core::JudgeResponse {
            source: JudgeSource::Rule,
            version: "rules-v1".to_owned(),
            answers: BTreeMap::from([(
                QuestionId::from("needs_new_observation"),
                Answer::Noul(NoulAnswer { noul: 0.1 }),
            )]),
            usage: None,
            request_id: None,
        };
        JudgmentRecord::from_response(&request, &response, Duration::from_millis(3))
            .expect("响应满足契约")
    }

    #[test]
    fn entries_round_trip_through_jsonl() {
        let dir = unique_dir("round-trip");
        let path = dir.join("calls.jsonl");
        let mut log = CallLog::open(&path).expect("应能打开调用记录");

        log.append(&CallEntry::Judgment {
            run: "run-1".to_owned(),
            case: "case-001".to_owned(),
            record: Box::new(record()),
        })
        .expect("应能追加判断记录");
        log.append(&CallEntry::Failure {
            run: "run-1".to_owned(),
            case: "case-002".to_owned(),
            source: JudgeSource::RealApi,
            latency_ms: 41,
            error: "判断调用失败：HTTP 401".to_owned(),
        })
        .expect("应能追加失败记录");
        drop(log);

        let entries = read_entries(&path).expect("应能读回记录");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].run(), "run-1");
        assert_eq!(entries[1].case(), "case-002");

        let wire = serde_json::to_value(&entries[1]).unwrap();
        assert_eq!(wire["kind"], serde_json::json!("failure"));
        assert_eq!(wire["source"], serde_json::json!("real_api"));
        assert!(wire.get("record").is_none(), "失败行不应带判断记录");

        let wire = serde_json::to_value(&entries[0]).unwrap();
        assert_eq!(wire["kind"], serde_json::json!("judgment"));
        assert_eq!(
            wire["record"]["judgments"]["needs_new_observation"]["question_version"],
            serde_json::json!(2)
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn replay_index_keeps_the_latest_answer_and_its_version() {
        let mut first = record();
        first.version = "rules-v1".to_owned();
        let mut second = record();
        second.version = "jev-1.13.0".to_owned();
        second
            .judgments
            .get_mut(&QuestionId::from("needs_new_observation"))
            .expect("该问题应在记录里")
            .answer = Answer::Noul(NoulAnswer { noul: 0.9 });

        let entries = vec![
            CallEntry::Judgment {
                run: "run-1".to_owned(),
                case: "case-001".to_owned(),
                record: Box::new(first),
            },
            CallEntry::Failure {
                run: "run-1".to_owned(),
                case: "case-001".to_owned(),
                source: JudgeSource::RealApi,
                latency_ms: 12,
                error: "失败".to_owned(),
            },
            CallEntry::Judgment {
                run: "run-2".to_owned(),
                case: "case-001".to_owned(),
                record: Box::new(second),
            },
        ];

        let index = replay_index(&entries);
        let key = (
            SnapshotId::from("case-001"),
            QuestionId::from("needs_new_observation"),
            QuestionVersion(2),
        );
        let (version, answer) = index.get(&key).expect("末条记录应进索引");
        assert_eq!(version, "jev-1.13.0");
        assert_eq!(answer, &Answer::Noul(NoulAnswer { noul: 0.9 }));
    }

    #[test]
    fn unreadable_and_malformed_records_fail_explicitly() {
        let dir = unique_dir("broken");
        let missing = dir.join("never-written.jsonl");
        let error = read_entries(&missing).expect_err("不存在的记录必须显式失败");
        assert!(error.to_string().contains("调用记录不可读"), "{error}");

        let path = dir.join("broken.jsonl");
        std::fs::write(&path, "{\"kind\":\"judgment\"}\n").expect("应能写入坏行");
        let error = read_entries(&path).expect_err("坏行必须显式失败");
        assert!(error.to_string().contains("第 1 行"), "{error}");

        std::fs::remove_dir_all(&dir).ok();
    }
}
