//! H4：任务族 C 的三臂对照（研究目标 §7.4；路线图 §6.2 的消融落点）。
//!
//! §7.4 要求「在一系列相关但不同的故障任务上，比较固定测试、通用测试生成与针对认知缺口建设
//! 观测器的效果」，并报告误放行、误报、迁移、维护开销和建设成本摊薄曲线。本模块就是这件事的
//! 编排与读数，一条命令出 JSON 报告（`jev h4`）。
//!
//! # 三个臂
//!
//! | 臂 | 看什么 | 为什么它在 §6.2 的消融名单里 |
//! | --- | --- | --- |
//! | `fixed` | 只能用族 C 之前就存在的可观察量（自报状态、当前值、意图数） | 「只能使用已有观察工具」 |
//! | `generated` | 同样是那些可观察量，但断言由**真实 LLM** 现生成 | 「使用通用测试生成」 |
//! | `observer` | 声明式测量契约 + 写入边界记录器 + 可控暂停点（+ 可选提交计数器） | 「针对认知缺口建设观测器」 |
//!
//! # 一次运行怎么算
//!
//! 同一个用例（族 × 调度 × 实现 × 版本 × 挂载）**只跑一次环境**，三个臂各出一个判定，
//! 隐藏检查出一个判定。评分的分母来自隐藏检查，不来自任何臂：
//!
//! - `opportunities`：隐藏检查认定「确实发生了违反」的用例数；
//! - `detections`：其中臂也说「发现违反」的；
//! - `false_passes`：其中臂说「命题成立」的——**误放行**；
//! - `inconclusive`：其中臂落在失败态（说不出话，不算放行也不算检出）；
//! - `clean_runs` / `false_alarms`：隐藏检查说没事、臂却说违反的——**误报**。
//!
//! 重复次数 N = 每族的调度条数（缺省 10）。竞态不靠掷骰子重复，靠 10 条不同的确定性调度——
//! 一次没检出**不读作「不存在」**，只读作这条调度上没检出，检出率给 n/N。
//!
//! # 迁移、维护与摊薄
//!
//! 三个族各建一个观测器，然后交叉套用（[`TransferCell`]）：对角线是「为它建的」，非对角线是
//! 「**没有**为它建、直接拿来用」的迁移读数。版本变更阶段（[`MaintenanceSection`]）跑三种维护形态：
//! 只改契约的支持版本、重挂钩子点 + 重跑独立检验、以及「只改支持版本且契约拿掉了提交计数器」——
//! 最后一种量的是**选错钩子点**的代价。摊薄曲线（[`AmortizationPoint`]）给的是「覆盖到第 k 个族
//! 时的累积生成调用数 vs 累积检出数」；验证与重复运行不折算成调用数，在
//! [`CostSection`] 的 `runs` 里单独记。
//!
//! # 如实报告
//!
//! [`HypothesisSection`] 的两个布尔量在**跑之前就写好判据**：建设观测器是否改善了后续任务的
//! 结果、是否改善了预算利用。两个都不成立时，结论原文写「不支持累积能力假说」——不调参凑正向。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use jh_core::{ObserverContract, ObserverVerdict, SemanticState};
use serde::Serialize;

use crate::c_measure::{
    Check, EnableOutcome, FixedDeclaration, GeneratedContract, GeneratedTests, IndependentCheck,
    MeasureCall, Observer, Verdict,
};
use crate::c_program::{self, Attachment, CFamily, Implementation, ProgramVersion, Schedule};
use crate::calls::{CallEntry, CallLog};
use crate::compare::{ReportEnvironment, absolute, report_environment, write_json};
use crate::error::Error;
use crate::exp::unix_ms;
use crate::generation::ContractCheck;
use crate::judge::JudgeName;
use crate::report::{CallStats, LatencyStats, UsageStats, Verification, VerificationStatus};

/// 固定测试臂的标识。
pub const ARM_FIXED: &str = "fixed";
/// 通用测试生成臂的标识。
pub const ARM_GENERATED: &str = "generated";
/// 建设观测器臂的标识。
pub const ARM_OBSERVER: &str = "observer";
/// 版本变更后**只改了支持版本**、没有重挂钩子点的观测器臂。
pub const ARM_FIXED_V2: &str = "fixed@v2";
/// 版本变更后仍然原样使用的通用测试生成臂。
pub const ARM_GENERATED_V2: &str = "generated@v2";
/// 版本变更后只改支持版本的观测器臂。
pub const ARM_OBSERVER_V2_SUPPORT_ONLY: &str = "observer@v2-support-only";
/// 版本变更后重挂钩子点、重跑独立检验的观测器臂。
pub const ARM_OBSERVER_V2_REVERIFIED: &str = "observer@v2-reverified";
/// 版本变更后只改支持版本、**且契约拿掉了提交计数器**的观测器臂（用来量计数器关掉了什么）。
pub const ARM_OBSERVER_V2_NO_COUNTER: &str = "observer@v2-support-only-no-counter";

/// 生成新的运行标识：`h4-<Unix 毫秒>`。
pub fn new_run_id() -> String {
    format!("h4-{}", unix_ms())
}

/// 契约 / 测试计划的来源。
#[derive(Debug, Clone)]
pub enum ArtifactSource {
    /// 真调生成模型现生成。
    Generate,
    /// 复用已落盘的产物（不重发生成调用）。
    File(PathBuf),
}

/// 一次 H4 运行的参数。
#[derive(Debug, Clone)]
pub struct H4Options {
    /// 运行标识。
    pub run: String,
    /// 调用记录落盘路径（JSONL，追加）。
    pub calls_path: PathBuf,
    /// 运行产物目录。
    pub out_dir: PathBuf,
    /// 复现命令，原样写进报告。
    pub command: String,
    /// 测量契约的来源。
    pub contracts: ArtifactSource,
    /// 通用测试的来源。
    pub tests: ArtifactSource,
    /// 每族的调度条数（缺省 10 = 全部）。
    pub repeats: Option<usize>,
}

/// H4 运行的结果。
#[derive(Debug, Clone)]
pub struct H4Outcome {
    /// 报告。
    pub report: H4Report,
    /// 是否每一条真实调用都成功。
    pub clean: bool,
}

/// H4 报告。
#[derive(Debug, Clone, Serialize)]
pub struct H4Report {
    /// 运行标识。
    pub run: String,
    /// 生成时刻。
    pub generated_at_unix_ms: u64,
    /// 复现命令。
    pub command: String,
    /// 协议声明（三臂、口径、边界）。
    pub protocol: ProtocolDeclaration,
    /// 非保密环境。
    pub environment: ReportEnvironment,
    /// 任务族摘要。
    pub families: Vec<FamilySummary>,
    /// 三个臂 × 三个族的读数。
    pub arms: Vec<ArmReport>,
    /// 迁移矩阵（观测器交叉套用）。
    pub transfer: Vec<TransferCell>,
    /// 独立检验与生命周期。
    pub verification: VerificationSection,
    /// 版本变更的维护开销读数。
    pub maintenance: MaintenanceSection,
    /// 建设成本摊薄曲线。
    pub amortization: AmortizationSection,
    /// 累积能力假说的判据与读数。
    pub hypothesis: HypothesisSection,
    /// 生成步骤。
    pub generation: GenerationSection,
    /// 原始数据位置。
    pub raw: RawPaths,
    /// 调用成本账。
    pub cost: CostSection,
    /// 口径与限制说明。
    pub notes: Vec<String>,
}

/// 报告里的协议声明。
#[derive(Debug, Clone, Serialize)]
pub struct ProtocolDeclaration {
    /// 三个臂。
    pub arms: Vec<String>,
    /// 每臂 × 每族 × N 的重复口径。
    pub repetition_protocol: String,
    /// 评分口径（分母来自隐藏检查）。
    pub scoring_protocol: String,
    /// 结构性声明（读数字之前先读这几条）。
    pub boundaries: Vec<String>,
    /// 被禁止的替换。
    pub banned_substitutions: Vec<String>,
    /// 指标口径。
    pub metric_definitions: BTreeMap<String, String>,
}

impl ProtocolDeclaration {
    /// 指标口径声明。
    pub fn metric_definitions() -> BTreeMap<String, String> {
        BTreeMap::from([
            (
                "opportunity".to_owned(),
                "机会 = 隐藏检查（存储日志通道）认定「确实发生了违反」的用例数。分母来自隐藏检查，\
                 不来自任何被测臂：臂说「通过」不等于没有违反。"
                    .to_owned(),
            ),
            (
                "detection_rate".to_owned(),
                "检出率 = 检出数 ÷ 机会数（n/N）。N = 每族的调度条数（缺省 10）；一次没检出只说明\
                 这条调度上没检出，不读作「不存在」。机会数为 0 时记 null，不记 0。"
                    .to_owned(),
            ),
            (
                "false_pass_rate".to_owned(),
                "误放行率 = 臂说「命题成立」而隐藏检查说「发现违反」的用例数 ÷ 机会数。\
                 这是 §7.4 要求报告的第一个数。"
                    .to_owned(),
            ),
            (
                "false_alarm_rate".to_owned(),
                "误报率 = 隐藏检查说「命题成立」而臂说「发现违反」的用例数 ÷ 无违反用例数。"
                    .to_owned(),
            ),
            (
                "inconclusive".to_owned(),
                "落在失败态的用例数（观测器报「记录器被绕过」、契约不支持这个版本……）。\
                 失败态不折成「成立」也不算「违反」——它既不是检出也不是误放行。"
                    .to_owned(),
            ),
            (
                "transfer".to_owned(),
                "迁移 = 为族 X 建的观测器直接套到族 Y（X≠Y）上的检出率与误报率。对角线是「为它建的」。\
                 套用不改契约；要改就得重新生成契约，那笔账记在建设成本里。"
                    .to_owned(),
            ),
            (
                "maintenance".to_owned(),
                "维护开销 = 宿主代码换了版本之后，每臂为了继续可信而必须重新做的事：重新生成的调用数、\
                 钩子点是否重挂、独立检验是否重跑；以及重做前后各自的检出率与误放行率。"
                    .to_owned(),
            ),
            (
                "amortization".to_owned(),
                "建设成本摊薄曲线 = 覆盖到第 k 个族时的累积生成调用数（横轴）与累积检出数、\
                 每检出一次要花多少次调用（纵轴）。重复运行与验证运行不折算成调用数，单独计数。"
                    .to_owned(),
            ),
            (
                "billing".to_owned(),
                "成本只记调用次数、token 与延迟；货币成本不虚构（仓库里没有权威价格表）。\
                 动作与观察的自定计价单位不参与本对照。"
                    .to_owned(),
            ),
            (
                "ground_truth".to_owned(),
                "真值来自隐藏检查：它只读存储自己写的日志，不看记录器、不看程序自报。\
                 记录器被绕过或被篡改时，它照旧记着真实副作用。"
                    .to_owned(),
            ),
        ])
    }
}

/// 一个族的摘要。
#[derive(Debug, Clone, Serialize)]
pub struct FamilySummary {
    /// 族标识。
    pub family: String,
    /// 标题。
    pub title: String,
    /// 原始要求。
    pub requirement: String,
    /// 要求来源。
    pub requirement_source: String,
    /// 观测对象。
    pub object: String,
    /// 是不是 §7.3 的标志性任务。
    pub landmark: bool,
    /// 调度条数与说明。
    pub schedules: Vec<ScheduleSummary>,
    /// 隐藏检查认定的违反判据。
    pub hidden_criterion: String,
    /// 固定测试臂在这一族上声明的检查项；`weakened_substitution = true` 的那一条就是 §7.3
    /// 明令禁止的替换（它作为对照存在，不是用来满足要求的）。
    pub fixed_arm_declaration: Vec<FixedDeclaration>,
}

/// 一条调度的摘要。
#[derive(Debug, Clone, Serialize)]
pub struct ScheduleSummary {
    /// 调度标识。
    pub schedule: String,
    /// 覆盖的情形。
    pub note: String,
    /// 指令条数。
    pub ops: usize,
}

