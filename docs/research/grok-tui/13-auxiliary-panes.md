# 专题 13：侧边与辅助面板

> 专题：侧边与辅助面板｜调研日期：2026-09-22｜证据范围：`crates/codegen/xai-grok-pager/src/views/{todo_pane,tasks_pane,timeline,agent_status,agent,subagent_catalog_pane,goal_detail,location,workflows,turn_status,progress_bar,dock}.rs`、`src/app/subagent.rs`、`src/app/subagent/lifecycle.rs`、`src/app/agent_view/{render,panes,subagent_takeover,viewer,workflows_overlay}.rs`、`src/app/{mouse,modals}.rs`、`src/scrollback/{state/types,state/timeline,state/nav,blocks/subagent}.rs`、`src/actions/defaults.rs`、`src/settings/defs.rs`、`src/slash/commands/{tasks,timeline}.rs`、`crates/codegen/xai-grok-pager-render/src/glyphs.rs`、`docs/user-guide/{16-subagents,19-plan-mode,20-background-tasks}.md`

Grok Build 的 agent 屏幕是**单列纵向堆叠**：所有辅助面板都插在顶部状态栏与 scrollback 之间，或 scrollback 与 prompt 之间。唯一占用列宽的是 timeline rail（它顶替 scrollbar 的位置）。这一点对自建 TUI 很关键——它意味着面板的开关不会改变主内容区的水平布局，只改变其高度。

## 13.1 屏幕分区与堆叠顺序

`AgentViewLayout::compute` 把每个「行」按固定顺序压成 `ratatui` 的垂直 `Constraint` 列表（`src/views/agent.rs:228-299`）。顺序即视觉顺序：

```
┌─StatusBar────────────────────────────────────────────────────────┐
│  chips: [plan] [Goal: Executing] 12.3k tok  ◆ 2  P 1  ⠋ MCP (2/3)│
├──────────────────────────────────────────────────────────────────┤
│ tasks pane    (present only when visible)                        │
├──────────────────────────────────────────────────────────────────┤
│ catalog pane  (present only when visible)                        │
├──────────────────────────────────────────────────────────────────┤
│ todo pane     (present only when visible; max 10 rows)           │
├──────────────────────────────────────────────────────────────────┤
│                                                                  │
│ SCROLLBACK    Constraint::Min(5)   <-- the only flexible row     │
│                                                                  │
├──────────────────────────────────────────────────────────────────┤
│ btw / queue / turn_status / banner / cta / follow_ups / dock     │
│   (each is 0 or N rows; a 1-row gap precedes it when vpad > 0)   │
├──────────────────────────────────────────────────────────────────┤
│ prompt                                                           │
├──────────────────────────────────────────────────────────────────┤
│ status_line (0..n)  /  shortcuts (1)                             │
└──────────────────────────────────────────────────────────────────┘
```

要点与证据：

- 顺序是**固定的**，不是可配置的分栏：`tasks → catalog → todo → scrollback → btw → queue → turn_status → banner → cta → follow_ups → dock → prompt`（`src/views/agent.rs:228-381`）。
- scrollback 是唯一的 `Constraint::Min`，其余都是 `Length`。注释明确说明求解器把 `Min` 排在每个 `Length` 之前，所以「总需求超出屏幕时，被压缩的是别的行」（`src/views/agent.rs:92-94`、`:246`）。
- 面板之间用 `pane_gap` 分隔，仅在 `top_vpad > 0` 时为 1 行（`src/views/agent.rs:231-243`）。
- 短终端（`area.height <= SHORT_TERMINAL_ROWS = 16`）时强制砍掉 CTA、follow-ups 与底部 vpad，保护 prompt 与 scrollback（`src/views/agent.rs:89-91`、`:205-219`）。
- 聚焦面板被鼠标点击时靠 `PaneAreas::hit_test` 判定，**tasks 优先于 catalog，catalog 优先于 todo，todo 优先于 queue**（`src/views/agent.rs:61-88`）。

## 13.2 各面板的尺寸、可见性与开关方式

