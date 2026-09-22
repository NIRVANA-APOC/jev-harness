### 3.5 代码高亮与 diff 的呈现

### 本节要点

- 高亮用 **syntect**，语言集是 build 期由 `two_face::syntax::extra_newlines()` 加一份补丁版 Swift 语法合成的 `syntaxes.bin`，运行期 `OnceLock` 反序列化——不是运行期建字典。
- 语言判定只有两条路：**文件扩展名**与**显式语言 token**（含 `37:65:crates/…` 这种行号引用形式的围栏）；**没有 shebang 解析，也没有内容嗅探**。
- 首帧只高亮 hunk，全文件升级交给单独线程 `edit-hl`，作业按 `entry_id` 合流（later wins）；全文件高亮有 **2 MiB / 50000 行**两道硬上限，超限即永久停留在 hunk-only。
- diff 数据层极薄：`DiffLine { text, lo, ln, tag }` 与 `DiffHunk = Vec<DiffLine>`，上下文固定 3 行，整文件 diff 500 ms 超时后接受「更粗但仍正确」的结果。
- 呈现上**不画 `+` / `-` 行首符号**，增删只靠行号颜色与内容背景带；行号栏可切换为旧/新双列，但**全仓没有并排内容（side-by-side）模式**。
- 降级层级显式：syntect 失败 → 纯色前景；未知语言 → 单色代码样式；超 caps → hunk-only。没有「静默出错」这一档。

本节路径省略 `reference/grok-build/` 前缀；下文 `tool/edit.rs`、`tool/read.rs` 指 `crates/codegen/xai-grok-pager/src/scrollback/blocks/tool/` 下的同名文件。

#### 3.5.1 四层职责与两条平行线

代码分四层：**数据层** `crates/codegen/xai-grok-pager-diff/src/lib.rs`（单文件 crate，编辑工具输出 → 带行号的 `DiffHunk`，并能反向生成 unified patch）；**颜色层** `crates/codegen/xai-grok-pager-render/src/syntax.rs` 与 `src/theme/`（syntect 单例、`.tmTheme` 选择、RGB→终端色量化与极性安全映射、diff 六个色槽）；**渲染层** `tool/edit.rs`（3185 行，负责 gutter、背景色带、软换行、hunk 分隔行、头部 diffstat）；**异步层** `crates/codegen/xai-grok-pager/src/app/edit_highlight_worker.rs`（后台做「全文件作用域」高亮升级）。另有两条平行线：`crates/codegen/xai-hunk-tracker/`（diff 归属与 LOC 统计，**不参与 TUI 渲染**）与 `crates/codegen/xai-grok-markdown(-core)/`（assistant 消息里 markdown 围栏代码块的高亮）。

#### 3.5.2 语法高亮：crate、语言集与判定

- pager 层不自己实现高亮，直接复用 markdown crate 的类型：`xai-grok-pager-render/src/syntax.rs:15` 的 `pub use xai_grok_markdown::Syntect;`；`Syntect` 是一个 `{ theme, syntax_set }` 二元组（`xai-grok-markdown/src/syntax.rs:17-22`，注释标注 "supports 250+ languages via two-face"）。
- 语言集在 build 期生成：`xai-grok-markdown/build.rs:12-21` 用 `two_face::syntax::extra_newlines().into_builder()` 追加一份 `assets/Swift.sublime-syntax`，再 `dump_to_uncompressed_file` 成 `syntaxes.bin`；运行期 `from_uncompressed_data(include_bytes!(...))` + `OnceLock` 缓存（`xai-grok-markdown/src/syntax.rs:92-100`）。build.rs 注明动机：运行期 `SyntaxSet::build` 会拖慢首次 Read 绘制（`build.rs:9-11`）。那份 Swift 语法是**补丁版**：`is_swift_token`（`syntax.rs:102-104`）命中时不查 `SyntaxSet`，而是从 `syntaxes()` 反向找**最后一个**带 `.swift` 扩展名的语法（`:106-114`）。
- 主题是三个内建 `.tmTheme`（`grok-night` / `grok-day` / `tokyo-night`，各约 40 KB），按 `ThemeKind` 走 `OnceLock<Syntect>` 单例；**GrokNight、RosePineMoon、OscuraMidnight、Terminal、Auto 五种 kind 共用 night 主题**（`xai-grok-pager-render/src/syntax.rs:119-134`）。`Terminal` / `Auto` 也走 night，是因为每个 token 的颜色随后会被重新映射，源调色板只需「完整」、极性无关（同文件 `:123-126` 注释）。用户手册把映射写成对外承诺，并明确主题文件**不可替换**（`docs/user-guide/06-theming.md:164-172`）。
- 判定只有两条路（`xai-grok-markdown/src/syntax.rs:37-51`）：`find_syntax_by_file_path`（扩展名直接丢给 `SyntaxSet`）与 `find_syntax_by_token`（markdown 围栏 info string，如 ` ```rust `）。第三条是前者变体：围栏 info 形如 `37:65:crates/.../read.rs` 时走 `find_syntax_for_fence_info`（`:72-89`），取**第二个冒号之后的整段**当路径解析，解析不出再整串当 token 试一次；`parse_line_citation_fence_info`（`:118-133`）逐段校验前两段必须是纯 ASCII 数字。
- **未找到证据**：全量检索 `shebang`、`find_syntax_by_first_line`、`sniff` 在 `xai-grok-pager-render/src/`、`xai-grok-markdown/src/`、`xai-grok-pager-diff/` 内**零命中**——判定入口只有「扩展名」与「显式 token」两类。
- 终端原生调色板下另有一条「极性安全」映射（`xai-grok-pager-render/src/syntax.rs:46-86`）：低彩度 token（`chroma = max - min < 40`）→ `Color::Reset`；有彩度的按 HSV 色相分桶映射到 **base ANSI-16**（`0..30 | 330..=360 → Red`、`30..90 → Yellow`、`90..150 → Green`、`150..210 → Cyan`、`210..255 → Blue`、其余 `Magenta`）。Magenta 从 255° 起算是为了让 Tokyo Night 的紫 `#bb9af7`（约 261°）落到 Magenta 而不是 Blue；**明确禁止 White/Black/Light\***，因为它们在反极性上会消失，`:164-205` 的测试遍历常见 night 主题 pastel 色断言这一不变式。
- 高亮失败分两条降级：diff 侧 `highlight_lines_by_file_path` 返回 `None` 时退回纯色 fg（`tool/edit.rs:706-734`；`xai-grok-pager-render/src/syntax.rs:90-115` 的 `highlight_line` 同样返回 `vec![Span::styled(text, fallback)]`）；markdown 侧未知 fence info 让 `syntax_highlight_raw` 返回 `None`，parse 退到 `ms.code_untagged`——**单色代码样式，仍有代码块背景但没有语法色**（`xai-grok-markdown/src/parse.rs:666-672`）。

#### 3.5.3 性能：廉价首帧 + 后台升级

`tool/edit.rs:1-19` 的模块文档把成本模型写成了表；「只走到最后一个 hunk 行」有明确实现：`max_needed = expected.keys().max()`，循环到该行即 `break`（`:292-299`）。文档同时承认：靠近文件末尾的 hunk 仍要为整个文件付 O(file lines) 的代价，这正是 caps 存在的原因。

| 阶段 | 行为 |
| --- | --- |
| First paint | 只做 hunk 级；**绝不在 UI 线程做全文件** |
| Upgrade | 后台一次 syntect 走查，只走到最后一个 hunk 行；绘制时把样式映射叠加到普通 hunk 渲染上 |
| 超限或读取失败 | 保持 hunk-only，不做无界工作 |

- **diff 内用两个 highlighter**，避免跨侧状态泄漏：`syntect.highlight_lines_by_file_path(path)` 取两份（`tool/edit.rs:193-214`，注释 "A diff interleaves two file versions; give each side its own highlighter so a multi-line construct can't leak across sides"）；Delete 只推进 old、Insert 只推进 new、Equal 用 new 渲染并**同时推进两者**（`advance_highlighter`，`:677-685`）。这是把 syntect 的逐行状态机正确映射到「交错的旧/新两侧」的关键处理。
- worker 是**一个**名为 `"edit-hl"` 的 `std::thread`，mpsc 收发（`app/edit_highlight_worker.rs:98-124`），刻意仿照 `mermaid_worker` 的形态。作业**按 `entry_id` 合流**，后者胜出、跨 key 保持 FIFO（`drain_coalesced`，`:126-134`）；没有 waker，靠每个 tick `try_recv` 轮询（`:281-297`），`edit_hl_needs_tick()` 让事件循环在有待办时保持快 tick（`:199-205`）；提交时先剪掉同 entry 的旧 pending（`rt.pending.retain(|(_, eid)| *eid != entry_id)`，`:243-245`），否则那些作业的结果永远不会到达、会把 fast tick 钉死；worker 死掉时 `abandon_edit_hl_worker` 把所有 `Pending` 回退成 `HunkOnly` 并丢弃 runtime 以便下次懒重建（`:346-364`，注释承认这在 `panic=abort` 的发行构建里近乎不可达）。
- 升级前必须验证「磁盘文本 == hunk 文本」：`compute_file_scoped_styles`（`tool/edit.rs:279-331`）逐行比对，任一不等即返回 `None`，**绝不用新内容覆盖屏幕上已显示的内容**；全部所需行命中才返回 1-based 新文件行号 → 样式的 map。覆盖范围另有讲究（`map_spans_for_line`，`:345-376`）：`Equal` 总可覆盖；`Insert` **只在有底部色带（banded）的主题**上覆盖；`Delete` 从不覆盖——保留用户已经看过的 per-hunk 配色（覆盖前再拼一次字符串做一致性检查，`joined != expanded` 即放弃）。样式被烘焙在某个 `ThemeKind` 下，运行期换主题即弃用：只有 `baked == crate::theme::cache::current_kind()` 才走带样式的渲染，否则回退 `render_diff_hunks_highlighted`（`:1012-1025`）。
- 硬上限（`tool/edit.rs:44-49`）：`EDIT_HL_MAX_BYTES = 2 * 1024 * 1024`、`EDIT_HL_MAX_LINES = 50_000`；判定入口 `file_text_within_hl_caps`（`:263-274`）。worker 侧 `read_file_capped` 在读之前按 `metadata().len()` 拦一次、读完再按实际字节数拦一次，并要求 `String::from_utf8` 成功（`app/edit_highlight_worker.rs:174-188`）。注释给的理由很直接：全文件 syntect 对多 MB 源码是糟糕的后台工作，不如保持 hunk-only。
- markdown 侧另有一套流式缓存（`xai-grok-markdown/src/open_code_highlighter.rs:1-33`）：仍在书写中的尾块跨 `rerender_tail` 持久化 syntect 的 `ParseState` / `HighlightState`，每行只高亮一次（否则每次 push 重跑整块，是 O(N²)；注释给的量级是约 1000 行块接近末尾时 ~35 ms/push）；被困在 tail 里的闭合块按 `(fence_info, body)` 记忆批渲染结果，`CLOSED_MEMO_CAP_BYTES = 256 * 1024`（`:43-44`），超出即整体清空——退化为重算而不会无界增长；任何 theme/style/width 变化都让流式渲染器整块丢弃（`:19-21`）。
- **未找到证据**：没有「只高亮可见区域」的裁剪（diff 渲染粒度是整块，可见性裁剪由上层 `max_lines` 与块高度预算承担）；也没有 per-hunk / per-line 的高亮结果缓存（`FileScoped` map 与流式 memo 之外无其他缓存）。

