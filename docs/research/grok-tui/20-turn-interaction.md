# 专题 20：Agent 轮次交互：流式、插话、队列与取消

> 专题：Agent 轮次交互——流式、插话、队列与取消｜调研日期：2026-09-22｜证据范围：`crates/codegen/xai-grok-pager/src/app/{prompt_ack.rs, cancel_latency.rs, turn_completion.rs(+tests), deferred_subagent_finishes.rs, queue_edit.rs, agent_view/*, dispatch/*, actions.rs}`、`crates/codegen/xai-grok-pager/src/{views/turn_status.rs, views/modal.rs, views/prompt_widget/*, acp/tracker.rs, notifications/*}`、`crates/codegen/xai-grok-pager/src/scrollback/blocks/{session_event.rs, cancel_cause.rs}`、`crates/common/xai-interjection-core/`（全部 4 文件）、`crates/codegen/xai-grok-shell/src/session/acp_session_impl/{interjection.rs, prompt_queue.rs, wait_interrupt.rs}`、`crates/codegen/xai-grok-telemetry/src/events/turn.rs`、`crates/codegen/xai-grok-pager/docs/user-guide/20-background-tasks.md`。注意：题目给出的 `crates/codegen/xai-interjection-core/` **不存在**，实际位置是 `crates/common/xai-interjection-core/`。

## 1. 总览：一次轮次的交互时间轴

Grok Build 把「用户按下发送」到「回合终结」的全部交互切成四段，每段各有独立的状态与兜底机制。这个分层是本专题所有设计的主干：

```
用户按 Enter
  │
  ├─① 乐观反馈（本地，0 延迟）
  │    用户气泡入 scrollback、composer 清空、draft 消费
  │    不等待任何网络往返
  │
  ├─② Prompt ack 窗口（网络，无 RPC deadline）
  │    发出 session/prompt，武装 arm_prompt_ack(prompt_id)
  │    ├─ 软期限 10s：spinner 文案改为「等待 agent 接受」
  │    ├─ 任一确认信号到达 → 撤销 watch，写 prompt.acked
  │    └─ 硬期限 120s：本地放弃并还原文本
  │
  ├─③ 回合运行中（流式）
  │    工具块逐条实时更新；用户可插话 / 排队 / Send now / Ctrl+B 转后台
  │    Esc 不取消（只提示），Ctrl+C 才取消
  │
  └─④ 收尾反馈（TurnEnded）
       落一条「Worked for X」marker + 终端通知 + 标题回 idle + 进度条清除
       queue 非空时抑制 TurnComplete 通知（下一条马上开始，避免闪烁）
```

轮次状态机只有 5 个值（`app/agent.rs:573-588`）：

| `AgentState` | 含义 |
| --- | --- |
| `Idle` | 无事发生，队列可以 drain |
| `TurnRunning` | 提示轮进行中 |
| `TurnCancelling` | 已发出取消，等 `PromptResponse` |
| `CommandRunning { command, started_at }` | 斜杠命令在飞（含 `/compact`） |
| `CommandCancelling { command }` | 命令取消已发出 |

关键不变式：**driver 侧只有 `session/prompt` 的 RPC 响应能离开 `TurnRunning`/`TurnCancelling`**（`app/turn_completion.rs:481-483`）。所有「广播先到、RPC 后到或丢失」的情况都靠 §4.4 的兜底补上。

## 2. Prompt ack：发送后到 agent 开始响应

### 2.1 时延预算

`session/prompt` RPC 自身没有 deadline，所以只对**确认（acknowledgment）**设界（`app/prompt_ack.rs:1-8`）：

| 常量 | 值 | 位置 |
| --- | --- | --- |
| `PROMPT_ACK_SOFT_NOTICE` | 10 s | `app/prompt_ack.rs:16` |
| `DEFAULT_PROMPT_ACK_TIMEOUT` | 120 s | `app/prompt_ack.rs:20` |
| `MIN_PROMPT_ACK_TIMEOUT_SECS` | 5 | `:22` |
| `MAX_PROMPT_ACK_TIMEOUT_SECS` | 3600 | `:24` |
| 环境变量 | `GROK_PROMPT_ACK_TIMEOUT_SECS` | `:14` |

软期限不是独立值，而是从硬期限派生：`soft = PROMPT_ACK_SOFT_NOTICE.min(hard / 2)`（`:48`）。所以把硬期限压到 12 s 时，软期限自动变 6 s——**软提示永远不可能晚于硬超时**。

120 s 这个数字的来源写在注释里（`:17-19`）：要盖过 shell 首个 prompt 的最坏情况——模板化 delivery-tools 前缀等待（60 s + 10 s）、一次设置刷新（最长 16.5 s）、一次 auth 刷新、两次 actor 往返；**已恢复的会话在 1 s 内就 ack**。这是设计意图陈述，仓库内没有实测分布数据（未找到证据）。

### 2.2 三种确认信号

`AckSignal`（`app/prompt_ack.rs:77-83`）枚举了仅有的三条解除路径：

| 信号 | 判定 |
| --- | --- |
| `QueueChanged` | `x.ai/queue/changed` 的 `running_prompt_id == prompt_id`，或 `entries` 里含该 id（`:129-135`） |
| `SessionUpdate` | `session/update` 携带该 `prompt_id` |
| `TurnEnded` | 回合结束（`mark_turn_finished` 里无条件调用，`app/agent_view/session.rs:499`） |

武装点有两个：`app/agent_view/session.rs:755`（本地 turn）与 `app/dispatch/queue.rs:639`（服务器队列路径）。解除点在 `app/acp_handler/mod.rs:158`（session/update）与 `app/acp_handler/queue.rs:140,221`（queue/changed、子视图）。

chat 型会话**从不武装**：注释理由是「gateway bridge 没有 queue 广播，它的第一个信号就是第一个 delta」（`app/agent_view/session.rs:507-511`）。

### 2.3 视觉信号：不是状态栏文案，是 spinner 的活动原因

软期限到达时，`dispatch/prompt_ack.rs:97-108` **只做两件事**：写一条 `unified_log` 的 `prompt.ack_soft_notice`（含 `waited_ms` / `limit_ms`），并返回 `true` 让事件循环重绘。没有任何 toast、没有新块。

真正的可见变化发生在活动解析层——`resolve_turn_activity_unenriched()` 在 `TurnRunning` 且 `prompt_ack.is_soft_noticed()` 时返回一个**专用原因**（`app/agent_view/session.rs:1077-1084`）：

```rust
if self.prompt_ack.as_ref().is_some_and(PromptAckWatch::is_soft_noticed) {
    return Some(TurnActivity::Waiting(WaitingReason::PromptAck));
}
```

`WaitingReason::PromptAck` 是 view-only 枚举值，tracker 从不存它（`app/acp/tracker.rs:70-72`），显示文案为：

```
Waiting for the agent to accept the prompt…
```

