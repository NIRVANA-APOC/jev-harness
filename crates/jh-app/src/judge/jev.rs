//! Jev 真调适配：`jh-core` 的请求 / 响应与 `jh-jev` 的 wire 类型一一转换。
//!
//! - 模型 pin 到 [`jh_jev::DEFAULT_MODEL`]（`jev-1.13.0`），模型版本随响应回填进记录；
//! - `version` 取响应的 `model`，`request_id` 取响应头 `x-typesafe-request-id`，
//!   `usage` 只在通道上报时写入（全空时留空，不编造 0）；
//! - 服务端返回未知答案原语（`jh_jev::Answer::Unknown`）时显式失败，不当作空答案放过；
//! - 不重试、不降级、不兜底（`docs/roadmap-1.0.md` §8）：错误原样映射成
//!   [`jh_core::Error`] 的两个通道类别。

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;

use jh_core::{
    Answer, ChoiceAnswer, Error, Judge, JudgeRequest, JudgeResponse, JudgeSource, NoulAnswer,
    Question, QuestionId, Questions, ScoreAnswer, State, StateEntry, Usage,
};

/// 调 `POST {base_url}/v1/systemone` 的判断器。
pub struct JevJudge {
    client: jh_jev::Client,
}

impl JevJudge {
    /// 从环境变量构造：`TYPESAFE_API_KEY` 必填，`TYPESAFE_BASE_URL` 可选（默认官方端点）。
    pub fn from_env() -> Result<Self, Error> {
        let client = jh_jev::Client::from_env().map_err(map_error)?;
        Ok(Self { client })
    }
}

impl Judge for JevJudge {
    fn judge<'a>(
        &'a self,
        request: &'a JudgeRequest,
    ) -> Pin<Box<dyn Future<Output = Result<JudgeResponse, Error>> + Send + 'a>> {
        Box::pin(async move {
            let body =
                jh_jev::SystemOneRequest::new(state(&request.state), questions(&request.questions));
            let response = self.client.system_one(&body).await.map_err(map_error)?;

            Ok(JudgeResponse {
                source: JudgeSource::RealApi,
                version: response.model.clone(),
                answers: answers(&response)?,
                usage: usage(&response.usage),
                request_id: response.request_id.clone(),
            })
        })
    }
}

/// 把通道错误折进判断器的三类显式错误：缺凭据是「不可用」，其余都是「调用失败」。
///
/// `jh_jev::Error` 的 `Display` 已经带上 HTTP 状态码与响应头里的请求 id（若有），
/// 因此失败记录里仍能对账到服务端的某次请求。
fn map_error(error: jh_jev::Error) -> Error {
    match error {
        jh_jev::Error::MissingApiKey => Error::Unavailable("缺少 TYPESAFE_API_KEY".to_owned()),
        other => Error::CallFailed(other.to_string()),
    }
}

/// 判断材料与 `jh_jev::State` 的三形态 1:1 对应。
fn state(state: &State) -> jh_jev::State {
    match state {
        State::Text(text) => jh_jev::State::Text(text.clone()),
        State::Object(fields) => jh_jev::State::Object(
            fields
                .iter()
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect(),
        ),
        State::Records(entries) => jh_jev::State::Messages(entries.iter().map(entry).collect()),
    }
}

fn entry(entry: &StateEntry) -> jh_jev::StateEntry {
    match entry {
        StateEntry::Text(text) => jh_jev::StateEntry::Text(text.clone()),
        StateEntry::Record(fields) => jh_jev::StateEntry::Record(
            fields
                .iter()
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect(),
        ),
    }
}

/// 问题集中每个问题的定义与 wire 形态一一对应；问题 ID 原样作为 key。
fn questions(questions: &Questions) -> jh_jev::Questions {
    questions
        .iter()
        .map(|(id, spec)| (id.as_str().to_owned(), question(&spec.question)))
        .collect()
}

