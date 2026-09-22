# 专题 15：多会话界面与 dashboard

> 专题：多会话界面与 dashboard｜调研日期：2026-09-22｜证据范围：`crates/codegen/xai-grok-pager/src/{views/dashboard/*, views/session_picker.rs, views/session_picker_surface.rs, views/session_title.rs, views/picker.rs, views/welcome/mod.rs, views/agent_view/render.rs, app/{app_view.rs, dashboard_session_picker.rs, foreign_sessions.rs, roster.rs, workspace_membership.rs, workspace_sync.rs, workspace_layout.rs, event_loop.rs, dispatch/dashboard.rs, dispatch/session/load.rs, acp_handler/session_notification.rs, dispatch/task_result.rs}}`、`crates/codegen/xai-grok-dashboard-store/src/{types.rs,store.rs}`、`crates/codegen/xai-grok-active-sessions/src/lib.rs`、`crates/codegen/xai-grok-pager-render/src/{util.rs,glyphs.rs}`、`docs/user-guide/23-dashboard.md`、`docs/user-guide/17-sessions.md`（路径均相对 `reference/grok-build/`）

## 0. 先分清三个列表屏

Grok Build 里同时存在三个"列出多个会话"的界面，职责互不重叠，代码也完全分开：

| 界面 | 数据来源 | 生命周期 | 入口 | 代码位置 |
| --- | --- | --- | --- | --- |
| Agent Dashboard | 本进程的 top-level agent（+ 子代理、roster、workspace store） | 本进程内存中的活会话 | `/dashboard`、`Ctrl+\`、`grok dashboard` | `crates/codegen/xai-grok-pager/src/views/dashboard/` |
| Session picker（`/resume`） | 磁盘上的历史会话（可含远端 / 外来工具会话） | 进程外、可常驻 | `/resume`、welcome 屏列表、dashboard 的 `Open Previous` | `src/views/session_picker.rs` + `src/views/welcome/` |
| Agents modal（`/agents`） | agent 定义 / persona | 配置 | `/config-agents` | `src/views/agents_modal.rs` |

Dashboard 的模块注释直接把这三者并列写出（`src/views/dashboard/mod.rs:1-4`），用户手册也专门声明"不是 agents modal、不是 session picker、不是 workflows run UI"（`docs/user-guide/23-dashboard.md:9-11`）。本专题聚焦前两者。

Dashboard 的状态宿主是 `AppView::dashboard: Option<DashboardState>`，仅在 `active_view == ActiveView::AgentDashboard` 时激活；`Option` 只在进程关闭时重置，所以关掉再打开会保留选择与折叠状态（`src/views/dashboard/mod.rs:3-4`）。

## 1. Dashboard 的信息架构

### 1.1 一屏的分区（自上而下）

布局是纯函数式计算，产出 9 个具名 `Rect`（`src/views/dashboard/layout.rs:167-191`）：

```
┌ terminal 全宽 ─────────────────────────────────────────────────────────────┐
│  top_margin（空白，高 > 6 行时存在）                                        │
│  main ~/xai [Choose Ctrl+L]      ◆ 2 awaiting │ ⋮ 1 working │ ◇ 1 idle      │ header
│  header_gap（高 > 12 行时）                                                 │
│  + New Agent               Open Previous /resume │ Worktree Ctrl+w          │ actions
│  actions_gap（高 > 13 行时）                                                │
│  ▾ Awaiting 2 ──────────────────────────────                               │ 分节标题
│  ▏● reviewer · audit token flow    Awaiting your input                  2m  │ 行（3 行高）
│   ● implementer · fix login bug    Running: cargo test                 12m  │
│  ▾ Idle 9 ───────────────────────────────                                   │
│   ○ housekeeping                    idle                                 1h │
│   + 5 more                                                                  │ Idle 溢出行
│  dispatch_gap（高 > 10 行时）                                               │
│  ╭───────────────────────────────────────────────╮                          │
│  │ ❯ Dispatch a new agent                        │  ← dispatch 或 peek      │ dispatch
│  ╰─ dispatch ────────────────────────────────────╯                          │
│  shortcuts_gap（高 > 11 行时）                                              │
│  ↑/↓ select (peek) · Enter open · Ctrl+R rename · …                          │ footer
│  bottom_margin（高 > 16 行时）                                              │
└────────────────────────────────────────────────────────────────────────────┘
```

由 `DashboardLayout` 的注释与 `dashboard_chrome_heights()` 得到（`layout.rs:167-191`、`layout.rs:229-257`）。要点：

- 每个装饰行（top_margin / header / header_gap / actions / actions_gap / dispatch_gap / shortcuts_gap / bottom_margin）都是 0 或 1 行，各自绑定一个终端高度阈值，从底部往上一行一行打开；注释明确说这样"每多一行终端高度，要么给列表、要么给恰好一个 gap"（`layout.rs:236-241`）。
- **peek 面板没有独立 rect**：它渲染在 dispatch 的 rect 内，选中行时 dispatch box 被 peek 顶替（`layout.rs:353`）。
- 列表左右各留 `LIST_OUTER_HPAD = 2` 列，dispatch 留 2 列，`header` 与 `actions` 也用 2 列以便与列表对齐（`layout.rs:152-164`、`layout.rs:411-421`）。

### 1.2 表头（header）

左侧是"下一个 agent 会在哪里跑"的位置标签，右侧是状态计数 chip：

- 位置：`branch + " " + [worktree 徽章] + cwd_display`，三段分别上色，整行可点击（hover 加下划线），点击即打开目录选择器（`src/views/dashboard/chrome.rs:129-179`）。
- 右侧 `[Choose Ctrl+L]` 提示只在预算够时出现；位置标签的预算 = 最左 chip 的 x 坐标再退 3 格（`chrome.rs:115-167`）。
- 状态 chip **只统计 `indent == 0` 的行**（子代理随父行归组，不重复计数），顺序是 `awaiting → working → idle → done → failed`，与排序优先级一致；`Inactive` 故意没有 chip，它的数量只出现在分节标题上（`chrome.rs:47-97`）。
- chip 视觉：菱形实心/空心 + ` N label`；working 用点状 spinner 帧（`chrome.rs:66-97`、`src/glyphs` 的 `diamond_filled`/`diamond_hollow`/`dot_spinner_frames`，见 `crates/codegen/xai-grok-pager-render/src/glyphs.rs:111,120,162`）。
- 位置标签左侧永远留 3 格 gutter，保证不会被 chip 压住（`chrome.rs:115-121`）。

### 1.3 动作行（actions）

左端是 `+ New Agent`，右端右到左依次是 `Worktree Ctrl+w`、`Open Previous /resume`；右端项遵循**严格右到左的优先级**：worktree 放不下就整段放弃，`Open Previous` 只在 worktree 已放下且还有空间时才尝试（`chrome.rs:294-339`）。worktree 已武装时文案变成 `+ New Agent in Worktree` 与 `Disable Worktree`（`chrome.rs:264-298`）。光标若停在某个右端项而该帧它没被画出来，会退回到 `+ New Agent`（`chrome.rs:361-370`）。

### 1.4 行的数据模型

`DashboardRow` 是每帧从 `app.agents` 重建的纯视图结构，不缓存（`src/views/dashboard/row.rs:18-59`，`mod.rs:19-22`）。字段即"每个会话显示什么"：

| 字段 | 含义 / 来源 |
| --- | --- |
| `label` | 显示名 → 生成标题 → 首个 prompt 前 30 字符 → `New session #<id8>`（`row.rs:537-567`） |
| `subtitle` | `branch + location[+ " worktree"]`，绘制时以 ` · ` 前置、dim 色（`row.rs:691-735`、`row_title.rs:71-80`） |
| `state` | `RowState` 六态，每帧重算（`row.rs:468-502`） |
| `activity` | 活跃行的一句活动描述（`Running: cargo test` 形态由 `format_activity_label` 生成）（`row_activity.rs:80-100`） |
| `secondary_line` | 第二行：NeedsInput 时是 `Pending: <标题>`，其余是 `last_turn_summary` 或最后一条 agent 消息首行（`row_activity.rs:19-67`） |
| `cwd_display` / `cwd` | `~` 压缩后的展示串与原始路径（`state.rs:4557`） |
| `last_change_at` | `SystemTime`（不是 `Instant`）：roster 时间戳可能早于本进程甚至开机（`row.rs:41-43`、`row.rs:608-621`） |
| `pinned` | 置顶标记 |
| `badges` | 8 种：`worktree`/`needs-input`/`subagents`/`tasks`/`watchers`/`workflows`/`pinned`/`failed`（`row.rs:62-84`） |
| `context_pct` | 上下文占用百分比（用于迷你 gauge），roster 行没有（`row.rs:49`、`row.rs:280`） |
| `indent` | 0 = top-level，1 = 子代理（`row.rs:50-51`） |
| `is_more_placeholder` / `more_count` | "… N more" 折叠行（`row.rs:54-58`） |

