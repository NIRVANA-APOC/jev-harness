# 专题 04：Markdown 渲染美学

> 专题：Markdown 渲染美学｜调研日期：2026-09-22｜证据范围：`crates/codegen/xai-grok-markdown/`（全部 src，含 latex / mermaid / streaming / hyperlinks 子模块）、`crates/codegen/xai-grok-markdown-core/src/lib.rs`、`crates/codegen/xai-grok-pager-render/src/theme/md_style.rs` 与 `theme/tokyonight.rs`、`theme/terminal_default.rs`、`crates/codegen/xai-grok-pager/src/scrollback/blocks/markdown_content.rs` 与 `quote_bar.rs`、`scrollback/render.rs`、`scrollback/render_tests.rs`、`crates/codegen/xai-grok-pager-render/src/render/wrapping.rs`、`render/osc8.rs`、`crates/codegen/xai-ratatui-inline/src/terminal.rs`、`docs/user-guide/03-keyboard-shortcuts.md`

## 1. 整体架构：一个生态 parser + 自研渲染层

Grok Build 的 Markdown **不是自研 parser**，而是复用 `pulldown-cmark 0.13`（`crates/codegen/xai-grok-markdown/Cargo.toml:26` 区段的 `pulldown-cmark = { workspace = true }`；根 `Cargo.toml:230` 锁定 `0.13`）。但 parser 之外的一切——事件如何变成终端单元格、表格如何排版、语法标记如何隐藏——全部自研。

两个 crate 分工明确：

| crate | 角色 | 依赖重量 |
| --- | --- | --- |
| `xai-grok-markdown-core` | 无头分析：parser 配置的**唯一真源** + 元素计数 + 结构问题检测 | 只依赖 `pulldown-cmark`、`strum`（`xai-grok-markdown-core/Cargo.toml`） |
| `xai-grok-markdown` | 终端渲染：把事件流变成 `ratatui::text::Line<'static>` | 还有 `syntect`、`two-face`、`textwrap`、`ratatui`、`anstyle`、`url`、`linkify`、`unicode-width`、`unicode-segmentation` |

```
markdown 文本
   │
   ├─ LatexDelimiterNormalizer     归一化 \(…\) \[…\] \begin{equation} → $…$ / $$…$$   latex_delimiters.rs:1-7
   ▼
pulldown-cmark  Event 流（parser_options: GFM|STRIKETHROUGH|MATH|TASKLISTS|TABLES）
   │                             xai-grok-markdown-core/src/lib.rs:18-24
   │   offset_events(): 附带字节 range，并把单波浪线 ~x~ 降级为字面量    lib.rs:29-86
   ▼
MarkdownParser::on_event   parse.rs:553
   │
   ├─ highlights[]      字节范围 → anstyle::Style（可嵌套/可合并）      buffers.rs
   ├─ transforms[]      字节范围 → 替换串（• │ ─── 空串 …）           parse.rs 各处
   ├─ replaces[]        代码块 syntect 高亮结果
   ├─ table_replaces[]  整表预排版结果（含 styled Lines）
   └─ mermaid_replaces[] 整图预排版结果
   ▼
render_ratatui()          render.rs:530
   │  按事件位置游标推进；pretty 模式下 all_hidden 的段直接跳过  render.rs:640-675
   ▼
Vec<Line<'static>> + line_source_map + hyperlinks + code_blocks + tables    output.rs:50-67
   ▼
MarkdownContent::output()  markdown_content.rs:369   词折行 + 引用条裁剪 + 背景传递
   ▼
BlockOutput → scrollback 渲染 → xai-ratatui-inline 帧 diff → 终端
```

关键点是**中间结构不是 AST，而是"并行缓冲 + 字节范围"**。`MarkdownParser` 在遍历事件时把样式、替换、表格分别塞进四个 `Vec`，每个元素带源字节 `Range<usize>`；`render_ratatui` 再按位置排序并把它们压平成 `Line`/`Span`（`render.rs:195-260` 的 `build_render_events`）。这样做的好处是渲染层不需要理解 Markdown 语义，只需要"在某段字节上叠样式"。

## 2. 支持的元素与终端呈现

parser 配置（`xai-grok-markdown-core/src/lib.rs:18-24`）：

```rust
Options::ENABLE_GFM
    | Options::ENABLE_STRIKETHROUGH
    | Options::ENABLE_MATH
    | Options::ENABLE_TASKLISTS
    | Options::ENABLE_TABLES
```

在此基础上有一处**刻意的语义偏离**：`ENABLE_STRIKETHROUGH` 让 pulldown 把 `~x~` 也当删除线，Grok 通过 `DoubleTildeOnlyStrike` 过滤器（`lib.rs:38-86`）把单波浪线对降级成字面量 `~`，动机写在 `lib.rs:8`——LLM 常输出 `~**10%**`（"约 10%"），被误判成删除线会造成严重误读。测试 `bill_single_tilde_percent_is_not_strikethrough`（`lib.rs:488-495`）就是这条规则的真实来源。

### 2.1 块级元素

| 元素 | pretty（默认）| raw（`r` 键切换）| 样式来源 |
| --- | --- | --- | --- |
| H1–H6 | `#` 与紧随空格**隐藏**；标题文本按级别取色 + 该级修饰符 | 显示 `## ` 前缀，用 `heading_outer`（同色 + DIM + HIDDEN）| `parse.rs:885-901`、`md_style.rs:120-122` |
| 段落 | 纯文本；软换行折叠成空格 | 同左（`force: true` 的 transform 在 raw 模式也生效，`render.rs:160-162`）| `parse.rs:735-755` |
| 围栏代码块 | 上下 ``` 行隐藏，但**在开栏处补一个空行**；代码体带 syntect 高亮 + 行级背景色 | 显示 ``` 与 info string | `render.rs:652-675`、`render.rs:739/818/836` |
| 4 空格缩进代码块 | 同上（无 info string）| 同左 | `parse.rs:312` |
| 引用块 | 每层 `>` → `│`（U+2502），默认可见（`blockquote_outer` 不 hidden），续行自动重复前缀 | 显示 `>` | `parse.rs:902-943`、`wrapping.rs:348-355` |
| 无序列表 | `- ` / `* ` → `• ` | 显示 `- ` / `* ` | `parse.rs:1037-1043` |
| 有序列表 | **保持源文本编号**（`1.`、`2.`、`1)` 等原样保留），仅套 `list_item` 颜色 | 同左 | `parse.rs:1021-1036` |
| 任务列表 | `[x]` / `[ ]` **原样保留**，仅套 `md_task_checked` / `md_task_unchecked`+DIM | 同左 | `parse.rs:859-866`、`md_style.rs:134-135` |
| 分隔线 | 整行标记替换为**固定 3 个 `─`**（不是按宽度拉满）| 同左（非 force，但 pretty 才应用；raw 显示 `---`）| `parse.rs:835-857` |
| 表格 | 整表替换为预排版盒线表（见 §4）| 显示源码 `\| a \| b \|` | `parse.rs:1048-1070`、`1298-1321` |
| HTML 块 | 当普通文本渲染，**不做代码高亮** | 同左 | `parse.rs:762-766`、`parse.rs:1001-1006` |
| 行内 HTML | `<br>`（含 `<br/>`、`<BR>`）→ 换行；其它标签按 HTML 语法高亮 | 同左 | `parse.rs:211-222`、`parse.rs:767-790` |
| 显示数学 `$$…$$` | 转为 Unicode 块，每行前置 2 空格，可多行 | 显示 TeX 源 | `parse.rs:1535-1585` |

