//! 状态面：状态行 + 快捷键栏。
//!
//! 设计来源：`docs/grok-tui-design.md` §4.5、§7.1 第 5 条（等待原因做成类型化枚举）
//! 与专题 `docs/research/grok-tui/12-status-surfaces.md`、`06-motion-loading-streaming.md`
//! （短等待用延迟门隐藏、动画帧用整除降频）。
//!
//! 状态行是本项目「概率上屏」的落点（设计 §7.3 P2）：把 Jev 的 `Choice` 概率与
//! `confidence` 直接写进文案，不塌缩成布尔、也不新增可视化组件。

use crate::actions::{KeyChord, KeyOwner, Keymap};
use crate::blocks::{BlockCtx, BlockLine, Selectable};
use crate::glyphs;
use crate::width;
use ratatui::text::{Line, Span};

/// 轮次阶段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnPhase {
    /// 没有在途轮次。
    Idle,
    /// 有在途轮次。
    Running,
    /// 上一轮正常收尾。
    Done,
    /// 上一轮失败。
    Failed,
}

/// 等待原因：类型化枚举而不是散落的字符串（上游有 7 种 `WaitingReason`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitingReason {
    /// 等 Jev 的类型化判定。
    JevJudging,
    /// 等 LLM 流式输出。
    LlmStreaming,
    /// 等工具返回。
    ToolRunning,
    /// 重试中。
    Retrying,
    /// 等人裁决审批卡。
    Approval,
}

impl WaitingReason {
    pub fn label(self) -> &'static str {
        match self {
            WaitingReason::JevJudging => "Jev 判定中",
            WaitingReason::LlmStreaming => "模型输出中",
            WaitingReason::ToolRunning => "工具执行中",
            WaitingReason::Retrying => "重试中",
            WaitingReason::Approval => "等待授权",
        }
    }

    /// 是否画转轮（等人的等待不转，免得看起来还在自己动）。
    pub fn animates(self) -> bool {
        !matches!(self, WaitingReason::Approval)
    }
}

/// 状态行数据。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StatusModel {
    pub phase: Option<TurnPhase>,
    pub waiting: Option<WaitingReason>,
    pub model: Option<String>,
    /// 上下文用量：`(已用, 上限)`。
    pub tokens: Option<(u32, u32)>,
}

impl StatusModel {
    pub fn idle() -> Self {
        StatusModel {
            phase: Some(TurnPhase::Idle),
            ..StatusModel::default()
        }
    }

