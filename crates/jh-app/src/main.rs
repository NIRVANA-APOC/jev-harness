//! `jev` 命令：`chat` 进真实终端跑会话（LLM 主循环 + Jev 判定门控 + 工具审批）；`ask` 未接线；
//! `run` 跑一个任务族 A 任务的匹配循环；`exp` 跑 M0 初始任务集
//! 的 JSON 基线；`compare` 跑 H1/H2 四组臂对照；`h3` 跑任务族 B 的 H3 对照（固定问题集合 vs 允许精化）；
//! `h4` 跑任务族 C 的 H4 三臂对照（固定测试 / 通用测试生成 / 针对缺口建设观测器）。
//!
//! ```text
//! cargo run -p jh-app -- chat                      # TUI 会话：真实模型 + Jev 门控（需要真实终端）
//! cargo run -p jh-app -- exp                       # 离线默认：规则 + 模拟真值
//! cargo run -p jh-app -- run --task A5 --judge rule        # 单任务端到端（含 §5.4 受控演示）
//! cargo run -p jh-app -- compare --judge rule              # 四组臂离线对照
//! TYPESAFE_API_KEY=... cargo run -p jh-app -- compare --judge jev
//! OPENAI_API_KEY=... OPENAI_MODEL=deepseek-flash OPENAI_BASE_URL=https://api.deepseek.com \
//!   cargo run -p jh-app -- h3 --judge jev --judge llm      # H3：一条命令出 JSON 报告
//! OPENAI_API_KEY=... OPENAI_MODEL=deepseek-flash OPENAI_BASE_URL=https://api.deepseek.com \
//!   cargo run -p jh-app -- h4                              # H4：任务族 C 的三臂对照
//! cargo run -p jh-app -- h4 --contracts var/.../contracts.json --tests var/.../tests.json
//! ```

use std::collections::BTreeMap;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Args, Parser, Subcommand};
use jh_app::ask::{self, AskOptions};
use jh_app::compare::{self, CompareOptions};
use jh_app::exp::{self, RunOptions};
use jh_app::family::FamilyA;
use jh_app::family_b::FamilyB;
use jh_app::h3::{self, CandidateSource, H3Options};
use jh_app::h4::{self, H4Options};
use jh_app::judge::JudgeName;
use jh_app::m4::{self, M4Options};
use jh_app::matching::{Arm, Cadence};
use jh_app::report;
use jh_app::session::{self, ChatOptions};
use jh_app::tasks::TaskSet;

#[derive(Parser)]
#[command(
    name = "jev",
    version,
    about = "jev-harness：LLM + Jev 的 Agent Harness 运行时"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// 交互式对话（TUI）：真实模型 + Jev 判定门控 + 工具审批
    Chat(ChatArgs),
    /// 单次判定：一个问题 + 一组选项，出一份读数（不采纳、不产生行动）
    Ask(AskArgs),
    /// 研究臂：跑一个任务族 A 任务的匹配循环（端到端；逐任务排查用）
    Run(RunArgs),
    /// 研究臂：跑初始任务集并输出 JSON 基线（M0 读数；最小的一条端到端）
    Exp(ExpArgs),
    /// 研究臂：H1 × H2 四组臂跑任务族 A（M1 读数；报告见 docs/m4-report.md）
    Compare(CompareArgs),
    /// 研究臂：H3 固定问题集合 vs 允许精化（任务族 B；M2 读数）
    H3(H3Args),
    /// 研究臂：H4 三臂对照（固定测试 / 通用测试生成 / 建设观测器；M3 读数）
    H4(H4Args),
    /// 研究臂：M4 综合评估（主对照 2×2 + 诊断对照 + 原生基线 + 两条残余消融 + 密度扫描）
    M4(M4Args),
}