几个具体到字节的行为值得记住：

**列表标记只认 `- ` 和 `* `**。`parse.rs:1019-1027` 判定 `marker_len`：`"- "`/`"* "` 为 2，否则找 `". "`/`") "` 且要求前面全是数字。**`+ item` 既不套色也不替换**，原样显示 `+ item`。

**引用块的 `>` 是被"逐层挑选"后替换的**，不是无脑全替换：`parse.rs:915` 用 `skip = bq_depth - 1` 跳过外层已处理的 `>`，只把本层那一个变成 `│`。嵌套引用因此渲染成 `│ │ text`（测试 `test_nested_blockquote_with_list`，`render.rs:1245-1261`）。

**引用块的续行有前缀，列表的续行没有**：

```
源：
> Foo
>
> > Bar
> >
> > - Baz

源：
- 一个很长的列表项文字在窄终端里会被折成多行……

pretty 渲染（引用块，续行重复竖线）        pretty 渲染（列表，续行顶到列 0）
┌──────────────────────────────────┐      ┌──────────────────────────────────┐
│ Foo                              │      │ • 一个很长的列表项文字在窄终端里  │
│                                  │      │ 会被折成多行……                    │
│ │ Bar                            │      └──────────────────────────────────┘
│ │                                │
│ │ • Baz                          │
└──────────────────────────────────┘
```

左图逐字来自 `render.rs:1255-1260` 的断言（`["│ Foo", "│", "│ │ Bar", "│ │", "│ │ • Baz"]`）。右图的行为由 `render_tests.rs:3248-3250` 的注释确认："Top-level list items render as `• …` with no quote-bar indent on continuation rows"——这条不是疏漏而是被测试锁定的设计。

### 2.2 行内元素

| 元素 | pretty 呈现 | 样式键 |
| --- | --- | --- |
| `**粗体**` | 文本加粗，`**` 隐藏 | `strong_inner` / `strong_outer` |
| `*斜体*` | 文本斜体，`*` 隐藏 | `emphasis_inner` / `emphasis_outer` |
| `~~删除线~~` | 文本带 CROSSED_OUT，`~~` 隐藏 | `strikethrough_inner` / `strikethrough_outer` |
| `` `行内代码` `` | 内容套 `md_code` 色 + 粗体；**反引号本身也着同色**（DIM+HIDDEN 隐藏） | `parse.rs:1437-1467`、`md_style.rs:129-130` |
| `[文本](url)` | 重写为 `文本 (url)`（见 §5） | `link_text` / `link_url` / `link_title` / `link_outer` |
| `![alt](url)` | 同链接，`!` 与 `[` 一并去掉 → `alt (url)` | `parse.rs:1089-1094`、`render.rs:1227-1240` |
| `$E=mc^2$` | 转 Unicode → `E=mc²`，斜体；转换失败则退化成行内代码呈现 | `parse.rs:699-734`、`md_style.rs:148` |
| HTML 实体 `&lt;` `&amp;` | 解码替换（**拒绝含控制字符的实体**，防转义注入）| `parse.rs:139-149`、`parse.rs:1472-1530` |
| 脚注引用 | 仅套 `link_outer` 色；**脚注定义块的渲染未找到证据** | `parse.rs:832-834` |

`md_style.rs:148` 的 `math: fg(theme.md_text).italic()` 说明行内数学的美学选择是"正文色 + 斜体"，和行内代码（`md_code` 色 + 粗体）刻意区分开。

### 2.3 未支持 / 未找到证据

- **脚注**：`parser_options()` 未显式列出 `ENABLE_FOOTNOTES`；`Event::FootnoteReference` 有处理分支但全仓库（含文档、测试）搜不到第二处 footnote 相关代码。**脚注定义如何渲染：未找到证据。**
- **删除线之外的 GFM 扩展**（如自动目录、emoji 短代码、定义列表、上标下标）：未在 `parser_options()` 中启用，也未在代码中找到处理分支。
- **图片的图文渲染**：Markdown 图片只降级成 `alt (url)` 文本链接；Grok 的图像渲染走的是另一条独立路径（`scrollback/blocks/` 下的 media 相关 block），与本渲染器无关。

## 3. 样式模型：`md_style.rs` 如何在主题与渲染器之间搭桥

### 3.1 `MarkdownStyle`：27 个字段，`_inner` / `_outer` 二元结构

`style.rs:124-156` 定义了 `MarkdownStyle`，全部字段是 `anstyle::Style`（不是 ratatui 的）。命名规律来自 `style.rs:123` 的注释：

> The `_inner` variants are applied to the content, while `_outer` variants are applied to the syntax markers (which are hidden in pretty mode).

也就是说 **`_outer` 的用途就是"给要隐藏的语法标记上色，供 raw 模式显示"**。凡是 pretty 模式下要消失的标记，都有对应的 `_outer`：标题 `##`、`**`、`*`、`~~`、反引号、链接括号、代码围栏。而 `blockquote_outer` 是唯一"不 hidden"的 `_outer`——它染的是替换后的 `│`，那个字符在 pretty 模式必须可见。

### 3.2 从主题取色：`build_style()`

`md_style.rs:96-150` 是唯一的映射函数，每次调用都重新从 `Theme::current()` 现取：

