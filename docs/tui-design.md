# jev-harness TUI 界面设计

> 本文件是 jev-harness 终端界面的**设计基准**：屏幕、布局、组件、交互与数据流。
> 视觉与结构参考 `ui-design/ref参考/` 的三张 Grok Build 截图（`main_ui1.png`、`main_ui2.png`、
> `startup.png`）与 [Grok TUI 设计调研](grok-tui-design.md)（下称「调研报告」）第 3、4、7 章；
> 研究视图的操作清单取自[研究目标](RESEARCH_GOALS.md) §11。
> 实现落在 `crates/jh-tui`（下称「本 crate」），批次划分见 §9。

## 1. 目的与范围

**本文件决定**：屏幕清单与每屏的布局、区域高度来源、组件清单与数据模型、焦点与键位、
视觉语言（色彩/字形/密度）、领域类型到视图模型的映射、批次划分。

**本文件不决定**：模型调用与判断循环本身（在 `jh-app`/`jh-core`）、持久化格式、
研究视图里每一项的统计口径（以研究目标与路线图为准）。

与 1.0 路线图的关系：路线图决定 4 把 TUI 接线推迟到 1.x
（[roadmap-1.0.md](roadmap-1.0.md) §3.1、§5、§7.1），本文件是该决定的**前置设计**，
不改动路线图本身；「路线图是否需要回写」列入待人工裁决项（§10）。

本文件描述的设计**已按批次二、三落地**（外壳、首屏、主界面、三个研究面板），
**批次四的接线也已落地**：`jev chat` 走真实 LLM 通道与 Jev 判定门控，研究视图读会话的真实运行产物。
落地范围、实现口径与未落地项逐条见 §9。

## 2. 设计原则

1. **机制照抄，量级要砍**（调研报告 §7）：抄单一动作注册表、块是纯函数、行级元数据一次产出、
   按需渲染与空闲零字节、门控带理由；不抄三形态渲染、自研 inline Terminal、Mermaid 栈、
   元测试与 PTY e2e 全家桶。
2. **整屏单列**：整屏只调用一次 `Layout::vertical`，横向没有分栏；唯一的弹性行是回滚区
   （调研报告 §4.1）。研究视图的「面板」是同一列里的一个带，不是可拖拽分栏。
3. **概率是一等数据**：Jev 的选项概率、`confidence`、`noul` 一律原样上屏，不塌缩成布尔或名次
   （`AGENTS.md`「概率优先」）。主界面把它画在状态行与判定块里；研究视图的判断详情面板画完整分布。
4. **决策分层可见**：确定性规则 → Jev → LLM 三级判定在界面上有统一的呈现位置
   （判定块与判断详情面板的「来源」字段），不让读者猜这条结论是谁给的。
5. **状态由能力位决定**：色彩与字形只来自 `Capabilities`，业务代码不读环境变量
   （调研报告 §7.1 第 7 条）。无色档与回退字形下，**正文一个字符都不许少**。
6. **门控带理由，拒绝要有出路**（调研报告 §7.1 第 8 条）：等待、失败、无效、未接线都要在屏上
   说清「为什么」与「怎么办」，不用空态糊过去。
7. **dispatch 不碰终端/网络/文件**：键盘处理只改状态并产出 `Effect` 值，副作用由宿主执行
   （调研报告 §7.1 第 9 条）。
8. **不做过度防御**：终端状态恢复（raw mode / 备用屏 / 括号粘贴）是少数保留的防御
   （调研报告 §7.1 第 10 条）；其余地方让错误显式失败。

## 3. 与参考截图的逐条对照

