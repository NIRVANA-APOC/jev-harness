//! 终端宿主：把 [`Update`] 翻译成 `jh_tui::app::App` 的调用，跑事件循环并执行 [`Effect`]。
//!
//! 设计来源：`docs/tui-design.md` §2.7（dispatch 不碰终端/网络/文件，副作用由宿主执行）与
//! `docs/grok-tui-design.md` §7.1 第 5、6、10 条（等待原因是类型化枚举、三条件重绘闸门、
//! 终端状态恢复是少数保留的防御）。
//!
//! 循环里有三件事同时在跑，用 `tokio::select!` 合成一条：
//!
//! 1. **终端事件**（`EventStream`）：交给 `App::handle_event` 判成动作与 [`Effect`]，
//!    再按 Effect 驱动会话或退出；
//! 2. **在途步骤**：模型调用（正文增量流式上屏）、门控判定、工具执行，各占一个分支；
//! 3. **按需 tick**：只按 `RenderGate::poll_timeout` 唤醒（空闲档是长睡），用来转轮与重绘。
//!
//! 进入/退出真实终端走 `jh_tui::terminal::TerminalGuard`：`enter()` 开 raw mode / 备用屏 /
//! 括号粘贴，`restore()` 幂等（正常退出、panic hook、SIGTERM/SIGHUP 三条路径都调它）。

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use crossterm::event::EventStream;
use futures::StreamExt;
use jh_tui::app::{App, Effect};
use jh_tui::blocks::Block;
use jh_tui::components::{PromptChrome, StatusModel};
use jh_tui::render;
use jh_tui::screens::Screen;
use jh_tui::terminal::{Capabilities, TerminalGuard};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::session::agent::{Agent, Stats, Step, Update};
use crate::session::gate::GateDecision;
use crate::session::model::ModelFuture;
use crate::session::tools::ToolOutcome;

/// 进门控判定的 future。
type GateFuture = Pin<Box<dyn Future<Output = GateDecision>>>;
/// 进工具执行的 future。
type ToolFuture = Pin<Box<dyn Future<Output = ToolOutcome> + Send>>;

/// 跑一次会话：进终端、播首屏、直到用户退出，返回累计读数。
pub async fn run(agent: &mut Agent, capabilities: &Capabilities) -> Result<Stats, crate::Error> {
    let guard = Arc::new(TerminalGuard::enter()?);
    guard.install_panic_hook();
    tokio::spawn(restore_on_signal(Arc::clone(&guard)));

    let outcome = {
        let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
        let mut app = App::new(capabilities);
        seed(&mut app, agent);
        session_loop(&mut terminal, &mut app, agent).await
    };
    // 幂等恢复：正常退出、panic hook 与信号路径都调它，互不踩。
    guard.restore()?;
    outcome.map(|()| agent.stats())
}

/// 事件循环。
async fn session_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    agent: &mut Agent,
) -> Result<(), crate::Error> {
    let mut events = EventStream::new();
    let mut model_future: Option<ModelFuture> = None;
    let mut model_deltas: Option<UnboundedReceiver<String>> = None;
    let mut gate_future: Option<GateFuture> = None;
    let mut tool_future: Option<ToolFuture> = None;
    // 研究视图按了单步：暂停中也放行一格。
    let mut step_requested = true;

    while !app.should_quit() {
        // 1) 三条件重绘闸门：dirty ∧ 无在途帧 ∧ writer 排空（画完 flush 再 ack）。
        if app.gate().should_draw() {
            let sequence = app.gate_mut().queue_frame();
            terminal.draw(|frame| render::draw(frame, app))?;
            terminal.flush()?;
            app.gate_mut().acknowledge(sequence);
        }

        // 2) 手上有在途步骤就不推进；暂停时只有单步放行。
        let busy = model_future.is_some()
            || model_deltas.is_some()
            || gate_future.is_some()
            || tool_future.is_some();
        if !busy && (!app.paused() || step_requested) {
            step_requested = false;
            let (step, updates) = agent.advance(Instant::now());
            apply(app, updates);
            match step {
                Step::Idle => {}
                Step::Model(request) => {
                    let (future, deltas) = agent.model_run(&request);
                    model_future = Some(future);
                    model_deltas = Some(deltas);
                }
                Step::Gate(call) => gate_future = Some(Box::pin(call.ask())),
                Step::Tool(call) => {
                    let tools = agent.tools();
                    tool_future = Some(Box::pin(async move { tools.execute(&call).await }));
                }
            }
        }

        // 3) 等一个唤醒：终端事件 / 模型增量 / 在途结果 / 按需 tick。
        let sleep = tokio::time::sleep(app.gate().poll_timeout());
        tokio::pin!(sleep);
        let mut interrupt = false;
        tokio::select! {
            biased;
            event = events.next() => {
                match event {
                    Some(Ok(event)) => {
                        interrupt = handle_event(app, agent, event, &mut step_requested);
                    }
                    // 输入通道断了（终端关了、stdin 关了）：退出循环，走正常恢复路径。
                    Some(Err(_)) | None => break,
                }
            }
            delta = next_delta(&mut model_deltas), if model_deltas.is_some() => {
                if let Some(delta) = delta {
                    app.stream_assistant(&delta);
                }
            }
            result = poll_model(&mut model_future), if model_future.is_some() => {
                model_future = None;
                model_deltas = None;
                apply(app, agent.on_model_result(result));
            }
            decision = poll_gate(&mut gate_future), if gate_future.is_some() => {
                gate_future = None;
                apply(app, agent.on_gate_result(decision, Instant::now()));
            }
            outcome = poll_tool(&mut tool_future), if tool_future.is_some() => {
                tool_future = None;
                apply(app, agent.on_tool_result(outcome));
            }
            _ = &mut sleep => {
                app.advance_tick();
            }
        }

        if interrupt {
            // 取消在途的调用：future 一丢，`bash` 的子进程也跟着走（`kill_on_drop`）。
            model_future = None;
            model_deltas = None;
            gate_future = None;
            tool_future = None;
        }

        // 4) 卡片被 Esc 关掉但没给裁决：这次调用不执行，如实回给模型。
        if agent.awaiting_approval() && app.modal().is_none() {
            apply(app, agent.on_cancel());
        }
    }
    Ok(())
}

