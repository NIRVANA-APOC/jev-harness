# 专题 16：Grok Build 的键盘模型与动作注册表

> 专题：键盘模型与动作注册表｜调研日期：2026-09-22｜证据范围：`crates/codegen/xai-grok-pager/src/actions/`（`mod.rs` 1029 行、`defaults.rs` 1207 行）、`src/app/actions.rs`、`src/app/event_loop.rs`、`src/app/reader_thread.rs`、`src/app/app_view.rs`、`src/app/mode_switch.rs`、`src/app/agent_view/{mod,input,prompt,panes}.rs`、`src/app/dispatch/{mod,router,modes}.rs`、`src/app/effects/mod.rs`、`src/views/{shortcuts_help.rs,shortcuts_bar.rs,dashboard/state.rs}`、`src/settings/defs.rs`、`crates/codegen/xai-grok-pager-render/src/input/{key.rs,keyboard_normalizer.rs,terminal_support.rs}`、`.../src/terminal/{mod.rs,kitty_keyboard.rs}`、`docs/user-guide/03-keyboard-shortcuts.md`、`tests/registered_features_are_documented.rs`

## 1. 一句话总览

键盘输入只有一条路径：**crossterm 事件 → 逐层精确匹配 `ActionRegistry` → `ActionId` → `Action` → 同步 `dispatch` 返回 `Effect` → 异步执行**。所有键位集中定义在 `actions/defaults.rs:56` 的 `default_actions()` 一个函数里，手册注释写明理由是 "All key bindings are defined here, not scattered across event handlers."（`actions/defaults.rs:1-3`）。

```
┌─ 表现层（渲染期间） ─────────────────────────────────────────────┐
│ 快捷键栏 shortcuts_bar  ← registry.hints(contexts)  8 个 hint     │
│ 速查表  shortcuts_help  ← registry.all() + Category 分组         │
└──────────────────────────────────────────────────────────────────┘
┌─ 解析层（事件驱动） ─────────────────────────────────────────────┐
│ registry.lookup(key, When)     精确上下文匹配，逐层冒泡            │
│ registry.lookup_with_mode(...) 额外叠加 vim_mode 裸字母抑制        │
└──────────────────────────────────────────────────────────────────┘
┌─ 数据层 ─────────────────────────────────────────────────────────┤
│ ActionRegistry { actions: Vec<ActionDef> }                        │
│ ActionDef { id: ActionId, default_key, alt_keys, category,        │
│             context: When, hint_priority, requires_confirmation }  │
└──────────────────────────────────────────────────────────────────┘
```

## 2. 动作抽象

### 2.1 命名：编译期 enum，没有字符串名

`ActionId` 是纯 Rust enum，`#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]`，注释即设计声明："Unique action identifier. **Compile-time checked, no strings.**"（`actions/mod.rs:34-36`）。共 **76 个变体**（`actions/mod.rs:37-145`），按注释分组：Prompt / Navigation / View / Agent / Focus / Block content / Link navigation / Panes / Prompt / Scrollback / Debug / App / Settings / Agent Dashboard。

**`ActionId` 不实现 `FromStr`、`Display` 或 serde**：全仓库搜索 `impl FromStr for ActionId` / `parse_action` 零命中。唯一的字符串化出口有两处，都是遥测用途：

- `When::telemetry_name()` 给出上下文标签 `"always"` / `"prompt_focused"` / …（`actions/mod.rs:169-182`）。
- `shortcut_used_action_label(id)` 是**两元素白名单**：`InterjectPrompt → "interject_prompt"`、`OpenExtensions → "open_extensions"`，其余返回 `None` 即不发射（`actions/mod.rs:487-493`）。测试 `shortcut_used_allowlist_is_ctrl_l_actions_only` 钉住了这个白名单（`actions/mod.rs:518-531`）。

### 2.2 注册表结构

```rust
pub struct ActionRegistry { actions: Vec<ActionDef> }   // actions/mod.rs:236-238
```

`ActionDef` 的 10 个字段（`actions/mod.rs:197-221`）承担了全部元数据：`id` / `label`（快捷键栏短标签）/ `description`（面板长描述）/ `long_help`（速查表手册式正文，`None` 时回落到 `description`）/ `default_key` / `alt_keys: Vec<KeyShortcut>` / `category` / `context: When` / `hint_priority: Option<u8>` / `hint_key_display: Option<&'static str>` / `requires_confirmation: bool`。

`hint_priority` 的语义在注释里写得很明确："`None` means don't show; `Some(0)` is the highest priority"（`actions/mod.rs:213`）。**全表只有 8 个动作设了它**：SelectNext `Some(0)`、NextTurn `Some(1)`、ToggleFold `Some(3)`、ExpandAllThinking `Some(3)`、GotoTop `Some(4)`、CommandPalette `Some(5)`、Quit `Some(10)`、VoiceToggle `Some(11)`（`actions/defaults.rs:85,111,163,298,328,680,782,797`）。这 8 个决定了底部快捷键栏显示什么。

三类查询接口（`actions/mod.rs:1-6` 声明了三个消费者，但见 §9 的出入）：

| 方法 | 语义 | 定义 |
| --- | --- | --- |
| `lookup(&KeyEvent, When)` | **精确**上下文匹配，纯读取不发遥测 | `actions/mod.rs:277-290` |
| `lookup_with_mode(&KeyEvent, When, vim_mode)` | 在上一者之上叠加裸字母抑制 | `actions/mod.rs:387-418` |
| `matches_id(ActionId, &KeyEvent)` | 忽略 `When`，只看该动作的键集合 | `actions/mod.rs:294-299` |
| `hints(&[When])` / `hint_items(&[When])` | 取 `hint_priority` 非空者并按优先级排序 | `actions/mod.rs:427-442` |
| `all()` | 全部 `ActionDef` 切片 | `actions/mod.rs:466-469` |
| `key_for(id)` / `key_for_mode(id, vim)` | 取主键 / 取 vim 感知的有效键 | `actions/mod.rs:444-464` |

`matches_id` 的用途在注释中说明为跨 pane 的共享和弦，例如队列面板的强制插话复用 `InterjectPrompt` 的键集合（`actions/mod.rs:292-293`）。

### 2.3 规模数据（实测）

