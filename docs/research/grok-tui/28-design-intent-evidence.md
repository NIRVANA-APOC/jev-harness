# 专题 28：设计意图与原则的证据采集

> 专题：设计意图与原则的证据采集｜调研日期：2026-09-22｜证据范围：`reference/grok-build/README.md`、`CONTRIBUTING.md`、`crates/codegen/xai-grok-pager/README.md`、`crates/codegen/xai-grok-pager/docs/user-guide/**`（重点 README / 01 / 03 / 04 / 05 / 21）、`crates/codegen/xai-grok-pager/docs/tutorial/**`（全 9 章）、`crates/codegen/xai-grok-pager/src/**`、`crates/codegen/xai-grok-pager-minimal/src/**`、`crates/codegen/xai-ratatui-inline/README.md`、`crates/codegen/xai-grok-pager-render/src/**`、`crates/codegen/xai-grok-announcements/src/lib.rs`、`crates/codegen/xai-grok-shell/CHANGELOG.md` 与 `changelogs/**`、`crates/codegen/xai-grok-tools/THIRD_PARTY_NOTICES.md`。以下路径均省略 `reference/grok-build/` 前缀。

## 0. 证据分级约定（阅读本文前必读）

本文严格区分三类论断，逐条标注：

| 标记 | 含义 |
| --- | --- |
| **【官方陈述】** | 面向用户/贡献者的明示表述：`README`、`docs/user-guide/**`、`docs/tutorial/**`、`CHANGELOG` |
| **【代码注释】** | 源码注释中写明了决策理由的 |
| **【归纳】** | 我从实现中推断出的模式，官方**未**明说；不得当作官方立场引用 |

找不到确证的地方一律写「未找到证据」。**本文最重要的发现是一条前提修正**，见 §1.2。

---

## 1. 官方自述的设计信条

### 1.1 形态定位：TUI 是主线，但不是唯一入口

**【官方陈述】** 仓库根的自我介绍把三种交付形态并列，而不是把交互 TUI 当唯一形态：

> "**Grok Build** is SpaceXAI's terminal-based AI coding agent. It runs as a full-screen TUI that understands your codebase, edits files, executes shell commands, searches the web, and manages long-running tasks — interactively, headlessly for scripting/CI, or embedded in editors via the Agent Client Protocol (ACP)."
> —— `README.md:13-17`

> 「Grok Build 是 SpaceXAI 的终端 AI 编程 agent。它作为全屏 TUI 运行，能理解你的代码库、编辑文件、执行 shell 命令、搜索网页、管理长任务——可交互使用，也可无头运行用于脚本/CI，或通过 ACP 嵌入编辑器。」

`docs/user-guide/01-getting-started.md:5` 复用同一句话。据此，本 TUI 的设计目标**不是**"做一个好用的聊天界面"，而是**同一个 agent 运行时的一层表现层**——这一点在仓库结构上被显式承认：

> "| `crates/codegen/xai-grok-pager` | The TUI: scrollback, prompt, modals, rendering |"
> —— `README.md:100`

### 1.2 关于「内联滚动 vs 全屏备用屏」——前提需要修正

**【官方陈述】** 任务简报假设 Grok Build「选择了终端内联滚动而不是全屏备用屏」。**这个前提与证据相反**。官方从未把内联作为首选形态陈述；事实是**三形态并存、按环境自动降级**：

> "Terminal UI (TUI) for Grok Build. Provides the interactive **full-screen** interface including the scrollback view, prompt input, session management, and all modal dialogs."
> —— `crates/codegen/xai-grok-pager/README.md:3-5`（加粗为本文所加）

> "Grok presents a **full-screen** TUI with two main areas: **Scrollback** … **Prompt** …"
> —— `docs/user-guide/01-getting-started.md:89-92`

三种渲染模式的权威定义在 `ScreenMode` 枚举：

```rust
/// Whether the pager uses the alternate screen (fullscreen) or stays inline.
pub(crate) enum ScreenMode {
    Fullscreen,
    Inline,
    /// Scrollback-native (experimental, `--minimal`): finalized blocks are printed into the
    /// terminal's native scrollback via `insert_before`.
    Minimal,
}
```
—— `crates/codegen/xai-grok-pager/src/app/mod.rs:340-348`

默认策略由两个命名极为克制的 `Auto` 变体说明：

> "Automatic: fullscreen in healthy environments, inline in degraded ones."
> —— `crates/codegen/xai-grok-pager-render/src/appearance/config.rs:679`（`RawAltScreenMode::Auto`）

> "Automatic: fullscreen in plain terminals and normal tmux, inline in tmux control mode and Zellij."
> —— `crates/codegen/xai-grok-pager-render/src/terminal/mod.rs:901`（`AltScreenMode::Auto`）

**【官方陈述】** 面向用户的解释与之一致：

> "Zellij and tmux control mode can limit the alternate screen. Grok normally uses inline mode in those environments. … You can configure `[terminal] alt_screen` in `~/.grok/pager.toml`, or run `grok --no-alt-screen` to confirm inline mode works."
> —— `docs/user-guide/21-terminal-support.md:174-179`

**正确的表述是**：内联是**备用屏不可用或不可靠时的保底渲染目标**，而 scrollback-native（`--minimal`）是一条**实验性的第三条路**。三者关系、以及为什么内联滚动本身仍被当作一等能力（独立 crate `xai-ratatui-inline`、`xai-grok-pager-minimal`），见 §1.3 与 §7 的示意图。

### 1.3 内联滚动为何仍被当作一等能力

**【官方陈述】** `xai-ratatui-inline` 的 README 把"内容进入终端原生 scrollback"直接列为产品特性，而非权宜之计：

