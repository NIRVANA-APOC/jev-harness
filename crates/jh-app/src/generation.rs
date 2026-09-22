//! 候选问题生成：真实生成模型依据**反例记录 + 现有问题集**产出候选问题（`docs/RESEARCH_GOALS.md` §6.2
//! 第 3、4 步）。
//!
//! # 最小契约（§6.3 八字段）
//!
//! 候选必须一次给全八项，缺一项、占位缺失、生命周期不是「候选」都显式失败，不做补全：
//!
//! | 字段 | 这里的落点 |
//! | --- | --- |
//! | 命题与类型 | [`CandidateQuestion::question`]（三类原语之一） |
//! | 区分目的 | [`CandidateQuestion::distinguishes`] |
//! | 证据需求 | [`CandidateQuestion::evidence`]（来源与字段）+ 时效 + 获取方法（后两项是本层的字段：
//!   `jh-core` 的 [`jh_core::NewQuestion`] 只表达来源与字段，见 `library.rs` 登记的缺口） |
//! | 适用范围 | [`CandidateQuestion::scope`] |
//! | 触发条件 | [`CandidateQuestion::trigger`] |
//! | 下游用途 | [`CandidateQuestion::consumers`]（不能为空） |
//! | 验证记录 | [`CandidateQuestion::validation_placeholder`]（生成时给不出读数，必须留占位说明） |
//! | 生命周期 | [`CandidateQuestion::lifecycle`]，只能是 `candidate` |
//!
//! # 精化 = 同一个门控问题的新版本
//!
//! `refines` 必须是被门控的那个问题 ID：精化只改「问题定义与读取范围」，不改消费者代码、决策器提示、
//! 权重或控制规则（§6.4 的固定下游消费协议）。选项标签集合必须原样保留——前提门按标签判，改标签
//! 就是改控制规则；[`check_contract`] 把这一条核出来。
//!
//! # 真值隔离
//!
//! 提示词里只放**开发案例**的失败现场（含开发案例的真值与读法），不放未见变体的隐藏真值
//! （`docs/RESEARCH_GOALS.md` §13.2）：H3 的对照读数在未见变体上采集。

use std::collections::{BTreeMap, BTreeSet};

use jh_core::{Counterexample, FieldSelection, NewQuestion, ObjectId, Question, QuestionId};
use jh_openai::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::Error;
use crate::family_b::FamilyB;
use crate::judge::llm::{Completion, complete, from_env_client};

/// 生成模型给出的候选问题（八字段 + 它要精化哪一个门控问题）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateQuestion {
    /// 精化哪一个已有的门控问题：精化是同一个 ID 的新版本（统一字段）。
    pub refines: QuestionId,
    /// 命题与类型。
    pub question: Question,
    /// 区分目的：它要区分哪些会导致不同动作的情形。
    pub distinguishes: String,
    /// 证据需求：来源与字段。版本在判定时由输入投影给出，这里不写。
    pub evidence: BTreeMap<ObjectId, FieldSelection>,
    /// 证据需求的时效（§6.3 的「时效」，`jh-core` 未建模，由本层承载）。
    pub evidence_freshness: String,
    /// 证据需求的获取方法（§6.3 的「获取方法」，同上）。
    pub evidence_method: String,
    /// 适用范围。
    pub scope: String,
    /// 触发条件。
    pub trigger: String,
    /// 下游用途（消费者标签）；不能为空。
    pub consumers: BTreeSet<String>,
    /// 验证记录占位：生成时还没有读数，这里必须写明「待填」。
    pub validation_placeholder: String,
    /// 生命周期：只能是 `candidate`。
    pub lifecycle: String,
}

impl CandidateQuestion {
    /// 读取范围 = 证据需求里的对象（按对象名排序）。
    pub fn reads(&self) -> Vec<ObjectId> {
        self.evidence.keys().cloned().collect()
    }

    /// 转成问题库的入库形态（六项由库管，验证记录与生命周期由库按流程写）。
    pub fn to_new_question(&self) -> NewQuestion {
        NewQuestion {
            question: self.question.clone(),
            distinguishes: self.distinguishes.clone(),
            evidence: self.evidence.clone(),
            scope: self.scope.clone(),
            trigger: self.trigger.clone(),
            consumers: self.consumers.clone(),
        }
    }

    /// 这个候选在提示词与报告里的稳定标识。
    pub fn label(&self) -> String {
        format!("{}→{}", self.refines, question_kind(&self.question))
    }
}