#### 3.5.4 diff 数据层：一行一格，上下文 3 行

- 类型极简（`xai-grok-pager-diff/src/lib.rs:11-19`）：`DiffLine { text, lo, ln, tag }`——`lo` / `ln` 是旧/新文件 1-based 行号，`tag` 直接复用 `similar::ChangeTag`；`pub type DiffHunk = Vec<DiffLine>`。crate 级 lint 是 `#![deny(clippy::indexing_slicing)]`（`:6`），全文用 `get` / `checked_sub` / `saturating_*` 访问——这是它能在渲染热路径上安全吞掉畸形输入的原因。
- `similar` 只承担**行级 diff 算法**，其余全部自建：逐行比较 `TextDiff::from_lines`（`:71`）、带超时的整文件比较 `TextDiff::configure().timeout(...).diff_lines(...)`（`:170-172`）、按上下文分组 `diff.grouped_ops(MAX_CONTEXT)`（`:174`）。**没有**使用 `similar` 的 `unified_diff()` 输出器——补丁文本是手写的。
- 关键常量（`:22-25`）：`MAX_CONTEXT = 3`、`WHOLE_FILE_DIFF_TIMEOUT = 500 ms`。超时的语义是「`similar` 返回更粗但仍正确的 diff」，不是失败（`:24` 注释）；注释另说明「相隔超过两倍 `MAX_CONTEXT` 的未改动行会分进不同 hunk，所以整文件 diff 永不显示中间未改动部分」（`:168`）。
- 三条提取策略依次尝试（`extract_edit_hunks`，`:319-379`）：① 从 `raw_output` 反序列化 `ToolOutput::SearchReplace(SearchReplaceOutput::EditsApplied(..))` 再 `build_diff_hunks`（失败 `tracing::warn!` 后继续）；② 从 ACP `ToolCallContent::Diff` 的 `diff.meta` 反序列化 `SearchReplaceEditContextInformation`（要求 `details` 非空）；③ 退到 `old_text` / `new_text` 全文 diff，起始行号取 `meta.new_line`、缺省 1。三条都失败返回 `(vec![], 1)`——注意返回的 count 至少为 1。
- 结构化路径的三个边界：**空到空但确实插了一行**（补一个 `"\n"` 让 `TextDiff` 看得见，`:54-63`；而「空写空文件」必须产出零 diff 行，测试 `empty_file_write_produces_no_hunks`）；**行中匹配**的 `line_prefix`（把缩进补回被修改行，且只补第一条，`:64-88`）；`context_before` 末尾换行不产生幽灵空行（测试 `context_before_trailing_newline_no_phantom_line`）。此外先用 `context_before` / `context_after` 包住改动，再按两端各留 `MAX_CONTEXT` 行裁剪（`:117-134`），最后把首尾**纯空白**的 Equal 行也裁掉（`:136-155`），全 Equal 的 hunk 直接丢弃（`:120-121`）。重叠 hunk 缝合 `stitch_overlapping_hunks`（`:207-219`）遵循「宁可分开也不画错」；`stitch_hunk_pair` 的 bail 规则很细（`:247-315`）：坐标不单调、文本不一致、插入/删除不配对、覆盖范围内出现未配对的 Insert，**任一命中即返回 `None`**。
- `diff_hunks_to_patch(path, hunks)`（`:383-431`）手写 unified diff：`--- a/{path}` / `+++ b/{path}`，每个 hunk 计算 `@@ -old_start,old_count +new_start,new_count @@`，行首前缀 `' '` / `'-'` / `'+'`；注释写用途是 "suitable for `git apply` or clipboard sharing"。
- **不要与 `xai-hunk-tracker` 混用**：它是独立 crate，**不复用** `xai-grok-pager-diff` 而是自己再调一次 `similar`，目的是给 hunk 做「谁改的」归属与 LOC 统计（`HunkTrackerActor` 独占一个 tokio 任务、调用方走 `HunkTrackerHandle`、事件走 `HunkEvent`，`xai-hunk-tracker/src/lib.rs:1-5`；`CONTEXT_LINES = 3`、`DIFF_TIMEOUT = 10s`、`MAX_DIFF_FILE_SIZE = 1 MB`，`src/diff.rs:13-20`）。它**才**用到 `similar` 的 `unified_diff().context_radius(CONTEXT_LINES)`（`src/diff.rs:61-65`），并用 `HunkSource::{AgentEdit{prompt_index}, ExternalEditOnAgentFile, External}` 标注来源（`src/types.rs:73-89`）。**TUI 渲染只依赖 `xai-grok-pager-diff`**，两者的 `Hunk` 类型不要混用。

#### 3.5.5 呈现：行号栏 + 背景带，没有符号

三个留白常量（`tool/edit.rs:103-105`）：`INDENT = "  "`（行号前固定缩进）、`GUTTER_GAP = " "`（双列模式下旧/新行号之间）、`CONTENT_GAP = "  "`（行号与内容之间）。

```
   单列（默认 dual_line_numbers = false）：
     INDENT  行号(右对齐)  CONTENT_GAP  内容        ← 背景带从 CONTENT_GAP 起
     12  fn main() {
     13      let x = 1;      ← Delete：行号 = lo = 13，fg = diff_delete_fg，bg = diff_delete_bg
     13      let x = 42;     ← Insert：行号 = ln = 13，fg = diff_insert_fg，bg = diff_insert_bg
     14  }                   ← Equal： 行号 = ln = 14，fg = diff_gutter_fg，无背景

   双列（dual_line_numbers = true，配置项注释写作 “like GitHub's unified diff”）：
     INDENT  旧行号  GUTTER_GAP  新行号  CONTENT_GAP  内容
     12  12  fn main() {     ← Equal：两列都写行号，皆 diff_gutter_fg
     13          let x = 1;  ← Delete：旧列 = 13 用 diff_delete_fg，新列打等宽空格
         13      let x = 42; ← Insert：旧列打等宽空格，新列 = 13 用 diff_insert_fg
```

- **整份 diff 渲染中没有任何 `+` / `-` 行首符号**：`render_gutter` 只输出数字或空格（`tool/edit.rs:603-669`），`render_content_spans` 只输出原文 span（`:688-736`）；增删完全靠颜色与背景带区分，符号只出现在复制出去的 patch 文本里。gutter 宽度按 hunk 内容动态算：单列取 `max(lo, ln)` 的十进制位数，双列分别取两列位数（`gutter_layout`，`:568-601`）。
- 六个 diff 色槽（`xai-grok-pager-render/src/theme/groknight.rs:104-109`，值取 `:30-50`）：`diff_delete_bg = #420e14`（删除行背景带）、`diff_delete_fg = #f7768e`（删除行行号 / 无带主题下的整行 fg）、`diff_insert_bg = #063806`、`diff_insert_fg = #9ece6a`，上下文行的文本与行号都取 `COMMENT = #6c6c6c`（`diff_equal_fg` / `diff_gutter_fg`）。

- 背景**默认只覆盖内容区**：`gutter_bg = false` 时 `compute_bg_start` 返回 `layout.total`（缩进 + 行号 + 留白的宽度），行号栏保持干净（`tool/edit.rs:469-484`）；打开 `gutter_bg` 才让背景从行首开始，`indent_bg` 决定是否跳过缩进列。`DiffLineOutput` 把 `background` 与 `content_start_col` 一起交给上层（`:107-121`），最终由 `BlockLine::with_background_from(bg, start_col)` 应用（`:1349-1352`）。
- **无带主题走另一条路**：`diff_uses_line_fg()` 判定 `diff_delete_bg` 与 `diff_insert_bg` 都是 `Reset`（`xai-grok-pager-render/src/theme/mod.rs:342-345`）。此时 Terminal default 主题（`theme/terminal_default.rs:78-83`）的变更行**整行用红/绿前景色，并且跳过语法高亮**（`tool/edit.rs:697-704`）。这是一处刻意的取舍：当颜色成为唯一区分手段时，语法高亮让位——若自己实现 diff，需要先决定相同冲突下的优先级。空白内容行会被 `painted()` 变成一个空格，保证色带可见且该行可被选中（`:671-675`）。

#### 3.5.6 hunk 分隔、软换行、折叠与复制

