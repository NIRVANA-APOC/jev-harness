# 专题 06：动效、加载态与流式呈现

> 专题：动效、加载态与流式呈现｜调研日期：2026-09-22｜证据范围：`crates/codegen/xai-grok-pager/src/{views,notifications,tips,app,scrollback,acp}/`、`crates/codegen/xai-grok-pager-render/src/{render,theme,glyphs,appearance}/`、`crates/codegen/xai-grok-shell/src/util/config/resolve/display_refresh.rs`、`crates/codegen/xai-grok-pager/docs/user-guide/{05,06,21,26}`

路径均相对 `reference/grok-build/`。

## 1. 总览：三条独立时钟 + 五层帧预算

Grok Build 的动效系统不是「一个动画引擎」，而是**一个全局 tick 计数器 + 若干除法/脉冲函数 + 一套按需调度**。理解它要先分清三条时钟：

| 时钟 | 默认值 | 用途 | 证据 |
| --- | --- | --- | --- |
| 动画 tick | `1000 / animation.fps`，fps 默认 30（clamp 1..60） | spinner 换帧、波形相位、提示条 TTL | `crates/codegen/xai-grok-pager-render/src/appearance/config.rs:335-362`、`:1282` |
| 最小绘制间隔 `min_draw_interval` | 16ms（~60Hz）；探测到高刷屏时自动降到 8ms | 单帧绘制节流 | `crates/codegen/xai-grok-shell/src/util/config/resolve/display_refresh.rs:11-12`、`:19-22`、`:57` |
| 滚动时钟 `scroll_cadence` | 同默认为 16ms | 滚轮/触控板残余行冲刷 | `src/app/event_loop.rs:1454-1461`、`:1834-1836`、`:2394-2403` |

调度语义（`src/app/event_loop.rs:3052-3067`）：

```rust
let interval = match app.tick_demand() {
    TickDemand::None => return,                                  // 完全不 arm，事件循环 park
    TickDemand::Fast => match app.tick_interval_ceiling() {       // /gboom 33ms、minimal 16ms
        Some(ceiling) => interval.min(ceiling),
        None => interval,
    },
    TickDemand::Slow => interval.max(SLOW_TICK_INTERVAL),         // 83ms ≈ 12fps
};
```

`TickDemand` 是**有序枚举** `None < Slow < Fast`（`src/app/app_view.rs:244-257`），`SLOW_TICK_INTERVAL = 83ms` 与欢迎页 logo 的 `SHIMMER_FPS = 12` 对齐；`Fast` 由约 40 个条件触发（运行中 agent、滚动回放、模态 spinner、待发取消、语音录音……`src/app/app_view.rs:5636-5784`）。**空闲即零唤醒**是这个设计的核心：idle agent + 无动画条目时事件循环不 tick。

「帧预算」没有单一常量，而是五层机制叠加：

1. **需求门控**：`TickDemand::None` 直接不排 tick（`event_loop.rs:3053-3056`）。
2. **绘制节流**：`Presenter::request_throttled(now, min_draw_interval)`，不够间隔就把绘制排到 `last_draw_at + min_draw_interval`（`event_loop.rs:606-615`）。
3. **在途闸门**：`in_flight_target` 记录已排队 escape 序列的序号水位，上一帧未写完就不再画（`event_loop.rs:498-515`、`:579-600`）。
4. **视口作用域**：只有「在视口内的 running 条目」才要求重绘（见 §4）。
5. **除法降帧**：所有 spinner 都是 `tick / DIVISOR`，而非每 tick 换帧（见 §2）。

## 2. 逐帧动画清单

### 2.1 spinner 家族：统一 `tick / 4`

全代码库的事实标准是 `SPINNER_DIVISOR = 4`，在 30fps 下约 133ms 一帧、约 7.5fps，注释明确说明「降低了速度」：

```rust
// src/views/turn_status.rs:29-31
/// Show each spinner frame for this many animation ticks.
/// At ~30fps, 4 ticks is ~133ms per frame, about 7.5 spinner fps.
pub(crate) const SPINNER_DIVISOR: u64 = 4;
```

