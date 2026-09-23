//! 设计预览：把 `docs/tui-design.md` 的三屏与三个研究面板画出来。
//!
//! - **非 TTY**（管道 / CI）：逐屏打纯文本单帧（首屏 / 主界面 / 主界面+审批卡 /
//!   研究视图的三个面板 / 无色回退档），离线、可复现，用来肉眼核对线框。
//! - **TTY**：进备用屏跑交互循环：`Enter` 发送、`Alt+Enter` 换行、`Ctrl+O` 切研究视图、
//!   `Tab` / `Shift+Tab` 切面板、`s` 单步、`Space` 暂停、`Esc` 返回会话、`Ctrl+C` 退出。
//!
//! 数据全是本地的：不调模型、不联网（接线见设计文档 §9 批次四）。
//! 信号触发的终端恢复：`SIGTERM` / `SIGHUP` 到来时先 [`TerminalGuard::restore`] 再退出。

use std::io::IsTerminal;
use std::sync::Arc;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use jh_tui::app::{App, Effect};
use jh_tui::blocks::{Block, Choice, DecisionTrace, ToolStatus};
use jh_tui::components::{
    ApprovalRequest, PromptChrome, StatusModel, Tone, TurnPhase, WaitingReason,
};
use jh_tui::panels::{
    ActionRow, Distribution, EdgeRow, EvidenceRow, EvidenceView, JudgmentView, PanelId, PathRow,
    PropagationView, Validity,
};
use jh_tui::render;
use jh_tui::screens::Screen;
use jh_tui::terminal::{Capabilities, TerminalGuard};
use ratatui::Terminal;
use ratatui::backend::{Backend, CrosstermBackend, TestBackend};

const WIDTH: u16 = 100;
const HEIGHT: u16 = 24;
const MODEL: &str = "deepseek-chat";
const JUDGE: &str = "jev-1.13.0";
const TOKENS: (u32, u32) = (12_480, 64_000);
const WORKSPACE: &str = "~/Documents/jev-harness";

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let capabilities = Capabilities::detect();
    if !std::io::stdout().is_terminal() {
        print_screens(&capabilities);
        return Ok(());
    }
    run_interactive(capabilities).await
}

/// 非 TTY 路径：逐屏打纯文本单帧。
fn print_screens(capabilities: &Capabilities) {
    println!(
        "jh-tui 设计预览 —— 非 TTY：打印纯文本单帧（{WIDTH}×{HEIGHT}），字形={:?}，色彩={:?}",
        capabilities.glyphs, capabilities.color
    );

    let screens: [(&str, Screen, Option<PanelId>); 5] = [
        ("首屏（启动）", Screen::Startup, None),
        ("主界面（会话）", Screen::Agent, None),
        (
            "研究视图 · 判断详情",
            Screen::Research,
            Some(PanelId::Judgment),
        ),
        (
            "研究视图 · 事件与证据",
            Screen::Research,
            Some(PanelId::Evidence),
        ),
        (
            "研究视图 · 失效传播",
            Screen::Research,
            Some(PanelId::Propagation),
        ),
    ];
    for (title, screen, panel) in screens {
        let mut app = App::new(capabilities);
        seed(&mut app, screen, panel);
        print_screen(title, &mut app);
    }

    // 审批卡压在回滚区底部，键盘归卡片。
    let mut app = App::new(capabilities);
    seed(&mut app, Screen::Agent, None);
    app.open_approval(approval_request());
    print_screen("主界面 · 审批卡（单槽位模态）", &mut app);

    // 降级档：无色 + 回退字形。
    let plain = Capabilities::from_env_lookup(
        |key| (key == "NO_COLOR").then(|| "1".to_owned()),
        true,
        true,
    );
    let mut app = App::new(&plain);
    seed(&mut app, Screen::Research, Some(PanelId::Judgment));
    print_screen("研究视图 · 无色 + 回退字形", &mut app);

    println!("\n（在 TTY 下运行会进备用屏跑交互循环；快捷键见底部快捷键栏）");
}

fn print_screen(title: &str, app: &mut App) {
    let mut terminal = Terminal::new(TestBackend::new(WIDTH, HEIGHT)).expect("TestBackend 可用");
    terminal
        .draw(|frame| render::draw(frame, app))
        .expect("绘制不失败");
    println!("\n===== {title} =====");
    for line in render::screen_lines(terminal.backend().buffer()) {
        println!("{line}");
    }
}

