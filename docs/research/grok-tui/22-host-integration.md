# 专题 22：宿主集成——通知、标题、OSC 与终端复用器

> 专题：宿主集成——通知、标题、OSC 与终端复用器｜调研日期：2026-09-22｜证据范围：`crates/codegen/xai-grok-pager/src/notifications/`（全部 9 个文件）、`src/app/{status_line.rs,app_view.rs,event_loop.rs,mod.rs,terminal_restore.rs,teardown_fence.rs}`、`crates/codegen/xai-grok-pager-render/src/terminal/`（`probe.rs`、`da2.rs`、`xtversion.rs`、`tmux.rs`、`tmux_probe.rs`、`pop_fence.rs`、`overlay.rs`、`hyperlinks.rs`、`kitty_keyboard.rs`、`mod.rs`）、`src/theme/{osc11.rs,mod.rs}`、`src/render/draw.rs`、`src/clipboard/mod.rs`、`crates/codegen/xai-tty-utils/src/lib.rs`、`docs/user-guide/{05-configuration.md,21-terminal-support.md}`

## 0. 这一层在做什么

宿主集成层负责「TUI 之外的一切终端交互」：把 Agent 的状态推给终端外围设施（窗口标题、标签页、通知中心、状态栏、系统剪贴板），同时把终端的回答拉回来（能力探测、焦点、尺寸）。

```
                        ┌──────────────────── 进程内 ────────────────────┐
 Agent 事件             │                                                │
  ├─ TurnComplete ─┐    │  AppView::update_notifications()  每 tick      │
  ├─ AgentError   ─┼──► │        │                                       │
  ├─ ApprovalReq  ─┤    │        ├─► TitleManager.update()  ─┐           │
  └─ SessionReady ─┘    │        └─► progress keep-alive    ─┤           │
                        │                                   │           │
 Event::FocusGained ──► │  FocusTracker ─► condition 门控     │           │
 Event::FocusLost   ──► │                                   ▼           │
                        │                     pending_notification_escapes
                        │                                   │（随帧走）
 通知（不走帧）─────────►│  NotificationService::notify()     │           │
                        │        │                          │           │
                        │        └─► protocol::emit_notification()      │
                        │                    │（EscapeWriter 队列）       │
                        └────────────────────┼──────────────────────────┘
                                             ▼
                                    WriterThread ──► stderr/tty
                                             │
                              tmux 背衬？ ──► DCS passthrough 包装
```

关键设计取向：**通知绕开帧管线**（走独立逃逸队列），**标题与进度条跟随帧**（走 `pending_notification_escapes`），二者共用同一个 WriterThread 队列以保证字节顺序。

## 1. 桌面通知

### 1.1 什么事件触发

`NotificationEventKind` 共 5 个变体（`crates/codegen/xai-grok-pager/src/notifications/config.rs:100-115`），但有发射点的只有 4 个：

| 事件 | 发射点 | 说明 |
| --- | --- | --- |
| `ApprovalRequired` | `src/app/acp_handler/permissions.rs:47` | 一批排队的权限请求只发一次（`permission_notified` 抑制），队列清空后复位（`src/app/app_view.rs:5813-5816`） |
| `TurnComplete` | `src/app/dispatch/prompt.rs:1675-1684` | 正文 `Turn complete in {duration}.` 或 `Turn complete.`；`bash_turn` 不发 |
| `AgentError` | `src/app/dispatch/prompt.rs:1685-1687` | 正文 `Error: {err}`，已展示专用 UX 时不发 |
| `SessionReady` | `src/app/dispatch/status.rs:517-529` | 标题固定 `"Grok"`，正文为事件名 |
| `TaskComplete` | **未找到发射点** | 出现在配置枚举与文档中；全仓库 grep `NotificationEventKind::TaskComplete` 无命中 |

事件先过两道门：`config.events` 白名单（`notifications/mod.rs:81-83`）与 `config.condition`（`notifications/mod.rs:85-91`）。

```rust
// crates/codegen/xai-grok-pager/src/notifications/mod.rs:96-107
pub fn notify(&self, event: NotificationEvent) {
    if !self.is_event_enabled(&event.kind) { return; }
    if self.should_emit_terminal() {
        protocol::emit_notification(self.protocol, &event.title, &event.body,
                                    self.terminal_ctx, &self.escape_writer);
        // ... 同时上报 NotificationEmitted telemetry
```

### 1.2 走什么协议

只有 5 种协议，没有直接调用系统通知 API（无 `libnotify`/D-Bus 调用；`dbus` 仅出现在沙箱的 socket 拒绝路径里）：

| 协议 | 字节形式 | 目标终端 |
| --- | --- | --- |
| `Osc9` | `\x1b]9;{body} · {title}\x07` | iTerm2 / WezTerm / Warp |
| `Osc99` | `\x1b]99;i=grok;{body} · {title}\x1b\\` | Kitty |
| `Osc777` | `\x1b]777;notify;Grok;{body}\x1b\\` | Ghostty / VTE / Terminator / foot |
| `Bel` | `\x07` | Apple Terminal / Alacritty / VS Code / Windows Terminal / Unknown… |
| `None` | 不发送 | Grok Desktop |

