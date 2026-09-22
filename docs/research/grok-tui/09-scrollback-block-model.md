# 专题 09：Scrollback 块模型与渲染管线

> 专题：Scrollback 块模型与渲染管线｜调研日期：2026-09-22｜证据范围：`crates/codegen/xai-grok-pager/src/scrollback/`（`mod.rs`、`block.rs`、`entry.rs`、`types.rs`、`layout.rs`、`render.rs`、`sticky.rs`、`search.rs`、`selection.rs`、`text_selection.rs`、`link_map.rs`、`table_geometry.rs`、`export.rs`、`scrollback_pane.rs`、`blocks/`、`blocks/tool/`、`state/`、`wrappers/`），辅以 `actions/defaults.rs`、`app/agent_view/{panes,links,selection,notices}.rs`、`xai-grok-pager-render/src/appearance/config.rs`、`docs/user-guide/03-keyboard-shortcuts.md`。只读调研，未运行构建。

## 1. 分层全景

Scrollback 是一条四段式流水线：**块（Block）→ 条目（Entry）→ 行（BlockLine）→ 单元格（Buffer cell）**。关键设计是：块产出的**已经是换行后的屏幕行**，因此布局层只做「高度算术 + 可见窗口切片」，不参与文本排版。

```
会话 / 工具事件
   │  AgentView 把消息、工具结果、会话事件转成块
   ▼
┌── 块层 ──────────────────────────────────────────────────────────┐
│ RenderBlock                       block.rs:298（13 个变体）        │
│   impl BlockContent               block.rs:40                     │
│       fn output(&self, ctx: &BlockContext) -> BlockOutput         │
│   blocks/{user,agent,thinking,tool/*,subagent,bg_task,…}.rs       │
└──────────────────────────────────────────────────────────────────┘
   │  BlockOutput { lines: Vec<BlockLine> }        types.rs:433
   │  ★ 每个 BlockLine 已是「按内容宽度换行后的一行」 render.rs:498
   ▼
┌── 条目层 ────────────────────────────────────────────────────────┐
│ ScrollbackEntry      entry.rs:57   块 + display_mode + 4 层缓存   │
│ ScrollbackState      state/mod.rs:53  entries / scroll / 选中 /   │
│   turns / group 折叠 / LayoutCache（state/layout.rs:42）          │
└──────────────────────────────────────────────────────────────────┘
   │  每帧：prepare_layout(state/mod.rs:1344) → paint window → render
   ▼
┌── 单元格层 ──────────────────────────────────────────────────────┐
│ ScrollbackPane::render_with_scratch   scrollback_pane.rs:139      │
│   └ EntryRenderer (impl Renderable)   entry_renderer.rs:23/543    │
│        desired_height(width) → 高度；render(area, buf) → 单元格   │
└──────────────────────────────────────────────────────────────────┘
```

## 2. 块抽象

### 2.1 `BlockContent` trait（`block.rs:40`）

trait 只描述「渲染什么」，不碰 Buffer。核心方法与职责：

| 方法 | 行 | 职责 |
| --- | --- | --- |
| `output(&self, ctx) -> BlockOutput` | `block.rs:42` | 产出内容行（唯一的必选内容方法） |
| `accent` / `bullet` / `accent_background` / `background` | `block.rs:46/52/58/62` | 左侧 1 列色条、bullet 颜色、背景带 |
| `has_vpad_for(appearance)` | `block.rs:69` | 上下各 1 行留白（默认 true） |
| `has_raw_mode` / `is_foldable` | `block.rs:81/86` | 能否切源码模式 / 能否折叠 |
| `next_fold_mode` / `collapse_mode` / `default_display_mode` / `finished_display_mode` | `block.rs:92/105/114/124` | 折叠状态机（见 §8.1） |
| `is_selectable` | `block.rs:131` | 是否参与 `j/k` 块级导航 |
| `has_bullet` / `is_groupable` | `block.rs:141/145` | 是否加 bullet / 是否参与密集分组 |
| `preamble` | `block.rs:148` | 全屏查看器的头部文本 |
| `inline_media` / `inline_media_placements` / `diagram_affordances` / `inline_open_button` | `block.rs:169/178/193/206` | 内嵌图片、Mermaid 图表的预留与点击行 |
| `estimate_extra_rows` | `block.rs:203` | 估算高度时补上看不见的行（仅 Mermaid 用） |

`RenderBlock::output()` 在转发后统一补 bullet：`prepend_bullet`（`block.rs:202`）把 bullet span 插到首行行首，并调用 `shift_selection_metadata_for_prefix`（`types.rs:398`）把「可选区」整体右移一格，避免 bullet 被复制进剪贴板。

### 2.2 `RenderBlock` enum 与 `delegate_block!`（`block.rs:298`）

枚举变体与内部类型一一对应：`Stub` / `UserPrompt` / `AgentMessage` / `ToolCall` / `Thinking` / `System` / `SessionEvent` / `BgTask` / `Subagent` / `Workflow` / `Btw` / `ContextInfo` / `MemoryCapture`（`block.rs:300-328`）。同文件内的 `delegate_block!` 宏把所有 trait 方法 match 转发到内部类型（`block.rs:331-347`），只有 `output()` 需要拦截：`RenderedBlockOutput` 路径用于 Edit / SentMessage 这两个需要额外 selection boundary 侧车数据的块（`block.rs:413-425`）。

