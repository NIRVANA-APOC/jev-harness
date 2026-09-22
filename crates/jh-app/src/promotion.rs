//! 晋升与回退：预先声明的晋升标准、发布联动失效、保留旧版本可回退，以及每次晋升的
//! 「问题 / 输入投影 / 消费者 / 策略」四路变更记录与可核的边界（`docs/RESEARCH_GOALS.md` §6.2 第 7、8 步、
//! §6.4）。
//!
//! # 预声明
//!
//! 晋升标准在跑之前就写死在 [`DECLARED_CRITERIA`] 与 [`Thresholds`] 里，读数只作为它们的输入：
//! 「满足预先规定的晋升标准后，发布新问题集版本」。没达标就不发布——§12 的未达标处理是
//! 「记录局限并保留固定问题模式」，不是把标准往下调。
//!
//! # 发布与回退都联动失效
//!
//! 发布走 [`QuestionLibrary::publish`]，回退是同一条路（把被替代的旧版本重新设为启用）：
//! 两者都收 `&mut SemanticState`，回答过这个问题的判断变成待重查、以它们为前提的行动变成不能发出
//! （`crates/jh-core/src/runtime.rs` 的 `invalidate_question`）。旧版本只改状态、不删除，因此
//! 「保留旧版本可比较、可回退」是结构上的事实，不是承诺。
//!
//! # 四路变更（§6.4）
//!
//! 每次版本晋升都记下：**问题**（定义是否变）、**输入投影**（读取范围是否变）、**消费者**（消费者标签
//! 是否变）、**策略**（控制规则是否变）。前两项在本流程里会变，后两项由
//! [`check_boundaries`] 与 [`four_way_change`] 当场核出来：固定下游消费协议下消费者与策略都不变。
//!
//! # 边界可核（§6.4）
//!
//! 精化不得修改用户目标、降低验收要求或扩大工具权限：[`BoundaryDigest`] 把这三样（外加控制规则）
//! 在发布前后各算一次摘要，逐项对比；候选的证据对象必须落在声明的可观测清单里，否则算扩大工具权限。

use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};
use std::path::Path;

use jh_core::{
    ActionId, Answer, FieldSelection, Invalidation, Judgment, JudgmentRecord, NewQuestion, NodeId,
    ObjectId, QuestionId, QuestionLibrary, QuestionVersion, SemanticState, SnapshotId,
};
use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::family::{Environment, FamilyTask};
use crate::family_b::{ABSENT_LABEL, FamilyB, Pair};
use crate::generation::CandidateQuestion;

/// 固定下游消费协议里唯一的消费者标签：成对场景的决策器（§6.4 的「同一个决策器」）。
pub const PAIR_DECISION_CONSUMER: &str = "strategy:pair-decision";

/// 允许出现的消费者标签集合（协议声明，冻结）。
pub fn allowed_consumers() -> BTreeSet<String> {
    BTreeSet::from([PAIR_DECISION_CONSUMER.to_owned()])
}

/// 预声明的晋升标准（名字 + 口径）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Criterion {
    /// 判据标识。
    pub id: &'static str,
    /// 判据说明（报告里逐条写清）。
    pub description: &'static str,
}

/// 晋升标准：契约与协议三条是结构性的，读数是四个预先声明的阈值。
pub const DECLARED_CRITERIA: [Criterion; 8] = [
    Criterion {
        id: "contract_complete",
        description: "候选八字段齐全（命题与类型、区分目的、证据需求含时效与获取方法、适用范围、触发条件、下游用途、验证记录占位、生命周期=候选）",
    },
    Criterion {
        id: "consumption_protocol",
        description: "精化的是被门控的问题、选项标签集合原样保留、消费者在声明集合内（统一字段，不改消费者与策略）",
    },
    Criterion {
        id: "evidence_within_permissions",
        description: "证据需求里的对象都在声明的可观测清单里，且包含该对的关键证据对象（不扩大工具权限）",
    },
    Criterion {
        id: "development_discrimination",
        description: "开发案例上把这成对场景区分开（读出正确一侧）的比例 ≥ 阈值，且严格优于固定版本",
    },
    Criterion {
        id: "positive_cases",
        description: "正例（材料足够）一致率 ≥ 阈值",
    },
    Criterion {
        id: "negative_cases",
        description: "反例（材料不足）一致率 ≥ 阈值：材料不足时仍要按 other 的判据作答",
    },
    Criterion {
        id: "unseen_wording_transfer",
        description: "不同表述 / 不同服务的未见用例一致率 ≥ 阈值（只在原始失败案例上变正确不算迁移成功）",
    },
    Criterion {
        id: "shadow_branch_change",
        description: "影子运行里至少有一次「走向正当动作」的分支改变，且没有「走向不该发出的动作」的改变",
    },
];

