# 专题 21：会话生命周期与终端状态管理

> 专题：会话生命周期与终端状态管理｜调研日期：2026-09-22｜证据范围：`crates/codegen/xai-grok-pager/src/{startup.rs, export_cmd.rs, share_cmd.rs, lib.rs}`、`src/app/{mod.rs, event_loop.rs, session_startup.rs, session_load_barrier.rs, screen_mode_relaunch.rs, mode_switch.rs, terminal_restore.rs, teardown_fence.rs, exit_timeout.rs, signal_handler.rs, reader_thread.rs, session_title_resolve.rs, prompt_ack.rs, display_refresh_startup.rs, dispatch/{router.rs, transcript.rs, session/load.rs, session/lifecycle.rs}}`、`src/views/rewind.rs`、`src/scrollback/{export.rs, state/mod.rs, blocks/{agent.rs, thinking.rs, markdown_content.rs}}`、`src/acp/tracker.rs`、`src/slash/commands/{export.rs, share.rs, rewind.rs, exit.rs, screen_mode_switch.rs}`、`crates/codegen/xai-grok-pager-render/src/terminal/pop_fence.rs`、`crates/codegen/xai-crash-handler/src/terminal.rs`、`crates/codegen/xai-grok-pager-bin/src/main.rs`、`docs/user-guide/{03-keyboard-shortcuts.md, 17-sessions.md, 21-terminal-support.md, 27-grok-clone.md}`

## 1. 全景

Grok Build 的「会话」有两个入口（新会话 / `--resume`）和三条出口（优雅退出、信号、panic），
而终端在整段时间里处于被 TUI 接管的状态。本文按 **启动 → 恢复 → 运行 → 退出** 的顺序拆解实现，
并把终端状态还原单独成章（第 4 章），因为那是这类 TUI 最容易留下灾难性残留的地方。

### 1.1 三种屏幕模式（先建立词汇）

`ScreenMode` 有三个变体（`crates/codegen/xai-grok-pager/src/app/mod.rs:340-349`）：

| 变体 | 含义 | 是否占备用屏 | 鼠标捕获 |
| --- | --- | --- | --- |
| `Fullscreen` | 经典 alt-screen 全屏 TUI | 是（`?1049h`） | 开 |
| `Inline` | 停在主屏、`Viewport::Inline(rows)` 的底部内联视口 | 否 | 开 |
| `Minimal` | 实验性 scrollback-native（`--minimal`），终局块用 `insert_before` 打进终端原生回滚 | 否 | **关** |

三个变体各有一个稳定的 telemetry 标签 `fullscreen` / `inline` / `minimal`（`app/mod.rs:376-382`，
注释警告改名会让外部分析流塌成 `"other"`）。全局「当前实际所在屏」另存一份原子量
`CURRENT_SCREEN_MODE`（`app/mod.rs:156-166`），因为 panic hook / 信号处理拿不到参数，必须读全局。

## 2. 启动序列：从进程启动到首屏可交互

### 2.1 进程入口

`crates/codegen/xai-grok-pager-bin/src/main.rs:2025` 的 `main()` 先处理三件与 TUI 无关的事：
mermaid / voice 的「子进程自执行」旁路（`main.rs:2028-2033`）、`PagerArgs::parse_cli()`、
以及版本/doctor 的早退。`/export`、`/share`、`sessions` 等子命令在这里分流
（`main.rs:2315-2319`），随后才调 `xai_grok_pager::app::run(args, bg_update_rx)`（`main.rs:2519`）。

### 2.2 `run()` 的串行前段——首屏之前真正花掉时间的地方

`app/mod.rs:663` 起是 TUI 主入口。首屏之前的工作**全部是串行 await**（没有 spl​​ash，只有 stderr 上的行输出）：

| 步骤 | 位置 | 备注 |
| --- | --- | --- |
| 吞掉一次性屏模式环境变量 | `app/mod.rs:669` | `GROK_SCREEN_MODE`，见 §5.3 |
| 读配置 | `app/mod.rs:672,747` | 读两次（预热 remote settings 后再读一次） |
| pre-TUI 外部登录 | `app/mod.rs:685-696` | 可能发生真实用户交互 |
| `redirect_native_stderr()` | `app/mod.rs:697` | 保住 fd 2 给后续输出 |
| auth 刷新 | `app/mod.rs:698-703` | 带 timeout |
| remote settings 预热 + join | `app/mod.rs:708-745` | 最长等 `EARLY_PREFETCH_WAIT = 2s`（`app/mod.rs:550`） |
| `materialize_startup()` | `app/mod.rs:824` | **最慢的一步**，含远端恢复 ≤ 90 s（§3.4） |
| 屏模式解析 + 主题预热 | `app/mod.rs:926-984` | §5.1 |
| spawn writer 线程 | `app/mod.rs:986-988` | 终端写入从此走 mpsc 到独立线程 |
| `init_terminal()` | `app/mod.rs:994-1001` | 一次性副作用清单见 §2.3 |
| 连接 agent（有界） | `app/mod.rs:1044-1062` | leader 失败自动回退 embedded（`app/mod.rs:1063-1088`） |

连接失败不是「黑屏挂住」：`bounded_connect`（`app/mod.rs:576-659`）把等待切成 100 ms 片以支持
managed profile 升级预算，超时后走 `restore_terminal(...)` 归还终端再把错误冒出去（`app/mod.rs:1106-1122`）。