`ToolCall` 再套一层 `ToolCallBlock`（`blocks/tool/mod.rs:149`）：`Execute` / `Read` / `Edit` / `ListDir` / `Search` / `WebFetch` / `WebSearch` / `IntegrationSearch`（MCP 发现）/ `UseTool`（MCP 调用）/ `MemorySearch` / `SentMessage` / `Skill` / `Other`，同样用 `delegate_tool!` 转发（`blocks/tool/mod.rs:171-189`）。

### 2.3 `blocks/` 各类块职责

| 块 | 文件 | 职责与关键行为 |
| --- | --- | --- |
| `UserPromptBlock` | `blocks/user.rs:84` | 用户 prompt；支持 bash/skill/cron/interjection 变体（`block.rs:490-520`）；折叠保留 3 行（`blocks/user.rs:17` `COLLAPSED_MAX_LINES`）；slash token 单独着色（`blocks/user.rs:39`） |
| `AgentMessageBlock` | `blocks/agent.rs:11` | 流式 markdown：`push_chunk` / `finish`（`blocks/agent.rs:46/67`），内嵌图片/视频引用与 Mermaid 检测 |
| `ThinkingBlock` | `blocks/thinking.rs:124` | 三态折叠（见 §8.1）；collapsed 头部 `Thought for Xs` + `(ctrl+e to expand)`（`blocks/thinking.rs:18`）；默认 `Truncated`、结束自动 `Collapsed`（`blocks/thinking.rs:554/558`） |
| `ToolCallBlock` + 13 子块 | `blocks/tool/*.rs` | 各类工具调用；共用 header 选择区 id `TOOL_HEADER_RANGE = 0`（`blocks/tool/mod.rs:47`）；`VerbGroupKind` 定义聚合动词/名词表（`blocks/tool/mod.rs:81-143`） |
| `SubagentBlock` | `blocks/subagent.rs:45` | 子 agent 生命周期；doc 明确「always collapsed and not foldable」（`blocks/subagent.rs:42-43`） |
| `BgTaskBlock` | `blocks/bg_task.rs:36` | 后台任务三态 Started/Completed/Failed；「always collapsed, not foldable, groupable, selectable」（`blocks/bg_task.rs:33-34`） |
| `WorkflowBlock` | `blocks/workflow.rs:28` | workflow 运行卡片：`WorkflowBlockStatus`（Running/Done/Failed/Cancelled/Paused）+ phases + active_agents（`blocks/workflow.rs:13-36`） |
| `SessionEventBlock` | `blocks/session_event.rs:603` | 结构化会话事件；`SessionEvent` 约 30 个变体（轮次完成/取消/失败、压缩、重试、磁盘满、hook 判定、recap…，`blocks/session_event.rs:49-190`）；不可选、不可折叠（`blocks/session_event.rs:47`） |
| `MemoryCaptureBlock` | `blocks/session_event.rs:195` | debug 专用的记忆捕获折叠视图 |
| `SystemMessageBlock` | `blocks/system.rs:10` | 任意系统文本，muted 样式 + 换行（`blocks/system.rs:20-30`） |
| `CancelledBy` | `blocks/cancel_cause.rs:11` | 取消原因枚举，映射到 banner 文案（策略类原因优先，未知不写「by user」，`blocks/cancel_cause.rs:1-2`） |
| `BtwBlock` | `blocks/btw.rs:16` | `/btw` 旁问：金色 accent，折叠只留 `/btw <question>` 头部（`blocks/btw.rs:1-2`） |
| `ContextInfoBlock` | `blocks/context_info.rs:21` | `/context` 快照：100 格分类色条 + 图例（`blocks/context_info.rs:18-19`），渲染时重新取主题色 |
| `QuoteBarStrip` | `blocks/quote_bar.rs:19` | 把 markdown 解析器生成的 `│` 引用条从「可选区」中排除（`blocks/quote_bar.rs:1-5`，`quote_prefix_selectable` :131） |
| `MarkdownContent` | `blocks/markdown_content.rs:40` | agent / thinking / btw 共享的流式 markdown 渲染 + 换行缓存（键为 `(width, generation, theme)`，`blocks/markdown_content.rs:23-37`） |
| `MermaidContent` | `blocks/mermaid_content.rs` | Mermaid 图表骨架与 affordance 行 |

## 3. 数据流细节

### 3.1 块 → 行

`output()` 内部完成三件事：markdown 渲染、按 `ctx.width` 换行、打 selection 元数据。以 `SystemMessageBlock` 为例，`word_wrap_lines(styled_lines, ctx.width as usize)`（`blocks/system.rs:26-30`）产出的每一行直接封装成 `BlockLine`。行的元数据结构：

```rust
// types.rs:146
pub struct BlockLine {
    pub content: Line<'static>,          // 绘制的 span 序列
    pub wrap: WrapMode,                  // Word / Character / Truncate (types.rs:16)
    pub selectable: Selectable,          // All / Spans(range) / None (types.rs:72)
    pub selection_range: Option<u16>,    // 逻辑选择分组 id；组头保留 u16::MAX
    pub selection_text: Option<String>,  // 复制文本覆盖（工具头等）
    pub joiner: Option<String>,          // 软换行连接符，复制时替代 '\n'
    pub background / bg_start_col / indent_width / link_target: …,
}
```

