//! H1/H2 对照：四组臂跑完整任务族 A，汇成 JSON 报告（`docs/roadmap-1.0.md` §5 M1、§6.2）。
//!
//! # 四组臂
//!
//! | 臂 | H1 依赖失效传播 | H2 重算 | 研究目标 §9.2 的消融行 |
//! | --- | --- | --- | --- |
//! | `h1on-h2inc` | 开 | 增量 | 基准组 |
//! | `h1off-h2inc` | 关 | 增量 | 「关闭依赖失效传播」 |
//! | `h1on-h2full` | 开 | 全量 | 「每次全量重算」 |
//! | `h1off-h2full` | 关 | 全量 | 两个消融的组合（报告交互作用） |
//!
//! 四组共享任务集、初始证据、扰动脚本、判定器配置与控制策略；差别只有 [`Arm`] 的两个开关。全量臂
//! 与增量臂用同一条输入投影规则（见 [`crate::matching`]），因此读数不会被「信息裁剪」冒充成
//! 「缓存收益」（研究目标 §8.4）。
//!
//! # 指标口径
//!
//! 报告里逐条写明口径（[`CompareMetricDefinitions::declared`]），主要读数是研究目标 §10.1 / §10.2
//! 的落点：
//!
//! - **关键决策机会**：场景协议定义的决策轮数（不是控制器提了多少动作）；
//! - **旧依据行动率**：基于失效前提作出的承诺次数 ÷ 关键决策机会数（同时报告绝对错误动作数、
//!   弃权与未恢复任务）；
//! - **恢复延迟**：从破坏前提的事件发生，到系统基于更新后的依据发出一个通过独立验收的动作所需的
//!   轮数、判断调用数与墙钟时间；
//! - **依赖漏失效 / 误失效**：逐条扰动对照「问题声明的读取范围」与运行时实际标记；
//! - **重算节点数 / 输入量 / 缓存复用率**：判断节点数、state 序列化字节与 token 用量、复用次数；
//! - **真实与模拟来源分开**：`source` / `live` / `verification` 三个字段与 M0 基线同口径。
//!
//! # 结论按配对读
//!
//! 报告里的 [`PairingReading`] 把同一个口径内的两条臂并排：`h1` 配对固定 H2 口径、只让依赖失效传播
//! 变，`h2` 配对固定 H1 开关、只让重算口径变。跨配对比会把交互作用当成主效应——本项目的读数里，
//! 关闭失效传播的恢复损失只在增量配对成立（全量重算补得回来），而「拿过期依据发动作」两个配对都
//! 补不回来。`notes` 里写明这条口径，摘要逐条打印配对读数。
//!
//! 报告可以「只重建、不重跑」：`compare --regenerate` 从 `traces_dir` 里的轨迹重新汇出报告（判定器
//! 一次都不调用），口径与 notes 取当前代码，读数与那次运行的轨迹一一对应。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use jh_core::{ActionId, Judge, JudgeSource, NodeId, QuestionId, QuestionVersion};
use serde::Serialize;

use crate::calls::{CallEntry, CallLog};
use crate::error::Error;
use crate::exp::unix_ms;
use crate::family::{FamilyA, FamilyTask};
use crate::judge::JudgeName;
use crate::matching::{
    ASK_SUCCEEDED, Arm, DecisionOutcome, LoopContext, Recompute, TaskTrace, run_task,
};
use crate::report::{
    CallStats, LatencyStats, QualityStats, QuestionQuality, UsageStats, Verification,
    VerificationStatus,
};

/// 一次对照运行的参数。
#[derive(Debug, Clone)]
pub struct CompareOptions {
    /// 要跑的判定器，按顺序；每个判定器跑完四组臂。
    pub judges: Vec<JudgeName>,
    /// 要跑的臂，按顺序。
    pub arms: Vec<Arm>,
    /// 运行标识。
    pub run: String,
    /// 调用记录落盘路径（JSONL，追加）。
    pub calls_path: PathBuf,
    /// 录制回放读取的记录文件。
    pub replay_path: PathBuf,
    /// 运行产物目录（轨迹与证据日志都落在它下面）。
    pub out_dir: PathBuf,
    /// 复现命令，原样写进报告。
    pub command: String,
    /// 只跑前 N 个任务（`None` 表示全部）。
    pub max_tasks: Option<usize>,
}

/// 一次对照运行的结果。
#[derive(Debug, Clone)]
pub struct CompareOutcome {
    /// 报告。
    pub report: CompareReport,
    /// 是否每个（判定器 × 臂 × 任务）都跑到底且没有判断失败。
    pub clean: bool,
}

/// 对照报告的顶层结构。
#[derive(Debug, Clone, Serialize)]
pub struct CompareReport {
    /// 运行标识。
    pub run: String,
    /// 生成时刻，Unix 毫秒。
    pub generated_at_unix_ms: u64,
    /// 复现命令。
    pub command: String,
    /// 任务族摘要。
    pub family: FamilySummary,
    /// 指标口径声明。
    pub metric_definitions: CompareMetricDefinitions,
    /// 判定器 × 臂的报告。
    pub arms: Vec<ArmReport>,
    /// 臂配对读数（每条只在同一口径内比较）。
    pub pairings: Vec<PairingReading>,
    /// **写这份报告时**的非保密环境（模型与端点；key 只记「是否设置」）。
    ///
    /// 重建报告时它反映重建时刻的 shell，不是那次运行的环境——真实运行的模型与端点以轨迹为准
    /// （每条判断的 `version` 有模型，`TaskTrace::judge_endpoint` 有端点）。
    pub report_environment: ReportEnvironment,
    /// 轨迹目录（绝对路径）。
    pub traces_dir: String,
    /// 证据日志目录（绝对路径）。
    pub evidence_dir: String,
    /// 调用记录（绝对路径）。
    pub calls_log: String,
    /// 口径与限制说明。
    pub notes: Vec<String>,
}

/// **写这份报告时**的非保密环境。
///
/// 只记模型标识与端点（复现时要用的那两样），以及 **key 是否设置**——key 的值任何情况下都不写进
/// 报告、日志或轨迹（`docs/roadmap-1.0.md` §9 风险表：key 只经环境变量）。
///
/// [`ReportEnvironment::recorded_at`] 标出这份取值是**什么样**的：`run` 表示报告与调用同进程产出
/// （取值就是那次运行的环境），`regenerate` 表示报告是从轨迹重建的（取值只是重建时刻的 shell）。
/// 那次运行真正用到的模型与端点在轨迹里：每条判断的 `version` 是模型，`TaskTrace::judge_endpoint`
/// 是端点（新代码产出的轨迹才有这个字段）。
#[derive(Debug, Clone, Serialize)]
pub struct ReportEnvironment {
    /// 这份环境取值的来源：`run`（与调用同进程）或 `regenerate`（重建报告时的 shell）。
    pub recorded_at: String,
    /// `OPENAI_MODEL`（LLM 判定器的模型、必填）。
    pub openai_model: Option<String>,
    /// `OPENAI_BASE_URL`（LLM 端点、必填）。
    pub openai_base_url: Option<String>,
    /// `OPENAI_API_KEY` 是否设置。
    pub openai_api_key_set: bool,
    /// `TYPESAFE_API_KEY` 是否设置。
    pub typesafe_api_key_set: bool,
    /// `TYPESAFE_BASE_URL`（Jev 端点覆盖，可选）。
    pub typesafe_base_url: Option<String>,
    /// Jev 客户端 pin 的模型。
    pub jev_model: String,
}

