> 本文由上游专题 `08-layout-and-panes.md`、`09-scrollback-block-model.md`、`10-modals-overlays-focus.md`、`11-prompt-widget.md` 提炼、去重、按主题重组，**未引入上游之外的新事实**；4 篇上游均为只读源码调研、未运行构建。上游之间冲突的说法并列保留，上游标注的「未找到证据」保持原标注。

## 4. 界面结构与布局

### 4.1 整体布局与 pane 模型

### 本节要点

- 主界面是**单列垂直堆叠**：整屏只调用一次 `Layout::vertical`，横向没有分栏；唯一弹性行是 scrollback，唯一地板是 `SCROLLBACK_MIN_ROWS = 5`。
- 每个区域都是「0 / 固定 n 行」的条，出现位置由硬编码的行顺序决定：上行（Tasks / Catalog / Todo）挂在 scrollback **之上**，下行（BTW / Queue / Dock）挂在 prompt **之上**。
- 「pane」不是可拖拽分栏，而是**共享同一条横向带、靠可见性与焦点状态机切换的 overlay 面板**；焦点是单一 `ActivePane` 枚举，切换可被脏编辑拒绝，滚轮按坐标优先级路由。
- 三档高度阈值 `16 / 20 / 28`、三档宽度阈值 `60 / 40 / 10` 全在常量层，并用 `const _: () = assert!(...)` 锁死大小关系；宽度**不触发** agent 主视图的结构性降级。
- dock 是**远程开关特性**，启用后吞掉 Tasks 与 Queue 两个旧面板（高度归零、队列正文搬进 dock），内部按纯函数 `grant_rows` 分预算，无 tab 无栈。

**唯一的布局算法。** 入口是纯函数 `AgentViewLayout::compute(params)`：输入 `AgentViewLayoutParams`（screen `area`、`LayoutConfig`、`ScrollbarConfig`，以及每一个可出现的行所请求的高度），输出 `AgentViewLayout`（每区域一个 `Rect`）。过程是先用 outer padding 求 `inner_area`，把各行 `Constraint` 顺序 push 进 `Vec`，最后 `Layout::vertical(constraints).split(inner_area)`（`views/agent.rs:108-176, 221-299`）。横向只有 `Constraint::Length(1)` 的滚动条列与可选 timeline 轨道，没有任何左右分栏（`views/agent.rs:382-402`）。行顺序（`views/agent.rs:228-298`，方括号表示条件出现）：

```
StatusBar (Length 1)
[gap 1][Tasks] · [gap 1][Catalog] · [gap 1][Todo]        各仅当对应 height > 0
gap 1 (status_gap；top_vpad != 0 时才是 1)
Scrollback (Min 5)                                       ★ 唯一的弹性行
[gap 1] BTW · Queue · TurnStatus · Banner · PluginCTA · FollowUps · Dock   同上，条件出现
[prompt_gap] · [VoiceRecording] · Prompt · [status_line] · Shortcuts (Length 1)
```

**默认 80×24 布局图。** 默认 `outer_vpad = 1`、`outer_hpad_left/right = 2`（`xai-grok-pager-render/src/appearance/config.rs:181-190`），未开 compact，dock 默认关闭，无 Tasks/Catalog/Todo/CTA/follow-ups，prompt 聚焦且正文一行。内区 = `76 列 × 22 行`；固定部分 = statusBar 1 + status_gap 1 + prompt_gap 1 + prompt 3 + shortcuts 1 = 7，故 **scrollback 得 15 行**。

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

关键几何：prompt 最小高度 `vpad_top + 1 + info_block` = 3 行（`app/agent_view/render.rs:965-966`、`views/prompt_widget/mod.rs:220-222, 275-277`）；prompt 上限 `area.height / 2`，24 行屏幕最多 12 行输入区（`app/agent_view/render.rs:806`）；`scrollbar_x = area.right() - gap_right - 1`，80 列、`gap_right = 0` 时落在**最后一列**（右 padding 之内），内容只在 `content_end_x < scrollback_right` 时才裁，故本例内容拿满 76 列（`views/agent.rs:382, 394-402`）。

**宽屏 120×40：轨道 + todo + dock。** 内区 = `116 列 × 38 行`；timeline 可用条件为 `show_timeline = true`、根视图、宽度 ≥ `MIN_TERMINAL_WIDTH = 60`、轮数 ≥ `MIN_TURNS = 2`（`views/timeline.rs:17-23, 61-73`）；todo 4 行、dock 6 行（2 个非空 section，各 header + 2 行）。固定部分 = 19 行 → **scrollback 得 19 行**。

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

rail 定位：`timeline_x = (scrollbar_x + 1) - timeline_width`，即恰好占据滚动条那一列往左的 2 列，且 `timeline_width > 0` 意味着**本帧滚动条不渲染**；宽度或轮数不足时回落为滚动条（`views/agent.rs:383-393`）。**宽度方向没有分栏**：全 crate 的 `Layout::horizontal` 只命中三类用途——对话框/菜单的水平居中（`views/new_worktree_dialog.rs:38-43`、`views/welcome/menu.rs:49-54`）、scrollback 单条 entry 的内部 gutter（`scrollback/layout.rs:15-25`）、prompt widget 内部的 chrome 列分配（`views/prompt_widget/mod.rs:3023-3041`）；**未找到证据**：任何「左右分栏 / 侧栏 / 可拖拽分隔条」的布局代码或配置。

**可停靠面板清单**（每面板各持一个 `OverlayState { visible, focused, fullscreen }`，`views/overlay.rs:47-56`）：

| 面板 | 归属 | 高度来源与上限 |
| --- | --- | --- |
| Tasks（后台任务 + 子代理 + 计划任务） | 上（scrollback 上方） | `tasks.desired_height(area.height)`，上限 `MAX_TASKS_HEIGHT = 8` 与 `MAX_TASKS_FRACTION = 0.15` 取小（`views/tasks_pane.rs:657-658, 1052-1064`） |
| Catalog（子代理目录） / Todo | 上 | `desired_height(...)`；上限 `8 / 0.15`、`MAX_TODO_HEIGHT = 10 / 0.15`；**Catalog 在 `view_height < 12` 直接返回 0**（`views/subagent_catalog_pane.rs:69-70, 173-187`、`views/todo_pane.rs:155-158, 270-281`） |
| BTW（`/btw` 侧问） / Queue（待发提示队列） | 下（scrollback 下方） | `btw_overlay::btw_panel_height(...)`（`app/agent_view/render.rs:1089-1090`）；`queue.desired_height()`，`MAX_QUEUE_HEIGHT = 3`、`clamp(1,3)`（`views/queue_pane.rs:349, 600-605`） |
| Dock | 下（prompt 正上方） | `dock::desired_height(&data)`，`MAX_DOCK_ROWS = 8`（`views/dock/mod.rs:22`） |

**dock：把三类面板合并成一条。** 自描述是「prompt 上方的合并面板：每个非空 section 一个 header」，由**远程开关 `dock_enabled` 门控**（`views/dock/mod.rs:1-2`）；section 固定四个 + 一个队尾 `Section { Workflows, Subagents, Tasks, Watchers, Queued }`（`:78-83`）。

- **开关解析优先级**（`app/mod.rs:193-218` `resolve_dock_enabled`）：托管 pin（`requirements.toml` 的 `[features] dock`）→ 环境变量 `GROK_DOCK`（未设时回落 `GROK_DOCK_V2`）→ 有效配置的 `[features] dock` → 远程 `dock_enabled` → **默认关闭**；结果写进进程级 `AtomicBool`（`app/event_loop.rs:1313-1315`）。唯一找到的用户级运行时开关：dock 开启时 `Ctrl+G`（`ActionId::ToggleTasks`）不再切 tasks 面板，而是切 `dock_hidden`（`app/agent_view/input.rs:1046-1062`）。
- **开启即吞并旧面板**：`dock_on = enabled() && !viewer_open && area.height > SHORT_TERMINAL_ROWS`，为真时 Tasks 与 Queue 高度强制归零，队列可见性改由 `dock_queued_expanded` 决定，`ActivePane::Tasks` / `ActivePane::Dock` 被驱逐回 Scrollback，队列 rect **重定向**为 `dock::queue_body_rect(layout.dock, dock)`（`app/agent_view/render.rs:1009-1044, 1037-1039, 1103-1106, 1226-1231`）。
- **空间分配是纯函数** `grant_rows(counts, cap)`（`views/dock/layout.rs:243-337`）：① 所有非空 section 的 header 先扣预算；② 每个展开 section 先拿 1 行正文（争抢时公平轮转）；③ 队列正文有一行「谁也不能抢」的 `queue_floor`；④ 被截断的 section 再拿 1 行画 `show N more`；⑤ 之后按 `share_rows` 在静息上限 `MAX_DOCK_ROWS` 内均分，**只有用户显式展开（`show_all`）的 section 才能越过它增长**；⑥ 不变量 `normalize_no_lone_rows`：一个 section 要么 0 行、要么 ≥ 2 行、要么全显示（`:339-388`）。调用方再做一次双向裁剪 `DockLayout::with_cap`（`:411-416`），契约由网格化穷举测试 `the_layout_contract_holds_across_the_grid` 验证（`:529-622`）。
- **没有 tab、没有栈**：四个 section 同时存在、各一个 header；section 内是**独立 band 滚动**（`SectionSlots<usize> offsets`），滚一个 section 不会移动别的 header（`views/dock/mod.rs:154-160`）。

**尺寸自适应阈值（集中常量层）。**

| 常量 | 值 | 出处与触发行为 |
| --- | --- | --- |
| `SHORT_TERMINAL_ROWS` | 16 | `views/agent.rs:91`；`area.height <= 16` 时底部 padding 归 0、`cta_height`/`follow_ups_height` 强制 0、`prompt_gap` 归 0，**dock 强制关闭**（`views/agent.rs:205-219`、`app/agent_view/render.rs:1012, 1079-1086`） |
| `AUTO_COMPACT_MAX_ROWS` | 20 | `views/agent.rs:97`；`effective_compact()` 在 `rows <= 20` 时强制 compact（`eff_outer_vpad → 0`、`eff_hpad_* → 1`）；**不写回用户设置**，窗口变大即恢复；`terminal_rows == 0`（未测量）永不强制（`:99-104`、`appearance/config.rs:197-217`） |
| 编译期断言 | — | `views/agent.rs:98`：`const _: () = assert!(SHORT_TERMINAL_ROWS < AUTO_COMPACT_MAX_ROWS);` |
| `SCROLLBACK_MIN_ROWS` | 5 | `views/agent.rs:94`；transcript 地板，注释说明求解器把它排在所有 `Length` 之上，过载时收缩的是别的行 |
| `SMALL_SCREEN_TIP_MAX_ROWS` | 28 | `tips/small_screen.rs:23`；21–28 行区间弹一次「Tight on space? Try /compact-mode」（`:27-29`） |
| `MIN_TERMINAL_WIDTH` / `MIN_DASHBOARD_WIDTH` / StatusBar `content_width < 10` | 60 / 40 / 10 | 窄于此隐藏 timeline 轨道（`views/timeline.rs:20`）；dashboard 落「单列、标签中截断」窄模式（`views/dashboard/layout.rs:10`、`views/dashboard/render.rs:169-172`）；顶栏整行不画内容（`views/status_bar.rs:53`） |

窗口宽度**没有**触发 agent 主视图的结构性降级（没有窄屏换布局的分支）——**未找到证据**表明 agent 视图存在横向降级；横向降级只发生在 dashboard、timeline rail 与 welcome 屏。

**可配置性：面板只有开/关。** 可调项为 `[scrollback.layout] outer_vpad = 1`、`outer_hpad_left/right = 2`（`docs/user-guide/05-configuration.md:707-713`；`MIN_HPAD = 1`，`appearance/config.rs:195, 220-228`）、`block_pad_left/right = 2`（`scrollback/layout.rs:20-25`）、`[scrollback.scrollbar] enabled = true` 与 `gap_left/gap_right = 0`（`05-configuration.md:715-718`）、`[ui] show_timeline`（默认 `yes`，`26-config-reference.md:627`）、`[ui] compact_mode`（默认 `yes`，或 `/compact-mode`，`:593`）；面板只有开/关——`Ctrl+T` todos、`Ctrl+G` tasks（dock 开启时为 dock）、`Ctrl+;`（macOS VS Code 家族 `Ctrl+4`）queue（`actions/defaults.rs:36-41, 533-539, 548-551`）。**分栏、面板宽度、面板位置、侧栏顺序、面板高度上限**：❌ **未找到证据**——面板宽度永远写死为整宽，高度上限全是硬编码常量。额外隐藏闸门：minimal 模式下动作注册表删掉 `ToggleTasks` 且不注册 `OpenDashboard`（`actions/mod.rs:671-674, 769-774`）。

