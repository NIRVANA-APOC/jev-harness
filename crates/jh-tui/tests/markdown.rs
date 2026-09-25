//! 轻量 markdown 子集的逐条断言：解析出的文本与样式、管道表格的排版与收窄、
//! 按显示宽度折行、助手块的流式降级。
//!
//! 全部离线，不碰真实终端：断言打在 `Line` / `Span` 与 `Block::output` 上。

use jh_tui::blocks::{Block, BlockCtx, Selectable};
use jh_tui::glyphs::Glyphs;
use jh_tui::markdown::{parse, wrap_lines};
use jh_tui::theme::Theme;
use jh_tui::width::display_width;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;

fn theme() -> Theme {
    Theme::DARK
}

/// 一行的纯文本。
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

/// 折行结果拼回正文：续行的缩进是装饰，拼之前剥掉。
fn joined(lines: &[Line<'static>]) -> String {
    lines
        .iter()
        .enumerate()
        .map(|(index, line)| match index {
            0 => text(line),
            _ => text(line).trim_start_matches(' ').to_owned(),
        })
        .collect()
}

fn ctx<'a>(theme: &'a Theme, width: usize) -> BlockCtx<'a> {
    BlockCtx {
        width,
        theme,
        glyphs: Glyphs::FANCY,
        frame: 0,
        decision: None,
    }
}

// ------------------------------------------------------------------ 块级结构

#[test]
fn headings_drop_the_markers_and_take_bold_plus_accent() {
    let theme = theme();
    let lines = parse(
        "# 一级标题\n## 二级标题\n### 三级标题\n#### 四级不是标题",
        &theme,
        40,
    );
    for (index, level) in ["一", "二", "三"].iter().enumerate() {
        let line = &lines[index];
        assert_eq!(text(line), format!("{level}级标题"), "`#` 标记去掉");
        assert_eq!(line.spans.len(), 1, "整行一个 span");
        assert!(
            line.spans[0].style.add_modifier.contains(Modifier::BOLD),
            "标题整行 BOLD"
        );
        assert_eq!(line.spans[0].style.fg, Some(theme.accent_system));
    }
    // 四级不在子集里：原样当普通文本。
    assert_eq!(text(&lines[3]), "#### 四级不是标题");
    assert!(
        !lines[3].spans[0]
            .style
            .add_modifier
            .contains(Modifier::BOLD)
    );
    // `#标签`（没有空格）也不是标题。
    let hash_tag = parse("#标签", &theme, 40).remove(0);
    assert_eq!(text(&hash_tag), "#标签");
}

#[test]
fn unordered_items_take_a_bullet_and_keep_their_level() {
    let lines = parse("- 一级\n  - 二级\n    - 三级\n+ 加号\n* 星号", &theme(), 40);
    assert_eq!(
        texts(&lines),
        vec!["• 一级", "  • 二级", "    • 三级", "• 加号", "• 星号"]
    );
    // 标记是 dim，正文是正文色。
    assert_eq!(lines[0].spans[0].style.fg, Some(theme().gray_dim));
    assert_eq!(lines[0].spans[1].style.fg, Some(theme().text_primary));
    // `-没有空格` 不是列表项。
    assert_eq!(
        text(&parse("-没有空格", &theme(), 40).remove(0)),
        "-没有空格"
    );
}

#[test]
fn ordered_items_keep_their_numbers() {
    let lines = parse("1. 第一\n  10. 第十\n3.没有空格", &theme(), 40);
    assert_eq!(texts(&lines), vec!["1. 第一", "  10. 第十", "3.没有空格"]);
    assert_eq!(lines[1].spans[0].content.as_ref(), "  10. ");
}

#[test]
fn code_fences_keep_the_body_verbatim() {
    let theme = theme();
    let lines = parse(
        "```rust\nlet a = **1**;\n\nfn main() {}\n```\n收尾",
        &theme,
        40,
    );
    // 块首是单独一行的 info string（dim），不是语法高亮。
    assert_eq!(text(&lines[0]), "  rust");
    assert_eq!(lines[0].spans[0].style.fg, Some(theme.gray_dim));
    // 块内不做行内解析：`**1**` 原样，样式是 muted。
    assert_eq!(text(&lines[1]), "  let a = **1**;");
    assert_eq!(lines[1].spans[1].style.fg, Some(theme.gray));
    assert!(
        !lines[1]
            .spans
            .iter()
            .any(|span| span.style.add_modifier.contains(Modifier::BOLD)),
        "代码块里不做行内解析"
    );
    assert_eq!(text(&lines[3]), "  fn main() {}");
    // 闭合围栏之后回到普通文本。
    assert_eq!(text(&lines[4]), "收尾");
    assert_eq!(lines.len(), 5);
    // 围栏标记本身不上屏（info string 除外）。
    assert!(texts(&lines).iter().all(|line| !line.contains("```")));
}

