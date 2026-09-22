# 专题 03：排版、留白与信息密度

> 专题：排版、留白与信息密度｜调研日期：2026-09-22｜证据范围：`crates/codegen/xai-grok-pager-render/src/{render/wrapping.rs,render/line_utils.rs,render/osc8.rs,appearance/config.rs,util.rs,glyphs.rs}`、`crates/codegen/xai-grok-pager/src/{scrollback/**,views/**,app/**}`、`crates/codegen/xai-grok-markdown/src/{render.rs,parse.rs,mermaid.rs}`、`crates/codegen/xai-ratatui-textarea/src/{wrapping.rs,textarea.rs}`、`crates/codegen/xai-grok-pager-minimal/src/{commit.rs,live.rs}`、`crates/codegen/xai-grok-status-line/src/config.rs`、`crates/codegen/xai-grok-pager/docs/user-guide/{05-configuration.md,26-config-reference.md}`

## 1. 总览：排版是三层结构，不是一套全局令牌

Grok Build 的排版参数分布在三个层次，互相之间没有统一命名空间：

| 层 | 载体 | 是否用户可配置 | 作用范围 |
| --- | --- | --- | --- |
| 1 | `LayoutConfig`（`appearance/config.rs:168`） | 是（`pager.toml`） | 滚动区视口 + 块内左右内边距 |
| 2 | 每种块的 `*Config`（`EditBlockConfig`/`PromptConfig`/`ThinkingConfig`/`ExecuteConfig`） | 是（`pager.toml`） | 单块的 indent / vpad / 截断行数 |
| 3 | 各 view 文件顶部的局部 `const` | 否 | 弹窗、欢迎页、设置面板等 |

第 3 层规模不小：`views/` 下带缩进/留白语义的局部常量至少有 `H_MARGIN = 2`、`H_MARGIN_COMPACT = 1`（`views/welcome/mod.rs:110,112`）、`HERO_BOX_MIN_WIDTH = 90`、`V_PAD = 1`、`H_INSET = 2`、`LOGO_H_PAD = 3`（`views/welcome/hero_box.rs:14,17,20,23`）、`CHROME_PAD = 2`（`views/welcome/prompt.rs:20`）、`RAIL_WIDTH = 2`、`MIN_TERMINAL_WIDTH = 60`（`views/timeline.rs:17,20`）、`DESC_INDENT = 4`（`views/picker.rs:833`）、`QUESTION_VIEW_HPAD = 5` 与 `LEFT_PAD = 3`（`views/question_view.rs:1048,1059`）、`ELICIT_HPAD = 5`（`views/elicitation_view/render.rs:18`）、`LEFT_PAD = 3`（`views/permission_view.rs:418`）、`STANDARD_MAX_WIDTH = 110`（`views/settings_modal/state.rs:31`）。

**结论：只有滚动区有集中的排版模块，其余 UI 面各自为政。**

整屏纵向骨架（`views/agent.rs:218-231`）：

```
┌─ 终端全宽 W ─────────────────────────────────────────────────────────┐
│ StatusBar                                            Length(1)       │
│ Padding::new(eff_hpad_left, eff_hpad_right, top_vpad, bottom_vpad)   │
│  ┌────────────────────────────────────────────────────────────────┐  │
│  │ tasks / catalog / todo 面板（各前置 pane_gap 行）              │  │
│  │ status_gap                                                     │  │
│  │ 滚动区                                          ← Constraint::Min(SCROLLBACK_MIN_ROWS = 5)
│  │ btw / queue / turn_status / banner / cta / follow_ups / dock    │  │
│  │ prompt_height                                                  │  │
│  │ shortcuts                                                      │  │
│  └────────────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────────────┘
```

`pane_gap = if top_vpad == 0 { 0 } else { 1 }`（`views/agent.rs:229`），`status_gap` 同规则（`:244`）。滚动区是整屏唯一的 `Constraint::Min`，其余全是 `Constraint::Length(n)`；`SCROLLBACK_MIN_ROWS = 5` 的注释写明「The solver ranks it above every `Length`」（`views/agent.rs:92-94`）。

## 2. 版心与缩进体系

### 2.1 滚动区水平版心：accent + 双向 padding

`HorizontalLayout` 是唯一的水平版心定义点（`scrollback/layout.rs:7-34`）：

```rust
pub const ACCENT: u16 = 1;

let [accent, left_padding, content, right_padding] = Layout::horizontal([
    Constraint::Length(Self::ACCENT),
    Constraint::Length(config.block_pad_left),
    Constraint::Min(1),                              // Content takes remaining space
    Constraint::Length(config.block_pad_right),
]).areas(area);
```

结构注释写作 `│A│PL│Content│PR│`（`scrollback/layout.rs:47`）。默认值（`appearance/config.rs:181-190`）：

| 键 | 默认值 | 语义 |
| --- | --- | --- |
| `outer_vpad` | 1 | 视口上下各 1 行 |
| `outer_hpad_left` | 2 | 视口左侧 2 列 |
| `outer_hpad_right` | 2 | 视口右侧 2 列 |
| `block_pad_left` | 2 | accent 之后、内容之前 2 列 |
| `block_pad_right` | 2 | 内容之后 2 列（注释：`// Match left padding for symmetry`） |

