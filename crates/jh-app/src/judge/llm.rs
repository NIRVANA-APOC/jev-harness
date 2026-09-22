//! LLM 结构化判断：`jh-openai` 的 Responses 通道 + 自带 JSON 契约。
//!
//! # 契约
//!
//! 请求只有一条用户消息，里面同时给出**输出契约**与**材料**（state + 问题集）：
//!
//! ```text
//! {"<问题ID>": {"probabilities": {"<选项名>": <概率>, ...}}, ...}
//! ```
//!
//! - choice 的概率键取自该问题的选项集；score 的键是级号（`criteria` 下标，从 0 开始）；
//!   noul 的键只能是 `true` / `false`；
//! - 模型只给分布：choice 的选择与 score 的期望值由本适配层按声明口径推导
//!   （概率最大的选项 / 概率加权期望），避免出现「标签与分布自相矛盾」的坏输出；
//! - `confidence` 与规则判断器同口径：`1 − H(p)/ln(支持集大小)`（[`super::derived_confidence`]）；
//! - 和不为 1 的分布按和归一后原样入记录；键集越界、和为 0、非有限值、缺题或多题一律显式失败。
//!
//! # 为什么不用 `instructions` 字段
//!
//! 判定契约必须随请求到达模型。不同 `/responses` 实现对未支持参数的处理不同
//! （DeepSeek 的 Responses 是无状态的，未支持参数被静默忽略），把契约放在可能被静默丢弃的字段里
//! 会让判定失去约束，因此契约与材料全部放进用户消息。
//!
//! # 为什么请求自带完整上下文
//!
//! 该通道无状态：不依赖 `store` / `previous_response_id`，每次请求都自带材料。

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::time::{Duration, Instant};

use jh_core::{
    Answer, ChoiceAnswer, Error, Judge, JudgeRequest, JudgeResponse, JudgeSource, NoulAnswer,
    Question, QuestionId, Questions, ScoreAnswer, Usage,
};
use jh_openai::{InputItem, ResponseStatus, ResponsesRequest};
use serde::Deserialize;

use super::derived_confidence;

/// 单次请求超时；LLM 输出的分布比 Jev 的整包响应慢，按仓库既有真调 smoke 的口径取 120s。
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

/// 输出上限：本判定的输出是一个 JSON 对象，长输出即契约失败。
///
/// 取值依据是「该 provider 的 thinking 模型把 reasoning token 也算进 `max_output_tokens`」
/// 这条实测事实，而不是「防止模型啰嗦」：调小只会截断思考，不会让输出更简短。
///
/// 2026-09-22 真调（同一提示词直调 `/responses`）：
/// cap 1024 → `incomplete`（`reason=max_output_tokens`，预算被 reasoning 吃光）；
/// cap 8192 → 已能完成，但同一提示词（`case-003`）重跑时 reasoning 用到 8481、11425 token，
/// 仍会被 8192 截断（判断器 4 次调用因此失败 1 次）；
/// cap 16384 → `completed`（reasoning 8481 / 4493），cap 32768 亦被服务端接受。
///
/// 取 16384 是两头取中：实测完成所需的 reasoning 最长 11425，16384 覆盖它；而按实测速率
/// （约 4.5 ms/token）跑满 16384 约 74 s，仍在 [`DEFAULT_TIMEOUT`] 的 120 s 之内——
/// 再往上（如 32768）跑满预算就会超时，失败形态从「截断」变成「超时」，反而更差。
pub const DEFAULT_MAX_OUTPUT_TOKENS: u32 = 16384;

/// 错误信息里保留的模型原始输出长度，用于定位契约漂移。
const RAW_OUTPUT_PREVIEW: usize = 200;

/// 调 `POST {base_url}/responses` 做结构化判断的判断器。
pub struct LlmJudge {
    client: jh_openai::Client,
}

