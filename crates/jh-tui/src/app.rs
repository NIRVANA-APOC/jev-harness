//! 应用状态 + 纯调度：键盘 → 动作 → 状态变化，副作用一律描述成 [`Effect`] 值。
//!
//! 设计来源：`docs/grok-tui-design.md` §7.1 第 9 条（决策层与 IO 层分离，dispatch 不碰
//! 终端/网络/文件系统，上游把它写成不变量：`"dispatch fully testable without tokio or a
//! terminal"`，`app/dispatch/mod.rs:1-10`）与 §7.3 的 `app.rs` 职责。
//!
//! 于是：本模块只依赖 [`crate::actions`]、[`crate::blocks`]、[`crate::components`] 与
//! [`crate::render::RenderGate`]，不出现任何 IO 调用；宿主拿 [`Effect`] 去干活。

use crossterm::event::{Event, KeyCode, KeyModifiers};

use crate::actions::{ActionId, KeyChord, KeyOwner, Keymap};
use crate::blocks::{Block, BlockCtx, DecisionTrace, ToolStatus};
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
}

/// 审批裁决。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalOutcome {
    AllowOnce,
    AllowAlways,
    Deny,
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
        self.screen = screen;
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

    /// 面板视口的回写（宽度与内高，用于折算内容行数与滚动上限）。
    pub fn set_panel_viewport(&mut self, width: usize, height: usize) {
        self.panel_width = width;
        self.panel_height = height;
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
        self.modal = Some(ActiveModal::Approval(request));
        self.status = StatusModel::running(WaitingReason::Approval).with_model(
            self.status
                .model
                .clone()
                .unwrap_or_else(|| "unknown".to_owned()),
        );
        self.gate.mark_dirty();
    }

    /// 关掉审批卡并恢复状态行。
    pub fn close_approval(&mut self) {
        self.modal = None;
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
                    self.gate.mark_dirty();
                }
                None
            }
            Event::Resize(_, _) => {
                self.gate.mark_dirty();
                None
            }
            _ => None,
        }
    }

    /// 处理一次按键：先查动作注册表；没命中且提示框持有键盘时，普通字符就是文本输入。
    pub fn handle_key(&mut self, chord: KeyChord) -> Option<Effect> {
        let owner = self.key_owner();
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
                self.gate.mark_dirty();
            }
        }
        None
    }

    /// 执行一个动作，返回需要宿主处理的副作用。
    pub fn apply(&mut self, action: ActionId) -> Option<Effect> {
        match action {
            ActionId::Quit => {
                self.should_quit = true;
                Some(Effect::Quit)
            }
            // `Esc` 的阶梯：先关最上面那层（卡片 → 研究视图 → 取消当前轮 → 退出）。
            ActionId::Interrupt => {
                if self.modal.take().is_some() {
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
                    self.should_quit = true;
                    Some(Effect::Interrupt)
                }
            }
            ActionId::Submit => {
                if self.prompt.is_empty() {
                    return None;
                }
                let text = self.prompt.take();
                // 首屏提交即进入会话。
                self.screen = Screen::Agent;
                // 用户块由宿主回显（它才有格式化好的时间戳）：这里再推一块会把同一句话画两遍。
                Some(Effect::Submit(text))
            }
            ActionId::InsertNewline => {
                self.prompt.insert_newline();
                self.gate.mark_dirty();
                None
            }
            ActionId::CursorLeft => self.edit_prompt(Prompt::move_left),
            ActionId::CursorRight => self.edit_prompt(Prompt::move_right),
            ActionId::CursorHome => self.edit_prompt(Prompt::move_home),
            ActionId::CursorEnd => self.edit_prompt(Prompt::move_end),
            ActionId::DeleteBackward => self.edit_prompt(Prompt::backspace),
            ActionId::DeleteForward => self.edit_prompt(Prompt::delete_forward),
            ActionId::CursorUp => {
                let width = self.prompt_width;
                self.prompt.move_up(width);
                self.gate.mark_dirty();
                None
            }
            ActionId::CursorDown => {
                let width = self.prompt_width;
                self.prompt.move_down(width);
                self.gate.mark_dirty();
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
        self.gate.mark_dirty();
        None
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

    fn press(app: &mut App, code: KeyCode) -> Option<Effect> {
        app.handle_key(KeyChord::plain(code))
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
        assert_eq!(press(&mut app, KeyCode::Esc), Some(Effect::Interrupt));
        assert!(app.should_quit());
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

    #[test]
    fn alt_enter_inserts_a_newline_instead_of_submitting() {
        let mut app = app();
        press(&mut app, KeyCode::Char('a'));
        app.handle_key(KeyChord::alt(KeyCode::Enter));
        press(&mut app, KeyCode::Char('b'));
        assert_eq!(app.prompt().text(), "a\nb");
        assert_eq!(app.scrollback().len(), 0, "没有提交");
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
        assert_eq!(press(&mut app, KeyCode::Esc), Some(Effect::Interrupt));
        assert!(app.should_quit());
    }

    #[test]
    fn escape_cancels_the_running_turn_before_it_quits() {
        let mut app = app();
        // 宿主用状态行的 Running 如实告诉本 crate「有在途轮次」。
        *app.status_mut() = StatusModel::running(WaitingReason::ToolRunning);
        assert_eq!(press(&mut app, KeyCode::Esc), Some(Effect::Interrupt));
        assert!(!app.should_quit(), "有在途轮次时 Esc 只取消这一轮");
        // 轮次收尾（空闲）之后，Esc 才退出。
        *app.status_mut() = StatusModel::idle();
        assert_eq!(press(&mut app, KeyCode::Esc), Some(Effect::Interrupt));
        assert!(app.should_quit());
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
}