- hunk 之间不画整段上下文，而是一行分隔提示（`tool/edit.rs:161-186`）：`… 9 unchanged lines`（单数为 `… 1 unchanged line`）。`hunk_separator` 默认 `"…"`，可配 `"───"`（线）、`"⋯"`（中线省略号）、`""`（不画）；格式由 `DiffRenderConfig` 承载并在 `appearance/config.rs:389-391` 暴露给 `pager.toml`。行数由 `hunk_gap_lines(prev, next)` 计算：前一个 hunk 最后一个非 Delete 行的 `ln` 与后一个 hunk 第一个非 Delete 行的 `ln` 之差减一（`:237-244`）；**算不出来时（例如两次调用各自针对不同文件快照）只画孤立分隔符，不写行数**（`:164-172` 的 `None => config.hunk_separator.clone()`）。分隔行 `is_separator = true`，不可选中且会打断选区连续性（`:119-120`）。
- 超长行没有硬截断，只有**按空白分词的软换行**（`wrap_text`，`tool/edit.rs:486-517`），`content_width = width - layout.total`。换行后要把彩色 span 按词边界重新投影（`project_styles_onto_wrap_segments`，`:519-554`）：用单调游标只在本就存在的 span 边界或换行边界切分，拼回结果必须与换行段完全一致，否则返回 `None`，退化为「每段一个纯色 span」（`:422-445`）。续行的 gutter 用等宽空格填充，不重复行号（`:446-465`）。
- 三种显示模式（`DisplayMode`）在编辑块上的分支（`tool/edit.rs:1228-1367`）：`Collapsed` **只渲染头部一行**；`Truncated` / `Expanded` 是头部（可换行、带悬挂缩进 `wrap_edit_header`，`:1072-1212`）+ 一空行 + diff 正文；之后统一施加 `ctx.max_lines` 预算（`lines.truncate(max)`，`:1357-1360`），错误信息以 muted 行插在头部与 diff 之间（`:1311-1320`）。折叠态头部摘要形如 `Edit src/main.rs +3/-2`，由三个 span 组成（` +N` 用 `diff_insert_fg`、`/` 用中性、`-M` 用 `diff_delete_fg`，`:943-972`）；两种情况会**抑制**统计并退化成 ` (N edits)`：`summary_untrusted`（调用跨多文件，比如 `apply_patch` 每个文件一个 Diff、只有第一个成为 hunks，`:759-762` 注释）或 `show_summary` 关闭——后者由 `effective_line_summary(...)` 决定：`pager.toml` 显式值优先，未设置则跟随 shell 的 `collapsed_edit_blocks` 标志（`appearance/config.rs:416-425`）。
- 复制与导出：编辑块整体复制 = unified patch（`copy_text()` → `diff_hunks_to_patch`，`:880-883`）；选区复制**排除 gutter**（`Selectable::Spans(gc..total)`，`:1336-1348`），每行的 `content_text` 单独保存，保证软换行的续行也能拼回原文。全屏 block viewer（`crates/codegen/xai-grok-pager/src/views/block_viewer/mod.rs:884-931`）复用同一个 `render_diff_lines`，因此高亮阶段、配色、分隔行完全一致；差别是固定 `width = 500` 并 `DiffRenderConfig::default()`，所以**假设不换行、渲染行与 `DiffLine` 一一对应**（注释明写这个假设，`:906-908`），并额外维护与渲染行平行的 `diff_meta: Vec<Option<DiffLineMeta>>`（分隔行为 `None`），据此 `patch_from_range(path, range)` 能**按屏幕选区**生成 unified patch（`:1231-1289`）。**未找到证据**：任何形如「已复制」「按 X 复制」的 UI 提示文案；代码块也没有复制按钮或快捷键提示。
- **没有并排（side-by-side）模式**：全量检索 `side_by_side` / `sidebyside` / `side-by-side`（`*.rs` + `*.md`，整个 `crates/codegen/`）只命中三处无关代码（`scrollback/blocks/context_info.rs:248` 注释、welcome 框布局 `views/welcome/hero_box.rs:13` 与 `views/welcome/mod.rs:317`）。`DiffRenderConfig` 唯一的「双列」是 `dual_line_numbers`——**双列行号，不是双列内容**（`tool/edit.rs:85-87`）。

#### 3.5.7 markdown 代码块与 Read 块（对照）

