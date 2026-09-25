//! `/` 命令补全列表：画在提示框上方、选中行高亮、点击 = 填入。
//!
//! 坐标一律**先渲染再从画出来的屏上定位**（与 `mouse.rs` 同一个口径），这样"画在哪"与
//! "点得到哪"是同一份布局，测试钉得住两者的漂移。

mod common;

use common::{
    assert_contains, assert_screen, capabilities, capabilities_plain, render_app, render_app_cells,
};
use jh_tui::actions::KeyChord;
use jh_tui::app::App;
use jh_tui::screens::Screen;
use jh_tui::theme::Theme;
use jh_tui::width::display_width;
use ratatui::crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::style::Modifier;

fn type_text(app: &mut App, text: &str) {
    for c in text.chars() {
        app.handle_key(KeyChord::plain(KeyCode::Char(c)));
    }
}

fn press(app: &mut App, code: KeyCode) {
    app.handle_key(KeyChord::plain(code));
}

/// 屏上某一行里 `needle` 的起始显示列（宽字符按显示宽度算）。
fn column_of(line: &str, needle: &str) -> u16 {
    let index = line.find(needle).expect("这一段在行内");
    display_width(&line[..index]) as u16
}

/// 在屏上找一个片段，返回它的 (行, 起始显示列)。
fn locate(screen: &common::Screen, needle: &str) -> (u16, u16) {
    for (row, line) in screen.lines.iter().enumerate() {
        if line.contains(needle) {
            return (row as u16, column_of(line, needle));
        }
    }
    panic!("屏上没有 `{needle}`：\n{}", screen.text());
}

