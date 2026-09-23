//! M4 综合评估：主对照四臂、诊断对照、原生基线、两条残余消融与检查密度扫描
//! （`docs/RESEARCH_GOALS.md` §9.1–§9.4、`docs/roadmap-1.0.md` §5 M4）。
//!
//! # 臂的建模（两个开关，不是四个字符串）
//!
//! 主对照是 §9.1 的 2×2：**控制结构**（循环式控制与重规划 / 依赖维护与结构精化）×
//! **判断通道**（普通 LLM 结构化判断 / Jev 判断）。臂由 [`Arm`] 表示，四组组合见 [`Arm::ALL`]。
//! 「只切换所属维度」在实现上是结构性的：同一族、同一任务、同一初值、同一判定器构造路径，
//! 组间差异只有 [`Arm::loop_arm`] 与 [`Arm::channel`] 两个取值。
//!
//! 两族任务怎么接到这两个维度上（**不是**把维度弱化，而是把它落到本仓库已有机制上）：
//!
//! | 族 | 控制结构维 | 判断通道维 | 驱动 |
//! | --- | --- | --- | --- |
//! | A / B | 循环式控制 = `Arm { propagation: false, recompute: Full }`（每轮全量重问、不做依赖维护）；依赖维护 = `{ propagation: true, recompute: Incremental }` | `judge::llm` / `judge::jev`，同一份问题集与读取范围 | [`crate::matching::run_task`] |
//! | C | 循环式控制 = 只用族 C 之前就存在的可观察量（不建观察通道）；依赖维护 = 声明式测量契约 + 独立检验后启用的观察通道 | 同一份证据材料交给两个判断通道（同一问题、同一选项），真值取隐藏检查 | [`crate::c_program::run_case`] + 本模块的判读臂 |
//!
//! # 诊断对照（§9.1 第 2 段）
//!
//! 「强制提供相同新增证据」在 A / B 的实现是[`Evidence::Equalized`]：所有臂的调度一律按
//! `Recompute::Full` 走（每轮把跟踪的问题全问一遍），因此手上的答案集合逐轮相同，剩下的差别只能是
//! 判断通道与依赖维护记录。C 的实现是把**同一份证据文本**交给两个通道（同族内的两臂材料逐字节相同）。
//!
//! # 其余三项
//!
//! - **原生基线**（§9.1 第 3 段）：[`run_native`] 让生成模型直接协调动作——没有类型化问题、没有 Jev、
//!   没有依赖维护；环境、动作候选集、计价与独立验收与对照组完全相同。
//! - **残余消融**（§9.2 第 6、7 行）：[`run_reuse_ablation`]（禁止跨任务复用：同一族的观察通道
//!   逐用例重建 vs 建一次复用）与 [`Cadence`]（固定频率检查 vs 依赖触发，H5）。
//! - **密度扫描**（§9.4）：检查密度（[`Cadence`]）× 规模（每族任务数），读数按 §10.1 主要指标与
//!   §10.2 诊断指标采集。
//!
//! # 口径
//!
//! - 完成声明不构成证据：族 A / B 的任务结果由 [`crate::family::accept`] 独立验收判定；族 C 的判定
//!   真值取 [`crate::c_program::hidden_check`] 的隐藏检查（`RunRecord.hidden`），**臂说「通过」不等于
//!   没有违反**。渲染证据文本时排除 `hidden` 与 `journal`（后两者是评分与独立检查专用通道）。
//! - 读数与口径一起落盘：报告里的 `protocol.metric_definitions` 是每条读数的定义原文，
//!   与 [`LoopReadings`] 的字段一一对应（同名口径与 [`crate::compare`] 的 M1 实现一致，差别只在
//!   本模块多出了「密度」「诊断」「复用」三组分组维度）。
//! - 失败如实计入：调用失败不重试、不换后端，落进调用记录与 `failed_asks`，并让进程退出码非 0。
//! - 概率不塌缩：族 A / B 的 Brier 与对数损失取自判断记录的原始分布；族 C 的判读按声明的阈值规则
//!   读标签，同时把整份分布写进 `channel-cases.jsonl`。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use jh_core::{
    ActionId, Answer, ChoiceQuestion, EvidenceLog, InputProjection, Judge, JudgeRequest,
    JudgmentRecord, NewEvidence, ObjectId, Question, QuestionId, QuestionSpec, QuestionVersion,
    SemanticState, SnapshotId, State,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::c_measure::{self, MeasureCall};
use crate::c_program::{self, Attachment, CFamily, Implementation, ProgramVersion, RunRecord};
use crate::calls::{CallEntry, CallLog, read_entries};
use crate::compare;
use crate::error::Error;
use crate::exp::unix_ms;
use crate::family::{
    Acceptance, AcceptanceInput, ActionSpec, DELIVERABLE_OBJECT, Environment, FamilyA, FamilyTask,
    QuestionSet, StepWhen, accept,
};
use crate::family_b::FamilyB;
use crate::judge::JudgeName;
use crate::judge::llm;
use crate::matching::{self, ASK_SUCCEEDED, Cadence, DecisionOutcome, LoopContext, TaskTrace};
use crate::metrics;
use crate::report::{CallStats, LatencyStats, UsageStats, Verification, VerificationStatus};

/// 控制结构维（§9.1 的横轴）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Structure {
    /// 循环式控制与重规划：不做依赖维护，每轮按全量重算重新规划。
    Loop,
    /// 依赖维护与结构精化：失效传播 + 增量复用（族 C 落到观察通道）。
    Maintained,
}

impl Structure {
    /// 报告里的标签。
    pub fn label(self) -> &'static str {
        match self {
            Self::Loop => "loop",
            Self::Maintained => "maintained",
        }
    }

    /// 中文说明。
    pub fn description(self) -> &'static str {
        match self {
            Self::Loop => "循环式控制与重规划（不维护依赖，每轮全量重问）",
            Self::Maintained => "依赖维护与结构精化（失效传播 + 增量复用）",
        }
    }
}

/// 判断通道维（§9.1 的纵轴）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JudgeSide {
    /// 普通 LLM 结构化判断。
    Llm,
    /// Jev 判断。
    Jev,
}

impl JudgeSide {
    /// 报告里的标签。
    pub fn label(self) -> &'static str {
        match self {
            Self::Llm => "llm",
            Self::Jev => "jev",
        }
    }

    /// 构造判定器用的名字。
    pub fn name(self) -> JudgeName {
        match self {
            Self::Llm => JudgeName::Llm,
            Self::Jev => JudgeName::Jev,
        }
    }
}

/// 一组主对照臂：控制结构 × 判断通道（§9.1 的四格）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Arm {
    /// 控制结构。
    pub structure: Structure,
    /// 判断通道。
    pub judge: JudgeSide,
}

impl Arm {
    /// 四组臂，顺序即报告顺序（§9.1 表的 A / B / C / D）。
    pub const ALL: [Self; 4] = [
        Self {
            structure: Structure::Loop,
            judge: JudgeSide::Llm,
        },
        Self {
            structure: Structure::Loop,
            judge: JudgeSide::Jev,
        },
        Self {
            structure: Structure::Maintained,
            judge: JudgeSide::Llm,
        },
        Self {
            structure: Structure::Maintained,
            judge: JudgeSide::Jev,
        },
    ];

    /// 臂标识（文件名与运行标识的组成部分）。
    pub fn label(self) -> String {
        format!("{}-{}", self.structure.label(), self.judge.label())
    }

    /// 中文说明。
    pub fn description(self) -> String {
        format!(
            "{} × {}",
            self.structure.description(),
            match self.judge {
                JudgeSide::Llm => "普通 LLM 结构化判断",
                JudgeSide::Jev => "Jev 判断",
            }
        )
    }

    /// 判断通道。
    pub fn channel(self) -> JudgeName {
        self.judge.name()
    }

    /// 接到匹配循环上的 H1 / H2 开关（族 A / B）。
    pub fn loop_arm(self) -> matching::Arm {
        match self.structure {
            Structure::Loop => matching::Arm {
                propagation: false,
                recompute: matching::Recompute::Full,
            },
            Structure::Maintained => matching::Arm {
                propagation: true,
                recompute: matching::Recompute::Incremental,
            },
        }
    }

    /// 从 CLI 标识解析。
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|arm| arm.label() == label)
    }
}

/// 证据口径（§9.1 第 2 段的诊断对照开关）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Evidence {
    /// 各臂按自己的结构取得证据（族 A / B 按臂的重算口径调度）。
    Own,
    /// 强制提供相同的新增证据：所有臂一律全量重问（族 C 的两臂材料本就逐字节相同）。
    Equalized,
}

impl Evidence {
    /// 报告里的标签。
    pub fn label(self) -> &'static str {
        match self {
            Self::Own => "own",
            Self::Equalized => "equalized",
        }
    }
}

/// 跨任务复用开关（残余消融 1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelReuse {
    /// 复用：同一族建一次观察通道，族内全部用例共用。
    Reuse,
    /// 禁止跨任务复用：每个用例都重新生成契约、重建并重新独立检验观察通道。
    Rebuild,
}

impl ChannelReuse {
    /// 报告里的标签。
    pub fn label(self) -> &'static str {
        match self {
            Self::Reuse => "reuse",
            Self::Rebuild => "rebuild-per-case",
        }
    }
}

/// 一次 M4 运行的参数。
#[derive(Debug, Clone)]
pub struct M4Options {
    /// 运行标识（文件与调用记录用）。
    pub run: String,
    /// 调用记录落盘路径（JSONL，追加写）。
    pub calls_path: PathBuf,
    /// 运行产物目录。
    pub out_dir: PathBuf,
    /// 复现命令（原样记录本次调用，key 只写环境变量名）。
    pub command: String,
    /// 要跑的臂；空表示 [`Arm::ALL`]。
    pub arms: Vec<Arm>,
    /// 主对照与诊断对照的重复次数（每格配对重复：同任务、同顺序）。
    pub repeats: usize,
    /// 每族（A / B）取前 N 个任务；`None` 表示全部。
    pub max_tasks: Option<usize>,
    /// 族 C 每族取前 N 条调度；`None` 表示全部。
    pub max_schedules: Option<usize>,
    /// 密度扫描的检查节奏轴（首项同时是主对照用的节奏）。
    pub cadences: Vec<Cadence>,
    /// 密度扫描的规模轴（每族任务数）。
    pub scales: Vec<usize>,
    /// 是否跑原生基线臂。
    pub native: bool,
    /// 是否跑跨任务复用消融。
    pub ablate_reuse: bool,
    /// 用本地判定器替代两个判断通道（离线复跑用；报告里如实标「未验证」）。
    pub channel_override: Option<JudgeName>,
    /// 复用已落盘的契约产物（不重发生成调用）。
    pub contracts: Option<PathBuf>,
}

/// 一次 M4 运行的结果。
#[derive(Debug)]
pub struct M4Outcome {
    /// 报告。
    pub report: M4Report,
    /// 是否没有失败调用（驱动进程退出码，不是结论）。
    pub clean: bool,
}

/// 运行标识。
pub fn new_run_id() -> String {
    format!("m4-{}", unix_ms())
}

/// 主对照与密度扫描用到的判定器（拼复现命令用）。
pub const JUDGE_CHANNELS: [JudgeName; 2] = [JudgeName::Llm, JudgeName::Jev];

/// 一族任务的声明读取范围（族 A 与族 B 都有这个方法，名字一样；这里只做一层薄适配，
/// 免得把「漏失效 / 误失效」的对照口径抄第二遍）。
trait ScopeOf {
    /// 问题声明的读取对象（缺失时按空范围读）。
    fn declared_scope(&self, question: &QuestionId) -> Option<&[ObjectId]>;
}

impl ScopeOf for FamilyA {
    fn declared_scope(&self, question: &QuestionId) -> Option<&[ObjectId]> {
        self.scope(question)
    }
}

impl ScopeOf for FamilyB {
    fn declared_scope(&self, question: &QuestionId) -> Option<&[ObjectId]> {
        self.scope(question)
    }
}

/// 一组（臂 × 族 × 重复）格的读数口径。
///
/// 与 [`crate::compare`] 的 M1 累加器同口径：决策机会、旧依据行动率、恢复延迟、漏失效 / 误失效、
/// 重算量与缓存复用率、概率指标（Brier / 对数损失）逐条同义，差别只在分组维度
/// （这里是「臂 × 族 × 重复 / 节奏 / 规模」）。
#[derive(Debug, Clone, Default, Serialize)]
pub struct LoopReadings {
    /// 任务数。
    pub tasks: u32,
    /// 独立验收通过的任务数。
    pub acceptance_passed: u32,
    /// 决策机会数（= 各任务轮数之和）。
    pub opportunities: u32,
    /// 发出的动作数。
    pub executions: u32,
    /// 不该发出的动作数（对照 [`FamilyTask::justified_at_round`]）。
    pub wrong_actions: u32,
    /// 基于已失效依据发出的动作数（旧依据行动率 = 它 / 机会数）。
    pub stale_basis_commitments: u32,
    /// 旧依据行动率。
    pub stale_basis_action_rate: Option<f64>,
    /// 被有效性检查挡下的动作数。
    pub blocked: u32,
    /// 前提门不成立的动作数。
    pub gate_unsatisfied: u32,
    /// 弃权（手上没有答案）次数。
    pub abstentions: u32,
    /// 完成声明条数（只作记录，不作证据）。
    pub claims: u32,
    /// 动作计价成本（发出的动作才计）。
    pub action_cost_units: f64,
    /// 观察类动作数（带产出的动作）。
    pub observation_actions: u32,
    /// 观察类动作的计价成本。
    pub observation_cost_units: f64,
    /// 判断请求数。
    pub asks: u32,
    /// 失败的判断请求数。
    pub failed_asks: u32,
    /// 判断节点数。
    pub judged_nodes: u32,
    /// 判分数。
    pub judgments: u32,
    /// 与隐藏真值一致数。
    pub agreements: u32,
    /// Brier 均值（对隐藏真值的有标签判分）。
    pub brier_mean: Option<f64>,
    /// 对数损失均值。
    pub log_loss_mean: Option<f64>,
    /// 对数损失无定义的判分数。
    pub log_loss_undefined: u32,
    /// 重算输入量（判断请求里 state 的字节数之和）。
    pub input_bytes: u64,
    /// 复用事件数。
    pub reuse_events: u32,
    /// 复用时已经不新鲜的事件数。
    pub stale_reuse_events: u32,
    /// 缓存复用率 = 复用 /（复用 + 请求）。
    pub cache_reuse_rate: Option<f64>,
    /// 被标记失效的判断节点数。
    pub marked_nodes: u32,
    /// 被标记待复核的动作数。
    pub marked_actions: u32,
    /// 漏失效（声明范围含该对象、却没有被标记）。
    pub missed: u32,
    /// 误失效（不在声明范围内、却被标记）。
    pub extra: u32,
    /// 无关结果保留（不在声明范围内、也没有被标记）。
    pub unrelated_preserved: u32,
    /// 必需动作总数。
    pub required_actions: u32,
    /// 已恢复的必需动作数。
    pub recovered_actions: u32,
    /// 未恢复的必需动作（`任务:动作`）。
    pub unrecovered: Vec<String>,
    /// 恢复动作发生在第几轮。
    pub recovery_rounds: Vec<u32>,
}

/// 一次重复里的读数摘要（方差估计的样本点）。
#[derive(Debug, Clone, Serialize)]
pub struct RepeatReading {
    /// 第几次重复（从 1 起）。
    pub repeat: usize,
    /// 任务数。
    pub tasks: u32,
    /// 验收通过数。
    pub acceptance_passed: u32,
    /// 验收通过率。
    pub acceptance_rate: Option<f64>,
    /// 发出的动作数。
    pub executions: u32,
    /// 不该发出的动作数。
    pub wrong_actions: u32,
    /// 旧依据承诺数。
    pub stale_basis_commitments: u32,
    /// 判断请求数。
    pub asks: u32,
    /// 重算输入量。
    pub input_bytes: u64,
    /// 判断调用数（调用记录里这一格的记录条数）。
    pub calls: u32,
    /// 输入 token（通道上报过才有）。
    pub input_tokens: Option<u64>,
    /// 输出 token。
    pub output_tokens: Option<u64>,
}

/// 跨重复的样本方差（`n < 2` 时为 `None`；样本标准差，自由度 n−1）。
#[derive(Debug, Clone, Serialize)]
pub struct Variance {
    /// 样本数（重复次数）。
    pub samples: usize,
    /// 验收通过率的样本标准差。
    pub acceptance_rate_sd: Option<f64>,
    /// 判断调用数的样本标准差。
    pub calls_sd: Option<f64>,
    /// 重算输入量的样本标准差。
    pub input_bytes_sd: Option<f64>,
    /// 不该发出的动作数的样本标准差。
    pub wrong_actions_sd: Option<f64>,
}

/// 一个循环格（臂 × 族 × 重复集合）的读数。
#[derive(Debug, Clone, Serialize)]
pub struct LoopCell {
    /// 用途：`main`（主对照）/ `diagnostic`（诊断对照）/ `density`（密度扫描）。
    pub purpose: String,
    /// 臂标识。
    pub arm: String,
    /// 控制结构标签。
    pub structure: String,
    /// 判断通道标签。
    pub judge: String,
    /// 族标签（`A` / `B`）。
    pub family: String,
    /// 证据口径。
    pub evidence: String,
    /// 检查节奏。
    pub cadence: String,
    /// 规模轴取值（每族任务数；`None` 表示密度扫描之外）。
    pub scale: Option<usize>,
    /// 接到匹配循环上的 H1 / H2 开关。
    pub loop_arm: String,
    /// 判定器的验证状态。
    pub verification: Verification,
    /// 重复次数。
    pub repeats: usize,
    /// 逐重复读数。
    pub per_repeat: Vec<RepeatReading>,
    /// 跨重复方差。
    pub variance: Variance,
    /// 汇总读数。
    pub readings: LoopReadings,
    /// 判断调用统计（读调用记录）。
    pub calls: CallStats,
    /// 延迟分位数。
    pub latency_ms: LatencyStats,
    /// token 用量。
    pub usage: UsageStats,
    /// 轨迹文件（每任务一份）。
    pub traces: Vec<String>,
    /// 证据日志（每任务一份）。
    pub evidence_files: Vec<String>,
}

/// 累加器：把轨迹折成 [`LoopReadings`]。
#[derive(Debug, Default)]
struct Accum {
    readings: LoopReadings,
    brier: Vec<f64>,
    log_loss: Vec<f64>,
}