/// 任务族摘要。
#[derive(Debug, Clone, Serialize)]
pub struct FamilySummary {
    /// 任务族名字。
    pub name: String,
    /// 仓库内位置。
    pub source: String,
    /// 任务个数。
    pub tasks: usize,
    /// 轮数口径：每个任务一次决策机会为一轮。
    pub rounds: BTreeMap<String, usize>,
    /// 问题版本历史（含当前标记）。
    pub questions: Vec<crate::family::QuestionVersionEntry>,
}

/// 一条臂在配对读数里的摘要。
#[derive(Debug, Clone, Serialize)]
pub struct ArmReading {
    /// 臂标识。
    pub arm: String,
    /// 独立验收通过与任务总数（`通过/总数`）。
    pub acceptance: String,
    /// 恢复的必需动作与总数（`恢复/总数`）。
    pub recovered: String,
    /// 基于失效前提发出的动作次数。
    pub stale_basis_commitments: u32,
    /// 不该发出的动作次数。
    pub wrong_actions: u32,
    /// 判断请求次数。
    pub calls: u32,
    /// 重算节点数。
    pub judged_nodes: u32,
    /// 输入字节合计。
    pub input_bytes: u64,
    /// 缓存复用率。
    pub cache_reuse_rate: Option<f64>,
}

/// 一组配对读数：**同一口径内**的两条臂并排。
///
/// 结论必须按配对读：`h1` 配对固定 H2 口径、只让依赖失效传播变，`h2` 配对固定 H1 开关、只让重算口径
/// 变。跨配对比会把交互作用当成主效应——本项目的读数里 H1 的灾难性表现在增量配对成立、在全量配对
/// 消失，H2 的成本优势在 H1 开启的配对成立、在关闭的配对质量反转。
#[derive(Debug, Clone, Serialize)]
pub struct PairingReading {
    /// 对照维度：`h1` 或 `h2`。
    pub dimension: String,
    /// 固定住的那一侧（`h2=incremental` / `h2=full` / `h1=on` / `h1=off`）。
    pub fixed: String,
    /// 判定器。
    pub judge: String,
    /// 左侧臂：`h1` 配对里是「传播开」，`h2` 配对里是「增量」。
    pub left: ArmReading,
    /// 右侧臂：`h1` 配对里是「传播关」，`h2` 配对里是「全量」。
    pub right: ArmReading,
    /// 这组配对怎么读。
    pub note: String,
}

/// 指标口径声明：报告里的每个读数怎么算，写在这里。
#[derive(Debug, Clone, Serialize)]
pub struct CompareMetricDefinitions {
    /// 关键决策机会。
    pub decision_opportunity: String,
    /// 旧依据行动率。
    pub stale_basis_action_rate: String,
    /// 恢复延迟。
    pub recovery_latency: String,
    /// 漏失效。
    pub missed_invalidation: String,
    /// 误失效。
    pub extra_invalidation: String,
    /// 重算节点数与输入量。
    pub recompute: String,
    /// 缓存复用率。
    pub cache_reuse: String,
    /// 独立验收。
    pub acceptance: String,
    /// 成本。
    pub cost: String,
    /// 真实与模拟来源的区分。
    pub sources: String,
    /// 输入投影规则（H2 两臂共用）。
    pub input_projection: String,
    /// 判断失败的口径。
    pub failures: String,
}

impl CompareMetricDefinitions {
    /// 本项目声明的口径文本。
    pub fn declared() -> Self {
        Self {
            decision_opportunity:
                "每个任务每一轮算一次决策机会（场景协议规定的轮数，不由控制器提了多少动作决定）。"
                    .to_owned(),
            stale_basis_action_rate:
                "基于失效前提作出的承诺次数 ÷ 关键决策机会数；同时报告绝对错误动作数、弃权次数与未恢复任务。"
                    .to_owned(),
            recovery_latency:
                "从「某动作首次被要求」那一轮的破坏性事件起算，到该动作被发出的轮数差、其间的判断调用数与墙钟毫秒；起点取该轮决策相位之前最后一条扰动，若这个要求从初始快照就成立（没有可起算的破坏性事件），起点回落到本次任务运行的开始时刻。"
                    .to_owned(),
            missed_invalidation:
                "逐条扰动对照：问题声明的读取范围包含被改动对象、但运行时没有把它的判断标成待重查（漏失效）。"
                    .to_owned(),
            extra_invalidation:
                "逐条扰动对照：被改动对象不在问题的声明读取范围里，运行时却把它的判断标成待重查（误失效，含模型提出的额外依赖）。"
                    .to_owned(),
            recompute:
                "重算节点数 = 落盘的判断条数；输入量 = 每次请求 state 的序列化字节之和与通道上报的输入 token（没有上报时为 null）。"
                    .to_owned(),
            cache_reuse:
                "缓存复用率 = 复用事件 ÷（复用事件 + 请求次数）；复用是否仍然新鲜按快照校验测量，不参与决策。"
                    .to_owned(),
            acceptance:
                "独立验收只读证据日志与隐藏真值：必需动作是否发出、有没有不该发出的动作、完成声明是否绑定实际交付物当前版本的证据；控制器的完成声明只计数。"
                    .to_owned(),
            cost:
                "成本分三笔分别记账：判断调用次数与 token 用量（通道上报）、动作计价成本（本项目自定单位，不是任何供应商价格表）、延迟分位数。"
                    .to_owned(),
            sources:
                "真实通道（jev / llm，live=true）与本地判定器（rule / replay，live=false）在 source / live / verification 三个字段上分开标注；模拟真值不参与本对照。"
                    .to_owned(),
            input_projection:
                "一次请求里的问题必须共享同一读取范围；本对照按问题声明的读取对象分组，投影 = 这组对象的当前版本。增量臂与全量臂用同一条规则，差别只在要不要复算。"
                    .to_owned(),
            failures:
                "判断失败如实计入失败数并写进调用记录，不丢弃、不静默重试；失败的问题是「没答上来的问题」，下一轮按同一调度规则再请求。"
                    .to_owned(),
        }
    }
}

/// 一组（判定器 × 臂）的汇总读数。
#[derive(Debug, Clone, Serialize)]
pub struct ArmReport {
    /// 判定器名字。
    pub judge: String,
    /// 判定器来源标签。
    pub source: JudgeSource,
    /// 是否真实网络调用。
    pub live: bool,
    /// 臂标识。
    pub arm: String,
    /// 臂说明。
    pub arm_description: String,
    /// H1 开关。
    pub h1_propagation: bool,
    /// H2 口径。
    pub h2_recompute: String,
    /// 验证状态。
    pub verification: Verification,
    /// 见过的执行者版本。
    pub versions: Vec<String>,
    /// 调用统计。
    pub calls: CallStats,
    /// 延迟分位数。
    pub latency_ms: LatencyStats,
    /// token 用量。
    pub usage: UsageStats,
    /// 决策与行动读数。
    pub decisions: DecisionStats,
    /// 重算与复用读数。
    pub recompute: RecomputeStats,
    /// 失效传播读数。
    pub invalidation: InvalidationStats,
    /// 判断质量读数（与 M0 基线同口径）。
    pub quality: QualityStats,
    /// 独立验收读数。
    pub acceptance: AcceptanceStats,
    /// 恢复延迟读数。
    pub recovery: RecoveryStats,
    /// 逐任务摘要。
    pub tasks: Vec<TaskSummary>,
}

