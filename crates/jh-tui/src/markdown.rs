//! 助手正文的轻量 markdown 子集：解析成带样式的行，再按显示宽度折行。
//!
//! 设计基准：`docs/tui-design.md` §4.4。范围边界见 [`crate::lib`] 的「不做」清单：
//! 只做助手正文用得上的那一小撮结构，不做完整 CommonMark、不做语法高亮，
//! 也不引入 `pulldown-cmark` / `syntect`。
//!
//! 两条硬约束：
//!
//! - **认不出来就原样当普通文本**：不报错、不吞字符，半截语法只影响它自己那一行；
//! - **流式安全**：正文可能停在半个 `**` 或未闭合的围栏上——未闭合的围栏把它之后的内容
//!   全当代码块渲染，未闭合的行内标记原样落字。
//!
//! 宽度一律走 [`crate::width`] 的显示列度量（CJK 宽字符算 2 列）；折行与
//! [`crate::width::wrap_words`] 同一个 `textwrap` 引擎，样式按字符贴回。
//!
//! 样式映射（色值全部来自 [`Theme`]，无色档自动退化成纯修饰符）：
//!
//! | 结构 | 文本 | 样式 |
//! | --- | --- | --- |
//! | `#` / `##` / `###` | 去掉 `#` 标记 | `theme.accent(theme.accent_system)`（BOLD + 强调色） |
//! | `- ` / `* ` / `+ ` | 换成 `• `，每级缩进 2 空格 | 标记 `theme.dim()`，正文 `theme.fg()` |
//! | `1. ` | 保留序号 | 标记 `theme.dim()`，正文 `theme.fg()` |
//! | ```` ``` ```` / ```` ```lang ```` | 块内原样（不解析行内） | 正文 `theme.muted()`，左缩进 2 格；`lang` 单独一行 `theme.dim()` |
//! | `> ` | 去掉 `>`，行首 `▏ ` | `theme.dim()` |
//! | `---` / `***` | 一行 `─` | `theme.dim()`，宽度铺满正文列 |
//! | `**粗**` | 去掉标记 | 基础样式 + `BOLD` |
//! | `*斜*` / `_斜_` | 去掉标记 | 基础样式 + `ITALIC` |
//! | `` `代码` `` | 去掉标记 | 基础样式 + `theme.accent_tool` |
//! | `[文本](url)` | `文本 (url)` | 文本 `theme.accent_system`，url `theme.dim()` |
//! | GFM 管道表格 | 排版成定宽的行（认表口径见 `table()`） | 表头 `theme.accent(theme.accent_system)`，规则线与 `│` `theme.dim()`，正文 `theme.fg()` |
//!
//! 表格是唯一**在这里就把宽度定死**的结构：每一条表格行的显示宽度都 ≤ `width`，
//! 所以块层的 [`wrap_lines`] 不会再折它（折行只可能把它的对齐拆坏）。

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::theme::Theme;
use crate::width::{clip, display_width};

/// 无序列表的标记、引用的竖条与分隔线字符：设计 §4.4 直接定死了这三个字符，
/// 它们不在字形表里（`glyphs.rs` 的花体 / 回退对不含列表与引用标记）。
const BULLET: &str = "• ";
const QUOTE: &str = "▏ ";
const RULE: char = '─';
/// 代码块的字面缩进（两格）。
const CODE_INDENT: &str = "  ";
/// 表格的列分隔符与规则线交叉点：两者都是 3 列，列宽才对得齐。
const COLUMN_SEP: &str = " │ ";
const RULE_SEP: &str = "─┼─";
/// 截断标记：单元格内容超宽、或右侧的列被裁掉时补它。
const ELLIPSIS: &str = "…";
/// 上面三个字面量的显示宽度（都是 1 列；分隔符 3 列）。
const ELLIPSIS_WIDTH: usize = 1;
const COLUMN_SEP_WIDTH: usize = 3;

/// 解析 markdown 子集 → 带样式的逻辑行（未折行）。
///
/// `width` 是正文可用列宽：分隔线用它铺满，表格用它排版（表格行排完就 ≤ `width`，
/// 其余结构的文本与样式都与布局无关，折行交给 [`wrap_lines`]）。
/// 除「只写了一个围栏标记」的行与表格（多条源行并成一次排版）之外，输入行与输出行一一对应：
/// 空行原样留空行，围栏标记本身不上屏。
pub fn parse(text: &str, theme: &Theme, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut in_fence = false;
    // 表格要往后看一行（分隔行），认下来之后整块吃掉：跳过的行数在这里记账。
    let mut skip = 0;
    let rows: Vec<&str> = text.split('\n').collect();
    for (index, raw) in rows.iter().copied().enumerate() {
        if skip > 0 {
            skip -= 1;
            continue;
        }
        // 围栏代码块：闭合前的内容整块当代码，块内不做行内解析。
        // 流式正文停在未闭合的围栏上时，这里就是「之后的内容全当代码块」的降级点。
        if in_fence {
            if fence(raw).is_some() {
                in_fence = false; // 闭合围栏只是标记，不上屏。
            } else {
                lines.push(code_line(raw, theme));
            }
            continue;
        }
        if let Some(info) = fence(raw) {
            in_fence = true;
            if !info.is_empty() {
                // info string（语言名）单独放块首：只标注，不做语法高亮。
                lines.push(Line::from(Span::styled(
                    format!("{CODE_INDENT}{info}"),
                    theme.dim(),
                )));
            }
            continue;
        }
        if let Some(title) = heading(raw) {
            // 标题：整行 BOLD + accent_system；行内标记在这个基础样式上细化。
            lines.push(Line::from(inline(
                title,
                theme.accent(theme.accent_system),
                theme,
            )));
            continue;
        }
        let trimmed = raw.trim_start();
        if is_rule(trimmed) {
            let rule = RULE.to_string().repeat(width);
            lines.push(Line::from(Span::styled(rule, theme.dim())));
            continue;
        }
        if let Some(text) = trimmed.strip_prefix('>') {
            // 引用：dim + 行首 `▏ `；只认一层（嵌套引用的第二层退化成普通文本）。
            let mut spans = vec![Span::styled(QUOTE, theme.dim())];
            spans.extend(inline(
                text.strip_prefix(' ').unwrap_or(text),
                theme.dim(),
                theme,
            ));
            lines.push(Line::from(spans));
            continue;
        }
        if let Some((prefix, text)) = list_item(raw) {
            let mut spans = vec![Span::styled(prefix, theme.dim())];
            spans.extend(inline(text, theme.fg(), theme));
            lines.push(Line::from(spans));
            continue;
        }
        // 管道表格：放在最后、普通文本之前——标题 / 列表 / 引用 / 围栏的标记优先。
        // 认不出来（没有分隔行、列数不齐、分隔行不合法）就落到下面逐字原样落字。
        if let Some((rendered, consumed)) = table(&rows[index..], theme, width) {
            lines.extend(rendered);
            skip = consumed - 1;
            continue;
        }
        lines.push(Line::from(inline(raw, theme.fg(), theme)));
    }
    lines
}