以 `ActionRegistry::defaults_for(ScreenMode::Fullscreen)` 为准：

| 维度 | 数量 | 证据 |
| --- | --- | --- |
| `ActionId` 变体 | 76 | `actions/mod.rs:37-145` |
| 实际注册的 `ActionDef` | **70** | 见下方分解 |
| 上下文分布 | Scrollback 28 / DashboardFocused 14 / AgentScreen 13 / PromptFocused 7 / Always 4 / DashboardOverlay 4 | `actions/defaults.rs:75-855`（主 vec 50 个，28+12+7+3）与 `878-1192`（19 个，14+4+1） |
| 需要双击确认 | 3（`NewSession`、`Quit`、`DashboardOverlayStop`） | `actions/defaults.rs:761,784,1187` |
| 键位的终端分支点 | 6（Quit、HalfPageDown、InterjectPrompt、OpenExtensions、ToggleQueue、OpenDashboard） | `actions/defaults.rs:224,554,595,629,770,883` |
| 编译期开关 `ScreenMode` | 影响 `mode_ctrl_g_action` 与 Minimal 裁剪 | `actions/defaults.rs:17-51,1197-1204` |
| 运行期开关 `mouse_reporting_toggle` | +1（`ToggleMouseCapture`） | `actions/defaults.rs:857-873` |

分解：主 vec `50` 个 + `mode_ctrl_g_action` **二选一**（非 Minimal 为 `ToggleTasks`，Minimal 为 `EditPromptExternal`，`actions/defaults.rs:17-51`）+ Dashboard 扩展块 `19` 个 = 70。

Minimal 模式的裁剪是白名单式 `retain`：移除全部 `ScrollbackFocused | DashboardFocused | DashboardOverlay` 上下文，另加 `OpenDashboard` 与 `FocusScrollback` 两个 id（`actions/defaults.rs:1197-1204`）。测试 `minimal_registry_omits_unsupported_surfaces_and_dashboard_entry` 逐个断言（`actions/mod.rs:671-746`）。

### 2.4 键位序列化与配置覆盖：**没有实现**

这是本专题最重要的一条负面结论，且**源码注释与实现相互矛盾**：

- **实现事实**：`ActionRegistry` 的构造入口只有 `defaults()` / `defaults_for(mode)` / `defaults_with_config(mouse_enabled)` 三条，最终都落到 `defaults::default_actions(screen_mode, mouse_reporting_toggle_enabled)`（`actions/mod.rs:240-272`）。全仓库 `ActionRegistry::new(...)` 的非测试调用点为零。运行时唯一重建 registry 的地方是 `mode_switch.rs:33-36` 的 `reseed_screen_mode()`。
- **手册事实**：用户手册第 3 行直说 "Bindings are built in and **cannot currently be remapped**."（`docs/user-guide/03-keyboard-shortcuts.md:3`）。
- **矛盾注释**：`actions/defaults.rs:901-902` 声称 dashboard 快捷键 "users can rebind any of them through `~/.grok/config.toml`"，但代码中不存在任何读取键位配置的路径（`ActionId` 无字符串解析、无 serde，`settings/defs.rs` 的 51 个 `SettingMeta` 里没有任何键位映射项）。

唯一可配置的「键盘相关」设置是 5 个行为开关，而非键位映射：`simple_mode`（`settings/defs.rs:639-660`）、`vim_mode`（`667-687`）、`voice_keybind_enabled`（`1345-1365`）、`voice_capture_mode`（`1373` 起）、`mouse_reporting_toggle`（`app/mod.rs:182-187`）。

## 3. 上下文模型 `When` 与分层冒泡

`When` 是 7 值枚举（`actions/mod.rs:149-167`）：

| 变体 | 语义 | 默认键位数 |
| --- | --- | --- |
| `Always` | 全局；所有视图之后兜底检查 | 4 |
| `PromptFocused` | 仅输入框聚焦 | 7 |
| `ScrollbackFocused` | 仅 scrollback 聚焦 | 28（+1 config） |
| `AgentScreen` | agent 层；pane 路由之后、global 之前 | 13 |
| `WelcomeScreen` | 仅欢迎页 | **0 —— 未找到证据**：枚举变体存在且 `telemetry_name()` 有分支，但 `defaults.rs` 中 `context: When::WelcomeScreen` 零出现 |
| `DashboardFocused` | Agent Dashboard 列表聚焦 | 14 |
| `DashboardOverlay` | Dashboard 内嵌会话浮层 | 4 |

**精确匹配是刻意设计**：`lookup()` 用 `def.context != context` 直接 `continue`，冒泡顺序写死在调用方代码里而不是藏在 `context_matches` 里（`actions/mod.rs:8-15, 277-290`）。测试 `exact_context_matching` 钉住了两个方向：同一 Ctrl+D 在 `ScrollbackFocused` 解析为 `HalfPageDown`（`defaults.rs:221-236`），在 `Always` 解析为 `Quit`（`defaults.rs:766-788`），而两者互不可见（`actions/mod.rs:602-620`）。

```
按键事件
   │
   ├─► 模态 / 浮层 / 下拉 / 语音 / 搜索 / 选区 先抢 Esc 与自身键
   │
   ├─► 【第 1 层 pane 级】精确匹配
   │      prompt 聚焦:    lookup(key, PromptFocused)   → 7 个输入框动作
   │                      未命中 → Tab 结构化 FocusScrollback
   │      scrollback 聚焦: lookup_with_mode(key, ScrollbackFocused, vim)
   │
   ├─► 【第 2 层 agent 级】
   │      lookup(key, AgentScreen) → CancelTurn / ToggleYolo / ModelPicker …
   │
   ├─► 【第 3 层 Esc 策略】仅 Prompt / Scrollback
   │      running → 提示 Ctrl+C 文案；cancelling → 吞掉
   │      idle 有草稿（仅 prompt） → ArmPending ClearPrompt (双按)
   │      idle 空 + 有消息 → ArmPending RewindShowPicker (双按)
   │      其余 → Changed（吞掉，不回传给 global）
   │
   └─► 【第 4 层 global】handle_unconsumed_input
          lookup(key, Always) → Quit / NewSession / OpenDashboard / VoiceToggle
```

