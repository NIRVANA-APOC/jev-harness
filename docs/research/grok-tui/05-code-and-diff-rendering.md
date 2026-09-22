# 专题 05：代码高亮与 diff 呈现

> 专题：代码高亮与 diff 呈现｜调研日期：2026-09-22｜证据范围：`crates/codegen/xai-grok-pager-diff/`、`crates/codegen/xai-grok-pager-render/src/syntax.rs`、`.../src/theme/`、`.../assets/*.tmTheme`、`crates/codegen/xai-grok-pager/src/scrollback/blocks/tool/{edit,read,mod}.rs`、`.../blocks/markdown_content.rs`、`.../app/edit_highlight_worker.rs`、`.../views/block_viewer/mod.rs`、`crates/codegen/xai-grok-markdown(+/ -core, build.rs)/src/{syntax,parse,render,style,output,open_code_highlighter}.rs`、`crates/codegen/xai-hunk-tracker/src/`、`crates/codegen/xai-grok-pager-render/src/appearance/config.rs`、`docs/user-guide/06-theming.md`。路径省略 `reference/grok-build/` 前缀。

## 1. 代码地图：四层职责

| 层 | 位置 | 职责 |
| --- | --- | --- |
| 数据层 | `xai-grok-pager-diff/src/lib.rs`（单文件 crate） | 把编辑工具输出变成带行号的 `DiffHunk`，以及反向生成 unified patch |
| 颜色层 | `xai-grok-pager-render/src/syntax.rs`、`src/theme/` | syntect 单例、`.tmTheme` 选择、RGB→终端色的量化与极性安全映射、diff 六个色槽 |
| 渲染层 | `xai-grok-pager/src/scrollback/blocks/tool/edit.rs`（3185 行） | gutter、背景色带、软换行、hunk 分隔行、头部 diffstat |
| 异步层 | `xai-grok-pager/src/app/edit_highlight_worker.rs` | 后台线程做“全文件作用域”高亮升级 |

另有两条平行线索：`xai-hunk-tracker`（diff 归属与 LOC 统计，**不参与 TUI 渲染**）与 `xai-grok-markdown(-core)`（assistant 消息里 markdown 围栏代码块的高亮）。

## 2. 语法高亮

### 2.1 用哪个 crate、语言集从哪来

用 **syntect**。pager 层不自己实现，直接复用 markdown crate 的类型：

```rust
// xai-grok-pager-render/src/syntax.rs:15
pub use xai_grok_markdown::Syntect;
```

`Syntect` 是一个 `{ theme, syntax_set }` 二元组（`xai-grok-markdown/src/syntax.rs:17-22`，注释标注 "supports 250+ languages via two-face"）。

语言集不是运行期加载，而是**编译期生成二进制 dump**：

```rust
// xai-grok-markdown/build.rs:12-21
let swift = SyntaxDefinition::load_from_str(include_str!("assets/Swift.sublime-syntax"), true, None)...;
let mut builder = two_face::syntax::extra_newlines().into_builder();
builder.add(swift);
let set = builder.build();
dump_to_uncompressed_file(&set, Path::new(&env::var("OUT_DIR").unwrap()).join("syntaxes.bin"))...;
```

即 `two-face` 提供的 Sublime syntax 全集 + 一份**打补丁的 Swift 语法**。运行期由 `from_uncompressed_data(include_bytes!(concat!(env!("OUT_DIR"), "/syntaxes.bin")))` 反序列化，并用 `OnceLock` 缓存（`xai-grok-markdown/src/syntax.rs:92-100`）。build.rs 的注释给出了动机：运行期 `SyntaxSet::build` 会拖慢首次 Read 绘制（`build.rs:9-11`）。

### 2.2 主题来源

三个内建 `.tmTheme`，位于 `xai-grok-pager-render/assets/`：`grok-night.tmTheme`、`grok-day.tmTheme`、`tokyo-night.tmTheme`（各约 40 KB）。每个主题一个 `OnceLock<Syntect>` 单例，按 `ThemeKind` 取：

```rust
// xai-grok-pager-render/src/syntax.rs:119-134（节选）
match crate::theme::Theme::current_kind() {
    ThemeKind::GrokNight | ThemeKind::RosePineMoon | ThemeKind::OscuraMidnight
    | ThemeKind::Terminal | ThemeKind::Auto
        => SYNTECT_GROKNIGHT.get_or_init(|| Syntect::new(include_bytes!("../assets/grok-night.tmTheme"))),
    ThemeKind::TokyoNight => SYNTECT_TOKYONIGHT.get_or_init(...tokyo-night.tmTheme),
    ThemeKind::GrokDay    => SYNTECT_GROKDAY.get_or_init(...grok-day.tmTheme),
}
```

用户手册把映射写成了对外承诺，且明确主题文件**不可替换**（`docs/user-guide/06-theming.md:164-172`）。注意 `Terminal` / `Auto` 也走 night 主题 —— 因为每个 token 的颜色后面会被重新映射（见 2.4），源调色板只需要“完整”，极性无关（`syntax.rs:123-126` 注释）。

### 2.3 语言/语法如何判定：只有两条路