（`app/acp/tracker.rs:134`；遥测名字 `waiting_prompt_ack` 在 `:146`，出现在 `views/turn_status.rs:896-898` 的期望文案表里。）

界面层次因此是：

```
┌─────────────────────────────────────────────────────────────┐
│ (scrollback)                                                │
│ ❯ 用户刚发的 prompt                                          │
│                                                             │
│ ⠧ Waiting for the agent to accept the prompt…   2m10s ⇣12k  │  ← turn status 行
│   ↑ spinner 不变，只有 label 换成 PromptAck                   │
├─────────────────────────────────────────────────────────────┤
│ ❯ _                                                         │  ← composer 已清空，
└─────────────────────────────────────────────────────────────┘     输入随时可用
```

注意 `[stop]` 在软提示期间仍然在（它按 running/cancelling 显示，不看 ack 状态）。

### 2.4 硬超时兜底：还原文本而不是丢消息

`fire_fail_safe()`（`dispatch/prompt_ack.rs:135-211`）是唯一动作点。它先决定文本去哪儿（`restore_target`，`:118-131`）：

| 条件 | 结果 |
| --- | --- |
| composer 处于 `PromptMode::Normal`，无文本无图片 | `ReplaceComposer`（直接放回输入框） |
| composer 有文本、无图片 | `MergeIntoDraft`（放在草稿**上方**） |
| composer 有图片 | `None` —— 两处图片来源都从 1 编号，`[Image #N]` 占位符会撞车（`:128`） |
| 正在编辑队列行（非 `Normal`） | `None` —— 写进去会污染那行 |

无可恢复目标时走 `NotRestorable`：结束回合、用户块留在 scrollback 供复制。收尾三连：

- scrollback 通知：`The agent did not accept your prompt within {N}s. {recovery} If this keeps happening, run /feedback.`（`:24-40`）
- toast：`Prompt not accepted, text restored` 或 `Prompt not accepted, turn stopped`（`:20-21`）
- 遥测 `PromptAckTimeoutFired { limit_ms, waited_ms, surface, disposition, prompt_kind }`（`:196-202`），日志 `prompt.ack_timeout` 还带 `queue_depth` / `shared_queue_len`

一个易被忽略的设计细节：这条路径用 `finish_turn_view(agent, TurnEnd::Aborted)`（`:165`），所以**它不会产生 cancel latency 事件**（见 §4.3），也显式清掉 `pending_cancel_resend`（`:166-167`）——因为「发一个 shell 从未运行过的 prompt 的取消」没有意义。

另外，`poll_prompt_ack_for_agent` 的 stale 守卫刻意让 `TurnCancelling` 继续保活 watch：注释写「取消中仍然武装，这样 wedge 期间按 Esc 不会让面板永远停在 Cancelling…」（`dispatch/prompt_ack.rs:83-84`）。

## 3. 运行中再次输入：队列、插话、Send now

这是 Grok Build 最容易误解的一块。**按下的键不同，语义完全不同**，而且其中一条的代码与其自身帮助文案互相矛盾。

### 3.1 三条路径对照

| | 普通 Enter | Send now chord | 真正的 Interject |
| --- | --- | --- | --- |
| 键位 | `Enter` | 非 Apple `Ctrl+Enter`（alt `Ctrl+I`）；Apple Terminal `Ctrl+O`（alt `Ctrl+Enter`/`Ctrl+I`）；VS Code 家族 `Ctrl+L` | 无独立 composer 键位（见下） |
| 是否取消当前轮 | 否 | **是** | 否 |
| producer | `dispatch/prompt.rs` 立即发或本地入队 | `agent_view/prompt.rs:555-597` → `Action::SendPromptNow` | `queue_edit.rs:509/535`、`plan.rs:498` |
| 效果 | 入队，当前回合结束后按序 drain | 取消当前轮，新文本作为下一轮 | 合成一条 user message 插进**正在运行**的回合 |
| 证据 | `actions/defaults.rs:497-511` | `actions/defaults.rs:625-654` | `dispatch/interject.rs:13-118` |

### 3.2 一个需要明确标注的矛盾

`ActionId::InterjectPrompt` 的 `ActionDef` 内部自相矛盾（`actions/defaults.rs:625-654`）：

- `label: "send now"`
- `description: "Send now while running (cancels the current turn)"`
- `long_help`: 「Sends a message to the agent mid-turn **without** cancelling it (interject)… Plain Enter while a turn is running queues a follow-up for later; this chord merges composer text into the current turn instead.」

代码实现站在 `description` 这一边。处理器（`app/agent_view/prompt.rs:555-597`）在 composer 非空且 `can_send_now()` 为真时直接返回取消并发送：

```rust
// Mid-turn send-now (cancel-and-send):
// Non-empty composer: cancel the running turn and send that text as the next prompt
```

```rust
return InputOutcome::Action(Action::SendPromptNow { text, images, image_notice });
```

composer 为空时退化为「强制发送队首可见行」（`try_send_now_queued_from_prompt`，`app/agent_view/queue.rs:83-110`）。`can_send_now()` 的定义是「turn 正在跑，或 wake turn 活着且未在取消」（`app/agent_view/session.rs:848-851`）。并且 `Action::Interject` 在整个 pager 里**只有三处 producer**：编辑队列行后 interject（`queue_edit.rs:509`、`queue_edit.rs:535`）、计划评审意见（`plan.rs:498`）、以及 dispatch 侧自身的乐观回显。composer 上的那个 chord 不产生 `Action::Interject`。

**给合稿的提示**：如果照抄 `long_help` 会得到错误的键位语义；应以 `description` + 处理器代码为准。

### 3.3 队列语义

队列有**两层**，pager 和 shell 各持一份，靠 `x.ai/queue/changed` 广播对齐。

```
                        pager（客户端）                         shell（服务端真相）
  ┌──────────────────────────────────────┐   session/prompt   ┌────────────────────────┐
  │ session.pending_prompts  （本地 drip）│ ─────────────────► │ state.pending_inputs   │
  │ shared_queue             （服务器镜像）│ ◄───────────────── │  （InputItem FIFO）    │
  │ optimistic_queue_ids     （乐观回声）  │  x.ai/queue/changed│                        │
  └──────────────────────────────────────┘                    └────────────────────────┘
```

「立即发到服务器队列」需要同时满足全部条件（`dispatch/queue.rs:20-31` `immediate_server_send_eligible`）：

- 处于 leader 模式、或 `follow_up_steer` 打开、或有 wake turn 在跑；
- **且** server busy（`is_turn_running()` 或 wake running 或 `shared_queue` 非空）；
- **且** 有 session id；
- **且** 本地 `pending_prompts` 为空（FIFO 守卫：本地队列非空时不许插队到服务器队列）；
- **且** 不处于队列行编辑模式、不在切模型、不在 replay。

不满足就走本地 `enqueue_prompt_with_skill_tokens`（`dispatch/prompt.rs:1214-1240`）。

界面表现：

