## 5. 交互逻辑

### 5.1 键盘模型与动作注册表

### 本节要点

- 键位只有一处来源：`actions/defaults.rs:56` 的 `default_actions(screen_mode, mouse_reporting_toggle_enabled)`。文件头注释写明理由 "All key bindings are defined here, not scattered across event handlers."（`actions/defaults.rs:1-3`）。
- 动作身份是编译期 enum `ActionId`（**76** 个变体，无字符串名、无 serde、无 `FromStr`）；默认注册 **70** 个 `ActionDef` = 主 vec 50 + `mode_ctrl_g_action` 二选一 + Dashboard 块 19，分布为 Scrollback 28 / DashboardFocused 14 / AgentScreen 13 / PromptFocused 7 / Always 4 / DashboardOverlay 4；开 `mouse_reporting_toggle` 再 +1。
- **键位不可重映射**：手册原话 "Bindings are built in and **cannot currently be remapped**."（`docs/user-guide/03-keyboard-shortcuts.md:3`）；而 `actions/defaults.rs:901-902` 声称可经 `~/.grok/config.toml` 重绑的注释**与实现不符**——代码里没有任何读取键位配置的路径。
- 上下文是 7 值 `When` 枚举，`lookup()` 做**精确**匹配、冒泡顺序写死在调用方；`When::WelcomeScreen` 有枚举分支但**零注册（未找到证据）**，欢迎页键位由 `handle_welcome_input()` 内联字面匹配。
- **没有多键序列，也没有 leader key**；`app/leader_cluster/` 是进程级 leader IPC 的测试装置，与键盘无关。
- 终端差异分四层屏蔽（协议协商 / 修饰键探针 / 品牌分支键位 / 语义归一）；**冲突校验为零**，只有速查表渲染期去重兜底。

#### 从 crossterm 事件到 handler

```
 crossterm::event::poll/read()                          app/reader_thread.rs:67-69
   ▼ TimedInputEvent{event,arrived_at}；启动期 type-ahead 过滤 / 粘贴合并 / 语音和弦抢先
                                              event_loop.rs:40-50, 3710-3740, 3554-3565
   ▼ AppView::handle_input_at_with_paste_provenance()     app/app_view.rs:2320
   ├─ 0 keyboard_normalizer.rescue(ev)  修饰键救援         input/keyboard_normalizer.rs:55
   ├─ 1 PendingAction 消费（双击确认）                     app/app_view.rs:2337
   ├─ 2 Tutorial / 鼠标滚动 / 模态 / 各浮层抢占；3 active_view 分派（Welcome / Agent / Dashboard）
   ▼ AgentView::handle_input → handle_input_inner         app/agent_view/input.rs:235, :329
   ├─ 第 1 层 pane：prompt → lookup(key,PromptFocused)（prompt.rs:499），未命中则 Tab 结构化 FocusScrollback
   │                scrollback → lookup_with_mode(key,ScrollbackFocused,vim)（panes.rs:76）
   ├─ 第 2 层 agent：lookup(key,AgentScreen) → CancelTurn / ToggleYolo / ModelPicker …（prompt.rs:636）
   ├─ 第 3 层 Esc 策略（仅 Prompt/Scrollback，prompt.rs:747-830）：running → 提示 Ctrl+C 文案；
   │    cancelling → 吞掉；idle 有草稿（仅 prompt）→ ArmPending ClearPrompt（双按）；
   │    idle 空 + 有消息 → ArmPending RewindShowPicker（双按）；其余 → Changed（吞掉，不回传 global）
   └─ 第 4 层 global：lookup(key,Always) → Quit/NewSession/OpenDashboard/VoiceToggle  app_view.rs:2952-3010
   ▼ InputOutcome{Action|ActionThenForward|ActionPair|ArmPending|Changed|Unchanged}
   ▼ dispatch::dispatch(action,&mut app) -> Vec<Effect>    app/dispatch/router.rs:156
   │    dispatch 层「不碰终端 / 网络 / 文件系统」，所有变更同步且确定性（app/dispatch/mod.rs:1-10）
   ▼ process_effects() → effects::execute(eff,…)          app/effects/mod.rs:120
```

这张图是 `app/agent_view/mod.rs:9-47` 模块文档的转写，第 3 层每个分支都能在 `try_handle_esc_policy()` 里逐行对上；文档另声明**没有 Esc 分支依赖 `vim_mode` 或 `simple_mode`**，"Tab remains leave-prompt in both modes."（`app/agent_view/mod.rs:49-51`）。两阶段边界的直接证据是 `app/leader_cluster/mod.rs:104-147`：进程内测试装置原样复用 `dispatch::dispatch` + `effects::execute` 跑完整轮会话，只把终端换成内存通道；`dispatch_then_forward()` 是唯一需要「同事件二次处理」的特例（`app/event_loop.rs:3907-3931`）。

#### 动作抽象与配置覆盖

- `ActionDef` 的 10 个字段承担全部元数据（`actions/mod.rs:197-221`）：`id` / `label`（栏内短标签）/ `description` / `long_help`（速查表正文，`None` 回落 `description`）/ `default_key` / `alt_keys` / `category` / `context: When` / `hint_priority` / `hint_key_display` / `requires_confirmation`。
- `hint_priority` 语义是 "`None` means don't show; `Some(0)` is the highest priority"（`actions/mod.rs:213`）。**全表只有 8 个动作设了它**：`SelectNext` 0、`NextTurn` 1、`ToggleFold` 3、`ExpandAllThinking` 3、`GotoTop` 4、`CommandPalette` 5、`Quit` 10、`VoiceToggle` 11——这 8 个决定底部快捷键栏显示什么（`actions/defaults.rs:85,111,163,298,328,680,782,797`）。
- `ActionId` 唯一的字符串化出口两处、都供遥测：`When::telemetry_name()`（`actions/mod.rs:169-182`）与**两元素白名单** `shortcut_used_action_label(id)`（只有 `InterjectPrompt`、`OpenExtensions`，`:487-493`）。`matches_id(id,&KeyEvent)` 忽略 `When`、只看键集合，用于跨 pane 共享和弦（队列面板强制插话复用 `InterjectPrompt` 的键集合，`:292-299`）。
- **配置覆盖：没有实现。** 构造入口只有 `defaults()` / `defaults_for(mode)` / `defaults_with_config(mouse_enabled)` 三条，都落到 `default_actions(...)`（`actions/mod.rs:240-272`）；`ActionRegistry::new(...)` 的非测试调用点为零；运行时唯一重建点是 `mode_switch.rs:33-36` 的 `reseed_screen_mode()`。唯一可配置的「键盘相关」项是 5 个**行为开关**而非键位映射：`simple_mode`（`settings/defs.rs:639-660`）、`vim_mode`（`:667-687`）、`voice_keybind_enabled`（`:1345-1365`）、`voice_capture_mode`（`:1373` 起）、`mouse_reporting_toggle`（`app/mod.rs:182-187`）；51 个 `SettingMeta` 中没有任何键位映射项。
- **模式影响解析的三个层次**：`ScreenMode` 是编译期开关（影响 `mode_ctrl_g_action` 与 Minimal 的白名单式 `retain` 裁剪，`actions/defaults.rs:1197-1204`）；`vim_mode` 经 `lookup_with_mode()` 叠加裸字母抑制（`actions/mod.rs:387-418`），关闭时裸字母改为 `ActionThenForward(Action::FocusPrompt)` 落进输入框（`panes.rs:119-133`）；模态 / 浮层 / 下拉 / 语音 / 搜索 / 选区在最前抢 `Esc` 与自身按键。同一按键在不同上下文解析成不同动作，靠精确匹配而非优先级：`Ctrl+D` 在 `ScrollbackFocused` 是 `HalfPageDown`（`defaults.rs:221-236`）、在 `Always` 是 `Quit`（`:766-788`），两者互不可见（`actions/mod.rs:602-620`）。

#### 快捷键总表

按 `context` 分组；出处列省略 `actions/defaults.rs` 前缀。除注明外均为默认（非 VS Code family、非 Apple Terminal）主机。`hint` 指该动作设了 `hint_priority`（会出现在底部快捷键栏）。

