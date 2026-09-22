# 专题 12：Grok Build 的状态显示面

> 专题：状态面——status bar / status line / context bar｜调研日期：2026-09-22｜证据范围：`crates/codegen/xai-grok-status-line/src/{lib,config,context}.rs`、`xai-grok-pager/src/app/{status_line,status_line_policy,status_blocks}.rs`、`src/app/status_line/{command,metrics}.rs`、`src/views/{status_bar,agent_status,context_bar,credit_bar,shortcuts_bar,turn_status,agent}.rs`、`src/views/status_line/{mod,sanitize,segments}.rs`、`src/scrollback/blocks/context_info.rs`、`src/acp/mod.rs`、`src/acp/tracker.rs`、`src/app/agent_view/render.rs`、`src/actions/defaults.rs`、`xai-grok-pager-minimal/src/live.rs`、`xai-grok-shell/src/session/acp_session_impl/status_line.rs`、`xai-grok-shell/src/extensions/notification.rs`、`xai-grok-shell/src/agent/config.rs`、`xai-grok-config/src/config_override.rs`、`xai-grok-pager/docs/user-guide/{24,25,26}`（24 为外部 OTel，本专题仅用于排除误引）

## 1. 一共有几个状态面

### 1.1 纵向排布（fullscreen）

fullscreen 的 agent view 由 `AgentViewLayout::compute` 一次性把整屏切成若干行（`crates/codegen/xai-grok-pager/src/views/agent.rs:181-381`）。自上而下：

```
┌────────────────────────────────────────────────────────────────────────────┐
│  ① status_bar（固定 1 行，Constraint::Length(1)，agent.rs:229）            │
│     左：分支 / 工作树徽标 / sandbox profile + 标题 + cwd 路径（render.rs:1403-1475）
│     右：AgentStatusBar 芯片组，右对齐，三列宽 " │ " 分隔（agent_status.rs:89-130）
│     [可选] Upgrade CTA 按钮，挤在左侧预算里（render.rs:1485-1506）         │
├────────────────────────────────────────────────────────────────────────────┤
│  [可选] tasks / catalog / todo 面板（各自前面带一个 pane_gap 行）          │
├────────────────────────────────────────────────────────────────────────────┤
│  status_gap（top_vpad>0 时为 1 行，agent.rs:244-245）                      │
├────────────────────────────────────────────────────────────────────────────┤
│  ② scrollback（Constraint::Min(5)，全屏唯一的 Min，agent.rs:246）          │
├────────────────────────────────────────────────────────────────────────────┤
│  [可选] /btw 面板、queue 面板、③ turn_status 行（1 行）、banner、           │
│  plugin CTA 行、follow-up 提示行、dock 面板 —— 每项前置 1 行间隔           │
├────────────────────────────────────────────────────────────────────────────┤
│  prompt_gap（0/1 行）→ [voice_recording 1 行]                              │
├────────────────────────────────────────────────────────────────────────────┤
│  prompt（N 行）                                                            │
├────────────────────────────────────────────────────────────────────────────┤
│  shortcuts_gap（仅当 bottom_vpad>0 且 status line 关闭时为 1 行）          │
├────────────────────────────────────────────────────────────────────────────┤
│  ④ status_line（0–5 行，可配置，默认 0 行；agent.rs:295-297）              │
├────────────────────────────────────────────────────────────────────────────┤
│  ⑤ shortcuts（固定 1 行，agent.rs:298）                                    │
└────────────────────────────────────────────────────────────────────────────┘
```

`status_line_height` 在压栈前先被夹一次：`status_line_height.min(inner_area.height - reserved)`，其中 `reserved` 是"其他所有行 + shortcuts"。注释给出的理由很直白：**一个太长的脚本应该丢自己的行，而不是让 prompt 或 shortcuts 丢行**（`agent.rs:134-135, 289-290`）。

### 1.2 状态面对照表

| # | 名称 | 屏幕位置 | 内容项 | 可配置性 |
| --- | --- | --- | --- | --- |
| ① | status bar（芯片行） | 最顶行，固定 1 行 | `bg_tasks` / `plan` / `goal` / `mcp` / `workspace_mode` / `context` / `switcher` / `dashboard` / `link_url`（`render.rs:1268-1381`）；左侧另画 branch/worktree/sandbox/cwd 与 Upgrade CTA | 不由用户配置；由运行时状态决定每个芯片是否 `push` |
| ② | scrollback | 中部，最小 5 行 | 会话流；也承载 `/context` 的 `ContextInfoBlock`、`/usage` 的用量块 | 无 |
| ③ | turn_status（运行态行） | prompt 上方 1 行，按需出现 | spinner + 活动文案 + 阶段计时 + turn 计时 + token 数 + `[stop]`/`[↓]` 按钮 | 无（`should_show` 决定） |
| ④ | status_line | prompt 之下、shortcuts 之上；0–5 行 | `builtin` 六种 item，或用户脚本 stdout（≤5 行） | **唯一用户可配置的状态面**：`[ui.status_line]` |
| ⑤ | shortcuts bar | 最底行，固定 1 行 | 当前 key owner 的快捷键提示；`compact` 模式下截断 | 由 `key_owner()` 与 action registry 决定 |

