//! 判断记录：绑定快照、问题版本、执行者版本、结果与分布、用量、延迟、阈值与来源。

use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::answer::{Answer, Usage};
use crate::error::Error;
use crate::judge::{JudgeRequest, JudgeResponse};
use crate::question::{QuestionId, QuestionVersion};
use crate::state::SnapshotId;

/// 判断来源标签。
///
/// 五种判断器各占一个标签（`docs/roadmap-1.0.md` §2.3）。模拟器真值在类型上就是另一种来源，
/// 因为 M0 的退出证据要求能区分真实 API 与模拟运行（`docs/roadmap-1.0.md` §5 M0）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JudgeSource {
    /// 真实 API 调用（当前指 Jev System One 通道）。
    RealApi,
    /// LLM 结构化判断（生成模型通道）。
    Llm,
    /// 确定性规则，不调模型。
    Rule,
    /// 录制回放：复用已落盘的判断记录，不发起调用。
    Replay,
    /// 模拟器真值：只准用于诊断与上限估计（`docs/RESEARCH_GOALS.md` §9.2、§10.2），不得冒充真实运行。
    SimulatedTruth,
}

/// 一条判断：某个问题在这一次调用里得到的答案。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Judgment {
    /// 作答时所用问题定义的版本。
    pub question_version: QuestionVersion,
    /// 类型化答案，含概率分布与置信度原值。
    pub answer: Answer,
    /// 解读该答案时使用的阈值（名字 → 值）；没有阈值时为空。
    ///
    /// 阈值来自消费方而不是判断器，所以 `from_response` 只留空位，由调用方在需要时写入。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub thresholds: BTreeMap<String, f64>,
}

/// 一次判断调用的记录。
///
/// 研究目标 §5.2 要求判断绑定输入快照、问题版本、模型版本、结果与分布；
/// `docs/jev-research.md` §13.3.3 另要求记录实际模型版本、`request_id` 与阈值。
/// 记录可 JSON 往返，因此可以离线回放（`docs/roadmap-1.0.md` §4 #3）；怎么落盘不在本 crate
/// （`docs/roadmap-1.0.md` §12 未确定项 2）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JudgmentRecord {
    /// 输入材料的快照标识。
    pub snapshot: SnapshotId,
    /// 这次判断由哪一类判断器做出。
    pub source: JudgeSource,
    /// 实际执行者的版本（模型 ID 或规则 / 模拟器版本串）。
    pub version: String,
    /// 通道侧请求 id；本地判断器时为空。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// token 用量；未上报时为空。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    /// 端到端延迟，毫秒（不足 1ms 记 0）。
    pub latency_ms: u64,
    /// 每个问题的判断：问题版本、答案与分布、解读阈值。
    pub judgments: BTreeMap<QuestionId, Judgment>,
}