/// 按显示宽度折行，保留每段样式。
///
/// - `width` 是本条逻辑行**正文列**的可用列宽：行首前缀（列表标记、代码缩进、引用竖条）
///   算在它里面；
/// - 续行在正文列内与行首前缀对齐（列表项、引用、代码块的续行因此落在正文列而不是标记列下），
///   并在正文列之外再缩进 `indent` 格（块标记列，调用方保证 `width + indent` 不超区域宽度）；
/// - 折行与断词交给 `textwrap`（同 [`crate::width::wrap_words`] 的口径）：CJK 按 Unicode
///   断行机会断，放不下的长词（URL / 路径）硬切；
/// - `width` 为 0 时折不出任何行（与 [`crate::width::wrap_words`] 一致）。
pub fn wrap_lines(lines: Vec<Line<'static>>, width: usize, indent: usize) -> Vec<Line<'static>> {
    if width == 0 {
        return Vec::new();
    }
    let mut output = Vec::new();
    for line in lines {
        wrap_line(line, width, indent, &mut output);
    }
    output
}

/// 折一条逻辑行。
fn wrap_line(line: Line<'static>, width: usize, indent: usize, output: &mut Vec<Line<'static>>) {
    // 行首前缀（列表标记 / 代码缩进 / 引用竖条）算在正文列之内，续行在正文列内与它对齐；
    // 块标记列 `indent` 在正文列之外，由这里补上。
    let prefix = prefix_width(&line);
    let inner = " ".repeat(prefix);
    let outer = " ".repeat(indent);
    let plain: String = line
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect();

    let runs = style_runs(&line.spans);
    let mut cursor = 0;
    let options = textwrap::Options::new(width).subsequent_indent(&inner);
    for (index, row) in textwrap::wrap(&plain, options).iter().enumerate() {
        // textwrap 给续行插了 subsequent_indent：剥掉，缩进由本函数按「列内 + 列外」重建。
        let body = match index {
            0 => row.as_ref(),
            _ => row.strip_prefix(inner.as_str()).unwrap_or(row),
        };
        // 折行只会丢掉空白：从 cursor 起跳过它，就得到本行在源文本里的字节区间。
        while cursor < plain.len() && !plain[cursor..].starts_with(body) {
            match plain[cursor..].chars().next() {
                Some(ch) if ch.is_whitespace() => cursor += ch.len_utf8(),
                _ => break,
            }
        }
        let end = (cursor + body.len()).min(plain.len());
        let mut spans = Vec::new();
        if index > 0 && !(outer.is_empty() && inner.is_empty()) {
            spans.push(Span::styled(format!("{outer}{inner}"), Style::new()));
        }
        spans.extend(slice_runs(&runs, &plain, cursor..end));
        output.push(Line::from(spans));
        cursor = end;
    }
}

/// 行首前缀宽度：markdown 解析出的行用首段当前缀（缩进空格 / `• ` / `1. ` / `▏ `），
/// 续行缩进与它对齐；普通正文与标题没有前缀，返回 0。
///
/// 判定口径：首段以空格收尾，且去掉空白后是空串、`•`、`▏` 或「数字.」。
fn prefix_width(line: &Line<'static>) -> usize {
    let Some(first) = line.spans.first() else {
        return 0;
    };
    let text = first.content.as_ref();
    if !text.ends_with(' ') {
        return 0;
    }
    let marker = text.trim();
    let ordered = marker.strip_suffix('.').is_some_and(|digits| {
        !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
    });
    if marker.is_empty() || marker == "•" || marker == "▏" || ordered {
        display_width(text)
    } else {
        0
    }
}

/// 源文本的样式切片表：`(字节区间, 样式)`，按顺序铺满整行。
fn style_runs(spans: &[Span<'static>]) -> Vec<(std::ops::Range<usize>, Style)> {
    let mut runs = Vec::with_capacity(spans.len());
    let mut start = 0;
    for span in spans {
        let end = start + span.content.len();
        runs.push((start..end, span.style));
        start = end;
    }
    runs
}

/// 取 `range` 覆盖到的样式切片（每段的样式原样保留，不做合并）。
fn slice_runs(
    runs: &[(std::ops::Range<usize>, Style)],
    plain: &str,
    range: std::ops::Range<usize>,
) -> Vec<Span<'static>> {
    runs.iter()
        .filter_map(|(run, style)| {
            let start = run.start.max(range.start);
            let end = run.end.min(range.end);
            (start < end).then(|| Span::styled(plain[start..end].to_owned(), *style))
        })
        .collect()
}

/// 行内四种标记 + 字面文本：`**粗**`、`*斜*` / `_斜_`、`` `代码` ``、`[文本](url)`。
/// 认不出来（未闭合、空内容、`snake_case`）就原样落字。
fn inline(text: &str, base: Style, theme: &Theme) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut literal = String::new();
    let mut rest = text;
    let mut previous: Option<char> = None;
    while let Some(ch) = rest.chars().next() {
        match marker(rest, previous, base, theme) {
            Some((marked, consumed)) => {
                flush(&mut spans, &mut literal, base);
                spans.extend(marked);
                previous = rest[..consumed].chars().next_back();
                rest = &rest[consumed..];
            }
            None => {
                literal.push(ch);
                previous = Some(ch);
                rest = &rest[ch.len_utf8()..];
            }
        }
    }
    flush(&mut spans, &mut literal, base);
    spans
}

