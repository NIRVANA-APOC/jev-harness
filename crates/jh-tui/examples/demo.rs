//! 一屏渲染全部组件。
//!
//! - **非 TTY**（管道 / CI）：把两屏纯文本单帧打到 stdout 后退出——离线、可复现。
//! - **TTY**：进备用屏跑交互循环：`Enter` 发送、`Alt+Enter` 换行、`PgUp/PgDn` 翻历史、
//!   `F2` 唤起审批卡（demo 自己的钩子，不进动作注册表）、`y/a/n` 裁决、`Ctrl+C` 退出。
//!
//! 信号触发的终端恢复：`SIGTERM` / `SIGHUP` 到来时先 [`TerminalGuard::restore`] 再退出，
//! 免得终端被留在 raw mode + 备用屏里（设计 §7.1 第 10 条）。
//!
//! demo 里的「助手回复 / 工具结果」都是本地伪造的，不调模型、不联网。

use std::io::IsTerminal;
use std::sync::Arc;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use jh_tui::app::{App, Effect};
use jh_tui::blocks::{Block, Choice, DecisionTrace, ToolStatus};
use jh_tui::components::{ApprovalRequest, StatusModel, TurnPhase, WaitingReason};
use jh_tui::render;
use jh_tui::terminal::{Capabilities, TerminalGuard};
use ratatui::Terminal;
use ratatui::backend::{Backend, CrosstermBackend, TestBackend};

const WIDTH: u16 = 100;
const HEIGHT: u16 = 20;
const MODEL: &str = "deepseek-chat";
const TOKENS: (u32, u32) = (12_480, 64_000);

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let capabilities = Capabilities::detect();
    if !std::io::stdout().is_terminal() {
        print_snapshots(&capabilities);
        return Ok(());
    }
    run_interactive(capabilities).await
}

/// 非 TTY 路径：静态打两屏出来（正常状态 / 审批卡状态）。
fn print_snapshots(capabilities: &Capabilities) {
    println!(
        "jh-tui demo —— 非 TTY：打印纯文本单帧（{WIDTH}×{HEIGHT}），字形={:?}，色彩={:?}",
        capabilities.glyphs, capabilities.color
    );
    for (title, with_approval) in [
        ("状态一：对话 + Jev 判定轨迹（概率不塌缩）", false),
        ("状态二：审批卡（单槽位模态，键盘归卡片）", true),
    ] {
        let mut app = App::new(capabilities);
        seed(&mut app, with_approval);
        let mut terminal =
            Terminal::new(TestBackend::new(WIDTH, HEIGHT)).expect("TestBackend 可用");
        terminal
            .draw(|frame| render::draw(frame, &mut app))
            .expect("绘制不失败");
        println!("\n===== {title} =====");
        for line in render::screen_lines(terminal.backend().buffer()) {
            println!("{line}");
        }
    }
    println!("\n（在 TTY 下运行会进备用屏跑交互循环；快捷键见底部快捷键栏）");
}

/// TTY 路径：备用屏 + raw mode + 信号恢复。
async fn run_interactive(capabilities: Capabilities) -> std::io::Result<()> {
    let guard = Arc::new(TerminalGuard::enter()?);
    guard.install_panic_hook();
    tokio::spawn(restore_on_signal(Arc::clone(&guard)));

    let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;
    let mut app = App::new(&capabilities);
    seed(&mut app, false);

    let outcome = run_loop(&mut terminal, &mut app);
    // 幂等恢复：正常退出、panic hook 与信号路径都调它，互不踩。
    guard.restore()?;
    outcome
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
) -> std::io::Result<()> {
    while !app.should_quit() {
        // 按需渲染闸门：dirty ∧ 无在途帧 ∧ writer 排空（画完 flush 再 ack）。
        if app.gate().should_draw() {
            let sequence = app.gate_mut().queue_frame();
            terminal.draw(|frame| render::draw(frame, app))?;
            terminal.backend_mut().flush()?;
            app.gate_mut().acknowledge(sequence);
        }
        // 空闲不重绘：没事件就按 TickDemand 给的时长睡过去（None 档等于长睡）。
        if event::poll(app.gate().poll_timeout())? {
            let event = event::read()?;
            demo_hook(app, &event);
            if let Some(effect) = app.handle_event(event) {
                apply_effect(app, effect);
            }
        } else {
            app.advance_tick();
        }
    }
    Ok(())
}

/// demo 自己的按键钩子：`F2` 唤起审批卡（真实宿主里这是 agent 循环推过来的请求）。
fn demo_hook(app: &mut App, event: &Event) {
    if let Event::Key(KeyEvent { code, kind, .. }) = event
        && *kind != KeyEventKind::Release
        && *code == KeyCode::F(2)
    {
        app.open_approval(approval_request());
    }
}