impl Accum {
    fn absorb(&mut self, family: &dyn ScopeOf, task: &FamilyTask, trace: &TaskTrace) {
        let readings = &mut self.readings;
        readings.tasks += 1;
        readings.opportunities += trace.rounds as u32;
        readings.claims += trace.claims.len() as u32;
        readings.judged_nodes += trace.judgments.len() as u32;
        readings.reuse_events += trace.reuses.len() as u32;
        readings.stale_reuse_events +=
            trace.reuses.iter().filter(|reuse| !reuse.fresh).count() as u32;
        readings.asks += trace.asks.len() as u32;
        readings.failed_asks += trace
            .asks
            .iter()
            .filter(|ask| ask.outcome != ASK_SUCCEEDED)
            .count() as u32;
        readings.input_bytes += trace
            .asks
            .iter()
            .map(|ask| ask.input_bytes as u64)
            .sum::<u64>();

        for judgment in &trace.judgments {
            readings.judgments += 1;
            if judgment.agreement == Some(true) {
                readings.agreements += 1;
            }
            if let Some(brier) = judgment.brier {
                self.brier.push(brier);
            }
            match judgment.log_loss {
                Some(loss) => self.log_loss.push(loss),
                None => readings.log_loss_undefined += 1,
            }
        }

        for decision in &trace.decisions {
            match decision.outcome {
                DecisionOutcome::Executed => {
                    readings.executions += 1;
                    readings.action_cost_units += decision.cost_units;
                    let observation = task
                        .action(&decision.action)
                        .is_some_and(|spec| spec.effect.is_some());
                    if observation {
                        readings.observation_actions += 1;
                        readings.observation_cost_units += decision.cost_units;
                    }
                    if !decision.basis_current {
                        readings.stale_basis_commitments += 1;
                    }
                    if !task.justified_at_round(decision.round, &decision.action) {
                        readings.wrong_actions += 1;
                    }
                }
                DecisionOutcome::Blocked => readings.blocked += 1,
                DecisionOutcome::GateUnsatisfied => readings.gate_unsatisfied += 1,
                DecisionOutcome::Unresolved => readings.abstentions += 1,
            }
        }

        // 失效传播读数：逐条扰动对照声明的读取范围（口径与 `compare` 的 M1 实现一致：
        // 只看这一步之前已经落过的判断，本轮新问出来的节点不算漏失效）。
        let mut node_question: BTreeMap<jh_core::NodeId, QuestionId> = BTreeMap::new();
        for judgment in &trace.judgments {
            node_question.insert(judgment.node, judgment.question.clone());
        }
        for step in &trace.steps {
            readings.marked_nodes += step.invalidation.stale.len() as u32;
            readings.marked_actions += step.invalidation.blocked.len() as u32;
            let marked: BTreeSet<&QuestionId> = step
                .invalidation
                .stale
                .iter()
                .filter_map(|node| node_question.get(node))
                .collect();
            let object = step.change.object();
            for tracked in &task.tracked {
                let has_nodes = trace.judgments.iter().any(|judgment| {
                    judgment.question == tracked.question && judgment.round < step.round
                });
                if !has_nodes {
                    continue;
                }
                let declared = family
                    .declared_scope(&tracked.question)
                    .unwrap_or_default()
                    .to_vec();
                let touched = declared.contains(object);
                let hit = marked.contains(&tracked.question);
                if touched && !hit {
                    readings.missed += 1;
                }
                if !touched && hit {
                    readings.extra += 1;
                }
                if !touched && !hit {
                    readings.unrelated_preserved += 1;
                }
            }
        }

        // 独立验收与恢复延迟。
        let acceptance = trace.acceptance.as_ref().expect("跑完的任务必然带独立验收");
        if acceptance.passed {
            readings.acceptance_passed += 1;
        }
        let executions = trace.executions();
        for action in task.required_actions() {
            readings.required_actions += 1;
            match executions.iter().find(|(round, executed)| {
                executed == &action && task.justified_at_round(*round, &action)
            }) {
                Some((round, _)) => {
                    readings.recovered_actions += 1;
                    readings.recovery_rounds.push(*round as u32);
                }
                None => readings.unrecovered.push(format!("{}:{}", task.id, action)),
            }
        }
    }

    fn finish(mut self) -> LoopReadings {
        let mean = |values: &[f64]| -> Option<f64> {
            (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
        };
        let readings = &mut self.readings;
        readings.brier_mean = mean(&self.brier);
        readings.log_loss_mean = mean(&self.log_loss);
        readings.stale_basis_action_rate = (readings.opportunities > 0).then(|| {
            f64::from(readings.stale_basis_commitments) / f64::from(readings.opportunities)
        });
        let reuse_total = readings.reuse_events + readings.asks;
        readings.cache_reuse_rate =
            (reuse_total > 0).then(|| f64::from(readings.reuse_events) / f64::from(reuse_total));
        self.readings
    }

    /// 折入一条原生基线的轨迹：与匹配循环共用同一套读数口径（没有的问题留空）。
    fn absorb_native(&mut self, task: &FamilyTask, trace: &NativeTrace) {
        let readings = &mut self.readings;
        readings.tasks += 1;
        readings.opportunities += trace.rounds as u32;
        readings.claims += trace.claims as u32;
        readings.asks += trace.calls;
        readings.failed_asks += trace.failures;
        readings.executions += trace.executions.len() as u32;
        readings.action_cost_units += trace.action_cost_units;
        readings.observation_actions += trace
            .executions
            .iter()
            .filter(|(_, action)| {
                task.action(action)
                    .is_some_and(|spec| spec.effect.is_some())
            })
            .count() as u32;
        for (round, action) in &trace.executions {
            if !task.justified_at_round(*round, action) {
                readings.wrong_actions += 1;
            }
        }
        if trace.acceptance.passed {
            readings.acceptance_passed += 1;
        }
        for action in task.required_actions() {
            readings.required_actions += 1;
            match trace.executions.iter().find(|(round, executed)| {
                executed.as_str() == action.as_str() && task.justified_at_round(*round, executed)
            }) {
                Some((round, _)) => {
                    readings.recovered_actions += 1;
                    readings.recovery_rounds.push(*round as u32);
                }
                None => readings.unrecovered.push(format!("{}:{}", task.id, action)),
            }
        }
    }
}

/// 一个格跑一次（一族 × 若干任务 × 一次重复）的结果。
#[derive(Debug)]
struct CellRun {
    readings: LoopReadings,
    run_ids: Vec<String>,
    traces: Vec<String>,
    evidence_files: Vec<String>,
    judge_failures: u32,
}

/// 一个格（臂 × 族）的定位信息。
#[derive(Debug, Clone, Copy)]
struct CellKey<'a> {
    arm: Arm,
    evidence: Evidence,
    cadence: Cadence,
    purpose: &'a str,
    family: &'a str,
    scale: Option<usize>,
    repeat: usize,
}

impl CellKey<'_> {
    /// 运行标识：`{run}-{purpose}-{arm}-{family}[-n{scale}]-r{repeat}`。
    fn run_id(&self, run: &str) -> String {
        let scale = self
            .scale
            .map_or_else(String::new, |scale| format!("-n{scale}"));
        format!(
            "{run}-{}-{}-{}{scale}-r{}",
            self.purpose,
            self.arm.label(),
            self.family,
            self.repeat
        )
    }
}

/// 跑一个格：同一族的一组任务，在给定的臂 / 节奏 / 证据口径下走一遍匹配循环。
async fn run_cell(
    family: &dyn QuestionSet,
    scope_of: &dyn ScopeOf,
    tasks: &[&FamilyTask],
    judge: &dyn Judge,
    key: &CellKey<'_>,
    options: &M4Options,
    calls: &mut CallLog,
) -> Result<CellRun, Error> {
    let run_id = key.run_id(&options.run);
    // 诊断对照：所有臂都按全量重问走，手上的答案集合逐轮相同（§9.1 第 2 段）。
    let loop_arm = match key.evidence {
        Evidence::Own => key.arm.loop_arm(),
        Evidence::Equalized => matching::Arm {
            propagation: key.arm.loop_arm().propagation,
            recompute: matching::Recompute::Full,
        },
    };
    let mut accum = Accum::default();
    let mut traces = Vec::new();
    let mut evidence_files = Vec::new();
    let mut judge_failures = 0;

    for task in tasks {
        let stem = format!("{run_id}-{}", task.id);
        let trace_path = options.out_dir.join("traces").join(format!("{stem}.json"));
        let evidence_path = options
            .out_dir
            .join("evidence")
            .join(format!("{stem}.jsonl"));
        let mut context = LoopContext {
            family,
            judge,
            channel: key.arm.channel(),
            arm: loop_arm,
            cadence: key.cadence,
            run: run_id.clone(),
            endpoint: compare::endpoint_for(key.arm.channel()),
            shadow: None,
            calls,
        };
        let trace = matching::run_task(&mut context, task, &evidence_path).await?;
        judge_failures += trace
            .asks
            .iter()
            .filter(|ask| ask.outcome != ASK_SUCCEEDED)
            .count() as u32;
        compare::write_json(&trace_path, &trace)?;
        accum.absorb(scope_of, task, &trace);
        traces.push(compare::absolute(&trace_path));
        evidence_files.push(compare::absolute(&evidence_path));
    }

    Ok(CellRun {
        readings: accum.finish(),
        run_ids: vec![run_id],
        traces,
        evidence_files,
        judge_failures,
    })
}

/// 原生基线臂的一条轨迹（§9.1 第 3 段：让生成模型直接协调任务）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeTrace {
    /// 任务标识。
    pub task: String,
    /// 臂标识（`native-llm`）。
    pub arm: String,
    /// 轮数。
    pub rounds: usize,
    /// 每轮提示词的字节数。
    pub prompt_bytes: Vec<u64>,
    /// 发出的动作：`(轮, 动作)`。
    pub executions: Vec<(usize, ActionId)>,
    /// 每轮的模型输出与解析结果。
    pub decisions: Vec<NativeRound>,
    /// 完成声明条数（只记录，不作证据）。
    pub claims: usize,
    /// 动作计价成本。
    pub action_cost_units: f64,
    /// 独立验收（与对照组同一条判定）。
    pub acceptance: Acceptance,
    /// 模型调用数。
    pub calls: u32,
    /// 失败调用数（含输出解析失败）。
    pub failures: u32,
}

/// 原生臂的一轮：原样保留模型输出，解析失败也留档。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeRound {
    /// 第几轮。
    pub round: usize,
    /// 模型输出原文。
    pub raw: String,
    /// 本轮真的发出的动作。
    pub actions: Vec<ActionId>,
    /// 模型是否声明任务已完成。
    pub done: bool,
    /// 失败原文（调用失败或输出不是约定的 JSON）。
    pub error: Option<String>,
}

/// 原生臂约定输出的 JSON 形状。
#[derive(Debug, Clone, Deserialize, Serialize)]
struct NativeDecision {
    #[serde(default)]
    actions: Vec<String>,
    #[serde(default)]
    done: bool,
    #[serde(default)]
    reason: String,
}

/// 把当前证据渲染成稳定的 JSON（按对象名与版本升序；已撤回的不出现）。
fn render_evidence(state: &SemanticState) -> Vec<Value> {
    let log = state.evidence();
    let mut by_object: BTreeMap<(String, u64), Value> = BTreeMap::new();
    for record in log.entries() {
        if log.is_retracted(record.id) {
            continue;
        }
        by_object.insert(
            (record.object.as_str().to_owned(), record.version.0),
            json!({
                "object": record.object.as_str(),
                "version": record.version.0,
                "source": record.source,
                "at_unix_ms": record.at_unix_ms,
                "content": record.content,
            }),
        );
    }
    by_object.into_values().collect()
}

/// 原生臂的提示词：现场（证据 + 动作表 + 自己发过的动作）+ 输出契约。
///
/// 动作表只给 id、计价与「是否观察类」，**不给前提门**——前提门是循环式控制那套类型化问题的产物，
/// 原生基线的定义就是「生成模型直接协调任务」，给它门就等于把它变成对照组的一个变体。
fn native_prompt(
    task: &FamilyTask,
    round: usize,
    state: &SemanticState,
    executed: &[(usize, ActionId)],
) -> String {
    let actions: Vec<Value> = task
        .actions
        .iter()
        .map(|spec: &ActionSpec| {
            json!({
                "id": spec.id.as_str(),
                "cost_units": spec.cost_units,
                "kind": if spec.effect.is_some() { "观察类（发出后会产出新证据）" } else { "处置类" },
            })
        })
        .collect();
    let done: Vec<String> = executed
        .iter()
        .map(|(round, action)| format!("第 {round} 轮 {action}"))
        .collect();
    let payload = json!({
        "任务": task.id,
        "轮": round,
        "总轮数": task.rounds,
        "任务说明": task.notes,
        "本任务可用的动作": actions,
        "已发出的动作": done,
        "当前证据": render_evidence(state),
    });

    format!(
        "你是一个直接协调任务的 Agent：手上没有结构化问题、没有依赖图，只有证据与候选动作。\n\
         按下面的现场决定这一轮要发出哪些动作（可以一个也不发，也可以发多个，按数组顺序执行），\
         认为任务已完成时把 done 置为 true。\n\
         只输出一个 JSON 对象，形如 {{\"actions\": [\"<动作 id>\"], \"done\": false, \"reason\": \"<一句话>\"}}；\
         不要输出 JSON 之外的任何文字；动作 id 必须来自「本任务可用的动作」，重复与未知的动作会被忽略。\n\n\
         现场（JSON）：\n{payload}"
    )
}

/// 发出一批动作：计价 + 观察类动作的产出进证据日志（与匹配循环里的执行语义一致）。
fn native_execute(
    task: &FamilyTask,
    state: &mut SemanticState,
    already: &mut BTreeSet<String>,
    requested: &[String],
) -> Result<(Vec<ActionId>, f64), Error> {
    let mut executed = Vec::new();
    let mut cost = 0.0;
    for id in requested {
        if already.contains(id) {
            continue;
        }
        let Some(spec) = task.action(&ActionId::from(id.clone())) else {
            continue;
        };
        if let Some(effect) = &spec.effect {
            state.append_evidence(NewEvidence {
                source: effect.source.clone(),
                at_unix_ms: unix_ms(),
                object: effect.object.clone(),
                version: effect.version,
                content: effect.content.clone(),
            })?;
        }
        already.insert(id.clone());
        executed.push(spec.id.clone());
        cost += spec.cost_units;
    }
    Ok((executed, cost))
}

/// 跑原生基线的一族任务（一条单循环）。
async fn run_native_cell(
    tasks: &[&FamilyTask],
    family: &str,
    client: &jh_openai::Client,
    repeat: usize,
    options: &M4Options,
    calls: &mut CallLog,
) -> Result<CellRun, Error> {
    let run_id = format!("{}-native-{family}-r{repeat}", options.run);
    let mut accum = Accum::default();
    let mut traces = Vec::new();
    let mut evidence_files = Vec::new();
    let mut failures = 0;

    for task in tasks {
        let mut state = SemanticState::in_memory();
        let mut environment = Environment::new(task);
        environment.seed(&mut state)?;
        let mut executed: Vec<(usize, ActionId)> = Vec::new();
        let mut executed_ids: BTreeSet<String> = BTreeSet::new();
        let mut rounds: Vec<NativeRound> = Vec::new();
        let mut prompt_bytes = Vec::new();
        let mut cost_units = 0.0;
        let mut task_failures: u32 = 0;
        let mut task_calls: u32 = 0;

        for round in 0..task.rounds {
            environment.apply_round(round, StepWhen::BeforeAsk, &mut state)?;
            let prompt = native_prompt(task, round, &state, &executed);
            let prompt_bytes_len = prompt.len() as u64;
            prompt_bytes.push(prompt_bytes_len);
            task_calls += 1;
            let started = Instant::now();
            let outcome = llm::complete(client, prompt).await;
            let latency = started.elapsed();
            let usage = outcome.as_ref().ok().and_then(|done| done.usage.clone());
            let (text, model, request_id, error): (
                String,
                Option<String>,
                Option<String>,
                Option<String>,
            ) = match &outcome {
                Ok(done) => (
                    done.text.clone(),
                    Some(done.model.clone()),
                    Some(done.request_id.clone()).filter(|id| !id.is_empty()),
                    None,
                ),
                Err(error) => (String::new(), None, None, Some(error.to_string())),
            };
            calls.append(&CallEntry::ModelCall {
                run: run_id.clone(),
                purpose: "native".to_owned(),
                case: task.id.clone(),
                source: jh_core::JudgeSource::Llm,
                version: model,
                request_id,
                usage,
                input_bytes: prompt_bytes_len,
                output_bytes: text.len() as u64,
                latency_ms: latency.as_millis() as u64,
                error: error.clone(),
            })?;

            let (actions, done, parse_error) = match (&outcome, text.as_str()) {
                (Err(_), _) => (Vec::new(), false, None),
                (Ok(_), text) => match parse_native_decision(text) {
                    Ok(decision) => (decision.actions, decision.done, None),
                    Err(message) => (Vec::new(), false, Some(message)),
                },
            };
            let (executed_now, cost) =
                native_execute(task, &mut state, &mut executed_ids, &actions)?;
            cost_units += cost;
            for action in &executed_now {
                executed.push((round, action.clone()));
            }
            if error.is_some() || parse_error.is_some() {
                task_failures += 1;
            }
            failures += u32::from(error.is_some() || parse_error.is_some());
            rounds.push(NativeRound {
                round,
                raw: text,
                actions: executed_now,
                done,
                error: error.clone().or(parse_error),
            });
            environment.apply_round(round, StepWhen::AfterAsk, &mut state)?;
        }

        // 完成声明：与循环式控制一样在轮末写一条，且同样不作证据。
        let claims = 1;
        // 交付物绑定的公平处置：原生臂直接读到当前证据，它的完成声明就是基于当前交付物版本做出的。
        let deliverable = ObjectId::from(DELIVERABLE_OBJECT);
        let deliverable_reads: Vec<InputProjection> = match (
            task.bind_deliverable,
            state.evidence().current_version(&deliverable),
        ) {
            (true, Some(version)) => vec![InputProjection::covering([(deliverable, version)])],
            _ => Vec::new(),
        };
        let acceptance = accept(
            task,
            &AcceptanceInput {
                executions: &executed,
                deliverable_reads: &deliverable_reads,
                claims,
                abstentions: task_failures as usize,
            },
            state.evidence(),
        );
        let trace = NativeTrace {
            task: task.id.clone(),
            arm: "native-llm".to_owned(),
            rounds: task.rounds,
            prompt_bytes,
            executions: executed.clone(),
            decisions: rounds,
            claims,
            action_cost_units: cost_units,
            acceptance,
            calls: task_calls,
            failures: task_failures,
        };
        let trace_path = options
            .out_dir
            .join("native")
            .join(format!("{run_id}-{}.json", task.id));
        let evidence_path = options
            .out_dir
            .join("native")
            .join(format!("{run_id}-{}.evidence.jsonl", task.id));
        compare::write_json(&trace_path, &trace)?;
        write_evidence_log(&evidence_path, state.evidence())?;
        accum.absorb_native(task, &trace);
        traces.push(compare::absolute(&trace_path));
        evidence_files.push(compare::absolute(&evidence_path));
    }

    Ok(CellRun {
        readings: accum.finish(),
        run_ids: vec![run_id],
        traces,
        evidence_files,
        judge_failures: failures,
    })
}

