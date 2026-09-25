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
| Markdown 表格 | 抄子集 | 只做助手正文的轻量子集（§4.4）：认 GFM 管道表格（表头 + 分隔行 + 对齐 + 窄屏收窄/裁列），列数不齐、缺分隔行等一律退回普通文本 |
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

### 4.4 助手正文的轻量 markdown 子集与样式映射

助手正文（`Block::assistant` / `Block::streaming_assistant`）走 `src/markdown.rs` 的**轻量子集**，
用户块、工具块、阶段行、决策块仍是纯文本。只做下表这一撮结构：不做完整 CommonMark，
不做嵌套引用、HTML 与语法高亮，也不引入 `pulldown-cmark` / `syntect`（§10 非目标 3）；表格只做 GFM 管道表这一个子集。

| 结构 | 上屏文本 | 样式 |
| --- | --- | --- |
| `#` / `##` / `###` | 去掉 `#` 标记 | 整行 `theme.accent(theme.accent_system)`（BOLD + 强调色）；`####` 与 `#标签` 不是标题 |
| `- ` / `* ` / `+ ` | 前缀换成 `• `，每级缩进 2 空格 | 标记 `theme.dim()`，正文 `theme.fg()` |
| `1. ` | 保留序号 | 标记 `theme.dim()`，正文 `theme.fg()` |
| ```` ``` ```` / ```` ```lang ```` | 块内原样（不解析行内） | 正文 `theme.muted()`（`gray`）+ 左缩进 2 格；`lang` 单独一行 `theme.dim()`，不做语法高亮 |
| `> ` | 去掉 `>`，行首 `▏ ` | `theme.dim()` |
| `---` / `***` | 一行 `─` | `theme.dim()`，宽度铺满正文列（窄屏裁短） |
| `**粗**` | 去掉标记 | 基础样式 + `BOLD` |
| `*斜*` / `_斜_` | 去掉标记 | 基础样式 + `ITALIC`；`_` 只在词两端认（`snake_case_name` 保持原样） |
| `` `代码` `` | 去掉标记 | 基础样式 + `theme.accent_tool` |
| `[文本](url)` | `文本 (url)` | 文本 `theme.accent_system`，url 追加为 `theme.dim()` 的 ` (url)` |
| 管道表格 | 表头 + `─┼─` 规则线 + 对齐过的数据行，列间 `│` | 表头 `theme.accent(theme.accent_system)`（BOLD），分隔符与规则线 `theme.dim()`，正文 `theme.fg()`；格内行内标记照常生效 |

**管道表格（GFM 子集，2026-09-24）**：表头行 + 紧邻的合法分隔行才认表，否则整块逐字原样——
`|` 单独一行、表头后没有分隔行、流式只写到一半，都退回普通文本（不猜、不吞字符）。口径：

- 分隔行每格必须 `:?-+:?`：`:--` 左、`--:` 右、`:-:` 中、`---` 左；表头与分隔行的**单元格数必须相等**；
- 首尾 `|` 可有可无（`| a | b |` 与 `a | b` 同义）；`\|` 是格内字面竖线；数据行没有未转义的 `|` 就结束表格；
- 列宽按**去掉行内标记后**的显示宽度取最大（CJK 算 2 列），**行宽恒 ≤ 正文列宽**，所以块层的折行不会再把它拆坏；
- 窄屏两级退让：先按自然宽度**比例收窄**（整数最大余数法分配，行仍铺满），放不下再**从右侧裁列**并在末尾标 `…`；
  单元格超宽时扣掉 `…` 的 1 列后按 grapheme 整簇裁（CJK 不劈开）；`width ≤ 4` 只留第一列且不打 `…`；
- 与 GFM 的三处刻意差异：数据行缺 `|` 就结束表格（GFM 当单列行）、表头/分隔行都必须带 `|`、只认 `\|` 一种转义。

折行口径：按**显示列**折（CJK 宽字符算 2 列，与 `width.rs` 同一套度量），词级折行 + 放不下的长词
（URL、路径）硬切。正文列宽 = 块宽 − 2 列标记列；续行在正文列内与行首前缀对齐（列表项、引用、
代码块的续行落在正文列而不是标记列下），并在正文列之外再缩进 2 格标记列。复制文本沿用软换行
`joiner`：同一条逻辑行的续行拼回空格，行首标记与缩进不进复制内容。