/// 决策与行动读数。
#[derive(Debug, Clone, Serialize)]
pub struct DecisionStats {
    /// 关键决策机会（决策轮数 × 任务数）。
    pub opportunities: u32,
    /// 发出的动作次数。
    pub executions: u32,
    /// 基于失效前提发出的动作次数（旧依据行动率的分子）。
    pub stale_basis_commitments: u32,
    /// 旧依据行动率。
    pub stale_basis_action_rate: Option<f64>,
    /// 不该发出的动作次数（提前发出、或候选集里没有该发的动作）。
    pub wrong_actions: u32,
    /// 被前提复核挡下的次数。
    pub blocked: u32,
    /// 前提门不成立的次数。
    pub gate_unsatisfied: u32,
    /// 手上没有答案、发不出动作的次数（弃权）。
    pub abstentions: u32,
    /// 动作计价成本合计（自定单位）。
    pub action_cost_units: f64,
}

/// 重算与复用读数。
#[derive(Debug, Clone, Serialize)]
pub struct RecomputeStats {
    /// 判断请求次数。
    pub asks: u32,
    /// 请求里问的问题条数。
    pub asked_questions: u32,
    /// 落盘的判断条数（重算节点数）。
    pub judged_nodes: u32,
    /// 复用事件次数。
    pub reuse_events: u32,
    /// 复用时结果已经失效的次数。
    pub stale_reuse_events: u32,
    /// 缓存复用率。
    pub cache_reuse_rate: Option<f64>,
    /// 请求 state 的序列化字节合计。
    pub input_bytes: u64,
}

/// 失效传播读数。
#[derive(Debug, Clone, Serialize)]
pub struct InvalidationStats {
    /// 被标成待重查的判断节点次数。
    pub marked_nodes: u32,
    /// 被标成待复核的动作次数。
    pub marked_actions: u32,
    /// 漏失效次数。
    pub missed: u32,
    /// 误失效次数。
    pub extra: u32,
    /// 无关结果保留次数（扰动落在声明范围之外、相关判断仍然当前有效）。
    pub unrelated_preserved: u32,
}

/// 独立验收读数。
#[derive(Debug, Clone, Serialize)]
pub struct AcceptanceStats {
    /// 通过验收的任务数。
    pub passed: u32,
    /// 任务总数。
    pub tasks: u32,
    /// 逐任务失败原因。
    pub failures: Vec<String>,
}

/// 恢复延迟读数。
#[derive(Debug, Clone, Serialize)]
pub struct RecoveryStats {
    /// 被要求过的动作次数。
    pub required_actions: u32,
    /// 其中真的发出了的次数。
    pub recovered: u32,
    /// 未恢复的动作。
    pub unrecovered: Vec<String>,
    /// 恢复所需轮数（升序）。
    pub rounds: Vec<u32>,
    /// 恢复所需判断调用数（升序）。
    pub calls: Vec<u32>,
    /// 恢复所需墙钟毫秒（升序）。
    pub wall_ms: Vec<u64>,
}

/// 一个任务的摘要（原始数据的位置在这里）。
#[derive(Debug, Clone, Serialize)]
pub struct TaskSummary {
    /// 任务标识。
    pub task: String,
    /// 决策机会数。
    pub opportunities: u32,
    /// 发出的动作。
    pub executions: Vec<String>,
    /// 不该发出的动作。
    pub wrong_actions: Vec<String>,
    /// 基于失效前提发出的动作。
    pub stale_basis: Vec<String>,
    /// 未恢复的必需动作。
    pub unrecovered: Vec<String>,
    /// 恢复轮数。
    pub recovery_rounds: Vec<u32>,
    /// 判断请求次数。
    pub asks: u32,
    /// 复用事件次数。
    pub reuse_events: u32,
    /// 判断失败次数。
    pub judge_failures: u32,
    /// 落盘的判断条数。
    pub judgments: u32,
    /// 与隐藏真值一致的判断条数（判定质量读数，用于区分「架构差异」与「判定答错」）。
    pub agreements: u32,
    /// 独立验收是否通过。
    pub acceptance_passed: bool,
    /// 验收判定明细。
    pub acceptance_checks: Vec<String>,
    /// 轨迹文件。
    pub trace: String,
    /// 证据日志文件。
    pub evidence: String,
}

/// 真实通道的端点（不含 key）：优先取环境变量覆盖，其次取通道的官方缺省；本地判定器给 `None`。
fn endpoint_for(name: JudgeName) -> Option<String> {
    let from_env = |key: &str| std::env::var(key).ok().filter(|value| !value.is_empty());
    match name {
        JudgeName::Llm => {
            from_env("OPENAI_BASE_URL").or_else(|| Some(jh_openai::BASE_URL.to_owned()))
        }
        JudgeName::Jev => {
            from_env("TYPESAFE_BASE_URL").or_else(|| Some(jh_jev::DEFAULT_BASE_URL.to_owned()))
        }
        JudgeName::Rule | JudgeName::Replay | JudgeName::SimulatedTruth => None,
    }
}

/// 跑四组臂 × 全部任务，汇出报告。
pub async fn run(family: &FamilyA, options: &CompareOptions) -> Result<CompareOutcome, Error> {
    let tasks: Vec<&FamilyTask> = match options.max_tasks {
        Some(limit) => family.tasks.iter().take(limit).collect(),
        None => family.tasks.iter().collect(),
    };

    std::fs::create_dir_all(&options.out_dir)?;
    let traces_dir = options.out_dir.join("traces");
    let evidence_dir = options.out_dir.join("evidence");
    std::fs::create_dir_all(&traces_dir)?;
    std::fs::create_dir_all(&evidence_dir)?;

    let mut calls = CallLog::open(options.calls_path.clone())?;
    let mut reports = Vec::new();
    let mut clean = true;

    for name in &options.judges {
        // 每个判定器构造一次；构造失败时该判定器整段标「未验证」，不换后端顶上。
        let built = build_judge(*name, &options.replay_path);
        let judge = match built {
            Ok(judge) => judge,
            Err(error) => {
                clean = false;
                for arm in &options.arms {
                    reports.push(unavailable_report(*name, *arm, &error));
                }
                continue;
            }
        };

        for arm in &options.arms {
            let mut accumulator = Accumulator::new(*name, *arm);
            let endpoint = endpoint_for(*name);
            for task in &tasks {
                let stem = format!("{}-{}-{}", name.cli_name(), arm.label(), task.id);
                let evidence_path = evidence_dir.join(format!("{stem}.jsonl"));
                let trace_path = traces_dir.join(format!("{stem}.json"));
                let mut context = LoopContext {
                    family,
                    judge: judge.as_ref(),
                    channel: *name,
                    arm: *arm,
                    run: options.run.clone(),
                    endpoint: endpoint.clone(),
                    calls: &mut calls,
                };
                let trace = run_task(&mut context, task, &evidence_path).await?;
                write_json(&trace_path, &trace)?;
                if trace.asks.iter().any(|ask| ask.outcome != ASK_SUCCEEDED) {
                    clean = false;
                }
                accumulator.absorb(
                    task,
                    &trace,
                    &absolute(&trace_path),
                    &absolute(&evidence_path),
                    family,
                );
            }
            reports.push(accumulator.finish());
        }
    }

    let pairings = pairings(&reports);

    Ok(CompareOutcome {
        report: CompareReport {
            run: options.run.clone(),
            generated_at_unix_ms: unix_ms(),
            command: options.command.clone(),
            family: FamilySummary {
                name: family.name.clone(),
                source: family.source.clone(),
                tasks: tasks.len(),
                rounds: tasks
                    .iter()
                    .map(|task| (task.id.clone(), task.rounds))
                    .collect(),
                questions: family.history().to_vec(),
            },
            metric_definitions: CompareMetricDefinitions::declared(),
            arms: reports,
            pairings,
            report_environment: report_environment("run"),
            traces_dir: absolute(&traces_dir),
            evidence_dir: absolute(&evidence_dir),
            calls_log: absolute(&options.calls_path),
            notes: declared_notes(),
        },
        clean,
    })
}

