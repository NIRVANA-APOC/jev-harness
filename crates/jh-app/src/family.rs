//! 任务族 A：会变化的故障环境，以及绑定它的 M1 固定问题集（`docs/roadmap-1.0.md` §5 M1）。
//!
//! 环境按「快照 + 可控扰动 + 隐藏真值 + 独立验收 + 可计价动作」组织
//! （`docs/RESEARCH_GOALS.md` §8.3、§9.3）：每个任务从一个初始证据快照出发，按脚本注入变化，
//! 变化对判定材料的影响由 [`SemanticState`] 负责；**隐藏真值与验收都不进判断请求**，只在
//! 判分与验收侧使用（研究目标 §13.2 的真值隔离）。
//!
//! # 问题集（M1 版）
//!
//! 固定问题集与任务族 A 的 state 绑定：问题带版本（同一 ID 改含义必须升版本，由
//! [`QuestionRegistry`] 强制），每个问题声明它读哪些对象（`reads`）——判断请求的 state 只装这些
//! 对象的**当前证据**，输入投影按同一份对象集合登记（研究目标 §5.3「共享一份请求 state 的问题保守
//! 依赖该请求的全部输入」）。因此「一个问题的输入范围」是问题集声明的事实，不是事后猜测。
//!
//! # 相位
//!
//! 环境的变化按**相位**编号：相位 0 是初始快照，之后每应用一条扰动就前进一相。判断请求落在提问
//! 相位上，行动决策落在该轮全部扰动之后的**决策相位**上；隐藏真值按相位给出，判分与验收都按这个
//! 编号对照（`docs/RESEARCH_GOALS.md` §10.1：关键决策机会由场景协议定义，不由控制器提出了多少动作
//! 决定）。
//!
//! # 独立验收
//!
//! [`accept`] 只读**证据日志与隐藏真值**：必需动作是否在某轮真的发出、有没有发出不该发出的动作、
//! 完成声明是否绑定到**实际交付物版本**的证据（研究目标 §5.2 末条）。控制器写在轨迹里的完成声明
//! 只计数、不作证据——`claims` 字段进验收输入，但在三条判定里一次都不参与。

use std::collections::{BTreeMap, BTreeSet};

use jh_core::{
    ActionId, EvidenceLog, InputProjection, Invalidation, NewEvidence, ObjectId, ObjectVersion,
    Question, QuestionId, QuestionRegistry, QuestionSpec, QuestionVersion, SemanticState,
    SnapshotCheck, verify_snapshot,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::Error;
use crate::tasks::Truth;

/// 任务族 A 的内嵌 JSON 原文：编译期内嵌，运行时不依赖工作目录。
pub const FAMILY_A_JSON: &str = include_str!("../tasks/family_a.json");

/// 任务族 A 在仓库里的位置，写进报告以便复核。
pub const FAMILY_A_SOURCE: &str = "crates/jh-app/tasks/family_a.json";

/// 交付物对象：完成声明必须绑定到它的**当前版本**的证据。
pub const DELIVERABLE_OBJECT: &str = "fix-report.md";

/// 交付物相关问题 ID（固定问题集里读 `fix-report.md` 的那一问）。
pub const DELIVERABLE_QUESTION: &str = "deliverable_ready";

/// 选择类问题 ID：首要原因。
pub const CAUSE_QUESTION: &str = "cause_primary";

/// 是非类问题 ID：材料是否已经确认失败原因（前提诊断）。
///
/// 本题不参与行动前提门——真实模型在「原因是否已被确认」这一档上答得不好（见批次的真调读数），
/// 前提门只用 [`CAUSE_QUESTION`]；本题照常判定与判分，作为材料确认度的诊断读数报告。
pub const PREMISE_QUESTION: &str = "premise_holds";

/// 是非类问题 ID：运行时指标是否仍在基线范围内。
pub const METRICS_QUESTION: &str = "metrics_within_baseline";

/// 扰动注入的时机。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepWhen {
    /// 在提问之前注入：这一轮的判断能看到它。
    BeforeAsk,
    /// 在提问之后注入：这一轮的判断是在它之前发出的——迟到结果因此带上过期标记
    /// （`docs/RESEARCH_GOALS.md` §5.2 的「延迟返回结果的快照校验」）。
    AfterAsk,
}

/// 一条扰动的内容。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Change {
    /// 追加一条新证据。
    Append {
        /// 来源（工具名）。
        source: String,
        /// 记录时间，Unix 毫秒。
        at_unix_ms: u64,
        /// 对象。
        object: ObjectId,
        /// 该对象的新版本。
        version: ObjectVersion,
        /// 原始内容。
        content: Value,
    },
    /// 撤回某对象某版本上的一条证据（该版本上仍有效的第一条）。
    Retract {
        /// 对象。
        object: ObjectId,
        /// 版本。
        version: ObjectVersion,
    },
}

impl Change {
    /// 这条扰动作用的对象。
    pub fn object(&self) -> &ObjectId {
        match self {
            Self::Append { object, .. } | Self::Retract { object, .. } => object,
        }
    }
}