fn question_kind(question: &Question) -> &'static str {
    match question {
        Question::Choice(_) => "choice",
        Question::Score(_) => "score",
        Question::Noul(_) => "noul",
    }
}

/// 从模型输出解析候选问题。
///
/// 严格到字段：缺字段、字段为空、生命周期不是 `candidate`、`refines` 缺失都显式失败并说出是哪一个
/// （研究目标 §8.2「失败候选保留记录，不悄悄覆盖有效版本」——解析失败也算一次生成失败，照样记）。
pub fn parse_candidate(output: &str) -> Result<CandidateQuestion, Error> {
    let trimmed = output.trim();
    let value: Value = serde_json::from_str(trimmed).map_err(|source| {
        Error::TaskSet(format!(
            "生成输出不是约定的 JSON（{source}）：{}",
            preview(trimmed)
        ))
    })?;
    let object = value
        .as_object()
        .ok_or_else(|| Error::TaskSet("生成输出不是一个 JSON 对象".to_owned()))?;

    let refines = QuestionId::from(required_str(object, "refines")?);
    let question: Question = serde_json::from_value(required(object, "question")?.clone())
        .map_err(|source| {
            Error::TaskSet(format!("字段 `question` 不是合法的问题定义：{source}"))
        })?;
    let distinguishes = required_str(object, "distinguishes")?;
    let evidence: BTreeMap<ObjectId, FieldSelection> =
        serde_json::from_value(required(object, "evidence")?.clone()).map_err(|source| {
            Error::TaskSet(format!(
                "字段 `evidence` 不是「对象 → 字段范围」的映射：{source}"
            ))
        })?;
    if evidence.is_empty() {
        return Err(Error::TaskSet(
            "字段 `evidence` 为空：候选必须声明需要的来源与字段".to_owned(),
        ));
    }
    let evidence_freshness = required_str(object, "evidence_freshness")?;
    let evidence_method = required_str(object, "evidence_method")?;
    let scope = required_str(object, "scope")?;
    let trigger = required_str(object, "trigger")?;
    let consumers: BTreeSet<String> =
        serde_json::from_value(required(object, "consumers")?.clone()).map_err(|source| {
            Error::TaskSet(format!("字段 `consumers` 不是字符串数组：{source}"))
        })?;
    if consumers.is_empty() {
        return Err(Error::TaskSet(
            "字段 `consumers` 为空：没有消费者的问题不默认常驻（§6.3）".to_owned(),
        ));
    }
    let validation_placeholder = required_str(object, "validation_placeholder")?;
    let lifecycle = required_str(object, "lifecycle")?;
    if lifecycle != "candidate" {
        return Err(Error::TaskSet(format!(
            "字段 `lifecycle` 必须是 `candidate`（新生成的候选还没验证），收到 `{lifecycle}`"
        )));
    }

    Ok(CandidateQuestion {
        refines,
        question,
        distinguishes,
        evidence,
        evidence_freshness,
        evidence_method,
        scope,
        trigger,
        consumers,
        validation_placeholder,
        lifecycle,
    })
}

fn required<'a>(object: &'a serde_json::Map<String, Value>, key: &str) -> Result<&'a Value, Error> {
    object
        .get(key)
        .ok_or_else(|| Error::TaskSet(format!("生成输出缺少字段 `{key}`（§6.3 的八项要一次给全）")))
}

fn required_str(object: &serde_json::Map<String, Value>, key: &str) -> Result<String, Error> {
    let value = required(object, key)?;
    let text = value
        .as_str()
        .ok_or_else(|| Error::TaskSet(format!("字段 `{key}` 不是字符串")))?;
    if text.trim().is_empty() {
        return Err(Error::TaskSet(format!("字段 `{key}` 是空串")));
    }
    Ok(text.to_owned())
}

fn preview(text: &str) -> String {
    text.chars().take(200).collect()
}

/// 契约与消费协议的核对结果（可核的约束点之一）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContractCheck {
    /// 是否全部通过。
    pub passed: bool,
    /// 逐条结果：名字、是否通过、依据。
    pub checks: Vec<ContractItem>,
}

/// 一条契约核对。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContractItem {
    /// 判据名。
    pub name: String,
    /// 是否通过。
    pub passed: bool,
    /// 依据说明。
    pub detail: String,
}