| 位置 | 常量 | 帧集 | 证据 |
| --- | --- | --- | --- |
| turn status 行 | `SPINNER_DIVISOR` (4) | braille 8 帧 | `src/views/turn_status.rs:367-377` |
| MCP chip | 同上（import） | braille | `src/views/agent_status.rs:22`、`:343-355` |
| goal chip | 硬编码 `/ 4` | dot 8 帧 | `src/views/agent_status.rs:313-319` |
| dock 活动行 | 同上（import） | dot | `src/views/dock/mod.rs:14`、`:404-406` |
| `/btw` overlay | 局部 `= 4` | braille | `src/views/btw_overlay.rs:151-152`、`:356-359` |
| extensions 模态 | 局部 `= 4` | — | `src/views/extensions_modal.rs:2760` |
| tasks pane | 局部 `= 4` | — | `src/views/tasks_pane.rs:32` |
| dashboard 行 | 局部 `= 4` | — | `src/views/dashboard/animation.rs:5` |

帧集定义在 `crates/codegen/xai-grok-pager-render/src/glyphs.rs:148-159`（braille 8 帧 U+280B…U+2827）与 `:162-169`（dot spinner）；每帧都保证 1 列宽，避免布局跳动。

### 2.2 慢脉冲与「呼吸」

- **监控/后台工作脉冲** `○ ◎ ◉ ◎`：`MONITOR_PULSE_DIVISOR = 8`，约 3.75fps、约 1.07s 一循环，注释说明「空闲的 still-running 提示应该安静地呼吸，而不是读成活跃 turn 的 spinner」（`src/views/turn_status.rs:33-36`、`:267-275`）。
- **等待你操作的菱形脉冲**：不用帧表，而是按 tick 计算颜色亮度，在 `bg_base` 与 accent 之间 blend `0.3 + brightness * 0.7`（`src/views/turn_status.rs:38-47`），速度常量 `USER_WAITING_PULSE_SPEED = 0.08`，并有专门测试锁死该值（`:1575-1578`）。周期按 `pulse_brightness` 注释为 `π / (speed · fps)`（`pager-render/src/theme/tokyonight.rs:358-363`）。
- **运行中块的 accent 竖条波形**（最典型的逐帧动画）：逐行相位偏移的 `sin²` 亮度波，`WAVE_SPEED = 0.15` rad/tick，「约 40 tick 走完一个块」（`src/scrollback/wrappers/entry_renderer.rs:19-21`、`:721-732`）；`wave_rows` 默认 32 行一个波周期，是用户可配项。运行中块的 bullet 用同一波形（`:857-867`）。

### 2.3 标题栏（OSC 0）与终端进度（OSC 9;4）

- 标题栏 spinner：`TITLE_SPINNER_DIVISOR = 8` ≈ 264ms/帧，理由写在注释里——「终端（尤其 Ghostty）会对标签标题做 debounce，每 tick（~33ms）写一次标题会产生超过标签栏渲染能力的 OSC 0 写入」（`src/notifications/title.rs:12-15`、`:92-94`）。
- 标题栏 `⚠ Action Required` 闪烁：`ACTION_REQUIRED_BLINK_DIVISOR = 15`，30fps 下约 500ms 亮 / 500ms 灭 = 1s 周期；**窗口聚焦时不闪**，改为静态前缀以免打字时干扰（`title.rs:17-20`、`:184-190`）。
- 终端原生进度指示：仅对 Ghostty、WezTerm、iTerm2 ≥ 3.6 发送 `OSC 9;4`（`src/notifications/progress.rs:10-21`），tmux 下自动包 DCS passthrough（`:34-38`）；因 Ghostty 约 15s 无消息会重置，按 `PROGRESS_KEEPALIVE = 5s` 重发（`src/notifications/mod.rs:14-16`、`:151-166`）。这是「把加载态交给终端外壳」的一条独立通道，非帧动画。

### 2.4 墙钟驱动的动画（刻意脱离 tick）

