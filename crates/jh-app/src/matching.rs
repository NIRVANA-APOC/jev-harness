//! 匹配循环基线：装配 state → 调判定器 → 落判断记录 → 传播失效 → 依据有效性检查 → 决定行动
//! （`docs/roadmap-1.0.md` §5 M1「匹配循环基线」）。
//!
//! # 一轮的形状
//!
//! 每一轮的顺序是固定的，四组对照臂共用同一份控制策略与同一套事件记录：
//!
//! 1. **注入提问前的扰动**：环境脚本里的变化落进证据日志，[`SemanticState`] 就地做失效传播；
//! 2. **发请求**：按本臂的调度决定这一轮要问哪些问题，装配 state、按读取范围分组、逐组调用判定器
//!    （结果先攒着不落记录）；
//! 3. **注入提问后的扰动**：本轮**全部**请求都发完之后才注入，因此这一轮的每条判断看到的是同一份
//!    输入快照；落记录时快照校验把迟到结果一起标成过期（研究目标 §5.2「延迟返回结果的快照校验」）；
//! 4. **落记录**：成功写判断记录、失败把节点的评估状态写成失败原文（不重试、不换后端）；
//! 5. **决定行动**：按候选动作的前提门取「手上最新的答案」，再按本臂的纪律决定能不能发出。
//!
//! 轮末再写一条**完成声明**：它进轨迹、进计数，但不进验收（[`crate::family::accept`] 只读证据
//! 日志与隐藏真值）。
//!
//! # state 装配（硬约定）
//!
//! 装配只用 [`EvidenceLog::current_evidence`]：投影里写下的版本就是这份 state 的读取版本。把同一
//! 对象的旧版本记录混进来不会漏失效（传播按对象走），但会让判定看到已被取代的材料，读数就不再有
//! 意义（M15 评审遗留约定 a）。
//!
//! # 输入投影规则（H2 两臂共用）
//!
//! 一次请求里的问题必须共享同一个读取范围：本循环按**问题声明的读取对象集合**分组，一组一次请求，
//! 投影 = 这组对象的当前版本（[`InputProjection::covering`]）。增量臂与全量臂用的是同一条规则——
//! 差别只在「要不要复算」，不在装什么（研究目标 §8.4）。
//!
//! # H1 开关（一处）
//!
//! [`Arm::uses_propagation`] 是唯一的分叉点，它同时决定两件事：
//!
//! - **重查触发**：关闭时只问「从没答上来的问题」，不再因为失效标记重查（没有依赖追踪就没有重查
//!   触发；按固定频率重查属于另一项消融，研究目标 §9.2 第 7 行，不在本批）；
//! - **行动纪律**：关闭时不读 [`SemanticState::check_action`]、不读 [`jh_core::JudgmentNode::is_usable`]、
//!   不读 [`SemanticState::recheck_set`]，手上的答案就直接用。
//!
//! 轨迹里与失效有关的读数（复用是否仍然新鲜、执行时的依据是否仍然有效）用快照校验与有效性字段
//! **测量**，但关闭臂的决策路径不读它们——这是本文件里唯一需要小心的地方，`decide` 与 `schedule`
//! 两个函数里各自只有一处 `arm.uses_propagation()` 分支。

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Instant;

use jh_core::{
    ActionId, Answer, EdgeKind, EdgeOrigin, EvaluationStatus, InputProjection, JsonlEvidenceLog,
    Judge, JudgeRequest, JudgmentRecord, NodeId, NodeRef, ObjectId, PremiseCheck, QuestionId,
    QuestionVersion, SemanticState, SnapshotCheck, SnapshotId, State, Validity, verify_snapshot,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::calls::{CallEntry, CallLog};
use crate::error::Error;
use crate::exp::unix_ms;
use crate::family::{
    Acceptance, AcceptanceInput, Environment, FamilyA, FamilyTask, Gate, StepEffect, StepWhen,
    accept,
};
use crate::judge::JudgeName;
use crate::metrics;

/// 判断请求成功时 [`AskTrace::outcome`] 的取值。
pub const ASK_SUCCEEDED: &str = "成功";

/// H2 的重算口径。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Recompute {
    /// 增量：只重算失效或从没答上来的问题，其余复用已有结果。
    Incremental,
    /// 全量：每轮把本任务跟踪的全部问题重算一遍。
    Full,
}

impl Recompute {
    /// 报告里用的标签（配对读数按它配对，只有这一处拼字符串）。
    pub fn label(self) -> &'static str {
        match self {
            Self::Incremental => "incremental",
            Self::Full => "full",
        }
    }
}

