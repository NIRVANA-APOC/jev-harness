//! 面板壳：四边盒 + 标题（或 tab 条）+ 内容行 + 脚注。
//!
//! 设计来源：`docs/tui-design.md` §5.3、§6——研究视图的带、首屏的居中面板都走这一层；
//! 边框与脚注的形状与提示框一致（底边内嵌信息），边框字形取自字形表，无色档退化成 ASCII。
//!
//! 面板只认自己的行模型（[`PanelLine`]），不认识领域类型：视图模型的翻译在各面板模块里做。

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::blocks::{BlockCtx, BlockLine, Selectable};
use crate::components::clip_spans;
use crate::glyphs;
use crate::theme::Theme;
use crate::width;

/// 边框与内容之间的横向内边距。
const PAD_LEFT: usize = 1;
const PAD_RIGHT: usize = 1;
/// 标签列宽度上限：超过就裁（面板内容列通常远窄于终端）。
const LABEL_CAP: usize = 12;

/// 内容行的语气：把「这一行是什么」和「它长什么样」分开。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tone {
    #[default]
    Normal,
    Muted,
    Dim,
    Plan,
    Success,
    Warning,
    Error,
}

impl Tone {
    pub fn style(self, theme: &Theme) -> Style {
        match self {
            Tone::Normal => theme.fg(),
            Tone::Muted => theme.muted(),
            Tone::Dim => theme.dim(),
            Tone::Plan => theme.accent(theme.accent_plan),
            Tone::Success => theme.accent(theme.accent_success),
            Tone::Warning => theme.accent(theme.accent_warning),
            Tone::Error => theme.accent(theme.accent_error),
        }
    }
}

/// 面板标题：单标题（首屏）或 tab 条（研究视图）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PanelTitle {
    Single(String),
    Tabs { labels: Vec<String>, active: usize },
}

/// 面板内的一行（未换行；渲染时按内容列宽折算成屏幕行）。
#[derive(Debug, Clone, PartialEq)]
pub enum PanelLine {
    /// 小节标题。
    Section(String),
    /// 键 + 值：标签列在面板内对齐。
    Field {
        label: String,
        text: String,
        tone: Tone,
    },
    /// 自由文本。
    Text { text: String, tone: Tone },
    /// 左右两栏：左栏（字标等）按面板内最宽的左栏对齐。
    Split {
        left: String,
        left_tone: Tone,
        right: String,
        right_tone: Tone,
    },
    /// 菜单行：标签靠左，键位靠右。
    Menu { label: String, key: String },
    /// 空行。
    Blank,
}

/// 面板滚动状态：距内容顶部的行数。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PanelState {
    pub offset: usize,
}

impl PanelState {
    pub fn reset(&mut self) {
        self.offset = 0;
    }
}

/// 面板：数据 + 行；渲染是纯函数。
#[derive(Debug, Clone, PartialEq)]
pub struct Panel {
    pub title: PanelTitle,
    pub lines: Vec<PanelLine>,
    /// 脚注：内嵌在底边，与提示框的信息行同一个位置。
    pub footer: Option<String>,
}

impl Panel {
    pub fn new(title: PanelTitle) -> Self {
        Panel {
            title,
            lines: Vec::new(),
            footer: None,
        }
    }

    pub fn with_lines(title: PanelTitle, lines: Vec<PanelLine>) -> Self {
        Panel {
            title,
            lines,
            footer: None,
        }
    }

    pub fn with_footer(mut self, footer: impl Into<String>) -> Self {
        self.footer = Some(footer.into());
        self
    }

    /// 内容列宽（扣掉两侧边框与内边距）。
    pub fn content_width(total_width: usize) -> usize {
        total_width.saturating_sub(2 + PAD_LEFT + PAD_RIGHT).max(1)
    }

    /// 标签列宽度：面板内最长标签，封顶 [`LABEL_CAP`] 列。
    fn label_width(&self) -> usize {
        self.lines
            .iter()
            .filter_map(|line| match line {
                PanelLine::Field { label, .. } => Some(width::display_width(label)),
                _ => None,
            })
            .max()
            .unwrap_or(0)
            .min(LABEL_CAP)
    }

