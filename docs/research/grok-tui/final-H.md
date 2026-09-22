## 2. 设计理念与原则

> 证据分级沿用专题文件 28 的三档约定：**【官方陈述】**＝面向用户/贡献者的明示表述（`README`、`docs/user-guide/**`、`docs/tutorial/**`、`CHANGELOG`），或源码注释中显式写明理由的；**【归纳】**＝从实现中读出的模式，官方未明说，**不得当官方立场引用**。路径均省略 `reference/grok-build/` 前缀。

### 本节要点

- **前提修正**：官方没有「选内联滚动而非全屏备用屏」这条信条。默认形态是全屏 `alt-screen`，内联是备用屏不可用或不可靠时的降级目标，第三条 `--minimal`（scrollback-native）是实验分支（`crates/codegen/xai-grok-pager/src/app/mod.rs:340-348`、`docs/user-guide/21-terminal-support.md:174-179`）。
- 2.1 每条都带英文原句 + 中文翻译 + 路径，且只收官方陈述；2.2 的十条模式全部标【归纳】。
- 复用价值最高的四条模式：单一真理来源、不阻塞输入、焦点显式所有权、只追加约束反向决定架构。
- 2.3 的「刻意放弃」几乎都有源码注释留痕，理由多与「影响子进程或用户环境」有关，而不只是性能。
- 上游仍存在文档-代码不一致与若干「未找到证据」项（见 2.3 末尾），引用时须保持原标注，不要升级为结论。

---

### 2.1 官方明确陈述的立场

**（1）形态定位：TUI 是同一个 agent 运行时的一层表现层，不是「聊天界面」**【官方陈述】

> "**Grok Build** is SpaceXAI's terminal-based AI coding agent. It runs as a full-screen TUI that understands your codebase, edits files, executes shell commands, searches the web, and manages long-running tasks — interactively, headlessly for scripting/CI, or embedded in editors via the Agent Client Protocol (ACP)."
> —— `README.md:13-17`（`docs/user-guide/01-getting-started.md:5` 复用同句）

「Grok Build 是 SpaceXAI 的终端 AI 编程 agent。它作为全屏 TUI 运行，能理解你的代码库、编辑文件、执行 shell 命令、搜索网页、管理长任务——可交互使用，也可无头运行用于脚本/CI，或通过 ACP 嵌入编辑器。」

仓库结构表用同一口径描述 TUI crate：`"| crates/codegen/xai-grok-pager | The TUI: scrollback, prompt, modals, rendering |"`（`README.md:100`）。

**（2）默认渲染形态是全屏，内联是降级目标**【官方陈述】

> "Terminal UI (TUI) for Grok Build. Provides the interactive **full-screen** interface including the scrollback view, prompt input, session management, and all modal dialogs."
> —— `crates/codegen/xai-grok-pager/README.md:3-5`（加粗为专题原文所加）

两条 `Auto` 变体的注释把默认策略写得更直白：

> "Automatic: fullscreen in healthy environments, inline in degraded ones." —— `crates/codegen/xai-grok-pager-render/src/appearance/config.rs:679`
> "Automatic: fullscreen in plain terminals and normal tmux, inline in tmux control mode and Zellij." —— `crates/codegen/xai-grok-pager-render/src/terminal/mod.rs:901`

面向用户的说明与之一致：「Zellij and tmux control mode can limit the alternate screen. Grok normally uses inline mode in those environments. … You can configure `[terminal] alt_screen` in `~/.grok/pager.toml`, or run `grok --no-alt-screen` to confirm inline mode works.」（`docs/user-guide/21-terminal-support.md:174-179`）

**（3）内联滚动的价值被写成独立 crate 的产品特性，而非权宜之计**【官方陈述】

> "A Rust library for building terminal applications with inline viewports - dynamic UI elements that stay at the bottom of the terminal while **preserving scrollback history above them**." —— `crates/codegen/xai-ratatui-inline/README.md:3`
> "This single implementation works universally across all terminals and multiplexers **without special modes or workarounds**." —— 同上 `:53`

**（4）为一等能力付出的代价：放弃 ratatui 自带的 resize 处理**【官方陈述】

> "When using inline viewports on the main screen (not alternate screen), terminal resize causes issues: Terminal reflows content automatically BEFORE the app receives SIGWINCH / Old viewport borders get reflowed as garbage text / **Built-in `autoresize()` corrupts scrollback history** / Cursor position queries (DSR) have race conditions during rapid resize"
> "We use the 'nuclear option' - completely reset and re-render"
> —— `crates/codegen/xai-ratatui-inline/README.md:101-106, 110`

**（5）键盘优先以「契约」而非口号陈述**【官方陈述】

> "Reference for key bindings in the Grok Build TUI. **Bindings are built in and cannot currently be remapped.**" —— `docs/user-guide/03-keyboard-shortcuts.md:3`
> "**Esc is not a focus key.** It follows the clear / rewind semantics below, and it never cancels a running turn (`Ctrl+C` does)." —— 同上 `:100`

阻塞式交互被定义为「键盘所有权显式转移」：

> "Four surfaces block the agent on your answer and **take over the keyboard** while they are open … the shortcuts bar always shows the keys of whichever one is receiving them."
> "**That hint is pinned, so a narrow bar can never trim away the only route back.**"
> —— `docs/user-guide/03-keyboard-shortcuts.md:104-125`