**焦点模型：单一 `ActivePane` + 每面板三态 `OverlayState`。** `ActivePane` 是全屏唯一的活动 pane 标识，7 个值（`views/agent.rs:30-41`）：`Scrollback / Todo / Queue / Prompt / Tasks / Catalog / Dock`；`ActivePane` 的别名 `AgentPane` 只是 `pub type AgentPane = ActivePane;`（`app/agent_view/mod.rs:209`）。

- 唯一写入口是 `set_active_pane(target, force)`：它同时清掉其它 pane 的 `overlay.focused`，并在切出 Scrollback 时关闭搜索（`app/agent_view/input.rs:1350-1380+`）。兼容子结构是每个 overlay 面板自己的 `focused: bool`，必须与 `active_pane` 一致，否则渲染与按键归属会分叉；dock 则现算 `focused: active_pane == ActivePane::Dock`（`app/agent_view/panes.rs:693`）。有些切换会被**拒绝**而非静默发生：脏的 queued-prompt 编辑阻止切换（`panes.rs:135-137`）；`ComposerRoute::Hidden` 时切到 Prompt 直接返回 `false`（`input.rs:1351-1353`）。
- 焦点移动键：`Tab` 聚焦 prompt（alt `i`、`Space`）/ 聚焦 scrollback（`actions/defaults.rs:469-487`）；Prompt → Dock、Prompt → Tasks 需 `dock_shown && !dock_hidden` / tasks overlay 可见（`panes.rs:41-54`）；Dock 内 `Tab` 回 Prompt，`BackTab`/`Shift+Tab`/`Esc`/`q` 回 Scrollback（`panes.rs:941-972`）；各 overlay 面板用 `Esc`/`q` 回 Scrollback（`panes.rs:289, 909, 1101, 1129`）；Prompt ↔ Scrollback 是 `Tab` 双向（`docs/user-guide/03-keyboard-shortcuts.md:90-98`）。
- **没有全局 focus ring**，而是「谁能吃这个键」：每个面板自己处理键，失焦时返回 `OverlayAction::FocusScrollback`（`views/overlay.rs:26-38`）。滚轮不走焦点，按坐标路由：`pane_areas.hit_test(col, row).unwrap_or(Scrollback)`，固定优先级 **Tasks > Catalog > Todo > Queue > Dock > Scrollback > Prompt**（`views/agent.rs:63-87`、`panes.rs:1296-1324`）。「区域 = 状态机」在源码里有自证图（`views/overlay.rs:8-20` 原文即为 ASCII：Hidden → Visible+Focused → Hidden，旁支是 Visible+Unfocused，`Tab`/`Space` 与 `Esc`/`q` 分别驱动）；结构性按键（`Tab`/`Esc`/`q`/`Space`/`Ctrl-F`）由共享的 `handle_overlay_key` 处理，面板只需实现自己的内容按键（`:1-6, 73-120`）。

**与 dashboard 的关系（本节只记边界）。** dashboard 是**第二套完全独立的布局函数**（`views/dashboard/layout.rs:195-199`，字段 `top_margin/header/header_gap/actions/actions_gap/list/dispatch/footer/bottom_margin`），用手写 y 游标 + 逐行阈值（`>6 / >4 / >10 / >11 / >12 / >13 / >=2 / >16`）而非 ratatui 约束求解，注释说明这个阶梯是为了让列表高度**永不随终端变高而缩小**（`:229-257`）。它与 agent 视图**只共享阈值约定**（`bottom_margin` 的 `> 16` 与 `SHORT_TERMINAL_ROWS` 同值，注释点名「same threshold as `views::agent::AgentViewLayout::compute`」）与 2 列 padding 习惯（`DISPATCH_OUTER_HPAD`/`HEADER_OUTER_HPAD`/`LIST_OUTER_HPAD` = 2，注释说 matches `LayoutConfig::outer_hpad_left/right = 2`），**不共享布局代码**（`:152-164, 189, 243-244`）；peek 预览面板是**列表优先分配**——`LIST_FLOOR_ROWS = 12` 地板先扣、peek 上限 `⌊H × 3/8⌋`（`PEEK_MAX_FRAC_NUM/DEN = 3/8`）、live-tail 正文再被 `MAX_LIVE_TAIL_ROWS = 28` 二次封顶，且 **peek 没有独立 rect**（画在 dispatch rect 里，`:353`）。多会话不是「分屏」，而是在同一个全屏视图里切换 `ActiveView`（`Welcome` / `Agent(AgentId)` / `AgentDashboard`，`app/app_view.rs:202-210`）。

### 4.2 Scrollback 的块模型与渲染管线

### 本节要点

- 数据流四段式：**块（Block）→ 条目（Entry）→ 行（BlockLine）→ 单元格（Buffer cell）**；关键决策是块产出的**已经是换行后的屏幕行**，布局层只做高度算术，不做文本排版。
- 块是**纯函数 + 显式上下文**：`BlockContent::output(ctx) -> BlockOutput` 无副作用；trait 还声明 accent/bullet/背景/折叠状态机/可选性，枚举用 `delegate_block!` 宏转发。
- 每行自带四类语义元数据（`Selectable`、`selection_range`、`selection_text`、`joiner`），一套数据同时服务绘制、搜索、复制与导出。
- 虚拟化靠 `LayoutCache.virtual_y` 前缀和 + `compute_paint_window` 的 `partition_point` 二分，每帧只遍历可见窗口；高度是「先估算 → 接近视口才精确测量」的两级制。
- 粘性头部是纯 1D 计算（`full_height - scroll_past`，1:1 收缩）；跟随尾部在用户上滚时立即关闭，只有「滚到底再过滚」才重新开启。
- 折叠有两族：主动的 VerbRun 聚合（「Read 3 files」）与预算截断的「N more」，二者由单次扫描统一产出 `GroupSpan`。

**分层全景**（`scrollback/` 模块）：

```
会话 / 工具事件 ──► AgentView 把消息、工具结果、会话事件转成块
【块层】 RenderBlock (block.rs:298，13 变体) impl BlockContent (block.rs:40)
    fn output(&self, ctx: &BlockContext) -> BlockOutput      // blocks/{user,agent,thinking,tool/*,…}.rs
   │  BlockOutput { lines: Vec<BlockLine> }        types.rs:433
   │  ★ 每个 BlockLine 已是「按内容宽度换行后的一行」 render.rs:498
【条目层】 ScrollbackEntry (entry.rs:57) = 块 + display_mode + 4 层缓存
    ScrollbackState (state/mod.rs:53) = entries / scroll / 选中 / turns / group 折叠 / LayoutCache (state/layout.rs:42)
   │  每帧 prepare_layout (state/mod.rs:1344) → paint window → render
【单元格层】 ScrollbackPane::render_with_scratch (scrollback_pane.rs:139)
    └ EntryRenderer (impl Renderable, entry_renderer.rs:23/543)
        desired_height(width) → 高度；render(area, buf) → 单元格
```

**块抽象。** `BlockContent`（`block.rs:40`）只描述「渲染什么」，不碰 Buffer，方法按职责分组：内容产出 `output`（`:42`，唯一必选内容方法）；样式 `accent` / `bullet` / `accent_background` / `background`（`:46-62`）与 `has_vpad_for`（`:69`，上下各 1 行留白，默认 true）；模式 `has_raw_mode` / `is_foldable`（`:81-86`）与折叠状态机 `next_fold_mode` / `collapse_mode` / `default_display_mode` / `finished_display_mode`（`:92-124`）；交互 `is_selectable`（`:131`）、`has_bullet` / `is_groupable`（`:141-145`）、`preamble`（`:148`，全屏查看器头部文本）；媒体 `inline_media` / `inline_media_placements` / `diagram_affordances` / `inline_open_button`（`:169-206`）与 `estimate_extra_rows`（`:203`，仅 Mermaid 用）。`RenderBlock::output()` 转发后统一补 bullet：`prepend_bullet`（`:202`）把 bullet span 插到首行行首，并调用 `shift_selection_metadata_for_prefix`（`types.rs:398`）把可选区整体右移一格，避免 bullet 被复制进剪贴板。

`RenderBlock`（`block.rs:298`）13 变体与内部类型一一对应：`Stub` / `UserPrompt` / `AgentMessage` / `ToolCall` / `Thinking` / `System` / `SessionEvent` / `BgTask` / `Subagent` / `Workflow` / `Btw` / `ContextInfo` / `MemoryCapture`；同文件 `delegate_block!` 宏把全部 trait 方法 match 转发到内部类型（`:331-347`），只有 `output()` 需拦截——`RenderedBlockOutput` 路径用于 Edit / SentMessage 这两个需要额外 selection boundary 侧车数据的块（`:413-425`）。`ToolCall` 再套一层 `ToolCallBlock`（`blocks/tool/mod.rs:149`）：`Execute` / `Read` / `Edit` / `ListDir` / `Search` / `WebFetch` / `WebSearch` / `IntegrationSearch`（MCP 发现）/ `UseTool`（MCP 调用）/ `MemorySearch` / `SentMessage` / `Skill` / `Other`，同样用 `delegate_tool!` 转发（`:171-189`）。各块的差异行为：`UserPromptBlock`（`blocks/user.rs:84`）折叠保留 3 行（`COLLAPSED_MAX_LINES`，`:15`）、slash token 单独着色；`AgentMessageBlock`（`blocks/agent.rs:11`）流式 `push_chunk` / `finish`（`:46/67`）；`ThinkingBlock`（`blocks/thinking.rs:124`）三态折叠、collapsed 头部 `Thought for Xs (ctrl+e to expand)`（`:18`）；`ToolCallBlock` 共用 header 选择区 id `TOOL_HEADER_RANGE = 0`（`blocks/tool/mod.rs:47`）并有 `VerbGroupKind` 聚合动词/名词表（`:81-143`）；`SubagentBlock`（`blocks/subagent.rs:45`）与 `BgTaskBlock`（`blocks/bg_task.rs:36`）均「always collapsed and not foldable」（`:42-43` / `:33-34`）；`SessionEventBlock`（`blocks/session_event.rs:603`）覆盖约 30 个会话事件变体（`:49-190`）、不可选不可折叠（`:47`）；`MarkdownContent`（`blocks/markdown_content.rs:40`）是 agent/thinking/btw 共享的流式 markdown，换行缓存键为 `(width, generation, theme)`（`:23-37`）；`QuoteBarStrip`（`blocks/quote_bar.rs:19`）把引用条 `│` 从可选区中排除（`quote_prefix_selectable`，`:131`）。

**块 → 行。** `output()` 内部完成三件事：markdown 渲染、按 `ctx.width` 换行（如 `word_wrap_lines(styled_lines, ctx.width)`，`blocks/system.rs:26-30`）、打 selection 元数据。`BlockLine`（`types.rs:146`）字段为 `content: Line<'static>`（绘制的 span 序列）、`wrap: WrapMode`（Word / Character / Truncate，`types.rs:16`）、`selectable: Selectable`（All / Spans(range) / None，`types.rs:72`）、`selection_range: Option<u16>`（逻辑选择分组 id，组头保留 `u16::MAX`）、`selection_text: Option<String>`（复制文本覆盖，工具头等）、`joiner: Option<String>`（软换行连接符，复制时替代 `\n`）、`background` / `bg_start_col` / `indent_width` / `link_target`。复制文本由 `derive_selection_text`（`:289`）推导：优先 `selection_text` 覆盖，其次按 `Selectable` 取 span 子集，`All` 会 trim 尾部空白；软换行分组靠 `joiner`——`prewrap_index_per_row`（`:605`）用「joiner 为空则逻辑行 +1」把后换行行映射回源码行，是搜索命中定位到行的基础。

**行 → 单元格。** `EntryRenderer` 是唯一往 Buffer 写单元格的地方（`wrappers/entry_renderer.rs:561`）：① `content_width = area.width - chrome_width - timestamp_reserved`；② `entry.ensure_cached(content_width, …)` 取缓存输出；③ 背景按 `BlockBackground` 逐列填 padding/content/right_pad，flat 模式（minimal）退回 `Color::Reset`（`fallback_bg`，`:110`）；④ accent 列**仅 `display_mode != Collapsed` 时**画，运行中的块用 `AccentStyle::animated` 画波纹（`WAVE_SPEED = 0.15` 弧度/tick，`:20`）；⑤ 内容行从 `skip_rows` 起逐行 `Buffer::set_line`。横向几何固定为 `accent(1) | block_pad_left | Min(1) | block_pad_right`，`Min(1)` 那格就是传给 `BlockContext.width` 的内容宽度（`scrollback/layout.rs:19`；选中边框另画，`selection_area` 向左右各扩 1 列，`:75`）。每帧时序：`ScrollbackState::prepare_layout(width, height)`（`state/mod.rs:1344`）依次做「视口尺寸变化 → `bump_generation()`（链接图失效，`:1352`）→ 缓存缺失/宽度变化则全量重建并捕获 `ScrollAnchor`（`:1370`）→ `settle_visible_measurements`（`state/layout.rs:745`）→ `handle_follow_mode`（`state/nav.rs:1024`）→ 更新 `total_height`」，随后 `ScrollbackPane::render_with_scratch`（`scrollback_pane.rs:139`）按 `AllTurns → render_with_sticky_headers(:294)` / `SingleTurn → render_single_turn(:277)` 分支、`state.paint_window(...)` 只遍历可见窗口（`:1498`）、逐条 `EntryRenderer::render`，返回 `RenderOutput{selection_box, scroll_info, selection_model, link_overlay, …}`。

