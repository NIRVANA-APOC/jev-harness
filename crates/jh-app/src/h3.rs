//! H3 对照：**固定问题集合** vs **允许精化**，外加任务族 B 的 B1 / B2 分开报告
//! （`docs/RESEARCH_GOALS.md` §9.2 的消融行、§9.3、§10.1、§10.2）。
//!
//! # 一条命令出报告
//!
//! ```text
//! TYPESAFE_API_KEY=… OPENAI_API_KEY=$DEEPSEEK_API_KEY OPENAI_BASE_URL=https://api.deepseek.com \
//!   OPENAI_MODEL=deepseek-flash \
//!   cargo run -p jh-app -- h3 --judge jev --judge llm --out var/h3/<run>.json
//! ```
//!
//! 报告里带原始数据位置（轨迹、证据日志、调用记录、候选产物）与完整命令行；`--candidates <file>` 可以
//! 复用已落盘的候选产物重跑对照（不重发生成调用）。
//!
//! # 固定下游消费协议（§6.4）
//!
//! 两臂共用同一段循环代码、同一套前提门与同一批任务、初始证据、判定器配置与决策时机；差别只有
//! **问题库当前启用的版本**（[`FamilyBQuestions`] 的覆盖表）：
//!
//! - 固定臂：只有 v1（固定问题集），生成出来的候选停在候选 / 影子状态；
//! - 精化臂：达标的候选被发布成 v2，循环读到的就是它的定义与读取范围。
//!
//! 不修改消费者代码、决策器提示、权重或控制规则；每次晋升都记「问题 / 输入投影 / 消费者 / 策略」
//! 各自是否改变（[`crate::promotion::FourWayChange`]），并逐项核边界（用户目标、验收要求、工具权限、
//! 控制规则）。
//!
//! # 成本的记法
//!
//! 生成、验证、影子、开发案例与两臂的模型调用全部按 `run` 标识记账（调用记录 JSONL），收益读数旁边
//! 并排列出这些成本（§6.5：收益须扣除生成、验证、后续评估与维护成本）；货币成本不虚构——只用调用数、
//! token 与延迟这三个可核的量。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use jh_core::{
    ActionId, Answer, Attribution, Counterexample, CounterexampleLog, EvidenceId, EvidenceLog,
    FailureSource, JsonlCounterexampleLog, Judge, JudgeSource, NewCounterexample, ObjectId,
    QuestionId, QuestionLibrary, QuestionVersion, SemanticState,
};
use serde::Serialize;

use crate::calls::{CallEntry, CallLog};
use crate::compare::{
    ReportEnvironment, absolute, build_judge, endpoint_for, logged_calls, report_environment,
    write_json,
};
use crate::error::Error;
use crate::exp::unix_ms;
use crate::family::accept;
use crate::family_b::{ABSENT_LABEL, FamilyB, FamilyBTask, Pair, Subset};
use crate::generation::{
    CandidateQuestion, GeneratedCandidate, GenerationRequest, Generator, check_contract,
};
use crate::judge::JudgeName;
use crate::matching::{ASK_SUCCEEDED, Arm, DecisionOutcome, LoopContext, TaskTrace, run_task};
use crate::metrics;
use crate::promotion;
use crate::report::{CallStats, LatencyStats, UsageStats, Verification, VerificationStatus};
use crate::shadow::{self, ShadowReadings};
use crate::validation::{self, ValidationRun};

/// 一次 H3 运行的参数。
#[derive(Debug, Clone)]
pub struct H3Options {
    /// 要跑的判定器；第一个同时用作精化循环（开发案例、验证、影子）的判定器。
    pub judges: Vec<JudgeName>,
    /// 运行标识。
    pub run: String,
    /// 调用记录落盘路径（JSONL，追加）。
    pub calls_path: PathBuf,
    /// 录制回放读取的记录文件。
    pub replay_path: PathBuf,
    /// 产物目录（轨迹、证据、候选产物都落在这里）。
    pub out_dir: PathBuf,
    /// 复现命令，原样写进报告。
    pub command: String,
    /// 只跑前 N 个未见变体（`None` 表示全部）。
    pub max_tasks: Option<usize>,
    /// 候选来源：真实生成，或复用已落盘的候选产物。
    pub candidates: CandidateSource,
}

/// 候选问题的来源。
#[derive(Debug, Clone)]
pub enum CandidateSource {
    /// 真实生成模型按反例记录生成。
    Generate,
    /// 复用已落盘的候选产物（不重发生成调用）。
    File(PathBuf),
}

/// H3 运行的结果。
#[derive(Debug, Clone)]
pub struct H3Outcome {
    /// 报告。
    pub report: H3Report,
    /// 是否每一条真实调用都成功（判断、生成、影子、验证）。
    pub clean: bool,
}

/// H3 报告的顶层结构。
#[derive(Debug, Clone, Serialize)]
pub struct H3Report {
    /// 运行标识。
    pub run: String,
    /// 生成时刻。
    pub generated_at_unix_ms: u64,
    /// 复现命令。
    pub command: String,
    /// 协议声明（两臂、口径、预声明的晋升标准与边界）。
    pub protocol: ProtocolDeclaration,
    /// 任务族摘要。
    pub family: FamilySummary,
    /// 写报告时的非保密环境。
    pub environment: ReportEnvironment,
    /// 生成步骤。
    pub generation: GenerationSection,
    /// 验证步骤。
    pub validation: ValidationSection,
    /// 影子运行。
    pub shadow: ShadowSection,
    /// 晋升与回退。
    pub promotion: PromotionSection,
    /// 判定器 × 臂读数。
    pub arms: Vec<ArmReport>,
    /// 精化收益（扣除成本之后的净读数）。
    pub gains: GainsSection,
    /// 逐任务的原始位置与摘要。
    pub raw: RawPaths,
    /// 成本账。
    pub cost: CostSection,
    /// 口径与限制说明。
    pub notes: Vec<String>,
}

/// 报告里的协议声明。
#[derive(Debug, Clone, Serialize)]
pub struct ProtocolDeclaration {
    /// H3 的两臂。
    pub arms: Vec<String>,
    /// 固定下游消费协议。
    pub consumption_protocol: String,
    /// 问题 / 输入投影 / 消费者 / 策略四路的记法。
    pub change_record: String,
    /// 边界（§6.4）。
    pub boundaries: Vec<String>,
    /// 预声明的晋升标准。
    pub promotion_criteria: Vec<crate::promotion::CriterionOutcome>,
    /// 预声明的阈值。
    pub thresholds: promotion::Thresholds,
    /// 材料充分性口径。
    pub material_sufficiency: String,
    /// 指标口径。
    pub metric_definitions: BTreeMap<String, String>,
}

/// 任务族摘要。
#[derive(Debug, Clone, Serialize)]
pub struct FamilySummary {
    /// 名字。
    pub name: String,
    /// 仓库内位置。
    pub source: String,
    /// 三对场景。
    pub pairs: Vec<Pair>,
    /// 开发案例与未见变体。
    pub tasks: Vec<TaskEntrySummary>,
    /// 问题定义（固定版本）。
    pub questions: BTreeMap<String, serde_json::Value>,
}

/// 一个任务的摘要行。
#[derive(Debug, Clone, Serialize)]
pub struct TaskEntrySummary {
    /// 任务标识。
    pub task: String,
    /// 哪一对。
    pub pair: String,
    /// 哪个子集。
    pub subset: String,
    /// 哪一侧。
    pub side: String,
    /// 是否开发案例。
    pub development: bool,
    /// 关键证据对象。
    pub key_object: String,
    /// 候选动作。
    pub actions: Vec<String>,
    /// 轮数。
    pub rounds: usize,
}

/// 生成步骤的报告段。
#[derive(Debug, Clone, Serialize)]
pub struct GenerationSection {
    /// 候选来源（`generate` 或 `file`）。
    pub source: String,
    /// 候选产物文件。
    pub candidates_file: String,
    /// 反例记录文件。
    pub counterexamples_file: String,
    /// 反例条数。
    pub counterexamples: u32,
    /// 逐对的生成结果。
    pub per_pair: Vec<GenerationEntry>,
    /// 生成调用数。
    pub calls: u32,
    /// 失败的生成调用数。
    pub failures: u32,
}

/// 一对场景上的生成结果。
#[derive(Debug, Clone, Serialize)]
pub struct GenerationEntry {
    /// 哪一对。
    pub pair: String,
    /// 生成读数。
    pub call: crate::generation::GenerationCall,
    /// 候选（失败时为 `None`）。
    pub candidate: Option<CandidateQuestion>,
    /// 契约与协议核对。
    pub contract: Option<crate::generation::ContractCheck>,
    /// 反例记录（开发案例的失败现场）。
    pub counterexamples: Vec<Counterexample>,
}

/// 验证步骤的报告段。
#[derive(Debug, Clone, Serialize)]
pub struct ValidationSection {
    /// 逐对的验证读数。
    pub per_pair: Vec<ValidationEntry>,
    /// 固定版本在开发案例上的判别一致率（对照读数）。
    pub fixed_development_rate: Option<f64>,
}

/// 一对场景上的验证结果。
#[derive(Debug, Clone, Serialize)]
pub struct ValidationEntry {
    /// 哪一对。
    pub pair: String,
    /// 被验证的问题与版本。
    pub question: String,
    /// 版本。
    pub version: u32,
    /// 全部用例读数。
    pub run: ValidationRun,
}

/// 影子运行的报告段。
#[derive(Debug, Clone, Serialize)]
pub struct ShadowSection {
    /// 逐对影子读数。
    pub per_pair: Vec<ShadowReadings>,
}

/// 晋升与回退的报告段。
#[derive(Debug, Clone, Serialize)]
pub struct PromotionSection {
    /// 逐对的晋升决定。
    pub decisions: Vec<PromotionDecision>,
    /// 回退演练（有晋升时才有）。
    pub rollback: Option<RollbackRecord>,
    /// 问题库版本状态（发布之后）。
    pub library: Vec<LibraryEntry>,
}

