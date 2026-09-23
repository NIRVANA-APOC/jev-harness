//! 屏幕与布局：`Startup` / `Agent` / `Research` 三屏的纯函数布局。
//!
//! 设计来源：`docs/tui-design.md` §5——整屏自上而下只调用一次 `Layout::vertical`，
//! 横向没有分栏；唯一的弹性行是回滚区（研究视图里是面板带）。
//! 尺寸阈值取自调研报告 §4.1（`SHORT_TERMINAL_ROWS = 16`），本文件是唯一的常量出处。

use ratatui::layout::{Constraint, Layout, Rect};

/// 屏幕：切换屏幕只改这一个值，`render::draw` 按它分发。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Screen {
    /// 首屏：居中面板 + 提示框 + 右下注解。
    #[default]
    Startup,
    /// 主界面：顶栏 + 回滚区 + 状态行 + 提示框 + 快捷键栏。
    Agent,
    /// 研究视图：顶栏 + 面板带 + 状态行 + 快捷键栏（无提示框）。
    Research,
}

impl Screen {
    pub fn label(self) -> &'static str {
        match self {
            Screen::Startup => "首屏",
            Screen::Agent => "会话",
            Screen::Research => "研究视图",
        }
    }
}

/// 短终端阈值：低于它就收起顶栏（调研报告 §4.1 的同名常量）。
pub const SHORT_TERMINAL_ROWS: u16 = 16;
/// 首屏能画出居中面板的最小高度与宽度；再小退化成纯文本列表。
pub const MIN_STARTUP_ROWS: u16 = 12;
pub const MIN_STARTUP_COLS: u16 = 48;
/// 首屏面板的最大宽度：宽屏下不让面板横跨整屏（对齐参考截图的居中卡片）。
pub const MAX_STARTUP_PANEL_COLS: u16 = 64;
/// 首屏面板与屏幕边缘的最小留白。
const STARTUP_PANEL_MARGIN: u16 = 4;

/// 主界面的五个区域，自上而下。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentLayout {
    pub header: Rect,
    pub scrollback: Rect,
    pub status: Rect,
    pub prompt: Rect,
    pub shortcuts: Rect,
}

/// 主界面布局：顶栏（短终端收起）/ 回滚区（唯一弹性）/ 状态行 / 提示框 / 快捷键栏。
pub fn agent_layout(area: Rect, prompt_rows: u16) -> AgentLayout {
    let header_rows = if area.height >= SHORT_TERMINAL_ROWS {
        1
    } else {
        0
    };
    let [header, scrollback, status, prompt, shortcuts] = Layout::vertical([
        Constraint::Length(header_rows),
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(prompt_rows),
        Constraint::Length(1),
    ])
    .areas(area);
    AgentLayout {
        header,
        scrollback,
        status,
        prompt,
        shortcuts,
    }
}

/// 研究视图的四个区域。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResearchLayout {
    pub header: Rect,
    pub panel: Rect,
    pub status: Rect,
    pub shortcuts: Rect,
}

/// 研究视图布局：顶栏 / 面板带（唯一弹性）/ 状态行 / 快捷键栏。
pub fn research_layout(area: Rect) -> ResearchLayout {
    let header_rows = if area.height >= SHORT_TERMINAL_ROWS {
        1
    } else {
        0
    };
    let [header, panel, status, shortcuts] = Layout::vertical([
        Constraint::Length(header_rows),
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);
    ResearchLayout {
        header,
        panel,
        status,
        shortcuts,
    }
}

/// 首屏的三个区域：面板带（其余高度）/ 提示框 / 右下注解。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartupLayout {
    pub body: Rect,
    pub prompt: Rect,
    pub note: Rect,
}

/// 首屏布局：注解固定 1 行、提示框按内容行数、其余留给居中面板。
pub fn startup_layout(area: Rect, prompt_rows: u16) -> StartupLayout {
    let [body, prompt, note] = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(prompt_rows),
        Constraint::Length(1),
    ])
    .areas(area);
    StartupLayout { body, prompt, note }
}