/// 核对候选是否满足 §6.3 契约与 §6.4 的固定消费协议。
///
/// `allowed_consumers` 与 `observables` 由调用方从任务族（冻结的协议）给出：
/// 消费者必须在声明过的集合里（否则等于换消费者），证据对象必须在可观测清单里（否则要求的证据
/// 根本取不到），并且必须把该对的关键证据对象包含进来（这正是精化的目标）。
pub fn check_contract(
    family: &FamilyB,
    pair: &crate::family_b::Pair,
    candidate: &CandidateQuestion,
    allowed_consumers: &BTreeSet<String>,
) -> ContractCheck {
    let mut checks = Vec::new();
    let mut push = |name: &str, passed: bool, detail: String| {
        checks.push(ContractItem {
            name: name.to_owned(),
            passed,
            detail,
        });
    };

    push(
        "refines_the_gated_question",
        candidate.refines == pair.question,
        format!(
            "必须精化被门控的问题 `{}`（统一字段），收到 `{}`",
            pair.question, candidate.refines
        ),
    );

    let fixed = family.question(&pair.question).expect("固定问题存在");
    let label_check = match (&fixed.question, &candidate.question) {
        (Question::Choice(fixed), Question::Choice(refined)) => {
            let left: BTreeSet<&String> = fixed.criteria.keys().collect();
            let right: BTreeSet<&String> = refined.criteria.keys().collect();
            let same = left == right;
            (
                same,
                format!(
                    "选项标签集合必须原样保留（前提门按标签判）：固定 {left:?}，候选 {right:?}"
                ),
            )
        }
        (fixed, refined) => (
            false,
            format!(
                "命题与类型必须与门控问题同型（决策器的门与答案原语要对得上）：固定 {}，候选 {}",
                question_kind(fixed),
                question_kind(refined)
            ),
        ),
    };
    push(
        "keeps_the_control_rule_intact",
        label_check.0,
        label_check.1,
    );

    let observables: BTreeSet<&ObjectId> = family
        .observables
        .iter()
        .map(|observable| &observable.object)
        .collect();
    let unknown: Vec<String> = candidate
        .evidence
        .keys()
        .filter(|object| !observables.contains(object))
        .map(|object| object.to_string())
        .collect();
    push(
        "evidence_is_observable",
        unknown.is_empty(),
        if unknown.is_empty() {
            format!(
                "证据需求里的 {} 个对象都在可观测清单里",
                candidate.evidence.len()
            )
        } else {
            format!("这些对象不在可观测清单里：{}", unknown.join("、"))
        },
    );
    push(
        "reads_the_key_evidence",
        candidate.evidence.contains_key(&pair.key_object),
        format!(
            "候选的证据需求必须包含关键证据对象 `{}`（精化的目标就是把它读进来）",
            pair.key_object
        ),
    );

    let foreign: Vec<&String> = candidate
        .consumers
        .iter()
        .filter(|consumer| !allowed_consumers.contains(*consumer))
        .collect();
    push(
        "consumers_within_the_declared_set",
        foreign.is_empty(),
        if foreign.is_empty() {
            format!("消费者 {:?} 都在声明集合里", candidate.consumers)
        } else {
            format!("这些消费者不在声明集合里：{foreign:?}")
        },
    );

    push(
        "evidence_requirement_complete",
        !candidate.evidence_freshness.trim().is_empty()
            && !candidate.evidence_method.trim().is_empty(),
        format!(
            "证据需求的时效与获取方法都要写明（§6.3）：时效 `{}`、方法 `{}`",
            candidate.evidence_freshness, candidate.evidence_method
        ),
    );
    push(
        "validation_placeholder_present",
        !candidate.validation_placeholder.trim().is_empty(),
        format!("验证记录占位：`{}`", candidate.validation_placeholder),
    );

    ContractCheck {
        passed: checks.iter().all(|check| check.passed),
        checks,
    }
}

/// 一次生成调用要给的开发材料（不含未见变体的真值）。
pub struct GenerationRequest<'a> {
    /// 属于哪一对。
    pub pair: &'a crate::family_b::Pair,
    /// 该对的固定问题定义。
    pub fixed_question: &'a Question,
    /// 该对的开发案例标识。
    pub dev_case: &'a str,
    /// 开发案例的失败现场（真值 → 实际读法 → 后果，含反例记录）。
    pub failure_facts: Vec<String>,
    /// 该对的反例记录。
    pub counterexamples: Vec<Counterexample>,
}