> "A Rust library for building terminal applications with inline viewports - dynamic UI elements that stay at the bottom of the terminal while **preserving scrollback history above them**."
> —— `crates/codegen/xai-ratatui-inline/README.md:3`

> "This single implementation works universally across all terminals and multiplexers **without special modes or workarounds**."
> —— `crates/codegen/xai-ratatui-inline/README.md:53`

**【官方陈述】** 同文档里有一条**反直觉的显式取舍**：放弃 ratatui 自带的 resize 处理，改用 RIS（`ESC c`）全清重画：

> "When using inline viewports on the main screen (not alternate screen), terminal resize causes issues: Terminal reflows content automatically BEFORE the app receives SIGWINCH / Old viewport borders get reflowed as garbage text / **Built-in `autoresize()` corrupts scrollback history** / Cursor position queries (DSR) have race conditions during rapid resize"
> —— `crates/codegen/xai-ratatui-inline/README.md:101-106`

> "We use the 'nuclear option' - completely reset and re-render"
> —— `crates/codegen/xai-ratatui-inline/README.md:110`

**【代码注释】** scrollback-native 模式下这条原则被推到极致：

> "In this mode finalized conversation blocks are printed once into the terminal's *native* scrollback. … The interactive `ScrollbackPane` (scroll, fold, selection, mouse) is not used; **the terminal owns history**."
> —— `crates/codegen/xai-grok-pager-minimal/src/lib.rs:3-5`

> "Minimal has no separate history pane, so the terminal's scrollback *is* the history. a resumed session would otherwise look empty."
> —— `crates/codegen/xai-grok-pager-minimal/src/commit.rs:317-318`

**代价被明确记录**：native scrollback 是**只追加**的，这条约束反向决定了整条 commit pipeline 的结构（详见 §3.3）。

### 1.4 键盘优先：官方以「契约」而非口号陈述

**【官方陈述】** 未找到"keyboard-first"这类字面宣称。能找到的是几条把键盘当唯一真理来源的硬约束：

> "Reference for key bindings in the Grok Build TUI. **Bindings are built in and cannot currently be remapped.**"
> —— `docs/user-guide/03-keyboard-shortcuts.md:3`

> "- **Simple mode** (default): Arrow keys for navigation, `Shift+Arrow` for turn navigation, `Space` to focus the prompt, and **any letter key auto-focuses the prompt**."
> —— `docs/user-guide/03-keyboard-shortcuts.md:11`

> "**Esc is not a focus key.** It follows the clear / rewind semantics below, and it never cancels a running turn (`Ctrl+C` does)."
> —— `docs/user-guide/03-keyboard-shortcuts.md:100`

最后一条是**对终端常识的刻意背离**：`Esc` 在多数 TUI 里既退出界面又取消操作，这里把它固定为"清空 / rewind / 退出一级"的单义键，取消操作单独交给 `Ctrl+C`。同一条规则在用户指南与 tutorial 里重复三次（`01-getting-started.md:96`、`02-first-prompt.md:17`、`03-keyboard-shortcuts.md:100`），且代码里有对应的断言测试与用户可见文案：

> "Interrupts the agent's current turn and stops generation, keeping the session open.\nCtrl+C cancels when the prompt is empty; with a non-empty draft it clears the prompt first and leaves the turn running.\n**Esc never cancels a turn; pressed mid-turn it shows a reminder to use Ctrl+C.**"
> —— `crates/codegen/xai-grok-pager/src/actions/defaults.rs:509-511`（`Ctrl+C` 动作的 `long_help`）

**【官方陈述】** 阻塞式交互被设计成"键盘所有权显式转移"的契约：

> "Four surfaces block the agent on your answer and **take over the keyboard** while they are open … When more than one is open the permission prompt has the keyboard first, then the cancel-turn panel, then question, then elicitation — and the shortcuts bar always shows the keys of whichever one is receiving them."
> —— `docs/user-guide/03-keyboard-shortcuts.md:104-109`

> "`Tab` / `Shift+Tab` walk that card's rows and wrap at both ends. **They never move focus out of the card, so the cursor is always somewhere you can see.** … With the keyboard parked, the shortcuts bar shows the scrollback's own keys, and its focus hint names the card rather than the prompt: `Tab/Space: question`. **That hint is pinned, so a narrow bar can never trim away the only route back.**"
> —— `docs/user-guide/03-keyboard-shortcuts.md:111-125`

**鼠标的定位**【官方陈述】：并非被禁，而是**可切换**，因为捕获鼠标会破坏终端原生选择：

> "Toggle terminal mouse reporting (mouse capture)" / "Toggle mouse reporting (native copy/paste)"
> —— `crates/codegen/xai-grok-pager/src/actions/defaults.rs:859-862`

> "// Toggle terminal mouse reporting (mouse capture). Single binding: Ctrl+R on scrollback only, which takes that // chord from OpenSessions while scrollback is focused. **Under Panels (not Essentials): advanced/opt-in only.**"
> —— `crates/codegen/xai-grok-pager/src/actions/defaults.rs:857-858`

### 1.5 不阻塞输入：官方陈述 + 代码双重证据

**【官方陈述】**

> "## Keep typing while Grok works
> While a turn is running, `Enter` **queues** your next message instead of interrupting. Change your mind? Press `Enter` on the empty prompt to stop the current turn and send the queued message right away."
> —— `docs/tutorial/02-first-prompt.md:9-13`

> "The **shortcuts bar** at the bottom always shows the keys relevant to what you're doing right now — when in doubt, look down."
> —— `docs/tutorial/02-first-prompt.md:22-23`

**【官方陈述】** 在跑动期间三种语义被刻意区分，且每条都写明理由：

