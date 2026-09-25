//! 回滚区：块 → 行 → 可见窗口切片。
//!
//! 设计来源：`docs/grok-tui-design.md` §4.2 与专题
//! `docs/research/grok-tui/09-scrollback-block-model.md`（块摊平成已换行的行、块间
//! `gap_after` 为 0 或 1、渲染只取可见窗口）。
//!
//! 与上游的差异：上游为**每个条目**维护多层缓存；这里只做**一层**摊平缓存（够用又不复杂）。
//!
//! 缓存不是白付的复杂度，是实测换来的：一次摊平要把每个块重新排版（助手块要跑一遍 markdown
//! 解析 + 折行），真调里 ~4900 行的会话量到 **18ms/次**；而拖动选文本时每个鼠标事件要换算一次
//! 坐标、每帧渲染还要再算一次，加起来就是「选择框不跟手」。缓存键 = 内容版本号 + 块宽 +
//! 字形档 + 动画帧号（决定转圈的帧）；主题假定进程内固定（`Capabilities` 只给一份）。

use std::cell::RefCell;
use std::rc::Rc;

use crate::blocks::{Block, BlockCtx, BlockLine, Selectable, ToolStatus};
use crate::glyphs;
use crate::glyphs::Glyphs;
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

/// 摊平结果缓存（一份）。
#[derive(Debug, Clone)]
struct LayoutCache {
    /// 内容版本号：任何一次块变更都会 +1。
    revision: u64,
    /// 块宽（决定折行）。
    width: usize,
    /// 字形档（花体/回退的标记宽度可能不同）。
    glyphs: Glyphs,
    /// 动画帧号（决定转圈的帧）。
    frame: usize,
    /// 摊平后的行。用 `Rc` 共享：命中缓存时不做深拷贝。
    lines: Rc<Vec<BlockLine>>,
}

/// 块序列 + 滚动位置（距底部行数，0 为贴底跟随）。
#[derive(Debug, Clone, Default)]
pub struct Scrollback {
    blocks: Vec<Block>,
    offset_from_bottom: usize,
    /// 内容版本号（缓存判据）：`push` / `clear` / `last_mut` 都会 +1。
    revision: u64,
    /// 摊平缓存。`RefCell` 是为了让 `layout(&self)` 这类只读接口也能回填。
    cache: RefCell<Option<LayoutCache>>,
}

impl Scrollback {
    pub fn new() -> Self {
        Scrollback::default()
    }

