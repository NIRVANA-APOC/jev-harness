### 4.5 状态面：status bar / status line / context bar

*本节事实来自上游专题 12/13/14/15，未引入其外的新内容；证据路径照抄上游，`src/…`、`views/…` 指 `crates/codegen/xai-grok-pager/` 下的同名路径。*

### 本节要点

- fullscreen 的 agent 视图共 5 个**屏幕状态面**：status bar（顶部，固定 1 行）、scrollback、turn_status（prompt 上方 1 行，按需出现）、status_line（prompt 之下 shortcuts 之上，0–5 行）、shortcuts bar（底部，固定 1 行）；另有 `/usage`、`/queue`、`/tasks` 三个提交进 scrollback 的纯文本块（`views/agent.rs:181-381`、`app/status_blocks.rs:1-14`）。
- status_line 是**唯一用户可配置**的状态面（`[ui.status_line]`）：只有 5 个键、6 个内置 item，代码里**没有模板引擎**，表达力只有「排序内置段」与「用户脚本 stdout」两条路（`xai-grok-status-line/src/config.rs:18-34`、`views/status_line/segments.rs:66-118`）。
- 空间不足时的取舍分四层（段内列上限 → 行内 elide → 行数回砍 → 整面 `Off`）；**先牺牲 status line 自己的行**，prompt 与 shortcuts 的 1 行永不被挤掉（`views/agent.rs:134-135, 229, 246, 281-298`）。
- 上下文用量有**三套互不通用的口径与阈值**（状态栏芯片 `8.5K / 1.0M`、status line `{pct}% ctx`、`/context` 的 100 格分类条）；成本有 `$0.005` 的显示地板（`views/context_bar.rs:16-41`、`views/status_line/segments.rs:11, 18`、`scrollback/blocks/context_info.rs:31-90`）。
- 运行态**不在**状态栏也不在 status line，而在独立的 `turn_status` 行：空闲且无 watcher 时整行消失；等待用户回答时 spinner 换成脉冲 `◆` 并隐藏阶段计时（`views/turn_status.rs:365-395, 725-735`）。
- 未找到证据：`credit_bar_line_for_session` 在当前快照没有生产调用点（不要把「状态栏有 Credits 芯片」当作现状）；手册称 repo-local `.grok/config.toml` 不能设 `[ui.status_line]`，但在 `xai-grok-config` / `xai-grok-shell/src/config` 里**没有找到**对应的过滤实现（`views/credit_bar.rs:231`、`render.rs:1387`）。

**纵向排布（fullscreen，`AgentViewLayout::compute`，`views/agent.rs:181-381`）**

```
① status_bar     固定 1 行：左 = branch / worktree 徽标 / sandbox profile + 标题 + cwd
                右（右对齐）= AgentStatusBar 芯片组，三段宽 " │ " 分隔
  [可选] tasks / catalog / todo 面板（各自前置一行 pane_gap）
  status_gap（top_vpad > 0 时为 1 行）
② scrollback     唯一 Constraint::Min(5)——全屏唯一会被压缩的行
  [可选] /btw、queue、③ turn_status(1 行)、banner、plugin CTA、follow-up、dock
  prompt_gap → [voice_recording 1 行] → prompt(N 行)
  shortcuts_gap（仅当 bottom_vpad > 0 且 status line 关闭时为 1 行）
④ status_line    0–5 行，默认 0 行；压栈前先夹 min(配置值, 总高 − 其余所有行)
⑤ shortcuts      固定 1 行
```

`status_line_height` 的夹取注释把理由写得很直白：**一个太长的脚本应该丢自己的行，而不是让 prompt 或 shortcuts 丢行**（`views/agent.rs:134-135, 289-290`）。

**状态面对照表**

| # | 名称 | 屏幕位置 | 内容项 | 可配置性 |
| --- | --- | --- | --- | --- |
| ① | status bar（芯片行） | 最顶行，固定 1 行 | `bg_tasks` / `plan` / `goal` / `mcp` / `workspace_mode` / `context` / `switcher` / `dashboard` / `link_url`，由运行时状态决定是否 `push`；左侧另画 branch/worktree/sandbox/cwd 与 Upgrade CTA | 用户不可配置 |
| ② | scrollback | 中部，最小 5 行 | 会话流；也承载 `/context` 的 `ContextInfoBlock` 与 `/usage` 的用量块 | 无 |
| ③ | turn_status | prompt 上方 1 行，按需 | spinner + 活动文案 + 阶段计时 + turn 计时 + token 数 + `[stop]`/`[↓]` | 无（`should_show` 决定） |
| ④ | status_line | prompt 之下、shortcuts 之上；0–5 行 | `builtin` 六种 item，或用户脚本 stdout（≤5 行） | **唯一可配置**：`[ui.status_line]` |
| ⑤ | shortcuts bar | 最底行，固定 1 行 | 当前 key owner 的快捷键提示，`compact` 模式截断 | 由 `key_owner()` 与 action registry 决定 |

（`views/agent.rs:181-381`、`views/agent_status.rs:89-130`、`app/agent_view/render.rs:1403-1475`。）

**status line 的两半契约**。`xai-grok-status-line` 是个 leaf crate：`config` 是用户写的、`context` 是 agent 发给客户端画的（`xai-grok-status-line/src/lib.rs:1-4`）。

- 配置键：`type`（`builtin`/`command`/`disabled`，默认 `disabled`，`off`/`none`/`hidden` 为别名）、`command`（`type = "command"` 时必填）、`items`（默认 `["cwd","model","context"]`，非法项逐项记录而非整表拒绝）、`padding`（上限 16）、`refresh_interval`（钳到 1–86400 秒，**只对 command 生效**）（`config.rs:145-187, 217-221, 288-308`）。
- 6 个内置 item 与取不到数据时的行为：`cwd`（列上限 40，无路径段则整段消失）、`model`（30）、`context`（`{pct}% ctx`，缺 `used_percentage` 则消失）、`cost`（`$X.XX`，**低于 `$0.005` 不画**，免得显示误导性的 `$0.00`）、`turn-timer`（`secs == 0` 不显示）、`session-name`（40，client 未填则消失）；段间分隔符固定 `SEGMENT_SEPARATOR = " │ "`，`builtin` 行永远只占 1 行（`views/status_line/segments.rs:9, 14-18, 72-117`、`views/status_line/mod.rs:158`）。
- payload 侧 schema 版本 `STATUS_LINE_SCHEMA_VERSION = 1`，两条硬规则：**取不到的值是 `None` 而不是 `0`**，字段一律 snake_case（改名会静默打断所有读它的脚本）（`xai-grok-status-line/src/context.rs:5-14`）。`session_name` 与 `trigger` 由**客户端**盖章；`cost.total_duration_ms` 从本进程挂载会话开始计时，不是会话创建时（`context.rs:25-28, 80-83, 141`）。
- 组装点是 `build_status_context`（模型/窗口来自 sampling config、token 来自 `try_get_estimated_total_tokens`、成本来自 usage ledger、git 信息在 blocking 池上跑 `git2::Repository::discover`），读失败就留 `None`——注释写明这会渲染成 `0% ctx`（`xai-grok-shell/src/session/acp_session_impl/status_line.rs:117-207`）。下发走瞬态通知 `XaiSessionUpdate::SessionStatus`，由 `run_status_emitter` 单循环驱动、`Notify` 把突发合并成一次构建（`xai-grok-shell/src/extensions/notification.rs:669`、`status_line.rs:275-336`）。
- 自定义命令的边界：客户端在 `initialize` 报能力位 `x.ai/statusLine`（缺省即关闭整条管线），会话级元数据 `clientStatusLine` 由 leader 注入 `session/new|load|resume`；`["ui","status_line"]` 整表列入 `PATCH_STRIP_PATHS`，远程/campaign/version 补丁不得注入，测试用 `command = "curl evil"` 钉住这条（`xai-grok-status-line/src/lib.rs:11-17`、`acp/mod.rs:458-460`、`xai-grok-config/src/config_override.rs:84-87, 313-325`）。
- 节流与生命周期常量（`app/status_line.rs`、`app/status_line/command.rs`）：`EVENT_DEBOUNCE = 300 ms`（防忙回合每帧跑脚本）、`MIN_REFRESH_INTERVAL_MS = 100 ms`、`ABANDON_AFTER = 30 s`（含「≥ 2× 命令超时」的 const 断言）、`REFRESH_FAILURES_TO_PAINT = 3`、`COMMAND_TIMEOUT = 10 s`、`MAX_COMMAND_OUTPUT_BYTES = 64 KiB`、`MAX_STATUS_LINE_LINES = 5`、`MAX_SANITIZED_CHARS = 1024`、从未绘制过时给脚本的 `RowSize::FALLBACK = 80×1`。interval 从**上一次运行结束**起算，避免慢脚本无间隙连跑；失败处理分两路——state 触发的失败立即画 `[status line: …]`，refresh 触发的失败在累计 3 次前保留上次输出（`app/status_line.rs:19-33, 366-400`、`views/status_line/mod.rs:206-208`）。