> "**Plain `Enter`** … **queues** a follow-up for later. … **`Enter` again on the emptied composer** (double-Enter) sends the **top** queued follow-up now."
> —— `docs/user-guide/03-keyboard-shortcuts.md:309-310`

> "**Send-now is intentionally interruptive** — it reads as 'stop what you're doing and take this'. To hand the agent a note **without** stopping it, queue with plain `Enter`."
> —— `docs/user-guide/03-keyboard-shortcuts.md:325`

**【代码注释】** 实现层把"不阻塞输入"落成若干具体机制，见 §3.1。

### 1.6 少即是多 / 渐进式披露

**【官方陈述】** 文档体系本身是三层渐进结构：

> "## Tier 1: Essential User Docs
> **Start here.** These guides cover what you need on your first day."
> —— `docs/user-guide/README.md:7-9`（其后为 `Tier 2: Core Feature Docs` :21、`Tier 3: Advanced Usage Docs` :38）

**【代码注释】** tutorial 与 how-to 的分工是显式设计，并且 `/tutorial` 严格 opt-in：

> "Short, curated topics shown by the `/tutorial` overlay (**strictly opt-in; nothing auto-shows**). **Deliberately separate** from `crate::docs` (the full how-to guides): these pages are bite-size intros that **point at the guides for depth**."
> —— `crates/codegen/xai-grok-pager/src/tutorial_docs.rs:3-4`

**【官方陈述】** 用户可见的"少即是多"建议：

> "Start with `AGENTS.md` and a theme; add the rest when you need it."
> —— `docs/tutorial/08-make-it-yours.md:38`

> "**You don't need to memorize anything:** `/` and `Ctrl+P` will always show you what's available."
> —— `docs/tutorial/05-slash-commands.md:33-34`

> "- `Shift+Tab`（prompt focused）cycles the mode … **A good habit: plan mode for 'how should we even do this?', normal mode for 'just do it'.**"
> —— `docs/tutorial/07-plan-and-permissions.md:22-31`

### 1.7 保守的副作用边界：官方明确"不做"的事

这批"我们故意不做 X"的陈述是本文档中**最接近设计信条**的材料：

> "Grok **deliberately does not** run `tmux source-file` or change the live tmux server. … The conservative config scan checks direct global assignments only; review sourced files, conditionals, plugins, and generated tmux setup yourself."
> —— `docs/user-guide/21-terminal-support.md:43-47`

（同处说明它只改持久化配置文件，保留行尾与权限位、改动前备份、拒绝有歧义的直接赋值：`21-terminal-support.md:38-41`。）

> "Many terminals already reorder right-to-left text themselves (VTE-based terminals, Terminal.app, Konsole, mlterm, and others). Grok Build therefore **does not** reorder RTL by default." / "If text looks correct with the default and becomes wrong after enabling this, turn it back off — your terminal is already handling bidi."
> —— `docs/user-guide/21-terminal-support.md:265-281`

> "Grok advertises only `Shift+Enter` and `Alt+Enter` as newline. **Many terminals bind Cmd+Enter to fullscreen, so `SUPER` is excluded from the newline matcher**, and a delivered `SUPER+Enter` does not match the agent's bare-Enter send binding."
> —— `docs/user-guide/21-terminal-support.md:216-219`

> "`method = \"bel\"` is treated as intentional."（用户显式设置 `bel` 时不再报诊断）
> —— `docs/user-guide/05-configuration.md:542`

**【归纳】** 共同的判断标准是：**当终端/环境已经在做某件事，或某个动作会污染用户环境时，应用侧主动退让**。

---

## 2. 教程里体现的教学式设计

### 2.1 「Coming from another tool」：迁移友好做到配置级

`docs/tutorial/01-coming-from-another-tool.md` 整章就是迁移设计：

> "Coming from Claude, Cursor, or Codex? / **Fear not — your settings, rules, and skills come with you.** Grok Build reads the same project conventions other agents use, and imports the rest."
> —— `docs/tutorial/01-coming-from-another-tool.md:1-4`

自动接管清单（同章 `:8-17`）：`AGENTS.md`（标注为 "the Codex/OpenCode convention"）、`CLAUDE.md`（含嵌套）、`.claude/rules/`、`.cursor/rules/`、`~/.claude/skills/`、`~/.claude/commands/`、`~/.cursor/skills/` 及其项目级对应物、`~/.claude.json`、`.cursor/mcp.json`、`.mcp.json`、`.claude/settings.json` 的 hooks（含 `Bash` 这类 matcher 别名）。

三步曲线（`:19-34`）：`/import-claude`（checkbox 预览后写入 `.grok` 配置）、`/resume-claude` `/resume-codex` `/resume-cursor`、`grok inspect`（"see every rules file, skill, and MCP server Grok picked up, **tagged with where it came from**"），并说明"Each compat source can be toggled in `[compat.claude]` / `[compat.cursor]` config sections"。

代码侧对应：`crates/codegen/xai-grok-pager/src/app/foreign_sessions.rs:206-257`（`ForeignSessionTool::{Claude, Codex, Cursor}`、显示名 `"Claude Code"` / `"Codex"`、skill 名 `resume-codex`、按 `compat.{claude,codex,cursor}` 开关）；`docs/user-guide/01-getting-started.md:259`「Grok also reads `CLAUDE.md` files for compatibility.」

**【归纳】** 迁移友好是"读别人的约定 + 显式导入 + 可审计 + 可关闭"四件套，而不是一次性转换脚本。

### 2.2 `/` 与 `@` 的心智模型如何建立

教程用**符号→语义**的一致映射建立心智模型，并且每个符号都绑定"只在空 prompt / 行首触发"以避免与普通文本冲突：