另有三个**非屏幕行**的状态出口：`/usage`（`app/status_blocks.rs:177-232`）、`/queue`（`status_blocks.rs:14-41`）、`/tasks`（`status_blocks.rs:44-174`），它们是提交进 scrollback 的纯文本块。`app/status_blocks.rs:1-4` 说明动机：minimal 模式没有交互式 pane，这些块是查看该状态的主要途径。

### 1.3 minimal 模式的排布

`xai-grok-pager-minimal/src/live.rs:4` 的模块注释给了顺序：`live tail · todos · /btw · status · prompt · overlay/info · the [ui.status_line] row`。即 status line 落在 prompt info 行**之下**，且 minimal **没有 shortcuts bar**（`live.rs:714` 明确写着 "which minimal does not render"）。

```
 live tail（top）
 todos 面板
 /btw 面板
 status（1 行，render_minimal_status）
 prompt
 overlay / info 行          ← render_prompt_info，含 usage warning
 ─────────────────────────
 [ui.status_line] 行（0–N 行，live.rs:404-416）
```

minimal 里同样先夹 status line 高度：`sl_h = frame.height().min(area.height - status_h - 1)`（`live.rs:167-169`，非模态路径 `live.rs:241-243`）。

## 2. status line 的契约

契约被拆成两个 crate 级模块，`xai-grok-status-line/src/lib.rs:1-4` 说得很清楚：`config` 是用户在 `[ui.status_line]` 里写的，`context` 是 agent 发给客户端去画的东西；这是个 leaf crate，位于 agent 和所有客户端之上。

### 2.1 用户写的一半

`StatusLineConfig` 的字段（`xai-grok-status-line/src/config.rs:18-34`）：

| 键 | 类型 | 默认 | 约束 | 证据 |
| --- | --- | --- | --- | --- |
| `type` | `builtin` / `command` / `disabled` | `disabled` | `off` / `none` / `hidden` 是 `disabled` 的别名；ASCII 大小写不敏感 | `config.rs:288-299, 306-308` |
| `command` | string | 无 | `type = "command"` 时必填（空白串视为未填） | `config.rs:217-221` |
| `items` | array | `["cwd","model","context"]` | 逐项解析，非法项被记录而非整表拒绝 | `config.rs:145-149, 111-123` |
| `padding` | u16 | `0` | 上限 16（`MAX_PADDING_PER_SIDE`） | `config.rs:156, 185-187` |
| `refresh_interval` | u64 秒 | 未设 | 钳制到 1–86400；**只对 `command` 生效** | `config.rs:151-154, 174-183, 242-244` |

内置 item 共 6 个（`config.rs:325-332`），kebab-case：

| item | 显示 | 取不到数据时 | 证据 |
| --- | --- | --- | --- |
| `cwd` | 当前目录 basename，列上限 40 | 无路径段则整段消失 | `segments.rs:75-78` |
| `model` | 模型显示名，列上限 30 | `display_name` 空/缺失则消失 | `segments.rs:79-86` |
| `context` | `{pct}% ctx`，pct ≥ 阈值时用 Warn 色 | `used_percentage` 缺失则消失 | `segments.rs:87-99` |
| `cost` | `$X.XX`，**低于 $0.005 不显示** | 成本缺失则消失 | `segments.rs:100-104` |
| `turn-timer` | 运行中回合的已过时间，`secs == 0` 不显示 | 回合未运行则消失 | `segments.rs:105-111` |
| `session-name` | 会话名，列上限 40 | client 未填则消失 | `segments.rs:112-115` |

段间分隔符是 `SEGMENT_SEPARATOR = " │ "`（`segments.rs:9`），`builtin` 行永远只占 1 行（`views/status_line/mod.rs:158`）。

### 2.2 agent 填的一半

`StatusLineContext`（`xai-grok-status-line/src/context.rs:16-46`），schema 版本常量 `STATUS_LINE_SCHEMA_VERSION = 1`（`context.rs:14`）。注释给了两条硬规则（`context.rs:5-8`）：Grok 取不到的值是 `None` 而不是 `0`；字段一律 snake_case，即使这会打破该仓库别处的 camelCase 约定，因为改名会静默打断所有读它的脚本。

字段分组（同一 crate 内每个字段都有类型级注释）：