1. **按文件扩展名**（`find_syntax_by_file_path`，`xai-grok-markdown/src/syntax.rs:37-43`）——`file_path.extension()` 直接丢给 `SyntaxSet::find_syntax_by_extension`。
2. **按语言 token**（`find_syntax_by_token`，同文件 `:46-51`）——用于 markdown 围栏的 info string，例如 ` ```rust `。
3. **围栏 info 里的行号引用**（`highlight_lines_for_fence_info` → `find_syntax_for_fence_info`，`:72-89`）：info 形如 `37:65:crates/.../read.rs` 时，取**第二个冒号之后的整段**当路径、按扩展名解析；解析不出语法再整串当 token 试一次。解析规则见 `parse_line_citation_fence_info`（`:118-133`），逐段校验前两段必须是纯 ASCII 数字。

**Swift 特例**：`is_swift_token`（`:102-104`）命中时不走 `SyntaxSet` 查找，而是 `patched_swift` —— 从 `syntaxes()` 反向找**最后一个**带 `.swift` 扩展名的语法，也就是 build.rs 追加的那份补丁版（`:106-114`）。

**未找到证据**的地方：全量检索 `shebang`、`find_syntax_by_first_line`、`sniff` 在 `xai-grok-pager-render/src/`、`xai-grok-markdown/src/`、`xai-grok-pager-diff/` 内**零命中**。判定入口只有“扩展名”和“显式 token”两类，没有 shebang 解析，也没有内容嗅探。

### 2.4 未知语言 / 高亮失败怎么办

两条路径的降级策略不同：

- **diff 路径**：`syntect.highlight_lines_by_file_path(path)` 返回 `Option<HighlightLines>`，未知扩展名即 `None`。`render_content_spans` 在 highlight 拿不到结果时退回纯色 fg（`scrollback/blocks/tool/edit.rs:706-734`）：

```rust
let style = match tag {
    ChangeTag::Equal => Style::default().fg(theme.diff_equal_fg),
    ChangeTag::Delete | ChangeTag::Insert => Style::default().fg(theme.text_primary),
};
spans.push(painted(content, style));
```

  同理 `xai-grok-pager-render/src/syntax.rs:90-115` 的 `highlight_line` 在 `highlighter` 为 `None` 或 `highlight_line()` 报错时，返回 `vec![Span::styled(text, fallback)]`。

- **markdown 路径**：未知 fence info 让 `syntax_highlight_raw` 返回 `None`，parse 退到 `ms.code_untagged`——**单色代码样式**，仍然有代码块背景，但没有语法色（`xai-grok-markdown/src/parse.rs:666-672`）。

```rust
if let Some(highlighted) = highlighted { self.buffers.replaces.push(...) }
else { self.push_highlight(Some(self.ms.code_untagged), &range); ... }
```

### 2.5 终端原生调色板下的“极性安全”映射

当 `theme::cache::terminal_native_active()` 成立（minimal 模式锁或 `Terminal` 主题），syntect 的 RGB **不再走过普通量化**，而是走一条专门的映射（`xai-grok-pager-render/src/syntax.rs:46-86`）：

- 低彩度（`chroma = max - min < 40`）的 token → `Color::Reset`，即继承终端默认前景色，两种极性下都可读；
- 有彩度的按 HSV 色相分桶映射到 **base ANSI-16**：`0..30 | 330..=360 → Red`、`30..90 → Yellow`、`90..150 → Green`、`150..210 → Cyan`、`210..255 → Blue`、其余 `Magenta`。注释专门说明 Magenta 从 255° 起算，是为了让 Tokyo Night 的紫 `#bb9af7`（约 261°）落到 Magenta 而不是 Blue；色相分桶的边界值有单元测试断言（`:197-205`）。
- 明确**禁止**使用 White/Black/Light*，因为它们在反极性上会消失；`:164-195` 的测试遍历常见 night 主题 pastel 色，断言输出永不落在这些槽位。

非终端原生路径下则走 `crate::theme::quantize(Color::Rgb(r,g,b))`（`syntax.rs:50`）。

## 3. 高亮的性能策略

### 3.1 首帧永远只高亮 hunk，不做全文件

`edit.rs:1-19` 的模块文档把成本模型写成了表格：

| 阶段 | 行为 |
| --- | --- |
| First paint | 只做 hunk 级；**绝不在 UI 线程做全文件** |
| Upgrade | 后台一次 syntect 走查，只走到最后一个 hunk 行；绘制时把样式映射叠加到普通 hunk 渲染上 |
| 超限或读取失败 | 保持 hunk-only，不做无界工作 |

“只走到最后一个 hunk 行”有明确实现：`max_needed = expected.keys().max()`，循环到该行即 `break`（`edit.rs:292-299`）。文档同时承认：靠近文件末尾的 hunk 仍要为整个文件付 O(file lines) 的代价，这正是 caps 存在的原因。

### 3.2 diff 内用**两个** highlighter，避免跨侧状态泄漏

```rust
// scrollback/blocks/tool/edit.rs:193-214（节选）
// A diff interleaves two file versions; give each side its own highlighter so a multi-line construct can't leak across sides
// Equal lines render on the new side and advance both
let mut old_highlighter = syntect.highlight_lines_by_file_path(path);
let mut new_highlighter = syntect.highlight_lines_by_file_path(path);
```