| 符号 | 语义 | 触发条件（官方原文） | 证据 |
| --- | --- | --- | --- |
| `/` | 命令（动作） | "Type `/` on an empty prompt and a searchable dropdown of commands appears." | `docs/tutorial/05-slash-commands.md:3` |
| `@` | 上下文（文件/行范围/目录） | "Type `@` for a fuzzy file picker — line ranges work too" | `docs/tutorial/03-attach-and-paste.md:6-8` |
| `!` | 直接跑 shell | "Type `!` **on an empty prompt** to run a shell command directly — the output lands in the scrollback where Grok can see it too." | `docs/tutorial/03-attach-and-paste.md:21-24` |
| `#` | 记忆 | "Start a prompt with `#` (or use `/remember`) to save a note for future sessions" | `docs/tutorial/08-make-it-yours.md:26-28` |

`@` 还设计了**逃生口**而非例外对话框：

> "By default it respects `.gitignore` and hides dotfiles. Prefix with `!` to search hidden files: `@!.github` / `@!.env`"
> —— `docs/user-guide/01-getting-started.md:108-113`

**【官方陈述】** `/` 不仅有下拉，还有第二入口以覆盖"记不住"的场景：`Ctrl+P` / `?` 打开命令面板，`Ctrl+.` 打开快捷键速查表（`docs/tutorial/05-slash-commands.md:26-31`）。`/help` 的文档注释说明了它在不同模式下的**角色变化**：

> "In minimal mode there's no always-visible footer of hints, so `/help` is the discoverable entry point (advertised in the status line)."
> —— `crates/codegen/xai-grok-pager/src/slash/commands/help.rs:3`

### 2.3 教学路径本身是被设计过的

**【代码注释】** 章节顺序不是随手排的，源码里写明理由：

> "The tutorial topics, in display order, **as a linear flow** (the topic screen's `→` advances through them). The flow: what carries over from other tools, send a prompt, feed it context, learn the screen, then the bigger features."
> —— `crates/codegen/xai-grok-pager/src/tutorial_docs.rs:30-31`

每章都带一个"更进一步"指针，且该指针被类型系统约束：

> "Title of the primary how-to guide this page's 'Go deeper' points at (**must match a `crate::docs` title**); `d` opens it in the overlay."
> —— `crates/codegen/xai-grok-pager/src/tutorial_docs.rs:15-16`

`/tutorial` 可从任意时刻重开（`docs/tutorial/09-where-next.md:26-28`），即"教程是可回访的参考，不是一次性引导"。

### 2.4 上下文提示（tips）与快捷键栏

**【代码注释】** tips 系统的设计约束：

> "Ephemeral tips: one hint line at a time, rendered in the banner rect above the prompt input and cleared after a TTL. / **Unlike the toast, an ephemeral tip survives typing**: only TTL expiry, prompt-box submission, or an explicit clear removes it. / A tip that carries a seen-count key stops appearing once `AppView::tip_seen_counts` says it has shown often enough this run."
> —— `crates/codegen/xai-grok-pager/src/tips/mod.rs:1-5`

**【代码注释】** 快捷键栏允许把关键提示"钉住"，不让窄屏裁掉：

> "/// When true, the hint survives compact-mode truncation and renders regardless of `max_visible`. /// Use for hints that should be discoverable in every scrollback context (e.g. nav, turn, mode)."
> —— `crates/codegen/xai-grok-pager/src/views/shortcuts_bar.rs:31-34`

**【官方陈述】** 用户可关掉提示，且与诊断建议分离：

> "Turn it off with `/settings` → **Show contextual hints** → **SSH wrap**, or set `ssh_wrap = false` under `[ui.contextual_hints]` in `$GROK_HOME/config.toml`. **This setting does not hide the Doctor recommendation.**"
> —— `docs/user-guide/21-terminal-support.md:156-159`

---

## 3. 代码注释中的设计决策理由（含反直觉选择）

### 3.1 输入路径：把一切移出 UI 线程

> "The daemon runs the regex scan off the input thread. Query mutations only enqueue the latest corpus and query (O(1) on the UI thread). … **This keeps per-keystroke typing responsive on long sessions where a synchronous scan would stall the input thread.**"
> —— `crates/codegen/xai-grok-pager/src/scrollback/search.rs:9-12`

**反直觉选择 1 —— 刻意用无界通道（放弃背压）：**

> "/// Deliberately detached, never joined: **closing a search must not block the UI on an in-flight scan.**"
> —— `crates/codegen/xai-grok-pager/src/scrollback/search.rs:203`

> "// Unbounded so `update_query`'s send never blocks the input thread. // **A bounded channel could stall the 257th keystroke on a full buffer, and dropping on full (try_send) could lose the final query** // Messages are tiny (an Arc pointer and the query) and the daemon drains the whole queue on each wakeup, so it stays short in practice"
> —— `crates/codegen/xai-grok-pager/src/scrollback/search.rs:211-214`

（常规做法是有界通道做背压；这里判定"卡住输入"比"内存增长"不可接受，并用"消息极小 + 每次唤醒全排空"论证上界。）

**反直觉选择 2 —— 宁可丢掉跨批片段，也要立刻渲染：**

> "// User input must render immediately // Carrying only `Bracket` across batches is therefore unnecessary and holds the key until the next keystroke // Deeper partial states (`[<…`) still persist for cross-batch continuation"
> —— `crates/codegen/xai-grok-pager/src/app/csi_filter.rs:109-111`

对应回归测试写明了用户体验代价：

> "/// A typed `[` must be emitted in the same batch, not held until the next keystroke (**which made the cursor look stuck / 'laggy'**)."
> —— `crates/codegen/xai-grok-pager/src/app/csi_filter.rs:374`