/// 原生臂输出解析：只接受约定的 JSON 对象（显式失败，不猜）。
fn parse_native_decision(text: &str) -> Result<NativeDecision, String> {
    let trimmed = text.trim();
    let body = trimmed
        .strip_prefix("```json")
        .and_then(|rest| rest.strip_suffix("```"))
        .map_or(trimmed, str::trim);
    serde_json::from_str::<NativeDecision>(body)
        .map_err(|error| format!("输出不是约定的 JSON：{error}"))
}

/// 把证据日志落成 JSONL（原生臂的原始数据位置）。
fn write_evidence_log(path: &Path, log: &dyn EvidenceLog) -> Result<(), Error> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    let mut text = String::new();
    for record in log.entries() {
        text.push_str(&serde_json::to_string(record)?);
        text.push('\n');
    }
    std::fs::write(path, text)?;
    Ok(())
}

/// 族 C 的观察通道装置（契约 + 观测器 + 独立检验 + 启用结果）。
struct ChannelSetup {
    contract: jh_core::ObserverContract,
    observer: c_measure::Observer,
    check: crate::generation::ContractCheck,
    independent: c_measure::IndependentCheck,
    enable: c_measure::EnableOutcome,
}

/// 一个族 C 用例：环境只跑一次，全部臂读同一份记录（同一份证据）。
struct ChannelCase {
    family: CFamily,
    schedule: String,
    implementation: Implementation,
    hidden: c_measure::Verdict,
    /// 循环式控制臂的材料：族 C 之前就存在的可观察量。
    observables: BTreeMap<String, Value>,
    /// 依赖维护臂的材料：观察通道（记录器 + 暂停点 + 提交计数器 + 被测版本）；没有可用通道时为 `None`。
    channel: Option<BTreeMap<String, Value>>,
}

/// 族 C 判读臂的一次用例读数（落 `channel-cases.jsonl`，概率原值保留）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelCaseReading {
    /// 臂标识。
    pub arm: String,
    /// 第几次重复。
    pub repeat: usize,
    /// 族。
    pub family: String,
    /// 调度。
    pub schedule: String,
    /// 实现路。
    pub implementation: String,
    /// 隐藏检查的真值标签。
    pub hidden: String,
    /// 真值说明。
    pub hidden_detail: String,
    /// 臂的判读标签。
    pub arm_verdict: String,
    /// 判读依据说明。
    pub arm_detail: String,
    /// 答案的概率分布原值（不塌缩成布尔）。
    pub probabilities: BTreeMap<String, f64>,
    /// 材料的字节数。
    pub material_bytes: u64,
    /// 调用失败原文。
    pub error: Option<String>,
}

/// 族 C 判读臂的计数（逐格汇算）。
#[derive(Debug, Clone, Default)]
struct ChannelCounts {
    cases: u32,
    opportunities: u32,
    clean_cases: u32,
    detections: u32,
    false_passes: u32,
    inconclusive_on_opportunity: u32,
    false_alarms: u32,
    correct_clears: u32,
    inconclusive_on_clean: u32,
    material_bytes: u64,
    calls: u32,
    failures: u32,
}

impl ChannelCounts {
    fn score(&mut self, hidden: &c_measure::Verdict, arm: &c_measure::Verdict) {
        // 隐藏检查自己失败：没有可信真值，这条用例不进任何分母（研究目标 §10.2）。
        let violated = match hidden.verdict {
            jh_core::ObserverVerdict::Violated => true,
            jh_core::ObserverVerdict::Satisfied => false,
            jh_core::ObserverVerdict::Failed(_) => return,
        };
        self.cases += 1;
        match (violated, &arm.verdict) {
            (true, jh_core::ObserverVerdict::Violated) => {
                self.opportunities += 1;
                self.detections += 1;
            }
            (true, jh_core::ObserverVerdict::Satisfied) => {
                self.opportunities += 1;
                self.false_passes += 1;
            }
            (true, jh_core::ObserverVerdict::Failed(_)) => {
                self.opportunities += 1;
                self.inconclusive_on_opportunity += 1;
            }
            (false, jh_core::ObserverVerdict::Satisfied) => {
                self.clean_cases += 1;
                self.correct_clears += 1;
            }
            (false, jh_core::ObserverVerdict::Violated) => {
                self.clean_cases += 1;
                self.false_alarms += 1;
            }
            (false, jh_core::ObserverVerdict::Failed(_)) => {
                self.clean_cases += 1;
                self.inconclusive_on_clean += 1;
            }
        }
    }
}

/// 族 C 一个格（臂 × 族 × 重复）的读数。
#[derive(Debug, Clone, Serialize)]
pub struct ChannelCell {
    /// 臂标识。
    pub arm: String,
    /// 控制结构标签。
    pub structure: String,
    /// 判断通道标签。
    pub judge: String,
    /// 族。
    pub family: String,
    /// 该族的要求原文。
    pub requirement: String,
    /// 材料种类：`observables` / `channel`。
    pub material: String,
    /// 判定器验证状态。
    pub verification: Verification,
    /// 重复次数。
    pub repeats: usize,
    /// 用例数（隐藏检查取到真值的用例）。
    pub cases: u32,
    /// 机会数（隐藏检查认定确实发生了违反的用例数）。
    pub opportunities: u32,
    /// 无违反用例数。
    pub clean_cases: u32,
    /// 检出数（真值违反、臂也说违反）。
    pub detections: u32,
    /// 误放行数（真值违反、臂说没有违反）。
    pub false_passes: u32,
    /// 机会上的弃权数。
    pub inconclusive_on_opportunity: u32,
    /// 误报数（真值干净、臂说违反）。
    pub false_alarms: u32,
    /// 判净数。
    pub correct_clears: u32,
    /// 干净用例上的弃权数。
    pub inconclusive_on_clean: u32,
    /// 检出率。
    pub detection_rate: Option<f64>,
    /// 误放行率（判据用的那一项）。
    pub false_pass_rate: Option<f64>,
    /// 误报率。
    pub false_alarm_rate: Option<f64>,
    /// 逐实现路的读数。
    pub by_implementation: BTreeMap<String, ChannelCountsReport>,
    /// 判断调用数。
    pub calls: u32,
    /// 失败调用数。
    pub failures: u32,
    /// 材料总字节数。
    pub material_bytes: u64,
    /// 逐用例原始读数文件。
    pub cases_file: String,
}

/// 逐实现路的计数（可序列化的镜像）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelCountsReport {
    /// 用例数。
    pub cases: u32,
    /// 机会数。
    pub opportunities: u32,
    /// 无违反用例数。
    pub clean_cases: u32,
    /// 检出数。
    pub detections: u32,
    /// 误放行数。
    pub false_passes: u32,
    /// 机会上的弃权数。
    pub inconclusive_on_opportunity: u32,
    /// 误报数。
    pub false_alarms: u32,
    /// 判净数。
    pub correct_clears: u32,
    /// 干净用例上的弃权数。
    pub inconclusive_on_clean: u32,
}

/// 把族 C 用例渲染成「循环式控制臂」的材料：族 C 之前就存在的可观察量。
fn observables_material(run: &RunRecord) -> BTreeMap<String, Value> {
    let mut material = BTreeMap::new();
    material.insert(
        "各任务自报状态".to_owned(),
        json!(&run.observables.statuses),
    );
    material.insert(
        "各任务自报写入意图数".to_owned(),
        json!(&run.observables.intents),
    );
    material.insert("当前值".to_owned(), json!(&run.observables.values));
    material.insert("当前版本号".to_owned(), json!(&run.observables.versions));
    material
}

/// 写入路径标签（与 serde 名一致）。
fn write_path_label(path: c_program::WritePath) -> &'static str {
    match path {
        c_program::WritePath::Boundary => "boundary",
        c_program::WritePath::Direct => "direct",
    }
}

/// 把族 C 用例渲染成「依赖维护臂」的材料：观察通道的读数（观测器判定读的就是这些通道）。
fn channel_material(run: &RunRecord) -> BTreeMap<String, Value> {
    let recorder: Vec<Value> = run
        .recorder
        .iter()
        .map(|event| {
            json!({
                "事件号": event.event,
                "序号": event.seq,
                "任务": event.task,
                "键": event.key,
                "操作": event.op,
                "值": event.value,
                "期望版本": event.expected_version,
                "写入后版本": event.version,
                "路径": write_path_label(event.path),
            })
        })
        .collect();
    let pause: Vec<Value> = run
        .pause
        .iter()
        .map(|event| {
            json!({
                "事件号": event.event,
                "任务": event.task,
                "暂停点": event.kind.label(),
            })
        })
        .collect();
    let mut material = BTreeMap::new();
    material.insert("被测版本".to_owned(), json!(run.version.id()));
    material.insert(
        "提交计数器读到的真实提交数".to_owned(),
        json!(run.commit_seq),
    );
    material.insert("写入边界记录器".to_owned(), json!(recorder));
    material.insert("可控暂停点轨迹".to_owned(), json!(pause));
    material
}

/// 用例输入：要求原文 + 调度计划（两个结构臂都看得到；实现路**不给**——它就是真值本身）。
fn case_input_material(family: CFamily, schedule: &c_program::Schedule) -> BTreeMap<String, Value> {
    let ops: Vec<Value> = schedule
        .ops
        .iter()
        .map(|op| json!(format!("{op:?}")))
        .collect();
    let mut material = BTreeMap::new();
    material.insert("要求".to_owned(), json!(family.requirement()));
    material.insert("要求出处".to_owned(), json!(family.requirement_source()));
    material.insert("被测对象".to_owned(), json!(family.object().as_str()));
    material.insert("本用例的调度计划".to_owned(), json!(ops));
    material
}

/// 族 C 判读臂的问题：一次运行是否满足要求（三个选项：违反 / 无违反 / 证据不足）。
fn requirement_question(family: CFamily) -> (QuestionId, BTreeMap<QuestionId, QuestionSpec>) {
    let id = QuestionId::from("m4_requirement_holds");
    let spec = QuestionSpec {
        version: QuestionVersion(1),
        question: Question::Choice(ChoiceQuestion {
            instructions: format!(
                "本次运行是否满足下面的要求？「{}」（{}）。\
                 只按给出的证据判断；证据不足以下判断时选「证据不足」。",
                family.requirement(),
                family.requirement_source()
            ),
            criteria: BTreeMap::from([
                (
                    "violated".to_owned(),
                    Some("证据显示要求没有被满足（违反确实发生）".to_owned()),
                ),
                ("clean".to_owned(), Some("证据显示没有发生违反".to_owned())),
                (
                    "undetermined".to_owned(),
                    Some("给出的证据不足以下这个判断".to_owned()),
                ),
            ]),
        }),
    };
    (id.clone(), BTreeMap::from([(id, spec)]))
}

/// 答案 → 判定。读出规则（与报告的 `protocol.metric_definitions` 同文）：取概率最大的选项；
/// 并列最大时记「证据不足」；选项缺失或概率全为 0 也记「证据不足」。
fn verdict_of_answer(answer: &Answer) -> c_measure::Verdict {
    let Answer::Choice(choice) = answer else {
        return c_measure::Verdict::failed("答案不是 Choice 类型");
    };
    let best = choice
        .probabilities
        .values()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    if !best.is_finite() || best <= 0.0 {
        return c_measure::Verdict::failed("概率分布为空或全为 0");
    }
    let top: Vec<&String> = choice
        .probabilities
        .iter()
        .filter(|(_, mass)| **mass == best)
        .map(|(label, _)| label)
        .collect();
    if top.len() > 1 {
        return c_measure::Verdict::failed("最大概率并列，按口径记「证据不足」");
    }
    match top[0].as_str() {
        "violated" => {
            c_measure::Verdict::violated(format!("{}（violated = {best:.4}）", choice.choice))
        }
        "clean" => c_measure::Verdict::satisfied(format!("{}（clean = {best:.4}）", choice.choice)),
        other => c_measure::Verdict::failed(format!("读出 `{other}`：证据不足")),
    }
}

/// 建一个族的观察通道：契约 → 观测器 → 四路独立检验 → 独立检验门槛下启用。
fn build_channel(
    library: &mut jh_core::ObserverLibrary,
    state: &mut SemanticState,
    family: CFamily,
    contract: jh_core::ObserverContract,
    check: crate::generation::ContractCheck,
) -> Result<Option<ChannelSetup>, Error> {
    let observer = c_measure::Observer::build(family, contract.clone(), ProgramVersion::V1)?;
    let independent =
        c_measure::independent_check(family, ProgramVersion::V1, &observer, unix_ms());
    // 自证式的伪检查：门槛必须拒绝它（研究目标 §7.3 的「不允许仪器自证」）。
    let forged = c_measure::self_certifying(
        &independent.check,
        &c_measure::Verdict::satisfied("观测器自己说没问题（自证）"),
    );
    let enable = c_measure::enable_with_independent_check(
        library,
        state,
        family,
        contract.clone(),
        independent.check.clone(),
        forged,
        unix_ms(),
    )?;
    Ok(Some(ChannelSetup {
        contract,
        observer,
        check,
        independent,
        enable,
    }))
}

/// 跑族 C 的全部用例：环境每个用例只跑一次，四个臂读同一份记录。
fn build_channel_cases(
    families: &[CFamily],
    schedules: &BTreeMap<CFamily, Vec<c_program::Schedule>>,
    setups: &BTreeMap<CFamily, ChannelSetup>,
) -> Vec<ChannelCase> {
    let mut cases = Vec::new();
    for family in families {
        for schedule in &schedules[family] {
            for implementation in Implementation::ALL {
                let run = c_program::run_case(
                    *family,
                    schedule,
                    implementation,
                    ProgramVersion::V1,
                    Attachment::at_boundary(),
                    false,
                );
                let base = case_input_material(*family, schedule);
                let mut observables = base.clone();
                observables.extend(observables_material(&run));
                let channel = setups.get(family).map(|_| {
                    let mut material = base.clone();
                    material.extend(channel_material(&run));
                    material
                });
                cases.push(ChannelCase {
                    family: *family,
                    schedule: schedule.id.clone(),
                    implementation,
                    hidden: c_measure::hidden_verdict(&run),
                    observables,
                    channel,
                });
            }
        }
    }
    cases
}

/// 一个判读臂 × 一个族 × 一次重复：逐用例调判定器，按声明的读出规则折算判定。
#[allow(clippy::too_many_arguments)]
async fn run_channel_cell(
    arm: Arm,
    judge: &dyn Judge,
    family: CFamily,
    cases: &[ChannelCase],
    repeat: usize,
    options: &M4Options,
    calls: &mut CallLog,
) -> Result<(ChannelCell, Vec<ChannelCaseReading>), Error> {
    let run_id = format!(
        "{}-channel-{}-{}-r{repeat}",
        options.run,
        arm.label(),
        family.id()
    );
    let (question, questions) = requirement_question(family);
    let mut counts = ChannelCounts::default();
    let mut by_implementation: BTreeMap<String, ChannelCounts> = BTreeMap::new();
    let mut readings = Vec::new();
    let mut case_errors = Vec::new();

    for case in cases.iter().filter(|case| case.family == family) {
        let material = match arm.structure {
            Structure::Loop => Some(case.observables.clone()),
            Structure::Maintained => case.channel.clone(),
        };
        let Some(material) = material else {
            // 通道没建起来（契约被拒或检验没过）：这一臂在这个族上没有材料，判读记失败态
            // （既不折成「成立」也不算「违反」），用例照常进分母、另按失败计数。
            let verdict = c_measure::Verdict::failed("观察通道未启用：没有可用材料");
            counts.score(&case.hidden, &verdict);
            counts.failures += 1;
            by_implementation
                .entry(case.implementation.id().to_owned())
                .or_default()
                .score(&case.hidden, &verdict);
            readings.push(ChannelCaseReading {
                arm: arm.label(),
                repeat,
                family: family.id().to_owned(),
                schedule: case.schedule.clone(),
                implementation: case.implementation.id().to_owned(),
                hidden: hidden_label(&case.hidden),
                hidden_detail: case.hidden.detail.clone(),
                arm_verdict: "failed".to_owned(),
                arm_detail: verdict.detail.clone(),
                probabilities: BTreeMap::new(),
                material_bytes: 0,
                error: Some("观察通道未启用".to_owned()),
            });
            continue;
        };

        let bytes = serde_json::to_string(&material)?.len() as u64;
        counts.material_bytes += bytes;
        let request = JudgeRequest {
            snapshot: SnapshotId::from(format!(
                "{}-{}-{}-r{repeat}",
                family.id(),
                case.schedule,
                case.implementation.id()
            )),
            state: State::Object(material),
            questions: questions.clone(),
        };
        counts.calls += 1;
        let started = Instant::now();
        let outcome = judge.judge(&request).await;
        let latency = started.elapsed();
        let (verdict, probabilities, error) = match &outcome {
            Ok(response) => {
                let record = JudgmentRecord::from_response(&request, response, latency)?;
                calls.append(&CallEntry::Judgment {
                    run: run_id.clone(),
                    case: format!(
                        "{}-{}-{}",
                        family.id(),
                        case.schedule,
                        case.implementation.id()
                    ),
                    record: Box::new(record),
                })?;
                match response.answers.get(&question) {
                    Some(answer) => {
                        let probabilities = metrics::distribution(answer);
                        (verdict_of_answer(answer), probabilities, None)
                    }
                    None => (
                        c_measure::Verdict::failed("答案里没有这个问题"),
                        BTreeMap::new(),
                        Some("答案里没有这个问题".to_owned()),
                    ),
                }
            }
            Err(failure) => {
                calls.append(&CallEntry::Failure {
                    run: run_id.clone(),
                    case: format!(
                        "{}-{}-{}",
                        family.id(),
                        case.schedule,
                        case.implementation.id()
                    ),
                    source: arm.channel().source(),
                    latency_ms: latency.as_millis() as u64,
                    error: failure.to_string(),
                })?;
                (
                    c_measure::Verdict::failed(format!("调用失败：{failure}")),
                    BTreeMap::new(),
                    Some(failure.to_string()),
                )
            }
        };
        if error.is_some() {
            counts.failures += 1;
            case_errors.push(error.clone().unwrap_or_default());
        }
        counts.score(&case.hidden, &verdict);
        by_implementation
            .entry(case.implementation.id().to_owned())
            .or_default()
            .score(&case.hidden, &verdict);
        readings.push(ChannelCaseReading {
            arm: arm.label(),
            repeat,
            family: family.id().to_owned(),
            schedule: case.schedule.clone(),
            implementation: case.implementation.id().to_owned(),
            hidden: hidden_label(&case.hidden),
            hidden_detail: case.hidden.detail.clone(),
            arm_verdict: verdict_label(&verdict),
            arm_detail: verdict.detail.clone(),
            probabilities,
            material_bytes: bytes,
            error,
        });
    }

    let cases_file = options
        .out_dir
        .join("channel-cases")
        .join(format!("{run_id}.jsonl"));
    write_jsonl(&cases_file, &readings)?;
    let cell = ChannelCell {
        arm: arm.label(),
        structure: arm.structure.label().to_owned(),
        judge: arm.judge.label().to_owned(),
        family: family.id().to_owned(),
        requirement: family.requirement().to_owned(),
        material: match arm.structure {
            Structure::Loop => "observables",
            Structure::Maintained => "channel",
        }
        .to_owned(),
        verification: channel_verification(
            arm.channel().live(),
            counts.cases,
            counts.calls,
            counts.failures,
            case_errors.first().cloned(),
        ),
        repeats: 1,
        cases: counts.cases,
        opportunities: counts.opportunities,
        clean_cases: counts.clean_cases,
        detections: counts.detections,
        false_passes: counts.false_passes,
        inconclusive_on_opportunity: counts.inconclusive_on_opportunity,
        false_alarms: counts.false_alarms,
        correct_clears: counts.correct_clears,
        inconclusive_on_clean: counts.inconclusive_on_clean,
        detection_rate: ratio(counts.detections, counts.opportunities),
        false_pass_rate: ratio(counts.false_passes, counts.opportunities),
        false_alarm_rate: ratio(counts.false_alarms, counts.clean_cases),
        by_implementation: by_implementation
            .into_iter()
            .map(|(implementation, counts)| {
                (
                    implementation,
                    ChannelCountsReport {
                        cases: counts.cases,
                        opportunities: counts.opportunities,
                        clean_cases: counts.clean_cases,
                        detections: counts.detections,
                        false_passes: counts.false_passes,
                        inconclusive_on_opportunity: counts.inconclusive_on_opportunity,
                        false_alarms: counts.false_alarms,
                        correct_clears: counts.correct_clears,
                        inconclusive_on_clean: counts.inconclusive_on_clean,
                    },
                )
            })
            .collect(),
        calls: counts.calls,
        failures: counts.failures,
        material_bytes: counts.material_bytes,
        cases_file: compare::absolute(&cases_file),
    };
    Ok((cell, readings))
}