| 面板 | 高度策略 | 默认 | 开关 | 证据 |
| --- | --- | --- | --- | --- |
| tasks | `min(MAX_TASKS_HEIGHT=8, 15% 视口)`，视口 < 12 行时返回 0；搜索/过滤条额外 +1 行 | 隐藏 | `Ctrl+G`；**运行中任务由 0→N 时自动弹出** | `src/views/tasks_pane.rs:657-658,1052-1072,911-925`；`src/actions/defaults.rs:16-55` |
| todo | `min(MAX_TODO_HEIGHT=10, 15% 视口)`；可见但无条目时返回 1 行 | 隐藏 | `Ctrl+T`，**不自动弹出** | `src/views/todo_pane.rs:154-158,270-281,218-223`；`src/actions/defaults.rs:531-543` |
| catalog | `min(MAX_CATALOG_HEIGHT=8, 15% 视口)`，视口 < 12 行时返回 0 | 隐藏 | 未找到打开入口 | `src/views/subagent_catalog_pane.rs:69-70,173-187` |
| timeline rail | 固定 2 列，顶替 scrollbar | 隐藏 | 设置项 `show_timeline` 或 `/timeline` | `src/views/timeline.rs:17-23,61-76`；`src/settings/defs.rs:533-548`；`src/slash/commands/timeline.rs:6-24` |
| goal detail | 居中圆角浮层，`area.width < 20 \|\| area.height < 6` 时不绘制 | 隐藏 | 点状态栏 goal chip；`Esc` / `g` 关闭 | `src/views/goal_detail.rs:3,405-418`；`src/app/mouse.rs:69-79` |
| dock | 最多 `MAX_DOCK_ROWS = 8` 行 | 隐藏（远端 `dock_enabled` 才开） | `Ctrl+G`；dock 开启时它是 tasks 面板的替代品 | `src/views/dock/mod.rs:2,22,45-56`；`src/app/agent_view/mod.rs:839-841` |
| turn status | 固定 1 行，idle 且无后台工作时为 0 | 隐藏 | 自动 | `src/views/turn_status.rs:725-736` |

### tasks 面板的自动开关逻辑（最值得抄的一条）

`TasksPane::sync` 每帧被调用（`src/app/agent_view/render.rs:992-997`），其中：

```rust
if running_count > 0 && self.prev_running_count == 0 {
    self.overlay.show();
    self.opened_by_auto = true;
}
if running_count == 0
    && counts.paused_workflows == 0
    && self.overlay.visible
    && !self.overlay.focused
    && self.opened_by_auto
    && !self.show_done
{
    self.overlay.visible = false;
    self.opened_by_auto = false;
}
```
（`src/views/tasks_pane.rs:911-927`）

三个细节值得注意：

1. **只在 0→N 的边沿弹出**，不在 N→N+1 时重弹；
2. **只关自己开的**（`opened_by_auto`）；用户自己 `Ctrl+G` 打开的面板不会被系统收走；
3. **replay 恢复的历史任务不算数**，否则 resume 一个旧会话会立刻弹面板（`src/views/tasks_pane.rs:903-909`）。

`on_state_change` 里把 `opened_by_auto` 清回 `false`，所以任何手动操作都会「接管」这个面板（`src/views/tasks_pane.rs:1045-1050`）。

### 面板的通用 chrome：聚焦框 + 关闭控件

四个侧栏共用 `OverlayState { visible, focused, fullscreen }` 三态机（`src/views/overlay.rs:47-131`）。聚焦时画一圈 `theme.selection_border`，hover 时画更淡的 `theme.hover_border`，两者都不满足就**完全不画**——面板在非聚焦时保持无框的「原生」外观（`src/views/agent.rs:574-625`）。`[✗]` 关闭按钮的命中矩形由渲染时回写（`src/app/agent_view/render.rs:1853-1869`），点击走 `overlay.escape()` 并把活动面板退回 scrollback（`src/app/mouse.rs:34-47`）。

## 13.3 Todo / plan 的呈现

### 状态图标与配色

待办条目只有四种状态，图标与文本样式成对定义（`src/views/todo_pane.rs:25-91`）：

| 状态 | 图标 | 图标色 | 文本样式 |
| --- | --- | --- | --- |
| `Pending` | `□` | `text_primary` | `text_primary` |
| `InProgress` | `▶` | `warning` | `text_primary` + **BOLD** |
| `Completed` | `✓`（`glyphs::check_mark()`） | `accent_success` | `gray_bright` |
| `Cancelled` | `✗`（`glyphs::ballot_x()`） | `accent_error` | `gray_bright` + **CROSSED_OUT** |

「进行中」靠**加粗 + 警示色**从灰色完成项里跳出来，而不是靠移动或动画——面板本身不做逐条动画。

### 层级与完成度

- **没有层级**：`TodoItem` 只有 `content` / `priority` / `status` / `meta` 四个字段（`crates/codegen/xai-grok-tools/src/implementations/grok_build/todo/mod.rs:143-150`），**pane 只渲染 `content` + `status`，`priority` 与 `meta` 完全不参与渲染**；`TodoListEntry` 的字段只有 `id/item/styled/status_style`（`src/views/todo_pane.rs:58-92`）。
- **顺序保持模型给的顺序**，注释明确写「no reordering by status」，条目 id 就是原始下标，因此 `ListPaneState` 的选择能跨重建存活（`src/views/todo_pane.rs:295-309`）。
- **完成度不做进度条/分数**；`TodoCounts { in_progress, pending, completed, cancelled }` 只用于空列表时的占位文案（`src/views/todo_pane.rs:143-152,390`）：