* 复制文本由 `derive_selection_text`（`types.rs:289`）推导：优先 `selection_text` 覆盖，其次按 `Selectable` 取 span 子集，`All` 会 trim 尾部空白。
* 软换行分组靠 `joiner`：`prewrap_index_per_row`（`types.rs:605`）用「joiner 为空则逻辑行 +1」把后换行行映射回源码行，是搜索命中定位到行的基础。

### 3.2 行 → 单元格

`EntryRenderer` 是唯一往 Buffer 写单元格的地方（`wrappers/entry_renderer.rs:561`）：

1. 计算 `content_width = area.width - chrome_width - timestamp_reserved`（:551-556）；
2. `entry.ensure_cached(content_width, …)` 取缓存输出（:557）；
3. 背景：按 `BlockBackground` 逐列填 padding/content/right_pad，flat 模式（minimal）退回 `Color::Reset`（:604-670，`fallback_bg` :110）；
4. accent 列：仅 `display_mode != Collapsed` 时画（:698 附近）；运行中的块用 `AccentStyle::animated` 画波纹（`WAVE_SPEED = 0.15` 弧度/tick，`wrappers/entry_renderer.rs:20`）；
5. 内容行：从 `skip_rows` 开始逐行 `Buffer::set_line`（局部可见条目直接渲染，不需要 scratch buffer——见 §4.5）。

### 3.3 横向布局

```rust
// layout.rs:19
Layout::horizontal([
    Constraint::Length(1),               // accent（恒定 1 列，layout.rs:17）
    Constraint::Length(block_pad_left),  // LayoutConfig
    Constraint::Min(1),                  // content → BlockContext.width
    Constraint::Length(block_pad_right),
]).areas(area)
```

```
viewport 宽 W
┌─1─┬─pad_l─┬───────────── content ─────────────┬─pad_r─┐
│ ▎ │       │  ← 传给 BlockContext.width 的就是这里 │       │
└───┴───────┴────────────────────────────────────┴───────┘
 accent       （选中边框另画：selection_area 向左右各扩 1 列，layout.rs:75）
```

`HorizontalLayout::for_row(y, height)` 复用同一列几何生成行级布局（`layout.rs:97`）。

### 3.4 每帧时序

```
ScrollbackState::prepare_layout(width, height)   state/mod.rs:1344
   ├ 视口尺寸变化 → bump_generation()（链接图失效，:1352）
   ├ 缓存缺失 / 宽度变化 → 全量重建（捕获 ScrollAnchor，:1370）
   ├ settle_visible_measurements（估算→精确，state/layout.rs:745）
   ├ handle_follow_mode（跟随到底，state/nav.rs:1024）
   └ 更新 total_height
ScrollbackPane::render_with_scratch               scrollback_pane.rs:139
   ├ AllTurns → render_with_sticky_headers(:294) / SingleTurn → render_single_turn(:277)
   ├ state.paint_window(...) → 只遍历可见窗口（state/layout.rs:1498）
   ├ 每个条目 → EntryRenderer::render → Buffer
   └ 返回 RenderOutput{selection_box, scroll_info, selection_model, link_overlay, …}
```

## 4. 虚拟化与增量渲染

### 4.1 布局缓存（`LayoutCache`，`state/layout.rs:42`）

```rust
pub(super) struct LayoutCache {
    entries: Vec<EntryLayoutInfo>,       // height + gap_after + 分组标志（state/types.rs:94）
    entry_truncated_heights: Vec<u16>,   // sticky 头部的 min_height 用
    measured: Vec<bool>,                 // true=精确测量，false=廉价估算
    virtual_y: Vec<usize>,               // 每个 entry 的累计 Y（前缀和）
    prompt_descriptors: Vec<PromptDescriptor>,
    groups: Vec<GroupSpan>,              // 折叠模型的权威描述
    width: u16,
}
```

`EntryLayoutInfo.height` 是**已换行后的行数**，`gap_after` 是条目间空行：相邻两个可密集相接的折叠块为 0，否则为 1（`gap_after_between`，`state/layout.rs:1555-1566`）。

### 4.2 只渲染可见窗口

`ScrollbackState::paint_window`（`state/layout.rs:1498`）→ `compute_paint_window`（:1568，纯函数、可单测）：

* 用 `partition_point` 在 `virtual_y` 上二分求 `[vp_start, vp_end)` 覆盖的条目，O(log n)（:1584-1604）；
* 起点回退一格以容纳「起点之上但越过视口顶的行」（:1585-1595）；
* 若窗口内存在**组头行**，把窗口右界扩到该 run 的末尾，让聚合标签能看到屏外成员（:1606-1615）——这是唯一会越过视口边界的情形；
* 返回 `(paint_range, content_y0)`，`content_y0` 让渲染不必从历史顶部累加（`state/layout.rs:1506-1510` 注释：「a viewport window does not re-walk off-screen history」）。

### 4.3 惰性精确测量

条目高度初始是估算值，只有接近视口才精确测量：