- assistant 消息里的围栏代码块只有一层背景色：`code_background` 一个样式字段（`xai-grok-markdown/src/style.rs:148`），在 pager 侧接到 `bg(theme.md_code_bg)`（`xai-grok-pager-render/src/theme/md_style.rs:145`）。GrokNight 的 `md_code_bg = rgb(28,28,28)`（`theme/groknight.rs:133`），Terminal default 是 `Reset`（`theme/terminal_default.rs:107`），即**没有**背景。**没有语言标签**：`CodeBlockSpan.info` 携带 fence info 原样字符串（`xai-grok-markdown/src/output.rs:26-30`），但 pager 里对 `code_blocks` 的唯一消费者是 mermaid 检测（`scrollback/blocks/mermaid_content.rs:96-97`），没有任何地方把 `info` 画成标签；也**没有边框、行号或折叠控件**。围栏行本身在 pretty 模式下被隐藏，且开围栏前补一个空行避免相邻块粘连（`xai-grok-markdown/src/render.rs:652-672`）；空的未收尾围栏（流式尾部）也会保住背景（同文件 `:1076` 注释）。` ```mermaid ` 是特例：不画代码、改画图（`render.rs:1164` 注释 + `mermaid_content.rs`），走独立的 `mermaid_worker`。
- `tool/read.rs` 是「带行号的代码展示」最直接的参考：行构成 = `{行号右对齐}{两个空格}` + `highlight_line(...)` 的 span，gutter 样式为 `theme.dim()`、内容 fallback 为 `theme.primary()`；行号宽度按 `base_line + raw_lines.len() - 1` 的位数算（`base_line` 来自读取范围起始，`:260-264`）；整个面板加 `theme.bg_dark` 背景（`:300-331`）。`Truncated` 模式折叠为**首 5 行 + 一行 `…` + 末 3 行**（`FIRST_LINES = 5` / `LAST_LINES = 3`，`:16-17`；折叠逻辑 `:294-315`），阈值是 `total > first + last`——这是本仓库里唯一对**首尾取样**（而非取中间）的上下文折叠。

#### 3.5.8 边界与截断（汇总）

| 场景 | 行为 | 证据 |
| --- | --- | --- |
| 编辑后文件 > 2 MiB 或 > 50000 行 | 不做全文件高亮，保持 hunk-only | `tool/edit.rs:44-49,263-274` |
| 文件读不到 / 非 UTF-8 | worker 记 debug 日志并回 `Failed`，界面保持 hunk-only | `app/edit_highlight_worker.rs:137-153,174-188` |
| 整文件 diff 计算慢 | `similar` 500 ms 超时 → 更粗但仍正确的 diff | `xai-grok-pager-diff/src/lib.rs:22-25,169-172` |
| diff 过大 | **未找到总行数 / 总字节的显式上限**；靠 hunk 粒度、`MAX_CONTEXT` 裁剪、`max_lines` 预算与后台 caps 共同约束 | `xai-grok-pager-diff/src/lib.rs:117-155`；`tool/edit.rs:1357-1360` |
| 二进制内容 | **未找到专门的二进制检测或占位渲染**；读工具对非文本走 `ReadMediaKind::{Image, Pdf}`（由图片 / PDF 路径渲染），文本 diff 路径没有二进制分支 | `tool/read.rs:20-27` |
| hunk-tracker 侧 | 单文件 > 1 MB 跳过；diff 超 10 s 返回 `None` | `xai-hunk-tracker/src/diff.rs:16-20,32-41,50-57` |

---

### 3.6 动效、加载态与流式呈现

### 本节要点

- 不是「一个动画引擎」：**三条独立时钟**（动画 tick `1000 / fps`、最小绘制间隔 `min_draw_interval`、滚动节奏 `scroll_cadence`）加一个**有序的 `TickDemand` 需求门控**；空闲时事件循环完全不 arm tick，即**零唤醒**。
- **真正逐帧的动画只有四类**：spinner 帧序列、运行中块的 accent 幅度波形、脉冲式亮度（等待菱形 / 欢迎页 logo）、dashboard 与状态栏的换帧。其余全部是「定时切换或顺序出现」。
- spinner 统一走 `tick / DIVISOR` 降帧，事实标准 `SPINNER_DIVISOR = 4`（30 fps 下约 133 ms/帧、约 7.5 fps），不是每 tick 换帧。
- 流式文本**没有逐 token 重绘，也没有打字机**：单批最多 drain 32 条 ACP 消息，绘制受 16 ms 节流 + 在途水位闸门约束，且只有视口内的 running 条目才要求重绘——屏幕上表现为「按帧吞吐的整段增长」。
- 等待态的语言是「一个 spinner + 精确命名的原因 + 颜色语义」，7 种 `WaitingReason` 各有文案；短等待用 300 ms 延迟门隐藏，杜绝闪烁。
- **代码里不存在 reduce-motion 概念**，也没有「关闭动画」开关；降级靠配置 fps、按显示器刷新率自动定档、离屏不驱动、minimal 模式收紧与老终端字形替换五条路径。

本节路径省略 `reference/grok-build/` 前缀。

#### 3.6.1 调度模型：一个 tick 计数器 + 五层「帧预算」

| 时钟 | 默认值 | 用途 | 证据 |
| --- | --- | --- | --- |
| 动画 tick | `1000 / animation.fps`，fps 默认 30（clamp 1..60） | spinner 换帧、波形相位、提示条 TTL | `crates/codegen/xai-grok-pager-render/src/appearance/config.rs:335-362`、`:1282` |
| 最小绘制间隔 `min_draw_interval` | 16 ms（~60 Hz）；探测到高刷屏时降到 8 ms | 单帧绘制节流 | `crates/codegen/xai-grok-shell/src/util/config/resolve/display_refresh.rs:11-12`、`:19-22`、`:57` |
| 滚动时钟 `scroll_cadence` | 默认同为 16 ms | 滚轮 / 触控板残余行冲刷 | `crates/codegen/xai-grok-pager/src/app/event_loop.rs:1454-1461`、`:1834-1836`、`:2394-2403` |

```rust
// src/app/event_loop.rs:3052-3067
let interval = match app.tick_demand() {
    TickDemand::None => return,                              // 完全不 arm，事件循环 park
    TickDemand::Fast => interval.min(tick_interval_ceiling()), // /gboom 33ms、minimal 16ms
    TickDemand::Slow => interval.max(SLOW_TICK_INTERVAL),     // 83ms ≈ 12fps
};
```

- `TickDemand` 是**有序枚举** `None < Slow < Fast`（`src/app/app_view.rs:244-257`）；`SLOW_TICK_INTERVAL = 83ms` 与欢迎页 logo 的 `SHIMMER_FPS = 12` 对齐；`Fast` 由约 40 个条件触发（运行中 agent、滚动回放、模态 spinner、待发取消、语音录音……`src/app/app_view.rs:5636-5784`）。
- 「帧预算」没有单一常量，而是五层机制叠加：① **需求门控**（`TickDemand::None` 直接不排 tick，`event_loop.rs:3053-3056`）；② **绘制节流** `Presenter::request_throttled(now, min_draw_interval)`，不够间隔就把绘制排到 `last_draw_at + min_draw_interval`（`event_loop.rs:606-615`）；③ **在途闸门** `in_flight_target` 记录已排队 escape 序列的序号水位，上一帧未写完就不再画（`event_loop.rs:498-515`、`:579-600`）；④ **视口作用域**，只有视口内的 running 条目才要求重绘；⑤ **除法降帧**，所有 spinner 都是 `tick / DIVISOR`。

#### 3.6.2 真的逐帧：四类动画

- **① spinner 家族统一 `tick / 4`。** 全代码库的事实标准是 `SPINNER_DIVISOR = 4`，注释明确写着「降低了速度」（`src/views/turn_status.rs:29-31`：*At ~30fps, 4 ticks is ~133ms per frame, about 7.5 spinner fps*）。使用点包括 turn status 行（`turn_status.rs:367-377`）、MCP chip（`views/agent_status.rs:22`、`:343-355`）、dock 活动行（`views/dock/mod.rs:14`、`:404-406`）、`/btw` overlay（`views/btw_overlay.rs:151-152`、`:356-359`）、extensions 模态（`views/extensions_modal.rs:2760`）、tasks pane（`views/tasks_pane.rs:32`）、dashboard 行（`views/dashboard/animation.rs:5`）；goal chip 是硬编码 `/ 4`、用 dot 8 帧（`views/agent_status.rs:313-319`）。帧集定义在 `crates/codegen/xai-grok-pager-render/src/glyphs.rs:148-169`（braille 8 帧 U+280B…U+2827 与 dot spinner），**每帧都保证 1 列宽，避免布局跳动**。
- **② 幅度调制式的两处脉冲。** 监控 / 后台工作脉冲 `○ ◎ ◉ ◎`：`MONITOR_PULSE_DIVISOR = 8`，约 3.75 fps、约 1.07 s 一循环，理由是「空闲的 still-running 提示应该安静地呼吸，而不是读成活跃 turn 的 spinner」（`src/views/turn_status.rs:33-36`、`:267-275`）。等待用户操作的菱形脉冲不用帧表，而是**按 tick 计算颜色亮度**，在 `bg_base` 与 accent 之间 blend `0.3 + brightness * 0.7`，速度常量 `USER_WAITING_PULSE_SPEED = 0.08`，并有专门测试锁死该值（`turn_status.rs:38-47`、`:1575-1578`）；周期按注释为 `π / (speed · fps)`（`xai-grok-pager-render/src/theme/tokyonight.rs:358-363`）。**③ 运行中块的 accent 竖条波形**（最典型的逐帧动画）是逐行相位偏移的 `sin²` 亮度波，`WAVE_SPEED = 0.15` rad/tick、「约 40 tick 走完一个块」；`wave_rows` 默认 32 行一个波周期，是用户可配项（`src/scrollback/wrappers/entry_renderer.rs:19-21`、`:721-732`），运行中块的 bullet 用同一波形（`:857-867`）。
- **④ 换帧与墙钟驱动的两处。** 标题栏 spinner `TITLE_SPINNER_DIVISOR = 8` ≈264 ms/帧，理由写在注释里——终端（尤其 Ghostty）会对标签标题做 debounce，每 tick（~33 ms）写一次 OSC 0 会超过标签栏的渲染能力（`src/notifications/title.rs:12-15`、`:92-94`）；`⚠ Action Required` 闪烁用 `ACTION_REQUIRED_BLINK_DIVISOR = 15`，30 fps 下约 500 ms 亮 / 500 ms 灭 = 1 s 周期，**窗口聚焦时不闪**、改为静态前缀以免打字时干扰（`title.rs:17-20`、`:184-190`）。**刻意脱离 tick 的两处**：欢迎页 logo 用 `SHIMMER_FPS = 12` 墙钟相位算帧号，一次对角光泽扫过 `CYCLE = 4.0s`（其中 `SWEEP_FRAC = 0.32` 用于扫动，其余停在屏外），底下叠 `PULSE = 0.06`、`PULSE_SECS = 5.0` 的呼吸（`src/views/welcome/logo.rs:106`、`:109-140`）；语音录音红点固定 0.7 s 墙钟正弦「呼吸」，注释明确「不随动画 tick，也不与流式文本重绘同步」（`src/app/agent_view/mod.rs:216-227`）。dashboard 不按 tick 盲翻帧，而是由渲染阶段回填 `PaintedAnimations`，`tick()` 只在真正的帧边界返回 `true`（`SPINNER_DIVISOR = 4`、`NEEDS_INPUT_BLINK_DIVISOR = 10`）；文件首行注释即 *A projected row that was not painted does not count.*（`src/views/dashboard/animation.rs:1-64`）。
- **另一条独立通道：把加载态交给终端外壳。** 仅对 Ghostty、WezTerm、iTerm2 ≥ 3.6 发送 `OSC 9;4` 不确定进度（`src/notifications/progress.rs:10-21`），tmux 下自动包 DCS passthrough（`:34-38`）；因 Ghostty 约 15 s 无消息会重置，按 `PROGRESS_KEEPALIVE = 5s` 重发（`src/notifications/mod.rs:14-16`、`:151-166`）。它不是帧动画。

状态行布局（活跃期几乎全部视觉层次集中在这一行）：

```
⠧ Run cargo test 0.2s              1m20s ⇣12k [↓] [stop]
│  │               │                │      │    │    └ 取消按钮（hover 变红）
│  │               │                │      │    └ 后台按钮（仅运行中的 execute，hover 展开）
│  │               │                │      └ turn 计时 + token（右对齐，灰）
│  │               └ 阶段计时（灰，永不截断；Ask 工具时隐藏）
│  └ 活动标签（按类型着色，可截断）
└ spinner（braille，tick/4）；工具阻塞在权限/提问时换成脉冲 ◆
```

（布局与顺序来自模块头注释与渲染顺序：`src/views/turn_status.rs:3-13`、`:399-417`、`:520-570`；「Ask 工具隐藏阶段计时」在 `:379-394`。）

#### 3.6.3 明确不是动画的部分（避免夸大）

- **进度条与滚动条都是静态的**：`views/progress_bar.rs` 提供 1/8 单元精度的 `▏▎▍▌▋▊▉█` 渲染（`:1-54`、`:59-91`），但只有两个调用点——上下文占用条（`src/views/context_bar.rs:11`、`:178-189`）与 goal 进度条（`src/views/goal_detail.rs:624`），两处都不持有 tick、值变了才重画；滚动条按 follow 模式提供两档静态样式（跟随=很暗，滚上去=较亮），无 tick / 相位 / 渐变（`xai-grok-pager-render/src/render/scrollbar.rs:6-7`、`:185-208`），minimal 模式下整体隐藏（`:42-55`）。
- **完成闪光不是渐变**：`FINISH_FLASH_DURATION_MS = 400`（`src/scrollback/state/types.rs:75`）期间把已完成块的 accent 保持为「静态成功色」（`entry_renderer.rs:680-704`），过期时补一次重绘恢复原色（`src/scrollback/state/mod.rs:427-450`）。是定时切换 + 一次收尾重绘，**不是颜色插值**。
- **提示条是 TTL 倒计时，不是动画**：默认 90 tick ≈ 3 s（`src/tips/ephemeral.rs:7-10`，每 tick 递减 `:103-116`）；SSH 提示 300 tick ≈ 10 s（`src/tips/ssh_wrap.rs:22-24`）；word_select 提示 600 tick ≈ 20 s（`src/tips/word_select.rs:17-19`）。某个 tip 甚至会因为「快速 Enter 会把几秒的多秒 turn 压缩成亚秒闪烁」而暂停 TTL（`src/tips/small_screen.rs:30-32`）。
- **选择高亮与光标闪烁都不由本程序动画**：选择高亮是 150 ms 定时消失（`DEFAULT_SELECTION_HIGHLIGHT_DURATION_MS = 150`，`src/app/agent_view/mod.rs:431`），仅触发一次重绘（`src/app/agent_view/selection.rs:42-59`）；光标闪烁交给终端——`[ui].cursor_blink` 三态映射到启动时的 `CursorStylePolicy`，默认 `Inherit` **不发任何转义序列**，注释说强制设置曾被报告为光标闪烁问题（`src/app/mod.rs:1427-1445`；配置项见 `docs/user-guide/26-config-reference.md:604`）。

#### 3.6.4 流式文本：三层合帧，无打字机

结论：**没有逐 token 追加渲染，也没有逐字延迟显示的「打字机」**（全仓 grep `typewriter|teletype|char_reveal` 无匹配）。文本按批次进入 scrollback，绘制由每帧合并：

1. **接收批次**：单次事件循环最多 drain `ACP_DRAIN_BATCH_MAX = 32` 条消息，且「输入一到就截断」，注释写着「流式期间每帧会有几十条消息排队」（`src/app/event_loop.rs:1837`、`:2193-2207`）。
2. **绘制节流 + 在途闸门**：`presenter.request_throttled(now, min_draw_interval)`（默认 16 ms）；上一次绘制尚有未写完的 payload 时不绘（`event_loop.rs:2208-2223`、`:579-618`）。同一处注释点明目的：*Cap paint rate so terminal input isn't starved during heavy ACP streaming*。
3. **视口作用域**：`ScrollbackState::needs_animation()` 仅在「有 running 条目且在视口内」时为真（`src/scrollback/state/mod.rs:475-479`），`tick()` 同理（`:419-453`）；注释解释「以 ~30fps 重绘一个静止画面纯属浪费：帧 diff 会是空的」。因此一次绘制可能合并多个 token chunk，屏幕上表现为**按帧吞吐的整段增长**，而不是字符逐个显现。

增量文本的 UTF-8 边界另有专门的增量解码器缓冲（`src/acp/tracker.rs:156-158`），与渲染节流无关。滚动不走动画 fps 而走 16 ms 节奏，注释明说 *Runs on the 16ms redraw cadence, not the slower animation fps*（`src/app/event_loop.rs:2394-2403`）。

#### 3.6.5 加载态与等待态：一层行 + 一套文案

```
┌─ scrollback ─────────────────────────────────────────┐
│ ▍⠧ Run cargo test                            0.3s    │ ← 运行中块：accent 波形 + 动画 bullet
│ ▍ Subagent "explore the parser"              12s     │ ← 子代理 Started 块（同样波形）
├──────────────────────────────────────────────────────┤
│ ⠧ Run cargo test 0.2s          1m20s ⇣12k [↓] [stop] │ ← turn status 行（仅忙碌/有 watcher 时占 1 行）
└──────────────────────────────────────────────────────┘
```

- 该行 `should_show()` 为假时**高度为 0，完全不占位**（`src/views/turn_status.rs:13`、`:723-736`）。活动语义由 `compute_activity` 决定颜色与文案：`Thinking…` / `Responding…` / `Verifying…` / `Compacting…` / 工具（绿色 `Run …`，有 description 时转为中性）/ 重试（warning 色）/ `Cancelling…`（红）（`turn_status.rs:580-698`）。
- **等待原因被显式命名**，而不是笼统的 `Waiting…`：`WaitingReason` 7 种 + 对应文案（`src/acp/tracker.rs:48-73`、`:116-136`）——`Waiting for response…`、`Waiting on subagent…`、`Waiting on task output…`、`Waiting on tasks…`、`Sleeping…`、`Running <event> hook…`、`Waiting for the agent to accept the prompt…`。**短等待不显示**：hook 批次未超过 `HOOK_REVEAL_DELAY = 300ms` 一律不画，避免「快 hook 闪一下」（`tracker.rs:68-75`）。工具参数写作阶段也有独立文案：`Writing subagent prompt…`、`Preparing MCP tool…`、`Searching MCP tools…`（`tracker.rs:156-191`）。
- 子代理生命周期在 scrollback 里是四态块：`Started`（运行中动画 bullet）/ `Completed` / `Failed` / `Cancelled`；阻塞式一个块变色，后台式保留 started 块另加完成块（`src/scrollback/blocks/subagent.rs:1-12`、`:252-277`）。后台工作在 turn status 上的「still running」计数按类型分列，cue 顺序按数量开头，窄屏裁尾也保住数字（`turn_status.rs:105-140`、`:1337-1352`）。
- **两处刻意不做动画**（重要设计倾向）：tasks 顶栏的运行计数用**静态菱形**——「头部整个会话都在视野里，这里放 spinner 读起来是噪音而非进度」（`views/agent_status.rs:165-172`）；阻塞在权限 / 提问时，把 spinner 换成脉冲菱形并把波形**冻结**成实色竖条，读作「暂停等你」而不是「还在加载」（`views/turn_status.rs:365-376`、`src/scrollback/wrappers/entry_renderer.rs:715-720`、`:839-856`）。

#### 3.6.6 完成 / 失败 / 取消：反馈是文本标记，不是特效

- `src/app/turn_completion.rs` 是 turn 终结的**唯一收敛点**：两条轨（fire-and-forget 的 `prompt_complete` 广播 + 可持久重放的 `TurnCompleted`）都收敛到 `finalize_turn_from_terminal`，以便中途 attach 的 viewer 也能从重放里正确结束 turn（`:1-7`、`:484-552`）。marker 选择是一张纯函数表（`:87-109`）：`EndTurn/Refusal/MaxTokens/... → TurnCompleted`；`Cancelled` 且 `send_now` → **不产生 marker**（取消并立即发送是静默的）；`RateLimit → None`；`Error` 且已有 error banner → `None`（避免重复报错）；hook 拒绝另走 `TurnBlockedByHook`（`:129-147`）。文案：`Worked for 2m5s` / 无计时钟时 `Turn completed.` / 取消是 `<cause 短语> in X.`（`src/scrollback/blocks/session_event.rs:377-400`）。driver 侧还有「丢失 RPC 对账」：收到 terminal 但 `session/prompt` 响应未到时先 arm `pending_turn_end_reconcile`，由 tick 的 reconcile 收尾（`:365-460`）；只要该操作 armed，`view_tick_demand` 就返回 `Fast`，保证对账不被饿死（`src/app/app_view.rs:5643-5649`）。
- `src/app/cancel_latency.rs` **只有 33 行、纯遥测、零 UI**：三个类型 `CancelOrigin`、`TurnEnd { Completed, Aborted }`、`CancelLatency { requested_at, scope }`（`:1-33`），文件头注释写「the settle rule behind the `CancellationCompleted` telemetry event」。打点仅「用户手势 + Root surface」时记录锚点、同一 turn 只留最早一次（`src/app/agent_view/session.rs:543-554`）；settle 时 `Completed` 产出 `latency_ms`，`Aborted`（reload/fork/session 失败强置 idle）直接丢弃（`:555-570`）。**它不参与任何视觉反馈**；取消的可见反馈全部来自状态行：`TurnCancelling` → 红色 `Cancelling…` 标签 + `[stop]` 保持可点（作为「取消没生效」的重试点）（`turn_status.rs:298-306`、`:588-591`，测试 `:984-1021`）。相邻的一处观测通道：终端不再读取 PTY 时（writer 队列有积压且零进展，超过 `WRITER_BLOCKED_WARN_AFTER`）发一次 `TermWriterBlocked` 遥测（`src/app/event_loop.rs:2151-2170`），不影响画面。

#### 3.6.7 降级与「减少动效」

**代码里不存在 reduce-motion 概念**：全仓 grep `reduce.motion|prefers-reduced|low.power` 无匹配，也没有任何「关闭动画」开关；`docs/user-guide/21-terminal-support.md` 完全不涉及动效 / 帧率 / 性能（该文件章节为颜色、剪贴板、tmux、按键、鼠标、语音、RTL 等）。实际存在的降级 / 收敛路径共五条：

1. **配置降帧**：`[animation] fps` 1–60、`wave_rows`（`docs/user-guide/05-configuration.md:686-692`、`06-theming.md:231-235`；clamp 见 `appearance/config.rs:1282-1283`）；**按显示器刷新率自动定档**：启动时探测刷新率，`ms = clamp(round(1000/hz), 8, 16)`，仅当 hz 落在 55..=240 且开关打开，可由环境变量 `GROK_MIN_DRAW_MS` / `GROK_SCROLL_CADENCE_MS` 覆盖（夹取 1..100 ms），也可用 `[ui.display_refresh].auto_cadence_enabled` 关掉退回 ~60 Hz（`display_refresh.rs:55-75`、`:300-321`；`src/app/display_refresh_startup.rs:14-37`、`:91-100`；`src/settings/defs.rs:1003-1020`）——注意这挂的是**显示器刷新率**，不是「终端慢」。
2. **离屏不驱动 + minimal 模式**：视口外的 running 条目不产生 tick 需求（`src/scrollback/state/mod.rs:460-479`），完成闪光同样只算视口内的条目（`:427-450`）；minimal 模式把 tick 上限收紧到 16 ms（`src/app/app_view.rs:5563-5571`）、滚动条整体隐藏（`render/scrollbar.rs:42-55`）。
3. **老终端字形替换（非降帧）**：legacy ConHost 用 CP437 回退字形（spinner 用 `| / - \`、monitor 脉冲用填充变化、进度条用 `░▒▓`）以免 tofu（`crates/codegen/xai-grok-pager-render/src/glyphs.rs:100-117`、`:148-175`；`src/views/progress_bar.rs:18-29`）。

另有一个相邻节流：状态行脚本（`$GROK_STATUS_LINE`）的事件驱动重算被限为 `EVENT_DEBOUNCE = 300ms`，强制 / 到期刷新降到 100 ms，注释说明目的是「让忙碌的 turn 不要每帧都跑一次脚本」（`src/app/status_line.rs:17-21`）。

**未找到证据**：① 无「打字机式」逐字显现或可配打字速度；② 无 reduce-motion / 无障碍动效开关；③ 无「终端慢则自动降帧」的检测路径（只有显示器刷新率探测）；④ 无完成时的庆祝 / 闪光特效，turn 完成的视觉产物只有一条文本标记；⑤ 未找到独立的「帧预算」常量或预算超支丢弃逻辑——预算效果由 §3.6.1 的五层机制间接达成。

---

### 3.7 图形化内容：Mermaid、内联图片与视频

### 本节要点

- Grok 的「图形」不是一种技术，而是**三条互不依赖的路径**：Mermaid 文本制表符图（全终端可用）、post-flush 逃逸序列贴的像素层（Kitty / iTerm2）、以及按需 PNG（子进程 + 磁盘缓存，交给 OS 看图器）。
- **渲染出的 Mermaid PNG 从不内联显示**；内联的文本图则不参与高保真渲染——两条实现互不相干，只靠注释纪律同步断词规则。
- 终端图形协议**只有 Kitty 与 iTerm2 两族**；`sixel` 全仓库零命中，`▀▄█` 只用于进度条 / 滚动条 / 光晕等文本语义，**没有栅格图转半块字符的渲染器**，因此降级链止于「文本 affordance」。
- scrollback 媒体是「**单元格占位 + 帧后贴像素**」：占位可选中可滚动可复制，像素不参与单元格绘制；滚动用 `top_crop_rows` 转成 Kitty 源裁剪，而不是重新解码图片。
- 视频是把**全部帧预抽到内存**的帧序列：10 fps、宽 ≤640、PNG（Kitty）/ JPEG（iTerm2），帧集可达几十至几百 MB，释放一律排到帧后。
- 复刻时最容易踩的三处：两套 Mermaid 渲染器 = 两套 bug 面；minimal 模式与图形能力是**两把独立的锁**；iTerm2 的 `preserveAspectRatio=0` 与 Kitty 的 `C=1` 直接决定观感。

本节路径省略 `reference/grok-build/` 前缀。

#### 3.7.1 三条路径总览

```
┌ 第 1 层：文本图形（ratatui Buffer，全终端可用，不依赖任何协议）───────┐
│  ```mermaid  → xai-grok-markdown/src/mermaid.rs  自建 parser + 布局 → │
│                 Unicode 制表符图（┌─┐│╎┃），随 markdown 一起绘制      │
│  图片/视频    → 预留 N 行空白 + 文件路径行 + [Open]/[Play]/[Copy] 行 │
└───────────────────────────┬───────────────────────────────────────────┘
                            │ 只在支持图形协议的终端上叠加第 2 层
┌───────────────────────────▼───────────────────────────────────────────┐
│ 第 2 层：像素图形（post-flush 逃逸序列，绕过 ratatui 单元格）          │
│  Kitty APC (\x1b_G…) 或 iTerm2 OSC 1337；写 stderr，在 cell flush 之后 │
│  位置用「单元格坐标 + 源像素裁剪」描述，不参与单元格绘制/选中/复制     │
└───────────────────────────┬───────────────────────────────────────────┘
                            │ 需要真实像素时另开进程
┌───────────────────────────▼───────────────────────────────────────────┐
│ 第 3 层：Mermaid PNG（子进程 + 磁盘缓存）                              │
│  xai-grok-pager __mermaid-render → mermaid-to-svg(dagre) → SVG →      │
│  resvg/tiny-skia → PNG 落盘 → 交给 OS 看图器或复制路径                 │
└───────────────────────────────────────────────────────────────────────┘
```