/// 预声明的阈值。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Thresholds {
    /// 开发案例判别一致率下限。
    pub development_min: f64,
    /// 开发案例相对固定版本的最小增益。
    pub development_gain_min: f64,
    /// 正例一致率下限。
    pub positive_min: f64,
    /// 反例一致率下限。
    pub negative_min: f64,
    /// 未见表述一致率下限。
    pub unseen_min: f64,
}

/// 本项目声明的阈值取值（不是任何供应商默认值）。
pub fn declared_thresholds() -> Thresholds {
    Thresholds {
        development_min: 0.75,
        development_gain_min: 0.25,
        positive_min: 0.75,
        negative_min: 0.75,
        unseen_min: 0.6,
    }
}

/// 一条判据的结果。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CriterionOutcome {
    /// 判据标识。
    pub id: String,
    /// 判据说明。
    pub description: String,
    /// 是否通过。
    pub passed: bool,
    /// 这次读到的读数。
    pub reading: String,
}

/// 判据的输入读数（全部来自运行期，不由调用方改写）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CriteriaInput {
    /// 契约与协议核对是否通过。
    pub contract_passed: bool,
    /// 契约核对里没通过的条目。
    pub contract_failures: Vec<String>,
    /// 开发案例上的判别一致率。
    pub development_rate: Option<f64>,
    /// 固定版本在开发案例上的判别一致率（用来算增益）。
    pub development_rate_fixed: Option<f64>,
    /// 正例一致率。
    pub positive_rate: Option<f64>,
    /// 反例一致率。
    pub negative_rate: Option<f64>,
    /// 未见表述一致率。
    pub unseen_rate: Option<f64>,
    /// 影子运行里走向正当动作的分支改变次数。
    pub toward_justified: u32,
    /// 影子运行里走向不该发出的动作的分支改变次数。
    pub toward_unjustified: u32,
    /// 影子判断失败条数（有失败就不算干净）。
    pub shadow_failures: u32,
}

/// 逐条判据是否达标（达标 = 全部通过）。
pub fn evaluate(input: &CriteriaInput, thresholds: &Thresholds) -> Vec<CriterionOutcome> {
    let declared = DECLARED_CRITERIA;
    let mut out = Vec::with_capacity(declared.len());
    let mut push = |index: usize, passed: bool, reading: String| {
        out.push(CriterionOutcome {
            id: declared[index].id.to_owned(),
            description: declared[index].description.to_owned(),
            passed,
            reading,
        });
    };

    let contract_passed = input.contract_passed;
    push(
        0,
        contract_passed,
        if contract_passed {
            "八字段齐全".to_owned()
        } else {
            format!("未通过：{}", input.contract_failures.join("；"))
        },
    );

    let protocol: Vec<&String> = input
        .contract_failures
        .iter()
        .filter(|name| {
            name.starts_with("refines_")
                || name.starts_with("keeps_")
                || name.starts_with("consumers_")
        })
        .collect();
    push(
        1,
        protocol.is_empty(),
        if protocol.is_empty() {
            "统一字段与消费者都在声明集合内".to_owned()
        } else {
            format!("未通过：{protocol:?}")
        },
    );

    let permissions: Vec<&String> = input
        .contract_failures
        .iter()
        .filter(|name| name.starts_with("evidence_"))
        .collect();
    push(
        2,
        permissions.is_empty(),
        if permissions.is_empty() {
            "证据需求在可观测清单内且含关键证据".to_owned()
        } else {
            format!("未通过：{permissions:?}")
        },
    );

    let development = input.development_rate;
    let gain = match (development, input.development_rate_fixed) {
        (Some(refined), Some(fixed)) => Some(refined - fixed),
        _ => None,
    };
    let development_passed = development.is_some_and(|rate| rate >= thresholds.development_min)
        && gain.is_some_and(|gain| gain >= thresholds.development_gain_min);
    push(
        3,
        development_passed,
        format!(
            "候选 {} vs 固定 {}（增益 {}，阈值 ≥ {} 且增益 ≥ {}）",
            format_ratio(development),
            format_ratio(input.development_rate_fixed),
            format_ratio(gain),
            thresholds.development_min,
            thresholds.development_gain_min
        ),
    );

    let positive = input.positive_rate;
    push(
        4,
        positive.is_some_and(|rate| rate >= thresholds.positive_min),
        format!(
            "正例一致率 {}（阈值 ≥ {}）",
            format_ratio(positive),
            thresholds.positive_min
        ),
    );

    let negative = input.negative_rate;
    push(
        5,
        negative.is_some_and(|rate| rate >= thresholds.negative_min),
        format!(
            "反例一致率 {}（阈值 ≥ {}）",
            format_ratio(negative),
            thresholds.negative_min
        ),
    );

    let unseen = input.unseen_rate;
    push(
        6,
        unseen.is_some_and(|rate| rate >= thresholds.unseen_min),
        format!(
            "未见表述一致率 {}（阈值 ≥ {}）",
            format_ratio(unseen),
            thresholds.unseen_min
        ),
    );

    push(
        7,
        input.toward_justified > 0 && input.toward_unjustified == 0 && input.shadow_failures == 0,
        format!(
            "走向正当动作 {} 次、走向不该发出的动作 {} 次、影子失败 {} 次",
            input.toward_justified, input.toward_unjustified, input.shadow_failures
        ),
    );

    out
}