/// 首屏面板的宽度：不超过 [`MAX_STARTUP_PANEL_COLS`]，两侧各留 [`STARTUP_PANEL_MARGIN`]。
pub fn startup_panel_width(area: Rect) -> u16 {
    area.width
        .saturating_sub(STARTUP_PANEL_MARGIN * 2)
        .clamp(1, MAX_STARTUP_PANEL_COLS)
}

/// 首屏面板的位置：横行居中、纵向偏上（参考截图的面板重心在上三分之一）。
pub fn startup_panel_rect(body: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(body.width).max(1);
    let height = height.min(body.height).max(1);
    let x = body.x + (body.width.saturating_sub(width)) / 2;
    let y = body.y + (body.height.saturating_sub(height)) / 3;
    Rect::new(x, y, width, height)
}

/// 首屏是否够大：不够就退化成纯文本列表（面板与字标都不画）。
pub fn startup_fits(area: Rect) -> bool {
    area.height >= MIN_STARTUP_ROWS && area.width >= MIN_STARTUP_COLS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_layout_stacks_five_bands() {
        let layout = agent_layout(Rect::new(0, 0, 80, 24), 3);
        assert_eq!(layout.header, Rect::new(0, 0, 80, 1));
        assert_eq!(layout.scrollback, Rect::new(0, 1, 80, 18));
        assert_eq!(layout.status, Rect::new(0, 19, 80, 1));
        assert_eq!(layout.prompt, Rect::new(0, 20, 80, 3));
        assert_eq!(layout.shortcuts, Rect::new(0, 23, 80, 1));
    }

    #[test]
    fn short_terminals_drop_the_header_first() {
        let layout = agent_layout(Rect::new(0, 0, 80, SHORT_TERMINAL_ROWS - 1), 3);
        assert_eq!(layout.header.height, 0, "短终端先收顶栏");
        // 15 行 = 回滚区 10 + 状态行 1 + 提示框 3 + 快捷键栏 1。
        assert_eq!(layout.scrollback.height, 10);
        let research = research_layout(Rect::new(0, 0, 80, 10));
        assert_eq!(research.header.height, 0);
        assert_eq!(research.panel, Rect::new(0, 0, 80, 8));
    }

    #[test]
    fn research_layout_has_no_prompt_band() {
        let layout = research_layout(Rect::new(0, 0, 100, 30));
        assert_eq!(layout.header, Rect::new(0, 0, 100, 1));
        assert_eq!(layout.panel, Rect::new(0, 1, 100, 27));
        assert_eq!(layout.status, Rect::new(0, 28, 100, 1));
        assert_eq!(layout.shortcuts, Rect::new(0, 29, 100, 1));
    }

    #[test]
    fn startup_layout_reserves_the_prompt_and_the_note() {
        let layout = startup_layout(Rect::new(0, 0, 80, 24), 3);
        assert_eq!(layout.body, Rect::new(0, 0, 80, 20));
        assert_eq!(layout.prompt, Rect::new(0, 20, 80, 3));
        assert_eq!(layout.note, Rect::new(0, 23, 80, 1));
    }

    #[test]
    fn startup_panel_is_centered_and_biased_upwards() {
        let body = Rect::new(0, 0, 80, 21);
        let rect = startup_panel_rect(body, startup_panel_width(body), 9);
        assert_eq!(rect.width, MAX_STARTUP_PANEL_COLS);
        assert_eq!(rect.x, (80 - MAX_STARTUP_PANEL_COLS) / 2);
        assert_eq!(rect.y, (21 - 9) / 3);
        // 面板比屏幕还大时收敛到整块，不越界。
        let tiny = startup_panel_rect(Rect::new(0, 0, 20, 4), 40, 20);
        assert_eq!(tiny, Rect::new(0, 0, 20, 4));
    }

    #[test]
    fn startup_degrades_on_small_screens() {
        assert!(startup_fits(Rect::new(0, 0, 80, 24)));
        assert!(!startup_fits(Rect::new(0, 0, 80, MIN_STARTUP_ROWS - 1)));
        assert!(!startup_fits(Rect::new(0, 0, MIN_STARTUP_COLS - 1, 24)));
    }
}