```rust
// crates/codegen/xai-grok-pager-render/src/theme/md_style.rs:120-149（节选）
MarkdownStyle {
    heading_inner: heading_inner_styles(heading_colors, heading_mods),   // [Style; 6]
    heading_outer: heading_outer_styles(heading_colors),                 // 丢弃 mod，统一 DIM+HIDDEN
    strong_inner: fg(theme.md_text).bold(),
    strong_outer: Style::new().dimmed().hidden(),
    inline_code_inner: fg(theme.md_code).bold(),
    blockquote_outer: fg(theme.md_muted).dimmed(),
    task_checked: fg(theme.md_task_checked),
    task_unchecked: fg(theme.md_task_unchecked).dimmed(),
    list_item: fg(theme.md_muted),
    rule: fg(theme.md_muted),
    link_text: fg(theme.link_fg).underline(),
    code_background: bg(theme.md_code_bg),
    table_outer: fg(theme.md_heading_h2).hidden(),
    text: fg(theme.md_text),
    math: fg(theme.md_text).italic(),
    ...
}
```

设计上的三个要点：

1. **标题是 6 元数组 + 6 个独立修饰符**。`heading_inner_styles`（`md_style.rs:66-87`）把该级颜色与 `md_heading_hN_mod` 的 effects 按位或合并；`heading_outer_styles`（`md_style.rs:90-92`）则**只用颜色、丢弃修饰符**，统一 `.dimmed().hidden()`。
2. **`_outer` 大多是常量**（`Style::new().dimmed().hidden()`），不依赖主题。主题只决定"内容长什么样"，"标记怎么淡出"由渲染器决定。
3. **一份颜色可能被多个元素共用**，映射关系写在主题字段注释里（`tokyonight.rs:125-143`）：`md_heading_h2` 同时用于 H2 和表格外框色，`md_heading_h3` 同时用于 H3 和代码语言标签，`md_heading_h5` 同时用于 H5 和链接标题，`md_muted` 同时用于引用块/列表标记/分隔线/链接 URL。

### 3.3 主题侧：12 色位 + 6 修饰符

主题结构体为 Markdown 单独留了接口（`theme/tokyonight.rs:123-143`）：

```rust
// Markdown rendering colors, used by md_style.rs for headings, code blocks, inline code, links, etc
// These default to the corresponding top-level theme colors but can be overridden per-theme to customise markdown appearance independently
pub md_heading_h1: Color,        // H1 headings
pub md_heading_h1_mod: Modifier, // H1 extra effects
...
pub md_code: Color,              // Inline code, code block delimiters
pub md_task_checked: Color,      // Task checked
pub md_task_unchecked: Color,    // Task unchecked
pub md_muted: Color,             // Blockquotes, list items, rules, links
pub md_code_bg: Color,           // Code block background
pub md_text: Color,              // Default body text (plain paragraphs, strong, emphasis)
pub link_fg: Color,              // Clickable link text color
```

对照具体主题能看到这个模型的可调性：

| 字段 | TokyoNight Storm | GrokNight | terminal-default |
| --- | --- | --- | --- |
| `md_heading_h1` | `TEAL` + BOLD（`tokyonight.rs:212-213`）| `TEAL` + BOLD（`groknight.rs:117-118`）| `Color::Reset` + **BOLD∪UNDERLINED**（`terminal_default.rs:91-92`）|
| `md_heading_h4..h6` | RED/GREEN/MAGENTA（`tokyonight.rs:218-223`）| — | 全部 `Reset`+BOLD，H6 用 MUTED（`terminal_default.rs:97-101`）|
| `md_code` | `GREEN1` | `BLUE1` | `Color::Cyan` |
| `md_code_bg` | `BG_HIGHLIGHT` | `rgb(28,28,28)` | `Color::Reset`（无背景）|
| `link_fg` | `BLUE` `#7aa2f7` | `rgb(122,166,218)` | `Color::Blue` |

`terminal_default` 主题把 H1 设为 `BOLD | UNDERLINED`，是"没有颜色预算时靠修饰符制造层级"的明确示范。量化（quantize）在 `theme/mod.rs:269-286` 统一处理：所有 `md_*` 色位在降级时经 `q()` 变换，`md_code_bg` 走特殊的 `push_away` 分支（`theme/mod.rs:401`）以保持与背景的对比。

### 3.4 anstyle → ratatui 的双向桥

整条链上有两次颜色表示转换，值得单独记住：

- **anstyle → ratatui**（`parse.rs:34-96`、`render.rs:31-92`）：`Color::Reset` 映射为"No fg"（`md_style.rs:8-11` 明确注释 "*`None` for `Reset`: anstyle has no terminal-default, and unset renders as that default*"），这个映射有回归测试 `reset_maps_to_no_color`（`md_style.rs:158-163`），注释说明历史上错误地把 Reset 映成 `AnsiColor::White`，导致浅色终端上渲染出灰糊糊的文本并破坏 `NO_COLOR`。
- **Gray/DarkGray 命名不一致**：ratatui 的 `Gray` → anstyle `White`，`DarkGray` → `BrightBlack`（`md_style.rs:21-22` 与对应测试 `named_colors_map_concretely`，`md_style.rs:166-180`）。

### 3.5 嵌套样式合并规则

`merge_styles()`（`style.rs:229-261`）是嵌套格式（如 `**粗体中的 *斜体***`）的核心。规则有两条显式约定：

- **BOLD 与 DIMMED 互斥**（`style.rs:244-249`）：后出现的那个会移除前一个。因为多数终端把 DIM 实现为"降低亮度"，与粗体同开会互相抵消，视觉上不可预测。
- **HIDDEN 会被剥离**（`style.rs:260`）：`out.effects(out.get_effects().remove(Effects::HIDDEN))`——HIDDEN 在合并阶段只充当"跳过这段"的标记，绝不进入最终渲染样式。

而"这段是否该跳过"由 `all_hidden()`（`style.rs:210-225`）判定：**所有**活跃样式都含 HIDDEN 才跳过。这保证 `**粗体**` 里的 `**` 会因为外层 `strong_inner` 不含 HIDDEN 而保留。

## 4. 表格布局

表格完全自研：pulldown 只给出 `alignments` 和单元格事件，**排版由 `format_table()` 完成**（`parse.rs:1588-1864`）。

### 4.1 边框与内边距

```rust
// parse.rs:1588-1593
let borders = TableBorders::BOX;
let padding = 1;
let border_style: ratatui::style::Style = self.ms.rule.style_into().dim();
```

- 边框字符集**硬编码为 `TableBorders::BOX`**，不是主题可选。`style.rs:11-38` 另定义了 `ASCII`（`- | +`）与 `DOUBLE`（`═ ║ ╔ …`）两套常量，但 `format_table` 只用 `BOX`——**ASCII/DOUBLE 在表渲染路径上未被使用**（`bin/md_table_test.rs` 的 playground 也不用）。
- 边框颜色 = 主题 `rule` 字段（即 `md_muted`）**再叠一层 DIM**，所以表格框线比正文明显更暗。
- 内边距固定 1 列：每个单元格左右各一个空格。