| 条件 | 文案 |
| --- | --- |
| 一个条目都没有 | `No todo items.` |
| 全完成 | `All done.` |
| 只有取消 | `2 cancelled.` |
| 混合 | `5 done. 1 cancelled.` |

（四项断言见 `src/views/todo_pane.rs:421-448`。）

### 更新时的视觉反馈

**没有闪烁、没有 toast、没有插消息**。数据入口是 ACP 的 `SessionUpdate::Plan`，整批替换：

```rust
} else if let acp::SessionUpdate::Plan(plan) = notif.request.update {
    let items: Vec<_> = plan.entries.into_iter().map(todo_item_from_plan_entry).collect();
    agent.todo.update_todos(items);
```
（`src/app/acp_handler/mod.rs:244-252`）

而 `update_todos` 的注释把产品决策写得很直白：

```rust
/// Replace all todo items (called from ACP Plan handler).
///
/// Does NOT auto-show the todo pane. Users toggle it with Ctrl-T.
```
（`src/views/todo_pane.rs:218-223`）

`render()` 每帧 `rebuild_entries()`，在条目数 < 20 的典型规模下明说「cheap」（`src/views/todo_pane.rs:368-386`）。可视反馈完全来自「下一次重绘时列表内容变了」这一事实，加上「进行中」的粗体。

### 面板内交互

- `h` 切换显示/隐藏已完成与已取消项（仅在输入栏未打开时生效），默认 `show_done = true`（`src/views/todo_pane.rs:208,263-265,314-320`）。
- 小视口限速：视口 0–5 行时每次滚动只走 1 行，6–10 行走 2 行，避免 3–5 行的滚动增量「一次跳过整屏」（`src/views/todo_pane.rs:332-344`）。
- 面板可搜索/过滤/复制（`search_enabled: true, copy_enabled: true`），但不跟随、不换行、不跳行（`src/views/todo_pane.rs:190-199`）。

### plan 走的是另一条路

Plan mode 的产物**不进 todo 面板**：它写到会话目录的 `plan.md`，由 `exit_plan_mode` 触发一个独立的可滚动审批浮层（`a` 批准 / `s` 请求修改 / `c` 行内评论 / `y` 复制 / `q` 放弃），见 `docs/user-guide/19-plan-mode.md`。状态栏另有一个 `plan` chip，点击可重新打开预览（`src/app/agent_view/render.rs:1276-1282`、`src/app/mouse.rs:89-107`）。

## 13.4 后台任务与 workflow

### 一个列表承载四类工作

`TasksPane` 把 bg task、subagent、scheduled `/loop`、workflow run 合并成一个可折叠列表（`src/views/tasks_pane.rs:1-5`），按 `GroupKind` 分组：

| 组 | 成员 | 组内顺序 |
| --- | --- | --- |
| `Workflows` | workflow run | 运行中优先，再按开始时间倒序 |
| `Subagents` | 非 workflow 的 subagent | 先按 agent type 字母序，再按开始时间倒序 |
| `Tasks` | 一次性后台命令 | 开始时间倒序 |
| `Watchers` | `monitor` 任务 + `/loop` 定时任务 | monitor 恒在 loop 之前 |

排序是四段比较器：**类型分组 → 组内运行中优先 → 组内时间倒序（Agent 先按 type 字母序）→ `stable_id` 兜底**，最后一条是为了「等时间戳的行不会逐帧重排」（`src/views/tasks_pane.rs:843-883`、`type_order()` 见 `:580-597`）。

分组表头是可折叠行，样式 `▾ Subagents 2` / `▸ Subagents 2`，其中「chevron + 空格」的宽度恰好等于条目的 2 空格缩进，所以标签能对齐（`src/views/tasks_pane.rs:535-552,611-617`）。Enter / `Ctrl+F` / 点击切换折叠，`←` / `→` 分别折叠与展开；折叠状态在重建显示列表时保持表头仍被选中，光标不跳（`src/views/tasks_pane.rs:960-987`）。**组被清空后其折叠状态会被遗忘**，新任务到来时该组重新以展开态出现，避免藏在过期的折叠表头下（`src/views/tasks_pane.rs:885-897`）。

### 单行的视觉层次

```
  ⋅  Task npm test · running                    (1.2k)  12s   [↗] [✗]
  ⸬  Explore 找到调用点 · Running: rg          3m10s  grok-4.6  [↗] [✗]
  │  │                                            │      │       │    │
  │  │                                            │      │       │    └ kill  [✗]
  │  │                                            │      │       └───── view  [↗]
  │  │                                            │      └───────────── model / context badge
  │  │                                            └──────────────────── line-count badge + elapsed
  │  └────────────────────────────────────────────────────────────────── styled label (single Line)
  └───────────────────────────────────────────────────────────────────── icon: spinner / ✓ / ✗ / ⏸
```