* `measurement_window`（`state/layout.rs:593`）：首个可见条目 + 其后 `MEASURE_MARGIN_ENTRIES = 8` 个（下方余量，无上方余量）；
* `measure_window_exact`（:669）：对未测量条目真正调 `output()` 量高；隐藏行（height 0）与合成组头跳过；
* `settle_visible_measurements`（:745）：循环「测量→重建 virtual_y→重锚视口」直到窗口内全部精确；跟随模式重新钉到底，非跟随模式只在内容缩短时 clamp；
* `warm_measure_pages_above`（:796）：resume 后向上预热 `RESUME_WARM_PAGES = 3` 屏，避免上滚时高度跳变；
* 估算本身便宜：`estimate_source_lines`（`entry.rs:286`）用缓存的**逐行显示宽度** `Vec<u32>` 做 `ceil(w / content_width)` 求和（`wrapped_lines_from_widths`，`entry.rs:48-62`，饱和到 `u16::MAX`）。

### 4.4 缓存粒度与失效

`ScrollbackEntry` 持有四层缓存（`entry.rs:88-102`），失效语义刻意分离：

| 缓存 | 键 | 失效方法 |
| --- | --- | --- |
| `cached_output: RenderedBlockOutput` | `(width, raw, theme, is_selected, cwd)`（`entry.rs:11-18`） | `invalidate_cache` / `invalidate_width_caches` |
| `cached_truncated_height` | `(content_width, raw, theme, cwd)`（`entry.rs:23`） | 同上 |
| `cached_estimate_lines` | `(content_width, lines)` | 同上 |
| `cached_line_widths` | 与宽度无关 | **只**被 `invalidate_cache` 清 |

```rust
// entry.rs:246 / 252
pub fn invalidate_cache(&mut self) {
    self.invalidate_width_caches();
    *self.cached_line_widths.borrow_mut() = None;   // 内容变了才丢逐行宽度
}
pub fn invalidate_width_caches(&mut self) {          // resize 路径
    *self.cached_output.borrow_mut() = None;
    *self.cached_truncated_height.borrow_mut() = None;
    *self.cached_estimate_lines.borrow_mut() = None;
}
```

`ensure_cached` 会把「只有部分块在意选中态」的知识编码进缓存键：仅 UserPrompt / ToolCall / Thinking / BgTask / Subagent 的 `is_selected` 参与键，其余块归一化为 false，避免选中移动导致缓存抖动（`entry.rs:326-332`）。离屏驱逐走 `evict_render_cache`（`entry.rs:261`）：丢掉重渲染输出，但**保留**高度缓存与布局，滚回来时透明重算。

`ScrollbackState` 侧另有两个代际计数：`generation`（视口/链接位置变化即 +1，供 `VisibleLinkMap::is_stale`）与 `content_generation`（只在增删条目或内容变化时 +1，供搜索语料缓存复用）（`state/mod.rs:196-203`）。

### 4.5 滚动时的性能手段汇总

| 手段 | 位置 | 效果 |
| --- | --- | --- |
| 只遍历 paint window | `state/layout.rs:1568` | O(log n) 定位 + O(可见条目) 绘制 |
| `skip_rows` 裁剪局部可见条目 | `wrappers/entry_renderer.rs:35-40`、`render.rs` 计算 `skip_rows`（:325-345） | 消除部分可见条目的 scratch buffer 渲染 |
| `ScratchBuffer` 复用 | `render.rs:69-100` | sticky 推挤裁剪复用同一 Buffer，不每帧分配 |
| 离屏重渲染缓存驱逐 | `state/layout.rs:627`（保留窗口 ±`EVICT_KEEP_MARGIN_ENTRIES = 128`） | 长会话回收数百 MB |
| 脏高度集合 | `state/mod.rs:74` `dirty_heights`；`update_dirty_entry_heights` `state/layout.rs:981` | 流式追加只重算脏条目 |
| 增量 virtual_y 修补 | `patch_virtual_y_for_dirty`（`state/layout.rs:1100`）、`extend_layout_cache_with_new_entry`（:1189） | 流式增长不触发 O(n) 全量重建（`gaps_may_be_dirty`，`state/mod.rs:166-170`） |
| 每帧高亮字符串复用 | `render.rs:275-276`、`highlight_text` 复用（:500-502） | 搜索高亮每帧至多一次分配 |
| 逐帧流式 markdown 冻结前缀 | `blocks/markdown_content.rs:23-37`（`frozen_pre_wrap_count`） | 已换行前缀不重复换行 |
| 批量插入 | `begin_batch` / `end_batch`（`state/mod.rs:161-165`） | 批量 push 时跳过 turns/布局重建 |

### 4.6 滚动锚点

两套锚点解决两类位移：

* `ScrollAnchor { entry_idx, logical_line, sub_rows }`（`state/layout.rs:16`）——宽度变化时按**逻辑行**重新定位（resize 后换行行数变了，绝对行号会指错内容），仅在非跟随模式且 `scroll_offset > 0` 时捕获（`state/mod.rs:1370-1375`）；
* `StructuralScrollAnchor { id: EntryId, rows_into_span, armed_scroll_offset }`（`state/layout.rs:26`）——增删条目这类会移动索引的突变，用稳定 `EntryId` 记录，下次 `prepare_layout` 消费（`state/mod.rs:1361-1363`），并在期间发生过显式导航时不生效。

## 5. 粘性元素（`sticky.rs`）