- **欢迎页 logo**：`SHIMMER_FPS = 12`，帧号由墙钟相位算出，把重绘节流到 12fps；一次对角光泽扫过 `CYCLE = 4.0s`（其中 `SWEEP_FRAC = 0.32` 用于扫动，其余停在屏外），底下叠一个 `PULSE = 0.06`、`PULSE_SECS = 5.0` 的呼吸（`src/views/welcome/logo.rs:106`、`:109-140`）。
- **语音录音红点**：固定 0.7s 墙钟周期的正弦「呼吸」，注释明确写「不随动画 tick，也不与流式文本重绘同步」（`src/app/agent_view/mod.rs:216-227`）。

### 2.5 dashboard 的「画过才算」棘轮

dashboard 不按 tick 盲翻帧，而是由渲染阶段回填 `PaintedAnimations`，`tick()` 只在真正的帧边界返回 `true`：`SPINNER_DIVISOR = 4`、`NEEDS_INPUT_BLINK_DIVISOR = 10`（`src/views/dashboard/animation.rs:1-64`）。文件首行注释即「A projected row that was not painted does not count.」

### 2.6 状态行布局示意

```
⠧ Run cargo test 0.2s              1m20s ⇣12k [↓] [stop]
│  │               │                │      │    │    └ 取消按钮（hover 变红）
│  │               │                │      │    └ 后台按钮（仅运行中的 execute，hover 展开）
│  │               │                │      └ turn 计时 + token（右对齐，灰）
│  │               │                └ 填充
│  │               └ 阶段计时（灰，永不截断；Ask 工具时隐藏）
│  └ 活动标签（按类型着色，可截断）
└ spinner（braille，tick/4）；工具阻塞在权限/提问时换成脉冲 ◆
```

布局与顺序来自模块头注释与渲染顺序：`src/views/turn_status.rs:3-13`、`:399-417`、`:520-570`；「Ask 工具隐藏阶段计时」在 `:379-394`。

## 3. 明确不是动画的部分（避免夸大）

- **进度条是静态的**：`views/progress_bar.rs` 提供 1/8 单元精度的 `▏▎▍▌▋▊▉█` 渲染（`:1-54`、`:59-91`），但只有两个调用点——上下文占用条（`src/views/context_bar.rs:11`、`:178-189`）与 goal 进度条（`src/views/goal_detail.rs:624`）。两处都不持有 tick，值变了才重画。
- **滚动条完全无动画**：按 follow 模式提供两档静态样式（跟随=很暗，滚上去=较亮），无 tick/相位/渐变（`crates/codegen/xai-grok-pager-render/src/render/scrollbar.rs:6-7`、`:185-208`）；minimal 模式下整体隐藏（`:42-55`）。
- **完成闪光不是渐变**：`FINISH_FLASH_DURATION_MS = 400`（`src/scrollback/state/types.rs:75`）期间把已完成块的 accent 保持为「静态成功色」（`entry_renderer.rs:680-704`），过期时补一次重绘恢复原色（`src/scrollback/state/mod.rs:427-450`）。是定时切换 + 一次收尾重绘，不是颜色插值动画。
- **提示条是 TTL 倒计时，不是动画**：默认 90 tick ≈ 3s（`src/tips/ephemeral.rs:7-10`，每 tick 递减 `:103-116`）；SSH 提示 300 tick ≈ 10s（`src/tips/ssh_wrap.rs:22-24`）；word_select 提示 600 tick ≈ 20s（`src/tips/word_select.rs:17-19`）。某个 tip 甚至会因为「快速 Enter 会把几秒的多秒 turn 压缩成亚秒闪烁」而暂停 TTL（`src/tips/small_screen.rs:30-32`）。
- **选择高亮是 150ms 定时消失**：`DEFAULT_SELECTION_HIGHLIGHT_DURATION_MS = 150`（`src/app/agent_view/mod.rs:431`），仅触发一次重绘（`src/app/agent_view/selection.rs:42-59`）。
- **光标闪烁由终端负责**：`[ui].cursor_blink` 三态映射到启动时的 `CursorStylePolicy`，默认 `Inherit` **不发任何转义序列**，注释说强制设置曾被报告为光标闪烁问题（`src/app/mod.rs:1427-1445`；配置项见 `docs/user-guide/26-config-reference.md:604`）。