**虚拟化与增量渲染。** `LayoutCache`（`state/layout.rs:42`）持有 `entries: Vec<EntryLayoutInfo>`（高度 + `gap_after` + 分组标志）、`entry_truncated_heights`（sticky 头部 min_height 用）、`measured: Vec<bool>`（true = 精确测量）、`virtual_y: Vec<usize>`（每个 entry 的累计 Y 前缀和）、`prompt_descriptors`、`groups: Vec<GroupSpan>`、`width`；`height` 是**已换行后的行数**，`gap_after` 让相邻两个可密集相接的折叠块为 0、否则为 1（`:1555-1566`）。`compute_paint_window`（`:1568`，纯函数、可单测）的三条规则：用 `partition_point` 在 `virtual_y` 上二分求 `[vp_start, vp_end)`，O(log n)；起点回退一格以容纳「起点之上但越过视口顶的行」；**若窗口内存在组头行，把窗口右界扩到该 run 末尾**，让聚合标签能看到屏外成员——这是唯一会越过视口边界的情形。返回的 `content_y0` 让渲染不必从历史顶部累加（源码注释：「a viewport window does not re-walk off-screen history」）。

高度是两级制：`measurement_window`（`:593`）只覆盖首个可见条目 + 其后 `MEASURE_MARGIN_ENTRIES = 8` 个（下方余量，**无上方余量**）；`measure_window_exact`（`:669`）对未测量条目真正调 `output()` 量高（隐藏行与合成组头跳过）；`settle_visible_measurements`（`:745`）循环「测量 → 重建 virtual_y → 重锚视口」直到窗口内全部精确；`warm_measure_pages_above`（`:796`）在 resume 后向上预热 `RESUME_WARM_PAGES = 3` 屏，避免上滚时高度跳变。估算本身便宜：`estimate_source_lines`（`entry.rs:286`）用缓存的逐行显示宽度 `Vec<u32>` 做 `ceil(w / content_width)` 求和（`wrapped_lines_from_widths`，`:48-62`，饱和到 `u16::MAX`）。

**缓存分四层且失效语义刻意分离**（`entry.rs:88-102`）：

| 缓存 | 键 | 失效方法 |
| --- | --- | --- |
| `cached_output: RenderedBlockOutput` | `(width, raw, theme, is_selected, cwd)`（`entry.rs:11-18`） | `invalidate_cache` / `invalidate_width_caches` |
| `cached_truncated_height` | `(content_width, raw, theme, cwd)`（`:23`） | 同上 |
| `cached_estimate_lines` | `(content_width, lines)` | 同上 |
| `cached_line_widths` | 与宽度无关 | **只**被 `invalidate_cache` 清 |

失效语义：`invalidate_cache`（`:246`）先调 `invalidate_width_caches`（`:252`，把 `cached_output` / `cached_truncated_height` / `cached_estimate_lines` 置 `None`）再把 `cached_line_widths` 置 `None`——即 resize 路径保留与宽度无关的逐行宽度，只有内容变了才丢它。`ensure_cached` 还把「只有部分块在意选中态」编码进缓存键：仅 UserPrompt / ToolCall / Thinking / BgTask / Subagent 的 `is_selected` 参与键，其余块归一化为 false，避免选中移动导致缓存抖动（`:326-332`）。离屏驱逐 `evict_render_cache`（`:261`）只丢重渲染输出、**保留**高度缓存与布局，滚回来透明重算，保留窗口为 `±EVICT_KEEP_MARGIN_ENTRIES = 128`（`state/layout.rs:627-633`）。`ScrollbackState` 侧另有两个代际计数：`generation`（视口/链接位置变化即 +1，供 `VisibleLinkMap::is_stale`）与 `content_generation`（只在增删条目或内容变化时 +1，供搜索语料缓存复用）（`state/mod.rs:196-203`）。其余滚动期手段：`skip_rows` 裁剪局部可见条目以消除其 scratch buffer 渲染（`entry_renderer.rs:35-40`）；`ScratchBuffer` 复用给 sticky 推挤裁剪（`render.rs:69-100`）；脏高度集合 `dirty_heights` + `update_dirty_entry_heights` 只重算脏条目（`state/mod.rs:74`、`state/layout.rs:981`）；增量 `patch_virtual_y_for_dirty` / `extend_layout_cache_with_new_entry` 避免流式增长触发 O(n) 全量重建（`:1100, 1189`）；流式 markdown 冻结已换行前缀（`frozen_pre_wrap_count`，`blocks/markdown_content.rs:23-37`）；`begin_batch` / `end_batch` 在批量 push 时跳过 turns 与布局重建（`state/mod.rs:161-165`）。

**滚动锚点两套**，解决两类位移：`ScrollAnchor { entry_idx, logical_line, sub_rows }`（`state/layout.rs:16`）用于宽度变化——按**逻辑行**重定位（resize 后换行行数变了，绝对行号会指错内容），只在非跟随模式且 `scroll_offset > 0` 时捕获（`state/mod.rs:1370-1375`）；`StructuralScrollAnchor { id: EntryId, rows_into_span, armed_scroll_offset }`（`:26`）用于增删条目这类会移动索引的突变，用稳定 `EntryId` 记录、下次 `prepare_layout` 消费，期间发生过显式导航则不生效（`state/mod.rs:1361-1363`）。

**粘性头部**（`sticky.rs`，纯 1D 坐标计算，不关心块内部结构）：每个 prompt 压缩成 `PromptDescriptor { entry_idx, y_virtual, full_height, min_height, sticky }`（`:23`）；取「最后一个已被滚过（`y_virtual < scroll_offset`）的 sticky prompt」作为 `pinned`（`:252-264`）。`calculate_render_height` 让 `render_height = full_height - scroll_past`，与滚动 **1:1** 收缩（保证内容底行不跳），下限 `min_height`（估算种子 `MAX_TRUNCATED_HEADER_HEIGHT = 6`，`state/types.rs:71`）、上限视口高；`MIN_PINNED_HEIGHT = 4`（`:45, 339`）。下一 prompt 的裸行号 ≤ `render_height + HEADER_CONTENT_GAP`（=1，`:92`）时进入 push 过渡，当前头变 `pushed{ render_height, clip_top }` 逐渐被推出屏幕；`next_naive_row == 0` 时头部消失、下一 prompt 接管（`:277-330`）。关闭条件：`scrollback.display.sticky_headers` 为 false 或 prompt 处于 compact 模式（`scrollback_pane.rs:310-315`）。

**跟随尾部（follow）。** 状态是 `follow_mode: bool` + 一次性标记 `follow_preserve_scroll` + 页面翻转预留 `pin_reserve_*`（`state/mod.rs:106-136`）。打开时每帧 `handle_follow_mode` → `follow_scroll_to_bottom` 把 `scroll_offset` 钉到 `max_scroll_offset()`（内容不足一屏时返回 0，顺带修正被缩短内容卡住的越界偏移，`state/nav.rs:1024, 1052, 1084-1088`）。**用户上滚即暂停跟随**：`scroll_up` 无条件 `follow_mode = false`（`:356-360`），其余显式导航（翻页、跳转、折叠、turn 导航）同样关闭跟随（`:106/218/248/486`）；**重新进入跟随**需向下滚到底后再滚一次（overscroll）且 `follow_by_overscroll` 为 true——`rows > 0` 与 `pin_reserve_pad == 0` 是防止「恰好落到最底」被误判为过滚的守卫（`:361-380`），另有 `enable_follow_mode` / `enable_follow` / `enable_follow_with_preserve` 三个程序化入口（`:522/531/538`）。跟随还会自动选中最后一个可选条目，但仅当「当前无选中」或「选中本来就在尾部」时才移动（`:1034-1047`，受 `follow_auto_select` 门控）。`pin_reserve` 把「最后一个 user prompt 顶到视口顶部」这种页面翻转位姿变成合法滚动底——否则 clamp 到内容高度会把视图弹回尾部；它在某轮结束后释放（`state/pin_reserve.rs:1-2, 30`）。

**搜索。** 语料 `ScrollbackSearchIndex` 为每个条目缓存一段 `searchable_text`（markdown 块给渲染纯文本，其余给源文本），整体 `Arc<[IndexedEntry]>`，只在 `content_generation` 变化时重建（`search.rs:44-93`；`searchable_text` 见 `block.rs:1000-1030`）。扫描在后台线程 `SearchDaemon`：UI 线程只发「最新语料 + 查询」的 O(1) 消息，`poll` 逐帧取结果；若结果属于已被取代的查询则丢弃，避免匹配数与光标错位（`:200, 378-398`）；空查询与非法正则立即清空匹配（`:333-341`）。结果模型 `ScrollbackMatch { entry_id, line_in_entry, byte_range }`（`:32`），`next/prev` 用 `rem_euclid` 环绕（`:401-417`）。**高亮是逐帧后处理遍**：对每个可见行把 span 文本拼进复用的 `highlight_text` 再调 `paint_match_highlights`（`render.rs:500-512`），因此高亮集合按屏幕字形计算，可能少于索引匹配集合（源码里的 markdown 标记在屏上不存在）。入口：vim 模式下 `/` 打开增量搜索栏（`app/agent_view/panes.rs:137-144`），`n`/`N` 与上下箭头推进（`:147-156`），搜索栏占底部 2 行（`search_reserved_rows`，`:160`）。

**选择是「三维逻辑坐标 + 二维屏幕几何」。** 每帧 `render.rs` 把可见行解析成扁平模型（`text_selection.rs:80-296`）：屏幕 `(col,row)` 经 `hit_test_selectable_range`（`:296`）得到 `RangeHit { entry_idx, range_id, block_line_idx, col_within_range }`（`:199`）；`ResolvedSelectionModel` 持有 `ranges` 与 `visible_blocks: Vec<VisibleBlockGeometry { area, content_area, selection_area, content_width, top_clipped, bottom_clipped }>`；`ResolvedSelectableLine`（`:143`）同时带逻辑身份（`entry_idx` / `range_id` / `block_line_idx`）与屏幕几何（`screen_y` / `screen_x` / `selectable_cols`），外加逻辑复制文本、实际画出的 `painted_region`（列吸附用）与 `joiner_to_previous`。三个坐标维度——条目 + 逻辑选择区间（块内从 0 递增，合成组头固定 `u16::MAX`）+ 块内行号（滚动不会改变它）——正是「锚点滚出屏幕仍能复制全文」的基础。拖拽状态机：`PendingTextDrag`（按下时记录 anchor + 锚点块 `content_width` 快照）→ 超过 `drag_threshold_exceeded`（`:436`）→ `ActiveTextDrag { anchor, head, kind }`（`:219`）；松手后 `PersistentTextSelection`（`:273`）保留高亮，遇到别处点击 / Esc / 任何 scrollback 导航才清除。鼠标侧：`SelectionOrigin { Drag, DoubleClick, TripleClick }`（`:234`）分别取范围/词/行，词边界由 `word_boundaries_at_col` + `configured_word_separators`（默认 `DEFAULT_WORD_SEPARATORS`，`:1150`）决定；URL 有专门识别并剥掉尾部标点（`url_range_at_col`，`:1296`；`TRAILING_URL_PUNCT`，`:1249`）；拖到边缘触发定时自动滚动：`compute_autoscroll`（`:40`）在距内容区上下沿 `EDGE_THRESHOLD = 2` 行内返回方向与速度，速度按越界距离分档 1/2/3/5 行每 tick（`:70-77`）；`semantic_selection_at`（`:1350`）处理双击/三击在软换行连接处的扩展。

**复制**走缓存重切片而非可见行：`reconstruct_drag_copy`（`app/agent_view/selection.rs:719`）先试表格几何匹配 → 表格 TSV，否则用锚点块的完整输出 `reconstruct_full_selection_text_with_boundaries`（`text_selection.rs:1016`）——即从块缓存重新切片，若块已不可取则退回可见模型的 `reconstruct_selection_text`。列语义基于 `painted_selectable_region`（`types.rs:322`，实际画出的字形），逻辑文本与绘制文本不一致时（工具头覆盖文本）按 bidi 状态分流（`:1076-1096`）；范围覆盖到行尾时剥掉渲染用的尾部填充（与 `derive_selection_text` 的 trim 一致）。boundary 侧车 `SelectionBoundary { prefix, suffix, has_empty_row_anchor }`（`types.rs:440-492`）记录「可见行省略掉的源码字节」，复制时按选区是否触达首/末列决定贴回，保证复制仍是完整源码。落点为 `copy_to_clipboard` → `crate::clipboard::copy_text_or_file` + toast，另有防抖版本（`app/agent_view/notices.rs:248-267`）；键位 `y` 复制块内容、`Y` 复制命令/路径（`actions/defaults.rs:352-370`）。**表格选择**：`TableGeometry` 从已渲染的行里**反推**箱线几何（`junction_cols` = 竖线显示列，个数为列数 + 1；`rows` = 每个逻辑行的块行范围，单元格内换行会跨多行，`table_geometry.rs:19-30`），`SelectionKind` 三态为 `Linear` / `TableCell`（锚点吸附在同一单元格列带内）/ `TableGrid { anchor: CellRef, head: CellRef }`（整格矩形，复制为 TSV，`text_selection.rs:243`）；`detect` 无法证明是表格时一律回退 `Linear`（`table_geometry.rs:1-2`），几何在 promote 时冻结，若之后流式重排导致行号漂移会重新检测并要求完全一致（`selection.rs:734-738`）。