/// 一组对照臂：H1（依赖失效传播开/关）× H2（增量/全量）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Arm {
    /// H1：是否使用依赖失效传播。
    pub propagation: bool,
    /// H2：重算口径。
    pub recompute: Recompute,
}

impl Arm {
    /// 四组对照臂，顺序即报告顺序。
    pub const ALL: [Self; 4] = [
        Self {
            propagation: true,
            recompute: Recompute::Incremental,
        },
        Self {
            propagation: false,
            recompute: Recompute::Incremental,
        },
        Self {
            propagation: true,
            recompute: Recompute::Full,
        },
        Self {
            propagation: false,
            recompute: Recompute::Full,
        },
    ];

    /// 臂标识（文件名的组成部分）。
    pub fn label(self) -> &'static str {
        match (self.propagation, self.recompute) {
            (true, Recompute::Incremental) => "h1on-h2inc",
            (false, Recompute::Incremental) => "h1off-h2inc",
            (true, Recompute::Full) => "h1on-h2full",
            (false, Recompute::Full) => "h1off-h2full",
        }
    }

    /// 中文说明（报告里逐臂写清两个开关）。
    pub fn description(self) -> &'static str {
        match (self.propagation, self.recompute) {
            (true, Recompute::Incremental) => "H1 开（依赖失效传播）+ H2 增量",
            (false, Recompute::Incremental) => "H1 关（不用失效传播）+ H2 增量",
            (true, Recompute::Full) => "H1 开（依赖失效传播）+ H2 全量重算",
            (false, Recompute::Full) => "H1 关（不用失效传播）+ H2 全量重算",
        }
    }

    /// H1 开关：决策路径是否使用失效传播。
    pub fn uses_propagation(self) -> bool {
        self.propagation
    }

    /// H2 口径。
    pub fn recompute(self) -> Recompute {
        self.recompute
    }

    /// 从 CLI 标识解析。
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|arm| arm.label() == label)
    }
}

/// 一次判断请求的轨迹。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AskTrace {
    /// 第几轮。
    pub round: usize,
    /// 提问相位。
    pub phase: usize,
    /// 这次请求装进来的对象。
    pub scope: Vec<ObjectId>,
    /// 快照标识。
    pub snapshot: String,
    /// 请求里的问题。
    pub questions: Vec<QuestionId>,
    /// 序列化后的 state 字节数（重算输入量的口径之一）。
    pub input_bytes: usize,
    /// 端到端延迟，毫秒。
    pub latency_ms: u64,
    /// 发起时刻，Unix 毫秒。
    pub at_unix_ms: u64,
    /// 结果：「成功」或判断器错误的原文（原样记录，不改写）。
    pub outcome: String,
    /// 执行者版本（通道响应的模型 ID / 规则版本串）。
    pub version: Option<String>,
    /// 通道侧请求 id。
    pub request_id: Option<String>,
    /// 上报的输入 token 数。
    pub input_tokens: Option<u64>,
    /// 上报的输出 token 数。
    pub output_tokens: Option<u64>,
}

/// 一条判断的轨迹（含判分读数与记录时是否已经漂移）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JudgmentTrace {
    /// 第几轮。
    pub round: usize,
    /// 提问相位。
    pub phase: usize,
    /// 问题。
    pub question: QuestionId,
    /// 问题版本。
    pub question_version: QuestionVersion,
    /// 判断节点。
    pub node: NodeId,
    /// 快照标识。
    pub snapshot: String,
    /// 答案原值（概率不塌缩）。
    pub answer: Answer,
    /// 读出的标签；读不出时为空串。
    pub reading: String,
    /// 隐藏真值标签（判分侧，不进判断请求）。
    pub truth: Option<String>,
    /// 与隐藏真值是否一致。
    pub agreement: Option<bool>,
    /// Brier 分数。
    pub brier: Option<f64>,
    /// 对数损失。
    pub log_loss: Option<f64>,
    /// 记录时投影是否已经漂移（迟到的结果带上这个标记）。
    pub drift: bool,
    /// 记录后的依据有效性。
    pub validity: Validity,
    /// 输入投影（验收核对交付物版本用它）。
    pub projection: InputProjection,
}

/// 一次复用的轨迹：这一轮没有发起请求，直接用了手上的答案。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReuseTrace {
    /// 第几轮。
    pub round: usize,
    /// 问题。
    pub question: QuestionId,
    /// 复用的判断节点。
    pub node: NodeId,
    /// 这条结果在复用时刻是否仍然新鲜（按快照校验测量，不参与决策）。
    pub fresh: bool,
}