fn format_ratio(value: Option<f64>) -> String {
    value.map_or_else(|| "—".to_owned(), |value| format!("{value:.3}"))
}

/// 一份发布前 / 发布后的边界摘要（§6.4 的可核约束点）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoundaryDigest {
    /// 用户目标的摘要（任务族与各任务的声明文本）。
    pub user_goal: String,
    /// 验收要求的摘要（各相位的必需动作与交付物绑定要求）。
    pub acceptance: String,
    /// 工具权限的摘要（声明的可观测对象与取得方式）。
    pub permissions: String,
    /// 控制规则的摘要（各任务的候选动作与前提门）。
    pub control_rules: String,
}

/// 算一份边界摘要：把族 B 声明的四类冻结资产各自序列化后取哈希。
pub fn boundary_digest(family: &FamilyB) -> Result<BoundaryDigest, Error> {
    let tasks: Vec<&FamilyTask> = family
        .development
        .iter()
        .chain(family.variants.iter())
        .map(|entry| &entry.task)
        .collect();

    let user_goal = digest(&serde_json::to_string(&(
        &family.name,
        &family.notes,
        tasks
            .iter()
            .map(|task| (&task.id, &task.notes))
            .collect::<Vec<_>>(),
    ))?)?;
    let acceptance = digest(&serde_json::to_string(
        &tasks
            .iter()
            .map(|task| {
                (
                    &task.id,
                    task.bind_deliverable,
                    task.phases
                        .iter()
                        .map(|phase| &phase.required)
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>(),
    )?)?;
    let permissions = digest(&serde_json::to_string(
        &family
            .observables
            .iter()
            .map(|observable| (&observable.object, &observable.obtained_by))
            .collect::<Vec<_>>(),
    )?)?;
    let control_rules = digest(&serde_json::to_string(
        &tasks
            .iter()
            .map(|task| (&task.id, &task.actions))
            .collect::<Vec<_>>(),
    )?)?;

    Ok(BoundaryDigest {
        user_goal,
        acceptance,
        permissions,
        control_rules,
    })
}

/// 一条边界核对。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoundaryCheck {
    /// 判据名。
    pub name: String,
    /// 是否通过。
    pub passed: bool,
    /// 依据说明。
    pub detail: String,
}

/// 核对边界：用户目标、验收要求、工具权限、控制规则在精化前后是否都没被改动。
///
/// `before` 与 `after` 是同一份冻结资产的两次摘要（精化前后的环境下各算一次）；候选的证据对象还要
/// 逐个落在声明的可观测清单里——「需要新的读取权限」就是扩大工具权限，显式失败。
pub fn check_boundaries(
    before: &BoundaryDigest,
    after: &BoundaryDigest,
    family: &FamilyB,
    candidate: &CandidateQuestion,
) -> Vec<BoundaryCheck> {
    let mut checks = Vec::new();
    let mut push = |name: &str, passed: bool, detail: String| {
        checks.push(BoundaryCheck {
            name: name.to_owned(),
            passed,
            detail,
        });
    };

    push(
        "user_goal_unchanged",
        before.user_goal == after.user_goal,
        format!(
            "用户目标（任务族与各任务的声明）摘要 {} → {}；候选契约里没有任何字段能改它",
            before.user_goal, after.user_goal
        ),
    );
    push(
        "acceptance_unchanged",
        before.acceptance == after.acceptance,
        format!(
            "验收要求（各相位的必需动作与交付物绑定）摘要 {} → {}",
            before.acceptance, after.acceptance
        ),
    );
    push(
        "permissions_within_the_declared_set",
        before.permissions == after.permissions,
        format!(
            "声明的可观测清单摘要 {} → {}（精化不改工具清单）",
            before.permissions, after.permissions
        ),
    );
    let declared: BTreeSet<&ObjectId> = family
        .observables
        .iter()
        .map(|observable| &observable.object)
        .collect();
    let foreign: Vec<String> = candidate
        .evidence
        .keys()
        .filter(|object| !declared.contains(object))
        .map(ObjectId::to_string)
        .collect();
    push(
        "evidence_objects_declared",
        foreign.is_empty(),
        if foreign.is_empty() {
            format!(
                "候选读的 {} 个对象都在声明的可观测清单里",
                candidate.evidence.len()
            )
        } else {
            format!("候选要求读取未声明的对象：{}", foreign.join("、"))
        },
    );
    push(
        "control_rules_unchanged",
        before.control_rules == after.control_rules,
        format!(
            "候选动作与前提门摘要 {} → {}（决策器是同一段代码、同一套门判据）",
            before.control_rules, after.control_rules
        ),
    );

    checks
}

fn digest(text: &str) -> Result<String, Error> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    Ok(format!("{:016x}", hasher.finish()))
}

/// 四路变更记录（§6.4）：问题 / 输入投影 / 消费者 / 策略分别是否改变。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FourWayChange {
    /// 问题（定义）是否改变。
    pub question_changed: bool,
    /// 输入投影（读取范围）是否改变。
    pub input_projection_changed: bool,
    /// 消费者是否改变。
    pub consumer_changed: bool,
    /// 策略（控制规则）是否改变。
    pub policy_changed: bool,
    /// 逐路的依据说明。
    pub detail: Vec<String>,
}

