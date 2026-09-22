### 5.5 Agent 轮次交互：流式、插话、队列与取消

> 路径约定：本节与 5.6–5.8 中不带 crate 前缀的短路径一律相对 `crates/codegen/xai-grok-pager/`；其它 crate 在首次出现时写全路径。

**本节要点**

- 一次轮次被切成四段：乐观本地回显 → prompt ack 窗口 → 流式运行 → 收尾；每段有独立状态与独立兜底（`app/prompt_ack.rs`、`app/dispatch/prompt_ack.rs`、`app/turn_completion.rs`）。
- 发送后**唯一被设界的是「确认（ack）」而不是 RPC**：软提示 10 s、硬放弃 120 s，且 `soft = PROMPT_ACK_SOFT_NOTICE.min(hard / 2)` 把两者强制耦合，软提示永不可能晚于硬超时（`app/prompt_ack.rs:14-24, 48`）。
- 运行中再次输入是**三分法**：普通 Enter 入队；Send now chord 取消当前轮并立刻发；真正的 `Action::Interject` 只由队列行编辑与计划评审意见产生（`actions/defaults.rs:497-511, 625-654`、`app/queue_edit.rs:509, 535`、`app/agent_view/plan.rs:498`）。
- **Esc 永不取消回合**，Ctrl+C 才取消；已在取消中再按 Ctrl+C 升级为退出。取消粒度只有 `Turn` 与 `Compaction`，**没有「取消当前工具」**（`app/agent_view/prompt.rs:747-821`、`app/agent_view/input.rs:1231-1247`、`crates/codegen/xai-grok-telemetry/src/events/turn.rs:38-43`）。
- 收尾是四件套而不是一句 toast：scrollback marker、终端通知、标题回 idle + 进度条清除、以及「仍有后台工作」的常驻计数行（`scrollback/blocks/session_event.rs:376-382`、`views/turn_status.rs:105-130`）。
- 全节贯穿一条纪律：**任何等待都不阻塞输入**，三套超时兜底一律跑在动画 tick 上（`app/event_loop.rs:2406-2430`）。

**一次轮次的四段时间轴（全节主干）。**

```
用户按 Enter
  ├─① 乐观反馈（本地，0 延迟）：用户气泡入 scrollback、composer 清空、draft 消费；不等任何网络往返
  ├─② Prompt ack 窗口（网络；session/prompt 自身无 RPC deadline）
  │    软期限 10s → spinner 文案改为「等待 agent 接受」；任一确认信号到达 → 撤销 watch、写 prompt.acked
  │    硬期限 120s → 本地放弃并还原文本
  ├─③ 回合运行中（流式）：工具块逐条实时更新；可插话 / 排队 / Send now / Ctrl+B 转后台
  │    Esc 不取消（只提示），Ctrl+C 才取消
  └─④ 收尾（TurnEnded）：落「Worked for X」marker + 终端通知 + 标题回 idle + 进度条清除
       queue 非空时抑制 TurnComplete 通知（下一条马上开始，避免闪烁）
```

轮次状态机只有 5 个值（`app/agent.rs:573-588`）：`Idle`、`TurnRunning`、`TurnCancelling`、`CommandRunning { command, started_at }`、`CommandCancelling { command }`。关键不变式：**driver 侧只有 `session/prompt` 的 RPC 响应能离开 `TurnRunning`/`TurnCancelling`**（`app/turn_completion.rs:481-483`），所有「广播先到、RPC 后到或丢失」都靠 tick 上的兜底补齐。

**Prompt ack 的预算与三种解除信号。**

`app/prompt_ack.rs:14-24` 的常量少到可以直接列完：软提示 `PROMPT_ACK_SOFT_NOTICE = 10 s`（`:16`）、硬期限 `DEFAULT_PROMPT_ACK_TIMEOUT = 120 s`（`:20`）、覆盖入口 `GROK_PROMPT_ACK_TIMEOUT_SECS`（`:14`）、钳制区间 `MIN/MAX_PROMPT_ACK_TIMEOUT_SECS = 5 / 3600`（`:22, 24`）。

120 s 的来源只写在注释里：要盖过 shell 首个 prompt 的最坏路径（模板化 delivery-tools 前缀 60 s + 10 s、一次设置刷新最长 16.5 s、一次 auth 刷新、两次 actor 往返），**仓库内没有实测分布数据（未找到证据）**（`app/prompt_ack.rs:17-19`）。解除信号恰有三种（`AckSignal`，`app/prompt_ack.rs:77-83`）：`x.ai/queue/changed` 的 `running_prompt_id` 命中或 `entries` 含该 id（`:129-135`）、`session/update` 携带该 `prompt_id`、以及回合结束（`mark_turn_finished` 里无条件调用，`app/agent_view/session.rs:499`）。chat 型会话**从不武装** watch，理由是「gateway bridge 没有 queue 广播，它的第一个信号就是第一个 delta」（`app/agent_view/session.rs:507-511`）。

**软提示不弹 UI，只换 spinner 的原因。** 软期限到达只做两件事：写一条 `prompt.ack_soft_notice` 日志（含 `waited_ms` / `limit_ms`）并请求重绘（`app/dispatch/prompt_ack.rs:97-108`）。可见变化发生在活动解析层——`resolve_turn_activity_unenriched()` 在 `TurnRunning` 且 `prompt_ack.is_soft_noticed()` 时返回专用的 `WaitingReason::PromptAck`（`app/agent_view/session.rs:1077-1084`）。该枚举是 view-only 值，tracker 从不存它（`app/acp/tracker.rs:70-72`），文案为 `Waiting for the agent to accept the prompt…`（`app/acp/tracker.rs:134`，遥测名 `waiting_prompt_ack` 在 `:146`）。软提示期间 `[stop]` 仍在——按钮按 running/cancelling 显示，不看 ack 状态。

**硬超时是「还原」而不是「丢消息」。** `fire_fail_safe()`（`app/dispatch/prompt_ack.rs:135-211`）先决定文本去哪儿（`restore_target`，`:118-131`）：

| 条件 | 结果 |
| --- | --- |
| composer 为 `PromptMode::Normal`、无文本无图片 | `ReplaceComposer`（放回输入框） |
| composer 有文本、无图片 | `MergeIntoDraft`（放在草稿**上方**） |
| composer 有图片 | `None` —— 两处图片来源都从 1 编号，`[Image #N]` 占位符会撞车（`:128`） |
| 正在编辑队列行（非 `Normal`） | `None` —— 写进去会污染那行 |

无可恢复目标时走 `NotRestorable`：结束回合、用户块留在 scrollback 供复制。收尾三连是 scrollback 通知 `The agent did not accept your prompt within {N}s. {recovery} If this keeps happening, run /feedback.`（`:24-40`）、toast（`Prompt not accepted, text restored` / `...turn stopped`，`:20-21`）与遥测 `PromptAckTimeoutFired { limit_ms, waited_ms, surface, disposition, prompt_kind }`（`:196-202`）。两个易忽略的细节：这条路径走 `finish_turn_view(agent, TurnEnd::Aborted)`（`:165`），所以**不产生 cancel latency 事件**，并显式清掉 `pending_cancel_resend`（`:166-167`）——取消一个 shell 从未运行过的 prompt 没有意义；另外 stale 守卫刻意让 `TurnCancelling` 继续保活 watch，注释写「取消中仍然武装，这样 wedge 期间按 Esc 不会让面板永远停在 Cancelling」（`app/dispatch/prompt_ack.rs:83-84`）。

**运行中再次输入：三分法（最容易被误解的一块）。**

| | 普通 Enter | Send now chord | 真正的 Interject |
| --- | --- | --- | --- |
| 键位 | `Enter` | 非 Apple `Ctrl+Enter`（alt `Ctrl+I`）；Apple Terminal `Ctrl+O`（alt `Ctrl+Enter`/`Ctrl+I`）；VS Code 家族 `Ctrl+L` | 无独立 composer 键位 |
| 是否取消当前轮 | 否 | **是** | 否 |
| producer | `dispatch/prompt.rs` 立即发或本地入队 | `app/agent_view/prompt.rs:555-597` → `Action::SendPromptNow` | `app/queue_edit.rs:509/535`、`app/agent_view/plan.rs:498` |
| 效果 | 入队，当前回合结束后按序 drain | 取消当前轮，新文本作为下一轮 | 合成一条 user message 插进**正在运行**的回合 |
| 证据 | `actions/defaults.rs:497-511` | `actions/defaults.rs:625-654` | `app/dispatch/interject.rs:13-118` |

这里是**一处需要并列保留的冲突**：`ActionId::InterjectPrompt` 的 `ActionDef` 内部自相矛盾（`actions/defaults.rs:625-654`）——`label: "send now"`、`description: "Send now while running (cancels the current turn)"`，而 `long_help` 却写「without cancelling it (interject)… this chord merges composer text into the current turn instead」。**代码站在 `description` 一边**：处理器在 composer 非空且 `can_send_now()` 为真时直接返回取消并发送（`app/agent_view/prompt.rs:555-597`）。composer 为空时退化为「强制发送队首可见行」（`try_send_now_queued_from_prompt`，`app/agent_view/queue.rs:83-110`）；`can_send_now()` 的定义是「turn 正在跑，或 wake turn 活着且未在取消」（`app/agent_view/session.rs:848-851`）。照抄 `long_help` 会得到错误的键位语义。

**队列语义：两层队列 + 乐观回声。** 队列有 pager 侧（`session.pending_prompts` 本地 drip、`shared_queue` 服务器镜像、`optimistic_queue_ids` 乐观回声）与 shell 侧（`state.pending_inputs` 的 `InputItem` FIFO）两层，靠 `x.ai/queue/changed` 对齐（`app/dispatch/queue.rs`）。「立即发到服务器队列」要同时满足全部条件（`immediate_server_send_eligible`，`app/dispatch/queue.rs:20-31`）：leader 模式 / `follow_up_steer` 打开 / 有 wake turn；**且** server busy；**且**有 session id；**且**本地 `pending_prompts` 为空（FIFO 守卫：本地非空时不许插队到服务器队列）；**且**不处于队列行编辑、不在切模型、不在 replay。不满足就走本地入队（`app/dispatch/prompt.rs:1214-1240`）。界面表现：队列面板 `Ctrl+;`（本地 mac VS Code 家族为 `Ctrl+4`，`actions/defaults.rs:547-560`；手册 `docs/user-guide/20-background-tasks.md:181`）；发送时先画乐观回声（`push_server_queue_echo`，`app/dispatch/queue.rs:35-54`）并登记 `optimistic_queue_ids`，未确认期间对该行按 Send now 会被**挂起**而不是发出去（真发会被 shell 超车然后 no-op，`app/agent_view/queue.rs:375-390`）；turn status 行只在 Enter 真能生效时才写 `· {N} queued, Enter to send now`，否则只写 `· {N} queued`（`views/turn_status.rs:489-495`，parked 分支同一逻辑 `:253-260`）。

**插话语义：回合内的合成 user message。** 核心实现放在共享 crate 供 shell 与 server 侧 agent loop 复用（`crates/common/xai-interjection-core/src/lib.rs:5-9`）。载荷统一由 `frame_user_turn` 生成（`crates/common/xai-interjection-core/src/format.rs`）：

（三段式：`The user sent a message while you were working:` / `<user_query>{用户文本}</user_query>` / `Make sure to complete any unfinished tasks from previous turns.`）

`INTERJECTION_NOTE`（`:4`）与 `INTERRUPT_NOTE = "The user interrupted the previous turn:"`（`:5`）共用同一信封（`:22-24`）；末尾的 unfinished-tasks 提醒是刻意的，防止中途 steer 或取消后跟进导致在飞任务被丢下（`:7-9`）；超过 `LARGE_PROMPT_THRESHOLD = 25_000` 按 UTF-8 边界截断并追加 `... [truncated]`（`:2, 27-37`）。排空只发生在「安全点」——一个工具批次之后、循环顶部、以及回合返回用户之前（`crates/codegen/xai-grok-shell/src/session/acp_session_impl/interjection.rs:279-301`）；同一函数若发现 `follow_up_steer_enabled()` 为真且队里还有可编辑的人写行，会先 `promote_queued_as_interjections()` **把队列行提升为插话**——这就是「队列 vs 插话」在配置层面的开关。排空实现用 `RestoreOnCancel` 守卫先接管全部 entry，逐个 sanitize（剥掉 `[Image #N: <path>]` 里的路径）、解析 `/skill` 引用、包装、走图片管线，然后**一次性** `try_push_user_messages_batch` 提交，成功后才落盘；守卫的 `Drop` 调 `EventQueue::restore_front()` 把未提交项放回**队首**（`crates/common/xai-interjection-core/src/events.rs:75-82`）。

**落空不丢消息。** 插话只在运行中合并；若落地时会话已空闲、或错过本回合最后一次排空，就转成独立回合，prompt id 前缀 `interject-fallback-`（`acp_session_impl/interjection.rs:17-24, 30-83`），并按原顺序推到队列**最前**（`flush_stranded_interjections`，`:87-96`，用反向 `push_front` 保持 entry 0 在前）。pager 侧（`app/dispatch/interject.rs:13-118`）立即推一个本地用户块（`RenderBlock::interjection_prompt`），把 id 记入 `self_interjection_ids` 以丢弃广播回声，同时记 `interjection_painted_blocks` 与 `interjection_retry_images`，弹 toast `Interjection sent`，最后返回 `Effect::SendInterject`。一条硬约束写在测试注释里：**dispatch 永不触碰 composer**，只有 composer 路径的调用点自己清空（`app/dispatch/interject.rs:216-218`，测试 `interject_dispatch_never_touches_the_composer`）。