/// 三类原语逐一对应；`jh_jev::Question::Unknown` 只为反序列化旧数据而存在，不会由这里产生。
fn question(question: &Question) -> jh_jev::Question {
    match question {
        Question::Choice(spec) => jh_jev::Question::Choice(jh_jev::ChoiceQuestion {
            instructions: jh_jev::Instructions::Text(spec.instructions.clone()),
            criteria: spec
                .criteria
                .iter()
                .map(|(option, description)| {
                    (
                        option.clone(),
                        description.clone().map(jh_jev::Instructions::Text),
                    )
                })
                .collect(),
        }),
        Question::Score(spec) => jh_jev::Question::Score(jh_jev::ScoreQuestion {
            instructions: jh_jev::Instructions::Text(spec.instructions.clone()),
            criteria: spec
                .criteria
                .iter()
                .cloned()
                .map(jh_jev::Instructions::Text)
                .collect(),
        }),
        Question::Noul(spec) => jh_jev::Question::Noul(jh_jev::NoulQuestion {
            instructions: jh_jev::Instructions::Text(spec.instructions.clone()),
            criteria: spec.criteria.as_ref().map(|criteria| jh_jev::NoulCriteria {
                r#true: criteria.r#true.clone().map(jh_jev::Instructions::Text),
                r#false: criteria.r#false.clone().map(jh_jev::Instructions::Text),
            }),
        }),
    }
}

/// 答案逐一对应；未知原语显式失败。
fn answers(response: &jh_jev::SystemOneResponse) -> Result<BTreeMap<QuestionId, Answer>, Error> {
    response
        .answers
        .iter()
        .map(|(id, answer)| {
            let answer = match answer {
                jh_jev::Answer::Choice(answer) => Answer::Choice(ChoiceAnswer {
                    choice: answer.choice.clone(),
                    probabilities: answer.probabilities.clone(),
                    confidence: answer.confidence,
                }),
                jh_jev::Answer::Score(answer) => Answer::Score(ScoreAnswer {
                    score: answer.score,
                    legend: answer.legend.clone(),
                    probabilities: answer.probabilities.clone(),
                    confidence: answer.confidence,
                }),
                jh_jev::Answer::Noul(answer) => Answer::Noul(NoulAnswer { noul: answer.noul }),
                jh_jev::Answer::Unknown => {
                    return Err(Error::InvalidResponse(format!(
                        "问题 `{id}` 的答案是未知原语（服务端可能新增了原语）"
                    )));
                }
            };
            Ok((QuestionId::from(id.clone()), answer))
        })
        .collect()
}

