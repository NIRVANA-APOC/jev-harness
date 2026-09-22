# 专题 25：Grok Build 的渲染管线与性能架构

> 专题：渲染管线与性能架构｜调研日期：2026-09-22｜证据范围：`crates/codegen/xai-grok-pager/src/app/{event_loop.rs, event_loop_stall.rs, display_refresh_startup.rs, csi_filter.rs, edit_highlight_worker.rs}`、`src/views/{fps_hud.rs, scroll_debug_hud.rs}`、`src/scrollback/{render.rs, entry.rs, search.rs, state/{layout.rs, types.rs, mod.rs}}`、`crates/codegen/xai-grok-pager-render/src/render/{draw.rs, terminal_output.rs, osc8.rs}`、`src/input/{mouse.rs, scroll_log.rs}`、`src/terminal/tmux.rs`、`crates/codegen/xai-ratatui-inline/src/{terminal.rs, common.rs}`、`crates/codegen/xai-grok-shell/src/util/config/resolve/display_refresh.rs`、`benches/{bench.md, render.rs, resize.rs, search.rs, edit_highlight.rs}`

## 1. 一句话总览

整个 TUI 的绘制是**按需驱动**的：事件循环本身是一个 bias 排序的 `tokio::select!`（`crates/codegen/xai-grok-pager/src/app/event_loop.rs:2110`），所有分支只做一件事——把状态改掉、把 `Presenter` 标脏；真正的绘制发生在**每轮循环末尾唯一一次** `presenter.present_if_dirty(...)`（`event_loop.rs:2938`），而这次绘制还要再穿过三道闸门（脏标记、绘制节奏、writer 在途帧）。空闲时终端收到的是**零字节**——这一点有专门的测试保证（`crates/codegen/xai-grok-pager-render/src/render/draw.rs:689-706`）。

事件循环自己的模块注释把职责边界写得很清楚：

> A thin `tokio::select!` loop. All input routing, rendering, and state management is delegated to [`AppView`]. The event loop only handles IO: terminal events, the ACP channel, spawned task results, animation ticks, and hot-reloadable config changes.
> —— `event_loop.rs:1-4`

## 2. 主循环结构

### 2.1 事件来源与合并

一轮循环里被 select 的 future 按注释给出的优先级顺序排列（`event_loop.rs:2113-2890`，`biased;` 在 2111 行声明）：

| # | 来源 | 位置 | 作用 |
|---|---|---|---|
| 1 | `connection_cancel.cancelled()` | `:2116` | leader 断开，直接 break |
| 2 | `quit_notify.notified()` | `:2122` | 信号处理器请求退出（刻意排在 ACP 洪流之前） |
| 3 | `writer_event_rx.recv()` | `:2129` | writer 线程的 `Written(seq)` / `Failed` 回执 |
| 4 | `writer_blocked_report`（定时器） | `:2159` | writer 卡住 5 s 未写动的现场诊断 |
| 5 | `acp_rx.recv()`，门控 `if input_rx.is_empty()` | `:2173` | agent 流消息；**被输入门控而非简单重排** |
| 6 | `tasks.join_next()` | `:2201` | 派发出去的后台任务完成 |
| 7 | `bg_update_rx` | `:2288` | 后台版本检查 |
| 8 | `input_rx.recv()` | `:2311` | 终端输入（reader 线程 → channel） |
| 9 | `stall_flush` / `resize_debounce` / `deferred_draw` / `suspend_retry` | `:2379-2395` | 四个定时器 |
| 10 | `scroll_tick` | `:2397` | 滚动残量 flush + 80 ms 手势间隙检测 |
| 11 | `animation_tick` | `:2404` | 动画/恢复类轮询 |
| 12 | `voice_rx.recv()` | `:2889` | **刻意放最后**，注释说明它几乎永远 ready，放前面会饿死其它分支 |

关于第 5 项的取舍，代码里留下了明确的设计记录：

> Gating, not reordering: moving input above ACP would flip the starvation direction (streaming redraws starving behind held keys)
> —— `event_loop.rs:2171`

ACP 侧还有两条批处理策略：立即抽取最多 32 条已就绪消息（`ACP_DRAIN_BATCH_MAX = 32`，`event_loop.rs:1837`；循环在 `:2197`），且**一旦输入到达就中断抽取**（循环条件里的 `input_rx.is_empty()`），保证滚轮/按键最多等一批。

### 2.2 一轮循环的伪代码