- **图标**（`render_*_overlay` 系列）：运行中 = `dot_spinner_frames()` 点状 spinner 按 `tick / SPINNER_DIVISOR(4)` 取帧；完成 = `✓` + `accent_success`；失败 = `✗` + `accent_error`；workflow 暂停 = `⏸` + `warning`（`src/views/tasks_pane.rs:32,1344-1414,1416-1466,1558-1602`）。
- **右侧按钮**：`[✗]`（`glyphs::ballot_x_button()`）与 `[↗]`（`glyphs::enlarge_button()`），各占 3 列，从右向左依次摆放；hover 时颜色从 `gray` 变亮（`src/views/tasks_pane.rs:1489-1535,1630-1676`）。
- **文案布局**：右侧从右到左是「kill → view → 耗时 → 行数徽标/上下文徽标 → model」；bg task 显示 stdout 行数徽标 `(2.0k+)`（截断而非四舍五入，所以徽标从不夸大；`+` 表示「至少这么多」，`src/views/tasks_pane.rs:108-131`）；subagent 显示 `format_context_badge` 的 `resumed` / `forked` 徽标与 model（`src/app/subagent.rs:919-925`）。
- **防透字**：因为按钮是叠绘在 label 之上的，绘制前先 `clear_overlay_area`，并且在清除前检测被覆盖位置是否已有文字，若有则在左邻格补一个 `…`，样式取自左边那格的颜色——用户看到的是「被截断了」而不是「莫名少了几个字」（`src/views/tasks_pane.rs:698-728`）。
- 这个面板**关掉了通用的右下角滚动指示器**，改成在预留行上居中画 `▲` / `▼`，只在确实需要时预留该行（`src/views/tasks_pane.rs:760-764,1200-1254`）。

### 失败态

失败没有单独的面板或红框：`BgTaskStatus::Failed` / `SubagentFinished.status != "completed"` 只体现为**红色 `✗` 图标 + label 降饱和**（`src/views/tasks_pane.rs:333-347,1456-1464`）。workflow 的终态判断更细：`is_terminal()` 覆盖 `interrupted | complete | failed | cancelled`，非 active 且非 terminal 视为「暂停」，计入状态栏的 `P N`（`src/views/workflows.rs:60-65`；`src/views/tasks_pane.rs:1026-1029`）。

### 任务的状态汇总（常驻状态栏）

任务数不进 transcript，而是作为右对齐的 chips 常驻在顶部状态栏（`src/views/agent_status.rs:141-182`）：

- `◆ N` = 运行中的 bg task + 非 workflow subagent + scheduled + active workflow 之和（`src/views/tasks_pane.rs:1007-1031`），用**静态**实心菱形而非 spinner，注释解释原因：「表头全程在视野内，这里的 spinner 读起来是噪声而不是进度」（`src/views/agent_status.rs:166-168`）；
- `P N` = 暂停的 workflow 数；
- 两者都为 0 时整条 chip 返回 `None`，即**不渲染**。

同一批 chip 还是鼠标目标：点 `◆ N` 切换 tasks 面板，点 goal chip 切换 goal/workflow 浮层（`src/app/mouse.rs:53-79`，命中矩形回写于 `src/app/agent_view/render.rs:1384-1385`）。

## 13.5 子代理的呈现

### 父会话里的就地块

子代理在**父的 scrollback** 里只有一个块，`SubagentAttemptInfo::scrollback_entry_id` 记住它的 `EntryId`，运行期间靠这个 id 就地改状态与活动后缀（`src/app/subagent.rs:80-92`）。doc 头把两种模式写得很清楚（`src/scrollback/blocks/subagent.rs:1-11`）：

- **阻塞式（sync）**：一个 `Started` 块，运行时闪烁、结束时变绿/变红。文本 `Subagent "description"`；
- **后台式（async）**：`Started` 块永久保留（转灰），结束时**另加**一个 `Completed` / `Failed` 块。文本 `Subagent started: "…"`、`Subagent completed in 43s: "…"`。

运行中把子代理的实时活动（"Thinking"、"Running: cargo test"、"Compacting"）作为后缀就地更新，所以「不打开子代理视图也能看到交互式进度」（`src/scrollback/blocks/subagent.rs:63-66`）。

### 父子层级与并发数量