/// 用量：通道什么都没上报时留空，不写成 `Some(0, 0)`。
fn usage(usage: &jh_jev::Usage) -> Option<Usage> {
    (usage.input_tokens.is_some() || usage.output_tokens.is_some()).then_some(Usage {
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use jh_core::{NoulCriteria, NoulQuestion, QuestionSpec, QuestionVersion, SnapshotId};
    use serde_json::json;

    fn request() -> JudgeRequest {
        JudgeRequest {
            snapshot: SnapshotId::from("case-001"),
            state: State::Object(BTreeMap::from([
                ("task".to_owned(), json!("取消部署")),
                ("log".to_owned(), json!("取消确认落库")),
            ])),
            questions: Questions::from([
                (
                    QuestionId::from("failure_kind"),
                    QuestionSpec {
                        version: QuestionVersion(1),
                        question: Question::Choice(jh_core::ChoiceQuestion {
                            instructions: "哪一类原因？".to_owned(),
                            criteria: BTreeMap::from([
                                ("cache_stale".to_owned(), Some("取消后仍写入".to_owned())),
                                ("other".to_owned(), None),
                            ]),
                        }),
                    },
                ),
                (
                    QuestionId::from("needs_new_observation"),
                    QuestionSpec {
                        version: QuestionVersion(1),
                        question: Question::Noul(NoulQuestion {
                            instructions: "是否必须新增观测？".to_owned(),
                            criteria: Some(NoulCriteria {
                                r#true: Some("必须先补观测".to_owned()),
                                r#false: None,
                            }),
                        }),
                    },
                ),
            ]),
        }
    }

    fn response() -> jh_jev::SystemOneResponse {
        serde_json::from_value(json!({
            "model": "jev-1.13.0",
            "answers": {
                "failure_kind": {
                    "type": "choice",
                    "choice": "cache_stale",
                    "probabilities": {"cache_stale": 0.91, "other": 0.09},
                    "confidence": 0.82
                },
                "needs_new_observation": {"type": "noul", "noul": 0.12}
            },
            "usage": {"input_tokens": 512, "output_tokens": 0}
        }))
        .expect("手写的官方式响应应能解析")
    }

    #[test]
    fn state_shapes_map_one_to_one() {
        assert_eq!(
            serde_json::to_value(state(&State::from("取消确认"))).unwrap(),
            json!("取消确认")
        );
        assert_eq!(
            serde_json::to_value(state(&State::Object(BTreeMap::from([(
                "log".to_owned(),
                json!("a")
            )]))))
            .unwrap(),
            json!({"log": "a"})
        );
        assert_eq!(
            serde_json::to_value(state(&State::Records(vec![
                StateEntry::Text("第一行".to_owned()),
                StateEntry::Record(BTreeMap::from([("role".to_owned(), json!("user"))])),
            ])))
            .unwrap(),
            json!(["第一行", {"role": "user"}])
        );
    }

    #[test]
    fn questions_map_to_the_wire_shape_with_ids_kept() {
        let wire = questions(&request().questions);
        let wire = serde_json::to_value(&wire).unwrap();

        assert_eq!(wire["failure_kind"]["type"], json!("choice"));
        assert_eq!(wire["failure_kind"]["instructions"], json!("哪一类原因？"));
        assert_eq!(
            wire["failure_kind"]["criteria"]["cache_stale"],
            json!("取消后仍写入")
        );
        assert_eq!(wire["failure_kind"]["criteria"]["other"], json!(null));
        assert_eq!(
            wire["needs_new_observation"]["criteria"]["true"],
            json!("必须先补观测")
        );
        assert!(
            wire["needs_new_observation"]["criteria"]
                .get("false")
                .is_none()
        );
    }

    #[test]
    fn answers_and_usage_come_back_typed() {
        let request = request();
        let response = response();

        let answers = answers(&response).expect("三类答案都应能转回来");
        let converted = JudgeResponse {
            source: JudgeSource::RealApi,
            version: response.model.clone(),
            answers,
            usage: usage(&response.usage),
            request_id: Some("req_1".to_owned()),
        };

        converted.validate(&request).expect("响应满足判断器契约");
        let Answer::Choice(choice) = &converted.answers[&QuestionId::from("failure_kind")] else {
            panic!("failure_kind 应是 choice 答案");
        };
        assert_eq!(choice.choice, "cache_stale");
        assert_eq!(choice.probabilities["cache_stale"], 0.91);
        assert_eq!(
            converted.usage,
            Some(Usage {
                input_tokens: Some(512),
                output_tokens: Some(0)
            })
        );
    }

    #[test]
    fn unknown_answer_primitive_fails_explicitly() {
        let response: jh_jev::SystemOneResponse = serde_json::from_value(json!({
            "model": "jev-1.14.0",
            "answers": {"failure_kind": {"type": "ranking", "order": ["a", "b"]}},
            "usage": {}
        }))
        .expect("未知原语应解析成 Answer::Unknown 而不是解析失败");

        let error = answers(&response).expect_err("未知原语必须显式失败");
        assert!(matches!(error, Error::InvalidResponse(_)), "{error}");
        assert!(error.to_string().contains("failure_kind"), "{error}");
    }

    #[test]
    fn unreported_usage_stays_empty() {
        let response: jh_jev::SystemOneResponse = serde_json::from_value(json!({
            "model": "jev-1.13.0",
            "answers": {},
            "usage": {"input_tokens": null, "output_tokens": null}
        }))
        .expect("空 usage 应能解析");

        assert_eq!(usage(&response.usage), None);
    }

    #[test]
    fn channel_errors_map_to_the_two_channel_categories() {
        let missing = map_error(jh_jev::Error::MissingApiKey);
        assert!(matches!(missing, Error::Unavailable(_)), "{missing}");

        let decode = map_error(jh_jev::Error::Decode {
            body: "not a systemone response".to_owned(),
            source: serde_json::from_str::<i32>("x").expect_err("解析必然失败"),
            request_id: Some("req_1".to_owned()),
        });
        assert!(matches!(decode, Error::CallFailed(_)), "{decode}");
        assert!(decode.to_string().contains("failed to decode"), "{decode}");
    }
}
