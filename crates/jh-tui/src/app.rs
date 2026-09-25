//! 应用状态 + 纯调度：键盘 → 动作 → 状态变化，副作用一律描述成 [`Effect`] 值。
//!
//! 设计来源：`docs/grok-tui-design.md` §7.1 第 9 条（决策层与 IO 层分离，dispatch 不碰
//! 终端/网络/文件系统，上游把它写成不变量：`"dispatch fully testable without tokio or a
//! terminal"`，`app/dispatch/mod.rs:1-10`）与 §7.3 的 `app.rs` 职责。
//!
//! 于是：本模块只依赖 [`crate::actions`]、[`crate::blocks`]、[`crate::components`] 与
//! [`crate::render::RenderGate`]，不出现任何 IO 调用；宿主拿 [`Effect`] 去干活。

use crossterm::event::{Event, KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use ratatui::layout::{Position, Rect};

use crate::actions::{ActionId, KeyChord, KeyOwner, Keymap};
use crate::blocks::{Block, BlockCtx, DecisionTrace, Selectable, ToolStatus};
use crate::commands::{self, CommandSpec, Parsed};
use crate::components::{
    ActiveModal, ApprovalRequest, HeaderModel, PanelState, Prompt, PromptChrome, Scrollback,
    StatusModel, TurnPhase, WaitingReason,
};
use crate::glyphs::Glyphs;
use crate::panels::{PanelId, Panels};
use crate::render::{RenderGate, TickDemand, animation_frame};
use crate::screens::Screen;
use crate::terminal::Capabilities;
use crate::theme::Theme;
use crate::width;

/// 一次按键可能产出的副作用：宿主执行，调度层不认识终端与网络。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// 退出。
    Quit,
    /// 取消当前轮（无在途轮次时等价于退出）。
    Interrupt,
    /// 提交提示框内容。
    Submit(String),
    /// 用户对审批卡作出裁决。
    Approval(ApprovalOutcome),
    /// 研究视图按了单步：宿主推进一步（本 crate 不接线）。
    Step,
    /// 研究视图切换了暂停：宿主按这个值停/继续（本 crate 不接线）。
    SetPaused(bool),
    /// 会话命令（`/model`、`/permission`、`/status`）：参数与会话状态都在宿主那边。
    Command(CommandCall),
    /// 在选项列表里按了 `Enter`：宿主用选中的值执行该命令。
    Choice {
        command: &'static str,
        value: String,
    },
    /// 拖选结束：把这段文本交给宿主写进剪贴板（本 crate 不直接碰终端）。
    Copy { text: String, rows: usize },
}

/// 一条要宿主执行的斜杠命令。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandCall {
    /// 命令名（[`crate::commands::COMMANDS`] 里的规范写法，不含 `/`）。
    pub name: &'static str,
    /// 命令名之后的原文（可能为空；整段原样交给宿主解释）。
    pub args: String,
}

/// 审批裁决。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalOutcome {
    AllowOnce,
    AllowAlways,
    Deny,
}

/// `/` 命令补全列表的状态：查询串 + 候选 + 选中项。
///
/// 「展开」与「可见」是两件事：`App::completion` 为 `Some` 表示输入框里正在敲命令名
/// （触发条件成立），候选可能为空（`/zzz`）；[`CompletionState::is_visible`] 为 true 时
/// 才画列表、才接管 `Up/Down/Tab/Esc`——没有候选时按键落回原来的语义。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionState {
    /// `/` 之后、第一个空白之前的那段原文（不含 `/`）。
    pub query: String,
    /// 候选，已按 [`crate::commands::candidates`] 的序排好。
    pub matches: Vec<&'static CommandSpec>,
    /// 选中项在 `matches` 里的下标。
    pub selected: usize,
}

impl CompletionState {
    /// 有候选才算可见（空候选不画列表、不接键、不抢 `Esc`）。
    pub fn is_visible(&self) -> bool {
        !self.matches.is_empty()
    }
}

/// 一次文本选择：两个端点都记在**摊平后的内容行号 + 列**上。
///
/// 用内容坐标而不是屏幕坐标：滚轮翻页、后面又有新块追加时，选择跟着内容走，不会错位。
/// `column` 是回滚区内的屏幕列（含块标记列），取文本时会按每行的 `Selectable` 裁掉标记列。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: (usize, u16),
    pub focus: (usize, u16),
}

impl Selection {
    /// 起止两个端点，保证 `start <= end`（按行、再按列比较）。
    pub fn ordered(&self) -> ((usize, u16), (usize, u16)) {
        if self.anchor <= self.focus {
            (self.anchor, self.focus)
        } else {
            (self.focus, self.anchor)
        }
    }

    /// 命中测试：线性（流式）选择——首行从起点列到行尾、中间整行、末行从头到终点列。
    pub fn covers(&self, row: usize, column: u16) -> bool {
        let (start, end) = self.ordered();
        if row < start.0 || row > end.0 {
            return false;
        }
        if row == start.0 && column < start.1 {
            return false;
        }
        if row == end.0 && column > end.1 {
            return false;
        }
        true
    }

    /// 只点了一下、没拖动（起止同格）：不算选择。
    fn is_click(&self) -> bool {
        self.anchor == self.focus
    }
}

/// 拖动中的落点：用来区分「点击」（执行命中动作）与「拖选」。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DragState {
    /// 按下的那一格（内容坐标）。
    origin: (usize, u16),
    /// 是否已经拖出原位（拖过就不当点击）。
    moved: bool,
}

/// 选项列表里的一条：交给命令的值 + 一行说明。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceEntry {
    /// 选中后作为命令参数的那个值（如 `auto`、`deepseek-v4-pro`、会话 ID）。
    pub value: String,
    /// 一行说明（可在里面标出「当前」）。
    pub label: String,
}

/// 选项列表：给「取值固定」的命令（`/resume`、`/permission`、`/model` …）用。
///
/// 开着时 `↑↓` 选、`Enter` 用选中的值执行该命令、`Esc` 关。宿主给数据（值与说明），
/// 界面只管交互——于是任何命令只要有一组固定取值，就能复用这一套。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceList {
    /// `Enter` 时要执行的命令名。
    pub command: &'static str,
    /// 标题行（含键位提示）。
    pub title: String,
    pub entries: Vec<ChoiceEntry>,
    pub selected: usize,
}

impl ChoiceList {
    /// 选中项的值。
    pub fn selected_value(&self) -> Option<&str> {
        self.entries
            .get(self.selected)
            .map(|entry| entry.value.as_str())
    }

    fn step(&mut self, delta: isize) {
        if self.entries.is_empty() {
            return;
        }
        let last = (self.entries.len() - 1) as isize;
        self.selected = (self.selected as isize + delta).clamp(0, last) as usize;
    }
}

impl ApprovalOutcome {
    pub fn action(self) -> ActionId {
        match self {
            ApprovalOutcome::AllowOnce => ActionId::ApproveOnce,
            ApprovalOutcome::AllowAlways => ActionId::ApproveAlways,
            ApprovalOutcome::Deny => ActionId::Deny,
        }
    }
}

/// 应用状态。所有字段都是纯数据，渲染与调度都从它出发。
#[derive(Debug)]
pub struct App {
    theme: Theme,
    glyphs: Glyphs,
    keymap: Keymap,
    scrollback: Scrollback,
    prompt: Prompt,
    /// 提示框的 chrome（标题与底边信息行）；宿主用 [`App::set_prompt_chrome`] 换成真值。
    prompt_chrome: PromptChrome,
    status: StatusModel,
    /// 顶栏的左半：工作目录与用量；右侧徽章由屏幕决定。
    header: HeaderModel,
    /// 首屏右下角的注解（判定器与接线状态）。
    startup_note: Option<String>,
    modal: Option<ActiveModal>,
    decision: Option<DecisionTrace>,
    screen: Screen,
    panels: Panels,
    panel: PanelId,
    panel_state: PanelState,
    /// 当前是否暂停（研究视图；真实运行状态在宿主那里）。
    paused: bool,
    tick: u64,
    gate: RenderGate,
    should_quit: bool,
    /// 上一次布局算出的回滚区宽度（滚动上限要用）。
    scrollback_width: usize,
    /// 上一次布局算出的回滚区高度（翻页步长要用）。
    scrollback_height: usize,
    /// 上一次布局算出的提示框内容宽度（纵向移动要用）。
    prompt_width: usize,
    /// 上一次布局算出的面板带宽（内容行数折算要用）。
    panel_width: usize,
    /// 上一次布局算出的面板内高（滚动上限要用）。
    panel_height: usize,
    /// 本帧登记的可点区域；渲染开始时重建，鼠标点击按它命中。
    click_targets: Vec<ClickTarget>,
    /// 命令的选项列表（`/resume`、`/permission`、`/model` …；宿主给数据，界面管交互）。
    choices: Option<ChoiceList>,
    /// 当前选中的文本（拖选出来的；`Esc` 或点别处清掉）。
    selection: Option<Selection>,
    /// 左键按下但还没松手（区分点击与拖选）。
    drag: Option<DragState>,
    /// 回滚区在屏上的位置：鼠标坐标要换算成内容坐标。
    scrollback_origin: (u16, u16),
    /// `/` 命令补全：`None` = 没展开（触发条件不满足，或被 `Esc` 收起）。
    completion: Option<CompletionState>,
    /// `Esc` 收起补全时记下的整段输入：文本没再变就保持收起，变了就重新展开。
    completion_dismissed: Option<String>,
    /// 本帧补全候选项的命中区（行矩形 → 候选下标）：动作 ID 不带参数，点的是第几项
    /// 只能由渲染期登记的这张表回答。渲染开始时重建。
    completion_hits: Vec<(Rect, usize)>,
}

/// 一个可点区域：矩形（屏幕坐标）+ 命中后执行的动作。
///
/// 登记发生在渲染期（组件知道自己的布局），点击发生在事件期；"点了什么"因此永远与"画了什么"
/// 一致，不需要在按键处理里重算一遍布局。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClickTarget {
    pub area: Rect,
    pub action: ActionId,
}

impl App {
    /// 从能力位建应用：主题与字形都跟着能力位走，业务代码不读环境变量。
    pub fn new(capabilities: &Capabilities) -> Self {
        App {
            theme: capabilities.theme(),
            glyphs: capabilities.glyphs,
            keymap: Keymap::from_defaults(),
            scrollback: Scrollback::new(),
            prompt: Prompt::new(),
            prompt_chrome: PromptChrome::unwired(),
            status: StatusModel::idle(),
            header: HeaderModel::new("jev-harness"),
            startup_note: None,
            modal: None,
            decision: None,
            screen: Screen::Startup,
            panels: Panels::default(),
            panel: PanelId::Judgment,
            panel_state: PanelState::default(),
            paused: false,
            tick: 0,
            gate: RenderGate::new(),
            should_quit: false,
            scrollback_width: 80,
            scrollback_height: 10,
            prompt_width: 76,
            panel_width: 80,
            panel_height: 10,
            click_targets: Vec::new(),
            choices: None,
            selection: None,
            drag: None,
            scrollback_origin: (0, 0),
            completion: None,
            completion_dismissed: None,
            completion_hits: Vec::new(),
        }
    }

    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    pub fn glyphs(&self) -> Glyphs {
        self.glyphs
    }

    pub fn keymap(&self) -> &Keymap {
        &self.keymap
    }