Delete 行只推进 old highlighter，Insert 只推进 new，Equal 用 new 渲染并**同时推进两者**（`advance_highlighter`，`edit.rs:677-685`）。这是把 syntect 的逐行状态机正确映射到“交错的旧/新两侧”的关键处理。

### 3.3 异步 worker：单线程 + mpsc + 按 entry 合并

`app/edit_highlight_worker.rs` 是主题里点名要看的文件。要点：

- **一个**名为 `"edit-hl"` 的 `std::thread`，mpsc 收发（`:98-124`），刻意仿照 `mermaid_worker` 的形态；
- 作业**按 `entry_id` 合流**，后者胜出，跨 key 保持 FIFO：

```rust
// app/edit_highlight_worker.rs:126-134
fn drain_coalesced(first: EditHlJob, rx: &Receiver<EditHlJob>) -> IndexMap<EntryId, EditHlJob> {
    let mut pending: IndexMap<EntryId, EditHlJob> = IndexMap::new();
    pending.insert(first.entry_id, first);
    while let Ok(job) = rx.try_recv() { pending.insert(job.entry_id, job); }
    pending
}
```

- 没有 waker，靠每个 tick `try_recv` 轮询（`:281-297`），`edit_hl_needs_tick()` 让事件循环在有待办时保持快 tick（`:199-205`）；
- 提交时先剪掉同 entry 的旧 pending（`rt.pending.retain(|(_, eid)| *eid != entry_id)`，`:243-245`），否则那些作业的结果永远不会到达、会把 fast tick 钉死；
- worker 线程死掉时 `abandon_edit_hl_worker` 把所有 `Pending` 回退成 `HunkOnly`，并丢弃 runtime 以便下次懒重建（`:346-364`）。注释承认这在 `panic=abort` 的发行构建里近乎不可达。

### 3.4 升级前必须验证“磁盘文本 == hunk 文本”

`compute_file_scoped_styles`（`edit.rs:279-331`）逐行比对：若磁盘第 `ln` 行经 tab 展开后与 hunk 里的文本不等，直接返回 `None`，**绝不用新内容覆盖屏幕上已显示的内容**。全部所需行都命中才返回 map，键是 1-based 新文件行号。

样式被烘焙在某个 `ThemeKind` 下，运行期换主题就不再用它：

```rust
// scrollback/blocks/tool/edit.rs:1012-1025
match &self.highlight {
    EditHighlightPhase::FileScoped { by_new_line, theme: baked }
        if *baked == crate::theme::cache::current_kind() => render_diff_hunks_with_styles(...),
    _ => render_diff_hunks_highlighted(...),
}
```

覆盖范围也有讲究（`map_spans_for_line`，`edit.rs:345-376`）：`Equal` 总是可覆盖；`Insert` **只在有底部色带（banded）的主题**上覆盖；`Delete` 从不覆盖 —— 保留用户已经看过的 per-hunk 配色。覆盖前再拼一次字符串做一致性检查（`joined != expanded` 就放弃）。

### 3.5 上限（caps）

```rust
// scrollback/blocks/tool/edit.rs:44-49
pub const EDIT_HL_MAX_BYTES: u64 = 2 * 1024 * 1024;   // 2 MiB
pub const EDIT_HL_MAX_LINES: usize = 50_000;
```

判定入口 `file_text_within_hl_caps`（`edit.rs:263-274`）；worker 侧另有 `read_file_capped` 在读之前按 `metadata().len()` 拦一次，读完再按实际字节数拦一次，并且要求 `String::from_utf8` 成功（`app/edit_highlight_worker.rs:174-188`）。README 风格的注释直接说明理由：全文件 syntect 对多 MB 源码是糟糕的后台工作，不如保持 hunk-only。

### 3.6 markdown 侧：流式高亮的两种缓存

`xai-grok-markdown/src/open_code_highlighter.rs:1-33` 记录了另一套（无关 diff 的）性能方案，同样值得参考：

- **仍在书写中的尾块**：跨 `rerender_tail` 持久化 syntect 的 `ParseState` / `HighlightState`，每行只高亮一次；否则每次 push 都重跑整块，是 O(N²)（注释给出的量级：约 1000 行块接近末尾时 ~35 ms/push）。
- **被困在 tail 里的闭合块**：按 `(fence_info, body)` 记忆批渲染结果，`CLOSED_MEMO_CAP_BYTES = 256 * 1024`（`:43-44`），超出即整体清空 —— 退化为重算，而不会无界增长。
- 缓存失效是“整块丢弃”：任何 theme/style/width 变化都让流式渲染器丢掉这个结构（`:19-21`）。

### 3.7 未找到证据的点

- **没有“只高亮可见区域”的裁剪**：diff 的渲染粒度是整块（`render_diff_hunks_core` 一次遍历所有 hunk），可见性裁剪由上层的 `max_lines` 与块高度预算承担，而不是高亮层。
- **没有 per-hunk / per-line 的高亮结果缓存**（rollback 除 `FileScoped` map 与流式 memo 外无其他缓存）。

## 4. diff 的数据结构（xai-grok-pager-diff）