/// 一对场景上的晋升决定。
#[derive(Debug, Clone, Serialize)]
pub struct PromotionDecision {
    /// 哪一对。
    pub pair: String,
    /// 候选版本。
    pub version: Option<u32>,
    /// 是否晋升。
    pub promoted: bool,
    /// 逐条判据结果。
    pub criteria: Vec<promotion::CriterionOutcome>,
    /// 决定说明。
    pub reason: String,
    /// 四路变更。
    pub change: Option<promotion::FourWayChange>,
    /// 边界核对。
    pub boundaries: Vec<promotion::BoundaryCheck>,
    /// 发布联动失效读数。
    pub invalidation: Option<promotion::InvalidationSummary>,
    /// 生命周期记录。
    pub lifecycle: Vec<String>,
}

/// 回退演练的记录。
#[derive(Debug, Clone, Serialize)]
pub struct RollbackRecord {
    /// 哪一对。
    pub pair: String,
    /// 从哪个版本回退到哪个版本。
    pub from: u32,
    /// 回到哪个版本。
    pub to: u32,
    /// 联动失效读数。
    pub invalidation: promotion::InvalidationSummary,
    /// 回退之后的版本状态（旧版本都还在）。
    pub versions: Vec<(u32, String)>,
    /// 说明。
    pub note: String,
}

/// 问题库状态的一行。
#[derive(Debug, Clone, Serialize)]
pub struct LibraryEntry {
    /// 问题。
    pub question: String,
    /// 版本与状态。
    pub versions: Vec<(u32, String)>,
    /// 当前启用版本。
    pub enabled: Option<u32>,
}

/// 一组（判定器 × 臂）的读数。
#[derive(Debug, Clone, Serialize)]
pub struct ArmReport {
    /// 判定器。
    pub judge: String,
    /// 来源标签。
    pub source: JudgeSource,
    /// 是否真实调用。
    pub live: bool,
    /// 臂标识（`fixed` / `refined`）。
    pub arm: String,
    /// 这一臂在每个问题上用的版本。
    pub question_versions: BTreeMap<String, u32>,
    /// 验证状态。
    pub verification: Verification,
    /// 调用统计。
    pub calls: CallStats,
    /// 延迟分位数。
    pub latency: LatencyStats,
    /// token 用量。
    pub usage: UsageStats,
    /// 全部任务的合计读数。
    pub totals: ArmTotals,
    /// B1 / B2 分开的读数。
    pub subsets: Vec<SubsetReport>,
    /// 逐任务摘要。
    pub tasks: Vec<TaskSummary>,
}

/// 一臂的合计读数。
#[derive(Debug, Clone, Default, Serialize)]
pub struct ArmTotals {
    /// 任务数。
    pub tasks: u32,
    /// 验收通过与任务数。
    pub acceptance: String,
    /// 必需动作数与恢复数。
    pub recovered: String,
    /// 未恢复的必需动作。
    pub unrecovered: Vec<String>,
    /// 必需动作总数（这一臂的分母）。
    pub required_actions: u32,
    /// 其中真的发出的数量。
    pub recovered_actions: u32,
    /// 不该发出的动作次数。
    pub wrong_actions: u32,
    /// 基于失效前提发出的动作次数。
    pub stale_basis_commitments: u32,
    /// 弃权次数。
    pub abstentions: u32,
    /// 观察动作发出次数。
    pub observation_actions: u32,
    /// 观察成本合计（自定计价单位）。
    pub observation_cost_units: f64,
    /// 动作成本合计。
    pub action_cost_units: f64,
    /// 落盘判断条数。
    pub judgments: u32,
    /// 与隐藏真值一致的判断条数。
    pub agreements: u32,
    /// 材料不足的判断条数。
    pub material_insufficient: u32,
    /// 材料不足却读出候选集内标签的条数。
    pub candidate_label_when_insufficient: u32,
    /// 判断请求次数。
    pub asks: u32,
    /// 输入字节合计。
    pub input_bytes: u64,
    /// 判断失败次数。
    pub judge_failures: u32,
}

/// 一个子集（B1 / B2）的读数。
#[derive(Debug, Clone, Serialize)]
pub struct SubsetReport {
    /// 子集标签。
    pub subset: String,
    /// 子集说明。
    pub description: String,
    /// 一臂在这个子集上的合计。
    pub totals: ArmTotals,
}

/// 一个任务的摘要。
#[derive(Debug, Clone, Serialize)]
pub struct TaskSummary {
    /// 任务标识。
    pub task: String,
    /// 哪一对。
    pub pair: String,
    /// 哪个子集。
    pub subset: String,
    /// 哪一侧。
    pub side: String,
    /// 这一臂在这个任务上用的问题版本。
    pub question_versions: BTreeMap<String, u32>,
    /// 验收是否通过。
    pub acceptance_passed: bool,
    /// 验收逐条判定。
    pub acceptance_checks: Vec<String>,
    /// 发出的动作。
    pub executions: Vec<String>,
    /// 不该发出的动作。
    pub wrong_actions: Vec<String>,
    /// 必需动作与恢复情况。
    pub unrecovered: Vec<String>,
    /// 必需动作总数。
    pub required_actions: u32,
    /// 其中真的发出的数量。
    pub recovered_actions: u32,
    /// 观察动作与成本。
    pub observations: Vec<String>,
    /// 观察动作的计价成本合计。
    pub observation_cost_units: f64,
    /// 全部动作的计价成本合计。
    pub action_cost_units: f64,
    /// 判断请求次数。
    pub asks: u32,
    /// 判断失败次数。
    pub judge_failures: u32,
    /// 判断条数。
    pub judgments: u32,
    /// 一致条数。
    pub agreements: u32,
    /// 材料不足的条数。
    pub material_insufficient: u32,
    /// 材料不足却读出候选集内标签的条数。
    pub candidate_label_when_insufficient: u32,
    /// 轨迹文件。
    pub trace: String,
    /// 证据日志文件。
    pub evidence: String,
}

impl TaskSummary {
    /// 这个任务上判断的一致率（没有判断时为 `None`）。
    pub fn agreement_rate(&self) -> Option<f64> {
        (self.judgments > 0).then(|| f64::from(self.agreements) / f64::from(self.judgments))
    }
}

/// 精化收益（§10.1「未见任务上的精化收益」）：两臂之差与它对应的成本并排给出。
///
/// 本项目不虚构货币价格：收益与成本都用可核的量（验收通过数、错误动作数、调用数、token、
/// 动作计价单位），净读数写成文字，不做「收益 > 成本」的自动判定——那是实验层的结论。
#[derive(Debug, Clone, Serialize)]
pub struct GainsSection {
    /// 逐判定器的收益读数。
    pub per_judge: Vec<GainReading>,
    /// 口径说明。
    pub note: String,
}

/// 一个判定器上的收益读数（精化臂 − 固定臂）。
#[derive(Debug, Clone, Serialize)]
pub struct GainReading {
    /// 判定器。
    pub judge: String,
    /// 验收通过与任务数：固定 → 精化。
    pub acceptance: String,
    /// 验收通过数之差。
    pub acceptance_delta: i32,
    /// 不该发出的动作数：固定 → 精化。
    pub wrong_actions: (u32, u32),
    /// 不该发出的动作数之差。
    pub wrong_actions_delta: i32,
    /// 恢复的必需动作：固定 → 精化。
    pub recovered: (String, String),
    /// 观察动作计价成本：固定 → 精化。
    pub observation_cost_units: (f64, f64),
    /// 判断调用数：固定 → 精化。
    pub calls: (u32, u32),
    /// 精化循环本身的调用数（生成 + 验证 + 影子）。
    pub refinement_calls: u32,
    /// 精化循环本身的 token（通道上报，缺报时为 `None`）。
    pub refinement_tokens: Option<u64>,
    /// 净读数（文字）：收益是什么、代价是什么、哪一条读不出。
    pub net: String,
}

/// 原始数据位置。
#[derive(Debug, Clone, Serialize)]
pub struct RawPaths {
    /// 产物目录。
    pub out_dir: String,
    /// 固定臂轨迹目录。
    pub fixed_traces: String,
    /// 精化臂轨迹目录。
    pub refined_traces: String,
    /// 开发案例轨迹目录。
    pub dev_traces: String,
    /// 影子轨迹目录。
    pub shadow_traces: String,
    /// 调用记录。
    pub calls_log: String,
    /// 候选产物。
    pub candidates_file: String,
    /// 反例记录。
    pub counterexamples_file: String,
    /// 固定臂的问题库（只有 v1）。
    pub fixed_library: String,
    /// 精化臂的问题库（带发布/回退记录与验证记录）。
    pub refined_library: String,
}

/// 成本账：按用途分组的调用读数（货币成本不虚构）。
#[derive(Debug, Clone, Serialize)]
pub struct CostSection {
    /// 逐用途的调用读数。
    pub by_purpose: Vec<PurposeCost>,
    /// 合计。
    pub total: PurposeCost,
    /// 说明。
    pub note: String,
}

/// 一个用途的调用读数。
#[derive(Debug, Clone, Default, Serialize)]
pub struct PurposeCost {
    /// 用途（生成 / 验证 / 影子 / 开发案例 / 固定臂 / 精化臂）。
    pub purpose: String,
    /// 调用次数。
    pub calls: u32,
    /// 成功次数。
    pub succeeded: u32,
    /// 失败次数。
    pub failed: u32,
    /// 输入 token。
    pub input_tokens: Option<u64>,
    /// 输出 token。
    pub output_tokens: Option<u64>,
    /// 延迟中位数。
    pub latency_p50_ms: Option<u64>,
}

/// 空读数（判定器构造失败时用）。
fn empty_totals() -> ArmTotals {
    ArmTotals::default()
}