/// 一条脚本化扰动。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Step {
    /// 在第几轮注入。
    pub round: usize,
    /// 在提问之前还是之后注入。
    pub when: StepWhen,
    /// 这条扰动在场景里意味着什么（可复核说明，不进判断请求）。
    pub note: String,
    /// 扰动内容。
    pub change: Change,
}

/// 一次扰动造成的实际效果（进轨迹与报告）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepEffect {
    /// 第几轮。
    pub round: usize,
    /// 提问前还是提问后。
    pub when: StepWhen,
    /// 扰动作用后的相位编号。
    pub phase: usize,
    /// 注入时刻，Unix 毫秒（恢复延迟的墙钟口径从这里起算）。
    pub at_unix_ms: u64,
    /// 场景说明。
    pub note: String,
    /// 扰动内容。
    pub change: Change,
    /// 运行时做的失效传播（研究目标 §5.4 的「标记 + 传播路径」）。
    pub invalidation: Invalidation,
}

/// 一条候选动作：发出它的前提由问题答案的门给出，发出一次计入它的计价成本。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionSpec {
    /// 动作标识。
    pub id: ActionId,
    /// 计价成本（本项目的自定口径，不是任何供应商价格表）。
    pub cost_units: f64,
    /// 前提门：全部成立才能发出。
    pub requires: Vec<Gate>,
}

/// 一条前提门：某个问题的答案要满足的条件。
///
/// 选择类问题用 [`Gate::equals`]；是非类问题用 [`Gate::at_least`] / [`Gate::at_most`]。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Gate {
    /// 问题标识。
    pub question: QuestionId,
    /// 选择类：读出标签必须等于这个选项。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub equals: Option<String>,
    /// 是非类：`noul` 不低于这个值。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_least: Option<f64>,
    /// 是非类：`noul` 不高于这个值。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_most: Option<f64>,
}

/// 一个相位上的隐藏真值。
///
/// `required` 是相对口径：**以本任务声明的候选动作集为全集**，列出该相位下应当发出的动作；
/// 为空表示候选集里没有该发的动作。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Phase {
    /// 首要原因的隐藏真值（`cause_primary` 的标签）。
    pub cause: String,
    /// 「补救动作前提成立」的隐藏真值。
    pub premise_holds: bool,
    /// 「指标仍在基线」的隐藏真值。
    pub metrics_within_baseline: bool,
    /// 「交付物已产出」的隐藏真值。
    pub deliverable_ready: bool,
    /// 该相位下应当发出的动作。
    pub required: Vec<ActionId>,
}

/// 一个问题从第几轮起进入本任务的调度。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackedQuestion {
    /// 问题标识。
    pub question: QuestionId,
    /// 从这一轮（含）起需要答案。
    pub due_round: usize,
}

/// 一个任务：初始快照 + 扰动脚本 + 隐藏真值 + 候选动作。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FamilyTask {
    /// 任务标识。
    pub id: String,
    /// 场景说明（只给读样本的人看，不进判断请求）。
    pub notes: String,
    /// 轮数：每轮一次行动决策机会（研究目标 §10.1 的「关键决策机会」）。
    pub rounds: usize,
    /// 初始证据，按顺序追加。
    pub initial: Vec<NewEvidence>,
    /// 本任务跟踪的问题与各自的生效轮。
    pub tracked: Vec<TrackedQuestion>,
    /// 模型建议的额外读取依赖（研究目标 §5.3 的语义关联，登记进依赖图时标 `Proposed`）。
    pub extra_dependencies: Vec<TrackedDependency>,
    /// 扰动脚本，按（轮、时机）排序。
    pub steps: Vec<Step>,
    /// 候选动作。
    pub actions: Vec<ActionSpec>,
    /// 各相位的隐藏真值；长度等于扰动条数 + 1。
    pub phases: Vec<Phase>,
    /// 验收是否要求完成声明绑定交付物当前版本的证据。
    pub bind_deliverable: bool,
}

/// 一条模型建议的额外依赖。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackedDependency {
    /// 读取方问题。
    pub question: QuestionId,
    /// 被读取的对象。
    pub object: ObjectId,
}

impl FamilyTask {
    /// 某条扰动之后（含）的相位编号 = 已应用扰动条数。
    fn phase_after_round(&self, round: usize) -> usize {
        self.steps.iter().filter(|step| step.round <= round).count()
    }

    /// 提问相位：本轮「提问前」扰动都已应用、本轮「提问后」扰动尚未应用。
    pub fn ask_phase(&self, round: usize) -> usize {
        self.steps
            .iter()
            .filter(|step| {
                step.round < round || (step.round == round && step.when == StepWhen::BeforeAsk)
            })
            .count()
    }

    /// 决策相位：本轮全部扰动都已应用。
    pub fn decision_phase(&self, round: usize) -> usize {
        self.phase_after_round(round)
    }