### 4.1 类型极简：一行一格

```rust
// xai-grok-pager-diff/src/lib.rs:11-19
#[derive(Debug, Clone, PartialEq)]
pub struct DiffLine {
    pub text: String,
    pub lo: usize,          // 旧文件行号（1-based）
    pub ln: usize,          // 新文件行号（1-based）
    pub tag: ChangeTag,     // similar::{Equal, Delete, Insert}
}
pub type DiffHunk = Vec<DiffLine>;
```

`tag` 直接暴露 `similar::ChangeTag`，这是该 crate 与 `similar` 最紧的耦合点。crate 级 lint 是 `#![deny(clippy::indexing_slicing)]`（`:6`），全文用 `get`/`checked_sub`/`saturating_*` 访问 —— 这是它能在渲染热路径上安全吞掉畸形输入的原因。

### 4.2 与 `similar` 的关系

`similar` 只承担**行级 diff 算法**，其余全部自建：

| 用途 | 调用 |
| --- | --- |
| 逐行比较 | `TextDiff::from_lines(old, new)`（`lib.rs:71`） |
| 带超时的整文件比较 | `TextDiff::configure().timeout(...).diff_lines(...)`（`:170-172`） |
| 按上下文分组 | `diff.grouped_ops(MAX_CONTEXT)`（`:174`） |

`Cargo.toml` 里 `similar` 的注释就写着 “Line-level diffing (`TextDiff`) and the `ChangeTag` carried by every hunk line”。**没有**使用 `similar` 的 `unified_diff()` 输出器 —— 补丁文本是手写的（见 4.5）。

### 4.3 三条提取策略

`extract_edit_hunks(&ToolCall) -> (Vec<DiffHunk>, usize)`（`lib.rs:319-379`）依次尝试：

1. 从 `raw_output` 反序列化 `ToolOutput::SearchReplace(SearchReplaceOutput::EditsApplied(..))`，再 `build_diff_hunks(&edits.edits.details)`；失败时 `tracing::warn!` 后继续；
2. 从 ACP `ToolCallContent::Diff` 的 `diff.meta` 反序列化 `SearchReplaceEditContextInformation`（要求 `details` 非空）；
3. 退到 `old_text` / `new_text` 全文 diff，起始行号从 `meta.new_line` 取，缺省 1。

三条都失败则返回 `(vec![], 1)` —— 注意返回的 count 至少为 1。

### 4.4 上下文与分组的具体数值

```rust
// xai-grok-pager-diff/src/lib.rs:22-25
const MAX_CONTEXT: usize = 3;
const WHOLE_FILE_DIFF_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(500);
```

- 结构化路径（`build_diff_hunks`）：先用 `context_before` / `context_after` 包住改动，再按两端各留 `MAX_CONTEXT` 行裁剪（`:117-134`），最后把首尾**纯空白**的 Equal 行也裁掉（`:136-155`），全 Equal 的 hunk 直接丢弃（`:120-121`，测试 `no_change_produces_no_hunks`）。
- 全文路径（`diff_hunks_from_strings`）：注释说明 “Changes more than twice `MAX_CONTEXT` unchanged lines apart go into separate hunks, so a whole-file diff never shows the unchanged middle”（`:168`），由 `grouped_ops(MAX_CONTEXT)` 实现。
- 500 ms 超时的语义是“`similar` 返回更粗但仍正确的 diff”，不是失败（`:24` 注释）。
- `build_diff_hunks` 还处理三个边界：**空到空但确实插了一行**（补一个 `"\n"` 让 `TextDiff` 看得见，`:54-63`；而“空写空文件”必须产出零 diff 行，测试 `empty_file_write_produces_no_hunks`）；**行中匹配**的 `line_prefix`（把缩进补回被修改行，且只补第一条，`:64-88`）；`context_before` 末尾换行不产生幽灵空行（测试 `context_before_trailing_newline_no_phantom_line`）。

### 4.5 重叠 hunk 缝合与 patch 输出

`stitch_overlapping_hunks`（`lib.rs:207-219`）把同一文件的连续编辑折叠，遵循“宁可分开也不画错”的原则（`:204-206` 注释）。`stitch_hunk_pair` 的 bail 规则很细（`:247-315`）：坐标不单调、文本不一致、插入/删除不配对、覆盖范围内出现未配对的 Insert，**任一命中就返回 `None`**。

`diff_hunks_to_patch(path, hunks)`（`:383-431`）手写 unified diff：`--- a/{path}` / `+++ b/{path}`，每个 hunk 计算 `@@ -old_start,old_count +new_start,new_count @@`，行首前缀 `' '` / `'-'` / `'+'`。注释说明用途是 “suitable for `git apply` or clipboard sharing”。

### 4.6 与 `xai-hunk-tracker` 的关系：同技术、不同目的

任务里提到的 `src/hunk_tracker` 在快照中不存在；对应物是独立 crate `crates/codegen/xai-hunk-tracker/`。它**不复用** `xai-grok-pager-diff`，而是自己再调一次 `similar`，目的是给 hunk 做“谁改的”归属与 LOC 统计：