`sticky.rs` 是**纯 1D 坐标计算**，不关心块的内部结构（`sticky.rs:1-17`）：每个 prompt 压缩成 `PromptDescriptor { entry_idx, y_virtual, full_height, min_height, sticky }`（`sticky.rs:23`）。

```
正常钉住：                        下一 prompt 逼近（push 过渡）：
┌────────────────────────────┐   ┌────────────────────────────┐
│ ◆ user prompt A（pinned）   │   │ ◆ prompt A 被向上推（clip_top>0）│ ← pushed
│                            │   ├────────────────────────────┤
├─ HEADER_CONTENT_GAP = 1 行 ─┤   │ 正文…（下一 prompt 尚未顶到 0 行）│
│ 正文内容…                   │   └────────────────────────────┘
└────────────────────────────┘
```

* 取「最后一个已被滚过（`y_virtual < scroll_offset`）的 sticky prompt」作为 `pinned`（`sticky.rs:252-264`）；
* `calculate_render_height`（:339）：`render_height = full_height - scroll_past`，与滚动 **1:1** 收缩，下限 `min_height`（`MAX_TRUNCATED_HEADER_HEIGHT = 6` 作为估算种子，`state/types.rs:71`）、上限视口高——1:1 保证内容底行不跳；
* 下一 prompt 的裸行号 ≤ `render_height + HEADER_CONTENT_GAP` 时进入 push 过渡，当前头变 `pushed{ render_height, clip_top }` 并逐渐被推出屏幕；`next_naive_row == 0` 时头部消失、下一 prompt 接管（`sticky.rs:277-330`）；
* `MIN_PINNED_HEIGHT = 4`（`sticky.rs:45`）；
* 关闭条件：`scrollback.display.sticky_headers` 为 false 或 prompt 处于 compact 模式（`scrollback_pane.rs:310-315`）。

## 6. 跟随尾部（follow）

* 状态：`follow_mode: bool`（`state/mod.rs:106`）+ `follow_preserve_scroll`（一次性标记，`state/mod.rs:108-111`）+ 页面翻转预留 `pin_reserve_*`（`state/mod.rs:113-136`）。
* 打开时每帧 `handle_follow_mode`（`state/nav.rs:1024`）→ `follow_scroll_to_bottom`（:1052）把 `scroll_offset` 钉到 `max_scroll_offset()`（内容不足一屏时返回 0，顺带修正被缩短内容卡住的越界偏移，:1084-1088）。
* **用户上滚即暂停跟随**：`scroll_up` 无条件 `self.follow_mode = false`（`state/nav.rs:356-360`）。其余显式导航（翻页、跳转、折叠、turn 导航）同样关闭跟随（`state/nav.rs:106/218/248/486`）。
* **重新进入跟随**：向下滚动已到底后再滚一次（overscroll）才重新开启，且需配置 `follow_by_overscroll` 为 true；`rows > 0` 与 `pin_reserve_pad == 0` 是防止「恰好落到最底」被误判为过滚的守卫（`state/nav.rs:361-380`）。`enable_follow_mode` / `enable_follow` / `enable_follow_with_preserve` 提供程序化入口（:522/531/538）。
* 跟随还会自动选中最后一个可选条目，但仅当「当前无选中」或「选中本来就在尾部」时才移动，避免覆盖用户在中部的选中（`state/nav.rs:1034-1047`，受 `follow_auto_select` 门控）。
* `pin_reserve`（`state/pin_reserve.rs:1-2`）：把「最后一个 user prompt 顶到视口顶部」这种页面翻转位姿变成合法滚动底——否则 clamp 到内容高度会把视图弹回尾部。它在某轮结束后释放（`note_pin_reserve_turn_finished`，:30）。

## 7. 搜索 / 选择 / 复制

### 7.1 搜索

* 语料：`ScrollbackSearchIndex` 为每个条目缓存一段 `searchable_text`（markdown 块给渲染纯文本，其余给源文本），整体是 `Arc<[IndexedEntry]>`；只在 `content_generation` 变化时重建（`search.rs:44-93`；`searchable_text` 定义见 `block.rs:1000-1030`）。
* 扫描在后台线程 `SearchDaemon`（`search.rs:200`）：UI 线程只发「最新语料 + 查询」的 O(1) 消息，`poll` 逐帧取结果；若结果属于已被取代的查询（`request_generation`/`query` 不匹配）则丢弃，避免匹配数与光标错位（`search.rs:378-398`）。空查询与非法正则立即清空匹配（:333-341）。
* 结果模型：`ScrollbackMatch { entry_id, line_in_entry, byte_range }`（`search.rs:32`）；`next/prev` 用 `rem_euclid` 环绕（:401-417）。
* 高亮是**逐帧后处理遍**：对每个可见行把 span 文本拼进复用的 `highlight_text`，再调 `paint_match_highlights`（`render.rs:500-512`）；因此高亮集合按屏幕字形计算，可能少于索引匹配集合（源码里的 markdown 标记在屏上不存在）。
* 入口与导航：vim 模式下 `/` 打开增量搜索栏（`app/agent_view/panes.rs:137-144`；测试 `links.rs:2183`），`n`/`N` 与上下箭头推进（`panes.rs:147-156`），搜索栏占用底部 2 行（`search_reserved_rows`，`panes.rs:160`）。

### 7.2 二维选择模型