/// 攒下的字面文本落成一个 span（保持「普通文本是一整段」）。
fn flush(spans: &mut Vec<Span<'static>>, literal: &mut String, style: Style) {
    if !literal.is_empty() {
        spans.push(Span::styled(std::mem::take(literal), style));
    }
}

/// 在 `rest` 行首认一个行内标记：返回（片段, 消费的字节数）；`None` 表示这个字符不是标记。
///
/// 认出来但内容不合法的标记（未闭合、空内容）也返回片段——把定界符整段原样落字并跳过，
/// 免得半个 `**` 的第二个 `*` 又被认成斜体起始。`previous` 是行首前一个字符，
/// 只给 `_斜_` 判词边界用。
fn marker(
    rest: &str,
    previous: Option<char>,
    base: Style,
    theme: &Theme,
) -> Option<(Vec<Span<'static>>, usize)> {
    if let Some(body) = rest.strip_prefix("**") {
        return Some(match body.find("**") {
            None | Some(0) => (vec![Span::styled("**", base)], 2),
            Some(end) => (
                vec![Span::styled(
                    body[..end].to_owned(),
                    base.add_modifier(Modifier::BOLD),
                )],
                end + 4,
            ),
        });
    }
    if let Some(body) = rest.strip_prefix('`') {
        return Some(match body.find('`') {
            None | Some(0) => (vec![Span::styled("`", base)], 1),
            Some(end) => (
                vec![Span::styled(
                    body[..end].to_owned(),
                    base.patch(theme.frame(theme.accent_tool)),
                )],
                end + 2,
            ),
        });
    }
    if let Some(body) = rest.strip_prefix('[') {
        return Some(match link(body) {
            Some((label, url, consumed)) => (
                vec![
                    Span::styled(label, base.patch(theme.frame(theme.accent_system))),
                    Span::styled(format!(" ({url})"), base.patch(theme.dim())),
                ],
                consumed,
            ),
            None => (vec![Span::styled("[", base)], 1),
        });
    }
    if let Some(body) = rest.strip_prefix('*') {
        return Some(match emphasis(body, '*') {
            Some(end) => (
                vec![Span::styled(
                    body[..end].to_owned(),
                    base.add_modifier(Modifier::ITALIC),
                )],
                end + 2,
            ),
            None => (vec![Span::styled("*", base)], 1),
        });
    }
    if let Some(body) = rest.strip_prefix('_') {
        // `_斜_` 只在词的两端认：否则 `snake_case_name` 会被吃掉两个下划线。
        let left_flanking = previous.is_none_or(|ch| !ch.is_alphanumeric());
        let closed = emphasis(body, '_').filter(|end| {
            body[end + 1..]
                .chars()
                .next()
                .is_none_or(|ch| !ch.is_alphanumeric())
        });
        return Some(match (left_flanking, closed) {
            (true, Some(end)) => (
                vec![Span::styled(
                    body[..end].to_owned(),
                    base.add_modifier(Modifier::ITALIC),
                )],
                end + 2,
            ),
            _ => (vec![Span::styled("_", base)], 1),
        });
    }
    None
}

/// 找闭合定界符：内容非空、两端都不挨着空白（`* 12 * 3` 里的星号不是斜体）。
fn emphasis(body: &str, delimiter: char) -> Option<usize> {
    let end = body.find(delimiter)?;
    let content = &body[..end];
    (!content.is_empty()
        && !content.starts_with(char::is_whitespace)
        && !content.ends_with(char::is_whitespace))
    .then_some(end)
}

/// `[文本](url)`：返回（文本, url, 消费的字节数）；不成对就返回 `None`（`[` 按字面落字）。
fn link(body: &str) -> Option<(String, String, usize)> {
    let close = body.find(']')?;
    let url = body[close + 1..].strip_prefix('(')?;
    let end = url.find(')')?;
    if close == 0 || end == 0 {
        return None;
    }
    Some((
        body[..close].to_owned(),
        url[..end].to_owned(),
        close + end + 4,
    ))
}

/// 标题：1..=3 个 `#` + 一个空格（`#标签` 与 `####` 都不是标题，原样当普通文本）。
fn heading(raw: &str) -> Option<&str> {
    let hashes = raw.len() - raw.trim_start_matches('#').len();
    if !(1..=3).contains(&hashes) {
        return None;
    }
    raw[hashes..].strip_prefix(' ').map(str::trim_start)
}

/// 分隔线：整行都是 `-` 或整行都是 `*`，至少 3 个。
fn is_rule(trimmed: &str) -> bool {
    trimmed.len() >= 3
        && (trimmed.chars().all(|ch| ch == '-') || trimmed.chars().all(|ch| ch == '*'))
}

/// 围栏标记：返回 info string（无 info 时是空串）；不是围栏返回 `None`。
///
/// 四个及以上反引号不在子集里，按普通文本处理（这样它也关不掉一个三反引号的代码块，
/// 代码块里出现的 ````` ```` ````` 不会把块提前截断）。
fn fence(raw: &str) -> Option<&str> {
    let rest = raw.trim_start().strip_prefix("```")?;
    if rest.starts_with('`') {
        return None;
    }
    Some(rest.trim())
}