这张图是 `app/agent_view/mod.rs:9-47` 模块文档的转写，其中第 3 层的每个分支都能在 `app/agent_view/prompt.rs:747-830` 的 `try_handle_esc_policy()` 里逐行对上。文档还特别声明："No Esc-policy branch depends on `[ui].vim_mode` … or `[ui].simple_mode` … Tab remains leave-prompt in both modes."（`app/agent_view/mod.rs:49-51`）。

## 4. 完整快捷键总表

以下各表按 `context` 分组，键位与 `actions/defaults.rs` 逐条对应。终端分支用 `→` 标注。除注明外均为默认（非 VS Code family、非 Apple Terminal）主机。

### 4.1 全局 `When::Always`（4）

| 键位 | 备用键 | 动作 | 确认 | 出处 |
| --- | --- | --- | --- | --- |
| `Ctrl+N` | | `NewSession` 新建会话 | 是（1000 ms） | `defaults.rs:751-765` |
| `Ctrl+Q` | `Ctrl+D` | `Quit` 退出；VS Code family 下 `Ctrl+D` 为唯一主键 | 是（1000 ms） | `defaults.rs:766-788` |
| `Ctrl+Space` | `F8` | `VoiceToggle` 语音采集 | 否 | `defaults.rs:669-686` |
| `Ctrl+\` | `Ctrl+4`（本地 macOS VS Code family 无此 alt） | `OpenDashboard` | 否 | `defaults.rs:878-899` |

### 4.2 `When::AgentScreen`（13）

| 键位 | 备用键 | 动作 | 出处 |
| --- | --- | --- | --- |
| `Ctrl+C` | | `CancelTurn`（草稿非空时先清空） | `defaults.rs:496-510` |
| `Ctrl+G` | | `ToggleTasks` 任务面板；Minimal 下改为 `EditPromptExternal` | `defaults.rs:17-51` |
| `Ctrl+T` | | `ToggleTodos` | `defaults.rs:532-546` |
| `Ctrl+;` | `Ctrl+'`；本地 macOS VS Code family：`Ctrl+4` 为主、`;` `'` 为 alt | `ToggleQueue` | `defaults.rs:547-574` |
| `Ctrl+R` | | `OpenSessions`（＝`/resume`） | `defaults.rs:575-589` |
| `Ctrl+L` | | `OpenExtensions`；VS Code family 下改为 `KeyCode::Null`（不绑定） | `defaults.rs:590-609` |
| `Ctrl+B` | | `SendToBackground` | `defaults.rs:610-622` |
| `Ctrl+O` | | `ToggleYolo` 总是批准 | `defaults.rs:736-750` |
| `Ctrl+P` | `?`（Shift+/） | `CommandPalette` | `defaults.rs:789-803` |
| `Ctrl+.` | `Ctrl+X`（两者都注册，`ctrl_dot_unreliable()` 决定谁是主键） | `ShortcutsHelp` | `defaults.rs:804-826` |
| `Ctrl+M` | | `ModelPicker`（输入框聚焦时该和弦归 `ToggleMultiline`） | `defaults.rs:827-841` |
| `F2` | `Ctrl+,` / `Cmd+,` | `OpenSettings` | `defaults.rs:842-854` |
| （无键） | | `EnableVoiceMode`：`default_key = Null`，由事件循环的语音和弦直接派发，不走注册表 | `defaults.rs:656-668` |

### 4.3 `When::ScrollbackFocused`（28 + 1 config）

| 键位 | 备用键 | 动作 | 出处 |
| --- | --- | --- | --- |
| `j` | `Down` | `SelectNext`（`hint_key_display="j/k"`，优先级 0） | `defaults.rs:77-89` |
| `k` | `Up` | `SelectPrev` | `defaults.rs:90-102` |
| `L` | `Shift+Right` | `NextTurn`（优先级 1） | `defaults.rs:103-115` |
| `H` | `Shift+Left` | `PrevTurn` | `defaults.rs:116-128` |
| `J` | | `NextResponse`（视口顶对齐下一轮） | `defaults.rs:129-141` |
| `K` | | `PrevResponse` | `defaults.rs:142-154` |
| `g` | | `GotoTop`（优先级 4） | `defaults.rs:155-167` |
| `G` | | `GotoBottom` | `defaults.rs:168-180` |
| `Ctrl+K` | | `ScrollUp` 逐行 | `defaults.rs:181-193` |
| `Ctrl+J` | | `ScrollDown` 逐行 | `defaults.rs:194-206` |
| `Ctrl+U` | | `HalfPageUp` | `defaults.rs:207-219` |
| `Ctrl+D` | VS Code family 改为裸 `D` | `HalfPageDown` | `defaults.rs:220-236` |
| `PageUp` / `PageDown` | | `PageUp` / `PageDown` | `defaults.rs:237-262` |
| `h` | `Left` | `Collapse` | `defaults.rs:264-276` |
| `l` | `Right` | `Expand` | `defaults.rs:277-289` |
| `e` | | `ToggleFold`（优先级 3） | `defaults.rs:290-304` |
| `E` | | `ToggleExpandAll` | `defaults.rs:305-319` |
| `Ctrl+E` | | `ExpandAllThinking`（优先级 3） | `defaults.rs:320-334` |
| `r` | | `ToggleRaw` | `defaults.rs:335-349` |
| `y` | | `CopyBlockContent` | `defaults.rs:350-365` |
| `Y` | | `CopyBlockMeta` | `defaults.rs:366-380` |
| `Enter` | `Ctrl+F` | `OpenBlockViewer` | `defaults.rs:381-395` |
| `o` / `O` | | `OpenNextLink` / `OpenPrevLink` | `defaults.rs:396-422` |
| `x` | | `KillBgTask` | `defaults.rs:439-453` |
| `Tab` | `i`、`Space` | `FocusPrompt` | `defaults.rs:468-480` |
| （无键） | | `Rewind`：`default_key = Null`，由 `Esc Esc` 双按策略与 `/rewind` 进入 | `defaults.rs:424-438` |
| `Ctrl+R` | | `ToggleMouseCapture`：**仅当 `[ui] mouse_reporting_toggle` 开启**才注册 | `defaults.rs:857-873` |

### 4.4 `When::PromptFocused`（7）

| 键位 | 备用键 | 动作 | 出处 |
| --- | --- | --- | --- |
| `Enter` | | `SendPrompt`（运行中为入队；草稿已空再按则直发队首） | `defaults.rs:454-467` |
| `Tab` | | `FocusScrollback` | `defaults.rs:481-495` |
| `Shift+Tab` | 另有 `BackTab`、`BackTab+SHIFT` 两种编码作为 alt | `CycleMode`（Normal → Plan → Always-Approve） | `defaults.rs:511-529` |
| `Ctrl+Enter` | `Ctrl+I`；Apple Terminal：`Ctrl+O` 为主、`Ctrl+Enter`/`Ctrl+I` 为 alt；VS Code family：`Ctrl+L` 且**无 alt** | `InterjectPrompt` | `defaults.rs:624-654` |
| `Ctrl+M` | | `ToggleMultiline` | `defaults.rs:689-703` |
| `Ctrl+S` | `Alt+S` | `StashPrompt` 暂存 / 取回草稿 | `defaults.rs:704-719` |
| `!` | | `BashMode`（空输入框打 `!` 进 shell 模式） | `defaults.rs:720-734` |

### 4.5 `When::DashboardFocused`（14）与 `When::DashboardOverlay`（4）

| 键位 | 动作 | 出处 |
| --- | --- | --- |
| `↓`/`↑`，`j`/`k` | `DashboardSelectNext` / `DashboardSelectPrev` | `defaults.rs:903-928` |
| `Ctrl+T` | `DashboardTogglePin` | `defaults.rs:929-943` |
| `Ctrl+R` | `DashboardBeginRename` | `defaults.rs:944-956` |
| `Ctrl+X` | `DashboardStop`（运行中取消，空闲后 2 s 内再按永久删除） | `defaults.rs:957-971` |
| `Shift+Tab`（3 种编码） | `DashboardCycleMode` | `defaults.rs:972-991` |
| `Ctrl+G` | `DashboardToggleGrouping` | `defaults.rs:992-1009` |
| `Shift+↑` / `Shift+↓` | `DashboardReorderUp` / `DashboardReorderDown` | `defaults.rs:1010-1035` |
| `Ctrl+.` / `?` | `DashboardShortcutsHelp` | `defaults.rs:1036-1058` |
| `Esc` | `DashboardExit`（Esc 级联：先关 peek → 清过滤 → …） | `defaults.rs:1062-1076` |
| `Ctrl+O` | `DashboardToggleAutoApprove` | `defaults.rs:1079-1093` |
| `Ctrl+L` | `DashboardOpenLocationPicker` | `defaults.rs:1096-1110` |
| `Ctrl+W` | `DashboardToggleWorktree` | `defaults.rs:1113-1127` |
| `Ctrl+\` | `DashboardOverlayExit`（浮层退出） | `defaults.rs:1131-1148` |
| `Ctrl+[` / `Ctrl+]` | `DashboardOverlayPrev` / `DashboardOverlayNext` | `defaults.rs:1149-1174` |
| `Ctrl+X` | `DashboardOverlayStop`（**需双击**） | `defaults.rs:1177-1191` |

### 4.6 注册表之外：三处键位孤岛

注册表并非唯一事实来源，有三类键位**完全绕过** `ActionRegistry`：

1. **欢迎页**：`When::WelcomeScreen` 零注册。`app/app_view.rs:3175` 的 `handle_welcome_input()` 用内联 `key!()` 字面匹配处理，例如 `Ctrl+W`（`:3696`，且需 `cwd_has_git_ancestor`）、`Ctrl+I`（`:3708`）、`Ctrl+Shift+I`（`:3711`）。有意思的是 `Ctrl+R` 例外——`:3699` 走 `ctx.registry.matches_id(ActionId::OpenSessions, key)` 复用注册表绑定。
2. **scrollback 搜索**：`/` 是 vim 模式下的内联处理器（`app/agent_view/panes.rs:111-120`），simple 模式改由 `/find` 斜杠命令；两者都以伪行形式手工写进速查表（`views/shortcuts_help.rs:230-250`）。
3. **Prompt 的 `↑` 历史、粘贴、撤销/重做**：均为 textarea 级别的和弦，不在注册表（`views/shortcuts_help.rs:260-310` 的 `push_pseudo` 分支），`long_help` 常量手工维护（`views/shortcuts_help.rs:80-125`）。

另有一处硬编码提醒：thinking 块的折叠提示文案 `"ctrl+e to expand"` 是常量，源码 TODO 写明因为它不走注册表，重映射后无法正确宣传（`scrollback/blocks/thinking.rs:16-17`）；Minimal 模式还有 `minimal_key_intercept()` 直接字面匹配 `Ctrl+T` / `Ctrl+E` / `Ctrl+O`（`app/app_view.rs:4259-4290`）。

## 5. 多键序列与 leader key

**结论：本项目没有多键序列机制，也没有 leader key。** 论据三条：

1. `ActionRegistry::lookup` 的签名是 `(&KeyEvent, When) -> Option<ActionId>`，单事件、单次匹配、首次命中即返回，**没有任何前缀状态或 pending 缓冲**（`actions/mod.rs:277-290`）。全仓库搜索 `LeaderKey` / `leader_key` / `leader key` 零命中。
2. 唯一带「两段式」语义的机制是**双击确认**（`PendingAction`，见 §6），它是「同一个键再按一次」而非「按键序列」。
3. **`app/leader_cluster/` 与键盘无关**，这是本专题最容易误读的目录名。它是进程级 leader IPC 的进程内测试装置：`PagerLeaderCluster` 起一个 unix socket 上的 leader server 加真实 `MvpAgent`，用 `LeaderClient::connect` 接多个完整 `AppView` 客户端，验证重连与代际切换（`app/leader_cluster/mod.rs:1-24, 256-402`）。「leader」指的是**主进程/leader server**，不是 vim 的 `<Leader>` 键。该模块的文档还给出了它刻意 `#[ignore]` 的理由（进程级环境变量污染）与手动运行命令（`app/leader_cluster/mod.rs:12-24`）。