```
进程启动
  │  mermaid/voice 旁路子进程 · PagerArgs::parse_cli · 子命令分流
  └─ app::run()                                      [app/mod.rs:663]
       ├─ ① 串行阻塞：配置 · 登录 · auth · settings 预热(≤2s) · materialize_startup(≤90s)
       ├─ ② 终端一次性副作用：spawn_writer_thread → init_terminal   [app/mod.rs:986-1001]
       │     raw mode → [alt screen] → 鼠标/焦点/粘贴 → Hide 光标
       │     → set_panic_hook → signal_handler::install → 收 typeahead
       │     → kitty flags → 建 viewport
       ├─ ③ bounded_connect（leader ↴ embedded 回退）
       └─ event_loop::run()            ← 首帧 request_presentation()  [event_loop.rs:1666]
             └─ Action::LoadSession(..) 之后才开始灌历史  [event_loop.rs:1692-1727]
```

### 2.3 `init_terminal()`：一次性副作用清单

`app/mod.rs:1459-1661`。顺序即语义，关键点：

1. `enable_raw_mode()`（`app/mod.rs:1468`）——**最先**，之后任何失败都必须走 teardown；
2. `EnterAlternateScreen`（仅 fullscreen，`app/mod.rs:1488-1492`）；
3. `EnableMouseCapture`（**非 minimal** 才开，`app/mod.rs:1497-1502`）；minimal 下若检测到
   「终端把鼠标报告当作文本漏出」（JediTerm/Windows）则额外发一次 `MOUSE_TRACKING_RESET`；
4. `EnableFocusChange` + `EnableBracketedPaste` + `cursor::Hide`（`app/mod.rs:1503-1508`）；
5. 光标样式三态策略 `CursorStylePolicy::{Inherit,ForceBlinking,ForceSteady}`（`app/mod.rs:1430-1446`）——
   默认 `Inherit` 不发任何样式转义，避免覆盖用户 shell 的选择；只有强制过才置 `CURSOR_STYLE_FORCED`，
   teardown 因此才知道该不该复位（`app/mod.rs:1523`）；
6. `set_panic_hook()` 与 `signal_handler::install(mode)`（`app/mod.rs:1528-1529`）——**在 raw mode 之后立刻**安装；
7. 收集「启动期打字」`startup_typeahead`（`app/mod.rs:1474-1476,1530-1535,1569-1572`）：
   在 raw mode 打开后、20 ms（有 `VTE_VERSION` 时）/ 0 ms 的静默窗口里排空 stdin；
8. 协商 kitty keyboard protocol（`app/mod.rs:1537-1568`）：失败/不支持则跳过，成功后 push flags 并记 `set_pushed_kitty_flags`；
9. 建 viewport（`app/mod.rs:1573-1648`）：fullscreen 用全屏 Terminal；否则 `Viewport::Inline(rows)`。
   **minimal 的探针失败会降级**：改用 `Viewport::Inline(rows)` 并重新打开鼠标捕获、把有效模式改成
   `Inline`（`app/mod.rs:1606-1625`），随后 `finish_theme_after_probe` 补上被跳过的主题解析
   （`app/mod.rs:410-418`）。

任何一步出错都走 `.inspect_err` 分支：直接 `emit_terminal_teardown_sequences` + `disable_raw_mode` +
`signal_handler::mark_restored()`（`app/mod.rs:1650-1655`），**不**走完整 restore（此时还没有 reader/writer 消费者）。

窗口标题也在这里设置，并且强制清洗控制字符与截断（`app/mod.rs:1662-1679`）：
`SetTitle` 的字符串是裸放在 OSC 里的，内嵌 BEL/ESC 会提前终结 OSC 并注入任意转义序列。

### 2.4 首帧与「启动期打字」的回放

`event_loop::run` 里，第一件事是 `presenter.request_presentation(...)`（`event_loop.rs:1666`），
之后才是 bundle status / billing / changelog 这些异步拉取。reader 线程在 typeahead 回放之后才 spawn
（`event_loop.rs:1612-1625`），保证用户抢在加载期敲的字**排在实时按键之前**；
若当前停在登录/信任/付费墙屏，则丢弃这些事件而不是让它们被误答（`event_loop.rs:166-177`）。

### 2.5 加载态如何呈现

**未找到**任何全屏启动 splash 或进度条的实现。加载态由三样东西承担：

- **欢迎屏直接可交互**：欢迎屏里先建一个「乐观 home 会话」，代码称 *husk*
  （`app/dispatch/session/lifecycle.rs:431`、`abandon_agent_as_unused_husk` @ `lifecycle.rs:855`），
  用户不必等后端就有输入焦点；
- **stderr 上的裸行输出**：远端恢复/git 校验等进度走 `emit_pre_tui_restore_line`
  （`app/session_startup.rs:1267-1276`），在 TUI 接手前先打在终端上；
- **失败时才出现的专属界面**：`startup_failure` 模块把连接失败分类成可读报告
  （`app/mod.rs:1106-1122` 退出时交给上层打印）。

## 3. 会话恢复

### 3.1 CLI 意图 → materialize

启动意图在 `session_startup.rs` 收敛成一个枚举 `SessionStartupIntent`
（`app/session_startup.rs:184`），由 `PagerArgs::session_startup_intent()` 生成（`app/mod.rs:818-824`），
再交给 `materialize_startup`（`app/session_startup.rs:849`）得到一个 `MaterializedStartup`
（`app/session_startup.rs:685`）：`Resume` / `Fork` / `NewWithId` / `NewAuto`。

`Resume` 的解析分三层（`resolve_existing_session` @ `app/session_startup.rs:982`）：

1. **本地 id** 命中 → 直接用；
2. **标题**命中 → `select_by_title`（`app/session_title_resolve.rs:35-76`）：简单小写比较；
   多个同名时**唯一的手动 `/rename` 胜出**；仍有歧义则报错并列出 id，绝不静默挑一个；
   UUID 形状的参数永远走 id 路径、绝不按标题解释（`session_title_resolve.rs:12-14`）；
3. 都没命中 → 按 `RemoteMissPlan` 决策（`app/session_startup.rs:1086-1113`）。