- `lib.rs:1-5`：`HunkTrackerActor` 独占一个 tokio 任务，调用方走 `HunkTrackerHandle`，事件走 `HunkEvent`；
- `diff.rs:13-20`：`CONTEXT_LINES = 3`、`DIFF_TIMEOUT = 10s`、`MAX_DIFF_FILE_SIZE = 1 MB`（注释写明是为避免病态 diff 行为）；
- `diff.rs:61-65`：这里**才**用到 `similar` 的 `unified_diff().context_radius(CONTEXT_LINES)`；
- `types.rs:47-66`：`HunkLineInfo { old_start, old_count, new_start, new_count }`，`Display` 输出 `@@ -a,b +c,d @@`；
- `types.rs:73-89`：`HunkSource::{AgentEdit{prompt_index}, ExternalEditOnAgentFile, External}`。

结论：**TUI 渲染只依赖 `xai-grok-pager-diff`；`xai-hunk-tracker` 是旁路，写代码时不要混用两者的 `Hunk` 类型。**

## 5. diff 的呈现设计

### 5.1 行布局与三个留白常量

```rust
// scrollback/blocks/tool/edit.rs:103-105
const INDENT: &str = "  ";        // 行号前的固定缩进
const GUTTER_GAP: &str = " ";     // 双列模式下旧/新行号之间
const CONTENT_GAP: &str = "  ";   // 行号与内容之间
```

单列模式（默认 `dual_line_numbers = false`；默认配置见 `edit.rs:90-100`）：

```
   列构成（单列）:
   ├ "  " 缩进 (INDENT)
   ├ 行号，右对齐；宽度 = hunk 内 max(lo, ln) 的十进制位数
   ├ "  " 内容前留白 (CONTENT_GAP)          ← 背景带从这里开始
   └ 内容（tab 已按 tab_width 展开）

    12  fn main() {
    13      let x = 1;      ← Delete：行号 = lo = 13，fg = diff_delete_fg，bg = diff_delete_bg
    13      let x = 42;     ← Insert：行号 = ln = 13，fg = diff_insert_fg，bg = diff_insert_bg
    14  }                    ← Equal： 行号 = ln = 14，fg = diff_gutter_fg，无背景
```

双列模式（`dual_line_numbers = true`，配置项注释写作 “like GitHub's unified diff”，`appearance/config.rs:392-394`）：

```
   列构成（双列）:
   ├ "  " 缩进
   ├ 旧行号(右对齐) + " " (GUTTER_GAP) + 新行号(右对齐)
   ├ "  " (CONTENT_GAP)
   └ 内容

    12  12  fn main() {      ← Equal：两列都写行号，皆 diff_gutter_fg
    13          let x = 1;   ← Delete：旧列 = 13 用 diff_delete_fg，新列打等宽空格
        13      let x = 42;  ← Insert：旧列打等宽空格，新列 = 13 用 diff_insert_fg
    14  14  }
```

**注意：整份 diff 渲染中没有任何 `+` / `-` 行首符号。**`render_gutter` 只输出数字或空格（`edit.rs:603-669`），`render_content_spans` 只输出原文 span（`:688-736`）。增删完全靠颜色与背景带区分，符号只出现在复制出去的 patch 文本里（4.5）。

gutter 宽度按 hunk 内容动态算：单列取 `max(lo, ln)` 的位数，双列分别取两列位数（`gutter_layout`，`edit.rs:568-601`）。

### 5.2 颜色与背景带

`Theme` 上有六个 diff 色槽（`groknight.rs:104-109`，值取自 `:30-50`）：

| 槽 | GrokNight 值 | 用途 |
| --- | --- | --- |
| `diff_delete_bg` | `rgb(66,14,20)` = `#420e14` | 删除行背景带 |
| `diff_delete_fg` | `rgb(247,118,142)` = `#f7768e` | 删除行号 / 无带主题的整行 fg |
| `diff_insert_bg` | `rgb(6,56,6)` = `#063806` | 新增行背景带 |
| `diff_insert_fg` | `rgb(158,206,106)` = `#9ece6a` | 新增行号 / 无带主题的整行 fg |
| `diff_equal_fg` | `COMMENT` = `#6c6c6c` | 上下文行文本 |
| `diff_gutter_fg` | `COMMENT` = `#6c6c6c` | 上下文行行号 |

背景**只覆盖内容区**：默认 `gutter_bg = false`，`compute_bg_start` 返回 `layout.total`（缩进 + 行号 + 留白的宽度），行号栏保持干净（`edit.rs:469-484`）。打开 `gutter_bg` 才让背景从行首开始，此时 `indent_bg` 决定是否跳过缩进列。`DiffLineOutput` 把 `background` 与 `content_start_col` 一起交给上层（`:107-121`），最终由 `BlockLine::with_background_from(bg, start_col)` 应用（`:1349-1352`）。

**无带主题走另一条路**：`diff_uses_line_fg()` 判定 `diff_delete_bg` 与 `diff_insert_bg` 都是 `Reset`（`theme/mod.rs:342-345`）。此时 Terminal default 主题（`terminal_default.rs:78-83`：两个 bg 都是 `Reset`，fg 是 `Red` / `Green`）的变更行**整行用红/绿前景色，并且跳过语法高亮**：