### 1.5 单行解剖（宽模式）

```
 ▏ ● implementer · fix login bug · main my-branch worktree  Subagents 2 · Tasks 3 · 12m
 ┬ ┬ ┬                        ┬                            ┬                      ┬
 │ │ └ 状态图标               └ 标题 +（· 副标题）           └ 活工作 chip          └ age / [✗]
 │ └ 缩进 2×indent + 1 格 gap
 └ 选中标记 ▏(U+258F，选中时贯穿两行；悬停不贯穿)
```

列位置由 `render_row` 计算：`marker(1) | gap(1) | indent(2n) | icon(1-2) | gap(1) | label...`（`src/views/dashboard/render.rs:1558-1584`）。行的纵向：`ROW_HEIGHT = 3`（`render.rs:31`），内容 1–2 行并在 3 格内垂直居中（`render.rs:1502-1515`）。背景三态：键盘选中 `bg_highlight` > 鼠标悬停 `bg_hover` > `bg_base`（`render.rs:1517-1526`）。

各状态的图标与颜色（`render.rs:3027-3051`）：

| 状态 | 图标 | 颜色 |
| --- | --- | --- |
| Working | 点状 spinner（`⋅ : ⸬ ⁙` 循环，legacy 控制台降级为 `. : ·`） | `accent_running` |
| NeedsInput | 实心菱形（宽模式闪烁） | `warning` |
| Idle / Inactive | 空心菱形 | `gray_dim` |
| Completed | 实心菱形 | `accent_success` |
| Failed | 实心菱形 | `accent_error` |