- **层级深度为 1**：只有顶层会话能 spawn，子代理再调 `spawn_subagent` 会因 depth-limit 失败（`docs/user-guide/16-subagents.md`，Depth Limits 一节）。
- 因此列表里**不需要树形缩进**：subagent 条目是一层平铺，靠 `type_label`（persona > role > subagent_type > tag > `general`，首字母大写）排序分组，而不是靠缩进表达父子（`src/app/subagent.rs:961-996`；`src/views/tasks_pane.rs:852-866`）。
- **并发上限没有硬编码在 pager 里**：配置项是 `subagents_max_concurrent`（`crates/codegen/xai-grok-config-types/src/lib.rs:847`）与 `workflow_max_concurrent_agents`（同文件 `:854`）。pager 侧只做计数与呈现，未在本次调研范围内找到默认数值——**未找到默认值的证据**。

### 进入子代理输出

三条路径，最终都走 `open_subagent_fullscreen(child_sid)`：

1. 在父 scrollback 的子代理块上按 `Enter` / `Ctrl+F`（`src/app/agent_view/subagent_takeover.rs:56-72` 从选中行的 `RenderBlock::child_session_id` 取 id）；
2. 在 tasks 面板里按 `[↗]`、双击行、或 Enter（鼠标路径见 `src/app/mouse.rs:608-704`，键盘路径见 `src/app/agent_view/panes.rs:816-1030`）；
3. dashboard 会话行（`src/app/dispatch/dashboard.rs:456`）。

打开时若子视图的 scrollback 还空着（或子代理已完成），会先 replay 子会话的 `updates.jsonl` 再显示（`src/app/agent_view/subagent_takeover.rs:33-52`）。

### 子代理全屏视图长什么样

它**不是分屏，而是替换父视图的边框画面**：`render_bordered_frame` 画圆角边框，标题行放全部状态，框内递归调用子视图自己的 `draw`（`src/app/agent_view/subagent_takeover.rs:93-343`）：

```
╭─ ⸬ Explore 找到调用点 grok-4.6     Running: rg · 3m10s    3m10s  [✗] ────────────╮
│                                                                                  │
│   <child transcript: its own scrollback / thinking / tool calls>                 │
│                                                                                  │
│                                                                                  │
╰──────────────────────────────────────────────────────────────────────────────────╯
```

标题行元素：状态图标（spinner / `✓` / `✗`）→ 类型标签 → **加粗**描述 → model → 右侧依次为上下文徽标 → 活动 `· 耗时` → `[✗]` 关闭按钮（`src/app/agent_view/subagent_takeover.rs:145-315`）。父视图的面板在打开期间全部让位：tasks / catalog / todo 高度强制为 0（`src/app/agent_view/render.rs:1014-1028`），composer 也是 0 行——这个视图是**纯观察态**，不能输入、不能排队（`docs/user-guide/16-subagents.md`，Fullscreen framed view 一节）。

### 如何返回

- `q` 或 `Esc`（仅当子视图处于「裸 scrollback」状态，即没有打开搜索等子状态）；搜索打开时第一下先关搜索（`src/app/agent_view/subagent_takeover.rs:378-387`）；
- 点 `[✗]`（`src/app/agent_view/subagent_takeover.rs:361-369`）；
- `Ctrl+Q` **永不被消费**，始终冒泡到全局退出（`src/app/agent_view/subagent_takeover.rs:358-360`，注释在 `:7` 把这条列为不变量）。

所有关闭路径都收敛到 `close_subagent_fullscreen`，它顺带对已完成的子视图执行 `evict_finished_child_view`，保证「恰好一次」（`src/app/agent_view/subagent_takeover.rs:73-80`）。被拒绝的根级快捷键（命令面板、模型选择器等）在这里是**静默重绘，无 toast**——fail closed（`docs/user-guide/16-subagents.md`）。

### subagent catalog 面板

`SubagentCatalogPane` 渲染 `BundleState` 里分组的 `Personas / Roles / Agents` 三段，表头不可选中，条目可搜索/过滤，`Personas` 与 `Roles` 条目会尾接 ` · 描述`（`src/views/subagent_catalog_pane.rs:106-161,25-49`）。空态文案 `No bundled items.`（`:254`）。

**注意**：在本快照的全部 `src/` 中**未找到**任何把 `catalog.overlay.visible` 置为 `true` 的路径（对 `.catalog.overlay` 的引用只有 `focused = false`、`escape()` 与按键处理）。也就是说，这个面板的打开入口在本快照中不可达——**未找到证据**，不宜写成「快捷键 X 打开」。

## 13.6 Timeline：turn 级导航轨道

### 它是什么抽象

Timeline 不是「滚动位置的第二种画法」，而是**会话的 turn 索引**。一个 `Turn` = 一条用户 prompt 到下一次 prompt 之前的全部内容：

```rust
pub struct Turn {
    pub prompt_index: usize,   // UserPrompt 条目的下标
    pub end_index: usize,      // 排他
    pub status: TurnStatus,    // Running | Completed | Failed | Cancelled
}
```
（`src/scrollback/state/types.rs:23-31`）

