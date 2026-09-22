use std::process::ExitCode;

use clap::{Parser, Subcommand};

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
    /// 实验：跑任务集并出统计
    Exp,
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Chat | Command::Ask | Command::Run | Command::Exp => {
            println!("未实现");
            ExitCode::from(2)
        }
    }
}