**链接与 OSC 8**：每帧从 `LinkOverlay`（markdown 超链接 + web_search/web_fetch 引用 URL）重建 `VisibleLinkMap`（`link_map.rs:40`），`VisibleLink.rects` 允许一个逻辑链接跨多行（`:14-18`）；失效靠代际 `is_stale(current_generation)`（`:52`），视口尺寸变化在 `prepare_layout` 里 `bump_generation`（`state/mod.rs:1352-1355`）；点击命中 `link_at(col,row)`（`:46`），`looks_like_bare_url_text`（`:25`）区分「屏上直接显示 URL」与「短标签/宽引用块」以决定提示文案。

**折叠/展开/截断。** 显示模式是三态枚举 `DisplayMode { Collapsed, Truncated, #[default] Expanded }`（`types.rs:63`），折叠是 trait 提供的状态机（`block.rs:92-127`）：默认 `Collapsed ↔ Expanded`；thinking 运行中 `Collapsed|Truncated → Expanded`、`Expanded → Truncated`，结束后 `Collapsed ↔ Expanded`（`blocks/thinking.rs:532-544`），`collapse_mode` 运行中给 `Truncated`（`:546-552`），默认模式 `Truncated`（`:554`），结束自动 `Collapsed`（`:558`）；`display_mode_pinned`（`entry.rs:73`）标记「用户手动折过」的块，配合 `respect_manual_folds` 让重排不再自动展开（`docs/user-guide/03-keyboard-shortcuts.md:68-70`）。分组折叠由 `state/groups.rs` 单次扫描同时决定，产出权威 `GroupSpan`，再 `project_to_layout` 写回每 entry 的标志（`:1-10/62-76`）；`GroupKind`（`:39`）两族为 `VerbRun { members }`（主动折叠的动词 run，标签由 `verb_group_header_label` 生成，动词/名词表在 `VerbGroupKind`，`blocks/tool/mod.rs:81-143`，`group_tool_verbs` 是总开关）与 `Truncation { participants, hidden }`（过长的密集 run 按 `max_visible` 预算隐藏，头部显示「N more」或聚合标签，`GroupHeaderLabel`，`state/verb_group.rs:150`）。渲染上合成组头行高 1，标签行本身是可选行（`range_id = GROUP_HEADER_RANGE_ID = u16::MAX`，`render.rs:466-490`），复制得到聚合文本；`EntryLayoutInfo::is_group_header()`（`state/types.rs:113`）是所有消费者的唯一判据；选中被隐藏条目时 `fixup_hidden_selection` 把选中移到最近可见条目、优先组头（`state/layout.rs:157-180`），用户展开过的组记在 `expanded_groups`（`state/mod.rs:191-194`）。

超长输出三档：`Truncated` 取前 `first_lines` + `…` + 后 `last_lines`，默认 `first_lines = 2`、`last_lines = 3`（`blocks/tool/execute.rs:471`；`appearance/config.rs:613-614`，可在 `pager.toml` 调整）；`Collapsed` 是单行摘要（Execute 的折叠头在有 description 时隐藏命令行，`execute.rs:317/379`）；`Expanded` 是全文（`truncate = None`，`execute.rs:660-667`）。流式中的 `!` 命令默认显示尾部、结束后可展开（`execute.rs:32/964-973` 测试）；Enter / `Ctrl-F` 打开块查看器是「显示更多」的最终出口（`actions/defaults.rs:382-386`）。高亮类块有显式上限防卡死：`EDIT_HL_MAX_BYTES = 2 MiB`、`EDIT_HL_MAX_LINES = 50 000`（`blocks/tool/edit.rs:46/49`）。键位（均有 registry 证据）：`h`/`Left`、`l`/`Right` 折叠展开（`actions/defaults.rs:265/278`）、`e` 切换折叠（`:291`）、`⇧E` 全部展开/折叠（`:306`）、`Ctrl+E` 全部 thinking（`blocks/thinking.rs:16-17`）、`r` 切 raw markdown（`:336`）、`Ctrl+K`/`Ctrl+J` 行滚动（`:182/195`）、`Ctrl+U`/`Ctrl+D` 与 `PageUp`/`PageDown` 半页整页（`:208/221/238/251`）、`y`/`Y` 复制（`:352/367`）。

**导出是旁路。** `export.rs` 把块序列转成人类可读 Markdown：只保留 `## User` / `## Assistant` / `## Tools`，Thinking / System / SessionEvent / BgTask / Subagent 等 chrome 跳过，连续 assistant 消息合并到一个标题下，工具调用压成一行摘要（`:1-6/14-66`）——这是「内容与呈现分离」的额外好处：导出不经过渲染管线。

### 4.3 模态、浮层与焦点栈

### 本节要点

- **没有「浮层栈」这个对象**：真正的模态是单槽位 `Option<ActiveModal>`，多浮层的 z 序由 `render.rs` 里 `if … return` 的**分支顺序**定义，因此这些分支实际上是互斥的。
- 所有带边框弹窗共用同一套 chrome（`ModalWindowConfig` + `render_modal_window`），默认尺寸 `0.9 / 140 / 60 / v_margin 7`，水平垂直双居中；小于 `20×6` 直接拒绝渲染并清空全部鼠标命中矩形。
- 遮罩**不统一**：主流是 `Clear` 清底、背景不变暗；只有少数全屏浮层用 `dim_area(0.5)`，失焦「退后」用 `recede_area(0.66)`。
- 焦点不是栈，而是一张显式排名表 `KeyOwner`（Permission 永远最高）加一个 ESC 阶梯枚举 `EscStep`（8 档，每档带用户可见标签，快捷键栏与实际处理读同一张表）。
- 嵌套靠三种手法实现（同一 popup 内换内容 / 后画覆盖前画 / 快照回退），**不能嵌套的是「同层再来一个模态」**；真正的「排队」只有 permission 队列（`VecDeque`，所有判定只看 `front()`）。

**渲染层：分支顺序即 z 序。** 唯一「栈式」的地方是 `AgentView::intercept_takeover_input`（`app/agent_view/subagent_takeover.rs:347`）：子代理全屏接管**在所有其他输入处理之前**跑（`app/agent_view/input.rs:361-363`），非空时直接返回，等价于压在整棵渲染树之上；`key_owner.rs:38-40` 注释也承认「这些全屏接管自己回答输入，因此不参与排名」。其余浮层的相对顺序就是分支顺序（`app/agent_view/render.rs`），**每个分支画完即 `return`**，所以它们实际互斥：

```
  优先级高 ┌──────────────────────────────────────────────────────────────┐
  (z 序前) │ active_modal (:3211)     → 模态弹窗（命令面板/设置/扩展/…）   │
           │ line_viewer (:3244)      → 计划/文件预览（圆角框 + 半透明蒙层）│
           │ image_viewer (:3430) · video_viewer (:3600) · gboom (:3643)   │
           │ block_viewer (:3702)     → 块全屏查看（Ctrl-F）              │
           │ agents_modal (:3894) └ persona_detail（同分支内后画，:3911）  │
           │ feedback_modal (:3922) · extensions_modal (:3940)            │
           │ goal_detail (:4239) · workflows (:4259)                      │
  优先级低 └──────────────────────────────────────────────────────────────┘
  更高一层（在这套渲染之前就 return）：子代理全屏接管
```

真正意义上的「叠」只发生在两条路径上：同一分支内先后两次绘制（agents_modal → persona_detail），以及同一个 popup 内换内容（Settings → ResetSettingsConfirm）。**五种浮层形态，五种容器**：① 模态弹窗（ModalWindow，屏幕居中矩形 + `Clear` 清底，**不做 dim**，如命令面板/设置/扩展/快捷键表）；② prompt 槽位接管（直接占用 `layout.prompt`，完全替换输入框，如 permission / question / elicitation 卡片、`/rewind`、`/jump`、cancel-turn，`render.rs:943-965, 2781-2794`）；③ 全屏接管（整屏或除 shortcuts bar 外全部，多数自带蒙层，如 block_viewer、line_viewer、图片/视频、子代理全屏）；④ 内嵌浮层（复用宿主 popup 矩形，覆盖其内容区，如 ResetConfirmOverlay、ManagedConnectorsWait、memory 全屏变体）；⑤ 下拉/横幅（prompt 上方或紧邻几行，**不遮底层**，如 slash_dropdown、completion_dropdown、file_search、privacy_banner、btw_overlay）。`/btw` 是特例：它**不是 popup，而是布局里的一行槽位**——`btw_height` 被算进 `AgentViewLayoutParams`（`app/agent_view/render.rs:1089-1090, 1123`；`views/agent.rs:120, 247-249`），高度由内容决定（`Loading` 恒 3 行，`Done` 为 `2 + clamp(行数, 1, DONE_MAX_BODY_LINES)`，`Error` 为 `2 + 折行后的错误行数`，`views/btw_overlay.rs:182-200`）。

**ModalWindow 的视觉规范。** 一切有边框的弹窗都走 `render_modal_window`（`views/modal_window.rs:236`），配置是 `ModalWindowConfig`（`:76-87`：`title` / `tabs` / `shortcuts` / `sizing` / `fold_info`），持久状态是纯数据类型 `ModalWindowState`，放在表现层 crate 里以免下层依赖 views 层（`xai-grok-pager-render/src/modal_window_state.rs:1-4`）。`ModalSizing` 七个字段（`:89-121`）：`width_pct = 0.9`、`max_width = 140`、`min_width = 60`、`v_margin = 7`、`h_pad = 2`、`v_pad = 2`（**下方不生效，底部归 footer**）、`footer_lines = 2`；三档预设 `default()` = `large()`、`medium()` = `0.60 / 120 / 44 / 4 / 2 / 1 / 2`（**未找到生产调用点**）、`with_compact(true)` 把 `v_margin→0`、`h_pad→1`、`v_pad→0`（注释：目标是「把每个 popup 的可用内容区最大化」，`:146-157`）。实际尺寸由 `compute_modal_dims`（`:575-586`）算：`max_width = area.width.saturating_sub(4).min(sizing.max_width)`，`preferred_width = (area.width as f32 * sizing.width_pct) as u16`，`modal_width = preferred_width.min(max_width).max(sizing.min_width).min(area.width)`——注意 `min_width` 可以顶到 `max_width` 之上，最后再被 `area.width` 收回（注释解释这是为了不让 `Clear`/`Block` 越界），而 `modal_height = area.height.saturating_sub(sizing.v_margin * 2)`，**没有下限也没有上限**。**两道拒绝渲染闸门**：`modal_width < 20 || modal_height < 6` 时直接返回 `None`，并清空 `popup_area` / `close_button_rect` / `shortcut_hits` / `tab_rects`，避免鼠标命中上一帧的陈旧矩形（`:253-260`）。水平、垂直都居中（`x = area.x + (area.width - modal_width) / 2`、`y = area.y + (area.height - modal_height) / 2`，`:262-271`）；页脚贴的是 **popup 内底**而非屏幕底（`footer_y = inner.y + inner.height - footer_height`，`:381`）。

```
area = 整屏 (W × H)，以 W=100 H=32、ModalSizing::default() 为例
modal_width  = min(100*0.9=90, min(100-4,140)=96) = 90     modal_height = 32 - 7*2 = 18
x = (100-90)/2 = 5      y = (32-18)/2 = 7
col: 0    5                                      94    99
row 0  ┌──────────────────── 上边距 7 行 ────────────────────────┐
row 7  │    ┌──────────────────────────────────────────────┐     │
row 8  │    │ ─ Title ─                            [✗]    │     │  ← 边框行，关闭按钮嵌在上边框
row 9  │    ├──────────────────────────────────────────────┤     │  ← 可选 tab bar 在此
row 10 │    │ ──────────────────────────────────────────── │     │  ← 全宽分隔线
row 11 │    │   content（左右各内缩 h_pad=2）           ║  │     │  ← 滚动条占 content 最右一列
row 23 │    │  ↑/↓ nav  |  Enter select  |  Esc close      │     │  ← footer 贴内底，水平居中
row 24 │    └──────────────────────────────────────────────┘     │
row 25 └──────────────────── 下边距 7 行 ────────────────────────┘
        x=0  x=5                                          x=94
```