#[derive(Args)]
struct M4Args {
    /// 只跑这些臂（可重复）；缺省四组全跑
    #[arg(long = "arm", value_name = "ARM")]
    arm: Vec<String>,
    /// 主对照与诊断对照的重复次数（每格配对重复）
    #[arg(long = "repeats", default_value_t = 3, value_name = "N")]
    repeats: usize,
    /// 每族（A / B）只跑前 N 个任务
    #[arg(long = "max-tasks", value_name = "N")]
    max_tasks: Option<usize>,
    /// 族 C 每族只跑前 N 条调度
    #[arg(long = "max-schedules", value_name = "N")]
    max_schedules: Option<usize>,
    /// 密度扫描的检查节奏（可重复，取值 triggered / every-K，K 为正整数）；缺省 triggered 与 every-3 / every-2 / every-1
    #[arg(long = "cadence", value_name = "CADENCE")]
    cadence: Vec<String>,
    /// 密度扫描的规模轴（每族任务数，可重复）；缺省 3 与 6
    #[arg(long = "scale", value_name = "N")]
    scale: Vec<usize>,
    /// 不跑原生基线臂
    #[arg(long = "no-native")]
    no_native: bool,
    /// 不跑跨任务复用消融
    #[arg(long = "no-reuse-ablation")]
    no_reuse_ablation: bool,
    /// 复用已落盘的契约产物（不重发生成调用）
    #[arg(long = "contracts", value_name = "FILE")]
    contracts: Option<PathBuf>,
    /// 用本地判定器替代两个判断通道（离线复跑；报告里如实标本地判定器）
    #[arg(long = "offline-judge", value_enum, value_name = "NAME")]
    offline_judge: Option<JudgeName>,
    /// 调用记录落盘路径（JSONL，追加写）
    #[arg(long = "calls", default_value = "var/calls.jsonl", value_name = "FILE")]
    calls: PathBuf,
    /// 运行产物目录；缺省 var/m4/<run>
    #[arg(long = "out-dir", value_name = "DIR")]
    out_dir: Option<PathBuf>,
    /// 报告输出路径；缺省 var/m4/<run>.json
    #[arg(long = "out", value_name = "FILE")]
    out: Option<PathBuf>,
}

#[derive(Args)]
struct ChatArgs {
    /// 工作区：工具的读写边界与 `bash` 的工作目录；缺省当前目录
    #[arg(long = "workspace", value_name = "DIR")]
    workspace: Option<PathBuf>,
    /// 门控判断器（缺凭据时如实标「不可用」，该次调用交人工裁决）
    #[arg(long = "judge", value_enum, default_value = "jev", value_name = "NAME")]
    judge: JudgeName,
    /// 防抖窗口（秒）：同一条命令在窗口内复用上一次判定
    #[arg(long = "debounce", default_value_t = 30, value_name = "SECONDS")]
    debounce: u64,
    /// 本轮模型调用上限；0 = 不限（缺省 32）
    #[arg(long = "max-rounds", default_value_t = session::DEFAULT_MAX_ROUNDS, value_name = "N")]
    max_rounds: usize,
    /// 调用记录落盘路径（JSONL，追加写）
    #[arg(long = "calls", default_value = "var/calls.jsonl", value_name = "FILE")]
    calls: PathBuf,
    /// 会话目录（证据与会话日志写这里）；缺省 ~/.jev-harness/sessions/<run>
    #[arg(long = "out-dir", value_name = "DIR")]
    out_dir: Option<PathBuf>,
}

#[derive(Args)]
struct H4Args {
    /// 复用已落盘的测量契约产物（不重发生成调用）
    #[arg(long = "contracts", value_name = "FILE")]
    contracts: Option<PathBuf>,
    /// 复用已落盘的测试计划产物（不重发生成调用）
    #[arg(long = "tests", value_name = "FILE")]
    tests: Option<PathBuf>,
    /// 每族只跑前 N 条调度（缺省全部 10 条）
    #[arg(long = "max-schedules", value_name = "N")]
    max_schedules: Option<usize>,
    /// 调用记录落盘路径（JSONL，追加写）
    #[arg(long = "calls", default_value = "var/calls.jsonl", value_name = "FILE")]
    calls: PathBuf,
    /// 运行产物目录；缺省 var/h4/<run>
    #[arg(long = "out-dir", value_name = "DIR")]
    out_dir: Option<PathBuf>,
    /// 报告输出路径；缺省 var/h4/<run>.json
    #[arg(long = "out", value_name = "FILE")]
    out: Option<PathBuf>,
}

