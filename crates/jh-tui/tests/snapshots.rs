//! 组件级与整屏 `TestBackend` 快照：全部离线，不碰真实终端。
//!
//! 组件级（回滚区 2、提示框 4、状态行 2、快捷键栏 2、审批卡 2、面板 2）与整屏
//! （首屏 / 主界面 / 主界面+审批卡 / 研究视图三个面板 / 窄屏 / 无色档）各一组；
//! 另有几条直接断言单元格样式（失焦降饱和、无色档、用户块填充条）。
//!
//! 期望值一律写成多行原始字符串；行尾空白与行尾空行不参与比较（那是画布高度留下的），
//! 行首空行参与比较——提示框光标那条就靠它锁住上方回滚区的高度。

mod common;

use common::{
    assert_screen, capabilities, capabilities_plain, render_app, render_app_cells,
    render_app_with_cursor, render_lines, render_lines_cells,
};
use jh_tui::actions::KeyOwner;
use jh_tui::app::App;
use jh_tui::blocks::{Block, BlockCtx, Choice, DecisionTrace, ToolStatus};
use jh_tui::components::{
    ActiveModal, ApprovalCard, ApprovalRequest, HeaderBar, HeaderModel, Panel, PanelLine,
    PanelState, PanelTitle, Prompt, PromptChrome, Scrollback, ShortcutBar, StatusModel, Tone,
    TurnPhase, WaitingReason,
};
use jh_tui::panels::{
    ActionRow, Distribution, EdgeRow, EvidenceRow, EvidenceView, JudgmentView, PanelId, PathRow,
    PropagationView, Validity,
};
use jh_tui::screens::Screen;
use jh_tui::startup::{self, StartupFacts};
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