- **边框** `Block::default().borders(Borders::ALL)`，未指定 `border_type`（默认直角），`border_style = fg(gray_dim) + bg(bg_base)`，整块样式 `bg(bg_base) fg(text_primary)`；模块注释自称 "accent-colored square border"（`:6`）但代码用 `gray_dim`——**以代码为准**。**标题画在上边框线上**，破折号用边框色、标题字用 `text_primary + BOLD`（`Span("─ ") + Span(title) + Span(" ─")`）；`title` 为空串则整条上边框连续无装饰（`:277-282, 315-322`）。**关闭按钮 `[✗]`** 是 5 cell 定宽串，位于 `x = modal_area.x + modal_area.width - (5 + 2)` 的第 0 行，悬停只把中间三个非空格 cell 改成 `text_primary + BOLD`，两侧 padding 保持原样（`:405-436`）。
- **可选 tab bar**（`config.tabs` 为 `Some` 时才画）：左缩进 2 列、标签间隔两个空格、按显示宽度贪心折行，放不下的行整行跳过并把对应 `tab_rects` 置 `None`；样式三态为「选中且有键盘焦点 → `text_primary on bg_visual + BOLD`」「选中但无焦点 → `accent_user + BOLD`」「未选中 → `gray`」（注释解释为何不用 `gray_dim`：ANSI16 下对比度会掉到 ~1.2:1）；tab bar 下方必画一条与内宽等长 `─`，**且存在 tab 时内容区 `v_pad` 被强制为 0**（分隔线已充当间距）（`:330-360, 508-531`）。
- **内容区与页脚**：`content_top = inner.y + effective_v_pad + tab_bar_height + tab_divider_height`，`content_area.x = inner.x + h_pad`、`width = inner.width - h_pad*2`；`footer_lines` 先取 `sizing.footer_lines`，若快捷键在当前宽度放不下就按**与渲染同一套贪心折行**的预测值抬高（`:363-365, 560-614`，注释强调「预测必须和实际渲染一致」）。页脚行底部对齐、每行水平居中、行内用 `"  |  "` 分隔；单个快捷键按第一个 ASCII 空格切成「键名（`text_secondary + BOLD`）+ 说明（`theme.muted()`）」。**所有快捷键——包括 `clickable: false` 的纯提示——都产出 `ShortcutHitArea`** 用于悬停高亮，只有 `clickable: true` 才派发 `ShortcutActivated`（`:616-761`）。`/settings` 与 how-to 指南多一层 tip 行：内容高 ≥ 3 时切出最后一行、≥ 6 时再空一行，tip 右对齐居中，页脚相应预留 `预测行数 + 1` 且至少 2 行（`:798-850`）。
- **滚动条**：列表型模态的滚动条来自 `tui-scrollbar`，在 `content_area` 最右列、宽度 1、高度等于可视行数（`views/picker.rs:2084-2098`）；`SCROLLBARS_HIDDEN` 为真时整体 no-op，minimal 模式启动时置真，改由页脚承担 "↑/↓ navigate" 提示（`render/scrollbar.rs:44-48, 211-229`）。滚动窗口策略是**居中跟随选中项**（`selected - visible/2` 夹到 `max_scroll`，`views/picker.rs:151-169`）；鼠标滚动写入 `scroll_offset`，一旦键盘导航就重置为 `None` 交还自动策略（`:1487-1490, 2004-2012`）。
- **embedded（minimal）变体**：进程级 `AtomicBool` `EMBEDDED` 把整个模态框切成无边框形态，注释明确「minimal 模式永远不会显示浮动模态框」；此时 `modal_width/height = area`，不画边框、关闭按钮与 `Clear` 以外 chrome，标题改成第一行的裸加粗文本，行选中态从「画色带」改成「改前景为 `fuzzy_accent`」（`:25-73, 246-251, 284-307`）。
- **遮罩三种互不相同的机制**：① `Clear` 清底（主流）——只重置弹窗覆盖的 cell，弹窗外内容原样保留、不变暗（`:274-275`）；② 半透明蒙层（少数全屏浮层）——block_viewer 画之前 `dim_area(buf, overlay_area, theme.bg_base, 0.5)`（`app/agent_view/render.rs:3779`），line_viewer 有 `should_dim` 分支且全屏时跳过 dim（`views/file_search/line_viewer.rs:1439, 1465`），混合公式是逐通道线性插值（`render/color.rs:152-157`）；③ 失焦「退后」（recede）——**焦点反馈而非层级遮罩**，`ListOverlay` 在 `focused == false` 时把整个区域朝面板底色混合 0.66（`views/overlay_list.rs:152-155`），`PickerMode` 也把 dim 绑在模式上：`Floating` = "dimmed background and rounded border"、`FullScreen` = "no border, no dim"（`views/picker.rs:104-113`）。`ModalWindowState` 里没有任何 dim/alpha 字段（`modal_window_state.rs:11-34`），从侧面印证第 1 档才是主流。
- **对照样本**：`new_worktree_dialog.rs` 没走 ModalWindow，而是用 ratatui 的 `Flex::Center` 手写居中（`MIN_DIALOG_WIDTH = 50`、`DIALOG_HEIGHT = 5`、内部左右 padding 4），且**小到放不下时会画一行 `[Esc] to close` 作为降级提示**而不是静默消失——少数做了「拒绝渲染但给出可退出线索」的浮层（`:11-31`）。

**prompt 槽位的接管优先级链。** 卡片与列表浮层不画在屏幕上，而是**抢占输入框那块矩形**，顺序在高度计算处就写死了（`app/agent_view/render.rs:806-880`）：`permission card`（`slot_card == Permission`，`:817-829`）→ `question card`（`:830-842`）→ `elicitation card`（`slot_card == McpElicitation`，`:843-855`）→ `/rewind` 浮层（上面三个高度都为 0 且 `rewind_state` 存在，`:856-865`）→ `cancel-turn` 面板（`slot_card == CancelTurn` 且 rewind 为 0，`:866-871`）→ `/jump` 浮层（`jump_slot_taken()` 为假，`:872-880`）。`prompt_height` 用同一顺序取第一个非 0 的高度（`:943-965`），绘制端也用同一顺序 `if … else if`（`:2378 → 2460 → 2772 → 2781 → 2790 → 2794`）——**「高度求解与绘制必须同序」**保证了不会出现「预留了行数但没人画」；槽位接管有全局上限 `max_prompt_height = area.height / 2`（`:806`）。**排队只有 permission 是真队列**：`permission_queue: VecDeque<PermissionViewState>`（`app/agent_view/mod.rs:1327`，初始化 `app/agent_view/session.rs:383`），所有读写只碰 `front()` / `front_mut()`（`key_owner.rs:96-108, 155-159`），因此多个授权请求 FIFO 排队、一次只显示一个；其余模态是「后开覆盖先开」的单槽位语义（例：`Ctrl+P` 打开命令面板时直接 `self.active_modal = Some(...)`，`app/agent_view/input.rs:1127`）。

**嵌套的三种实现手法（不是栈）：**

| 手法 | 例子 | 证据 |
| --- | --- | --- |
| 同一 popup 内换内容（状态搬迁） | `ResetSettingsConfirm` 把 `SettingsModalState` 整体搬进自己的枚举变体，取消时再搬回去，从而保留过滤/滚动位置 | `views/modal.rs:304-313`；`app/modals.rs:2376-2400` |
| 后画覆盖前画 | `/config-agents` 画完 agents_modal 后若 `persona_detail` 存在就再画一层 | `app/agent_view/render.rs:3894-3918` |
| 快照 + 回退 | 命令面板 → 参数选择、DocPicker → DocViewer 把上层状态存进 `previous_palette: Option<PaletteSnapshot>`，Esc 时还原 | `views/modal.rs:331-335, 243-245`；`app/modals.rs:174-180, 278-282` |
| 宿主内部局部浮层 | ManagedConnectorsWait「盖住列表」；`overlay_content_rect` 给出「距 popup 左右各 1 列、从 tab bar 下两行到底部」的内嵌矩形 | `views/managed_connectors_wait.rs:1-5, 126`；`modal_window.rs:1076-1093` |

**焦点排名表 `KeyOwner`**（`app/agent_view/key_owner.rs:38-53`）：枚举为 `{ LineViewer, BlockViewer, Card(BlockingCard), PlanApproval, Pane }`，排名实现在 `key_owner_when_parked`（`:110-132`），逐条 `if/else if`：① `Permission` 卡片（**永远最高**，注释 "a permission request interrupts whatever else is waiting"）；② `LineViewer`；③ `BlockViewer`；④ `PlanApproval`；⑤ 其余卡片固定序 `CancelTurn → Question → McpElicitation`（`:96-108`）；⑥ `Pane`（普通面板 + 全局绑定）。`parked` 布尔量（键盘停在 scrollback）会把卡片与 plan approval 摘出排名，使「卡片还画着但键盘在滚动区」成为可表达状态（`:114-116, 142-153`）。

**`EscStep`：ESC 是一级一级的阶梯**（`key_owner.rs:59-91`），8 个可枚举档位，每档都有给快捷键栏显示的标签（`label()`，`:78-91`）：

| 档位 | 含义 | 用户可见标签 |
| --- | --- | --- |
| `DismissFileSearch` / `LeaveTextInput` | 关掉卡片输入框上的 `@` 文件搜索下拉 / 从卡片文本框退回其选项行 | `dismiss` / `back` |
| `DiscardPatternEdit` / `ClearSelection` | 丢弃半成品的 always-allow 规则编辑 / 取消当前问题的选择 | `cancel` / `unselect` |
| `BackOutOverlay` / `ParkFocus` | 退回 dashboard（卡片保持挂起）/ 把键盘交给滚动区，卡片继续画着 | `dashboard` / `scrollback` |
| `KeepRunning` / `DismissElicitWaiting` | 关掉 cancel-turn 面板，回合与子代理继续跑 / 关掉 elicitation 的等待态 | `keep running` / `dismiss` |

设计意图写在注释里：**「每个在卡片上标注 Esc 的提示都读这张表，每个拥有该键的处理函数也都按它分发，因此快捷键栏不可能承诺一个按键到不了的档位」**（`:55-57`）。`card_esc()` 按当前焦点算出该走哪一档，`handle_card_esc()` 执行（`:167-250`）；`KeepRunning` 的实现特意不用 `ContinueToRun`，因为那仍会取消父回合（`:241-244`）。

**输入派发顺序**（`app/agent_view/input.rs:329` 起，先匹配先返回）：scrollback drag latch（`:335`，拖拽中的鼠标事件先被吞）→ 子代理全屏接管（`:361`）→ `/jump` 抑制期的 Esc 吞掉（`:364`）→ wrap-host 图片粘贴（`:372, :378`）→ Esc+d 组合 → `DumpInputLog`（`:386`）→ `image_viewer`（`:399`）/ `video_viewer`（`:428`）/ `gboom`（`:439`）→ `workflows` 浮层（`:454`）/ `goal_detail`（`:457`）→ `active_modal`（`:488`，**早于 btw、早于卡片**）→ `/btw` 面板（`:501` 起：Esc → dismiss、关闭按钮、悬停、滚动）→ 扩展模态（`:642`）、`persona_detail`（`:646`）、`agents_modal`（`:663`）、`block_viewer`（`:681`）→ `key_owner() == PlanApproval`（`:800`）、Permission 卡片（`:725`）。路由到最上层的方式是每层返回 `InputOutcome`（`Changed`/`Action` 表示已消费，`Unchanged` 表示「我没吃这个键」），穿透规则只有两条例外值得记：`active_modal` 分支**只放行 `ActionId::Quit`**（`registry.lookup(key, When::Always) == Some(ActionId::Quit) → Unchanged`），其余按键全部交给模态自己，即 **Ctrl+Q 是唯一能穿透模态的全局键**（`:490-495`）；`extensions_modal` / `persona_detail` / `agents_modal` / `block_viewer` 则更宽——**任何 `When::Always` 的全局动作都放行**（`:644-688`），所以 `views/extensions_modal.rs:3` 的「Blocks all input until closed with Esc」严格说只在非全局键上成立。模态窗口自身的键盘处理极小：`handle_modal_key` 只认 `Esc → CloseRequested`、`Left/h`、`Right/l`（后两者在 `tabs_focused` 时直接 `Unhandled`，否则按 `fold_info` 返回折叠类结果），其余一律 `Unhandled` 交给各模态内容处理器；**Tab 不由 chrome 处理**，由各模态/Picker 自己实现（`modal_window.rs:899-942`）。