**Send now：唯一同时产生「取消」和「新消息」的路径。** `dispatch_send_prompt_now()`（`app/dispatch/interject.rs:122-205`）需要三处配合：断连守卫（`reconnect_pending` 时把文本 requeue 到本地队首并 toast `Reconnecting, please wait...`，而不是打进死通道 `:145-163`）；**静默取消**（`arm_send_now_and_paint_dispatched()` 让取消的 `TurnCancelled` marker 被抑制——`TurnStopReason::Cancelled if input.send_now_cancel => None`，`app/turn_completion.rs:95`，注释称之为「cancel-and-send 的静默那一半」）；以及乐观队列回声 + `Effect::SendPromptNow`（`:190-204`）。

**取消：Esc 不取消是刻意的。** `try_handle_esc_policy()` 的运行中分支（`app/agent_view/prompt.rs:747-821`）直接提示正确键名而不取消，三个设计点：提示里的键名**从 registry 现取**（`registry.key_for(ActionId::CancelTurn)`），用户重绑定后提示跟着变（`:773-778`）；同一个 Esc 顺便把 rewind-arming 推迟 `ESC_CANCEL_REWIND_GRACE = 1000 ms`（`:738-741, 781`），避免「回合即将结束时空按 Esc，下次按就立刻武装双击 rewind picker」；minimal 模式没有 toast 槽位，提示改写成 scrollback 里的 system 块且**每个用户回合最多一条**（`show_cancel_key_hint`，`:824-836`）。`ActionId::CancelTurn` 的 `long_help` 把策略写成正式契约（`actions/defaults.rs:508`）：*Ctrl+C cancels when the prompt is empty; with a non-empty draft it clears the prompt first and leaves the turn running. **Esc never cancels a turn**.* 已在取消中时 Esc 被**静默吞掉、连提示都不给**——那时 Ctrl+C 已升级为退出，再提示 Ctrl+C 会误导（`app/agent_view/input.rs:2276-2285`）。空闲时 Esc 是另一套「双击」语义（有草稿 → 武装 `ClearPrompt`；空且有历史 → 武装 `RewindShowPicker`），窗口 `PendingAction::ESC_DOUBLE_PRESS_TTL = 800 ms`（`app/app_view.rs:476`），可用 `GROK_ESC_DOUBLE_PRESS_MS` 覆盖（上限 `ESC_DOUBLE_PRESS_TEST_MS = 60_000`，`:501-514`）。

**取消粒度与延迟指标。** `CancellationScope` 只有两个值（`crates/codegen/xai-grok-telemetry/src/events/turn.rs:38-43`）：`Turn` → `session.cancel_turn()`、`Compaction`（`state.is_compact_running()`）→ `session.cancel_compact_command()`；两者由 `cancel_and_arm(scope, origin)` 一个入口分派，保证「动作」与「它测量的 scope」不漂移（`app/agent_view/session.rs:543-554`）。`/compact` 是唯一被当作独立 scope 的命令，测试确认 `/jump` 打开时 Ctrl+C 仍能取消 compact 而不是被 picker 吃掉（`app/agent_view/input.rs:2334-2351`）。延迟指标先 anchor 后结算：只有 `CancelOrigin::UserGesture` 且 `surface() == ViewSurface::Root` 才设 anchor（`Programmatic` 标了 `#[allow(dead_code)]`），`TurnEnd::Completed` 才产出 `CancellationCompleted { latency_ms, scope }`（遥测名 `cancellation_completed`，`crates/codegen/xai-grok-telemetry/src/events/mod.rs:289`），`Aborted`（reload / fork / 会话失败强制置闲）**直接丢弃**；anchor 与消费都 `take()`，所以每个事件最多上报一次（`app/cancel_latency.rs:8-25`、`app/agent_view/session.rs:550-571`）。

**二次按下与升级：Ctrl+C → Ctrl+C = 退出。** 默认键 `Ctrl+C`，且**只在 `When::AgentScreen` 层注册**——scrollback 层与全局层都查不到它（`actions/mod.rs:785-796` 的测试 `cancel_at_agent_level`），这条注册边界本身就是「取消是 agent 屏幕级动作，不是全局退出」的表达。处理器逻辑（`app/agent_view/input.rs:1231-1247`）：活动在跑 → `Action::CancelTurn` 并记 `cancel_trigger_hint`；已在 cancelling → 返回 `Action::Quit`；minimal 模式空闲且草稿为空 → `Quit`。同一升级在 wake turn 上也被测试锁定（`:2376-2394`）。

**取消前的闸门：子代理还在跑怎么办。** 触发取消时若仍有子代理在运行，会先弹一个 4 选项面板（`views/modal.rs:120-145, 899-960`）：

（面板形如 `Subagents are still running. Stop them?` + 四选项 `1 (●) Stop running` / `2 (○) Continue to run` / `3 (○) Always stop` / `4 (○) Always continue`。）

标题带子代理计数行 `1 subagent running` / `N subagents running`（`:906-912`）；前两项只影响这次取消，后两项写全局/会话偏好 `cancel_subagents_on_turn_cancel`（`app/dispatch/turn.rs:230-249`）；键盘走 `app/agent_view/interactions.rs:224-300`，鼠标按行 hit-test。

**取消后的界面状态与两条兜底。** `cancel_agent_turn()`（`app/dispatch/turn.rs:253-349`）的顺序是：compact 在跑 → 走 `Compaction` scope；wake turn 在跑 → `mark_wake_cancel_sent()`（wake turn 从不被 adopt，取消状态挂在 `running_wake_turn` 而不是 `AgentState`，`:526-527`）；非运行 → 直接返回；否则先评估 rewind（`cancel_rewind_enabled && in_flight_prompt` 存在 `&& !queue_held_behind_turn && !in_flight_committed && !composer_has_draft && rewind_prompt_id.is_some()` 全真时，把提示文本**收回 composer**，界面看起来像「用户从没按过发送」）；收尾清 cancel panel、`drain_permission_queue`（每个挂起权限各得一个 Cancelled）、`dismiss_in_turn_plan_review`、`clear_send_now_expectation`。注释还明确一条克制策略：交互式取消**只拆当前回合**，不把队列里的 prompt 拉回输入框、也不预测新队列顺序，让 agent 自己把队首提升为下一轮（`:126-128`）。两条兜底都跑在动画 tick 上，顺序是 cancel → prompt ack → lost response（`app/event_loop.rs:2406-2430`）：取消重发（fire-and-forget 的 `session/cancel` 会丢）用 `CANCEL_RESEND_GRACE = 3 s` + `CANCEL_RESEND_MAX_ATTEMPTS = 3`（`app/dispatch/turn.rs:502-506, 510-564`），丢失 RPC 响应的回合收尾用 `TURN_END_RECONCILE_GRACE = 2 s`（`app/dispatch/turn.rs:566-569, 574+`）。

重发有一条精细抑制：一旦 `pending_turn_end_reconcile` 已武装（说明收到了 `prompt_complete` 广播）就**不再重发**，否则会竞争甚至取消掉 shell 已经提升的队列 prompt（`:533-540`）。

**取消回执归因。** 文案由 `_meta.cancelTrigger`（`"send_now" | "ctrl_c" | "mouse" | "dashboard_stop" | "session_close" | "session_delete" | "shutdown" | "host_interrupt"`）与 `_meta.cancellationCategory`（`HookDenied` / 权限拒绝 / 权限忽略 / 达到回合上限）拼出，另有 `_meta.completionKind: RemovedFromQueue` 用来区分「排队但从没跑过」（`app/turn_completion.rs:22-27`）。解析顺序是 **category 优先、trigger 兜底**（`scrollback/blocks/cancel_cause.rs:24-39`），得到 8 种 `CancelledBy`：`User`（`Turn cancelled by user`）、`SessionClosed`、`Shutdown`、`MaxTurns`、`PermissionDenied`、`PermissionDismissed`、`HostInterrupt`、`Unspecified`（短语表 `:41-52`）。值得注意的是 wire 上的 `"esc"` 也算用户手势（测试 `:61` 遍历 `["esc","ctrl_c","mouse","dashboard_stop"]` 全部映射到 `User`）——这是给其它前端留的兼容位，pager 自己不发它。pager 侧的 `CancelTrigger` 只有三个值：`CtrlC` / `Mouse` / `DashboardStop`（`app/actions.rs:1154-1171`），随 `session/cancel` 的 `_meta.cancelTrigger` 上报，让 agent 的 `mid_turn_abort` 遥测能区分手势（`app/actions.rs:1576-1580`）。

**收尾反馈的四件套。** pager 从两条轨道得知回合终结——fire-and-forget 的 `x.ai/session/prompt_complete` 广播，与持久化、会写盘并重放的 `XaiSessionUpdate::TurnCompleted`——最终汇聚到 `finalize_turn_from_terminal()`（`app/turn_completion.rs:1-7, 484-552`），分流规则是 driver（`!attached_as_viewer`）不在这里结束回合（`PromptResponse` RPC 拥有生命周期），viewer 则 adopt 并自行终结，这样「回合中途重新挂上的 viewer」能从重放里终结回合而不是永远卡在 Waiting。

- **scrollback marker**（唯一文案来源 `scrollback/blocks/session_event.rs:376-382`）：`Worked for 1m20s`（故意**不加句点**）、`Turn completed.`、`{cause} in 1m20s.`、`Turn blocked by a hook in 1m20s.`；三种情况**不推标记**（`app/turn_completion.rs:87-109`）：send-now 的静默取消、`RateLimit`、以及已有错误横幅的 `Error`。
- **终端通知**：5 种事件类型（`notifications/config.rs:100-115`），默认只开 `TurnComplete` 与 `ApprovalRequired`（`:30-33`）；编码 `NotificationMethod`：`Auto`（默认）/ `Osc9` / `Osc99` / `Osc777` / `Bel` / `None`，触发条件 `NotificationCondition`：`Unfocused`（默认）/ `Always` / `Never`（`:44-63`）。注意 **`Bel` 是通知编码方式之一，而不是独立的「响铃」功能**——仓库里没有找到独立的声音/蜂鸣逻辑（未找到证据）。**队列非空时抑制 TurnComplete 与 idle escapes**（下一轮马上起来，会覆盖标题造成闪烁），并把「已 stash 的服务器权威 adoption」当作队列非空处理（`app/dispatch/prompt.rs:1724-1744`）；通知**故意延迟 3 帧（≈99 ms）**，因为 Ghostty 对 `setTitle()` 有 75 ms 防抖，立刻读标题会读到旧值（`:1753-1773`），倒计时在每 tick 递减、归零才 `notify()`（`app/app_view.rs:5495-5503`）。
- **标题与进度条**：`TitleItem` 有 8 种（`notifications/config.rs:87-98`），默认顺序 `[ActionRequired, Spinner, Activity, SessionName, Grok]`（`:72-85`），`ActionRequired` 只在有待处理权限时出现并**跨 tick 闪烁**（测试 `title.rs:562-616`）；进度条走 OSC 9;4，`PROGRESS_KEEPALIVE = 5 s`（Ghostty 约 15 s 静默后自动复位，故需定期重发），空闲时 `build_idle_escapes()` 清进度并把标题改回 idle（`notifications/mod.rs:14-16, 118-124, 140-179`）。
- **后台仍在跑的常驻提示**：`◎ 1 command · 2 monitors · 1 loop · 1 subagent still running`（`views/turn_status.rs:105-130`、`docs/user-guide/20-background-tasks.md:187-201`），`0` 的种类不列出、单数不加 `s`；这是**唯一**展示运行计数的地方；等待型 blocked 状态追加 `· send a message to interrupt`（`:253-262`）；脉冲图标 `○ ◎ ◉ ◎` 用 `MONITOR_PULSE_DIVISOR = 8`，约为活动 spinner（`SPINNER_DIVISOR = 4`，~7.5 fps）的一半速度，注释理由：idle 提示应当「呼吸」而不是被读成活动中的 spinner（`:29-36`）。

**「一切不阻塞输入」的设计手法（8 组）。**

| # | 手法 | 证据 |
| --- | --- | --- |
| 1 | 乐观本地回显：插话、Send now、服务器队列行都在 dispatch 时画出，不等服务器确认 | `app/dispatch/interject.rs:76-89`、`app/dispatch/queue.rs:35-54` |
| 2 | 草稿 stash & restore（卡片、队列编辑、外部编辑器轮转），加取消安全：被 abort 的排空把未提交项放回队首、乐观队列行在 RPC 落定后退役 | `app/agent_view/elicitation.rs:2`、`app/queue_edit.rs:6-7`、`crates/common/xai-interjection-core/src/events.rs:75-82`、`app/dispatch/queue.rs:56-60` |
| 3 | 阻塞式交互做成卡片而非阻塞循环，用 `KeyOwner` 优先级表仲裁键盘；卡片挡住键盘时仍留 `Tab`/`Space` 交还路径，Esc 分级后退 | `app/agent_view/key_owner.rs:18-36, 38-53, 55-60` |
| 4 | 误触防护：Esc 不取消（只提示正确键名），Ctrl+C 取消，再按升级为退出 | `app/agent_view/prompt.rs:772-782`、`app/agent_view/input.rs:1231-1247` |
| 5 | 插话落空不丢消息：错过则转 `interject-fallback-` 独立回合并推到队首 | `acp_session_impl/interjection.rs:17-24, 87-96` |
| 6 | 三套超时兜底都跑在动画 tick 上，失败也不阻塞输入：ack 120 s、取消重发 3 s×3、丢失响应 2 s | `app/prompt_ack.rs:20`、`app/dispatch/turn.rs:503-506, 569`、`app/event_loop.rs:2406-2430` |
| 7 | 通知 / 标题 / 进度走 out-of-band 队列不等帧管线（帧可能被 ack 挂住），但 idle 标题转义仍走帧管线以保序；tick 需求按需升降频 | `notifications/mod.rs:45-46, 93-95, 118-124`、`app/app_view.rs:5659-5661`、`app/status_line_policy.rs:47-60` |
| 8 | 长输出用覆盖式单缓冲 + `BG_TASK_MAX_STDOUT = 10 MiB` 上限 + 单向 `truncated` 闩锁 | `app/acp_handler/background.rs:19-53`、`app/agent.rs:153, 218-235` |