#[derive(Args)]
struct H3Args {
    /// 判定器，可重复给出；第一个同时用作精化循环（开发案例、验证、影子）的判定器；缺省用离线组合 rule
    #[arg(long = "judge", value_enum, value_name = "NAME")]
    judge: Vec<JudgeName>,
    /// 只跑前 N 个未见变体（缺省全部）
    #[arg(long = "max-tasks", value_name = "N")]
    max_tasks: Option<usize>,
    /// 复用已落盘的候选产物（不重发生成调用）
    #[arg(long = "candidates", value_name = "FILE")]
    candidates: Option<PathBuf>,
    /// 调用记录落盘路径（JSONL，追加写）
    #[arg(long = "calls", default_value = "var/calls.jsonl", value_name = "FILE")]
    calls: PathBuf,
    /// 录制回放读取的记录文件
    #[arg(
        long = "replay",
        default_value = "var/calls.jsonl",
        value_name = "FILE"
    )]
    replay: PathBuf,
    /// 运行产物目录（轨迹、证据、候选产物）；缺省 var/h3/<run>
    #[arg(long = "out-dir", value_name = "DIR")]
    out_dir: Option<PathBuf>,
    /// 报告输出路径；缺省 var/h3/<run>.json
    #[arg(long = "out", value_name = "FILE")]
    out: Option<PathBuf>,
}

/// `jev ask`：单次判定。
#[derive(Args)]
struct AskArgs {
    /// 要判定什么（写进问题的 instructions，越具体越好）
    #[arg(long = "question", value_name = "TEXT")]
    question: String,
    /// 选项名；至少两个，可重复（顺序即上屏顺序）
    #[arg(long = "label", value_name = "NAME", required = true)]
    labels: Vec<String>,
    /// 给某个选项补一句判据：`--describe allow=证据齐全`
    #[arg(long = "describe", value_name = "NAME=TEXT")]
    describes: Vec<String>,
    /// 交给判定器的材料（一段文本）；不给就是「没有附加材料」
    #[arg(long = "material", value_name = "TEXT")]
    material: Option<String>,
    /// 用哪个判定器（`ask` 只支持不需要任务集的三个）
    #[arg(long = "judge", value_enum, default_value_t = JudgeName::Jev)]
    judge: JudgeName,
    /// 报告落盘路径（JSON）
    #[arg(long = "out", value_name = "FILE")]
    out: Option<PathBuf>,
}

#[derive(Args)]
struct RunArgs {
    /// 任务标识（A1…A6）
    #[arg(long = "task", default_value = "A1", value_name = "ID")]
    task: String,
    /// 判定器
    #[arg(
        long = "judge",
        value_enum,
        default_value = "rule",
        value_name = "NAME"
    )]
    judge: JudgeName,
    /// 对照臂：h1on-h2inc / h1off-h2inc / h1on-h2full / h1off-h2full
    #[arg(long = "arm", default_value = "h1on-h2inc", value_name = "ARM")]
    arm: String,
    /// 调用记录落盘路径（JSONL，追加写）
    #[arg(long = "calls", default_value = "var/calls.jsonl", value_name = "FILE")]
    calls: PathBuf,
    /// 轨迹与证据日志的输出目录
    #[arg(long = "out-dir", default_value = "var/run", value_name = "DIR")]
    out_dir: PathBuf,
}

#[derive(Args)]
struct CompareArgs {
    /// 判断器，可重复给出；缺省用离线组合 rule
    #[arg(long = "judge", value_enum, value_name = "NAME")]
    judge: Vec<JudgeName>,
    /// 只跑这些臂（可重复）；缺省四组全跑
    #[arg(long = "arm", value_name = "ARM")]
    arm: Vec<String>,
    /// 只跑前 N 个任务（缺省全部）
    #[arg(long = "max-tasks", value_name = "N")]
    max_tasks: Option<usize>,
    /// 调用记录落盘路径（JSONL，追加写）
    #[arg(long = "calls", default_value = "var/calls.jsonl", value_name = "FILE")]
    calls: PathBuf,
    /// 录制回放读取的记录文件
    #[arg(
        long = "replay",
        default_value = "var/calls.jsonl",
        value_name = "FILE"
    )]
    replay: PathBuf,
    /// 运行产物目录（轨迹与证据日志）；缺省 var/compare/<run>
    #[arg(long = "out-dir", value_name = "DIR")]
    out_dir: Option<PathBuf>,
    /// 报告输出路径；缺省 var/compare/<run>.json
    #[arg(long = "out", value_name = "FILE")]
    out: Option<PathBuf>,
    /// 不跑判定器，直接从 --out-dir 下已有的轨迹重建报告（口径与 notes 取当前代码）
    #[arg(long = "regenerate")]
    regenerate: bool,
}