**模态内的焦点循环**以 picker 系为样本：三个焦点区 **tab bar / 搜索框 / 列表**，用 `PickerState` 的两个布尔量 `search_active`、`tabs_focused` 表达（`views/picker.rs:1474-1503`）。搜索框内按 `Tab`/`BackTab` 也换 tab（`:2693-2703`）；搜索框内按 `Up` 若配置了 tab 就交给 tab bar，否则跳列表最后一项，按 `Down` 跳到第一个可选项（`:2717-2746`）；`tabs_focused` 下 `Down`/`j` 回到搜索框（`:2762-2770`），列表底的 `Down` 与列表顶的 `Up` 把焦点交回，注释称之为 "completing the arrow cycle"（`:2758-2762`）；`Left`/`Right` 按 `(active_tab ± 1) % tab_count` 环绕（`:2965-2980`）；行导航有 `j`/`k` 别名（条件 `config.show_search_hint || config.vim_normal_first`，`:2755-2757`）。vim 模式下 `i` / `/` 是唯一进入搜索态的方式，普通可打印字符不输入（`:2994-3001`）；`push_vim_nav_search_hint` 会在页脚补一条 `i search`，非导航态或 vim 关闭时不加（`modal_window.rs:186-197`）。

**其它输入捕获规则**：粘贴（`Event::Paste`）按同一条优先级链单独分发（`handle_modal_paste`，`input.rs:497`），卡片在 `FollowupInput` 焦点下把粘贴路由给输入框、在 `PatternEdit` 下逐字符插入并过滤 `\n\r`（`:787-802`）；鼠标先判关闭按钮 → tab → 可点击页脚 → **点在 popup 之外直接 `CloseRequested`（点击穿透到背景即关闭）**，`Moved` 只更新悬停并返回 `Handled`，最后落到 `Unhandled` 交给内容（`modal_window.rs:981-1026`）；**Ctrl+Q 永远保留**，出现在几乎每一层的最前面（`input.rs:402, 431, 445, 492, 731`），是「卡在某一层出不来」的最后退路。

**关闭 / 取消 / 确认的统一语义**：`Esc` = 关一层，且这一层由 `EscStep` 或模态自身的子模式状态决定——设置模态在 `FilterFocused` / `PickingEnum` / `PickingGroup` / `EditingValue` 四个子模式下**先短路**，只有顶层才走宿主的 `CloseRequested`（`app/modals.rs:399-431`）；MemoryBrowser 同理（`:369-397`）。点击 popup 之外或 `[✗]` 都是同一个 `CloseRequested`，关掉时重建/清空 hit 状态以避免鼠标点到上一帧的幽灵矩形（`modal_window.rs:253-258`、`settings_modal/render.rs:147-155` 的 `reset_hit_rects`）；命令面板与 DocPicker 的 Esc **第一跳不是关闭**而是先清空查询（`esc_clears` 标志），查询已空才关（`app/modals.rs:130-172`）。**不可逆动作按代价分四档**：① 双次按键 + TTL（最轻）——`requires_confirmation` 的动作第一次只装填 `PendingAction`，`PendingAction::TTL = 1000ms` 内再按一次才执行，快捷键栏显示 "press again to {label}"（`app/app_view.rs:460-476`），空闲态 `Esc Esc` 的窗口单独定义为 `ESC_DOUBLE_PRESS_TTL = 800ms`（`:475-476`，与 `actions/defaults.rs:436` 一致）；② 装填 + `y`/`n` 二次确认——会话删除用 `PendingDelete { source, session_id, cwd }` 记住目标（注释解释「即使列表发生位移，`y` 仍绑到正确目标」），`handle_pending_delete_key` 规定 `y` 确认、`n` 取消、**任何其它无修饰键先把装填清掉但仍继续处理该键**（`Disarmed`），且只接受 `KeyEventKind::Press` 且无修饰（`views/session_picker.rs:57-132`）；③ 阻塞式确认对话框——`ModalConfirmation<R>` 把「键 → 结果」写进类型里，一次只允许匹配到的键产生结果（`views/modal.rs:14-44`），`EditConfirmResult` 四态且标签随上下文变化（`save` vs `save & send`、`discard changes` vs `discard & send`，`:60-82`），恢复默认设置的确认**叠在设置模态上**且 `Esc`、`F2`、`Ctrl+,`、`Cmd+,` 全映射到 `Cancel` 而非「关模态」，注释「只有无修饰的按键才算：`Ctrl+Y` 不得触发 Reset」（`app/modals.rs:439-459`）；④ 把危险选项排除出默认路径——`/rewind` 的 `RewindPhase::Confirm` 是**独立的阶段枚举值**而不是模态（`views/rewind.rs:57-78`），"Yes, and don't ask again" 是会写回设置的一等选项，dashboard 覆盖层的 Ctrl+X 明确禁止自动重复（按住键）驱动破坏性动作（`views/dashboard/state.rs:3338-3340`）并用 `CONFIRM_WINDOW` 而非默认 TTL（`app/app_view.rs:480-483`）。反面样本：`app/modals.rs:1718-1719` 留了一条注释——`EditConfirm` 已无绘制分支也不再被布防，理由是「布防它会不可见地捕获所有输入」；**一个没有渲染分支的模态等于键盘黑洞**。

**模态与浮层清单**（`views/modal.rs:180-330` 共 12 个 `ActiveModal` 变体）：

| 名称 | 用途 | 触发 |
| --- | --- | --- |
| `CommandPalette` / `ArgPicker`（`:187-212`） | 模糊搜索所有动作与 slash 命令 / 命令参数选择（model、theme、reasoning effort） | `Ctrl+P` / `?`（`actions/defaults.rs:790-799`）；从命令面板选中带参命令 |
| `SessionPicker` / `DocPicker` / `DocViewer`（`:213-271`） | `/resume` 会话选择（支持深度内容搜索与 `d` 删除）/ how-to 指南列表 / 正文阅读 | `/resume`（`app/agent_view/input.rs:1344`）；命令面板的 HowTo 项 |
| `ShortcutsHelp` / `MemoryBrowser`（`:272-294`） | 按键速查表（可过滤、展开、进详情页）/ `/memory` 浏览器（左列表右预览，可全屏） | `Ctrl+.` / `Ctrl+X`（`actions/defaults.rs:805-817`）；`/memory` |
| `Settings` / `UsageInfo`（`:295-303`） | 设置模态 / 用量与会话信息（多 tab） | `F2` / `Ctrl+,` / `Ctrl+,`(SUPER) / `/settings`（`:843-851`）；`/usage`、`/session-info`、`/context`、context-bar 点击 |
| `ResetSettingsConfirm` / `RememberNoteReview` / `EditConfirm`（`:181-186, 304-330`） | 叠在设置上的「恢复默认」确认 / `#` 记忆笔记预览（Tab 切原始与增强）/ 放弃未保存的队列编辑 | 设置里对某项按重置；提交 `#` 笔记；**`EditConfirm` 已不再布防**（`app/modals.rs:1718-1719`） |
| Extensions / Agents / Persona detail / Feedback / Import Claude | Hooks/Plugins/Marketplace/Skills/Workflows/MCP 六 tab；列出所有 agent 定义；persona 详情与内联编辑（**叠在 Agents 之上**）；反馈编辑器；选择性导入 Claude 设置 | `/hooks`、`/plugins` 等；`/config-agents`（别名 `/agents`）；Personas tab 按 Enter；`/feedback`；`/import-claude` 或欢迎页按 `i` |
| Block viewer / Line viewer / Tutorial / Goal detail / Workflows | 块全屏查看（含搜索、可视选择、复制）；文件与计划预览（带 `[↗][✗]` 按钮与半透明蒙层）；两屏上手教程；目标进度（预算条 + todo + 事件史）；工作流运行列表 | 选中块按 `Ctrl-F`；计划审批预览、prompt 上的文件预览；`/tutorial`；目标 chip（`Esc` 或 `g` 关闭）；overlay 输入路径（`app/agent_view/input.rs:454`） |
| 卡片：Permission / Question / Elicitation / Plan approval / Cancel-turn | 工具授权（选项列表 + followup 输入 + 规则编辑）；`AskUserQuestion` 结构化提问；MCP URL/form elicitation；计划审批（预览关闭时占键盘）；回合运行中询问是否中断 | 代理请求授权 / 提问时；`x.ai/mcp/elicit`；plan 模式提交计划；回合运行中按取消 |
| `/rewind` / `/jump` / `/btw` / New worktree / Managed connectors wait | 回退到某回合（Picker → CancelOffer → Confirm → Executing）；跳到某回合、光标移动即预览；侧问侧答（内容随 Esc 落回滚动区）；新 worktree 命名；等用户完成连接器的浮层（**盖在扩展模态列表上**） | `/rewind`（或空闲空输入框 800ms 内 `Esc Esc`，`actions/defaults.rs:427-436`）；`/jump`；`/btw`；命令面板 New Session in Worktree；MCP tab 的 Ctrl+O / URL 点击 |

`overlay.rs` 里的 `OverlayState` 是**另一类**东西：它服务于可切换的侧栏面板（tracing / todo / 后台任务），语义为快捷键进入时「显示并聚焦」；`Tab` 退全屏并失焦、焦点交回滚动区；`Space` 同上但焦点交给输入框；`Esc`/`q`「退一层嵌套」（全屏时先退全屏，否则整体隐藏）；`Ctrl-F` 切换全屏且**即使在输入条打开时也生效**（`views/overlay.rs:73-142`）。`handle_overlay_key` 与 `handle_overlay_nav_key` 被刻意拆成两个函数，因为调用方需要先判断 `has_input_bar` 再决定是否处理导航键（`:133-156`）。

**小屏与模态相关的部分**（高度阈值见 §4.1）：compact 传导到模态的方式是每个模态自己在 config 里调 `ModalSizing::with_compact(compact)`，参数统一取自 `self.scrollback.appearance().prompt.compact`（例：`app/modals.rs:1762, 2179, 2192, 2248`），而该字段是**派生渲染值**，注释明确「应用可以因终端过矮而强制打开，持久化的用户设置是 `UiConfig::compact_mode`」（`appearance/config.rs:76-80`）。放不下就不画、并把 hit 状态清干净：模态 20×6（`modal_window.rs:253-260`）、`ListOverlay` 在 `height == 0 || width < 10` 不画（`overlay_list.rs:56-58, 90-92`）、`render_btw_panel` 在 `width < 12 || height < 3` 返回（`btw_overlay.rs:219-221`）、settings 在内容区小于 `CONTENT_MIN_WIDTH = 10` 时 `reset_hit_rects` 并返回（`settings_modal/render.rs:152-155`，常量在 `settings_modal/state.rs:28`）。小屏也有用户可用的「放大」入口：memory 模态的全屏变体（`width_pct: 1.0, max_width: u16::MAX, v_margin: 0`，偏好会持久化）、overlay 的 `Ctrl-F`、line_viewer 的 `[↗]`（全屏时主动跳过 dim）。模态自身高度也有弹性：页脚行数随宽度反向增长（窄了要多折行）并由 `shortcuts_rows_needed` 在布局前预测，tab bar 也在窄屏折成多行（`modal_window.rs:363-365, 438-478, 591-614`）。

### 4.4 输入区（prompt widget）

### 本节要点

- 输入区是「**可复用组件 + 独立 textarea crate**」两层：`prompt_widget` 明确不管焦点管理（Esc、Tab）也不画 accent 线/选择框；同一组件被 agent 视图、welcome 屏、dashboard dispatch 框、权限/提问面板复用。
- 视觉是一个四边盒：顶边可内嵌会话标题（草稿 stash 时显示 `Stashed`）、底边内嵌 `模型 · flags` 与 `multiline`，前缀默认 `❯`（宽 2 列）。
- 自动增高**没有组件级行数上限**，上限由调用方给——全屏 agent 视图是 `area.height / 2`，welcome 屏是 `(内容高/2).min(列高)` 且起始 3 行。
- 光标是三层机制叠加：终端光标默认隐藏、位置由渲染结果回传、颜色用 OSC 12 改成主题 `accent_user`；**形状不是按模式切换的**，而是启动时一次性策略 `[ui].cursor_blink`。
- 图片与粘贴都是**与文本同流的原子 chip**（`[Image #N]`、`[Pasted: N lines]`），不是附件栏；chip 是「显示文本 ≠ 缓冲文本」的通用机制。
- queue pane 最多 3 行、只发意图不改业务状态；外部编辑器走「临时文件 0600 → 子进程接管 TTY → 读回校验并剥末尾换行 → stale/非法一律保留原草稿」。

**三层结构**：组件层 `views/prompt_widget/mod.rs`（3820 行，负责 chrome 绘制、提交路由、chip 元素、图片、下拉协调、ghost text）；编辑内核 `crates/codegen/xai-ratatui-textarea/src/textarea.rs`（3482 行，负责文本缓冲、wrap 缓存、撤销栈、kill buffer、选区、鼠标、滚动条）；表现层原语 `crates/codegen/xai-grok-pager-render/src/`（`key` 键位匹配宏、`prompt_images`、`theme`、`render` overlay 几何）。模块注释明确划了责任边界：**它不管焦点管理（Esc、Tab）也不画 accent 线/选择框，那些由调用方负责**（`views/prompt_widget/mod.rs:3-5`）；拆分让同一组件被 agent 视图、welcome 屏、dashboard dispatch 框、权限/提问面板复用（`:240-272` 的 `PromptStyle::overlay()` / `inline(bg)`）。同目录另有 `image_state.rs`（199 行，图片 chip 重绑定）与 `tests.rs`（5273 行，测试量是主体的 1.4 倍）。