/// demo 的假宿主：把副作用变成屏幕上看得见的块。
fn apply_effect(app: &mut App, effect: Effect) {
    match effect {
        Effect::Submit(text) => {
            *app.status_mut() = StatusModel::running(WaitingReason::JevJudging)
                .with_model(MODEL)
                .with_tokens(TOKENS.0, TOKENS.1);
            app.push_block(Block::assistant(format!(
                "收到「{text}」。demo 不调真实模型，这里只回显，并假装做了一次 Jev 判定。"
            )));

            let trace = DecisionTrace::new(
                "shell_command_allowed",
                vec![
                    Choice::new("usr:allow", "允许一次", 0.47),
                    Choice::new("usr:deny", "拒绝", 0.44),
                ],
            )
            .with_prompt("是否允许执行 `git status --short`？")
            .with_confidence(0.55)
            .with_chosen(0);
            app.push_block(Block::decision(trace.clone()));
            // 同一份判定喂给状态行：概率与置信度原样上屏。
            app.set_decision(Some(trace));

            *app.status_mut() = StatusModel::running(WaitingReason::ToolRunning)
                .with_model(MODEL)
                .with_tokens(TOKENS.0, TOKENS.1);
            app.push_block(Block::tool("bash", "git status --short", ToolStatus::Ok));

            *app.status_mut() = StatusModel {
                phase: Some(TurnPhase::Done),
                waiting: None,
                model: Some(MODEL.to_owned()),
                tokens: Some(TOKENS),
            };
        }
        Effect::Approval(outcome) => {
            let label = match outcome {
                jh_tui::app::ApprovalOutcome::AllowOnce => "允许一次",
                jh_tui::app::ApprovalOutcome::AllowAlways => "本次会话总是允许",
                jh_tui::app::ApprovalOutcome::Deny => "拒绝",
            };
            app.push_block(Block::tool(
                "permission",
                format!("{}（{}）", "git push --force origin main", label),
                ToolStatus::Ok,
            ));
            app.set_decision(None);
            *app.status_mut() = StatusModel::idle()
                .with_model(MODEL)
                .with_tokens(TOKENS.0, TOKENS.1);
        }
        Effect::Quit | Effect::Interrupt => {}
    }
}

/// 信号 → 恢复护栏 → 退出。`restore()` 是幂等的，与 Drop、panic hook 不冲突。
async fn restore_on_signal(guard: Arc<TerminalGuard>) {
    use tokio::signal::unix::{SignalKind, signal};
    let (Ok(mut terminate), Ok(mut hangup)) = (
        signal(SignalKind::terminate()),
        signal(SignalKind::hangup()),
    ) else {
        // 注册不上就退化成「只靠正常退出与 panic hook」，不再硬撑。
        return;
    };
    tokio::select! {
        _ = terminate.recv() => {}
        _ = hangup.recv() => {}
    }
    let _ = guard.restore();
    std::process::exit(130);
}

/// 与快照测试同款的初始一屏。
fn seed(app: &mut App, with_approval: bool) {
    app.push_block(Block::user("帮我看一下未提交的改动"));
    app.push_block(Block::assistant(
        "好的，我先跑 git status，再用 Jev 判断是否需要你确认。",
    ));
    app.push_block(Block::tool("bash", "git status --short", ToolStatus::Ok));
    app.push_block(Block::decision(
        DecisionTrace::new(
            "shell_command_allowed",
            vec![
                Choice::new("usr:allow", "允许一次", 0.71),
                Choice::new("usr:deny", "拒绝", 0.22),
            ],
        )
        .with_prompt("是否允许执行 `git push --force`？")
        .with_confidence(0.64)
        .with_chosen(0),
    ));
    app.set_decision(Some(
        DecisionTrace::new(
            "shell_command_allowed",
            vec![
                Choice::new("usr:allow", "允许一次", 0.71),
                Choice::new("usr:deny", "拒绝", 0.22),
            ],
        )
        .with_confidence(0.64),
    ));
    *app.status_mut() = StatusModel::running(WaitingReason::JevJudging)
        .with_model(MODEL)
        .with_tokens(TOKENS.0, TOKENS.1);
    app.prompt_mut().insert_str("顺便看一下 CI 配置");
    if with_approval {
        app.open_approval(approval_request());
    }
}

fn approval_request() -> ApprovalRequest {
    ApprovalRequest::new("bash", "git push --force origin main")
        .with_body("该命令会改写远端历史，且无法从本地恢复。")
}