两处与「等待」相关的具体机制值得单列：`Ctrl+B`（`ActionId::SendToBackground`，label `send to bg`，`actions/defaults.rs:611-621`）把正在跑的前台命令转后台（手册补一条容易忽略的：**发一条新消息也会把该命令转后台而不是杀掉它**）；当回合阻塞在「可被用户打断的等待」上时，发消息会立即打断等待并运行你的消息，判定 `is_parked_on_sendable_wait()` 要求 `is_sendable_wait(activity)` 为真**且** goal 循环未激活——因为 shell 在 goal 运行期会抑制这个 abort，把它们当成可打断会说谎（`app/agent_view/queue.rs:105-118`）。子代理「先完成、后 spawn」的生命周期错位则用一个有界缓冲处理：`MAX_DEFERRED_SUBAGENT_FINISHES = 256`、`DEFERRED_SUBAGENT_FINISH_TTL = 60 s`，`defer()` 时把 `SubagentFinished` 的 `output` 置空再存，真正的输出在 `take()` 时再取（`app/deferred_subagent_finishes.rs:10-11, 82-84`）。

### 5.6 会话生命周期与终端状态管理

**本节要点**

- 首屏之前的全部工作都是**串行 await**，唯一有名字的等待预算是 `EARLY_PREFETCH_WAIT = 2 s`；最慢的一步是 `materialize_startup()`（含远端恢复 ≤90 s）。没有 splash、没有进度条实现（未找到证据）（`app/mod.rs:550, 663-1062`、`app/session_startup.rs:1267-1276`）。
- 终端接管是一张**一次性副作用清单**，顺序即语义：raw mode 最先，panic hook 与信号处理器紧随其后安装，光标样式默认 `Inherit` 不发转义（`app/mod.rs:1459-1661`）。
- 「向终端写什么」只有**一个真源** `emit_terminal_teardown_sequences`（且**不含** `disable_raw_mode()`），而「谁在什么条件下调它」有三条路径：事件循环后、panic hook、信号（`app/terminal_restore.rs:61-134, 143-185`）。
- 所有等待都有界：writer join 2 s、reader join 250 ms、DA1 fence 1 s、exit 看门狗 20 s + 5 s（`app/terminal_restore.rs:27`、`app/reader_thread.rs:16`、`app/teardown_fence.rs:37-48`、`app/exit_timeout.rs:8`）。
- 历史重放有专门的「延迟渲染」通道与 `SessionLoaded` 栅栏：本会话 replay **不设超时**，无关流量 2 s 放行（`app/acp/tracker.rs:1132-1136`、`app/session_load_barrier.rs:26, 174-180`）。
- 屏幕模式可在进程内切换（失败即回滚），exec 热重启只是兜底；Unix 用 `exec` 保住 PTY 身份（`app/mode_switch.rs:108-207`、`app/screen_mode_relaunch.rs:211-217`）。

**启动序列：串行阻塞段与一次性副作用。** `main()` 先处理三件与 TUI 无关的事：mermaid / voice 的「子进程自执行」旁路、`PagerArgs::parse_cli()`、版本与 doctor 的早退；`/export`、`/share`、`sessions` 等子命令在这里分流，随后才调 `xai_grok_pager::app::run(args, bg_update_rx)`（`crates/codegen/xai-grok-pager-bin/src/main.rs:2025, 2028-2033, 2315-2319, 2519`）。首屏之前没有 splash，只有 stderr 上的行输出：

```
app::run()                                     [app/mod.rs:663]
  ├─ ① 串行阻塞：一次性 GROK_SCREEN_MODE → 读配置(两次) → pre-TUI 登录 → redirect_native_stderr
  │     → auth 刷新 → remote settings 预热(join 最长 EARLY_PREFETCH_WAIT = 2s)
  │     → materialize_startup()（最慢一步，含远端恢复 ≤90s）→ 屏模式解析 + 主题预热
  ├─ ② 终端一次性副作用：spawn writer 线程 → init_terminal()      [app/mod.rs:986-1001]
  ├─ ③ bounded_connect（100 ms 切片；leader 失败自动回退 embedded）
  └─ ④ event_loop::run()   ← 首帧 request_presentation()；之后才 Action::LoadSession 灌历史
```

连接失败不是「黑屏挂住」：`bounded_connect`（`app/mod.rs:576-659`）把等待切成 100 ms 片以支持 managed profile 升级预算，超时后走 `restore_terminal(...)` 归还终端再把错误冒出去（`app/mod.rs:1044-1122`），由 `startup_failure` 模块打印可读报告。**加载态由三样东西承担（没有全屏 splash，未找到证据）**：欢迎屏先建一个乐观 home 会话（代码称 *husk*，`app/dispatch/session/lifecycle.rs:431, 855`）让用户不必等后端就有输入焦点；远端恢复 / git 校验进度走 `emit_pre_tui_restore_line` 打在 TUI 接手之前的终端上（`app/session_startup.rs:1267-1276`）；失败才出现专属界面。reader 线程在 typeahead 回放之后才 spawn（`app/event_loop.rs:1612-1625`），保证用户抢在加载期敲的字排在实时按键之前；若当前停在登录 / 信任 / 付费墙屏，这些事件会被丢弃而不是被误答（`:166-177`）。

**`init_terminal()`：一次性副作用清单（`app/mod.rs:1459-1661`，顺序即语义）。**

1. `enable_raw_mode()`（`:1468`）——**最先**，之后任何失败都必须走 teardown；
2. `EnterAlternateScreen`（仅 fullscreen，`:1488-1492`）；
3. `EnableMouseCapture`（**非 minimal** 才开，`:1497-1502`）；minimal 下若检测到「终端把鼠标报告当文本漏出」（JediTerm / Windows）则额外发一次 `MOUSE_TRACKING_RESET`；
4. `EnableFocusChange` + `EnableBracketedPaste` + `cursor::Hide`（`:1503-1508`）；
5. 光标样式三态 `CursorStylePolicy::{Inherit, ForceBlinking, ForceSteady}`（`:1430-1446`）——默认 `Inherit` 不发任何样式转义，避免覆盖用户 shell 的选择；只有强制过才置 `CURSOR_STYLE_FORCED`，teardown 因此知道该不该复位（`:1523`）；
6. `set_panic_hook()` 与 `signal_handler::install(mode)`（`:1528-1529`）——**在 raw mode 之后立刻**安装；
7. 收集「启动期打字」`startup_typeahead`（`:1474-1476, 1530-1535, 1569-1572`）：raw mode 打开后，在 20 ms（有 `VTE_VERSION` 时）/ 0 ms 的静默窗口里排空 stdin；
8. 协商 kitty keyboard protocol（`:1537-1568`）：失败 / 不支持则跳过，成功则 push flags 并记 `set_pushed_kitty_flags`；
9. 建 viewport（`:1573-1648`）：fullscreen 用全屏 Terminal，否则 `Viewport::Inline(rows)`。**minimal 的探针失败会降级**：改用 `Viewport::Inline(rows)`、重新打开鼠标捕获、把有效模式改成 `Inline`，随后 `finish_theme_after_probe` 补上被跳过的主题解析（`:1606-1625`、`:410-418`）。

任何一步出错都走 `.inspect_err` 分支：`emit_terminal_teardown_sequences` + `disable_raw_mode` + `signal_handler::mark_restored()`，**不**走完整 restore（此时还没有 reader / writer 消费者）（`:1650-1655`）。窗口标题也在这里设置，并强制清洗控制字符与截断：`SetTitle` 的字符串是裸放在 OSC 里的，内嵌 BEL / ESC 会提前终结 OSC 并注入任意转义序列（`:1662-1679`）。

**会话恢复：意图解析三层 + 历史重放的三个要点。** 启动意图收敛成 `SessionStartupIntent` → `materialize_startup()` → `MaterializedStartup`：`Resume` / `Fork` / `NewWithId` / `NewAuto`（`app/session_startup.rs:184, 685, 849`；`app/mod.rs:818-824`）。`Resume` 分三层解析（`resolve_existing_session`，`app/session_startup.rs:982`）：本地 id 命中即用 → 标题命中走 `select_by_title`（简单小写比较；多个同名时**唯一的手动 `/rename` 胜出**；仍有歧义则报错并列出 id，**绝不静默挑一个**；UUID 形状的参数永远走 id 路径）→ 都没命中按 `RemoteMissPlan` 决策（`app/session_title_resolve.rs:12-14, 35-76`；`app/session_startup.rs:1086-1113`）。

事件循环在首帧之后才发 `Action::LoadSession`（`app/event_loop.rs:1714-1727`），随后 ACP 回放带 `_meta.isReplay = true` 的通知（`app/acp/meta.rs:29, 45-51`）。三个实现要点：

- **重放全程用「延迟渲染」变体**，避免每条 chunk 都跑一遍 markdown：`is_replay` 走 `push_chunk_to_agent_deferred`（`app/acp/tracker.rs:1132-1136`；`scrollback/blocks/markdown_content.rs:135-142` 的注释写明「batch markdown work and render once after replay completes」）。
- **重放的 thinking 块用另一个构造器** `streaming_replay()` 把 `started_at` 置 `None`，否则本地壁钟计时器会冻成 ~0 ms，渲染出假的 `Thought for 0.0s`（`scrollback/blocks/thinking.rs:152-160`）。
- **`SessionLoaded` 要等回放排空才派发**（`app/session_load_barrier.rs` 整篇就是这件事）：队列头是本会话 replay → 一直等、**不设超时**；头是无关会话的流量 → 只在事件循环有余力 drain 时累计秒表，超过 `SESSION_LOADED_ACP_BARRIER = 2 s` 放行（输入饿死时秒表不走，避免恢复被卡住）；头是本会话的 live → 立即放行，因为 leader 模式保证线序 `[unicast replay] → [load response] → [buffered live]`（`:3-8, 26, 174-180`）。

状态不一致时的处理：标题歧义 → 报错 + 列候选 id；本地找不到且允许远端 → 远端恢复，**只恢复会话状态与记忆，从不恢复代码**；远端超时但已有本地子会话 → 降级为继续用本地会话并明确告知；`--restore-code` 但无 `--worktree` → 拒绝原地检查代码快照（`RejectInPlaceCodeRestore`）；欢迎屏留下的乐观空会话 → `abandon_unused_empty_for_load` 丢弃 husk 但**有草稿就保留**；恢复把上一条「还在跑」的任务行读回来 → 有专门清理函数（`app/session_startup.rs:1102-1112, 1240-1256, 1264-1282`；`app/dispatch/session/lifecycle.rs:825-853`；`app/dispatch/turn.rs:834-835`）。

**退出：单一真源的 teardown 序列。** `app/terminal_restore.rs:61-134` 的注释明说 *「So the on-wire byte order is defined exactly once.」* 它**不含** `disable_raw_mode()`——raw mode 必须留到所有读操作之后才关：

```
OSC 9;4 清除进度条（无条件；终端不支持时是无害空操作）
EndSynchronizedUpdate (?2026l)
reset cursor color（仅本进程真正设置过时）
写 MOUSE_PASTE_RESET 裸序列: ?1000l ?1002l ?1003l ?1015l ?1006l ?2004l
   └ 若 MOUSE_CAPTURE_ENABLED 曾置位：Windows 额外补一次 DisableMouseCapture（winapi）
DisableFocusChange (?1004l)
pop /gboom 额外键盘层（若游戏开过，且它叠在基础层之上）
PopKeyboardEnhancementFlags (CSI<u)          ← 用 swap 保证至多一次
分屏模式收尾：
  fullscreen : [SetCursorStyle::DefaultUserShape 若强制过] → cursor::Show → LeaveAlternateScreen
  inline     : [同上] → MoveTo(0, inline_cursor_row) → cursor::Show → 换行
Windows: restore_stdin_mode()（最外层快照，放最后）
```

这组模式位是**规范清单**，写在 `crates/codegen/xai-crash-handler/src/terminal.rs:6-46`，注释要求「每条 teardown 路径都必须禁用同一组模式」，并给出两个常量：`MOUSE_PASTE_RESET`（`:38-39`）与完整 `RESTORE_SEQ`（`:44-46`）；其中 kitty 的 `CSI<u` pop 排在 `?1049l` **之前**，因为「协议栈是按屏计数的」。**inline 的光标落点是专门的坑**：minimal 视口不贴底，把光标移到屏幕最后一行会让 shell 提示符与续跑提示和 pager 最后一行隔一整屏空白，所以传入的 `inline_cursor_row` 取 `terminal.viewport_area().bottom()` 并被 `min(last)` 夹住；信号路径拿不到 terminal 句柄，只能回落到屏幕底行（`app/terminal_restore.rs:112-127, 163`；`app/signal_handler.rs:229-230`）。

**三条调用路径的差异，以及「所有等待都有界」。**

| 路径 | 入口 | reader join | 排空 writer | fence | 有界保护 |
| --- | --- | --- | --- | --- | --- |
| 事件循环后（正常退出） | `app/mod.rs:1169-1174` 调 `restore_terminal` | 是（250 ms） | 是（2 s） | 是 | writer 超时才用 `run_bounded_teardown` |
| panic hook | `app/terminal_restore.rs:243-258` | 否 | **否**（会死锁） | 否 | 整个 teardown 放 helper 线程 + 2 s 上限 |
| 信号 | `app/signal_handler.rs:219-239` | 否 | 否 | 否 | 无（靠 exit 看门狗兜底） |

`restore_terminal_with`（`app/terminal_restore.rs:143-185`）把顺序固定为：**join reader → fullscreen 下 clear → 抓取 inline 落点 → drain writer → teardown → fence（仍在 raw mode）→ `disable_raw_mode()` → `mark_restored()` → 恢复 fd 2**。`mark_restored()` 之后到达的信号就不会再往用户的提示符上画转义序列（`app/signal_handler.rs:84-88`）。四个有界值：