    /// 追加块：原本贴底就继续贴底（新输出自动可见），已经翻上去就不动视口。
    pub fn push(&mut self, block: Block) {
        self.revision = self.revision.wrapping_add(1);
        self.blocks.push(block);
    }

    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    /// 末尾块的就地写权限：流式助手文本要接着写，工具块执行完要回填状态与结果摘要。
    pub fn last_mut(&mut self) -> Option<&mut Block> {
        // 拿可变引用就是要改内容（流式追加）：版本号先动，免得缓存留着旧排版。
        self.revision = self.revision.wrapping_add(1);
        self.blocks.last_mut()
    }

    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    /// 清空回滚区（`/clear`）：会话上下文与语义状态都不动，只把可见历史收走。
    pub fn clear(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.blocks.clear();
        self.offset_from_bottom = 0;
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
    ///
    /// 命中缓存时直接返回同一份 `Rc`（不做深拷贝）；渲染参数或内容一变就重算。
    pub fn layout(&self, ctx: &BlockCtx) -> Rc<Vec<BlockLine>> {
        let key = self.cache_key(ctx);
        if let Some(cache) = self.cache.borrow().as_ref()
            && cache.revision == key.revision
            && cache.width == key.width
            && cache.glyphs == key.glyphs
            && cache.frame == key.frame
        {
            return Rc::clone(&cache.lines);
        }
        let mut lines = Vec::new();
        for (index, block) in self.blocks.iter().enumerate() {
            if index > 0 {
                lines.push(BlockLine::new(Line::default(), Selectable::None));
            }
            lines.extend(block.output(ctx).lines);
        }
        let lines = Rc::new(lines);
        *self.cache.borrow_mut() = Some(LayoutCache {
            revision: key.revision,
            width: key.width,
            glyphs: key.glyphs,
            frame: key.frame,
            lines: Rc::clone(&lines),
        });
        lines
    }

    /// 缓存键：**只有真有转圈中的工具块**才把动画帧号算进去。
    ///
    /// 帧号每个 tick 都变；无脑把它算进键，等于工具一转圈、长会话的缓存就每帧失效
    /// （又回到 18ms/帧）。没有动画块时帧号对排版毫无影响，钉成 0 让缓存跨 tick 有效。
    fn cache_key(&self, ctx: &BlockCtx) -> LayoutCache {
        let animated = self
            .blocks
            .iter()
            .any(|block| matches!(block, Block::Tool(tool) if tool.status == ToolStatus::Running));
        LayoutCache {
            revision: self.revision,
            width: ctx.width,
            glyphs: ctx.glyphs,
            frame: if animated { ctx.frame } else { 0 },
            lines: Rc::new(Vec::new()),
        }
    }

    /// 可见窗口的几何：`(总行数, 窗口首行的摊平行号, 窗口行数)`。
    ///
    /// 鼠标命中换算只认这三个数——不必把窗口那几十行也拷出来（拖动时每个鼠标事件都要换算）。
    pub fn geometry(&self, ctx: &BlockCtx, height: usize) -> (usize, usize, usize) {
        let lines = self.layout(ctx);
        let total = lines.len();
        let offset = self.offset_from_bottom.min(total.saturating_sub(height));
        let end = total - offset;
        let start = end.saturating_sub(height);
        (total, start, end - start)
    }

    /// 摊平后的总行数。
    pub fn total_rows(&self, ctx: &BlockCtx) -> usize {
        self.layout(ctx).len()
    }

    /// 摊平缓存当前是否命中（单测与性能回归用）。
    pub fn cache_hits(&self, ctx: &BlockCtx) -> bool {
        let key = self.cache_key(ctx);
        self.cache.borrow().as_ref().is_some_and(|cache| {
            cache.revision == key.revision
                && cache.width == key.width
                && cache.glyphs == key.glyphs
                && cache.frame == key.frame
        })
    }

    /// 最大可滚动行数（内容不足一屏时为 0）。
    pub fn max_offset(&self, ctx: &BlockCtx, height: usize) -> usize {
        self.total_rows(ctx).saturating_sub(height)
    }

    /// 切出可见窗口：从底部往上数 `offset` 行，取 `height` 行。
    pub fn window(&self, ctx: &BlockCtx, height: usize) -> ScrollWindow {
        let lines = self.layout(ctx);
        let (total, start, len) = self.geometry(ctx, height);
        ScrollWindow {
            // 只拷可见窗口那几十行（整份摊平结果是共享的）。
            lines: lines[start..start + len].to_vec(),
            offset: total - (start + len),
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

    /// 摊平缓存：同一份内容重复问只算一次（拖动选文本时每个鼠标事件都要问一次）。
    #[test]
    fn the_layout_cache_is_reused_until_the_content_changes() {
        let theme = Theme::DARK;
        let block_ctx = ctx(&theme);
        let mut scrollback = Scrollback::new();
        scrollback.push(Block::assistant("一行字"));

        let first = scrollback.layout(&block_ctx);
        let second = scrollback.layout(&block_ctx);
        assert!(
            std::rc::Rc::ptr_eq(&first, &second),
            "内容没变就该复用同一份摊平结果"
        );
        assert!(scrollback.cache_hits(&block_ctx));

        // 内容一变（追加块）就必须重算，否则会拿着旧排版画新内容
        scrollback.push(Block::assistant("又一行"));
        let third = scrollback.layout(&block_ctx);
        assert!(!std::rc::Rc::ptr_eq(&first, &third), "追加块后要重算");
        assert_eq!(
            third.len(),
            first.len() + 2,
            "多一块 = 多一行 + 一条块间空行"
        );

        // 可变引用（流式追加正文）同样算内容变化
        if let Some(block) = scrollback.last_mut() {
            *block = Block::assistant("又一行，而且更长一些");
        }
        let fourth = scrollback.layout(&block_ctx);
        assert!(!std::rc::Rc::ptr_eq(&third, &fourth), "改过最后一块要重算");

        // 宽度变了（终端缩放）也要重算
        let mut narrow = ctx(&theme);
        narrow.width = 10;
        let fifth = scrollback.layout(&narrow);
        assert!(!std::rc::Rc::ptr_eq(&fourth, &fifth), "块宽变了要重算");
    }

    /// 帧号只在**有转圈块**时参与缓存键：空闲会话跨 tick 命中，转圈时每帧重算（spinner 要动）。
    #[test]
    fn the_frame_number_only_matters_while_something_animates() {
        let theme = Theme::DARK;
        let mut scrollback = Scrollback::new();
        scrollback.push(Block::assistant("静止的一行"));
        let mut first = ctx(&theme);
        first.frame = 3;
        let second = ctx(&theme);
        let second = BlockCtx { frame: 9, ..second };

        let idle_a = scrollback.layout(&first);
        let idle_b = scrollback.layout(&second);
        assert!(
            std::rc::Rc::ptr_eq(&idle_a, &idle_b),
            "没有动画块时换帧号不该重算"
        );

        // 加一个「执行中」的工具块：帧号必须进键，否则 spinner 会卡住不动
        scrollback.push(Block::tool("bash", "cargo build", ToolStatus::Running));
        let running_a = scrollback.layout(&first);
        let running_b = scrollback.layout(&second);
        assert!(
            !std::rc::Rc::ptr_eq(&running_a, &running_b),
            "有转圈块时换帧号要重算"
        );
    }

    /// `geometry()` 与 `window()` 必须给同一套坐标（命中换算只读前者）。
    #[test]
    fn geometry_matches_the_window_it_describes() {
        let theme = Theme::DARK;
        let block_ctx = ctx(&theme);
        let mut scrollback = Scrollback::new();
        for index in 0..8 {
            scrollback.push(Block::assistant(format!("第 {index} 块")));
        }
        for height in [1usize, 3, 30] {
            for offset in [0usize, 2, 100] {
                scrollback.scroll_up(offset, usize::MAX);
                let window = scrollback.window(&block_ctx, height);
                let (total, start, len) = scrollback.geometry(&block_ctx, height);
                assert_eq!(total, window.total);
                assert_eq!(len, window.lines.len());
                assert_eq!(total - (start + len), window.offset);
                scrollback.scroll_to_bottom();
            }
        }
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