    /// 左右两栏里左栏的宽度。
    fn split_left_width(&self) -> usize {
        self.lines
            .iter()
            .filter_map(|line| match line {
                PanelLine::Split { left, .. } => Some(width::display_width(left)),
                _ => None,
            })
            .max()
            .unwrap_or(0)
    }

    /// 内容折算成屏幕行后的行数（换行与标签列对齐之后）。
    pub fn content_rows(&self, ctx: &BlockCtx) -> usize {
        self.rows(ctx).len()
    }

    /// 面板需要的高度（内容 + 上下边框）。
    pub fn height(&self, ctx: &BlockCtx) -> usize {
        self.content_rows(ctx) + 2
    }

    /// 渲染：`height` 是分给面板的整块高度（含边框），内容不足时补空行，
    /// 超出时按 [`PanelState::offset`] 切片并在边框上给滚动提示。
    pub fn render(
        &self,
        ctx: &BlockCtx,
        height: usize,
        state: &PanelState,
        focused: bool,
    ) -> Vec<BlockLine> {
        if height == 0 {
            return Vec::new();
        }
        let border = ctx.theme.frame(if focused {
            ctx.theme.panel_border_active
        } else {
            ctx.theme.panel_border
        });
        let rows = self.rows(ctx);
        let total = rows.len();
        let inner_height = height.saturating_sub(2);
        let offset = state
            .offset
            .min(total.saturating_sub(inner_height.min(total)));
        let above = offset > 0;
        let below = offset + inner_height < total;

        let mut output = Vec::with_capacity(height);
        output.push(self.top_border(ctx, border, above));
        for index in 0..inner_height {
            output.push(match rows.get(offset + index) {
                Some(spans) => content_line(ctx, spans.clone(), border),
                None => empty_line(ctx, border),
            });
        }
        if height >= 2 {
            output.push(self.bottom_border(ctx, border, below));
        }
        output
    }

    /// 顶边：`╭─ 标题 ───╮`，tab 形态在标签之间插 `┬`。
    fn top_border(&self, ctx: &BlockCtx, style: Style, hint: bool) -> BlockLine {
        let horizontal = ctx.glyphs.resolve(glyphs::box_horizontal());
        let mut spans = vec![Span::styled(
            ctx.glyphs.resolve(glyphs::box_top_left()).to_owned(),
            style,
        )];
        match &self.title {
            PanelTitle::Single(title) => {
                if !title.is_empty() {
                    spans.push(Span::styled(format!("{horizontal} "), style));
                    spans.push(Span::styled(title.clone(), ctx.theme.muted()));
                    spans.push(Span::styled(format!(" {horizontal}"), style));
                }
            }
            PanelTitle::Tabs { labels, active } => {
                let tee = ctx.glyphs.resolve(glyphs::box_tee_down());
                for (index, label) in labels.iter().enumerate() {
                    if index > 0 {
                        spans.push(Span::styled(tee.to_owned(), style));
                    }
                    spans.push(Span::styled(format!("{horizontal} "), style));
                    let label_style = if index == *active {
                        ctx.theme.accent(ctx.theme.accent_system)
                    } else {
                        ctx.theme.dim()
                    };
                    spans.push(Span::styled(label.clone(), label_style));
                    spans.push(Span::styled(format!(" {horizontal}"), style));
                }
            }
        }
        let hint = if hint {
            Some(ctx.glyphs.resolve(glyphs::arrow_up()))
        } else {
            None
        };
        close_border(ctx, spans, style, hint, true)
    }

    /// 底边：`╰─ 脚注 ─── ▼ ╯`。
    fn bottom_border(&self, ctx: &BlockCtx, style: Style, hint: bool) -> BlockLine {
        let horizontal = ctx.glyphs.resolve(glyphs::box_horizontal());
        let mut spans = vec![Span::styled(
            ctx.glyphs.resolve(glyphs::box_bottom_left()).to_owned(),
            style,
        )];
        if let Some(footer) = &self.footer {
            spans.push(Span::styled(format!("{horizontal} "), style));
            spans.push(Span::styled(footer.clone(), ctx.theme.dim()));
            spans.push(Span::styled(format!(" {horizontal}"), style));
        }
        let hint = if hint {
            Some(ctx.glyphs.resolve(glyphs::scroll_hint()))
        } else {
            None
        };
        close_border(ctx, spans, style, hint, false)
    }