依据：`crates/codegen/xai-grok-pager/src/notifications/protocol.rs:6-19`（变体与文档注释）、`:58-74`（`notification_sequence`）、`:21-47`（`select_protocol` 的品牌映射）。

两个细节值得注意：

- **OSC 9 与 OSC 99 是纯正文协议**，所以把会话名折进正文（` · ` 分隔）；OSC 777 的 title 参数被固定成 `Grok`，因为标签页标题已经承担了会话名（`protocol.rs:66-70`）。
- **Zellij 被硬编码成 `Bel`**：`select_protocol` 第一行就判 `multiplexer == Zellij` 返回 `Bel`（`protocol.rs:22-24`）。反过来，tmux 和 screen **不**参与协议选择——包装发生在发送时刻而非选择时刻（`protocol.rs:129` 的注释与测试）。

### 1.3 用户如何配置

配置节 `[ui.notifications]`，由 `load_notification_config` 从原始 TOML 读取，缺失或畸形一律回落默认值（`notifications/mod.rs:262-269`）。默认值：`method = "auto"`、`condition = "unfocused"`、`idle_threshold_secs = 3`、`events = ["turn_complete", "approval_required"]`、`sleep_prevention = true`、`progress_bar = true`（`config.rs:24-42`）。带注释的模板由 `NotificationConfig::to_toml_with_comments()` 生成（`config.rs:139-180`），用户手册同步列出同一张表（`docs/user-guide/05-configuration.md:459-490`）。

`condition` 的语义：`Always` 恒发、`Never` 永不发、`Unfocused` 只在丢失焦点且超过 `idle_threshold_secs` 后发（`notifications/mod.rs:85-91` + `notifications/focus.rs:48-56`）。

另有三个与「人不在场」相关的开关：`sleep_prevention`（阻止息屏，macOS 用 `IOPMAssertionCreateWithName` + `NoIdleSleepAssertion`，Linux 起 `systemd-inhibit --what=idle … sleep infinity`；平台失败会置 `platform_unavailable` 只试一次，见 `notifications/sleep.rs:37-46/57-125`）、`session_recap` 与 `session_recap_threshold_secs`（回到终端时的「我之前做到哪」回顾，重试退避 90 s，见 `focus.rs:5-6,65-85`）。

### 1.4 发送路径与失败处理

`emit_notification` 把字节丢进 `EscapeWriter`（非阻塞、队列化），**不是**内联写 stderr——因为通知从事件循环线程发出，内联取 stderr 锁在终端停止读取时会死锁（`crates/codegen/xai-grok-pager-render/src/render/draw.rs:241-247`、`protocol.rs:97-107`）。发送失败被记录在共享的 `WriterSync` 上而非返回给调用方（`draw.rs:266-276`）。

内容净化和注入防护：标题与正文都过 `sanitize_osc_text`，剥掉所有控制字符（含 BEL 与 C1 ST），避免模型产出的文本提前终止 OSC/DCS（`protocol.rs:49-55`，测试见 `:229-248`）。

**TurnComplete 的时序补偿**是这一层最细的一个设计：先发 idle 标题转义（队列非空时跳过，避免标题闪烁），再延迟 3 个 tick（≈99 ms）才真正发通知。原因是 Ghostty 对 `setTitle()` 有 75 ms 防抖，通知若立刻读标题会读到旧值（`src/app/dispatch/prompt.rs:1735-1773`）。延迟计数在 `AppView` 中递减，归零才调 `notify`（`src/app/app_view.rs:5495-5501`）。

### 1.5 通知 hooks（重要发现）

`[ui.notifications.hooks]` 可配置任意 shell 命令，通过环境变量 `GROK_EVENT` / `GROK_MESSAGE` / `GROK_SESSION_ID` 传参；默认 `only_unfocused = true`、`timeout_secs = 10`（`config.rs:116-133`）。实现（`notifications/hooks.rs:8-115`）相当完整：`sh -c` 启动、`detach_std_command` 脱离控制终端、限制子进程网络、进程组登记、超时后杀进程树、ECHLD 后交给 reaper 线程收尾。

但**未找到生产调用点**：全仓库 grep `run_hook`，除 `notifications/hooks.rs:99` 的定义外，其余命中全是该文件的单元测试；`NotificationHook` 类型除 `notifications/mod.rs:19` 的重导出外无消费者；`NotificationService::notify` 也不遍历 `config.hooks`。shell 侧另有一套同名概念 `SessionCommand::DispatchNotificationHook`（`crates/codegen/xai-grok-shell/src/session/commands.rs:748-754`），由 `xai-grok-shell` 的对话流程派发，是**不同机制**，不能拿来当作 pager 配置项已生效的证据。此处应标记为待复核项。

## 2. 终端标题

### 2.1 标题内容如何生成

标题是「按顺序拼接的部件列表」，默认 `[action-required, spinner, activity, session-name, grok]`（`config.rs:72-85`），部件之间用 `" - "` 连接（`notifications/title.rs:199-203`）。若所有部件都没产出内容，回落成字面量 `grok`（`title.rs:76-79`）。