| 截图元素 | 照抄 / 替换 / 不抄 | jev-harness 的对应物 |
| --- | --- | --- |
| 顶栏：左工作目录、右 `18K / 1.0M` 与 `[Dashboard]` | 替换 | 顶栏：左工作目录（`~` 缩写），右上下文用量 `12.4k/64k` 与 `[研究视图]` 入口 |
| 用户消息：整宽填充条 + `❯` 前缀 + 右对齐时间 | 照抄 | 用户块：填充条背景（`bg_light`）、`❯ ` 前缀、首行右对齐时间戳 |
| `◆ Thought for 0.3s` / `Worked for 1.5s` | 替换 | 阶段行（Notice 块）：`◆ 判定 12ms · 采纳 usr:allow 0.71 · 倾向`、` 用时 1.5s` |
| `◆ Run List all files…`（工具调用一行摘要） | 照抄 | 工具块：`▸ bash git status --short`（已有实现，保持） |
| 助手正文 | 照抄 | 助手块：`◆ ` 前缀 + 词级换行 + 续行缩进（已有实现） |
| Markdown 表格 | 不抄 | 本批不引入 Markdown 解析（非目标，§10） |
| 提示框：四边盒 + 底边内嵌 `模型 · 模式` | 照抄 | 提示框：四边盒 + 底边信息行 `模型 · 判定器 · 授权模式` |
| 快捷键栏：`Enter:send │ Shift+Enter…` | 替换 | 快捷键栏：数据源只有动作注册表，中文标签 + `·` 分隔（已有实现） |
| 回滚区底部居中的 `▼`（`main_ui2.png`） | 照抄 | 滚动提示：不贴底时在回滚区最后一行居中画 `▼` |
| 首屏：居中面板（ASCII 图形 + 版本 + 条目 + 菜单） | 照抄 | 首屏：居中面板（字标 + 版本 + 概览 + 菜单）+ 底部提示框 + 右下状态 |
| 首屏右下 `Logged in with API Key` | 替换 | 首屏右下：`判定器 rule · 未接线`（如实标注能力状态） |
| 审批卡 | 照抄 | 已有实现：强调轨 + 命令 + 理由 + 选项行，单槽位模态 |
| Dashboard / 多会话 | 不抄 | 非目标；研究视图以面板切换承载 |

## 4. 视觉语言

### 4.1 色彩

直接复用 `theme.rs` 的 GrokNight 深色取值与四级色深量化（`None < Basic < Ansi256 < TrueColor`），
不新增主题、不做热切换。批次二新增五个角色字段：

| 字段 | 用途 | 取值来源 |
| --- | --- | --- |
| `panel_border` | 面板与提示框的边框（未聚焦） | 复用 `selection_border` 的取值量级 |
| `panel_border_active` | 聚焦面板的边框 | 复用 `prompt_border_active` 的取值量级 |
| `rule` | 面板内小节分隔线 | 取 `gray_dim` 一档 |
| `timestamp` | 用户块时间戳、证据时间列 | 取 `gray` 一档 |
| `user_block_bg` | 用户块填充条背景 | 复用 `bg_light` |

无色档下这五个字段全部量化为 `Reset`，靠 `Modifier::DIM`/`BOLD` 与字形区分（现有口径不变）。

### 4.2 字形

沿用「每个字形一个函数、花体与回退同宽」的硬约束（`glyphs.rs` 的 `ALL` 表 + 同宽断言）。
批次二新增：

| 字形 | 花体 | 回退 | 用途 |
| --- | --- | --- | --- |
| `box_top_left` / `box_top_right` / `box_bottom_left` / `box_bottom_right` | `╭ ╮ ╰ ╯` | `+` | 提示框与面板的四角 |
| `box_horizontal` / `box_vertical` | `─ │` | `-` `\|` | 边框横竖 |
| `box_tee_down` | `┬` | `+` | 面板 tab 条的分隔（底边不画分叉，故没有向上的那一个） |
| `scroll_hint` | `▼` | `v` | 回滚区「还有下文」提示 |
| `arrow_up` | `▲` | `^` | 面板「上方还有内容」提示 |

字标（首屏图形）不进字形表：它是首屏专有的多行块，花体与回退各一份常量
（形如 `SPINNER_FRAMES` / `SPINNER_FRAMES_FALLBACK` 的既有做法）。

### 4.3 排版与密度

| 项 | 取值 | 依据 |
| --- | --- | --- |
| 整屏外边距 | 0（区域贴满，靠内容自己的缩进呼吸） | 本 crate 现状（`render.rs::layout`） |
| 块标记列 | 2 列（`❯ ` / `◆ ` / `▸ ` / `◇ `） | 现状 |
| 块间空行 | 1 行（不可选、不参与复制） | 现状（`Scrollback::layout` 的 `gap_after`） |
| 面板内缩进 | 边框 1 列 + 内容 2 列 | 对齐提示框的 `│ ` 前缀 |
| 面板标签列 | 面板内最长标签宽度（同一面板内对齐），封顶 12 列 | 判断详情面板的「键 值」行 |
| 助手与阶段行标记 | 2 列（`◆ `）；正文从第 3 列起，续行同列缩进 | 参考截图的 `◆ Thought for 0.3s` |
| 提示框最小高度 | 3 行（顶边 + 1 行内容 + 底边信息行） | 调研报告 §4.4 的 `vpad_top + 1 + info_block` |
| 提示框最大高度 | `区域高度 / 2`，且不超过 `MAX_PROMPT_ROWS = 8`（含边框时按内容行折算） | 现状 + 调研报告 §4.4 |

## 5. 屏幕清单与布局