命令行上先看到的那行提示来自 `app/session_startup.rs:1330`：
`Resuming session <id> (matched by title)`。

### 3.2 历史如何重放进 scrollback

事件循环在首帧之后才发 `Action::LoadSession`（`event_loop.rs:1714-1727`），随后 ACP 回放
带 `_meta.isReplay = true` 的通知（`acp/meta.rs:29,45-51`）。三条实现要点：

**（a）重放全程用「延迟渲染」变体**，避免每条 chunk 都跑一遍 markdown：

```rust
// crates/codegen/xai-grok-pager/src/acp/tracker.rs:1132-1136
if meta.is_replay {
    scrollback.push_chunk_to_agent_deferred(id, &text)
} else {
    scrollback.push_chunk_to_agent(id, &text)
}
```

`markdown_content.rs:135-142` 的注释把意图写得很直白：*「Used for historical replay during
`session/load` so the pager can batch markdown work and render once after replay completes.」*

**（b）重放的 thinking 块用另一个构造器**（`scrollback/blocks/thinking.rs:152-160`）：
`streaming_replay()` 把 `started_at` 置 `None`，否则本地壁钟计时器会冻成 ~0 ms，
渲染出假的「Thought for 0.0s」。

**（c）`SessionLoaded` 要等回放排空才派发**——`app/session_load_barrier.rs` 整篇就是这件事：

- 队列头是**本会话的 replay** → 一直等，**不设超时**（`session_load_barrier.rs:174-179` 的 `ReplayHead => true`）；
- 队列头是**无关会话的流量** → 只在事件循环有余力 drain 时累计秒表，超过
  `SESSION_LOADED_ACP_BARRIER = 2s` 就放行（`session_load_barrier.rs:26,178-180`）；
  输入饿死时秒表不走，避免恢复被卡住；
- 头是**本会话的 live** → 立即放行，因为 leader 模式保证了线序
  `[unicast replay] → [load response] → [buffered live]`（`session_load_barrier.rs:3-8`）。

### 3.3 恢复时的视觉差异

- **退出时的续跑提示**（`app/mod.rs:1239-1262`）：fullscreen 模式下还会先打「会话标题 / 最后提问 /
  最后回复」三段摘要，摘要只在 fullscreen 构建（`event_loop.rs:3095-3116`），随后统一给：

```
<会话标题>
> 最后一条用户提问
  最后一条回复

Resume this session with:
  grok --resume 0199c0ff-...
```

  minimal 模式多带一个 `--minimal`（`app/mod.rs:1257-1261`）。宽度按终端列数截断，缺省 80。
- **启动时**：`Resuming session <id> (matched by title)` 打在 TUI 之外的 stderr
  （`app/session_startup.rs:1330`）；远端恢复期间是 `Session "<id>" not found locally, restoring
  conversation from remote...` 加逐行进度（`app/session_startup.rs:1156-1166`）。
- **无参数 `--resume`** 取当前目录最近会话（`app/session_startup.rs:795` 的 `most_recent_session_id`）。
- **欢迎屏**列当前目录的近期会话，`Ctrl+R`（= `/resume`）在任何时候都能打开会话选择器
  （`docs/user-guide/03-keyboard-shortcuts.md:253,361`）；另有一个 `foreign_resume_hint`，
  在提示区提供「上次那个别的工具的会话」一键续跑（`views/welcome/mod.rs:710-711,2187`）。

### 3.4 状态不一致时

| 情况 | 处理 | 证据 |
| --- | --- | --- |
| 标题歧义 | 报错 + 列出候选 id，提示改用 id 续跑 | `session_title_resolve.rs:69-73` |
| 本地找不到、允许远端 | 远端恢复，**只恢复会话状态与记忆，从不恢复代码** | `session_startup.rs:1264-1266,1280-1282` |
| 远端超时/失败但已有本地子会话 | 降级为「继续用这个本地会话」并明确告知 | `session_startup.rs:1240-1256` |
| `--restore-code` 但无 `--worktree` | 拒绝原地检查代码快照（`RejectInPlaceCodeRestore`） | `session_startup.rs:1102-1112` |
| 欢迎屏留下了乐观空会话 | `abandon_unused_empty_for_load` 丢弃 husk，但**有草稿就保留** | `dispatch/session/lifecycle.rs:825-853` |
| 恢复把上一条「还在跑」的任务行读回来 | 该动作随上个进程一起死了，专门有一个清理函数 | `dispatch/turn.rs:834-835` |

## 4. 退出路径与终端状态还原

这是本专题的重头戏。核心设计是：**「向终端写什么」只有一个函数，而「谁在什么条件下调它」有三条路径**。

### 4.1 单一真源：`emit_terminal_teardown_sequences`

`app/terminal_restore.rs:61-134`，注释明说：*「So the on-wire byte order is defined exactly once.」*
它**不含** `disable_raw_mode()`——raw mode 必须留到所有读操作之后才关。严格顺序：

```
OSC 9;4 清除进度条（无条件，终端不支持时也是无害空操作）
EndSynchronizedUpdate (?2026l)
reset cursor color（仅本进程真正设置过时）
写 MOUSE_PASTE_RESET 裸序列:  ?1000l ?1002l ?1003l ?1015l ?1006l ?2004l
   └ 若 MOUSE_CAPTURE_ENABLED 曾置位：Windows 额外补一次 DisableMouseCapture 走 winapi
DisableFocusChange (?1004l)
pop /gboom 额外键盘层（若游戏开过，且它叠在基础层之上）
PopKeyboardEnhancementFlags (CSI<u)          ← 用 swap 保证至多一次
分屏模式收尾：
  fullscreen : [SetCursorStyle::DefaultUserShape 若强制过] → cursor::Show → LeaveAlternateScreen
  inline     : [同上] → MoveTo(0, inline_cursor_row) → cursor::Show → 换行
Windows: restore_stdin_mode()（最外层快照，放最后）
```