配套的用户可见文案把 `Esc` 与 `Ctrl+C` 的分工钉死：「**Esc never cancels a turn; pressed mid-turn it shows a reminder to use Ctrl+C.**」（`crates/codegen/xai-grok-pager/src/actions/defaults.rs:509-511`）

> **注意**：专题文件 28 §1.4 明确记录，**未找到** "keyboard-first" 这类字面宣称；键盘优先是本节从上述硬约束归纳的，不是官方原词。

**（6）不阻塞输入**【官方陈述】

> "## Keep typing while Grok works / While a turn is running, `Enter` **queues** your next message instead of interrupting. Change your mind? Press `Enter` on the empty prompt to stop the current turn and send the queued message right away."
> —— `docs/tutorial/02-first-prompt.md:9-13`

三种语义被刻意区分，且写明理由：「**Send-now is intentionally interruptive** — it reads as 'stop what you're doing and take this'. To hand the agent a note **without** stopping it, queue with plain `Enter`.」（`docs/user-guide/03-keyboard-shortcuts.md:325`）

**（7）渐进式披露与少即是多**【官方陈述】

- 文档体系分三档：「## Tier 1: Essential User Docs / **Start here.** These guides cover what you need on your first day.」（`docs/user-guide/README.md:7-9`，其后 `Tier 2: Core Feature Docs` `:21`、`Tier 3: Advanced Usage Docs` `:38`）
- 「Start with `AGENTS.md` and a theme; add the rest when you need it.」（`docs/tutorial/08-make-it-yours.md:38`）；「**You don't need to memorize anything:** `/` and `Ctrl+P` will always show you what's available.」（`docs/tutorial/05-slash-commands.md:33-34`）

**（8）保守的副作用边界：官方明确列出「不做」的事**【官方陈述】

> "Grok **deliberately does not** run `tmux source-file` or change the live tmux server. … The conservative config scan checks direct global assignments only; review sourced files, conditionals, plugins, and generated tmux setup yourself." —— `docs/user-guide/21-terminal-support.md:43-47`
> "Many terminals already reorder right-to-left text themselves … Grok Build therefore **does not** reorder RTL by default." —— 同上 `:265-269`
> "Grok advertises only `Shift+Enter` and `Alt+Enter` as newline. **Many terminals bind Cmd+Enter to fullscreen, so `SUPER` is excluded from the newline matcher**" —— 同上 `:216-219`

**【归纳】** 共同的判断标准是：**当终端或环境已经在做某件事，或某个动作会污染用户环境时，应用侧主动退让**。同章另有一条同向细节：`method = "bel"` 被当作用户的显式意图，不再报诊断（`docs/user-guide/05-configuration.md:542`）。

**（9）迁移友好做到配置级**【官方陈述】

> "Coming from Claude, Cursor, or Codex? / **Fear not — your settings, rules, and skills come with you.** Grok Build reads the same project conventions other agents use, and imports the rest." —— `docs/tutorial/01-coming-from-another-tool.md:1-4`

自动接管清单见同章 `:8-17`（`AGENTS.md`、`CLAUDE.md` 含嵌套、`.claude/rules/`、`.cursor/rules/`、`~/.claude/skills/`、`.mcp.json`、`.claude/settings.json` 的 hooks 等），配套 `/import-claude`、`/resume-{claude,codex,cursor}`、`grok inspect`，且每个兼容来源可在 `[compat.claude]` / `[compat.cursor]` 下关闭。

**图 1：三种渲染模式不是并列互斥，而是「默认 + 降级 + 实验」次序**（源自专题 28 图 1，仅压缩排版）

```
                       ScreenMode  (src/app/mod.rs:340-348)
  [默认，健康环境]  Fullscreen  alt-screen 整块替换终端画面（AltScreenMode::Always / Auto）
  [降级，环境受限]  Inline      --no-alt-screen；tmux control mode / Zellij 自动选中
                              仍走 ScrollbackPane（应用内滚动/折叠/选择/鼠标）
  [实验，第三条路]  Minimal     --minimal / /minimal；定稿块一次性打印进 native scrollback
                              "the terminal owns history"

  优先级：env override(GROK_SCREEN_MODE_ENV) > minimal flag/config > alt-screen policy
          （src/app/screen_mode_relaunch.rs:303-320）
  持久化：/settings → Default screen mode → [ui] screen_mode（重启生效）；
          /minimal //fullscreen 只切当前会话（src/settings/defs.rs:495-507）
```

---

### 2.2 从实现中归纳出的设计模式

以下十条**全部是【归纳】**，来源见各条证据；官方没有以这些措辞陈述过它们。

**模式 1：单一真理来源——一处定义，消费方只读**

- 颜色只有一个来源：扁平 `Theme` 结构体的 58 个 `Color` + 6 个 `Modifier` 字段，渲染层无硬编码色（`.../theme/tokyonight.rs:46-144`、`.../theme/mod.rs:1`）。
- `xai-grok-pager-render/src/glyphs.rs` 是全项目唯一的 chrome 字形源，34 个 `pub fn`，被两个 crate 共同引用（`glyphs.rs:16-334`）。
- 键位只有一处注册路径：`actions/defaults.rs:56` 的 `default_actions(screen_mode, …)`，全仓库无其他生产注册路径（`actions/mod.rs:240-272`）。
- 「快捷栏提示」与「按键实际行为」读同一张表：`EscStep` 8 档枚举带用户可见标签，机制上杜绝提示与行为不一致（`app/agent_view/key_owner.rs:55-91, 167-250`）。