**启动期也不丢按键：**

> "/// Keystrokes the user typed while the app was still loading, captured by the post-raw-mode drains. /// Replayed into the composer by `event_loop::run`."
> —— `crates/codegen/xai-grok-pager/src/app/mod.rs:1451-1453`

（配套的清洗规则：`src/app/event_loop.rs:55`「The Esc is non-text, and `filter_startup_typeahead` truncates the batch at the first Esc key」，见 `event_loop.rs:80`。）

**为保光标闪烁而跳过空 diff 渲染：**

> "/// Uses `xai_ratatui_inline::Terminal` instead of stock `ratatui::Terminal`: our `flush()` returns a `bool` saying whether any cells changed. /// This lets `crate::render::draw::draw_frame` skip cursor escape sequences on frames with empty diffs (e.g., off-screen animation ticks). /// **Skipping them preserves the cursor blink timer**"
> —— `crates/codegen/xai-grok-pager/src/app/mod.rs:333-336`

### 3.2 终端状态：最小占用、可恢复、别影响子进程

> "Without these, signal-triggered termination (SIGINT, SIGTERM, SIGHUP from WSL/SSH disconnect) leaves the terminal in raw mode. The alternate screen stays active, mouse capture stays on, and Kitty keyboard protocol flags stay pushed. The next time the terminal becomes visible **the user sees a stale snapshot of the TUI that looks alive but is backed by a dead process.**"
> —— `crates/codegen/xai-grok-pager/src/app/signal_handler.rs:3-5`

**反直觉选择 3 —— 故意不处理 SIGPIPE：**

> "SIGPIPE is **intentionally left alone**. The disposition is `SIG_IGN` (Rust's stdlib default), so writes to a closed pipe return `Err(BrokenPipe)` instead of terminating the process. … **Installing a custom handler would change the disposition inherited by fork+exec children (MCP servers, hooks) from `SIG_IGN` to `SIG_DFL`. That re-introduces a prior SIGPIPE regression.**"
> —— `crates/codegen/xai-grok-pager/src/app/signal_handler.rs:12-16`

（这是本文档里"为什么不用显然方案"写得最完整的一条：显然方案是装 handler，但被"影响子进程继承的 disposition"否决。）

**渲染模式与终端类型强绑定的断言：**

> "/// Minimal's `insert_before` / `set_viewport_height` commit pipeline is a no-op on the `Viewport::Fixed` fallback, **so it cannot function there.**"
> —— `crates/codegen/xai-grok-pager/src/app/mod.rs:1457-1459`

**配置与子进程隔离：**

> "/// Consume the one-shot screen-mode override env (see `GROK_SCREEN_MODE_ENV`). /// **Every spawned child (tool shells, workers, nested `grok` invocations) would otherwise inherit a forced screen mode the user never asked for.**"
> —— `crates/codegen/xai-grok-pager/src/app/screen_mode_relaunch.rs:268-269`

### 3.3 native scrollback 的"只追加"约束如何反向决定架构

**这是内联/滚动回模式中最硬的结构性约束，也是本文档对自研 TUI 最有复用价值的一条。**

> "/// Add a finalized block positioned immediately before the entry `anchor`, instead of at the end. **`anchor` must not already be committed. A terminal's native scrollback is append-only. Inserting above a block already printed there would emit the new block below content that logically follows it.**"
> —— `crates/codegen/xai-grok-pager/src/scrollback/state/mod.rs:592-595`

代码用 `debug_assert!` 把这条约束固化：

```rust
debug_assert!(
    !self.committed.contains(&anchor),
    "insert_block_before: anchor {anchor:?} is already committed — the inserted \
     block would print out of order in native scrollback"
);
```
—— `crates/codegen/xai-grok-pager/src/scrollback/state/mod.rs:596-602`

由此派生的两个设计：
- **committed frontier**（已打印条目的连续前缀）：`crates/codegen/xai-grok-pager-minimal/src/commit.rs:5-7`
- **已提交集合用 `EntryId` 而非位置索引**：`"Keyed by `EntryId` (not a per-entry flag) so it survives `shift_remove` / `remove_from` reordering for free"`（`src/scrollback/state/mod.rs:75-76`）

**反直觉选择 4 —— 等待用户输入的块绝不提交，即使内容"看起来已定稿"：**

> "// A block awaiting user input (permission / ask_user_question) holds the frontier in EVERY turn state, not just mid-turn // **Its rendered form still changes when the prompt resolves, so committing it (print-once) would freeze the 'waiting' form on the terminal**"
> —— `crates/codegen/xai-grok-pager-minimal/src/commit.rs:27-29`

**提交前后高度必须一致，否则提示框会抖：**

> "/// Whatever the value, both sides of the commit frontier must apply it identically (`super::live::draw_tail`, `super::live::tail_height`). **Otherwise a block's height would change as it moves into native scrollback and the prompt would shift on every commit.**"
> —— `crates/codegen/xai-grok-pager-minimal/src/commit.rs:14-16`

**同一原则的排版侧表述：**

> "/// That keeps a block's height identical on either side of the commit frontier, **so the prompt does not jerk** when it crosses."
> —— `crates/codegen/xai-grok-pager-minimal/src/commit.rs:69`

### 3.4 模式门控必须带理由，而不是静默隐藏

**这是"渐进式披露"在工程上的正面实现，很值得自研 TUI 借用。**

```rust
/// What to tell a user who typed a command the current mode cannot run.
pub enum Remedy {
    SwitchMode {
        /// Sentence fragment, parenthesized in the refusal: `"minimal is single-session"`.
        why: &'static str,
    },
    /// Imperative clause naming what to do in this mode instead.
    UseInstead(&'static str),
    AlreadyInMode,
}
```
—— `crates/codegen/xai-grok-pager/src/slash/mode_support.rs:3-15`

