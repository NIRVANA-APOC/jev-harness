//! 帧管线：布局、按需渲染闸门、缓冲区文本化。
//!
//! 设计来源：`docs/grok-tui-design.md` §6.2、§7.1 第 6 条（三条件重绘闸门 + 空闲零字节 +
//! 整数除法降帧）与专题 `docs/research/grok-tui/25-render-pipeline-performance.md`
//! （`try_present` 的三条件、`TickDemand::{None, Slow, Fast}`、`SLOW_TICK_INTERVAL = 83ms`、
//! `min_draw_interval` 默认 16ms）与专题 06（`SPINNER_DIVISOR = 4`）。
//!
//! 与上游的差异：上游有独立的 writer 线程与序号（`sequence`）协议；本项目把「在途帧」与
//! 「writer 排空」做成同一套序号的两端，宿主可以单线程立即 ack，也可以异步延后 ack。

use std::time::Duration;

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::text::{Line, Text};
use ratatui::widgets::Paragraph;

use crate::app::App;
use crate::components::{ApprovalCard, MAX_PROMPT_ROWS, ShortcutBar};

/// 动画降帧：每 4 个 tick 前进一帧（上游 `SPINNER_DIVISOR = 4`，专题 06 §2）。
pub const SPINNER_DIVISOR: u64 = 4;
/// 动画 tick 间隔（上游动画 30fps，专题 06）。
pub const TICK_INTERVAL: Duration = Duration::from_millis(33);
/// 慢 tick：有在途轮次但不动画时的重绘间隔（上游 `SLOW_TICK_INTERVAL = 83ms`）。
pub const SLOW_TICK_INTERVAL: Duration = Duration::from_millis(83);
/// 空闲时的等待时长：没有在途信号就干脆不醒（`TickDemand::None` 的落地写法）。
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(3600);

/// 这一轮需不需要唤醒画一帧（上游 `TickDemand`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TickDemand {
    /// 不需要定时唤醒：空闲时零重绘、零字节。
    #[default]
    None,
    /// 慢速唤醒：有在途轮次但不做动画。
    Slow,
    /// 快速唤醒：正在动画（转轮）。
    Fast,
}

impl TickDemand {
    /// 下一次唤醒的间隔。
    pub fn interval(self) -> Duration {
        match self {
            TickDemand::None => IDLE_TIMEOUT,
            TickDemand::Slow => SLOW_TICK_INTERVAL,
            TickDemand::Fast => TICK_INTERVAL,
        }
    }
}

/// 动画帧号：用整数除法降帧，不另设帧预算常量（设计 §7.1 第 6 条）。
pub fn animation_frame(tick: u64) -> usize {
    (tick / SPINNER_DIVISOR) as usize
}

/// 三条件重绘闸门：`dirty ∧ 无在途帧 ∧ writer 排空`。
///
/// 对应上游 `event_loop.rs:579-600` 的 `try_present`：先看 writer 水位，再看在途帧与
/// dirty；开画时清 dirty 并记下新的水位。这样终端读不过来时循环仍在处理输入，
/// 而不是排队等帧。
#[derive(Debug, Clone)]
pub struct RenderGate {
    dirty: bool,
    in_flight: Option<u64>,
    queued: u64,
    written: u64,
    demand: TickDemand,
}

impl Default for RenderGate {
    /// 首帧必须先画：初值就是 dirty。
    fn default() -> Self {
        RenderGate {
            dirty: true,
            in_flight: None,
            queued: 0,
            written: 0,
            demand: TickDemand::None,
        }
    }
}

impl RenderGate {
    pub fn new() -> Self {
        RenderGate::default()
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// 在途未确认的帧序号。
    pub fn in_flight(&self) -> Option<u64> {
        self.in_flight
    }

    pub fn queued(&self) -> u64 {
        self.queued
    }

    pub fn written(&self) -> u64 {
        self.written
    }

    pub fn demand(&self) -> TickDemand {
        self.demand
    }

    pub fn set_demand(&mut self, demand: TickDemand) {
        self.demand = demand;
    }

    /// 三个条件全部满足才认为该画一帧。
    pub fn should_draw(&self) -> bool {
        self.dirty && self.in_flight.is_none() && self.written >= self.queued
    }

    /// 占用一个帧序号（清 dirty）。宿主拿到序号，画完并 flush 后送回 [`RenderGate::acknowledge`]。
    pub fn queue_frame(&mut self) -> u64 {
        self.queued += 1;
        self.in_flight = Some(self.queued);
        self.dirty = false;
        self.queued
    }

    /// writer 回报「序号 `sequence` 之前的字节都写出去了」。
    pub fn acknowledge(&mut self, sequence: u64) {
        self.written = self.written.max(sequence);
        if self.in_flight.is_some_and(|target| self.written >= target) {
            self.in_flight = None;
        }
    }

    /// 宿主事件循环下一次 poll 的等待时长。
    pub fn poll_timeout(&self) -> Duration {
        self.demand.interval()
    }
}

/// 一屏的四个区域。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppLayout {
    pub scrollback: Rect,
    pub status: Rect,
    pub prompt: Rect,
    pub shortcuts: Rect,
}