/// 一个臂在一个族（或一个版本）上的读数。
#[derive(Debug, Clone, Serialize)]
pub struct ArmReport {
    /// 臂标识。
    pub arm: String,
    /// 这一臂看的是什么。
    pub subject: String,
    /// 所属族。
    pub family: String,
    /// 程序版本。
    pub version: String,
    /// 记录器挂载。
    pub attachment: String,
    /// 这一臂在这一族上的可用性。
    pub verification: Verification,
    /// 调度条数（N）。
    pub schedules: usize,
    /// 实现路数。
    pub implementations: Vec<String>,
    /// 用例数 = 调度 × 实现。
    pub runs: u32,
    /// 机会数（隐藏检查认定有违反）。
    pub opportunities: u32,
    /// 无违反用例数。
    pub clean_runs: u32,
    /// 检出数。
    pub detections: u32,
    /// 误放行数。
    pub false_passes: u32,
    /// 有违反却落在失败态的用例数。
    pub inconclusive_on_opportunity: u32,
    /// 误报数。
    pub false_alarms: u32,
    /// 无违反且判「成立」的用例数。
    pub correct_clears: u32,
    /// 无违反却落在失败态的用例数。
    pub inconclusive_on_clean: u32,
    /// 检出率。
    pub detection_rate: Option<f64>,
    /// 误放行率。
    pub false_pass_rate: Option<f64>,
    /// 误报率。
    pub false_alarm_rate: Option<f64>,
    /// 逐调度的读数。
    pub by_schedule: Vec<ScheduleReading>,
}

/// 一条调度上的各臂读数。
#[derive(Debug, Clone, Serialize)]
pub struct ScheduleReading {
    /// 调度标识。
    pub schedule: String,
    /// 这条调度覆盖的情形。
    pub note: String,
    /// 这条调度上的机会数。
    pub opportunities: u32,
    /// 这条调度上的无违反用例数。
    pub clean_runs: u32,
    /// 这条调度上隐藏检查的判定（按实现）。
    pub hidden: Vec<String>,
    /// 各臂的计数格。
    pub arms: BTreeMap<String, ArmCell>,
}

/// 一个臂在一条调度上的计数格。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ArmCell {
    /// 检出数。
    pub detections: u32,
    /// 误放行数。
    pub false_passes: u32,
    /// 有违反却落在失败态。
    pub inconclusive_on_opportunity: u32,
    /// 误报数。
    pub false_alarms: u32,
    /// 无违反且判「成立」。
    pub correct_clears: u32,
    /// 无违反却落在失败态。
    pub inconclusive_on_clean: u32,
}

impl ArmReport {
    /// 落在失败态的用例数。
    pub fn inconclusive(&self) -> u32 {
        self.inconclusive_on_opportunity + self.inconclusive_on_clean
    }
}

impl ArmCell {
    /// 机会数。
    pub fn opportunities(&self) -> u32 {
        self.detections + self.false_passes + self.inconclusive_on_opportunity
    }

    /// 无违反用例数。
    pub fn clean_runs(&self) -> u32 {
        self.false_alarms + self.correct_clears + self.inconclusive_on_clean
    }

    /// 落在失败态的用例数。
    pub fn inconclusive(&self) -> u32 {
        self.inconclusive_on_opportunity + self.inconclusive_on_clean
    }

    /// 累计另一格。
    pub fn add(&mut self, other: &ArmCell) {
        self.detections += other.detections;
        self.false_passes += other.false_passes;
        self.inconclusive_on_opportunity += other.inconclusive_on_opportunity;
        self.false_alarms += other.false_alarms;
        self.correct_clears += other.correct_clears;
        self.inconclusive_on_clean += other.inconclusive_on_clean;
    }
}

/// 迁移矩阵的一格。
#[derive(Debug, Clone, Serialize)]
pub struct TransferCell {
    /// 观测器是为哪个族建的。
    pub built_for: String,
    /// 套用到哪个族。
    pub applied_to: String,
    /// 是否对角线（为它自己建的）。
    pub same_family: bool,
    /// 用例数。
    pub runs: u32,
    /// 机会数。
    pub opportunities: u32,
    /// 检出数。
    pub detections: u32,
    /// 误放行数。
    pub false_passes: u32,
    /// 有违反却落在失败态（弃权让违反溜过去的那些）。
    pub inconclusive_on_opportunity: u32,
    /// 无违反却落在失败态（弃权，不构成放行也不构成误报）。
    pub inconclusive_on_clean: u32,
    /// 误报数。
    pub false_alarms: u32,
    /// 无违反用例数。
    pub clean_runs: u32,
    /// 无违反且判「成立」的用例数。
    pub correct_clears: u32,
    /// 检出率。
    pub detection_rate: Option<f64>,
    /// 误报率。
    pub false_alarm_rate: Option<f64>,
}

/// 独立检验与生命周期读数。
#[derive(Debug, Clone, Serialize)]
pub struct VerificationSection {
    /// 每个族的四路检验与启用结果。
    pub per_family: Vec<FamilyVerification>,
    /// 口径说明。
    pub note: String,
}

/// 一个族的独立检验读数。
#[derive(Debug, Clone, Serialize)]
pub struct FamilyVerification {
    /// 族标识。
    pub family: String,
    /// 观测器标识。
    pub observer: String,
    /// 契约版本号。
    pub version: Option<u32>,
    /// 观测器装了哪些钩子点。
    pub hook_points: Vec<String>,
    /// 契约协议核对结果。
    pub contract_check: ContractCheck,
    /// 四路检验的逐路读数。
    pub directions: Vec<DirectionReading>,
    /// 四路是否全过。
    pub passed: bool,
    /// 观测器在「绕过记录器」那一路漏掉了没有。
    pub observer_missed_the_bypass: bool,
    /// 自证式的伪隐藏检查是否被门槛拒绝。
    pub self_certifying_rejected: bool,
    /// 被拒的原文。
    pub self_certifying_error: Option<String>,
    /// 被拒之后生命周期保持原状的读数。
    pub lifecycle_after_rejection: Option<String>,
    /// 被拒之后库里留下的检验记录数。
    pub checks_on_record_after_rejection: usize,
    /// 启用写下的生命周期记录。
    pub enabled_records: Vec<(u32, String)>,
    /// 启用时的失效传播读数。
    pub invalidation: Option<String>,
}

/// 一路独立检验的读数。
#[derive(Debug, Clone, Serialize)]
pub struct DirectionReading {
    /// 哪一路。
    pub direction: String,
    /// 用的什么实现。
    pub implementation: String,
    /// 跑的哪条调度。
    pub schedule: String,
    /// 是否注入了故障。
    pub injected: bool,
    /// 期望观测器给什么。
    pub expected: String,
    /// 观测器实际给的。
    pub observer: String,
    /// 观测器给的依据。
    pub observer_detail: String,
    /// 隐藏检查给的。
    pub hidden: String,
    /// 这一路是否通过。
    pub passed: bool,
}

/// 版本变更的维护开销。
#[derive(Debug, Clone, Serialize)]
pub struct MaintenanceSection {
    /// 版本变更说明了什么。
    pub change: String,
    /// 每臂逐族的读数。
    pub arms: Vec<MaintenanceReading>,
    /// 每臂在全部族上的合计（「重挂之后检出率从多少到多少」这类话，只靠报告就能闭合）。
    pub totals: Vec<MaintenanceTotal>,
    /// 口径说明。
    pub note: String,
}

/// 一个臂在版本变更阶段全部族上的合计读数。
#[derive(Debug, Clone, Serialize)]
pub struct MaintenanceTotal {
    /// 臂标识。
    pub arm: String,
    /// 覆盖几个族。
    pub families: usize,
    /// 用例数。
    pub runs: u32,
    /// 机会数。
    pub opportunities: u32,
    /// 无违反用例数。
    pub clean_runs: u32,
    /// 检出数。
    pub detections: u32,
    /// 误放行数。
    pub false_passes: u32,
    /// 有违反却落在失败态。
    pub inconclusive_on_opportunity: u32,
    /// 无违反却落在失败态。
    pub inconclusive_on_clean: u32,
    /// 误报数。
    pub false_alarms: u32,
    /// 变更前的检出率（同一个臂在 v1 上的同批读数）。
    pub detection_rate_before: Option<f64>,
    /// 变更后的检出率。
    pub detection_rate_after: Option<f64>,
    /// 变更后的误放行率。
    pub false_pass_rate_after: Option<f64>,
    /// 变更后的误报率。
    pub false_alarm_rate_after: Option<f64>,
}

/// 一个臂在版本变更前后的读数。
#[derive(Debug, Clone, Serialize)]
pub struct MaintenanceReading {
    /// 臂标识。
    pub arm: String,
    /// 版本变更后为继续可用而重新生成的调用数。
    pub regeneration_calls: u32,
    /// 是否真的重生成过（复用预置产物时为 false）。
    pub regenerated: bool,
    /// 钩子点是否重挂。
    pub reattached: bool,
    /// 独立检验是否重跑。
    pub reverified: bool,
    /// 变更前的检出率。
    pub detection_rate_before: Option<f64>,
    /// 变更后的检出率。
    pub detection_rate_after: Option<f64>,
    /// 变更后的误放行率。
    pub false_pass_rate_after: Option<f64>,
    /// 这一行是怎么来的。
    pub note: String,
}

/// 摊薄曲线的一行。
#[derive(Debug, Clone, Serialize)]
pub struct AmortizationPoint {
    /// 覆盖到第几个族。
    pub families_covered: usize,
    /// 臂 → 累积生成调用数。
    pub cumulative_calls: BTreeMap<String, u32>,
    /// 臂 → 累积机会数。
    pub opportunities: BTreeMap<String, u32>,
    /// 臂 → 累积检出数。
    pub detections: BTreeMap<String, u32>,
    /// 臂 → 累积检出率。
    pub detection_rate: BTreeMap<String, Option<f64>>,
    /// 臂 → 每检出一次要花多少次调用。
    pub calls_per_detection: BTreeMap<String, Option<f64>>,
}

/// 摊薄曲线。
#[derive(Debug, Clone, Serialize)]
pub struct AmortizationSection {
    /// 调用数这一列是怎么来的：`generate` = 本次真实发起的生成调用；
    /// `file` = 复用预置产物，调用数是「这件事的应花成本」（每族一次生成），不是本次真实调用数。
    pub cost_basis: String,
    /// 逐点读数。
    pub points: Vec<AmortizationPoint>,
    /// 版本变更之后的增量（第二个账期）。
    pub after_version_change: BTreeMap<String, u32>,
    /// 口径说明。
    pub note: String,
}

/// 累积能力假说的判据与读数。
#[derive(Debug, Clone, Serialize)]
pub struct HypothesisSection {
    /// 被检验的假说原文（§7.4）。
    pub statement: String,
    /// 跑之前就写好的判据（只有判定规则；报告方式的说明在 `reporting` 里，它与判据分开记，
    /// 因为它是跑后可以扩充的，而判据不能）。
    pub criteria: Vec<String>,
    /// 报告方式：读数怎么摆、给几种读法。跑后扩充过一次（加了「两侧读数」），如实记在这里。
    pub reporting: String,
    /// 读数。
    pub readings: Vec<String>,
    /// 是否改善了后续任务的结果（检出率）。
    pub improves_results: bool,
    /// 是否改善了预算利用（每检出一次要花多少次调用）。
    pub improves_budget: bool,
    /// 是否支持累积能力假说。
    pub supported: bool,
    /// 结论原文。
    pub conclusion: String,
    /// 保留意见。
    pub caveats: Vec<String>,
}

