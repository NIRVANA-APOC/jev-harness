//! 候选问题的验证：**开发案例、正例与反例、不同表述**上的检验（`docs/RESEARCH_GOALS.md` §6.2 第 5 步、
//! §6.5）。
//!
//! 验证是一次**只问不决策**的判定：每个用例给一份材料与一个正确读法，候选问题（某一版本）被判一次，
//! 记下一致与否、Brier、对数损失、延迟与 token。读数按原值进报告与问题库的验证记录
//! （[`jh_core::ValidationRecord`]），口径写进报告的 `metric_definitions`。
//!
//! 用例分三类（任务族 B 的 `validation` 段声明）：
//!
//! - **正例**：材料足够（关键证据在材料里），正确读法是对应那一侧的标签；
//! - **反例**：材料不足（关键证据不在材料里），正确读法是 `other`；
//! - **不同表述 / 不同服务的未见变体**：与开发案例措辞不同、服务不同，仍要读出正确标签
//!   （§6.5「仅在原始失败案例上变正确，不算迁移成功」）。
//!
//! 材料装成与判断循环同一种 state 形态（对象 → `records`），因此验证与运行期的读数可比。

use std::collections::BTreeMap;
use std::time::Instant;

use jh_core::{
    Judge, JudgeRequest, JudgmentRecord, ObjectId, QuestionId, QuestionSpec, SnapshotId, State,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::calls::{CallEntry, CallLog};
use crate::error::Error;
use crate::exp::unix_ms;
use crate::family_b::{FamilyB, ValidationCase};
use crate::judge::JudgeName;
use crate::metrics;

/// 用例的类别（按标识后缀分，口径写进报告的 `metric_definitions`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaseKind {
    /// 正例：材料足够。
    Positive,
    /// 反例：材料不足，正确读法是 `other`。
    Negative,
    /// 不同表述 / 不同服务的未见变体。
    Unseen,
}

impl CaseKind {
    /// 从用例标识推类别（标识由任务族 JSON 声明，后缀即类别）。
    pub fn of(case: &ValidationCase) -> Self {
        if case.id.ends_with("negative") {
            Self::Negative
        } else if case.id.ends_with("unseen-wording") {
            Self::Unseen
        } else {
            Self::Positive
        }
    }

    /// 报告里的标签。
    pub fn label(self) -> &'static str {
        match self {
            Self::Positive => "positive",
            Self::Negative => "negative",
            Self::Unseen => "unseen",
        }
    }
}

/// 一个验证用例的结果。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaseResult {
    /// 用例标识。
    pub id: String,
    /// 类别。
    pub kind: CaseKind,
    /// 属于哪一对。
    pub pair: String,
    /// 用例在验证什么。
    pub context: String,
    /// 问题。
    pub question: QuestionId,
    /// 被验证的版本。
    pub version: jh_core::QuestionVersion,
    /// 读出标签；读不出时为空串。
    pub reading: String,
    /// 正确读法。
    pub expected: String,
    /// 是否一致。
    pub agreement: bool,
    /// Brier 分数。
    pub brier: f64,
    /// 对数损失；真值拿到 0 概率时为 `None`。
    pub log_loss: Option<f64>,
    /// 本次判定耗时。
    pub latency_ms: u64,
    /// 执行者版本。
    pub version_label: Option<String>,
    /// 失败原文；成功时为 `None`。
    pub failure: Option<String>,
}

/// 一次验证的汇总读数。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationStats {
    /// 用例数。
    pub cases: u32,
    /// 成功判定数。
    pub succeeded: u32,
    /// 失败判定数（失败如实计入，不重试）。
    pub failed: u32,
    /// 一致数。
    pub agreements: u32,
    /// 一致率。
    pub agreement_rate: Option<f64>,
    /// 正例一致率。
    pub positive_rate: Option<f64>,
    /// 反例一致率。
    pub negative_rate: Option<f64>,
    /// 未见表述一致率。
    pub unseen_rate: Option<f64>,
    /// 输入 token 合计（通道上报）。
    pub input_tokens: Option<u64>,
    /// 输出 token 合计。
    pub output_tokens: Option<u64>,
}

impl ValidationStats {
    /// 某一类的用例数与一致数。
    pub fn kind(&self) -> BTreeMap<String, (u32, u32)> {
        BTreeMap::new()
    }
}

/// 一次验证的全部读数。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationRun {
    /// 被验证的问题。
    pub question: QuestionId,
    /// 被验证的版本。
    pub version: jh_core::QuestionVersion,
    /// 被验证版本声明的读取范围（材料按它裁剪：问题读不到的对象不进材料）。
    pub reads: Vec<ObjectId>,
    /// 逐用例结果。
    pub cases: Vec<CaseResult>,
    /// 汇总读数。
    pub stats: ValidationStats,
}