| 上下文 | 键位（备用键） | 动作 | 出处 |
| --- | --- | --- | --- |
| Always | `Ctrl+N` | `NewSession` 新建会话（双击确认 1000 ms） | `:751-765` |
| Always | `Ctrl+Q`（`Ctrl+D`） | `Quit` 退出；VS Code family 下 `Ctrl+D` 为唯一主键（双击确认 1000 ms，hint 10） | `:766-788` |
| Always | `Ctrl+Space`（`F8`） | `VoiceToggle`（hint 11）；`Ctrl+\`（`Ctrl+4`）为 `OpenDashboard`（本地 macOS VS Code family 无该 alt） | `:669-686`；`:878-899` |
| AgentScreen | `Ctrl+C` / `Ctrl+B` | `CancelTurn`（草稿非空时先清空）/ `SendToBackground` | `:496-510`；`:610-622` |
| AgentScreen | `Ctrl+G` / `Ctrl+T` | `ToggleTasks`（Minimal 下改为 `EditPromptExternal`）/ `ToggleTodos` | `:17-51`；`:532-546` |
| AgentScreen | `Ctrl+;`（`Ctrl+'`） | `ToggleQueue`；本地 macOS VS Code family 下 `Ctrl+4` 为主键、`;`/`'` 为 alt | `:547-574` |
| AgentScreen | `Ctrl+R` / `Ctrl+L` | `OpenSessions`（＝`/resume`）/ `OpenExtensions`（VS Code family 下改为 `KeyCode::Null`） | `:575-609` |
| AgentScreen | `Ctrl+O` / `F2`（`Ctrl+,`/`Cmd+,`） | `ToggleYolo` 总是批准 / `OpenSettings` | `:736-750`；`:842-854` |
| AgentScreen | `Ctrl+P`（`?`） | `CommandPalette`（hint 5）；`Ctrl+.`/`Ctrl+X` 是 `ShortcutsHelp`，两者都注册、`ctrl_dot_unreliable()` 决定谁是主键 | `:789-826` |
| AgentScreen | `Ctrl+M` | `ModelPicker`（输入框聚焦时该和弦归 `ToggleMultiline`）；`EnableVoiceMode` 是 `default_key = Null`、由事件循环的语音和弦直接派发，不走注册表 | `:827-841`；`:656-668` |
| Scrollback | `j`/`k`（`Down`/`Up`）、`J`/`K` | `SelectNext`/`SelectPrev`（`hint_key_display="j/k"`，hint 0）、`NextResponse`/`PrevResponse`（视口顶对齐） | `:77-102`；`:129-154` |
| Scrollback | `L`/`H`（`Shift+Right`/`Shift+Left`） | `NextTurn`（hint 1）/ `PrevTurn` | `:103-128` |
| Scrollback | `g`/`G`、`PageUp`/`PageDown` | `GotoTop`（hint 4）/ `GotoBottom`；同名 `PageUp`/`PageDown` | `:155-180`；`:237-262` |
| Scrollback | `Ctrl+K`/`Ctrl+J`、`Ctrl+U`/`Ctrl+D` | `ScrollUp`/`ScrollDown` 逐行；`HalfPageUp`/`HalfPageDown`（VS Code family 下后者改为裸 `D`） | `:181-206`；`:207-236` |
| Scrollback | `h`/`l`（`Left`/`Right`）、`e`/`E`、`Ctrl+E` | `Collapse`/`Expand`；`ToggleFold`（hint 3）/`ToggleExpandAll`；`ExpandAllThinking`（hint 3） | `:264-334` |
| Scrollback | `r`、`y`/`Y`、`x` | `ToggleRaw`；`CopyBlockContent`/`CopyBlockMeta`；`KillBgTask` | `:335-380`；`:439-453` |
| Scrollback | `Enter`（`Ctrl+F`）、`o`/`O` | `OpenBlockViewer`；`OpenNextLink`/`OpenPrevLink` | `:381-422` |
| Scrollback | `Tab`（`i`、`Space`） | `FocusPrompt`；`Rewind` 是 `default_key = Null`，由 `Esc Esc` 双按策略与 `/rewind` 进入 | `:468-480`；`:424-438` |
| Scrollback | `Ctrl+R` | `ToggleMouseCapture`，**仅当 `[ui] mouse_reporting_toggle` 开启**才注册 | `:857-873` |
| PromptFocused | `Enter` / `Tab` | `SendPrompt`（运行中为入队；草稿已空再按则直发队首）/ `FocusScrollback` | `:454-467`；`:481-495` |
| PromptFocused | `Shift+Tab`（`BackTab`、`BackTab+SHIFT`） | `CycleMode`（Normal → Plan → Always-Approve） | `:511-529` |
| PromptFocused | `Ctrl+Enter`（`Ctrl+I`） | `InterjectPrompt`；Apple Terminal 下 `Ctrl+O` 为主键，VS Code family 下改为 `Ctrl+L` 且无 alt | `:624-654` |
| PromptFocused | `Ctrl+M`、`Ctrl+S`（`Alt+S`）、`!` | `ToggleMultiline`；`StashPrompt` 暂存/取回草稿；`BashMode`（空输入框打 `!` 进 shell 模式） | `:689-734` |
| Dashboard | `↓`/`↑`、`j`/`k`、`Shift+↑`/`Shift+↓` | `DashboardSelectNext`/`Prev`；`DashboardReorderUp`/`Down` | `:903-928`；`:1010-1035` |
| Dashboard | `Ctrl+T` / `Ctrl+R` / `Ctrl+G` | `DashboardTogglePin` / `DashboardBeginRename` / `DashboardToggleGrouping` | `:929-943`；`:944-956`；`:992-1009` |
| Dashboard | `Ctrl+X` / `Esc` | `DashboardStop`（运行中取消，空闲后 2 s 内再按永久删除）/ `DashboardExit`（级联：先关 peek → 清过滤 → …） | `:957-971`；`:1062-1076` |
| Dashboard | `Shift+Tab`（3 种编码）/ `Ctrl+.`/`?` | `DashboardCycleMode` / `DashboardShortcutsHelp` | `:972-991`；`:1036-1058` |
| Dashboard | `Ctrl+O` / `Ctrl+L` / `Ctrl+W` | `DashboardToggleAutoApprove` / `DashboardOpenLocationPicker` / `DashboardToggleWorktree` | `:1079-1127` |
| DashboardOverlay | `Ctrl+\` / `Ctrl+[`/`Ctrl+]` / `Ctrl+X` | `Exit` 浮层退出 / `Prev`/`Next` / `Stop`（**需双击**） | `:1131-1191` |
| 模态·卡片 | `Enter`·`1`–`9`·`←→`·`e`·`Ctrl+O`·`Ctrl+C`·`Esc` | 权限卡：接受光标项 / 数字直选 / 调作用域 / 编辑 glob / 开 always-approve / 取消 / 交出焦点（≠拒绝） | `interactions.rs:88-101, 105-213` |
| 模态·卡片 | `a`·`s`·`c`·`q`·`y`；`1`–`9`·`a`–`f`·`z`·`Shift+X`；`y`·`d`·`Ctrl+C`·`Tab`·`Esc` | 计划批准（approve / request changes / comment / quit plan / copy plan）；提问卡（直选 / 自由文本 / 跳过整组，`g G h j k l n s` 留给导航）；elicitation（Accept / Decline / cancel / next field / 逐层退出）；三个下拉共用劫持方向键的骨架、`Tab` 只接受不执行 | `line_viewer.rs:1660-1760`；`question_view.rs:1028-1046`；`elicitation_view/render.rs:141-154`；`prompt.rs:158-253` |

#### 注册表之外的三处键位孤岛与元测试

- **欢迎页**：`When::WelcomeScreen` 零注册，`handle_welcome_input()` 用内联 `key!()` 字面匹配（`Ctrl+W` `:3696`、`Ctrl+I` `:3708`、`Ctrl+Shift+I` `:3711`）；唯一例外是 `Ctrl+R` 走 `ctx.registry.matches_id(ActionId::OpenSessions, key)`（`:3699`）。**scrollback 搜索**的 `/` 是 vim 下的内联处理器（`app/agent_view/panes.rs:111-120`），simple 模式改由 `/find`；**Prompt 的 `↑` 历史、粘贴、撤销/重做**是 textarea 级和弦。三者都以伪行形式手工写进速查表，`long_help` 常量手工维护（`views/shortcuts_help.rs:230-310, 80-125`）；thinking 块折叠提示 `"ctrl+e to expand"` 也是常量，TODO 注明因为它不走注册表、重映射后无法正确宣传（`scrollback/blocks/thinking.rs:16-17`）。
- **超时四值**：`PendingAction::TTL` 1000 ms（`NewSession`/`Quit` 双击）、`ESC_DOUBLE_PRESS_TTL` 800 ms（空闲 `Esc Esc` 清空/回退）、Dashboard 删除确认 2 s（`CONFIRM_WINDOW`，经 `PendingAction::with_ttl` 传入）、`ESC_CANCEL_REWIND_GRACE` 1000 ms（回合中途按 Esc 后推迟「回退 arm」，防连按 Esc 跨过回合结束时误开回退选择器）（`app/app_view.rs:474-489`；`app/agent_view/prompt.rs:739-746, 837-843`）。消费逻辑在 `handle_input_at_with_paste_provenance` 最前面；细节修正：pending 是 `ClearPrompt`/`RewindShowPicker` 而回合已跑起来时视为过期不触发（`app/app_view.rs:2320-2360`）。`Esc` 窗口有仅测试用的 `GROK_ESC_DOUBLE_PRESS_MS`，上限 60 000 ms（原因是 pty_e2e 分片的渲染往返可能超过 800 ms 默认值）。
- **冲突只在渲染期处理**：注册期无校验，同上下文抢键由「`Vec` 顺序先命中者胜」决定、无断言保护（`actions/mod.rs:277-290`）；速查表按 `Category` 分组后用 `seen_in_cat` 对 `default_key` 去重，同键冲突时「**激活上下文的那一行胜出**」（`views/shortcuts_help.rs:145-148, 236-248`），overlay 的键从其他上下文的**亮行**剔除、整行全被遮蔽则整行消失（`:137-144, 208-222`），同义 alt 折叠（`:170-185`）。源码里唯一写下来的冲突规避是三处注释，属「人工约定 + 测试钉住」（`actions/defaults.rs:883-890, 1176, 994-998`）。
- **元测试的常见误解**：`tests/registered_features_are_documented.rs` 与键盘无关（23 行，校验 `FEATURES` 特性开关表里每个 `spec.key`/`spec.env` 都出现在两份文档里），项目里**没有**「键位文档完整性测试」的等价物。真正的键盘测试在 `actions/mod.rs` 内嵌 `mod tests`（约 530 行）、`views/shortcuts_help_tests.rs`（69 个 `#[test]`）与 `app/app_view_tests.rs`。
- **命令面板未接入注册表**：`actions/mod.rs:5` 声称 "**Command palette**: `registry.all()` returns the fuzzy searchable list"，但全仓库 `registry.all()` 的生产调用点为零；面板由 `views/modal.rs:376` 的 `default_palette_entries()` 手写构建，`shortcut` 是**硬编码字符串**（`:387-391`），可能与实际绑定漂移（VS Code family 下 `Quit` 主键已变 `Ctrl+D`，面板仍是字面量）。

#### 终端差异屏蔽的四层

- **协议协商**：只推 `DISAMBIGUATE_ESCAPE_CODES`；DA2 版本 ≤ 阈值常量 `2401`（Alacritty 0.14.x 及更旧，`alacritty_terminal` ≤ 0.24.1）时不叠加 `REPORT_EVENT_TYPES`——它对 Backspace/Tab/Enter/Escape 重复上报 legacy release，会被 crossterm 读成第二次 Press 导致 Enter 双提交（`terminal/kitty_keyboard.rs:29-50, 74-76`）。由此区分三个谓词 `kitty_flags_pushed()` / `kitty_releases_reported()` / `kitty_event_types_withheld()`，注释要求所有等待 release 的特性（hold-to-talk、修饰键抬起跟踪）只 gate 在后者上（`:65-83`）。**未知版本不降级**：DA2 在多路复用器下常被跳过，"no answer" 不能剥夺健康终端的 release 事件（`:34, 109-116`）。`kitty_skip_reason()` 覆盖 9 类原因：`vscode`（含 Cursor/Windsurf/Zed）、`apple_terminal`、`vte`、`windows_terminal`、`jetbrains`、`screen`、`tmux_old`、`tmux_extended_keys_off`、`unknown_no_multiplexer`（`terminal/mod.rs:317-352`）；最后一条的理由是「没有 KKP 支持的正面证据就跳过」，因为 xterm.js 会错误编码 shifted 键、而探测无响应的终端会阻塞启动（`:342-347`）。`/gboom` 打开时再叠一层 `REPORT_ALL_KEYS_AS_ESCAPE_CODES`（WASD 需要 release），用独立原子量跟踪、teardown 按 LIFO 弹两层（`app/mod.rs:105-133`）。
- **OS 修饰键探针救援**：`KeyboardNormalizer::rescue_key()` 在 `handle_input` 顶部运行（`input/keyboard_normalizer.rs:38-40`）。裸 `\x02` → `Char('b')+CONTROL` 与终端能力无关、总是执行；仅 `Backspace`/`Delete` 且终端被判定丢失该修饰轴（`ModifierFate::Dropped`）时，用 macOS CoreGraphics 读物理键状态补 `SUPER` 或 `ALT`，`Cmd` 优先于 `Opt`（理由：「Cmd+Backspace（行杀）是更强的动作」，`:74`）。`suppress_os_modifier_rescue()` 是 thread-local 可嵌套 guard——重放启动期排队的事件时不能拿**当前**物理状态补键，否则会凭空造出用户没按的和弦（`input/terminal_support.rs:11-31`；`app/event_loop.rs:3338-3340`）。Apple Terminal 的 `Enter` 特例由 `is_mod_enter()` 处理，把「终端丢标志但物理上按着 Shift/Opt/Cmd 的裸 Enter」统一视为换行，且**显式排除 SUPER**（很多终端把 Cmd+Enter 绑成全屏）（`input/terminal_support.rs:33-63`）。
- **品牌分支默认键位**：`terminal_context()` 的 `TerminalName`（18 变体 + `Unknown`，`terminal/mod.rs:58-107`）在 `default_actions()` 开头读一次，决定 6 处键位分叉（`actions/defaults.rs:60-73`）。`ctrl_dot_unreliable()` 直接复用 `kitty_skip_reason().is_some()`（没有 KKP 时 `Ctrl+.` 会塌缩成裸 `.`），保证「键位可靠性」与「协议支持」两个判断永不漂移（`terminal/mod.rs:432-435`）。`shift_tab_keys()` 集中返回 `[BackTab, BackTab+SHIFT, Tab+SHIFT]`（`input/key.rs:287-295`），因为 registry 是**精确修饰键**匹配、带 SHIFT 的形态必须进 `alt_keys`。
- **语义归一**：`KeyShortcut::new` 让 `Char('G')+NONE` 与 `Char('g')+SHIFT` 等价（`input/key.rs:46-66`）；`matches()` 对 `KeyEventKind::Release` 一律返回 false（`:60-62`）。三套显示形态用途严格分离：`display()`（栏内紧凑小写 `Ctrl+c`）、`display_pretty()`（速查表 `Ctrl+C` 但 `Ctrl+l` 小写字母）、`display_telemetry()`（遥测、跨平台稳定、字母大写），测试 `telemetry_chord_is_platform_stable` 明确断言后两者不可混用（`actions/mod.rs:533-541`；`input/key.rs:139-235`）。`Ctrl+Space` 无 KKP 时解码为 NUL 但上报为 `Char(' ')+CONTROL`，因此语音和弦在所有终端都能解析（`actions/defaults.rs:670-672`）。

### 5.2 鼠标、选择与剪贴板

### 本节要点

- 鼠标链路是「**App 层归一化滚动 / Pane 层算命中 / 渲染帧提供几何**」三层分工：非滚轮事件全部按**上一帧缓存矩形**命中，坐标基准来自渲染期产物 `ResolvedSelectionModel`（每帧重建，注释写明「命中测试始终假设几何来自上一帧」）。
- 双击的语义由设置项 `keep_text_selection` 决定：默认 `flash` 时双击是**折叠/导航**而非选词，只有 `word_select` 才变成选词；该能力通过可 `Ctrl+Y` 就地接受的提示灰度投放。
- 选择端点是稳定的 `(entry_idx, range_id, block_line_idx, col_within_range)` 四元组，屏幕坐标只是每帧反查；软/硬换行的区别保存在 `joiner_to_previous`，决定复制出来是段落还是多行。
- 复制一次**同时写三条 leg**（native / tmux `load-buffer` / OSC 52），`ClipboardDelivery` 只在「某条 leg 真的到达被信任目的地」时判 `Confirmed`，SSH 未识别品牌时降级为 `Unverified` 而非失败；备份文件 `~/.grok/last-copy.txt` 无论成败都会写。
- **「按住 Shift 让出鼠标给终端」在 app 代码里没有实现**（`handle_mouse` 全程不检查 Shift 位），放行是**终端层的手势约定**；app 侧只有 minimal 模式与 opt-in 的 `/toggle-mouse-reporting` 两条出口。
- 反直觉的取舍：交互冲突用「延迟武装」而不是模态解决——滚动区按下未命中文本时不立刻决定是块拖选还是文本拖选。

#### 命中分层与交互清单

非滚轮事件入口是 `AgentView::handle_mouse`（`app/mouse.rs:30`），它只做两件事：比对上一帧缓存的 `hit_*` 矩形、把 Scrollback 区域分派给文本拖选 / 块拖选。区域从上到下依次是——**状态条/按钮行**（`hit_todo_close`、`hit_bg_status`、`hit_cancel_button` …，左键 Down 触发 Action，全部要求 `!pos_occluded()`，`app/mouse.rs:34-180`）；**粘性头部**（自己发布 `ResolvedSelectableLine` 并 `splice(0..0, …)` 插到模型最前，可被拖选但被自动滚动区域剔除，`scrollback/scrollback_pane.rs:56-74`；`selection.rs:963-998`）；**滚动区内容**（单击选块 / 拖动拖选 / 双击折叠或选词 / 三击单元格或整段，`app/mouse.rs:826-868`）；**时间线轨道** `timeline_rail.hit`（点箭头跳轮次，`:384-396`）；**滚动条** `hit_scrollbar`（点击与拖动复用同一套 `scrollbar_click_to_offset` 算术，`:378-383, 1225-1256`）；**滚动区与输入框之间的空带**（只延迟武装文本拖选，`selection.rs:592-624`）；**prompt 输入框**（textarea 自管，双击是纯按时间配对、不比坐标，注释：配对错误只损失一步 undo，`app/mouse.rs:19-26`）。点击定焦走 `pane_areas.hit_test → set_active_pane`（`:423, :718`）；中键仅在 Linux 有行为：`Down(Middle)` 且无修饰键时读 X11 PRIMARY、转成 `Event::Paste`，不触发文本拖选（`app/event_loop.rs:3158-3176`）。