**空间不足 / 数据缺失时谁先被砍**。注意 `app/status_line_policy.rs` 管的是节流与运行生命周期，**不是取舍优先级**（`status_line_policy.rs:110-128, 280-349`）。真正的取舍是固定四层：

```
L1 段内列上限：cwd 40 / model 30 / session-name 40 → fit_columns 尾接 "…"
L2 行内 elide：从右往左吃，末尾补 "…"；link span 同步夹到被保留的列
L3 行数：脚本 5 行上限先被配置层砍（sanitize），再被 min(总高 − 其余所有行) 回砍
L4 整面 Off：ResolvedStatusLine 为 None，或 settled 且无内容 → 高度 0
```

- 缺数据的 item 整段消失而不显示占位值（`filter_map`，`views/status_line/segments.rs:72-117`）。
- 成功但**什么都没打印**的脚本会让整行消失，而不是回退到内置段；`Reserved` 状态在首个结果到达前占住 1 行，避免第一份输出把输入框往上顶；短终端从底部开始丢行（`status_line_policy.rs:293-294`、`views/status_line/mod.rs:116-118, 188`）。
- 状态栏芯片的坐标只算一次、永远右对齐，唯一的前插接口 `push_front` 必须先用 `room_for_front` 预量宽度；`link_url` 就是先扣掉 24 列位置地板、再要求 ≥ 12 列才画（`views/agent_status.rs:64-77`、`render.rs:1371-1381`）。

**上下文与成本的阈值**

| 口径 | 位置与形状 | 阈值与颜色 |
| --- | --- | --- |
| `used / total` 紧凑串 | status bar `context` 芯片，`8.5K / 1.0M`（`fmt_tokens`，宽度 ≤4 字符） | 连续色带，断点 0 / 50 / 65 / 75 / 85 / 95 %，段间线性插值，色序 text_primary → accent_user → warning → accent_error |
| hover 态 | 同一芯片换成进度条 + `fmt_pct5`（固定 5 字符，`≥100%` 显示 `MAX %`） | 同一色带 |
| `{pct}% ctx` | status line `context` item，整数百分比 | 二值：`pct ≥ CONTEXT_WARN_PCT(80)` 用 Warn，否则 Dim |

（`views/context_bar.rs:16-41, 53-100, 141-196`、`views/status_line/segments.rs:11, 87-99`。）芯片的**宽度不变量**是刻意设计：默认串右侧补齐到最小 6 列（`BAR_PCT_GAP 1 + PCT_WIDTH 5`），hover 切换不引起布局抖动；色带插值后要 `quantize`，测试钉住 0–49% 必须留在终端默认前景色（`views/context_bar.rs:133-141, 165-183, 205-227`）。

其余阈值：`MIN_DISPLAYED_COST_USD = 0.005`；credit 芯片 80% warning / 100% accent_error；余额告警 `LOW_BALANCE_CENTS = 1000`、`PAY_AS_YOU_GO_CRITICAL_CENTS = 500`（走 prompt 的 usage warning）；`/context` 只在 `(80, auto_compact_threshold)` 区间才提示「run /compact to free up context space」（`views/status_line/segments.rs:18`、`views/credit_bar.rs:141-255`、`scrollback/blocks/context_info.rs:437-446`）。

`/context` 块本身：条形永远是 **100 格**，两种形状共享同样的 100 格所以视觉份额不变——`WIDE` 5 行 × 20 格（约 39 列）、`NARROW` 10 行 × 10 格（约 19 列），切换点 `NARROW_BREAKPOINT = 50` 列；字形 ◆（system/messages）/ ◈（tools，只作参考行不进条）/ ◇（free），全部经 `glyphs` 走 CP437 降级，列宽由实际渲染行反推以保证数字对齐（`scrollback/blocks/context_info.rs:31-63, 92-119, 248-264`）。

**运行态行（turn_status）**。文案分派全在 `compute_activity` 的一个大 match（`views/turn_status.rs:580-698`）：`Cancelling…` / `Verifying…` / `Thinking…` / `Responding…` / `Compacting…` / 重试标签（warning）/ `Running…` / `Waiting…`，工具运行中则留空文案交给阶段标签。`Waiting(reason)` 有 7 种视图专用原因文案（`acp/tracker.rs:48-73, 116-136`）：`Waiting for response…`、`Waiting on subagent…`、`Waiting on task output…`、`Waiting on tasks…`、`Sleeping…`、`Running {event} hook…`、`Waiting for the agent to accept the prompt…`。出现条件是 `parked || !state.is_idle() || drain_blocked || session_starting_since.is_some() || watchers.total() > 0`。空闲的其余表达：`/tasks` 芯片在 `counts == default()` 时返回 `None` 整条消失，`goal` 芯片只在 `goal_state` 存在时 push（`views/agent_status.rs:146-148`、`render.rs:1283-1297`）。

**shortcuts bar**。内容由 `key_owner()` 三选一（`app/agent_view/render.rs:277-318`）：阻挡式表面（line/block viewer、权限卡、plan approval、question 卡等）原样渲染 `Surface(hints)`；普通 pane 走 `Pane(hints)` → `.compact(5, help_hint)`；`Hidden` 时不画。放不下的项整项 `break`（先试 5 列分隔符，再试键 / `:` / 标签），**不画省略号**；`compact` 下 `pinned` 的 hint 永远保留，`help_hint` 无条件追加（`views/shortcuts_bar.rs:261-292, 338-366`）。cheatsheet 默认绑 `Ctrl+.`，终端不可靠时主键换 `Ctrl+X`、另一个进 `alt_keys`（`actions/defaults.rs:805-827`）。pending confirmation 会顶掉所有 hint，只显示 `{key}:press again to {label}`（`views/shortcuts_bar.rs:229-248`）。

- 与 status line 的邻接：status line 在上、shortcuts 在下；status line 关闭而底部有 vpad 时补一个 `shortcuts_gap` 行。minimal 模式**没有** shortcuts bar，status line 照样画，且落在 prompt info 行之下（`views/agent.rs:291-298`、`xai-grok-pager-minimal/src/live.rs:4, 404-416, 714`）。
- 同时消失的情形：全屏子代理视图用 `a_subagent_owns_the_frame()` 直接 return；welcome 屏 `status_line_source_agent()` 返回 `None`；`active_modal.is_some()` 时直接画模态并 return（`app/status_line_policy.rs:31-33, 82-91`、`render.rs:3211-3215`）。

---

### 4.6 侧边与辅助面板

### 本节要点

- 所有辅助面板都**纵向插入**在「顶部状态栏 ↔ scrollback」或「scrollback ↔ prompt」之间，顺序固定；唯一占列宽的是 timeline rail（它顶替 scrollbar 的位置），因此面板开关只改变高度、不改变主内容区的水平布局（`src/views/agent.rs:228-381`）。
- 只有 **tasks 面板会自动弹出**，且只在「运行中任务 0→N」的边沿弹、只收起自己开的实例、排除 replay 恢复的历史任务；todo 面板**绝不自动弹出**（`src/views/tasks_pane.rs:903-927, 1045-1050`、`src/views/todo_pane.rs:218-223`）。
- todo 既**没有层级**也**不显示完成度**，条目只有 `□ ▶ ✓ ✗` 四态；「进行中」靠加粗 + 警示色，而不是动画或移动（`src/views/todo_pane.rs:25-91, 143-152, 295-309`）。
- 「不打断主流程」的核心手法是**状态只在非 transcript 表面就地更新**：运行计数只活在 idle 的 still-running 状态行上，完成才落一个 chip，transcript 从不再复述计数（`docs/user-guide/20-background-tasks.md`、`src/views/turn_status.rs:108-125`）。
- 子代理在父 scrollback 里**只有一个块**，靠 `scrollback_entry_id` 就地改状态与活动后缀；进入子代理输出是**边框画面替换父视图**而不是分屏，父视图的面板与 composer 全部归零（`src/app/subagent.rs:80-92`、`src/app/agent_view/subagent_takeover.rs:93-343`、`src/app/agent_view/render.rs:1014-1028`）。
- timeline 是**一 turn 一 tick 的导航轨道**（tick 位置表会话顺序，不是滚动比例），恒宽 2 列并顶替 scrollbar；未找到证据：catalog 面板在本快照中没有把 `visible` 置 `true` 的路径（打开入口不可达），子代理并发上限的默认值也未找到。

**各面板一览**（`src/views/agent.rs:228-381`、`src/views/todo_pane.rs:154-158`、`src/views/tasks_pane.rs:657-658`、`src/views/subagent_catalog_pane.rs:69-70`、`src/views/timeline.rs:17-23`、`src/views/goal_detail.rs:3`、`src/views/dock/mod.rs:2, 22`）