`?1000/?1002/?1003/?1015/?1006 / ?2004 / ?1004 / ?25 / ?1049 / ?2026 / CSI<u` 这一整组是
**规范清单**，写在 `crates/codegen/xai-crash-handler/src/terminal.rs:6-46`，注释要求「每条 teardown 路径
都必须禁用同一组模式」，并给出两个常量：`MOUSE_PASTE_RESET`（`terminal.rs:38-39`）与完整
`RESTORE_SEQ`（`terminal.rs:44-46`）。其中 kitty 的 `CSI<u` pop 排在 `?1049l` **之前**，
因为「协议栈是按屏计数的」。

**inline 的光标落点是专门的坑**（`app/terminal_restore.rs:112-127`）：minimal 模式视口不是贴底的，
把光标移到屏幕最后一行会让 shell 提示符和续跑提示离 pager 最后一行隔一整屏空白；
所以传入的 `inline_cursor_row` 取的是 `terminal.viewport_area().bottom()`（`terminal_restore.rs:163`），
且被 `min(last)` 夹住。信号路径拿不到 terminal 句柄，只能回落到屏幕底行（`signal_handler.rs:229-230`）。

### 4.2 三条路径的差异

| 路径 | 入口 | reader join | 排空 writer | fence | 有界保护 |
| --- | --- | --- | --- | --- | --- |
| 事件循环后（正常退出） | `app/mod.rs:1169-1174` 调 `restore_terminal` | 是（250 ms） | 是（2 s） | 是 | writer 超时才用 `run_bounded_teardown` |
| panic hook | `terminal_restore.rs:243-258` | 否 | **否**（会死锁） | 否 | 整个 teardown 放在 helper 线程 + 2 s 上限 |
| 信号 | `signal_handler.rs:219-239` | 否 | 否 | 否 | 无（靠 exit 看门狗兜底） |

`restore_terminal_with`（`terminal_restore.rs:143-185`）把顺序固定为：
**join reader → fullscreen 下 clear → 抓取 inline 落点 → drain writer → teardown → fence（仍在 raw mode）→
`disable_raw_mode()` → `mark_restored()` → 恢复 fd 2**。
`mark_restored()` 之后到达的信号就不会再画转义序列到用户的提示符上（`signal_handler.rs:84-88`）。

### 4.3 所有等待都有界

- **writer join**：`WRITER_JOIN_GRACE = 2s`（`terminal_restore.rs:27`）。超时不报错，而是记
  `term.writer.join_timeout` 并把线程**脱钩**（`terminal_restore.rs:32-46`）；后续所有 stderr 写可能阻塞，
  所以之后的 teardown 必须走 `run_bounded_teardown(..., 2s)`（`terminal_restore.rs:138,169`），
  否则「终端不再读」会让 `/quit` 变成永久挂起。
- **reader join**：`READER_JOIN_GRACE = 250 ms`（`app/reader_thread.rs:16`）。线程用
  `poll(20ms)` 再 `read()`（`reader_thread.rs:68-72`）而非裸阻塞读，这样「暂停标志」和「通道关闭」
  最多一个轮询周期就被观察到。`join_within` **从不 join 一个还在跑的线程**，超时即 detach 并返回
  `ReaderJoin::TimedOut`（`reader_thread.rs:103-123`）。
- **typeahead / 编辑器交还**：reader 的 `paused`/`parked` 协议让外部编辑器、pager 接管 stdin
  （`reader_thread.rs:45-64`），handoff 延迟上限就是一个 `POLL_TIMEOUT`。
- **crossterm 解析垃圾**：连续 50 次读错误才退出 reader，否则只 warn（`reader_thread.rs:81-94`，
  注释点名 ratatui#1275 与 VTE/SSH PTY 的脏输入）。

### 4.4 最后一道闸门：kitty pop fence（DA1 往返）

这是整套设计里最精巧的一环，位于 `crates/codegen/xai-grok-pager-render/src/terminal/pop_fence.rs`。

**要解决的问题**（`pop_fence.rs:1-8` 原文）：终端按顺序处理输出，因此当 `CSI ? c`（DA1）的回复到达时，
终端在应用 `CSI < u`（pop）之前发出的**所有按键报告都已经躺在 stdin 里**了；不排空它们，
这些 key release 事件会被用户 shell 当成击键——注释举的例子是 **fish 把 `ESC [ 100 ; 5 : 3 u` 读成
Ctrl-D 然后退出**。读取用裸 fd，因为 crossterm 会把 DA1 回复从事件流里过滤掉。

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
| 其余 | `Run`（`POP_FENCE_TIMEOUT = 1s`，注释称取自 neovim 的实战值） |

只要 fence 没真跑或结果是 `Unsupported` / `QueryFailed`，就退回旧式的「静默排空」
`drain_pending_events(10ms)`（`teardown_fence.rs:13-16,60-63`）。结果统一记到
`teardown.kitty_pop_fence`，字段含 `decision` / `outcome` / `residue_bytes` / `reader_join`
（`teardown_fence.rs:78-95`），用**直写**通道发出，因为启动失败时 ACP forwarder 没有发送端。

### 4.5 信号路径

`app/signal_handler.rs` 的模块注释（`:1-16`）把动机写全了：没有这些处理，SIGINT/SIGTERM/SIGHUP
（WSL/SSH 断开）会留下 raw mode + 备用屏 + 鼠标捕获 + kitty flags，终端看起来「还活着」但进程已死。

设计要点：

- 信号在 **tokio task** 里处理（`signal_handler.rs:90-135`），不是真正的异步信号上下文，
  所以可以安全地走完整 `emit_terminal_teardown_sequences`（含加锁 stderr、条件复位光标样式）；