关键取向：**渲染出的 PNG 从不内联显示**（`scrollback/blocks/mermaid_content.rs:6-8`：*The rendered PNG is never drawn inline; it is reached only through the affordance row's actions.*），而内联的文本图不参与高保真渲染；两条实现互不相干，**注释明确要求人工保持同步**——`LABEL_BREAK_CHARS` 与 `third_party/mermaid-to-svg/src/text_wrap.rs` 的 `TOKEN_BREAK_CHARS` 一致（`xai-grok-markdown/src/mermaid.rs:35-38`）。

#### 3.7.2 内联文本图：零依赖、即时、永远可用

- 识别只认 `mermaid` fence 的**第一个 token**：`is_mermaid_info()` 取 info string 第一个空白分隔 token、大小写不敏感比较 `"mermaid"`（`mermaid_content.rs:87-92`），因此 ` ```Mermaid `、` ```mermaid theme=base ` 都算；数据来自 markdown 渲染器暴露的 `CodeBlockSpan`（`:94-123`）。流式解析器**只在闭合 fence 上**替换正文（`xai-grok-markdown/src/parse.rs:636-643`：`range.end < self.text.len()`，即未闭合的 fence 在流式过程中继续显示源码），且仅在 `pretty` 模式生效（`render.rs:469`），raw 模式保留源码。检测结果缓存在 message 级别：`AgentBlock` 在构造 / 结束时一次算出 `MermaidContent`，**不是每个流式 chunk 都算**（`scrollback/blocks/agent.rs:17-18`、`:74`）。
- `crates/codegen/xai-grok-markdown/src/mermaid.rs`（5699 行）是一个**自包含的终端 Mermaid 渲染器**，不依赖 `xai-grok-mermaid`、不依赖 dagre、不需要 PNG；支持 `graph`/`flowchart`、`stateDiagram`、`classDiagram`、`erDiagram`、`sequenceDiagram`（`:67-82` 的 `or_else` 链）。布局是自建的 Sugiyama 风格：`compute_ranks` → `order_ranks`（barycenter 交替上下扫，最多 8 轮，取交叉数最少者，`:2672-2753`）→ `assign_positions`（10 轮松弛，`:2806-2868`）→ `place_td` / `place_lr`。画布用一个位掩码画布 `Canvas`（上/下/左/右 4 个 bit），最后 `finalize_mask()` 统一选制表符（`mask_char`，`:1730-1745`）；虚线走 `╌╎`、粗线走 `┃━┏┓┗┛`（`:1747-1770`）。`BT`/`RL` 的做法是**先按 TD/LR 排好再整体翻转画布**，翻转后把文本 run 反向回来保证可读（`flip_vertical`/`flip_horizontal`，`:1621-1669`）。
- 硬上限（防不可信输入膨胀，`:28-45`）：`MAX_NODES=128`、`MAX_EDGES=512`、`MAX_GROUPS=24`、`MAX_GROUP_DEPTH=6`、`MAX_CANVAS_CELLS=1<<21`、标签 `WRAP_WIDTH=24` / `MAX_LINES=4` / `MAX_LABEL=28`。画不下或解析不了时一律退化为**带标题的框**：标题 ` mermaid: <首个 token> `，正文是原文换行截断（`fallback()`，`:3925-3997`）；若原因是「太宽」，额外附一行斜体提示 `This diagram is too wide to display here — open the image to view it in full.`（`:3922-3923`、`:3983-3985`）。
- 内联文本图下面会插入一行**不可选中的 affordance 行**（`apply_affordance_rows`，`mermaid_content.rs:373-397`）：`◇ mermaid   [Open Image]   [Copy Image Path]   [Copy Source]`，右侧在渲染中显示 `rendering diagram…`；它不参与复制、也不移动 pre-wrap 行号。按钮间距 `AFFORDANCE_GAP = 3`（`:31-38`），行布局只有 `affordance_row()` 一个来源（`:310-325`），绘制与点击命中读同一份数据（`app/agent_view/media.rs:229-313`），因此不会漂移。设置项 `ui.render_mermaid = auto|on|off`（默认 `auto`，`docs/user-guide/26-config-reference.md:621`；读取路径 `appearance/cache.rs:593-607`）：`off` → 只有内联图、无 affordance 行；`auto`/`on` → 有（`mermaid_content.rs:239-246`）。**minimal（静态提交）模式强制 `SourceOnly`**，因为绘制循环不会运行、按钮会是死行（`:249-257`）。Mermaid 也是唯一刻意不看图形能力的部分——`mermaid_content.rs:236-238` 注释：*Terminal graphics capability is intentionally not consulted either: the affordance row is text plus mouse hit-rects, so it works everywhere.*