/// 写报告时的非保密环境；`recorded_at` 标出来源（`run` / `regenerate`）。
pub fn report_environment(recorded_at: &str) -> ReportEnvironment {
    let set = |name: &str| std::env::var_os(name).is_some_and(|value| !value.is_empty());
    let value = |name: &str| std::env::var(name).ok().filter(|text| !text.is_empty());
    ReportEnvironment {
        recorded_at: recorded_at.to_owned(),
        openai_model: value("OPENAI_MODEL"),
        openai_base_url: value("OPENAI_BASE_URL"),
        openai_api_key_set: set("OPENAI_API_KEY"),
        typesafe_api_key_set: set("TYPESAFE_API_KEY"),
        typesafe_base_url: value("TYPESAFE_BASE_URL"),
        jev_model: jh_jev::DEFAULT_MODEL.to_owned(),
    }
}

/// 复现命令：把本次运行**用到**的非保密环境写在 argv 前面（key 只写变量名，绝不写值）。
///
/// 只带上与所选判定器相关的变量：跑 `rule` / `replay` 时不该出现模型或端点，免得把命令写成
/// 「看起来要联网」。
pub fn reproduction_command(argv: &[String], judges: &[JudgeName]) -> String {
    let environment = report_environment("command");
    let mut prefix: Vec<String> = Vec::new();
    let mut push = |name: &str, shown: &str| prefix.push(format!("{name}={shown}"));

    if judges.contains(&JudgeName::Llm) {
        if let Some(model) = environment.openai_model {
            push("OPENAI_MODEL", &model);
        }
        if let Some(base_url) = environment.openai_base_url {
            push("OPENAI_BASE_URL", &base_url);
        }
        if environment.openai_api_key_set {
            push("OPENAI_API_KEY", "$OPENAI_API_KEY");
        }
    }
    if judges.contains(&JudgeName::Jev) {
        if environment.typesafe_api_key_set {
            push("TYPESAFE_API_KEY", "$TYPESAFE_API_KEY");
        }
        if let Some(base_url) = environment.typesafe_base_url {
            push("TYPESAFE_BASE_URL", &base_url);
        }
    }

    format!("{} {}", prefix.join(" "), argv.join(" "))
        .trim()
        .to_owned()
}

/// 由已完成的臂报告生成配对读数：每条只在同一口径内比较。
fn pairings(arms: &[ArmReport]) -> Vec<PairingReading> {
    let reading = |arm: &ArmReport| ArmReading {
        arm: arm.arm.clone(),
        acceptance: format!("{}/{}", arm.acceptance.passed, arm.acceptance.tasks),
        recovered: format!(
            "{}/{}",
            arm.recovery.recovered, arm.recovery.required_actions
        ),
        stale_basis_commitments: arm.decisions.stale_basis_commitments,
        wrong_actions: arm.decisions.wrong_actions,
        calls: arm.calls.attempted,
        judged_nodes: arm.recompute.judged_nodes,
        input_bytes: arm.recompute.input_bytes,
        cache_reuse_rate: arm.recompute.cache_reuse_rate,
    };
    let judges: BTreeSet<&String> = arms.iter().map(|arm| &arm.judge).collect();
    let mut pairings = Vec::new();

    for judge in judges {
        let find = |propagation: bool, recompute: Recompute| {
            arms.iter().find(|arm| {
                arm.judge == *judge
                    && arm.h1_propagation == propagation
                    && arm.h2_recompute == recompute.label()
            })
        };
        for recompute in [Recompute::Incremental, Recompute::Full] {
            let (Some(left), Some(right)) = (find(true, recompute), find(false, recompute)) else {
                continue;
            };
            pairings.push(PairingReading {
                dimension: "h1".to_owned(),
                fixed: format!("h2={}", recompute.label()),
                judge: judge.clone(),
                left: reading(left),
                right: reading(right),
                note: "同一 H2 口径内比较依赖失效传播：差异只能归因于「是否使用失效传播」，跨配对的差异是交互作用，不能当主效应读。"
                    .to_owned(),
            });
        }
        for propagation in [true, false] {
            let (Some(left), Some(right)) = (
                find(propagation, Recompute::Incremental),
                find(propagation, Recompute::Full),
            ) else {
                continue;
            };
            let (left_reading, right_reading) = (reading(left), reading(right));
            // 这句按本组读数现场生成，不写死数字：两组之间「要不要复算」既可能只体现在成本上，
            // 也可能改变循环看得到什么（没有失效传播时，每轮全量重看相当于把丢失的重查触发补回来）。
            let note = format!(
                "同一 H1 开关内比较重算口径：两边用同一条输入投影规则，差异只能归因于「要不要复算」。本组：调用 {} vs {}、输入 {} 字节 vs {} 字节、判断节点 {} vs {}、验收 {} vs {}、恢复 {} vs {}、旧依据承诺 {} vs {}——全量重算每轮把全部问题重看一遍，因此该差异可能只体现在成本上（复算量与输入量），也可能改变循环看得到什么（没有失效传播时把「输入变了要重查」的触发补回来）。",
                left_reading.calls,
                right_reading.calls,
                left_reading.input_bytes,
                right_reading.input_bytes,
                left_reading.judged_nodes,
                right_reading.judged_nodes,
                left_reading.acceptance,
                right_reading.acceptance,
                left_reading.recovered,
                right_reading.recovered,
                left_reading.stale_basis_commitments,
                right_reading.stale_basis_commitments,
            );
            pairings.push(PairingReading {
                dimension: "h2".to_owned(),
                fixed: format!("h1={}", if propagation { "on" } else { "off" }),
                judge: judge.clone(),
                left: left_reading,
                right: right_reading,
                note,
            });
        }
    }
    pairings
}

/// 报告的固定说明。
fn declared_notes() -> Vec<String> {
    vec![
        "四组臂共享任务集、初始证据、扰动脚本、判定器配置与控制策略，差别只有 H1/H2 两个开关。"
            .to_owned(),
        "模拟真值不参与本对照（研究目标 §9.2：其特权访问只用于诊断），报告里只有真实通道与本地判定器的读数。"
            .to_owned(),
        "真实通道（jev / llm）与本地判定器（rule / replay）在 source / live / verification 上分开标注；没有 key 时真实通道整段「未验证」，不拿别的判定器顶替。"
            .to_owned(),
        "判断失败如实计入失败数并写进调用记录（不丢弃、不静默重试）；失败的问题是「没答上来的问题」，下一轮按同一调度规则再请求。"
            .to_owned(),
        "轨迹与证据日志逐任务落盘，报告里的 traces_dir / evidence_dir / calls_log 是绝对路径；command 字段带本报告用到判定器的非保密环境前缀，report_environment 记**写报告时**的模型与端点（`recorded_at` 标出是 `run` 还是 `regenerate`，key 只记是否设置）——运行时打到的模型与端点看轨迹（每条判断的 version 与 judge_endpoint）。"
            .to_owned(),
        "结论按 `pairings` 里的配对读：h1 配对固定 H2 口径、h2 配对固定 H1 开关。跨配对读会把交互作用当成主效应——例如「关掉失效传播就恢复不了」只可能在某个重算口径下成立，换口径要重新看那一组配对。"
            .to_owned(),
        "本批每臂独立发起真实调用：同一提示词在四组臂里各调用一次，读数含判定器的抽样波动，不做跨臂的结果共享。"
            .to_owned(),
        "关闭臂（h1off-*）的重查触发只有一条：从没答上来的问题。它不读失效标记，因此「输入变了要重查」这件事在它那里没有触发条件——这正是被消融掉的机制；按固定频率重查属于另一项消融（研究目标 §9.2 第 7 行，H5），不在本批，所以关闭臂不会靠定期重算掩盖差别。"
            .to_owned(),
        "关闭臂仍然登记行动前提（依赖图与事件记录因此可比），只是发出前不读 check_action；两组臂判定的输入、问题、判据与阈值完全相同。"
            .to_owned(),
        "增量臂与全量臂使用同一条输入投影规则（按问题声明的读取对象分组、投影写当前版本），差别只在要不要复算。"
            .to_owned(),
        "独立验收覆盖研究与目标 §5.2 末条「对完成声明建立与实际交付物版本绑定的证据要求」：交付物被取代之后，只按旧版本判定过交付物的运行拿不到验收（任务 A6 就是这一条），而控制器的完成声明在验收里一次都不参与判定。"
            .to_owned(),
        "行动前提门只用 `cause_primary`（真实模型答得稳的那一问）；`premise_holds`（材料是否已确认原因）照常判定与判分，作为判定质量的诊断读数报告，不参与行动前提——真实模型的读数已在批次里如实记录。"
            .to_owned(),
        "真调复现（key 只在环境变量里，不进报告、日志与提交）：`TYPESAFE_API_KEY=… cargo run -p jh-app -- compare --judge jev`；`OPENAI_API_KEY=$DEEPSEEK_API_KEY OPENAI_BASE_URL=https://api.deepseek.com OPENAI_MODEL=deepseek-flash cargo run -p jh-app -- compare --judge llm`；两个判定器一起跑就并排给出四组臂。离线复现把 `--judge` 换成 `rule`（或 `replay --replay var/calls.jsonl`）。"
            .to_owned(),
    ]
}