- 队列面板 `Ctrl+;`（本地 mac VS Code 家族为 `Ctrl+4`，`actions/defaults.rs:547-560`；手册确认 `Ctrl+;`，`docs/user-guide/20-background-tasks.md:181`）
- 发送时先画**乐观回声**（`push_server_queue_echo`，`dispatch/queue.rs:35-54`），并登记到 `optimistic_queue_ids`，等 `x.ai/queue/changed` 确认；未确认期间对该行按 Send now 会被**挂起**而不是发出去（`app/agent_view/queue.rs:375-390`：真发会被 shell 侧超车然后 no-op）
- 首次「运行中发送」且无 steer、也不在可等待的 parked 状态时，会提示 Send now tip（`dispatch/prompt.rs:1213-1218`）
- turn status 行会显示排队提示，且只在 Enter 真能生效时才写「Enter to send now」：`· {N} queued, Enter to send now` / `· {N} queued`（`views/turn_status.rs:489-495`，同一逻辑在 parked 分支 `:253-260`）

### 3.4 插话语义

插话（interjection）是**回合内**的合成 user message，等价于「不打断，直接追加一句话」。核心实现放在共享 crate，shell 与 server 侧 agent loop 复用（`crates/common/xai-interjection-core/src/lib.rs:5-9`）。

**载荷格式**（`crates/common/xai-interjection-core/src/format.rs`）：

```
The user sent a message while you were working:
<user_query>
{用户文本}
</user_query>
Make sure to complete any unfinished tasks from previous turns.
```

- `INTERJECTION_NOTE`（`:4`）与 `INTERRUPT_NOTE = "The user interrupted the previous turn:"`（`:5`）共用同一个信封（`:22-24` `frame_user_turn`）
- 第 5 行的 unfinished-tasks 提醒是刻意加的：防止中途 steer 或取消后跟进导致在飞任务被丢下（`:7-9`）
- 超过 `LARGE_PROMPT_THRESHOLD = 25_000` 按 UTF-8 边界截断并追加 `... [truncated]`（`:2, 27-37`）

**排空时机**：只在「安全点」（`acp_session_impl/interjection.rs:279-301` 的文档注释）——一个工具批次之后、循环顶部、以及回合返回用户之前。同一函数还有一个副作用：若 `follow_up_steer_enabled()` 为真且队里还有可编辑的人写的行，就先 `promote_queued_as_interjections()`，**把队列行提升为插话**。这就是「队列 vs 插话」在配置层面的开关：默认队列，切到 steer 后队列行会被提前合并进当前回合。

**排空实现**（`:311-388`）：`RestoreOnCancel` 守卫先接管全部 entry，逐个做 sanitize（剥掉 `[Image #N: <path>]` 里的路径）、解析 `/skill` 引用、`format_interjection` 包装、图片管线，然后**一次性** `try_push_user_messages_batch` 提交，成功后才落盘（注释明确：若在提交前取消，entry 回缓冲、由 fallback turn 去落盘，否则 `updates.jsonl` 会重复）。守卫的 `Drop` 调用 `EventQueue::restore_front()` 把未提交项放回**队首**（`crates/common/xai-interjection-core/src/events.rs:75-82`，注释：「读者异步处理中被 abort 时恢复未处理的剩余部分，它们在后来 push 的任何东西之前到达」）。

**落空不丢消息**：插话只在运行中合并。若它落地时会话已空闲、或错过了本回合最后一次排空，转成独立回合，prompt id 前缀 `interject-fallback-`（`acp_session_impl/interjection.rs:17-24, 30-83`），并按原顺序推到队列**最前**（`flush_stranded_interjections`，`:87-96`，用反向 `push_front` 保持 entry 0 在前）。有测试专门锁这条生命周期：`crates/codegen/xai-grok-pager-pty-harness/tests/pty_e2e/queue_and_interjection_lifecycle.rs`。

**pager 侧**（`dispatch/interject.rs:13-118`）：立即推一个本地用户块（`RenderBlock::interjection_prompt`），把 id 记入 `self_interjection_ids` 以便丢弃广播回声；同时记 `interjection_painted_blocks` 与 `interjection_retry_images`；弹 toast `Interjection sent`；最后返回 `Effect::SendInterject`。一条硬约束写在测试注释里：**dispatch 永不触碰 composer**，只有 composer 路径的调用点（`InterjectPrompt` registry arm）自己清空（`dispatch/interject.rs:216-218`，测试 `interject_dispatch_never_touches_the_composer`）。

### 3.5 Send now（取消并发送）

`dispatch_send_prompt_now()`（`dispatch/interject.rs:122-205`）值得单独看，因为它是**唯一会同时产生「取消」和「新消息」**的路径，因此需要三处配合：

1. 断连守卫：`reconnect_pending` 时把文本 requeue 到本地队首并 toast `Reconnecting, please wait...`，而不是打进死通道（`:145-163`）
2. 静默取消：`arm_send_now_and_paint_dispatched()` 武装期望，让取消的 `TurnCancelled` marker 被**抑制**——`terminal_marker()` 里 `TurnStopReason::Cancelled if input.send_now_cancel => None`（`app/turn_completion.rs:95`）。注释称之为「cancel-and-send 的静默那一半」（`:22`）
3. 乐观队列回声 + `Effect::SendPromptNow`（`:190-204`）

## 4. 取消：粒度、升级与延迟指标

### 4.1 Esc 不取消——这是刻意的

运行中按 Esc **不会**取消回合。`try_handle_esc_policy()`（`app/agent_view/prompt.rs:747-821`）的运行中分支：

```rust
// Mid-turn (running or already cancelling), every mode: Esc never cancels;
// point at the registry cancel binding instead
if self.stoppable_activity_running() || self.any_cancel_pending() {
    if self.stoppable_activity_running()
        && let Some(cancel_key) = registry.key_for(ActionId::CancelTurn)
    {
        let cancel_key = cancel_key.display();
        self.show_cancel_key_hint(&format!("Press {cancel_key} to cancel the turn"));
    }
    self.suppress_rewind_arm(std::time::Instant::now());
    return Some(InputOutcome::Changed);
}
```

三个设计点：

- 提示文案里的键名**从 registry 现取**，不是硬编码 `Ctrl+c`，所以用户重绑定后提示跟着变（`:773-778`）
- 同一个 Esc 顺便把 rewind-arming 推到 `ESC_CANCEL_REWIND_GRACE = 1000 ms` 之后（`:738-741, 781`）。理由是用户在回合即将结束时空按 Esc，落地时会落在空闲且通常为空的 composer 上，没有 grace 的话下一次按就立刻武装并触发双击 rewind picker
- minimal 模式没有 toast 槽位，提示改写成 scrollback 里的 system 块，且**每个用户回合最多一条**（`show_cancel_key_hint`，`:824-836`，靠 `minimal_cancel_hint_turn` 去重）

`ActionId::CancelTurn` 的 `long_help` 把这条策略写成正式契约（`actions/defaults.rs:508`）：