```
loop {
    loop_entry = Instant::now()

    // ── 循环顶：处理上一次遗留的挂起状态（非阻塞） ─────────────
    run_pending_suspends(...)        // 编辑器/转录的 tty 交接
    run_pending_mode_switch(...)     // fullscreen <-> minimal
    schedule_tick(&mut animation_tick_at, &app, tick_interval)

    // ── 为每个「有 deadline 的东西」准备一个 future ────────────
    // 没有 deadline 的一律 std::future::pending()，永不就绪
    animation_tick   = sleep_until(animation_tick_at)  // 配置 fps，默认 30
    scroll_tick      = sleep_until(scroll_clock_deadline(now))  // 默认 16ms
    resize_debounce  = sleep_until(resize_debounce_at)  // 现在+16ms
    deferred_draw    = sleep_until(presenter.draw_scheduled_at)
    writer_blocked   = sleep_until(presenter.blocked_report_deadline())
    stall_flush      = sleep_until(stall_rollup.deadline())
    ... (billing / gate / status-line / dashboard / recap / load-barrier)

    // ── 每轮一次的水位观测（用于卡死诊断） ─────────────────────
    presenter.observe_writer_progress(queued, written, now)

    select! { biased;
        cancel | quit                     => break
        writer_event                      => presenter.acknowledge(seq)
        writer_blocked                    => 上报一次 term.writer.blocked
        acp  (仅当 input_rx 为空)         => handle + 最多再抽 32 条
                                             state_changed ⇒ presenter.request_throttled()
        task_complete                     => dispatch
        input                             => drain_and_process(...)   // 批处理 + 合并
                                             needs_draw ⇒ presenter.request()
                                             resize_only ⇒ 只排 debounce
        resize_debounce                   => presenter.request(false)
        deferred_draw                     => presenter.request(false)  // 被节流后的补画
        scroll_tick                       => if app.tick_scroll() { presenter.request(false) }
        animation_tick                    => app.tick() / 各类 reconcile
        voice (最后)                       => …
    }

    // ── 循环尾：唯一的绘制点 ───────────────────────────────────
    if stall_rollup 到期 { emit_event_loop_stall(window) }
    处理 app.pending_effects
    presenter.present_if_dirty(&mut app, terminal)   // ← 真正的 draw
    schedule_tick(&mut animation_tick_at, &app, tick_interval)
}
```

### 2.3 「这一轮要不要重绘」的判定

判定完全落在 `Presenter` 上（`event_loop.rs:499-647`）。它是三个独立条件的与：

```
dirty == true
  ∧ 没有在途未确认的帧 (in_flight_target == None)
  ∧ writer 队列已排空 (written >= queued_before)
```

对应 `try_present`（`event_loop.rs:579-600`）：先检查 `written < queued_before` 直接返回 false，再检查 `in_flight_target.is_some() || !self.dirty` 返回 false；通过后清 `dirty`、取走 `force_full_repaint`、执行 `draw`，并用 `queued_after()` 记录在途水位。回执由 `acknowledge(sequence)` 清闸（`:530-541`，要求 `sequence >= target`）。

这个「在途帧」闸门是**背压**：终端不再读 pty 时，事件循环不会堆积无限多帧，只会继续处理输入与定时器。与之配套的是 `WRITER_BLOCKED_WARN_AFTER = 5s`（`event_loop.rs:487`）与 `observe_writer_progress`（`:542-565`）——队列有积压且水位零推进才算一次 stall episode，一旦有推进就重新锚定，避免慢终端累积成假警报。

## 3. 渲染节流与合帧

### 3.1 三套彼此独立的时钟

| 时钟 | 默认 | 来源 | 控制什么 |
|---|---|---|---|
| 绘制上限 `min_draw_interval` | 16 ms（≈62.5 fps） | `GROK_MIN_DRAW_MS`，或探测到的显示器刷新率 | 一帧 `draw_frame` 的最快重复间隔 |
| 动画 tick | 30 fps | `[animation] fps`（1–60） | `app.tick()` 的节奏 |
| 滚动 flush | 16 ms | `GROK_SCROLL_CADENCE_MS` | 滚轮/触控板的合帧节奏 |

绘制上限的解析在 `crates/codegen/xai-grok-shell/src/util/config/resolve/display_refresh.rs`：默认常量 `DISPLAY_REFRESH_DEFAULT_CADENCE_MS = 16`（`:12`），允许区间 1–100（`display_refresh_startup.rs:15-16`）。若开启了显示器刷新率探测（默认开，`display_refresh.rs:15-16`），则 `ms = clamp(round(1000/hz), 8, 16)`（`:59-63`），hz 需落在 `[55, 240]` 内，否则不启用 auto。两个 env 都设置时 env 胜（`merge_motion_cadence`，`:282-306`）。解析结果在启动时固化为 `MotionClocks`（`display_refresh_startup.rs:19-23, 91-100`），事件循环读一次即用整个生命周期（`event_loop.rs:1460`）。

动画 tick 的节奏不是无条件 30 fps：`TickDemand::{None, Slow, Fast}`（`app/app_view.rs:245-257`）决定是否排下一次 tick——`None` 时 `schedule_tick` 直接 return，事件循环**完全 park（零唤醒）**；`Slow` 抬到 `SLOW_TICK_INTERVAL = 83ms`（约 12 fps），注释说明是为对齐欢迎页 logo 的 `SHIMMER_FPS`（`:255-257`）。

### 3.2 脏标记粒度：全局一个 bool，不是区域级