#[test]
fn quotes_are_dim_with_a_leading_bar() {
    let theme = theme();
    let lines = parse("> 引用 **粗体**", &theme, 40);
    assert_eq!(text(&lines[0]), "▏ 引用 粗体");
    assert_eq!(lines[0].spans[0].content.as_ref(), "▏ ");
    assert_eq!(lines[0].spans[0].style.fg, Some(theme.gray_dim));
    // 引用的基础样式是 dim；行内标记在它之上加修饰符，不把 dim 丢掉。
    let bold = lines[0]
        .spans
        .iter()
        .find(|span| span.style.add_modifier.contains(Modifier::BOLD))
        .expect("引用里的粗体");
    assert_eq!(bold.content.as_ref(), "粗体");
    assert_eq!(bold.style.fg, Some(theme.gray_dim));
}

#[test]
fn rules_fill_the_content_width() {
    let theme = theme();
    let lines = parse("---\n***\n___\n--", &theme, 10);
    assert_eq!(text(&lines[0]), "─".repeat(10));
    assert_eq!(text(&lines[1]), "─".repeat(10));
    assert_eq!(lines[0].spans[0].style.fg, Some(theme.gray_dim));
    // 下划线不在子集里，两横也不是分隔线。
    assert_eq!(text(&lines[2]), "___");
    assert_eq!(text(&lines[3]), "--");
    // 宽度跟着正文列走（窄屏裁短）。
    assert_eq!(text(&parse("---", &theme, 3).remove(0)), "───");
}

#[test]
fn inline_markers_map_to_their_styles() {
    let theme = theme();
    let lines = parse("**粗** *斜* _斜_ `码` [文本](https://x.dev)", &theme, 40);
    assert_eq!(text(&lines[0]), "粗 斜 斜 码 文本 (https://x.dev)");
    let styles: Vec<Style> = lines[0].spans.iter().map(|span| span.style).collect();
    assert!(styles[0].add_modifier.contains(Modifier::BOLD));
    assert!(styles[2].add_modifier.contains(Modifier::ITALIC), "*斜*");
    assert!(styles[4].add_modifier.contains(Modifier::ITALIC), "_斜_");
    assert_eq!(styles[6].fg, Some(theme.accent_tool), "`码`");
    assert_eq!(styles[8].fg, Some(theme.accent_system), "链接文本");
    assert_eq!(styles[9].fg, Some(theme.gray_dim), "链接地址");
    assert_eq!(
        lines[0].spans[9].content.as_ref(),
        " (https://x.dev)",
        "地址以 dim 的 ` (url)` 附在文本后面"
    );
    // 反引号与方括号标记本身不上屏。
    assert!(!text(&lines[0]).contains('`'));
    assert!(!text(&lines[0]).contains('['));
}

#[test]
fn unknown_structures_stay_literal() {
    let theme = theme();
    let source = "| 表格 | 不在子集 |\n~~删除线~~\n   缩进代码块\n1) 圆括号序号";
    let lines = parse(source, &theme, 40);
    assert_eq!(texts(&lines), source.split('\n').collect::<Vec<_>>());
}

// ------------------------------------------------------------------ 管道表格

/// 带对齐、CJK 与截断需求的一张表：列宽 4 / 10，正文列 17。
const CJK_TABLE: &str = "| 名称 | 说明 |\n| --- | :--: |\n| cpu | 中央处理器 |\n| mem | 内存 |";

#[test]
fn pipe_tables_render_with_a_header_rule_and_alignment() {
    let theme = theme();
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
    // 表头：强调色 + BOLD；`│` 与 `─┼─`：dim；正文：正文色。
    let header = &lines[0].spans[0];
    assert!(header.style.add_modifier.contains(Modifier::BOLD));
    assert_eq!(header.style.fg, Some(theme.accent_system));
    assert_eq!(lines[0].spans[1].style.fg, Some(theme.gray_dim), "列分隔符");
    assert_eq!(lines[1].spans[0].style.fg, Some(theme.gray_dim), "规则线");
    assert_eq!(lines[2].spans[0].style.fg, Some(theme.text_primary));
    // 列宽按显示宽度算：第二列 10 列 = 5 个汉字的宽度，规则线正好铺满 4 + 3 + 10。
    assert_eq!(display_width(&text(&lines[1])), 17);
}

