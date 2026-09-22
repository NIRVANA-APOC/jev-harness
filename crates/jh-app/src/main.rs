//! `jev` 命令：`chat` / `ask` 未接线；`run` 跑一个任务族 A 任务的匹配循环；`exp` 跑 M0 初始任务集
//! 的 JSON 基线；`compare` 跑 H1/H2 四组臂对照。
//!
//! ```text
//! cargo run -p jh-app -- exp                       # 离线默认：规则 + 模拟真值
//! cargo run -p jh-app -- run --task A5 --judge rule        # 单任务端到端（含 §5.4 受控演示）
//! cargo run -p jh-app -- compare --judge rule              # 四组臂离线对照
//! TYPESAFE_API_KEY=... cargo run -p jh-app -- compare --judge jev
//! OPENAI_API_KEY=... OPENAI_MODEL=deepseek-flash OPENAI_BASE_URL=https://api.deepseek.com \
//!   cargo run -p jh-app -- compare --judge llm
//! ```
//!
//! 真实通道的环境变量都必填（`TYPESAFE_API_KEY`；`OPENAI_API_KEY` + `OPENAI_MODEL` +
//! `OPENAI_BASE_URL`）：缺任何一项该判定器整段标「未验证」，不会拿别的判定器的结果顶替，
//! 也不会把 key 发往不是本人配置的端点。

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use jh_app::compare::{self, CompareOptions};
use jh_app::exp::{self, RunOptions};
use jh_app::family::FamilyA;
use jh_app::judge::JudgeName;
use jh_app::matching::Arm;
use jh_app::report;
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
    /// 交互式对话（TUI）
    Chat,
    /// 单次 Jev 判定
    Ask,
    /// 跑一个任务族 A 任务的匹配循环（端到端）
    Run(RunArgs),
    /// 实验：跑初始任务集并输出 JSON 基线
    Exp(ExpArgs),
    /// 对照：H1（依赖失效传播开 / 关）× H2（增量 / 全量）四组臂跑任务族 A
    Compare(CompareArgs),
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
        Command::Chat | Command::Ask => {
            eprintln!("未实现");
            ExitCode::from(2)
        }
        Command::Run(args) => run_task(args).await,
        Command::Exp(args) => run_exp(args).await,
        Command::Compare(args) => run_compare(args).await,
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
        .unwrap_or_else(|| PathBuf::from(format!("var/compare/{run}")));
    let options = CompareOptions {
        judges,
        arms,
        run: run.clone(),
        calls_path: args.calls,
        replay_path: args.replay,
        out_dir,
        command: std::env::args().collect::<Vec<_>>().join(" "),
        max_tasks: args.max_tasks,
    };

    let outcome = match compare::run(&family, &options).await {
        Ok(outcome) => outcome,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
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
