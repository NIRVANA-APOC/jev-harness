//! 块模型：块是纯函数 `output(ctx)`，产出**已换行的屏幕行**，行级元数据一次产出。
//!
//! 设计来源：`docs/grok-tui-design.md` §4.2、§7.1 第 2/3 条；专题
//! `docs/research/grok-tui/09-scrollback-block-model.md`（§1 块是纯函数 + 显式上下文、
//! `types.rs:146` 的行级元数据、`render.rs:498`「每个 BlockLine 已是按内容宽度换行后的一行」、
//! `layout.rs:17-19` 的内容宽度来源）。
//!
//! 与上游的差异：
//! - 上游 `RenderBlock` 13 个变体（工具块内部还有 13 个），这里只保留最小 4 类：
//!   用户 / 助手 / 工具 / 决策轨迹。
//! - 行级元数据只保留两件：`selectable`（决定能被复制/搜索的范围）与 `joiner`
//!   （软换行行尾的拼接串）。上游另有 `wrap` / `selection_range` / `background` /
//!   `bg_start_col` / `indent_width` / `link_target`，本项目暂无消费者。
//! - 决策轨迹块是本项目专有：把 Jev 的 `Choice` 概率与 `confidence` 画到屏上，
//!   全程不塌缩成布尔（AGENTS.md「概率优先」）。

use std::ops::Range;

use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::glyphs::{self, Glyphs};
use crate::theme::Theme;
use crate::width;

/// 行在复制/导出时的可选范围。
///
/// 对应上游 `Selectable`（`types.rs:72`）：`All` 会把行尾空白裁掉，`Spans` 只取列区间
/// （用于把块标记列排除在复制文本外，上游 `shift_selection_metadata_for_prefix`
/// 做的正是这件事，`types.rs:398`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selectable {
    /// 整行可选。
    All,
    /// 只取 `[start, end)` 列。
    Spans(Range<u16>),
    /// 不可选：空白分隔行与纯装饰行。
    None,
}

/// 一行屏幕内容 + 行级元数据。
#[derive(Debug, Clone, PartialEq)]
pub struct BlockLine {
    pub content: Line<'static>,
    pub selectable: Selectable,
    /// 本行与**上一行**的拼接串：`None` 表示这一行是逻辑行首（复制时前面落 `\n`），
    /// `Some(" ")` 表示这一行是上一行的软换行续行。语义与上游一致（`types.rs:146`
    /// 的 `joiner`：「joiner 为空则逻辑行 +1」）。
    pub joiner: Option<String>,
}

impl BlockLine {
    /// 纯文本内容（所有 span 拼接，未做可选范围裁剪）。
    pub fn text(&self) -> String {
        self.content
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    /// 供复制/导出使用的文本；`Selectable::None` 返回 `None`。
    pub fn selection_text(&self) -> Option<String> {
        let text = self.text();
        match &self.selectable {
            Selectable::None => None,
            Selectable::All => Some(text.trim_end().to_owned()),
            Selectable::Spans(range) => {
                let head = width::clip(&text, range.start as usize).len();
                let tail = (range.end - range.start) as usize;
                Some(width::clip(&text[head..], tail).to_owned())
            }
        }
    }

    pub fn new(content: Line<'static>, selectable: Selectable) -> Self {
        BlockLine {
            content,
            selectable,
            joiner: None,
        }
    }

    /// 打上软换行标记（与上一行同属一条逻辑行）。
    pub fn soft_wrapped(mut self) -> Self {
        self.joiner = Some(" ".to_owned());
        self
    }
}

/// 块的产出：已换行的屏幕行序列。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BlockOutput {
    pub lines: Vec<BlockLine>,
}

impl BlockOutput {
    /// 按行级 `joiner` 还原文本：续行用**本行**的 joiner 拼回上一行，逻辑行首落 `\n`。
    /// 这是 `joiner` 元数据的唯一消费者，也是「一次产出、四处复用」的最小证据。
    pub fn plain_text(&self) -> String {
        let mut out = String::new();
        for (index, line) in self.lines.iter().enumerate() {
            if index > 0 {
                match &line.joiner {
                    Some(joiner) => out.push_str(joiner),
                    None => out.push('\n'),
                }
            }
            out.push_str(&line.selection_text().unwrap_or_else(|| line.text()));
        }
        out
    }