**模式 2：不阻塞输入——把一切重的移出 UI 线程**

- 滚动搜索在后台 daemon 跑正则：「**This keeps per-keystroke typing responsive on long sessions where a synchronous scan would stall the input thread.**」（`src/scrollback/search.rs:9-12`）
- 终端输入由专属 reader 线程 `poll/read` 后经 mpsc 送入事件循环（`app/reader_thread.rs:67-69`）。
- 三条重路径被移出 UI 线程：滚动搜索 daemon、编辑 diff 整文件高亮 worker、Mermaid worker（`25-render-pipeline-performance.md` §11）。
- 连「丢一个字符」都不接受：用户输入必须同批渲染（见 2.3 放弃项 2），启动期按键被捕获并回放（`src/app/mod.rs:1451-1453`）。

**模式 3：渲染按需驱动——一个 `select!` + 每轮唯一绘制点**

- 主循环是单个 bias 排序的 `tokio::select!`，绘制只发生在每轮末尾唯一一次 `present_if_dirty`；空闲时终端收到零字节（`event_loop.rs:2110, 2938`、`draw.rs:689-706`）。
- 「要不要重绘」是三条件与：`dirty` ∧ 无在途未确认帧 ∧ writer 队列已排空（`event_loop.rs:579-600`）。
- 空闲时连 tick 都不 arm（`TickDemand::None`）；对应的降级手段是配置 fps 与按显示器刷新率自动定档，**不存在 reduce-motion 概念**（`app/app_view.rs:244-257`、`event_loop.rs:3052-3067`；另见 2.3 放弃项 7）。
- 动画帧靠整数除法降帧而非调帧率：事实标准 `SPINNER_DIVISOR = 4`（30 fps 下约 7.5 fps）（`src/views/turn_status.rs:29-36`）。

**模式 4：焦点是显式所有权 + 固定排名，不是栈**

- 面板焦点是单一 `ActivePane` 枚举（7 值）加各自的 `OverlayState` 三态；切换集中在 `set_active_pane`，脏编辑会拒绝切换（`views/agent.rs:31-41`、`views/overlay.rs:47-56`）。
- 真正的模态是**单槽位** `Option<ActiveModal>`，多层浮层的 z 序由渲染函数的 `if … return` 分支顺序定义，代码里就是一张行号清单：`:3211 → 3244 → 3430 → 3600 → 3643 → 3702 → 3894 → 3922 → 3940 → 4239 → 4259`（`views/modal.rs:180`、`app/agent_view/render.rs:3211-4259`）。
- 键盘归属是排名表而非栈：`KeyOwner` 固定为 Permission > LineViewer > BlockViewer > PlanApproval > 其它卡片 > Pane（`app/agent_view/key_owner.rs:110-132`）。
- 五类裁决交互统一收敛为「阻塞卡」，`blocking_card()` 给出固定优先级，permission 可抢断其他卡片（`key_owner.rs:96-105`）。

**模式 5：环境能力探测成枚举，再由枚举驱动降级；`Auto` 是默认值，显式配置只是逃生口**

- 业务代码从不读环境变量，只读 `TerminalContext` 上的能力位（`terminal/mod.rs:225-265`）；`TerminalContext` 同时保留乐观的 `brand` 与原始 `env_brand`，所有 default-deny 判断回查后者（`terminal/mod.rs:228-232, 725-730`）。
- 色彩能力四档 `None < Basic < Ansi256 < TrueColor`，`NO_COLOR` 拥有最高优先级直接判 `None`（`color_support.rs:17-30, 83-89`）。
- 字形降级是 default-deny：Windows 且终端品牌不在 11 项白名单内即视为 legacy ConHost（`glyphs.rs:404-423`）。
- 形态选择也自动化：`Auto` 按 `tmux control mode` / Zellij / 具体终端缺陷降级，例如「JetBrains terminals on Windows now default to minimal mode to avoid raw mouse-report leaks in the prompt.」（`CHANGELOG.md:1416`）

**模式 6：native scrollback 的「只追加」约束反向决定整条架构**

这是对内联 / 滚动回模式最硬、也最有复用价值的结构性约束：

> "/// Add a finalized block positioned immediately before the entry `anchor`, instead of at the end. **`anchor` must not already be committed. A terminal's native scrollback is append-only. Inserting above a block already printed there would emit the new block below content that logically follows it.**"
> —— `src/scrollback/state/mod.rs:592-595`

约束用 `debug_assert!` 固化（`state/mod.rs:596-602`），并派生出三件事：**committed frontier**（已打印条目的连续前缀，`xai-grok-pager-minimal/src/commit.rs:5-7`）、**已提交集合用 `EntryId` 而非位置索引**（`"Keyed by EntryId (not a per-entry flag) so it survives shift_remove / remove_from reordering for free"`，`state/mod.rs:75-76`）、以及**待输入块绝不提交**（见 2.3 放弃项 5）。

**模式 7：就地更新——状态活在不进 transcript 的表面**