每帧渲染时，`render.rs` 把可见行解析成一个扁平模型：

```
屏幕 (col,row)
   │ hit_test_selectable_range          text_selection.rs:296
   ▼
RangeHit { entry_idx, range_id, block_line_idx, col_within_range }   text_selection.rs:199
   ▲
   │ 构建
ResolvedSelectionModel {                                             text_selection.rs:80
  ranges: Vec<ResolvedSelectableRange { entry_idx, range_id, lines }>,   :136
  visible_blocks: Vec<VisibleBlockGeometry { area, content_area,        :187
      selection_area, content_width, top_clipped, bottom_clipped }>,
}
ResolvedSelectableLine {                                             text_selection.rs:143
  entry_idx, range_id, block_line_idx,   // 逻辑身份（跨滚动稳定）
  screen_y, screen_x, selectable_cols,   // 屏幕几何
  text,                                  // 逻辑复制文本
  painted_region,                        // 实际画出的字形（列吸附用）
  joiner_to_previous,
}
```

三个坐标维度：**条目**（entry_idx）+ **逻辑选择区间**（`range_id`，块内从 0 递增，合成组头固定 `u16::MAX`，`render.rs:32`）+ **块内行号**（`block_line_idx`，滚动不会改变它，`text_selection.rs:202-204` 注释）——这是「锚点滚出屏幕仍能复制全文」的基础。

拖拽状态机：`PendingTextDrag`（按下时记录 anchor + 锚点块的 `content_width` 快照）→ 超过 `drag_threshold_exceeded`（:436）→ `ActiveTextDrag { anchor, head, kind }`（:219）；松手后由 `PersistentTextSelection`（:273）保留高亮，遇到别处点击 / Esc / 任何 scrollback 导航才清除（:271-272）。

### 7.3 鼠标：拖拽、双击/三击、边缘自动滚动

* `SelectionOrigin { Drag, DoubleClick, TripleClick }`（`text_selection.rs:234`）：双击取词、三击取行；词边界由 `word_boundaries_at_col` + `configured_word_separators` 决定（:1153/1186，默认分隔符 `DEFAULT_WORD_SEPARATORS`，:1150）。
* URL 有专门识别，并剥掉尾部标点（`url_range_at_col`，:1296；`TRAILING_URL_PUNCT`，:1249）。
* 拖到边缘触发定时自动滚动：`compute_autoscroll`（:40）在距内容区上下沿 `EDGE_THRESHOLD = 2` 行内返回方向与速度，速度按越界距离分档 1/2/3/5 行每 tick（`speed_for_distance`，:70-77）。
* 语义选择（`semantic_selection_at`，:1350）处理双击/三击在软换行连接处的扩展。

### 7.4 复制

* 路径优先级（`app/agent_view/selection.rs:719` `reconstruct_drag_copy`）：表格几何匹配 → 表格 TSV；否则用锚点块的完整输出 `reconstruct_full_selection_text_with_boundaries`（`text_selection.rs:1016`）——即从块缓存重新切片，**不依赖当前可见行**；若块已不可取则退回可见模型的 `reconstruct_selection_text`。
* 列语义：吸附与切片都基于 `painted_selectable_region`（`types.rs:322`，实际画出的字形），逻辑文本与绘制文本不一致时（工具头覆盖文本）按 bidi 状态分流处理（`text_selection.rs:1076-1096`）；范围覆盖到行尾时剥掉渲染用的尾部填充（与 `derive_selection_text` 的 trim 一致）。
* boundary 侧车：`SelectionBoundary{prefix, suffix, has_empty_row_anchor}`（`types.rs:440-492`）记录「可见行省略掉的源码字节」，复制时按选区是否触达首/末列决定贴回，保证复制仍是完整源码（含 markdown 表格边框外的空白行锚点）。
* 剪贴板落点：`copy_to_clipboard` → `crate::clipboard::copy_text_or_file` + toast，并有防抖版本 `copy_to_clipboard_debounced`（`app/agent_view/notices.rs:248-267`）。
* 键位：`y` 复制块内容、`Y` 复制命令/路径（`actions/defaults.rs:352-370`）。

### 7.5 表格选择（`table_geometry.rs`）

`TableGeometry` 从已渲染的行里**反推**箱线表格几何：`junction_cols`（竖线显示列，个数 = 列数 + 1）+ `rows`（每个逻辑行的块行范围，单元格内换行会跨多行）（`table_geometry.rs:19-30`）。`SelectionKind` 因此有三态（`text_selection.rs:243`）：

* `Linear`——普通线性选择，扫过整行；
* `TableCell`——锚点吸附在同一单元格列带内；
* `TableGrid { anchor: CellRef, head: CellRef }`——整格矩形，复制为 TSV。

`detect` 无法证明是表格时一律回退 `Linear`（`table_geometry.rs:1-2`）；几何在 promote 时冻结，若之后流式重排导致行号漂移，会重新检测并要求完全一致（`app/agent_view/selection.rs:734-738`）。

### 7.6 链接与 OSC 8