## 4. 流式推理文本：三层合帧，无打字机

结论：**没有逐 token 追加渲染，也没有逐字延迟显示的「打字机」**（全仓 grep `typewriter|teletype|char_reveal` 无匹配）。文本按批次进入 scrollback，绘制由每帧合并：

1. **接收批次**：单次事件循环最多 drain `ACP_DRAIN_BATCH_MAX = 32` 条消息，且「输入一到就截断」，注释写着「流式期间每帧会有几十条消息排队」（`src/app/event_loop.rs:1837`、`:2193-2207`）。
2. **绘制节流 + 在途闸门**：`presenter.request_throttled(now, min_draw_interval)`（默认 16ms）；上一次绘制尚有未写完的 payload 时不绘（`event_loop.rs:2208-2223`、`:579-618`）。同一处注释直接点明目的：「Cap paint rate so terminal input isn't starved during heavy ACP streaming」。
3. **视口作用域**：`ScrollbackState::needs_animation()` 仅在「有 running 条目且在视口内」时为真（`src/scrollback/state/mod.rs:475-479`），`tick()` 同理（`:419-453`），注释解释「以 ~30fps 重绘一个静止画面纯属浪费：帧 diff 会是空的」。

因此一次绘制可能合并多个 token chunk，屏幕上表现为**按帧吞吐的整段增长**，而不是字符逐个显现。增量文本的 UTF-8 边界另有专门的增量解码器缓冲（`src/acp/tracker.rs:156-158`），与渲染节流无关。

另外，滚动不走动画 fps 而走 16ms 节奏，注释明说「Runs on the 16ms redraw cadence, not the slower animation fps」（`src/app/event_loop.rs:2394-2403`）。

## 5. 加载态与等待态：一层行 + 一套文案

活跃期的视觉层次几乎全部集中在一行 turn status，加上 scrollback 内部的运行中块：

```
┌─ scrollback ─────────────────────────────────────────┐
│ ▍⠧ Run cargo test                            0.3s    │ ← 运行中块：accent 波形 + 动画 bullet
│ ▍ Subagent "explore the parser"              12s     │ ← 子代理 Started 块（同样波形）
├──────────────────────────────────────────────────────┤
│ ⠧ Run cargo test 0.2s          1m20s ⇣12k [↓] [stop] │ ← turn status 行（仅忙碌/有 watcher 时占 1 行）
└──────────────────────────────────────────────────────┘
   ❯  （prompt）
```

- 该行 `should_show()` 为假时高度为 0，完全不占位（`src/views/turn_status.rs:13`、`:723-736`）。
- 活动语义由 `compute_activity` 决定颜色与文案：`Thinking…` / `Responding…` / `Verifying…` / `Compacting…` / 工具（绿色 `Run …`、有 description 时转为中性）/ 重试（warning 色）/ `Cancelling…`（红）（`src/views/turn_status.rs:580-698`）。
- **等待原因被显式命名**，而不是笼统「Waiting…」：`WaitingReason` 7 种 + 对应文案（`src/acp/tracker.rs:48-73`、`:116-136`）——`Waiting for response…`、`Waiting on subagent…`、`Waiting on task output…`、`Waiting on tasks…`、`Sleeping…`、`Running <event> hook…`、`Waiting for the agent to accept the prompt…`。
- **短等待不显示**：hook 批次未超过 `HOOK_REVEAL_DELAY = 300ms` 一律不画，避免「快 hook 闪一下」（`tracker.rs:68-75`）。
- **工具参数写作阶段也有独立文案**：`Writing subagent prompt…`、`Preparing MCP tool…`、`Searching MCP tools…`（`tracker.rs:156-191`）。
- 子代理生命周期在 scrollback 里是四态块：`Started`（运行中动画 bullet）/ `Completed` / `Failed` / `Cancelled`，阻塞式一个块变色、后台式保留 started 块另加完成块（`src/scrollback/blocks/subagent.rs:1-12`、`:252-277`）。
- 后台工作在 turn status 上的「still running」计数按类型分列，且 cue 顺序按数量开头，窄屏裁尾也保住数字（`src/views/turn_status.rs:105-140`、`:1337-1352`）。