## 6. 双击确认与超时策略

超时窗口有两个常量，语义不同（`app/app_view.rs:474-476`）：

```rust
pub const TTL: Duration = Duration::from_millis(1000);                  // 确认型动作
pub const ESC_DOUBLE_PRESS_TTL: Duration = Duration::from_millis(800);  // Esc 清空 / 回退
```

消费逻辑在 `handle_input_at_with_paste_provenance` 的最前面：pending 若未过期且 `pending.shortcut.matches(key)`，取出动作直接返回 `InputOutcome::Action`；否则清空 pending（`app/app_view.rs:2320-2360`）。这里有个细节修正：若 pending 是 `ClearPrompt` / `RewindShowPicker` 而此刻回合已经跑起来，则视为过期不触发（`app/app_view.rs:2337-2348`）。

三档超时值：

| 窗口 | 值 | 用途 | 出处 |
| --- | --- | --- | --- |
| `PendingAction::TTL` | 1000 ms | `NewSession` / `Quit` 双击（手册第 341 行亦如此描述） | `app/app_view.rs:474`；`docs/user-guide/03-keyboard-shortcuts.md:341` |
| `PendingAction::ESC_DOUBLE_PRESS_TTL` | 800 ms | 空闲 `Esc Esc` 清空草稿 / 打开回退选择器 | `app/app_view.rs:476, 500-514` |
| Dashboard 删除确认 | 2 s（`CONFIRM_WINDOW`），由 `PendingAction::with_ttl` 传入 | `Ctrl+X` 二次删除会话 | `app/app_view.rs:483-489`；`defaults.rs:968-970` |
| `ESC_CANCEL_REWIND_GRACE` | 1000 ms | 回合中途按 Esc 后，把「回退 arm」推迟，防止连按 Esc 跨过回合结束时误开回退选择器 | `app/agent_view/prompt.rs:739-746, 837-843` |