- 子代理在父 scrollback 里**只有一个块**，靠 `scrollback_entry_id` 就地更新状态与活动后缀；只有后台式才在结束时追加第二个块（`src/app/subagent.rs:80-92`、`src/scrollback/blocks/subagent.rs:1-11`）。
- 任务计数只活在 idle 的 still-running 状态行上，完成才落一个 `Task completed` chip，**transcript 从不再复述计数**（`docs/user-guide/20-background-tasks.md`、`src/views/turn_status.rs:108-125`）。
- 屏幕状态面一共 5 个，运行态在独立的 `turn_status` 行而不是状态栏；`Idle` 且无 watcher 时整行消失（`views/turn_status.rs:365-395`）。
- 上游把这条归纳成一句：「**不打断主流程**的核心手法是状态只在非 transcript 表面就地更新」（`13-auxiliary-panes.md` 关键结论 12）。

**模式 8：渐进式披露，且模式门控必须带理由**

- 可发现性是分层的：常驻 footer（全屏）→ 状态栏广告（minimal）→ 命令面板 → 教程 → how-to 文档，逐层变重（`src/slash/commands/help.rs:3`、`src/tutorial_docs.rs:3-4`、`docs/user-guide/README.md:7-41`）。
- minimal 模式没有常驻 footer，于是 `/help` 被提升为可发现入口并在状态行广告（`src/slash/commands/help.rs:3`）。
- 不可用命令**不静默隐藏**，而是返回带 `why` 的拒绝文案并指出切换命令：

```rust
pub enum Remedy {
    SwitchMode { why: &'static str },   // 例："minimal is single-session"
    UseInstead(&'static str),
    AlreadyInMode,
}
```
—— `src/slash/mode_support.rs:3-15`；文案模板 ``"/{token} isn't available in {current} mode ({why}). Run {switch} to switch this session."``（`:45-49`）

- 另一种披露手法是**变暗而非隐藏**：快捷键手册把上下文不生效的绑定 dim 掉，注册表仍全量可见（`src/views/shortcuts_help.rs:63-72, 127-130`）。

**模式 9：分层数据流——块 → 条目 → 行 → 单元格，块产出的已是屏幕行**

关键分层决策是：`BlockContent::output()` 产出的 `BlockLine` **已经是换行后的屏幕行**，因此布局层只做「高度算术 + 可见窗口切片」，不参与文本排版（`render.rs:498`、`blocks/system.rs:26-30`）。块本身是纯函数，渲染到 Buffer 由另一个 trait（`Renderable`）负责（`block.rs:40, 298`）。

```
会话 / 工具事件
   │  AgentView 把消息、工具结果、会话事件转成块
   ▼
块（RenderBlock 13 变体 / BlockContent::output）   block.rs:40, 298
   │  ★ 每个 BlockLine 已是「按内容宽度换行后的一行」  render.rs:498
   ▼
条目（ScrollbackEntry + 4 层缓存 + LayoutCache）   entry.rs:57、layout.rs:42
   │  每帧：prepare_layout → compute_paint_window（只遍历可见窗口）
   ▼
单元格（ScrollbackPane::render_with_scratch → EntryRenderer）
                                             scrollback_pane.rs:139、entry_renderer.rs:23
```
（源自专题 09 §1 的分层全景图，此处压缩为骨架）

每行自带四类语义元数据：`Selectable`、`selection_range`、`selection_text`、`joiner`（软换行连接符），**一套数据同时服务绘制 / 搜索 / 复制 / 导出**（`types.rs:146-169`、`export.rs:1-6`）。

**模式 10：远端数据一律容错，跨模块约定写成会失败的测试**

- 远端 JSON、公告、changelog 一律「字段全 optional、坏输入降级为空、绝不因为远端数据让客户端崩」：`"All fields use #[serde(default)] so a single malformed entry doesn't kill the entire array parse."`（`xai-grok-shell-base/src/util/changelog.rs:22-23`）、`"Each format is fetched independently with its own cache file, so a failure in one doesn't block the other."`（同上 `:41-42`）。
- 元测试把约定变成会失败的测试：文档与代码对账靠 `include_str!` 读入 Markdown 再按反引号切表格单元做集合运算，**反引号是机器可读标记**（`src/app/status_line_tests.rs:510-515`）；minimal 模式甚至用一个源码扫描测试禁止未使用的 API（`xai-grok-pager-minimal/src/guard.rs:9-13`）。

**关键常量与阈值**（照抄上游，勿改口径）