- **首个信号 = 优雅退出**（通知事件循环，走和 `/exit` 一样的收尾：teardown + 历史/遥测 flush），
  **第二个信号 = 立即强制退出**（`signal_handler.rs:98-101,198-209`）；
- `SIGTTIN` / `SIGTTOU` 显式忽略（`signal_handler.rs:75-79`）：子进程（或孙进程）`tcsetpgrp()`
  抢走前台进程组时，整个 pager 不能被挂起——那会留下一个 raw mode 的终端；
- **`SIGPIPE` 故意不装处理器**（`signal_handler.rs:12-16`）：保持 Rust stdlib 的 `SIG_IGN`，
  写坏管道只是 `Err(BrokenPipe)`；自定义处理器会把 fork+exec 子进程（MCP server、hook）继承的
  disposition 从 `SIG_IGN` 改成 `SIG_DFL`，重新引入旧回归；
- 退出码映射与 agent 二进制共享同一个函数，避免漂移：Ctrl+C `130`、SIGTERM `143`、SIGHUP `129`
  （`signal_handler.rs:137-150`）；
- 顺序上**先还原终端，再 flush Sentry**（最长 2 s），否则用户要盯着一个 raw + 备用屏的终端等 2 秒
  （`signal_handler.rs:216-218,230-231`）；
- Windows 分支不同（`signal_handler.rs:103-124`）：Ctrl+C 享受同样的「首次优雅 / 二次强制」，
  Ctrl+Close / Logoff / Shutdown 则**立即**处理，因为系统只给很短窗口。

### 4.6 exit 看门狗

`app/exit_timeout.rs`：优雅退出被武装后启动一个 20 s 兜底（`DEFAULT_EXIT_TIMEOUT`，`:8`），
超时后再等 5 s（`HARD_EXIT_GRACE`）执行 `libc::_exit`——注释特意说明 `_exit` 是
async-signal-safe 的且跳过 atexit（`:53-61`）。环境变量 `GROK_EXIT_TIMEOUT_SECS` 可调，
**`0` 表示关闭**（`:73-79`）；`GROK_TEST_HOLD_TEARDOWN_SECS` 用来在测试里故意拖住 teardown
（`:64-71`）。武装点在事件循环返回之后（`app/mod.rs:1165-1168`），以及信号路径里
（`signal_handler.rs:204`，注释：*「An orphan never gets the second, forcing signal; bound the quit.」*）。

### 4.7 失败模式清单

| 失败模式 | 应对 | 证据 |
| --- | --- | --- |
| 终端停止读取输出（SSH 掉线等） | writer join 2 s 超时 → 脱钩；teardown 有界化；最终由 20 s 看门狗 `_exit` | `terminal_restore.rs:27,143-185`；`exit_timeout.rs:8` |
| stdin reader 卡在 `read` | 250 ms 有界 join → detach，fence 放弃（`SkipReaderAlive`），退回 10 ms 静默排空 | `reader_thread.rs:103-123`；`teardown_fence.rs:37-48` |
| panic | hook 在 helper 线程跑有界 teardown（2 s），并主动 `kill_all` 子进程、记录崩溃采样 | `terminal_restore.rs:243-258` |
| 重复 teardown（panic + 信号、或双信号） | `TERMINAL_OWNED` 布尔 + kitty flags 的 `swap` + Windows stdin 模式快照的 `swap` 保证至多一次 | `signal_handler.rs:60-88,232-233`；`mod.rs:111`；`mod.rs:1378-1390` |
| panic=abort 下仍有输出需求 | 退出提示等打印全部 best-effort（写失败即忽略），注释点名 `panic = "abort"` | `app/mod.rs:1236-1238` |
| 崩溃路径（连 panic hook 都没跑完） | `xai_crash_handler` 的 `RESTORE_SEQ` 用裸 `write(2)` 写 fd 2 | `xai-crash-handler/src/terminal.rs:44-60` |
| SIGKILL | **未找到证据**表明有处理；信号任务只订阅 SIGINT / SIGTERM / SIGHUP | `signal_handler.rs:90-135` |

## 5. 屏幕模式切换与热重启

### 5.1 模式解析链（从高到低）

`app/screen_mode_relaunch.rs:280-319`：

```
GROK_SCREEN_MODE 环境变量（一次性，读完即删）
  > --minimal / --fullscreen
  > [ui] screen_mode（config.toml）
  > pager.toml [terminal] minimal（legacy）
  > alt-screen 策略（[terminal] alt_screen / --no-alt-screen / Zellij·tmux control 探测 / 鼠标泄漏终端）
```

注释点明 env 一次性且**读后即删**的原因：否则每个被 spawn 的子进程（工具 shell、worker、嵌套
`grok`）都会继承一个用户从未要求的强制模式（`screen_mode_relaunch.rs:17-20,268-278`）。

### 5.2 进程内切换（默认路径）

`/minimal` 与 `/fullscreen`（别名 `/full`）发出 `Action::RelaunchInScreenMode`
（`slash/commands/screen_mode_switch.rs:67-73`），事件循环里的处理是
`run_pending_mode_switch`（`event_loop.rs:909-1020`）：

```
Action::RelaunchInScreenMode
  ├─ park input reader（≤500ms）        失败 → Aborted（提示，留在原模式）
  ├─ writer wait_drained（≤750ms）      超时 → Aborted
  ├─ perform_screen_transition()                     [app/mode_switch.rs:108-207]
  │    ├─ Switched          → reseed_screen_mode + 全量重绘 + 一条 system 块 / toast
  │    ├─ Aborted           → 回滚到原屏与原鼠标状态
  │    └─ NeedsExecFallback → app.relaunch = Some(..) → 事件循环退出 → exec
  └─ GROK_SCREEN_MODE_SWITCH=exec 时直接走 exec 路径（escape hatch）
```

几个细节：

