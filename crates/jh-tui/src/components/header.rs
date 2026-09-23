//! 顶栏：一行，左侧标识、右侧用量与入口。
//!
//! 设计来源：`docs/tui-design.md` §5.1 与 §3 的截图对照——参考截图的顶栏是
//! 「左工作目录 + 右用量 `18K / 1.0M` + `[Dashboard]` 入口」，这里保留形状、
//! 换成本项目的三个字段（标识 / 上下文用量 / 研究视图入口）。

use ratatui::text::{Line, Span};

use crate::blocks::{BlockCtx, BlockLine, Selectable};
use crate::components::{clip_spans, format_tokens};
use crate::glyphs;
use crate::width;

/// 顶栏数据：左边一个标识，右边最多两段（用量、入口徽章）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeaderModel {
    /// 左侧标识：工作目录（`~` 缩写）或会话标题。
    pub title: String,
    /// 上下文用量 `(已用, 上限)`。
    pub tokens: Option<(u32, u32)>,
    /// 右侧入口徽章（例如 `[研究视图]`）。
    pub badge: Option<String>,
}

impl HeaderModel {
    pub fn new(title: impl Into<String>) -> Self {
        HeaderModel {
            title: title.into(),
            tokens: None,
            badge: None,
        }
    }

    pub fn with_tokens(mut self, used: u32, limit: u32) -> Self {
        self.tokens = Some((used, limit));
        self
    }

    pub fn with_badge(mut self, badge: impl Into<String>) -> Self {
        self.badge = Some(badge.into());
        self
    }
}

/// 顶栏组件：本身无状态，数据全在 [`HeaderModel`]。
#[derive(Debug, Clone, Copy, Default)]
pub struct HeaderBar;

impl HeaderBar {
    /// 渲染一行顶栏：左侧标识按显示列裁剪，右侧两段（用量、徽章）右对齐。
    pub fn render(model: &HeaderModel, ctx: &BlockCtx) -> BlockLine {
        let separator = ctx.glyphs.resolve(glyphs::separator());

        let mut right: Vec<Span<'static>> = Vec::new();
        if let Some((used, limit)) = model.tokens {
            right.push(Span::styled(format_tokens(used, limit), ctx.theme.dim()));
        }
        if let Some(badge) = &model.badge {
            if !right.is_empty() {
                right.push(Span::styled(format!(" {separator} "), ctx.theme.dim()));
            }
            right.push(Span::styled(
                badge.clone(),
                ctx.theme.accent(ctx.theme.accent_system),
            ));
        }
        let right_width: usize = right
            .iter()
            .map(|span| width::display_width(span.content.as_ref()))
            .sum();

        // 左侧至少留一列，其余让给右侧两段；标识太长时按显示列裁掉尾部。
        let left_budget = ctx.width.saturating_sub(right_width + 1);
        let title = width::clip_with_ellipsis(
            &model.title,
            left_budget,
            ctx.glyphs.resolve(glyphs::ellipsis()),
        );
        let title_width = width::display_width(&title);
        // 只有右侧有内容时才补间隙：没有右侧段时整行就是标识本身，不画尾随空白。
        let padding = if right_width > 0 {
            ctx.width.saturating_sub(title_width + right_width).max(1)
        } else {
            0
        };

        let mut spans = vec![Span::styled(title, ctx.theme.muted())];
        if padding > 0 {
            spans.push(Span::styled(" ".repeat(padding), ctx.theme.dim()));
        }
        spans.extend(right);

        BlockLine::new(
            Line::from(clip_spans(
                spans,
                ctx.width,
                ctx.glyphs.resolve(glyphs::ellipsis()),
            )),
            Selectable::All,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::glyphs::Glyphs;
    use crate::theme::Theme;

    fn ctx<'a>(theme: &'a Theme, width: usize) -> BlockCtx<'a> {
        BlockCtx {
            width,
            theme,
            glyphs: Glyphs::FANCY,
            frame: 0,
            decision: None,
        }
    }

    #[test]
    fn header_puts_title_left_and_segments_right() {
        let theme = Theme::DARK;
        let model = HeaderModel::new("~/Documents/jev-harness")
            .with_tokens(12_480, 64_000)
            .with_badge("[研究视图]");
        let line = HeaderBar::render(&model, &ctx(&theme, 60));
        let text = line.text();
        assert!(text.starts_with("~/Documents/jev-harness"), "{text}");
        assert!(text.ends_with("12.5k/64k · [研究视图]"), "{text}");
        assert_eq!(width::display_width(&text), 60, "顶栏铺满整行：{text:?}");
        assert_eq!(line.selectable, Selectable::All);
    }

    #[test]
    fn header_clips_the_title_before_the_right_segments() {
        let theme = Theme::DARK;
        let model = HeaderModel::new("~/very/long/workspace/path/that/keeps/going")
            .with_tokens(1, 2)
            .with_badge("[研究视图]");
        let text = HeaderBar::render(&model, &ctx(&theme, 24)).text();
        assert!(width::display_width(&text) <= 24, "{text:?}");
        assert!(text.contains("[研究视图]"), "右侧两段优先保住：{text}");
        assert!(text.contains('…'), "{text}");
    }

    #[test]
    fn header_without_segments_is_just_the_title() {
        let theme = Theme::DARK;
        let text = HeaderBar::render(&HeaderModel::new("会话 1"), &ctx(&theme, 40)).text();
        assert_eq!(text, "会话 1");
    }
}