### 4.2 列宽算法（两阶段）

**阶段一 · 自然宽度**（`parse.rs:1609-1622`）：逐单元格取 `plain_text()` 按 `\n` 分行后的**最大显示宽度**，按列取 max。用 `unicode_display_width`（`buffers.rs`），即 CJK 宽字符按 2 算。

**阶段二 · 收缩**（仅在 `max_table_width = Some(w)` 时，`parse.rs:1626-1727`）：

```rust
let overhead = num_cols * (2 * padding + 1) + 1;   // parse.rs:1627
let content_budget = max_width.saturating_sub(overhead);
```

对 2 列表格：`overhead = 2*3+1 = 7`，正好是 4 个 `│` + 4 个空格 + 2 个 `┬` 少算 1 的修正。这个公式在 playground 里被独立复现为"最小渲染宽度 = `4k + 1`"（`bin/md_table_test.rs:40-46`），可交叉验证。

收缩分三档，**先算出两个下限**：

| 下限 | 定义 | 代码 |
| --- | --- | --- |
| `min_col_widths` | 该列**最长不可断词**的显示宽度（用 `cell_word_separator` 切词）| `parse.rs:1634-1649` |
| `hard_floors` | 该列**最宽单个 grapheme** 的宽度（`max(1)`），即"再窄就会丢内容"的极限 | `parse.rs:1636-1659` |

然后按 `min_total` 与 `hard_total` 相对预算的位置选基准与目标（`parse.rs:1666-1674`）：

```
if min_total > budget && hard_total <= budget:
      基准 = hard_floors，目标 = min_col_widths     # 词宽都放不下，退到 grapheme 下限
else:
      基准 = min_col_widths，目标 = 自然宽度         # 常态：从词宽往自然宽长
```

**注意这里没有等分**：`extra_budget = budget - base_total` 先按各列"缺口占比"做浮点比例分配并 `floor()`（`parse.rs:1689-1693`），再把取整丢掉的**逐列补偿给缺口最大的列**（`parse.rs:1696-1722`，按 `unmet` 降序排，每次 +1 且不超过该轮目标宽）。代码注释直说原因："*Hand out any remaining columns (from floor rounding) to columns with the most unmet want, one at a time*"。

若 `min_total > budget && hard_total > budget`（连 grapheme 下限都放不下），则**保持 `min_col_widths` 不动**，注释（`parse.rs:1668`）明确："*When even the grapheme floors cannot fit, keep the word minimums: downstream clipping remains the safety net*"。

### 4.3 超宽单元格的折行

列宽定稿后，内容用 `wrap_cell_text_joins()`（`parse.rs:1869-1911`）折行：

```rust
let opts = textwrap::Options::new(width)
    .wrap_algorithm(textwrap::WrapAlgorithm::FirstFit)
    .word_separator(textwrap::WordSeparator::Custom(cell_word_separator))
    .break_words(false);
```

关键点是**自定义词分隔符 `cell_word_separator`**（`parse.rs:226-390`），三遍扫描：

1. 找断点：空白处必断；标点后跟字母必断（`foo/bar`）；"数字-标点-数字"会断，**但 `,` 和 `.` 例外**，所以 `$145,000`、`3.14` 保持完整（`parse.rs:245-246`）。
2. **URL 保护**：用 `url::Url::parse` 逐个 token 验证，命中的范围内的断点全部丢弃（`parse.rs:281-303`）。注释给了理由：URL 内断开会让终端的 Cmd+Click 超链接识别失效。
3. 标点归属：在"贴左/贴右"之间选让左右段最大值更小的那一侧，平手贴左（`parse.rs:305-369`）。

`break_words(false)` 意味着 textwrap 不会拆词；若某词硬超列宽，则回落到**按 grapheme 边界手工硬切**（`parse.rs:1885-1901`），注释解释了为什么不能用 textwrap 的应急拆行："*textwrap's char-based emergency split can tear grapheme clusters*"。

行内格式跨折行时会**被精确保留**：`format_styled_content_lines`（`parse.rs:1917-2097`）先算出本可视行在完整单元格明文中的字节区间，再让该单元格的每个 `span` 与这个区间求交，逐段套原样式（`parse.rs:2017-2071`）。同一列维护单调递增的 `source_cursors`（`parse.rs:1950-1952`），防止重复子串被错配到更早的位置。

### 4.4 行结构（每行都有分隔线）

```
┌─── 顶部边框（c_tl, t_t, c_tr）          parse.rs:1750-1758
│ 表头单元格（is_header → 强制 bold）      parse.rs:1763-1784, 2039-2041
├─── 表头分隔线（t_l, x, t_r）             parse.rs:1787-1797
│ 表体第 1 行                              parse.rs:1801-1824
├─── 表体行间分隔线（i < rows.len()-1）     parse.rs:1827-1839
│ 表体第 2 行
└─── 底部边框（c_bl, t_b, c_br）          parse.rs:1844-1854
```

**表体每两行之间都插一条 `├─┼─┤`**，这不是可选装饰，是 `format_table` 的固定输出。表头行强制加粗，与正文的 `strong` 无关。

### 4.5 ASCII 示例

**自然宽度**（未设 `max_table_width`）。源：

```markdown
| Status | Note |
|---|---|
| OK | fast |
```

按 `parse.rs:1609-1622` 得列宽 `[6, 4]`，输出：

```
┌────────┬──────┐
│ Status │ Note │
├────────┼──────┤
│ OK     │ fast │
└────────┴──────┘
```

**超宽收缩**。源为 `markdown_content.rs:518-520` 测试用的 6 列表：

```markdown
| Alpha | Bravo | Ident | DeptName | RoleName | Amount |
|---|---|---|---|---|---|
| LongalphaToken | TokenTwo | ID-AA1001 | EngineeringOps | ManagerRole | $145,000 |
```

在宽度 30 下，按 §4.2 的算法手工演绎：

```
自然列宽        [5, 5, 5, 8, 8, 6]  sum=37
overhead        6*3+1 = 19  →  content_budget = 30-19 = 11
min_col_widths  [14, 8, 9, 14, 11, 8]   (LongalphaToken / TokenTwo / ID-AA1001 / EngineeringOps / ManagerRole / $145,000)
hard_floors     [1, 1, 1, 1, 1, 1]       ← grapheme 宽度
判定            37 > 11 且 64 > 11 但 hard_total=6 ≤ 11  →  基准 = hard_floors，目标 = min_col_widths
比例分配        extra_budget = 11-6 = 5，缺口 [13,7,8,13,10,7]（sum 58）
                floor 份额 → [1,0,0,1,0,0]，宽度 [2,1,1,2,1,1] sum=8
余量补偿        缺口排序 col0=12, col3=12, col4=10, col2=8 … 各 +1 → 最终 [3,1,1,3,2,1] sum=11
```

