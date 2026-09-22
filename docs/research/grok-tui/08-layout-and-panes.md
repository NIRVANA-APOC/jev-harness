# 专题 08：整体布局架构与 pane 模型

> 专题：整体布局架构与 pane 模型｜调研日期：2026-09-22｜证据范围：`crates/codegen/xai-grok-pager/src/views/agent.rs`、`app/agent_view/{render,panes,input,mod}.rs`、`views/dock/{mod,layout}.rs`、`views/list_pane/{mod,layout,render}.rs`、`views/{modal_window,overlay,status_bar,timeline,todo_pane,tasks_pane,queue_pane,subagent_catalog_pane}.rs`、`views/status_line/mod.rs`、`views/dashboard/{layout,render}.rs`、`app/{mod,status_line_policy,status_line}.rs`、`app/workspace_{layout,membership}.rs`、`actions/{mod,defaults}.rs`、`xai-grok-pager-render/src/appearance/config.rs`、`tips/small_screen.rs`、`docs/user-guide/{03,05,06,23,25,26}`

## 1. 一句话结论

Grok Build 的 agent 主界面是**一个垂直堆叠的、无分栏的单列布局**：唯一弹性区域是 transcript（scrollback），其余所有区域都是 0/1/固定行高的「条」，能出现的位置由一条固定的行顺序决定；横向只有一个可选的 1 列滚动条或 2 列 timeline 轨道（`views/agent.rs:181-425`）。它的「pane」不是可拖拽分栏，而是**共享同一条横向带、靠可见性与焦点状态机切换的 overlay 面板**（`views/overlay.rs:1-20`）。

## 2. 屏幕如何被切分

### 2.1 唯一的布局算法

整屏只调用一次 `Layout::vertical`，入口是纯函数 `AgentViewLayout::compute(params)`：

- 输入：`AgentViewLayoutParams`，包含 screen `area`、`LayoutConfig`、`ScrollbarConfig`，以及**每一个可出现的行所请求的高度**（`views/agent.rs:108-138`）。
- 输出：`AgentViewLayout`，每个区域一个 `Rect`（`views/agent.rs:141-176`）。
- 计算过程：先用 outer padding 求 `inner_area`，再把各行的 `Constraint` 顺序 push 进 `Vec`，最后 `Layout::vertical(constraints).split(inner_area)`（`views/agent.rs:221-299`）。
- 横向：只有 `Constraint::Length(1)` 的滚动条列与可选 timeline 轨道；没有任何左右分栏（`views/agent.rs:382-402`）。

行顺序（`views/agent.rs:228-298`，自上而下，方括号表示条件出现）：

```
StatusBar (Length 1)
[gap 1][Tasks]          只有 tasks_height > 0
[gap 1][Catalog]        只有 catalog_height > 0
[gap 1][Todo]           只有 todo_height > 0
gap 1                   status_gap，top_vpad != 0 时才是 1
Scrollback (Min 5)      ★ 唯一的弹性行
[gap 1][BTW]
[gap 1][Queue]
[gap 1][TurnStatus]
[gap 1][Banner]
[gap 1][PluginCTA]
[gap 1][FollowUps]
[gap 1][Dock]
[prompt_gap]
[VoiceRecording]
Prompt
[status_line]
Shortcuts (Length 1)
```

**注意上半 / 下半的分界**：Tasks / Catalog / Todo 挂在 **scrollback 之上**（状态栏下方），BTW / Queue / Dock 挂在 **scrollback 之下**（输入框上方）。这一侧向归属是硬编码在约束顺序里的，没有配置项（`views/agent.rs:232-274`）。

唯一的下界由 `SCROLLBACK_MIN_ROWS = 5` 承担，注释明确说「求解器把它排在所有 `Length` 之上，所以过载时收缩的是别的行」（`views/agent.rs:92-94, 246`）。

### 2.2 默认 80×24 布局图

取默认配置：`outer_vpad = 1`、`outer_hpad_left/right = 2`（`xai-grok-pager-render/src/appearance/config.rs:181-190`），未开 compact，dock 关闭（默认关，`views/agent.rs:127-129`），无 Tasks/Catalog/Todo/CTA/follow-ups，prompt 聚焦且正文一行。