```
默认配置下的标题演化（部件顺序：action-required, spinner, activity, session-name, grok）
─────────────────────────────────────────────────────────────────────────────
 空闲              │  grok
 会话已命名        │  my project - grok
 思考中            │  ⠋ - Thinking - my project - grok
 跑工具            │  ⠙ - Running: bash - my project - grok
 等待首个 chunk    │  ⠹ - Waiting - my project - grok
 需要授权(未聚焦)  │  ⚠ Action Required - ⠸ - Waiting - my project - grok
 需要授权(已聚焦)  │  ⚠ Action Required - ...（静态前缀，不闪烁）
```

部件行为要点（`title.rs:110-197`）：

- `Spinner`：8 帧盲文点阵 `⠋⠙⠹⠸⠼⠴⠦⠧`，但**每 8 个 tick 才换一帧**（30 fps 下 ≈264 ms），因为终端（尤其 Ghostty）会防抖标签标题（`title.rs:8-15`）。
- `ActionRequired`：仅在 `has_pending_permissions` 时出现；**未聚焦才闪烁**（15 tick 半周期 ≈500 ms 可见/500 ms 隐藏），聚焦时静态显示以消除输入时的抖动（`title.rs:17-20,180-193`）。
- `SessionName` 截断到 40 字符、`Model` / `Cwd` 到 30 字符；`Cwd` 只取路径最后一段；`TurnTimer` 少于 1 秒不显示（`title.rs:144-179`）。超长用 `…` 收尾（`title.rs:243-257`）。

### 2.2 什么时候更新

不要按「状态变化」去找主动推送点——标题是**轮询式每 tick 比较**的：`AppView::update_notifications()` 每 tick 组装一次 `TitleState` 并调 `on_tick()`，只有合成结果与上次不同才返回逃逸序列（去重，`title.rs:81-89`），结果追加到 `pending_notification_escapes` 随后续帧发出（`src/app/app_view.rs:5790-5833`）。

`TitleState` 的字段就是标题的全部输入：`session_name`、`model`、`activity`、`has_pending_permissions`、`cwd`、`turn_elapsed`、`is_busy`、`focused`（`title.rs:23-35`）。渲染线程外的两个特殊点：`is_busy` 且 `activity` 为空时显示 `Waiting`（首个 chunk 之前的空档）；`renders_parked()` 时把 activity / elapsed 视为空（`app_view.rs:5800-5808`）。

转义序列本身由 crossterm 的 `SetTitle` 生成（`title.rs:259-266`），即 OSC 0（`\x1b]0;…\x07`）。退出时 `shutdown()` 把标题复位成 `grok`（`title.rs:99-106`、`notifications/mod.rs:179-183`）。

### 2.3 tmux 下的特殊处理

标题本身**不做** DCS 包装——`build_title_escape` 直接产出 OSC 0。这一点与通知/进度条/OSC 11 查询不同（见 §6.2）。tmux 相关的差异体现在部件内容而非包装：`Cwd` 用 basename 避免长路径、`SessionName` 截断 40 字符，都是给窄标签栏留余量。

## 3. 进度条与光标色

### 3.1 OSC 9;4 标签页进度条

`progress_bar = true` 时，busy 显示不确定态、idle 清除：

```rust
// crates/codegen/xai-grok-pager/src/notifications/progress.rs:20-21
const OSC_INDETERMINATE: &str = "\x1b]9;4;1;-1\x07";
pub(crate) const OSC_CLEAR: &str = "\x1b]9;4;0;0\x07";
```

支持面很窄：仅 Ghostty、WezTerm，以及 iTerm2 ≥ 3.6（早期 iTerm2 会把 `9;4` 当成 OSC 9 通知，把原始参数 `4;1;-1` 当正文弹出来，`progress.rs:10-18`）。保活周期 5 秒，因为 Ghostty 约 15 秒无输出就会自行复位指示器（`notifications/mod.rs:14-16,149-171`）。tmux 下包装成 DCS passthrough（`progress.rs:34-38`）。

清退路径不依赖 `NotificationService`：信号/panic 处理器无法访问它，因此 `emit_terminal_teardown_sequences` 无条件直写一次 `OSC_CLEAR`（`src/app/terminal_restore.rs:60-70`）。

### 3.2 OSC 12 / OSC 112 光标色

这里**没有查询，只有写入**：`cursor_color_escape()` 用主题的 `accent_user` 生成 `\x1b]12;rgb:RR/GG/BB\x07`，无 RGB 强调色（终端原生调色板 / NO_COLOR）时返回 `None`（`crates/codegen/xai-grok-pager-render/src/theme/mod.rs:563-568`）。复位用 OSC 112，且**必须**由 `CURSOR_COLOR_APPLIED` 门控——无端复位会让 Ghostty 锁死光标色并停止跟随主题切换，所以只撤销自己画过的东西（`theme/mod.rs:559-563,587-622`）。

单一发射者是事件循环里的小型跟踪器：只在目标值变化时发 OSC 12 或 OSC 112（`src/app/event_loop.rs:1971-1986`）；启动阶段在 writer 线程存在之前由 `apply_cursor_color()` 内联写一次（`src/app/mod.rs:1538`）。

## 4. 终端能力探测

### 4.1 总表

