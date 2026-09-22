//! 判断器接口：`state + 类型化问题 → 类型化答案 + 概率 + usage + 显式错误`。

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;

use crate::answer::{Answer, Usage};
use crate::error::Error;
use crate::question::{QuestionId, QuestionSpec, Questions};
use crate::record::JudgeSource;
use crate::state::{SnapshotId, State};

/// 一次判断请求。
#[derive(Debug, Clone, PartialEq)]
pub struct JudgeRequest {
    /// 输入材料的快照标识；判断器不必解读它，它随请求一起进判断记录。
    pub snapshot: SnapshotId,
    /// 交给判断器的材料。
    pub state: State,
    /// 问题集：一次请求放 1 个还是 N 个问题由调用方决定。
    pub questions: Questions,
}

/// 一次判断调用的结果。
///
/// 来源与版本跟着内容一起流动：M0 的退出证据要求能区分真实 API 与模拟运行
/// （`docs/roadmap-1.0.md` §5 M0），所以「谁判的」写在结果里，而不是靠调用方事后回忆。
#[derive(Debug, Clone, PartialEq)]
pub struct JudgeResponse {
    /// 这次判断由哪一类判断器做出。
    pub source: JudgeSource,
    /// 实际执行者的版本：真实通道填响应里的模型 ID（如 `jev-1.13.0`），
    /// 规则 / 录制 / 模拟器填各自的版本串。
    pub version: String,
    /// 每个问题的答案，键与请求里的问题 ID 对应。
    pub answers: BTreeMap<QuestionId, Answer>,
    /// token 用量；通道未上报或本地判断器时为空。
    pub usage: Option<Usage>,
    /// 通道侧请求 id（如 Jev 的 `x-typesafe-request-id`）；本地判断器时为空。
    pub request_id: Option<String>,
}

impl JudgeResponse {
    /// 检查响应是否满足判断器契约：请求里的每个问题都恰有一个类型匹配的答案，且没有多余答案。
    ///
    /// 这是「错误显式失败」的检查入口：做不到就返回 [`Error::InvalidResponse`]，不产出部分结果。
    /// 分布本身的形状（概率是否和为 1 等）不在这里校验——那是通道的契约问题。
    pub fn validate(&self, request: &JudgeRequest) -> Result<(), Error> {
        self.checked_answers(request).map(|_| ())
    }

    /// [`Self::validate`] 的实现：顺带把答案按请求的顺序排好，供构造判断记录使用。
    pub(crate) fn checked_answers<'a>(
        &'a self,
        request: &'a JudgeRequest,
    ) -> Result<Vec<(&'a QuestionId, &'a QuestionSpec, &'a Answer)>, Error> {
        let mut checked = Vec::with_capacity(request.questions.len());
        for (id, spec) in &request.questions {
            let Some(answer) = self.answers.get(id) else {
                return Err(Error::InvalidResponse(format!("问题 `{id}` 没有答案")));
            };
            if !answer.matches(&spec.question) {
                return Err(Error::InvalidResponse(format!(
                    "问题 `{id}` 的答案原语与问题不符"
                )));
            }
            checked.push((id, spec, answer));
        }
        for id in self.answers.keys() {
            if !request.questions.contains_key(id) {
                return Err(Error::InvalidResponse(format!(
                    "答案 `{id}` 不在请求的问题集里"
                )));
            }
        }
        Ok(checked)
    }
}