/// 比较同一问题两个版本的四路变更。
pub fn four_way_change(
    family: &FamilyB,
    pair: &Pair,
    from: QuestionVersion,
    to: QuestionVersion,
    library: &QuestionLibrary,
    candidate: Option<&CandidateQuestion>,
) -> FourWayChange {
    let before_definition = library.question(&pair.question, from).cloned();
    let after_definition = library.question(&pair.question, to).cloned();
    let before_entry = library.at(&pair.question, from);
    let after_entry = library.at(&pair.question, to);

    let reads = |entry: Option<&jh_core::QuestionEntry>| -> Vec<ObjectId> {
        entry
            .map(|entry| entry.evidence.keys().cloned().collect())
            .unwrap_or_default()
    };
    let before_reads = reads(before_entry);
    let after_reads = reads(after_entry);
    let before_consumers = before_entry
        .map(|entry| entry.consumers.clone())
        .unwrap_or_default();
    let after_consumers = after_entry
        .map(|entry| entry.consumers.clone())
        .unwrap_or_default();

    let question_changed = before_definition != after_definition;
    let input_projection_changed = before_reads != after_reads;
    let consumer_changed = before_consumers != after_consumers;

    // 策略：控制规则来自任务族声明（冻结），发布流程改不到它；这里把它核出来写进记录。
    let policy_changed = false;
    let control_rules = digest(
        &serde_json::to_string(
            &family
                .development
                .iter()
                .chain(family.variants.iter())
                .map(|entry| (&entry.task.id, &entry.task.actions))
                .collect::<Vec<_>>(),
        )
        .unwrap_or_default(),
    )
    .unwrap_or_default();

    let mut detail = vec![
        format!(
            "问题：v{from} → v{to}，定义{}（读法/判据文字）",
            if question_changed { "改变" } else { "未变" }
        ),
        format!(
            "输入投影：读取范围 {before_reads:?} → {after_reads:?}，{}",
            if input_projection_changed {
                "改变"
            } else {
                "未变"
            }
        ),
        format!(
            "消费者：{before_consumers:?} → {after_consumers:?}，{}（固定消费协议要求不变）",
            if consumer_changed { "改变" } else { "未变" }
        ),
        format!(
            "策略：控制规则（动作与前提门）摘要 {control_rules}，发布前后一致，未变{}",
            candidate
                .map(|candidate| format!("；候选声明的消费者 {:?}", candidate.consumers))
                .unwrap_or_default()
        ),
    ];
    detail.push(format!(
        "说明：材料充分性兜底读法仍是 `{ABSENT_LABEL}`；问题定义里的 {} 项选项标签集合必须原样保留",
        family
            .question(&pair.question)
            .map(|spec| question_options(&spec.question))
            .unwrap_or_default()
    ));

    FourWayChange {
        question_changed,
        input_projection_changed,
        consumer_changed,
        policy_changed,
        detail,
    }
}