`Esc` 双按窗口有一个仅测试用的环境变量覆盖 `GROK_ESC_DOUBLE_PRESS_MS`，带 60 000 ms 上限（`app/app_view.rs:500-514`）；注释说明覆盖存在的原因是 pty_e2e 分片的渲染往返可能超过 800 ms 默认值。

## 7. 分发链路：从 crossterm 事件到 handler

### 7.1 调用链

```
 crossterm::event::poll(POLL_TIMEOUT) / read()          app/reader_thread.rs:67-69
        │  专属 reader 线程，pause 标志在 poll 间隙被观测
        ▼  mpsc
 TimedInputEvent { event, arrived_at }                  app/event_loop.rs:40-50
        │
        ├─ 启动期 type-ahead 过滤 / 粘贴合并 / 语音和弦抢先
        │    is_typeahead_event, filter_startup_typeahead   app/event_loop.rs:55-90
        │    merge_paste_fragments, coalesce_rapid_keys     app/event_loop.rs:3710-3740
        │    is_voice_chord → dispatch(Action::VoiceToggle) app/event_loop.rs:3554-3565
        ▼
 AppView::handle_input_at_with_paste_provenance(&Event)  app/app_view.rs:2320
        │  (公开入口 handle_input() 只是它的一行包装: app/app_view.rs:2317-2319)
        │
        ├─ 0. keyboard_normalizer.rescue(ev)  ← 修饰键救援，见 §8   render/src/input/keyboard_normalizer.rs:55
        ├─ 1. PendingAction 消费（双击确认）                      app/app_view.rs:2337
        ├─ 2. Tutorial / 鼠标滚动 / 模态 / 各浮层抢占
        ├─ 3. 按 active_view 分派:
        │      Welcome        → handle_welcome_input()            app/app_view.rs:3175
        │      Agent(id)      → DashboardOverlay 拦截 → AgentView::handle_input()
        │      AgentDashboard → dashboard.state::handle_key()      views/dashboard/state.rs:3050
        ▼
 AgentView::handle_input(ev, registry)                   app/agent_view/input.rs:235
        └─ handle_input_inner(...)                       app/agent_view/input.rs:329
             ├─ 模态 / pane 分发
             ├─ Prompt    → AgentView::handle_prompt_key()          app/agent_view/prompt.rs:91
             │                ├─ registry.lookup(key, PromptFocused)     prompt.rs:499
             │                ├─ registry.lookup(key, AgentScreen)       prompt.rs:636
             │                │   （Ctrl+C 在有草稿时故意跳过此提升，落到 widget 的清空路径）
             │                └─ try_handle_esc_policy()                 prompt.rs:747
             ├─ Scrollback → handle_scrollback_key()                    app/agent_view/panes.rs:20
             │                └─ registry.lookup_with_mode(key, ScrollbackFocused, vim_mode)  panes.rs:76
             └─ try_handle_agent_action()   → registry.lookup(key, AgentScreen)
                 └─ handle_agent_action_with_registry(id, registry)     app/agent_view/input.rs:1221
        ▼  返回 InputOutcome { Action | ActionThenForward | ActionPair | ArmPending | Changed | Unchanged }
                                                            app/app_view.rs:422-445
        ▼
 handle_unconsumed_input() → registry.lookup(key, When::Always) → handle_global_action()
                                                            app/app_view.rs:2952-3010
        ▼
 event_loop 的 InputOutcome::Action 分支                      app/event_loop.rs:3341-3355
        │
        ├─ dispatch::dispatch(action, &mut app) -> Vec<Effect>   app/dispatch/router.rs:156
        │     dispatch_inner(): 纯同步、确定性；不碰终端/网络/文件系统
        │                        app/dispatch/mod.rs:1-10
        └─ process_effects() → effects::execute(eff, …)          app/effects/mod.rs:120
               异步副作用：spawn 任务、ACP 发送、写盘
```

`dispatch_then_forward()` 是唯一需要「同事件二次处理」的特例：先 dispatch 再让事件重新走一遍（现在已变化的）视图，服务于欢迎页建会话与 `CycleMode` 共享 `session/new` 标志的场景（`app/event_loop.rs:3907-3931`；`app/app_view.rs:425-428`）。

### 7.2 两阶段架构的边界

模块文档把不变量写得很清楚：dispatch 模块 "never touches the terminal, network, or filesystem"，"All mutations are synchronous and deterministic"，"Async work is described as `Effect` values, not executed"，因此 "dispatch fully testable without tokio or a terminal"（`app/dispatch/mod.rs:1-10`）。这条边界的直接证据是 `app/leader_cluster/mod.rs:104-147`：一个进程内测试装置能原样复用 `dispatch::dispatch` + `effects::execute` 完成整轮会话，只把终端换成了内存通道。

