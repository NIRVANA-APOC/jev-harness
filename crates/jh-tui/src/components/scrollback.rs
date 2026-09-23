//! 回滚区：块 → 行 → 可见窗口切片。
//!
//! 设计来源：`docs/grok-tui-design.md` §4.2 与专题
//! `docs/research/grok-tui/09-scrollback-block-model.md`（块摊平成已换行的行、块间
//! `gap_after` 为 0 或 1、渲染只取可见窗口）。
//!
//! 与上游的差异：上游为每条目维护多层缓存（`ensure_cached(content_width)`），本项目
//! 每帧重新摊平——行数规模小一个数量级，缓存是白付的复杂度（设计 §7「量级要砍」）。

use crate::blocks::{Block, BlockCtx, BlockLine, Selectable};
use crate::glyphs;
use crate::width;
use ratatui::text::{Line, Span};

/// 可见窗口切片。
#[derive(Debug, Clone, PartialEq)]
pub struct ScrollWindow {
    /// 本帧真正要画的行。
    pub lines: Vec<BlockLine>,
    /// 实际生效的「距底部行数」（已按内容高度收敛）。
    pub offset: usize,
    /// 摊平后的总行数。
    pub total: usize,
}

impl ScrollWindow {
    /// 是否贴在底部（跟随最新输出）。
    pub fn at_bottom(&self) -> bool {
        self.offset == 0
    }
}

/// 滚动提示：不贴底时在回滚区最后一行居中画一个下箭头。
///
/// 形状对齐参考截图 `main_ui2.png` 底部居中的 `▼`；贴底（跟随最新输出）时整行让给内容。
#[derive(Debug, Clone, Copy, Default)]
pub struct ScrollHint;

impl ScrollHint {
    /// 渲染提示行；贴底或宽度不足时返回 `None`。
    pub fn render(window: &ScrollWindow, ctx: &BlockCtx) -> Option<BlockLine> {
        if window.at_bottom() {
            return None;
        }
        let glyph = ctx.glyphs.resolve(glyphs::scroll_hint());
        let used = width::display_width(glyph);
        if used == 0 || ctx.width < used {
            return None;
        }
        let left = (ctx.width - used) / 2;
        let right = ctx.width - used - left;
        Some(BlockLine::new(
            Line::from(vec![
                Span::styled(" ".repeat(left), ctx.theme.dim()),
                Span::styled(glyph.to_owned(), ctx.theme.muted()),
                Span::styled(" ".repeat(right), ctx.theme.dim()),
            ]),
            Selectable::None,
        ))
    }
}

/// 块序列 + 滚动位置（距底部行数，0 为贴底跟随）。
#[derive(Debug, Clone, Default)]
pub struct Scrollback {
    blocks: Vec<Block>,
    offset_from_bottom: usize,
}

impl Scrollback {
    pub fn new() -> Self {
        Scrollback::default()
    }

    /// 追加块：原本贴底就继续贴底（新输出自动可见），已经翻上去就不动视口。
    pub fn push(&mut self, block: Block) {
        self.blocks.push(block);
    }

    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    /// 末尾块的就地写权限：流式助手文本要接着写，工具块执行完要回填状态与结果摘要。
    pub fn last_mut(&mut self) -> Option<&mut Block> {
        self.blocks.last_mut()
    }

    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    /// 距底部行数（原始值，未经内容高度收敛）。
    pub fn offset_from_bottom(&self) -> usize {
        self.offset_from_bottom
    }

    /// 是否贴在底部。
    pub fn is_at_bottom(&self) -> bool {
        self.offset_from_bottom == 0
    }

    /// 摊平全部行：块之间插一条不可选的空行（上游 `gap_after`）。
    pub fn layout(&self, ctx: &BlockCtx) -> Vec<BlockLine> {
        let mut lines = Vec::new();
        for (index, block) in self.blocks.iter().enumerate() {
            if index > 0 {
                lines.push(BlockLine::new(Line::default(), Selectable::None));
            }
            lines.extend(block.output(ctx).lines);
        }
        lines
    }

    /// 摊平后的总行数。
    pub fn total_rows(&self, ctx: &BlockCtx) -> usize {
        self.layout(ctx).len()
    }

    /// 最大可滚动行数（内容不足一屏时为 0）。
    pub fn max_offset(&self, ctx: &BlockCtx, height: usize) -> usize {
        self.total_rows(ctx).saturating_sub(height)
    }

    /// 切出可见窗口：从底部往上数 `offset` 行，取 `height` 行。
    pub fn window(&self, ctx: &BlockCtx, height: usize) -> ScrollWindow {
        let lines = self.layout(ctx);
        let total = lines.len();
        let offset = self.offset_from_bottom.min(total.saturating_sub(height));
        let end = total - offset;
        let start = end.saturating_sub(height);
        ScrollWindow {
            lines: lines[start..end].to_vec(),
            offset,
            total,
        }
    }

    /// 上滚若干行（视口向上移动，看更早的内容）。
    pub fn scroll_up(&mut self, rows: usize, max_offset: usize) {
        self.offset_from_bottom = (self.offset_from_bottom + rows).min(max_offset);
    }

    /// 下滚若干行。
    pub fn scroll_down(&mut self, rows: usize) {
        self.offset_from_bottom = self.offset_from_bottom.saturating_sub(rows);
    }