/// 代码行：不做行内解析，左缩进 2 格，正文用 muted（`gray`）。
fn code_line(raw: &str, theme: &Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled(CODE_INDENT, theme.dim()),
        Span::styled(raw.to_owned(), theme.muted()),
    ])
}

/// 列表项：`- ` / `* ` / `+ ` 换成 `• `，有序列表保留序号；缩进每级 2 空格。
/// 返回（行首前缀, 正文）。
fn list_item(raw: &str) -> Option<(String, &str)> {
    let indent = raw.len() - raw.trim_start_matches(' ').len();
    let level = indent / 2;
    let body = &raw[indent..];
    for marker in ["- ", "* ", "+ "] {
        if let Some(text) = body.strip_prefix(marker) {
            return Some((format!("{}{BULLET}", "  ".repeat(level)), text));
        }
    }
    let digits = body.len()
        - body
            .trim_start_matches(|ch: char| ch.is_ascii_digit())
            .len();
    if digits == 0 || !body[digits..].starts_with(". ") {
        return None;
    }
    Some((
        format!("{}{}. ", "  ".repeat(level), &body[..digits]),
        &body[digits + 2..],
    ))
}

// ------------------------------------------------------------------ 管道表格

/// 单元格的横向对齐，来自分隔行的 `:`。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Align {
    Left,
    Center,
    Right,
}

/// 认一张 GFM 管道表格并排版：`rows[0]` 是表头、`rows[1]` 是分隔行时认表，
/// 返回（排好版的行, 吃掉的行数）；否则 `None`（调用方按普通文本逐字落字）。
///
/// 认表口径（比 GFM 保守，宁可退回普通文本也不猜）：
///
/// - 表头与分隔行都必须含未转义的 `|`（分隔行不带 `|` 时它就是一条分隔线，不是表）；
/// - 两行列数必须相等，分隔行每个单元格必须是 `:?-+:?`（`:--` 左 / `--:` 右 / `:-:` 中）；
/// - 首尾 `|` 可有可无（`| a | b |` 与 `a | b` 同义），`\|` 是单元格里的字面竖线；
/// - 数据行必须含未转义的 `|`，否则表格到此为止（GFM 会把没有竖线的一行当成单列行，
///   这里不做：让它回到普通文本）；数据行列数按表头补齐 / 截断。
fn table(rows: &[&str], theme: &Theme, width: usize) -> Option<(Vec<Line<'static>>, usize)> {
    let header = split_row(rows.first()?)?;
    let delimiter = split_row(rows.get(1)?)?;
    if header.len() != delimiter.len() {
        return None;
    }
    let aligns = delimiter
        .iter()
        .map(|cell| alignment(cell))
        .collect::<Option<Vec<Align>>>()?;
    let mut body = Vec::new();
    for row in &rows[2..] {
        let Some(cells) = split_row(row) else {
            break;
        };
        body.push(cells);
    }
    let rendered = render_table(&header, &body, &aligns, theme, width);
    Some((rendered, body.len() + 2))
}

/// 拆一行表格：按未转义的 `|` 切单元格、剥掉首尾各一个 `|`、`\|` 还原成字面 `|`，
/// 每个单元格去掉两端空白。整行没有未转义的 `|` 时返回 `None`（这行不是表格行）。
fn split_row(row: &str) -> Option<Vec<String>> {
    let row = row.trim();
    let mut cells: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut escaped = false;
    let mut saw_pipe = false;
    let mut ends_with_sep = false;
    for ch in row.chars() {
        if escaped {
            escaped = false;
            if ch != '|' {
                // 子集不做通用转义：`\` 后不是 `|` 就把反斜杠原样留着。
                current.push('\\');
            }
            current.push(ch);
            ends_with_sep = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '|' => {
                saw_pipe = true;
                ends_with_sep = true;
                cells.push(std::mem::take(&mut current));
            }
            _ => {
                current.push(ch);
                ends_with_sep = false;
            }
        }
    }
    if escaped {
        current.push('\\');
    }
    cells.push(current);
    if !saw_pipe {
        return None;
    }
    // 首尾的 `|` 切出来的空壳不算单元格；`|` 这种只剩一格的写法要留住那一格。
    if cells.len() >= 2 && row.starts_with('|') {
        cells.remove(0);
    }
    if cells.len() >= 2 && ends_with_sep {
        cells.pop();
    }
    Some(
        cells
            .into_iter()
            .map(|cell| cell.trim().to_owned())
            .collect(),
    )
}

/// 分隔行单元格的对齐方式；不是 `:?-+:?` 就不是分隔行（`None`）。
fn alignment(cell: &str) -> Option<Align> {
    let body = cell.strip_prefix(':').unwrap_or(cell);
    let body = body.strip_suffix(':').unwrap_or(body);
    if body.is_empty() || !body.bytes().all(|byte| byte == b'-') {
        return None;
    }
    Some(match (cell.starts_with(':'), cell.ends_with(':')) {
        (true, true) => Align::Center,
        (false, true) => Align::Right,
        _ => Align::Left,
    })
}

/// 排版一张表格：表头行 + `─┼─` 规则线 + 数据行。每行的显示宽度都 ≤ `width`。
fn render_table(
    header: &[String],
    body: &[Vec<String>],
    aligns: &[Align],
    theme: &Theme,
    width: usize,
) -> Vec<Line<'static>> {
    if width == 0 {
        // 一列也画不出来：只保证行数对得上、不 panic、不越界（块层不会给 0）。
        return vec![Line::default(); body.len() + 2];
    }
    // 自然宽度：本列所有单元格的最大显示宽度，空列也占 1 列（否则规则线会断）。
    // 宽度按去掉行内标记之后的文本算（`` `码` `` 占 2 列而不是 4 列）。
    let columns = header.len();
    let mut natural = vec![1usize; columns];
    for (index, cell) in header.iter().enumerate() {
        natural[index] = natural[index].max(cell_width(cell, theme));
    }
    for row in body {
        for (index, cell) in row.iter().enumerate().take(columns) {
            natural[index] = natural[index].max(cell_width(cell, theme));
        }
    }
    let (widths, dropped) = column_widths(&natural, width);
    let mut lines = Vec::with_capacity(body.len() + 2);
    lines.push(row_line(
        header,
        &widths,
        aligns,
        theme.accent(theme.accent_system),
        theme,
        dropped,
    ));
    lines.push(rule_line(&widths, dropped, theme));
    for row in body {
        lines.push(row_line(row, &widths, aligns, theme.fg(), theme, dropped));
    }
    lines
}