右端元数据槽是二选一：默认显示 age，鼠标悬停或已武装删除时换成 `[✗]`（`render.rs:1672-1736`）；只有"可删除"的行才参与（`RowState::allows_delete` = Idle/Inactive/Completed/Failed，`state.rs:206-213`）。age 用的是粗粒度分桶 `just now / 5m / 3h / 2d / 1mo / 1y`，注释说明分桶刻意放宽以"避免秒级抖动"（`crates/codegen/xai-grok-pager-render/src/util.rs:108-133`）。

活工作 chip 用灰字、无括号，格式 `Subagents 2 · Tasks 3`（`row_title.rs:1-2`）。空间不足时按 `Full`（`Subagents/Watchers/Workflows`）→ `Short`（`Sub/Subs/Watch/Flow`）→ `2 bg` → `bg` 逐级降级，并给标题留出至少 1 格（`row_title.rs:126-194`）。chip 与 age 之间用 ` · ` 连接（`render.rs:1687-1703`）。

### 1.6 分节、折叠与 Idle 折叠

- 分节标题只在 `Grouping::State` 下产生（`state.rs:270-279`）；形态是 `▸/▾ + 组名 + 计数 + ────` 长横线，折叠用 `▸`、展开用 `▾`（`render.rs:1271-1313`，字形在 `glyphs.rs` 的 `disclosure_closed/open`）。窄模式退化成 `Done 12` 无横线（`render.rs:1408+`）。
- 组名读作 `Awaiting / Working / Idle / Inactive / Done / Failed`（`state.rs:231-242`）。代码注释解释了为什么用 `Done` 而不是 `Completed`：短促、更像分组头。
- `Inactive` 分节**默认就是折叠的**：`DashboardState::default()` 用 `once(SectionKey::State(RowState::Inactive)).collect()` 初始化（`state.rs:1196-1198`）。
- **Idle 折叠**：默认只显示最新的 8 个 Idle 会话，外加"1 小时内活跃过的全部"，其余折成一行溢出行（`render.rs:897-925`）。常量：`MAX_VISIBLE_IDLE = 8`、`IDLE_FRESHNESS = 3600s`、`MIN_IDLE_FOLD = 2`（`render.rs:929-938`）。少于 2 行不折（"1 older" 行占的空间和它省下的那行一样）。溢出行的视觉是 `+ N more` / `- show fewer`（`render.rs:1385-1400`）。搜索或过滤生效时**暂停折叠**（`render.rs:855-861`）。
- 子代理的折叠独立：每个父行最多 8 个（`MAX_VISIBLE_SUBAGENTS = 8`），多的折成 `… N more` 且不可被光标选中（`row.rs:86-87`、`row.rs:320-358`）。

### 1.7 排序与分组

排序分两级：`sort_rows` 只做"组内重排"，分组本身由渲染时的分节头体现（`row.rs:826-838`）。

排序键（`ClusterKey`，`row.rs:907-960`）：

1. `pinned` 降序 —— 置顶行永远在最前；
2. 置顶组内：按手动 `reorder` 序 → `session_id` → 行 id；
3. 非置顶组内（State 分组）：先 `state.group_priority()` 降序，再手动序，再 `last_change_at` **降序**（越新越靠前），最后用行 id 兜底以保证确定性；
4. Directory 分组：非置顶行先按 `cwd_display` 字典序，再用同一个键（此时"状态优先"关掉，`state_before_reorder = false`）（`row.rs:871-896`）。

`group_priority` 取值（`state.rs:215-228`）：`NeedsInput 6`、`Working 5`、`Idle 3`、`Inactive 2`、`Completed 1`、`Failed 0` —— 注意没有 4，也即 Idle 与 Working 之间刻意留空。

"cluster"（簇）= 一个 top-level 行 + 紧跟其后的子代理行；排序以簇为单位，子代理**永不跨父行重排**（`row.rs:961-974`、`row.rs:95-96`）。

持久化：`pinned` / `reorder` / `grouping` / `enabled` 写在 `~/.grok/config.toml` 的 `[dashboard]` 段，键用 **session id**（`top:<session_id>` / `sub:<parent>:<child>`）而非进程内槽位，所以重启后依然有效（`state.rs:380-399`、`state.rs:4512-4515`、`docs/user-guide/23-dashboard.md:306-319`）。解析上限：256 条、键长 1024 字节（`state.rs:4517-4520`）。环境变量 `GROK_AGENT_DASHBOARD=0` 可整体关掉（`mod.rs:84-92`）。

### 1.8 过滤与搜索

过滤在 `apply_filter`（`row.rs:791-825`），语法解析在 `parse_filter`（`state.rs:4322-4352`）：

| 前缀 | 匹配对象 | 备注 |
| --- | --- | --- |
| `a:<text>` | `label` + `parent_label`（大小写不敏感子串） | 子代理可被父名命中 |
| `s:<state>` | 精确状态 | 同义词表：`busy/running`→Working、`done`→Completed、`dormant`→Inactive、`errored/cancelled`→Failed（`state.rs:4354-4377`）；未知 token 回退成子串匹配，让用户能看出状态路径没生效 |
| `#<text>` | 子串，且**连 `#` 一起匹配** | 防止误匹配到任意数字 |
| 其他 | `label` + `cwd_display` 子串 | 空串视为不过滤 |