| 探测 | 查询 | 读法 | 门控 | 超时 |
| --- | --- | --- | --- | --- |
| OSC 11 背景色 | `\x1b]11;?\x07` | 同步读 fd，裸查询优先，失败再试 DCS 包装 | stdin 是 TTY 即试 | 500 ms（包装版 80 ms） |
| DA2 | `\x1b[>0c` | 同步读 fd | 仅 Alacritty 且非 CSI 拦截型复用器 | 500 ms |
| XTVERSION | `\x1b[>0q` | fire-and-forget；事件循环过滤器识别 DCS 回复 | Unknown/Kitty/WezTerm/Ghostty/iTerm2/Rio 且非 CSI 拦截型复用器 | 过滤器 5 s 武装窗口 |
| 终端版本 | 环境变量 | 纯字符串解析 | 必须由同品牌自证 | — |

`xtversion::gate_allows_probe` 的名单与 `da2::gate_allows_probe` 的名单必须互斥（`terminal/xtversion.rs:81-89`、`da2.rs:65-70`），否则两种读法会互吃对方的回车。

### 4.2 OSC 11 背景色

用途是把「终端画布是亮还是暗」变成一个可用的输入，供主题/配色判断（`classify_luminance` 用 BT.709 + sRGB 逆传输函数，阈值 0.5，`src/theme/osc11.rs:22-23,64-74`）。

流程（`theme/osc11.rs:34-53`）：先发**裸** OSC 11 查询——tmux ≥ 3.2 会替 pane 作答，3.4 还会在 attach 时快照外层终端颜色；裸查询失败且 `should_wrap_osc11` 成立时，才用 DCS passthrough 包一层，并把超时砍到 80 ms，因为这个包装通常被吞掉、不能让它再吃掉一个完整 500 ms 的启动时间（`osc11.rs:27-28`）。

模块文档明确两条约束：**只在启动阶段调用**（crossterm 的 `EventStream` 一旦活跃就会争抢 stdin）；**编辑器 `:terminal` 内不包装**（libvterm 会把信封当乱码画出来）（`osc11.rs:11-14`）。回复格式同时支持 `rgb:RRRR/GGGG/BBBB` 与两位缩写（`osc11.rs:76-100`）。

### 4.3 DA2

只为 Alacritty 存在：它不导出任何版本环境变量，且原则上拒绝 XTVERSION，唯一能问到的是它回的自报版本（`terminal/da2.rs:1-13`）。回复 `CSI > Pp ; Pv ; Pc c` 的 `Pv` 打包成 `major*10000 + minor*100 + patch`，解析时**只接受 Alacritty 的精确形状**（`Pp == 0`、`Pc == 1`、`packed ≤ 999999`），否则宁可不认也不要凭空造一个版本号（`da2.rs:103-133`）。

同步读的代价必须记住：迟到但已开头的回复会被 `probe::finish_after_deadline` 排到安静为止；**一整段都没到时则会留给 crossterm，被当作用户输入打进输入框**，所以超时是「为慢链路而非静默终端」设的 500 ms（`da2.rs:10-13,32-34`）。结果存 `OnceLock`，`TerminalContext::term_version()` 中**运行时探测优先于环境变量**（`terminal/term_version.rs:61-71`）。消费者之一是 kitty 键盘 flags：只在拿到「确定损坏」的库里版本（≤ 2401，即 0.14.0）时才降级不报 release 事件（`terminal/kitty_keyboard.rs:30-45`）。

### 4.4 XTVERSION

走另一条路：只写查询，不做定时读；回复在任意时刻到达时由事件循环的过滤器（`XtversionFilter`）识别并吞掉（`terminal/xtversion.rs:1-12`）。它承担两项用户可见职责——telemetry 里的 `xtversion` 字段（`terminal/mod.rs:522`），以及**反馈信息里的品牌补全**：品牌为 Unknown 时显示 `Unknown (XTVERSION: kitty 0.35.2)`（`terminal/mod.rs:536-541`）。

已接受的残留风险写在模块文档里：SSH *来自* JediTerm 时仍会发查询并泄漏，回复若在过滤器 5 s 武装窗口之后才到，会被解析成 Alt+Shift+P 加一串字面文本（`xtversion.rs:8-12`）。payload 会剥控制字符并 trim，空串视为无回复（`xtversion.rs:109-117`）。

### 4.5 版本信息的兜底：环境变量必须自证身份

环境变量会穿过进程、SSH 与复用器边界，所以「一个版本变量只有在环境同时为它所属品牌作证时才可信」（`terminal/term_version.rs:1-10`）。优先级：`TERM_PROGRAM_VERSION` → `LC_TERMINAL_VERSION`（仅 LC_TERMINAL=iTerm2 时）→ `WEZTERM_VERSION` → `VTE_VERSION`（`term_version.rs:80-116`）。`ZELLIJ_VERSION` 与 `KONSOLE_VERSION` 一律不读（前者会让 Zellij 里的 Alacritty pane 报 Zellij 的版本号，`term_version.rs:9-10`）。VS Code 家族的放宽是单向的（`vscode` 可以让 Cursor/Windsurf 借用，反之不行，`term_version.rs:50-59`）。tmux ≥ 3.2 会导出自己的 `TERM_PROGRAM_VERSION`，因此必须靠品牌自证挡掉（`term_version.rs:84-91`，测试 `:282-293`）。

### 4.6 `probe.rs`：三类读法的公共原语

`terminal/probe.rs` 提供写与读的原语，并声明两条安全不变量：**必须是 stdin 的唯一读者**（在 crossterm 读线程存在之前，或它 join 之后）；**读窗口里敲的键会被消费并丢弃**，没有可移植的回注手段（`probe.rs:1-7`）。