    /// 用配置覆盖键位（单一注册表的逃生口）。
    pub fn set_keymap(&mut self, keymap: Keymap) {
        self.keymap = keymap;
        self.gate.mark_dirty();
    }

    pub fn keymap_mut(&mut self) -> &mut Keymap {
        &mut self.keymap
    }

    pub fn scrollback(&self) -> &Scrollback {
        &self.scrollback
    }

    pub fn scrollback_mut(&mut self) -> &mut Scrollback {
        &mut self.scrollback
    }

    pub fn prompt(&self) -> &Prompt {
        &self.prompt
    }

    pub fn prompt_mut(&mut self) -> &mut Prompt {
        &mut self.prompt
    }

    /// 当前补全状态（没展开就是 `None`）。
    pub fn completion(&self) -> Option<&CompletionState> {
        self.completion.as_ref()
    }

    /// 补全列表当前可见：`Up/Down/Tab/Esc` 只有这时才归它。
    pub fn completion_visible(&self) -> bool {
        self.completion
            .as_ref()
            .is_some_and(CompletionState::is_visible)
    }

    pub fn status(&self) -> &StatusModel {
        &self.status
    }

    pub fn status_mut(&mut self) -> &mut StatusModel {
        &mut self.status
    }

    pub fn prompt_chrome(&self) -> &PromptChrome {
        &self.prompt_chrome
    }

    /// 换提示框的 chrome（模型、判定器、授权模式的真值由宿主给）。
    pub fn set_prompt_chrome(&mut self, chrome: PromptChrome) {
        self.prompt_chrome = chrome;
        self.gate.mark_dirty();
    }

    /// 顶栏数据：左半来自宿主，右侧徽章跟着屏幕走（会话 ↔ 研究视图的入口说明）。
    pub fn header(&self) -> HeaderModel {
        let badge = match self.screen {
            Screen::Agent => Some("[研究视图]".to_owned()),
            Screen::Research => Some("[会话]".to_owned()),
            Screen::Startup => None,
        };
        HeaderModel {
            badge,
            ..self.header.clone()
        }
    }

    pub fn header_model_mut(&mut self) -> &mut HeaderModel {
        &mut self.header
    }

    pub fn startup_note(&self) -> Option<&str> {
        self.startup_note.as_deref()
    }

    /// 首屏右下角的注解（例如 `判定器 rule · 未接线`）。
    pub fn set_startup_note(&mut self, note: impl Into<String>) {
        self.startup_note = Some(note.into());
        self.gate.mark_dirty();
    }

    pub fn screen(&self) -> Screen {
        self.screen
    }

    pub fn set_screen(&mut self, screen: Screen) {
        self.clear_click_targets();
        self.screen = screen;
        // 研究视图没有提示框：切过去时补全自然收起；切回来又满足条件就重新展开。
        self.refresh_completion();
        self.gate.mark_dirty();
    }

    pub fn panels(&self) -> &Panels {
        &self.panels
    }

    /// 喂面板数据（本批由预览样例给，接线后由适配层给）。
    pub fn panels_mut(&mut self) -> &mut Panels {
        &mut self.panels
    }

    pub fn panel(&self) -> PanelId {
        self.panel
    }

    pub fn panel_state(&self) -> &PanelState {
        &self.panel_state
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    pub fn modal(&self) -> Option<&ActiveModal> {
        self.modal.as_ref()
    }

    /// 当前轮次的判定轨迹（状态行据此把概率画到屏上）。
    pub fn decision(&self) -> Option<&DecisionTrace> {
        self.decision.as_ref()
    }

    pub fn tick(&self) -> u64 {
        self.tick
    }

    pub fn gate(&self) -> &RenderGate {
        &self.gate
    }

    pub fn gate_mut(&mut self) -> &mut RenderGate {
        &mut self.gate
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    /// 渲染上下文（主题 / 字形 / 动画帧号 / 本轮判定）。
    pub fn block_ctx(&self, width: usize) -> BlockCtx<'_> {
        BlockCtx {
            width,
            theme: &self.theme,
            glyphs: self.glyphs,
            frame: animation_frame(self.tick),
            decision: self.decision.as_ref(),
        }
    }

    /// 这一拍键盘归谁：有模态卡就是卡的（权限键优先）；研究视图归面板；其余归提示框。
    pub fn key_owner(&self) -> KeyOwner {
        if self.modal.is_some() {
            KeyOwner::Approval
        } else if self.screen == Screen::Research {
            KeyOwner::Research
        } else {
            KeyOwner::Prompt
        }
    }

    pub fn is_prompt_focused(&self) -> bool {
        self.key_owner() == KeyOwner::Prompt
    }

    /// 布局回写：渲染阶段算出视口尺寸，按键处理据此决定翻页步长与滚动上限。
    pub fn set_viewport(
        &mut self,
        scrollback_width: usize,
        scrollback_height: usize,
        prompt_width: usize,
    ) {
        self.scrollback_width = scrollback_width;
        self.scrollback_height = scrollback_height;
        self.prompt_width = prompt_width;
    }

    /// 回滚区在屏上的原点（鼠标坐标 → 内容坐标要用）。
    pub fn set_scrollback_origin(&mut self, x: u16, y: u16) {
        self.scrollback_origin = (x, y);
    }

    /// 面板视口的回写（宽度与内高，用于折算内容行数与滚动上限）。
    pub fn set_panel_viewport(&mut self, width: usize, height: usize) {
        self.panel_width = width;
        self.panel_height = height;
    }

    /// 清空上一帧登记的可点区域与补全命中区（渲染开始时调；帧内重建）。
    pub fn clear_click_targets(&mut self) {
        self.click_targets.clear();
        self.completion_hits.clear();
    }

    /// 登记一个可点区域（渲染期调）。
    pub fn register_click(&mut self, area: Rect, action: ActionId) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        self.click_targets.push(ClickTarget { area, action });
    }