`chrome_width = ACCENT + block_pad_left + block_pad_right`（`scrollback/layout.rs:41-43`）。

80 列下默认配置的完整列坐标（自算，依据上述常量）：

```
列   0   1   2   3   4   5 ...                                    W-3 W-2
     │   │   │   │   │
     ┃   ␠   ␠   ◆   ␠   文字起点
     │   │   │   └───┴── bullet 前缀 = bullet.char() 宽度 + 1 = 2
     │   │   └────────── block_pad_left = 2
     │   └────────────── accent 列（ACCENT = 1），画 ┃ / ❙ / ◆ 族字形
     └────────────────── outer_hpad_left = 2（在 HorizontalLayout 之外）
```

文字距窗口左沿 = `outer_hpad_left(2) + ACCENT(1) + block_pad_left(2) + bullet_indent(2) = 第 5 列`。
内容宽度 = `W − chrome_width − 2×outer_hpad = 80 − 5 − 4 = 71`（不含 bullet 时 `content.width` 为 `80 − 5 − 4 = 71`）。

### 2.2 缩进的度量单位

**全部是"终端单元格宽度"（display width），不是字节数也不是 char 数。** 证据：

- `BlockContext::bullet_indent()` 直接对字形取 `UnicodeWidthStr::width(c) + 1`（`scrollback/types.rs:115-122`），所以换成不同 bullet 字形时缩进会跟着变
- `BlockContext::content_width() = width − bullet_indent()`，注释说明这是为了让单行折叠内容不越过 bullet（`scrollback/types.rs:128-130`）
- `compute_subsequent_indent_width()` 对 `│ ` 前缀取 `UnicodeWidthStr::width("\u{2502} ")`（`scrollback/blocks/markdown_content.rs:411-428`）
- `pad_to_width` 的注释直说 `format!` 的 `{:<width$}` 按 char 数补齐会「shears columns after wide (e.g. CJK) glyphs」（`xai-grok-pager-render/src/util.rs:231-239`）

唯一的例外：`EditToolCallBlock` 用 `const INDENT: &str = "  "` 并取 `INDENT.len()` 作为宽度（`scrollback/blocks/tool/edit.rs:103,163,191,578,615`）。因为纯 ASCII 空格下字节数 == 单元格数，这里等价；但它是一个**字符串长度当宽度用**的隐患点。

### 2.3 缩进层级清单

| 层级 | 宽度 | 来源 |
| --- | --- | --- |
| 视口左右留白 | 2 / 2（compact 时 1 / 1） | `config.rs:184-188`、`:203-218` |
| accent 列 | 1 | `scrollback/layout.rs:17` |
| 块左内边距 | 2 | `config.rs:187` |
| 工具块 bullet 前缀 | `bullet.char()` 宽度 + 1 = 2（默认 `◆`） | `types.rs:115-122`、`config.rs:527` |
| Edit 块 diff 内容缩进 | 2（可选，`indent = true`） | `edit.rs:103`、`config.rs:400` |
| thinking 体内 rail | 2（`BODY_RAIL_WIDTH`，仅 minimal 模式） | `blocks/thinking.rs:62,108-115` |
| Markdown 列表嵌套 | **跟随源文档空格数**，渲染不做归一化 | `xai-grok-markdown/src/parse.rs:1012-1042` |
| Markdown 引用嵌套 | 每级 2 列（`│ `） | `parse.rs:922-924`、`markdown_content.rs:411-428` |
| 表格单元格内边距 | 1（每格左右各 1） | `xai-grok-markdown/src/parse.rs:1590` |

即归纳起来是 **3 个固定级别（视口 2 / accent 1 + 块内 2 / bullet 2）+ 2 个可变级别（Markdown 列表、引用）**。

### 2.4 第二套版心：居中定宽盒

内容流是"全宽 + 小留白"，覆盖层则是"定宽 + 居中"，两套并存：

- 欢迎页 hero box：`box_width = content_area.width.saturating_sub(6).min(120)`（`views/welcome/hero_box.rs:102`），最小宽 `HERO_BOX_MIN_WIDTH = 90`（:14），内部 `V_PAD = 1`、`H_INSET = 2`、`LOGO_H_PAD = 3`（:17,20,23），用 `Layout::horizontal([Min(0), Length(box_width), Min(0)]).flex(Flex::Center)` 居中（:170-176）
- 弹窗 `ModalSizing`（`views/modal_window.rs:89-118`）：默认 `width_pct = 0.9`、`max_width = 140`、`min_width = 60`、`v_margin = 7`、`h_pad = 2`、`v_pad = 2`、`footer_lines = 2`；中等弹窗 `medium()` 为 `0.60 / 120 / 44 / 4 / 2 / 1 / 2`（:124-135）
- 实际宽度求解（`modal_window.rs:576-585`）：
  ```rust
  let max_width = area.width.saturating_sub(4).min(sizing.max_width);
  let preferred_width = (area.width as f32 * sizing.width_pct) as u16;
  let modal_width = preferred_width.min(max_width).max(sizing.min_width).min(area.width);
  ```
  注释强调「Clamp to the buffer last: the `min_width` floor can exceed a narrow terminal」