`Screen` 三个值：`Startup` / `Agent` / `Research`。审批卡是 `Agent` 与 `Startup` 共用的单槽位模态
（`Option<ActiveModal>`，不上栈——调研报告 §4.3）。

### 5.1 主界面 Agent

```text
 ~/Documents/jev-harness                          12.5k/64k · [研究视图]   ← 顶栏（1 行）
 ❯ 帮我看一下未提交的改动                                    14:18         ← 用户块（填充条 + 时间戳）
 ◆ 我先跑 git status，再用 Jev 判断是否需要你确认。                        ← 助手块（◆ + 空格）
 ◆ 判定 12ms · 采纳 usr:allow 0.71 · margin 0.49 · 倾向                    ← 阶段行（Notice 块）
 ▸ bash git status --short
 ◇ shell_command_allowed
   是否允许执行 `git push --force`？
   ▸ 0.71 █████▋   允许一次 (usr:allow)
     0.22 █▊       拒绝 (usr:deny)
   采纳 允许一次 · usr:allow 0.71 · margin 0.49 · confidence 0.64 · 倾向
                                                                          ← 回滚区（Min，唯一弹性）
                                                              ▼           ← 不贴底时的滚动提示
 ⠹ Jev 判定中 · 判定 usr:allow 0.71 · margin 0.49 · confidence 0.64 · …   ← 状态行（1 行）
 ╭──────────────────────────────────────────────────────────────────╮
 │ ❯ 顺便看一下 CI 配置                                              │
 ╰─ deepseek-chat · jev-1.13.0 · always-approve ────────────────────╯
 Ctrl+c 退出 · Esc 取消 · Enter 发送 · Alt+Enter 换行 · PgUp 上翻 …        ← 快捷键栏（1 行）
```

阶段行用 [`NoticeBlock`]：`marker = true` 时画 `◆ `，`marker = false`（轮次收尾的
`用时 1.5s` 一类）时用等宽空白占位，两种形态的正文从同一列起。

区域高度（自上而下，`agent_layout`）：

| 区域 | 约束 | 说明 |
| --- | --- | --- |
| `header` | `Length(1)`，`area.height < SHORT_TERMINAL_ROWS(16)` 时为 0 | 工作目录 / 用量 / 研究视图入口 |
| `scrollback` | `Min(1)` | 唯一弹性行；`pending` 溢出的只有它 |
| `status` | `Length(1)` | 阶段 + 等待原因 + 本轮判定概率 + 模型 + 上下文 |
| `prompt` | `Length(3..=10)` | 四边盒形态 = 内容行（1..=8，受 `MAX_PROMPT_ROWS` 与屏高一半约束）+ 顶边与底边信息行 |
| `shortcuts` | `Length(1)` | 数据源是动作注册表的 `hint_priority` |

### 5.2 首屏 Startup

```text
            ╭─ jev-harness 0.1.0 ──────────────────────────────╮
            │ ╭─╮  jev-harness 0.1.0                           │
            │ │◇│  判定器  rule · 未接线                         │
            │ ╰─╯  工作区  ~/Documents/jev-harness             │
            │                                                  │
            │ 新建会话                                  Ctrl+N │
            │ 研究视图                                  Ctrl+O │
            │ 退出                                      Ctrl+C │
            ╰──────────────────────────────────────────────────╯

 ╭────────────────────────────────────────────────────────────────────╮
 │ ❯ 输入消息，Enter 发送，Alt+Enter 换行                             │   ← 空输入时的幽灵文案
 ╰─ 未接线 · 不调用模型与 API ────────────────────────────────────────╯
                                                    判定器 rule · 未接线
```

区域：居中面板（横行居中、纵向偏上，宽不超过 `MAX_STARTUP_PANEL_COLS(64)`）+
底部提示框 + 右下注解（1 行）。菜单项的键位靠右对齐，条目文案与动作注册表一致
（`Ctrl+N` 建会话、`Ctrl+O` 进研究视图、`Ctrl+C` 退出）。
窗口小于 `MIN_STARTUP_ROWS(12)` 或 `MIN_STARTUP_COLS(48)` 时退化为纯文本列表（不画面板与字标）。

### 5.3 研究视图 Research