/// 跑完一次 H3：开发失败 → 生成 → 验证 → 影子 → 晋升 → 两臂对照 → 报告。
pub async fn run(family: &FamilyB, options: &H3Options) -> Result<H3Outcome, Error> {
    let started = unix_ms();
    let cycle_judge = options.judges[0];
    let traces_root = options.out_dir.join("traces");
    let dev_dir = traces_root.join("development");
    let fixed_dir = traces_root.join("fixed");
    let refined_dir = traces_root.join("refined");
    for dir in [&dev_dir, &fixed_dir, &refined_dir] {
        std::fs::create_dir_all(dir)?;
    }
    let calls_path = options.calls_path.clone();
    let mut calls = CallLog::open(calls_path.clone())?;
    let mut clean = true;

    let dev_tasks: Vec<&FamilyBTask> = family.development.iter().collect();
    let variants: Vec<&FamilyBTask> = match options.max_tasks {
        Some(limit) => family.variants.iter().take(limit).collect(),
        None => family.variants.iter().collect(),
    };
    let variants: Vec<&FamilyBTask> = variants;

    // ---- 1. 开发案例上的失败现场（反例提取的材料） --------------------------------
    let cycle_judge_built = build_judge(cycle_judge, &options.replay_path)?;
    let fixed_view = family.question_set();
    let dev_run = run_arm(
        &dev_tasks,
        &fixed_view,
        cycle_judge,
        cycle_judge_built.as_ref(),
        "fixed",
        &format!("{}-dev", options.run),
        &mut calls,
        &dev_dir,
    )
    .await?;
    clean &= dev_run.judge_failures == 0;

    let counterexample_path = options.out_dir.join("counterexamples.jsonl");
    let mut counterexample_log = JsonlCounterexampleLog::open(&counterexample_path)?;
    let mut per_pair_counterexamples: BTreeMap<String, Vec<Counterexample>> = BTreeMap::new();
    let mut failure_facts: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for summary in &dev_run.tasks {
        let entry = family.task(&summary.task).expect("开发案例在族里");
        let facts = dev_facts(family, entry, Path::new(&summary.trace))?;
        let entries = extract_counterexamples(family, entry, Path::new(&summary.trace))?;
        for new_entry in entries {
            let recorded = counterexample_log.append(new_entry)?;
            per_pair_counterexamples
                .entry(entry.pair.clone())
                .or_default()
                .push(recorded);
        }
        failure_facts.insert(entry.pair.clone(), facts);
    }

    // ---- 2. 候选生成 --------------------------------------------------------------
    let (generated, generation_calls, generation_source) = match &options.candidates {
        CandidateSource::Generate => {
            let generator = Generator::from_env()?;
            let requests: Vec<GenerationRequest<'_>> = family
                .pairs
                .iter()
                .map(|pair| GenerationRequest {
                    pair,
                    fixed_question: &family
                        .question(&pair.question)
                        .expect("固定问题存在")
                        .question,
                    dev_case: &pair.dev_case,
                    failure_facts: failure_facts.get(&pair.id).cloned().unwrap_or_default(),
                    counterexamples: per_pair_counterexamples
                        .get(&pair.id)
                        .cloned()
                        .unwrap_or_default(),
                })
                .collect();
            let produced = generator.generate(family, &requests).await?;
            let generation_calls = produced.len() as u32;
            for entry in &produced {
                calls.append(&CallEntry::ModelCall {
                    run: format!("{}-generation", options.run),
                    purpose: "generation".to_owned(),
                    case: entry.pair.clone(),
                    source: JudgeSource::Llm,
                    version: entry.call.version.clone(),
                    request_id: entry.call.request_id.clone(),
                    usage: entry.call.usage.clone(),
                    input_bytes: entry.call.input_bytes,
                    output_bytes: entry.call.output_bytes,
                    latency_ms: entry.call.latency_ms,
                    error: entry.call.error.clone(),
                })?;
                if entry.call.error.is_some() {
                    clean = false;
                }
            }
            (produced, generation_calls, "generate".to_owned())
        }
        CandidateSource::File(path) => {
            let text = std::fs::read_to_string(path).map_err(|error| {
                Error::TaskSet(format!("候选产物 {} 不可读：{error}", absolute(path)))
            })?;
            let mut parsed: Vec<GeneratedCandidate> =
                serde_json::from_str(&text).map_err(|error| {
                    Error::TaskSet(format!("候选产物 {} 无法解析：{error}", absolute(path)))
                })?;
            for entry in &mut parsed {
                entry.call.error = None;
            }
            let generation_calls = parsed.len() as u32;
            (parsed, generation_calls, "file".to_owned())
        }
    };

    let candidates_file = options.out_dir.join("candidates.json");
    write_json(&candidates_file, &generated)?;

    // ---- 3. 问题库：两臂各一份，固定臂只入库，精化臂会带上发布的新版本 ---------------
    let mut fixed_library = QuestionLibrary::default();
    let mut library_state = SemanticState::in_memory();
    promotion::adopt_fixed_set(family, &mut fixed_library, &mut library_state, unix_ms())?;
    let mut refined_library = fixed_library.clone();

    // ---- 4. 逐对：验证 → 影子 → 晋升 ----------------------------------------------
    let thresholds = promotion::declared_thresholds();
    let allowed_consumers = promotion::allowed_consumers();
    let digest_before = promotion::boundary_digest(family)?;

    let mut generation_entries = Vec::new();
    let mut validation_entries = Vec::new();
    let mut shadow_entries = Vec::new();
    let mut decisions = Vec::new();
    let mut rollback_record = None;

    for pair in &family.pairs {
        let produced = generated.iter().find(|entry| entry.pair == pair.id);
        let Some(produced) = produced else {
            generation_entries.push(GenerationEntry {
                pair: pair.id.clone(),
                call: crate::generation::GenerationCall {
                    pair: pair.id.clone(),
                    version: None,
                    request_id: None,
                    usage: None,
                    input_bytes: 0,
                    output_bytes: 0,
                    latency_ms: 0,
                    error: Some("没有为这一对生成候选".to_owned()),
                },
                candidate: None,
                contract: None,
                counterexamples: per_pair_counterexamples
                    .get(&pair.id)
                    .cloned()
                    .unwrap_or_default(),
            });
            decisions.push(PromotionDecision {
                pair: pair.id.clone(),
                version: None,
                promoted: false,
                criteria: Vec::new(),
                reason: "生成步骤没有给出这一对的候选：保留固定问题模式（§12 的未达标处理）"
                    .to_owned(),
                change: None,
                boundaries: Vec::new(),
                invalidation: None,
                lifecycle: Vec::new(),
            });
            continue;
        };

        let contract = produced
            .candidate
            .as_ref()
            .map(|candidate| check_contract(family, pair, candidate, &allowed_consumers));
        generation_entries.push(GenerationEntry {
            pair: pair.id.clone(),
            call: produced.call.clone(),
            candidate: produced.candidate.clone(),
            contract: contract.clone(),
            counterexamples: per_pair_counterexamples
                .get(&pair.id)
                .cloned()
                .unwrap_or_default(),
        });

        let Some(candidate) = produced.candidate.clone() else {
            decisions.push(PromotionDecision {
                pair: pair.id.clone(),
                version: None,
                promoted: false,
                criteria: Vec::new(),
                reason: format!(
                    "生成失败：{}（失败如实记录，不重试）",
                    produced.call.error.clone().unwrap_or_default()
                ),
                change: None,
                boundaries: Vec::new(),
                invalidation: None,
                lifecycle: Vec::new(),
            });
            continue;
        };

        let version = match promotion::propose(&mut refined_library, &candidate, unix_ms()) {
            Ok(version) => version,
            Err(error) => {
                decisions.push(PromotionDecision {
                    pair: pair.id.clone(),
                    version: None,
                    promoted: false,
                    criteria: Vec::new(),
                    reason: format!("候选无法入库：{error}"),
                    change: None,
                    boundaries: Vec::new(),
                    invalidation: None,
                    lifecycle: Vec::new(),
                });
                continue;
            }
        };

        // 验证（开发案例、正例 / 反例、不同表述）
        let spec = jh_core::QuestionSpec {
            version,
            question: candidate.question.clone(),
        };
        let validation_run = validation::run(
            family,
            &spec,
            &pair.question,
            &candidate.reads(),
            cycle_judge_built.as_ref(),
            cycle_judge,
            &format!("{}-validation", options.run),
            &mut calls,
        )
        .await?;
        clean &= validation_run.stats.failed == 0;
        validation_entries.push(ValidationEntry {
            pair: pair.id.clone(),
            question: pair.question.to_string(),
            version: version.0,
            run: validation_run.clone(),
        });
        // 验证记录下来（§6.3 的「验证记录」）：读数按原值进问题库。
        promotion::record_validation(
            &mut refined_library,
            &pair.question,
            version,
            jh_core::ValidationKind::Development,
            BTreeMap::from([
                ("cases".to_owned(), f64::from(validation_run.stats.cases)),
                (
                    "agreement_rate".to_owned(),
                    validation_run.stats.agreement_rate.unwrap_or(0.0),
                ),
                (
                    "positive_rate".to_owned(),
                    validation_run.stats.positive_rate.unwrap_or(0.0),
                ),
                (
                    "negative_rate".to_owned(),
                    validation_run.stats.negative_rate.unwrap_or(0.0),
                ),
            ]),
            "开发案例、正例与反例上的检验（jev h3 的验证步骤）".to_owned(),
            unix_ms(),
        )?;
        promotion::record_validation(
            &mut refined_library,
            &pair.question,
            version,
            jh_core::ValidationKind::UnseenVariant,
            BTreeMap::from([(
                "unseen_wording_rate".to_owned(),
                validation_run
                    .rate_of(validation::CaseKind::Unseen)
                    .unwrap_or(0.0),
            )]),
            "不同表述 / 不同服务的未见用例一致率".to_owned(),
            unix_ms(),
        )?;

        // 影子运行：控制路径用固定问题集，候选版本只判定不进决策。
        // 影子只跑这一对的开发案例：影子读数按对读，别的对的任务在它们自己的影子运行里。
        let dev_entry = family.dev_case(&pair.id).expect("每对一个开发案例");
        promotion::start_shadow(
            &mut refined_library,
            &pair.question,
            version,
            unix_ms(),
            "进入影子评估".to_owned(),
        )?;
        let plan = shadow::plan(
            &pair.question,
            spec.clone(),
            candidate.reads(),
            "候选版本：参与判断、不进决策",
        );
        let shadow_readings = shadow::run(
            family,
            &[dev_entry],
            &family.question_set(),
            &plan,
            cycle_judge_built.as_ref(),
            cycle_judge,
            &format!("{}-shadow", options.run),
            endpoint_for(cycle_judge),
            &mut calls,
            &options.out_dir,
        )
        .await?;
        clean &= shadow::clean(&shadow_readings);
        shadow_entries.push(shadow_readings.clone());
        promotion::record_validation(
            &mut refined_library,
            &pair.question,
            version,
            jh_core::ValidationKind::ShadowRun,
            BTreeMap::from([
                (
                    "toward_justified".to_owned(),
                    f64::from(shadow_readings.stats.toward_justified),
                ),
                (
                    "toward_unjustified".to_owned(),
                    f64::from(shadow_readings.stats.toward_unjustified),
                ),
                (
                    "shadow_judgments".to_owned(),
                    f64::from(shadow_readings.stats.shadow_judgments),
                ),
            ]),
            "影子运行：新问题参与判断、不进决策；记「若采纳会不会改变有价值的行动分支」".to_owned(),
            unix_ms(),
        )?;

        // 晋升判据
        // 区分口径：**材料足够**的判断里读出「正确一侧」的比例——固定版本一次都没读到关键证据，
        // 按 0 记（它区分不了），不是「没有读数」。
        let dev_rate = discrimination_rate(
            shadow_readings
                .tasks
                .iter()
                .filter(|reading| reading.pair == pair.id)
                .map(|reading| (reading.side_matches, reading.side_sufficient)),
        );
        let fixed_dev_rate = dev_run
            .tasks
            .iter()
            .find(|summary| summary.task == pair.dev_case)
            .map(|summary| {
                side_rate_of(Path::new(&summary.trace), &dev_entry.side, &pair.key_object)
            });
        let input = promotion::CriteriaInput {
            contract_passed: contract.as_ref().is_some_and(|check| check.passed),
            contract_failures: contract
                .as_ref()
                .map(|check| {
                    check
                        .checks
                        .iter()
                        .filter(|item| !item.passed)
                        .map(|item| item.name.clone())
                        .collect()
                })
                .unwrap_or_default(),
            development_rate: dev_rate,
            development_rate_fixed: fixed_dev_rate,
            positive_rate: validation_run.rate_of(validation::CaseKind::Positive),
            negative_rate: validation_run.rate_of(validation::CaseKind::Negative),
            unseen_rate: validation_run.rate_of(validation::CaseKind::Unseen),
            toward_justified: shadow_readings
                .tasks
                .iter()
                .filter(|reading| reading.pair == pair.id)
                .map(|reading| {
                    reading
                        .branch_changes
                        .iter()
                        .filter(|branch| {
                            branch.hypothetical == DecisionOutcome::Executed
                                && branch.toward_justified
                        })
                        .count() as u32
                })
                .sum(),
            toward_unjustified: shadow_readings
                .tasks
                .iter()
                .filter(|reading| reading.pair == pair.id)
                .map(|reading| {
                    reading
                        .branch_changes
                        .iter()
                        .filter(|branch| {
                            branch.hypothetical == DecisionOutcome::Executed
                                && !branch.toward_justified
                        })
                        .count() as u32
                })
                .sum(),
            shadow_failures: shadow_readings.stats.shadow_failures,
        };
        let criteria = promotion::evaluate(&input, &thresholds);
        let promoted = criteria.iter().all(|criterion| criterion.passed);

        let change = promotion::four_way_change(
            family,
            pair,
            QuestionVersion(1),
            version,
            &refined_library,
            Some(&candidate),
        );
        let digest_after = promotion::boundary_digest(family)?;
        let boundaries =
            promotion::check_boundaries(&digest_before, &digest_after, family, &candidate);

        if !promoted {
            decisions.push(PromotionDecision {
                pair: pair.id.clone(),
                version: Some(version.0),
                promoted: false,
                criteria,
                reason: "未达到预先声明的晋升标准：候选留在候选 / 影子状态，保留固定问题模式（研究目标 §12 的未达标处理）".to_owned(),
                change: Some(change),
                boundaries,
                invalidation: None,
                lifecycle: Vec::new(),
            });
            continue;
        }

        // 发布（晋升）：在「真有判断」的状态上联动失效
        let dev_entry = family.dev_case(&pair.id).expect("每对一个开发案例");
        let (mut promote_state, _) = promotion::state_with_judgment(
            &dev_entry.task,
            &pair.question,
            QuestionVersion(1),
            dev_answer(&dev_run, &pair.dev_case, &pair.question)
                .unwrap_or(Answer::Noul(jh_core::NoulAnswer { noul: 1.0 })),
            &pair.remedies[&dev_entry.side],
            cycle_judge.source(),
        )?;
        let (records, invalidation) = promotion::publish(
            &mut refined_library,
            &mut promote_state,
            &pair.question,
            version,
            unix_ms(),
            format!("预先声明的晋升标准全部达标（判据 {} 条）", criteria.len()),
        )?;
        decisions.push(PromotionDecision {
            pair: pair.id.clone(),
            version: Some(version.0),
            promoted: true,
            criteria,
            reason: "晋升：发布新问题集版本，并联动失效".to_owned(),
            change: Some(change),
            boundaries,
            invalidation: Some(invalidation),
            lifecycle: records.iter().map(|record| format!("{record:?}")).collect(),
        });

        // 回退演练：在晋升后的库副本上把旧版本重新设为启用，核「旧版本还在、可回退」。
        // 先在状态里补一条 v2 上的判断：回退会把它标成待重查（联动失效看得见）。
        let mut drill_library = refined_library.clone();
        let on_new_version = promotion::record_judgment(
            &mut promote_state,
            &pair.question,
            version,
            dev_answer(&dev_run, &pair.dev_case, &pair.question)
                .unwrap_or(Answer::Noul(jh_core::NoulAnswer { noul: 1.0 })),
            &format!("rollback-drill-{}", pair.dev_case),
            cycle_judge.source(),
        )?;
        let (_, rollback_invalidation) = promotion::publish(
            &mut drill_library,
            &mut promote_state,
            &pair.question,
            QuestionVersion(1),
            unix_ms(),
            "回退演练：新版本在未见变体上退化时回到旧版本".to_owned(),
        )?;
        let _ = on_new_version;
        rollback_record = Some(RollbackRecord {
            pair: pair.id.clone(),
            from: version.0,
            to: 1,
            invalidation: rollback_invalidation,
            versions: promotion::version_states(&drill_library, &pair.question)
                .into_iter()
                .map(|(version, state)| (version.0, state))
                .collect(),
            note: "回退走的是同一个发布函数（把被替代的旧版本重新设为启用），旧版本只改状态、不删除、不重编号".to_owned(),
        });
    }

    let library_entries: Vec<LibraryEntry> = family
        .pairs
        .iter()
        .map(|pair| LibraryEntry {
            question: pair.question.to_string(),
            versions: promotion::version_states(&refined_library, &pair.question)
                .into_iter()
                .map(|(version, state)| (version.0, state))
                .collect(),
            enabled: promotion::enabled_version(&refined_library, &pair.question).map(|v| v.0),
        })
        .collect();
    // 两臂的问题库各自落盘：回退目标、版本状态与验证记录都能单独复核。
    let fixed_library_path = options.out_dir.join("question-library-fixed.json");
    let refined_library_path = options.out_dir.join("question-library-refined.json");
    promotion::write_library(&fixed_library_path, &fixed_library)?;
    promotion::write_library(&refined_library_path, &refined_library)?;

    // ---- 5. 两臂对照 --------------------------------------------------------------
    let fixed_view = family.question_set_from_library(&fixed_library);
    let refined_view = family.question_set_from_library(&refined_library);
    let mut arms = Vec::new();
    for name in &options.judges {
        let judge = match build_judge(*name, &options.replay_path) {
            Ok(judge) => Some(judge),
            Err(error) => {
                clean = false;
                arms.push(unavailable_arm(*name, "fixed", &error));
                arms.push(unavailable_arm(*name, "refined", &error));
                continue;
            }
        };
        let judge = judge.expect("已构造");
        for (arm, view, dir) in [
            ("fixed", &fixed_view, &fixed_dir),
            ("refined", &refined_view, &refined_dir),
        ] {
            let run_id = format!("{}-{}", options.run, arm);
            let outcome = run_arm(
                &variants,
                view,
                *name,
                judge.as_ref(),
                arm,
                &run_id,
                &mut calls,
                dir,
            )
            .await?;
            clean &= outcome.judge_failures == 0;
            let totals = totals_of(&outcome.tasks);
            let subsets = subset_reports(&outcome.tasks);
            arms.push(ArmReport {
                judge: name.cli_name().to_owned(),
                source: name.source(),
                live: name.live(),
                arm: arm.to_owned(),
                question_versions: view
                    .versions()
                    .into_iter()
                    .map(|(question, version)| (question.to_string(), version.0))
                    .collect(),
                verification: verification_of(*name, &outcome),
                calls: CallStats {
                    attempted: outcome.asks_total,
                    succeeded: outcome.asks_total - outcome.judge_failures,
                    failed: outcome.judge_failures,
                    errors: outcome.errors.clone(),
                },
                latency: LatencyStats {
                    samples: outcome.latencies.len() as u32,
                    min: outcome.latencies.iter().copied().min(),
                    p50: metrics::percentile(&outcome.latencies, 0.5),
                    p95: metrics::percentile(&outcome.latencies, 0.95),
                    max: outcome.latencies.iter().copied().max(),
                },
                usage: UsageStats {
                    calls_with_usage: outcome.usage_calls,
                    input_tokens: outcome.usage_seen.then_some(outcome.input_tokens),
                    output_tokens: outcome.usage_seen.then_some(outcome.output_tokens),
                },
                totals,
                subsets,
                tasks: outcome.tasks,
            });
        }
    }

    // ---- 6. 成本账与报告 -----------------------------------------------------------
    let cost = cost_section(&calls_path, &options.run, generation_calls)?;
    let gains = gains_section(&arms, &options.judges, &cost);
    let mut protocols = Vec::new();
    for pair in &family.pairs {
        protocols.push(format!(
            "{}：门控问题 {}（统一字段），关键证据 {}，观察动作 {}",
            pair.id, pair.question, pair.key_object, pair.probe_action
        ));
    }

    let report = H3Report {
        run: options.run.clone(),
        generated_at_unix_ms: unix_ms(),
        command: options.command.clone(),
        protocol: ProtocolDeclaration {
            arms: vec![
                "fixed：只启用固定问题集（v1）；生成产物停在候选 / 影子状态，不进控制".to_owned(),
                "refined：达标的候选被发布为 v2 启用，循环读它的定义与读取范围".to_owned(),
            ],
            consumption_protocol: "两臂共用同一段循环代码、同一套前提门判据（jh_app::matching 的同一函数）、同一批任务、初始证据、判定器配置与决策时机；差别只有问题库当前启用的版本（研究目标 §6.4：新判断通过统一字段进入同一个决策器，不修改消费者代码、决策器提示、权重或控制规则）".to_owned(),
            change_record: "每次晋升记录问题（定义是否变）、输入投影（读取范围是否变）、消费者（标签集合是否变）、策略（控制规则是否变）四路".to_owned(),
            boundaries: vec![
                "用户目标不变：任务族与各任务的声明文本摘要在精化前后必须一致".to_owned(),
                "验收要求不变：各相位的必需动作与交付物绑定要求摘要在精化前后必须一致".to_owned(),
                "工具权限不扩大：候选的证据对象必须落在声明的可观测清单里".to_owned(),
                "控制规则不变：候选动作与前提门摘要在精化前后必须一致；选项标签集合原样保留".to_owned(),
            ],
            promotion_criteria: promotion::DECLARED_CRITERIA
                .iter()
                .map(|criterion| promotion::CriterionOutcome {
                    id: criterion.id.to_owned(),
                    description: criterion.description.to_owned(),
                    passed: decisions
                        .iter()
                        .filter(|decision| decision.promoted)
                        .any(|decision| {
                            decision
                                .criteria
                                .iter()
                                .any(|item| item.id == criterion.id && item.passed)
                        }),
                    reading: decisions
                        .iter()
                        .flat_map(|decision| decision.criteria.iter())
                        .find(|item| item.id == criterion.id)
                        .map(|item| item.reading.clone())
                        .unwrap_or_else(|| "未评估".to_owned()),
                })
                .collect(),
            thresholds,
            material_sufficiency: "材料是否足够按该问题**这次读到的输入投影**判定：关键证据对象不在投影里时，正确读法是 other（问题读不到的材料不算它的材料）。材料不足却读出一个候选集内标签，按错读计数并单独报告（M1 的 A1 教训）".to_owned(),
            metric_definitions: metric_definitions(),
        },
        family: FamilySummary {
            name: family.name.clone(),
            source: family.source.clone(),
            pairs: family.pairs.clone(),
            tasks: dev_tasks
                .iter()
                .chain(variants.iter())
                .map(|entry| TaskEntrySummary {
                    task: entry.task.id.clone(),
                    pair: entry.pair.clone(),
                    subset: if entry.development {
                        "development".to_owned()
                    } else {
                        entry.subset.label().to_owned()
                    },
                    side: entry.side.clone(),
                    development: entry.development,
                    key_object: family
                        .pair(&entry.pair)
                        .map(|pair| pair.key_object.to_string())
                        .unwrap_or_default(),
                    actions: entry
                        .task
                        .actions
                        .iter()
                        .map(|action| action.id.to_string())
                        .collect(),
                    rounds: entry.task.rounds,
                })
                .collect(),
            questions: family
                .questions()
                .iter()
                .map(|(question, spec)| {
                    (
                        question.to_string(),
                        serde_json::to_value(spec).unwrap_or(serde_json::Value::Null),
                    )
                })
                .collect(),
        },
        environment: report_environment("run"),
        generation: GenerationSection {
            source: generation_source,
            candidates_file: absolute(&candidates_file),
            counterexamples_file: absolute(&counterexample_path),
            counterexamples: per_pair_counterexamples
                .values()
                .map(|entries| entries.len() as u32)
                .sum(),
            per_pair: generation_entries,
            calls: generation_calls,
            failures: generated
                .iter()
                .filter(|entry| entry.call.error.is_some())
                .count() as u32,
        },
        validation: ValidationSection {
            per_pair: validation_entries,
            fixed_development_rate: {
                let rates: Vec<f64> = dev_run
                    .tasks
                    .iter()
                    .filter_map(|summary| summary.agreement_rate())
                    .collect();
                (!rates.is_empty()).then(|| rates.iter().sum::<f64>() / rates.len() as f64)
            },
        },
        shadow: ShadowSection {
            per_pair: shadow_entries,
        },
        promotion: PromotionSection {
            decisions,
            rollback: rollback_record,
            library: library_entries,
        },
        arms,
        gains,
        raw: RawPaths {
            out_dir: absolute(&options.out_dir),
            fixed_traces: absolute(&fixed_dir),
            refined_traces: absolute(&refined_dir),
            dev_traces: absolute(&dev_dir),
            shadow_traces: absolute(&options.out_dir.join("shadow-traces")),
            calls_log: absolute(&calls_path),
            candidates_file: absolute(&candidates_file),
            counterexamples_file: absolute(&counterexample_path),
            fixed_library: absolute(&fixed_library_path),
            refined_library: absolute(&refined_library_path),
        },
        cost,
        notes: declared_notes(),
    };

    let _ = started;
    Ok(H3Outcome { report, clean })
}