**刻意不做动画的两处**（重要设计倾向）：

- tasks 顶栏的运行计数用**静态菱形**——「头部整个会话都在视野里，这里放 spinner 读起来是噪音而非进度」（`src/views/agent_status.rs:165-172`）。
- 阻塞在权限/提问时，把 spinner 换成脉冲菱形并把波形**冻结**成实色竖条，读作「暂停等你」而不是「还在加载」（`src/views/turn_status.rs:365-376`、`src/scrollback/wrappers/entry_renderer.rs:715-720`、`:839-856`）。

## 6. 完成 / 失败 / 取消的瞬时反馈

### 6.1 `turn_completion.rs`：terminate 收敛点，产出的是**一条 scrollback 标记**，不是特效

- 两条轨（fire-and-forget 的 `prompt_complete` 广播 + 可持久重放的 `TurnCompleted`）都收敛到 `finalize_turn_from_terminal`，以便中途 attach 的 viewer 也能从重放里正确结束 turn（`src/app/turn_completion.rs:1-7`、`:484-552`）。
- marker 选择是一张纯函数表：`EndTurn/Refusal/MaxTokens/... → TurnCompleted`；`Cancelled` 且 `send_now` → **不产生 marker**（取消并立即发送是静默的）；`RateLimit → None`；`Error` 且已有 error banner → `None`（避免重复报错）（`:87-109`）。hook 拒绝另走 `TurnBlockedByHook`（`:129-147`）。
- 文案：`Worked for 2m5s` / 无计时钟时 `Turn completed.` / 取消是 `<cause 短语> in X.`（`src/scrollback/blocks/session_event.rs:377-400`）。
- driver 侧还有「丢失 RPC 对账」：收到 terminal 但 `session/prompt` 响应未到时先 arm `pending_turn_end_reconcile`，由 tick 的 reconcile 收尾（`:365-460`）；只要该操作 armed，`view_tick_demand` 就返回 `Fast`，保证对账不被饿死（`src/app/app_view.rs:5643-5649`）。

### 6.2 `cancel_latency.rs`：33 行，纯遥测，零 UI

- 文件只有三个类型：`CancelOrigin`、`TurnEnd { Completed, Aborted }`、`CancelLatency { requested_at, scope }`（`src/app/cancel_latency.rs:1-33`）。文件头注释直接写「the settle rule behind the `CancellationCompleted` telemetry event」。
- 打点：仅「用户手势 + Root surface」时记录锚点，同一 turn 只留最早一次（`src/app/agent_view/session.rs:543-554`）；settle 时 `Completed` 产出 `latency_ms`、`Aborted`（reload/fork/session 失败强置 idle）直接丢弃（`:555-570`）。
- **它不参与任何视觉反馈**。取消的可见反馈全部来自状态行：`TurnCancelling` → 红色 `Cancelling…` 标签 + `[stop]` 保持可点（作为「取消没生效」的重试点）（`src/views/turn_status.rs:298-306`、`:588-591`，测试 `:984-1021`）。
- 与取消相关的两处恢复逻辑同样跑在动画 tick 上：超期取消重发、超期 prompt ack 对账（`src/app/event_loop.rs:2405-2422`）。

### 6.3 长任务写入受阻时的诊断通道

终端不再读取 PTY 时（writer 队列有积压且零进展，超过 `WRITER_BLOCKED_WARN_AFTER`），会发一次 `TermWriterBlocked` 遥测（`src/app/event_loop.rs:2151-2170`）。这是「看起来卡住」场景的观测手段，不影响画面。