/// 判定器构造失败时该（判定器 × 臂）的报告：零调用、「未验证」。
fn unavailable_report(name: JudgeName, arm: Arm, error: &Error) -> ArmReport {
    let mut accumulator = Accumulator::new(name, arm);
    accumulator.detail = format!("判定器未构造成功：{error}");
    accumulator.finish()
}

/// 按名字构造判定器。
///
/// 与 M0 的 [`judge::build`] 分开：任务族 A 的隐藏真值按相位组织，模拟真值判定器在这里没有定义，
/// 因此显式拒绝（研究目标 §9.2 也要求它不参展）。
fn build_judge(name: JudgeName, replay_path: &Path) -> Result<Box<dyn Judge>, Error> {
    Ok(match name {
        JudgeName::SimulatedTruth => {
            return Err(Error::TaskSet(
                "模拟真值不参与任务族 A 的对照（研究目标 §9.2：其特权访问只用于诊断）".to_owned(),
            ));
        }
        JudgeName::Rule => Box::new(crate::judge::RuleJudge::new()),
        JudgeName::Replay => Box::new(crate::judge::ReplayJudge::load(replay_path)?),
        JudgeName::Jev => Box::new(crate::judge::JevJudge::from_env()?),
        JudgeName::Llm => Box::new(crate::judge::LlmJudge::from_env()?),
    })
}

/// 单任务端到端运行（`jev run` 用）：跑一个臂、一个任务，返回轨迹。
pub async fn run_single(
    family: &FamilyA,
    task: &FamilyTask,
    name: JudgeName,
    arm: Arm,
    run: &str,
    calls_path: &Path,
    out_dir: &Path,
) -> Result<TaskTrace, Error> {
    std::fs::create_dir_all(out_dir)?;
    let evidence_path = out_dir.join(format!(
        "{}-{}-{}.evidence.jsonl",
        name.cli_name(),
        arm.label(),
        task.id
    ));
    let judge = build_judge(name, calls_path)?;
    let mut calls = CallLog::open(calls_path.to_path_buf())?;
    let mut context = LoopContext {
        family,
        judge: judge.as_ref(),
        channel: name,
        arm,
        run: run.to_owned(),
        endpoint: endpoint_for(name),
        calls: &mut calls,
    };
    run_task(&mut context, task, &evidence_path).await
}

/// 分题读数的一行：`(一致, 边界读数, Brier, 对数损失)`。
type QuestionRow = (bool, bool, f64, Option<f64>);

/// 汇总器：把轨迹折成一组臂的读数。
struct Accumulator {
    name: JudgeName,
    arm: Arm,
    detail: String,
    succeeded_calls: u32,
    failed_calls: u32,
    attempted_calls: u32,
    call_errors: BTreeMap<String, u32>,
    latencies_ms: Vec<u64>,
    versions: BTreeSet<String>,
    usage_calls: u32,
    usage_seen: bool,
    input_tokens: u64,
    output_tokens: u64,
    asked_questions: u32,
    asks: u32,
    input_bytes: u64,
    reuse_events: u32,
    stale_reuse_events: u32,
    opportunities: u32,
    executions: u32,
    stale_basis: u32,
    wrong_actions: u32,
    blocked: u32,
    gate_unsatisfied: u32,
    abstentions: u32,
    action_cost: f64,
    marked_nodes: u32,
    marked_actions: u32,
    missed: u32,
    extra: u32,
    unrelated_preserved: u32,
    judgments: u32,
    agreements: u32,
    boundaries: u32,
    unscored: u32,
    brier: Vec<f64>,
    log_loss: Vec<f64>,
    log_loss_undefined: u32,
    per_question: BTreeMap<(QuestionId, QuestionVersion), Vec<QuestionRow>>,
    required_actions: u32,
    recovered: u32,
    unrecovered: Vec<String>,
    recovery_rounds: Vec<u32>,
    recovery_calls: Vec<u32>,
    recovery_wall_ms: Vec<u64>,
    acceptance_passed: u32,
    acceptance_failures: Vec<String>,
    tasks: Vec<TaskSummary>,
}

impl Accumulator {
    fn new(name: JudgeName, arm: Arm) -> Self {
        Self {
            name,
            arm,
            detail: String::new(),
            succeeded_calls: 0,
            failed_calls: 0,
            attempted_calls: 0,
            call_errors: BTreeMap::new(),
            latencies_ms: Vec::new(),
            versions: BTreeSet::new(),
            usage_calls: 0,
            usage_seen: false,
            input_tokens: 0,
            output_tokens: 0,
            asked_questions: 0,
            asks: 0,
            input_bytes: 0,
            reuse_events: 0,
            stale_reuse_events: 0,
            opportunities: 0,
            executions: 0,
            stale_basis: 0,
            wrong_actions: 0,
            blocked: 0,
            gate_unsatisfied: 0,
            abstentions: 0,
            action_cost: 0.0,
            marked_nodes: 0,
            marked_actions: 0,
            missed: 0,
            extra: 0,
            unrelated_preserved: 0,
            judgments: 0,
            agreements: 0,
            boundaries: 0,
            unscored: 0,
            brier: Vec::new(),
            log_loss: Vec::new(),
            log_loss_undefined: 0,
            per_question: BTreeMap::new(),
            required_actions: 0,
            recovered: 0,
            unrecovered: Vec::new(),
            recovery_rounds: Vec::new(),
            recovery_calls: Vec::new(),
            recovery_wall_ms: Vec::new(),
            acceptance_passed: 0,
            acceptance_failures: Vec::new(),
            tasks: Vec::new(),
        }
    }

