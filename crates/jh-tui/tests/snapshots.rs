//! 组件级与整屏 `TestBackend` 快照：全部离线，不碰真实终端。
//!
//! 每个组件至少一条缓冲区快照（回滚区 2 条、提示框 3 条、状态行 2 条、快捷键栏 2 条、
//! 审批卡 2 条、整屏 3 条），另有几条直接断言单元格样式（失焦降饱和、无色档）。
//!
//! 期望值一律写成多行原始字符串；行尾空白与行尾空行不参与比较（那是画布高度留下的），
//! 行首空行参与比较——提示框光标那条就靠它锁住上方回滚区的高度。

mod common;

use common::{
    assert_screen, capabilities, capabilities_plain, render_app, render_app_with_cursor,
    render_lines, render_lines_cells,
};
use jh_tui::actions::KeyOwner;
use jh_tui::app::App;
use jh_tui::blocks::{Block, BlockCtx, Choice, DecisionTrace, ToolStatus};
use jh_tui::components::{
    ActiveModal, ApprovalCard, ApprovalRequest, Prompt, Scrollback, ShortcutBar, StatusModel,
    TurnPhase, WaitingReason,
};
use jh_tui::theme::Theme;

/// 构造一个固定宽度的块上下文（真彩 + 花体，快照确定）。
fn ctx_for<'a>(theme: &'a Theme, width: usize) -> BlockCtx<'a> {
    BlockCtx {
        width,
        theme,
        glyphs: capabilities().glyphs,
        frame: 0,
        decision: None,
    }
}

#[test]
fn scrollback_snapshot_shows_the_visible_window() {
    let theme = capabilities().theme();
    let ctx = ctx_for(&theme, 44);
    let mut scrollback = Scrollback::new();
    scrollback.push(Block::user("看一下仓库状态"));
    scrollback.push(Block::assistant(
        "我先跑一下 git status，看有没有未提交改动。",
    ));
    scrollback.push(Block::tool("bash", "git status --short", ToolStatus::Ok));

    let window = scrollback.window(&ctx, 8);
    assert!(window.at_bottom());
    let screen = render_lines(
        44,
        8,
        window.lines.into_iter().map(|line| line.content).collect(),
    );
    assert_screen(
        &screen,
        "❯ 看一下仓库状态

◆我先跑一下 git status，看有没有未提交改动。

▸ bash git status --short",
    );
}

#[test]
fn scrollback_snapshot_scrolled_up_shifts_the_window() {
    let theme = capabilities().theme();
    let ctx = ctx_for(&theme, 44);
    let mut scrollback = Scrollback::new();
    for index in 1..=6 {
        scrollback.push(Block::user(format!("第 {index} 条消息")));
    }
    let max = scrollback.max_offset(&ctx, 6);
    scrollback.scroll_up(max, max);

    let window = scrollback.window(&ctx, 6);
    assert!(!window.at_bottom());
    assert_eq!(window.offset, max);
    let screen = render_lines(
        44,
        6,
        window.lines.into_iter().map(|line| line.content).collect(),
    );
    // 上翻到底：可见窗口换成最早的三条，而不是继续跟着最新输出。
    assert_screen(
        &screen,
        "❯ 第 1 条消息

❯ 第 2 条消息

❯ 第 3 条消息",
    );
}

#[test]
fn prompt_snapshot_renders_multiline_input() {
    let theme = capabilities().theme();
    let ctx = ctx_for(&theme, 40);
    let prompt = Prompt::with_text("帮我查一下\ncrates/jh-tui 的依赖树");
    let screen = render_lines(
        40,
        2,
        prompt
            .render(&ctx, true)
            .into_iter()
            .map(|line| line.content)
            .collect(),
    );
    assert_screen(
        &screen,
        "❯ 帮我查一下
  crates/jh-tui 的依赖树",
    );
}

#[test]
fn prompt_snapshot_soft_wraps_long_input() {
    let theme = capabilities().theme();
    let ctx = ctx_for(&theme, 24);
    let prompt =
        Prompt::with_text("这是一条很长的输入，用来验证软折行之后每一行都不会超过内容宽度。");
    let lines = prompt.render(&ctx, true);
    let screen = render_lines(
        24,
        lines.len() as u16,
        lines.into_iter().map(|line| line.content).collect(),
    );
    assert_screen(
        &screen,
        "❯ 这是一条很长的输入，用
  来验证软折行之后每一行
  都不会超过内容宽度。",
    );
}

#[test]
fn prompt_cursor_sits_at_the_edit_position() {
    let mut app = App::new(&capabilities());
    app.prompt_mut().insert_str("ab\n中");
    let (screen, cursor) = render_app_with_cursor(30, 8, &mut app);
    // 布局 = 回滚区 4 行（空）/ 状态行 / 提示框 2 行 / 快捷键栏。
    assert_screen(
        &screen,
        "\n\n\n\n● 空闲\n❯ ab\n  中\nCtrl+c 退出 · Esc 取消 …",
    );
    // 光标在第 2 个提示行的第 2 列：`❯ ` 占两列，`中` 占两列。
    assert_eq!(cursor, (4, 6), "终端光标应落在编辑位置");
}