/// 列宽分配：放得下就用自然宽度；放不下按自然宽度比例收窄（每列至少 1 列）；
/// 窄到每列 1 列都放不下时从右侧裁列，末尾留 1 列 + 1 个分隔符给 `…`。
/// 返回（列宽, 是否裁掉了右侧的列）。
fn column_widths(natural: &[usize], width: usize) -> (Vec<usize>, bool) {
    let columns = natural.len();
    let overhead = COLUMN_SEP_WIDTH * (columns - 1);
    if natural.iter().sum::<usize>() + overhead <= width {
        return (natural.to_vec(), false);
    }
    let available = width.saturating_sub(overhead);
    if available >= columns {
        return (distribute(natural, available), false);
    }
    // 每保留一列要 1 列内容 + 3 列分隔符，末尾的 `…` 再要 1 列：4k + 1 ≤ width。
    let kept = ((width - 1) / 4).min(columns - 1);
    if kept == 0 {
        // 窄到连 `…` 的位置都没有（width ≤ 4）：只留第一列，裁掉的列不打标记。
        return (vec![natural[0].min(width)], false);
    }
    let available = width - (COLUMN_SEP_WIDTH * kept + ELLIPSIS_WIDTH);
    // 裁列之后余下的宽度按保留列的比例分，但不把列撑得比它的自然宽度还宽。
    let budget = available.min(natural[..kept].iter().sum());
    (distribute(&natural[..kept], budget), true)
}

/// 把 `budget`（≥ 列数）列分给各列：每列先占 1 列，余下按自然宽度的比例分，
/// 除不尽的余数给小数部分最大的列（并列时靠左的列先拿）。
fn distribute(natural: &[usize], budget: usize) -> Vec<usize> {
    let columns = natural.len();
    let total: usize = natural.iter().sum();
    let extra_total = budget - columns;
    let mut extra: Vec<usize> = natural
        .iter()
        .map(|width| width * extra_total / total)
        .collect();
    // 余数大的先补：整数除法丢掉的份额一定少于列数，`order` 够用。
    let missing = extra_total - extra.iter().sum::<usize>();
    let mut order: Vec<usize> = (0..columns).collect();
    order.sort_by_key(|index| std::cmp::Reverse((natural[*index] * extra_total) % total));
    for index in order.into_iter().take(missing) {
        extra[index] += 1;
    }
    extra.into_iter().map(|value| value + 1).collect()
}

/// 一条表格行：单元格按列宽裁剪 / 补齐，列间 ` │ `；裁过列时末尾补 ` │ …`。
fn row_line(
    cells: &[String],
    widths: &[usize],
    aligns: &[Align],
    base: Style,
    theme: &Theme,
    dropped: bool,
) -> Line<'static> {
    let mut spans = Vec::new();
    for (index, (&width, &align)) in widths.iter().zip(aligns).enumerate() {
        if index > 0 {
            spans.push(Span::styled(COLUMN_SEP, theme.dim()));
        }
        let cell = cells.get(index).map(String::as_str).unwrap_or("");
        spans.extend(cell_spans(cell, width, align, base, theme));
    }
    if dropped {
        spans.push(Span::styled(COLUMN_SEP, theme.dim()));
        spans.push(Span::styled(ELLIPSIS, theme.dim()));
    }
    trim_end_spaces(&mut spans);
    Line::from(spans)
}

/// 表头与正文之间的规则线：`─` 铺满每列，列间 `─┼─` 与 `│` 同列。
fn rule_line(widths: &[usize], dropped: bool, theme: &Theme) -> Line<'static> {
    let mut rule = String::new();
    for (index, width) in widths.iter().enumerate() {
        if index > 0 {
            rule.push_str(RULE_SEP);
        }
        rule.extend(std::iter::repeat_n(RULE, *width));
    }
    if dropped {
        rule.push_str(RULE_SEP);
        rule.push(RULE);
    }
    Line::from(Span::styled(rule, theme.dim()))
}

/// 一个单元格：行内标记照常解析 → 超宽按显示宽度 `…` 截断 → 按对齐方式补空格到列宽。
fn cell_spans(
    cell: &str,
    width: usize,
    align: Align,
    base: Style,
    theme: &Theme,
) -> Vec<Span<'static>> {
    let mut spans = inline(cell, base, theme);
    let mut used = spans_width(&spans);
    if used > width {
        spans = clip_spans(spans, width.saturating_sub(ELLIPSIS_WIDTH));
        used = spans_width(&spans);
        if width >= ELLIPSIS_WIDTH {
            spans.push(Span::styled(ELLIPSIS, base));
            used += ELLIPSIS_WIDTH;
        }
    }
    let pad = width.saturating_sub(used);
    let (left, right) = match align {
        Align::Left => (0, pad),
        Align::Right => (pad, 0),
        Align::Center => (pad / 2, pad - pad / 2),
    };
    if left > 0 {
        spans.insert(0, Span::styled(" ".repeat(left), base));
    }
    if right > 0 {
        spans.push(Span::styled(" ".repeat(right), base));
    }
    spans
}

/// 一个单元格排版后的显示宽度：样式不参与宽度，这里借 [`inline`] 把标记去掉再量。
fn cell_width(cell: &str, theme: &Theme) -> usize {
    spans_width(&inline(cell, Style::new(), theme))
}