**右键在主视图未找到交互证据**：`Down` 分支只匹配 `MouseButton::Left`（`app/mouse.rs:32`），全仓仅 `app/agent_view/modals.rs:1266-1270` 把右键/中键当成「任意按下即关掉模态错误条」。模态内的拖选另有独立实现 `views/drag_select.rs`，只负责「鼠标单元格 ↔ 文本行」映射与色带绘制，状态机与复制都在调用方（`:1-5, 43-59, 146-180`）。

**多击有两套计数**：文本级以 `(entry_idx, range_id, block_line_idx)` 为准、窗口 `MULTI_CLICK_TIMEOUT_MS = 300`，且只有**精确落在可选列**上才算（`app/agent_view/mod.rs:412-424`；`scrollback/text_selection.rs:390-410`）；块级 `last_click` 只比 `entry_idx`（`app/mouse.rs:1082-1095`）。文本路径：第 2 击 `select_word_at` 选词或 URL、`origin = DoubleClick`、立即防抖复制（节流 500 ms）；第 3 击先试 `select_cell_at`（表格单元格，含整表退化），失败则 `select_paragraph_at`；第 4 击起不处理（`selection.rs:1254-1335`；`notices.rs:255-267`）。**但这只在 `word_select` 模式下生效**（`is_text_selection_on_double_click()` 读 `keep_text_selection`，`app/agent_view/mod.rs:520-525`；`render/src/appearance/text_selection.rs:11-52`）；默认 `flash` 下双击走 `handle_scrollback_click`：双击组头展开/折叠、双击 bg-task 或子会话打开 viewer、双击用户消息折叠并滚到顶部（in-place 编辑被 `INLINE_EDIT_ENABLED` 门控关掉，`selection.rs:1082-1228`）。

#### 滚轮：归一化加手写惯性

滚轮不在 `mouse.rs` 处理，而在 App 层：`ScrollDirection::from_mouse_event` → `MouseScrollState::on_scroll_event_at` → 按行数 `dispatch_scroll`（`app/app_view.rs:2367-2384, :2227`）。归一化参数（`render/src/input/mouse.rs:46-77`）：流间隔 80 ms 切分事件流、刷帧 16 ms、默认 3 事件/格、轨迹板加速度上限 3、带宽区间阈值 8 ms / 20 ms、倍率 1.0 / 1.6 / 2.5、滚动窗口 6 次事件、单次刷屏增量下限 6 行。「每个终端每格发几个事件」是硬编码表（`:282-330`）：iTerm2 / WezTerm / VS Code 系 / Zed / Cursor / Windsurf = **1**，Apple Terminal / Warp / Alacritty / Rio / foot / Ghostty / Kitty = **3**，**经 multiplexer 重新分块时强制 1**；`wheel_lines_per_tick` 在 iTerm2/WezTerm 或 multiplexer 下 1 行、其余 3 行；VS Code 嵌入终端的轨迹板补偿为 15 行/格、检测窗口放宽到 25/50/60 ms。模式判定是启发式「偏向轨迹板（避免过冲），只在第一格事件极快时才升级成滚轮」，可用 `scroll_mode` / `GROK_SCROLL_MODE` 强制（`:10-12, 25-33`）。

#### 二维选择模型

渲染期把每个可见块的每个可选行写成 `ResolvedSelectableLine`（写入点 `scrollback/render.rs:479` 合成组头行、`:542` 普通行），字段即映射表（`scrollback/text_selection.rs:142-159`）：`screen_x`/`screen_y`（该行首字符所在屏幕单元格）、`selectable_cols`（行内可选列区间，相对 `screen_x`）、`block_line_idx`（**稳定行号**：块内完整输出的行序号，不随视口滚动变化）、`text`/`painted_region`（复制用的逻辑文本 / 实际绘制的单元格文本）、`joiner_to_previous`（软换行拼接符，`None` 表示硬换行）。命中有三个粒度：`hit_test_text_exact`（只认距离 0，用于双击选词）、`hit_test_selectable_range`（取最近行，用于首次按下）、`hit_test_nearest_in_range`（**锁在锚点区间内**取最近，用于拖动；注释明说这样「拖到死行会继续延伸而不会缩回」，`:299-334`）；每行列度量统一由 `col_metrics` 提供，注释强调这是「同一行行为不可能分歧」的唯一入口（`:165-183`）。

端点四元组 `RangeHit` 额外记录 `anchor_content_width`——按下瞬间的块内容宽度，因为流式重排会让所有 `block_line_idx` 平移，复制必须用「同一几何」重建文本（`:199-225`；`app/agent_view/selection.rs:717-796`）。跨块能力由另一套结构 `ActiveBlockDrag { anchor_entry_idx, head_entry_idx }` 表达，复制时把区间内每个块的 `copy_visible_text_in_state` 结果用 `"\n\n"` 拼接（`text_selection.rs:430-434`；`selection.rs:1000-1077`）。

```
按下：命中可选文本 → PendingTextDrag{anchor,start}                 selection.rs:471-508
      否则          → PendingBlockDrag + deferred_text_press       :893-918
拖动：位移 ≥ 1 单元格即升级（text_selection.rs:436-440）
  ├ PendingTextDrag → ActiveTextDrag（含表格 kind 求解） :525-591 ｜ deferred press → 转文本拖选 :596-624
  ├ PendingBlockDrag → ActiveBlockDrag :920-944 ｜ 按指针位置重算 drag_autoscroll :626-649, 963-998
抬起：ActiveTextDrag → 重建文本 → 成功才落持久高亮 → 复制 → toast（「复制失败就不留空高亮」）:870-891
      ActiveBlockDrag → 逐块拼接复制 :1000-1077 ｜ 未升过级 → 交回点击路径（含链接点击）
```

`handle_scrollback_drag_motion` 的分支顺序本身是设计约束：滚动条 → 活动文本拖选 → deferred 转换 → 活动块拖选 → pending 文本 → pending 块；末尾注释写明「带锚定的无锚按下独占整个手势，避免拖拽漏到 prompt 里变成编辑文本」（`selection.rs:692-697`）。拖到边缘的自动滚动由 **30 fps** tick 驱动（`app/app_view.rs:5279, :5442`）：`EDGE_THRESHOLD = 2` 行，速率按距离分四档 **1 / 2 / 3 / 5** 行每拍（`text_selection.rs:35-77`）；每拍先用**旧**模型重算拖选头保响应，渲染后再对**新**模型重夹一次（`selection.rs:302-310, :361`）。

**高亮只有一个函数**（`text_selection.rs:848-861`）：反相色带 `bg = theme.text_primary`、`fg = theme.bg_base`；若 `text_primary` 或 `bg_base` 是 `Color::Reset`（无色 / 终端原生主题）则回退 `Modifier::REVERSED`，并清掉搜索高亮设过的 `REVERSED`。块拖选是整块矩形铺底色（`:491-511`）。另一种「选择框」`SelectionBox` 是块选中态的边框（侧边 `│`、上下角 `┌┐└┘`，越界用虚线 `┆`），它跨组件绘制，所以渲染函数不直接画、而是回传 `RenderOutput.selection_box` 让 frame 最后画（`scrollback/selection.rs:1-3, 14-43, 152-242`）。持久性由 `SelectionOrigin`（Drag / DoubleClick / TripleClick）与设置共同决定：`flash` 下 150 ms 后清除，`hold` / `word_select` 只靠 Esc / 点击 / 滚动清除（`app/agent_view/mod.rs:431`；`selection.rs:46-59`）。

**表格是选区的一等特例**：`SelectionKind` 有 `TableCell`（头锁在锚点单元格内、按该单元格列带跨其换行片段）与 `TableGrid { anchor, head }`（矩形单元格区间，按 **TSV** 复制）两档（`text_selection.rs:240-251`）。拖动时由 `resolve_table_drag_kind` 逐帧求解，几何作为 side-car `TableSelectionGeometry` 挂键在 `(entry_idx, range_id)` 上、键不匹配就整段不画（`:734-752, 255-267`）；表格复制前会**重新探测几何并要求完全相等**，否则退化为线性复制，且持久高亮记录的 `kind` 是「实际进剪贴板的形状」而不是拖动形状（`selection.rs:720-744, 798-812`）。

#### 鼠标复制与终端原生复制的共存

三条并存的路径：**app 内拖选**（默认，鼠标被捕获，mouse-up 即写剪贴板并显示 toast）；**整块复制**（`CopyBlockContent` / 块拖选）；**交回终端**（`minimal` 模式直接关闭鼠标捕获，或 opt-in 的 `/toggle-mouse-reporting`／scrollback 聚焦时 `Ctrl+R`）（`slash/commands/toggle_mouse_reporting.rs:1-8`；`actions/defaults.rs:858-873`）。进程级单一状态位 `MOUSE_CAPTURE_ENABLED` 覆盖 crossterm `EnableMouseCapture` 打开的 5 个 DEC 模式加 bracketed paste（`app/mod.rs:134-135`）。切换点：启动/内联 `store(!want_minimal, …)`、全屏路径显式置 true；切到 minimal 先 swap 成 false 再发 `DisableMouseCapture`，若是「鼠标上报会泄漏成文本」的终端额外补一发 ANSI 复位（`app/mode_switch.rs:114-128`），启动前就判定会泄漏的终端（JetBrains + Windows，即 JediTerm）被自动选成 minimal（`app/mod.rs:136-142`；`terminal/mod.rs:734-738`）。**原生选择持有期**：欢迎页展示认证 URL 时临时关闭捕获让终端接管拖选，窗口结束再恢复，且「只有我们自己拿掉的才恢复」（`app/app_view.rs:4287-4315`）。**重聚焦重发**：ConPTY 转发可能吞掉 DEC 私有模式、把 SGR 上报降级成 X10 进而被解析成乱码，因此每次重聚焦在已开启前提下重发一次 capture（`app/event_loop.rs:655-670`）。Windows 特例：`EnableMouseCapture` 走 winapi 且 `SetConsoleMode` **会跨进程残留**，因此「终端拥有鼠标」时显式打开 QuickEdit、清掉 `ENABLE_MOUSE_INPUT`、保留窗口尺寸事件（`app/mod.rs:1309-1326, 1357-1375`）。

**「按住 Shift 放行」在代码里未找到证据**：`handle_mouse` 全程不检查 `mouse.modifiers` 的 Shift 位，只查平台链接修饰键（`app/mouse.rs:411, :727, :1201`）。放行是终端层的手势约定——手册写 `Shift+Insert` 是终端原生粘贴，许多终端用 `Shift+中键` 绕过应用鼠标上报（`docs/user-guide/03-keyboard-shortcuts.md:295-301`；`docs/user-guide/21-terminal-support.md:131-135`）。

#### 剪贴板后端与降级

一次复制会同时写多条 leg，路由是一个纯函数（`render/src/clipboard/mod.rs:130-148`，测试可注入 kill-switch）：`native: true` 始终尝试（macOS `pbcopy` / arboard / CLI 工具如 `wl-copy`、`xclip`）；`tmux_buffer: is_tmux` 走 `tmux load-buffer -`（stdin 走 spool，2 s 上限等待）；`osc52 = !opts.no_osc52 && (cfg!(target_os = "linux") || is_tmux || is_remote() || is_container_no_display() || opts.wrap_sink)`，tmux 下按需包 DCS passthrough（编辑器 `:terminal` 内不包）。调用链 `copy_to_clipboard`（`app/agent_view/notices.rs:248-252`）→ `clipboard::copy_text_or_file`（`clipboard/mod.rs:565-595`）→ `clipboard_write_with_route`（`:225-266`）→ `trust::resolve_copy_decision` → toast + 遥测，另有 `write_copy_fallback` 写 `~/.grok/last-copy.txt`（0600，父目录 0700）。