拒绝文案模板（`:45-49`）：

```rust
"/{token} isn't available in {current} mode ({why}). Run {switch} to switch this session."
```

实例如 `crates/codegen/xai-grok-pager/src/slash/commands/jump.rs:10-15`：

```rust
mode_support: ModeSupport::FullscreenOnly(Remedy::SwitchMode {
    why: "minimal scrolls with your terminal's native scrollback",
}),
```

**同一哲学的另一种形式**：`Remedy::UseInstead` 被用于"`Ctrl+G` 在 minimal 是外部编辑器、在别处是 tasks pane"这类**同键不同义**的情形（`mode_support.rs:10-14` 的注释把这条讲清楚了）。

### 3.5 其他有价值的理由注释

| 判断 | 原文（节选） | 位置 |
| --- | --- | --- |
| 宁可挡住用户，也不误闪付费墙 | "Gates from cached auth meta … can be stale: the user may have subscribed since the snapshot was computed. Painting such a gate directly **flashes a paywall at a paying user** … **On check failure or timeout we err on blocking.**" | `src/app/subscription.rs:6-9` |
| 超时故意给宽 | "/// This is a safety net for a hung ACP round-trip only … **Generous on purpose**: the check can chain a `/user` fetch, a JWT refresh, and a settings re-fetch." | `src/app/subscription.rs:32-35` |
| 远端配置要有下限 | "/// Floor for the server-supplied cadence: **a fat-fingered remote settings value must not make the whole fleet poll rapidly.**" | `src/app/subscription.rs:21-22` |
| 别用 per-frame 剪枝 | "/// Call this from real update paths only: **a per-frame prune would churn on transient list states.**" | `crates/codegen/xai-grok-announcements/src/lib.rs:113-114` |
| 行为继承旧名以免连锁改动 | "// Name predates the method rename to `.../update`; **renaming would churn the pager consumer.**" | `crates/codegen/xai-grok-announcements/src/lib.rs:58` |
| 屏幕刷新策略要单点决策 | "/// **The opening marker here and the closing marker of the live frame must be decided by one synchronization policy on one terminal context.**" | `crates/codegen/xai-grok-pager-minimal/src/lib.rs:41-42` |
| 实验特性的隔离边界 | "/// If you don't work on minimal, **treat this variant as opaque**; the fullscreen/inline paths are unaffected." | `src/app/mod.rs:346-347` |
| 超时兜底而非阻塞 | "/// Boundaries never follow symlinks and stop at `Volume` … **since descending into an unresponsive network mount blocks past any timeout.**" | `src/fs_size.rs:3` |

---

## 4. 版本变更 / 公告中体现的设计演进

### 4.1 CHANGELOG 的演进轨迹

CHANGELOG 规模本身说明项目处于快速迭代：`crates/codegen/xai-grok-shell/CHANGELOG.md` 2817 行，`changelogs/` 下 318 个按版本拆分的 md/json。要点：

| 版本 | 日期 | 条目 | 设计含义 |
| --- | --- | --- | --- |
| 0.2.92 | 2026-07-08 | "**/minimal** and **/fullscreen** commands let you switch the current session between minimal and fullscreen modes." | 双模式从"启动时选一次"变成**会话内可切换**（`CHANGELOG.md:1513`） |
| 0.2.96 | 2026-07-10 | "**JetBrains terminals on Windows now default to minimal mode to avoid raw mouse-report leaks in the prompt.**" | **按终端厂商缺陷自动降级形态**的实例，与 §1.2 的 `Auto` 策略一致（`CHANGELOG.md:1416`） |
| 0.2.116 | 2026-07-30 | "**Slash commands** are now correctly hidden or refused in minimal or fullscreen mode based on their declared support." | §3.4 的声明式模式门控进入正式形态（`CHANGELOG.md:936`） |
| 0.2.117 | 2026-07-30 | "**Terminal resize** is much faster on long conversations in fullscreen mode." | 与 `xai-ratatui-inline` 的 RIS 重画策略同源问题域（`CHANGELOG.md:927`） |
| 最新 1.0.38 | 2026-09-19 | 见 `CHANGELOG.md:3` | 仍在修 TUI 细节（长回复不被截断、权限提示长参数换行、子 agent overlay 不卡在 "Cancelling"） |

**【官方陈述】** changelog 是**双格式 CDN 分发**，两种格式服务两个 UI 位置：

> "`/release-notes` uses `changelog.markdown` for rich scrollback display / The welcome screen uses `changelog.entries` for bullet rendering"
> —— `crates/codegen/xai-grok-shell-base/src/util/changelog.rs:7-9`

> "Each format is fetched independently with its own cache file, **so a failure in one doesn't block the other.**"
> —— `crates/codegen/xai-grok-shell-base/src/util/changelog.rs:41-42`

> "All fields use `#[serde(default)]` **so a single malformed entry doesn't kill the entire array parse.**"
> —— `crates/codegen/xai-grok-shell-base/src/util/changelog.rs:22-23`

**【归纳】** 整体风格是**容错优先**：远端 JSON、公告、changelog 一律"字段全 optional、坏输入降级为空、绝不因为远端数据让客户端崩"。

### 4.2 公告（announcements）的设计

**【代码注释】** 位置只有一个，且优先级固定：

> "Session announcement banner: **one slot, critical always wins over promo.**"
> —— `crates/codegen/xai-grok-pager/src/views/announcements.rs:1`