```text
 ~/Documents/jev-harness                        12.5k/64k · [会话]        ← 顶栏
 ╭─ 判断详情 ─┬─ 事件与证据 ─┬─ 失效传播 ──────────────────────────╮      ← 面板 tab 条
 │ 问题   shell_command_allowed                                   │
 │ 版本   v3（当前）                                               │
 │                                                                │
 │ 分布                                                            │
 │ ▸ 0.71 █████▋    允许一次 (usr:allow)                           │
 │   0.22 █▊        拒绝 (usr:deny)                                │
 │ confidence 0.64                                                │
 │                                                                │
 │ 来源                                                            │
 │ 判断器 RealApi · jev-1.13.0                                     │
 │ 快照   7b1e                                                    │
 │ 有效性 待重查                                                   │
 ╰─ shell_command_allowed v3 ────────────────────────────────────▼ ╯      ← 脚注 + 未到底提示
 ● 研究视图 · 判断详情 · 有效性 待重查                                     ← 状态行
 Ctrl+c 退出 · Esc 取消 · Tab 下一个面板 · Shift+Tab 上一个面板 · s 单步 …  ← 快捷键栏
```

面板内容是换行后的行序列：内容不足时补空行，超出时按偏移切片，未到底/未到顶时
在底边或顶边的右端画 `▼` / `▲`（不占内容列）。

区域：`header(1)` + `panel(Min(1))` + `status(1)` + `shortcuts(1)`。
研究视图**不显示提示框**：研究态不接受自然语言输入，焦点归面板（`KeyOwner::Research`）。

### 5.4 审批卡（已实现；批次四接线后覆盖所有工具调用）

单槽位模态，压在回滚区底部，整行铺底不透明；键盘归卡片时快捷键栏换成权限键，
状态行显示「等待授权」。门控理由写在卡片正文（`ApprovalRequest::body`），
没有理由的裁决请求在设计上不接受。

批次四起，**只读工具与 `bash` 都要过这张卡**（用户要求「每一次副作用先经审批卡」），
卡片正文固定含两段：确定性风险类别，以及门控判定（判定不可用时如实写「判定不可用」而不是
「拒绝」）。「本次会话总是允许」按工具名记入会话白名单，之后的同类调用不再上卡——
授权模式在提示框底边的信息行里常驻显示（§5.1 的 `模型 · 判定器 · 授权模式`）。

## 6. 组件清单与代码映射

| 组件 | 状态 | 数据模型 | 文件 |
| --- | --- | --- | --- |
| 主题与色深 | 扩展 | `Theme`（+5 字段）、`ColorLevel` | `src/theme.rs` |
| 字形表 | 扩展 | `Glyph`、`Glyphs`、`ALL` | `src/glyphs.rs` |
| 块模型 | 扩展 | `Block`（+`Notice`）、`UserBlock`（+`timestamp`） | `src/blocks.rs` |
| 回滚区 | 现有 | `Scrollback`、`ScrollWindow` | `src/components/scrollback.rs` |
| 滚动提示 | 新增 | `ScrollHint` | `src/components/scrollback.rs` |
| 提示框 | 扩展 | `Prompt`、`PromptChrome`（标题 + 底边信息行，几何助手 `inner_width` / `content_origin` / `rows`） | `src/components/prompt.rs` |
| 顶栏 | 新增 | `HeaderBar`、`HeaderModel` | `src/components/header.rs` |
| 面板壳 | 新增 | `Panel`、`PanelTitle`、`PanelLine`、`Tone`、`PanelState` | `src/components/panel.rs` |
| 状态行 | 现有 | `StatusModel`、`TurnPhase`、`WaitingReason` | `src/components/status.rs` |
| 快捷键栏 | 现有 | `ShortcutBar` | `src/components/status.rs` |
| 审批卡 | 现有 | `ActiveModal`、`ApprovalRequest`、`ApprovalCard` | `src/components/approval.rs` |
| 屏幕与布局 | 新增 | `Screen`、`StartupLayout`、`AgentLayout`、`ResearchLayout`、尺寸阈值常量 | `src/screens.rs` |
| 首屏面板 | 新增 | `StartupFacts`、`panel` / `plain`、`MENU`、字标常量 | `src/startup.rs` |
| 研究面板 | 新增 | `PanelId`、`Panels`、判断/证据/传播三份视图模型 | `src/panels/` |
| 应用状态 | 扩展 | `App`（+`screen`/`panels`/`panel_state`/`paused`）、`Effect`（+`Step`/`SetPaused`） | `src/app.rs` |
| 动作注册表 | 扩展 | `ActionId`（+5）、`When`（+`ResearchFocused`）、`KeyOwner`（+`Research`） | `src/actions.rs` |
| 帧管线 | 扩展 | `RenderGate`（不变）、`draw` 按屏分发 | `src/render.rs` |
| 离线预览 | 新增 | —— | `examples/design.rs` |

## 7. 交互

### 7.1 焦点模型

`KeyOwner` 三档扩成四档，仍然只有一个焦点：