- 顶层：`schema_version` / `cwd` / `session_id` / `session_name` / `prompt_id` / `transcript_path` / `model` / `workspace` / `version` / `cost` / `context_window` / `effort` / `worktree` / `turn` / `trigger`（`context.rs:18-46`）。
- `session_name` **由客户端填**（名字本地可改），出现在 command 行的 stdin 上，不出现在 `SessionStatus` 通知里（`context.rs:25-28`）；客户端侧唯一来源是 `ClientOwnedFields { session_name }`（`app/status_line.rs:49-52`，pager 从 `agent.display_name` 或 `generated_session_title` 取，`app/status_line_policy.rs:249-259`）。
- `trigger` 同样是客户端盖章的，且**是单次运行的属性**，所以放在 staleness 输入之外（`app/status_line.rs:46-48`）。
- `cost.total_duration_ms` 的时钟从**本进程挂载会话**开始，不是会话创建时（`context.rs:141`）。
- `context_window.session_usage` 是整会话累计；`context_tokens` 是"当前窗口占用"（`context.rs:154-161`）。
- `StatusLineSessionUsage` 的 `input_tokens` 与 cache 桶**不重叠**，三者相加不重复计数（`context.rs:170-178`）。
- `turn.started_at_ms` 是 Unix 毫秒，"客户端自己减"（`context.rs:80-83`）。

生产侧组装点是 `xai-grok-shell/src/session/acp_session_impl/status_line.rs:117-207` 的 `build_status_context`：模型/窗口来自 sampling config、token 来自 `try_get_estimated_total_tokens`、成本来自 usage ledger、git 信息来自 `git2::Repository::discover`（在 blocking 池上跑）。注释点名了一个刻意的选择：读失败就留 `None`，"which renders as `0% ctx`"（`status_line.rs:134-138`）。

下发方式是**瞬态通知** `XaiSessionUpdate::SessionStatus`（`xai-grok-shell/src/extensions/notification.rs:669`），由 `run_status_emitter` 单循环驱动、`Notify` 把突发合并成一次构建（`status_line.rs:275-336`）。

### 2.3 模板/变量展开：不存在

代码里**没有模板引擎**，也没有 `${...}` 占位符。表达力只有两条路：

1. `type = "builtin"`：从 6 个枚举 item 里挑一个顺序，段与段之间用固定分隔符拼（`segments.rs:67-118`）。
2. `type = "command"`：把 payload 以 JSON 写进脚本 stdin，取它的 stdout（《25-status-line.md》"Command" 节；实现见 `src/app/status_line/command.rs`）。

所以「模板」这件事在 Grok Build 里被整体外包给了用户脚本；配置层只提供枚举 + 顺序。

### 2.4 自定义命令注入：有，且是唯一扩展点

`type = "command"` 就是命令注入，它有明确的边界：

- **能力声明**：客户端在 `initialize` 的 `clientCapabilities._meta` 里报 `x.ai/statusLine`（`xai-grok-status-line/src/lib.rs:13`、`crates/codegen/xai-grok-pager/src/acp/mod.rs:458-460`）；值来自 `ConnectFlags.status_line: bool`（`acp/mod.rs:144`），该布尔量由 `load_initial_ui_config().status_line.reserves_a_row()` 算出（`app/mod.rs:956-958`，配置加载见 `app/event_loop.rs:2948-2960`）。缺省即关闭（`lib.rs:11-13`）。
- **按会话而非按进程**：`CLIENT_STATUS_LINE_META = "clientStatusLine"` 由 leader 注入到 `session/new|load|resume` 的 `_meta`，因为一个 leader 复用多个客户端（`lib.rs:15-17`）。
- **配置层级限制**：`["ui","status_line"]` 整表列入 `PATCH_STRIP_PATHS`，远程/campaign/version-override 补丁不得注入（`xai-grok-config/src/config_override.rs:84-87`），理由是这些表"携带客户端会执行的命令"。测试 `apply_patches_strips_a_remote_status_line_command` 用 `command = "curl evil"` 钉住了这条（`config_override.rs:313-325`）。
- **诊断出口**：配置问题与未知键变成 `config_warnings`（`xai-grok-shell/src/agent/config.rs:2040-2055`），这条路径同时喂给 `grok inspect`（《25-status-line.md》"Troubleshooting" 节）。
- **未找到证据**：手册 §25 说"repo-local `.grok/config.toml` 只读 MCP，`[ui.status_line]` 不在任何 project 作用域层里"。我在 `xai-grok-config/src/{config_layers,loader,validation}.rs` 与 `xai-grok-shell/src/config/` 里**没有找到**实现这一层过滤的常量或分支，只能确证上面的补丁剥离规则。合稿若要引用"仓库无法设置 status line"，需另找证据或只引手册。

### 2.5 生命周期与节流