模块头一句话点明与 scrollback 的关系：

```rust
//! Timeline sidebar: a tick rail (one tick per turn) that replaces the scrollbar in its gutter while enabled.
//! Tick position encodes conversation order, not scroll proportion.
```
（`src/views/timeline.rs:1-3`）

所以 `TurnStatus` 在 rail 上**不参与渲染**（`compute_rail` 只吃 turn 数量与视口位置），rail 是纯导航器。

### 几何与门槛

- `RAIL_WIDTH = 2` 列；终端宽 `< MIN_TERMINAL_WIDTH(60)`、turn 数 `< MIN_TURNS(2)`、或当前不是 `ViewSurface::Root` 时宽度为 0（`src/views/timeline.rs:17-23,61-76`）。理由写在注释里：窄终端上「transcript 比导航器更需要那些列」，且「1 个 turn 的时间线是噪声」。
- rail **顶替 scrollbar**：`timeline_x = (scrollbar_x + 1) - timeline_width`，而 `scrollbar_cfg.enabled == false` 时 rail 宽度被强制为 0——它需要 scrollbar 的 gutter 几何（`src/views/agent.rs:382-393`）。
- 几何每帧算一次成 `TimelineRail`，渲染与鼠标命中**共用同一份数据**，因此「显示和动作不可能不一致」（`src/views/timeline.rs:4,26-46`）。
- turn 数超过行数时开一个窗口：在底部优先贴尾（保住最新 tick），但**永不排除视口顶部那个 turn**，否则没有 tick 会高亮（`src/views/timeline.rs:109-128`）。chevron 与 tick 栈整体垂直居中，「和 web 版 rail 一致」（`:130-135`）。

### 视觉

```
   rail columns (RAIL_WIDTH = 2, replacing the scrollbar gutter)
   |--|
      ▴     up_y     previous-turn chevron; a dim glyph means the click is a no-op
      ──             inactive tick: only the RIGHT cell is drawn (" ─" at rail.rect.x)
      ━━             active tick = the turn owning the viewport top ("━━", legacy ConHost "══")
      ──
      ▾     down_y   next-turn chevron (jump_to_turn)
```

- 活动 tick 用 `glyphs::timeline_tick_active()` = `"━━"`（legacy ConHost 用 `"══"`）；hover 用 `"──"`；非活动只是**最右格的一小段浅色横线**（`" ─"`，`src/views/timeline.rs:228-242`；字形见 `crates/codegen/xai-grok-pager-render/src/glyphs.rs:222,232`）。
- **不画轨道底色**：注释说深色轨道条在 tick 少的时候读起来像一条尴尬的空带，所以 rail 直接画在 scrollback 背景上（`src/views/timeline.rs:185-187`）。
- chevron 的「变暗」与点击的无操作由同一个 `chevron_target` 决定，所以**变暗的 chevron 一定是 no-op**（`src/views/timeline.rs:153-161,197-213`）。
- ▲ 的目标是「严格位于视口顶部之上的最近 turn」，不是 `active - 1`；▼ 是「视口顶部之下最近的 turn」（`src/views/timeline.rs:36-42`；取数见 `src/scrollback/state/timeline.rs:85-104`）。
- **hover 预览卡**：悬浮在 rail 左侧，圆角边框，最多 2 行（末行省略号），宽度按显示宽度钳在 `16..32` 列；终端太矮就不画（`src/views/timeline.rs:248-331`）。内容是该 turn 的首个非空行，上限 120 字符（`src/scrollback/state/timeline.rs:5-36,61-70`）。
- hover 状态在浮层打开时被清空（`overlay_blocks_rail_hover`：jump / rewind / blocking card / block viewer），避免浮层下还留着悬停高亮（`src/app/agent_view/render.rs:1151-1187`）。

### 开关

设置项 `show_timeline`（Appearance 分类，描述：「Per-turn tick rail in place of the scrollbar: hover previews a turn, click jumps to it.」），minimal 模式下隐藏该设置（`src/settings/defs.rs:533-548`）；`/timeline` 斜杠命令切换它，minimal 模式返回 `ModeSupport::FullscreenOnly`（`src/slash/commands/timeline.rs:12-24`）。**没有默认快捷键**——`actions/defaults.rs` 中没有 timeline 相关 Action。

### 与 `/jump` 的关系

两者共用同一份 turn 数据：`timeline_entries()` 返回每 turn 的 `{turn_idx, prompt_entry_id, preview}`，其中 `prompt_entry_id` 是稳定 id（不是下标），因此条目的插入/删除不会让导航指向别的块（`src/scrollback/state/timeline.rs:9-59`）。`/jump` 选择器与 timeline rail 是这份数据的两个消费者（`src/app/dispatch/jump.rs:21`、`src/app/agent_view/jump.rs:153`）。