`Presenter.dirty` 是一个 `bool`（`event_loop.rs:501`），粒度是**整个屏幕**，没有区域级脏矩形。真正的细粒度在下一层：ratatui `Buffer` 的逐 cell diff（见 §4.2），只把变化过的 cell 变成转义序列。绘制请求只有两个入口：

- `request(force_full_repaint)`（`:601-605`）——立即标脏；
- `request_throttled(now, min_draw_interval)`（`:606-615`）——在上限内则标脏并返回 true，否则把 `draw_scheduled_at` 记成 `last_draw_at + min_draw_interval`，由循环顶的 `deferred_draw` future 到点补画（arm 在 `:2387-2391`）。

`force_full_repaint` 只在少数场景置位，例如窗口重新聚焦且终端会带外重绘 pane 时（`:2344-2349`），此时 `present_if_dirty` 先 `terminal.clear()` 再 `overlay::reset_owner()`（`:625-632`）。

### 3.3 resize：16 ms 防抖 + 本轮立即重画 owner

连续拖拽会每秒产生几十个 `Event::Resize`，每个都重建全部 entry 的布局。做法是**防抖**：`RESIZE_DEBOUNCE = 16ms`（`event_loop.rs:34`），每个新 resize 都重置定时器，布局只重建一次（arm 在 `:2353-2357`）。有一条 const 断言把这个值钉在 `SLOW_TICK_INTERVAL` 之下，理由是「防抖的那次绘制必须先记录新宽度，强制重跑才读得到」：

```rust
const _: () = assert!(
    RESIZE_DEBOUNCE.as_millis() < crate::app::app_view::SLOW_TICK_INTERVAL.as_millis(),
    "the debounced draw must record the new width before the forced re-run reads it"
);
```
（`event_loop.rs:35-38`）

一个例外：若屏幕上有已提交的 iTerm2 图像覆盖层，resize 时**立刻**画一帧（`:2358-2368`），注释给出的理由是 iTerm2 在拖拽期间会把已提交像素抹开。

### 3.4 与 synchronized output（DEC 2026）的配合

每帧被 `BeginSynchronizedUpdate` / `EndSynchronizedUpdate` 包住，使「cell diff + 图像 + 光标移动」原子呈现（`render/draw.rs:37`，代码在 `:513-537`）。是否包裹由终端上下文单点决定，两个标记永远同进同出：

```rust
pub fn should_emit_synchronized_output(ctx: &TerminalContext) -> bool {
    !(ctx.is_tmux_backed() && ctx.embedded_editor.is_none())
}
```
（`crates/codegen/xai-grok-pager-render/src/terminal/tmux.rs:55-57`）

即 **tmux 直连时故意不发 DEC 2026**：tmux 在 block 关闭时会重绘整个 pane，并且它自己已经朝外层终端做了同步（`draw.rs:38`）。测试用字面量钉住序列：`\x1b[?2026h` / `\x1b[?2026l`（`draw.rs:711-712`，用例 `synchronized_markers_follow_terminal_context`，`:709`）。

## 4. 输出层：`Buffer` → 终端字节

### 4.1 `draw_frame` 的六步

```
draw_frame(terminal, cursor, ctx, render_fn)          draw.rs:501
  ① 可选 queue(BeginSynchronizedUpdate)                :513-516
  ② terminal.autoresize()          // 吞掉尺寸变化      :517
  ③ frame = terminal.get_frame(); render_fn(&mut frame, &mut link_spans)   :520-524
  ④ terminal.set_frame_links(&link_spans)              :525
  ⑤ has_changes = terminal.flush_with_links()          :528
     terminal.swap_buffers()                           :529  ← 交换双缓冲
  ⑥ if !has_changes && !post_flush && action==None {
        writer.discard(); return; }                    :529-533  ← 零字节空闲帧
     post_flush.write_to(...) → cursor.apply(action) → 可选 EndSyncUpdate → flush()
```

`draw_frame` 刻意绕过 ratatui 的 `Terminal::try_draw()`，模块头给出了完整理由：`try_draw()` 每帧无条件发光标转义（`Show`+`MoveTo`，或 `Hide`），而 `Show`/`MoveTo` 都会重置终端的**光标闪烁计时器**，30 fps 下 500 ms 的闪烁周期永远走不完，光标看起来是实心的（`draw.rs:1-30`）。替代方案是把光标管理完全交给 `CursorState`，用去重表把「无 cell 变化 + 位置不变」变成零光标指令（`draw.rs:439-486`）。

### 4.2 自定义 diff

`xai-ratatui-inline` 自己实现了 diff，而不是用 ratatui 的 `Buffer::diff`，原因是一条具体缺陷：

> Upstream ratatui (0.29) `Buffer::pos_of()` casts the flat index to `u16` before dividing by width, silently wrapping at 65 535. This replacement performs the division in `usize` so terminals with >65 535 cells render correctly.
> —— `crates/codegen/xai-ratatui-inline/src/terminal.rs:1040-1042`