| 面板 | 高度策略 | 默认 | 开关 |
| --- | --- | --- | --- |
| tasks | `min(MAX_TASKS_HEIGHT=8, 15% 视口)`；视口 < 12 行返回 0；搜索/过滤条额外 +1 行 | 隐藏 | `Ctrl+G`；**运行中任务 0→N 时自动弹出** |
| todo | `min(MAX_TODO_HEIGHT=10, 15% 视口)`；可见但无条目时返回 1 行 | 隐藏 | `Ctrl+T`，不自动弹出 |
| catalog | `min(MAX_CATALOG_HEIGHT=8, 15% 视口)`；视口 < 12 行返回 0 | 隐藏 | 未找到打开入口 |
| timeline rail | 固定 2 列，顶替 scrollbar | 隐藏 | 设置项 `show_timeline` 或 `/timeline`（无默认快捷键） |
| goal detail | 居中圆角浮层；`width < 20 \|\| height < 6` 不绘制 | 隐藏 | 点状态栏 goal chip；`Esc` / `g` 关闭 |
| dock | 最多 `MAX_DOCK_ROWS = 8` 行 | 隐藏（远端 `dock_enabled` 才开） | `Ctrl+G`；开启时是 tasks 面板的替代品 |
| turn status | 固定 1 行，idle 且无后台工作时为 0 | 隐藏 | 自动 |

固定顺序为 `tasks → catalog → todo → scrollback → btw → queue → turn_status → banner → cta → follow_ups → dock → prompt`；scrollback 是唯一的 `Constraint::Min`（注释说明求解器把 `Min` 排在每个 `Length` 之前，所以「总需求超出屏幕时，被压缩的是别的行」）；面板间 `pane_gap` 仅在 `top_vpad > 0` 时为 1 行；`SHORT_TERMINAL_ROWS = 16` 以下强制砍掉 CTA、follow-ups 与底部 vpad；鼠标 `hit_test` 的优先级是 tasks > catalog > todo > queue（`src/views/agent.rs:61-94, 205-246`）。

**tasks 面板的自动开关（最值得抄的一条）**（`src/views/tasks_pane.rs:903-927, 1045-1050`）：

```rust
if running_count > 0 && self.prev_running_count == 0 {      // 只在 0→N 的边沿
    self.overlay.show();
    self.opened_by_auto = true;
}
if running_count == 0 && counts.paused_workflows == 0
    && self.overlay.visible && !self.overlay.focused
    && self.opened_by_auto && !self.show_done {              // 只关自己开的
    self.overlay.visible = false;
    self.opened_by_auto = false;
}
```

三个细节：不在 N→N+1 时重弹；用户自己 `Ctrl+G` 打开的面板不会被系统收走（`on_state_change` 把 `opened_by_auto` 清回 `false`，任何手动操作都「接管」面板）；**replay 恢复的历史任务不算数**，否则 resume 旧会话会立刻弹面板。四个侧栏共用 `OverlayState { visible, focused, fullscreen }` 三态机，聚焦画 `selection_border`、hover 画更淡的 `hover_border`、两者都不满足就完全不画（面板在非聚焦时保持无框外观）（`src/views/overlay.rs:47-131`、`src/views/agent.rs:574-625`）。

**todo / plan 的呈现**。`TodoItem` 只有 `content` / `priority` / `status` / `meta`，**pane 只渲染前两者**；条目顺序保持模型给的顺序（注释明确「no reordering by status」），条目 id 就是原始下标，因此选择能跨重建存活。数据入口是 ACP `SessionUpdate::Plan`，整批替换（`update_todos` 的注释直说「Does NOT auto-show the todo pane」）；更新时没有闪烁、没有 toast、**不插消息**，视觉反馈完全来自「下一次重绘时内容变了」加上进行中的粗体。面板内 `h` 切换已完成/已取消项（默认 `show_done = true`）；小视口限速：0–5 行时每次滚动 1 行、6–10 行走 2 行（`crates/codegen/xai-grok-tools/src/implementations/grok_build/todo/mod.rs:143-150`、`src/views/todo_pane.rs:190-199, 208, 218-223, 314-344, 368-386`、`src/app/acp_handler/mod.rs:244-252`）。plan mode 的产物**不进 todo 面板**：它写会话目录的 `plan.md`，由 `exit_plan_mode` 触发独立审批浮层（`a` 批准 / `s` 请求修改 / `c` 行内评论 / `y` 复制 / `q` 放弃），状态栏另有 `plan` chip 可重开预览（`docs/user-guide/19-plan-mode.md`、`src/app/agent_view/render.rs:1276-1282`）。

**tasks 面板：一个列表承载四类工作**。`Workflows`（运行中优先，再按开始时间倒序）/ `Subagents`（先按 agent type 字母序，再按开始时间倒序）/ `Tasks`（开始时间倒序）/ `Watchers`（monitor 恒在 loop 之前）；排序是四段比较器「类型分组 → 组内运行中优先 → 组内时间倒序 → `stable_id` 兜底」，最后一条是为了「等时间戳的行不会逐帧重排」。分组表头是可折叠行（`▾ Subagents 2` / `▸ Subagents 2`，chevron + 空格的宽度恰好等于条目的 2 空格缩进）；**组被清空后其折叠状态会被遗忘**，新任务到来时该组重新以展开态出现，避免藏在过期的折叠表头下。每行右侧叠绘 `[✗]`（kill）与 `[↗]`（view）两个 3 列按钮；因为按钮叠绘在 label 之上，绘制前先清格，若覆盖到文字就在左邻格补一个 `…`（样式取自左邻格颜色），让用户看到的是「被截断了」而不是「莫名少了几个字」。失败没有单独面板或红框，只体现为红色 `✗` 图标 + label 降饱和；workflow 的终态覆盖 `interrupted | complete | failed | cancelled`，非 active 且非 terminal 视为「暂停」（`src/views/tasks_pane.rs:1-5, 108-131, 333-347, 535-552, 580-597, 698-728, 843-897, 960-987, 1344-1535`、`src/views/workflows.rs:60-65`）。

任务计数常驻顶部状态栏：`◆ N` = 运行中的 bg task + 非 workflow subagent + scheduled + active workflow 之和，`P N` = 暂停的 workflow 数，两者都为 0 时整条 chip 不渲染；`◆` 用**静态**实心菱形而不是 spinner，注释理由是「表头全程在视野内，这里的 spinner 读起来是噪声而不是进度」。同一批 chip 也是鼠标目标（点 `◆ N` 切 tasks 面板，点 goal chip 切 goal/workflow 浮层）（`src/views/agent_status.rs:141-182`、`src/views/tasks_pane.rs:1007-1031`、`src/app/mouse.rs:53-79`）。

**子代理**。父 scrollback 里只有**一个块**：阻塞式（sync）一个 `Started` 块，运行时闪烁、结束变绿/变红；后台式（async）的 `Started` 块永久保留（转灰），结束时**另加**一个 `Completed` / `Failed` 块。运行中把子代理的实时活动（"Thinking"、"Running: cargo test"、"Compacting"）作为后缀就地更新，所以不打开子代理视图也能看到交互式进度。嵌套深度上限为 1（子代理再 spawn 会因 depth-limit 失败），所以列表里**不需要树形缩进**，靠 `type_label`（persona > role > subagent_type > tag > `general`）排序分组。进入输出有三条路径（父块上 `Enter`/`Ctrl+F`、tasks 面板的 `[↗]`/双击/Enter、dashboard 会话行），最终都走 `open_subagent_fullscreen(child_sid)`；若子视图 scrollback 还空着会先 replay 子会话的 `updates.jsonl`。全屏视图是**替换父视图的边框画面**，标题行放全部状态（状态图标 → 类型标签 → 加粗描述 → model → 上下文徽标 → 活动 `· 耗时` → `[✗]`），父视图的 tasks/catalog/todo 高度强制为 0、composer 也是 0 行（纯观察态，不能输入、不能排队）。返回靠 `q` 或 `Esc`（仅当子视图处于「裸 scrollback」；搜索打开时第一下先关搜索）、点 `[✗]`；`Ctrl+Q` **永不被消费**，始终冒泡到全局退出，注释把这条列为不变量（`src/app/subagent.rs:80-92, 961-996`、`src/scrollback/blocks/subagent.rs:1-11, 63-66`、`src/app/agent_view/subagent_takeover.rs:7, 33-80, 93-343, 358-409`、`docs/user-guide/16-subagents.md`）。

**timeline rail**。它**不是**「滚动位置的第二种画法」而是**会话的 turn 索引**（`Turn { prompt_index, end_index, status }`），模块头写明「tick 位置表会话顺序，不是滚动比例」，所以 `TurnStatus` 在 rail 上不参与渲染。出现门槛：终端宽 `< MIN_TERMINAL_WIDTH(60)`、turn 数 `< MIN_TURNS(2)`、或当前不是 `ViewSurface::Root` 时宽度为 0（理由：窄终端上「transcript 比导航器更需要那些列」，「1 个 turn 的时间线是噪声」）。几何每帧算一次成 `TimelineRail`，渲染与鼠标命中**共用同一份数据**；rail 需要 scrollbar 的 gutter 几何（`scrollbar_cfg.enabled == false` 时 rail 宽度强制为 0），活动 tick 是「拥有视口顶部的那个 turn」，chevron 的「变暗」与点击 no-op 由同一个 `chevron_target` 决定，所以**变暗的 chevron 一定是 no-op**。它**不画轨道底色**（深色轨道条在 tick 少时读起来像一条尴尬的空带）。hover 预览卡悬浮在 rail 左侧、最多 2 行、宽度钳在 16..32 列，内容是该 turn 首个非空行（上限 120 字符）。它与 `/jump` 共用 `timeline_entries()` 返回的稳定 `prompt_entry_id`（不是下标），所以条目的插入/删除不会让导航指向别的块（`src/views/timeline.rs:1-3, 4, 17-23, 26-46, 61-76, 109-161, 185-213, 228-242, 248-331`、`src/scrollback/state/types.rs:23-31`、`src/scrollback/state/timeline.rs:5-59, 85-104`、`src/views/agent.rs:382-393`、`src/settings/defs.rs:533-548`）。