#[derive(Args)]
struct ExpArgs {
    /// 判断器，可重复给出；缺省用离线组合 rule + simulated-truth
    #[arg(long = "judge", value_enum, value_name = "NAME")]
    judge: Vec<JudgeName>,
    /// 调用记录落盘路径（JSONL，追加写）
    #[arg(long = "calls", default_value = "var/calls.jsonl", value_name = "FILE")]
    calls: PathBuf,
    /// 录制回放读取的记录文件
    #[arg(
        long = "replay",
        default_value = "var/calls.jsonl",
        value_name = "FILE"
    )]
    replay: PathBuf,
    /// 报告输出路径；缺省 var/exp/<run>.json
    #[arg(long = "out", value_name = "FILE")]
    out: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Chat(args) => run_chat(args).await,
        Command::Ask(args) => run_ask(args).await,
        Command::Run(args) => run_task(args).await,
        Command::Exp(args) => run_exp(args).await,
        Command::Compare(args) => run_compare(args).await,
        Command::H3(args) => run_h3(args).await,
        Command::H4(args) => run_h4(args).await,
        Command::M4(args) => run_m4(args).await,
    }
}

/// `jev m4`：主对照 2×2 + 诊断对照 + 原生基线 + 两条残余消融 + 密度扫描，落 JSON 报告。
async fn run_m4(args: M4Args) -> ExitCode {
    let run = m4::new_run_id();
    let out_dir = args
        .out_dir
        .clone()
        .unwrap_or_else(|| PathBuf::from(format!("var/m4/{run}")));
    // 两个判断通道都要出现在复现命令里（key 只写变量名）。
    let command =
        compare::reproduction_command(&std::env::args().collect::<Vec<_>>(), &m4::JUDGE_CHANNELS);

    let mut arms = Vec::new();
    for label in &args.arm {
        match m4::Arm::from_label(label) {
            Some(arm) => arms.push(arm),
            None => {
                eprintln!(
                    "臂 `{label}` 不存在（可用：loop-llm / loop-jev / maintained-llm / maintained-jev）"
                );
                return ExitCode::from(2);
            }
        }
    }

    let mut cadences = Vec::new();
    for label in &args.cadence {
        match Cadence::parse(label) {
            Some(cadence) => cadences.push(cadence),
            None => {
                eprintln!("节奏 `{label}` 不存在（可用：triggered / every-K）");
                return ExitCode::from(2);
            }
        }
    }
    if cadences.is_empty() {
        cadences = vec![
            Cadence::Triggered,
            Cadence::FixedEvery(3),
            Cadence::FixedEvery(2),
            Cadence::FixedEvery(1),
        ];
    }
    if args.repeats == 0 {
        eprintln!("重复次数至少为 1");
        return ExitCode::from(2);
    }

    let options = M4Options {
        run: run.clone(),
        calls_path: args.calls,
        out_dir: out_dir.clone(),
        command,
        arms,
        repeats: args.repeats,
        max_tasks: args.max_tasks,
        max_schedules: args.max_schedules,
        cadences,
        scales: if args.scale.is_empty() {
            vec![3, 6]
        } else {
            args.scale.clone()
        },
        native: !args.no_native,
        ablate_reuse: !args.no_reuse_ablation,
        channel_override: args.offline_judge,
        contracts: args.contracts,
    };

    let outcome = match m4::run(&options).await {
        Ok(outcome) => outcome,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };

    let out = args
        .out
        .unwrap_or_else(|| PathBuf::from(format!("var/m4/{run}.json")));
    if let Err(error) = compare::write_json(&out, &outcome.report) {
        eprintln!("{error}");
        return ExitCode::from(2);
    }

    for line in outcome.report.summary_lines() {
        println!("{line}");
    }
    println!("报告：{}", out.display());

    if outcome.clean {
        ExitCode::SUCCESS
    } else {
        eprintln!(
            "有失败调用／被拒契约：见报告里的 calls.failed、channel_setup 与各格的 verification（如实计入，不重试）"
        );
        ExitCode::from(1)
    }
}