最终每行宽度 = `11 + 19 = 30`，渲染形状：

```
┌───┬─┬─┬───┬──┬─┐
│ A │B│I│ De│Ro│A│
│ l │ │ │ pt│le│m│
│ p │T│I│ Na│Na│o│
│ h │o│D│ me│me│u│
│ a │k│-│   │  │n│
│   │e│A│   │  │t│
│ L │n│A│ En│Ma│$│
│ o │T│1│ gi│na│1│
│ n │w│0│ ne│ge│4│
│ g │o│0│ er│rR│5│
│ a │ │1│ in│ol│,│
│ l │ │ │ gO│e │0│
│ p │ │ │ ps│  │0│
│ h │ │ │   │  │0│
│ a │ │ │   │  │ │
│ T │ │ │   │  │ │
│ o │ │ │   │  │ │
│ k │ │ │   │  │ │
│ e │ │ │   │  │ │
│ n │ │ │   │  │ │
│ [ │ │ │   │  │ │
│ … │ │ │   │  │ │
└───┴─┴─┴───┴──┴─┘
```

> 上表是**按算法手工推算**的形状（不是测试断言的字符串）。`markdown_content.rs:515-543` 的 `test_six_col_table_output_30_keeps_right_border` 只断言两件事：每行显示宽度**恰好等于 30**，且每行末尾字符是 `│`/`┐`/`┘`/`┤` 之一。测试注释解释了为什么要锁右边框："*A table that overflows the budget gets hard-clipped by the wrap layer, which eats the right `│`*"。

### 4.6 与折行层的关键契约：**表格行绝不折行**

表格在渲染器里已经自己折好并补齐到内容宽度，因此下游折行层必须**跳过表格行**，否则列对齐会被破坏：

```rust
// crates/codegen/xai-grok-pager-render/src/render/wrapping.rs:284-285
/// Never wrap tables: wrapping destroys column alignment. ...
fn is_table_line(line: &Line<'_>) -> bool {
```

判定规则（`wrapping.rs:285-313`）：首字符落在 U+2500–U+257F 且不是 `│`/`┃` → 边框行；首字符是 `│` 时，若"前缀区"（只含 `│` 和空格）之后还出现 `│` → 表格内容行；首字符 `|` → ASCII 表格。命中后走 `fit_line_to_width`（`wrapping.rs:340-344`）补齐宽度，注释说明补齐的必要性："*terminal clipping desyncs on wider-than-measured glyphs and strands a ghost cell*"。

表格的**背景色不会被行级背景覆盖**，但单元格背景随整行传递（`markdown_content.rs:395-399`）。另外表格行是非折行的，所以"一个渲染行 = 一个 block 行"，选择层据此实现**按单元格选择**（`scrollback/table_geometry.rs:1-2`：`Box-drawing table grid detection so selection inside rendered tables operates on cells`）。

### 4.7 对齐

来自 GFM 的 `alignments`，逐列独立处理（`parse.rs:1729-1737`、`1973-1986`）：

| `pulldown_cmark::Alignment` | 左填充 | 右填充 |
| --- | --- | --- |
| `Left` 或 `None` | 0 | 全部余量 |
| `Right` | 全部余量 | 0 |
| `Center` | `total/2` | `total - total/2` |

`total_padding = col_width - cell_line_width`（`parse.rs:1971`），再在两侧各加固定 1 列 padding。**表头单元格也参与对齐**，但额外强制加粗。

## 5. 链接呈现与点击

### 5.1 pretty 模式把 `[text](url)` 重写成 `text (url)`

`parse.rs:1176-1185` 推入两条 transform：

```rust
// 去掉 '['
Transform { range: range.start+open_bracket .. +1,  to: "".to_string(),      force: false }
// '](' → ' ('
Transform { range: bracket_abs .. bracket_abs+2,   to: " (".to_string(),    force: false }
```

于是 `[text](url)` 显示为 `text (url)`——URL **直接可见**，不需要 hover 才能看到目标。`render.rs:1207-1223` 的测试断言输出包含 `link (https://example.com)`。`force: false` 意味着 raw 模式保留原始括号。

可视文本与 URL 分色：`link_text = fg(link_fg).underline()`、`link_url = fg(md_muted)`（`md_style.rs:139-140`）。标题（`[a](u "T")`）用第三种颜色 `link_title = fg(md_heading_h5)`（`md_style.rs:141`）。

链接文本的样式有个微妙规则：**链接内部的粗体/斜体不得覆盖链接色**。`parse.rs:564-572` 会把链接祖先样式的 **fg 剥掉**，只让 effects（bold/italic）通过，并解释原因："*merge_styles is last-wins on fg, so keeping the fg would clobber the link color*"。测试 `formatted_link_keeps_link_fg_and_gains_effects`（`hyperlinks.rs:658`）锁定该行为。

### 5.2 三层坐标系

`hyperlinks.rs:1-10` 的模块文档把整条链说得很清楚：

```
1. Source bytes     源字节偏移           LinkTarget::source_range
2. Transformed bytes apply_transforms 之后的字节（pretty 删了 '['、重写了 ']('）
3. Display cells    (line_index, display_column)      HyperlinkTarget
```

坐标从 1 投影到 2 由 `source_to_chunk_offset()` 完成（`hyperlinks.rs:34-70`），含一条**"跨骑策略"**：若某 transform 的源区间包含目标位置，位置就钳到替换串起点（`hyperlinks.rs:38`），并有 `debug_assert!(false, ...)` 兜底（`hyperlinks.rs:56-61`）。

公共输出是 `HyperlinkTarget`（`output.rs:11-21`）：

```rust
pub struct HyperlinkTarget {
    pub line_index: usize,                  // 渲染行号（pre-wrap）
    pub column_range: Range<usize>,          // 该行上的显示列区间
    pub url: String,
    pub id: u32,                             // 同一逻辑链接跨折行分组用
}
```

`id` 的存在理由写在字段注释里："*Stable identifier for grouping link fragments that belong to the same logical link (e.g., a link whose text wraps across lines)*"。表格单元格里的链接走**独立路径**（`TableHyperlink`，`parse.rs:1097-1103`），因为"*The paragraph link path (LinkTarget + chunk_link_offsets) can't project links onto rendered table cells*"。