    /// 回到底部并继续跟随输出。
    pub fn scroll_to_bottom(&mut self) {
        self.offset_from_bottom = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::{Block, ToolStatus};
    use crate::glyphs::Glyphs;
    use crate::theme::Theme;

    fn ctx<'a>(theme: &'a Theme) -> BlockCtx<'a> {
        BlockCtx {
            width: 24,
            theme,
            glyphs: Glyphs::FANCY,
            frame: 0,
            decision: None,
        }
    }

    #[test]
    fn scroll_hint_appears_only_when_not_at_bottom() {
        let theme = Theme::DARK;
        let ctx = ctx(&theme);
        let mut scrollback = Scrollback::new();
        for text in ["a", "b", "c", "d", "e"] {
            scrollback.push(Block::user(text));
        }
        let bottom = scrollback.window(&ctx, 3);
        assert!(ScrollHint::render(&bottom, &ctx).is_none(), "贴底不画提示");

        scrollback.scroll_up(2, scrollback.max_offset(&ctx, 3));
        let scrolled = scrollback.window(&ctx, 3);
        let hint = ScrollHint::render(&scrolled, &ctx).expect("上滚后应有提示");
        assert_eq!(hint.text().trim(), "▼");
        assert_eq!(width::display_width(&hint.text()), ctx.width);
        assert_eq!(hint.selectable, Selectable::None);
    }

    #[test]
    fn layout_inserts_one_gap_row_between_blocks() {
        let theme = Theme::DARK;
        let ctx = ctx(&theme);
        let mut scrollback = Scrollback::new();
        scrollback.push(Block::user("一"));
        scrollback.push(Block::assistant("二"));
        // 2 块 × 1 行 + 1 条间隔行。
        assert_eq!(scrollback.total_rows(&ctx), 3);
        let lines = scrollback.layout(&ctx);
        assert_eq!(lines[1].selectable, Selectable::None, "间隔行不可选");
        assert_eq!(lines[1].text(), "");
    }

    #[test]
    fn window_is_bottom_anchored_and_slices_by_height() {
        let theme = Theme::DARK;
        let ctx = ctx(&theme);
        let mut scrollback = Scrollback::new();
        for text in ["a", "b", "c", "d"] {
            scrollback.push(Block::user(text));
        }
        // 4 行内容 + 3 条间隔 = 7 行；窗口 3 行时只看到最后 3 行。
        assert_eq!(scrollback.total_rows(&ctx), 7);
        let window = scrollback.window(&ctx, 3);
        assert_eq!(window.total, 7);
        assert!(window.at_bottom());
        assert_eq!(window.lines.len(), 3);
        // 用户块铺满整行（填充条），比对时只看正文。
        assert_eq!(window.lines[2].text().trim_end(), "❯ d");

        // 上滚 2 行 → 窗口整体上移。
        scrollback.scroll_up(2, scrollback.max_offset(&ctx, 3));
        let scrolled = scrollback.window(&ctx, 3);
        assert_eq!(scrolled.offset, 2);
        assert!(!scrolled.at_bottom());
        assert_eq!(scrolled.lines[2].text().trim_end(), "❯ c");

        // 回到底部。
        scrollback.scroll_to_bottom();
        assert_eq!(scrollback.window(&ctx, 3).offset, 0);
    }

    #[test]
    fn scroll_offset_is_clamped_to_content_height() {
        let theme = Theme::DARK;
        let ctx = ctx(&theme);
        let mut scrollback = Scrollback::new();
        scrollback.push(Block::user("only"));
        // 内容只有 1 行、窗口 5 行：没有可滚动空间。
        assert_eq!(scrollback.max_offset(&ctx, 5), 0);
        scrollback.scroll_up(99, scrollback.max_offset(&ctx, 5));
        let window = scrollback.window(&ctx, 5);
        assert_eq!(window.offset, 0);
        assert_eq!(window.lines.len(), 1);
    }

    #[test]
    fn offset_is_measured_from_the_bottom() {
        let theme = Theme::DARK;
        let ctx = ctx(&theme);
        let mut scrollback = Scrollback::new();
        for text in ["a", "b", "c"] {
            scrollback.push(Block::user(text));
        }
        scrollback.scroll_up(2, scrollback.max_offset(&ctx, 2));
        let offset = scrollback.offset_from_bottom();
        // 原始语义：偏移是「距底部的行数」。追加新内容会让同一个偏移落在更早的内容上，
        // 「翻上去看历史时锚住视口」由上层负责（见 `App::push_block`）。
        scrollback.push(Block::tool("bash", "ls", ToolStatus::Ok));
        assert_eq!(scrollback.offset_from_bottom(), offset);
        let window = scrollback.window(&ctx, 2);
        assert_eq!(window.total, 7, "4 行 + 3 条间隔");
        assert!(!window.at_bottom());
    }

    #[test]
    fn every_block_kind_renders_into_the_window() {
        let theme = Theme::DARK;
        let ctx = ctx(&theme);
        let mut scrollback = Scrollback::new();
        scrollback.push(Block::user("看下仓库状态"));
        scrollback.push(Block::assistant("好，我先跑一下"));
        scrollback.push(Block::tool("bash", "git status", ToolStatus::Ok));
        scrollback.push(Block::decision(crate::blocks::DecisionTrace::new(
            "shell_command_allowed",
            vec![crate::blocks::Choice::new("usr:allow", "允许", 0.71)],
        )));

        let text: String = scrollback
            .layout(&ctx)
            .iter()
            .map(BlockLine::text)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("看下仓库状态"), "{text}");
        assert!(text.contains("好，我先跑一下"), "{text}");
        assert!(text.contains("bash"), "{text}");
        assert!(text.contains("shell_command_allowed"), "{text}");
        assert!(text.contains("0.71"), "{text}");
    }
}