> Ctrl+C cancels when the prompt is empty; with a non-empty draft it clears the prompt first and leaves the turn running. **Esc never cancels a turn**; pressed mid-turn it shows a reminder to use Ctrl+C.

空闲时 Esc 是另一套「双击」语义（`prompt.rs:787-812`）：有草稿且焦点在 prompt → 武装 `ClearPrompt`；空且 `scrollback.turn_count() > 0` → 武装 `RewindShowPicker`（首次静默）；都没有就吞掉。双击窗口 `PendingAction::ESC_DOUBLE_PRESS_TTL = 800 ms`（`app/app_view.rs:476`），可用 `GROK_ESC_DOUBLE_PRESS_MS` 覆盖（上限 `ESC_DOUBLE_PRESS_TEST_MS = 60_000`，`app/app_view.rs:501-514`）。另外 Esc 后 500 ms 内按 `d` 会触发 `Action::DumpInputLog`（`app/agent_view/input.rs:386-397`）。

已在取消中时，Esc 被**静默吞掉、连提示都不给**——理由写在测试里：那时 Ctrl+C 已经升级为退出，再提示 Ctrl+C 会误导（`app/agent_view/input.rs:2276-2285`）。

### 4.2 取消也分粒度：Turn 与 Compaction

`CancellationScope` 只有两个值（`crates/codegen/xai-grok-telemetry/src/events/turn.rs:38-43`）：

| scope | 触发 | 动作 |
| --- | --- | --- |
| `Turn` | 普通回合运行中 | `session.cancel_turn()` |
| `Compaction` | `state.is_compact_running()` | `session.cancel_compact_command()` |

两者由一个入口分派，从而保证「动作」与「它测量的 scope」不会漂移（`app/agent_view/session.rs:543-554`）：

```rust
pub(crate) fn cancel_and_arm(&mut self, scope: CancellationScope, origin: CancelOrigin) {
    let now = Instant::now();
    match scope {
        CancellationScope::Turn => self.session.cancel_turn(&mut self.scrollback),
        CancellationScope::Compaction => self.session.cancel_compact_command(),
    }
    if origin == CancelOrigin::UserGesture && self.surface() == ViewSurface::Root {
        self.cancel_latency.get_or_insert_with(|| CancelLatency::new(now, scope));
    }
}
```

**没有「取消当前工具」这一级**；粒度只有上述两种，加上 §4.5 的「取消子代理」选项。`/compact` 是唯一被当作独立 scope 的命令，测试确认在 `/jump` 打开时 `Ctrl+C` 仍能取消 compact 而不是被 picker 吃掉（`app/agent_view/input.rs:2334-2351`）。

### 4.3 延迟指标：`cancel_latency`

三个小类型（`app/cancel_latency.rs`，全文件 34 行）：

```rust
pub(crate) enum CancelOrigin { UserGesture, Programmatic }   // :8-12
pub(crate) enum TurnEnd { Completed, Aborted }               // :16-20
pub(crate) struct CancelLatency { requested_at: Instant, scope: CancellationScope }  // :21-25
```

结算规则（`app/agent_view/session.rs:555-571`）：

```rust
pub(crate) fn settle_cancel(&mut self, end: TurnEnd, now: Instant) -> Option<CancellationCompleted> {
    let pending = self.cancel_latency.take();
    match end {
        TurnEnd::Completed => pending.map(|p| CancellationCompleted {
            latency_ms: now.saturating_duration_since(p.requested_at).as_millis() as u64,
            scope: p.scope,
        }),
        TurnEnd::Aborted => None,
    }
}
```

- `Completed`：回合走到自己的终结态（自然结束，或一次被尊重的取消落地）→ 计量并上报 `CancellationCompleted { latency_ms, scope }`（遥测名 `cancellation_completed`，`crates/codegen/xai-grok-telemetry/src/events/mod.rs:289`）
- `Aborted`：视图被 reload / fork / 会话失败强制置闲 → **丢弃** anchor，不上报
- 只有 `UserGesture` 且 `surface() == Root` 才设 anchor（`session.rs:550-553`），`Programmatic` 在 `cancel_latency.rs:10` 标了 `#[allow(dead_code)]`
- anchor 与消费都 `take()`，所以每个事件**最多上报一次**（`session.rs:556-558` 注释）
- 模块注释给出一条边界：「所有消费者都在 `agent_view`（anchor/settle）与 `dispatch`（取消调用点）；`agent.rs` 不用它们」（`cancel_latency.rs:4`）

### 4.4 二次按下与升级：Ctrl+C → Ctrl+C = 退出

`ActionId::CancelTurn` 的默认键是 `Ctrl+C`（`actions/defaults.rs:497`），且**只在 `When::AgentScreen` 层注册**——scrollback 层与全局层都查不到它（`crates/codegen/xai-grok-pager/src/actions/mod.rs:785-796` 的测试 `cancel_at_agent_level`）。这条注册边界本身就是「取消是 agent 屏幕级动作，不是全局退出」的表达。

二次按下的行为在处理器里（`app/agent_view/input.rs:1231-1247`）：

```rust
ActionId::CancelTurn => {
    if self.stoppable_activity_running() {
        self.cancel_trigger_hint = Some(crate::app::actions::CancelTrigger::CtrlC);
        return InputOutcome::Action(Action::CancelTurn);
    }
    if self.any_cancel_pending() {
        return InputOutcome::Action(Action::Quit);          // ← 二次按下 = 退出
    }
    if crate::app::minimal_mode_active() && self.session.state.is_idle() && self.prompt.text().is_empty() {
        return InputOutcome::Action(Action::Quit);
    }
    InputOutcome::Unchanged
}
```

即：**第一按取消回合，回合已进入 cancelling/wake-cancelling 时再按升级为 Quit**。同一升级在 wake turn 上也被测试锁定（`app/agent_view/input.rs:2376-2394`：首个 Ctrl+C 发 CancelTurn，`cancel_sent = true` 后第二个变 `Action::Quit`）。

### 4.5 取消前的闸门：子代理还在跑怎么办

若触发取消时仍有子代理在运行，会先弹一个 4 选项面板（`views/modal.rs:120-145, 899-960`）：

```
┌─ Subagents are still running. Stop them? ──────────────┐
│                                                        │
│ 1 (●) Stop running                                     │
│ 2 (○) Continue to run                                  │
│ 3 (○) Always stop                                      │
│ 4 (○) Always continue                                  │
└────────────────────────────────────────────────────────┘
```

- 标题带一个子代理计数行：`1 subagent running` / `N subagents running`（`:906-912`）
- 前两项是一次性选择，只影响这次取消；后两项写全局/会话偏好 `cancel_subagents_on_turn_cancel`（`dispatch/turn.rs:230-249`）
- 键盘走 `agent_view/interactions.rs:224-300`（上下移动、数字直选、Enter 确认），鼠标按行 hit-test

### 4.6 取消后的界面状态与两条兜底