    pub fn push(&mut self, line: BlockLine) {
        self.lines.push(line);
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

/// 渲染上下文：块的全部环境输入（上游 `BlockContext`，`types.rs:98-111`）。
#[derive(Debug, Clone, Copy)]
pub struct BlockCtx<'a> {
    /// 本块可用总列宽（含块自己的标记列）。
    pub width: usize,
    pub theme: &'a Theme,
    pub glyphs: Glyphs,
    /// 动画帧号（= tick / [`crate::render::SPINNER_DIVISOR`]）。
    pub frame: usize,
    /// 当前轮次的 Jev 判定轨迹：概率与置信度的**一等数据**。
    /// 状态行用它把本轮概率上屏（设计 §7 P2：不新增可视化组件）；
    /// 历史里的决策块**不吃这一份**，而是渲染自身载荷（块载荷管历史，ctx 只管本轮）。
    pub decision: Option<&'a DecisionTrace>,
}

impl BlockCtx<'_> {
    /// 内容列宽（总宽减去块标记列）。
    pub fn content_width(&self, prefix_width: usize) -> usize {
        self.width.saturating_sub(prefix_width)
    }
}

/// 一个判定选项：概率是必填的一等数据，不是排序后的名次。
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    pub id: String,
    pub label: String,
    pub probability: f32,
}

impl Choice {
    pub fn new(id: impl Into<String>, label: impl Into<String>, probability: f32) -> Self {
        Choice {
            id: id.into(),
            label: label.into(),
            probability,
        }
    }
}

/// 不确定度分档：本项目自定的阈值（上游没有对应机制），只为把「不确定」画到屏上。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Uncertainty {
    /// 首选概率 ≥ 0.8 且与次选差距 ≥ 0.5
    Clear,
    /// 首选概率 ≥ 0.5
    Leaning,
    /// 其余情况，含无选项
    Uncertain,
}

impl Uncertainty {
    pub fn label(self) -> &'static str {
        match self {
            Uncertainty::Clear => "确定",
            Uncertainty::Leaning => "倾向",
            Uncertainty::Uncertain => "不确定",
        }
    }

    /// 是否属于「不确定」，用于上色与提示。
    pub fn is_uncertain(self) -> bool {
        self == Uncertainty::Uncertain
    }
}

/// Jev 判定轨迹：state + 类型化问题 → 带概率的答案。
#[derive(Debug, Clone, PartialEq)]
pub struct DecisionTrace {
    /// 判定问题（类型化问题的可读形式，例如 `shell_command_allowed`）。
    pub question: String,
    /// 面向用户的问句，可以为空。
    pub prompt: String,
    pub choices: Vec<Choice>,
    /// Jev 返回的整体置信度；`None` 表示本轮没有返回置信度。
    pub confidence: Option<f32>,
    /// 被采纳的选项下标；`None` 表示尚未采纳。
    pub chosen: Option<usize>,
}

impl DecisionTrace {
    pub fn new(question: impl Into<String>, choices: Vec<Choice>) -> Self {
        DecisionTrace {
            question: question.into(),
            prompt: String::new(),
            choices,
            confidence: None,
            chosen: None,
        }
    }

    pub fn with_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.prompt = prompt.into();
        self
    }

    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = Some(confidence);
        self
    }

    pub fn with_chosen(mut self, index: usize) -> Self {
        self.chosen = Some(index);
        self
    }

    /// 概率最高者（并列时取先注册的那个）。
    pub fn top(&self) -> Option<&Choice> {
        self.choices.iter().reduce(|best, choice| {
            if choice.probability > best.probability {
                choice
            } else {
                best
            }
        })
    }

    /// 首选与次选的概率差；不足两个选项时为 `None`。
    pub fn margin(&self) -> Option<f32> {
        let mut sorted: Vec<f32> = self.choices.iter().map(|c| c.probability).collect();
        if sorted.len() < 2 {
            return None;
        }
        sorted.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
        Some(sorted[0] - sorted[1])
    }

    /// 不确定度分档。
    pub fn uncertainty(&self) -> Uncertainty {
        let Some(top) = self.top() else {
            return Uncertainty::Uncertain;
        };
        let margin = self.margin().unwrap_or(1.0);
        if top.probability >= 0.8 && margin >= 0.5 {
            Uncertainty::Clear
        } else if top.probability >= 0.5 {
            Uncertainty::Leaning
        } else {
            Uncertainty::Uncertain
        }
    }

    /// 一行摘要（状态行与决策块共用），例：`usr:allow 0.71 · margin 0.49 · 不确定`。
    /// 分隔符由调用方传入（取自字形表），免得排版符号在数据层里另起一份硬编码。
    pub fn summary(&self, separator: &str) -> String {
        let top = match self.top() {
            Some(top) => top,
            None => return "无选项".to_owned(),
        };
        let margin = match self.margin() {
            Some(margin) => format!(" {separator} margin {margin:.2}"),
            None => String::new(),
        };
        let confidence = match self.confidence {
            Some(confidence) => format!(" {separator} confidence {confidence:.2}"),
            None => String::new(),
        };
        format!(
            "{} {:.2}{margin}{confidence} {separator} {}",
            top.id,
            top.probability,
            self.uncertainty().label()
        )
    }
}