    /// 全部决策相位（同一个相位可能被多轮共用，去重后升序）。
    pub fn decision_phases(&self) -> BTreeSet<usize> {
        (0..self.rounds)
            .map(|round| self.decision_phase(round))
            .collect()
    }

    /// 某轮的决策相位上应当发出的动作。
    pub fn required_at_round(&self, round: usize) -> &[ActionId] {
        &self.phases[self.decision_phase(round)].required
    }

    /// 在某个决策相位上应当发出的动作。
    pub fn required_at_phase(&self, phase: usize) -> Option<&[ActionId]> {
        self.phases
            .get(phase)
            .map(|phase| phase.required.as_slice())
    }

    /// 全流程需要发出的动作（决策相位上的并集）。
    pub fn required_actions(&self) -> BTreeSet<ActionId> {
        self.decision_phases()
            .into_iter()
            .filter_map(|phase| self.phases.get(phase))
            .flat_map(|phase| phase.required.iter().cloned())
            .collect()
    }

    /// 某动作第一次被要求的轮：早于这一轮发出它属于「提前行动」。
    pub fn first_required_round(&self, action: &ActionId) -> Option<usize> {
        (0..self.rounds).find(|round| self.required_at_round(*round).contains(action))
    }

    /// 某动作在这一轮发出是否正当：它在这一轮或更早被要求过。
    pub fn justified_at_round(&self, round: usize, action: &ActionId) -> bool {
        self.first_required_round(action)
            .is_some_and(|first| round >= first)
    }

    /// 该问题在某相位上的隐藏真值（固定问题集的四个问题）。
    pub fn truth_at(&self, phase: usize, question: &QuestionId) -> Option<Truth> {
        let phase = self.phases.get(phase)?;
        match question.as_str() {
            CAUSE_QUESTION => Some(Truth::Choice {
                label: phase.cause.clone(),
            }),
            PREMISE_QUESTION => Some(Truth::Noul {
                yes: phase.premise_holds,
            }),
            METRICS_QUESTION => Some(Truth::Noul {
                yes: phase.metrics_within_baseline,
            }),
            DELIVERABLE_QUESTION => Some(Truth::Noul {
                yes: phase.deliverable_ready,
            }),
            _ => None,
        }
    }

    /// 本任务从第几轮起需要某个问题的答案。
    pub fn due_round(&self, question: &QuestionId) -> Option<usize> {
        self.tracked
            .iter()
            .find(|tracked| tracked.question == *question)
            .map(|tracked| tracked.due_round)
    }

    /// 某个候选动作的定义。
    pub fn action(&self, id: &ActionId) -> Option<&ActionSpec> {
        self.actions.iter().find(|action| action.id == *id)
    }
}

/// 任务族 A：固定问题集 + 任务列表。
pub struct FamilyA {
    /// 任务集名字。
    pub name: String,
    /// 仓库内位置。
    pub source: String,
    /// 任务集说明。
    pub notes: String,
    /// 任务列表。
    pub tasks: Vec<FamilyTask>,
    questions: BTreeMap<QuestionId, QuestionSpec>,
    scopes: BTreeMap<QuestionId, Vec<ObjectId>>,
    history: Vec<QuestionVersionEntry>,
    registry: QuestionRegistry,
}

/// 问题集里的一条版本记录（进报告，用来复核「同一 ID 改含义产生新版本」）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuestionVersionEntry {
    /// 问题标识。
    pub question: QuestionId,
    /// 版本。
    pub version: QuestionVersion,
    /// 是不是当前使用的版本。
    pub current: bool,
    /// 该版本读哪些对象。
    pub reads: Vec<ObjectId>,
}

impl FamilyA {
    /// 读取内嵌的任务族 A 并校验。
    pub fn load() -> Result<Self, Error> {
        Self::from_json(FAMILY_A_JSON)
    }