/// 真值标签。
fn hidden_label(verdict: &c_measure::Verdict) -> String {
    verdict_label(verdict)
}

/// 判定标签（与 h4 的读数口径同名：成立 / 发现违反 / 观测失败）。
fn verdict_label(verdict: &c_measure::Verdict) -> String {
    match &verdict.verdict {
        jh_core::ObserverVerdict::Satisfied => "satisfied".to_owned(),
        jh_core::ObserverVerdict::Violated => "violated".to_owned(),
        jh_core::ObserverVerdict::Failed(_) => "failed".to_owned(),
    }
}

/// 比率：分母为 0 时记 `None`，不记 0。
fn ratio(numerator: u32, denominator: u32) -> Option<f64> {
    (denominator > 0).then(|| f64::from(numerator) / f64::from(denominator))
}

/// 比率的人读形式（读数文本用）。
fn percent(value: Option<f64>) -> String {
    value.map_or_else(|| "无".to_owned(), |value| format!("{:.3}", value))
}

/// JSONL 落盘。
fn write_jsonl<T: Serialize>(path: &Path, rows: &[T]) -> Result<(), Error> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    let mut text = String::new();
    for row in rows {
        text.push_str(&serde_json::to_string(row)?);
        text.push('\n');
    }
    std::fs::write(path, text)?;
    Ok(())
}

/// 从调用记录里取出的一格账（调用、延迟、用量）。
#[derive(Debug)]
struct LogBucket {
    calls: CallStats,
    latencies: Vec<u64>,
    usage_calls: u32,
    input_tokens: u64,
    output_tokens: u64,
    usage_seen: bool,
    failures: u32,
}

impl LogBucket {
    fn new() -> Self {
        Self {
            calls: CallStats {
                attempted: 0,
                succeeded: 0,
                failed: 0,
                errors: BTreeMap::new(),
            },
            latencies: Vec::new(),
            usage_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            usage_seen: false,
            failures: 0,
        }
    }

    fn usage(&self) -> UsageStats {
        UsageStats {
            calls_with_usage: self.usage_calls,
            input_tokens: self.usage_seen.then_some(self.input_tokens),
            output_tokens: self.usage_seen.then_some(self.output_tokens),
        }
    }

    fn latency(&self) -> LatencyStats {
        let mut samples = self.latencies.clone();
        samples.sort_unstable();
        LatencyStats {
            samples: samples.len() as u32,
            min: samples.first().copied(),
            p50: metrics::percentile(&samples, 0.5),
            p95: metrics::percentile(&samples, 0.95),
            max: samples.last().copied(),
        }
    }
}

/// 按运行标识过滤调用记录。
fn log_bucket(entries: &[CallEntry], run_ids: &[String]) -> LogBucket {
    let wanted: BTreeSet<&str> = run_ids.iter().map(String::as_str).collect();
    let mut bucket = LogBucket::new();
    for entry in entries {
        if !wanted.contains(entry.run()) {
            continue;
        }
        match entry {
            CallEntry::Judgment { record, .. } => {
                bucket.calls.attempted += 1;
                bucket.calls.succeeded += 1;
                bucket.latencies.push(record.latency_ms);
                if let Some(usage) = &record.usage {
                    bucket.usage_calls += 1;
                    bucket.usage_seen = true;
                    bucket.input_tokens += usage.input_tokens.unwrap_or(0);
                    bucket.output_tokens += usage.output_tokens.unwrap_or(0);
                }
            }
            CallEntry::Failure {
                latency_ms, error, ..
            } => {
                bucket.calls.attempted += 1;
                bucket.calls.failed += 1;
                bucket.failures += 1;
                bucket.latencies.push(*latency_ms);
                *bucket.calls.errors.entry(error.clone()).or_insert(0) += 1;
            }
            CallEntry::ModelCall {
                latency_ms,
                usage,
                error,
                ..
            } => {
                bucket.calls.attempted += 1;
                bucket.latencies.push(*latency_ms);
                match error {
                    Some(error) => {
                        bucket.calls.failed += 1;
                        bucket.failures += 1;
                        *bucket.calls.errors.entry(error.clone()).or_insert(0) += 1;
                    }
                    None => bucket.calls.succeeded += 1,
                }
                if let Some(usage) = usage {
                    bucket.usage_calls += 1;
                    bucket.usage_seen = true;
                    bucket.input_tokens += usage.input_tokens.unwrap_or(0);
                    bucket.output_tokens += usage.output_tokens.unwrap_or(0);
                }
            }
        }
    }
    bucket
}

/// 把一格的多次重复读数并起来（计数相加、向量相连、均值按判分数加权）。
fn merge_readings(target: &mut LoopReadings, other: &LoopReadings) {
    let weighted =
        |left: Option<f64>, right: Option<f64>, left_n: u32, right_n: u32| match (left, right) {
            (Some(left), Some(right)) => {
                let total = f64::from(left_n + right_n);
                (total > 0.0)
                    .then(|| (left * f64::from(left_n) + right * f64::from(right_n)) / total)
            }
            (Some(value), None) => Some(value),
            (None, Some(value)) => Some(value),
            (None, None) => None,
        };

    target.brier_mean = weighted(
        target.brier_mean,
        other.brier_mean,
        target.judgments,
        other.judgments,
    );
    target.log_loss_mean = weighted(
        target.log_loss_mean,
        other.log_loss_mean,
        target.judgments,
        other.judgments,
    );

    target.tasks += other.tasks;
    target.acceptance_passed += other.acceptance_passed;
    target.opportunities += other.opportunities;
    target.executions += other.executions;
    target.wrong_actions += other.wrong_actions;
    target.stale_basis_commitments += other.stale_basis_commitments;
    target.blocked += other.blocked;
    target.gate_unsatisfied += other.gate_unsatisfied;
    target.abstentions += other.abstentions;
    target.claims += other.claims;
    target.action_cost_units += other.action_cost_units;
    target.observation_actions += other.observation_actions;
    target.observation_cost_units += other.observation_cost_units;
    target.asks += other.asks;
    target.failed_asks += other.failed_asks;
    target.judged_nodes += other.judged_nodes;
    target.judgments += other.judgments;
    target.agreements += other.agreements;
    target.log_loss_undefined += other.log_loss_undefined;
    target.input_bytes += other.input_bytes;
    target.reuse_events += other.reuse_events;
    target.stale_reuse_events += other.stale_reuse_events;
    target.marked_nodes += other.marked_nodes;
    target.marked_actions += other.marked_actions;
    target.missed += other.missed;
    target.extra += other.extra;
    target.unrelated_preserved += other.unrelated_preserved;
    target.required_actions += other.required_actions;
    target.recovered_actions += other.recovered_actions;
    target.unrecovered.extend(other.unrecovered.clone());
    target.recovery_rounds.extend(other.recovery_rounds.clone());
}

/// 合并后重算派生率。
fn finalize_rates(readings: &mut LoopReadings) {
    readings.stale_basis_action_rate = (readings.opportunities > 0)
        .then(|| f64::from(readings.stale_basis_commitments) / f64::from(readings.opportunities));
    let reuse_total = readings.reuse_events + readings.asks;
    readings.cache_reuse_rate =
        (reuse_total > 0).then(|| f64::from(readings.reuse_events) / f64::from(reuse_total));
}

/// 样本标准差（自由度 n−1）。
fn sample_sd(values: &[f64]) -> Option<f64> {
    if values.len() < 2 {
        return None;
    }
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let variance = values
        .iter()
        .map(|value| (value - mean).powi(2))
        .sum::<f64>()
        / (values.len() - 1) as f64;
    Some(variance.sqrt())
}

/// 把一次格的运行折成报告里的 [`LoopCell`]（测试用：真实路径走 [`RawCell::loop_cell`]）。
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn loop_cell(
    purpose: &str,
    key: &CellKey<'_>,
    repeats: usize,
    per_repeat: Vec<RepeatReading>,
    readings: LoopReadings,
    run_ids: &[String],
    traces: Vec<String>,
    evidence_files: Vec<String>,
    entries: &[CallEntry],
    verification: Verification,
) -> LoopCell {
    let bucket = log_bucket(entries, run_ids);
    let variance = Variance {
        samples: per_repeat.len(),
        acceptance_rate_sd: sample_sd(
            &per_repeat
                .iter()
                .filter_map(|repeat| repeat.acceptance_rate)
                .collect::<Vec<f64>>(),
        ),
        calls_sd: sample_sd(
            &per_repeat
                .iter()
                .map(|repeat| f64::from(repeat.calls))
                .collect::<Vec<f64>>(),
        ),
        input_bytes_sd: sample_sd(
            &per_repeat
                .iter()
                .map(|repeat| repeat.input_bytes as f64)
                .collect::<Vec<f64>>(),
        ),
        wrong_actions_sd: sample_sd(
            &per_repeat
                .iter()
                .map(|repeat| f64::from(repeat.wrong_actions))
                .collect::<Vec<f64>>(),
        ),
    };

    LoopCell {
        purpose: purpose.to_owned(),
        arm: key.arm.label(),
        structure: key.arm.structure.label().to_owned(),
        judge: key.arm.judge.label().to_owned(),
        family: key.family.to_owned(),
        evidence: key.evidence.label().to_owned(),
        cadence: key.cadence.label(),
        scale: key.scale,
        loop_arm: key.arm.loop_arm().label().to_owned(),
        verification,
        repeats,
        per_repeat,
        variance,
        readings,
        calls: bucket.calls.clone(),
        latency_ms: bucket.latency(),
        usage: bucket.usage(),
        traces,
        evidence_files,
    }
}

/// 未经构造的判定器：该通道的臂一律标「未验证」，不换后端（`docs/roadmap-1.0.md` §3.2）。
fn unavailable_verification(name: JudgeName, detail: &str) -> Verification {
    Verification {
        status: VerificationStatus::NotVerified,
        detail: format!(
            "判定器 `{}` 没有构造成功（{}）：相关结论保持未验证",
            name.cli_name(),
            detail
        ),
    }
}

/// 一个格的重复读数（从计数折算）。
fn repeat_reading(repeat: usize, readings: &LoopReadings, bucket: &LogBucket) -> RepeatReading {
    RepeatReading {
        repeat,
        tasks: readings.tasks,
        acceptance_passed: readings.acceptance_passed,
        acceptance_rate: ratio(readings.acceptance_passed, readings.tasks),
        executions: readings.executions,
        wrong_actions: readings.wrong_actions,
        stale_basis_commitments: readings.stale_basis_commitments,
        asks: readings.asks,
        input_bytes: readings.input_bytes,
        calls: bucket.calls.attempted,
        input_tokens: bucket.usage().input_tokens,
        output_tokens: bucket.usage().output_tokens,
    }
}

/// 密度扫描的一格：检查密度 × 规模 × 臂。
#[derive(Debug, Clone, Serialize)]
pub struct DensityCell {
    /// 检查节奏（密度轴）。
    pub cadence: String,
    /// 规模轴（每族任务数）。
    pub scale: usize,
    /// 臂标识。
    pub arm: String,
    /// 参与汇总的族。
    pub families: Vec<String>,
    /// 重复次数。
    pub repeats: usize,
    /// 读数（按 §10.1 主要指标与 §10.2 诊断指标采集）。
    pub readings: LoopReadings,
    /// 判断调用统计。
    pub calls: CallStats,
    /// 延迟分位数。
    pub latency_ms: LatencyStats,
    /// token 用量。
    pub usage: UsageStats,
}

/// 主对照的一条比较（同一族内只切一个维度）。
#[derive(Debug, Clone, Serialize)]
pub struct Contrast {
    /// 族。
    pub family: String,
    /// 切的是哪个维度：`structure` / `judge`。
    pub dimension: String,
    /// 被固定住的那一维取值。
    pub fixed: String,
    /// 左臂。
    pub left: String,
    /// 右臂。
    pub right: String,
    /// 左臂验收通过率。
    pub left_acceptance: Option<f64>,
    /// 右臂验收通过率。
    pub right_acceptance: Option<f64>,
    /// 左臂不该发出的动作数。
    pub left_wrong_actions: u32,
    /// 右臂不该发出的动作数。
    pub right_wrong_actions: u32,
    /// 左臂旧依据承诺数。
    pub left_stale_basis: u32,
    /// 右臂旧依据承诺数。
    pub right_stale_basis: u32,
    /// 左臂判断调用数。
    pub left_calls: u32,
    /// 右臂判断调用数。
    pub right_calls: u32,
    /// 左臂重算输入量。
    pub left_input_bytes: u64,
    /// 右臂重算输入量。
    pub right_input_bytes: u64,
    /// 读取纪律说明。
    pub note: String,
}

/// 协议声明：跑之前写死的口径。
#[derive(Debug, Clone, Serialize)]
pub struct ProtocolDeclaration {
    /// 臂的声明。
    pub arms: Vec<ArmDeclaration>,
    /// 两个维度。
    pub dimensions: Vec<String>,
    /// 证据口径（诊断对照）。
    pub evidence_modes: Vec<String>,
    /// 重复协议。
    pub repetition: String,
    /// 原生基线协议。
    pub native: String,
    /// 两条残余消融的协议。
    pub ablations: Vec<String>,
    /// 密度扫描协议。
    pub density: String,
    /// 读数口径原文。
    pub metric_definitions: BTreeMap<String, String>,
}

/// 一条臂的声明。
#[derive(Debug, Clone, Serialize)]
pub struct ArmDeclaration {
    /// 臂标识。
    pub arm: String,
    /// 控制结构。
    pub structure: String,
    /// 判断通道。
    pub judge: String,
    /// 接到匹配循环上的 H1 / H2 开关。
    pub loop_arm: String,
    /// 说明。
    pub description: String,
}

/// 合并任务集摘要。
#[derive(Debug, Clone, Serialize)]
pub struct MergedTaskSet {
    /// 逐族摘要。
    pub families: Vec<FamilyEntry>,
    /// 合并后的任务总数（族 A / B 的任务 + 族 C 的用例）。
    pub merged_tasks: usize,
    /// 口径说明。
    pub note: String,
}

/// 一族的摘要。
#[derive(Debug, Clone, Serialize)]
pub struct FamilyEntry {
    /// 族。
    pub family: String,
    /// 任务来源。
    pub source: String,
    /// 本次参与的任务数。
    pub tasks: usize,
    /// 轮数或调度数说明。
    pub shape: String,
}

/// 跨任务复用消融的一侧读数。
#[derive(Debug, Clone, Serialize)]
pub struct ReuseSide {
    /// 一侧的标识（`reuse` / `rebuild-per-case`）。
    pub side: String,
    /// 判读臂（固定为依赖维护 + Jev）。
    pub arm: String,
    /// 用例数。
    pub cases: u32,
    /// 生成调用数（本侧的账）。
    pub generation_calls: u32,
    /// 生成失败 / 被拒数。
    pub generation_failures: u32,
    /// 生成输入 token。
    pub input_tokens: Option<u64>,
    /// 生成输出 token。
    pub output_tokens: Option<u64>,
    /// 通道可用的用例数：**既要有这一族的通道、也要有这一用例的材料**（口径见文档 §5.6）。
    pub channel_available: u32,
    /// 其中通道沿用自更早用例的用例数（复用侧的形态；重建侧修好口径后应为 0）。
    pub channel_from_earlier_case: u32,
    /// 检出数。
    pub detections: u32,
    /// 误放行数。
    pub false_passes: u32,
    /// 弃权数。
    pub inconclusive: u32,
    /// 逐用例读数文件。
    pub cases_file: String,
}

/// 复用曲线的采样点。
#[derive(Debug, Clone, Serialize)]
pub struct ReusePoint {
    /// 一侧的标识。
    pub side: String,
    /// 已覆盖用例数。
    pub cases_covered: u32,
    /// 累计生成调用数。
    pub cumulative_generation_calls: u32,
    /// 累计检出数。
    pub detections: u32,
    /// 累计误放行数。
    pub false_passes: u32,
}

/// 跨任务复用消融段。
#[derive(Debug, Clone, Serialize)]
pub struct ReuseSection {
    /// 跑没跑（没跑时 `enabled = false`）。
    pub enabled: bool,
    /// 协议原文。
    pub protocol: String,
    /// 复用一侧。
    pub reuse: Option<ReuseSide>,
    /// 逐用例重建一侧。
    pub rebuild: Option<ReuseSide>,
    /// 累计曲线。
    pub curve: Vec<ReusePoint>,
    /// 口径与限制。
    pub note: String,
}