impl LlmJudge {
    /// 从环境变量构造：`OPENAI_API_KEY`、`OPENAI_MODEL`、`OPENAI_BASE_URL` 三者都必填。
    ///
    /// 端点不设默认值：`jh-openai` 的缺省端点是官方 `https://api.openai.com/v1`，而本项目 LLM 侧
    /// 走的是 DeepSeek；只给模型不给端点会把 key 发到不是本人配置的端点上，所以宁可显式失败。
    /// 自建兼容端点写成不带尾斜杠的形式（例如 `https://api.deepseek.com`）。
    pub fn from_env() -> Result<Self, Error> {
        Ok(Self {
            client: from_env_client()?,
        })
    }
}

/// 一次「单提示词 → 单段文本」的通道调用结果。
pub(crate) struct Completion {
    /// 模型输出的文本。
    pub text: String,
    /// 执行者版本（通道响应的模型 ID）。
    pub model: String,
    /// 通道侧请求 id。
    pub request_id: String,
    /// 上报的 token 用量。
    pub usage: Option<Usage>,
    /// 本次调用耗时。
    pub latency: Duration,
}

/// 从环境变量构造 `jh-openai` 客户端：`OPENAI_API_KEY`、`OPENAI_MODEL`、`OPENAI_BASE_URL` 三者都必填。
///
/// 结构化判断与候选问题生成（M19 的生成步骤）共用这一份构造：两条路都走同一个模型、同一个端点、
/// 同一套「缺配置就显式失败」的口径。
pub(crate) fn from_env_client() -> Result<jh_openai::Client, Error> {
    let model = std::env::var("OPENAI_MODEL").map_err(|_| {
        Error::Unavailable("缺少 OPENAI_MODEL：LLM 通道必须显式给出模型 ID".to_owned())
    })?;
    let base_url = std::env::var("OPENAI_BASE_URL").map_err(|_| {
        Error::Unavailable(
            "缺少 OPENAI_BASE_URL：LLM 通道必须显式给出端点，\
             否则 key 会被发往 jh-openai 的官方缺省端点"
                .to_owned(),
        )
    })?;

    jh_openai::Client::builder()
        .model(model)
        .base_url(base_url)
        .timeout(DEFAULT_TIMEOUT)
        .build()
        .map_err(map_error)
}

/// 发一次单提示词请求并取回文本：失败响应、非 `completed` 状态都在这里显式失败。
///
/// 被判定的材料与输出契约**全部放在用户消息里**（理由见模块文档）；耗时从发请求前开始计。
pub(crate) async fn complete(
    client: &jh_openai::Client,
    prompt: String,
) -> Result<Completion, Error> {
    let body = ResponsesRequest::new(vec![InputItem::user_text(prompt)])
        .max_output_tokens(DEFAULT_MAX_OUTPUT_TOKENS);
    let started = Instant::now();
    let response = client.create(&body).await.map_err(map_error)?;
    let latency = started.elapsed();

    if let Some(error) = &response.error {
        let detail = error
            .message
            .clone()
            .unwrap_or_else(|| format!("{error:?}"));
        return Err(Error::CallFailed(format!("通道返回失败响应：{detail}")));
    }
    if response.status != ResponseStatus::Completed {
        // 截断原因随错误一起落进报告：只写「状态不是 completed」时，
        // 报告本身分不清是预算被 reasoning 吃光还是内容过滤，还得回头重跑才知道。
        let reason = response
            .incomplete_details
            .as_ref()
            .map(|details| format!("，incomplete_details.reason={:?}", details.reason))
            .unwrap_or_default();
        return Err(Error::InvalidResponse(format!(
            "响应状态是 {:?}{reason}，不是 completed",
            response.status
        )));
    }

    Ok(Completion {
        text: response.output_text(),
        model: response.model.clone(),
        request_id: response.id.clone(),
        usage: response.usage.as_ref().map(|usage| Usage {
            input_tokens: Some(usage.input_tokens),
            output_tokens: Some(usage.output_tokens),
        }),
        latency,
    })
}