/// 工具调用状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStatus {
    Running,
    Ok,
    Failed,
}

/// 用户消息块。
///
/// 时间戳由适配层格式化好（本 crate 不做时区与日历计算），渲染时贴在填充条第一行的右端。
#[derive(Debug, Clone, PartialEq)]
pub struct UserBlock {
    pub text: String,
    pub timestamp: Option<String>,
}

/// 提示语气：阶段行、完成行与失败行共用一处语气映射。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeTone {
    Info,
    Success,
    Warning,
    Error,
}

impl NoticeTone {
    fn style(self, theme: &Theme) -> Style {
        match self {
            NoticeTone::Info => theme.muted(),
            NoticeTone::Success => theme.accent(theme.accent_success),
            NoticeTone::Warning => theme.accent(theme.accent_warning),
            NoticeTone::Error => theme.accent(theme.accent_error),
        }
    }
}

/// 阶段行块：`◆ 判定 12ms · 采纳 usr:allow 0.71 · 倾向`，或没有标记的 `用时 1.5s`。
#[derive(Debug, Clone, PartialEq)]
pub struct NoticeBlock {
    pub text: String,
    /// 是否画标记列（`◆ `）；不画时用等宽空白占位，保证换行后对齐。
    pub marker: bool,
    pub tone: NoticeTone,
}

/// 助手消息块。
#[derive(Debug, Clone, PartialEq)]
pub struct AssistantBlock {
    pub text: String,
    pub streaming: bool,
}

/// 工具调用块。
#[derive(Debug, Clone, PartialEq)]
pub struct ToolBlock {
    pub name: String,
    pub args: String,
    pub status: ToolStatus,
    /// 结果摘要（可多行，渲染时按内容宽度换行并缩进）。
    pub detail: Option<String>,
}

/// 决策轨迹块：把 Jev 的概率画到屏上。
#[derive(Debug, Clone, PartialEq)]
pub struct DecisionBlock {
    pub trace: DecisionTrace,
}

/// 块：纯函数 `output(ctx)` 的五种形态。
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    User(UserBlock),
    Assistant(AssistantBlock),
    Tool(ToolBlock),
    Decision(DecisionBlock),
    Notice(NoticeBlock),
}

impl Block {
    pub fn user(text: impl Into<String>) -> Self {
        Block::User(UserBlock {
            text: text.into(),
            timestamp: None,
        })
    }

    /// 带时间戳的用户块（时间戳由适配层格式化）。
    pub fn user_at(text: impl Into<String>, timestamp: impl Into<String>) -> Self {
        Block::User(UserBlock {
            text: text.into(),
            timestamp: Some(timestamp.into()),
        })
    }

    /// 阶段行：带标记（`◆ 判定 …`）。
    pub fn notice(text: impl Into<String>) -> Self {
        Block::Notice(NoticeBlock {
            text: text.into(),
            marker: true,
            tone: NoticeTone::Info,
        })
    }

    /// 无标记的阶段行（`用时 1.5s`）。
    pub fn plain_notice(text: impl Into<String>) -> Self {
        Block::Notice(NoticeBlock {
            text: text.into(),
            marker: false,
            tone: NoticeTone::Info,
        })
    }

    pub fn with_tone(mut self, tone: NoticeTone) -> Self {
        if let Block::Notice(notice) = &mut self {
            notice.tone = tone;
        }
        self
    }

    pub fn assistant(text: impl Into<String>) -> Self {
        Block::Assistant(AssistantBlock {
            text: text.into(),
            streaming: false,
        })
    }

    /// 流式助手块：正文还在长（末尾画三角）；收尾时由宿主把 `streaming` 清掉。
    pub fn streaming_assistant(text: impl Into<String>) -> Self {
        Block::Assistant(AssistantBlock {
            text: text.into(),
            streaming: true,
        })
    }

    pub fn tool(name: impl Into<String>, args: impl Into<String>, status: ToolStatus) -> Self {
        Block::Tool(ToolBlock {
            name: name.into(),
            args: args.into(),
            status,
            detail: None,
        })
    }

    pub fn decision(trace: DecisionTrace) -> Self {
        Block::Decision(DecisionBlock { trace })
    }