```
diff_large(prev, next) -> Vec<(x, y, &Cell)>        terminal.rs:1043
  逐 cell 比较 current != previous；维护两个游标：
    to_skip      = 当前 wide char 的额外列（2 宽字符占 1 个 cell 位）
    invalidated  = 受影响宽度，保证宽字符被覆盖时其邻居也被重写
  命中则 push (area.x + i % width, area.y + i / width, cell)
```

同一层还有一个 OSC 8 超链接层：`link_ids`（每 cell 一个 u32，0 表示无链接）与 `link_tables` 参与 diff——**同一 cell 只要链接变了也要重写**（`terminal.rs:1071-1113`），这样链接的增删改无需带外重绘。为保持常见路径零开销，`flush_with_links` 有一个 fast path：当前帧与上一帧都没有链接时直接退回纯 `flush()`，逐字节等价（`terminal.rs:391-400`）。

`Terminal` 持有两个 `Buffer` 加两套 `link_ids`/`link_tables`（`terminal.rs:137-172`），`gap_after` 之外没有额外缓冲；`flush` 返回 `bool` 表示「有没有 cell 被写过」。

### 4.3 批量写与缓冲

写字节不在事件循环线程上。`TermWriter` 是一个把 `Write::write` 收进内存的 sink：

- 每个 `TermWriter` 自带 `Vec<u8>`，容量 **32 KiB**（`render/draw.rs:205`）；
- `flush()` 非空时 `send_payload` 把 `Vec` `mem::take` 出来投进 mpsc（`:224-233`），落空则不投（空闲帧零字节的关键）；
- `send_payload` 用 `debug_assert` 强制**单生产者**：所有 payload 必须来自事件循环线程，否则字节顺序敏感的转义会被打乱（`:171-185`）；
- 每个 payload 带单调递增 `sequence`，writer 写成功后 `mark_written` 推进 `written` 水位（`:105-109、342-360`）。

writer 线程（名字 `term-writer`）在 stderr 的 dup fd 上再套一层 **64 KiB** `BufWriter`，每次写入前取 stderr 锁（`draw.rs:374-437`，容量在 `:399` 与 `:402-404`）。这一层的意义是隔离阻塞：终端若不读 pty，**只有 writer 线程停住**，事件循环继续处理定时器、事件与 ACP（`draw.rs:150-152`）。

此外还有一条带外通道 `EscapeWriter`：克隆同一队列的 handle，供非绘制路径（如重新聚焦时重发鼠标捕获）投递转义；注释记录了现场教训——在事件循环里内联锁 stderr 会在终端停止读取时死锁（`draw.rs:244-248`）。

### 4.4 澄清：`terminal_output.rs` 不是终端输出层

`crates/codegen/xai-grok-pager-render/src/render/terminal_output.rs` 尽管名字像输出层，实际职责是**工具输出的转义解析器**：bash / 终端类工具返回的是原始 PTY 字节流，可能含 SGR 颜色、光标移动、行擦除、回车（进度条原地重写），ratatui 会把它们当字面量打印成 `[1m[36m`。因此该模块基于 `vte` parser 做了一个**面向行的最小 VTE 模拟器**，产出带样式的 `Line` 加去转义纯文本（`terminal_output.rs:1-12, 27-33`）。

- 与真正的屏幕/网格模拟器不同，它保留**无界的完整转录**，以便映射到 pager 的行模型（`:10-11`）；
- 两个上限只为挡住畸形转义：`MAX_ROWS = 50_000`、`MAX_COLS = 8_192`（`:16-18`）；
- 每次调用都新建模拟器，保证渲染路径与高度缓存路径的结果确定且幂等（`:24`）；
- 调用点：`src/scrollback/blocks/tool/execute.rs:121,516`、`src/scrollback/blocks/tool/list_dir.rs:171`、`src/views/block_viewer/mod.rs:816`。

## 5. 性能指标与现场诊断

### 5.1 FPS HUD（release 也可用）

`src/views/fps_hud.rs` 刻意做成 release 可开：dev-only 的 `FrameMetrics` 会改动帧管线（把分阶段计时刻进 `draw_frame`），而 dev-only 的 HUD 无法探测生产路径（`scroll_debug_hud.rs:13-16`）。它量的是**整个 `draw_frame` 调用的墙钟耗时**（渲染 + flush + writer 交接），只做观测、不改管线（`fps_hud.rs:4-6, 15`）。

一个容易误读的点，模块注释专门澄清：

> "fps" here is render throughput (1 / mean frame cost), not paint frequency. The pager draws on demand, so an idle UI paints nothing and a busy one is bounded by this number.
> —— `fps_hud.rs:12-13`

参数与开关：`SAMPLE_CAP = 120`（约 4 s @30 fps）、`REFRESH = 250ms`（避免每帧重排序与格式化）、`PANEL_WIDTH = 32`；`GROK_FPS` 真值判定为「非空且不等于 `"0"`」，`/debug fps` 可运行时切换（`fps_hud.rs:22-29, 47-69`）。统计量是均值 fps + p50 + p95，用线性插值百分位（`:101-132`）。开销约定：关闭时每帧一次 bool 判断（`:15`）。