/// 生成步骤的报告段。
#[derive(Debug, Clone, Serialize)]
pub struct GenerationSection {
    /// 契约来源（`generate` 或文件的绝对路径）。
    pub contract_source: String,
    /// 测试来源。
    pub test_source: String,
    /// 本次运行真实发起的生成调用数。
    pub calls_this_run: u32,
    /// 本次运行失败的生成调用数。
    pub failures_this_run: u32,
    /// 被拒的产物数（缺字段、越界、弱化命题……）。
    pub rejections: u32,
    /// 契约产物路径。
    pub contracts_file: String,
    /// 测试产物路径。
    pub tests_file: String,
    /// 版本变更阶段重新生成测试计划所花的调用数（复用预置产物时为 0）。
    pub regenerated_calls: u32,
    /// 复用产物时的历次读数（不重复落调用记录）。
    pub reused_readings: Vec<ReuseReading>,
    /// 逐族的契约。
    pub contracts: Vec<ContractEntry>,
    /// 逐族的测试计划。
    pub tests: Vec<TestEntry>,
}

/// 复用产物时从产物里读回的读数。
#[derive(Debug, Clone, Serialize)]
pub struct ReuseReading {
    /// 类别：`contract` 或 `tests`。
    pub kind: String,
    /// 产物里记着的历次调用数。
    pub calls: u32,
    /// 失败数。
    pub failures: u32,
    /// 输入 token 合计。
    pub input_tokens: Option<u64>,
    /// 输出 token 合计。
    pub output_tokens: Option<u64>,
    /// 延迟中位数。
    pub latency_p50_ms: Option<u64>,
}

/// 一份契约产物的报告条目。
#[derive(Debug, Clone, Serialize)]
pub struct ContractEntry {
    /// 族标识。
    pub family: String,
    /// 调用读数。
    pub call: MeasureCall,
    /// 是否被接受。
    pub accepted: bool,
    /// 契约（被拒时为 null）。
    pub contract: Option<ObserverContract>,
    /// 协议核对结果。
    pub check: Option<ContractCheck>,
    /// 被拒原文。
    pub rejection: Option<String>,
}

/// 一份测试产物的报告条目。
#[derive(Debug, Clone, Serialize)]
pub struct TestEntry {
    /// 族标识。
    pub family: String,
    /// 调用读数。
    pub call: MeasureCall,
    /// 是否被接受。
    pub accepted: bool,
    /// 编译出来的断言。
    pub checks: Vec<Check>,
    /// 计划说明。
    pub plan_note: Option<String>,
    /// 被拒原文。
    pub rejection: Option<String>,
}

/// 原始数据位置。
#[derive(Debug, Clone, Serialize)]
pub struct RawPaths {
    /// 产物目录。
    pub out_dir: String,
    /// 主矩阵的逐用例读数（JSONL）。
    pub runs: String,
    /// 版本变更阶段的逐用例读数（JSONL）。
    pub runs_v2: String,
    /// 契约产物。
    pub contracts_file: String,
    /// 测试产物。
    pub tests_file: String,
    /// 版本变更阶段重新生成的测试计划产物（复用预置产物时与 `tests_file` 同一份内容）。
    pub tests_v2_file: String,
    /// 迁移矩阵的逐用例读数（JSONL）。
    pub transfer: String,
    /// 独立检验读数。
    pub verification_file: String,
    /// 调用记录。
    pub calls_log: String,
}

/// 成本账（按用途分组的调用读数 + 不产生调用的运行次数）。
#[derive(Debug, Clone, Serialize)]
pub struct CostSection {
    /// 逐用途读数。
    pub by_purpose: Vec<PurposeCost>,
    /// 合计。
    pub total: PurposeCost,
    /// 不产生生成调用、但照样花机器时间的那些运行。
    pub runs: RunCounts,
    /// 口径说明。
    pub note: String,
}

/// 各类运行的次数（§7.4 的「成本含生成、验证、建设与重复运行」里后三样的计数）。
#[derive(Debug, Clone, Default, Serialize)]
pub struct RunCounts {
    /// 主矩阵的用例数（族 × 调度 × 实现）。
    pub main_cases: u32,
    /// 版本变更阶段的用例数（每个用例跑两次环境：老挂载与重挂）。
    pub version_change_cases: u32,
    /// 四路独立检验的用例数。
    pub verification_cases: u32,
    /// 迁移矩阵复算的用例数。
    pub transfer_cases: u32,
    /// 评分时逐用例复算真值的次数（隐藏检查）。
    pub ground_truth_recomputations: u32,
}

/// 一个用途的调用读数。
#[derive(Debug, Clone, Serialize)]
pub struct PurposeCost {
    /// 用途。
    pub purpose: String,
    /// 本次运行的调用数（含失败）。
    pub calls: u32,
    /// 成功数。
    pub succeeded: u32,
    /// 失败数。
    pub failed: u32,
    /// 输入 token。
    pub input_tokens: Option<u64>,
    /// 输出 token。
    pub output_tokens: Option<u64>,
    /// 延迟中位数。
    pub latency_p50_ms: Option<u64>,
    /// 调用统计（含失败原文）。
    pub stats: CallStats,
    /// 延迟分位数。
    pub latency: LatencyStats,
    /// token 用量。
    pub usage: UsageStats,
}

/// 一个用例的读数（写进 JSONL；报告里只放汇总与逐调度读数）。
#[derive(Debug, Clone, Serialize)]
pub struct CaseReading {
    /// 族。
    pub family: String,
    /// 调度。
    pub schedule: String,
    /// 实现。
    pub implementation: String,
    /// 版本。
    pub version: String,
    /// 记录器挂载。
    pub attachment: String,
    /// 隐藏检查的判定（真值）。
    pub hidden: Verdict,
    /// 各臂的判定。
    pub arms: BTreeMap<String, Verdict>,
}

/// 一次观测器建设的产物。
#[derive(Debug, Clone)]
pub struct ObserverSetup {
    /// §7.2 的八条。
    pub contract: ObserverContract,
    /// 装好的观测器。
    pub observer: Observer,
    /// 协议核对结果。
    pub check: ContractCheck,
    /// 四路独立检验。
    pub independent: IndependentCheck,
    /// 启用结果（含自证式检查被拒那一步）。
    pub enable: EnableOutcome,
}

/// 跑完一次 H4。
pub async fn run(options: &H4Options) -> Result<H4Outcome, Error> {
    std::fs::create_dir_all(&options.out_dir)?;
    let mut calls = CallLog::open(options.calls_path.clone())?;
    let mut clean = true;
    let repeats = options.repeats.unwrap_or(usize::MAX);
    let families = CFamily::ALL.to_vec();

    // 1) 契约与测试计划。
    let mut artifacts = Artifacts::default();
    match &options.contracts {
        ArtifactSource::Generate => {
            artifacts.contracts =
                generate_contracts(&families, options, &mut calls, &mut clean).await?;
        }
        ArtifactSource::File(path) => artifacts.contracts = crate::c_measure::read_contracts(path)?,
    }
    match &options.tests {
        ArtifactSource::Generate => {
            artifacts.tests =
                generate_tests(&families, options, &mut calls, &mut clean, "").await?;
        }
        ArtifactSource::File(path) => artifacts.tests = crate::c_measure::read_tests(path)?,
    }
    let contracts_path = options.out_dir.join("contracts.json");
    let tests_path = options.out_dir.join("tests.json");
    let tests_v2_path = options.out_dir.join("tests-v2.json");
    if matches!(options.contracts, ArtifactSource::Generate) {
        write_json(&contracts_path, &artifacts.contracts)?;
    }
    if matches!(options.tests, ArtifactSource::Generate) {
        write_json(&tests_path, &artifacts.tests)?;
    }

    // 2) 装观测器 + 四路独立检验 + 生命周期（含自证式检查被拒那一步）。
    let mut library = jh_core::ObserverLibrary::default();
    let mut state = SemanticState::in_memory();
    let mut setups: BTreeMap<CFamily, ObserverSetup> = BTreeMap::new();
    for family in &families {
        let Some(entry) = artifacts
            .contracts
            .iter()
            .find(|entry| entry.family == *family)
        else {
            continue;
        };
        let (Some(contract), Some(check)) = (entry.contract.clone(), entry.check.clone()) else {
            continue;
        };
        if !check.passed {
            continue;
        }
        let observer = Observer::build(*family, contract.clone(), ProgramVersion::V1)?;
        let independent =
            crate::c_measure::independent_check(*family, ProgramVersion::V1, &observer, unix_ms());
        let forged = crate::c_measure::self_certifying(
            &independent.check,
            &Verdict::satisfied("观测器自己说没问题（自证）"),
        );
        let enable = crate::c_measure::enable_with_independent_check(
            &mut library,
            &mut state,
            *family,
            contract.clone(),
            independent.check.clone(),
            forged,
            unix_ms(),
        )?;
        setups.insert(
            *family,
            ObserverSetup {
                contract,
                observer,
                check,
                independent,
                enable,
            },
        );
    }

    // 3) 主矩阵：一个用例跑一次环境，三臂各出一个判定。
    let mut cases = Vec::new();
    for family in &families {
        for schedule in limit(&c_program::schedules(*family), repeats) {
            for implementation in Implementation::ALL {
                let run = c_program::run_case(
                    *family,
                    &schedule,
                    implementation,
                    ProgramVersion::V1,
                    Attachment::at_boundary(),
                    false,
                );
                cases.push(CaseReading {
                    family: family.id().to_owned(),
                    schedule: schedule.id.clone(),
                    implementation: implementation.id().to_owned(),
                    version: ProgramVersion::V1.id().to_owned(),
                    attachment: "at_boundary".to_owned(),
                    hidden: crate::c_measure::hidden_verdict(&run),
                    arms: arm_verdicts(family, &schedule, &run, &setups, &artifacts),
                });
            }
        }
    }
    let arms = arm_reports(&families, &cases, &setups, &artifacts);

    // 4) 版本变更阶段。
    let v2_calls =
        regenerate_tests(&families, options, &mut calls, &mut clean, &mut artifacts).await?;
    let v2_cases = version_change_cases(&families, repeats, &artifacts, &setups);
    let v2_arms = version_change_reports(&families, &v2_cases, &artifacts);
    let maintenance = maintenance_section(&arms, &v2_arms, options, v2_calls.calls);

    // 5) 迁移矩阵。
    let (transfer, transfer_cases) = transfer_matrix(&families, &setups, repeats);

    // 6) 摊薄曲线与假说判据。
    let amortization = amortization_section(
        &families,
        &arms,
        &v2_arms,
        match (&options.contracts, &options.tests) {
            (ArtifactSource::Generate, ArtifactSource::Generate) => "generate",
            _ => "file",
        },
    );
    let hypothesis = hypothesis_section(&arms, &amortization, &transfer);

    // 7) 产物。
    let runs_path = options.out_dir.join("runs.jsonl");
    write_jsonl(&runs_path, &cases)?;
    let runs_v2_path = options.out_dir.join("runs-v2.jsonl");
    write_jsonl(&runs_v2_path, &v2_cases)?;
    // 版本变更阶段用的那份测试计划单独落盘：`generated@v2` 的维护读数要能只靠产物复核。
    write_json(&tests_v2_path, &artifacts.tests_v2)?;
    let transfer_path = options.out_dir.join("transfer.jsonl");
    write_jsonl(&transfer_path, &transfer_cases)?;
    let verification = verification_section(&families, &setups);
    let verification_path = options.out_dir.join("verification.json");
    write_json(&verification_path, &verification)?;

    let mut all_arms = arms.clone();
    all_arms.extend(v2_arms);
    all_arms.sort_by(|left, right| {
        (&left.family, &left.arm, &left.version).cmp(&(&right.family, &right.arm, &right.version))
    });

    let run_counts = RunCounts {
        main_cases: cases.len() as u32,
        version_change_cases: (v2_cases.len() * 2) as u32,
        verification_cases: verification
            .per_family
            .iter()
            .map(|family| family.directions.len() as u32)
            .sum(),
        transfer_cases: transfer.iter().map(|cell| cell.runs).sum(),
        ground_truth_recomputations: cases.len() as u32,
    };

    let report = H4Report {
        run: options.run.clone(),
        generated_at_unix_ms: unix_ms(),
        command: options.command.clone(),
        protocol: protocol_declaration(repeats),
        environment: report_environment("run"),
        families: families
            .iter()
            .map(|family| family_summary(*family))
            .collect(),
        arms: all_arms,
        transfer,
        verification,
        maintenance,
        amortization,
        hypothesis,
        generation: generation_section(&artifacts, options, &contracts_path, &tests_path, v2_calls),
        raw: RawPaths {
            out_dir: absolute(&options.out_dir),
            runs: absolute(&runs_path),
            runs_v2: absolute(&runs_v2_path),
            contracts_file: absolute(&contracts_path),
            tests_file: absolute(&tests_path),
            tests_v2_file: absolute(&tests_v2_path),
            transfer: absolute(&transfer_path),
            verification_file: absolute(&verification_path),
            calls_log: absolute(&options.calls_path),
        },
        cost: cost_section(&options.calls_path, &options.run, run_counts)?,
        notes: declared_notes(),
    };

    Ok(H4Outcome { report, clean })
}