    /// 块名（测试与调试用，也是快照测试的锚点）。
    pub fn kind(&self) -> &'static str {
        match self {
            Block::User(_) => "user",
            Block::Assistant(_) => "assistant",
            Block::Tool(_) => "tool",
            Block::Decision(_) => "decision",
            Block::Notice(_) => "notice",
        }
    }

    /// 纯函数：给定上下文产出已换行的屏幕行；无副作用、无缓存。
    pub fn output(&self, ctx: &BlockCtx) -> BlockOutput {
        match self {
            Block::User(block) => text_block_lines(
                ctx,
                &TextBlock {
                    marker: ctx.glyphs.resolve(glyphs::prompt()).to_owned(),
                    style: ctx.theme.fg(),
                    // 无色档不铺底色（`Reset` 会覆盖用户终端的背景）。
                    fill: ctx
                        .theme
                        .level
                        .has_color()
                        .then_some(ctx.theme.user_block_bg),
                    tail: block.timestamp.as_deref(),
                    streaming: false,
                },
                &block.text,
            ),
            Block::Assistant(block) => text_block_lines(
                ctx,
                &TextBlock {
                    // 标记后留一列空格：与参考截图的 `◆ Thought for 0.3s` 同形。
                    marker: format!("{} ", ctx.glyphs.resolve(glyphs::diamond())),
                    style: ctx.theme.fg(),
                    fill: None,
                    tail: None,
                    streaming: block.streaming,
                },
                &block.text,
            ),
            Block::Tool(block) => tool_lines(ctx, block),
            Block::Decision(block) => decision_lines(ctx, &block.trace),
            Block::Notice(block) => notice_lines(ctx, block),
        }
    }
}

/// 给样式补一层背景色；`None` 时原样返回。
fn with_bg(style: Style, fill: Option<ratatui::style::Color>) -> Style {
    match fill {
        Some(color) => style.bg(color),
        None => style,
    }
}

/// 文本类块（用户 / 助手 / 阶段行）的渲染参数。
struct TextBlock<'a> {
    /// 标记列（已解析字形，含尾随空格）：续行缩进与正文列都按它的宽度算。
    marker: String,
    style: Style,
    /// 整行铺满的背景色（用户块的填充条）；`None` 表示不铺。
    fill: Option<ratatui::style::Color>,
    /// 首行右对齐的尾注（用户块的时间戳）。
    tail: Option<&'a str>,
    streaming: bool,
}

/// 消息类块的共用渲染：标记列 + 词级换行 + 续行缩进（+ 可选填充条与尾注）。
fn text_block_lines(ctx: &BlockCtx, spec: &TextBlock, text: &str) -> BlockOutput {
    let marker = spec.marker.as_str();
    let prefix_width = width::display_width(marker);
    let content_width = ctx.content_width(prefix_width).max(1);
    let mut output = BlockOutput::default();

    let paragraphs: Vec<&str> = text.split('\n').collect();
    let last_paragraph = paragraphs.len() - 1;
    for (paragraph_index, paragraph) in paragraphs.iter().enumerate() {
        let wrapped = width::wrap_words(paragraph, content_width);
        let last_row = wrapped.len() - 1;
        for (line_index, text) in wrapped.iter().enumerate() {
            let head = if paragraph_index == 0 && line_index == 0 {
                marker.to_owned()
            } else {
                " ".repeat(prefix_width)
            };
            let mut spans = vec![
                Span::styled(head, with_bg(ctx.theme.dim(), spec.fill)),
                Span::styled(text.clone(), with_bg(spec.style, spec.fill)),
            ];
            // 流式末尾画一个「还在长」的三角，与软换行标记一样只是行级元数据之外的装饰。
            if spec.streaming && paragraph_index == last_paragraph && line_index == last_row {
                spans.push(Span::styled(
                    ctx.glyphs.resolve(glyphs::triangle_right()),
                    ctx.theme.dim(),
                ));
            }
            // 尾注贴在填充条的右端：先补间隙再写尾注，尾注之后不再补空白。
            let tail = match (paragraph_index, line_index) {
                (0, 0) => spec.tail,
                _ => None,
            };
            let used = prefix_width + width::display_width(text);
            let padding = ctx
                .width
                .saturating_sub(used + tail.map_or(0, width::display_width));
            if let Some(fill) = spec.fill {
                spans.push(Span::styled(" ".repeat(padding), Style::new().bg(fill)));
            } else if tail.is_some() {
                spans.push(Span::styled(" ".repeat(padding), Style::new()));
            }
            if let Some(tail) = tail {
                spans.push(Span::styled(
                    tail.to_owned(),
                    with_bg(ctx.theme.frame(ctx.theme.timestamp), spec.fill),
                ));
            }
            let start = prefix_width;
            let end = prefix_width + width::display_width(text);
            let line = BlockLine::new(
                Line::from(spans),
                Selectable::Spans(start as u16..end as u16),
            );
            // 同一逻辑行的续行打上软换行标记；逻辑换行（`\n`）不打。
            output.push(if line_index == 0 {
                line
            } else {
                line.soft_wrapped()
            });
        }
    }
    if output.is_empty() {
        output.push(BlockLine::new(
            Line::from(Span::styled(marker.to_owned(), ctx.theme.dim())),
            Selectable::None,
        ));
    }
    output
}