**「不打断主流程」的六条手法**（`docs/user-guide/20-background-tasks.md`、`src/views/turn_status.rs:108-125`、`src/views/tasks_pane.rs:786-958, 1074-1085`、`src/views/todo_pane.rs:368-386`、`src/app/subagent.rs:80-92`、`src/views/agent.rs:95-103, 208-219`）

1. **状态只在非 transcript 表面上更新**：运行中的计数只活在 idle 时的 still-running 状态行（`◎ 1 command · 2 monitors · 1 loop · 1 subagent still running`）；完成事件才在 transcript 里落一个 `Task completed` chip；transcript 从不复述或重述这些计数。
2. **单行高度在 0/1 之间切换**，不占位也不弹层：turn status 靠 layout 里那一行出现/消失来「通知」，列表高度也用同一套路（隐藏时 `desired_height` 返回 0）。
3. **就地更新而非追加**：子代理运行时只有 1 个块，靠 `scrollback_entry_id` 就地改颜色与活动后缀；目标状态 chip（`[Goal: Executing] 12.3k/50k tokens 3m`）是状态栏 item，每帧重建，从不进 transcript。
4. **每帧从单一数据源全量重建视图模型，不做增量 patch**：代价由「条目数 < 20 时很便宜」承担，换来的是**不存在视图与数据不同步的窗口**；只保留 `highlight_cache` 与「按 stable_id 保持选择」两处必要缓存。
5. **动画靠单一 tick 驱动，只在有活干时才走**：`TasksPane::tick` 自增并返回「是否有运行中的条目」，子代理全屏视图的标题 spinner 直接复用 `tasks.tick_count()` 而不另起时钟；速度常量分场景（turn spinner `SPINNER_DIVISOR = 4` 约 7.5fps；idle 的 still-running 呼吸 `MONITOR_PULSE_DIVISOR = 8` 约为前者一半，理由是「idle 的提示应当平静地呼吸」）。
6. **屏幕太小时先砍辅助行**：`SHORT_TERMINAL_ROWS = 16` 以下砍 CTA 与 follow-ups，`AUTO_COMPACT_MAX_ROWS = 20` 以下强制 compact，视口 < 12 行时 tasks / catalog 直接返回高度 0。

---

### 4.7 首屏、空状态与渐进式披露

### 本节要点

- 首屏不是一个页面，而是**同一个欢迎页上的 6 个短路分支**：`render_welcome` 用一个 `match params.auth_state` 顺序判断，**臂的顺序就是优先级**；`auth_state` / `trust_state` / `consent_state` 三者全部 `Done` 才允许创建会话（`crates/codegen/xai-grok-pager/src/views/welcome/mod.rs:758-903`、`src/app/app_view.rs:1252`）。
- **可读性是硬门禁而非尽力而为**：同意声明正文限 12 行 / 2000 字节 / 78 列，装不下就撤掉 Accept 只留 Quit，并提示放大窗口（`src/views/welcome/consent.rs:21-30, 65-80`、`src/app/consent.rs:16-20`）。
- 欢迎页有两套版面：内容宽 ≥ 90 列且高度够时用带边框 hero box（logo 左、版本/信息槽/菜单右），否则退化为 stacked；放不下时**降级而不是截断**（`src/views/welcome/hero_box.rs:14, 50-61`、`src/views/welcome/mod.rs:356-495`）。
- 空状态统一走「一行灰字 + 通用选择器」：无匹配是 `No matches`，dashboard 无会话是 `No sessions yet. Esc to dispatch one.`，选择器里是 toast `No sessions found for this directory`；唯一的品牌图案是欢迎页的盲文 logo，**没有**任何空状态插画或大图（`src/views/picker.rs:1922-1944`、`src/views/dashboard/render.rs:534-537`、`src/app/dispatch/session/foreign.rs:184-195`）。
- tips 是**两套互不相干的系统**：欢迎页 Tip of the Day（持久游标，每次启动一条）与会话内 ephemeral hint（单槽 + TTL + 去重键 + 四层防骚扰）（`crates/codegen/xai-grok-shell-base/src/util/tips.rs:41-52`、`src/tips/ephemeral.rs:10-33`）。
- 渐进式披露在代码里有三种手法：**变暗而非隐藏**、**置位才出现**、**子项收进子页**；教程则反过来走「绝不自动弹出 + 编译期嵌入 markdown + 强约束测试」（`src/views/shortcuts_help.rs:63-72, 127-130`、`src/views/welcome/mod.rs:1763-1789`、`src/settings/defs.rs:456-467, 1259-1301`）。

**首次启动的完整序列**

```
grok 启动
   │
AuthState::Pending ────────────► ① 登录菜单屏     Login with grok.com / Quit
   │ 已认证 / 免登录
AuthState::Authenticating ─────► ② 认证等待屏     浏览器授权 / 设备码 / 粘贴 token
   │
is_zdr_blocked ────────────────► ③ ZDR 阻断屏     Switch account / Quit
   │ 否
has_access == false ───────────► ④ 订阅闸门屏     Tier + [Refresh] + 文案 + 链接
   │ 是
ConsentState::Pending ─────────► ⑤ 同意声明屏     a Accept / q Quit
   │ 无待同意
TrustState::Pending ───────────► ⑥ 目录信任屏     y Yes, proceed / n No, quit
   │ 已信任
⑦ 正常欢迎页（hero box 或 stacked）
```

- 门禁顺序在注释里被显式固定：同意是账号级，所以先于工作区级的信任问题解析（`views/welcome/mod.rs:866`）。信任与同意都在**首帧渲染前** seed（`src/app/event_loop.rs:336-377`），注释写明「必须早于第一次渲染，否则启动意图块会在门禁后面就把会话开起来」。
- **输入捕获的守卫**：门禁未清空时，启动时敲入的 prompt 会被丢弃而不是重放，否则一个以 `n` 开头的 prompt 可能替用户回答信任问题并退出（`src/app/app_view.rs:1257-1266`）。
- ①② 的视觉：登录屏走「Logo. {prompt}. Version」三段式（上次认证失败时 `error` 以 `accent_error` 画在同一行槽位），且**仍绘制 home 草稿的输入框**；认证等待屏四个子臂共享「logo + 居中块 + 退出提示」结构，设备码是从验证 URL 里解析出 `user_code` 再上屏（反钓鱼），原始 URL 模式关闭鼠标捕获以便用终端原生选择复制整条 URL；退出提示按终端品牌切换（VS Code 系显示 `ctrl+d`，其余 `ctrl+q`）。未找到取消/中断认证的按键证据（`views/welcome/mod.rs:49-70, 793-800, 955-966, 1079-1128, 1203, 1303, 2552`）。
- ⑤ 同意屏：正文按 `app::consent::wrap` 排版，校验失败有结构化枚举 `ConsentArmRefusal`（正文过高、未配对标记、纯文本里的 URL、版本号不合理……），失败一律 fail-open；布局会**优先丢掉 logo 而不是判定不可读**（`views/welcome/consent.rs:27-30, 110-115`、`src/app/consent.rs:47-70`）。
- ⑥ 信任屏的触发条件是「特性开启 + 该目录未信任 + 目录里有仓库级可执行配置」；警告文案被拆成两行，注释说明是为了在窄/紧凑宽度下不截断（`src/app/event_loop.rs:344-355`、`views/welcome/mod.rs:1004-1026`）。
- 启动警告（终端诊断、认证、配置迁移等任何子系统）共用**单槽** banner，画在错误槽位；选取规则是「第一个 Warning 级，否则最后一条」，避免旧的 Info 挡住新的 Warning（`src/startup.rs:66-73`、`views/welcome/mod.rs:2601`）。

**欢迎页版面与槽位争夺**。三种版面：全屏会话选择器（`welcome_compact = true`，logo/菜单隐藏）、hero box（非紧凑 + 内容宽 ≥ `HERO_BOX_MIN_WIDTH = 90` + 菜单非空）、stacked；hero box 放不下时整体回退到 stacked。盒内边距 `V_PAD = 1`、`H_INSET = 2`、`LOGO_H_PAD = 3`；副标题只在信息槽为空时占一行，避免盒子变高；菜单行按 `label … shortcut` 排版（标签左对齐、快捷键右对齐、中间至少 4 列）。信息槽里的公告优先于更新日志。行优先级（注释写死在代码里）：**待应用更新提示 > 隐私横幅 > 外部会话恢复提示 > 常规 Tip of the Day**；选择器可见时 tip 高度直接为 0。菜单行也不是静态的：`Import Claude settings` 只在检测到待导入的 `.claude/` 配置时插到最前；`Changelog` 行在 `has_access` 时**从第一帧就在**，免得等 CDN 拉取时菜单跳动（`views/welcome/hero_box.rs:17-38`、`views/welcome/mod.rs:327-340, 1707-1756, 1763-1789, 1802-1806, 1981-2010`、`views/welcome/menu.rs:37-47`、`views/welcome/logo.rs:1-3, 16-18, 36-42`）。