| 常量 / 阈值 | 值 | 位置 |
| --- | --- | --- |
| `SCROLLBACK_MIN_ROWS` | 5（scrollback 唯一地板） | `views/agent.rs:181-299` |
| `SHORT_TERMINAL_ROWS` / `AUTO_COMPACT_MAX_ROWS` / `SMALL_SCREEN_TIP_MAX_ROWS` / `MIN_TERMINAL_WIDTH` | 16 / 20 / 28 / 60 列 | `views/agent.rs:89-104`、`tips/small_screen.rs:23-29`、`views/timeline.rs:20` |
| `MEASURE_MARGIN_ENTRIES` / `RESUME_WARM_PAGES` | 8 / 3 屏 | `state/layout.rs:669/745` |
| `animation.fps` / `SPINNER_DIVISOR` | 30 / 4 | `appearance/config.rs:335-362`、`views/turn_status.rs:29-36` |
| `min_draw_interval` | 16 ms（钳 1–100） | `event_loop.rs:2193-2222` |
| `ACP_DRAIN_BATCH_MAX` | 32 条 | `event_loop.rs:1837` |
| `WRITER_BLOCKED_WARN_AFTER` | 5 s | `event_loop.rs:487` |
| 双击确认 TTL / `Esc Esc` / Dashboard 删除 | 1000 ms / 800 ms / 2 s | `app/app_view.rs:474-489` |
| prompt ack 硬超时 / 软提示 | 120 s / 10 s（钳 5–3600 s） | `app/prompt_ack.rs:14-24, 48` |
| 提问卡超时 | 30 分钟（可覆盖） | `ask_user_question/mod.rs:47-64` |
| 全文件高亮上限 | 2 MiB / 50000 行 | `blocks/tool/edit.rs:44-49` |
| 终端输出模拟器上限 | `MAX_ROWS = 50_000` / `MAX_COLS = 8_192` | `terminal_output.rs:1-33` |

**图 2：两种「历史归属」，决定了整条渲染管线**（源自专题 28 图 2，压缩排版）

```
【Fullscreen / Inline —— 应用拥有历史】
┌──────────────────────────────────────────────────────┐
│ ScrollbackPane（应用内滚动：选择/折叠/鼠标/翻页）      │ ← Tab 可聚焦
│ [todos Ctrl+T] [tasks Ctrl+G] ← 按需滑入侧栏         │
│ Prompt（`/` `@` `!` `#` 前缀触发各自下拉）           │
│ Shortcuts bar（本上下文相关键位；关键提示 pinned）    │
└──────────────────────────────────────────────────────┘

【Minimal —— 终端拥有历史】
   …（上方全是终端自己的 scrollback，可用原生滚动/搜索/复制）…
   定稿块已按 committed frontier 一次性打印 ↑