## 13.7 刷新时机与「不打断主流程」

这一节是本题最有复制价值的部分——Grok Build 系统性地避免「用消息表达状态」。

**手法 1：状态只在非 transcript 表面上更新。**
用户手册把这条写成显式规则：运行中的计数**只**活在 idle 时的 still-running 状态行上；完成事件才在 transcript 里落一个 `Task completed` chip；`Worked for` 之类的时间标记保持朴素——**transcript 从不复述或重述这些计数**（`docs/user-guide/20-background-tasks.md`，The Still-Running Status Line 一节）。对应的实现是 `turn_status::format_still_running`（`src/views/turn_status.rs:108-125`）：

```
◎ 1 command · 2 monitors · 1 loop · 1 subagent still running
```

**手法 2：单行高度在 0 / 1 之间切换，不占位也不弹层。**
turn status 行的显示条件是 `parked || !state.is_idle() || drain_blocked || session_starting_since.is_some() || watchers.total() > 0`（`src/views/turn_status.rs:725-736`）——它靠 layout 里那一行出现/消失来「通知」，而不是往对话里插一条系统消息。列表本身的高度也用同一套路：`desired_height` 隐藏时返回 0（`src/views/tasks_pane.rs:1052-1058`、`src/views/todo_pane.rs:270-273`）。

**手法 3：就地更新而非追加。**
子代理运行时只有 1 个块，靠 `scrollback_entry_id` 就地改 bullet 颜色/活动后缀（`src/app/subagent.rs:80-92`、`src/scrollback/blocks/subagent.rs:1-11`）。目标状态 chip（`[Goal: Executing] 12.3k/50k tokens 3m`）是状态栏 item，每帧重建，从不进 transcript（`src/views/agent_status.rs:267-327`）。

**手法 4：每帧从单一数据源重建视图模型，不做增量 patch。**
`TasksPane::sync` 每帧从头构造 `items` → 排序 → `rebuild_entries()`（`src/views/tasks_pane.rs:786-958`）；`TodoPane::render` 每帧 `rebuild_entries()`（`src/views/todo_pane.rs:368-386`）；`SubagentCatalogPane::sync_from_bundle` 每帧全量重建（`src/views/agent_view/render.rs:1001-1008`）。代价由「条目数 < 20 时很便宜」承担，换来的是**不存在视图与数据不同步的窗口**。唯一保留的是 `highlight_cache`（bash 语法高亮）与「按 stable_id 保持选择」两处必要缓存（`src/views/tasks_pane.rs:687`、`src/views/todo_pane.rs:295-309`）。

**手法 5：动画靠单一 tick 驱动，只在有活干时才走。**
`TasksPane::tick` 自增并返回「是否有运行中的条目」，`needs_tick()` 给出同样的判断（`src/views/tasks_pane.rs:1074-1085`）。subagent 全屏视图的标题 spinner 直接复用 `self.tasks.tick_count()`，不另起时钟（`src/app/agent_view/subagent_takeover.rs:155-164`）。不同场景用不同速度常量：turn status spinner `SPINNER_DIVISOR = 4`（约 7.5fps），idle 的 still-running 呼吸 `MONITOR_PULSE_DIVISOR = 8`（约为前者一半，约 1.07s 一圈），理由写明是「idle 的提示应当平静地呼吸，而不是读起来像正在转的 turn spinner」（`src/views/turn_status.rs:21-31`）。

**手法 6：屏幕太小时先砍辅助行，不砍主内容。**
`SHORT_TERMINAL_ROWS = 16` 以下强制砍掉 CTA 与 follow-ups（`src/views/agent.rs:208-219`）；`AUTO_COMPACT_MAX_ROWS = 20` 以下强制 compact（`src/views/agent.rs:95-103`）；视口 < 12 行时 tasks 与 catalog 面板直接返回高度 0（`src/views/tasks_pane.rs:1056-1058`、`src/views/subagent_catalog_pane.rs:177-179`）。

**手法 7：输入栏与叠绘按钮的冲突要显式处理。**
tasks 面板的行内按钮是叠绘在列表上的，所以渲染时先从分配区域里减去输入栏高度再叠绘，否则「spinner 会把 `search:` 画成 `⸬earch:`」（`src/views/tasks_pane.rs:1256-1263`）。同时 `desired_height` 为输入栏多要 1 行，让它有自己的一行而不是挤掉最后一条任务（`src/views/tasks_pane.rs:1065-1071`）。

## 13.8 对 jev-harness TUI 的直接启示