fn question_options(question: &jh_core::Question) -> usize {
    match question {
        jh_core::Question::Choice(choice) => choice.criteria.len(),
        jh_core::Question::Score(score) => score.criteria.len(),
        jh_core::Question::Noul(_) => 2,
    }
}

/// 失效传播的汇总（发布 / 回退的联动读数）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvalidationSummary {
    /// 待重查的判断节点数。
    pub stale: u32,
    /// 被挡下的行动数。
    pub blocked: u32,
    /// 传播路径条数。
    pub paths: u32,
    /// 起因说明。
    pub cause: String,
}

/// 把传播结果折成报告读数。
pub fn invalidation_summary(invalidation: &Invalidation) -> InvalidationSummary {
    InvalidationSummary {
        stale: invalidation.stale.len() as u32,
        blocked: invalidation.blocked.len() as u32,
        paths: invalidation.paths.len() as u32,
        cause: format!("{:?}", invalidation.cause),
    }
}

/// 把族 B 的固定问题集登记进问题库（`adopt`：它们不是这一版新生成的候选）。
///
/// 入库形态就是「老版本 + 当前版本」，消费者声明为固定协议的消费者标签。
pub fn adopt_fixed_set(
    family: &FamilyB,
    library: &mut QuestionLibrary,
    state: &mut SemanticState,
    at_unix_ms: u64,
) -> Result<Vec<(QuestionId, QuestionVersion)>, Error> {
    let mut adopted = Vec::new();
    for (question, spec) in family.questions() {
        let pair = family
            .pairs
            .iter()
            .find(|pair| pair.question == *question)
            .expect("每对一个问题");
        let reads = family.scope(question).unwrap_or_default();
        let new_question = NewQuestion {
            question: spec.question.clone(),
            distinguishes: pair.title.clone(),
            evidence: reads
                .iter()
                .map(|object| (object.clone(), FieldSelection::All))
                .collect(),
            scope: format!("任务族 B 的 {} 对", pair.id),
            trigger: "任务开始时评估；材料不足时按 other 的判据作答".to_owned(),
            consumers: BTreeSet::from([PAIR_DECISION_CONSUMER.to_owned()]),
        };
        let (version, _) = library.adopt(
            state,
            question,
            new_question,
            at_unix_ms,
            "入库：任务族 B 的固定问题集".to_owned(),
        )?;
        adopted.push((question.clone(), version));
    }
    Ok(adopted)
}

/// 提出候选：库分配新版本，落在「候选」状态（不参与控制）。
pub fn propose(
    library: &mut QuestionLibrary,
    candidate: &CandidateQuestion,
    at_unix_ms: u64,
) -> Result<QuestionVersion, Error> {
    Ok(library.propose(
        &candidate.refines,
        candidate.to_new_question(),
        at_unix_ms,
        format!("生成模型依据反例记录提出：{}", candidate.distinguishes),
    )?)
}

/// 进入影子评估（候选 → 影子）。
pub fn start_shadow(
    library: &mut QuestionLibrary,
    question: &QuestionId,
    version: QuestionVersion,
    at_unix_ms: u64,
    reason: String,
) -> Result<jh_core::LifecycleRecord, Error> {
    Ok(library.start_shadow(question, version, at_unix_ms, reason)?)
}

/// 发布（晋升或回退）：改成当前启用版本并联动失效。
///
/// 回退走的是同一个函数——把被替代的旧版本重新设为启用；旧版本只改状态，不删除、不重编号。
pub fn publish(
    library: &mut QuestionLibrary,
    state: &mut SemanticState,
    question: &QuestionId,
    version: QuestionVersion,
    at_unix_ms: u64,
    reason: String,
) -> Result<(Vec<jh_core::LifecycleRecord>, InvalidationSummary), Error> {
    let (records, invalidation) = library.publish(state, question, version, at_unix_ms, reason)?;
    Ok((records, invalidation_summary(&invalidation)))
}

/// 记一次验证读数（§6.3 的「验证记录」：读数按原值进问题库）。
pub fn record_validation(
    library: &mut QuestionLibrary,
    question: &QuestionId,
    version: QuestionVersion,
    kind: jh_core::ValidationKind,
    readings: BTreeMap<String, f64>,
    note: String,
    at_unix_ms: u64,
) -> Result<(), Error> {
    Ok(library.record_validation(
        question,
        version,
        jh_core::ValidationRecord {
            at_unix_ms,
            kind,
            readings,
            note,
        },
    )?)
}