裸 URL 由后置扫描补齐：`url_scan::detect_plain_urls`（`lib.rs:122-127`）在每次渲染后扫描输出行，找出 pretty 模式链接的 `(url)` 后缀和正文里的裸 URL。扫描器发的 id 从 `1 << 31` 起（`render/osc8.rs:100` `SCANNER_ID_BASE`），与 Markdown 自己的小 id 空间隔开，避免"扫描到的 URL"和"Markdown 链接"因同 id 被终端当成同一条链接。

### 5.3 折叠到屏幕坐标，再发 OSC 8

`map_hyperlinks_to_overlay()`（`pager/src/scrollback/render.rs:834`）把 pre-wrap 的 `HyperlinkTarget` 映射到屏幕坐标的 `OverlayLink`，需要处理三件事：折行分段（`render.rs:846-877` 建立 `pre_wrap_segments`）、joiner 占位列、以及续行 `indent_width` 的扣减（`render.rs:870-871`、`render.rs:927-931`）。**不安全的 scheme 在这一步被丢弃**（`render.rs:885-886`，`javascript:`、`data:` 等），因为 OSC 8 的 URL 是直接写进终端的，不经过 `link_opener` 的 scheme 过滤。

真正的转义序列由帧 diff 层发出（`xai-ratatui-inline/src/terminal.rs:55-72`）：

```rust
fn write_osc8_open<W: Write>(w: &mut W, url: &str, id: Option<u32>) -> io::Result<()> {
    // 注释：Control characters are stripped from `url` to prevent premature sequence termination or escape injection.
    match id {
        Some(id) => write!(w, "\x1b]8;id={id};{sanitized}\x07"),
        None     => write!(w, "\x1b]8;;{sanitized}\x07"),
    }
}
fn write_osc8_close<W: Write>(w: &mut W) -> io::Result<()> { w.write_all(b"\x1b]8;;\x07") }
```

三个实现细节：

- **URL 先做控制字符剥离**（`terminal.rs:58-62`），否则恶意 Markdown 可以靠 `\x1b` 提前终止序列注入转义。
- **`id=` 是重铸的，不是 Markdown 的 id**。`next_osc8_id()`（`terminal.rs:74-95`）注释解释：Markdown 的 id 每文档从 0 重数，而 OSC 8 的 `id=` 是终端全局的，全屏应用必须重铸，否则不同消息里的同号链接会被 Windows Terminal 的 hover/Ctrl+Click 合并成一条。相邻且 `(source_id, url)` 相同的 span 复用同一个重铸 id——这正是"折行片段被终端当作一条链接"的实现方式。
- **链接参与帧 diff**。`flush_with_links()`（`terminal.rs:385-420`）在没有链接时直接退化为普通 `flush()`；有链接时用 `diff_large_with_links`，**样式相同但链接不同的单元格也会被判为变化**（`terminal.rs:1070`）。发射时按"连续相同链接"合并成 run，一个 run 一个开-闭序列（`emit_frame_with_links`，`terminal.rs:1114-1164`），并保证即使 cell draw 出错也总会 `write_osc8_close`（`terminal.rs:1152-1155`）。

终端能力分级在 `pager-render/src/terminal/hyperlinks.rs`：`Osc8Support` 有 `Native` / `HostileParser` / `Unsupported` / `Unknown` 四档，Apple Terminal 归为 `HostileParser`、Warp 归为 `Unsupported`、JetBrains 与未知终端归为 `Unknown`（`hyperlinks.rs:59-175`），`TerminalContext::hyperlink_skip_reason()`（`terminal/mod.rs:437-460`）据此给出跳过原因。**点击行为本身由终端负责**（Cmd+Click / Ctrl+Click），Grok 侧只提供 OSC 8 与安全 URL；应用自己打开 URL 的路径是 `Action::OpenUrl`（`pager/src/app/actions.rs:69`）。

## 6. 流式（半截）Markdown

### 6.1 checkpoint 冻结：只有"顶层块边界"能冻结

`checkpoint.rs:1-14` 的文档给出核心约束：

> Checkpoints are only created at **top-level** (depth=0) block boundaries. Blocks nested inside lists, blockquotes, or tables cannot be checkpoints because the outer container might continue.

深度由 `depth` 计数器维护（`parse.rs:872-877`、`1330-1333`：进入/离开 `BlockQuote | List | Item | Table` 各 ±1）。在 `on_end` 中只有 `depth == 0` 才记录 checkpoint（`parse.rs:1341-1355`），且还需满足 `has_blank_line_after(...)` **或**代码块已正确闭合（`parse.rs:1354-1359`）。checkpoint 类型共 8 种：`Heading`、`Paragraph`、`CodeBlock`、`BlockQuote`、`List`、`ThematicBreak`、`Table`、`HtmlBlock`（`checkpoint.rs:37-58`）。

渲染器据此维护冻结前缀（`streaming.rs:105-142`）。每次 `rerender_tail()`（`streaming.rs:257-369`）的流程是：

```
1. 把 output 截断回冻结边界（行 / line_source_map / hyperlinks / tables / code_blocks）  streaming.rs:259-272
2. 只渲染 tail = source[frozen.source_bytes ..]，并修正tail起始处可能多出的 '\n'   streaming.rs:277-306
3. tail 结果整体偏移 fosen 行数/字节数后追加                               streaming.rs:309-339
4. 对 tail 做裸 URL 扫描                                                 streaming.rs:341-351
5. 若 tail 渲染返回 checkpoint，推进冻结边界                              streaming.rs:362-368
```

复杂度从 O(N²) 降到约 O(N)（`streaming.rs:11`）。

### 6.2 不完整语法如何处理