/// 一臂的跑批结果（内部用）。
struct ArmRun {
    tasks: Vec<TaskSummary>,
    latencies: Vec<u64>,
    errors: BTreeMap<String, u32>,
    asks_total: u32,
    judge_failures: u32,
    usage_calls: u32,
    usage_seen: bool,
    input_tokens: u64,
    output_tokens: u64,
}

/// 跑一臂 × 一组任务。
#[allow(clippy::too_many_arguments)]
async fn run_arm(
    tasks: &[&FamilyBTask],
    view: &crate::family_b::FamilyBQuestions,
    judge_name: JudgeName,
    judge: &dyn Judge,
    arm: &str,
    run_id: &str,
    calls: &mut CallLog,
    out_dir: &Path,
) -> Result<ArmRun, Error> {
    let evidence_dir = out_dir.join("evidence");
    std::fs::create_dir_all(&evidence_dir)?;
    let mut outcome = ArmRun {
        tasks: Vec::new(),
        latencies: Vec::new(),
        errors: BTreeMap::new(),
        asks_total: 0,
        judge_failures: 0,
        usage_calls: 0,
        usage_seen: false,
        input_tokens: 0,
        output_tokens: 0,
    };

    for entry in tasks {
        let stem = format!("{}-{}-{}", arm, judge_name.cli_name(), entry.task.id);
        let evidence_path = evidence_dir.join(format!("{stem}.jsonl"));
        let trace_path = out_dir.join(format!("{stem}.json"));
        let mut context = LoopContext {
            family: view,
            judge,
            channel: judge_name,
            arm: Arm::ALL[0],
            run: run_id.to_owned(),
            endpoint: endpoint_for(judge_name),
            shadow: None,
            calls,
        };
        let trace = run_task(&mut context, &entry.task, &evidence_path).await?;
        write_json(&trace_path, &trace)?;
        absorb(
            &mut outcome,
            entry,
            view,
            &trace,
            &trace_path,
            &evidence_path,
        );
    }

    Ok(outcome)
}