/// 阶段行：标记列 + 词级换行，语气来自 [`NoticeBlock::tone`]。
fn notice_lines(ctx: &BlockCtx, block: &NoticeBlock) -> BlockOutput {
    let marker = ctx.glyphs.resolve(glyphs::diamond());
    let head = if block.marker {
        format!("{marker} ")
    } else {
        " ".repeat(width::display_width(marker) + 1)
    };
    let prefix_width = width::display_width(&head);
    let content_width = ctx.content_width(prefix_width).max(1);
    let style = block.tone.style(ctx.theme);

    let mut output = BlockOutput::default();
    for (index, text) in width::wrap_words(&block.text, content_width)
        .iter()
        .enumerate()
    {
        let head = if index == 0 {
            head.clone()
        } else {
            " ".repeat(prefix_width)
        };
        let start = prefix_width;
        let end = prefix_width + width::display_width(text);
        let line = BlockLine::new(
            Line::from(vec![
                Span::styled(head, ctx.theme.dim()),
                Span::styled(text.clone(), style),
            ]),
            Selectable::Spans(start as u16..end as u16),
        );
        output.push(if index == 0 {
            line
        } else {
            line.soft_wrapped()
        });
    }
    output
}

fn tool_lines(ctx: &BlockCtx, block: &ToolBlock) -> BlockOutput {
    let marker = match block.status {
        ToolStatus::Running => glyphs::spinner_frame(ctx.glyphs, ctx.frame),
        _ => ctx.glyphs.resolve(glyphs::triangle_right()),
    };
    let status_style = match block.status {
        ToolStatus::Running => ctx.theme.muted(),
        ToolStatus::Ok => ctx.theme.accent(ctx.theme.accent_success),
        ToolStatus::Failed => ctx.theme.accent(ctx.theme.accent_error),
    };
    let head = format!("{marker} ");
    let prefix_width = width::display_width(&head);
    let content_width = ctx.content_width(prefix_width).max(1);

    let mut header = vec![
        Span::styled(head, ctx.theme.dim()),
        Span::styled(block.name.clone(), status_style),
    ];
    if !block.args.is_empty() {
        let args_width = ctx
            .width
            .saturating_sub(prefix_width + width::display_width(&block.name) + 1);
        header.push(Span::styled(
            format!(
                " {}",
                width::clip_with_ellipsis(
                    &block.args,
                    args_width,
                    ctx.glyphs.resolve(glyphs::ellipsis())
                )
            ),
            ctx.theme.dim(),
        ));
    }

    let mut output = BlockOutput::default();
    output.push(BlockLine::new(
        Line::from(header),
        Selectable::Spans(prefix_width as u16..ctx.width as u16),
    ));

    if let Some(detail) = &block.detail {
        let indent = " ".repeat(prefix_width);
        for (index, wrapped) in width::wrap_words(detail, content_width).iter().enumerate() {
            output.push(
                BlockLine::new(
                    Line::from(vec![
                        Span::styled(indent.clone(), ctx.theme.dim()),
                        Span::styled(wrapped.clone(), ctx.theme.muted()),
                    ]),
                    Selectable::Spans(
                        prefix_width as u16..(prefix_width + width::display_width(wrapped)) as u16,
                    ),
                )
                .mark_soft_if(index > 0),
            );
        }
    }
    output
}