| 常量 | 值 | 含义 | 证据 |
| --- | --- | --- | --- |
| `EVENT_DEBOUNCE` | 300 ms | 事件驱动重算的最小间隔，防止忙回合每帧跑脚本 | `app/status_line.rs:19` |
| `MIN_REFRESH_INTERVAL_MS` | 100 ms | force 或到期 refresh 降到这个下限 | `app/status_line.rs:21` |
| `ABANDON_AFTER` | 30 s | run 槽位的看门狗；并有 const 断言它 ≥ 2× 命令超时 | `app/status_line.rs:23, 30-33` |
| `REFRESH_FAILURES_TO_PAINT` | 3 | 连续 refresh 失败到这个数才把错误画出来 | `app/status_line.rs:28` |
| `COMMAND_TIMEOUT` | 10 s | 脚本超时 | `app/status_line/command.rs:13` |
| `MAX_COMMAND_OUTPUT_BYTES` | 64 KiB | stdout 上限，超出即截断并停掉脚本 | `command.rs:15` |
| `MAX_STATUS_LINE_LINES` | 5 | 保留的行数上限 | `views/status_line/sanitize.rs:13` |
| `MAX_SANITIZED_CHARS` | 1024 | 每行字符上限（含 ANSI 转义本身） | `sanitize.rs:25` |
| `RowSize::FALLBACK` | 80×1 | 从未绘制过时给脚本的 `COLUMNS`/`LINES` | `views/status_line/mod.rs:206-208` |

run 槽位是一个四态枚举 `RunState::{Idle, Running, Superseded, Abandoned}`（`app/status_line.rs:72-90`），`RunSlot` 对外只暴露 `Free / WithinDeadline / PastDeadline`。设计要点：`trigger` 存在槽位而不是任务结果上，"这样一个结果永远无法声称一个它启动时没有的 trigger"（`app/status_line.rs:62-64`）；interval 从**上一次运行结束**起算，避免慢脚本无间隙连跑（`status_line.rs:366-367`）；supersede 时若原 run 是 refresh 触发的，会把 `refresh_due` 重新挂起（`status_line.rs:406-424`）。

失败处理分两路（`status_line.rs:383-400`）：state 触发的失败**立即**画 `[status line: …]`；refresh 触发的失败在累计到 3 次之前**保留上一次输出**，并把失败写进 `~/.grok/logs/unified.jsonl`（日志等级选择见 `app/status_line_policy.rs:365-387`）。

## 3. 空间不足时谁先被砍

### 3.1 先澄清：policy 文件里没有优先级表

`app/status_line_policy.rs` 管的是**节流与运行生命周期**，不是取舍优先级：`status_line_tick_demand`（`status_line_policy.rs:318-349`）判断要不要 tick，`status_line_work`（`:110-128`）判断这一帧该 Clear / Problem / Builtin / Command，`status_line_frame`（`:280-297`）判断这一帧的 `Off / Reserved / On`。里面没有任何"先砍谁"的排序。

真正的取舍分散在四层，而且顺序是固定的：

```
层级 1  item 列上限 —— 段内截断，不涉及别的段
        cwd 40 / model 30 / session-name 40（segments.rs:14-16）→ fit_columns 尾接 "…"
        ↓ 仍然放不下
层级 2  行内 elide —— 从右往左吃，末尾补 "…"
        elide()（views/status_line/mod.rs:65-102）；link span 同步夹到被保留的列
        ↓ 仍然放不下
层级 3  行数 —— script 的 5 行上限先被配置层砍（sanitize.rs:13）
        再被布局砍：status_line_height.min(inner_area.height - reserved)（views/agent.rs:290）
        ↓ 仍然放不下
层级 4  整面消失 —— ResolvedStatusLine 为 None 或 settled 且无内容 → StatusLineFrame::Off，高度 0
        （status_line_policy.rs:283-296）
```

**关键取舍**：prompt 与 shortcuts 的 1 行是 `Constraint::Length(1)` 写死的，scrollback 是唯一的 `Min(5)`（`views/agent.rs:246, 281, 298`）；status line 的高度是**唯一**会被布局回砍的自愿行。所以空间不足时，status line 先让路，绝不会把 prompt 或 shortcuts 挤掉。

### 3.2 数据缺失时的取舍：整段消失，不占位

`compose_builtin` 用 `filter_map`（`segments.rs:72-117`），所以任何取不到数据的 item 根本不产生段：

- `cost` 低于 `MIN_DISPLAYED_COST_USD = 0.005` 直接不画，"免得显示误导性的 `$0.00`"（`segments.rs:18, 100-104`）。
- `turn-timer` 只在 `secs > 0` 时出现（`segments.rs:106-109`）。
- `context` 缺 `used_percentage` 就消失（`segments.rs:89`）。

### 3.3 脚本行的取舍

- 成功但**什么都没打印**的脚本会让整行消失，而不是回退到内置段（`status_line_policy.rs:293-294`，注释解释了原因）。
- `Reserved` 状态会在首个结果到达前占住 1 行，避免第一份输出把输入框往上顶（`views/status_line/mod.rs:116-118`）。
- 短终端会吃掉多余行，从底部开始丢（《25-status-line.md》"Output" 节 / `mod.rs:188` 的 `take(area.height)`）。

### 3.4 状态栏芯片的取舍：前插项自己先量好宽度

`AgentStatusBar` 的坐标只算一次、永远右对齐（`views/agent_status.rs:89-130`）。唯一的前插接口 `push_front` 必须用 `room_for_front(area_width)` 预量，注释写明理由："它绝不会把已经 push 进去的项挤出这一行"（`views/agent_status.rs:64-77`）。`link_url` 就是这么进来的：先 `room_for_front` 减去 24 列位置地板，再要求 ≥ 12 列才画（`render.rs:1371-1381`）。