- 写：`write_query` 在共享 stderr 锁内写，TTY 检查跑在 dup 出来的 render fd 上而非 `std::io::stderr()`（`probe.rs:24-36`）；`write_query_until` 用同一个 deadline 约束**取锁等待 + 写入**，避免卡死的 writer 线程拖住清退（`probe.rs:38-50`）。
- 读：`read_tty_reply` 逐字节 poll，有 256 字节上限防「不发给终止符的终端」；deadline 到期后有 100 ms 宽限 + 25 ms 静默窗口把在途回复读完（`probe.rs:12-21,207-267`）。
- 排空：`drain_tty_until` 只保留 64 字节尾窗（够装最长的 DA1 回复），不限制总字节数，因此任意多的按键上报/预输入都可以出现在终止符之前（`probe.rs:288-335`）。
- 输入源：`tty_input_fd()` 先用 stdin，不是终端就开 `/dev/tty`（读写模式，与 crossterm 的 `tty_fd` 一致），这样 `grok </dev/null` 也能读到回复（`probe.rs:160-202`）。

### 4.7 未找到证据的项

- **OSC 12 查询**：不存在。OSC 12 在本仓库只作为写入（设置光标色），全仓库 grep `12;?` 无命中。
- **XTGETTCAP（`CSI > q` 之外的能力查询）**：不存在。全仓库 grep `gettcap`（大小写不敏感）无命中。
- 因此题目中「OSC 12/cursor 查询」与「XTGETTCAP 查询」在这份快照里**应改写为 OSC 12 写入**与「无对应实现」。

## 5. `pop_fence` 与 `overlay` 到底解决什么问题

题干猜测它们用于「终端能力探测时的干扰规避」。**部分正确，但两者都不是探测机制**：

### 5.1 `pop_fence`：kitty 键盘 pop 之后的 DA1 栅栏

场景：退出时把 kitty 键盘协议的 flags pop 掉（`CSI < u`）。终端按输出顺序处理，所以 `CSI ? … c`（DA1）回复到达时，终端在 pop 之前发出的所有按键上报都已躺在 stdin 里。若不排空，这些 release 事件会漏到用户的 shell——文档给出的实例是 fish 把 `ESC [ 100 ; 5 : 3 u` 读成 Ctrl-D 然后退出（`terminal/pop_fence.rs:1-8`）。

```
teardown 顺序（crates/codegen/xai-grok-pager/src/app/terminal_restore.rs:60-120）
  1. OSC 9;4 清进度条（无条件写）
  2. EndSynchronizedUpdate（DEC 2026）
  3. reset_cursor_color_if_applied()（OSC 112，有条件）
  4. 关闭鼠标/粘贴的 raw 模式、DisableMouseCapture
  5. DisableFocusChange
  6. pop gboom flags → pop kitty flags（每个 push 最多 pop 一次）
  7. 需要时恢复光标样式；Show + LeaveAlternateScreen（或 inline 定位）
  ── 之后 ──► pop_fence::run(1s)：写 DA1 查询并排空到完整回复
```

`run()` 用一个 deadline 同时约束取锁、写入与读取；只认「完整 DA1 回复 `ESC [ ? [0-9;]* c`」作为终止，DA2 回复、kitty flags 上报、CSI-u 按键事件都无法误终止（`pop_fence.rs:39-101`）。是否运行的决策在 `src/app/teardown_fence.rs:35-69`：writer 卡死 > reader 仍活 > 从未 push 过 flags，三者任一都不跑，退回 10 ms 静默期的传统排空（`teardown_fence.rs:13-16,26-33`）。超时沿用 neovim 的 1 秒经验值（`teardown_fence.rs:12-13`）。结果（决策/结局/残留字节/耗时）写进 `teardown.kitty_pop_fence` 日志（`teardown_fence.rs:78-95`）。

### 5.2 `overlay`：图形图片层，与探测无关

`terminal/overlay.rs` 是 kitty / iTerm2 图形协议的**图片覆盖层生命周期管理**：`static`（跨帧保持，按 `owner_id` + 几何位置判定是否需要重画）与 `volatile`（每帧重画）、`clear`（kitty 按 id 删除；iTerm2 无删除能力但必须复位所有权）、以及 `PostFlush`——图片转义在 cell diff 之后写入，并且**只有写入成功才提交所有权**（`overlay.rs:1-11,104-110,160-269`）。它服务的功能是提示里的图片/预览（`src/views/prompt_widget/mod.rs:3455-3472`、`src/views/welcome/mod.rs:911-912`），与终端能力探测没有关系。与探测唯一的交集是：这些转义同样在 tmux 下不可用（`TerminalContext::graphics_protocol_skip_reason` 直接返回 `"tmux"`，`terminal/mod.rs:373-379`）。

## 6. tmux / screen 兼容清单

### 6.1 复用器识别

`MultiplexerKind` 有 6 个取值（`terminal/mod.rs:169-189`），识别顺序为：Byobu 显式后端 → `TMUX` → `ZELLIJ`/`ZELLIJ_SESSION_NAME` → `STY`（screen）→ `HERDR_ENV` → cmux 三兄弟 → Undetected（`terminal/mod.rs:777-814`）。注释解释了「嵌套时真实复用器优先」的动因：herdr daemon 从 tmux 里起会把 `TMUX` 冻结进每个 pane，若误判为 herdr 就会把 OSC 52 包进 DCS，而 herdr 会把这段信封当文本打印出来。