- 弹窗内边距：`inner_width = modal_width − 2`（边框各 1 列，`modal_window.rs:569-570`），内容与页脚再各减 `h_pad`（:296-299、:363、:372-374）

## 3. 垂直节奏

### 3.1 块内 vpad：只有两类块真的有

基类默认 `has_vpad_for → true`（`scrollback/block.rs:68-70`，注释「Vertical padding (blank line with accent top/bottom)」），但**几乎每个块都覆写为 `false`**。全量盘点（`scrollback/blocks/**`）：

| 块 | `has_vpad_for` | 条件 |
| --- | --- | --- |
| 用户提示词 | `prompt.vpad && !compact` | 默认真 → **有 vpad** |
| Edit | `edit.vpad` | 默认 false → 无 vpad |
| Agent 消息 | `false` | `blocks/agent.rs:184-186` |
| thinking | `false` | `thinking.rs:520-522` |
| execute（bash） | `false` | `tool/execute.rs:687-689` |
| read / search / search_tool / web_search / web_fetch / use_tool / list_dir / memory_search / sent_message / other | 全部 `false` | 各文件同名方法 |
| system / session_event / context_info / subagent / bg_task / btw / workflow | 全部 `false` | 各文件同名方法 |

因此**默认形态下只有用户提示词上下各空 1 行**，工具与思考块是紧凑的。

vpad 的记账方式：`Has_vpad → vpad = 2`（上 1 下 1），`entry.rs:416-419`、`scrollback_pane.rs:589`；渲染时 `vpad_top = if has_vpad { 1u16 } else { 0 }`（`scrollback/render.rs:438-441`）。空间不足时整体放弃：

```rust
// Need at least 3 rows: vpad_top (1) + content (1) + vpad_bottom (1)
// If less space, skip vpad to prioritize content.
let use_vpad = block_has_vpad && content_area.height >= 3;
```
（`scrollback/scrollback_pane.rs:743-746`）

### 3.2 块间距：一条成对规则，不是常量

块与块之间的空行不写死在块里，而是由相邻两块的属性决定（`scrollback/state/layout.rs:1555-1563`）：

```rust
fn gap_after_between(prev: &ScrollbackEntry, next: &ScrollbackEntry) -> u16 {
    if groups::can_join_dense_run(prev, /*collapsed_only=*/ true)
        && groups::can_join_dense_run(next, /*collapsed_only=*/ true)
    { 0 } else { 1 }
}
```

dense run 的准入条件（`scrollback/state/groups.rs:172-176`）：

```rust
pub(super) fn can_join_dense_run(entry: &ScrollbackEntry, collapsed_only: bool) -> bool {
    entry.block.is_groupable()
        && !entry.block.is_turn_terminal_marker()
        && (!collapsed_only || entry.display_mode == DisplayMode::Collapsed)
}
```

**于是垂直节奏的语义是：连续默认折叠的同族工具块堆成无间隔的密集带（gap = 0）；一旦跨回合（turn-terminal 标记）或任一块展开（不再是 `Collapsed`），就插入 1 行间隔。** 末条目固定 `gap_after = 1`（`state/layout.rs:1257-1260`、`:1305`），用于选择框的尾部留白（注释见 `state/layout.rs:966-968`）。

```
默认折叠的两个工具块之间：
 ┃ ◆ Read a.rs
 ┃ ◆ Read b.rs        ← gap_after = 0，密集带

其中一个展开后：
 ┃ ◆ Read a.rs
 ┃   ─── src/a.rs ───
 ┃   …12 行…
                     ← gap_after = 1
 ┃ ◆ Read b.rs
```

### 3.3 Markdown 块级空行：开栅栏前必空行