- 切到 Minimal 前会**先拿掉鼠标捕获**，且 `Clear(All)` 而**不是** `Clear(Purge)`——
  注释：purge 会毁掉用户真实的回滚缓冲（`mode_switch.rs:121-140`）；
- Minimal 分支在设置视口前先 `set_current_screen_mode(Minimal)`，这样「切换中途失败」的 teardown
  也知道终端实际在哪块屏上（`mode_switch.rs:141-143`；同一条原则见 `app/mod.rs:156-159`）；
- 成功后给用户一条可逆的确认：minimal 方向打 system 块
  `Switched to minimal mode · /fullscreen to go back`，fullscreen 方向打 toast
  `Switched to fullscreen mode · /minimal to go back`（`event_loop.rs:947-972`）；
- 切到 minimal 时会**关掉 fullscreen 专属浮层**（block viewer、图片/视频查看器、/gboom 游戏等），
  否则它们会「看不见却仍然吃键」（`mode_switch.rs:224-240`）；
- 重绘前要 `reseed_screen_mode`（`mode_switch.rs:29-53`）：重绑快捷键注册表、重置 hover、
  重算 thinking 折叠策略——**启动时按模式决定的东西，切换时都得重播一遍**。

### 5.3 exec 热重启（兜底）

`app/screen_mode_relaunch.rs`。适用场景只有三种（`screen_mode_relaunch.rs:1-10` 注释）：
启动期的 env 覆盖、`GROK_SCREEN_MODE_SWITCH=exec` 逃逸开关、以及不可恢复的切换。

- **argv 重建** `build_screen_mode_relaunch_args`（`:59-160`）会剥掉：`--minimal/--fullscreen`、
  `--continue/-c`、`--fork-session`、`--restore-code`（一次性指令，在新进程里重放会做第二次）、
  `--resume/--load/--session-id/-s/--worktree/--worktree-ref/--ref` 及其 `=` 形式与后续值、
  裸 positional prompt、以及 `--` 之后的一切；最后追加 `--resume <id>` 与目标模式 flag。
  哪些 flag 吃值是**从 clap 定义本身推导**的（`clap::CommandFactory`，`:25-54`），
  防止分类与 CLI 漂移。
- **Unix 用 `cmd.exec()`**，保住 PTY/fd 身份（PTY 测试因此能观察切换而不必重启进程，`:6-8,211-217`）；
- **Windows 没有 `exec`**，改为 spawn 子进程 + 父进程 `wait` 并以子进程码退出，同时
  先用 `SetConsoleCtrlHandler(None, 1)` 忽略父进程的 Ctrl+C、并 sleep 150 ms 让父进程的 reader
  退出，避免和子进程抢控制台输入记录（`:219-241`）；
- 失败时打印可粘贴的命令，带 `GROK_SCREEN_MODE` 与显式 flag 双保险
  （`screen_mode_relaunch_resume_hint`，`:179-187`；调用点 `app/mod.rs:1264-1277`）。

```
Resume this session with:
  GROK_SCREEN_MODE=fullscreen grok --fullscreen --resume <session-id>
```

## 6. rewind / checkpoint 的时间旅行界面

`/rewind`（别名 `/undo`）只截断**对话历史**，**不回滚磁盘上的文件**——
`docs/user-guide/17-sessions.md:143,158` 用了加粗的 "Important" 强调这一点。
磁盘上每个会话目录里的 `rewind_points.jsonl` 是它的数据来源（`17-sessions.md:35`）。

触发方式两条：命令本身，或**空闲 + 空输入框 + 有消息时 800 ms 内连按两次 Esc**
（`docs/user-guide/03-keyboard-shortcuts.md:230`；命令实现 `slash/commands/rewind.rs:8-16`）。
空输入框连按 Esc 若提示非空则语义变成「清空草稿并 stash」，两种行为共享同一个时间窗
（`03-keyboard-shortcuts.md:229-231`）；切换回合结束后约 1 秒内 rewind 分支被抑制，
防止「乱按 Esc 恰好回合结束」意外打开选择器（同文件 `:233`）。

界面状态机有 5 个阶段（`views/rewind.rs:56-78`）：`Loading` → `Picker` → (`CancelOffer`) →
`Confirm` → `Executing` → 失败则 `Error`。选择器复用 `/jump` 的列表浮层几何
（`rewind.rs:359-389`，注释：*「Shared list-overlay frame and row geometry (also used by /jump)」*）：

```
┌ 左侧一列 accent 竖条（accent_bar，行首 3 列留白）      ← rewind.rs:329-340
│  Rewind to which turn?
│  · <某个 user prompt 的预览>            ← 每行 "· " + 预览，超宽截断
│  · <下一条>                             ← 光标行加粗
│  ...
```

确认框（`Confirm before rewind` 默认为开，`17-sessions.md:156`；三个选项在
`rewind.rs:118-119` 定义）：

```
Rewind conversation to "<prompt 预览>"?
  ( ) y  Yes
  ( ) a  Yes, and don't ask again        ← 选它同时关掉确认设置
  ( ) n  No
```

回合仍在跑时会插一步（`rewind.rs:391-433`）：

```
A turn is currently running.
Would you like to cancel it before rewinding?
  ( ) y  Cancel turn and rewind
  ( ) n  Let it finish
```

键位（`rewind.rs:121-171`）：`j/k` 或方向键移动，`Enter` 确认当前项，`Esc` 取消；
鼠标可点可悬停（`rewind_row_at` / `set_rewind_cursor`，`rewind.rs:215-289`）。
后端契约是 ACP 风格的结构体 `RewindPointInfo`（含 `prompt_index` / `created_at` /
`num_file_snapshots` / `prompt_preview` / `has_file_changes`）与 `RewindResponse`
（`success` / `target_prompt_index` / `reverted_files` / `clean_files` / `conflicts` / `mode` /
`prompt_text`）（`rewind.rs:10-54`）——界面模型已经为「有文件快照」预留了字段，
但当前 UX 文案仍明确「不动文件」。