    /// 从 JSON 原文构造（测试与探针用同一套校验）。
    pub fn from_json(text: &str) -> Result<Self, Error> {
        let file: FamilyFile = serde_json::from_str(text)
            .map_err(|error| Error::TaskSet(format!("{FAMILY_A_SOURCE}：{error}")))?;

        let mut registry = QuestionRegistry::default();
        let mut history = Vec::new();
        for entry in &file.questions {
            if entry.reads.is_empty() {
                return Err(invalid(&format!(
                    "问题 `{}` v{} 没有声明输入对象",
                    entry.id, entry.version
                )));
            }
            // 同一 ID 改含义必须升版本：同版本换含义在这里显式失败（研究目标 §5.2）。
            registry
                .register(
                    &entry.id,
                    &QuestionSpec {
                        version: entry.version,
                        question: entry.question.clone(),
                    },
                )
                .map_err(|error| invalid(&format!("问题 `{}` 登记失败：{error}", entry.id)))?;
            history.push(QuestionVersionEntry {
                question: entry.id.clone(),
                version: entry.version,
                current: false,
                reads: entry.reads.clone(),
            });
        }

        // 当前版本 = 版本号最大的那一条；问题集只按当前版本请求。
        let mut questions = BTreeMap::new();
        let mut current_scopes = BTreeMap::new();
        for entry in &file.questions {
            let latest = questions
                .get(&entry.id)
                .map(|spec: &QuestionSpec| spec.version)
                .unwrap_or(QuestionVersion(0));
            if entry.version >= latest {
                questions.insert(
                    entry.id.clone(),
                    QuestionSpec {
                        version: entry.version,
                        question: entry.question.clone(),
                    },
                );
                current_scopes.insert(entry.id.clone(), entry.reads.clone());
            }
        }
        for entry in history.iter_mut() {
            entry.current = questions
                .get(&entry.question)
                .is_some_and(|spec| spec.version == entry.version);
        }
        if questions.is_empty() {
            return Err(invalid("问题集为空"));
        }

        let mut tasks = Vec::with_capacity(file.tasks.len());
        for task in file.tasks {
            tasks.push(validate_task(task, &questions, &current_scopes)?);
        }
        if tasks.is_empty() {
            return Err(invalid("任务列表为空"));
        }

        Ok(Self {
            name: file.name,
            source: file.source,
            notes: file.notes,
            tasks,
            questions,
            scopes: current_scopes,
            history,
            registry,
        })
    }

    /// 取一个任务。
    pub fn task(&self, id: &str) -> Option<&FamilyTask> {
        self.tasks.iter().find(|task| task.id == id)
    }

    /// 当前版本的问题集（判断请求用这一份）。
    pub fn questions(&self) -> &BTreeMap<QuestionId, QuestionSpec> {
        &self.questions
    }

    /// 某个问题当前版本的读取对象。
    pub fn scope(&self, question: &QuestionId) -> Option<&[ObjectId]> {
        self.scopes.get(question).map(Vec::as_slice)
    }

    /// 某个问题当前版本的定义。
    pub fn question(&self, question: &QuestionId) -> Option<&QuestionSpec> {
        self.questions.get(question)
    }

    /// 问题版本历史（含当前标记）。
    pub fn history(&self) -> &[QuestionVersionEntry] {
        &self.history
    }

    /// 问题版本表（校验「同版本换含义」显式失败）。
    pub fn registry(&self) -> &QuestionRegistry {
        &self.registry
    }
}