## 8. 冲突检测、用户自定义与元测试

### 8.1 冲突检测：只有渲染期去重，没有注册期校验

`ActionRegistry` **不检查重复绑定**。同一上下文内两个动作抢同一个键在执行期由「`Vec` 顺序先命中者胜」决定（`actions/mod.rs:277-290` 的 `return` 在首次匹配时发生）——这是隐式行为，无任何断言保护。

显式处理只发生在速查表渲染时（`views/shortcuts_help.rs`）：

- 按 `Category` 分组后，用 `seen_in_cat: HashMap<KeyShortcut, usize>` 对 `default_key` 去重；同键位冲突时「**激活上下文的那一行胜出**」，实现方式是若先前那行是 `dimmed` 而当前行是 lit 就整行替换（`views/shortcuts_help.rs:145-148, 236-248`）。注释点名的实例是 `DashboardStop`（列表）与 `DashboardOverlayStop`（浮层）共用 `Ctrl+X` 且同属 `Dashboard` 分类。
- overlay 遮蔽：`DashboardOverlay` 的 `default_key` 集合会从其他上下文的**亮行**里剔除；若某行所有键都被遮蔽则该行整条消失（`views/shortcuts_help.rs:137-144, 208-222`）。注释举的例子正是速查表自身的 `Ctrl+X` alt 被 overlay 的 stop 遮蔽。
- 同义键折叠：同一动作的多个 alt 若 `display_pretty()` 相同则折叠，避免出现「Shift+Tab / Shift+Tab / Shift+Tab」（`views/shortcuts_help.rs:170-185`），对应测试 `build_entries_dedupes_identically_rendered_alt_keys`（`views/shortcuts_help_tests.rs:67`）。

源码里唯一被显式写下来的冲突规避是三处注释，属于「人工约定 + 测试钉住」而非通用检测：`Ctrl+4` 在 `OpenDashboard` 与 `ToggleQueue` 之间只能有一个（`actions/defaults.rs:883-890`），`Ctrl+X` 在 `ShortcutsHelp` alt 与 `DashboardOverlayStop` 之间有意让后者遮蔽（`actions/defaults.rs:1176`），`Ctrl+G` 与 `Ctrl+L` 靠**上下文不重叠**来共存（`actions/defaults.rs:994-998, 1093-1095`）。

### 8.2 元测试：注册表本身没有，速查表有 69 个

**`tests/registered_features_are_documented.rs` 与键盘无关**。它只有 23 行，校验的是 `xai_grok_shell::agent::config::FEATURES` 这个**特性开关表**中每个 `spec.key` / `spec.env` 都出现在 `docs/internal/25-enterprise.md` 与 `22-environment-variables.md` 里（`crates/codegen/xai-grok-pager/tests/registered_features_are_documented.rs:1-23`）。用它来类比「键位文档完整性测试」是不成立的——项目里没有等价物。

真正针对键盘的测试分布在三处：

| 测试文件/模块 | 规模 | 覆盖内容 |
| --- | --- | --- |
| `src/actions/mod.rs` 内嵌 `mod tests` | 约 530 行 | 精确上下文匹配、`Ctrl+4` 互斥、终端族注册表、Minimal 裁剪、超时语义、遥测白名单 |
| `src/views/shortcuts_help_tests.rs` | 69 个 `#[test]` | 分组、去重、变暗、vim 门控、模式正确性、详情节交互 |
| `src/app/app_view_tests.rs` 等 | — | vim 模式行为、欢迎页、dashboard 键路由 |

`actions/mod.rs` 里最接近「冲突不变量」的测试是 `open_dashboard_and_toggle_queue_do_not_both_bind_ctrl_4`，它用 XOR 断言「宿主机默认下二者恰有一个占住 Ctrl+4」（`actions/mod.rs:933-944`）。`shortcuts_help_tests.rs` 里最接近的是 `build_entries_deduplicates_within_category`，遍历渲染结果断言同分类内首个键不重复（`views/shortcuts_help_tests.rs:317-339`）——注意它验证的是**渲染输出**，而非注册表输入。

### 8.3 用户自定义：命令面板是手写表，不是注册表驱动

`actions/mod.rs:5` 声称 "**Command palette**: `registry.all()` returns the fuzzy searchable list"。**实现不符**：全仓库 `registry.all()` 的生产调用点为零（仅在测试与 `settings_modal` 的另一个 `SettingsRegistry` 中出现）。命令面板实际由 `views/modal.rs:376` 的 `default_palette_entries()` 构建，其条目是手写的 `PaletteEntry`，且 `shortcut` 字段是**硬编码字符串**：

```rust
PaletteEntry { label: "New Session".into(), shortcut: "Ctrl+N".into(),
               command: PaletteCommand::NewSession },        // views/modal.rs:387-391
```

这意味着命令面板里显示的快捷键可能与注册表实际绑定漂移（例如 VS Code family 下 `Quit` 主键变为 `Ctrl+D`，而面板字符串仍是字面量）。这是「单一事实来源」在这一个消费者上没做到的缺口，也是后续复刻时值得引以为戒的地方。

## 9. 键盘协议与终端差异屏蔽

终端差异的屏蔽分四层，从协议到语义逐层兜底。

### 9.1 Kitty keyboard protocol 协商

启动时按「品牌 + 多路复用器 + DA2 版本」决定推什么标志（`app/mod.rs:1544-1574`）：

```rust
pub fn negotiated_kitty_flags(skip_reason: Option<&str>, da2_packed: Option<u32>)
    -> KeyboardEnhancementFlags {
    if skip_reason.is_some() { return KeyboardEnhancementFlags::empty(); }
    let mut flags = KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES;
    if !(da2_packed.is_some_and(|p| p <= ALACRITTY_BROKEN_EVENT_TYPES_MAX_PACKED)) {
        flags |= KeyboardEnhancementFlags::REPORT_EVENT_TYPES;
    }
    flags
}                                                    // terminal/kitty_keyboard.rs:36-50
```

关键设计取舍有三条，都在模块文档里写明了：