同处 `:3-24` 给出两种布局的行级契约（critical 恒 2 行、promo 1 行、`dismissible: false` 时抑制所有 hide 入口并回收预留列宽）。隐藏状态存 `~/.grok/announcements.json`，无 id 的条目用"标题+`\x1f`+正文"派生的内容键兜底（`crates/codegen/xai-grok-announcements/src/lib.rs:75-77`），且**旧格式的 `{"hidden": bool}` 会衰减为空集、公告重新出现一次**（`:90-91`、测试 `:337-342`）。启动时 `GROK_ANNOUNCEMENTS_OVERRIDE`（JSON）优先于远端（`:199-200`）。

---

## 5. 与同类 TUI 的取舍说明

**结论：未找到证据**表明 Grok Build 在 TUI 设计层面点名过 codex / opencode / claude code 并做取舍对比。我用 `rg -i 'codex|opencode|claude code'` 扫过 `crates/codegen/xai-grok-pager/src`，命中的全部是以下三类，**没有一条是 UI 层面的设计对照**：

1. **工具实现的源码移植**【官方陈述】：
   > "The tool implementations under `src/implementations/codex/` (`apply_patch`, `grep_files`, `list_dir`, `read_file`) are ported from the [openai/codex] project" / "The tool implementations under `src/implementations/opencode/` (`bash`, `edit`, `glob`, `grep`, `read`, `skill`, `todowrite`, `write`) are ported from the [sst/opencode] project"
   > —— `crates/codegen/xai-grok-tools/THIRD_PARTY_NOTICES.md:17-31`

   这是**"借用"而非"对照"**：Grok Build 直接采用竞品的工具语义约定。

2. **配置与会话兼容**（迁移友好，见 §2.1）：`src/app/foreign_sessions.rs:206-257`。

3. **内置 agent 类型名**：`src/views/agents_modal.rs:270` 的枚举注释列出 `GrokBuildConcise, GrokBuildPlan, GrokBuildPlanNoSubagents, GrokBuildAskUser, Codex, Opencode, CursorExtended, GrokBuildOrchestrator` —— 说明"外来的 agent 配置形态"被当作一等输入。

**【归纳】** 立场是**兼容并收**（读它们的配置、沿用它们的工具实现），而不是在交互设计上做显式对照。这一点与 `docs/tutorial/01-coming-from-another-tool.md:1-4` 的语气完全一致。

---

## 6. 【归纳】从实现中读出的模式（官方未明说，勿当官方立场引用）

1. **焦点单一所有权**：任何时刻键盘只属于一个 surface，且转移必须显式（`Tab`/`Shift+Tab`/`Esc` 的层级语义）。证据：`docs/user-guide/03-keyboard-shortcuts.md:100-125`。
2. **同名动作在不同模式下用同一个键表达同一个意图，冲突时改绑而非改语义**（`Ctrl+G`：tasks pane ↔ 外部编辑器；`Ctrl+L`：插件 modal ↔ mid-turn interject）。证据：`crates/codegen/xai-grok-pager/README.md:43-44, 56-57`。
3. **渲染形态按环境自适应，而不是让用户先做选择**：`Auto` 是默认值，显式配置是逃生口。证据：`xai-grok-pager-render/src/appearance/config.rs:679`、`terminal/mod.rs:901`。
4. **可发现性分层**：常驻 footer（全屏）→ 状态栏广告（minimal）→ 命令面板 → 教程 → how-to 文档，逐层变重。证据：`src/slash/commands/help.rs:3`、`src/tutorial_docs.rs:3-4`、`docs/user-guide/README.md:7-41`。
5. **提示（tip/announcement）与诊断（doctor）严格分工**：关掉提示不隐藏诊断。证据：`docs/user-guide/21-terminal-support.md:156-159`、`src/views/announcements.rs:1`。
6. **远端数据一律容错**：字段 optional、坏输入降级、单条坏数据不牵连整批。证据：`src/app/subscription.rs:6-11`、`xai-grok-shell-base/src/util/changelog.rs:22-23`、`xai-grok-announcements/src/lib.rs:90-101`。

---

## 7. ASCII 示意图

### 图 1：三种渲染模式的关系（非并列互斥，而是有默认与降级次序）

```
                ScreenMode  (src/app/mod.rs:340-348)
                ┌──────────────────────────────────────────────┐
                │                                              │
   [默认，健康环境]│  Fullscreen   alt-screen（备用屏）           │
                │     ├─ 用户可见 shape：整块替换终端画面      │
                │     └─ 策略来源：AltScreenMode::Always / Auto │
                │                                              │
   [降级，环境受限]│  Inline       --no-alt-screen               │
                │     ├─ tmux control mode / Zellij → 自动选中 │
                │     └─ 仍走 ScrollbackPane（应用内滚动条、   │
                │        折叠、选择、鼠标）                    │
                │                                              │
   [实验，第三条路]│  Minimal      --minimal / /minimal          │
                │     ├─ 定稿块一次性打印进 native scrollback  │
                │     ├─ "the terminal owns history"           │
                │     └─ 只保留 pinned live region + 最少命令  │
                └──────────────────────────────────────────────┘
   优先级：env override(GROK_SCREEN_MODE_ENV) > minimal flag/config > alt-screen policy
           （src/app/screen_mode_relaunch.rs:303-320）
   持久化：/settings → Default screen mode → [ui] screen_mode（重启生效）
           /minimal //fullscreen 只切当前会话（src/settings/defs.rs:495-507）
```

### 图 2：布局层次与两种滚动归属的对比