fn decision_trace() -> DecisionTrace {
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

// ---------------------------------------------------------------- 组件级

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

◆ 我先跑一下 git status，看有没有未提交
  改动。

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
fn assistant_block_snapshot_renders_the_markdown_subset() {
    let theme = capabilities().theme();
    let ctx = ctx_for(&theme, 40);
    // 一张图看全子集：标题 / 列表（行内代码与链接）/ 引用 / 代码块 / 分隔线。
    let block = Block::assistant(
        "# 结论\n\n- 先看 `git status`\n- 再看 [文档](https://x.dev)\n\n> 引用一行\n\n```rust\nlet a = **1**;\n```\n\n---",
    );
    let output = block.output(&ctx);
    let screen = render_lines(
        40,
        14,
        output.lines.into_iter().map(|line| line.content).collect(),
    );
    assert_screen(
        &screen,
        "◆ 结论

  • 先看 git status
  • 再看 文档 (https://x.dev)

  ▏ 引用一行

    rust
    let a = **1**;

  ──────────────────────────────────────",
    );
}

#[test]
fn streaming_assistant_snapshot_marks_the_growing_text() {
    let mut app = App::new(&capabilities());
    app.set_viewport(44, 8, 40);
    app.push_block(Block::user("看一下仓库状态"));
    // 同一次流式输出：两次增量落在同一个块里。
    app.stream_assistant("我先跑一下");
    app.stream_assistant(" git status，看有没有未提交改动。");
    assert_eq!(app.scrollback().len(), 2, "两个增量共用一个块");

    let ctx = app.block_ctx(44);
    let window = app.scrollback().window(&ctx, 8);
    let screen = render_lines(
        44,
        8,
        window.lines.into_iter().map(|line| line.content).collect(),
    );
    // 末尾的 `▸` 就是「还在长」的标记；正文按内容宽度换行、续行对齐正文列。
    assert_screen(
        &screen,
        "❯ 看一下仓库状态

◆ 我先跑一下 git status，看有没有未提交
  改动。▸",
    );

    // 收尾：末尾的「还在长」标记消失，正文一个字符都不少。
    app.finish_assistant();
    let ctx = app.block_ctx(44);
    let window = app.scrollback().window(&ctx, 8);
    let screen = render_lines(
        44,
        8,
        window.lines.into_iter().map(|line| line.content).collect(),
    );
    assert_screen(
        &screen,
        "❯ 看一下仓库状态

◆ 我先跑一下 git status，看有没有未提交
  改动。",
    );
}

#[test]
fn tool_block_snapshot_takes_the_result_detail() {
    let mut app = App::new(&capabilities());
    app.begin_tool("bash", "git status --short");
    let ctx = app.block_ctx(44);
    let window = app.scrollback().window(&ctx, 6);
    let running = render_lines(
        44,
        6,
        window.lines.into_iter().map(|line| line.content).collect(),
    );
    // 执行中：只有工具行与转轮，没有结果行。
    assert_screen(&running, "⠋ bash git status --short");

    app.finish_tool(ToolStatus::Ok, Some("干净：没有未提交改动".to_owned()));
    let ctx = app.block_ctx(44);
    let window = app.scrollback().window(&ctx, 6);
    let done = render_lines(
        44,
        6,
        window.lines.into_iter().map(|line| line.content).collect(),
    );
    // 结果回来：状态从转轮换成标记，摘要缩进在工具行的正文列下。
    assert_screen(
        &done,
        "▸ bash git status --short
  干净：没有未提交改动",
    );
}

#[test]
fn user_block_snapshot_fills_the_row_and_right_aligns_the_timestamp() {
    let theme = capabilities().theme();
    let ctx = ctx_for(&theme, 40);
    let block = Block::user_at("看一下仓库状态", "14:22");
    let (screen, buffer) = render_lines_cells(
        40,
        1,
        block
            .output(&ctx)
            .lines
            .into_iter()
            .map(|line| line.content)
            .collect(),
    );
    assert_screen(&screen, "❯ 看一下仓库状态                   14:22");
    // 填充条铺满整行：首格与末格都带底色（行尾空白被断言文本裁掉，样式仍在单元格上）。
    assert_eq!(buffer[(0, 0)].bg, theme.user_block_bg);
    assert_eq!(buffer[(39, 0)].bg, theme.user_block_bg);
}

#[test]
fn prompt_snapshot_renders_multiline_input_flat() {
    let theme = capabilities().theme();
    let ctx = ctx_for(&theme, 40);
    let prompt = Prompt::with_text("帮我查一下\ncrates/jh-tui 的依赖树");
    let screen = render_lines(
        40,
        2,
        prompt
            .render(&ctx, &PromptChrome::default(), true, "Shift+Enter/Ctrl+J")
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
fn prompt_snapshot_soft_wraps_long_input_flat() {
    let theme = capabilities().theme();
    let ctx = ctx_for(&theme, 24);
    let prompt =
        Prompt::with_text("这是一条很长的输入，用来验证软折行之后每一行都不会超过内容宽度。");
    let lines = prompt.render(&ctx, &PromptChrome::default(), true, "Shift+Enter/Ctrl+J");
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
fn prompt_snapshot_draws_the_box_with_title_and_info() {
    let theme = capabilities().theme();
    let ctx = ctx_for(&theme, 40);
    let prompt = Prompt::with_text("看一下 CI 配置");
    let chrome = PromptChrome::default()
        .with_title("会话 1")
        .with_info("deepseek-chat · jev-1.13.0 · always-approve");
    let lines = prompt.render(&ctx, &chrome, true, "Shift+Enter/Ctrl+J");
    let screen = render_lines(
        40,
        lines.len() as u16,
        lines.into_iter().map(|line| line.content).collect(),
    );
    assert_screen(
        &screen,
        "╭─ 会话 1 ─────────────────────────────╮
│ ❯ 看一下 CI 配置                     │
╰─ deepseek-chat · jev-1.13.0 · alwa… ─╯",
    );
}

#[test]
fn prompt_snapshot_degrades_without_color_and_glyphs() {
    let theme = capabilities_plain().theme();
    let ctx = BlockCtx {
        width: 30,
        theme: &theme,
        glyphs: capabilities_plain().glyphs,
        frame: 0,
        decision: None,
    };
    let prompt = Prompt::with_text("hi");
    let chrome = PromptChrome::default().with_info("未接线");
    let lines = prompt.render(&ctx, &chrome, false, "Shift+Enter/Ctrl+J");
    let screen = render_lines(
        30,
        lines.len() as u16,
        lines.into_iter().map(|line| line.content).collect(),
    );
    assert_screen(
        &screen,
        "+----------------------------+
| > hi                       |
+- 未接线 -------------------+",
    );
}

#[test]
fn prompt_cursor_sits_at_the_edit_position() {
    let mut app = App::new(&capabilities());
    app.set_screen(Screen::Agent);
    app.prompt_mut().insert_str("ab\n中");
    let (screen, cursor) = render_app_with_cursor(30, 8, &mut app);
    // 布局 = 顶栏 1 / 回滚区 1 / 状态行 1 / 提示框 3（含边框）/ 快捷键栏 1，8 行只够画到这里。
    assert_screen(
        &screen,
        "

● 空闲
╭────────────────────────────╮
│ ❯ ab                       │
│   中                       │
╰─ 未接线 · 不调用模型与 A… ─╯
Ctrl+c 退出 · Esc 取消 …",
    );
    // 光标在盒内第 2 行（顶边占 1 行）：`│ ❯ ` 占 4 列，`中` 之前的 `ab` 是 2 列 → 第 6 列。
    assert_eq!(cursor, (6, 5), "终端光标应落在编辑位置");
}

#[test]
fn header_snapshot_puts_tokens_and_badge_on_the_right() {
    let theme = capabilities().theme();
    let ctx = ctx_for(&theme, 60);
    let model = HeaderModel::new("~/Documents/jev-harness")
        .with_tokens(12_480, 64_000)
        .with_badge("[研究视图]");
    let screen = render_lines(60, 1, vec![HeaderBar::render(&model, &ctx).content]);
    assert_screen(
        &screen,
        "~/Documents/jev-harness               12.5k/64k · [研究视图]",
    );
}

#[test]
fn status_line_snapshot_shows_waiting_and_decision_probability() {
    let theme = capabilities().theme();
    let trace = decision_trace();
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
    let bar = ShortcutBar::render(&ctx, &keymap, KeyOwner::Prompt).line;
    let screen = render_lines(72, 1, vec![bar.content]);
    assert_screen(
        &screen,
        "Ctrl+c 退出 · Esc 取消 · Enter 发送 · Shift+Enter/Ctrl+J 换行 …",
    );
}

#[test]
fn shortcut_bar_snapshot_truncates_on_a_narrow_screen() {
    let theme = capabilities().theme();
    let ctx = ctx_for(&theme, 24);
    let keymap = jh_tui::actions::Keymap::from_defaults();
    let bar = ShortcutBar::render(&ctx, &keymap, KeyOwner::Prompt).line;
    let screen = render_lines(24, 1, vec![bar.content]);
    // 整条放不下的提示直接丢掉，不留半截。
    assert_screen(&screen, "Ctrl+c 退出 · Esc 取消 …");
}

#[test]
fn shortcut_bar_snapshot_for_the_research_screen() {
    let theme = capabilities().theme();
    let ctx = ctx_for(&theme, 84);
    let keymap = jh_tui::actions::Keymap::from_defaults();
    let bar = ShortcutBar::render(&ctx, &keymap, KeyOwner::Research).line;
    let screen = render_lines(84, 1, vec![bar.content]);
    assert_screen(
        &screen,
        "Ctrl+c 退出 · Esc 取消 · Tab 下一个面板 · Shift+Tab 上一个面板 · s 单步 …",
    );
}

#[test]
fn approval_card_snapshot_renders_rail_body_and_options() {
    let theme = capabilities().theme();
    let keymap = jh_tui::actions::Keymap::from_defaults();
    let request = ApprovalRequest::new("bash", "git push --force origin main")
        .with_body("该命令会改写远端历史，且无法从本地恢复。");
    let ctx = ctx_for(&theme, 52);
    let lines = ApprovalCard::render(&request, &keymap, &ctx, true, 8).lines;
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
    let lines = ApprovalCard::render(&request, &keymap, &ctx, false, 8).lines;
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

    let focused = ApprovalCard::render(&request, &keymap, &ctx, true, 8).lines;
    let (_, focused_buffer) = render_lines_cells(
        40,
        focused.len() as u16,
        focused.into_iter().map(|line| line.content).collect(),
    );
    assert_eq!(focused_buffer[(0, 0)].fg, theme.accent_warning);
}

#[test]
fn panel_snapshot_draws_tabs_content_and_footer() {
    let theme = capabilities().theme();
    let ctx = ctx_for(&theme, 48);
    let panel = Panel::with_lines(
        PanelTitle::Tabs {
            labels: vec!["判断详情".to_owned(), "事件与证据".to_owned()],
            active: 0,
        },
        vec![
            PanelLine::Field {
                label: "问题".to_owned(),
                text: "shell_command_allowed v3".to_owned(),
                tone: Tone::Normal,
            },
            PanelLine::Field {
                label: "有效性".to_owned(),
                text: "待重查".to_owned(),
                tone: Tone::Warning,
            },
        ],
    )
    .with_footer("3 条待重查");
    let lines = panel.render(&ctx, 6, &PanelState::default(), true);
    let screen = render_lines(
        48,
        lines.len() as u16,
        lines.into_iter().map(|line| line.content).collect(),
    );
    assert_screen(
        &screen,
        "╭─ 判断详情 ─┬─ 事件与证据 ────────────────────╮
│ 问题   shell_command_allowed v3              │
│ 有效性 待重查                                │
│                                              │
│                                              │
╰─ 3 条待重查 ─────────────────────────────────╯",
    );
}

// ---------------------------------------------------------------- 整屏

#[test]
fn startup_screen_snapshot_has_the_centered_panel() {
    let mut app = App::new(&capabilities());
    seed_header(&mut app);
    let screen = render_app(76, 24, &mut app);
    assert_screen(
        &screen,
        "


      ╭─ jev-harness 0.1.0 ──────────────────────────────────────────╮
      │ ╭─╮  jev-harness 0.1.0                                       │
      │ │◇│  判定器  rule · 未接线                                   │
      │ ╰─╯  工作区  ~/Documents/jev-harness                         │
      │                                                              │
      │ 新建会话                                              Ctrl+N │
      │ 研究视图                                              Ctrl+O │
      │ 退出                                                  Ctrl+C │
      ╰──────────────────────────────────────────────────────────────╯








╭──────────────────────────────────────────────────────────────────────────╮
│ ❯ Enter 发送，Shift+Enter/Ctrl+J 换行 · Ctrl+Shift+V 粘贴 · /help 看命令 │
╰─ 未接线 · 不调用模型与 API ──────────────────────────────────────────────╯
                                                               rule · 未接线",
    );
}

#[test]
fn agent_screen_snapshot_has_every_component() {
    let mut app = App::new(&capabilities());
    seed_agent(&mut app, false);
    let screen = render_app(76, 20, &mut app);
    assert_screen(
        &screen,
        "~/Documents/jev-harness                               12.5k/64k · [研究视图]
❯ 帮我看一下未提交的改动                                               14:18

◆ 我先跑 git status，再用 Jev 判断是否需要你确认。

◆ 判定 12ms · 采纳 usr:allow 0.71 · margin 0.49 · 倾向

▸ bash git status --short

◇ shell_command_allowed
  是否允许执行 `git push --force`？
  ▸ 0.71 █████▋   允许一次 (usr:allow)
    0.22 █▊       拒绝 (usr:deny)
    0.07 ▌        总是允许 (usr:always)
  采纳 允许一次 · usr:allow 0.71 · margin 0.49 · confidence 0.64 · 倾向
⠋ Jev 判定中 · 判定 usr:allow 0.71 · margin 0.49 · confidence 0.64 · 倾向 ·…
╭──────────────────────────────────────────────────────────────────────────╮
│ ❯ 顺便看一下 CI 配置                                                     │
╰─ deepseek-chat · jev-1.13.0 · always-approve ────────────────────────────╯
Ctrl+c 退出 · Esc 取消 · Enter 发送 · Shift+Enter/Ctrl+J 换行 · PgUp 上翻 …",
    );
}

#[test]
fn agent_screen_snapshot_with_approval_card() {
    let mut app = App::new(&capabilities());
    seed_agent(&mut app, true);
    let screen = render_app(76, 20, &mut app);
    // 审批卡压在回滚区底部，且不透明（底下的判定行不会透上来）；键盘归卡片，
    // 所以状态行显示「等待授权」、快捷键栏换成权限键。
    assert_screen(
        &screen,
        "~/Documents/jev-harness                               12.5k/64k · [研究视图]
❯ 帮我看一下未提交的改动                                               14:18

◆ 我先跑 git status，再用 Jev 判断是否需要你确认。

◆ 判定 12ms · 采纳 usr:allow 0.71 · margin 0.49 · 倾向

▸ bash git status --short

◇ shell_command_allowed
  是否允许执行 `git push --force`？
┃  权限请求 · bash
┃  命令  git push --force origin main
┃  该命令会改写远端历史，且无法从本地恢复。
┃  y 允许一次   a 总是允许   n 拒绝
⠋ 等待授权 · 判定 usr:allow 0.71 · margin 0.49 · confidence 0.64 · 倾向 · d…
╭──────────────────────────────────────────────────────────────────────────╮
│ ❯ 顺便看一下 CI 配置                                                     │
╰─ deepseek-chat · jev-1.13.0 · always-approve ────────────────────────────╯
y 允许一次 · Ctrl+c 退出 · Esc 取消 · a 总是允许 · n 拒绝 · Ctrl+o 切换视图",
    );
}

#[test]
fn agent_screen_snapshot_degrades_without_color_and_fallback_glyphs() {
    let mut app = App::new(&capabilities_plain());
    seed_agent(&mut app, true);
    let (screen, buffer) = render_app_cells(76, 20, &mut app);

    // 「无色」半边也断言在缓冲区上：整屏零 RGB / 索引色，前景退回终端默认。
    assert_eq!(
        buffer[(0, 0)].fg,
        ratatui::style::Color::Reset,
        "无色档下顶栏不该带色"
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
        "~/Documents/jev-harness                               12.5k/64k | [研究视图]
> 帮我看一下未提交的改动                                               14:18

♦ 我先跑 git status，再用 Jev 判断是否需要你确认。

♦ 判定 12ms · 采纳 usr:allow 0.71 · margin 0.49 · 倾向

> bash git status --short

○ shell_command_allowed
  是否允许执行 `git push --force`？
│  权限请求 | bash
│  命令  git push --force origin main
│  该命令会改写远端历史，且无法从本地恢复。
│  y 允许一次   a 总是允许   n 拒绝
| 等待授权 | 判定 usr:allow 0.71 | margin 0.49 | confidence 0.64 | 倾向 | d.
+--------------------------------------------------------------------------+
| > 顺便看一下 CI 配置                                                     |
+- deepseek-chat · jev-1.13.0 · always-approve ----------------------------+
y 允许一次 | Ctrl+c 退出 | Esc 取消 | a 总是允许 | n 拒绝 | Ctrl+o 切换视图",
    );
}

#[test]
fn research_screen_snapshot_shows_the_judgment_panel() {
    let mut app = App::new(&capabilities());
    seed_research(&mut app, PanelId::Judgment);
    let screen = render_app(84, 20, &mut app);
    assert_screen(
        &screen,
        "~/Documents/jev-harness                                           12.5k/64k · [会话]
╭─ 判断详情 ─┬─ 事件与证据 ─┬─ 失效传播 ───────────────────────────────────────────╮
│ 问题   shell_command_allowed                                                     │
│ 版本   v3（当前）                                                                │
│                                                                                  │
│ 分布                                                                             │
│ ▸ 0.71 █████▋    允许一次 (usr:allow)                                            │
│   0.22 █▊        拒绝 (usr:deny)                                                 │
│   0.07 ▌         总是允许 (usr:always)                                           │
│ confidence 0.64                                                                  │
│                                                                                  │
│ 来源                                                                             │
│ 判断器 RealApi · jev-1.13.0                                                      │
│ 请求   3f9c1a                                                                    │
│ 快照   7b1e                                                                      │
│ 延迟   12 ms                                                                     │
│ 用量   in 356 / out 38                                                           │
╰─ shell_command_allowed v3 ─────────────────────────────────────────────────────▼ ╯
● 研究视图 · 判断详情 · 有效性 待重查
Ctrl+c 退出 · Esc 取消 · Tab 下一个面板 · Shift+Tab 上一个面板 · s 单步 …",
    );
}

#[test]
fn research_screen_snapshot_shows_the_evidence_panel() {
    let mut app = App::new(&capabilities());
    seed_research(&mut app, PanelId::Evidence);
    let screen = render_app(84, 14, &mut app);
    assert_screen(
        &screen,
        "╭─ 判断详情 ─┬─ 事件与证据 ─┬─ 失效传播 ───────────────────────────────────────────╮
│ #1 14:19:40 · config v3                                                          │
│    9d21e4 · config · 监听端口被改回 9090                                         │
│ #2 14:22:01 · app.log v3                                                         │
│    3f9c1a · app.log · 端口占用的报错消失（样本只到 14:22）                       │
│ #3 撤回 · 14:23:15 · runtime.metrics v1                                          │
│    c0ffee · runtime.metrics · 采样窗口过短，指标不可用                           │
│                                                                                  │
│                                                                                  │
│                                                                                  │
│                                                                                  │
╰─ 3 条证据 · 1 条撤回 ────────────────────────────────────────────────────────────╯
● 研究视图 · 事件与证据 · 3 条证据 · 1 条撤回
Ctrl+c 退出 · Esc 取消 · Tab 下一个面板 · Shift+Tab 上一个面板 · s 单步 …",
    );
}

#[test]
fn research_screen_snapshot_shows_the_propagation_panel() {
    let mut app = App::new(&capabilities());
    seed_research(&mut app, PanelId::Propagation);
    let screen = render_app(84, 18, &mut app);
    assert_screen(
        &screen,
        "~/Documents/jev-harness                                           12.5k/64k · [会话]
╭─ 判断详情 ─┬─ 事件与证据 ─┬─ 失效传播 ───────────────────────────────────────────╮
│ 起因         追加证据 #3（app.log v3）                                           │
│                                                                                  │
│ 待重查判断                                                                       │
│ ▸ cause_primary v2                                                               │
│                                                                                  │
│ 待复核动作                                                                       │
│ ▸ apply_fix                                                                      │
│                                                                                  │
│ 传播路径                                                                         │
│ cause_primary 读取 → 支持                                                        │
│                                                                                  │
│ 依赖边                                                                           │
│ app.log ─[read · 确定性]→ cause_primary                                          │
╰─ 1 条待重查 · 1 个动作待复核 ──────────────────────────────────────────────────▼ ╯
● 研究视图 · 失效传播 · 1 条待重查 · 1 个动作待复核
Ctrl+c 退出 · Esc 取消 · Tab 下一个面板 · Shift+Tab 上一个面板 · s 单步 …",
    );
}

#[test]
fn research_screen_degrades_on_a_short_terminal() {
    let mut app = App::new(&capabilities());
    seed_research(&mut app, PanelId::Judgment);
    let screen = render_app(60, 12, &mut app);
    // 12 行 < SHORT_TERMINAL_ROWS：顶栏收起，面板只剩 8 行内容。
    assert!(!screen.row(0).contains("jev-harness"), "{}", screen.text());
    assert!(
        screen.row(0).starts_with("╭─ 判断详情"),
        "{}",
        screen.row(0)
    );
    assert!(screen.text().contains("有效性 待重查"), "{}", screen.text());
}

// ---------------------------------------------------------------- 种子数据

fn seed_header(app: &mut App) {
    app.header_model_mut().title = "~/Documents/jev-harness".to_owned();
    app.header_model_mut().tokens = Some((12_480, 64_000));
    app.set_startup_note("rule · 未接线");
}

/// 一屏里放齐五类块、顶栏、状态行、四边盒提示框与快捷键栏。
fn seed_agent(app: &mut App, with_approval: bool) {
    seed_header(app);
    app.set_screen(Screen::Agent);
    app.set_prompt_chrome(
        PromptChrome::default().with_info("deepseek-chat · jev-1.13.0 · always-approve"),
    );
    app.push_block(Block::user_at("帮我看一下未提交的改动", "14:18"));
    app.push_block(Block::assistant(
        "我先跑 git status，再用 Jev 判断是否需要你确认。",
    ));
    app.push_block(Block::notice(
        "判定 12ms · 采纳 usr:allow 0.71 · margin 0.49 · 倾向",
    ));
    app.push_block(Block::tool("bash", "git status --short", ToolStatus::Ok));
    app.push_block(Block::decision(decision_trace()));
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

fn seed_research(app: &mut App, panel: PanelId) {
    seed_header(app);
    app.set_screen(Screen::Research);
    app.set_prompt_chrome(
        PromptChrome::default().with_info("deepseek-chat · jev-1.13.0 · always-approve"),
    );
    while app.panel() != panel {
        let _ = app.apply(jh_tui::actions::ActionId::NextPanel);
    }
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
        executor: Some("jev-1.13.0".to_owned()),
        request_id: Some("3f9c1a".to_owned()),
        snapshot: Some("7b1e".to_owned()),
        validity: Validity::NeedsRecheck,
        projection: vec!["app.log: 全部字段".to_owned()],
        latency_ms: Some(12),
        usage: Some((356, 38)),
    });
    panels.evidence = EvidenceView {
        rows: vec![
            evidence_row(
                1,
                "14:19:40",
                "config",
                3,
                "9d21e4",
                "监听端口被改回 9090",
                false,
            ),
            evidence_row(
                2,
                "14:22:01",
                "app.log",
                3,
                "3f9c1a",
                "端口占用的报错消失（样本只到 14:22）",
                false,
            ),
            evidence_row(
                3,
                "14:23:15",
                "runtime.metrics",
                1,
                "c0ffee",
                "采样窗口过短，指标不可用",
                true,
            ),
        ],
    };
    panels.propagation = PropagationView {
        cause: Some("追加证据 #3（app.log v3）".to_owned()),
        stale: vec!["cause_primary v2".to_owned()],
        blocked: vec!["apply_fix".to_owned()],
        paths: vec![PathRow {
            target: "cause_primary".to_owned(),
            hops: "读取 → 支持".to_owned(),
        }],
        edges: vec![EdgeRow {
            from: "app.log".to_owned(),
            kind: "read".to_owned(),
            to: "cause_primary".to_owned(),
            origin: "确定性".to_owned(),
        }],
        actions: vec![ActionRow {
            action: "apply_fix".to_owned(),
            check: "被挡下：前提 cause_primary".to_owned(),
            tone: Tone::Warning,
        }],
    };
}

fn evidence_row(
    id: u64,
    at: &str,
    source: &str,
    version: u64,
    hash: &str,
    summary: &str,
    retracted: bool,
) -> EvidenceRow {
    EvidenceRow {
        id,
        at: at.to_owned(),
        source: source.to_owned(),
        object: source.to_owned(),
        version,
        hash: hash.to_owned(),
        summary: summary.to_owned(),
        retracted,
    }
}

#[test]
fn startup_panel_facts_come_from_the_app_header() {
    let theme = capabilities().theme();
    let ctx = ctx_for(&theme, 52);
    let facts = StartupFacts {
        version: "0.1.0",
        judge: "rule · 未接线",
        workspace: "~/Documents/jev-harness",
    };
    let panel = startup::panel(&facts, capabilities().glyphs);
    let lines = panel.render(&ctx, 10, &PanelState::default(), true);
    let text: Vec<String> = lines.iter().map(|line| line.text()).collect();
    assert!(text[0].starts_with("╭─ jev-harness 0.1.0"), "{}", text[0]);
    assert!(
        text.iter().any(|line| line.contains("新建会话")),
        "{text:?}"
    );
}