1. **辅助面板用「高度 0/1 的插入行」而不是浮层或分栏**，主内容区宽度不变，也就不用处理水平重排（`src/views/agent.rs:228-381`）。
2. **自动弹出的面板要记录「谁打开的」**，并且只在 0→1 的边沿弹；否则 `Ctrl+G` 这类手动开关会被系统反复争夺（`src/views/tasks_pane.rs:911-927`）。
3. **状态计数留在状态栏，事件留在 transcript**——两者不重复。这条对 Jev 也有直接价值：Jev 的概率/置信度适合做成状态栏 chip 或面板内的徽标，而不是每帧往对话插一条消息（`docs/user-guide/20-background-tasks.md`）。
4. **「进行中」靠样式（粗体 + 警示色）而非动画区分**，动画只保留一个全局 tick 且只在有活干时推进（`src/views/todo_pane.rs:25-53`、`src/views/tasks_pane.rs:1074-1085`）。

## 关键结论（供合稿引用）

1. 所有辅助面板都在**顶部状态栏与 scrollback 之间**或 **scrollback 与 prompt 之间**纵向插入，顺序固定；scrollback 是唯一的 `Constraint::Min`，空间不足时被压缩的是别的行。证据：`src/views/agent.rs:228-381`、`:92-94`。
2. tasks 面板是唯一会自动弹出的面板，且只在「运行中任务 0→N」的边沿弹，只收起自己开的实例，并排除 replay 恢复的历史任务。证据：`src/views/tasks_pane.rs:903-927`、`:1045-1050`。
3. todo 面板**绝不自动弹出**（`Ctrl+T` 手动开关），条目状态只有 `□ ▶ ✓ ✗` 四态，靠加粗与颜色而非动画区分「进行中」。证据：`src/views/todo_pane.rs:25-91`、`:218-223`。
4. todo **没有层级**，渲染只用 `content` + `status`（`priority` / `meta` 不参与渲染），也**不显示完成度进度**，计数只出现在空列表占位文案里。证据：`crates/codegen/xai-grok-tools/src/implementations/grok_build/todo/mod.rs:143-150`、`src/views/todo_pane.rs:143-152`、`:295-309`。
5. tasks 面板把四类工作合并为 `Workflows / Subagents / Tasks / Watchers` 四个可折叠组，排序为「类型分组 → 运行中优先 → 时间倒序 → stable_id 兜底」；组清空后折叠状态被遗忘。证据：`src/views/tasks_pane.rs:144-174`、`:843-897`。
6. 每行右侧叠绘 `[✗]`（kill）与 `[↗]`（view）两个 3 列按钮；叠绘前先清格，若覆盖到文字就在左邻格补 `…` 并用左侧颜色。证据：`src/views/tasks_pane.rs:698-728`、`:1489-1535`。
7. 任务计数做成常驻状态栏 chips（`◆ N` 运行中、`P N` 暂停 workflow），都是静态字形而非 spinner——「表头全程在视野内，spinner 在这里是噪声」；计数为 0 时整条不渲染。证据：`src/views/agent_status.rs:141-182`。
8. 子代理在父 scrollback 里**只有一个块**，靠 `scrollback_entry_id` 就地更新状态与活动后缀；后台式才在结束时追加第二个块。证据：`src/app/subagent.rs:80-92`、`src/scrollback/blocks/subagent.rs:1-11`。
9. 进入子代理 = 边框画面替换父视图（不是分屏），父视图的 tasks/catalog/todo 与 composer 全部归零；返回靠 `q` / `Esc`（裸 scrollback 时）或点 `[✗]`，`Ctrl+Q` 永不吞键。证据：`src/app/agent_view/subagent_takeover.rs:93-343`、`:358-409`、`src/app/agent_view/render.rs:1014-1028`。
10. 子代理嵌套深度上限为 1，所以侧栏里的子代理是**平铺列表**，靠 `type_label` 字母序而不是缩进表达关系。证据：`docs/user-guide/16-subagents.md` Depth Limits；`src/app/subagent.rs:961-996`；`src/views/tasks_pane.rs:852-866`。
11. timeline 是**一 turn 一 tick 的导航轨道**（tick 位置表会话顺序而非滚动比例），恒宽 2 列并顶替 scrollbar，仅在 `show_timeline` 开启、根视图、终端 ≥ 60 列、turn ≥ 2 时出现；它与 `/jump` 共用 `timeline_entries()` 的稳定 `prompt_entry_id`。证据：`src/views/timeline.rs:1-23`、`:61-76`；`src/views/agent.rs:382-393`；`src/scrollback/state/timeline.rs:9-59`。
12. 「不打断主流程」的核心手法是**状态只在非 transcript 表面就地更新**：运行计数只活在 idle 的 still-running 状态行上，完成才落一个 `Task completed` chip，transcript 从不再复述计数；面板每帧全量重建视图模型而非增量 patch。证据：`docs/user-guide/20-background-tasks.md`；`src/views/turn_status.rs:108-125`、`:725-736`；`src/views/tasks_pane.rs:786-958`。