交付判定是三态 `ClipboardDelivery`（`trust.rs:13-20`）：本机正常且某条 native leg 真写入为 `Confirmed`（Wayland 下必须要么 `wl-copy` 成功，要么「arboard 成功**且** data-control 可用」，`:96-98, 162-175`）；OSC 52 已发出但外层终端品牌未知且在 SSH/容器里为 `Unverified`——注释解释「未知的 SSH 边界会把品牌标记抹掉，此时不该断言失败」（`:131-139`）；全部不可用为 `Failed`，remote/container 与本地用不同文案（`:212-216`）。toast 文案与时长（`clipboard/mod.rs:331-357`）：`Copied!` / `Copied to tmux buffer, paste with prefix + ]` / `Copied via OSC 52.` / `Copy sent. If paste fails, use grok wrap or /minimal.` / `Copy failed. Try /doctor or /minimal.`；成功 30 tick（约 1 s），其余 120 tick（约 4 s）。**备份文件降级链**（`:418-468, 543-595`）：备份文件无论剪贴板成败都会写；剪贴板报成功而文件写失败只记 debug，剪贴板失败而文件成功则 toast 变成「`Copy sent, saved to ~/.grok/last-copy.txt`」，两者都失败才是 `CopyDelivery::Failed`。相关环境变量：`GROK_OSC52_SINK` / `LC_GROK_OSC52_SINK`（`grok wrap` 声明会在本地接住 OSC 52；走 `LC_*` 是因为 SSH 默认只转发 `LC_*`，`:35-43`）、`GROK_CLIPBOARD_NO_OSC52`（全局关掉 OSC 52，`:45-49`）、`GROK_COPY_FILE`（备份路径覆盖，`:19-21, 471-481`）、`GROK_CLIPBOARD_NO_DATA_CONTROL`（禁用 Wayland data-control 后端、退回 CLI 工具）。

两条相关提示：**双击选词发现提示**在 fold/nav 模式下一次「重复的双击助手文本」会弹 `Want double-click to select? /settings → Text selection · Ctrl+Y: enable now`，同一 session 最多 3 次、约 20 s 窗口、可 `Ctrl+Y` 就地接受（触发窗口 10 s，`tips/word_select.rs:16-26, 36-52`；`selection.rs:23-26`）；**剪贴板里有图片的提示**是机会式轮询，节流 1 s 只读一次 changeCount、只有变化才做较重的类型判定，触发冷却 30 s（`tips/clipboard_focus.rs:29-34, 118-166`）。

#### OSC 8 超链接

`HyperlinkCapabilities` 把品牌差异收敛成一个结构，渲染与输入都只读它（`render/src/terminal/hyperlinks.rs:44-57`）：Apple Terminal 是 `HostileParser`（五个能力全 ✗——解析器会咬坏未知 OSC）；iTerm2 / Ghostty / Kitty / VTE / Terminator / Windows Terminal / Alacritty / Rio / foot / WezTerm 支持 `osc8 = Native` 与 `id_param`，其中 iTerm2/Ghostty/Kitty 还支持 `osc22_cursor`；VS Code / Cursor / Windsurf / Zed 同为 `Native` 且有 `native_link_hover`；Warp 是 `Unsupported` 但**唯一** `native_plain_url_open` 为真的品牌（测试 `:242-257` 断言这一点）；JetBrains / GrokDesktop / Otty / Unknown 全是 `Unknown`。

是否真的发序列由 route 决定：`emit_osc8 = (osc8 == Native) && skip_reason.is_none()`、`emit_id = emit_osc8 && id_param`，路由进程内缓存一次（`hyperlink_route.rs:20-40`）。`hyperlink_skip_reason` 按固定顺序给人类可读原因（`terminal/mod.rs:440-468`）：`apple_terminal` → `unsupported_terminal`（Warp）→ `vte_old`（VTE 版本号 < 5004，**先于 multiplexer 判定**，避免用户白折腾 tmux 配置）→ `unknown_terminal` → `screen` → `tmux_old`（tmux < 3.4）。

生成分两步：扫描期 `scan_lines_for_url_overlays` 在**软换行重新拼好的逻辑行**上跑 `linkify` 与自写正则，再把命中拆回各绘制行；扫描分配的 id 从 `1 << 31`（`SCANNER_ID_BASE`）起，避免与 markdown 文档内从 0 递增的源 id 撞号而把两处不同链接合并成一条（`render/osc8.rs:1-6, 97-100, 395-397`）。刷帧期由 `xai_ratatui_inline` 把 `LinkSpan` 折成逐单元格的 link 层参与帧差分，因此**链接受 diff 管理**（新增/移动会重写单元格，移除会重写单元格但不再带 OSC 8，`xai-ratatui-inline/src/terminal.rs:19-44, 343-380`）。序列形如 `\x1b]8;id={id};{sanitized}\x07`，URL 中的控制字符会被剥掉；`id=` 会被**重铸**成进程内单调 id，因为 markdown id 是按文档从 0 计数、而 OSC 8 的 `id=` 是终端全局的——折行的同一链接靠这个 id 在终端侧被视为一体（`:55-92`）。

点击打开：按下时 `try_arm_link_click` 要求持有平台链接修饰键、未被遮挡、命中 `visible_link_map`，有原生 hover 的终端直接返回 false 让终端处理（`app/agent_view/links.rs:168-179`；macOS 走 CoreGraphics 快照判 Cmd，其他平台用事件里的 Ctrl，`mod.rs:443-452`）；抬起时只有「按下与抬起是同一单元格」才触发 `Action::OpenLink`（`app/mouse.rs:817-822`）。规则是**Warp 之外一律由 app 打开**，Warp 上只有「标准 scheme 且显示文本就是裸 URL」的链接交给终端（`mod.rs:489-519`）。打开走 `link_opener::open_url`：macOS `open` / 其他 `xdg-open`，分离 stdio；无 `DISPLAY`/`WAYLAND_DISPLAY`/`BROWSER` 时**不 spawn**，改为提示 `Could not open a browser. Open this URL manually` 并附完整 URL；安全上先过 scheme 过滤（`Standard` = http/https/mailto；`EditorExtended` 另含 file/vscode/cursor/idea/zed），Windows 走 `ShellExecuteW` 而不是 `cmd /c start`（注释说明 `start` 会被 `&` 与 `%VAR%` 劫持）（`link_opener.rs:41-56, 60-127`）。OSC 22 手型光标只在 iTerm2 / Ghostty / Kitty 且 `osc22_cursor` 为真时切换（`terminal/hyperlinks.rs:183-213`；`app/app_view.rs:4330-4345`）。无原生 hover 的终端上由 app 自绘 hover；macOS 需要轮询 Cmd 状态，只在指针近期移动过（3 s 窗口）或链接高亮仍激活时才轮询，避免空闲占用动画循环；切换 screen mode 后用「屏幕外 Moved」重放一遍清掉残留 hover（`links.rs:216-252`；`app/mouse.rs:1211-1221`）。

识别范围不止 URL，还包括无空格路径段、含内部空格但必须以 `.ext` 结尾的段（macOS app bundle）、相对路径（必须带扩展名，避免 "and/or"、"TCP/IP" 被误判）、带引号的路径（`render/osc8.rs:199-250`）；相对路径只在「能唯一匹配本会话生成的媒体文件」或「join 到 cwd 且不逃出 cwd」时才成链接（`:332-360`）；邮箱补 `mailto:`，但 scp 风格的 `git@github.com:org/repo` 故意不识别（`:181-183`）。**「URL 截断显示」未找到通用实现**：只有 Web Fetch 工具头按可用宽度截断显示文本（链接目标仍完整）与浏览器不可用 toast 两处局部处理（`scrollback/blocks/tool/web_fetch.rs:110-124`；`link_opener.rs:48-56`）。

ssh / tmux / 特殊终端的兼容处理共用一组开关：tmux < 3.4 与 screen 不发 OSC 8；VTE < 0.50.4 不发且优先于 tmux 判定；tmux 内复制额外写 paste buffer 并把 OSC 52 包 DCS passthrough；multiplexer 下滚轮强制 1 事件/格、每格 1 行；SSH 复制不可达时仍写备份文件、toast 报路径、OSC 52 记 `Unverified`；Apple Terminal over SSH 不支持 OSC 52，只能靠备份文件 / `/copy` / `/minimal` / `grok wrap`（`terminal/mod.rs:449-466`；`clipboard/mod.rs:81-88, 130-148, 441-458`；`input/mouse.rs:287-330`；`docs/user-guide/21-terminal-support.md:141-152`）。两个专门的容错件：未用 wrap 的 SSH 会话一次性提示 `Run /doctor for details and fixes.`（`tips/ssh_wrap.rs:23-49`）；`X10ReassemblyFilter` 把「魔数形状的鼠标事件 + 紧随其后的位移行字节」在 `MAX_COMPLETION_GAP = 50ms` 内重组成一条真实事件（`app/x10_filter.rs:11-70`）。

**未找到证据 / 未覆盖**：Shift 放行无实现；右键在主视图无功能；三击在表格边框上升级为整表复制、其完整行为未逐行阅读；「URL 显示截断」无通用组件；`views/shortcuts_help.rs` 的鼠标只有「详情模式下滚轮翻 1 行」与「把事件转交 picker」两件事（`:960-987`）；拖选期间向编辑器/粘贴板写入图片（`wrap_clipboard_image.rs`）与语音等其它剪贴板方向未覆盖。

### 5.3 命令、补全与搜索

### 本节要点

- slash 命令是一个约 25 个方法的 `SlashCommand` trait，执行是同步的，需要异步的一律返回 `CommandResult::Action` 交给 dispatch；内置命令共 **74** 个，定义在单个 `builtin_commands()` vec 中。
- **分类即注释分组、菜单顺序即 vec 顺序**：没有独立的分类枚举，而这个顺序是空查询排序的最终 tiebreak（源码注释「moving an entry moves it in the menu」）。
- 注册表把「命令对象」与「菜单条目」分离：每个 canonical/alias 生成一条 `CommandTrigger`，技能额外生成裸后缀 trigger，模糊匹配只作用于 `trigger.match_text`；可见性有三档闸门 `hidden` / `menu_hidden` / `restricted`，第三档刻意保留可发现性。
- 补全是**单列**下拉：标签列宽 = min(最长标签, min(内容宽 × 3/5, 40))，可见行数上限 8，溢出显示最右 1 列滚动条；`Tab` 只接受不执行，`Enter` 有三路分支。
- 模糊匹配是 nucleo `MultiPattern(1)` 的薄封装，`CaseMatching::Smart` + `Normalization::Smart`，返回**字符级**高亮下标；排序靠 MRU 衰减分（7 天半衰期、下限 0.1）而非原始时间戳或使用次数。
- 历史搜索**没有 Ctrl-R 之类的键**：入口是 `/history` 与空提示符按 `↑`；匹配跑后台线程、上限 100 条、两端停住不环绕。

#### slash 命令体系

`SlashCommand` trait（`crates/codegen/xai-grok-pager/src/slash/command.rs:213`）的契约分三类：身份与展示（`name()` / `aliases()` / `description()` / `usage()`）；静态与动态两档能力声明（`takes_args()` 与 `takes_args_now(ctx)`——后者只看「当前这一刻能不能接参数」，被下拉的尾随空格、参数阶段快照与参数建议三处消费，`:239, :246`）；闸门与副作用（`visible(ctx)` / `session_scoped()` / `offered_when_session_less()` / `dashboard_only()` / `mode_support()` / `required_tools()`（fail-closed）/ `submission_refusal()`（在提交路径改动 composer 之前拒绝）/ `run()`）。回传是 8 变体 `CommandResult`：`Handled` / `Doctor` / `Error` / `Message` / `Action` / `QueueCommand` / `InjectSkill` / `PassThrough`——最后一项是「认不出的命令原样交给 shell/模型」的出口（`:33-64`）。元数据用宏 `slash_meta!` 生成，字段必须按 trait 声明顺序书写、只有显式列出的字段才产生 override（`:357-375`）。

74 个内置命令的**分组即菜单分组**（`slash/commands/mod.rs:77`，行号区间 `:79-165`），空查询时这就是显示顺序，且这个顺序是排序的最终 tiebreak（`:74-76` 注释「moving an entry moves it in the menu」）：首屏 8 条（`/tutorial` `/settings` `/dashboard` `/workflows` `/plugins` `/btw` `/voice` `/new`）；Per turn 6 条（`/effort` `/model` `/context` `/compact` `/fork` `/resume`）；Steering the work in front of you 13 条（`/loop` `/plan` `/view-plan` `/remember` `/memory` `/flush` `/dream` `/recap` `/rewind` `/jump` `/expand` `/edit-prompt` `/queue`）；This session and what came out of it 10 条（`/session-info` `/share` `/rename` `/history` `/transcript` `/export` `/copy` `/find` `/usage` `/tasks`）；Extending the agent 7 条（`/skills` `/mcps` `/hooks` `/marketplace` `/workflow` `/personas` `/config-agents`）；Settings and display 8 条（`/theme` `/auto` `/always-approve` `/vim-mode` `/multiline` `/compact-mode` `/timestamps` `/toggle-mouse-reporting`）；Screen-mode switchers 2 条（`/minimal` `/fullscreen`，只在对面模式可见）；Reached for occasionally 4 条（`/timeline` `/cd` `/imagine` `/imagine-video`）；Docs / account / one-off maintenance 13 条（`/docs` `/release-notes` `/announcements` `/feedback` `/privacy` `/doctor` `/import-claude` `/login` `/logout` `/home` `/delete` `/help` `/exit`）；另有 3 个隐藏条目 `/gboom`（彩蛋，永不列出）、`/scroll-debug`、`/debug`（仅 debug 二进制列出）。除内置命令外，shell 通过 ACP `AvailableCommandsUpdate` 广告的命令与 skill 也进同一菜单，命令总数因此是运行时的（`slash/registry.rs:392, :407`）。