#### 3.7.3 按需 PNG：点击才渲染，且必须出进程

时序（生产路径）：

```
用户点 [Open Image]
   │
   ├─ 磁盘命中（read_cached_png：非符号链接 + 能解码）→ 直接 open_media_natively / 复制路径，不渲染
   │
   └─ 未命中 → ensure_mermaid_runtime() 懒启动 worker 线程（"mermaid-render"）
                → MermaidJob{key, source, out_path, theme_dark, quality=Open}
                → 子进程: current_exe() __mermaid-render --out … --theme … --quality open --width … --deadline-ms …
                │            stdin = 图表源码（≤64 KiB+1 字节则拒绝）
                │            子进程内：install_child_backstops() → read_stdin_capped() →
                │                      render_checked(default_engine()) → write_png_atomic()
                → 墙钟超时（3s / 20s）→ killpg(SIGKILL) 清理进程组
                → map_run_result：零退出 + PNG 可解码才算成功
                → mpsc 回主线程 → poll_mermaid_results → complete_mermaid_action
```

- 入口分三档：先查内存 pending → 再查磁盘缓存命中 → 未命中才派发 worker 任务（`request_mermaid_render`，`src/app/mermaid_worker.rs:955-1031`）；**两种动作都强制走 Open 画质档**（`:958-960`）。worker **只有一个线程**（不是线程池），并**按 cache key 合并同一批请求**（`drain_coalesced` + `IndexMap` FIFO，`:208-261`）。
- 隐藏子命令 `__mermaid-render` 在 `main` **最早期**被拦截，早于任何 TUI / agent / tokio 初始化（`mermaid_worker.rs:365-382` 与模块文档 `:19-24`）；参数 `--out --theme --quality --width --deadline-ms`（`:291-320`），`--max-height` 作为 legacy 被接受并忽略（`:505-508`）。Linux 上额外设 `_RJEM_MALLOC_CONF`/`MALLOC_CONF=narenas:1`，避免多核机器上分配器预留逼近 `RLIMIT_AS` 导致每次渲染都 abort（`:312-319`）。
- 尺寸与画质两档（`mermaid_worker.rs:66-89`、`:579-602`；`xai-grok-mermaid/src/lib.rs:132-161`）：**`Terminal` 档**（预留，当前点击路径不用）宽度 = 内容列数 × `APPROX_CELL_W_PX(8)` × `RENDER_SCALE(2)`、clamp 到 320–1600，高度上限 2400 px，超时 3 s；**`Open` 档**（`[Open Image]` / `[Copy Image Path]` 实际使用）走 `RenderParams::for_os_viewer`（`scale=2.0` + 最小宽度 2560），高度上限 8192 px，超时 20 s。表面色只有两个：`LIGHT_SURFACE = #FAFAFA`、`DARK_SURFACE = #18181B`（`xai-grok-mermaid/src/lib.rs:64-67`）；只有 `GrokDay` 判为亮色，`Terminal` 主题也按暗色处理（`mermaid_content.rs:126-129`）。
- **为什么必须出进程**：shipped profile 用 `panic = "abort"`，`render_checked` 里的 `catch_unwind` 在那里是空操作；同步渲染也无法被超时打断。模块文档把「真实崩溃隔离在进程之外」写成了设计结论（`xai-grok-mermaid/src/lib.rs:13-21`；`mermaid_worker.rs:14-34`）。
- **为什么 vendored 一整套 dagre / graphlib**（理由出自仓库自己的注释，不是推测）：`third_party/README.md:8-14` 与 `third_party/mermaid-to-svg/Cargo.toml:7-12` 写的是「渲染的是**不可信模型输出**」，vendoring 换来完整审计面 + 钉死版本 + 免受 crates.io yank。依赖链为 `xai-grok-mermaid → mermaid-to-svg (MIT) → dagre_rust (Apache-2.0) → graphlib_rust → ordered_hashmap`，上游是 `warpdotdev/mermaid-to-svg` 与 `r3alst/dagre-rust`（`third_party/README.md:17-24`、`:27-37`）。**算法等效 mermaid.js**：mermaid-to-svg 把布局交给 dagre.js 的 Rust 移植，注释写它「修复了上一个引擎在环形 flowchart 回边上的错误路由」；`xai-grok-mermaid` 的测试用同一个环形登录流回归——8 条边必须各自带一个 `marker-end="url(#arrowhead)"`（`xai-grok-mermaid/src/pure.rs:120-154`）。
- 本地补丁有清单，逐条记在 vendoring notes 里：不 vendor CLI bin 与 snapshot 测试；`mermaid_port::is_enabled()` 无条件返回 `false`（该实验分支读环境变量、对不可信输入不确定，且会错路由环形回边）；整段重写 sequence 解析器；CJK 宽度用 `unicode-width` 计算；open edge-label 语法；单一超长 token 不再被硬切（`third_party/mermaid-to-svg/Cargo.toml:14-96`）。`dagre_rust` 的补丁是把上游 `static mut UNIQUE_STARTER` 换成 `AtomicUsize`，消除多线程渲染的数据竞争（`third_party/dagre_rust/Cargo.toml:18-26`）。升级流程被写成 4 步 checklist：读 vendoring notes → 重放补丁 → 核对许可证 → 刷新 `NOTICE`（`third_party/README.md:39-45`）。

#### 3.7.4 终端图形协议：两族，且降级顺序固定

```rust
// pager-render/src/terminal/image.rs:144-157（简化）
pub fn protocol_for_brand(brand: TerminalName, is_windows: bool) -> GraphicsProtocol {
    if is_windows { return GraphicsProtocol::None; }   // ConPTY 会吞掉 APC/OSC
    match brand {
        Kitty | Ghostty | WezTerm | WarpTerminal => GraphicsProtocol::Kitty,
        Iterm2 => GraphicsProtocol::None,              // OSC 1337 无 placement/clear，预览另行放开
        _ => GraphicsProtocol::None,
    }
}
```

- `GraphicsProtocol` 只有 `Kitty` / `ITerm2` / `None` 三个值（`crates/codegen/xai-grok-pager-render/src/terminal/image.rs:29-38`）；探测是**纯环境推断 + 一次缓存**（进程级 `OnceLock`，`:46`、`:71-83`），不做运行时特性协商；品牌来自 `TerminalName`（20 个变体，`terminal/mod.rs:58-107`）；tmux 直接跳过（`terminal/mod.rs:374-379`）。
- **scrollback 内联叠加层比协议支持更窄**：只在 `Kitty` 协议**且**品牌 ∈ {Kitty, Ghostty, WezTerm} 时启用——注释说明 Warp 虽然接受部分逃逸，但不支持 placement / scrollback 模型，会留下残影（`terminal/image.rs:85-120`）。**iTerm2 只在 prompt 预览这一处放开**（因此上面的 `Iterm2 => None` 与这条并不矛盾，是分层放开）：条件是 `TERM_FEATURES` 含 `F`，或该变量不存在但处于 SSH（因为该变量不过 SSH，缺失不等于拒绝）；理由注释是「预览框永不落在最后一行，关闭时会重绘所有占位单元格，因此不需要 clear 逃逸」（`:159-198`）。minimal 模式用原子开关**强制关闭**内联叠加层，原因是它只提交静态文本、不跑绘制循环（`:48-62`；启动时由 `src/app/mod.rs:391` 按 minimal 传入）。

降级顺序与判定表：