fn absorb(
    outcome: &mut ArmRun,
    entry: &FamilyBTask,
    view: &crate::family_b::FamilyBQuestions,
    trace: &TaskTrace,
    trace_path: &Path,
    evidence_path: &Path,
) {
    outcome
        .tasks
        .push(summary_of(entry, view, trace, trace_path, evidence_path));
    for ask in &trace.asks {
        outcome.asks_total += 1;
        outcome.latencies.push(ask.latency_ms);
        if ask.outcome != ASK_SUCCEEDED {
            outcome.judge_failures += 1;
            *outcome.errors.entry(ask.outcome.clone()).or_insert(0) += 1;
        }
        if ask.input_tokens.is_some() || ask.output_tokens.is_some() {
            outcome.usage_calls += 1;
            outcome.usage_seen = true;
            outcome.input_tokens += ask.input_tokens.unwrap_or(0);
            outcome.output_tokens += ask.output_tokens.unwrap_or(0);
        }
    }
}

fn summary_of(
    entry: &FamilyBTask,
    view: &crate::family_b::FamilyBQuestions,
    trace: &TaskTrace,
    trace_path: &Path,
    evidence_path: &Path,
) -> TaskSummary {
    let question = entry.question();
    let key_object = entry
        .task
        .material_gate
        .get(question)
        .map(|gate| gate.key_object.clone())
        .unwrap_or_else(|| ObjectId::from(""));
    let mut question_versions = BTreeMap::new();
    if let Some(spec) = view.current(question) {
        question_versions.insert(question.to_string(), spec.version.0);
    }

    let material_insufficient = trace
        .judgments
        .iter()
        .filter(|judgment| !judgment.projection.reads(&key_object))
        .count() as u32;
    let candidate_label_when_insufficient = trace
        .judgments
        .iter()
        .filter(|judgment| {
            !judgment.projection.reads(&key_object)
                && !judgment.reading.is_empty()
                && judgment.reading != ABSENT_LABEL
        })
        .count() as u32;

    let executions: Vec<String> = trace
        .decisions
        .iter()
        .filter(|decision| decision.outcome == DecisionOutcome::Executed)
        .map(|decision| format!("第 {} 轮 {}", decision.round, decision.action))
        .collect();
    let wrong_actions: Vec<String> = trace
        .decisions
        .iter()
        .filter(|decision| {
            decision.outcome == DecisionOutcome::Executed
                && !entry
                    .task
                    .justified_at_round(decision.round, &decision.action)
        })
        .map(|decision| format!("第 {} 轮 {}", decision.round, decision.action))
        .collect();
    let required = entry.task.required_actions();
    let executed_actions = trace.executions();
    let executed: BTreeSet<&ActionId> = executed_actions.iter().map(|(_, id)| id).collect();
    let unrecovered: Vec<String> = required
        .iter()
        .filter(|action| !executed.contains(action))
        .map(ActionId::to_string)
        .collect();
    let observations: Vec<String> = trace
        .decisions
        .iter()
        .filter(|decision| decision.outcome == DecisionOutcome::Executed)
        .filter(|decision| {
            entry
                .task
                .action(&decision.action)
                .is_some_and(|spec| spec.effect.is_some())
        })
        .map(|decision| {
            format!(
                "第 {} 轮 {}（{} 单位）",
                decision.round, decision.action, decision.cost_units
            )
        })
        .collect();
    let observation_cost_units: f64 = trace
        .decisions
        .iter()
        .filter(|decision| decision.outcome == DecisionOutcome::Executed)
        .filter(|decision| {
            entry
                .task
                .action(&decision.action)
                .is_some_and(|spec| spec.effect.is_some())
        })
        .map(|decision| decision.cost_units)
        .sum();
    let action_cost_units: f64 = trace
        .decisions
        .iter()
        .filter(|decision| decision.outcome == DecisionOutcome::Executed)
        .map(|decision| decision.cost_units)
        .sum();

    let acceptance = trace.acceptance.clone().unwrap_or_else(|| {
        accept(
            &entry.task,
            &crate::family::AcceptanceInput {
                executions: &[],
                deliverable_reads: &[],
                claims: 0,
                abstentions: 0,
            },
            &jh_core::MemoryEvidenceLog::default(),
        )
    });

    TaskSummary {
        task: entry.task.id.clone(),
        pair: entry.pair.clone(),
        subset: if entry.development {
            "development".to_owned()
        } else {
            entry.subset.label().to_owned()
        },
        side: entry.side.clone(),
        question_versions,
        acceptance_passed: acceptance.passed,
        acceptance_checks: acceptance
            .checks
            .iter()
            .map(|check| format!("{}={}（{}）", check.name, check.passed, check.detail))
            .collect(),
        executions,
        wrong_actions,
        required_actions: required.len() as u32,
        recovered_actions: (required.len() - unrecovered.len()) as u32,
        unrecovered,
        observations,
        observation_cost_units,
        action_cost_units,
        asks: trace.asks.len() as u32,
        judge_failures: trace
            .asks
            .iter()
            .filter(|ask| ask.outcome != ASK_SUCCEEDED)
            .count() as u32,
        judgments: trace.judgments.len() as u32,
        agreements: trace
            .judgments
            .iter()
            .filter(|judgment| judgment.agreement == Some(true))
            .count() as u32,
        material_insufficient,
        candidate_label_when_insufficient,
        trace: absolute(trace_path),
        evidence: absolute(evidence_path),
    }
}