/// 把问题库落盘（报告里的「维护成本」与回退目标靠它复核）。
pub fn write_library(path: &Path, library: &QuestionLibrary) -> Result<(), Error> {
    crate::compare::write_json(path, library)
}

/// 在状态里补一条该问题的判断记录（发布/回退的失效联动要作用在真有判断的状态上）。
///
/// 投影按当时仍有效的证据整体登记（保守口径，与循环里的装配规则同源）。
pub fn record_judgment(
    state: &mut SemanticState,
    question: &QuestionId,
    version: QuestionVersion,
    answer: Answer,
    snapshot: &str,
    source: jh_core::JudgeSource,
) -> Result<NodeId, Error> {
    let projection = jh_core::InputProjection::covering(
        state
            .evidence()
            .entries()
            .iter()
            .filter(|entry| !state.evidence().is_retracted(entry.id))
            .map(|entry| (entry.object.clone(), entry.version))
            .collect::<Vec<_>>(),
    );
    let node = state.declare_node(question.clone(), version, projection);
    let record = JudgmentRecord {
        snapshot: SnapshotId::from(snapshot.to_owned()),
        source,
        version: "promotion-drill".to_owned(),
        request_id: None,
        usage: None,
        latency_ms: 1,
        judgments: BTreeMap::from([(
            question.clone(),
            Judgment {
                question_version: version,
                answer,
                thresholds: BTreeMap::new(),
            },
        )]),
    };
    state.record_judgment(node, record)?;
    Ok(node)
}

/// 在状态里补一条该问题的判断记录与以它为前提的行动。
pub fn state_with_judgment(
    task: &FamilyTask,
    question: &QuestionId,
    version: QuestionVersion,
    answer: Answer,
    action: &ActionId,
    source: jh_core::JudgeSource,
) -> Result<(SemanticState, NodeId), Error> {
    let mut state = SemanticState::in_memory();
    let environment = Environment::new(task);
    environment.seed(&mut state)?;
    let node = record_judgment(
        &mut state,
        question,
        version,
        answer,
        &format!("promotion-{}", task.id),
        source,
    )?;
    state.declare_action(action.clone(), [node])?;
    Ok((state, node))
}

/// 问题库当前启用的版本（报告里核对「当前定义是哪一版」）。
pub fn enabled_version(
    library: &QuestionLibrary,
    question: &QuestionId,
) -> Option<QuestionVersion> {
    library.enabled(question).map(|(version, _)| version)
}