- **writer join `WRITER_JOIN_GRACE = 2 s`**：超时**不报错**，而是记 `term.writer.join_timeout` 并把线程**脱钩**；后续所有 stderr 写都可能阻塞，所以之后的 teardown 必须走 `run_bounded_teardown(..., 2 s)`，否则「终端不再读」会让 `/quit` 变成永久挂起（`app/terminal_restore.rs:27, 32-46, 138, 169`）。
- **reader join `READER_JOIN_GRACE = 250 ms`**：读线程用 `poll(20 ms)` 再 `read()` 而非裸阻塞读，这样「暂停标志」与「通道关闭」最多一个轮询周期就被观察到；`join_within` **从不 join 一个还在跑的线程**，超时即 detach 并返回 `ReaderJoin::TimedOut`（`app/reader_thread.rs:16, 68-72, 103-123`）。
- **crossterm 解析垃圾**：连续 50 次读错误才退出 reader，否则只 warn（注释点名 ratatui#1275 与 VTE/SSH PTY 的脏输入，`app/reader_thread.rs:81-94`）。
- **exit 看门狗**：优雅退出武装后 20 s 兜底（`DEFAULT_EXIT_TIMEOUT`），超时后再等 5 s（`HARD_EXIT_GRACE`）执行 `libc::_exit`（async-signal-safe 且跳过 atexit）；`GROK_EXIT_TIMEOUT_SECS` 可调、**`0` 表示关闭**，`GROK_TEST_HOLD_TEARDOWN_SECS` 用于测试里故意拖住 teardown（`app/exit_timeout.rs:8, 53-71, 73-79`）。

**最后一道闸门：kitty pop fence（DA1 往返）。** 这是整套设计里最精巧的一环，位于 `crates/codegen/xai-grok-pager-render/src/terminal/pop_fence.rs`。要解决的问题是：终端按顺序处理输出，所以当 `CSI ? c`（DA1）的回复到达时，终端在应用 `CSI < u`（pop）之前发出的**所有按键报告都已经躺在 stdin 里**；不排空它们，这些 key release 事件会被用户 shell 当成击键——注释举的例子是 **fish 把 `ESC [ 100 ; 5 : 3 u` 读成 Ctrl-D 然后退出**。读取用裸 fd，因为 crossterm 会把 DA1 回复从事件流里过滤掉。

```
pop 已写 (CSI < u)
  │  再写 CSI c（DA1 查询，同一 fd）
  ▼
终端输出队列（FIFO）：… [用户先前按下的 key release 全在这里] … [DA1 回复]
  ▼
读到 ESC [ ? … c  ⇒ 证明终端已应用 pop
   ├ 沿途字节 = residue_bytes（非 0 表示拦下了一次泄漏）
   └ 一个 deadline 同时约束「拿锁 + 写 + 读」：终端不再读时也不会拖住退出
```

`TeardownFence::decide()`（`app/teardown_fence.rs:37-48`）的决策表：

| 条件 | 决策 |
| --- | --- |
| writer join 超时 | `SkipWriterWedged`——**优先级最高**，终端不再读，问了也没人答 |
| reader 超时（还有别的 stdin 读者） | `SkipReaderAlive`——回复会被抢走 |
| 本次没 push 过 kitty flags | `SkipNoFlags` |
| 其余 | `Run`（`POP_FENCE_TIMEOUT = 1 s`，注释称取自 neovim 的实战值） |

只要 fence 没真跑或结果是 `Unsupported` / `QueryFailed`，就退回旧式的「静默排空」`drain_pending_events(10 ms)`；结果统一记到 `teardown.kitty_pop_fence`，字段含 `decision` / `outcome` / `residue_bytes` / `reader_join`，用**直写**通道发出（启动失败时 ACP forwarder 没有发送端）（`app/teardown_fence.rs:13-16, 60-63, 78-95`）。

**信号策略。** 模块注释把动机写全了：没有这些处理，SIGINT / SIGTERM / SIGHUP（WSL/SSH 断开）会留下 raw mode + 备用屏 + 鼠标捕获 + kitty flags，终端看起来「还活着」但进程已死（`app/signal_handler.rs:1-16`）。要点：信号在 **tokio task** 里处理，不是真正的异步信号上下文，所以可以安全地走完整 `emit_terminal_teardown_sequences`（含加锁 stderr、条件复位光标样式）；**首个信号 = 优雅退出**（走和 `/exit` 一样的收尾）、**第二个 = 立即强制退出**（`:90-135, 198-209`）；`SIGTTIN` / `SIGTTOU` 显式忽略，避免子进程（或孙进程）`tcsetpgrp()` 抢走前台进程组时整个 pager 被挂起而留下 raw mode 终端（`:75-79`）；**`SIGPIPE` 故意不装处理器**，保持 Rust stdlib 的 `SIG_IGN`——自定义处理器会把 fork+exec 子进程（MCP server、hook）继承的 disposition 从 `SIG_IGN` 改成 `SIG_DFL`，重新引入旧回归（`:12-16`）；退出码与 agent 二进制共享同一函数（Ctrl+C `130`、SIGTERM `143`、SIGHUP `129`，`:137-150`）；顺序上**先还原终端，再 flush Sentry**（最长 2 s），否则用户要盯着一个 raw + 备用屏的终端等 2 秒（`:216-218, 230-231`）；Windows 分支不同（`:103-124`）——Ctrl+C 享受同样的「首次优雅 / 二次强制」，Ctrl+Close / Logoff / Shutdown 则**立即**处理，因为系统只给很短窗口。

**明确的失败模式清单。**

| 失败模式 | 应对 | 证据 |
| --- | --- | --- |
| 终端停止读取输出（SSH 掉线等） | writer join 2 s 超时 → 脱钩；teardown 有界化；最终由 20 s 看门狗 `_exit` | `app/terminal_restore.rs:27, 143-185`；`app/exit_timeout.rs:8` |
| stdin reader 卡在 `read` | 250 ms 有界 join → detach，fence 放弃（`SkipReaderAlive`），退回 10 ms 静默排空 | `app/reader_thread.rs:103-123`；`app/teardown_fence.rs:37-48` |
| panic / panic=abort | hook 在 helper 线程跑有界 teardown（2 s），并主动 `kill_all` 子进程、记录崩溃采样；退出提示等打印全部 best-effort（写失败即忽略） | `app/terminal_restore.rs:243-258`；`app/mod.rs:1236-1238` |
| 重复 teardown（panic + 信号、或双信号） | `TERMINAL_OWNED` 布尔 + kitty flags 的 `swap` + Windows stdin 模式快照的 `swap` 保证至多一次 | `app/signal_handler.rs:60-88, 232-233`；`app/mod.rs:111, 1378-1390` |
| 崩溃路径（连 panic hook 都没跑完） | `xai_crash_handler` 的 `RESTORE_SEQ` 用裸 `write(2)` 写 fd 2 | `crates/codegen/xai-crash-handler/src/terminal.rs:44-60` |
| SIGKILL | **未找到证据**表明有处理；信号任务只订阅 SIGINT / SIGTERM / SIGHUP | `app/signal_handler.rs:90-135` |

**屏幕模式：三种模式、进程内切换与 exec 热重启。** `ScreenMode` 三个变体（`app/mod.rs:340-349`）：`Fullscreen`（经典 alt-screen 全屏 TUI，占备用屏 `?1049h`，鼠标捕获开）、`Inline`（停在主屏、`Viewport::Inline(rows)` 的底部内联视口，不占备用屏，鼠标开）、`Minimal`（实验性 scrollback-native（`--minimal`），终局块用 `insert_before` 打进终端原生回滚，鼠标**关**）。

三者各有稳定 telemetry 标签 `fullscreen` / `inline` / `minimal`（注释警告改名会让外部分析流塌成 `"other"`）；另存全局原子量 `CURRENT_SCREEN_MODE`，因为 panic hook / 信号处理拿不到参数（`app/mod.rs:156-166, 376-382`）。解析链从高到低：`GROK_SCREEN_MODE` 环境变量（一次性、**读完即删**，否则每个被 spawn 的子进程都会继承一个用户从未要求的强制模式）> `--minimal` / `--fullscreen` > `[ui] screen_mode` > legacy `pager.toml [terminal] minimal` > alt-screen 策略（`[terminal] alt_screen` / `--no-alt-screen` / Zellij·tmux control 探测 / 鼠标泄漏终端）（`app/screen_mode_relaunch.rs:17-20, 268-278, 280-319`）。

`/minimal` 与 `/fullscreen`（别名 `/full`）发 `Action::RelaunchInScreenMode`，事件循环里 `run_pending_mode_switch` 的顺序是：park input reader（≤500 ms，失败则 Aborted 并留在原模式）→ writer `wait_drained`（≤750 ms，超时 Aborted）→ `perform_screen_transition()`，结果三选：`Switched`（reseed + 全量重绘 + 一条 system 块 / toast）、`Aborted`（回滚到原屏与原鼠标状态）、`NeedsExecFallback`（→ exec）（`app/event_loop.rs:909-1020`、`app/mode_switch.rs:108-207`）。细节：

- 切到 Minimal 前**先拿掉鼠标捕获**，且 `Clear(All)` 而**不是** `Clear(Purge)`——注释：purge 会毁掉用户真实的回滚缓冲（`app/mode_switch.rs:121-140`）。
- Minimal 分支在设置视口前先 `set_current_screen_mode(Minimal)`，这样「切换中途失败」的 teardown 也知道终端实际在哪块屏（`:141-143`）。
- 成功后给一条可逆确认：minimal 方向打 system 块 `Switched to minimal mode · /fullscreen to go back`，fullscreen 方向打反向 toast（`app/event_loop.rs:947-972`）。
- 切到 minimal 时会**关掉 fullscreen 专属浮层**（block viewer、图片 / 视频查看器、/gboom 游戏等），否则它们会「看不见却仍然吃键」（`app/mode_switch.rs:224-240`）。
- 重绘前要 `reseed_screen_mode`：重绑快捷键注册表、重置 hover、重算 thinking 折叠策略——**启动时按模式决定的东西，切换时都得重播一遍**（`:29-53`）。

exec 热重启只适用三种场景：启动期的 env 覆盖、`GROK_SCREEN_MODE_SWITCH=exec` 逃逸开关、以及不可恢复的切换（`app/screen_mode_relaunch.rs:1-10`）。`build_screen_mode_relaunch_args`（`:59-160`）剥掉一次性与已消费的 flag（`--minimal/--fullscreen`、`--continue/-c`、`--fork-session`、`--restore-code`、`--resume/--load/--session-id/-s/--worktree/--worktree-ref/--ref` 及其 `=` 形式与后续值、裸 positional prompt、`--` 之后的一切），最后追加 `--resume <id>` 与目标模式 flag；**哪些 flag 吃值是从 clap 定义本身推导**的（`clap::CommandFactory`，`:25-54`），防止分类与 CLI 漂移。Unix 用 `cmd.exec()` 保住 PTY/fd 身份（PTY 测试因此能观察切换而不必重启进程）；Windows 没有 `exec`，改为 spawn 子进程 + 父进程 `wait` 并以其退出码退出，同时 `SetConsoleCtrlHandler(None, 1)` 忽略父进程的 Ctrl+C、sleep 150 ms 让父进程 reader 退出，避免和子进程抢控制台输入记录（`:6-8, 211-241`）；失败时打印可粘贴的命令，带 `GROK_SCREEN_MODE` 与显式 flag 双保险（`screen_mode_relaunch_resume_hint`，`:179-187`）。

**rewind 与导出（压缩记录）。** `/rewind`（别名 `/undo`）只截断**对话历史**，**不回滚磁盘上的文件**——手册用加粗的 "Important" 强调这一点；数据来源是每个会话目录里的 `rewind_points.jsonl`。触发有两条：命令本身，或**空闲 + 空输入框 + 有消息时 800 ms 内连按两次 Esc**；切换回合结束后约 1 秒内 rewind 分支被抑制，防止「乱按 Esc 恰好回合结束」意外打开选择器（`docs/user-guide/03-keyboard-shortcuts.md:229-233`、`docs/user-guide/17-sessions.md:35, 143, 158`）。界面状态机有 5 个阶段 `Loading → Picker → (CancelOffer) → Confirm → Executing → Error`（`views/rewind.rs:56-78`），选择器复用 `/jump` 的列表浮层几何；后端契约 `RewindPointInfo` / `RewindResponse` 已为「有文件快照」预留字段（`num_file_snapshots` / `has_file_changes` / `reverted_files` / `conflicts`），但当前 UX 文案仍明确不动文件（`views/rewind.rs:10-54, 359-389`）。`/export` 与 `grok export` 共用同一个纯函数 `render_blocks_to_markdown`（`scrollback/export.rs:13-62`），产物只有 `## User` / `## Assistant` / `## Tools` 三种小节，thinking / 系统 / 子代理 / 额度等「界面外壳」块全部跳过；写失败时**只用通用措辞** `Failed to write file: <e>`，注释明说「不要回显用户给的路径，可能含密钥或 PII」（`app/dispatch/transcript.rs:152-197`）。**share 在当前快照里两条路径都硬编码关闭**（`slash/commands/share.rs:14-17`、`share_cmd.rs:11-13`），因此没有任何「分享链接 / 公开 URL」的产物形态。

### 5.7 宿主集成：通知、标题、OSC 与终端复用器

**本节要点**