/// H5 的判定段（照 h4 的假说段写法：判据跑之前写死，跑后只读数）。
#[derive(Debug, Clone, Serialize)]
pub struct HypothesisSection {
    /// 命题。
    pub statement: String,
    /// 判据（事先声明）。
    pub criteria: Vec<String>,
    /// 报告方式说明。
    pub reporting: String,
    /// 读数。
    pub readings: Vec<String>,
    /// 依赖触发在质量上不劣。
    pub improves_quality: bool,
    /// 依赖触发在成本上更省。
    pub improves_cost: bool,
    /// 两条判据都成立。
    pub supported: bool,
    /// 结论（§10.4 的四档之内）。
    pub conclusion: String,
    /// 随批登记的注意项。
    pub caveats: Vec<String>,
}

/// 成本段。
#[derive(Debug, Clone, Serialize)]
pub struct CostSection {
    /// 逐用途的成本。
    pub by_purpose: Vec<PurposeCost>,
    /// 合计。
    pub total: PurposeCost,
    /// 非调用成本（次数）。
    pub runs: BTreeMap<String, u32>,
    /// 口径说明。
    pub note: String,
}

/// 一个用途的成本。
#[derive(Debug, Clone, Serialize)]
pub struct PurposeCost {
    /// 用途。
    pub purpose: String,
    /// 调用统计。
    pub calls: CallStats,
    /// 延迟分位数。
    pub latency_ms: LatencyStats,
    /// token 用量。
    pub usage: UsageStats,
}

/// 原始数据位置。
#[derive(Debug, Clone, Serialize)]
pub struct RawPaths {
    /// 产物目录。
    pub out_dir: String,
    /// 主对照与诊断对照的轨迹。
    pub traces: String,
    /// 主对照与诊断对照的证据日志。
    pub evidence: String,
    /// 原生基线轨迹。
    pub native: String,
    /// 族 C 逐用例读数。
    pub channel_cases: String,
    /// 族 C 的契约产物。
    pub contracts: String,
    /// 复用消融的逐用例读数。
    pub reuse_cases: String,
    /// 调用记录。
    pub calls_log: String,
}

/// M4 的报告。
#[derive(Debug, Clone, Serialize)]
pub struct M4Report {
    /// 运行标识。
    pub run: String,
    /// 生成时刻（Unix 毫秒）。
    pub generated_at_unix_ms: u64,
    /// 复现命令。
    pub command: String,
    /// 协议声明。
    pub protocol: ProtocolDeclaration,
    /// 运行环境（不写 key 值）。
    pub environment: compare::ReportEnvironment,
    /// 任务集摘要。
    pub task_set: MergedTaskSet,
    /// 主对照：臂 × 族。
    pub main: Vec<LoopCell>,
    /// 诊断对照：臂 × 族（相同新增证据）。
    pub diagnostic: Vec<LoopCell>,
    /// 原生基线臂（族 A）。
    pub native: Vec<LoopCell>,
    /// 密度扫描：检查密度 × 规模。
    pub density: Vec<DensityCell>,
    /// 族 C 判读臂。
    pub channel: Vec<ChannelCell>,
    /// 族 C 观察通道的建立过程（逐族；契约被拒也如实记）。
    pub channel_setup: Vec<ChannelSetupReport>,
    /// 跨任务复用消融。
    pub reuse: ReuseSection,
    /// H5 判定（固定频率检查）。
    pub h5: HypothesisSection,
    /// 主对照内只切一个维度的比较。
    pub contrasts: Vec<Contrast>,
    /// 成本。
    pub cost: CostSection,
    /// 原始数据位置。
    pub raw: RawPaths,
    /// 注意事项。
    pub notes: Vec<String>,
}

/// 族 C 观察通道的建立过程（逐族留档：契约被拒也如实记）。
#[derive(Debug, Clone, Serialize)]
pub struct ChannelSetupReport {
    /// 族。
    pub family: String,
    /// 契约来源：`generated` / `file`。
    pub source: String,
    /// 契约是否被接受（协议核对通过）。
    pub accepted: bool,
    /// 被拒原因。
    pub rejection: Option<String>,
    /// 契约核对是否通过。
    pub contract_check_passed: bool,
    /// 契约选中的测量位置。
    pub hook_points: Vec<String>,
    /// 四路独立检验是否通过。
    pub independent_passed: bool,
    /// 自证式的伪隐藏检查是否被门槛拒绝。
    pub self_certifying_rejected: bool,
    /// 拒绝自证时的错误原文。
    pub self_certifying_error: Option<String>,
    /// 门槛拒绝后库里的记录条数。
    pub checks_on_record_after_rejection: usize,
    /// 启用后的版本号。
    pub enabled_version: Option<u32>,
    /// 是否启用成功。
    pub enabled: bool,
    /// 启用引发的失效传播摘要。
    pub invalidation: Option<String>,
    /// 生成调用的读数（复用文件时为 `None`）。
    pub call: Option<MeasureCall>,
}

/// 契约来源：真调生成，或复用已落盘的产物。
enum Contracts {
    Generate(c_measure::ContractGenerator),
    File(Vec<c_measure::GeneratedContract>),
}

impl Contracts {
    /// 取一族的契约（生成时顺带落调用记录）。
    async fn of(
        &self,
        family: CFamily,
        run: &str,
        calls: &mut CallLog,
    ) -> Result<Option<c_measure::GeneratedContract>, Error> {
        match self {
            Self::Generate(generator) => {
                let entry = generator.generate_one(family, ProgramVersion::V1).await?;
                log_call(calls, run, family, &entry.call)?;
                Ok(Some(entry))
            }
            Self::File(entries) => Ok(entries.iter().find(|entry| entry.family == family).cloned()),
        }
    }
}

/// 生成调用落进调用记录（与 h4 同一条账）。
fn log_call(
    calls: &mut CallLog,
    run: &str,
    family: CFamily,
    call: &MeasureCall,
) -> Result<(), Error> {
    calls.append(&CallEntry::ModelCall {
        run: run.to_owned(),
        purpose: "contract".to_owned(),
        case: family.id().to_owned(),
        source: jh_core::JudgeSource::Llm,
        version: call.model.clone(),
        request_id: call.request_id.clone(),
        usage: call.usage.clone(),
        input_bytes: call.input_bytes,
        output_bytes: call.output_bytes,
        latency_ms: call.latency_ms,
        error: call.error.clone(),
    })
}

/// 建立通道：一族一次（复用契约或真调生成）。
///
/// 返回「报告 + 可用装置」：契约被拒或检验没过时装置为 `None`，报告里如实记下原因。
#[allow(clippy::too_many_arguments)]
async fn setup_channel(
    contracts: &Contracts,
    library: &mut jh_core::ObserverLibrary,
    state: &mut SemanticState,
    family: CFamily,
    run: &str,
    calls: &mut CallLog,
) -> Result<(Option<ChannelSetupReport>, Option<ChannelSetup>), Error> {
    let Some(entry) = contracts.of(family, run, calls).await? else {
        return Ok((None, None));
    };
    let mut report = ChannelSetupReport {
        family: family.id().to_owned(),
        source: match contracts {
            Contracts::Generate(_) => "generated".to_owned(),
            Contracts::File(_) => "file".to_owned(),
        },
        accepted: false,
        rejection: entry.rejection.clone(),
        contract_check_passed: false,
        hook_points: Vec::new(),
        independent_passed: false,
        self_certifying_rejected: false,
        self_certifying_error: None,
        checks_on_record_after_rejection: 0,
        enabled_version: None,
        enabled: false,
        invalidation: None,
        call: Some(entry.call.clone()),
    };
    let (Some(contract), Some(check)) = (entry.contract.clone(), entry.check.clone()) else {
        return Ok((Some(report), None));
    };
    report.contract_check_passed = check.passed;
    report.hook_points = vec![contract.measurement.site.clone()];
    if !check.passed {
        report.rejection = Some("契约没有通过协议核对".to_owned());
        return Ok((Some(report), None));
    }
    report.accepted = true;

    let setup = build_channel(library, state, family, contract, check)?;
    let Some(setup) = setup else {
        return Ok((Some(report), None));
    };
    report.hook_points = setup
        .observer
        .points()
        .iter()
        .map(|point| point.id().to_owned())
        .collect();
    report.independent_passed = setup.independent.check.passed();
    report.self_certifying_rejected = setup.enable.self_certifying_rejected;
    report.self_certifying_error = setup.enable.self_certifying_error.clone();
    report.checks_on_record_after_rejection = setup.enable.checks_on_record_after_rejection;
    report.enabled_version = setup.enable.version;
    report.enabled = setup.enable.version.is_some();
    report.invalidation = setup.enable.invalidation.clone();
    if !report.enabled {
        return Ok((Some(report), None));
    }
    Ok((Some(report), Some(setup)))
}

/// 跑一个判读臂的全部族。
#[allow(clippy::too_many_arguments)]
async fn channel_cells_for_arm(
    arm: Arm,
    judge: &dyn Judge,
    families: &[CFamily],
    cases: &[ChannelCase],
    repeats: usize,
    options: &M4Options,
    calls: &mut CallLog,
) -> Result<Vec<ChannelCell>, Error> {
    let mut cells: Vec<ChannelCell> = Vec::new();
    for family in families {
        for repeat in 1..=repeats {
            let (mut cell, _) =
                run_channel_cell(arm, judge, *family, cases, repeat, options, calls).await?;
            cell.repeats = repeats;
            // 同一族多次重复：把重复的读数并起来（分母相加），逐用例原始读数留在 JSONL 里。
            if let Some(previous) = cells.last()
                && previous.family == cell.family
                && previous.arm == cell.arm
            {
                let merged = merge_channel_cells(previous, &cell);
                cells.pop();
                cells.push(merged);
                continue;
            }
            cells.push(cell);
        }
    }
    Ok(cells)
}

/// 合并同一格的两次重复（计数相加、比率重算）。
fn merge_channel_cells(left: &ChannelCell, right: &ChannelCell) -> ChannelCell {
    let mut merged = left.clone();
    merged.repeats = left.repeats.max(right.repeats);
    merged.cases += right.cases;
    merged.opportunities += right.opportunities;
    merged.clean_cases += right.clean_cases;
    merged.detections += right.detections;
    merged.false_passes += right.false_passes;
    merged.inconclusive_on_opportunity += right.inconclusive_on_opportunity;
    merged.false_alarms += right.false_alarms;
    merged.correct_clears += right.correct_clears;
    merged.inconclusive_on_clean += right.inconclusive_on_clean;
    merged.material_bytes += right.material_bytes;
    merged.calls += right.calls;
    merged.failures += right.failures;
    merged.detection_rate = ratio(merged.detections, merged.opportunities);
    merged.false_pass_rate = ratio(merged.false_passes, merged.opportunities);
    merged.false_alarm_rate = ratio(merged.false_alarms, merged.clean_cases);
    let mut by_implementation = left.by_implementation.clone();
    for (implementation, counts) in &right.by_implementation {
        let entry = by_implementation
            .entry(implementation.clone())
            .or_insert_with(|| ChannelCountsReport {
                cases: 0,
                opportunities: 0,
                clean_cases: 0,
                detections: 0,
                false_passes: 0,
                inconclusive_on_opportunity: 0,
                false_alarms: 0,
                correct_clears: 0,
                inconclusive_on_clean: 0,
            });
        entry.cases += counts.cases;
        entry.opportunities += counts.opportunities;
        entry.clean_cases += counts.clean_cases;
        entry.detections += counts.detections;
        entry.false_passes += counts.false_passes;
        entry.inconclusive_on_opportunity += counts.inconclusive_on_opportunity;
        entry.false_alarms += counts.false_alarms;
        entry.correct_clears += counts.correct_clears;
        entry.inconclusive_on_clean += counts.inconclusive_on_clean;
    }
    merged.by_implementation = by_implementation;
    // 验证状态按合并后的计数重算：读数用的是结构化字段，但报告读者会读这串文案。
    merged.verification = channel_verification(
        left.verification.status != VerificationStatus::Simulated,
        merged.cases,
        merged.calls,
        merged.failures,
        None,
    );
    merged
}

/// 复用消融的一侧读数（落 JSONL 的逐用例结构）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReuseCaseReading {
    /// 一侧。
    pub side: String,
    /// 用例序号（从 1 起，曲线按它累计）。
    pub index: u32,
    /// 族。
    pub family: String,
    /// 调度。
    pub schedule: String,
    /// 实现路。
    pub implementation: String,
    /// 真值。
    pub hidden: String,
    /// 判读结果。
    pub arm_verdict: String,
    /// 判读依据。
    pub arm_detail: String,
    /// 概率原值。
    pub probabilities: BTreeMap<String, f64>,
    /// 本用例是否新建了通道。
    pub channel_built_this_case: bool,
    /// 本用例上通道是否可用：**既要有这一族的通道、也要有这一用例的材料**（口径见 §5.6）。
    pub channel_available: bool,
    /// 本用例的通道是否沿用自更早的用例（重建侧修好口径后恒为 false）。
    pub channel_from_earlier_case: bool,
    /// 本用例的生成调用数。
    pub generation_calls: u32,
    /// 契约被拒原因。
    pub rejection: Option<String>,
    /// 调用失败原文。
    pub error: Option<String>,
}

/// 跑复用消融的一侧。
#[allow(clippy::too_many_arguments)]
async fn run_reuse_side(
    side: ChannelReuse,
    arm: Arm,
    judge: &dyn Judge,
    families: &[CFamily],
    cases: &[ChannelCase],
    contracts: &Contracts,
    options: &M4Options,
    calls: &mut CallLog,
) -> Result<(ReuseSide, Vec<ReuseCaseReading>, Vec<ReusePoint>), Error> {
    let run_id = format!("{}-reuse-{}-{}", options.run, arm.label(), side.label());
    let mut library = jh_core::ObserverLibrary::default();
    let mut state = SemanticState::in_memory();
    let mut setups: BTreeMap<CFamily, ChannelSetup> = BTreeMap::new();
    let (question, questions) = requirement_question(families[0]);
    let mut readings = Vec::new();
    let mut curve = Vec::new();
    let mut counts = ChannelCounts::default();
    let mut available = 0;
    let mut from_earlier_cases = 0;
    let mut generation_calls = 0;
    let mut generation_failures = 0;
    let mut index = 0;

    for family in families {
        let mut built_for_family = false;
        for case in cases.iter().filter(|case| case.family == *family) {
            index += 1;
            let mut built_this_case = false;
            let mut generation_this_case = 0;
            let mut rejection = None;
            let needs_build = match side {
                ChannelReuse::Reuse => !built_for_family,
                ChannelReuse::Rebuild => true,
            };
            if needs_build {
                // 重建侧不许沿用旧通道：先清掉这一族此前留下的装置，重建失败就没有通道
                // （「一次被拒后仍用先前启用的通道」本身就是跨用例复用，与这一侧的定义相冲）。
                if matches!(side, ChannelReuse::Rebuild) {
                    setups.remove(family);
                }
                let (report, setup) =
                    setup_channel(contracts, &mut library, &mut state, *family, &run_id, calls)
                        .await?;
                built_this_case = true;
                built_for_family = true;
                generation_this_case = 1;
                generation_calls += 1;
                match setup {
                    Some(setup) => {
                        setups.insert(*family, setup);
                    }
                    None => {
                        rejection = report.as_ref().and_then(|report| {
                            report.rejection.clone().or_else(|| {
                                report
                                    .self_certifying_error
                                    .clone()
                                    .or_else(|| Some("通道没有启用".to_owned()))
                            })
                        });
                        generation_failures += 1;
                    }
                }
            }
            // 「通道可用」= 本用例真的能拿材料判读：既要有这一族的通道，也要有这一用例的材料
            // （材料是 setup 阶段预计算的 `case.channel`，通道被拒的那一族整段没有材料）。
            let material = case.channel.clone();
            let channel_available = setups.contains_key(family) && material.is_some();
            if channel_available {
                available += 1;
                if !built_this_case {
                    from_earlier_cases += 1;
                }
            }
            let (verdict, probabilities, error) = match (setups.contains_key(family), material) {
                (false, _) => (
                    c_measure::Verdict::failed("本用例上没有可用的观察通道"),
                    BTreeMap::new(),
                    None,
                ),
                (true, None) => (
                    c_measure::Verdict::failed("没有可读的通道材料"),
                    BTreeMap::new(),
                    None,
                ),
                (true, Some(material)) => {
                    let request = JudgeRequest {
                        snapshot: SnapshotId::from(format!(
                            "{}-{}-{}-reuse",
                            family.id(),
                            case.schedule,
                            case.implementation.id()
                        )),
                        state: State::Object(material),
                        questions: questions.clone(),
                    };
                    let started = Instant::now();
                    let outcome = judge.judge(&request).await;
                    let latency = started.elapsed();
                    match &outcome {
                        Ok(response) => {
                            let record =
                                JudgmentRecord::from_response(&request, response, latency)?;
                            calls.append(&CallEntry::Judgment {
                                run: run_id.clone(),
                                case: format!(
                                    "{}-{}-{}",
                                    family.id(),
                                    case.schedule,
                                    case.implementation.id()
                                ),
                                record: Box::new(record),
                            })?;
                            match response.answers.get(&question) {
                                Some(answer) => (
                                    verdict_of_answer(answer),
                                    metrics::distribution(answer),
                                    None,
                                ),
                                None => (
                                    c_measure::Verdict::failed("答案里没有这个问题"),
                                    BTreeMap::new(),
                                    Some("答案里没有这个问题".to_owned()),
                                ),
                            }
                        }
                        Err(failure) => {
                            calls.append(&CallEntry::Failure {
                                run: run_id.clone(),
                                case: format!(
                                    "{}-{}-{}",
                                    family.id(),
                                    case.schedule,
                                    case.implementation.id()
                                ),
                                source: arm.channel().source(),
                                latency_ms: latency.as_millis() as u64,
                                error: failure.to_string(),
                            })?;
                            (
                                c_measure::Verdict::failed(format!("调用失败：{failure}")),
                                BTreeMap::new(),
                                Some(failure.to_string()),
                            )
                        }
                    }
                }
            };
            counts.calls += 1;
            if error.is_some() || !channel_available {
                counts.failures += 1;
            }
            counts.score(&case.hidden, &verdict);
            readings.push(ReuseCaseReading {
                side: side.label().to_owned(),
                index,
                family: family.id().to_owned(),
                schedule: case.schedule.clone(),
                implementation: case.implementation.id().to_owned(),
                hidden: verdict_label(&case.hidden),
                arm_verdict: verdict_label(&verdict),
                arm_detail: verdict.detail.clone(),
                probabilities,
                channel_built_this_case: built_this_case,
                channel_available,
                channel_from_earlier_case: channel_available && !built_this_case,
                generation_calls: generation_this_case,
                rejection,
                error,
            });
            curve.push(ReusePoint {
                side: side.label().to_owned(),
                cases_covered: index,
                cumulative_generation_calls: generation_calls,
                detections: counts.detections,
                false_passes: counts.false_passes,
            });
        }
    }

    let cases_file = options
        .out_dir
        .join("reuse")
        .join(format!("{run_id}.jsonl"));
    write_jsonl(&cases_file, &readings)?;
    let bucket = log_bucket(
        &read_entries(&options.calls_path)?,
        std::slice::from_ref(&run_id),
    );
    let side_report = ReuseSide {
        side: side.label().to_owned(),
        arm: arm.label(),
        cases: counts.cases,
        generation_calls,
        generation_failures,
        input_tokens: bucket.usage().input_tokens,
        output_tokens: bucket.usage().output_tokens,
        channel_available: available,
        channel_from_earlier_case: from_earlier_cases,
        detections: counts.detections,
        false_passes: counts.false_passes,
        inconclusive: counts.inconclusive_on_opportunity + counts.inconclusive_on_clean,
        cases_file: compare::absolute(&cases_file),
    };
    Ok((side_report, readings, curve))
}