过滤命中 `is_more_placeholder` 行时直接保留（`row.rs:802-805`）。`s:` 过滤时不发分节头（`render.rs:978-980`）。

## 2. Session picker（`/resume`）的交互

### 2.1 一套逻辑，三种宿主、两种渲染

`SessionPickerSurface` 是与宿主无关的状态容器（generation、`PickerState`、entries、loading、lanes、content_results、source_filter、pending_delete）（`src/views/session_picker_surface.rs:22-65`）。宿主枚举有三值：`Welcome`、`AgentModal`、`Dashboard`（`session_picker_surface.rs:7-17`）。渲染只有两种模式（`session_picker_surface.rs:67-108`）：

- `Fullscreen` → 交给 welcome 屏的 picker body；
- `Modal` → `ModalSizing { width_pct: 0.65, max_width: 120, min_width: 48, v_margin: 4, h_pad: 2, v_pad: 1, footer_lines: 2 }`，标题由宿主给（`session_picker_surface.rs:161-214`）。

Dashboard 里打开的 picker 标题是 `Open session`（常量 `DASHBOARD_PICKER_TITLE`，`session_picker_surface.rs:19-20`），只列 `kind == "build"` 且 `SourceFilter::Local` 的本地会话（`src/app/dispatch/dashboard.rs:316-346`）。它的输入路由是一份 89 行的薄壳：把事件交给 `handle_picker_input`，只把 `Selected` 映射成 `Action::DashboardPickSession`（`src/app/dashboard_session_picker.rs:13-88`）。

### 2.2 搜索

- 搜索栏标签是固定的 `" search: "`；搜索模式激活时标签用 `text_primary` 加粗，非激活但已有 query 时整条 query 画成 dim 且不画光标（`session_picker_surface.rs:110-148`）。
- 输入即在标题上过滤，同时触发**内容深搜**：查询长度 ≥ 2、且当前源支持 FTS 时才发 `Effect::DeepSearchSessions`，并带 `deep_search_seq` 代际号防止旧响应串台（`src/app/dispatch/session/load.rs:712-720`、`session_picker.rs` 的 `CONTENT_EXPAND_OFFSET = 100_000` 用于隔离内容命中与模糊命中的展开索引）。
- 深搜区标题是 `Extended search results (remote and local sessions)`，加载中是 `⸬ Searching session content…`（`session_picker.rs:861-881`）。
- `External` 源禁用内容搜索（外来会话没有 FTS 索引，`session_picker.rs:234-237`）。

### 2.3 预览：行内展开，而不是第二个面板

Session picker 没有独立的预览面板；它的"预览"是**行内展开详情字段**：

- 每行右侧固定显示相对时间（`format_time_ago`，右对齐 8 列）（`session_picker.rs:676`、`session_picker.rs:901-917`）。
- 展开后依次显示 `ID / CWD / Model / Created / Updated / Source / Host / Messages / Recap / Last turn / Turns+Tools / Prompt`，长文本按可用宽度截断（`session_picker.rs:680-722`）。
- 键位（通用 picker）：`y` 复制、`e` 或 `→` 展开、`E` 或 `←` 折叠、空格切换（`src/views/picker.rs:2933-2946`）；在结果里有命中时自动强制展开以便看到组内匹配（`picker.rs:1598`）。
- 行按 `repo_name` 分组，当前目录所属 repo 组被提到最前，其余按字母序；组头是不可选中的 `Header` 行（`session_picker.rs:41-50`、`session_picker.rs:736-790`）。

### 2.4 源筛选页签与徽章

`SourceFilter` 有 6 个值，轮换顺序固定为 `Grok → Headless → External → All → Local → Remote → Grok`（`session_picker.rs:196-237`）：

```
   ┌ 筛选页签（按 f 循环） ───────────────────────────────────────┐
   │  Grok │ Headless │ External │ All │ Local │ Remote           │
   └──────────────────────────────────────────────────────────────┘
   默认 Grok（= 原生且排除外来工具会话）；is_active() 以 != Grok 为准
```

- 徽章：`conversation` → `chat`，否则 `claude` / `codex` / `cursor`，未知源留空（`src/app/foreign_sessions.rs:286-293`）。
- Headless 页会隐藏外来工具会话，并在页脚提示 `N external session(s) hidden · f to show`（`session_picker.rs:882-900`）。
- 删除：`d` 武装、`y` 确认、其他键解除武装（`session_picker.rs:57-131`）；外来源的行不可删除（`session_picker.rs:76-102`）。
- 页脚快捷键由宿主拼装：全屏模式含 `Esc back / Enter select / ctrl+w worktree / ↑↓ navigate / f filter / d delete`；武装删除时整条页脚替换为 `y confirm delete / n cancel`（`src/views/welcome/mod.rs:2456-2515`）。
- Dashboard 宿主额外吞掉 `Ctrl+F` 一个事件（`dashboard_session_picker.rs:32-40`）；modal 模式的页脚是 `↑↓ nav / Enter select / / search / Esc close`（`session_picker_surface.rs:172-193`）。

## 3. 并发模型如何在界面上表达

### 3.1 "谁在跑"：状态不是持久标志，而是每帧重算