- **"Flags pushed" ≠ "releases arrive"**。Alacritty 0.14.x 及更旧（`alacritty_terminal` ≤ 0.24.1，阈值常量 `2401`，`kitty_keyboard.rs:29-32`）会被推到 `DISAMBIGUATE_ESCAPE_CODES` 但不给 `REPORT_EVENT_TYPES`，因为它对 Backspace/Tab/Enter/Escape 会重复上报 legacy release，crossterm 读成第二次 Press 导致 Enter 双提交（`kitty_keyboard.rs:3-5, 74-76`）。
- 因此代码区分三个谓词：`kitty_flags_pushed()`、`kitty_releases_reported()`、`kitty_event_types_withheld()`，注释明确要求 "Every feature that waits for a release (hold-to-talk, modifier-lift tracking) must gate on this, not on `kitty_flags_pushed`"（`kitty_keyboard.rs:65-83`）。
- **未知版本不降级**：DA2 在多路复用器下常被跳过，"no answer" 是常见情况，不能因此剥夺健康终端的 release 事件（`kitty_keyboard.rs:34, 109-116`）。

跳过原因表 `kitty_skip_reason()`（`terminal/mod.rs:317-352`）覆盖：`vscode`（含 Cursor/Windsurf/Zed）、`apple_terminal`、`vte`、`windows_terminal`、`jetbrains`、`screen`、`tmux_old`、`tmux_extended_keys_off`、`unknown_no_multiplexer`。注释给出最后一条的理由：没有 KKP 支持的正面证据就跳过，因为 xterm.js 会错误编码 shifted 键（引用 xtermjs/xterm.js#5823），而探测无响应的终端会阻塞启动（`terminal/mod.rs:342-347`）。

额外的按需层：`/gboom` 游戏打开时**再叠一层** `REPORT_ALL_KEYS_AS_ESCAPE_CODES`（WASD 需要 release 事件），用独立原子量跟踪，保证 teardown 时按 LIFO 弹两层（`app/mod.rs:105-133`）。

### 9.2 OS 修饰键探针救援

`KeyboardNormalizer::rescue_key()` 在 `handle_input` 顶部运行，因此所有下游表面看到的都是修好的事件（`input/keyboard_normalizer.rs:38-40`）。它做两件事：

1. **裸 `\x02` 规范化**：`Char('\u{0002}') + NONE` → `Char('b') + CONTROL`，与终端能力无关，总是执行（`keyboard_normalizer.rs:56-61`，测试 `raw_ctrl_b_canonicalizes_without_modifier_probe`）。
2. **修饰键补回**：仅对 `Backspace` / `Delete`，且仅当终端被判定为丢失该修饰轴（`ModifierDelivery` / `ModifierFate::Dropped`），用 macOS CoreGraphics 读物理键状态补 `SUPER` 或 `ALT`（`keyboard_normalizer.rs:62-91`；平台分支 `input/macos_modifiers.rs`，非 macOS 返回全 false，`keyboard_normalizer.rs:26-36`）。`Cmd` 优先于 `Opt`，注释理由：「Cmd+Backspace（行杀）是更强的动作」（`keyboard_normalizer.rs:74`）。

配套的抑制机制 `suppress_os_modifier_rescue()` 是 thread-local 且可嵌套的 guard（`input/terminal_support.rs:11-31`）：重放启动期排队的事件时不能拿**当前**物理状态去补修饰键，否则会凭空造出用户没按的和弦（`app/event_loop.rs:3338-3340`；测试 `queued_event_suppression_skips_os_probe_but_keeps_raw_ctrl_b_normalization`）。

Apple Terminal 的 `Enter` 特例单独处理：`is_mod_enter()` 把「Shift/Alt+Enter」与「终端丢标志但物理上按着 Shift/Opt/Cmd 的裸 Enter」统一视为换行，且**显式排除 SUPER**，因为很多终端把 Cmd+Enter 绑成全屏（`input/terminal_support.rs:33-53, 55-63`）。

### 9.3 品牌分支的默认键位

`terminal_context()` 的 `TerminalName`（18 个变体 + `Unknown`，`terminal/mod.rs:58-107`）在 `default_actions()` 开头被读一次，决定 6 处键位分叉（`actions/defaults.rs:60-73`）：`is_vscode_family()` 影响 Quit / HalfPageDown / InterjectPrompt / OpenExtensions；`AppleTerminal` 影响 InterjectPrompt；`local_mac_vscode`（VS Code family ∧ 非 SSH ∧ macOS）影响 ToggleQueue 与 OpenDashboard 的 `Ctrl+4` 归属。

`ctrl_dot_unreliable()` 是「键位可靠性」抽象成谓词的典型：没有 KKP 时 `Ctrl+.` 不是 C0 控制字符，会塌缩成裸 `.`，因此该函数直接复用 `kitty_skip_reason().is_some()`，保证两个判断永不漂移（`terminal/mod.rs:432-435`）；`actions/defaults.rs:12-14` 在其上再叠加 Windows 与 WSL。

Shift+Tab 的三种编码由 `shift_tab_keys()` 集中成列表返回 `[BackTab, BackTab+SHIFT, Tab+SHIFT]`，`CycleMode` 与 `DashboardCycleMode` 的 `default_key`/`alt_keys` 都从它读取（`input/key.rs:287-295`；`actions/defaults.rs:515-520, 976-982`）。注释点明原因：registry 的匹配是**精确修饰键**匹配，带 SHIFT 的形态必须放进 `alt_keys`。

### 9.4 语义层的差异吸收

- **大小写归一**：`KeyShortcut::new` 会把 `Char('G')` 补上 SHIFT，把 `Char('g')+SHIFT` 提成大写，于是 `Char('G')+NONE` 与 `Char('g')+SHIFT` 等价（`input/key.rs:46-57, 59-66`）。
- **Release 事件一致性**：`matches()` 对 `KeyEventKind::Release` 一律返回 false（`input/key.rs:60-62`），`is_shift_tab` 同样（`input/key.rs:297-301` 及其测试）。
- **非 KKP 终端的字母回落**：`vim_mode` 关闭时裸字母不解析为动作，而是 `ActionThenForward(Action::FocusPrompt)`，让字母落进输入框（`app/agent_view/panes.rs:119-133`）。
- **无需 KKP 也能用的和弦**：`Ctrl+Space` 无 KKP 时解码为 NUL 但上报为 `Char(' ')+CONTROL`，因此语音和弦在所有终端都能解析（`actions/defaults.rs:670-672`）。