* 每帧从 `LinkOverlay`（markdown 超链接 + web_search/web_fetch 引用 URL）重建 `VisibleLinkMap`（`link_map.rs:40`），`VisibleLink.rects` 允许一个逻辑链接跨多行（:14-18）。
* 失效靠代际：`is_stale(current_generation)`（:52）；视口尺寸变化在 `prepare_layout` 里 `bump_generation`（`state/mod.rs:1352-1355`）。
* 点击命中 `link_at(col,row)`（:46）；`looks_like_bare_url_text`（:25）用于区分「屏上直接显示 URL」与「短标签/宽引用块」，决定提示文案。

## 8. 折叠 / 展开 / 截断

### 8.1 三态显示模式

```rust
// types.rs:63
pub enum DisplayMode { Collapsed, Truncated, #[default] Expanded }
```

块的折叠是**状态机**，由 trait 提供（`block.rs:92-127`）：

* 默认：`Collapsed ↔ Expanded`；
* thinking：运行中 `Collapsed|Truncated → Expanded`、`Expanded → Truncated`；结束后 `Collapsed ↔ Expanded`（`blocks/thinking.rs:532-544`），`collapse_mode` 运行中给 `Truncated`（:546-552），默认模式 `Truncated`（:554），结束自动 `Collapsed`（:558）；
* `display_mode_pinned`（`entry.rs:73`）用于「用户手动折过」的块，配合 `respect_manual_folds` 让重排不再自动展开（配置见 `docs/user-guide/03-keyboard-shortcuts.md:68-70`）。

### 8.2 分组折叠：两个家族

`state/groups.rs` 单次扫描同时决定所有折叠，产出权威的 `GroupSpan`，再 `project_to_layout` 写回每 entry 的标志（`state/groups.rs:1-10/62-76`）：

```rust
// state/groups.rs:39
pub enum GroupKind {
    VerbRun { members: usize },                        // 主动折叠的动词 run：聚合标签
    Truncation { participants: usize, hidden: usize }, // 预算截断："N more"
}
```

* **VerbRun**：连续同类工具调用（+ 已完成的折叠 thinking）折成一行，标签由 `verb_group_header_label` 生成，动词/名词表在 `VerbGroupKind`（`blocks/tool/mod.rs:81-143`，如 `VerbGroupKind::verb` :110 / `noun` :132）；`group_tool_verbs` 是总开关。
* **Truncation**：过长的密集 run 按 `max_visible` 预算隐藏，头部显示「N more」或聚合标签（`GroupHeaderLabel::{VerbRun, Truncation}`，`state/verb_group.rs:150`）。
* 渲染层：合成组头行高 1，标签行本身是可选的一行（`range_id = GROUP_HEADER_RANGE_ID = u16::MAX`，`render.rs:466-490`），复制得到聚合文本；`EntryLayoutInfo.is_group_header()`（`state/types.rs:113`）是所有消费者的唯一判据。
* 选中被隐藏条目时 `fixup_hidden_selection` 把选中移到最近可见条目，优先组头（`state/layout.rs:157-180`）；用户展开某个组记在 `expanded_groups`（`state/mod.rs:191-194`）。

### 8.3 超长输出与「显示更多」

* `Truncated` 模式：`ExecuteToolCallBlock::render_with_truncation`（`blocks/tool/execute.rs:471`）接受 `Some((first, last))`，取前 `first_lines` + `…` + 后 `last_lines`；默认 `first_lines = 2`、`last_lines = 3`（`xai-grok-pager-render/src/appearance/config.rs:613-614`），用户可在 `pager.toml` 调整（配置项注释 :1136-1139）。
* `Collapsed` 模式：单行摘要（工具头 + 结果预览），Execute 的折叠头在有 description 时隐藏命令行（`blocks/tool/execute.rs:317/379`）。
* `Expanded`：全文（`truncate = None`，`blocks/tool/execute.rs:660-667`）。流式中的 `!` 命令默认流式显示尾部、结束后可展开（`blocks/tool/execute.rs:32/964-973` 测试）。
* 查看器兜底：Enter / Ctrl-F 打开块查看器看完整输出（`actions/defaults.rs:382-386`），这是「显示更多」的最终出口。
* 高亮类块有显式上限防止卡死：diff 高亮 `EDIT_HL_MAX_BYTES = 2 MiB`、`EDIT_HL_MAX_LINES = 50 000`（`blocks/tool/edit.rs:46/49`）。

### 8.4 键位（均有 registry 证据）

| 动作 | 键 | 证据 |
| --- | --- | --- |
| 折叠 / 展开选中条目 | `h` / `Left`、`l` / `Right` | `actions/defaults.rs:265/278`；`docs/user-guide/03-keyboard-shortcuts.md:61-62` |
| 切换折叠 | `e` | `defaults.rs:291`；同上 :63 |
| 全部展开/折叠 | `⇧E` | `defaults.rs:306`（`ToggleExpandAll`）；同上 :64 |
| 展开/折叠全部 thinking | `Ctrl+E` | 同上 :65；另见 `blocks/thinking.rs:16-17` |
| 切换 raw markdown | `r` | `defaults.rs:336`（`ToggleRaw`）；同上 :66 |
| 行滚动 | `Ctrl+K` / `Ctrl+J` | `defaults.rs:182/195` |
| 半页 / 整页 | `Ctrl+U` / `Ctrl+D`、`PageUp` / `PageDown` | `defaults.rs:208/221/238/251` |
| 复制内容 / 命令·路径 | `y` / `Y` | `defaults.rs:352/367` |
| 打开块查看器 | `Enter` / `Ctrl+F` | `defaults.rs:382-386` |