`classify_top_level(agent)`（`row.rs:465-479`）：

```
有待答权限 或 有待答提问        → NeedsInput
否则 有活跃父级活动（turn/唤醒/命令/排队 prompt/replay） → Working
否则 有后台工作（running bg task 或 非空 scheduled_tasks） → Working
否则                            → Idle
```

其中 `has_live_parent_activity` 是 `!session.state.is_idle() || wake_turn_active() || turn_activity().is_some() || !pending_prompts.is_empty() || loading_replay`（`row_activity.rs:10-16`）。因此**一轮 turn 结束但还有后台任务 / monitor / 定时 loop 时，行仍显示 Working**，并由 chip（`Tasks`/`Watchers`）与活动文案说明"还挂着什么"。

子代理只有三种可能：未完成 → Working、完成 → Completed/Failed（按 `status` 取 `failed|cancelled|error` 判失败），**永远不会进入 NeedsInput**（`row.rs:490-502`，注释明确"v1 子代理无法向用户提问，有测试固化"）。

### 3.2 "跑了多久"

age 列 = `format_time_ago(row.last_change_at.elapsed())`（`render.rs:1681`）。锚点选择是有讲究的：Working 行用 `turn_started_at` 优先（这样"跑了 12m"指本 turn），其余状态用 `last_active_at`；两者都缺时落到进程级 `fallback_epoch`（`OnceLock` 冻结一次，避免每帧重锚）（`row.rs:88-94`、`row.rs:607-621`）。roster / workspace 行的锚点是磁盘上的 unix ms，用 `system_time_from_unix_ms` 转成 wall clock，非正值当作"现在"（`util.rs:135-144`）。

peek 面板头部也带一个 time-ago 后缀，字段就是 `PeekPanelState::time_ago`（`peek.rs:69-70`）。

### 3.3 状态如何刷新

- Dashboard 打开时，事件循环按 `DASHBOARD_POLL_INTERVAL = 1s` 轮询，并**只在 `active_view == AgentDashboard` 时**生效；三条路径择一：workspace store 刷新（`workspace_sync::refresh`）→ leader roster（`Effect::FetchRoster`）→ 本地磁盘会话列表（`Effect::FetchDashboardSessions`）（`src/app/event_loop.rs:1659`、`src/app/event_loop.rs:2493-2509`）。
- 行本身不缓存：每帧 `build_rows_with_workspace` 重建并按每行重算排序键；模块注释解释这样做的理由是"一个 pager 进程里 agent 数量是个位数，重建是免费的"（`mod.rs:19-22`）。
- 局部动画按 `spinner_tick` 推进，并且**只有真正被绘制出来的动画才继续 tick**（`PaintedAnimations`）：spinner 每 4 tick 换帧、NeedsInput 闪烁每 10 tick（`animation.rs:5-7`、`animation.rs:25-64`）。头部 chip 的"working" spinner 也以"这一帧画过 chip"为准来标记（`chrome.rs:110-113`）。
- 本地 agent 的元数据（title / model / last_turn_summary / is_worktree / last_change）会持续同步进 workspace store，字段各自带上限（title 1024B、model 256B、summary 8192B），超长按 **char 边界截断而非报错**——"因为一次元数据同步不该因为标题太长而失败"（`src/app/workspace_sync.rs:177-205`、`crates/codegen/xai-grok-dashboard-store/src/types.rs:20-29`、`types.rs:306-341`）。

## 4. 会话标题：生成 → 优先级 → 截断

### 4.1 展示优先级（三处独立实现，语义一致）

Dashboard 行标题 `top_level_label`（`row.rs:537-567`）：

1. `display_name`（手动 `/rename` 后）
2. `generated_session_title`
3. `pending_prompts` 队首前 30 字符
4. scrollback 里第一条用户消息前 30 字符
5. `New session #<session_id 前 8 位>`，或没有 session id 时纯 `New session`

`session_title::entry_title`（`src/views/session_title.rs:22-33`）走同一思路但回退到 `session <id8>` / `loading...`，并集中做 trim + 60 字符截断；`named_title` 是"前三级，没有就返回 None"的版本，供**会话头部**这种"宁可不显示也不显示假标题"的场合使用（`session_title.rs:35-50`）。

### 4.2 谁生成

标题由 shell 侧推送进 ACP 通知，pager 只负责接收：`generated_session_title = Some(clean)`（`src/app/acp_handler/session_notification.rs:1004`、`1019`）、任务结果回填（`src/app/dispatch/task_result.rs:655`），`/rename --auto` 则把 `generated_session_title` 清空并发 `Effect::ResetSessionTitle`（`src/app/dispatch/session/modal.rs:149-151`）。生成时机与"早几轮重生成后冻结"的策略见用户手册 `docs/user-guide/17-sessions.md:44-46`（本仓库未在 pager 侧找到重生成策略的代码，属于 shell 侧行为，未找到证据）。

### 4.3 截断

三层截断，各自服务于不同层级的宽度预算：