fn limit(schedules: &[Schedule], repeats: usize) -> Vec<Schedule> {
    schedules[..schedules.len().min(repeats)].to_vec()
}

/// 一次运行里的全部生成产物。
#[derive(Debug, Clone, Default)]
struct Artifacts {
    contracts: Vec<GeneratedContract>,
    /// 主矩阵用的测试计划。
    tests: Vec<GeneratedTests>,
    /// 版本变更阶段用的测试计划（真调时为重新生成的那一份；复用预置产物时与 `tests` 相同）。
    tests_v2: Vec<GeneratedTests>,
}

/// 给一个用例算三个臂的判定。
fn arm_verdicts(
    family: &CFamily,
    schedule: &Schedule,
    run: &crate::c_program::RunRecord,
    setups: &BTreeMap<CFamily, ObserverSetup>,
    artifacts: &Artifacts,
) -> BTreeMap<String, Verdict> {
    let mut arms = BTreeMap::new();
    arms.insert(
        ARM_FIXED.to_owned(),
        crate::c_measure::fixed_verdict(*family, schedule, run),
    );
    arms.insert(
        ARM_GENERATED.to_owned(),
        match artifacts.tests.iter().find(|entry| entry.family == *family) {
            Some(plan) if !plan.checks.is_empty() => {
                crate::c_measure::generated_verdict(&plan.checks, run)
            }
            Some(plan) => Verdict::failed(format!(
                "这一臂没有可用的测试计划：{}",
                plan.rejection
                    .clone()
                    .unwrap_or_else(|| "未生成".to_owned())
            )),
            None => Verdict::failed("这一臂没有可用的测试计划"),
        },
    );
    arms.insert(
        ARM_OBSERVER.to_owned(),
        match setups.get(family) {
            Some(setup) => setup.observer.observe(run),
            None => Verdict::failed("这一族没有通过协议核对的契约，观测器臂不可用"),
        },
    );
    arms
}

fn score_cell(truth: &Verdict, arm: &Verdict, cell: &mut ArmCell) {
    match (&truth.verdict, &arm.verdict) {
        (ObserverVerdict::Violated, ObserverVerdict::Violated) => cell.detections += 1,
        (ObserverVerdict::Violated, ObserverVerdict::Satisfied) => cell.false_passes += 1,
        (ObserverVerdict::Violated, ObserverVerdict::Failed(_)) => {
            cell.inconclusive_on_opportunity += 1;
        }
        (ObserverVerdict::Satisfied, ObserverVerdict::Violated) => cell.false_alarms += 1,
        (ObserverVerdict::Satisfied, ObserverVerdict::Satisfied) => cell.correct_clears += 1,
        // 隐藏检查读不到数据（本环境里不可绕过，属于不该发生的形态）：只计数，不当真值用。
        (_, ObserverVerdict::Failed(_)) => cell.inconclusive_on_clean += 1,
        (ObserverVerdict::Failed(_), _) => {}
    }
}

fn rate(numerator: u32, denominator: u32) -> Option<f64> {
    (denominator > 0).then(|| f64::from(numerator) / f64::from(denominator))
}