注册表持 10 个字段（`commands` / `sources` / `key_to_index` / `triggers` / `hidden` / `menu_hidden` / `restricted` / `available_tools` / `saved_workflows` / `skipped_acp_names`，`registry.rs:114-135`），其中 `triggers` 是模糊匹配的唯一输入；`bare_suffix_sibling()` 只对 skill 产生裸后缀副本、且它是**独立 trigger**（不是拼接 haystack），目的是让分数与高亮下标保持诚实（`:84-104`）。两张值得照抄的黑名单：`BLOCKED_ACP_NAMES`（`help`、`hooks-*`、`reload-plugins` 等 shell 名字永不被 pager 菜单采用）与 `SHELL_GATED_COMMANDS = ["memory","flush","dream"]`（跟随 shell 自己的门控，ACP 目录广告了才显形）（`:21-30, :110`）。两个查询入口的差别是刻意设计的：`get()` 做完整可见性判定（hidden + menu_hidden + restricted + 工具不足都返回 `None`），`get_for_dispatch()` 只忽略 menu-only 一层——**完全类型化的提交必须能到达 handler**（`:187, :195`）。三档可见性（`:147-158, 512 起`）：`hidden` 不出现在菜单也不能执行（如 `/dashboard`、`/recap`、`/voice`、`/auto`、`/memory`、`/flush`、`/dream`、`/hooks`、`/plugins`）；`menu_hidden` 不进菜单但完全类型化输入仍可执行（`/share`）；`restricted` 仍出现在菜单与 ghost 里（保可发现性），只是执行被拒——测试 `restricted_commands_stay_visible_in_dropdown` 钉住了这一点。

**按模式可用性**用带解药的枚举表达（`slash/mode_support.rs:19-32`）：`ModeSupport::{Both, FullscreenOnly(Remedy), MinimalOnly(Remedy)}`，`Remedy::{SwitchMode{why}, UseInstead(&str), AlreadyInMode}`；`supports()` 把 `Inline` 归入 fullscreen 一侧，`refusal()` 把拒绝拼成给人看的一句话——能靠切换模式解决的指向 `/fullscreen` 或 `/minimal`，本模式已有等价操作的直接点名替代动作（`:26-53`）。7 条内置命令受限，拒绝理由的原文是：`/dashboard`「minimal is single-session」、`/find`「minimal has no scrollback pane: use your terminal's own search」、`/jump`「minimal scrolls with your terminal's native scrollback」、`/theme`「minimal renders with your terminal's own palette」、`/timeline`「the timeline rail needs the interactive scrollback pane」、`/tutorial`「the tutorial overlay needs fullscreen」（以上 6 条 FullscreenOnly），以及 `/expand` 为 MinimalOnly 且 `UseInstead("press Tab to focus the scrollback, then → on the block")`。`/minimal` 与 `/fullscreen` 自身用 `Remedy::AlreadyInMode`，形成「在你不需要它的那一边才出现」的对称（`slash/commands/screen_mode_switch.rs:57-63`）。拒绝文案只在两处产生：直接提交（`app/dispatch/prompt.rs:871`）与编辑排队行（`app/dispatch/queue.rs:1222`）；`mode_support_tests.rs:94-101` 的 `mode_specific_builtin_refusals_are_pinned` 把所有模式的拒绝句钉在测试里。菜单里另有两层与模式正交的门：`hide_session_scoped` 与 `dashboard_only`（`/cd` 反过来只在无会话面板出现），三者与运算是 `command_offered()`（`slash/mod.rs:1285-1300`）。

#### 补全下拉：排序与视觉

`views/slash_dropdown.rs` 与 `views/completion_dropdown.rs` 共用几何常量：`PREFIX_W = 2`（`"❯ "` 或 `"  "`）、`LABEL_CAP = 40`、`LABEL_DESC_GAP = 2`（`:24-30`）。**是单列，没有多列网格。**

```
┌──────────────────── 提示符上方的下拉（宽 = 提示符区宽度） ─────────────────────┐
│❯ /model              Switch the active model                    built-in     │ ← 选中行：箭头 + BOLD + bg_visual
│  /mo…                (长名字按标签列宽截断为 …)                                │
│  /btw [new]          Ask a side question without interrupting                 │ ← tag 右对齐在标签列内，tab 后是描述
│  /acme:login         Acme account login                        skill · acme   │ ← provenance badge 右对齐到行尾
│                                          ▐                                    │ ← 内容溢出时最右 1 列是滚动条
└───────────────────────────────────────────────────────────────────────────────┘
   标签列宽 = min(max(各行 display + " [tag]" 宽), min(内容宽 × 3/5, 40))
   描述从 (2 + 标签列宽 + 2) 列开始；过长描述按词换行，续行缩进到描述列
```

证据：`compute_label_column_w()`（`:55-63`）、`build_item_lines()`（`:294-395`）、badge 右对齐（`:374-383`）、滚动条（`:205-229`）。completion 版本几乎相同但**不换行、不显示 tag/badge**，只截断描述（`views/completion_dropdown.rs:160-217`），可见行数是 6 且带 1 行分隔符：`dropdown_height = 1 + items.min(6)`（`:18, 29-35`）。slash 侧可见行数 8（`MAX_VISIBLE_SUGGESTIONS = 8`，`slash/mod.rs:39`，被 `MAX_DROPDOWN_ROWS` 复用）；这**不是候选上限**——空查询时 `command_suggestions()` 返回全部可见命令、由渲染层滚动（注释 "No cap here; the dropdown renderer handles scrolling"，`:1021-1023`）。高度按「**行**」而非「条目」计算：`desired_item_rows()` 数描述换行后的物理行数并饱和到 8，避免长描述把后面的候选挤出屏幕（`views/slash_dropdown.rs:32-40`，测试 `:671`）。选中行 = 箭头 + `bg_visual` + `BOLD`；鼠标悬停 `bg_hover`（25% 混合）；Terminal 主题（fg 全为 Reset）改为叠加 `selection_overlay()` / `hover_overlay()` 反显；匹配字符用 `theme.fuzzy_accent` 上色、相邻同类字符合并为一个 `Span`（`:83-118, 400-449`）。

**排序分两条链。** 裸 `/`（空查询）用 `MenuKey`，字段按声明顺序比较（`slash/mod.rs:67-90`）：`group`（`Command` < `BundledSkill` < `OtherSkill`，技能一定沉在命令之下）→ `untagged`（false 在前，有 tag 的行被顶到本组最上）→ `recency`（MRU 衰减分降序，技能恒为 0）→ `name`（技能按 display 小写字母序，命令组为空串以保留注册表顺序）；排序是稳定排序，所以同键的行回到 `builtin_commands()` 的顺序（`:1056-1068`）。带查询时按「分数 + 四级 tiebreak」（`:1172-1193`）：nucleo 分数降序 → `owns_typed_name`（canonical/alias 精确等于所键入文本者优先）→ MRU 衰减分降序 → Builtin 先于 Acp → display 升序。一个命令可能有多条 trigger（alias、技能裸后缀），先按 `command_index` 去重，取胜规则是「分高 > 精确查询 > canonical 优于 alias > display 字典序」（`:1090-1121`）；`owns_typed_name` 存在的理由写在同一文件 `:201-203`——避免 MRU 把同名 skill 顶到刚被完整键入的 builtin 之上。

**MRU 本体**（`slash/mru.rs`）：存 `$GROK_HOME/slash-mru.json`，扁平 `{命令名: last_used_unix秒}`、最多 256 条；平铺不按前缀分桶（旧 schema `by_prefix` 只在迁移时读一次）。排序用衰减分而非原始时间戳——`RECENCY_HALF_LIFE_SECS = 7.0 * 86_400.0`、`RECENCY_FLOOR = 0.1`，`factor = (0.5_f64.powf(age / RECENCY_HALF_LIFE_SECS)).max(RECENCY_FLOOR)`，使「很久以前用过」永远略高于「从没用过」（`:95-104`）。写入路径完全离开 UI 线程：`touch` 只标脏，录制命令时交给唯一的长驻线程 `slash-mru-writer` 做「临时文件 + fsync + rename」；读失败则本会话停用持久化，避免覆盖读不出来的文件（`:266-293, 240-261, 119-127`）。每个 `SlashController` 持 `Rc<RefCell<SlashMru>>` 由 `AppView` 注入共享，agent 提示符与 dashboard dispatch 因此共用一份（`:6-8`；`slash/mod.rs:373`）。

**键盘语义**：三个下拉共用「菜单打开时劫持方向键」的结构，但 Tab/Enter 各自不同。slash 下拉（`app/agent_view/prompt.rs:158-235`）：`↑`/`Ctrl-P` 与 `↓`/`Ctrl-N` 环绕移动，`PageUp`/`PageDown` 按可见行数（8）整页滚动并在两端夹紧，`Esc` 回滚实时预览并关闭；`Tab` **只接受文本、不执行**；`Enter` 三路分支——`is_typed_slash_selected(...)` 为真（光标在命令名内、命令完整、选中行就是所键入的命令或它的别名）时提交预览、关闭下拉、直接发送；否则接受补全：选中行 `insert_text` 以空格结尾（即该命令能接参数）就停在参数阶段不发送，否则关闭下拉并发送（`:207-230`）。`accept_slash_completion()` 按光标所在相位改写不同区间（命令名相位替换 `command_range`，`/mo` → `/model `；参数相位替换 `args_range`），行尾尾随空格会「吸收」已有空格使光标落到分隔符之后（`views/prompt_widget/mod.rs:1336-1396`）。`is_command_complete()` 是 Enter 的另一半契约：`takes_args=false` 或 `args_required=false` 一律完整；`args_required=true` 时必须已有非空参数；**未注册的命令一律视为完整**，交由 shell `PassThrough`（`slash/mod.rs:1423`）。

**行内 ghost 是下拉之外的第二条补全路径**：光标在一个 `/token` 内、该 token 又尚未被认作完整命令时，快照会带一段 ghost 后缀，用 muted 颜色画在 token 之后（`views/prompt_widget/mod.rs:3197-3213`）。ghost 的来源不是独立匹配器，而是**当前下拉选中的那一行**（`sync_inline_ghost_to_selection()`，`slash/mod.rs:234-248`），因此 ghost 与 Tab 接受的结果永远一致；前缀匹配用与 nucleo `CaseMatching::Smart` 对齐的规则——查询全小写忽略大小写、含大写要求精确前缀（`command_prefix_matches_smart()`，`:163-180`）；Tab 在没有下拉时接受 ghost，直接把 token 区间替换成 `/{full_name}`（`app/agent_view/prompt.rs:236-253`）。

第三个下拉 `views/completion_dropdown.rs` 服务 ACP `x.ai/suggest` 返回的补全（bash 模式），与 slash 无关：条目字段 `display` / `description` / `insert_text` / `source`（`None|History|PathExecutable|FilePath|AI`）/ `priority` / `replace_range` + `token_text` / `truncated`（`views/suggestion_controller/mod.rs:53-69`）；响应到达时**按 wire 顺序直出**——**未找到 pager 侧按 `priority` 排序的代码**，顺序由 shell 决定（`:685`）；下拉不会自动打开（注释 "Don't auto-open; Tab opens it whenever items exist"，`:686-687`）；选择环绕（`move_selection`），鼠标滚动夹紧不环绕（`scroll_selection`）（`:235-253`）。

#### 模糊匹配算法与参数

