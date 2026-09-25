//! 提示框：多行输入 + 光标。
//!
//! 设计来源：`docs/grok-tui-design.md` §4.4 与专题
//! `docs/research/grok-tui/11-prompt-widget.md`（软折行是显示层缓存、纵向移动走**可视行**
//! 且带粘性目标列、`Enter` 提交而 `Shift+Enter` 与 `Ctrl+J` 插硬换行、光标位置由渲染
//! 结果回报给宿主）。
//!
//! 与上游的差异：
//! - 上游自建 `xai-ratatui-textarea`（3482 行，含 undo / kill buffer / chip / 滚动条）；
//!   这里只保留多行编辑与光标，不做 undo、不做元素 chip。
//! - 折行用**按显示列的硬折行**而不是词级重排：块级内容可以词级重排，输入框的
//!   光标列换算需要行边界可确定（宽字符整簇落行，不劈开）。
//! - 光标由宿主把终端光标移到 [`Prompt::cursor_cell`] 指的位置，不在缓冲区里画方块。

use unicode_segmentation::UnicodeSegmentation;

use crate::blocks::{BlockCtx, BlockLine, Selectable};
use crate::glyphs;
use crate::width;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

/// 提示框默认高度上限（可视行数），与上游 `area.height / 2` 的口径一致
/// （`app/agent_view/render.rs:806` 的 `max_prompt_height`）。
pub const MAX_PROMPT_ROWS: usize = 8;

/// 光标：逻辑行下标 + 该行内的 grapheme 簇下标。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextCursor {
    pub row: usize,
    pub column: usize,
}

/// 提示框的 chrome：顶边内嵌的标题与底边内嵌的信息行（`模型 · 判定器 · 授权模式`）。
///
/// 信息行为 `None` 时退回平铺形态（无边框）——窄终端与组件级调用用得上；
/// 主界面与首屏都给了信息行，因此都是四边盒。几何口径（边框 1 列 + 内边距 1 列）与
/// 面板壳一致。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PromptChrome {
    /// 顶边内嵌的标题（会话名）；`None` 表示顶边不嵌标题。
    pub title: Option<String>,
    /// 底边内嵌的信息行；`None` 表示不画盒子。
    pub info: Option<String>,
}

impl PromptChrome {
    /// 未接线时的信息行：如实标注当前状态，不假装接了模型（`AGENTS.md`「不虚构」）。
    pub fn unwired() -> Self {
        PromptChrome {
            title: None,
            info: Some("未接线 · 不调用模型与 API".to_owned()),
        }
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_info(mut self, info: impl Into<String>) -> Self {
        self.info = Some(info.into());
        self
    }

    /// 是否画四边盒：有信息行就画。
    pub fn boxed(&self) -> bool {
        self.info.is_some()
    }

    /// 内容区（含标记列）的列宽。
    pub fn inner_width(&self, area_width: usize) -> usize {
        if self.boxed() {
            area_width.saturating_sub(BOX_BORDERS + BOX_PAD * 2)
        } else {
            area_width
        }
    }

    /// 内容区相对区域原点的偏移 `(列, 行)`。
    pub fn content_origin(&self) -> (u16, u16) {
        if self.boxed() {
            ((BOX_BORDERS / 2 + BOX_PAD) as u16, 1)
        } else {
            (0, 0)
        }
    }

    /// 给定内容可视行数时，区域需要的总行数（盒子上下各一条边框）。
    pub fn rows(&self, content_rows: usize) -> u16 {
        let borders = if self.boxed() { BOX_BORDERS } else { 0 };
        (content_rows + borders) as u16
    }
}

/// 盒子左右各一条边框。
const BOX_BORDERS: usize = 2;
/// 边框与内容之间的内边距（左右各一列）。
const BOX_PAD: usize = 1;

/// 多行输入缓冲。
#[derive(Debug, Clone, PartialEq)]
pub struct Prompt {
    lines: Vec<String>,
    cursor: TextCursor,
    /// 纵向移动的粘性目标列（显示列），横向移动后失效。
    preferred_column: Option<usize>,
}

impl Default for Prompt {
    fn default() -> Self {
        Prompt::new()
    }
}

impl Prompt {
    pub fn new() -> Self {
        Prompt {
            lines: vec![String::new()],
            cursor: TextCursor::default(),
            preferred_column: None,
        }
    }