impl Judge for LlmJudge {
    fn judge<'a>(
        &'a self,
        request: &'a JudgeRequest,
    ) -> Pin<Box<dyn Future<Output = Result<JudgeResponse, Error>> + Send + 'a>> {
        Box::pin(async move {
            let completion = complete(&self.client, prompt(request)?).await?;
            Ok(JudgeResponse {
                source: JudgeSource::Llm,
                version: completion.model,
                answers: parse_answers(&completion.text, &request.questions)?,
                usage: completion.usage,
                request_id: Some(completion.request_id),
            })
        })
    }
}

/// 把通道错误折进判断器的三类显式错误：缺凭据 / 缺配置是「不可用」，其余是「调用失败」。
fn map_error(error: jh_openai::Error) -> Error {
    match error {
        jh_openai::Error::MissingApiKey(name) => Error::Unavailable(format!("缺少 {name}")),
        jh_openai::Error::Config(detail) => {
            Error::Unavailable(format!("LLM 通道配置无效：{detail}"))
        }
        other => Error::CallFailed(other.to_string()),
    }
}

/// 一次请求的用户消息：先给出输出契约，再给出材料。
///
/// 材料是 state 与问题集的原样 JSON——问题定义里已含原语、说明与选项 / 级数，
/// 适配层不再另造一份描述，避免同一语义两套表示。
fn prompt(request: &JudgeRequest) -> Result<String, Error> {
    let material = serde_json::json!({
        "state": request.state,
        "questions": request.questions,
    });
    let material = serde_json::to_string_pretty(&material)
        .map_err(|error| Error::CallFailed(format!("材料无法序列化：{error}")))?;

    Ok(format!(
        "你是结构化判断器。只输出一个 JSON 对象：不要解释、不要代码块、不要多余文本。\n\
         \n\
         输出契约（顶层键 = 问题 ID，每个问题都要有键）：\n\
         {{\"<问题ID>\": {{\"probabilities\": {{\"<标签>\": <概率>, ...}}}}}}\n\
         标签与概率按问题原语给出：\n\
         - choice：标签取自该问题的选项名（criteria 的键）；\n\
         - score：标签是级号，即 criteria 数组的下标（从 0 开始）；\n\
         - noul：标签只能是 \"true\" 与 \"false\"，\"true\" 表示答案为是。\n\
         每个问题的概率之和为 1；只依据 state 里记录的事实作答，不引入外部知识；\n\
         证据不足时用接近均匀的分布表达不确定，不要编造 state 里没有的事实。\n\
         \n\
         材料：\n{material}"
    ))
}

/// 模型的原始回复：只关心概率分布。
#[derive(Debug, Deserialize)]
struct ModelAnswer {
    probabilities: BTreeMap<String, f64>,
}

/// 把模型输出解析成类型化答案：顶层键必须是请求里的问题 ID，一个不多一个不少。
pub fn parse_answers(
    output: &str,
    questions: &Questions,
) -> Result<BTreeMap<QuestionId, Answer>, Error> {
    let trimmed = output.trim();
    let raw: BTreeMap<String, ModelAnswer> = serde_json::from_str(trimmed).map_err(|source| {
        Error::InvalidResponse(format!(
            "模型输出不是约定的 JSON（{source}）：{}",
            preview(trimmed)
        ))
    })?;

    for id in raw.keys() {
        let id = QuestionId::from(id.clone());
        if !questions.contains_key(&id) {
            return Err(Error::InvalidResponse(format!(
                "模型给出了请求之外的问题 `{id}`"
            )));
        }
    }

    questions
        .iter()
        .map(|(id, spec)| {
            let raw = raw
                .get(id.as_str())
                .ok_or_else(|| Error::InvalidResponse(format!("模型没有回答问题 `{id}`")))?;
            let answer = answer(&spec.question, raw)?;
            Ok((id.clone(), answer))
        })
        .collect::<Result<_, _>>()
}