    /// 登记一条补全候选项的命中区（渲染期调；`index` 是候选在 `matches` 里的下标）。
    ///
    /// 候选项的点击动作都是 [`ActionId::AcceptCompletion`]（动作 ID 不带参数），
    /// 「点的是第几项」由这张表回答。
    pub fn register_completion_hit(&mut self, area: Rect, index: usize) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        self.completion_hits.push((area, index));
    }

    /// 命中本帧的补全候选项：返回候选下标。
    fn completion_hit(&self, column: u16, row: u16) -> Option<usize> {
        let position = Position::new(column, row);
        self.completion_hits
            .iter()
            .find(|(area, _)| area.contains(position))
            .map(|(_, index)| *index)
    }

    /// 打开命令的选项列表（宿主把取值与说明递进来）。
    pub fn open_choices(
        &mut self,
        command: &'static str,
        title: impl Into<String>,
        entries: Vec<ChoiceEntry>,
    ) {
        self.choices = Some(ChoiceList {
            command,
            title: title.into(),
            entries,
            selected: 0,
        });
        self.gate.mark_dirty();
    }

    /// 选项列表状态（渲染与测试读它）。
    pub fn choices(&self) -> Option<&ChoiceList> {
        self.choices.as_ref()
    }

    /// 关掉选项列表（`Esc`）。
    pub fn close_choices(&mut self) {
        self.choices = None;
        self.gate.mark_dirty();
    }

    /// 屏幕坐标 → 内容坐标（摊平行号 + 回滚区内列号）；不在回滚区里返回 `None`。
    fn scrollback_cell(&self, column: u16, row: u16) -> Option<(usize, u16)> {
        let (origin_x, origin_y) = self.scrollback_origin;
        let width = self.scrollback_width as u16;
        let height = self.scrollback_height as u16;
        if width == 0 || height == 0 {
            return None;
        }
        if column < origin_x
            || column >= origin_x + width
            || row < origin_y
            || row >= origin_y + height
        {
            return None;
        }
        let ctx = self.block_ctx(self.scrollback_width);
        // 只问几何，不把窗口那几十行拷出来：拖动时每个鼠标事件都要走这里。
        let (_, start, len) = self.scrollback.geometry(&ctx, self.scrollback_height);
        let window_row = (row - origin_y) as usize;
        if window_row >= len {
            // 窗口没填满时下面的空白格：不算内容。
            return None;
        }
        Some((start + window_row, column - origin_x))
    }

    /// 同 [`App::scrollback_cell`]，但把区域外的坐标夹到最近一格（拖出边界时选择跟着到边）。
    fn scrollback_cell_clamped(&self, column: u16, row: u16) -> Option<(usize, u16)> {
        let (origin_x, origin_y) = self.scrollback_origin;
        let width = self.scrollback_width as u16;
        let height = self.scrollback_height as u16;
        if width == 0 || height == 0 {
            return None;
        }
        let column = column.clamp(origin_x, origin_x + width - 1);
        let row = row.clamp(origin_y, origin_y + height - 1);
        self.scrollback_cell(column, row)
    }

    /// 开始拖选（左键按下）。
    pub fn begin_selection(&mut self, cell: (usize, u16)) {
        self.selection = Some(Selection {
            anchor: cell,
            focus: cell,
        });
        self.drag = Some(DragState {
            origin: cell,
            moved: false,
        });
        self.gate.mark_dirty();
    }

    /// 拖动中（左键按住移动）。
    pub fn extend_selection(&mut self, cell: (usize, u16)) {
        let Some(drag) = self.drag.as_mut() else {
            return;
        };
        drag.moved = true;
        if let Some(selection) = self.selection.as_mut() {
            selection.focus = cell;
        }
        self.gate.mark_dirty();
    }

    /// 当前选择（渲染高亮用）。
    pub fn selection(&self) -> Option<Selection> {
        self.selection.filter(|selection| !selection.is_click())
    }

    /// 清掉选择与拖动状态。
    pub fn clear_selection(&mut self) {
        if self.selection.is_some() || self.drag.is_some() {
            self.selection = None;
            self.drag = None;
            self.gate.mark_dirty();
        }
    }

    /// 结束拖动：拖出过内容就给出一段可复制的文本，否则当作普通点击（返回 `None`）。
    pub fn finish_selection(&mut self) -> Option<(String, usize)> {
        let dragged = self.drag.take().is_some_and(|drag| drag.moved);
        let selection = self.selection.filter(|selection| !selection.is_click());
        if !dragged {
            self.selection = None;
            return None;
        }
        let text = selection.and_then(|selection| self.selected_text(selection))?;
        Some(text)
    }

    /// 把选择区取成文本：逐行按 `Selectable` 裁掉块标记列，再按列区间取。
    ///
    /// 软换行的续行用 `joiner` 拼回上一行（不落换行符）——与 `BlockOutput::plain_text` 同一口径。
    fn selected_text(&self, selection: Selection) -> Option<(String, usize)> {
        let (start, end) = selection.ordered();
        let ctx = self.block_ctx(self.scrollback_width);
        let lines = self.scrollback.layout(&ctx);
        let mut out = String::new();
        let mut rows = 0usize;
        let last = end.0.min(lines.len().saturating_sub(1));
        for (row, line) in lines.iter().enumerate().take(last + 1).skip(start.0) {
            let Some(selectable) = line.selection_text() else {
                // 不可选的行（块间空行、纯装饰行）：只在选择跨过去时落一个换行。
                continue;
            };
            let selectable_start = match &line.selectable {
                Selectable::Spans(range) => range.start as usize,
                _ => 0,
            };
            let (from, to) = if row == start.0 && row == end.0 {
                (start.1, end.1)
            } else if row == start.0 {
                (start.1, u16::MAX)
            } else if row == end.0 {
                (0, end.1)
            } else {
                (0, u16::MAX)
            };
            let from = (from as usize).max(selectable_start);
            let to = to as usize;
            if to < selectable_start {
                continue;
            }
            // 屏幕列 → 文本列：先跳过标记列，再按显示宽度取区间。
            let head = width::clip(&selectable, from.saturating_sub(selectable_start)).len();
            let tail = &selectable[head..];
            let take = to.saturating_sub(from.max(selectable_start));
            // `to` 是闭区间，所以取 `take + 1` 列；超宽时 `clip` 自然返回整段。
            let piece = width::clip(tail, take.saturating_add(1)).to_owned();
            if piece.is_empty() && !out.is_empty() {
                continue;
            }
            if !out.is_empty() {
                match &line.joiner {
                    Some(joiner) => out.push_str(joiner),
                    None => out.push('\n'),
                }
            }
            out.push_str(piece.trim_end());
            rows += 1;
        }
        let text = out.trim_end().to_owned();
        if text.trim().is_empty() {
            return None;
        }
        Some((text, rows))
    }

    /// 命中查询：坐标落在哪个已登记区域里；重叠时取**后登记**的那个（上层压下层）。
    pub fn click_target(&self, column: u16, row: u16) -> Option<ActionId> {
        self.click_targets
            .iter()
            .rev()
            .find(|target| {
                let area = target.area;
                column >= area.x && column < area.right() && row >= area.y && row < area.bottom()
            })
            .map(|target| target.action)
    }

    /// 追加一个块；翻上去看历史时把视口锚在原地（否则新输出会把视口往上拽）。
    pub fn push_block(&mut self, block: Block) {
        let rows_before = if self.scrollback.is_at_bottom() {
            None
        } else {
            let ctx = self.block_ctx(self.scrollback_width);
            Some(self.scrollback.total_rows(&ctx))
        };
        self.scrollback.push(block);
        if let Some(rows_before) = rows_before {
            let ctx = self.block_ctx(self.scrollback_width);
            let added = self.scrollback.total_rows(&ctx).saturating_sub(rows_before);
            self.scrollback.scroll_up(added, usize::MAX);
        }
        self.gate.mark_dirty();
    }

    /// 更新本轮判定轨迹。
    pub fn set_decision(&mut self, decision: Option<DecisionTrace>) {
        self.decision = decision;
        self.gate.mark_dirty();
    }

    /// 流式追加助手文本：末尾是流式助手块就接着写，否则新起一块。
    pub fn stream_assistant(&mut self, delta: &str) {
        if self.trailing_streams() {
            self.edit_trailing(|block| {
                if let Block::Assistant(assistant) = block {
                    assistant.text.push_str(delta);
                }
            });
            return;
        }
        self.push_block(Block::streaming_assistant(delta));
    }

    /// 收尾流式助手块：清掉「还在长」的标记，块本身留在屏上。
    ///
    /// 返回末尾是不是流式助手块（false 表示这次收尾没有落到任何块上）；宿主据此
    /// 决定要不要把正文另外压一块，而不是静默丢掉。
    pub fn finish_assistant(&mut self) -> bool {
        let streaming = self.trailing_streams();
        if streaming {
            self.edit_trailing(|block| {
                if let Block::Assistant(assistant) = block {
                    assistant.streaming = false;
                }
            });
        }
        streaming
    }

    /// 起一个工具块（执行中）；结果回来后用 [`App::finish_tool`] 回填。
    pub fn begin_tool(&mut self, name: impl Into<String>, args: impl Into<String>) {
        self.push_block(Block::tool(name, args, ToolStatus::Running));
    }

    /// 回填末尾工具块：状态与结果摘要。
    ///
    /// 返回末尾是不是工具块；false 表示没有可回填的块（宿主据此如实上屏结果，
    /// 免得工具块永远转圈、结果摘要无声丢掉）。
    pub fn finish_tool(&mut self, status: ToolStatus, detail: Option<String>) -> bool {
        let is_tool = matches!(self.scrollback.blocks().last(), Some(Block::Tool(_)));
        if is_tool {
            self.edit_trailing(|block| {
                if let Block::Tool(tool) = block {
                    tool.status = status;
                    tool.detail = detail;
                }
            });
        }
        is_tool
    }

    fn trailing_streams(&self) -> bool {
        matches!(self.scrollback.blocks().last(), Some(Block::Assistant(block)) if block.streaming)
    }

    /// 就地改末尾块（流式追加与工具回填共用）：翻上去看历史时把视口锚在原地。
    fn edit_trailing(&mut self, edit: impl FnOnce(&mut Block)) {
        let rows_before = if self.scrollback.is_at_bottom() {
            None
        } else {
            let ctx = self.block_ctx(self.scrollback_width);
            Some(self.scrollback.total_rows(&ctx))
        };
        if let Some(block) = self.scrollback.last_mut() {
            edit(block);
        }
        if let Some(rows_before) = rows_before {
            let ctx = self.block_ctx(self.scrollback_width);
            let added = self.scrollback.total_rows(&ctx).saturating_sub(rows_before);
            self.scrollback.scroll_up(added, usize::MAX);
        }
        self.gate.mark_dirty();
    }

    /// 打开审批卡：单槽位，重复调用就是换一张卡（没有浮层栈）。
    pub fn open_approval(&mut self, request: ApprovalRequest) {
        self.clear_click_targets();
        self.modal = Some(ActiveModal::Approval(request));
        self.status = StatusModel::running(WaitingReason::Approval).with_model(
            self.status
                .model
                .clone()
                .unwrap_or_else(|| "unknown".to_owned()),
        );
        // 卡片持有键盘：补全收起（卡片关掉后条件恢复，会自己再展开）。
        self.refresh_completion();
        self.gate.mark_dirty();
    }

    /// 关掉审批卡并恢复状态行。
    pub fn close_approval(&mut self) {
        self.clear_click_targets();
        self.modal = None;
        self.refresh_completion();
        self.gate.mark_dirty();
    }

    /// 推进一步动画 tick：只有真的在动时才标 dirty（空闲不重绘）。
    pub fn advance_tick(&mut self) {
        self.tick += 1;
        let demand = self.tick_demand();
        self.gate.set_demand(demand);
        if demand != TickDemand::None {
            self.gate.mark_dirty();
        }
    }

    /// 当前需要的唤醒档位。
    pub fn tick_demand(&self) -> TickDemand {
        match self.status.waiting {
            Some(reason) if reason.animates() => TickDemand::Fast,
            Some(_) => TickDemand::Slow,
            None if self.status.phase == Some(TurnPhase::Running) => TickDemand::Slow,
            None => TickDemand::None,
        }
    }

    /// 处理一次终端事件（仍是纯的：不读键盘、不画屏，只改状态并返回副作用）。
    pub fn handle_event(&mut self, event: Event) -> Option<Effect> {
        match event {
            Event::Key(key) => {
                let chord = KeyChord::from_event(key)?;
                self.handle_key(chord)
            }
            Event::Paste(text) => {
                if self.is_prompt_focused() {
                    self.prompt.insert_str(&text);
                    // 粘贴同样是一次文本变化：候选跟着重算，选中归零。
                    self.refresh_completion();
                    self.gate.mark_dirty();
                }
                None
            }
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            Event::Resize(_, _) => {
                self.gate.mark_dirty();
                None
            }
            _ => None,
        }
    }

    /// 处理鼠标：左键点击 = 命中区域登记的那个动作（与快捷键走同一条 `apply` 通路），
    /// 滚轮 = 上/下翻页（研究视图翻面板带，其余翻回滚区——路由复用 `apply(Scroll*)`）。
    ///
    /// 左键在回滚区**空白处**按下并拖动 = 选文本；松手时把选中的文本交给宿主写剪贴板
    /// （压在选择层上面的审批卡 / 补全行 / 首屏菜单仍是点击，不参与拖选）。
    ///
    /// 快捷键本身**不下线**：滚轮只是让 PgUp/PgDn 不再是唯一选择。
    pub fn handle_mouse(&mut self, mouse: MouseEvent) -> Option<Effect> {
        self.refresh_completion();
        match mouse.kind {
            MouseEventKind::ScrollUp => self.apply(ActionId::ScrollPageUp),
            MouseEventKind::ScrollDown => self.apply(ActionId::ScrollPageDown),
            MouseEventKind::Down(MouseButton::Left) => {
                // 命中登记区域（审批卡、补全行、首屏菜单）就照旧执行动作——它们压在选择层上面。
                if let Some(action) = self.click_target(mouse.column, mouse.row) {
                    // 补全行：点哪一项就选中哪一项，随后的 `apply` 与按 Tab 是同一条通路。
                    if action == ActionId::AcceptCompletion
                        && let Some(index) = self.completion_hit(mouse.column, mouse.row)
                        && let Some(state) = self.completion.as_mut()
                    {
                        state.selected = index;
                    }
                    return self.apply(action);
                }
                // 空白处：起选（松手时再决定是「点了一下」还是「拖选」）。
                if let Some(cell) = self.scrollback_cell(mouse.column, mouse.row) {
                    self.begin_selection(cell);
                } else {
                    self.clear_selection();
                }
                None
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if let Some(cell) = self.scrollback_cell_clamped(mouse.column, mouse.row) {
                    self.extend_selection(cell);
                }
                None
            }
            MouseEventKind::Up(MouseButton::Left) => self
                .finish_selection()
                .map(|(text, rows)| Effect::Copy { text, rows }),
            _ => None,
        }
    }

    /// 这一拍 `Enter` 该不该**采纳高亮的候选**（而不是提交整行）。
    ///
    /// 三个条件缺一不可：
    /// 1. 补全列表开着（没候选就不谈采纳）；
    /// 2. 输入框里只有一个裸查询（`/` 或 `/pm`，还没进参数区）——已经带参数的整行是在提交；
    /// 3. 输入**不等于**选中候选的完整名字——`/quit` 打全了还要求按两次回车是反直觉的，
    ///    这时按一次就该提交。
    ///
    /// 于是 `/` 或 `/pm` + `Enter` 先补全成一个完整命令（列表里明明高亮着一项，按回车却提交
    /// 半截命令并报「未知命令」，是最容易踩的一处反直觉）；`/quit` + `Enter` 直接执行。
    fn enter_accepts_completion(&self) -> bool {
        let Some(state) = self.completion.as_ref().filter(|state| state.is_visible()) else {
            return false;
        };
        let text = self.prompt.text();
        if text != format!("/{}", state.query) {
            return false;
        }
        !state
            .matches
            .get(state.selected)
            .is_some_and(|spec| text == format!("/{}", spec.name))
    }

    /// 处理一次按键：先查动作注册表；没命中且提示框持有键盘时，普通字符就是文本输入。
    pub fn handle_key(&mut self, chord: KeyChord) -> Option<Effect> {
        // 选项列表开着时先归它：`Up/Down` 选、`Enter` 用选中的值执行、`Esc` 关。
        // 其余按键照常走下面的路径（用户还能继续打字，不必先关列表）。
        if self.choices.is_some() {
            match chord {
                KeyChord {
                    code: KeyCode::Up,
                    modifiers: KeyModifiers::NONE,
                } => {
                    if let Some(list) = self.choices.as_mut() {
                        list.step(-1);
                    }
                    self.gate.mark_dirty();
                    return None;
                }
                KeyChord {
                    code: KeyCode::Down,
                    modifiers: KeyModifiers::NONE,
                } => {
                    if let Some(list) = self.choices.as_mut() {
                        list.step(1);
                    }
                    self.gate.mark_dirty();
                    return None;
                }
                KeyChord {
                    code: KeyCode::Enter,
                    modifiers: KeyModifiers::NONE,
                } => {
                    let picked = self.choices.as_ref().and_then(|list| {
                        list.selected_value()
                            .map(|value| (list.command, value.to_owned()))
                    });
                    self.choices = None;
                    self.gate.mark_dirty();
                    return picked.map(|(command, value)| Effect::Choice { command, value });
                }
                KeyChord {
                    code: KeyCode::Esc,
                    modifiers: KeyModifiers::NONE,
                } => {
                    self.close_choices();
                    return None;
                }
                _ => {}
            }
        }
        // 宿主可能直接改写提示框文本（`prompt_mut`），按键前先对一次账。
        self.refresh_completion();
        // 有选中文本时，`Esc` 先清选择——取消这一轮更重要，别让一次误触顺手取消掉。
        let escape = KeyChord {
            code: KeyCode::Esc,
            modifiers: KeyModifiers::NONE,
        };
        if chord == escape && self.selection.is_some() {
            self.clear_selection();
            return None;
        }
        let owner = self.key_owner();
        // 补全列表开着且输入框只有一个裸查询时，`Enter` 先采纳候选（第二次 `Enter` 才提交）。
        if KeyChord::plain(KeyCode::Enter) == chord
            && owner == KeyOwner::Prompt
            && self.enter_accepts_completion()
        {
            self.accept_completion();
            return None;
        }
        if let Some(action) = self.keymap.lookup(owner, chord) {
            return self.apply(action);
        }
        // 能走到这里说明键位表没有认这一拍——剩下的就是「打字」。
        // Shift 是**打字用的**修饰键：ASCII 大写的 Shift 已被 [`KeyChord::new`] 归一化进字符
        // 本身（`actions.rs`），需要 Shift 的符号（`!`、`?`）也带着它，所以这里先把它减掉；
        // 减完还剩下 Control/Alt/Super 的，是没绑定的组合键，不往提示框里塞字符。
        //
        // 基线 `3ddd519` 只减了后三个，于是「大写字母」与「Ctrl+小写字母」都在这条路径上被
        // 静默吃掉（批次四的真调与评审暴露，见 `docs/tui-design.md` §10 的修正记录）。
        if owner == KeyOwner::Prompt
            && let KeyCode::Char(c) = chord.code
            && !c.is_control()
        {
            let modifiers = chord.modifiers - KeyModifiers::SHIFT;
            if modifiers.is_empty() {
                self.prompt.insert_char(c);
                self.refresh_completion();
                self.gate.mark_dirty();
            }
        }
        None
    }

    /// 执行一个动作，返回需要宿主处理的副作用。
    pub fn apply(&mut self, action: ActionId) -> Option<Effect> {
        // 动作会改界面：本帧命中区随即作废，等下一次渲染重建（否则关掉审批卡后，
        // 点在原按钮位置仍会命中上一帧登记的裁决）。
        self.clear_click_targets();
        match action {
            ActionId::Quit => {
                self.should_quit = true;
                Some(Effect::Quit)
            }
            // `Esc` 的阶梯：先关最上面那层（卡片 → 补全列表 → 研究视图 → 取消当前轮 → 退出）。
            ActionId::Interrupt => {
                if self.modal.take().is_some() {
                    self.refresh_completion();
                    self.gate.mark_dirty();
                    None
                } else if self.completion_visible() {
                    // 补全列表展开时 `Esc` 先收列表：输入文本原样保留，再按一次才轮下一档。
                    self.completion_dismissed = Some(self.prompt.text());
                    self.completion = None;
                    self.gate.mark_dirty();
                    None
                } else if self.screen == Screen::Research {
                    self.screen = Screen::Agent;
                    self.gate.mark_dirty();
                    None
                } else if self.status.phase == Some(TurnPhase::Running) {
                    // 有在途轮次：只取消这一轮（宿主据此停掉在途的调用），不退出。
                    // 「有没有在途轮次」是宿主的事实，它用状态行的 Running 如实告诉本 crate。
                    self.gate.mark_dirty();
                    Some(Effect::Interrupt)
                } else {
                    // 空闲时 `Esc` **什么都不做**：退出只走 `Ctrl+C` 与 `/quit`（用户口径）。
                    // 这样键位栏写「Esc 取消」就是字面意思，也不会一次误触把没发出去的草稿丢掉。
                    None
                }
            }
            ActionId::Submit => {
                if self.prompt.is_empty() {
                    return None;
                }
                let text = self.prompt.take();
                // 提交即清空输入框：补全状态跟着重算（`/` 不再开头，列表收起）。
                self.refresh_completion();
                match commands::parse(&text) {
                    // 普通消息：首屏提交即进入会话。用户块由宿主回显（它才有格式化好的时间戳），
                    // 这里再推一块会把同一句话画两遍。
                    Parsed::NotACommand => {
                        self.screen = Screen::Agent;
                        Some(Effect::Submit(text))
                    }
                    // 认不出的命令不静默吞掉，也不当消息发出去。
                    Parsed::Unknown(name) => {
                        self.screen = Screen::Agent;
                        self.push_block(Block::plain_notice(format!(
                            "未知命令 /{name}。输入 /help 看全部命令。"
                        )));
                        self.gate.mark_dirty();
                        None
                    }
                    Parsed::Known(invocation) => self.run_command(invocation),
                }
            }
            ActionId::InsertNewline => {
                self.prompt.insert_newline();
                // 换行之后第一个空白就在命令名之前：列表收起。
                self.refresh_completion();
                self.gate.mark_dirty();
                None
            }
            ActionId::CursorLeft => self.edit_prompt(Prompt::move_left),
            ActionId::CursorRight => self.edit_prompt(Prompt::move_right),
            ActionId::CursorHome => self.edit_prompt(Prompt::move_home),
            ActionId::CursorEnd => self.edit_prompt(Prompt::move_end),
            ActionId::DeleteBackward => self.edit_prompt(Prompt::backspace),
            ActionId::DeleteForward => self.edit_prompt(Prompt::delete_forward),
            // 补全可见时 `Up/Down` 改选候选（边界夹住），否则还是提示框里的光标移动。
            ActionId::CursorUp => {
                if self.step_completion(-1) {
                    self.gate.mark_dirty();
                    return None;
                }
                let width = self.prompt_width;
                self.prompt.move_up(width);
                self.gate.mark_dirty();
                None
            }
            ActionId::CursorDown => {
                if self.step_completion(1) {
                    self.gate.mark_dirty();
                    return None;
                }
                let width = self.prompt_width;
                self.prompt.move_down(width);
                self.gate.mark_dirty();
                None
            }
            // 粘贴是**终端**的事（应用读不到系统剪贴板，OSC 52 只能写不能读）：
            // 这个键走到应用里，说明终端没把它当粘贴——那就如实告诉用户该按哪个键。
            ActionId::PasteHint => {
                self.push_block(Block::notice(
                    "粘贴由终端负责（应用读不到系统剪贴板）：Windows Terminal 用 Ctrl+Shift+V，\
                     iTerm2 / kitty / WezTerm 用 ⌘V 或 Ctrl+Shift+V，mintty 用 Shift+Insert。\
                     粘贴多行不会误提交（括号粘贴已开）。",
                ));
                None
            }
            ActionId::AcceptCompletion => {
                self.accept_completion();
                None
            }
            ActionId::ScrollLineUp => self.scroll(1, false),
            ActionId::ScrollLineDown => self.scroll(1, true),
            ActionId::ScrollPageUp => {
                let step = self.page_step();
                self.scroll(step, false)
            }
            ActionId::ScrollPageDown => {
                let step = self.page_step();
                self.scroll(step, true)
            }
            ActionId::ScrollToBottom => {
                self.scrollback.scroll_to_bottom();
                self.gate.mark_dirty();
                None
            }
            ActionId::ApproveOnce | ActionId::ApproveAlways | ActionId::Deny => {
                let modal = self.modal.take()?;
                let outcome = match action {
                    ActionId::ApproveOnce => ApprovalOutcome::AllowOnce,
                    ActionId::ApproveAlways => ApprovalOutcome::AllowAlways,
                    _ => ApprovalOutcome::Deny,
                };
                let tool = modal.request().tool.clone();
                self.push_block(Block::assistant(format!(
                    "已裁决：{} {}",
                    tool,
                    match outcome {
                        ApprovalOutcome::AllowOnce => "允许一次",
                        ApprovalOutcome::AllowAlways => "本次会话总是允许",
                        ApprovalOutcome::Deny => "拒绝",
                    }
                )));
                self.status = StatusModel::idle();
                Some(Effect::Approval(outcome))
            }
            ActionId::ToggleResearch => {
                self.screen = match self.screen {
                    Screen::Research => Screen::Agent,
                    _ => Screen::Research,
                };
                self.gate.mark_dirty();
                None
            }
            ActionId::NextPanel => {
                self.panel = self.panel.next();
                self.panel_state.reset();
                self.gate.mark_dirty();
                None
            }
            ActionId::PrevPanel => {
                self.panel = self.panel.prev();
                self.panel_state.reset();
                self.gate.mark_dirty();
                None
            }
            ActionId::StepOnce => Some(Effect::Step),
            ActionId::PauseToggle => {
                self.paused = !self.paused;
                self.gate.mark_dirty();
                Some(Effect::SetPaused(self.paused))
            }
        }
    }

    /// 滚动：研究视图里滚面板，其余滚回滚区。
    /// 执行一条已解析的命令。界面自处理的直接办；要会话状态的折成 [`Effect::Command`]。
    fn run_command(&mut self, invocation: commands::Invocation) -> Option<Effect> {
        match invocation.spec.name {
            "help" => {
                self.screen = Screen::Agent;
                self.push_block(Block::plain_notice(commands::help_text()));
                self.gate.mark_dirty();
                None
            }
            "quit" => {
                self.should_quit = true;
                Some(Effect::Quit)
            }
            "clear" => {
                self.screen = Screen::Agent;
                self.scrollback.clear();
                self.gate.mark_dirty();
                None
            }
            "research" => {
                self.set_screen(Screen::Research);
                None
            }
            name => {
                // 宿主命令：回来的是通知与状态更新（模型换了还要刷信息行）。
                self.screen = Screen::Agent;
                Some(Effect::Command(CommandCall {
                    name: commands::find(name).map_or(name, |spec| spec.name),
                    args: invocation.args,
                }))
            }
        }
    }

    fn scroll(&mut self, rows: usize, down: bool) -> Option<Effect> {
        if self.key_owner() == KeyOwner::Research {
            let max = self.panel_max_offset();
            if down {
                self.panel_state.offset = self.panel_state.offset.saturating_sub(rows);
            } else {
                self.panel_state.offset = (self.panel_state.offset + rows).min(max);
            }
        } else if down {
            self.scrollback.scroll_down(rows);
        } else {
            let max = self.max_scroll_offset();
            self.scrollback.scroll_up(rows, max);
        }
        self.gate.mark_dirty();
        None
    }

    fn edit_prompt(&mut self, edit: impl Fn(&mut Prompt)) -> Option<Effect> {
        edit(&mut self.prompt);
        // 打字 / 退格 / 删除都会走到这里：每次文本变化都重算候选并把选中归零。
        self.refresh_completion();
        self.gate.mark_dirty();
        None
    }

    /// 重算补全状态：查询串一变就重算候选并把选中归零，条件不满足就收起。
    ///
    /// 触发条件（任一不满足即不展开）：
    /// - 提示框持有键盘（研究视图与审批卡都不持有）；
    /// - 文本以 `/` 开头；
    /// - 第一个空白之前（还在敲命令名，没进参数区）。
    ///
    /// `Esc` 收起过的文本只要没再变就保持收起（[`App::completion_dismissed`]）。
    ///
    /// 渲染前（[`crate::render::draw`]）与每次按键前都会调一次：宿主可以直接改写提示框
    /// 文本，补全状态不允许和文本对不上账。
    pub(crate) fn refresh_completion(&mut self) {
        let text = self.prompt.text();
        if self.completion_dismissed.as_deref() == Some(text.as_str()) {
            self.completion = None;
            return;
        }
        self.completion_dismissed = None;
        let Some(query) = self.completion_query(&text) else {
            self.completion = None;
            return;
        };
        // 查询串没变就不重算（候选是查询串的纯函数），选中项不会因为别的原因被拉回第一项。
        if let Some(state) = self.completion.as_mut()
            && state.query == query
        {
            return;
        }
        self.completion = Some(CompletionState {
            matches: commands::candidates(&query),
            query,
            selected: 0,
        });
    }

    /// 补全的查询串：`/` 之后、第一个空白之前的那段；触发条件不满足时为 `None`。
    fn completion_query(&self, text: &str) -> Option<String> {
        if self.key_owner() != KeyOwner::Prompt {
            return None;
        }
        let query = text.strip_prefix('/')?;
        if query.contains(char::is_whitespace) {
            return None;
        }
        Some(query.to_owned())
    }

    /// 补全可见时把选中项挪 `delta` 格：**边界夹住，不环绕**。
    ///
    /// 返回这一拍有没有被吃掉：没有候选时返回 false，`Up/Down` 落回提示框光标移动。
    fn step_completion(&mut self, delta: isize) -> bool {
        let Some(state) = self.completion.as_mut() else {
            return false;
        };
        if !state.is_visible() {
            return false;
        }
        let last = (state.matches.len() - 1) as isize;
        state.selected = (state.selected as isize + delta).clamp(0, last) as usize;
        true
    }

    /// 把选中的候选填进输入框（`Tab` 与鼠标点击共用）。
    ///
    /// 填的是 `/name`；命令可能带参数时留一个尾空格，光标正好落在参数位上。填入本身
    /// 也是一次文本变化：填完重算候选（`/model ` 已经进了参数区，列表因此自动收起）。
    fn accept_completion(&mut self) {
        let Some(spec) = self
            .completion
            .as_ref()
            .and_then(|state| state.matches.get(state.selected))
            .copied()
        else {
            return;
        };
        let mut text = format!("/{}", spec.name);
        if spec.takes_args() {
            text.push(' ');
        }
        self.prompt.take();
        self.prompt.insert_str(&text);
        self.refresh_completion();
        self.gate.mark_dirty();
    }

    fn max_scroll_offset(&self) -> usize {
        let ctx = self.block_ctx(self.scrollback_width);
        self.scrollback.max_offset(&ctx, self.scrollback_height)
    }

    /// 面板能上滚的最大行数（内容不足一屏时为 0）。
    pub fn panel_max_offset(&self) -> usize {
        let ctx = self.block_ctx(self.panel_width);
        let rows = self
            .panels
            .panel(self.panel, self.glyphs)
            .content_rows(&ctx);
        rows.saturating_sub(self.panel_height)
    }

    fn page_step(&self) -> usize {
        let height = if self.key_owner() == KeyOwner::Research {
            self.panel_height
        } else {
            self.scrollback_height
        };
        height.saturating_sub(1).max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::WaitingReason;
    use crate::theme::ColorLevel;

    fn app() -> App {
        App::new(&Capabilities::from_env_lookup(|_| None, true, false))
    }

    fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    fn press(app: &mut App, code: KeyCode) -> Option<Effect> {
        app.handle_key(KeyChord::plain(code))
    }

    /// 指定尺寸画一帧（首屏那种「放不下就不画」的行为要按尺寸分别验）。
    fn screen_sized(app: &mut App, width: u16, height: u16) -> String {
        use crate::render::{self, screen_lines};
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("TestBackend");
        terminal
            .draw(|frame| render::draw(frame, app))
            .expect("绘制不失败");
        screen_lines(terminal.backend().buffer()).join("\n")
    }

    /// 画一帧再看屏上有啥（断言命令回执用）。
    fn screen(app: &mut App) -> String {
        use crate::render::{self, screen_lines};
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut terminal = Terminal::new(TestBackend::new(60, 16)).expect("TestBackend");
        terminal
            .draw(|frame| render::draw(frame, app))
            .expect("绘制不失败");
        screen_lines(terminal.backend().buffer()).join("\n")
    }

    #[test]
    fn key_owner_follows_the_modal_slot() {
        let mut app = app();
        assert_eq!(app.key_owner(), KeyOwner::Prompt);
        assert!(app.is_prompt_focused());
        app.open_approval(ApprovalRequest::new("bash", "rm -rf build"));
        assert_eq!(app.key_owner(), KeyOwner::Approval);
        assert!(!app.is_prompt_focused());
    }

    #[test]
    fn typing_goes_into_the_prompt_and_enter_submits() {
        let mut app = app();
        press(&mut app, KeyCode::Char('h'));
        press(&mut app, KeyCode::Char('i'));
        assert_eq!(app.prompt().text(), "hi");
        let effect = press(&mut app, KeyCode::Enter);
        assert_eq!(effect, Some(Effect::Submit("hi".to_owned())));
        assert!(app.prompt().is_empty(), "提交后输入框清空");
        assert_eq!(
            app.scrollback().len(),
            0,
            "回显交给宿主（带时间戳的那一块），本 crate 不再推块"
        );
        assert!(app.gate().is_dirty());
    }

    #[test]
    fn submitting_from_startup_enters_the_agent_screen() {
        let mut app = app();
        assert_eq!(app.screen(), Screen::Startup);
        press(&mut app, KeyCode::Char('h'));
        assert_eq!(
            press(&mut app, KeyCode::Enter),
            Some(Effect::Submit("h".to_owned()))
        );
        assert_eq!(app.screen(), Screen::Agent, "首屏提交即进入会话");
    }

    #[test]
    fn uppercase_and_shifted_characters_are_typed_not_swallowed() {
        let mut app = app();
        // 终端报大写字符的两种形态（有的带 SHIFT、有的不带）归一化后都该能打进去。
        app.handle_key(KeyChord::new(KeyCode::Char('D'), KeyModifiers::NONE));
        app.handle_key(KeyChord::new(KeyCode::Char('E'), KeyModifiers::SHIFT));
        app.handle_key(KeyChord::plain(KeyCode::Char('F')));
        // 需要 Shift 的符号同理（`!` 这类不是字母，归一化不动它，SHIFT 会一路带进来）。
        app.handle_key(KeyChord::new(KeyCode::Char('!'), KeyModifiers::SHIFT));
        assert_eq!(app.prompt().text(), "DEF!");
    }

    #[test]
    fn unbound_combinations_do_not_leak_characters_into_the_prompt() {
        let mut app = app();
        press(&mut app, KeyCode::Char('a'));
        app.handle_key(KeyChord::ctrl(KeyCode::Char('q')));
        app.handle_key(KeyChord::alt(KeyCode::Char('x')));
        app.handle_key(KeyChord::ctrl(KeyCode::Char('z')));
        assert_eq!(app.prompt().text(), "a", "没绑定的组合键不该塞字符进来");
    }

    #[test]
    fn empty_prompt_does_not_submit() {
        let mut app = app();
        assert_eq!(press(&mut app, KeyCode::Enter), None);
        assert_eq!(app.scrollback().len(), 0);
    }

    #[test]
    fn toggle_research_switches_screens_and_hands_the_keyboard_to_the_panel() {
        let mut app = app();
        assert_eq!(
            app.handle_key(KeyChord::ctrl(KeyCode::Char('o'))),
            None,
            "切视图本身没有副作用"
        );
        assert_eq!(app.screen(), Screen::Research);
        assert_eq!(app.key_owner(), KeyOwner::Research);
        // 研究视图里 `s` 是单步，不是文本输入。
        assert_eq!(press(&mut app, KeyCode::Char('s')), Some(Effect::Step));
        assert_eq!(app.prompt().text(), "");
        // 再按一次回到会话。
        app.handle_key(KeyChord::ctrl(KeyCode::Char('o')));
        assert_eq!(app.screen(), Screen::Agent);
        assert_eq!(app.key_owner(), KeyOwner::Prompt);
    }

    #[test]
    fn escape_leaves_the_research_screen_before_quitting() {
        let mut app = app();
        app.set_screen(Screen::Research);
        assert_eq!(press(&mut app, KeyCode::Esc), None);
        assert_eq!(app.screen(), Screen::Agent, "Esc 先退出研究视图");
        assert!(!app.should_quit());
        // 回到会话屏后空闲：`Esc` 什么都不做。
        assert_eq!(press(&mut app, KeyCode::Esc), None);
        assert!(!app.should_quit(), "空闲 Esc 不再退出会话");
    }

    #[test]
    fn panel_keys_cycle_and_reset_the_scroll_offset() {
        let mut app = app();
        app.set_screen(Screen::Research);
        app.set_panel_viewport(40, 2);
        assert_eq!(app.panel(), PanelId::Judgment);
        assert_eq!(press(&mut app, KeyCode::Tab), None);
        assert_eq!(app.panel(), PanelId::Evidence);
        // Shift+Tab（终端报成 BackTab）回到上一个面板。
        app.handle_key(KeyChord::new(KeyCode::BackTab, KeyModifiers::SHIFT));
        assert_eq!(app.panel(), PanelId::Judgment);
        assert_eq!(press(&mut app, KeyCode::Tab), None);
        assert_eq!(app.panel(), PanelId::Evidence);

        // 面板滚动：上滚受内容高度约束，下滚回到顶部。
        app.set_panel_viewport(40, 1);
        press(&mut app, KeyCode::Down);
        assert_eq!(app.panel_state().offset, 0, "空数据没有可滚内容");
        press(&mut app, KeyCode::PageDown);
        assert_eq!(app.panel_state().offset, 0);
    }

    #[test]
    fn pause_toggles_and_reports_the_host_side_effect() {
        let mut app = app();
        app.set_screen(Screen::Research);
        assert!(!app.paused());
        assert_eq!(
            press(&mut app, KeyCode::Char(' ')),
            Some(Effect::SetPaused(true))
        );
        assert!(app.paused());
        assert_eq!(
            press(&mut app, KeyCode::Char(' ')),
            Some(Effect::SetPaused(false))
        );
        assert!(!app.paused());
    }

    /// `Shift+Enter` 换行、`Enter` 提交；`Ctrl+J` 是不依赖键盘增强协议的退路，
    /// `Alt+Enter` 自本批起不再绑定（用户口径）。
    #[test]
    fn shift_enter_inserts_a_newline_instead_of_submitting() {
        let mut app = app();
        press(&mut app, KeyCode::Char('a'));
        app.handle_key(KeyChord::shift(KeyCode::Enter));
        press(&mut app, KeyCode::Char('b'));
        assert_eq!(app.prompt().text(), "a\nb");
        assert_eq!(app.scrollback().len(), 0, "没有提交");

        // 退路：`Ctrl+J` 同样插换行。
        app.handle_key(KeyChord::ctrl(KeyCode::Char('j')));
        press(&mut app, KeyCode::Char('c'));
        assert_eq!(app.prompt().text(), "a\nb\nc");

        // `Alt+Enter` 不再是换行键：它落回普通按键路径（不提交、不换行）。
        let before = app.prompt().text();
        app.handle_key(KeyChord::alt(KeyCode::Enter));
        assert_eq!(app.prompt().text(), before, "Alt+Enter 不该改变输入框");
        assert_eq!(app.scrollback().len(), 0, "Alt+Enter 也不提交");
    }

    /// `/help`、`/quit`、`/clear`、`/research` 由界面自己办；`/model` 等折成 `Effect::Command`
    /// 交给宿主。命令永远不进对话上下文。
    #[test]
    fn slash_commands_are_intercepted_before_the_model() {
        let mut app = app();

        // /help：回一块命令清单，不提交。命令多了以后清单长过一屏，断言打在**回滚区全文**上
        // （屏幕只显示可见窗口，末尾几条会把开头挤出视野）。
        app.prompt_mut().insert_str("/help");
        assert_eq!(press(&mut app, KeyCode::Enter), None);
        // 回滚区全文：块自己的 `plain_text()` 就是屏上那份文本（含换行符）。
        let help = {
            let ctx = app.block_ctx(76);
            app.scrollback()
                .blocks()
                .iter()
                .map(|block| block.output(&ctx).plain_text())
                .collect::<Vec<_>>()
                .join("\n")
        };
        assert!(help.contains("/quit"), "{help}");
        assert!(help.contains("/permission"), "{help}");
        assert!(help.contains("/resume"), "{help}");
        assert_eq!(app.scrollback().len(), 1, "只推了帮助块");

        // /clear：清空回滚区，不动别的。
        app.prompt_mut().insert_str("/clear");
        assert_eq!(press(&mut app, KeyCode::Enter), None);
        assert_eq!(app.scrollback().len(), 0);

        // /research：切研究视图。
        app.prompt_mut().insert_str("/research");
        assert_eq!(press(&mut app, KeyCode::Enter), None);
        assert_eq!(app.screen(), Screen::Research);

        // /model：宿主命令，带参数原样传过去。
        app.set_screen(Screen::Agent);
        app.prompt_mut().insert_str("/model deepseek-reasoner");
        assert_eq!(
            press(&mut app, KeyCode::Enter),
            Some(Effect::Command(CommandCall {
                name: "model",
                args: "deepseek-reasoner".to_owned(),
            }))
        );

        // /quit：置退出位并给出 Quit。
        app.prompt_mut().insert_str("/quit");
        assert_eq!(press(&mut app, KeyCode::Enter), Some(Effect::Quit));
        assert!(app.should_quit());
    }

    /// 选项列表：`Up/Down` 选、`Enter` 用选中的值执行该命令、`Esc` 关；边界夹住。
    #[test]
    fn the_choice_list_moves_and_confirms_with_the_keyboard() {
        let mut app = app();
        app.open_choices(
            "resume",
            "历史会话（2 个）· ↑↓ 选择 · Enter 接回 · Esc 关闭",
            vec![
                ChoiceEntry {
                    value: "run-a".to_owned(),
                    label: "2 轮 · deepseek-flash".to_owned(),
                },
                ChoiceEntry {
                    value: "run-b".to_owned(),
                    label: "1 轮 · deepseek-flash".to_owned(),
                },
            ],
        );
        let list = app.choices().expect("列表开着");
        assert_eq!(list.command, "resume");
        assert_eq!(list.selected, 0);
        assert_eq!(list.selected_value(), Some("run-a"));

        // 边界夹住：到顶再按 Up 不动
        app.handle_key(KeyChord::plain(KeyCode::Up));
        assert_eq!(app.choices().expect("还开着").selected, 0);
        app.handle_key(KeyChord::plain(KeyCode::Down));
        assert_eq!(app.choices().expect("还开着").selected, 1);
        app.handle_key(KeyChord::plain(KeyCode::Down));
        assert_eq!(app.choices().expect("还开着").selected, 1, "到底不动");

        // Enter：给出「命令 + 选中值」的副作用，并把列表收掉
        assert_eq!(
            app.handle_key(KeyChord::plain(KeyCode::Enter)),
            Some(Effect::Choice {
                command: "resume",
                value: "run-b".to_owned()
            })
        );
        assert!(app.choices().is_none(), "选完列表收掉");
    }

    /// `/permission` 这类固定取值命令同样走选项列表：命令名跟着列表走。
    #[test]
    fn a_choice_list_carries_its_own_command_name() {
        let mut app = app();
        app.open_choices(
            "permission",
            "授权模式 · ↑↓ 选择 · Enter 切换 · Esc 关闭",
            vec![
                ChoiceEntry {
                    value: "auto".to_owned(),
                    label: "自动（Jev 判定）· 当前".to_owned(),
                },
                ChoiceEntry {
                    value: "never".to_owned(),
                    label: "从不询问".to_owned(),
                },
            ],
        );
        app.handle_key(KeyChord::plain(KeyCode::Down));
        assert_eq!(
            app.handle_key(KeyChord::plain(KeyCode::Enter)),
            Some(Effect::Choice {
                command: "permission",
                value: "never".to_owned()
            })
        );
    }

    /// 首屏不画回滚区，但按 `Ctrl+V` 得到的通知要看得见（贴在居中面板下方）。
    #[test]
    fn the_startup_screen_shows_the_paste_notice() {
        let mut app = app();
        assert_eq!(app.screen(), Screen::Startup, "默认就在首屏");
        assert_eq!(app.handle_key(KeyChord::ctrl(KeyCode::Char('v'))), None);
        let drawn = screen_sized(&mut app, 100, 28);
        assert!(
            drawn.contains("Ctrl+Shift+V"),
            "首屏也要能看到提示：{drawn}"
        );
        assert!(drawn.contains("粘贴由终端负责"), "{drawn}");
    }

    /// 列表画在提示框上方：标题 + 每行 值/说明 + 选中行带标记。
    #[test]
    fn the_choice_list_is_drawn_above_the_prompt() {
        let mut app = app();
        app.set_screen(Screen::Agent);
        app.open_choices(
            "model",
            "模型 · ↑↓ 选择 · Enter 切换 · Esc 关闭",
            vec![
                ChoiceEntry {
                    value: "deepseek-flash".to_owned(),
                    label: "DeepSeek 缺省 · 当前".to_owned(),
                },
                ChoiceEntry {
                    value: "deepseek-v4-pro".to_owned(),
                    label: "DeepSeek 官方".to_owned(),
                },
            ],
        );
        let drawn = screen(&mut app);
        assert!(drawn.contains("模型 · ↑↓ 选择"), "{drawn}");
        assert!(
            drawn.contains("deepseek-flash") && drawn.contains("deepseek-v4-pro"),
            "{drawn}"
        );
        assert!(drawn.contains("DeepSeek 官方"), "{drawn}");
        // 选中行带行首标记（字形档不同、标记字符也不同，所以只断言「不以空白开头」）；
        // 未选中的那行按同一列留白。
        let selected = drawn
            .lines()
            .find(|line| line.contains("deepseek-flash"))
            .expect("选中行在屏上");
        let other = drawn
            .lines()
            .find(|line| line.contains("deepseek-v4-pro"))
            .expect("另一行在屏上");
        assert!(!selected.starts_with(' '), "选中行带标记：{selected:?}");
        assert!(other.starts_with(' '), "未选中行留白对齐：{other:?}");
    }

    #[test]
    fn the_choice_list_closes_on_esc_without_choosing() {
        let mut app = app();
        app.open_choices(
            "resume",
            "历史会话（1 个）",
            vec![ChoiceEntry {
                value: "run-a".to_owned(),
                label: "1 轮".to_owned(),
            }],
        );
        assert_eq!(app.handle_key(KeyChord::plain(KeyCode::Esc)), None);
        assert!(app.choices().is_none());
    }

    /// 空的列表不接键：`Up/Down/Enter` 落回原来的语义。
    #[test]
    fn an_empty_choice_list_yields_the_keyboard() {
        let mut app = app();
        app.open_choices("permission", "授权模式", Vec::new());
        assert_eq!(app.handle_key(KeyChord::plain(KeyCode::Enter)), None);
        assert!(app.choices().is_none(), "Enter 收掉空列表");
    }

    /// `Ctrl+V` 不粘贴（应用读不到系统剪贴板），但要**如实说出来**该按哪个键，而不是静默无视。
    #[test]
    fn ctrl_v_explains_which_key_pastes() {
        let mut app = app();
        app.set_screen(Screen::Agent);
        app.prompt_mut().insert_str("已经打好的字");

        assert_eq!(
            app.handle_key(KeyChord::ctrl(KeyCode::Char('v'))),
            None,
            "粘贴提示没有副作用，只是回一块通知"
        );
        assert_eq!(app.prompt().text(), "已经打好的字", "不许往输入框里塞字符");
        let block = app
            .scrollback()
            .blocks()
            .last()
            .expect("回了一块通知")
            .output(&app.block_ctx(60))
            .plain_text();
        assert!(block.contains("Ctrl+Shift+V"), "{block}");
        assert!(block.contains("终端"), "要说清是终端负责：{block}");

        // 终端把 `Ctrl+Shift+V` 漏给应用时，同样给提示
        let key = KeyChord {
            code: KeyCode::Char('v'),
            modifiers: KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        };
        assert_eq!(app.handle_key(key), None);
        assert!(app.prompt().text() == "已经打好的字");
    }

    /// 在回滚区拖着选文本：松手时给出要复制的文本（按显示列裁、不含块标记列）。
    #[test]
    fn dragging_over_the_scrollback_selects_and_copies_text() {
        let mut app = app();
        app.set_screen(Screen::Agent);
        app.push_block(Block::assistant("第一行\n第二行"));
        app.set_viewport(20, 6, 18);
        app.set_scrollback_origin(0, 1);

        // 行号与列号都按渲染时的口径：回滚区原点 (0,1)，内容从第一行开始。
        let ctx = app.block_ctx(20);
        let lines = app.scrollback().layout(&ctx);
        assert_eq!(lines.len(), 2, "两行内容：{:?}", lines.len());
        assert_eq!(
            lines[0].selection_text().unwrap(),
            "第一行",
            "块标记列不可选"
        );

        assert_eq!(
            app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 2, 1)),
            None,
            "按下不起作用，要等松手"
        );
        assert!(
            app.selection().is_none(),
            "只按下还没拖：不算选择，也不需要高亮"
        );
        app.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), 5, 2));
        assert!(app.selection().is_some(), "拖起来就该有高亮");
        let effect = app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), 5, 2));
        let Some(Effect::Copy { text, rows }) = effect else {
            panic!("松手应当给出复制内容：{effect:?}");
        };
        // 第一行从「第」开始整行；第二行到第 5 列（含）→ 取到「第二」两个字。
        assert_eq!(text, "第一行\n第二", "{text}");
        assert_eq!(rows, 2);
    }

    /// 高亮真的画在格子上：选中区加反显，没选中的格子不加，文本一个字不变。
    ///
    /// 宽字符只占一格样式位：ratatui 会把它的尾格 `reset()`（尾格被字形盖住，样式用不上，
    /// 见 `Buffer::set_stringn`），所以断言打在**字形所在列**上，不要求尾格也带反显。
    #[test]
    fn the_selection_is_drawn_as_reverse_video() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        use ratatui::style::Modifier;

        let mut app = app();
        app.set_screen(Screen::Agent);
        app.push_block(Block::assistant("一行字"));
        // 先画一帧：App 在这一帧里记下回滚区的原点与视口，鼠标坐标才有意义。
        let mut terminal = Terminal::new(TestBackend::new(40, 12)).expect("TestBackend");
        let draw = |app: &mut App, terminal: &mut Terminal<TestBackend>| {
            terminal
                .draw(|frame| crate::render::draw(frame, app))
                .expect("画一帧");
        };
        draw(&mut app, &mut terminal);
        let (x, y) = app.scrollback_origin;
        let before: String = (0..40)
            .map(|column| terminal.backend().buffer()[(column, y)].symbol().to_owned())
            .collect();

        // 只选中第一个字（列 2 起，宽字符占 2 列）：按下与拖到同一格会被当成点击，
        // 所以这里拖到它占的第二列。
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), x + 2, y));
        app.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), x + 3, y));
        draw(&mut app, &mut terminal);
        let buffer = terminal.backend().buffer();
        assert!(
            buffer[(x + 2, y)]
                .style()
                .add_modifier
                .contains(Modifier::REVERSED),
            "选中的字形要反显：{:?}",
            buffer[(x + 2, y)].style()
        );
        assert!(
            !buffer[(x + 6, y)]
                .style()
                .add_modifier
                .contains(Modifier::REVERSED),
            "没选中的格子不该反显"
        );

        // 整行都选上：三个字都反显，文本逐格不变（切 span 不许吞字或加字）
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), x + 2, y));
        app.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), x + 20, y));
        draw(&mut app, &mut terminal);
        let buffer = terminal.backend().buffer();
        for column in [x + 2, x + 4, x + 6] {
            assert!(
                buffer[(column, y)]
                    .style()
                    .add_modifier
                    .contains(Modifier::REVERSED),
                "第 {column} 列的字形要反显"
            );
        }
        let after: String = (0..40)
            .map(|column| buffer[(column, y)].symbol().to_owned())
            .collect();
        assert_eq!(after, before, "高亮不许改文本");
    }

    /// 单击（没拖动）不算选择：既不复制，也不留高亮。
    #[test]
    fn a_plain_click_in_the_scrollback_copies_nothing() {
        let mut app = app();
        app.set_screen(Screen::Agent);
        app.push_block(Block::assistant("一行字"));
        app.set_viewport(20, 6, 18);
        app.set_scrollback_origin(0, 1);

        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 2, 1));
        let effect = app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), 2, 1));
        assert_eq!(effect, None, "没拖动就没有复制");
        assert!(app.selection().is_none(), "也不留选择");
    }

    /// 命中登记区域（审批卡、补全行、首屏菜单）仍然是点击，不进入拖选。
    #[test]
    fn a_click_on_a_registered_target_wins_over_selection() {
        let mut app = app();
        app.set_screen(Screen::Agent);
        app.push_block(Block::assistant("一行字"));
        app.set_viewport(20, 6, 18);
        app.set_scrollback_origin(0, 1);
        app.register_click(Rect::new(0, 1, 20, 1), ActionId::ScrollPageUp);

        let effect = app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 3, 1));
        assert!(
            matches!(effect, Some(Effect::Command(_)) | None),
            "命中区域走原来的动作通路：{effect:?}"
        );
        assert!(app.selection().is_none(), "命中区域不参与拖选");
    }

    /// `Esc` 先清选择（不顺手取消这一轮）；没有选择时 `Esc` 才走原来的取消。
    #[test]
    fn esc_clears_the_selection_before_cancelling_the_turn() {
        let mut app = app();
        app.set_screen(Screen::Agent);
        app.push_block(Block::assistant("一行字"));
        app.set_viewport(20, 6, 18);
        app.set_scrollback_origin(0, 1);
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 2, 1));
        app.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), 6, 1));
        assert!(app.selection().is_some());

        assert_eq!(press(&mut app, KeyCode::Esc), None, "第一次 Esc 只清选择");
        assert!(app.selection().is_none());
        assert_eq!(press(&mut app, KeyCode::Esc), None, "空闲 Esc 不再退出");
        assert!(!app.should_quit());
    }

    #[test]
    fn an_unknown_command_is_reported_instead_of_sent_to_the_model() {
        let mut app = app();
        app.prompt_mut().insert_str("/nope 参数");
        assert_eq!(press(&mut app, KeyCode::Enter), None, "不发给模型");
        let screen = screen(&mut app);
        assert!(screen.contains("未知命令"), "{screen}");
        assert!(screen.contains("/help"), "{screen}");
    }

    #[test]
    fn a_plain_message_still_goes_to_the_model() {
        let mut app = app();
        app.prompt_mut().insert_str("解释一下 app.log");
        assert_eq!(
            press(&mut app, KeyCode::Enter),
            Some(Effect::Submit("解释一下 app.log".to_owned()))
        );
        assert_eq!(app.screen(), Screen::Agent);
    }

    #[test]
    fn permission_keys_are_owned_by_the_card() {
        let mut app = app();
        app.open_approval(ApprovalRequest::new("bash", "ls"));
        // 卡片在时 `y` 是权限键，不会被当成输入。
        let effect = press(&mut app, KeyCode::Char('y'));
        assert_eq!(effect, Some(Effect::Approval(ApprovalOutcome::AllowOnce)));
        assert_eq!(app.prompt().text(), "");
        assert!(app.modal().is_none(), "裁决后槽位清空");
        // 卡片不在了，`y` 又是普通文本。
        press(&mut app, KeyCode::Char('y'));
        assert_eq!(app.prompt().text(), "y");
    }

    #[test]
    fn escape_closes_the_card_first_and_only_then_cancels_or_quits() {
        let mut app = app();
        app.open_approval(ApprovalRequest::new("bash", "ls"));
        assert_eq!(press(&mut app, KeyCode::Esc), None);
        assert!(app.modal().is_none());
        assert!(!app.should_quit());
        // 卡片关掉时状态行还停在「等待授权」（这一轮仍在途）：下一记 Esc 取消这一轮。
        assert_eq!(press(&mut app, KeyCode::Esc), Some(Effect::Interrupt));
        assert!(!app.should_quit(), "在途轮次没结束前不退出");
        // 宿主把轮次收尾（状态行空闲）之后，Esc 才是退出。
        *app.status_mut() = StatusModel::idle();
        // 空闲之后再按 `Esc`：什么都不做（退出只走 Ctrl+C / /quit）。
        assert_eq!(press(&mut app, KeyCode::Esc), None);
        assert!(!app.should_quit(), "空闲 Esc 不再退出会话");
    }

    #[test]
    fn escape_cancels_the_running_turn_and_idle_escape_does_nothing() {
        let mut app = app();
        // 宿主用状态行的 Running 如实告诉本 crate「有在途轮次」。
        *app.status_mut() = StatusModel::running(WaitingReason::ToolRunning);
        assert_eq!(press(&mut app, KeyCode::Esc), Some(Effect::Interrupt));
        assert!(!app.should_quit(), "有在途轮次时 Esc 只取消这一轮");
        // 轮次收尾（空闲）之后，`Esc` **什么都不做**：退出只走 Ctrl+C 与 /quit。
        *app.status_mut() = StatusModel::idle();
        assert_eq!(press(&mut app, KeyCode::Esc), None);
        assert!(!app.should_quit(), "空闲 Esc 不再退出会话");
    }

    #[test]
    fn paging_scrolls_the_window_and_writes_nothing_extra() {
        let mut app = app();
        app.set_viewport(40, 3, 36);
        for index in 0..10 {
            app.push_block(Block::user(format!("第 {index} 行")));
        }
        let bottom = |app: &App| {
            let ctx = app.block_ctx(40);
            app.scrollback().window(&ctx, 3).lines[2].text()
        };
        let at_bottom = |app: &App| {
            let ctx = app.block_ctx(40);
            app.scrollback().window(&ctx, 3).at_bottom()
        };
        assert_eq!(bottom(&app), "❯ 第 9 行");
        // PgUp 只注册在 scrollback 上下文，输入时靠下溢一档命中。
        assert_eq!(press(&mut app, KeyCode::PageUp), None);
        assert_ne!(bottom(&app), "❯ 第 9 行");
        assert!(!at_bottom(&app));
        assert_eq!(press(&mut app, KeyCode::PageDown), None);
        assert!(at_bottom(&app));
    }

    #[test]
    fn pushing_while_scrolled_keeps_the_viewport_anchored() {
        let mut app = app();
        app.set_viewport(40, 2, 36);
        for index in 0..3 {
            app.push_block(Block::user(format!("第 {index} 行")));
        }
        assert_eq!(press(&mut app, KeyCode::PageUp), None);
        let anchored = |app: &App| {
            let ctx = app.block_ctx(40);
            app.scrollback().window(&ctx, 2).lines[0].text()
        };
        let before = anchored(&app);
        app.push_block(Block::user("新消息"));
        assert_eq!(
            anchored(&app),
            before,
            "翻上去看历史时，新输出不应把视口拽走"
        );
    }

    #[test]
    fn streaming_assistant_text_grows_one_block_and_finishes_it() {
        let mut app = app();
        app.stream_assistant("先看");
        app.stream_assistant("一下改动");
        assert_eq!(app.scrollback().len(), 1, "同一次流式输出只占一个块");
        let Block::Assistant(block) = &app.scrollback().blocks()[0] else {
            panic!("末尾应是助手块");
        };
        assert_eq!(block.text, "先看一下改动");
        assert!(block.streaming);
        app.finish_assistant();
        let Block::Assistant(block) = &app.scrollback().blocks()[0] else {
            panic!("末尾应是助手块");
        };
        assert!(!block.streaming, "收尾后不再画「还在长」的三角");
        // 收尾之后再来的是下一轮的块，不并进上一轮。
        app.stream_assistant("第二段");
        assert_eq!(app.scrollback().len(), 2);
    }

    #[test]
    fn tool_block_starts_running_and_takes_the_result() {
        let mut app = app();
        app.begin_tool("bash", "git status --short");
        let Block::Tool(tool) = &app.scrollback().blocks()[0] else {
            panic!("末尾应是工具块");
        };
        assert_eq!(tool.status, ToolStatus::Running);
        assert!(tool.detail.is_none());
        assert!(
            app.finish_tool(ToolStatus::Ok, Some("工作区干净".to_owned())),
            "末尾是工具块，回填应当生效"
        );
        let Block::Tool(tool) = &app.scrollback().blocks()[0] else {
            panic!("末尾应是工具块");
        };
        assert_eq!(tool.status, ToolStatus::Ok);
        assert_eq!(tool.detail.as_deref(), Some("工作区干净"));
        // 末尾不是工具块时如实返回 false（宿主据此另外上屏，不静默丢结果）。
        app.push_block(Block::user("插一句话"));
        assert!(!app.finish_tool(ToolStatus::Failed, Some("失败".to_owned())));
        app.stream_assistant("流式");
        assert!(app.finish_assistant());
        assert!(!app.finish_assistant(), "收尾一次之后末尾不再是流式块");
    }

    #[test]
    fn streaming_while_scrolled_keeps_the_viewport_anchored() {
        let mut app = app();
        app.set_viewport(40, 2, 36);
        for index in 0..3 {
            app.push_block(Block::user(format!("第 {index} 行")));
        }
        assert_eq!(press(&mut app, KeyCode::PageUp), None);
        let anchored = |app: &App| {
            let ctx = app.block_ctx(40);
            app.scrollback().window(&ctx, 2).lines[0].text()
        };
        let before = anchored(&app);
        app.stream_assistant("很长的一段流式输出，会把行数顶开");
        assert_eq!(anchored(&app), before, "流式追加同样不该把视口拽走");
    }

    #[test]
    fn idle_ticks_do_not_repaint() {
        let mut app = app();
        // 先画一帧把 dirty 清掉。
        let sequence = app.gate_mut().queue_frame();
        app.gate_mut().acknowledge(sequence);
        assert!(!app.gate().should_draw());
        app.advance_tick();
        assert_eq!(app.tick(), 1);
        assert_eq!(app.tick_demand(), TickDemand::None);
        assert!(!app.gate().should_draw(), "空闲不重绘");

        // 有在途轮次时才会重绘，且帧号按整除降频。
        *app.status_mut() = StatusModel::running(WaitingReason::ToolRunning);
        assert_eq!(app.tick_demand(), TickDemand::Fast);
        for _ in 0..3 {
            app.advance_tick();
        }
        assert_eq!(app.tick(), 4);
        assert!(app.gate().should_draw());
        assert_eq!(app.block_ctx(10).frame, 1, "tick 4 → 第 1 帧");
    }

    #[test]
    fn quit_is_always_available() {
        let mut app = app();
        assert_eq!(
            app.handle_key(KeyChord::ctrl(KeyCode::Char('c'))),
            Some(Effect::Quit)
        );
        assert!(app.should_quit());
    }

    #[test]
    fn theme_follows_the_capability_bits() {
        let plain = App::new(&Capabilities::from_env_lookup(|_| None, true, false));
        assert_eq!(plain.theme().level, ColorLevel::None);
        let fancy = App::new(&Capabilities::from_env_lookup(
            |key| match key {
                "COLORTERM" => Some("truecolor".to_owned()),
                _ => None,
            },
            true,
            false,
        ));
        assert_eq!(fancy.theme().level, ColorLevel::TrueColor);
    }

    #[test]
    fn paste_goes_into_the_prompt_unless_a_card_owns_the_keyboard() {
        let mut app = app();
        app.handle_event(Event::Paste("粘贴\n两行".to_owned()));
        assert_eq!(app.prompt().text(), "粘贴\n两行");
        app.open_approval(ApprovalRequest::new("bash", "ls"));
        app.handle_event(Event::Paste("不该进来".to_owned()));
        assert_eq!(app.prompt().text(), "粘贴\n两行");
    }

    /// 当前候选的名字（断言筛选与排序用）。
    fn candidate_names(app: &App) -> Vec<&'static str> {
        app.completion()
            .map(|state| state.matches.iter().map(|spec| spec.name).collect())
            .unwrap_or_default()
    }

    fn type_text(app: &mut App, text: &str) {
        for c in text.chars() {
            press(app, KeyCode::Char(c));
        }
    }

    #[test]
    fn a_leading_slash_opens_the_completion_list_and_narrows_it() {
        let mut app = app();
        type_text(&mut app, "/");
        let state = app.completion().expect("`/` 应当展开补全列表");
        assert_eq!(state.query, "");
        assert_eq!(
            state.matches.len(),
            commands::COMMANDS.len(),
            "空查询列出全部命令"
        );
        assert_eq!(state.selected, 0);

        type_text(&mut app, "mdl");
        assert_eq!(app.prompt().text(), "/mdl");
        assert_eq!(app.completion().unwrap().query, "mdl");
        assert_eq!(candidate_names(&app), vec!["model"]);
    }

    #[test]
    fn tab_fills_the_selected_command_and_leaves_a_space_when_it_takes_arguments() {
        // 带参数的命令：填入 `/model` 并留一个尾空格，光标正好落在参数位。
        let mut app = app();
        type_text(&mut app, "/mdl");
        assert_eq!(press(&mut app, KeyCode::Tab), None, "填入本身没有副作用");
        assert_eq!(app.prompt().text(), "/model ", "带参数的命令留一个尾空格");
        assert!(app.completion().is_none(), "进入参数区后列表收起");
    }

    #[test]
    fn tab_fills_a_command_without_arguments_verbatim() {
        let mut app = app();
        type_text(&mut app, "/he");
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.prompt().text(), "/help", "不带参数的命令没有尾空格");
        assert_eq!(candidate_names(&app), vec!["help"], "填入后列表只剩这一条");
    }

    #[test]
    fn up_and_down_move_the_selection_and_clamp_at_the_edges() {
        let mut app = app();
        type_text(&mut app, "/");
        let count = app.completion().unwrap().matches.len();
        assert!(count > 3, "全部命令都在候选里");

        // 边界夹住，不环绕：按到底停在最后一条。
        for _ in 0..count + 3 {
            press(&mut app, KeyCode::Down);
        }
        assert_eq!(app.completion().unwrap().selected, count - 1);
        for _ in 0..count + 3 {
            press(&mut app, KeyCode::Up);
        }
        assert_eq!(app.completion().unwrap().selected, 0);
        assert_eq!(app.prompt().text(), "/", "选候选不该动输入文本");
    }

    #[test]
    fn up_and_down_still_move_the_cursor_when_no_list_is_visible() {
        let mut app = app();
        app.prompt_mut().insert_str("第一行\n第二行");
        assert_eq!(app.prompt().cursor().row, 1);
        press(&mut app, KeyCode::Up);
        assert_eq!(app.prompt().cursor().row, 0, "没有补全时 Up 仍是光标上移");
        press(&mut app, KeyCode::Down);
        assert_eq!(app.prompt().cursor().row, 1);
    }

    /// 查询没有候选时列表不可见：Up/Down 不该被它吃掉。
    #[test]
    fn up_and_down_are_not_hijacked_when_there_are_no_candidates() {
        let mut app = app();
        type_text(&mut app, "/zzz");
        assert!(app.completion().is_some(), "仍在敲命令名");
        assert!(!app.completion_visible(), "没有候选就不可见");
        app.prompt_mut().take();
        app.prompt_mut().insert_str("/zzz\n第二行");
        assert_eq!(app.prompt().cursor().row, 1);
        press(&mut app, KeyCode::Up);
        assert_eq!(app.prompt().cursor().row, 0, "空候选不接管 Up");
    }

    #[test]
    fn escape_closes_the_list_and_keeps_the_text() {
        let mut app = app();
        type_text(&mut app, "/mdl");
        assert!(app.completion_visible());
        assert_eq!(press(&mut app, KeyCode::Esc), None, "收起列表不是取消/退出");
        assert!(app.completion().is_none());
        assert_eq!(app.prompt().text(), "/mdl", "输入文本原样保留");
        assert!(!app.should_quit());
        // 文本没再变时不重新展开（否则 Esc 等于没按）。
        press(&mut app, KeyCode::Down);
        assert!(app.completion().is_none());
        // 文本一变就重新展开。
        type_text(&mut app, "x");
        assert_eq!(app.completion().unwrap().query, "mdlx");
        assert!(!app.completion_visible(), "`/mdlx` 没有候选");
        // 列表不可见时 Esc 回到原来的阶梯：空闲态**什么都不做**。
        assert_eq!(press(&mut app, KeyCode::Esc), None);
        assert!(!app.should_quit());
    }

    #[test]
    fn a_space_after_the_command_name_closes_the_list() {
        let mut app = app();
        type_text(&mut app, "/model");
        assert!(app.completion_visible());
        type_text(&mut app, " ");
        assert!(app.completion().is_none(), "进入参数区后列表自动收起");
        type_text(&mut app, "deepseek-reasoner");
        assert!(app.completion().is_none(), "参数区里不再展开");
    }

    #[test]
    fn every_text_change_recomputes_the_candidates_and_resets_the_selection() {
        let mut app = app();
        type_text(&mut app, "/");
        for _ in 0..3 {
            press(&mut app, KeyCode::Down);
        }
        assert_eq!(app.completion().unwrap().selected, 3);

        type_text(&mut app, "m");
        let state = app.completion().unwrap();
        assert_eq!(state.query, "m");
        assert_eq!(state.selected, 0, "文本一变就把选中归零");
        assert_eq!(candidate_names(&app), vec!["model", "permission", "resume"]);

        // 退格回到空查询：又是全部命令。
        press(&mut app, KeyCode::Backspace);
        assert_eq!(app.completion().unwrap().query, "");
        assert_eq!(
            app.completion().unwrap().matches.len(),
            commands::COMMANDS.len()
        );
    }

    /// 粘贴同样是一次文本变化：候选跟着重算。
    #[test]
    fn paste_recomputes_the_candidates() {
        let mut app = app();
        app.handle_event(Event::Paste("/pm".to_owned()));
        assert_eq!(candidate_names(&app), vec!["permission"]);
        assert_eq!(app.completion().unwrap().selected, 0);
    }

    #[test]
    fn enter_accepts_a_partial_completion_but_submits_a_complete_command() {
        // ① 半截查询：`Enter` 采纳高亮候选（这里是模糊匹配 `pm` → `/permission`）
        let mut app = crate::app::App::new(&Capabilities::from_env_lookup(|_| None, true, false));
        type_text(&mut app, "/pm");
        assert!(app.completion_visible(), "列表该开着");
        assert_eq!(
            press(&mut app, KeyCode::Enter),
            None,
            "这一拍只补全，不提交"
        );
        assert_eq!(
            app.prompt().text(),
            "/permission ",
            "带参数的命令留一个尾空格"
        );
        assert!(!app.should_quit());

        // ② 只有 `/`：采纳第一项（`/help`），不提交
        let mut app = crate::app::App::new(&Capabilities::from_env_lookup(|_| None, true, false));
        type_text(&mut app, "/");
        assert_eq!(press(&mut app, KeyCode::Enter), None);
        assert_eq!(app.prompt().text(), "/help");

        // ③ 已经打全：这一拍就是提交（不然每个命令都要按两次回车）
        let mut app = crate::app::App::new(&Capabilities::from_env_lookup(|_| None, true, false));
        type_text(&mut app, "/quit");
        assert!(app.completion_visible(), "列表还开着");
        assert_eq!(
            press(&mut app, KeyCode::Enter),
            Some(Effect::Quit),
            "打全了就该执行"
        );
        assert!(app.should_quit());

        // ④ 带参数的整行：照旧提交
        let mut app = crate::app::App::new(&Capabilities::from_env_lookup(|_| None, true, false));
        type_text(&mut app, "/model deepseek-flash");
        assert_eq!(
            press(&mut app, KeyCode::Enter),
            Some(Effect::Command(CommandCall {
                name: "model",
                args: "deepseek-flash".to_owned(),
            }))
        );
    }

    #[test]
    fn the_completion_list_belongs_to_the_prompt_screen_only() {
        let mut app = app();
        type_text(&mut app, "/mdl");
        assert!(app.completion_visible());

        // 审批卡持有键盘：收起；卡片关掉后条件恢复，列表回来。
        app.open_approval(ApprovalRequest::new("bash", "ls"));
        assert!(app.completion().is_none());
        app.close_approval();
        assert!(app.completion_visible());

        // 研究视图没有提示框。
        app.set_screen(Screen::Research);
        assert!(app.completion().is_none());
        app.set_screen(Screen::Agent);
        assert!(app.completion_visible(), "回到会话屏后重新展开");
    }
}