```rust
// scrollback/blocks/tool/edit.rs:697-704
if tag != ChangeTag::Equal && theme.diff_uses_line_fg() {
    let fg = match tag { ChangeTag::Delete => theme.diff_delete_fg, _ => theme.diff_insert_fg };
    spans.push(painted(content, Style::default().fg(fg)));
    return spans;
}
```

空白内容行会被 `painted()` 变成一个空格，保证色带可见且该行可被选中（`edit.rs:671-675`）。

### 5.3 hunk 之间的“上下文折叠”行

多个 hunk 之间不画整段上下文，而是一行分隔提示（`edit.rs:161-186`）：

```
  ⋯ 或 …
  … 9 unchanged lines        ← 居中文本：INDENT + hunk_separator + " " + N + " unchanged lines"
  … 1 unchanged line         ← 单数形式
```

`hunk_separator` 默认 `"…"`，可配 `"───"`（线）、`"⋯"`（中线省略号）、`""`（不画）；格式由 `DiffRenderConfig` 承载并在 `appearance/config.rs:389-391` 暴露给 `pager.toml`。行数由 `hunk_gap_lines(prev, next)` 计算，取“前一个 hunk 最后一个非 Delete 行的 `ln`”与“后一个 hunk 第一个非 Delete 行的 `ln`”之差减一（`:237-244`）；**算不出来时（比如两次调用各自针对不同文件快照）只画孤立的分隔符，不写行数**（`:164-172` 的 `None => config.hunk_separator.clone()`）。

分隔行 `is_separator = true`，不可选中，且会打断选区连续性（`:119-120`），在上层被转成 `BlockLine::separator` 并让选区 id 自增（`:1329-1334`）。

### 5.4 超长行

没有硬截断，只有**按空白分词的软换行**（`wrap_text`，`edit.rs:486-517`）：以空白为界切词，放不下就换行，`content_width = width - layout.total`。

换行后要把彩色 span 按词边界重新投影（`project_styles_onto_wrap_segments`，`:519-554`）：用单调游标只在本就存在的 span 边界或换行边界切分，拼接结果必须与换行段完全一致，否则返回 `None`，退化为“每段一个纯色 span”（`:422-445`）。续行的 gutter 用等宽空格填充，不重复行号（`:446-465`）。

### 5.5 有没有并排（side-by-side）模式

**没有。**全量检索 `side_by_side` / `sidebyside` / `side-by-side`（`*.rs` + `*.md`，整个 `crates/codegen/`）只命中三处无关代码：`scrollback/blocks/context_info.rs:248` 的注释、`views/welcome/hero_box.rs:13` 与 `views/welcome/mod.rs:317` 的 welcome 框布局。`DiffRenderConfig` 唯一的“双列”是 `dual_line_numbers` —— **双列行号，不是双列内容**（`edit.rs:85-87`）。

### 5.6 第二个呈现面：全屏 block viewer

`views/block_viewer/mod.rs:884-931` 复用同一个 `render_diff_lines`（因此高亮阶段、配色、分隔行完全一致），差别在：

- 固定用 `width = 500`（“wide width, NoWrap mode”）并 `DiffRenderConfig::default()`，所以**假设不换行、渲染行与 `DiffLine` 一一对应**（注释明确写出这个假设，`mod.rs:906-908`）；
- 额外维护与渲染行平行的 `diff_meta: Vec<Option<DiffLineMeta>>`，分隔行为 `None`；据此 `patch_from_range(path, range)` 能**按屏幕选区**生成 unified patch（`mod.rs:1231-1289`）。

## 6. scrollback 中的包围装饰

### 6.1 编辑块头部：`Edit <path> +N/-M`

头部行由 `prefix` + path 组成（`prefix` 例如 `"Edit "` / `"Creating "`，`edit.rs:757`），差分统计只出现在**折叠态**的一行摘要里（`edit.rs:943-972`）：

```rust
// 三 span："+3" / "/" / "-2"
Span::styled(format!(" +{ins}"), Style::default().fg(theme.diff_insert_fg)),
Span::styled("/", detail_style),
Span::styled(format!("-{del}"), Style::default().fg(theme.diff_delete_fg)),
```

即 `Edit src/main.rs +3/-2`。计数由 `compute_changes` 在 `set_hunks` 时预先算好（`:885-905`）。两种情况会**抑制**统计并退化成 ` (N edits)`：`summary_untrusted`（调用跨多文件，比如 `apply_patch` 每个文件一个 Diff，只有第一个成为 hunks，`:759-762` 注释）或 `show_summary` 关闭。`show_summary` 由 `effective_line_summary(...)` 决定：`pager.toml` 显式值优先，未设置则跟随 shell 的 `collapsed_edit_blocks` 标志（`appearance/config.rs:416-425`）。

### 6.2 折叠与行数预算

三种显示模式（`scrollback/types.rs` 的 `DisplayMode`）在编辑块上的分支（`edit.rs:1228-1367`）：

- `Collapsed`：**只渲染头部一行**，path span 可选；
- `Truncated` / `Expanded`：头部（可换行、带悬挂缩进 `wrap_edit_header`，`:1072-1212`）+ 一空行 + diff 正文；
- 之后统一施加 `ctx.max_lines` 预算：`lines.truncate(max)`（`:1357-1360`）。