#[derive(Debug, Deserialize)]
struct FamilyFile {
    name: String,
    source: String,
    #[serde(default)]
    notes: String,
    questions: Vec<QuestionEntry>,
    tasks: Vec<TaskEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct QuestionEntry {
    id: QuestionId,
    version: QuestionVersion,
    reads: Vec<ObjectId>,
    question: Question,
}

#[derive(Debug, Clone, Deserialize)]
struct TaskEntry {
    id: String,
    #[serde(default)]
    notes: String,
    rounds: usize,
    initial: Vec<EvidenceEntry>,
    tracked: Vec<TrackedQuestion>,
    #[serde(default)]
    extra_dependencies: Vec<TrackedDependency>,
    steps: Vec<Step>,
    actions: Vec<ActionSpec>,
    phases: Vec<Phase>,
    acceptance: AcceptanceEntry,
}

#[derive(Debug, Clone, Deserialize)]
struct EvidenceEntry {
    source: String,
    at_unix_ms: u64,
    object: ObjectId,
    version: ObjectVersion,
    content: Value,
}

#[derive(Debug, Clone, Deserialize)]
struct AcceptanceEntry {
    bind_deliverable: bool,
}

fn invalid(detail: &str) -> Error {
    Error::TaskSet(detail.to_owned())
}

/// 校验单个任务并把 JSON 形态转成运行形态。
fn validate_task(
    task: TaskEntry,
    questions: &BTreeMap<QuestionId, QuestionSpec>,
    scopes: &BTreeMap<QuestionId, Vec<ObjectId>>,
) -> Result<FamilyTask, Error> {
    let id = task.id.clone();
    let fail = |detail: String| invalid(&format!("任务 {id}：{detail}"));

    if task.rounds < 2 {
        return Err(fail(
            "轮数至少为 2（第一轮之后必须还有一次决策机会）".to_owned(),
        ));
    }
    if task.steps.is_empty() {
        return Err(fail("没有扰动：这不属于任务族 A".to_owned()));
    }
    if task.phases.len() != task.steps.len() + 1 {
        return Err(fail(format!(
            "相位数量 {} 与扰动数量 {} 不匹配（应为扰动数 + 1）",
            task.phases.len(),
            task.steps.len()
        )));
    }
    for step in &task.steps {
        if step.round >= task.rounds {
            return Err(fail(format!(
                "扰动落在第 {} 轮，超出 {} 轮的范围",
                step.round, task.rounds
            )));
        }
    }
    let mut ordered = task.steps.clone();
    ordered.sort_by_key(|step| (step.round, step.when == StepWhen::AfterAsk));
    if ordered != task.steps {
        return Err(fail(
            "扰动脚本必须先按轮、再按「提问前 < 提问后」排序".to_owned(),
        ));
    }
    if task.tracked.is_empty() {
        return Err(fail("没有跟踪任何问题".to_owned()));
    }
    let mut action_ids = BTreeSet::new();
    for action in &task.actions {
        if !action_ids.insert(action.id.clone()) {
            return Err(fail(format!("候选动作 `{}` 重复", action.id)));
        }
        if action.requires.is_empty() {
            return Err(fail(format!("候选动作 `{}` 没有前提门", action.id)));
        }
        for gate in &action.requires {
            if !questions.contains_key(&gate.question) {
                return Err(fail(format!(
                    "候选动作 `{}` 的前提门引用了未知问题 `{}`",
                    action.id, gate.question
                )));
            }
            if gate.equals.is_none() && gate.at_least.is_none() && gate.at_most.is_none() {
                return Err(fail(format!(
                    "候选动作 `{}` 在问题 `{}` 上的前提门没有条件",
                    action.id, gate.question
                )));
            }
        }
    }
    for tracked in &task.tracked {
        if !questions.contains_key(&tracked.question) {
            return Err(fail(format!("跟踪了未知问题 `{}`", tracked.question)));
        }
        if tracked.due_round >= task.rounds {
            return Err(fail(format!(
                "问题 `{}` 的生效轮 {} 超出 {} 轮的范围",
                tracked.question, tracked.due_round, task.rounds
            )));
        }
    }
    for dependency in &task.extra_dependencies {
        if !task
            .tracked
            .iter()
            .any(|tracked| tracked.question == dependency.question)
        {
            return Err(fail(format!(
                "额外依赖的读取方 `{}` 不在本任务跟踪的问题里",
                dependency.question
            )));
        }
        let reads = scopes
            .get(&dependency.question)
            .map(Vec::as_slice)
            .unwrap_or_default();
        if reads.contains(&dependency.object) {
            return Err(fail(format!(
                "额外依赖 `{}` → `{}` 已经在声明范围内，不算模型提出的额外依赖",
                dependency.question, dependency.object
            )));
        }
    }
    let options: Vec<String> = match questions.get(&QuestionId::from(CAUSE_QUESTION)) {
        Some(spec) => match &spec.question {
            Question::Choice(choice) => choice.criteria.keys().cloned().collect(),
            other => {
                return Err(fail(format!(
                    "`{CAUSE_QUESTION}` 必须是选择类问题，实际是 {other:?}"
                )));
            }
        },
        None => return Err(fail(format!("问题集里没有 `{CAUSE_QUESTION}`"))),
    };
    for (index, phase) in task.phases.iter().enumerate() {
        for action in &phase.required {
            if !action_ids.contains(action) {
                return Err(fail(format!("相位 {index} 要求了未声明的动作 `{action}`")));
            }
        }
        if !options.contains(&phase.cause) {
            return Err(fail(format!(
                "相位 {index} 的真值 `{}` 不在 `{CAUSE_QUESTION}` 的选项集里",
                phase.cause
            )));
        }
    }
    if task.acceptance.bind_deliverable
        && !task
            .tracked
            .iter()
            .any(|tracked| tracked.question.as_str() == DELIVERABLE_QUESTION)
    {
        return Err(fail(format!(
            "要求交付物绑定时必须跟踪 `{DELIVERABLE_QUESTION}`"
        )));
    }

    Ok(FamilyTask {
        id: task.id,
        notes: task.notes,
        rounds: task.rounds,
        initial: task
            .initial
            .into_iter()
            .map(|entry| NewEvidence {
                source: entry.source,
                at_unix_ms: entry.at_unix_ms,
                object: entry.object,
                version: entry.version,
                content: entry.content,
            })
            .collect(),
        tracked: task.tracked,
        extra_dependencies: task.extra_dependencies,
        steps: task.steps,
        actions: task.actions,
        phases: task.phases,
        bind_deliverable: task.acceptance.bind_deliverable,
    })
}

/// 环境的执行状态：相位游标 + 已经注入的扰动。
pub struct Environment<'a> {
    task: &'a FamilyTask,
    applied: usize,
}

impl<'a> Environment<'a> {
    /// 为一个任务新建环境。
    pub fn new(task: &'a FamilyTask) -> Self {
        Self { task, applied: 0 }
    }

    /// 当前相位编号（已应用扰动条数）。
    pub fn phase(&self) -> usize {
        self.applied
    }

    /// 把初始快照写进证据日志。初始追加发生在任何判断之前，因此不产生失效传播。
    pub fn seed(&self, state: &mut SemanticState) -> Result<(), Error> {
        for entry in &self.task.initial {
            state.append_evidence(entry.clone())?;
        }
        Ok(())
    }