pretty 模式下代码围栏的 ``` 会被隐藏，所以渲染器在隐藏围栏时补一行空行，**只补在开栅栏前，闭合栅栏前不补**（`xai-grok-markdown/src/render.rs:657-676`）：

```rust
if is_code_fence {
    // Emit a blank separator before an OPENING fence (not closing)
    // Otherwise adjacent blocks (a list then a code block) collapse together once pretty mode hides the fence markers
    if !in_hidden_code_block && lines.last().is_some_and(|l| l.width() > 0) {
        line_source_map.push(current_source_line);
        lines.push(Line::default());
        cur_col_in_line = 0;
    }
    in_hidden_code_block = !in_hidden_code_block;
}
```

回归测试 `test_list_followed_by_code_block_has_separator`（`render.rs:1271-1288`）断言列表项与代码块之间 `world_idx - hello_idx >= 2`。缩进的围栏块会先去掉缩进再走同一路径，并与非缩进版本产出完全一致的输出（`render.rs:2516-2552`）；且「no spurious trailing blank line」（`render.rs:2581`）。

Markdown 段落之间**不做额外增行**：软换行在非续行位置被折叠成等长空格（`parse.rs:736-751`），块间空行来自源文档本身。

### 3.4 首行与末行

- 顶部：`outer_vpad` 1 行；`SHORT_TERMINAL_ROWS = 16` 及以下时底部 padding 归 0（`views/agent.rs:205-209`）
- 末条：`gap_after = 1`（`state/layout.rs:1260`）
- 粘性头：`HEADER_CONTENT_GAP = 1`（`scrollback/sticky.rs:92`，注释「Gap row between header and content for visual separation」），且「pinned_height includes gap」（`scrollback_pane.rs:1116`）
- 面板与滚动区之间：`Pane_gap`/`status_gap`，窄屏 compact 时归 0（`views/agent.rs:229,244`）
- 输入框内部：`vpad_top | text(Min 1) | info_block`（`views/prompt_widget/mod.rs:3024-3032`），`info_block = if has_info { 1 } else { 0 }`（`:275-277`）

## 4. 换行策略

### 4.1 度量与算法

所有换行都走 `textwrap`，配置集中在 `RtOptions`（`xai-grok-pager-render/src/render/wrapping.rs:148-181`）：

```rust
pub struct RtOptions<'a> {
    pub width: usize,
    pub line_ending: textwrap::LineEnding,
    pub initial_indent: Line<'a>,
    pub subsequent_indent: Line<'a>,
    pub break_words: bool,
    pub wrap_algorithm: textwrap::WrapAlgorithm,
    pub word_separator: textwrap::WordSeparator,
    pub word_splitter: textwrap::WordSplitter,
}