    pub fn with_text(text: &str) -> Self {
        let lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
        let cursor = TextCursor {
            row: lines.len() - 1,
            column: grapheme_count(lines.last().expect("至少一行")),
        };
        Prompt {
            lines,
            cursor,
            preferred_column: None,
        }
    }

    /// 提交后清空，返回提交出去的文本。
    pub fn take(&mut self) -> String {
        let text = self.text();
        *self = Prompt::new();
        text
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn is_empty(&self) -> bool {
        self.lines.len() == 1 && self.lines[0].is_empty()
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn cursor(&self) -> TextCursor {
        self.cursor
    }

    /// 插入一个字符（落在光标位置）。
    pub fn insert_char(&mut self, c: char) {
        let mut buffer = [0u8; 4];
        self.insert_str(c.encode_utf8(&mut buffer));
    }

    /// 插入一段文本（粘贴路径）：内部换行会被拆成多个逻辑行。
    pub fn insert_str(&mut self, text: &str) {
        let mut parts = text.split('\n');
        if let Some(first) = parts.next() {
            self.insert_into_current_line(first);
        }
        for part in parts {
            self.insert_newline();
            self.insert_into_current_line(part);
        }
        self.preferred_column = None;
    }

    fn insert_into_current_line(&mut self, text: &str) {
        let line = &mut self.lines[self.cursor.row];
        let at = byte_of_grapheme(line, self.cursor.column);
        line.insert_str(at, text);
        self.cursor.column += grapheme_count(text);
    }

    /// 在光标处断开当前逻辑行。
    pub fn insert_newline(&mut self) {
        let line = &mut self.lines[self.cursor.row];
        let at = byte_of_grapheme(line, self.cursor.column);
        let tail = line.split_off(at);
        let head_graphemes = grapheme_count(line);
        debug_assert_eq!(head_graphemes, self.cursor.column);
        self.lines.insert(self.cursor.row + 1, tail);
        self.cursor = TextCursor {
            row: self.cursor.row + 1,
            column: 0,
        };
        self.preferred_column = None;
    }

    /// 退格：删光标前的簇；行首则与上一行合并。
    pub fn backspace(&mut self) {
        if self.cursor.column > 0 {
            let line = &mut self.lines[self.cursor.row];
            let (start, end) = grapheme_range(line, self.cursor.column - 1).expect("簇存在");
            line.replace_range(start..end, "");
            self.cursor.column -= 1;
        } else if self.cursor.row > 0 {
            let tail = self.lines.remove(self.cursor.row);
            let previous = &mut self.lines[self.cursor.row - 1];
            let join_at = previous.len();
            previous.push_str(&tail);
            self.cursor = TextCursor {
                row: self.cursor.row - 1,
                column: grapheme_count(&previous[..join_at]),
            };
        }
        self.preferred_column = None;
    }

    /// 前向删除：删光标所在簇；行尾则与下一行合并。
    pub fn delete_forward(&mut self) {
        let line = &self.lines[self.cursor.row];
        if self.cursor.column < grapheme_count(line) {
            let line = &mut self.lines[self.cursor.row];
            let (start, end) = grapheme_range(line, self.cursor.column).expect("簇存在");
            line.replace_range(start..end, "");
        } else if self.cursor.row + 1 < self.lines.len() {
            let tail = self.lines.remove(self.cursor.row + 1);
            self.lines[self.cursor.row].push_str(&tail);
        }
        self.preferred_column = None;
    }

    pub fn move_left(&mut self) {
        if self.cursor.column > 0 {
            self.cursor.column -= 1;
        } else if self.cursor.row > 0 {
            self.cursor.row -= 1;
            self.cursor.column = grapheme_count(&self.lines[self.cursor.row]);
        }
        self.preferred_column = None;
    }

    pub fn move_right(&mut self) {
        let line = &self.lines[self.cursor.row];
        if self.cursor.column < grapheme_count(line) {
            self.cursor.column += 1;
        } else if self.cursor.row + 1 < self.lines.len() {
            self.cursor.row += 1;
            self.cursor.column = 0;
        }
        self.preferred_column = None;
    }

    /// 逻辑行首。
    pub fn move_home(&mut self) {
        self.cursor.column = 0;
        self.preferred_column = None;
    }

    /// 逻辑行尾。
    pub fn move_end(&mut self) {
        self.cursor.column = grapheme_count(&self.lines[self.cursor.row]);
        self.preferred_column = None;
    }

    /// 上移一个**可视行**（宽度变了也要落在同一显示列上，粘性目标列）。
    pub fn move_up(&mut self, content_width: usize) {
        let target = self
            .preferred_column
            .unwrap_or_else(|| self.visual_column(content_width));
        let line = self.lines[self.cursor.row].clone();
        let rows = visual_rows(&line, content_width);
        let row_index = row_of_grapheme(&rows, self.cursor.column);
        if row_index > 0 {
            self.cursor.column = grapheme_at_column(&line, &rows[row_index - 1], target);
        } else if self.cursor.row > 0 {
            self.cursor.row -= 1;
            let line = self.lines[self.cursor.row].clone();
            let rows = visual_rows(&line, content_width);
            self.cursor.column = grapheme_at_column(&line, &rows[0], target);
        } else {
            // 已在首行首个可视行：夹到目标列而不是跳到行首，免得按一下上键就丢位置。
            self.cursor.column = grapheme_at_column(&line, &rows[0], target);
        }
        self.preferred_column = Some(target);
    }

    /// 下移一个可视行。
    pub fn move_down(&mut self, content_width: usize) {
        let target = self
            .preferred_column
            .unwrap_or_else(|| self.visual_column(content_width));
        let line = self.lines[self.cursor.row].clone();
        let rows = visual_rows(&line, content_width);
        let row_index = row_of_grapheme(&rows, self.cursor.column);
        if row_index + 1 < rows.len() {
            self.cursor.column = grapheme_at_column(&line, &rows[row_index + 1], target);
        } else if self.cursor.row + 1 < self.lines.len() {
            self.cursor.row += 1;
            let line = self.lines[self.cursor.row].clone();
            let rows = visual_rows(&line, content_width);
            self.cursor.column = grapheme_at_column(&line, &rows[0], target);
        } else {
            let line = self.lines[self.cursor.row].clone();
            let rows = visual_rows(&line, content_width);
            self.cursor.column = grapheme_at_column(&line, &rows[rows.len() - 1], target);
        }
        self.preferred_column = Some(target);
    }

    /// 光标在其**可视行内**的显示列（纵向移动的粘性目标列就是这个数）。
    pub fn visual_column(&self, content_width: usize) -> usize {
        let line = &self.lines[self.cursor.row];
        let rows = visual_rows(line, content_width);
        let row = rows[row_of_grapheme(&rows, self.cursor.column)];
        width_of_graphemes(line, row.0, self.cursor.column)
    }

    /// 摊平后的可视行数（渲染与布局都要用）。
    pub fn visual_rows(&self, content_width: usize) -> usize {
        self.lines
            .iter()
            .map(|line| visual_rows(line, content_width).len())
            .sum()
    }

    /// 光标在提示框区域内的 (行, 列)。调用方加上区域原点即可定位终端光标。
    pub fn cursor_cell(&self, content_width: usize) -> (usize, usize) {
        let mut row_offset = 0;
        for (index, line) in self.lines.iter().enumerate() {
            let rows = visual_rows(line, content_width);
            if index == self.cursor.row {
                let row_index = row_of_grapheme(&rows, self.cursor.column);
                let row = rows[row_index];
                return (
                    row_offset + row_index,
                    width_of_graphemes(line, row.0, self.cursor.column),
                );
            }
            row_offset += rows.len();
        }
        (row_offset, 0)
    }

    /// 渲染提示框（已换行的屏幕行）。
    ///
    /// `chrome.info` 为 `None` 时是平铺形态（标记列 + 内容列）；给了信息行就画四边盒，
    /// 标题内嵌顶边、信息行内嵌底边（与面板壳同一个形状，见 `docs/tui-design.md` §5.1）。
    pub fn render(
        &self,
        ctx: &BlockCtx,
        chrome: &PromptChrome,
        focused: bool,
        hints: &str,
    ) -> Vec<BlockLine> {
        if !chrome.boxed() {
            return self.content_lines(ctx, focused, hints);
        }
        let inner = BlockCtx {
            width: chrome.inner_width(ctx.width),
            ..*ctx
        };
        let border = ctx.theme.frame(if focused {
            ctx.theme.prompt_border_active
        } else {
            ctx.theme.prompt_border
        });
        let lines = self.content_lines(&inner, focused, hints);
        let mut output = Vec::with_capacity(lines.len() + 2);
        output.push(border_line(ctx, border, chrome.title.as_deref(), true));
        for line in lines {
            output.push(wrap_row(ctx, line, border));
        }
        output.push(border_line(ctx, border, chrome.info.as_deref(), false));
        output
    }

    /// 平铺形态：标记列 + 内容列；空输入时画幽灵文案。
    fn content_lines(&self, ctx: &BlockCtx, focused: bool, hints: &str) -> Vec<BlockLine> {
        let marker = ctx.glyphs.resolve(glyphs::prompt());
        let prefix_width = width::display_width(marker);
        let content_width = ctx.content_width(prefix_width).max(1);
        let marker_style = if focused {
            ctx.theme.accent(ctx.theme.prompt_border_active)
        } else {
            ctx.theme.dim()
        };

        let mut output = Vec::new();
        for line in &self.lines {
            for (row_index, row) in visual_rows(line, content_width).into_iter().enumerate() {
                let head = if output.is_empty() && row_index == 0 {
                    marker.to_owned()
                } else {
                    " ".repeat(prefix_width)
                };
                let text = graphemes_in_range(line, row);
                let mut spans = vec![
                    Span::styled(head, marker_style),
                    Span::styled(text, ctx.theme.fg()),
                ];
                if self.is_empty() && output.is_empty() {
                    spans.push(Span::styled(
                        // 键位文案由调用方从 keymap 取（换行、粘贴都算）：键位改了、
                        // 或终端不支持 Shift+Enter 只能走 Ctrl+J 时，这句话跟着变，不写死。
                        // 一行放不下太多：`❯` 已经表明这是输入框，于是省掉「输入消息，」，
                        // 把位置让给粘贴键（Ctrl+V 是终端的事，不说用户会以为程序坏了）。
                        format!("Enter 发送，{hints} · /help 看命令"),
                        ctx.theme.dim(),
                    ));
                }
                output.push(BlockLine::new(
                    Line::from(spans),
                    Selectable::Spans(prefix_width as u16..ctx.width as u16),
                ));
            }
        }
        if output.is_empty() {
            output.push(BlockLine::new(
                Line::from(Span::styled(marker.to_owned(), marker_style)),
                Selectable::None,
            ));
        }
        output
    }
}

/// 盒子的一条边框：顶边内嵌标题、底边内嵌信息行，都左对齐（与面板壳同一个形状）。
fn border_line(ctx: &BlockCtx, border: Style, label: Option<&str>, top: bool) -> BlockLine {
    let horizontal = ctx.glyphs.resolve(glyphs::box_horizontal());
    let (open, close) = if top {
        (
            ctx.glyphs.resolve(glyphs::box_top_left()),
            ctx.glyphs.resolve(glyphs::box_top_right()),
        )
    } else {
        (
            ctx.glyphs.resolve(glyphs::box_bottom_left()),
            ctx.glyphs.resolve(glyphs::box_bottom_right()),
        )
    };

    let mut spans = vec![Span::styled(open.to_owned(), border)];
    if let Some(label) = label {
        // 标签最大列宽：整行去掉两角与两侧的 `─ ` / ` ─` 共 6 列。
        let label_budget = ctx.width.saturating_sub(6).max(1);
        let label =
            width::clip_with_ellipsis(label, label_budget, ctx.glyphs.resolve(glyphs::ellipsis()));
        spans.push(Span::styled(format!("{horizontal} "), border));
        spans.push(Span::styled(label, ctx.theme.dim()));
        spans.push(Span::styled(format!(" {horizontal}"), border));
    }
    let used: usize = spans
        .iter()
        .map(|span| width::display_width(span.content.as_ref()))
        .sum();
    let fill = ctx.width.saturating_sub(used + 1);
    if fill > 0 {
        spans.push(Span::styled(horizontal.repeat(fill), border));
    }
    spans.push(Span::styled(close.to_owned(), border));
    BlockLine::new(Line::from(spans), Selectable::None)
}

/// 给一条内容行套上盒子的左右边框，并把可选列区间整体右移。
fn wrap_row(ctx: &BlockCtx, line: BlockLine, border: Style) -> BlockLine {
    let vertical = ctx.glyphs.resolve(glyphs::box_vertical());
    let inner = ctx.width.saturating_sub(BOX_BORDERS + BOX_PAD * 2);
    let used = width::display_width(&line.text());
    let mut spans = vec![Span::styled(
        format!("{vertical}{}", " ".repeat(BOX_PAD)),
        border,
    )];
    spans.extend(line.content.spans);
    if used < inner {
        spans.push(Span::styled(" ".repeat(inner - used), Style::new()));
    }
    spans.push(Span::styled(
        format!("{}{vertical}", " ".repeat(BOX_PAD)),
        border,
    ));
    let selectable = match line.selectable {
        Selectable::Spans(range) => Selectable::Spans(range.start + 2..range.end + 2),
        other => other,
    };
    let wrapped = BlockLine::new(Line::from(spans), selectable);
    if line.joiner.is_some() {
        wrapped.soft_wrapped()
    } else {
        wrapped
    }
}

/// grapheme 簇个数。
pub fn grapheme_count(s: &str) -> usize {
    s.graphemes(true).count()
}

/// 第 `index` 个簇的起始字节偏移；越界返回 `s.len()`。
pub fn byte_of_grapheme(s: &str, index: usize) -> usize {
    s.grapheme_indices(true)
        .nth(index)
        .map_or(s.len(), |(at, _)| at)
}

/// 第 `index` 个簇的字节区间；越界返回 `None`。
pub fn grapheme_range(s: &str, index: usize) -> Option<(usize, usize)> {
    let mut iter = s.grapheme_indices(true);
    let (start, cluster) = iter.nth(index)?;
    Some((start, start + cluster.len()))
}

/// 拼出第 `[start, end)` 个簇的文本。
pub fn graphemes_in_range(s: &str, range: (usize, usize)) -> String {
    let from = byte_of_grapheme(s, range.0);
    let to = byte_of_grapheme(s, range.1);
    s[from..to].to_owned()
}

/// `[start, end)` 个簇的显示列宽。
pub fn width_of_graphemes(s: &str, start: usize, end: usize) -> usize {
    width::display_width(&graphemes_in_range(s, (start, end)))
}

/// 按显示列把一行切成可视行，返回每个可视行的簇区间（宽字符整簇落行，不劈开）。
pub fn visual_rows(line: &str, content_width: usize) -> Vec<(usize, usize)> {
    let width_limit = content_width.max(1);
    let mut rows = Vec::new();
    let mut start = 0;
    let mut used = 0;
    for (index, cluster) in line.graphemes(true).enumerate() {
        let w = width::cluster_width(cluster);
        if used + w > width_limit && index > start {
            rows.push((start, index));
            start = index;
            used = 0;
        }
        used += w;
    }
    rows.push((start, grapheme_count(line)));
    rows
}

/// 某个簇落在第几个可视行。
fn row_of_grapheme(rows: &[(usize, usize)], column: usize) -> usize {
    rows.iter()
        .position(|(start, end)| {
            column >= *start && column < *end || (*start == *end && column == *start)
        })
        .unwrap_or(rows.len() - 1)
}

/// 在某个可视行里取最接近目标显示列的簇下标。
fn grapheme_at_column(line: &str, row: &(usize, usize), target_column: usize) -> usize {
    let mut used = 0;
    for index in row.0..row.1 {
        let cluster = graphemes_in_range(line, (index, index + 1));
        if used + width::display_width(&cluster) > target_column {
            return index;
        }
        used += width::display_width(&cluster);
    }
    row.1
}

/// 测试里用的键位文案（生产路径由 keymap 给：换行 + 粘贴）。
#[cfg(test)]
const NEWLINE_HINT: &str = "Shift+Enter/Ctrl+J 换行 · Ctrl+Shift+V 粘贴";

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
    fn typing_edits_at_the_cursor() {
        let mut prompt = Prompt::new();
        prompt.insert_str("hello");
        assert_eq!(prompt.text(), "hello");
        prompt.move_left();
        prompt.insert_char('X');
        assert_eq!(prompt.text(), "hellXo", "插入落在光标处");
        assert_eq!(prompt.cursor(), TextCursor { row: 0, column: 5 });
    }

    #[test]
    fn cursor_moves_by_grapheme_cluster_not_by_char() {
        let mut prompt = Prompt::with_text("e\u{301}👩‍👩‍👧x");
        // 三个簇：e+组合符 / 家庭 emoji / x。
        assert_eq!(grapheme_count(&prompt.text()), 3);
        prompt.move_home();
        prompt.move_right();
        assert_eq!(prompt.cursor().column, 1);
        prompt.backspace();
        assert_eq!(prompt.text(), "👩‍👩‍👧x", "退格没有劈开组合符");
        prompt.move_left();
        assert_eq!(prompt.cursor().column, 0);
    }

    #[test]
    fn newline_splits_and_backspace_joins() {
        let mut prompt = Prompt::with_text("ab");
        prompt.move_home();
        prompt.move_right();
        prompt.insert_newline();
        assert_eq!(prompt.lines(), &["a".to_owned(), "b".to_owned()]);
        assert_eq!(prompt.cursor(), TextCursor { row: 1, column: 0 });
        prompt.backspace();
        assert_eq!(prompt.text(), "ab");
        assert_eq!(prompt.cursor(), TextCursor { row: 0, column: 1 });
        prompt.delete_forward();
        assert_eq!(prompt.text(), "a");
    }

    #[test]
    fn delete_forward_merges_the_next_line_at_end_of_line() {
        let mut prompt = Prompt::with_text("ab\ncd");
        prompt.move_up(4);
        assert_eq!(
            prompt.cursor(),
            TextCursor { row: 0, column: 2 },
            "上移到第一行行尾"
        );
        prompt.delete_forward();
        assert_eq!(prompt.text(), "abcd");
        assert_eq!(prompt.lines().len(), 1);
    }

    #[test]
    fn wide_chars_wrap_without_splitting_the_cluster() {
        // 内容宽度 4 → 每行两个汉字。
        let rows = visual_rows("中文中文", 4);
        assert_eq!(rows, vec![(0, 2), (2, 4)]);
        assert_eq!(graphemes_in_range("中文中文", rows[0]), "中文");
        // 一列宽时宽字符自己占一行（宽 2 > 1），但绝不劈开。
        let narrow = visual_rows("中文", 1);
        assert_eq!(narrow, vec![(0, 1), (1, 2)]);
    }

    #[test]
    fn vertical_movement_walks_visual_rows_with_sticky_column() {
        let mut prompt = Prompt::with_text("abcdefghij");
        // 内容宽度 4 → 3 个可视行：abcd / efgh / ij。
        assert_eq!(prompt.visual_rows(4), 3);
        assert_eq!(prompt.visual_column(4), 2, "光标落在末行的第 2 列");
        prompt.move_up(4);
        assert_eq!(prompt.cursor().column, 6, "落在上一可视行的同一显示列");
        assert_eq!(prompt.visual_column(4), 2);
        prompt.move_up(4);
        assert_eq!(prompt.cursor().column, 2);
        prompt.move_up(4);
        assert_eq!(
            prompt.cursor().column,
            2,
            "已在首行则夹在目标列，不回跳行首"
        );
        // 粘性列：下移回到底部仍是同一目标列。
        prompt.move_down(4);
        assert_eq!(prompt.cursor().column, 6);
        prompt.move_down(4);
        assert_eq!(prompt.cursor().column, 10);
        prompt.move_down(4);
        assert_eq!(prompt.cursor().column, 10, "末行同样夹住");
    }

    #[test]
    fn cursor_cell_points_inside_the_prompt_area() {
        let mut prompt = Prompt::with_text("中文x");
        // 内容宽度 4：第一可视行 = 中文（2 列），第二可视行 = x。
        assert_eq!(prompt.cursor_cell(4), (1, 1));
        prompt.move_home();
        assert_eq!(prompt.cursor_cell(4), (0, 0));
        prompt.move_right();
        prompt.move_right();
        assert_eq!(prompt.cursor_cell(4), (1, 0));
    }

    #[test]
    fn flat_render_marks_the_first_row_and_keeps_rows_aligned() {
        let theme = Theme::DARK;
        let prompt = Prompt::with_text("hi\nthere");
        let lines = prompt.render(
            &ctx(&theme, 20),
            &PromptChrome::default(),
            true,
            NEWLINE_HINT,
        );
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text(), "❯ hi");
        assert_eq!(lines[1].text(), "  there");
        for line in &lines {
            assert_eq!(line.selectable, Selectable::Spans(2..20));
        }
    }