/// `jev ask`：一次判定，出一份读数。
///
/// 不打任务集、不进会话：**只问一句**。缺凭据、选项不合法、判定器不支持一律显式退 2。
async fn run_ask(args: AskArgs) -> ExitCode {
    let mut describes = BTreeMap::new();
    for spec in &args.describes {
        let Some((name, text)) = spec.split_once('=') else {
            eprintln!("--describe 的写法是 `--describe 选项名=说明`，收到：{spec}");
            return ExitCode::from(2);
        };
        describes.insert(name.trim().to_owned(), text.trim().to_owned());
    }
    let options = AskOptions {
        question: args.question,
        labels: args.labels,
        describes,
        material: args.material,
        judge: args.judge,
        run: compare::new_run_id(),
    };
    let report = match ask::ask(options).await {
        Ok(report) => report,
        Err(error) => {
            eprintln!("判定失败：{error}");
            return ExitCode::from(2);
        }
    };
    for line in ask::lines(&report) {
        println!("{line}");
    }
    if let Some(path) = &args.out {
        match report::write_json(path, &report) {
            Ok(()) => println!("报告：{}", path.display()),
            Err(error) => {
                eprintln!("报告没写上：{error}");
                return ExitCode::from(2);
            }
        }
    }
    ExitCode::SUCCESS
}

