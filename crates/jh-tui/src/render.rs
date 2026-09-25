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
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;

use crate::actions::ActionId;
use crate::app::{App, Selection};
use crate::blocks::{Block, BlockCtx, BlockLine, Selectable};
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
    // 命中区跟着这一帧重建：清掉上一帧的，避免布局变了还能点到旧按钮。
    app.clear_click_targets();
    // 补全状态跟着提示框文本走：宿主直接改写文本时，画之前先对一次账。
    app.refresh_completion();
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
    app.set_scrollback_origin(layout.scrollback.x, layout.scrollback.y);

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
    // 选择高亮在**构造可见行时**贴上：与文字走同一条渲染路径（当帧生效），
    // 而不是画完再回头补缓冲区（那条路要等下一次 flush 才上屏）。
    let selection = app.selection();
    let window_start = (window.total - window.offset).saturating_sub(window.lines.len());
    let mut lines: Vec<Line<'_>> = window
        .lines
        .iter()
        .enumerate()
        .map(|(row, line)| match selection {
            Some(selection) => highlight_selection(&line.content, &selection, window_start + row),
            None => line.content.clone(),
        })
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
        let height = card.lines.len() as u16;
        if height > 0 && height <= layout.scrollback.height {
            let card_area = Rect::new(
                layout.scrollback.x,
                layout.scrollback.y + layout.scrollback.height - height,
                layout.scrollback.width,
                height,
            );
            render_lines(
                frame,
                card_area,
                card.lines.into_iter().map(|line| line.content),
            );
            // 选项行上的按钮：点击 = 按那个键（同一个动作 ID，走同一条 apply 通路）。
            for option in card.options {
                let x = card_area.x + option.column as u16;
                let y = card_area.y + option.row as u16;
                let width = option.width as u16;
                if x + width <= card_area.right() && y < card_area.bottom() {
                    app.register_click(Rect::new(x, y, width, 1), option.action);
                }
            }
        }
    }

    // 4) 浮层：命令的选项列表优先于补全列表（同一块地方，先开的先画）。
    if !draw_choices(frame, app, layout.scrollback) {
        draw_completion(frame, app, layout.scrollback);
    }

    // 5) 状态行。
    let status_ctx = app.block_ctx(layout.status.width as usize);
    let status = app.status().render(&status_ctx);
    render_lines(frame, layout.status, std::iter::once(status.content));

    // 6) 提示框（+ 终端光标）。
    draw_prompt(frame, app, layout.prompt);

    // 7) 快捷键栏：条目来自动作注册表的 hint_priority。
    draw_shortcut_bar(frame, app, layout.shortcuts);
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
    // 首屏没有回滚区：把原点挪开，免得鼠标在首屏拖出一次「选择」。
    app.set_scrollback_origin(0, 0);

    if screens::startup_fits(area) {
        let panel_width = screens::startup_panel_width(layout.body);
        let ctx = app.block_ctx(panel_width as usize);
        let panel = startup::panel(&facts, app.glyphs());
        let height = (panel.height(&ctx) as u16).min(layout.body.height).max(1);
        let rect = screens::startup_panel_rect(layout.body, panel_width, height);
        let lines = panel.render(&ctx, height as usize, &PanelState::default(), true);
        // 菜单里带真动作的两项整行可点（「新建会话」没有对应动作，不登记，免得点了没反应）。
        let menu: Vec<(usize, ActionId)> = startup::MENU
            .iter()
            .filter_map(|(label, _)| menu_action(label))
            .filter_map(|(label, action)| {
                lines
                    .iter()
                    .position(|line| line.text().contains(label))
                    .map(|row| (row, action))
            })
            .collect();
        render_lines(frame, rect, lines.into_iter().map(|line| line.content));
        // 首屏不画回滚区，但**通知**不能因此看不见：按 `Ctrl+V` 得到的「粘贴该按哪个键」
        // 就是在这种时候回的。把它画在面板下方那块空白里（放不下就不画，不硬挤）。
        draw_startup_notice(frame, app, layout.body, rect.bottom() + 1);
        for (row, action) in menu {
            if row < rect.height as usize {
                app.register_click(
                    Rect::new(rect.x, rect.y + row as u16, rect.width, 1),
                    action,
                );
            }
        }
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

    // 首屏也能敲 `/` 命令：候选列表同样压在提示框上方（面板带底部）。
    if !draw_choices(frame, app, layout.body) {
        draw_completion(frame, app, layout.body);
    }

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

    draw_shortcut_bar(frame, app, layout.shortcuts);
}