盲文 logo 是唯一的手绘图案（`assets/logo/logo07.txt` / `logo05.txt`），按终端高度分三档：≥ 26 显示全高图案，≥ 22 显示小图案，低于 22 不画；旧版 Windows 控制台字体不含 U+2800 盲文块会渲染成 tofu，因此在那些终端上**整个 logo 隐藏**（`views/welcome/logo.rs:1-3, 12-18, 36-42, 65-67`）。

**空状态与占位符清单**

| 场景 | 显示 |
| --- | --- |
| 无会话（resume 选择器，原生通道） | toast `No sessions found for this directory`（部分加载失败时改用 `picker_notice`） |
| 无会话（dashboard 面板） | ` No sessions yet. Esc to dispatch one. ` |
| 选择器无匹配（文件搜索 / 历史搜索 / 设置筛选的通用路径） | `  No matches`，`gray_dim`，画在内容区首行 |
| 选择器加载中 | 居中点阵 spinner + `Loading…`（帧按 tick/4 轮转） |
| 空状态与 spinner 的竞态 | 「空状态必须等 native 与 foreign 两条加载通道都停下来」，且渲染 / 重绘 / tick 三处共用同一判据 |
| 设置搜索无结果 | `No matches for <query>` |
| 无消息（欢迎页输入框） | 占位符 `Type a message...` |
| 无消息（会话内输入框） | 主输入框聚焦时隐藏占位符 |
| 无公告 | 会话 banner 高度为 0（槽位不占行） |

（`src/app/dispatch/session/foreign.rs:184-195`、`src/views/dashboard/render.rs:534-537`、`src/views/picker.rs:1922-1944`、`src/views/session_picker.rs:152-172`、`src/views/settings_modal/render.rs:462-465`、`src/views/welcome/prompt.rs:31`、`src/app/agent_view/mod.rs:370-381`、`src/views/prompt_widget/mod.rs:169-173`、`src/views/announcements.rs:321-323`。）

**tips A 类：欢迎页 Tip of the Day**。内容是四层配置合并（requirements > remote settings > 用户 config > managed config）；任一层设 `exclude_default = true` 会丢掉 remote 层，`[cli] show_tips = false` 直接关掉全部。每条 tip **每次启动只显示一条**，由持久化游标顺序推进：读 `~/.grok/tip_cursor.json` 的 `cursor`，取 `tips[cursor % len]`，写回 `cursor + 1`，全部轮完再循环。骚扰控制很直接：用户在欢迎页**只要一开始打字**，本行的 tip 就永久隐藏到本次进程结束（`crates/codegen/xai-grok-shell-base/src/util/tips.rs:11-52`、`crates/codegen/xai-grok-shell/src/util/config/tips.rs:29-77`、`src/app/event_loop.rs:1424-1433`、`src/app/app_view.rs:4471-4478`）。

**tips B 类：会话内 ephemeral hint**。核心原语是**单槽 + TTL + 去重键**的提示行，画在输入框上方的 banner 行；`DEFAULT_TIP_TICKS = 90`（≈3s @30fps；到期判定在减 1 之前，所以 N 值实际存活 N+1 tick）。同 key 再 show 只刷新 TTL、不叠加：

| 提示 | 触发条件 | 上限/会话 | TTL（tick） | ambient |
| --- | --- | --- | --- | --- |
| `Input cleared · ctrl+z to undo` | 草稿峰值 ≥ 20 字符后被清空到 ≤ 5 字符 | 3 | 90 | 否 |
| `Planning? Check out plan mode via shift+tab` | prompt 里出现整词命中的规划关键词，且非 plan 模式、会话空闲、输入模式 Normal | 3 | 90 | 否 |
| ctrl+v 可粘贴剪贴板图片 | 终端聚焦 + 当前模型接受图片 + 未附图片；机会式轮询 1s 节流 + 30s 冷却 + changeCount 去重 | 冷却+去重 | 90 | 否 |
| `Queued · Enter to send now` | 回合进行中排队跟进后 | 3 | 90 | 否 |
| `Tight on space? Try /compact-mode` | 终端行数落在 `(AUTO_COMPACT_MAX_ROWS=20, 28]` 且未开 compact mode | 1 | 90 | 是 |
| `grok wrap ssh` 引导 / `/doctor` | 首个稳定 agent 绘制时由 `diagnostics::ssh_wrap_hint` 判定环境 | 1 | 300 | 是 |
| 双击选词可改设置项 | text selection 仍为 fold/nav 时在 scrollback 双击 | 3 | 600 | 是 |
| 拖拽复制后提示导出/复制 | 15s 窗口内 ≥ 3 次拖拽复制，先 armed 再于 toast 结束后落屏 | 3 | 600 | 否 |

（`src/tips/*.rs`，常量位置：`clear_detector.rs:18`、`plan_nudge.rs:18`、`send_now.rs:11`、`small_screen.rs:20`、`ssh_wrap.rs:21, 26`、`word_select.rs:16, 21`、`export_copy.rs:14, 16`。）

**防骚扰的四层机制**（`src/app/agent_view/notices.rs:28-37, 73-137`、`src/tips/ephemeral.rs:10, 27-33, 77-101`、`crates/codegen/xai-grok-shared/src/ui_config.rs:184-230`）：

1. **展示门禁**：`show_ephemeral_tip` 先过 `ephemeral_tip_renderable`，不可绘制就直接返回 `false`——不计 seen、不烧 TTL、不发遥测。不可绘制的条件有十余种：权限询问队列非空、question view、任何 active modal、隐私横幅激活、子代理全屏接管、各类 viewer、gboom、goal detail、workflows、prompt 下拉打开、会话 announcement banner 占用、终端尺寸未知或过矮（16 行及以下不显示，17 行起显示）。
2. **TTL 只在可见时燃烧**：会话 banner 或隐私横幅占用槽位时停表；ambient 提示在被遮挡时**暂停而非扣减**。
3. **会话级配额**：`session_seen = Some((key, cap))`，计数在内存、每次运行重置。
4. **用户级开关**：8 个 `[ui.contextual_hints]` 布尔项（`undo` / `plan_mode` / `image_input` / `send_now` / `small_screen` / `word_select` / `export_copy` / `ssh_wrap`），`None` 继承（默认开），`Some(bool)` 是用户显式选择并压过 remote 默认；每个 dispatch 入口都先查开关。

遥测上每次落屏记 `tip.shown`，离开槽位记 `tip.dismissed` 并带原因 `replaced` / `expired` / `cleared`；提示文案里的按键**从真实绑定派生**（如 `ctrl+z` 由 `is_undo_key` 认可的 KeyShortcut 生成、`shift+tab` 取 `shift_tab_keys()[0]`），避免文档与实现漂移（`src/tips/ephemeral.rs:180-197`、`src/tips/clear_detector.rs:29-35`、`src/tips/plan_nudge.rs:37-43`）。

**announcements（公告）与隐私横幅**。只有 `critical` 与 `promo` 两个严重级会进入会话槽位（`info` / `warning` 只上欢迎页），会话槽里 **critical 永远压过 promo** 且每帧重新推导；critical 也压过隐私横幅，promo 不压（「故障通知不能被长期 nag 挡掉」）。高度：critical 2 行、promo 1 行、无公告 0 行；promo 在会话内只画 1 行按钮/隐藏入口、**不画 message 正文**，欢迎页 hero 才有空间画正文并支持点开内联展开。去重键优先用 `id`，否则用 `content:<title>\u{1f}<message>` 合成；持久化形状 `{"hidden_ids": [...]}`（`BTreeSet`，确定性顺序），旧版 `{"hidden": bool}` 会退化成空集并重新显示一次；过期键只在真实更新路径清理（注释明确禁止逐帧清理）。`dismissible: false` 抑制两种公告上的所有隐藏入口并让文本收回预留列；逃生口是 `/announcements show`，其可用性判据**故意忽略隐藏集合**，因此全隐藏时仍能自救（`src/views/announcements.rs:1-23, 109-159, 203-248, 290-333, 383-410`、`crates/codegen/xai-grok-announcements/src/lib.rs:78-125`）。

隐私横幅有 6 重门禁（非 minimal 屏、命中 rollout 分群、非 ZDR 且非 team 非管理员、没有正在进行的写入、当前 opt-out、认证/权限/信任全部就绪）；已确认后按天数决定重提醒（`None` / `0` = 永不再现，解析失败按「该重现」处理）。版面是标题 `Help improve Grok` + 右侧 `[Opt out]` / `[Opt in]` 共享一行 + 要点正文 + 法务行。三个法务行变体**从宽到窄挑第一个能整行放下的，绝不裁剪**；按钮**要么整块画要么不画**——防止「空白处的幽灵点击区」把用户静默 opt-in。正文最多 4 行、超出用 `…` 省略；若按钮列会让正文超过 3 行就放弃按钮列改用全宽（`src/app/app_view.rs:1140-1152, 1219-1248`、`src/views/privacy_banner.rs:12-58, 88-120`）。

