//! 模拟真值判断器：直接回答任务集的隐藏真值。
//!
//! **只用于诊断与上限估计**（`docs/RESEARCH_GOALS.md` §9.2、§10.2）：它知道真值，因此它的读数
//! 是「判断层不出错时能到什么水平」的上界，不代表任何真实通道的行为。记录与报告靠
//! [`jh_core::JudgeSource::SimulatedTruth`] 标签把它与真实调用区分开，禁止用它冒充实测。
//!
//! 答案分布是 one-hot（概率 1）：它给的不是判断，而是真值本身。
//! 真值里没有的快照或问题一律显式失败——它只对自己那份任务集有效。

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;

use jh_core::{Error, Judge, JudgeRequest, JudgeResponse, JudgeSource, SnapshotId};

use crate::tasks::TruthMap;

/// 模拟真值判断器的版本串。
pub const VERSION: &str = "simulated-truth-v1";

/// 按快照回答真值的判断器。
pub struct SimulatedTruthJudge {
    truth: BTreeMap<SnapshotId, TruthMap>,
}

impl SimulatedTruthJudge {
    /// 用任务集真值表构造（见 [`crate::tasks::TaskSet::truth_map`]）。
    pub fn new(truth: BTreeMap<SnapshotId, TruthMap>) -> Self {
        Self { truth }
    }
}

impl Judge for SimulatedTruthJudge {
    fn judge<'a>(
        &'a self,
        request: &'a JudgeRequest,
    ) -> Pin<Box<dyn Future<Output = Result<JudgeResponse, Error>> + Send + 'a>> {
        Box::pin(async move {
            let truth = self.truth.get(&request.snapshot).ok_or_else(|| {
                Error::Unavailable(format!("真值表里没有快照 `{}`", request.snapshot))
            })?;

            let mut answers = BTreeMap::new();
            for (id, spec) in &request.questions {
                let truth = truth.get(id).ok_or_else(|| {
                    Error::Unavailable(format!(
                        "快照 `{}` 没有问题 `{id}` 的真值",
                        request.snapshot
                    ))
                })?;
                answers.insert(id.clone(), truth.render(&spec.question)?);
            }

            Ok(JudgeResponse {
                source: JudgeSource::SimulatedTruth,
                version: VERSION.to_owned(),
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
    use crate::tasks::Truth;
    use jh_core::{
        Answer, ChoiceQuestion, Question, QuestionId, QuestionSpec, QuestionVersion, State,
    };

    fn block_on<F: Future>(future: F) -> F::Output {
        use std::pin::pin;
        use std::task::{Context, Poll, Waker};

        let mut future = pin!(future);
        let mut context = Context::from_waker(Waker::noop());
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => output,
            Poll::Pending => panic!("模拟真值判断器不应挂起"),
        }
    }

    fn request() -> JudgeRequest {
        JudgeRequest {
            snapshot: SnapshotId::from("case-001"),
            state: State::from("09:00:01 取消确认落库\n09:00:05 缓存写入"),
            questions: BTreeMap::from([(
                QuestionId::from("failure_kind"),
                QuestionSpec {
                    version: QuestionVersion(1),
                    question: Question::Choice(ChoiceQuestion {
                        instructions: "哪一类原因？".to_owned(),
                        criteria: BTreeMap::from([
                            ("cache_stale".to_owned(), None),
                            ("other".to_owned(), None),
                        ]),
                    }),
                },
            )]),
        }
    }

    #[test]
    fn answers_are_the_hidden_truth_with_the_simulated_source_label() {
        let judge = SimulatedTruthJudge::new(BTreeMap::from([(
            SnapshotId::from("case-001"),
            BTreeMap::from([(
                QuestionId::from("failure_kind"),
                Truth::Choice {
                    label: "cache_stale".to_owned(),
                },
            )]),
        )]));

        let request = request();
        let response = block_on(judge.judge(&request)).expect("真值表里有这个快照");
        response.validate(&request).expect("响应满足判断器契约");

        assert_eq!(response.source, JudgeSource::SimulatedTruth);
        assert_eq!(response.version, VERSION);
        assert_eq!(response.usage, None);
        assert_eq!(response.request_id, None);
        assert_eq!(
            response.answers[&QuestionId::from("failure_kind")],
            Answer::Choice(jh_core::ChoiceAnswer {
                choice: "cache_stale".to_owned(),
                probabilities: BTreeMap::from([("cache_stale".to_owned(), 1.0)]),
                confidence: 1.0,
            })
        );
    }

    #[test]
    fn an_unknown_snapshot_fails_explicitly() {
        let judge = SimulatedTruthJudge::new(BTreeMap::new());
        let error = block_on(judge.judge(&request())).expect_err("没有真值必须显式失败");

        assert!(matches!(error, Error::Unavailable(_)), "{error}");
        assert!(error.to_string().contains("case-001"), "{error}");
    }

    #[test]
    fn a_question_without_truth_fails_explicitly() {
        let judge = SimulatedTruthJudge::new(BTreeMap::from([(
            SnapshotId::from("case-001"),
            BTreeMap::new(),
        )]));
        let error = block_on(judge.judge(&request())).expect_err("缺真值必须显式失败");

        assert!(error.to_string().contains("failure_kind"), "{error}");
    }
}
