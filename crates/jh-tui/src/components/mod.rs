//! 组件层：回滚区 / 提示框 / 状态行 / 快捷键栏 / 审批卡。
//!
//! 组件之间不互相调用，数据都来自参数（块上下文、动作注册表、状态模型）；
//! 唯一被共享的排版工具是 [`clip_spans`]——按显示列裁剪带样式的一行。

pub mod approval;
pub mod prompt;
pub mod scrollback;
pub mod status;

pub use approval::{ActiveModal, ApprovalCard, ApprovalOption, ApprovalRequest};
pub use prompt::{MAX_PROMPT_ROWS, Prompt, TextCursor};
pub use scrollback::{ScrollWindow, Scrollback};
pub use status::{ShortcutBar, StatusModel, TurnPhase, WaitingReason};

use ratatui::style::Style;
use ratatui::text::Span;

use crate::width;

/// 按显示列裁剪一组带样式的 span：超宽时裁掉尾部的簇并在末尾补省略号，
/// 省略号沿用被抓到的那个 span 的样式。宽度不足时不会劈开宽字符。
pub fn clip_spans(spans: Vec<Span<'static>>, max: usize, ellipsis: &str) -> Vec<Span<'static>> {
    if max == 0 {
        return Vec::new();
    }
    let text: String = spans.iter().map(|span| span.content.as_ref()).collect();
    if width::display_width(&text) <= max {
        return spans;
    }
    let budget = max.saturating_sub(width::display_width(ellipsis));
    let mut out = Vec::new();
    let mut used = 0usize;
    let mut style = Style::new();
    for span in spans {
        style = span.style;
        let content = span.content.to_string();
        if used + width::display_width(&content) <= budget {
            used += width::display_width(&content);
            out.push(span);
            continue;
        }
        let head = width::clip(&content, budget - used).to_owned();
        if !head.is_empty() {
            out.push(Span::styled(head, span.style));
        }
        break;
    }
    out.push(Span::styled(ellipsis.to_owned(), style));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;
    use ratatui::style::Modifier;

    #[test]
    fn clip_spans_keeps_short_lines_untouched() {
        let spans = vec![Span::styled("abc".to_owned(), Theme::DARK.fg())];
        assert_eq!(clip_spans(spans.clone(), 5, "…"), spans);
    }

    #[test]
    fn clip_spans_preserves_style_and_reserves_ellipsis_column() {
        let theme = Theme::DARK;
        let spans = vec![
            Span::styled("ab".to_owned(), theme.fg()),
            Span::styled("cdef".to_owned(), theme.accent(theme.accent_error)),
        ];
        let clipped = clip_spans(spans, 4, "…");
        let text: String = clipped.iter().map(|span| span.content.as_ref()).collect();
        assert_eq!(text, "abc…");
        assert_eq!(width::display_width(&text), 4);
        // 省略号沿用被截断那个 span 的样式。
        assert_eq!(clipped.last().unwrap().style.fg, Some(theme.accent_error));
    }

    #[test]
    fn clip_spans_never_splits_a_wide_cluster() {
        let spans = vec![Span::styled("中文中文".to_owned(), Theme::DARK.fg())];
        let clipped = clip_spans(spans, 5, "…");
        let text: String = clipped.iter().map(|span| span.content.as_ref()).collect();
        assert_eq!(text, "中文…");
        assert_eq!(width::display_width(&text), 5);
    }

    #[test]
    fn clip_spans_works_with_zero_budget() {
        let spans = vec![Span::styled(
            "abc".to_owned(),
            Style::new().add_modifier(Modifier::BOLD),
        )];
        let clipped = clip_spans(spans, 0, "…");
        let text: String = clipped.iter().map(|span| span.content.as_ref()).collect();
        assert_eq!(text, "");
    }
}