内区 = `76 列 × 22 行`；行预算固定部分 = statusBar 1 + status_gap 1 + prompt_gap 1 + prompt 3 + shortcuts 1 = 7，故 **scrollback 得到 15 行**。

```
列:   1  2  3                                         78  79 80
      ┌──┬─────────────────────────────────────────────┬──┬──┐
y=1   │  │              outer_vpad (1 行留白)            │　│　│   ← 空白，填 bg_base
y=2   │  │ StatusBar                          Length 1 │　│▓▓│   ▓ = 滚动条 (gap_right=0)
y=3   │  │ (status_gap)                       Length 1 │　│　│
y=4   │  │ ┌─────────────────────────────────────────┐ │　│▓▓│
 ..   │  │ │                                         │ │　│▓▓│
y=18  │  │ │ Scrollback   Min(5) → 实得 15 行         │ │　│▓▓│
      │  │ └─────────────────────────────────────────┘ │　│▓▓│
y=19  │  │ (prompt_gap)                       Length 1 │　│　│
y=20  │  │ ╭─────────────────────────────────────────╮ │　│　│
y=21  │  │ │ ❯ 输入                                  │ │　│　│   prompt = vpad_top(1)+1+info(1) = 3 行
y=22  │  │ ╰─ info ──────────────────────────────────╯ │　│　│
y=23  │  │ Shortcuts                          Length 1 │　│　│
y=24  │  │              outer_vpad (1 行留白)            │　│　│
      └──┴─────────────────────────────────────────────┴──┴──┘
        └2 列 hpad_left                        hpad_right┘└2 列┘
```

关键几何：

- prompt 最小高度 = `vpad_top + 1 + info_block(true)` = `1 + 1 + 1` = 3 行（`app/agent_view/render.rs:965-966`，`views/prompt_widget/mod.rs:220-222, 275-277, 1602`）。
- prompt 上限 = `area.height / 2`，即 24 行屏幕最多 12 行输入区（`app/agent_view/render.rs:806`）。
- 滚动条：`scrollbar_x = area.right() - gap_right - 1`（`views/agent.rs:382`）。80 列、`gap_right = 0` 时它落在**最后一列**，即右 padding 之内；内容宽度只在 `content_end_x < scrollback_right` 时才被裁（`views/agent.rs:394-402`），所以本例内容拿满 76 列。

### 2.3 宽屏 120×40：轨道 + todo + dock

内区 = `116 列 × 38 行`。假设 timeline 可用（`show_timeline = true`、根视图、宽度 ≥ `MIN_TERMINAL_WIDTH = 60`、轮数 ≥ `MIN_TURNS = 2`，`views/timeline.rs:17-23, 61-73`），todo 面板 4 行，dock 6 行（2 个非空 section，各 header + 2 行）。

固定部分 = 1(statusBar) + 1(pane_gap) + 4(todo) + 1(status_gap) + 1(gap) + 6(dock) + 1(prompt_gap) + 3(prompt) + 1(shortcuts) = 19 → **scrollback 得 19 行**。

```
列:   1  2  3                                        118  119 120
      ┌──┬─────────────────────────────────────────────┬───┬───┐
y=1   │  │                  outer_vpad                  │   │   │
y=2   │  │ StatusBar                                    │   │   │
y=3   │  │ (pane_gap)                                   │   │   │
y=4.. │  │ ┌─ Todos ──────────────────────────────────┐ │   │   │
y=7   │  │ └──────────────────────────────────────────┘ │   │   │
y=8   │  │ (status_gap)                                 │   │   │
y=9.. │  │ ┌──────────────────────────────────────────┐ │   │ ▌ │
y=27  │  │ │ Scrollback  Min(5) → 实得 19 行            │ │   │ ▐ │ ← rail RAIL_WIDTH = 2
      │  │ └──────────────────────────────────────────┘ │   │ ▌ │   (滚动条本帧不画)
y=28  │  │ (gap)                                        │   │   │
y=29..│  │ ┌ ▾ Subagents ─────────────────────────────┐ │   │   │
y=34  │  │ │ ▾ Tasks ─────────────────────────────────│ │   │   │
      │  │ └──────────────────────────────────────────┘ │   │   │
y=35  │  │ (prompt_gap)                                 │   │   │
y=36..│  │ ╭──────────────────────────────────────────╮ │   │   │
y=38  │  │ ╰─ info ───────────────────────────────────╯ │   │   │
y=39  │  │ Shortcuts                                    │   │   │
y=40  │  │                  outer_vpad                  │   │   │
      └──┴─────────────────────────────────────────────┴───┴───┘
```