| `KeyOwner` | 何时 | 键盘归属 |
| --- | --- | --- |
| `Approval` | 审批卡在槽位上 | 权限键优先，不吃提示框的提交键 |
| `Research` | 研究视图（无卡） | 面板滚动、面板切换、单步/暂停 |
| `Prompt` | 首屏与主界面 | 文本输入 + 提交；翻页键下溢一档到回滚区 |

`Esc` 的阶梯（沿用现有「先关最上面那层」的写法）：① 有关 → 关卡片；② 在研究视图 → 回主界面；
③ 其余 → 取消当前轮（无在途轮次时退出）。

### 7.2 新增动作与默认键位

| 动作 | 键位 | 上下文 | 说明 |
| --- | --- | --- | --- |
| `ToggleResearch` | `Ctrl+O` | `Always` | 主界面 ↔ 研究视图（首屏也可进） |
| `NextPanel` | `Tab` | `ResearchFocused` | 下一个研究面板 |
| `PrevPanel` | `Shift+Tab` | `ResearchFocused` | 上一个研究面板 |
| `StepOnce` | `s` | `ResearchFocused` | 单步；产出 `Effect::Step`，宿主执行 |
| `PauseToggle` | `Space` | `ResearchFocused` | 暂停/继续；产出 `Effect::SetPaused(bool)` |
| `ScrollLineUp/Down`、`ScrollPageUp/Down` | `↑ ↓ PgUp PgDn` | `ResearchFocused`（新增绑定） | 滚动面板内容 |

键位仍是「单一注册表 + 可覆盖」（`Keymap::with_overrides`），新增绑定必须过 `validate()`：
同一动作可以在多个上下文里各有一条注册，`registry_covers_every_action_id` 相应放宽为
「每个 `ActionId` 至少出现一次」。

### 7.3 模式切换语义

- 首屏提交（`Enter`）→ 切到主界面（`Screen::Agent`）并回显用户块。
- `Ctrl+O` → 切到研究视图；`Esc` → 回主界面；两屏共享同一个 `App` 状态（回滚区位置、面板偏移都保留）。
- 研究视图里 `paused == true` 时状态行显示「已暂停」；`StepOnce` 不清除暂停标记。

## 8. 数据流与分层

```text
jh-core / jh-app（领域对象）
        │  适配层（未来接线时落在 jh-app 或宿主的 TUI 装配代码里）
        ▼
jh-tui 视图模型（DecisionTrace / JudgmentView / EvidenceView / PropagationView / HeaderModel / PanelLine）
        │  纯函数渲染
        ▼
BlockLine（已换行 + 行级元数据）→ 缓冲区
```

- **本 crate 不依赖 `jh-core`/`jh-app`**：视图模型是本 crate 自己的类型，适配层把领域对象翻译成
  它们。这样组件库可以离线测试，也让「Jev 客户端换实现」不影响渲染层。
- 时间戳一律由适配层格式化成字符串（本 crate 不做时区与日历计算，不引入日期库）。
- 概率与置信度是 `f32`/`f64` 原值，渲染层只做裁剪与显示，不做归一化或名次化。

领域类型 → 视图模型对照（本批落地的字段）：

| 领域类型（`jh-core`） | 字段 | 视图模型 |
| --- | --- | --- |
| `JudgmentNode` | `id`、`question`、`question_version`、`validity`、`evaluation` | `JudgmentView` 的问题与状态行 |
| `InputProjection` | `objects`（对象 → 字段选择） | `JudgmentView::projection`（读出为「对象:字段」文本） |
| `JudgmentRecord` | `source`、`version`、`request_id`、`usage`、`latency_ms`、`snapshot` | `JudgmentView::source`、`request_id`、`usage`、`latency_ms`、`snapshot` |
| `Answer::Choice` | `choice`、`probabilities`、`confidence` | `Distribution::Choice` + `blocks::Choice` + `confidence` |
| `Answer::Score` | `score`、`legend`、`probabilities`、`confidence` | `Distribution::Score`（期望值 + 各级概率条） |
| `Answer::Noul` | `noul` | `Distribution::Noul`（**没有** `confidence` 字段——类型上就不存在） |
| `Evidence` | `id`、`source`、`at_unix_ms`、`object`、`version`、`content_hash` | `EvidenceRow`（时间由适配层格式化；撤回是「另记一笔」，界面按 `retracted` 标记显示） |
| `Edge` / `EdgeKind` / `EdgeOrigin` | `from`、`to`、`kind`、`origin` | `PropagationView::edges`（四类边 + 确定性/模型提出） |
| `PropagationPath` / `Invalidation` | `target`、`hops`、`stale`、`blocked` | `PropagationView::paths`、`stale`、`blocked` |
| `Action` / `PremiseCheck` | `id`、`premises`、`validity`、`Cleared/Blocked/Withdrawn` | `PropagationView::actions` |