取消路径 `cancel_agent_turn()`（`dispatch/turn.rs:253-349`）的完整副作用序列：

1. compact 在跑 → 走 `Compaction` scope，清空 cancel panel、`drain_permission_queue`、清 send-now 期望，直接发 `Effect::CancelTurn`
2. wake turn 在跑 → `mark_wake_cancel_sent()`（wake turn 从不被 adopt，所以取消状态挂在 `running_wake_turn` 而不是 `AgentState`，见 `dispatch/turn.rs:526-527`）
3. 非运行 → 直接返回空
4. 否则评估 **rewind**：`cancel_rewind_enabled && in_flight_prompt` 存在 `&& !queue_held_behind_turn && !in_flight_committed && !composer_has_draft && rewind_prompt_id.is_some()`。条件全真时把提示文本**收回 composer**（`RewindTarget::ReplaceComposer`），界面看起来像「用户从没按过发送」；否则才真正 `cancel_and_arm(Turn)`
5. 收尾：清 cancel panel 与按钮、`drain_permission_queue`（每个挂起权限各得一个 Cancelled）、`dismiss_in_turn_plan_review`、`clear_send_now_expectation`
6. 注释明确一条克制策略：交互式取消**只拆当前回合**，不把队列里的 prompt 拉回输入框、也不预测新队列顺序，让 agent 自己把队首提升为下一轮（`dispatch/turn.rs:126-128`）

两条兜底都跑在动画 tick 上（`app/event_loop.rs:2406-2430`，顺序是 cancel → prompt ack → lost response）：

| 机制 | 阈值 | 位置 |
| --- | --- | --- |
| 取消重发（fire-and-forget 的 `session/cancel` 会丢） | `CANCEL_RESEND_GRACE = 3 s`，`CANCEL_RESEND_MAX_ATTEMPTS = 3` | `dispatch/turn.rs:502-506, 510-564` |
| 丢失 RPC 响应的回合收尾 | `TURN_END_RECONCILE_GRACE = 2 s` | `dispatch/turn.rs:566-569, 574+` |

重发的抑制条件很讲究：一旦 `pending_turn_end_reconcile` 已武装（说明收到了 `prompt_complete` 广播），就**不再重发**，因为重发会和它竞争、甚至取消掉 shell 已经提升的队列 prompt（`dispatch/turn.rs:533-540`）。

取消回执文案由两段信息拼出——`_meta.cancelTrigger` 与 `_meta.cancellationCategory`（`app/turn_completion.rs:22-27`）：

```
_meta.cancelTrigger:        "send_now" | "ctrl_c" | "mouse" | "dashboard_stop" |
                            "session_close" | "session_delete" | "shutdown" | "host_interrupt"
_meta.cancellationCategory: "HookDenied" | 权限拒绝 / 权限忽略 / 达到回合上限
_meta.completionKind:       RemovedFromQueue（排队但从没跑过，用来区别于真取消）
```

`CancelTrigger` 的 pager 侧只有三个值（`app/actions.rs:1154-1171`）：`CtrlC → "ctrl_c"`、`Mouse → "mouse"`、`DashboardStop → "dashboard_stop"`；`Effect::CancelTurn.trigger` 的注释说明它随 `session/cancel` 的 `_meta.cancelTrigger` 上报，让 agent 的 `mid_turn_abort` 遥测能区分手势（`app/actions.rs:1576-1580`）。解析顺序是 **category 优先、trigger 兜底**（`scrollback/blocks/cancel_cause.rs:24-39`），得到 8 种归因：

| `CancelledBy` | 回执短语（`cancel_cause.rs:41-52`） |
| --- | --- |
| `User` | `Turn cancelled by user` |
| `SessionClosed` | `Turn cancelled because the session closed` |
| `Shutdown` | `Turn cancelled because the session shut down` |
| `MaxTurns` | `Turn cancelled after reaching the turn limit` |
| `PermissionDenied` | `Turn cancelled because a permission was denied` |
| `PermissionDismissed` | `Turn cancelled because a permission prompt was dismissed` |
| `HostInterrupt` | `Turn cancelled by the agent host` |
| `Unspecified` | `Turn cancelled` |

值得注意：wire 上的 `"esc"` 也算用户手势（测试 `cancel_cause.rs:61` 遍历 `["esc","ctrl_c","mouse","dashboard_stop"]` 全部映射到 `User`）——这是给其它前端/旧客户端留的兼容位，pager 自己不发它。

界面状态汇总：

| 时刻 | turn status 行 | 说明 |
| --- | --- | --- |
| 运行中 | `⠧ <activity> 0.2s … 1m20s ⇣12k [stop]` | `[stop]` 可点 |
| 已取消、等响应 | `Cancelling…` + `[stop]` | **取消中仍保留可点的 `[stop]`**，作为丢失取消的重试入口（`views/turn_status.rs:984, 1014-1018`） |
| 落地 | scrollback 追加 `Turn cancelled by user in 12.3s.` | 见 §5.4 |

## 5. 轮次完成时的收尾反馈

### 5.1 两条到达轨道

pager 从两条轨道得知回合终结，最终都汇聚到同一个函数，避免行为漂移（`app/turn_completion.rs:1-7`）：

- fire-and-forget 的 `x.ai/session/prompt_complete` 广播（为未升级的 leader 保留一个版本）
- 持久化的 `XaiSessionUpdate::TurnCompleted`（会写盘并重放）

汇聚点是 `finalize_turn_from_terminal()`（`:484-552`）。分流规则是：driver（`!attached_as_viewer`）**不在这里结束回合**，因为 `PromptResponse` RPC 拥有生命周期；viewer 则 adopt driver 的回合并自行终结（`:481-483, 498-503`）。这样「回合中途重新挂上的 viewer」能从重放里终结回合，而不是永远卡在 Waiting。

### 5.2 终端通知

5 种事件类型（`notifications/config.rs:100-115`），默认启用前两种（`:30-33`）：

| 事件 | 显示名 | 默认 |
| --- | --- | --- |
| `TurnComplete` | `Turn complete` | ✅ |
| `ApprovalRequired` | `Approval required` | ✅ |
| `SessionReady` | `Session ready` | — |
| `TaskComplete` | `Task complete` | — |
| `AgentError` | `Agent error` | — |

编码方式 `NotificationMethod`：`Auto`（默认）/ `Osc9` / `Osc99` / `Osc777` / `Bel` / `None`（`:44-54`）；触发条件 `NotificationCondition`：`Unfocused`（默认）/ `Always` / `Never`（`:56-63`）。注意 **`Bel` 是通知编码方式之一，而不是独立的「响铃」功能**——仓库里没有找到独立的声音/蜂鸣逻辑（未找到证据）；协议层实现在 `notifications/protocol.rs`，本次未逐行核对具体转义序列。

`notify()` 同时做三件事：查事件白名单、查 focus 条件、命中后写遥测（`notifications/mod.rs:96-117`）：