## 7. 导出与分享

### 7.1 `/export [filename]`（TUI 内）

`slash/commands/export.rs:5-8` 的模块说明：带文件名则写 UTF-8 `.md`（支持 `~` 展开、含空格路径、
自动建父目录），不带则整份拷进剪贴板。实现只是发 `Action::ExportConversation { file_path }`
（`export.rs:41-48`），真正干活在 `app/dispatch/transcript.rs:152-197`：

- 遍历 scrollback 的 `RenderBlock`，交给纯函数 `render_blocks_to_markdown`
  （`scrollback/export.rs:13-62`）；
- 空内容 → 打 system 块 `No conversation content to export`；
- 写文件成功 → system 块 `Conversation exported to <path>`；
- 写失败 → **只用通用措辞** `Failed to write file: <e>`，注释明说「不要回显用户给的路径，
  可能含密钥或 PII」（`transcript.rs:192-196`）；
- 参数补全复用 `/model` 那套路径补全思路，目录带尾 `/` 让下拉继续深入（`export.rs:54-80`）。

### 7.2 产物形态

`render_blocks_to_markdown`（`scrollback/export.rs:13-62`）确定产物只有三种小节：
`## User`、`## Assistant`、`## Tools`；连续 assistant 消息合并到同一个标题下；
工具调用压缩成一行摘要（`tool_summary`，`export.rs:64-91`）：

```markdown
## User

给重试加一个上限

## Assistant

已加上 `max_retries`，默认 3。          ← 优先用原始 Markdown 源文本

## Tools

- Edit: crates/foo/src/client.rs
- Execute: cargo test (unit tests)
- Read: crates/foo/src/client.rs (120-160)
```

Thinking / 系统 / 子代理 / 额度等「界面外壳」块全部跳过（`export.rs:53-55`）；
被跳过的 thinking 不会打断 `last_was_agent`，所以它两侧的 assistant 消息仍共享一个标题。

### 7.3 `grok export`（无 TUI）

`export_cmd.rs` 是同一个渲染管线的无界面入口：
`load_updates_for_replay(session_id)`（`export_cmd.rs:25`，实现在
`crates/codegen/xai-grok-shell/src/session/storage/replay.rs:158-170`）→ 依次喂给
`AcpUpdateTracker` + 空的 `ScrollbackState`，`_meta.isReplay = true`
（`export_cmd.rs:28-37`）→ 同一份 `render_blocks_to_markdown`。
三种出口：`--output` 写文件（自动建父目录，`export_cmd.rs:51-65`）、`-c/--clipboard`
（`export_cmd.rs:66-79`）、否则 stdout。会话不存在或没有对话内容都会报错退出
（`export_cmd.rs:25-26,44-49`）。

### 7.4 share：当前不可用

两条路径都硬编码关闭：

- TUI：`/share` 的 `run` 直接返回 `CommandResult::Error("Session sharing is temporarily disabled")`
  （`slash/commands/share.rs:14-17`）；
- CLI：`grok share <session-id>` 的实现体是 `anyhow::bail!("Session sharing is temporarily disabled")`
  （`share_cmd.rs:11-13`），`ShareArgs` 的入参被显式丢弃。

因此**本快照里没有任何「分享链接 / 公开 URL」的产物形态**——只有本地 Markdown 导出与剪贴板。

### 7.5 一处需要标注的清单偏差

任务清单里的 `docs/user-guide/27-grok-clone.md` **与「会话克隆」无关**：它讲的是 Grove 的
`grok clone`（把 Git 仓库拉进内容存储并投影出工作树，depth-1 bootstrap、FUSE/NFS、
凭证分类与 `grove reload-credentials`，见该文件 `:1-30`）。会话层面的「复制对话到新会话」
对应的是 `/fork [--worktree|--no-worktree] [directive]`（`docs/user-guide/17-sessions.md:118-126`），
其界面实现另有专题。

## 关键结论（供合稿引用）