fn answer(question: &Question, raw: &ModelAnswer) -> Result<Answer, Error> {
    let total = total(&raw.probabilities)?;

    match question {
        Question::Choice(spec) => {
            let mut masses = BTreeMap::new();
            for (option, mass) in &raw.probabilities {
                if !spec.criteria.contains_key(option) {
                    return Err(Error::InvalidResponse(format!(
                        "选项 `{option}` 不在问题的选项集里"
                    )));
                }
                masses.insert(option.clone(), mass / total);
            }
            if spec.criteria.is_empty() {
                return Err(Error::InvalidResponse("问题的选项集为空".to_owned()));
            }
            // 未出现的选项概率记 0；并列取字典序最小（BTreeMap 升序遍历的第一个最大值）。
            let choice = spec
                .criteria
                .keys()
                .map(|option| (option.clone(), masses.get(option).copied().unwrap_or(0.0)))
                .fold(None, |best: Option<(String, f64)>, candidate| match best {
                    Some((_, best_mass)) if candidate.1 <= best_mass => best,
                    _ => Some(candidate),
                })
                .map(|(option, _)| option)
                .expect("选项集非空");

            let support = spec
                .criteria
                .keys()
                .map(|option| masses.get(option).copied().unwrap_or(0.0));
            let confidence = derived_confidence(support, spec.criteria.len());
            Ok(Answer::Choice(ChoiceAnswer {
                choice,
                probabilities: masses,
                confidence,
            }))
        }
        Question::Score(spec) => {
            let levels = spec.criteria.len();
            let mut masses = BTreeMap::new();
            for (label, mass) in &raw.probabilities {
                let level: u8 = label.parse().map_err(|_| {
                    Error::InvalidResponse(format!("级号 `{label}` 不是 0–255 的整数"))
                })?;
                if usize::from(level) >= levels {
                    return Err(Error::InvalidResponse(format!(
                        "级号 {level} 超出问题的 {levels} 级标尺"
                    )));
                }
                masses.insert(level, mass / total);
            }
            let score = masses
                .iter()
                .map(|(level, mass)| f64::from(*level) * mass)
                .sum();
            let support = (0..levels as u8).map(|level| masses.get(&level).copied().unwrap_or(0.0));
            let confidence = derived_confidence(support, levels);
            Ok(Answer::Score(ScoreAnswer {
                score,
                legend: spec
                    .criteria
                    .iter()
                    .enumerate()
                    .map(|(index, text)| (index as u8, text.clone()))
                    .collect(),
                probabilities: masses,
                confidence,
            }))
        }
        Question::Noul(_) => {
            for label in raw.probabilities.keys() {
                if label != "true" && label != "false" {
                    return Err(Error::InvalidResponse(format!(
                        "noul 答案的标签只能是 true / false，收到 `{label}`"
                    )));
                }
            }
            let (Some(yes), Some(no)) = (
                raw.probabilities.get("true"),
                raw.probabilities.get("false"),
            ) else {
                return Err(Error::InvalidResponse(
                    "noul 答案必须同时给出 true 与 false 的概率".to_owned(),
                ));
            };
            Ok(Answer::Noul(NoulAnswer {
                noul: yes / (yes + no),
            }))
        }
    }
}

/// 概率值的公共校验，返回归一化用的总和。
fn total(probabilities: &BTreeMap<String, f64>) -> Result<f64, Error> {
    let mut total = 0.0;
    for (label, mass) in probabilities {
        if !mass.is_finite() || *mass < 0.0 {
            return Err(Error::InvalidResponse(format!(
                "标签 `{label}` 的概率 {mass} 不合法"
            )));
        }
        total += mass;
    }
    if total <= 0.0 {
        return Err(Error::InvalidResponse("概率之和不是正数".to_owned()));
    }
    Ok(total)
}