/// 首屏的最后一格通知：贴在居中面板下方（首屏没有回滚区，通知没别的地方去）。
///
/// 只画 `Notice` 块——首屏本来就不显示历史内容，别让别的东西从这儿漏出来。
fn draw_startup_notice(frame: &mut Frame, app: &App, body: Rect, top: u16) {
    let Some(Block::Notice(_)) = app.scrollback().blocks().last() else {
        return;
    };
    if top >= body.bottom() {
        return;
    }
    let ctx = app.block_ctx(body.width as usize);
    let lines = app
        .scrollback()
        .blocks()
        .last()
        .map(|block| block.output(&ctx).lines)
        .unwrap_or_default();
    let available = (body.bottom() - top) as usize;
    if lines.is_empty() || lines.len() > available {
        return;
    }
    let area = Rect::new(body.x, top, body.width, lines.len() as u16);
    render_lines(frame, area, lines.into_iter().map(|line| line.content));
}

/// 首屏菜单项 → 动作：只有真的绑了动作的项才返回（点得动的东西必须做得了事）。
fn menu_action(label: &str) -> Option<(&'static str, ActionId)> {
    /// 与 `startup::MENU` 的标签一一对应；新增菜单项时这里一起补。
    const MENU_ACTIONS: [(&str, ActionId); 2] = [
        ("研究视图", ActionId::ToggleResearch),
        ("退出", ActionId::Quit),
    ];
    MENU_ACTIONS
        .iter()
        .find(|(name, _)| name == &label)
        .map(|(name, action)| (*name, *action))
}

/// 画快捷键栏，并把每条提示登记成可点区：点一条提示 = 按那个键。
///
/// 提示的列范围由 [`ShortcutBar`] 在排版时一并给出，这里只做「列 → 屏幕坐标」的换算与
/// 「键位 → 动作」的查表——不重算一遍排版，点击与画面因此不会漂移。
fn draw_shortcut_bar(frame: &mut Frame, app: &mut App, area: Rect) {
    let bar_ctx = app.block_ctx(area.width as usize);
    let owner = app.key_owner();
    let bar = ShortcutBar::render(&bar_ctx, app.keymap(), owner);
    let targets: Vec<(Rect, ActionId)> = bar
        .chips
        .iter()
        .filter_map(|chip| {
            let action = app.keymap().lookup(owner, chip.chord)?;
            Some((
                Rect::new(
                    area.x + chip.start as u16,
                    area.y,
                    chip.width as u16,
                    area.height.min(1),
                ),
                action,
            ))
        })
        .collect();
    render_lines(frame, area, std::iter::once(bar.line.content));
    for (rect, action) in targets {
        app.register_click(rect, action);
    }
}