## 9. 分期

| 批次 | 内容 | 状态 |
| --- | --- | --- |
| 一 | 组件套件：主题/字形/宽度/块模型/回滚区/提示框/状态行/快捷键栏/审批卡/动作注册表/重绘闸门/终端护栏 | 已落地 |
| 二 | 外壳与两屏：`Screen` 与布局、顶栏、四边盒提示框、滚动提示、阶段行块、用户块时间戳、首屏 | 已落地（本文对应提交） |
| 三 | 研究视图：面板壳 + 判断详情 / 事件与证据 / 失效传播三个面板、研究焦点与键位、单步/暂停的 `Effect` | 已落地（面板骨架；数据由宿主喂入） |
| 四 | 接线：`jev chat` 会话（真实 LLM 主循环 + Jev 判定门控 + 工具审批）、研究视图读真实运行产物；成本与观测器面板 | 已落地（会话与真实数据；成本/观测器面板未落地，见 §10） |
| 五 | 研究目标 §11 的其余操作：问题版本比较、目标与验收条件、观测器契约、成本与执行时间、环境分叉 | 未开始 |

### 9.1 批次四的落地范围与实现口径

宿主落在 `crates/jh-app/src/session/**`（本 crate 仍不依赖 `jh-core`/`jh-app`）：

| 模块 | 职责 |
| --- | --- |
| `model` | LLM 侧：一次调用的抽象 + `jh-openai` 的 `/responses` 流式实现（future 不借用会话，可进 `select!`） |
| `tools` | 工具集：工作区内只读（`read_file` / `list_dir` / `search`）+ `bash` 命令执行 |
| `gate` | 门控：确定性风险层 + Jev 类型化判定（`shell_command_allowed`）+ 防抖 + 采纳门槛 |
| `approval` | 会话内审批策略：「本次会话总是允许」按工具名记入白名单；提示框信息行的授权模式 |
| `agent` | 会话状态机（模型 → 门控 → 审批 → 执行 → 回收），界面变化描述成 `Update` |
| `view` | 领域对象 → 判断详情 / 证据 / 传播三份视图模型与判定轨迹 |
| `tui` | 终端宿主：`EventStream` + 在途步骤 + 按需 tick 的 `select!` 循环、`Effect` 执行、恢复护栏 |

口径（批次四新定的六条，实现与文档同步）：

1. **审批卡覆盖所有工具调用**：只读工具与 `bash` 都上卡（用户要求「每一次副作用先经审批卡」取严）；
   「本次会话总是允许」按工具名记入会话白名单，之后的同类调用不再上卡并在阶段行说明。
2. **门控三档**：确定性层先分类（只读 / 会改动工作区 / 破坏性，只写理由不裁决，只读工具到此为止）
   → Jev 问 `shell_command_allowed`（`usr:allow` / `usr:deny`，概率与 `confidence` 原样保留）
   → **采纳**首选项：`usr:allow` 照常上卡；`usr:deny` 且 `margin ≥ 0.15`、`confidence ≥ 0.5`
   时**不上卡直接拒绝**（理由与出路写在阶段行与工具块里）；判定器不可用、分布太接近或 confidence
   不足一律标「判定不可用」并交人工裁决。**「判定不可用」与「门控拒绝」在文案与语气上是两句不同的话**，
   界面上不许混读成「模型判了拒绝」。
3. **概率是一等数据**：判定轨迹（状态行 + 判定块）、审批卡正文、判断记录（`thresholds` 写明采纳门槛）
   与研究视图的判断详情都是同一份概率原值，不塌缩成布尔，也不排名化。
4. **对象模型**：`session.ask`（每次提交一个版本）、`shell.command`（每次 shell 调用一个版本）、
   `tool.result`（每次工具执行一个版本）三个对象承载证据；门控判断的输入投影覆盖它们，
   于是「新证据 → 依据变旧 → 待重查 → 行动被挡下」在会话里真实发生，传播面板画的就是它。
5. **流式正文与工具结果**：`jh-tui` 新增 `App::{stream_assistant, finish_assistant, begin_tool, finish_tool}`
   （`Scrollback::last_mut` + 视口锚定），模型增量与工具结果就地写进同一块，不产生「一个增量一个块」。
6. **缺凭据不阻止进入界面**：非 TTY 下 `jev chat` 显式报「需要真实终端」并退 2；TTY 下判定器缺凭据
   标「不可用」、模型缺配置在提交后如实上屏失败原因——界面照常可用，不假装接了模型。