rail 的定位：`timeline_x = (scrollbar_x + 1) - timeline_width`，即**恰好占据滚动条那一列往左的 2 列**，且 `timeline_width > 0` 意味着本帧滚动条不渲染（`views/agent.rs:383-393`）。宽度不足或轮数不足时回落为滚动条。

### 2.4 宽度方向的证据：没有分栏

全 crate 搜索 `Layout::horizontal` 只命中三类用途，都不是「视图分栏」：

- 对话框/菜单的水平居中：`views/new_worktree_dialog.rs:38-43`、`views/welcome/menu.rs:49-54`、`views/welcome/hero_box.rs:170-174`。
- scrollback 单条 entry 的内部 gutter：`accent(1) | block_pad_left | Min(1) | block_pad_right`（`scrollback/layout.rs:15-25`、`scrollback/wrappers/entry_renderer.rs:215-220`、`:603-608`）。
- prompt widget 内部的 chrome 列分配（`views/prompt_widget/mod.rs:3023-3041`）。

**未找到证据**：任何「左右分栏 / 侧栏 / 可拖拽分隔条」的布局代码或配置。

## 3. pane / dock 模型

### 3.1 可停靠面板清单

| 面板 | 归属 | 位置 | 高度来源 | 上限 |
| --- | --- | --- | --- | --- |
| Tasks（后台任务 + 子代理 + 计划任务） | 上 | scrollback 上方 | `tasks.desired_height(area.height)` | `MAX_TASKS_HEIGHT = 8` 与 `MAX_TASKS_FRACTION = 0.15` 取小（`views/tasks_pane.rs:657-658, 1052-1064`） |
| Catalog（子代理目录） | 上 | 同上 | `catalog.desired_height(area.height)` | `MAX_CATALOG_HEIGHT = 8` / `MAX_CATALOG_FRACTION = 0.15`；**`view_height < 12` 直接返回 0**（`views/subagent_catalog_pane.rs:69-70, 173-187`） |
| Todo | 上 | 同上 | `todo.desired_height(area.height)` | `MAX_TODO_HEIGHT = 10` / `MAX_TODO_FRACTION = 0.15`（`views/todo_pane.rs:155-158, 270-281`） |
| BTW（`/btw` 侧问） | 下 | scrollback 下方 | `btw_overlay::btw_panel_height(...)`（`app/agent_view/render.rs:1089-1090`） | — |
| Queue（待发提示队列） | 下 | scrollback 下方 | `queue.desired_height()` | `MAX_QUEUE_HEIGHT = 3`，`clamp(1,3)`（`views/queue_pane.rs:349, 600-605`） |
| Dock | 下 | prompt 正上方 | `dock::desired_height(&data)` | `MAX_DOCK_ROWS = 8`（`views/dock/mod.rs:22`） |

每个面板各自持有一个 `OverlayState { visible, focused, fullscreen }`（`views/overlay.rs:47-56`）。

### 3.2 dock：把三类面板合并成一条

dock 的自我描述是「prompt 上方的合并面板：每个非空 section 一个 header」，并且**由远程开关 `dock_enabled` 门控**（`views/dock/mod.rs:1-2`）。section 固定四个 + 一个队尾：

```rust
pub enum Section { Workflows, Subagents, Tasks, Watchers, Queued }  // views/dock/mod.rs:78-83
pub enum DockItem { Header(Section), Row(Section, usize), RevealRemaining(Section) }  // :120-125
```

**开关解析优先级**（`app/mod.rs:193-218`，函数名 `resolve_dock_enabled`）：托管 pin（`requirements.toml` 的 `[features] dock`）→ 环境变量 `GROK_DOCK`（未设时回落 `GROK_DOCK_V2`）→ 有效配置的 `[features] dock` → 远程 `dock_enabled` → **默认关闭**。结果写进进程级 `AtomicBool`：`views/dock::set_enabled(...)`（`app/event_loop.rs:1313-1315`、`app/acp_handler/settings.rs:154-155`）。