- 出站分两条链：**通知走独立逃逸队列**（帧可能被 ack 无限期挂住），**标题与进度随帧**；两者共用同一 WriterThread 队列以保证字节顺序，否则 idle 标题会被还在排队的 busy 标题盖回去（`notifications/mod.rs:96-116, 118-135`、`crates/codegen/xai-grok-pager-render/src/render/draw.rs:241-276`）。
- 桌面通知只有 5 种字节协议（OSC 9 / 99 / 777 / BEL / None），**没有任何系统级通知 API 调用**（无 `libnotify`/D-Bus）；Zellij 被硬编码为 BEL，tmux / screen 不参与协议选择（`notifications/protocol.rs:6-47, 22-24, 129`）。
- 标题是「每 tick 重新合成 + 去重」的部件列表，默认 `[action-required, spinner, activity, session-name, grok]`，` - ` 连接；**tmux 下不做 DCS 包装**，这点与通知 / 进度条 / OSC 11 都不同（`notifications/title.rs:8-20, 59-97, 259-266`）。
- 三类能力探测三种读法：OSC 11 裸查优先、失败再 DCS 包装（500 ms / 80 ms）；DA2 同步读 fd（仅 Alacritty，500 ms）；XTVERSION fire-and-forget，回复由事件循环过滤器识别（`pager-render/src/theme/osc11.rs:25-53`、`terminal/da2.rs:29-70`、`terminal/xtversion.rs:1-89`）。
- OSC 12 **只写不查**，复位必须由「本 session 画过」门控；**XTGETTCAP 不存在**（全仓库 grep `gettcap` 无命中），OSC 12 查询同样不存在（grep `12;?` 无命中）。
- tmux 包装条件逐项不同：通知不看版本、进度条与 OSC 11 要 ≥3.3、OSC 52 另有判定，而 DEC 2026 在 tmux 直接终端时**反而不发**——一刀切必然错一半（`terminal/tmux.rs:38-57`、`notifications/progress.rs:34-38`、`clipboard/mod.rs:136-147`）。

**这一层的定位与两条出站链。** 宿主集成层负责「TUI 之外的一切终端交互」：把 Agent 状态推给窗口标题、标签页、通知中心、进度指示，同时把终端的回答拉回来（能力探测、焦点、尺寸）。通知若内联写 stderr 会死锁——它从事件循环线程发出，而终端停止读取时取 stderr 锁永不返回——所以走非阻塞的 `EscapeWriter` 队列；发送失败记录在共享的 `WriterSync` 上而非返回给调用方（`pager-render/src/render/draw.rs:241-247, 266-276`、`notifications/protocol.rs:97-107`）。标题与 OSC 9;4 则走帧管线，理由写在注释里：idle 标题的转义必须排在「仍带 busy 标题的旧帧」之后（`notifications/mod.rs:118-124`）。

**桌面通知：事件、协议、配置。** 事件枚举有 5 个变体，但**只有 4 个发射点**：

| 事件 | 发射点 | 说明 |
| --- | --- | --- |
| `ApprovalRequired` | `app/acp_handler/permissions.rs:47` | 一批排队的权限请求只发一次（`permission_notified` 抑制），队列清空后复位（`app/app_view.rs:5813-5816`） |
| `TurnComplete` | `app/dispatch/prompt.rs:1675-1684` | 正文 `Turn complete in {duration}.` / `Turn complete.`；`bash_turn` 不发 |
| `AgentError` | 同处 `:1685-1687` | 正文 `Error: {err}`；已展示专用 UX 时不发 |
| `SessionReady` | `app/dispatch/status.rs:517-529` | 标题固定 `"Grok"`，正文为事件名 |
| `TaskComplete` | **未找到发射点** | 存在于配置枚举与文档中，全仓库 grep 无命中 |

事件先过两道门：`config.events` 白名单与 `config.condition`（`notifications/mod.rs:81-91`）。协议是 5 种字节形式，按终端品牌自动选择：`Osc9`（`\x1b]9;{body} · {title}\x07`，iTerm2/WezTerm/Warp）、`Osc99`（`\x1b]99;i=grok;…\x1b\\`，Kitty）、`Osc777`（`\x1b]777;notify;Grok;{body}\x1b\\`，Ghostty/VTE/Terminator/foot）、`Bel`（`\x07`，Apple Terminal/Alacritty/VS Code/Windows Terminal/Unknown）、`None`（Grok Desktop）（`protocol.rs:6-19, 21-47, 58-74`）。两个细节：OSC 9 / 99 是**纯正文协议**，所以把会话名折进正文（` · ` 分隔），OSC 777 的 title 参数固定成 `Grok`，因为标签页标题已承担会话名；**Zellij 被硬编码成 `Bel`**（`select_protocol` 第一行就判），tmux / screen 不参与协议选择——包装发生在发送时刻而非选择时刻（`protocol.rs:22-24, 66-70, 129`）。配置节 `[ui.notifications]` 默认 `method = "auto"`、`condition = "unfocused"`、`idle_threshold_secs = 3`、`events = ["turn_complete", "approval_required"]`、`sleep_prevention = true`、`progress_bar = true`；缺失或畸形 TOML 一律回落默认值，带注释模板由 `to_toml_with_comments()` 生成（`notifications/config.rs:24-42, 139-180`、`notifications/mod.rs:262-269`、`docs/user-guide/05-configuration.md:459-490`）。`condition` 语义：`Always` 恒发、`Never` 永不发、`Unfocused` 只在丢失焦点且超过 `idle_threshold_secs` 后发。另有三个「人不在场」开关：`sleep_prevention`（macOS `IOPMAssertionCreateWithName` + `NoIdleSleepAssertion`，Linux `systemd-inhibit --what=idle … sleep infinity`；平台失败置 `platform_unavailable` 且只试一次）、`session_recap` 与 `session_recap_threshold_secs`（回到终端时的回顾，重试退避 90 s）（`notifications/sleep.rs:37-46, 57-125`、`notifications/focus.rs:5-6, 48-56, 65-85`）。**内容净化是硬要求**：标题与正文都过 `sanitize_osc_text`，剥掉所有控制字符（含 BEL 与 C1 ST），避免模型产出的文本提前终止 OSC/DCS（`protocol.rs:49-55`，测试 `:229-248`）。

**通知 hooks 是待复核项（不要写成已生效）。** `[ui.notifications.hooks]` 可配置任意 shell 命令，通过 `GROK_EVENT` / `GROK_MESSAGE` / `GROK_SESSION_ID` 传参，默认 `only_unfocused = true`、`timeout_secs = 10`；实现相当完整（`sh -c` 启动、`detach_std_command` 脱离控制终端、限制子进程网络、进程组登记、超时后杀进程树、ECHLD 后交给 reaper 线程）。但**未找到生产调用点**：全仓库 grep `run_hook` 除定义外全是该文件的单元测试，`NotificationHook` 除重导出外无消费者，`NotificationService::notify` 也不遍历 `config.hooks`。shell 侧另有一套同名概念 `SessionCommand::DispatchNotificationHook`，由 `xai-grok-shell` 的对话流程派发，是**不同机制**，不能拿来当作 pager 配置项已生效的证据（`notifications/config.rs:116-133`、`notifications/hooks.rs:8-115, 99`、`notifications/mod.rs:19`、`crates/codegen/xai-grok-shell/src/session/commands.rs:748-754`）。

**终端标题。** 标题是「按顺序拼接的部件列表」，默认 `[action-required, spinner, activity, session-name, grok]`，部件间 `" - "` 连接，所有部件都没产出内容时回落成字面量 `grok`（`notifications/config.rs:72-85`、`notifications/title.rs:76-79, 199-203`）。部件行为（`title.rs:110-197`）：`Spinner` 是 8 帧盲文点阵 `⠋⠙⠹⠸⠼⠴⠦⠧`，但**每 8 个 tick 才换一帧**（30 fps 下 ≈264 ms，因为终端尤其 Ghostty 会防抖标签标题）；`ActionRequired` 只在 `has_pending_permissions` 时出现、**未聚焦才闪烁**（15 tick 半周期 ≈500 ms），聚焦时静态显示以消除输入时的抖动；`SessionName` 截断到 40 字符、`Model` / `Cwd` 到 30 字符、`Cwd` 只取路径最后一段、`TurnTimer` 少于 1 秒不显示；超长用 `…` 收尾。更新方式是**轮询式每 tick 比较**，不是按状态变化主动推送：`AppView::update_notifications()` 每 tick 组装一次 `TitleState`（字段就是标题的全部输入：`session_name` / `model` / `activity` / `has_pending_permissions` / `cwd` / `turn_elapsed` / `is_busy` / `focused`）并调 `on_tick()`，只有合成结果与上次不同才返回逃逸序列（去重），结果追加到 `pending_notification_escapes` 随后续帧发出；两个特殊点：`is_busy` 且 `activity` 为空时显示 `Waiting`（首个 chunk 之前的空档），`renders_parked()` 时把 activity / elapsed 视为空（`title.rs:23-35, 81-89`、`app/app_view.rs:5790-5833, 5800-5808`）。转义序列本身由 crossterm `SetTitle` 生成，即 OSC 0；退出时 `shutdown()` 把标题复位成 `grok`（`title.rs:99-106, 259-266`、`notifications/mod.rs:179-183`）。**tmux 下标题不做 DCS 包装**——`build_title_escape` 直接产出 OSC 0；tmux 的差异只体现在部件内容（`Cwd` 取 basename、`SessionName` 截断，都是给窄标签栏留余量）。

**进度条与光标色。** OSC 9;4 在 busy 显示不确定态、idle 清除（`notifications/progress.rs:20-21`）：

```rust
const OSC_INDETERMINATE: &str = "\x1b]9;4;1;-1\x07";
pub(crate) const OSC_CLEAR: &str = "\x1b]9;4;0;0\x07";
```

支持面很窄：仅 Ghostty、WezTerm 与 iTerm2 ≥ 3.6（早期 iTerm2 会把 `9;4` 当成 OSC 9 通知，把原始参数 `4;1;-1` 当正文弹出来）；**保活周期 5 秒**，因为 Ghostty 约 15 秒无输出就会自行复位指示器；tmux 下包装成 DCS passthrough（`progress.rs:10-18, 34-38`、`notifications/mod.rs:14-16, 149-171`）。清退路径**不依赖 `NotificationService`**——信号 / panic 处理器无法访问它，因此 `emit_terminal_teardown_sequences` 无条件直写一次 `OSC_CLEAR`（`app/terminal_restore.rs:60-70`）。光标色 OSC 12 **只写不查**：`cursor_color_escape()` 用主题的 `accent_user` 生成 `\x1b]12;rgb:RR/GG/BB\x07`，无 RGB 强调色（终端原生调色板 / `NO_COLOR`）时返回 `None`；复位用 OSC 112，且**必须**由 `CURSOR_COLOR_APPLIED` 门控——无端复位会让 Ghostty 锁死光标色并停止跟随主题切换，所以只撤销自己画过的东西。单一发射者是事件循环里的跟踪器，只在目标值变化时发 OSC 12 或 OSC 112；启动阶段在 writer 线程存在之前由 `apply_cursor_color()` 内联写一次（`pager-render/src/theme/mod.rs:559-568, 587-622`、`app/event_loop.rs:1971-1986`、`app/mod.rs:1538`）。

**终端能力探测总表。**

| 探测 | 查询 | 读法 | 门控 | 超时 |
| --- | --- | --- | --- | --- |
| OSC 11 背景色 | `\x1b]11;?\x07` | 同步读 fd，裸查询优先，失败再试 DCS 包装 | stdin 是 TTY 即试 | 500 ms（包装版 80 ms） |
| DA2 | `\x1b[>0c` | 同步读 fd | 仅 Alacritty 且非 CSI 拦截型复用器 | 500 ms |
| XTVERSION | `\x1b[>0q` | fire-and-forget；事件循环过滤器识别 DCS 回复 | Unknown/Kitty/WezTerm/Ghostty/iTerm2/Rio 且非 CSI 拦截型复用器 | 过滤器 5 s 武装窗口 |
| 终端版本 | 环境变量 | 纯字符串解析 | 必须由同品牌自证 | — |

`xtversion::gate_allows_probe` 与 `da2::gate_allows_probe` 的名单**必须互斥**，否则两种读法会互吃对方的回车（`terminal/xtversion.rs:81-89`、`terminal/da2.rs:65-70`）。OSC 11 的用途是把「终端画布是亮还是暗」变成一个可用输入，供主题 / 配色判断（`classify_luminance` 用 BT.709 + sRGB 逆传输函数，阈值 0.5）；流程是先发**裸**查询（tmux ≥3.2 会替 pane 作答，3.4 还会在 attach 时快照外层终端颜色），裸查询失败且 `should_wrap_osc11` 成立时才用 DCS 包一层并把超时砍到 80 ms——该包装通常被吞掉，不能让它再吃掉一个完整 500 ms 的启动时间。模块文档明确两条约束：**只在启动阶段调用**（crossterm 的 `EventStream` 一旦活跃就会争抢 stdin）；**编辑器 `:terminal` 内不包装**（libvterm 会把信封当乱码画出来）。回复格式同时支持 `rgb:RRRR/GGGG/BBBB` 与两位缩写（`pager-render/src/theme/osc11.rs:11-14, 22-23, 34-53, 64-74, 76-100`）。DA2 只为 Alacritty 存在（它不导出任何版本环境变量，且原则上拒绝 XTVERSION），回复 `CSI > Pp ; Pv ; Pc c` 的 `Pv` 打包成 `major*10000 + minor*100 + patch`，解析**只接受 Alacritty 的精确形状**（`Pp == 0`、`Pc == 1`、`packed ≤ 999999`），否则宁可不认也不凭空造一个版本号；同步读的代价必须记住——迟到但已开头的回复会被 `probe::finish_after_deadline` 排到安静为止，**一整段都没到时则会留给 crossterm，被当作用户输入打进输入框**，所以 500 ms 是「为慢链路而非静默终端」设的；结果存 `OnceLock`，`TerminalContext::term_version()` 中**运行时探测优先于环境变量**（`terminal/da2.rs:1-13, 10-13, 29-34, 103-133`、`terminal/term_version.rs:61-71`）。它的消费者之一是 kitty 键盘 flags：只在拿到「确定损坏」的库里版本（≤2401，即 0.14.0）时才降级不报 release 事件（`terminal/kitty_keyboard.rs:30-45`）。XTVERSION 承担两项用户可见职责——telemetry 的 `xtversion` 字段，以及**反馈信息里的品牌补全**（品牌为 Unknown 时显示 `Unknown (XTVERSION: kitty 0.35.2)`）；已接受的残留风险写在模块文档里：SSH 来自 JediTerm 时仍会发查询并泄漏，回复若在过滤器 5 s 武装窗口之后才到，会被解析成 `Alt+Shift+P` 加一串字面文本（`terminal/xtversion.rs:1-12, 8-12, 109-117`、`terminal/mod.rs:522, 536-541`）。环境变量侧的公理是**必须自证身份**——「一个版本变量只有在环境同时为它所属品牌作证时才可信」，因为变量会穿过进程、SSH 与复用器边界：优先级 `TERM_PROGRAM_VERSION` → `LC_TERMINAL_VERSION`（仅 `LC_TERMINAL=iTerm2` 时）→ `WEZTERM_VERSION` → `VTE_VERSION`；`ZELLIJ_VERSION` 与 `KONSOLE_VERSION` 一律不读（前者会让 Zellij 里的 Alacritty pane 报 Zellij 的版本号）；VS Code 家族的放宽是单向的（`vscode` 可让 Cursor/Windsurf 借用，反之不行）；tmux ≥3.2 会导出自己的 `TERM_PROGRAM_VERSION`，因此必须靠品牌自证挡掉（`terminal/term_version.rs:1-10, 9-10, 50-59, 80-116`）。