#[test]
fn pipe_table_columns_shrink_then_drop_on_narrow_screens() {
    let theme = theme();
    // 12 列：按自然宽度比例收窄到 3 + 3 + 6，超宽的单元格用 `…` 截断。
    assert_eq!(
        texts(&parse(CJK_TABLE, &theme, 12)),
        vec!["名… │  说明", "────┼───────", "cpu │ 中央…", "mem │  内存"]
    );
    // 8 列：每列 1 列都放不下，裁掉右侧的列并在末尾标 `…`。
    assert_eq!(
        texts(&parse(CJK_TABLE, &theme, 8)),
        vec!["…  │ 说…", "───┼────", "c… │ 中…", "m… │ 内…"]
    );
    // 硬约束：1..=40 列逐档扫，任何一行都不许超过宽度。
    for width in 1..=40 {
        for line in parse(CJK_TABLE, &theme, width) {
            assert!(
                display_width(&text(&line)) <= width,
                "width={width}：{line:?}"
            );
        }
    }
}

#[test]
fn table_rows_survive_the_assistant_block_wrapper() {
    let theme = theme();
    // 40 列：块标记列 2 + 正文列 38，表格按自然宽度 17 排，一行一条逻辑行。
    let output = Block::assistant(CJK_TABLE).output(&ctx(&theme, 40));
    assert_eq!(
        output
            .lines
            .iter()
            .map(|line| line.text())
            .collect::<Vec<_>>(),
        vec![
            "◆ 名称 │    说明",
            "  ─────┼───────────",
            "  cpu  │ 中央处理器",
            "  mem  │    内存",
        ]
    );
    // 表格行是硬换行：逻辑行之间不拼空格，复制文本逐行原样。
    assert!(output.lines.iter().all(|line| line.joiner.is_none()));
    assert_eq!(
        output.plain_text(),
        "名称 │    说明\n─────┼───────────\ncpu  │ 中央处理器\nmem  │    内存"
    );
    // 窄屏（block 宽 8 → 正文列 6）：表格自己收窄 / 裁列，块层不再折它，也不越界。
    let narrow = Block::assistant(CJK_TABLE).output(&ctx(&theme, 8));
    assert_eq!(
        narrow
            .lines
            .iter()
            .map(|line| line.text())
            .collect::<Vec<_>>(),
        vec!["◆ … │ …", "  ──┼───", "  … │ …", "  … │ …"]
    );
    for line in &narrow.lines {
        assert!(display_width(&line.text()) <= 8, "{:?}", line.text());
    }
}