dock 的可见性还有个**运行时开关**：dock 开启时，`Ctrl+G`（`ActionId::ToggleTasks`）不再切换 tasks 面板，而是切换 `dock_hidden`（`app/agent_view/input.rs:1046-1062`）。这是 dock 唯一找到的用户级开关。

### 3.3 dock 开启会吞掉旧面板

这是理解 dock 模型的关键：`dock_on` 为真时，旧的 Tasks 与 Queue 面板高度被强制归零，队列正文被搬进 dock 内部。

```rust
// app/agent_view/render.rs:1009-1044
let viewer_open = self.active_subagent.is_some();
let dock_on = crate::views::dock::enabled()
    && !viewer_open
    && area.height > agent::SHORT_TERMINAL_ROWS;      // 矮屏强制关闭
let tasks_height = if viewer_open || dock_on { 0 } else { self.tasks.desired_height(area.height) };
...
let queue_height = if dock_on { 0 } else { self.queue.desired_height() };
```

配套的三处联动：

- 队列的可见性改由 dock 的展开状态决定：`self.queue.overlay.visible = self.dock_queued_expanded && self.visible_held_queue_len() > 0`（`render.rs:1030-1033`）。
- 焦点被驱逐：`dock_on && active_pane == Tasks` → 强制回 `Scrollback`（`render.rs:1037-1039`）；`dock_shown == false && active_pane == Dock` → 同样回 `Scrollback`（`render.rs:1103-1106`）。
- 队列的 rect 被**重定向到 dock 内部**：`layout.queue = dock::queue_body_rect(layout.dock, dock)`（`render.rs:1226-1231`，实现见 `views/dock/mod.rs:237-255`）。也就是说 `ActivePane::Queue` 在 dock 模式下指的是「dock 里的队列正文」。

### 3.4 空间分配：header 优先、正文按需、无栈

dock 的行分配是一个**纯函数** `grant_rows(counts, cap)`（`views/dock/layout.rs:243-337`），规则写得很明确：

1. **header 最优先**：所有非空 section 的 header 先扣除预算，正文用剩下的 `spare`（`layout.rs:256-257`）。
2. Pass 1：每个展开 section 先拿 1 行正文；若干 section 争抢时是公平轮转（`layout.rs:262-273`）。
3. 队列正文有一行「谁也不能抢」的底（`queue_floor`），否则 Queued 的 header 会展开在死区上（`layout.rs:278-281`）。
4. Pass 2：被截断的 section 再拿 1 行，用于画 `show N more`（`layout.rs:284-295`）。
5. 之后按 `share_rows` 在静息上限（`MAX_DOCK_ROWS`）内均分剩余；**只有被用户显式展开（`show_all`）的 section 才能越过静息上限增长**（`layout.rs:297-335`）。
6. 不变量 `normalize_no_lone_rows`：一个 section 要么 0 行、要么 ≥ 2 行、要么全显示——绝不留一行「说不清是什么」的行（`layout.rs:339-388`）。

调用方还做了一次双向裁剪：布局先按 `paint_cap`（`max(max_rows, floor_need)`）要行，帧实际给的高度再通过 `DockLayout::with_cap` 传回，保证既不画画超出、也不丢 header（`layout.rs:411-416, 84-88`；契约由网格化测试 `the_layout_contract_holds_across_the_grid` 穷举验证，`layout.rs:529-622`）。

**没有 tab、没有栈**：dock 的四个 section 全部同时存在（各自一个 header），只是行数按上面的算法分配；section 内的滚动是**独立 band 滚动**（`SectionSlots<usize> offsets`，`views/dock/mod.rs:154-156`），滚一个 section 不会移动别的 header。

## 4. 尺寸自适应：阈值清单

### 行数（高度）