    #[test]
    fn boxed_render_draws_borders_and_embeds_title_and_info() {
        let theme = Theme::DARK;
        let prompt = Prompt::with_text("看一下 CI");
        let chrome = PromptChrome::default()
            .with_title("会话 1")
            .with_info("deepseek-chat · rule · always-approve");
        let lines = prompt.render(&ctx(&theme, 40), &chrome, true, NEWLINE_HINT);
        assert_eq!(lines.len(), 3, "顶边 + 内容 + 底边");
        let text: Vec<String> = lines.iter().map(BlockLine::text).collect();
        assert!(text[0].starts_with("╭─ 会话 1 "), "{}", text[0]);
        assert!(text[0].ends_with('╮'), "{}", text[0]);
        assert!(text[1].starts_with("│ ❯ 看一下 CI"), "{}", text[1]);
        assert!(
            text[2].starts_with("╰─ deepseek-chat · rule"),
            "{}",
            text[2]
        );
        for line in &text {
            assert_eq!(width::display_width(line), 40, "{line:?}");
        }
        // 可选列区间整体右移了两个列（边框 + 内边距）。
        assert_eq!(lines[1].selectable, Selectable::Spans(4..40 - 2));
    }

    #[test]
    fn boxed_cursor_origin_matches_the_border() {
        let boxed = PromptChrome {
            title: None,
            info: Some("x".to_owned()),
        };
        assert_eq!(boxed.content_origin(), (2, 1));
        assert_eq!(boxed.inner_width(40), 36);
        assert_eq!(boxed.rows(1), 3);
        let flat = PromptChrome::default();
        assert_eq!(flat.content_origin(), (0, 0));
        assert_eq!(flat.inner_width(40), 40);
        assert_eq!(flat.rows(2), 2);
    }

    #[test]
    fn empty_prompt_shows_ghost_text() {
        let theme = Theme::DARK;
        let prompt = Prompt::new();
        let lines = prompt.render(
            &ctx(&theme, 40),
            &PromptChrome::default(),
            false,
            NEWLINE_HINT,
        );
        assert_eq!(lines.len(), 1);
        assert!(
            lines[0].text().contains("Enter 发送"),
            "{}",
            lines[0].text()
        );
        assert!(
            lines[0].text().contains("Ctrl+Shift+V 粘贴"),
            "{}",
            lines[0].text()
        );
        assert!(prompt.is_empty());
    }

    #[test]
    fn take_clears_and_returns_text() {
        let mut prompt = Prompt::with_text("a\nb");
        assert_eq!(prompt.take(), "a\nb");
        assert!(prompt.is_empty());
        assert_eq!(prompt.cursor(), TextCursor::default());
    }

    #[test]
    fn paste_with_newlines_creates_logical_lines() {
        let mut prompt = Prompt::new();
        prompt.insert_str("one\ntwo\nthree");
        assert_eq!(prompt.lines(), &["one", "two", "three"]);
        assert_eq!(prompt.cursor(), TextCursor { row: 2, column: 5 });
    }
}