#[test]
fn table_cells_keep_inline_styles_and_escaped_pipes() {
    let theme = theme();
    // 单元格里的行内标记：标记字符不上屏，列宽按去掉标记之后的文本算。
    let lines = parse("| a | b |\n| - | - |\n| `码` | **粗** |", &theme, 30);
    assert_eq!(texts(&lines), vec!["a  │ b", "───┼───", "码 │ 粗"]);
    assert_eq!(lines[2].spans[0].style.fg, Some(theme.accent_tool), "`码`");
    assert!(
        lines[2].spans[2]
            .style
            .add_modifier
            .contains(Modifier::BOLD),
        "**粗**"
    );
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
fn non_table_lines_around_a_table_are_untouched() {
    let theme = theme();
    let source = "# 标题\n| a | b |\n| --- | --- |\n| 1 | 2 |\n- 列表项\n普通正文";
    assert_eq!(
        texts(&parse(source, &theme, 30)),
        vec!["标题", "a │ b", "──┼──", "1 │ 2", "• 列表项", "普通正文"]
    );
    // 表头后面没有分隔行：整行逐字原样（「认不出来就原样落字」的口径不变）。
    assert_eq!(
        texts(&parse("| 表格 | 两列 |\n~~删除线~~", &theme, 40)),
        vec!["| 表格 | 两列 |", "~~删除线~~"]
    );
    // 分隔行列数与表头不一致：同样逐字原样，一行都不吞。
    assert_eq!(
        texts(&parse("| a | b |\n| --- |\n| 1 | 2 |", &theme, 30)),
        vec!["| a | b |", "| --- |", "| 1 | 2 |"]
    );
}

#[test]
fn streaming_half_a_table_never_panics_nor_swallows_text() {
    let theme = theme();
    // 表头刚写完、分隔行还没来：整段还是普通文本，一个字都不少，末尾照挂「还在长」的三角。
    let half = Block::streaming_assistant("| 名称 | 说明 |").output(&ctx(&theme, 40));
    assert_eq!(half.lines.len(), 1);
    assert_eq!(half.lines[0].text(), "◆ | 名称 | 说明 |▸");
    assert_eq!(half.lines[0].selection_text().unwrap(), "| 名称 | 说明 |");
    // 分隔行写了一半（`:` 后面还没有 `-`）：同样退回普通文本。
    let partial = Block::streaming_assistant("| 名称 | 说明 |\n| --- | :").output(&ctx(&theme, 40));
    assert_eq!(
        partial
            .lines
            .iter()
            .map(|line| line.text())
            .collect::<Vec<_>>(),
        vec!["◆ | 名称 | 说明 |", "  | --- | :▸"]
    );
    // 分隔行到齐：表格先画出来，三角挂在最后一行。
    let ready =
        Block::streaming_assistant("| 名称 | 说明 |\n| --- | :--:").output(&ctx(&theme, 40));
    assert_eq!(
        ready
            .lines
            .iter()
            .map(|line| line.text())
            .collect::<Vec<_>>(),
        vec!["◆ 名称 │ 说明", "  ─────┼─────▸"]
    );
}

// ------------------------------------------------------------------ 折行

#[test]
fn cjk_wrapping_counts_display_columns() {
    let theme = theme();
    // 6 个汉字 = 12 列：宽度 5 只能放下两个（4 列），放不下第三个（6 列）。
    let rows = wrap_lines(parse("中文中文中文", &theme, 5), 5, 0);
    assert_eq!(text(&rows[0]), "中文");
    assert_eq!(joined(&rows), "中文中文中文");
    for row in &rows {
        assert!(display_width(&text(row)) <= 5, "超宽：{row:?}");
    }
}

#[test]
fn wrapping_keeps_every_character_and_indents_continuations() {
    let theme = theme();
    let source = "这是一段很长的中文正文，需要折行显示，一个字都不能少。";
    let rows = wrap_lines(parse(source, &theme, 12), 12, 2);
    assert!(rows.len() > 1, "12 列放不下这句话");
    assert_eq!(joined(&rows), source, "折行不吞字符");
    for (index, row) in rows.iter().enumerate() {
        let line = text(row);
        if index == 0 {
            assert_eq!(display_width(&line), display_width(&line));
        } else {
            assert!(line.starts_with("  "), "续行缩进 2 格：{line:?}");
        }
        assert!(
            display_width(&line) <= 14,
            "续行缩进之外不超 12 列：{line:?}"
        );
    }
}

#[test]
fn long_words_are_hard_split() {
    let theme = theme();
    let url = "https://example.com/a/very/long/path/that/never/fits";
    let rows = wrap_lines(parse(url, &theme, 10), 10, 0);
    assert!(rows.len() >= 3, "10 列放不下这个 URL");
    assert_eq!(joined(&rows), url, "硬切也不丢字符");
    for row in &rows {
        assert!(display_width(&text(row)) <= 10, "硬切到 10 列：{row:?}");
    }
}

// ------------------------------------------------------------------ 助手块接线

#[test]
fn assistant_block_renders_the_markdown_subset() {
    let theme = theme();
    let block = Block::assistant("# 结论\n\n- 第一点\n- 第二点");
    let output = block.output(&ctx(&theme, 40));
    let lines: Vec<String> = output.lines.iter().map(|line| line.text()).collect();
    assert_eq!(lines, vec!["◆ 结论", "  ", "  • 第一点", "  • 第二点"]);
    // 标题那一行的正文 span 带着标题样式（块标记是 dim）。
    let heading = &output.lines[0].content.spans[1];
    assert!(heading.style.add_modifier.contains(Modifier::BOLD));
    assert_eq!(heading.style.fg, Some(theme.accent_system));
    // 行级元数据：块标记列不进复制文本，逻辑行之间不是软换行。
    assert_eq!(
        output.lines[0].selectable,
        Selectable::Spans(2..2 + display_width("结论") as u16)
    );
    assert_eq!(output.lines[0].selection_text().unwrap(), "结论");
    assert!(output.lines.iter().all(|line| line.joiner.is_none()));
}

#[test]
fn assistant_list_continuations_align_with_the_item_text() {
    let theme = theme();
    // 20 列 = 块标记列 `◆ `(2) + 正文列 18 列；正文列里先放 `• `(2)，余下 16 列 = 8 个汉字。
    let block = Block::assistant("- 一个很长的列表项内容需要折行才能放下");
    let output = block.output(&ctx(&theme, 20));
    assert_eq!(output.lines.len(), 3);
    let first = output.lines[0].text();
    let second = output.lines[1].text();
    assert_eq!(first, "◆ • 一个很长的列表项");
    // 续行缩进 4 格 = 块标记列 + 列表标记：正文与列表项正文同列（第 4 列）。
    assert_eq!(second, "    内容需要折行才能");
    assert_eq!(display_width(&first), 20, "首行铺满");
    assert_eq!(display_width(&second), 20, "续行也铺满，且不越界");
    // 续行是软换行：复制时拼回同一条逻辑行，且不带缩进与标记。
    assert_eq!(output.lines[1].joiner.as_deref(), Some(" "));
    assert_eq!(
        output.lines[1].selection_text().unwrap(),
        "内容需要折行才能"
    );
    assert_eq!(output.lines[2].text(), "    放下");
}

#[test]
fn streaming_text_that_stops_on_a_half_marker_still_renders() {
    let theme = theme();
    let block = Block::streaming_assistant("先看代码：\n```rust\nlet x = 1;\n");
    let output = block.output(&ctx(&theme, 40));
    let plain = output.plain_text();
    // 未闭合的围栏把它之后的内容全当代码块：语言名与代码都在，且还画着「还在长」的三角。
    assert!(plain.contains("rust"), "{plain}");
    assert!(plain.contains("let x = 1;"), "{plain}");
    assert!(!plain.contains("```"), "围栏标记不上屏：{plain}");
    assert!(output.lines.last().unwrap().text().ends_with('▸'));

    // 半个 `**`：标记与正文都还在，后面的字不会被当成粗体。
    let half = Block::streaming_assistant("正在**加粗");
    let output = half.output(&ctx(&theme, 40));
    assert!(output.plain_text().contains("**加粗"));
    assert_eq!(output.lines.len(), 1);
    assert!(output.lines[0].text().ends_with("加粗▸"));
}

#[test]
fn a_block_that_is_only_a_fence_marker_still_draws_its_marker_column() {
    let theme = theme();
    // 极端流式输入：正文只剩一个未闭合的围栏标记，没有任何可画的行。
    let output = Block::streaming_assistant("```").output(&ctx(&theme, 40));
    assert_eq!(output.lines.len(), 1);
    assert_eq!(output.lines[0].text(), "◆ ▸");
    assert_eq!(output.lines[0].selectable, Selectable::None);
}

#[test]
fn plain_prose_is_untouched_by_the_markdown_path() {
    let theme = theme();
    // 回归护栏：不含标记的正文与纯文本块逐字一致（用户块仍是纯文本，行尾填充不算）。
    let prose = "我先跑一下 git status，看有没有未提交改动。";
    let assistant = Block::assistant(prose).output(&ctx(&theme, 44));
    let user = Block::user(prose).output(&ctx(&theme, 44));
    assert_eq!(
        assistant.lines[0].text().trim_end(),
        user.lines[0].text().trim_end().replacen('❯', "◆", 1)
    );
    // 复制文本与纯文本块同口径：续行按软换行拼回空格，不产生新逻辑行。
    assert_eq!(
        assistant.plain_text(),
        jh_tui::width::wrap_words(prose, 42).join(" ")
    );
}

#[test]
fn no_color_theme_degrades_without_losing_text() {
    let theme = Theme::DARK.quantized(jh_tui::theme::ColorLevel::None);
    let block = Block::assistant("# 标题\n- 项 `码`");
    let output = block.output(&ctx(&theme, 40));
    let lines: Vec<String> = output.lines.iter().map(|line| line.text()).collect();
    assert_eq!(lines, vec!["◆ 标题", "  • 项 码"]);
    for line in &output.lines {
        for span in &line.content.spans {
            assert_eq!(span.style.fg, None, "无色档不许留下前景色");
            assert_ne!(span.style.fg, Some(Color::Reset));
        }
    }
}