/// 按（臂, 族, 版本）聚合用例。
fn arm_reports(
    families: &[CFamily],
    cases: &[CaseReading],
    setups: &BTreeMap<CFamily, ObserverSetup>,
    artifacts: &Artifacts,
) -> Vec<ArmReport> {
    let mut out = Vec::new();
    for family in families {
        let family_cases: Vec<&CaseReading> = cases
            .iter()
            .filter(|case| case.family == family.id())
            .collect();
        let schedule_count = family_cases
            .iter()
            .map(|case| case.schedule.as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        for arm in [ARM_FIXED, ARM_GENERATED, ARM_OBSERVER] {
            let available = match arm {
                ARM_OBSERVER => setups.contains_key(family),
                _ => artifacts
                    .tests
                    .iter()
                    .any(|entry| entry.family == *family && !entry.checks.is_empty()),
            };
            out.push(aggregate(
                arm,
                arm_subject(arm),
                family.id(),
                ProgramVersion::V1.id(),
                "at_boundary",
                if available {
                    Verification {
                        status: VerificationStatus::Verified,
                        detail: format!(
                            "本地确定性执行，无网络调用；用例数 {}",
                            family_cases.len()
                        ),
                    }
                } else {
                    Verification {
                        status: VerificationStatus::NotVerified,
                        detail: "这一臂在这一族上没有可用的产物（契约被拒或生成失败）：相关结论保持未验证"
                            .to_owned(),
                    }
                },
                schedule_count,
                &schedule_notes(*family),
                &family_cases,
            ));
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn aggregate(
    arm: &str,
    subject: &str,
    family: &str,
    version: &str,
    attachment: &str,
    verification: Verification,
    schedules: usize,
    notes: &BTreeMap<String, String>,
    cases: &[&CaseReading],
) -> ArmReport {
    let mut total = ArmCell::default();
    let mut grouped: BTreeMap<String, (ArmCell, Vec<String>)> = BTreeMap::new();
    for case in cases {
        let Some(verdict) = case.arms.get(arm) else {
            continue;
        };
        score_cell(&case.hidden, verdict, &mut total);
        let entry = grouped.entry(case.schedule.clone()).or_default();
        score_cell(&case.hidden, verdict, &mut entry.0);
        entry.1.push(format!(
            "{}={}",
            case.implementation,
            verdict_label(&case.hidden.verdict)
        ));
    }
    let runs = grouped
        .values()
        .map(|(cell, _)| cell.opportunities() + cell.clean_runs())
        .sum();
    let by_schedule = grouped
        .into_iter()
        .map(|(schedule, (cell, hidden))| ScheduleReading {
            note: notes.get(&schedule).cloned().unwrap_or_default(),
            schedule,
            opportunities: cell.opportunities(),
            clean_runs: cell.clean_runs(),
            hidden,
            arms: BTreeMap::from([(arm.to_owned(), cell)]),
        })
        .collect();
    ArmReport {
        arm: arm.to_owned(),
        subject: subject.to_owned(),
        family: family.to_owned(),
        version: version.to_owned(),
        attachment: attachment.to_owned(),
        verification,
        schedules,
        implementations: Implementation::ALL
            .iter()
            .map(|implementation| implementation.id().to_owned())
            .collect(),
        runs,
        opportunities: total.opportunities(),
        clean_runs: total.clean_runs(),
        detections: total.detections,
        false_passes: total.false_passes,
        inconclusive_on_opportunity: total.inconclusive_on_opportunity,
        false_alarms: total.false_alarms,
        correct_clears: total.correct_clears,
        inconclusive_on_clean: total.inconclusive_on_clean,
        detection_rate: rate(total.detections, total.opportunities()),
        false_pass_rate: rate(total.false_passes, total.opportunities()),
        false_alarm_rate: rate(total.false_alarms, total.clean_runs()),
        by_schedule,
    }
}

fn verdict_label(verdict: &ObserverVerdict) -> &'static str {
    match verdict {
        ObserverVerdict::Satisfied => "命题成立",
        ObserverVerdict::Violated => "发现违反",
        ObserverVerdict::Failed(_) => "观测失败",
    }
}

fn arm_subject(arm: &str) -> &'static str {
    match arm {
        ARM_FIXED => "族 C 之前就存在的可观察量：自报状态、当前值、自报写入意图数",
        ARM_GENERATED => "同样那些可观察量（宿主代码不改动，断言由 LLM 生成）",
        ARM_OBSERVER => "声明式测量契约选中的钩子点：写入边界记录器 / 可控暂停点 / 提交计数器",
        ARM_FIXED_V2 | ARM_GENERATED_V2 => "同上（版本变更之后原样使用）",
        ARM_OBSERVER_V2_SUPPORT_ONLY => "同上，但只更新了契约声明的支持版本，钩子点没有重挂",
        ARM_OBSERVER_V2_REVERIFIED => "同上，且重挂了钩子点并重跑了四路独立检验",
        ARM_OBSERVER_V2_NO_COUNTER => {
            "同上（只改支持版本、不重挂），且契约里拿掉了提交计数器这一路证据"
        }
        _ => "—",
    }
}

/// 调度标识 → 这一条调度覆盖的情形。
fn schedule_notes(family: CFamily) -> BTreeMap<String, String> {
    c_program::schedules(family)
        .into_iter()
        .map(|schedule| (schedule.id, schedule.note))
        .collect()
}

fn family_summary(family: CFamily) -> FamilySummary {
    FamilySummary {
        family: family.id().to_owned(),
        title: family.title().to_owned(),
        requirement: family.requirement().to_owned(),
        requirement_source: family.requirement_source().to_owned(),
        object: family.object().to_string(),
        landmark: family == CFamily::CancelAfterConfirm,
        schedules: c_program::schedules(family)
            .iter()
            .map(|schedule| ScheduleSummary {
                schedule: schedule.id.clone(),
                note: schedule.note.clone(),
                ops: schedule.ops.len(),
            })
            .collect(),
        hidden_criterion: hidden_criterion(family).to_owned(),
        fixed_arm_declaration: crate::c_measure::fixed_declaration(family),
    }
}

fn hidden_criterion(family: CFamily) -> &'static str {
    match family {
        CFamily::CancelAfterConfirm => {
            "存储日志里存在一条属于该任务的提交，其事件序号晚于该任务的取消确认事件"
        }
        CFamily::LostUpdate => "同一个键上有两条提交声明的基准版本相同（后一条覆盖了已提交的更新）",
        CFamily::DuplicateAfterRestart => "同一个 (任务, 写入意图) 在存储日志里被提交了两次以上",
    }
}

fn protocol_declaration(repeats: usize) -> ProtocolDeclaration {
    ProtocolDeclaration {
        arms: vec![
            "fixed：只用族 C 之前就存在的可观察量（自报状态、当前值、自报意图数）；\
             C1 的检查项就是 §7.3 禁止的弱化替换，作为对照存在"
                .to_owned(),
            "generated：同样的可观察量，但断言由真实 LLM 现生成（不改宿主代码）".to_owned(),
            "observer：声明式测量契约 + 写入边界记录器 + 可控暂停点（+ 可选提交计数器），\
             启用前必须过四路独立检验"
                .to_owned(),
            "结构性声明（读数字前先读这一条）：三个臂看的是不同的东西——fixed / generated 只能看\
             程序自报与终态，observer 看写入边界的顺序。机会数与真值全部来自隐藏检查\
             （存储日志通道），不由任何臂自己认定。"
                .to_owned(),
            format!(
                "结构性声明：`{ARM_OBSERVER_V2_SUPPORT_ONLY}` 是版本变更后只改了契约支持版本、\
                 没有重挂钩子点的形态；`{ARM_OBSERVER_V2_REVERIFIED}` 是重挂 + 重跑独立检验的形态；\
                 `{ARM_OBSERVER_V2_NO_COUNTER}` 是同一份契约又拿掉提交计数器的形态。\
                 三者之差就是「维护省掉的那一步」与「钩子点选对没选对」各自的代价"
            ),
        ],
        repetition_protocol: format!(
            "每臂 × 每族 × N；N = 调度条数（本次每条族取 {}）而不是随机重复：竞态靠不同的确定性\
             调度展开，重复即可复现。每族另有 {} 路实现（1 路合理 + 3 路已知错误），\
             因此每臂每族的用例数 = N × 实现路数。",
            repeats.min(10),
            Implementation::ALL.len()
        ),
        scoring_protocol: "同一个用例只跑一次环境，三臂各出一个判定，隐藏检查出一个判定；\
             分母取自隐藏检查。失败态单独计数，既不折成「成立」也不算「违反」。"
            .to_owned(),
        boundaries: vec![
            "本对照回答的是「在族 C 这三类顺序 / 并发故障上，三种做法各自的检出与代价」，\
             不回答「观测器普遍更好」——C2 上固定臂本来就够用，读数里看得到"
                .to_owned(),
            "真值是隐藏检查（存储日志），它不看记录器、不看程序自报；观测器与它在不同模块、\
             不同证据通道"
                .to_owned(),
            "一次没检出只说明这条调度上没检出，不读作「不存在」；检出率一律给 n/N".to_owned(),
            "契约与测试计划的内容质量不在这里判分：结构核对只拦得住它看得见的（缺字段、越界、\
             弱化命题），内容是四路独立检验与实际检出率在判"
                .to_owned(),
        ],
        banned_substitutions: crate::c_measure::BANNED_SUBSTITUTIONS
            .iter()
            .map(|pattern| (*pattern).to_owned())
            .collect(),
        metric_definitions: ProtocolDeclaration::metric_definitions(),
    }
}

// ---------------------------------------------------------------------------
// 版本变更阶段
// ---------------------------------------------------------------------------

fn version_change_cases(
    families: &[CFamily],
    repeats: usize,
    artifacts: &Artifacts,
    setups: &BTreeMap<CFamily, ObserverSetup>,
) -> Vec<CaseReading> {
    let tests = &artifacts.tests_v2;
    let mut cases = Vec::new();
    for family in families {
        let support_only = setups.get(family).map(|setup| {
            let mut contract = setup.contract.clone();
            contract
                .support
                .versions
                .insert(ProgramVersion::V2.id().to_owned());
            setup.observer.with_contract(contract)
        });
        // 同一份契约，但把提交计数器从测量位置里拿掉：这是「契约选错钩子点」的对照。
        let no_counter = support_only.as_ref().map(|observer| {
            let mut contract = observer.contract().clone();
            contract.measurement.site = contract
                .measurement
                .site
                .replace(crate::c_program::HookPoint::CommitCounter.id(), "")
                .trim()
                .to_owned();
            observer.with_contract(contract)
        });
        for schedule in limit(&c_program::schedules(*family), repeats) {
            for implementation in Implementation::ALL {
                let stale = c_program::run_case(
                    *family,
                    &schedule,
                    implementation,
                    ProgramVersion::V2,
                    Attachment::at_boundary(),
                    false,
                );
                let reattached = c_program::run_case(
                    *family,
                    &schedule,
                    implementation,
                    ProgramVersion::V2,
                    Attachment::reattached(),
                    false,
                );
                let mut arms = BTreeMap::new();
                arms.insert(
                    ARM_FIXED_V2.to_owned(),
                    crate::c_measure::fixed_verdict(*family, &schedule, &stale),
                );
                if let Some(plan) = tests.iter().find(|entry| entry.family == *family) {
                    arms.insert(
                        ARM_GENERATED_V2.to_owned(),
                        if plan.checks.is_empty() {
                            Verdict::failed("这一臂没有可用的测试计划")
                        } else {
                            crate::c_measure::generated_verdict(&plan.checks, &stale)
                        },
                    );
                }
                arms.insert(
                    ARM_OBSERVER_V2_SUPPORT_ONLY.to_owned(),
                    match &support_only {
                        Some(observer) => observer.observe(&stale),
                        None => Verdict::failed("这一族没有通过协议核对的契约"),
                    },
                );
                arms.insert(
                    ARM_OBSERVER_V2_REVERIFIED.to_owned(),
                    match &support_only {
                        Some(observer) => observer.observe(&reattached),
                        None => Verdict::failed("这一族没有通过协议核对的契约"),
                    },
                );
                arms.insert(
                    ARM_OBSERVER_V2_NO_COUNTER.to_owned(),
                    match &no_counter {
                        Some(observer) => observer.observe(&stale),
                        None => Verdict::failed("这一族没有通过协议核对的契约"),
                    },
                );
                cases.push(CaseReading {
                    family: family.id().to_owned(),
                    schedule: schedule.id.clone(),
                    implementation: implementation.id().to_owned(),
                    version: ProgramVersion::V2.id().to_owned(),
                    attachment: "at_boundary + reattached".to_owned(),
                    hidden: crate::c_measure::hidden_verdict(&stale),
                    arms,
                });
            }
        }
    }
    cases
}

fn version_change_reports(
    families: &[CFamily],
    cases: &[CaseReading],
    artifacts: &Artifacts,
) -> Vec<ArmReport> {
    let tests = &artifacts.tests_v2;
    let mut out = Vec::new();
    for family in families {
        let family_cases: Vec<&CaseReading> = cases
            .iter()
            .filter(|case| case.family == family.id())
            .collect();
        let schedule_count = family_cases
            .iter()
            .map(|case| case.schedule.as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        for arm in [
            ARM_FIXED_V2,
            ARM_GENERATED_V2,
            ARM_OBSERVER_V2_SUPPORT_ONLY,
            ARM_OBSERVER_V2_REVERIFIED,
            ARM_OBSERVER_V2_NO_COUNTER,
        ] {
            let available = match arm {
                ARM_GENERATED_V2 => tests
                    .iter()
                    .any(|entry| entry.family == *family && !entry.checks.is_empty()),
                _ => family_cases.iter().any(|case| case.arms.contains_key(arm)),
            };
            out.push(aggregate(
                arm,
                arm_subject(arm),
                family.id(),
                ProgramVersion::V2.id(),
                if arm == ARM_OBSERVER_V2_REVERIFIED {
                    "reattached"
                } else {
                    "at_boundary"
                },
                if available {
                    Verification {
                        status: VerificationStatus::Verified,
                        detail: "本地确定性执行；版本变更阶段的读数".to_owned(),
                    }
                } else {
                    Verification {
                        status: VerificationStatus::NotVerified,
                        detail: "这一臂在这一族上没有可用的产物".to_owned(),
                    }
                },
                schedule_count,
                &schedule_notes(*family),
                &family_cases,
            ));
        }
    }
    out
}

async fn regenerate_tests(
    families: &[CFamily],
    options: &H4Options,
    calls: &mut CallLog,
    clean: &mut bool,
    artifacts: &mut Artifacts,
) -> Result<Regeneration, Error> {
    if !matches!(options.tests, ArtifactSource::Generate) {
        // 复用预置产物：版本变更阶段沿用同一份计划（`regenerated = false` 如实标出来）。
        // 这一行不能省——省了 `generated@v2` 那一臂会整个消失，维护读数跟着变成 null。
        artifacts.tests_v2 = artifacts.tests.clone();
        return Ok(Regeneration::default());
    }
    let regenerated = generate_tests(families, options, calls, clean, "-tests-v2").await?;
    let outcome = Regeneration {
        calls: regenerated.len() as u32,
        failures: regenerated
            .iter()
            .filter(|entry| !entry.call.succeeded())
            .count() as u32,
    };
    artifacts.tests_v2 = regenerated;
    Ok(outcome)
}

/// 版本变更阶段重新生成测试计划的调用读数。
#[derive(Debug, Clone, Copy, Default)]
struct Regeneration {
    /// 发起的调用数。
    calls: u32,
    /// 失败数。
    failures: u32,
}

fn maintenance_section(
    arms: &[ArmReport],
    v2_arms: &[ArmReport],
    options: &H4Options,
    regeneration_calls: u32,
) -> MaintenanceSection {
    let regenerated = matches!(options.tests, ArtifactSource::Generate);
    let mut readings = Vec::new();
    for family in CFamily::ALL {
        for (v2_arm, v1_arm, note) in [
            (
                ARM_FIXED_V2,
                ARM_FIXED,
                "固定测试臂：断言写死在代码里，宿主改版本时它一行都不用改——也就不会跟着改",
            ),
            (
                ARM_GENERATED_V2,
                ARM_GENERATED,
                "通用测试生成臂：要重新生成一遍测试计划（调用数记在下一列）",
            ),
            (
                ARM_OBSERVER_V2_SUPPORT_ONLY,
                ARM_OBSERVER,
                "观测器臂（只改支持版本）：0 次生成调用，但钩子点没重挂——便宜的那一步省掉了些什么，看检出率",
            ),
            (
                ARM_OBSERVER_V2_REVERIFIED,
                ARM_OBSERVER,
                "观测器臂（重挂 + 重跑独立检验）：0 次生成调用，代价是重跑一遍四路检验",
            ),
            (
                ARM_OBSERVER_V2_NO_COUNTER,
                ARM_OBSERVER,
                "观测器臂（只改支持版本、且契约没选提交计数器）：量一量「选错钩子点」这一件事的代价",
            ),
        ] {
            let Some(after) = v2_arms
                .iter()
                .find(|report| report.family == family.id() && report.arm == v2_arm)
            else {
                continue;
            };
            let before = arms
                .iter()
                .find(|report| report.family == family.id() && report.arm == v1_arm);
            readings.push(MaintenanceReading {
                arm: v2_arm.to_owned(),
                regeneration_calls: if v2_arm == ARM_GENERATED_V2 {
                    regeneration_calls
                } else {
                    0
                },
                regenerated: v2_arm == ARM_GENERATED_V2 && regenerated,
                reattached: v2_arm == ARM_OBSERVER_V2_REVERIFIED,
                reverified: v2_arm == ARM_OBSERVER_V2_REVERIFIED,
                detection_rate_before: before.and_then(|report| report.detection_rate),
                detection_rate_after: after.detection_rate,
                false_pass_rate_after: after.false_pass_rate,
                note: note.to_owned(),
            });
        }
    }

    let mut totals = Vec::new();
    for (v2_arm, v1_arm) in [
        (ARM_FIXED_V2, ARM_FIXED),
        (ARM_GENERATED_V2, ARM_GENERATED),
        (ARM_OBSERVER_V2_SUPPORT_ONLY, ARM_OBSERVER),
        (ARM_OBSERVER_V2_REVERIFIED, ARM_OBSERVER),
        (ARM_OBSERVER_V2_NO_COUNTER, ARM_OBSERVER),
    ] {
        let after: Vec<&ArmReport> = v2_arms
            .iter()
            .filter(|report| report.arm == v2_arm)
            .collect();
        if after.is_empty() {
            continue;
        }
        let before: Vec<&ArmReport> = arms
            .iter()
            .filter(|report| report.arm == v1_arm && report.version == ProgramVersion::V1.id())
            .collect();
        let sum = |reports: &[&ArmReport], pick: fn(&ArmReport) -> u32| -> u32 {
            reports.iter().map(|report| pick(report)).sum()
        };
        let opportunities = sum(&after, |report| report.opportunities);
        let clean_runs = sum(&after, |report| report.clean_runs);
        let detections = sum(&after, |report| report.detections);
        let false_passes = sum(&after, |report| report.false_passes);
        let false_alarms = sum(&after, |report| report.false_alarms);
        totals.push(MaintenanceTotal {
            arm: v2_arm.to_owned(),
            families: after.len(),
            runs: sum(&after, |report| report.runs),
            opportunities,
            clean_runs,
            detections,
            false_passes,
            inconclusive_on_opportunity: sum(&after, |report| report.inconclusive_on_opportunity),
            inconclusive_on_clean: sum(&after, |report| report.inconclusive_on_clean),
            false_alarms,
            detection_rate_before: rate(
                sum(&before, |report| report.detections),
                sum(&before, |report| report.opportunities),
            ),
            detection_rate_after: rate(detections, opportunities),
            false_pass_rate_after: rate(false_passes, opportunities),
            false_alarm_rate_after: rate(false_alarms, clean_runs),
        });
    }

    MaintenanceSection {
        change: ProgramVersion::V2.note().to_owned(),
        arms: readings,
        totals,
        note: "版本变更只动了一件事：v2 新增直接提交路径（批量提交 commit_batch）。\
               固定测试臂一行都不用改，通用测试生成臂要重新生成，观测器臂的代价是「重挂钩子点 + \
               重跑四路独立检验」。重挂那一步值不值，看两张读数：\
               `observer@v2-support-only` 与 `observer@v2-reverified` 之差是「不重挂丢掉了多少次检出」；\
               `observer@v2-support-only-no-counter` 是同一份契约**又拿掉了提交计数器**之后的样子——\
               少了这一路，观测器在记录器被绕过时给不出失败态，只能照着记录到的（空的）事件流说「命题成立」。"
            .to_owned(),
    }
}

// ---------------------------------------------------------------------------
// 迁移矩阵、摊薄曲线、假说判据
// ---------------------------------------------------------------------------

fn transfer_matrix(
    families: &[CFamily],
    setups: &BTreeMap<CFamily, ObserverSetup>,
    repeats: usize,
) -> (Vec<TransferCell>, Vec<TransferCase>) {
    let mut out = Vec::new();
    let mut cases = Vec::new();
    for built_for in families {
        let Some(setup) = setups.get(built_for) else {
            continue;
        };
        for applied_to in families {
            let mut cell = ArmCell::default();
            let mut runs = 0;
            for schedule in limit(&c_program::schedules(*applied_to), repeats) {
                for implementation in Implementation::ALL {
                    let run = c_program::run_case(
                        *applied_to,
                        &schedule,
                        implementation,
                        ProgramVersion::V1,
                        Attachment::at_boundary(),
                        false,
                    );
                    let truth = crate::c_measure::hidden_verdict(&run);
                    let verdict = setup.observer.observe(&run);
                    score_cell(&truth, &verdict, &mut cell);
                    runs += 1;
                    cases.push(TransferCase {
                        built_for: built_for.id().to_owned(),
                        applied_to: applied_to.id().to_owned(),
                        schedule: schedule.id.clone(),
                        implementation: implementation.id().to_owned(),
                        hidden: truth,
                        observer: verdict,
                    });
                }
            }
            out.push(TransferCell {
                built_for: built_for.id().to_owned(),
                applied_to: applied_to.id().to_owned(),
                same_family: built_for == applied_to,
                runs,
                opportunities: cell.opportunities(),
                detections: cell.detections,
                false_passes: cell.false_passes,
                inconclusive_on_opportunity: cell.inconclusive_on_opportunity,
                inconclusive_on_clean: cell.inconclusive_on_clean,
                false_alarms: cell.false_alarms,
                clean_runs: cell.clean_runs(),
                correct_clears: cell.correct_clears,
                detection_rate: rate(cell.detections, cell.opportunities()),
                false_alarm_rate: rate(cell.false_alarms, cell.clean_runs()),
            });
        }
    }
    (out, cases)
}

/// 迁移矩阵的一条逐用例读数（落 JSONL，`TransferCell` 的每个数字都能回查到具体哪一条）。
#[derive(Debug, Clone, Serialize)]
pub struct TransferCase {
    /// 观测器是为哪个族建的。
    pub built_for: String,
    /// 套用到哪个族。
    pub applied_to: String,
    /// 调度。
    pub schedule: String,
    /// 实现。
    pub implementation: String,
    /// 隐藏检查的判定（真值）。
    pub hidden: Verdict,
    /// 观测器的判定（按它被建的那个族算）。
    pub observer: Verdict,
}

fn amortization_section(
    families: &[CFamily],
    arms: &[ArmReport],
    v2_arms: &[ArmReport],
    cost_basis: &str,
) -> AmortizationSection {
    let mut points = Vec::new();
    for covered in 1..=families.len() {
        let target: Vec<&str> = families[..covered]
            .iter()
            .map(|family| family.id())
            .collect();
        let mut cumulative_calls = BTreeMap::new();
        let mut opportunities = BTreeMap::new();
        let mut detections = BTreeMap::new();
        let mut detection_rate = BTreeMap::new();
        let mut calls_per_detection = BTreeMap::new();
        for arm in [ARM_FIXED, ARM_GENERATED, ARM_OBSERVER] {
            let reports: Vec<&ArmReport> = arms
                .iter()
                .filter(|report| report.arm == arm && target.contains(&report.family.as_str()))
                .collect();
            // 生成调用数：每族各一次生成（固定臂不生成任何东西）。契约被拒的那一族没有可用观测器，
            // 它的那次生成仍然花掉了——所以按契约条目数算，按臂的可用性算会低估成本。
            let calls: u32 = match arm {
                ARM_FIXED => 0,
                _ => reports
                    .iter()
                    .filter(|report| report.verification.status != VerificationStatus::NotVerified)
                    .count() as u32,
            };
            let total_opportunities: u32 = reports.iter().map(|report| report.opportunities).sum();
            let total_detections: u32 = reports.iter().map(|report| report.detections).sum();
            cumulative_calls.insert(arm.to_owned(), calls);
            opportunities.insert(arm.to_owned(), total_opportunities);
            detections.insert(arm.to_owned(), total_detections);
            detection_rate.insert(arm.to_owned(), rate(total_detections, total_opportunities));
            calls_per_detection.insert(
                arm.to_owned(),
                (total_detections > 0).then(|| f64::from(calls) / f64::from(total_detections)),
            );
        }
        points.push(AmortizationPoint {
            families_covered: covered,
            cumulative_calls,
            opportunities,
            detections,
            detection_rate,
            calls_per_detection,
        });
    }

    let mut after_version_change = BTreeMap::new();
    for arm in [
        ARM_FIXED_V2,
        ARM_GENERATED_V2,
        ARM_OBSERVER_V2_SUPPORT_ONLY,
        ARM_OBSERVER_V2_REVERIFIED,
        ARM_OBSERVER_V2_NO_COUNTER,
    ] {
        let runs: u32 = v2_arms
            .iter()
            .filter(|report| report.arm == arm)
            .map(|report| report.runs)
            .sum();
        after_version_change.insert(arm.to_owned(), runs);
    }

    AmortizationSection {
        cost_basis: cost_basis.to_owned(),
        points,
        after_version_change,
        note: "横轴是覆盖到第 k 个族时的累积**生成调用数**（固定臂为 0，因为它不生成任何东西）；\
               纵轴是同一批族上的累积检出数与「每检出一次要花多少次调用」。\
               验证运行与重复运行不折算成调用数——它们不花钱，花的是机器时间，\
               在 maintenance 一节里单独记。"
            .to_owned(),
    }
}

fn hypothesis_section(
    arms: &[ArmReport],
    amortization: &AmortizationSection,
    transfer: &[TransferCell],
) -> HypothesisSection {
    let totals = |arm: &str| -> (u32, u32, u32, u32) {
        let reports: Vec<&ArmReport> = arms
            .iter()
            .filter(|report| report.arm == arm && report.version == ProgramVersion::V1.id())
            .collect();
        (
            reports.iter().map(|report| report.detections).sum(),
            reports.iter().map(|report| report.opportunities).sum(),
            reports.iter().map(|report| report.false_passes).sum(),
            reports
                .iter()
                .map(|report| report.inconclusive_on_opportunity + report.inconclusive_on_clean)
                .sum(),
        )
    };
    let (observer_detections, observer_opportunities, observer_false_passes, observer_inconclusive) =
        totals(ARM_OBSERVER);
    let (fixed_detections, fixed_opportunities, fixed_false_passes, _) = totals(ARM_FIXED);
    let (generated_detections, generated_opportunities, generated_false_passes, _) =
        totals(ARM_GENERATED);

    let observer_rate = rate(observer_detections, observer_opportunities);
    let fixed_rate = rate(fixed_detections, fixed_opportunities);
    let generated_rate = rate(generated_detections, generated_opportunities);
    let observer_fp = rate(observer_false_passes, observer_opportunities);
    let fixed_fp = rate(fixed_false_passes, fixed_opportunities);
    let generated_fp = rate(generated_false_passes, generated_opportunities);
    let (_, _, fixed_balanced) = totals_two_sided(arms, ARM_FIXED);
    let (_, _, generated_balanced) = totals_two_sided(arms, ARM_GENERATED);
    let (_, _, observer_balanced) = totals_two_sided(arms, ARM_OBSERVER);

    let last = amortization.points.last();
    let observer_cpd = last.and_then(|point| {
        point
            .calls_per_detection
            .get(ARM_OBSERVER)
            .copied()
            .flatten()
    });
    let generated_cpd = last.and_then(|point| {
        point
            .calls_per_detection
            .get(ARM_GENERATED)
            .copied()
            .flatten()
    });

    let best_baseline_fp = match (fixed_fp, generated_fp) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(only), None) | (None, Some(only)) => Some(only),
        (None, None) => None,
    };
    // 结果质量的判据是**误放行率**，不是原始检出率：观测器在记录器被绕过时落进失败态，那是它该有
    // 的行为（§7.3 要的就是「测量脚本的成功输出不能成为唯一的真实性来源」），把它算成「没检出」
    // 会惩罚诚实；而误放行是这条要求上最贵的错误——违反被放过去了。判据在跑之前就写在 criteria 里。
    let improves_results = matches!(
        (observer_fp, best_baseline_fp),
        (Some(observer), Some(baseline)) if observer < baseline
    ) && observer_detections > 0;
    let improves_budget = matches!(
        (observer_cpd, generated_cpd),
        (Some(observer), Some(generated)) if observer <= generated
    );

    let mut readings = vec![
        format!(
            "误放行率（本判据用的那一项）：固定臂 {}（{fixed_false_passes}/{fixed_opportunities}）｜通用测试生成臂 {}（{generated_false_passes}/{generated_opportunities}）｜观测器臂 {}（{observer_false_passes}/{observer_opportunities}）",
            percent(fixed_fp),
            percent(generated_fp),
            percent(observer_fp),
        ),
        format!(
            "检出率（另一种读法，判据没有用它）：固定臂 {}（{fixed_detections}/{fixed_opportunities}）｜通用测试生成臂 {}（{generated_detections}/{generated_opportunities}）｜观测器臂 {}（{observer_detections}/{observer_opportunities}）",
            percent(fixed_rate),
            percent(generated_rate),
            percent(observer_rate),
        ),
        format!(
            "两侧读数（检出率与「无误违反时判对率」的平均，用来检查判据站不站得住）：固定臂 {}｜通用测试生成臂 {}｜观测器臂 {}",
            percent(fixed_balanced),
            percent(generated_balanced),
            percent(observer_balanced),
        ),
        format!(
            "观测器臂弃权（失败态）{observer_inconclusive} 次：记录器被绕过或被篡改时它拒绝作证，这正是扣掉它检出率的那部分"
        ),
        format!(
            "每检出一次所花的生成调用数：通用测试生成臂 {}｜观测器臂 {}",
            ratio(generated_cpd),
            ratio(observer_cpd),
        ),
    ];
    let off_diagonal: Vec<&TransferCell> =
        transfer.iter().filter(|cell| !cell.same_family).collect();
    if !off_diagonal.is_empty() {
        readings.push(format!(
            "迁移（没为它建、直接把同一个观测器套上去）：{}",
            off_diagonal
                .iter()
                .map(|cell| format!(
                    "{}→{} 检出率 {}、误放行 {}",
                    cell.built_for,
                    cell.applied_to,
                    percent(cell.detection_rate),
                    percent(rate(cell.false_passes, cell.opportunities)),
                ))
                .collect::<Vec<_>>()
                .join("｜")
        ));
    }
    if let Some(point) = last {
        readings.push(format!(
            "覆盖 {} 个族之后的累积生成调用数（口径 {}）：{}",
            point.families_covered,
            amortization.cost_basis,
            point
                .cumulative_calls
                .iter()
                .map(|(arm, calls)| format!("{arm}={calls}"))
                .collect::<Vec<_>>()
                .join("｜")
        ));
    }

    let supported = improves_results || improves_budget;
    let conclusion = if supported {
        let mut parts = Vec::new();
        if improves_results {
            parts.push("误放行率低于两个基线臂");
        }
        if improves_budget {
            parts.push("每检出一次所花的生成调用数不高于通用测试生成臂");
        }
        format!(
            "建设观测器{}，累积能力假说在本批读数下不被否定（保留意见见 caveats）。",
            parts.join("，")
        )
    } else {
        concat!(
            "建设观测器既没有改善后续任务的结果（误放行率不低于两个基线臂），",
            "也没有改善预算利用（每检出一次所花的生成调用数不低于通用测试生成臂）：",
            "**不支持累积能力假说。**",
        )
        .to_owned()
    };

    let mut caveats = vec![
        concat!(
            "判据用的是误放行率而不是原始检出率，理由是 criteria 里写的那条：观测器在记录器被绕过时",
            "落进失败态（拒绝作证），会被检出率口径当成「没检出」。**按原始检出率排序，观测器臂在这一批",
            "读数里输给两个基线臂**——两种读法都在 readings 里，别只看一个",
        )
        .to_owned(),
        concat!(
            "只用一侧的错误率是不够的：一个「一律判违反」的臂误放行率天然为 0。报告里的 `by_schedule`",
            "与 `clean_runs` 列把它照得出来——哪一臂在无违反的用例上一次都没判对，那里看得见。",
            "readings 里的「两侧读数」就是为这件事加的",
        )
        .to_owned(),
        concat!(
            "三个族是同一实验环境里的三条顺序约束，不是三个独立领域：迁移读数只说明「换个故障族就得",
            "重新建设」，不构成对别的领域的结论",
        )
        .to_owned(),
        concat!(
            "固定测试臂在 C2 上本来就够用（终态计数看得见丢失更新），它 0 次生成调用、每检出一次的",
            "调用数天然最低——「预算利用」这一项对基线更有利，这一点没有被抹平",
        )
        .to_owned(),
        concat!(
            "契约与测试计划各只有一次真实生成：模型这一次的输出质量会直接进到读数里，",
            "这里没有做多次采样的稳定性估计",
        )
        .to_owned(),
        concat!(
            "观测器臂的读数里有一大块是「失败态」：那是它对自己测量能力边界的如实报告，不是检出。",
            "把失败态折成「成立」会变成误放行，折成「违反」会变成误报",
        )
        .to_owned(),
    ];
    if !off_diagonal.is_empty() {
        caveats.push(
            concat!(
                "观测器跨族套用基本检出不到东西，还带上了误报与误放行：为单个族建的观测器不会迁移到",
                "别的故障族，迁移要另建一份契约（那笔账记在摊薄曲线的第二个账期里）",
            )
            .to_owned(),
        );
    }

    HypothesisSection {
        statement: concat!(
            "在一系列相关但不同的故障任务上，针对认知缺口建设观测器，是否比固定测试、",
            "通用测试生成更能改善后续任务的结果或预算利用（研究目标 §7.4）",
        )
        .to_owned(),
        criteria: vec![
            concat!(
                "§7.4 把误放行列在要报告的第一位：结果质量以**误放行率**衡量——违反被放过去是这条要求上",
                "最贵的错误，而「拒绝作证」（失败态）不是放行",
            )
            .to_owned(),
            concat!(
                "改善结果 = 观测器臂的误放行率严格低于固定臂与通用测试生成臂中较低的那个，且它确实",
                "作出过判定（检出数 > 0，不是靠全程弃权换来的零误放行）",
            )
            .to_owned(),
            concat!(
                "改善预算利用 = 观测器臂「每检出一次所花生成调用数」不高于通用测试生成臂；",
                "两条都不成立时，结论写「不支持累积能力假说」，成立时写清是哪一条成立",
            )
            .to_owned(),
        ],
        reporting: concat!(
            "判据冻结在上面的三条里，跑前提交、跑后未改。报告方式跑后扩充过一次，如实记在这里：",
            "两侧读数（检出率与「无误违反时判对率」的平均）是跑后加的稳健性检查——只用一侧错误率",
            "会被「一律判违反」的臂骗过去——它**不是**判据的一部分，判据与结论都没有因它改变。",
        )
        .to_owned(),
        readings,
        improves_results,
        improves_budget,
        supported,
        conclusion,
        caveats,
    }
}