/// 一条前提门的判定。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateCheck {
    /// 问题。
    pub question: QuestionId,
    /// 是否成立；`None` 表示手上没有可用的答案。
    pub satisfied: Option<bool>,
    /// 依据说明。
    pub detail: String,
}

/// 一次行动决策的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionOutcome {
    /// 发出。
    Executed,
    /// 有效性检查挡住（前提失效）。
    Blocked,
    /// 前提门不成立。
    GateUnsatisfied,
    /// 手上没有答案，无法决定（弃权）。
    Unresolved,
}

/// 一条行动决策的轨迹。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionTrace {
    /// 第几轮。
    pub round: usize,
    /// 决策相位。
    pub phase: usize,
    /// 动作。
    pub action: ActionId,
    /// 结果。
    pub outcome: DecisionOutcome,
    /// 前提门的逐条判定。
    pub gates: Vec<GateCheck>,
    /// 采纳的判断节点（门所用的答案）。
    pub bases: Vec<NodeId>,
    /// 这些依据在决策时刻是否仍然当前有效（快照校验 + 有效性，测量口径）。
    pub basis_current: bool,
    /// 被挡下的前提节点（[`DecisionOutcome::Blocked`] 时非空）。
    pub blocked_premises: Vec<NodeId>,
    /// 动作计价成本（发出时计入）。
    pub cost_units: f64,
    /// 发出时刻，Unix 毫秒。
    pub at_unix_ms: u64,
}

/// 一条完成声明（控制器自己写的，不作证据）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClaimTrace {
    /// 第几轮。
    pub round: usize,
    /// 声明文本。
    pub text: String,
}

/// 一个任务在一次运行里的完整轨迹。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskTrace {
    /// 判断器名字。
    pub channel: String,
    /// 臂标识。
    pub arm: String,
    /// 任务标识。
    pub task: String,
    /// 轮数。
    pub rounds: usize,
    /// 本次任务运行的开始时刻，Unix 毫秒（恢复延迟在「没有可起算的破坏性事件」时的起点）。
    ///
    /// 早期轨迹没有这个字段：按 0 读，[`crate::compare`] 的恢复延迟会用本次运行第一次请求的时刻兜底。
    #[serde(default)]
    pub started_at_unix_ms: u64,
    /// 判断请求。
    pub asks: Vec<AskTrace>,
    /// 判断记录。
    pub judgments: Vec<JudgmentTrace>,
    /// 复用事件。
    pub reuses: Vec<ReuseTrace>,
    /// 扰动效果（含失效传播的路径）。
    pub steps: Vec<StepEffect>,
    /// 行动决策。
    pub decisions: Vec<DecisionTrace>,
    /// 完成声明。
    pub claims: Vec<ClaimTrace>,
    /// 独立验收结果。
    pub acceptance: Option<Acceptance>,
}

impl TaskTrace {
    /// 已发出的动作：`(轮, 动作)`。
    pub fn executions(&self) -> Vec<(usize, ActionId)> {
        self.decisions
            .iter()
            .filter(|decision| decision.outcome == DecisionOutcome::Executed)
            .map(|decision| (decision.round, decision.action.clone()))
            .collect()
    }

    /// 弃权次数：手上没有答案、发不出动作的决策。
    pub fn abstentions(&self) -> usize {
        self.decisions
            .iter()
            .filter(|decision| decision.outcome == DecisionOutcome::Unresolved)
            .count()
    }

    /// 交付物相关判断的输入投影。
    pub fn deliverable_reads(&self) -> Vec<InputProjection> {
        self.judgments
            .iter()
            .filter(|judgment| judgment.question.as_str() == crate::family::DELIVERABLE_QUESTION)
            .map(|judgment| judgment.projection.clone())
            .collect()
    }
}

/// 一次单任务运行的全部外部依赖。
pub struct LoopContext<'a> {
    /// 任务族（问题集与读取范围）。
    pub family: &'a FamilyA,
    /// 判定器。
    pub judge: &'a dyn Judge,
    /// 判定器名字。
    pub channel: JudgeName,
    /// 对照臂。
    pub arm: Arm,
    /// 运行标识。
    pub run: String,
    /// 调用记录（JSONL，追加）。
    pub calls: &'a mut CallLog,
}