    /// 注入某一轮、某个时机的全部扰动，返回它们造成的效果。
    ///
    /// 只消费脚本**队首**那一段：脚本已按（轮、提问前 < 提问后）排好序，因此按顺序应用前缀即可。
    /// 「提问后」的扰动由 [`crate::matching::run_task`] 在提问之后、落判断记录之前调用这里注入，
    /// 迟到的结果因此会被快照校验标成过期（研究目标 §5.2）；没有提问的轮次里，同一个调用在轮末兜底。
    pub fn apply_round(
        &mut self,
        round: usize,
        when: StepWhen,
        state: &mut SemanticState,
    ) -> Result<Vec<StepEffect>, Error> {
        let mut effects = Vec::new();
        while let Some(step) = self.task.steps.get(self.applied) {
            if step.round != round || step.when != when {
                break;
            }
            let step = step.clone();
            let invalidation = match &step.change {
                Change::Append {
                    source,
                    at_unix_ms,
                    object,
                    version,
                    content,
                } => {
                    let (_, invalidation) = state.append_evidence(NewEvidence {
                        source: source.clone(),
                        at_unix_ms: *at_unix_ms,
                        object: object.clone(),
                        version: *version,
                        content: content.clone(),
                    })?;
                    invalidation
                }
                Change::Retract { object, version } => {
                    let target = state
                        .evidence()
                        .entries()
                        .iter()
                        .find(|entry| {
                            entry.object == *object
                                && entry.version == *version
                                && !state.evidence().is_retracted(entry.id)
                        })
                        .map(|entry| entry.id)
                        .ok_or_else(|| {
                            invalid(&format!(
                                "任务 {}：撤回目标不存在（{object} v{version} 上没有仍有效的记录）",
                                self.task.id
                            ))
                        })?;
                    state.retract_evidence(target)?
                }
            };
            self.applied += 1;
            effects.push(StepEffect {
                round,
                when,
                phase: self.applied,
                at_unix_ms: crate::exp::unix_ms(),
                note: step.note.clone(),
                change: step.change.clone(),
                invalidation,
            });
        }
        Ok(effects)
    }
}

/// 独立验收的输入：控制器留下的执行轨迹里与验收有关的部分。
///
/// 完成声明只以计数形式进来（`claims`），三条判定都不读它的文本——研究目标 §10.1
/// 「真实任务成功率由隐藏环境状态、独立验收或隐藏测试判断，不能由 Agent 的完成声明替代」。
#[derive(Debug, Clone)]
pub struct AcceptanceInput<'a> {
    /// 已发出的动作：`(轮, 动作)`。
    pub executions: &'a [(usize, ActionId)],
    /// 交付物相关判断的输入投影（按记录顺序）。
    pub deliverable_reads: &'a [InputProjection],
    /// 控制器写下的完成声明条数（只记录，不作证据）。
    pub claims: usize,
    /// 未取得答案的动作决策次数（弃权：问题没答上来，动作发不出去）。
    pub abstentions: usize,
}

/// 一条验收判定。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AcceptanceCheck {
    /// 判定名。
    pub name: String,
    /// 是否通过。
    pub passed: bool,
    /// 依据说明。
    pub detail: String,
}

/// 独立验收结果。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Acceptance {
    /// 三条判定全部通过才算通过。
    pub passed: bool,
    /// 逐条判定。
    pub checks: Vec<AcceptanceCheck>,
    /// 口径说明。
    pub notes: Vec<String>,
}