/// `jev chat`：进真实终端跑会话，退出时打一行读数。
async fn run_chat(args: ChatArgs) -> ExitCode {
    // TUI 要有真终端才有 raw mode / 备用屏可言：管道里跑只会得到一串转义序列。
    if !std::io::stdout().is_terminal() {
        eprintln!("chat 需要真实终端（当前标准输出不是 TTY）：请在终端里直接运行 `jev chat`");
        return ExitCode::from(2);
    }
    let workspace = match &args.workspace {
        Some(dir) => dir.clone(),
        None => match std::env::current_dir() {
            Ok(dir) => dir,
            Err(error) => {
                eprintln!("取不到当前工作目录：{error}");
                return ExitCode::from(2);
            }
        },
    };
    let run = compare::new_run_id();
    // 会话历史默认落在 `~/.jev-harness/sessions/<run>`（`JEV_HARNESS_HOME` 可覆盖）；
    // 取不到根目录时如实说一句再退回工作区的 `var/chat/<run>`，不假装历史可用。
    let out_dir = match args.out_dir.clone() {
        Some(dir) => dir,
        None => match session::SessionStore::open_default() {
            Ok(store) => store.paths(&run).dir,
            Err(error) => {
                eprintln!("会话历史目录取不到（{error}）；本次产物改落 var/chat/{run}");
                PathBuf::from(format!("var/chat/{run}"))
            }
        },
    };
    let options = ChatOptions {
        workspace,
        judge: args.judge,
        run,
        calls_path: args.calls,
        out_dir,
        debounce: Duration::from_secs(args.debounce),
        max_rounds: args.max_rounds,
    };

    match session::chat(options).await {
        Ok(stats) => {
            println!(
                "会话结束：{} 轮 · 模型 {} 次 · 判定 {} 次（防抖复用 {}）· 工具 {} 次 \
                 · 门控拒绝 {} · 人工拒绝 {}",
                stats.turns,
                stats.model_calls,
                stats.judgments,
                stats.judge_reuses,
                stats.tool_calls,
                stats.refusals,
                stats.denials
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}

/// `jev h3`：任务族 B 的 H3 对照，落 JSON 报告。
async fn run_h3(args: H3Args) -> ExitCode {
    let family = match FamilyB::load() {
        Ok(family) => family,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    let judges = if args.judge.is_empty() {
        vec![JudgeName::Rule]
    } else {
        args.judge.clone()
    };

    let run = h3::new_run_id();
    let out_dir = args
        .out_dir
        .clone()
        .unwrap_or_else(|| PathBuf::from(format!("var/h3/{run}")));
    let command = compare::reproduction_command(&std::env::args().collect::<Vec<_>>(), &judges);
    let options = H3Options {
        judges,
        run: run.clone(),
        calls_path: args.calls,
        replay_path: args.replay,
        out_dir,
        command,
        max_tasks: args.max_tasks,
        candidates: match args.candidates {
            Some(path) => CandidateSource::File(path),
            None => CandidateSource::Generate,
        },
    };

    let outcome = match h3::run(&family, &options).await {
        Ok(outcome) => outcome,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };

    let out = args
        .out
        .unwrap_or_else(|| PathBuf::from(format!("var/h3/{run}.json")));
    if let Err(error) = compare::write_json(&out, &outcome.report) {
        eprintln!("{error}");
        return ExitCode::from(2);
    }

    for line in outcome.report.summary_lines() {
        println!("{line}");
    }
    println!("报告：{}", out.display());

    if outcome.clean {
        ExitCode::SUCCESS
    } else {
        eprintln!("有调用失败：见报告里的 calls.failed 与 cost（失败如实计入，不静默重试）");
        ExitCode::from(1)
    }
}

/// `jev h4`：任务族 C 的三臂对照，落 JSON 报告。
async fn run_h4(args: H4Args) -> ExitCode {
    let run = h4::new_run_id();
    let out_dir = args
        .out_dir
        .clone()
        .unwrap_or_else(|| PathBuf::from(format!("var/h4/{run}")));
    // 生成走 LLM 通道：复现命令里带上 LLM 侧的环境变量（key 只写变量名，不写值）。
    let command = compare::reproduction_command(
        &std::env::args().collect::<Vec<_>>(),
        &h4::GENERATION_JUDGES,
    );
    let options = H4Options {
        run: run.clone(),
        calls_path: args.calls,
        out_dir,
        command,
        contracts: match args.contracts {
            Some(path) => h4::ArtifactSource::File(path),
            None => h4::ArtifactSource::Generate,
        },
        tests: match args.tests {
            Some(path) => h4::ArtifactSource::File(path),
            None => h4::ArtifactSource::Generate,
        },
        repeats: args.max_schedules,
    };

    let outcome = match h4::run(&options).await {
        Ok(outcome) => outcome,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };

    let out = args
        .out
        .unwrap_or_else(|| PathBuf::from(format!("var/h4/{run}.json")));
    if let Err(error) = compare::write_json(&out, &outcome.report) {
        eprintln!("{error}");
        return ExitCode::from(2);
    }

    for line in outcome.report.summary_lines() {
        println!("{line}");
    }
    println!("报告：{}", out.display());

    if outcome.clean {
        ExitCode::SUCCESS
    } else {
        eprintln!(
            "有生成调用失败：见报告里的 generation.failures_this_run 与 cost（失败如实计入，不重试）"
        );
        ExitCode::from(1)
    }
}

/// `jev run`：跑一个任务族 A 任务，打印逐轮轨迹（§5.4 的受控演示从这里读）。
async fn run_task(args: RunArgs) -> ExitCode {
    let family = match FamilyA::load() {
        Ok(family) => family,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    let Some(task) = family.task(&args.task) else {
        eprintln!("任务 `{}` 不在任务族 A 里", args.task);
        return ExitCode::from(2);
    };
    let Some(arm) = Arm::from_label(&args.arm) else {
        eprintln!("臂 `{}` 不存在（四组见 compare --help）", args.arm);
        return ExitCode::from(2);
    };

    let run = compare::new_run_id();
    let trace = match compare::run_single(
        &family,
        task,
        args.judge,
        arm,
        &run,
        &args.calls,
        &args.out_dir,
    )
    .await
    {
        Ok(trace) => trace,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };

    let out = args.out_dir.join(format!(
        "{}-{}-{}.trace.json",
        args.judge.cli_name(),
        arm.label(),
        task.id
    ));
    if let Err(error) = compare::write_json(&out, &trace) {
        eprintln!("{error}");
        return ExitCode::from(2);
    }
    print_trace(&trace);
    println!("轨迹：{}", out.display());
    println!("调用记录：{}", compare::absolute(&args.calls));

    match trace.acceptance.as_ref() {
        Some(acceptance) if acceptance.passed => ExitCode::SUCCESS,
        _ => {
            eprintln!("独立验收未通过：见上面的判定明细");
            ExitCode::from(1)
        }
    }
}

/// 逐轮打印一条轨迹：扰动（含失效传播路径）、判断、决策、完成声明、验收。
fn print_trace(trace: &jh_app::matching::TaskTrace) {
    println!(
        "任务 {}｜臂 {}｜判定器 {}｜{} 轮",
        trace.task, trace.arm, trace.channel, trace.rounds
    );

    for step in &trace.steps {
        let when = match step.when {
            jh_app::family::StepWhen::BeforeAsk => "提问前",
            jh_app::family::StepWhen::AfterAsk => "提问后",
        };
        println!(
            "  [扰动] 第 {} 轮{}（相位 {}，{}）：{}",
            step.round,
            when,
            step.phase,
            step.change.object(),
            step.note
        );
        if !step.invalidation.stale.is_empty() || !step.invalidation.blocked.is_empty() {
            let stale: Vec<String> = step
                .invalidation
                .stale
                .iter()
                .map(|node| node.to_string())
                .collect();
            let blocked: Vec<String> = step
                .invalidation
                .blocked
                .iter()
                .map(|action| action.to_string())
                .collect();
            println!(
                "    失效传播：待重查 [{}]｜待复核动作 [{}]",
                stale.join("、"),
                blocked.join("、")
            );
            for path in &step.invalidation.paths {
                let hops: Vec<String> = path
                    .hops
                    .iter()
                    .map(|hop| format!("{:?}", hop.kind))
                    .collect();
                println!("    路径 → {:?}：{}", path.target, hops.join(" → "));
            }
        }
    }

    for ask in &trace.asks {
        println!(
            "  [判断] 第 {} 轮（相位 {}）范围 {:?}：{}（{} ms，{} 条问题）",
            ask.round,
            ask.phase,
            ask.scope,
            ask.outcome,
            ask.latency_ms,
            ask.questions.len()
        );
    }
    for judgment in &trace.judgments {
        println!(
            "    问题 {} v{} → 读出 `{}`（真值 {}，漂移 {}，有效性 {:?}）",
            judgment.question,
            judgment.question_version,
            judgment.reading,
            judgment.truth.as_deref().unwrap_or("—"),
            judgment.drift,
            judgment.validity
        );
    }
    for reuse in &trace.reuses {
        println!(
            "    [复用] 第 {} 轮问题 {}：节点 {}（仍新鲜 {}）",
            reuse.round, reuse.question, reuse.node.0, reuse.fresh
        );
    }
    for decision in &trace.decisions {
        let gates: Vec<String> = decision
            .gates
            .iter()
            .map(|gate| format!("{}={:?}", gate.question, gate.satisfied))
            .collect();
        println!(
            "  [决策] 第 {} 轮动作 {}：{:?}（前提 {:?}，依据当前有效 {}）{}",
            decision.round,
            decision.action,
            decision.outcome,
            gates,
            decision.basis_current,
            if decision.blocked_premises.is_empty() {
                String::new()
            } else {
                format!("，被挡下的前提 {:?}", decision.blocked_premises)
            }
        );
    }
    for claim in &trace.claims {
        println!("  [完成声明] 第 {} 轮：{}", claim.round, claim.text);
    }
    if let Some(acceptance) = &trace.acceptance {
        println!("  [独立验收] 通过 = {}", acceptance.passed);
        for check in &acceptance.checks {
            println!("    {} = {}（{}）", check.name, check.passed, check.detail);
        }
        for note in &acceptance.notes {
            println!("    {note}");
        }
    }
}

/// `jev compare`：四组臂 × 全部任务，落 JSON 报告。
async fn run_compare(args: CompareArgs) -> ExitCode {
    let family = match FamilyA::load() {
        Ok(family) => family,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };

    let mut arms = Vec::new();
    for label in &args.arm {
        match Arm::from_label(label) {
            Some(arm) => arms.push(arm),
            None => {
                eprintln!(
                    "臂 `{label}` 不存在（可用：h1on-h2inc / h1off-h2inc / h1on-h2full / h1off-h2full）"
                );
                return ExitCode::from(2);
            }
        }
    }
    if arms.is_empty() {
        arms = Arm::ALL.to_vec();
    }
    let judges = if args.judge.is_empty() {
        vec![JudgeName::Rule]
    } else {
        args.judge.clone()
    };

    let run = compare::new_run_id();
    let out_dir = args
        .out_dir
        .clone()
        .unwrap_or_else(|| PathBuf::from(format!("var/compare/{run}")));
    // 重建时沿用产物目录名当运行标识：报告里的 run 与轨迹、调用记录里的 run 必须对得上。
    let run = if args.regenerate {
        out_dir
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
            .unwrap_or(run)
    } else {
        run
    };
    let command = compare::reproduction_command(&std::env::args().collect::<Vec<_>>(), &judges);

    let outcome = if args.regenerate {
        // 重建：不跑判定器，从 --out-dir 下的轨迹重新汇出报告（口径与 notes 取当前代码）。
        let options = compare::RegenerateOptions {
            judges: judges.clone(),
            arms: arms.clone(),
            run: run.clone(),
            out_dir: out_dir.clone(),
            calls_path: args.calls.clone(),
            command,
            max_tasks: args.max_tasks,
        };
        match compare::regenerate(&family, &options) {
            Ok(outcome) => outcome,
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::from(2);
            }
        }
    } else {
        let options = CompareOptions {
            judges,
            arms,
            run: run.clone(),
            calls_path: args.calls,
            replay_path: args.replay,
            out_dir,
            command,
            max_tasks: args.max_tasks,
        };
        match compare::run(&family, &options).await {
            Ok(outcome) => outcome,
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::from(2);
            }
        }
    };

    let out = args
        .out
        .unwrap_or_else(|| PathBuf::from(format!("var/compare/{run}.json")));
    if let Err(error) = compare::write_json(&out, &outcome.report) {
        eprintln!("{error}");
        return ExitCode::from(2);
    }

    for line in outcome.report.summary_lines() {
        println!("{line}");
    }
    println!("报告：{}", out.display());

    if outcome.clean {
        ExitCode::SUCCESS
    } else {
        eprintln!(
            "有判断调用失败：见报告里的 calls.failed 与各任务的 judge_failures（如实计入，不重试）"
        );
        ExitCode::from(1)
    }
}

async fn run_exp(args: ExpArgs) -> ExitCode {
    let task_set = match TaskSet::load_initial() {
        Ok(task_set) => task_set,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };

    let run = exp::new_run_id();
    let options = RunOptions {
        judges: if args.judge.is_empty() {
            JudgeName::OFFLINE_DEFAULT.to_vec()
        } else {
            args.judge
        },
        run: run.clone(),
        calls_path: args.calls,
        replay_path: args.replay,
        command: std::env::args().collect::<Vec<_>>().join(" "),
    };

    let outcome = match exp::run(&task_set, &options).await {
        Ok(outcome) => outcome,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };

    let out = args
        .out
        .unwrap_or_else(|| PathBuf::from(format!("var/exp/{run}.json")));
    if let Err(error) = report::write(&out, &outcome.report) {
        eprintln!("{error}");
        return ExitCode::from(2);
    }

    for line in outcome.report.summary_lines() {
        println!("{line}");
    }
    println!("报告：{}", out.display());

    if outcome.clean {
        ExitCode::SUCCESS
    } else {
        eprintln!(
            "有判断器没跑通：见报告里的 verification 与 calls.failed（真实通道缺 key 时属预期）"
        );
        ExitCode::from(1)
    }
}
