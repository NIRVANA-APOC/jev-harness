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
use crate::blocks::{Block, BlockCtx, DecisionTrace};
use crate::components::{
    ActiveModal, ApprovalRequest, Prompt, Scrollback, StatusModel, TurnPhase, WaitingReason,
};
use crate::glyphs::Glyphs;
use crate::render::{RenderGate, TickDemand, animation_frame};
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
    status: StatusModel,
    modal: Option<ActiveModal>,
    decision: Option<DecisionTrace>,
    tick: u64,
    gate: RenderGate,
    should_quit: bool,
    /// 上一次布局算出的回滚区宽度（滚动上限要用）。
    scrollback_width: usize,
    /// 上一次布局算出的回滚区高度（翻页步长要用）。
    scrollback_height: usize,
    /// 上一次布局算出的提示框内容宽度（纵向移动要用）。
    prompt_width: usize,
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
            status: StatusModel::idle(),
            modal: None,
            decision: None,
            tick: 0,
            gate: RenderGate::new(),
            should_quit: false,
            scrollback_width: 80,
            scrollback_height: 10,
            prompt_width: 76,
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

    /// 这一拍键盘归谁：有模态卡就是卡的（权限键优先），否则归提示框。
    pub fn key_owner(&self) -> KeyOwner {
        if self.modal.is_some() {
            KeyOwner::Approval
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
        if owner == KeyOwner::Prompt
            && let KeyCode::Char(c) = chord.code
        {
            let modifiers =
                chord.modifiers - KeyModifiers::CONTROL - KeyModifiers::ALT - KeyModifiers::SUPER;
            if modifiers.is_empty() && !c.is_control() {
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
            ActionId::Interrupt => {
                if self.modal.take().is_some() {
                    self.gate.mark_dirty();
                    None
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
                self.push_block(Block::user(text.clone()));
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
            ActionId::ScrollLineUp => {
                let max = self.max_scroll_offset();
                self.scrollback.scroll_up(1, max);
                self.gate.mark_dirty();
                None
            }
            ActionId::ScrollLineDown => {
                self.scrollback.scroll_down(1);
                self.gate.mark_dirty();
                None
            }
            ActionId::ScrollPageUp => {
                let step = self.page_step();
                let max = self.max_scroll_offset();
                self.scrollback.scroll_up(step, max);
                self.gate.mark_dirty();
                None
            }
            ActionId::ScrollPageDown => {
                let step = self.page_step();
                self.scrollback.scroll_down(step);
                self.gate.mark_dirty();
                None
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
        }
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

    fn page_step(&self) -> usize {
        self.scrollback_height.saturating_sub(1).max(1)
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
        assert_eq!(app.scrollback().len(), 1, "回显了一个用户块");
        assert!(app.gate().is_dirty());
    }

    #[test]
    fn empty_prompt_does_not_submit() {
        let mut app = app();
        assert_eq!(press(&mut app, KeyCode::Enter), None);
        assert_eq!(app.scrollback().len(), 0);
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
    fn escape_closes_the_card_first_and_only_then_quits() {
        let mut app = app();
        app.open_approval(ApprovalRequest::new("bash", "ls"));
        assert_eq!(press(&mut app, KeyCode::Esc), None);
        assert!(app.modal().is_none());
        assert!(!app.should_quit());
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