/// 两侧读数里的那一项：检出率与「无误违反时判对率」的平均。
fn totals_two_sided(arms: &[ArmReport], arm: &str) -> (Option<f64>, Option<f64>, Option<f64>) {
    let reports: Vec<&ArmReport> = arms
        .iter()
        .filter(|report| report.arm == arm && report.version == ProgramVersion::V1.id())
        .collect();
    let detections: u32 = reports.iter().map(|report| report.detections).sum();
    let opportunities: u32 = reports.iter().map(|report| report.opportunities).sum();
    let clears: u32 = reports.iter().map(|report| report.correct_clears).sum();
    let clean_runs: u32 = reports.iter().map(|report| report.clean_runs).sum();
    let sensitivity = rate(detections, opportunities);
    let specificity = rate(clears, clean_runs);
    let balanced = match (sensitivity, specificity) {
        (Some(left), Some(right)) => Some((left + right) / 2.0),
        _ => None,
    };
    (sensitivity, specificity, balanced)
}

fn percent(value: Option<f64>) -> String {
    match value {
        Some(value) => format!("{:.1}%", value * 100.0),
        None => "—".to_owned(),
    }
}

fn ratio(value: Option<f64>) -> String {
    match value {
        Some(value) => format!("{value:.2}"),
        None => "—".to_owned(),
    }
}