fn totals_of(tasks: &[TaskSummary]) -> ArmTotals {
    let mut totals = empty_totals();
    totals.tasks = tasks.len() as u32;
    let passed = tasks.iter().filter(|task| task.acceptance_passed).count();
    totals.acceptance = format!("{passed}/{}", tasks.len());
    for task in tasks {
        totals.unrecovered.extend(task.unrecovered.clone());
        totals.required_actions += task.required_actions;
        totals.recovered_actions += task.recovered_actions;
        totals.judgments += task.judgments;
        totals.agreements += task.agreements;
        totals.material_insufficient += task.material_insufficient;
        totals.candidate_label_when_insufficient += task.candidate_label_when_insufficient;
        totals.asks += task.asks;
        totals.judge_failures += task.judge_failures;
        totals.observation_actions += task.observations.len() as u32;
        totals.observation_cost_units += task.observation_cost_units;
        totals.action_cost_units += task.action_cost_units;
        totals.wrong_actions += task.wrong_actions.len() as u32;
    }
    totals.recovered = format!("{}/{}", totals.recovered_actions, totals.required_actions);
    totals
}

fn subset_reports(tasks: &[TaskSummary]) -> Vec<SubsetReport> {
    [Subset::Given, Subset::Observed]
        .into_iter()
        .map(|subset| {
            let rows: Vec<TaskSummary> = tasks
                .iter()
                .filter(|task| task.subset == subset.label())
                .cloned()
                .collect();
            SubsetReport {
                subset: subset.label().to_owned(),
                description: subset.description().to_owned(),
                totals: totals_of(&rows),
            }
        })
        .collect()
}

fn verification_of(name: JudgeName, outcome: &ArmRun) -> Verification {
    let succeeded = outcome.asks_total - outcome.judge_failures;
    let failed_note = if outcome.judge_failures > 0 {
        format!("，另失败 {} 次", outcome.judge_failures)
    } else {
        String::new()
    };
    match (name.live(), succeeded > 0) {
        (true, true) => Verification {
            status: VerificationStatus::Verified,
            detail: format!("真实调用成功 {succeeded} 次{failed_note}"),
        },
        (true, false) => Verification {
            status: VerificationStatus::NotVerified,
            detail: format!(
                "真实调用未成功（{} 次尝试全部失败）：相关结论保持未验证",
                outcome.asks_total
            ),
        },
        (false, true) => Verification {
            status: VerificationStatus::Simulated,
            detail: format!("本地判定器：没有真实调用，读数只作对照与上限估计{failed_note}"),
        },
        (false, false) => Verification {
            status: VerificationStatus::NotVerified,
            detail: format!("本地判定器 {} 次调用全部失败", outcome.asks_total),
        },
    }
}

fn unavailable_arm(name: JudgeName, arm: &str, error: &Error) -> ArmReport {
    ArmReport {
        judge: name.cli_name().to_owned(),
        source: name.source(),
        live: name.live(),
        arm: arm.to_owned(),
        question_versions: BTreeMap::new(),
        verification: Verification {
            status: VerificationStatus::NotVerified,
            detail: format!("判定器未构造成功：{error}"),
        },
        calls: CallStats {
            attempted: 0,
            succeeded: 0,
            failed: 0,
            errors: BTreeMap::new(),
        },
        latency: LatencyStats {
            samples: 0,
            min: None,
            p50: None,
            p95: None,
            max: None,
        },
        usage: UsageStats {
            calls_with_usage: 0,
            input_tokens: None,
            output_tokens: None,
        },
        totals: empty_totals(),
        subsets: Vec::new(),
        tasks: Vec::new(),
    }
}

/// 区分率：把若干 `(读出正确一侧的条数, 材料足够的条数)` 合起来算。
///
/// 分母是**材料足够**的判断数（关键证据在该判断的输入投影里）；一次都没有足够材料时记 0——
/// 「一次都没读到关键证据」就是区分不了，不是没有读数。
fn discrimination_rate(rows: impl Iterator<Item = (u32, u32)>) -> Option<f64> {
    let mut matches = 0;
    let mut sufficient = 0;
    let mut seen = false;
    for (row_matches, row_sufficient) in rows {
        seen = true;
        matches += row_matches;
        sufficient += row_sufficient;
    }
    if !seen {
        return None;
    }
    Some(if sufficient == 0 {
        0.0
    } else {
        f64::from(matches) / f64::from(sufficient)
    })
}

/// 某个任务轨迹里读出「正确一侧」的比例（区分口径：分母是材料足够——投影覆盖关键证据——的判断）。
fn side_rate_of(trace_path: &Path, side: &str, key_object: &ObjectId) -> f64 {
    let Ok(trace) = read_trace(trace_path) else {
        return 0.0;
    };
    let sufficient: Vec<&crate::matching::JudgmentTrace> = trace
        .judgments
        .iter()
        .filter(|judgment| judgment.projection.reads(key_object))
        .collect();
    if sufficient.is_empty() {
        return 0.0;
    }
    let matches = sufficient
        .iter()
        .filter(|judgment| judgment.reading == side)
        .count();
    matches as f64 / sufficient.len() as f64
}

/// 从开发案例的轨迹里抽反例（§6.2 第 1 步的三条入口）。
fn extract_counterexamples(
    family: &FamilyB,
    entry: &FamilyBTask,
    trace_path: &Path,
) -> Result<Vec<NewCounterexample>, Error> {
    let trace: TaskTrace = read_trace(trace_path)?;
    let pair = family.pair(&entry.pair).expect("对存在");
    let question = pair.question.clone();
    let mut out = Vec::new();

    let evidence_path = trace_path.with_extension("").with_file_name(format!(
        "{}.jsonl",
        trace_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default()
    ));
    let evidence = evidence_ids(&evidence_path)?;

    let mismatched: Vec<&crate::matching::JudgmentTrace> = trace
        .judgments
        .iter()
        .filter(|judgment| judgment.agreement == Some(false))
        .collect();
    for judgment in mismatched {
        let insufficient = !judgment.projection.reads(&pair.key_object);
        let mut attributions = BTreeSet::new();
        attributions.insert(if insufficient {
            Attribution::StateMissesDimension
        } else {
            Attribution::QuestionAnsweredWrongly
        });
        if insufficient {
            attributions.insert(Attribution::RawFactMissing);
        }
        out.push(NewCounterexample {
            source: FailureSource::ExpectedEffectMissed,
            at_unix_ms: unix_ms(),
            question: question.clone(),
            question_version: judgment.question_version,
            evidence: evidence_of(&evidence, &judgment.projection),
            action: trace
                .decisions
                .iter()
                .find(|decision| {
                    decision.round >= judgment.round
                        && decision.outcome == DecisionOutcome::GateUnsatisfied
                })
                .map(|decision| decision.action.clone()),
            control_rule: Some(format!(
                "{} == {} → {}",
                pair.question, entry.side, pair.remedies[&entry.side]
            )),
            attributions,
            note: format!(
                "开发案例 {}：第 {} 轮读出 `{}`，真值是 `{}`（关键证据 {} {}）",
                entry.task.id,
                judgment.round,
                judgment.reading,
                judgment.truth.clone().unwrap_or_default(),
                pair.key_object,
                if insufficient {
                    "不在这次读到的材料里"
                } else {
                    "在材料里"
                }
            ),
        });
    }

    if trace
        .acceptance
        .as_ref()
        .is_some_and(|acceptance| !acceptance.passed)
    {
        let failed: Vec<String> = trace
            .acceptance
            .as_ref()
            .map(|acceptance| {
                acceptance
                    .checks
                    .iter()
                    .filter(|check| !check.passed)
                    .map(|check| format!("{}（{}）", check.name, check.detail))
                    .collect()
            })
            .unwrap_or_default();
        out.push(NewCounterexample {
            source: FailureSource::VerificationFailed,
            at_unix_ms: unix_ms(),
            question: question.clone(),
            question_version: trace
                .judgments
                .first()
                .map(|judgment| judgment.question_version)
                .unwrap_or(QuestionVersion(1)),
            evidence: Vec::new(),
            action: trace.decisions.iter().find_map(|decision| {
                (decision.outcome == DecisionOutcome::Executed).then(|| decision.action.clone())
            }),
            control_rule: Some(format!(
                "{} == {} → {}",
                pair.question, entry.side, pair.remedies[&entry.side]
            )),
            attributions: BTreeSet::from([
                Attribution::StateMissesDimension,
                Attribution::RawFactMissing,
            ]),
            note: format!(
                "开发案例 {} 的独立验收未通过：{}；关键证据 {} 不在固定问题 {} v1 的读取范围里",
                entry.task.id,
                failed.join("；"),
                pair.key_object,
                pair.question
            ),
        });
    }

    Ok(out)
}