```rust
// crates/codegen/xai-grok-pager-render/src/terminal/mod.rs:191-197
pub fn intercepts_csi_queries(self) -> bool {
    matches!(self, Self::Tmux | Self::Screen | Self::Zellij | Self::Herdr)
}
```

**Cmux 不拦 CSI**，所以它的品牌探测照常进行（`xtversion.rs:169-173` 的测试）。

### 6.2 DCS passthrough 包装

```rust
// crates/codegen/xai-grok-pager-render/src/terminal/tmux.rs:9-20
pub fn tmux_passthrough(sequence: &[u8]) -> Vec<u8> {
    // \x1bPtmux; … \x1b\\，其中每个 0x1b 都写两遍
}
```

可用条件是 **tmux ≥ 3.3**，版本未知按旧版处理（`tmux.rs:38-42`）。模块文档把责任边界写得很清楚：包装前调用方必须已经净化过载荷，这两个 helper **只做 ESC 双写**，不剥离 CAN(`0x18`)/SUB(`0x1a`)/C1 ST(`0x9c`)/BEL(`0x07`)——这些字符在 tmux/xterm 下会中止或终止 DCS/OSC（`tmux.rs:1-4`）。

被包装的序列（工具函数是同一套，调用点不同）：

| 载荷 | 判定函数 | 证据 |
| --- | --- | --- |
| 桌面通知（OSC 9/99/777） | `ctx.is_tmux_backed()`（不看版本） | `notifications/protocol.rs:79-93` |
| OSC 9;4 进度条 | `passthrough_available` | `notifications/progress.rs:34-38` |
| OSC 11 查询（仅回退路径） | `should_wrap_osc11` = 可用且非编辑器 `:terminal` | `terminal/tmux.rs:44-48`、`theme/osc11.rs:47-50` |
| OSC 52 剪贴板 | `osc52_tmux_passthrough` = 开了 OSC 52 且 tmux 直接终端 | `src/clipboard/mod.rs:136-147` |

注意通知那条用的是 `is_tmux_backed()` 而非 `passthrough_available()`——两者对 tmux < 3.3 的行为不同，这是一处不对称，若未来重构需留意。

### 6.3 被跳过或被改写的东西

| 项目 | tmux | screen | 其他 | 证据 |
| --- | --- | --- | --- | --- |
| 桌面通知协议选择 | 不干预 | 不干预 | Zellij 强制 `Bel` | `notifications/protocol.rs:22-24,129` |
| DEC 2026 同步输出 | **直接终端时整个不发**（tmux 自己整屏重绘并已对外同步）；编辑器 `:terminal` 内仍发 | 保留 | 保留（含 Zellij/Byobu） | `terminal/tmux.rs:50-57`、`render/draw.rs:505-529` |
| kitty 键盘协议 | < 3.3、或 `extended-keys off` → 跳过 | 跳过 | — | `terminal/mod.rs:356-371` |
| 超链接 OSC 8 | < 3.4 → 跳过 | 跳过 | Apple Terminal 视为敌意解析器；VTE < 0.50.4 跳过 | `terminal/mod.rs:436-468` |
| 行内图形图片 | 跳过 | — | — | `terminal/mod.rs:373-379` |
| 备用屏 | 控制模式下自动走 inline | — | Zellij 自动走 inline | `terminal/mod.rs:962-978` |

### 6.4 tmux 的带外查询

tmux 的版本、选项与客户端能力通过子进程查询，而不是转义序列（`terminal/tmux_probe.rs`）：`tmux -V`、`show-option -gqv <opt>`、`show-option -gv <opt>`（用于区分「不支持该选项」与「tmux 不可用」，靠 stderr 精确匹配 `invalid option:` / `unknown option:`）、`display-message -p #{client_flags}`（控制模式判定）、`display-message -p #{client_termfeatures}`（24 位色是否能在 tmux 中幸存，注释强调**这个值而非 pane 的 `COLORTERM` 才是判据**，`tmux_probe.rs:198-200`）。所有查询 2 秒超时、进程组托管、SIGTERM 后 100 ms 宽限升级 SIGKILL，读取失败统一归一到 `None`（fail-open 为「未知」而非「否定」，`tmux_probe.rs:4-9,134-165`）。

启动时只在这些条件成立才查：`multiplexer == Tmux`（`terminal/mod.rs:593-596`）。控制模式判定用于备用屏策略（`terminal/mod.rs:918-925`）。

### 6.5 用户可见的修复入口

`/doctor` 会给出四个可持久的 tmux 修复（`docs/user-guide/21-terminal-support.md:29-35`）：`set -g set-clipboard on`、`set -wg allow-passthrough on`、`set -g extended-keys on`、`set -as terminal-features ",*:RGB"`。实现上刻意**不**执行 `tmux source-file`、不热改服务器，只改宿主上的配置文件并保留行尾与权限（`21-terminal-support.md:36-47`）。

### 6.6 启动与清退的对称性