    pub fn running(reason: WaitingReason) -> Self {
        StatusModel {
            phase: Some(TurnPhase::Running),
            waiting: Some(reason),
            ..StatusModel::default()
        }
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    pub fn with_tokens(mut self, used: u32, limit: u32) -> Self {
        self.tokens = Some((used, limit));
        self
    }

    /// 渲染状态行（一行，已按显示列裁剪）。
    pub fn render(&self, ctx: &BlockCtx) -> BlockLine {
        let mut spans = Vec::new();
        match (self.phase, self.waiting) {
            (Some(TurnPhase::Running), Some(reason)) => {
                let style = if reason == WaitingReason::Approval {
                    ctx.theme.accent(ctx.theme.accent_warning)
                } else {
                    ctx.theme.muted()
                };
                let frame = glyphs::spinner_frame(ctx.glyphs, ctx.frame);
                spans.push(Span::styled(format!("{frame} "), style));
                spans.push(Span::styled(reason.label().to_owned(), style));
            }
            (Some(TurnPhase::Done), _) => {
                spans.push(Span::styled(
                    format!("{} ", ctx.glyphs.resolve(glyphs::success())),
                    ctx.theme.accent(ctx.theme.accent_success),
                ));
                spans.push(Span::styled("完成".to_owned(), ctx.theme.muted()));
            }
            (Some(TurnPhase::Failed), _) => {
                spans.push(Span::styled(
                    format!("{} ", ctx.glyphs.resolve(glyphs::failure())),
                    ctx.theme.accent(ctx.theme.accent_error),
                ));
                spans.push(Span::styled("失败".to_owned(), ctx.theme.muted()));
            }
            _ => {
                spans.push(Span::styled(
                    format!("{} ", ctx.glyphs.resolve(glyphs::dot())),
                    ctx.theme.dim(),
                ));
                spans.push(Span::styled("空闲".to_owned(), ctx.theme.muted()));
            }
        }

        // 概率上屏：本轮判定轨迹的首选概率、差距与置信度。
        // 分隔符走字形表，与快捷键栏、块内摘要保持同一个符号。
        let separator = ctx.glyphs.resolve(glyphs::separator());
        if let Some(trace) = ctx.decision {
            let style = if trace.uncertainty().is_uncertain() {
                ctx.theme.accent(ctx.theme.accent_warning)
            } else {
                ctx.theme.accent(ctx.theme.accent_plan)
            };
            spans.push(Span::styled(format!(" {separator} 判定 "), ctx.theme.dim()));
            spans.push(Span::styled(trace.summary(separator), style));
        }

        if let Some(model) = &self.model {
            spans.push(Span::styled(format!(" {separator} "), ctx.theme.dim()));
            spans.push(Span::styled(model.clone(), ctx.theme.dim()));
        }
        if let Some((used, limit)) = self.tokens {
            spans.push(Span::styled(format!(" {separator} "), ctx.theme.dim()));
            spans.push(Span::styled(
                format!("{} 上下文", format_tokens(used, limit)),
                ctx.theme.dim(),
            ));
        }

        BlockLine::new(
            Line::from(crate::components::clip_spans(
                spans,
                ctx.width,
                ctx.glyphs.resolve(glyphs::ellipsis()),
            )),
            Selectable::All,
        )
    }
}

/// 上下文用量的短写法：`12.3k/64k`（整千不带小数）。
fn format_tokens(used: u32, limit: u32) -> String {
    fn short(value: u32) -> String {
        if value < 1000 {
            return value.to_string();
        }
        let thousands = value as f32 / 1000.0;
        if thousands.fract() < f32::EPSILON {
            format!("{}k", thousands as u32)
        } else {
            format!("{thousands:.1}k")
        }
    }
    format!("{}/{}", short(used), short(limit))
}

/// 快捷键栏：数据源只有 [`Keymap::hints`]（注册表），这里只做排版与裁剪。
#[derive(Debug, Clone, Copy, Default)]
pub struct ShortcutBar;

impl ShortcutBar {
    /// 渲染一行快捷键提示：按显示列放**整条**提示，放不下的整条丢掉并在末尾补省略号。
    pub fn render(ctx: &BlockCtx, keymap: &Keymap, owner: KeyOwner) -> BlockLine {
        let hints = keymap.hints(owner);
        let (full, placed) = hint_spans(&hints, ctx, ctx.width);
        let spans = if placed == hints.len() {
            full
        } else {
            let ellipsis = ctx.glyphs.resolve(glyphs::ellipsis());
            let reserve = width::display_width(ellipsis) + 1;
            let (mut clipped, _) = hint_spans(&hints, ctx, ctx.width.saturating_sub(reserve));
            if !clipped.is_empty() {
                clipped.push(Span::styled(" ".to_owned(), ctx.theme.dim()));
            }
            clipped.push(Span::styled(ellipsis.to_owned(), ctx.theme.dim()));
            clipped
        };
        BlockLine::new(Line::from(spans), Selectable::None)
    }
}

/// 在 `budget` 列内尽量多放整条提示，返回 `(spans, 放下的条数)`。
fn hint_spans(
    hints: &[(KeyChord, &'static str)],
    ctx: &BlockCtx,
    budget: usize,
) -> (Vec<Span<'static>>, usize) {
    let separator = ctx.glyphs.resolve(glyphs::separator());
    // 提示之间是「空格 + 分隔符 + 空格」，宽度跟着字形表走。
    let separator_width = width::display_width(separator) + 2;
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut used = 0usize;
    let mut placed = 0usize;
    for (chord, label) in hints {
        let piece_width = width::display_width(&chord.display()) + 1 + width::display_width(label);
        let joined_width = used + if placed == 0 { 0 } else { separator_width } + piece_width;
        if joined_width > budget {
            break;
        }
        if placed > 0 {
            spans.push(Span::styled(format!(" {separator} "), ctx.theme.dim()));
        }
        spans.push(Span::styled(
            chord.display(),
            ctx.theme.accent(ctx.theme.accent_system),
        ));
        spans.push(Span::styled(format!(" {label}"), ctx.theme.dim()));
        used = joined_width;
        placed += 1;
    }
    (spans, placed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::{Choice, DecisionTrace};
    use crate::glyphs::Glyphs;
    use crate::theme::Theme;

    fn ctx<'a>(
        theme: &'a Theme,
        width: usize,
        decision: Option<&'a DecisionTrace>,
    ) -> BlockCtx<'a> {
        BlockCtx {
            width,
            theme,
            glyphs: Glyphs::FANCY,
            frame: 3,
            decision,
        }
    }

    #[test]
    fn idle_status_shows_phase_model_and_tokens() {
        let theme = Theme::DARK;
        let model = StatusModel::idle()
            .with_model("deepseek")
            .with_tokens(12_300, 64_000);
        let text = model.render(&ctx(&theme, 80, None)).text();
        assert!(text.contains("空闲"), "{text}");
        assert!(text.contains("deepseek"), "{text}");
        assert!(text.contains("12.3k/64k"), "{text}");
    }

    #[test]
    fn waiting_reason_is_typed_and_animated() {
        let theme = Theme::DARK;
        let model = StatusModel::running(WaitingReason::JevJudging);
        assert!(
            model
                .render(&ctx(&theme, 80, None))
                .text()
                .contains("Jev 判定中")
        );
        assert!(WaitingReason::JevJudging.animates());
        assert!(!WaitingReason::Approval.animates());
        // 不同原因文案不同，且都来自枚举本身。
        let labels: Vec<&str> = [
            WaitingReason::JevJudging,
            WaitingReason::LlmStreaming,
            WaitingReason::ToolRunning,
            WaitingReason::Retrying,
            WaitingReason::Approval,
        ]
        .iter()
        .map(|reason| reason.label())
        .collect();
        assert_eq!(labels.len(), 5);
        assert!(labels.iter().all(|label| !label.is_empty()));
    }

    #[test]
    fn decision_probability_is_rendered_not_collapsed() {
        let theme = Theme::DARK;
        let trace = DecisionTrace::new(
            "shell_command_allowed",
            vec![
                Choice::new("usr:allow", "允许一次", 0.47),
                Choice::new("usr:deny", "拒绝", 0.44),
            ],
        )
        .with_confidence(0.55);
        let text = StatusModel::running(WaitingReason::JevJudging)
            .render(&ctx(&theme, 120, Some(&trace)))
            .text();
        assert!(text.contains("0.47"), "{text}");
        assert!(text.contains("margin 0.03"), "{text}");
        assert!(text.contains("confidence 0.55"), "{text}");
        assert!(text.contains("不确定"), "{text}");
    }

    #[test]
    fn status_line_is_clipped_to_width() {
        let theme = Theme::DARK;
        let model = StatusModel::idle()
            .with_model("a-very-long-model-name")
            .with_tokens(1, 2);
        let line = model.render(&ctx(&theme, 12, None));
        assert!(
            width::display_width(&line.text()) <= 12,
            "{:?}",
            line.text()
        );
        assert!(line.text().ends_with('…'));
    }

    #[test]
    fn shortcut_bar_uses_the_registry_and_truncates() {
        let theme = Theme::DARK;
        let keymap = Keymap::from_defaults();
        let wide = ShortcutBar::render(&ctx(&theme, 80, None), &keymap, KeyOwner::Prompt);
        let text = wide.text();
        assert!(text.contains("Ctrl+c 退出"), "{text}");
        assert!(text.contains("Enter 发送"), "{text}");
        assert!(text.contains("PgUp 上翻"), "{text}");
        assert_eq!(wide.selectable, Selectable::None);

        // 窄屏：整条丢，末尾补省略号，不撕裂某条提示。
        let narrow = ShortcutBar::render(&ctx(&theme, 18, None), &keymap, KeyOwner::Prompt);
        assert!(
            width::display_width(&narrow.text()) <= 19,
            "{:?}",
            narrow.text()
        );
        assert!(narrow.text().ends_with('…'));

        // 审批卡在时栏里是权限键。
        let approval = ShortcutBar::render(&ctx(&theme, 80, None), &keymap, KeyOwner::Approval);
        assert!(
            approval.text().contains("y 允许一次"),
            "{}",
            approval.text()
        );
        assert!(!approval.text().contains("Enter 发送"));
    }
}