/// 自上而下：回滚区（吃剩余高度）/ 状态行（1）/ 提示框（1..=n）/ 快捷键栏（1）。
pub fn layout(area: Rect, prompt_rows: u16) -> AppLayout {
    let [scrollback, status, prompt, shortcuts] = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(prompt_rows),
        Constraint::Length(1),
    ])
    .areas(area);
    AppLayout {
        scrollback,
        status,
        prompt,
        shortcuts,
    }
}

/// 提示框占几行：按软折行后的可视行数，受区域高度一半与 [`MAX_PROMPT_ROWS`] 限制。
pub fn prompt_rows(app: &App, area: Rect) -> u16 {
    let content_width = prompt_content_width(app, area.width);
    let rows = app.prompt().visual_rows(content_width);
    let ceiling = (area.height / 2).max(1).min(MAX_PROMPT_ROWS as u16);
    (rows as u16).clamp(1, ceiling)
}

/// 提示框内容列宽（扣掉提示字符占的列）。
pub fn prompt_content_width(app: &App, area_width: u16) -> usize {
    (area_width as usize)
        .saturating_sub(crate::width::display_width(
            app.glyphs().resolve(crate::glyphs::prompt()),
        ))
        .max(1)
}

/// 唯一绘制入口：一屏画完所有组件。
pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let rows = prompt_rows(app, area);
    let layout = layout(area, rows);

    // 布局信息回写：按键处理（纵向移动、翻页）要用到上一次布局算出的宽度与高度。
    app.set_viewport(
        layout.scrollback.width as usize,
        layout.scrollback.height as usize,
        prompt_content_width(app, layout.prompt.width),
    );

    // 1) 回滚区：只画可见窗口里的行（块已经自己换好行了）。
    let ctx = app.block_ctx(layout.scrollback.width as usize);
    let window = app
        .scrollback()
        .window(&ctx, layout.scrollback.height as usize);
    render_lines(
        frame,
        layout.scrollback,
        window.lines.iter().map(|line| line.content.clone()),
    );

    // 2) 审批卡：单槽位模态，压在回滚区底部；在槽位里就持有键盘。
    if let Some(modal) = app.modal() {
        let request = modal.request().clone();
        let card_ctx = app.block_ctx(layout.scrollback.width as usize);
        let max_rows = layout.scrollback.height.saturating_sub(1).max(1) as usize;
        let card = ApprovalCard::render(&request, app.keymap(), &card_ctx, true, max_rows);
        let height = card.len() as u16;
        if height > 0 && height <= layout.scrollback.height {
            let card_area = Rect::new(
                layout.scrollback.x,
                layout.scrollback.y + layout.scrollback.height - height,
                layout.scrollback.width,
                height,
            );
            render_lines(frame, card_area, card.into_iter().map(|line| line.content));
        }
    }

    // 3) 状态行。
    let status_ctx = app.block_ctx(layout.status.width as usize);
    let status = app.status().render(&status_ctx);
    render_lines(frame, layout.status, std::iter::once(status.content));

    // 4) 提示框（+ 终端光标）。
    let prompt_ctx = app.block_ctx(layout.prompt.width as usize);
    let focused = app.is_prompt_focused();
    let prompt_lines = app.prompt().render(&prompt_ctx, focused);
    render_lines(
        frame,
        layout.prompt,
        prompt_lines.into_iter().map(|line| line.content),
    );
    if focused {
        let (row, column) = app
            .prompt()
            .cursor_cell(prompt_content_width(app, layout.prompt.width));
        let x = layout.prompt.x
            + crate::width::display_width(event_prompt_glyph(app)) as u16
            + column as u16;
        let y = layout.prompt.y + row as u16;
        if x < layout.prompt.right() && y < layout.prompt.bottom() {
            frame.set_cursor_position(Position::new(x, y));
        }
    }

    // 5) 快捷键栏：条目来自动作注册表的 hint_priority。
    let bar = ShortcutBar::render(&prompt_ctx, app.keymap(), app.key_owner());
    render_lines(frame, layout.shortcuts, std::iter::once(bar.content));
}