启动（`src/app/mod.rs:1500-1520`）：fullscreen 才进备用屏；非 minimal 才开鼠标捕获；随后**无条件** `EnableFocusChange` + `EnableBracketedPaste` + 隐藏光标；光标样式按策略继承或强制。清退（`terminal_restore.rs:84-110`）逆序关掉：鼠标捕获 → 焦点上报 → 键盘 flags pop。Q 清退路径还有 2 秒的 writer join 宽限（`terminal_restore.rs:25-27`），超时则脱离线程继续清退，避免终端不读时 `/quit` 变挂起。

## 7. 焦点事件与尺寸变化

### 7.1 焦点

上报由 crossterm `EnableFocusChange` 在启动时一次性打开（`src/app/mod.rs:1505`）。到达侧还有一层修复：被拆散的 `\e[I` / `\e[O`（普遍发生在 Shift 处于按下状态时，大写字母被单独解码）会由 CSI 过滤器重组成 `FocusGained`/`FocusLost`，而不是被丢弃（`src/app/csi_filter.rs:10,60-70`）。

`Event::FocusGained` 的副作用不止通知（`src/app/event_loop.rs:3234-3245`）：

- 重新发一次鼠标捕获（`reassert_mouse_capture_on_focus`，`event_loop.rs:656-660`）——焦点切换后有的终端会丢掉捕获；
- 若「外层可能越权重绘 pane」（有编辑器 `:terminal` 或检测到任意复用器）则**强制整屏重绘**（`terminal/mod.rs:288-293`）；
- 检查会话回顾是否到期并派发 `SendRecap { auto: true }`（`event_loop.rs:3240-3262`），随后 `on_focus_gained()` 复位空闲计时与回顾状态。

`Event::FocusLost` 只做两件事：`on_focus_lost()`（开始计时、允许新的自动回顾）与释放 gboom 游戏（`event_loop.rs:3288-3294`）。

一个容易被忽略的前提：越权重绘修复只在焦点事件真的到达时才有用，而 tmux 的 `focus-events` 默认关闭（`terminal/mod.rs:290-292` 的注释）。

### 7.2 尺寸

resize 走普通输入路径（`event_loop.rs:3337` 的 `is_resize` 标记），但有 16 ms 去抖（`event_loop.rs:34`），并且 tick 间隔会随需求分档：动画密集时 16 ms、一般 33 ms、只有低频需求时 83 ms（`src/app/app_view.rs:250-257,5563-5568`）。resize 事件在启动预输入（startup typeahead）中会被识别并重放（`event_loop.rs:4128,4157`）。`draw_frame` 每帧先 `terminal.autoresize()`（`render/draw.rs:510-511`），因此布局跟上尺寸变化不依赖额外的 resize 分支。

## 8. SSH 与慢链路下的降级

`is_ssh` 由 `SSH_CONNECTION` / `SSH_TTY` / `SSH_CLIENT` 任一存在判定（`terminal/mod.rs:837-839`）。降级不是一个集中开关，而是散落在各处的一致性偏好：

| 面向 | 行为 | 证据 |
| --- | --- | --- |
| 换行键 | 优先宣传 `Alt+Enter`（SSH 或复用器会丢扩展 Enter） | `terminal/mod.rs:423-430` |
| 剪贴板 | Linux 恒发 OSC 52；macOS/Windows 仅在 tmux/SSH/容器/有 wrap sink 时发 | `src/clipboard/mod.rs:132-147` |
| 剪贴板回退 | Apple Terminal over SSH 无法到达本地剪贴板，每次复制仍落盘 `~/.grok/last-copy.txt`，toast 提示路径 | `docs/user-guide/21-terminal-support.md:141-148` |
| 终端模式 | `grok wrap ssh user@host` 在本地 PTY 里转发 OSC 52 并盖上 `LC_GROK_APPEARANCE`，连接断开后恢复终端模式 | `src/wrap_cmd.rs:1-9`；`docs/user-guide/21-terminal-support.md:149-159` |
| 诊断 | doctor 对 SSH（及官方 VS Code remote）单独出建议，包含 `terminal.ssh-wrap` | `src/diagnostics/mod.rs:411-415`、`src/diagnostics/fix.rs:559,704` |
| 探测超时 | DA2 的 500 ms 明确是「为慢链路而非静默终端」设定的：整段未到会被打进输入框 | `terminal/da2.rs:32-34` |
| 启动预输入 | VTE 环境给 20 ms 排空窗口，其余环境为 0 | `src/app/mod.rs:1533-1537` |

需要明确的是：**本快照里没有按网络延迟自适应帧率或降低绘制频率的逻辑**。帧率分档只依据「有什么动画在跑」（`app_view.rs:5563-5568`），全仓库 grep `slow_link` 只命中 DA2 的那句注释。若 jev-harness 需要真正的慢链路降级，这一层需要自己设计。

## 9. 对 jev-harness 的直接启示

