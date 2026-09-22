//! `jev` 命令：`chat` / `ask` / `run` 未接线，`exp` 跑初始任务集并出 JSON 基线报告。
//!
//! ```text
//! cargo run -p jh-app -- exp                       # 离线默认：规则 + 模拟真值
//! cargo run -p jh-app -- exp --judge rule --judge simulated-truth --judge replay
//! TYPESAFE_API_KEY=... cargo run -p jh-app -- exp --judge jev
//! OPENAI_API_KEY=... OPENAI_MODEL=deepseek-flash OPENAI_BASE_URL=https://api.deepseek.com \
//!   cargo run -p jh-app -- exp --judge llm
//! ```
//!
//! 真实通道的环境变量都必填（`TYPESAFE_API_KEY`；`OPENAI_API_KEY` + `OPENAI_MODEL` +
//! `OPENAI_BASE_URL`）：缺任何一项该判断器整段标「未验证」，不会拿别的判断器的结果顶替，
//! 也不会把 key 发往不是本人配置的端点。

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use jh_app::exp::{self, RunOptions};
use jh_app::judge::JudgeName;
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
    /// 运行一次完整任务
    Run,
    /// 实验：跑初始任务集并输出 JSON 基线
    Exp(ExpArgs),
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
        Command::Chat | Command::Ask | Command::Run => {
            eprintln!("未实现");
            ExitCode::from(2)
        }
        Command::Exp(args) => run_exp(args).await,
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