/// 开发案例的失败事实（进生成提示词；只有开发案例，没有未见变体的真值）。
fn dev_facts(
    family: &FamilyB,
    entry: &FamilyBTask,
    trace_path: &Path,
) -> Result<Vec<String>, Error> {
    let trace: TaskTrace = read_trace(trace_path)?;
    let pair = family.pair(&entry.pair).expect("对存在");
    let mut facts = vec![format!(
        "开发案例 {}（对 {}，隐藏侧 {}）",
        entry.task.id, pair.id, entry.side
    )];
    for judgment in &trace.judgments {
        facts.push(format!(
            "第 {} 轮读出 `{}`，真值是 `{}`；材料读到的对象 {} 个{}",
            judgment.round,
            judgment.reading,
            judgment.truth.clone().unwrap_or_default(),
            judgment.projection.len(),
            if judgment.projection.reads(&pair.key_object) {
                "（含关键证据）"
            } else {
                "（不含关键证据）"
            }
        ));
    }
    for decision in &trace.decisions {
        facts.push(format!(
            "第 {} 轮动作 `{}` → {:?}",
            decision.round, decision.action, decision.outcome
        ));
    }
    if let Some(acceptance) = &trace.acceptance {
        facts.push(format!(
            "独立验收通过 = {}（{}）",
            acceptance.passed,
            acceptance
                .checks
                .iter()
                .map(|check| format!("{}={}", check.name, check.passed))
                .collect::<Vec<_>>()
                .join("、")
        ));
    }
    Ok(facts)
}

fn read_trace(path: &Path) -> Result<TaskTrace, Error> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| Error::TaskSet(format!("轨迹 {} 不可读：{error}", absolute(path))))?;
    serde_json::from_str(&text)
        .map_err(|error| Error::TaskSet(format!("轨迹 {} 无法解析：{error}", absolute(path))))
}

fn evidence_ids(path: &Path) -> Result<Vec<(ObjectId, jh_core::ObjectVersion, EvidenceId)>, Error> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let log = jh_core::JsonlEvidenceLog::open(path)?;
    Ok(log
        .entries()
        .iter()
        .map(|entry| (entry.object.clone(), entry.version, entry.id))
        .collect())
}

fn evidence_of(
    entries: &[(ObjectId, jh_core::ObjectVersion, EvidenceId)],
    projection: &jh_core::InputProjection,
) -> Vec<EvidenceId> {
    let mut out: Vec<EvidenceId> = entries
        .iter()
        .filter(|(object, version, _)| projection.version_of(object) == Some(*version))
        .map(|(_, _, id)| *id)
        .collect();
    out.sort();
    out.dedup();
    out
}

fn dev_answer(run: &ArmRun, task: &str, question: &QuestionId) -> Option<Answer> {
    let summary = run.tasks.iter().find(|summary| summary.task == task)?;
    let trace: TaskTrace = read_trace(Path::new(&summary.trace)).ok()?;
    trace
        .judgments
        .iter()
        .find(|judgment| judgment.question == *question)
        .map(|judgment| judgment.answer.clone())
}

/// 精化收益：两臂之差与精化循环的成本并排（§10.1、§6.5）。
fn gains_section(arms: &[ArmReport], judges: &[JudgeName], cost: &CostSection) -> GainsSection {
    let refinement_calls: u32 = cost
        .by_purpose
        .iter()
        .filter(|purpose| {
            matches!(
                purpose.purpose.as_str(),
                "generation" | "validation" | "shadow"
            )
        })
        .map(|purpose| purpose.calls)
        .sum();
    let refinement_tokens: Option<u64> = {
        let input: u64 = cost
            .by_purpose
            .iter()
            .filter(|purpose| {
                matches!(
                    purpose.purpose.as_str(),
                    "generation" | "validation" | "shadow"
                )
            })
            .filter_map(|purpose| purpose.input_tokens)
            .sum();
        let output: u64 = cost
            .by_purpose
            .iter()
            .filter(|purpose| {
                matches!(
                    purpose.purpose.as_str(),
                    "generation" | "validation" | "shadow"
                )
            })
            .filter_map(|purpose| purpose.output_tokens)
            .sum();
        (input + output > 0).then_some(input + output)
    };

    let mut per_judge = Vec::new();
    for name in judges {
        let find = |arm: &str| {
            arms.iter()
                .find(|entry| entry.judge == name.cli_name() && entry.arm == arm)
        };
        let (Some(fixed), Some(refined)) = (find("fixed"), find("refined")) else {
            continue;
        };
        let acceptance_of = |arm: &ArmReport| {
            arm.totals
                .acceptance
                .split_once('/')
                .and_then(|(passed, _)| passed.parse::<u32>().ok())
                .unwrap_or(0) as i32
        };
        let acceptance_delta = acceptance_of(refined) - acceptance_of(fixed);
        let wrong_actions_delta =
            refined.totals.wrong_actions as i32 - fixed.totals.wrong_actions as i32;
        let net = format!(
            "收益：验收 {} → {}（{:+} 个任务）、不该发出的动作 {} → {}（{:+}）；代价：精化循环 {} 次调用（生成 + 验证 + 影子）{}、两臂判断调用 {} vs {}、观察计价 {} vs {} 单位。读数只记原值：不做「收益大于成本」的自动判定，货币成本本项目不虚构。",
            fixed.totals.acceptance,
            refined.totals.acceptance,
            acceptance_delta,
            fixed.totals.wrong_actions,
            refined.totals.wrong_actions,
            wrong_actions_delta,
            refinement_calls,
            refinement_tokens
                .map(|tokens| format!("（{tokens} token）"))
                .unwrap_or_else(|| "（token 未上报）".to_owned()),
            fixed.calls.attempted,
            refined.calls.attempted,
            fixed.totals.observation_cost_units,
            refined.totals.observation_cost_units,
        );
        per_judge.push(GainReading {
            judge: name.cli_name().to_owned(),
            acceptance: format!(
                "{} → {}",
                fixed.totals.acceptance, refined.totals.acceptance
            ),
            acceptance_delta,
            wrong_actions: (fixed.totals.wrong_actions, refined.totals.wrong_actions),
            wrong_actions_delta,
            recovered: (
                fixed.totals.recovered.clone(),
                refined.totals.recovered.clone(),
            ),
            observation_cost_units: (
                fixed.totals.observation_cost_units,
                refined.totals.observation_cost_units,
            ),
            calls: (fixed.calls.attempted, refined.calls.attempted),
            refinement_calls,
            refinement_tokens,
            net,
        });
    }

    GainsSection {
        per_judge,
        note: "精化收益按判定器分别给出：差值是精化臂 − 固定臂，两臂共享任务集、初始证据、判定器配置与决策时机，差别只有问题库启用的版本。成本侧包含生成、验证、影子（后续评估）与两臂判断调用；维护成本的一等表达是问题库的版本数与停用/替代记录（见 promotion.library）。".to_owned(),
    }
}

/// 成本账：从调用记录里按 `run` 标识统计每个用途的调用读数。
fn cost_section(calls_path: &Path, run: &str, generation_calls: u32) -> Result<CostSection, Error> {
    let entries = crate::calls::read_entries(calls_path)?;
    let purposes = [
        ("generation", format!("{run}-generation")),
        ("validation", format!("{run}-validation")),
        ("shadow", format!("{run}-shadow")),
        ("development", format!("{run}-dev")),
        ("fixed_arm", format!("{run}-fixed")),
        ("refined_arm", format!("{run}-refined")),
    ];
    let mut by_purpose = Vec::new();
    let mut total = PurposeCost {
        purpose: "合计".to_owned(),
        ..Default::default()
    };
    for (purpose, run_id) in purposes {
        let mut stats = PurposeCost {
            purpose: purpose.to_owned(),
            ..Default::default()
        };
        let mut latencies = Vec::new();
        let mut input_tokens = 0u64;
        let mut output_tokens = 0u64;
        let mut usage_seen = false;
        for entry in entries.iter().filter(|entry| entry.run() == run_id) {
            stats.calls += 1;
            match entry {
                CallEntry::Judgment { record, .. } => {
                    stats.succeeded += 1;
                    latencies.push(record.latency_ms);
                    if let Some(usage) = &record.usage {
                        usage_seen = true;
                        input_tokens += usage.input_tokens.unwrap_or(0);
                        output_tokens += usage.output_tokens.unwrap_or(0);
                    }
                }
                CallEntry::Failure { latency_ms, .. } => {
                    stats.failed += 1;
                    latencies.push(*latency_ms);
                }
                CallEntry::ModelCall {
                    latency_ms,
                    usage,
                    error,
                    ..
                } => {
                    if error.is_none() {
                        stats.succeeded += 1;
                    } else {
                        stats.failed += 1;
                    }
                    latencies.push(*latency_ms);
                    if let Some(usage) = usage {
                        usage_seen = true;
                        input_tokens += usage.input_tokens.unwrap_or(0);
                        output_tokens += usage.output_tokens.unwrap_or(0);
                    }
                }
            }
        }
        stats.latency_p50_ms = metrics::percentile(&latencies, 0.5);
        stats.input_tokens = usage_seen.then_some(input_tokens);
        stats.output_tokens = usage_seen.then_some(output_tokens);
        total.calls += stats.calls;
        total.succeeded += stats.succeeded;
        total.failed += stats.failed;
        by_purpose.push(stats);
    }
    total.input_tokens = by_purpose
        .iter()
        .filter_map(|entry| entry.input_tokens)
        .sum::<u64>()
        .into();
    total.output_tokens = by_purpose
        .iter()
        .filter_map(|entry| entry.output_tokens)
        .sum::<u64>()
        .into();
    total.input_tokens = if by_purpose.iter().any(|entry| entry.input_tokens.is_some()) {
        total.input_tokens
    } else {
        None
    };
    total.output_tokens = if by_purpose.iter().any(|entry| entry.output_tokens.is_some()) {
        total.output_tokens
    } else {
        None
    };

    let logged = logged_calls(calls_path, run)?;
    Ok(CostSection {
        by_purpose,
        total,
        note: format!(
            "成本只记可核的量：调用次数、token（通道上报）、延迟中位数；本项目不虚构货币价格。生成调用 {generation_calls} 次（本运行标识下共记 {logged} 条调用记录）；观察动作的计价成本按任务族自定单位单独记在动作成本里。"
        ),
    })
}