fn event_prompt_glyph(app: &App) -> &'static str {
    app.glyphs().resolve(crate::glyphs::prompt())
}

/// 把一组已换行的行画进矩形（块已经换好行，这里不做折行）。
fn render_lines<'a>(frame: &mut Frame, area: Rect, lines: impl IntoIterator<Item = Line<'a>>) {
    let text = Text::from(lines.into_iter().collect::<Vec<_>>());
    frame.render_widget(Paragraph::new(text), area);
}

/// 把缓冲区取成纯文本行（去掉行尾空白、跳过宽字符的续格）：快照测试与
/// `--example demo` 共用。宽字符占两格，第二格里是空占位，直接拼会多出一个空格。
pub fn screen_lines(buffer: &Buffer) -> Vec<String> {
    let area = buffer.area;
    (area.y..area.bottom())
        .map(|y| {
            let mut line = String::new();
            let mut continuation_cells = 0usize;
            for x in area.x..area.right() {
                if continuation_cells > 0 {
                    continuation_cells -= 1;
                    continue;
                }
                let symbol = buffer[(x, y)].symbol();
                continuation_cells = crate::width::display_width(symbol).saturating_sub(1);
                line.push_str(symbol);
            }
            line.trim_end().to_owned()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_needs_all_three_conditions() {
        let mut gate = RenderGate::new();
        // 首帧：dirty ∧ 无在途 ∧ 水位齐平 → 画。
        assert!(gate.should_draw());
        let sequence = gate.queue_frame();
        assert_eq!(sequence, 1);
        assert!(!gate.should_draw(), "有在途帧时不许再画（背压）");
        assert!(!gate.is_dirty(), "开画即清 dirty");
        gate.acknowledge(sequence);
        assert!(!gate.should_draw(), "空闲不重绘：没有新变化就不画");
    }

    #[test]
    fn writer_backlog_blocks_the_frame() {
        let mut gate = RenderGate::new();
        gate.mark_dirty();
        gate.acknowledge(0);
        // 队列水位高于已写水位（writer 还没排空）→ 不画。
        let sequence = gate.queue_frame();
        gate.acknowledge(sequence - 1);
        assert!(gate.written() < gate.queued());
        assert!(!gate.should_draw());
        gate.acknowledge(sequence);
        assert!(gate.in_flight().is_none());
        gate.mark_dirty();
        assert!(gate.should_draw(), "水位追上后即可继续画");
    }

    #[test]
    fn stale_acknowledgement_does_not_release_an_in_flight_frame() {
        let mut gate = RenderGate::new();
        let first = gate.queue_frame();
        gate.acknowledge(first);
        let second = gate.queue_frame();
        // 迟到的旧序号不足以释放新帧。
        gate.acknowledge(first);
        assert_eq!(gate.in_flight(), Some(second));
        gate.acknowledge(second);
        assert_eq!(gate.in_flight(), None);
    }

    #[test]
    fn tick_demand_drives_the_poll_timeout() {
        let mut gate = RenderGate::new();
        assert_eq!(gate.demand(), TickDemand::None);
        assert_eq!(gate.poll_timeout(), IDLE_TIMEOUT);
        gate.set_demand(TickDemand::Fast);
        assert_eq!(gate.poll_timeout(), TICK_INTERVAL);
        gate.set_demand(TickDemand::Slow);
        assert_eq!(gate.poll_timeout(), SLOW_TICK_INTERVAL);
    }

    #[test]
    fn animation_frame_uses_integer_division() {
        assert_eq!(animation_frame(0), 0);
        assert_eq!(animation_frame(3), 0);
        assert_eq!(animation_frame(4), 1);
        assert_eq!(animation_frame(9), 2);
    }

    #[test]
    fn layout_carves_the_screen_from_top_to_bottom() {
        let area = Rect::new(0, 0, 40, 12);
        let layout = layout(area, 3);
        assert_eq!(layout.scrollback, Rect::new(0, 0, 40, 7));
        assert_eq!(layout.status, Rect::new(0, 7, 40, 1));
        assert_eq!(layout.prompt, Rect::new(0, 8, 40, 3));
        assert_eq!(layout.shortcuts, Rect::new(0, 11, 40, 1));
    }
}