## 10. 边角细节

- `interjection_possible(turn_running, has_payload)` 是插话和弦的可用性闸门，注释解释了最后一格存在的原因：空输入框的强制发送（`actions/mod.rs:301-306`）。
- 测试专用注册表 `non_vscode_for_test` / `apple_terminal_for_test` / `vscode_family_for_test` 通过复制默认表再改写 4 个或 1 个字段来钉住宿主无关行为，并保留了 `ScreenMode`（`actions/mod.rs:308-382, 765-782`）。
- `key_for_mode()` 在非 vim 模式下返回「第一个非字母 alt」，注释理由是「让快捷键栏展示一个真正能用的键」（`actions/mod.rs:448-464`）。
- 快捷键栏是结构化渲染而非拼字符串：`HintItem` 携带 `Vec<KeyShortcut>`，共享修饰前缀时渲染成 `Ctrl+J/K` 形式（`views/shortcuts_bar.rs:17-34, 78-125`）；`pinned` 的提示在 compact 截断中永远保留（`views/shortcuts_bar.rs:144-151, 335-355`）。
- `KeyShortcut` 有三套显示形态，用途严格分离：`display()`（栏内紧凑，小写，`Ctrl+c`）、`display_pretty()`（速查表，`Ctrl+C` 但 `Ctrl+l` 小写字母）、`display_telemetry()`（遥测，跨平台稳定，字母大写，`Ctrl+L`）。测试 `telemetry_chord_is_platform_stable` 明确断言 pretty 与 telemetry 不可混用（`actions/mod.rs:533-541`；`input/key.rs:139-235`）。

## 关键结论（供合稿引用）

1. 键位只有一处来源：`actions/defaults.rs:56` 的 `default_actions(screen_mode, mouse_reporting_toggle_enabled)`，全仓库无其他生产注册路径（`actions/mod.rs:240-272`、`actions/defaults.rs:1-3`）。
2. 动作身份是编译期 enum：`ActionId` 76 个变体、无字符串名、无 serde、无 `FromStr`，唯一字符串化出口是 2 项遥测白名单与上下文标签（`actions/mod.rs:34-36, 169-182, 487-493`）。
3. 默认注册 70 个 `ActionDef`：主 vec 50 + `mode_ctrl_g_action` 二选一 + Dashboard 块 19；开启 `mouse_reporting_toggle` 再 +1，Minimal 模式按上下文与 id 双重裁剪（`actions/defaults.rs:75-855, 857-873, 878-1192, 1197-1204`）。
4. **键位不可重映射**，用户手册明说 "cannot currently be remapped"，而 `actions/defaults.rs:901-902` 的注释声称可经 `config.toml` 重绑定属**注释与实现不符**（`docs/user-guide/03-keyboard-shortcuts.md:3`）。
5. 上下文是 7 值 `When` 枚举，`lookup()` 做**精确**匹配、冒泡顺序写死在调用方；`When::WelcomeScreen` 有枚举分支但**零注册**，欢迎页键位由 `handle_welcome_input()` 内联字面匹配（`actions/mod.rs:149-167, 277-290`；`app/app_view.rs:3175, 3696, 3708, 3711`）。
6. **没有多键序列，也没有 leader key**；`app/leader_cluster/` 是进程级 leader IPC 测试装置，与键盘无关（`actions/mod.rs:277-290`；`app/leader_cluster/mod.rs:1-24`）。
7. 唯一的「两段式」机制是双击确认：确认型动作 1000 ms、空闲 `Esc Esc` 800 ms、Dashboard 删除 2 s，另有 1000 ms 的 `ESC_CANCEL_REWIND_GRACE` 防误触（`app/app_view.rs:474-476, 483-489`；`app/agent_view/prompt.rs:739-746`）。
8. 分发链路是 `crossterm poll/read → TimedInputEvent → AppView::handle_input → pane/agent/global 三层 lookup → InputOutcome → Action → 同步 dispatch → Effect → 异步 execute`，dispatch 层不碰终端/网络/文件系统（`app/reader_thread.rs:67-69`；`app/app_view.rs:2320`；`app/dispatch/router.rs:156`；`app/dispatch/mod.rs:1-10`）。
9. 没有注册期冲突校验；冲突处理只出现在速查表**渲染期**——同分类按 `default_key` 去重且「激活上下文胜出」，overlay 占用的键从其他上下文亮行剔除（`views/shortcuts_help.rs:137-148, 208-248`）。
10. `tests/registered_features_are_documented.rs` 与键盘无关，它校验的是 `FEATURES` 特性开关的文档覆盖（`tests/registered_features_are_documented.rs:1-23`）；键位测试在 `actions/mod.rs` 内嵌测试与 `views/shortcuts_help_tests.rs`（69 个 `#[test]`）中。
11. 命令面板**未**接入注册表：`actions/mod.rs:5` 声称用 `registry.all()`，实际是 `views/modal.rs:376` 的手写 `PaletteEntry` 表，快捷键以硬编码字符串给出（`views/modal.rs:387-391`）。
12. Kitty 协议只推 `DISAMBIGUATE_ESCAPE_CODES`，并按 DA2 版本决定是否叠加 `REPORT_EVENT_TYPES`（Alacritty ≤ 0.24.1 不叠加，阈值 2401）；「推了标志」与「能收到 release」是两个独立谓词（`terminal/kitty_keyboard.rs:29-50, 65-83`）。
13. 终端差异屏蔽分四层：协议协商（`kitty_skip_reason()` 9 类原因）、OS 修饰键探针救援（CoreGraphics 补 Cmd/Opt+Backspace）、品牌分支默认键位（6 处分叉）、语义归一（大小写归一、Release 一律不匹配、非 vim 字母回落转发给输入框）（`terminal/mod.rs:317-352`；`input/keyboard_normalizer.rs:55-91`；`actions/defaults.rs:60-73`；`input/key.rs:46-66`）。
14. 除注册表外还有三处键位孤岛需要单独清点：欢迎页内联匹配、scrollback 的 vim `/` 搜索、prompt 的 `↑` 历史与粘贴/撤销重做伪行（`app/app_view.rs:3175`；`app/agent_view/panes.rs:111-120`；`views/shortcuts_help.rs:230-310`）。