## 4. 上下文占用 / token 用量 / 成本

### 4.1 同一件事的三套口径（别混用）

| 口径 | 位置 | 单位/形状 | 阈值与颜色 |
| --- | --- | --- | --- |
| `used / total` 紧凑串 | status bar `context` 芯片 | `8.5K / 1.0M`，宽度 ≤4 字符的 `fmt_tokens` | 连续色带（见下） |
| 同上，hover 时 | status bar `context` 芯片 | 进度条 + `fmt_pct5`（固定 5 字符，`≥100%` 显示 `MAX %`） | 同一套连续色带 |
| `{pct}% ctx` | status line `context` item | 整数百分比 | 二值：`pct ≥ warn_at` 用 `SegmentTone::Warn`，否则 Dim |

（`views/context_bar.rs:16-41, 141-196`；`views/status_line/segments.rs:87-99`。）

`context` 芯片的**宽度不变量**是一处刻意的设计：默认串右侧补齐到最小 6 列（`BAR_PCT_GAP 1 + PCT_WIDTH 5`），这样 hover 切换不会引起布局抖动；bar 宽度 = 默认串宽度 − 6（`views/context_bar.rs:133-141, 165-183`）。

### 4.2 阈值常量

| 常量 | 值 | 用途 | 证据 |
| --- | --- | --- | --- |
| `CONTEXT_WARN_PCT` | 80 | status line `context` 的兜底告警线（agent 没报 auto-compact 阈值时） | `views/status_line/segments.rs:11, 90-97` |
| 色带断点 | 0 / 50 / 65 / 75 / 85 / 95 % | 芯片颜色在断点间线性插值；越往后 text_primary → accent_user → warning → accent_error | `views/context_bar.rs:53-100` |
| `MIN_DISPLAYED_COST_USD` | 0.005 | status line `cost` 的显示地板 | `views/status_line/segments.rs:18` |
| credit 芯片 | 80% warning / 100% accent_error | `credit_bar_line_for_session` | `views/credit_bar.rs:231-255` |
| 余额告警 | `LOW_BALANCE_CENTS = 1000`、`PAY_AS_YOU_GO_CRITICAL_CENTS = 500` | prompt 的 usage warning | `views/credit_bar.rs:141-223` |
| `/context` 提示条 | `(80..auto_compact_threshold)` 区间内才提示 `/compact` | scrollback 块 | `scrollback/blocks/context_info.rs:437-446` |

色带在插值后要 `quantize`（`views/context_bar.rs:176`），保证低色深终端上不会出现非法色；测试 `terminal_theme_meter_keeps_the_terminal_foreground` 钉住了 0–49% 必须留在终端默认前景色（`context_bar.rs:205-227`）。

**未找到证据**：`views/credit_bar.rs:231` 的 `credit_bar_line_for_session` 在本次快照里没有任何生产调用点（只有本文件内部与测试引用），而 `render.rs:1387` 仍在读 `areas.get("credits")`。也就是说 credits 芯片的绘制函数与命中区域都在，但**没有 push 内容的调用**；不要把「状态栏有 Credits used 芯片」当成现状写进合稿。

### 4.3 `/context` 的 100 格分类条

`ContextInfoBlock`（`scrollback/blocks/context_info.rs:18-90`）：条形永远是 **100 格**，两种形状共享同样的 100 格所以视觉份额不变，只有长宽比不同 —— `WIDE` 5 行 × 20 格（约 39 列），`NARROW` 10 行 × 10 格（约 19 列），切换点 `NARROW_BREAKPOINT = 50` 列（`context_info.rs:31-63`）。

```
Context
                                                      ← 空行
8.5K / 1.0M tokens (0.85%)                            ← used/total + 两位小数
grok-4

◆ ◆ ◆ ◆ ◈ ◈ ◇ ◇ ◇ ◇ ◇ ◇ ◇ ◇ ◇ ◇ ◇ ◇ ◇ ◇
◇ ◇ ◇ ◇ ◇ ◇ ◇ ◇ ◇ ◇ ◇ ◇ ◇ ◇ ◇ ◇ ◇ ◇ ◇ ◇     ← 5 行 × 20 格
…（共 100 格，按 legend 顺序左→右填充）

◆ System prompt      1.2K    (0.1%)
◆ Messages           7.3K    (0.7%)
◆ Reasoning/overhead 0.1K    (0.0%)
◇ Free               992K   (99.2%)
                                                       ← 空行
Already counted above
◈ Tool definitions     12 tokens    · 3 tools             ← 只作参考行，不进条
◈ <usage_categories…>  …

Auto-compact at 80% · ~800k tokens remaining           ← 达标时改画 warning 色
Turns: 12 · Tool calls: 34 · Compactions: 1
Tip: run /compact to free up context space.            ← 仅 80% ≤ pct < 阈值
```