/// 一族（A / B）的任务与视图：同一份问题集与读取范围，主对照与诊断对照共用。
struct FamilyTasks<'a> {
    label: &'a str,
    source: String,
    shape: String,
    view: &'a dyn QuestionSet,
    scope: &'a dyn ScopeOf,
    tasks: Vec<&'a FamilyTask>,
    rounds: usize,
}

/// 一个格的标识（自有字符串，便于在报告组装阶段使用）。
#[derive(Debug, Clone)]
struct OwnedCellKey {
    arm: Arm,
    evidence: Evidence,
    cadence: Cadence,
    family: String,
    scale: Option<usize>,
}

/// 一次重复的运行结果。
struct RawRepeat {
    run_id: String,
    readings: LoopReadings,
}

/// 一个格的原始运行结果（报告组装前）。
struct RawCell {
    purpose: String,
    /// 报告里的四个标签（臂 / 结构 / 通道 / 循环开关）；`None` 时按 [`Arm`] 推。
    label: Option<CellLabel>,
    key: OwnedCellKey,
    repeats: Vec<RawRepeat>,
    traces: Vec<String>,
    evidence_files: Vec<String>,
    verification: Verification,
}

/// 报告里的标签覆盖：原生基线臂不是 2×2 里的任何一格，标签要如实写开。
#[derive(Debug, Clone)]
struct CellLabel {
    arm: String,
    structure: String,
    judge: String,
    loop_arm: String,
}

impl CellLabel {
    /// 原生基线臂的标签。
    fn native() -> Self {
        Self {
            arm: "native-llm".to_owned(),
            structure: "native".to_owned(),
            judge: "llm".to_owned(),
            loop_arm: "n/a（单循环，不用依赖维护）".to_owned(),
        }
    }
}

impl RawCell {
    fn loop_cell(&self, entries: &[CallEntry]) -> LoopCell {
        let mut aggregate = LoopReadings::default();
        let mut per_repeat = Vec::new();
        for (index, repeat) in self.repeats.iter().enumerate() {
            merge_readings(&mut aggregate, &repeat.readings);
            let bucket = log_bucket(entries, std::slice::from_ref(&repeat.run_id));
            per_repeat.push(repeat_reading(index + 1, &repeat.readings, &bucket));
        }
        finalize_rates(&mut aggregate);
        let run_ids: Vec<String> = self
            .repeats
            .iter()
            .map(|repeat| repeat.run_id.clone())
            .collect();
        let bucket = log_bucket(entries, &run_ids);
        let variance = Variance {
            samples: self.repeats.len(),
            acceptance_rate_sd: sample_sd(
                &per_repeat
                    .iter()
                    .filter_map(|repeat| repeat.acceptance_rate)
                    .collect::<Vec<f64>>(),
            ),
            calls_sd: sample_sd(
                &per_repeat
                    .iter()
                    .map(|repeat| f64::from(repeat.calls))
                    .collect::<Vec<f64>>(),
            ),
            input_bytes_sd: sample_sd(
                &per_repeat
                    .iter()
                    .map(|repeat| repeat.input_bytes as f64)
                    .collect::<Vec<f64>>(),
            ),
            wrong_actions_sd: sample_sd(
                &per_repeat
                    .iter()
                    .map(|repeat| f64::from(repeat.wrong_actions))
                    .collect::<Vec<f64>>(),
            ),
        };
        let mut verification = self.verification.clone();
        if verification.status == VerificationStatus::Verified {
            verification.detail = format!(
                "真实通道调用 {} 次，失败 {} 次",
                bucket.calls.attempted, bucket.calls.failed
            );
        }
        let label = self.label.clone().unwrap_or_else(|| CellLabel {
            arm: self.key.arm.label(),
            structure: self.key.arm.structure.label().to_owned(),
            judge: self.key.arm.judge.label().to_owned(),
            loop_arm: self.key.arm.loop_arm().label().to_owned(),
        });
        LoopCell {
            purpose: self.purpose.clone(),
            arm: label.arm,
            structure: label.structure,
            judge: label.judge,
            family: self.key.family.clone(),
            evidence: self.key.evidence.label().to_owned(),
            cadence: self.key.cadence.label(),
            scale: self.key.scale,
            loop_arm: label.loop_arm,
            verification,
            repeats: self.repeats.len(),
            per_repeat,
            variance,
            readings: aggregate,
            calls: bucket.calls.clone(),
            latency_ms: bucket.latency(),
            usage: bucket.usage(),
            traces: self.traces.clone(),
            evidence_files: self.evidence_files.clone(),
        }
    }
}

/// 判读臂的验证状态：重复合并之后要按**合并计数**重算，否则读者会看到半样本的数字
/// （例如结构化字段 failures = 16、文案却写「失败 8 次」）。
fn channel_verification(
    live: bool,
    cases: u32,
    calls: u32,
    failures: u32,
    reason: Option<String>,
) -> Verification {
    if cases > 0 && failures == cases {
        Verification {
            status: VerificationStatus::NotVerified,
            detail: match reason {
                Some(reason) => format!("本格全部用例判定失败：{reason}"),
                None => format!("本格全部用例判定失败（{failures} 次）"),
            },
        }
    } else if live {
        Verification {
            status: VerificationStatus::Verified,
            detail: format!("真实通道调用 {calls} 次，失败 {failures} 次"),
        }
    } else {
        Verification {
            status: VerificationStatus::Simulated,
            detail: "本地判定器".to_owned(),
        }
    }
}

/// 判定器的验证状态（真实通道 vs 本地判定器）。
///
/// 真实通道的调用次数在报告组装阶段从调用记录回填（[`RawCell::loop_cell`]），因为这一刻还没跑。
fn verification_for(name: JudgeName) -> Verification {
    if name.live() {
        Verification {
            status: VerificationStatus::Verified,
            detail: "真实通道（调用次数见调用记录）".to_owned(),
        }
    } else {
        Verification {
            status: VerificationStatus::Simulated,
            detail: format!("本地判定器 `{}`：读数只作对照与上限", name.cli_name()),
        }
    }
}

/// 跑一次 M4（`jev m4`）。
pub async fn run(options: &M4Options) -> Result<M4Outcome, Error> {
    let repeats = options.repeats.max(1);
    let mut calls = CallLog::open(options.calls_path.clone())?;
    // 产物目录先建出来：证据日志与轨迹都按路径直接落盘，缺目录就显式失败。
    for dir in ["traces", "evidence", "native", "channel-cases", "reuse"] {
        std::fs::create_dir_all(options.out_dir.join(dir))?;
    }
    let family_a = FamilyA::load()?;
    let family_b = FamilyB::load()?;
    let b_view = family_b.question_set();
    let limit = options.max_tasks.unwrap_or(usize::MAX);
    let a_tasks: Vec<&FamilyTask> = family_a.tasks.iter().take(limit).collect();
    let b_tasks: Vec<&FamilyTask> = family_b
        .variants
        .iter()
        .map(|entry| &entry.task)
        .take(limit)
        .collect();

    let families = vec![
        FamilyTasks {
            label: "A",
            source: family_a.source.clone(),
            shape: format!("{} 个任务 × 3 轮", a_tasks.len()),
            view: &family_a,
            scope: &family_a,
            rounds: 3,
            tasks: a_tasks,
        },
        FamilyTasks {
            label: "B",
            source: family_b.source.clone(),
            shape: format!("{} 个未见变体 × 3 轮", b_tasks.len()),
            view: &b_view,
            scope: &family_b,
            rounds: 3,
            tasks: b_tasks,
        },
    ];

    let arms = if options.arms.is_empty() {
        Arm::ALL.to_vec()
    } else {
        options.arms.clone()
    };

    // 判定器：每个判断通道构造一次；构造失败不换后端，该通道的臂一律标「未验证」。
    type BuiltJudge = (JudgeName, Result<Box<dyn Judge>, String>);
    let mut built: BTreeMap<&'static str, BuiltJudge> = BTreeMap::new();
    for side in [JudgeSide::Llm, JudgeSide::Jev] {
        let name = options.channel_override.unwrap_or(side.name());
        let judge =
            compare::build_judge(name, &options.calls_path).map_err(|error| error.to_string());
        built.insert(side.label(), (name, judge));
    }

    let mut raws: Vec<RawCell> = Vec::new();
    let mut judge_failures = 0;

    // 1. 主对照与诊断对照：臂 × 族 × 重复。
    for (purpose, evidence) in [("main", Evidence::Own), ("diagnostic", Evidence::Equalized)] {
        for arm in &arms {
            for family in &families {
                let (name, judge) = &built[arm.judge.label()];
                let mut raw = RawCell {
                    purpose: purpose.to_owned(),
                    label: None,
                    key: OwnedCellKey {
                        arm: *arm,
                        evidence,
                        cadence: Cadence::Triggered,
                        family: family.label.to_owned(),
                        scale: None,
                    },
                    repeats: Vec::new(),
                    traces: Vec::new(),
                    evidence_files: Vec::new(),
                    verification: match judge {
                        Ok(_) => verification_for(*name),
                        Err(detail) => unavailable_verification(*name, detail),
                    },
                };
                if let Ok(judge) = judge {
                    for repeat in 1..=repeats {
                        let key = CellKey {
                            arm: *arm,
                            evidence,
                            cadence: Cadence::Triggered,
                            purpose,
                            family: family.label,
                            scale: None,
                            repeat,
                        };
                        let cell = run_cell(
                            family.view,
                            family.scope,
                            &family.tasks,
                            judge.as_ref(),
                            &key,
                            options,
                            &mut calls,
                        )
                        .await?;
                        judge_failures += cell.judge_failures;
                        raw.repeats.push(RawRepeat {
                            run_id: cell.run_ids[0].clone(),
                            readings: cell.readings,
                        });
                        raw.traces.extend(cell.traces);
                        raw.evidence_files.extend(cell.evidence_files);
                    }
                }
                raws.push(raw);
            }
        }
    }

    // 2. 原生基线臂：族 A 的全部任务（同一环境、同一计价、同一条独立验收）。
    let mut native_raw = None;
    if options.native {
        match llm::from_env_client() {
            Ok(client) => {
                let mut raw = RawCell {
                    purpose: "native".to_owned(),
                    label: Some(CellLabel::native()),
                    key: OwnedCellKey {
                        arm: Arm {
                            structure: Structure::Loop,
                            judge: JudgeSide::Llm,
                        },
                        evidence: Evidence::Own,
                        cadence: Cadence::Triggered,
                        family: "A".to_owned(),
                        scale: None,
                    },
                    repeats: Vec::new(),
                    traces: Vec::new(),
                    evidence_files: Vec::new(),
                    verification: Verification {
                        status: VerificationStatus::Verified,
                        detail: "生成模型直接协调动作（单循环，无类型化问题、无依赖维护）"
                            .to_owned(),
                    },
                };
                for repeat in 1..=repeats {
                    let cell = run_native_cell(
                        &families[0].tasks,
                        families[0].label,
                        &client,
                        repeat,
                        options,
                        &mut calls,
                    )
                    .await?;
                    judge_failures += cell.judge_failures;
                    raw.repeats.push(RawRepeat {
                        run_id: cell.run_ids[0].clone(),
                        readings: cell.readings,
                    });
                    raw.traces.extend(cell.traces);
                    raw.evidence_files.extend(cell.evidence_files);
                }
                native_raw = Some(raw);
            }
            Err(error) => {
                judge_failures += 1;
                native_raw = Some(RawCell {
                    purpose: "native".to_owned(),
                    label: Some(CellLabel::native()),
                    key: OwnedCellKey {
                        arm: Arm {
                            structure: Structure::Loop,
                            judge: JudgeSide::Llm,
                        },
                        evidence: Evidence::Own,
                        cadence: Cadence::Triggered,
                        family: "A".to_owned(),
                        scale: None,
                    },
                    repeats: Vec::new(),
                    traces: Vec::new(),
                    evidence_files: Vec::new(),
                    verification: unavailable_verification(JudgeName::Llm, &error.to_string()),
                });
            }
        }
    }

    // 3. 密度扫描：检查节奏 × 规模，逐臂跑（缺省用两个「依赖维护」臂；没有就退回全部臂）。
    let density_arms: Vec<Arm> = {
        let maintained: Vec<Arm> = arms
            .iter()
            .copied()
            .filter(|arm| arm.structure == Structure::Maintained)
            .collect();
        if maintained.is_empty() {
            arms.clone()
        } else {
            maintained
        }
    };
    let mut density: Vec<DensityCell> = Vec::new();
    for cadence in &options.cadences {
        for scale in &options.scales {
            for arm in &density_arms {
                let (name, judge) = &built[arm.judge.label()];
                let Ok(judge) = judge else {
                    continue;
                };
                let mut aggregate = LoopReadings::default();
                let mut run_ids = Vec::new();
                for family in &families {
                    let tasks: Vec<&FamilyTask> =
                        family.tasks.iter().take(*scale).copied().collect();
                    let key = CellKey {
                        arm: *arm,
                        evidence: Evidence::Own,
                        cadence: *cadence,
                        purpose: "density",
                        family: family.label,
                        scale: Some(*scale),
                        repeat: 1,
                    };
                    let cell = run_cell(
                        family.view,
                        family.scope,
                        &tasks,
                        judge.as_ref(),
                        &key,
                        options,
                        &mut calls,
                    )
                    .await?;
                    judge_failures += cell.judge_failures;
                    merge_readings(&mut aggregate, &cell.readings);
                    run_ids.extend(cell.run_ids);
                }
                finalize_rates(&mut aggregate);
                let bucket = log_bucket(&read_entries(&options.calls_path)?, &run_ids);
                density.push(DensityCell {
                    cadence: cadence.label(),
                    scale: *scale,
                    arm: arm.label(),
                    families: families
                        .iter()
                        .map(|family| family.label.to_owned())
                        .collect(),
                    repeats: 1,
                    readings: aggregate,
                    calls: bucket.calls.clone(),
                    latency_ms: bucket.latency(),
                    usage: bucket.usage(),
                });
                let _ = name;
            }
        }
    }

    // 4. 族 C 判读臂：环境每个用例只跑一次，四个臂读同一份记录。
    let schedules: BTreeMap<CFamily, Vec<c_program::Schedule>> = CFamily::ALL
        .into_iter()
        .map(|family| {
            let limit = options.max_schedules.unwrap_or(usize::MAX);
            (
                family,
                c_program::schedules(family)
                    .into_iter()
                    .take(limit)
                    .collect(),
            )
        })
        .collect();
    let contracts = match &options.contracts {
        Some(path) => Contracts::File(c_measure::read_contracts(path)?),
        None => Contracts::Generate(c_measure::ContractGenerator::from_env()?),
    };
    let mut library = jh_core::ObserverLibrary::default();
    let mut observer_state = SemanticState::in_memory();
    let mut setups: BTreeMap<CFamily, ChannelSetup> = BTreeMap::new();
    let mut setup_reports = Vec::new();
    let contract_run = format!("{}-contract", options.run);
    let mut contract_entries: Vec<c_measure::GeneratedContract> = Vec::new();
    for family in CFamily::ALL {
        let (report, setup) = setup_channel(
            &contracts,
            &mut library,
            &mut observer_state,
            family,
            &contract_run,
            &mut calls,
        )
        .await?;
        if let Some(report) = report {
            if report.call.is_some() {
                judge_failures += u32::from(!report.accepted);
            }
            if !report.enabled {
                judge_failures += 1;
            }
            setup_reports.push(report);
        }
        if let Some(setup) = setup {
            contract_entries.push(c_measure::GeneratedContract {
                family,
                call: MeasureCall {
                    model: None,
                    request_id: None,
                    usage: None,
                    input_bytes: 0,
                    output_bytes: 0,
                    latency_ms: 0,
                    error: None,
                },
                contract: Some(setup.contract.clone()),
                check: Some(setup.check.clone()),
                rejection: None,
            });
            setups.insert(family, setup);
        }
    }
    if matches!(contracts, Contracts::Generate(_)) {
        let path = options.out_dir.join("contracts.json");
        compare::write_json(&path, &contract_entries)?;
    }

    let channel_families: Vec<CFamily> = CFamily::ALL
        .into_iter()
        .filter(|family| !schedules[family].is_empty())
        .collect();
    let channel_cases = build_channel_cases(&channel_families, &schedules, &setups);
    let mut channel: Vec<ChannelCell> = Vec::new();
    for arm in &arms {
        let (name, judge) = &built[arm.judge.label()];
        let Ok(judge) = judge else {
            continue;
        };
        if arm.structure == Structure::Maintained && setups.is_empty() {
            continue;
        }
        let cells = channel_cells_for_arm(
            *arm,
            judge.as_ref(),
            &channel_families,
            &channel_cases,
            repeats,
            options,
            &mut calls,
        )
        .await?;
        for cell in &cells {
            judge_failures += cell.failures;
        }
        let _ = name;
        channel.extend(cells);
    }

    // 5. 跨任务复用消融：同一族的通道建一次（复用）vs 每个用例重建。
    let mut reuse_section = ReuseSection {
        enabled: false,
        protocol: "固定为「依赖维护 + Jev」这一臂；两侧读同一份通道材料，差别只在通道的建法：\
                   复用侧一族建一次并复用到该族全部用例，重建侧每个用例都重新生成契约、重建并重新独立检验。"
            .to_owned(),
        reuse: None,
        rebuild: None,
        curve: Vec::new(),
        note: "统计单位是「用例数 → 累计生成调用数 / 累计检出数」；侧间用例集合逐条相同。"
            .to_owned(),
    };
    if options.ablate_reuse && !channel_cases.is_empty() {
        let arm = Arm {
            structure: Structure::Maintained,
            judge: JudgeSide::Jev,
        };
        let judge_name = options.channel_override.unwrap_or(arm.channel());
        let judge = compare::build_judge(judge_name, &options.calls_path)?;
        for side in [ChannelReuse::Reuse, ChannelReuse::Rebuild] {
            let (report, _, points) = run_reuse_side(
                side,
                arm,
                judge.as_ref(),
                &channel_families,
                &channel_cases,
                &contracts,
                options,
                &mut calls,
            )
            .await?;
            if matches!(side, ChannelReuse::Reuse) {
                reuse_section.reuse = Some(report);
            } else {
                reuse_section.rebuild = Some(report);
            }
            reuse_section.curve.extend(points);
        }
        if let (Some(reuse), Some(rebuild)) = (&reuse_section.reuse, &reuse_section.rebuild) {
            judge_failures += reuse.generation_failures + rebuild.generation_failures;
        }
        reuse_section.enabled = true;
    }

    // 6. 报告：读一次调用记录，逐格回填账与读数。
    let entries = read_entries(&options.calls_path)?;
    let main: Vec<LoopCell> = raws
        .iter()
        .filter(|raw| raw.purpose == "main")
        .map(|raw| raw.loop_cell(&entries))
        .collect();
    let diagnostic: Vec<LoopCell> = raws
        .iter()
        .filter(|raw| raw.purpose == "diagnostic")
        .map(|raw| raw.loop_cell(&entries))
        .collect();
    let native: Vec<LoopCell> = native_raw
        .iter()
        .map(|raw| raw.loop_cell(&entries))
        .collect();
    let contrasts = contrasts_of(&main);
    let h5 = hypothesis_section(&density, &main);
    let cost = cost_section(&entries, &options.run);
    let reuse_calls = reuse_section
        .reuse
        .as_ref()
        .map(|side| side.generation_calls)
        .unwrap_or(0)
        + reuse_section
            .rebuild
            .as_ref()
            .map(|side| side.generation_calls)
            .unwrap_or(0);
    let _ = reuse_calls;

    let report = M4Report {
        run: options.run.clone(),
        generated_at_unix_ms: unix_ms(),
        command: options.command.clone(),
        protocol: protocol_declaration(options, &arms, &density_arms, repeats),
        environment: compare::report_environment("run"),
        task_set: MergedTaskSet {
            families: families
                .iter()
                .map(|family| FamilyEntry {
                    family: family.label.to_owned(),
                    source: family.source.clone(),
                    tasks: family.tasks.len(),
                    shape: format!("{}（{} 轮）", family.shape, family.rounds),
                })
                .chain(std::iter::once(FamilyEntry {
                    family: "C".to_owned(),
                    source: "crates/jh-app/src/c_program.rs".to_owned(),
                    tasks: channel_cases.len(),
                    shape: format!(
                        "{} 个故障族 × 每族前 {} 条调度 × 4 路实现",
                        channel_families.len(),
                        options.max_schedules.unwrap_or(10)
                    ),
                }))
                .collect(),
            merged_tasks: families.iter().map(|family| family.tasks.len()).sum::<usize>()
                + channel_cases.len(),
            note: "合并任务集 = 族 A 固定任务 + 族 B 未见变体（两族走同一段匹配循环）+ 族 C 可控程序用例\
                   （走隔离环境与隐藏检查）。族 A / B 的主对照格与族 C 的判读格分别报告。"
                .to_owned(),
        },
        main,
        diagnostic,
        native,
        density,
        channel,
        channel_setup: setup_reports,
        reuse: reuse_section,
        h5,
        contrasts,
        cost,
        raw: RawPaths {
            out_dir: compare::absolute(&options.out_dir),
            traces: compare::absolute(&options.out_dir.join("traces")),
            evidence: compare::absolute(&options.out_dir.join("evidence")),
            native: compare::absolute(&options.out_dir.join("native")),
            channel_cases: compare::absolute(&options.out_dir.join("channel-cases")),
            contracts: compare::absolute(&options.out_dir.join("contracts.json")),
            reuse_cases: compare::absolute(&options.out_dir.join("reuse")),
            calls_log: compare::absolute(&options.calls_path),
        },
        notes: declared_notes(),
    };

    Ok(M4Outcome {
        report,
        clean: judge_failures == 0,
    })
}