```
【Fullscreen / Inline —— 应用拥有历史】
┌──────────────────────────────────────────────────────────┐
│ ScrollbackPane（应用内滚动：选择/折叠/鼠标/shift 翻页）    │  ← Tab 可聚焦
│   ▸ user prompt（sticky header）                          │
│   ▾ agent message（markdown + 高亮）                      │
│   ▸ thinking（可折叠）                                    │
│   ▸ tool call（inline diff / 命令输出）                   │
├──────────────────────────────────────────────────────────┤
│ [todos pane Ctrl+T] [tasks pane Ctrl+G]  ← 按需滑入侧栏   │
├──────────────────────────────────────────────────────────┤
│ Prompt（composer；`/` `@` `!` `#` 前缀触发各自下拉）       │
├──────────────────────────────────────────────────────────┤
│ Shortcuts bar（本上下文相关键位；关键提示 pinned 不被裁） │  ← "when in doubt, look down"
└──────────────────────────────────────────────────────────┘

【Minimal —— 终端拥有历史】
   …（上方全部是终端自己的 scrollback，可用终端原生滚动/搜索/复制）…
   定稿块已按 committed frontier 一次性打印 ↑
┌──────────────────────────────────────────────────────────┐
│ pinned live region（minimal_live_rows，默认 10 行）        │
│   · running-turn 状态 / todos / /btw 侧panel              │
│   · Prompt                                                │
│   · minimal 状态行（含 /help 广告位）                      │
└──────────────────────────────────────────────────────────┘

交叉点：无论哪种模式，单条目都能进"全屏 viewer"细看
        （docs/user-guide/01-getting-started.md:145、docs/tutorial/04-navigation.md:20）
```

---

## 关键结论（供合稿引用）

1. **官方没有"选内联滚动而非全屏备用屏"这条信条**；默认形态是全屏 alt-screen，内联是备用屏不可用时的降级目标，另有实验性 scrollback-native 模式，三者由 `ScreenMode` 并存（`crates/codegen/xai-grok-pager/src/app/mod.rs:340-348`、`xai-grok-pager-render/src/appearance/config.rs:679`、`docs/user-guide/21-terminal-support.md:174-179`）。
2. **形态选择自动化**：`Auto` 是默认值，按 `tmux control mode` / Zellij / 具体终端缺陷自动降级，显式配置只作为逃生口（`xai-grok-pager-render/src/terminal/mod.rs:901`、`CHANGELOG.md:1416`）。
3. **内联滚动的核心价值被写进独立 crate 的特性清单**："Content above the viewport becomes part of the terminal's native scrollback"，且为此放弃了 ratatui 自带的 `autoresize()`，改用 RIS 全清重画（`crates/codegen/xai-ratatui-inline/README.md:3, 101-110`）。
4. **native scrollback 只追加**是内联/滚动回模式最硬的结构性约束，直接推导出 committed frontier、`EntryId` 键控与"待输入块绝不提交"三件事（`src/scrollback/state/mod.rs:592-602`、`xai-grok-pager-minimal/src/commit.rs:5-7, 27-29`）。
5. **"不阻塞输入"被落实为代码级机制**：搜索在后台 daemon、无界通道（明确否决有界/`try_send`）、CSI 片段宁可丢弃也要同批渲染、启动期按键捕获回放（`src/scrollback/search.rs:12, 203, 211-214`、`src/app/csi_filter.rs:109-111, 374`、`src/app/mod.rs:1451-1453`）。
6. **键盘优先表现为契约而非口号**：键位不可重映射、`Esc` 被明确规定为"非焦点键且永不取消回合"、阻塞卡片拥有显式键盘所有权与钉住的返回提示（`docs/user-guide/03-keyboard-shortcuts.md:3, 100, 104-125`、`src/actions/defaults.rs:509-511`）。
7. **模式门控必须带理由**：不可用命令不是静默隐藏，而是返回带 `why` 的拒绝文案并指出切换命令（`src/slash/mode_support.rs:3-15, 45-49`、`src/slash/commands/jump.rs:10-15`）。
8. **渐进式披露是分层的**：`/tutorial` 严格 opt-in 且与 how-to 分离，how-to 分三档；minimal 模式因为没有常驻 footer，`/help` 被提升为可发现入口并在状态行广告（`src/tutorial_docs.rs:3-4, 30-31`、`docs/user-guide/README.md:7-41`、`src/slash/commands/help.rs:3`）。
9. **迁移友好做到配置级**：自动读取 `AGENTS.md`/`CLAUDE.md`/`.claude/**`/`.cursor/**`，提供 `/import-claude`、`/resume-{claude,codex,cursor}` 与 `grok inspect`，且每个来源可开关（`docs/tutorial/01-coming-from-another-tool.md:1-34`、`src/app/foreign_sessions.rs:206-257`）。
10. **保守的副作用边界**是明确陈述的原则：不运行 `tmux source-file`、不改动用户 live tmux server、RTL 默认不重排、`SUPER+Enter` 不参与换行匹配（`docs/user-guide/21-terminal-support.md:43-47, 216-219, 265-269`）。
11. **反直觉取舍都有注释留痕**：SIGPIPE 故意不处理（怕影响 fork+exec 子进程）、订阅门控超时宁可阻塞、搜索用无界通道、保光标闪烁而跳过空 diff 渲染（`src/app/signal_handler.rs:12-16`、`src/app/subscription.rs:6-9`、`src/scrollback/search.rs:211-214`、`src/app/mod.rs:333-336`）。
12. **与同类 TUI 的关系是"兼容并收"而非设计对照**：只找到工具实现移植（openai/codex、sst/opencode）与配置/会话兼容，**未找到** UI 设计层面的正面比较（`crates/codegen/xai-grok-tools/THIRD_PARTY_NOTICES.md:17-31`、`src/views/agents_modal.rs:270`）。