/// 截断原始输出，避免把长文本整段塞进错误信息。
fn preview(text: &str) -> String {
    text.chars().take(RAW_OUTPUT_PREVIEW).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use jh_core::{
        ChoiceQuestion, NoulCriteria, NoulQuestion, QuestionSpec, QuestionVersion, ScoreQuestion,
    };

    fn questions() -> Questions {
        Questions::from([
            (
                QuestionId::from("failure_kind"),
                QuestionSpec {
                    version: QuestionVersion(1),
                    question: Question::Choice(ChoiceQuestion {
                        instructions: "哪一类原因？".to_owned(),
                        criteria: BTreeMap::from([
                            ("cache_stale".to_owned(), Some("取消后仍写入".to_owned())),
                            ("tool_timeout".to_owned(), None),
                            ("other".to_owned(), None),
                        ]),
                    }),
                },
            ),
            (
                QuestionId::from("evidence_strength"),
                QuestionSpec {
                    version: QuestionVersion(1),
                    question: Question::Score(ScoreQuestion {
                        instructions: "证据强度？".to_owned(),
                        criteria: vec!["无".to_owned(), "弱".to_owned(), "强".to_owned()],
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
                            r#false: Some("现有记录已足够".to_owned()),
                        }),
                    }),
                },
            ),
        ])
    }

    /// 手写的模型回复（不是任何真实调用的录制）：形状即契约本身。
    const HANDS_WRITTEN_REPLY: &str = r#"{
        "failure_kind": {"probabilities": {"cache_stale": 0.7, "tool_timeout": 0.2, "other": 0.1}},
        "evidence_strength": {"probabilities": {"1": 0.25, "2": 0.75}},
        "needs_new_observation": {"probabilities": {"true": 0.2, "false": 0.8}}
    }"#;

    #[test]
    fn parses_a_well_formed_reply_into_typed_answers() {
        let questions = questions();
        let request = JudgeRequest {
            snapshot: jh_core::SnapshotId::from("case-001"),
            state: jh_core::State::from("取消确认落库"),
            questions: questions.clone(),
        };
        let answers = parse_answers(HANDS_WRITTEN_REPLY, &questions).expect("应能解析成类型化答案");

        let response = JudgeResponse {
            source: JudgeSource::Llm,
            version: "deepseek-flash".to_owned(),
            answers,
            usage: None,
            request_id: Some("resp_1".to_owned()),
        };
        response.validate(&request).expect("响应满足判断器契约");

        let Answer::Choice(choice) = &response.answers[&QuestionId::from("failure_kind")] else {
            panic!("failure_kind 应是 choice 答案");
        };
        assert_eq!(choice.choice, "cache_stale");
        assert_eq!(choice.probabilities["cache_stale"], 0.7);
        let expected = derived_confidence([0.7, 0.2, 0.1], 3);
        assert!(
            (choice.confidence - expected).abs() < 1e-12,
            "confidence {} 应等于 {expected}",
            choice.confidence
        );

        let Answer::Score(score) = &response.answers[&QuestionId::from("evidence_strength")] else {
            panic!("evidence_strength 应是 score 答案");
        };
        assert_eq!(score.score, 1.75);
        assert_eq!(score.legend[&2], "强");
        assert_eq!(score.probabilities, BTreeMap::from([(1, 0.25), (2, 0.75)]));

        let Answer::Noul(noul) = &response.answers[&QuestionId::from("needs_new_observation")]
        else {
            panic!("needs_new_observation 应是 noul 答案");
        };
        assert_eq!(noul.noul, 0.2);
    }

    #[test]
    fn probabilities_that_do_not_sum_to_one_are_normalised() {
        let questions = questions();
        let answers = parse_answers(
            r#"{"failure_kind": {"probabilities": {"cache_stale": 7, "other": 3}},
                "evidence_strength": {"probabilities": {"0": 1}},
                "needs_new_observation": {"probabilities": {"true": 3, "false": 1}}}"#,
            &questions,
        )
        .expect("和不为 1 时按和归一，不算失败");

        let Answer::Choice(choice) = &answers[&QuestionId::from("failure_kind")] else {
            panic!("failure_kind 应是 choice 答案");
        };
        assert_eq!(choice.probabilities["cache_stale"], 0.7);
        assert_eq!(choice.probabilities["other"], 0.3);
    }

    #[test]
    fn missing_or_extra_questions_fail_explicitly() {
        let questions = questions();
        let missing = parse_answers(
            r#"{"failure_kind": {"probabilities": {"other": 1}},
                "evidence_strength": {"probabilities": {"0": 1}}}"#,
            &questions,
        )
        .expect_err("缺题必须显式失败");
        assert!(missing.to_string().contains("needs_new_observation"));

        let extra = parse_answers(
            r#"{"failure_kind": {"probabilities": {"other": 1}},
                "evidence_strength": {"probabilities": {"0": 1}},
                "needs_new_observation": {"probabilities": {"true": 1, "false": 0}},
                "never_asked": {"probabilities": {"x": 1}}}"#,
            &questions,
        )
        .expect_err("多题必须显式失败");
        assert!(extra.to_string().contains("never_asked"));
    }

    #[test]
    fn out_of_range_labels_and_bad_probabilities_fail_explicitly() {
        let questions = questions();

        let error = parse_answers(
            r#"{"failure_kind": {"probabilities": {"never_declared": 1}},
                "evidence_strength": {"probabilities": {"0": 1}},
                "needs_new_observation": {"probabilities": {"true": 1, "false": 0}}}"#,
            &questions,
        )
        .expect_err("越界选项必须显式失败");
        assert!(error.to_string().contains("never_declared"), "{error}");

        let error = parse_answers(
            r#"{"failure_kind": {"probabilities": {"other": 0}},
                "evidence_strength": {"probabilities": {"7": 1}},
                "needs_new_observation": {"probabilities": {"true": 1, "false": 0}}}"#,
            &questions,
        )
        .expect_err("和为 0 必须显式失败");
        assert!(matches!(error, Error::InvalidResponse(_)), "{error}");
    }

    #[test]
    fn a_score_level_outside_the_scale_fails_explicitly() {
        let questions = questions();
        let error = parse_answers(
            r#"{"failure_kind": {"probabilities": {"other": 1}},
                "evidence_strength": {"probabilities": {"9": 1}},
                "needs_new_observation": {"probabilities": {"true": 1, "false": 0}}}"#,
            &questions,
        )
        .expect_err("越界级号必须显式失败");
        assert!(error.to_string().contains("级号"), "{error}");
    }

    #[test]
    fn a_noul_answer_missing_a_side_fails_explicitly() {
        let questions = questions();
        let error = parse_answers(
            r#"{"failure_kind": {"probabilities": {"other": 1}},
                "evidence_strength": {"probabilities": {"0": 1}},
                "needs_new_observation": {"probabilities": {"true": 1}}}"#,
            &questions,
        )
        .expect_err("noul 缺一侧必须显式失败");
        assert!(error.to_string().contains("true 与 false"), "{error}");
    }

    #[test]
    fn non_json_output_fails_with_a_preview() {
        let questions = questions();
        let error = parse_answers("这是解释文本，不是 JSON。", &questions)
            .expect_err("非 JSON 输出必须显式失败");
        assert!(error.to_string().contains("不是约定的 JSON"), "{error}");
        assert!(error.to_string().contains("解释文本"), "{error}");
    }

    #[test]
    fn the_prompt_carries_the_contract_and_the_material() {
        let questions = questions();
        let request = JudgeRequest {
            snapshot: jh_core::SnapshotId::from("case-001"),
            state: jh_core::State::from("取消确认落库"),
            questions,
        };
        let prompt = prompt(&request).expect("提示词应能生成");

        assert!(prompt.contains("只输出一个 JSON 对象"));
        assert!(prompt.contains("\"failure_kind\""));
        assert!(prompt.contains("取消确认落库"));
        assert!(prompt.contains("cache_stale"));
    }
}