| 常量 | 值 | 出处 | 触发行为 |
| --- | --- | --- | --- |
| `SHORT_TERMINAL_ROWS` | 16 | `views/agent.rs:91` | `area.height <= 16` 时：底部 padding 归 0、`cta_height` 与 `follow_ups_height` 强制 0、`prompt_gap` 归 0（`views/agent.rs:205-219`、`app/agent_view/render.rs:1079-1086`）；dock 强制关闭（`render.rs:1012`） |
| `AUTO_COMPACT_MAX_ROWS` | 20 | `views/agent.rs:97` | `effective_compact()` 在 `rows <= 20` 时把 compact 置为真（`views/agent.rs:99-104`）；compact 使 `eff_outer_vpad → 0`、`eff_hpad_* → 1`（`appearance/config.rs:197-217`）。**不写回用户设置**，窗口变大即恢复 |
| 编译期断言 | — | `views/agent.rs:98` | `const _: () = assert!(SHORT_TERMINAL_ROWS < AUTO_COMPACT_MAX_ROWS);` |
| `SCROLLBACK_MIN_ROWS` | 5 | `views/agent.rs:94` | transcript 的地板；过载时优先保证它 |
| `SMALL_SCREEN_TIP_MAX_ROWS` | 28 | `tips/small_screen.rs:23` | 21–28 行区间弹出一次「Tight on space? Try /compact-mode」提示（`tips/small_screen.rs:27-29`） |

### 列数（宽度）

| 阈值 | 值 | 出处 | 触发行为 |
| --- | --- | --- | --- |
| `MIN_TERMINAL_WIDTH` | 60 | `views/timeline.rs:20` | 窄于此隐藏 timeline 轨道 |
| `MIN_DASHBOARD_WIDTH` | 40 | `views/dashboard/layout.rs:10` | dashboard 落到「单列、标签中截断」的窄模式（`views/dashboard/render.rs:169-172`） |
| StatusBar `content_width < 10` | 10 | `views/status_bar.rs:53` | 顶栏整行不画内容 |

窗口宽度没有触发 agent 主视图的结构性降级（没有窄屏换布局的分支）——**未找到证据**表明 agent 视图存在横向降级；横向降级只发生在 dashboard、timeline rail 和 welcome 屏。

## 5. 布局可配置性

| 想调的东西 | 能不能调 | 位置 / 默认值 |
| --- | --- | --- |
| 上下留白、左右边距 | ✅ | `[scrollback.layout] outer_vpad = 1`、`outer_hpad_left = 2`、`outer_hpad_right = 2`（`03`→`docs/user-guide/05-configuration.md:707-713`；默认值见 `appearance/config.rs:181-190`，左/右最小值为 `MIN_HPAD = 1`，`:195, 220-228`） |
| block 内 padding | ✅ | `block_pad_left/right = 2`（同上；用于 scrollback entry 的 gutter，`scrollback/layout.rs:20-25`） |
| 滚动条开关与间距 | ✅ | `[scrollback.scrollbar] enabled = true`、`gap_left = 0`、`gap_right = 0`（`docs/user-guide/05-configuration.md:715-718`；`appearance/config.rs:248-258`） |
| 是否用 timeline 轨道代替滚动条 | ✅ | `[ui] show_timeline`，默认 `yes`（`docs/user-guide/26-config-reference.md:627`） |
| 紧凑模式 | ✅ | `[ui] compact_mode`，默认 `yes`，或 `/compact-mode`（`26-config-reference.md:593`） |
| 面板开/关 | ⚠️ 仅开/关 | `Ctrl+T` todos、`Ctrl+G` tasks（dock 开启时为 dock）、`Ctrl+;`（macOS VS Code 家族为 `Ctrl+4`）queue（`actions/defaults.rs:36-41, 533-539, 548-551`） |
| **分栏、面板宽度、面板位置、侧栏顺序** | ❌ **未找到证据** | 面板宽度永远写死为整宽（`views/agent.rs:228-298` 全部 `Constraint::Length` 都是整行高） |
| 面板高度上限 | ❌ | 全是硬编码常量（见 §3.1） |

额外的隐藏闸门：minimal 模式下动作注册表会删掉 `ToggleTasks` 且不注册 `OpenDashboard`（`actions/mod.rs:671-674, 769-774`），dock 在 minimal/矮屏也不显示。

## 6. 焦点模型

`ActivePane` 是**全屏唯一的活动 pane 标识**，7 个值（`views/agent.rs:30-41`）：

```rust
pub enum ActivePane { Scrollback, Todo, Queue, Prompt, Tasks, Catalog, Dock }
```