```rust
pub fn notify(&self, event: NotificationEvent) {
    if !self.is_event_enabled(&event.kind) { return; }
    if self.should_emit_terminal() {
        protocol::emit_notification(self.protocol, ..., &self.escape_writer);
        xai_grok_telemetry::session_ctx::log_event(NotificationEmitted { protocol, event_kind, was_focused });
    }
}
```

**通知走 out-of-band escape 队列**，绕过帧管线——注释理由是「罕见的一次性事件不该等下一帧（16 ms 之后，或者在有帧 ack 挂起时无限期）」（`:93-95`）。

### 5.3 通知延迟与队列非空的抑制

TurnComplete 的正文由 `dispatch/prompt.rs:1675-1689` 生成：

| 情况 | 正文 |
| --- | --- |
| `Ok` 且不是 bash turn | `Turn complete in 1m20s.` / `Turn complete.` |
| `Err` 且未走专用 UX | `Error: {err}`（kind = `AgentError`） |

**抑制规则**（`:1724-1744`）：队列非空时不发 TurnComplete，也不发 idle escapes——因为下一轮马上就起来，会覆盖标题造成闪烁。判断里还额外把「已 stash 的服务器权威 adoption」当作队列非空处理。

**延迟 3 帧**（`:1753-1773`）：

```rust
// Defer the notification so the terminal has time to apply the idle title
// Ghostty debounces setTitle() by 75 ms (SurfaceView_AppKit.swift:576)
// So we need more than 75 ms before the notification reads self.title for the
// subtitle; 3 ticks × 33 ms ≈ 99 ms
app.deferred_notification = Some((NotificationEvent { kind, title, body, session_id }, 3));
```

倒计时在每 tick 递减，归零才真正 `notify()`（`app/app_view.rs:5495-5503`），且只要还有 deferred 通知就要求 `TickDemand::Fast`（`:5659-5661`）。通知标题用会话名（`:1760-1763`），无名字时用 `"Grok"`。

唤醒型回合（wake turn）有一条平行路径 `queue_wake_turn_complete_notification()`（`app/acp_handler/session_notification.rs:1407-1440`），同样有「`pending_prompts` 非空则不发」的门禁，并复用 `build_idle_escapes`。

### 5.4 Scrollback 收尾标记

`SessionEvent::message()` 是唯一文案来源（`scrollback/blocks/session_event.rs:376-382`）：

| 事件 | 文案 |
| --- | --- |
| `TurnCompleted { elapsed: Some }` | `Worked for 1m20s`（故意**不加句点**，见 `:375` 注释） |
| `TurnCompleted { elapsed: None }` | `Turn completed.` |
| `TurnCancelled { elapsed: Some, cause }` | `{cause} in 1m20s.` |
| `TurnCancelled { elapsed: None, cause }` | `{cause}.` |
| `TurnBlockedByHook` | `Turn blocked by a hook in 1m20s.` / `Turn blocked by a hook.` |

三种情况**不推标记**（`app/turn_completion.rs:87-109`）：send-now 的静默取消、`RateLimit`、以及已有错误横幅的 `Error`（错误横幅本身已经说了）。

hook 拒绝回合还有一条额外路径：`note_hook_blocked_turn()`（`:151-204`）挂起队列、把被拒 prompt 重新入队到**队首**（注释：修好的 resubmission 必须跑在滞留的跟随者前面），若弹不出卡片就退化为 toast `A hook blocked the last prompt — the queue is paused`。卡片提供 `Edit / Resend / Discard` 三选（`:282-301`）。

### 5.5 标题与进度条

标题项 `TitleItem`（`notifications/config.rs:87-98`）：`Spinner` / `Activity` / `SessionName` / `Cwd` / `Model` / `TurnTimer` / `Grok` / `ActionRequired`。默认配置与顺序（`:72-85`）：

```rust
items: vec![TitleItem::ActionRequired, TitleItem::Spinner, TitleItem::Activity,
            TitleItem::SessionName, TitleItem::Grok]
```

`ActionRequired` 只在有待处理权限时出现，并且**跨 tick 闪烁**（测试 `title.rs:562-616` 覆盖「首个 tick 可见」「跨 tick 闪烁」「无权限时隐藏」）。

进度条走 OSC 9;4，`PROGRESS_KEEPALIVE = 5 s`（`notifications/mod.rs:14-16`）：注释说明 Ghostty 约 15 s 静默后会自动重置该指示器，所以要定期重发。`on_tick()` 统一产出标题与进度两类转义（`:140-179`）；空闲时 `build_idle_escapes()` 清进度并把标题改回 idle，且刻意让调用方把这些字节走**帧管线**，理由是「这样它们相对之前可能仍带 busy 标题的帧顺序才是对的」（`:118-124`）。

### 5.6 后台仍在跑时的常驻提示

回合结束后如果还有后台工作，turn status 行不会消失，而是变成常驻提示（`views/turn_status.rs:105-130`、`docs/user-guide/20-background-tasks.md:187-201`）：

```
◎ 1 command · 2 monitors · 1 loop · 1 subagent still running
```

- 计数覆盖后台命令、monitor、`/loop`、后台子代理；`0` 的种类不列出；单数不加 `s`（`format_still_running`，`:107-128`）
- 这是**唯一**展示运行计数的地方：「Worked for」标记保持朴素，transcript 从不重复运行计数（手册 `:193`）
- 在等待型 blocked 状态下额外追加提示（`views/turn_status.rs:253-262`）：

```
◎ 1 command still running · send a message to interrupt
◎ waiting · send a message to interrupt
```

- 脉冲图标 `○ ◎ ◉ ◎` 用 `MONITOR_PULSE_DIVISOR = 8`，约为活动 spinner（`SPINNER_DIVISOR = 4`，~7.5 fps）的一半速度，注释理由：idle 提示应当「呼吸」而不是读成活动中的 spinner（`:29-36`）

turn status 行的完整布局（`views/turn_status.rs:1-13` 的模块文档）：

```
⠧ Run command 0.2s              1m20s ⇣12k [stop]
└┬┘└─────┬─────┘                └──┬──┘└─┬─┘└──┬──┘
 │        │                         │     │    └─ 取消按钮（红色，hover 高亮；
 │        │                         │     │       取消中仍可点，作为重试入口）
 │        │                         │     └────── token 数 ⇣Nk（右对齐，灰）
 │        │                         └──────────── 回合计时 Xm Ys（右对齐，灰）
 │        └────────────────────────────────────── 活动标签（按类型着色，可截断）
 └──────────────────────────────────────────────  spinner（~7.5 fps）
        中间可选：· N queued, Enter to send now（灰，仅可发送等待态）
```

状态标签由 `compute_activity` 统一产生（`:580-620`），包括 `Cancelling…`（红）/ `Verifying…` / `Thinking…` / `Responding…`，以及 §2.3 的 `Waiting for the agent to accept the prompt…`。整个行 idle 时高度为 0（`:13`）。

## 6. 工具调用期间的界面

### 6.1 工具块的实时更新