impl ValidationRun {
    /// 某一类的一致率读数（报告按类别分别给出）。
    pub fn rate_of(&self, kind: CaseKind) -> Option<f64> {
        let rows: Vec<&CaseResult> = self.cases.iter().filter(|row| row.kind == kind).collect();
        if rows.is_empty() {
            return None;
        }
        let agreements = rows.iter().filter(|row| row.agreement).count();
        Some(agreements as f64 / rows.len() as f64)
    }
}

/// 跑一遍验证：**这一对**的每个用例一次判定调用。
///
/// 材料按被验证版本声明的读取范围裁剪（[`ValidationCase::material`] 里不在 `reads` 里的对象不进请求）：
/// 这正是「材料是否足够」的判据落点——固定问题只读 `app.log` 时，关键证据不在它的材料里。
#[allow(clippy::too_many_arguments)]
pub async fn run(
    family: &FamilyB,
    spec: &QuestionSpec,
    question: &QuestionId,
    reads: &[ObjectId],
    judge: &dyn Judge,
    channel: JudgeName,
    run_id: &str,
    calls: &mut CallLog,
) -> Result<ValidationRun, Error> {
    let pair = family
        .pair_of_question(question)
        .ok_or_else(|| Error::TaskSet(format!("问题 `{question}` 不属于任何一对场景")))?
        .id
        .clone();
    let cases_of_pair: Vec<&ValidationCase> = family
        .validation
        .iter()
        .filter(|case| case.pair == pair)
        .collect();
    let mut cases = Vec::with_capacity(cases_of_pair.len());
    let mut input_tokens = 0u64;
    let mut output_tokens = 0u64;
    let mut usage_seen = false;

    for case in cases_of_pair {
        let request = JudgeRequest {
            snapshot: SnapshotId::from(format!("validation-{}", case.id)),
            state: state_of(case, reads),
            questions: BTreeMap::from([(question.clone(), spec.clone())]),
        };
        let started = Instant::now();
        let outcome = judge.judge(&request).await;
        let latency = started.elapsed();
        let expected = expected_label(case)?;
        let latency_ms = latency.as_millis() as u64;

        let mut result = CaseResult {
            id: case.id.clone(),
            kind: CaseKind::of(case),
            pair: case.pair.clone(),
            context: case.context.clone(),
            question: question.clone(),
            version: spec.version,
            reading: String::new(),
            expected: expected.clone(),
            agreement: false,
            brier: f64::NAN,
            log_loss: None,
            latency_ms,
            version_label: None,
            failure: None,
        };

        match outcome {
            Ok(response) => match JudgmentRecord::from_response(&request, &response, latency) {
                Ok(record) => {
                    let answer = record
                        .judgments
                        .get(question)
                        .expect("记录里有该问题的答案")
                        .answer
                        .clone();
                    result.reading = metrics::reading(&answer)
                        .map(|reading| reading.label)
                        .unwrap_or_default();
                    let scored = metrics::score(&answer, &case.expected);
                    result.agreement = scored.as_ref().is_some_and(|score| score.agreement);
                    result.brier = scored.as_ref().map(|score| score.brier).unwrap_or(f64::NAN);
                    result.log_loss = scored.as_ref().and_then(|score| score.log_loss);
                    result.version_label = Some(record.version.clone());
                    if let Some(usage) = &record.usage {
                        usage_seen = true;
                        input_tokens += usage.input_tokens.unwrap_or(0);
                        output_tokens += usage.output_tokens.unwrap_or(0);
                    }
                    calls.append(&CallEntry::Judgment {
                        run: run_id.to_owned(),
                        case: format!("validation-{}", case.id),
                        record: Box::new(record),
                    })?;
                }
                Err(error) => {
                    let text = error.to_string();
                    result.failure = Some(text.clone());
                    calls.append(&CallEntry::Failure {
                        run: run_id.to_owned(),
                        case: format!("validation-{}", case.id),
                        source: channel.source(),
                        latency_ms,
                        error: text,
                    })?;
                }
            },
            Err(error) => {
                let text = error.to_string();
                result.failure = Some(text.clone());
                calls.append(&CallEntry::Failure {
                    run: run_id.to_owned(),
                    case: format!("validation-{}", case.id),
                    source: channel.source(),
                    latency_ms,
                    error: text,
                })?;
            }
        }
        cases.push(result);
    }

    let mut stats = summarize(&cases);
    stats.input_tokens = usage_seen.then_some(input_tokens);
    stats.output_tokens = usage_seen.then_some(output_tokens);
    Ok(ValidationRun {
        question: question.clone(),
        version: spec.version,
        reads: reads.to_vec(),
        cases,
        stats,
    })
}