### 4.4 关于 `docs/user-guide/24-monitoring-usage.md`

该文件**不是** TUI 内的状态面文档，而是外部 OpenTelemetry 导出指南（主题是 `GROK_EXTERNAL_OTEL` 主开关、`OTEL_*` 环境变量、`grok_code.*` 指标与事件、内容门控与 ZDR），整篇不涉及 status bar / status line / context bar 的渲染。合稿引用「用量监控」时不要把这份手册当作 TUI 呈现的证据。

字形/颜色映射：system 与 messages 共用实心菱形 `◆`（分别 gray_bright / text_primary），tools 用点状菱形 `◈`（accent_skill），free 用空心菱形 `◇`（gray_dim），overhead 用实心菱形（accent_verify）；全部经 `glyphs` 走 CP437 降级（`context_info.rs:248-264`）。列宽由实际渲染行反推（`RowLayout::measure`，`context_info.rs:92-119`），保证 token 数与百分比永远对齐。

## 5. 运行状态的表达

运行态**不在** status bar，也不在 status line，而在独立的 `turn_status` 行（`views/turn_status.rs`）。分派逻辑全在 `compute_activity` 的一个大 match 里（`turn_status.rs:580-698`）：

| `AgentState` | `TurnActivity` | 文案 | 颜色 | 行号 |
| --- | --- | --- | --- | --- |
| `TurnCancelling` / `CommandCancelling` | 任意 | `Cancelling…` | `accent_error` | `:588-592` |
| `TurnRunning` | 任意（goal 校验中） | `Verifying…` | `text_secondary` | `:596-600` |
| `TurnRunning` | `Thinking` | `Thinking…` | `text_secondary` | `:601-605` |
| `TurnRunning` | `Responding` | `Responding…` | `text_secondary` | `:606-610` |
| `TurnRunning` | `ToolRunning{title,description}` | 空文案，交给阶段标签 | Ask 工具或有 description → `text_secondary`；否则 `accent_success` | `:611-626` |
| `TurnRunning` | `AutoCompacting` | `Compacting…` | `text_secondary` | `:627-631` |
| `TurnRunning` | `Retrying{…}` | 重试标签 | `warning` | `:632-650` |
| `TurnRunning` | `WritingToolCall` | `writing.label()` | `text_secondary` | `:651-655` |
| `TurnRunning` | `Waiting(reason)` | `reason.label()` | `text_secondary` | `:656-662` |
| `TurnRunning` | `None`（bash 回合） | `Running…` | `text_secondary` | `:663-668` |
| `TurnRunning` | `None` | `Waiting…`（兜底） | `text_secondary` | `:669-675` |
| `CommandRunning {…}` | 任意 | `{命令名}…` | 工作树/恢复类用 `gray`，其余 `text_secondary` | `:676-695` |
| `Idle` | 任意 | 空 | — | `:696` |

`WaitingReason` 有 7 个变体（`acp/tracker.rs:48-73`，文案在 `:116-136`）：`Model` → `Waiting for response…`、`Subagent` → `Waiting on subagent…` 或子代理名、`TaskOutput` → `Waiting on task output…` 或 `{subject}…`、`TasksComplete` → `Waiting on tasks…`、`Sleep` → `Sleeping…`、`Hooks` → `Running {event} hook…`、`PromptAck` → `Waiting for the agent to accept the prompt…`。设计动机写在 `:68-72`：这些是"视图专用"的等待原因，tracker 自己不存。

**等待用户**有独立视觉信号：当工具有权限提示或 `ask_user_question` 阻塞时，braille spinner 换成脉冲的 `◆`（`turn_status.rs:365-376`），且 "Ask" 类工具会隐藏阶段计时器，"免得用户在回答问题时感到被时间催促"（`turn_status.rs:379-395`）。

行的出现条件：`should_show = parked || !state.is_idle() || drain_blocked || session_starting_since.is_some() || watchers.total() > 0`（`turn_status.rs:725-735`）——即**空闲且无 watcher 时整行消失**，不占屏幕。

**Idle 的其余表达**：空闲时状态栏仍在，`/tasks` 之类芯片靠 `task_status_line` 在 `counts == default()` 时返回 `None` 而消失（`views/agent_status.rs:146-148`）；`goal` 芯片同理只在 `goal_state` 存在时 push（`render.rs:1283-1297`）。所以「空闲」= status bar 只剩 context/cwd 等常驻项 + turn_status 行消失。

## 6. shortcuts bar

### 6.1 三个内容来源

`shortcuts_bar_content` 先问 `key_owner()` 再决定（`app/agent_view/render.rs:277-318`）：

| Content | 何时 | 渲染方式 | 证据 |
| --- | --- | --- | --- |
| `Surface(hints)` | 阻挡式表面拥有键盘：line viewer、block viewer、权限卡、plan approval、cancel-turn 卡、question 卡、MCP elicitation | 原样渲染 | `render.rs:281-315, 3218-3222` |
| `Pane(hints)` | 普通 pane（scrollback / prompt / todo / queue / tasks / dock…） | `.compact(5, help_hint)` 截断后追加 cheatsheet hint | `render.rs:316, 3223-3239` |
| `Hidden` | 该行由别的表面自己画（例如 line viewer 打开时），或 plan-approval hints 为空 | 不画 | `render.rs:319-337` |