    /// 内容行：`│ 内容 │`；内容铺满，保证右侧边框对齐、盒子不透明。
    fn rows(&self, ctx: &BlockCtx) -> Vec<Vec<Span<'static>>> {
        let content_width = Self::content_width(ctx.width);
        let label_width = self.label_width();
        let left_width = self.split_left_width();
        let ellipsis = ctx.glyphs.resolve(glyphs::ellipsis());
        let mut rows: Vec<Vec<Span<'static>>> = Vec::new();

        for line in &self.lines {
            match line {
                PanelLine::Section(text) => {
                    let style = ctx.theme.muted().add_modifier(Modifier::BOLD);
                    for part in width::wrap_words(text, content_width) {
                        rows.push(vec![Span::styled(part, style)]);
                    }
                }
                PanelLine::Field { label, text, tone } => {
                    let indent = label_width + 1;
                    let body_width = content_width.saturating_sub(indent).max(1);
                    let pad = label_width
                        .saturating_sub(width::display_width(label))
                        .min(label_width);
                    let head = format!("{label}{}", " ".repeat(pad + 1));
                    let style = tone.style(ctx.theme);
                    for (index, part) in width::wrap_words(text, body_width).iter().enumerate() {
                        let prefix = if index == 0 {
                            head.clone()
                        } else {
                            " ".repeat(indent)
                        };
                        rows.push(vec![
                            Span::styled(prefix, ctx.theme.dim()),
                            Span::styled(part.clone(), style),
                        ]);
                    }
                }
                PanelLine::Text { text, tone } => {
                    let style = tone.style(ctx.theme);
                    for part in width::wrap_words(text, content_width) {
                        rows.push(vec![Span::styled(part, style)]);
                    }
                }
                PanelLine::Split {
                    left,
                    left_tone,
                    right,
                    right_tone,
                } => {
                    let pad = left_width.saturating_sub(width::display_width(left));
                    let budget = content_width.saturating_sub(left_width + 2).max(1);
                    let right = width::clip_with_ellipsis(right, budget, ellipsis);
                    rows.push(vec![
                        Span::styled(left.clone(), left_tone.style(ctx.theme)),
                        Span::styled(" ".repeat(pad + 2), ctx.theme.dim()),
                        Span::styled(right, right_tone.style(ctx.theme)),
                    ]);
                }
                PanelLine::Menu { label, key } => {
                    let key_width = width::display_width(key);
                    let budget = content_width.saturating_sub(key_width + 1).max(1);
                    let label = width::clip_with_ellipsis(label, budget, ellipsis);
                    let pad =
                        content_width.saturating_sub(width::display_width(&label) + key_width);
                    rows.push(vec![
                        Span::styled(label, ctx.theme.fg()),
                        Span::styled(" ".repeat(pad), ctx.theme.dim()),
                        Span::styled(key.clone(), ctx.theme.dim()),
                    ]);
                }
                PanelLine::Blank => rows.push(Vec::new()),
            }
        }
        rows
    }
}

/// 收口一条边框：补横线到边缘，可选在右角前放一个提示字形。
fn close_border(
    ctx: &BlockCtx,
    mut spans: Vec<Span<'static>>,
    style: Style,
    hint: Option<&str>,
    top: bool,
) -> BlockLine {
    let horizontal = ctx.glyphs.resolve(glyphs::box_horizontal());
    let corner = if top {
        ctx.glyphs.resolve(glyphs::box_top_right())
    } else {
        ctx.glyphs.resolve(glyphs::box_bottom_right())
    };
    let used: usize = spans
        .iter()
        .map(|span| width::display_width(span.content.as_ref()))
        .sum();
    let reserved = hint.map_or(0, |hint| width::display_width(hint) + 1);
    let fill = ctx.width.saturating_sub(used + 1 + reserved);
    if fill > 0 {
        spans.push(Span::styled(horizontal.repeat(fill), style));
    }
    if let Some(hint) = hint {
        spans.push(Span::styled(hint.to_owned(), style));
        spans.push(Span::styled(" ".to_owned(), style));
    }
    spans.push(Span::styled(corner.to_owned(), style));
    BlockLine::new(
        Line::from(clip_spans(
            spans,
            ctx.width,
            ctx.glyphs.resolve(glyphs::ellipsis()),
        )),
        Selectable::None,
    )
}