slash 侧是 nucleo 的薄封装 `FuzzyMatcher`（`slash/matcher.rs:14-31`）：`MultiPattern::new(1)` 单列、`Matcher::new(Config::DEFAULT)`，`pattern.reparse(0, trimmed, CaseMatching::Smart, Normalization::Smart, false)`。具体行为：`CaseMatching::Smart` + `Normalization::Smart`（`:57, :104`）；`rank()` 返回 `(index, score)`，按分数降序、同分按 key 文本升序，上限 `limit`（`:74-77`）；空查询退化为「前 limit 条、分数 0、插入序」（`:50-54`）；`indices()` 返回**字符**位置（`Utf32String` 索引，`:86-95`）；匹配失败返回 `None`，例如 `"fix s"` 对 `"ssh-wrap"`（`:98-113`，测试 `:123-126`）。**路径分隔符处理在参数补全那一层**：`argument_highlight_indices()` 取查询的最后一个空白 token，再截掉最后一个 `/` 或 `\`，只拿 basename 做高亮；若匹配失败，再用最后一个 `.` 之后的扩展名重试（`slash/mod.rs:1227-1239`）——命令名的匹配不做这种截断，匹配对象是 `trigger.match_text`。

两条 CJK 相关事实：截断与列宽全部走 `unicode_width`，双宽字符按显示宽度占列（`views/slash_dropdown.rs:343, 454-500`；`views/file_search/dropdown.rs:205-215`）；**行内 token 扫描是字节级的**，`/` 前一个字节必须是 ASCII 空白才认作 token（`slash/mod.rs:1603-1612`），因此紧跟中文字符的 `/`（如 `你好/btw`）不会被识别为命令 token。退化几何测试显式覆盖 CJK 与 emoji（条目含 `/日本語コマンド` 与泰文长词），断言 0–8 列宽、0–3 行高下既不 panic 也不死循环（`views/slash_dropdown.rs:636-667`）。

文件搜索侧用同一「家族」但独立的实现（`crates/codegen/xai-fuzzy-file-search/src/lib.rs`），参数不同：`Nucleo::new(Config::DEFAULT.match_paths(), …, Some(2), 1)` 面向路径匹配（`:197-206`），nucleo 2 线程、ignore walker 8 线程（`:30-31`），大小写/归一化同上（`:415`），**最小分 `min_score = 7 + len * 14`**（`len` 为查询字符数，注释引自 helix，`:479-481`），UI 侧要 1000 条（`views/file_search/state.rs:19`），排序依次按 score、路径长度、路径名（`lib.rs:454`），遍历用 `git_ignore/git_global/git_exclude/ignore/hidden(true)`、`follow_links(false)`、`require_git(false)` 外加 `!.git` 覆盖规则（`:290-310`），空查询走串行 depth-1 walk 并按文件名排序（`:268-287`），退化策略是线程池被拒 → `BrowseOnly`（空查询可浏览、带 key 查询返回空）、daemon 线程被拒 → `Disabled`（`MatcherMode`，`:38-50, 164-193`）。

#### `@` 文件搜索

token 解析规则写死在 `views/file_search/context.rs` 顶部（`:7-18`）：`@` 前一个字符不能是字母数字或下划线（否则 `user@example.com` 会被当成文件引用，`:71-76`）；token 从 `@` 延伸到第一个空白、逗号或分号；光标必须落在 token 范围内，查询是 `@` 之后到光标之间的文本；**dir mode** 查询以 `/` 结尾则只匹配目录（`:33-35`）；**hidden mode** 查询以 `!` 开头则显示隐藏/gitignore 文件并**重启遍历**（`:37-39`；`views/file_search/state.rs:205-243`）；`drill_prefix` 让钻入目录后的空格不再终止 token（`my dir/` 保持一个 token），且一旦文本前缀不再匹配就自动失效（`context.rs:64-72`；`state.rs:196-215`）。面板最多 8 行结果，顶线右侧是 `k/n` 计数（≥1000 显示 `1k+/n`），装饰（`Clear` + 上下横线 + 计数）由 `AgentView` 画、`views/file_search/dropdown.rs` 只画结果行（`app/agent_view/render.rs:2836-2903`；`dropdown.rs:20-22`）；匹配字符用 `theme.fuzzy_accent`，无图标、无文件类型列，dir mode 给目录补尾部斜杠。**无结果即隐藏整个面板**（`item_rows == 0` 时不渲染，`render.rs:2839`），可见性判定是「有 @ 上下文 且 topk 非空」（`state.rs:153-155`）。输入即搜 + 增量结果：每次文本/光标变化都 `update_context()` → `start_query()`（`daemon.set_query`），UI 每 tick 轮询；同一 query 只走 `Keep`，只有 hidden 模式切换才 `Restart` 遍历（`state.rs:137-150, 205-243`）。防闪烁与过期防护：空结果不覆盖已有结果（除非 `status.done`），并用单调递增的 `min_generation` 挡掉上一轮 query 的迟到结果（`:257-300, 83-88`）。**没有多选**：选中是单个 `usize`，接受操作写回一个替换区间（`FileSearchReplacement`，`:72, 26-37`）。键位（`views/prompt_widget/mod.rs:1946-1997`）：`↑`/`Ctrl-P`/`Ctrl-K` 上移、`↓`/`Ctrl-N`/`Ctrl-J` 下移、`PageUp`/`Ctrl-U` 与 `PageDown`/`Ctrl-D` 翻半页（`page_move(±1, 8)`）、`Tab`/`Enter` 接受并补一个尾随空格、`→` 接受**不补空格**且对目录是「钻入」（替换路径、保持 @ 上下文活着）、`:` 或 `Ctrl-L` 接受并用行内查看器打开（仅文件）、`Esc` 关闭。`→` 与 `Tab` 的差别有明确注释：文件下面没有可钻的结构，所以对文件二者等价；对目录，`→` 才是下钻（`:2003-2043`）。

#### 历史搜索与 `mid_text_hoist`

`/history` 对应 `Action::OpenHistorySearch`（`slash/commands/history.rs:22`）。设计上**没有** Ctrl-R 之类的键，`actions/defaults.rs:687-688` 的注释直接写着「Prompt history has no key chord of its own: `/history` opens the search panel; Up on an empty prompt browses」。两种模式共用一个覆盖层、靠 `Mode` 区分（`views/history_search.rs:263-272`）：**Search**（`/history`）把 composer 变成过滤查询框，方向键只移动高亮，Enter/Tab 接受，打开时保存原文（`saved_text`）、关闭时恢复；**Browse**（空提示符按 `↑`）每次移动**实时把选中项写进 composer**，一旦打字就脱离浏览转为编辑，在最新一条按 `↓` 关闭面板（`:385-397, 423-428`）。它是覆盖在提示符上方的独立面板：高度 = `min(结果数, 8) + 2`、上下各一条横线，左上 ` history `、右上结果数，空结果显示 `  no matching history`、加载中显示 `  Loading...`（`app/agent_view/render.rs:2984-2992, 3018-3043, 3060-3072`）。匹配同样跑后台线程：`Daemon` 拥有 nucleo `Matcher` + `MultiPattern(1)`，UI 通过 channel 送 query、每 tick `poll()` 取 `Arc<Mutex<Snapshot>>`（`history_search.rs:1-23, 454-470`）；结果上限 `MAX_RESULTS = 100`（`:55`）。空查询时全部条目**倒序**（最近的在最后）并让选择粘在底部，直到用户主动导航（`stick_to_bottom`，`:279-290`）。**方向键在两端停住不环绕**（测试 `move_up_selects_earlier_then_stops_at_top`、`move_down_at_bottom_reports_end_instead_of_wrapping`）——真正环绕的是 `completion_dropdown` 的 `move_selection`（`views/suggestion_controller/mod.rs:235-242`）；滚动偏移是「选中项贴底」而非居中（`render.rs:3074-3079`）。守护线程只在首次激活时 spawn、每个 `PromptWidget` 一个，注释解释了为什么不能急切创建：每个 subagent 子视图都会永久占一个 parked 线程（`:11-16`）。

`mid_text_hoist` 解决的问题是：提示符里写「先看错误处理，/btw WBC 是什么」时命令不在开头，按常规语义应当作普通文本，但用户显然想要一次旁问。方案是允许**个别命令**声明自己可以被「抬起」到行首（`slash/mid_text_hoist.rs`）：

```rust
// prose /btw q   →   /btw prose q
let before = text.get(..token.range.start)?;
let after  = text.get(token.range.end..)?.trim_start();
let question = format!("{before}{after}").trim();
Some(format!("/{} {question}", command.name()))
```

（`:42-61`。）只有 `can_hoist_from_mid_text() == true` 或 `is_skill()` 的命令能这样用，内置命令里实现该声明的**只有 `/btw`**（`:15-17`；`slash/commands/btw.rs:18`）。重写只在**整条消息不算命令**（不以 `/` 开头）时发生，且 token 只吞掉紧随其后的空白串、换行得以保留（`:1-4, 52-54`）。安全阀是 `token_is_armed_inline()`：行首（前面只有空白）永远算，句中出现则要求 hoist 或 skill——这也是 `recognized_token_ranges()`（青色高亮）的过滤条件（`:19-30`；`slash/mod.rs:937-954`）。手册的概括是 "`/btw` can also appear mid-message: the whole message (minus the token) becomes the side question and nothing goes to the main turn. Only `/btw` works this way"（`docs/user-guide/04-slash-commands.md:347`）；与之相对的负面提示留给 `/goal`：`MID_TEXT_GOAL_NOTICE = "Slash commands only work at the start of the message — put \`/goal …\` first."`（`:10-11`）。

#### 没有补全时如何发现命令

分层机制：**边打字边提示**（下拉 + 行内 ghost（选中行的后缀）+ 参数占位符，如 `[context]` 以 muted 画在光标后，`slash/mod.rs:234`；`views/prompt_widget/mod.rs:3177-3195`）→ **命令目录**（`/help` 打开命令面板，与 `Ctrl+P` 同一入口；minimal 模式没有常驻快捷键脚注，`/help` 因此是官方发现入口并在状态行广告，`slash/commands/help.rs:1-5`）→ **命令面板与速查**（`Ctrl+P` 或 `?` 打开命令面板，每行显示键位或对应 slash 命令；`Ctrl+.` / `Ctrl+X` 打开快捷键速查，按 `Category` 分组、支持搜索与两级展开，`views/shortcuts_help.rs:1-16`）。反馈层三条：**模式受限**给出「为什么 + 出路」的原话，如 `/theme isn't available in minimal mode (minimal renders with your terminal's own palette). Run /fullscreen to switch this session.`（`slash/mode_support.rs:34-53`；`docs/user-guide/04-slash-commands.md:160`）；**参数缺失**时 Enter 不发送、改为接受补全停在参数相位，真正执行时空参数由命令自己报错，如 `/model` 返回 `Usage: /model <name> [effort]`（`slash/commands/model.rs:56-58`）；**未注册命令**不报错，原样 `PassThrough` 交给 shell/模型（`app/dispatch/prompt.rs:885-887`；`slash/mod.rs:1430-1432`）。命名冲突也可见：skill 与 builtin 撞裸名时两行都带 provenance badge `built-in` 与 `skill · <plugin>`（`slash/command.rs:200-209`；`views/slash_dropdown.rs:374-383`）。

### 5.4 审批、提问与确认

### 本节要点

- 五类裁决交互（工具授权 / 计划批准 / 澄清提问 / MCP elicitation / 敏感操作同意）收敛为「**阻塞卡**」，`blocking_card()` 是唯一权威判定且带固定优先级，**permission 可抢断其他卡片**。
- 所有卡片共享同一骨架：左侧 1 列 `accent_bar`（U+2503 `┃`）、`content_x = area.x + 3`、底部固定动作行；失焦时整体降饱和 0.66 **并撤掉焦点行的背景带**，让「视觉上没有选中项」与「按键不会命中某项」保持一致。
- **默认光标在无粘性选择时会落到 YOLO（"always allow on all sessions"）行**，这是配置项的显式默认值；但降级方向是安全的——具体目标行缺失时退到 `AllowOnce`，绝不升级到全局 always-approve。
- **风险分级不靠颜色**：整卡没有按风险级别切换标题色或底色的分支；风险靠措辞、固定顺序（always-allow 打头、persistent deny 压尾）与危险命令被 pin 到完整命令三处表达。
- **失败策略按路径分裂**：本地 TUI 权限卡**没有超时、无限等待**；consent gate **所有失败路径 fail open**；headless 非 yolo 与 hub 远程权限路径 **fail closed / 拒绝**。
- **Esc 不是拒绝**（权限卡 `Esc` = 交出焦点、卡片存留，取消只有 `Ctrl+C`）；计划批准把「请求修改」建模为**非终态**。

#### 五类裁决交互与统一视觉语法

`blocking_card()` 给出固定优先级（`app/agent_view/key_owner.rs:96-105`）：`Permission` → `CancelTurn` → `Question` → `McpElicitation`。`Permission` 的语义是 "a permission request interrupts whatever else is waiting"。计划批准不在这个枚举里，它有自己的 `KeyOwner::PlanApproval`（`app/agent_view/render.rs:295-300`）。

| 类型 | 载体 | 源码入口 | 用户可做的决定 |
| --- | --- | --- | --- |
| 工具授权 | `BlockingCard::Permission` | `app/acp_handler/permissions.rs:3-58` | 允许一次 / 总是允许 / 拒绝一次（可附理由）/ 永不允许 / 开启 always-approve |
| 计划批准 | `KeyOwner::PlanApproval` | `views/plan_approval_view.rs` | `approved` / `cancelled`+feedback / `abandoned` |
| 澄清提问 | `BlockingCard::Question` | `views/question_view.rs` | 逐题选答案 / 自由文本 / 整组跳过 |
| MCP elicitation | `BlockingCard::McpElicitation` | `views/elicitation_view/` | 填表 Accept / Decline / 取消；URL 表单则是 Open URL / Decline |
| 敏感操作同意 | `RewindPhase::Confirm`；consent gate | `views/rewind.rs:446-509`；`app/consent.rs` | rewind 二次确认；法律告知接受/退出 |

（`BlockingCard::CancelTurn` 也是裁决类交互，但属于「打断正在跑的回合」，不在本专题重点内。）五类共享同一骨架：**左侧 1 列 accent 竖条 + 标题 + 正文 + 底部独占的动作行**（`glyphs::accent_bar()` = U+2503 `┃`，legacy ConHost 降级为 `│`），标题 `BOLD` + `theme.text_primary`，正文 `theme.gray` 或语法高亮，错误 banner 用 `theme.accent_error`。权限卡的坐标写死在渲染函数里：`content_x = area.x + 3`、`content_width = area.width - 5`，整个 area 填 `theme.bg_light`（`views/permission_view.rs:442-457`）。卡片不是模态到「不许离开」的程度：焦点移到 scrollback 时卡片仍绘制但整体降饱和（`recede_area(buf, area, theme.bg_light, 0.66)`，`:707-709`；`question_view.rs`、`elicitation_view/render.rs`、`rewind.rs:543-545` 用同一模式，注释互相引用 "Mirrors the unfocused prompt widget pattern"），同时**焦点行的背景带被撤掉**（`:665-671`）——让「视觉上看起来没有选中项」与「按键不会命中某项」保持一致，失焦后按 Enter 不会误批。

#### 工具授权卡

**选项集由 shell 侧构造，pager 只负责渲染。** `AcpPrompter::build_options` 先按访问类型选基础集合，再剥掉被设置门控的行，最后**把 YOLO 行插到第 0 位**（`xai-grok-workspace/src/permission/prompter.rs:506-520`）；`REMEMBER_TOOL_APPROVALS_GATED_IDS` 共 8 个 id（`:290-299`），由 `[ui] remember_tool_approvals` 统一开关（默认开）。bash 分支的排序理由写在注释里（`:579-581`）："Always-allow leads; persistent deny trails so it never sits between safe options"。

```text
┃
┃  Run the build and tests                  ← 标题：优先用 bash description
┃  cargo test --lib && cargo build          ← 命令原文，≤5 行；超出显示 "... Ctrl-F to expand"
┃  ← → narrow scope  ·  e edit pattern      ← 作用域提示行（dim，键位用 accent_user）
┃
┃  1 (●) Yes, and don't ask again for anything (always-approve mode)
┃  2 (○) Always allow: cargo test
┃  3 (○) Yes, proceed
┃  4 (○) No, and tell Grok what to do differently
┃  5 (○) Never allow: cargo test
```

标题逻辑（`app/acp_handler/permissions.rs:226-270`）：执行类优先取 `BashToolInput.description`，否则退到 ``Allow `<bin>`?``；编辑类是 `Allow Edit to {path}?`；其余是 `Allow {tool}?`，并按 `ToolKind` 兜底成 `Allow Edit?` / `Allow Execute?` / `Allow Delete?` / `Allow?`；local workspace 模式下追加 ` (on your machine)`（`:276-290`）。选项行的字节布局是 `"1 "` + `"(●) "` 或 `"(○) "` + 标签三段、`BOLD` 仅当 `is_cursor`，`on` 状态下追加作用域词——bash 的走 `tasks_pane::highlight_bash_command` 做语法高亮、MCP 的走纯文本（`permission_view.rs:1466-1487`）。**折叠与高度**：`PERMISSION_COLLAPSED_ROWS = 5`（`:348`），正文或命令超过 5 行时只画 4 行 + 一行 `... Ctrl-F to expand`（`:356-376, 1351-1361`）；卡片高度默认压到屏幕的一半、展开后解除（`:331-346`）——`let cap = (screen_h as u32 / 2).max(10).min(screen_h as u32 * 80 / 100) as u16;`。

**拒绝时收集理由**：`RejectOnce` 行是**行内可编辑的输入行**，不是普通 radio 行——未输入时标签是灰色占位 `"No, reject (type to add feedback)"`，已有输入则显示首行截断到 50 字符的预览（`:1519-1528`）。在 Options 焦点下直接敲非数字字符会进入 `PermissionFocus::FollowupInput` 并把按键转给 composer（`app/agent_view/interactions.rs:174-183`），Enter 提交后理由进 ACP 响应的 `meta.followup_message`（`app/dispatch/permissions.rs:213-222`）；若该 prompt 没有 `RejectOnce` 选项则改为回 `Cancelled`（`:201-211`）。

**作用域的收放**由 `←/→` 调节（`interactions.rs:105-160`）：无 MCP 时若当前行不是 scoped 行，先把光标跳到 scoped 行（`scoped_row_jump_idx`）；选中 `REJECT_ALWAYS_COMMAND_OPTION_ID` 时改的是 `bash_deny_selection_count`（每按一次 ±1 个词）；选中 `ALLOW_ALWAYS_COMMAND_OPTION_ID` 时走 `step_persisting_allow_scope`，**跳过所有「存了也白存」的作用域**（`permission_view.rs:243-257`）；MCP 场景下 `←` 选 Server 作用域、`→` 选 Tool 作用域。`e` 键进入自由 pattern 编辑器并预填当前命令文本（`interactions.rs:161-173`），编辑器下方预览行做实时校验、用三种颜色区分结局（`permission_view.rs:777-846`）：`✗ matches everything, won't be saved`（catch-all glob 被拒，`accent_error`）、`✓ matches this command · ⚠ very broad`（`accent_success` / `warning`）、`✗ won't match this command`（`accent_error`）。进入编辑器前会拒绝 catch-all pattern：`bash_glob_is_catchall(p)` 为真时 Enter 不提交（`interactions.rs:203-213`）。

#### 计划批准

```rust
pub enum PlanApprovalFocus { Preview, Prompt, Commenting }   // plan_approval_view.rs:34-39