`GROK_FPS` 的归属有意写成单一入口：debug/dev 构建里 env 归 `FrameMetrics`，release 里本 HUD 认领（`HONORS_GROK_FPS_ENV: bool = true`，`:27-29`），保证 `GROK_FPS=1` 永远不会静默失效（`:8-10`）。

**未找到证据**：`render::frame_metrics` 与 `FrameMetrics` 在本快照中被 `fps_hud.rs:3,8,27,49`、`scroll_debug_hud.rs:13,26` 反复引用，但全仓库搜索不到其源文件；`src/app/app_view.rs:2224-2226` 的 `dev_fps_rows()` 直接返回字面量 `0`（注释 "0 in non-dev builds"）。因此本文不描述该 dev overlay 的行为。

### 5.2 滚动诊断 HUD

`src/views/scroll_debug_hud.rs` 是「滚动游乐场」：右上角 46 列面板（`PANEL_WIDTH = 46`，`:24`）逐帧打印滚动状态机快照，并显示当前 scrollback 的视口事实（`:1-11`）。配方写在文件头：

```
GROK_FPS=1 GROK_SCROLL_DEBUG=1 grok --resume <session>      # 然后 /settings 里翻 scroll_mode 等
```

- `GROK_SCROLL_DEBUG` 控制启动即开，隐藏命令 `/scroll-debug` 运行时开关（`:27, 41, 50-52`）；
- 快照字段见 `crates/codegen/xai-grok-pager-render/src/input/mouse.rs:471-496`：`events_per_tick`、`wheel/trackpad_lines_per_tick`、`viewport_height`、`flush_cap`、`cadence_ms`、流状态（`kind`/`promoted`/`events`/`avg_interval_ms`/`accel`/`desired`/`applied`/`backlog`/`carry_lines`）等；
- `GROK_SCROLL_LOG=1` 额外把每个滚动事件写成 JSONL（`:7`，实现 `input/scroll_log.rs:1-11`）；
- 不变量：HUD 绝不改变滚动行为，快照是只读 `&self`，禁用时每帧一次 bool 判断（`:9-11`）。

### 5.3 事件循环卡顿检测

`src/app/event_loop_stall.rs` 是一个 60 秒滚动窗口的聚合器（`STALL_REPORT_WINDOW = 60s`，`:9`）：

- 每次输入被处理时测「等待时长」`input_wait(arrived_at, handled_at, loop_entry)`（`:35-44`）；
- `StallRollup::observe` 只保留窗口内**最差一次**卡顿，以及那一刻的活动快照（`compaction_active` / `subagents_active` / `mcp_servers_connected`，`:50-78`）；
- 注释说明了两个刻意的设计：窗口**从不切分**、到期必然 flush；`window_started_at` 在首次 observe 前为 `None`，**空闲循环因此不开窗口也不设定时器**（`:50-54`）。

调用点在事件循环里成对出现：输入分支里 observe（`event_loop.rs:2315-2320`），循环尾按到期 flush（`:2935-2937`），并有一对 `emit_event_loop_stall` / `flush_pending_stall` 包装（`:3076-3084`）。

与之并行的是 §2.3 提到的 writer 侧诊断：卡住 5 s 上报 `term.writer.blocked`（含 queued/written 水位），恢复时记 `term.writer.recovered`（`event_loop.rs:2159-2172, 2096-2105`）。注释明确这是「中期冻结家族」的现场诊断——**循环还活着但屏幕冻住**（`:2154-2156`）。

### 5.4 Benchmark：种类与数据

`crates/codegen/xai-grok-pager/benches/` 下是 4 个 criterion 基准 + 1 篇说明文档。

| 文件 | 基准名 | 测什么 | 语料规模（源码写死） |
|---|---|---|---|
| `render.rs` | `render/single_frame`、`render/full_scroll`、`render/windowed_scroll` | 视口渲染与滚动时的逐帧成本 | 120×50 视口（`:25-26`），`SCROLL_STEP = 10`（`:29`） |
| `render.rs` | `reveal/navigate_skip`、`reveal/navigate_rebuild` | 搜索 n/N 定位的「跳过 vs 全量 rebuild」 | `REVEAL_ENTRIES = 3000`，注释写「约 3,200 条 ≈ 5 MB 的长会话」（`:32-33`） |
| `resize.rs` | `resize/width_step`、`resize/drag_20_steps`、`resize/same_width_noop` | 拖拽边缘时的重排成本 | `TURNS = 400`，注释写「约 3,200 条 / 约 5 MB，数小时会话的规模」（`:15-19`） |
| `search.rs` | `search/scan`、`search/query_steady`、`search/query_cold` | 后台 daemon 前后的正则扫描与 UI 线程成本 | `CORPUS_ENTRIES = 30_000`（`:20`） |
| `edit_highlight.rs` | `edit_hl/matrix`、`edit_hl/upgrade`（`hunk_only` / `full_file_slice` / `paint_with_precomputed` / `upgrade_once_per_file` / `prefix_per_hunk`） | 编辑 diff 的语法高亮策略 | 500 行 + 8 hunk、10 000 行 + 40 hunk（`:290, 311, 332`） |