/// TTY 路径：备用屏 + raw mode + 信号恢复。
async fn run_interactive(capabilities: Capabilities) -> std::io::Result<()> {
    let guard = Arc::new(TerminalGuard::enter()?);
    guard.install_panic_hook();
    tokio::spawn(restore_on_signal(Arc::clone(&guard)));

    let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;
    let mut app = App::new(&capabilities);
    seed(&mut app, Screen::Agent, None);

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

/// 预览自己的按键钩子：`F2` 唤起审批卡（真实宿主里这是 agent 循环推过来的请求）。
fn demo_hook(app: &mut App, event: &Event) {
    if let Event::Key(KeyEvent { code, kind, .. }) = event
        && *kind != KeyEventKind::Release
        && *code == KeyCode::F(2)
    {
        app.open_approval(approval_request());
    }
}

/// 预览的假宿主：把副作用变成屏幕上看得见的块。
fn apply_effect(app: &mut App, effect: Effect) {
    match effect {
        Effect::Submit(text) => {
            let reply =
                format!("收到「{text}」。预览不调真实模型，这里只回显，并假装做了一次 Jev 判定。");
            app.push_block(Block::user_at(text, "14:22"));
            *app.status_mut() = StatusModel::running(WaitingReason::JevJudging)
                .with_model(MODEL)
                .with_tokens(TOKENS.0, TOKENS.1);
            app.push_block(Block::assistant(reply));
            app.push_block(Block::notice(
                "判定 12ms · 采纳 usr:allow 0.71 · margin 0.49 · 倾向",
            ));
            let trace = sample_trace();
            app.push_block(Block::decision(trace.clone()));
            app.set_decision(Some(trace));
            app.push_block(Block::tool("bash", "git status --short", ToolStatus::Ok));
            *app.status_mut() = StatusModel {
                phase: Some(TurnPhase::Done),
                waiting: None,
                model: Some(MODEL.to_owned()),
                tokens: Some(TOKENS),
            };
            app.push_block(Block::plain_notice("用时 1.5s"));
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
        Effect::Step => {
            app.push_block(Block::plain_notice("单步：宿主推进一步（预览不接线）"));
        }
        Effect::SetPaused(paused) => {
            let text = if paused {
                "暂停：宿主挂起循环"
            } else {
                "继续"
            };
            app.push_block(Block::plain_notice(text));
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

fn sample_trace() -> DecisionTrace {
    DecisionTrace::new(
        "shell_command_allowed",
        vec![
            Choice::new("usr:allow", "允许一次", 0.71),
            Choice::new("usr:deny", "拒绝", 0.22),
            Choice::new("usr:always", "总是允许", 0.07),
        ],
    )
    .with_prompt("是否允许执行 `git push --force`？")
    .with_confidence(0.64)
    .with_chosen(0)
}

fn approval_request() -> ApprovalRequest {
    ApprovalRequest::new("bash", "git push --force origin main")
        .with_body("该命令会改写远端历史，且无法从本地恢复。")
}

/// 三屏各自的初始数据。
fn seed(app: &mut App, screen: Screen, panel: Option<PanelId>) {
    app.header_model_mut().title = WORKSPACE.to_owned();
    app.header_model_mut().tokens = Some(TOKENS);
    app.set_startup_note("rule · 未接线");
    app.set_prompt_chrome(
        PromptChrome::default().with_info(format!("{MODEL} · {JUDGE} · always-approve")),
    );
    seed_panels(app);
    *app.status_mut() = StatusModel::running(WaitingReason::JevJudging)
        .with_model(MODEL)
        .with_tokens(TOKENS.0, TOKENS.1);
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

    match screen {
        Screen::Startup => {}
        Screen::Agent => {
            app.set_screen(Screen::Agent);
            seed_conversation(app);
            app.prompt_mut().insert_str("顺便看一下 CI 配置");
        }
        Screen::Research => {
            seed_conversation(app);
            if let Some(panel) = panel {
                app.set_screen(Screen::Research);
                while app.panel() != panel {
                    let _ = app.apply(jh_tui::actions::ActionId::NextPanel);
                }
            }
        }
    }
}

/// 会话流的前几块：一屏里放齐用户 / 助手 / 阶段行 / 工具 / 判定。
fn seed_conversation(app: &mut App) {
    app.push_block(Block::user_at("帮我看一下未提交的改动", "14:18"));
    app.push_block(Block::assistant(
        "我先跑一下 git status，再用 Jev 判断是否要你确认。",
    ));
    app.push_block(Block::plain_notice(
        "判定 12ms · 采纳 usr:allow 0.71 · margin 0.49",
    ));
    app.push_block(Block::tool("bash", "git status --short", ToolStatus::Ok));
    app.push_block(Block::decision(sample_trace()));
}

/// 三个研究面板的离线样例数据（字段与 `jh-core` 的领域类型一一对应，见设计文档 §8）。
fn seed_panels(app: &mut App) {
    let panels = app.panels_mut();
    panels.judgment = Some(JudgmentView {
        question: "shell_command_allowed".to_owned(),
        question_version: 3,
        current_version: true,
        distribution: Some(Distribution::Choice {
            choices: vec![
                Choice::new("usr:allow", "允许一次", 0.71),
                Choice::new("usr:deny", "拒绝", 0.22),
                Choice::new("usr:always", "总是允许", 0.07),
            ],
            chosen: Some(0),
        }),
        confidence: Some(0.64),
        source: "RealApi".to_owned(),
        executor: Some(JUDGE.to_owned()),
        request_id: Some("3f9c1a".to_owned()),
        snapshot: Some("7b1e".to_owned()),
        validity: Validity::NeedsRecheck,
        projection: vec![
            "app.log: 全部字段".to_owned(),
            "config: port, mode".to_owned(),
        ],
        latency_ms: Some(12),
        usage: Some((356, 38)),
    });
    panels.evidence = EvidenceView {
        rows: vec![
            EvidenceRow {
                id: 1,
                at: "14:18:02".to_owned(),
                source: "app.log".to_owned(),
                object: "app.log".to_owned(),
                version: 2,
                hash: "1a77c0".to_owned(),
                summary: "启动时端口 8080 被占用".to_owned(),
                retracted: false,
            },
            EvidenceRow {
                id: 2,
                at: "14:19:40".to_owned(),
                source: "config".to_owned(),
                object: "config".to_owned(),
                version: 3,
                hash: "9d21e4".to_owned(),
                summary: "监听端口被改回 9090".to_owned(),
                retracted: false,
            },
            EvidenceRow {
                id: 3,
                at: "14:22:01".to_owned(),
                source: "app.log".to_owned(),
                object: "app.log".to_owned(),
                version: 3,
                hash: "3f9c1a".to_owned(),
                summary: "端口占用的报错消失（样本只到 14:22）".to_owned(),
                retracted: false,
            },
            EvidenceRow {
                id: 4,
                at: "14:23:15".to_owned(),
                source: "probe".to_owned(),
                object: "runtime.metrics".to_owned(),
                version: 1,
                hash: "c0ffee".to_owned(),
                summary: "采样窗口过短，指标不可用".to_owned(),
                retracted: true,
            },
        ],
    };
    panels.propagation = PropagationView {
        cause: Some("追加证据 #3（app.log v3）".to_owned()),
        stale: vec!["cause_primary v2".to_owned(), "premise_holds v1".to_owned()],
        blocked: vec!["apply_fix".to_owned()],
        paths: vec![PathRow {
            target: "cause_primary".to_owned(),
            hops: "读取 → 支持 → 前提".to_owned(),
        }],
        edges: vec![
            EdgeRow {
                from: "app.log".to_owned(),
                kind: "read".to_owned(),
                to: "cause_primary".to_owned(),
                origin: "确定性".to_owned(),
            },
            EdgeRow {
                from: "cause_primary".to_owned(),
                kind: "premise".to_owned(),
                to: "apply_fix".to_owned(),
                origin: "确定性".to_owned(),
            },
            EdgeRow {
                from: "metrics_within_baseline".to_owned(),
                kind: "conflict".to_owned(),
                to: "cause_primary".to_owned(),
                origin: "模型提出".to_owned(),
            },
        ],
        actions: vec![
            ActionRow {
                action: "apply_fix".to_owned(),
                check: "被挡下：前提 cause_primary".to_owned(),
                tone: Tone::Warning,
            },
            ActionRow {
                action: "collect_metrics".to_owned(),
                check: "可发出".to_owned(),
                tone: Tone::Success,
            },
        ],
    };
}