/// 跑完一个任务的整条循环，返回轨迹。
///
/// 证据日志写在 `evidence_path`（JSONL），判断调用逐条追加进 [`LoopContext::calls`]；两者都是报告里
/// 「原始数据位置」的来源。
pub async fn run_task(
    context: &mut LoopContext<'_>,
    task: &FamilyTask,
    evidence_path: &Path,
) -> Result<TaskTrace, Error> {
    let log = JsonlEvidenceLog::open(evidence_path)?;
    let mut state = SemanticState::new(Box::new(log));
    let mut environment = Environment::new(task);
    environment.seed(&mut state)?;

    let mut trace = TaskTrace {
        channel: context.channel.cli_name().to_owned(),
        arm: context.arm.label().to_owned(),
        task: task.id.clone(),
        rounds: task.rounds,
        started_at_unix_ms: unix_ms(),
        asks: Vec::new(),
        judgments: Vec::new(),
        reuses: Vec::new(),
        steps: Vec::new(),
        decisions: Vec::new(),
        claims: Vec::new(),
        acceptance: None,
    };

    for round in 0..task.rounds {
        // 1. 提问前的扰动：这一轮的判断能看到它们。
        trace
            .steps
            .extend(environment.apply_round(round, StepWhen::BeforeAsk, &mut state)?);

        // 2. 发请求：按臂的调度决定问什么、按读取范围分组；本步只发不收。
        let (asks, reuses) = schedule(context.arm, context.family, task, round, &state)?;
        trace.reuses.extend(reuses);
        let mut sent = Vec::with_capacity(asks.len());
        for ask in asks {
            sent.push(send_ask(context, task, round, environment.phase(), &mut state, &ask).await?);
        }

        // 3. 提问后的扰动：**本轮全部请求都发完之后**才注入，因此这一轮的每条迟到结果看到的是同一份
        //    变化，落记录时一起被快照校验标成过期（研究目标 §5.2）。分组不影响谁看到什么材料。
        trace
            .steps
            .extend(environment.apply_round(round, StepWhen::AfterAsk, &mut state)?);

        // 4. 收结果：落判断记录（漂移在这里被判定）或把失败如实写进节点与调用记录。
        for pending in sent {
            record_ask(task, &mut state, &mut trace, pending)?;
        }

        // 5. 决定行动。
        decide(context, task, round, &mut state, &mut trace)?;
    }

    // 5. 完成声明：写进轨迹、计入条数，但不作证据。
    trace.claims.push(ClaimTrace {
        round: task.rounds - 1,
        text: format!(
            "{} 第 {} 轮：本任务已完成（声明不构成证据，验收另按证据日志与隐藏真值独立进行）",
            task.id,
            task.rounds - 1
        ),
    });

    let executions = trace.executions();
    let deliverable_reads = trace.deliverable_reads();
    let acceptance = accept(
        task,
        &AcceptanceInput {
            executions: &executions,
            deliverable_reads: &deliverable_reads,
            claims: trace.claims.len(),
            abstentions: trace.abstentions(),
        },
        state.evidence(),
    );
    trace.acceptance = Some(acceptance);

    Ok(trace)
}

/// 一次请求：同一读取范围的问题合成一批。
#[derive(Debug, Clone, PartialEq)]
struct Ask {
    scope: Vec<ObjectId>,
    questions: Vec<QuestionId>,
}

/// 本轮的调度：要问哪些问题、哪些直接复用。
///
/// **H1 开关的唯一分叉点之一**：关闭臂只问「从没答上来的问题」，不看失效标记。
fn schedule(
    arm: Arm,
    family: &FamilyA,
    task: &FamilyTask,
    round: usize,
    state: &SemanticState,
) -> Result<(Vec<Ask>, Vec<ReuseTrace>), Error> {
    let mut needed: Vec<QuestionId> = Vec::new();
    let mut reuses: Vec<ReuseTrace> = Vec::new();

    for tracked in &task.tracked {
        if tracked.due_round > round {
            continue;
        }
        let latest = latest_node(state, &tracked.question);
        let ask = match arm.recompute() {
            Recompute::Full => true,
            Recompute::Incremental => match latest {
                None => true,
                Some(node) => match node.evaluation {
                    // 关闭臂：只要答上过一次就继续用，不看它是否已经失效。
                    EvaluationStatus::Succeeded => {
                        arm.uses_propagation() && node.validity != Validity::Current
                    }
                    // 从没答上来（未请求 / 失败）：任何臂都要再问。
                    _ => true,
                },
            },
        };

        if ask {
            needed.push(tracked.question.clone());
            continue;
        }

        let node = latest.expect("只有拿到节点时才会走到复用");
        // 复用的新鲜程度按**快照校验**测量（不是按有效性字段）：关闭臂的决策路径不读有效性。
        let fresh = verify_snapshot(state.evidence(), &node.projection) == SnapshotCheck::Current;
        reuses.push(ReuseTrace {
            round,
            question: tracked.question.clone(),
            node: node.id,
            fresh,
        });
    }

    Ok((group_by_scope(family, needed)?, reuses))
}