1. **通知与帧是两条队列，但共用一条出站通道**。绕帧管线是必要的（帧可能被 ack 卡住），但顺序性必须靠「同一个 WriterThread 队列」保证，否则 idle 标题会被还在排队的 busy 标题盖回去（`notifications/mod.rs:118-135` 的注释就是这个坑）。
2. **所有外来文本在进转义序列前必须剥控制字符**。模型产出的标题/正文直接进 OSC 就等于把终端控制权交出去（`protocol.rs:49-55`、`title.rs:259-266`）。
3. **tmux 的包装要逐项判定，不要全局套一层**。DEC 2026 要「直接终端时反而不发」，OSC 11 要先裸查再包装，通知不看版本而进度条看版本——一刀切必然错一半（`terminal/tmux.rs:38-57`）。
4. **探测前先问「谁是 stdin 的唯一读者」**。同步读与 crossterm 的 `EventStream` 不能共存，这个约束决定了探测必须发生在启动窗口内（`terminal/probe.rs:1-7`）。
5. **对「有没有此能力」永远保守到「未知」一侧**。版本未知按旧版（`terminal/mod.rs:471-481`）、DA2 只认精确形状（`da2.rs:103-118`）、探测失败存 `None` 而非 `false`（`tmux_probe.rs:158-165`）。

## 关键结论（供合稿引用）

1. 桌面通知只支持 5 种终端协议（OSC 9 / 99 / 777 / BEL / None），按终端品牌自动选择，**没有任何系统级通知 API 调用**；Zellij 被硬编码为 BEL。（`src/notifications/protocol.rs:6-47`）
2. 通知事件实际只有 4 个发射点（TurnComplete、AgentError、ApprovalRequired、SessionReady）；`TaskComplete` 存在于配置枚举但**未找到发射点**。（`src/app/dispatch/prompt.rs:1675-1689`、`src/app/acp_handler/permissions.rs:47`、`src/app/dispatch/status.rs:523`）
3. 通知走独立逃逸队列（`EscapeWriter`，非阻塞），标题与进度条走帧管线，两者共用同一 WriterThread 以保序。（`src/notifications/mod.rs:96-116`、`crates/codegen/xai-grok-pager-render/src/render/draw.rs:241-276`）
4. TurnComplete 会延迟 3 tick（≈99 ms）发送，因为 Ghostty 对 `setTitle()` 有 75 ms 防抖，否则通知标题读到旧会话名。（`src/app/dispatch/prompt.rs:1753-1773`）
5. 标题是每 tick 重新合成 + 去重的部件列表，默认 `[action-required, spinner, activity, session-name, grok]`，用 ` - ` 连接；spinner 每 8 tick 换帧、授权提示仅在未聚焦时闪烁。（`src/notifications/title.rs:8-20,59-97`）
6. 标题用 crossterm `SetTitle`（OSC 0），tmux 下**不做** DCS 包装，与通知/进度条/OSC 11 的处理不同。（`src/notifications/title.rs:259-266`）
7. OSC 9;4 进度条仅支持 Ghostty/WezTerm/iTerm2≥3.6，5 秒保活（Ghostty 约 15 秒无输出即自行复位）；清退时无条件直写一次清除序列。（`src/notifications/progress.rs:10-21`、`src/notifications/mod.rs:14-16`、`src/app/terminal_restore.rs:60-70`）
8. OSC 12 在本仓库**只写不查**，复位用 OSC 112 且由「本 session 是否画过」门控，避免无端复位让 Ghostty 锁死光标色。（`crates/codegen/xai-grok-pager-render/src/theme/mod.rs:559-568`、`src/app/event_loop.rs:1971-1986`）
9. 三类探测各有独立读法：OSC 11 裸查优先再 DCS 包装（500 ms/80 ms）、DA2 同步读到 fd（仅 Alacritty，500 ms）、XTVERSION fire-and-forget 由事件循环过滤器识别。（`crates/codegen/xai-grok-pager-render/src/theme/osc11.rs:25-53`、`terminal/da2.rs:29-70`、`terminal/xtversion.rs:1-89`）
10. `pop_fence` 不是探测机制，而是 kitty 键盘 pop 之后的 DA1 清退栅栏，防止残留按键上报漏进用户 shell；`overlay` 是图形图片层，同样与探测无关。（`terminal/pop_fence.rs:1-8`、`src/app/teardown_fence.rs:35-69`、`terminal/overlay.rs:160-269`）
11. tmux 包装的条件不统一：通知只看 `is_tmux_backed()`，进度条与 OSC 11 要 ≥ 3.3，OSC 52 另用 `osc52_tmux_passthrough`；DEC 2026 在 tmux 直接终端时**反而不发**。（`src/notifications/protocol.rs:79-93`、`terminal/tmux.rs:38-57`、`src/clipboard/mod.rs:136-147`）
12. SSH 下没有帧率自适应降级：`is_ssh` 只影响换行键宣传、剪贴板路由、doctor 建议与 `grok wrap`；慢链路的唯一专门设计是 DA2 的 500 ms 超时取值。（`terminal/mod.rs:423-430`、`src/clipboard/mod.rs:132-147`、`terminal/da2.rs:32-34`）

## 待复核 / 未找到证据

- `[ui.notifications.hooks]` 与 `notifications::hooks::run_hook` 的生产调用点：未找到（只有单元测试调用）。shell 侧的 `SessionCommand::DispatchNotificationHook` 是另一套机制，不能作为该配置已生效的证据。
- `NotificationEventKind::TaskComplete` 的发射点：未找到。
- OSC 12 查询、XTGETTCAP 查询：不存在（Oscar 12 只有写入；全仓库 grep `gettcap` 无命中）。