/// 生成提示词：输出契约 + 约束 + 开发材料（材料里没有未见变体的真值）。
pub fn prompt(family: &FamilyB, request: &GenerationRequest<'_>) -> Result<String, Error> {
    let pair = request.pair;
    let observables = family
        .observables
        .iter()
        .map(|observable| {
            serde_json::json!({
                "object": observable.object,
                "obtained_by": observable.obtained_by,
                "fields": observable.fields,
                "note": observable.note,
            })
        })
        .collect::<Vec<_>>();
    let remedies: Vec<Value> = pair
        .sides
        .iter()
        .map(|side| {
            serde_json::json!({
                "label": side,
                "action": pair.remedies.get(side),
            })
        })
        .collect();
    let counterexamples: Vec<Value> = request
        .counterexamples
        .iter()
        .map(|entry| {
            serde_json::json!({
                "id": entry.id.to_string(),
                "source": entry.source,
                "question": entry.question,
                "question_version": entry.question_version,
                "action": entry.action,
                "control_rule": entry.control_rule,
                "attributions": entry.attributions,
                "note": entry.note,
            })
        })
        .collect();

    let material = serde_json::json!({
        "pair": {"id": pair.id, "title": pair.title, "key_object": pair.key_object},
        "fixed_question": {"id": pair.question, "definition": request.fixed_question},
        "development_case": request.dev_case,
        "failure_facts": request.failure_facts,
        "counterexamples": counterexamples,
        "observables": observables,
        "actions": remedies,
    });
    let material = serde_json::to_string_pretty(&material)?;

    Ok(format!(
        "你是 Agent Harness 问题集合精化流程里的生成模型。任务：为下面这一对场景提出**一个**候选问题，\
         让它能把两侧区分开，并指出需要的证据。\n\
         \n\
         只输出一个 JSON 对象：不要解释、不要代码块、不要多余文本。输出契约（顶层键一个都不能少）：\n\
         {{\n\
         \x20 \"refines\": \"<要精化的问题 ID；必须等于 fixed_question.id>\",\n\
         \x20 \"question\": {{\"type\": \"choice\", \"instructions\": \"<要做出的判断>\", \
         \"criteria\": {{\"<标签>\": \"<该标签的判据>\"}} }},\n\
         \x20 \"distinguishes\": \"<这道题要区分哪些会导致不同动作的情形>\",\n\
         \x20 \"evidence\": {{\"<对象名>\": {{\"kind\": \"all\"}} 或 {{\"kind\": \"named\", \"fields\": [\"..\"]}}}},\n\
         \x20 \"evidence_freshness\": \"<这份证据要多新才算数>\",\n\
         \x20 \"evidence_method\": \"<怎么取得：可观测清单里的 initial 或某个观察动作>\",\n\
         \x20 \"scope\": \"<对哪些任务、工具与环境版本适用>\",\n\
         \x20 \"trigger\": \"<何时值得评估、何时可跳过>\",\n\
         \x20 \"consumers\": [\"<消费它的策略标签>\", ...],\n\
         \x20 \"validation_placeholder\": \"<验证记录待填：说明还没有读数>\",\n\
         \x20 \"lifecycle\": \"candidate\"\n\
         }}\n\
         \n\
         约束（违反任一条这个候选都会被拒）：\n\
         - `refines` 必须等于固定问题 ID；命题与类型必须同为 choice，且**选项标签集合必须原样保留**——\
         决策器的前提门按标签判，改标签等于改控制规则（固定下游消费协议）；\n\
         - `evidence` 的对象名只能取自材料里的可观测清单，且必须包含 `key_object`（这次精化的目标就是\
         把它读进来）；`evidence_method` 要说明它靠初始快照还是哪个观察动作取得；\n\
         - `consumers` 不能为空，取值只能是 `[\"strategy:pair-decision\"]`；\n\
         - `lifecycle` 只能是 `candidate`；`validation_placeholder` 不能为空。\n\
         \n\
         材料（开发案例的失败现场；未见变体的真值不会提供给你）：\n{material}"
    ))
}

/// 一次生成调用的读数（进调用记录与成本账）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenerationCall {
    /// 属于哪一对。
    pub pair: String,
    /// 生成用的模型（通道响应里的模型 ID）。
    pub version: Option<String>,
    /// 通道侧请求 id。
    pub request_id: Option<String>,
    /// 上报的 token 用量。
    pub usage: Option<jh_core::Usage>,
    /// 提示词字符数。
    pub input_bytes: u64,
    /// 输出字符数。
    pub output_bytes: u64,
    /// 耗时。
    pub latency_ms: u64,
    /// 失败原文；成功时为 `None`。
    pub error: Option<String>,
}