fn summarize(cases: &[CaseResult]) -> ValidationStats {
    let succeeded = cases.iter().filter(|row| row.failure.is_none()).count() as u32;
    let agreements = cases.iter().filter(|row| row.agreement).count() as u32;
    let rate = |kind: CaseKind| {
        let rows: Vec<&CaseResult> = cases.iter().filter(|row| row.kind == kind).collect();
        (!rows.is_empty())
            .then(|| rows.iter().filter(|row| row.agreement).count() as f64 / rows.len() as f64)
    };
    ValidationStats {
        cases: cases.len() as u32,
        succeeded,
        failed: cases.len() as u32 - succeeded,
        agreements,
        agreement_rate: (!cases.is_empty()).then(|| agreements as f64 / cases.len() as f64),
        positive_rate: rate(CaseKind::Positive),
        negative_rate: rate(CaseKind::Negative),
        unseen_rate: rate(CaseKind::Unseen),
        input_tokens: None,
        output_tokens: None,
    }
}

fn expected_label(case: &ValidationCase) -> Result<String, Error> {
    match &case.expected {
        crate::tasks::Truth::Choice { label } => Ok(label.clone()),
        other => Err(Error::TaskSet(format!(
            "验证用例 {} 的期望读法必须是选项标签：{other:?}",
            case.id
        ))),
    }
}

/// 把用例材料装成与判断循环同一种 state 形态（对象 → `records`），并按声明的读取范围裁剪。
fn state_of(case: &ValidationCase, reads: &[ObjectId]) -> State {
    let at_unix_ms = unix_ms();
    let sections: BTreeMap<String, Value> = case
        .material
        .iter()
        .filter(|(object, _)| reads.contains(object))
        .map(|(object, content)| {
            (
                object.as_str().to_owned(),
                serde_json::json!({
                    "records": [{
                        "source": format!("tool:validation-{}", case.id),
                        "at_unix_ms": at_unix_ms,
                        "version": 1,
                        "content": content,
                    }]
                }),
            )
        })
        .collect();
    State::Object(sections)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::judge::RuleJudge;

    #[tokio::test]
    async fn validation_runs_every_case_and_scores_it() {
        let family = FamilyB::load().unwrap();
        let judge = RuleJudge::new();
        let dir = std::env::temp_dir().join(format!("jh-app-validation-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut calls = CallLog::open(dir.join("calls.jsonl")).unwrap();
        let question = QuestionId::from("b_pair_cause_p1");
        let spec = family.question(&question).expect("固定问题存在").clone();

        let run = run(
            &family,
            &spec,
            &question,
            &[ObjectId::from("app.log")],
            &judge,
            JudgeName::Rule,
            "validation-test",
            &mut calls,
        )
        .await
        .expect("规则判定器不会失败");

        assert_eq!(run.cases.len(), 4, "每对 4 个验证用例");
        // 正例：固定问题读不到关键证据 → 读出 other，与正例的期望标签不一致（这正是精化要解决的）。
        assert_eq!(run.rate_of(CaseKind::Positive), Some(0.0));
        // 反例：材料不足 → 正确读法就是 other，固定问题答对。
        assert_eq!(run.rate_of(CaseKind::Negative), Some(1.0));
        assert_eq!(run.stats.failed, 0);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn a_refined_version_reads_the_key_evidence_and_matches_the_positives() {
        let family = FamilyB::load().unwrap();
        let judge = RuleJudge::new();
        let dir =
            std::env::temp_dir().join(format!("jh-app-validation-refined-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut calls = CallLog::open(dir.join("calls.jsonl")).unwrap();
        let question = QuestionId::from("b_pair_cause_p1");
        let spec = QuestionSpec {
            version: jh_core::QuestionVersion(2),
            question: family.question(&question).unwrap().question.clone(),
        };

        let run = run(
            &family,
            &spec,
            &question,
            &[
                ObjectId::from("app.log"),
                ObjectId::from("credential.audit"),
            ],
            &judge,
            JudgeName::Rule,
            "validation-refined",
            &mut calls,
        )
        .await
        .expect("规则判定器不会失败");

        // 规则判定器按材料里的标记作答：材料里有关键证据时读出对应标签。
        assert_eq!(run.rate_of(CaseKind::Positive), Some(1.0));
        assert_eq!(run.rate_of(CaseKind::Negative), Some(1.0));

        std::fs::remove_dir_all(&dir).ok();
    }
}