**探测原语的两条安全不变量。** `pager-render/src/terminal/probe.rs` 声明：**必须是 stdin 的唯一读者**（在 crossterm 读线程存在之前，或它 join 之后）；**读窗口里敲的键会被消费并丢弃**，没有可移植的回注手段。写侧 `write_query` 在共享 stderr 锁内写，TTY 检查跑在 dup 出来的 render fd 上而非 `std::io::stderr()`；`write_query_until` 用同一个 deadline 约束**取锁等待 + 写入**，避免卡死的 writer 线程拖住清退。读侧 `read_tty_reply` 逐字节 poll、有 256 字节上限以对付「不发给终止符的终端」，deadline 到期后有 100 ms 宽限 + 25 ms 静默窗口把在途回复读完；`drain_tty_until` 只保留 64 字节尾窗（够装最长的 DA1 回复）、不限制总字节数，因此任意多的按键上报 / 预输入都可以出现在终止符之前。输入源 `tty_input_fd()` 先用 stdin，不是终端就开 `/dev/tty`（读写模式，与 crossterm 的 `tty_fd` 一致），这样 `grok </dev/null` 也能读到回复（`probe.rs:1-7, 12-21, 24-36, 38-50, 160-202, 207-267, 288-335`）。

**`pop_fence` 与 `overlay` 都不是探测机制。** `pop_fence` 是 5.6 讲的 kitty pop 之后的 DA1 清退栅栏；`overlay` 是 kitty / iTerm2 图形协议的**图片覆盖层生命周期管理**：`static`（跨帧保持，按 `owner_id` + 几何位置判定是否重画）与 `volatile`（每帧重画）、`clear`（kitty 按 id 删除；iTerm2 无删除能力但必须复位所有权），以及 `PostFlush`——图片转义在 cell diff 之后写入，并且**只有写入成功才提交所有权**。它服务提示里的图片 / 预览，与能力探测唯一的交集是「这些转义同样在 tmux 下不可用」（`TerminalContext::graphics_protocol_skip_reason` 直接返回 `"tmux"`）（`pager-render/src/terminal/overlay.rs:1-11, 104-110, 160-269`、`terminal/mod.rs:373-379`）。

**tmux / screen 兼容清单。** `MultiplexerKind` 有 6 个取值，识别顺序为 Byobu 显式后端 → `TMUX` → `ZELLIJ`/`ZELLIJ_SESSION_NAME` → `STY`（screen）→ `HERDR_ENV` → cmux 三兄弟 → Undetected；注释解释「嵌套时真实复用器优先」的动因：herdr daemon 从 tmux 里起会把 `TMUX` 冻结进每个 pane，若误判为 herdr 就会把 OSC 52 包进 DCS，而 herdr 会把这段信封当文本打印出来。`intercepts_csi_queries()` 判定 `Tmux | Screen | Zellij | Herdr` 为真——**cmux 不拦 CSI**，所以它的品牌探测照常进行（`terminal/mod.rs:169-197, 777-814`、`terminal/xtversion.rs:169-173`）。DCS passthrough 形如 `\x1bPtmux; … \x1b\\`，其中每个 `0x1b` 写两遍，可用条件是 **tmux ≥3.3**（版本未知按旧版处理）；模块文档把责任边界写清楚：包装前调用方必须已经净化过载荷，helper **只做 ESC 双写**，不剥离 CAN(`0x18`)/SUB(`0x1a`)/C1 ST(`0x9c`)/BEL(`0x07`)——这些字符在 tmux/xterm 下会中止或终止 DCS/OSC（`terminal/tmux.rs:1-4, 9-20, 38-42`）。被包装与被跳过的东西：

| 载荷 | 判定 | 备注 |
| --- | --- | --- |
| 桌面通知（OSC 9/99/777） | `ctx.is_tmux_backed()`，**不看版本** | 与进度条对 tmux <3.3 的行为不同，这是一处不对称 |
| OSC 9;4 进度条 / OSC 11 查询 / OSC 52 剪贴板 | 分别要 `passthrough_available`（≥3.3）、回退路径 `should_wrap_osc11`（可用且非编辑器 `:terminal`）、`osc52_tmux_passthrough`（开了 OSC 52 且 tmux 是直接终端） | `notifications/progress.rs:34-38`、`terminal/tmux.rs:44-48`、`theme/osc11.rs:47-50`、`clipboard/mod.rs:136-147` |
| DEC 2026 同步输出 | tmux **直接终端时整个不发**（tmux 自己整屏重绘并已对外同步）；编辑器 `:terminal` 内仍发 | `terminal/tmux.rs:50-57`、`render/draw.rs:505-529` |
| kitty 键盘协议 / 超链接 OSC 8 | tmux <3.3 或 `extended-keys off` → 跳过 KKP；tmux <3.4 → 跳过 OSC 8。screen 直接判 `kitty_skip_reason = "screen"`；Apple Terminal 视为敌意解析器；VTE <0.50.4 跳过 | `terminal/mod.rs:356-371, 436-468` |
| 行内图形图片 / 备用屏 | tmux 下跳过图片；控制模式或 Zellij 下备用屏自动走 inline | `terminal/mod.rs:373-379, 962-978` |

tmux 的版本、选项与客户端能力走**带外子进程查询**而非转义序列：`tmux -V`、`show-option -gqv <opt>`、`show-option -gv <opt>`（靠 stderr 精确匹配 `invalid option:` / `unknown option:` 区分「不支持该选项」与「tmux 不可用」）、`display-message -p #{client_flags}`（控制模式判定）、`display-message -p #{client_termfeatures}`（24 位色能否在 tmux 中幸存，注释强调**这个值而非 pane 的 `COLORTERM` 才是判据**）；所有查询 2 秒超时、进程组托管、SIGTERM 后 100 ms 宽限升级 SIGKILL，读取失败统一归一到 `None`（**fail-open 为「未知」而不是「否定」**）（`terminal/tmux_probe.rs:4-9, 134-165, 198-200`）。用户可见的修复入口是 `/doctor` 给出的四个可持久 tmux 修复：`set -g set-clipboard on`、`set -wg allow-passthrough on`、`set -g extended-keys on`、`set -as terminal-features ",*:RGB"`；实现上刻意**不**执行 `tmux source-file`、不热改服务器，只改宿主配置文件并保留行尾与权限（`docs/user-guide/21-terminal-support.md:29-47`）。

**焦点与尺寸事件。** 上报由 crossterm `EnableFocusChange` 在启动时一次性打开；到达侧还有一层修复：被拆散的 `\e[I` / `\e[O`（普遍发生在 Shift 处于按下状态时，大写字母被单独解码）会由 CSI 过滤器重组成 `FocusGained`/`FocusLost`，而不是被丢弃（`app/mod.rs:1505`、`app/csi_filter.rs:10, 60-70`）。`Event::FocusGained` 的副作用不止通知：重新发一次鼠标捕获（焦点切换后有的终端会丢掉捕获）、若「外层可能越权重绘 pane」（有编辑器 `:terminal` 或检测到任意复用器）则**强制整屏重绘**、检查会话回顾是否到期并派发 `SendRecap { auto: true }`，随后 `on_focus_gained()` 复位空闲计时与回顾状态；`Event::FocusLost` 只做两件事：`on_focus_lost()`（开始计时、允许新的自动回顾）与释放 gboom 游戏（`app/event_loop.rs:656-660, 3234-3262, 3288-3294`、`terminal/mod.rs:288-293`）。一个容易被忽略的前提：越权重绘修复只在焦点事件真的到达时才有用，而 **tmux 的 `focus-events` 默认关闭**。resize 走普通输入路径（`is_resize` 标记）但有 16 ms 去抖，tick 间隔随需求分三档：动画密集时 16 ms、一般 33 ms、只有低频需求时 83 ms；resize 事件在启动预输入中会被识别并重放，`draw_frame` 每帧先 `terminal.autoresize()`，因此布局跟上尺寸变化不依赖额外的 resize 分支（`app/event_loop.rs:34, 3337, 4128, 4157`、`app/app_view.rs:250-257, 5563-5568`、`pager-render/src/render/draw.rs:510-511`）。

**SSH 与慢链路下的降级。** `is_ssh` 由 `SSH_CONNECTION` / `SSH_TTY` / `SSH_CLIENT` 任一存在判定；降级不是一个集中开关，而是散落各处的一致性偏好：换行键优先宣传 `Alt+Enter`（SSH 或复用器会丢扩展 Enter）；剪贴板 Linux 恒发 OSC 52，macOS/Windows 仅在 tmux/SSH/容器/有 wrap sink 时发；Apple Terminal over SSH 无法到达本地剪贴板，每次复制仍落盘 `~/.grok/last-copy.txt` 并 toast 提示路径；`grok wrap ssh user@host` 在本地 PTY 里转发 OSC 52 并盖上 `LC_GROK_APPEARANCE`，连接断开后恢复终端模式；doctor 对 SSH（及官方 VS Code remote）单独出建议，含 `terminal.ssh-wrap`；启动预输入在 VTE 环境给 20 ms 排空窗口、其余为 0（`terminal/mod.rs:423-430, 837-839`、`clipboard/mod.rs:132-147`、`docs/user-guide/21-terminal-support.md:141-159`、`app/diagnostics/mod.rs:411-415`、`app/mod.rs:1533-1537`）。需要明确的是：**本快照里没有按网络延迟自适应帧率或降低绘制频率的逻辑**——帧率分档只依据「有什么动画在跑」，全仓库 grep `slow_link` 只命中 DA2 的那句注释；慢链路唯一专门的设计是 DA2 的 500 ms 超时取值。若 jev-harness 需要真正的慢链路降级，这一层需要自己设计。

### 5.8 无障碍、自适应与终端能力协商

**本节要点**

- 这一层是**探测 → 分类 → 降级**两段式：启动时把环境变量与探测回复压成 `TerminalContext` 上的能力位，各功能只读枚举，**业务代码从不 `match` 环境变量**（`pager-render/src/terminal/mod.rs:225-265`）。
- `TerminalContext` 同时保留乐观的 `brand` 与原始的 `env_brand`，所有「必须默认拒绝」的判断都回查后者（原生 Windows 会把 `Unknown` 乐观当成 `WindowsTerminal`）（`terminal/mod.rs:228-232, 725-730`）。
- 高度自适应有三级互相咬合的阈值，而且**自动 compact 从不回写配置或磁盘**，窗口拉高后用户选择立刻恢复；整屏布局只有一个 `Min`，就是 `SCROLLBACK_MIN_ROWS = 5`，所以行数不够时被压的是别的行而不是 scrollback（`views/agent.rs:89-104, 123`）。
- 低色彩能力下**不是简单降级而是重做配色**：ANSI 16 专用替代配色按极性选语义色、以 `high_contrast_fg` 承载焦点与正文；Windows 另有量化前的对比度提升。但**不存在色盲（CVD）专用方案**，`color_support.rs` 的 16 色映射是能力降级而非无障碍配色（`pager-render/src/theme/mod.rs:325-334, 367-404, 429-560`）。
- 无障碍开关的真实清单全是配置项与一个环境变量；**没有全局「减少动效」开关**，动画节奏是硬编码常量（`views/turn_status.rs:31/36/40`、`tips/ephemeral.rs:11`）。
- 四个输入过滤器解决「终端应答污染输入流」的不同侧面，都是**兄弟模块**且互相交叉引用；其中 X10 重组的判据是**魔数形状而非坐标越界**，这是大终端上踩过坑后的修正。

**能力识别：探测优先级本身就是可靠性排序。** `detect_terminal_brand_from_env` 的顺序（`terminal/mod.rs:621-723`）：fork 专属标记（`CURSOR_TRACE_ID`、`VSCODE_GIT_ASKPASS_MAIN`，它们即使在 SSH / tmux 下也存活，而 `TERM_PROGRAM` 不一定）→ `TERM_PROGRAM`（最可靠的单信号）→ `TERMINAL_EMULATOR`（JetBrains / JediTerm，必须排在 `TERM_SESSION_ID` 之前，否则会误判成 Apple Terminal）→ 品牌专属变量（`WEZTERM_VERSION` → `ITERM_SESSION_ID` / `LC_TERMINAL`（官方注释指出 `LC_TERMINAL` 能穿过 SSH 的 `SendEnv/AcceptEnv LC_*`）→ `TERM_SESSION_ID` → `KITTY_WINDOW_ID` → `ALACRITTY_SOCKET` → `TERM` 精确匹配 `rio` / `foot`）→ `TERMINATOR_UUID`（必须先于 VTE 兜底）→ `VTE_VERSION` 兜底为 `Vte` → `WT_SESSION` → 否则 `Unknown`。文档明示 15 类**被识别**的终端，代码里的 `TerminalName` 却是 20 个变体（多出 `Terminator`、`Otty` 与 `Unknown`），并配一组分类谓词：`is_vte_based()` / `is_vscode_family()` / `is_xtermjs_embed()` 把「同一类怪癖」归组，`is_capability_unclassified()`（`Unknown | Otty`）共享 fail-closed 姿态，`supports_osc52_clipboard()` **默认 false**、只有显式列出的品牌才是 true，`delivers_ime_as_bracketed_paste()` 只有 Otty 为 true（`terminal/mod.rs:58-160`、`docs/user-guide/21-terminal-support.md:51-76`）。官方同时列出三条识别限制：tmux 内可能拿不到外层终端变量；SSH 不转发多数终端变量；tmux 的 global environment 反映**第一个**接入的客户端，未必是当前客户端。