/// 一条内容行：`│ 内容 │`。
fn content_line(ctx: &BlockCtx, spans: Vec<Span<'static>>, border: Style) -> BlockLine {
    let inner = Panel::content_width(ctx.width);
    let used: usize = spans
        .iter()
        .map(|span| width::display_width(span.content.as_ref()))
        .sum();
    let mut body = spans;
    if used > inner {
        body = clip_spans(body, inner, ctx.glyphs.resolve(glyphs::ellipsis()));
    } else if used < inner {
        body.push(Span::styled(" ".repeat(inner - used), Style::new()));
    }
    line_with_borders(
        ctx,
        body,
        border,
        Selectable::Spans(2..ctx.width.saturating_sub(2) as u16),
    )
}

/// 补空白的内部行。
fn empty_line(ctx: &BlockCtx, border: Style) -> BlockLine {
    let inner = Panel::content_width(ctx.width);
    line_with_borders(
        ctx,
        vec![Span::styled(" ".repeat(inner), Style::new())],
        border,
        Selectable::None,
    )
}

fn line_with_borders(
    ctx: &BlockCtx,
    body: Vec<Span<'static>>,
    border: Style,
    selectable: Selectable,
) -> BlockLine {
    let vertical = ctx.glyphs.resolve(glyphs::box_vertical());
    let mut spans = Vec::with_capacity(body.len() + 2);
    spans.push(Span::styled(
        format!("{vertical}{}", " ".repeat(PAD_LEFT)),
        border,
    ));
    spans.extend(body);
    spans.push(Span::styled(
        format!("{}{vertical}", " ".repeat(PAD_RIGHT)),
        border,
    ));
    let _ = ctx;
    BlockLine::new(Line::from(spans), selectable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ColorLevel;

    fn ctx<'a>(theme: &'a Theme, width: usize) -> BlockCtx<'a> {
        BlockCtx {
            width,
            theme,
            glyphs: crate::glyphs::Glyphs::FANCY,
            frame: 0,
            decision: None,
        }
    }

    fn sample_panel() -> Panel {
        Panel::with_lines(
            PanelTitle::Single("判断详情".to_owned()),
            vec![
                PanelLine::Field {
                    label: "问题".to_owned(),
                    text: "shell_command_allowed v3".to_owned(),
                    tone: Tone::Normal,
                },
                PanelLine::Field {
                    label: "有效性".to_owned(),
                    text: "待重查".to_owned(),
                    tone: Tone::Warning,
                },
                PanelLine::Blank,
                PanelLine::Section("分布".to_owned()),
                PanelLine::Text {
                    text: "0.71 允许一次".to_owned(),
                    tone: Tone::Plan,
                },
            ],
        )
        .with_footer("12 条判断")
    }

    #[test]
    fn panel_fills_the_given_height_and_draws_both_borders() {
        let theme = Theme::DARK;
        let ctx = ctx(&theme, 44);
        let lines = sample_panel().render(&ctx, 8, &PanelState::default(), true);
        assert_eq!(lines.len(), 8);
        let text: Vec<String> = lines.iter().map(BlockLine::text).collect();
        assert!(text[0].starts_with("╭─ 判断详情"), "{}", text[0]);
        assert!(text[0].ends_with("╮"), "{}", text[0]);
        assert!(text[7].starts_with("╰─ 12 条判断"), "{}", text[7]);
        assert!(text[7].ends_with("╯"), "{}", text[7]);
        // 内容行两侧都有边框，且整宽一致。
        for row in &lines {
            assert_eq!(width::display_width(&row.text()), 44, "{:?}", row.text());
        }
        assert!(text.iter().any(|row| row.contains("shell_command_allowed")));
        assert!(text.iter().any(|row| row.contains("分布")));
    }

    #[test]
    fn fields_align_their_labels() {
        let theme = Theme::DARK;
        let ctx = ctx(&theme, 44);
        let text: Vec<String> = sample_panel()
            .render(&ctx, 8, &PanelState::default(), true)
            .iter()
            .map(BlockLine::text)
            .collect();
        let question = text.iter().find(|row| row.contains("问题")).unwrap();
        let validity = text.iter().find(|row| row.contains("有效性")).unwrap();
        // 「问题」2 列 + 3 空格 = 第 5 列起；「有效性」3 列 + 2 空格 = 同一列起。
        let column = |row: &str, needle: &str| {
            let at = row.find(needle).unwrap();
            width::display_width(&row[..at])
        };
        assert_eq!(
            column(question, "shell_command_allowed"),
            column(validity, "待重查"),
            "{question:?} vs {validity:?}"
        );
    }

    #[test]
    fn scrolling_slices_the_content_and_hints_with_arrows() {
        let theme = Theme::DARK;
        let ctx = ctx(&theme, 24);
        let panel = sample_panel();
        let total = panel.content_rows(&ctx);
        assert!(total > 3);

        let lines = panel.render(&ctx, 4, &PanelState { offset: 0 }, true);
        let text: Vec<String> = lines.iter().map(BlockLine::text).collect();
        assert!(!text[0].contains('▲'), "顶部不画上箭头：{}", text[0]);
        assert!(text[3].contains('▼'), "还有下文时底边画下箭头：{}", text[3]);

        let lines = panel.render(&ctx, 4, &PanelState { offset: total - 1 }, true);
        let text: Vec<String> = lines.iter().map(BlockLine::text).collect();
        assert!(text[0].contains('▲'), "已上滚时顶边画上箭头：{}", text[0]);
        assert!(!text[3].contains('▼'), "到底了不画下箭头：{}", text[3]);
    }

    #[test]
    fn tabs_mark_the_active_one_and_are_not_tees_misplaced() {
        let theme = Theme::DARK;
        let ctx = ctx(&theme, 40);
        let panel = Panel::with_lines(
            PanelTitle::Tabs {
                labels: vec!["判断详情".to_owned(), "事件与证据".to_owned()],
                active: 1,
            },
            vec![PanelLine::Text {
                text: "x".to_owned(),
                tone: Tone::Normal,
            }],
        );
        let top = panel.render(&ctx, 4, &PanelState::default(), true)[0].text();
        assert!(top.starts_with("╭─ 判断详情 ─┬─ 事件与证据"), "{top}");
        assert_eq!(width::display_width(&top), 40, "{top}");
    }

    #[test]
    fn no_color_and_fallback_glyphs_keep_every_character() {
        let theme = Theme::DARK.quantized(ColorLevel::None);
        let ctx = BlockCtx {
            width: 44,
            theme: &theme,
            glyphs: crate::glyphs::Glyphs::FALLBACK,
            frame: 0,
            decision: None,
        };
        let text: Vec<String> = sample_panel()
            .render(&ctx, 8, &PanelState::default(), false)
            .iter()
            .map(BlockLine::text)
            .collect();
        assert!(text[0].starts_with("+- 判断详情"), "{}", text[0]);
        assert!(text[0].ends_with('+'), "{}", text[0]);
        for row in &text {
            assert_eq!(width::display_width(row), 44, "{row:?}");
        }
        assert!(text.iter().any(|row| row.contains("shell_command_allowed")));
    }

    #[test]
    fn menu_rows_push_the_key_to_the_right_edge() {
        let theme = Theme::DARK;
        let ctx = ctx(&theme, 30);
        let panel = Panel::with_lines(
            PanelTitle::Single(String::new()),
            vec![PanelLine::Menu {
                label: "新建会话".to_owned(),
                key: "Ctrl+N".to_owned(),
            }],
        );
        let row = panel.render(&ctx, 3, &PanelState::default(), true)[1].text();
        assert!(row.starts_with("│ 新建会话"), "{row}");
        assert!(
            row.trim_end_matches('│').trim_end().ends_with("Ctrl+N"),
            "{row}"
        );
    }

    #[test]
    fn height_one_degrades_to_a_single_border() {
        let theme = Theme::DARK;
        let ctx = ctx(&theme, 20);
        let lines = sample_panel().render(&ctx, 1, &PanelState::default(), false);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].text().starts_with('╭'));
    }
}