流式降级（正文可能停在半个标记上）：未闭合的围栏把它之后的内容全当代码块渲染，未闭合的
`**` / `` ` `` / `[` 原样落字（后面的字符不会被误认成新标记），认不出来的行原样当普通文本——
不报错、不吞字符、不 panic。

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
 Ctrl+c 退出 · Esc 取消 · Enter 发送 · Shift+Enter 换行 · PgUp 上翻 …     ← 快捷键栏（1 行，可点）
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
 │ ❯ 输入消息，Enter 发送，Shift+Enter 换行 · /help 看命令             │   ← 空输入时的幽灵文案
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

卡片正文固定含两段：确定性风险类别，以及门控判定（判定不可用时如实写「判定不可用」而不是
「拒绝」）。「本次会话总是允许」按工具名记入会话白名单，之后的同类调用不再上卡——
授权模式在提示框底边的信息行里常驻显示（§5.1 的 `模型 · 判定器 · 授权模式`）。

**三种授权模式（2026-09-24，`/permission` 切换，缺省 `auto`）**：

| 模式 | 门控 | 何时上卡 |
| --- | --- | --- |
| `auto`（缺省） | 只读工具由确定性层直接放行；其余问 Jev，**过门槛采纳「允许」即执行** | 判不了（不可用 / 分布太接近 / 置信度低），或命令命中**破坏性**标记（`rm -rf` 一类） |
| `ask`（每次询问） | **不问 Jev** | 除已「总是允许」的工具外，每次都上卡（**只读工具也要确认**） |
| `never`（从不询问） | **不问 Jev** | 从不上卡，直接执行（风险分类照旧上屏与记账） |

口径：**两个方向都卡门槛**——采纳「允许」与采纳「拒绝」都要求 `margin ≥ 0.15` 且
`confidence ≥ 0.5`（旧口径只卡拒绝，是因为那时「允许」也要走人工卡；`auto` 采纳即执行后不再成立）。
白名单在三种模式里都先生效：卡片上的「总是允许」是用户自己的授权，不被模式反过来作废。

**工具的风险类别**（`Tools::risk`，与「是不是只读工具」分开）：`bash` 看命令（
`gate::assess`，22 条破坏性标记）；`write_file` **覆盖已有文件算破坏性**、新建算「会改动工作区」；
`edit_file` 算「会改动工作区」（锚点必须唯一命中，找不到或多处命中都显式失败，不猜）。
只读三件之外的调用**一律过门控**——`bash` 即使命令恰好只读也不例外（它是能改东西的工具）。

## 6. 组件清单与代码映射

| 组件 | 状态 | 数据模型 | 文件 |
| --- | --- | --- | --- |
| 主题与色深 | 扩展 | `Theme`（+5 字段）、`ColorLevel` | `src/theme.rs` |
| 字形表 | 扩展 | `Glyph`、`Glyphs`、`ALL` | `src/glyphs.rs` |
| 块模型 | 扩展 | `Block`（+`Notice`）、`UserBlock`（+`timestamp`）、助手块走 markdown | `src/blocks.rs` |
| 助手正文 markdown | 新增 | `parse`（逻辑行 + 样式）、`wrap_lines`（按显示列折行 + 续行缩进） | `src/markdown.rs` |
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

`Esc` 的阶梯（沿用现有「先关最上面那层」的写法，2026-09-24 按用户口径改末档）：
① 有选择 → 清选择；② 有卡 → 关卡片；③ 选项列表开着 → 关列表；④ 补全列表开着 → 收列表（文本保留）；
⑤ 在研究视图 → 回主界面；⑥ 有在途轮次 → 取消这一轮；⑦ **其余什么都不做**——
退出只走 `Ctrl+C` 与 `/quit`。这样键位栏写「Esc 取消」就是字面意思，也不会一次误触把
没发出去的草稿丢掉（可用性审计把旧行为列为阻断项：空闲按 Esc 直接退出且丢草稿）。

### 7.2 新增动作与默认键位

| 动作 | 键位 | 上下文 | 说明 |
| --- | --- | --- | --- |
| `ToggleResearch` | `Ctrl+O` | `Always` | 主界面 ↔ 研究视图（首屏也可进；亦可点快捷键栏提示 / 首屏菜单项） |
| `InsertNewline` | `Shift+Enter` | `PromptFocused` | 插硬换行。`Shift+Enter` 需要终端支持 Kitty 键盘协议（**Windows Terminal 到 Preview 1.25 才支持**，稳定版会把 Shift+Enter 当普通 Enter 发出去）；`Ctrl+J` 是裸 LF，任何终端都送得出来，快捷键栏与幽灵文案把两条都写出来（`hint_key`），不让用户按一个必然被当成 Enter 的键 |
| 鼠标左键 | — | 命中区 | 点审批卡按钮 / 快捷键栏提示 / 首屏菜单项 = 执行该动作（与快捷键同一条 `apply` 通路） |
| 鼠标拖选 | — | 回滚区空白处 | 按住左键拖过文本 = 选中（反显）；松手即复制（见 §7.5）。命中区优先：卡片/补全行上的点击仍是点击 |
| `/jev` | — | `PromptFocused` | 手工问一次判定器：`/jev <判定什么> [--label A --label B] [--material 材料]`，缺省选项 `allow` / `deny`。走会话自带的判定器（与门控同一个），读数与 `jev ask` 同一份口径：**不采纳、不套门槛、不产生行动**；结果落一条 `manual_judgment`（见 [会话日志](session-log.md) §3） |
| `Ctrl+V` | `Ctrl+V` | `PromptFocused` | **不粘贴**（粘贴是终端的事，见 §7.5）：回一块通知说明这个终端该按哪个键，而不是静默无视。`Ctrl+Shift+V` 同样绑上 |
| `Enter`（补全语义） | — | 补全列表展开且输入框只有裸查询 | `Enter` **采纳高亮的候选**而不是提交半截命令；输入已经等于选中候选的完整名字（如 `/quit`）或已进参数区时，`Enter` 照旧提交 |
| 列表选中标记 | — | 补全列表 / 选项列表 | 用 `▸`（与工具块同形）而**不是**提示框的 `❯`：列表压在提示框上方，同一个标记会让人以为输入框被列表占了（可用性审计的实测） |
| `Esc` | `Esc` | 有选中时 | 只清掉选择，**不**取消当前轮（取消这一轮更重要，别让误触顺手取消掉）；没有选中时才走 §7.1 ③ |
| 滚轮上/下 | — | `Prompt` / `Research` | 等价 `ScrollPageUp/Down`：回滚区或研究面板带上/下翻页 |
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
- 输入框里以 `/` 开头的整行是**命令**，不进对话上下文（`commands.rs` 注册表 + 解析）：
  `/help`、`/quit`、`/clear`、`/research` 由界面直接执行；`/model [模型ID]`、
  `/permission [auto|ask|never|reset]`、`/status`、`/continue`、`/sessions`、`/resume <会话ID>`
  折成 `Effect::Command` 交宿主（会话状态在那边），结果以通知块上屏。
  认不出的命令回一条带 `/help` 提示的通知，**既不静默吞掉、也不当消息发给模型**。
  输入以 `/` 开头时提示框上方展开候选列表（模糊匹配 + `Up/Down` 选择 + `Tab` 填入 + 可点击）。
- **取值固定的命令**不带参数时展开**选项列表**（与补全列表同一块地方，`App::open_choices`）：
  `↑↓` 选、`Enter` 用选中的值执行该命令、`Esc` 关，其余按键照常（仍可直接打参数）。
  目前用它的命令：`/resume`（历史会话，值 = 会话 ID）、`/permission`（`auto` / `ask` / `never` /
  `reset`，当前模式标「当前」）、`/model`（当前模型 + 各预设缺省模型 + DeepSeek 官方模型名）。
  数据一律由宿主给（`Update::Choices`），界面只管交互——以后任何命令只要有固定取值就能接入。
- `Ctrl+O` → 切到研究视图；`Esc` → 回主界面；两屏共享同一个 `App` 状态（回滚区位置、面板偏移都保留）。
- 研究视图里 `paused == true` 时状态行显示「已暂停」；`StepOnce` 不清除暂停标记。

### 7.5 文本选择与复制（2026-09-24）

回滚区里按住左键拖过文本即选中（选中的格子加反显），**松手就复制**，不需要再按别的键。
口径：

- **坐标记在内容上**：两端都是「摊平后的行号 + 回滚区列号」，所以滚轮翻页、后面又有新块追加
  都不会让选择错位（`App::begin_selection` / `extend_selection` / `finish_selection`）。
- **摊平结果带一层缓存**（`Scrollback` 的 `LayoutCache`，键 = 内容版本号 + 块宽 + 字形档 + 动画帧号）：
  这是实测换来的，不是提前优化——一次摊平要把每个块重新排版（助手块要跑 markdown 解析 + 折行），
  ~4900 行的会话量到 **18 ms/次**；而拖选时「命中换算 + 每帧渲染」各要一次，合起来就是
  **选择框不跟手**（每个鼠标事件 ≈37 ms）。加缓存后：命中换算 **0.5 µs**、整帧绘制 **0.7 ms**。
- **点击与拖选分得开**：按下时若命中了登记区域（审批卡按钮、补全行、首屏菜单项）就走原来的
  点击通路；只有落在空白处才开始选。没拖出原位就只当一次点击，不留高亮、不复制。
- **取文本按块自己的可选范围裁**：每行用 `BlockLine::selection_text()`（`Selectable`），
  于是块标记列（`◆ `、`▸ `、行号列）不会混进复制内容；软换行的续行按 `joiner` 拼回上一行，
  不落多余的换行。整段完全空白时不给复制（`selected_text` 返回 `None`）。
- **写剪贴板用 OSC 52**（`ESC ] 52 ; c ; <base64> BEL`，`crates/jh-tui/src/terminal.rs`）：
  由终端自己写系统剪贴板，**不引依赖**、隔着 SSH 也能用。代价是**终端得支持**——Windows Terminal、
  iTerm2、kitty、WezTerm、alacritty 支持；tmux 需要 `set-clipboard on`；不支持的终端会静默忽略。
  因此回执只说「已复制 N 行 / M 字节（OSC 52；终端不支持时不生效）」，**不替终端打包票**
  （本 crate 无法探测对端是否真的写进了剪贴板）。
- 选择是**浮层**：只在已画好的缓冲区上改样式（`Modifier::REVERSED`），不动块模型、也不动
  复制用的文本——同一份 `Selectable` 元数据既服务高亮也服务导出。

**粘贴（与复制相对的另一半）**：粘贴动作由**终端**完成，应用侧读不到系统剪贴板
（OSC 52 只能写不能读，且读剪贴板在多数终端默认禁用——隐私考虑）。于是这里的做法是：

- 终端把内容粘进来时走**括号粘贴**（`EnableBracketedPaste`，`terminal.rs` 进屏时开）→
  `Event::Paste` → 原样插进提示框，**多行不会误提交**（换行插成硬换行，不是 Enter）；
- 终端**没**把它当粘贴、而是把 `Ctrl+V` 发给应用时（Windows Terminal 默认如此），应用回一块
  如实说明的通知：Windows Terminal 用 `Ctrl+Shift+V`，iTerm2 / kitty / WezTerm 用 `⌘V` 或
  `Ctrl+Shift+V`，mintty 用 `Shift+Insert`；
- 提示框的幽灵文案里直接写着粘贴键（`Enter 发送，Shift+Enter/Ctrl+J 换行 · Ctrl+Shift+V 粘贴 ·
  /help 看命令`，键位文案来自动作注册表，改键位文案跟着走）——不必先按错一次才知道；
- 首屏没有回滚区，通知改为画在居中面板下方（`draw_startup_notice`），否则首屏按 `Ctrl+V`
  会「没反应」。

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
| 六 | 会话可用性（2026-09-24）：LLM provider 预设（默认 DeepSeek）、取消轮内调用上限硬截断、`/` 命令补全列表、换行键退路、三种授权模式、写 / 编辑文件工具、会话日志与会话历史 + `/resume`（§9.0）、命令固定选项列表、markdown 管道表格、文本选择与复制、粘贴提示 | 已落地（逐条见 §9.2） |

### 9.0 会话历史与日志（2026-09-24 新增）

`jev chat` 的会话目录**默认**落在 `~/.jev-harness/sessions/<会话ID>/`（`JEV_HARNESS_HOME` 改根目录，
`--out-dir` 整体改道；取不到 `HOME` 时如实报一句再退回 `var/chat/<run>`）。目录里四份文件：

| 文件 | 内容 |
| --- | --- |
| `session.jsonl` | 完整会话日志（用户 / 模型 / 门控 / 审批 / 工具 / 错误，一行一个事件）；格式契约见 [`docs/session-log.md`](session-log.md) |
| `conversation.jsonl` | **完整对话记录**（用户消息 / 助手正文 / 工具调用 / 工具输出，`jh_openai::InputItem`），`/resume` 靠它接回上下文 |
| `meta.json` | 摘要：时间、工作区、模型、轮数、首末句话 |
| `evidence.jsonl` | 语义状态证据（既有） |

`/resume`（或 `/sessions`）在提示框上方弹出**选项列表**：`↑↓` 选、`Enter` 接回、`Esc` 关闭，
同时回一块文本回执（id 可复制，也支持 `/resume <会话ID>` 直接指名）。列表最多画 8 行、按更新时间倒序，
与 `/permission`、`/model` 共用同一套机制（§7.3）。

接回的是**对话上下文**与界面回放：用户消息、助手正文、每次工具调用（含输出）按当时的顺序
画出来（工具块行首用「●」并把状态标成「接回」——对话记录里没有当时的状态，不假装成功也不
假装失败），回放前有一条 `── 以下是接回的历史（id） ──` 分隔、之后是这一次的读数。
语义状态（判断节点、依赖图、失效传播）与授权白名单是进程内对象，**不跨进程恢复**——
回执里如实写明这条边界。

### 9.1 批次四的落地范围与实现口径

宿主落在 `crates/jh-app/src/session/**`（本 crate 仍不依赖 `jh-core`/`jh-app`）：

| 模块 | 职责 |
| --- | --- |
| `model` | LLM 侧：一次调用的抽象 + `jh-openai` 的 `/responses` 流式实现（future 不借用会话，可进 `select!`） |
| `tools` | 工具集：工作区内只读（`read_file` / `list_dir` / `search`）+ 写与编辑（`write_file` / `edit_file`）+ `bash` 命令执行 |
| `gate` | 门控：确定性风险层 + Jev 类型化判定（`shell_command_allowed`）+ 防抖 + 采纳门槛 |
| `approval` | 会话内审批策略：「本次会话总是允许」按工具名记入白名单；提示框信息行的授权模式 |
| `agent` | 会话状态机（模型 → 门控 → 审批 → 执行 → 回收），界面变化描述成 `Update` |
| `view` | 领域对象 → 判断详情 / 证据 / 传播三份视图模型与判定轨迹 |
| `tui` | 终端宿主：`EventStream` + 在途步骤 + 按需 tick 的 `select!` 循环、`Effect` 执行、恢复护栏 |

口径（批次四新定的六条，实现与文档同步）：

1. **审批卡覆盖面按授权模式走（2026-09-24 起，见 §5.4）**：`auto`（缺省）下只读工具由确定性层
   直接放行、其余问 Jev、过门槛采纳即执行，判不了或破坏性才上卡；`ask` 下除白名单外每次都上卡；
   `never` 下不上卡直接执行。批次四当时的口径是「只读工具与 `bash` 都上卡」（取严），
   三种模式落地后由 §5.4 覆盖。「本次会话总是允许」仍是按工具名记入白名单并在阶段行说明。
2. **门控三档**：确定性层先分类（只读 / 会改动工作区 / 破坏性，只写理由不裁决，只读工具到此为止）
   → Jev 问 `shell_command_allowed`（`usr:allow` / `usr:deny`，概率与 `confidence` 原样保留）
   → **过门槛才采纳，两个方向都卡**（`margin ≥ 0.15` 且 `confidence ≥ 0.5`）：`usr:allow` 在
   `auto` 下直接执行（其余模式上卡）、`usr:deny` 不上卡直接拒绝（理由与出路写在阶段行与工具块里）；
   判定器不可用、分布太接近或 confidence 不足一律标「判定不可用」并交人工裁决。
   **破坏性命令（`rm -rf` 一类）即使判定允许也上卡**（`auto` 下不交给单一判定层）。**「判定不可用」与「门控拒绝」在文案与语气上是两句不同的话**，
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

验收口径（批次二至四的实际读数；**2026-09-24 重测**）：`cargo test --workspace` 全绿（29 个 test target、
666 passed / 0 failed / 7 ignored；按 crate：`jh-tui` 271、`jh-app` 235、`jh-core` 92、`jh-openai` 41、`jh-jev` 27
（其中 9 条是文档测试；另 7 个真调用例标 `#[ignore]`）），
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
export OPENAI_API_KEY="$DEEPSEEK_API_KEY" OPENAI_BASE_URL=https://api.deepseek.com \
  OPENAI_MODEL=deepseek-flash
tmux new-session -d -s live -x 120 -y 40 \
  "cd <仓库> && target/debug/jev chat --workspace /tmp/m25live/ws"
tmux pipe-pane -o -t live 'cat >> /tmp/m25live2.log'
# 打字：请用 bash 跑 ls -1 列出工作区文件，再用一句话总结。→ Enter → y（允许一次）→ Ctrl+C
```

窗口尺寸（`-x 120 -y 40`，或 `script` 路径下的 `stty rows/cols`）是必需的：伪终端没有尺寸时
ratatui 画不出内容——循环照常跑、读数照常有，界面却是空的。
读取屏上内容用 `tmux pipe-pane` 把 pty 原始字节落盘再按转义切分：本机的 `tmux capture-pane`
对这些会话返回空白（**不是**应用没画——同一份字节流里内容齐全），记在这里免得下次白试。

读数（run `h1h2-1790162050626`，证据 `var/chat/h1h2-1790162050626/evidence.jsonl`，调用记录追加进
`var/calls.jsonl`）：

| 项 | 读数 |
| --- | --- |
| 轮数 / 退出 | 1 轮，`Ctrl+C` 退出，终端恢复，退出码 0，屏上 `用时 14.7s` |
| 模型调用 | 2 次 `deepseek-flash`：823 ms（in 799 / out 73 token）、727 ms（in 895 / out 75 token），累计 in 1694 / out 148 |
| 门控判定 | 1 次 `shell_command_allowed` → `real_api` / `jev-1.13.0`，353 ms，`usr:allow 0.99` / `usr:deny 0.01`，`confidence 0.99`，阈值 `margin 0.15` / `confidence 0.5`（写进判断记录），`request_id` `req_01a0cdf9…` |
| 工具与审批 | `bash ls -1`（确定性层：只读）→ 审批卡「允许一次」→ 真实执行，退出码 0，输出 `a.rs` / `b.md`，结果回填到工具块 |
| 证据 | `session.ask` v1（用户原话）、`shell.command` v1（命令 + cwd + 风险类别）、`tool.result` v1（状态 + 输出） |
| 屏上顺序 | 首屏 → 用户块（**只有一块**：`❯ 请用bash跑ls-1列出工作区文件，再用一句话总结。` + 右对齐时间戳 `11:14:13Z`）→ `⠋ 模型输出中` → `◆ 模型请求了 1 个工具调用` → `◆ 判定 353ms · 采纳 usr:allow 0.99 · margin 0.98 · confidence 0.99 · 倾向允许` → 判定块（两条概率条 + 采纳行）→ 审批卡（标题 `权限请求 · bash`、命令行 `ls -1`、两段理由、`y/a/n`）→ `◆ 已裁决：bash 允许一次` → `⠋ bash ls -1` → `▸ bash 退出码 0 …` → 助手正文 → `用时 14.7s` → `✓ 完成 · 判定 usr:allow 0.99 · margin 0.98 · confidence 0.99 · 确定 · deepseek-flash` |

复审（r1）修正后的复跑验证，都能在同一份字节流里核对：

- **转轮在动、收尾帧不再等按键**：整场除 `y` 与 `Ctrl+C` 外没有按键，字节流里出现 8 个不同的
  转轮帧（`⠋⠙⠦⠧⠴⠸⠹⠼`），`✓ 完成 …` 那一帧在无按键的情况下画出；
- **用户块只有一块**：文案 `请用 bash 跑 ls -1 …` 在字节流里只写了一遍。

真调与复审暴露并已修掉的问题（按发现顺序）：

1. 工具块曾画成 `▸ bash bash ls -1`、审批卡命令行也重复工具名——参数摘要与工具名拆成
   `Tools::detail`（块与卡片用）/ `Tools::display`（通知与证据用）；
2. 子进程摘除名单漏了 `DEEPSEEK_API_KEY`（本机真 key 的别名，一条 `env` 就能把它打进证据文件）；
3. **P1**：宿主从不喂 `TickDemand`，`poll_timeout()` 永远是空闲长睡（转轮停帧、收尾帧要等下一次
   事件）——`apply()` 结尾统一喂一次，且推进之后立刻回头画一帧；
4. **P1**：`App::apply(Submit)` 与宿主回显各画一次用户块——只留宿主那一块（带时间戳）。

key 自检：`var/calls.jsonl`、各 run 的证据文件与各次伪终端/管道日志里都没有 key 原文（逐一 `grep` 过）；
会话侧真 key 的别名 `DEEPSEEK_API_KEY` 也在 `tools.rs` 的子进程摘除名单里。

### 9.2 批次六（会话可用性）逐条回写

| 能力 | 落点 | 口径与边界 |
| --- | --- | --- |
| LLM provider 预设 | `crates/jh-app/src/provider.rs` | `JEV_LLM_PROVIDER` 选预设（缺省 `deepseek`），端点 / 默认模型 / 密钥变量一处解析；`OPENAI_BASE_URL` / `OPENAI_MODEL` 降级为覆盖，不再必填 |
| 轮内调用上限 | `session/agent.rs` | 撞上限不再硬截断：暂停并给出 `/continue` 续跑；`--max-rounds 0` = 不限 |
| `/` 命令补全 | `jh-tui/src/commands.rs` + `app.rs` | 输入 `/` 出候选（模糊子序列匹配），`↑↓` 选、`Tab` 填入、`Enter` 提交、可点击；最多画 6 行并标「还有 N 条」 |
| 换行键 | `actions.rs` | 缺省 `Shift+Enter`（需终端支持 Kitty 键盘协议），`Ctrl+J` 是任何终端都能送的退路；两个都写进幽灵文案与快捷键栏 |
| 三种授权模式 | `session/approval.rs` + `agent.rs` | 见 §5.4；`/permission auto\|ask\|never\|reset`，白名单在三种模式下都先生效 |
| 写 / 编辑文件工具 | `session/tools.rs` | `write_file`（覆盖已有文件算**破坏性** → 一律上卡；新建算会改动工作区）、`edit_file`（锚点必须唯一命中，找不到 / 多命中显式失败）；路径按父目录 canonicalize 后比对工作区前缀；单次 ≤256 KiB；新证据对象 `file.change` |
| 会话日志 | `session/log.rs` + `docs/session-log.md` | 一行一事件（12 类：会话起止 / 用户 / 命令 / 模型请求与回复 / 门控 / 审批 / 工具 / 错误 / 打断），长文本 16 KiB 截断并记原长度，**不含凭据**；写失败不静默 |
| 会话历史与 `/resume` | `session/store.rs` + §9.0 | 默认 `~/.jev-harness/sessions/<id>/`（`JEV_HARNESS_HOME` / `--out-dir` 可改道）；`conversation.jsonl` 是**完整对话记录**（助手正文也落盘——真调里踩过只落用户消息的坑） |
| 命令固定选项列表 | `jh-tui/src/app.rs`（`ChoiceList`） | `/resume`、`/permission`、`/model` 不带参数时弹选项列表：`↑↓` 选、`Enter` 用选中值执行该命令、`Esc` 关；数据由宿主给（`Update::Choices`），界面只管交互 |
| markdown 管道表格 | `jh-tui/src/markdown.rs` | GFM 子集：表头 + 分隔行 + 对齐，行宽恒 ≤ 正文列宽，窄屏先比例收窄再从右侧裁列标 `…`；认不出就整块逐字原样（口径见 §4.4） |
| 文本选择与复制 | `jh-tui/src/app.rs` + `terminal.rs` | 拖选 → 反显 → 松手即复制；文本按块的 `Selectable` 裁掉标记列；写剪贴板走 **OSC 52**（不引依赖、隔 SSH 可用；终端不支持时不生效，回执如实说明） |
| 粘贴 | `actions.rs` + `terminal.rs` | 粘贴由**终端**负责（应用读不到系统剪贴板）：括号粘贴已开、多行不误提交；`Ctrl+V` 到应用时回一块说明（各终端键位见 §7.5） |

批次六的验收读数（2026-09-24 重测）：`cargo test --workspace` = 29 个 test target、**666 passed / 0 failed / 7 ignored**；
`cargo clippy --workspace --all-targets` 零告警；`cargo fmt --all -- --check` 干净。真机（伪终端 120×40）逐条验过：
三种授权模式的上卡 / 不上卡与状态行一致、`write_file` + `edit_file` 真实改盘、覆盖已有文件弹卡且拒绝后文件逐字节未变、
`/resume` 回放含助手正文与工具调用、拖选复制的 OSC 52 base64 解出文本与选择逐字一致（`用来测试复制`）、
管道表格按列对齐渲染、`Ctrl+V` 在首屏与主界面都给出说明。**边界**（照实登记）：回退字形档下表格与列表标记不降级；
`/resume` 不恢复语义状态与授权白名单；选择只有鼠标、拖到区域外不自动滚屏；OSC 52 依赖终端支持；
取消硬截断后「轮内无上限」的用户可用 `--max-rounds` 自设。

## 10. 非目标与未确定项

**非目标**（与本批设计无关，且已有明确取舍）：

1. 多形态渲染（Fullscreen / Inline / Minimal）——先只做 alt-screen 全屏（调研报告 §7.2 第 1 条）。
2. 主题热切换、命令面板与模糊搜索（调研报告 §7.2 第 3、6 条与专题 17）——仍是非目标。
   **鼠标与剪贴板已交付（2026-09-24，批次六）**：左键点击审批卡按钮 / 快捷键栏提示 / 首屏菜单项走同一条
   动作通路（渲染期登记命中区，`ClickTarget`），滚轮上/下 = `ScrollPageUp/Down`（研究视图里滚面板带）；
   回滚区拖选文本 → 反显 → 松手即复制（OSC 52，见 §7.5）；命令的固定选项列表（`/resume` /
   `/permission` / `/model`）支持 `↑↓` 选 + `Enter` 执行。
3. 完整 Markdown 解析（嵌套引用、嵌套列表、HTML）、语法高亮、Mermaid 与内联图片（调研报告 §7.2 第 3 条）；表格只做了管道表子集，表格外的结构仍原样平铺。
   助手正文只做 §4.4 的轻量子集，不引 `pulldown-cmark` / `syntect`。
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
12. **选择只有鼠标**（批次六）：没有键盘选区（`Shift+↑↓` 之类），拖到回滚区外不自动滚屏；
    复制依赖终端支持 OSC 52（不支持的终端静默忽略，本 crate 探测不到）。
13. **选项列表没有鼠标点击**（批次六）：`↑↓/Enter/Esc` 可用，点击没做（补全列表有点击，
    两者机制同源，接一条 `ActionId` + 命中区即可）。
14. **回退字形档下 markdown 子集不降级**（批次六）：`•` / `▏` / `─` / `│` / `─┼─` / `…` 是硬编码
    字符，未走 `glyphs.rs`；要降级需要给 `parse` 传 `Glyphs` 并改 `blocks.rs` 的调用点。
15. **采纳门槛未校准**：`margin ≥ 0.15` 且 `confidence ≥ 0.5` 才采纳「拒绝」是**声明口径**
    （写进判断记录的 `thresholds`）。真调（§9.1）只走到 **allow 分支**（`usr:allow 0.99`），
    **拒绝分支**的门槛取值仍没有实测——要校它得凑出真实的高危命令场景。
16. ~~模型正文仍按纯文本画~~ **已定（轻量子集，§4.4）**：模型正文按 §4.4 的子集渲染；
    嵌套引用、`####` 以上的标题、四反引号围栏仍原样平铺；管道表格已按上面的子集渲染，其余排版待定。
17. **成本与观测器面板未落地**（批次四的另一半）：会话已经在记调用记录（`var/calls.jsonl`）
    与证据（`var/chat/<run>/evidence.jsonl`），但界面里还没有读它们的面板——留待批次五。
18. ~~`Shift+Enter` 未绑定~~ **已定（2026-09-24，批次六，用户口径）**：换行键改为 `Shift+Enter`
    （默认键；`Alt+Enter` 移除），`Ctrl+J` 保留为不依赖键盘增强协议的退路。代价与前提见 §7.2：
    `terminal.rs` 在进屏时推 Kitty 的 `DISAMBIGUATE_ESCAPE_CODES`（不先查询，避免 2 s 超时）；
    终端若不支持该协议（Windows Terminal 稳定版即如此，Kitty 协议要 Preview 1.25+），`Shift+Enter`
    与 `Enter` 同码——此时退路是 `Ctrl+J`，并且**提示文案同时写出两条键**（`ActionDef::hint_key`，
    幽灵文案也由它生成），用户不必猜哪个键能用。

**基线缺陷的修正记录**（`3ddd519` 自带、批次四的真调与评审暴露，均已修）：

1. **提示框静默吞掉大写字母与部分符号**（真调实拍：`abc DEF ghi` 进证据只剩 `abc  ghi`）。
   成因：`KeyChord::new` 把 ASCII 大写的 SHIFT 归一化进字符本身（`actions.rs` 的既有设计，为的是
   「同一物理按键不因事件带不带 SHIFT 而落到不同动作」），而 `App::handle_key` 的插入分支只减
   Control/Alt/Super 就要求「修饰键恰好为空」——大写字母于是既匹配不上键位表、也插不进提示框。
   修法：插入分支只把 SHIFT（打字用的修饰键）减掉；减完还剩 Control/Alt/Super 的才算未绑定的
   组合键、不塞字符。
2. **未绑定的 `Ctrl+小写字母` 会往提示框里塞字符**（同一条路径的反向错误）：同一处一并修。
3. 两处都补了单测（大写字符的两种送达形态、带 Shift 的符号、未绑定组合键），并做了变异确认会红。
   **这两条是基线就有的缺陷**，不是批次四引入的。