    /// 折入一个任务的轨迹。
    fn absorb(
        &mut self,
        task: &FamilyTask,
        trace: &TaskTrace,
        trace_path: &str,
        evidence_path: &str,
        family: &FamilyA,
    ) {
        self.opportunities += trace.rounds as u32;

        // 判断请求与判分读数。
        let mut failures = 0;
        for ask in &trace.asks {
            self.attempted_calls += 1;
            self.asks += 1;
            self.asked_questions += ask.questions.len() as u32;
            self.input_bytes += ask.input_bytes as u64;
            self.latencies_ms.push(ask.latency_ms);
            match &ask.version {
                Some(version) => {
                    self.succeeded_calls += 1;
                    self.versions.insert(version.clone());
                }
                None => {
                    self.failed_calls += 1;
                    failures += 1;
                    *self.call_errors.entry(ask.outcome.clone()).or_insert(0) += 1;
                }
            }
            if ask.input_tokens.is_some() || ask.output_tokens.is_some() {
                self.usage_calls += 1;
                self.usage_seen = true;
                self.input_tokens += ask.input_tokens.unwrap_or(0);
                self.output_tokens += ask.output_tokens.unwrap_or(0);
            }
        }

        for judgment in &trace.judgments {
            self.judgments += 1;
            match judgment.agreement {
                Some(true) => self.agreements += 1,
                Some(false) => {}
                None => self.unscored += 1,
            }
            if boundary(&judgment.answer) {
                self.boundaries += 1;
            }
            if let Some(brier) = judgment.brier {
                self.brier.push(brier);
            }
            match judgment.log_loss {
                Some(loss) => self.log_loss.push(loss),
                None => self.log_loss_undefined += 1,
            }
            self.per_question
                .entry((judgment.question.clone(), judgment.question_version))
                .or_default()
                .push((
                    judgment.agreement.unwrap_or(false),
                    boundary(&judgment.answer),
                    judgment.brier.unwrap_or(0.0),
                    judgment.log_loss,
                ));
        }

        for reuse in &trace.reuses {
            self.reuse_events += 1;
            if !reuse.fresh {
                self.stale_reuse_events += 1;
            }
        }

        // 失效传播读数：逐条扰动对照声明的读取范围。
        let mut node_question: BTreeMap<NodeId, QuestionId> = BTreeMap::new();
        for judgment in &trace.judgments {
            node_question.insert(judgment.node, judgment.question.clone());
        }
        for step in &trace.steps {
            self.marked_nodes += step.invalidation.stale.len() as u32;
            self.marked_actions += step.invalidation.blocked.len() as u32;
            let marked: BTreeSet<&QuestionId> = step
                .invalidation
                .stale
                .iter()
                .filter_map(|node| node_question.get(node))
                .collect();
            let object = step.change.object();
            for tracked in &task.tracked {
                // 只看这一步**之前**已经落过的判断：这一轮新问出来的节点不可能被本轮扰动标掉，
                // 把它们算进来会把「当时根本没有可失效的判断」误记成漏失效。
                let has_nodes = trace.judgments.iter().any(|judgment| {
                    judgment.question == tracked.question && judgment.round < step.round
                });
                if !has_nodes {
                    continue;
                }
                let declared = family
                    .scope(&tracked.question)
                    .map(<[jh_core::ObjectId]>::to_vec)
                    .unwrap_or_default();
                let touched = declared.contains(object);
                let hit = marked.contains(&tracked.question);
                if touched && !hit {
                    self.missed += 1;
                }
                if !touched && hit {
                    self.extra += 1;
                }
                if !touched && !hit {
                    self.unrelated_preserved += 1;
                }
            }
        }

        // 行动决策。
        let mut executions: Vec<String> = Vec::new();
        let mut wrong: Vec<String> = Vec::new();
        let mut stale: Vec<String> = Vec::new();
        for decision in &trace.decisions {
            match decision.outcome {
                DecisionOutcome::Executed => {
                    self.executions += 1;
                    self.action_cost += decision.cost_units;
                    executions.push(format!("第 {} 轮 {}", decision.round, decision.action));
                    if !decision.basis_current {
                        self.stale_basis += 1;
                        stale.push(format!("第 {} 轮 {}", decision.round, decision.action));
                    }
                    if !task.justified_at_round(decision.round, &decision.action) {
                        self.wrong_actions += 1;
                        wrong.push(format!("第 {} 轮 {}", decision.round, decision.action));
                    }
                }
                DecisionOutcome::Blocked => self.blocked += 1,
                DecisionOutcome::GateUnsatisfied => self.gate_unsatisfied += 1,
                DecisionOutcome::Unresolved => self.abstentions += 1,
            }
        }

        // 恢复延迟。
        let executions_by_action: BTreeMap<ActionId, usize> = trace
            .executions()
            .into_iter()
            .map(|(round, action)| (action, round))
            .collect();
        let mut unrecovered = Vec::new();
        let mut recovery_rounds = Vec::new();
        for action in task.required_actions() {
            self.required_actions += 1;
            let Some(first) = task.first_required_round(&action) else {
                continue;
            };
            match executions_by_action.get(&action) {
                Some(round) if *round >= first => {
                    self.recovered += 1;
                    recovery_rounds.push((round - first) as u32);
                    self.recovery_rounds.push((round - first) as u32);
                    self.recovery_calls.push(count_asks(trace, first, *round));
                    self.recovery_wall_ms
                        .push(recovery_wall_ms(task, trace, first, *round));
                }
                _ => {
                    unrecovered.push(format!("{action}（第 {first} 轮起被要求）"));
                    self.unrecovered
                        .push(format!("{}：{action}（第 {first} 轮起被要求）", task.id));
                }
            }
        }

        // 独立验收（跑完的轨迹必然带上结果）。
        let acceptance = trace
            .acceptance
            .clone()
            .expect("run_task 结束前一定写进独立验收结果");
        let acceptance_passed = acceptance.passed;
        if acceptance_passed {
            self.acceptance_passed += 1;
        } else {
            self.acceptance_failures.push(format!(
                "{}：{}",
                task.id,
                acceptance
                    .checks
                    .iter()
                    .filter(|check| !check.passed)
                    .map(|check| format!("{}（{}）", check.name, check.detail))
                    .collect::<Vec<_>>()
                    .join("；")
            ));
        }

        self.tasks.push(TaskSummary {
            task: task.id.clone(),
            opportunities: trace.rounds as u32,
            executions,
            wrong_actions: wrong,
            stale_basis: stale,
            unrecovered,
            recovery_rounds,
            asks: trace.asks.len() as u32,
            reuse_events: trace.reuses.len() as u32,
            judge_failures: failures,
            judgments: trace.judgments.len() as u32,
            agreements: trace
                .judgments
                .iter()
                .filter(|judgment| judgment.agreement == Some(true))
                .count() as u32,
            acceptance_passed,
            acceptance_checks: acceptance
                .checks
                .iter()
                .map(|check| format!("{}={}", check.name, check.passed))
                .collect(),
            trace: trace_path.to_owned(),
            evidence: evidence_path.to_owned(),
        });
    }