fn decision_lines(ctx: &BlockCtx, trace: &DecisionTrace) -> BlockOutput {
    let marker = ctx.glyphs.resolve(glyphs::diamond_open());
    let indent = " ".repeat(width::display_width(marker) + 1);
    let body_width = ctx.content_width(width::display_width(marker) + 1).max(1);
    let uncertainty = trace.uncertainty();
    let accent = if uncertainty.is_uncertain() {
        ctx.theme.accent_warning
    } else {
        ctx.theme.accent_plan
    };
    let separator = ctx.glyphs.resolve(glyphs::separator());

    let mut output = BlockOutput::default();
    let mut header = vec![
        Span::styled(format!("{marker} "), ctx.theme.accent(accent)),
        Span::styled(trace.question.clone(), ctx.theme.fg()),
    ];
    if trace.confidence.is_none() && !trace.choices.is_empty() {
        header.push(Span::styled(
            format!(" {separator} 无置信度"),
            ctx.theme.dim(),
        ));
    }
    output.push(BlockLine::new(
        Line::from(header),
        Selectable::Spans(width::display_width(marker) as u16 + 1..ctx.width as u16),
    ));

    if !trace.prompt.is_empty() {
        for (index, wrapped) in width::wrap_words(&trace.prompt, body_width)
            .iter()
            .enumerate()
        {
            output.push(
                BlockLine::new(
                    Line::from(vec![
                        Span::styled(indent.clone(), ctx.theme.dim()),
                        Span::styled(wrapped.clone(), ctx.theme.muted()),
                    ]),
                    Selectable::All,
                )
                .mark_soft_if(index > 0),
            );
        }
    }

    for (index, choice) in trace.choices.iter().enumerate() {
        let bar = probability_bar(ctx.glyphs, choice.probability, 8);
        let chosen = trace.chosen == Some(index);
        let marker = if chosen {
            ctx.glyphs.resolve(glyphs::triangle_right())
        } else {
            " "
        };
        let label_style = if chosen {
            ctx.theme.accent(accent)
        } else {
            ctx.theme.fg()
        };
        output.push(BlockLine::new(
            Line::from(vec![
                Span::styled(indent.clone(), ctx.theme.dim()),
                Span::styled(format!("{marker} "), ctx.theme.accent(accent)),
                Span::styled(format!("{:.2} ", choice.probability), ctx.theme.muted()),
                Span::styled(format!("{bar} "), ctx.theme.accent(accent)),
                Span::styled(choice.label.clone(), label_style),
                Span::styled(format!(" ({})", choice.id), ctx.theme.dim()),
            ]),
            Selectable::All,
        ));
    }

    let verdict = match trace.chosen.and_then(|index| trace.choices.get(index)) {
        Some(choice) => format!(
            "采纳 {} {separator} {}",
            choice.label,
            trace.summary(separator)
        ),
        None => trace.summary(separator),
    };
    let verdict_style = if uncertainty.is_uncertain() {
        ctx.theme.accent(ctx.theme.accent_warning)
    } else {
        ctx.theme.muted()
    };
    for (index, wrapped) in width::wrap_words(&verdict, body_width).iter().enumerate() {
        output.push(
            BlockLine::new(
                Line::from(vec![
                    Span::styled(indent.clone(), ctx.theme.dim()),
                    Span::styled(wrapped.clone(), verdict_style),
                ]),
                Selectable::All,
            )
            .mark_soft_if(index > 0),
        );
    }
    output
}

/// 概率条：`width` 列宽的一条填充条（花体 9 级半格块，回退用阴影块），
/// 刻度来自 `glyphs::PROGRESS_LEVELS`。
pub fn probability_bar(glyphs: Glyphs, probability: f32, width: usize) -> String {
    let clamped = probability.clamp(0.0, 1.0);
    let exact = clamped * width as f32;
    let full = exact.floor() as usize;
    let partial = exact - exact.floor();
    let mut bar = String::new();
    for column in 0..width {
        if column < full {
            bar.push_str(glyphs::progress_glyph(glyphs, 8));
        } else if column == full {
            let level = (partial * 8.0).round() as u8;
            let glyph = glyphs::progress_glyph(glyphs, level);
            if glyph.is_empty() {
                bar.push(' ');
            } else {
                bar.push_str(glyph);
            }
        } else {
            bar.push(' ');
        }
    }
    bar
}