工具块的运行态由 `ScrollbackEntry::is_running` 表示（`scrollback/entry.rs:67`，构造 `:184`），折叠策略随它变化（`scrollback/block.rs:88-100`）。`session/update` 的 `ToolCallUpdate` 在 `app/acp_handler/mod.rs:260-264` 被拦截；如果它属于某个后台任务就改道（见下），否则交给正常 tracker。

回合终结时会做一次收尾清扫，理由写得具体（`app/turn_completion.rs:624-625`）：

```rust
// `finish_turn` only reaches rows the tracker saw. A resumed transcript's
// running tools would otherwise spin under an idle footer.
child.scrollback.finish_all_running();
```

### 6.2 长命令输出：覆盖式单块更新 + 硬上限

后台任务的 stdout 不走「追加文本块」，而是**整段覆盖**同一个内部缓冲区（`app/acp_handler/background.rs:8-58`）：

```rust
// The shell sends the full cumulative output buffer, so just overwrite
if !stdout.is_empty() && let Some(bg_task) = session.bg_tasks.get_mut(&task_id) {
    bg_task.set_stdout(stdout);
    if chunk_truncated { bg_task.truncated = true; }
}
```

要点：

- `tool_call_id → task_id` 的映射存在 `bg_task_to_task`（`:14-17`），所以后续 chunk 能找到归属
- 优先取 `output_for_prompt`（shell 侧已剥壳的字符串），否则解码 `output` 字节数组
- 空 stdout 不覆盖——shell 完成时会清空缓冲区（`:44-46`）
- `truncated` 是**单向闩锁**：shell 侧一旦置真就保持（滚动缓冲无法「不截断」，`:38-42`）
- `BG_TASK_MAX_STDOUT = 10 * 1024 * 1024`（`app/agent.rs:153`）。超过时保留头部、按 UTF-8 字符边界裁剪并置 `truncated`（`agent.rs:218-235`）
- 单独缓存 `stdout_line_count`，理由是「否则 tasks-pane 每帧每个可见任务都要 memchr 扫描最多 10 MiB」（`agent.rs:186-192`）

过渡事件 `x.ai/task_backgrounded` 由 `handle_task_backgrounded()` 处理：建 `BgTaskState`、注册映射，或推一个全新的 BgTask 块（`background.rs:60-90`）。

### 6.3 需要用户输入的交互：卡片 + 键盘所有权仲裁

所有「阻塞式」交互都不阻塞事件循环，而是做成**卡片（card）或全屏接管**，由 `KeyOwner` 一层层仲裁（`app/agent_view/key_owner.rs`）。

需要用户回答的卡片只有四种（`key_owner.rs:9-16`）：

| `BlockingCard` | 来源 |
| --- | --- |
| `Permission` | 权限面板 |
| `CancelTurn` | §4.5 的子代理取消闸门 |
| `Question` | `x.ai/ask_user_question` |
| `McpElicitation` | MCP `x.ai/mcp/elicit` |

优先级顺序（`key_owner.rs:38-53`）：

```
全屏接管（子代理视图 / 媒体查看器 / /gboom / modal 栈）   ← 自己回答按键，不在本表内
  │
  └─ LineViewer / BlockViewer（计划预览、文件预览）
       │
       └─ Card(BlockingCard)      ← Permission 排在 LineViewer 之前，以便 followup 能打字
            │
            └─ PlanApproval（预览已关闭的计划审批提示）
                 │
                 └─ Pane           ← 什么都没有阻挡：聚焦面板 + 全局绑定
```

两条对用户很友好的细节：

- 卡片虽在，但 `Tab` / `Space` 能把键盘交还 scrollback（`BlockingCard::focus_hint()`，`key_owner.rs:18-36`）；这个 hint 是 **pinned** 的，窄条被裁剪也不会丢
- 卡片上的 Esc 是「一级一级后退」而不是一步关闭：`EscStep`（`key_owner.rs:55-60`）先清掉卡片内挂起的输入，然后再离开；所有声明 Esc 的提示都读同一个函数，所以快捷键条不会承诺按不出来的动作

`elicitation` 卡片的模块文档直接说明了它与 composer 的关系（`app/agent_view/elicitation.rs:1-2`）：

> The MCP elicitation card (`x.ai/mcp/elicit`): routes key, mouse, and paste input and resolves accept, decline, and cancel. Also promotes the next pending request when a card closes, and **stashes and restores the composer draft**.

即：卡片弹出时把 composer 草稿 stash 走，关闭时还原——用户不会因为在卡片里操作而丢草稿。同一模式在队列编辑里更严格：脏编辑会**锁住面板切换**并提示 `Editing a queued prompt: press Enter to save, Esc to discard`（`app/queue_edit.rs:79-101`），且刻意不武装 `EditConfirm` modal，因为「它没有绘制分支，会隐形地吃掉所有输入」（`:84-86`）。

### 6.4 前台命令的降级与等待打断

- `Ctrl+B`（`ActionId::SendToBackground`，label `send to bg`，`actions/defaults.rs:611-621`）把正在跑的前台命令转后台，任务继续跑、完成时通知（手册 `:44`）。派发到 `Effect::DemoteToBackground`（`app/effects/mod.rs:1925`）。手册补充一条容易忽略的行为：**「发送一条新消息也会把该命令转后台而不是杀掉它」**
- 当回合阻塞在「可被用户打断的等待」上（如 `get_command_or_subagent_output` 正阻塞等输出）时，发消息会立即打断等待并运行你的消息；界面提示就是 §5.6 的 `· send a message to interrupt`（手册 `:195-201`）
- 相关判定：`is_parked_on_sendable_wait()` 要求 `is_sendable_wait(activity)` 为真，**且** goal 循环未激活——因为 shell 在 goal 运行期会抑制这个 abort，把它们当成可打断会说谎（`app/agent_view/queue.rs:105-118`）

### 6.5 不丢完成事件的缓冲

子代理「先完成、后 spawn」的生命周期错位用一个有界缓冲处理（`app/deferred_subagent_finishes.rs`）：

- `MAX_DEFERRED_SUBAGENT_FINISHES = 256`，`DEFERRED_SUBAGENT_FINISH_TTL = 60 s`（`:10-11`）
- `defer()` 时把 `SubagentFinished` 的 `output` 字段**置空**再存（`:82-84`）——只留占位，真正的输出在 `take()` 时再取
- 超限时 `evict_oldest()`（`:89-93`），过期时 `prune_expired()` 同时让生命周期「忘掉」该 pending attempt（`:149-174`）
- `sanitize_lifecycle()` 用来剔除没有后备通知的 pending attempt（`:58-70`）

## 7. 「不阻塞输入」的设计手法汇总

把散落各处的做法集中起来，一共 12 条，是 Grok Build 交互延迟观感的来源：