fn click(column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

/// 列表整段画在提示框**上方**，选中行用强调色 + 粗体高亮，`Down` 之后高亮跟着走。
#[test]
fn the_list_is_drawn_above_the_prompt_and_highlights_the_selection() {
    let theme = Theme::DARK;
    let mut app = App::new(&capabilities());
    app.set_screen(Screen::Agent);
    type_text(&mut app, "/");
    let (screen, buffer) = render_app_cells(100, 24, &mut app);

    // 提示框盒子的顶边（`╭`）：候选列表的末行正好压在它上面。
    let prompt_top = screen
        .lines
        .iter()
        .position(|line| line.contains('╭'))
        .expect("会话屏有提示框盒子");
    let selected_row = screen
        .lines
        .iter()
        .position(|line| line.contains("▸ /help"))
        .expect("第一项应当选中");
    let footer_row = screen
        .lines
        .iter()
        .position(|line| line.contains("还有 6 条"))
        .expect("候选多于窗口时末行是溢出提示");
    // 列表占的是回滚区底部：紧贴状态行，状态行再往下才是提示框。
    let status_row = screen
        .lines
        .iter()
        .position(|line| line.contains("空闲"))
        .expect("状态行在屏上");
    assert_eq!(
        footer_row + 1,
        status_row,
        "列表压在回滚区底部：\n{}",
        screen.text()
    );
    assert!(
        status_row < prompt_top,
        "状态行与提示框都在列表下方：\n{}",
        screen.text()
    );
    assert_eq!(selected_row + 5, footer_row, "窗口从第一条开始");

    // 空查询列出全部命令的头五条（注册表顺序）+ 溢出提示。
    for name in ["/help", "/quit", "/clear", "/research", "/model"] {
        assert_contains(&screen, name);
    }
    assert!(!screen.text().contains("/status"), "窗口外的候选不画");
    assert_contains(&screen, "列出全部命令");

    // 选中行高亮：行首标记与命令名都是强调色 + 粗体。
    let row = selected_row as u16;
    let marker = buffer[(0, row)].style();
    assert_eq!(marker.fg, Some(theme.accent_system), "行首标记用强调色");
    assert!(marker.add_modifier.contains(Modifier::BOLD));
    let name_column = column_of(screen.row(selected_row), "/help");
    let name = buffer[(name_column, row)].style();
    assert_eq!(name.fg, Some(theme.accent_system), "选中行的命令名用强调色");
    assert!(name.add_modifier.contains(Modifier::BOLD));

    // 未选中行不抢高亮。
    let quiet_row = screen
        .lines
        .iter()
        .position(|line| line.contains("/quit"))
        .expect("第二项在屏上");
    let quiet = buffer[(column_of(screen.row(quiet_row), "/quit"), quiet_row as u16)].style();
    assert_ne!(quiet.fg, Some(theme.accent_system));
    assert!(!quiet.add_modifier.contains(Modifier::BOLD));

    // Down：高亮从 `/help` 挪到 `/quit`，并且只挪一行。
    press(&mut app, KeyCode::Down);
    let (screen, buffer) = render_app_cells(100, 24, &mut app);
    let quit_row = screen
        .lines
        .iter()
        .position(|line| line.contains("▸ /quit"))
        .expect("高亮跟着选中项走");
    let quit = buffer[(column_of(screen.row(quit_row), "/quit"), quit_row as u16)].style();
    assert_eq!(quit.fg, Some(theme.accent_system));
    let help_row = screen
        .lines
        .iter()
        .position(|line| line.contains("/help"))
        .expect("第一项还在");
    let help = buffer[(column_of(screen.row(help_row), "/help"), help_row as u16)].style();
    assert_ne!(help.fg, Some(theme.accent_system), "落选的那行不再高亮");
}

/// 输入把列表筛窄（屏上只剩命中的那些）。
#[test]
fn typing_narrows_the_list_on_the_screen() {
    let mut app = App::new(&capabilities());
    app.set_screen(Screen::Agent);
    type_text(&mut app, "/mdl");
    let screen = render_app(100, 24, &mut app);

    assert_contains(&screen, "▸ /model");
    assert_contains(&screen, "显示当前模型");
    assert!(!screen.text().contains("/permission"), "没命中的候选不画");
    assert!(!screen.text().contains("还有"), "只剩一条，没有溢出提示");
}

/// 选中项走到窗口外时列表跟着滚，并把「还有 N 条」写在末行。
#[test]
fn moving_below_the_visible_window_scrolls_the_list() {
    let mut app = App::new(&capabilities());
    app.set_screen(Screen::Agent);
    type_text(&mut app, "/");
    // 走到最后一个候选（条数会随注册表增长，这里跟着注册表算）。
    for _ in 0..jh_tui::commands::COMMANDS.len() - 1 {
        press(&mut app, KeyCode::Down);
    }
    let screen = render_app(100, 24, &mut app);

    assert_contains(&screen, "▸ /status");
    assert!(
        !screen.text().contains("/help"),
        "窗口已经滚过第一屏：\n{}",
        screen.text()
    );
    assert_contains(&screen, "还有 6 条");
    let status_row = screen
        .lines
        .iter()
        .position(|line| line.contains("空闲"))
        .expect("状态行在屏上");
    let last = screen
        .lines
        .iter()
        .position(|line| line.contains("还有 6 条"))
        .expect("溢出提示");
    assert_eq!(last + 1, status_row, "滚过窗口后列表仍压在回滚区底部");
}

/// 点击列表里的一项 = `Tab` 填入那一项（不是填入当前选中项）。
#[test]
fn clicking_a_candidate_fills_that_one_into_the_prompt() {
    let mut app = App::new(&capabilities());
    app.set_screen(Screen::Agent);
    type_text(&mut app, "/");
    let screen = render_app(100, 24, &mut app);

    // `/model` 是空查询下的第 5 项（没被选中，选中的是第 1 项 `/help`）。
    let (row, column) = locate(&screen, "/model");
    assert_eq!(app.handle_mouse(click(column, row)), None);
    assert_eq!(
        app.prompt().text(),
        "/model ",
        "点击 = 填入那一项，带参数留尾空格"
    );
    assert!(app.completion().is_none(), "进了参数区，列表收起");

    // 点选中的第一项同样成立。
    let mut second = App::new(&capabilities());
    second.set_screen(Screen::Agent);
    type_text(&mut second, "/");
    let screen = render_app(100, 24, &mut second);
    let (row, column) = locate(&screen, "/clear");
    second.handle_mouse(click(column, row));
    assert_eq!(second.prompt().text(), "/clear");
}

/// 首屏也能敲 `/` 命令：候选列表同样压在提示框上方（首屏没有状态行）。
#[test]
fn the_startup_screen_shows_the_list_above_its_prompt() {
    let mut app = App::new(&capabilities());
    assert_eq!(app.screen(), Screen::Startup, "先停在首屏");
    type_text(&mut app, "/");
    let screen = render_app(100, 24, &mut app);

    // 首屏上还有居中面板这个盒子：提示框是屏上最下面那个盒子。
    let prompt_top = screen
        .lines
        .iter()
        .rposition(|line| line.contains('╭'))
        .expect("首屏也有提示框");
    let footer_row = screen
        .lines
        .iter()
        .position(|line| line.contains("还有 6 条"))
        .expect("溢出提示");
    assert_eq!(
        footer_row + 1,
        prompt_top,
        "首屏的列表贴着提示框上方：\n{}",
        screen.text()
    );
    assert_contains(&screen, "▸ /help");
}

/// 无色 + 回退字形档：列表照样画，行首标记退成 `> `。
#[test]
fn the_list_degrades_without_color_and_with_fallback_glyphs() {
    let mut app = App::new(&capabilities_plain());
    app.set_screen(Screen::Agent);
    type_text(&mut app, "/");
    let screen = render_app(100, 24, &mut app);

    assert_contains(&screen, "> /help");
    assert_contains(&screen, "还有 6 条");
    assert!(!screen.text().contains('❯'), "回退档不用花体字形");
}

/// 研究视图没有提示框，补全列表也不画。
#[test]
fn the_research_view_shows_no_completion_list() {
    let mut app = App::new(&capabilities());
    app.set_screen(Screen::Agent);
    type_text(&mut app, "/");
    let agent = render_app(100, 24, &mut app);
    assert_contains(&agent, "还有 6 条");

    app.set_screen(Screen::Research);
    assert!(app.completion().is_none(), "研究视图里补全收起");
    let research = render_app(100, 24, &mut app);
    assert!(
        !research.text().contains("还有"),
        "研究视图不画补全列表：\n{}",
        research.text()
    );
    assert!(
        !research.text().contains("▸ /"),
        "研究视图里没有候选行：\n{}",
        research.text()
    );
}

/// 整屏快照：空查询下候选列表压在回滚区底部（提示框上方隔着状态行），
/// 选中第一项，末行是溢出提示。
#[test]
fn the_agent_screen_with_the_completion_list_matches_the_snapshot() {
    let mut app = App::new(&capabilities());
    app.set_screen(Screen::Agent);
    type_text(&mut app, "/");
    let screen = render_app(100, 24, &mut app);
    assert_screen(
        &screen,
        r#"jev-harness                                                                               [研究视图]












▸ /help        列出全部命令
  /quit        退出会话（等价于 Ctrl+C）
  /clear       清空回滚区（不动会话上下文）
  /research    在会话与研究视图之间切换
  /model       显示当前模型，或换一个模型继续会话
  … 还有 6 条
● 空闲
╭──────────────────────────────────────────────────────────────────────────────────────────────────╮
│ ❯ /                                                                                              │
╰─ 未接线 · 不调用模型与 API ──────────────────────────────────────────────────────────────────────╯
Ctrl+c 退出 · Esc 取消 · Enter 发送 · Shift+Enter/Ctrl+J 换行 · PgUp 上翻 · PgDn 下翻 …"#,
    );
}