验收口径（批次二至四的实际读数）：`cargo test --workspace` 全绿（`jh-tui` 单元测试 138 +
整屏与组件快照 28 + 文档测试 1；`jh-app` 含会话宿主的 26 条会话级单测（`session::*`）），
`cargo clippy --workspace --all-targets` 零告警；`cargo run -p jh-tui --example design` 在非 TTY 下按
100×24 逐屏打印首屏 / 主界面 / 审批卡 / 三个研究面板 / 无色回退档六种单帧；
`jev chat` 在真实终端里进备用屏、`Ctrl+C` 退出并恢复终端（已在伪终端里冒烟：进入 → 提交一句 →
退出，退出码 0 并打出本轮读数）。快照里的期望文本与线框一一对应，行尾空白不参与比较。

#### 门控真调

`cargo test -p jh-app --lib -- --ignored the_gate_question`（需要 `TYPESAFE_API_KEY`）：
门控问题 `shell_command_allowed` 被服务端接受，`git status --short` 得到
**`采纳 usr:allow 0.99 · margin 0.98 · confidence 0.98`**（620 ms，执行者 `jev-1.13.0`）——
问题定义、分布与 `confidence` 的读回口径按线上形态走通。

#### 会话真调（2026-09-23，DeepSeek 通道 × Jev）

命令（key 只经环境变量，本机真 key 在 `DEEPSEEK_API_KEY` 里；`OPENAI_BASE_URL` 不带尾斜杠）：

```text
(sleep 3; printf '请用 bash 跑 ls -1 列出工作区文件，再用一句话总结。\r'; sleep 40; printf 'y'; \
  sleep 35; printf '\x03'; sleep 3) \
  | OPENAI_API_KEY="$DEEPSEEK_API_KEY" OPENAI_BASE_URL=https://api.deepseek.com \
    OPENAI_MODEL=deepseek-flash \
    script -qec 'stty rows 40 cols 120; target/debug/jev chat --workspace /tmp/chat-live3' /dev/null
```

`stty` 是必需的：伪终端没有窗口尺寸时 ratatui 画不出内容——循环照常跑、读数照常有，界面却是空的
（同一天的 `h1h2-1790160738057` 那次就是这样，记在这里免得下次再踩）。

读数（产物 `var/chat/h1h2-1790160989294/evidence.jsonl`；调用记录追加进 `var/calls.jsonl`）：

| 项 | 读数 |
| --- | --- |
| 轮数 / 退出 | 1 轮，`Ctrl+C` 退出，终端恢复，退出码 0，屏上 `用时 41.3s`（含伪终端等待按键的节奏） |
| 模型调用 | 2 次 `deepseek-flash`：989 ms（in 799 / out 64 token）、1384 ms（in 885 / out 120 token），累计 in 1684 / out 184 |
| 门控判定 | 1 次 `shell_command_allowed` → `real_api` / `jev-1.13.0`，664 ms，`usr:allow 0.99` / `usr:deny 0.01`，`confidence 0.98`，阈值 `margin 0.15` / `confidence 0.5`（写进判断记录），`request_id` `req_01a0cde8…` |
| 工具与审批 | `bash ls -1`（确定性层：只读）→ 审批卡「允许一次」→ 真实执行，退出码 0，输出 `notes.md` / `src`，结果回填到工具块 |
| 证据 | `session.ask` v1（用户原话）、`shell.command` v1（命令 + cwd + 风险类别）、`tool.result` v1（状态 + 输出） |
| 屏上顺序 | 首屏 → 用户块（时间戳）→ `⠋ 模型输出中` → `◆ 模型请求了 1 个工具调用` → `◆ 判定 664ms · 采纳 usr:allow 0.99 · margin 0.98 · confidence 0.98 · 倾向允许` → 判定块（两条概率条 + 采纳行）→ 审批卡（标题 `权限请求 · bash`、命令行 `ls -1`、两段理由、`y/a/n`）→ `◆ 已裁决：bash 允许一次` → `⠋ bash ls -1` → `▸ bash 退出码 0 …` → 助手正文 → `✓ 完成 · 判定 …` |

真调暴露并已修掉的一处 bug：工具块曾画成 `▸ bash bash ls -1`、审批卡命令行也重复工具名——
参数摘要与工具名拆成两个方法（`Tools::detail` 给块与卡片、`Tools::display` 给通知与证据），
修复后同一次真调的屏上是 `⠋ bash ls -1` 与 `命令  ls -1`。