头部自身以词为单位换行，并保留 path 的 span 位置用于选中与 OSC 8 链接（`:989-1000`）。错误信息以 muted 行插在头部与 diff 之间（`:1311-1320`）。

### 6.3 复制

- 编辑块整体复制 = unified patch：`copy_text()` → `diff_hunks_to_patch(&self.path, &self.hunks)`（`edit.rs:880-883`）；
- 选区复制**排除 gutter**：gutter 内的 span 不进入可选范围（`Selectable::Spans(gc..total)`，`:1336-1348`），每行的 `content_text` 单独保存，保证软换行的续行也能拼回原文；
- 全屏 viewer 支持按选区导出 patch（5.6）。

**未找到证据**：任何形如“已复制”“按 X 复制”的 UI 提示文案，也没有代码块的复制按钮/快捷键提示。剪贴板路径本身在其他专题的范围内（`keyboard-shortcuts` 文档提到 drag-copy 与 `y` 复制整块，见 `docs/user-guide/17-sessions.md:198`，但那是 session info 面板而非 diff）。

### 6.4 markdown 代码块：有背景，没有标签和边框

assistant 消息里的围栏代码块：

- 只用 `code_background` 一个样式字段（`xai-grok-markdown/src/style.rs:148`），在 pager 侧接到 `bg(theme.md_code_bg)`（`xai-grok-pager-render/src/theme/md_style.rs:145`）。GrokNight 的 `md_code_bg = rgb(28,28,28)`（`theme/groknight.rs:133`），Terminal default 是 `Reset`（`theme/terminal_default.rs:107`），即**没有**背景。
- **没有语言标签**：`CodeBlockSpan.info` 携带 fence info 原样字符串（`xai-grok-markdown/src/output.rs:26-30`），但 pager 里对 `code_blocks` 的唯一消费者是 mermaid 检测（`scrollback/blocks/mermaid_content.rs:96-97`），没有任何地方把 `info` 画成标签。
- **没有边框、没有行号、没有折叠控件**：只有一层背景色。
- 围栏行本身在 pretty 模式下被隐藏，且开围栏前会补一个空行，避免相邻块粘连（`xai-grok-markdown/src/render.rs:652-672`）；空的未收尾围栏（流式尾部）也会保住背景（同文件 `:1076` 注释）。
- ` ```mermaid ` 是特例：不画代码，改画图（`render.rs:1164` 注释 + `mermaid_content.rs`），走独立的 `mermaid_worker`。

### 6.5 Read 块的代码渲染（对照）

`tool/read.rs` 是“带行号的代码展示”最直接的参考：

```
   行构成： "{行号右对齐}{两个空格}" + highlight_line(...) 的 span
   gutter 样式 = theme.dim()，内容 fallback = theme.primary()

    41      fn main() {
    42          let x = 1;
    …
   498      }