/// 主对照内只切一个维度的比较：同一族、同一判断通道比结构，同一族、同一结构比判断通道。
fn contrasts_of(cells: &[LoopCell]) -> Vec<Contrast> {
    let mut contrasts = Vec::new();
    let families: BTreeSet<&str> = cells.iter().map(|cell| cell.family.as_str()).collect();
    for family in families {
        for judge in ["llm", "jev"] {
            let left = cells.iter().find(|cell| {
                cell.family == family && cell.judge == judge && cell.structure == "loop"
            });
            let right = cells.iter().find(|cell| {
                cell.family == family && cell.judge == judge && cell.structure == "maintained"
            });
            if let (Some(left), Some(right)) = (left, right) {
                contrasts.push(contrast(
                    family,
                    "structure",
                    &format!("judge={judge}"),
                    left,
                    right,
                ));
            }
        }
        for structure in ["loop", "maintained"] {
            let left = cells.iter().find(|cell| {
                cell.family == family && cell.structure == structure && cell.judge == "llm"
            });
            let right = cells.iter().find(|cell| {
                cell.family == family && cell.structure == structure && cell.judge == "jev"
            });
            if let (Some(left), Some(right)) = (left, right) {
                contrasts.push(contrast(
                    family,
                    "judge",
                    &format!("structure={structure}"),
                    left,
                    right,
                ));
            }
        }
    }
    contrasts
}

/// 一条比较读数。
fn contrast(
    family: &str,
    dimension: &str,
    fixed: &str,
    left: &LoopCell,
    right: &LoopCell,
) -> Contrast {
    Contrast {
        family: family.to_owned(),
        dimension: dimension.to_owned(),
        fixed: fixed.to_owned(),
        left: left.arm.clone(),
        right: right.arm.clone(),
        left_acceptance: ratio(left.readings.acceptance_passed, left.readings.tasks),
        right_acceptance: ratio(right.readings.acceptance_passed, right.readings.tasks),
        left_wrong_actions: left.readings.wrong_actions,
        right_wrong_actions: right.readings.wrong_actions,
        left_stale_basis: left.readings.stale_basis_commitments,
        right_stale_basis: right.readings.stale_basis_commitments,
        left_calls: left.calls.attempted,
        right_calls: right.calls.attempted,
        left_input_bytes: left.readings.input_bytes,
        right_input_bytes: right.readings.input_bytes,
        note: "只读同一族、同一被固定维度下的一对；跨配对的差异是交互作用，不作主效应读".to_owned(),
    }
}

/// H5 判定：依赖触发调度是否改善检查密度与成本折中。
fn hypothesis_section(density: &[DensityCell], main: &[LoopCell]) -> HypothesisSection {
    let mut readings = Vec::new();
    let mut improves_quality = true;
    let mut improves_cost = true;
    let mut compared = 0;
    let scales: BTreeSet<usize> = density.iter().map(|cell| cell.scale).collect();
    for scale in scales {
        let triggered: Vec<&DensityCell> = density
            .iter()
            .filter(|cell| cell.cadence == "triggered" && cell.scale == scale)
            .collect();
        let fixed: Vec<&DensityCell> = density
            .iter()
            .filter(|cell| cell.cadence != "triggered" && cell.scale == scale)
            .collect();
        for left in &triggered {
            for right in &fixed {
                if left.arm != right.arm {
                    continue;
                }
                compared += 1;
                let left_rate = ratio(left.readings.acceptance_passed, left.readings.tasks);
                let right_rate = ratio(right.readings.acceptance_passed, right.readings.tasks);
                let left_calls = left.calls.attempted;
                let right_calls = right.calls.attempted;
                readings.push(format!(
                    "规模 {scale}｜臂 {}｜依赖触发 vs {}：验收 {} vs {}｜判断请求 {} vs {}｜重算输入 {} vs {} 字节",
                    left.arm,
                    right.cadence,
                    percent(left_rate),
                    percent(right_rate),
                    left.readings.asks,
                    right.readings.asks,
                    left.readings.input_bytes,
                    right.readings.input_bytes,
                ));
                if let (Some(left_rate), Some(right_rate)) = (left_rate, right_rate)
                    && left_rate < right_rate
                {
                    improves_quality = false;
                }
                let cheaper = left.readings.asks <= right.readings.asks
                    && left.readings.input_bytes <= right.readings.input_bytes;
                let strictly = left.readings.asks < right.readings.asks
                    || left.readings.input_bytes < right.readings.input_bytes;
                if !(cheaper && strictly) {
                    improves_cost = false;
                }
                let _ = (left_calls, right_calls);
            }
        }
    }
    if compared == 0 {
        readings.push("密度扫描没有产生可比的一对（缺依赖触发或定频格）".to_owned());
    }
    let main_acceptance: Vec<String> = main
        .iter()
        .map(|cell| {
            format!(
                "{}｜{}：验收 {}（{}/{}）、请求 {} 次",
                cell.arm,
                cell.family,
                percent(ratio(cell.readings.acceptance_passed, cell.readings.tasks)),
                cell.readings.acceptance_passed,
                cell.readings.tasks,
                cell.readings.asks
            )
        })
        .collect();

    let supported = improves_quality && improves_cost && compared > 0;
    let conclusion = if compared == 0 {
        "证据不足：密度扫描没有形成可比的一对，H5 在本批不判定。".to_owned()
    } else if supported {
        "获得支持（本环境先导读数）：依赖触发调度在质量不劣的前提下判断请求与重算输入都不高于定频臂。"
            .to_owned()
    } else if improves_quality || improves_cost {
        "条件性支持（本环境先导读数）：两条判据只成立一条，适用条件见 readings 里的逐格读数。"
            .to_owned()
    } else {
        "不支持或被否定（本环境先导读数）：依赖触发调度在质量与成本上都没有优于定频臂。".to_owned()
    };

    HypothesisSection {
        statement: "H5：依赖触发的重查调度比固定频率检查改善「检查密度—成本」折中（研究目标 §9.2 第 7 行、\
                    §9.4 的检查密度维度）。"
            .to_owned(),
        criteria: vec![
            "判据 1（质量非劣）：相同规模下，依赖触发臂的验收通过率不低于任何一个定频臂。".to_owned(),
            "判据 2（成本更省）：相同规模下，依赖触发臂的判断请求数不高于、且至少有一项严格低于定频臂\
             （判断请求数或重算输入量）。"
                .to_owned(),
        ],
        reporting: "两条判据都在跑之前写死；跑后只把读数与布尔值填进来，不调参凑结论。\
                    定频臂保留同一套问题集合（只改重查的触发方式）。"
            .to_owned(),
        readings: readings.into_iter().chain(main_acceptance).collect(),
        improves_quality,
        improves_cost,
        supported,
        conclusion,
        caveats: vec![
            "先导规模：密度扫描每格只跑一次、各族任务数取规模轴的截断，不构成统计结论（研究目标 §10.3）。"
                .to_owned(),
            "密度扫描固定用两个「依赖维护」臂，判断通道维度的交互不在这一扫描里。".to_owned(),
            "定频臂的「证据需求没满足就重问」仍然生效：那是问题集契约的一部分，不是失效驱动的调度。"
                .to_owned(),
        ],
    }
}

/// 成本段：按运行标识的第二段（用途）分账。
fn cost_section(entries: &[CallEntry], run: &str) -> CostSection {
    let mut by_purpose: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut all = Vec::new();
    let mut runs: BTreeMap<String, u32> = BTreeMap::new();
    for entry in entries {
        let run_id = entry.run().to_owned();
        if !(run_id == run || run_id.starts_with(&format!("{run}-"))) {
            continue;
        }
        let purpose = purpose_of(&run_id, run);
        by_purpose.entry(purpose).or_default().push(run_id.clone());
        *runs.entry(run_id.clone()).or_insert(0) += 1;
        all.push(run_id);
    }
    let bucket_cost = |run_ids: &[String], purpose: &str| {
        let bucket = log_bucket(entries, run_ids);
        PurposeCost {
            purpose: purpose.to_owned(),
            calls: bucket.calls.clone(),
            latency_ms: bucket.latency(),
            usage: bucket.usage(),
        }
    };
    let by_purpose: Vec<PurposeCost> = by_purpose
        .iter()
        .map(|(purpose, run_ids)| bucket_cost(run_ids, purpose))
        .collect();
    let total = bucket_cost(&all, "total");

    CostSection {
        by_purpose,
        total,
        runs,
        note: "只报调用次数、token 用量与延迟，不折算金额（仓库没有权威价格表）；\
               动作与观察的自定计价单位另记在各自的读数里，不参与本段合计。"
            .to_owned(),
    }
}

/// 运行标识 → 用途。
fn purpose_of(run_id: &str, run: &str) -> String {
    match run_id.strip_prefix(run) {
        Some(rest) => rest
            .trim_start_matches('-')
            .split('-')
            .next()
            .unwrap_or("other")
            .to_owned(),
        None => "other".to_owned(),
    }
}

/// 协议声明。
fn protocol_declaration(
    options: &M4Options,
    arms: &[Arm],
    density_arms: &[Arm],
    repeats: usize,
) -> ProtocolDeclaration {
    ProtocolDeclaration {
        arms: arms
            .iter()
            .map(|arm| ArmDeclaration {
                arm: arm.label(),
                structure: arm.structure.label().to_owned(),
                judge: arm.judge.label().to_owned(),
                loop_arm: arm.loop_arm().label().to_owned(),
                description: arm.description(),
            })
            .collect(),
        dimensions: vec![
            "控制结构：循环式控制与重规划（不维护依赖、每轮全量重问） vs 依赖维护与结构精化\
             （失效传播 + 增量复用）"
                .to_owned(),
            "判断通道：普通 LLM 结构化判断 vs Jev 判断（同一份问题集、同一读取范围、同一材料）"
                .to_owned(),
        ],
        evidence_modes: vec![
            "own：各臂按自己的结构取得证据（主对照）".to_owned(),
            "equalized：所有臂一律全量重问，手上的答案集合逐轮相同（诊断对照，§9.1 第 2 段）".to_owned(),
        ],
        repetition: format!(
            "主对照与诊断对照每格重复 {repeats} 次；每次重复用同一份任务集合与顺序（配对），\
             方差按样本标准差（自由度 n−1）报，n < 2 时记 null。"
        ),
        native: format!(
            "原生基线臂（{}）：生成模型直接协调动作，无类型化问题、无 Jev 判断、无依赖维护；\
             环境、动作候选集、计价与独立验收与对照组相同。动作表只给 id / 计价 / 是否观察类，\
             不给前提门；完成声明与循环式控制一样在轮末写一条，同样不作证据。\
             两处与循环臂的机械差异如实登记：① 第三条验收（完成声明绑定交付物当前版本）由代码直接构造\
             输入投影，只要交付物存在即成立、不需要任何一次「读交付物」的判断（对基线偏宽的处置）；\
             ② 动作在当轮的提问后扰动注入之前执行，而循环臂的决策发生在注入之后。",
            options.native
        ),
        ablations: vec![
            "跨任务复用：族 C 的观察通道一族建一次并复用（reuse） vs 每个用例重新生成契约、重建并重新\
             独立检验（rebuild-per-case）；判读臂固定为「依赖维护 + Jev」。"
                .to_owned(),
            "固定频率检查：依赖触发（triggered） vs 每 k 轮全量重查（every-k），保留同一套问题集合。"
                .to_owned(),
        ],
        density: format!(
            "检查密度 × 规模：节奏 {:?} × 每族任务数 {:?}，逐臂跑；族 A 与族 B 合并汇总，每格跑一次。",
            options.cadences.iter().map(|c| c.label()).collect::<Vec<String>>(),
            options.scales
        ),
        metric_definitions: metric_definitions(),
    }
    .with_density_arms(density_arms)
}

impl ProtocolDeclaration {
    /// 密度扫描的臂表（放在密度协议文本之后，便于复核）。
    fn with_density_arms(mut self, density_arms: &[Arm]) -> Self {
        self.density.push_str(&format!(
            "；密度扫描的臂：{}。",
            density_arms
                .iter()
                .map(|arm| arm.label())
                .collect::<Vec<String>>()
                .join("、")
        ));
        self
    }
}

/// 读数口径原文（与 [`LoopReadings`]、[`ChannelCell`] 的字段一一对应）。
pub fn metric_definitions() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "acceptance".to_owned(),
            "任务结果由 family::accept 的三条独立判定给出（必需动作真的发出、没有不该发出的动作、\
             完成声明绑定当前交付物版本），完成声明不构成证据。"
                .to_owned(),
        ),
        (
            "opportunities".to_owned(),
            "决策机会 = 各任务轮数之和（族 A / B）；族 C 的机会 = 隐藏检查认定确实发生了违反的用例数。"
                .to_owned(),
        ),
        (
            "stale_basis_action_rate".to_owned(),
            "旧依据行动率 = 基于已失效依据发出的动作数 / 决策机会数。"
                .to_owned(),
        ),
        (
            "recovery".to_owned(),
            "恢复 = 必需动作在它第一次被要求的那一轮或之后发出；未恢复按任务与动作逐条列出。"
                .to_owned(),
        ),
        (
            "recompute".to_owned(),
            "重算输入量 = 判断请求里 state 的字节数之和；缓存复用率 = 复用事件 /（复用 + 请求）。"
                .to_owned(),
        ),
        (
            "invalidation".to_owned(),
            "漏失效 = 扰动触及的问题声明读取范围、却没有标记失效；误失效 = 不在范围内却被标记；\
             无关结果保留 = 不在范围内也没有被标记。"
                .to_owned(),
        ),
        (
            "probability".to_owned(),
            "Brier = Σ(p − t)²，i 遍历答案与真值标签的并集；对数损失 = −ln p_真值，真值被分配 0 概率时记 null。"
                .to_owned(),
        ),
        (
            "channel_verdict".to_owned(),
            "族 C 判读按声明的读出规则折算：取概率最大的选项；并列最大、选项缺失或概率全为 0 时记\
             「证据不足」（失败态，不折成成立也不算违反）。"
                .to_owned(),
        ),
        (
            "hidden_truth".to_owned(),
            "族 C 的真值取自 c_program::hidden_check 的隐藏检查（存储日志通道），臂说「通过」不等于没有违反；\
             隐藏检查自身失败时该用例不进分母。"
                .to_owned(),
        ),
        (
            "cost".to_owned(),
            "只报调用次数、token 用量与延迟；失败调用不重试、不换后端，如实计入并让退出码非 0。"
                .to_owned(),
        ),
        (
            "reuse".to_owned(),
            "跨任务复用的累计曲线按「用例数 → 累计生成调用数 / 累计检出数」读，两侧用例集合逐条相同。"
                .to_owned(),
        ),
    ])
}