| # | 手法 | 证据 |
| --- | --- | --- |
| 1 | **乐观本地回显**：插话、Send now、服务器队列行都在 dispatch 时就画出来，不等服务器确认 | `dispatch/interject.rs:76-89`、`dispatch/queue.rs:35-54` |
| 2 | **草稿 stash & restore**：卡片、队列编辑、外部编辑器轮转期间草稿被暂存，退出时精确还原一次 | `agent_view/elicitation.rs:2`、`queue_edit.rs:6-7`（「`exit_editing_mode` 是唯一还原者」） |
| 3 | **阻塞式交互做成卡片而非阻塞循环**，用优先级表仲裁键盘，而不是让事件循环停下等回答 | `agent_view/key_owner.rs:38-53` |
| 4 | **卡片 Esc 分级后退**，且卡片在挡住键盘时仍提供 `Tab`/`Space` 交还路径 | `key_owner.rs:18-36, 55-60` |
| 5 | **误触防护**：运行中 Esc 不取消（只提示正确键名），Ctrl+C 才取消；已在取消中再按 Ctrl+C 升级为退出 | `agent_view/prompt.rs:772-782`、`input.rs:1231-1247` |
| 6 | **插话落空不丢消息**：只在运行中合并，错过则转成 `interject-fallback-` 独立回合并推到队首 | `acp_session_impl/interjection.rs:17-24, 87-96` |
| 7 | **取消安全（cancel-safety）**：被 abort 的排空把未提交项放回队首；乐观队列行在 RPC 落定后退役 | `crates/common/xai-interjection-core/src/events.rs:75-82`、`dispatch/queue.rs:56-60` |
| 8 | **三套超时兜底都跑在动画 tick 上**，失败也不阻塞输入：prompt ack 120 s、取消重发 3 s×3、丢失响应 2 s | `app/prompt_ack.rs:20`、`dispatch/turn.rs:503-506, 569`、`event_loop.rs:2406-2430` |
| 9 | **通知/标题/进度走 out-of-band escape 队列**，不等帧管线（可能被帧 ack 无限期挂住） | `notifications/mod.rs:45-46, 93-95` |
| 10 | **idle 标题的转义仍走帧管线**，以保证相对「仍带 busy 标题的旧帧」的顺序正确 | `notifications/mod.rs:118-124` |
| 11 | **长输出用覆盖式单缓冲 + 10 MiB 上限 + 单向 truncated 闩锁**，避免增量文本块把 scrollback 撑爆 | `acp_handler/background.rs:19-53`、`app/agent.rs:153, 218-235` |
| 12 | **tick 需求按需升降频**：有待发通知时要求 `TickDemand::Fast`，空闲时可降频 | `app/app_view.rs:5659-5661`、`app/status_line_policy.rs:47-60` |

## 关键结论（供合稿引用）

1. Prompt ack 只有一条硬预算——**120 s**，软提示固定 **10 s**（且被 `hard/2` 压制），可用 `GROK_PROMPT_ACK_TIMEOUT_SECS` 覆盖，钳制在 5–3600 s；120 s 是为盖住 shell 首个 prompt 的最坏路径而定的（`crates/codegen/xai-grok-pager/src/app/prompt_ack.rs:14-24, 48`）。
2. 软提示**不弹任何 UI**，只把 spinner 的活动原因切成 `WaitingReason::PromptAck`，文案 `Waiting for the agent to accept the prompt…`；唯一副作用是写一条 `prompt.ack_soft_notice` 日志（`app/dispatch/prompt_ack.rs:97-108`、`app/agent_view/session.rs:1077-1084`、`app/acp/tracker.rs:134`）。
3. 硬超时的兜底是**还原而非丢弃**：按 composer 状态决定 `ReplaceComposer` / `MergeIntoDraft` / `NotRestorable`，编辑器忙或草稿含图片时明确不还原（`app/dispatch/prompt_ack.rs:20-40, 118-131`）。
4. ack 只有三种解除信号：`x.ai/queue/changed` 列出该 prompt、`session/update` 携带该 prompt、回合结束；chat 型会话不武装 watch（`app/prompt_ack.rs:77-83, 129-135`、`app/agent_view/session.rs:507-511`）。
5. 运行中再次输入是**三分法**：普通 Enter = 入队；Send now chord = 取消当前轮并立刻发（`Action::SendPromptNow`）；真正的 `Action::Interject` 只由队列行编辑与计划评审意见产生（`actions/defaults.rs:625-654`、`app/agent_view/prompt.rs:555-597`、`app/queue_edit.rs:509, 535`、`app/agent_view/plan.rs:498`）。
6. `ActionId::InterjectPrompt` 的 `long_help` 与 `description` **互相矛盾**（前者说不取消、后者说取消），代码站在 `description` 一边；照抄 `long_help` 会写错键位语义（`actions/defaults.rs:625-654`）。
7. 插话只在「安全点」排空——工具批次后、循环顶部、回合返回用户前；`follow_up_steer` 打开时队列行会被**提升为插话**（`crates/codegen/xai-grok-shell/src/session/acp_session_impl/interjection.rs:279-301`）。
8. 插话信封由共享 crate 统一：`INTERJECTION_NOTE` + `<user_query>` + 尾部「完成未竟任务」提醒，25 000 字符截断；`INTERRUPT_NOTE` 复用同一信封（`crates/common/xai-interjection-core/src/format.rs:2-51`）。
9. **Esc 永不取消回合**——运行中只提示 registry 里 `CancelTurn` 的实际绑定并把 rewind 武装推迟 1000 ms；已在取消中时 Esc 被无提示吞掉（`app/agent_view/prompt.rs:738-741, 772-782`、`app/agent_view/input.rs:2276-2285`）。
10. 取消粒度只有 `Turn` 与 `Compaction` 两种，**没有「取消当前工具」**；Ctrl+C 在已取消中再一次升级为 `Action::Quit`（`crates/codegen/xai-grok-telemetry/src/events/turn.rs:38-43`、`app/agent_view/session.rs:543-554`、`app/agent_view/input.rs:1231-1247`）。
11. 取消延迟是「先 anchor 后结算」：只有 `UserGesture` 且 `surface()==Root` 才设 anchor，`TurnEnd::Completed` 才产出 `CancellationCompleted{latency_ms, scope}`，`Aborted`（reload/fork/会话失败）直接丢弃；anchor 两端都 `take()`，每事件最多一次（`app/cancel_latency.rs:8-25`、`app/agent_view/session.rs:550-571`）。
12. 回合完成的收尾是四件套——scrollback marker（`Worked for X` / `Turn cancelled by user in X.`）、终端通知、标题回 idle + OSC 9;4 清除、以及「仍有后台工作」的常驻计数行；其中 TurnComplete 通知在**队列非空时被抑制**，并且**故意延迟 3 帧（≈99 ms）**以跨过 Ghostty 标题的 75 ms debounce（`app/dispatch/prompt.rs:1675-1689, 1724-1773`、`app/app_view.rs:5495-5503`、`scrollback/blocks/session_event.rs:376-382`、`views/turn_status.rs:105-130`）。