#[test]
fn status_line_snapshot_shows_waiting_and_decision_probability() {
    let theme = capabilities().theme();
    let trace = DecisionTrace::new(
        "shell_command_allowed",
        vec![
            Choice::new("usr:allow", "允许一次", 0.71),
            Choice::new("usr:deny", "拒绝", 0.22),
            Choice::new("usr:always", "总是允许", 0.07),
        ],
    )
    .with_confidence(0.64);
    let ctx = BlockCtx {
        width: 96,
        theme: &theme,
        glyphs: capabilities().glyphs,
        frame: 2,
        decision: Some(&trace),
    };
    let model = StatusModel::running(WaitingReason::JevJudging)
        .with_model("deepseek-chat")
        .with_tokens(12_480, 64_000);
    let line = model.render(&ctx);
    let screen = render_lines(96, 1, vec![line.content]);
    // 概率与置信度原样上屏（0.71 / margin 0.49 / confidence 0.64），末尾按宽度裁掉。
    assert_screen(
        &screen,
        "⠹ Jev 判定中 · 判定 usr:allow 0.71 · margin 0.49 · confidence 0.64 · 倾向 · deepseek-chat · 12.…",
    );
}

#[test]
fn status_line_snapshot_shows_idle_and_failure() {
    let theme = capabilities().theme();
    let ctx = ctx_for(&theme, 60);
    let idle = StatusModel::idle().with_model("deepseek-chat");
    let screen = render_lines(60, 1, vec![idle.render(&ctx).content]);
    assert_screen(&screen, "● 空闲 · deepseek-chat");

    let failed = StatusModel {
        phase: Some(TurnPhase::Failed),
        waiting: None,
        model: Some("deepseek-chat".to_owned()),
        tokens: None,
    };
    let screen = render_lines(60, 1, vec![failed.render(&ctx).content]);
    assert_screen(&screen, "✗ 失败 · deepseek-chat");
}

#[test]
fn shortcut_bar_snapshot_lists_registry_hints() {
    let theme = capabilities().theme();
    let ctx = ctx_for(&theme, 72);
    let keymap = jh_tui::actions::Keymap::from_defaults();
    let bar = ShortcutBar::render(&ctx, &keymap, KeyOwner::Prompt);
    let screen = render_lines(72, 1, vec![bar.content]);
    assert_screen(
        &screen,
        "Ctrl+c 退出 · Esc 取消 · Enter 发送 · Alt+Enter 换行 · PgUp 上翻 …",
    );
}

#[test]
fn shortcut_bar_snapshot_truncates_on_a_narrow_screen() {
    let theme = capabilities().theme();
    let ctx = ctx_for(&theme, 24);
    let keymap = jh_tui::actions::Keymap::from_defaults();
    let bar = ShortcutBar::render(&ctx, &keymap, KeyOwner::Prompt);
    let screen = render_lines(24, 1, vec![bar.content]);
    // 整条放不下的提示直接丢掉，不留半截。
    assert_screen(&screen, "Ctrl+c 退出 · Esc 取消 …");
}

#[test]
fn approval_card_snapshot_renders_rail_body_and_options() {
    let theme = capabilities().theme();
    let keymap = jh_tui::actions::Keymap::from_defaults();
    let request = ApprovalRequest::new("bash", "git push --force origin main")
        .with_body("该命令会改写远端历史，且无法从本地恢复。");
    let ctx = ctx_for(&theme, 52);
    let lines = ApprovalCard::render(&request, &keymap, &ctx, true, 8);
    let screen = render_lines(
        52,
        lines.len() as u16,
        lines.into_iter().map(|line| line.content).collect(),
    );
    assert_screen(
        &screen,
        "┃  权限请求 · bash
┃  命令  git push --force origin main
┃  该命令会改写远端历史，且无法从本地恢复。
┃  y 允许一次   a 总是允许   n 拒绝",
    );
}

#[test]
fn approval_card_snapshot_recedes_when_unfocused() {
    let theme = capabilities().theme();
    let keymap = jh_tui::actions::Keymap::from_defaults();
    let request = ApprovalRequest::new("bash", "ls -la");
    let ctx = ctx_for(&theme, 40);
    let lines = ApprovalCard::render(&request, &keymap, &ctx, false, 8);
    let (screen, buffer) = render_lines_cells(
        40,
        lines.len() as u16,
        lines.into_iter().map(|line| line.content).collect(),
    );
    assert_screen(
        &screen,
        "┃  权限请求 · bash
┃  命令  ls -la
┃  y 允许一次   a 总是允许   n 拒绝",
    );
    // 失焦的判定打在缓冲区单元格上：强调轨不是警示色，而是弱化色。
    assert_eq!(buffer[(0, 0)].fg, theme.gray_dim);

    let focused = ApprovalCard::render(&request, &keymap, &ctx, true, 8);
    let (_, focused_buffer) = render_lines_cells(
        40,
        focused.len() as u16,
        focused.into_iter().map(|line| line.content).collect(),
    );
    assert_eq!(focused_buffer[(0, 0)].fg, theme.accent_warning);
}