/// 报告里的注意事项。
pub fn declared_notes() -> Vec<String> {
    vec![
        "臂的两个维度只切换所属维度：同一族、同一任务、同一初值、同一判定器构造路径。".to_owned(),
        "诊断对照（equalized）单独报告，不与主对照混读：它强行把证据差异抹平，代价是重算量不再可比。"
            .to_owned(),
        "原生基线臂单独报告：它检验接口统一是否人为限制了基线，不参与 2×2 的差值读法。".to_owned(),
        "族 C 的判读臂在给定结构下两个通道读到**逐字节相同**的材料；跨结构的材料差异是结构维度的\
         中介，不作判断器差异读。"
            .to_owned(),
        "显式失败：判定器构造失败、调用失败、契约被拒、通道未启用都如实留档，不换后端、不静默兜底。"
            .to_owned(),
        "先导规模：读数只作本环境基线，不构成统计结论，也不回答 RQ5 / H5 本身（研究目标 §10.3、§10.4）。"
            .to_owned(),
    ]
}

impl M4Report {
    /// 人读摘要：逐段给出读数、产物位置与复现入口。
    pub fn summary_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        lines.push(format!(
            "任务集：{}（合并 {} 个任务／用例）",
            self.task_set
                .families
                .iter()
                .map(|family| format!("{}={}", family.family, family.source))
                .collect::<Vec<String>>()
                .join("、"),
            self.task_set.merged_tasks
        ));

        lines.push(format!(
            "臂：{}",
            self.protocol
                .arms
                .iter()
                .map(|arm| format!("{}（{}）", arm.arm, arm.description))
                .collect::<Vec<String>>()
                .join("；")
        ));

        for cell in &self.main {
            lines.push(format!(
                "主对照｜{}｜族 {}：验收 {}（{}/{}）｜不该发出的动作 {}｜旧依据承诺 {}｜判断请求 {}（失败 {}）｜重算输入 {} 字节｜复用率 {}｜调用 {} 成功 {} 失败 {}｜延迟 p50 {}ms p95 {}ms｜token 输入 {} 输出 {}｜{}",
                cell.arm,
                cell.family,
                percent(ratio(
                    cell.readings.acceptance_passed,
                    cell.readings.tasks
                )),
                cell.readings.acceptance_passed,
                cell.readings.tasks,
                cell.readings.wrong_actions,
                cell.readings.stale_basis_commitments,
                cell.readings.asks,
                cell.readings.failed_asks,
                cell.readings.input_bytes,
                percent(cell.readings.cache_reuse_rate),
                cell.calls.attempted,
                cell.calls.succeeded,
                cell.calls.failed,
                cell.latency_ms.p50.unwrap_or(0),
                cell.latency_ms.p95.unwrap_or(0),
                cell.usage
                    .input_tokens
                    .map_or_else(|| "-".to_owned(), |value| value.to_string()),
                cell.usage
                    .output_tokens
                    .map_or_else(|| "-".to_owned(), |value| value.to_string()),
                cell.verification.detail
            ));
        }

        for cell in &self.diagnostic {
            lines.push(format!(
                "诊断对照（相同新增证据）｜{}｜族 {}：验收 {}（{}/{}）｜不该发出的动作 {}｜旧依据承诺 {}｜判断请求 {}｜重算输入 {} 字节",
                cell.arm,
                cell.family,
                percent(ratio(
                    cell.readings.acceptance_passed,
                    cell.readings.tasks
                )),
                cell.readings.acceptance_passed,
                cell.readings.tasks,
                cell.readings.wrong_actions,
                cell.readings.stale_basis_commitments,
                cell.readings.asks,
                cell.readings.input_bytes,
            ));
        }

        for cell in &self.native {
            lines.push(format!(
                "原生基线｜族 {}：验收 {}（{}/{}）｜不该发出的动作 {}｜发出动作 {}｜计价成本 {}｜调用 {} 失败 {}｜{}",
                cell.family,
                percent(ratio(
                    cell.readings.acceptance_passed,
                    cell.readings.tasks
                )),
                cell.readings.acceptance_passed,
                cell.readings.tasks,
                cell.readings.wrong_actions,
                cell.readings.executions,
                cell.readings.action_cost_units,
                cell.calls.attempted,
                cell.calls.failed,
                cell.verification.detail
            ));
        }

        for setup in &self.channel_setup {
            lines.push(format!(
                "族 C 通道｜{}（{}）：契约接受 {}｜核对通过 {}｜测量位置 [{}]｜独立检验 {}｜自证被拒 {}｜启用版本 {:?}｜失效 {:?}{}",
                setup.family,
                setup.source,
                setup.accepted,
                setup.contract_check_passed,
                setup.hook_points.join("、"),
                setup.independent_passed,
                setup.self_certifying_rejected,
                setup.enabled_version,
                setup.invalidation,
                setup
                    .rejection
                    .as_ref()
                    .map_or_else(String::new, |reason| format!("｜被拒：{reason}"))
            ));
        }

        for cell in &self.channel {
            lines.push(format!(
                "族 C 判读｜{}｜{}（材料 {}）：机会 {} 用例 {} 干净 {}｜检出 {}｜误放行 {}｜误报 {}｜弃权 {}（机会上 {} / 干净上 {}）｜检出率 {}｜误放行率 {}｜误报率 {}｜调用 {} 失败 {}｜材料 {} 字节｜{}",
                cell.arm,
                cell.family,
                cell.material,
                cell.opportunities,
                cell.cases,
                cell.clean_cases,
                cell.detections,
                cell.false_passes,
                cell.false_alarms,
                cell.inconclusive_on_opportunity + cell.inconclusive_on_clean,
                cell.inconclusive_on_opportunity,
                cell.inconclusive_on_clean,
                percent(cell.detection_rate),
                percent(cell.false_pass_rate),
                percent(cell.false_alarm_rate),
                cell.calls,
                cell.failures,
                cell.material_bytes,
                cell.verification.detail
            ));
        }

        for cell in &self.density {
            lines.push(format!(
                "密度扫描｜节奏 {}｜规模 {} 个任务／族｜{}：验收 {}（{}/{}）｜判断请求 {}｜重算输入 {} 字节｜不该发出的动作 {}",
                cell.cadence,
                cell.scale,
                cell.arm,
                percent(ratio(
                    cell.readings.acceptance_passed,
                    cell.readings.tasks
                )),
                cell.readings.acceptance_passed,
                cell.readings.tasks,
                cell.readings.asks,
                cell.readings.input_bytes,
                cell.readings.wrong_actions
            ));
        }

        if self.reuse.enabled {
            for side in [&self.reuse.reuse, &self.reuse.rebuild]
                .into_iter()
                .flatten()
            {
                lines.push(format!(
                    "跨任务复用｜{}（臂 {}）：用例 {}｜通道可用 {}｜生成调用 {}（失败／被拒 {}）｜检出 {}｜误放行 {}｜弃权 {}｜token 输入 {} 输出 {}",
                    side.side,
                    side.arm,
                    side.cases,
                    side.channel_available,
                    side.generation_calls,
                    side.generation_failures,
                    side.detections,
                    side.false_passes,
                    side.inconclusive,
                    side.input_tokens
                        .map_or_else(|| "-".to_owned(), |value| value.to_string()),
                    side.output_tokens
                        .map_or_else(|| "-".to_owned(), |value| value.to_string())
                ));
            }
        }

        lines.push(format!(
            "H5｜质量非劣 {}｜成本更省 {}｜结论：{}",
            self.h5.improves_quality, self.h5.improves_cost, self.h5.conclusion
        ));

        for cost in &self.cost.by_purpose {
            lines.push(format!(
                "成本｜{}：调用 {} 成功 {} 失败 {}｜延迟 p50 {}ms p95 {}ms｜token 输入 {} 输出 {}",
                cost.purpose,
                cost.calls.attempted,
                cost.calls.succeeded,
                cost.calls.failed,
                cost.latency_ms.p50.unwrap_or(0),
                cost.latency_ms.p95.unwrap_or(0),
                cost.usage
                    .input_tokens
                    .map_or_else(|| "-".to_owned(), |value| value.to_string()),
                cost.usage
                    .output_tokens
                    .map_or_else(|| "-".to_owned(), |value| value.to_string())
            ));
        }
        lines.push(format!(
            "合计：调用 {} 成功 {} 失败 {}｜token 输入 {} 输出 {}",
            self.cost.total.calls.attempted,
            self.cost.total.calls.succeeded,
            self.cost.total.calls.failed,
            self.cost
                .total
                .usage
                .input_tokens
                .map_or_else(|| "-".to_owned(), |value| value.to_string()),
            self.cost
                .total
                .usage
                .output_tokens
                .map_or_else(|| "-".to_owned(), |value| value.to_string())
        ));

        lines.push(format!("产物：{}", self.raw.out_dir));
        lines.push(format!("调用记录：{}", self.raw.calls_log));
        lines.push(format!("命令：{}", self.command));
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::judge::RuleJudge;

    fn cell_key<'a>(arm: Arm, evidence: Evidence, family: &'a str) -> CellKey<'a> {
        CellKey {
            arm,
            evidence,
            cadence: Cadence::Triggered,
            purpose: "main",
            family,
            scale: None,
            repeat: 1,
        }
    }

    #[test]
    fn the_channel_scoring_matrix_separates_failure_states() {
        use jh_core::ObserverVerdict;

        let violated = c_measure::Verdict::violated("真值：违反");
        let satisfied = c_measure::Verdict::satisfied("真值：无违反");
        let failed = c_measure::Verdict::failed("观测失败");

        let mut counts = ChannelCounts::default();
        // 真值违反：说违反=检出、说成立=误放行、失败=机会上的弃权。
        counts.score(&violated, &violated);
        counts.score(&violated, &satisfied);
        counts.score(&violated, &failed);
        // 真值干净：说成立=判净、说违反=误报、失败=干净上的弃权。
        counts.score(&satisfied, &satisfied);
        counts.score(&satisfied, &violated);
        counts.score(&satisfied, &failed);
        // 隐藏检查自己失败：不进任何分母。
        counts.score(&failed, &violated);

        assert_eq!(counts.opportunities, 3);
        assert_eq!(counts.clean_cases, 3);
        assert_eq!(counts.detections, 1);
        assert_eq!(counts.false_passes, 1);
        assert_eq!(counts.inconclusive_on_opportunity, 1);
        assert_eq!(counts.correct_clears, 1);
        assert_eq!(counts.false_alarms, 1);
        assert_eq!(counts.inconclusive_on_clean, 1);
        assert_eq!(counts.cases, 6, "隐藏检查失败的用例不进计数");
        assert!(matches!(failed.verdict, ObserverVerdict::Failed(_)));
    }

    #[test]
    fn the_native_cell_labels_itself_apart_from_the_two_by_two() {
        // 先导批次的报告里原生臂的 `arm` 字段一度被写成 `loop-llm`（两处都用同一套标签推导），
        // 这里把标签钉住：原生臂不是 2×2 里的任何一格。
        let label = CellLabel::native();
        assert_eq!(label.arm, "native-llm");
        assert_eq!(label.structure, "native");
        assert_eq!(label.judge, "llm");
        assert!(label.loop_arm.starts_with("n/a"));
        assert!(
            Arm::ALL
                .iter()
                .all(|arm| arm.label() != label.arm && arm.structure.label() != label.structure)
        );
    }

    #[test]
    fn the_arm_matrix_covers_the_two_by_two_without_extra_switches() {
        let labels: Vec<String> = Arm::ALL.iter().map(|arm| arm.label()).collect();
        assert_eq!(
            labels,
            vec![
                "loop-llm".to_owned(),
                "loop-jev".to_owned(),
                "maintained-llm".to_owned(),
                "maintained-jev".to_owned()
            ]
        );
        for arm in Arm::ALL {
            assert_eq!(Arm::from_label(&arm.label()), Some(arm));
            let loop_arm = arm.loop_arm();
            match arm.structure {
                Structure::Loop => {
                    assert!(!loop_arm.uses_propagation());
                    assert_eq!(loop_arm.recompute(), matching::Recompute::Full);
                }
                Structure::Maintained => {
                    assert!(loop_arm.uses_propagation());
                    assert_eq!(loop_arm.recompute(), matching::Recompute::Incremental);
                }
            }
            assert_eq!(arm.channel().cli_name(), arm.judge.label());
        }
    }

    #[test]
    fn the_equalized_mode_forces_the_same_ask_scope_for_every_arm() {
        // 诊断对照把两臂都推到全量重问：结构差异只剩失效传播与行动纪律。
        for arm in Arm::ALL {
            let loop_arm = matching::Arm {
                propagation: arm.loop_arm().propagation,
                recompute: matching::Recompute::Full,
            };
            assert_eq!(loop_arm.recompute(), matching::Recompute::Full);
            assert_eq!(
                loop_arm.uses_propagation(),
                arm.structure == Structure::Maintained
            );
        }
    }

    #[test]
    fn the_channel_reading_follows_the_declared_rule() {
        let answer = |label: &str, probabilities: BTreeMap<String, f64>| {
            Answer::Choice(jh_core::ChoiceAnswer {
                choice: label.to_owned(),
                probabilities,
                confidence: 1.0,
            })
        };
        let violated = verdict_of_answer(&answer(
            "violated",
            BTreeMap::from([("violated".to_owned(), 0.8), ("clean".to_owned(), 0.2)]),
        ));
        assert!(matches!(
            violated.verdict,
            jh_core::ObserverVerdict::Violated
        ));

        let clean = verdict_of_answer(&answer(
            "clean",
            BTreeMap::from([("violated".to_owned(), 0.1), ("clean".to_owned(), 0.9)]),
        ));
        assert!(matches!(clean.verdict, jh_core::ObserverVerdict::Satisfied));

        // 证据不足：显式选它，或概率并列，都记失败态（不折成成立、也不算违反）。
        let undetermined = verdict_of_answer(&answer(
            "undetermined",
            BTreeMap::from([
                ("violated".to_owned(), 0.2),
                ("clean".to_owned(), 0.2),
                ("undetermined".to_owned(), 0.6),
            ]),
        ));
        assert!(matches!(
            undetermined.verdict,
            jh_core::ObserverVerdict::Failed(_)
        ));
        let tied = verdict_of_answer(&answer(
            "violated",
            BTreeMap::from([("violated".to_owned(), 0.5), ("clean".to_owned(), 0.5)]),
        ));
        assert!(matches!(tied.verdict, jh_core::ObserverVerdict::Failed(_)));
        let empty = verdict_of_answer(&answer("clean", BTreeMap::new()));
        assert!(matches!(empty.verdict, jh_core::ObserverVerdict::Failed(_)));
    }

    #[test]
    fn readings_merge_by_count_and_recompute_their_rates() {
        let mut left = LoopReadings {
            tasks: 6,
            acceptance_passed: 5,
            opportunities: 18,
            asks: 20,
            reuse_events: 10,
            stale_basis_commitments: 2,
            ..LoopReadings::default()
        };
        finalize_rates(&mut left);
        let right = LoopReadings {
            tasks: 6,
            acceptance_passed: 4,
            opportunities: 18,
            asks: 30,
            reuse_events: 5,
            stale_basis_commitments: 0,
            ..LoopReadings::default()
        };

        let mut merged = left.clone();
        merge_readings(&mut merged, &right);
        finalize_rates(&mut merged);

        assert_eq!(merged.tasks, 12);
        assert_eq!(merged.acceptance_passed, 9);
        assert_eq!(merged.opportunities, 36);
        assert_eq!(merged.stale_basis_commitments, 2);
        assert!((merged.stale_basis_action_rate.expect("有分母") - 2.0 / 36.0).abs() < 1e-12);
        assert!((merged.cache_reuse_rate.expect("有分母") - 15.0 / 65.0).abs() < 1e-12);
    }

    #[test]
    fn the_cell_report_carries_both_dimensions_and_the_variance() {
        let key = cell_key(Arm::ALL[2], Evidence::Equalized, "A");
        let per_repeat = vec![
            RepeatReading {
                repeat: 1,
                tasks: 6,
                acceptance_passed: 6,
                acceptance_rate: Some(1.0),
                executions: 12,
                wrong_actions: 0,
                stale_basis_commitments: 0,
                asks: 18,
                input_bytes: 1000,
                calls: 18,
                input_tokens: Some(10),
                output_tokens: Some(20),
            },
            RepeatReading {
                repeat: 2,
                tasks: 6,
                acceptance_passed: 5,
                acceptance_rate: Some(5.0 / 6.0),
                executions: 11,
                wrong_actions: 1,
                stale_basis_commitments: 0,
                asks: 20,
                input_bytes: 1100,
                calls: 20,
                input_tokens: Some(12),
                output_tokens: Some(22),
            },
        ];
        let cell = loop_cell(
            "main",
            &key,
            2,
            per_repeat,
            LoopReadings {
                tasks: 12,
                acceptance_passed: 11,
                opportunities: 36,
                ..LoopReadings::default()
            },
            &["m4-1-main-maintained-llm-A-r1".to_owned()],
            Vec::new(),
            Vec::new(),
            &[],
            Verification {
                status: VerificationStatus::Verified,
                detail: "真实通道".to_owned(),
            },
        );

        assert_eq!(cell.structure, "maintained");
        assert_eq!(cell.judge, "llm");
        assert_eq!(cell.evidence, "equalized");
        assert_eq!(cell.loop_arm, "h1on-h2inc");
        assert_eq!(cell.repeats, 2);
        assert_eq!(cell.variance.samples, 2);
        let sd = cell.variance.acceptance_rate_sd.expect("两个样本有标准差");
        assert!((sd - 0.117_851_130_197_757_93).abs() < 1e-12, "{sd}");
        assert!(cell.calls.errors.is_empty());
    }

    #[test]
    fn purposes_come_from_the_run_id_shape_and_unknown_ids_do_not_panic() {
        let run = "m4-1790000000000";
        assert_eq!(
            purpose_of(&format!("{run}-main-loop-llm-A-r1"), run),
            "main"
        );
        assert_eq!(purpose_of(&format!("{run}-contract"), run), "contract");
        assert_eq!(
            purpose_of(&format!("{run}-reuse-reuse-maintained-jev"), run),
            "reuse"
        );
        assert_eq!(purpose_of("h3-1-fixed", run), "other");
    }

    #[test]
    fn a_rule_judge_can_drive_the_loop_without_network() {
        // 离线口径：本地判定器照样能走完整条循环（真调路径由 --judge 覆盖 + 集成测试覆盖）。
        let _judge = RuleJudge::new();
        assert_eq!(ratio(1, 0), None);
        assert_eq!(sample_sd(&[1.0]), None);
        assert!(sample_sd(&[1.0, 3.0]).expect("两个样本有标准差") > 1.0);
    }
}