`app/agent_view/mod.rs:209` 里 `pub type AgentPane = ActivePane;` —— 只是别名。

### 谁持有焦点

- 字段 `self.active_pane`，唯一写入口是 `set_active_pane(target, force)`；它同时负则把其它 pane 的 `overlay.focused` 清掉，并在切出 Scrollback 时关闭搜索（`app/agent_view/input.rs:1350-1380+`）。
- **兼容子结构**：每个 overlay 面板还有自己的 `focused: bool`（`views/overlay.rs:47-56`），必须与 `active_pane` 保持一致；不一致时渲染与按键归属会分叉。此外 `dock` 用 `focused: self.active_pane == ActivePane::Dock` 现算（`app/agent_view/panes.rs:693`）。
- 有些切换会被拒绝而不是静默发生：脏的 queued-prompt 编辑会阻止切换（`app/agent_view/panes.rs:135-137`）；`ComposerRoute::Hidden` 时切到 Prompt 直接返回 `false`（`input.rs:1351-1353`）。

### 焦点如何在区域间移动

| 动作 | 键 | 证据 |
| --- | --- | --- |
| 聚焦 prompt | `Tab`（alt `i`、`Space`） | `actions/defaults.rs:469-474` |
| 聚焦 scrollback | `Tab` | `actions/defaults.rs:482-487` |
| Prompt → Dock | `Tab`，且 `dock_shown && !dock_hidden` | `panes.rs:41-48` |
| Prompt → Tasks | `Tab`，且 tasks overlay 可见 | `panes.rs:49-54` |
| Dock 内 → Prompt | `Tab`（不按 Shift） | `panes.rs:941` |
| Dock → Scrollback | `BackTab` / `Shift+Tab`、`Esc`、`q` | `panes.rs:942-945, 969-972` |
| 各 overlay 面板 → Scrollback | `Esc` / `q`，或面板自我失焦 | `panes.rs:289, 909, 1101, 1129` |
| Prompt ↔ Scrollback | `Tab` 双向 | `docs/user-guide/03-keyboard-shortcuts.md:90-98` |

要点：**没有全局的「focus ring」**，而是「谁能吃这个键」——每个面板自己处理键，失焦时返回 `OverlayAction::FocusScrollback`（`views/overlay.rs:26-38`）。滚轮不走焦点，而是按坐标路由：`pane_areas.hit_test(col, row).unwrap_or(Scrollback)`，其中 hit_test 有固定优先级 **Tasks > Catalog > Todo > Queue > Dock > Scrollback > Prompt**（`views/agent.rs:63-87`、`panes.rs:1296-1324`）。

### 「区域 = 状态机」：是

`views/overlay.rs` 的模块头自己画了这张图（原文即为 ASCII，`views/overlay.rs:8-20`）：

```
  ┌────────┐  shortcut  ┌──────────────────┐  shortcut  ┌────────┐
  │ Hidden │ ─────────► │ Visible + Focused │ ─────────► │ Hidden │
  └────────┘            └──────────────────┘            └────────┘
                          │  Tab/Space  ▲                    ▲
                          ▼             │ shortcut           │ Esc/q
                    ┌──────────────────┐                    │
                    │ Visible+Unfocused│ ───────────────────┘
                    └──────────────────┘
```

状态三元组 `OverlayState { visible, focused, fullscreen }` 被 Tasks/Todo/Catalog/Queue 共用；结构性按键（`Tab`/`Esc`/`q`/`Space`/`Ctrl-F`）由共享的 `handle_overlay_key` 处理，面板只需实现自己的内容按键（`views/overlay.rs:1-6, 47-56, 73-120`）。

### 叠加层与模态层

主视图之上还有两层：`views/modal.rs`、`app/modals.rs`、`app/agent_view/modals.rs` 提供叠加/模态，统一走单一 `ModalWindow`（边框、标题、关闭按钮、可选 tab bar、footer 快捷键），并通过全局 `EMBEDDED` 标记在 minimal 模式下退化为**无边框嵌入式渲染**（`views/modal_window.rs:1-4, 24-33`）。这解释了「为什么全是行布局也不难看」：真正的浮层由 modal 层负责居中。

## 7. 与 dashboard / 多会话界面的关系