    fn finish(self) -> ArmReport {
        let mean = |values: &[f64]| -> Option<f64> {
            (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
        };
        let reuse_total = self.reuse_events + self.asks;

        ArmReport {
            judge: self.name.cli_name().to_owned(),
            source: self.name.source(),
            live: self.name.live(),
            arm: self.arm.label().to_owned(),
            arm_description: self.arm.description().to_owned(),
            h1_propagation: self.arm.uses_propagation(),
            h2_recompute: self.arm.recompute().label().to_owned(),
            verification: self.verification(),
            versions: self.versions.iter().cloned().collect(),
            calls: CallStats {
                attempted: self.attempted_calls,
                succeeded: self.succeeded_calls,
                failed: self.failed_calls,
                errors: self.call_errors.clone(),
            },
            latency_ms: LatencyStats {
                samples: self.latencies_ms.len() as u32,
                min: self.latencies_ms.iter().copied().min(),
                p50: crate::metrics::percentile(&self.latencies_ms, 0.5),
                p95: crate::metrics::percentile(&self.latencies_ms, 0.95),
                max: self.latencies_ms.iter().copied().max(),
            },
            usage: UsageStats {
                calls_with_usage: self.usage_calls,
                input_tokens: self.usage_seen.then_some(self.input_tokens),
                output_tokens: self.usage_seen.then_some(self.output_tokens),
            },
            decisions: DecisionStats {
                opportunities: self.opportunities,
                executions: self.executions,
                stale_basis_commitments: self.stale_basis,
                stale_basis_action_rate: (self.opportunities > 0)
                    .then(|| f64::from(self.stale_basis) / f64::from(self.opportunities)),
                wrong_actions: self.wrong_actions,
                blocked: self.blocked,
                gate_unsatisfied: self.gate_unsatisfied,
                abstentions: self.abstentions,
                action_cost_units: self.action_cost,
            },
            recompute: RecomputeStats {
                asks: self.asks,
                asked_questions: self.asked_questions,
                judged_nodes: self.judgments,
                reuse_events: self.reuse_events,
                stale_reuse_events: self.stale_reuse_events,
                cache_reuse_rate: (reuse_total > 0)
                    .then(|| f64::from(self.reuse_events) / f64::from(reuse_total)),
                input_bytes: self.input_bytes,
            },
            invalidation: InvalidationStats {
                marked_nodes: self.marked_nodes,
                marked_actions: self.marked_actions,
                missed: self.missed,
                extra: self.extra,
                unrelated_preserved: self.unrelated_preserved,
            },
            quality: QualityStats {
                judgments: self.judgments,
                agreements: self.agreements,
                agreement_rate: (self.judgments > 0)
                    .then(|| f64::from(self.agreements) / f64::from(self.judgments)),
                unscored: self.unscored,
                boundary_readings: self.boundaries,
                brier_mean: mean(&self.brier),
                log_loss_mean: mean(&self.log_loss),
                log_loss_undefined: self.log_loss_undefined,
                score_mae: None,
                score_mae_samples: 0,
                per_question: self
                    .per_question
                    .iter()
                    .map(|((question, version), rows)| QuestionQuality {
                        question: question.clone(),
                        version: *version,
                        judgments: rows.len() as u32,
                        agreements: rows.iter().filter(|row| row.0).count() as u32,
                        brier_mean: mean(&rows.iter().map(|row| row.2).collect::<Vec<_>>()),
                        log_loss_mean: mean(
                            &rows.iter().filter_map(|row| row.3).collect::<Vec<_>>(),
                        ),
                        log_loss_undefined: rows.iter().filter(|row| row.3.is_none()).count()
                            as u32,
                    })
                    .collect(),
            },
            acceptance: AcceptanceStats {
                passed: self.acceptance_passed,
                tasks: self.tasks.len() as u32,
                failures: self.acceptance_failures.clone(),
            },
            recovery: RecoveryStats {
                required_actions: self.required_actions,
                recovered: self.recovered,
                unrecovered: self.unrecovered.clone(),
                rounds: self.recovery_rounds.clone(),
                calls: self.recovery_calls.clone(),
                wall_ms: self.recovery_wall_ms.clone(),
            },
            tasks: self.tasks.clone(),
        }
    }

    /// 验证状态：真实通道有成功调用才是「已验证」；本地判定器记「模拟」；全败记「未验证」。
    fn verification(&self) -> Verification {
        let note = if !self.detail.is_empty() {
            format!("（{}）", self.detail)
        } else {
            String::new()
        };
        let failed_note = if self.failed_calls > 0 {
            format!("，另失败 {} 次", self.failed_calls)
        } else {
            String::new()
        };

        match (self.name.live(), self.succeeded_calls > 0) {
            (true, true) => Verification {
                status: VerificationStatus::Verified,
                detail: format!(
                    "真实调用成功 {} 次{failed_note}{note}",
                    self.succeeded_calls
                ),
            },
            (true, false) => Verification {
                status: VerificationStatus::NotVerified,
                detail: format!(
                    "真实调用未成功（{} 次尝试全部失败）：相关结论保持未验证{note}",
                    self.attempted_calls
                ),
            },
            (false, true) => Verification {
                status: VerificationStatus::Simulated,
                detail: format!(
                    "本地判定器：没有真实调用，读数只作对照与上限估计{failed_note}{note}"
                ),
            },
            (false, false) => Verification {
                status: VerificationStatus::NotVerified,
                detail: format!(
                    "本地判定器 {} 次调用全部失败：本次没有任何可用读数{note}",
                    self.attempted_calls
                ),
            },
        }
    }
}

/// 恢复所需判断调用数：从被要求那一轮到恢复那一轮之间的请求次数。
fn count_asks(trace: &TaskTrace, first: usize, recovered: usize) -> u32 {
    trace
        .asks
        .iter()
        .filter(|ask| ask.round >= first && ask.round <= recovered)
        .count() as u32
}

/// 恢复所需墙钟毫秒：从「破坏前提的事件」到动作发出的时间。
///
/// 起点取「该动作首次被要求那一轮的决策相位之前最后一条扰动」；若这个要求从初始快照就成立
/// （脚本里没有可起算的破坏性事件），起点回落到本次任务运行的开始时刻——记 0 会被读成
/// 「恢复没花时间」，与事实不符。
fn recovery_wall_ms(task: &FamilyTask, trace: &TaskTrace, first: usize, recovered: usize) -> u64 {
    let decision_phase = task.decision_phase(first);
    let from = trace
        .steps
        .iter()
        .filter(|step| step.phase <= decision_phase)
        .max_by_key(|step| step.phase)
        .map(|step| step.at_unix_ms)
        .unwrap_or_else(|| run_start(trace));
    let executed = trace
        .decisions
        .iter()
        .find(|decision| {
            decision.outcome == DecisionOutcome::Executed && decision.round == recovered
        })
        .map(|decision| decision.at_unix_ms);
    match executed {
        Some(to) => to.saturating_sub(from),
        None => 0,
    }
}

/// 本次任务运行的开始时刻：优先用轨迹自己记的；早期轨迹没有这个字段时用第一次请求的时刻兜底。
fn run_start(trace: &TaskTrace) -> u64 {
    if trace.started_at_unix_ms > 0 {
        return trace.started_at_unix_ms;
    }
    trace
        .asks
        .iter()
        .map(|ask| ask.at_unix_ms)
        .min()
        .unwrap_or(0)
}

/// 判断读数是否为边界读数（并列最大概率 / noul 恰好 0.5）。
fn boundary(answer: &jh_core::Answer) -> bool {
    crate::metrics::reading(answer).is_some_and(|reading| reading.boundary)
}

/// 生成新的运行标识：`h1h2-<Unix 毫秒>`。
pub fn new_run_id() -> String {
    format!("h1h2-{}", unix_ms())
}

/// 写一张报告（父目录一并创建）。
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), Error> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    let mut text = serde_json::to_string_pretty(value)?;
    text.push('\n');
    std::fs::write(path, text)?;
    Ok(())
}