// Default::new:
//   break_words: true
//   wrap_algorithm: WrapAlgorithm::FirstFit
//   word_splitter: WordSplitter::HyphenSplitter
//   word_separator: WordSeparator::new()
```

宽度单位是**显示列**：`textwrap` 通过 `UnicodeWidthStr`/`UnicodeWidthChar` 计宽（`wrapping.rs:7`、`line_utils.rs:3`），`byte_offset_to_display_col` 逐字符累加 `ch.width().unwrap_or(0)`（`wrapping.rs:71-82`）。

首行与续行的可用宽度分别扣除各自缩进，且下限为 1：

```rust
let initial_width_available = opts.width.saturating_sub(rt_opts.initial_indent.width()).max(1);
let subsequent_width_available = opts.width.saturating_sub(rt_opts.subsequent_indent.width()).max(1);
```
（`wrapping.rs:367-371`、`:406-409`）

`FirstFit` 是贪心算法，注释特别提醒：「FirstFit is greedy, so single-pass equals the two-stage wrap; a non-greedy change must mirror it」（`wrapping.rs:84-85`）——这是为了让搜索高亮的断点与视觉渲染严格一致。

### 4.2 CJK 与 emoji

- **CJK**：无专门分支，靠 `unicode_width` 判为宽 2 列自然参与换行；`truncate_to_width("日本語ラベル", 5) == "日本…"`（`util.rs:269-276` 测试）
- **emoji**：按 **grapheme cluster** 整体处理，绝不切半。`fit_line_to_width` 逐 `graphemes(true)` 取整簇，若一簇宽 2 放不进剩余 1 列则整簇丢弃并用 1 个空格补位（`wrapping.rs` 的 `line_utils.rs:125-159`）。测试 `fit_line_does_not_split_emoji_grapheme` 断言 `"ab\u{26A0}\u{FE0F}"` 裁到 3 列得 `"ab "`（`line_utils.rs:326-343`）
- **emoji 计宽**：`line_height_counts_double_width_emoji` 断言 `"😀😀😀"` 在宽度 4 时折成 2 行、宽度 2 时折成 3 行（`xai-ratatui-textarea/src/wrapping.rs:587-593`）

### 4.3 长 token / 长 URL 的硬断

**没有 URL 专用断行器**（`grep "url_split\|UrlSplit"` 只命中测试名）。统一靠 `break_words(true)`：长 token 无法放进一行时在字符边界强制断开。`wrap_ranges_many_newlines_width_one_does_not_panic` 之类的边界测试覆盖了宽度 1 的退化情形（`xai-ratatui-textarea/src/wrapping.rs:596-620`）。

真正特别的是**表格单元格**：这里 `break_words(false)`，绝不切开单词（`xai-grok-markdown/src/parse.rs:1870-1876`），但若某个不可断词仍超宽，则改用 grapheme 粒度硬切，理由写在注释里：

```rust
// An unbreakable word survived textwrap wider than the column
// break_words is off, and textwrap's char-based emergency split can tear grapheme clusters
// Hard-split on grapheme boundaries using the same display-width model as the table formatter
```
（`parse.rs:1884-1907`）

软换行被折行的路径仍然是**整段可点击**的：OSC 8 扫描把 `Some(joiner)` 的续行重新拼回一条逻辑行再匹配（`xai-grok-pager-render/src/render/osc8.rs:390-431`），注释「Soft-wrapped rows (`Some` joiner) are re-joined before matching so a wrapped path is detected whole」。

### 4.4 表格永不换行

```rust
/// Never wrap tables: wrapping destroys column alignment. A leading `│` is a blockquote unless an interior `│` separates cells.
fn is_table_line(line: &Line<'_>) -> bool { … }
```
（`wrapping.rs:284-313`）

命中后走 `fit_line_to_width` 补齐到内容宽度，而不是折行（`wrapping.rs:340-344`）：

```rust
// Never wrap tables. Pad to content width: terminal clipping desyncs on wider-than-measured glyphs and strands a ghost cell.
if is_table_line(line) {
    let fitted = fit_line_to_width(line.clone(), rt_opts.width);
    return (vec![fitted], vec![None]);
}
```

### 4.5 截断与省略号

两套语义截然不同的"变窄"操作：

| 函数 | 位置 | 行为 | 省略号 |
| --- | --- | --- | --- |
| `truncate_line` | `line_utils.rs:63-100` | 在预算耗尽处截断并追加省略号，保留 span 样式 | `…`（U+2026），预留 1 列 |
| `fit_line_to_width` | `line_utils.rs:104-166` | 按 grapheme 裁剪**不**加省略号，或补空格到目标宽度 | 无 |
| `truncate_to_width` | `pager-render/src/util.rs:204-216` | 字符串版，`byte_offset_at_width(s, max_width - 1)` + `…` | `…` |
| `cascade_truncate` | `line_utils.rs:177-222` | 固定优先级降级序列 | `…`（由 `truncate_str` 加） |

`cascade_truncate` 的降级顺序写在文档注释里：「Drop description first, then meta, then truncate activity, then type」（`line_utils.rs:176`）——这是窄屏下"信息优先级"的显式编码。

### 4.6 软换行 / 硬换行的表示

`word_wrap_line_with_joiners` 返回 `(Vec<Line>, Vec<Option<String>>)`，**`None` = 硬换行，`Some(s)` = 软换行且 `s` 是断点处被跳过的原文空白**（`wrapping.rs:330-334`）：

```rust
/// First line is `None`. Continuation joiners are the exact substring skipped at the wrap boundary (often spaces, possibly empty).
```

这个 joiner 数组被下游复用为：文本复制取回原文（`blocks/block.rs:1284` 的 `test_copy_visible_text_in_state_rejoins_wrapped_lines`）、链接扫描重拼逻辑行（`render/osc8.rs:390-398`）、选择区映射跳过 `indent_width`（`scrollback/render.rs:930-936`）。

### 4.7 引用块与标题的缩进策略

- **块引用**：`blockquote_prefix_len` 数出前导 `│ ` 序列（`wrapping.rs:315-328`），若调用方未设 `subsequent_indent` 就自动设成该前缀，使续行重复引用标记（`wrapping.rs:348-355`）
- **悬挂缩进**（工具头一行标题在前、续行对齐到内容列）`wrap_header_hanging(header, width, extra_indent)`：首 span 留在第 1 行，其余内容按 `extra_indent + prefix_width` 悬挂缩进（`wrapping.rs:634-675`）；调用点 `tool/execute.rs:412,451`
- **齐头缩进**（续行缩到固定列，通常是 bullet 宽度）`wrap_header_flush(header, width, indent)`（`wrapping.rs:677-689`）；调用点 `tool/web_fetch.rs:198`、`tool/search_tool.rs:190`、`tool/use_tool.rs:159`、`tool/web_search.rs:226`、`tool/memory_search.rs:150`、`app/agent_view/render.rs:3820`

有两份几乎相同的 wrapping 模块：`xai-grok-pager-render/src/render/wrapping.rs`（`pub`）与 `xai-ratatui-textarea/src/wrapping.rs`（`pub(crate)`），差异在于 textarea 版会跳过指针不在输入缓冲区内的 `Cow::Borrowed` 片段，并把行尾空格并入 range（`xai-ratatui-textarea/src/wrapping.rs:19-34`）。

## 5. 信息密度取舍

### 5.1 三种显示模式与默认值全表

模式枚举 `Collapsed | Truncated | Expanded`（`scrollback/types.rs:63-70`），基类默认 `Expanded`（`block.rs:102-108`）。各块实测默认值：

| 块 | 默认模式 | 证据 |
| --- | --- | --- |
| 用户提示词 | 可折叠→`Collapsed`，否则 `Expanded` | `blocks/user.rs:508-513` |
| thinking | `Truncated` | `blocks/thinking.rs:554-555` |
| 用户 `!` bash | `Truncated`；完成时自动 `Expanded` | `tool/execute.rs:719-723`、`:727-733` |
| Agent 工具 bash | `Collapsed` | `tool/execute.rs:719-723` |
| Edit | `Collapsed`，再由 `edit_default_display_mode` 升级 | `tool/edit.rs:1419-1422`、`state/mod.rs:1684-1691` |
| read / search / search_tool / web_search / web_fetch / use_tool / list_dir / memory_search / sent_message / other | 全部 `Collapsed` | 各文件 `default_display_mode` |
| subagent / bg_task / btw / workflow | 全部 `Collapsed` | 各文件 `default_display_mode` |
| 部分 session_event | `Collapsed`（:326）/ `Expanded`（:787） | `blocks/session_event.rs` |

Edit 的升级规则（`state/mod.rs:1684-1691`）：

```rust
fn edit_default_display_mode(expanded_by_default: bool, edit: &EditToolCallBlock) -> DisplayMode {
    if edit.is_success() && (edit.summary_untrusted || expanded_by_default) {
        DisplayMode::Expanded
    } else { DisplayMode::Collapsed }
}
```

`expanded_by_default` 由 `EditBlockConfig::effective_expanded` 决定：显式 `pager.toml` 值优先，未设置时回落到 shell 侧 `collapsed_edit_blocks` 标志的取反（`config.rs:415-420`）。失败的 Edit 折叠、`summary_untrusted` 的 Edit 展开。thinking 与 execute 在 `finish_running` 时还有 `finished_display_mode` 覆盖（`block.rs:113-117`）。

### 5.2 截断模式的构图

thinking 默认 `Truncated` = 一行 `…` + 末尾 N 行（`thinking.rs:340-396`，文档注释见 `:121`）：

```
┃◆ Thought for 12s
┃  …
┃  最后第 3 行
┃  最后第 2 行
┃  最后第 1 行        ← truncated_lines = 3（config.rs:495）
```

内容行数 `<= n` 时全部显示、不加省略号（`thinking.rs:360-375`）。

bash 截断用首/尾两段：`first_lines = 2`、`last_lines = 3`（`config.rs:611-613`）。

### 5.3 分组与「N more」

超过 `group_max_visible = 10`（`config.rs:161`）的折叠密集带被折叠成一行组头：`render_group_header` 用 dimmed `◈`（`group_header_chrome_prefix()` = `diamond_dotted()` + 1 空格）加粗灰字（`wrappers/entry_renderer.rs:279-292`、`:520-527`）。文案分两支：

```rust
let label = if self.group_collapse_header {
    format!("{n} tool calls & thoughts")
} else {
    format!("{n} more")
};
```
（`entry_renderer.rs:280-286`）

组头仍走同一套 `accent | block_pad_left | content(Min 1) | block_pad_right` 布局（`entry_renderer.rs:207-219`），并且 accent 列被显式填充背景而不是保留上一帧字形（注释：「an unowned cell survives the frame diff」，`entry_renderer.rs:224-227`）。

### 5.4 窄屏 / 矮屏降级规则

**两级阈值，全在 `views/agent.rs:89-98`：**

```rust
pub const SHORT_TERMINAL_ROWS: u16 = 16;
pub const SCROLLBACK_MIN_ROWS: u16 = 5;
pub const AUTO_COMPACT_MAX_ROWS: u16 = 20;
const _: () = assert!(SHORT_TERMINAL_ROWS < AUTO_COMPACT_MAX_ROWS);
```

| 触发条件 | 动作 | 证据 |
| --- | --- | --- |
| `terminal_rows <= 20` | 强制 compact：`outer_vpad` 1→0，`outer_hpad_left/right` 2→`MIN_HPAD = 1` | `views/agent.rs:102-104`、`config.rs:198-218` |
| `area.height <= 16` | `bottom_vpad → 0`，`cta_height → 0`，`follow_ups_height → 0` | `views/agent.rs:205-216` |
| `top_vpad == 0` | `pane_gap`/`status_gap` → 0 | `views/agent.rs:229,244` |
| 块可用高度 `< 3` | 放弃 vpad，优先保内容 | `scrollback_pane.rs:743-746` |
| `area_width < 60` 或不满足最小轮次 | 不显示时间轴 rail（`RAIL_WIDTH = 2`） | `views/timeline.rs:14-20,61-76` |
| 欢迎页 compact | `h_margin` 2→`H_MARGIN_COMPACT = 1` | `views/welcome/mod.rs:110-112,765-770` |
| 弹窗 `modal_width < 20 \|\| modal_height < 6` | 整体不渲染弹窗，清空命中矩形 | `views/modal_window.rs:253-259` |

compact 是**派生渲染值**，不写回用户设置也不落盘（`config.rs:77-80`、`app/agent_view.rs:2150-2152`），重算时机仅为 Resize 事件（`app/event_loop.rs:2540`）、设置变更（`app/dispatch/settings/setters.rs:806`）与启动（`app/event_loop.rs:1601`）。`user.rs:481` 里 `prompt.compact` 直接参与 `has_vpad_for`，所以 compact 会同时吃掉提示词块的上下空行。

最小模式另有一套更激进的收缩：`block_pad_left = block_pad_right = 0`、`show_timestamps = false`，并用 `with_hide_accent(true)` 回收 accent 列（`xai-grok-pager-minimal/src/commit.rs:180-193`、`:214-222`）。

### 5.5 输入框区域

输入框有独立的一套内边距，与滚动区不同（`views/prompt_widget/mod.rs:217-224`）：

```rust
vpad_top: 1,
chrome_pad_left: 2,
chrome_pad_right: 1,   // 注意左右不对称
```

三种预设：默认（带 chrome 与边框）、`overlay()`（`chrome = false, vpad_top = 0`，:242-248）、`inline(bg)`（另加 `chrome_pad_left = chrome_pad_right = 0`、`show_prefix = false`，:249-265）。textarea 自身：默认 `tab_width = 4`（`textarea.rs:246-247`，与滚动区 `tab_width = 4` 对齐，`config.rs:160`），滚动条可见时内容宽度再减 `1 + scrollbar_padding`（`textarea.rs:3112-3126`），prompt widget 把 `scrollbar_padding = 1`（`prompt_widget/mod.rs:708`）。

## 6. 排版参数表（最可靠的一份）

来源优先级：代码 `Default` 实现 > 用户手册。两者已逐项核对一致（`docs/user-guide/05-configuration.md:703-770`）。

### 6.1 可配置项

| 参数 | 默认 | 单位 | 位置 |
| --- | --- | --- | --- |
| `scrollback.layout.outer_vpad` | 1 | 行 | `config.rs:184` |
| `scrollback.layout.outer_hpad_left` | 2 | 列 | `config.rs:185` |
| `scrollback.layout.outer_hpad_right` | 2 | 列 | `config.rs:186` |
| `scrollback.layout.block_pad_left` | 2 | 列 | `config.rs:187` |
| `scrollback.layout.block_pad_right` | 2 | 列 | `config.rs:188` |
| `MIN_HPAD`（常量，非键） | 1 | 列 | `config.rs:195` |
| `scrollbar.gap_left` / `gap_right` | 0 / 0 | 列 | `config.rs:252-253` |
| `display.tab_width` | 4 | 列 | `config.rs:160` |
| `display.group_max_visible` | 10 | 条 | `config.rs:161` |
| `display.expandable_indicator_char` | `"›"` | 字形 | `config.rs:157` |
| `display.dim_accent` | 0.5 | 比例 | `config.rs:152` |
| `display.collapsed_accent_char` | `glyphs::collapsed_accent()` = `❙` | 字形 | `config.rs:151` |
| `blocks.edit.indent` | true | 开关（2 列） | `config.rs:400` |
| `blocks.edit.vpad` | false | 开关（2 行） | `config.rs:401` |
| `blocks.edit.hunk_separator` | `"…"` | 字符串 | `config.rs:409` |
| `blocks.edit.dual_line_numbers` | false | 开关 | `config.rs:410` |
| `blocks.prompt.vpad` | true | 开关（2 行） | `config.rs:448` |
| `blocks.prompt.min_lines` | 2 | 行 | `config.rs:450` |
| `blocks.thinking.truncated_lines` | 3 | 行 | `config.rs:495` |
| `blocks.thinking.bg_blend` | 0.7 | 比例 | `config.rs:494` |
| `blocks.execute.first_lines` / `last_lines` | 2 / 3 | 行 | `config.rs:611-612` |
| `blocks.tool.bullet` | `Diamond`（`◆`） | 枚举 | `config.rs:527` |
| `ui.compact_mode` | false | 开关 | `config.rs:89`、`docs/user-guide/26-config-reference.md:593` |

### 6.2 硬编码常量（不可配置）

| 常量 | 值 | 位置 |
| --- | --- | --- |
| `HorizontalLayout::ACCENT` | 1 列 | `scrollback/layout.rs:17` |
| `EntryRenderer::CHROME_WIDTH` | `1 + 2 + 1 = 4`（legacy 估算） | `wrappers/entry_renderer.rs:306` |
| Edit `INDENT` | `"  "`（2 列） | `tool/edit.rs:103` |
| `BODY_RAIL_WIDTH` | 2 列 | `blocks/thinking.rs:62` |
| `HEADER_CONTENT_GAP` | 1 行 | `scrollback/sticky.rs:92` |
| `SHORT_TERMINAL_ROWS` | 16 行 | `views/agent.rs:91` |
| `SCROLLBACK_MIN_ROWS` | 5 行 | `views/agent.rs:94` |
| `AUTO_COMPACT_MAX_ROWS` | 20 行 | `views/agent.rs:97` |
| `RAIL_WIDTH` / `MIN_TERMINAL_WIDTH` | 2 列 / 60 列 | `views/timeline.rs:17,20` |
| `H_MARGIN` / `H_MARGIN_COMPACT` | 2 / 1 列 | `views/welcome/mod.rs:110,112` |
| `HERO_BOX_MIN_WIDTH` / `V_PAD` / `H_INSET` / `LOGO_H_PAD` | 90 / 1 / 2 / 3 | `views/welcome/hero_box.rs:14,17,20,23` |
| `ModalSizing::default()` | `0.9 / 140 / 60 / 7 / 2 / 2 / 2` | `views/modal_window.rs:109-117` |
| `ModalSizing::medium()` | `0.60 / 120 / 44 / 4 / 2 / 1 / 2` | `views/modal_window.rs:124-135` |
| 弹窗最小可视尺寸 | 20 列 × 6 行 | `views/modal_window.rs:253` |
| 表格单元格 padding | 1 列 | `xai-grok-markdown/src/parse.rs:1590` |
| `PROMPT_ARROW_WIDTH` | 2 列（`❯ `） | `pager-render/src/glyphs.rs:16-18` |
| mermaid `MAX_LABEL/PAD/GAP_X/GAP_Y/WRAP_WIDTH/MAX_LINES` | 28 / 1 / 3 / 2 / 24 / 4 | `xai-grok-markdown/src/mermaid.rs:28-34` |
| mermaid `SEQ_GAP` | 5 | `mermaid.rs:3374` |
| status-line `MAX_PADDING_PER_SIDE` | 16 列 | `xai-grok-status-line/src/config.rs:156` |

## 7. 未找到证据的项

- **行距（line spacing）**：只在 `RtOptions.line_ending` 里看到 `LineEnding::LF`（`wrapping.rs:173`），没有"行高 > 1"的概念。未找到任何行距参数。
- **"代码块后必空行"**：只有"开栅栏前空行"（`xai-grok-markdown/src/render.rs:663-676`）；闭合栅栏后是否有空行由源文档决定，未找到强制规则。
- **窄屏"标签变短"**：找到字号/内容降级（`cascade_truncate`）、rail 隐藏、面板行删除，但**未找到**任何"文案变短"的映射表（例如把 `Thinking...` 缩成 `…`）。
- **全局设计令牌模块**：未找到。`LayoutConfig` 只覆盖滚动区；`spacing`/`padding`/`indent` 三词没有一个统一的常量模块。
- **换行算法在 CJK 下的专用断行规则**（如 CJK 禁则换行）：未找到证据，行为完全由 `textwrap` + `unicode_width` 决定。
- **`block_pad_left/right` 的合法区间校验**：`LayoutConfig::validated()` 只 clamp `outer_hpad_*`，`block_pad_*` 原样透传（`config.rs:220-229`）。

## 关键结论（供合稿引用）

1. **排版三层结构，只有滚动区集中**：`LayoutConfig`（`appearance/config.rs:168`）+ 各类块 `*Config` 是可配置层，其余 UI 面是散落的局部 `const`，不存在全局设计令牌模块。
2. **滚动区水平版心 = accent(1) + block_pad_left(2) + 内容 + block_pad_right(2)**，定义在 `scrollback/layout.rs:17-34`，`chrome_width` 由三者相加得出（:41-43）。
3. **外层视口留白默认上下 1、左右各 2**（`appearance/config.rs:184-188`），用户文档与代码默认值逐项一致（`docs/user-guide/05-configuration.md:703-713`）。
4. **缩进一律以终端显示列（display width）度量**，绝不按字节或 char：`bullet_indent()` 现算字形宽度 + 1（`scrollback/types.rs:115-122`），`pad_to_width` 专门绕开 `format!` 的 char 计数（`pager-render/src/util.rs:231-239`）。
5. **块间距不是常量而是一条成对规则**：`gap_after_between` 在两侧都可加入 dense run 时返回 0、否则返回 1（`scrollback/state/layout.rs:1555-1563`），准入条件见 `scrollback/state/groups.rs:172-176`；末条目固定为 1。
6. **默认只有用户提示词块有垂直 padding**：基类 `has_vpad_for` 默认 true（`block.rs:68-70`），但工具/思考/系统块全部覆写为 false；实际生效者仅 `prompt.vpad && !compact`（`blocks/user.rs:480-482`）与 `edit.vpad`（`tool/edit.rs:1402-1404`）。高度不足 3 行时放弃 vpad（`scrollback_pane.rs:743-746`）。
7. **换行统一走 textwrap `FirstFit` + `HyphenSplitter` + `break_words(true)`**（`render/wrapping.rs:176-179`），无 URL 专用断行器；表格是唯一 `break_words(false)` 并改用 grapheme 硬切的地方（`xai-grok-markdown/src/parse.rs:1870-1907`）。
8. **emoji 按 grapheme cluster 整体处理、绝不切半**，裁不下就整簇丢弃并补空格（`render/line_utils.rs:138-158`，测试 `:326-343`）；CJK 无特殊分支，靠 `unicode_width` 的宽 2 判定。
9. **硬换行与软换行由 joiner 数组区分**：`None` = 硬断，`Some(被跳过的空白)` = 软断（`render/wrapping.rs:330-334`）；该信息被复制回取、链接重拼与选择映射三处复用。
10. **省略号统一是 `…`（U+2026，占 1 列）**，在预算中显式预留：`truncate_line` 用 `max_width - 1`（`render/line_utils.rs:74`），`truncate_to_width` 同样（`pager-render/src/util.rs:211-215`）。
11. **信息密度默认高度偏紧**：几乎所有工具块与思考块默认 `Collapsed` 或 `Truncated`，Edit 默认 `Collapsed` 再按 `effective_expanded` 升级（`state/mod.rs:1684-1691`），仅用户提示词与 agent 消息默认 `Expanded`。
12. **两条窄屏阈值：`terminal_rows <= 20` 触发强制 compact**（视口留白 1/2/2 → 0/1/1，面板间隔归 0），**`area.height <= 16` 触发更深降级**（底部留白、CTA、follow-up 全部归 0），另有 60 列以下隐藏时间轴 rail（`views/agent.rs:89-98,205-216`、`views/timeline.rs:61-76`）。compact 是渲染期派生值，不写回设置也不落盘（`app/agent_view.rs:2150-2152`）。