**视觉与状态。** `PromptStyle::default()` 的几何参数为 `vpad_top = 1, chrome = true, chrome_pad_left = 2, chrome_pad_right = 1, show_prefix = true, show_accent_line = false, show_borders = true, image_preview = true`（`:216-238`）。按 `PromptWidget::draw`（`:2971-3474`）的绘制顺序得到：

```
 ╭─────────────────────────── 会话标题(可省) ───────────────╮   ← 顶边（vpad 行，show_borders 时）
 │                                                          │
 │  ❯ 输入文本在这里，长行自动软换行…                        │   ← ❯ 前缀(2 列) + TextArea
 │    续行                                                   │
 │                                    [Pasted: 12 lines]    │   ← chip 与文本同流排版
 ╰───────────────────────── grok-3 · plan ──────────╯        │   ← 底边内嵌 info 行
   ↑ 若 show_accent_line，最左列整列画 ┃
```

- 顶边 `╭─╮`（`:3049-3054`）、侧边 `│`（`:3330-3335`）、底边 `╰─╯`（`:3356-3361`）；accent 竖线用 `crate::glyphs::accent_bar()`，默认关闭且关闭时该列归还给内容（`:176-178, 3001-3010`）。前缀默认 `❯`（`PREFIX_WIDTH = crate::glyphs::PROMPT_ARROW_WIDTH`，`:693-694`），三种覆盖：历史搜索时 `? `、模式覆盖（bash `! ` 黄、remember `# `）、普通 `❯`（`:3084-3104`）。
- 未聚焦时对盒内内容整体向背景色衰减 0.66，边框色切到 `theme.prompt_border`（`:2990-2994, 3378-3389`）。顶边可内嵌会话标题（`style.title`），与底边 info 行同款样式；草稿被 stash 时标题为 `Stashed`，再与 `/rename` 标题用 `·` 拼起来（`:3060-3081`；`app/agent_view/prompt_stash.rs:41-58`）。底边 info 行左右两段：左 `模型名 · flag1 · flag2`（flag 由 `mode_flags()` 生成，plan 用 `accent_plan`、非 Ask 的权限用灰或蓝），右 `multiline` 与 `usage_warning`（`:291-351, 3489-3594`）。
- 占位符默认文案写死在 `draw` 里：`"Build anything"`，只在文本为空、且（未聚焦或 `placeholder_when_focused`）且没有语音 interim 时显示，并按输入区宽度截断（`:3310-3321`）。主 composer 默认「聚焦时隐藏占位符」（`placeholder_when_focused = false`），feedback 框保留；remember 模式文案是 `"Save a memory note... (Shift+Enter for multiline)"`，多行模式下换成 `"... (Enter for newline, Shift+Enter to save)"`（`app/agent_view/mod.rs:370-381`）。

**光标三层机制。** ① 终端光标默认被隐藏：初始化时 `execute!(stderr, …, cursor::Hide)`（`app/mod.rs:1503-1508`）。② 位置由渲染结果回传：`draw` 返回 `PromptRenderResult{ cursor_pos, post_flush_escapes }`，`cursor_pos` 只在 `style.focused` 且没有「空输入 + 语音 interim」时给出，否则 `None`，该值一路上抛到 `app/app_view.rs:4833-4846` 由终端写入（`:3391-3399, 404-411`）。③ 颜色用 OSC 12：`cursor_color_escape()` 产出 `\x1b]12;rgb:RR/GG/BB\x07`，取色为当前主题的 `accent_user`，启动时 `apply_cursor_color()` 写入，退出/切主题用 OSC 112 还原，并用 `CURSOR_COLOR_APPLIED` 原子开关保证「只撤销自己画过的」（`theme/mod.rs:559-596`）；`terminal` 主题（不涂背景）特意不动光标颜色（`docs/user-guide/06-theming.md:26, 141`）。**形状不是按模式切换的**：`[ui].cursor_blink` 三态 → `CursorStylePolicy::{Inherit, ForceBlinking, ForceSteady}`，未设置时**不发任何转义**、保留终端自己的形状/闪烁，强制时分别发 `EnableBlinking + SetCursorStyle::BlinkingBlock` 或 `DisableBlinking + SetCursorStyle::SteadyBlock`（`app/mod.rs:1427-1446, 1509-1523`）。**vim / 多模式下的光标差异：未找到证据**——输入区没有 normal/insert 两态，`/vim-mode` 只改 scrollback 键位，注册表的 `key_for_mode` 也只对 `When::ScrollbackFocused` 的裸字母键做替换（`slash/commands/vim_mode.rs:14-16`、`actions/mod.rs:448-463`）；多模式真正改变的是**前缀色与文案**（Normal/Bash/Remember 三态，`app/agent_view/mod.rs:355-388`）。

**高亮只覆盖命令与 chip**（没有通用语法高亮，也不按语言/任意路径着色）：`/命令` 青色高亮取 `theme.accent_skill`，嵌入式最小模式退化为 `text_primary`；着色条件严格——行首 `/` token 在「下拉打开或命令已识别」时着色，文本中段 token 只在被真正识别（hoist）或是 skill/plugin 提及时着色（`:3137-3165, 3212-3222`，着色函数 `paint_slash_token_highlight` `:2946`）。另有参数占位 ghost（命令声明 `arg_placeholder` 且参数未输入时，在光标后以 `theme.muted()` 画，`:3180-3194`）与 `/命令` 不完整后缀 ghost（`:3196-3210`）。

**chip 是「显示文本 ≠ 缓冲文本」的通用机制**：`@文件引用` 被接受后不是普通文本，而是 `KIND_FILE_REF` 原子元素，缓冲区文本是 `@path`，显示由 `file_ref_display()` 生成（`@` 与 `:` 灰、路径用 `theme.path`、行号用 `text_primary`，支持 `@foo/bar.rs:10-12`，`:2076-2095, 3620-3633`）；粘贴 chip 在行数 ≥ 4（compact ≥ 2）或字节数 > `PASTE_CHIP_DISPLAY_BYTES = 10_000` 时生成，标签 `[Pasted: N lines]` / `[Pasted: 1.0 MB]`（十进单位，`:47-48, 2306-2325`），**再粘一次相同内容 = 就地展开**该 chip 为全文（`:2288-2299`）；图片 chip 是 `[Image #N]`，样式为方括号用 `theme.paste_dim`、内容用 `theme.paste_fg`、背景 `theme.paste_bg`（`:3760-3810`）。`TextArea` 为每个元素保存可选 `display: Line`，渲染时按显示宽度重算 x 坐标；显示超宽时截断且**保留右括号**，于是 `[Pasted ~10 lines]` 变成 `[Pasted ~1…]` 而不是 `[Pasted ~10…`（`xai-ratatui-textarea/src/textarea.rs:3242-3307, 3408-3446`）。多个元素重叠一行时按显示宽度逐段推进，这也是「文本里的 `[Image #1]` 不会破坏命令解析」的原因——斜杠系统看到的是剥离了所有元素占位符的干净文本（`:3654-3695`）。

**自动增高与上限。** 高度公式（`PromptWidget::desired_height`，`:1601-1627`）为 `height = vpad_top + textarea.desired_height(text_width) + info_block`（`info_block` 有 info 行则 1），再 `clamp(min.min(max_height), max_height)`，其中 `min = vpad_top + 1 + info_block`。上限不是组件常量，而是调用方给的 `max_height`：全屏 agent 视图 `max_prompt_height = area.height / 2`（未聚焦且开了 `collapse_unfocused` 时折叠到 `vpad_top + 1 + info_block`，`app/agent_view/render.rs:806-814`）；welcome 屏 `PROMPT_HEIGHT = 3` 是起始高度，`prompt_max_height = (content_height / 2).min(column_fit).max(PROMPT_HEIGHT)`（`views/welcome/mod.rs:177-194`）。两个抑制抖动的细节：**浏览历史时冻结为 1 行**（`history_search.is_browse()` 时 `text_height = 1`，否则在不同高度的历史条目间上下移动会导致每按一次键布局就重排，`:1614-1621`）；**内容超出可视高度时出现滚动条**（`TextArea.show_scrollbar` 默认 `true`，启用时最右一列留给轨道、正文按 `width - 1` 换行，`textarea.rs:224-237, 3165-3223`）。行高由 wrap 缓存计算：宽度变化才重算；无自定义显示的元素走 `textwrap` 的 `WrapAlgorithm::FirstFit`，有 chip 元素时改走 `element_aware_wrap_ranges`——**把元素当作原子单位**，放不下就整体挪到下一行，元素内部的换行符不产生新视觉行（`:2885-2930`）。