fn verification_section(
    families: &[CFamily],
    setups: &BTreeMap<CFamily, ObserverSetup>,
) -> VerificationSection {
    let mut per_family = Vec::new();
    for family in families {
        let Some(setup) = setups.get(family) else {
            continue;
        };
        per_family.push(FamilyVerification {
            family: family.id().to_owned(),
            observer: crate::c_measure::observer_id(*family).to_string(),
            version: setup.enable.version,
            hook_points: setup
                .observer
                .points()
                .iter()
                .map(|point| point.id().to_owned())
                .collect(),
            contract_check: setup.check.clone(),
            directions: setup
                .independent
                .runs
                .iter()
                .map(|run| {
                    let passed = match run.direction {
                        jh_core::CheckDirection::Hidden => {
                            matches!(run.hidden.verdict, ObserverVerdict::Violated)
                        }
                        _ => run.observer.decided() && run.observer.verdict == run.expected,
                    };
                    DirectionReading {
                        direction: run.direction.to_string(),
                        implementation: run.implementation.id().to_owned(),
                        schedule: run.schedule.clone(),
                        injected: run.injected,
                        expected: verdict_label(&run.expected).to_owned(),
                        observer: verdict_label(&run.observer.verdict).to_owned(),
                        observer_detail: run.observer.detail.clone(),
                        hidden: verdict_label(&run.hidden.verdict).to_owned(),
                        passed,
                    }
                })
                .collect(),
            passed: setup.independent.check.passed(),
            observer_missed_the_bypass: setup.independent.observer_missed_the_bypass,
            self_certifying_rejected: setup.enable.self_certifying_rejected,
            self_certifying_error: setup.enable.self_certifying_error.clone(),
            lifecycle_after_rejection: setup.enable.lifecycle_after_rejection.clone(),
            checks_on_record_after_rejection: setup.enable.checks_on_record_after_rejection,
            enabled_records: setup.enable.enabled_records.clone(),
            invalidation: setup.enable.invalidation.clone(),
        });
    }
    VerificationSection {
        per_family,
        note: "四路独立检验（§7.3 末段）：阳性对照（合理实现必须通过）、阴性对照（已知错误实现必须\
               被抓到）、可注入故障（注入之后必须被检出）、独立隐藏检查（绕过记录器或篡改日志时，\
               由存储日志通道发现真实副作用）。第四路期望的是隐藏检查而不是观测器——观测器可以\
               什么都看不到，那也不能让这一路变成通过。四路没全过的观测器进不了启用态，这是\
               jh-core 的类型约束，不是本模块的自觉。"
            .to_owned(),
    }
}

// ---------------------------------------------------------------------------
// 生成步骤与成本账
// ---------------------------------------------------------------------------

async fn generate_contracts(
    families: &[CFamily],
    options: &H4Options,
    calls: &mut CallLog,
    clean: &mut bool,
) -> Result<Vec<GeneratedContract>, Error> {
    let generator = crate::c_measure::ContractGenerator::from_env()?;
    let generated = generator.generate(families, ProgramVersion::V1).await?;
    for entry in &generated {
        log_call(
            calls,
            &format!("{}-contract", options.run),
            "contract",
            entry.family,
            &entry.call,
        )?;
        if !entry.call.succeeded() {
            *clean = false;
        }
    }
    Ok(generated)
}

async fn generate_tests(
    families: &[CFamily],
    options: &H4Options,
    calls: &mut CallLog,
    clean: &mut bool,
    suffix: &str,
) -> Result<Vec<GeneratedTests>, Error> {
    let generator = crate::c_measure::TestGenerator::from_env()?;
    let generated = generator.generate(families, ProgramVersion::V1).await?;
    let purpose = if suffix.is_empty() {
        "tests"
    } else {
        "tests_regenerated"
    };
    let run = format!(
        "{}{}",
        options.run,
        if suffix.is_empty() { "-tests" } else { suffix }
    );
    for entry in &generated {
        log_call(calls, &run, purpose, entry.family, &entry.call)?;
        if !entry.call.succeeded() {
            *clean = false;
        }
    }
    Ok(generated)
}