**渐进式披露的三种手法**

| 手法 | 例子 |
| --- | --- |
| **变暗而非隐藏** | 快捷键手册里**所有** action 都在，只是当前上下文不生效的绑定变暗；分类按新手友好顺序 |
| **置位才出现** | `Import Claude settings` 行、工作区模式分段控件（仅 chat 模式且 `has_access`）、`Changelog` 行；更新日志区块在终端太矮/紧凑/被 tip 抢空间时整块消失；hero 副标题在信息槽有内容时让位 |
| **子项收进子页** | 设置页顶层的 8 个 contextual hints 只显示一个 `Show contextual hints` 组行，子项藏在子页里靠关键词索引串起来；设置项另带 `hidden_in_minimal` 标记控制 minimal 模式的可见性 |

（`src/views/shortcuts_help.rs:63-72, 127-130`、`src/views/welcome/mod.rs:327-340, 1752, 1763-1789, 1802-1806`、`src/settings/defs.rs:456-467, 489, 1257-1301`。）另外，`prompt_suggestion`（下一句 prompt 幽灵补全）的可见性刻意做成**每帧从当前输入推导**而不是按键驱动状态机，避免状态漂移；接受键是 Tab / 右方向键，仅当光标在文末时出现（`src/views/prompt_suggestion.rs:1-13, 78-104`）。

**引导与文档的联动**。教程内容是**编译期嵌入的 markdown**（`include_str!` 打进二进制，9 个主题，每项 `title`/`blurb`/`content`/`go_deeper`）；主题顺序被显式设计成一条线性流程（从别的工具迁移 → 第一条 prompt → 附加上下文 → 认识界面 → 更大的功能），overlay 里 `→` 按此推进，列表显示 `N/9 explored`。测试强制三条：每页 ≤ 50 行、`go_deeper` 的标题必须能解析到真实指南、标题唯一。`/tutorial` 是唯一入口（别名 `tour`、`onboarding`），命令注释写明「**绝不自动弹出**」；更深的文档走另一套嵌入表（`USER_GUIDE` / `REFERENCE_DOCS`，`/docs` 打开 DocPicker）。官方用户指南也把首启写进文档：首次启动会打开浏览器完成 grok.com 认证，凭证存 `~/.grok/auth.json` 并自动刷新（`src/tutorial_docs.rs:19-28, 30-31, 108-142`、`src/views/tutorial.rs:33-40, 305-311, 320-355, 387`、`src/slash/commands/tutorial.rs:1-23`、`src/docs.rs:1-8, 40-60`、`src/slash/commands/docs.rs:1-10`、`docs/tutorial/09-where-next.md`、`docs/user-guide/01-getting-started.md:74`）。

---

### 4.8 多会话界面与 dashboard

### 本节要点

- 同时存在**三个「列出多个会话」的界面**，职责互不重叠：Agent Dashboard（本进程活会话）、Session picker（磁盘历史会话）、Agents modal（agent 定义/配置）（`crates/codegen/xai-grok-pager/src/views/dashboard/mod.rs:1-4`、`docs/user-guide/23-dashboard.md:9-11`）。
- Dashboard 一屏由 9 个具名 `Rect` 组成，**每个装饰行都是 0 或 1 行高并各自绑定一个终端高度阈值**；列表下方那块位置只有一个 owner——选中行时是 peek 面板，未选中时是 dispatch 输入框（`src/views/dashboard/layout.rs:167-257, 353`）。
- 行状态**每帧从 agent 实时分类，没有「标记完成」的命令**；排序 = 置顶优先 → 状态优先级（`NeedsInput 6 / Working 5 / Idle 3 / Inactive 2 / Completed 1 / Failed 0`，**没有 4**）→ 手动序 → `last_change_at` 降序 → id 兜底，且以「父行 + 其子代理」的簇为单位（`src/views/dashboard/row.rs:465-502, 826-974`、`src/views/dashboard/state.rs:215-228`）。
- Idle 组默认只留最新 8 个 + 1 小时内活跃者（`MAX_VISIBLE_IDLE = 8`、`IDLE_FRESHNESS = 3600s`、少于 `MIN_IDLE_FOLD = 2` 行不折）；每个父行最多 8 个子代理；搜索或过滤生效时**暂停折叠**（`src/views/dashboard/render.rs:897-938, 1385-1400`、`row.rs:86-87, 320-358`）。
- 跨进程会话靠 **leader roster**（`Working/NeedsInput/Idle|Dormant/Completed/Dead` 映射为界面状态，`Inactive` 是 roster 专属语义、默认折叠）与 **SQLite workspace store**（容量 256，`origin` 只在插入时写一次）表达（`src/app/roster.rs:14-78`、`row.rs:362-371`、`state.rs:194-199, 1196-1198`、`xai-grok-dashboard-store/src/types.rs:11-17, 207-214`）。
- Session picker 的「预览」是**行内展开详情字段**而不是第二个面板；源页签按 `Grok → Headless → External → All → Local → Remote` 固定循环，外来会话带 `claude`/`codex`/`cursor` 徽章且不可删除（`src/views/session_picker.rs:196-237, 676-790`、`crates/codegen/xai-grok-pager/src/views/picker.rs:2933-2946`）。

**三个列表屏的分工**

| 界面 | 数据来源 | 生命周期 | 入口 |
| --- | --- | --- | --- |
| Agent Dashboard | 本进程的 top-level agent（+ 子代理、roster、workspace store） | 本进程内存中的活会话 | `/dashboard`、`Ctrl+\`、`grok dashboard` |
| Session picker（`/resume`） | 磁盘上的历史会话（可含远端 / 外来工具会话） | 进程外、可常驻 | `/resume`、welcome 屏列表、dashboard 的 `Open Previous` |
| Agents modal（`/agents`） | agent 定义 / persona | 配置 | `/config-agents` |

Dashboard 的状态宿主是 `AppView::dashboard: Option<DashboardState>`，仅在 `active_view == ActiveView::AgentDashboard` 时激活；`Option` 只在进程关闭时重置，所以关掉再打开会保留选择与折叠状态（`src/views/dashboard/mod.rs:3-4`）。

**一屏分区**（`DashboardLayout`，`src/views/dashboard/layout.rs:167-257`）

```
┌ terminal 全宽 ───────────────────────────────────────────────────────────┐
│ top_margin（高 > 6 行时）                                                 │
│ main ~/xai [Choose Ctrl+L]   ◆ 2 awaiting │ ⋮ 1 working │ ◇ 1 idle       │ header
│ header_gap（> 12）                                                        │
│ + New Agent        Open Previous /resume │ Worktree Ctrl+w               │ actions
│ actions_gap（> 13）                                                       │
│ ▾ Awaiting 2 ─────────────────────────                                    │ 分节标题
│ ▏● reviewer · audit token flow        Awaiting your input            2m  │ 行（3 行高）
│  ● implementer · fix login bug        Running: cargo test            12m  │
│ ▾ Idle 9 ────────────────────────────                                     │
│  ○ housekeeping                       idle                            1h  │
│  + 5 more                                                                 │ Idle 溢出行
│ dispatch_gap（> 10）                                                      │
│ ╭───────────────────────────────────────────╮                             │
│ │ ❯ Dispatch a new agent                    │  ← dispatch 或 peek         │
│ ╰─ dispatch ────────────────────────────────╯                             │
│ shortcuts_gap（> 11）· footer：↑/↓ select (peek) · Enter open · Ctrl+R rename │
│ bottom_margin（> 16）                                                     │
└──────────────────────────────────────────────────────────────────────────┘
```

装饰行「从底部往上一行一行打开」，注释说这样「每多一行终端高度，要么给列表、要么给恰好一个 gap」；**peek 没有独立 rect**，它渲染在 dispatch 的 rect 内；列表左右各留 `LIST_OUTER_HPAD = 2` 列，header 与 actions 也用 2 列以便与列表对齐（`layout.rs:236-241, 353, 411-421`）。

**表头与动作行**。左侧位置标签是 `branch + " " + [worktree 徽章] + cwd_display`，三段分别上色、整行可点击（hover 加下划线），点击打开目录选择器；右侧 `[Choose Ctrl+L]` 只在预算够时出现（预算 = 最左 chip 的 x 坐标再退 3 格，位置标签左侧永远留 3 格 gutter 保证不被 chip 压住）。状态 chip **只统计 `indent == 0` 的行**（子代理随父行归组，不重复计数），顺序 `awaiting → working → idle → done → failed` 与排序优先级一致；`Inactive` 故意没有 chip，它的数量只出现在分节标题上。动作行左端 `+ New Agent`，右端**严格右到左**：worktree 放不下就整段放弃，`Open Previous` 只在 worktree 已放下且还有空间时才尝试；worktree 已武装时文案变成 `+ New Agent in Worktree` 与 `Disable Worktree`；光标若停在某个右端项而该帧它没被画出来，会退回 `+ New Agent`（`src/views/dashboard/chrome.rs:47-97, 110-179, 264-339, 361-370`）。

**行的数据模型与解剖**。`DashboardRow` 是每帧从 `app.agents` 重建的纯视图结构，不缓存。字段：`label`（显示名 → 生成标题 → 首个 prompt 前 30 字符 → `New session #<id8>`）、`subtitle`（`branch + location[+ " worktree"]`，绘制时以 ` · ` 前置、dim 色）、`state`、`activity`、`secondary_line`（NeedsInput 时是 `Pending: <标题>`，其余是 `last_turn_summary` 或最后一条 agent 消息首行）、`cwd_display` / `cwd`、`last_change_at`（**`SystemTime` 而不是 `Instant`**：roster 时间戳可能早于本进程甚至开机）、`pinned`、`badges`（`worktree`/`needs-input`/`subagents`/`tasks`/`watchers`/`workflows`/`pinned`/`failed`）、`context_pct`（roster 行没有）、`indent`（0 = top-level，1 = 子代理）、`is_more_placeholder`/`more_count`（`src/views/dashboard/row.rs:18-96, 280, 608-621`）。