/// 按读取范围分组：范围相同的问题共享一次请求（研究目标 §5.3 的保守依赖口径）。
///
/// 读取范围只有一个真身：问题集声明的那份（[`FamilyA::scope`]，来自任务 JSON 的 `reads`）。
/// 这里不保留副本——否则改问题集时投影与「漏失效 / 误失效」读数会一起漂，守卫就被自己破坏了。
fn group_by_scope(family: &FamilyA, questions: Vec<QuestionId>) -> Result<Vec<Ask>, Error> {
    let mut groups: BTreeMap<Vec<ObjectId>, Vec<QuestionId>> = BTreeMap::new();
    for question in questions {
        let scope = family.scope(&question).ok_or_else(|| {
            Error::TaskSet(format!(
                "问题 `{question}` 没有声明读取范围：它不在当前固定问题集里"
            ))
        })?;
        groups.entry(scope.to_vec()).or_default().push(question);
    }
    Ok(groups
        .into_iter()
        .map(|(scope, questions)| Ask { scope, questions })
        .collect())
}

/// 某个问题上手上最新的判断节点（按节点标识取最大者）。
fn latest_node<'a>(
    state: &'a SemanticState,
    question: &QuestionId,
) -> Option<&'a jh_core::JudgmentNode> {
    state
        .nodes()
        .filter(|node| node.question == *question)
        .max_by_key(|node| node.id.0)
}

/// 手上最新的**有答案**的判断节点。
fn latest_answer<'a>(
    state: &'a SemanticState,
    question: &QuestionId,
) -> Option<&'a jh_core::JudgmentNode> {
    state
        .nodes()
        .filter(|node| node.question == *question && node.record.is_some())
        .max_by_key(|node| node.id.0)
}

/// 从当前输入装配 state 与输入投影。
///
/// 只装 `current_evidence`：投影里写下的版本就是这份 state 的读取版本（M15 评审遗留约定 a）。
/// 某个对象在当前版本上没有仍有效的记录时，它不进 state、也不进投影。
fn assemble(state: &SemanticState, scope: &[ObjectId]) -> (State, InputProjection) {
    let evidence = state.evidence();
    let mut sections = BTreeMap::new();
    let mut reads = Vec::new();

    for object in scope {
        let Some(version) = evidence.current_version(object) else {
            continue;
        };
        let records: Vec<serde_json::Value> = evidence
            .current_evidence(object)
            .into_iter()
            .map(|entry| {
                json!({
                    "source": entry.source,
                    "at_unix_ms": entry.at_unix_ms,
                    "version": entry.version.0,
                    "content": entry.content,
                })
            })
            .collect();
        sections.insert(object.as_str().to_owned(), json!({ "records": records }));
        reads.push((object.clone(), version));
    }

    (State::Object(sections), InputProjection::covering(reads))
}

/// 已经发出、还没落记录的请求。
///
/// 结果先攒在这里，等本轮「提问后」的扰动注入之后再一起落记录：这样一轮里的每条判断看到的是同一份
/// 输入快照，迟到结果也一起被判成过期，不会出现「同一轮里先发的看到旧材料、后发的看到新材料」。
struct PendingAsk {
    trace: AskTrace,
    phase: usize,
    nodes: BTreeMap<QuestionId, NodeId>,
    outcome: Result<JudgmentRecord, String>,
}