/// 按显示宽度裁掉超出的部分（grapheme 整簇，样式分段原样保留）。
fn clip_spans(spans: Vec<Span<'static>>, max: usize) -> Vec<Span<'static>> {
    let mut budget = max;
    let mut clipped = Vec::with_capacity(spans.len());
    for span in spans {
        if budget == 0 {
            break;
        }
        let text = clip(span.content.as_ref(), budget);
        let width = display_width(text);
        if width > 0 {
            clipped.push(Span::styled(text.to_owned(), span.style));
        }
        budget -= width;
    }
    clipped
}

/// 若干片段的显示宽度。
fn spans_width(spans: &[Span<'static>]) -> usize {
    spans
        .iter()
        .map(|span| display_width(span.content.as_ref()))
        .sum()
}

/// 去掉行尾的补齐空格（右对齐与居中的补齐在行首，动不到）。
fn trim_end_spaces(spans: &mut Vec<Span<'static>>) {
    while let Some(last) = spans.last_mut() {
        let trimmed = last.content.trim_end_matches(' ');
        if trimmed.len() == last.content.len() {
            break;
        }
        if trimmed.is_empty() {
            spans.pop();
        } else {
            last.content = trimmed.to_owned().into();
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ColorLevel;

    fn text(line: &Line<'static>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    /// 多行的纯文本。
    fn texts(lines: &[Line<'static>]) -> Vec<String> {
        lines.iter().map(text).collect()
    }

    fn themed() -> Theme {
        Theme::DARK
    }

    fn style_of(line: &Line<'static>, index: usize) -> Style {
        line.spans[index].style
    }

    /// 带对齐、CJK 与截断需求的一张表：多数表格断言都拿它当样本。
    const CJK_TABLE: &str = "| 名称 | 说明 |\n| --- | :--: |\n| cpu | 中央处理器 |\n| mem | 内存 |";

    // -------------------------------------------------------------- 管道表格

    #[test]
    fn table_needs_a_delimiter_row_with_matching_cells() {
        let theme = themed();
        // 表头之后没有分隔行：逐字原样（流式正文只写到表头时就走这条）。
        assert_eq!(
            texts(&parse("| a | b |\n没有分隔行", &theme, 30)),
            vec!["| a | b |", "没有分隔行"]
        );
        assert_eq!(texts(&parse("| a | b |", &theme, 30)), vec!["| a | b |"]);
        // 分隔行列数与表头不一致：整块退回普通文本，一行都不吞。
        assert_eq!(
            texts(&parse("| a | b |\n| --- |\n| 1 | 2 |", &theme, 30)),
            vec!["| a | b |", "| --- |", "| 1 | 2 |"]
        );
        // 分隔行里混了非 `-` 的字符：也不是分隔行。
        assert_eq!(
            texts(&parse("| a | b |\n| --- | :x: |", &theme, 30)),
            vec!["| a | b |", "| --- | :x: |"]
        );
        // 分隔行还在长（`:--` 只写了一半的 `:`）：同样退回普通文本。
        assert_eq!(
            texts(&parse("| a | b |\n| --- | :", &theme, 30)),
            vec!["| a | b |", "| --- | :"]
        );
        // 单独一行 `|`：没有分隔行，也不是表。
        assert_eq!(texts(&parse("|", &theme, 30)), vec!["|"]);
        // 分隔行不带 `|` 时它就是分隔线（`---` 铺满宽度），不是表。
        assert_eq!(
            texts(&parse("| a |\n---", &theme, 4)),
            vec!["| a |", "────"]
        );
    }

    #[test]
    fn table_renders_aligned_columns_and_a_rule_line() {
        let theme = themed();
        let lines = parse(CJK_TABLE, &theme, 40);
        assert_eq!(
            texts(&lines),
            vec![
                "名称 │    说明",
                "─────┼───────────",
                "cpu  │ 中央处理器",
                "mem  │    内存",
            ]
        );
        // 表头：强调色 + BOLD；规则线与列分隔符：dim；正文：正文色。
        assert_eq!(style_of(&lines[0], 0).fg, Some(theme.accent_system));
        assert!(style_of(&lines[0], 0).add_modifier.contains(Modifier::BOLD));
        assert_eq!(style_of(&lines[1], 0).fg, Some(theme.gray_dim));
        assert_eq!(style_of(&lines[2], 2).fg, Some(theme.gray_dim), "列分隔符");
        assert_eq!(style_of(&lines[2], 0).fg, Some(theme.text_primary));
        // 自然宽度 4 + 3 + 10 = 17：表头与数据行的行尾补齐不算宽度，规则线铺满。
        assert_eq!(display_width(&text(&lines[1])), 17);
        assert!(lines.iter().all(|line| display_width(&text(line)) <= 17));
    }

    #[test]
    fn table_takes_cjk_width_and_all_three_alignments() {
        let theme = themed();
        // 不带首尾 `|` 的写法同样认；列宽按显示宽度算（这里每列都是 3 个汉字 = 6 列）。
        let lines = parse(
            "左 | 中 | 右\n:-- | :-: | --:\n1 | 2 | 3\n长一点 | 长一点 | 长一点",
            &theme,
            40,
        );
        assert_eq!(
            texts(&lines),
            vec![
                "左     │   中   │     右",
                "───────┼────────┼───────",
                "1      │   2    │      3",
                "长一点 │ 长一点 │ 长一点",
            ]
        );
        assert!(lines.iter().all(|line| display_width(&text(line)) == 24));
    }

    #[test]
    fn narrow_tables_shrink_cells_then_drop_columns_with_a_marker() {
        let theme = themed();
        // 12 列：按比例收窄到 3 + 3 + 6 = 12，超宽的单元格用 `…` 截断。
        assert_eq!(
            texts(&parse(CJK_TABLE, &theme, 12)),
            vec!["名… │  说明", "────┼───────", "cpu │ 中央…", "mem │  内存",]
        );
        // 8 列：每列 1 列都放不下（需要 4k + 1 ≤ 8），裁掉右侧的列并在末尾标 `…`。
        assert_eq!(
            texts(&parse(CJK_TABLE, &theme, 8)),
            vec!["…  │ 说…", "───┼────", "c… │ 中…", "m… │ 内…"]
        );
        // 三列、宽度 8：只留第一列 + `…`。
        assert_eq!(
            texts(&parse(
                "| a | b | c |\n| :- | :-: | -: |\n| 1 | 2 | 3 |",
                &theme,
                8
            )),
            vec!["a │ …", "──┼──", "1 │ …"]
        );
        // 窄到连 `…` 的位置都没有（width ≤ 4）：只留第一列，不打标记。
        assert_eq!(
            texts(&parse(CJK_TABLE, &theme, 4)),
            vec!["名称", "────", "cpu", "mem"]
        );
        assert_eq!(
            texts(&parse(CJK_TABLE, &theme, 1)),
            vec!["…", "─", "…", "…"]
        );
        // width 0：画不出任何一列，但行数与不 panic 都要守住。
        assert_eq!(texts(&parse(CJK_TABLE, &theme, 0)), vec!["", "", "", ""]);
    }

    #[test]
    fn column_widths_shrink_then_drop_columns() {
        // 放得下：自然宽度原样。
        assert_eq!(column_widths(&[4, 10], 40), (vec![4, 10], false));
        // 放不下：按比例收窄到刚好铺满（12 - 3 = 9 → 3 + 6）。
        assert_eq!(column_widths(&[4, 10], 12), (vec![3, 6], false));
        // 每列 1 列都放不下：裁列并在末尾留 `…` 的位置（4k + 1 ≤ width）。
        assert_eq!(column_widths(&[4, 10, 6], 8), (vec![4], true));
        assert_eq!(column_widths(&[4, 10, 6], 6), (vec![2], true));
        // 连 `…` 的位置都没有：只留第一列且不打标记。
        assert_eq!(column_widths(&[4, 10, 6], 4), (vec![4], false));
        // 单列只收窄，不裁。
        assert_eq!(column_widths(&[10], 6), (vec![6], false));
    }

    #[test]
    fn table_cells_keep_inline_styles_and_escaped_pipes() {
        let theme = themed();
        // 单元格里的行内标记：标记字符不上屏，样式保住；列宽按去标记之后的文本算。
        let lines = parse("| a | b |\n| - | - |\n| `码` | **粗** |", &theme, 30);
        assert_eq!(texts(&lines), vec!["a  │ b", "───┼───", "码 │ 粗"]);
        assert_eq!(style_of(&lines[2], 0).fg, Some(theme.accent_tool), "`码`");
        assert!(style_of(&lines[2], 2).add_modifier.contains(Modifier::BOLD));
        assert!(!text(&lines[2]).contains('`') && !text(&lines[2]).contains('*'));
        // `\|` 是单元格里的字面竖线，不切列。
        assert_eq!(
            texts(&parse(
                "| a \\| b | c |\n| --- | --- |\n| 1 | 2 |",
                &theme,
                30
            )),
            vec!["a | b │ c", "──────┼──", "1     │ 2"]
        );
    }

    #[test]
    fn table_rows_are_sized_and_body_rows_follow_the_header() {
        let theme = themed();
        // 少写的单元格按空补、多写的截掉，列数始终跟着表头。
        assert_eq!(
            texts(&parse(
                "| a | b |\n| --- | --- |\n| 1 |\n| 2 | 3 | 4 |",
                &theme,
                30
            )),
            vec!["a │ b", "──┼──", "1 │", "2 │ 3"]
        );
        // 表里的空单元格与空表头也要占满列宽（规则线不许断）。
        assert_eq!(
            texts(&parse("| |\n|---|\n| |", &theme, 20)),
            vec!["", "─", ""]
        );
        // 数据行没有未转义的 `|` 时表格就此打住，后面那行按普通文本走。
        assert_eq!(
            texts(&parse(
                "| a | b |\n| --- | --- |\n| 1 | 2 |\n这一行不是表格行",
                &theme,
                30
            )),
            vec!["a │ b", "──┼──", "1 │ 2", "这一行不是表格行"]
        );
        // 表格前后的行不受影响。
        let mixed = parse("# 标题\n| a | b |\n| --- | --- |\n- 列表项", &theme, 30);
        assert_eq!(texts(&mixed), vec!["标题", "a │ b", "──┼──", "• 列表项"]);
    }

    #[test]
    fn table_lines_never_exceed_the_width_nor_get_rewrapped() {
        let theme = themed();
        let sources = [
            CJK_TABLE,
            "左 | 中 | 右\n:-- | :-: | --:\n1 | 2 | 3\n长一点 | 长一点 | 长一点",
            "| a \\| b | c |\n| --- | --- |\n| 1 | 2 |",
            // 第一列右对齐：行首是补齐空格，折行引擎不许把它当缩进吃掉。
            "| 长长 | b |\n| --: | :-- |\n| 1 | 2 |",
        ];
        for source in sources {
            for width in 1..=40 {
                let lines = parse(source, &theme, width);
                for line in &lines {
                    assert!(
                        display_width(&text(line)) <= width,
                        "width={width} 排出了超宽的行：{line:?}"
                    );
                }
                // 块层用同一个宽度再折一次：表格行已经排好版，必须一行进一行出、一字不差。
                let rows = wrap_lines(lines.clone(), width, 2);
                assert_eq!(rows.len(), lines.len(), "width={width} 被二次折行");
                for (row, line) in rows.iter().zip(&lines) {
                    assert_eq!(text(row), text(line), "width={width}");
                }
            }
        }
    }

    #[test]
    fn prefix_width_reads_every_marker_form() {
        let theme = themed();
        for (source, expected) in [
            ("- 项", 2),
            ("  - 项", 4),
            ("12. 项", 4),
            ("> 引用", 2),
            ("```rust", 0),
            ("**粗**", 0),
            ("# 标题", 0),
            ("hello world", 0),
        ] {
            let line = parse(source, &theme, 20).remove(0);
            assert_eq!(prefix_width(&line), expected, "{source:?}");
        }
        // 代码块的缩进空格也算前缀。
        let code = parse("```\ncode\n```", &theme, 20).remove(0);
        assert_eq!(prefix_width(&code), 2);
    }

    #[test]
    fn unclosed_inline_markers_stay_literal() {
        let theme = themed();
        // 流式正文停在半个 `**` 上：标记与正文都还在，且第二个 `*` 不会被认成斜体。
        let line = parse("正在**加粗", &theme, 40).remove(0);
        assert_eq!(text(&line), "正在**加粗");
        assert!(
            line.spans
                .iter()
                .all(|span| !span.style.add_modifier.contains(Modifier::BOLD)),
            "未闭合的 `**` 不许把后面的字变成粗体：{line:?}"
        );
        // 半个 `[`、半个反引号同理。
        assert_eq!(text(&parse("[文本](ht", &theme, 40).remove(0)), "[文本](ht");
        assert_eq!(text(&parse("看 `code", &theme, 40).remove(0)), "看 `code");
    }

    #[test]
    fn emphasis_rejects_whitespace_and_snake_case() {
        let theme = themed();
        assert_eq!(
            text(&parse("算 a * 12 * 3", &theme, 40).remove(0)),
            "算 a * 12 * 3"
        );
        // 列表项之外的下划线不构成斜体：`snake_case_name` 一个字符都不少。
        let line = parse("变量 snake_case_name 在此", &theme, 40).remove(0);
        assert_eq!(text(&line), "变量 snake_case_name 在此");
        assert!(
            line.spans
                .iter()
                .all(|span| !span.style.add_modifier.contains(Modifier::ITALIC))
        );
        // 词的两端才认。
        let italic = parse("这是 _斜_ 体", &theme, 40).remove(0);
        assert!(
            italic
                .spans
                .iter()
                .any(|span| span.style.add_modifier.contains(Modifier::ITALIC))
        );
    }

    #[test]
    fn link_needs_both_halves() {
        let theme = themed();
        let link = parse("[文档](https://example.com)", &theme, 40).remove(0);
        assert_eq!(text(&link), "文档 (https://example.com)");
        assert_eq!(link.spans.len(), 2);
        assert_eq!(link.spans[0].style.fg, Some(theme.accent_system));
        assert_eq!(link.spans[1].style.fg, Some(theme.gray_dim));
        // 缺一半就原样落字。
        assert_eq!(
            text(&parse("[文档] 没有地址", &theme, 40).remove(0)),
            "[文档] 没有地址"
        );
        assert_eq!(text(&parse("[](空的)", &theme, 40).remove(0)), "[](空的)");
    }

    #[test]
    fn no_color_theme_keeps_every_character() {
        let theme = Theme::DARK.quantized(ColorLevel::None);
        let line = parse("# 标题 **粗** `码`", &theme, 40).remove(0);
        assert_eq!(text(&line), "标题 粗 码");
        for span in &line.spans {
            assert_eq!(span.style.fg, None, "无色档不许留下前景色");
        }
    }

    #[test]
    fn wrap_keeps_the_style_across_the_break() {
        let theme = themed();
        let line = parse("**这一段很长的粗体会被折断**", &theme, 8).remove(0);
        let rows = wrap_lines(vec![line], 8, 2);
        assert!(rows.len() > 1, "8 列放不下这么多字");
        // 拼回正文时要先剥掉续行缩进：缩进是装饰，不属于正文。
        let joined: String = rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                let text = text(row);
                match index {
                    0 => text,
                    _ => text.trim_start_matches(' ').to_owned(),
                }
            })
            .collect();
        assert_eq!(joined, "这一段很长的粗体会被折断");
        for row in &rows {
            let styled = &row.spans[row.spans.len() - 1];
            assert!(
                styled.style.add_modifier.contains(Modifier::BOLD),
                "折断后每一段都还该是粗体：{row:?}"
            );
        }
        // 续行缩进 2 格，正文宽度不超过 8。
        for row in rows.iter().skip(1) {
            assert!(text(row).starts_with("  "), "{row:?}");
            assert!(display_width(&text(row)) <= 10, "{row:?}");
        }
    }

    #[test]
    fn wrap_drops_no_character_and_keeps_cjk_width() {
        let theme = themed();
        let source = "中文正文很长很长需要折行显示的一段话";
        let rows = wrap_lines(parse(source, &theme, 6), 6, 0);
        assert_eq!(rows.iter().map(text).collect::<String>(), source);
        for row in &rows {
            assert!(display_width(&text(row)) <= 6, "{row:?}");
        }
    }

    #[test]
    fn rule_fills_the_content_width() {
        let theme = themed();
        let line = parse("---", &theme, 12).remove(0);
        assert_eq!(text(&line), "─".repeat(12));
        assert_eq!(style_of(&line, 0).fg, Some(theme.gray_dim));
        assert_eq!(text(&parse("***", &theme, 4).remove(0)), "────");
        // `--` 不足 3 个不是分隔线。
        assert_eq!(text(&parse("--", &theme, 4).remove(0)), "--");
    }

    #[test]
    fn four_backtick_fence_is_plain_text() {
        let theme = themed();
        let lines = parse("````\n```\ncode\n```", &theme, 20);
        // ```` 当普通文本，随后的 ``` 才是围栏：代码块内容只有一行。
        assert_eq!(text(&lines[0]), "````");
        assert_eq!(text(&lines[1]), "  code");
        assert_eq!(lines.len(), 2);
        assert_eq!(
            text(&parse("```\n````\nstill code\n```", &theme, 20)[0]),
            "  ````"
        );
    }
}