/// 指标口径声明。
pub fn metric_definitions() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "validation".to_owned(),
            "候选在开发案例、正例（材料足够）、反例（材料不足 → other）与不同表述 / 不同服务的未见用例上各判一次；材料按被验证版本声明的读取范围裁剪，读数（一致率、Brier、对数损失、调用与 token）按原值进报告与问题库的验证记录。".to_owned(),
        ),
        (
            "promotion".to_owned(),
            "晋升标准与阈值在跑之前写死在 promotion::DECLARED_CRITERIA 与 declared_thresholds 里；没达标就不发布（保留固定问题模式），标准不因读数而下调。".to_owned(),
        ),
        (
            "acceptance".to_owned(),
            "独立验收只读证据日志与隐藏真值：必需动作（含观察动作）是否在某轮真的发出、有没有不该发出的动作；控制器的完成声明不参与判定。".to_owned(),
        ),
        (
            "refinement_gain".to_owned(),
            "未见变体上的精化收益 = 精化臂 − 固定臂（验收通过数、不该发出的动作数、恢复的必需动作数、观察成本），逐判定器与逐子集分别读。".to_owned(),
        ),
        (
            "cost".to_owned(),
            "成本 = 生成 + 验证 + 影子 + 开发案例 + 两臂的调用数、token 与延迟；收益读数与这些成本并排给出（§6.5 要求收益扣除成本），货币成本不虚构。".to_owned(),
        ),
        (
            "observation_billing".to_owned(),
            "B2 的关键证据只能由观察动作取得；观察动作按任务族声明的计价单位计费，发出一次记一次成本——防止「永远读取全部信息」成为无代价策略。".to_owned(),
        ),
        (
            "material_sufficiency".to_owned(),
            "材料是否足够按该问题这次读到的输入投影判定：关键证据不在投影里时正确读法是 other；「材料不足却读出候选集内标签」单独计数（M1 的 A1 教训）。".to_owned(),
        ),
        (
            "transfer".to_owned(),
            "迁移读数只看未见变体（措辞、服务名、初始快照都重写）：只在原始失败案例上变正确不算迁移成功。".to_owned(),
        ),
        (
            "separate_evaluation".to_owned(),
            "新问题族的读数单独给出（逐对、逐版本、逐子集），不用全局校准统计替代。".to_owned(),
        ),
        (
            "failures".to_owned(),
            "生成、验证、影子与判断的失败如实计数并落调用记录，不丢弃、不静默重试。".to_owned(),
        ),
        (
            "shadow".to_owned(),
            "影子判断落在独立状态里，控制路径只读主状态；「若采纳会不会改变有价值的行动分支」按同一条门判据重算，方向按独立验收的正当性口径判定。".to_owned(),
        ),
    ])
}

/// 报告的固定说明。
fn declared_notes() -> Vec<String> {
    vec![
        "两臂共享任务集、初始证据、判定器配置与决策时机；差别只有问题库当前启用的版本。".to_owned(),
        "生成、验证、影子、开发案例与两臂的真实调用全部记账（调用记录 JSONL，按 run 标识分组），报告里的 cost 段是收益读数的成本侧。".to_owned(),
        "模拟真值不参与本对照（研究目标 §9.2）：它没有资格出现在任务族 B 的两臂里，构造被显式拒绝。".to_owned(),
        "开发案例（development）只用于反例提取、候选验证与晋升判据；H3 的对照读数只在未见变体上采集。".to_owned(),
        "看不到的读数就说看不到：判定器缺凭据、生成失败、候选未达标都如实记，不用别的来源顶替。".to_owned(),
    ]
}

/// 生成新的运行标识：`h3-<Unix 毫秒>`。
pub fn new_run_id() -> String {
    format!("h3-{}", unix_ms())
}

impl H3Report {
    /// 终端摘要行。
    pub fn summary_lines(&self) -> Vec<String> {
        let mut lines = vec![
            format!(
                "任务族 {}：{} 对场景（{}）",
                self.family.name,
                self.family.pairs.len(),
                self.family.source
            ),
            "H3 两臂 = 固定问题集合 vs 允许精化；B1 / B2 分开报告".to_owned(),
        ];
        lines.push(format!(
            "生成：{} 个候选（来源 {}，调用 {} 次，失败 {}）｜反例 {} 条",
            self.generation.per_pair.len(),
            self.generation.source,
            self.generation.calls,
            self.generation.failures,
            self.generation.counterexamples
        ));
        for decision in &self.promotion.decisions {
            let failed: Vec<&str> = decision
                .criteria
                .iter()
                .filter(|criterion| !criterion.passed)
                .map(|criterion| criterion.id.as_str())
                .collect();
            lines.push(format!(
                "晋升 {}：{}（版本 {}；未达标判据 {}）",
                decision.pair,
                if decision.promoted {
                    "已发布"
                } else {
                    "未发布"
                },
                decision
                    .version
                    .map(|version| format!("v{version}"))
                    .unwrap_or_else(|| "—".to_owned()),
                if failed.is_empty() {
                    "无".to_owned()
                } else {
                    failed.join("、")
                }
            ));
        }
        if let Some(rollback) = &self.promotion.rollback {
            lines.push(format!(
                "回退演练：{} v{} → v{}（待重查 {}、挡下动作 {}；旧版本保留）",
                rollback.pair,
                rollback.from,
                rollback.to,
                rollback.invalidation.stale,
                rollback.invalidation.blocked
            ));
        }
        for arm in &self.arms {
            lines.push(format!(
                "{} × {}：验收 {}｜恢复 {}｜错动作 {}｜观察 {} 次（{} 单位）｜判断 {} 条（一致 {}）｜材料不足 {}（其中读候选标签 {}）｜调用 {}/{} 失败 {}",
                arm.judge,
                arm.arm,
                arm.totals.acceptance,
                arm.totals.recovered,
                arm.totals.wrong_actions,
                arm.totals.observation_actions,
                arm.totals.observation_cost_units,
                arm.totals.judgments,
                arm.totals.agreements,
                arm.totals.material_insufficient,
                arm.totals.candidate_label_when_insufficient,
                arm.calls.attempted,
                arm.calls.succeeded,
                arm.calls.failed,
            ));
            for subset in &arm.subsets {
                lines.push(format!(
                    "   {}：验收 {}｜恢复 {}｜错动作 {}｜观察 {} 次（{} 单位）｜材料不足 {}（其中读候选标签 {}）",
                    subset.subset,
                    subset.totals.acceptance,
                    subset.totals.recovered,
                    subset.totals.wrong_actions,
                    subset.totals.observation_actions,
                    subset.totals.observation_cost_units,
                    subset.totals.material_insufficient,
                    subset.totals.candidate_label_when_insufficient,
                ));
            }
            lines.push(format!("   {}", arm.verification.detail));
        }
        for reading in &self.gains.per_judge {
            lines.push(format!(
                "精化收益（{}）：验收 {}（{:+}）｜错动作 {} → {}｜恢复 {} → {}｜调用 {} vs {}｜精化循环 {} 次",
                reading.judge,
                reading.acceptance,
                reading.acceptance_delta,
                reading.wrong_actions.0,
                reading.wrong_actions.1,
                reading.recovered.0,
                reading.recovered.1,
                reading.calls.0,
                reading.calls.1,
                reading.refinement_calls,
            ));
        }
        for purpose in &self.cost.by_purpose {
            lines.push(format!(
                "成本 {}：调用 {}（成功 {} 失败 {}），输入 token {}，输出 token {}，延迟中位数 {} ms",
                purpose.purpose,
                purpose.calls,
                purpose.succeeded,
                purpose.failed,
                purpose
                    .input_tokens
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "—".to_owned()),
                purpose
                    .output_tokens
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "—".to_owned()),
                purpose
                    .latency_p50_ms
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "—".to_owned()),
            ));
        }
        lines.push(format!("产物：{}", self.raw.out_dir));
        lines.push(format!("调用记录：{}", self.raw.calls_log));
        lines.push(format!("候选产物：{}", self.raw.candidates_file));
        lines.push(format!("命令：{}", self.command));
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metric_definitions_cover_the_declared_sections() {
        let definitions = metric_definitions();
        for key in [
            "acceptance",
            "refinement_gain",
            "cost",
            "observation_billing",
            "material_sufficiency",
            "transfer",
            "separate_evaluation",
        ] {
            assert!(definitions.contains_key(key), "缺口径：{key}");
        }
    }
}