```

- 行号宽度按 `base_line + raw_lines.len() - 1` 的位数算，`base_line` 来自读取范围起始（`read.rs:260-264`）；
- 整个面板加 `theme.bg_dark` 背景（`:300-331`）；
- `Truncated` 模式折叠为**首 5 行 + 一行 `…` + 末 3 行**（`FIRST_LINES = 5` / `LAST_LINES = 3`，`:16-17`；折叠逻辑 `:294-315`），阈值是 `total > first + last` 才折叠。这是本仓库里唯一“上下文折叠”的另一种形态 —— 对首尾取样，而不是对中间取样。

## 7. 边界与截断策略汇总

| 场景 | 行为 | 证据 |
| --- | --- | --- |
| 编辑后文件 > 2 MiB | 不做全文件高亮，保持 hunk-only | `tool/edit.rs:44-49,263-274` |
| 编辑后文件 > 50000 行 | 同上 | 同上 |
| 文件读不到 / 非 UTF-8 | worker 记录 debug 日志并回 `Failed`，界面保持 hunk-only | `app/edit_highlight_worker.rs:137-153,174-188` |
| 磁盘内容与 hunk 文本不一致 | `compute_file_scoped_styles` 返回 `None`，绝不改写已显示内容 | `tool/edit.rs:305-310,327-329` |
| 运行期切换主题 | 烘焙主题与当前不符则回退 hunk-only 渲染 | `tool/edit.rs:1012-1025` |
| 整文件 diff 计算慢 | `similar` 500 ms 超时 → 更粗但正确的 diff | `xai-grok-pager-diff/src/lib.rs:22-25,169-172` |
| diff 过大 | **未找到总行数/总字节的显式上限**；靠 hunk 粒度、`MAX_CONTEXT` 裁剪、`max_lines` 预算与后台 caps 共同约束 | `xai-grok-pager-diff/src/lib.rs:117-155`；`tool/edit.rs:1357-1360` |
| 二进制内容 | **未找到专门的二进制检测或占位渲染**。读工具对非文本走 `ReadMediaKind::{Image, Pdf}`（`tool/read.rs:20-27`），由图片/PDF 路径渲染；文本 diff 路径没有二进制分支 | `tool/read.rs:20-27` |
| hunk-tracker 侧 | 单文件 > 1 MB 跳过；diff 超 10 s 返回 `None` | `xai-hunk-tracker/src/diff.rs:16-20,32-41,50-57` |

## 8. 对本项目的可借用要点

1. **“廉价首帧 + 后台升级”**：先出 hunk 级高亮让首屏立即可用，再由单线程 worker 升级到全文件作用域，且升级前后做文本一致性校验 —— 这套模式对 Jev 混入的 agent 循环同样适用（任何“可能慢的增强”都该是可选升级而非阻塞）。
2. **合流键选 `entry_id` 而非 job 序号**：同一条目重复提交只保留最新作业，避免过期结果堆积把事件循环钉在快 tick 上。
3. **坐标双列（`lo` / `ln`）比“相对行号 + 偏移”更抗错**：缝合、选区、复制、patch 生成都直接读坐标，不需要重新推导。
4. **降级层级要显式**：syntect 失败 → 纯色；未知语言 → 单色代码样式；超 caps → hunk-only。每层都有明确触发条件，没有“静默出错”。
5. **颜色不是唯一表达手段的坑**：无带主题下变更行整行红/绿且放弃语法高亮，是一处刻意的取舍；若我们自己实现 diff，需要决定相同冲突下的优先级。

## 关键结论（供合稿引用）

1. 语法高亮用 **syntect**，语言集是 build 期由 `two-face::syntax::extra_newlines()` 加一份补丁 Swift 语法生成的 `syntaxes.bin`（`crates/codegen/xai-grok-markdown/build.rs:12-21`、`crates/codegen/xai-grok-markdown/src/syntax.rs:92-100`）。
2. 主题是三个内建 `.tmTheme`（night / day / tokyo-night），按 `ThemeKind` 走 `OnceLock` 单例，night 主题还被 RosePineMoon / OscuraMidnight / Terminal / Auto 共用（`crates/codegen/xai-grok-pager-render/src/syntax.rs:119-134`，手册 `docs/user-guide/06-theming.md:164-172`）。
3. 语言判定只有“文件扩展名”和“围栏 token / 行号引用”两条路，**没有 shebang 解析也没有内容嗅探**（`crates/codegen/xai-grok-markdown/src/syntax.rs:37-89`；相关关键词全量检索零命中）。
4. 未知语言分两条降级：diff 侧退回纯色前景（`crates/codegen/xai-grok-pager/src/scrollback/blocks/tool/edit.rs:729-733`），markdown 侧改用单色 `code_untagged` 样式（`crates/codegen/xai-grok-markdown/src/parse.rs:668-672`）。
5. 首帧只做 hunk 级高亮、全文件升级交给名为 `edit-hl` 的单独线程，作业按 `entry_id` 合流（later wins），结果靠每 tick `try_recv` 轮询（`crates/codegen/xai-grok-pager/src/app/edit_highlight_worker.rs:1-13,98-134,281-297`）。
6. 全文件高亮有两道硬上限：2 MiB 与 50000 行，超限即永久停在 hunk-only（`crates/codegen/xai-grok-pager/src/scrollback/blocks/tool/edit.rs:44-49`）。
7. diff 数据层极薄：`DiffLine { text, lo, ln, tag }` 与 `DiffHunk = Vec<DiffLine>`，`tag` 直接复用 `similar::ChangeTag`，上下文固定 3 行、整文件 diff 500 ms 超时（`crates/codegen/xai-grok-pager-diff/src/lib.rs:11-25`）。
8. diff 不画任何 `+` / `-` 行首符号，增删完全靠行号色 + 内容背景带区分；背景默认只覆盖内容区，行号栏保持干净（`crates/codegen/xai-grok-pager/src/scrollback/blocks/tool/edit.rs:103-105,469-484,603-669`）。
9. 行号栏可切换为 GitHub 式旧/新双列（`dual_line_numbers`），但这只是**行号**双列；**全仓没有并排内容（side-by-side）模式**（`crates/codegen/xai-grok-pager/src/scrollback/blocks/tool/edit.rs:85-87,568-669`；`side-by-side` 全量检索仅命中 welcome 页等无关处）。
10. hunk 之间的上下文用一行 `… N unchanged lines` 折叠表达，行数算不出来时只画孤立分隔符（`crates/codegen/xai-grok-pager/src/scrollback/blocks/tool/edit.rs:161-186,237-244`）。
11. 折叠头部一行摘要形如 `Edit src/main.rs +3/-2`，被判定为“摘要不可信”（多文件调用）时抑制统计（`crates/codegen/xai-grok-pager/src/scrollback/blocks/tool/edit.rs:943-972`）。
12. markdown 围栏代码块只有一层背景色（`md_code_bg`），**没有语言标签、边框、行号或折叠控件**，唯一消费者是 mermaid 检测（`crates/codegen/xai-grok-pager-render/src/theme/md_style.rs:145`、`crates/codegen/xai-grok-pager/src/scrollback/blocks/mermaid_content.rs:96-97`）；`xai-hunk-tracker` 是 diff 归属/LOC 的旁路，不参与 TUI 渲染（`crates/codegen/xai-hunk-tracker/src/lib.rs:1-5`、`src/diff.rs:13-20`）。