`resize.rs` 的模块注释是这套基准里最有信息量的一段——它直接列出了三个**曾在真实会话上测量并移除**的回归（`resize.rs:1-6`）：

1. 按宽度重新派生 entry 源文本，而不是复用其缓存的宽度轮廓（使估算 pass 变成 O(对话字节数)）；
2. 每个 entry 都构建或克隆一次 `AppearanceConfig`；
3. 每次 resize 都跑 `warm_measure_pages_above`，而不是等宽度稳定后只跑一次。

关于 `benches/bench.md` 必须做一处澄清：**它不是实测结果**。文件末尾自述 "Generated for benchmarking purposes."（`bench.md:337`），且性能数字用的都是期望语气。可引用的内容有两类：

- **结构性结论**（有价值）：管线六步中「步骤 1–3（markdown 解析、语法高亮、换行）跨帧缓存，只有步骤 4–5 每帧运行」（`bench.md:18`）；"Buffer diff + flush" 的复杂度是 **O(变化 cell 数) 而非 O(总 cell 数)**（`:16`）。
- **期望量级**（引用时须标注为期望）：120×30 视口 + 200 行 markdown，「预期 ~50–200 µs/帧」，其中约 80% 花在 `EntryRenderer::render()` 与 scratch buffer 拷贝、约 15% 在缓存命中路径的 `BlockContent::output()`、约 5% 在布局计算；「若 >500 µs/帧，可能有意外的 cache miss 或热路径分配」（`:305-314`）。

另外两处需注意的不一致/空白：`bench.md` 用 120×30 视口，而 `render.rs:25-26` 实际用的是 120×50；`benches/` 目录下**没有提交任何基准结果或 baseline 文件**（仅有 4 个 `.rs` 与 `bench.md`）。

## 6. 已知性能陷阱与代码里的对策

```
┌──────────────────── 用户按键/滚轮 ────────────────────┐
│ 每击键若同步跑正则扫描 ⇒ 长会话卡输入线程               │
│ 对策：后台 SearchDaemon + Arc 零拷贝语料（search.rs）   │
└───────────────────────┬───────────────────────────────┘
┌───────────────────────▼───────────────────────────────┐
│ 拖拽边缘 ⇒ 每秒数十次全量重排                          │
│ 对策：16ms 防抖 + 延迟 warm_above + 复用宽度轮廓        │
└───────────────────────┬───────────────────────────────┘
┌───────────────────────▼───────────────────────────────┐
│ 编辑 diff 整文件 syntect 会挡住首帧                     │
│ 对策：首帧 hunk-only，worker 线程升级（2 MiB/50k 上限） │
└───────────────────────┬───────────────────────────────┘
┌───────────────────────▼───────────────────────────────┐
│ 每帧重排/重解析 ⇒ 无谓 CPU                              │
│ 对策：wrap/output/高度分层缓存 + 懒测量 + 正则 OnceLock │
└───────────────────────┬───────────────────────────────┘
┌───────────────────────▼───────────────────────────────┐
│ 终端不读 pty ⇒ 事件循环被写阻塞                          │
│ 对策：writer 线程 + 在途帧闸门 + 队列水位诊断             │
└───────────────────────────────────────────────────────┘
```

### 6.1 任务线程化

- **滚动搜索**：可搜索文本按 `content_generation` 缓存为 `Arc<[IndexedEntry]>`（每条一个 `String`，内存随可搜索文本总量而非渲染行数增长），整块零拷贝交给后台 `SearchDaemon`；`update_query` 在 UI 线程只编译 matcher 并入队，`poll` 取结果（`src/scrollback/search.rs:1-14, 311-336`）。通道刻意用 unbounded，注释解释了理由：有界通道会让第 257 次击键被卡住，`try_send` 丢满则可能丢掉最后一次查询（`:211-214`）。基准注释写明：这段扫描以前**每击键同步跑在输入线程上**（`benches/search.rs:3-7`）。
- **编辑高亮**：首帧只用 hunk 范围高亮，绝不在 UI 线程做整文件；随后由独立 `std::thread` 跑一次 syntect pass（止于最后一个 hunk 行），按 `entry_id` 合并、**最新任务胜**，每 tick `try_recv` 收结果（`src/app/edit_highlight_worker.rs:1-16, 127`）。超出 `EDIT_HL_MAX_BYTES = 2 MiB` / `EDIT_HL_MAX_LINES = 50 000` 或读文件失败就**停留在 hunk-only，不做无界工作**（`benches/edit_highlight.rs:13`、`edit_highlight_worker.rs:11-13`）。结果带主题标记，主题不匹配的样式图会被绘制端跳过（`edit_highlight_worker.rs:53-57`）。
- **Mermaid**：同样的 worker + 磁盘缓存清理模式（`src/app/mermaid_worker.rs:98-99`）。

### 6.2 缓存分层