| 层 | 手段 | 位置 |
| --- | --- | --- |
| 标题源本身 | 60 字符 + `...`（按 char 计数，不切多字节） | `session_title.rs:16-17`、`session_title.rs:151-159` |
| 行内标题 | `truncate_str(label, 可用宽度)`，宽度已扣掉 chip 与 age | `row_title.rs:65-70`、`render.rs:1691-1695` |
| `New session #id` 后缀 | 先画 `New session`，剩余宽度才画 ` #id`，且后缀用 dim 色 | `row_title.rs:42-64` |
| 窄模式 | `MIN_DASHBOARD_WIDTH = 40` 以下退化成单列，标签按宽度截断 | `layout.rs:8-10`、`render.rs:541-545`、`row_title.rs:90-101` |

标题文本在进入渲染前统一过 `sanitize_display_text`：C0/C1 控制符、bidi 与格式控制字符替换为 U+FFFD，防止后端/模型内容注入终端转义序列或伪造标题（`session_title.rs:161-180`，dashboard 侧包装成 `row::sanitize`，`row.rs:503-507`——其注释点明理由：dashboard 走 `set_string` 原样写进 ratatui buffer）。

## 5. 跨进程 / 跨机会话的界面表达

### 5.1 Leader roster（跨进程、可跨机）

- 传输：请求 `x.ai/sessions/list` → `RosterListResponse`，广播 `x.ai/sessions/changed` → `RosterChanged { upserted, removed }`；pager 侧是**只反序列化**的镜像类型（`src/app/roster.rs:1-78`）。解析器必须先剥 `{ "result": ... }` 信封，否则会"成功解析出空列表"，这个坑有专门的回归测试（`roster.rs:80-87`、`roster.rs:113-150`）。
- 字段：`session_id / title / cwd / is_worktree / session_kind / model_id / yolo / activity / last_turn_summary / resident / last_change_unix_ms / origin{kind,host}`；`session_kind` 只在"leader 为远端 claim 合成的行"上出现，这类行不可加载（`roster.rs:34-62`、`roster.rs:169-207`）。
- 界面映射：`RosterActivity → RowState`：`Working→Working`、`NeedsInput→NeedsInput`、`Idle|Dormant→Inactive`、`Completed→Completed`、`Dead→Failed`（`row.rs:362-371`）。
- **`Inactive` 是 roster 专属语义**：只由 roster 产生，本地 agent 永不落入；其定位是"别的 pager 进程拥有、本进程没加载"的后台噪音，所以放在 Idle 之下、Done 之上，且默认折叠（`state.rs:194-199`、`state.rs:217-228`、`state.rs:1196-1198`）。
- 显示条件：与本地 agent 同 session id 的 roster 行会被跳过（本地行信息更全）；既无标题又不活跃且未置顶的 roster 行**不显示**（`row.rs:373-407`）。roster 行没有 `context_pct`，`secondary_line` 取 `last_turn_summary` 或 `model_id`（`row.rs:438-462`）。
- 交互：点开 roster 行会去 `leader_roster` 或 `dashboard_local_sessions` 里找 cwd 并 `dispatch_load_session`，`origin.kind == "conversation"` 的行按会话入口加载（`src/app/dispatch/dashboard.rs:470-502`）。

### 5.2 Workspace store（跨进程共享的持久工作区）

`crates/codegen/xai-grok-dashboard-store` 是一个 SQLite 存储，作为 dashboard v2 的权威数据源（"membership、layout、以及本进程没加载的会话"都由它说了算，`row.rs:144`）：

- `Member`：`session_id / kind / origin / cwd / title / model / last_turn_summary / is_worktree / last_change_unix_ms / pin_rank / order_rank`（`types.rs:374-391`）。
- `MemberKind = Build | Conversation | Other`（`types.rs:169-176`）；`MemberOrigin = Local | Remote | Other`，origin **只在插入时写一次，之后任何 API 都不改**（`types.rs:207-214`、`types.rs:365-372`）。
- 上限与排序：`WORKSPACE_CAPACITY = 256`，超限时在同一事务里淘汰最旧的未置顶成员（`types.rs:11-13`、`types.rs:456-465`）；rank 用间隔 1024 的整数，插入取中点，`None` 表示未置顶 / 无手动位（`types.rs:15-17`、`types.rs:386-390`）。
- `WorkspaceSnapshot` 在同一个读事务里带上 `PRAGMA data_version`，注释说这样"轮询方不会漏掉落在快照与首次轮询之间的那次提交"（`types.rs:393-403`）。
- 界面上多出来的两种行：`DashboardRowId::Workspace`（store 里有、本进程没加载 → 标题取 title、退化到 cwd 名、再退化到 session id；`secondary_line` 取 summary 或 model；状态**恒为 Idle**）（`row.rs:219-286`）；以及 `provisional` 行（活 agent 但还没落库的成员）（`row.rs:131-202`、`workspace_sync.rs:124-158`）。

### 5.3 Foreign sessions（其它 CLI 的历史会话）

外来会话是**磁盘上别的工具的会话**，只在 session picker 里表达，不进 dashboard 行列表：

- 源：`claude / codex / cursor`，显示名 `Claude Code / Codex / Cursor`，各自由 `resume-claude|codex|cursor` 技能门控（`src/app/foreign_sessions.rs:204-260`）。
- 界面表达：行徽章（`claude`/`codex`/`cursor`），且不可删除、不参与"展开折叠"（`foreign_sessions.rs:286-293`、`session_picker.rs:678`）。
- 还有一条独立的"启动时探测可恢复的外部会话"链路：welcome 屏在满足一串前提（当前视图是 Welcome、没有认证返回流、home 目录未用或为空、非 chat 模式、非 ZDR 阻断、没有待定更新）时才发起探测，并用 `foreign_resume_launch_generation` 令牌 + cwd canonicalize 做竞态防护（`foreign_sessions.rs:13-120`）。