/// 发一次判断请求：装配 state、声明节点、调判定器、落调用记录；不写判断记录。
///
/// 失败原样记进调用记录，不重试、不换后端（落节点的失败状态在 [`record_ask`] 里做）。
async fn send_ask(
    context: &mut LoopContext<'_>,
    task: &FamilyTask,
    round: usize,
    phase: usize,
    state: &mut SemanticState,
    ask: &Ask,
) -> Result<PendingAsk, Error> {
    let (material, projection) = assemble(state, &ask.scope);
    let input_bytes = serde_json::to_string(&material)?.len();
    let scope_key = ask
        .scope
        .iter()
        .map(|object| object.as_str().to_owned())
        .collect::<Vec<_>>()
        .join("+");
    let snapshot = SnapshotId::from(format!("{}-r{round}-{scope_key}", task.id));

    let mut questions = BTreeMap::new();
    for question in &ask.questions {
        let spec = context
            .family
            .question(question)
            .ok_or_else(|| {
                Error::TaskSet(format!(
                    "任务 {}：问题 `{question}` 不在固定问题集里",
                    task.id
                ))
            })?
            .clone();
        questions.insert(question.clone(), spec);
    }

    // 声明节点：绑定问题版本与输入投影，同时登记确定性读取边。
    let nodes: BTreeMap<QuestionId, NodeId> = questions
        .iter()
        .map(|(question, spec)| {
            (
                question.clone(),
                state.declare_node(question.clone(), spec.version, projection.clone()),
            )
        })
        .collect();
    // 模型提出的额外依赖：来源标 Proposed，与确定性读取边分开记录（研究目标 §5.3）。
    for dependency in &task.extra_dependencies {
        if let Some(node) = nodes.get(&dependency.question) {
            state.graph_mut().add_edge(
                NodeRef::Object(dependency.object.clone()),
                NodeRef::Judgment(*node),
                EdgeKind::Read,
                EdgeOrigin::Proposed,
            );
        }
    }

    let request = JudgeRequest {
        snapshot: snapshot.clone(),
        state: material,
        questions,
    };

    let started = Instant::now();
    let at_unix_ms = unix_ms();
    let outcome = context.judge.judge(&request).await;
    let latency = started.elapsed();
    let latency_ms = latency.as_millis() as u64;

    let mut trace = AskTrace {
        round,
        phase,
        scope: ask.scope.clone(),
        snapshot: snapshot.as_str().to_owned(),
        questions: ask.questions.clone(),
        input_bytes,
        latency_ms,
        at_unix_ms,
        outcome: String::new(),
        version: None,
        request_id: None,
        input_tokens: None,
        output_tokens: None,
    };

    let result = match outcome {
        Ok(response) => match JudgmentRecord::from_response(&request, &response, latency) {
            Ok(record) => {
                trace.outcome = ASK_SUCCEEDED.to_owned();
                trace.version = Some(record.version.clone());
                trace.request_id = record.request_id.clone();
                trace.input_tokens = record.usage.as_ref().and_then(|usage| usage.input_tokens);
                trace.output_tokens = record.usage.as_ref().and_then(|usage| usage.output_tokens);
                context.calls.append(&CallEntry::Judgment {
                    run: context.run.clone(),
                    case: format!("{}-r{round}-{scope_key}", task.id),
                    record: Box::new(record.clone()),
                })?;
                Ok(record)
            }
            Err(error) => {
                let text = error.to_string();
                trace.outcome = text.clone();
                context.calls.append(&CallEntry::Failure {
                    run: context.run.clone(),
                    case: format!("{}-r{round}-{scope_key}", task.id),
                    source: context.channel.source(),
                    latency_ms,
                    error: text.clone(),
                })?;
                Err(text)
            }
        },
        Err(error) => {
            let text = error.to_string();
            trace.outcome = text.clone();
            context.calls.append(&CallEntry::Failure {
                run: context.run.clone(),
                case: format!("{}-r{round}-{scope_key}", task.id),
                source: context.channel.source(),
                latency_ms,
                error: text.clone(),
            })?;
            Err(text)
        }
    };

    Ok(PendingAsk {
        trace,
        phase,
        nodes,
        outcome: result,
    })
}

/// 落一条已经回来的请求：成功写判断记录与轨迹，失败把节点的评估状态写成失败原文。
///
/// 快照校验在这里发生，所以「请求在途时输入变化」的迟到结果会带上过期标记（研究目标 §5.2）。
fn record_ask(
    task: &FamilyTask,
    state: &mut SemanticState,
    trace: &mut TaskTrace,
    pending: PendingAsk,
) -> Result<(), Error> {
    let PendingAsk {
        trace: ask_trace,
        phase,
        nodes,
        outcome,
    } = pending;

    let record = match outcome {
        Ok(record) => record,
        Err(text) => {
            fail_nodes(state, &nodes, &text)?;
            trace.asks.push(ask_trace);
            return Ok(());
        }
    };

    let round = ask_trace.round;
    trace.asks.push(ask_trace);
    for (question, node) in &nodes {
        state.record_judgment(*node, record.clone())?;
        let recorded = state.node(*node).expect("刚写入的节点必然存在");
        let answer = recorded
            .judgment()
            .expect("记录里有该问题的答案")
            .answer
            .clone();
        let reading = metrics::reading(&answer);
        let truth = task.truth_at(phase, question);
        let score = truth
            .as_ref()
            .and_then(|truth| metrics::score(&answer, truth));
        trace.judgments.push(JudgmentTrace {
            round,
            phase,
            question: question.clone(),
            question_version: recorded.question_version,
            node: *node,
            snapshot: recorded
                .record
                .as_ref()
                .map(|record| record.snapshot.as_str().to_owned())
                .unwrap_or_default(),
            answer,
            reading: reading.map(|reading| reading.label).unwrap_or_default(),
            truth: truth.as_ref().map(metrics::truth_label),
            agreement: score.map(|score| score.agreement),
            brier: score.map(|score| score.brier),
            log_loss: score.and_then(|score| score.log_loss),
            drift: recorded.validity == Validity::NeedsRecheck,
            validity: recorded.validity,
            projection: recorded.projection.clone(),
        });
    }
    Ok(())
}