- **宽度轮廓缓存**：`ScrollbackEntry::cached_line_widths`（`src/scrollback/entry.rs:102`）避免每次换宽度都重新派生源文本；`invalidate_width_caches`（`:252-257`）在宽度变化时精确失效三样东西（`cached_output` / `cached_truncated_height` / `cached_estimate_lines`）。
- **高度缓存键**：`(content_width, raw, theme, cwd)`；`is_selected` 刻意不入键，注释理由是「行数从不依赖选中状态」（`entry.rs:390-396`）。
- **去转义结果缓存**：`render_terminal_lines` 每次新建模拟器以保证确定性（`terminal_output.rs:24`）。
- **正则**：OSC 8 路径识别的三条正则用 `OnceLock` 进程内缓存（`render/osc8.rs:212-249`）；其他若干处用 `LazyLock`（`views/prompt_widget/image_state.rs:197`、`views/prompt_widget/mod.rs:3650`、`prompt_images.rs:1615,1634,1716`）。
- **每帧分配**：搜索高亮的文本缓冲在所有可见行间复用，「每帧最多分配一次而不是每行一次」（`scrollback/render.rs:256-258`）。

### 6.3 避免每帧重排 + 局部渲染

`ScrollbackState` 的布局缓存 `LayoutCache`（`src/scrollback/state/layout.rs:44-70`）保存：每 entry 的 `EntryLayoutInfo`、截断高度、`measured: Vec<bool>`（高度是精确测量还是廉价估算）、`virtual_y` 前缀和、prompt 描述符、分组 span 与宽度。定位可见 entry 用 `virtual_y` 上的 `partition_point` 二分（`:75-99`）。

高度策略是**估算 + 懒升级**：只有进入（或接近）视口的 entry 才被精确测量（`:51-55`），测量窗口是可见范围向下再扩 `MEASURE_MARGIN_ENTRIES = 8`（`state/types.rs:79`，窗口计算在 `layout.rs:596-605`）。

渲染只走窗口：生产路径用 `paint_window(...)` 算出该画的 entry 区间，`render_scrolled_entries_with_scratch` 只遍历这个切片，`content_y0` / `entry_index_base` 给出该窗口在全局虚拟坐标里的位置——「没有全量的 EntryLayout vec，y 从 `content_y0` 起按缓存高度与 gap 前进」（`scrollback/render.rs:250-262`，窗口化基准见 `benches/render.rs:214-261`）。

被顶/底裁掉一半的 entry 用可复用的 `ScratchBuffer` 先整条渲染，再只把可见行拷进输出（`scrollback/render.rs:69`；`bench.md:150-171` 描述了这一 cell-by-cell 拷贝循环，并把它标为最热路径）。

## 7. 大历史（长会话）下的内存与滚动设计

### 7.1 内存：淘汰「看得见的代价」，保留「算得出的代价」

条目本身不删。淘汰的是屏幕外 entry 的**重型渲染缓存**：

```
       视口 + MEASURE_MARGIN_ENTRIES(8)
              │
   ┌──────────┴──────────┐
   │   测量窗口           │
┌──┴─────────────────────┴──┐
│  保留区 = 测量窗口 ± 128    │   EVICT_KEEP_MARGIN_ENTRIES
└──┬─────────────────────┬──┘
   │  之外：evict_render_cache()
   └─ 选中 entry 始终跳过（selection 例外）
```

- `evict_offscreen_render_caches`（`src/scrollback/state/layout.rs:621-648`）遍历全部 entry，跳过保留区与当前选中项；
- `evict_render_cache`（`entry.rs:258-268`）只丢 `cached_output` 与 block 内部可重建缓存，返回是否真的丢了东西（供 sweep 统计）。注释明确：**布局（entry 高度、滚动位置）不动**，滚回视口时会透明重渲染；
- 触发节奏：`CACHE_EVICT_INTERVAL = 5s`（`app/app_view.rs:5059-5089`），注释解释「sweep 是 O(entries) 的指针级遍历，相对一次帧渲染微不足道，但也没有理由每帧跑」；
- 动机写在 doc 注释里：多 MB 转录的重型拷贝「轻易达到数百 MB，而这些内容不滚动就永远看不到」（`layout.rs:622-624`）。

分配器侧还有一层：jemalloc 会把释放的页留在进程里，所以库在「重型瞬时对象刚被丢弃」的边缘触发 purge，而**从不按帧触发**；draw/tick 路径内部命中的内存悬崖改用 `request_release_after_draw(reason)` 推迟到本次 flush 之后，避免 purge 卡住帧（`src/memory_release.rs:1-9, 38-60`）。

### 7.2 滚动：状态机 + 固定 cadence + 每次 flush 的 delta 上限

模块头一句话点明策略：「Redraw is coalesced to a fixed cadence.」（`crates/codegen/xai-grok-pager-render/src/input/mouse.rs:5`）