| 半截语法 | 处理方式 | 证据 |
| --- | --- | --- |
| 未闭合的代码围栏 | pulldown 在输入结束处**会合成一个 block end**，所以 `TagEnd::CodeBlock` 不能证明围栏闭合。判据是 `pending.body_range.end < range.end`（闭合行一定在正文之后）| `parse.rs:1243-1256` |
| 未闭合围栏的代码块跨度 | 无 `CodeBlockSpan`；且流式下该跨度**永远不进入冻结前缀** | `output.rs:255-271`、测试 `streaming_open_fence_has_no_frozen_span`（`output.rs:527-541`）|
| 未闭合围栏的整体识别 | 交给**独立的分析 crate** 报 `StructuralIssue::UnterminatedCodeBlock`，判据是"opener 之后没有任何长度 ≥ opener 且同字符的闭栏行" | `lib.rs:210-224` |
| 半截表格 | 只有 `TagEnd::Table` 才产出 `TableReplace`；在分隔行/表体到达前，表格**一直以段落形式显示**，之后整块重排为盒线表 | `parse.rs:1298-1321` |
| 表格列数不匹配 | `StructuralIssue::MalformedTable` 专门检测这个失败模式（"*the 'made a table but it didn't show' bug*"）| `lib.rs:186-188`、`lib.rs:246-270` |
| 半截链接 | pulldown 未闭合时不会发 `Tag::Link`，`[text](ur` 以字面量显示，等 `)` 到达后重排 | 由 `Tag::Link` 的进入条件推得；`parse.rs:1089-1204` 整段依赖完整 range |
| 半截 LaTeX 定界符 | `LatexDelimiterNormalizer` **扣住有界的歧义后缀**到下一块，`finish()` 时 flush；扣留量受数学长度上限约束，"永不闭合的 opener 不能卡住流" | `latex_delimiters.rs:45-51` |
| 单波浪线 | 非流式问题，`DoubleTildeOnlyStrike` 在事件层降级为字面量 | `lib.rs:36-86` |

`pulldown-cmark` **从不报错**（`lib.rs:182`："*every input parses as something*"），所以"半截"永远表现为"退化渲染"，不能用错误处理。这正是 `xai-grok-markdown-core` 存在的理由——它用一个独立的 `analyze()` 把"用户/模型意图了什么结构 vs 实际解析出了什么"做差集，产出 `MarkdownAnalysis { stats, issues }`（`lib.rs:272-333`）。

### 6.3 是否重排、如何规避闪烁

**是否重排**：已冻结的前缀永不重排；**未冻结的尾段每次 chunk 都整段重渲染**（`streaming.rs:257` 开头就 `truncate` 再重渲染）。所以视觉上是"上半部分绝对稳定，最后一段可能轻微跳动"。

防闪烁/防跳动的具体机制：

1. **冻结边界只在语义安全处推进**，且未闭合的围栏、未完成的表格都留在 tail。任何"先显示成段落、后来变成表格"的重排都发生在视觉最底部。
2. **折行缓存按 `(width, generation, theme)` 三元组命中**（`markdown_content.rs:27-38`）。滚动（内容不变，generation 不变）直接返回缓存。
3. **折行也做增量**：`ensure_wrapped()`（`markdown_content.rs:270-352`）只重折"新冻结的行"与"tail 行"两个区间，已冻结行的折行结果保留在 `cache_lines[0..frozen_wrapped_count]`。测试 `frozen_cache_is_reused`（`markdown_content.rs:650-673`）断言冻结计数单调增长。
4. **主题切换不重置内容**：`cache_theme` 变化时只调 `renderer.set_style()` 再强制缓存失效，不复用旧色（`markdown_content.rs:276-283`）。
5. **尾部未闭合代码块走增量高亮**：`OpenCodeHighlighter`（`parse.rs:182-185`、`streaming.rs:134-136`）让"一个正在流式增长的代码块"总高亮代价是 O(N) 而非 O(N²)。
6. **帧级 diff**：只有变化的单元格被写往终端（`terminal.rs:385-420`），未变的单元格不产生字节。这是全屏 TUI 不闪烁的最后一道保障，与 Markdown 层无关但必须配合。
7. **收敛保证**：`finish()`（`streaming.rs:432-486`）做**一次全量重渲染**以保证正确性，`MarkdownContent::finish()` 随后把冻结计数清零（`markdown_content.rs:144-152`），使下一次 `ensure_wrapped` 全量重折。两条测试锁定"流式结果 == 一次性渲染结果"：`incremental_wrap_matches_full_wrap`（`markdown_content.rs:594-646`）与 `streaming_chunked_matches_full_render`（`output.rs:544-574`，用 `[4, 9, 1, 13, 7]` 循环的不规则 chunk 边界）。

`syntect` 句柄由 pager 侧统一供给（`markdown_content.rs:90`、`131` 传入 `Some(get_syntect())`），并在主题/风格/表宽/软换行模式变化时清空缓存（`streaming.rs:194-224`），避免主题切换后残留旧配色高亮。

## 7. 与 ratatui 的对接

对接面**极窄**——渲染器只产出 `Vec<ratatui::text::Line<'static>>` 加若干并列元数据（`output.rs:50-67`）：

```rust
pub struct MarkdownRenderOutput {
    pub lines: Vec<Line<'static>>,      // 可直接显示
    pub line_source_map: Vec<usize>,    // 渲染行 → 源行号（raw/pretty 切换保持光标位置）
    pub hyperlinks: Vec<HyperlinkTarget>,
    pub code_blocks: Vec<CodeBlockSpan>,// 围栏信息、干净 body、输出行范围、源字节范围
    pub tables: Vec<TableCopyMeta>,     // 单元格抄写元数据
}
```

要点：

- **没有自定义 Widget**。Markdown 不用 `ratatui::widgets` 的任何东西，只返回 `Line`，由 pager 的 scrollback 渲染层负责摆位。这降低了耦合面，也解释了为什么行级背景色要写在 `Line::style` 上（`render.rs:555`、`739`、`818`、`836`）：这是唯一能跨 span 传递"整行底色"的通道。
- **元数据是并列数组，不是嵌套结构**。`code_blocks` 用 `output_line_range` 指回 `lines`（`output.rs:36-39`），`tables` 用 `line_index + line_count` 指回，`hyperlinks` 用 `(line_index, column_range)`。这样 pager 可以在**折行之后**重新投影这些坐标（`markdown_content.rs:185-227` 暴露 `line_source_map()` / `pre_wrap_lines()` / `with_hyperlinks()` / `with_table_copy_meta()` / `mermaid_block_ranges()`）。
- **折行发生在 pager 侧，不在这两个 crate 里**。`MarkdownContent::ensure_wrapped()` 调 `word_wrap_lines_with_joiners`（`markdown_content.rs:312`、`325`），并把每个折行点的"被吃掉的空白"记成 joiner（`Vec<Option<String>>`），供超链接列坐标回算（`markdown_content.rs:369-405`）。
- **引用条在输出阶段被识别并排除出选区**。`MarkdownContent::output()` 用 `QuoteBarStrip` 把行首的 `│ ` 前缀标成不可选中（`markdown_content.rs:371`、`383-386`），`quote_bar.rs:53-61` 的 `quote_bar_style()` 必须与 `md_style.rs:133` 的 `blockquote_outer` **逐字段一致**，两侧注释互相指向并各有端到端测试锁死。
- **raw/pretty 是块级开关，默认键位 `r`**（`docs/user-guide/03-keyboard-shortcuts.md:66`：「Toggle raw markdown on selected entry」）。切换会调 `set_pretty` → 全量重渲染并清冻结状态（`streaming.rs:408-418`、`markdown_content.rs:255-266`），同时 generation +1 使折行缓存失效。