| 场景 | Kitty | iTerm2 | None（含 tmux / Windows / 未知终端） |
| --- | --- | --- | --- |
| scrollback 内联图片 / 视频 | Kitty/Ghostty/WezTerm 才启用（`terminal/image.rs:109-120`） | ✗ 不用 | ✗ |
| prompt 悬停预览（image chip） | ✓ | `TERM_FEATURES` 含 `F` 或 SSH 时 ✓ | ✗ → 元数据文本 |
| 图片 / 视频查看器模态 | ✓ | ✓ | ✗：被 `guard_image_support()` 拦下并弹 toast |
| Mermaid PNG | 与终端能力**无关**：生成文件后交给 OS 看图器 | 同左 | 同左 |

非图形终端打开图片查看器会被 `guard_image_support()` 提前拦下并弹一句 toast：tmux 下是 `Inline images disabled within tmux.`，其他终端是 `Image rendering not supported in this terminal`（`app/agent_view/notices.rs:271-281`，调用点 `app/agent_view/prompt.rs:468-477`）。

**未找到证据**：全仓库（Rust 源码、文档、`Cargo.toml`、vendored 源码）检索 `sixel` **零命中**；`braille` / `quadrant` / `chafa` / `viu` / `timg` 亦零命中，`▀ ▄ █ ▌` 的用法集中在进度条（`views/progress_bar.rs:1`、`:16-20`）、滚动条（`render/scrollbar.rs:185`）、dashboard 光晕（`views/dashboard/render.rs:1104`、`:1254`）、工具用量图标（`tool_usage.rs:64-70`）等**文本语义**场景。结论：降级链是 **Kitty → iTerm2（仅 prompt 预览）→ 文本 affordance**，不存在第 4 档字符画。

#### 3.7.5 两族协议的实现差异（决定观感的部分）

- **Kitty**：头部字段 `a=T,f=100,t=d,q=2,C=1,z=,i=1,p=1,c=,r=`——`a=T` 传输并显示、`f=100` 只收 PNG、`t=d` 直接传数据、`q=2` 静默、`C=1` 保留光标、`z` 决定层级、`i/p` 同一个 placement id 1、`c/r` 是单元格尺寸；数据 base64 后按 4096 字节切块，非末块 `m=1`（`terminal/image.rs:326-386`）。**缩放交给终端**：注释明确 *terminal scales to the cell rect (it knows HiDPI)*（`:325`）。两种原语：`transmit_kitty_image`（`a=t`，只上传不显示，`:358-361`）与 `place_kitty_image`（`a=p`，约 50 字节的纯定位，`:391-396`），带 `x,y,w,h` 的 `place_kitty_image_cropped` 负责源裁剪（`:402-416`）。层级：模态叠加用 `z=1`（文本之上），scrollback 内联媒体用 `z=-1`（文本之下、背景之上，下拉菜单能盖住它）（`:337-343`、`:542`）。删除靠 id：`clear_kitty_image(id)` → `a=d,d=i,i=<id>`（`:419-421`）。**Warp 特别处理**：placement replace 被忽略，所以固定 id 的模态图先删再传，否则会叠影（`:441-443`）。编码转换：只有 PNG 能直传，非 PNG 在 macOS 走 `sips`（保留 ICC profile，避免 JPEG→PNG 伪影），其他平台走 `image` crate 重编 PNG（`:233-315`）。
- **iTerm2**：一行 OSC 1337 内联图片，尺寸直接给单元格数并**关闭宽高比保持**（`preserveAspectRatio=0`），因为调用方已经算好矩形、再保持比例会留黑边（`terminal/image.rs:428-434`）：`ESC ] 1337 ; File=inline=1;width=<cols>cells;height=<rows>cells;preserveAspectRatio=0:<base64> BEL`。iTerm2 **没有 place-only 逃逸**，所以每次矩形变化都要重发整幅图（`app/agent_view/media.rs:200-221` 的 `emit_iterm`）；而且 OSC 1337 会推进光标，必须用 DECSC/DECRC（`\x1b7`/`\x1b8`）把光标包起来（`terminal/image.rs:464-471`）。另一个 Kitty 没有的问题：拖拽改变窗口尺寸时 iTerm2 会**连已提交的像素一起按网格缩放**，而 OSC 1337 没有 clear 原语——所以预览在 resize 后的 300 ms 静默窗口内被隐藏（并保留 100 ms 额外 tick 边距持续重绘）；Kitty 终端则是丢弃并按新矩形重放（`app/agent_view/notices.rs:13-16`、`:160-177`）。

#### 3.7.6 尺寸、占位、滚动与清理

- 单元格换算的基准是**实测 cell 宽高比**：`cell_aspect()` 用 crossterm 的 `window_size()` 算 `(窗口像素宽/列数) ÷ (窗口像素高/行数)`，只在 `0.3..=0.8` 这个「真实等宽字体」带内接受，否则回落到 `DEFAULT_CELL_ASPECT = 0.5`（典型 8×16 cell）——理由注释是协议会填满单元格矩形、假设 1:2 会把其他字体的图拉变形，且 tmux / Windows / 非 tty 的报告不可信（`terminal/image.rs:555-591`）。`fit_image_to_cells(img_w, img_h, max_cols, max_rows)` 把图像宽高比换算成「每行需要多少列」（`cols_per_row = img_aspect / cell_aspect`），先尝试按宽铺满、行数超了就改按高铺满（`:594-621`）。
- 图片 / 视频在 scrollback 里是**两层分离**：单元格层画占位（可选中、可滚动、可复制），像素层用逃逸序列贴在算好的矩形上。

```text
┌ scrollback 终端窗口 ────────────────────────────┐
│  上一段消息文本…                                 │
│ ┌ tool media 块（entry_renderer 预留 rows+3 行）┐│
│ │ ① 顶部 1 行：文件路径（点击复制路径）          ││ ← content_y_start + 1
│ │ ② 中间 rows 行：图像占位（像素由 post-flush   ││ ← screen_rect（随滚动裁剪）
│ │    逃逸贴在这块矩形上，选择/复制看不到像素）   ││
│ │ ③ 末尾 1 行：空白 spacer                      ││
│ │ ④ 最后 1 行：[Open] / [Play] / [Copy]         ││ ← content_lines - 2
│ └───────────────────────────────────────────────┘│
│  下一段消息文本…                                 │
└──────────────────────────────────────────────────┘
```

- 预留行数取**单一来源**：`inline_media_reserved_rows()` 返回 `(图像行数, 总行数=图像行数+3)`，被「预留占位」与「实际贴图」两处共用，注释强调两者不能漂移（`inline_media_ffmpeg.rs:121-134`；`src/scrollback/wrappers/entry_renderer.rs:350-364`）。图像行预算：`max_cols = content_width - 2`，`max_rows = (content_width / 2).clamp(4, 20)`。非图形终端返回 0 行，改为输出文本 `[Open]` 行（`entry_renderer.rs:351-353` → 0；`scrollback/blocks/tool/other.rs:413-427` 的 `inline_open_button()`）。视频海报在 scrollback 里就是普通图片路径；无 ffmpeg 时预留行数缩为 1–2 行横幅（安装提示），其余布局不变（`inline_media_ffmpeg.rs:126-129`、`:183-194`）。
- **滚动是「移动窗口」而不是重新解码**：每帧按虚拟坐标算出可见部分，用 `top_crop_rows` 表达「被上方裁掉多少行」，可见高度不足完整高度时转成 Kitty 源裁剪的 `y/w/h`（`src/scrollback/render.rs:650-712`；换算 `src_y = top_crop_rows * img_h / fit_rows`，`terminal/image.rs:518-543`）。可见性门槛：`full_image_h >= 1 && media_width >= 4`。Mermaid 的 affordance 行也走同一套虚拟坐标 → 屏幕矩形换算，只画**行内可见**的那一行（`scrollback/render.rs:728-745`）。
- **帧间清理与叠加层记账**：每帧收集本帧实际放置的 Kitty id 集合 `this_frame_ids`，与上一帧 `last_placed_ids` 求差——消失的 id 立即发 `clear_kitty_image` 并从 `inline_media_ids` 移除（`app/agent_view/render.rs:4173-4181`）；本帧没有任何放置时整批清理（`:4183-4206`）。打开任何模态（图片 / 视频查看器、gboom、block viewer、feedback 等）会先把 scrollback 的像素全部清掉，避免浮在模态之上（`:629-650`）；dashboard 切换时也有一次性的「陈旧图像清理」（`app/app_view.rs:4209-4235`）。非 scrollback 的叠加层（图片查看器 / prompt 预览）共用 `KITTY_PLACEMENT_ID = 1`，由 `terminal/overlay.rs` 用 `(owner_id, cols, rows, x, y)` 记账：几何完全没变就发**空串**（不重传），变了才「先删再传」（`:161-184`；测试 `:301-352`），`commit()` 只在写成功后记账（`:104-110`、`:355-375`）。

#### 3.7.7 视频：预抽帧到内存的帧序列

- **抽帧在后台线程**：`start_inline_video_playback` 起线程调 `VideoViewerState::open_from_path`，通过 mpsc 回传，UI 先给 `Loading video…` toast（`app/agent_view/media.rs:430-462`）；若 `open_from_path` 失败，tick 里弹 `Video playback requires ffmpeg`（`app/app_view.rs:5454-5465`）。
- **参数与海报帧**：`VIDEO_FPS = 10.0`（取源帧率与 10 的较小者）、`VIDEO_MAX_WIDTH = 640`（更宽才加 `scale=640:-2`）、`-q:v 5`；输出格式按协议选 **PNG（Kitty）/ JPEG（iTerm2）**，写成临时目录里 `%06d.<ext>`，读完帧就删目录（`crates/codegen/xai-grok-pager-render/src/prompt_images.rs:214-217`、`:263-330`）；帧集合被注释描述为「5–15 秒 10 fps ≈ 50–150 帧、约 1–3 秒」。scrollback 静止显示用**海报帧**：`extract_poster_frame` 先 `-ss 1` 取第 1 秒的画面试代表帧，失败再取首帧，协议为 `None` 时直接返回 `None`（`:397-437`）。
- **播放节奏**：`app_view` 每 tick 用 `last_frame_time.elapsed() >= 1/fps` 推进一帧，末帧后置 `finished`（`app/app_view.rs:5469-5482`）；`view_tick_demand()` 把「视频未播完 / 正在抽帧 / mermaid 有待办」都算作需要继续 tick 的理由（`:5636`、`:5717-5719`）。每次换视频或关闭查看器都会丢弃**整个帧集**（几十到几百 MB），通过 `memory_release::request_release_after_draw` 在**帧后**归还内存，绝不在绘制中途同步释放（`app/agent_view/media.rs:370-388`、`:592-614`；`app/agent_view/render.rs:629-650`）。