hint 集合来自 `views/agent.rs:691-838` 的 `build_hints`，按 `ActivePane` 分支：`Prompt` 分支会按 `is_turn_running` 把 `Enter` 的标签在 `send`/`queue` 之间切换，并在有多行模式、粘贴元素、文件引用、prompt suggestion 等条件下增减（`views/agent.rs:787-837`）。session 级 hint（`ToggleQueue`、dashboard overlay 的 stop/cycle）在 `render.rs:491-515` 追加。

**pending confirmation** 会顶掉所有 hint：bar 只显示 `{key}:press again to {label}`（`views/shortcuts_bar.rs:229-248`）。

```
 shortcuts（1 行，最底）
 ┌──────────────────────────────────────────────────────────────────────┐
 │ Enter:send  │  Shift+Enter:newline  │  Ctrl+.:shortcuts          team  │
 └──────────────────────────────────────────────────────────────────────┘
   └─ 键用 BOLD+text_secondary ─┘        └─ 标签用 muted ─┘      └ 右对齐文本 ┘
   分隔符固定 "  │  "（5 列，DIM gray）
```

（键色/标签色：`shortcuts_bar.rs:216-226`；分隔符：`shortcuts_bar.rs:251-270`；右对齐文本：`shortcuts_bar.rs:294-306`；`Enter:send` 的断言见 `views/agent.rs:1927`。）

### 6.2 空间不足时的规则

- 逐项累加，任一项放不下就 `break`：先试分隔符（5 列），再试键、`:`、标签（`shortcuts_bar.rs:261-292`）。**不会**在行内画省略号。
- `compact` 模式的两条规则（`shortcuts_bar.rs:338-366`）：`pinned` 的 hint **永远保留**，其余在 `max_visible − pinned_count` 的预算内按原序取；`help_hint` 无条件追加，即使 hint 列表为空。`pinned` 的用途注释写明：让 nav/turn/mode 这类在每个 scrollback 上下文都该被发现的键始终可见（`shortcuts_bar.rs:31-33`）。
- 普通 pane 用 `max_visible = 5`（`render.rs:3234-3235`）。cheatsheet 的绑定默认是 `Ctrl+.`，若终端对它不可靠则主键换成 `Ctrl+X`、另一个进 `alt_keys`（`actions/defaults.rs:805-827`）。

### 6.3 与 status line 的邻接关系

- **上下**：status line 在 shortcuts 之上（`views/agent.rs:295-298`）；status line 关闭时，若底部有 vpad 就补一个 `shortcuts_gap` 行填位（`views/agent.rs:291-293`）。
- **何时 status line 在而 shortcuts 不在**：minimal 模式 —— 它没有 shortcuts bar（`xai-grok-pager-minimal/src/live.rs:714`），status line 照样画（`live.rs:404-416`）。
- **同时消失**：全屏子代理视图下，策略层用 `a_subagent_owns_the_frame()` 直接 return，不构建也不刷新（`app/status_line_policy.rs:88-91`）；welcome 屏没有 agent tab，`status_line_source_agent()` 返回 `None`，`update_status_line` 随即 `invalidate` 并返回（`status_line_policy.rs:31-33, 82-86`）——《25-status-line.md》"Troubleshooting" 节也确认这两处不画。
- **全屏模态**：`active_modal.is_some()` 时直接画模态并 return，两者都不走（`render.rs:3211-3215`）。

## 7. 对 jev-harness 可复用的做法

1. **契约拆成 leaf crate**（`config` + `context` 两个模块，`lib.rs:1-4`）：用户写的形状与运行时填的形状分开，且不依赖 agent 或任何客户端。
2. **能力协商关闭整条管线**：客户端在 initialize 报 `x.ai/statusLine`，agent 侧据此跳过整次 payload 构建（`acp/mod.rs:458-460`、`status_line.rs:275-289`）；这是"默认关闭的可选 UI"最省事的实现方式。
3. **"取不到就是 `None`，不是 0"**（`context.rs:5-8`）：把不确定性显式留在数据里，再让渲染层把缺失项整段丢掉（`segments.rs:72-117`）。这与本项目"概率优先、不要过早塌缩"的原则同向。
4. **自愿行是唯一会被布局回砍的行**：所有固定 UI 用 `Length` 写死、滚动区用唯一 `Min`，可配置行的高度上限由 `总高 − 其余所有行` 动态算出（`views/agent.rs:289-290`）。
5. **前插必须自带预算**：`room_for_front` 把"这一项能占多少列"变成调用方的责任，而不是渲染时再挤（`views/agent_status.rs:64-77`）。
6. **宽度不变量优先于信息密度**：hover 态与默认态同宽，宁可右侧补空格也不让布局抖（`views/context_bar.rs:165-183`）。