/// A revision request is not an outcome: the review reopens and plan mode stays on.
pub enum PlanReviewOutcome { Approved, Abandoned }            // :47-52
```

这是本专题最有价值的一条语义约定：**「请求修改」不是终态**——它映射成 `cancelled` + feedback，界面重开、plan mode 保持开启；只有 `approved` / `abandoned` 会真正关闭 review。三个发送函数（`:229-243`）都通过 `oneshot::Sender`，而它被 `take()` 走，因此**二次发送返回 false**，测试 `test_double_send_returns_false`（`:417-422`）把这条钉死。界面是（`views/file_search/line_viewer.rs:1660-1760`）：

```text
  # Plan / ## Step 1 / Do something     ← 预览区，可 j/k/箭头滚动，可选中行范围加评论
  ────────────────────────────────
      a approve  |  s request changes  |  c comment ② ●  |  q quit plan  |  y copy plan
```

按钮居中排在底部分隔线下方；按钮宽度不足时**先丢 `y copy plan`**（`:1748-1750`）；有评论时 `a` 的标签变成 `approve w/ comments`（`:1685-1694`）。一个易被忽略的「拒绝路径」：当注释里出现完整 slash 命令（如 `/feedback <text>`）时，按 `a` 会被**拒绝**并给出 toast，而不是把命令当评论发出去——"Run the slash command in the notes with Enter, or clear it, before approving."（`app/agent_view/plan.rs:416-436`）。

#### 澄清提问（AskUserQuestion）

```text
┃
┃  [2/3] Which database should we use?      ← 多题时才有 [n/m] 前缀
┃  <问题描述，markdown 渲染，上限 5 行>        ← 超出显示 "... Ctrl-F to expand"
┃  <聚焦选项的 preview，上限 6 行>
┃
┃  1 ( ) PostgreSQL                          ← 单选 ( ) / (●)
┃  2 (●) SQLite
┃  3 [ ] Add a cache layer                   ← 多选：[ ] / [X]
┃  z ❯ Other: <自由文本>                       ← 粘在底部的自由文本行
```

计数器在 `app/agent_view/render.rs:541-542`；选项行前缀固定 6 列宽（`X (●) ` 或 `X [✓] `，`question_view.rs:1048-1056`）；自由文本行是 sticky 的、不参与滚动列表（`:1674-1700`），可用 `no_freeform` 整体关掉（`:189-192`）。**快捷键映射写得很克制，而且显示与接收不对称**：`option_shortcut_label(idx)` 显示时 `0..=8` 给 `1`–`9`、`9..=34` 给 `a`–`z`，但 `option_index_for_key()` **接收时只认 `1`–`9` 与 `a`–`f`**（`question_view.rs:1028-1046`），注释点明原因："Only a-f are mapped as shortcuts to avoid conflicts with navigation keys. (g=top, h=prev-question, j=down, k=up, l=next-question, n=next, s=skip)"。保留给导航的键：`g/G` 首/末、`h/l` 与 `[`/`]`/`←→` 切题、`j/k` 上下、`Ctrl+D/U` 半页、`PageUp/Down`、`z` 跳到自由文本、`y` 复制选项文本、`Tab` 在行间环绕（`interactions.rs:410-560`）。

#### MCP elicitation 与敏感操作同意

三阶段状态机 `ElicitationStage { Form(FormStage), UrlConsent(UrlConsentStage), UrlWaiting(UrlWaitingStage) }` 的设计意图写在文件头：`Form` 与 `UrlConsent` 自己持有 pending ACP responder，`UrlWaiting` 只在响应已发出后存在，所以**阶段本身编码了「这张卡是否还欠一个响应」**（`views/elicitation_view/state.rs:1-2`）；`UrlWaiting` 阶段只剩本地动作 `y Done`，不许再产生 ACP 响应——在类型层面避免双发。表单界面是标题 `MCP "postgres" requests your input` + server message + schema 解析错误 banner（`accent_error`，≤2 行）+ 字段名与值列对齐的 `host/port/ssl` 行（bool/enum 用 `Tab` 或 `Enter` 切换，滚动标记 `↑ more` / `↓ more` 画在分隔行上，`render.rs:231-243`）。动作行由 `actions()` 决定（`:141-154`）：`Form` → `y Accept / d Decline`；`UrlConsent` → `y Open URL / d Decline`；`UrlWaiting` → 只剩 `y Done`。动作行**钉死在底部**、正文在中间滚动（`:244-288`，注释 "Pin the action rows at the bottom; the body scrolls in between"）；高度策略是软上限屏高 33%、硬上限 80%，但必须给正文留至少 3 行（`:156-180`）。**默认焦点是 Accept**（`state.rs:359-367` 的 `action_focus: ElicitationActionFocus::Accept`）——这是本专题里默认值偏「放行」的第二处；URL 表单的焦点则直接落在 Actions、不进入 Fields（`:339-352`）。URL 表单会先做校验再决定渲染：`check_elicit_url` 失败时 `display.host` 为 `None`、`invalid` 有值，banner 用 `accent_error` 画出原因（`:333-352`）；server 名与 message 都过 `sanitize_server_text` 截断，但 `server_name_wire` 单独保留原始串供后续通知做相等比较（`:235-243, 34-35`）。快捷键栏对该卡给出 `Enter` accept/toggle、`d` decline、`Ctrl+C` cancel、`Tab` next field 与逐层的 Esc 提示（`app/agent_view/render.rs:307-313`）；`Esc` 在 `UrlWaiting` 阶段是 `EscStep::DismissElicitWaiting`（label `dismiss`），只关掉等待界面而不重发取消（`key_owner.rs:59-91, 247`）。

rewind 的 `RewindPhase` 明确区分「选择点」和「确认」，`Confirm` 是独立一屏，常量把选项写清楚（`views/rewind.rs:52-72, 86-88`）：`CANCEL_OFFER_OPTIONS = 2`、`/// Yes / Yes, and don't ask again / No.` `CONFIRM_OPTIONS = 3`。界面是 `Rewind conversation to "fix the auth bug"?` 加三行单选，`y` / `a` / `n` **单键直接触发、不需要先移动光标**，同时也支持 `j/k` + Enter（`:148-160`）；`render_radio_row` 的 key 列固定 4 列宽（`:600-604`）；`CancelOffer` 阶段（有回合在跑时先问要不要打断）同样用 `y` / `n` 单键（`:140-147`）。consent gate（法律告知）是一个**与安全门相反的失败策略**，`app/consent.rs:1-5` 开头写明 "Every failure path fails open: a client that cannot reach settings must stay usable. Validation is the trust boundary, so a notice that survives it is safe to paint."。它的防误触手段是**「没画出来就不许接受」**（`:83-95`）：`ConsentLegibility::{Illegible, Painted}` 只有 `Painted` 才能 `can_accept()`，渲染层据此**直接不画接受行**而不是画了再忽略——"Accept is refused while the body is unread, so the row is withheld rather than offered and ignored. Quit stays, or the screen would show no way out at all."（`views/welcome/consent.rs:73-79`）。按键也做了避让：接受键是 `a` 而非 `y`（"`y` accepts on the trust screen one step later, and Enter may be buffered before the notice paints"），没有 decline，`q` 退出、其余按键被吞（`app/consent.rs:507-541`）；**在正文 paint 之前到来的按键一律丢弃**，只放行 `Ctrl+C` / `Ctrl+D`（因为「这个屏是它们唯一的处理器」，`:529-537`）；链接编号用 `1`–`9`，`MAX_KEYED_LINKS = 9`（`:50-52`）。

#### 默认选中项如何决定

这是本专题**最需要谨慎借鉴**的一处，`resolve_initial_cursor` 的注释就是完整规范（`xai-grok-pager-render/src/appearance/permission_cursor.rs:161-185`）：`/// Sticky last-used, then configured default, then the YOLO row by identity, else index 0.` `/// A concrete target skips YOLO. A missing target kind degrades to one-shot allow, never to global always-approve.` 实现是三级 fallback——目标 `target` 由 `match last_used_permission() { AlwaysAllowAllSessions => load_default_selected_permission(), sticky => sticky }` 求出，先在 options 里找 `target.matches_kind(&o.kind) && !is_enable_always_approve_option(o)` 的行；找不到且 target 非 YOLO 时退而求第一个 `AllowOnce`（同样排除 YOLO 行）；再不行就取 YOLO 行的下标，否则 `unwrap_or(0)`。