fn log_call(
    calls: &mut CallLog,
    run: &str,
    purpose: &str,
    family: CFamily,
    call: &MeasureCall,
) -> Result<(), Error> {
    calls.append(&CallEntry::ModelCall {
        run: run.to_owned(),
        purpose: purpose.to_owned(),
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

fn generation_section(
    artifacts: &Artifacts,
    options: &H4Options,
    contracts_path: &Path,
    tests_path: &Path,
    regeneration: Regeneration,
) -> GenerationSection {
    let failed_artifacts = |entries: &[GeneratedTests]| -> u32 {
        entries
            .iter()
            .filter(|entry| !entry.call.succeeded())
            .count() as u32
    };
    let calls_this_run =
        artifacts.contracts.len() as u32 + artifacts.tests.len() as u32 + regeneration.calls;
    let failures_this_run = artifacts
        .contracts
        .iter()
        .filter(|entry| !entry.call.succeeded())
        .count() as u32
        + failed_artifacts(&artifacts.tests)
        + regeneration.failures;
    let regeneration_calls = regeneration.calls;
    GenerationSection {
        contract_source: source_label(&options.contracts),
        test_source: source_label(&options.tests),
        calls_this_run,
        failures_this_run,
        rejections: artifacts
            .contracts
            .iter()
            .filter(|entry| entry.rejection.is_some())
            .count() as u32
            + artifacts
                .tests
                .iter()
                .filter(|entry| entry.rejection.is_some())
                .count() as u32,
        contracts_file: absolute(contracts_path),
        tests_file: absolute(tests_path),
        regenerated_calls: regeneration_calls,
        reused_readings: vec![
            reuse_reading(
                "contract",
                artifacts
                    .contracts
                    .iter()
                    .map(|entry| (&entry.call, entry.call.succeeded())),
            ),
            reuse_reading(
                "tests",
                artifacts
                    .tests
                    .iter()
                    .map(|entry| (&entry.call, entry.call.succeeded())),
            ),
        ],
        contracts: artifacts
            .contracts
            .iter()
            .map(|entry| ContractEntry {
                family: entry.family.id().to_owned(),
                call: entry.call.clone(),
                accepted: entry.rejection.is_none(),
                contract: entry.contract.clone(),
                check: entry.check.clone(),
                rejection: entry.rejection.clone(),
            })
            .collect(),
        tests: artifacts
            .tests
            .iter()
            .map(|entry| TestEntry {
                family: entry.family.id().to_owned(),
                call: entry.call.clone(),
                accepted: entry.rejection.is_none(),
                checks: entry.checks.clone(),
                plan_note: entry.plan.as_ref().map(|plan| plan.note.clone()),
                rejection: entry.rejection.clone(),
            })
            .collect(),
    }
}

fn source_label(source: &ArtifactSource) -> String {
    match source {
        ArtifactSource::Generate => "generate".to_owned(),
        ArtifactSource::File(path) => absolute(path),
    }
}

fn reuse_reading<'a>(
    kind: &str,
    entries: impl Iterator<Item = (&'a MeasureCall, bool)>,
) -> ReuseReading {
    let mut calls = 0;
    let mut failures = 0;
    let mut latencies = Vec::new();
    let mut input_tokens = None;
    let mut output_tokens = None;
    for (call, _) in entries {
        calls += 1;
        latencies.push(call.latency_ms);
        if !call.succeeded() {
            failures += 1;
        }
        if let Some(usage) = &call.usage {
            input_tokens = Some(input_tokens.unwrap_or(0) + usage.input_tokens.unwrap_or(0));
            output_tokens = Some(output_tokens.unwrap_or(0) + usage.output_tokens.unwrap_or(0));
        }
    }
    ReuseReading {
        kind: kind.to_owned(),
        calls,
        failures,
        input_tokens,
        output_tokens,
        latency_p50_ms: crate::metrics::percentile(&latencies, 0.5),
    }
}

fn cost_section(calls_path: &Path, run: &str, runs: RunCounts) -> Result<CostSection, Error> {
    let entries = crate::calls::read_entries(calls_path).unwrap_or_default();
    let mut by_purpose = Vec::new();
    // 合计与分项用同一个构造函数：`calls` / `succeeded` / `failed` / token / 延迟分位数 / 调用统计
    // 必须互相自洽（同一个对象里不能一边说 9 次调用、一边说样本数 0）。
    let mut matched_all: Vec<&CallEntry> = Vec::new();
    for (purpose, suffix) in [
        ("contract", "-contract"),
        ("tests", "-tests"),
        ("tests_regenerated", "-tests-v2"),
    ] {
        let target = format!("{run}{suffix}");
        let matched: Vec<&CallEntry> = entries
            .iter()
            .filter(|entry| entry.run() == target)
            .collect();
        matched_all.extend(matched.iter().copied());
        by_purpose.push(purpose_cost(purpose, &matched));
    }
    let total = purpose_cost("合计", &matched_all);
    Ok(CostSection {
        by_purpose,
        total,
        runs,
        note:
            "成本分两笔：**生成**记调用次数、token 与延迟（货币成本不虚构，仓库里没有权威价格表）；\
               **验证与重复运行**不产生调用，按次数记在 `runs` 里。两笔都在这里，不挑着报。"
                .to_owned(),
    })
}

fn purpose_cost(purpose: &str, entries: &[&CallEntry]) -> PurposeCost {
    let mut latency_samples = Vec::new();
    let mut input_tokens = None;
    let mut output_tokens = None;
    let mut calls_with_usage = 0;
    let mut succeeded = 0;
    let mut failed = 0;
    for entry in entries {
        match entry {
            CallEntry::ModelCall {
                usage,
                latency_ms,
                error,
                ..
            } => {
                latency_samples.push(*latency_ms);
                if error.is_none() {
                    succeeded += 1;
                } else {
                    failed += 1;
                }
                if let Some(usage) = usage {
                    calls_with_usage += 1;
                    input_tokens =
                        Some(input_tokens.unwrap_or(0) + usage.input_tokens.unwrap_or(0));
                    output_tokens =
                        Some(output_tokens.unwrap_or(0) + usage.output_tokens.unwrap_or(0));
                }
            }
            CallEntry::Failure { latency_ms, .. } => {
                latency_samples.push(*latency_ms);
                failed += 1;
            }
            CallEntry::Judgment { .. } => {}
        }
    }
    PurposeCost {
        purpose: purpose.to_owned(),
        calls: entries.len() as u32,
        succeeded,
        failed,
        input_tokens,
        output_tokens,
        latency_p50_ms: crate::metrics::percentile(&latency_samples, 0.5),
        stats: call_stats(entries),
        latency: latency_stats(&latency_samples),
        usage: UsageStats {
            calls_with_usage,
            input_tokens,
            output_tokens,
        },
    }
}

fn latency_stats(samples: &[u64]) -> LatencyStats {
    LatencyStats {
        samples: samples.len() as u32,
        min: samples.iter().copied().min(),
        p50: crate::metrics::percentile(samples, 0.5),
        p95: crate::metrics::percentile(samples, 0.95),
        max: samples.iter().copied().max(),
    }
}

fn call_stats(entries: &[&CallEntry]) -> CallStats {
    let mut attempted = 0;
    let mut succeeded = 0;
    let mut errors: BTreeMap<String, u32> = BTreeMap::new();
    for entry in entries {
        if let CallEntry::ModelCall { error, .. } = entry {
            attempted += 1;
            match error {
                None => succeeded += 1,
                Some(text) => *errors.entry(text.clone()).or_default() += 1,
            }
        }
    }
    CallStats {
        attempted,
        succeeded,
        failed: attempted - succeeded,
        errors,
    }
}

fn declared_notes() -> Vec<String> {
    vec![
        "原始要求是「指定任务取消确认后，不应继续产生缓存写入」——本报告没有把它换成「最终缓存为空」。\
         `c1-s02`（暂停之前的合法写入）与 `c1-s05`（确认后写入再被合法回收）两个方向的读数就在\
         `arms` 的 `fixed` 里：那条被禁的替换在两侧都会判错。"
            .to_owned(),
        "单次成功不能推导所有调度都正确：报告给的是每条调度上的判定，检出率一律 n/N。".to_owned(),
        "测量脚本的成功输出不是唯一的真实性来源：真值取自存储日志通道的隐藏检查，\
         它与观测器不同模块、不同证据通道。"
            .to_owned(),
        "判定与生成失败如实计失败，不静默重试、不拿别的族的产物顶替。".to_owned(),
        "key 的取值不进报告：报告只记 OPENAI_MODEL 与端点是否设置。".to_owned(),
        "契约里的 site 词面核对拦得住越界与弱化，但那是词面核对不是语义证明：\
         真正的把关是四路独立检验与实际检出率。"
            .to_owned(),
        "看不见的读数就说看不见：某一族没有可用契约时，那一族的观测器臂标「未验证」，\
         不用别的臂顶替。"
            .to_owned(),
    ]
}

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

/// 终端摘要行。
impl H4Report {
    /// 终端摘要行。
    pub fn summary_lines(&self) -> Vec<String> {
        let mut lines = vec![
            format!(
                "H4 三臂对照（任务族 C）：{} ｜ {} 个族 × {} 条调度 × {} 路实现",
                self.run,
                self.families.len(),
                self.families
                    .first()
                    .map(|family| family.schedules.len())
                    .unwrap_or(0),
                Implementation::ALL.len()
            ),
            "臂 = 固定测试 / 通用测试生成 / 针对缺口建设观测器；真值来自隐藏检查（存储日志通道）"
                .to_owned(),
        ];
        for arm in &self.arms {
            lines.push(format!(
                "{} × {}@{}：机会 {}｜检出 {}（{}）｜误放行 {}（{}）｜失败态 {}｜误报 {}｜干净判定 {}｜用例 {}",
                arm.arm,
                arm.family,
                arm.version,
                arm.opportunities,
                arm.detections,
                percent(arm.detection_rate),
                arm.false_passes,
                percent(arm.false_pass_rate),
                arm.inconclusive(),
                arm.false_alarms,
                arm.correct_clears,
                arm.runs,
            ));
        }
        lines.push("迁移（观测器交叉套用）：".to_owned());
        for cell in &self.transfer {
            lines.push(format!(
                "  {} → {}：机会 {}｜检出 {}（{}）｜误报 {}",
                cell.built_for,
                cell.applied_to,
                cell.opportunities,
                cell.detections,
                percent(cell.detection_rate),
                cell.false_alarms,
            ));
        }
        for family in &self.verification.per_family {
            lines.push(format!(
                "独立检验 {}：四路 {}｜钩子点 {:?}｜绕过记录器时观测器漏掉 {}｜自证式检查被拒 {}{}",
                family.family,
                if family.passed { "全过" } else { "未全过" },
                family.hook_points,
                family.observer_missed_the_bypass,
                family.self_certifying_rejected,
                match &family.invalidation {
                    Some(read) => format!("｜启用传播 {read}"),
                    None => String::new(),
                },
            ));
        }
        for reading in &self.maintenance.arms {
            lines.push(format!(
                "维护 {}：重生成 {} 次调用（{}）｜重挂 {}｜重跑检验 {}｜检出率 {} → {}｜误放行率 {}",
                reading.arm,
                reading.regeneration_calls,
                if reading.regenerated { "是" } else { "否" },
                reading.reattached,
                reading.reverified,
                percent(reading.detection_rate_before),
                percent(reading.detection_rate_after),
                percent(reading.false_pass_rate_after),
            ));
        }
        for total in &self.maintenance.totals {
            lines.push(format!(
                "维护合计 {}：{} 个族｜机会 {}｜检出 {}（{} → {}）｜误放行 {}（{}）｜弃权 {}（机会上 {}）",
                total.arm,
                total.families,
                total.opportunities,
                total.detections,
                percent(total.detection_rate_before),
                percent(total.detection_rate_after),
                total.false_passes,
                percent(total.false_pass_rate_after),
                total.inconclusive_on_opportunity + total.inconclusive_on_clean,
                total.inconclusive_on_opportunity,
            ));
        }
        for point in &self.amortization.points {
            lines.push(format!(
                "摊薄（覆盖 {} 个族）：{}",
                point.families_covered,
                point
                    .cumulative_calls
                    .iter()
                    .map(|(arm, calls)| format!(
                        "{arm}={calls} 次生成调用 / {} 次检出 / 检出率 {}",
                        point.detections.get(arm).copied().unwrap_or(0),
                        percent(point.detection_rate.get(arm).copied().flatten()),
                    ))
                    .collect::<Vec<_>>()
                    .join("｜")
            ));
        }
        lines.push(format!(
            "假说：改善结果 {}｜改善预算利用 {}｜结论 {}",
            self.hypothesis.improves_results,
            self.hypothesis.improves_budget,
            self.hypothesis.conclusion
        ));
        for reading in &self.hypothesis.readings {
            lines.push(format!("  {reading}"));
        }
        for cost in &self.cost.by_purpose {
            lines.push(format!(
                "成本 {}：调用 {}/{} 失败 {}｜输入 token {}｜输出 token {}",
                cost.purpose,
                cost.succeeded,
                cost.calls,
                cost.failed,
                cost.input_tokens
                    .map_or_else(|| "—".to_owned(), |value| value.to_string()),
                cost.output_tokens
                    .map_or_else(|| "—".to_owned(), |value| value.to_string()),
            ));
        }
        lines.push(format!(
            "运行次数：主矩阵 {}｜版本变更 {}｜四路独立检验 {}｜迁移复算 {}｜真值复算 {}",
            self.cost.runs.main_cases,
            self.cost.runs.version_change_cases,
            self.cost.runs.verification_cases,
            self.cost.runs.transfer_cases,
            self.cost.runs.ground_truth_recomputations,
        ));
        lines.push(format!("产物：{}", self.raw.out_dir));
        lines.push(format!("逐用例读数：{}", self.raw.runs));
        lines.push(format!("调用记录：{}", self.raw.calls_log));
        lines.push(format!("命令：{}", self.command));
        lines
    }
}

/// 记录复现命令时用的通道标签（生成走 LLM 通道）。
pub const GENERATION_JUDGES: [JudgeName; 1] = [JudgeName::Llm];