#[test]
fn full_screen_snapshot_has_every_component() {
    let mut app = App::new(&capabilities());
    seed_app(&mut app, false);
    let screen = render_app(76, 16, &mut app);
    assert_screen(
        &screen,
        "❯ 帮我看一下未提交的改动

◆好的，我先跑 git status，再用 Jev 判断是否需要你确认。

▸ bash git status --short

◇ shell_command_allowed
  是否允许执行 `git push --force`？
  ▸ 0.71 █████▋   允许一次 (usr:allow)
    0.22 █▊       拒绝 (usr:deny)
  采纳 允许一次 · usr:allow 0.71 · margin 0.49 · confidence 0.64 · 倾向


⠋ Jev 判定中 · 判定 usr:allow 0.71 · margin 0.49 · confidence 0.64 · 倾向 ·…
❯ 顺便看一下 CI 配置
Ctrl+c 退出 · Esc 取消 · Enter 发送 · Alt+Enter 换行 · PgUp 上翻 · PgDn 下翻",
    );
}

#[test]
fn full_screen_snapshot_with_approval_card() {
    let mut app = App::new(&capabilities());
    seed_app(&mut app, true);
    let screen = render_app(76, 16, &mut app);
    // 审批卡压在回滚区底部，且不透明（底下的判定行不会透上来）；键盘归卡片，
    // 所以状态行显示「等待授权」、快捷键栏换成权限键。
    assert_screen(
        &screen,
        "❯ 帮我看一下未提交的改动

◆好的，我先跑 git status，再用 Jev 判断是否需要你确认。

▸ bash git status --short

◇ shell_command_allowed
  是否允许执行 `git push --force`？
  ▸ 0.71 █████▋   允许一次 (usr:allow)
┃  权限请求 · bash
┃  命令  git push --force origin main
┃  该命令会改写远端历史，且无法从本地恢复。
┃  y 允许一次   a 总是允许   n 拒绝
⠋ 等待授权 · 判定 usr:allow 0.71 · margin 0.49 · confidence 0.64 · 倾向 · d…
❯ 顺便看一下 CI 配置
y 允许一次 · Ctrl+c 退出 · Esc 取消 · a 总是允许 · n 拒绝",
    );
}

#[test]
fn full_screen_snapshot_degrades_without_color_and_with_fallback_glyphs() {
    let mut app = App::new(&capabilities_plain());
    seed_app(&mut app, true);
    let (screen, buffer) = common::render_app_cells(76, 16, &mut app);

    // 「无色」半边也断言在缓冲区上：整屏零 RGB / 索引色，前景退回终端默认。
    assert_eq!(
        buffer[(0, 0)].fg,
        ratatui::style::Color::Reset,
        "无色档下提示标记不该带色"
    );
    for cell in buffer.content() {
        assert!(
            !matches!(
                cell.fg,
                ratatui::style::Color::Rgb(..) | ratatui::style::Color::Indexed(_)
            ),
            "无色档仍有前景色：{:?}",
            cell.fg
        );
        assert!(
            !matches!(
                cell.bg,
                ratatui::style::Color::Rgb(..) | ratatui::style::Color::Indexed(_)
            ),
            "无色档仍有底色：{:?}",
            cell.bg
        );
    }
    // NO_COLOR + legacy 控制台：字形全换成同宽 ASCII 回退形态，正文一个字符都不少。
    assert_screen(
        &screen,
        "> 帮我看一下未提交的改动

♦好的，我先跑 git status，再用 Jev 判断是否需要你确认。

> bash git status --short

○ shell_command_allowed
  是否允许执行 `git push --force`？
  > 0.71 █████▒   允许一次 (usr:allow)
│  权限请求 | bash
│  命令  git push --force origin main
│  该命令会改写远端历史，且无法从本地恢复。
│  y 允许一次   a 总是允许   n 拒绝
| 等待授权 | 判定 usr:allow 0.71 | margin 0.49 | confidence 0.64 | 倾向 | d.
> 顺便看一下 CI 配置
y 允许一次 | Ctrl+c 退出 | Esc 取消 | a 总是允许 | n 拒绝",
    );
}

/// 一屏里放齐四类块、状态行、提示框与快捷键栏。
fn seed_app(app: &mut App, with_approval: bool) {
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
        .with_model("deepseek-chat")
        .with_tokens(12_480, 64_000);
    app.prompt_mut().insert_str("顺便看一下 CI 配置");
    if with_approval {
        app.open_approval(
            ApprovalRequest::new("bash", "git push --force origin main")
                .with_body("该命令会改写远端历史，且无法从本地恢复。"),
        );
        assert!(matches!(app.modal(), Some(ActiveModal::Approval(_))));
    }
}