/// 判断失败时把节点的评估状态写清楚（失败就是失败，不给空答案）。
fn fail_nodes(
    state: &mut SemanticState,
    nodes: &BTreeMap<QuestionId, NodeId>,
    error: &str,
) -> Result<(), Error> {
    for node in nodes.values() {
        state.set_evaluation(*node, EvaluationStatus::Failed(error.to_owned()))?;
    }
    Ok(())
}

/// 决定这一轮的行动。
///
/// **H1 开关的唯一分叉点之二**：打开时先声明行动前提再走
/// [`SemanticState::check_action`]；关闭时直接按手上的答案发出，不读有效性（消融纪律 b）。
fn decide(
    context: &mut LoopContext<'_>,
    task: &FamilyTask,
    round: usize,
    state: &mut SemanticState,
    trace: &mut TaskTrace,
) -> Result<(), Error> {
    let executed: BTreeSet<ActionId> = trace
        .decisions
        .iter()
        .filter(|decision| decision.outcome == DecisionOutcome::Executed)
        .map(|decision| decision.action.clone())
        .collect();

    for spec in &task.actions {
        if executed.contains(&spec.id) {
            continue;
        }

        let mut gates = Vec::new();
        let mut bases = Vec::new();
        let mut satisfied = true;
        let mut unresolved = false;
        for gate in &spec.requires {
            match latest_answer(state, &gate.question) {
                None => {
                    unresolved = true;
                    satisfied = false;
                    gates.push(GateCheck {
                        question: gate.question.clone(),
                        satisfied: None,
                        detail: "手上没有该问题的答案".to_owned(),
                    });
                }
                Some(node) => {
                    let answer = node
                        .judgment()
                        .expect("有记录的节点必有答案")
                        .answer
                        .clone();
                    let holds = gate_holds(gate, &answer);
                    if !holds {
                        satisfied = false;
                    }
                    bases.push(node.id);
                    gates.push(GateCheck {
                        question: gate.question.clone(),
                        satisfied: Some(holds),
                        detail: gate_detail(gate, &answer),
                    });
                }
            }
        }

        let basis_current = bases.iter().all(|node| {
            state.node(*node).is_some_and(|node| {
                node.validity == Validity::Current
                    && verify_snapshot(state.evidence(), &node.projection) == SnapshotCheck::Current
            })
        });

        let mut decision = DecisionTrace {
            round,
            phase: task.decision_phase(round),
            action: spec.id.clone(),
            outcome: if unresolved {
                DecisionOutcome::Unresolved
            } else {
                DecisionOutcome::GateUnsatisfied
            },
            gates,
            bases: bases.clone(),
            basis_current,
            blocked_premises: Vec::new(),
            cost_units: spec.cost_units,
            at_unix_ms: unix_ms(),
        };

        if satisfied {
            // 两边都登记行动前提：依赖图是事件记录的一部分，四组臂的记录保持可比；
            // 差别只在下一条——开启臂读 check_action，关闭臂不读。
            state.declare_action(spec.id.clone(), bases.clone())?;
            decision.outcome = if context.arm.uses_propagation() {
                // H1 开：行动前提复核（发出前复核，撤销只改使用资格）。
                match state.check_action(&spec.id)? {
                    PremiseCheck::Cleared => DecisionOutcome::Executed,
                    PremiseCheck::Blocked { premises } => {
                        decision.blocked_premises = premises;
                        DecisionOutcome::Blocked
                    }
                    PremiseCheck::Withdrawn => {
                        decision.blocked_premises = bases.clone();
                        DecisionOutcome::Blocked
                    }
                }
            } else {
                // H1 关：不读 check_action / is_usable / recheck_set，手上的答案直接用。
                DecisionOutcome::Executed
            };
            if decision.outcome != DecisionOutcome::Executed {
                decision.cost_units = 0.0;
            }
        } else {
            decision.cost_units = 0.0;
        }

        trace.decisions.push(decision);
    }

    Ok(())
}