**行为差异矩阵（按「代码依据的怪癖」列，而不是按品牌罗列）。**

| 终端 / 环境 | 依据 | 行为差异 |
| --- | --- | --- |
| Apple Terminal | `keyboard.rs:102-104`、`terminal/mod.rs:138-154` | `Cmd+Bsp` 被窗口管理器截获（`Unrecoverable`），`Opt` 与修饰 Enter 为 `Dropped` 但可被 CoreGraphics 抢救；不支持 OSC 52，远端复制退回 `~/.grok/last-copy.txt`；用 `Ctrl+O` 代替 `Ctrl+Enter` 插话（doc 21:142-147, 202-203） |
| VS Code / Cursor / Windsurf / Zed | `terminal/mod.rs:318-324, 402-410` | **从不协商** Kitty 键盘协议（xterm.js 对 shifted 可打印键编码有误，issue 5823），`Shift+Enter` 与 `Enter` 同码，UI 改推 `Alt+Enter` |
| Kitty / Ghostty / foot / WezTerm / Alacritty / Rio | `keyboard.rs:88-100`、`terminal/mod.rs:81-84`、doc 21:197-203 | Kitty 系修饰键 `Native` 无需抢救并纳入 truecolor 品牌白名单；Alacritty / Rio 的 `Cmd` 原生（映射到 `^U`）、`Opt` 被丢弃但可抢救；WezTerm 出厂关闭 Kitty 键盘协议，`/doctor` 给 `terminal.wezterm-kitty` 设置项 |
| Warp | `terminal/image.rs:85-95` | 接受部分 Kitty graphics 转义但不支持 placement/scrollback 模型，会留残影，故不启用内联图像 |
| JetBrains（JediTerm） | `terminal/mod.rs:85-89, 736-738`、`keyboard.rs:108-110` | 无 `TERM_FEATURES`、XTVERSION 会泄漏、DA1 只是裸 VT102，**无法运行时探测**，能力全 `Unknown`；Windows 上鼠标上报会漏成原始文本，该组合**默认进 minimal 模式**（`app/mod.rs:939-953`） |
| VTE 系 / Terminator | `terminal/mod.rs:388-400` | `VTE_VERSION < 8200` 或无法解析版本时判定 `Shift+Enter` 不可用 |
| Windows Terminal / 旧版 ConHost | `color_support.rs:266-268`、`glyphs.rs:1-11, 148-159, 377-391` | 原生 Windows 默认假定 ConHost 已开 VT 处理、直接认定 truecolor；旧 ConHost 字形降级为 ASCII / CP437（`❯`→`> `、`✗`→`x`、braille spinner→`|/-\`），有 `GROK_FORCE_LEGACY_CONSOLE` 强制开关 |
| Zellij / tmux / GNU screen / Byobu+screen | `terminal/mod.rs:194-197, 356-372, 374-379, 968-971`、`terminal/tmux.rs:55-57`、doc 21:260-263 | 都拦 CSI 查询、使外围探测失效；Zellij 自动关备用屏退回 inline；tmux 不进同步输出、禁用内联图像、`<3.3` 或 `extended-keys = off` 时 `Shift+Enter` 退化；screen 直接 `kitty_skip_reason = "screen"`；Byobu-screen 支持有限，`/doctor` 报 `terminal.byobu-screen` 并建议切 tmux 后端 |
| Otty / Unknown / 原生 Windows（ConPTY） | `terminal/mod.rs:156-159, 340-348, 414-416`、`terminal/image.rs:141-144` | Otty 是唯一把 IME 提交包成 bracketed paste 的品牌；`Unknown` 无正证据即跳过 KKP，且 `/doctor` 报通知协议回退到响铃；ConPTY 会剥掉 Kitty / iTerm2 的 APC 转义，故内联图像协议返回 `None` |

备用屏是独立的一个轴：`AltScreenMode { Auto, Always, Never }`，`--no-alt-screen` 优先级最高，`Auto` 在 Zellij 与 tmux control mode 下**自动退回 inline**（`terminal/mod.rs:898-908, 955-983`）。

**尺寸自适应：三级阈值 + 一条不回写约束。** 阈值全部集中在 `views/agent.rs:89-98`，并用编译期断言把关系钉死：

```rust
pub const SHORT_TERMINAL_ROWS: u16 = 16;      // ≤16：深降级区，砍掉提示区可选行
pub const SCROLLBACK_MIN_ROWS: u16 = 5;       // scrollback 的地板（整屏布局里唯一的 Min）
pub const AUTO_COMPACT_MAX_ROWS: u16 = 20;    // ≤20：强制 compact 渲染
const _: () = assert!(SHORT_TERMINAL_ROWS < AUTO_COMPACT_MAX_ROWS);
// 行数  0    ──16──     ──20──     ──28──     ∞
//       深降级区 │ 自动 compact │ 小屏提示区 │ 默认布局
```

- **≤16**：底部垂直 padding 归零、插件 CTA 行归零、follow-up 建议行归零（`views/agent.rs:205-217`）；prompt 上方 gap 归零、面板 dock 关闭（`app/agent_view/render.rs:1010-1013, 1076-1084`）；ephemeral tip 行**不再渲染**（`tips/ephemeral.rs:13-16`）——设计意图是保证 prompt 与 scrollback 不被饿死。
- **≤20**：渲染层收到 `compact = true` 走更密的 padding；自动判定是纯函数且**结果从不回写** `current_ui.compact_mode`、渲染缓存或磁盘；`terminal_rows == 0` 表示「还没测量」，永不强制 compact（`views/agent.rs:99-104`）。
- **21–28**：小屏提示带。`SMALL_SCREEN_TIP_MAX_ROWS = 28`，提示文案 `Tight on space? Try /compact-mode`，会话内**最多显示一次**（`SMALL_SCREEN_TIP_SEEN_CAP = 1`）且标记为 `ambient()`——提交 prompt 不会让它消失，被遮挡时 TTL 暂停计时；显示时机是**首次稳定绘制**，不在后续 resize 时重放（`tips/small_screen.rs:1-4, 20-49`）。

**宽度没有视图级阈值**，自适应是逐组件的：模态窗口有 `min_width` 地板，各调用方取值 40 / 44 / 50 / 60，地板值最后会被**夹回缓冲区**并有回归测试；宽度不足以渲染时返回 0 / 跳过绘制并清掉悬停与拖拽状态，防止上一帧的陈旧矩形继续吃点击；仪表板头部在放不下 cwd + 提示时**先丢提示再截断路径**（`views/modal_window.rs:96, 559, 579-582, 1308`、`views/dashboard/chrome.rs:259` 附近测试）。**没有「终端过小就拒绝渲染并报错」的路径**——整屏层面只有 `SCROLLBACK_MIN_ROWS = 5` 这一个地板。

**色彩：四级能力、量化管线、以及两处专门的对比增强。** `ColorLevel` 是有序枚举 `None < Basic(ANSI 16) < Ansi256 < TrueColor`，检测优先级：**`NO_COLOR` 直接判 `None`**（注释写明「显式退出优先」）→ 非 TTY 默认 TrueColor（理由：「TUI 总归跑在终端里」）→ `supports-color` crate 读 stdout 映射四档 → 若 crate 只报 256 色但品牌已知支持 24-bit 则**升级为 TrueColor**（注释解释 tmux/SSH/mosh 常把 `COLORTERM` 剥掉，不补这一步会让主题被量化到 16 色，背景/边框/弱化色的层次全塌在一起）→ 终端原生锁定（minimal 模式或选了 `terminal` 主题）把上限**压到 `Basic`**（`pager-render/src/theme/color_support.rs:1-4, 17-44, 83-127, 243-245`、`theme/cache.rs:105-120`）。另有一条独立路径 `standalone()` 专供 `grok doctor --json`：它**不看 stdout**（doctor 常被管道接走），只看 stderr 是否为终端或 `/dev/tty`（Windows `CONIN$`）能否打开，`TERM` 为空或 `dumb` 时老实回答「无色彩证据」（`:138-203`）。降级管线 `quantize_color`：TrueColor 原样；Ansi256 走 `nearest_indexed`；Basic 把 RGB/Indexed 经 `indexed_to_ansi16`（0–15 一一对应，16–255 先转 RGB 再按**平方欧氏距离**找最近 ANSI 16）；None 全部压成 `Color::Reset`——最后这点在主题层被显式防住：必须用 `has_color()` 判断，否则「named-ANSI 重绘会把 `Reset` 又染回去，使 `NO_COLOR` 失效」（`:220-234, 274-336`、`theme/mod.rs:324`）。主题可用性随档位变化：非 truecolor 时主题选择器**隐藏** TokyoNight / RosePineMoon / OscuraMidnight（染色背景量化后会失去特征），默认主题 GrokNight 则声明「在 256 色与 16 色终端上都能干净地量化」（`docs/user-guide/06-theming.md:13, 131`）。

对比增强是本专题最重要的正面发现——**不是单纯降级，而是针对低色彩能力重做配色**：**(a) Windows 对比度提升**在量化**之前**采样主题明暗极性，再用 `push_away` 把一批弱对比色推离背景（`user_block_push` 暗色 28 / 亮色 8，`gray_dim` 40、`prompt_border` 40、`prompt_border_active` 60，按通道推并夹在 0–255）；**(b) ANSI 16 专用替代配色**在 `Basic` 档或旧 ConHost 未达 truecolor 时启用：以 `Black`/`White` 作画布底、**升一级**用 `DarkGray`/`Gray` 表达「抬升面」（注释承认「在 ANSI 16 里这比 truecolor 设计读起来更响，但在包括博物馆级 `TERM=ansi` 在内的任何 16 色终端上都保证可见」），语义色按极性选（暗底用 `LightRed`/`LightGreen` 等亮变体、亮底用常规变体，理由是常规 ANSI 色 idx 1–7 约 50% 亮度、亮变体 idx 9–15 全饱和），`high_contrast_fg` 取 `White`/`Black` 专门用于 `prompt_border_active`、`text_primary`、`md_text`、`accent_user` 等焦点与正文位，并有测试断言 `scrollbar_thumb_contrasts_with_track_in_all_themes`（`theme/mod.rs:325-334, 367-404, 429-560, 1021`）。

**色盲：零命中。** 在 `crates/` 全树检索 `colorblind` / `color_blind` / `colour_blind` / `deuteran` / `protan` / `cvd` / `daltoni` **零命中**：没有 CVD 专用调色板，没有红绿之外的冗余编码开关。唯一与「不靠颜色分辨」相关的是 diff 渲染的退化路径——当主题的 `diff_*_bg` 为 `Reset` 时变更行改用**整行红/绿前景色**而非色带，这仍依赖色调，不构成 CVD 方案。ANSI 16 替代配色客观上提高了对比度与极性正确性，但它的设计目标写在注释里是「保证可见」，不是色觉障碍适配。**合稿时不要把能力降级写成无障碍配色。**

**无障碍开关全清单（唯一的完整入口，别处不再重复）。**

| 开关 | 取值 | 作用 | 出处 |
| --- | --- | --- | --- |
| `NO_COLOR` | 环境变量，存在即生效 | 强制 `ColorLevel::None`，全部颜色映射为 `Reset` | `color_support.rs:99, 178`；doc 06:119 |
| `ui.cursor_blink` | 三态：未设 / `true` / `false` | 未设 = `Inherit`（不发任何样式转义，保留终端自身设置）；`true` = 闪烁块、`false` = 常亮块。注释说明强制设置曾导致闪烁问题 | `app/mod.rs:1427-1446`；doc 26 `ui` 表 |
| `ui.screen_mode` / `ui.compact_mode` + `/compact-mode` | `fullscreen`/`minimal`；boolean / 命令 | 渲染模式（重启生效，minimal 为键盘优先、终端持有历史）与密度（小屏提示推荐 compact） | doc 26；`app/mod.rs:399-411`、`tips/small_screen.rs:41-47` |
| `ui.mouse_reporting_toggle` + `Ctrl+R` + `/toggle-mouse-reporting` | boolean | 关闭鼠标捕获，解决鼠标上报坏掉 / 干扰。`Ctrl+R` 只在 scrollback 聚焦时可用，prompt 侧走斜杠命令 | `app/mod.rs:182-187, 329`；doc 26 |
| `ui.simple_mode` / `ui.vim_mode` / `ui.show_thinking_blocks` | boolean | 输入编辑与导航范式（readline ↔ vim）、是否显示推理块 | doc 05:71-72、doc 03:9-29、doc 26 |
| `ui.invert_scroll` / `scroll_lines` / `scroll_speed` / `scroll_mode`、`ui.display_refresh.auto_cadence_enabled` | bool / int / enum | 滚动方向、步长、速度、输入分类；刷新节奏匹配显示器（自适应，**不是**减少动效） | doc 26；探测实现 `host/display_refresh.rs` |
| `[scrollback.display] rtl_bidi` | boolean，默认 false | 开启应用侧 RTL 重排 | doc 21:274-277；`render/bidi.rs:19-30` |
| `[ui.notifications] method` / `condition` | `auto`/`osc9`/`osc99`/`osc777`/`bel`/`none`；`unfocused`/`always`/`never` | 通知协议与时机；**`none` 是唯一的「静音」入口** | `notifications/config.rs:52-68` |
| `[ui.contextual_hints].*` | boolean 逐项 | 关闭各类上下文提示（`small_screen`、`ssh_wrap`、`image_input`、`plan_mode`、`send_now`、`undo`、`word_select`） | doc 26 |
| `GROK_CLIPBOARD_NO_OSC52` / `GROK_CLIPBOARD_NO_DATA_CONTROL` / `GROK_FORCE_LEGACY_CONSOLE` | 环境变量 | 关掉 OSC 52 粘贴路由、关掉 Wayland data-control 路由（改用命令行工具）、强制旧控制台字形降级（ASCII / CP437） | doc 21:111-120；`glyphs.rs:389-391` |

**明确未找到证据的三项**：**全局减少动效 / reduce motion 开关**（无：spinner 每 `SPINNER_DIVISOR = 4` 个 tick 换帧、idle 监视脉冲 `MONITOR_PULSE_DIVISOR = 8`、「等待你」菱形脉冲 `USER_WAITING_PULSE_SPEED = 0.08`、tip TTL `DEFAULT_TIP_TICKS = 90`，都是硬编码常量，`ui.cursor_blink` 是本仓库唯一与动效相关的无障碍开关）；**静音 / 响铃开关（通知协议之外）**（未见主动写 BEL 的代码，唯一相关的是**被动回退**：终端未识别时通知协议落到 `NotificationProtocol::Bel`，`/doctor` 报 `WarningCategory::NotificationProtocolFallback`，文案「Grok is using the terminal bell because the terminal was not recognized」，并提示把 `method` 改成终端支持的协议或 `none`）；**高对比模式 / 字体缩放 / 屏幕阅读器支持**（`screen_reader`、`screenreader`、`aria`、`a11y`、`accessib` 在 Rust 源码中零命中）（`views/turn_status.rs:31/36/40`、`tips/ephemeral.rs:11`、`diagnostics/mod.rs:106, 522-537`）。

**输入过滤层：四个过滤器在防什么。** 终端不只是键盘输入源：它会主动往 stdin 塞 (1) 对应用提问的**应答**（DA2、XTVERSION 的 DCS 回复），(2) 应用自己开启的**上报**（SGR 鼠标、焦点、bracketed paste）。crossterm 按自己的语法把这些字节解码成 `Key` 事件，于是「终端应答」会伪装成用户按键混进事件流，而在特定坐标下**鼠标上报本身也会被解码错**。四个过滤器各守一面，且彼此是**兄弟模块**、注释里显式交叉引用：`XtversionFilter`（`app/xt_filter.rs`）吞 XTVERSION 的 DCS 应答、`CsiFragmentFilter`（`app/csi_filter.rs`）重组 SGR 鼠标与焦点片段、`X10ReassemblyFilter`（`app/x10_filter.rs`）重组被错解的 X10 鼠标坐标，三者都在事件循环之前；`Osc52Filter`（`app/wrap_filter.rs`）与 `ModeTracker`（`app/wrap_restore.rs`）则在 `grok wrap` 的**输出侧**（子进程 PTY 输出与 stdout 之间）工作。

- **`X10ReassemblyFilter`**：当坐标字节恰好构成 UTF-8 多字节序列时，解码器会把两个字节拆成「列 = lead 字节 − 33，行 = continuation − 33」，于是 `0xC2`/`0xC3`（161/162）配 `0x80..0xBF`（95..158）成为**魔数形状**：`matches!(m.column, 161 | 162) && (95..=158).contains(&m.row)`。命中后扣住事件，等紧随其后「被挤出的行字节」（以普通 `Char` 或 `Backspace` 到达），两者必须在一个批次内相邻到达，超时上限 `MAX_COMPLETION_GAP = 50 ms`，超时后的真实击键原样放行；`reconstruct` 反解真实列，行号取被挤出字节减 33。重要演进：早期用**坐标是否越界**判断，结果在 180×120 这样的大终端上「错解形状恰好落在边界内」而漏过，现在**形状即判据**、与终端尺寸无关（`app/x10_filter.rs:11-13, 28, 80-82, 87-118, 218-228, 262`）。
- **`CsiFragmentFilter`**：状态机重组被拆成片段的两类 CSI——SGR 鼠标 `[<d;d;d{M,m}` 与焦点上报 `[I`/`[O`。最有价值的是**焦点上报的翻译**：裸 `\e` 与 `[I`/`[O` 落在同一批次时撤回那个裸 Esc，改吐 `Event::FocusGained` / `FocusLost`，这样「焦点驱动的功能（prompt 重新聚焦、recap 离开计时器、`/gboom` 松键）在 SSH 下仍然可用」。代价与边界写得很诚实：单个 `[` 不能跨批次持有（否则用户打的 `[` 会卡住不显示），所以**上一批次孤立了 `\e` 的焦点上报仍会漏过**；类型击键有防护——只有 `\e[` 前缀才进状态机，`arr[I]` 这类文本正常通过（`app/csi_filter.rs:9-11, 57-81, 109-159`）。
- **`XtversionFilter`**：吞掉终端对 XTVERSION 的 DCS 应答，因为 crossterm 会把 `ESC P` 呈现为 `Alt+Shift+P`、payload 呈现为一串 `Char`、ST 呈现为 `Alt+\`、BEL 呈现为 `Ctrl+G`。四个时间常量各有理由：`XT_ARM_WINDOW = 5 s`（对标 opentui 的非阻塞 5 s 窗口，解除后零开销）、`XT_FRAGMENT_TIMEOUT = 150 ms`（其他 TUI 栈用的值）、`XT_MAX_HOLD = 1 s`（总持有上限，防终端「细水长流」地吐合法 payload 字符拖死事件循环）、`XT_MAX_PAYLOAD = 64`（真实应答很短，示例 `kitty 0.35.2`）。两个可借鉴的细节：armed 状态来自 `reply_pending()`，且 `deadline` 是**首次 `filter()` 调用时**才设置而不是构造时——「启动加载可能耗时数秒，那段时间不能烧掉 armed 窗口」；一旦 intro（`ESC P` 之后的部分）被确认就不再中途过期，因为在途应答必须走完，否则尾巴会作为用户输入漏出去（`app/xt_filter.rs:10-28, 55, 124-126`）。
- **`Osc52Filter`**：只服务 `grok wrap`，位置在**子进程 PTY 输出与 stdout 之间**（输出侧，不是输入侧）。职责是拦截 OSC 52 剪贴板序列（含 tmux DCS passthrough 形式）与私有 host-image 请求 OSC，其余全部**逐字节透传**，同时把每个 CSI 报告给 wrap 模式追踪器。三个容量常量都注明推导关系：`MAX_ESC_BUFFER = 1 MiB`（要容纳 768 KiB payload 的 base64（~1.33×）加转义外壳）、`MAX_CSI_BUFFER = 128`（单条 DECSET 列出全部被跟踪模式约 69 字节，有单测钉住关系）、`MAX_CLIPBOARD_PAYLOAD = 768 KiB`（与前者对齐；SSH 场景下不现实有更大的剪贴板）。base64 解码用 `DecodePaddingMode::Indifferent`，理由是「现实中（含部分 Go 实现与终端）会省略 `=` 填充」；另有一条约束：被子进程 EOF 截断的 CSI **故意不冲刷**，因为「发出半开的 CSI 会让真实终端解析器停在序列中间，吃掉退出路径紧接着写的恢复字节」（`app/wrap_filter.rs:1-11, 13-38, 51-55`）。

`grok wrap` 侧还有一个 `ModeTracker`（`app/wrap_restore.rs`）：它区分不了子进程正常退出与连接中断（SSH 传输死亡到达时只是一个普通 PTY EOF 加退出码），能知道的是子进程在本地终端上**开了哪些 DEC 私有模式却没关**。跟踪 14 个模式位，除常规鼠标编码（`?1000/1002/1003/1005/1015/1016`）、`?2004`、`?1004`、`?2026` 外，还包括**老式备用屏** `?47`、`?1047`、`?1049`；`?25` 是**反向**跟踪的（DECTCEM 的 set 侧是「显示光标」，需要修复的态是「见过 `?25l` 而之后没有 `?25h`」）。恢复字节按 `DISABLE_ORDER` 发出，顺序与 `xai_crash_handler::terminal` 的 `RESTORE_SEQ` 用单测钉在一起。已知局限也老实记录：Kitty 协议每个屏幕缓冲区各自维护一层栈，而这里只有一个净深度计数，所以「在一个屏幕 push、在另一个屏幕死掉」的子进程是按计数而非按屏幕恢复的（`:1-8, 19-50`）。

**CJK、RTL 与组合字符。** 宽度计算统一走 `unicode-width` 的显示宽度而不是字符数：截断函数按显示列截断（CJK 计 2）；一个具体的坑是 `format!` 的 `{:<width$}` 按**字符数**补齐，遇到 CJK 这类宽字形后列会错位，所以另写了对齐助手并有测试 `print_table_pads_cjk_labels_by_display_width` 钉住；无关字形（em dash、CJK）在字形替换模块中**原样透传**（`pager-render/src/util.rs:203, 232`、`worktree_cmd/display.rs:233-235`、`glyphs.rs:569`）。组合字符与字素簇当作原子：编辑器侧测试直接以 `["e\u{301}", "👩🏽\u{200d}💻", "🇺🇸", "界"]` 为用例，断言光标移动与删除**从不切开字素簇**，换行按显示宽度而非字节，行高会算上双宽 emoji（`xai-ratatui-textarea/src/editor_tests/editing.rs:35-40, 55-67, 84-104`、`wrapping.rs:512, 588-589`）。**未找到证据**：没有基于 `LANG` / `LC_ALL` / `LC_CTYPE` 的字符集能力探测或 ASCII 降级路径——`host::collect_unicode_env` 名字容易误导，它只是把环境变量收集成 UTF-8 map；字形降级只有一个触发条件即旧版 Windows ConHost。IME 只有一处显式处理（Otty 的 `delivers_ime_as_bracketed_paste()`），未见预编辑渲染、光标定位或组合态 UI（`terminal/mod.rs:156-159`、`terminal/test.rs:55-70`）。

**RTL 默认关闭，理由明确：** `render/bidi.rs` 的模块头一句话讲清了定位——*Off by default. Many terminals already run implicit bidi; reordering in the app double-flips text on those hosts.* 开启后（`[scrollback.display] rtl_bidi = true`，进程级原子门闩、随 appearance 配置加载生效无需重启）只对含强 RTL 或阿拉伯数字字符的文本重排（`BidiClass::R | AL | AN`），剥离 bidi override / isolate / embedding 控制符但**不含 ZWJ/ZWNJ**，`is_table_row` 的行直接跳过以保持列对齐，RTL run 以字素簇为单位反转并**镜像配对标点**，基方向**逐行**而非按段落判定。配套成本最高的是命中测试的双向映射：`logical_slice_for_visual_cols` 由视觉列区间取回逻辑文本、`visual_col_to_logical_col` 是绘制重排的逆运算，保证搜索高亮、选择 / 拖拽复制、双击选词选 URL、链接命中区域都落在正确字形上，而粘贴回剪贴板仍是逻辑序；适用范围限于 scrollback、列表内容与全屏块查看器，**chrome、下拉、模态保持逻辑序**以便命中测试一致，文件名末尾也承认 *This is not a full mirrored RTL UI*（`render/bidi.rs:1-9, 19-31, 33-56, 74-101, 164-166, 199-320, 494-495`、`docs/user-guide/21-terminal-support.md:266-297`）。

**键盘可访问性是真实的工程投入，但没有形式化保证。** 代码层存在「键盘专用宿主」这一等公民概念——`views/turn_status.rs` 用 `Option` 表达鼠标可用性而不是布尔分支：`None` 表示 keyboard-only host（minimal 模式、无鼠标捕获），此时**同时**抑制按钮与悬停状态上报、`[stop]` 不显示、**连命中矩形都不生成**，minimal 渲染复用同一 widget 并明确以 `None` 传入（`views/turn_status.rs:61, 66, 212, 298, 1217`、`minimal/live.rs:526-527`）；模式切换时也要重置 `app.last_mouse_pos = None`，否则「Minimal 关掉鼠标捕获后没有移动事件，上一帧的悬停会粘住」（`app/mode_switch.rs:38-40`）。四类阻塞式界面（question 卡、MCP elicitation 卡、权限提示、取消轮次面板）共享一份显式契约：`Tab`/`Shift+Tab` 只在**本卡内部**行走且两端环绕、**永不把焦点挪出去**；`Esc` 逐级退出（先清卡片待处理项，清无可清才离开），question 卡与权限提示把键盘**停在 scrollback**（卡片留在屏上）以便回看上下文；多卡同时打开时优先级为权限提示 > 取消面板 > question > elicitation，快捷键栏始终显示**当前真正接收按键**的那张卡（doc 03:109-125）。鼠标动作的键盘等价逐项对照见 doc 03 的快捷键表（滚轮 ↔ `Ctrl+K`/`Ctrl+J`/`PageUp`/`PageDown`/`Ctrl+U`/`Ctrl+D`；折叠 ↔ `h`/`l`/`e`/`⇧E`；复制块 ↔ `y`/`⇧Y`；模态关闭 ↔ `Esc`/`F2`/`Ctrl+,`，注释明写「X 按钮的点击取消与这几条键盘路径镜像」），`PageUp`/`PageDown` 在 prompt 聚焦时也能滚动会话且**不移动焦点、不改草稿**，但 prompt 历史 / `@` 文件搜索 / 斜杠菜单 / 补全下拉打开时按键优先归它们（doc 03:41-110, 412-421、`app/modals.rs:1588`）。**未找到证据**：没有任何文档或穷举测试声明「每个鼠标动作都有键盘等价」这条不变量——上面的对照是从快捷键表与代码分支反推出来的，覆盖操作类点击，但不构成形式化保证。