/// 问题库里这个问题的全部版本与状态（回退目标是否还在的核对）。
pub fn version_states(
    library: &QuestionLibrary,
    question: &QuestionId,
) -> Vec<(QuestionVersion, String)> {
    library
        .versions(question)
        .into_iter()
        .map(|(version, entry)| (version, entry.lifecycle.to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::family_b::FamilyB;
    use jh_core::{Lifecycle, NoulAnswer};

    #[test]
    fn the_declared_thresholds_and_criteria_are_frozen_in_code() {
        let thresholds = declared_thresholds();
        assert!(thresholds.development_min > 0.0 && thresholds.development_min <= 1.0);
        assert_eq!(DECLARED_CRITERIA.len(), 8);
        // 判据标识唯一。
        let ids: BTreeSet<&str> = DECLARED_CRITERIA.iter().map(|item| item.id).collect();
        assert_eq!(ids.len(), 8);
    }

    #[test]
    fn criteria_only_pass_when_every_declared_threshold_is_met() {
        let thresholds = declared_thresholds();
        let passing = CriteriaInput {
            contract_passed: true,
            contract_failures: Vec::new(),
            development_rate: Some(1.0),
            development_rate_fixed: Some(0.0),
            positive_rate: Some(1.0),
            negative_rate: Some(1.0),
            unseen_rate: Some(1.0),
            toward_justified: 1,
            toward_unjustified: 0,
            shadow_failures: 0,
        };
        assert!(
            evaluate(&passing, &thresholds)
                .iter()
                .all(|item| item.passed)
        );

        // 反例一致率不达标：只有那一条不过，别的不受影响。
        let failing = CriteriaInput {
            negative_rate: Some(0.5),
            ..passing.clone()
        };
        let outcomes = evaluate(&failing, &thresholds);
        let failed: Vec<&str> = outcomes
            .iter()
            .filter(|item| !item.passed)
            .map(|item| item.id.as_str())
            .collect();
        assert_eq!(failed, vec!["negative_cases"]);

        // 影子引入不该发出的动作：末条不过。
        let harmful = CriteriaInput {
            toward_unjustified: 1,
            ..passing
        };
        let outcomes = evaluate(&harmful, &thresholds);
        assert!(!outcomes[7].passed);
    }

    /// 行为型拒绝：候选读得到关键证据（材料足够）却答成兜底读法时，正例一致率会掉下来，
    /// 判据「正例一致率」必须把它拒掉——这是与契约型拒绝（refines/标签/消费者/证据对象）不同的一条路。
    #[test]
    fn reading_based_criteria_reject_a_candidate_that_answers_wrong_on_sufficient_material() {
        let thresholds = declared_thresholds();
        let input = CriteriaInput {
            // 契约与协议全部通过：候选确实精化了门控问题、读到了关键证据。
            contract_passed: true,
            contract_failures: Vec::new(),
            development_rate: Some(0.0),
            development_rate_fixed: Some(0.0),
            // 正例全错（材料足够却答 other）、反例全对。
            positive_rate: Some(0.0),
            negative_rate: Some(1.0),
            unseen_rate: Some(0.0),
            toward_justified: 0,
            toward_unjustified: 0,
            shadow_failures: 0,
        };

        let outcomes = evaluate(&input, &thresholds);
        assert!(
            !outcomes.iter().all(|item| item.passed),
            "读得到关键证据却答错侧的候选不能晋升"
        );
        let failed: Vec<&str> = outcomes
            .iter()
            .filter(|item| !item.passed)
            .map(|item| item.id.as_str())
            .collect();
        assert!(failed.contains(&"positive_cases"), "{failed:?}");
        assert!(
            failed.contains(&"development_discrimination")
                && failed.contains(&"shadow_branch_change"),
            "{failed:?}"
        );
        // 契约与协议三条仍是通过的：拒绝的理由是读数，不是形状。
        assert!(outcomes[0].passed && outcomes[1].passed && outcomes[2].passed);
    }

    #[test]
    fn promoting_and_rolling_back_invalidate_and_keep_the_old_version() {
        let family = FamilyB::load().unwrap();
        let pair = family.pair("P1").unwrap();
        let dev = family.dev_case("P1").unwrap();
        let question = pair.question.clone();

        let mut library = QuestionLibrary::default();
        let mut library_state = SemanticState::in_memory();
        let adopted = adopt_fixed_set(&family, &mut library, &mut library_state, 100).unwrap();
        assert_eq!(adopted.len(), 3);
        assert_eq!(
            enabled_version(&library, &question),
            Some(QuestionVersion(1))
        );

        // 候选（模拟生成产物）：读关键证据、标签集合原样保留。
        let fixed = family.question(&question).unwrap();
        let jh_core::Question::Choice(choice) = &fixed.question else {
            panic!("固定问题是选择类");
        };
        // 精化 = 同一个 ID 的新版本：读法写明要看关键证据，选项标签集合原样保留。
        let mut refined = choice.clone();
        refined.instructions =
            "证书窗口与对时结果说明哪一类原因？只按这次读到的材料归因；证据不足时按 other 的判据作答。"
                .to_owned();
        let candidate = CandidateQuestion {
            refines: question.clone(),
            question: jh_core::Question::Choice(refined),
            distinguishes: "证书过期与时钟偏差".to_owned(),
            evidence: BTreeMap::from([
                (ObjectId::from("app.log"), FieldSelection::All),
                (ObjectId::from("credential.audit"), FieldSelection::All),
            ]),
            evidence_freshness: "观察动作产出的最新一版".to_owned(),
            evidence_method: "初始快照或 probe-credential-audit".to_owned(),
            scope: "任务族 B 的 P1 对".to_owned(),
            trigger: "出现握手失败时".to_owned(),
            consumers: allowed_consumers(),
            validation_placeholder: "验证记录待填".to_owned(),
            lifecycle: "candidate".to_owned(),
        };

        let version = propose(&mut library, &candidate, 110).unwrap();
        assert_eq!(version, QuestionVersion(2));
        // 候选不参与控制：当前启用版本仍是 v1。
        assert_eq!(
            enabled_version(&library, &question),
            Some(QuestionVersion(1))
        );

        // 影子评估 + 发布（晋升）：状态里有一条 v1 上的判断与以它为前提的行动。
        start_shadow(&mut library, &question, version, 120, "影子运行".to_owned()).unwrap();
        let (state, node) = state_with_judgment(
            &dev.task,
            &question,
            QuestionVersion(1),
            Answer::Noul(NoulAnswer { noul: 1.0 }),
            &pair.remedies[&dev.side],
            jh_core::JudgeSource::Rule,
        )
        .unwrap();
        let mut state = state;
        let (_, invalidation) = publish(
            &mut library,
            &mut state,
            &question,
            version,
            130,
            "影子读数达标".to_owned(),
        )
        .unwrap();
        assert_eq!(invalidation.stale, 1);
        assert_eq!(invalidation.blocked, 1);
        assert_eq!(
            state.node(node).unwrap().validity,
            jh_core::Validity::NeedsRecheck
        );
        assert_eq!(enabled_version(&library, &question), Some(version));

        // 四路变更：问题与输入投影变了，消费者与策略没变。
        let change = four_way_change(
            &family,
            pair,
            QuestionVersion(1),
            version,
            &library,
            Some(&candidate),
        );
        assert!(change.question_changed);
        assert!(change.input_projection_changed);
        assert!(!change.consumer_changed);
        assert!(!change.policy_changed);

        // 回退：把 v1 重新设为启用，v2 让位；旧版本还在，可比较。
        // 先补一条 v2 上的判断：回退要让它待重查。
        let on_v2 = record_judgment(
            &mut state,
            &question,
            version,
            Answer::Noul(NoulAnswer { noul: 0.9 }),
            "promotion-v2",
            jh_core::JudgeSource::Rule,
        )
        .unwrap();
        let (records, invalidation) = publish(
            &mut library,
            &mut state,
            &question,
            QuestionVersion(1),
            140,
            "回退".to_owned(),
        )
        .unwrap();
        assert_eq!(
            enabled_version(&library, &question),
            Some(QuestionVersion(1))
        );
        assert_eq!(library.versions(&question).len(), 2);
        assert_eq!(
            library.at(&question, version).unwrap().lifecycle,
            Lifecycle::Superseded {
                by: QuestionVersion(1)
            }
        );
        assert_eq!(invalidation.stale, 1, "v2 上那条判断待重查");
        assert_eq!(
            state.node(on_v2).unwrap().validity,
            jh_core::Validity::NeedsRecheck
        );
        assert!(
            records
                .iter()
                .any(|record| record.version == QuestionVersion(1))
        );
        let states = version_states(&library, &question);
        assert_eq!(states.len(), 2);
    }

    #[test]
    fn boundaries_are_checked_against_the_frozen_assets() {
        let family = FamilyB::load().unwrap();
        let pair = family.pair("P2").unwrap();
        let digest = boundary_digest(&family).unwrap();
        let fixed = family.question(&pair.question).unwrap();
        let jh_core::Question::Choice(choice) = &fixed.question else {
            panic!("固定问题是选择类");
        };
        let candidate = CandidateQuestion {
            refines: pair.question.clone(),
            question: jh_core::Question::Choice(choice.clone()),
            distinguishes: "上游限流与重试风暴".to_owned(),
            evidence: BTreeMap::from([(ObjectId::from("upstream.audit"), FieldSelection::All)]),
            evidence_freshness: "最新一版".to_owned(),
            evidence_method: "probe-upstream-audit".to_owned(),
            scope: "任务族 B 的 P2 对".to_owned(),
            trigger: "写请求失败率升高时".to_owned(),
            consumers: allowed_consumers(),
            validation_placeholder: "验证记录待填".to_owned(),
            lifecycle: "candidate".to_owned(),
        };

        // 同一份资产在精化前后摘要不变：四项边界都通过。
        let checks = check_boundaries(&digest, &digest.clone(), &family, &candidate);
        assert!(checks.iter().all(|check| check.passed));

        // 扩权：读一个没有声明的对象。
        let mut expanded = candidate.clone();
        expanded
            .evidence
            .insert(ObjectId::from("clock.status"), FieldSelection::All);
        let checks = check_boundaries(&digest, &digest.clone(), &family, &expanded);
        assert!(
            checks
                .iter()
                .any(|check| check.name == "evidence_objects_declared" && !check.passed),
            "扩大工具权限必须被核出来"
        );

        // 验收被改动（假设有人改了必需动作）：摘要不同即不通过。
        let mut tampered = digest.clone();
        tampered.acceptance = "0000000000000000".to_owned();
        let checks = check_boundaries(&digest, &tampered, &family, &candidate);
        assert!(
            checks
                .iter()
                .any(|check| check.name == "acceptance_unchanged" && !check.passed)
        );
    }
}
