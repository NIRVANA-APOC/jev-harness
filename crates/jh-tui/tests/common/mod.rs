//! 快照测试的公共工具：一切断言都打在 `TestBackend` 的缓冲区上，离线、不碰真实终端。

#![allow(dead_code)]

use jh_tui::app::App;
use jh_tui::render;
use jh_tui::terminal::Capabilities;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Text};
use ratatui::widgets::Paragraph;

/// 一屏的纯文本快照（行尾空白已裁掉）。
#[derive(Debug, Clone)]
pub struct Screen {
    pub width: u16,
    pub height: u16,
    pub lines: Vec<String>,
}

impl Screen {
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    /// 第 `row` 行（0 起）。
    pub fn row(&self, row: usize) -> &str {
        &self.lines[row]
    }
}

/// 花体字形 + 真彩能力位（快照里的文字形态与样式都确定）。
pub fn capabilities() -> Capabilities {
    Capabilities::from_env_lookup(
        |key| match key {
            "COLORTERM" => Some("truecolor".to_owned()),
            _ => None,
        },
        true,
        false,
    )
}

/// 无色能力位（`NO_COLOR`）＋ 回退字形：降级路径的快照用。
pub fn capabilities_plain() -> Capabilities {
    Capabilities::from_env_lookup(
        |key| (key == "NO_COLOR").then(|| "1".to_owned()),
        true,
        true,
    )
}

/// 把一组行直接画进 `TestBackend`（组件级快照用）。
pub fn render_lines(width: u16, height: u16, lines: Vec<Line<'static>>) -> Screen {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("TestBackend 可用");
    terminal
        .draw(|frame| {
            frame.render_widget(
                Paragraph::new(Text::from(lines)),
                Rect::new(0, 0, width, height),
            );
        })
        .expect("绘制不失败");
    capture(&mut terminal)
}

/// 组件级渲染，同时把缓冲区交回来做样式断言（失焦降饱和、无色档都用得上）。
pub fn render_lines_cells(width: u16, height: u16, lines: Vec<Line<'static>>) -> (Screen, Buffer) {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("TestBackend 可用");
    terminal
        .draw(|frame| {
            frame.render_widget(
                Paragraph::new(Text::from(lines)),
                Rect::new(0, 0, width, height),
            );
        })
        .expect("绘制不失败");
    let screen = capture(&mut terminal);
    (screen, terminal.backend().buffer().clone())
}

/// 整屏渲染（走 [`render::draw`]，含布局与光标）。
pub fn render_app(width: u16, height: u16, app: &mut App) -> Screen {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("TestBackend 可用");
    terminal
        .draw(|frame| render::draw(frame, app))
        .expect("绘制不失败");
    capture(&mut terminal)
}

/// 整屏渲染并同时取回终端光标位置（提示框光标断言用）。
pub fn render_app_with_cursor(width: u16, height: u16, app: &mut App) -> (Screen, (u16, u16)) {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("TestBackend 可用");
    terminal
        .draw(|frame| render::draw(frame, app))
        .expect("绘制不失败");
    let cursor = terminal.backend().cursor_position();
    (capture(&mut terminal), (cursor.x, cursor.y))
}

fn capture(terminal: &mut Terminal<TestBackend>) -> Screen {
    let buffer: &Buffer = terminal.backend().buffer();
    Screen {
        width: buffer.area.width,
        height: buffer.area.height,
        lines: render::screen_lines(buffer),
    }
}

/// 断言整屏快照：逐行比对（行尾空白与**行尾空行**不参与比较，它们只是画布高度留下的），
/// 失败时把两边都打出来。
#[track_caller]
pub fn assert_screen(screen: &Screen, expected: &str) {
    fn normalize(lines: &[String]) -> Vec<&str> {
        let mut out: Vec<&str> = lines.iter().map(|line| line.trim_end()).collect();
        while out.last().is_some_and(|line| line.is_empty()) {
            out.pop();
        }
        out
    }
    let expected_lines: Vec<String> = expected
        .lines()
        .map(|line| line.trim_end().to_owned())
        .collect();
    if normalize(&screen.lines) == normalize(&expected_lines) {
        return;
    }
    panic!(
        "快照不一致（宽 {} 高 {}）\n--- 实际 ---\n{}\n--- 期望 ---\n{}\n",
        screen.width,
        screen.height,
        screen.text(),
        expected_lines.join("\n")
    );
}

/// 断言某一行包含片段（快照之外的粗粒度检查）。
#[track_caller]
pub fn assert_contains(screen: &Screen, needle: &str) {
    assert!(
        screen.lines.iter().any(|line| line.contains(needle)),
        "屏上没有找到 {needle:?}\n--- 实际 ---\n{}",
        screen.text()
    );
}