impl JudgmentRecord {
    /// 由一次请求与响应构造记录。
    ///
    /// 响应不满足判断器契约时显式失败，不产出半条记录；阈值位留空，由调用方按需要写入
    /// [`Judgment::thresholds`]。
    pub fn from_response(
        request: &JudgeRequest,
        response: &JudgeResponse,
        latency: Duration,
    ) -> Result<Self, Error> {
        let judgments = response
            .checked_answers(request)?
            .into_iter()
            .map(|(id, spec, answer)| {
                (
                    id.clone(),
                    Judgment {
                        question_version: spec.version,
                        answer: answer.clone(),
                        thresholds: BTreeMap::new(),
                    },
                )
            })
            .collect();

        Ok(Self {
            snapshot: request.snapshot.clone(),
            source: response.source,
            version: response.version.clone(),
            request_id: response.request_id.clone(),
            usage: response.usage.clone(),
            latency_ms: latency.as_millis() as u64,
            judgments,
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::answer::NoulAnswer;
    use crate::question::{NoulQuestion, Question, QuestionSpec, Questions};
    use crate::state::State;

    fn request() -> JudgeRequest {
        JudgeRequest {
            snapshot: SnapshotId::from("case-001"),
            state: State::from("取消已确认"),
            questions: Questions::from([(
                QuestionId::from("cache_written_after_cancel"),
                QuestionSpec {
                    version: QuestionVersion(3),
                    question: Question::Noul(NoulQuestion {
                        instructions: "？".to_owned(),
                        criteria: None,
                    }),
                },
            )]),
        }
    }

    fn response(noul: f64) -> JudgeResponse {
        JudgeResponse {
            source: JudgeSource::RealApi,
            version: "jev-1.13.0".to_owned(),
            answers: BTreeMap::from([(
                QuestionId::from("cache_written_after_cancel"),
                Answer::Noul(NoulAnswer { noul }),
            )]),
            usage: Some(Usage {
                input_tokens: Some(512),
                output_tokens: None,
            }),
            request_id: Some("req-7".to_owned()),
        }
    }

    #[test]
    fn record_binds_request_and_response_metadata() {
        let request = request();
        let record =
            JudgmentRecord::from_response(&request, &response(0.42), Duration::from_millis(137))
                .expect("响应满足契约");

        assert_eq!(record.snapshot, SnapshotId::from("case-001"));
        assert_eq!(record.source, JudgeSource::RealApi);
        assert_eq!(record.version, "jev-1.13.0");
        assert_eq!(record.request_id.as_deref(), Some("req-7"));
        assert_eq!(
            record.usage.as_ref().and_then(|usage| usage.input_tokens),
            Some(512)
        );
        assert_eq!(record.latency_ms, 137);

        let judgment = &record.judgments[&QuestionId::from("cache_written_after_cancel")];
        assert_eq!(judgment.question_version, QuestionVersion(3));
        assert_eq!(judgment.answer, Answer::Noul(NoulAnswer { noul: 0.42 }));
        assert!(judgment.thresholds.is_empty());
    }

    #[test]
    fn record_round_trips_through_json_with_thresholds_and_raw_probabilities() {
        let mut record =
            JudgmentRecord::from_response(&request(), &response(0.42), Duration::from_millis(137))
                .expect("响应满足契约");
        record
            .judgments
            .get_mut(&QuestionId::from("cache_written_after_cancel"))
            .expect("该问题应在记录里")
            .thresholds
            .insert("yes_min".to_owned(), 0.8);

        let wire = serde_json::to_value(&record).unwrap();
        assert_eq!(wire["source"], json!("real_api"));
        assert_eq!(wire["version"], json!("jev-1.13.0"));
        assert_eq!(wire["latency_ms"], json!(137));
        assert_eq!(
            wire["judgments"]["cache_written_after_cancel"]["answer"]["noul"],
            json!(0.42)
        );
        assert_eq!(
            wire["judgments"]["cache_written_after_cancel"]["thresholds"]["yes_min"],
            json!(0.8)
        );

        let round_tripped: JudgmentRecord =
            serde_json::from_str(&serde_json::to_string(&record).unwrap()).unwrap();
        assert_eq!(round_tripped, record);
    }

    #[test]
    fn record_construction_fails_on_incomplete_responses() {
        let mut response = response(0.42);
        response.answers.clear();

        let error = JudgmentRecord::from_response(&request(), &response, Duration::ZERO)
            .expect_err("缺答案必须显式失败");
        assert!(matches!(error, Error::InvalidResponse(_)), "{error}");
    }

    #[test]
    fn simulated_truth_is_a_distinct_source_label() {
        let mut response = response(1.0);
        response.source = JudgeSource::SimulatedTruth;
        response.request_id = None;
        response.usage = None;

        let record = JudgmentRecord::from_response(&request(), &response, Duration::from_millis(1))
            .expect("响应满足契约");
        let wire = serde_json::to_value(&record).unwrap();

        assert_eq!(wire["source"], json!("simulated_truth"));
        assert!(wire.get("request_id").is_none());
        assert!(wire.get("usage").is_none());
    }
}