### 5.4 本机崩溃恢复

`crates/codegen/xai-grok-active-sessions` 维护 `~/.grok/active_sessions.json`（`{session_id, pid, cwd, opened_at}`），干净退出删条目、崩溃留下；下次启动 `collect_crashed()` 用 `kill(pid, 0)` 挑出死掉的 PID（`src/lib.rs:1-3`、`lib.rs:16-26`、`lib.rs:175-206`）。它提供的是"进程级的会话占用事实"，为 dashboard 的"这个会话在别的进程里活着"提供依据（本专题未逐一追踪它在 dashboard 中的消费点，未找到直接渲染证据）。

## 6. 与单会话 TUI 布局的切换机制

### 6.1 只有三个视图态

```rust
pub enum ActiveView { Welcome, Agent(AgentId), AgentDashboard }
```
（`src/app/app_view.rs:206-220`）

Dashboard 不是独立的渲染栈，而是同一个 `AgentView` 外加一层"从 dashboard 来"的铬标（overlay）。切换靠三个字段：

- `app.active_view`：当前画哪个；
- `app.dashboard: Option<DashboardState>`：dashboard 的全部状态（关闭视图不销毁状态）；
- `dashboard.attached_agent: Option<AgentId>`：**面包屑** —— 进入会话时设上，退出 dashboard 时清空（`src/app/app_view_tests.rs:6300` 的注释把语义写得很直白）。

```mermaid
不适用（文本示意）：
Dashboard ── Enter/点击行 ──▶ Agent(id)（attached_agent = Some(id)，头部出现 ‹ i/n › 与 [Dashboard]）
   ▲                                   │
   └──── Esc / Ctrl+\ / 点 [Dashboard] ─┘
```

### 6.2 进入与返回

- 进入：`dispatch_open_dashboard` 先做四道门（功能关闭 / 未登录 / 同意书未答 / 目录信任未答），再记录 `dashboard_return`（`Agent(id)` / `Welcome` / `Overlay(id)`），刷新 git 信息，最后按 `workspace_dashboard_enabled` 分派加载路径（`src/app/dispatch/dashboard.rs:160-226`）。已在 dashboard 时再按一次等于"退出"（`181-183`）。
- 返回：`dispatch_exit_dashboard` 消费 `dashboard_return`；目标 agent 已死则退到任意非 home tab，再不行退到 Welcome（`dispatch/dashboard.rs:250-288`）。从 overlay 返回时会**重新装回** `attached_agent` 与行光标，若仍有活跃子代理接管则恢复子代理行（`rearm_session_overlay`，`dispatch/dashboard.rs:289-315`）。
- 头部控件：`[Dashboard]` 按钮（`src/app/agent_view/render.rs:1360-1372`）与 `‹ i/n ›` 位置指示（同文件 `1341-1358`，`i/n` 由 `overlay_header.cycle_position()` 提供；只有 1 个会话时不显示）。
- `‹` / `›` 循环顺序 = **渲染器实际画出来的行序**（`overlay_cycle_order`，`mod.rs:57-79`），所以"上一个/下一个"与用户看到的顺序一致，而不是 agents map 的插入序；workspace 模式下改走 `workspace_rows` 的可见行（`dispatch/dashboard.rs:1008-1046`）。
- 细节视图（overlay）内的 `Ctrl+X` 是状态相关的：turn 运行中 = 取消 turn；空闲/命令在飞 = 武装 2 秒确认关闭会话（`CONFIRM_WINDOW = 2s`，`state.rs:174`）。

### 6.3 peek 与 dispatch 的互斥

列表下方那块位置只有一个 owner：选中行时它是 **peek 面板**（可回复、可答权限、可切模式），未选中时是 **dispatch 输入框**（永远是"新建会话"）；分配由 `layout::allocate_peek` 做 **list-first**：先保 `LIST_FLOOR_ROWS = 12` 行列表，剩余空间给 peek，且 peek 整体高度封顶 `⌊H × 3/8⌋`，内部 body 再封顶 28 行；压到 `PEEK_MIN_BOX_LIVE_TAIL = 8`（普通）或 `PEEK_MIN_BOX_QUESTION = 10`（有选项待答）以下就干脆不显示 peek（`layout.rs:12-26`、`layout.rs:36-150`、`layout.rs:259-274`）。peek 面板字段见 `peek.rs:64-105`：标题行是"最近一次响应类型 + 时间"，底部边框挂 model 与 `always-approve` / `auto` / `plan` 标志。

用户手册补充了两条行为约定：预览面板可在 `/settings → Appearance → Dashboard preview`（`[ui] dashboard_preview`）关闭以换取列表空间（`docs/user-guide/23-dashboard.md:95-101`、`src/settings/defs.rs:548-561`）；关闭预览后 `set_peek(None)` 并恢复 viewport（`src/views/dashboard/preview.rs:31-40`）。

## 7. 实现层面值得借鉴的几点