/// 一次生成的结果：候选（或失败原文）+ 调用读数。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeneratedCandidate {
    /// 属于哪一对。
    pub pair: String,
    /// 调用读数。
    pub call: GenerationCall,
    /// 解析成功的候选：`Err` 是失败原文（失败如实记录，不重试）。
    pub candidate: Option<CandidateQuestion>,
}

/// 生成器：真实模型通道（与 LLM 判断同一套环境变量与端点）。
pub struct Generator {
    client: Client,
}

impl Generator {
    /// 从环境变量构造：`OPENAI_API_KEY`、`OPENAI_MODEL`、`OPENAI_BASE_URL` 三者都必填。
    pub fn from_env() -> Result<Self, Error> {
        Ok(Self {
            client: from_env_client()?,
        })
    }

    /// 逐对生成候选：每对一次调用；解析失败算这一次生成失败（保留读数，不重试）。
    pub async fn generate(
        &self,
        family: &FamilyB,
        requests: &[GenerationRequest<'_>],
    ) -> Result<Vec<GeneratedCandidate>, Error> {
        let mut out = Vec::with_capacity(requests.len());
        for request in requests {
            let text = prompt(family, request)?;
            let input_bytes = text.len() as u64;
            let outcome = complete(&self.client, text).await;
            out.push(finish(request.pair.id.clone(), input_bytes, outcome));
        }
        Ok(out)
    }
}

fn finish(
    pair: String,
    input_bytes: u64,
    outcome: Result<Completion, jh_core::Error>,
) -> GeneratedCandidate {
    let pair_name = pair.clone();
    match outcome {
        Ok(completion) => {
            let parsed = parse_candidate(&completion.text);
            GeneratedCandidate {
                pair: pair_name.clone(),
                call: GenerationCall {
                    pair: pair_name,
                    version: Some(completion.model),
                    request_id: Some(completion.request_id),
                    usage: completion.usage,
                    input_bytes,
                    output_bytes: completion.text.len() as u64,
                    latency_ms: completion.latency.as_millis() as u64,
                    error: parsed.as_ref().err().map(ToString::to_string),
                },
                candidate: parsed.ok(),
            }
        }
        Err(error) => GeneratedCandidate {
            pair: pair_name.clone(),
            call: GenerationCall {
                pair: pair_name,
                version: None,
                request_id: None,
                usage: None,
                input_bytes,
                output_bytes: 0,
                latency_ms: 0,
                error: Some(error.to_string()),
            },
            candidate: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WELL_FORMED: &str = r#"{
        "refines": "b_pair_cause_p1",
        "question": {
            "type": "choice",
            "instructions": "证书窗口与对时结果说明哪一类原因？",
            "criteria": {
                "expired_credential": "证书窗口已过期",
                "clock_skew": "系统时钟偏差超窗",
                "other": "未列入候选集：材料不足以归入以上任何一类"
            }
        },
        "distinguishes": "证书过期与时钟偏差：两者的补救动作不同",
        "evidence": {
            "app.log": {"kind": "all"},
            "credential.audit": {"kind": "named", "fields": ["summary"]}
        },
        "evidence_freshness": "观察动作产出的最新一版",
        "evidence_method": "初始快照或观察动作 probe-credential-audit",
        "scope": "任务族 B 的 P1 对、probe 工具、环境版本 1",
        "trigger": "app.log 里出现握手失败时评估",
        "consumers": ["strategy:pair-decision"],
        "validation_placeholder": "验证记录待填：尚无开发案例读数",
        "lifecycle": "candidate"
    }"#;

    #[test]
    fn a_well_formed_reply_parses_into_the_eight_fields() {
        let candidate = parse_candidate(WELL_FORMED).expect("应能解析");
        assert_eq!(candidate.refines, QuestionId::from("b_pair_cause_p1"));
        assert_eq!(candidate.lifecycle, "candidate");
        assert_eq!(candidate.reads().len(), 2);
        assert_eq!(
            candidate.evidence[&ObjectId::from("credential.audit")],
            FieldSelection::Named {
                fields: BTreeSet::from(["summary".to_owned()])
            }
        );
        assert_eq!(
            candidate.consumers,
            BTreeSet::from(["strategy:pair-decision".to_owned()])
        );
        let new_question = candidate.to_new_question();
        assert_eq!(new_question.consumers.len(), 1);
    }

    #[test]
    fn a_missing_field_fails_and_names_itself() {
        let mut value: Value = serde_json::from_str(WELL_FORMED).unwrap();
        value.as_object_mut().unwrap().remove("evidence_freshness");
        let error = parse_candidate(&value.to_string()).expect_err("缺字段必须显式失败");
        assert!(error.to_string().contains("evidence_freshness"), "{error}");
    }

    #[test]
    fn a_missing_placeholder_or_a_wrong_lifecycle_fails() {
        let mut value: Value = serde_json::from_str(WELL_FORMED).unwrap();
        value.as_object_mut().unwrap().insert(
            "validation_placeholder".to_owned(),
            Value::String(String::new()),
        );
        let error = parse_candidate(&value.to_string()).expect_err("占位缺失必须显式失败");
        assert!(
            error.to_string().contains("validation_placeholder"),
            "{error}"
        );

        let mut value: Value = serde_json::from_str(WELL_FORMED).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("lifecycle".to_owned(), Value::String("enabled".to_owned()));
        let error = parse_candidate(&value.to_string()).expect_err("生命周期不合法必须显式失败");
        assert!(error.to_string().contains("candidate"), "{error}");
    }

    #[test]
    fn an_empty_consumer_set_or_empty_evidence_fails() {
        let mut value: Value = serde_json::from_str(WELL_FORMED).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("consumers".to_owned(), Value::Array(Vec::new()));
        let error = parse_candidate(&value.to_string()).expect_err("没有消费者必须显式失败");
        assert!(error.to_string().contains("consumers"), "{error}");

        let mut value: Value = serde_json::from_str(WELL_FORMED).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("evidence".to_owned(), Value::Object(Default::default()));
        let error = parse_candidate(&value.to_string()).expect_err("证据需求为空必须显式失败");
        assert!(error.to_string().contains("evidence"), "{error}");
    }

    #[test]
    fn non_json_output_fails_with_a_preview() {
        let error = parse_candidate("这是解释文本").expect_err("非 JSON 必须显式失败");
        assert!(error.to_string().contains("不是约定的 JSON"), "{error}");
    }

    #[test]
    fn the_contract_check_catches_protocol_violations() {
        let family = FamilyB::load().unwrap();
        let pair = family.pair("P1").unwrap();
        let allowed = BTreeSet::from(["strategy:pair-decision".to_owned()]);
        let candidate = parse_candidate(WELL_FORMED).unwrap();

        let check = check_contract(&family, pair, &candidate, &allowed);
        assert!(check.passed, "{check:?}");

        // 换一个不存在的证据对象：可观测性不通过。
        let mut broken = candidate.clone();
        broken
            .evidence
            .insert(ObjectId::from("never.declared"), FieldSelection::All);
        let check = check_contract(&family, pair, &broken, &allowed);
        assert!(!check.passed);
        assert!(
            check
                .checks
                .iter()
                .any(|item| item.name == "evidence_is_observable" && !item.passed)
        );

        // 换标签：等于改控制规则。
        let mut broken = candidate.clone();
        broken.question = Question::Choice(jh_core::ChoiceQuestion {
            instructions: "换一套标签".to_owned(),
            criteria: BTreeMap::from([("renamed".to_owned(), None)]),
        });
        let check = check_contract(&family, pair, &broken, &allowed);
        assert!(
            check
                .checks
                .iter()
                .any(|item| item.name == "keeps_the_control_rule_intact" && !item.passed)
        );

        // 消费者越界。
        let mut broken = candidate.clone();
        broken.consumers = BTreeSet::from(["strategy:other".to_owned()]);
        let check = check_contract(&family, pair, &broken, &allowed);
        assert!(
            check
                .checks
                .iter()
                .any(|item| item.name == "consumers_within_the_declared_set" && !item.passed)
        );

        // 精化别的问题。
        let mut broken = candidate;
        broken.refines = QuestionId::from("b_pair_cause_p2");
        let check = check_contract(&family, pair, &broken, &allowed);
        assert!(
            check
                .checks
                .iter()
                .any(|item| item.name == "refines_the_gated_question" && !item.passed)
        );
    }
}