#### 3.7.8 Prompt 侧的视觉：图片 chip 与 `@` 附件

- 粘贴 / 拖入成功后计数器 +1，占位文本 `[Image #N]`（`prompt_images.rs:744-747`），并作为 textarea 的**原子元素**插入（`ElementKind(3)` = `KIND_IMAGE`，`views/prompt_widget/mod.rs:45`、`:2363-2373`）。**chip 里永远不含路径**——路径只出现在预览与 overlay（`prompt_images.rs:743-746` 注释）。视觉是**整块反色底色的 chip**：`[`、`]` 用 `theme.paste_dim` 前景 + `theme.paste_bg` 背景，中间文字用 `theme.paste_fg` + `theme.paste_bg`，与粘贴文本块同款（`views/prompt_widget/mod.rs:3761-3770`）。插入前校验：宽或高小于 `MIN_SIDE = 8` 像素直接报错拒收（`:2354-2361`）。光标 / 悬停在 chip 上时元素交互类型是 `ImagePreview`（`:2938`）。
- `@` 文件引用是另一套元素（`KIND_FILE_REF`），显示形如 `@foo/bar.rs` 或 `@foo/bar.rs:10-12`：`@` 与 `:` 用灰色、路径用 `theme.path`、行号用 `text_primary`（`views/prompt_widget/mod.rs:3620-3634`、`file_ref_display`）。交互是把元素展开成文本（`ElementInteraction::Inlined`），**没有图片预览**（`:2933-2940`）。文档口径一致：`docs/tutorial/03-attach-and-paste.md:5-23` 把「`@` 提及文件（支持行号、`@!` 取隐藏文件）」「粘贴图片（macOS `Cmd+V` / Linux `Ctrl+V` / Windows `Alt+V`）」「`!` 直接跑 shell 命令」列为三种入口。
- **悬停预览是一个 2×2 决策矩阵**（`crates/codegen/xai-grok-pager-render/src/render/image_overlay.rs:1-12` 的模块注释直接给出）：有像素 + 有路径 = 图片 + 路径 footer；有像素 + 无路径 = 仅有图片；无像素 + 有路径 = 元数据 + 路径；无像素 + 无路径 = 仅元数据。框的最小宽 28、有像素最小高 8、纯元数据最小高 6；元数据框宽 = 区域宽 × 0.75、内容 4 行 + 2 行边框（`render/image_overlay/geometry.rs:8-14`、`:89-100`）。有像素时内部尺寸按 `fit_image_to_cells` 反推框大小、图在内部再居中（`geometry.rs:61-86`），框外整块区域做 50% 变暗（`image_overlay.rs:86`），拿不到 `prepared()` 就先画居中 `Loading...`；无像素时打印 `Format:` / `Dimensions:` / `Size:` / `Path:`（或 `Preview unavailable` / `Preview pending`）（`image_overlay.rs:142-186`）。像素通过 `overlay::static_image_for_protocol` 走共享 id 记账（`:202-213`）。
- **图片 / 视频查看器模态**都是居中 90% 的圆角框（宽至少 28、高至少 8），外圈 50% 变暗；视频版在底边框上画进度条（`▶`/`⏸` + `━` 已播 / `─` 未播 + `m:ss/m:ss`）（`render/video_overlay.rs:24-49`、`:77-122`）。图片查看器的显示字节在打开时就按协议准备好（JPEG→PNG for Kitty，`prompt_images.rs:56-82`），`overlay_owner_id` 与 prompt 预览共用同一套 owner 记账。键盘：`Esc`/`q` 关闭并先清 Kitty 图（`app/agent_view/media.rs:76-100`）；视频查看器另有空格播放 / 暂停、`←/→`（或 `h/l`）快退 / 快进（`:585-615`）；关闭时同步归还内存（查看器帧集可达 50–300 MB）。
- **远程 / 包裹场景的剪贴板图片**：`wrap_clipboard_image.rs` 解决 `grok wrap`（本地终端包住远程 pager）拿不到剪贴板图片的问题——本地全 miss 且 OSC 52 sink 已激活时，向 stderr 发私有 OSC `ESC ] 999;GrokWrapClipboardImage? BEL`（`:17-28`、`:49-87`）；wrapper 用**括号粘贴帧**回注 `GROK_WRAP_IMG\n<mime>\n<base64>` 或 `GROK_WRAP_NONE`（`:30-34`、`:124-138`），其中 `GROK_WRAP_NONE` **不是** `GROK_WRAP_IMG` 的前缀，正是为了区分「明确没有图片」与「帧被截断」。预算 `MAX_WRAP_IMAGE_BYTES = 20 MiB`（注释说 4 MiB 太小会让 retina 截图静默变成 NONE，`:36-39`）；超预算先按 JPEG 质量阶梯 `[85, 70, 55, 40, 25]` 重压，仍不行再半分辨率 JPEG 60（`:144-185`）；解码前先用 base64 长度估算拒绝，避免超大分配（`:106-117`）。安全注释把这条路径定性为「等价于一次图片剪贴板读取」，并明确写「不要把这个模式泛化到不可信的多路复用器上」（`:6-12`）。

#### 3.7.9 容错、缓存与失败降级

超时与进程隔离（`panic = "abort"` 下唯一的真实防线）：

| 机制 | 值 | 位置 |
| --- | --- | --- |
| 父进程墙钟预算（Terminal / Open 档） | 3 s / 20 s | `mermaid_worker.rs:82-96` |
| 子进程自毁看门狗（退出码 2，与普通失败 exit 1 区分） | 预算 + 3 s 后 `exit(2)` | `mermaid_worker.rs:384-427`、`:389-392` |
| 超时后清理 | `killpg(SIGKILL)` 整个进程组 | `xai-grok-mermaid/src/subprocess.rs:95-141` |
| 父子同生共死 | `prctl(PR_SET_PDEATHSIG, SIGKILL)`（Linux） | `mermaid_worker.rs:458-468` |
| 地址空间上限 | `RLIMIT_AS = 2 GiB`（Linux，只降不升） | `mermaid_worker.rs:401-456` |
| 源码大小上限 | 64 KiB（父进程先拒一次，子进程再拒一次） | `xai-grok-mermaid/src/engine.rs:36-50` |
| 光栅面积 / 单轴上限 | 32 MP / 16 384 px | `xai-grok-mermaid/src/raster.rs:22-27` |

缓存与记账（一条值得抄的不变式：**先拿到字节，再分配 Kitty id**——否则「读文件失败但已记 id」会让下一帧只发 `place` 不发 `transmit`，永久留一块空白，`app/agent_view/media.rs:135-138`、`:187-196`）：

| 缓存 | 键 / 容量 | 位置 |
| --- | --- | --- |
| Mermaid 磁盘 PNG | `blake3(source)` + 主题 + 宽度桶 + 画质档 + `RENDER_REVISION=3` | `mermaid_content.rs:134-195` |
| 宽度分桶 | 终端档每 8 列一桶；Open 档用哨兵 `u16::MAX`（不与终端档共用文件） | `mermaid_content.rs:39-53`、`:170-186` |
| 会话磁盘上限 | 200 MiB，超限按 mtime 由旧到新删；每 8 次新写入扫一次 | `mermaid_worker.rs:62-64`、`:98-101`、`:725-774` |
| 缓存命中的判定 | 非符号链接 + 能解码成图（半截 / 损坏文件视为 miss 重渲染） | `mermaid_worker.rs:263-277` |
| 内联媒体编码字节 | 64 MiB 上限，超限条目只作临时使用不入缓存（随机淘汰） | `app/agent_view/media.rs:46-70`、`:176-186` |
| 失败负缓存 | 按 `(len, mtime, ino, ctime)` 指纹记录读取失败；文件一变就重试 | `app/agent_view/media.rs:13-44`、`:149-175` |
| Kitty 上传记账 | id 一旦分配就保留在 `inline_media_ids`，即使 CPU 侧字节被淘汰也不必重传 | `app/agent_view/media.rs:176-178` |
| 预览结果 | `PromptImagePreview`（`OnceLock` + `identity()`）供同一张图跨帧复用 | `pager-render/src/prompt_images.rs:613-630` |

失败降级表：

| 失败 | 用户看到 | 证据 |
| --- | --- | --- |
| Mermaid 解析 / 布局 / 光栅化失败、超时、子进程崩溃 | toast `Could not render diagram`，内联文本图仍在 | `mermaid_worker.rs:341-360`、`:1048-1066` |
| 源码超 64 KiB | 父进程直接判失败，不启动子进程 | `mermaid_worker.rs:662-672` |
| 还没有 session 目录可缓存 | toast `Diagram not ready yet` | `mermaid_worker.rs:961-971` |
| 渲染进行中 | toast `Rendering diagram…` + 行尾 `rendering diagram…` | `mermaid_worker.rs:89`、`:1018`；`mermaid_content.rs:26-28` |
| 终端不支持图形 | 无像素，仅文本 affordance（`[Open]` 行） | `entry_renderer.rs:351-353`；`blocks/tool/other.rs:413-427` |
| 视频缺 ffmpeg | 1–2 行 `Install ffmpeg to view inline` 横幅（有包管理器时附可执行的 `! …install ffmpeg`） | `inline_media_ffmpeg.rs:5`、`:50-70`、`:117-129` |
| 媒体文件读不到 / 解码失败 | 不显示像素，且按指纹负缓存避免每帧重试 | `app/agent_view/media.rs:143-175` |
| 非图形终端点开图片查看器 | toast `Image rendering not supported in this terminal`（tmux：`Inline images disabled within tmux.`） | `app/agent_view/notices.rs:271-281` |
| iTerm2 预览缺 `TERM_FEATURES` 的 `F` | 降为元数据文本预览 | `terminal/image.rs:181-198` |

复刻时的边角（踩过才知道）：**两套 Mermaid 渲染器意味着两套 bug 面**——内联文本图与 PNG 的换行 / 断词规则靠注释纪律同步，不是共享代码（`xai-grok-markdown/src/mermaid.rs:35-38`）；**不要在绘制中途释放大内存**——帧集 / 大图的释放全部走 `request_release_after_draw`，输入路径（关闭查看器）才允许同步释放（`app/agent_view/media.rs:370-388`）。**minimal 模式与图形能力是两把独立的锁**：`set_inline_overlay_force_off` 与 truecolor / 协议探测互不影响，任何「是否会画像素」的判断都要同时看这两者（`terminal/image.rs:48-62`）；而子进程路径要先 `current_exe()`——worker 用当前可执行文件重新执行自己，`current_exe()` 失败时渲染功能整体关闭而不是回落到进程内渲染（`mermaid_worker.rs:178-203`）。