1. **布局与渲染分离**：`layout.rs` 只算 rect，`render.rs` 只画，`chrome.rs` 只管表头/动作行，`row.rs` 只管数据，`state.rs` 只管状态与输入。`layout_tests.rs` / `render_tests.rs` / `state_tests.rs` 分别锁住这三层，测试体量（约 1.2 万行）大于实现体量。
2. **纯函数化 + 每帧重建**：行、排序键、focusables（光标可达目标）都由同一份 `build_dashboard_lines` 派生，注释明确"导航与渲染永远不会对屏幕上有什么产生分歧"（`render.rs:948-976`）。
3. **窄终端是一等公民**：`MIN_DASHBOARD_WIDTH` 以下换整套窄渲染，宽窄两条路径共享同一份行数据。
4. **动画按"画过才动"计费**：避免在列表被折叠/过滤掉时还空转 tick。
5. **远端/磁盘数据的 UI 侧防护**：所有非本地文本先 sanitize；roster 的 `result` 信封陷阱有专门回归测试；快照带 `data_version` 以便轮询不漏提交。
6. **身份用稳定 key 而非位置**：选中/置顶/手动序用 `DashboardRowId`（`TopLevel(AgentId)` / `Subagent{parent,child}` / `Roster{..}` / `Workspace{..}`，`state.rs:27-44`），持久化再用 session id（`PersistedRowId`，`state.rs:123-133`），从而"重命名、重排、完成都不会让光标失效"（`state.rs:402-404`）。

## 关键结论（供合稿引用）

1. Dashboard 是"本进程活会话"的调度台，session picker 是"磁盘历史会话"的检索台，两者数据源、宿主、状态互不复用（`views/dashboard/mod.rs:1-4`、`views/session_picker_surface.rs:7-17`）。
2. Dashboard 一屏由 9 个具名 rect 组成，全部装饰行都是 0/1 高度且绑定终端高度阈值，peek 复用 dispatch 的 rect（`views/dashboard/layout.rs:167-257`、`layout.rs:353`）。
3. 每行固定 3 行高：`marker(1)+gap+indent+icon(1-2)+gap` 之后是标题，右端是 age 或 `[✗]`，中间按预算降级 live-work chip（`views/dashboard/render.rs:31,1558-1584,1681-1736`、`row_title.rs:150-194`）。
4. 行状态每帧从 agent 实时分类，没有"标记完成"命令：有权限/提问 → NeedsInput，有 turn 或后台工作 → Working，否则 Idle；子代理永不进入 NeedsInput（`views/dashboard/row.rs:465-502`、`row_activity.rs:10-16`）。
5. 排序 = 置顶优先 → 状态优先级（6/5/3/2/1/0，无 4）→ 手动序 → `last_change_at` 降序 → id 兜底；排序以"父行 + 其子代理"的簇为单位（`row.rs:826-974`、`state.rs:215-228`）。
6. Idle 组默认只留最新 8 个 + 1 小时内活跃者，其余折成 `+ N more`；少于 2 行不折；搜索/过滤时暂停折叠（`render.rs:897-938`、`render.rs:1385-1400`）。
7. Dashboard 每 1 秒轮询一次且仅在 dashboard 打开时进行，按 workspace store → leader roster → 本地列表三选一分派（`app/event_loop.rs:1659,2493-2509`）。
8. 跨进程会话靠 leader roster：`Working/NeedsInput/Idle|Dormant/Completed/Dead` 映射为界面状态，`Inactive` 是 roster 专属（"别的进程拥有、本进程未加载"），该分节默认折叠（`app/roster.rs:14-78`、`row.rs:362-371`、`state.rs:194-199,1196-1198`）。
9. 跨进程持久工作区由 SQLite store 承载（`Member` + `pin_rank`/`order_rank`、容量 256、rank 间隔 1024、快照带 `data_version`），store 里有而本进程没加载的会话会以恒为 Idle 的行出现（`xai-grok-dashboard-store/src/types.rs:11-17,374-403`、`views/dashboard/row.rs:219-286`）。
10. 会话标题优先级：`display_name` → `generated_session_title` → 首个 prompt 前 30 字 → `New session #<id8>`；来源标题按 60 字符截断，行内再按可用宽度截断，`#id` 后缀单独 dim 绘制（`views/dashboard/row.rs:537-567`、`views/session_title.rs:16-33,151-159`、`row_title.rs:42-70`）。
11. Session picker 的"预览"是行内展开（`e/→` 展开、`E/←` 折叠、`y` 复制），展开字段含 ID/CWD/Model/时间/Source/Host/Messages/Recap/Last turn/Turns+Tools/Prompt；源页签按 `Grok→Headless→External→All→Local→Remote` 循环，外来会话带 `claude/codex/cursor` 徽章且不可删除（`session_picker.rs:220-229,680-722,736-790,882-900`、`views/picker.rs:2933-2946`、`app/foreign_sessions.rs:286-293`）。
12. 单会话与 dashboard 不是两套布局：`ActiveView` 三态 + `dashboard_return` + `attached_agent` 面包屑，会话头部以 `[Dashboard]` 与 `‹ i/n ›` 提供返回与循环，循环顺序等于渲染出的行序（`app/app_view.rs:206-243`、`app/dispatch/dashboard.rs:160-315,1008-1073`、`app/agent_view/render.rs:1341-1372`）。