dashboard 是**第二套完全独立的布局函数**，不复用 `AgentViewLayout`：

- `views/dashboard/layout.rs:195-199`：`compute_layout(area, peek_visible)` → `DashboardLayout`，字段 `top_margin / header / header_gap / actions / actions_gap / list / dispatch / footer / bottom_margin`（`:167-191`）。
- 它用的是**手写 y 游标 + 逐行阈值**，而不是 ratatui 约束求解：`dashboard_chrome_heights` 里每条 chrome 行都是 `u16::from(area.height > N)`，阈值从下往上依次是 `>6`(top_margin)、`>4`(header、actions)、`>10`(dispatch_gap)、`>11`(shortcuts_gap)、`>12`(header_gap)、`>13`(actions_gap)、`>=2`(footer)、`>16`(bottom_margin)（`views/dashboard/layout.rs:229-257`）。注释说明这个「每多一行屏幕就恰好点亮一条 gap」的阶梯是为了让列表高度**永不随终端变高而缩小**（`:236-237`）。
- 阈值 `> 16` 与 agent 的 `SHORT_TERMINAL_ROWS = 16` 同值，`bottom_margin` 的注释直接点名「same threshold as `views::agent::AgentViewLayout::compute`」（`views/dashboard/layout.rs:189, 243-244`）。
- dashboard 的 peek（预览面板）是**列表优先分配**：`LIST_FLOOR_ROWS = 12` 的地板先扣，peek 上限为 `⌊H × 3/8⌋`（`PEEK_MAX_FRAC_NUM/DEN = 3/8`，`views/dashboard/layout.rs:13-23, 101-150`）；peek 的 live-tail 正文再被 `MAX_LIVE_TAIL_ROWS = 28` 二次封顶（`:25-26`）。**peek 没有独立的 rect**，它画在 dispatch rect 里（`:353`）。
- dashboard 内部焦点是另一套模型，只有两个区域：**overview list ↔ dispatch input**，`Tab` 切换（`docs/user-guide/23-dashboard.md`「Focus: input bar ↔ overview list (`Tab`)」）。
- workspace 侧的 `app/workspace_layout.rs` 与题目暗示的「屏幕布局」无关：它做的是 dashboard 列表的**成员排序与分组**（pin rank、manual order、`State`/`Directory` 分组），核心类型是 `LayoutOverlay` / `WorkspaceView` / `WorkspaceGrouping`（`app/workspace_layout.rs:10-46, 200-341`）。

关系总结：dashboard 与 agent 视图共享**窗口尺寸约定的阈值**（16 行）与**视觉 padding 习惯**（`DISPATCH_OUTER_HPAD = 2`、`HEADER_OUTER_HPAD = 2`、`LIST_OUTER_HPAD = 2`，注释明确说「matches `LayoutConfig::outer_hpad_left/right = 2`」，`views/dashboard/layout.rs:152-164`），但**不共享布局代码**。多会话不是「分屏」，而是同一个全屏视图里切换 `ActiveView`（`Welcome` / `Agent(AgentId)` / `AgentDashboard`，`app/app_view.rs:202-210`）。

## 8. 对 jev-harness 的可复用点

1. **「一次垂直切分 + 唯一 Min 行」是低成本可维护的架构**：把每个区域做成「0 / 固定 n 行」的条，只让主内容区弹性，就不需要嵌套布局或手算坐标；ratatui 的 `Min` 约束天然承担过载仲裁（`views/agent.rs:246`）。
2. **阈值要集中在常量层并加编译期断言**：`SHORT_TERMINAL_ROWS < AUTO_COMPACT_MAX_ROWS` 用 `const _: () = assert!(...)` 锁死（`views/agent.rs:98`），避免后续调整阈值时出现「矮屏降级比紧凑模式还宽松」的悖论。
3. **自动降级不要写回用户设置**：`effective_compact` 只作为派生值传入渲染，窗口变大即恢复用户选择（`views/agent.rs:99-104`）。这对 Jev 门控场景也适用——运行时策略不应污染持久意图。
4. **面板状态机而非级联 if**：`OverlayState` 三态 + 共享按键处理器，是「新增一个面板」时最容易复制的模式（`views/overlay.rs`）。
5. **空间分配用纯函数 + 穷举测试**：dock 的 `grant_rows` 是纯函数，契约测试直接对 (len × expanded × opened × queued × cap) 的笛卡尔积做断言（`views/dock/layout.rs:529-622`）。把「行预算算法」从渲染里抽出来是这套代码最值得抄的一点。