## 8. 不确定项与未找到证据

- **脚注定义的渲染方式**：`Event::FootnoteReference` 有处理（`parse.rs:832-834`），但 `parser_options()` 未显式启用 `ENABLE_FOOTNOTES`，全仓库（含测试、文档）无第二处证据。**未找到证据**。
- **`+ item` 列表标记**：`parse.rs:1019-1027` 的判定只覆盖 `- ` / `* ` / `N. ` / `N) `，`+ ` 既不套色也不替换。这是从代码路径推断的必然结果，但**没有测试直接断言 `+ item` 的渲染输出**。
- **任务列表 `[x]` 的最终视觉**：已确认**不存在**字形替换（全 crate 无 `☐`/`☑`/`✅`），只对 `[x]` 范围套色；因此推断 pretty 输出形如 `• [x] done`（`•` 来自列表标记替换，`[x]` 来自源文本）。**没有测试直接断言该组合输出**。
- **`TableBorders::ASCII` / `DOUBLE` 是否有调用方**：`format_table` 硬编码 `::BOX`（`parse.rs:1589`），两套常量在仓库内未找到使用点。可能是为未来主题化预留。
- **表格单元格的换行继承**：`Line::style` 的背景色随行传递（`markdown_content.rs:395-399`），但未找到证据表明表格单元格会使用 `code_background` 或任何自定义背景。
- **明文表格的 ANSI 渲染路径**（`render_ansi`，`render.rs:272`）与 ratatui 路径共享排版代码，但本专题只验证了 ratatui 路径；`render_ansi` 的具体消费方未追查。
- **Mermaid / LaTeX 的完整渲染细节**：本专题只覆盖到"Markdown 元素如何触发它们"的接口（`parse.rs:1375-1397`、`parse.rs:1535-1585`）；`mermaid.rs`（5699 行）与 `latex/`（多个模块）的算法属于独立课题。

## 关键结论（供合稿引用）

1. **解析器复用生态 crate，渲染完全自研**：`pulldown-cmark 0.13` 只负责出事件流，配置集中在 `xai-grok-markdown-core/src/lib.rs:18-24`（GFM + STRIKETHROUGH + MATH + TASKLISTS + TABLES），该 crate 刻意只依赖 `pulldown-cmark` 以便无头复用。
2. **只用 `~~x~~` 当删除线**：单波浪线对在事件层被降级为字面量 `~`，动机是 LLM 常写 `~**10%**` 表示"约"，`lib.rs:8` 与 `lib.rs:488-495`。
3. **样式是 27 字段的 `_inner`/`_outer` 二元结构**：`_inner` 管内容、`_outer` 管"pretty 模式下要隐藏的语法标记"，由 `pager-render/src/theme/md_style.rs:96-150` 每次从 `Theme::current()` 现取主题的 12 个 `md_*` 色位 + 6 个标题修饰符（`theme/tokyonight.rs:123-143`）。
4. **语法标记靠 `Effects::HIDDEN` 隐藏，不是删除文本**：`all_hidden()` 判定"所有活跃样式都 hidden"才跳过（`style.rs:210-225`），合并时 HIDDEN 被剥离（`style.rs:260`），且 BOLD 与 DIMMED 互斥（`style.rs:244-249`）。
5. **表格是自研两阶段列宽算法**：先取自然宽度，再扣 `num_cols*(2*padding+1)+1` 的边框开销，按"缺口占比"比例分配余量并把取整残差补给缺口最大的列；词宽放不下就退到"最宽 grapheme"下限，再放不下就保留词宽交由下游裁剪（`parse.rs:1609-1727`）。padding 固定 1，边框硬编码 `TableBorders::BOX`。
6. **表格超宽的处理是"折行 + 保证右边框"**：单元格用自定义词分隔符 `FirstFit` 折行（`break_words(false)`，URL 保护、`,`/`.` 数字格式保护），硬超列宽时按 grapheme 边界手切（`parse.rs:226-390`、`1869-1911`）；`markdown_content.rs:515-543` 断言每行宽度恰好等于内容宽度且保留右边框。
7. **折行层被明确告知"不要折表格"**：`pager-render/src/render/wrapping.rs:284-313` 的 `is_table_line()` 检测框线，命中则改为 `fit_line_to_width` 补宽，理由是"换行会破坏列对齐"。
8. **表格每两行表体之间都有 `├─┼─┤` 分隔线**，表头行强制加粗，边框色 = 主题 `md_muted` 再叠 DIM（`parse.rs:1593`、`1827-1839`、`2039-2041`）。
9. **链接渲染成 `text (url)`**：pretty 模式用两条 transform 删掉 `[` 并把 `](` 重写成 ` (`（`parse.rs:1176-1185`），URL 因此始终可见；点击能力由 OSC 8 提供，`xai-ratatui-inline/src/terminal.rs:57-72` 发射 `\x1b]8;id={id};{url}\x07` 并剥离 URL 中的控制字符。
10. **超链接经过三层坐标系投影**（源字节 → transform 后字节 → 显示单元格，`hyperlinks.rs:1-10`），Markdown 侧输出 `HyperlinkTarget { line_index, column_range, url, id }`（`output.rs:11-21`），OSC 8 的 `id=` 由终端层**重铸**以便折行片段被 hover 成同一条链接（`terminal.rs:74-95`）。
11. **流式只重渲染尾部，冻结只发生在顶层块边界**：`checkpoint.rs:1-14` 要求 `depth == 0` 且有空白行或围栏已闭合；未闭合围栏永远不进冻结区（`output.rs:527-541`），复杂度由 O(N²) 降到约 O(N)。
12. **闪烁规避靠四层叠加**：语义安全的冻结边界 + `(width, generation, theme)` 折行缓存与增量折行（`markdown_content.rs:270-352`）+ 尾部开放代码块的 O(N) 增量高亮 + 终端帧级单元格 diff（`terminal.rs:385-420`）；`finish()` 做全量重渲染并以两条测试锁定"流式 == 一次性渲染"（`markdown_content.rs:594-646`、`output.rs:544-574`）。
13. **没有自定义 ratatui Widget**：对接面只有 `Line<'static>` 加四个并列元数据数组（`output.rs:50-67`），折行、引用条裁剪、选区排除、超链接坐标回算全在 pager 侧完成（`markdown_content.rs:369-405`）。