impl BlockLine {
    /// 给续行打软换行标记（`joiner = " "`）。
    fn mark_soft_if(self, soft: bool) -> Self {
        if soft { self.soft_wrapped() } else { self }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{ColorLevel, Theme};

    fn ctx<'a>(theme: &'a Theme, decision: Option<&'a DecisionTrace>) -> BlockCtx<'a> {
        BlockCtx {
            width: 40,
            theme,
            glyphs: Glyphs::FANCY,
            frame: 0,
            decision,
        }
    }

    #[test]
    fn choice_probability_is_preserved_not_collapsed() {
        let trace = DecisionTrace::new(
            "shell_command_allowed",
            vec![
                Choice::new("usr:allow", "允许一次", 0.71),
                Choice::new("usr:deny", "拒绝", 0.22),
                Choice::new("usr:always", "总是允许", 0.07),
            ],
        )
        .with_confidence(0.64);

        assert_eq!(trace.top().unwrap().id, "usr:allow");
        assert!((trace.margin().unwrap() - 0.49).abs() < 1e-6);
        assert_eq!(trace.uncertainty(), Uncertainty::Leaning);
        assert!(
            !trace.uncertainty().is_uncertain(),
            "0.71 对 0.22 属于「倾向」"
        );
        assert!(
            trace
                .summary(Glyphs::FANCY.resolve(glyphs::separator()))
                .contains("0.71")
        );
        assert!(
            trace
                .summary(Glyphs::FANCY.resolve(glyphs::separator()))
                .contains("倾向")
        );
        // 概率没有被归一化成名次：原始值必须原样可读。
        assert_eq!(trace.choices[2].probability, 0.07);
    }

    #[test]
    fn decision_block_draws_every_option_probability() {
        let theme = Theme::DARK;
        let trace = DecisionTrace::new(
            "shell_command_allowed",
            vec![
                Choice::new("usr:allow", "允许一次", 0.47),
                Choice::new("usr:deny", "拒绝", 0.44),
            ],
        )
        .with_confidence(0.55)
        .with_chosen(0);
        let block = Block::decision(trace);
        let output = block.output(&ctx(&theme, None));
        let text = output.plain_text();
        assert!(text.contains("0.47"), "{text}");
        assert!(text.contains("0.44"), "{text}");
        assert!(text.contains("不确定"), "{text}");
        assert!(text.contains("confidence 0.55"), "{text}");
    }

    #[test]
    fn probability_bar_keeps_requested_width() {
        for probability in [0.0, 0.07, 0.5, 0.99, 1.0] {
            for glyphs in [Glyphs::FANCY, Glyphs::FALLBACK] {
                let bar = probability_bar(glyphs, probability, 8);
                assert_eq!(
                    crate::width::display_width(&bar),
                    8,
                    "概率 {probability} 的条宽漂移：{bar:?}"
                );
            }
        }
        assert_eq!(probability_bar(Glyphs::FANCY, 1.0, 4), "████");
        assert_eq!(probability_bar(Glyphs::FANCY, 0.0, 4), "    ");
    }

    #[test]
    fn soft_wrap_joiner_is_recorded_and_reused() {
        let theme = Theme::DARK;
        let block = Block::assistant("one two three four five six seven eight nine ten");
        let narrow = BlockCtx {
            width: 12,
            theme: &theme,
            glyphs: Glyphs::FANCY,
            frame: 0,
            decision: None,
        };
        let output = block.output(&narrow);
        assert!(output.lines.len() > 1);
        assert_eq!(output.lines[0].joiner, None, "第一条是逻辑行首");
        assert_eq!(output.lines[1].joiner.as_deref(), Some(" "), "续行是软换行");
        // 复制文本把软换行拼回空格，而不是换行符。
        let text = output.plain_text();
        assert!(text.contains("one two three"), "{text:?}");
        assert_eq!(text.lines().count(), 1, "软换行不应产生新逻辑行：{text:?}");
    }

    #[test]
    fn selection_spans_exclude_the_marker_column() {
        let theme = Theme::DARK;
        let block = Block::user("看一下仓库状态");
        let output = block.output(&ctx(&theme, None));
        let first = output.lines.first().unwrap();
        assert_eq!(first.selectable, Selectable::Spans(2..16));
        assert_eq!(first.selection_text().unwrap(), "看一下仓库状态");
        // 标记列 `❯ ` 不在复制文本里。
        assert!(!first.selection_text().unwrap().contains('❯'));
    }

    #[test]
    fn render_context_carries_the_current_decision() {
        let theme = Theme::DARK;
        let trace = DecisionTrace::new("plan_step_ok", vec![Choice::new("yes", "继续", 0.9)]);
        let ctx = ctx(&theme, Some(&trace));
        assert_eq!(ctx.decision.unwrap().top().unwrap().id, "yes");
        // 决策块自身仍以块载荷为准（历史里的多个判定块不能共用一个 ctx）。
        let block = Block::decision(trace.clone());
        let output = block.output(&ctx);
        assert!(output.plain_text().contains("plan_step_ok"));
    }

    #[test]
    fn no_color_theme_keeps_block_text_intact() {
        let theme = Theme::DARK.quantized(ColorLevel::None);
        let block = Block::tool("bash", "git status --short", ToolStatus::Ok);
        let output = block.output(&ctx(&theme, None));
        let text = output.plain_text();
        assert!(text.contains("bash"), "{text}");
        assert!(text.contains("git status --short"), "{text}");
    }

    #[test]
    fn running_tool_uses_spinner_frames_and_stays_one_column() {
        let theme = Theme::DARK;
        let block = Block::tool("cargo", "test -p jh-tui", ToolStatus::Running);
        let mut widths = Vec::new();
        for frame in 0..8 {
            let ctx = BlockCtx {
                width: 40,
                theme: &theme,
                glyphs: Glyphs::FANCY,
                frame,
                decision: None,
            };
            let text = block.output(&ctx).lines[0].text();
            widths.push(text.chars().next().unwrap());
        }
        assert!(widths.contains(&'⠋'));
        assert!(widths.contains(&'⠙'));
        // 动画帧变而首行内容宽度不变（否则整块会随节拍跳动）。
        for frame in 0..8 {
            let fallback = BlockCtx {
                width: 40,
                theme: &theme,
                glyphs: Glyphs::FALLBACK,
                frame,
                decision: None,
            };
            assert_eq!(
                crate::width::display_width(&block.output(&fallback).lines[0].text()),
                crate::width::display_width(
                    &block
                        .output(&BlockCtx {
                            glyphs: Glyphs::FANCY,
                            ..fallback
                        })
                        .lines[0]
                        .text()
                ),
            );
        }
    }

    #[test]
    fn kinds_are_distinguishable() {
        assert_eq!(Block::user("a").kind(), "user");
        assert_eq!(Block::assistant("a").kind(), "assistant");
        assert_eq!(Block::tool("bash", "", ToolStatus::Ok).kind(), "tool");
        assert_eq!(
            Block::decision(DecisionTrace::new("q", vec![])).kind(),
            "decision"
        );
        assert_eq!(Block::notice("阶段").kind(), "notice");
    }

    #[test]
    fn notice_lines_keep_the_marker_column_stable() {
        let theme = Theme::DARK;
        let ctx = BlockCtx {
            width: 24,
            theme: &theme,
            glyphs: Glyphs::FANCY,
            frame: 0,
            decision: None,
        };
        let marked = Block::notice("判定 12ms · 采纳 usr:allow 0.71").output(&ctx);
        let plain = Block::plain_notice("用时 1.5s").output(&ctx);
        let marked_text = marked.lines[0].text();
        let plain_text = plain.lines[0].text();
        assert!(marked_text.starts_with("◆ "), "{marked_text:?}");
        // 无标记的那条用等宽空白占位：正文与有标记的那条从同一列起（按显示列算，不按字节）。
        assert!(plain_text.starts_with("  "), "{plain_text:?}");
        assert_eq!(
            crate::width::display_width(&marked_text[..marked_text.find('判').unwrap()]),
            crate::width::display_width(&plain_text[..plain_text.find('用').unwrap()]),
        );
        // 语气只影响样式，不改文本。
        let failed = Block::notice("判定失败").with_tone(NoticeTone::Error);
        assert!(failed.output(&ctx).plain_text().contains("判定失败"));
    }

    #[test]
    fn user_block_fills_the_row_and_keeps_the_timestamp_out_of_the_selection() {
        let theme = Theme::DARK;
        let ctx = BlockCtx {
            width: 30,
            theme: &theme,
            glyphs: Glyphs::FANCY,
            frame: 0,
            decision: None,
        };
        let output = Block::user_at("看一下仓库状态", "14:22").output(&ctx);
        let first = &output.lines[0];
        // 30 列 = `❯ `(2) + 正文(14) + 间隙(9) + 时间戳(5)：时间戳贴右端。
        assert_eq!(first.text(), "❯ 看一下仓库状态         14:22");
        assert_eq!(first.selection_text().unwrap(), "看一下仓库状态");
        assert!(!first.selection_text().unwrap().contains("14:22"));
        // 填充条铺满整行（快照与不透明性都依赖这一点）。
        let line = first.content.clone();
        assert_eq!(
            line.spans.last().unwrap().style.bg,
            Some(theme.user_block_bg)
        );
    }

    #[test]
    fn user_block_without_timestamp_still_fills_the_row() {
        let theme = Theme::DARK.quantized(ColorLevel::None);
        let ctx = BlockCtx {
            width: 20,
            theme: &theme,
            glyphs: Glyphs::FANCY,
            frame: 0,
            decision: None,
        };
        let output = Block::user("你好").output(&ctx);
        // 无色档不铺底色（Reset 会盖掉终端自己的背景）。
        assert!(
            output.lines[0]
                .content
                .spans
                .iter()
                .all(|span| span.style.bg.is_none())
        );
    }
}