**多行编辑。** 软换行是显示层的、缓存为字节区间 `Vec<Range<usize>>`（`WrapCache{width, lines}`，`:250-254`），上下移动按**视觉行**走并带 sticky 列 `preferred_col`（`:2375-2501`）；Home/End 默认按视觉行，Ctrl+A/Ctrl+E 按逻辑行（`:2503-2533`）。硬换行只有几条来源：`Shift+Enter`/`Alt+Enter`（`crate::input::is_mod_enter`）、Kitty 送达的 `SUPER+Enter`、Apple Terminal 的 CoreGraphics 修饰键补救、裸 `Ctrl+J`/`Ctrl+M`、以及**反斜杠续行**（光标前是 `\` 时按 Enter 用换行替换它）（`:1813-1819, 2136-2188`；`textarea.rs:2005-2014`）。裸 Enter 是**提交**：`route_enter` 判定顺序为「文件搜索下拉打开 → PassThrough」「Apple Terminal 换行修饰键 → 插入换行」「反斜杠续行 → 插入换行」「裸 Enter → Submit」（`:2157-2188`，语义枚举 `:92-103`）。`/multiline`（输入框聚焦时 `Ctrl+M`）把 Enter 与 `Shift+Enter` 语义对调，并在 info 行右侧显示 `multiline`（`ActionId::ToggleMultiline`，`app/agent_view/prompt.rs:486-529, 598-600`）。

历史条目就是 `HistoryEntry { text: String }`——多行提示词是一个含 `\n` 的字符串，不按行拆分；模糊匹配在后台 nucleo 线程做，前端快照最多 `MAX_RESULTS = 100` 条（`views/history_search.rs:4-28, 55`）。两个入口共享同一面板（`Mode`：`Search` / `Browse`）：`/history` 是 Search（输入框当过滤词，导航只高亮，Enter/Tab 接受），空输入框按 `↑` 是 Browse（每移动一格把整条历史灌回输入框，输入即「脱离」变成编辑，到底部最新一条再按 `↓` 关闭）（`:264-301, 381-395`；`app/agent_view/prompt.rs:709-737`）。Browse 打开时选中项吸附在最新一条（`stick_to_bottom = true`），用户一旦导航就清除（`:286-290, 404-418`）。`↑` 的完整路由：仅当模式为 Normal、输入为空、无文件搜索时生效；**队列非空时优先切焦点进 queue pane**，否则才开历史面板；`↓` 永远不会打开历史面板（`app/agent_view/prompt.rs:407-424`）。另有独立的 **stash**（`Ctrl+S`/`Alt+S`）：整个草稿（含图片、chip、`!` 模式）存一个槽位，空输入框按同一键恢复；被替换掉的旧草稿**只有文本**进 `↑` 历史（`app/agent_view/prompt_stash.rs:1-26`）。

**撤销是快照式**：`UndoEntry{text, cursor, elements}`，栈深上限 **100**（`UndoState.max_depth`，`textarea.rs:269-330`）。分组规则（`MutationKind`，`:279-293, 2096-2126`）：连续同类输入合并为一步，`Kill`/`Element`/`Replace` 永远单独成步，**光标跳变也断开分组**（比较 `last_cursor`）；宿主驱动的 `set_text` 故意留一个检查点（误替换可撤销），`clear_history()` 才是硬重置。键位：Ctrl/Cmd+Z 撤销、重做为 Ctrl+Shift+Z / Alt+Z（为把 Ctrl+Shift+Z 送成 Ctrl+Z 字节的终端准备）/ Ctrl+R（`:2023-2055`）。chip 也进撤销快照（`elements` 一起存），图片则另有 `image_undo_stash` 在撤销后重挂（`:656-666`；`image_state.rs:30-40`）。**kill / yank 是单槽缓冲，不是 kill-ring**：`TextArea` 只有一个 `kill_buffer: String`，没有环、没有 append 语义（`:184-186`）；kill 命令为 Ctrl+K（到行尾）、Ctrl+U（到行首）、Ctrl+W（空白分词向后删）、Alt/⌘+D 与 Alt+Ctrl+H（向前/向后删词），`kill_current_line` 在空行上吞掉前一个换行以合并行（`:2290-2339`；`editor_keys.rs:46-74, 129-155`）；Ctrl+Y 是 yank，且**撤销不恢复 `kill_buffer`**（注释明确写了 yank 与 undo 分离，`:2092-2093`）。

其它编辑行为：选区来自鼠标拖拽/双击/三击、Shift+方向键扩展，`selection_style` 默认 tokyonight 蓝底 `rgb(49,62,115)` + 亮前景 `rgb(192,202,245)`（为在任何终端配色下都可读，`:188-208`）；`Cmd+A` 全选被**显式限制在 Ghostty**（其他终端的 Cmd+A 属于终端自身的 Select All，不该抢），且需用户在 Ghostty 里写 `keybind = cmd+a=unbind`（`:133-138, 1861-1873`）；`Ctrl+C` 在文本非空时清空草稿并返回 `Edited`，空则 `Ignored` 交给上层（`:1821-1829`）；粘贴分流——`Ctrl+V`/`Cmd+V` 被拦截走 `handle_paste()`（否则多行内容会被内联插入而不是变成 chip），`Ctrl+Shift+V` 是「内联粘贴」不做 chip（`:1831-1859`）；`tab_width = 4`，显示与插入时都把 `\t` 展开成空格、由外观设置热同步（`textarea.rs:244-247`）；鼠标滚轮可临时脱离光标独立滚动（`scroll_override`），一旦光标移动就弹回，拖拽到区域外会节流加速滚动（`:211-223, 354-376`）。

**附件：图片是与文本同流的原子 chip。** 单条提示最多 `IMAGE_CAP = 10` 张图，后端拒绝任一边 < 8 px，插入前先校验并给出可直接显示的文案（`:2338-2361`）。插入过程是 `begin_undo_group()` → 插入 `KIND_IMAGE` 元素 → 补一个空格 → `end_undo_group()`，即**一次撤销就整体消失**，同时记录 `post_insert_image_preview` 让预览立刻出现（`:2363-2395`）。缓冲文本是规范的 `[Image #N]`，chip **不带路径**，路径只出现在预览浮层里。预览触发条件（任一）：光标在 chip 上或正好在其右边界、鼠标悬停该 chip、或刚插入且光标未移动（`:2594-2607`）；渲染走 `render_image_overlay`，返回**flush 之后**要写的终端转义（Kitty/iTerm2 内联图片），且每帧若不重画就主动清掉 overlay 的 ID 1（`:3441-3468`；几何常量 `MIN_BOX_WIDTH` / `MIN_META_BOX_HEIGHT` / `MIN_PIXEL_BOX_HEIGHT` 见 `render/image_overlay.rs:24-33`）。「chip 文本变回纯文本」被显式兜底：yank、撤销、纯文本粘贴后若 `[Image #N]` 还在而记录仍在 `images`/撤销暂存里，`rechip_orphan_image_placeholders` 会重新注册 chip；找不到记录的占位符就作为纯文本发送并弹 toast（`image_state.rs:1-40`）。「有附件」还会挡住外部编辑器：`ATTACHMENT_MESSAGE = "External prompt editing is not available while the draft has attachments."`（`app/external_editor.rs:12-13`）。

**排队消息（queue）的界面。** queue pane 是独立的 `ListPane`，**只在非空且可见时占位**（`is_visible()` = `overlay.visible && !entries.is_empty()`，`views/queue_pane.rs:1-5, 583-585`）：

- **高度上限 3 行**：`MAX_QUEUE_HEIGHT: u16 = 3`，`desired_height() = entries.len().clamp(1, 3)`，隐藏或为空返回 0（`:348-349, 599-606`）。**与输入框的空间竞争**：面板最多吃 3 行、属于输入框上方的弹出层；选中行若为多行，还会在 overlay 区域画全文预览（`:1011-1021`）；最小模式另有 dock 布局用 `queue_body_rows` 分配队列区域（`views/dock/layout.rs:73`、`views/dock/mod.rs:153`）。
- **行内容**：前缀是 1-based 位置号 `#N`（灰色），内容取**第一个非空行（trim 后）**，多行提示尾部加 ` (+N lines)` / ` (+1 line)`；前缀宽度按最大位置号动态算 `2 + digit_count(max_pos)`（`:104-134, 182-200, 296-305, 868-875`）。**三种动作按钮**右对齐从右往左摆 `[cancel]`、`[edit]`、`[Send now]`，只在**鼠标悬停的那一行**或**面板聚焦时的选中行**上绘制（悬停优先），且当一行放不下 `[Send now] + [edit]` 时会主动放弃 `[edit]` 保住 `[Send now]`；按钮 rect 每帧重绑、悬停状态跨帧保留（`:351-416, 921-1009`）。
- **键盘**（`:608-657`）：interject 键 → `ForceInterject`；`x`/`Delete`/`Backspace` → 删除；`e`/`Enter` → 编辑；`Shift+J` / `Shift+K` → 与下/上相邻行交换。所有动作先过 `capabilities`（`can_delete`/`can_edit`/`can_reorder`/`can_send_now`），不可编辑的行尾部标 ` · protected`（`:90-91`），删除后选中邻居而不是跳回顶部（`:700-712`）。面板**从不直接改业务状态**，只发 `QueueEvent` 意图，由 `AgentView::handle_queue_key` 落到 `pending_prompts`（`:331-346`）。数据有两个来源，删除/重排按 origin 路由：本地 `pending_prompts`（Local）与服务器权威共享队列（Server，走 `x.ai/queue/*`），server 行的 id 是其 `prompt_id` 的稳定 hash 并置最高位以免与本地自增 id 冲突（`:33-50, 80-88`）。
- **编辑流程**（`app/queue_edit.rs`）：进入时把当前草稿 stash 一次（`stash invariant` 只在 `enter_queue_edit` 设置、只在退出时 `take()` 一次），提示模式切到 `PromptMode::EditingQueued{id, original, server_id, kind}`；`Shift/Alt+Enter` 仍是插换行、裸 Enter 保存；乐观队列行（enqueue RPC 未确认）按下编辑会提示 `"Still queueing, try again in a moment"`（`:16-18`）；脏草稿时切换面板会被挡住，且**不会**去打开一个「画不出来却吞输入」的确认弹窗（`:4-8`）。相关联的另一处编辑是**就地编辑历史用户消息**（双击/Enter 进入、Enter 提交会 rewind 后重发）——总开关 `INLINE_EDIT_ENABLED: bool = false`，**功能当前是关闭的**，它复用同一套 `TextArea` + `TextAreaState`（`app/inline_edit.rs:1-6, 25-45`）。

**外部编辑器（`$EDITOR`）集成。** 入口：最小模式绑 `Ctrl+G`，全屏 TUI 走 `/edit-prompt` 或命令面板里的 **Edit Prompt in External Editor**（`docs/user-guide/03-keyboard-shortcuts.md:259, 273`；`actions/mod.rs:102`、`actions/defaults.rs:20`）。流程（`app/dispatch/external_editor.rs:8-41` + `app/external_editor.rs` + `app/event_loop.rs`）：`dispatch_edit_prompt_external` 先检查「已有 `pending_editor` 就直接返回」「语音进行中 / 粘贴探测未完成 / 有附件 → 直接拒绝并给出文案」，再构造 `PendingEditorRequest::PromptDraft{agent_id, original_text}`；`prepare()` 做 revalidate（可能被别的输入面抢走）→ 解析编辑器 → 写 `grok-prompt-<uuid>.md`；`suspend_for_child()` 让子进程接管 TTY、输入读取暂停（超时会 requeue 并报「等待中」）；`finish()` 成功则读回并校验，失败 / 非零退出 / 非法 UTF-8 / 超 4 MiB / 草稿已被改动都**保留原草稿并提示**。

- 编辑器解析顺序 `$VISUAL` → `$EDITOR` → `vi`，用 `shlex` 切分（允许带引号的参数），解析失败提示 `could not parse $VISUAL or $EDITOR`（`:123-141, 474`）。临时文件 `std::env::temp_dir()/grok-prompt-<uuid>.md`，unix 下 `mode(0o600)`，`Drop` 里删除（`:72-121`）。
- 读回契约：上限 `PROMPT_EDITOR_MAX_BYTES = 4 * 1024 * 1024`；非 UTF-8 拒绝；**剥掉编辑器自动追加的那一个末尾换行**——判定依据是「原草稿本来就不以 `\n` 结尾」，所以草稿自己的换行原样保留（`:11, 97-110, 289-301`，测试 `:600-646`）。**stale 保护**：编辑器期间草稿被改过就丢弃编辑器结果并提示 `"The draft changed while the external editor was open; the newer draft was kept."`（`:29-30, 313-335`）。
- 成功路径 `apply_prompt_text()`：`history_search.deactivate()` → `set_text` → **`clear_history()`**（外部编辑器写入是一个新的起点，旧撤销栈作废）→ 光标移到末尾 → `refresh_slash()` → 清空 prompt suggestion（`:337-347`）。四种拒绝理由（附件 / 语音 / 粘贴处理中 / 输入面已被占用）各有一条用户可见文案（`:12-19`）；最小模式下失败信息 push 进 scrollback，全屏下走 toast（`:244-260`）。挂起/恢复由 `suspend_for_child` + `restore_after_child` 处理，并有超时重试与「正在等待」提示（`app/event_loop.rs:769-840`）。

**提示语 / 建议 / 幽灵文本。** 两种 ghost 都在文本之后渲染、**从不修改文本**：① shell 补全 ghost（`SuggestionController`，ACP `x.ai/suggest`）——打字去抖后请求，带 generation 丢弃过期响应，命中前缀时本地裁剪 ghost（progressive matching）而不发请求（`views/suggestion_controller/mod.rs:1-6, 34-46, 849-853`）；② 下一句提示 ghost（`PromptSuggestionController`，turn 结束后问 shell `suggestPrompt`）——同 generation 机制，**可见性每帧从当前文本推导**，不按键维护状态机（`views/prompt_suggestion.rs:1-14, 32-70`）。两者都画在光标处、用 `theme.ghost_text_style()` 并截断到行尾（`:3401-3426`）；shell ghost 额外要求**光标必须在文末**、且没有 slash 高亮/内联 ghost（`:3404-3415`）。预测 ghost 的门：`prompt_suggestion_active`（每帧由 AgentView 在绘制/按键前设置，turn 运行中、bash/remember 模式、编辑队列行时为 false）→ 没有 shell ghost → 没有文件搜索/历史面板 → 光标在文末 → 没有 slash 活动（`:807-825, 651-654`）；文本不匹配时 ghost 直接消失、清空后回来，按 Esc 在空输入框上会 dismiss 直到下一条建议（`views/prompt_suggestion.rs:75-102`；`app/agent_view/prompt.rs:374-396`）。接受方式：预测 ghost 用 `Tab` 或 `→`（`→` 在文末本来就是空操作，沿用 fish/zsh autosuggest 惯例），且**优先于动作注册表**，以免 Tab 被抢去切到 scrollback（`app/agent_view/prompt.rs:342-373`）；shell ghost 支持整词或整段接受（`AcceptMode`，`:790-800`）。打断保护：任何下拉/面板打开时 `any_dropdown_open()` 为真，临时提示条（ephemeral tip）会主动让位；另有按配置开关的上下文提示（undo 提示 / plan 关键词提示），默认两个都关时整段 per-keystroke 扫描被跳过（`:860-867, 1682-1691`）。

**本节的不确定项（按上游原文保留）**

- `ModalWindow` 边框的「accent 色」说法：模块注释写 "accent-colored square border"，代码用 `theme.gray_dim`——**以代码为准**，引用视觉描述时建议写「灰色直角边框，标题加粗」。
- `render_modal_window` 的 `Clear` 是否算遮罩：代码只重置被覆盖的矩形，**未找到**「模态打开时整屏变暗」的证据，除非该浮层单独调用 `dim_area`。
- `ActiveModal` 变体数量：`views/modal.rs:180-330` 共 12 个，未在 `src/views/` 之外找到另一份并行的「模态枚举」。
- `extensions_modal` 的 `Enter`/提交类确认语义：只核到它用 `ModalWindowState::with_tabs` 与「Esc 关闭」，其 tab 内部的具体确认流程在 8000 行文件里未逐条通读，**未找到**可引用的统一约定。
- 统一主题/常量表：`modal_window.rs` 内的 `footer_lines`、`v_pad` 等没有集中常量，都是各模态各自字面量；`ModalSizing::medium()` 存在但**未找到生产调用点**（grep `medium()` 于 `xai-grok-pager/src` 无命中）。
- 输入区的「vim / 多模式光标差异」：**未找到任何**按模式切换光标形状或颜色的代码，也没有 normal/insert 两态。
- 任意文件路径的通用高亮：除 `@` 引用 chip 之外未见实现；`@` 补全下拉行内的高亮属于下拉组件而非输入文本。
- 占位符 `"Build anything"` 之外的默认值、以及 banner/tip 与输入框的具体遮挡规则没有逐帧验证（`any_dropdown_open()` 的让位语义只读到公开方法层）。
- agent 主视图的横向降级：**未找到证据**；横向降级只发生在 dashboard（`MIN_DASHBOARD_WIDTH = 40`）、timeline rail（`MIN_TERMINAL_WIDTH = 60`）与 welcome 屏。