1. 首屏之前的工作**全部串行 await**，没有 splash、也没有进度条实现；唯一有名字的等待预算是 `EARLY_PREFETCH_WAIT = 2 s`，最慢的一步是含远端恢复 ≤90 s 的 `materialize_startup()`（`crates/codegen/xai-grok-pager/src/app/mod.rs:550, 663-1001, 824`）。
2. `init_terminal()` 的顺序即语义：`enable_raw_mode()` 最先，panic hook 与信号处理器紧随其后安装；鼠标捕获只在非 minimal 打开；光标样式默认 `Inherit` 不发任何转义，只有强制过才置 `CURSOR_STYLE_FORCED`，teardown 因此知道该不该复位（`crates/codegen/xai-grok-pager/src/app/mod.rs:1430-1446, 1459-1661, 1523`）。
3. 「向终端写什么」只有一个真源 `emit_terminal_teardown_sequences`（且**不含** `disable_raw_mode()`），「谁在什么条件下调它」有三条路径；`restore_terminal_with` 的顺序固定为 join reader → drain writer → teardown → fence（仍在 raw mode）→ `disable_raw_mode()` → `mark_restored()` → 恢复 fd 2（`crates/codegen/xai-grok-pager/src/app/terminal_restore.rs:61-134, 143-185`）。
4. panic hook 路径**不 join reader、不排空 writer**（会死锁），整个 teardown 放在 helper 线程并受 2 s 上限；信号路径不 join、靠 exit 看门狗兜底（`crates/codegen/xai-grok-pager/src/app/terminal_restore.rs:243-258`、`crates/codegen/xai-grok-pager/src/app/signal_handler.rs:219-239`、`crates/codegen/xai-grok-pager/src/app/exit_timeout.rs:8`）。
5. 所有等待都有界：`WRITER_JOIN_GRACE = 2 s`（超时不报错而是脱钩并记 `term.writer.join_timeout`，后续 teardown 必须走 `run_bounded_teardown(..., 2 s)`，否则「终端不再读」会让 `/quit` 永久挂起）、`READER_JOIN_GRACE = 250 ms`（`join_within` 从不 join 一个还在跑的线程）、exit 看门狗 20 s 加 `HARD_EXIT_GRACE` 5 s 后 `libc::_exit`（`crates/codegen/xai-grok-pager/src/app/terminal_restore.rs:27, 32-46, 138, 169`、`crates/codegen/xai-grok-pager/src/app/reader_thread.rs:16, 103-123`、`crates/codegen/xai-grok-pager/src/app/exit_timeout.rs:8, 53-79`）。
6. 重复 teardown（panic + 信号、或双信号）由 `TERMINAL_OWNED` 布尔、kitty flags 的 `swap`、Windows stdin 模式快照的 `swap` 三者保证至多一次（`crates/codegen/xai-grok-pager/src/app/signal_handler.rs:60-88, 232-233`、`crates/codegen/xai-grok-pager/src/app/mod.rs:111, 1378-1390`）。
7. 信号策略：首个信号优雅退出（走和 `/exit` 一样的收尾）、第二个立即强制退出；`SIGTTIN`/`SIGTTOU` 显式忽略；**`SIGPIPE` 故意不装处理器**（保持 Rust stdlib 的 `SIG_IGN`，避免把 fork+exec 子进程继承的 disposition 改成 `SIG_DFL`）；退出码 Ctrl+C `130` / SIGTERM `143` / SIGHUP `129`，且先还原终端再 flush Sentry（`crates/codegen/xai-grok-pager/src/app/signal_handler.rs:12-16, 75-79, 90-135, 137-150, 198-209, 216-231`）。
8. 恢复历史走「延迟渲染」并在 `SessionLoaded` 前设栅栏：本会话 replay **不设超时**，无关会话流量只在事件循环有余力 drain 时累计秒表、超过 `SESSION_LOADED_ACP_BARRIER = 2 s` 放行；重放的 thinking 块用 `streaming_replay()` 把 `started_at` 置 `None`，否则会渲染出假的 `Thought for 0.0s`（`crates/codegen/xai-grok-pager/src/app/acp/tracker.rs:1132-1136`、`crates/codegen/xai-grok-pager/src/app/session_load_barrier.rs:3-8, 26, 174-180`、`crates/codegen/xai-grok-pager/src/scrollback/blocks/thinking.rs:152-160`、`crates/codegen/xai-grok-pager/src/scrollback/blocks/markdown_content.rs:135-142`）。
9. 退出时 kitty pop 之后有 DA1 栅栏：`CSI < u` 之后再写 `CSI c`，读到完整 `ESC [ ? … c` 才收工，目的是排空「终端应用 pop 之前发出的按键上报」，否则 fish 会把 `ESC [ 100 ; 5 : 3 u` 读成 Ctrl-D 然后退出；`TeardownFence::decide()` 的跳过优先级是 writer 卡死 > reader 仍活 > 从未 push 过 flags，`POP_FENCE_TIMEOUT = 1 s` 取自 neovim 的实战值（`crates/codegen/xai-grok-pager-render/src/terminal/pop_fence.rs:1-8, 39-101`、`crates/codegen/xai-grok-pager/src/app/teardown_fence.rs:37-48, 78-95`）。
10. 屏幕模式解析链从高到低是 `GROK_SCREEN_MODE`（一次性、**读完即删**，否则每个被 spawn 的子进程都会继承用户从未要求的强制模式）> CLI `--minimal`/`--fullscreen` > `[ui] screen_mode` > legacy `pager.toml [terminal] minimal` > alt-screen 策略；切到 minimal 前先拿掉鼠标捕获，且必须 `Clear(All)` 而**不是** `Clear(Purge)`（purge 会毁掉用户真实的回滚缓冲），重绘前 `reseed_screen_mode` 把「启动时按模式决定的东西」重播一遍（`crates/codegen/xai-grok-pager/src/app/screen_mode_relaunch.rs:17-20, 268-278, 280-319`、`crates/codegen/xai-grok-pager/src/app/mode_switch.rs:29-53, 121-140`）。
11. exec 热重启只适用三种场景（启动期 env 覆盖、`GROK_SCREEN_MODE_SWITCH=exec` 逃逸开关、不可恢复的切换）；argv 重建时**哪些 flag 吃值是从 clap 定义本身推导**的（`clap::CommandFactory`），Unix 用 `cmd.exec()` 保住 PTY/fd 身份，Windows 无 `exec` 改为 spawn 子进程 + 父进程 `wait`（`crates/codegen/xai-grok-pager/src/app/screen_mode_relaunch.rs:1-10, 25-54, 59-160, 211-241`）。
12. 明确的失败模式与应对：终端停止读取 → writer 脱钩 + 有界 teardown + 20 s 看门狗 `_exit`；stdin reader 卡住 → 250 ms 有界 join + fence 放弃（`SkipReaderAlive`）+ 10 ms 静默排空；连 panic hook 都没跑完 → `xai_crash_handler` 的 `RESTORE_SEQ` 用裸 `write(2)` 写 fd 2；**SIGKILL 未找到证据**表明有处理（`crates/codegen/xai-grok-pager/src/app/terminal_restore.rs:27, 143-185`、`crates/codegen/xai-grok-pager/src/app/reader_thread.rs:103-123`、`crates/codegen/xai-grok-pager/src/app/teardown_fence.rs:37-48`、`crates/codegen/xai-crash-handler/src/terminal.rs:44-60`、`crates/codegen/xai-grok-pager/src/app/signal_handler.rs:90-135`）。
