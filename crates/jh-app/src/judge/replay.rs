//! 录制回放判断器：复用已落盘的判断记录，不发起任何调用。
//!
//! # 复用条件
//!
//! 回放索引的键是 `(快照标识, 问题 ID, 问题版本)`，三者全等才复用（`docs/RESEARCH_GOALS.md` §8.4）：
//! 快照绑定输入材料，问题版本绑定问题含义。索引在**构造时**一次性读入，
//! 因此同一次运行里新写入的记录不会被自己回放（回放不是自我实现的预言）。
//!
//! # 版本与来源
//!
//! - `source` 一律是 [`jh_core::JudgeSource::Replay`]：这是回放，不是本次运行的真实调用；
//! - `version` 保留**原记录的执行者版本**（例如 `jev-1.13.0` 或 `rules-v1`），
//!   因为答案的语义来自那个版本，回放只是搬运；
//! - `usage` 与 `request_id` 留空：本次运行既没有花 token，也没有发出请求。

use std::collections::BTreeMap;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;

use jh_core::{Error, Judge, JudgeRequest, JudgeResponse, JudgeSource};

use crate::calls::{ReplayIndex, read_entries, replay_index};

/// 录制回放判断器。
pub struct ReplayJudge {
    index: ReplayIndex,
}

impl ReplayJudge {
    /// 读入调用记录并建索引。
    ///
    /// 文件不存在、内容不可解析时显式失败（[`jh_core::Error::Unavailable`]），
    /// 不用别的判断器的结果顶上。
    pub fn load(path: &Path) -> Result<Self, Error> {
        if !path.exists() {
            return Err(Error::Unavailable(format!(
                "回放记录不存在：{}",
                path.display()
            )));
        }
        let entries = read_entries(path).map_err(|error| match error {
            crate::Error::Judge(error) => error,
            other => Error::Unavailable(other.to_string()),
        })?;
        Ok(Self {
            index: replay_index(&entries),
        })
    }

    /// 用现成索引构造（测试与复用场景）。
    pub fn with_index(index: ReplayIndex) -> Self {
        Self { index }
    }
}

impl Judge for ReplayJudge {
    fn judge<'a>(
        &'a self,
        request: &'a JudgeRequest,
    ) -> Pin<Box<dyn Future<Output = Result<JudgeResponse, Error>> + Send + 'a>> {
        Box::pin(async move {
            let mut answers = BTreeMap::new();
            let mut versions: Vec<String> = Vec::new();

            for (id, spec) in &request.questions {
                let key = (request.snapshot.clone(), id.clone(), spec.version);
                let (version, answer) = self.index.get(&key).ok_or_else(|| {
                    Error::Unavailable(format!(
                        "没有可回放的记录：快照 `{}`、问题 `{id}`（版本 {}）",
                        request.snapshot, spec.version
                    ))
                })?;
                versions.push(version.clone());
                answers.insert(id.clone(), answer.clone());
            }

            // 一次请求里的问题可以有不同来源版本（例如分批录制的记录），逐题保留、
            // 汇总成一行多版本串；只有一条时就等于那条记录的版本。
            versions.sort();
            versions.dedup();
            let version = versions.join(" + ");

            Ok(JudgeResponse {
                source: JudgeSource::Replay,
                version,
                answers,
                usage: None,
                request_id: None,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jh_core::{
        Answer, JudgeRequest, NoulAnswer, NoulQuestion, Question, QuestionId, QuestionSpec,
        QuestionVersion, SnapshotId, State,
    };

    fn block_on<F: Future>(future: F) -> F::Output {
        use std::pin::pin;
        use std::task::{Context, Poll, Waker};

        let mut future = pin!(future);
        let mut context = Context::from_waker(Waker::noop());
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => output,
            Poll::Pending => panic!("回放判断器不应挂起"),
        }
    }

    fn request(version: QuestionVersion) -> JudgeRequest {
        JudgeRequest {
            snapshot: SnapshotId::from("case-001"),
            state: State::from("取消确认落库"),
            questions: BTreeMap::from([(
                QuestionId::from("needs_new_observation"),
                QuestionSpec {
                    version,
                    question: Question::Noul(NoulQuestion {
                        instructions: "？".to_owned(),
                        criteria: None,
                    }),
                },
            )]),
        }
    }

    fn judge(version: QuestionVersion) -> ReplayJudge {
        ReplayJudge::with_index(BTreeMap::from([(
            (
                SnapshotId::from("case-001"),
                QuestionId::from("needs_new_observation"),
                version,
            ),
            (
                "jev-1.13.0".to_owned(),
                Answer::Noul(NoulAnswer { noul: 0.12 }),
            ),
        )]))
    }

    #[test]
    fn replay_returns_the_recorded_answer_with_its_original_version() {
        let request = request(QuestionVersion(2));
        let response = block_on(judge(QuestionVersion(2)).judge(&request)).expect("有记录应能回放");

        assert_eq!(response.source, JudgeSource::Replay);
        assert_eq!(response.version, "jev-1.13.0");
        assert_eq!(
            response.answers[&QuestionId::from("needs_new_observation")],
            Answer::Noul(NoulAnswer { noul: 0.12 })
        );
        assert_eq!(response.usage, None, "回放不产生 token 用量");
        assert_eq!(response.request_id, None, "回放没有发出请求");
        response.validate(&request).expect("响应满足判断器契约");
    }

    #[test]
    fn a_different_question_version_misses_the_record() {
        let request = request(QuestionVersion(3));
        let error =
            block_on(judge(QuestionVersion(2)).judge(&request)).expect_err("版本不同必须显式失败");

        assert!(matches!(error, Error::Unavailable(_)), "{error}");
        assert!(error.to_string().contains("版本 3"), "{error}");
    }

    #[test]
    fn loading_a_missing_file_fails_explicitly() {
        let path = std::env::temp_dir().join("jh-app-replay-never-written.jsonl");
        let Err(error) = ReplayJudge::load(&path) else {
            panic!("不存在的记录必须显式失败");
        };
        assert!(matches!(error, Error::Unavailable(_)), "{error}");
        assert!(error.to_string().contains("回放记录不存在"), "{error}");
    }
}