impl CompareReport {
    /// 终端摘要行。
    pub fn summary_lines(&self) -> Vec<String> {
        let mut lines = vec![
            format!(
                "任务族 {}：{} 个任务（{}）",
                self.family.name, self.family.tasks, self.family.source
            ),
            "臂 = H1（依赖失效传播）× H2（增量 / 全量）；判定器与臂逐行给出读数".to_owned(),
        ];
        for arm in &self.arms {
            lines.push(format!(
                "{} × {}（{}）：验收 {}/{}｜旧依据行动率 {}（{} 次）｜错误动作 {}｜重算节点 {}｜复用率 {}｜恢复 {}/{} 个动作｜调用 {}/{} 失败 {}",
                arm.judge,
                arm.arm,
                arm.arm_description,
                arm.acceptance.passed,
                arm.acceptance.tasks,
                format_ratio(arm.decisions.stale_basis_action_rate),
                arm.decisions.stale_basis_commitments,
                arm.decisions.wrong_actions,
                arm.recompute.judged_nodes,
                format_ratio(arm.recompute.cache_reuse_rate),
                arm.recovery.recovered,
                arm.recovery.required_actions,
                arm.calls.attempted,
                arm.calls.succeeded,
                arm.calls.failed,
            ));
            lines.push(format!("   {}", arm.verification.detail));
        }
        lines.push("配对读数（结论按配对读，跨配对是交互作用）：".to_owned());
        for pairing in &self.pairings {
            let fixed = format!("{}（固定）", pairing.fixed);
            lines.push(format!(
                "{}｜{}｜左 {}：验收 {}、恢复 {}、旧依据 {}、错动作 {}、调用 {}、节点 {}、输入 {} 字节、复用 {}｜右 {}：验收 {}、恢复 {}、旧依据 {}、错动作 {}、调用 {}、节点 {}、输入 {} 字节、复用 {}",
                pairing.judge,
                fixed,
                pairing.left.arm,
                pairing.left.acceptance,
                pairing.left.recovered,
                pairing.left.stale_basis_commitments,
                pairing.left.wrong_actions,
                pairing.left.calls,
                pairing.left.judged_nodes,
                pairing.left.input_bytes,
                format_ratio(pairing.left.cache_reuse_rate),
                pairing.right.arm,
                pairing.right.acceptance,
                pairing.right.recovered,
                pairing.right.stale_basis_commitments,
                pairing.right.wrong_actions,
                pairing.right.calls,
                pairing.right.judged_nodes,
                pairing.right.input_bytes,
                format_ratio(pairing.right.cache_reuse_rate),
            ));
        }
        lines.push(format!(
            "写报告时的环境（recorded_at={}）：OPENAI_MODEL={}｜OPENAI_BASE_URL={}｜OPENAI_API_KEY={}｜TYPESAFE_API_KEY={}｜Jev pin={}",
            self.report_environment.recorded_at,
            self.report_environment.openai_model.as_deref().unwrap_or("—"),
            self.report_environment.openai_base_url.as_deref().unwrap_or("—"),
            if self.report_environment.openai_api_key_set { "已设置" } else { "未设置" },
            if self.report_environment.typesafe_api_key_set { "已设置" } else { "未设置" },
            self.report_environment.jev_model,
        ));
        lines.push(format!("轨迹：{}", self.traces_dir));
        lines.push(format!("证据：{}", self.evidence_dir));
        lines.push(format!("调用记录：{}", self.calls_log));
        lines.push(format!("命令：{}", self.command));
        lines
    }
}

fn format_ratio(value: Option<f64>) -> String {
    value.map_or_else(|| "—".to_owned(), |value| format!("{value:.3}"))
}

/// 相对路径转绝对路径（报告里的原始数据位置）。
pub fn absolute(path: &Path) -> String {
    if path.is_absolute() {
        return path.display().to_string();
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(path).display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}

/// 调用记录里该运行的行数（报告复现时核对调用数与落盘一致）。
pub fn logged_calls(path: &Path, run: &str) -> Result<usize, Error> {
    let entries: Vec<CallEntry> = crate::calls::read_entries(path)?
        .into_iter()
        .filter(|entry| entry.run() == run)
        .collect();
    Ok(entries.len())
}

/// 从已有轨迹重建报告（`jev compare --regenerate`）。
///
/// 用途：报告口径与 notes 变了、但真调不该重跑时，用当前代码从**同一批轨迹与证据日志**重新汇出报告；
/// 判定器一次都不调用，读数只来自 `traces_dir` 里的轨迹文件。缺哪个（判定器 × 臂 × 任务）的轨迹就
/// 显式失败，不拿别的运行的产物顶替。
pub fn regenerate(family: &FamilyA, options: &RegenerateOptions) -> Result<CompareOutcome, Error> {
    let tasks: Vec<&FamilyTask> = match options.max_tasks {
        Some(limit) => family.tasks.iter().take(limit).collect(),
        None => family.tasks.iter().collect(),
    };
    let traces_dir = options.out_dir.join("traces");
    let evidence_dir = options.out_dir.join("evidence");
    let mut reports = Vec::new();
    let mut clean = true;

    for name in &options.judges {
        for arm in &options.arms {
            let mut accumulator = Accumulator::new(*name, *arm);
            for task in &tasks {
                let stem = format!("{}-{}-{}", name.cli_name(), arm.label(), task.id);
                let trace_path = traces_dir.join(format!("{stem}.json"));
                let text = std::fs::read_to_string(&trace_path).map_err(|error| {
                    Error::TaskSet(format!(
                        "重建报告需要轨迹文件 {}（{error}）：请确认 --out-dir 指向该次运行的目录",
                        trace_path.display()
                    ))
                })?;
                let trace: TaskTrace = serde_json::from_str(&text).map_err(|error| {
                    Error::TaskSet(format!("轨迹 {} 无法解析：{error}", trace_path.display()))
                })?;
                if trace.asks.iter().any(|ask| ask.outcome != ASK_SUCCEEDED) {
                    clean = false;
                }
                accumulator.absorb(
                    task,
                    &trace,
                    &absolute(&trace_path),
                    &absolute(&evidence_dir.join(format!("{stem}.jsonl"))),
                    family,
                );
            }
            reports.push(accumulator.finish());
        }
    }

    let pairings = pairings(&reports);
    Ok(CompareOutcome {
        report: CompareReport {
            run: options.run.clone(),
            generated_at_unix_ms: unix_ms(),
            command: options.command.clone(),
            family: FamilySummary {
                name: family.name.clone(),
                source: family.source.clone(),
                tasks: tasks.len(),
                rounds: tasks
                    .iter()
                    .map(|task| (task.id.clone(), task.rounds))
                    .collect(),
                questions: family.history().to_vec(),
            },
            metric_definitions: CompareMetricDefinitions::declared(),
            arms: reports,
            pairings,
            report_environment: report_environment("regenerate"),
            traces_dir: absolute(&traces_dir),
            evidence_dir: absolute(&evidence_dir),
            calls_log: absolute(&options.calls_path),
            notes: {
                let mut notes = declared_notes();
                notes.push(format!(
                    "本报告由 `compare --regenerate` 从 {} 里的轨迹重建（判定器一次都没调用）：读数与那次运行的调用记录一一对应，notes 与字段取当前代码；`report_environment.recorded_at = regenerate` 说明它的取值只是**重建时刻**的 shell 环境，那次运行真正用的模型与端点看轨迹（每条判断的 version 是模型，judge_endpoint 是端点；早期轨迹没有 judge_endpoint 字段时会为空）。",
                    absolute(&traces_dir)
                ));
                notes
            },
        },
        clean,
    })
}

/// 重建报告的参数。
#[derive(Debug, Clone)]
pub struct RegenerateOptions {
    /// 要重建的判定器。
    pub judges: Vec<JudgeName>,
    /// 要重建的臂。
    pub arms: Vec<Arm>,
    /// 运行标识（写进报告）。
    pub run: String,
    /// 该次运行的产物目录（轨迹与证据日志都从它下面读）。
    pub out_dir: PathBuf,
    /// 调用记录路径（只写进报告，不读）。
    pub calls_path: PathBuf,
    /// 复现命令。
    pub command: String,
    /// 只重建前 N 个任务。
    pub max_tasks: Option<usize>,
}