## 9. 导出（旁路）

`export.rs` 把块序列转成人类可读 Markdown：只保留 `## User` / `## Assistant` / `## Tools`，Thinking / System / SessionEvent / BgTask / Subagent 等「chrome」被跳过；连续 assistant 消息合并到一个标题下，工具调用压成一行摘要（`export.rs:1-6/14-66`）。这是块模型「内容与呈现分离」的一个额外好处：导出不需要经过渲染管线。

## 10. 对本项目的观察（调研者观察，非源码结论）

* **块 = 纯函数 + 显式上下文**：`output(ctx)` 无副作用，`ctx` 携带 `mode / is_running / width / raw / max_lines / is_selected / cwd`（`types.rs:98-111`）。jev-harness 若把 Jev 的概率判定结果（Choice 概率、置信度）也放进类似 ctx 或块的 `accent/background` 决策里，渲染层就能直接表达「不确定」，而无需塌缩成布尔。
* **行级 selectable/joiner 元数据**是可复制的解耦点：一份内容同时服务绘制、搜索、复制、导出四种用途。
* **缓存键必须显式列出影响输出的输入**（width/raw/theme/selected/cwd）——否则主题切换或 cwd 变化会静默渲染陈旧内容（`entry.rs:11-18`）。
* **纯算术的 sticky/布局模块可单测**（`sticky.rs:1-7`、`compute_paint_window` 是自由函数），这对没有终端可测的 CI 很关键。

## 关键结论（供合稿引用）

1. 块抽象是 `BlockContent` trait（`block.rs:40`）+ `RenderBlock` 枚举（`block.rs:298`，13 变体），枚举用 `delegate_block!` 转发全部方法，仅 `output()` 被拦截以补 bullet —— 块是纯函数，渲染到 Buffer 由另一个 trait（`Renderable`）负责。
2. 工具块在 `RenderBlock::ToolCall` 下再分 `ToolCallBlock` 13 个变体（`blocks/tool/mod.rs:149`），第二级同样用宏转发，新增工具只需加变体 + 实现文件。
3. 关键分层决策：`BlockContent::output()` 产出的 `BlockLine` **已经是换行后的屏幕行**（`render.rs:498` 注释、`blocks/system.rs:26-30` 换行调用），因此布局层只做高度算术，不做文本排版。
4. 每行自带四类语义元数据：`Selectable`（可选列）、`selection_range`（逻辑选择分组）、`selection_text`（复制覆盖）、`joiner`（软换行连接符）（`types.rs:146-169`），一套数据同时服务绘制/搜索/复制/导出。
5. 虚拟化核心是 `LayoutCache` 的 `virtual_y` 前缀和 + `compute_paint_window` 的 `partition_point` 二分（`state/layout.rs:42/1568`），每帧只遍历可见窗口，且组头行会向 run 末尾扩窗以便聚合标签统计屏外成员。
6. 高度是「估算 → 接近视口才精确」的两级制：`measured: Vec<bool>` 标记、`MEASURE_MARGIN_ENTRIES = 8` 的下方余量、`settle_visible_measurements` 迭代收敛（`state/layout.rs:593/669/745`），resume 时向上预热 `RESUME_WARM_PAGES = 3` 屏。
7. 缓存分四层且失效分离：`invalidate_width_caches` 保留与宽度无关的 `cached_line_widths`；离屏驱逐 `evict_render_cache` 只丢重渲染输出、保留高度与布局（`entry.rs:246/252/261/627`）。
8. 粘性头部是纯 1D 计算：`pinned` 头部高度随滚动 1:1 缩小到 `min_height`，下一个 prompt 逼近时转为 `pushed`（`clip_top > 0`）被推出屏幕，间隙恒为 1 行（`sticky.rs:244/339`、`HEADER_CONTENT_GAP` :92、`MIN_PINNED_HEIGHT` :45）。
9. 跟随尾部在用户上滚时立即关闭（`scroll_up` 置 `follow_mode = false`，`state/nav.rs:356`），只有滚到底再「过滚」才重新开启（受 `follow_by_overscroll` 门控，`state/nav.rs:361-380`）；`pin_reserve` 用一次性底部预留让「最新 prompt 顶到视口顶」成为合法滚动位姿（`state/pin_reserve.rs:1-2`）。
10. 选择是三维坐标（entry + 逻辑 range_id + 块内行号）+ 屏幕几何的二维模型（`text_selection.rs:143/199`），复制时从块缓存对完整输出重新切片（`reconstruct_full_selection_text_with_boundaries`，`text_selection.rs:1016`），因此锚点滚出视口仍能复制全文。
11. 折叠有两族：主动的 VerbRun 聚合（「Read 3 files」）与预算截断的「N more」（`state/groups.rs:39`），二者由单次扫描产出 `GroupSpan` 并统一投影到 `EntryLayoutInfo`，组头行作为合成可选行（`range_id = u16::MAX`）参与复制。
12. 搜索把语料缓存成 `Arc<[IndexedEntry]>` 交给后台 `SearchDaemon`，按 `content_generation` 失效，逐帧 `poll` 丢弃过期请求的结果；高亮是按屏幕字形的逐帧后处理遍（`search.rs:44/200/378`、`render.rs:500`）。