- 常量集中在 `mouse.rs:46-77`：`STREAM_GAP_MS = 80`（手势结束判定）、`REDRAW_CADENCE_MS = 16`、`MIN_DELTA_PER_FLUSH = 6`、`DEFAULT_*` 与加速度参数；
- 每次 flush 的行数上限 `flush_cap = max(viewport_height / 2, MIN_DELTA_PER_FLUSH)`（`:429-430`）——**超出的行留在流里，由后续 cadence flush 继续送**（注释见 `:61-63`）；
- `on_scroll_event_at` 只在距上次重绘达到 cadence（或手势刚升级）时才产生行（`:622`）；`on_tick_at` 负责 cadence flush 与 80 ms 间隙收尾（`:657-694`）；
- 事件循环侧的 arm 直接对应：`scroll_tick` 里 `if app.tick_scroll() { presenter.request(false) }`，注释强调「跑在 16 ms 重绘节奏上，不是较慢的动画 fps」（`event_loop.rs:2393-2404`）；`tick_scroll` 再把行数派发给视图并返回是否需要重绘（`app/app_view.rs:5512-5527`）。

### 7.3 resize 与大历史的交叉：延迟预热

`warm_measure_pages_above` 会在底部钉住时预测量上方几页，让第一次上滚无故障。但它在 resize 路径上代价最大——「每次拖拽事件都预热三个屏幕外的页，然后在下一次宽度变化时全部丢掉」。因此 resize 时把它置为 `Deferred`，只在下一次 `begin_frame()` 之后才真正执行（`src/scrollback/state/mod.rs:1466-1476, 1555-1568`）。这正是 `benches/resize.rs:1-6` 记录的第三条已修复回归。

===

## 关键结论（供合稿引用）

1. 主循环是单个 bias 排序的 `tokio::select!`，绘制只发生在每轮循环末尾唯一一次 `present_if_dirty`；空闲时终端收到零字节（`event_loop.rs:2110`、`:2938`、`draw.rs:689-706`）。
2. 「要不要重绘」是三条件与：`dirty` ∧ 无在途未确认帧 ∧ writer 队列已排空；在途帧构成对慢终端的背压（`event_loop.rs:579-600`）。
3. 绘制上限与动画节奏是两套时钟：绘制默认 16 ms（`GROK_MIN_DRAW_MS`／显示器刷新率，钳 1–100 ms），动画来自 `[animation] fps` 默认 30（`display_refresh.rs:12, 59-63`、`config.rs:335-361`）。
4. 脏标记粒度是全局一个 `bool`，没有区域级脏矩形；细粒度靠 `Buffer` 逐 cell diff 实现（`event_loop.rs:501`、`terminal.rs:1043-1068`）。
5. DEC 2026 同步更新成对包裹每帧，但 **tmux 直连时刻意不发**，因为 tmux 会重绘整个 pane 且自身已同步（`draw.rs:513-537`、`terminal/tmux.rs:55-57`）。
6. 输出层绕过 ratatui `try_draw()` 是为保住光标闪烁：`try_draw` 每帧无条件发 `Show`/`MoveTo` 会重置闪烁计时器（`draw.rs:1-30`、`:439-486`）。
7. 写字节在独立 `term-writer` 线程上，32 KiB 帧缓冲 + 64 KiB `BufWriter`，每帧带单调序号、写后推进水位、以 `Written(seq)` 回执解闸（`draw.rs:194-233`、`:374-437`）。
8. `terminal_output.rs` 不是终端输出层，而是工具输出的按行 VTE 模拟器（原始 PTY 字节 → 带样式 `Line`），上限 `MAX_ROWS = 50_000` / `MAX_COLS = 8_192`（`terminal_output.rs:1-33`）。
9. 诊断三件套：release 可用的 FPS HUD（120 样本环、250 ms 刷新，量 `draw_frame` 吞吐而非绘制频率）、滚动 HUD + JSONL 日志、60 s 窗口的卡顿聚合器（`fps_hud.rs:12-29`、`scroll_debug_hud.rs:1-27`、`event_loop_stall.rs:9, 50-78`）。
10. 长会话不删条目，只淘汰屏幕外 entry 的重型渲染缓存（保留区 = 测量窗口 ± 128，选中项例外），5 s 一次，高度与布局缓存刻意保留（`layout.rs:621-648`、`types.rs:83`、`app_view.rs:5059-5089`）。
11. 三条重路径被移出 UI 线程：滚动搜索 daemon（`Arc` 语料零拷贝）、编辑 diff 整文件高亮 worker（2 MiB / 50k 行上限，超限保持 hunk-only）、Mermaid worker（`scrollback/search.rs:1-14`、`edit_highlight_worker.rs:1-16`）。
12. `benches/bench.md` 不是实测数据（自述 "Generated for benchmarking purposes."，数字均为 "we expect"），可引用的只有结构性结论（步骤 1–3 跨帧缓存、diff 为 O(变化 cell)）；仓库未提交任何基准结果文件（`bench.md:18, 16, 305-314, 337`）。

**未找到证据**：`render::frame_metrics` / `FrameMetrics` 的源码不在本快照中（`app_view.rs:2224-2226` 的 `dev_fps_rows()` 恒返回 `0`）；`benches/` 下无实测输出。