## 关键结论（供合稿引用）

1. 共有 5 个屏幕状态面：status bar 芯片行（顶，1 行固定）、scrollback、turn_status 运行态行（prompt 上方 1 行，按需）、status_line（prompt 之下 shortcuts 之上，0–5 行，默认关闭）、shortcuts bar（底，1 行固定）；另有 `/usage`、`/queue`、`/tasks` 三个提交进 scrollback 的纯文本块（`views/agent.rs:181-381`、`app/status_blocks.rs:1-14`）。
2. status_line 是唯一用户可配置的状态面，配置为 `[ui.status_line]`，只有 `type` / `command` / `items` / `padding` / `refresh_interval` 五个键，`type` 默认 `disabled`（`xai-grok-status-line/src/config.rs:18-34, 288-299`）。
3. **没有模板引擎**：表达力只有"6 个内置 item 排序"与"用户脚本读 JSON stdin 输出 stdout"两条路（`config.rs:325-332`、`views/status_line/segments.rs:66-118`、`docs/user-guide/25-status-line.md`）。
4. payload 与配置分属两个模块、两个方向：`StatusLineContext` 由 agent 的 `build_status_context` 组装，经瞬态 `SessionStatus` 通知下发；`session_name` 与 `trigger` 由**客户端**盖章（`xai-grok-status-line/src/context.rs:16-46`、`xai-grok-shell/src/session/acp_session_impl/status_line.rs:117-207`、`extensions/notification.rs:669`）。
5. 自定义命令注入确实存在（`type = "command"`），安全边界是：能力位协商 + 按会话的 `clientStatusLine` 元数据 + `["ui","status_line"]` 被列入 `PATCH_STRIP_PATHS` 不得由远程补丁注入（`lib.rs:11-17`、`xai-grok-config/src/config_override.rs:84-87`、`acp/mod.rs:458-460`）。
6. `status_line_policy.rs` **不负责取舍优先级**，它只管 tick 需求与 run 生命周期；取舍分四层：item 列上限（cwd 40 / model 30 / session-name 40）→ 行内 `elide` 尾截加 `…` → 行数（脚本 5 行上限，再由 `status_line_height.min(总高 − 其余所有行)` 回砍）→ 整面 `Off`（`app/status_line_policy.rs:280-297, 318-349`、`views/status_line/segments.rs:14-16`、`views/status_line/mod.rs:52-102`、`views/agent.rs:289-298`）。
7. 空间不足时先牺牲 status_line 自己的行，**prompt 与 shortcuts 的 1 行永不被挤掉**（它们与 status bar 一样是 `Length(1)`，scrollback 是唯一 `Min(5)`），注释明说"高脚本丢自己的行"（`views/agent.rs:134-135, 229, 246, 281-298`）。
8. 缺数据的 item 整段消失而非显示占位值：`cost` 低于 `$0.005` 不画，`turn-timer` 在 0 秒不画，`context` 缺 `used_percentage` 不画（`views/status_line/segments.rs:18, 87-115`）。
9. 上下文有三套口径与三套阈值，别混用：状态栏芯片用 `8.5K / 1.0M`（hover 换成进度条 + 固定 5 字符百分比，`≥100%` 显示 `MAX %`），status line 用 `{pct}% ctx`（兜底告警线 `CONTEXT_WARN_PCT = 80`），`/context` 块用 100 格分类条并把 `/compact` 提示限制在 `(80, auto_compact_threshold)` 区间；芯片颜色按 0/50/65/75/85/95 六个断点连续插值后收尾于 `warning`/`accent_error` 并统一 `quantize`（`views/context_bar.rs:16-41, 53-100, 141-196`、`views/status_line/segments.rs:11, 87-99`、`scrollback/blocks/context_info.rs:31-90, 437-446`）。
10. 运行态在独立的 `turn_status` 行而不是状态栏：`Idle` 且无 watcher 时整行消失；`Waiting(reason)` 有 7 种原因文案；等待用户输入时 spinner 换成脉冲 `◆` 并隐藏阶段计时（`views/turn_status.rs:365-395, 580-698, 725-735`、`acp/tracker.rs:48-73, 116-136`）。
11. shortcuts bar 的内容由 `key_owner()` 三选一（Surface / Pane + `.compact(5, help_hint)` / Hidden）；`pinned` 的 hint 永远保留、cheatsheet hint 无条件追加，放不下就整项 `break` 而不画省略号（`app/agent_view/render.rs:277-318, 3216-3239`、`views/shortcuts_bar.rs:338-366`）。
12. 未找到证据：`credit_bar_line_for_session` 在当前快照没有生产调用点，`render.rs:1387` 仍在读空的 `credits` 命中区域；手册中"repo-local 配置不能设 `[ui.status_line]`"的说法在 `xai-grok-config` / `xai-grok-shell/src/config` 里也未找到对应的过滤实现（`views/credit_bar.rs:231`、`render.rs:1387`）。