/// 判断器：把一份 state 上的类型化问题判成类型化答案。
///
/// # 契约
///
/// - 输入 `state + 类型化问题`，输出 `类型化答案 + 概率/置信度 + usage`，或显式 [`Error`]；
/// - **批量与逐问是同一个接口**：请求里放 1 个还是 N 个问题由调用方决定
///   （`docs/roadmap-1.0.md` §3.2 要求两种都能表达）；
/// - **不重试、不降级、不兜底**：失败原样返回错误（`docs/roadmap-1.0.md` §8 #9）；
///   「降级 = 换后端」指的是换一个 [`Judge`] 实现，不是实现内部的备用路径；
/// - **不塌缩概率**：`probabilities` / `confidence` / `noul` 原值透传，是否按阈值折成行动由消费方决定
///   （`docs/jev-research.md` §13.3.3）；
/// - 实现必须保证请求里的每个问题都有一个类型匹配的答案：
///   [`JudgeResponse::validate`] 是这条契约的检查入口。
///
/// # 为什么返回装箱 future 而不是 `async fn`
///
/// 判断器要在运行时切换（Jev / LLM / 规则 / 录制 / 模拟真值），接口必须能放进 `Box<dyn Judge>`；
/// 带 `async fn` 或 RPITIT 的 trait 不是 dyn 兼容的。本 crate 不引入异步运行时，future 由调用方
/// 自己的 executor 驱动。
///
/// # 实现示例
///
/// ```
/// use std::collections::BTreeMap;
/// use std::future::Future;
/// use std::pin::Pin;
///
/// use jh_core::{
///     Answer, Error, Judge, JudgeRequest, JudgeResponse, JudgeSource, NoulAnswer, Question,
/// };
///
/// /// 只支持 Noul 的规则判断器。
/// struct AllYes;
///
/// impl Judge for AllYes {
///     fn judge<'a>(
///         &'a self,
///         request: &'a JudgeRequest,
///     ) -> Pin<Box<dyn Future<Output = Result<JudgeResponse, Error>> + Send + 'a>> {
///         Box::pin(async move {
///             let mut answers = BTreeMap::new();
///             for (id, spec) in &request.questions {
///                 let Question::Noul(_) = &spec.question else {
///                     // 不支持的原语显式失败，不做兜底。
///                     return Err(Error::Unavailable("规则判断器只支持 Noul".to_owned()));
///                 };
///                 answers.insert(id.clone(), Answer::Noul(NoulAnswer { noul: 1.0 }));
///             }
///             Ok(JudgeResponse {
///                 source: JudgeSource::Rule,
///                 version: "all-yes-v1".to_owned(),
///                 answers,
///                 usage: None,
///                 request_id: None,
///             })
///         })
///     }
/// }
/// ```
pub trait Judge {
    /// 判定一份请求。
    fn judge<'a>(
        &'a self,
        request: &'a JudgeRequest,
    ) -> Pin<Box<dyn Future<Output = Result<JudgeResponse, Error>> + Send + 'a>>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::answer::{ChoiceAnswer, NoulAnswer, ScoreAnswer};
    use crate::question::{ChoiceQuestion, NoulQuestion, Question, QuestionSpec, QuestionVersion};
    use crate::state::State;

    fn request() -> JudgeRequest {
        JudgeRequest {
            snapshot: SnapshotId::from("case-001"),
            state: State::from("取消已确认"),
            questions: Questions::from([
                (
                    QuestionId::from("noul_question"),
                    QuestionSpec {
                        version: QuestionVersion(1),
                        question: Question::Noul(NoulQuestion {
                            instructions: "？".to_owned(),
                            criteria: None,
                        }),
                    },
                ),
                (
                    QuestionId::from("choice_question"),
                    QuestionSpec {
                        version: QuestionVersion(1),
                        question: Question::Choice(ChoiceQuestion {
                            instructions: "？".to_owned(),
                            criteria: BTreeMap::from([("a".to_owned(), None)]),
                        }),
                    },
                ),
            ]),
        }
    }

    fn response(answers: BTreeMap<QuestionId, Answer>) -> JudgeResponse {
        JudgeResponse {
            source: JudgeSource::Rule,
            version: "test-v1".to_owned(),
            answers,
            usage: None,
            request_id: None,
        }
    }

    #[test]
    fn validate_accepts_one_matching_answer_per_question() {
        let request = request();
        let answers = BTreeMap::from([
            (
                QuestionId::from("noul_question"),
                Answer::Noul(NoulAnswer { noul: 0.42 }),
            ),
            (
                QuestionId::from("choice_question"),
                Answer::Choice(ChoiceAnswer {
                    choice: "a".to_owned(),
                    probabilities: BTreeMap::from([("a".to_owned(), 1.0)]),
                    confidence: 1.0,
                }),
            ),
        ]);

        assert_eq!(response(answers).validate(&request), Ok(()));
    }

    #[test]
    fn validate_names_the_question_whose_answer_is_missing() {
        let request = request();
        let answers = BTreeMap::from([(
            QuestionId::from("noul_question"),
            Answer::Noul(NoulAnswer { noul: 0.42 }),
        )]);

        let error = response(answers)
            .validate(&request)
            .expect_err("缺答案必须显式失败");
        assert!(matches!(error, Error::InvalidResponse(_)), "{error}");
        assert!(error.to_string().contains("choice_question"), "{error}");
    }

    #[test]
    fn validate_rejects_answers_of_the_wrong_primitive() {
        let request = request();
        let answers = BTreeMap::from([
            (
                QuestionId::from("noul_question"),
                // Noul 位置放了 Score 答案：类型不符，不能当成「空答案」放过。
                Answer::Score(ScoreAnswer {
                    score: 1.0,
                    legend: BTreeMap::from([(0, "低".to_owned()), (1, "高".to_owned())]),
                    probabilities: BTreeMap::from([(0, 0.0), (1, 1.0)]),
                    confidence: 1.0,
                }),
            ),
            (
                QuestionId::from("choice_question"),
                Answer::Choice(ChoiceAnswer {
                    choice: "a".to_owned(),
                    probabilities: BTreeMap::new(),
                    confidence: 1.0,
                }),
            ),
        ]);

        let error = response(answers)
            .validate(&request)
            .expect_err("原语不符必须显式失败");
        assert!(error.to_string().contains("noul_question"), "{error}");
    }

    #[test]
    fn validate_rejects_answers_outside_the_request() {
        let request = request();
        let answers = BTreeMap::from([
            (
                QuestionId::from("noul_question"),
                Answer::Noul(NoulAnswer { noul: 0.42 }),
            ),
            (
                QuestionId::from("choice_question"),
                Answer::Choice(ChoiceAnswer {
                    choice: "a".to_owned(),
                    probabilities: BTreeMap::new(),
                    confidence: 1.0,
                }),
            ),
            (
                QuestionId::from("never_asked"),
                Answer::Noul(NoulAnswer { noul: 1.0 }),
            ),
        ]);

        let error = response(answers)
            .validate(&request)
            .expect_err("多余答案必须显式失败");
        assert!(error.to_string().contains("never_asked"), "{error}");
    }
}