┌──────────────────────────────────────────────────────┐
│ pinned live region（minimal_live_rows，默认 10 行）   │
│   · running-turn 状态 / todos  · Prompt              │
│   · minimal 状态行（含 /help 广告位）                │
└──────────────────────────────────────────────────────┘
```

---

### 2.3 被刻意放弃的方案

这一批取舍的共同特征：**被否决的往往是「显然方案」，且理由写在源码注释里**。以下标注均为【代码注释】或【官方陈述】，逐条给出原文。

**放弃 1：用有界通道做背压 —— 宁可内存增长，也不卡输入线程**

> "// Unbounded so `update_query`'s send never blocks the input thread. // **A bounded channel could stall the 257th keystroke on a full buffer, and dropping on full (try_send) could lose the final query** // Messages are tiny (an Arc pointer and the query) and the daemon drains the whole queue on each wakeup, so it stays short in practice"
> —— `src/scrollback/search.rs:211-214`；配套 `"/// Deliberately detached, never joined: closing a search must not block the UI on an in-flight scan."`（`:203`）

常规做法是有界通道加背压；这里判定「卡住输入」比「内存增长」不可接受，并用「消息极小 + 每次唤醒全排空」论证上界。

**放弃 2：跨批保留未完成的 CSI 片段 —— 宁可丢片段，也要立刻渲染**

> "// User input must render immediately // Carrying only `Bracket` across batches is therefore unnecessary and holds the key until the next keystroke"
> —— `src/app/csi_filter.rs:109-111`

回归测试直接写明用户体验代价：`"/// A typed `[` must be emitted in the same batch, not held until the next keystroke (**which made the cursor look stuck / 'laggy'**)."`（`csi_filter.rs:374`）

**放弃 3：刻意不处理 `SIGPIPE` —— 因为会影响 fork+exec 子进程**

> "SIGPIPE is **intentionally left alone**. The disposition is `SIG_IGN` (Rust's stdlib default), so writes to a closed pipe return `Err(BrokenPipe)` instead of terminating the process. … **Installing a custom handler would change the disposition inherited by fork+exec children (MCP servers, hooks) from `SIG_IGN` to `SIG_DFL`. That re-introduces a prior SIGPIPE regression.**"
> —— `src/app/signal_handler.rs:12-16`

专题 28 评价这是「为什么不用显然方案」写得最完整的一条：显然方案是装 handler，但被「影响子进程继承的 disposition」否决。

**放弃 4：不走 ratatui 的 `try_draw()` / `autoresize()`**

帧管线手工编排 `autoresize → get_frame → set_frame_links → flush_with_links → swap_buffers`，理由是 `try_draw` 每帧无条件发 `Show`/`MoveTo` 会重置闪烁计时器（`render/draw.rs:1-38, 439-486`）；为保光标闪烁还会**跳过空 diff 的渲染**（`"/// Skipping them preserves the cursor blink timer"`，`src/app/mod.rs:333-336`）。内联视口另行放弃 `autoresize()`，改用全清重画（见 2.1 第 4 条）。

**放弃 5：等待用户输入的块绝不提交，即使内容「看起来已定稿」**

> "// A block awaiting user input (permission / ask_user_question) holds the frontier in EVERY turn state, not just mid-turn // **Its rendered form still changes when the prompt resolves, so committing it (print-once) would freeze the 'waiting' form on the terminal**"
> —— `xai-grok-pager-minimal/src/commit.rs:27-29`

同源约束：提交前后高度必须一致，否则提示框会抖——`"/// Whatever the value, both sides of the commit frontier must apply it identically … Otherwise a block's height would change as it moves into native scrollback and the prompt would shift on every commit."`（`commit.rs:14-16`）

**放弃 6：并排（side-by-side）diff —— 全仓没有这个模式**

diff 不画任何 `+` / `-` 行首符号，增删完全靠行号色 + 内容背景带区分；行号栏可切换 GitHub 式旧/新双列，但**这只是行号双列，不是并排内容**（`blocks/tool/edit.rs:85-87, 103-105, 469-484, 568-669`；全量检索 `side-by-side` 仅命中 welcome 页等无关处）。

**放弃 7：reduce-motion 概念 —— 不存在，也不需要**

降级手段换成了别的：配置 fps、按显示器刷新率自动定档（`GROK_MIN_DRAW_MS` / `GROK_SCROLL_CADENCE_MS`，8–16 ms，clamp 1..100）、离屏不驱动、minimal 模式收紧到 16 ms、老终端字形回退（不改帧率）（`display_refresh.rs:11-22, 300-321`、`app_view.rs:5563-5571`、`glyphs.rs:100-175`）。上游确认**未找到**全局减少动效开关（`23-accessibility-adaptivity.md` 关键结论 11）。

**放弃 8：键位重映射（以及一段注释与实现不符）**

用户手册明说 `"Bindings are built in and cannot currently be remapped."`（`docs/user-guide/03-keyboard-shortcuts.md:3`），而 `actions/defaults.rs:901-902` 的注释声称可经 `config.toml` 重绑定属**注释与实现不符**（`16-keyboard-and-actions.md` 关键结论 4）。同类克制还包括：没有多键序列、没有 leader key、没有注册期冲突校验，冲突只在速查表渲染期去重。

**放弃 9：状态栏不做模板引擎**

表达力只有两条路：6 个内置 item 排序，或用户脚本读 JSON stdin 输出 stdout（`xai-grok-status-line/src/config.rs:325-332`、`views/status_line/segments.rs:66-118`）。空间不足时先牺牲 status_line 自己的行，**prompt 与 shortcuts 的 1 行永不被挤掉**（`views/agent.rs:134-135, 229, 246, 281-298`）。

**放弃 10：色盲专用方案 —— 不存在（不可与能力降级混用）**

`color_support.rs` 的 16 色映射是色彩能力降级，**不是无障碍配色**；两者在合稿时不可混用（`23-accessibility-adaptivity.md` 关键结论 7、6）。

**放弃 11：per-frame 剪枝、门控 fail-open、tmux 直连下的重复同步 —— 三处都选择不做**

- 「别用 per-frame 剪枝」：`"/// Call this from real update paths only: **a per-frame prune would churn on transient list states.**"`（`xai-grok-announcements/src/lib.rs:113-114`）
- 订阅门控宁可挡住用户：`"Painting such a gate directly **flashes a paywall at a paying user** … **On check failure or timeout we err on blocking.**"`（`src/app/subscription.rs:6-9`）
- tmux 直连时**刻意不发** DEC 2026 同步更新，因为 tmux 会重绘整个 pane 且自身已同步（`terminal/tmux.rs:55-57`）。
- 取消粒度只有 `Turn` 与 `Compaction` 两种，**没有「取消当前工具」**（`crates/codegen/xai-grok-telemetry/src/events/turn.rs:38-43`）。

**不确定项 / 上游标注「未找到证据」的部分（引用时保留原标注）**

- 与同类 TUI 的关系：**未找到证据**表明 Grok Build 在 TUI 设计层面点名过 codex / opencode / claude code 做取舍对比；扫描命中的只有工具实现移植（`crates/codegen/xai-grok-tools/THIRD_PARTY_NOTICES.md:17-31`）、配置/会话兼容（`src/app/foreign_sessions.rs:206-257`）与内置 agent 类型名（`src/views/agents_modal.rs:270`）。立场是**兼容并收**，不是设计对照。
- resize 文案与代码不一致：`xai-ratatui-inline` 的 README 称用 RIS（`ESC c`），代码实际发 `\x1b[2J\x1b[3J\x1b[H` 并注释说明 RIS 不清 iTerm/Terminal.app 的 scrollback（`README.md:108-111` vs `src/resize.rs:16-17`）。
- **不存在**「非 Unicode 终端 → ASCII」的通用降级；唯一开关是 legacy Windows ConHost（`glyphs.rs:406-424`）。**未发现任何全局水印机制**（`views/welcome/logo.rs`）。
- 输入区**未找到**按模式切换光标形状或颜色的代码，也没有 normal/insert 两态（`11-prompt-widget.md` §10）。
- 「所有鼠标操作都有键盘等价」**没有形式化保证**，只找到零散的镜像断言（`23-accessibility-adaptivity.md` 关键结论 12）。
- `ActionId::InterjectPrompt` 的 `long_help` 与 `description` **互相矛盾**（前者说不取消、后者说取消），代码站在 `description` 一边；照抄 `long_help` 会写错键位语义（`actions/defaults.rs:625-654`）。

---

## 7. 对本项目（jev-harness）的启示

### 本节要点

- 值得抄的是**机制**而不是**规模**：单一动作注册表、块是纯函数、按需渲染、不阻塞输入、门控带理由。
- 明确的过度设计（就本项目现状而言）：三形态渲染、自研 inline Terminal fork、1347 个 package 的依赖面、元测试与 PTY e2e 全家桶、远端容错体系。
- 本项目「奥卡姆剃刀 / 模块从简 / 不做过度防御性编程 / 决策分层 / 概率优先」与 Grok 的取舍方向一致，但量级差一到两个数量级——**方向可抄，量级要砍**。
- 终端状态泄漏（raw mode、备用屏、鼠标捕获、子进程继承）是少数值得保留防御的地方。
- 7.3 给一个 5 模块、单 crate、四期的骨架；每条选型都能回溯到专题 26 §7.1 的证据。

### 7.1 可直接借鉴的做法

1. **单一动作注册表 + 编译期动作枚举。** Grok 的键位、命令面板、快捷键手册、遥测共用 `ActionId`（76 变体）与 `default_actions()` 一处注册（`actions/mod.rs:34-36, 240-272`；`actions/defaults.rs:56`）。对本项目的额外好处是**决策分层**天然落在这一层：确定性路由 → Jev 判定 → LLM 决策，三者最终都产出一个 `Action`，测试与遥测同源。

2. **块是纯函数，`output(ctx)` 带显式上下文。** 专题 09 §10 已点名适合本项目：把 Jev 的 Choice 概率 / confidence 放进 `ctx` 或块的 `accent`/`background` 决策里，渲染层就能直接表达「不确定」，无需塌缩成布尔——与「概率优先，不要过早塌缩」同向。

3. **行级元数据一次产出、四处复用**（绘制 / 搜索 / 复制 / 导出）：`Selectable`、`selection_range`、`selection_text`、`joiner`（`types.rs:146-169`）。低成本、高回报的解耦点。

4. **不阻塞输入**：后台 worker + 输入线程零阻塞 + 用户输入同批渲染（2.2 模式 2）。本项目要做 LLM 流式与 Jev 判定并发，这条是硬约束。

5. **等待原因做成类型化枚举。** Grok 有 7 种 `WaitingReason`，短等待用 300 ms 延迟门隐藏以杜绝闪烁（`acp/tracker.rs:48-75, 116-136`）。专题 06 明确把它对应到本项目的 Jev 判定输出：让状态行文案随判定结果切换。

6. **按需渲染 + 空闲零字节 + 整数除法降帧。** 三条件重绘闸门（`dirty` ∧ 无在途帧 ∧ writer 排空）与 `TickDemand::None` 是最直接可抄的两条（`event_loop.rs:579-600`、`app_view.rs:244-257`）；动画统一 `tick / DIVISOR` 降帧，不另设帧预算常量。

7. **能力探测成枚举，`Auto` 是默认值，显式配置只是逃生口**（2.2 模式 5）。本项目只会有两档（色彩能力、是否 TTY），但「业务代码不读环境变量、只读能力位」这条边界值得抄。

8. **门控必须带理由，拒绝要有出路。** 照抄 `Remedy::SwitchMode { why }` 的形状（2.2 模式 8）。本项目将来一定有「Jev 不可用 / 未配 API key / 超限」这类门控，拒绝时同时给出「为什么」与「怎么办」。

9. **决策层与 IO 层分离：dispatch 不碰终端 / 网络 / 文件系统，异步一律描述成 `Effect` 值。** 上游把它写成不变量：`"dispatch fully testable without tokio or a terminal"`（`app/dispatch/mod.rs:1-10`）。本项目是单二进制，这条能让「LLM / Jev 决策路由」脱离终端单测。

10. **两条终端护栏值得保留。** 信号触发的恢复：没有它，SIGINT/SIGTERM/SIGHUP 会把终端留在 raw mode、备用屏、鼠标捕获、Kitty 协议标志全开的状态，用户「看到一个还活着、其实背后进程已死」的 TUI（`signal_handler.rs:3-5`）。子进程隔离：one-shot env 覆盖必须消费掉，否则每个 spawned child 都继承一个用户没要的强制屏幕模式（`screen_mode_relaunch.rs:268-269`）。这与「不做过度防御」不冲突——它们防的是真实故障，不是推测的故障。

### 7.2 需要谨慎或代价过高的做法

1. **三形态渲染（Fullscreen / Inline / Minimal）。** 上游为此付出：`ScreenMode` 三变体、两套 commit pipeline（scrollback-native 的只追加约束见 2.2 模式 6）、声明式模式门控系统、relaunch env 一次性覆盖，以及两个额外 crate（`xai-ratatui-inline`、`xai-grok-pager-minimal`）。本项目只有一个作者、一套终端认知：**先选定 alt-screen 全屏**。专题 26 已有对应判断：「jev-harness 若走 alt-screen 全屏 TUI，用 ratatui 自带 `Terminal` 即可」。

2. **自研 inline Terminal / 自研 textarea。** `xai-ratatui-inline` 是对 ratatui 0.29 `Terminal` 的 fork（`xai-ratatui-inline/NOTICE:1-9`）；`xai-ratatui-textarea` 也不是上游依赖（`Cargo.lock` 无 `tui-textarea`）。专题 26 的建议是「通用需求先用上游评估」，专题 24 则记录拆 crate 的**编译时间收益未找到证据**——拆分的可确证收益只有测试与复用。

3. **依赖面规模。** Grok 的量级：pager 87 条直接依赖、`xai-grok-pager-render` 47 条、`Cargo.lock` 1347 个 package、101 个 workspace member、4 个 vendored crate。专题 26 点名最应先砍的是 Mermaid 栈（`mermaid-to-svg` + `dagre_rust` + `graphlib_rust` + `resvg`/`tiny-skia`/`fontdb`）；`obfstr` / `cryptify` 属分发硬化，个人学习项目不需要。

4. **元测试与 PTY e2e 全家桶。** 价值真实（会失败的约定测试、源码扫描 guard、`alacritty_terminal` 虚拟终端、390 处 `#[ignore]` 闸门），成本也真实（独立 pty-harness crate、Bazel opt-in、按平台维护 JSON 基准）。本项目在做出第一个可用 TUI 之前，先只保留「纯函数单测 + `Buffer` 就地断言」这一档（专题 27 §3–§4）。

5. **远端数据容错体系。** announcements / subscription gate / changelog 的三套容错（字段全 optional、坏输入降级为空、旧格式衰减后隐藏项重新出现一次）。本项目没有远端配置与付费门控，整套不需要；但「单条坏数据不牵连整批」在解析 Jev 响应与流式 JSON 时值得保留。

6. **键位不可重映射。** 在 Grok 可控（有 74 个内置命令与命令面板兜底），在本项目会变成负资产。**保留「单一注册表」的实现方式，但把键位放进配置**——这一点与上游相反，理由是规模而非理念。

7. **同键不同义。** `Ctrl+G`、`Ctrl+L` 在不同模式下表示不同意图（`crates/codegen/xai-grok-pager/README.md:43-44, 56-57`），需要模式门控与提示系统配套才不产生认知负担。在只有单模式之前不要引入。

8. **无界通道式的背压豁免。** 它正确的前提是「消息极小 + 每次唤醒全排空」这两个论证；抄走结论而不抄论证，就是无界泄漏。若本项目要抄，必须把同样的上界论证写进注释。

### 7.3 建议的最小 TUI 骨架

**模块划分：单 crate 内 5 个模块，不拆 workspace**

```
src/
├── app.rs       事件循环 + 应用状态（单一 select! + 循环尾唯一绘制点）
├── actions.rs   ActionId 枚举 + 单一注册表（键 → 动作，带上下文）
├── blocks.rs    块 = 纯函数 output(ctx)；行级元数据（selectable / joiner）
├── render.rs    帧管线 + 布局 + 快捷键栏（不进 app 状态的绘制自由函数）
└── terminal.rs  能力探测成枚举 + 进入 / 退出 / 信号恢复护栏
```
上游拆 crate 的可确证收益只有测试与复用（专题 24）；本项目一个 crate 起步，出现第二个消费者再拆。

**crate 选型（全部取自专题 26 §7.1 的「事实标准」清单，路径见该节）**

- `ratatui` 0.29 + `crossterm` 0.28——上游同一组合（`pager/Cargo.toml:27,29`），生态默认答案。
- `unicode-width` + `unicode-segmentation` + `textwrap`——宽度与折行三件套；宽度一律按显示列度量、宽字符整簇裁剪（专题 03/02）。
- `similar`——diff，0 依赖纯算法库；本项目要显示代码改动时够用。
- `nucleo`——模糊筛选（命令面板 / 文件选择）；**不做模糊匹配就不引入**。
- 暂不引入：`supports-color` / `anstyle`（先写死一套主题）、`syntect`（先不做高亮）、`pulldown-cmark`（先只渲染纯文本 + 代码块）、Mermaid 栈（专题 26 点名最应先砍）。

**分期路线**

1. **P0「能跑」**：单窗格（滚动区 + prompt + 快捷键栏）+ ActionId 注册表 + 按需渲染。不做主题系统、折叠、搜索。验收：块级纯函数有单测，`cargo test` 通过。
2. **P1「不阻塞」**：LLM 流式与 Jev 判定各起一个后台 task，结果经 channel 回事件循环；输入路径上不允许任何阻塞调用。验收：流式输出期间按键延迟不随输出增长。
3. **P2「概率上屏」**：把 Jev 的 Choice 概率 / confidence 放进 `ctx`，用状态行文案或块的 accent 表达「不确定」；不新增可视化组件。
4. **P3「才考虑」**：历史归属（应用内滚动 vs 终端原生滚动）、折叠分组、搜索 daemon。每一项都要先回答「谁来持有历史」，因为只追加约束会反向决定整条 commit pipeline（2.2 模式 6）；若走终端原生滚动，就必须同时接受上游那三条断言（committed frontier、`EntryId` 键控、待输入块绝不提交）。

> **本节限制**：以上选型均来自上游专题文件已有证据，**未做任何本地验证**（没有编译、没有跑过任何 TUI 原型）。crate 版本号是上游锁文件在调研时点的取值（`ratatui 0.29.0`、`crossterm 0.28.1`、`textwrap 0.16.2`、`similar 2.7.0`），本项目落地前须重新核对当前版本。