key 自检：`var/calls.jsonl`、三个 run 的证据文件与三次伪终端日志里都没有 key 原文（逐一 `grep` 过）；
会话侧真 key 的别名 `DEEPSEEK_API_KEY` 也进了 `tools.rs` 的子进程摘除名单。

## 10. 非目标与未确定项

**非目标**（与本批设计无关，且已有明确取舍）：

1. 多形态渲染（Fullscreen / Inline / Minimal）——先只做 alt-screen 全屏（调研报告 §7.2 第 1 条）。
2. 鼠标、选择与剪贴板、主题热切换、命令面板与模糊搜索（调研报告 §7.2 第 3、6 条与专题 17）。
3. Markdown 解析、语法高亮、Mermaid 与内联图片（调研报告 §7.2 第 3 条）。
4. 多会话 dashboard 与 session picker（截图里有 `[Dashboard]`，本设计以研究视图代替）。
5. 元测试、PTY e2e、性能基准（调研报告 §7.2 第 4 条）。
6. （批次二、三的边界）不改 `jh-app`、不新增依赖、不做接线——**批次四已按 §9.1 打破这一条**：
   宿主落在 `jh-app/src/session/**`，`jh-app` 新增 `jh-tui` / `ratatui` / `crossterm` / `futures` 四个依赖
   （都登记在根 `Cargo.toml` 的 `[workspace.dependencies]`），`jh-tui` 仍然不反向依赖 `jh-app`/`jh-core`。

**未确定项**：

1. 三个研究面板之外的 5 项研究操作（问题版本比较、目标与验收、观测器契约、成本与执行时间、
   环境分叉）只有位置（同一面板壳），没有字段设计——留待批次五。
2. 环境分叉（fork）的交互语义（在哪一步分叉、分叉出什么）未设计。
3. 顶栏徽章与首屏概览的确切文案未定稿（当前为占位：工作目录、用量、`[研究视图]`、`[会话]`）。
4. 首屏字标的图形未定稿：当前是 3 行占位字标，花体/回退各一份。
5. `Score` 与 `Noul` 分布已按概率条画出（`Score` 额外给期望值，`Noul` 附一句说明它没有
   独立的 `confidence`），但「是否画 50% 中线」「等级条是否与选项条用同一刻度」仍未定。
6. `docs/roadmap-1.0.md` §3.1/§7.1 把 `jh-tui` 记为「冻结」；本批扩展了它的公共 API，
   是否回写路线图由人工裁决。
7. 视觉参数（配色取值、概率条刻度、首屏面板的纵向偏置 1/3）只在 `TestBackend` 缓冲区与
   离线渲染图上核对过（`ui-design/current/`），**未在真实终端的每类配色下逐一对照**；
   留给接线批次一起看。
8. **东亚宽度歧义**：`◆`、`◇`、`█`、`▏`、`─`、`╭`、`→` 这一组在 Unicode 里是 Ambiguous，
   我们的布局按 1 列算；CJK 字体常把它们画成 1em（是 1 列的两倍）。离线渲染时这批字形按半格
   压缩过，真机是否错位取决于终端的 East-Asian-Ambiguous 设置与字体——这也是字形表里
   回退形态（`+ - | ♦ ○ v`）存在的理由：不确定时整库切回退形态 [[`Capabilities`]] 即可。
9. 研究视图是否需要「面板内容搜索」未定（调研报告把搜索归入代价过高的做法）。
10. **顶栏上下文用量留空**：会话不知道该模型的上下文窗口上限（`OPENAI_MODEL` 只给模型 ID），
   而设计 §5.1 的 `12.4k/64k` 要有上限才画得出来——本批不猜数字，用量显示留待有配置来源时再补。
11. **防抖口径未校准**：窗口缺省 30 秒、键 = 工具名 + 参数摘要（不含整份材料），
    同一命令在窗口内复用上一次判定；「材料变了还复不复用」这条线是按会话形态拍的，
    没有实测数据，改窗口用 `jev chat --debounce`。
12. **采纳门槛未校准**：`margin ≥ 0.15` 且 `confidence ≥ 0.5` 才采纳「拒绝」是**声明口径**
    （写进判断记录的 `thresholds`），取值的合理性要看真实调用的分布，本批没有真调读数。
13. 模型正文仍按纯文本画（不做 Markdown 解析，§10 非目标 3 不变）：会话里模型给的列表与代码块
    会原样平铺，排版待定。
14. **成本与观测器面板未落地**（批次四的另一半）：会话已经在记调用记录（`var/calls.jsonl`）
    与证据（`var/chat/<run>/evidence.jsonl`），但界面里还没有读它们的面板——留待批次五。