/// 提示框（三屏共用）：四边盒 + 终端光标。
fn draw_prompt(frame: &mut Frame, app: &mut App, area: Rect) {
    if area.height == 0 {
        return;
    }
    let ctx = app.block_ctx(area.width as usize);
    let focused = app.is_prompt_focused();
    // 幽灵文案里的键位取自注册表：键位可覆盖，文案也跟着走。
    // 粘贴那条尤其要在这儿露面——Ctrl+V 是终端的事，不写出来用户按了只会以为程序坏了。
    let hint_text = |id: ActionId, fallback: &str| {
        app.keymap()
            .hint_key_text(id)
            .unwrap_or_else(|| fallback.to_owned())
    };
    let hints = format!(
        "{} 换行 · {} 粘贴",
        hint_text(ActionId::InsertNewline, "Enter"),
        hint_text(ActionId::PasteHint, "Ctrl+Shift+V")
    );
    let lines = app
        .prompt()
        .render(&ctx, app.prompt_chrome(), focused, &hints);
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

/// 补全候选列表最多画几行（含「… 还有 N 条」那一行）。
const MAX_COMPLETION_ROWS: usize = 6;

/// 补全候选的可见窗口：`(第一个候选下标, 画几行候选项, 要不要画「还有 N 条」)`。
///
/// 候选不超过上限就全画；超过时末行让给「还有 N 条」，窗口跟着选中项滚动——
/// 保证高亮那一行永远在屏上（`Down` 到底时也不会把选中项滚出可视区）。
fn completion_window(len: usize, selected: usize, max_rows: usize) -> (usize, usize, bool) {
    if max_rows == 0 || len == 0 {
        return (0, 0, false);
    }
    if len <= max_rows {
        return (0, len, false);
    }
    let shown = max_rows - 1;
    let offset = (selected + 1).saturating_sub(shown).min(len - shown);
    (offset, shown, true)
}

/// 选项列表最多画几行（不含标题行）。
const MAX_CHOICE_ROWS: usize = 8;

/// 画命令的选项列表：压在提示框上方（与补全列表同一块地方）。
///
/// 首行是标题与键位提示，之后每行 `标记 + 值 + 说明`，选中行高亮；返回是否画了东西
/// （画了就不要再画补全列表，两者抢同一块地方）。
fn draw_choices(frame: &mut Frame, app: &mut App, area: Rect) -> bool {
    let Some(list) = app.choices() else {
        return false;
    };
    if area.height < 2 || list.entries.is_empty() {
        return false;
    }
    let max_rows = MAX_CHOICE_ROWS.min(area.height as usize - 1);
    let offset = (list.selected + 1)
        .saturating_sub(max_rows)
        .min(list.entries.len().saturating_sub(max_rows));
    let shown = list.entries.len().min(max_rows);
    let rows = shown + 1; // 标题行
    let rect = Rect::new(
        area.x,
        area.y + area.height - rows as u16,
        area.width,
        rows as u16,
    );
    let ctx = app.block_ctx(rect.width as usize);
    let theme = *ctx.theme;
    // 选中标记**不能**用提示框的 `❯`：列表就压在提示框上方，同一个标记会让人以为
    // 输入框被列表占了（可用性审计的实测）。工具块用的是 `▸`，这里跟它一致。
    let marker = ctx.glyphs.resolve(glyphs::triangle_right());
    let ellipsis = ctx.glyphs.resolve(glyphs::ellipsis());
    let value_column = list
        .entries
        .iter()
        .map(|entry| width::display_width(&entry.value))
        .max()
        .unwrap_or(0);

    let mut lines = vec![Line::from(clip_spans(
        vec![Span::styled(
            list.title.clone(),
            theme.accent(theme.accent_system),
        )],
        ctx.width,
        ellipsis,
    ))];
    for (row, entry) in list.entries[offset..offset + shown].iter().enumerate() {
        let index = offset + row;
        let padding =
            " ".repeat(value_column.saturating_sub(width::display_width(&entry.value)) + 2);
        let (head, head_style, value_style, label_style) = if index == list.selected {
            (
                format!("{} ", marker),
                theme
                    .accent(theme.accent_system)
                    .add_modifier(Modifier::BOLD),
                theme.fg().add_modifier(Modifier::BOLD),
                theme.fg(),
            )
        } else {
            (
                " ".repeat(width::display_width(marker) + 1),
                theme.dim(),
                theme.muted(),
                theme.dim(),
            )
        };
        lines.push(Line::from(clip_spans(
            vec![
                Span::styled(head, head_style),
                Span::styled(entry.value.clone(), value_style),
                Span::styled(padding, theme.dim()),
                Span::styled(entry.label.clone(), label_style),
            ],
            ctx.width,
            ellipsis,
        )));
    }
    render_lines(frame, rect, lines);
    true
}

/// 画 `/` 命令补全候选列表：压在回滚区最底下那几行（于是就在提示框上方，不新开布局区）。
///
/// 每行 `/name` + 一句说明；选中行用强调色 + 行首标记高亮，候选多于上限时末行写
/// 「… 还有 N 条」。研究视图没有提示框，不调用本函数。每一行都登记成可点区
/// （点击 = `Tab` 填入那一项；动作 ID 不带参数，"第几项"由 [`App::register_completion_hit`] 记）。
fn draw_completion(frame: &mut Frame, app: &mut App, area: Rect) {
    if area.height == 0 || !app.completion_visible() {
        return;
    }
    let Some(state) = app.completion() else {
        return;
    };
    let max_rows = MAX_COMPLETION_ROWS.min(area.height as usize);
    let (offset, shown, overflow) =
        completion_window(state.matches.len(), state.selected, max_rows);
    let rows = shown + usize::from(overflow);
    if rows == 0 {
        return;
    }
    let rect = Rect::new(
        area.x,
        area.y + area.height - rows as u16,
        area.width,
        rows as u16,
    );

    let ctx = app.block_ctx(rect.width as usize);
    let theme = *ctx.theme;
    // 选中标记**不能**用提示框的 `❯`：列表就压在提示框上方，同一个标记会让人以为
    // 输入框被列表占了（可用性审计的实测）。工具块用的是 `▸`，这里跟它一致。
    let marker = ctx.glyphs.resolve(glyphs::triangle_right());
    let ellipsis = ctx.glyphs.resolve(glyphs::ellipsis());
    // 名字列对齐到最宽的那条（所有候选一起算，滚窗口时列宽不跳）。
    let name_column = state
        .matches
        .iter()
        .map(|spec| width::display_width(&format!("/{}", spec.name)))
        .max()
        .unwrap_or(0);
    // 标记后留一列空格：`▸`（与回退档的 `>`）不带尾空格，得自己补。
    let marker_head = format!("{marker} ");
    let blank_head = " ".repeat(width::display_width(&marker_head));
    let mut lines = Vec::with_capacity(rows);
    let mut hits = Vec::with_capacity(shown);
    for (row, spec) in state.matches[offset..offset + shown].iter().enumerate() {
        let index = offset + row;
        let name = format!("/{}", spec.name);
        let padding = " ".repeat(name_column.saturating_sub(width::display_width(&name)) + 2);
        // 行首标记只给选中项：无色档下也能一眼看出选的是哪一行。
        let (marker_text, marker_style, name_style, summary_style) = if index == state.selected {
            (
                marker_head.clone(),
                theme.accent(theme.accent_system),
                theme.accent(theme.accent_system),
                theme.fg(),
            )
        } else {
            (blank_head.clone(), theme.dim(), theme.muted(), theme.dim())
        };
        lines.push(Line::from(clip_spans(
            vec![
                Span::styled(marker_text, marker_style),
                Span::styled(name, name_style),
                Span::styled(padding, Style::new()),
                Span::styled(spec.summary.to_owned(), summary_style),
            ],
            ctx.width,
            ellipsis,
        )));
        hits.push((Rect::new(rect.x, rect.y + row as u16, rect.width, 1), index));
    }
    if overflow {
        let hidden = state.matches.len() - shown;
        lines.push(Line::from(clip_spans(
            vec![
                Span::styled(blank_head.clone(), Style::new()),
                Span::styled(format!("{ellipsis} 还有 {hidden} 条"), theme.dim()),
            ],
            ctx.width,
            ellipsis,
        )));
    }

    render_lines(frame, rect, lines);
    for (row_area, index) in hits {
        app.register_click(row_area, ActionId::AcceptCompletion);
        app.register_completion_hit(row_area, index);
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

/// 给一行贴上选择高亮：命中的列区间加反显，其余原样。
///
/// 按**显示列**切 span（CJK 一格两列，切点必须落在字符边界上，所以用 `width::clip` 折算
/// 而不是按字节切），样式沿用原 span 的，只多一个 `REVERSED`。
fn highlight_selection(line: &Line<'static>, selection: &Selection, row: usize) -> Line<'static> {
    let (start, end) = selection.ordered();
    if row < start.0 || row > end.0 {
        return line.clone();
    }
    let from = if row == start.0 { start.1 } else { 0 };
    let to = if row == end.0 { end.1 } else { u16::MAX };
    let mut spans = Vec::with_capacity(line.spans.len() + 2);
    let mut column = 0u16;
    for span in &line.spans {
        let text = span.content.as_ref();
        let width = width::display_width(text) as u16;
        let span_start = column;
        let span_end = column.saturating_add(width).saturating_sub(1);
        column = column.saturating_add(width);
        if width == 0 {
            spans.push(span.clone());
            continue;
        }
        if span_end < from || (to != u16::MAX && span_start > to) {
            spans.push(span.clone());
            continue;
        }
        let head_at = from.saturating_sub(span_start).min(width);
        let (head, rest) = split_columns(text, head_at);
        let within = from.max(span_start);
        let tail_at = to.saturating_sub(within).saturating_add(1).min(width);
        let (middle, tail) = split_columns(rest, tail_at);
        if !head.is_empty() {
            spans.push(Span::styled(head.to_owned(), span.style));
        }
        if !middle.is_empty() {
            spans.push(Span::styled(
                middle.to_owned(),
                span.style.add_modifier(Modifier::REVERSED),
            ));
        }
        if !tail.is_empty() {
            spans.push(Span::styled(tail.to_owned(), span.style));
        }
    }
    Line::from(spans)
}

/// 按显示列把一段文本切成两半：前 `columns` 列与剩下部分（切点落在字符边界）。
fn split_columns(text: &str, columns: u16) -> (&str, &str) {
    if columns == u16::MAX {
        return (text, "");
    }
    let head = width::clip(text, columns as usize);
    (head, &text[head.len()..])
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

    #[test]
    fn the_completion_window_keeps_the_selection_on_screen() {
        // 8 条候选、上限 6 行：5 行候选 + 1 行「还有 N 条」，窗口跟着选中项滚。
        assert_eq!(completion_window(8, 0, 6), (0, 5, true));
        assert_eq!(completion_window(8, 4, 6), (0, 5, true));
        assert_eq!(completion_window(8, 5, 6), (1, 5, true));
        assert_eq!(completion_window(8, 7, 6), (3, 5, true));
        // 装得下就全画，没有溢出提示。
        assert_eq!(completion_window(3, 2, 6), (0, 3, false));
        assert_eq!(completion_window(0, 0, 6), (0, 0, false));
        assert_eq!(completion_window(8, 0, 0), (0, 0, false));
    }
}