行固定 3 行高（`ROW_HEIGHT = 3`），内容 1–2 行并在 3 格内垂直居中；列位是 `marker(1) | gap(1) | indent(2n) | icon(1-2) | gap(1) | label...`；背景三态为键盘选中 `bg_highlight` > 鼠标悬停 `bg_hover` > `bg_base`。状态图标：Working = 点状 spinner（`⋅ : ⸬ ⁙` 循环，legacy 控制台降级为 `. : ·`）配 `accent_running`；NeedsInput = 实心菱形（宽模式闪烁）配 `warning`；Idle/Inactive = 空心菱形配 `gray_dim`；Completed = 实心菱形配 `accent_success`；Failed = 实心菱形配 `accent_error`（`render.rs:31, 1502-1526, 1558-1584, 3027-3051`）。右端元数据槽二选一：默认 age，鼠标悬停或已武装删除时换成 `[✗]`，只有「可删除」的行（Idle/Inactive/Completed/Failed）参与；age 用粗粒度分桶 `just now / 5m / 3h / 2d / 1mo / 1y`，注释说明分桶刻意放宽以「避免秒级抖动」（`render.rs:1672-1736`、`state.rs:206-213`、`crates/codegen/xai-grok-pager-render/src/util.rs:108-133`）。活工作 chip 用灰字、无括号，格式 `Subagents 2 · Tasks 3`；空间不足时按 `Full`（`Subagents/Watchers/Workflows`）→ `Short`（`Sub/Subs/Watch/Flow`）→ `2 bg` → `bg` 逐级降级，并给标题留出至少 1 格（`row_title.rs:1-2, 126-194`）。

**分节、折叠与排序**。分节标题只在 `Grouping::State` 下产生，形态 `▸/▾ + 组名 + 计数 + ────` 长横线（窄模式退化成 `Done 12` 无横线），组名读作 `Awaiting / Working / Idle / Inactive / Done / Failed`——注释解释了为什么用 `Done` 而不是 `Completed`：短促、更像分组头。`Inactive` 分节**默认就是折叠的**。Idle 折叠见上方要点；子代理折叠独立（每父行最多 `MAX_VISIBLE_SUBAGENTS = 8`，多的折成 `… N more` 且不可被光标选中）。排序以簇为单位，子代理**永不跨父行重排**；`Directory` 分组下非置顶行先按 `cwd_display` 字典序，此时「状态优先」关掉（`state.rs:270-279`、`render.rs:1271-1408`、`row.rs:871-974`）。

**持久化**：`pinned` / `reorder` / `grouping` / `enabled` 写在 `~/.grok/config.toml` 的 `[dashboard]` 段，键用 **session id**（`top:<session_id>` / `sub:<parent>:<child>`）而非进程内槽位，所以重启后依然有效；解析上限 256 条、键长 1024 字节。环境变量 `GROK_AGENT_DASHBOARD=0` 可整体关掉（`state.rs:380-399, 4512-4520`、`docs/user-guide/23-dashboard.md:306-319`、`mod.rs:84-92`）。

**过滤与搜索**：`a:<text>` 匹配 `label` + `parent_label`（子代理可被父名命中）；`s:<state>` 精确匹配状态，同义词表 `busy/running→Working`、`done→Completed`、`dormant→Inactive`、`errored/cancelled→Failed`，未知 token 回退成子串匹配（让用户能看出状态路径没生效）；`#<text>` 连 `#` 一起匹配（防止误匹配到任意数字）；其他走 `label` + `cwd_display` 子串。命中 `is_more_placeholder` 行时直接保留；`s:` 过滤时不发分节头（`row.rs:791-825`、`state.rs:4322-4377`、`render.rs:978-980`）。

**Session picker 的交互**

- 一套逻辑、三种宿主（`Welcome` / `AgentModal` / `Dashboard`）、两种渲染（`Fullscreen` 交给 welcome 屏的 picker body；`Modal` 用 `ModalSizing { width_pct: 0.65, max_width: 120, min_width: 48, v_margin: 4, h_pad: 2, v_pad: 1, footer_lines: 2 }`）。Dashboard 里打开的标题是 `Open session`（`DASHBOARD_PICKER_TITLE`），只列 `kind == "build"` 且 `SourceFilter::Local` 的本地会话；它的输入路由是一份 89 行的薄壳，只把 `Selected` 映射成 `Action::DashboardPickSession`（`src/views/session_picker_surface.rs:7-65, 67-108, 161-214`、`src/app/dispatch/dashboard.rs:316-346`、`src/app/dashboard_session_picker.rs:13-88`）。
- 搜索栏标签固定 `" search: "`；搜索模式激活时标签用 `text_primary` 加粗，非激活但已有 query 时整条 query 画成 dim 且不画光标。输入即在标题上过滤，同时触发**内容深搜**：查询长度 ≥ 2 且当前源支持 FTS 时才发 `Effect::DeepSearchSessions`，并带 `deep_search_seq` 代际号防止旧响应串台（`CONTENT_EXPAND_OFFSET = 100_000` 用于隔离内容命中与模糊命中的展开索引）；深搜区标题是 `Extended search results (remote and local sessions)`，加载中是 `⸬ Searching session content…`；`External` 源禁用内容搜索（外来会话没有 FTS 索引）（`session_picker_surface.rs:110-148`、`src/app/dispatch/session/load.rs:712-720`、`session_picker.rs:234-237, 861-881`）。
- 「预览」是行内展开：每行右侧固定显示相对时间（右对齐 8 列），展开后依次显示 `ID / CWD / Model / Created / Updated / Source / Host / Messages / Recap / Last turn / Turns+Tools / Prompt`；键位 `y` 复制、`e` 或 `→` 展开、`E` 或 `←` 折叠、空格切换，结果里有命中时自动强制展开。行按 `repo_name` 分组，当前目录所属 repo 组提到最前，其余按字母序，组头是不可选中的 `Header` 行（`session_picker.rs:41-50, 676-722, 736-790`、`views/picker.rs:1598, 2933-2946`）。
- 源页签：`SourceFilter` 六值，循环顺序固定 `Grok → Headless → External → All → Local → Remote → Grok`（默认 `Grok` = 原生且排除外来工具会话；`is_active()` 以 `!= Grok` 为准）。徽章 `conversation` → `chat`，否则 `claude` / `codex` / `cursor`，未知源留空；Headless 页隐藏外来工具会话并在页脚提示 `N external session(s) hidden · f to show`。删除是 `d` 武装、`y` 确认、其他键解除武装，外来源的行不可删除；武装删除时整条页脚替换为 `y confirm delete / n cancel`（`session_picker.rs:57-131, 196-237, 882-900`、`src/app/foreign_sessions.rs:286-293`、`src/views/welcome/mod.rs:2456-2515`）。

**并发模型如何在界面上表达**

