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
use ratatui::layout::{Position, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;

use crate::app::App;
use crate::blocks::{BlockCtx, BlockLine, Selectable};
use crate::components::{
    ApprovalCard, HeaderBar, MAX_PROMPT_ROWS, PanelState, ScrollHint, ShortcutBar, clip_spans,
};
use crate::glyphs;
use crate::screens::{self, Screen};
use crate::startup::{self, StartupFacts};
use crate::width;

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

/// 提示框占几行：按软折行后的可视行数，受区域高度一半与 [`MAX_PROMPT_ROWS`] 限制，
/// 再按 chrome 加上盒子的上下边框。
pub fn prompt_rows(app: &App, area: Rect) -> u16 {
    let content_width = prompt_content_width(app, area.width);
    let rows = app.prompt().visual_rows(content_width);
    let ceiling = (area.height / 2).max(1).min(MAX_PROMPT_ROWS as u16);
    let content_rows = (rows as u16).clamp(1, ceiling);
    app.prompt_chrome().rows(content_rows as usize)
}

/// 提示框内容列宽（扣掉边框、内边距与提示字符占的列）。
pub fn prompt_content_width(app: &App, area_width: u16) -> usize {
    let inner = app.prompt_chrome().inner_width(area_width as usize);
    inner
        .saturating_sub(width::display_width(app.glyphs().resolve(glyphs::prompt())))
        .max(1)
}

/// 唯一绘制入口：按 [`Screen`] 分发到三屏之一。
pub fn draw(frame: &mut Frame, app: &mut App) {
    match app.screen() {
        Screen::Startup => draw_startup(frame, app),
        Screen::Agent => draw_agent(frame, app),
        Screen::Research => draw_research(frame, app),
    }
}

/// 主界面：顶栏 / 回滚区（+ 审批卡、滚动提示）/ 状态行 / 提示框 / 快捷键栏。
fn draw_agent(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let rows = prompt_rows(app, area);
    let layout = screens::agent_layout(area, rows);

    // 布局信息回写：按键处理（纵向移动、翻页）要用到上一次布局算出的宽度与高度。
    app.set_viewport(
        layout.scrollback.width as usize,
        layout.scrollback.height as usize,
        prompt_content_width(app, layout.prompt.width),
    );

    // 1) 顶栏。
    if layout.header.height > 0 {
        let header_ctx = app.block_ctx(layout.header.width as usize);
        let header = app.header();
        render_lines(
            frame,
            layout.header,
            std::iter::once(HeaderBar::render(&header, &header_ctx).content),
        );
    }

    // 2) 回滚区：只画可见窗口里的行（块已经自己换好行了）；不贴底时最后一行让给滚动提示。
    let ctx = app.block_ctx(layout.scrollback.width as usize);
    let window = app
        .scrollback()
        .window(&ctx, layout.scrollback.height as usize);
    let mut lines: Vec<Line<'_>> = window
        .lines
        .iter()
        .map(|line| line.content.clone())
        .collect();
    if let Some(hint) = ScrollHint::render(&window, &ctx)
        && let Some(last) = lines.last_mut()
    {
        *last = hint.content;
    }
    render_lines(frame, layout.scrollback, lines);

    // 3) 审批卡：单槽位模态，压在回滚区底部；在槽位里就持有键盘。
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

    // 4) 状态行。
    let status_ctx = app.block_ctx(layout.status.width as usize);
    let status = app.status().render(&status_ctx);
    render_lines(frame, layout.status, std::iter::once(status.content));

    // 5) 提示框（+ 终端光标）。
    draw_prompt(frame, app, layout.prompt);

    // 6) 快捷键栏：条目来自动作注册表的 hint_priority。
    let bar_ctx = app.block_ctx(layout.shortcuts.width as usize);
    let bar = ShortcutBar::render(&bar_ctx, app.keymap(), app.key_owner());
    render_lines(frame, layout.shortcuts, std::iter::once(bar.content));
}

/// 首屏：居中面板（或小屏退化列表）/ 提示框 / 右下注解。
fn draw_startup(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let rows = prompt_rows(app, area);
    let layout = screens::startup_layout(area, rows);
    let workspace = app.header().title;
    let note = app.startup_note().unwrap_or("未接线").to_owned();
    let facts = StartupFacts {
        // 工作区各 crate 同版本；要显示别的版本号时由宿主的顶栏标题承担。
        version: env!("CARGO_PKG_VERSION"),
        judge: &note,
        workspace: &workspace,
    };

    app.set_viewport(
        layout.body.width as usize,
        layout.body.height as usize,
        prompt_content_width(app, layout.prompt.width),
    );

    if screens::startup_fits(area) {
        let panel_width = screens::startup_panel_width(layout.body);
        let ctx = app.block_ctx(panel_width as usize);
        let panel = startup::panel(&facts, app.glyphs());
        let height = (panel.height(&ctx) as u16).min(layout.body.height).max(1);
        let rect = screens::startup_panel_rect(layout.body, panel_width, height);
        let lines = panel.render(&ctx, height as usize, &PanelState::default(), true);
        render_lines(frame, rect, lines.into_iter().map(|line| line.content));
    } else {
        let ctx = app.block_ctx(layout.body.width as usize);
        render_lines(
            frame,
            layout.body,
            startup::plain(&facts, &ctx)
                .into_iter()
                .map(|line| line.content),
        );
    }

    // 右下注解：判定器与接线状态（如实标注，不假装接了模型）。
    let note_ctx = app.block_ctx(layout.note.width as usize);
    render_lines(
        frame,
        layout.note,
        std::iter::once(note_line(&note, &note_ctx).content),
    );

    draw_prompt(frame, app, layout.prompt);
}

/// 研究视图：顶栏 / 面板带 / 状态行 / 快捷键栏（没有提示框：研究态不接受自然语言输入）。
fn draw_research(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let layout = screens::research_layout(area);

    if layout.header.height > 0 {
        let header_ctx = app.block_ctx(layout.header.width as usize);
        let header = app.header();
        render_lines(
            frame,
            layout.header,
            std::iter::once(HeaderBar::render(&header, &header_ctx).content),
        );
    }

    let band = layout.panel;
    app.set_panel_viewport(band.width as usize, band.height.saturating_sub(2) as usize);
    let ctx = app.block_ctx(band.width as usize);
    let panel = app.panels().panel(app.panel(), app.glyphs());
    let state = *app.panel_state();
    let lines = panel.render(&ctx, band.height as usize, &state, true);
    render_lines(frame, band, lines.into_iter().map(|line| line.content));

    let status_ctx = app.block_ctx(layout.status.width as usize);
    render_lines(
        frame,
        layout.status,
        std::iter::once(research_status(app, &status_ctx).content),
    );

    let bar_ctx = app.block_ctx(layout.shortcuts.width as usize);
    let bar = ShortcutBar::render(&bar_ctx, app.keymap(), app.key_owner());
    render_lines(frame, layout.shortcuts, std::iter::once(bar.content));
}

/// 提示框（三屏共用）：四边盒 + 终端光标。
fn draw_prompt(frame: &mut Frame, app: &mut App, area: Rect) {
    if area.height == 0 {
        return;
    }
    let ctx = app.block_ctx(area.width as usize);
    let focused = app.is_prompt_focused();
    let lines = app.prompt().render(&ctx, app.prompt_chrome(), focused);
    render_lines(frame, area, lines.into_iter().map(|line| line.content));

    if focused {
        let (dx, dy) = app.prompt_chrome().content_origin();
        let (row, column) = app
            .prompt()
            .cursor_cell(prompt_content_width(app, area.width));
        let x = area.x
            + dx
            + width::display_width(app.glyphs().resolve(glyphs::prompt())) as u16
            + column as u16;
        let y = area.y + dy + row as u16;
        if x < area.right() && y < area.bottom() {
            frame.set_cursor_position(Position::new(x, y));
        }
    }
}

/// 研究视图的状态行：屏幕名 + 当前面板 + 暂停标记 + 面板的规模读数。
fn research_status(app: &App, ctx: &BlockCtx) -> BlockLine {
    let separator = ctx.glyphs.resolve(glyphs::separator());
    let mut spans = vec![
        Span::styled(
            format!("{} ", ctx.glyphs.resolve(glyphs::dot())),
            ctx.theme.dim(),
        ),
        Span::styled(Screen::Research.label().to_owned(), ctx.theme.muted()),
        Span::styled(format!(" {separator} "), ctx.theme.dim()),
        Span::styled(
            app.panel().label().to_owned(),
            ctx.theme.accent(ctx.theme.accent_system),
        ),
    ];
    if let Some(summary) = app.panels().summary(app.panel()) {
        spans.push(Span::styled(format!(" {separator} "), ctx.theme.dim()));
        spans.push(Span::styled(summary, ctx.theme.dim()));
    }
    if app.paused() {
        spans.push(Span::styled(format!(" {separator} "), ctx.theme.dim()));
        spans.push(Span::styled(
            "已暂停".to_owned(),
            ctx.theme.accent(ctx.theme.accent_warning),
        ));
    }
    BlockLine::new(
        Line::from(clip_spans(
            spans,
            ctx.width,
            ctx.glyphs.resolve(glyphs::ellipsis()),
        )),
        Selectable::All,
    )
}

/// 右对齐的一行（首屏右下注解）。
fn note_line(text: &str, ctx: &BlockCtx) -> BlockLine {
    let text = width::clip_with_ellipsis(text, ctx.width, ctx.glyphs.resolve(glyphs::ellipsis()));
    let padding = ctx.width.saturating_sub(width::display_width(&text));
    BlockLine::new(
        Line::from(vec![
            Span::styled(" ".repeat(padding), Style::new()),
            Span::styled(text, ctx.theme.dim()),
        ]),
        Selectable::All,
    )
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
        // 12 行 < SHORT_TERMINAL_ROWS：顶栏收起，其余自上而下排开。
        let area = Rect::new(0, 0, 40, 12);
        let layout = screens::agent_layout(area, 3);
        assert_eq!(layout.header.height, 0);
        assert_eq!(layout.scrollback, Rect::new(0, 0, 40, 7));
        assert_eq!(layout.status, Rect::new(0, 7, 40, 1));
        assert_eq!(layout.prompt, Rect::new(0, 8, 40, 3));
        assert_eq!(layout.shortcuts, Rect::new(0, 11, 40, 1));
    }
}