## 关键结论（供合稿引用）

1. 主界面是**单列垂直堆叠**：一次 `Layout::vertical` 切完所有行，横向无分栏，唯一弹性行是 scrollback，`SCROLLBACK_MIN_ROWS = 5` 是它唯一的地板——`views/agent.rs:181-299`。
2. 上行（Tasks/Catalog/Todo）在 scrollback **之上**，下行（BTW/Queue/Dock）在 prompt **之上**，这个侧向归属是硬编码顺序，无配置项——`views/agent.rs:228-298`。
3. 三个高度阈值：`SHORT_TERMINAL_ROWS = 16`（硬砍 CTA/follow-ups/底部 padding/dock）、`AUTO_COMPACT_MAX_ROWS = 20`（派生 compact，不写回设置）、`SMALL_SCREEN_TIP_MAX_ROWS = 28`（21–28 行提示 `/compact-mode`）——`views/agent.rs:91-104`、`tips/small_screen.rs:23-29`。
4. 宽度阈值只影响非主视图元素：timeline 轨道 `MIN_TERMINAL_WIDTH = 60`、dashboard 窄模式 `MIN_DASHBOARD_WIDTH = 40`、StatusBar `content_width < 10`——`views/timeline.rs:20`、`views/dashboard/layout.rs:10`、`views/status_bar.rs:53`。
5. dock 是**远程开关特性**，优先级 pin → `GROK_DOCK`/`GROK_DOCK_V2` → `[features] dock` → 远程 `dock_enabled` → **默认关**——`app/mod.rs:193-218`。
6. dock 开启会**吞掉 Tasks 与 Queue 两个旧面板**（高度归零），并把队列正文重定向进 dock 内部（`layout.queue = queue_body_rect(layout.dock, ...)`）——`app/agent_view/render.rs:1009-1044, 1226-1231`。
7. dock 内部是 4 个固定 section（Workflows/Subagents/Tasks/Watchers）+ Queued 尾段，**无 tab 无栈**，靠行预算分配共存，section 可独立 band 滚动——`views/dock/mod.rs:78-83, 154-160`、`views/dock/layout.rs:243-337`。
8. dock 的 `MAX_DOCK_ROWS = 8` 是**静息上限**，只有用户显式展开（`show_all`）的 section 才能越过它增长——`views/dock/mod.rs:20-22`、`views/dock/layout.rs:303-335`。
9. 面板只有「开/关」，**没有任何分栏宽度/位置/顺序配置**；可配的只有主题级 padding（`[scrollback.layout]`、`[scrollback.scrollbar]`）与 `[ui] compact_mode / show_timeline / dashboard_preview / status_line.*`——`docs/user-guide/05-configuration.md:707-718`、`docs/user-guide/26-config-reference.md:593,595,627,630-631`。
10. 焦点是**单一 `ActivePane` 枚举（7 值）+ 面板各自的三态 `OverlayState`**；切换集中在 `set_active_pane`，且脏编辑会拒绝切换；滚轮按坐标优先级路由（Tasks > Catalog > Todo > Queue > Dock > Scrollback > Prompt）——`views/agent.rs:31-41, 63-87`、`app/agent_view/input.rs:1350+`、`views/overlay.rs:47-56`、`app/agent_view/panes.rs:1296-1324`。
11. 「区域 = 状态机」在源码里有自证的 ASCII 图（Hidden / Visible+Focused / Visible+Unfocused），共享按键处理器——`views/overlay.rs:1-20`。
12. dashboard 是**第二套独立布局函数**，用手写 y 游标与逐行高度阈值（`>6/>4/>10/>11/>12/>13/>=2/>16`），只与 agent 视图共享阈值约定与 2 列 padding 习惯，不共享代码——`views/dashboard/layout.rs:167-257, 293-488`；`app/workspace_layout.rs` 是 dashboard 的成员排序/分组，与屏幕分区无关——`app/workspace_layout.rs:10-46`。