/// 一条前提门是否成立。
fn gate_holds(gate: &Gate, answer: &Answer) -> bool {
    match (gate, answer) {
        (
            Gate {
                equals: Some(label),
                ..
            },
            Answer::Choice(answer),
        ) => answer.choice == *label,
        (
            Gate {
                at_least: Some(min),
                ..
            },
            Answer::Noul(answer),
        ) => answer.noul >= *min,
        (
            Gate {
                at_most: Some(max), ..
            },
            Answer::Noul(answer),
        ) => answer.noul <= *max,
        _ => false,
    }
}

/// 前提门的依据说明。
fn gate_detail(gate: &Gate, answer: &Answer) -> String {
    match answer {
        Answer::Choice(answer) => format!(
            "读出标签 `{}`{}",
            answer.choice,
            gate.equals
                .as_ref()
                .map(|label| format!("，门要求 `{label}`"))
                .unwrap_or_default()
        ),
        Answer::Noul(answer) => format!(
            "noul = {:.3}{}",
            answer.noul,
            match (gate.at_least, gate.at_most) {
                (Some(min), _) => format!("，门要求 ≥ {min}"),
                (_, Some(max)) => format!("，门要求 ≤ {max}"),
                _ => String::new(),
            }
        ),
        other => format!("答案原语与前提门不符：{other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::family::FamilyA;
    use crate::judge::RuleJudge;

    #[tokio::test]
    async fn the_open_arm_recovers_and_the_closed_arm_does_not() {
        let family = FamilyA::load().unwrap();
        let task = family.task("A1").unwrap();
        let dir = std::env::temp_dir().join(format!("jh-app-loop-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let open = run_variant(&family, task, Arm::ALL[0], &dir, "open").await;
        assert_eq!(
            open.executions(),
            vec![(1, ActionId::from("rollback-cache"))],
            "开启臂应当在新证据到来后重查并发出补救动作"
        );
        assert!(open.acceptance.as_ref().unwrap().passed);
        assert!(
            open.reuses.iter().any(|reuse| reuse.fresh),
            "没被波及的结果应当被复用"
        );

        let closed = run_variant(&family, task, Arm::ALL[1], &dir, "closed").await;
        assert!(
            closed.executions().is_empty(),
            "关闭臂没有重查触发，停在 r0 的答案上，发不出动作"
        );
        assert!(!closed.acceptance.as_ref().unwrap().passed);
        assert!(
            closed.reuses.iter().any(|reuse| !reuse.fresh),
            "关闭臂复用的正是已经失效的结果"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn a_late_result_is_marked_stale_in_the_open_arm_and_used_anyway_in_the_closed_arm() {
        let family = FamilyA::load().unwrap();
        let task = family.task("A4").unwrap();
        let dir = std::env::temp_dir().join(format!("jh-app-loop-drift-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let open = run_variant(&family, task, Arm::ALL[0], &dir, "open").await;
        assert!(
            open.judgments.iter().any(|judgment| judgment.drift),
            "请求在途时输入变化，迟到结果应当带上过期标记"
        );
        assert!(open.executions().is_empty(), "过期依据发不出动作");
        assert!(open.acceptance.as_ref().unwrap().passed);

        let closed = run_variant(&family, task, Arm::ALL[1], &dir, "closed").await;
        assert_eq!(
            closed.executions(),
            vec![(0, ActionId::from("rollback-cache"))]
        );
        let decision = closed
            .decisions
            .iter()
            .find(|decision| decision.outcome == DecisionOutcome::Executed)
            .unwrap();
        assert!(
            !decision.basis_current,
            "关闭臂拿的是已经过期的依据（旧依据行动率的分子）"
        );
        assert!(
            !closed.acceptance.as_ref().unwrap().passed,
            "不该发出的动作"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 离线跑一个臂 × 一个任务（规则判断器，不联网）。
    async fn run_variant(
        family: &FamilyA,
        task: &FamilyTask,
        arm: Arm,
        dir: &Path,
        name: &str,
    ) -> TaskTrace {
        let judge = RuleJudge::new();
        let mut calls = CallLog::open(dir.join(format!("{name}.calls.jsonl"))).unwrap();
        let mut context = LoopContext {
            family,
            judge: &judge,
            channel: JudgeName::Rule,
            arm,
            run: name.to_owned(),
            calls: &mut calls,
        };
        run_task(
            &mut context,
            task,
            &dir.join(format!("{name}.evidence.jsonl")),
        )
        .await
        .expect("离线规则判断器不会失败")
    }
}