## 7. 「减少动效」与慢终端降级

**代码里不存在 reduce-motion 概念**：全仓 grep `reduce.motion|prefers-reduced|low.power` 无匹配，也没有任何「关闭动画」开关。`docs/user-guide/21-terminal-support.md` 完全不涉及动效/帧率/性能（该文件章节为颜色、剪贴板、tmux、按键、鼠标、语音、RTL 等）。

实际存在的降级/收敛路径共五条：

1. **配置降帧**：`[animation] fps` 1–60、`wave_rows`（`docs/user-guide/05-configuration.md:686-692`、`06-theming.md:231-235`；clamp 见 `appearance/config.rs:1282-1283`）。
2. **按显示器刷新率自动定档**：启动时探测刷新率，`ms = clamp(round(1000/hz), 8, 16)`，仅当 hz 落在 55..=240 且开关打开；可由环境变量 `GROK_MIN_DRAW_MS` / `GROK_SCROLL_CADENCE_MS` 覆盖（夹取 1..100ms），也可用 `[ui.display_refresh].auto_cadence_enabled` 关掉退回 ~60Hz（`display_refresh.rs:55-75`、`:300-321`；`src/app/display_refresh_startup.rs:14-37`、`:91-100`；`src/settings/defs.rs:1003-1020`）。注意这挂的是**显示器刷新率**，不是「终端慢」。
3. **离屏不驱动**：视口外的 running 条目不产生 tick 需求（`src/scrollback/state/mod.rs:460-479`）；完成闪光同样只算视口内的条目（`:427-450`）。
4. **minimal 模式**：tick 上限收紧到 16ms（`src/app/app_view.rs:5563-5571`）、滚动条整体隐藏（`render/scrollbar.rs:42-55`）。
5. **老终端字形替换（非降帧）**：legacy ConHost 用 CP437 回退字形（spinner 用 `| / - \`、monitor 脉冲用填充变化、进度条用 `░▒▓`）以免 tofu（`crates/codegen/xai-grok-pager-render/src/glyphs.rs:100-117`、`:148-175`；`src/views/progress_bar.rs:18-29`）。

另有一个相邻的节流值得注意：状态行脚本（`$GROK_STATUS_LINE`）的事件驱动重算被限为 `EVENT_DEBOUNCE = 300ms`，强制/到期刷新降到 100ms，注释说明目的是「让忙碌的 turn 不要每帧都跑一次脚本」（`src/app/status_line.rs:17-21`）。

## 8. 未找到证据的事项

- 无「打字机式」逐字显现或可配打字速度。
- 无 reduce-motion / 无障碍动效开关。
- 无「终端慢则自动降帧」的检测路径（只有显示器刷新率探测）。
- 无完成时的庆祝/闪光特效；turn 完成的视觉产物只有一条文本标记。
- 未找到独立的「帧预算」常量或预算超支丢弃逻辑；预算效果由 §1 的五层机制间接达成。

## 关键结论（供合稿引用）

1. 动效由一个全局 tick 计数器驱动，默认 `animation.fps = 30`（`1000/fps` ms，clamp 1..60），但**空闲时事件循环零唤醒**：`TickDemand::None` 直接不 arm tick。证据：`crates/codegen/xai-grok-pager-render/src/appearance/config.rs:335-362`、`src/app/app_view.rs:244-257`、`src/app/event_loop.rs:3052-3067`。
2. 所有 spinner 都用 `tick / DIVISOR` 降帧，事实标准 `SPINNER_DIVISOR = 4`（30fps 下 ≈7.5fps）；标题栏用 8、监控脉冲用 8、dashboard 需求闪烁用 10。证据：`src/views/turn_status.rs:29-36`、`src/notifications/title.rs:12-20`、`src/views/dashboard/animation.rs:5-7`。
3. 流式文本**不是逐 token 重绘**：ACP 单批最多 drain 32 条消息，绘制受 `min_draw_interval`（默认 16ms）节流并带在途水位闸门，且只在视口内有 running 条目时才要求重绘。证据：`src/app/event_loop.rs:1837`、`:2193-2222`、`:579-618`、`src/scrollback/state/mod.rs:419-479`。
4. 真正「逐帧」的动画只有四类：spinner 帧序列、运行块的 accent 幅度波形（`WAVE_SPEED = 0.15` rad/tick，约 40 tick 一个块）、脉冲式亮度（等待菱形 `speed = 0.08`、欢迎页 logo）、以及 dashboard/状态栏的换帧。证据：`src/scrollback/wrappers/entry_renderer.rs:19-21`、`:721-732`、`src/views/turn_status.rs:38-47`、`src/views/welcome/logo.rs:106-140`。
5. 其余都只是「定时切换或顺序出现」：400ms 完成闪光（保色 + 过期补一帧）、90/300/600 tick 的提示条 TTL、150ms 选择高亮、静态进度条与静态滚动条。证据：`src/scrollback/state/types.rs:75`、`src/scrollback/state/mod.rs:427-450`、`src/tips/ephemeral.rs:7-16`、`src/app/agent_view/mod.rs:431`、`src/views/progress_bar.rs:1-54`、`crates/codegen/xai-grok-pager-render/src/render/scrollbar.rs:185-208`。
6. 等待态的语言是「一个 spinner + 精确命名的原因 + 颜色语义」，7 种 `WaitingReason` 各有文案，短等待用 300ms 延迟门隐藏以杜绝闪烁。证据：`src/acp/tracker.rs:48-75`、`:116-136`、`src/views/turn_status.rs:580-698`。
7. 有两处刻意**反过度动画**的设计可作参照：任务顶栏计数用静态菱形（「头部常驻，spinner 读作噪音」），等待用户输入时把波形冻成实色竖条（读作「暂停等你」而非「加载中」）。证据：`src/views/agent_status.rs:165-172`、`src/scrollback/wrappers/entry_renderer.rs:715-720`。
8. `app/turn_completion.rs` 是 turn 终结的唯一收敛点，产物是一条 scrollback 文本标记（`Worked for X` / 取消原因 / hook 阻断），并按 stop reason 决定是否静默（send_now 取消、rate limit、已有 error banner 都不出标记）。证据：`src/app/turn_completion.rs:1-7`、`:87-109`、`:484-552`、`src/scrollback/blocks/session_event.rs:377-400`。
9. `app/cancel_latency.rs` 只有 33 行、纯遥测（`CancellationCompleted` 的锚点与 settle 规则），不驱动任何 UI；取消的可见反馈只是状态行的红色 `Cancelling…` 与保持可点的 `[stop]`。证据：`src/app/cancel_latency.rs:1-33`、`src/app/agent_view/session.rs:543-570`、`src/views/turn_status.rs:298-306`。
10. 不存在 reduce-motion 概念；降级手段是配置 fps、按显示器刷新率自动定档（`GROK_MIN_DRAW_MS`/`GROK_SCROLL_CADENCE_MS`，8–16ms，clamp 1..100）、离屏不驱动、minimal 模式收紧到 16ms，以及老终端字形回退（不改帧率）。证据：`xai-grok-shell/src/util/config/resolve/display_refresh.rs:11-22`、`:300-321`、`src/app/app_view.rs:5563-5571`、`crates/codegen/xai-grok-pager-render/src/glyphs.rs:100-175`。
11. 「把加载态交给终端外壳」是一条独立通道：仅 Ghostty / WezTerm / iTerm2 ≥3.6 收到 `OSC 9;4` 不确定进度，tmux 下包 DCS passthrough，并按 5s 保活重发。证据：`src/notifications/progress.rs:10-39`、`src/notifications/mod.rs:14-16`。
12. 对 jev-harness 的直接启示：动效成本应靠「需求门控 + 除法降帧 + 绘制节流 + 在途水位 + 视口作用域」五件事压住，而不是靠一个帧预算常量；并且应为「等待原因」建类型化枚举（对应 Jev 的 Choice/Noul 判定输出），让状态行文案随判定结果切换。