/// 处理一次终端事件，返回是否需要取消在途步骤。
fn handle_event(
    app: &mut App,
    agent: &mut Agent,
    event: crossterm::event::Event,
    step: &mut bool,
) -> bool {
    let Some(effect) = app.handle_event(event) else {
        return false;
    };
    match effect {
        // `should_quit` 已置上，循环下一圈就退出。
        Effect::Quit => false,
        Effect::Submit(text) => {
            // 首屏提交会由 App 自己切到会话屏；这里只把话送进会话。
            apply(app, agent.submit(text));
            false
        }
        Effect::Approval(outcome) => {
            apply(app, agent.on_approval(outcome));
            false
        }
        Effect::Step => {
            *step = true;
            false
        }
        Effect::SetPaused(paused) => {
            if !paused {
                *step = true;
            }
            false
        }
        Effect::Interrupt => {
            apply(app, agent.on_interrupt());
            true
        }
    }
}

/// 把会话的更新翻译成界面的调用。
fn apply(app: &mut App, updates: Vec<Update>) {
    for update in updates {
        match update {
            Update::User { text, at } => app.push_block(Block::user_at(text, at)),
            Update::AssistantEnd => app.finish_assistant(),
            Update::Notice { text, tone } => app.push_block(Block::notice(text).with_tone(tone)),
            Update::PlainNotice(text) => app.push_block(Block::plain_notice(text)),
            Update::ToolBegin { name, args } => app.begin_tool(name, args),
            Update::ToolEnd { status, detail } => {
                let detail = (!detail.is_empty()).then_some(detail);
                app.finish_tool(status, detail);
            }
            Update::Decision(trace) => {
                app.push_block(Block::decision((*trace).clone()));
                app.set_decision(Some(*trace));
            }
            Update::Approval(request) => app.open_approval(*request),
            Update::Status(status) => *app.status_mut() = status,
            Update::Panels(panels) => *app.panels_mut() = *panels,
            Update::PromptInfo(info) => {
                let title = app.prompt_chrome().title.clone();
                app.set_prompt_chrome(PromptChrome {
                    title,
                    info: Some(info),
                });
            }
            Update::HeaderTitle(title) => app.header_model_mut().title = title,
        }
    }
}

/// 首屏与常驻区间的初值：工作目录、接线状态、提示框信息行与面板数据都来自会话。
fn seed(app: &mut App, agent: &Agent) {
    app.header_model_mut().title = agent.header_title();
    app.set_startup_note(agent.startup_note());
    app.set_prompt_chrome(
        PromptChrome::unwired()
            .with_title("jev chat")
            .with_info(agent.prompt_info()),
    );
    *app.status_mut() = StatusModel::idle();
    *app.panels_mut() = agent.panels();
    // 首屏与主界面是同一份状态：先把屏幕停在首屏（设计 §5.2）。
    app.set_screen(Screen::Startup);
}

/// 模型正文增量：通道关了（发送端已丢）就交出 `None`，让模型结果分支去收尾。
async fn next_delta(deltas: &mut Option<UnboundedReceiver<String>>) -> Option<String> {
    match deltas {
        Some(deltas) => deltas.recv().await,
        None => std::future::pending().await,
    }
}

async fn poll_model(
    future: &mut Option<ModelFuture>,
) -> Result<crate::session::model::ModelTurn, jh_core::Error> {
    match future {
        Some(future) => future.as_mut().await,
        None => std::future::pending().await,
    }
}

async fn poll_gate(future: &mut Option<GateFuture>) -> GateDecision {
    match future {
        Some(future) => future.as_mut().await,
        None => std::future::pending().await,
    }
}

async fn poll_tool(future: &mut Option<ToolFuture>) -> ToolOutcome {
    match future {
        Some(future) => future.as_mut().await,
        None => std::future::pending().await,
    }
}

/// 信号 → 恢复护栏 → 退出。`restore()` 幂等，与 Drop、panic hook 不冲突。
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
