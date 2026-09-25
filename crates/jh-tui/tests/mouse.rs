//! 鼠标：点击命中区走与快捷键同一条动作通路；滚轮翻页。
//!
//! 断言一律先渲染一帧、再**从画出来的屏上**定位坐标去点——这样「画在哪」与「点得到哪」
//! 是同一份布局，测试也就钉得住两者的漂移。

mod common;

use common::{capabilities, render_app};
use jh_tui::app::{App, ApprovalOutcome, Effect};
use jh_tui::blocks::Block;
use jh_tui::components::ApprovalRequest;
use jh_tui::screens::Screen;
use jh_tui::width::display_width;
use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

/// 在屏上找一个标签，返回它的 (行, 起始显示列)——宽字符按显示宽度算。
fn locate(screen: &common::Screen, needle: &str) -> (u16, u16) {
    for (row, line) in screen.lines.iter().enumerate() {
        if let Some(index) = line.find(needle) {
            let column = display_width(&line[..index]) as u16;
            return (row as u16, column);
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

fn wheel(kind: MouseEventKind) -> MouseEvent {
    MouseEvent {
        kind,
        column: 1,
        row: 1,
        modifiers: KeyModifiers::NONE,
    }
}

#[test]
fn clicking_an_approval_option_answers_like_the_key() {
    let mut app = App::new(&capabilities());
    app.set_screen(Screen::Agent);
    app.open_approval(ApprovalRequest::new("bash", "ls -1"));
    let screen = render_app(80, 24, &mut app);

    // 「允许一次」按钮：点它 = 按 y。
    let (row, column) = locate(&screen, "允许一次");
    assert_eq!(
        app.handle_mouse(click(column, row)),
        Some(Effect::Approval(ApprovalOutcome::AllowOnce)),
        "点「允许一次」要给出与按键相同的裁决"
    );

    // 重新画一帧再点「拒绝」：位置由布局给出，不是写死的。
    app.close_approval();
    app.open_approval(ApprovalRequest::new("bash", "rm -rf build"));
    let screen = render_app(80, 24, &mut app);
    let (row, column) = locate(&screen, "拒绝");
    assert_eq!(
        app.handle_mouse(click(column, row)),
        Some(Effect::Approval(ApprovalOutcome::Deny))
    );
}

#[test]
fn a_click_off_the_buttons_does_nothing() {
    let mut app = App::new(&capabilities());
    app.set_screen(Screen::Agent);
    app.open_approval(ApprovalRequest::new("bash", "ls -1"));
    let _ = render_app(80, 24, &mut app);
    // 回滚区正中：没有登记任何命中区。
    assert_eq!(app.handle_mouse(click(40, 3)), None);
    assert!(app.modal().is_some(), "空点不该裁决掉卡片");
}

#[test]
fn clicking_a_shortcut_bar_chip_runs_that_action() {
    let mut app = App::new(&capabilities());
    app.set_screen(Screen::Agent);
    let screen = render_app(100, 24, &mut app);

    let (row, column) = locate(&screen, "退出");
    assert!(row as usize >= 20, "应该点在底部快捷键栏：第 {row} 行");
    assert_eq!(app.handle_mouse(click(column, row)), Some(Effect::Quit));
    assert!(app.should_quit());
}

#[test]
fn clicking_the_startup_menu_switches_screens() {
    let mut app = App::new(&capabilities());
    let screen = render_app(80, 24, &mut app);
    let (row, column) = locate(&screen, "研究视图");
    assert_eq!(app.handle_mouse(click(column, row)), None);
    assert_eq!(app.screen(), Screen::Research, "点菜单项要真的切屏");

    // 切回去再点「退出」。
    app.set_screen(Screen::Startup);
    let screen = render_app(80, 24, &mut app);
    let (row, column) = locate(&screen, "退出");
    assert_eq!(app.handle_mouse(click(column, row)), Some(Effect::Quit));
}

#[test]
fn the_wheel_scrolls_the_scrollback_instead_of_needing_page_keys() {
    let mut app = App::new(&capabilities());
    app.set_screen(Screen::Agent);
    for index in 0..40 {
        app.push_block(Block::plain_notice(format!("第 {index} 行")));
    }
    let bottom = render_app(60, 16, &mut app);
    assert!(bottom.text().contains("第 39 行"), "先停在底部");

    // 滚轮向上 = 上翻一页（与 PgUp 同一个动作）。
    assert_eq!(app.handle_mouse(wheel(MouseEventKind::ScrollUp)), None);
    let scrolled = render_app(60, 16, &mut app);
    assert_ne!(bottom.text(), scrolled.text(), "滚轮要改变可见窗口");
    assert!(
        !scrolled.text().contains("第 39 行"),
        "上翻后不该还是底部：\n{}",
        scrolled.text()
    );

    // 滚轮向下 = 下翻，回到更靠后的内容。
    app.handle_mouse(wheel(MouseEventKind::ScrollDown));
    let back = render_app(60, 16, &mut app);
    assert!(back.text().contains("第 39 行"), "下翻回到底部");
}

#[test]
fn the_wheel_scrolls_the_research_panel_band_too() {
    let mut app = App::new(&capabilities());
    app.set_screen(Screen::Research);
    let before = render_app(80, 24, &mut app);
    // 面板内容不足一屏时上滚是空操作，但动作路由必须落在研究屏（不炸、不改屏）。
    app.handle_mouse(wheel(MouseEventKind::ScrollUp));
    let after = render_app(80, 24, &mut app);
    assert_eq!(app.screen(), Screen::Research);
    assert_eq!(before.text(), after.text(), "内容不足一屏时不移动");
}