1. **无粘性选择时，光标落在 YOLO 行**。`DefaultSelectedPermission::AlwaysAllowAllSessions` 是「未设置/无法识别」的兜底值，而配置项的默认值也是它——`src/settings/registry.rs:626-628` 与 `:1125-1138` 的测试注释直接写 "The first prompt's cursor lands on the 'Always allow on all sessions' row, picked explicitly in `enqueue_permission`"；测试 `resolve_unset_lands_on_yolo_row` 断言 index 1（YOLO 行所在位置）。`enqueue_permission` 的调用点是 `app/acp_handler/permissions.rs:146-148`。
2. **粘性选择只记录「允许类」**。记录点在 `app/dispatch/permissions.rs:132-151`，注释解释为什么排除拒绝：a rejection must not steer a later prompt's cursor onto a reject row——否则会把一个无关的后续 prompt 引到它的「always allow this command」行并扩大作用域；同时排除 YOLO 行与 `allow-edits-session`。
3. **降级方向是安全的**：具体目标行不存在时退到 `AllowOnce`，**绝不**升到全局 always-approve。测试 `unmatched_concrete_target_degrades_to_allow_once_not_yolo` 的注释："It never escalates to the global always-approve row, which is one Enter away from persisting always-approve mode."
4. 配置键是 `[ui].default_selected_permission`，四个枚举值 `always_allow_all_sessions` / `allow_once` / `allow_command_always` / `reject`；env 覆盖 `GROK_DEFAULT_SELECTED_PERMISSION`；设置弹窗改动走 `set_default_selected_permission` 做乐观更新（`permission_cursor.rs:28-62, 105-131`）。

其余卡片的默认值：计划批准 `PlanApprovalFocus::Preview`、提问卡 `QuestionSelection::Single(None)`（无预选）、rewind Confirm `active_idx: 0`（= Yes）、elicitation `action_focus: Accept`。**注意 rewind 与 elicitation 的默认项都在「放行」侧**，只有提问卡没有预选（`plan_approval_view.rs:114`；`rewind.rs:648`；`elicitation_view/state.rs:359-367`）。

#### 持久化选项与风险分级

| 行 | option_id | kind | 持久化范围 | 证据 |
| --- | --- | --- | --- | --- |
| `Yes, and don't ask again for anything (always-approve mode)` | `enable-always-approve` | `AllowOnce` | 写 `[ui] permission_mode = "always-approve"`，**跨会话** | `prompter.rs:22-40`；`app/dispatch/permissions.rs:166-178` |
| `Always allow: <cmd>` / `Never allow: <cmd>` | `allow-always-command` / `reject-always-command` | `AllowAlways` / `RejectAlways` | 项目级 `permission.toml` | `prompter.rs:587-606` |
| `Always allow: <server>__<tool>` | `allow-always-mcp` | `AllowAlways` | 项目级，可在 Tool/Server 两种作用域间切 | `prompter.rs:677-693`；`interactions.rs:110-119` |
| `Yes, always allow <domain> for this project` | `allow-always-domain` | `AllowAlways` | 项目级 | `prompter.rs:626-634` |
| `Yes, allow all edits during this session` | `allow-edits-session` | `AllowAlways` | **仅内存**，不落盘、不跨重启 | `prompter.rs:18-20` |
| rewind 的 `Yes, and don't ask again` | — | — | 关闭 `confirm_before_rewind` 设置 | `rewind.rs:84-90`；`views/settings_modal/state.rs:934` |

`AllowOnce` 的 kind 选择有讲究：YOLO 行故意用 `AllowOnce`，因为 "the pager's YOLO auto-approve drain, which answers with the first `AllowOnce`, picks this option. That is safe: the drain bypasses `dispatch_permission_select`, so picking it does not re-fire `set_yolo_mode(true)`"（`prompter.rs:31-33`）——**用 kind 复用让自动放行逻辑天然选中它，同时靠旁路调用点避免副作用重入**。另外「Always allow」行不是永远出现：`always_allow_row_is_effective(cmd)` 会用一份干净的 scratch 状态模拟「接受默认作用域后整条脚本能否无提示重放」，失败则该行不显示（`grants/bash_grants.rs:134-160`，注释 "the 'Always allow' row is not offered at all rather than saving a rule that would not work"）。

**风险分级在界面上的表达：颜色几乎不承担，风险靠「措辞 + 作用域 + 行是否出现」。** 通读 `permission_view.rs`（3037 行）后用到的 theme 字段只有 `accent_user`（键位/竖条）、`text_primary`、`text_secondary`、`gray`、`bg_light`、`bg_hover`、`bg_visual`、`selection_overlay()`、`hover_overlay()`，以及 pattern 预览行的 `accent_error` / `accent_success` / `warning`（`:802, 817, 824`）——**不存在按风险级别切换标题色或整卡底色的分支**。风险表达在三处：**措辞**（选项标签是自然语言句子而非动作名：`Yes, proceed` / `No, and tell Grok what to do differently` / `Never allow: git push`）；**固定顺序**（always-allow 打头、persistent deny 压尾，deny 绝不夹在安全选项之间，`prompter.rs:579-581`）；**危险命令被 pin 到完整命令**——`always_allow_scope_pinned` 让危险动词（`rm` / `chmod` / `chown` / `chgrp` / `chattr` / `pkill` / `kill` / `killall` / `git push`，`grants.rs:439-455`）与 exec 载体（解释器、包管理器、`sudo`/`ssh`）只能以完整命令持久化，判定条件是 `is_dangerous_command_words(words) || head_is_exec_vehicle(words) || normalized_command_head(words).as_deref() == Some("sed")`（`grants.rs:356-372`）。`minimum_always_allow_scope` 让 `←` 也**收不到比默认更窄**的位置（`:427-435`），注释写 "Narrowing `gh` broadens the grant"；危险命令清单同时是「记住的前缀不算数、必须重新问」的依据。

#### 超时与到期行为

**本地 TUI 的权限卡没有超时**：在 `xai-grok-pager/src/`、`xai-grok-workspace/src/permission/` 内检索 `timeout` 的唯一命中是 `AUTO_CLASSIFIER_TIMEOUT`（`permission/reasons.rs:10`，auto-mode 分类器用）——那是分类器超时，不是等用户。权限队列只在三种情况解阻塞：用户选择（`dispatch_permission_select`）、用户取消（`dispatch_permission_cancel`）、回合结束/取消时批量 drain（`drain_permission_queue`，`app/dispatch/permissions.rs:259-276`）。**不响应 = 无限期等待**，是 fail-closed 的一种朴素形式（不会自动放行）。

| 路径 | 超时 | 到期行为 | 证据 |
| --- | --- | --- | --- |
| 本地 TUI 权限卡 | 无 | 永久等待 | 上述检索；`dispatch/permissions.rs:236-276` |
| `AskUserQuestion` | **30 分钟**（可关） | 返回与「用户跳过」相同的文本，**不算工具失败** | 下方 |
| hub / 远程权限（chat-over-server） | 600 s backstop | **拒绝**（fail-closed） | 下方 |
| consent gate | 无 | 未接受则不进入会话；**所有失败路径 fail open** | `app/consent.rs:3-4` |
| headless（`grok -p`） | 不适用 | 非 yolo 一律回 `Cancelled`；yolo 取第一个 `AllowOnce`/`AllowAlways`，都没有也回 `Cancelled` | `headless.rs:1766-1786` |

提问卡的 `RESPONSE_TIMEOUT = Duration::from_secs(30 * 60)`、`DEFAULT_ASK_USER_QUESTION_TIMEOUT_ENABLED = true`，注释写明「全部问题共用一个计时器，到期返回与用户 dismiss 相同的 `format::unanswered_text`，不算工具失败」（`xai-grok-tools/src/implementations/grok_build/ask_user_question/mod.rs:47-59`）。env 覆盖 `GROK_ASK_USER_QUESTION_TIMEOUT_SECS`；`timeout_enabled = Some(false)` 表示「永远等待答案」，`timeout_secs = 0` 被显式拒绝并回落到默认值（`:61-64, 96-124`）。到期/取消的文案区分有无操作者（`ask_user_question/format.rs:21-35`），且措辞里有**显式防呆**——`CANCEL_TEXT = "User declined to answer the questions. Continue with the task using your best judgment, or ask different questions."`、`NO_OPERATOR_TEXT = "No user is available to answer questions in this non-interactive session. Continue with your best judgment; do not wait for clarification."`，超时文本本身在阻止模型原地重问。hub 路径把超时与其它错误都导向拒绝：`Err(e) => { tracing::error!(error = %e, "hub permission request failed; rejecting"); PromptOutcome::Error(...) }`（`xai-grok-workspace/src/permission/hub_permission.rs:271-275`，`:88-101` 只把超时单独计入 `grok_workspace_permission_timeout_total` 指标）。600 s 来自 `xai-computer-hub-sdk/src/server.rs:1365-1372` 的 `HOOK_REQUEST_BACKSTOP_TIMEOUT`，注释说明它 "deliberately long: the hook is normally released by the real reply or requester teardown; this only bounds a request whose reply is lost (e.g. a dead connection)"。**一条强相关推断（非直接证据）**：`is_timeout_err` 只做子串匹配 `"timed out"`（`hub_permission.rs:38-40`），其测试用 "request timed out after 600s"，与上述常量吻合，但两者在代码中没有直接引用关系。

#### 键盘流与防误触

| 场景 | 最少击键 | 键 |
| --- | --- | --- |
| 权限卡（接受当前光标项 / 接受第 n 项） | **1** | `Enter` / `1`–`9`（直接选中并派发，不经过光标移动） |
| 权限卡（拒绝并说明理由 / 缩小作用域后接受 / 手写 glob / 开 always-approve） | 2、3+、2+、1 | 敲字符进 FollowupInput → `Enter` / `←` × n → `Enter` / `e` → 编辑 → `Enter` / `Ctrl+O`（全局切换，不必选中第 1 行） |
| 计划批准 / 请求修改 | 1 / 2+ | `a`（**前提：prompt 输入框为空**）/ `s` → 输入 → `Enter` |
| 提问卡（选第 n 项 / 跳过整组） | **1** | `1`–`9` / `a`–`f`（最后一题直接提交整组）/ `Shift+X` |
| elicitation（Accept / Decline） | 1 | `y` 或 `Enter` / `d` |
| rewind 确认 / consent 接受 | 1 | `y` / `a` / `n`；`a`（仅在正文已绘制后） |

数字直选的三处实现：权限卡 `KeyCode::Char(ch @ '1'..='9')`（`interactions.rs:88-96`）、提问卡走 `option_index_for_key`（`:520-545`）、`CancelTurnChoice` 用 `1`–`4`（`:255-262`）。`Ctrl+O` 在任何 `PermissionFocus::Options` 状态下都可用（`:99-101`）并出现在快捷键栏（`app/agent_view/render.rs:249`）。`a` approve 的前置条件值得注意——**输入框非空时 `a` 不生效**，会落进 composer 变成普通字符：`!is_commenting && key.code == KeyCode::Char('a') && key.modifiers.is_empty() && self.prompt.text_without_image_chips().trim().is_empty() && !self.prompt.file_search_visible()`（`app/agent_view/plan.rs:789-796`），这是一条隐式的「防止带着半截评论误批」保护。

防误触机制按强度从强到弱：**① Esc 不是拒绝**（最强的结构性保护）——权限卡的 `Esc` 映射到 `EscStep::ParkFocus`、快捷键栏标签是 `scrollback`，交出键盘焦点而卡片继续挂着（`FollowupInput → LeaveTextInput` label `back`、`PatternEdit → DiscardPatternEdit` label `cancel`、`Options → ParkFocus`，`app/agent_view/key_owner.rs:167-173`）；提问卡的 `Esc` 也不交答案（无选择时 ParkFocus，有选择时先 `ClearSelection` label `unselect`，`:177-192`）；取消权限**只有 `Ctrl+C` 一个入口**（`interactions.rs:28-29` 注释 "`Esc` steps back down through them and `Ctrl+C` is the only cancel"）；每个卡片的 `Esc` 语义都是**栈式**的，`EscStep` 有 8 个变体、逐层退出而不是一步取消（`key_owner.rs:59-91`）。**② 双击确认（`PendingAction`）**——第一次按下只是 arm，快捷键栏被整条替换为 "press again to {label}"（`views/shortcuts_bar.rs:1-3`；`app/app_view.rs:459-478`），`label: None` 可「静默 arm」（`:425-434`）。**③ 不可接受的行根本不画**——consent gate 的接受行在正文不可读时直接不渲染（`views/welcome/consent.rs:73-79`）；作用域行不可用时 `has_adjustable_scope()` 为 false，提示行与 `←/→` 处理一起消失，避免出现「按键没反应」的死键（`permission_view.rs:178-196`；`interactions.rs:105`）。**④ 危险项的默认作用域被 pin 死**（`grants.rs:356-372, 427-435`；`bash_glob_is_catchall` 让 catch-all glob 无法被保存，`permission_view.rs:798-806`）。**⑤ 失焦即「无选择」**（撤背景带 + 降饱和）。**⑥ 其它**：单选与自由文本互斥（`interactions.rs:466-472, 510-514`）；多客户端下另一个客户端已解决同一交互时本地卡会被撤回 `dismiss_resolved_interaction(tool_call_id)`（`:1060-1072`，"Retract an interaction modal (permission, question, or plan approval) that another connected client already resolved."）；切到下一个权限请求时焦点重置为 `Options` 并清掉上一个请求残留的 followup 文本、pattern 编辑缓冲无条件丢弃（`app/dispatch/permissions.rs:283-294`）。