- 「谁在跑」不是持久标志而是**每帧重算**：有待答权限或待答提问 → `NeedsInput`；否则有活跃父级活动（turn/唤醒/命令/排队 prompt/replay）→ `Working`；否则有后台工作（running bg task 或非空 `scheduled_tasks`）→ `Working`；否则 `Idle`。因此**一轮 turn 结束但还有后台任务 / monitor / 定时 loop 时，行仍显示 Working**，并由 chip 与活动文案说明「还挂着什么」。子代理只有三种可能：未完成 → Working、完成 → Completed/Failed，**永远不会进入 NeedsInput**（v1 子代理无法向用户提问，有测试固化）（`row.rs:465-502`、`row_activity.rs:10-16`）。
- 「跑了多久」的锚点是有讲究的：Working 行用 `turn_started_at` 优先（这样「跑了 12m」指本 turn），其余状态用 `last_active_at`，两者都缺时落到进程级 `fallback_epoch`（`OnceLock` 冻结一次，避免每帧重锚）；roster / workspace 行的锚点是磁盘上的 unix ms，非正值当作「现在」（`row.rs:88-94, 607-621`、`render.rs:1681`、`util.rs:135-144`）。
- 刷新：Dashboard 打开时事件循环按 `DASHBOARD_POLL_INTERVAL = 1s` 轮询，且**只在 `active_view == AgentDashboard` 时**生效，三条路径择一：workspace store 刷新 → leader roster（`Effect::FetchRoster`）→ 本地磁盘会话列表（`Effect::FetchDashboardSessions`）。行本身不缓存，每帧重建（理由：「一个 pager 进程里 agent 数量是个位数，重建是免费的」）。局部动画按 `spinner_tick` 推进，且**只有真正被绘制出来的动画才继续 tick**（spinner 每 4 tick 换帧、NeedsInput 闪烁每 10 tick）；头部 chip 的 working spinner 也以「这一帧画过 chip」为准来标记（`src/app/event_loop.rs:1659, 2493-2509`、`src/views/dashboard/mod.rs:19-22`、`src/views/dashboard/animation.rs:5-7, 25-64`、`chrome.rs:110-113`）。
- 本地 agent 的元数据（title / model / last_turn_summary / is_worktree / last_change）持续同步进 workspace store，字段各自带上限（title 1024B、model 256B、summary 8192B），超长**按 char 边界截断而非报错**——「因为一次元数据同步不该因为标题太长而失败」（`src/app/workspace_sync.rs:177-205`、`xai-grok-dashboard-store/src/types.rs:20-29, 306-341`）。

**会话标题：生成 → 优先级 → 截断**。展示优先级（三处独立实现、语义一致）：`display_name`（手动 `/rename` 后）→ `generated_session_title` → `pending_prompts` 队首前 30 字符 → scrollback 第一条用户消息前 30 字符 → `New session #<session_id 前 8 位>`。`named_title` 是「前三级，没有就返回 `None`」的版本，供会话头部这种「宁可不显示也不显示假标题」的场合使用。标题由 shell 侧推送进 ACP 通知，pager 只负责接收；`/rename --auto` 把 `generated_session_title` 清空并发 `Effect::ResetSessionTitle`。生成时机与「早几轮重生成后冻结」的策略见用户手册（**本仓库未在 pager 侧找到重生成策略的代码，属于 shell 侧行为，未找到证据**）。截断有三层：标题源本身 60 字符 + `...`（按 char 计数，不切多字节）→ 行内 `truncate_str(label, 可用宽度)`（宽度已扣掉 chip 与 age）→ `New session #id` 后缀先画 `New session`、剩余宽度才画 dim 色的 ` #id`。标题文本进入渲染前统一过 `sanitize_display_text`：C0/C1 控制符、bidi 与格式控制字符替换为 U+FFFD，防止后端/模型内容注入终端转义序列或伪造标题（dashboard 走 `set_string` 原样写进 ratatui buffer，所以这层过滤是必需的）（`row.rs:503-507, 537-567`、`src/views/session_title.rs:16-33, 35-50, 151-180`、`row_title.rs:42-70`、`src/app/acp_handler/session_notification.rs:1004, 1019`、`docs/user-guide/17-sessions.md:44-46`）。

**跨进程 / 跨机的表达**

- **Leader roster**：请求 `x.ai/sessions/list` → `RosterListResponse`，广播 `x.ai/sessions/changed` → `RosterChanged { upserted, removed }`；pager 侧是**只反序列化**的镜像类型。解析器必须先剥 `{ "result": ... }` 信封，否则会「成功解析出空列表」，这个坑有专门的回归测试。界面映射 `RosterActivity → RowState`：`Working→Working`、`NeedsInput→NeedsInput`、`Idle|Dormant→Inactive`、`Completed→Completed`、`Dead→Failed`。**`Inactive` 是 roster 专属语义**（只由 roster 产生，本地 agent 永不落入），定位是「别的 pager 进程拥有、本进程没加载」的后台噪音，所以放在 Idle 之下、Done 之上且默认折叠。显示条件：与本地 agent 同 session id 的 roster 行被跳过；既无标题又不活跃且未置顶的 roster 行不显示；roster 行没有 `context_pct`，`secondary_line` 取 `last_turn_summary` 或 `model_id`（`src/app/roster.rs:1-78, 80-87, 113-150`、`row.rs:362-462`、`state.rs:194-199, 217-228, 1196-1198`）。
- **Workspace store**（dashboard v2 的权威数据源，「membership、layout、以及本进程没加载的会话」都由它说了算）：SQLite 存储；`Member` 含 `session_id / kind / origin / cwd / title / model / last_turn_summary / is_worktree / last_change_unix_ms / pin_rank / order_rank`；`MemberKind = Build | Conversation | Other`，`origin` **只在插入时写一次，之后任何 API 都不改**；容量 `WORKSPACE_CAPACITY = 256`，超限时在同一事务里淘汰最旧的未置顶成员；rank 用间隔 1024 的整数、插入取中点；`WorkspaceSnapshot` 在同一个读事务里带 `PRAGMA data_version`，这样「轮询方不会漏掉落在快照与首次轮询之间的那次提交」。界面上多出来的两种行：`DashboardRowId::Workspace`（store 里有、本进程没加载 → 标题取 title、退化到 cwd 名、再退化到 session id；状态**恒为 Idle**）与 `provisional` 行（活 agent 但还没落库的成员）（`xai-grok-dashboard-store/src/types.rs:11-17, 169-214, 365-403, 456-465`、`row.rs:131-286`）。
- **Foreign sessions**（其它 CLI 的历史会话）只在 session picker 里表达，不进 dashboard 行列表：源为 `claude / codex / cursor`（显示名 `Claude Code / Codex / Cursor`），各自由 `resume-claude|codex|cursor` 技能门控；另有一条独立的「启动时探测可恢复的外部会话」链路，welcome 屏在满足一串前提（当前视图是 Welcome、没有认证返回流、home 目录未用或为空、非 chat 模式、非 ZDR 阻断、没有待定更新）时才发起探测，并用 `foreign_resume_launch_generation` 令牌 + cwd canonicalize 做竞态防护（`src/app/foreign_sessions.rs:13-120, 204-260`）。
- **本机崩溃恢复**：`xai-grok-active-sessions` 维护 `~/.grok/active_sessions.json`（`{session_id, pid, cwd, opened_at}`），干净退出删条目、崩溃留下；下次启动 `collect_crashed()` 用 `kill(pid, 0)` 挑出死掉的 PID。它提供的是「进程级的会话占用事实」——**未找到它在 dashboard 中被直接渲染的证据**（`crates/codegen/xai-grok-active-sessions/src/lib.rs:1-3, 16-26, 175-206`）。

**与单会话 TUI 的切换与 peek/dispatch 互斥**。`ActiveView` 只有三态 `Welcome / Agent(AgentId) / AgentDashboard`；dashboard 不是独立渲染栈，而是同一个 `AgentView` 外加一层「从 dashboard 来」的铬标：`app.active_view` 决定画哪个，`app.dashboard: Option<DashboardState>` 保存全部状态（关闭视图不销毁状态），`dashboard.attached_agent: Option<AgentId>` 是**面包屑**（进入会话时设上，退出 dashboard 时清空）。进入要过四道门（功能关闭 / 未登录 / 同意书未答 / 目录信任未答），并记录 `dashboard_return`（`Agent(id)` / `Welcome` / `Overlay(id)`）；已在 dashboard 时再按一次等于退出。会话头部用 `[Dashboard]` 按钮与 `‹ i/n ›` 位置指示提供返回与循环（只有 1 个会话时不显示），**循环顺序 = 渲染器实际画出来的行序**，而不是 agents map 的插入序。细节视图内的 `Ctrl+X` 是状态相关的：turn 运行中 = 取消 turn；空闲 / 命令在飞 = 武装 2 秒确认关闭会话（`CONFIRM_WINDOW = 2s`）（`src/app/app_view.rs:206-243`、`src/app/dispatch/dashboard.rs:160-315`、`src/app/agent_view/render.rs:1341-1372`、`src/views/dashboard/mod.rs:57-79`）。

列表下方那块位置只有一个 owner：分配由 `layout::allocate_peek` 做 **list-first**——先保 `LIST_FLOOR_ROWS = 12` 行列表，剩余空间给 peek，peek 整体高度封顶 `⌊H × 3/8⌋`、内部 body 再封顶 28 行；压到 `PEEK_MIN_BOX_LIVE_TAIL = 8`（普通）或 `PEEK_MIN_BOX_QUESTION = 10`（有选项待答）以下就干脆不显示。peek 标题行是「最近一次响应类型 + 时间」，底部边框挂 model 与 `always-approve` / `auto` / `plan` 标志。预览面板可在 `/settings → Appearance → Dashboard preview`（`[ui] dashboard_preview`）关闭以换取列表空间，关闭后 `set_peek(None)` 并恢复 viewport（`src/views/dashboard/layout.rs:12-26, 36-150, 259-274`、`peek.rs:64-105`、`docs/user-guide/23-dashboard.md:95-101`、`src/settings/defs.rs:548-561`、`src/views/dashboard/preview.rs:31-40`）。