/// 独立验收：只读证据日志与隐藏真值。
///
/// 三条判定：
///
/// 1. **必需动作真的发出了**：决策相位上被要求的动作，都在它第一次被要求的那一轮或之后发出；
/// 2. **没有发出不该发出的动作**：提前发出、或候选集里根本没有该发的动作时发出，都算错误动作；
/// 3. **完成声明绑定实际交付物版本**（`bind_deliverable` 时）：存在一条读交付物**当前版本**的
///    `deliverable_ready` 判断，且这条投影在验收时仍是当前输入——只按被取代的旧版本判定过交付物的
///    运行，在这里对不上（研究目标 §5.2 末条）。
pub fn accept(task: &FamilyTask, input: &AcceptanceInput<'_>, log: &dyn EvidenceLog) -> Acceptance {
    let mut checks = Vec::new();

    let required = task.required_actions();
    let executed: BTreeSet<&ActionId> = input.executions.iter().map(|(_, id)| id).collect();
    let missing: Vec<String> = required
        .iter()
        .filter(|action| !executed.contains(action))
        .map(|action| action.to_string())
        .collect();
    checks.push(AcceptanceCheck {
        name: "required_actions_executed".to_owned(),
        passed: missing.is_empty(),
        detail: if missing.is_empty() {
            format!("{} 个必需动作都已发出", required.len())
        } else {
            format!("未发出的必需动作：{}", missing.join("、"))
        },
    });

    let wrong: Vec<String> = input
        .executions
        .iter()
        .filter(|(round, action)| !task.justified_at_round(*round, action))
        .map(|(round, action)| format!("第 {round} 轮 {action}"))
        .collect();
    checks.push(AcceptanceCheck {
        name: "no_unjustified_actions".to_owned(),
        passed: wrong.is_empty(),
        detail: if wrong.is_empty() {
            format!("{} 次执行都在正当轮次", input.executions.len())
        } else {
            format!("不该发出的动作：{}", wrong.join("、"))
        },
    });

    let current = log.current_version(&ObjectId::from(DELIVERABLE_OBJECT));
    let (bound, detail) = match (task.bind_deliverable, current) {
        (false, _) => (true, "本任务不要求交付物绑定".to_owned()),
        (true, None) => (false, format!("证据日志里没有 {DELIVERABLE_OBJECT} 的记录")),
        (true, Some(version)) => {
            let hit = input.deliverable_reads.iter().find(|projection| {
                projection.version_of(&ObjectId::from(DELIVERABLE_OBJECT)) == Some(version)
                    && verify_snapshot(log, projection) == SnapshotCheck::Current
            });
            match hit {
                Some(_) => (
                    true,
                    format!("完成声明绑定到 {DELIVERABLE_OBJECT} v{version} 的当前证据"),
                ),
                None => (
                    false,
                    format!(
                        "没有任何 `{DELIVERABLE_QUESTION}` 判断读到 {DELIVERABLE_OBJECT} 的当前版本 v{version}"
                    ),
                ),
            }
        }
    };
    checks.push(AcceptanceCheck {
        name: "claim_bound_to_deliverable_version".to_owned(),
        passed: bound,
        detail,
    });

    Acceptance {
        passed: checks.iter().all(|check| check.passed),
        checks,
        notes: vec![
            format!(
                "完成声明 {} 条、弃权 {} 次：只作记录，不参与上面三条判定",
                input.claims, input.abstentions
            ),
            "验收读的是证据日志与隐藏真值，不读控制器写的完成声明".to_owned(),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jh_core::{MemoryEvidenceLog, ObjectId as Oid};

    #[test]
    fn the_embedded_family_loads_and_carries_the_fixed_question_set() {
        let family = FamilyA::load().expect("内嵌任务族 A 应自洽");

        assert_eq!(family.name, "family-a");
        assert_eq!(family.tasks.len(), 6, "先导规模 6–10 个任务");
        assert_eq!(family.questions.len(), 4, "固定问题集四个问题");
        assert!(family.task("A5").is_some());

        // 同一 ID 改含义必须升版本：cause_primary 有 v1、v2 两个历史版本，当前用 v2。
        let versions: Vec<QuestionVersion> = family
            .history()
            .iter()
            .filter(|entry| entry.question.as_str() == CAUSE_QUESTION)
            .map(|entry| entry.version)
            .collect();
        assert_eq!(versions, vec![QuestionVersion(1), QuestionVersion(2)]);
        assert_eq!(
            family
                .question(&QuestionId::from(CAUSE_QUESTION))
                .unwrap()
                .version,
            QuestionVersion(2)
        );
        assert_eq!(
            family.scope(&QuestionId::from(CAUSE_QUESTION)).unwrap(),
            [ObjectId::from("app.log"), ObjectId::from("config")]
        );
        assert!(
            family
                .history()
                .iter()
                .any(|entry| entry.question.as_str() == CAUSE_QUESTION
                    && entry.version == QuestionVersion(2)
                    && entry.current)
        );
    }

    #[test]
    fn the_question_registry_refuses_a_second_meaning_on_the_same_version() {
        let family = FamilyA::load().expect("内嵌任务族 A 应自洽");
        let id = QuestionId::from(CAUSE_QUESTION);

        // 把 cause_primary v2 的版本号改回 v1：同一 ID + 同一版本上出现两份不同定义，必须显式失败。
        let tampered = FAMILY_A_JSON.replace(
            "\"version\": 2,\n      \"reads\": [\"app.log\", \"config\"],",
            "\"version\": 1,\n      \"reads\": [\"app.log\", \"config\"],",
        );
        let error = FamilyA::from_json(&tampered)
            .err()
            .expect("同版本换含义必须显式失败");
        assert!(error.to_string().contains("cause_primary"), "{error}");
        assert!(family.registry().at(&id, QuestionVersion(2)).is_some());
    }

    #[test]
    fn phase_mapping_matches_the_perturbation_script() {
        let family = FamilyA::load().unwrap();
        let a4 = family.task("A4").unwrap();

        // A4：r0 的「提问后」扰动会推翻前提，r2 才产出交付物。
        assert_eq!(a4.ask_phase(0), 0, "r0 提问时还没应用任何扰动");
        assert_eq!(a4.decision_phase(0), 1, "r0 决策时取回的结果已经过期");
        assert_eq!(a4.decision_phase(1), 1);
        assert_eq!(a4.decision_phase(2), 2);
        assert!(a4.required_at_round(0).is_empty());
        assert!(a4.required_actions().is_empty(), "A4 的正解是不发补救动作");
        assert_eq!(
            a4.first_required_round(&ActionId::from("rollback-cache")),
            None
        );

        // A1：补救动作从 r1 起被要求。
        let a1 = family.task("A1").unwrap();
        assert_eq!(
            a1.first_required_round(&ActionId::from("rollback-cache")),
            Some(1)
        );
        assert!(a1.justified_at_round(1, &ActionId::from("rollback-cache")));
        assert!(!a1.justified_at_round(0, &ActionId::from("rollback-cache")));
        assert!(!a1.justified_at_round(2, &ActionId::from("restart-service")));
    }

    #[test]
    fn truth_is_per_phase_and_matches_the_question_primitives() {
        let family = FamilyA::load().unwrap();
        let a1 = family.task("A1").unwrap();
        let cause = QuestionId::from(CAUSE_QUESTION);
        let premise = QuestionId::from(PREMISE_QUESTION);

        assert_eq!(
            a1.truth_at(0, &cause),
            Some(Truth::Choice {
                label: "other".to_owned()
            })
        );
        assert_eq!(a1.truth_at(1, &premise), Some(Truth::Noul { yes: true }));
        assert_eq!(a1.truth_at(0, &premise), Some(Truth::Noul { yes: false }));
        assert_eq!(a1.truth_at(9, &cause), None);
        assert_eq!(a1.truth_at(0, &QuestionId::from("never_declared")), None);
    }

    #[test]
    fn applying_the_script_advances_phases_and_retracts_evidence() {
        let family = FamilyA::load().unwrap();
        let a5 = family.task("A5").unwrap();
        let mut state = SemanticState::in_memory();
        let mut env = Environment::new(a5);
        env.seed(&mut state).expect("初始快照应能落库");

        assert_eq!(env.phase(), 0);
        let before = env.apply_round(0, StepWhen::BeforeAsk, &mut state).unwrap();
        assert!(before.is_empty());
        assert_eq!(env.phase(), 0);

        // r1 的撤回发生在提问之后。
        assert!(
            env.apply_round(1, StepWhen::BeforeAsk, &mut state)
                .unwrap()
                .is_empty()
        );
        let effects = env.apply_round(1, StepWhen::AfterAsk, &mut state).unwrap();
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0].phase, 1);
        assert_eq!(effects[0].change.object(), &ObjectId::from("app.log"));
        // 撤回之后 app.log 上仍有仍有效的记录吗？A5 只有 v1，因此当前版本为空。
        assert_eq!(
            state.evidence().current_version(&ObjectId::from("app.log")),
            None
        );
    }

    #[test]
    fn acceptance_reads_the_log_and_the_hidden_truth_not_the_claim() {
        let family = FamilyA::load().unwrap();
        let a1 = family.task("A1").unwrap();
        let mut log = MemoryEvidenceLog::default();
        log.append(NewEvidence {
            source: "tool:write_report".to_owned(),
            at_unix_ms: 1,
            object: Oid::from(DELIVERABLE_OBJECT),
            version: ObjectVersion(1),
            content: Value::Null,
        })
        .expect("内存日志不会失败");

        // 只在轨迹里写「已完成」、没有任何执行：验收不通过。
        let claimed_only = accept(
            a1,
            &AcceptanceInput {
                executions: &[],
                deliverable_reads: &[],
                claims: 3,
                abstentions: 2,
            },
            &log,
        );
        assert!(!claimed_only.passed);
        assert!(
            !claimed_only
                .checks
                .iter()
                .find(|check| check.name == "required_actions_executed")
                .unwrap()
                .passed
        );

        // 发出必需动作、且有一条读到交付物当前版本的判断：通过。
        let deliverable_read =
            InputProjection::covering([(Oid::from(DELIVERABLE_OBJECT), ObjectVersion(1))]);
        let good = accept(
            a1,
            &AcceptanceInput {
                executions: &[(1, ActionId::from("rollback-cache"))],
                deliverable_reads: std::slice::from_ref(&deliverable_read),
                claims: 1,
                abstentions: 0,
            },
            &log,
        );
        assert!(good.passed, "{good:?}");

        // 提前发出（第 0 轮）算错误动作。
        let premature = accept(
            a1,
            &AcceptanceInput {
                executions: &[(0, ActionId::from("rollback-cache"))],
                deliverable_reads: std::slice::from_ref(&deliverable_read),
                claims: 0,
                abstentions: 0,
            },
            &log,
        );
        assert!(!premature.passed);
        assert!(
            premature
                .checks
                .iter()
                .any(|check| check.name == "no_unjustified_actions" && !check.passed)
        );

        // 读到的是被取代的旧版本：交付物绑定不成立。
        let stale_read =
            InputProjection::covering([(Oid::from(DELIVERABLE_OBJECT), ObjectVersion(9))]);
        let unbound = accept(
            a1,
            &AcceptanceInput {
                executions: &[(1, ActionId::from("rollback-cache"))],
                deliverable_reads: std::slice::from_ref(&stale_read),
                claims: 1,
                abstentions: 0,
            },
            &log,
        );
        assert!(!unbound.passed);
        assert!(
            unbound
                .checks
                .iter()
                .any(|check| check.name == "claim_bound_to_deliverable_version" && !check.passed)
        );
    }
}
