# Grok Build TUI 设计调研报告

> **调研对象**：SpaceXAI 的 **Grok Build**——Rust 编写的终端 AI coding agent，快照位于 `reference/grok-build/`；其 TUI 主体是 `crates/codegen/xai-grok-pager`。快照版本 SOURCE_REV `9bb727ccdff0a793ee73bcde4e2e09cbef6b5387`（已与 `reference/grok-build/SOURCE_REV` 核对一致）。
>
> **调研日期**：2026-09-22（全篇事实与数字均以该时点、该快照为准；引用的 crate 版本号是当时锁文件的取值，落地前须重新核对）。
>
> **调研方法与流水线**：28 个只读子代理逐行取证（每个专题限定证据范围，逐条给出 `路径:行号`）→ 8 篇分节稿按主题提炼去重（`final-A.md` … `final-H.md`）→ 合稿并撰写本节（前置部分与执行摘要）。
>
> **原料位置**：`docs/research/grok-tui/`（28 篇专题 `01-*.md` … `28-*.md`，8 篇分节稿 `final-A.md` … `final-H.md`）。
>
> **事实纪律**：本报告只使用上述原料文件中已有的事实、数字与路径，**不引入原料之外的任何新事实**；**未对 Grok Build 做任何编译或运行验证**（没有执行任何 `cargo` 命令，没有跑过该 TUI），全部结论均来自静态阅读，每个论断都带 `路径:行号` 或分节稿章节号证据；上游标注「未找到证据」的条目在正文中保留原标注，不升级为结论。
>
> **路径约定**：正文中的源码路径一律省略 `reference/grok-build/` 前缀。

## 可信度标记约定

本报告严格区分四类论断，逐条就近标注：

- **【官方陈述】**＝面向用户/贡献者的明示表述（`README`、`docs/user-guide/**`、`docs/tutorial/**`、`CHANGELOG`），或源码注释中显式写明理由的。专题文件 28 把「源码注释」单列为第三档【代码注释】，本报告按 §2 的口径把两者并入【官方陈述】（§2 的每一节声明与此一致）。
- **【第三方】**＝仓库快照之外的第三方材料。**本次调研没有使用任何此类材料**——28 篇专题的证据范围全部限定在仓库快照自身（含其自带文档、`CHANGELOG` 与 vendored 源码的 `NOTICE`），因此正文与执行摘要中不会出现该标记；此处保留定义，以免与同族报告 `docs/jev-research.md` 的标记体系混淆。
- **【归纳】**＝从实现中读出的模式，官方未明说，**不得当官方立场引用**。§2.2 的十条模式与执行摘要中带此标记的条目都属于这一档。
- **未找到证据**＝上游专题明确标注「未找到证据」或「未确定」的条目。本报告保持原标注，仅作线索，不作决策依据。

只给出 `路径:行号` 而不带标记的句子，表示对源码实现的中性描述（不含设计意图判断）。引用具体常量、阈值与数值时照抄上游口径，不改写、不换算。

## 术语约定

以下专有名词在报告中反复出现，定义取自原料中对同一机制的描述，「展开」列给出本文档中详细讨论该词的章节。

| 术语 | 定义与展开位置 |
| --- | --- |
| **block / entry / row（块 / 条目 / 行）** | Scrollback 流水线的三段数据抽象：块（`RenderBlock`，13 个变体）是纯函数 `output(ctx) -> BlockOutput`；条目（`ScrollbackEntry`）是块 + 显示模式 + 多层缓存；行（`BlockLine`）是**已按内容宽度换行后的屏幕行**，再往下才是最终的 `Buffer` 单元格。展开见 §4.2（`09-scrollback-block-model.md` §1；`block.rs:40, 298`、`entry.rs:57`、`types.rs:146`）。 |
| **scrollback** | 既指 Grok 全屏形态下那个「应用内历史区」面板，也指它背后的整套渲染管线；它自己拥有滚动、折叠、选择与鼠标交互，而不是把这些交给终端。展开见 §2.1（历史归属）与 §4.2（管线）。 |
| **alt-screen（备用屏 / alternate screen）** | 整块替换终端画面、退出后恢复原画面的渲染形态；`ScreenMode::Fullscreen` 即使用备用屏，是 Grok 的默认形态。展开见 §2.1、§5.6。 |
| **inline（内联模式）** | 不占备用屏、动态视口贴在终端底部并保留上方历史的模式（`--no-alt-screen`；tmux control mode 与 Zellij 下自动选中），定位是备用屏不可用或不可靠时的**降级目标**，不是官方首选。展开见 §2.1、§4.2。 |
| **native scrollback（终端原生滚动缓冲）** | 终端自己持有的历史；它 **append-only**，应用无法在已打印内容之上插入新块——「committed frontier（已提交前沿）」与「待输入块绝不提交」两条规则都出自这条约束。展开见 §2.2 模式 6、§2.3 放弃项 5。 |
| **minimal（`--minimal` / scrollback-native）** | 第三条实验分支：定稿块经 `insert_before` 一次性打印进终端原生 scrollback（「the terminal owns history」），界面只保留底部固定 live region（`minimal_live_rows`，默认 10 行）。展开见 §2.1 图 1、§2.3。 |
| **pane** | 不是可拖拽分栏，而是**共享同一条横向带、由可见性与单一 `ActivePane` 焦点枚举切换的 overlay 面板**；开关面板只改变高度，不改变主内容区的水平布局。展开见 §4.1。 |
| **dock** | 一个远程开关特性面板（默认关），启用后吞掉 Tasks 与 Queue 两个旧面板（高度归零），并把队列正文搬进 dock 内部；内部按纯函数 `grant_rows` 分预算，无 tab 无栈。展开见 §4.1、§4.6。 |
| **status line** | prompt 之下、shortcuts bar 之上、占 0–5 行的一行式状态面；在 5 个屏幕状态面（status bar / scrollback / turn_status / status line / shortcuts bar）中，它是**唯一用户可配置**的一个，但没有模板引擎。展开见 §4.5。 |
| **turn（轮次）** | 一次用户提交到 agent 收尾的完整过程，被切成四段：乐观本地回显 → prompt ack 窗口 → 流式运行 → 收尾。展开见 §5.5。 |
| **interjection（插话）** | 运行中再次输入的三种语义之一：普通 `Enter` 入队、Send-now 取消当前轮并立刻发，而真正的 `Action::Interject` 只由队列行编辑与计划评审意见产生。上游材料只出现 interjection 一词，**未出现 steering**；展开见 §5.5、§5.1。 |
| **elicitation（MCP elicitation）** | MCP 服务器向用户索取信息的交互；它与工具授权、计划批准、澄清提问、敏感操作同意一起收敛为同一种「阻塞卡」呈现。展开见 §5.4。 |
| **阻塞卡（blocking card）** | 五类需要用户裁决的交互的统一骨架与键盘所有权机制：左侧 1 列 `accent_bar`、`content_x = area.x + 3`、底部固定动作行，失焦整体降饱和并撤掉焦点行背景带；`blocking_card()` 是唯一权威判定，permission 可抢断其他卡片。展开见 §5.4。 |
| **ActionId / 动作注册表** | 动作身份是编译期 enum（76 个变体）而非字符串；全部默认键位在 `default_actions()` 一处注册，键位、命令面板、快捷键手册与遥测共用同一套身份。展开见 §5.1。 |
| **OSC 8** | 终端超链接转义，是 Markdown 链接与命令输出链接可点击的来源（`id=` 由终端层重铸，URL 先剥离控制字符）。展开见 §3.4、§5.2。 |
| **OSC 11** | 查询/设置终端背景色的转义；Grok 只在**启动期**用它做背景明暗探测，**不是**色彩能力探测（色彩能力走 `supports-color` 与品牌白名单）。展开见 §3.1、§5.7。 |
| **OSC 12** | 设置光标颜色的转义；Grok **只写不查**，复位用 OSC 112 且受「本 session 是否画过」门控（避免无端复位让 Ghostty 锁死光标色）。展开见 §4.4、§5.7。 |
| **OSC 52** | 把文本写入终端剪贴板的转义，是复制三条 leg（native / tmux `load-buffer` / OSC 52）中的一条，也是 SSH 场景的主要路径。展开见 §5.2。 |
| **DEC 2026** | 同步输出（synchronized update）开关，Grok 用它成对包裹每一帧以避免撕裂；在 tmux 直接终端时**刻意不发**（tmux 自己整屏重绘并已对外同步）。展开见 §5.7、§6.2。 |
| **DA1** | `CSI c` 主设备属性查询。Grok 把它用作**退出时的清退栅栏**（`pop_fence`）：pop 掉 kitty 键盘标志后写 DA1 并排空 stdin，防止残留按键上报漏进用户 shell；相邻概念 DA2（`CSI > c`，读终端版本，仅 Alacritty 且非 CSI 拦截型复用器）用于 kitty flags 降级判断。展开见 §5.6、§5.8、§5.1。 |
| **DCS passthrough** | 把转义序列包进 tmux 的 DCS 信封再转发给外层终端。Grok 只把它当包装 helper（只做 ESC 双写，不负责净化载荷），且启用条件逐项不同——通知不看版本、进度条与 OSC 11 要求 tmux ≥ 3.3。展开见 §5.7。 |
| **Kitty keyboard protocol（KKP）** | 终端把修饰键与按键 release 事件以 CSI-u 形式上报的协议。Grok 只推 `DISAMBIGUATE_ESCAPE_CODES`，并按 DA2 版本决定是否叠加 `REPORT_EVENT_TYPES`（Alacritty ≤ 0.24.1 不叠加）；代码区分「推了标志」与「能收到 release」两个独立谓词。展开见 §5.1、§5.8。 |

## 目录

- [可信度标记约定](#可信度标记约定)
- [术语约定](#术语约定)
- [1. 执行摘要](#1-执行摘要)
- [2. 设计理念与原则](#2-设计理念与原则)
  - [2.1 官方明确陈述的立场](#21-官方明确陈述的立场)
  - [2.2 从实现中归纳出的设计模式](#22-从实现中归纳出的设计模式)
  - [2.3 被刻意放弃的方案](#23-被刻意放弃的方案)
- [3. 视觉语言与设计美学](#3-视觉语言与设计美学)
  - [3.1 色彩与主题系统](#31-色彩与主题系统)
  - [3.2 字形、图标与边框符号](#32-字形图标与边框符号)
  - [3.3 排版、留白与信息密度](#33-排版留白与信息密度)
  - [3.4 Markdown 的终端化呈现](#34-markdown-的终端化呈现)
  - [3.5 代码高亮与 diff 的呈现](#35-代码高亮与-diff-的呈现)
  - [3.6 动效、加载态与流式呈现](#36-动效加载态与流式呈现)
  - [3.7 图形化内容：Mermaid、内联图片与视频](#37-图形化内容mermaid内联图片与视频)
- [4. 界面结构与布局](#4-界面结构与布局)
  - [4.1 整体布局与 pane 模型](#41-整体布局与-pane-模型)
  - [4.2 Scrollback 的块模型与渲染管线](#42-scrollback-的块模型与渲染管线)
  - [4.3 模态、浮层与焦点栈](#43-模态浮层与焦点栈)
  - [4.4 输入区（prompt widget）](#44-输入区prompt-widget)
  - [4.5 状态面：status bar / status line / context bar](#45-状态面status-bar--status-line--context-bar)
  - [4.6 侧边与辅助面板](#46-侧边与辅助面板)
  - [4.7 首屏、空状态与渐进式披露](#47-首屏空状态与渐进式披露)
  - [4.8 多会话界面与 dashboard](#48-多会话界面与-dashboard)
- [5. 交互逻辑](#5-交互逻辑)
  - [5.1 键盘模型与动作注册表](#51-键盘模型与动作注册表)
  - [5.2 鼠标、选择与剪贴板](#52-鼠标选择与剪贴板)
  - [5.3 命令、补全与搜索](#53-命令补全与搜索)
  - [5.4 审批、提问与确认](#54-审批提问与确认)
  - [5.5 Agent 轮次交互：流式、插话、队列与取消](#55-agent-轮次交互流式插话队列与取消)
  - [5.6 会话生命周期与终端状态管理](#56-会话生命周期与终端状态管理)
  - [5.7 宿主集成：通知、标题、OSC 与终端复用器](#57-宿主集成通知标题osc-与终端复用器)
  - [5.8 无障碍、自适应与终端能力协商](#58-无障碍自适应与终端能力协商)
- [6. 技术栈与工程实现](#6-技术栈与工程实现)
  - [6.1 自研 widget 与组件抽象](#61-自研-widget-与组件抽象)
  - [6.2 渲染管线与性能架构](#62-渲染管线与性能架构)
  - [6.3 依赖、版本与 feature 门控](#63-依赖版本与-feature-门控)
  - [6.4 工程化：测试、快照、基准与代码组织](#64-工程化测试快照基准与代码组织)
- [7. 对本项目（jev-harness）的启示](#7-对本项目jev-harness的启示)
  - [7.1 可直接借鉴的做法](#71-可直接借鉴的做法)
  - [7.2 需要谨慎或代价过高的做法](#72-需要谨慎或代价过高的做法)
  - [7.3 建议的最小 TUI 骨架](#73-建议的最小-tui-骨架)
- [8. 附录](#8-附录)

## 1. 执行摘要

> 16 条，每条一句；只保留会影响本项目技术选型与架构决策的结论。括号内是主要证据路径：`final-*.md` 指本目录的分节稿，`NN-*.md` 指本目录的专题稿，其余为 `reference/grok-build/` 内的源码路径（已略去前缀）。

1. **Grok Build 是 SpaceXAI 的终端 AI coding agent，TUI 只是同一个 agent 运行时的一层表现层；它的默认形态是全屏 alt-screen，内联（`--no-alt-screen`）是备用屏不可用或不可靠时的降级目标，`--minimal`（scrollback-native）是第三条实验分支**——因此「Grok 选择内联滚动而非全屏备用屏」是一个常见误判，引用前必须先修正。（【官方陈述】`28-design-intent-evidence.md` §1.2–§1.3；`final-H.md` §2.1 图 1；`app/mod.rs:340-348`）
2. **量级比本项目该做的规模大一到两个数量级：TUI 主体 `xai-grok-pager` 有 652 个 `.rs` 文件，表现层原语抽成 `xai-grok-pager-render`（47 条直接依赖），组合根 `xai-grok-pager-bin` 的 `main.rs` 就有 3,774 行，另有滚动区模式 crate `xai-grok-pager-minimal`；workspace 共 101 个 member、`Cargo.lock` 1347 个 package、110 个 crate 名存在多版本；但真正值得学的是工程纪律——每条跨模块约定都配一个会失败的测试（元测试用 `include_str!` 读入 Markdown 再按反引号切表格单元做集合运算），快照被刻意限制在 11 个纯文本 insta `.snap`，PTY e2e 由独立 crate 承载且刻意不被 pager 依赖。**（`final-G.md` §6.3–§6.4；`27-engineering-practices.md` §1、§11.1；`26-dependency-stack.md` 关键结论 11；`status_line_tests.rs:510-515`）
3. **视觉语言的统一手法是「单一来源 + 显式降级」**：全部颜色只来自扁平 `Theme` 结构体的 58 个 `Color` + 6 个 `Modifier` 字段，主题是「每主题一个 `const fn` + 内联 `mod palette`」编译进二进制而非资源文件，色深降级四档 `None < Basic < Ansi256 < TrueColor`（`NO_COLOR` 优先）；字形有 34 个 `pub fn` 的单一来源且每个 `(fancy, fallback)` 对都断言同宽，唯一字形降级开关是 legacy Windows ConHost（default-deny），**不存在**「非 Unicode 终端 → ASCII」的通用降级；排版**没有全局设计令牌模块**（参数分 `LayoutConfig` / 各类 `*Config` / 视图局部 `const` 三层），缩进一律以终端显示列度量。（【归纳】`final-A.md` §3.1–§3.3；`theme/mod.rs:1`、`theme/color_support.rs:17-30`、`glyphs.rs:406-424, 426-558`、`appearance/config.rs:168`）
4. **文本渲染的边界划得很清楚**：Markdown 只借 `pulldown-cmark 0.13` 出事件流、渲染完全自研，对接面是 `Vec<Line<'static>>` 加四个并列元数据数组（**没有自定义 ratatui `Widget`**），语法标记靠 `Effects::HIDDEN` 隐藏而不是删文本，流式只在顶层块边界冻结；代码高亮用 `syntect` + build 期合成的 `syntaxes.bin`，语言判定只有文件扩展名与显式语言 token 两条路（无 shebang、无内容嗅探）；diff 不画 `+`/`-` 行首符号、全仓不存在并排模式，增删只靠行号颜色与内容背景带，并有 2 MiB / 50000 行两道全文件高亮硬上限。（`final-A.md` §3.4；`final-B.md` §3.5；`output.rs:50-67`；`scrollback/blocks/tool/edit.rs:44-49, 85-87`）
5. **动效被压到最低限度且代码里不存在 reduce-motion 概念，图形内容走三条互不依赖的路径**：真正逐帧的动画只有 spinner 帧序列、运行中块的 accent 波形、脉冲亮度与换帧四类，统一走 `tick / DIVISOR` 降帧（事实标准 `SPINNER_DIVISOR = 4`），空闲时事件循环完全不 arm tick（零唤醒）；「图形」分 Mermaid 文本制表符图、Kitty/iTerm2 post-flush 像素层、按需 PNG 三条路径，`sixel` 全仓库零命中，渲染出的 Mermaid PNG 从不内联显示，媒体在 scrollback 里是「单元格占位 + 帧后贴像素」。（`final-B.md` §3.6–§3.7）
6. **布局是单列垂直堆叠：整屏只调用一次 `Layout::vertical`，横向没有分栏，唯一弹性行是 scrollback 且 `SCROLLBACK_MIN_ROWS = 5` 是它唯一地板（行数不够时被压的是别的行）；所谓 pane 不是可拖拽分栏，而是共享同一条横向带、靠可见性与单一 `ActivePane` 枚举（7 值）切换的 overlay 面板**，面板开关只改变高度、不改变主内容区的水平布局，高度阈值集中在 `16 / 20 / 28` 三档、宽度阈值 `60 / 40 / 10` 三档并用 `const _: () = assert!(...)` 锁死大小关系。（`final-C.md` §4.1；`final-F.md` §5.8；`views/agent.rs:89-104, 108-176, 181-299`）
7. **Scrollback 最关键的架构决策是「块产出的已经是换行后的屏幕行」**：数据流为块 → 条目 → 行 → 单元格，布局层因此只做高度算术与可见窗口切片，不参与文本排版；每行自带 `Selectable` / `selection_range` / `selection_text` / `joiner` 四类元数据，一套数据同时服务绘制、搜索、复制与导出；虚拟化靠 `LayoutCache.virtual_y` 前缀和 + `partition_point` 二分，每帧只遍历可见窗口，高度是「先估算 → 接近视口才精确测量」两级制。（【归纳】`final-C.md` §4.2；`09-scrollback-block-model.md` 关键结论 3–6；`render.rs:498`、`types.rs:146-169`、`state/layout.rs:42, 1568`）
8. **模态与焦点的实现与常见做法相反：没有「浮层栈」这个对象——真正的模态是单槽位 `Option<ActiveModal>`，多浮层的 z 序由渲染函数里 `if … return` 的分支顺序定义（因此这些分支实际互斥）；键盘归属是显式排名表 `KeyOwner`（Permission 永远最高）加 8 档 `EscStep` 阶梯，快捷键栏提示与实际按键处理读同一张表**；嵌套只靠三种手法（同一 popup 内换内容 / 后画覆盖前画 / 快照回退），真正的「排队」只有 permission 队列（`VecDeque`，判定只看 `front()`）。（【归纳】`final-C.md` §4.3；`final-H.md` §2.2 模式 4；`key_owner.rs:55-91, 96-105, 110-132`）
9. **键位只有一处来源：编译期 `ActionId` enum（76 个变体、无字符串名）与 `default_actions(screen_mode, …)` 单点注册（默认 70 个 `ActionDef`），且官方手册明说绑定 built in、不能重映射**——`actions/defaults.rs:901-902` 声称可经配置重绑的注释与实现不符（代码里没有任何读取键位配置的路径）；同时没有多键序列、没有 leader key、注册期冲突校验为零，冲突只在速查表渲染期去重兜底。（【官方陈述】`final-E.md` §5.1；`actions/defaults.rs:1-3, 56`；`docs/user-guide/03-keyboard-shortcuts.md:3`）
10. **五类裁决交互（工具授权 / 计划批准 / 澄清提问 / MCP elicitation / 敏感操作同意）收敛为统一的「阻塞卡」，`blocking_card()` 是唯一权威判定且 permission 可抢断其他卡片**；风险分级**不靠颜色**（靠措辞、固定顺序与危险命令被 pin 到完整命令）；默认光标在无粘性选择时会落到 YOLO（always allow on all sessions）行，但降级方向安全（具体目标行缺失时退到 `AllowOnce`）；失败策略按路径分裂——本地 TUI 权限卡无超时、无限等待，consent gate 所有失败路径 fail open，headless 非 yolo 与 hub 远程权限路径 fail closed；`Esc` 不是拒绝。（`final-E.md` §5.4；`key_owner.rs:96-105`）
11. **轮次语义切得很细：一次轮次被切成四段（乐观本地回显 → prompt ack → 流式运行 → 收尾），发送后唯一被设界的是 ack 而不是 RPC（软提示 10 s、硬放弃 120 s，且 soft 被强制 ≤ hard/2）；运行中再次输入是三分法（普通 `Enter` 入队 / Send-now 取消当前轮并立刻发 / 真正的 `Interject` 只由队列行编辑与计划评审意见产生）；`Esc` 永不取消回合、`Ctrl+C` 才取消，取消粒度只有 `Turn` 与 `Compaction` 两种、没有「取消当前工具」，且任何等待都不阻塞输入（三套超时兜底都跑在动画 tick 上）。**（`final-F.md` §5.5；`app/prompt_ack.rs:14-24, 48`；`app/agent_view/prompt.rs:747-821`；`xai-grok-telemetry/src/events/turn.rs:38-43`）
12. **终端状态还原是这一层最硬的部分：终端接管是一张一次性副作用清单（raw mode 最先，panic hook 与信号处理器紧随其后），「向终端写什么」只有 `emit_terminal_teardown_sequences` 一个真源却有三条调用路径（事件循环后 / panic hook / 信号），且所有等待都有界（writer join 2 s、reader join 250 ms、DA1 fence 1 s、exit 看门狗 20 s + 5 s）；宿主集成侧只支持 5 种终端字节协议、没有任何系统级通知 API，tmux 包装条件逐项不同——DEC 2026 在 tmux 直接终端时反而刻意不发，「一刀切必然错一半」。**（`final-F.md` §5.6–§5.7；`terminal_restore.rs:61-134, 143-185`；`notifications/protocol.rs:6-47`；`terminal/tmux.rs:38-57`）
13. **技术栈结论：底座是 `ratatui 0.29` + `crossterm 0.28` 的生态事实标准组合，Grok 自研其中三层——`xai-ratatui-inline` 是 ratatui `Terminal` 的 fork（补的是上游不暴露的视口面积/位置、背缓冲与 resize 内部状态），`xai-ratatui-textarea` 不是上游 `tui-textarea`（是否源自某个 fork「未找到确证」）；全仓库只有 6 处 ratatui widget trait 实现，组件复用主要靠自由函数与自研 trait；渲染按需驱动（`biased` 排序的 `tokio::select!` + 每轮循环末尾唯一一次 `present_if_dirty`，空闲时终端收到零字节，「要不要重绘」是 `dirty` ∧ 无在途未确认帧 ∧ writer 队列已排空的三条件与），并刻意绕过 ratatui `try_draw()` 以保住光标闪烁。**（`final-G.md` §6.1–§6.3；`xai-ratatui-inline/NOTICE:1-9`；`event_loop.rs:579-600, 2110, 2938`；`draw.rs:689-706`）
14. **启示一「方向可抄、量级要砍」**：三形态渲染（及其两套 commit pipeline）、自研 inline `Terminal` fork、1347 个 package 的依赖面、元测试与 PTY e2e 全家桶、远端容错体系，就本项目现状都属过度设计；Grok 的取舍方向与本项目「奥卡姆剃刀 / 模块从简 / 不做过度防御 / 决策分层 / 概率优先」一致，但量级差一到两个数量级。（`final-H.md` §7.2；`26-dependency-stack.md` §7.1）
15. **启示二「可直接借鉴的机制」**：单一动作注册表 + 编译期 `ActionId`（确定性路由 → Jev 判定 → LLM 决策最终都产出同一个 `Action`）、块是纯函数 `output(ctx)`（把 Jev 的 Choice 概率 / confidence 放进 `ctx` 或块的 `accent`，让「不确定」直接上屏而不塌缩成布尔）、行级元数据一次产出供四处复用（绘制 / 搜索 / 复制 / 导出）、按需渲染三条件闸门 + 空闲零字节、门控必须带理由并给出出路（照抄 `Remedy::SwitchMode { why }` 的形状）。（`final-H.md` §7.1；`actions/mod.rs:34-36, 240-272`；`types.rs:146-169`；`event_loop.rs:579-600`；`slash/mode_support.rs:3-15`）
16. **启示三「保留的防御与起步骨架」**：终端状态泄漏（raw mode、备用屏、鼠标捕获、子进程继承）是少数值得保留防御的地方——信号触发的恢复与 one-shot env 覆盖的消费分别防的是「进程已死但终端仍是 TUI 状态」与「每个子进程都继承一个用户没要的强制屏幕模式」；起步骨架是单 crate 5 模块（`app.rs` / `actions.rs` / `blocks.rs` / `render.rs` / `terminal.rs`），先选定 alt-screen 全屏并用 ratatui 自带 `Terminal`，按「能跑 → 不阻塞 → 概率上屏 → 才考虑历史归属」四期推进。（`final-H.md` §7.1 第 10 条、§7.3；`signal_handler.rs:3-5`；`screen_mode_relaunch.rs:268-269`）

---

## 2. 设计理念与原则

> 证据分级沿用专题文件 28 的三档约定：**【官方陈述】**＝面向用户/贡献者的明示表述（`README`、`docs/user-guide/**`、`docs/tutorial/**`、`CHANGELOG`），或源码注释中显式写明理由的；**【归纳】**＝从实现中读出的模式，官方未明说，**不得当官方立场引用**。路径均省略 `reference/grok-build/` 前缀。

### 本节要点

- **前提修正**：官方没有「选内联滚动而非全屏备用屏」这条信条。默认形态是全屏 `alt-screen`，内联是备用屏不可用或不可靠时的降级目标，第三条 `--minimal`（scrollback-native）是实验分支（`crates/codegen/xai-grok-pager/src/app/mod.rs:340-348`、`docs/user-guide/21-terminal-support.md:174-179`）。
- 2.1 每条都带英文原句 + 中文翻译 + 路径，且只收官方陈述；2.2 的十条模式全部标【归纳】。
- 复用价值最高的四条模式：单一真理来源、不阻塞输入、焦点显式所有权、只追加约束反向决定架构。
- 2.3 的「刻意放弃」几乎都有源码注释留痕，理由多与「影响子进程或用户环境」有关，而不只是性能。
- 上游仍存在文档-代码不一致与若干「未找到证据」项（见 2.3 末尾），引用时须保持原标注，不要升级为结论。

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

## 3. 视觉语言与设计美学

> 来源专题：`01-theme-color-system.md`、`02-glyphs-and-iconography.md`、`03-typography-spacing-density.md`、`04-markdown-rendering.md`。
> 路径简写：`theme/*`、`render/*`、`glyphs.rs`、`util.rs` 属 `crates/codegen/xai-grok-pager-render/src/`；`parse.rs`、`render.rs`、`style.rs`、`checkpoint.rs`、`streaming.rs`、`output.rs`、`hyperlinks.rs` 属 `crates/codegen/xai-grok-markdown/src/`（`lib.rs` 特指 `xai-grok-markdown-core/src/lib.rs`）；`views/*`、`scrollback/*`、`app/*`、`slash/*` 属 `crates/codegen/xai-grok-pager/src/`。

### 3.1 色彩与主题系统

#### 本节要点
- 全部颜色只有一个来源：扁平 `Theme` 结构体的 **58 个 `Color` + 6 个 `Modifier`** 字段，注释明写 "All colors come from the `Theme` struct. No hardcoded colors elsewhere."（`theme/mod.rs:1`、`theme/tokyonight.rs:1`）。
- 主题**不是静态表、不是资源文件**，而是「每个主题一个 `const fn` 构造函数 + 内联 `mod palette` 色值表」，编译期常量直接进二进制（`theme/groknight.rs:15-52, 55-137`）。
- 降级链是四档 `None < Basic < Ansi256 < TrueColor`，探测顺序为 `NO_COLOR` 优先 → `supports-color` → 非 TTY 假设 truecolor → 终端品牌白名单升级（`theme/color_support.rs:17-30, 83-127, 246-269`）。
- `Theme::current()` 每帧重建主题（无脏标记、无缓存失效逻辑），所以热切换只需改一个 `AtomicU8`，代价是每帧 58 次 `quantize_color`（`theme/mod.rs:205-289, 293-333`）。
- 运行时写入的唯一入口是 `Theme::apply_kind`：先按 truecolor 能力钳制主题，再连带同步 Markdown 色彩极性（`theme/mod.rs:348-363`、`theme/cache.rs:89-100, 137-146`）；**OSC 11 不是色彩能力探测**，而是背景明暗探测且严格限于启动期（`theme/osc11.rs:13-14, 23, 65-74`）。

```
┌─ 第 1 层：调色板常量（每主题一个内联 mod palette）──────────────┐
│  pub const BG: Color = rgb(20, 20, 20);   // #141414            │
└──────────────────────────────┬──────────────────────────────────┘
                               │  use palette::*;
┌──────────────────────────────▼──────────────────────────────────┐
│ 第 2 层：Theme 结构体（58 个语义 role + 6 个 Modifier）          │
│  bg_base / text_primary / accent_tool / diff_insert_bg / md_code │
└──────────────────────────────┬──────────────────────────────────┘
                               │  Theme::current() → quantized(level)
┌──────────────────────────────▼──────────────────────────────────┐
│ 第 3 层：使用点。700 处调用 Theme::current()（其中 pager 633 处）│
└─────────────────────────────────────────────────────────────────┘
```

**内置主题**：`ThemeKind` = 6 个具体主题 + `auto` 元主题；判别值本身是载荷（缓存按 `u8` 存，`theme/cache.rs:18, 36-47`）。

| Kind | 判别值 | 规范名 / 别名 | 明暗 | `requires_truecolor` | 备注 |
| --- | --- | --- | --- | --- | --- |
| `GrokNight` | 0 | `groknight` / `grok-night`, `dark` | 暗 | false | 默认主题（`theme/mod.rs:197-201`） |
| `GrokDay` | 1 | `grokday` / `grok-day`, `light`, `day` | 亮 | false | GrokNight 的亮色对应版 |
| `TokyoNight` | 2 | `tokyonight` / `tokyo-night`, `tokyo` | 暗 | true | 蓝调背景 |
| `RosePineMoon` | 3 | `rosepine-moon` / `rosepine`, `rose-pine` | 暗 | true | Rosé Pine 家族 |
| `Auto` | 4 | `auto` / `system` | 元主题 | false | 磁盘存 `"auto"`，内存只存解析结果 |
| `OscuraMidnight` | 5 | `oscura-midnight` / `oscura` | 暗 | true | 极深底 + 紫强调 |
| `Terminal` | 6 | `terminal` / `terminal-default`, `transparent`, `native` | 中性 | false | 受 rollout gate 控制 |

`ALL` 只含 6 个具体主题、不含 `Auto`（`from_name` 额外把 `Auto` 串进查找链，`theme/mod.rs:44-51, 148-154`）；别名见 `:131-141`，`display_name` 见 `:97-107`，`oscura-midnight` 无专门分支、落到 `other => other` 原样返回（`:185-195`）。**可用性三级过滤**：`ALL` → `selectable()`（按 `terminal` gate 剔除）→ `available()`（再按 truecolor 能力剔除，前置判断是 `detect().has_truecolor()`）（`:72-95`）。设计取向取自文件头注释：GrokNight「`Grayscale ramp anchored at bg #141414 / fg #f3f3f3. Accents are TokyoNight Night hex.`」（`groknight.rs:14`）；GrokDay 同色相族整体加深（`grokday.rs:2-3`）；Oscura 用 OKLCH hue 265（`oscura.rs:9, 15-46`）。

**组织方式**：① 每主题一个 `const fn` 逐字段赋值——`tokyonight()`（`tokyonight.rs:148-232`）、`groknight()`（`groknight.rs:55-137`）、`grokday()`（`:53-135`）、`rosepine_moon()`（`rosepine.rs:32-114`）、`oscura_midnight()`（`oscura.rs:51-135`）、`terminal_default()` / `terminal()`（`terminal_default.rs:28-111, 115-126`）。② 色值以内联 `mod palette` 组织再 `use palette::*;`，`rgb()` 是本地 `const fn rgb(r,g,b) -> Color { Color::Rgb(..) }`（`groknight.rs:10-12`）。③ 规模：`groknight.rs` 138 行 / 26 个 palette 常量 / 36 个唯一 RGB 三元组，`tokyonight.rs` 363 行（它同时承载 `Theme` 结构体定义与全部字段注释），`terminal_default.rs` 414 行 / 0 个 palette 常量（全 `Reset`）。④ 用户能改的只有「选哪个主题」（`[ui].theme`），**没有自定义色值入口**。⑤ 唯一的打包资源是 3 个 `.tmTheme`（语法高亮，不属于主题系统本体），`include_bytes!` 内联（`pager-render/src/syntax.rs:128-132`）。

**语义色字段规模与分组**：58 个字段是扁平命名空间，按用途分组并带注释（`theme/tokyonight.rs:47-144`）：背景 6、`accent_*` 9、文本 2、灰阶 3、语义 4、搜索 1、模式强调 3（plan / verify / remember）、边框 4、滚动条 2、diff 6、`bg_visual` 1、paste 3、markdown 13，另有 6 个 `Modifier`（`md_heading_h1_mod` … `md_heading_h6_mod`）。**没有独立的 token / role 定义文件**——字段名本身就是 role，另一层「样式 token」由 `const fn` 方法组合（`tokyonight.rs:234-341`）；颜色合成同样集中（`blend_color` 在任一输入是 `Indexed` 时把结果量化回索引色，`nearest_indexed` 在 6×6×6 色立方与 24 级灰阶间取最近邻，`render/color.rs:160-174, 56-91`）：

| 方法 | 语义 |
| --- | --- |
| `primary()` / `fg(color)` | 主文本样式 / 单色样式 |
| `muted()` / `muted_over_chrome()` | 次级文本；后者把 `Reset` 钉住以免继承边框色 |
| `dim()` / `faint()` | 最弱装饰文本；bandless 调色板走 `Modifier::DIM` 而不是画亮黑；`faint()` 再弱一档（`gray_dim` 向 `bg_base` 混 0.66，不能混则退回 `dim()`） |
| `is_bandless()` | `bg_visual == Color::Reset`，是「用反显 / 装饰替代色块」的总开关 |
| `selection_overlay()` / `hover_overlay()` / `block_cursor_over()` | 选中、悬停、块光标；bandless 下统一 `REVERSED` |
| `row_hover_bg()` / `panel_border_fg()` / `diff_uses_line_fg()` / `ghost_text_style()` | 行悬停（`bg_base→bg_dark` 半混）；分隔线取 `bg_highlight`（bandless 取 `gray_dim`）；diff 带为 `Reset` 时改用整行前景色；幽灵文本 = `dim()` + `ITALIC`（`theme/mod.rs:340-345, 406-411`） |

**四档降级链**（`theme/color_support.rs:17-30`）：① **`NO_COLOR` 优先**——环境里有该变量即返回 `None`（`:99-101`）；② `supports_color::on(Stream::Stdout)`——`has_16m`→TrueColor、`has_256`→Ansi256、`has_basic`→Basic，否则 `None`（`:103`）；③ **非 TTY 回退 TrueColor**——`supports-color` 返回 `None` 时（tests / piped）直接判 TrueColor，理由是「TUI 总归跑在终端里」（`:115-117`）；④ **品牌白名单升级**——只判到 256 或更低，但品牌属于 iTerm2 / Ghostty / Kitty / WezTerm / Alacritty / Rio / Warp / VS Code / Windows Terminal / Foot 或宿主是 Windows，则提升为 TrueColor（`:121-123, 246-269`），动机是 tmux / SSH / mosh 常剥掉 `COLORTERM`、而 ConHost 自 Win10 1709 起支持 24 位色却不上报。结果缓存在 `OnceLock`，`detect()` 再叠一层「minimal 模式锁 → 上限 Basic」的封顶（`:54, 85-88`）。`TERM` / `COLORTERM` 的显式解析**只存在于 `grok doctor` 的独立路径**（`standalone_from_env`，`:172-203`），那条路径刻意不读 stdout（`--json` 常被管道化），改用 stderr 是否为终端或能否打开 `/dev/tty` / `CONIN$`（`:129-170`）。`ColorLevel::None` 的实现方式就是「所有色值塌缩为 `Color::Reset`」，再配合 `muted()` / `dim()` 的 `Modifier::DIM` 达到无彩效果（`:232`、`tokyonight.rs:239-244`）：

| 目标档 | `Color::Rgb` 输入 | `Color::Indexed` 输入 | `Reset` / 命名色 |
| --- | --- | --- | --- |
| TrueColor | 原样 | 原样 | 原样 |
| Ansi256 | `Indexed(nearest_indexed(r,g,b))` | 原样 | 原样 |
| Basic | `indexed_to_ansi16(nearest_indexed(..))` | `indexed_to_ansi16(n)` | 原样 |
| None | `Reset` | `Reset` | `Reset` |

（量化规则见 `theme/color_support.rs:220-234`。）**`Theme::current()` 每次调用都重新组装**（`theme/mod.rs:293-333`）：① minimal 模式的 `terminal_native_locked()` → 直接用 `terminal_default().quantized(level)`（`:295-297`）；② `terminal` 主题走 `Self::terminal().quantized(level)` 并**早返回**，理由注释是「极性适配和 ANSI16 override 会把不透明背景盖到 `Reset` 上」（`:299-302`）；③ 按 `current_kind()` 构造基础主题 → **先采样极性** `base.is_dark()`（此刻 `bg_base` 还是 RGB）→ Windows 上做 `windows_contrast_boost` → `quantized(level)`（`:314-322`）；④ Basic 档或「legacy Windows 控制台且无 truecolor」时再做 `ansi16_chrome_overrides(dark)`（`:325-332`），此分支额外要求 `level.has_color()`，注释明确理由：**否则命名 ANSI 重绘会把 `Reset` 覆盖，从而破坏 `NO_COLOR`**。两个补偿层：`windows_contrast_boost`（`:367-404`）把若干背景 / 边框色**推离** `bg_base`（Windows 显示 gamma 会把 ~12 级 RGB 台阶压扁），步长 8–60 不等（`prompt_border_active` 60、`gray_dim` 40、深色下的用户块 28）；`ansi16_chrome_overrides`（`:429-556`）在只有两个灰阶的 ANSI16 下把 chrome 钉到 `Black/White/Gray/DarkGray` 四个中性槽，语义色按「暗底用亮变体、亮底用常规变体」映射，并把紫 / 青 / 黄族折叠（magenta 吸收 purple/violet、yellow 吸收 orange/gold/plan）。运行时生成的色也要量化：语法高亮 `syntect_rgb_to_fg` 非终端原生时走 `theme::quantize`（`pager-render/src/syntax.rs:46-51`），`Theme::faint()` 的混合结果同样是量化后的字段参与运算；Markdown 的 `md_style::style()` 每次从 `Theme::current()` 重建并把 ratatui 色桥接到 `anstyle`，`Reset` 映射为 `None`（`theme/md_style.rs:8-31, 94-101`，含一条回归测试说明它曾错误回退到 `AnsiColor::White`，在浅色终端上渲染出灰糊糊文本并破坏 `NO_COLOR`）。

**OSC 11 的边界（易误读）**：`osc11.rs` 探测的是**背景明暗**，与 truecolor / 256 / 16 无关。查询串 `\x1b]11;?\x07`（`:30`），主超时 500 ms、tmux passthrough 回退 80 ms（`:25-28`），解析同时兼容 4 位与 2 位十六进制通道（`:76-100`），亮度用 ITU-R BT.709 系数 + sRGB 反 gamma、阈值 0.5（`:23, 65-74`）。它是**仅启动期**探测：crossterm 的 `EventStream` 一旦接管 stdin 就不能再查询（`:13-14`）。系统外观因此是四级链、前一级有结果就不走下一级：桌面 API `dark_light::detect()` → `GROK_APPEARANCE` / `LC_GROK_APPEARANCE`（`LC_*` 别名是为了穿过 SSH 的 `AcceptEnv LC_*`）→ 启动期 OSC 11（结果写 `OnceLock`，运行时复用不重探）→ `COLORFGBG` 兜底（取最后一个字段当背景索引，`0..=6 | 8` → Dark、`7 | 9..=15` → Light、非数字如 `default` → `None`）（`theme/system_appearance.rs:3-11, 53-79`、`theme/env_appearance.rs:3-4, 20-31, 45-67`）。运行时由 `SystemAppearanceWatcher` 每 5 秒调一次 `detect()`（测试下 50 ms），只有值变化才通过 `watch` 通道发出，且它不触发 OSC 11、不直接改主题缓存（`system_appearance.rs:121-171`）；`auto` → 具体主题由 `to_theme_kind` 决定（Light → GrokDay、Dark → GrokNight，可被 `[ui].auto_dark_theme` / `auto_light_theme` 覆盖，读取时过滤掉 `auto` 自身以防循环）（`:108-119`、`cache.rs:267-289`）。

**热切换的唯一写入入口**：`/theme <name>` → `Action::SetTheme(canonical)` → router → `set_theme(app, v)` → `Theme::apply_kind(kind)` → `cache::set` → `store_kind` → `sync_markdown_polarity`，同时发 `Effect::PersistSetting` 落盘并 `presenter.request(false)` 重绘（`slash/commands/theme.rs:115, 123`、`app/dispatch/router.rs:1160`、`theme/mod.rs:348-355`、`theme/cache.rs:89-100, 138-146`、`xai-grok-shell/src/util/config/settings_writes.rs:220-222`）。

- `apply_kind` 在 minimal 模式锁下直接 no-op；否则先把「要求 truecolor 但不支持」的主题**钳到 `GrokNight`**（`theme/mod.rs:357-363`）。钳制只影响**实时显示**，用户仍可把 `tokyonight` 写进配置（`slash/commands/theme.rs:119` 注释）。
- `/theme` 的方向键预览走 `preview_arg` / `cancel_preview`，只调 `apply_kind`，**不落盘、不弹 toast**（`slash/commands/theme.rs:8, 49-65`）；系统外观变化路径同样是 `apply_kind`，仅在 `effective != current` 时重绘，watcher 启停由 `AUTO_MODE` 驱动（`app/event_loop.rs:1847, 2547-2576, 3066-3075`）。
- 光标色：`cursor_color_escape()` 把 `accent_user` 解析回 RGB 生成 OSC 12，`Reset` 返回 `None`（表示不动终端自己的光标色）（`theme/mod.rs:563-568`）；启动期直接写 stderr（`:570-585`，调用点 `app/mod.rs:414, 1536`），进事件循环后改为**差分发射**——比较 wanted / on-wire，变化才发 OSC 12，转为 None 时发 OSC 112（`app/event_loop.rs:1971-1982`），退出时 `reset_cursor_color_if_applied()`（`app/terminal_restore.rs:76`）。`CURSOR_COLOR_APPLIED` 门闩的存在理由有注释：**无端复位会让 Ghostty 锁死光标色并不再跟随主题**（`theme/mod.rs:559-561, 587-594`）。

**`terminal` 主题与 minimal 模式**是两条「终端原生」路径，共用同一套调色板 `terminal_default()` 但触发方式不同：minimal 由 `TERMINAL_NATIVE_LOCK` 整会话锁死、并把色彩能力封顶到 `Basic`（`cache.rs:27-28, 117-120`、`app/mod.rs:394`、`color_support.rs:85-88`）；`terminal` 主题由用户选中、可随时切走，名字解析受 rollout gate `TERMINAL_THEME_ENABLED` 控制（关闭时目录里不出现）（`cache.rs:30-32, 124-135`、`theme/mod.rs:53-67, 146-159`）。调色板设计（`terminal_default.rs:1-20` 模块注释给出完整理由）：**所有背景为 `Reset`**（有测试断言 `bg_*`、`md_code_bg`、`diff_*_bg`、`paste_bg` 全为 `Reset`，`:207-222`）；**正文与次级文本一律 `Reset` + `DIM`** 而不是画亮黑 ANSI 8，因为许多暗色 profile 把亮黑调得很暗、画上去会把 tool 输出和信息行洗掉（`:10-21`）；`terminal()` 变体把装饰槽改为亮黑（`selection_border` / `hover_border` / `prompt_border` / `scrollbar_fg` / `gray_dim` → `Color::DarkGray`）而 `prompt_border_active` 保持 `Reset` 让聚焦仍醒目（`:113-126`）；**语法高亮不跟着明暗走**——`polarity_safe_syntax_fg` 把低彩度 token 映为默认前景、彩色 token 映到 ANSI 16 基色，但源码 palette 仍加载 night 主题文件（理由注释「极性无关，只要 palette 完整」，`pager-render/src/syntax.rs:54-86, 119-132`）。

**边角细节**：`bg_terminal` 字段注释写着 `currently unused, using bg_dark instead`，但字段仍被量化与测试覆盖（`tokyonight.rs:53`）；主题 rollout gate 独立于 minimal 锁，`selected_kind()` 的存在就是为了看穿 minimal 锁的伪装，防止被 gate 关闭的主题在锁解除后复活（`cache.rs:72-86`）；测试基建上 `color_support::set_level_for_test` 在 `OnceLock` 写入前做覆盖（理由是环境里的 `NO_COLOR` 会因调度顺序而赢，`:56-66`），`cache::pin_theme()` 同时钉住主题与色深（理由同样是 agent shell 会导出 `NO_COLOR`，`:324-334`）。

### 3.2 字形、图标与边框符号

#### 本节要点
- **不存在「非 Unicode 终端 → ASCII」的通用降级**；唯一开关是 legacy Windows ConHost 一个布尔量，非 Windows 平台在 `decide_legacy_windows_console` 第一句就返回 `false`（`glyphs.rs:406-424`）。
- 判定是 **default-deny**：Windows + 品牌不在 11 项白名单内即视为 legacy，因为裸 `cmd.exe` / `powershell.exe` 在 ConHost 里不设置任何终端环境变量（`glyphs.rs:404-423`）。
- fallback 是「**ASCII 优先、必要时用 CP437**」而非纯 ASCII：`✓`→`√`、`◆`→`♦`、`◇`→`○`、`┃`→`│`、`━`→`─`、`▏`→`│`（`glyphs.rs:59-65, 111-126, 175-181, 206-218, 248-254`）。
- 所有 `(fancy, fallback)` 对都被断言**同宽**（5 张帧表 + 6 组字符表覆盖 20 余对）；帧数可以不等，但每帧恒为 1 列（`glyphs.rs:426-558`，尤其 `spinner_frames_are_one_column` 在 `:511-525`）。
- legacy 回退会让部分符号退化成同一字形，此时语义**完全依赖颜色**——`◈` 与 `◆` 都变 `♦`，`━` 与 `─` 都变 `─`（`glyphs.rs:128, 220-221`）；宽字符则按 **grapheme 簇**整簇裁剪，放不下就丢弃并补空格（`render/line_utils.rs:102-166`）。

整套符号分三层，**只有第一层集中**：层 1 是 `pager-render/src/glyphs.rs` 的 34 个 `pub fn`（单一事实来源，被 `xai-grok-pager` 与 `xai-grok-pager-minimal` 共同引用）；层 2 是分散在各视图的结构字形（框线 `views/prompt_widget/mod.rs:3049-3358`、进度条 `views/progress_bar.rs:16-29`、表格 `style.rs:28-38`、滚动条 `render/scrollbar.rs:268`）；层 3 是 Markdown 的解析期字符改写（见下）。模块注释说清全部动机（`glyphs.rs:1-7`）：

> Legacy-console fallbacks for chrome glyphs that don't ship in the legacy Windows ConHost default font (Consolas / Lucida Console). … Fallbacks are ASCII where possible (`x`, `o`, `c`, `*`), or a CP437 glyph when one reads better and still renders on the raster font. … **ConHost does no font fallback, so missing glyphs render as tofu.**

**符号词汇表（节选，可直接照抄）**。调用方会拿宽度做布局运算，所以每个字形与它的 fallback 必须同宽；`—` 表示上游专题未逐条列出该字形的 fallback。

| 字形 | 码点 | 用途 | legacy fallback | 出处 |
| --- | --- | --- | --- | --- |
| `❯ ` | U+276F + 空格 | 用户消息前缀箭头，恒 2 列（`PROMPT_ARROW_WIDTH = 2`） | `> ` | `glyphs.rs:16-25` |
| `✗` / `✓` | U+2717 / U+2713 | 失败 · 关闭 · 取消 · kill / 完成 | `x` / `√` U+221A | `glyphs.rs:47-65` |
| `❙` / `↗` / `⧉` / `⇣` | U+2759 / U+2197 / U+29C9 / U+21E3 | 折叠块强调条 · 放大 · 复制 · token 计数 | —（ASCII 形态见模块注释） | `glyphs.rs:39-45, 68-96` |
| `◆` | U+25C6 | 实心菱形：容器 / 工具 / 运行中 | `♦` U+2666 | `glyphs.rs:111-117` |
| `◇` / `○` | U+25C7 / U+25CB | 空心菱形；空闲、未完成标记 | `○` U+25CB（CP437 自带） | `glyphs.rs:120-126` |
| `◈` | U+25C8 | 点状菱形，tools 用量 | `♦`（与 `◆` 同形，靠颜色消歧） | `glyphs.rs:129-135` |
| `●` | U+25CF | 实心圆点（选中标记、单选） | `•` | `glyphs.rs:237-243` |
| `┃` / `▏` | U+2503 / U+258F | accent rail；选中行左侧细条 | `│` U+2502 | `glyphs.rs:175-181, 248-254` |
| `━` / `─` | U+2501 / U+2500 | 重横线 / 浅横线（后者是唯一无 fallback 的结构字形） | `─`（CP437 `0xC4`）/ 无 | `glyphs.rs:206-218` |
| `━━` / `──` | U+2501×2 / U+2500×2 | timeline 活跃刻度（预组合 2 列）/ hover 刻度 | `══` U+2550×2 / 无 | `glyphs.rs:222-234` |
| `▴` / `▾` | U+25B4 / U+25BE | 小三角；timeline 上 / 下 chevron | `▲` U+25B2 / `▼` U+25BC | `glyphs.rs:184-190, 196-…` |
| `›` / `‹` / `⌄` / `▸` | U+203A / U+2039 / U+2304 / U+25B8 | chevron 右 / 左（面包屑、步进器）· 轻量下 chevron · disclosure | — | `glyphs.rs:256-312` |
| `[✗]` / `[↗]` | — | 预组合按钮，恒 3 列 | `[x]` / `[o]` | `glyphs.rs:317-334` |

预组合的理由（`glyphs.rs:316`）：「预组合成 `&'static str`，这样每帧渲染路径复用静态字符串，而不是每帧 `format!` 分配一次。」另有逐字符映射供 toast 等自由文本使用：`✓`→`√`、`✗`→`x`、`⚠`→`!`（`to_legacy_glyphs`，`:364-373`），外层 `legacy_glyph_fallback` 先做「是否含这三个字符」的快速预检、不含则 `Cow::Borrowed` 零拷贝返回（`:337-345`）。

**动画帧表**（有状态的是帧序列，不是单个字形）：

| 名称 | fancy 帧 | 帧数 | fallback 帧 | 出处 |
| --- | --- | --- | --- | --- |
| braille 转轮 | U+280B `⠋⠙⠹⠸⠼⠴⠦⠧` | 8 | `\|` `/` `-` `\` | `glyphs.rs:148-159` |
| 点转轮 | U+22C5 `:` U+2E2C U+2059（×2） | 8 | `.` `:` U+00B7 | `glyphs.rs:162-172` |
| monitor 脉冲 | U+25CB U+25CE U+25C9 U+25CE | 4 | U+00B7 `○` U+2022 `○` | `glyphs.rs:100-108` |

monitor 脉冲的注释是整套 fallback 设计的核心约束（`glyphs.rs:98-99`）：**「只有 ○ 在 CP437 里，所以 legacy ConHost 靠填充而非靠大小做脉冲。每一帧都是 1 列，后面的标签才不会位移。」** 节拍由各视图自己的除数控制：`SPINNER_DIVISOR = 4`（`views/turn_status.rs:31`，~30 fps 下约 7.5 帧/秒）、`MONITOR_PULSE_DIVISOR = 8`（`:36`，约 3.75 帧/秒，「idle 提示应该缓慢呼吸，而不是读起来像运行中转轮」）、`NEEDS_INPUT_BLINK_DIVISOR = 10`（`views/dashboard/animation.rs:7`）。

**判定与逃生舱**：`is_legacy_windows_console()` 用 `OnceLock` 缓存一次探测结果；平台短路 + default-deny，白名单 11 个品牌为 `WindowsTerminal` / `VsCode` / `Cursor` / `Windsurf` / `Zed` / `WezTerm` / `Kitty` / `Alacritty` / `Ghostty` / `Rio` / `GrokDesktop`（`glyphs.rs:377-424`）。QA 逃生舱 `GROK_FORCE_LEGACY_CONSOLE`：`"1"` / `"true"` 强制开、`"0"` / `"false"` 强制关、其余值走正常探测，「让 QA 不必用 ConHost 也能检查 fallback」（`:388-401`）。易踩的坑（`:381-382`）：必须用 `terminal_context().env_brand` 而**不是** `brand`，因为后者在原生 Windows 上会乐观地变成 `WindowsTerminal`，导致 legacy 终端拿不到 fallback。

**影响面：`is_legacy_windows_console()` 的全部 6 个调用点**（`rg` 全仓确认）：

| 位置 | 行为 |
| --- | --- |
| `views/progress_bar.rs:24` | 进度条整表从 `BLOCKS` 换 `SHADES` |
| `views/welcome/logo.rs:77` | Braille logo **整体隐藏**（无 ASCII 替身） |
| `scrollback/blocks/thinking.rs:54` | 去掉 `ITALIC` 修饰符 |
| `scrollback/types.rs:140` | 折叠块不再变暗——「16 色量化后选中 / 未选的颜色落差读起来像调色板噪声」 |
| `app/agent_view/render.rs:734` | 提示前缀 `●` 换成 `•` |
| `theme/mod.rs:327` | legacy + 非 truecolor 时启用 `ansi16_chrome_overrides` 重映射主题色 |

**两处 fallback 漏网（实测发现）**：`scrollback/blocks/workflow.rs:66-68` 直接硬编码 `"✓"` / `"●"` / `"○"` 字面量，没走 `glyphs::check_mark()` / `glyphs::filled_dot()`；`views/goal_detail.rs:679-681` 同样用字面量 `"\u{25a1}"` / `"\u{25b6}"`，而同一四分支 `match` 的另外两分支却走了 `crate::glyphs::check_mark()` / `ballot_x()`（对比 `views/todo_pane.rs:86-91` 的同名枚举全部走 `glyphs::`，是正确写法）。

**宽字符与不安全字符的对齐处理**：

- 宽度运算全部走 `unicode_width`：`truncate_to_width`（截到 N 显示列、切断时补 `…` U+2026，实现上先按 `max_width - 1` 取前缀以给省略号留一列）、`byte_offset_at_width`、`pad_to_width`（注释：`format!` 的 `{:<width$}` 按 char 数补齐，宽字形如 CJK 之后列会错位）（`util.rs:204-215, 219-229, 233-238`）。
- `fit_line_to_width` 在 grapheme 边界裁剪（不加省略号）或补空格，让每个列都是真实单元格，否则「更宽的字形会遗留一个陈旧单元格（表格旁边的幽灵字形）」（`render/line_utils.rs:104-166`）。测试固化：`Line::from("ab\u{26A0}\u{FE0F}")` 裁到 3 列得 `"ab "`（放不下的宽 grapheme 整簇丢弃、空出的列补空格）；用 `graphemes(true)`，因此 `⚠` + U+FE0F 变体选择符算一个 grapheme（`:326-343`）。
- status line 另实现「实际绘制宽度」模型 `painted_clusters()`：丢弃含控制字符的簇、丢弃零宽簇；对应测试逐条列出两套宽度模型的每一个分歧点（ASCII 作对照组、变体选择符、ZWJ 家庭 emoji）（`views/status_line/mod.rs:25-35`、`views/status_line/tests.rs:129-139`）。
- `is_unsafe_display_char` 白名单式清洗：`is_control()` 或 U+061C、U+200B–U+200F、U+202A–U+202E、U+2060–U+206F、U+FEFF；理由注释是「C0/C1 控制字符可以注入终端转义序列；bidi 控制符与零宽格式字符会开启 Trojan-Source 欺骗」（`render/line_utils.rs:31-44`）。**ZWJ（U+200D）不在这个集合里**，因为它要参与 emoji 簇的宽度计算。
- 越界保护 `set_line_safe` / `set_span_safe` / `set_string_safe`：ratatui 的 `Buffer::set_line` 等在越界坐标会 panic（经由 `index_of`），而 resize 竞态下算出的 widget 区域可能瞬时越界，这些辅助函数跳过写入而不是 panic（`render/safe_buf.rs:1-3`）。

**第二张集中的表与「未接线」的预设**：`TableBorders` 用 11 个 `char` 槽位（`H/V/TL/TR/BL/BR/T_T/T_B/T_L/T_R/X`）表达表格边框，配 `BOX`（`─│┌┐└┘┬┴├┤┼`）、`ASCII`（`-|+`）、`DOUBLE`（`═║╔…`）三套预设（`style.rs:9-38`）。**限制（已核实）**：`ASCII` 与 `DOUBLE` 在本快照中从未被选中——`parse.rs:1589` 的 `format_table` 硬编码 `TableBorders::BOX`，`render/wrapping.rs:309` 只在注释里提到 ASCII 边框，`MarkdownStyle` 没有 `table_borders` 字段，`Default for TableBorders` 返回 `BOX`（`style.rs:115-119`）。这是「API 预留了 3 套、实际只接线了 1 套」，**不是**由终端能力驱动的降级路径，与 `glyphs.rs` 的 ConHost 判定互不相干。

**Markdown 的字符改写在解析期，不是渲染期**：`>` → `│` U+2502、`- ` / `* ` → `• ` U+2022、`---` → `───`（U+2500×3）都是解析阶段写进 `Transform` 缓冲区的（`parse.rs:836, 903, 924, 1040-1044`），引用条改写的注释是「Transform the `>` belonging to THIS blockquote level to `│`」；有序列表（`1. ` / `1) `）不改写，只加 `list_item` 高亮（`parse.rs:1022-1036`）。下游因此要做两件配套工作：按「条必须是 `bar_style` 着色」精确识别前缀并标成不可拖选（引用条会泄漏进拖选复制内容，`scrollback/blocks/quote_bar.rs:6-10, 54-92`）；以及用**码点区间**替代语法分析区分表格与引用块——行首命中 U+2500–U+257F 且非竖线即判为表格，`│` 需扫描过「只含 `│` 与空格的前缀区」后再遇 `│` 才算表格内容行，ASCII 兜底只看 `|`（`render/wrapping.rs:288-311`）。

**滚动条与进度条是两个独立的块字符用户**：滚动条把 ratatui-core 的 `ScrollBar` 渲到临时 buffer 再逐格改写，`" "` → 轨道、其余 → U+2588 `█`，**不做 1/8 分辨率**，thumb 永远是整格（`render/scrollbar.rs:246-270`）；有个 macOS Terminal.app 兼容处理：thumb 单元格的 `bg` 必须等于 `fg`，否则「行间空隙像素没被涂到，thumb 会呈现深色横条纹」（`:410-411`）。进度条用 1/8 格分辨率并带独立 fallback：`BLOCKS = ["", ▏▎▍▌▋▊▉█]` → `SHADES = ["", ░░░▒▒▓▓█]`，原因是 Consolas 缺 U+258F..=U+2589（引 microsoft/terminal#387），模块注释承认代价「同样是 1/8 分辨率输入，只是单元格读起来像密度图案，而不是真正的左对齐进度条」（`views/progress_bar.rs:1-38`）。另有 `views/usage_modal.rs:817-822` 直接手写 `"█".repeat(filled)` + `"░".repeat(..)` 的 30 列用量条，**未走**该模块。

**品牌 logo 是唯一「降级 = 不显示」的字形**：`assets/logo/{logo07,logo05}.txt` 两份 Braille 点阵资产用 `include_str!` 内嵌为 `LOGO`（7 行）与 `LOGO_SMALL`（5 行）（`views/welcome/logo.rs:12-13`）。三档高度由 `SMALL_LOGO_MIN_HEIGHT = 22` / `FULL_LOGO_MIN_HEIGHT = 26` 决定，输出 `LOGO` → `LOGO_SMALL` → `None`，并有 `step_down()` 让堆叠布局逐档退让（`:16-64`）；另一入口 `render_full_logo` **永远用大图且不看高度**，因为「hero box 是排在菜单旁边的，box 放得下它就放得下」（`:210-233`）。legacy ConHost 上整体隐藏，因为「ConHost 的栅格字体不覆盖 U+2800 Braille 区块，会渲染成豆腐块」，且**没有** ASCII 替身（`:1, 76-78`）。logo 带对角线 shimmer：`BAND 0.38` / `CYCLE 4.0`（一次扫过 + 静止的总秒数）/ `SWEEP_FRAC 0.32`（~1.3 s 闪光）/ `SHINE 0.33` / `PULSE 0.06` / `PULSE_SECS 5.0`；相位基于墙钟（与帧率解耦），节流到 `SHIMMER_FPS = 12.0`，对角坐标 `diag = (col + (rows - 1 - row)) / (cols + rows)`（左下 → 右上），颜色用 `blend_color(theme.gray, theme.text_primary, opacity)`，相邻同色合并成 Span 以降低每帧分配（`:97-187`）。**水印：未找到证据**——除欢迎屏 logo 外未发现任何全局水印 / 背景图案绘制代码。

**「符号 × 颜色」没有全局表**，集中度最高的只是 dashboard 的一对相邻函数 `state_icon()` / `state_color()`（`views/dashboard/render.rs:3027-3051`），其注释明确写下「同符不同色」契约：`NeedsInput` / `Completed` / `Failed` **共用 `◆`**，仅靠前景色区分（`accent_user` / `accent_success` / `accent_error`）；同文件顶部还记录跨视图形状契约：「dashboard 用菱形而不是圆形，这样它读起来与兄弟 activity 视图（那些用圆形）不同」（`:24-27`）。「等待用户」的统一脉冲色是唯一被文档化要求「必须统一调用」的颜色约定：`USER_WAITING_PULSE_SPEED = 0.08`，`pending_diamond_color()` 用 `pulse_brightness` + `blend_color(bg_base, accent, 0.3 + brightness * 0.7)`，drain-blocked / pending-user-input / plan-approval 三个 cue 共享同一常量（`views/turn_status.rs:38-47, 1576`）；工具阻塞在权限提示或 `ask_user_question` 上时，把运行中的 braille 转轮换成脉冲的 `◆`（`:366-368`）。上下文构成条是最典型的「符号 + 颜色共同编码」实例（`scrollback/blocks/context_info.rs:248-264`）：`◆ ◆ ◆ ◈ ◈ ◈ ◆ ◆ ◆ ◇ ◇ ◇ ◇ ◇`，按 `gray / teal / violet / dim` 分段对应 system / tools / overhead / free——5 个类别只用 3 种字形、靠 5 色区分；注释说明这是刻意选择：「Messages 拿到最亮的处理（primary），因为它们主导构成分解，也是用户真正在引导的对话」。

**光标不走字形，走终端转义**：`CursorStylePolicy` 三态 `Inherit`（默认，尊重用户终端自己的光标设置）/ `ForceBlinking` / `ForceSteady`，由 `[ui].cursor_blink` 三态配置映射并在 `init_terminal` 中执行（`app/mod.rs:1430-1452, 1509-1524`）。主输入框用**真实终端光标**（`views/prompt_widget/mod.rs:3394` 算 `cursor_pos` 交给终端定位转义），模态窗口里的文本输入用**假光标**（直接画 `selection_bar()` 字形，`views/settings_modal/render.rs:1667, 1686`）。`CursorAction` 会跟踪上次写入的光标位置只发必要的转义，因为「多余的 `Show`/`Hide`/`MoveTo` 会重置终端的闪烁计时器」（`render/draw.rs:436-497`）。

### 3.3 排版、留白与信息密度

#### 本节要点
- 排版参数分三层、**互相没有统一命名空间**：可配置的 `LayoutConfig`（只覆盖滚动区）+ 各类块 `*Config` + 各 view 文件顶部的局部 `const`；**不存在全局设计令牌模块**（`appearance/config.rs:168`）。
- 滚动区水平版心是固定公式 `accent(1) + block_pad_left(2) + 内容 + block_pad_right(2)`，外层视口留白默认上下 1、左右各 2（`scrollback/layout.rs:7-43`、`appearance/config.rs:181-190`）。
- 缩进一律以**终端显示列**度量，绝不按字节或 char；`bullet_indent()` 现算字形宽度 + 1，`pad_to_width` 专门绕开 `format!` 的 char 计数（`scrollback/types.rs:115-122`、`pager-render/src/util.rs:231-239`）。
- **块间距不是常量而是一条成对规则**：两侧都能加入 dense run 时 gap = 0，否则 1；一跨回合或任一块展开就插 1 行间隔（`scrollback/state/layout.rs:1555-1563`、`scrollback/state/groups.rs:172-176`）。
- **默认只有用户提示词块有垂直 padding**：基类 `has_vpad_for` 默认 true，但工具 / 思考 / 系统块全部覆写为 false；高度不足 3 行时放弃 vpad（`scrollback/block.rs:68-70`、`scrollback/scrollback_pane.rs:743-746`）。
- 换行统一走 `textwrap` 的 `FirstFit` + `break_words(true)`，**无 URL 专用断行器**；表格是唯一 `break_words(false)` 并改用 grapheme 硬切的地方（`render/wrapping.rs:176-179`、`parse.rs:1870-1907`）。

第 3 层规模不小：`views/` 下带缩进 / 留白语义的局部常量至少有 `H_MARGIN = 2`、`H_MARGIN_COMPACT = 1`（`views/welcome/mod.rs:110, 112`）、`HERO_BOX_MIN_WIDTH = 90` / `V_PAD = 1` / `H_INSET = 2` / `LOGO_H_PAD = 3`（`views/welcome/hero_box.rs:14, 17, 20, 23`）、`CHROME_PAD = 2`（`views/welcome/prompt.rs:20`）、`RAIL_WIDTH = 2` / `MIN_TERMINAL_WIDTH = 60`（`views/timeline.rs:17, 20`）、`DESC_INDENT = 4`（`views/picker.rs:833`）、`QUESTION_VIEW_HPAD = 5` / `LEFT_PAD = 3`（`views/question_view.rs:1048, 1059`）、`ELICIT_HPAD = 5`（`views/elicitation_view/render.rs:18`）、`STANDARD_MAX_WIDTH = 110`（`views/settings_modal/state.rs:31`）。整屏纵向骨架（`views/agent.rs:218-231`）：

```
┌─ 终端全宽 W ─────────────────────────────────────────────────────────┐
│ StatusBar                                            Length(1)       │
│ Padding::new(eff_hpad_left, eff_hpad_right, top_vpad, bottom_vpad)   │
│  tasks / catalog / todo 面板（各前置 pane_gap 行）                   │
│  滚动区                              ← Constraint::Min(SCROLLBACK_MIN_ROWS = 5)
│  btw / queue / turn_status / banner / cta / follow_ups / dock        │
│  prompt_height / shortcuts                                           │
└──────────────────────────────────────────────────────────────────────┘
```

`pane_gap = if top_vpad == 0 { 0 } else { 1 }`，`status_gap` 同规则（`:229, 244`）。滚动区是整屏唯一的 `Constraint::Min`（注释「The solver ranks it above every `Length`」），其余全是 `Constraint::Length(n)`（`:92-94`）。

**滚动区水平版心**：`HorizontalLayout` 是唯一的水平版心定义点，结构注释写作 `│A│PL│Content│PR│`，`ACCENT = 1`，`chrome_width = ACCENT + block_pad_left + block_pad_right`（`scrollback/layout.rs:7-47`）。默认值：`outer_vpad` 1、`outer_hpad_left/right` 2 / 2、`block_pad_left/right` 2 / 2（后者注释 `// Match left padding for symmetry`）（`appearance/config.rs:181-190`）。80 列下默认配置（自算）：文字距窗口左沿 = `outer_hpad_left(2) + ACCENT(1) + block_pad_left(2) + bullet_indent(2) = 第 5 列`，其中 `bullet_indent = bullet.char() 宽度 + 1 = 2`（默认 `◆`）；内容宽度 = `W − chrome_width − 2×outer_hpad = 80 − 5 − 4 = 71`。

**缩进层级清单**（归纳起来是 **3 个固定级别 + 2 个可变级别**；唯一例外是 `EditToolCallBlock` 用 `const INDENT: &str = "  "` 并取 `INDENT.len()` 当宽度，纯 ASCII 空格下等价，但是一个「字符串长度当宽度用」的隐患点，`tool/edit.rs:103, 163, 191, 578, 615`）：

| 层级 | 宽度 | 来源 |
| --- | --- | --- |
| 视口左右留白 | 2 / 2（compact 时 1 / 1） | `appearance/config.rs:184-188, 203-218` |
| accent 列 | 1 | `scrollback/layout.rs:17` |
| 块左内边距 | 2 | `appearance/config.rs:187` |
| 工具块 bullet 前缀 | `bullet.char()` 宽度 + 1 = 2（默认 `◆`） | `scrollback/types.rs:115-122`、`appearance/config.rs:527` |
| Edit 块 diff 内容缩进 | 2（可选，`indent = true`） | `tool/edit.rs:103`、`appearance/config.rs:400` |
| thinking 体内 rail | 2（`BODY_RAIL_WIDTH`，仅 minimal 模式） | `blocks/thinking.rs:62, 108-115` |
| Markdown 列表嵌套 | **跟随源文档空格数**，渲染不做归一化 | `parse.rs:1012-1042` |
| Markdown 引用嵌套 | 每级 2 列（`│ `） | `parse.rs:922-924`、`markdown_content.rs:411-428` |
| 表格单元格内边距 | 1（每格左右各 1） | `parse.rs:1590` |

**第二套版心：覆盖层是「定宽 + 居中」**，与内容流的「全宽 + 小留白」并存：欢迎页 hero box 的 `box_width = content_area.width.saturating_sub(6).min(120)`、最小宽 90，内部 `V_PAD = 1` / `H_INSET = 2` / `LOGO_H_PAD = 3`，用 `Layout::horizontal([Min(0), Length(box_width), Min(0)]).flex(Flex::Center)` 居中（`views/welcome/hero_box.rs:14, 17, 20, 23, 102, 170-176`）；弹窗 `ModalSizing`（`views/modal_window.rs:89-135`）默认 `width_pct 0.9 / max_width 140 / min_width 60 / v_margin 7 / h_pad 2 / v_pad 2 / footer_lines 2`，`medium()` 为 `0.60 / 120 / 44 / 4 / 2 / 1 / 2`；宽度求解为 `max_width = area.width.saturating_sub(4).min(sizing.max_width)`、`modal_width = (area.width * width_pct).min(max_width).max(min_width).min(area.width)`，注释强调「Clamp to the buffer last: the `min_width` floor can exceed a narrow terminal」；`inner_width = modal_width − 2`（边框各 1 列），内容与页脚再各减 `h_pad`（`:296-299, 363, 372-374, 569-585`）。

**垂直节奏**：

- 基类 `has_vpad_for` 默认 true（注释「Vertical padding (blank line with accent top/bottom)」，`scrollback/block.rs:68-70`），但**几乎每个块都覆写为 false**——用户提示词 `prompt.vpad && !compact`（默认真 → 有 vpad）、Edit `edit.vpad`（默认 false）、agent 消息 / thinking / execute / read / search / web_* / use_tool / list_dir / memory_search / system / session_event / context_info / subagent / bg_task / btw / workflow 全部 `false`。因此**默认形态下只有用户提示词上下各空 1 行**，工具与思考块是紧凑的。
- vpad 的记账：`Has_vpad → vpad = 2`（上 1 下 1）（`entry.rs:416-419`、`scrollback_pane.rs:589`），渲染时 `vpad_top = if has_vpad { 1 } else { 0 }`（`scrollback/render.rs:438-441`）；空间不足时整体放弃——「Need at least 3 rows: vpad_top (1) + content (1) + vpad_bottom (1). If less space, skip vpad to prioritize content.」（`scrollback_pane.rs:743-746`）。
- **块间距由相邻两块属性决定**（`scrollback/state/layout.rs:1555-1563`）：`gap_after_between(prev, next)` 在 `can_join_dense_run` 对两侧都成立时返回 0、否则返回 1；准入条件是 `entry.block.is_groupable() && !entry.block.is_turn_terminal_marker() && (!collapsed_only || entry.display_mode == DisplayMode::Collapsed)`（`scrollback/state/groups.rs:172-176`）。语义即：**连续默认折叠的同族工具块堆成无间隔密集带；一旦跨回合或任一块展开，就插 1 行间隔**。末条目固定 `gap_after = 1`（`state/layout.rs:1257-1260, 1305`）。
- **Markdown 代码围栏前必空行**：pretty 模式下 ``` 被隐藏，渲染器在隐藏围栏时补一行空行，**只补在开栅栏前、闭合栅栏前不补**（`render.rs:657-676`），理由是「Otherwise adjacent blocks (a list then a code block) collapse together once pretty mode hides the fence markers」；回归测试 `test_list_followed_by_code_block_has_separator` 断言列表项与代码块之间 `world_idx - hello_idx >= 2`（`:1271-1288`）。段落之间**不做额外增行**：软换行在非续行位置被折叠成等长空格（`parse.rs:736-751`），块间空行来自源文档本身。
- 首末行：顶部 `outer_vpad` 1 行，`SHORT_TERMINAL_ROWS = 16` 及以下时底部 padding 归 0（`views/agent.rs:205-209`）；粘性头 `HEADER_CONTENT_GAP = 1`（注释「Gap row between header and content for visual separation」）且 `pinned_height` 把它算在内（`scrollback/sticky.rs:92`、`scrollback_pane.rs:1116`）；输入框内部为 `vpad_top | text(Min 1) | info_block`，`info_block = if has_info { 1 } else { 0 }`（`views/prompt_widget/mod.rs:275-277, 3024-3032`）。

**换行策略**：配置集中在 `RtOptions`（`render/wrapping.rs:148-181`），默认 `break_words: true`、`WrapAlgorithm::FirstFit`、`WordSplitter::HyphenSplitter`、`WordSeparator::new()`；宽度单位是**显示列**（`textwrap` 通过 `UnicodeWidthStr` / `UnicodeWidthChar` 计宽）。首行与续行的可用宽度分别扣除各自缩进且下限为 1：`initial_width_available = opts.width.saturating_sub(rt_opts.initial_indent.width()).max(1)`，续行同理（`:367-371, 406-409`）。`FirstFit` 是贪心算法，注释特别提醒「FirstFit is greedy, so single-pass equals the two-stage wrap; a non-greedy change must mirror it」（`:84-85`）——为了让搜索高亮的断点与视觉渲染严格一致。

- **CJK 无专门分支**，靠 `unicode_width` 判为宽 2 列自然参与换行：`truncate_to_width("日本語ラベル", 5) == "日本…"`（`util.rs:269-276` 测试）。
- **emoji 按 grapheme cluster 整体处理、绝不切半**：一簇宽 2 放不进剩余 1 列则整簇丢弃并用 1 个空格补位；`line_height_counts_double_width_emoji` 断言 `"😀😀😀"` 在宽度 4 时折成 2 行、宽度 2 时折成 3 行（`xai-ratatui-textarea/src/wrapping.rs:587-593, 596-620`）。
- **长 token / 长 URL 靠 `break_words(true)` 硬断**，没有 URL 专用断行器；表格单元格例外——`break_words(false)` 绝不切开单词，若某不可断词仍超宽则改用 grapheme 粒度硬切，因为「textwrap's char-based emergency split can tear grapheme clusters」（`parse.rs:1870-1907`）。
- **表格永不换行**：`is_table_line()` 的文档注释写明「Never wrap tables: wrapping destroys column alignment. A leading `│` is a blockquote unless an interior `│` separates cells.」；命中后走 `fit_line_to_width` 补齐到内容宽度而不是折行，理由是「terminal clipping desyncs on wider-than-measured glyphs and strands a ghost cell」（`render/wrapping.rs:284-313, 340-344`）。
- **两套语义截然不同的「变窄」操作**：`truncate_line` 在预算耗尽处截断并追加 `…`（预留 1 列）且保留 span 样式；`fit_line_to_width` 按 grapheme 裁剪**不**加省略号或补空格；`cascade_truncate` 是固定优先级降级序列（文档注释「Drop description first, then meta, then truncate activity, then type」，`render/line_utils.rs:63-222`）。省略号统一是 `…`（U+2026，占 1 列）。
- **软换行 / 硬换行由 joiner 数组区分**：`word_wrap_line_with_joiners` 返回 `(Vec<Line>, Vec<Option<String>>)`，`None` = 硬换行，`Some(s)` = 软换行且 `s` 是断点处被跳过的原文空白（`render/wrapping.rs:330-334`）。该信息被三处复用：复制取回原文、链接扫描重拼逻辑行、选择区映射跳过 `indent_width`（`blocks/block.rs:1284`、`render/osc8.rs:390-398`、`scrollback/render.rs:930-936`）。
- 引用块与标题的缩进：`blockquote_prefix_len` 数出前导 `│ ` 序列，若调用方未设 `subsequent_indent` 就自动设成该前缀，使续行重复引用标记（`render/wrapping.rs:315-328, 348-355`）；`wrap_header_hanging`（首 span 留在第 1 行、其余按 `extra_indent + prefix_width` 悬挂缩进，用于 bash 工具头）与 `wrap_header_flush`（续行缩到固定列，用于 web_fetch / search_tool / use_tool / web_search / memory_search 等）（`:634-689`）。另有两份几乎相同的 wrapping 模块（`pager-render` 的 `pub` 版与 `xai-ratatui-textarea` 的 `pub(crate)` 版），差异在 textarea 版会跳过指针不在输入缓冲区内的 `Cow::Borrowed` 片段并把行尾空格并入 range（`xai-ratatui-textarea/src/wrapping.rs:19-34`）。

**信息密度取舍**：显示模式枚举 `Collapsed | Truncated | Expanded`，基类默认 `Expanded`（`scrollback/types.rs:63-70`、`block.rs:102-108`），但**几乎所有工具块与思考块默认收窄**：

| 块 | 默认模式 |
| --- | --- |
| 用户提示词 | 可折叠 → `Collapsed`，否则 `Expanded`（`blocks/user.rs:508-513`） |
| thinking | `Truncated`（`blocks/thinking.rs:554-555`）；截断 = 一行 `…` + 末尾 `truncated_lines = 3` 行，内容行数 `<= n` 时全部显示、不加省略号（`:340-396`、`appearance/config.rs:495`） |
| 用户 `!` bash | `Truncated`，完成时自动 `Expanded`；Agent 工具 bash 为 `Collapsed`（`tool/execute.rs:719-733`） |
| Edit | `Collapsed`，再由 `edit_default_display_mode` 升级（`tool/edit.rs:1419-1422`、`state/mod.rs:1684-1691`） |
| read / search / web_* / use_tool / list_dir / memory_search / sent_message / subagent / bg_task / btw / workflow | 全部 `Collapsed` |
| 部分 session_event | `Collapsed`（:326）/ `Expanded`（:787） |

Edit 的升级规则是 `is_success() && (summary_untrusted || expanded_by_default)` 才 `Expanded`；`expanded_by_default` 由显式 `pager.toml` 值优先、未设置时回落到 shell 侧 `collapsed_edit_blocks` 标志的取反（`appearance/config.rs:415-420`）——**失败**的 Edit 折叠、`summary_untrusted` 的 Edit 展开。bash 截断用首 / 尾两段：`first_lines = 2`、`last_lines = 3`（`appearance/config.rs:611-613`）。超过 `group_max_visible = 10` 的折叠密集带折成一行组头，用 dimmed `◈` 加粗灰字，文案为 `"{n} tool calls & thoughts"` / `"{n} more"` 两支；组头仍走同一套版心，且 accent 列被显式填充背景而不是保留上一帧字形（注释「an unowned cell survives the frame diff」）（`appearance/config.rs:161`、`wrappers/entry_renderer.rs:207-227, 279-292, 520-527`）。

**窄屏 / 矮屏降级（两级阈值，全在 `views/agent.rs:89-98`）**：`SHORT_TERMINAL_ROWS = 16`、`SCROLLBACK_MIN_ROWS = 5`、`AUTO_COMPACT_MAX_ROWS = 20`，并有编译期断言 `SHORT_TERMINAL_ROWS < AUTO_COMPACT_MAX_ROWS`。

| 触发条件 | 动作 | 证据 |
| --- | --- | --- |
| `terminal_rows <= 20` | 强制 compact：`outer_vpad` 1→0，`outer_hpad_left/right` 2→`MIN_HPAD = 1` | `views/agent.rs:102-104`、`appearance/config.rs:195, 198-218` |
| `area.height <= 16` | `bottom_vpad → 0`，`cta_height → 0`，`follow_ups_height → 0` | `views/agent.rs:205-216` |
| `top_vpad == 0` | `pane_gap` / `status_gap` → 0 | `views/agent.rs:229, 244` |
| 块可用高度 `< 3` | 放弃 vpad，优先保内容 | `scrollback_pane.rs:743-746` |
| `area_width < 60` | 不显示时间轴 rail（`RAIL_WIDTH = 2`） | `views/timeline.rs:14-20, 61-76` |
| 欢迎页 compact | `h_margin` 2 → `H_MARGIN_COMPACT = 1` | `views/welcome/mod.rs:110-112, 765-770` |
| 弹窗 `modal_width < 20 \|\| modal_height < 6` | 整体不渲染弹窗，清空命中矩形 | `views/modal_window.rs:253-259` |

compact 是**派生渲染值**，不写回用户设置也不落盘，重算时机仅为 Resize 事件、设置变更与启动（`appearance/config.rs:77-80`、`app/agent_view.rs:2150-2152`、`app/event_loop.rs:1601, 2540`、`app/dispatch/settings/setters.rs:806`）；`prompt.compact` 直接参与 `has_vpad_for`，所以 compact 会同时吃掉提示词块的上下空行（`blocks/user.rs:481`）。minimal 模式另有一套更激进的收缩：`block_pad_left = block_pad_right = 0`、`show_timestamps = false`，并用 `with_hide_accent(true)` 回收 accent 列（`xai-grok-pager-minimal/src/commit.rs:180-193, 214-222`）。输入框区域有独立的内边距、与滚动区不同（`views/prompt_widget/mod.rs:217-224`）：`vpad_top: 1`、`chrome_pad_left: 2`、`chrome_pad_right: 1`（注意左右不对称）；三种预设为默认（带 chrome 与边框）、`overlay()`（`chrome = false, vpad_top = 0`）、`inline(bg)`（另加两侧 chrome pad 归 0、`show_prefix = false`）（`:242-265`）。textarea 默认 `tab_width = 4`（与滚动区 `tab_width = 4` 对齐），滚动条可见时内容宽度再减 `1 + scrollbar_padding`，prompt widget 把 `scrollbar_padding` 设为 1（`xai-ratatui-textarea/src/textarea.rs:246-247, 3112-3126`、`appearance/config.rs:160`、`views/prompt_widget/mod.rs:708`）。

**排版参数表**（来源优先级：代码 `Default` 实现 > 用户手册；两者已逐项核对一致，`docs/user-guide/05-configuration.md:703-770`）：

| 可配置项 | 默认 | 单位 | 位置 |
| --- | --- | --- | --- |
| `scrollback.layout.outer_vpad` | 1 | 行 | `appearance/config.rs:184` |
| `scrollback.layout.outer_hpad_left` / `outer_hpad_right` | 2 / 2 | 列 | `:185-186` |
| `scrollback.layout.block_pad_left` / `block_pad_right` | 2 / 2 | 列 | `:187-188` |
| `MIN_HPAD`（常量，非键） | 1 | 列 | `:195` |
| `display.tab_width` / `group_max_visible` | 4 / 10 | 列 / 条 | `:160-161` |
| `display.expandable_indicator_char` / `dim_accent` | `"›"` / 0.5 | 字形 / 比例 | `:157, 152` |
| `display.collapsed_accent_char` | `glyphs::collapsed_accent()` = `❙` | 字形 | `:151` |
| `blocks.edit.indent` / `blocks.edit.vpad` | true / false | 开关（2 列 / 2 行） | `:400-401` |
| `blocks.edit.hunk_separator` / `dual_line_numbers` | `"…"` / false | 字符串 / 开关 | `:409-410` |
| `blocks.prompt.vpad` / `min_lines` | true / 2 | 开关（2 行）/ 行 | `:448, 450` |
| `blocks.thinking.truncated_lines` / `bg_blend` | 3 / 0.7 | 行 / 比例 | `:494-495` |
| `blocks.execute.first_lines` / `last_lines` | 2 / 3 | 行 | `:611-612` |
| `blocks.tool.bullet` | `Diamond`（`◆`） | 枚举 | `:527` |
| `ui.compact_mode` | false | 开关 | `:89`、`docs/user-guide/26-config-reference.md:593` |

| 硬编码常量 | 值 | 位置 |
| --- | --- | --- |
| `HorizontalLayout::ACCENT` / `EntryRenderer::CHROME_WIDTH` | 1 列 / `1 + 2 + 1 = 4`（legacy 估算） | `scrollback/layout.rs:17`、`wrappers/entry_renderer.rs:306` |
| Edit `INDENT` / `BODY_RAIL_WIDTH` / `HEADER_CONTENT_GAP` | `"  "`（2 列）/ 2 列 / 1 行 | `tool/edit.rs:103`、`blocks/thinking.rs:62`、`scrollback/sticky.rs:92` |
| `SHORT_TERMINAL_ROWS` / `SCROLLBACK_MIN_ROWS` / `AUTO_COMPACT_MAX_ROWS` | 16 / 5 / 20 行 | `views/agent.rs:91, 94, 97` |
| `RAIL_WIDTH` / `MIN_TERMINAL_WIDTH` / `H_MARGIN` / `H_MARGIN_COMPACT` | 2 / 60 / 2 / 1 列 | `views/timeline.rs:17, 20`、`views/welcome/mod.rs:110, 112` |
| `HERO_BOX_MIN_WIDTH` / `V_PAD` / `H_INSET` / `LOGO_H_PAD` | 90 / 1 / 2 / 3 | `views/welcome/hero_box.rs:14, 17, 20, 23` |
| `ModalSizing::default()` / `medium()`；弹窗最小可视尺寸 | `0.9 / 140 / 60 / 7 / 2 / 2 / 2`、`0.60 / 120 / 44 / 4 / 2 / 1 / 2`；20 列 × 6 行 | `views/modal_window.rs:109-135, 253` |
| 表格单元格 padding / `PROMPT_ARROW_WIDTH` | 1 列 / 2 列（`❯ `） | `parse.rs:1590`、`pager-render/src/glyphs.rs:16-18` |
| mermaid `MAX_LABEL/PAD/GAP_X/GAP_Y/WRAP_WIDTH/MAX_LINES`、`SEQ_GAP`；status-line `MAX_PADDING_PER_SIDE` | 28 / 1 / 3 / 2 / 24 / 4、5；16 列 | `mermaid.rs:28-34, 3374`、`xai-grok-status-line/src/config.rs:156` |

**未找到证据的项**：行距（只在 `RtOptions.line_ending` 看到 `LineEnding::LF`，没有「行高 > 1」的概念）；「代码块后必空行」（只找到「开栅栏前空行」，闭合栅栏后由源文档决定）；窄屏「标签变短」的映射表（找到字号 / 内容降级、rail 隐藏、面板行删除，但**未找到**任何把 `Thinking...` 缩成 `…` 之类的文案缩短表）；全局设计令牌模块（`LayoutConfig` 只覆盖滚动区，`spacing` / `padding` / `indent` 三词没有一个统一的常量模块）；CJK 禁则换行（无专门断行规则，行为完全由 `textwrap` + `unicode_width` 决定）；`block_pad_left/right` 的合法区间校验（`LayoutConfig::validated()` 只 clamp `outer_hpad_*`，`block_pad_*` 原样透传，`appearance/config.rs:220-229`）。

### 3.4 Markdown 的终端化呈现

#### 本节要点
- 解析器复用生态 crate、渲染完全自研：`pulldown-cmark 0.13` 只出事件流，配置集中在 `xai-grok-markdown-core/src/lib.rs:18-24`（GFM + STRIKETHROUGH + MATH + TASKLISTS + TABLES）；core crate 刻意只依赖 `pulldown-cmark` / `strum` 以便无头复用。
- 中间结构**不是 AST 而是「并行缓冲 + 字节范围」**：`MarkdownParser` 把 `highlights[]`、`transforms[]`、`replaces[]`、`table_replaces[]`、`mermaid_replaces[]` 分别塞进 `Vec`，每个元素带源字节 `Range<usize>`（`parse.rs:553`、`render.rs:195-260`）。
- 语法标记靠 `Effects::HIDDEN` 隐藏而**不是删除文本**：`all_hidden()` 判定「所有活跃样式都 hidden」才跳过，合并时 HIDDEN 被剥离，且 BOLD 与 DIMMED 互斥（`style.rs:210-225, 244-249, 260`）。
- 链接渲染成 `text (url)`、URL 始终可见；点击由 OSC 8 提供，`id=` 由终端层重铸，URL 先剥离控制字符（`parse.rs:1176-1185`、`terminal.rs:57-95`）。
- 流式只重渲染尾部，**冻结只发生在顶层块边界**（`depth == 0` 且有空白行或围栏已闭合），未闭合围栏永远不进冻结区，复杂度由 O(N²) 降到约 O(N)（`checkpoint.rs:1-14`、`output.rs:527-541`）。
- 对接面极窄：**没有自定义 ratatui Widget**，只产出 `Vec<Line<'static>>` 加四个并列元数据数组（`output.rs:50-67`）。

**数据流**：markdown 文本 → `LatexDelimiterNormalizer`（`\(…\)` / `\[…\]` / `\begin{equation}` → `$…$` / `$$…$$`，`latex_delimiters.rs:1-7`）→ `pulldown-cmark` Event 流（`lib.rs:18-24`；`offset_events()` 附带字节 range 并把单波浪线 `~x~` 降级为字面量，`lib.rs:29-86`）→ `MarkdownParser::on_event`（`parse.rs:553`）分别写入 highlights / transforms / replaces / table_replaces / mermaid_replaces → `render_ratatui()`（`render.rs:530`）按事件位置游标推进、pretty 模式下 `all_hidden` 的段直接跳过 → `Vec<Line<'static>>` + `line_source_map` + `hyperlinks` + `code_blocks` + `tables`（`output.rs:50-67`）→ `MarkdownContent::output()`（`markdown_content.rs:369`）词折行 + 引用条裁剪 + 背景传递 → scrollback 渲染 → `xai-ratatui-inline` 帧 diff → 终端。

**一处刻意的语义偏离**：`ENABLE_STRIKETHROUGH` 让 pulldown 把 `~x~` 也当删除线，Grok 通过 `DoubleTildeOnlyStrike` 过滤器把单波浪线对降级成字面量 `~`，动机写在 `lib.rs:8`——LLM 常输出 `~**10%**`（「约 10%」），被误判成删除线会造成严重误读；测试 `bill_single_tilde_percent_is_not_strikethrough` 就是这条规则的真实来源（`lib.rs:488-495`）。

**块级元素逐项呈现**：

| 元素 | pretty（默认） | raw（`r` 键切换） | 样式来源 |
| --- | --- | --- | --- |
| H1–H6 | `#` 与紧随空格**隐藏**；标题文本按级别取色 + 该级修饰符 | 显示 `## ` 前缀，用 `heading_outer`（同色 + DIM + HIDDEN） | `parse.rs:885-901`、`md_style.rs:120-122` |
| 段落 | 纯文本；软换行折叠成空格 | 同左（`force: true` 的 transform 在 raw 模式也生效） | `parse.rs:735-755`、`render.rs:160-162` |
| 围栏代码块 | 上下 ``` 行隐藏，但**在开栏处补一个空行**；代码体带 syntect 高亮 + 行级背景色 | 显示 ``` 与 info string | `render.rs:652-675, 739/818/836` |
| 4 空格缩进代码块 | 同上（无 info string） | 同左 | `parse.rs:312` |
| 引用块 | 每层 `>` → `│`（U+2502），默认可见；续行自动重复前缀 | 显示 `>` | `parse.rs:902-943`、`wrapping.rs:348-355` |
| 无序列表 | `- ` / `* ` → `• ` | 显示 `- ` / `* ` | `parse.rs:1037-1043` |
| 有序列表 | **保持源文本编号**（`1.` / `1)` 原样），仅套 `list_item` 颜色 | 同左 | `parse.rs:1021-1036` |
| 任务列表 | `[x]` / `[ ]` **原样保留**，仅套 `md_task_checked` / `md_task_unchecked`+DIM | 同左 | `parse.rs:859-866`、`md_style.rs:134-135` |
| 分隔线 | 整行标记替换为**固定 3 个 `─`**（不按宽度拉满） | 显示 `---` | `parse.rs:835-857` |
| 表格 | 整表替换为预排版盒线表 | 显示源码 `\| a \| b \|` | `parse.rs:1048-1070, 1298-1321` |
| HTML 块 / 行内 HTML | 块当普通文本渲染、**不做代码高亮**；`<br>`（含 `<br/>`、`<BR>`）→ 换行，其它标签按 HTML 语法高亮 | 同左 | `parse.rs:211-222, 762-790, 1001-1006` |
| 显示数学 `$$…$$` | 转为 Unicode 块，每行前置 2 空格，可多行 | 显示 TeX 源 | `parse.rs:1535-1585` |

**列表标记只认 `- ` 和 `* `**（`marker_len` 判定：`"- "` / `"* "` 为 2，否则找 `". "` / `") "` 且要求前面全是数字），因此 **`+ item` 既不套色也不替换**、原样显示（`parse.rs:1019-1027`）。**引用块的 `>` 是「逐层挑选」后替换的**：用 `skip = bq_depth - 1` 跳过外层已处理的 `>`，只把本层那一个变成 `│`，嵌套引用因此渲染成 `│ │ text`（`parse.rs:915`；测试 `test_nested_blockquote_with_list` 断言 `["│ Foo", "│", "│ │ Bar", "│ │", "│ │ • Baz"]`，`render.rs:1245-1261`）。**引用块续行有前缀、列表续行没有**：

```
引用块（续行重复竖线，左）              列表（续行顶到列 0，右）
┌──────────────────────────────────┐   ┌──────────────────────────────────┐
│ Foo                              │   │ • 一个很长的列表项文字在窄终端里  │
│ │ Bar                            │   │ 会被折成多行……                    │
│ │ • Baz                          │   └──────────────────────────────────┘
└──────────────────────────────────┘
```

右半的行为由 `render_tests.rs:3248-3250` 的注释确认——「Top-level list items render as `• …` with no quote-bar indent on continuation rows」——**不是疏漏，而是被测试锁定的设计**。

**行内元素**：

| 元素 | pretty 呈现 | 样式键 |
| --- | --- | --- |
| `**粗体**` / `*斜体*` / `~~删除线~~` | 加粗 / 斜体 / `CROSSED_OUT`，标记本身隐藏 | `strong_inner` / `emphasis_inner` / `strikethrough_inner`（各自另有 `_outer`） |
| `` `行内代码` `` | 内容套 `md_code` 色 + 粗体；**反引号本身也着同色**（DIM + HIDDEN 隐藏） | `parse.rs:1437-1467`、`md_style.rs:129-130` |
| `[文本](url)` | 重写为 `文本 (url)` | `link_text` / `link_url` / `link_title` / `link_outer` |
| `![alt](url)` | 同链接，`!` 与 `[` 一并去掉 → `alt (url)` | `parse.rs:1089-1094`、`render.rs:1227-1240` |
| `$E=mc^2$` | 转 Unicode → `E=mc²`，斜体；转换失败则退化成行内代码呈现 | `parse.rs:699-734`、`md_style.rs:148` |
| HTML 实体 `&lt;` `&amp;` | 解码替换（**拒绝含控制字符的实体**，防转义注入） | `parse.rs:139-149, 1472-1530` |
| 脚注引用 | 仅套 `link_outer` 色；**脚注定义块的渲染未找到证据** | `parse.rs:832-834` |

`math: fg(theme.md_text).italic()` 说明行内数学的取舍是「正文色 + 斜体」，与行内代码（`md_code` 色 + 粗体）刻意区分。**未支持 / 未找到证据**：脚注（`parser_options()` 未显式列 `ENABLE_FOOTNOTES`，`Event::FootnoteReference` 的处理是全仓唯一一处）、删除线之外的 GFM 扩展（自动目录、emoji 短代码、定义列表、上下标等均未启用也无处理分支）、图片的图文渲染（Markdown 图片只降级成 `alt (url)` 文本链接，Grok 的图像渲染走另一条独立路径）。

**样式模型：27 个字段的 `_inner` / `_outer` 二元结构**（`style.rs:124-156`，全部字段是 `anstyle::Style` 而**不是** ratatui 的）：

> The `_inner` variants are applied to the content, while `_outer` variants are applied to the syntax markers (which are hidden in pretty mode).（`style.rs:123`）

即 **`_outer` 的用途就是「给要隐藏的语法标记上色，供 raw 模式显示」**：标题 `##`、`**`、`*`、`~~`、反引号、链接括号、代码围栏都有对应 `_outer`；`blockquote_outer` 是唯一「不 hidden」的 `_outer`——它染的是替换后的 `│`，那个字符在 pretty 模式必须可见。`build_style()`（`md_style.rs:96-150`）是唯一的映射函数，每次调用都重新从 `Theme::current()` 现取：**标题是 6 元数组 + 6 个独立修饰符**（`heading_inner_styles` 把该级颜色与 `md_heading_hN_mod` 的 effects 按位或合并，`heading_outer_styles` 则**只用颜色、丢弃修饰符**，统一 `.dimmed().hidden()`，`:66-92`）；**`_outer` 大多是常量**（`Style::new().dimmed().hidden()`），不依赖主题——主题只决定「内容长什么样」，「标记怎么淡出」由渲染器决定；**一份颜色可能被多个元素共用**（`md_heading_h2` 同时用于 H2 和表格外框色、`md_heading_h3` 同时用于 H3 和代码语言标签、`md_heading_h5` 同时用于 H5 和链接标题、`md_muted` 同时用于引用块 / 列表标记 / 分隔线 / 链接 URL，`theme/tokyonight.rs:125-143`）。

主题侧为 Markdown 留了 12 个色位 + 6 个修饰符（`md_heading_h1..h6` 及 `_mod`、`md_code`、`md_task_checked`、`md_task_unchecked`、`md_muted`、`md_code_bg`、`md_text`、`link_fg`），注释说明「These default to the corresponding top-level theme colors but can be overridden per-theme」：

| 字段 | TokyoNight Storm | GrokNight | terminal-default |
| --- | --- | --- | --- |
| `md_heading_h1` | `TEAL` + BOLD | `TEAL` + BOLD | `Color::Reset` + **BOLD ∪ UNDERLINED** |
| `md_heading_h4..h6` | RED / GREEN / MAGENTA | — | 全 `Reset` + BOLD，H6 用 MUTED |
| `md_code` | `GREEN1` | `BLUE1` | `Color::Cyan` |
| `md_code_bg` | `BG_HIGHLIGHT` | `rgb(28,28,28)` | `Color::Reset`（无背景） |
| `link_fg` | `BLUE` `#7aa2f7` | `rgb(122,166,218)` | `Color::Blue` |

（`tokyonight.rs:212-223`、`groknight.rs:117-118`、`terminal_default.rs:91-101`。）`terminal_default` 把 H1 设为 `BOLD | UNDERLINED`，是**「没有颜色预算时靠修饰符制造层级」的明确示范**。修饰符也有主题级差异：RosePine 的 h2 是 `BOLD ∪ UNDERLINED`、h4 是 `BOLD ∪ ITALIC`（`rosepine.rs:97, 101`），Oscura 的 h4 是 `BOLD ∪ ITALIC`（`oscura.rs:122`），GrokNight / GrokDay 只用 BOLD。量化在 `theme/mod.rs:269-286` 统一处理，`md_code_bg` 走特殊的 `push_away` 分支以保持与背景的对比（`:401`）。

**嵌套样式合并规则**（`merge_styles()`，`style.rs:229-261`）：**BOLD 与 DIMMED 互斥**（后出现的那个会移除前一个，因为多数终端把 DIM 实现为「降低亮度」，与粗体同开会互相抵消、视觉上不可预测）；**HIDDEN 会被剥离**（它在合并阶段只充当「跳过这段」的标记，绝不进入最终渲染样式）。「这段是否该跳过」由 `all_hidden()` 判定——**所有**活跃样式都含 HIDDEN 才跳过，这保证 `**粗体**` 里的 `**` 会因为外层 `strong_inner` 不含 HIDDEN 而保留。另有一次颜色表示转换：anstyle → ratatui 时 `Color::Reset` 映射为「No fg」（`md_style.rs:8-11`，有回归测试 `reset_maps_to_no_color`）；命名色不一致——ratatui 的 `Gray` → anstyle `White`，`DarkGray` → `BrightBlack`（`:21-22, 166-180`）。

**表格布局完全自研**（pulldown 只给 `alignments` 与单元格事件，排版由 `format_table()` 完成，`parse.rs:1588-1864`）：边框字符集**硬编码 `TableBorders::BOX`**、内边距固定 1 列、边框色 = 主题 `rule` 字段（即 `md_muted`）**再叠一层 DIM**，所以框线比正文明显更暗（`parse.rs:1588-1593`）；**表体每两行之间都插一条 `├─┼─┤`**，这不是可选装饰而是固定输出，表头行强制加粗（`parse.rs:1750-1854, 2039-2041`）。

列宽算法两阶段。**阶段一 · 自然宽度**：逐单元格取 `plain_text()` 按 `\n` 分行后的最大显示宽度，按列取 max，用 `unicode_display_width`（CJK 按 2 算）（`parse.rs:1609-1622`）。**阶段二 · 收缩**（仅在 `max_table_width = Some(w)` 时）：`overhead = num_cols * (2 * padding + 1) + 1`，`content_budget = max_width.saturating_sub(overhead)`；2 列表格的 `overhead = 2*3+1 = 7` 正好是 4 个 `│` + 4 个空格 + 2 个 `┬` 少算 1 的修正，该公式在 playground 里被独立复现为「最小渲染宽度 = `4k + 1`」（`parse.rs:1626-1727`、`bin/md_table_test.rs:40-46`）。收缩先算两个下限——`min_col_widths`（该列**最长不可断词**的显示宽度）与 `hard_floors`（该列**最宽单个 grapheme** 的宽度，`max(1)`，即「再窄就会丢内容」的极限）（`:1634-1659`），再按 `min_total` / `hard_total` 相对预算的位置选基准与目标：**词宽放不下就退到 grapheme 下限**（`min_total > budget && hard_total <= budget` 时基准取 `hard_floors`、目标取 `min_col_widths`），**否则从词宽往自然宽长**（基准取 `min_col_widths`、目标取自然宽度）。

**这里没有等分**：`extra_budget = budget - base_total` 先按各列「缺口占比」做浮点比例分配并 `floor()`，再把取整丢掉的**逐列补偿给缺口最大的列**（按 `unmet` 降序排，每次 +1 且不超过该轮目标宽），注释直说原因「Hand out any remaining columns (from floor rounding) to columns with the most unmet want, one at a time」（`:1689-1722`）；若连 grapheme 下限都放不下则**保持 `min_col_widths` 不动**（「When even the grapheme floors cannot fit, keep the word minimums: downstream clipping remains the safety net」，`:1666-1674`）。上游专题给出的 6 列 × 宽度 30 示例（最终列宽 `[3,1,1,3,2,1]`、每行宽度 `11 + 19 = 30`）的渲染形状是**按算法手工推算**的，`markdown_content.rs:515-543` 的 `test_six_col_table_output_30_keeps_right_border` 实际只断言两件事：每行显示宽度**恰好等于 30**，且每行末尾字符是 `│`/`┐`/`┘`/`┤` 之一（注释解释了为什么要锁右边框：「A table that overflows the budget gets hard-clipped by the wrap layer, which eats the right `│`」）。

超宽单元格用 `wrap_cell_text_joins()` 折行（`FirstFit` + 自定义词分隔符 + `break_words(false)`）。自定义词分隔符 `cell_word_separator` 三遍扫描（`parse.rs:226-390`）：找断点（空白处必断；标点后跟字母必断如 `foo/bar`；「数字-标点-数字」会断，**但 `,` 和 `.` 例外**，所以 `$145,000`、`3.14` 保持完整）；**URL 保护**（用 `url::Url::parse` 逐个 token 验证、命中范围内的断点全部丢弃，理由是 URL 内断开会让终端的 Cmd+Click 超链接识别失效）；标点归属（在「贴左 / 贴右」之间选让左右段最大值更小的那一侧，平手贴左）。行内格式跨折行时**被精确保留**：`format_styled_content_lines` 先算出本可视行在完整单元格明文中的字节区间，再让该单元格的每个 span 与这个区间求交、逐段套原样式，同一列维护单调递增的 `source_cursors` 防止重复子串被错配到更早位置（`:1917-2097, 1950-1952`）。

**与折行层的关键契约：表格行绝不折行**（判定规则见 3.2）。表格的**背景色不会被行级背景覆盖**，但单元格背景随整行传递（`markdown_content.rs:395-399`）；又因为表格行非折行，「一个渲染行 = 一个 block 行」，选择层据此实现**按单元格选择**（`scrollback/table_geometry.rs:1-2`：`Box-drawing table grid detection so selection inside rendered tables operates on cells`）。**对齐逐列独立处理**（来自 GFM 的 `alignments`）：`Left` 或 `None` → 左填充 0、右填充全部余量；`Right` → 相反；`Center` → `total/2` 与 `total - total/2`。`total_padding = col_width - cell_line_width`，再在两侧各加固定 1 列 padding；**表头单元格也参与对齐**，但额外强制加粗（`parse.rs:1729-1737, 1971-1986`）。

**链接：先重写成 `text (url)`，再折叠到屏幕坐标，最后发 OSC 8**。

- pretty 模式推入两条 transform：删掉 `[`，把 `](` 重写成 ` (`（`parse.rs:1176-1185`），于是 `[text](url)` 显示为 `text (url)`——**URL 直接可见，不需要 hover 才能看到目标**；`force: false` 意味着 raw 模式保留原始括号。可视文本与 URL 分色：`link_text = fg(link_fg).underline()`、`link_url = fg(md_muted)`，标题（`[a](u "T")`）用第三种颜色 `link_title = fg(md_heading_h5)`（`md_style.rs:139-141`）。链接内部的粗体 / 斜体**不得覆盖链接色**：`parse.rs:564-572` 会把链接祖先样式的 fg 剥掉、只让 effects 通过，理由是「merge_styles is last-wins on fg, so keeping the fg would clobber the link color」（测试 `formatted_link_keeps_link_fg_and_gains_effects`，`hyperlinks.rs:658`）。
- **三层坐标系**：源字节偏移 `LinkTarget::source_range` → `apply_transforms` 之后的字节 → 显示单元格 `(line_index, display_column)`（`hyperlinks.rs:1-10`）；1→2 由 `source_to_chunk_offset()` 完成，含一条「跨骑策略」：若某 transform 的源区间包含目标位置，位置就钳到替换串起点，并有 `debug_assert!` 兜底（`:34-70`）。公共输出 `HyperlinkTarget { line_index, column_range, url, id }`，`id` 用于把「同一逻辑链接的折行片段」分组（`output.rs:11-21`）；表格单元格里的链接走独立路径 `TableHyperlink`，因为「The paragraph link path (LinkTarget + chunk_link_offsets) can't project links onto rendered table cells」（`parse.rs:1097-1103`）。裸 URL 由后置扫描补齐（`url_scan::detect_plain_urls`，`lib.rs:122-127`），扫描器发的 id 从 `1 << 31` 起（`render/osc8.rs:100` `SCANNER_ID_BASE`），与 Markdown 自己的小 id 空间隔开，避免「扫描到的 URL」和「Markdown 链接」因同 id 被终端当成同一条。
- `map_hyperlinks_to_overlay()` 把 pre-wrap 的 `HyperlinkTarget` 映射到屏幕坐标，需处理折行分段、joiner 占位列与续行 `indent_width` 扣减；**不安全的 scheme 在这一步被丢弃**（`javascript:`、`data:` 等），因为 OSC 8 的 URL 是直接写进终端的、不经过 `link_opener` 的 scheme 过滤（`scrollback/render.rs:834-931`）。真正的转义序列由帧 diff 层发出：`\x1b]8;id={id};{url}\x07` 与关闭序列 `\x1b]8;;\x07`（`xai-ratatui-inline/src/terminal.rs:55-72`），三个实现细节是——**URL 先做控制字符剥离**（否则恶意 Markdown 可以靠 `\x1b` 提前终止序列注入转义）；**`id=` 是重铸的**（Markdown 的 id 每文档从 0 重数而 OSC 8 的 `id=` 是终端全局的，全屏应用必须重铸、否则不同消息里的同号链接会被 Windows Terminal 的 hover / Ctrl+Click 合并成一条）；**链接参与帧 diff**（样式相同但链接不同的单元格也会被判为变化，发射时按「连续相同链接」合并成 run，一个 run 一个开-闭序列，即使 cell draw 出错也总会 `write_osc8_close`）（`:74-95, 385-420, 1070, 1114-1164`）。终端能力分级 `Osc8Support` 有 `Native` / `HostileParser` / `Unsupported` / `Unknown` 四档（Apple Terminal 归 `HostileParser`、Warp 归 `Unsupported`、JetBrains 与未知终端归 `Unknown`）（`pager-render/src/terminal/hyperlinks.rs:59-175`、`terminal/mod.rs:437-460`）。**点击行为本身由终端负责**（Cmd+Click / Ctrl+Click），Grok 侧只提供 OSC 8 与安全 URL。

**流式（半截语法）**。`checkpoint.rs:1-14` 给出核心约束：

> Checkpoints are only created at **top-level** (depth=0) block boundaries. Blocks nested inside lists, blockquotes, or tables cannot be checkpoints because the outer container might continue.

深度由 `depth` 计数器维护（进入 / 离开 `BlockQuote | List | Item | Table` 各 ±1），只有 `depth == 0` 才记录 checkpoint，且还需满足「后面有空白行」**或**「代码块已正确闭合」；checkpoint 类型共 8 种：`Heading`、`Paragraph`、`CodeBlock`、`BlockQuote`、`List`、`ThematicBreak`、`Table`、`HtmlBlock`（`parse.rs:872-877, 1330-1359`、`checkpoint.rs:37-58`）。渲染器据此维护冻结前缀，`rerender_tail()` 的流程是：把 output 截断回冻结边界（行 / `line_source_map` / hyperlinks / tables / code_blocks）→ 只渲染 tail（并修正 tail 起始处可能多出的 `\n`）→ 结果整体偏移后追加 → 对 tail 做裸 URL 扫描 → 若 tail 渲染返回 checkpoint 则推进冻结边界（`streaming.rs:105-142, 257-369`）。

| 半截语法 | 处理方式 | 证据 |
| --- | --- | --- |
| 未闭合的代码围栏 | pulldown 在输入结束处**会合成一个 block end**，所以 `TagEnd::CodeBlock` 不能证明围栏闭合；判据是 `pending.body_range.end < range.end`（闭合行一定在正文之后） | `parse.rs:1243-1256` |
| 未闭合围栏的代码块跨度 | 无 `CodeBlockSpan`，且流式下该跨度**永远不进入冻结前缀** | `output.rs:255-271`、测试 `streaming_open_fence_has_no_frozen_span`（`:527-541`） |
| 未闭合围栏的整体识别 | 交给独立的分析 crate 报 `StructuralIssue::UnterminatedCodeBlock`（判据：opener 之后没有任何长度 ≥ opener 且同字符的闭栏行） | `lib.rs:210-224` |
| 半截表格 | 只有 `TagEnd::Table` 才产出 `TableReplace`；在分隔行 / 表体到达前，表格**一直以段落形式显示**，之后整块重排为盒线表 | `parse.rs:1298-1321` |
| 表格列数不匹配 | `StructuralIssue::MalformedTable` 专门检测这个失败模式（「the 'made a table but it didn't show' bug」） | `lib.rs:186-188, 246-270` |
| 半截链接 | 未闭合时 pulldown 不发 `Tag::Link`，`[text](ur` 以字面量显示，等 `)` 到达后重排 | 由 `Tag::Link` 进入条件推得（`parse.rs:1089-1204` 整段依赖完整 range） |
| 半截 LaTeX 定界符 | `LatexDelimiterNormalizer` **扣住有界的歧义后缀**到下一块，`finish()` 时 flush；扣留量受数学长度上限约束，「永不闭合的 opener 不能卡住流」 | `latex_delimiters.rs:45-51` |
| 单波浪线 | 非流式问题，`DoubleTildeOnlyStrike` 在事件层降级为字面量 | `lib.rs:36-86` |

`pulldown-cmark` **从不报错**（`lib.rs:182`：「every input parses as something」），所以「半截」永远表现为「退化渲染」，不能用错误处理。这正是 `xai-grok-markdown-core` 存在的理由——它用一个独立的 `analyze()` 把「用户 / 模型意图了什么结构 vs 实际解析出了什么」做差集，产出 `MarkdownAnalysis { stats, issues }`（`lib.rs:272-333`）。

**防闪烁 / 防跳动**（7 条机制）：① 冻结边界只在语义安全处推进，未闭合围栏与未完成表格都留在 tail——任何「先显示成段落、后来变成表格」的重排都发生在视觉最底部；② 折行缓存按 `(width, generation, theme)` 三元组命中，滚动（内容与 generation 不变）直接返回缓存（`markdown_content.rs:27-38`）；③ 折行也做**增量**——`ensure_wrapped()` 只重折「新冻结的行」与「tail 行」两个区间，已冻结行的结果保留在 `cache_lines[0..frozen_wrapped_count]`，测试 `frozen_cache_is_reused` 断言冻结计数单调增长（`:270-352, 650-673`）；④ 主题切换不重置内容——`cache_theme` 变化时只调 `renderer.set_style()` 再强制缓存失效、不复用旧色（`:276-283`）；⑤ 尾部未闭合代码块走**增量高亮**——`OpenCodeHighlighter` 让「一个正在流式增长的代码块」总高亮代价是 O(N) 而非 O(N²)（`parse.rs:182-185`、`streaming.rs:134-136`）；⑥ **帧级 diff**——只有变化的单元格被写往终端，未变的单元格不产生字节（`terminal.rs:385-420`），这是全屏 TUI 不闪烁的最后一道保障，与 Markdown 层无关但必须配合；⑦ **收敛保证**——`finish()` 做**一次全量重渲染**以保证正确性，`MarkdownContent::finish()` 随后把冻结计数清零使下一次 `ensure_wrapped` 全量重折，两条测试锁定「流式结果 == 一次性渲染结果」：`incremental_wrap_matches_full_wrap` 与 `streaming_chunked_matches_full_render`（用 `[4, 9, 1, 13, 7]` 循环的不规则 chunk 边界）（`streaming.rs:432-486`、`markdown_content.rs:144-152, 594-646`、`output.rs:544-574`）。`syntect` 句柄由 pager 侧统一供给，并在主题 / 风格 / 表宽 / 软换行模式变化时清空缓存，避免主题切换后残留旧配色高亮（`markdown_content.rs:90, 131`、`streaming.rs:194-224`）。

**与 ratatui 的对接面极窄**——渲染器只产出 `Vec<ratatui::text::Line<'static>>` 加若干并列元数据（`output.rs:50-67`）：`lines`、`line_source_map`（渲染行 → 源行号，供 raw / pretty 切换保持光标位置）、`hyperlinks`、`code_blocks`（围栏信息、干净 body、输出行范围、源字节范围）、`tables`（单元格抄写元数据）。

- **没有自定义 Widget**：Markdown 不用 `ratatui::widgets` 的任何东西，只返回 `Line`，由 pager 的 scrollback 渲染层负责摆位；这也解释了为什么行级背景色要写在 `Line::style` 上——这是唯一能跨 span 传递「整行底色」的通道（`render.rs:555, 739, 818, 836`）。
- **元数据是并列数组而非嵌套结构**，用 `output_line_range` / `line_index + line_count` / `(line_index, column_range)` 指回 `lines`，使 pager 可以在**折行之后**重新投影这些坐标（`output.rs:36-39`、`markdown_content.rs:185-227`）。**折行发生在 pager 侧**：`MarkdownContent::ensure_wrapped()` 调 `word_wrap_lines_with_joiners`，并把每个折行点「被吃掉的空白」记成 joiner 供超链接列坐标回算（`markdown_content.rs:312, 325, 369-405`）。
- **引用条在输出阶段被识别并排除出选区**：`QuoteBarStrip` 把行首 `│ ` 前缀标成不可选中，且 `quote_bar_style()` 必须与 `md_style.rs:133` 的 `blockquote_outer` **逐字段一致**——两侧注释互相指向并各有端到端测试锁死（`markdown_content.rs:371, 383-386`、`quote_bar.rs:53-61`）。
- **raw / pretty 是块级开关，默认键位 `r`**（`docs/user-guide/03-keyboard-shortcuts.md:66`：「Toggle raw markdown on selected entry」）；切换会 `set_pretty` → 全量重渲染并清冻结状态，同时 generation +1 使折行缓存失效（`streaming.rs:408-418`、`markdown_content.rs:255-266`）。

### 3.5 代码高亮与 diff 的呈现

#### 本节要点

- 高亮用 **syntect**，语言集是 build 期由 `two_face::syntax::extra_newlines()` 加一份补丁版 Swift 语法合成的 `syntaxes.bin`，运行期 `OnceLock` 反序列化——不是运行期建字典。
- 语言判定只有两条路：**文件扩展名**与**显式语言 token**（含 `37:65:crates/…` 这种行号引用形式的围栏）；**没有 shebang 解析，也没有内容嗅探**。
- 首帧只高亮 hunk，全文件升级交给单独线程 `edit-hl`，作业按 `entry_id` 合流（later wins）；全文件高亮有 **2 MiB / 50000 行**两道硬上限，超限即永久停留在 hunk-only。
- diff 数据层极薄：`DiffLine { text, lo, ln, tag }` 与 `DiffHunk = Vec<DiffLine>`，上下文固定 3 行，整文件 diff 500 ms 超时后接受「更粗但仍正确」的结果。
- 呈现上**不画 `+` / `-` 行首符号**，增删只靠行号颜色与内容背景带；行号栏可切换为旧/新双列，但**全仓没有并排内容（side-by-side）模式**。
- 降级层级显式：syntect 失败 → 纯色前景；未知语言 → 单色代码样式；超 caps → hunk-only。没有「静默出错」这一档。

本节路径省略 `reference/grok-build/` 前缀；下文 `tool/edit.rs`、`tool/read.rs` 指 `crates/codegen/xai-grok-pager/src/scrollback/blocks/tool/` 下的同名文件。

#### 3.5.1 四层职责与两条平行线

代码分四层：**数据层** `crates/codegen/xai-grok-pager-diff/src/lib.rs`（单文件 crate，编辑工具输出 → 带行号的 `DiffHunk`，并能反向生成 unified patch）；**颜色层** `crates/codegen/xai-grok-pager-render/src/syntax.rs` 与 `src/theme/`（syntect 单例、`.tmTheme` 选择、RGB→终端色量化与极性安全映射、diff 六个色槽）；**渲染层** `tool/edit.rs`（3185 行，负责 gutter、背景色带、软换行、hunk 分隔行、头部 diffstat）；**异步层** `crates/codegen/xai-grok-pager/src/app/edit_highlight_worker.rs`（后台做「全文件作用域」高亮升级）。另有两条平行线：`crates/codegen/xai-hunk-tracker/`（diff 归属与 LOC 统计，**不参与 TUI 渲染**）与 `crates/codegen/xai-grok-markdown(-core)/`（assistant 消息里 markdown 围栏代码块的高亮）。

#### 3.5.2 语法高亮：crate、语言集与判定

- pager 层不自己实现高亮，直接复用 markdown crate 的类型：`xai-grok-pager-render/src/syntax.rs:15` 的 `pub use xai_grok_markdown::Syntect;`；`Syntect` 是一个 `{ theme, syntax_set }` 二元组（`xai-grok-markdown/src/syntax.rs:17-22`，注释标注 "supports 250+ languages via two-face"）。
- 语言集在 build 期生成：`xai-grok-markdown/build.rs:12-21` 用 `two_face::syntax::extra_newlines().into_builder()` 追加一份 `assets/Swift.sublime-syntax`，再 `dump_to_uncompressed_file` 成 `syntaxes.bin`；运行期 `from_uncompressed_data(include_bytes!(...))` + `OnceLock` 缓存（`xai-grok-markdown/src/syntax.rs:92-100`）。build.rs 注明动机：运行期 `SyntaxSet::build` 会拖慢首次 Read 绘制（`build.rs:9-11`）。那份 Swift 语法是**补丁版**：`is_swift_token`（`syntax.rs:102-104`）命中时不查 `SyntaxSet`，而是从 `syntaxes()` 反向找**最后一个**带 `.swift` 扩展名的语法（`:106-114`）。
- 主题是三个内建 `.tmTheme`（`grok-night` / `grok-day` / `tokyo-night`，各约 40 KB），按 `ThemeKind` 走 `OnceLock<Syntect>` 单例；**GrokNight、RosePineMoon、OscuraMidnight、Terminal、Auto 五种 kind 共用 night 主题**（`xai-grok-pager-render/src/syntax.rs:119-134`）。`Terminal` / `Auto` 也走 night，是因为每个 token 的颜色随后会被重新映射，源调色板只需「完整」、极性无关（同文件 `:123-126` 注释）。用户手册把映射写成对外承诺，并明确主题文件**不可替换**（`docs/user-guide/06-theming.md:164-172`）。
- 判定只有两条路（`xai-grok-markdown/src/syntax.rs:37-51`）：`find_syntax_by_file_path`（扩展名直接丢给 `SyntaxSet`）与 `find_syntax_by_token`（markdown 围栏 info string，如 ` ```rust `）。第三条是前者变体：围栏 info 形如 `37:65:crates/.../read.rs` 时走 `find_syntax_for_fence_info`（`:72-89`），取**第二个冒号之后的整段**当路径解析，解析不出再整串当 token 试一次；`parse_line_citation_fence_info`（`:118-133`）逐段校验前两段必须是纯 ASCII 数字。
- **未找到证据**：全量检索 `shebang`、`find_syntax_by_first_line`、`sniff` 在 `xai-grok-pager-render/src/`、`xai-grok-markdown/src/`、`xai-grok-pager-diff/` 内**零命中**——判定入口只有「扩展名」与「显式 token」两类。
- 终端原生调色板下另有一条「极性安全」映射（`xai-grok-pager-render/src/syntax.rs:46-86`）：低彩度 token（`chroma = max - min < 40`）→ `Color::Reset`；有彩度的按 HSV 色相分桶映射到 **base ANSI-16**（`0..30 | 330..=360 → Red`、`30..90 → Yellow`、`90..150 → Green`、`150..210 → Cyan`、`210..255 → Blue`、其余 `Magenta`）。Magenta 从 255° 起算是为了让 Tokyo Night 的紫 `#bb9af7`（约 261°）落到 Magenta 而不是 Blue；**明确禁止 White/Black/Light\***，因为它们在反极性上会消失，`:164-205` 的测试遍历常见 night 主题 pastel 色断言这一不变式。
- 高亮失败分两条降级：diff 侧 `highlight_lines_by_file_path` 返回 `None` 时退回纯色 fg（`tool/edit.rs:706-734`；`xai-grok-pager-render/src/syntax.rs:90-115` 的 `highlight_line` 同样返回 `vec![Span::styled(text, fallback)]`）；markdown 侧未知 fence info 让 `syntax_highlight_raw` 返回 `None`，parse 退到 `ms.code_untagged`——**单色代码样式，仍有代码块背景但没有语法色**（`xai-grok-markdown/src/parse.rs:666-672`）。

> 交叉引用：本小节与 §3.1 在「ThemeKind → 语法高亮主题映射」上有内容重叠，两节结论一致。

#### 3.5.3 性能：廉价首帧 + 后台升级

`tool/edit.rs:1-19` 的模块文档把成本模型写成了表；「只走到最后一个 hunk 行」有明确实现：`max_needed = expected.keys().max()`，循环到该行即 `break`（`:292-299`）。文档同时承认：靠近文件末尾的 hunk 仍要为整个文件付 O(file lines) 的代价，这正是 caps 存在的原因。

| 阶段 | 行为 |
| --- | --- |
| First paint | 只做 hunk 级；**绝不在 UI 线程做全文件** |
| Upgrade | 后台一次 syntect 走查，只走到最后一个 hunk 行；绘制时把样式映射叠加到普通 hunk 渲染上 |
| 超限或读取失败 | 保持 hunk-only，不做无界工作 |

- **diff 内用两个 highlighter**，避免跨侧状态泄漏：`syntect.highlight_lines_by_file_path(path)` 取两份（`tool/edit.rs:193-214`，注释 "A diff interleaves two file versions; give each side its own highlighter so a multi-line construct can't leak across sides"）；Delete 只推进 old、Insert 只推进 new、Equal 用 new 渲染并**同时推进两者**（`advance_highlighter`，`:677-685`）。这是把 syntect 的逐行状态机正确映射到「交错的旧/新两侧」的关键处理。
- worker 是**一个**名为 `"edit-hl"` 的 `std::thread`，mpsc 收发（`app/edit_highlight_worker.rs:98-124`），刻意仿照 `mermaid_worker` 的形态。作业**按 `entry_id` 合流**，后者胜出、跨 key 保持 FIFO（`drain_coalesced`，`:126-134`）；没有 waker，靠每个 tick `try_recv` 轮询（`:281-297`），`edit_hl_needs_tick()` 让事件循环在有待办时保持快 tick（`:199-205`）；提交时先剪掉同 entry 的旧 pending（`rt.pending.retain(|(_, eid)| *eid != entry_id)`，`:243-245`），否则那些作业的结果永远不会到达、会把 fast tick 钉死；worker 死掉时 `abandon_edit_hl_worker` 把所有 `Pending` 回退成 `HunkOnly` 并丢弃 runtime 以便下次懒重建（`:346-364`，注释承认这在 `panic=abort` 的发行构建里近乎不可达）。
- 升级前必须验证「磁盘文本 == hunk 文本」：`compute_file_scoped_styles`（`tool/edit.rs:279-331`）逐行比对，任一不等即返回 `None`，**绝不用新内容覆盖屏幕上已显示的内容**；全部所需行命中才返回 1-based 新文件行号 → 样式的 map。覆盖范围另有讲究（`map_spans_for_line`，`:345-376`）：`Equal` 总可覆盖；`Insert` **只在有底部色带（banded）的主题**上覆盖；`Delete` 从不覆盖——保留用户已经看过的 per-hunk 配色（覆盖前再拼一次字符串做一致性检查，`joined != expanded` 即放弃）。样式被烘焙在某个 `ThemeKind` 下，运行期换主题即弃用：只有 `baked == crate::theme::cache::current_kind()` 才走带样式的渲染，否则回退 `render_diff_hunks_highlighted`（`:1012-1025`）。
- 硬上限（`tool/edit.rs:44-49`）：`EDIT_HL_MAX_BYTES = 2 * 1024 * 1024`、`EDIT_HL_MAX_LINES = 50_000`；判定入口 `file_text_within_hl_caps`（`:263-274`）。worker 侧 `read_file_capped` 在读之前按 `metadata().len()` 拦一次、读完再按实际字节数拦一次，并要求 `String::from_utf8` 成功（`app/edit_highlight_worker.rs:174-188`）。注释给的理由很直接：全文件 syntect 对多 MB 源码是糟糕的后台工作，不如保持 hunk-only。
- markdown 侧另有一套流式缓存（`xai-grok-markdown/src/open_code_highlighter.rs:1-33`）：仍在书写中的尾块跨 `rerender_tail` 持久化 syntect 的 `ParseState` / `HighlightState`，每行只高亮一次（否则每次 push 重跑整块，是 O(N²)；注释给的量级是约 1000 行块接近末尾时 ~35 ms/push）；被困在 tail 里的闭合块按 `(fence_info, body)` 记忆批渲染结果，`CLOSED_MEMO_CAP_BYTES = 256 * 1024`（`:43-44`），超出即整体清空——退化为重算而不会无界增长；任何 theme/style/width 变化都让流式渲染器整块丢弃（`:19-21`）。
- **未找到证据**：没有「只高亮可见区域」的裁剪（diff 渲染粒度是整块，可见性裁剪由上层 `max_lines` 与块高度预算承担）；也没有 per-hunk / per-line 的高亮结果缓存（`FileScoped` map 与流式 memo 之外无其他缓存）。

#### 3.5.4 diff 数据层：一行一格，上下文 3 行

- 类型极简（`xai-grok-pager-diff/src/lib.rs:11-19`）：`DiffLine { text, lo, ln, tag }`——`lo` / `ln` 是旧/新文件 1-based 行号，`tag` 直接复用 `similar::ChangeTag`；`pub type DiffHunk = Vec<DiffLine>`。crate 级 lint 是 `#![deny(clippy::indexing_slicing)]`（`:6`），全文用 `get` / `checked_sub` / `saturating_*` 访问——这是它能在渲染热路径上安全吞掉畸形输入的原因。
- `similar` 只承担**行级 diff 算法**，其余全部自建：逐行比较 `TextDiff::from_lines`（`:71`）、带超时的整文件比较 `TextDiff::configure().timeout(...).diff_lines(...)`（`:170-172`）、按上下文分组 `diff.grouped_ops(MAX_CONTEXT)`（`:174`）。**没有**使用 `similar` 的 `unified_diff()` 输出器——补丁文本是手写的。
- 关键常量（`:22-25`）：`MAX_CONTEXT = 3`、`WHOLE_FILE_DIFF_TIMEOUT = 500 ms`。超时的语义是「`similar` 返回更粗但仍正确的 diff」，不是失败（`:24` 注释）；注释另说明「相隔超过两倍 `MAX_CONTEXT` 的未改动行会分进不同 hunk，所以整文件 diff 永不显示中间未改动部分」（`:168`）。
- 三条提取策略依次尝试（`extract_edit_hunks`，`:319-379`）：① 从 `raw_output` 反序列化 `ToolOutput::SearchReplace(SearchReplaceOutput::EditsApplied(..))` 再 `build_diff_hunks`（失败 `tracing::warn!` 后继续）；② 从 ACP `ToolCallContent::Diff` 的 `diff.meta` 反序列化 `SearchReplaceEditContextInformation`（要求 `details` 非空）；③ 退到 `old_text` / `new_text` 全文 diff，起始行号取 `meta.new_line`、缺省 1。三条都失败返回 `(vec![], 1)`——注意返回的 count 至少为 1。
- 结构化路径的三个边界：**空到空但确实插了一行**（补一个 `"\n"` 让 `TextDiff` 看得见，`:54-63`；而「空写空文件」必须产出零 diff 行，测试 `empty_file_write_produces_no_hunks`）；**行中匹配**的 `line_prefix`（把缩进补回被修改行，且只补第一条，`:64-88`）；`context_before` 末尾换行不产生幽灵空行（测试 `context_before_trailing_newline_no_phantom_line`）。此外先用 `context_before` / `context_after` 包住改动，再按两端各留 `MAX_CONTEXT` 行裁剪（`:117-134`），最后把首尾**纯空白**的 Equal 行也裁掉（`:136-155`），全 Equal 的 hunk 直接丢弃（`:120-121`）。重叠 hunk 缝合 `stitch_overlapping_hunks`（`:207-219`）遵循「宁可分开也不画错」；`stitch_hunk_pair` 的 bail 规则很细（`:247-315`）：坐标不单调、文本不一致、插入/删除不配对、覆盖范围内出现未配对的 Insert，**任一命中即返回 `None`**。
- `diff_hunks_to_patch(path, hunks)`（`:383-431`）手写 unified diff：`--- a/{path}` / `+++ b/{path}`，每个 hunk 计算 `@@ -old_start,old_count +new_start,new_count @@`，行首前缀 `' '` / `'-'` / `'+'`；注释写用途是 "suitable for `git apply` or clipboard sharing"。
- **不要与 `xai-hunk-tracker` 混用**：它是独立 crate，**不复用** `xai-grok-pager-diff` 而是自己再调一次 `similar`，目的是给 hunk 做「谁改的」归属与 LOC 统计（`HunkTrackerActor` 独占一个 tokio 任务、调用方走 `HunkTrackerHandle`、事件走 `HunkEvent`，`xai-hunk-tracker/src/lib.rs:1-5`；`CONTEXT_LINES = 3`、`DIFF_TIMEOUT = 10s`、`MAX_DIFF_FILE_SIZE = 1 MB`，`src/diff.rs:13-20`）。它**才**用到 `similar` 的 `unified_diff().context_radius(CONTEXT_LINES)`（`src/diff.rs:61-65`），并用 `HunkSource::{AgentEdit{prompt_index}, ExternalEditOnAgentFile, External}` 标注来源（`src/types.rs:73-89`）。**TUI 渲染只依赖 `xai-grok-pager-diff`**，两者的 `Hunk` 类型不要混用。

#### 3.5.5 呈现：行号栏 + 背景带，没有符号

三个留白常量（`tool/edit.rs:103-105`）：`INDENT = "  "`（行号前固定缩进）、`GUTTER_GAP = " "`（双列模式下旧/新行号之间）、`CONTENT_GAP = "  "`（行号与内容之间）。

```
   单列（默认 dual_line_numbers = false）：
     INDENT  行号(右对齐)  CONTENT_GAP  内容        ← 背景带从 CONTENT_GAP 起
     12  fn main() {
     13      let x = 1;      ← Delete：行号 = lo = 13，fg = diff_delete_fg，bg = diff_delete_bg
     13      let x = 42;     ← Insert：行号 = ln = 13，fg = diff_insert_fg，bg = diff_insert_bg
     14  }                   ← Equal： 行号 = ln = 14，fg = diff_gutter_fg，无背景

   双列（dual_line_numbers = true，配置项注释写作 “like GitHub's unified diff”）：
     INDENT  旧行号  GUTTER_GAP  新行号  CONTENT_GAP  内容
     12  12  fn main() {     ← Equal：两列都写行号，皆 diff_gutter_fg
     13          let x = 1;  ← Delete：旧列 = 13 用 diff_delete_fg，新列打等宽空格
         13      let x = 42; ← Insert：旧列打等宽空格，新列 = 13 用 diff_insert_fg
```

- **整份 diff 渲染中没有任何 `+` / `-` 行首符号**：`render_gutter` 只输出数字或空格（`tool/edit.rs:603-669`），`render_content_spans` 只输出原文 span（`:688-736`）；增删完全靠颜色与背景带区分，符号只出现在复制出去的 patch 文本里。gutter 宽度按 hunk 内容动态算：单列取 `max(lo, ln)` 的十进制位数，双列分别取两列位数（`gutter_layout`，`:568-601`）。
- 六个 diff 色槽（`xai-grok-pager-render/src/theme/groknight.rs:104-109`，值取 `:30-50`）：`diff_delete_bg = #420e14`（删除行背景带）、`diff_delete_fg = #f7768e`（删除行行号 / 无带主题下的整行 fg）、`diff_insert_bg = #063806`、`diff_insert_fg = #9ece6a`，上下文行的文本与行号都取 `COMMENT = #6c6c6c`（`diff_equal_fg` / `diff_gutter_fg`）。

- 背景**默认只覆盖内容区**：`gutter_bg = false` 时 `compute_bg_start` 返回 `layout.total`（缩进 + 行号 + 留白的宽度），行号栏保持干净（`tool/edit.rs:469-484`）；打开 `gutter_bg` 才让背景从行首开始，`indent_bg` 决定是否跳过缩进列。`DiffLineOutput` 把 `background` 与 `content_start_col` 一起交给上层（`:107-121`），最终由 `BlockLine::with_background_from(bg, start_col)` 应用（`:1349-1352`）。
- **无带主题走另一条路**：`diff_uses_line_fg()` 判定 `diff_delete_bg` 与 `diff_insert_bg` 都是 `Reset`（`xai-grok-pager-render/src/theme/mod.rs:342-345`）。此时 Terminal default 主题（`theme/terminal_default.rs:78-83`）的变更行**整行用红/绿前景色，并且跳过语法高亮**（`tool/edit.rs:697-704`）。这是一处刻意的取舍：当颜色成为唯一区分手段时，语法高亮让位——若自己实现 diff，需要先决定相同冲突下的优先级。空白内容行会被 `painted()` 变成一个空格，保证色带可见且该行可被选中（`:671-675`）。

#### 3.5.6 hunk 分隔、软换行、折叠与复制

- hunk 之间不画整段上下文，而是一行分隔提示（`tool/edit.rs:161-186`）：`… 9 unchanged lines`（单数为 `… 1 unchanged line`）。`hunk_separator` 默认 `"…"`，可配 `"───"`（线）、`"⋯"`（中线省略号）、`""`（不画）；格式由 `DiffRenderConfig` 承载并在 `appearance/config.rs:389-391` 暴露给 `pager.toml`。行数由 `hunk_gap_lines(prev, next)` 计算：前一个 hunk 最后一个非 Delete 行的 `ln` 与后一个 hunk 第一个非 Delete 行的 `ln` 之差减一（`:237-244`）；**算不出来时（例如两次调用各自针对不同文件快照）只画孤立分隔符，不写行数**（`:164-172` 的 `None => config.hunk_separator.clone()`）。分隔行 `is_separator = true`，不可选中且会打断选区连续性（`:119-120`）。
- 超长行没有硬截断，只有**按空白分词的软换行**（`wrap_text`，`tool/edit.rs:486-517`），`content_width = width - layout.total`。换行后要把彩色 span 按词边界重新投影（`project_styles_onto_wrap_segments`，`:519-554`）：用单调游标只在本就存在的 span 边界或换行边界切分，拼回结果必须与换行段完全一致，否则返回 `None`，退化为「每段一个纯色 span」（`:422-445`）。续行的 gutter 用等宽空格填充，不重复行号（`:446-465`）。
- 三种显示模式（`DisplayMode`）在编辑块上的分支（`tool/edit.rs:1228-1367`）：`Collapsed` **只渲染头部一行**；`Truncated` / `Expanded` 是头部（可换行、带悬挂缩进 `wrap_edit_header`，`:1072-1212`）+ 一空行 + diff 正文；之后统一施加 `ctx.max_lines` 预算（`lines.truncate(max)`，`:1357-1360`），错误信息以 muted 行插在头部与 diff 之间（`:1311-1320`）。折叠态头部摘要形如 `Edit src/main.rs +3/-2`，由三个 span 组成（` +N` 用 `diff_insert_fg`、`/` 用中性、`-M` 用 `diff_delete_fg`，`:943-972`）；两种情况会**抑制**统计并退化成 ` (N edits)`：`summary_untrusted`（调用跨多文件，比如 `apply_patch` 每个文件一个 Diff、只有第一个成为 hunks，`:759-762` 注释）或 `show_summary` 关闭——后者由 `effective_line_summary(...)` 决定：`pager.toml` 显式值优先，未设置则跟随 shell 的 `collapsed_edit_blocks` 标志（`appearance/config.rs:416-425`）。
- 复制与导出：编辑块整体复制 = unified patch（`copy_text()` → `diff_hunks_to_patch`，`:880-883`）；选区复制**排除 gutter**（`Selectable::Spans(gc..total)`，`:1336-1348`），每行的 `content_text` 单独保存，保证软换行的续行也能拼回原文。全屏 block viewer（`crates/codegen/xai-grok-pager/src/views/block_viewer/mod.rs:884-931`）复用同一个 `render_diff_lines`，因此高亮阶段、配色、分隔行完全一致；差别是固定 `width = 500` 并 `DiffRenderConfig::default()`，所以**假设不换行、渲染行与 `DiffLine` 一一对应**（注释明写这个假设，`:906-908`），并额外维护与渲染行平行的 `diff_meta: Vec<Option<DiffLineMeta>>`（分隔行为 `None`），据此 `patch_from_range(path, range)` 能**按屏幕选区**生成 unified patch（`:1231-1289`）。**未找到证据**：任何形如「已复制」「按 X 复制」的 UI 提示文案；代码块也没有复制按钮或快捷键提示。
- **没有并排（side-by-side）模式**：全量检索 `side_by_side` / `sidebyside` / `side-by-side`（`*.rs` + `*.md`，整个 `crates/codegen/`）只命中三处无关代码（`scrollback/blocks/context_info.rs:248` 注释、welcome 框布局 `views/welcome/hero_box.rs:13` 与 `views/welcome/mod.rs:317`）。`DiffRenderConfig` 唯一的「双列」是 `dual_line_numbers`——**双列行号，不是双列内容**（`tool/edit.rs:85-87`）。

#### 3.5.7 markdown 代码块与 Read 块（对照）

- assistant 消息里的围栏代码块只有一层背景色：`code_background` 一个样式字段（`xai-grok-markdown/src/style.rs:148`），在 pager 侧接到 `bg(theme.md_code_bg)`（`xai-grok-pager-render/src/theme/md_style.rs:145`）。GrokNight 的 `md_code_bg = rgb(28,28,28)`（`theme/groknight.rs:133`），Terminal default 是 `Reset`（`theme/terminal_default.rs:107`），即**没有**背景。**没有语言标签**：`CodeBlockSpan.info` 携带 fence info 原样字符串（`xai-grok-markdown/src/output.rs:26-30`），但 pager 里对 `code_blocks` 的唯一消费者是 mermaid 检测（`scrollback/blocks/mermaid_content.rs:96-97`），没有任何地方把 `info` 画成标签；也**没有边框、行号或折叠控件**。围栏行本身在 pretty 模式下被隐藏，且开围栏前补一个空行避免相邻块粘连（`xai-grok-markdown/src/render.rs:652-672`）；空的未收尾围栏（流式尾部）也会保住背景（同文件 `:1076` 注释）。` ```mermaid ` 是特例：不画代码、改画图（`render.rs:1164` 注释 + `mermaid_content.rs`），走独立的 `mermaid_worker`。
- `tool/read.rs` 是「带行号的代码展示」最直接的参考：行构成 = `{行号右对齐}{两个空格}` + `highlight_line(...)` 的 span，gutter 样式为 `theme.dim()`、内容 fallback 为 `theme.primary()`；行号宽度按 `base_line + raw_lines.len() - 1` 的位数算（`base_line` 来自读取范围起始，`:260-264`）；整个面板加 `theme.bg_dark` 背景（`:300-331`）。`Truncated` 模式折叠为**首 5 行 + 一行 `…` + 末 3 行**（`FIRST_LINES = 5` / `LAST_LINES = 3`，`:16-17`；折叠逻辑 `:294-315`），阈值是 `total > first + last`——这是本仓库里唯一对**首尾取样**（而非取中间）的上下文折叠。

> 交叉引用：本小节与 §3.4 在「围栏行在 pretty 模式下隐藏、开栏前补空行」上有内容重叠，两节结论一致。

#### 3.5.8 边界与截断（汇总）

| 场景 | 行为 | 证据 |
| --- | --- | --- |
| 编辑后文件 > 2 MiB 或 > 50000 行 | 不做全文件高亮，保持 hunk-only | `tool/edit.rs:44-49,263-274` |
| 文件读不到 / 非 UTF-8 | worker 记 debug 日志并回 `Failed`，界面保持 hunk-only | `app/edit_highlight_worker.rs:137-153,174-188` |
| 整文件 diff 计算慢 | `similar` 500 ms 超时 → 更粗但仍正确的 diff | `xai-grok-pager-diff/src/lib.rs:22-25,169-172` |
| diff 过大 | **未找到总行数 / 总字节的显式上限**；靠 hunk 粒度、`MAX_CONTEXT` 裁剪、`max_lines` 预算与后台 caps 共同约束 | `xai-grok-pager-diff/src/lib.rs:117-155`；`tool/edit.rs:1357-1360` |
| 二进制内容 | **未找到专门的二进制检测或占位渲染**；读工具对非文本走 `ReadMediaKind::{Image, Pdf}`（由图片 / PDF 路径渲染），文本 diff 路径没有二进制分支 | `tool/read.rs:20-27` |
| hunk-tracker 侧 | 单文件 > 1 MB 跳过；diff 超 10 s 返回 `None` | `xai-hunk-tracker/src/diff.rs:16-20,32-41,50-57` |

### 3.6 动效、加载态与流式呈现

#### 本节要点

- 不是「一个动画引擎」：**三条独立时钟**（动画 tick `1000 / fps`、最小绘制间隔 `min_draw_interval`、滚动节奏 `scroll_cadence`）加一个**有序的 `TickDemand` 需求门控**；空闲时事件循环完全不 arm tick，即**零唤醒**。
- **真正逐帧的动画只有四类**：spinner 帧序列、运行中块的 accent 幅度波形、脉冲式亮度（等待菱形 / 欢迎页 logo）、dashboard 与状态栏的换帧。其余全部是「定时切换或顺序出现」。
- spinner 统一走 `tick / DIVISOR` 降帧，事实标准 `SPINNER_DIVISOR = 4`（30 fps 下约 133 ms/帧、约 7.5 fps），不是每 tick 换帧。
- 流式文本**没有逐 token 重绘，也没有打字机**：单批最多 drain 32 条 ACP 消息，绘制受 16 ms 节流 + 在途水位闸门约束，且只有视口内的 running 条目才要求重绘——屏幕上表现为「按帧吞吐的整段增长」。
- 等待态的语言是「一个 spinner + 精确命名的原因 + 颜色语义」，7 种 `WaitingReason` 各有文案；短等待用 300 ms 延迟门隐藏，杜绝闪烁。
- **代码里不存在 reduce-motion 概念**，也没有「关闭动画」开关；降级靠配置 fps、按显示器刷新率自动定档、离屏不驱动、minimal 模式收紧与老终端字形替换五条路径。

本节路径省略 `reference/grok-build/` 前缀。

#### 3.6.1 调度模型：一个 tick 计数器 + 五层「帧预算」

| 时钟 | 默认值 | 用途 | 证据 |
| --- | --- | --- | --- |
| 动画 tick | `1000 / animation.fps`，fps 默认 30（clamp 1..60） | spinner 换帧、波形相位、提示条 TTL | `crates/codegen/xai-grok-pager-render/src/appearance/config.rs:335-362`、`:1282` |
| 最小绘制间隔 `min_draw_interval` | 16 ms（~60 Hz）；探测到高刷屏时降到 8 ms | 单帧绘制节流 | `crates/codegen/xai-grok-shell/src/util/config/resolve/display_refresh.rs:11-12`、`:19-22`、`:57` |
| 滚动时钟 `scroll_cadence` | 默认同为 16 ms | 滚轮 / 触控板残余行冲刷 | `crates/codegen/xai-grok-pager/src/app/event_loop.rs:1454-1461`、`:1834-1836`、`:2394-2403` |

```rust
// src/app/event_loop.rs:3052-3067
let interval = match app.tick_demand() {
    TickDemand::None => return,                              // 完全不 arm，事件循环 park
    TickDemand::Fast => interval.min(tick_interval_ceiling()), // /gboom 33ms、minimal 16ms
    TickDemand::Slow => interval.max(SLOW_TICK_INTERVAL),     // 83ms ≈ 12fps
};
```

- `TickDemand` 是**有序枚举** `None < Slow < Fast`（`src/app/app_view.rs:244-257`）；`SLOW_TICK_INTERVAL = 83ms` 与欢迎页 logo 的 `SHIMMER_FPS = 12` 对齐；`Fast` 由约 40 个条件触发（运行中 agent、滚动回放、模态 spinner、待发取消、语音录音……`src/app/app_view.rs:5636-5784`）。
- 「帧预算」没有单一常量，而是五层机制叠加：① **需求门控**（`TickDemand::None` 直接不排 tick，`event_loop.rs:3053-3056`）；② **绘制节流** `Presenter::request_throttled(now, min_draw_interval)`，不够间隔就把绘制排到 `last_draw_at + min_draw_interval`（`event_loop.rs:606-615`）；③ **在途闸门** `in_flight_target` 记录已排队 escape 序列的序号水位，上一帧未写完就不再画（`event_loop.rs:498-515`、`:579-600`）；④ **视口作用域**，只有视口内的 running 条目才要求重绘；⑤ **除法降帧**，所有 spinner 都是 `tick / DIVISOR`。

#### 3.6.2 真的逐帧：四类动画

- **① spinner 家族统一 `tick / 4`。** 全代码库的事实标准是 `SPINNER_DIVISOR = 4`，注释明确写着「降低了速度」（`src/views/turn_status.rs:29-31`：*At ~30fps, 4 ticks is ~133ms per frame, about 7.5 spinner fps*）。使用点包括 turn status 行（`turn_status.rs:367-377`）、MCP chip（`views/agent_status.rs:22`、`:343-355`）、dock 活动行（`views/dock/mod.rs:14`、`:404-406`）、`/btw` overlay（`views/btw_overlay.rs:151-152`、`:356-359`）、extensions 模态（`views/extensions_modal.rs:2760`）、tasks pane（`views/tasks_pane.rs:32`）、dashboard 行（`views/dashboard/animation.rs:5`）；goal chip 是硬编码 `/ 4`、用 dot 8 帧（`views/agent_status.rs:313-319`）。帧集定义在 `crates/codegen/xai-grok-pager-render/src/glyphs.rs:148-169`（braille 8 帧 U+280B…U+2827 与 dot spinner），**每帧都保证 1 列宽，避免布局跳动**。
- **② 幅度调制式的两处脉冲。** 监控 / 后台工作脉冲 `○ ◎ ◉ ◎`：`MONITOR_PULSE_DIVISOR = 8`，约 3.75 fps、约 1.07 s 一循环，理由是「空闲的 still-running 提示应该安静地呼吸，而不是读成活跃 turn 的 spinner」（`src/views/turn_status.rs:33-36`、`:267-275`）。等待用户操作的菱形脉冲不用帧表，而是**按 tick 计算颜色亮度**，在 `bg_base` 与 accent 之间 blend `0.3 + brightness * 0.7`，速度常量 `USER_WAITING_PULSE_SPEED = 0.08`，并有专门测试锁死该值（`turn_status.rs:38-47`、`:1575-1578`）；周期按注释为 `π / (speed · fps)`（`xai-grok-pager-render/src/theme/tokyonight.rs:358-363`）。**③ 运行中块的 accent 竖条波形**（最典型的逐帧动画）是逐行相位偏移的 `sin²` 亮度波，`WAVE_SPEED = 0.15` rad/tick、「约 40 tick 走完一个块」；`wave_rows` 默认 32 行一个波周期，是用户可配项（`src/scrollback/wrappers/entry_renderer.rs:19-21`、`:721-732`），运行中块的 bullet 用同一波形（`:857-867`）。
- **④ 换帧与墙钟驱动的两处。** 标题栏 spinner `TITLE_SPINNER_DIVISOR = 8` ≈264 ms/帧，理由写在注释里——终端（尤其 Ghostty）会对标签标题做 debounce，每 tick（~33 ms）写一次 OSC 0 会超过标签栏的渲染能力（`src/notifications/title.rs:12-15`、`:92-94`）；`⚠ Action Required` 闪烁用 `ACTION_REQUIRED_BLINK_DIVISOR = 15`，30 fps 下约 500 ms 亮 / 500 ms 灭 = 1 s 周期，**窗口聚焦时不闪**、改为静态前缀以免打字时干扰（`title.rs:17-20`、`:184-190`）。**刻意脱离 tick 的两处**：欢迎页 logo 用 `SHIMMER_FPS = 12` 墙钟相位算帧号，一次对角光泽扫过 `CYCLE = 4.0s`（其中 `SWEEP_FRAC = 0.32` 用于扫动，其余停在屏外），底下叠 `PULSE = 0.06`、`PULSE_SECS = 5.0` 的呼吸（`src/views/welcome/logo.rs:106`、`:109-140`）；语音录音红点固定 0.7 s 墙钟正弦「呼吸」，注释明确「不随动画 tick，也不与流式文本重绘同步」（`src/app/agent_view/mod.rs:216-227`）。dashboard 不按 tick 盲翻帧，而是由渲染阶段回填 `PaintedAnimations`，`tick()` 只在真正的帧边界返回 `true`（`SPINNER_DIVISOR = 4`、`NEEDS_INPUT_BLINK_DIVISOR = 10`）；文件首行注释即 *A projected row that was not painted does not count.*（`src/views/dashboard/animation.rs:1-64`）。
- **另一条独立通道：把加载态交给终端外壳。** 仅对 Ghostty、WezTerm、iTerm2 ≥ 3.6 发送 `OSC 9;4` 不确定进度（`src/notifications/progress.rs:10-21`），tmux 下自动包 DCS passthrough（`:34-38`）；因 Ghostty 约 15 s 无消息会重置，按 `PROGRESS_KEEPALIVE = 5s` 重发（`src/notifications/mod.rs:14-16`、`:151-166`）。它不是帧动画。

状态行布局（活跃期几乎全部视觉层次集中在这一行）：

```
⠧ Run cargo test 0.2s              1m20s ⇣12k [↓] [stop]
│  │               │                │      │    │    └ 取消按钮（hover 变红）
│  │               │                │      │    └ 后台按钮（仅运行中的 execute，hover 展开）
│  │               │                │      └ turn 计时 + token（右对齐，灰）
│  │               └ 阶段计时（灰，永不截断；Ask 工具时隐藏）
│  └ 活动标签（按类型着色，可截断）
└ spinner（braille，tick/4）；工具阻塞在权限/提问时换成脉冲 ◆
```

（布局与顺序来自模块头注释与渲染顺序：`src/views/turn_status.rs:3-13`、`:399-417`、`:520-570`；「Ask 工具隐藏阶段计时」在 `:379-394`。）

#### 3.6.3 明确不是动画的部分（避免夸大）

- **进度条与滚动条都是静态的**：`views/progress_bar.rs` 提供 1/8 单元精度的 `▏▎▍▌▋▊▉█` 渲染（`:1-54`、`:59-91`），但只有两个调用点——上下文占用条（`src/views/context_bar.rs:11`、`:178-189`）与 goal 进度条（`src/views/goal_detail.rs:624`），两处都不持有 tick、值变了才重画；滚动条按 follow 模式提供两档静态样式（跟随=很暗，滚上去=较亮），无 tick / 相位 / 渐变（`xai-grok-pager-render/src/render/scrollbar.rs:6-7`、`:185-208`），minimal 模式下整体隐藏（`:42-55`）。
- **完成闪光不是渐变**：`FINISH_FLASH_DURATION_MS = 400`（`src/scrollback/state/types.rs:75`）期间把已完成块的 accent 保持为「静态成功色」（`entry_renderer.rs:680-704`），过期时补一次重绘恢复原色（`src/scrollback/state/mod.rs:427-450`）。是定时切换 + 一次收尾重绘，**不是颜色插值**。
- **提示条是 TTL 倒计时，不是动画**：默认 90 tick ≈ 3 s（`src/tips/ephemeral.rs:7-10`，每 tick 递减 `:103-116`）；SSH 提示 300 tick ≈ 10 s（`src/tips/ssh_wrap.rs:22-24`）；word_select 提示 600 tick ≈ 20 s（`src/tips/word_select.rs:17-19`）。某个 tip 甚至会因为「快速 Enter 会把几秒的多秒 turn 压缩成亚秒闪烁」而暂停 TTL（`src/tips/small_screen.rs:30-32`）。
- **选择高亮与光标闪烁都不由本程序动画**：选择高亮是 150 ms 定时消失（`DEFAULT_SELECTION_HIGHLIGHT_DURATION_MS = 150`，`src/app/agent_view/mod.rs:431`），仅触发一次重绘（`src/app/agent_view/selection.rs:42-59`）；光标闪烁交给终端——`[ui].cursor_blink` 三态映射到启动时的 `CursorStylePolicy`，默认 `Inherit` **不发任何转义序列**，注释说强制设置曾被报告为光标闪烁问题（`src/app/mod.rs:1427-1445`；配置项见 `docs/user-guide/26-config-reference.md:604`）。

#### 3.6.4 流式文本：三层合帧，无打字机

结论：**没有逐 token 追加渲染，也没有逐字延迟显示的「打字机」**（全仓 grep `typewriter|teletype|char_reveal` 无匹配）。文本按批次进入 scrollback，绘制由每帧合并：

1. **接收批次**：单次事件循环最多 drain `ACP_DRAIN_BATCH_MAX = 32` 条消息，且「输入一到就截断」，注释写着「流式期间每帧会有几十条消息排队」（`src/app/event_loop.rs:1837`、`:2193-2207`）。
2. **绘制节流 + 在途闸门**：`presenter.request_throttled(now, min_draw_interval)`（默认 16 ms）；上一次绘制尚有未写完的 payload 时不绘（`event_loop.rs:2208-2223`、`:579-618`）。同一处注释点明目的：*Cap paint rate so terminal input isn't starved during heavy ACP streaming*。
3. **视口作用域**：`ScrollbackState::needs_animation()` 仅在「有 running 条目且在视口内」时为真（`src/scrollback/state/mod.rs:475-479`），`tick()` 同理（`:419-453`）；注释解释「以 ~30fps 重绘一个静止画面纯属浪费：帧 diff 会是空的」。因此一次绘制可能合并多个 token chunk，屏幕上表现为**按帧吞吐的整段增长**，而不是字符逐个显现。

增量文本的 UTF-8 边界另有专门的增量解码器缓冲（`src/acp/tracker.rs:156-158`），与渲染节流无关。滚动不走动画 fps 而走 16 ms 节奏，注释明说 *Runs on the 16ms redraw cadence, not the slower animation fps*（`src/app/event_loop.rs:2394-2403`）。

#### 3.6.5 加载态与等待态：一层行 + 一套文案

```
┌─ scrollback ─────────────────────────────────────────┐
│ ▍⠧ Run cargo test                            0.3s    │ ← 运行中块：accent 波形 + 动画 bullet
│ ▍ Subagent "explore the parser"              12s     │ ← 子代理 Started 块（同样波形）
├──────────────────────────────────────────────────────┤
│ ⠧ Run cargo test 0.2s          1m20s ⇣12k [↓] [stop] │ ← turn status 行（仅忙碌/有 watcher 时占 1 行）
└──────────────────────────────────────────────────────┘
```

- 该行 `should_show()` 为假时**高度为 0，完全不占位**（`src/views/turn_status.rs:13`、`:723-736`）。活动语义由 `compute_activity` 决定颜色与文案：`Thinking…` / `Responding…` / `Verifying…` / `Compacting…` / 工具（绿色 `Run …`，有 description 时转为中性）/ 重试（warning 色）/ `Cancelling…`（红）（`turn_status.rs:580-698`）。
- **等待原因被显式命名**，而不是笼统的 `Waiting…`：`WaitingReason` 7 种 + 对应文案（`src/acp/tracker.rs:48-73`、`:116-136`）——`Waiting for response…`、`Waiting on subagent…`、`Waiting on task output…`、`Waiting on tasks…`、`Sleeping…`、`Running <event> hook…`、`Waiting for the agent to accept the prompt…`。**短等待不显示**：hook 批次未超过 `HOOK_REVEAL_DELAY = 300ms` 一律不画，避免「快 hook 闪一下」（`tracker.rs:68-75`）。工具参数写作阶段也有独立文案：`Writing subagent prompt…`、`Preparing MCP tool…`、`Searching MCP tools…`（`tracker.rs:156-191`）。
- 子代理生命周期在 scrollback 里是四态块：`Started`（运行中动画 bullet）/ `Completed` / `Failed` / `Cancelled`；阻塞式一个块变色，后台式保留 started 块另加完成块（`src/scrollback/blocks/subagent.rs:1-12`、`:252-277`）。后台工作在 turn status 上的「still running」计数按类型分列，cue 顺序按数量开头，窄屏裁尾也保住数字（`turn_status.rs:105-140`、`:1337-1352`）。
- **两处刻意不做动画**（重要设计倾向）：tasks 顶栏的运行计数用**静态菱形**——「头部整个会话都在视野里，这里放 spinner 读起来是噪音而非进度」（`views/agent_status.rs:165-172`）；阻塞在权限 / 提问时，把 spinner 换成脉冲菱形并把波形**冻结**成实色竖条，读作「暂停等你」而不是「还在加载」（`views/turn_status.rs:365-376`、`src/scrollback/wrappers/entry_renderer.rs:715-720`、`:839-856`）。

#### 3.6.6 完成 / 失败 / 取消：反馈是文本标记，不是特效

- `src/app/turn_completion.rs` 是 turn 终结的**唯一收敛点**：两条轨（fire-and-forget 的 `prompt_complete` 广播 + 可持久重放的 `TurnCompleted`）都收敛到 `finalize_turn_from_terminal`，以便中途 attach 的 viewer 也能从重放里正确结束 turn（`:1-7`、`:484-552`）。marker 选择是一张纯函数表（`:87-109`）：`EndTurn/Refusal/MaxTokens/... → TurnCompleted`；`Cancelled` 且 `send_now` → **不产生 marker**（取消并立即发送是静默的）；`RateLimit → None`；`Error` 且已有 error banner → `None`（避免重复报错）；hook 拒绝另走 `TurnBlockedByHook`（`:129-147`）。文案：`Worked for 2m5s` / 无计时钟时 `Turn completed.` / 取消是 `<cause 短语> in X.`（`src/scrollback/blocks/session_event.rs:377-400`）。driver 侧还有「丢失 RPC 对账」：收到 terminal 但 `session/prompt` 响应未到时先 arm `pending_turn_end_reconcile`，由 tick 的 reconcile 收尾（`:365-460`）；只要该操作 armed，`view_tick_demand` 就返回 `Fast`，保证对账不被饿死（`src/app/app_view.rs:5643-5649`）。
- `src/app/cancel_latency.rs` **只有 33 行、纯遥测、零 UI**：三个类型 `CancelOrigin`、`TurnEnd { Completed, Aborted }`、`CancelLatency { requested_at, scope }`（`:1-33`），文件头注释写「the settle rule behind the `CancellationCompleted` telemetry event」。打点仅「用户手势 + Root surface」时记录锚点、同一 turn 只留最早一次（`src/app/agent_view/session.rs:543-554`）；settle 时 `Completed` 产出 `latency_ms`，`Aborted`（reload/fork/session 失败强置 idle）直接丢弃（`:555-570`）。**它不参与任何视觉反馈**；取消的可见反馈全部来自状态行：`TurnCancelling` → 红色 `Cancelling…` 标签 + `[stop]` 保持可点（作为「取消没生效」的重试点）（`turn_status.rs:298-306`、`:588-591`，测试 `:984-1021`）。相邻的一处观测通道：终端不再读取 PTY 时（writer 队列有积压且零进展，超过 `WRITER_BLOCKED_WARN_AFTER`）发一次 `TermWriterBlocked` 遥测（`src/app/event_loop.rs:2151-2170`），不影响画面。

#### 3.6.7 降级与「减少动效」

**代码里不存在 reduce-motion 概念**：全仓 grep `reduce.motion|prefers-reduced|low.power` 无匹配，也没有任何「关闭动画」开关；`docs/user-guide/21-terminal-support.md` 完全不涉及动效 / 帧率 / 性能（该文件章节为颜色、剪贴板、tmux、按键、鼠标、语音、RTL 等）。实际存在的降级 / 收敛路径共五条：

1. **配置降帧**：`[animation] fps` 1–60、`wave_rows`（`docs/user-guide/05-configuration.md:686-692`、`06-theming.md:231-235`；clamp 见 `appearance/config.rs:1282-1283`）；**按显示器刷新率自动定档**：启动时探测刷新率，`ms = clamp(round(1000/hz), 8, 16)`，仅当 hz 落在 55..=240 且开关打开，可由环境变量 `GROK_MIN_DRAW_MS` / `GROK_SCROLL_CADENCE_MS` 覆盖（夹取 1..100 ms），也可用 `[ui.display_refresh].auto_cadence_enabled` 关掉退回 ~60 Hz（`display_refresh.rs:55-75`、`:300-321`；`src/app/display_refresh_startup.rs:14-37`、`:91-100`；`src/settings/defs.rs:1003-1020`）——注意这挂的是**显示器刷新率**，不是「终端慢」。
2. **离屏不驱动 + minimal 模式**：视口外的 running 条目不产生 tick 需求（`src/scrollback/state/mod.rs:460-479`），完成闪光同样只算视口内的条目（`:427-450`）；minimal 模式把 tick 上限收紧到 16 ms（`src/app/app_view.rs:5563-5571`）、滚动条整体隐藏（`render/scrollbar.rs:42-55`）。
3. **老终端字形替换（非降帧）**：legacy ConHost 用 CP437 回退字形（spinner 用 `| / - \`、monitor 脉冲用填充变化、进度条用 `░▒▓`）以免 tofu（`crates/codegen/xai-grok-pager-render/src/glyphs.rs:100-117`、`:148-175`；`src/views/progress_bar.rs:18-29`）。

另有一个相邻节流：状态行脚本（`$GROK_STATUS_LINE`）的事件驱动重算被限为 `EVENT_DEBOUNCE = 300ms`，强制 / 到期刷新降到 100 ms，注释说明目的是「让忙碌的 turn 不要每帧都跑一次脚本」（`src/app/status_line.rs:17-21`）。

**未找到证据**：① 无「打字机式」逐字显现或可配打字速度；② 无 reduce-motion / 无障碍动效开关；③ 无「终端慢则自动降帧」的检测路径（只有显示器刷新率探测）；④ 无完成时的庆祝 / 闪光特效，turn 完成的视觉产物只有一条文本标记；⑤ 未找到独立的「帧预算」常量或预算超支丢弃逻辑——预算效果由 §3.6.1 的五层机制间接达成。

### 3.7 图形化内容：Mermaid、内联图片与视频

#### 本节要点

- Grok 的「图形」不是一种技术，而是**三条互不依赖的路径**：Mermaid 文本制表符图（全终端可用）、post-flush 逃逸序列贴的像素层（Kitty / iTerm2）、以及按需 PNG（子进程 + 磁盘缓存，交给 OS 看图器）。
- **渲染出的 Mermaid PNG 从不内联显示**；内联的文本图则不参与高保真渲染——两条实现互不相干，只靠注释纪律同步断词规则。
- 终端图形协议**只有 Kitty 与 iTerm2 两族**；`sixel` 全仓库零命中，`▀▄█` 只用于进度条 / 滚动条 / 光晕等文本语义，**没有栅格图转半块字符的渲染器**，因此降级链止于「文本 affordance」。
- scrollback 媒体是「**单元格占位 + 帧后贴像素**」：占位可选中可滚动可复制，像素不参与单元格绘制；滚动用 `top_crop_rows` 转成 Kitty 源裁剪，而不是重新解码图片。
- 视频是把**全部帧预抽到内存**的帧序列：10 fps、宽 ≤640、PNG（Kitty）/ JPEG（iTerm2），帧集可达几十至几百 MB，释放一律排到帧后。
- 复刻时最容易踩的三处：两套 Mermaid 渲染器 = 两套 bug 面；minimal 模式与图形能力是**两把独立的锁**；iTerm2 的 `preserveAspectRatio=0` 与 Kitty 的 `C=1` 直接决定观感。

本节路径省略 `reference/grok-build/` 前缀。

#### 3.7.1 三条路径总览

```
┌ 第 1 层：文本图形（ratatui Buffer，全终端可用，不依赖任何协议）───────┐
│  ```mermaid  → xai-grok-markdown/src/mermaid.rs  自建 parser + 布局 → │
│                 Unicode 制表符图（┌─┐│╎┃），随 markdown 一起绘制      │
│  图片/视频    → 预留 N 行空白 + 文件路径行 + [Open]/[Play]/[Copy] 行 │
└───────────────────────────┬───────────────────────────────────────────┘
                            │ 只在支持图形协议的终端上叠加第 2 层
┌───────────────────────────▼───────────────────────────────────────────┐
│ 第 2 层：像素图形（post-flush 逃逸序列，绕过 ratatui 单元格）          │
│  Kitty APC (\x1b_G…) 或 iTerm2 OSC 1337；写 stderr，在 cell flush 之后 │
│  位置用「单元格坐标 + 源像素裁剪」描述，不参与单元格绘制/选中/复制     │
└───────────────────────────┬───────────────────────────────────────────┘
                            │ 需要真实像素时另开进程
┌───────────────────────────▼───────────────────────────────────────────┐
│ 第 3 层：Mermaid PNG（子进程 + 磁盘缓存）                              │
│  xai-grok-pager __mermaid-render → mermaid-to-svg(dagre) → SVG →      │
│  resvg/tiny-skia → PNG 落盘 → 交给 OS 看图器或复制路径                 │
└───────────────────────────────────────────────────────────────────────┘
```

关键取向：**渲染出的 PNG 从不内联显示**（`scrollback/blocks/mermaid_content.rs:6-8`：*The rendered PNG is never drawn inline; it is reached only through the affordance row's actions.*），而内联的文本图不参与高保真渲染；两条实现互不相干，**注释明确要求人工保持同步**——`LABEL_BREAK_CHARS` 与 `third_party/mermaid-to-svg/src/text_wrap.rs` 的 `TOKEN_BREAK_CHARS` 一致（`xai-grok-markdown/src/mermaid.rs:35-38`）。

#### 3.7.2 内联文本图：零依赖、即时、永远可用

- 识别只认 `mermaid` fence 的**第一个 token**：`is_mermaid_info()` 取 info string 第一个空白分隔 token、大小写不敏感比较 `"mermaid"`（`mermaid_content.rs:87-92`），因此 ` ```Mermaid `、` ```mermaid theme=base ` 都算；数据来自 markdown 渲染器暴露的 `CodeBlockSpan`（`:94-123`）。流式解析器**只在闭合 fence 上**替换正文（`xai-grok-markdown/src/parse.rs:636-643`：`range.end < self.text.len()`，即未闭合的 fence 在流式过程中继续显示源码），且仅在 `pretty` 模式生效（`render.rs:469`），raw 模式保留源码。检测结果缓存在 message 级别：`AgentBlock` 在构造 / 结束时一次算出 `MermaidContent`，**不是每个流式 chunk 都算**（`scrollback/blocks/agent.rs:17-18`、`:74`）。
- `crates/codegen/xai-grok-markdown/src/mermaid.rs`（5699 行）是一个**自包含的终端 Mermaid 渲染器**，不依赖 `xai-grok-mermaid`、不依赖 dagre、不需要 PNG；支持 `graph`/`flowchart`、`stateDiagram`、`classDiagram`、`erDiagram`、`sequenceDiagram`（`:67-82` 的 `or_else` 链）。布局是自建的 Sugiyama 风格：`compute_ranks` → `order_ranks`（barycenter 交替上下扫，最多 8 轮，取交叉数最少者，`:2672-2753`）→ `assign_positions`（10 轮松弛，`:2806-2868`）→ `place_td` / `place_lr`。画布用一个位掩码画布 `Canvas`（上/下/左/右 4 个 bit），最后 `finalize_mask()` 统一选制表符（`mask_char`，`:1730-1745`）；虚线走 `╌╎`、粗线走 `┃━┏┓┗┛`（`:1747-1770`）。`BT`/`RL` 的做法是**先按 TD/LR 排好再整体翻转画布**，翻转后把文本 run 反向回来保证可读（`flip_vertical`/`flip_horizontal`，`:1621-1669`）。
- 硬上限（防不可信输入膨胀，`:28-45`）：`MAX_NODES=128`、`MAX_EDGES=512`、`MAX_GROUPS=24`、`MAX_GROUP_DEPTH=6`、`MAX_CANVAS_CELLS=1<<21`、标签 `WRAP_WIDTH=24` / `MAX_LINES=4` / `MAX_LABEL=28`。画不下或解析不了时一律退化为**带标题的框**：标题 ` mermaid: <首个 token> `，正文是原文换行截断（`fallback()`，`:3925-3997`）；若原因是「太宽」，额外附一行斜体提示 `This diagram is too wide to display here — open the image to view it in full.`（`:3922-3923`、`:3983-3985`）。
- 内联文本图下面会插入一行**不可选中的 affordance 行**（`apply_affordance_rows`，`mermaid_content.rs:373-397`）：`◇ mermaid   [Open Image]   [Copy Image Path]   [Copy Source]`，右侧在渲染中显示 `rendering diagram…`；它不参与复制、也不移动 pre-wrap 行号。按钮间距 `AFFORDANCE_GAP = 3`（`:31-38`），行布局只有 `affordance_row()` 一个来源（`:310-325`），绘制与点击命中读同一份数据（`app/agent_view/media.rs:229-313`），因此不会漂移。设置项 `ui.render_mermaid = auto|on|off`（默认 `auto`，`docs/user-guide/26-config-reference.md:621`；读取路径 `appearance/cache.rs:593-607`）：`off` → 只有内联图、无 affordance 行；`auto`/`on` → 有（`mermaid_content.rs:239-246`）。**minimal（静态提交）模式强制 `SourceOnly`**，因为绘制循环不会运行、按钮会是死行（`:249-257`）。Mermaid 也是唯一刻意不看图形能力的部分——`mermaid_content.rs:236-238` 注释：*Terminal graphics capability is intentionally not consulted either: the affordance row is text plus mouse hit-rects, so it works everywhere.*

> 交叉引用：本小节与 §3.4 在「围栏闭合判定与 pretty 模式下的替换时机」上有内容重叠，两节结论一致。

#### 3.7.3 按需 PNG：点击才渲染，且必须出进程

时序（生产路径）：

```
用户点 [Open Image]
   │
   ├─ 磁盘命中（read_cached_png：非符号链接 + 能解码）→ 直接 open_media_natively / 复制路径，不渲染
   │
   └─ 未命中 → ensure_mermaid_runtime() 懒启动 worker 线程（"mermaid-render"）
                → MermaidJob{key, source, out_path, theme_dark, quality=Open}
                → 子进程: current_exe() __mermaid-render --out … --theme … --quality open --width … --deadline-ms …
                │            stdin = 图表源码（≤64 KiB+1 字节则拒绝）
                │            子进程内：install_child_backstops() → read_stdin_capped() →
                │                      render_checked(default_engine()) → write_png_atomic()
                → 墙钟超时（3s / 20s）→ killpg(SIGKILL) 清理进程组
                → map_run_result：零退出 + PNG 可解码才算成功
                → mpsc 回主线程 → poll_mermaid_results → complete_mermaid_action
```

- 入口分三档：先查内存 pending → 再查磁盘缓存命中 → 未命中才派发 worker 任务（`request_mermaid_render`，`src/app/mermaid_worker.rs:955-1031`）；**两种动作都强制走 Open 画质档**（`:958-960`）。worker **只有一个线程**（不是线程池），并**按 cache key 合并同一批请求**（`drain_coalesced` + `IndexMap` FIFO，`:208-261`）。
- 隐藏子命令 `__mermaid-render` 在 `main` **最早期**被拦截，早于任何 TUI / agent / tokio 初始化（`mermaid_worker.rs:365-382` 与模块文档 `:19-24`）；参数 `--out --theme --quality --width --deadline-ms`（`:291-320`），`--max-height` 作为 legacy 被接受并忽略（`:505-508`）。Linux 上额外设 `_RJEM_MALLOC_CONF`/`MALLOC_CONF=narenas:1`，避免多核机器上分配器预留逼近 `RLIMIT_AS` 导致每次渲染都 abort（`:312-319`）。
- 尺寸与画质两档（`mermaid_worker.rs:66-89`、`:579-602`；`xai-grok-mermaid/src/lib.rs:132-161`）：**`Terminal` 档**（预留，当前点击路径不用）宽度 = 内容列数 × `APPROX_CELL_W_PX(8)` × `RENDER_SCALE(2)`、clamp 到 320–1600，高度上限 2400 px，超时 3 s；**`Open` 档**（`[Open Image]` / `[Copy Image Path]` 实际使用）走 `RenderParams::for_os_viewer`（`scale=2.0` + 最小宽度 2560），高度上限 8192 px，超时 20 s。表面色只有两个：`LIGHT_SURFACE = #FAFAFA`、`DARK_SURFACE = #18181B`（`xai-grok-mermaid/src/lib.rs:64-67`）；只有 `GrokDay` 判为亮色，`Terminal` 主题也按暗色处理（`mermaid_content.rs:126-129`）。
- **为什么必须出进程**：shipped profile 用 `panic = "abort"`，`render_checked` 里的 `catch_unwind` 在那里是空操作；同步渲染也无法被超时打断。模块文档把「真实崩溃隔离在进程之外」写成了设计结论（`xai-grok-mermaid/src/lib.rs:13-21`；`mermaid_worker.rs:14-34`）。
- **为什么 vendored 一整套 dagre / graphlib**（理由出自仓库自己的注释，不是推测）：`third_party/README.md:8-14` 与 `third_party/mermaid-to-svg/Cargo.toml:7-12` 写的是「渲染的是**不可信模型输出**」，vendoring 换来完整审计面 + 钉死版本 + 免受 crates.io yank。依赖链为 `xai-grok-mermaid → mermaid-to-svg (MIT) → dagre_rust (Apache-2.0) → graphlib_rust → ordered_hashmap`，上游是 `warpdotdev/mermaid-to-svg` 与 `r3alst/dagre-rust`（`third_party/README.md:17-24`、`:27-37`）。**算法等效 mermaid.js**：mermaid-to-svg 把布局交给 dagre.js 的 Rust 移植，注释写它「修复了上一个引擎在环形 flowchart 回边上的错误路由」；`xai-grok-mermaid` 的测试用同一个环形登录流回归——8 条边必须各自带一个 `marker-end="url(#arrowhead)"`（`xai-grok-mermaid/src/pure.rs:120-154`）。
- 本地补丁有清单，逐条记在 vendoring notes 里：不 vendor CLI bin 与 snapshot 测试；`mermaid_port::is_enabled()` 无条件返回 `false`（该实验分支读环境变量、对不可信输入不确定，且会错路由环形回边）；整段重写 sequence 解析器；CJK 宽度用 `unicode-width` 计算；open edge-label 语法；单一超长 token 不再被硬切（`third_party/mermaid-to-svg/Cargo.toml:14-96`）。`dagre_rust` 的补丁是把上游 `static mut UNIQUE_STARTER` 换成 `AtomicUsize`，消除多线程渲染的数据竞争（`third_party/dagre_rust/Cargo.toml:18-26`）。升级流程被写成 4 步 checklist：读 vendoring notes → 重放补丁 → 核对许可证 → 刷新 `NOTICE`（`third_party/README.md:39-45`）。

#### 3.7.4 终端图形协议：两族，且降级顺序固定

```rust
// pager-render/src/terminal/image.rs:144-157（简化）
pub fn protocol_for_brand(brand: TerminalName, is_windows: bool) -> GraphicsProtocol {
    if is_windows { return GraphicsProtocol::None; }   // ConPTY 会吞掉 APC/OSC
    match brand {
        Kitty | Ghostty | WezTerm | WarpTerminal => GraphicsProtocol::Kitty,
        Iterm2 => GraphicsProtocol::None,              // OSC 1337 无 placement/clear，预览另行放开
        _ => GraphicsProtocol::None,
    }
}
```

- `GraphicsProtocol` 只有 `Kitty` / `ITerm2` / `None` 三个值（`crates/codegen/xai-grok-pager-render/src/terminal/image.rs:29-38`）；探测是**纯环境推断 + 一次缓存**（进程级 `OnceLock`，`:46`、`:71-83`），不做运行时特性协商；品牌来自 `TerminalName`（20 个变体，`terminal/mod.rs:58-107`）；tmux 直接跳过（`terminal/mod.rs:374-379`）。
- **scrollback 内联叠加层比协议支持更窄**：只在 `Kitty` 协议**且**品牌 ∈ {Kitty, Ghostty, WezTerm} 时启用——注释说明 Warp 虽然接受部分逃逸，但不支持 placement / scrollback 模型，会留下残影（`terminal/image.rs:85-120`）。**iTerm2 只在 prompt 预览这一处放开**（因此上面的 `Iterm2 => None` 与这条并不矛盾，是分层放开）：条件是 `TERM_FEATURES` 含 `F`，或该变量不存在但处于 SSH（因为该变量不过 SSH，缺失不等于拒绝）；理由注释是「预览框永不落在最后一行，关闭时会重绘所有占位单元格，因此不需要 clear 逃逸」（`:159-198`）。minimal 模式用原子开关**强制关闭**内联叠加层，原因是它只提交静态文本、不跑绘制循环（`:48-62`；启动时由 `src/app/mod.rs:391` 按 minimal 传入）。

降级顺序与判定表：

| 场景 | Kitty | iTerm2 | None（含 tmux / Windows / 未知终端） |
| --- | --- | --- | --- |
| scrollback 内联图片 / 视频 | Kitty/Ghostty/WezTerm 才启用（`terminal/image.rs:109-120`） | ✗ 不用 | ✗ |
| prompt 悬停预览（image chip） | ✓ | `TERM_FEATURES` 含 `F` 或 SSH 时 ✓ | ✗ → 元数据文本 |
| 图片 / 视频查看器模态 | ✓ | ✓ | ✗：被 `guard_image_support()` 拦下并弹 toast |
| Mermaid PNG | 与终端能力**无关**：生成文件后交给 OS 看图器 | 同左 | 同左 |

非图形终端打开图片查看器会被 `guard_image_support()` 提前拦下并弹一句 toast：tmux 下是 `Inline images disabled within tmux.`，其他终端是 `Image rendering not supported in this terminal`（`app/agent_view/notices.rs:271-281`，调用点 `app/agent_view/prompt.rs:468-477`）。

**未找到证据**：全仓库（Rust 源码、文档、`Cargo.toml`、vendored 源码）检索 `sixel` **零命中**；`braille` / `quadrant` / `chafa` / `viu` / `timg` 亦零命中，`▀ ▄ █ ▌` 的用法集中在进度条（`views/progress_bar.rs:1`、`:16-20`）、滚动条（`render/scrollbar.rs:185`）、dashboard 光晕（`views/dashboard/render.rs:1104`、`:1254`）、工具用量图标（`tool_usage.rs:64-70`）等**文本语义**场景。结论：降级链是 **Kitty → iTerm2（仅 prompt 预览）→ 文本 affordance**，不存在第 4 档字符画。

#### 3.7.5 两族协议的实现差异（决定观感的部分）

- **Kitty**：头部字段 `a=T,f=100,t=d,q=2,C=1,z=,i=1,p=1,c=,r=`——`a=T` 传输并显示、`f=100` 只收 PNG、`t=d` 直接传数据、`q=2` 静默、`C=1` 保留光标、`z` 决定层级、`i/p` 同一个 placement id 1、`c/r` 是单元格尺寸；数据 base64 后按 4096 字节切块，非末块 `m=1`（`terminal/image.rs:326-386`）。**缩放交给终端**：注释明确 *terminal scales to the cell rect (it knows HiDPI)*（`:325`）。两种原语：`transmit_kitty_image`（`a=t`，只上传不显示，`:358-361`）与 `place_kitty_image`（`a=p`，约 50 字节的纯定位，`:391-396`），带 `x,y,w,h` 的 `place_kitty_image_cropped` 负责源裁剪（`:402-416`）。层级：模态叠加用 `z=1`（文本之上），scrollback 内联媒体用 `z=-1`（文本之下、背景之上，下拉菜单能盖住它）（`:337-343`、`:542`）。删除靠 id：`clear_kitty_image(id)` → `a=d,d=i,i=<id>`（`:419-421`）。**Warp 特别处理**：placement replace 被忽略，所以固定 id 的模态图先删再传，否则会叠影（`:441-443`）。编码转换：只有 PNG 能直传，非 PNG 在 macOS 走 `sips`（保留 ICC profile，避免 JPEG→PNG 伪影），其他平台走 `image` crate 重编 PNG（`:233-315`）。
- **iTerm2**：一行 OSC 1337 内联图片，尺寸直接给单元格数并**关闭宽高比保持**（`preserveAspectRatio=0`），因为调用方已经算好矩形、再保持比例会留黑边（`terminal/image.rs:428-434`）：`ESC ] 1337 ; File=inline=1;width=<cols>cells;height=<rows>cells;preserveAspectRatio=0:<base64> BEL`。iTerm2 **没有 place-only 逃逸**，所以每次矩形变化都要重发整幅图（`app/agent_view/media.rs:200-221` 的 `emit_iterm`）；而且 OSC 1337 会推进光标，必须用 DECSC/DECRC（`\x1b7`/`\x1b8`）把光标包起来（`terminal/image.rs:464-471`）。另一个 Kitty 没有的问题：拖拽改变窗口尺寸时 iTerm2 会**连已提交的像素一起按网格缩放**，而 OSC 1337 没有 clear 原语——所以预览在 resize 后的 300 ms 静默窗口内被隐藏（并保留 100 ms 额外 tick 边距持续重绘）；Kitty 终端则是丢弃并按新矩形重放（`app/agent_view/notices.rs:13-16`、`:160-177`）。

#### 3.7.6 尺寸、占位、滚动与清理

- 单元格换算的基准是**实测 cell 宽高比**：`cell_aspect()` 用 crossterm 的 `window_size()` 算 `(窗口像素宽/列数) ÷ (窗口像素高/行数)`，只在 `0.3..=0.8` 这个「真实等宽字体」带内接受，否则回落到 `DEFAULT_CELL_ASPECT = 0.5`（典型 8×16 cell）——理由注释是协议会填满单元格矩形、假设 1:2 会把其他字体的图拉变形，且 tmux / Windows / 非 tty 的报告不可信（`terminal/image.rs:555-591`）。`fit_image_to_cells(img_w, img_h, max_cols, max_rows)` 把图像宽高比换算成「每行需要多少列」（`cols_per_row = img_aspect / cell_aspect`），先尝试按宽铺满、行数超了就改按高铺满（`:594-621`）。
- 图片 / 视频在 scrollback 里是**两层分离**：单元格层画占位（可选中、可滚动、可复制），像素层用逃逸序列贴在算好的矩形上。

```text
┌ scrollback 终端窗口 ────────────────────────────┐
│  上一段消息文本…                                 │
│ ┌ tool media 块（entry_renderer 预留 rows+3 行）┐│
│ │ ① 顶部 1 行：文件路径（点击复制路径）          ││ ← content_y_start + 1
│ │ ② 中间 rows 行：图像占位（像素由 post-flush   ││ ← screen_rect（随滚动裁剪）
│ │    逃逸贴在这块矩形上，选择/复制看不到像素）   ││
│ │ ③ 末尾 1 行：空白 spacer                      ││
│ │ ④ 最后 1 行：[Open] / [Play] / [Copy]         ││ ← content_lines - 2
│ └───────────────────────────────────────────────┘│
│  下一段消息文本…                                 │
└──────────────────────────────────────────────────┘
```

- 预留行数取**单一来源**：`inline_media_reserved_rows()` 返回 `(图像行数, 总行数=图像行数+3)`，被「预留占位」与「实际贴图」两处共用，注释强调两者不能漂移（`inline_media_ffmpeg.rs:121-134`；`src/scrollback/wrappers/entry_renderer.rs:350-364`）。图像行预算：`max_cols = content_width - 2`，`max_rows = (content_width / 2).clamp(4, 20)`。非图形终端返回 0 行，改为输出文本 `[Open]` 行（`entry_renderer.rs:351-353` → 0；`scrollback/blocks/tool/other.rs:413-427` 的 `inline_open_button()`）。视频海报在 scrollback 里就是普通图片路径；无 ffmpeg 时预留行数缩为 1–2 行横幅（安装提示），其余布局不变（`inline_media_ffmpeg.rs:126-129`、`:183-194`）。
- **滚动是「移动窗口」而不是重新解码**：每帧按虚拟坐标算出可见部分，用 `top_crop_rows` 表达「被上方裁掉多少行」，可见高度不足完整高度时转成 Kitty 源裁剪的 `y/w/h`（`src/scrollback/render.rs:650-712`；换算 `src_y = top_crop_rows * img_h / fit_rows`，`terminal/image.rs:518-543`）。可见性门槛：`full_image_h >= 1 && media_width >= 4`。Mermaid 的 affordance 行也走同一套虚拟坐标 → 屏幕矩形换算，只画**行内可见**的那一行（`scrollback/render.rs:728-745`）。
- **帧间清理与叠加层记账**：每帧收集本帧实际放置的 Kitty id 集合 `this_frame_ids`，与上一帧 `last_placed_ids` 求差——消失的 id 立即发 `clear_kitty_image` 并从 `inline_media_ids` 移除（`app/agent_view/render.rs:4173-4181`）；本帧没有任何放置时整批清理（`:4183-4206`）。打开任何模态（图片 / 视频查看器、gboom、block viewer、feedback 等）会先把 scrollback 的像素全部清掉，避免浮在模态之上（`:629-650`）；dashboard 切换时也有一次性的「陈旧图像清理」（`app/app_view.rs:4209-4235`）。非 scrollback 的叠加层（图片查看器 / prompt 预览）共用 `KITTY_PLACEMENT_ID = 1`，由 `terminal/overlay.rs` 用 `(owner_id, cols, rows, x, y)` 记账：几何完全没变就发**空串**（不重传），变了才「先删再传」（`:161-184`；测试 `:301-352`），`commit()` 只在写成功后记账（`:104-110`、`:355-375`）。

#### 3.7.7 视频：预抽帧到内存的帧序列

- **抽帧在后台线程**：`start_inline_video_playback` 起线程调 `VideoViewerState::open_from_path`，通过 mpsc 回传，UI 先给 `Loading video…` toast（`app/agent_view/media.rs:430-462`）；若 `open_from_path` 失败，tick 里弹 `Video playback requires ffmpeg`（`app/app_view.rs:5454-5465`）。
- **参数与海报帧**：`VIDEO_FPS = 10.0`（取源帧率与 10 的较小者）、`VIDEO_MAX_WIDTH = 640`（更宽才加 `scale=640:-2`）、`-q:v 5`；输出格式按协议选 **PNG（Kitty）/ JPEG（iTerm2）**，写成临时目录里 `%06d.<ext>`，读完帧就删目录（`crates/codegen/xai-grok-pager-render/src/prompt_images.rs:214-217`、`:263-330`）；帧集合被注释描述为「5–15 秒 10 fps ≈ 50–150 帧、约 1–3 秒」。scrollback 静止显示用**海报帧**：`extract_poster_frame` 先 `-ss 1` 取第 1 秒的画面试代表帧，失败再取首帧，协议为 `None` 时直接返回 `None`（`:397-437`）。
- **播放节奏**：`app_view` 每 tick 用 `last_frame_time.elapsed() >= 1/fps` 推进一帧，末帧后置 `finished`（`app/app_view.rs:5469-5482`）；`view_tick_demand()` 把「视频未播完 / 正在抽帧 / mermaid 有待办」都算作需要继续 tick 的理由（`:5636`、`:5717-5719`）。每次换视频或关闭查看器都会丢弃**整个帧集**（几十到几百 MB），通过 `memory_release::request_release_after_draw` 在**帧后**归还内存，绝不在绘制中途同步释放（`app/agent_view/media.rs:370-388`、`:592-614`；`app/agent_view/render.rs:629-650`）。

#### 3.7.8 Prompt 侧的视觉：图片 chip 与 `@` 附件

- 粘贴 / 拖入成功后计数器 +1，占位文本 `[Image #N]`（`prompt_images.rs:744-747`），并作为 textarea 的**原子元素**插入（`ElementKind(3)` = `KIND_IMAGE`，`views/prompt_widget/mod.rs:45`、`:2363-2373`）。**chip 里永远不含路径**——路径只出现在预览与 overlay（`prompt_images.rs:743-746` 注释）。视觉是**整块反色底色的 chip**：`[`、`]` 用 `theme.paste_dim` 前景 + `theme.paste_bg` 背景，中间文字用 `theme.paste_fg` + `theme.paste_bg`，与粘贴文本块同款（`views/prompt_widget/mod.rs:3761-3770`）。插入前校验：宽或高小于 `MIN_SIDE = 8` 像素直接报错拒收（`:2354-2361`）。光标 / 悬停在 chip 上时元素交互类型是 `ImagePreview`（`:2938`）。
- `@` 文件引用是另一套元素（`KIND_FILE_REF`），显示形如 `@foo/bar.rs` 或 `@foo/bar.rs:10-12`：`@` 与 `:` 用灰色、路径用 `theme.path`、行号用 `text_primary`（`views/prompt_widget/mod.rs:3620-3634`、`file_ref_display`）。交互是把元素展开成文本（`ElementInteraction::Inlined`），**没有图片预览**（`:2933-2940`）。文档口径一致：`docs/tutorial/03-attach-and-paste.md:5-23` 把「`@` 提及文件（支持行号、`@!` 取隐藏文件）」「粘贴图片（macOS `Cmd+V` / Linux `Ctrl+V` / Windows `Alt+V`）」「`!` 直接跑 shell 命令」列为三种入口。
- **悬停预览是一个 2×2 决策矩阵**（`crates/codegen/xai-grok-pager-render/src/render/image_overlay.rs:1-12` 的模块注释直接给出）：有像素 + 有路径 = 图片 + 路径 footer；有像素 + 无路径 = 仅有图片；无像素 + 有路径 = 元数据 + 路径；无像素 + 无路径 = 仅元数据。框的最小宽 28、有像素最小高 8、纯元数据最小高 6；元数据框宽 = 区域宽 × 0.75、内容 4 行 + 2 行边框（`render/image_overlay/geometry.rs:8-14`、`:89-100`）。有像素时内部尺寸按 `fit_image_to_cells` 反推框大小、图在内部再居中（`geometry.rs:61-86`），框外整块区域做 50% 变暗（`image_overlay.rs:86`），拿不到 `prepared()` 就先画居中 `Loading...`；无像素时打印 `Format:` / `Dimensions:` / `Size:` / `Path:`（或 `Preview unavailable` / `Preview pending`）（`image_overlay.rs:142-186`）。像素通过 `overlay::static_image_for_protocol` 走共享 id 记账（`:202-213`）。
- **图片 / 视频查看器模态**都是居中 90% 的圆角框（宽至少 28、高至少 8），外圈 50% 变暗；视频版在底边框上画进度条（`▶`/`⏸` + `━` 已播 / `─` 未播 + `m:ss/m:ss`）（`render/video_overlay.rs:24-49`、`:77-122`）。图片查看器的显示字节在打开时就按协议准备好（JPEG→PNG for Kitty，`prompt_images.rs:56-82`），`overlay_owner_id` 与 prompt 预览共用同一套 owner 记账。键盘：`Esc`/`q` 关闭并先清 Kitty 图（`app/agent_view/media.rs:76-100`）；视频查看器另有空格播放 / 暂停、`←/→`（或 `h/l`）快退 / 快进（`:585-615`）；关闭时同步归还内存（查看器帧集可达 50–300 MB）。
- **远程 / 包裹场景的剪贴板图片**：`wrap_clipboard_image.rs` 解决 `grok wrap`（本地终端包住远程 pager）拿不到剪贴板图片的问题——本地全 miss 且 OSC 52 sink 已激活时，向 stderr 发私有 OSC `ESC ] 999;GrokWrapClipboardImage? BEL`（`:17-28`、`:49-87`）；wrapper 用**括号粘贴帧**回注 `GROK_WRAP_IMG\n<mime>\n<base64>` 或 `GROK_WRAP_NONE`（`:30-34`、`:124-138`），其中 `GROK_WRAP_NONE` **不是** `GROK_WRAP_IMG` 的前缀，正是为了区分「明确没有图片」与「帧被截断」。预算 `MAX_WRAP_IMAGE_BYTES = 20 MiB`（注释说 4 MiB 太小会让 retina 截图静默变成 NONE，`:36-39`）；超预算先按 JPEG 质量阶梯 `[85, 70, 55, 40, 25]` 重压，仍不行再半分辨率 JPEG 60（`:144-185`）；解码前先用 base64 长度估算拒绝，避免超大分配（`:106-117`）。安全注释把这条路径定性为「等价于一次图片剪贴板读取」，并明确写「不要把这个模式泛化到不可信的多路复用器上」（`:6-12`）。

#### 3.7.9 容错、缓存与失败降级

超时与进程隔离（`panic = "abort"` 下唯一的真实防线）：

| 机制 | 值 | 位置 |
| --- | --- | --- |
| 父进程墙钟预算（Terminal / Open 档） | 3 s / 20 s | `mermaid_worker.rs:82-96` |
| 子进程自毁看门狗（退出码 2，与普通失败 exit 1 区分） | 预算 + 3 s 后 `exit(2)` | `mermaid_worker.rs:384-427`、`:389-392` |
| 超时后清理 | `killpg(SIGKILL)` 整个进程组 | `xai-grok-mermaid/src/subprocess.rs:95-141` |
| 父子同生共死 | `prctl(PR_SET_PDEATHSIG, SIGKILL)`（Linux） | `mermaid_worker.rs:458-468` |
| 地址空间上限 | `RLIMIT_AS = 2 GiB`（Linux，只降不升） | `mermaid_worker.rs:401-456` |
| 源码大小上限 | 64 KiB（父进程先拒一次，子进程再拒一次） | `xai-grok-mermaid/src/engine.rs:36-50` |
| 光栅面积 / 单轴上限 | 32 MP / 16 384 px | `xai-grok-mermaid/src/raster.rs:22-27` |

缓存与记账（一条值得抄的不变式：**先拿到字节，再分配 Kitty id**——否则「读文件失败但已记 id」会让下一帧只发 `place` 不发 `transmit`，永久留一块空白，`app/agent_view/media.rs:135-138`、`:187-196`）：

| 缓存 | 键 / 容量 | 位置 |
| --- | --- | --- |
| Mermaid 磁盘 PNG | `blake3(source)` + 主题 + 宽度桶 + 画质档 + `RENDER_REVISION=3` | `mermaid_content.rs:134-195` |
| 宽度分桶 | 终端档每 8 列一桶；Open 档用哨兵 `u16::MAX`（不与终端档共用文件） | `mermaid_content.rs:39-53`、`:170-186` |
| 会话磁盘上限 | 200 MiB，超限按 mtime 由旧到新删；每 8 次新写入扫一次 | `mermaid_worker.rs:62-64`、`:98-101`、`:725-774` |
| 缓存命中的判定 | 非符号链接 + 能解码成图（半截 / 损坏文件视为 miss 重渲染） | `mermaid_worker.rs:263-277` |
| 内联媒体编码字节 | 64 MiB 上限，超限条目只作临时使用不入缓存（随机淘汰） | `app/agent_view/media.rs:46-70`、`:176-186` |
| 失败负缓存 | 按 `(len, mtime, ino, ctime)` 指纹记录读取失败；文件一变就重试 | `app/agent_view/media.rs:13-44`、`:149-175` |
| Kitty 上传记账 | id 一旦分配就保留在 `inline_media_ids`，即使 CPU 侧字节被淘汰也不必重传 | `app/agent_view/media.rs:176-178` |
| 预览结果 | `PromptImagePreview`（`OnceLock` + `identity()`）供同一张图跨帧复用 | `pager-render/src/prompt_images.rs:613-630` |

失败降级表：

| 失败 | 用户看到 | 证据 |
| --- | --- | --- |
| Mermaid 解析 / 布局 / 光栅化失败、超时、子进程崩溃 | toast `Could not render diagram`，内联文本图仍在 | `mermaid_worker.rs:341-360`、`:1048-1066` |
| 源码超 64 KiB | 父进程直接判失败，不启动子进程 | `mermaid_worker.rs:662-672` |
| 还没有 session 目录可缓存 | toast `Diagram not ready yet` | `mermaid_worker.rs:961-971` |
| 渲染进行中 | toast `Rendering diagram…` + 行尾 `rendering diagram…` | `mermaid_worker.rs:89`、`:1018`；`mermaid_content.rs:26-28` |
| 终端不支持图形 | 无像素，仅文本 affordance（`[Open]` 行） | `entry_renderer.rs:351-353`；`blocks/tool/other.rs:413-427` |
| 视频缺 ffmpeg | 1–2 行 `Install ffmpeg to view inline` 横幅（有包管理器时附可执行的 `! …install ffmpeg`） | `inline_media_ffmpeg.rs:5`、`:50-70`、`:117-129` |
| 媒体文件读不到 / 解码失败 | 不显示像素，且按指纹负缓存避免每帧重试 | `app/agent_view/media.rs:143-175` |
| 非图形终端点开图片查看器 | toast `Image rendering not supported in this terminal`（tmux：`Inline images disabled within tmux.`） | `app/agent_view/notices.rs:271-281` |
| iTerm2 预览缺 `TERM_FEATURES` 的 `F` | 降为元数据文本预览 | `terminal/image.rs:181-198` |

复刻时的边角（踩过才知道）：**两套 Mermaid 渲染器意味着两套 bug 面**——内联文本图与 PNG 的换行 / 断词规则靠注释纪律同步，不是共享代码（`xai-grok-markdown/src/mermaid.rs:35-38`）；**不要在绘制中途释放大内存**——帧集 / 大图的释放全部走 `request_release_after_draw`，输入路径（关闭查看器）才允许同步释放（`app/agent_view/media.rs:370-388`）。**minimal 模式与图形能力是两把独立的锁**：`set_inline_overlay_force_off` 与 truecolor / 协议探测互不影响，任何「是否会画像素」的判断都要同时看这两者（`terminal/image.rs:48-62`）；而子进程路径要先 `current_exe()`——worker 用当前可执行文件重新执行自己，`current_exe()` 失败时渲染功能整体关闭而不是回落到进程内渲染（`mermaid_worker.rs:178-203`）。

---

> 来源专题：`08-layout-and-panes.md`、`09-scrollback-block-model.md`、`10-modals-overlays-focus.md`、`11-prompt-widget.md`；4 篇上游均为只读源码调研、未运行构建。

## 4. 界面结构与布局

### 4.1 整体布局与 pane 模型

#### 本节要点

- 主界面是**单列垂直堆叠**：整屏只调用一次 `Layout::vertical`，横向没有分栏；唯一弹性行是 scrollback，唯一地板是 `SCROLLBACK_MIN_ROWS = 5`。
- 每个区域都是「0 / 固定 n 行」的条，出现位置由硬编码的行顺序决定：上行（Tasks / Catalog / Todo）挂在 scrollback **之上**，下行（BTW / Queue / Dock）挂在 prompt **之上**。
- 「pane」不是可拖拽分栏，而是**共享同一条横向带、靠可见性与焦点状态机切换的 overlay 面板**；焦点是单一 `ActivePane` 枚举，切换可被脏编辑拒绝，滚轮按坐标优先级路由。
- 三档高度阈值 `16 / 20 / 28`、三档宽度阈值 `60 / 40 / 10` 全在常量层，并用 `const _: () = assert!(...)` 锁死大小关系；宽度**不触发** agent 主视图的结构性降级。
- dock 是**远程开关特性**，启用后吞掉 Tasks 与 Queue 两个旧面板（高度归零、队列正文搬进 dock），内部按纯函数 `grant_rows` 分预算，无 tab 无栈。

**唯一的布局算法。** 入口是纯函数 `AgentViewLayout::compute(params)`：输入 `AgentViewLayoutParams`（screen `area`、`LayoutConfig`、`ScrollbarConfig`，以及每一个可出现的行所请求的高度），输出 `AgentViewLayout`（每区域一个 `Rect`）。过程是先用 outer padding 求 `inner_area`，把各行 `Constraint` 顺序 push 进 `Vec`，最后 `Layout::vertical(constraints).split(inner_area)`（`views/agent.rs:108-176, 221-299`）。横向只有 `Constraint::Length(1)` 的滚动条列与可选 timeline 轨道，没有任何左右分栏（`views/agent.rs:382-402`）。行顺序（`views/agent.rs:228-298`，方括号表示条件出现）：

```
StatusBar (Length 1)
[gap 1][Tasks] · [gap 1][Catalog] · [gap 1][Todo]        各仅当对应 height > 0
gap 1 (status_gap；top_vpad != 0 时才是 1)
Scrollback (Min 5)                                       ★ 唯一的弹性行
[gap 1] BTW · Queue · TurnStatus · Banner · PluginCTA · FollowUps · Dock   同上，条件出现
[prompt_gap] · [VoiceRecording] · Prompt · [status_line] · Shortcuts (Length 1)
```

**默认 80×24 布局图。** 默认 `outer_vpad = 1`、`outer_hpad_left/right = 2`（`xai-grok-pager-render/src/appearance/config.rs:181-190`），未开 compact，dock 默认关闭，无 Tasks/Catalog/Todo/CTA/follow-ups，prompt 聚焦且正文一行。内区 = `76 列 × 22 行`；固定部分 = statusBar 1 + status_gap 1 + prompt_gap 1 + prompt 3 + shortcuts 1 = 7，故 **scrollback 得 15 行**。

```
列:   1  2  3                                         78  79 80
      ┌──┬─────────────────────────────────────────────┬──┬──┐
y=1   │  │              outer_vpad (1 行留白)            │　│　│   ← 空白，填 bg_base
y=2   │  │ StatusBar                          Length 1 │　│▓▓│   ▓ = 滚动条 (gap_right=0)
y=3   │  │ (status_gap)                       Length 1 │　│　│
y=4   │  │ ┌─────────────────────────────────────────┐ │　│▓▓│
 ..   │  │ │                                         │ │　│▓▓│
y=18  │  │ │ Scrollback   Min(5) → 实得 15 行         │ │　│▓▓│
      │  │ └─────────────────────────────────────────┘ │　│▓▓│
y=19  │  │ (prompt_gap)                       Length 1 │　│　│
y=20  │  │ ╭─────────────────────────────────────────╮ │　│　│
y=21  │  │ │ ❯ 输入                                  │ │　│　│   prompt = vpad_top(1)+1+info(1) = 3 行
y=22  │  │ ╰─ info ──────────────────────────────────╯ │　│　│
y=23  │  │ Shortcuts                          Length 1 │　│　│
y=24  │  │              outer_vpad (1 行留白)            │　│　│
      └──┴─────────────────────────────────────────────┴──┴──┘
        └2 列 hpad_left                        hpad_right┘└2 列┘
```

关键几何：prompt 最小高度 `vpad_top + 1 + info_block` = 3 行（`app/agent_view/render.rs:965-966`、`views/prompt_widget/mod.rs:220-222, 275-277`）；prompt 上限 `area.height / 2`，24 行屏幕最多 12 行输入区（`app/agent_view/render.rs:806`）；`scrollbar_x = area.right() - gap_right - 1`，80 列、`gap_right = 0` 时落在**最后一列**（右 padding 之内），内容只在 `content_end_x < scrollback_right` 时才裁，故本例内容拿满 76 列（`views/agent.rs:382, 394-402`）。

**宽屏 120×40：轨道 + todo + dock。** 内区 = `116 列 × 38 行`；timeline 可用条件为 `show_timeline = true`、根视图、宽度 ≥ `MIN_TERMINAL_WIDTH = 60`、轮数 ≥ `MIN_TURNS = 2`（`views/timeline.rs:17-23, 61-73`）；todo 4 行、dock 6 行（2 个非空 section，各 header + 2 行）。固定部分 = 19 行 → **scrollback 得 19 行**。

```
列:   1  2  3                                        118  119 120
      ┌──┬─────────────────────────────────────────────┬───┬───┐
y=1   │  │                  outer_vpad                  │   │   │
y=2   │  │ StatusBar                                    │   │   │
y=3   │  │ (pane_gap)                                   │   │   │
y=4.. │  │ ┌─ Todos ──────────────────────────────────┐ │   │   │
y=7   │  │ └──────────────────────────────────────────┘ │   │   │
y=8   │  │ (status_gap)                                 │   │   │
y=9.. │  │ ┌──────────────────────────────────────────┐ │   │ ▌ │
y=27  │  │ │ Scrollback  Min(5) → 实得 19 行            │ │   │ ▐ │ ← rail RAIL_WIDTH = 2
      │  │ └──────────────────────────────────────────┘ │   │ ▌ │   (滚动条本帧不画)
y=28  │  │ (gap)                                        │   │   │
y=29..│  │ ┌ ▾ Subagents ─────────────────────────────┐ │   │   │
y=34  │  │ │ ▾ Tasks ─────────────────────────────────│ │   │   │
      │  │ └──────────────────────────────────────────┘ │   │   │
y=35  │  │ (prompt_gap)                                 │   │   │
y=36..│  │ ╭──────────────────────────────────────────╮ │   │   │
y=38  │  │ ╰─ info ───────────────────────────────────╯ │   │   │
y=39  │  │ Shortcuts                                    │   │   │
y=40  │  │                  outer_vpad                  │   │   │
      └──┴─────────────────────────────────────────────┴───┴───┘
```

rail 定位：`timeline_x = (scrollbar_x + 1) - timeline_width`，即恰好占据滚动条那一列往左的 2 列，且 `timeline_width > 0` 意味着**本帧滚动条不渲染**；宽度或轮数不足时回落为滚动条（`views/agent.rs:383-393`）。**宽度方向没有分栏**：全 crate 的 `Layout::horizontal` 只命中三类用途——对话框/菜单的水平居中（`views/new_worktree_dialog.rs:38-43`、`views/welcome/menu.rs:49-54`）、scrollback 单条 entry 的内部 gutter（`scrollback/layout.rs:15-25`）、prompt widget 内部的 chrome 列分配（`views/prompt_widget/mod.rs:3023-3041`）；**未找到证据**：任何「左右分栏 / 侧栏 / 可拖拽分隔条」的布局代码或配置。

**可停靠面板清单**（每面板各持一个 `OverlayState { visible, focused, fullscreen }`，`views/overlay.rs:47-56`）：

| 面板 | 归属 | 高度来源与上限 |
| --- | --- | --- |
| Tasks（后台任务 + 子代理 + 计划任务） | 上（scrollback 上方） | `tasks.desired_height(area.height)`，上限 `MAX_TASKS_HEIGHT = 8` 与 `MAX_TASKS_FRACTION = 0.15` 取小（`views/tasks_pane.rs:657-658, 1052-1064`） |
| Catalog（子代理目录） / Todo | 上 | `desired_height(...)`；上限 `8 / 0.15`、`MAX_TODO_HEIGHT = 10 / 0.15`；**Catalog 在 `view_height < 12` 直接返回 0**（`views/subagent_catalog_pane.rs:69-70, 173-187`、`views/todo_pane.rs:155-158, 270-281`） |
| BTW（`/btw` 侧问） / Queue（待发提示队列） | 下（scrollback 下方） | `btw_overlay::btw_panel_height(...)`（`app/agent_view/render.rs:1089-1090`）；`queue.desired_height()`，`MAX_QUEUE_HEIGHT = 3`、`clamp(1,3)`（`views/queue_pane.rs:349, 600-605`） |
| Dock | 下（prompt 正上方） | `dock::desired_height(&data)`，`MAX_DOCK_ROWS = 8`（`views/dock/mod.rs:22`） |

**dock：把三类面板合并成一条。** 自描述是「prompt 上方的合并面板：每个非空 section 一个 header」，由**远程开关 `dock_enabled` 门控**（`views/dock/mod.rs:1-2`）；section 固定四个 + 一个队尾 `Section { Workflows, Subagents, Tasks, Watchers, Queued }`（`:78-83`）。

- **开关解析优先级**（`app/mod.rs:193-218` `resolve_dock_enabled`）：托管 pin（`requirements.toml` 的 `[features] dock`）→ 环境变量 `GROK_DOCK`（未设时回落 `GROK_DOCK_V2`）→ 有效配置的 `[features] dock` → 远程 `dock_enabled` → **默认关闭**；结果写进进程级 `AtomicBool`（`app/event_loop.rs:1313-1315`）。唯一找到的用户级运行时开关：dock 开启时 `Ctrl+G`（`ActionId::ToggleTasks`）不再切 tasks 面板，而是切 `dock_hidden`（`app/agent_view/input.rs:1046-1062`）。
- **开启即吞并旧面板**：`dock_on = enabled() && !viewer_open && area.height > SHORT_TERMINAL_ROWS`，为真时 Tasks 与 Queue 高度强制归零，队列可见性改由 `dock_queued_expanded` 决定，`ActivePane::Tasks` / `ActivePane::Dock` 被驱逐回 Scrollback，队列 rect **重定向**为 `dock::queue_body_rect(layout.dock, dock)`（`app/agent_view/render.rs:1009-1044, 1037-1039, 1103-1106, 1226-1231`）。
- **空间分配是纯函数** `grant_rows(counts, cap)`（`views/dock/layout.rs:243-337`）：① 所有非空 section 的 header 先扣预算；② 每个展开 section 先拿 1 行正文（争抢时公平轮转）；③ 队列正文有一行「谁也不能抢」的 `queue_floor`；④ 被截断的 section 再拿 1 行画 `show N more`；⑤ 之后按 `share_rows` 在静息上限 `MAX_DOCK_ROWS` 内均分，**只有用户显式展开（`show_all`）的 section 才能越过它增长**；⑥ 不变量 `normalize_no_lone_rows`：一个 section 要么 0 行、要么 ≥ 2 行、要么全显示（`:339-388`）。调用方再做一次双向裁剪 `DockLayout::with_cap`（`:411-416`），契约由网格化穷举测试 `the_layout_contract_holds_across_the_grid` 验证（`:529-622`）。
- **没有 tab、没有栈**：四个 section 同时存在、各一个 header；section 内是**独立 band 滚动**（`SectionSlots<usize> offsets`），滚一个 section 不会移动别的 header（`views/dock/mod.rs:154-160`）。

**尺寸自适应阈值（集中常量层）。**

| 常量 | 值 | 出处与触发行为 |
| --- | --- | --- |
| `SHORT_TERMINAL_ROWS` | 16 | `views/agent.rs:91`；`area.height <= 16` 时底部 padding 归 0、`cta_height`/`follow_ups_height` 强制 0、`prompt_gap` 归 0，**dock 强制关闭**（`views/agent.rs:205-219`、`app/agent_view/render.rs:1012, 1079-1086`） |
| `AUTO_COMPACT_MAX_ROWS` | 20 | `views/agent.rs:97`；`effective_compact()` 在 `rows <= 20` 时强制 compact（`eff_outer_vpad → 0`、`eff_hpad_* → 1`）；**不写回用户设置**，窗口变大即恢复；`terminal_rows == 0`（未测量）永不强制（`:99-104`、`appearance/config.rs:197-217`） |
| 编译期断言 | — | `views/agent.rs:98`：`const _: () = assert!(SHORT_TERMINAL_ROWS < AUTO_COMPACT_MAX_ROWS);` |
| `SCROLLBACK_MIN_ROWS` | 5 | `views/agent.rs:94`；transcript 地板，注释说明求解器把它排在所有 `Length` 之上，过载时收缩的是别的行 |
| `SMALL_SCREEN_TIP_MAX_ROWS` | 28 | `tips/small_screen.rs:23`；21–28 行区间弹一次「Tight on space? Try /compact-mode」（`:27-29`） |
| `MIN_TERMINAL_WIDTH` / `MIN_DASHBOARD_WIDTH` / StatusBar `content_width < 10` | 60 / 40 / 10 | 窄于此隐藏 timeline 轨道（`views/timeline.rs:20`）；dashboard 落「单列、标签中截断」窄模式（`views/dashboard/layout.rs:10`、`views/dashboard/render.rs:169-172`）；顶栏整行不画内容（`views/status_bar.rs:53`） |

窗口宽度**没有**触发 agent 主视图的结构性降级（没有窄屏换布局的分支）——**未找到证据**表明 agent 视图存在横向降级；横向降级只发生在 dashboard、timeline rail 与 welcome 屏。

**可配置性：面板只有开/关。** 可调项为 `[scrollback.layout] outer_vpad = 1`、`outer_hpad_left/right = 2`（`docs/user-guide/05-configuration.md:707-713`；`MIN_HPAD = 1`，`appearance/config.rs:195, 220-228`）、`block_pad_left/right = 2`（`scrollback/layout.rs:20-25`）、`[scrollback.scrollbar] enabled = true` 与 `gap_left/gap_right = 0`（`05-configuration.md:715-718`）、`[ui] show_timeline`（默认 `yes`，`26-config-reference.md:627`）、`[ui] compact_mode`（默认 `yes`，或 `/compact-mode`，`:593`）；面板只有开/关——`Ctrl+T` todos、`Ctrl+G` tasks（dock 开启时为 dock）、`Ctrl+;`（macOS VS Code 家族 `Ctrl+4`）queue（`actions/defaults.rs:36-41, 533-539, 548-551`）。**分栏、面板宽度、面板位置、侧栏顺序、面板高度上限**：❌ **未找到证据**——面板宽度永远写死为整宽，高度上限全是硬编码常量。额外隐藏闸门：minimal 模式下动作注册表删掉 `ToggleTasks` 且不注册 `OpenDashboard`（`actions/mod.rs:671-674, 769-774`）。

**焦点模型：单一 `ActivePane` + 每面板三态 `OverlayState`。** `ActivePane` 是全屏唯一的活动 pane 标识，7 个值（`views/agent.rs:30-41`）：`Scrollback / Todo / Queue / Prompt / Tasks / Catalog / Dock`；`ActivePane` 的别名 `AgentPane` 只是 `pub type AgentPane = ActivePane;`（`app/agent_view/mod.rs:209`）。

- 唯一写入口是 `set_active_pane(target, force)`：它同时清掉其它 pane 的 `overlay.focused`，并在切出 Scrollback 时关闭搜索（`app/agent_view/input.rs:1350-1380+`）。兼容子结构是每个 overlay 面板自己的 `focused: bool`，必须与 `active_pane` 一致，否则渲染与按键归属会分叉；dock 则现算 `focused: active_pane == ActivePane::Dock`（`app/agent_view/panes.rs:693`）。有些切换会被**拒绝**而非静默发生：脏的 queued-prompt 编辑阻止切换（`panes.rs:135-137`）；`ComposerRoute::Hidden` 时切到 Prompt 直接返回 `false`（`input.rs:1351-1353`）。
- 焦点移动键：`Tab` 聚焦 prompt（alt `i`、`Space`）/ 聚焦 scrollback（`actions/defaults.rs:469-487`）；Prompt → Dock、Prompt → Tasks 需 `dock_shown && !dock_hidden` / tasks overlay 可见（`panes.rs:41-54`）；Dock 内 `Tab` 回 Prompt，`BackTab`/`Shift+Tab`/`Esc`/`q` 回 Scrollback（`panes.rs:941-972`）；各 overlay 面板用 `Esc`/`q` 回 Scrollback（`panes.rs:289, 909, 1101, 1129`）；Prompt ↔ Scrollback 是 `Tab` 双向（`docs/user-guide/03-keyboard-shortcuts.md:90-98`）。
- **没有全局 focus ring**，而是「谁能吃这个键」：每个面板自己处理键，失焦时返回 `OverlayAction::FocusScrollback`（`views/overlay.rs:26-38`）。滚轮不走焦点，按坐标路由：`pane_areas.hit_test(col, row).unwrap_or(Scrollback)`，固定优先级 **Tasks > Catalog > Todo > Queue > Dock > Scrollback > Prompt**（`views/agent.rs:63-87`、`panes.rs:1296-1324`）。「区域 = 状态机」在源码里有自证图（`views/overlay.rs:8-20` 原文即为 ASCII：Hidden → Visible+Focused → Hidden，旁支是 Visible+Unfocused，`Tab`/`Space` 与 `Esc`/`q` 分别驱动）；结构性按键（`Tab`/`Esc`/`q`/`Space`/`Ctrl-F`）由共享的 `handle_overlay_key` 处理，面板只需实现自己的内容按键（`:1-6, 73-120`）。

**与 dashboard 的关系（本节只记边界）。** dashboard 是**第二套完全独立的布局函数**（`views/dashboard/layout.rs:195-199`，字段 `top_margin/header/header_gap/actions/actions_gap/list/dispatch/footer/bottom_margin`），用手写 y 游标 + 逐行阈值（`>6 / >4 / >10 / >11 / >12 / >13 / >=2 / >16`）而非 ratatui 约束求解，注释说明这个阶梯是为了让列表高度**永不随终端变高而缩小**（`:229-257`）。它与 agent 视图**只共享阈值约定**（`bottom_margin` 的 `> 16` 与 `SHORT_TERMINAL_ROWS` 同值，注释点名「same threshold as `views::agent::AgentViewLayout::compute`」）与 2 列 padding 习惯（`DISPATCH_OUTER_HPAD`/`HEADER_OUTER_HPAD`/`LIST_OUTER_HPAD` = 2，注释说 matches `LayoutConfig::outer_hpad_left/right = 2`），**不共享布局代码**（`:152-164, 189, 243-244`）；peek 预览面板是**列表优先分配**——`LIST_FLOOR_ROWS = 12` 地板先扣、peek 上限 `⌊H × 3/8⌋`（`PEEK_MAX_FRAC_NUM/DEN = 3/8`）、live-tail 正文再被 `MAX_LIVE_TAIL_ROWS = 28` 二次封顶，且 **peek 没有独立 rect**（画在 dispatch rect 里，`:353`）。多会话不是「分屏」，而是在同一个全屏视图里切换 `ActiveView`（`Welcome` / `Agent(AgentId)` / `AgentDashboard`，`app/app_view.rs:202-210`）。

### 4.2 Scrollback 的块模型与渲染管线

#### 本节要点

- 数据流四段式：**块（Block）→ 条目（Entry）→ 行（BlockLine）→ 单元格（Buffer cell）**；关键决策是块产出的**已经是换行后的屏幕行**，布局层只做高度算术，不做文本排版。
- 块是**纯函数 + 显式上下文**：`BlockContent::output(ctx) -> BlockOutput` 无副作用；trait 还声明 accent/bullet/背景/折叠状态机/可选性，枚举用 `delegate_block!` 宏转发。
- 每行自带四类语义元数据（`Selectable`、`selection_range`、`selection_text`、`joiner`），一套数据同时服务绘制、搜索、复制与导出。
- 虚拟化靠 `LayoutCache.virtual_y` 前缀和 + `compute_paint_window` 的 `partition_point` 二分，每帧只遍历可见窗口；高度是「先估算 → 接近视口才精确测量」的两级制。
- 粘性头部是纯 1D 计算（`full_height - scroll_past`，1:1 收缩）；跟随尾部在用户上滚时立即关闭，只有「滚到底再过滚」才重新开启。
- 折叠有两族：主动的 VerbRun 聚合（「Read 3 files」）与预算截断的「N more」，二者由单次扫描统一产出 `GroupSpan`。

**分层全景**（`scrollback/` 模块）：

```
会话 / 工具事件 ──► AgentView 把消息、工具结果、会话事件转成块
【块层】 RenderBlock (block.rs:298，13 变体) impl BlockContent (block.rs:40)
    fn output(&self, ctx: &BlockContext) -> BlockOutput      // blocks/{user,agent,thinking,tool/*,…}.rs
   │  BlockOutput { lines: Vec<BlockLine> }        types.rs:433
   │  ★ 每个 BlockLine 已是「按内容宽度换行后的一行」 render.rs:498
【条目层】 ScrollbackEntry (entry.rs:57) = 块 + display_mode + 4 层缓存
    ScrollbackState (state/mod.rs:53) = entries / scroll / 选中 / turns / group 折叠 / LayoutCache (state/layout.rs:42)
   │  每帧 prepare_layout (state/mod.rs:1344) → paint window → render
【单元格层】 ScrollbackPane::render_with_scratch (scrollback_pane.rs:139)
    └ EntryRenderer (impl Renderable, entry_renderer.rs:23/543)
        desired_height(width) → 高度；render(area, buf) → 单元格
```

**块抽象。** `BlockContent`（`block.rs:40`）只描述「渲染什么」，不碰 Buffer，方法按职责分组：内容产出 `output`（`:42`，唯一必选内容方法）；样式 `accent` / `bullet` / `accent_background` / `background`（`:46-62`）与 `has_vpad_for`（`:69`，上下各 1 行留白，默认 true）；模式 `has_raw_mode` / `is_foldable`（`:81-86`）与折叠状态机 `next_fold_mode` / `collapse_mode` / `default_display_mode` / `finished_display_mode`（`:92-124`）；交互 `is_selectable`（`:131`）、`has_bullet` / `is_groupable`（`:141-145`）、`preamble`（`:148`，全屏查看器头部文本）；媒体 `inline_media` / `inline_media_placements` / `diagram_affordances` / `inline_open_button`（`:169-206`）与 `estimate_extra_rows`（`:203`，仅 Mermaid 用）。`RenderBlock::output()` 转发后统一补 bullet：`prepend_bullet`（`:202`）把 bullet span 插到首行行首，并调用 `shift_selection_metadata_for_prefix`（`types.rs:398`）把可选区整体右移一格，避免 bullet 被复制进剪贴板。

`RenderBlock`（`block.rs:298`）13 变体与内部类型一一对应：`Stub` / `UserPrompt` / `AgentMessage` / `ToolCall` / `Thinking` / `System` / `SessionEvent` / `BgTask` / `Subagent` / `Workflow` / `Btw` / `ContextInfo` / `MemoryCapture`；同文件 `delegate_block!` 宏把全部 trait 方法 match 转发到内部类型（`:331-347`），只有 `output()` 需拦截——`RenderedBlockOutput` 路径用于 Edit / SentMessage 这两个需要额外 selection boundary 侧车数据的块（`:413-425`）。`ToolCall` 再套一层 `ToolCallBlock`（`blocks/tool/mod.rs:149`）：`Execute` / `Read` / `Edit` / `ListDir` / `Search` / `WebFetch` / `WebSearch` / `IntegrationSearch`（MCP 发现）/ `UseTool`（MCP 调用）/ `MemorySearch` / `SentMessage` / `Skill` / `Other`，同样用 `delegate_tool!` 转发（`:171-189`）。各块的差异行为：`UserPromptBlock`（`blocks/user.rs:84`）折叠保留 3 行（`COLLAPSED_MAX_LINES`，`:15`）、slash token 单独着色；`AgentMessageBlock`（`blocks/agent.rs:11`）流式 `push_chunk` / `finish`（`:46/67`）；`ThinkingBlock`（`blocks/thinking.rs:124`）三态折叠、collapsed 头部 `Thought for Xs (ctrl+e to expand)`（`:18`）；`ToolCallBlock` 共用 header 选择区 id `TOOL_HEADER_RANGE = 0`（`blocks/tool/mod.rs:47`）并有 `VerbGroupKind` 聚合动词/名词表（`:81-143`）；`SubagentBlock`（`blocks/subagent.rs:45`）与 `BgTaskBlock`（`blocks/bg_task.rs:36`）均「always collapsed and not foldable」（`:42-43` / `:33-34`）；`SessionEventBlock`（`blocks/session_event.rs:603`）覆盖约 30 个会话事件变体（`:49-190`）、不可选不可折叠（`:47`）；`MarkdownContent`（`blocks/markdown_content.rs:40`）是 agent/thinking/btw 共享的流式 markdown，换行缓存键为 `(width, generation, theme)`（`:23-37`）；`QuoteBarStrip`（`blocks/quote_bar.rs:19`）把引用条 `│` 从可选区中排除（`quote_prefix_selectable`，`:131`）。

**块 → 行。** `output()` 内部完成三件事：markdown 渲染、按 `ctx.width` 换行（如 `word_wrap_lines(styled_lines, ctx.width)`，`blocks/system.rs:26-30`）、打 selection 元数据。`BlockLine`（`types.rs:146`）字段为 `content: Line<'static>`（绘制的 span 序列）、`wrap: WrapMode`（Word / Character / Truncate，`types.rs:16`）、`selectable: Selectable`（All / Spans(range) / None，`types.rs:72`）、`selection_range: Option<u16>`（逻辑选择分组 id，组头保留 `u16::MAX`）、`selection_text: Option<String>`（复制文本覆盖，工具头等）、`joiner: Option<String>`（软换行连接符，复制时替代 `\n`）、`background` / `bg_start_col` / `indent_width` / `link_target`。复制文本由 `derive_selection_text`（`:289`）推导：优先 `selection_text` 覆盖，其次按 `Selectable` 取 span 子集，`All` 会 trim 尾部空白；软换行分组靠 `joiner`——`prewrap_index_per_row`（`:605`）用「joiner 为空则逻辑行 +1」把后换行行映射回源码行，是搜索命中定位到行的基础。

**行 → 单元格。** `EntryRenderer` 是唯一往 Buffer 写单元格的地方（`wrappers/entry_renderer.rs:561`）：① `content_width = area.width - chrome_width - timestamp_reserved`；② `entry.ensure_cached(content_width, …)` 取缓存输出；③ 背景按 `BlockBackground` 逐列填 padding/content/right_pad，flat 模式（minimal）退回 `Color::Reset`（`fallback_bg`，`:110`）；④ accent 列**仅 `display_mode != Collapsed` 时**画，运行中的块用 `AccentStyle::animated` 画波纹（`WAVE_SPEED = 0.15` 弧度/tick，`:20`）；⑤ 内容行从 `skip_rows` 起逐行 `Buffer::set_line`。横向几何固定为 `accent(1) | block_pad_left | Min(1) | block_pad_right`，`Min(1)` 那格就是传给 `BlockContext.width` 的内容宽度（`scrollback/layout.rs:19`；选中边框另画，`selection_area` 向左右各扩 1 列，`:75`）。每帧时序：`ScrollbackState::prepare_layout(width, height)`（`state/mod.rs:1344`）依次做「视口尺寸变化 → `bump_generation()`（链接图失效，`:1352`）→ 缓存缺失/宽度变化则全量重建并捕获 `ScrollAnchor`（`:1370`）→ `settle_visible_measurements`（`state/layout.rs:745`）→ `handle_follow_mode`（`state/nav.rs:1024`）→ 更新 `total_height`」，随后 `ScrollbackPane::render_with_scratch`（`scrollback_pane.rs:139`）按 `AllTurns → render_with_sticky_headers(:294)` / `SingleTurn → render_single_turn(:277)` 分支、`state.paint_window(...)` 只遍历可见窗口（`:1498`）、逐条 `EntryRenderer::render`，返回 `RenderOutput{selection_box, scroll_info, selection_model, link_overlay, …}`。

**虚拟化与增量渲染。** `LayoutCache`（`state/layout.rs:42`）持有 `entries: Vec<EntryLayoutInfo>`（高度 + `gap_after` + 分组标志）、`entry_truncated_heights`（sticky 头部 min_height 用）、`measured: Vec<bool>`（true = 精确测量）、`virtual_y: Vec<usize>`（每个 entry 的累计 Y 前缀和）、`prompt_descriptors`、`groups: Vec<GroupSpan>`、`width`；`height` 是**已换行后的行数**，`gap_after` 让相邻两个可密集相接的折叠块为 0、否则为 1（`:1555-1566`）。`compute_paint_window`（`:1568`，纯函数、可单测）的三条规则：用 `partition_point` 在 `virtual_y` 上二分求 `[vp_start, vp_end)`，O(log n)；起点回退一格以容纳「起点之上但越过视口顶的行」；**若窗口内存在组头行，把窗口右界扩到该 run 末尾**，让聚合标签能看到屏外成员——这是唯一会越过视口边界的情形。返回的 `content_y0` 让渲染不必从历史顶部累加（源码注释：「a viewport window does not re-walk off-screen history」）。

高度是两级制：`measurement_window`（`:593`）只覆盖首个可见条目 + 其后 `MEASURE_MARGIN_ENTRIES = 8` 个（下方余量，**无上方余量**）；`measure_window_exact`（`:669`）对未测量条目真正调 `output()` 量高（隐藏行与合成组头跳过）；`settle_visible_measurements`（`:745`）循环「测量 → 重建 virtual_y → 重锚视口」直到窗口内全部精确；`warm_measure_pages_above`（`:796`）在 resume 后向上预热 `RESUME_WARM_PAGES = 3` 屏，避免上滚时高度跳变。估算本身便宜：`estimate_source_lines`（`entry.rs:286`）用缓存的逐行显示宽度 `Vec<u32>` 做 `ceil(w / content_width)` 求和（`wrapped_lines_from_widths`，`:48-62`，饱和到 `u16::MAX`）。

**缓存分四层且失效语义刻意分离**（`entry.rs:88-102`）：

| 缓存 | 键 | 失效方法 |
| --- | --- | --- |
| `cached_output: RenderedBlockOutput` | `(width, raw, theme, is_selected, cwd)`（`entry.rs:11-18`） | `invalidate_cache` / `invalidate_width_caches` |
| `cached_truncated_height` | `(content_width, raw, theme, cwd)`（`:23`） | 同上 |
| `cached_estimate_lines` | `(content_width, lines)` | 同上 |
| `cached_line_widths` | 与宽度无关 | **只**被 `invalidate_cache` 清 |

失效语义：`invalidate_cache`（`:246`）先调 `invalidate_width_caches`（`:252`，把 `cached_output` / `cached_truncated_height` / `cached_estimate_lines` 置 `None`）再把 `cached_line_widths` 置 `None`——即 resize 路径保留与宽度无关的逐行宽度，只有内容变了才丢它。`ensure_cached` 还把「只有部分块在意选中态」编码进缓存键：仅 UserPrompt / ToolCall / Thinking / BgTask / Subagent 的 `is_selected` 参与键，其余块归一化为 false，避免选中移动导致缓存抖动（`:326-332`）。离屏驱逐 `evict_render_cache`（`:261`）只丢重渲染输出、**保留**高度缓存与布局，滚回来透明重算，保留窗口为 `±EVICT_KEEP_MARGIN_ENTRIES = 128`（`state/layout.rs:627-633`）。`ScrollbackState` 侧另有两个代际计数：`generation`（视口/链接位置变化即 +1，供 `VisibleLinkMap::is_stale`）与 `content_generation`（只在增删条目或内容变化时 +1，供搜索语料缓存复用）（`state/mod.rs:196-203`）。其余滚动期手段：`skip_rows` 裁剪局部可见条目以消除其 scratch buffer 渲染（`entry_renderer.rs:35-40`）；`ScratchBuffer` 复用给 sticky 推挤裁剪（`render.rs:69-100`）；脏高度集合 `dirty_heights` + `update_dirty_entry_heights` 只重算脏条目（`state/mod.rs:74`、`state/layout.rs:981`）；增量 `patch_virtual_y_for_dirty` / `extend_layout_cache_with_new_entry` 避免流式增长触发 O(n) 全量重建（`:1100, 1189`）；流式 markdown 冻结已换行前缀（`frozen_pre_wrap_count`，`blocks/markdown_content.rs:23-37`）；`begin_batch` / `end_batch` 在批量 push 时跳过 turns 与布局重建（`state/mod.rs:161-165`）。

**滚动锚点两套**，解决两类位移：`ScrollAnchor { entry_idx, logical_line, sub_rows }`（`state/layout.rs:16`）用于宽度变化——按**逻辑行**重定位（resize 后换行行数变了，绝对行号会指错内容），只在非跟随模式且 `scroll_offset > 0` 时捕获（`state/mod.rs:1370-1375`）；`StructuralScrollAnchor { id: EntryId, rows_into_span, armed_scroll_offset }`（`:26`）用于增删条目这类会移动索引的突变，用稳定 `EntryId` 记录、下次 `prepare_layout` 消费，期间发生过显式导航则不生效（`state/mod.rs:1361-1363`）。

**粘性头部**（`sticky.rs`，纯 1D 坐标计算，不关心块内部结构）：每个 prompt 压缩成 `PromptDescriptor { entry_idx, y_virtual, full_height, min_height, sticky }`（`:23`）；取「最后一个已被滚过（`y_virtual < scroll_offset`）的 sticky prompt」作为 `pinned`（`:252-264`）。`calculate_render_height` 让 `render_height = full_height - scroll_past`，与滚动 **1:1** 收缩（保证内容底行不跳），下限 `min_height`（估算种子 `MAX_TRUNCATED_HEADER_HEIGHT = 6`，`state/types.rs:71`）、上限视口高；`MIN_PINNED_HEIGHT = 4`（`:45, 339`）。下一 prompt 的裸行号 ≤ `render_height + HEADER_CONTENT_GAP`（=1，`:92`）时进入 push 过渡，当前头变 `pushed{ render_height, clip_top }` 逐渐被推出屏幕；`next_naive_row == 0` 时头部消失、下一 prompt 接管（`:277-330`）。关闭条件：`scrollback.display.sticky_headers` 为 false 或 prompt 处于 compact 模式（`scrollback_pane.rs:310-315`）。

**跟随尾部（follow）。** 状态是 `follow_mode: bool` + 一次性标记 `follow_preserve_scroll` + 页面翻转预留 `pin_reserve_*`（`state/mod.rs:106-136`）。打开时每帧 `handle_follow_mode` → `follow_scroll_to_bottom` 把 `scroll_offset` 钉到 `max_scroll_offset()`（内容不足一屏时返回 0，顺带修正被缩短内容卡住的越界偏移，`state/nav.rs:1024, 1052, 1084-1088`）。**用户上滚即暂停跟随**：`scroll_up` 无条件 `follow_mode = false`（`:356-360`），其余显式导航（翻页、跳转、折叠、turn 导航）同样关闭跟随（`:106/218/248/486`）；**重新进入跟随**需向下滚到底后再滚一次（overscroll）且 `follow_by_overscroll` 为 true——`rows > 0` 与 `pin_reserve_pad == 0` 是防止「恰好落到最底」被误判为过滚的守卫（`:361-380`），另有 `enable_follow_mode` / `enable_follow` / `enable_follow_with_preserve` 三个程序化入口（`:522/531/538`）。跟随还会自动选中最后一个可选条目，但仅当「当前无选中」或「选中本来就在尾部」时才移动（`:1034-1047`，受 `follow_auto_select` 门控）。`pin_reserve` 把「最后一个 user prompt 顶到视口顶部」这种页面翻转位姿变成合法滚动底——否则 clamp 到内容高度会把视图弹回尾部；它在某轮结束后释放（`state/pin_reserve.rs:1-2, 30`）。

**搜索。** 语料 `ScrollbackSearchIndex` 为每个条目缓存一段 `searchable_text`（markdown 块给渲染纯文本，其余给源文本），整体 `Arc<[IndexedEntry]>`，只在 `content_generation` 变化时重建（`search.rs:44-93`；`searchable_text` 见 `block.rs:1000-1030`）。扫描在后台线程 `SearchDaemon`：UI 线程只发「最新语料 + 查询」的 O(1) 消息，`poll` 逐帧取结果；若结果属于已被取代的查询则丢弃，避免匹配数与光标错位（`:200, 378-398`）；空查询与非法正则立即清空匹配（`:333-341`）。结果模型 `ScrollbackMatch { entry_id, line_in_entry, byte_range }`（`:32`），`next/prev` 用 `rem_euclid` 环绕（`:401-417`）。**高亮是逐帧后处理遍**：对每个可见行把 span 文本拼进复用的 `highlight_text` 再调 `paint_match_highlights`（`render.rs:500-512`），因此高亮集合按屏幕字形计算，可能少于索引匹配集合（源码里的 markdown 标记在屏上不存在）。入口：vim 模式下 `/` 打开增量搜索栏（`app/agent_view/panes.rs:137-144`），`n`/`N` 与上下箭头推进（`:147-156`），搜索栏占底部 2 行（`search_reserved_rows`，`:160`）。

**选择是「三维逻辑坐标 + 二维屏幕几何」。** 每帧 `render.rs` 把可见行解析成扁平模型（`text_selection.rs:80-296`）：屏幕 `(col,row)` 经 `hit_test_selectable_range`（`:296`）得到 `RangeHit { entry_idx, range_id, block_line_idx, col_within_range }`（`:199`）；`ResolvedSelectionModel` 持有 `ranges` 与 `visible_blocks: Vec<VisibleBlockGeometry { area, content_area, selection_area, content_width, top_clipped, bottom_clipped }>`；`ResolvedSelectableLine`（`:143`）同时带逻辑身份（`entry_idx` / `range_id` / `block_line_idx`）与屏幕几何（`screen_y` / `screen_x` / `selectable_cols`），外加逻辑复制文本、实际画出的 `painted_region`（列吸附用）与 `joiner_to_previous`。三个坐标维度——条目 + 逻辑选择区间（块内从 0 递增，合成组头固定 `u16::MAX`）+ 块内行号（滚动不会改变它）——正是「锚点滚出屏幕仍能复制全文」的基础。拖拽状态机：`PendingTextDrag`（按下时记录 anchor + 锚点块 `content_width` 快照）→ 超过 `drag_threshold_exceeded`（`:436`）→ `ActiveTextDrag { anchor, head, kind }`（`:219`）；松手后 `PersistentTextSelection`（`:273`）保留高亮，遇到别处点击 / Esc / 任何 scrollback 导航才清除。鼠标侧：`SelectionOrigin { Drag, DoubleClick, TripleClick }`（`:234`）分别取范围/词/行，词边界由 `word_boundaries_at_col` + `configured_word_separators`（默认 `DEFAULT_WORD_SEPARATORS`，`:1150`）决定；URL 有专门识别并剥掉尾部标点（`url_range_at_col`，`:1296`；`TRAILING_URL_PUNCT`，`:1249`）；拖到边缘触发定时自动滚动：`compute_autoscroll`（`:40`）在距内容区上下沿 `EDGE_THRESHOLD = 2` 行内返回方向与速度，速度按越界距离分档 1/2/3/5 行每 tick（`:70-77`）；`semantic_selection_at`（`:1350`）处理双击/三击在软换行连接处的扩展。

**复制**走缓存重切片而非可见行：`reconstruct_drag_copy`（`app/agent_view/selection.rs:719`）先试表格几何匹配 → 表格 TSV，否则用锚点块的完整输出 `reconstruct_full_selection_text_with_boundaries`（`text_selection.rs:1016`）——即从块缓存重新切片，若块已不可取则退回可见模型的 `reconstruct_selection_text`。列语义基于 `painted_selectable_region`（`types.rs:322`，实际画出的字形），逻辑文本与绘制文本不一致时（工具头覆盖文本）按 bidi 状态分流（`:1076-1096`）；范围覆盖到行尾时剥掉渲染用的尾部填充（与 `derive_selection_text` 的 trim 一致）。boundary 侧车 `SelectionBoundary { prefix, suffix, has_empty_row_anchor }`（`types.rs:440-492`）记录「可见行省略掉的源码字节」，复制时按选区是否触达首/末列决定贴回，保证复制仍是完整源码。落点为 `copy_to_clipboard` → `crate::clipboard::copy_text_or_file` + toast，另有防抖版本（`app/agent_view/notices.rs:248-267`）；键位 `y` 复制块内容、`Y` 复制命令/路径（`actions/defaults.rs:352-370`）。**表格选择**：`TableGeometry` 从已渲染的行里**反推**箱线几何（`junction_cols` = 竖线显示列，个数为列数 + 1；`rows` = 每个逻辑行的块行范围，单元格内换行会跨多行，`table_geometry.rs:19-30`），`SelectionKind` 三态为 `Linear` / `TableCell`（锚点吸附在同一单元格列带内）/ `TableGrid { anchor: CellRef, head: CellRef }`（整格矩形，复制为 TSV，`text_selection.rs:243`）；`detect` 无法证明是表格时一律回退 `Linear`（`table_geometry.rs:1-2`），几何在 promote 时冻结，若之后流式重排导致行号漂移会重新检测并要求完全一致（`selection.rs:734-738`）。

**链接与 OSC 8**：每帧从 `LinkOverlay`（markdown 超链接 + web_search/web_fetch 引用 URL）重建 `VisibleLinkMap`（`link_map.rs:40`），`VisibleLink.rects` 允许一个逻辑链接跨多行（`:14-18`）；失效靠代际 `is_stale(current_generation)`（`:52`），视口尺寸变化在 `prepare_layout` 里 `bump_generation`（`state/mod.rs:1352-1355`）；点击命中 `link_at(col,row)`（`:46`），`looks_like_bare_url_text`（`:25`）区分「屏上直接显示 URL」与「短标签/宽引用块」以决定提示文案。

**折叠/展开/截断。** 显示模式是三态枚举 `DisplayMode { Collapsed, Truncated, #[default] Expanded }`（`types.rs:63`），折叠是 trait 提供的状态机（`block.rs:92-127`）：默认 `Collapsed ↔ Expanded`；thinking 运行中 `Collapsed|Truncated → Expanded`、`Expanded → Truncated`，结束后 `Collapsed ↔ Expanded`（`blocks/thinking.rs:532-544`），`collapse_mode` 运行中给 `Truncated`（`:546-552`），默认模式 `Truncated`（`:554`），结束自动 `Collapsed`（`:558`）；`display_mode_pinned`（`entry.rs:73`）标记「用户手动折过」的块，配合 `respect_manual_folds` 让重排不再自动展开（`docs/user-guide/03-keyboard-shortcuts.md:68-70`）。分组折叠由 `state/groups.rs` 单次扫描同时决定，产出权威 `GroupSpan`，再 `project_to_layout` 写回每 entry 的标志（`:1-10/62-76`）；`GroupKind`（`:39`）两族为 `VerbRun { members }`（主动折叠的动词 run，标签由 `verb_group_header_label` 生成，动词/名词表在 `VerbGroupKind`，`blocks/tool/mod.rs:81-143`，`group_tool_verbs` 是总开关）与 `Truncation { participants, hidden }`（过长的密集 run 按 `max_visible` 预算隐藏，头部显示「N more」或聚合标签，`GroupHeaderLabel`，`state/verb_group.rs:150`）。渲染上合成组头行高 1，标签行本身是可选行（`range_id = GROUP_HEADER_RANGE_ID = u16::MAX`，`render.rs:466-490`），复制得到聚合文本；`EntryLayoutInfo::is_group_header()`（`state/types.rs:113`）是所有消费者的唯一判据；选中被隐藏条目时 `fixup_hidden_selection` 把选中移到最近可见条目、优先组头（`state/layout.rs:157-180`），用户展开过的组记在 `expanded_groups`（`state/mod.rs:191-194`）。

超长输出三档：`Truncated` 取前 `first_lines` + `…` + 后 `last_lines`，默认 `first_lines = 2`、`last_lines = 3`（`blocks/tool/execute.rs:471`；`appearance/config.rs:613-614`，可在 `pager.toml` 调整）；`Collapsed` 是单行摘要（Execute 的折叠头在有 description 时隐藏命令行，`execute.rs:317/379`）；`Expanded` 是全文（`truncate = None`，`execute.rs:660-667`）。流式中的 `!` 命令默认显示尾部、结束后可展开（`execute.rs:32/964-973` 测试）；Enter / `Ctrl-F` 打开块查看器是「显示更多」的最终出口（`actions/defaults.rs:382-386`）。高亮类块有显式上限防卡死：`EDIT_HL_MAX_BYTES = 2 MiB`、`EDIT_HL_MAX_LINES = 50 000`（`blocks/tool/edit.rs:46/49`）。键位（均有 registry 证据）：`h`/`Left`、`l`/`Right` 折叠展开（`actions/defaults.rs:265/278`）、`e` 切换折叠（`:291`）、`⇧E` 全部展开/折叠（`:306`）、`Ctrl+E` 全部 thinking（`blocks/thinking.rs:16-17`）、`r` 切 raw markdown（`:336`）、`Ctrl+K`/`Ctrl+J` 行滚动（`:182/195`）、`Ctrl+U`/`Ctrl+D` 与 `PageUp`/`PageDown` 半页整页（`:208/221/238/251`）、`y`/`Y` 复制（`:352/367`）。

**导出是旁路。** `export.rs` 把块序列转成人类可读 Markdown：只保留 `## User` / `## Assistant` / `## Tools`，Thinking / System / SessionEvent / BgTask / Subagent 等 chrome 跳过，连续 assistant 消息合并到一个标题下，工具调用压成一行摘要（`:1-6/14-66`）——这是「内容与呈现分离」的额外好处：导出不经过渲染管线。

### 4.3 模态、浮层与焦点栈

#### 本节要点

- **没有「浮层栈」这个对象**：真正的模态是单槽位 `Option<ActiveModal>`，多浮层的 z 序由 `render.rs` 里 `if … return` 的**分支顺序**定义，因此这些分支实际上是互斥的。
- 所有带边框弹窗共用同一套 chrome（`ModalWindowConfig` + `render_modal_window`），默认尺寸 `0.9 / 140 / 60 / v_margin 7`，水平垂直双居中；小于 `20×6` 直接拒绝渲染并清空全部鼠标命中矩形。
- 遮罩**不统一**：主流是 `Clear` 清底、背景不变暗；只有少数全屏浮层用 `dim_area(0.5)`，失焦「退后」用 `recede_area(0.66)`。
- 焦点不是栈，而是一张显式排名表 `KeyOwner`（Permission 永远最高）加一个 ESC 阶梯枚举 `EscStep`（8 档，每档带用户可见标签，快捷键栏与实际处理读同一张表）。
- 嵌套靠三种手法实现（同一 popup 内换内容 / 后画覆盖前画 / 快照回退），**不能嵌套的是「同层再来一个模态」**；真正的「排队」只有 permission 队列（`VecDeque`，所有判定只看 `front()`）。

**渲染层：分支顺序即 z 序。** 唯一「栈式」的地方是 `AgentView::intercept_takeover_input`（`app/agent_view/subagent_takeover.rs:347`）：子代理全屏接管**在所有其他输入处理之前**跑（`app/agent_view/input.rs:361-363`），非空时直接返回，等价于压在整棵渲染树之上；`key_owner.rs:38-40` 注释也承认「这些全屏接管自己回答输入，因此不参与排名」。其余浮层的相对顺序就是分支顺序（`app/agent_view/render.rs`），**每个分支画完即 `return`**，所以它们实际互斥：

```
  优先级高 ┌──────────────────────────────────────────────────────────────┐
  (z 序前) │ active_modal (:3211)     → 模态弹窗（命令面板/设置/扩展/…）   │
           │ line_viewer (:3244)      → 计划/文件预览（圆角框 + 半透明蒙层）│
           │ image_viewer (:3430) · video_viewer (:3600) · gboom (:3643)   │
           │ block_viewer (:3702)     → 块全屏查看（Ctrl-F）              │
           │ agents_modal (:3894) └ persona_detail（同分支内后画，:3911）  │
           │ feedback_modal (:3922) · extensions_modal (:3940)            │
           │ goal_detail (:4239) · workflows (:4259)                      │
  优先级低 └──────────────────────────────────────────────────────────────┘
  更高一层（在这套渲染之前就 return）：子代理全屏接管
```

真正意义上的「叠」只发生在两条路径上：同一分支内先后两次绘制（agents_modal → persona_detail），以及同一个 popup 内换内容（Settings → ResetSettingsConfirm）。**五种浮层形态，五种容器**：① 模态弹窗（ModalWindow，屏幕居中矩形 + `Clear` 清底，**不做 dim**，如命令面板/设置/扩展/快捷键表）；② prompt 槽位接管（直接占用 `layout.prompt`，完全替换输入框，如 permission / question / elicitation 卡片、`/rewind`、`/jump`、cancel-turn，`render.rs:943-965, 2781-2794`）；③ 全屏接管（整屏或除 shortcuts bar 外全部，多数自带蒙层，如 block_viewer、line_viewer、图片/视频、子代理全屏）；④ 内嵌浮层（复用宿主 popup 矩形，覆盖其内容区，如 ResetConfirmOverlay、ManagedConnectorsWait、memory 全屏变体）；⑤ 下拉/横幅（prompt 上方或紧邻几行，**不遮底层**，如 slash_dropdown、completion_dropdown、file_search、privacy_banner、btw_overlay）。`/btw` 是特例：它**不是 popup，而是布局里的一行槽位**——`btw_height` 被算进 `AgentViewLayoutParams`（`app/agent_view/render.rs:1089-1090, 1123`；`views/agent.rs:120, 247-249`），高度由内容决定（`Loading` 恒 3 行，`Done` 为 `2 + clamp(行数, 1, DONE_MAX_BODY_LINES)`，`Error` 为 `2 + 折行后的错误行数`，`views/btw_overlay.rs:182-200`）。

**ModalWindow 的视觉规范。** 一切有边框的弹窗都走 `render_modal_window`（`views/modal_window.rs:236`），配置是 `ModalWindowConfig`（`:76-87`：`title` / `tabs` / `shortcuts` / `sizing` / `fold_info`），持久状态是纯数据类型 `ModalWindowState`，放在表现层 crate 里以免下层依赖 views 层（`xai-grok-pager-render/src/modal_window_state.rs:1-4`）。`ModalSizing` 七个字段（`:89-121`）：`width_pct = 0.9`、`max_width = 140`、`min_width = 60`、`v_margin = 7`、`h_pad = 2`、`v_pad = 2`（**下方不生效，底部归 footer**）、`footer_lines = 2`；三档预设 `default()` = `large()`、`medium()` = `0.60 / 120 / 44 / 4 / 2 / 1 / 2`（**未找到生产调用点**）、`with_compact(true)` 把 `v_margin→0`、`h_pad→1`、`v_pad→0`（注释：目标是「把每个 popup 的可用内容区最大化」，`:146-157`）。实际尺寸由 `compute_modal_dims`（`:575-586`）算：`max_width = area.width.saturating_sub(4).min(sizing.max_width)`，`preferred_width = (area.width as f32 * sizing.width_pct) as u16`，`modal_width = preferred_width.min(max_width).max(sizing.min_width).min(area.width)`——注意 `min_width` 可以顶到 `max_width` 之上，最后再被 `area.width` 收回（注释解释这是为了不让 `Clear`/`Block` 越界），而 `modal_height = area.height.saturating_sub(sizing.v_margin * 2)`，**没有下限也没有上限**。**两道拒绝渲染闸门**：`modal_width < 20 || modal_height < 6` 时直接返回 `None`，并清空 `popup_area` / `close_button_rect` / `shortcut_hits` / `tab_rects`，避免鼠标命中上一帧的陈旧矩形（`:253-260`）。水平、垂直都居中（`x = area.x + (area.width - modal_width) / 2`、`y = area.y + (area.height - modal_height) / 2`，`:262-271`）；页脚贴的是 **popup 内底**而非屏幕底（`footer_y = inner.y + inner.height - footer_height`，`:381`）。

```
area = 整屏 (W × H)，以 W=100 H=32、ModalSizing::default() 为例
modal_width  = min(100*0.9=90, min(100-4,140)=96) = 90     modal_height = 32 - 7*2 = 18
x = (100-90)/2 = 5      y = (32-18)/2 = 7
col: 0    5                                      94    99
row 0  ┌──────────────────── 上边距 7 行 ────────────────────────┐
row 7  │    ┌──────────────────────────────────────────────┐     │
row 8  │    │ ─ Title ─                            [✗]    │     │  ← 边框行，关闭按钮嵌在上边框
row 9  │    ├──────────────────────────────────────────────┤     │  ← 可选 tab bar 在此
row 10 │    │ ──────────────────────────────────────────── │     │  ← 全宽分隔线
row 11 │    │   content（左右各内缩 h_pad=2）           ║  │     │  ← 滚动条占 content 最右一列
row 23 │    │  ↑/↓ nav  |  Enter select  |  Esc close      │     │  ← footer 贴内底，水平居中
row 24 │    └──────────────────────────────────────────────┘     │
row 25 └──────────────────── 下边距 7 行 ────────────────────────┘
        x=0  x=5                                          x=94
```

- **边框** `Block::default().borders(Borders::ALL)`，未指定 `border_type`（默认直角），`border_style = fg(gray_dim) + bg(bg_base)`，整块样式 `bg(bg_base) fg(text_primary)`；模块注释自称 "accent-colored square border"（`:6`）但代码用 `gray_dim`——**以代码为准**。**标题画在上边框线上**，破折号用边框色、标题字用 `text_primary + BOLD`（`Span("─ ") + Span(title) + Span(" ─")`）；`title` 为空串则整条上边框连续无装饰（`:277-282, 315-322`）。**关闭按钮 `[✗]`** 是 5 cell 定宽串，位于 `x = modal_area.x + modal_area.width - (5 + 2)` 的第 0 行，悬停只把中间三个非空格 cell 改成 `text_primary + BOLD`，两侧 padding 保持原样（`:405-436`）。
- **可选 tab bar**（`config.tabs` 为 `Some` 时才画）：左缩进 2 列、标签间隔两个空格、按显示宽度贪心折行，放不下的行整行跳过并把对应 `tab_rects` 置 `None`；样式三态为「选中且有键盘焦点 → `text_primary on bg_visual + BOLD`」「选中但无焦点 → `accent_user + BOLD`」「未选中 → `gray`」（注释解释为何不用 `gray_dim`：ANSI16 下对比度会掉到 ~1.2:1）；tab bar 下方必画一条与内宽等长 `─`，**且存在 tab 时内容区 `v_pad` 被强制为 0**（分隔线已充当间距）（`:330-360, 508-531`）。
- **内容区与页脚**：`content_top = inner.y + effective_v_pad + tab_bar_height + tab_divider_height`，`content_area.x = inner.x + h_pad`、`width = inner.width - h_pad*2`；`footer_lines` 先取 `sizing.footer_lines`，若快捷键在当前宽度放不下就按**与渲染同一套贪心折行**的预测值抬高（`:363-365, 560-614`，注释强调「预测必须和实际渲染一致」）。页脚行底部对齐、每行水平居中、行内用 `"  |  "` 分隔；单个快捷键按第一个 ASCII 空格切成「键名（`text_secondary + BOLD`）+ 说明（`theme.muted()`）」。**所有快捷键——包括 `clickable: false` 的纯提示——都产出 `ShortcutHitArea`** 用于悬停高亮，只有 `clickable: true` 才派发 `ShortcutActivated`（`:616-761`）。`/settings` 与 how-to 指南多一层 tip 行：内容高 ≥ 3 时切出最后一行、≥ 6 时再空一行，tip 右对齐居中，页脚相应预留 `预测行数 + 1` 且至少 2 行（`:798-850`）。
- **滚动条**：列表型模态的滚动条来自 `tui-scrollbar`，在 `content_area` 最右列、宽度 1、高度等于可视行数（`views/picker.rs:2084-2098`）；`SCROLLBARS_HIDDEN` 为真时整体 no-op，minimal 模式启动时置真，改由页脚承担 "↑/↓ navigate" 提示（`render/scrollbar.rs:44-48, 211-229`）。滚动窗口策略是**居中跟随选中项**（`selected - visible/2` 夹到 `max_scroll`，`views/picker.rs:151-169`）；鼠标滚动写入 `scroll_offset`，一旦键盘导航就重置为 `None` 交还自动策略（`:1487-1490, 2004-2012`）。
- **embedded（minimal）变体**：进程级 `AtomicBool` `EMBEDDED` 把整个模态框切成无边框形态，注释明确「minimal 模式永远不会显示浮动模态框」；此时 `modal_width/height = area`，不画边框、关闭按钮与 `Clear` 以外 chrome，标题改成第一行的裸加粗文本，行选中态从「画色带」改成「改前景为 `fuzzy_accent`」（`:25-73, 246-251, 284-307`）。
- **遮罩三种互不相同的机制**：① `Clear` 清底（主流）——只重置弹窗覆盖的 cell，弹窗外内容原样保留、不变暗（`:274-275`）；② 半透明蒙层（少数全屏浮层）——block_viewer 画之前 `dim_area(buf, overlay_area, theme.bg_base, 0.5)`（`app/agent_view/render.rs:3779`），line_viewer 有 `should_dim` 分支且全屏时跳过 dim（`views/file_search/line_viewer.rs:1439, 1465`），混合公式是逐通道线性插值（`render/color.rs:152-157`）；③ 失焦「退后」（recede）——**焦点反馈而非层级遮罩**，`ListOverlay` 在 `focused == false` 时把整个区域朝面板底色混合 0.66（`views/overlay_list.rs:152-155`），`PickerMode` 也把 dim 绑在模式上：`Floating` = "dimmed background and rounded border"、`FullScreen` = "no border, no dim"（`views/picker.rs:104-113`）。`ModalWindowState` 里没有任何 dim/alpha 字段（`modal_window_state.rs:11-34`），从侧面印证第 1 档才是主流。
- **对照样本**：`new_worktree_dialog.rs` 没走 ModalWindow，而是用 ratatui 的 `Flex::Center` 手写居中（`MIN_DIALOG_WIDTH = 50`、`DIALOG_HEIGHT = 5`、内部左右 padding 4），且**小到放不下时会画一行 `[Esc] to close` 作为降级提示**而不是静默消失——少数做了「拒绝渲染但给出可退出线索」的浮层（`:11-31`）。

**prompt 槽位的接管优先级链。** 卡片与列表浮层不画在屏幕上，而是**抢占输入框那块矩形**，顺序在高度计算处就写死了（`app/agent_view/render.rs:806-880`）：`permission card`（`slot_card == Permission`，`:817-829`）→ `question card`（`:830-842`）→ `elicitation card`（`slot_card == McpElicitation`，`:843-855`）→ `/rewind` 浮层（上面三个高度都为 0 且 `rewind_state` 存在，`:856-865`）→ `cancel-turn` 面板（`slot_card == CancelTurn` 且 rewind 为 0，`:866-871`）→ `/jump` 浮层（`jump_slot_taken()` 为假，`:872-880`）。`prompt_height` 用同一顺序取第一个非 0 的高度（`:943-965`），绘制端也用同一顺序 `if … else if`（`:2378 → 2460 → 2772 → 2781 → 2790 → 2794`）——**「高度求解与绘制必须同序」**保证了不会出现「预留了行数但没人画」；槽位接管有全局上限 `max_prompt_height = area.height / 2`（`:806`）。**排队只有 permission 是真队列**：`permission_queue: VecDeque<PermissionViewState>`（`app/agent_view/mod.rs:1327`，初始化 `app/agent_view/session.rs:383`），所有读写只碰 `front()` / `front_mut()`（`key_owner.rs:96-108, 155-159`），因此多个授权请求 FIFO 排队、一次只显示一个；其余模态是「后开覆盖先开」的单槽位语义（例：`Ctrl+P` 打开命令面板时直接 `self.active_modal = Some(...)`，`app/agent_view/input.rs:1127`）。

**嵌套的三种实现手法（不是栈）：**

| 手法 | 例子 | 证据 |
| --- | --- | --- |
| 同一 popup 内换内容（状态搬迁） | `ResetSettingsConfirm` 把 `SettingsModalState` 整体搬进自己的枚举变体，取消时再搬回去，从而保留过滤/滚动位置 | `views/modal.rs:304-313`；`app/modals.rs:2376-2400` |
| 后画覆盖前画 | `/config-agents` 画完 agents_modal 后若 `persona_detail` 存在就再画一层 | `app/agent_view/render.rs:3894-3918` |
| 快照 + 回退 | 命令面板 → 参数选择、DocPicker → DocViewer 把上层状态存进 `previous_palette: Option<PaletteSnapshot>`，Esc 时还原 | `views/modal.rs:331-335, 243-245`；`app/modals.rs:174-180, 278-282` |
| 宿主内部局部浮层 | ManagedConnectorsWait「盖住列表」；`overlay_content_rect` 给出「距 popup 左右各 1 列、从 tab bar 下两行到底部」的内嵌矩形 | `views/managed_connectors_wait.rs:1-5, 126`；`modal_window.rs:1076-1093` |

**焦点排名表 `KeyOwner`**（`app/agent_view/key_owner.rs:38-53`）：枚举为 `{ LineViewer, BlockViewer, Card(BlockingCard), PlanApproval, Pane }`，排名实现在 `key_owner_when_parked`（`:110-132`），逐条 `if/else if`：① `Permission` 卡片（**永远最高**，注释 "a permission request interrupts whatever else is waiting"）；② `LineViewer`；③ `BlockViewer`；④ `PlanApproval`；⑤ 其余卡片固定序 `CancelTurn → Question → McpElicitation`（`:96-108`）；⑥ `Pane`（普通面板 + 全局绑定）。`parked` 布尔量（键盘停在 scrollback）会把卡片与 plan approval 摘出排名，使「卡片还画着但键盘在滚动区」成为可表达状态（`:114-116, 142-153`）。

**`EscStep`：ESC 是一级一级的阶梯**（`key_owner.rs:59-91`），8 个可枚举档位，每档都有给快捷键栏显示的标签（`label()`，`:78-91`）：

| 档位 | 含义 | 用户可见标签 |
| --- | --- | --- |
| `DismissFileSearch` / `LeaveTextInput` | 关掉卡片输入框上的 `@` 文件搜索下拉 / 从卡片文本框退回其选项行 | `dismiss` / `back` |
| `DiscardPatternEdit` / `ClearSelection` | 丢弃半成品的 always-allow 规则编辑 / 取消当前问题的选择 | `cancel` / `unselect` |
| `BackOutOverlay` / `ParkFocus` | 退回 dashboard（卡片保持挂起）/ 把键盘交给滚动区，卡片继续画着 | `dashboard` / `scrollback` |
| `KeepRunning` / `DismissElicitWaiting` | 关掉 cancel-turn 面板，回合与子代理继续跑 / 关掉 elicitation 的等待态 | `keep running` / `dismiss` |

设计意图写在注释里：**「每个在卡片上标注 Esc 的提示都读这张表，每个拥有该键的处理函数也都按它分发，因此快捷键栏不可能承诺一个按键到不了的档位」**（`:55-57`）。`card_esc()` 按当前焦点算出该走哪一档，`handle_card_esc()` 执行（`:167-250`）；`KeepRunning` 的实现特意不用 `ContinueToRun`，因为那仍会取消父回合（`:241-244`）。

**输入派发顺序**（`app/agent_view/input.rs:329` 起，先匹配先返回）：scrollback drag latch（`:335`，拖拽中的鼠标事件先被吞）→ 子代理全屏接管（`:361`）→ `/jump` 抑制期的 Esc 吞掉（`:364`）→ wrap-host 图片粘贴（`:372, :378`）→ Esc+d 组合 → `DumpInputLog`（`:386`）→ `image_viewer`（`:399`）/ `video_viewer`（`:428`）/ `gboom`（`:439`）→ `workflows` 浮层（`:454`）/ `goal_detail`（`:457`）→ `active_modal`（`:488`，**早于 btw、早于卡片**）→ `/btw` 面板（`:501` 起：Esc → dismiss、关闭按钮、悬停、滚动）→ 扩展模态（`:642`）、`persona_detail`（`:646`）、`agents_modal`（`:663`）、`block_viewer`（`:681`）→ `key_owner() == PlanApproval`（`:800`）、Permission 卡片（`:725`）。路由到最上层的方式是每层返回 `InputOutcome`（`Changed`/`Action` 表示已消费，`Unchanged` 表示「我没吃这个键」），穿透规则只有两条例外值得记：`active_modal` 分支**只放行 `ActionId::Quit`**（`registry.lookup(key, When::Always) == Some(ActionId::Quit) → Unchanged`），其余按键全部交给模态自己，即 **Ctrl+Q 是唯一能穿透模态的全局键**（`:490-495`）；`extensions_modal` / `persona_detail` / `agents_modal` / `block_viewer` 则更宽——**任何 `When::Always` 的全局动作都放行**（`:644-688`），所以 `views/extensions_modal.rs:3` 的「Blocks all input until closed with Esc」严格说只在非全局键上成立。模态窗口自身的键盘处理极小：`handle_modal_key` 只认 `Esc → CloseRequested`、`Left/h`、`Right/l`（后两者在 `tabs_focused` 时直接 `Unhandled`，否则按 `fold_info` 返回折叠类结果），其余一律 `Unhandled` 交给各模态内容处理器；**Tab 不由 chrome 处理**，由各模态/Picker 自己实现（`modal_window.rs:899-942`）。

**模态内的焦点循环**以 picker 系为样本：三个焦点区 **tab bar / 搜索框 / 列表**，用 `PickerState` 的两个布尔量 `search_active`、`tabs_focused` 表达（`views/picker.rs:1474-1503`）。搜索框内按 `Tab`/`BackTab` 也换 tab（`:2693-2703`）；搜索框内按 `Up` 若配置了 tab 就交给 tab bar，否则跳列表最后一项，按 `Down` 跳到第一个可选项（`:2717-2746`）；`tabs_focused` 下 `Down`/`j` 回到搜索框（`:2762-2770`），列表底的 `Down` 与列表顶的 `Up` 把焦点交回，注释称之为 "completing the arrow cycle"（`:2758-2762`）；`Left`/`Right` 按 `(active_tab ± 1) % tab_count` 环绕（`:2965-2980`）；行导航有 `j`/`k` 别名（条件 `config.show_search_hint || config.vim_normal_first`，`:2755-2757`）。vim 模式下 `i` / `/` 是唯一进入搜索态的方式，普通可打印字符不输入（`:2994-3001`）；`push_vim_nav_search_hint` 会在页脚补一条 `i search`，非导航态或 vim 关闭时不加（`modal_window.rs:186-197`）。

**其它输入捕获规则**：粘贴（`Event::Paste`）按同一条优先级链单独分发（`handle_modal_paste`，`input.rs:497`），卡片在 `FollowupInput` 焦点下把粘贴路由给输入框、在 `PatternEdit` 下逐字符插入并过滤 `\n\r`（`:787-802`）；鼠标先判关闭按钮 → tab → 可点击页脚 → **点在 popup 之外直接 `CloseRequested`（点击穿透到背景即关闭）**，`Moved` 只更新悬停并返回 `Handled`，最后落到 `Unhandled` 交给内容（`modal_window.rs:981-1026`）；**Ctrl+Q 永远保留**，出现在几乎每一层的最前面（`input.rs:402, 431, 445, 492, 731`），是「卡在某一层出不来」的最后退路。

**关闭 / 取消 / 确认的统一语义**：`Esc` = 关一层，且这一层由 `EscStep` 或模态自身的子模式状态决定——设置模态在 `FilterFocused` / `PickingEnum` / `PickingGroup` / `EditingValue` 四个子模式下**先短路**，只有顶层才走宿主的 `CloseRequested`（`app/modals.rs:399-431`）；MemoryBrowser 同理（`:369-397`）。点击 popup 之外或 `[✗]` 都是同一个 `CloseRequested`，关掉时重建/清空 hit 状态以避免鼠标点到上一帧的幽灵矩形（`modal_window.rs:253-258`、`settings_modal/render.rs:147-155` 的 `reset_hit_rects`）；命令面板与 DocPicker 的 Esc **第一跳不是关闭**而是先清空查询（`esc_clears` 标志），查询已空才关（`app/modals.rs:130-172`）。**不可逆动作按代价分四档**：① 双次按键 + TTL（最轻）——`requires_confirmation` 的动作第一次只装填 `PendingAction`，`PendingAction::TTL = 1000ms` 内再按一次才执行，快捷键栏显示 "press again to {label}"（`app/app_view.rs:460-476`），空闲态 `Esc Esc` 的窗口单独定义为 `ESC_DOUBLE_PRESS_TTL = 800ms`（`:475-476`，与 `actions/defaults.rs:436` 一致）；② 装填 + `y`/`n` 二次确认——会话删除用 `PendingDelete { source, session_id, cwd }` 记住目标（注释解释「即使列表发生位移，`y` 仍绑到正确目标」），`handle_pending_delete_key` 规定 `y` 确认、`n` 取消、**任何其它无修饰键先把装填清掉但仍继续处理该键**（`Disarmed`），且只接受 `KeyEventKind::Press` 且无修饰（`views/session_picker.rs:57-132`）；③ 阻塞式确认对话框——`ModalConfirmation<R>` 把「键 → 结果」写进类型里，一次只允许匹配到的键产生结果（`views/modal.rs:14-44`），`EditConfirmResult` 四态且标签随上下文变化（`save` vs `save & send`、`discard changes` vs `discard & send`，`:60-82`），恢复默认设置的确认**叠在设置模态上**且 `Esc`、`F2`、`Ctrl+,`、`Cmd+,` 全映射到 `Cancel` 而非「关模态」，注释「只有无修饰的按键才算：`Ctrl+Y` 不得触发 Reset」（`app/modals.rs:439-459`）；④ 把危险选项排除出默认路径——`/rewind` 的 `RewindPhase::Confirm` 是**独立的阶段枚举值**而不是模态（`views/rewind.rs:57-78`），"Yes, and don't ask again" 是会写回设置的一等选项，dashboard 覆盖层的 Ctrl+X 明确禁止自动重复（按住键）驱动破坏性动作（`views/dashboard/state.rs:3338-3340`）并用 `CONFIRM_WINDOW` 而非默认 TTL（`app/app_view.rs:480-483`）。反面样本：`app/modals.rs:1718-1719` 留了一条注释——`EditConfirm` 已无绘制分支也不再被布防，理由是「布防它会不可见地捕获所有输入」；**一个没有渲染分支的模态等于键盘黑洞**。

**模态与浮层清单**（`views/modal.rs:180-330` 共 12 个 `ActiveModal` 变体）：

| 名称 | 用途 | 触发 |
| --- | --- | --- |
| `CommandPalette` / `ArgPicker`（`:187-212`） | 模糊搜索所有动作与 slash 命令 / 命令参数选择（model、theme、reasoning effort） | `Ctrl+P` / `?`（`actions/defaults.rs:790-799`）；从命令面板选中带参命令 |
| `SessionPicker` / `DocPicker` / `DocViewer`（`:213-271`） | `/resume` 会话选择（支持深度内容搜索与 `d` 删除）/ how-to 指南列表 / 正文阅读 | `/resume`（`app/agent_view/input.rs:1344`）；命令面板的 HowTo 项 |
| `ShortcutsHelp` / `MemoryBrowser`（`:272-294`） | 按键速查表（可过滤、展开、进详情页）/ `/memory` 浏览器（左列表右预览，可全屏） | `Ctrl+.` / `Ctrl+X`（`actions/defaults.rs:805-817`）；`/memory` |
| `Settings` / `UsageInfo`（`:295-303`） | 设置模态 / 用量与会话信息（多 tab） | `F2` / `Ctrl+,` / `Ctrl+,`(SUPER) / `/settings`（`:843-851`）；`/usage`、`/session-info`、`/context`、context-bar 点击 |
| `ResetSettingsConfirm` / `RememberNoteReview` / `EditConfirm`（`:181-186, 304-330`） | 叠在设置上的「恢复默认」确认 / `#` 记忆笔记预览（Tab 切原始与增强）/ 放弃未保存的队列编辑 | 设置里对某项按重置；提交 `#` 笔记；**`EditConfirm` 已不再布防**（`app/modals.rs:1718-1719`） |
| Extensions / Agents / Persona detail / Feedback / Import Claude | Hooks/Plugins/Marketplace/Skills/Workflows/MCP 六 tab；列出所有 agent 定义；persona 详情与内联编辑（**叠在 Agents 之上**）；反馈编辑器；选择性导入 Claude 设置 | `/hooks`、`/plugins` 等；`/config-agents`（别名 `/agents`）；Personas tab 按 Enter；`/feedback`；`/import-claude` 或欢迎页按 `i` |
| Block viewer / Line viewer / Tutorial / Goal detail / Workflows | 块全屏查看（含搜索、可视选择、复制）；文件与计划预览（带 `[↗][✗]` 按钮与半透明蒙层）；两屏上手教程；目标进度（预算条 + todo + 事件史）；工作流运行列表 | 选中块按 `Ctrl-F`；计划审批预览、prompt 上的文件预览；`/tutorial`；目标 chip（`Esc` 或 `g` 关闭）；overlay 输入路径（`app/agent_view/input.rs:454`） |
| 卡片：Permission / Question / Elicitation / Plan approval / Cancel-turn | 工具授权（选项列表 + followup 输入 + 规则编辑）；`AskUserQuestion` 结构化提问；MCP URL/form elicitation；计划审批（预览关闭时占键盘）；回合运行中询问是否中断 | 代理请求授权 / 提问时；`x.ai/mcp/elicit`；plan 模式提交计划；回合运行中按取消 |
| `/rewind` / `/jump` / `/btw` / New worktree / Managed connectors wait | 回退到某回合（Picker → CancelOffer → Confirm → Executing）；跳到某回合、光标移动即预览；侧问侧答（内容随 Esc 落回滚动区）；新 worktree 命名；等用户完成连接器的浮层（**盖在扩展模态列表上**） | `/rewind`（或空闲空输入框 800ms 内 `Esc Esc`，`actions/defaults.rs:427-436`）；`/jump`；`/btw`；命令面板 New Session in Worktree；MCP tab 的 Ctrl+O / URL 点击 |

`overlay.rs` 里的 `OverlayState` 是**另一类**东西：它服务于可切换的侧栏面板（tracing / todo / 后台任务），语义为快捷键进入时「显示并聚焦」；`Tab` 退全屏并失焦、焦点交回滚动区；`Space` 同上但焦点交给输入框；`Esc`/`q`「退一层嵌套」（全屏时先退全屏，否则整体隐藏）；`Ctrl-F` 切换全屏且**即使在输入条打开时也生效**（`views/overlay.rs:73-142`）。`handle_overlay_key` 与 `handle_overlay_nav_key` 被刻意拆成两个函数，因为调用方需要先判断 `has_input_bar` 再决定是否处理导航键（`:133-156`）。

**小屏与模态相关的部分**（高度阈值见 §4.1）：compact 传导到模态的方式是每个模态自己在 config 里调 `ModalSizing::with_compact(compact)`，参数统一取自 `self.scrollback.appearance().prompt.compact`（例：`app/modals.rs:1762, 2179, 2192, 2248`），而该字段是**派生渲染值**，注释明确「应用可以因终端过矮而强制打开，持久化的用户设置是 `UiConfig::compact_mode`」（`appearance/config.rs:76-80`）。放不下就不画、并把 hit 状态清干净：模态 20×6（`modal_window.rs:253-260`）、`ListOverlay` 在 `height == 0 || width < 10` 不画（`overlay_list.rs:56-58, 90-92`）、`render_btw_panel` 在 `width < 12 || height < 3` 返回（`btw_overlay.rs:219-221`）、settings 在内容区小于 `CONTENT_MIN_WIDTH = 10` 时 `reset_hit_rects` 并返回（`settings_modal/render.rs:152-155`，常量在 `settings_modal/state.rs:28`）。小屏也有用户可用的「放大」入口：memory 模态的全屏变体（`width_pct: 1.0, max_width: u16::MAX, v_margin: 0`，偏好会持久化）、overlay 的 `Ctrl-F`、line_viewer 的 `[↗]`（全屏时主动跳过 dim）。模态自身高度也有弹性：页脚行数随宽度反向增长（窄了要多折行）并由 `shortcuts_rows_needed` 在布局前预测，tab bar 也在窄屏折成多行（`modal_window.rs:363-365, 438-478, 591-614`）。

### 4.4 输入区（prompt widget）

#### 本节要点

- 输入区是「**可复用组件 + 独立 textarea crate**」两层：`prompt_widget` 明确不管焦点管理（Esc、Tab）也不画 accent 线/选择框；同一组件被 agent 视图、welcome 屏、dashboard dispatch 框、权限/提问面板复用。
- 视觉是一个四边盒：顶边可内嵌会话标题（草稿 stash 时显示 `Stashed`）、底边内嵌 `模型 · flags` 与 `multiline`，前缀默认 `❯`（宽 2 列）。
- 自动增高**没有组件级行数上限**，上限由调用方给——全屏 agent 视图是 `area.height / 2`，welcome 屏是 `(内容高/2).min(列高)` 且起始 3 行。
- 光标是三层机制叠加：终端光标默认隐藏、位置由渲染结果回传、颜色用 OSC 12 改成主题 `accent_user`；**形状不是按模式切换的**，而是启动时一次性策略 `[ui].cursor_blink`。
- 图片与粘贴都是**与文本同流的原子 chip**（`[Image #N]`、`[Pasted: N lines]`），不是附件栏；chip 是「显示文本 ≠ 缓冲文本」的通用机制。
- queue pane 最多 3 行、只发意图不改业务状态；外部编辑器走「临时文件 0600 → 子进程接管 TTY → 读回校验并剥末尾换行 → stale/非法一律保留原草稿」。

**三层结构**：组件层 `views/prompt_widget/mod.rs`（3820 行，负责 chrome 绘制、提交路由、chip 元素、图片、下拉协调、ghost text）；编辑内核 `crates/codegen/xai-ratatui-textarea/src/textarea.rs`（3482 行，负责文本缓冲、wrap 缓存、撤销栈、kill buffer、选区、鼠标、滚动条）；表现层原语 `crates/codegen/xai-grok-pager-render/src/`（`key` 键位匹配宏、`prompt_images`、`theme`、`render` overlay 几何）。模块注释明确划了责任边界：**它不管焦点管理（Esc、Tab）也不画 accent 线/选择框，那些由调用方负责**（`views/prompt_widget/mod.rs:3-5`）；拆分让同一组件被 agent 视图、welcome 屏、dashboard dispatch 框、权限/提问面板复用（`:240-272` 的 `PromptStyle::overlay()` / `inline(bg)`）。同目录另有 `image_state.rs`（199 行，图片 chip 重绑定）与 `tests.rs`（5273 行，测试量是主体的 1.4 倍）。

**视觉与状态。** `PromptStyle::default()` 的几何参数为 `vpad_top = 1, chrome = true, chrome_pad_left = 2, chrome_pad_right = 1, show_prefix = true, show_accent_line = false, show_borders = true, image_preview = true`（`:216-238`）。按 `PromptWidget::draw`（`:2971-3474`）的绘制顺序得到：

```
 ╭─────────────────────────── 会话标题(可省) ───────────────╮   ← 顶边（vpad 行，show_borders 时）
 │                                                          │
 │  ❯ 输入文本在这里，长行自动软换行…                        │   ← ❯ 前缀(2 列) + TextArea
 │    续行                                                   │
 │                                    [Pasted: 12 lines]    │   ← chip 与文本同流排版
 ╰───────────────────────── grok-3 · plan ──────────╯        │   ← 底边内嵌 info 行
   ↑ 若 show_accent_line，最左列整列画 ┃
```

- 顶边 `╭─╮`（`:3049-3054`）、侧边 `│`（`:3330-3335`）、底边 `╰─╯`（`:3356-3361`）；accent 竖线用 `crate::glyphs::accent_bar()`，默认关闭且关闭时该列归还给内容（`:176-178, 3001-3010`）。前缀默认 `❯`（`PREFIX_WIDTH = crate::glyphs::PROMPT_ARROW_WIDTH`，`:693-694`），三种覆盖：历史搜索时 `? `、模式覆盖（bash `! ` 黄、remember `# `）、普通 `❯`（`:3084-3104`）。
- 未聚焦时对盒内内容整体向背景色衰减 0.66，边框色切到 `theme.prompt_border`（`:2990-2994, 3378-3389`）。顶边可内嵌会话标题（`style.title`），与底边 info 行同款样式；草稿被 stash 时标题为 `Stashed`，再与 `/rename` 标题用 `·` 拼起来（`:3060-3081`；`app/agent_view/prompt_stash.rs:41-58`）。底边 info 行左右两段：左 `模型名 · flag1 · flag2`（flag 由 `mode_flags()` 生成，plan 用 `accent_plan`、非 Ask 的权限用灰或蓝），右 `multiline` 与 `usage_warning`（`:291-351, 3489-3594`）。
- 占位符默认文案写死在 `draw` 里：`"Build anything"`，只在文本为空、且（未聚焦或 `placeholder_when_focused`）且没有语音 interim 时显示，并按输入区宽度截断（`:3310-3321`）。主 composer 默认「聚焦时隐藏占位符」（`placeholder_when_focused = false`），feedback 框保留；remember 模式文案是 `"Save a memory note... (Shift+Enter for multiline)"`，多行模式下换成 `"... (Enter for newline, Shift+Enter to save)"`（`app/agent_view/mod.rs:370-381`）。

**光标三层机制。** ① 终端光标默认被隐藏：初始化时 `execute!(stderr, …, cursor::Hide)`（`app/mod.rs:1503-1508`）。② 位置由渲染结果回传：`draw` 返回 `PromptRenderResult{ cursor_pos, post_flush_escapes }`，`cursor_pos` 只在 `style.focused` 且没有「空输入 + 语音 interim」时给出，否则 `None`，该值一路上抛到 `app/app_view.rs:4833-4846` 由终端写入（`:3391-3399, 404-411`）。③ 颜色用 OSC 12：`cursor_color_escape()` 产出 `\x1b]12;rgb:RR/GG/BB\x07`，取色为当前主题的 `accent_user`，启动时 `apply_cursor_color()` 写入，退出/切主题用 OSC 112 还原，并用 `CURSOR_COLOR_APPLIED` 原子开关保证「只撤销自己画过的」（`theme/mod.rs:559-596`）；`terminal` 主题（不涂背景）特意不动光标颜色（`docs/user-guide/06-theming.md:26, 141`）。**形状不是按模式切换的**：`[ui].cursor_blink` 三态 → `CursorStylePolicy::{Inherit, ForceBlinking, ForceSteady}`，未设置时**不发任何转义**、保留终端自己的形状/闪烁，强制时分别发 `EnableBlinking + SetCursorStyle::BlinkingBlock` 或 `DisableBlinking + SetCursorStyle::SteadyBlock`（`app/mod.rs:1427-1446, 1509-1523`）。**vim / 多模式下的光标差异：未找到证据**——输入区没有 normal/insert 两态，`/vim-mode` 只改 scrollback 键位，注册表的 `key_for_mode` 也只对 `When::ScrollbackFocused` 的裸字母键做替换（`slash/commands/vim_mode.rs:14-16`、`actions/mod.rs:448-463`）；多模式真正改变的是**前缀色与文案**（Normal/Bash/Remember 三态，`app/agent_view/mod.rs:355-388`）。

**高亮只覆盖命令与 chip**（没有通用语法高亮，也不按语言/任意路径着色）：`/命令` 青色高亮取 `theme.accent_skill`，嵌入式最小模式退化为 `text_primary`；着色条件严格——行首 `/` token 在「下拉打开或命令已识别」时着色，文本中段 token 只在被真正识别（hoist）或是 skill/plugin 提及时着色（`:3137-3165, 3212-3222`，着色函数 `paint_slash_token_highlight` `:2946`）。另有参数占位 ghost（命令声明 `arg_placeholder` 且参数未输入时，在光标后以 `theme.muted()` 画，`:3180-3194`）与 `/命令` 不完整后缀 ghost（`:3196-3210`）。

**chip 是「显示文本 ≠ 缓冲文本」的通用机制**：`@文件引用` 被接受后不是普通文本，而是 `KIND_FILE_REF` 原子元素，缓冲区文本是 `@path`，显示由 `file_ref_display()` 生成（`@` 与 `:` 灰、路径用 `theme.path`、行号用 `text_primary`，支持 `@foo/bar.rs:10-12`，`:2076-2095, 3620-3633`）；粘贴 chip 在行数 ≥ 4（compact ≥ 2）或字节数 > `PASTE_CHIP_DISPLAY_BYTES = 10_000` 时生成，标签 `[Pasted: N lines]` / `[Pasted: 1.0 MB]`（十进单位，`:47-48, 2306-2325`），**再粘一次相同内容 = 就地展开**该 chip 为全文（`:2288-2299`）；图片 chip 是 `[Image #N]`，样式为方括号用 `theme.paste_dim`、内容用 `theme.paste_fg`、背景 `theme.paste_bg`（`:3760-3810`）。`TextArea` 为每个元素保存可选 `display: Line`，渲染时按显示宽度重算 x 坐标；显示超宽时截断且**保留右括号**，于是 `[Pasted ~10 lines]` 变成 `[Pasted ~1…]` 而不是 `[Pasted ~10…`（`xai-ratatui-textarea/src/textarea.rs:3242-3307, 3408-3446`）。多个元素重叠一行时按显示宽度逐段推进，这也是「文本里的 `[Image #1]` 不会破坏命令解析」的原因——斜杠系统看到的是剥离了所有元素占位符的干净文本（`:3654-3695`）。

**自动增高与上限。** 高度公式（`PromptWidget::desired_height`，`:1601-1627`）为 `height = vpad_top + textarea.desired_height(text_width) + info_block`（`info_block` 有 info 行则 1），再 `clamp(min.min(max_height), max_height)`，其中 `min = vpad_top + 1 + info_block`。上限不是组件常量，而是调用方给的 `max_height`：全屏 agent 视图 `max_prompt_height = area.height / 2`（未聚焦且开了 `collapse_unfocused` 时折叠到 `vpad_top + 1 + info_block`，`app/agent_view/render.rs:806-814`）；welcome 屏 `PROMPT_HEIGHT = 3` 是起始高度，`prompt_max_height = (content_height / 2).min(column_fit).max(PROMPT_HEIGHT)`（`views/welcome/mod.rs:177-194`）。两个抑制抖动的细节：**浏览历史时冻结为 1 行**（`history_search.is_browse()` 时 `text_height = 1`，否则在不同高度的历史条目间上下移动会导致每按一次键布局就重排，`:1614-1621`）；**内容超出可视高度时出现滚动条**（`TextArea.show_scrollbar` 默认 `true`，启用时最右一列留给轨道、正文按 `width - 1` 换行，`textarea.rs:224-237, 3165-3223`）。行高由 wrap 缓存计算：宽度变化才重算；无自定义显示的元素走 `textwrap` 的 `WrapAlgorithm::FirstFit`，有 chip 元素时改走 `element_aware_wrap_ranges`——**把元素当作原子单位**，放不下就整体挪到下一行，元素内部的换行符不产生新视觉行（`:2885-2930`）。

**多行编辑。** 软换行是显示层的、缓存为字节区间 `Vec<Range<usize>>`（`WrapCache{width, lines}`，`:250-254`），上下移动按**视觉行**走并带 sticky 列 `preferred_col`（`:2375-2501`）；Home/End 默认按视觉行，Ctrl+A/Ctrl+E 按逻辑行（`:2503-2533`）。硬换行只有几条来源：`Shift+Enter`/`Alt+Enter`（`crate::input::is_mod_enter`）、Kitty 送达的 `SUPER+Enter`、Apple Terminal 的 CoreGraphics 修饰键补救、裸 `Ctrl+J`/`Ctrl+M`、以及**反斜杠续行**（光标前是 `\` 时按 Enter 用换行替换它）（`:1813-1819, 2136-2188`；`textarea.rs:2005-2014`）。裸 Enter 是**提交**：`route_enter` 判定顺序为「文件搜索下拉打开 → PassThrough」「Apple Terminal 换行修饰键 → 插入换行」「反斜杠续行 → 插入换行」「裸 Enter → Submit」（`:2157-2188`，语义枚举 `:92-103`）。`/multiline`（输入框聚焦时 `Ctrl+M`）把 Enter 与 `Shift+Enter` 语义对调，并在 info 行右侧显示 `multiline`（`ActionId::ToggleMultiline`，`app/agent_view/prompt.rs:486-529, 598-600`）。

历史条目就是 `HistoryEntry { text: String }`——多行提示词是一个含 `\n` 的字符串，不按行拆分；模糊匹配在后台 nucleo 线程做，前端快照最多 `MAX_RESULTS = 100` 条（`views/history_search.rs:4-28, 55`）。两个入口共享同一面板（`Mode`：`Search` / `Browse`）：`/history` 是 Search（输入框当过滤词，导航只高亮，Enter/Tab 接受），空输入框按 `↑` 是 Browse（每移动一格把整条历史灌回输入框，输入即「脱离」变成编辑，到底部最新一条再按 `↓` 关闭）（`:264-301, 381-395`；`app/agent_view/prompt.rs:709-737`）。Browse 打开时选中项吸附在最新一条（`stick_to_bottom = true`），用户一旦导航就清除（`:286-290, 404-418`）。`↑` 的完整路由：仅当模式为 Normal、输入为空、无文件搜索时生效；**队列非空时优先切焦点进 queue pane**，否则才开历史面板；`↓` 永远不会打开历史面板（`app/agent_view/prompt.rs:407-424`）。另有独立的 **stash**（`Ctrl+S`/`Alt+S`）：整个草稿（含图片、chip、`!` 模式）存一个槽位，空输入框按同一键恢复；被替换掉的旧草稿**只有文本**进 `↑` 历史（`app/agent_view/prompt_stash.rs:1-26`）。

**撤销是快照式**：`UndoEntry{text, cursor, elements}`，栈深上限 **100**（`UndoState.max_depth`，`textarea.rs:269-330`）。分组规则（`MutationKind`，`:279-293, 2096-2126`）：连续同类输入合并为一步，`Kill`/`Element`/`Replace` 永远单独成步，**光标跳变也断开分组**（比较 `last_cursor`）；宿主驱动的 `set_text` 故意留一个检查点（误替换可撤销），`clear_history()` 才是硬重置。键位：Ctrl/Cmd+Z 撤销、重做为 Ctrl+Shift+Z / Alt+Z（为把 Ctrl+Shift+Z 送成 Ctrl+Z 字节的终端准备）/ Ctrl+R（`:2023-2055`）。chip 也进撤销快照（`elements` 一起存），图片则另有 `image_undo_stash` 在撤销后重挂（`:656-666`；`image_state.rs:30-40`）。**kill / yank 是单槽缓冲，不是 kill-ring**：`TextArea` 只有一个 `kill_buffer: String`，没有环、没有 append 语义（`:184-186`）；kill 命令为 Ctrl+K（到行尾）、Ctrl+U（到行首）、Ctrl+W（空白分词向后删）、Alt/⌘+D 与 Alt+Ctrl+H（向前/向后删词），`kill_current_line` 在空行上吞掉前一个换行以合并行（`:2290-2339`；`editor_keys.rs:46-74, 129-155`）；Ctrl+Y 是 yank，且**撤销不恢复 `kill_buffer`**（注释明确写了 yank 与 undo 分离，`:2092-2093`）。

其它编辑行为：选区来自鼠标拖拽/双击/三击、Shift+方向键扩展，`selection_style` 默认 tokyonight 蓝底 `rgb(49,62,115)` + 亮前景 `rgb(192,202,245)`（为在任何终端配色下都可读，`:188-208`）；`Cmd+A` 全选被**显式限制在 Ghostty**（其他终端的 Cmd+A 属于终端自身的 Select All，不该抢），且需用户在 Ghostty 里写 `keybind = cmd+a=unbind`（`:133-138, 1861-1873`）；`Ctrl+C` 在文本非空时清空草稿并返回 `Edited`，空则 `Ignored` 交给上层（`:1821-1829`）；粘贴分流——`Ctrl+V`/`Cmd+V` 被拦截走 `handle_paste()`（否则多行内容会被内联插入而不是变成 chip），`Ctrl+Shift+V` 是「内联粘贴」不做 chip（`:1831-1859`）；`tab_width = 4`，显示与插入时都把 `\t` 展开成空格、由外观设置热同步（`textarea.rs:244-247`）；鼠标滚轮可临时脱离光标独立滚动（`scroll_override`），一旦光标移动就弹回，拖拽到区域外会节流加速滚动（`:211-223, 354-376`）。

**附件：图片是与文本同流的原子 chip。** 单条提示最多 `IMAGE_CAP = 10` 张图，后端拒绝任一边 < 8 px，插入前先校验并给出可直接显示的文案（`:2338-2361`）。插入过程是 `begin_undo_group()` → 插入 `KIND_IMAGE` 元素 → 补一个空格 → `end_undo_group()`，即**一次撤销就整体消失**，同时记录 `post_insert_image_preview` 让预览立刻出现（`:2363-2395`）。缓冲文本是规范的 `[Image #N]`，chip **不带路径**，路径只出现在预览浮层里。预览触发条件（任一）：光标在 chip 上或正好在其右边界、鼠标悬停该 chip、或刚插入且光标未移动（`:2594-2607`）；渲染走 `render_image_overlay`，返回**flush 之后**要写的终端转义（Kitty/iTerm2 内联图片），且每帧若不重画就主动清掉 overlay 的 ID 1（`:3441-3468`；几何常量 `MIN_BOX_WIDTH` / `MIN_META_BOX_HEIGHT` / `MIN_PIXEL_BOX_HEIGHT` 见 `render/image_overlay.rs:24-33`）。「chip 文本变回纯文本」被显式兜底：yank、撤销、纯文本粘贴后若 `[Image #N]` 还在而记录仍在 `images`/撤销暂存里，`rechip_orphan_image_placeholders` 会重新注册 chip；找不到记录的占位符就作为纯文本发送并弹 toast（`image_state.rs:1-40`）。「有附件」还会挡住外部编辑器：`ATTACHMENT_MESSAGE = "External prompt editing is not available while the draft has attachments."`（`app/external_editor.rs:12-13`）。

**排队消息（queue）的界面。** queue pane 是独立的 `ListPane`，**只在非空且可见时占位**（`is_visible()` = `overlay.visible && !entries.is_empty()`，`views/queue_pane.rs:1-5, 583-585`）：

- **高度上限 3 行**：`MAX_QUEUE_HEIGHT: u16 = 3`，`desired_height() = entries.len().clamp(1, 3)`，隐藏或为空返回 0（`:348-349, 599-606`）。**与输入框的空间竞争**：面板最多吃 3 行、属于输入框上方的弹出层；选中行若为多行，还会在 overlay 区域画全文预览（`:1011-1021`）；最小模式另有 dock 布局用 `queue_body_rows` 分配队列区域（`views/dock/layout.rs:73`、`views/dock/mod.rs:153`）。
- **行内容**：前缀是 1-based 位置号 `#N`（灰色），内容取**第一个非空行（trim 后）**，多行提示尾部加 ` (+N lines)` / ` (+1 line)`；前缀宽度按最大位置号动态算 `2 + digit_count(max_pos)`（`:104-134, 182-200, 296-305, 868-875`）。**三种动作按钮**右对齐从右往左摆 `[cancel]`、`[edit]`、`[Send now]`，只在**鼠标悬停的那一行**或**面板聚焦时的选中行**上绘制（悬停优先），且当一行放不下 `[Send now] + [edit]` 时会主动放弃 `[edit]` 保住 `[Send now]`；按钮 rect 每帧重绑、悬停状态跨帧保留（`:351-416, 921-1009`）。
- **键盘**（`:608-657`）：interject 键 → `ForceInterject`；`x`/`Delete`/`Backspace` → 删除；`e`/`Enter` → 编辑；`Shift+J` / `Shift+K` → 与下/上相邻行交换。所有动作先过 `capabilities`（`can_delete`/`can_edit`/`can_reorder`/`can_send_now`），不可编辑的行尾部标 ` · protected`（`:90-91`），删除后选中邻居而不是跳回顶部（`:700-712`）。面板**从不直接改业务状态**，只发 `QueueEvent` 意图，由 `AgentView::handle_queue_key` 落到 `pending_prompts`（`:331-346`）。数据有两个来源，删除/重排按 origin 路由：本地 `pending_prompts`（Local）与服务器权威共享队列（Server，走 `x.ai/queue/*`），server 行的 id 是其 `prompt_id` 的稳定 hash 并置最高位以免与本地自增 id 冲突（`:33-50, 80-88`）。
- **编辑流程**（`app/queue_edit.rs`）：进入时把当前草稿 stash 一次（`stash invariant` 只在 `enter_queue_edit` 设置、只在退出时 `take()` 一次），提示模式切到 `PromptMode::EditingQueued{id, original, server_id, kind}`；`Shift/Alt+Enter` 仍是插换行、裸 Enter 保存；乐观队列行（enqueue RPC 未确认）按下编辑会提示 `"Still queueing, try again in a moment"`（`:16-18`）；脏草稿时切换面板会被挡住，且**不会**去打开一个「画不出来却吞输入」的确认弹窗（`:4-8`）。相关联的另一处编辑是**就地编辑历史用户消息**（双击/Enter 进入、Enter 提交会 rewind 后重发）——总开关 `INLINE_EDIT_ENABLED: bool = false`，**功能当前是关闭的**，它复用同一套 `TextArea` + `TextAreaState`（`app/inline_edit.rs:1-6, 25-45`）。

**外部编辑器（`$EDITOR`）集成。** 入口：最小模式绑 `Ctrl+G`，全屏 TUI 走 `/edit-prompt` 或命令面板里的 **Edit Prompt in External Editor**（`docs/user-guide/03-keyboard-shortcuts.md:259, 273`；`actions/mod.rs:102`、`actions/defaults.rs:20`）。流程（`app/dispatch/external_editor.rs:8-41` + `app/external_editor.rs` + `app/event_loop.rs`）：`dispatch_edit_prompt_external` 先检查「已有 `pending_editor` 就直接返回」「语音进行中 / 粘贴探测未完成 / 有附件 → 直接拒绝并给出文案」，再构造 `PendingEditorRequest::PromptDraft{agent_id, original_text}`；`prepare()` 做 revalidate（可能被别的输入面抢走）→ 解析编辑器 → 写 `grok-prompt-<uuid>.md`；`suspend_for_child()` 让子进程接管 TTY、输入读取暂停（超时会 requeue 并报「等待中」）；`finish()` 成功则读回并校验，失败 / 非零退出 / 非法 UTF-8 / 超 4 MiB / 草稿已被改动都**保留原草稿并提示**。

- 编辑器解析顺序 `$VISUAL` → `$EDITOR` → `vi`，用 `shlex` 切分（允许带引号的参数），解析失败提示 `could not parse $VISUAL or $EDITOR`（`:123-141, 474`）。临时文件 `std::env::temp_dir()/grok-prompt-<uuid>.md`，unix 下 `mode(0o600)`，`Drop` 里删除（`:72-121`）。
- 读回契约：上限 `PROMPT_EDITOR_MAX_BYTES = 4 * 1024 * 1024`；非 UTF-8 拒绝；**剥掉编辑器自动追加的那一个末尾换行**——判定依据是「原草稿本来就不以 `\n` 结尾」，所以草稿自己的换行原样保留（`:11, 97-110, 289-301`，测试 `:600-646`）。**stale 保护**：编辑器期间草稿被改过就丢弃编辑器结果并提示 `"The draft changed while the external editor was open; the newer draft was kept."`（`:29-30, 313-335`）。
- 成功路径 `apply_prompt_text()`：`history_search.deactivate()` → `set_text` → **`clear_history()`**（外部编辑器写入是一个新的起点，旧撤销栈作废）→ 光标移到末尾 → `refresh_slash()` → 清空 prompt suggestion（`:337-347`）。四种拒绝理由（附件 / 语音 / 粘贴处理中 / 输入面已被占用）各有一条用户可见文案（`:12-19`）；最小模式下失败信息 push 进 scrollback，全屏下走 toast（`:244-260`）。挂起/恢复由 `suspend_for_child` + `restore_after_child` 处理，并有超时重试与「正在等待」提示（`app/event_loop.rs:769-840`）。

**提示语 / 建议 / 幽灵文本。** 两种 ghost 都在文本之后渲染、**从不修改文本**：① shell 补全 ghost（`SuggestionController`，ACP `x.ai/suggest`）——打字去抖后请求，带 generation 丢弃过期响应，命中前缀时本地裁剪 ghost（progressive matching）而不发请求（`views/suggestion_controller/mod.rs:1-6, 34-46, 849-853`）；② 下一句提示 ghost（`PromptSuggestionController`，turn 结束后问 shell `suggestPrompt`）——同 generation 机制，**可见性每帧从当前文本推导**，不按键维护状态机（`views/prompt_suggestion.rs:1-14, 32-70`）。两者都画在光标处、用 `theme.ghost_text_style()` 并截断到行尾（`:3401-3426`）；shell ghost 额外要求**光标必须在文末**、且没有 slash 高亮/内联 ghost（`:3404-3415`）。预测 ghost 的门：`prompt_suggestion_active`（每帧由 AgentView 在绘制/按键前设置，turn 运行中、bash/remember 模式、编辑队列行时为 false）→ 没有 shell ghost → 没有文件搜索/历史面板 → 光标在文末 → 没有 slash 活动（`:807-825, 651-654`）；文本不匹配时 ghost 直接消失、清空后回来，按 Esc 在空输入框上会 dismiss 直到下一条建议（`views/prompt_suggestion.rs:75-102`；`app/agent_view/prompt.rs:374-396`）。接受方式：预测 ghost 用 `Tab` 或 `→`（`→` 在文末本来就是空操作，沿用 fish/zsh autosuggest 惯例），且**优先于动作注册表**，以免 Tab 被抢去切到 scrollback（`app/agent_view/prompt.rs:342-373`）；shell ghost 支持整词或整段接受（`AcceptMode`，`:790-800`）。打断保护：任何下拉/面板打开时 `any_dropdown_open()` 为真，临时提示条（ephemeral tip）会主动让位；另有按配置开关的上下文提示（undo 提示 / plan 关键词提示），默认两个都关时整段 per-keystroke 扫描被跳过（`:860-867, 1682-1691`）。

**本节的不确定项（按上游原文保留）**

- `ModalWindow` 边框的「accent 色」说法：模块注释写 "accent-colored square border"，代码用 `theme.gray_dim`——**以代码为准**，引用视觉描述时建议写「灰色直角边框，标题加粗」。
- `render_modal_window` 的 `Clear` 是否算遮罩：代码只重置被覆盖的矩形，**未找到**「模态打开时整屏变暗」的证据，除非该浮层单独调用 `dim_area`。
- `ActiveModal` 变体数量：`views/modal.rs:180-330` 共 12 个，未在 `src/views/` 之外找到另一份并行的「模态枚举」。
- `extensions_modal` 的 `Enter`/提交类确认语义：只核到它用 `ModalWindowState::with_tabs` 与「Esc 关闭」，其 tab 内部的具体确认流程在 8000 行文件里未逐条通读，**未找到**可引用的统一约定。
- 统一主题/常量表：`modal_window.rs` 内的 `footer_lines`、`v_pad` 等没有集中常量，都是各模态各自字面量；`ModalSizing::medium()` 存在但**未找到生产调用点**（grep `medium()` 于 `xai-grok-pager/src` 无命中）。
- 输入区的「vim / 多模式光标差异」：**未找到任何**按模式切换光标形状或颜色的代码，也没有 normal/insert 两态。
- 任意文件路径的通用高亮：除 `@` 引用 chip 之外未见实现；`@` 补全下拉行内的高亮属于下拉组件而非输入文本。
- 占位符 `"Build anything"` 之外的默认值、以及 banner/tip 与输入框的具体遮挡规则没有逐帧验证（`any_dropdown_open()` 的让位语义只读到公开方法层）。
- agent 主视图的横向降级：**未找到证据**；横向降级只发生在 dashboard（`MIN_DASHBOARD_WIDTH = 40`）、timeline rail（`MIN_TERMINAL_WIDTH = 60`）与 welcome 屏。

### 4.5 状态面：status bar / status line / context bar

*来源专题：12/13/14/15；路径照抄上游，`src/…`、`views/…` 指 `crates/codegen/xai-grok-pager/` 下的同名路径。*

#### 本节要点

- fullscreen 的 agent 视图共 5 个**屏幕状态面**：status bar（顶部，固定 1 行）、scrollback、turn_status（prompt 上方 1 行，按需出现）、status_line（prompt 之下 shortcuts 之上，0–5 行）、shortcuts bar（底部，固定 1 行）；另有 `/usage`、`/queue`、`/tasks` 三个提交进 scrollback 的纯文本块（`views/agent.rs:181-381`、`app/status_blocks.rs:1-14`）。
- status_line 是**唯一用户可配置**的状态面（`[ui.status_line]`）：只有 5 个键、6 个内置 item，代码里**没有模板引擎**，表达力只有「排序内置段」与「用户脚本 stdout」两条路（`xai-grok-status-line/src/config.rs:18-34`、`views/status_line/segments.rs:66-118`）。
- 空间不足时的取舍分四层（段内列上限 → 行内 elide → 行数回砍 → 整面 `Off`）；**先牺牲 status line 自己的行**，prompt 与 shortcuts 的 1 行永不被挤掉（`views/agent.rs:134-135, 229, 246, 281-298`）。
- 上下文用量有**三套互不通用的口径与阈值**（状态栏芯片 `8.5K / 1.0M`、status line `{pct}% ctx`、`/context` 的 100 格分类条）；成本有 `$0.005` 的显示地板（`views/context_bar.rs:16-41`、`views/status_line/segments.rs:11, 18`、`scrollback/blocks/context_info.rs:31-90`）。
- 运行态**不在**状态栏也不在 status line，而在独立的 `turn_status` 行：空闲且无 watcher 时整行消失；等待用户回答时 spinner 换成脉冲 `◆` 并隐藏阶段计时（`views/turn_status.rs:365-395, 725-735`）。
- 未找到证据：`credit_bar_line_for_session` 在当前快照没有生产调用点（不要把「状态栏有 Credits 芯片」当作现状）；手册称 repo-local `.grok/config.toml` 不能设 `[ui.status_line]`，但在 `xai-grok-config` / `xai-grok-shell/src/config` 里**没有找到**对应的过滤实现（`views/credit_bar.rs:231`、`render.rs:1387`）。

**纵向排布（fullscreen，`AgentViewLayout::compute`，`views/agent.rs:181-381`）**

```
① status_bar     固定 1 行：左 = branch / worktree 徽标 / sandbox profile + 标题 + cwd
                右（右对齐）= AgentStatusBar 芯片组，三段宽 " │ " 分隔
  [可选] tasks / catalog / todo 面板（各自前置一行 pane_gap）
  status_gap（top_vpad > 0 时为 1 行）
② scrollback     唯一 Constraint::Min(5)——全屏唯一会被压缩的行
  [可选] /btw、queue、③ turn_status(1 行)、banner、plugin CTA、follow-up、dock
  prompt_gap → [voice_recording 1 行] → prompt(N 行)
  shortcuts_gap（仅当 bottom_vpad > 0 且 status line 关闭时为 1 行）
④ status_line    0–5 行，默认 0 行；压栈前先夹 min(配置值, 总高 − 其余所有行)
⑤ shortcuts      固定 1 行
```

`status_line_height` 的夹取注释把理由写得很直白：**一个太长的脚本应该丢自己的行，而不是让 prompt 或 shortcuts 丢行**（`views/agent.rs:134-135, 289-290`）。

**状态面对照表**

| # | 名称 | 屏幕位置 | 内容项 | 可配置性 |
| --- | --- | --- | --- | --- |
| ① | status bar（芯片行） | 最顶行，固定 1 行 | `bg_tasks` / `plan` / `goal` / `mcp` / `workspace_mode` / `context` / `switcher` / `dashboard` / `link_url`，由运行时状态决定是否 `push`；左侧另画 branch/worktree/sandbox/cwd 与 Upgrade CTA | 用户不可配置 |
| ② | scrollback | 中部，最小 5 行 | 会话流；也承载 `/context` 的 `ContextInfoBlock` 与 `/usage` 的用量块 | 无 |
| ③ | turn_status | prompt 上方 1 行，按需 | spinner + 活动文案 + 阶段计时 + turn 计时 + token 数 + `[stop]`/`[↓]` | 无（`should_show` 决定） |
| ④ | status_line | prompt 之下、shortcuts 之上；0–5 行 | `builtin` 六种 item，或用户脚本 stdout（≤5 行） | **唯一可配置**：`[ui.status_line]` |
| ⑤ | shortcuts bar | 最底行，固定 1 行 | 当前 key owner 的快捷键提示，`compact` 模式截断 | 由 `key_owner()` 与 action registry 决定 |

（`views/agent.rs:181-381`、`views/agent_status.rs:89-130`、`app/agent_view/render.rs:1403-1475`。）

**status line 的两半契约**。`xai-grok-status-line` 是个 leaf crate：`config` 是用户写的、`context` 是 agent 发给客户端画的（`xai-grok-status-line/src/lib.rs:1-4`）。

- 配置键：`type`（`builtin`/`command`/`disabled`，默认 `disabled`，`off`/`none`/`hidden` 为别名）、`command`（`type = "command"` 时必填）、`items`（默认 `["cwd","model","context"]`，非法项逐项记录而非整表拒绝）、`padding`（上限 16）、`refresh_interval`（钳到 1–86400 秒，**只对 command 生效**）（`config.rs:145-187, 217-221, 288-308`）。
- 6 个内置 item 与取不到数据时的行为：`cwd`（列上限 40，无路径段则整段消失）、`model`（30）、`context`（`{pct}% ctx`，缺 `used_percentage` 则消失）、`cost`（`$X.XX`，**低于 `$0.005` 不画**，免得显示误导性的 `$0.00`）、`turn-timer`（`secs == 0` 不显示）、`session-name`（40，client 未填则消失）；段间分隔符固定 `SEGMENT_SEPARATOR = " │ "`，`builtin` 行永远只占 1 行（`views/status_line/segments.rs:9, 14-18, 72-117`、`views/status_line/mod.rs:158`）。
- payload 侧 schema 版本 `STATUS_LINE_SCHEMA_VERSION = 1`，两条硬规则：**取不到的值是 `None` 而不是 `0`**，字段一律 snake_case（改名会静默打断所有读它的脚本）（`xai-grok-status-line/src/context.rs:5-14`）。`session_name` 与 `trigger` 由**客户端**盖章；`cost.total_duration_ms` 从本进程挂载会话开始计时，不是会话创建时（`context.rs:25-28, 80-83, 141`）。
- 组装点是 `build_status_context`（模型/窗口来自 sampling config、token 来自 `try_get_estimated_total_tokens`、成本来自 usage ledger、git 信息在 blocking 池上跑 `git2::Repository::discover`），读失败就留 `None`——注释写明这会渲染成 `0% ctx`（`xai-grok-shell/src/session/acp_session_impl/status_line.rs:117-207`）。下发走瞬态通知 `XaiSessionUpdate::SessionStatus`，由 `run_status_emitter` 单循环驱动、`Notify` 把突发合并成一次构建（`xai-grok-shell/src/extensions/notification.rs:669`、`status_line.rs:275-336`）。
- 自定义命令的边界：客户端在 `initialize` 报能力位 `x.ai/statusLine`（缺省即关闭整条管线），会话级元数据 `clientStatusLine` 由 leader 注入 `session/new|load|resume`；`["ui","status_line"]` 整表列入 `PATCH_STRIP_PATHS`，远程/campaign/version 补丁不得注入，测试用 `command = "curl evil"` 钉住这条（`xai-grok-status-line/src/lib.rs:11-17`、`acp/mod.rs:458-460`、`xai-grok-config/src/config_override.rs:84-87, 313-325`）。
- 节流与生命周期常量（`app/status_line.rs`、`app/status_line/command.rs`）：`EVENT_DEBOUNCE = 300 ms`（防忙回合每帧跑脚本）、`MIN_REFRESH_INTERVAL_MS = 100 ms`、`ABANDON_AFTER = 30 s`（含「≥ 2× 命令超时」的 const 断言）、`REFRESH_FAILURES_TO_PAINT = 3`、`COMMAND_TIMEOUT = 10 s`、`MAX_COMMAND_OUTPUT_BYTES = 64 KiB`、`MAX_STATUS_LINE_LINES = 5`、`MAX_SANITIZED_CHARS = 1024`、从未绘制过时给脚本的 `RowSize::FALLBACK = 80×1`。interval 从**上一次运行结束**起算，避免慢脚本无间隙连跑；失败处理分两路——state 触发的失败立即画 `[status line: …]`，refresh 触发的失败在累计 3 次前保留上次输出（`app/status_line.rs:19-33, 366-400`、`views/status_line/mod.rs:206-208`）。

**空间不足 / 数据缺失时谁先被砍**。注意 `app/status_line_policy.rs` 管的是节流与运行生命周期，**不是取舍优先级**（`status_line_policy.rs:110-128, 280-349`）。真正的取舍是固定四层：

```
L1 段内列上限：cwd 40 / model 30 / session-name 40 → fit_columns 尾接 "…"
L2 行内 elide：从右往左吃，末尾补 "…"；link span 同步夹到被保留的列
L3 行数：脚本 5 行上限先被配置层砍（sanitize），再被 min(总高 − 其余所有行) 回砍
L4 整面 Off：ResolvedStatusLine 为 None，或 settled 且无内容 → 高度 0
```

- 缺数据的 item 整段消失而不显示占位值（`filter_map`，`views/status_line/segments.rs:72-117`）。
- 成功但**什么都没打印**的脚本会让整行消失，而不是回退到内置段；`Reserved` 状态在首个结果到达前占住 1 行，避免第一份输出把输入框往上顶；短终端从底部开始丢行（`status_line_policy.rs:293-294`、`views/status_line/mod.rs:116-118, 188`）。
- 状态栏芯片的坐标只算一次、永远右对齐，唯一的前插接口 `push_front` 必须先用 `room_for_front` 预量宽度；`link_url` 就是先扣掉 24 列位置地板、再要求 ≥ 12 列才画（`views/agent_status.rs:64-77`、`render.rs:1371-1381`）。

**上下文与成本的阈值**

| 口径 | 位置与形状 | 阈值与颜色 |
| --- | --- | --- |
| `used / total` 紧凑串 | status bar `context` 芯片，`8.5K / 1.0M`（`fmt_tokens`，宽度 ≤4 字符） | 连续色带，断点 0 / 50 / 65 / 75 / 85 / 95 %，段间线性插值，色序 text_primary → accent_user → warning → accent_error |
| hover 态 | 同一芯片换成进度条 + `fmt_pct5`（固定 5 字符，`≥100%` 显示 `MAX %`） | 同一色带 |
| `{pct}% ctx` | status line `context` item，整数百分比 | 二值：`pct ≥ CONTEXT_WARN_PCT(80)` 用 Warn，否则 Dim |

（`views/context_bar.rs:16-41, 53-100, 141-196`、`views/status_line/segments.rs:11, 87-99`。）芯片的**宽度不变量**是刻意设计：默认串右侧补齐到最小 6 列（`BAR_PCT_GAP 1 + PCT_WIDTH 5`），hover 切换不引起布局抖动；色带插值后要 `quantize`，测试钉住 0–49% 必须留在终端默认前景色（`views/context_bar.rs:133-141, 165-183, 205-227`）。

其余阈值：`MIN_DISPLAYED_COST_USD = 0.005`；credit 芯片 80% warning / 100% accent_error；余额告警 `LOW_BALANCE_CENTS = 1000`、`PAY_AS_YOU_GO_CRITICAL_CENTS = 500`（走 prompt 的 usage warning）；`/context` 只在 `(80, auto_compact_threshold)` 区间才提示「run /compact to free up context space」（`views/status_line/segments.rs:18`、`views/credit_bar.rs:141-255`、`scrollback/blocks/context_info.rs:437-446`）。

`/context` 块本身：条形永远是 **100 格**，两种形状共享同样的 100 格所以视觉份额不变——`WIDE` 5 行 × 20 格（约 39 列）、`NARROW` 10 行 × 10 格（约 19 列），切换点 `NARROW_BREAKPOINT = 50` 列；字形 ◆（system/messages）/ ◈（tools，只作参考行不进条）/ ◇（free），全部经 `glyphs` 走 CP437 降级，列宽由实际渲染行反推以保证数字对齐（`scrollback/blocks/context_info.rs:31-63, 92-119, 248-264`）。

**运行态行（turn_status）**。文案分派全在 `compute_activity` 的一个大 match（`views/turn_status.rs:580-698`）：`Cancelling…` / `Verifying…` / `Thinking…` / `Responding…` / `Compacting…` / 重试标签（warning）/ `Running…` / `Waiting…`，工具运行中则留空文案交给阶段标签。`Waiting(reason)` 有 7 种视图专用原因文案（`acp/tracker.rs:48-73, 116-136`）：`Waiting for response…`、`Waiting on subagent…`、`Waiting on task output…`、`Waiting on tasks…`、`Sleeping…`、`Running {event} hook…`、`Waiting for the agent to accept the prompt…`。出现条件是 `parked || !state.is_idle() || drain_blocked || session_starting_since.is_some() || watchers.total() > 0`。空闲的其余表达：`/tasks` 芯片在 `counts == default()` 时返回 `None` 整条消失，`goal` 芯片只在 `goal_state` 存在时 push（`views/agent_status.rs:146-148`、`render.rs:1283-1297`）。

**shortcuts bar**。内容由 `key_owner()` 三选一（`app/agent_view/render.rs:277-318`）：阻挡式表面（line/block viewer、权限卡、plan approval、question 卡等）原样渲染 `Surface(hints)`；普通 pane 走 `Pane(hints)` → `.compact(5, help_hint)`；`Hidden` 时不画。放不下的项整项 `break`（先试 5 列分隔符，再试键 / `:` / 标签），**不画省略号**；`compact` 下 `pinned` 的 hint 永远保留，`help_hint` 无条件追加（`views/shortcuts_bar.rs:261-292, 338-366`）。cheatsheet 默认绑 `Ctrl+.`，终端不可靠时主键换 `Ctrl+X`、另一个进 `alt_keys`（`actions/defaults.rs:805-827`）。pending confirmation 会顶掉所有 hint，只显示 `{key}:press again to {label}`（`views/shortcuts_bar.rs:229-248`）。

- 与 status line 的邻接：status line 在上、shortcuts 在下；status line 关闭而底部有 vpad 时补一个 `shortcuts_gap` 行。minimal 模式**没有** shortcuts bar，status line 照样画，且落在 prompt info 行之下（`views/agent.rs:291-298`、`xai-grok-pager-minimal/src/live.rs:4, 404-416, 714`）。
- 同时消失的情形：全屏子代理视图用 `a_subagent_owns_the_frame()` 直接 return；welcome 屏 `status_line_source_agent()` 返回 `None`；`active_modal.is_some()` 时直接画模态并 return（`app/status_line_policy.rs:31-33, 82-91`、`render.rs:3211-3215`）。

### 4.6 侧边与辅助面板

#### 本节要点

- 所有辅助面板都**纵向插入**在「顶部状态栏 ↔ scrollback」或「scrollback ↔ prompt」之间，顺序固定；唯一占列宽的是 timeline rail（它顶替 scrollbar 的位置），因此面板开关只改变高度、不改变主内容区的水平布局（`src/views/agent.rs:228-381`）。
- 只有 **tasks 面板会自动弹出**，且只在「运行中任务 0→N」的边沿弹、只收起自己开的实例、排除 replay 恢复的历史任务；todo 面板**绝不自动弹出**（`src/views/tasks_pane.rs:903-927, 1045-1050`、`src/views/todo_pane.rs:218-223`）。
- todo 既**没有层级**也**不显示完成度**，条目只有 `□ ▶ ✓ ✗` 四态；「进行中」靠加粗 + 警示色，而不是动画或移动（`src/views/todo_pane.rs:25-91, 143-152, 295-309`）。
- 「不打断主流程」的核心手法是**状态只在非 transcript 表面就地更新**：运行计数只活在 idle 的 still-running 状态行上，完成才落一个 chip，transcript 从不再复述计数（`docs/user-guide/20-background-tasks.md`、`src/views/turn_status.rs:108-125`）。
- 子代理在父 scrollback 里**只有一个块**，靠 `scrollback_entry_id` 就地改状态与活动后缀；进入子代理输出是**边框画面替换父视图**而不是分屏，父视图的面板与 composer 全部归零（`src/app/subagent.rs:80-92`、`src/app/agent_view/subagent_takeover.rs:93-343`、`src/app/agent_view/render.rs:1014-1028`）。
- timeline 是**一 turn 一 tick 的导航轨道**（tick 位置表会话顺序，不是滚动比例），恒宽 2 列并顶替 scrollbar；未找到证据：catalog 面板在本快照中没有把 `visible` 置 `true` 的路径（打开入口不可达），子代理并发上限的默认值也未找到。

**各面板一览**（`src/views/agent.rs:228-381`、`src/views/todo_pane.rs:154-158`、`src/views/tasks_pane.rs:657-658`、`src/views/subagent_catalog_pane.rs:69-70`、`src/views/timeline.rs:17-23`、`src/views/goal_detail.rs:3`、`src/views/dock/mod.rs:2, 22`）

| 面板 | 高度策略 | 默认 | 开关 |
| --- | --- | --- | --- |
| tasks | `min(MAX_TASKS_HEIGHT=8, 15% 视口)`；视口 < 12 行返回 0；搜索/过滤条额外 +1 行 | 隐藏 | `Ctrl+G`；**运行中任务 0→N 时自动弹出** |
| todo | `min(MAX_TODO_HEIGHT=10, 15% 视口)`；可见但无条目时返回 1 行 | 隐藏 | `Ctrl+T`，不自动弹出 |
| catalog | `min(MAX_CATALOG_HEIGHT=8, 15% 视口)`；视口 < 12 行返回 0 | 隐藏 | 未找到打开入口 |
| timeline rail | 固定 2 列，顶替 scrollbar | 隐藏 | 设置项 `show_timeline` 或 `/timeline`（无默认快捷键） |
| goal detail | 居中圆角浮层；`width < 20 \|\| height < 6` 不绘制 | 隐藏 | 点状态栏 goal chip；`Esc` / `g` 关闭 |
| dock | 最多 `MAX_DOCK_ROWS = 8` 行 | 隐藏（远端 `dock_enabled` 才开） | `Ctrl+G`；开启时是 tasks 面板的替代品 |
| turn status | 固定 1 行，idle 且无后台工作时为 0 | 隐藏 | 自动 |

固定顺序为 `tasks → catalog → todo → scrollback → btw → queue → turn_status → banner → cta → follow_ups → dock → prompt`；scrollback 是唯一的 `Constraint::Min`（注释说明求解器把 `Min` 排在每个 `Length` 之前，所以「总需求超出屏幕时，被压缩的是别的行」）；面板间 `pane_gap` 仅在 `top_vpad > 0` 时为 1 行；`SHORT_TERMINAL_ROWS = 16` 以下强制砍掉 CTA、follow-ups 与底部 vpad；鼠标 `hit_test` 的优先级是 tasks > catalog > todo > queue（`src/views/agent.rs:61-94, 205-246`）。

**tasks 面板的自动开关（最值得抄的一条）**（`src/views/tasks_pane.rs:903-927, 1045-1050`）：

```rust
if running_count > 0 && self.prev_running_count == 0 {      // 只在 0→N 的边沿
    self.overlay.show();
    self.opened_by_auto = true;
}
if running_count == 0 && counts.paused_workflows == 0
    && self.overlay.visible && !self.overlay.focused
    && self.opened_by_auto && !self.show_done {              // 只关自己开的
    self.overlay.visible = false;
    self.opened_by_auto = false;
}
```

三个细节：不在 N→N+1 时重弹；用户自己 `Ctrl+G` 打开的面板不会被系统收走（`on_state_change` 把 `opened_by_auto` 清回 `false`，任何手动操作都「接管」面板）；**replay 恢复的历史任务不算数**，否则 resume 旧会话会立刻弹面板。四个侧栏共用 `OverlayState { visible, focused, fullscreen }` 三态机，聚焦画 `selection_border`、hover 画更淡的 `hover_border`、两者都不满足就完全不画（面板在非聚焦时保持无框外观）（`src/views/overlay.rs:47-131`、`src/views/agent.rs:574-625`）。

**todo / plan 的呈现**。`TodoItem` 只有 `content` / `priority` / `status` / `meta`，**pane 只渲染前两者**；条目顺序保持模型给的顺序（注释明确「no reordering by status」），条目 id 就是原始下标，因此选择能跨重建存活。数据入口是 ACP `SessionUpdate::Plan`，整批替换（`update_todos` 的注释直说「Does NOT auto-show the todo pane」）；更新时没有闪烁、没有 toast、**不插消息**，视觉反馈完全来自「下一次重绘时内容变了」加上进行中的粗体。面板内 `h` 切换已完成/已取消项（默认 `show_done = true`）；小视口限速：0–5 行时每次滚动 1 行、6–10 行走 2 行（`crates/codegen/xai-grok-tools/src/implementations/grok_build/todo/mod.rs:143-150`、`src/views/todo_pane.rs:190-199, 208, 218-223, 314-344, 368-386`、`src/app/acp_handler/mod.rs:244-252`）。plan mode 的产物**不进 todo 面板**：它写会话目录的 `plan.md`，由 `exit_plan_mode` 触发独立审批浮层（`a` 批准 / `s` 请求修改 / `c` 行内评论 / `y` 复制 / `q` 放弃），状态栏另有 `plan` chip 可重开预览（`docs/user-guide/19-plan-mode.md`、`src/app/agent_view/render.rs:1276-1282`）。

**tasks 面板：一个列表承载四类工作**。`Workflows`（运行中优先，再按开始时间倒序）/ `Subagents`（先按 agent type 字母序，再按开始时间倒序）/ `Tasks`（开始时间倒序）/ `Watchers`（monitor 恒在 loop 之前）；排序是四段比较器「类型分组 → 组内运行中优先 → 组内时间倒序 → `stable_id` 兜底」，最后一条是为了「等时间戳的行不会逐帧重排」。分组表头是可折叠行（`▾ Subagents 2` / `▸ Subagents 2`，chevron + 空格的宽度恰好等于条目的 2 空格缩进）；**组被清空后其折叠状态会被遗忘**，新任务到来时该组重新以展开态出现，避免藏在过期的折叠表头下。每行右侧叠绘 `[✗]`（kill）与 `[↗]`（view）两个 3 列按钮；因为按钮叠绘在 label 之上，绘制前先清格，若覆盖到文字就在左邻格补一个 `…`（样式取自左邻格颜色），让用户看到的是「被截断了」而不是「莫名少了几个字」。失败没有单独面板或红框，只体现为红色 `✗` 图标 + label 降饱和；workflow 的终态覆盖 `interrupted | complete | failed | cancelled`，非 active 且非 terminal 视为「暂停」（`src/views/tasks_pane.rs:1-5, 108-131, 333-347, 535-552, 580-597, 698-728, 843-897, 960-987, 1344-1535`、`src/views/workflows.rs:60-65`）。

任务计数常驻顶部状态栏：`◆ N` = 运行中的 bg task + 非 workflow subagent + scheduled + active workflow 之和，`P N` = 暂停的 workflow 数，两者都为 0 时整条 chip 不渲染；`◆` 用**静态**实心菱形而不是 spinner，注释理由是「表头全程在视野内，这里的 spinner 读起来是噪声而不是进度」。同一批 chip 也是鼠标目标（点 `◆ N` 切 tasks 面板，点 goal chip 切 goal/workflow 浮层）（`src/views/agent_status.rs:141-182`、`src/views/tasks_pane.rs:1007-1031`、`src/app/mouse.rs:53-79`）。

**子代理**。父 scrollback 里只有**一个块**：阻塞式（sync）一个 `Started` 块，运行时闪烁、结束变绿/变红；后台式（async）的 `Started` 块永久保留（转灰），结束时**另加**一个 `Completed` / `Failed` 块。运行中把子代理的实时活动（"Thinking"、"Running: cargo test"、"Compacting"）作为后缀就地更新，所以不打开子代理视图也能看到交互式进度。嵌套深度上限为 1（子代理再 spawn 会因 depth-limit 失败），所以列表里**不需要树形缩进**，靠 `type_label`（persona > role > subagent_type > tag > `general`）排序分组。进入输出有三条路径（父块上 `Enter`/`Ctrl+F`、tasks 面板的 `[↗]`/双击/Enter、dashboard 会话行），最终都走 `open_subagent_fullscreen(child_sid)`；若子视图 scrollback 还空着会先 replay 子会话的 `updates.jsonl`。全屏视图是**替换父视图的边框画面**，标题行放全部状态（状态图标 → 类型标签 → 加粗描述 → model → 上下文徽标 → 活动 `· 耗时` → `[✗]`），父视图的 tasks/catalog/todo 高度强制为 0、composer 也是 0 行（纯观察态，不能输入、不能排队）。返回靠 `q` 或 `Esc`（仅当子视图处于「裸 scrollback」；搜索打开时第一下先关搜索）、点 `[✗]`；`Ctrl+Q` **永不被消费**，始终冒泡到全局退出，注释把这条列为不变量（`src/app/subagent.rs:80-92, 961-996`、`src/scrollback/blocks/subagent.rs:1-11, 63-66`、`src/app/agent_view/subagent_takeover.rs:7, 33-80, 93-343, 358-409`、`docs/user-guide/16-subagents.md`）。

**timeline rail**。它**不是**「滚动位置的第二种画法」而是**会话的 turn 索引**（`Turn { prompt_index, end_index, status }`），模块头写明「tick 位置表会话顺序，不是滚动比例」，所以 `TurnStatus` 在 rail 上不参与渲染。出现门槛：终端宽 `< MIN_TERMINAL_WIDTH(60)`、turn 数 `< MIN_TURNS(2)`、或当前不是 `ViewSurface::Root` 时宽度为 0（理由：窄终端上「transcript 比导航器更需要那些列」，「1 个 turn 的时间线是噪声」）。几何每帧算一次成 `TimelineRail`，渲染与鼠标命中**共用同一份数据**；rail 需要 scrollbar 的 gutter 几何（`scrollbar_cfg.enabled == false` 时 rail 宽度强制为 0），活动 tick 是「拥有视口顶部的那个 turn」，chevron 的「变暗」与点击 no-op 由同一个 `chevron_target` 决定，所以**变暗的 chevron 一定是 no-op**。它**不画轨道底色**（深色轨道条在 tick 少时读起来像一条尴尬的空带）。hover 预览卡悬浮在 rail 左侧、最多 2 行、宽度钳在 16..32 列，内容是该 turn 首个非空行（上限 120 字符）。它与 `/jump` 共用 `timeline_entries()` 返回的稳定 `prompt_entry_id`（不是下标），所以条目的插入/删除不会让导航指向别的块（`src/views/timeline.rs:1-3, 4, 17-23, 26-46, 61-76, 109-161, 185-213, 228-242, 248-331`、`src/scrollback/state/types.rs:23-31`、`src/scrollback/state/timeline.rs:5-59, 85-104`、`src/views/agent.rs:382-393`、`src/settings/defs.rs:533-548`）。

**「不打断主流程」的六条手法**（`docs/user-guide/20-background-tasks.md`、`src/views/turn_status.rs:108-125`、`src/views/tasks_pane.rs:786-958, 1074-1085`、`src/views/todo_pane.rs:368-386`、`src/app/subagent.rs:80-92`、`src/views/agent.rs:95-103, 208-219`）

1. **状态只在非 transcript 表面上更新**：运行中的计数只活在 idle 时的 still-running 状态行（`◎ 1 command · 2 monitors · 1 loop · 1 subagent still running`）；完成事件才在 transcript 里落一个 `Task completed` chip；transcript 从不复述或重述这些计数。
2. **单行高度在 0/1 之间切换**，不占位也不弹层：turn status 靠 layout 里那一行出现/消失来「通知」，列表高度也用同一套路（隐藏时 `desired_height` 返回 0）。
3. **就地更新而非追加**：子代理运行时只有 1 个块，靠 `scrollback_entry_id` 就地改颜色与活动后缀；目标状态 chip（`[Goal: Executing] 12.3k/50k tokens 3m`）是状态栏 item，每帧重建，从不进 transcript。
4. **每帧从单一数据源全量重建视图模型，不做增量 patch**：代价由「条目数 < 20 时很便宜」承担，换来的是**不存在视图与数据不同步的窗口**；只保留 `highlight_cache` 与「按 stable_id 保持选择」两处必要缓存。
5. **动画靠单一 tick 驱动，只在有活干时才走**：`TasksPane::tick` 自增并返回「是否有运行中的条目」，子代理全屏视图的标题 spinner 直接复用 `tasks.tick_count()` 而不另起时钟；速度常量分场景（turn spinner `SPINNER_DIVISOR = 4` 约 7.5fps；idle 的 still-running 呼吸 `MONITOR_PULSE_DIVISOR = 8` 约为前者一半，理由是「idle 的提示应当平静地呼吸」）。
6. **屏幕太小时先砍辅助行**：`SHORT_TERMINAL_ROWS = 16` 以下砍 CTA 与 follow-ups，`AUTO_COMPACT_MAX_ROWS = 20` 以下强制 compact，视口 < 12 行时 tasks / catalog 直接返回高度 0。

### 4.7 首屏、空状态与渐进式披露

#### 本节要点

- 首屏不是一个页面，而是**同一个欢迎页上的 6 个短路分支**：`render_welcome` 用一个 `match params.auth_state` 顺序判断，**臂的顺序就是优先级**；`auth_state` / `trust_state` / `consent_state` 三者全部 `Done` 才允许创建会话（`crates/codegen/xai-grok-pager/src/views/welcome/mod.rs:758-903`、`src/app/app_view.rs:1252`）。
- **可读性是硬门禁而非尽力而为**：同意声明正文限 12 行 / 2000 字节 / 78 列，装不下就撤掉 Accept 只留 Quit，并提示放大窗口（`src/views/welcome/consent.rs:21-30, 65-80`、`src/app/consent.rs:16-20`）。
- 欢迎页有两套版面：内容宽 ≥ 90 列且高度够时用带边框 hero box（logo 左、版本/信息槽/菜单右），否则退化为 stacked；放不下时**降级而不是截断**（`src/views/welcome/hero_box.rs:14, 50-61`、`src/views/welcome/mod.rs:356-495`）。
- 空状态统一走「一行灰字 + 通用选择器」：无匹配是 `No matches`，dashboard 无会话是 `No sessions yet. Esc to dispatch one.`，选择器里是 toast `No sessions found for this directory`；唯一的品牌图案是欢迎页的盲文 logo，**没有**任何空状态插画或大图（`src/views/picker.rs:1922-1944`、`src/views/dashboard/render.rs:534-537`、`src/app/dispatch/session/foreign.rs:184-195`）。
- tips 是**两套互不相干的系统**：欢迎页 Tip of the Day（持久游标，每次启动一条）与会话内 ephemeral hint（单槽 + TTL + 去重键 + 四层防骚扰）（`crates/codegen/xai-grok-shell-base/src/util/tips.rs:41-52`、`src/tips/ephemeral.rs:10-33`）。
- 渐进式披露在代码里有三种手法：**变暗而非隐藏**、**置位才出现**、**子项收进子页**；教程则反过来走「绝不自动弹出 + 编译期嵌入 markdown + 强约束测试」（`src/views/shortcuts_help.rs:63-72, 127-130`、`src/views/welcome/mod.rs:1763-1789`、`src/settings/defs.rs:456-467, 1259-1301`）。

**首次启动的完整序列**

```
grok 启动
   │
AuthState::Pending ────────────► ① 登录菜单屏     Login with grok.com / Quit
   │ 已认证 / 免登录
AuthState::Authenticating ─────► ② 认证等待屏     浏览器授权 / 设备码 / 粘贴 token
   │
is_zdr_blocked ────────────────► ③ ZDR 阻断屏     Switch account / Quit
   │ 否
has_access == false ───────────► ④ 订阅闸门屏     Tier + [Refresh] + 文案 + 链接
   │ 是
ConsentState::Pending ─────────► ⑤ 同意声明屏     a Accept / q Quit
   │ 无待同意
TrustState::Pending ───────────► ⑥ 目录信任屏     y Yes, proceed / n No, quit
   │ 已信任
⑦ 正常欢迎页（hero box 或 stacked）
```

- 门禁顺序在注释里被显式固定：同意是账号级，所以先于工作区级的信任问题解析（`views/welcome/mod.rs:866`）。信任与同意都在**首帧渲染前** seed（`src/app/event_loop.rs:336-377`），注释写明「必须早于第一次渲染，否则启动意图块会在门禁后面就把会话开起来」。
- **输入捕获的守卫**：门禁未清空时，启动时敲入的 prompt 会被丢弃而不是重放，否则一个以 `n` 开头的 prompt 可能替用户回答信任问题并退出（`src/app/app_view.rs:1257-1266`）。
- ①② 的视觉：登录屏走「Logo. {prompt}. Version」三段式（上次认证失败时 `error` 以 `accent_error` 画在同一行槽位），且**仍绘制 home 草稿的输入框**；认证等待屏四个子臂共享「logo + 居中块 + 退出提示」结构，设备码是从验证 URL 里解析出 `user_code` 再上屏（反钓鱼），原始 URL 模式关闭鼠标捕获以便用终端原生选择复制整条 URL；退出提示按终端品牌切换（VS Code 系显示 `ctrl+d`，其余 `ctrl+q`）。未找到取消/中断认证的按键证据（`views/welcome/mod.rs:49-70, 793-800, 955-966, 1079-1128, 1203, 1303, 2552`）。
- ⑤ 同意屏：正文按 `app::consent::wrap` 排版，校验失败有结构化枚举 `ConsentArmRefusal`（正文过高、未配对标记、纯文本里的 URL、版本号不合理……），失败一律 fail-open；布局会**优先丢掉 logo 而不是判定不可读**（`views/welcome/consent.rs:27-30, 110-115`、`src/app/consent.rs:47-70`）。
- ⑥ 信任屏的触发条件是「特性开启 + 该目录未信任 + 目录里有仓库级可执行配置」；警告文案被拆成两行，注释说明是为了在窄/紧凑宽度下不截断（`src/app/event_loop.rs:344-355`、`views/welcome/mod.rs:1004-1026`）。
- 启动警告（终端诊断、认证、配置迁移等任何子系统）共用**单槽** banner，画在错误槽位；选取规则是「第一个 Warning 级，否则最后一条」，避免旧的 Info 挡住新的 Warning（`src/startup.rs:66-73`、`views/welcome/mod.rs:2601`）。

**欢迎页版面与槽位争夺**。三种版面：全屏会话选择器（`welcome_compact = true`，logo/菜单隐藏）、hero box（非紧凑 + 内容宽 ≥ `HERO_BOX_MIN_WIDTH = 90` + 菜单非空）、stacked；hero box 放不下时整体回退到 stacked。盒内边距 `V_PAD = 1`、`H_INSET = 2`、`LOGO_H_PAD = 3`；副标题只在信息槽为空时占一行，避免盒子变高；菜单行按 `label … shortcut` 排版（标签左对齐、快捷键右对齐、中间至少 4 列）。信息槽里的公告优先于更新日志。行优先级（注释写死在代码里）：**待应用更新提示 > 隐私横幅 > 外部会话恢复提示 > 常规 Tip of the Day**；选择器可见时 tip 高度直接为 0。菜单行也不是静态的：`Import Claude settings` 只在检测到待导入的 `.claude/` 配置时插到最前；`Changelog` 行在 `has_access` 时**从第一帧就在**，免得等 CDN 拉取时菜单跳动（`views/welcome/hero_box.rs:17-38`、`views/welcome/mod.rs:327-340, 1707-1756, 1763-1789, 1802-1806, 1981-2010`、`views/welcome/menu.rs:37-47`、`views/welcome/logo.rs:1-3, 16-18, 36-42`）。

盲文 logo 是唯一的手绘图案（`assets/logo/logo07.txt` / `logo05.txt`），按终端高度分三档：≥ 26 显示全高图案，≥ 22 显示小图案，低于 22 不画；旧版 Windows 控制台字体不含 U+2800 盲文块会渲染成 tofu，因此在那些终端上**整个 logo 隐藏**（`views/welcome/logo.rs:1-3, 12-18, 36-42, 65-67`）。

**空状态与占位符清单**

| 场景 | 显示 |
| --- | --- |
| 无会话（resume 选择器，原生通道） | toast `No sessions found for this directory`（部分加载失败时改用 `picker_notice`） |
| 无会话（dashboard 面板） | ` No sessions yet. Esc to dispatch one. ` |
| 选择器无匹配（文件搜索 / 历史搜索 / 设置筛选的通用路径） | `  No matches`，`gray_dim`，画在内容区首行 |
| 选择器加载中 | 居中点阵 spinner + `Loading…`（帧按 tick/4 轮转） |
| 空状态与 spinner 的竞态 | 「空状态必须等 native 与 foreign 两条加载通道都停下来」，且渲染 / 重绘 / tick 三处共用同一判据 |
| 设置搜索无结果 | `No matches for <query>` |
| 无消息（欢迎页输入框） | 占位符 `Type a message...` |
| 无消息（会话内输入框） | 主输入框聚焦时隐藏占位符 |
| 无公告 | 会话 banner 高度为 0（槽位不占行） |

（`src/app/dispatch/session/foreign.rs:184-195`、`src/views/dashboard/render.rs:534-537`、`src/views/picker.rs:1922-1944`、`src/views/session_picker.rs:152-172`、`src/views/settings_modal/render.rs:462-465`、`src/views/welcome/prompt.rs:31`、`src/app/agent_view/mod.rs:370-381`、`src/views/prompt_widget/mod.rs:169-173`、`src/views/announcements.rs:321-323`。）

**tips A 类：欢迎页 Tip of the Day**。内容是四层配置合并（requirements > remote settings > 用户 config > managed config）；任一层设 `exclude_default = true` 会丢掉 remote 层，`[cli] show_tips = false` 直接关掉全部。每条 tip **每次启动只显示一条**，由持久化游标顺序推进：读 `~/.grok/tip_cursor.json` 的 `cursor`，取 `tips[cursor % len]`，写回 `cursor + 1`，全部轮完再循环。骚扰控制很直接：用户在欢迎页**只要一开始打字**，本行的 tip 就永久隐藏到本次进程结束（`crates/codegen/xai-grok-shell-base/src/util/tips.rs:11-52`、`crates/codegen/xai-grok-shell/src/util/config/tips.rs:29-77`、`src/app/event_loop.rs:1424-1433`、`src/app/app_view.rs:4471-4478`）。

**tips B 类：会话内 ephemeral hint**。核心原语是**单槽 + TTL + 去重键**的提示行，画在输入框上方的 banner 行；`DEFAULT_TIP_TICKS = 90`（≈3s @30fps；到期判定在减 1 之前，所以 N 值实际存活 N+1 tick）。同 key 再 show 只刷新 TTL、不叠加：

| 提示 | 触发条件 | 上限/会话 | TTL（tick） | ambient |
| --- | --- | --- | --- | --- |
| `Input cleared · ctrl+z to undo` | 草稿峰值 ≥ 20 字符后被清空到 ≤ 5 字符 | 3 | 90 | 否 |
| `Planning? Check out plan mode via shift+tab` | prompt 里出现整词命中的规划关键词，且非 plan 模式、会话空闲、输入模式 Normal | 3 | 90 | 否 |
| ctrl+v 可粘贴剪贴板图片 | 终端聚焦 + 当前模型接受图片 + 未附图片；机会式轮询 1s 节流 + 30s 冷却 + changeCount 去重 | 冷却+去重 | 90 | 否 |
| `Queued · Enter to send now` | 回合进行中排队跟进后 | 3 | 90 | 否 |
| `Tight on space? Try /compact-mode` | 终端行数落在 `(AUTO_COMPACT_MAX_ROWS=20, 28]` 且未开 compact mode | 1 | 90 | 是 |
| `grok wrap ssh` 引导 / `/doctor` | 首个稳定 agent 绘制时由 `diagnostics::ssh_wrap_hint` 判定环境 | 1 | 300 | 是 |
| 双击选词可改设置项 | text selection 仍为 fold/nav 时在 scrollback 双击 | 3 | 600 | 是 |
| 拖拽复制后提示导出/复制 | 15s 窗口内 ≥ 3 次拖拽复制，先 armed 再于 toast 结束后落屏 | 3 | 600 | 否 |

（`src/tips/*.rs`，常量位置：`clear_detector.rs:18`、`plan_nudge.rs:18`、`send_now.rs:11`、`small_screen.rs:20`、`ssh_wrap.rs:21, 26`、`word_select.rs:16, 21`、`export_copy.rs:14, 16`。）

**防骚扰的四层机制**（`src/app/agent_view/notices.rs:28-37, 73-137`、`src/tips/ephemeral.rs:10, 27-33, 77-101`、`crates/codegen/xai-grok-shared/src/ui_config.rs:184-230`）：

1. **展示门禁**：`show_ephemeral_tip` 先过 `ephemeral_tip_renderable`，不可绘制就直接返回 `false`——不计 seen、不烧 TTL、不发遥测。不可绘制的条件有十余种：权限询问队列非空、question view、任何 active modal、隐私横幅激活、子代理全屏接管、各类 viewer、gboom、goal detail、workflows、prompt 下拉打开、会话 announcement banner 占用、终端尺寸未知或过矮（16 行及以下不显示，17 行起显示）。
2. **TTL 只在可见时燃烧**：会话 banner 或隐私横幅占用槽位时停表；ambient 提示在被遮挡时**暂停而非扣减**。
3. **会话级配额**：`session_seen = Some((key, cap))`，计数在内存、每次运行重置。
4. **用户级开关**：8 个 `[ui.contextual_hints]` 布尔项（`undo` / `plan_mode` / `image_input` / `send_now` / `small_screen` / `word_select` / `export_copy` / `ssh_wrap`），`None` 继承（默认开），`Some(bool)` 是用户显式选择并压过 remote 默认；每个 dispatch 入口都先查开关。

遥测上每次落屏记 `tip.shown`，离开槽位记 `tip.dismissed` 并带原因 `replaced` / `expired` / `cleared`；提示文案里的按键**从真实绑定派生**（如 `ctrl+z` 由 `is_undo_key` 认可的 KeyShortcut 生成、`shift+tab` 取 `shift_tab_keys()[0]`），避免文档与实现漂移（`src/tips/ephemeral.rs:180-197`、`src/tips/clear_detector.rs:29-35`、`src/tips/plan_nudge.rs:37-43`）。

**announcements（公告）与隐私横幅**。只有 `critical` 与 `promo` 两个严重级会进入会话槽位（`info` / `warning` 只上欢迎页），会话槽里 **critical 永远压过 promo** 且每帧重新推导；critical 也压过隐私横幅，promo 不压（「故障通知不能被长期 nag 挡掉」）。高度：critical 2 行、promo 1 行、无公告 0 行；promo 在会话内只画 1 行按钮/隐藏入口、**不画 message 正文**，欢迎页 hero 才有空间画正文并支持点开内联展开。去重键优先用 `id`，否则用 `content:<title>\u{1f}<message>` 合成；持久化形状 `{"hidden_ids": [...]}`（`BTreeSet`，确定性顺序），旧版 `{"hidden": bool}` 会退化成空集并重新显示一次；过期键只在真实更新路径清理（注释明确禁止逐帧清理）。`dismissible: false` 抑制两种公告上的所有隐藏入口并让文本收回预留列；逃生口是 `/announcements show`，其可用性判据**故意忽略隐藏集合**，因此全隐藏时仍能自救（`src/views/announcements.rs:1-23, 109-159, 203-248, 290-333, 383-410`、`crates/codegen/xai-grok-announcements/src/lib.rs:78-125`）。

隐私横幅有 6 重门禁（非 minimal 屏、命中 rollout 分群、非 ZDR 且非 team 非管理员、没有正在进行的写入、当前 opt-out、认证/权限/信任全部就绪）；已确认后按天数决定重提醒（`None` / `0` = 永不再现，解析失败按「该重现」处理）。版面是标题 `Help improve Grok` + 右侧 `[Opt out]` / `[Opt in]` 共享一行 + 要点正文 + 法务行。三个法务行变体**从宽到窄挑第一个能整行放下的，绝不裁剪**；按钮**要么整块画要么不画**——防止「空白处的幽灵点击区」把用户静默 opt-in。正文最多 4 行、超出用 `…` 省略；若按钮列会让正文超过 3 行就放弃按钮列改用全宽（`src/app/app_view.rs:1140-1152, 1219-1248`、`src/views/privacy_banner.rs:12-58, 88-120`）。

**渐进式披露的三种手法**

| 手法 | 例子 |
| --- | --- |
| **变暗而非隐藏** | 快捷键手册里**所有** action 都在，只是当前上下文不生效的绑定变暗；分类按新手友好顺序 |
| **置位才出现** | `Import Claude settings` 行、工作区模式分段控件（仅 chat 模式且 `has_access`）、`Changelog` 行；更新日志区块在终端太矮/紧凑/被 tip 抢空间时整块消失；hero 副标题在信息槽有内容时让位 |
| **子项收进子页** | 设置页顶层的 8 个 contextual hints 只显示一个 `Show contextual hints` 组行，子项藏在子页里靠关键词索引串起来；设置项另带 `hidden_in_minimal` 标记控制 minimal 模式的可见性 |

（`src/views/shortcuts_help.rs:63-72, 127-130`、`src/views/welcome/mod.rs:327-340, 1752, 1763-1789, 1802-1806`、`src/settings/defs.rs:456-467, 489, 1257-1301`。）另外，`prompt_suggestion`（下一句 prompt 幽灵补全）的可见性刻意做成**每帧从当前输入推导**而不是按键驱动状态机，避免状态漂移；接受键是 Tab / 右方向键，仅当光标在文末时出现（`src/views/prompt_suggestion.rs:1-13, 78-104`）。

**引导与文档的联动**。教程内容是**编译期嵌入的 markdown**（`include_str!` 打进二进制，9 个主题，每项 `title`/`blurb`/`content`/`go_deeper`）；主题顺序被显式设计成一条线性流程（从别的工具迁移 → 第一条 prompt → 附加上下文 → 认识界面 → 更大的功能），overlay 里 `→` 按此推进，列表显示 `N/9 explored`。测试强制三条：每页 ≤ 50 行、`go_deeper` 的标题必须能解析到真实指南、标题唯一。`/tutorial` 是唯一入口（别名 `tour`、`onboarding`），命令注释写明「**绝不自动弹出**」；更深的文档走另一套嵌入表（`USER_GUIDE` / `REFERENCE_DOCS`，`/docs` 打开 DocPicker）。官方用户指南也把首启写进文档：首次启动会打开浏览器完成 grok.com 认证，凭证存 `~/.grok/auth.json` 并自动刷新（`src/tutorial_docs.rs:19-28, 30-31, 108-142`、`src/views/tutorial.rs:33-40, 305-311, 320-355, 387`、`src/slash/commands/tutorial.rs:1-23`、`src/docs.rs:1-8, 40-60`、`src/slash/commands/docs.rs:1-10`、`docs/tutorial/09-where-next.md`、`docs/user-guide/01-getting-started.md:74`）。

### 4.8 多会话界面与 dashboard

#### 本节要点

- 同时存在**三个「列出多个会话」的界面**，职责互不重叠：Agent Dashboard（本进程活会话）、Session picker（磁盘历史会话）、Agents modal（agent 定义/配置）（`crates/codegen/xai-grok-pager/src/views/dashboard/mod.rs:1-4`、`docs/user-guide/23-dashboard.md:9-11`）。
- Dashboard 一屏由 9 个具名 `Rect` 组成，**每个装饰行都是 0 或 1 行高并各自绑定一个终端高度阈值**；列表下方那块位置只有一个 owner——选中行时是 peek 面板，未选中时是 dispatch 输入框（`src/views/dashboard/layout.rs:167-257, 353`）。
- 行状态**每帧从 agent 实时分类，没有「标记完成」的命令**；排序 = 置顶优先 → 状态优先级（`NeedsInput 6 / Working 5 / Idle 3 / Inactive 2 / Completed 1 / Failed 0`，**没有 4**）→ 手动序 → `last_change_at` 降序 → id 兜底，且以「父行 + 其子代理」的簇为单位（`src/views/dashboard/row.rs:465-502, 826-974`、`src/views/dashboard/state.rs:215-228`）。
- Idle 组默认只留最新 8 个 + 1 小时内活跃者（`MAX_VISIBLE_IDLE = 8`、`IDLE_FRESHNESS = 3600s`、少于 `MIN_IDLE_FOLD = 2` 行不折）；每个父行最多 8 个子代理；搜索或过滤生效时**暂停折叠**（`src/views/dashboard/render.rs:897-938, 1385-1400`、`row.rs:86-87, 320-358`）。
- 跨进程会话靠 **leader roster**（`Working/NeedsInput/Idle|Dormant/Completed/Dead` 映射为界面状态，`Inactive` 是 roster 专属语义、默认折叠）与 **SQLite workspace store**（容量 256，`origin` 只在插入时写一次）表达（`src/app/roster.rs:14-78`、`row.rs:362-371`、`state.rs:194-199, 1196-1198`、`xai-grok-dashboard-store/src/types.rs:11-17, 207-214`）。
- Session picker 的「预览」是**行内展开详情字段**而不是第二个面板；源页签按 `Grok → Headless → External → All → Local → Remote` 固定循环，外来会话带 `claude`/`codex`/`cursor` 徽章且不可删除（`src/views/session_picker.rs:196-237, 676-790`、`crates/codegen/xai-grok-pager/src/views/picker.rs:2933-2946`）。

**三个列表屏的分工**

| 界面 | 数据来源 | 生命周期 | 入口 |
| --- | --- | --- | --- |
| Agent Dashboard | 本进程的 top-level agent（+ 子代理、roster、workspace store） | 本进程内存中的活会话 | `/dashboard`、`Ctrl+\`、`grok dashboard` |
| Session picker（`/resume`） | 磁盘上的历史会话（可含远端 / 外来工具会话） | 进程外、可常驻 | `/resume`、welcome 屏列表、dashboard 的 `Open Previous` |
| Agents modal（`/agents`） | agent 定义 / persona | 配置 | `/config-agents` |

Dashboard 的状态宿主是 `AppView::dashboard: Option<DashboardState>`，仅在 `active_view == ActiveView::AgentDashboard` 时激活；`Option` 只在进程关闭时重置，所以关掉再打开会保留选择与折叠状态（`src/views/dashboard/mod.rs:3-4`）。

**一屏分区**（`DashboardLayout`，`src/views/dashboard/layout.rs:167-257`）

```
┌ terminal 全宽 ───────────────────────────────────────────────────────────┐
│ top_margin（高 > 6 行时）                                                 │
│ main ~/xai [Choose Ctrl+L]   ◆ 2 awaiting │ ⋮ 1 working │ ◇ 1 idle       │ header
│ header_gap（> 12）                                                        │
│ + New Agent        Open Previous /resume │ Worktree Ctrl+w               │ actions
│ actions_gap（> 13）                                                       │
│ ▾ Awaiting 2 ─────────────────────────                                    │ 分节标题
│ ▏● reviewer · audit token flow        Awaiting your input            2m  │ 行（3 行高）
│  ● implementer · fix login bug        Running: cargo test            12m  │
│ ▾ Idle 9 ────────────────────────────                                     │
│  ○ housekeeping                       idle                            1h  │
│  + 5 more                                                                 │ Idle 溢出行
│ dispatch_gap（> 10）                                                      │
│ ╭───────────────────────────────────────────╮                             │
│ │ ❯ Dispatch a new agent                    │  ← dispatch 或 peek         │
│ ╰─ dispatch ────────────────────────────────╯                             │
│ shortcuts_gap（> 11）· footer：↑/↓ select (peek) · Enter open · Ctrl+R rename │
│ bottom_margin（> 16）                                                     │
└──────────────────────────────────────────────────────────────────────────┘
```

装饰行「从底部往上一行一行打开」，注释说这样「每多一行终端高度，要么给列表、要么给恰好一个 gap」；**peek 没有独立 rect**，它渲染在 dispatch 的 rect 内；列表左右各留 `LIST_OUTER_HPAD = 2` 列，header 与 actions 也用 2 列以便与列表对齐（`layout.rs:236-241, 353, 411-421`）。

**表头与动作行**。左侧位置标签是 `branch + " " + [worktree 徽章] + cwd_display`，三段分别上色、整行可点击（hover 加下划线），点击打开目录选择器；右侧 `[Choose Ctrl+L]` 只在预算够时出现（预算 = 最左 chip 的 x 坐标再退 3 格，位置标签左侧永远留 3 格 gutter 保证不被 chip 压住）。状态 chip **只统计 `indent == 0` 的行**（子代理随父行归组，不重复计数），顺序 `awaiting → working → idle → done → failed` 与排序优先级一致；`Inactive` 故意没有 chip，它的数量只出现在分节标题上。动作行左端 `+ New Agent`，右端**严格右到左**：worktree 放不下就整段放弃，`Open Previous` 只在 worktree 已放下且还有空间时才尝试；worktree 已武装时文案变成 `+ New Agent in Worktree` 与 `Disable Worktree`；光标若停在某个右端项而该帧它没被画出来，会退回 `+ New Agent`（`src/views/dashboard/chrome.rs:47-97, 110-179, 264-339, 361-370`）。

**行的数据模型与解剖**。`DashboardRow` 是每帧从 `app.agents` 重建的纯视图结构，不缓存。字段：`label`（显示名 → 生成标题 → 首个 prompt 前 30 字符 → `New session #<id8>`）、`subtitle`（`branch + location[+ " worktree"]`，绘制时以 ` · ` 前置、dim 色）、`state`、`activity`、`secondary_line`（NeedsInput 时是 `Pending: <标题>`，其余是 `last_turn_summary` 或最后一条 agent 消息首行）、`cwd_display` / `cwd`、`last_change_at`（**`SystemTime` 而不是 `Instant`**：roster 时间戳可能早于本进程甚至开机）、`pinned`、`badges`（`worktree`/`needs-input`/`subagents`/`tasks`/`watchers`/`workflows`/`pinned`/`failed`）、`context_pct`（roster 行没有）、`indent`（0 = top-level，1 = 子代理）、`is_more_placeholder`/`more_count`（`src/views/dashboard/row.rs:18-96, 280, 608-621`）。

行固定 3 行高（`ROW_HEIGHT = 3`），内容 1–2 行并在 3 格内垂直居中；列位是 `marker(1) | gap(1) | indent(2n) | icon(1-2) | gap(1) | label...`；背景三态为键盘选中 `bg_highlight` > 鼠标悬停 `bg_hover` > `bg_base`。状态图标：Working = 点状 spinner（`⋅ : ⸬ ⁙` 循环，legacy 控制台降级为 `. : ·`）配 `accent_running`；NeedsInput = 实心菱形（宽模式闪烁）配 `warning`；Idle/Inactive = 空心菱形配 `gray_dim`；Completed = 实心菱形配 `accent_success`；Failed = 实心菱形配 `accent_error`（`render.rs:31, 1502-1526, 1558-1584, 3027-3051`）。右端元数据槽二选一：默认 age，鼠标悬停或已武装删除时换成 `[✗]`，只有「可删除」的行（Idle/Inactive/Completed/Failed）参与；age 用粗粒度分桶 `just now / 5m / 3h / 2d / 1mo / 1y`，注释说明分桶刻意放宽以「避免秒级抖动」（`render.rs:1672-1736`、`state.rs:206-213`、`crates/codegen/xai-grok-pager-render/src/util.rs:108-133`）。活工作 chip 用灰字、无括号，格式 `Subagents 2 · Tasks 3`；空间不足时按 `Full`（`Subagents/Watchers/Workflows`）→ `Short`（`Sub/Subs/Watch/Flow`）→ `2 bg` → `bg` 逐级降级，并给标题留出至少 1 格（`row_title.rs:1-2, 126-194`）。

**分节、折叠与排序**。分节标题只在 `Grouping::State` 下产生，形态 `▸/▾ + 组名 + 计数 + ────` 长横线（窄模式退化成 `Done 12` 无横线），组名读作 `Awaiting / Working / Idle / Inactive / Done / Failed`——注释解释了为什么用 `Done` 而不是 `Completed`：短促、更像分组头。`Inactive` 分节**默认就是折叠的**。Idle 折叠见上方要点；子代理折叠独立（每父行最多 `MAX_VISIBLE_SUBAGENTS = 8`，多的折成 `… N more` 且不可被光标选中）。排序以簇为单位，子代理**永不跨父行重排**；`Directory` 分组下非置顶行先按 `cwd_display` 字典序，此时「状态优先」关掉（`state.rs:270-279`、`render.rs:1271-1408`、`row.rs:871-974`）。

**持久化**：`pinned` / `reorder` / `grouping` / `enabled` 写在 `~/.grok/config.toml` 的 `[dashboard]` 段，键用 **session id**（`top:<session_id>` / `sub:<parent>:<child>`）而非进程内槽位，所以重启后依然有效；解析上限 256 条、键长 1024 字节。环境变量 `GROK_AGENT_DASHBOARD=0` 可整体关掉（`state.rs:380-399, 4512-4520`、`docs/user-guide/23-dashboard.md:306-319`、`mod.rs:84-92`）。

**过滤与搜索**：`a:<text>` 匹配 `label` + `parent_label`（子代理可被父名命中）；`s:<state>` 精确匹配状态，同义词表 `busy/running→Working`、`done→Completed`、`dormant→Inactive`、`errored/cancelled→Failed`，未知 token 回退成子串匹配（让用户能看出状态路径没生效）；`#<text>` 连 `#` 一起匹配（防止误匹配到任意数字）；其他走 `label` + `cwd_display` 子串。命中 `is_more_placeholder` 行时直接保留；`s:` 过滤时不发分节头（`row.rs:791-825`、`state.rs:4322-4377`、`render.rs:978-980`）。

**Session picker 的交互**

- 一套逻辑、三种宿主（`Welcome` / `AgentModal` / `Dashboard`）、两种渲染（`Fullscreen` 交给 welcome 屏的 picker body；`Modal` 用 `ModalSizing { width_pct: 0.65, max_width: 120, min_width: 48, v_margin: 4, h_pad: 2, v_pad: 1, footer_lines: 2 }`）。Dashboard 里打开的标题是 `Open session`（`DASHBOARD_PICKER_TITLE`），只列 `kind == "build"` 且 `SourceFilter::Local` 的本地会话；它的输入路由是一份 89 行的薄壳，只把 `Selected` 映射成 `Action::DashboardPickSession`（`src/views/session_picker_surface.rs:7-65, 67-108, 161-214`、`src/app/dispatch/dashboard.rs:316-346`、`src/app/dashboard_session_picker.rs:13-88`）。
- 搜索栏标签固定 `" search: "`；搜索模式激活时标签用 `text_primary` 加粗，非激活但已有 query 时整条 query 画成 dim 且不画光标。输入即在标题上过滤，同时触发**内容深搜**：查询长度 ≥ 2 且当前源支持 FTS 时才发 `Effect::DeepSearchSessions`，并带 `deep_search_seq` 代际号防止旧响应串台（`CONTENT_EXPAND_OFFSET = 100_000` 用于隔离内容命中与模糊命中的展开索引）；深搜区标题是 `Extended search results (remote and local sessions)`，加载中是 `⸬ Searching session content…`；`External` 源禁用内容搜索（外来会话没有 FTS 索引）（`session_picker_surface.rs:110-148`、`src/app/dispatch/session/load.rs:712-720`、`session_picker.rs:234-237, 861-881`）。
- 「预览」是行内展开：每行右侧固定显示相对时间（右对齐 8 列），展开后依次显示 `ID / CWD / Model / Created / Updated / Source / Host / Messages / Recap / Last turn / Turns+Tools / Prompt`；键位 `y` 复制、`e` 或 `→` 展开、`E` 或 `←` 折叠、空格切换，结果里有命中时自动强制展开。行按 `repo_name` 分组，当前目录所属 repo 组提到最前，其余按字母序，组头是不可选中的 `Header` 行（`session_picker.rs:41-50, 676-722, 736-790`、`views/picker.rs:1598, 2933-2946`）。
- 源页签：`SourceFilter` 六值，循环顺序固定 `Grok → Headless → External → All → Local → Remote → Grok`（默认 `Grok` = 原生且排除外来工具会话；`is_active()` 以 `!= Grok` 为准）。徽章 `conversation` → `chat`，否则 `claude` / `codex` / `cursor`，未知源留空；Headless 页隐藏外来工具会话并在页脚提示 `N external session(s) hidden · f to show`。删除是 `d` 武装、`y` 确认、其他键解除武装，外来源的行不可删除；武装删除时整条页脚替换为 `y confirm delete / n cancel`（`session_picker.rs:57-131, 196-237, 882-900`、`src/app/foreign_sessions.rs:286-293`、`src/views/welcome/mod.rs:2456-2515`）。

**并发模型如何在界面上表达**

- 「谁在跑」不是持久标志而是**每帧重算**：有待答权限或待答提问 → `NeedsInput`；否则有活跃父级活动（turn/唤醒/命令/排队 prompt/replay）→ `Working`；否则有后台工作（running bg task 或非空 `scheduled_tasks`）→ `Working`；否则 `Idle`。因此**一轮 turn 结束但还有后台任务 / monitor / 定时 loop 时，行仍显示 Working**，并由 chip 与活动文案说明「还挂着什么」。子代理只有三种可能：未完成 → Working、完成 → Completed/Failed，**永远不会进入 NeedsInput**（v1 子代理无法向用户提问，有测试固化）（`row.rs:465-502`、`row_activity.rs:10-16`）。
- 「跑了多久」的锚点是有讲究的：Working 行用 `turn_started_at` 优先（这样「跑了 12m」指本 turn），其余状态用 `last_active_at`，两者都缺时落到进程级 `fallback_epoch`（`OnceLock` 冻结一次，避免每帧重锚）；roster / workspace 行的锚点是磁盘上的 unix ms，非正值当作「现在」（`row.rs:88-94, 607-621`、`render.rs:1681`、`util.rs:135-144`）。
- 刷新：Dashboard 打开时事件循环按 `DASHBOARD_POLL_INTERVAL = 1s` 轮询，且**只在 `active_view == AgentDashboard` 时**生效，三条路径择一：workspace store 刷新 → leader roster（`Effect::FetchRoster`）→ 本地磁盘会话列表（`Effect::FetchDashboardSessions`）。行本身不缓存，每帧重建（理由：「一个 pager 进程里 agent 数量是个位数，重建是免费的」）。局部动画按 `spinner_tick` 推进，且**只有真正被绘制出来的动画才继续 tick**（spinner 每 4 tick 换帧、NeedsInput 闪烁每 10 tick）；头部 chip 的 working spinner 也以「这一帧画过 chip」为准来标记（`src/app/event_loop.rs:1659, 2493-2509`、`src/views/dashboard/mod.rs:19-22`、`src/views/dashboard/animation.rs:5-7, 25-64`、`chrome.rs:110-113`）。
- 本地 agent 的元数据（title / model / last_turn_summary / is_worktree / last_change）持续同步进 workspace store，字段各自带上限（title 1024B、model 256B、summary 8192B），超长**按 char 边界截断而非报错**——「因为一次元数据同步不该因为标题太长而失败」（`src/app/workspace_sync.rs:177-205`、`xai-grok-dashboard-store/src/types.rs:20-29, 306-341`）。

**会话标题：生成 → 优先级 → 截断**。展示优先级（三处独立实现、语义一致）：`display_name`（手动 `/rename` 后）→ `generated_session_title` → `pending_prompts` 队首前 30 字符 → scrollback 第一条用户消息前 30 字符 → `New session #<session_id 前 8 位>`。`named_title` 是「前三级，没有就返回 `None`」的版本，供会话头部这种「宁可不显示也不显示假标题」的场合使用。标题由 shell 侧推送进 ACP 通知，pager 只负责接收；`/rename --auto` 把 `generated_session_title` 清空并发 `Effect::ResetSessionTitle`。生成时机与「早几轮重生成后冻结」的策略见用户手册（**本仓库未在 pager 侧找到重生成策略的代码，属于 shell 侧行为，未找到证据**）。截断有三层：标题源本身 60 字符 + `...`（按 char 计数，不切多字节）→ 行内 `truncate_str(label, 可用宽度)`（宽度已扣掉 chip 与 age）→ `New session #id` 后缀先画 `New session`、剩余宽度才画 dim 色的 ` #id`。标题文本进入渲染前统一过 `sanitize_display_text`：C0/C1 控制符、bidi 与格式控制字符替换为 U+FFFD，防止后端/模型内容注入终端转义序列或伪造标题（dashboard 走 `set_string` 原样写进 ratatui buffer，所以这层过滤是必需的）（`row.rs:503-507, 537-567`、`src/views/session_title.rs:16-33, 35-50, 151-180`、`row_title.rs:42-70`、`src/app/acp_handler/session_notification.rs:1004, 1019`、`docs/user-guide/17-sessions.md:44-46`）。

**跨进程 / 跨机的表达**

- **Leader roster**：请求 `x.ai/sessions/list` → `RosterListResponse`，广播 `x.ai/sessions/changed` → `RosterChanged { upserted, removed }`；pager 侧是**只反序列化**的镜像类型。解析器必须先剥 `{ "result": ... }` 信封，否则会「成功解析出空列表」，这个坑有专门的回归测试。界面映射 `RosterActivity → RowState`：`Working→Working`、`NeedsInput→NeedsInput`、`Idle|Dormant→Inactive`、`Completed→Completed`、`Dead→Failed`。**`Inactive` 是 roster 专属语义**（只由 roster 产生，本地 agent 永不落入），定位是「别的 pager 进程拥有、本进程没加载」的后台噪音，所以放在 Idle 之下、Done 之上且默认折叠。显示条件：与本地 agent 同 session id 的 roster 行被跳过；既无标题又不活跃且未置顶的 roster 行不显示；roster 行没有 `context_pct`，`secondary_line` 取 `last_turn_summary` 或 `model_id`（`src/app/roster.rs:1-78, 80-87, 113-150`、`row.rs:362-462`、`state.rs:194-199, 217-228, 1196-1198`）。
- **Workspace store**（dashboard v2 的权威数据源，「membership、layout、以及本进程没加载的会话」都由它说了算）：SQLite 存储；`Member` 含 `session_id / kind / origin / cwd / title / model / last_turn_summary / is_worktree / last_change_unix_ms / pin_rank / order_rank`；`MemberKind = Build | Conversation | Other`，`origin` **只在插入时写一次，之后任何 API 都不改**；容量 `WORKSPACE_CAPACITY = 256`，超限时在同一事务里淘汰最旧的未置顶成员；rank 用间隔 1024 的整数、插入取中点；`WorkspaceSnapshot` 在同一个读事务里带 `PRAGMA data_version`，这样「轮询方不会漏掉落在快照与首次轮询之间的那次提交」。界面上多出来的两种行：`DashboardRowId::Workspace`（store 里有、本进程没加载 → 标题取 title、退化到 cwd 名、再退化到 session id；状态**恒为 Idle**）与 `provisional` 行（活 agent 但还没落库的成员）（`xai-grok-dashboard-store/src/types.rs:11-17, 169-214, 365-403, 456-465`、`row.rs:131-286`）。
- **Foreign sessions**（其它 CLI 的历史会话）只在 session picker 里表达，不进 dashboard 行列表：源为 `claude / codex / cursor`（显示名 `Claude Code / Codex / Cursor`），各自由 `resume-claude|codex|cursor` 技能门控；另有一条独立的「启动时探测可恢复的外部会话」链路，welcome 屏在满足一串前提（当前视图是 Welcome、没有认证返回流、home 目录未用或为空、非 chat 模式、非 ZDR 阻断、没有待定更新）时才发起探测，并用 `foreign_resume_launch_generation` 令牌 + cwd canonicalize 做竞态防护（`src/app/foreign_sessions.rs:13-120, 204-260`）。
- **本机崩溃恢复**：`xai-grok-active-sessions` 维护 `~/.grok/active_sessions.json`（`{session_id, pid, cwd, opened_at}`），干净退出删条目、崩溃留下；下次启动 `collect_crashed()` 用 `kill(pid, 0)` 挑出死掉的 PID。它提供的是「进程级的会话占用事实」——**未找到它在 dashboard 中被直接渲染的证据**（`crates/codegen/xai-grok-active-sessions/src/lib.rs:1-3, 16-26, 175-206`）。

**与单会话 TUI 的切换与 peek/dispatch 互斥**。`ActiveView` 只有三态 `Welcome / Agent(AgentId) / AgentDashboard`；dashboard 不是独立渲染栈，而是同一个 `AgentView` 外加一层「从 dashboard 来」的铬标：`app.active_view` 决定画哪个，`app.dashboard: Option<DashboardState>` 保存全部状态（关闭视图不销毁状态），`dashboard.attached_agent: Option<AgentId>` 是**面包屑**（进入会话时设上，退出 dashboard 时清空）。进入要过四道门（功能关闭 / 未登录 / 同意书未答 / 目录信任未答），并记录 `dashboard_return`（`Agent(id)` / `Welcome` / `Overlay(id)`）；已在 dashboard 时再按一次等于退出。会话头部用 `[Dashboard]` 按钮与 `‹ i/n ›` 位置指示提供返回与循环（只有 1 个会话时不显示），**循环顺序 = 渲染器实际画出来的行序**，而不是 agents map 的插入序。细节视图内的 `Ctrl+X` 是状态相关的：turn 运行中 = 取消 turn；空闲 / 命令在飞 = 武装 2 秒确认关闭会话（`CONFIRM_WINDOW = 2s`）（`src/app/app_view.rs:206-243`、`src/app/dispatch/dashboard.rs:160-315`、`src/app/agent_view/render.rs:1341-1372`、`src/views/dashboard/mod.rs:57-79`）。

列表下方那块位置只有一个 owner：分配由 `layout::allocate_peek` 做 **list-first**——先保 `LIST_FLOOR_ROWS = 12` 行列表，剩余空间给 peek，peek 整体高度封顶 `⌊H × 3/8⌋`、内部 body 再封顶 28 行；压到 `PEEK_MIN_BOX_LIVE_TAIL = 8`（普通）或 `PEEK_MIN_BOX_QUESTION = 10`（有选项待答）以下就干脆不显示。peek 标题行是「最近一次响应类型 + 时间」，底部边框挂 model 与 `always-approve` / `auto` / `plan` 标志。预览面板可在 `/settings → Appearance → Dashboard preview`（`[ui] dashboard_preview`）关闭以换取列表空间，关闭后 `set_peek(None)` 并恢复 viewport（`src/views/dashboard/layout.rs:12-26, 36-150, 259-274`、`peek.rs:64-105`、`docs/user-guide/23-dashboard.md:95-101`、`src/settings/defs.rs:548-561`、`src/views/dashboard/preview.rs:31-40`）。

---

## 5. 交互逻辑

### 5.1 键盘模型与动作注册表

#### 本节要点

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

#### 本节要点

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

#### 本节要点

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

#### 本节要点

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

### 5.5 Agent 轮次交互：流式、插话、队列与取消

> 路径约定：本节与 5.6–5.8 中不带 crate 前缀的短路径一律相对 `crates/codegen/xai-grok-pager/`；其它 crate 在首次出现时写全路径。

#### 本节要点

- 一次轮次被切成四段：乐观本地回显 → prompt ack 窗口 → 流式运行 → 收尾；每段有独立状态与独立兜底（`app/prompt_ack.rs`、`app/dispatch/prompt_ack.rs`、`app/turn_completion.rs`）。
- 发送后**唯一被设界的是「确认（ack）」而不是 RPC**：软提示 10 s、硬放弃 120 s，且 `soft = PROMPT_ACK_SOFT_NOTICE.min(hard / 2)` 把两者强制耦合，软提示永不可能晚于硬超时（`app/prompt_ack.rs:14-24, 48`）。
- 运行中再次输入是**三分法**：普通 Enter 入队；Send now chord 取消当前轮并立刻发；真正的 `Action::Interject` 只由队列行编辑与计划评审意见产生（`actions/defaults.rs:497-511, 625-654`、`app/queue_edit.rs:509, 535`、`app/agent_view/plan.rs:498`）。
- **Esc 永不取消回合**，Ctrl+C 才取消；已在取消中再按 Ctrl+C 升级为退出。取消粒度只有 `Turn` 与 `Compaction`，**没有「取消当前工具」**（`app/agent_view/prompt.rs:747-821`、`app/agent_view/input.rs:1231-1247`、`crates/codegen/xai-grok-telemetry/src/events/turn.rs:38-43`）。
- 收尾是四件套而不是一句 toast：scrollback marker、终端通知、标题回 idle + 进度条清除、以及「仍有后台工作」的常驻计数行（`scrollback/blocks/session_event.rs:376-382`、`views/turn_status.rs:105-130`）。
- 全节贯穿一条纪律：**任何等待都不阻塞输入**，三套超时兜底一律跑在动画 tick 上（`app/event_loop.rs:2406-2430`）。

**一次轮次的四段时间轴（全节主干）。**

```
用户按 Enter
  ├─① 乐观反馈（本地，0 延迟）：用户气泡入 scrollback、composer 清空、draft 消费；不等任何网络往返
  ├─② Prompt ack 窗口（网络；session/prompt 自身无 RPC deadline）
  │    软期限 10s → spinner 文案改为「等待 agent 接受」；任一确认信号到达 → 撤销 watch、写 prompt.acked
  │    硬期限 120s → 本地放弃并还原文本
  ├─③ 回合运行中（流式）：工具块逐条实时更新；可插话 / 排队 / Send now / Ctrl+B 转后台
  │    Esc 不取消（只提示），Ctrl+C 才取消
  └─④ 收尾（TurnEnded）：落「Worked for X」marker + 终端通知 + 标题回 idle + 进度条清除
       queue 非空时抑制 TurnComplete 通知（下一条马上开始，避免闪烁）
```

轮次状态机只有 5 个值（`app/agent.rs:573-588`）：`Idle`、`TurnRunning`、`TurnCancelling`、`CommandRunning { command, started_at }`、`CommandCancelling { command }`。关键不变式：**driver 侧只有 `session/prompt` 的 RPC 响应能离开 `TurnRunning`/`TurnCancelling`**（`app/turn_completion.rs:481-483`），所有「广播先到、RPC 后到或丢失」都靠 tick 上的兜底补齐。

**Prompt ack 的预算与三种解除信号。**

`app/prompt_ack.rs:14-24` 的常量少到可以直接列完：软提示 `PROMPT_ACK_SOFT_NOTICE = 10 s`（`:16`）、硬期限 `DEFAULT_PROMPT_ACK_TIMEOUT = 120 s`（`:20`）、覆盖入口 `GROK_PROMPT_ACK_TIMEOUT_SECS`（`:14`）、钳制区间 `MIN/MAX_PROMPT_ACK_TIMEOUT_SECS = 5 / 3600`（`:22, 24`）。

120 s 的来源只写在注释里：要盖过 shell 首个 prompt 的最坏路径（模板化 delivery-tools 前缀 60 s + 10 s、一次设置刷新最长 16.5 s、一次 auth 刷新、两次 actor 往返），**仓库内没有实测分布数据（未找到证据）**（`app/prompt_ack.rs:17-19`）。解除信号恰有三种（`AckSignal`，`app/prompt_ack.rs:77-83`）：`x.ai/queue/changed` 的 `running_prompt_id` 命中或 `entries` 含该 id（`:129-135`）、`session/update` 携带该 `prompt_id`、以及回合结束（`mark_turn_finished` 里无条件调用，`app/agent_view/session.rs:499`）。chat 型会话**从不武装** watch，理由是「gateway bridge 没有 queue 广播，它的第一个信号就是第一个 delta」（`app/agent_view/session.rs:507-511`）。

**软提示不弹 UI，只换 spinner 的原因。** 软期限到达只做两件事：写一条 `prompt.ack_soft_notice` 日志（含 `waited_ms` / `limit_ms`）并请求重绘（`app/dispatch/prompt_ack.rs:97-108`）。可见变化发生在活动解析层——`resolve_turn_activity_unenriched()` 在 `TurnRunning` 且 `prompt_ack.is_soft_noticed()` 时返回专用的 `WaitingReason::PromptAck`（`app/agent_view/session.rs:1077-1084`）。该枚举是 view-only 值，tracker 从不存它（`app/acp/tracker.rs:70-72`），文案为 `Waiting for the agent to accept the prompt…`（`app/acp/tracker.rs:134`，遥测名 `waiting_prompt_ack` 在 `:146`）。软提示期间 `[stop]` 仍在——按钮按 running/cancelling 显示，不看 ack 状态。

**硬超时是「还原」而不是「丢消息」。** `fire_fail_safe()`（`app/dispatch/prompt_ack.rs:135-211`）先决定文本去哪儿（`restore_target`，`:118-131`）：

| 条件 | 结果 |
| --- | --- |
| composer 为 `PromptMode::Normal`、无文本无图片 | `ReplaceComposer`（放回输入框） |
| composer 有文本、无图片 | `MergeIntoDraft`（放在草稿**上方**） |
| composer 有图片 | `None` —— 两处图片来源都从 1 编号，`[Image #N]` 占位符会撞车（`:128`） |
| 正在编辑队列行（非 `Normal`） | `None` —— 写进去会污染那行 |

无可恢复目标时走 `NotRestorable`：结束回合、用户块留在 scrollback 供复制。收尾三连是 scrollback 通知 `The agent did not accept your prompt within {N}s. {recovery} If this keeps happening, run /feedback.`（`:24-40`）、toast（`Prompt not accepted, text restored` / `...turn stopped`，`:20-21`）与遥测 `PromptAckTimeoutFired { limit_ms, waited_ms, surface, disposition, prompt_kind }`（`:196-202`）。两个易忽略的细节：这条路径走 `finish_turn_view(agent, TurnEnd::Aborted)`（`:165`），所以**不产生 cancel latency 事件**，并显式清掉 `pending_cancel_resend`（`:166-167`）——取消一个 shell 从未运行过的 prompt 没有意义；另外 stale 守卫刻意让 `TurnCancelling` 继续保活 watch，注释写「取消中仍然武装，这样 wedge 期间按 Esc 不会让面板永远停在 Cancelling」（`app/dispatch/prompt_ack.rs:83-84`）。

**运行中再次输入：三分法（最容易被误解的一块）。**

| | 普通 Enter | Send now chord | 真正的 Interject |
| --- | --- | --- | --- |
| 键位 | `Enter` | 非 Apple `Ctrl+Enter`（alt `Ctrl+I`）；Apple Terminal `Ctrl+O`（alt `Ctrl+Enter`/`Ctrl+I`）；VS Code 家族 `Ctrl+L` | 无独立 composer 键位 |
| 是否取消当前轮 | 否 | **是** | 否 |
| producer | `dispatch/prompt.rs` 立即发或本地入队 | `app/agent_view/prompt.rs:555-597` → `Action::SendPromptNow` | `app/queue_edit.rs:509/535`、`app/agent_view/plan.rs:498` |
| 效果 | 入队，当前回合结束后按序 drain | 取消当前轮，新文本作为下一轮 | 合成一条 user message 插进**正在运行**的回合 |
| 证据 | `actions/defaults.rs:497-511` | `actions/defaults.rs:625-654` | `app/dispatch/interject.rs:13-118` |

这里是**一处需要并列保留的冲突**：`ActionId::InterjectPrompt` 的 `ActionDef` 内部自相矛盾（`actions/defaults.rs:625-654`）——`label: "send now"`、`description: "Send now while running (cancels the current turn)"`，而 `long_help` 却写「without cancelling it (interject)… this chord merges composer text into the current turn instead」。**代码站在 `description` 一边**：处理器在 composer 非空且 `can_send_now()` 为真时直接返回取消并发送（`app/agent_view/prompt.rs:555-597`）。composer 为空时退化为「强制发送队首可见行」（`try_send_now_queued_from_prompt`，`app/agent_view/queue.rs:83-110`）；`can_send_now()` 的定义是「turn 正在跑，或 wake turn 活着且未在取消」（`app/agent_view/session.rs:848-851`）。照抄 `long_help` 会得到错误的键位语义。

**队列语义：两层队列 + 乐观回声。** 队列有 pager 侧（`session.pending_prompts` 本地 drip、`shared_queue` 服务器镜像、`optimistic_queue_ids` 乐观回声）与 shell 侧（`state.pending_inputs` 的 `InputItem` FIFO）两层，靠 `x.ai/queue/changed` 对齐（`app/dispatch/queue.rs`）。「立即发到服务器队列」要同时满足全部条件（`immediate_server_send_eligible`，`app/dispatch/queue.rs:20-31`）：leader 模式 / `follow_up_steer` 打开 / 有 wake turn；**且** server busy；**且**有 session id；**且**本地 `pending_prompts` 为空（FIFO 守卫：本地非空时不许插队到服务器队列）；**且**不处于队列行编辑、不在切模型、不在 replay。不满足就走本地入队（`app/dispatch/prompt.rs:1214-1240`）。界面表现：队列面板 `Ctrl+;`（本地 mac VS Code 家族为 `Ctrl+4`，`actions/defaults.rs:547-560`；手册 `docs/user-guide/20-background-tasks.md:181`）；发送时先画乐观回声（`push_server_queue_echo`，`app/dispatch/queue.rs:35-54`）并登记 `optimistic_queue_ids`，未确认期间对该行按 Send now 会被**挂起**而不是发出去（真发会被 shell 超车然后 no-op，`app/agent_view/queue.rs:375-390`）；turn status 行只在 Enter 真能生效时才写 `· {N} queued, Enter to send now`，否则只写 `· {N} queued`（`views/turn_status.rs:489-495`，parked 分支同一逻辑 `:253-260`）。

**插话语义：回合内的合成 user message。** 核心实现放在共享 crate 供 shell 与 server 侧 agent loop 复用（`crates/common/xai-interjection-core/src/lib.rs:5-9`）。载荷统一由 `frame_user_turn` 生成（`crates/common/xai-interjection-core/src/format.rs`）：

（三段式：`The user sent a message while you were working:` / `<user_query>{用户文本}</user_query>` / `Make sure to complete any unfinished tasks from previous turns.`）

`INTERJECTION_NOTE`（`:4`）与 `INTERRUPT_NOTE = "The user interrupted the previous turn:"`（`:5`）共用同一信封（`:22-24`）；末尾的 unfinished-tasks 提醒是刻意的，防止中途 steer 或取消后跟进导致在飞任务被丢下（`:7-9`）；超过 `LARGE_PROMPT_THRESHOLD = 25_000` 按 UTF-8 边界截断并追加 `... [truncated]`（`:2, 27-37`）。排空只发生在「安全点」——一个工具批次之后、循环顶部、以及回合返回用户之前（`crates/codegen/xai-grok-shell/src/session/acp_session_impl/interjection.rs:279-301`）；同一函数若发现 `follow_up_steer_enabled()` 为真且队里还有可编辑的人写行，会先 `promote_queued_as_interjections()` **把队列行提升为插话**——这就是「队列 vs 插话」在配置层面的开关。排空实现用 `RestoreOnCancel` 守卫先接管全部 entry，逐个 sanitize（剥掉 `[Image #N: <path>]` 里的路径）、解析 `/skill` 引用、包装、走图片管线，然后**一次性** `try_push_user_messages_batch` 提交，成功后才落盘；守卫的 `Drop` 调 `EventQueue::restore_front()` 把未提交项放回**队首**（`crates/common/xai-interjection-core/src/events.rs:75-82`）。

**落空不丢消息。** 插话只在运行中合并；若落地时会话已空闲、或错过本回合最后一次排空，就转成独立回合，prompt id 前缀 `interject-fallback-`（`acp_session_impl/interjection.rs:17-24, 30-83`），并按原顺序推到队列**最前**（`flush_stranded_interjections`，`:87-96`，用反向 `push_front` 保持 entry 0 在前）。pager 侧（`app/dispatch/interject.rs:13-118`）立即推一个本地用户块（`RenderBlock::interjection_prompt`），把 id 记入 `self_interjection_ids` 以丢弃广播回声，同时记 `interjection_painted_blocks` 与 `interjection_retry_images`，弹 toast `Interjection sent`，最后返回 `Effect::SendInterject`。一条硬约束写在测试注释里：**dispatch 永不触碰 composer**，只有 composer 路径的调用点自己清空（`app/dispatch/interject.rs:216-218`，测试 `interject_dispatch_never_touches_the_composer`）。

**Send now：唯一同时产生「取消」和「新消息」的路径。** `dispatch_send_prompt_now()`（`app/dispatch/interject.rs:122-205`）需要三处配合：断连守卫（`reconnect_pending` 时把文本 requeue 到本地队首并 toast `Reconnecting, please wait...`，而不是打进死通道 `:145-163`）；**静默取消**（`arm_send_now_and_paint_dispatched()` 让取消的 `TurnCancelled` marker 被抑制——`TurnStopReason::Cancelled if input.send_now_cancel => None`，`app/turn_completion.rs:95`，注释称之为「cancel-and-send 的静默那一半」）；以及乐观队列回声 + `Effect::SendPromptNow`（`:190-204`）。

**取消：Esc 不取消是刻意的。** `try_handle_esc_policy()` 的运行中分支（`app/agent_view/prompt.rs:747-821`）直接提示正确键名而不取消，三个设计点：提示里的键名**从 registry 现取**（`registry.key_for(ActionId::CancelTurn)`），用户重绑定后提示跟着变（`:773-778`）；同一个 Esc 顺便把 rewind-arming 推迟 `ESC_CANCEL_REWIND_GRACE = 1000 ms`（`:738-741, 781`），避免「回合即将结束时空按 Esc，下次按就立刻武装双击 rewind picker」；minimal 模式没有 toast 槽位，提示改写成 scrollback 里的 system 块且**每个用户回合最多一条**（`show_cancel_key_hint`，`:824-836`）。`ActionId::CancelTurn` 的 `long_help` 把策略写成正式契约（`actions/defaults.rs:508`）：*Ctrl+C cancels when the prompt is empty; with a non-empty draft it clears the prompt first and leaves the turn running. **Esc never cancels a turn**.* 已在取消中时 Esc 被**静默吞掉、连提示都不给**——那时 Ctrl+C 已升级为退出，再提示 Ctrl+C 会误导（`app/agent_view/input.rs:2276-2285`）。空闲时 Esc 是另一套「双击」语义（有草稿 → 武装 `ClearPrompt`；空且有历史 → 武装 `RewindShowPicker`），窗口 `PendingAction::ESC_DOUBLE_PRESS_TTL = 800 ms`（`app/app_view.rs:476`），可用 `GROK_ESC_DOUBLE_PRESS_MS` 覆盖（上限 `ESC_DOUBLE_PRESS_TEST_MS = 60_000`，`:501-514`）。

**取消粒度与延迟指标。** `CancellationScope` 只有两个值（`crates/codegen/xai-grok-telemetry/src/events/turn.rs:38-43`）：`Turn` → `session.cancel_turn()`、`Compaction`（`state.is_compact_running()`）→ `session.cancel_compact_command()`；两者由 `cancel_and_arm(scope, origin)` 一个入口分派，保证「动作」与「它测量的 scope」不漂移（`app/agent_view/session.rs:543-554`）。`/compact` 是唯一被当作独立 scope 的命令，测试确认 `/jump` 打开时 Ctrl+C 仍能取消 compact 而不是被 picker 吃掉（`app/agent_view/input.rs:2334-2351`）。延迟指标先 anchor 后结算：只有 `CancelOrigin::UserGesture` 且 `surface() == ViewSurface::Root` 才设 anchor（`Programmatic` 标了 `#[allow(dead_code)]`），`TurnEnd::Completed` 才产出 `CancellationCompleted { latency_ms, scope }`（遥测名 `cancellation_completed`，`crates/codegen/xai-grok-telemetry/src/events/mod.rs:289`），`Aborted`（reload / fork / 会话失败强制置闲）**直接丢弃**；anchor 与消费都 `take()`，所以每个事件最多上报一次（`app/cancel_latency.rs:8-25`、`app/agent_view/session.rs:550-571`）。

**二次按下与升级：Ctrl+C → Ctrl+C = 退出。** 默认键 `Ctrl+C`，且**只在 `When::AgentScreen` 层注册**——scrollback 层与全局层都查不到它（`actions/mod.rs:785-796` 的测试 `cancel_at_agent_level`），这条注册边界本身就是「取消是 agent 屏幕级动作，不是全局退出」的表达。处理器逻辑（`app/agent_view/input.rs:1231-1247`）：活动在跑 → `Action::CancelTurn` 并记 `cancel_trigger_hint`；已在 cancelling → 返回 `Action::Quit`；minimal 模式空闲且草稿为空 → `Quit`。同一升级在 wake turn 上也被测试锁定（`:2376-2394`）。

**取消前的闸门：子代理还在跑怎么办。** 触发取消时若仍有子代理在运行，会先弹一个 4 选项面板（`views/modal.rs:120-145, 899-960`）：

（面板形如 `Subagents are still running. Stop them?` + 四选项 `1 (●) Stop running` / `2 (○) Continue to run` / `3 (○) Always stop` / `4 (○) Always continue`。）

标题带子代理计数行 `1 subagent running` / `N subagents running`（`:906-912`）；前两项只影响这次取消，后两项写全局/会话偏好 `cancel_subagents_on_turn_cancel`（`app/dispatch/turn.rs:230-249`）；键盘走 `app/agent_view/interactions.rs:224-300`，鼠标按行 hit-test。

**取消后的界面状态与两条兜底。** `cancel_agent_turn()`（`app/dispatch/turn.rs:253-349`）的顺序是：compact 在跑 → 走 `Compaction` scope；wake turn 在跑 → `mark_wake_cancel_sent()`（wake turn 从不被 adopt，取消状态挂在 `running_wake_turn` 而不是 `AgentState`，`:526-527`）；非运行 → 直接返回；否则先评估 rewind（`cancel_rewind_enabled && in_flight_prompt` 存在 `&& !queue_held_behind_turn && !in_flight_committed && !composer_has_draft && rewind_prompt_id.is_some()` 全真时，把提示文本**收回 composer**，界面看起来像「用户从没按过发送」）；收尾清 cancel panel、`drain_permission_queue`（每个挂起权限各得一个 Cancelled）、`dismiss_in_turn_plan_review`、`clear_send_now_expectation`。注释还明确一条克制策略：交互式取消**只拆当前回合**，不把队列里的 prompt 拉回输入框、也不预测新队列顺序，让 agent 自己把队首提升为下一轮（`:126-128`）。两条兜底都跑在动画 tick 上，顺序是 cancel → prompt ack → lost response（`app/event_loop.rs:2406-2430`）：取消重发（fire-and-forget 的 `session/cancel` 会丢）用 `CANCEL_RESEND_GRACE = 3 s` + `CANCEL_RESEND_MAX_ATTEMPTS = 3`（`app/dispatch/turn.rs:502-506, 510-564`），丢失 RPC 响应的回合收尾用 `TURN_END_RECONCILE_GRACE = 2 s`（`app/dispatch/turn.rs:566-569, 574+`）。

重发有一条精细抑制：一旦 `pending_turn_end_reconcile` 已武装（说明收到了 `prompt_complete` 广播）就**不再重发**，否则会竞争甚至取消掉 shell 已经提升的队列 prompt（`:533-540`）。

**取消回执归因。** 文案由 `_meta.cancelTrigger`（`"send_now" | "ctrl_c" | "mouse" | "dashboard_stop" | "session_close" | "session_delete" | "shutdown" | "host_interrupt"`）与 `_meta.cancellationCategory`（`HookDenied` / 权限拒绝 / 权限忽略 / 达到回合上限）拼出，另有 `_meta.completionKind: RemovedFromQueue` 用来区分「排队但从没跑过」（`app/turn_completion.rs:22-27`）。解析顺序是 **category 优先、trigger 兜底**（`scrollback/blocks/cancel_cause.rs:24-39`），得到 8 种 `CancelledBy`：`User`（`Turn cancelled by user`）、`SessionClosed`、`Shutdown`、`MaxTurns`、`PermissionDenied`、`PermissionDismissed`、`HostInterrupt`、`Unspecified`（短语表 `:41-52`）。值得注意的是 wire 上的 `"esc"` 也算用户手势（测试 `:61` 遍历 `["esc","ctrl_c","mouse","dashboard_stop"]` 全部映射到 `User`）——这是给其它前端留的兼容位，pager 自己不发它。pager 侧的 `CancelTrigger` 只有三个值：`CtrlC` / `Mouse` / `DashboardStop`（`app/actions.rs:1154-1171`），随 `session/cancel` 的 `_meta.cancelTrigger` 上报，让 agent 的 `mid_turn_abort` 遥测能区分手势（`app/actions.rs:1576-1580`）。

**收尾反馈的四件套。** pager 从两条轨道得知回合终结——fire-and-forget 的 `x.ai/session/prompt_complete` 广播，与持久化、会写盘并重放的 `XaiSessionUpdate::TurnCompleted`——最终汇聚到 `finalize_turn_from_terminal()`（`app/turn_completion.rs:1-7, 484-552`），分流规则是 driver（`!attached_as_viewer`）不在这里结束回合（`PromptResponse` RPC 拥有生命周期），viewer 则 adopt 并自行终结，这样「回合中途重新挂上的 viewer」能从重放里终结回合而不是永远卡在 Waiting。

- **scrollback marker**（唯一文案来源 `scrollback/blocks/session_event.rs:376-382`）：`Worked for 1m20s`（故意**不加句点**）、`Turn completed.`、`{cause} in 1m20s.`、`Turn blocked by a hook in 1m20s.`；三种情况**不推标记**（`app/turn_completion.rs:87-109`）：send-now 的静默取消、`RateLimit`、以及已有错误横幅的 `Error`。
- **终端通知**：5 种事件类型（`notifications/config.rs:100-115`），默认只开 `TurnComplete` 与 `ApprovalRequired`（`:30-33`）；编码 `NotificationMethod`：`Auto`（默认）/ `Osc9` / `Osc99` / `Osc777` / `Bel` / `None`，触发条件 `NotificationCondition`：`Unfocused`（默认）/ `Always` / `Never`（`:44-63`）。注意 **`Bel` 是通知编码方式之一，而不是独立的「响铃」功能**——仓库里没有找到独立的声音/蜂鸣逻辑（未找到证据）。**队列非空时抑制 TurnComplete 与 idle escapes**（下一轮马上起来，会覆盖标题造成闪烁），并把「已 stash 的服务器权威 adoption」当作队列非空处理（`app/dispatch/prompt.rs:1724-1744`）；通知**故意延迟 3 帧（≈99 ms）**，因为 Ghostty 对 `setTitle()` 有 75 ms 防抖，立刻读标题会读到旧值（`:1753-1773`），倒计时在每 tick 递减、归零才 `notify()`（`app/app_view.rs:5495-5503`）。
- **标题与进度条**：`TitleItem` 有 8 种（`notifications/config.rs:87-98`），默认顺序 `[ActionRequired, Spinner, Activity, SessionName, Grok]`（`:72-85`），`ActionRequired` 只在有待处理权限时出现并**跨 tick 闪烁**（测试 `title.rs:562-616`）；进度条走 OSC 9;4，`PROGRESS_KEEPALIVE = 5 s`（Ghostty 约 15 s 静默后自动复位，故需定期重发），空闲时 `build_idle_escapes()` 清进度并把标题改回 idle（`notifications/mod.rs:14-16, 118-124, 140-179`）。
- **后台仍在跑的常驻提示**：`◎ 1 command · 2 monitors · 1 loop · 1 subagent still running`（`views/turn_status.rs:105-130`、`docs/user-guide/20-background-tasks.md:187-201`），`0` 的种类不列出、单数不加 `s`；这是**唯一**展示运行计数的地方；等待型 blocked 状态追加 `· send a message to interrupt`（`:253-262`）；脉冲图标 `○ ◎ ◉ ◎` 用 `MONITOR_PULSE_DIVISOR = 8`，约为活动 spinner（`SPINNER_DIVISOR = 4`，~7.5 fps）的一半速度，注释理由：idle 提示应当「呼吸」而不是被读成活动中的 spinner（`:29-36`）。

**「一切不阻塞输入」的设计手法（8 组）。**

| # | 手法 | 证据 |
| --- | --- | --- |
| 1 | 乐观本地回显：插话、Send now、服务器队列行都在 dispatch 时画出，不等服务器确认 | `app/dispatch/interject.rs:76-89`、`app/dispatch/queue.rs:35-54` |
| 2 | 草稿 stash & restore（卡片、队列编辑、外部编辑器轮转），加取消安全：被 abort 的排空把未提交项放回队首、乐观队列行在 RPC 落定后退役 | `app/agent_view/elicitation.rs:2`、`app/queue_edit.rs:6-7`、`crates/common/xai-interjection-core/src/events.rs:75-82`、`app/dispatch/queue.rs:56-60` |
| 3 | 阻塞式交互做成卡片而非阻塞循环，用 `KeyOwner` 优先级表仲裁键盘；卡片挡住键盘时仍留 `Tab`/`Space` 交还路径，Esc 分级后退 | `app/agent_view/key_owner.rs:18-36, 38-53, 55-60` |
| 4 | 误触防护：Esc 不取消（只提示正确键名），Ctrl+C 取消，再按升级为退出 | `app/agent_view/prompt.rs:772-782`、`app/agent_view/input.rs:1231-1247` |
| 5 | 插话落空不丢消息：错过则转 `interject-fallback-` 独立回合并推到队首 | `acp_session_impl/interjection.rs:17-24, 87-96` |
| 6 | 三套超时兜底都跑在动画 tick 上，失败也不阻塞输入：ack 120 s、取消重发 3 s×3、丢失响应 2 s | `app/prompt_ack.rs:20`、`app/dispatch/turn.rs:503-506, 569`、`app/event_loop.rs:2406-2430` |
| 7 | 通知 / 标题 / 进度走 out-of-band 队列不等帧管线（帧可能被 ack 挂住），但 idle 标题转义仍走帧管线以保序；tick 需求按需升降频 | `notifications/mod.rs:45-46, 93-95, 118-124`、`app/app_view.rs:5659-5661`、`app/status_line_policy.rs:47-60` |
| 8 | 长输出用覆盖式单缓冲 + `BG_TASK_MAX_STDOUT = 10 MiB` 上限 + 单向 `truncated` 闩锁 | `app/acp_handler/background.rs:19-53`、`app/agent.rs:153, 218-235` |

两处与「等待」相关的具体机制值得单列：`Ctrl+B`（`ActionId::SendToBackground`，label `send to bg`，`actions/defaults.rs:611-621`）把正在跑的前台命令转后台（手册补一条容易忽略的：**发一条新消息也会把该命令转后台而不是杀掉它**）；当回合阻塞在「可被用户打断的等待」上时，发消息会立即打断等待并运行你的消息，判定 `is_parked_on_sendable_wait()` 要求 `is_sendable_wait(activity)` 为真**且** goal 循环未激活——因为 shell 在 goal 运行期会抑制这个 abort，把它们当成可打断会说谎（`app/agent_view/queue.rs:105-118`）。子代理「先完成、后 spawn」的生命周期错位则用一个有界缓冲处理：`MAX_DEFERRED_SUBAGENT_FINISHES = 256`、`DEFERRED_SUBAGENT_FINISH_TTL = 60 s`，`defer()` 时把 `SubagentFinished` 的 `output` 置空再存，真正的输出在 `take()` 时再取（`app/deferred_subagent_finishes.rs:10-11, 82-84`）。

### 5.6 会话生命周期与终端状态管理

#### 本节要点

- 首屏之前的全部工作都是**串行 await**，唯一有名字的等待预算是 `EARLY_PREFETCH_WAIT = 2 s`；最慢的一步是 `materialize_startup()`（含远端恢复 ≤90 s）。没有 splash、没有进度条实现（未找到证据）（`app/mod.rs:550, 663-1062`、`app/session_startup.rs:1267-1276`）。
- 终端接管是一张**一次性副作用清单**，顺序即语义：raw mode 最先，panic hook 与信号处理器紧随其后安装，光标样式默认 `Inherit` 不发转义（`app/mod.rs:1459-1661`）。
- 「向终端写什么」只有**一个真源** `emit_terminal_teardown_sequences`（且**不含** `disable_raw_mode()`），而「谁在什么条件下调它」有三条路径：事件循环后、panic hook、信号（`app/terminal_restore.rs:61-134, 143-185`）。
- 所有等待都有界：writer join 2 s、reader join 250 ms、DA1 fence 1 s、exit 看门狗 20 s + 5 s（`app/terminal_restore.rs:27`、`app/reader_thread.rs:16`、`app/teardown_fence.rs:37-48`、`app/exit_timeout.rs:8`）。
- 历史重放有专门的「延迟渲染」通道与 `SessionLoaded` 栅栏：本会话 replay **不设超时**，无关流量 2 s 放行（`app/acp/tracker.rs:1132-1136`、`app/session_load_barrier.rs:26, 174-180`）。
- 屏幕模式可在进程内切换（失败即回滚），exec 热重启只是兜底；Unix 用 `exec` 保住 PTY 身份（`app/mode_switch.rs:108-207`、`app/screen_mode_relaunch.rs:211-217`）。

**启动序列：串行阻塞段与一次性副作用。** `main()` 先处理三件与 TUI 无关的事：mermaid / voice 的「子进程自执行」旁路、`PagerArgs::parse_cli()`、版本与 doctor 的早退；`/export`、`/share`、`sessions` 等子命令在这里分流，随后才调 `xai_grok_pager::app::run(args, bg_update_rx)`（`crates/codegen/xai-grok-pager-bin/src/main.rs:2025, 2028-2033, 2315-2319, 2519`）。首屏之前没有 splash，只有 stderr 上的行输出：

```
app::run()                                     [app/mod.rs:663]
  ├─ ① 串行阻塞：一次性 GROK_SCREEN_MODE → 读配置(两次) → pre-TUI 登录 → redirect_native_stderr
  │     → auth 刷新 → remote settings 预热(join 最长 EARLY_PREFETCH_WAIT = 2s)
  │     → materialize_startup()（最慢一步，含远端恢复 ≤90s）→ 屏模式解析 + 主题预热
  ├─ ② 终端一次性副作用：spawn writer 线程 → init_terminal()      [app/mod.rs:986-1001]
  ├─ ③ bounded_connect（100 ms 切片；leader 失败自动回退 embedded）
  └─ ④ event_loop::run()   ← 首帧 request_presentation()；之后才 Action::LoadSession 灌历史
```

连接失败不是「黑屏挂住」：`bounded_connect`（`app/mod.rs:576-659`）把等待切成 100 ms 片以支持 managed profile 升级预算，超时后走 `restore_terminal(...)` 归还终端再把错误冒出去（`app/mod.rs:1044-1122`），由 `startup_failure` 模块打印可读报告。**加载态由三样东西承担（没有全屏 splash，未找到证据）**：欢迎屏先建一个乐观 home 会话（代码称 *husk*，`app/dispatch/session/lifecycle.rs:431, 855`）让用户不必等后端就有输入焦点；远端恢复 / git 校验进度走 `emit_pre_tui_restore_line` 打在 TUI 接手之前的终端上（`app/session_startup.rs:1267-1276`）；失败才出现专属界面。reader 线程在 typeahead 回放之后才 spawn（`app/event_loop.rs:1612-1625`），保证用户抢在加载期敲的字排在实时按键之前；若当前停在登录 / 信任 / 付费墙屏，这些事件会被丢弃而不是被误答（`:166-177`）。

**`init_terminal()`：一次性副作用清单（`app/mod.rs:1459-1661`，顺序即语义）。**

1. `enable_raw_mode()`（`:1468`）——**最先**，之后任何失败都必须走 teardown；
2. `EnterAlternateScreen`（仅 fullscreen，`:1488-1492`）；
3. `EnableMouseCapture`（**非 minimal** 才开，`:1497-1502`）；minimal 下若检测到「终端把鼠标报告当文本漏出」（JediTerm / Windows）则额外发一次 `MOUSE_TRACKING_RESET`；
4. `EnableFocusChange` + `EnableBracketedPaste` + `cursor::Hide`（`:1503-1508`）；
5. 光标样式三态 `CursorStylePolicy::{Inherit, ForceBlinking, ForceSteady}`（`:1430-1446`）——默认 `Inherit` 不发任何样式转义，避免覆盖用户 shell 的选择；只有强制过才置 `CURSOR_STYLE_FORCED`，teardown 因此知道该不该复位（`:1523`）；
6. `set_panic_hook()` 与 `signal_handler::install(mode)`（`:1528-1529`）——**在 raw mode 之后立刻**安装；
7. 收集「启动期打字」`startup_typeahead`（`:1474-1476, 1530-1535, 1569-1572`）：raw mode 打开后，在 20 ms（有 `VTE_VERSION` 时）/ 0 ms 的静默窗口里排空 stdin；
8. 协商 kitty keyboard protocol（`:1537-1568`）：失败 / 不支持则跳过，成功则 push flags 并记 `set_pushed_kitty_flags`；
9. 建 viewport（`:1573-1648`）：fullscreen 用全屏 Terminal，否则 `Viewport::Inline(rows)`。**minimal 的探针失败会降级**：改用 `Viewport::Inline(rows)`、重新打开鼠标捕获、把有效模式改成 `Inline`，随后 `finish_theme_after_probe` 补上被跳过的主题解析（`:1606-1625`、`:410-418`）。

任何一步出错都走 `.inspect_err` 分支：`emit_terminal_teardown_sequences` + `disable_raw_mode` + `signal_handler::mark_restored()`，**不**走完整 restore（此时还没有 reader / writer 消费者）（`:1650-1655`）。窗口标题也在这里设置，并强制清洗控制字符与截断：`SetTitle` 的字符串是裸放在 OSC 里的，内嵌 BEL / ESC 会提前终结 OSC 并注入任意转义序列（`:1662-1679`）。

**会话恢复：意图解析三层 + 历史重放的三个要点。** 启动意图收敛成 `SessionStartupIntent` → `materialize_startup()` → `MaterializedStartup`：`Resume` / `Fork` / `NewWithId` / `NewAuto`（`app/session_startup.rs:184, 685, 849`；`app/mod.rs:818-824`）。`Resume` 分三层解析（`resolve_existing_session`，`app/session_startup.rs:982`）：本地 id 命中即用 → 标题命中走 `select_by_title`（简单小写比较；多个同名时**唯一的手动 `/rename` 胜出**；仍有歧义则报错并列出 id，**绝不静默挑一个**；UUID 形状的参数永远走 id 路径）→ 都没命中按 `RemoteMissPlan` 决策（`app/session_title_resolve.rs:12-14, 35-76`；`app/session_startup.rs:1086-1113`）。

事件循环在首帧之后才发 `Action::LoadSession`（`app/event_loop.rs:1714-1727`），随后 ACP 回放带 `_meta.isReplay = true` 的通知（`app/acp/meta.rs:29, 45-51`）。三个实现要点：

- **重放全程用「延迟渲染」变体**，避免每条 chunk 都跑一遍 markdown：`is_replay` 走 `push_chunk_to_agent_deferred`（`app/acp/tracker.rs:1132-1136`；`scrollback/blocks/markdown_content.rs:135-142` 的注释写明「batch markdown work and render once after replay completes」）。
- **重放的 thinking 块用另一个构造器** `streaming_replay()` 把 `started_at` 置 `None`，否则本地壁钟计时器会冻成 ~0 ms，渲染出假的 `Thought for 0.0s`（`scrollback/blocks/thinking.rs:152-160`）。
- **`SessionLoaded` 要等回放排空才派发**（`app/session_load_barrier.rs` 整篇就是这件事）：队列头是本会话 replay → 一直等、**不设超时**；头是无关会话的流量 → 只在事件循环有余力 drain 时累计秒表，超过 `SESSION_LOADED_ACP_BARRIER = 2 s` 放行（输入饿死时秒表不走，避免恢复被卡住）；头是本会话的 live → 立即放行，因为 leader 模式保证线序 `[unicast replay] → [load response] → [buffered live]`（`:3-8, 26, 174-180`）。

状态不一致时的处理：标题歧义 → 报错 + 列候选 id；本地找不到且允许远端 → 远端恢复，**只恢复会话状态与记忆，从不恢复代码**；远端超时但已有本地子会话 → 降级为继续用本地会话并明确告知；`--restore-code` 但无 `--worktree` → 拒绝原地检查代码快照（`RejectInPlaceCodeRestore`）；欢迎屏留下的乐观空会话 → `abandon_unused_empty_for_load` 丢弃 husk 但**有草稿就保留**；恢复把上一条「还在跑」的任务行读回来 → 有专门清理函数（`app/session_startup.rs:1102-1112, 1240-1256, 1264-1282`；`app/dispatch/session/lifecycle.rs:825-853`；`app/dispatch/turn.rs:834-835`）。

**退出：单一真源的 teardown 序列。** `app/terminal_restore.rs:61-134` 的注释明说 *「So the on-wire byte order is defined exactly once.」* 它**不含** `disable_raw_mode()`——raw mode 必须留到所有读操作之后才关：

```
OSC 9;4 清除进度条（无条件；终端不支持时是无害空操作）
EndSynchronizedUpdate (?2026l)
reset cursor color（仅本进程真正设置过时）
写 MOUSE_PASTE_RESET 裸序列: ?1000l ?1002l ?1003l ?1015l ?1006l ?2004l
   └ 若 MOUSE_CAPTURE_ENABLED 曾置位：Windows 额外补一次 DisableMouseCapture（winapi）
DisableFocusChange (?1004l)
pop /gboom 额外键盘层（若游戏开过，且它叠在基础层之上）
PopKeyboardEnhancementFlags (CSI<u)          ← 用 swap 保证至多一次
分屏模式收尾：
  fullscreen : [SetCursorStyle::DefaultUserShape 若强制过] → cursor::Show → LeaveAlternateScreen
  inline     : [同上] → MoveTo(0, inline_cursor_row) → cursor::Show → 换行
Windows: restore_stdin_mode()（最外层快照，放最后）
```

这组模式位是**规范清单**，写在 `crates/codegen/xai-crash-handler/src/terminal.rs:6-46`，注释要求「每条 teardown 路径都必须禁用同一组模式」，并给出两个常量：`MOUSE_PASTE_RESET`（`:38-39`）与完整 `RESTORE_SEQ`（`:44-46`）；其中 kitty 的 `CSI<u` pop 排在 `?1049l` **之前**，因为「协议栈是按屏计数的」。**inline 的光标落点是专门的坑**：minimal 视口不贴底，把光标移到屏幕最后一行会让 shell 提示符与续跑提示和 pager 最后一行隔一整屏空白，所以传入的 `inline_cursor_row` 取 `terminal.viewport_area().bottom()` 并被 `min(last)` 夹住；信号路径拿不到 terminal 句柄，只能回落到屏幕底行（`app/terminal_restore.rs:112-127, 163`；`app/signal_handler.rs:229-230`）。

**三条调用路径的差异，以及「所有等待都有界」。**

| 路径 | 入口 | reader join | 排空 writer | fence | 有界保护 |
| --- | --- | --- | --- | --- | --- |
| 事件循环后（正常退出） | `app/mod.rs:1169-1174` 调 `restore_terminal` | 是（250 ms） | 是（2 s） | 是 | writer 超时才用 `run_bounded_teardown` |
| panic hook | `app/terminal_restore.rs:243-258` | 否 | **否**（会死锁） | 否 | 整个 teardown 放 helper 线程 + 2 s 上限 |
| 信号 | `app/signal_handler.rs:219-239` | 否 | 否 | 否 | 无（靠 exit 看门狗兜底） |

`restore_terminal_with`（`app/terminal_restore.rs:143-185`）把顺序固定为：**join reader → fullscreen 下 clear → 抓取 inline 落点 → drain writer → teardown → fence（仍在 raw mode）→ `disable_raw_mode()` → `mark_restored()` → 恢复 fd 2**。`mark_restored()` 之后到达的信号就不会再往用户的提示符上画转义序列（`app/signal_handler.rs:84-88`）。四个有界值：

- **writer join `WRITER_JOIN_GRACE = 2 s`**：超时**不报错**，而是记 `term.writer.join_timeout` 并把线程**脱钩**；后续所有 stderr 写都可能阻塞，所以之后的 teardown 必须走 `run_bounded_teardown(..., 2 s)`，否则「终端不再读」会让 `/quit` 变成永久挂起（`app/terminal_restore.rs:27, 32-46, 138, 169`）。
- **reader join `READER_JOIN_GRACE = 250 ms`**：读线程用 `poll(20 ms)` 再 `read()` 而非裸阻塞读，这样「暂停标志」与「通道关闭」最多一个轮询周期就被观察到；`join_within` **从不 join 一个还在跑的线程**，超时即 detach 并返回 `ReaderJoin::TimedOut`（`app/reader_thread.rs:16, 68-72, 103-123`）。
- **crossterm 解析垃圾**：连续 50 次读错误才退出 reader，否则只 warn（注释点名 ratatui#1275 与 VTE/SSH PTY 的脏输入，`app/reader_thread.rs:81-94`）。
- **exit 看门狗**：优雅退出武装后 20 s 兜底（`DEFAULT_EXIT_TIMEOUT`），超时后再等 5 s（`HARD_EXIT_GRACE`）执行 `libc::_exit`（async-signal-safe 且跳过 atexit）；`GROK_EXIT_TIMEOUT_SECS` 可调、**`0` 表示关闭**，`GROK_TEST_HOLD_TEARDOWN_SECS` 用于测试里故意拖住 teardown（`app/exit_timeout.rs:8, 53-71, 73-79`）。

**最后一道闸门：kitty pop fence（DA1 往返）。** 这是整套设计里最精巧的一环，位于 `crates/codegen/xai-grok-pager-render/src/terminal/pop_fence.rs`。要解决的问题是：终端按顺序处理输出，所以当 `CSI ? c`（DA1）的回复到达时，终端在应用 `CSI < u`（pop）之前发出的**所有按键报告都已经躺在 stdin 里**；不排空它们，这些 key release 事件会被用户 shell 当成击键——注释举的例子是 **fish 把 `ESC [ 100 ; 5 : 3 u` 读成 Ctrl-D 然后退出**。读取用裸 fd，因为 crossterm 会把 DA1 回复从事件流里过滤掉。

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
| 其余 | `Run`（`POP_FENCE_TIMEOUT = 1 s`，注释称取自 neovim 的实战值） |

只要 fence 没真跑或结果是 `Unsupported` / `QueryFailed`，就退回旧式的「静默排空」`drain_pending_events(10 ms)`；结果统一记到 `teardown.kitty_pop_fence`，字段含 `decision` / `outcome` / `residue_bytes` / `reader_join`，用**直写**通道发出（启动失败时 ACP forwarder 没有发送端）（`app/teardown_fence.rs:13-16, 60-63, 78-95`）。

**信号策略。** 模块注释把动机写全了：没有这些处理，SIGINT / SIGTERM / SIGHUP（WSL/SSH 断开）会留下 raw mode + 备用屏 + 鼠标捕获 + kitty flags，终端看起来「还活着」但进程已死（`app/signal_handler.rs:1-16`）。要点：信号在 **tokio task** 里处理，不是真正的异步信号上下文，所以可以安全地走完整 `emit_terminal_teardown_sequences`（含加锁 stderr、条件复位光标样式）；**首个信号 = 优雅退出**（走和 `/exit` 一样的收尾）、**第二个 = 立即强制退出**（`:90-135, 198-209`）；`SIGTTIN` / `SIGTTOU` 显式忽略，避免子进程（或孙进程）`tcsetpgrp()` 抢走前台进程组时整个 pager 被挂起而留下 raw mode 终端（`:75-79`）；**`SIGPIPE` 故意不装处理器**，保持 Rust stdlib 的 `SIG_IGN`——自定义处理器会把 fork+exec 子进程（MCP server、hook）继承的 disposition 从 `SIG_IGN` 改成 `SIG_DFL`，重新引入旧回归（`:12-16`）；退出码与 agent 二进制共享同一函数（Ctrl+C `130`、SIGTERM `143`、SIGHUP `129`，`:137-150`）；顺序上**先还原终端，再 flush Sentry**（最长 2 s），否则用户要盯着一个 raw + 备用屏的终端等 2 秒（`:216-218, 230-231`）；Windows 分支不同（`:103-124`）——Ctrl+C 享受同样的「首次优雅 / 二次强制」，Ctrl+Close / Logoff / Shutdown 则**立即**处理，因为系统只给很短窗口。

**明确的失败模式清单。**

| 失败模式 | 应对 | 证据 |
| --- | --- | --- |
| 终端停止读取输出（SSH 掉线等） | writer join 2 s 超时 → 脱钩；teardown 有界化；最终由 20 s 看门狗 `_exit` | `app/terminal_restore.rs:27, 143-185`；`app/exit_timeout.rs:8` |
| stdin reader 卡在 `read` | 250 ms 有界 join → detach，fence 放弃（`SkipReaderAlive`），退回 10 ms 静默排空 | `app/reader_thread.rs:103-123`；`app/teardown_fence.rs:37-48` |
| panic / panic=abort | hook 在 helper 线程跑有界 teardown（2 s），并主动 `kill_all` 子进程、记录崩溃采样；退出提示等打印全部 best-effort（写失败即忽略） | `app/terminal_restore.rs:243-258`；`app/mod.rs:1236-1238` |
| 重复 teardown（panic + 信号、或双信号） | `TERMINAL_OWNED` 布尔 + kitty flags 的 `swap` + Windows stdin 模式快照的 `swap` 保证至多一次 | `app/signal_handler.rs:60-88, 232-233`；`app/mod.rs:111, 1378-1390` |
| 崩溃路径（连 panic hook 都没跑完） | `xai_crash_handler` 的 `RESTORE_SEQ` 用裸 `write(2)` 写 fd 2 | `crates/codegen/xai-crash-handler/src/terminal.rs:44-60` |
| SIGKILL | **未找到证据**表明有处理；信号任务只订阅 SIGINT / SIGTERM / SIGHUP | `app/signal_handler.rs:90-135` |

**屏幕模式：三种模式、进程内切换与 exec 热重启。** `ScreenMode` 三个变体（`app/mod.rs:340-349`）：`Fullscreen`（经典 alt-screen 全屏 TUI，占备用屏 `?1049h`，鼠标捕获开）、`Inline`（停在主屏、`Viewport::Inline(rows)` 的底部内联视口，不占备用屏，鼠标开）、`Minimal`（实验性 scrollback-native（`--minimal`），终局块用 `insert_before` 打进终端原生回滚，鼠标**关**）。

三者各有稳定 telemetry 标签 `fullscreen` / `inline` / `minimal`（注释警告改名会让外部分析流塌成 `"other"`）；另存全局原子量 `CURRENT_SCREEN_MODE`，因为 panic hook / 信号处理拿不到参数（`app/mod.rs:156-166, 376-382`）。解析链从高到低：`GROK_SCREEN_MODE` 环境变量（一次性、**读完即删**，否则每个被 spawn 的子进程都会继承一个用户从未要求的强制模式）> `--minimal` / `--fullscreen` > `[ui] screen_mode` > legacy `pager.toml [terminal] minimal` > alt-screen 策略（`[terminal] alt_screen` / `--no-alt-screen` / Zellij·tmux control 探测 / 鼠标泄漏终端）（`app/screen_mode_relaunch.rs:17-20, 268-278, 280-319`）。

`/minimal` 与 `/fullscreen`（别名 `/full`）发 `Action::RelaunchInScreenMode`，事件循环里 `run_pending_mode_switch` 的顺序是：park input reader（≤500 ms，失败则 Aborted 并留在原模式）→ writer `wait_drained`（≤750 ms，超时 Aborted）→ `perform_screen_transition()`，结果三选：`Switched`（reseed + 全量重绘 + 一条 system 块 / toast）、`Aborted`（回滚到原屏与原鼠标状态）、`NeedsExecFallback`（→ exec）（`app/event_loop.rs:909-1020`、`app/mode_switch.rs:108-207`）。细节：

- 切到 Minimal 前**先拿掉鼠标捕获**，且 `Clear(All)` 而**不是** `Clear(Purge)`——注释：purge 会毁掉用户真实的回滚缓冲（`app/mode_switch.rs:121-140`）。
- Minimal 分支在设置视口前先 `set_current_screen_mode(Minimal)`，这样「切换中途失败」的 teardown 也知道终端实际在哪块屏（`:141-143`）。
- 成功后给一条可逆确认：minimal 方向打 system 块 `Switched to minimal mode · /fullscreen to go back`，fullscreen 方向打反向 toast（`app/event_loop.rs:947-972`）。
- 切到 minimal 时会**关掉 fullscreen 专属浮层**（block viewer、图片 / 视频查看器、/gboom 游戏等），否则它们会「看不见却仍然吃键」（`app/mode_switch.rs:224-240`）。
- 重绘前要 `reseed_screen_mode`：重绑快捷键注册表、重置 hover、重算 thinking 折叠策略——**启动时按模式决定的东西，切换时都得重播一遍**（`:29-53`）。

exec 热重启只适用三种场景：启动期的 env 覆盖、`GROK_SCREEN_MODE_SWITCH=exec` 逃逸开关、以及不可恢复的切换（`app/screen_mode_relaunch.rs:1-10`）。`build_screen_mode_relaunch_args`（`:59-160`）剥掉一次性与已消费的 flag（`--minimal/--fullscreen`、`--continue/-c`、`--fork-session`、`--restore-code`、`--resume/--load/--session-id/-s/--worktree/--worktree-ref/--ref` 及其 `=` 形式与后续值、裸 positional prompt、`--` 之后的一切），最后追加 `--resume <id>` 与目标模式 flag；**哪些 flag 吃值是从 clap 定义本身推导**的（`clap::CommandFactory`，`:25-54`），防止分类与 CLI 漂移。Unix 用 `cmd.exec()` 保住 PTY/fd 身份（PTY 测试因此能观察切换而不必重启进程）；Windows 没有 `exec`，改为 spawn 子进程 + 父进程 `wait` 并以其退出码退出，同时 `SetConsoleCtrlHandler(None, 1)` 忽略父进程的 Ctrl+C、sleep 150 ms 让父进程 reader 退出，避免和子进程抢控制台输入记录（`:6-8, 211-241`）；失败时打印可粘贴的命令，带 `GROK_SCREEN_MODE` 与显式 flag 双保险（`screen_mode_relaunch_resume_hint`，`:179-187`）。

**rewind 与导出（压缩记录）。** `/rewind`（别名 `/undo`）只截断**对话历史**，**不回滚磁盘上的文件**——手册用加粗的 "Important" 强调这一点；数据来源是每个会话目录里的 `rewind_points.jsonl`。触发有两条：命令本身，或**空闲 + 空输入框 + 有消息时 800 ms 内连按两次 Esc**；切换回合结束后约 1 秒内 rewind 分支被抑制，防止「乱按 Esc 恰好回合结束」意外打开选择器（`docs/user-guide/03-keyboard-shortcuts.md:229-233`、`docs/user-guide/17-sessions.md:35, 143, 158`）。界面状态机有 5 个阶段 `Loading → Picker → (CancelOffer) → Confirm → Executing → Error`（`views/rewind.rs:56-78`），选择器复用 `/jump` 的列表浮层几何；后端契约 `RewindPointInfo` / `RewindResponse` 已为「有文件快照」预留字段（`num_file_snapshots` / `has_file_changes` / `reverted_files` / `conflicts`），但当前 UX 文案仍明确不动文件（`views/rewind.rs:10-54, 359-389`）。`/export` 与 `grok export` 共用同一个纯函数 `render_blocks_to_markdown`（`scrollback/export.rs:13-62`），产物只有 `## User` / `## Assistant` / `## Tools` 三种小节，thinking / 系统 / 子代理 / 额度等「界面外壳」块全部跳过；写失败时**只用通用措辞** `Failed to write file: <e>`，注释明说「不要回显用户给的路径，可能含密钥或 PII」（`app/dispatch/transcript.rs:152-197`）。**share 在当前快照里两条路径都硬编码关闭**（`slash/commands/share.rs:14-17`、`share_cmd.rs:11-13`），因此没有任何「分享链接 / 公开 URL」的产物形态。

### 5.7 宿主集成：通知、标题、OSC 与终端复用器

#### 本节要点

- 出站分两条链：**通知走独立逃逸队列**（帧可能被 ack 无限期挂住），**标题与进度随帧**；两者共用同一 WriterThread 队列以保证字节顺序，否则 idle 标题会被还在排队的 busy 标题盖回去（`notifications/mod.rs:96-116, 118-135`、`crates/codegen/xai-grok-pager-render/src/render/draw.rs:241-276`）。
- 桌面通知只有 5 种字节协议（OSC 9 / 99 / 777 / BEL / None），**没有任何系统级通知 API 调用**（无 `libnotify`/D-Bus）；Zellij 被硬编码为 BEL，tmux / screen 不参与协议选择（`notifications/protocol.rs:6-47, 22-24, 129`）。
- 标题是「每 tick 重新合成 + 去重」的部件列表，默认 `[action-required, spinner, activity, session-name, grok]`，` - ` 连接；**tmux 下不做 DCS 包装**，这点与通知 / 进度条 / OSC 11 都不同（`notifications/title.rs:8-20, 59-97, 259-266`）。
- 三类能力探测三种读法：OSC 11 裸查优先、失败再 DCS 包装（500 ms / 80 ms）；DA2 同步读 fd（仅 Alacritty，500 ms）；XTVERSION fire-and-forget，回复由事件循环过滤器识别（`pager-render/src/theme/osc11.rs:25-53`、`terminal/da2.rs:29-70`、`terminal/xtversion.rs:1-89`）。
- OSC 12 **只写不查**，复位必须由「本 session 画过」门控；**XTGETTCAP 不存在**（全仓库 grep `gettcap` 无命中），OSC 12 查询同样不存在（grep `12;?` 无命中）。
- tmux 包装条件逐项不同：通知不看版本、进度条与 OSC 11 要 ≥3.3、OSC 52 另有判定，而 DEC 2026 在 tmux 直接终端时**反而不发**——一刀切必然错一半（`terminal/tmux.rs:38-57`、`notifications/progress.rs:34-38`、`clipboard/mod.rs:136-147`）。

**这一层的定位与两条出站链。** 宿主集成层负责「TUI 之外的一切终端交互」：把 Agent 状态推给窗口标题、标签页、通知中心、进度指示，同时把终端的回答拉回来（能力探测、焦点、尺寸）。通知若内联写 stderr 会死锁——它从事件循环线程发出，而终端停止读取时取 stderr 锁永不返回——所以走非阻塞的 `EscapeWriter` 队列；发送失败记录在共享的 `WriterSync` 上而非返回给调用方（`pager-render/src/render/draw.rs:241-247, 266-276`、`notifications/protocol.rs:97-107`）。标题与 OSC 9;4 则走帧管线，理由写在注释里：idle 标题的转义必须排在「仍带 busy 标题的旧帧」之后（`notifications/mod.rs:118-124`）。

**桌面通知：事件、协议、配置。** 事件枚举有 5 个变体，但**只有 4 个发射点**：

| 事件 | 发射点 | 说明 |
| --- | --- | --- |
| `ApprovalRequired` | `app/acp_handler/permissions.rs:47` | 一批排队的权限请求只发一次（`permission_notified` 抑制），队列清空后复位（`app/app_view.rs:5813-5816`） |
| `TurnComplete` | `app/dispatch/prompt.rs:1675-1684` | 正文 `Turn complete in {duration}.` / `Turn complete.`；`bash_turn` 不发 |
| `AgentError` | 同处 `:1685-1687` | 正文 `Error: {err}`；已展示专用 UX 时不发 |
| `SessionReady` | `app/dispatch/status.rs:517-529` | 标题固定 `"Grok"`，正文为事件名 |
| `TaskComplete` | **未找到发射点** | 存在于配置枚举与文档中，全仓库 grep 无命中 |

事件先过两道门：`config.events` 白名单与 `config.condition`（`notifications/mod.rs:81-91`）。协议是 5 种字节形式，按终端品牌自动选择：`Osc9`（`\x1b]9;{body} · {title}\x07`，iTerm2/WezTerm/Warp）、`Osc99`（`\x1b]99;i=grok;…\x1b\\`，Kitty）、`Osc777`（`\x1b]777;notify;Grok;{body}\x1b\\`，Ghostty/VTE/Terminator/foot）、`Bel`（`\x07`，Apple Terminal/Alacritty/VS Code/Windows Terminal/Unknown）、`None`（Grok Desktop）（`protocol.rs:6-19, 21-47, 58-74`）。两个细节：OSC 9 / 99 是**纯正文协议**，所以把会话名折进正文（` · ` 分隔），OSC 777 的 title 参数固定成 `Grok`，因为标签页标题已承担会话名；**Zellij 被硬编码成 `Bel`**（`select_protocol` 第一行就判），tmux / screen 不参与协议选择——包装发生在发送时刻而非选择时刻（`protocol.rs:22-24, 66-70, 129`）。配置节 `[ui.notifications]` 默认 `method = "auto"`、`condition = "unfocused"`、`idle_threshold_secs = 3`、`events = ["turn_complete", "approval_required"]`、`sleep_prevention = true`、`progress_bar = true`；缺失或畸形 TOML 一律回落默认值，带注释模板由 `to_toml_with_comments()` 生成（`notifications/config.rs:24-42, 139-180`、`notifications/mod.rs:262-269`、`docs/user-guide/05-configuration.md:459-490`）。`condition` 语义：`Always` 恒发、`Never` 永不发、`Unfocused` 只在丢失焦点且超过 `idle_threshold_secs` 后发。另有三个「人不在场」开关：`sleep_prevention`（macOS `IOPMAssertionCreateWithName` + `NoIdleSleepAssertion`，Linux `systemd-inhibit --what=idle … sleep infinity`；平台失败置 `platform_unavailable` 且只试一次）、`session_recap` 与 `session_recap_threshold_secs`（回到终端时的回顾，重试退避 90 s）（`notifications/sleep.rs:37-46, 57-125`、`notifications/focus.rs:5-6, 48-56, 65-85`）。**内容净化是硬要求**：标题与正文都过 `sanitize_osc_text`，剥掉所有控制字符（含 BEL 与 C1 ST），避免模型产出的文本提前终止 OSC/DCS（`protocol.rs:49-55`，测试 `:229-248`）。

**通知 hooks 是待复核项（不要写成已生效）。** `[ui.notifications.hooks]` 可配置任意 shell 命令，通过 `GROK_EVENT` / `GROK_MESSAGE` / `GROK_SESSION_ID` 传参，默认 `only_unfocused = true`、`timeout_secs = 10`；实现相当完整（`sh -c` 启动、`detach_std_command` 脱离控制终端、限制子进程网络、进程组登记、超时后杀进程树、ECHLD 后交给 reaper 线程）。但**未找到生产调用点**：全仓库 grep `run_hook` 除定义外全是该文件的单元测试，`NotificationHook` 除重导出外无消费者，`NotificationService::notify` 也不遍历 `config.hooks`。shell 侧另有一套同名概念 `SessionCommand::DispatchNotificationHook`，由 `xai-grok-shell` 的对话流程派发，是**不同机制**，不能拿来当作 pager 配置项已生效的证据（`notifications/config.rs:116-133`、`notifications/hooks.rs:8-115, 99`、`notifications/mod.rs:19`、`crates/codegen/xai-grok-shell/src/session/commands.rs:748-754`）。

**终端标题。** 标题是「按顺序拼接的部件列表」，默认 `[action-required, spinner, activity, session-name, grok]`，部件间 `" - "` 连接，所有部件都没产出内容时回落成字面量 `grok`（`notifications/config.rs:72-85`、`notifications/title.rs:76-79, 199-203`）。部件行为（`title.rs:110-197`）：`Spinner` 是 8 帧盲文点阵 `⠋⠙⠹⠸⠼⠴⠦⠧`，但**每 8 个 tick 才换一帧**（30 fps 下 ≈264 ms，因为终端尤其 Ghostty 会防抖标签标题）；`ActionRequired` 只在 `has_pending_permissions` 时出现、**未聚焦才闪烁**（15 tick 半周期 ≈500 ms），聚焦时静态显示以消除输入时的抖动；`SessionName` 截断到 40 字符、`Model` / `Cwd` 到 30 字符、`Cwd` 只取路径最后一段、`TurnTimer` 少于 1 秒不显示；超长用 `…` 收尾。更新方式是**轮询式每 tick 比较**，不是按状态变化主动推送：`AppView::update_notifications()` 每 tick 组装一次 `TitleState`（字段就是标题的全部输入：`session_name` / `model` / `activity` / `has_pending_permissions` / `cwd` / `turn_elapsed` / `is_busy` / `focused`）并调 `on_tick()`，只有合成结果与上次不同才返回逃逸序列（去重），结果追加到 `pending_notification_escapes` 随后续帧发出；两个特殊点：`is_busy` 且 `activity` 为空时显示 `Waiting`（首个 chunk 之前的空档），`renders_parked()` 时把 activity / elapsed 视为空（`title.rs:23-35, 81-89`、`app/app_view.rs:5790-5833, 5800-5808`）。转义序列本身由 crossterm `SetTitle` 生成，即 OSC 0；退出时 `shutdown()` 把标题复位成 `grok`（`title.rs:99-106, 259-266`、`notifications/mod.rs:179-183`）。**tmux 下标题不做 DCS 包装**——`build_title_escape` 直接产出 OSC 0；tmux 的差异只体现在部件内容（`Cwd` 取 basename、`SessionName` 截断，都是给窄标签栏留余量）。

**进度条与光标色。** OSC 9;4 在 busy 显示不确定态、idle 清除（`notifications/progress.rs:20-21`）：

```rust
const OSC_INDETERMINATE: &str = "\x1b]9;4;1;-1\x07";
pub(crate) const OSC_CLEAR: &str = "\x1b]9;4;0;0\x07";
```

支持面很窄：仅 Ghostty、WezTerm 与 iTerm2 ≥ 3.6（早期 iTerm2 会把 `9;4` 当成 OSC 9 通知，把原始参数 `4;1;-1` 当正文弹出来）；**保活周期 5 秒**，因为 Ghostty 约 15 秒无输出就会自行复位指示器；tmux 下包装成 DCS passthrough（`progress.rs:10-18, 34-38`、`notifications/mod.rs:14-16, 149-171`）。清退路径**不依赖 `NotificationService`**——信号 / panic 处理器无法访问它，因此 `emit_terminal_teardown_sequences` 无条件直写一次 `OSC_CLEAR`（`app/terminal_restore.rs:60-70`）。光标色 OSC 12 **只写不查**：`cursor_color_escape()` 用主题的 `accent_user` 生成 `\x1b]12;rgb:RR/GG/BB\x07`，无 RGB 强调色（终端原生调色板 / `NO_COLOR`）时返回 `None`；复位用 OSC 112，且**必须**由 `CURSOR_COLOR_APPLIED` 门控——无端复位会让 Ghostty 锁死光标色并停止跟随主题切换，所以只撤销自己画过的东西。单一发射者是事件循环里的跟踪器，只在目标值变化时发 OSC 12 或 OSC 112；启动阶段在 writer 线程存在之前由 `apply_cursor_color()` 内联写一次（`pager-render/src/theme/mod.rs:559-568, 587-622`、`app/event_loop.rs:1971-1986`、`app/mod.rs:1538`）。

**终端能力探测总表。**

| 探测 | 查询 | 读法 | 门控 | 超时 |
| --- | --- | --- | --- | --- |
| OSC 11 背景色 | `\x1b]11;?\x07` | 同步读 fd，裸查询优先，失败再试 DCS 包装 | stdin 是 TTY 即试 | 500 ms（包装版 80 ms） |
| DA2 | `\x1b[>0c` | 同步读 fd | 仅 Alacritty 且非 CSI 拦截型复用器 | 500 ms |
| XTVERSION | `\x1b[>0q` | fire-and-forget；事件循环过滤器识别 DCS 回复 | Unknown/Kitty/WezTerm/Ghostty/iTerm2/Rio 且非 CSI 拦截型复用器 | 过滤器 5 s 武装窗口 |
| 终端版本 | 环境变量 | 纯字符串解析 | 必须由同品牌自证 | — |

`xtversion::gate_allows_probe` 与 `da2::gate_allows_probe` 的名单**必须互斥**，否则两种读法会互吃对方的回车（`terminal/xtversion.rs:81-89`、`terminal/da2.rs:65-70`）。OSC 11 的用途是把「终端画布是亮还是暗」变成一个可用输入，供主题 / 配色判断（`classify_luminance` 用 BT.709 + sRGB 逆传输函数，阈值 0.5）；流程是先发**裸**查询（tmux ≥3.2 会替 pane 作答，3.4 还会在 attach 时快照外层终端颜色），裸查询失败且 `should_wrap_osc11` 成立时才用 DCS 包一层并把超时砍到 80 ms——该包装通常被吞掉，不能让它再吃掉一个完整 500 ms 的启动时间。模块文档明确两条约束：**只在启动阶段调用**（crossterm 的 `EventStream` 一旦活跃就会争抢 stdin）；**编辑器 `:terminal` 内不包装**（libvterm 会把信封当乱码画出来）。回复格式同时支持 `rgb:RRRR/GGGG/BBBB` 与两位缩写（`pager-render/src/theme/osc11.rs:11-14, 22-23, 34-53, 64-74, 76-100`）。DA2 只为 Alacritty 存在（它不导出任何版本环境变量，且原则上拒绝 XTVERSION），回复 `CSI > Pp ; Pv ; Pc c` 的 `Pv` 打包成 `major*10000 + minor*100 + patch`，解析**只接受 Alacritty 的精确形状**（`Pp == 0`、`Pc == 1`、`packed ≤ 999999`），否则宁可不认也不凭空造一个版本号；同步读的代价必须记住——迟到但已开头的回复会被 `probe::finish_after_deadline` 排到安静为止，**一整段都没到时则会留给 crossterm，被当作用户输入打进输入框**，所以 500 ms 是「为慢链路而非静默终端」设的；结果存 `OnceLock`，`TerminalContext::term_version()` 中**运行时探测优先于环境变量**（`terminal/da2.rs:1-13, 10-13, 29-34, 103-133`、`terminal/term_version.rs:61-71`）。它的消费者之一是 kitty 键盘 flags：只在拿到「确定损坏」的库里版本（≤2401，即 0.14.0）时才降级不报 release 事件（`terminal/kitty_keyboard.rs:30-45`）。XTVERSION 承担两项用户可见职责——telemetry 的 `xtversion` 字段，以及**反馈信息里的品牌补全**（品牌为 Unknown 时显示 `Unknown (XTVERSION: kitty 0.35.2)`）；已接受的残留风险写在模块文档里：SSH 来自 JediTerm 时仍会发查询并泄漏，回复若在过滤器 5 s 武装窗口之后才到，会被解析成 `Alt+Shift+P` 加一串字面文本（`terminal/xtversion.rs:1-12, 8-12, 109-117`、`terminal/mod.rs:522, 536-541`）。环境变量侧的公理是**必须自证身份**——「一个版本变量只有在环境同时为它所属品牌作证时才可信」，因为变量会穿过进程、SSH 与复用器边界：优先级 `TERM_PROGRAM_VERSION` → `LC_TERMINAL_VERSION`（仅 `LC_TERMINAL=iTerm2` 时）→ `WEZTERM_VERSION` → `VTE_VERSION`；`ZELLIJ_VERSION` 与 `KONSOLE_VERSION` 一律不读（前者会让 Zellij 里的 Alacritty pane 报 Zellij 的版本号）；VS Code 家族的放宽是单向的（`vscode` 可让 Cursor/Windsurf 借用，反之不行）；tmux ≥3.2 会导出自己的 `TERM_PROGRAM_VERSION`，因此必须靠品牌自证挡掉（`terminal/term_version.rs:1-10, 9-10, 50-59, 80-116`）。

**探测原语的两条安全不变量。** `pager-render/src/terminal/probe.rs` 声明：**必须是 stdin 的唯一读者**（在 crossterm 读线程存在之前，或它 join 之后）；**读窗口里敲的键会被消费并丢弃**，没有可移植的回注手段。写侧 `write_query` 在共享 stderr 锁内写，TTY 检查跑在 dup 出来的 render fd 上而非 `std::io::stderr()`；`write_query_until` 用同一个 deadline 约束**取锁等待 + 写入**，避免卡死的 writer 线程拖住清退。读侧 `read_tty_reply` 逐字节 poll、有 256 字节上限以对付「不发给终止符的终端」，deadline 到期后有 100 ms 宽限 + 25 ms 静默窗口把在途回复读完；`drain_tty_until` 只保留 64 字节尾窗（够装最长的 DA1 回复）、不限制总字节数，因此任意多的按键上报 / 预输入都可以出现在终止符之前。输入源 `tty_input_fd()` 先用 stdin，不是终端就开 `/dev/tty`（读写模式，与 crossterm 的 `tty_fd` 一致），这样 `grok </dev/null` 也能读到回复（`probe.rs:1-7, 12-21, 24-36, 38-50, 160-202, 207-267, 288-335`）。

**`pop_fence` 与 `overlay` 都不是探测机制。** `pop_fence` 是 5.6 讲的 kitty pop 之后的 DA1 清退栅栏；`overlay` 是 kitty / iTerm2 图形协议的**图片覆盖层生命周期管理**：`static`（跨帧保持，按 `owner_id` + 几何位置判定是否重画）与 `volatile`（每帧重画）、`clear`（kitty 按 id 删除；iTerm2 无删除能力但必须复位所有权），以及 `PostFlush`——图片转义在 cell diff 之后写入，并且**只有写入成功才提交所有权**。它服务提示里的图片 / 预览，与能力探测唯一的交集是「这些转义同样在 tmux 下不可用」（`TerminalContext::graphics_protocol_skip_reason` 直接返回 `"tmux"`）（`pager-render/src/terminal/overlay.rs:1-11, 104-110, 160-269`、`terminal/mod.rs:373-379`）。

**tmux / screen 兼容清单。** `MultiplexerKind` 有 6 个取值，识别顺序为 Byobu 显式后端 → `TMUX` → `ZELLIJ`/`ZELLIJ_SESSION_NAME` → `STY`（screen）→ `HERDR_ENV` → cmux 三兄弟 → Undetected；注释解释「嵌套时真实复用器优先」的动因：herdr daemon 从 tmux 里起会把 `TMUX` 冻结进每个 pane，若误判为 herdr 就会把 OSC 52 包进 DCS，而 herdr 会把这段信封当文本打印出来。`intercepts_csi_queries()` 判定 `Tmux | Screen | Zellij | Herdr` 为真——**cmux 不拦 CSI**，所以它的品牌探测照常进行（`terminal/mod.rs:169-197, 777-814`、`terminal/xtversion.rs:169-173`）。DCS passthrough 形如 `\x1bPtmux; … \x1b\\`，其中每个 `0x1b` 写两遍，可用条件是 **tmux ≥3.3**（版本未知按旧版处理）；模块文档把责任边界写清楚：包装前调用方必须已经净化过载荷，helper **只做 ESC 双写**，不剥离 CAN(`0x18`)/SUB(`0x1a`)/C1 ST(`0x9c`)/BEL(`0x07`)——这些字符在 tmux/xterm 下会中止或终止 DCS/OSC（`terminal/tmux.rs:1-4, 9-20, 38-42`）。被包装与被跳过的东西：

| 载荷 | 判定 | 备注 |
| --- | --- | --- |
| 桌面通知（OSC 9/99/777） | `ctx.is_tmux_backed()`，**不看版本** | 与进度条对 tmux <3.3 的行为不同，这是一处不对称 |
| OSC 9;4 进度条 / OSC 11 查询 / OSC 52 剪贴板 | 分别要 `passthrough_available`（≥3.3）、回退路径 `should_wrap_osc11`（可用且非编辑器 `:terminal`）、`osc52_tmux_passthrough`（开了 OSC 52 且 tmux 是直接终端） | `notifications/progress.rs:34-38`、`terminal/tmux.rs:44-48`、`theme/osc11.rs:47-50`、`clipboard/mod.rs:136-147` |
| DEC 2026 同步输出 | tmux **直接终端时整个不发**（tmux 自己整屏重绘并已对外同步）；编辑器 `:terminal` 内仍发 | `terminal/tmux.rs:50-57`、`render/draw.rs:505-529` |
| kitty 键盘协议 / 超链接 OSC 8 | tmux <3.3 或 `extended-keys off` → 跳过 KKP；tmux <3.4 → 跳过 OSC 8。screen 直接判 `kitty_skip_reason = "screen"`；Apple Terminal 视为敌意解析器；VTE <0.50.4 跳过 | `terminal/mod.rs:356-371, 436-468` |
| 行内图形图片 / 备用屏 | tmux 下跳过图片；控制模式或 Zellij 下备用屏自动走 inline | `terminal/mod.rs:373-379, 962-978` |

tmux 的版本、选项与客户端能力走**带外子进程查询**而非转义序列：`tmux -V`、`show-option -gqv <opt>`、`show-option -gv <opt>`（靠 stderr 精确匹配 `invalid option:` / `unknown option:` 区分「不支持该选项」与「tmux 不可用」）、`display-message -p #{client_flags}`（控制模式判定）、`display-message -p #{client_termfeatures}`（24 位色能否在 tmux 中幸存，注释强调**这个值而非 pane 的 `COLORTERM` 才是判据**）；所有查询 2 秒超时、进程组托管、SIGTERM 后 100 ms 宽限升级 SIGKILL，读取失败统一归一到 `None`（**fail-open 为「未知」而不是「否定」**）（`terminal/tmux_probe.rs:4-9, 134-165, 198-200`）。用户可见的修复入口是 `/doctor` 给出的四个可持久 tmux 修复：`set -g set-clipboard on`、`set -wg allow-passthrough on`、`set -g extended-keys on`、`set -as terminal-features ",*:RGB"`；实现上刻意**不**执行 `tmux source-file`、不热改服务器，只改宿主配置文件并保留行尾与权限（`docs/user-guide/21-terminal-support.md:29-47`）。

**焦点与尺寸事件。** 上报由 crossterm `EnableFocusChange` 在启动时一次性打开；到达侧还有一层修复：被拆散的 `\e[I` / `\e[O`（普遍发生在 Shift 处于按下状态时，大写字母被单独解码）会由 CSI 过滤器重组成 `FocusGained`/`FocusLost`，而不是被丢弃（`app/mod.rs:1505`、`app/csi_filter.rs:10, 60-70`）。`Event::FocusGained` 的副作用不止通知：重新发一次鼠标捕获（焦点切换后有的终端会丢掉捕获）、若「外层可能越权重绘 pane」（有编辑器 `:terminal` 或检测到任意复用器）则**强制整屏重绘**、检查会话回顾是否到期并派发 `SendRecap { auto: true }`，随后 `on_focus_gained()` 复位空闲计时与回顾状态；`Event::FocusLost` 只做两件事：`on_focus_lost()`（开始计时、允许新的自动回顾）与释放 gboom 游戏（`app/event_loop.rs:656-660, 3234-3262, 3288-3294`、`terminal/mod.rs:288-293`）。一个容易被忽略的前提：越权重绘修复只在焦点事件真的到达时才有用，而 **tmux 的 `focus-events` 默认关闭**。resize 走普通输入路径（`is_resize` 标记）但有 16 ms 去抖，tick 间隔随需求分三档：动画密集时 16 ms、一般 33 ms、只有低频需求时 83 ms；resize 事件在启动预输入中会被识别并重放，`draw_frame` 每帧先 `terminal.autoresize()`，因此布局跟上尺寸变化不依赖额外的 resize 分支（`app/event_loop.rs:34, 3337, 4128, 4157`、`app/app_view.rs:250-257, 5563-5568`、`pager-render/src/render/draw.rs:510-511`）。

**SSH 与慢链路下的降级。** `is_ssh` 由 `SSH_CONNECTION` / `SSH_TTY` / `SSH_CLIENT` 任一存在判定；降级不是一个集中开关，而是散落各处的一致性偏好：换行键优先宣传 `Alt+Enter`（SSH 或复用器会丢扩展 Enter）；剪贴板 Linux 恒发 OSC 52，macOS/Windows 仅在 tmux/SSH/容器/有 wrap sink 时发；Apple Terminal over SSH 无法到达本地剪贴板，每次复制仍落盘 `~/.grok/last-copy.txt` 并 toast 提示路径；`grok wrap ssh user@host` 在本地 PTY 里转发 OSC 52 并盖上 `LC_GROK_APPEARANCE`，连接断开后恢复终端模式；doctor 对 SSH（及官方 VS Code remote）单独出建议，含 `terminal.ssh-wrap`；启动预输入在 VTE 环境给 20 ms 排空窗口、其余为 0（`terminal/mod.rs:423-430, 837-839`、`clipboard/mod.rs:132-147`、`docs/user-guide/21-terminal-support.md:141-159`、`app/diagnostics/mod.rs:411-415`、`app/mod.rs:1533-1537`）。需要明确的是：**本快照里没有按网络延迟自适应帧率或降低绘制频率的逻辑**——帧率分档只依据「有什么动画在跑」，全仓库 grep `slow_link` 只命中 DA2 的那句注释；慢链路唯一专门的设计是 DA2 的 500 ms 超时取值。若 jev-harness 需要真正的慢链路降级，这一层需要自己设计。

### 5.8 无障碍、自适应与终端能力协商

#### 本节要点

- 这一层是**探测 → 分类 → 降级**两段式：启动时把环境变量与探测回复压成 `TerminalContext` 上的能力位，各功能只读枚举，**业务代码从不 `match` 环境变量**（`pager-render/src/terminal/mod.rs:225-265`）。
- `TerminalContext` 同时保留乐观的 `brand` 与原始的 `env_brand`，所有「必须默认拒绝」的判断都回查后者（原生 Windows 会把 `Unknown` 乐观当成 `WindowsTerminal`）（`terminal/mod.rs:228-232, 725-730`）。
- 高度自适应有三级互相咬合的阈值，而且**自动 compact 从不回写配置或磁盘**，窗口拉高后用户选择立刻恢复；整屏布局只有一个 `Min`，就是 `SCROLLBACK_MIN_ROWS = 5`，所以行数不够时被压的是别的行而不是 scrollback（`views/agent.rs:89-104, 123`）。
- 低色彩能力下**不是简单降级而是重做配色**：ANSI 16 专用替代配色按极性选语义色、以 `high_contrast_fg` 承载焦点与正文；Windows 另有量化前的对比度提升。但**不存在色盲（CVD）专用方案**，`color_support.rs` 的 16 色映射是能力降级而非无障碍配色（`pager-render/src/theme/mod.rs:325-334, 367-404, 429-560`）。
- 无障碍开关的真实清单全是配置项与一个环境变量；**没有全局「减少动效」开关**，动画节奏是硬编码常量（`views/turn_status.rs:31/36/40`、`tips/ephemeral.rs:11`）。
- 四个输入过滤器解决「终端应答污染输入流」的不同侧面，都是**兄弟模块**且互相交叉引用；其中 X10 重组的判据是**魔数形状而非坐标越界**，这是大终端上踩过坑后的修正。

**能力识别：探测优先级本身就是可靠性排序。** `detect_terminal_brand_from_env` 的顺序（`terminal/mod.rs:621-723`）：fork 专属标记（`CURSOR_TRACE_ID`、`VSCODE_GIT_ASKPASS_MAIN`，它们即使在 SSH / tmux 下也存活，而 `TERM_PROGRAM` 不一定）→ `TERM_PROGRAM`（最可靠的单信号）→ `TERMINAL_EMULATOR`（JetBrains / JediTerm，必须排在 `TERM_SESSION_ID` 之前，否则会误判成 Apple Terminal）→ 品牌专属变量（`WEZTERM_VERSION` → `ITERM_SESSION_ID` / `LC_TERMINAL`（官方注释指出 `LC_TERMINAL` 能穿过 SSH 的 `SendEnv/AcceptEnv LC_*`）→ `TERM_SESSION_ID` → `KITTY_WINDOW_ID` → `ALACRITTY_SOCKET` → `TERM` 精确匹配 `rio` / `foot`）→ `TERMINATOR_UUID`（必须先于 VTE 兜底）→ `VTE_VERSION` 兜底为 `Vte` → `WT_SESSION` → 否则 `Unknown`。文档明示 15 类**被识别**的终端，代码里的 `TerminalName` 却是 20 个变体（多出 `Terminator`、`Otty` 与 `Unknown`），并配一组分类谓词：`is_vte_based()` / `is_vscode_family()` / `is_xtermjs_embed()` 把「同一类怪癖」归组，`is_capability_unclassified()`（`Unknown | Otty`）共享 fail-closed 姿态，`supports_osc52_clipboard()` **默认 false**、只有显式列出的品牌才是 true，`delivers_ime_as_bracketed_paste()` 只有 Otty 为 true（`terminal/mod.rs:58-160`、`docs/user-guide/21-terminal-support.md:51-76`）。官方同时列出三条识别限制：tmux 内可能拿不到外层终端变量；SSH 不转发多数终端变量；tmux 的 global environment 反映**第一个**接入的客户端，未必是当前客户端。

**行为差异矩阵（按「代码依据的怪癖」列，而不是按品牌罗列）。**

| 终端 / 环境 | 依据 | 行为差异 |
| --- | --- | --- |
| Apple Terminal | `keyboard.rs:102-104`、`terminal/mod.rs:138-154` | `Cmd+Bsp` 被窗口管理器截获（`Unrecoverable`），`Opt` 与修饰 Enter 为 `Dropped` 但可被 CoreGraphics 抢救；不支持 OSC 52，远端复制退回 `~/.grok/last-copy.txt`；用 `Ctrl+O` 代替 `Ctrl+Enter` 插话（doc 21:142-147, 202-203） |
| VS Code / Cursor / Windsurf / Zed | `terminal/mod.rs:318-324, 402-410` | **从不协商** Kitty 键盘协议（xterm.js 对 shifted 可打印键编码有误，issue 5823），`Shift+Enter` 与 `Enter` 同码，UI 改推 `Alt+Enter` |
| Kitty / Ghostty / foot / WezTerm / Alacritty / Rio | `keyboard.rs:88-100`、`terminal/mod.rs:81-84`、doc 21:197-203 | Kitty 系修饰键 `Native` 无需抢救并纳入 truecolor 品牌白名单；Alacritty / Rio 的 `Cmd` 原生（映射到 `^U`）、`Opt` 被丢弃但可抢救；WezTerm 出厂关闭 Kitty 键盘协议，`/doctor` 给 `terminal.wezterm-kitty` 设置项 |
| Warp | `terminal/image.rs:85-95` | 接受部分 Kitty graphics 转义但不支持 placement/scrollback 模型，会留残影，故不启用内联图像 |
| JetBrains（JediTerm） | `terminal/mod.rs:85-89, 736-738`、`keyboard.rs:108-110` | 无 `TERM_FEATURES`、XTVERSION 会泄漏、DA1 只是裸 VT102，**无法运行时探测**，能力全 `Unknown`；Windows 上鼠标上报会漏成原始文本，该组合**默认进 minimal 模式**（`app/mod.rs:939-953`） |
| VTE 系 / Terminator | `terminal/mod.rs:388-400` | `VTE_VERSION < 8200` 或无法解析版本时判定 `Shift+Enter` 不可用 |
| Windows Terminal / 旧版 ConHost | `color_support.rs:266-268`、`glyphs.rs:1-11, 148-159, 377-391` | 原生 Windows 默认假定 ConHost 已开 VT 处理、直接认定 truecolor；旧 ConHost 字形降级为 ASCII / CP437（`❯`→`> `、`✗`→`x`、braille spinner→`|/-\`），有 `GROK_FORCE_LEGACY_CONSOLE` 强制开关 |
| Zellij / tmux / GNU screen / Byobu+screen | `terminal/mod.rs:194-197, 356-372, 374-379, 968-971`、`terminal/tmux.rs:55-57`、doc 21:260-263 | 都拦 CSI 查询、使外围探测失效；Zellij 自动关备用屏退回 inline；tmux 不进同步输出、禁用内联图像、`<3.3` 或 `extended-keys = off` 时 `Shift+Enter` 退化；screen 直接 `kitty_skip_reason = "screen"`；Byobu-screen 支持有限，`/doctor` 报 `terminal.byobu-screen` 并建议切 tmux 后端 |
| Otty / Unknown / 原生 Windows（ConPTY） | `terminal/mod.rs:156-159, 340-348, 414-416`、`terminal/image.rs:141-144` | Otty 是唯一把 IME 提交包成 bracketed paste 的品牌；`Unknown` 无正证据即跳过 KKP，且 `/doctor` 报通知协议回退到响铃；ConPTY 会剥掉 Kitty / iTerm2 的 APC 转义，故内联图像协议返回 `None` |

备用屏是独立的一个轴：`AltScreenMode { Auto, Always, Never }`，`--no-alt-screen` 优先级最高，`Auto` 在 Zellij 与 tmux control mode 下**自动退回 inline**（`terminal/mod.rs:898-908, 955-983`）。

**尺寸自适应：三级阈值 + 一条不回写约束。** 阈值全部集中在 `views/agent.rs:89-98`，并用编译期断言把关系钉死：

```rust
pub const SHORT_TERMINAL_ROWS: u16 = 16;      // ≤16：深降级区，砍掉提示区可选行
pub const SCROLLBACK_MIN_ROWS: u16 = 5;       // scrollback 的地板（整屏布局里唯一的 Min）
pub const AUTO_COMPACT_MAX_ROWS: u16 = 20;    // ≤20：强制 compact 渲染
const _: () = assert!(SHORT_TERMINAL_ROWS < AUTO_COMPACT_MAX_ROWS);
// 行数  0    ──16──     ──20──     ──28──     ∞
//       深降级区 │ 自动 compact │ 小屏提示区 │ 默认布局
```

- **≤16**：底部垂直 padding 归零、插件 CTA 行归零、follow-up 建议行归零（`views/agent.rs:205-217`）；prompt 上方 gap 归零、面板 dock 关闭（`app/agent_view/render.rs:1010-1013, 1076-1084`）；ephemeral tip 行**不再渲染**（`tips/ephemeral.rs:13-16`）——设计意图是保证 prompt 与 scrollback 不被饿死。
- **≤20**：渲染层收到 `compact = true` 走更密的 padding；自动判定是纯函数且**结果从不回写** `current_ui.compact_mode`、渲染缓存或磁盘；`terminal_rows == 0` 表示「还没测量」，永不强制 compact（`views/agent.rs:99-104`）。
- **21–28**：小屏提示带。`SMALL_SCREEN_TIP_MAX_ROWS = 28`，提示文案 `Tight on space? Try /compact-mode`，会话内**最多显示一次**（`SMALL_SCREEN_TIP_SEEN_CAP = 1`）且标记为 `ambient()`——提交 prompt 不会让它消失，被遮挡时 TTL 暂停计时；显示时机是**首次稳定绘制**，不在后续 resize 时重放（`tips/small_screen.rs:1-4, 20-49`）。

**宽度没有视图级阈值**，自适应是逐组件的：模态窗口有 `min_width` 地板，各调用方取值 40 / 44 / 50 / 60，地板值最后会被**夹回缓冲区**并有回归测试；宽度不足以渲染时返回 0 / 跳过绘制并清掉悬停与拖拽状态，防止上一帧的陈旧矩形继续吃点击；仪表板头部在放不下 cwd + 提示时**先丢提示再截断路径**（`views/modal_window.rs:96, 559, 579-582, 1308`、`views/dashboard/chrome.rs:259` 附近测试）。**没有「终端过小就拒绝渲染并报错」的路径**——整屏层面只有 `SCROLLBACK_MIN_ROWS = 5` 这一个地板。

**色彩：四级能力、量化管线、以及两处专门的对比增强。** `ColorLevel` 是有序枚举 `None < Basic(ANSI 16) < Ansi256 < TrueColor`，检测优先级：**`NO_COLOR` 直接判 `None`**（注释写明「显式退出优先」）→ 非 TTY 默认 TrueColor（理由：「TUI 总归跑在终端里」）→ `supports-color` crate 读 stdout 映射四档 → 若 crate 只报 256 色但品牌已知支持 24-bit 则**升级为 TrueColor**（注释解释 tmux/SSH/mosh 常把 `COLORTERM` 剥掉，不补这一步会让主题被量化到 16 色，背景/边框/弱化色的层次全塌在一起）→ 终端原生锁定（minimal 模式或选了 `terminal` 主题）把上限**压到 `Basic`**（`pager-render/src/theme/color_support.rs:1-4, 17-44, 83-127, 243-245`、`theme/cache.rs:105-120`）。另有一条独立路径 `standalone()` 专供 `grok doctor --json`：它**不看 stdout**（doctor 常被管道接走），只看 stderr 是否为终端或 `/dev/tty`（Windows `CONIN$`）能否打开，`TERM` 为空或 `dumb` 时老实回答「无色彩证据」（`:138-203`）。降级管线 `quantize_color`：TrueColor 原样；Ansi256 走 `nearest_indexed`；Basic 把 RGB/Indexed 经 `indexed_to_ansi16`（0–15 一一对应，16–255 先转 RGB 再按**平方欧氏距离**找最近 ANSI 16）；None 全部压成 `Color::Reset`——最后这点在主题层被显式防住：必须用 `has_color()` 判断，否则「named-ANSI 重绘会把 `Reset` 又染回去，使 `NO_COLOR` 失效」（`:220-234, 274-336`、`theme/mod.rs:324`）。主题可用性随档位变化：非 truecolor 时主题选择器**隐藏** TokyoNight / RosePineMoon / OscuraMidnight（染色背景量化后会失去特征），默认主题 GrokNight 则声明「在 256 色与 16 色终端上都能干净地量化」（`docs/user-guide/06-theming.md:13, 131`）。

对比增强是本专题最重要的正面发现——**不是单纯降级，而是针对低色彩能力重做配色**：**(a) Windows 对比度提升**在量化**之前**采样主题明暗极性，再用 `push_away` 把一批弱对比色推离背景（`user_block_push` 暗色 28 / 亮色 8，`gray_dim` 40、`prompt_border` 40、`prompt_border_active` 60，按通道推并夹在 0–255）；**(b) ANSI 16 专用替代配色**在 `Basic` 档或旧 ConHost 未达 truecolor 时启用：以 `Black`/`White` 作画布底、**升一级**用 `DarkGray`/`Gray` 表达「抬升面」（注释承认「在 ANSI 16 里这比 truecolor 设计读起来更响，但在包括博物馆级 `TERM=ansi` 在内的任何 16 色终端上都保证可见」），语义色按极性选（暗底用 `LightRed`/`LightGreen` 等亮变体、亮底用常规变体，理由是常规 ANSI 色 idx 1–7 约 50% 亮度、亮变体 idx 9–15 全饱和），`high_contrast_fg` 取 `White`/`Black` 专门用于 `prompt_border_active`、`text_primary`、`md_text`、`accent_user` 等焦点与正文位，并有测试断言 `scrollbar_thumb_contrasts_with_track_in_all_themes`（`theme/mod.rs:325-334, 367-404, 429-560, 1021`）。

**色盲：零命中。** 在 `crates/` 全树检索 `colorblind` / `color_blind` / `colour_blind` / `deuteran` / `protan` / `cvd` / `daltoni` **零命中**：没有 CVD 专用调色板，没有红绿之外的冗余编码开关。唯一与「不靠颜色分辨」相关的是 diff 渲染的退化路径——当主题的 `diff_*_bg` 为 `Reset` 时变更行改用**整行红/绿前景色**而非色带，这仍依赖色调，不构成 CVD 方案。ANSI 16 替代配色客观上提高了对比度与极性正确性，但它的设计目标写在注释里是「保证可见」，不是色觉障碍适配。**合稿时不要把能力降级写成无障碍配色。**

**无障碍开关全清单（唯一的完整入口，别处不再重复）。**

| 开关 | 取值 | 作用 | 出处 |
| --- | --- | --- | --- |
| `NO_COLOR` | 环境变量，存在即生效 | 强制 `ColorLevel::None`，全部颜色映射为 `Reset` | `color_support.rs:99, 178`；doc 06:119 |
| `ui.cursor_blink` | 三态：未设 / `true` / `false` | 未设 = `Inherit`（不发任何样式转义，保留终端自身设置）；`true` = 闪烁块、`false` = 常亮块。注释说明强制设置曾导致闪烁问题 | `app/mod.rs:1427-1446`；doc 26 `ui` 表 |
| `ui.screen_mode` / `ui.compact_mode` + `/compact-mode` | `fullscreen`/`minimal`；boolean / 命令 | 渲染模式（重启生效，minimal 为键盘优先、终端持有历史）与密度（小屏提示推荐 compact） | doc 26；`app/mod.rs:399-411`、`tips/small_screen.rs:41-47` |
| `ui.mouse_reporting_toggle` + `Ctrl+R` + `/toggle-mouse-reporting` | boolean | 关闭鼠标捕获，解决鼠标上报坏掉 / 干扰。`Ctrl+R` 只在 scrollback 聚焦时可用，prompt 侧走斜杠命令 | `app/mod.rs:182-187, 329`；doc 26 |
| `ui.simple_mode` / `ui.vim_mode` / `ui.show_thinking_blocks` | boolean | 输入编辑与导航范式（readline ↔ vim）、是否显示推理块 | doc 05:71-72、doc 03:9-29、doc 26 |
| `ui.invert_scroll` / `scroll_lines` / `scroll_speed` / `scroll_mode`、`ui.display_refresh.auto_cadence_enabled` | bool / int / enum | 滚动方向、步长、速度、输入分类；刷新节奏匹配显示器（自适应，**不是**减少动效） | doc 26；探测实现 `host/display_refresh.rs` |
| `[scrollback.display] rtl_bidi` | boolean，默认 false | 开启应用侧 RTL 重排 | doc 21:274-277；`render/bidi.rs:19-30` |
| `[ui.notifications] method` / `condition` | `auto`/`osc9`/`osc99`/`osc777`/`bel`/`none`；`unfocused`/`always`/`never` | 通知协议与时机；**`none` 是唯一的「静音」入口** | `notifications/config.rs:52-68` |
| `[ui.contextual_hints].*` | boolean 逐项 | 关闭各类上下文提示（`small_screen`、`ssh_wrap`、`image_input`、`plan_mode`、`send_now`、`undo`、`word_select`） | doc 26 |
| `GROK_CLIPBOARD_NO_OSC52` / `GROK_CLIPBOARD_NO_DATA_CONTROL` / `GROK_FORCE_LEGACY_CONSOLE` | 环境变量 | 关掉 OSC 52 粘贴路由、关掉 Wayland data-control 路由（改用命令行工具）、强制旧控制台字形降级（ASCII / CP437） | doc 21:111-120；`glyphs.rs:389-391` |

**明确未找到证据的三项**：**全局减少动效 / reduce motion 开关**（无：spinner 每 `SPINNER_DIVISOR = 4` 个 tick 换帧、idle 监视脉冲 `MONITOR_PULSE_DIVISOR = 8`、「等待你」菱形脉冲 `USER_WAITING_PULSE_SPEED = 0.08`、tip TTL `DEFAULT_TIP_TICKS = 90`，都是硬编码常量，`ui.cursor_blink` 是本仓库唯一与动效相关的无障碍开关）；**静音 / 响铃开关（通知协议之外）**（未见主动写 BEL 的代码，唯一相关的是**被动回退**：终端未识别时通知协议落到 `NotificationProtocol::Bel`，`/doctor` 报 `WarningCategory::NotificationProtocolFallback`，文案「Grok is using the terminal bell because the terminal was not recognized」，并提示把 `method` 改成终端支持的协议或 `none`）；**高对比模式 / 字体缩放 / 屏幕阅读器支持**（`screen_reader`、`screenreader`、`aria`、`a11y`、`accessib` 在 Rust 源码中零命中）（`views/turn_status.rs:31/36/40`、`tips/ephemeral.rs:11`、`diagnostics/mod.rs:106, 522-537`）。

**输入过滤层：四个过滤器在防什么。** 终端不只是键盘输入源：它会主动往 stdin 塞 (1) 对应用提问的**应答**（DA2、XTVERSION 的 DCS 回复），(2) 应用自己开启的**上报**（SGR 鼠标、焦点、bracketed paste）。crossterm 按自己的语法把这些字节解码成 `Key` 事件，于是「终端应答」会伪装成用户按键混进事件流，而在特定坐标下**鼠标上报本身也会被解码错**。四个过滤器各守一面，且彼此是**兄弟模块**、注释里显式交叉引用：`XtversionFilter`（`app/xt_filter.rs`）吞 XTVERSION 的 DCS 应答、`CsiFragmentFilter`（`app/csi_filter.rs`）重组 SGR 鼠标与焦点片段、`X10ReassemblyFilter`（`app/x10_filter.rs`）重组被错解的 X10 鼠标坐标，三者都在事件循环之前；`Osc52Filter`（`app/wrap_filter.rs`）与 `ModeTracker`（`app/wrap_restore.rs`）则在 `grok wrap` 的**输出侧**（子进程 PTY 输出与 stdout 之间）工作。

- **`X10ReassemblyFilter`**：当坐标字节恰好构成 UTF-8 多字节序列时，解码器会把两个字节拆成「列 = lead 字节 − 33，行 = continuation − 33」，于是 `0xC2`/`0xC3`（161/162）配 `0x80..0xBF`（95..158）成为**魔数形状**：`matches!(m.column, 161 | 162) && (95..=158).contains(&m.row)`。命中后扣住事件，等紧随其后「被挤出的行字节」（以普通 `Char` 或 `Backspace` 到达），两者必须在一个批次内相邻到达，超时上限 `MAX_COMPLETION_GAP = 50 ms`，超时后的真实击键原样放行；`reconstruct` 反解真实列，行号取被挤出字节减 33。重要演进：早期用**坐标是否越界**判断，结果在 180×120 这样的大终端上「错解形状恰好落在边界内」而漏过，现在**形状即判据**、与终端尺寸无关（`app/x10_filter.rs:11-13, 28, 80-82, 87-118, 218-228, 262`）。
- **`CsiFragmentFilter`**：状态机重组被拆成片段的两类 CSI——SGR 鼠标 `[<d;d;d{M,m}` 与焦点上报 `[I`/`[O`。最有价值的是**焦点上报的翻译**：裸 `\e` 与 `[I`/`[O` 落在同一批次时撤回那个裸 Esc，改吐 `Event::FocusGained` / `FocusLost`，这样「焦点驱动的功能（prompt 重新聚焦、recap 离开计时器、`/gboom` 松键）在 SSH 下仍然可用」。代价与边界写得很诚实：单个 `[` 不能跨批次持有（否则用户打的 `[` 会卡住不显示），所以**上一批次孤立了 `\e` 的焦点上报仍会漏过**；类型击键有防护——只有 `\e[` 前缀才进状态机，`arr[I]` 这类文本正常通过（`app/csi_filter.rs:9-11, 57-81, 109-159`）。
- **`XtversionFilter`**：吞掉终端对 XTVERSION 的 DCS 应答，因为 crossterm 会把 `ESC P` 呈现为 `Alt+Shift+P`、payload 呈现为一串 `Char`、ST 呈现为 `Alt+\`、BEL 呈现为 `Ctrl+G`。四个时间常量各有理由：`XT_ARM_WINDOW = 5 s`（对标 opentui 的非阻塞 5 s 窗口，解除后零开销）、`XT_FRAGMENT_TIMEOUT = 150 ms`（其他 TUI 栈用的值）、`XT_MAX_HOLD = 1 s`（总持有上限，防终端「细水长流」地吐合法 payload 字符拖死事件循环）、`XT_MAX_PAYLOAD = 64`（真实应答很短，示例 `kitty 0.35.2`）。两个可借鉴的细节：armed 状态来自 `reply_pending()`，且 `deadline` 是**首次 `filter()` 调用时**才设置而不是构造时——「启动加载可能耗时数秒，那段时间不能烧掉 armed 窗口」；一旦 intro（`ESC P` 之后的部分）被确认就不再中途过期，因为在途应答必须走完，否则尾巴会作为用户输入漏出去（`app/xt_filter.rs:10-28, 55, 124-126`）。
- **`Osc52Filter`**：只服务 `grok wrap`，位置在**子进程 PTY 输出与 stdout 之间**（输出侧，不是输入侧）。职责是拦截 OSC 52 剪贴板序列（含 tmux DCS passthrough 形式）与私有 host-image 请求 OSC，其余全部**逐字节透传**，同时把每个 CSI 报告给 wrap 模式追踪器。三个容量常量都注明推导关系：`MAX_ESC_BUFFER = 1 MiB`（要容纳 768 KiB payload 的 base64（~1.33×）加转义外壳）、`MAX_CSI_BUFFER = 128`（单条 DECSET 列出全部被跟踪模式约 69 字节，有单测钉住关系）、`MAX_CLIPBOARD_PAYLOAD = 768 KiB`（与前者对齐；SSH 场景下不现实有更大的剪贴板）。base64 解码用 `DecodePaddingMode::Indifferent`，理由是「现实中（含部分 Go 实现与终端）会省略 `=` 填充」；另有一条约束：被子进程 EOF 截断的 CSI **故意不冲刷**，因为「发出半开的 CSI 会让真实终端解析器停在序列中间，吃掉退出路径紧接着写的恢复字节」（`app/wrap_filter.rs:1-11, 13-38, 51-55`）。

`grok wrap` 侧还有一个 `ModeTracker`（`app/wrap_restore.rs`）：它区分不了子进程正常退出与连接中断（SSH 传输死亡到达时只是一个普通 PTY EOF 加退出码），能知道的是子进程在本地终端上**开了哪些 DEC 私有模式却没关**。跟踪 14 个模式位，除常规鼠标编码（`?1000/1002/1003/1005/1015/1016`）、`?2004`、`?1004`、`?2026` 外，还包括**老式备用屏** `?47`、`?1047`、`?1049`；`?25` 是**反向**跟踪的（DECTCEM 的 set 侧是「显示光标」，需要修复的态是「见过 `?25l` 而之后没有 `?25h`」）。恢复字节按 `DISABLE_ORDER` 发出，顺序与 `xai_crash_handler::terminal` 的 `RESTORE_SEQ` 用单测钉在一起。已知局限也老实记录：Kitty 协议每个屏幕缓冲区各自维护一层栈，而这里只有一个净深度计数，所以「在一个屏幕 push、在另一个屏幕死掉」的子进程是按计数而非按屏幕恢复的（`:1-8, 19-50`）。

**CJK、RTL 与组合字符。** 宽度计算统一走 `unicode-width` 的显示宽度而不是字符数：截断函数按显示列截断（CJK 计 2）；一个具体的坑是 `format!` 的 `{:<width$}` 按**字符数**补齐，遇到 CJK 这类宽字形后列会错位，所以另写了对齐助手并有测试 `print_table_pads_cjk_labels_by_display_width` 钉住；无关字形（em dash、CJK）在字形替换模块中**原样透传**（`pager-render/src/util.rs:203, 232`、`worktree_cmd/display.rs:233-235`、`glyphs.rs:569`）。组合字符与字素簇当作原子：编辑器侧测试直接以 `["e\u{301}", "👩🏽\u{200d}💻", "🇺🇸", "界"]` 为用例，断言光标移动与删除**从不切开字素簇**，换行按显示宽度而非字节，行高会算上双宽 emoji（`xai-ratatui-textarea/src/editor_tests/editing.rs:35-40, 55-67, 84-104`、`wrapping.rs:512, 588-589`）。**未找到证据**：没有基于 `LANG` / `LC_ALL` / `LC_CTYPE` 的字符集能力探测或 ASCII 降级路径——`host::collect_unicode_env` 名字容易误导，它只是把环境变量收集成 UTF-8 map；字形降级只有一个触发条件即旧版 Windows ConHost。IME 只有一处显式处理（Otty 的 `delivers_ime_as_bracketed_paste()`），未见预编辑渲染、光标定位或组合态 UI（`terminal/mod.rs:156-159`、`terminal/test.rs:55-70`）。

**RTL 默认关闭，理由明确：** `render/bidi.rs` 的模块头一句话讲清了定位——*Off by default. Many terminals already run implicit bidi; reordering in the app double-flips text on those hosts.* 开启后（`[scrollback.display] rtl_bidi = true`，进程级原子门闩、随 appearance 配置加载生效无需重启）只对含强 RTL 或阿拉伯数字字符的文本重排（`BidiClass::R | AL | AN`），剥离 bidi override / isolate / embedding 控制符但**不含 ZWJ/ZWNJ**，`is_table_row` 的行直接跳过以保持列对齐，RTL run 以字素簇为单位反转并**镜像配对标点**，基方向**逐行**而非按段落判定。配套成本最高的是命中测试的双向映射：`logical_slice_for_visual_cols` 由视觉列区间取回逻辑文本、`visual_col_to_logical_col` 是绘制重排的逆运算，保证搜索高亮、选择 / 拖拽复制、双击选词选 URL、链接命中区域都落在正确字形上，而粘贴回剪贴板仍是逻辑序；适用范围限于 scrollback、列表内容与全屏块查看器，**chrome、下拉、模态保持逻辑序**以便命中测试一致，文件名末尾也承认 *This is not a full mirrored RTL UI*（`render/bidi.rs:1-9, 19-31, 33-56, 74-101, 164-166, 199-320, 494-495`、`docs/user-guide/21-terminal-support.md:266-297`）。

**键盘可访问性是真实的工程投入，但没有形式化保证。** 代码层存在「键盘专用宿主」这一等公民概念——`views/turn_status.rs` 用 `Option` 表达鼠标可用性而不是布尔分支：`None` 表示 keyboard-only host（minimal 模式、无鼠标捕获），此时**同时**抑制按钮与悬停状态上报、`[stop]` 不显示、**连命中矩形都不生成**，minimal 渲染复用同一 widget 并明确以 `None` 传入（`views/turn_status.rs:61, 66, 212, 298, 1217`、`minimal/live.rs:526-527`）；模式切换时也要重置 `app.last_mouse_pos = None`，否则「Minimal 关掉鼠标捕获后没有移动事件，上一帧的悬停会粘住」（`app/mode_switch.rs:38-40`）。四类阻塞式界面（question 卡、MCP elicitation 卡、权限提示、取消轮次面板）共享一份显式契约：`Tab`/`Shift+Tab` 只在**本卡内部**行走且两端环绕、**永不把焦点挪出去**；`Esc` 逐级退出（先清卡片待处理项，清无可清才离开），question 卡与权限提示把键盘**停在 scrollback**（卡片留在屏上）以便回看上下文；多卡同时打开时优先级为权限提示 > 取消面板 > question > elicitation，快捷键栏始终显示**当前真正接收按键**的那张卡（doc 03:109-125）。鼠标动作的键盘等价逐项对照见 doc 03 的快捷键表（滚轮 ↔ `Ctrl+K`/`Ctrl+J`/`PageUp`/`PageDown`/`Ctrl+U`/`Ctrl+D`；折叠 ↔ `h`/`l`/`e`/`⇧E`；复制块 ↔ `y`/`⇧Y`；模态关闭 ↔ `Esc`/`F2`/`Ctrl+,`，注释明写「X 按钮的点击取消与这几条键盘路径镜像」），`PageUp`/`PageDown` 在 prompt 聚焦时也能滚动会话且**不移动焦点、不改草稿**，但 prompt 历史 / `@` 文件搜索 / 斜杠菜单 / 补全下拉打开时按键优先归它们（doc 03:41-110, 412-421、`app/modals.rs:1588`）。**未找到证据**：没有任何文档或穷举测试声明「每个鼠标动作都有键盘等价」这条不变量——上面的对照是从快捷键表与代码分支反推出来的，覆盖操作类点击，但不构成形式化保证。

---

## 6. 技术栈与工程实现

> 来源专题：`24-custom-widgets.md`、`25-render-pipeline-performance.md`、`26-dependency-stack.md`、`27-engineering-practices.md`；路径相对 `reference/grok-build/`。

### 6.1 自研 widget 与组件抽象

#### 本节要点

- 绘制栈分四层，Grok 自研其中三层；底座是 ratatui `0.29` + ratatui-core `0.1` + crossterm `0.28` + tui-scrollbar `0.2`（`Cargo.toml:233-234,150,296`）。
- `xai-ratatui-inline` 是 ratatui `Terminal` 的 **fork**，补的是上游不暴露的视口面积/位置、背缓冲与 resize 内部状态；它与官方 `Terminal::insert_before` 属「同源加强」而非替代（`xai-ratatui-inline/NOTICE:1-9`、`README.md:63-71`、`src/terminal.rs:1-3`）。
- `xai-ratatui-textarea` **不是** `tui-textarea` 依赖（全仓 `Cargo.lock` 无该包），NOTICE 声明为 xAI 一方代码；但 `RtOptions` / `word_wrap_line` 等符号在仓库内有两份独立实现，**「源自 fork 还是完全自研」未找到确证**。
- 全仓库只有 6 处 ratatui widget trait 实现（`pager` 4 + `textarea` 2），`pager-render` 与 `xai-ratatui-inline` 为零 —— 组件复用主要靠自由函数，而不是 trait 组合。
- 浮层的统一契约是 `fn(&mut Buffer, Rect, …) -> Option<Rect>`（需要发像素的返回 `Option<Escapes>`），几何由纯函数先算，渲染与鼠标命中测试共用同一份计算。

四层绘制栈（`render/mod.rs:1` 把 `pager-render` 定位为 "Generic rendering primitives used by the scrollback and viewport"）：

```text
┌─ xai-grok-pager ─────────────────────────────────────────────┐
│  app/agent_view、views::*、scrollback::*                      │
│  → 组装布局，调用下层原语；只有极少数类型实现 ratatui Widget    │
├─ xai-grok-pager-render ──────────────────────────────────────┤
│  render/{draw, safe_buf, renderable, color, line_utils, …}    │
│  → 帧管线、越界安全写、颜色混合、截断/换行、浮层、OSC 8 链接     │
├─ xai-ratatui-inline ─────────────┬─ xai-ratatui-textarea ─────┤
│  fork 的 Terminal + 内联视口      │  TextArea 多行/单行输入     │
├──────────────────────────────────┴────────────────────────────┤
│ ratatui 0.29 / ratatui-core 0.1 / crossterm 0.28 / tui-scrollbar 0.2 │
└──────────────────────────────────────────────────────────────┘
```

**`xai-ratatui-inline`：fork 的事实、理由与取舍。** 派生声明在 `src/terminal.rs:1-3`，授权说明在 `NOTICE:1-9`；README 自己列了必须 fork 的四项内部访问（`README.md:63-71`）：view 区域访问、直接设置视口位置、背缓冲重置与前缓冲访问、resize 时的缓冲状态。代码里新增的公开面都带 "HACK"/说明注释：`viewport_area()`（`:1203`，"HACK: this is added"）、`set_viewport_area()`（`:1247`，"HACK: this is made pub"）、`last_known_area()`（`:1210`）、`set_viewport()`（`:1216`，支持运行中在 `Inline`/`Fullscreen` 间切换）、`reset_back_buffer()`（`:612`）、`completed_buffer()`（`:312`，`test-support` 门控）。与上游的取舍点：

| 维度 | ratatui 0.29 上游 | 本 fork |
| --- | --- | --- |
| `insert_before` 与 wrap 细节 | 接收 `&mut Buffer` 网格；无 xenl 处理 | 保留原版，另加 `insert_before_rows(&[(String, bool, bool)])`（`:649`）按「折行 / 硬断行 / xenl 挂起」语义发 ANSI 行，避免把 soft wrap 变成硬换行；`xenl_pending`（`:199`）、`mid_chunk_end`（`:223`）、`write_semantic_rows`（`:852`）显式处理 DECAWM/xenl 与 `\x1b[?7l` 临时关自动换行 |
| 大终端差分 | `Buffer::diff` 的 `pos_of()` 把扁平索引截成 `u16`，`width*height > 65535` 时整屏错位 | `diff_large`（`:1043`）在 `usize` 里做除法；`flush` 默认走它（`:329-338`，注释给出 420×160 = 67200 的反例） |
| 超链接 | 无 | 每帧链接层：`LinkSpan`（`:22`）→ `set_frame_links`（`:343`）→ `diff_large_with_links` / `emit_frame_with_links`（`:1074`、`:1114`）；链接变化与单元格变化一起参与 diff，并用 OSC 8 `id=` 把换行碎片合成一个链接（`:74-95`） |
| scrolling-regions 与大视口特例 | 上游特性可选；无全高 inline 特例 | 保留两条实现，`Cargo.toml:32-34` 注明该 feature "required for terminal fork only"、pager 不用它（`:666`）；`Inline` 视口顶在 y=0 且高度 ≥ 终端高时按「占满终端」处理（`:429-434`），修的是改行后仍被旧高度截断的回归（测试 `inline_full_height_grows_with_terminal`，`:1283`） |

核心抽象只有五个：`TerminalLike` trait（`common.rs:15`，把「写 scrollback 需要的终端能力」抽成 6 个方法，`scrollback.rs` 与 `resize.rs` 只依赖它，于是 `tests.rs:12` 的 `MockTerminal` 能断言字节流）；`segment`（`segment.rs:8/84`，基于 `anstyle-parse` 的 VTE 状态机逐字节推进，只区分 `Print`/`CR`/`LF`/`Other` 四种事件（`:29-35`），用 `unicode_width` 计视觉宽度决定折行点，裸 `\r` 重置列宽（`:132-137`），超宽字符单独成段（`:156-159`）；`emit_to_scrollback`（`scrollback.rs:13`，定位视口顶 → `\x1b[J` 清到屏底 → 打印分段 → 补视口高度的 `\r\n` → 清出新视口区 → 重置背缓冲 → 平移视口）；`resize_purge_rerender`（`resize.rs:11`，整屏清 + **清 scrollback**（`\x1b[2J\x1b[3J\x1b[H`，`:17`）再重放历史并重算视口 y）；`resize_viewport_height`（`resize.rs:68`，不换终端尺寸只改视口高度，放大先向下扩、不够再顶上去，缩小时锚顶，非法高度返回 `InvalidInput`）。

**两条互相排斥的「历史归属」策略。** `ScreenMode` 有三个值（`xai-grok-pager/src/app/mod.rs:342-349`）：`Fullscreen`、`Inline`、`Minimal`（实验性 `--minimal`）。在 `Minimal` 下只有 `insert_before_rows` / `insert_before` / `set_viewport_height` 合法（`xai-grok-pager-minimal/src/lib.rs:1-6`、`commit.rs:250`）；该约束不是靠注释而是靠测试维持：`guard.rs:9-13` 用 `include_str!` 读入本 crate 所有源文件，断言不出现 `resize_purge_rerender` / `emit_to_scrollback` / `resize_viewport_height`，失败信息写明「would double-print committed scrollback」。反过来，这三个 helper 在 `pager`、`pager-render`、`pager-minimal` 三个生产 crate 里**没有调用点**，全仓只出现在 `xai-ratatui-inline/examples/inline.rs:253/270/297` 与该 guard 测试中（**未找到其他证据**）。

```text
内联模式下的屏幕（无备用屏 / --minimal）
┌─────────────────────────────────────────────┐
│ 已提交历史（终端原生 scrollback，用户自己滚）  │ ← insert_before_rows 打印一次，永不重绘
│ …                                            │
├─────────────────────────────────────────────┤
│ viewport_area（Viewport::Inline(n)，双缓冲）  │ ← 每帧 diff，只写变化单元
│  prompt ／ 状态行 ／ 下拉浮层                  │
└─────────────────────────────────────────────┘
```

一处 README 与代码不同步：README:108-111 说 resize 用 RIS `ESC c`，代码实际发 `\x1b[2J\x1b[3J\x1b[H`，`resize.rs:16` 的注释解释了原因（RIS 在 iTerm/Terminal.app 不清 scrollback）。

**`xai-ratatui-textarea`：输入组件。** 可确证的边界是：`Cargo.lock` / workspace 依赖树里**不存在** `tui-textarea`（全仓检索 `tui-textarea` / `tui_textarea` / `rhysd` 只命中本地 crate 名 `xai-ratatui-textarea`），`NOTICE:1-2` 声明 "first-party code maintained by xAI … not a wholesale redistribution of third-party source"。形状相似的旁证只到「同一组符号名在仓库内被复制多份」：`wrapping.rs:71/177/271/296` 的 `RtOptions` / `word_wrap_line` 等在 `pager-render/src/render/wrapping.rs:148/239` 又有一份独立实现，`examples/textarea_demo.rs:34` 也从本 crate 导入 `RtOptions` / `word_wrap_line`；仓库内**没有**任何指向 tui-textarea 或 textwrap 授权的注释，故**是 fork 还是自研无法定论，未找到确证**。能力面：

- 编辑核心与渲染无关：`EditBuffer` / `EditPlan`（`editor.rs:212/158`）、命令表 `EditCommand`（`:20-33`）、键位映射独立成 `editor_keys.rs`（`classify_key_event:41`、`resolve_movement:5`）、Shift/折叠边编进类型（`Movement::collapse_edge`，`:69`）、计划-校验-应用三段式（`plan_replace_byte_range:322`、`apply_plan:480`、`validate_plan:514`）、撤销分组（`textarea.rs:2186/2196/2223`）、Emacs 风 kill ring（`:2316-2341`）、单行视图 `SingleLineViewport`（`editor.rs:203`）被搜索框与模态输入复用（`scrollback/search.rs:448`、`views/memory_modal.rs:510`、`app/app_view.rs:43`）；剪贴板走 `ClipboardProvider` trait（`:48`），默认 `InternalClipboard`（`:59`），pager 侧换成 arboard（`pager-render/src/clipboard/mod.rs:11`）。
- 渲染只实现两个接口：`WidgetRef for &TextArea`（`:3134`）与 `StatefulWidgetRef`（`:3146`，`State = TextAreaState` 只有 `scroll` 一个字段），因此两个 crate 都打开 ratatui 的 `unstable-widget-ref`（textarea `Cargo.toml:9`、`pager-render/Cargo.toml:9`）。折行宽度「两枪定」：先按满宽折一次判是否溢出，溢出才把宽度减去 `1 + scrollbar_padding` 给滚动条让位；滚动条用 `tui_scrollbar::ScrollBar` 画到临时缓冲再逐格拷回；逐段渲染 `render_lines` 按 **display 坐标**累计列宽，避免元素展示文本比原始文本宽时后续内容错位。
- 内嵌元素 `TextElement`（`:88-99`）：`ElementId` + 字节区间 + 宿主自定义 `ElementKind` + 可选 `display: Line<'static>`，对光标与编辑是原子操作；接口 `insert_element:2555`、`replace_range_with_element:2568`、`restore_elements:2658`、`set_element_display:2643`，交互事件 `poll_element_event:1013` 返回点击/悬停进入/离开。

**自定义 widget 清单。** 判定口径是「是否 `impl Widget` / `StatefulWidget`（或 `WidgetRef` / `StatefulWidgetRef`）」；`xai-ratatui-inline` 与 `pager-render` 全目录 grep 无命中。

| 名称 | 文件 | 职责 | 形态 |
| --- | --- | --- | --- |
| `StatusBar` | `views/status_bar.rs:39` | 一行状态栏，左/中/右三段对齐 | `Widget` |
| `ShortcutsBar` | `views/shortcuts_bar.rs:196` | 底部快捷键提示条（渲染前逐格 `reset()` 再上底色，含待确认提示位 `with_pending`） | `Widget` |
| `ListPane<T: ListItem>` | `views/list_pane/render.rs:61` | 通用列表：选中高亮、滚动条、可选底栏/搜索栏 | `StatefulWidget` |
| `ScrollbackPane` | `scrollback/scrollback_pane.rs:125` | 主对话区；`Widget` 实现只是薄壳，真实路径是 `render_with_scratch`（`:139`）复用 `ScratchBuffer`（`scrollback/render.rs:69`） | `StatefulWidget` |
| `TextArea` | `xai-ratatui-textarea/src/textarea.rs:3134/3146` | 多行/单行输入框 | `WidgetRef` + `StatefulWidgetRef`（2 处） |
| `Renderable` | `render/renderable.rs:10` | 自研虚拟滚动 trait：`render(area, buf)` + `desired_height(width)`（注释要求 "ideally O(1)"）；已实现 `()`/`&str`/`String`/`Span`/`Line`/`Option<R>`/`Arc<R>`/`Box<R>` | 自研 trait |
| `EntryRenderer` | `scrollback/wrappers/entry_renderer.rs:543` | 单条 scrollback 条目的虚拟化渲染（`desired_height:544`、`render:563`），`skip_rows` 支持半可见条目直接落绘（`:25-28`）；`BlockContent`（`scrollback/block.rs:40`）是各 block 类型的内容接口 | 实现 `Renderable` |
| `SafeBuf` | `render/safe_buf.rs:15` | 给 `Buffer` 加越界安全的 `set_line_safe` / `set_span_safe` / `set_string_safe` / `set_line_safe_bidi` | 扩展 trait |

| 绘制原语与视图组件（全部自由函数） | 文件 | 职责 |
| --- | --- | --- |
| scrollbar 与 progress bar | `render/scrollbar.rs:165/210`、`pager/src/views/progress_bar.rs:73/59` | 用 `tui-scrollbar` 画平滑滚动条（跟随模式暗、脱离跟随变亮；几何辅助 `split_area_for_scrollbar:68`、`needs_scrollbar:102`、命中换算 `scrollbar_click_to_offset:130`）；1/8 格精度进度条（`▏▎▍▌▋▊▉█`，`:16`，ConHost 退化用 `░▒▓`） |
| 四种 overlay | `render/preview_overlay.rs:90`、`image_overlay.rs:43`、`video_overlay.rs:16`、`gboom_overlay.rs:17` | 底部锚定预览框（首 N 行 + `⋮` + 末 N 行）；图片 chip 预览浮层（有像素/无像素 × 有路径/无路径 2×2）；视频浮层框架（90% 居中框 + 标题 + 进度条 + 时间，画面由调用方 flush 后发协议）；`/gboom` 彩蛋浮层 + HUD 条。几何是纯函数 `overlay_geometry`（`image_overlay/geometry.rs:52`，最小尺寸 `MIN_BOX_WIDTH=28`、`MIN_PIXEL_BOX_HEIGHT=8`、`MIN_META_BOX_HEIGHT=6`） |
| 高亮与不可信文本 | `render/highlight.rs:14`、`render/line_utils.rs:34/58/104/177` | 搜索命中加 `Modifier::REVERSED`（`invert_cell:91`，不覆盖底层配色）；`is_unsafe_display_char` 是**唯一**清洗判定点（挡 C0/C1 与 bidi 控制符，防逃逸注入与 Trojan-Source 伪装），`truncate_str`/`truncate_line`/`fit_line_to_width`/`cascade_truncate` 做宽字符安全截断与适配 |
| 颜色语义 | `render/color.rs:281/216/226/251/270/160` | `dim_area`（浮层压暗）、`fade_region`（粘性头被顶出时渐隐）、`blend_area`/`recede_area`/`force_area_fg`/`blend_color`（256 色与真彩混色，`indexed_to_rgb:12`、`nearest_indexed:56`） |
| 工具输出、链接与 bidi | `render/terminal_output.rs:27/39`、`render/osc8.rs:104/397`、`render/bidi.rs:125/26` | PTY 原始字节 → 最小 VTE 解释器 → 带样式 `Line` + 去转义纯文本（上限 `MAX_ROWS=50_000`/`MAX_COLS=8_192`）；扫描文本产出可点链接区域；RTL 视觉重排只给需要按视觉列做命中/选择的调用方（`safe_buf.rs:4-6`） |
| 工具头路径与业务组件 | `render/tool_paths.rs:124/187/198/205`、`views/modal_window.rs:236`、`views/overlay_list.rs:18`、`views/picker.rs:127/219/878/1746/2110`、各业务弹窗 | Read/Edit 工具头的路径显示策略（`ToolPathSurface::{Collapsed,Expanded,Fullscreen}`）；弹窗共用边框/标题/关闭按钮/Tab 栏/底部快捷键/尺寸协商（`ModalSizing:90`、`predict_shortcut_rows:560`）；prompt 区共享列表浮层；选择器/命令面板共用框、搜索栏、行、分页 |
| 轨道、状态片段与调试 HUD | `views/timeline.rs:93/187/248`、`views/agent_status.rs:41/89`、`views/agent.rs:629`、`views/dock/mod.rs:1`、`views/debug_style.rs:30`、`views/{fps_hud,scroll_debug_hud}.rs` | `TimelineRail` 用「每回合一 tick」的轨道**替代**滚动条，几何每帧算一次供渲染与命中测试共用（`:1-4`）；`AgentStatusBar` 返回 `HashMap<&'static str, Rect>` 供点击命中；视图内滚动条（跟随模式把 thumb 混色变暗）；prompt 上方固定信息带（`MAX_DOCK_ROWS=8`）；主题无关的调试面板外观 + FPS / 滚动读数 |

`pager` 只在「长期存在、需要被 ratatui 布局系统按 `Widget` 调用」的四个组件上实现 trait，其余全部走自由函数：三种复用形态按「谁需要知道尺寸」选择——① trait（需被布局按 trait 调用，或虚拟滚动按宽度问高度）；② 自由函数 + `Rect`/`Buffer` 入参 + `Option<Rect>`/`Option<Escapes>` 出参（绝大多数浮层）；③ 纯几何/纯行构造（`overlay_geometry`、`compute_rail`、`progress_bar_spans`），把「算在哪画」与「画」分开，几何可单测、可被命中测试复用（`timeline.rs:1-4` 明说这是为了渲染与命中不漂移）。三种 overlay 的合成顺序一致：`dim_area(buf, area, bg, 0.5)` 先整体压暗 → `overlay_geometry(area, …) -> Rect` 纯函数定位（含最小尺寸早退 → `None`）→ 边框/标题/内容/底部提示 → 需要像素时返回 `Escapes`，由 `draw_frame` 在 diff 之后发出。`SafeBuf` 的存在理由是上游 `Buffer::set_line`/`set_span`/`set_string` 遇越界坐标会 panic（`index_of`），而 resize 竞态下算出的区域可能瞬时越界，故统一先判 `y`/`x` 再写（`safe_buf.rs:1-3/:33`）；RTL 重排只挂在 `set_line_safe_bidi` 上，`set_line_safe` 保持逻辑顺序，因为下拉/弹窗的命中测试基于逻辑列（`:4-6`）。样式不自造合并层，直接用 ratatui `Style` + `Buffer::set_style`。

**为什么拆成独立 crate。** ① 工作区显式登记（`Cargo.toml:86-87`），依赖面窄（inline 4 个、textarea 7 个），处于依赖图叶子位置；② 跨渲染模式复用：同一个 `Terminal` 服务三种 `ScreenMode`，minimal 复用 `EntryRenderer`（`xai-grok-pager-minimal/src/lib.rs:4`），并刻意只经 pager 再导出的 `PagerTerminal` 触达该 crate，为此给 `cargo-shear` 加了 `ignored = ["xai-ratatui-inline"]`（`minimal/Cargo.toml:6-13`）；③ 可测试性（证据最充分）：`MockTerminal` 实现 `TerminalLike` 以断言写出的字节与视口 y（`tests.rs:12`，用例见 `scrollback.rs:104-147`、`resize.rs:284-304`），`tests/segment_differential.rs:1-12` 把**上一版基于 termwiz 的实现逐字内嵌**为参照、断言新 `anstyle-parse` 实现输出一致（`termwiz` 只作 dev-dependency，`Cargo.toml:21-23` 注释 "Not linked into shipped binaries"），另有 criterion 基准与 `test-support` 暴露的 `completed_buffer`；④ 行为约束固化在测试里（minimal 的源码扫描 guard）。**编译时间收益未找到证据**：仓库内没有关于「拆 crate 以缩短编译时间」的说明，只能给出结构性事实——两个 widget crate 依赖极少、处于叶子，而 `pager-render` 自身依赖 40 余项（`Cargo.toml:8-70`，含 syntect、vte、image、tokio、notify 等），并不是编译时间的收益点。

### 6.2 渲染管线与性能架构

#### 本节要点

- 整个 TUI 是**按需驱动**的：事件循环是一个 `biased` 排序的 `tokio::select!`（`crates/codegen/xai-grok-pager/src/app/event_loop.rs:2110`），所有分支只做「改状态 + 标脏」，真正的绘制只发生在每轮循环末尾唯一一次 `presenter.present_if_dirty(...)`（`:2938`）；空闲时终端收到**零字节**（`crates/codegen/xai-grok-pager-render/src/render/draw.rs:689-706` 有测试）。
- 「这一轮要不要重绘」是三个独立条件的与：`dirty` ∧ 无在途未确认帧 ∧ writer 队列已排空；在途帧闸门同时构成对慢终端的**背压**（`event_loop.rs:579-600`）。
- 三套彼此独立的时钟：绘制上限默认 16 ms、动画 tick 默认 30 fps、滚动合帧 16 ms。脏标记**只有一个全局 `bool`**，没有区域级脏矩形，细粒度靠 `Buffer` 逐 cell diff。
- 输出层刻意绕过 ratatui `try_draw()`，为的是保住光标闪烁；写字节在独立 `term-writer` 线程上（32 KiB 帧缓冲 + 64 KiB `BufWriter`，每帧带单调序号与回执）。
- 长会话不删条目，只淘汰屏幕外 entry 的重型渲染缓存（保留区 = 测量窗口 ± 128，选中项例外），并靠窗口化渲染 + 懒测量避免全量重排；重路径（搜索、编辑高亮、Mermaid）全部移出 UI 线程。
- `benches/bench.md` **不是实测数据**（自述 "Generated for benchmarking purposes."）；仓库未提交任何基准结果或 baseline 文件。

事件循环自己的模块注释写清了职责边界：「A thin `tokio::select!` loop. All input routing, rendering, and state management is delegated to `AppView`. The event loop only handles IO…」（`event_loop.rs:1-4`）。分支优先级由 `biased;`（`:2111`）与注释给出：`connection_cancel` → `quit_notify`（刻意排在 ACP 洪流之前）→ writer 回执 → writer 卡死诊断（`:2159`）→ ACP（**门控** `if input_rx.is_empty()`，`:2173`）→ 后台任务 → 版本检查 → 终端输入 → 四个定时器 → `scroll_tick` → `animation_tick` → `voice_rx`（刻意放最后，注释说明它几乎永远 ready，放前面会饿死其它分支）。ACP 门控的取舍在代码里留有记录：

> Gating, not reordering: moving input above ACP would flip the starvation direction (streaming redraws starving behind held keys) —— `event_loop.rs:2171`

ACP 侧还有两条批处理策略：立即抽取最多 32 条已就绪消息（`ACP_DRAIN_BATCH_MAX = 32`，`:1837`），且**一旦输入到达就中断抽取**（循环条件里的 `input_rx.is_empty()`），保证滚轮/按键最多等一批。主循环伪代码（据 `event_loop.rs:2113-2890` 与 `:2938` 归纳）：

```text
loop {
    // 循环顶：处理上一轮遗留的挂起状态（非阻塞），并为每个「有 deadline 的东西」准备 future
    run_pending_suspends(...); run_pending_mode_switch(...)     // 编辑器/转录 tty 交接、fullscreen <-> minimal
    animation_tick = sleep_until(animation_tick_at)             // 配置 fps，默认 30；TickDemand::None 时不排
    scroll_tick    = sleep_until(scroll_clock_deadline(now))    // 默认 16ms；另有 resize_debounce(now+16ms)、
    deferred_draw  = sleep_until(presenter.draw_scheduled_at)   //   deferred_draw、writer_blocked、stall_flush
    presenter.observe_writer_progress(queued, written, now)     // 每轮一次的水位观测（诊断用）

    select! { biased;
        cancel | quit                   => break
        writer_event                    => presenter.acknowledge(seq)     // Written(seq) 回执解闸
        writer_blocked                  => 上报 term.writer.blocked
        acp    (仅当 input_rx 为空)      => 处理 + 最多再抽 32 条；state_changed ⇒ request_throttled()
        task_complete                   => dispatch
        input                           => drain_and_process(...)         // 批处理 + 合并
                                           needs_draw ⇒ request()；resize_only ⇒ 只排 debounce
        resize_debounce | deferred_draw => presenter.request(false)
        scroll_tick                     => if app.tick_scroll() { presenter.request(false) }
        animation_tick                  => app.tick() / 各类 reconcile
        voice (刻意放最后)               => …
    }
    if stall_rollup 到期 { emit_event_loop_stall(window) }
    处理 app.pending_effects
    presenter.present_if_dirty(&mut app, terminal)     // ← 唯一的绘制点
}
```

**重绘判定与背压**（`Presenter`，`event_loop.rs:499-647`）：`try_present`（`:579-600`）先看 `written < queued_before` 直接返回 false，再看 `in_flight_target.is_some() || !self.dirty` 返回 false；通过后清 `dirty`、取走 `force_full_repaint`、执行 `draw`，并用 `queued_after()` 记录在途水位；回执由 `acknowledge(sequence)` 清闸（`:530-541`，要求 `sequence >= target`）。配套 `WRITER_BLOCKED_WARN_AFTER = 5s`（`:487`）与 `observe_writer_progress`（`:542-565`）：**队列有积压且水位零推进**才算一次 stall episode，一旦有推进就重新锚定，避免慢终端累积成假警报。`force_full_repaint` 只在少数场景置位（例如窗口重新聚焦且终端会带外重绘 pane，`:2344-2349`），此时先 `terminal.clear()` 再 `overlay::reset_owner()`。

**渲染节流与合帧。** 三套时钟互不替代：

| 时钟 | 默认 | 来源 | 控制什么 |
| --- | --- | --- | --- |
| 绘制上限 `min_draw_interval` | 16 ms（≈62.5 fps） | `GROK_MIN_DRAW_MS`，或探测到的显示器刷新率 | 一帧 `draw_frame` 的最快重复间隔 |
| 动画 tick | 30 fps | `[animation] fps`（1–60） | `app.tick()` 的节奏 |
| 滚动 flush | 16 ms | `GROK_SCROLL_CADENCE_MS` | 滚轮/触控板的合帧节奏 |

绘制上限的解析在 `crates/codegen/xai-grok-shell/src/util/config/resolve/display_refresh.rs`：默认常量 `DISPLAY_REFRESH_DEFAULT_CADENCE_MS = 16`（`:12`），允许区间 1–100（`display_refresh_startup.rs:15-16`）；若开启显示器刷新率探测（默认开），则 `ms = clamp(round(1000/hz), 8, 16)`（`:59-63`），hz 须落在 `[55, 240]`，两个 env 都设置时 env 胜（`merge_motion_cadence`，`:282-306`）；结果在启动时固化为 `MotionClocks`，事件循环读一次即用整个生命周期。动画 tick 不是无条件 30 fps：`TickDemand::{None, Slow, Fast}`（`app/app_view.rs:245-257`）决定是否排下一次——`None` 时 `schedule_tick` 直接 return，事件循环**完全 park（零唤醒）**；`Slow` 抬到 `SLOW_TICK_INTERVAL = 83ms`（约 12 fps，对齐欢迎页 logo 的 `SHIMMER_FPS`）。resize 用 `RESIZE_DEBOUNCE = 16ms` 防抖（`event_loop.rs:34`），每个新 resize 重置定时器；一条 const 断言把它钉在 `SLOW_TICK_INTERVAL` 之下，理由是「the debounced draw must record the new width before the forced re-run reads it」（`:35-38`）。例外：屏幕上有已提交的 iTerm2 图像覆盖层时 resize **立刻**画一帧（`:2358-2368`），因为 iTerm2 在拖拽期间会把已提交像素抹开。脏标记只有 `Presenter.dirty: bool`（`:501`），请求入口两个：`request(force_full_repaint)`（`:601-605`）与 `request_throttled`（`:606-615`，超上限则把 `draw_scheduled_at` 记成 `last_draw_at + min_draw_interval`，由 `deferred_draw` 到点补画）。

**DEC 2026 同步更新。** 每帧被 `BeginSynchronizedUpdate` / `EndSynchronizedUpdate` 包住，使「cell diff + 图像 + 光标移动」原子呈现（`render/draw.rs:37`，代码在 `:513-537`）；两个标记永远同进同出，是否包裹由终端上下文单点决定：`should_emit_synchronized_output(ctx) { !(ctx.is_tmux_backed() && ctx.embedded_editor.is_none()) }`（`terminal/tmux.rs:55-57`）——即 **tmux 直连时故意不发 DEC 2026**：tmux 在 block 关闭时会重绘整个 pane，并且它自己已经朝外层终端做了同步（`draw.rs:38`）。测试用字面量 `\x1b[?2026h` / `\x1b[?2026l` 钉住序列（`draw.rs:711-712`）。

**输出层：`Buffer` → 终端字节。** `draw_frame` 六步（`render/draw.rs:501-540`）：

```text
draw_frame(terminal, cursor, ctx, render_fn)                                  draw.rs:501
  ① 可选 queue(BeginSynchronizedUpdate)                                        :513-516
  ② terminal.autoresize()                    // 吞掉尺寸变化                   :517
  ③ frame = terminal.get_frame(); render_fn(&mut frame, &mut link_spans)       :520-524
  ④ terminal.set_frame_links(&link_spans)                                     :525
  ⑤ has_changes = terminal.flush_with_links(); terminal.swap_buffers()  :528-529
  ⑥ if !has_changes && !post_flush && action == CursorAction::None { writer.discard(); return; }
        // ↑ 空帧零字节；否则 post_flush.write_to(...) → cursor.apply(action) → 可选 EndSync → flush()
```

- **为什么绕过 `try_draw()`**：`try_draw()` 每帧无条件发光标转义（`Show`+`MoveTo`，或 `Hide`），而 `Show`/`MoveTo` 都会重置终端的**光标闪烁计时器**，30 fps 下 500 ms 的闪烁周期永远走不完，光标看起来是实心的（`draw.rs:1-30`）。替代方案是把光标管理完全交给 `CursorState`，用去重表把「无 cell 变化 + 位置不变」变成零光标指令（`:439-486`）；空帧零字节有测试 `idle_frame_emits_zero_bytes`（`:691`）。
- **自定义 diff**：不用 ratatui `Buffer::diff`，因为上游 `pos_of()` 在除以宽度前把扁平索引截成 `u16`，静默在 65 535 处回绕（`xai-ratatui-inline/src/terminal.rs:1040-1042`）。`diff_large`（`:1043`）在 `usize` 里做除法，并维护两个游标：`to_skip`（当前宽字符占用的额外列）与 `invalidated`（受影响宽度，保证宽字符被覆盖时其邻居也被重写）。
- **链接参与 diff**：`link_ids`（每 cell 一个 u32，0 表示无链接）与 `link_tables` 一并比较，**同一 cell 只要链接变了也要重写**（`terminal.rs:1071-1113`），链接的增删改无需带外重绘；为保持常见路径零开销，当前帧与上一帧都没有链接时 `flush_with_links` 直接退回纯 `flush()`，逐字节等价（`:391-400`）。`Terminal` 持两个 `Buffer` 加两套链接表（`:137-172`），`flush` 返回 `bool` 表示有没有 cell 被写过。
- **写线程与缓冲**：`TermWriter` 自带 32 KiB `Vec<u8>`（`draw.rs:205`），`flush()` 非空时 `mem::take` 出来投进 mpsc（`:224-233`，落空则不投——这是空闲帧零字节的关键）；`send_payload` 用 `debug_assert` 强制**单生产者**（`:171-185`）；每个 payload 带单调递增 `sequence`，写完推进 `written` 水位（`:105-109,342-360`）。writer 线程（名字 `term-writer`）在 stderr 的 dup fd 上再套一层 64 KiB `BufWriter`（`:374-437`），意义是隔离阻塞：终端不读 pty 时**只有 writer 线程停住**，事件循环继续处理定时器、事件与 ACP。另有带外通道 `EscapeWriter` 供非绘制路径投递转义（如重新聚焦时重发鼠标捕获），注释记录了现场教训——在事件循环里内联锁 stderr 会在终端停止读取时死锁（`:244-248`）。`PostFlush`（`terminal/overlay.rs:73`）承载图片/视频等「diff 之后才能发」的字节，保证同一 DCS 块内原子呈现。
- **一处必须澄清的命名**：`render/terminal_output.rs` 尽管名字像输出层，实际是**工具输出的转义解析器**——bash/终端类工具返回原始 PTY 字节流（SGR、光标移动、行擦除、`\r` 进度条原地重写），ratatui 会把它们当字面量打印成 `[1m[36m`，因此该模块基于 `vte` 做一个**面向行的最小 VTE 模拟器**。与真正的屏幕/网格模拟器不同，它保留**无界的完整转录**以便映射到 pager 的行模型；两个上限只为挡住畸形转义（`MAX_ROWS = 50_000`、`MAX_COLS = 8_192`）；每次调用都新建模拟器，保证渲染路径与高度缓存路径的结果确定且幂等（`terminal_output.rs:10-24`）。调用点：`scrollback/blocks/tool/execute.rs:121,516`、`blocks/tool/list_dir.rs:171`、`views/block_viewer/mod.rs:816`。

**FPS / 卡顿诊断工具三件套。** ① **FPS HUD**（`views/fps_hud.rs`）刻意做成 release 可开：dev-only 的 `FrameMetrics` 会改动帧管线（把分阶段计时刻进 `draw_frame`），而 dev-only 的 HUD 无法探测生产路径（`scroll_debug_hud.rs:13-16`）；它量的是**整个 `draw_frame` 调用的墙钟耗时**（渲染 + flush + writer 交接），只观测、不改管线。注释专门澄清语义：「"fps" here is render throughput (1 / mean frame cost), not paint frequency. The pager draws on demand, so an idle UI paints nothing and a busy one is bounded by this number.」（`fps_hud.rs:12-13`）。参数：`SAMPLE_CAP = 120`（约 4 s @30 fps）、`REFRESH = 250ms`、`PANEL_WIDTH = 32`；`GROK_FPS` 真值判定为「非空且不等于 `"0"`」，`/debug fps` 可运行时切换；统计量为均值 fps + p50 + p95（线性插值百分位）；关闭时每帧一次 bool 判断；`HONORS_GROK_FPS_ENV: bool = true`（`:27-29`）保证 `GROK_FPS=1` 永不静默失效。② **滚动诊断 HUD**（`views/scroll_debug_hud.rs`）：右上角 46 列面板（`PANEL_WIDTH = 46`）逐帧打印滚动状态机快照，配方 `GROK_FPS=1 GROK_SCROLL_DEBUG=1 grok --resume <session>`；`GROK_SCROLL_DEBUG` 启动即开，隐藏命令 `/scroll-debug` 运行时开关；快照字段见 `input/mouse.rs:471-496`（`events_per_tick`、`wheel/trackpad_lines_per_tick`、`viewport_height`、`flush_cap`、`cadence_ms`、流状态 `kind`/`promoted`/`events`/`avg_interval_ms`/`accel`/`desired`/`applied`/`backlog`/`carry_lines` 等）；`GROK_SCROLL_LOG=1` 额外把每个滚动事件写成 JSONL（`input/scroll_log.rs:1-11`）；不变量是 HUD 绝不改变滚动行为。③ **事件循环卡顿聚合**（`app/event_loop_stall.rs`）：60 秒滚动窗口（`STALL_REPORT_WINDOW = 60s`），`StallRollup::observe` 只保留窗口内**最差一次**卡顿及当时活动快照（`compaction_active`/`subagents_active`/`mcp_servers_connected`）；窗口**从不切分**、到期必然 flush，首次 observe 前 `window_started_at` 为 `None`，**空闲循环因此不开窗口也不设定时器**。与之并行的是 writer 侧诊断：卡住 5 s 上报 `term.writer.blocked`（含 queued/written 水位），恢复时记 `term.writer.recovered`，注释明确这是「循环还活着但屏幕冻住」的现场诊断（`:2154-2156`）。**未找到证据**：`render::frame_metrics` / `FrameMetrics` 在本快照中被 `fps_hud.rs:3,8,27,49`、`scroll_debug_hud.rs:13,26` 反复引用，但全仓搜不到其源文件，`app_view.rs:2224-2226` 的 `dev_fps_rows()` 直接返回字面量 `0`（注释 "0 in non-dev builds"），故本文不描述该 dev overlay 的行为。

**基准数字。** `crates/codegen/xai-grok-pager/benches/` 下是 4 个 criterion 基准 + 1 篇说明文档：

| 文件 | 基准名 | 测什么 | 语料规模（源码写死） |
| --- | --- | --- | --- |
| `render.rs` | `render/single_frame`、`render/full_scroll`、`render/windowed_scroll` | 视口渲染与滚动时的逐帧成本 | 120×50 视口（`:25-26`）、`SCROLL_STEP = 10`（`:29`） |
| `render.rs` | `reveal/navigate_skip`、`reveal/navigate_rebuild` | 搜索 n/N 定位的「跳过 vs 全量 rebuild」 | `REVEAL_ENTRIES = 3000`，注释写「约 3,200 条 ≈ 5 MB 的长会话」（`:32-33`） |
| `resize.rs` | `resize/width_step`、`resize/drag_20_steps`、`resize/same_width_noop` | 拖拽边缘时的重排成本 | `TURNS = 400`，注释写「约 3,200 条 / 约 5 MB，数小时会话的规模」（`:15-19`） |
| `search.rs` | `search/scan`、`search/query_steady`、`search/query_cold` | 后台 daemon 前后的正则扫描与 UI 线程成本 | `CORPUS_ENTRIES = 30_000`（`:20`） |
| `edit_highlight.rs` | `edit_hl/matrix`、`edit_hl/upgrade` | 编辑 diff 的语法高亮策略 | 500 行 + 8 hunk、10 000 行 + 40 hunk（`:290,311,332`） |

`resize.rs:1-6` 的模块注释是这套基准里最有信息量的一段，它直接列出三个**曾在真实会话上测量并移除**的回归：① 按宽度重新派生 entry 源文本（使估算 pass 变成 O(对话字节数)）；② 每个 entry 都构建或克隆一次 `AppearanceConfig`；③ 每次 resize 都跑 `warm_measure_pages_above`。关于 `bench.md` 必须做一处澄清：**它不是实测结果**——文件末尾自述 "Generated for benchmarking purposes."（`bench.md:337`），性能数字用的是期望语气。可引用的是两类：**结构性结论**——「步骤 1–3（markdown 解析、语法高亮、换行）跨帧缓存，只有步骤 4–5 每帧运行」（`:18`），"Buffer diff + flush" 的复杂度是 **O(变化 cell 数) 而非 O(总 cell 数)**（`:16`）；**期望量级**（引用时须标注为期望）——120×30 视口 + 200 行 markdown「预期 ~50–200 µs/帧」，其中约 80% 花在 `EntryRenderer::render()` 与 scratch buffer 拷贝、约 15% 在缓存命中路径的 `BlockContent::output()`、约 5% 在布局计算，「若 >500 µs/帧，可能有意外的 cache miss 或热路径分配」（`:305-314`）。另有两处不一致/空白：`bench.md` 用 120×30 视口，而 `render.rs:25-26` 实际用 120×50；`benches/` 目录下**没有提交任何基准结果或 baseline 文件**。

**已知性能陷阱与代码里的对策**（反直觉发现集中在「已测量并移除的回归」里）：每击键同步跑正则扫描会卡住输入线程 → 后台 `SearchDaemon` + `Arc` 零拷贝语料；拖拽边缘每秒数十次全量重排 → 16 ms 防抖 + 延迟 `warm_above` + 复用宽度轮廓；编辑 diff 整文件 syntect 会挡住首帧 → 首帧 hunk-only，worker 线程升级（2 MiB / 50k 行上限）；每帧重排/重解析 → wrap/output/高度分层缓存 + 懒测量 + 正则 `OnceLock`；终端不读 pty ⇒ 事件循环被写阻塞 → writer 线程 + 在途帧闸门 + 队列水位诊断。

**长会话下的内存与滚动设计。** 条目本身不删，淘汰的是屏幕外 entry 的**重型渲染缓存**：`evict_offscreen_render_caches`（`scrollback/state/layout.rs:621-648`）遍历全部 entry，跳过保留区（测量窗口 ± `EVICT_KEEP_MARGIN_ENTRIES = 128`）与当前选中项；`evict_render_cache`（`entry.rs:258-268`）只丢 `cached_output` 与 block 内部可重建缓存，**布局（entry 高度、滚动位置）不动**，滚回视口时会透明重渲染；触发节奏 `CACHE_EVICT_INTERVAL = 5s`（`app/app_view.rs:5059-5089`），注释解释「sweep 是 O(entries) 的指针级遍历，相对一次帧渲染微不足道，但也没有理由每帧跑」；动机写在 doc 注释里：多 MB 转录的重型拷贝「轻易达到数百 MB，而这些内容不滚动就永远看不到」（`layout.rs:622-624`）。分配器侧还有一层：jemalloc 会把释放的页留在进程里，所以库在「重型瞬时对象刚被丢弃」的边缘触发 purge，**从不按帧触发**；draw/tick 路径内部命中的内存悬崖改用 `request_release_after_draw(reason)` 推迟到本次 flush 之后，避免 purge 卡住帧（`src/memory_release.rs:1-9,38-60`）。

滚动侧的策略一句话写在模块头：「Redraw is coalesced to a fixed cadence.」（`input/mouse.rs:5`）。常量集中在 `mouse.rs:46-77`：`STREAM_GAP_MS = 80`（手势结束判定）、`REDRAW_CADENCE_MS = 16`、`MIN_DELTA_PER_FLUSH = 6`；每次 flush 的行数上限 `flush_cap = max(viewport_height / 2, MIN_DELTA_PER_FLUSH)`（`:429-430`），**超出的行留在流里**由后续 cadence flush 继续送；`on_scroll_event_at` 只在距上次重绘达到 cadence（或手势刚升级）时才产生行（`:622`），`on_tick_at` 负责 cadence flush 与 80 ms 间隙收尾（`:657-694`）。缓存与窗口：宽度轮廓 `cached_line_widths`（`entry.rs:102`）避免每次换宽度都重新派生源文本，`invalidate_width_caches`（`:252-257`）精确失效 `cached_output` / `cached_truncated_height` / `cached_estimate_lines`；高度缓存键是 `(content_width, raw, theme, cwd)`，`is_selected` 刻意不入键（理由是「行数从不依赖选中状态」，`:390-396`）；OSC 8 路径识别的三条正则用 `OnceLock`（`render/osc8.rs:212-249`）；搜索高亮的文本缓冲在所有可见行间复用，「每帧最多分配一次而不是每行一次」（`scrollback/render.rs:256-258`）。避免每帧重排靠 `LayoutCache`（`state/layout.rs:44-70`，保存每 entry 的 `EntryLayoutInfo`、截断高度、`measured: Vec<bool>`、`virtual_y` 前缀和等，定位可见 entry 用 `virtual_y` 上的 `partition_point` 二分）与**估算 + 懒升级**的高度策略（只有进入或接近视口的 entry 才精确测量，窗口是可见范围向下再扩 `MEASURE_MARGIN_ENTRIES = 8`）；渲染只走窗口：`paint_window(...)` 算出该画的 entry 区间，`render_scrolled_entries_with_scratch` 只遍历这个切片，被顶/底裁掉一半的 entry 用可复用的 `ScratchBuffer` 先整条渲染、再只把可见行拷进输出（`bench.md:150-171` 把这一 cell-by-cell 拷贝标为最热路径）。resize 与大历史的交叉点是 `warm_measure_pages_above`：它在底部钉住时预测量上方几页，但 resize 期间代价最大（「每次拖拽事件都预热三个屏幕外的页，然后在下一次宽度变化时全部丢掉」），因此 resize 时置为 `Deferred`，只在下一次 `begin_frame()` 之后才真正执行（`state/mod.rs:1466-1476,1555-1568`）。三条重路径被移出 UI 线程：滚动搜索 daemon（通道刻意用 unbounded，注释解释有界通道会让第 257 次击键被卡住、`try_send` 丢满则可能丢掉最后一次查询；这段扫描以前**每击键同步跑在输入线程上**）、编辑 diff 高亮 worker（首帧只用 hunk 范围高亮，随后独立 `std::thread` 跑一次 syntect pass，按 `entry_id` 合并、最新任务胜；超出 `EDIT_HL_MAX_BYTES = 2 MiB` / `EDIT_HL_MAX_LINES = 50 000` 或读文件失败就**停留在 hunk-only，不做无界工作**）、Mermaid worker（同样的 worker + 磁盘缓存清理模式）。

### 6.3 依赖、版本与 feature 门控

#### 本节要点

- 工具链基线：Rust `1.94.0` + `edition = "2024"` + `resolver = "2"`；`rustfmt.toml` 只有一行 `use_field_init_shorthand = true`，风格几乎完全交给工具默认。
- 规模：workspace 101 个 member，`Cargo.lock` 1347 个 package（registry 1242 + in-tree 101 + git 4），101 个 in-tree 与 101 个 member 完全吻合；110 个 crate 名存在 ≥2 个版本。
- TUI 运行时依赖面是「ratatui `0.29` + crossterm `0.28`」的经典组合 + `textwrap`/`unicode-width`/`unicode-segmentation`/`anstyle`/`supports-color`/`vte`/`strip-ansi-escapes`，全部无可选门控，属生态事实标准。
- 同一构建里并存两代 ratatui API（`ratatui 0.29.0` 与 `ratatui-core 0.1.0`），是生态过渡期的真实状态，不是刻意设计。
- feature 门控主要服务三件事：测试专用辅助（`test-support` + `default-bazel` 双默认集）、平台/环境分支（`local-workspace` 217 处 `cfg`）、把重依赖推到边界。
- **5 个 pager 可选依赖在源码里零引用**，`pager-render` 的 `notify` 同样无可达使用点——合稿引用「可选依赖」时不要把它们当成真实能力开关。

**工具链、edition、lint 与格式化配置。** `rust-toolchain.toml:11-17`：

```text
channel    = "1.94.0"                         # :11
components = ["rustfmt", "clippy"]            # :12
profile    = "default"                        # :13
targets    = [x86_64-unknown-linux-gnu, aarch64-unknown-linux-gnu]   # :14-17
```

文件内注释要求「手动升级、一次只跳一个小版本、发布后等两周」，升级后跑 `cargo check --all-targets --workspace` 与 `cargo clippy --all-targets --workspace`（`:3-10`）。仓库根 `Cargo.toml` 首行明确写了它是**自动生成**的、改动应落到各 crate 的清单（`Cargo.toml:1`），这解释了为什么 `[workspace.dependencies]` 集中托管版本而绝大多数 crate 用 `{ workspace = true }` 继承；edition `2024` 由 workspace 统一（`Cargo.toml:113`）。与体积/发布相关的 profile：`release`（`incremental = true`、`panic = "abort"`，`:387-389`）、`release-dist`（继承 release，`lto = "thin"`、`codegen-units = 1`、`strip = false`、`debug = 1`，`:400-406`；注释解释为何不设为默认——本地构建会慢约 2.2 倍，`:391-399`，该数字来自注释、本次未复测）、`admin-cli-dist`（`opt-level = "z"`、`lto = true`、`strip = true`，`:430-435`）、`dev`（`panic = "abort"`、`codegen-units = 128`、`debug = "line-tables-only"`，`:437-448`）。

`clippy.toml` 值得注意：它**躺在仓库根，却自称是 `crates/codegen/**` 的配置**，并要求「复制 repo-root //clippy.toml 的任何设置」（`:1-4`），而全仓只有这一个 `clippy.toml`、被引用的「//clippy.toml」在快照中不存在。可迁移的三条：`large-error-threshold = 256`（`:7`，为 tonic 生成代码放宽大 `Err` 变体告警）；`allow-indexing-slicing-in-tests = true`（`:13`），配套约定是「生产 crate 在 crate 根用 `#![deny(clippy::indexing_slicing)]` 自行开启」（`:9-12`）；`disallowed-methods` 共 12 条（`:40-54`），是**团队级架构约束用 lint 表达**的范例——`reqwest::Client::new` / `ClientBuilder::build` 必须走 `xai_grok_extra_ca`（TLS 策略），`std::fs::canonicalize` / `Path::canonicalize` / `tokio::fs::canonicalize` 必须用 `dunce::canonicalize`（Windows `\\?\` 前缀），`std::env::home_dir` / `dirs::home_dir` 统一走 `xai_dirs::home_dir`，`std::process::Command::spawn` / `tokio::process::…` / `portable_pty::SlavePty::spawn_command` 必须经 `xai_tty_utils::ProcessScope::enroll`（终端 pid 登记）。`[workspace.lints.clippy]`（`Cargo.toml:453-477`）**全部是 allow、没有 deny 项**，前四项与 proto 生成代码/历史代码妥协、后两项为无 merge queue 的团队流程与 fastrace 宏让路，注释写明了理由与解除条件；83 个 `crates/` 下的 `Cargo.toml` 通过 `[lints] workspace = true` 选择性继承。`.cargo/config.toml` 只有 rustflags、无 alias：Darwin 加 `-undefined dynamic_lookup` 与 `-ObjC`，linux-gnu 加 `-C force-unwind-tables=yes`，aarch64-gnu 额外加 `-C target-cpu=neoverse-v2`，两个 musl target 加链接硬化 `-Wl,-z,relro,-z,now,-z,noexecstack`（Full RELRO + NX stack，`:10-59`）。

**TUI 依赖总表。** 版本列格式为「workspace 声明 / Cargo.lock 实际锁定」；只有 `pager`、`pager-render`、`xai-ratatui-inline`、`xai-ratatui-textarea`、`xai-grok-markdown` 五个 crate 里的才算「TUI 运行时依赖」。

| crate | 版本 | 在 TUI 里的职责 | 可选 | 证据 |
| --- | --- | --- | --- | --- |
| ratatui | `0.29` / 0.29.0 | TUI 框架本体（Buffer / Widget / Layout / Frame）；pager 启用 `crossterm` + `unstable-widget-ref`，render 追加 `unstable-backend-writer` | 否 | `Cargo.toml:233`、`Cargo.lock:8602`、`pager/Cargo.toml:27` |
| ratatui-core | `0.1` / 0.1.0 | 与 ratatui 0.29 **并存**的下一代核心 API；`tui-scrollbar 0.2.2` 依赖它 | 否 | `Cargo.toml:234`、`Cargo.lock:8623` |
| crossterm | `0.28` / 0.28.1 | 终端后端：raw mode、事件流、鼠标、bracketed paste（feature `event-stream` + `bracketed-paste`） | 否 | `Cargo.toml:150`、`Cargo.lock:2312`、`pager/Cargo.toml:29` |
| tui-scrollbar | `0.2` / 0.2.2 | 滚动条组件（render 与 textarea 共用） | 否 | `Cargo.toml:296`、`pager-render/Cargo.toml:27` |
| vte | `0.15.0`（另有传递引入的 0.14.1） | VTE 状态机：命令输出按 SGR/光标/`\r` 原生渲染 | 否 | `Cargo.toml:306`、`pager-render/Cargo.toml:44` |
| ansi-to-tui | `7.0.0` | ANSI 文本 → ratatui 文本（tracing 面板、status line 清洗） | 否 | `Cargo.toml:119`、`Cargo.lock:190`、`pager/Cargo.toml:55` |
| textwrap / unicode-width / unicode-segmentation / strip-ansi-escapes | `0.16` / 0.16.2；`0.2` / 0.2.0（传递引入 0.1.14）；`1.12.0`；`0.2.1` | 折行（覆盖层、隐私横幅、列表面板、启动失败页）；显示宽度（CJK/emoji 双宽）；字素簇边界；去转义后再测量/落盘（标题、peek、会话事件） | 否 | `Cargo.toml:273,302,301,265`、`pager/Cargo.toml:30,32,33,34` |
| unicode-bidi / unicode-bidi-mirroring | `0.3` / `0.4`（0.3.18 / 0.4.0） | 双向文本与括号镜像（仅 render） | 否 | `Cargo.toml:298-299` |
| linkify | `0.10` / 0.10.0 | URL 识别 | 否 | `Cargo.toml:197`、`pager-render/Cargo.toml:26` |
| anstyle / anstyle-lossy / anstyle-parse / anstyle-syntect | `1.0` / `1.1.4` / `0.2` / `1.0.4` | ANSI 样式模型与解析、与 syntect 的互转桥 | 否 | `Cargo.toml:121-124`、`xai-ratatui-inline/Cargo.toml:12` |
| supports-color | `3.0` / 3.0.2 | 颜色能力探测 | 否 | `Cargo.toml:267`、`pager-render/Cargo.toml:33` |
| dark-light | `2` / 2.0.0 | 系统深浅色探测（render 直接写版本号，未走 workspace） | 否 | `pager-render/Cargo.toml:47` |
| syntect | `5.3`（`default-features = false`，`features = ["default-onig"]`）/ 5.3.0 | TextMate 语法高亮；**走 onig 而非纯 Rust 后端**，因此引入 C 依赖 `onig_sys 69.9.3` | 否 | `Cargo.toml:268`、`pager-render/src/syntax.rs` |
| two-face | `0.4`（`syntect-onig`）/ 0.4.5 | 扩充语法/主题集；仅被 `xai-grok-markdown` 的 **build-dependency** 使用 | 否（构建期） | `Cargo.toml:297`、`xai-grok-markdown/build.rs:1-24` |
| similar | `2.7` / 2.7.0 | 行级 diff（edit 块、block viewer、edit_highlight_worker） | 否 | `Cargo.toml:262`、`pager/Cargo.toml:45` |
| indexmap | `2`（`serde`）/ 2.14.0 | 有序 map，保证面板/行顺序稳定 | 否 | `Cargo.toml:189`、`pager/Cargo.toml:46` |
| nucleo | git `helix-editor/nucleo` rev `5b74652` / 0.5.0（+ `nucleo-matcher` 0.3.1） | 模糊匹配：slash 命令、历史搜索、文件搜索 | 否 | `Cargo.toml:209`、`pager/Cargo.toml:87-88` |
| pulldown-cmark | `0.13` / 0.13.0 | Markdown 解析；`xai-grok-markdown-core` 的存在目的就是「共享完全相同的 pulldown-cmark 配置」 | 否 | `Cargo.toml:230`、`xai-grok-markdown-core/Cargo.toml:6-10` |
| portable-pty / alacritty_terminal | `0.9` / 0.9.0；`0.26.0` | PTY：`grok wrap` 包装器、e2e、benchmark（pager 里**直接写版本号**而非 `workspace = true`）；终端仿真内核**不在 pager 二进制内**，只在 `ptyctl` 与 `xai-grok-pager-pty-harness`（spawn-only 测试/基准） | 否（后者属工具链/测试） | `pager/Cargo.toml:57-58`、`Cargo.toml:118,223`、`ptyctl/Cargo.toml:10` |
| termwiz / ansi-width | `0.23`（`default-features = false`）/ 0.23.3；`0.1` / 0.1.0 | 仅 `xai-ratatui-inline` 的 dev-dependency：分割器的差分参考实现、宽度基准 | dev only | `Cargo.toml:272,120`、`xai-ratatui-inline/Cargo.toml:18,24-26` |
| clap / clap_complete / tokio / tokio-util | `4`（`derive`,`env`）/ 4.5.53；`1`（workspace 声明 `full`）/ 1.52.3；`0.7`（`compat`） | 子命令与参数解析（doctor、sessions、mcp、completions 等）与 shell 补全；异步运行时；futures ⇄ tokio 互操作以接 crossterm 事件流 | 否 | `Cargo.toml:143-144,280,284`、`pager/Cargo.toml:64-65,67-77` |
| obfstr | `0.4` / 0.4.4 | 编译期字符串混淆（token、auth 机制名、密钥环境变量名） | 否 | `Cargo.toml:212`、`pager/Cargo.toml:82-85` |
| image / git2 / shellexpand | `0.25.10`（+ `png/jpeg/tiff/gif/webp`）；`0.20`（+ `vendored-libgit2`）；`3.1` / 3.1.2 | 剪贴板图片解码/缩放校验；启动时一次性采集 git 信息（`spawn_blocking`，渲染路径不碰 git）；`~` 展开（pager/render 各自直接写版本） | 否 | `Cargo.toml:188,177`、`pager/Cargo.toml:146,154-158,152` |
| resvg / tiny-skia / fontdb / which / wait-timeout | `0.47` / `0.12` / `0.23` / `8` / `0.2` | Mermaid SVG→PNG 光栅化、字体、外部渲染器查找与超时 | 否 | `Cargo.toml:241,279,170,311,307`、`xai-grok-mermaid/Cargo.toml:12-21` |
| nix（`signal`）/ signal-hook / libc / core-foundation / windows-sys | 按 target 分节：unix / macOS / windows（`0.59`） | 信号处理与 `SIGWINCH`（`grok wrap`）、`theme/osc11.rs` 的非阻塞终端读；IOKit 电源断言（`SleepInhibitor`）；`SetConsoleCtrlHandler`（`/minimal`、`/fullscreen` 重启动）与 render 侧 `EnumDisplaySettingsW`、`ShellExecuteW` | 否 | `pager/Cargo.toml:170-187`、`pager-render/Cargo.toml:99-110` |

TUI 栈内部的「代差」现象（同一构建里并存两代 API）：`ratatui 0.29.0` + `ratatui-core 0.1.0`、`compact_str 0.8.1`/`0.9.0`、`itertools 0.13.0`/`0.14.0`、`lru 0.12.5`/`0.16.3`、`unicode-truncate 1.1.0`/`2.0.1`；另外 `strip-ansi-escapes 0.2.1` 仍依赖老的 `vte 0.14.1`，而 workspace 自己用 `vte 0.15.0`。

**pager 的内部依赖分层（39 条内部 crate）** 按层次分组（`pager/Cargo.toml:8-172` 共 87 条 = 内部 39 + 第三方 48，另有 unix 3、macOS 1、windows 1、dev 16）：**表现层原语**（`xai-grok-pager-render:18`、`xai-ratatui-textarea:91`、`xai-ratatui-inline:92`、`xai-grok-markdown:37`、`xai-grok-gboom:21`、`xai-grok-pager-diff:24`）；**agent / 协议 / 工具**（`xai-grok-agent:134`、`xai-grok-shell:116`、`xai-acp-lib:99`、`agent-client-protocol:98`、`xai-tool-types:107`、`xai-grok-tools:133`、`xai-prompt-queue:17-18`）；**配置 / 数据 / 会话**（`xai-grok-config:104`、`xai-grok-status-line:120`（注释说明做成叶子 crate 是为了让客户端从上游读取契约）、`xai-grok-active-sessions:103`、`xai-token-estimation:124`、`xai-dirs:108` 等）；**运维 / 平台适配**（`xai-crash-handler:165`、`xai-grok-telemetry:125`、`xai-grok-otel:126`、`xai-grok-update:95`、`xai-sandbox:140`、`xai-tty-utils:11`（TTY 安全：把子进程从控制终端上摘下来）等）。依赖方向含一个组合根：

```text
xai-grok-pager-bin  (组合根 binary；pager-bin/Cargo.toml:9-13,20,22,59-60,70-77)
├── xai-grok-pager                       ← 应用逻辑与视图模型
│   ├── xai-grok-pager-render            ← 表现层原语 (pager:18)
│   │   ├── xai-ratatui-inline / xai-ratatui-textarea / xai-grok-markdown (render:30,64-65)
│   ├── xai-grok-pager-diff (pager:24) ; xai-grok-status-line (pager:120)
│   └── xai-grok-mermaid (pager:41) ──→ third_party/mermaid-to-svg ──→ dagre_rust ──→ graphlib_rust ──→ ordered_hashmap
└── xai-grok-pager-minimal               ← 可选滚动区渲染模式；反向依赖 pager，pager 不可回指它 (pager-minimal:16-22)
```

**feature 门控一览。** `xai-grok-pager`：`default = ["jemalloc", "sandbox-enforce"]`（`:241`）、`default-bazel`（jemalloc + sandbox-enforce + test-support + local-workspace，`:242-247`）、`jemalloc`（**空 feature**，真正的 `#[global_allocator]` 与 `tikv-jemallocator` 在 `pager-bin`，这里只为让统一默认集解得出来，`:248-254`）、`sandbox-enforce`（转发 `xai-grok-sandbox/enforce`）、`local-workspace`（转发 `xai-grok-shell/local-workspace`，源码中 217 处 `cfg`，最大的一支）、`release-dist`（发布版专用分支，`src/app/mod.rs` 5 处）、`test-support`（暴露测试用 view-model 构造器给兄弟 crate，「生产构建永不启用」）。`xai-grok-pager-bin`：`jemalloc`（`dep:tikv-jemallocator` + `-sys` + `-ctl`（`stats`））、`test-seams`（测试签名密钥的注入缝，明确「永不加入发布 feature 集」且额外要求 `debug_assertions`）、`release-dist`（转发 pager 的同名 feature）。`xai-grok-pager-render`：`test-support`（暴露 mock setter/测试守卫）、`notify`（由 `optional = true` 隐式产生，**无任何使用点**）。`xai-ratatui-inline`：`scrolling-regions`（转发 `ratatui/scrolling-regions`，注释「required for terminal fork only」）、`test-support`（暴露 `completed_buffer`）。`xai-ratatui-textarea`：`debug-logs = ["dep:tracing"]`（让 tracing 成为可选依赖，仅调试日志）。`xai-grok-markdown`：`playground = ["dep:crossterm", "dep:xai-ratatui-textarea"]`（打开两个 playground 二进制 `md-table-test` / `md-mermaid-test`，`required-features`）。`xai-grok-status-line`：`test-support`（暴露 `[ui.status_line]` 逐字段构造的 fixture）。`xai-grok-mermaid` 与 `xai-grok-markdown-core` **无 `[features]`**：Mermaid 恒编译、仅在运行时由 `appearance.render_mermaid` 控制；markdown-core 是 2 个依赖的纯解析 crate。`default-bazel` 是跨 crate 复现的模式，注释解释了动机：CI 只构建**单一 rlib 变体**，所以把 test-only 辅助放进 CI 默认集，而 cargo 的 `default` 保持干净（`render:121-126`、`inline:38-40`、`status-line:19-22`）。

**被声明为可选、但源码零引用的依赖（重要）**：pager 的 5 个 `optional = true` 依赖没有任何显式 feature 引用它们，只能作为隐式 feature 存在——`tokio-stream`（`:76`）、`tempfile`（`:78`）、`xai-chat-state`（`:100`）、`xai-grok-sampling-types`（`:106`）、`gethostname`（`:128`）；对整个 `pager/` 目录（含 `src/`、`tests/`、`benches/`、`build.rs`）搜索这些名字，唯一命中就是清单本身，pager 也没有 `build.rs`，因此这 5 项在默认构建里**不参与编译**、也**没有任何代码路径会用到**——要么是为下游 opt-in 预留，要么是历史残留（`tempfile` 例外：`[dev-dependencies]` 另有一条，测试里用的是它）。`pager-render` 的 `notify` 同样无可达使用点（`appearance/watcher.rs:15,33-49` 只有 `start_static()`，注释「prod mode, no hot-reload」）。**默认无法关掉的重依赖**：Mermaid 栈（`xai-grok-mermaid` + vendored `mermaid-to-svg` + `dagre_rust` + `resvg`/`tiny-skia`/`fontdb`）在 pager 侧无 feature 门；`syntect` 走 `default-onig` 会编译 C 依赖 `onig_sys`；`git2` 用 `vendored-libgit2`。另需注意 **`workspace = true` 不会收窄 feature**：workspace 把 tokio 声明为 `features = ["full"]`，而 pager 只列了 6 个 feature——由于 feature 并集语义，pager 的清单是**叠加**而非裁剪，想真正瘦身必须改 workspace 声明。

**vendored 源码与移植。** `third_party/` 只放 in-tree vendored 的上游源码、不放一方代码，理由是完整审计面、锁定精确源、免疫 crates.io yank（`third_party/README.md:1-14`）；workspace 里只有 4 个 vendored member（`Cargo.toml:106-109`）：`mermaid-to-svg`（MIT，warpdotdev/mermaid-to-svg rev `40cecf2be376e47e15053eadbfb782a531777420`）→ `dagre_rust 0.0.5` / `graphlib_rust 0.0.2` / `ordered_hashmap 0.0.3`（均 Apache-2.0）。值得借鉴的是 **vendoring 纪律**：每个 vendored crate 的 `Cargo.toml` 头部就是「本地改动 + 重审计清单」的唯一真相源，升级时逐条重放；`mermaid-to-svg` 记了 **13 条**本地改动，其中一条与安全直接相关——**hermetic 补丁**：`mermaid_port::is_enabled()` 恒返回 `false`，因为上游读环境变量决定是否走实验性布局端口，而它渲染的是不可信模型输出，读环境即不确定（`mermaid-to-svg/Cargo.toml:34-42`）；`dagre_rust` 的改动之一是把上游 `static mut UNIQUE_STARTER`（无同步的 `unsafe` 自增）换成 `AtomicUsize::fetch_add`，理由是并行 `cargo test` 下的数据竞争（`dagre_rust/Cargo.toml:19-33`）。此外 `xai-grok-tools` 移植了 codex（Apache-2.0，4 个工具：apply_patch / grep_files / list_dir / read_file）与 opencode（MIT，8 个工具：bash / edit / glob / grep / read / skill / todowrite / write）的实现，用 `ToolNamespace::Codex` 做命名空间互斥（同名工具「namespace-exclusive，消费者二选一，绝不并存」），并以 crate 级 `THIRD_PARTY_NOTICES.md` 履行 Apache-2.0 §4(b) 的变更通知（`xai-grok-tools/THIRD_PARTY_NOTICES.md:1-13,16-28,33-41`）。

**依赖规模量级。**

| 指标 | 值 | 说明 |
| --- | --- | --- |
| lock 格式 / package 总数 | `version = 4` / **1347** | registry 1242 + in-tree path 101 + git 4（`Cargo.lock:3`；git 4 = `async-openai` 系 patch + `nucleo` 系） |
| 同名多版本 | **110** 个 crate 名存在 ≥2 个版本 | 前几名 `windows-sys` 6、`nix` 5、`phf` 系 4、`itertools` 4、`hashbrown` 4、`strum` 3、`rand` 3、`syn` 3 |
| 许可证分布（NOTICES 口径） | MIT 996、Apache-2.0 97、BSD-3-Clause 19、Unicode-3.0 18、ISC 15、MPL-2.0 9 | `THIRD-PARTY-NOTICES`，共 1172 个 package 条目 / 18925 行 |
| 各层直接依赖 / 传递面 | `pager` 87（39 内部 / 48 第三方）、`pager-render` 47（12 / 35）、`xai-ratatui-textarea` 9（1 可选）、`xai-ratatui-inline` 4（+6 dev）、`xai-grok-markdown` 13（+2 可选 +2 build）、`xai-grok-status-line` 2、`xai-grok-markdown-core` 2；传递面 `termwiz` 34、`alacritty_terminal` 17、`ratatui` 13、`syntect` 13、`portable-pty` 13（`similar` 是 **0 依赖**，`textwrap` 3） | 见各 `Cargo.toml` / `Cargo.lock` |
| 重量级原生依赖 | `tikv-jemalloc-sys 0.7.1+5.3.1`、`aws-lc-sys 0.39.1`、`zstd-sys 2.0.16`、`libgit2-sys 0.18.2+1.9.1`、`onig_sys 69.9.3`、`openssl-sys 0.9.117`、`tree-sitter 0.25.10`、`resvg`/`usvg 0.47.0`/`tiny-skia 0.12.0`/`fontdb 0.23.0`、`tonic 0.14.3`/`prost 0.14.3` | 集中在「安全 + 高亮 + 图形」三处；若要轻量 TUI，Mermaid 那一组最应先砍 |

**判定：哪些是事实标准、哪些是 Grok 特有。** *事实标准（Rust TUI 的通用底座，可放心直接采用）*：框架与后端 `ratatui` 0.29 + `crossterm` 0.28（`xai-ratatui-inline/Cargo.toml:9-15` 的注释直接说 `anstyle-parse` 「已通过 anstream/clap 挂在已发布依赖树里」）；宽度与折行三件套 `unicode-width` + `unicode-segmentation` + `textwrap`；颜色与能力探测 `anstyle` / `anstyle-parse` / `supports-color`；纯文本处理 `strip-ansi-escapes` / `linkify` / `vte` / `pulldown-cmark`；diff 与有序容器 `similar` / `indexmap`；模糊搜索 `nucleo`（Helix 出品）；运行时与 CLI `tokio` / `clap` + `clap_complete`；PTY `portable-pty`；ANSI → 文本 `ansi-to-tui` / `tui-scrollbar`。唯一要警觉的是 `ratatui` 0.29 与 `ratatui-core` 0.1 并存——那是生态过渡期的真实状态，不是本项目该模仿的设计。*Grok 特有（不要照抄，但取舍值得读）*：`xai-ratatui-inline` 是「原生滚动区（不占 alt screen）」这条产品路线的核心自研资产，若走 alt-screen 全屏 TUI，用 ratatui 自带 `Terminal` 即可；`xai-ratatui-textarea` 是通用需求，先用上游 `tui-textarea` 之类评估，其 `dep:tracing` feature 写法可借鉴；`xai-grok-mermaid` + vendored dagre 栈只在「要在 TUI 里显示图」时才需要，代价是 4 个 vendored crate + resvg/tiny-skia/fontdb；`obfstr` / `cryptify` 属**分发硬化**，闭源/分发型 CLI 才划算；`alacritty_terminal` + `ptyctl` 是**测试方法论**上最值得学的一点（用独立 crate + PTY 把「需要真终端」的测试从单测里隔离出去）；`xai-grok-status-line` 示范了把「用户写的配置」与「agent 发来的载荷」抽成 2 依赖的叶子 crate，让契约与实现分离——这对 LLM + Jev 双模型契约同样适用；`xai-grok-pager-bin` 作为组合根，用 cargo 循环限制反向逼出「依赖单向」的架构，值得抄；`xai-grok-workspace` / `-client` / `-daemon` / `-types` 这类工作区 daemon 拆分早期不需要。**可直接照搬的组织法是 feature 双默认集**：把「CI 需要的 test-only 能力」与「cargo 生产 `default`」分开；即使不用 Bazel，也可保留同样的约定，只是把它命名为 CI 专用集即可。

**未找到证据 / 存疑点（引用时不要采信）**：① `third_party/NOTICE` 与目录不一致——`NOTICE:52-68` 与 `README.md:24-25` 列了 `nfsserve 0.11.0`（BSD-3-Clause）与 `fuser 0.18.0`（MIT），但快照内**未找到这两个目录**，它们也不在 workspace members，合稿时不要把这两个 crate 算进 TUI 依赖面。② `clippy.toml` 自我描述与位置矛盾（见上）。③ 5 个无使用点的 pager 可选依赖：**无法判断**是有意为下游预留还是残留。④ `xai-grok-pager-render` 的 `notify` 无可达使用点。⑤ `rustls` 的实际归属未确认——pager 注释说它已移到 `pager-bin`，但并未出现在 `pager-bin` 的 `[dependencies]` 段，推测经 `xai-grok-http`/`xai-grok-extra-ca` 间接进入，本次**未追踪**。⑥ **体积与编译时长未测量**：未运行任何 `cargo` 命令，feature 的瘦身收益、`release-dist` 的代价（注释称本地构建慢约 2.2 倍）均未验证。⑦ 未展开：`xai-grok-shell`、`xai-grok-workspace`、`xai-grok-ocr-sampler` 等非 TUI 侧一百多个成员 crate 的依赖细节。

### 6.4 工程化：测试、快照、基准与代码组织

#### 本节要点

- Grok Build 的工程化特征不是「测试数量多」，而是**每一条跨模块约定都配了一个会失败的测试**：文档与注册表对账、架构禁区源码扫描、终端宿主环境自动清洗、帧时数值门禁（`tests/registered_features_are_documented.rs:1-22`、`xai-grok-shell/src/config_docs/mod.rs:1-7`）。
- 文档对账的实现方式是 `include_str!` 读入 Markdown、再按**反引号**切表格单元做集合运算——反引号是机器可读标记，不是排版装饰（`src/app/status_line_tests.rs:510-515`）。
- 快照测试被刻意限制在极小范围：全仓只有 11 个 insta `.snap`，全部是被测函数返回的**纯文本**，不存在像素级或全屏 golden 快照。
- 渲染层测试的默认形式是 `ratatui::buffer::Buffer` 就地断言单元格文本，而非落盘比对；单元测试与源码同目录并置。
- PTY 端到端由独立 crate 承载，且该 crate **刻意不被 pager 依赖**；`#[ignore]` 是所有进程级用例的统一闸门（全仓 390 处），由 Bazel 显式 opt-in 并限流。
- 基准分两套：criterion 微基准与 spawn 真实二进制的 `pty-bench`，后者按平台维护 JSON 基线、p99 回归 >15% 即失败。

**量化画像。**

| 指标 | 数值 | 统计方式 |
| --- | --- | --- |
| `xai-grok-pager` 下 `.rs` 文件 / 含 `#[cfg(test)]` 的源文件 | 652 / 340 | `find … -name '*.rs' \| wc -l`、`rg -c '^#\[cfg\(test\)\]'` 文件数 |
| 测试属性数（pager / 全仓库） | 10,509 / 34,077 | `rg -c '#\[test\]\|#\[tokio::test\]\|#\[test_with'` 求和 |
| 全仓库 `#[ignore` / `serial_test` 出现次数 | 390 / ~1,002 | 同上；串行化用例靠它避让共享资源 |
| `tests.rs` / `*_tests.rs`；有 `tests/` 目录的 crate | 11 + 74 = 85；32 | `find` 两个模式分别计数 |

```text
L5 脚本化场景 + 视觉产物  48 个 YAML（tests/scenarios/*.yaml）        #[ignore]，Bazel opt-in
L4 PTY 端到端 + 性能基线  12 个独立 [[test]] 家族（pty_e2e_*）        #[ignore]，Bazel opt-in
L3 子进程集成             pager/tests/ 的 6 个普通集成测试（默认跑）  + mermaid 子进程用例（#[ignore]）
L2 渲染快照 / 缓冲断言    Buffer::empty 就地断言 + 11 个 insta .snap  默认跑
L1 同目录单元测试         85 个 tests.rs / *_tests.rs + 内联 mod tests 默认跑
```

关键隔离手段是 `#[ignore]`：进程级用例一律默认跳过，由 Bazel 显式 opt-in。`tests/pty_e2e_smoke.rs:3` 写明「All cases are ignored for ordinary Cargo runs; Bazel opts in and caps this process-heavy family at four concurrent libtest workers」。

**单元测试：与源码同目录**，两种并置风格并存——风格 A 是「拆分文件 + 显式声明」（`src/views/settings_modal/mod.rs:22-27` 的 `#[cfg(test)] mod tests;`，同目录 `tests.rs` 用 `use super::input::*;` 直接访问三个兄弟模块）；风格 B 是「`*_tests.rs` 文件名 + `#[path]`」（`src/scrollback/render.rs:977-979` 的 `#[path = "render_tests.rs"]`，这种声明在 pager 中出现 69 次，对应 74 个 `*_tests.rs` 文件）。测试函数名是完整英文句子，说明**期望的行为**而非被测函数名：`every_registered_setting_is_exercised`（`tests/settings_e2e.rs:79`）、`every_field_on_the_wire_is_named_in_the_guide`（`src/app/status_line_tests.rs:356`）、`go_deeper_titles_resolve_to_real_guides`（`src/tutorial_docs.rs:108`）、`minimal_never_uses_ris_rerender_or_emit_to_scrollback`（`xai-grok-pager-minimal/src/guard.rs:9`）。断言依赖 `pretty_assertions`（根 `Cargo.toml:225` 定为 `1`），在 `src/scrollback/render_tests.rs:8` 覆盖内置宏。

**快照 / golden 测试：范围刻意收窄。** 用 **insta**（根 `Cargo.toml:192`、`pager/Cargo.toml:200`），全仓库只有 11 个 `.snap`、0 个 `.snap.new`，集中在两处：`src/scrollback/blocks/tool/snapshots/`（9 个，来自 `blocks/tool/edit.rs:2412,2448,2479,2517,2578,2599,2635,2669,2707`）与 `src/app/snapshots/`（2 个，来自 `app/status_blocks.rs:323,351`）。**快照是纯文本，不是屏幕像素**——断言对象是被测函数返回的 `String`（`insta::assert_snapshot!("diff_basic", diff_outputs_to_string(&outputs))`）。文件名是 insta 默认规则 `<crate>__<模块路径>__<测试模块>__<快照名>.snap`，模块路径本身编码进文件名，因此重命名模块会直接产生孤儿快照；覆盖内容正是「行号栏宽度、双栏对齐、换行重排、hunk 间省略标记」这类用断言写起来很啰嗦的等宽文本输出（如 `diff_three_digit_lines.snap` 验证三位行号的列宽切换、`diff_merged_hunks_gap_markers.snap` 验证 `… 7 unchanged lines` 这类间隙标记）。**未找到证据**：仓库中没有 ratatui 全屏快照（无 `TestBackend` 输出落盘为 golden 文件的机制），也没有 `insta` 的 `glob!` 或自定义 `Settings` 配置。

**渲染层测试：缓冲区就地断言。** 惯用法是构造 `ratatui::buffer::Buffer::empty(Rect)`、把 widget 画进去、再断言单元格文本；命中次数前列：`views/dashboard/render_tests.rs` 117、`views/settings_modal/tests.rs` 94、`views/dock/mod.rs` 38、`views/dashboard/chrome_tests.rs` 27、`scrollback/render_tests.rs` 26、`xai-ratatui-textarea/src/textarea_tests.rs` 24。`xai-grok-pager-render` 自身有 7 个测试文件、1,216 个 `#[test]`。配合的辅助风格是在测试文件顶部手写小工厂（如 `render_tests.rs:22-30` 的 `make_entries(count)` / `make_markdown_entry(text)`），以及一对 `at` / `at_mut` 取下标辅助——后者是因为生产 crate 用 `#![deny(clippy::indexing_slicing)]` 禁止索引切片。

**PTY 端到端：用真正的终端模拟器看屏幕。** `xai-grok-pager-pty-harness/src/lib.rs:10-20` 自述四层，同一套 API 服务三类消费者（回归场景、基准、本地复现）：`L1 pty`（PTY 管理：spawn / 注入按键 / resize / drain）、`L2a screen`（经 `alacritty_terminal` 的虚拟终端状态还原「用户看到什么」）、`L2b timing`（经 `CSI ? 2026 h/l` 标记测每帧耗时）、`L3 content`（mock 推理服务），外加 `scenarios`（命名化、参数化的工作负载，返回 `BenchResults`）、`results`（汇总统计与基线比较）、`scroll_matrix`（`GROK_SCROLL_LOG` JSONL 不变量套件）。`ScreenTracker`（`src/screen.rs:9-14`）持有一个 ptyctl 的 `Terminal`，提供 `contents()`（可见屏纯文本）、`contains(text)`、`cursor_position()`（0 基，ptyctl 是 1 基）、`scrollback_text()` / `full_text()`（注释明确「对已提交输出的断言应使用 `full_text()`」，因为 minimal 模式下内容可能落在任一区）、`styled()` / `html()`（视觉产物）。断言原语全部是轮询式 `wait_*`（`wait_for_text:447`、`wait_for_text_absent:454`、`wait_for_turn_idle:465`、`wait_for_full_text:542`、`wait_for_full_text_absent:552`、`wait_for_kitty_graphics:569`、`quit:615`、`wait_for_exit_and_drain:627`）。一个完整用例只有十行：

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn welcome_screen() {
    let content = ContentController::start().await.expect("start content");
    let binary = pager_binary().expect("resolve pager binary");
    let mut harness = PtyHarness::spawn_with_content(&binary, DEFAULT_ROWS, DEFAULT_COLS, &content, &[])
        .expect("spawn pager");
    harness.wait_for_text(WELCOME_SCREEN_SENTINEL, WELCOME_TIMEOUT).expect("welcome text");
    harness.quit().expect("clean quit");     // tests/pty_e2e/welcome_screen.rs:7-22
}
```

共享常量在 `tests/pty_e2e/common.rs:16-40`：`DEFAULT_ROWS = 50`、`DEFAULT_COLS = 120`、`WELCOME_TIMEOUT = 20s`、`WELCOME_SCREEN_SENTINEL = "Quit"`、`MOCK_RESPONSE_SENTINEL = "MOCKRESPONSE"`（注释解释为何 50×120：「大到 welcome 屏不折行，小到 `screen_contents()` 扫描够便宜」）；测试模块经 `#[path]` 组装（`tests/pty_e2e_smoke.rs:6-81` 列了 40+ 个 `#[path = "pty_e2e/….rs"] mod …;`），共享的 `common.rs` 有 1,667 行。两个最容易踩的坑各有一处机制对应：**终端应答必须回写**——`drain_responses()`（`screen.rs:32-38`）把模拟器产生的应答（光标位置报告、设备属性查询）写回子进程，注释写着「**Must be written back or terminal probes hang.** A missed startup cursor-position query downgrades `--minimal` to full-screen inline」；**宿主环境必须清洗**——`pty_spawn.rs:19-70` 定义三张表：`CLIPBOARD_SINK_ENV_VARS`（`GROK_OSC52_SINK`、`LC_GROK_OSC52_SINK`）、`APPEARANCE_ENV_VARS`（`GROK_APPEARANCE`、`GROK_THEME`、`COLORFGBG`…，注释「would make `theme=auto` non-deterministic in PTY tests」）、`HOST_TERMINAL_ENV_VARS`（终端品牌链、复用器标记 `TMUX`/`ZELLIJ`/`STY`/`BYOBU_*`/`CMUX_*`、嵌入编辑器标记 `NVIM`/`INSIDE_EMACS`，注释「**A leak reclassifies the child**」）；改环境走类型化的 `EnvOp`（Set/Remove）而非裸字符串覆盖。**依赖边界**是这条策略的关键：`pty-harness/Cargo.toml:10-13` 的注释写明它「Not imported into the production pager binary. Spawn-only pager tests live here so `cargo test -p xai-grok-pager` / `cargo test -p xai-grok-pager-bin` never compile this crate, and the spawn tests never link the pager library.」该 crate 有 42,234 行、48 个 YAML 场景、4 个 `[[bin]]` 与 2 个 `[[bench]]`；每个测试家族单独声明 `[[test]]`（`Cargo.toml:69-117` 共 12 个），注释「Each root is its own Cargo/Bazel process so CI can schedule them independently」。

**子进程集成测试：进程边界留在被它测试的 crate 里。** `pager/tests/mermaid_render_subprocess.rs` 是「测试要 spawn 真实二进制，但断言的是库代码」的范式：它用生产代码里的同一函数 `render_via_subprocess`，把真实构建出的 pager 二进制当作隐藏子命令 `__mermaid-render` 启动（常量在 `src/app/mermaid_worker.rs:56`），定位二进制时同时认 `PAGER_BINARY` 与 `CARGO_BIN_EXE_xai-grok-pager` 并 `std::path::absolute` 化（Bazel 给的是 runfiles 相对路径）。四个用例覆盖真实进程行为：正常渲染返回 `Ok` 且输出文件能被 `image::load_from_memory` 解码、宽高 > 0；超 64 KiB 上限的源被「容纳」（父进程返回 `Err` 且**不留 PNG**）；非法图同样被容纳；1 ms 预算下父进程必须按时返回（< 10 s）并杀掉子进程、无 PNG。文件头解释它为何留在 pager 而非 harness：「Unlike the spawn-only suites in `xai-grok-pager-pty-harness`, this test stays here because it exercises pager library code.」全部 `#[ignore]`，用 `PAGER_BINARY=target/debug/xai-grok-pager cargo test … -- --ignored` 启用。

**声明式场景与「飞行记录仪」。** `tests/scenarios/` 有 48 个 YAML，由 `ScriptedScenarioRunner` 回放（`src/scripted.rs`，2,300 行）；`welcome.yaml` 全文即一个可读规格（`wait_for_text` / `assert_contains` / `assert_not_contains` / `screenshot` / `resize` / `assert_running`），驱动它的 Rust 壳极薄且强制三条元约定：`report.status == Passed`、`report.bugs.is_empty()`、报告中**必须存在至少一个带产物的 `screenshot` 步骤**（`tests/scripted_scenarios.rs:43-50`）；尺寸断言支持精确值与区间两种（`DimensionAssertion::Exact` / `Range { min, max }`），产物落 `text` / `html` / `svg` / `styled_json` 四种格式。更硬核的是 `src/scroll_matrix/`：让 pager 打开 `GROK_SCROLL_LOG` 写 JSONL「飞行记录仪」，测试端按手势重放鼠标 SGR 报告，再用不变量判定日志。两个设计选择值得引用——**不依赖 pager 类型**：「This module deliberately re-declares the wire schema instead of importing pager types. `log::ScrollLogLine` keeps every always-emitted producer field **required**, so a pager-side rename or removal fails deserialization here.」（用反序列化失败当 schema 漂移警报，生产者侧有一对偶测试 `pager/src/input/mouse/tests.rs`）；**抗负载**：「every timing predicate reads the RECORDER's clock (`ts_ms`, `ms_since_prev_flush`, `avg_interval_ms`), never the test process's… So no invariant here can false-fail on a loaded machine.」不变量用 `I-` 编号命名并带文档注释：`I-ORD`（时间戳非递减）、`I-CAP`（`|flushed| ≤ cap`）、`I-DROP-EQ`（finalize 的 `dropped == backlog_after`）、`I-CADENCE`、`I-CONS-W`（强制滚轮总量精确）等。

**基准测试：两套并行体系。** A. **criterion 微基准**（criterion 0.6，根 `Cargo.toml:148`；pager 的四个 target 全部 `harness = false`，`pager/Cargo.toml:211-225`），其文件头本身就是设计文档（`resize.rs:1-6` 列出它守护的三条已修复回归，`search.rs:1-7` 说明 `scan` 为什么从输入线程搬到后台守护进程）。B. **PTY 帧时基准 + 数值门禁**：`pty_bench` spawn 真实 release pager 并产出 JSON：

```rust
pub struct BenchResults {              // pty-harness/src/results.rs:13-24
    pub scenario: String, pub total_frames: u64, pub avg_fps: f64,
    pub p50_ms: f64, pub p99_ms: f64, pub max_ms: f64,
    pub jank_count: u64, pub jank_rate: f64, pub chars_per_frame_avg: f64,
}   // pub const DEFAULT_REGRESSION_THRESHOLD: f64 = 0.15;   results.rs:86
```

jank 的定义是「帧时 > 2 × p50」（`results.rs:61-63`）；基线按平台分文件（`linux-x86_64`、`linux-aarch64`、`macos-aarch64`），CI 在 p99 增长超过 15% 时失败，有意改变性能的 PR 必须同 PR 更新基线并把干净的 `pty-bench` 输出贴进 PR 正文。帧时来自 `src/timing.rs:1-4`：用 VTE 解析 `CSI ? 2026 h/l` 同步更新标记（即 crossterm 的 `BeginSynchronizedUpdate` / `EndSynchronizedUpdate`），把「一帧」的墙钟时长量出来。两处注意：快照中 `benches/pty_baselines/` **只有 `README.md`**，平台基线 JSON 由首次 CI 运行播种；`benches/pty_baselines/README.md` 的示例命令写作 `cargo run -p xai-grok-pager --release --bin pty-bench`，但 `[[bench]] name = "pty_bench"` 声明在 `xai-grok-pager-pty-harness/Cargo.toml:143-146`，且 `xai-grok-pager/Cargo.toml` 里没有任何 `[[bin]]`（**内部不一致**）。**未找到证据**：`rg -l proptest --glob '*.rs' crates` 无命中，根 `Cargo.toml` 也没有 `proptest` / `quickcheck` / `arbitrary`——仓库里的 property 风格检查以「对一批数据跑一组不变量的断言函数」形式存在（即上面的 `scroll_matrix/invariants.rs`），而非专门的 property-testing 库。

**元测试：文档即契约。** 这是 Grok Build 最有辨识度的工程化手法——用 `include_str!` 把 Markdown 读进测试，解析表格里的反引号单元，与代码注册表做双向对账：

| 校验对象 | 机制 | 位置 |
| --- | --- | --- |
| feature 键 / env 变量 ↔ 运维文档 | `include_str!` + 子串包含 | `tests/registered_features_are_documented.rs:10-22` |
| 配置参考页 ↔ 解析器（双向） | 解析表格；「CI fails when a registered key has no row, or an MCP or `features.*` row **names an unknown key**」 | `xai-grok-shell/src/config_docs/mod.rs:1-7,253+` |
| status line wire 字段 ↔ 指南 | 解析表格 + JSON fixture 遍历（只校验叶子值路径，容器由其成员代言） | `src/app/status_line_tests.rs:356-421` |
| settings 注册表 ↔ 测试矩阵（双向） | 集合差；`ALL_SETTINGS_EXERCISED` 49 项，每个新增 `SettingMeta` 必须同时补键盘与鼠标两条路径的测试 | `tests/settings_e2e.rs:23-76,79,95` |
| 指南结构 / 教程页 / 文档抽取 | 首字符必须是 `#`、标题唯一、无重名文件、每页 ≤ 50 行、`go_deeper` 必须能解析到真实标题、抽取到 `GROK_HOME` 时清理陈旧文件但保留用户自建的 `notes.md` | `src/docs.rs:279-370`、`src/tutorial_docs.rs:93-140` |

最小但最具代表性的样例是 23 行的 `tests/registered_features_are_documented.rs`，头部注释就是它的存在理由：「`FEATURES` is the source of truth and the operator tables are hand-maintained mirrors with no compile-time check of their own. This test is that check.」其断言形式是 `assert!(ENTERPRISE.contains(&format!("`{}`", spec.key)), …)`。反引号的契约性体现在 `table_cells` 的实现上：

```rust
fn table_cells(section: &str) -> impl Iterator<Item = &str> {
    section.lines().filter(|line| line.starts_with('|'))
        .flat_map(|row| row.split('`').skip(1).step_by(2))     // status_line_tests.rs:510-515
}
```

即**表格单元必须用反引号包裹**；`section` 刻意按 `##` 切段，注释解释「Scoped, because a name in a neighbouring section would otherwise vouch for a row that was deleted」。`the_numbers_the_guide_states_are_pinned`（`:418`）进一步把文档写死的数字钉在代码常量上（如 `REFRESH_FAILURES_TO_PAINT == 3`），注释：「so none of them can move without editing 25-status-line.md in the same change」。架构禁区同样写成可运行的源码扫描：`xai-grok-pager-minimal/src/guard.rs:1-40` 用 `include_str!` 读入本 crate 的每个模块源文件，断言不含三个禁用标识符，失败消息解释后果「it would double-print committed scrollback (design K6 / risk #2)」；文件顶部注释还提醒「Keep in sync with `lib.rs`'s module list: a module missing here is a hole in the guard」——即这个 guard 有一个**已知的、需人工维护的缺口**。**未找到证据**：仓库中没有外部链接有效性检查脚本，也没有 CI 工作流文件（快照里无 `.github/`）；此外 `docs/internal/` 目录在本快照中不存在（`ls docs/internal` → No such file or directory），因此上面那个 23 行的测试在当前快照下**无法编译**——这是上游专题明确记录的缺口。

**结构与依赖分层：四个 crate 的边界。**

```text
xai-grok-pager-bin  (composition root, [[bin]] name = "xai-grok-pager")
      │  main.rs 3,774 行 + build.rs
      ├──────────────► xai-grok-pager      (库：全部应用逻辑，652 个文件)
      │                     │
      │                     └──► xai-grok-pager-render  (表现层原语，被 re-export)
      └──────────────► xai-grok-pager-minimal  (可选渲染模式)
                            │
                            └──► xai-grok-pager  ← 反向引用，构成循环
                                 ⇒ 二者不能互相依赖，改用函数指针 seam
```

`xai-grok-pager`（`src/lib.rs:1-73`）crate 根有 `#![deny(clippy::indexing_slicing)]`（`:11`），且开头有 `#![allow(unused_imports, unused_variables, unused_mut, unreachable_code, dead_code)]`（`:1-7`）把死代码告警在库级别整体关掉；它公开 60 个左右模块，并把 pager-render 的模块整体转出（`:61-65`）；`#[path = "minimal/api.rs"] pub mod minimal_api;` 与 `#[path = "minimal/hook.rs"] pub mod minimal_hook;`（`:35-38`）说明 minimal 模式的**契约住在 pager、实现住在另一个 crate**；测试专用模块也在 crate 根显式声明（`#[cfg(test)] pub mod test_util;`，328 行）。`xai-grok-pager-render`（`src/lib.rs:1-17`）只暴露 17 个模块、第一个语句就是 `#![deny(clippy::indexing_slicing)]`，**未找到 crate 级文档注释**，依赖画像是纯表现层（ratatui / crossterm / textwrap / unicode-bidi / unicode-width / linkify / tui-scrollbar + `xai-grok-markdown` / syntect / anstyle / supports-color + vte + dark-light），它**不依赖** pager，因此可被 pager 单向依赖并转出。`xai-grok-pager-minimal`（`src/lib.rs:1-21`）的模块文档说清了机制：最小模式下定稿的对话块通过 `xai_ratatui_inline::Terminal::insert_before_rows` 打进终端**原生 scrollback**，交互式 `ScrollbackPane` 不参与，「the terminal owns history」；`# Wiring` 段写明反向依赖会成环、pager 暴露 `xai_grok_pager::minimal_hook` 函数指针、由 binary 调用 `install()` 注册，未安装时 pager 的 minimal 分支是惰性的。`xai-grok-pager-bin` 的存在理由写在 `Cargo.toml:9-13`：承接「binary 需要同时链接 pager 库与可选 `xai-grok-pager-minimal`」这件事，并在启动时装好 IoC 钩子；`src/main.rs` 3,774 行 + `agent_command.rs` 100 行 + `build.rs` 49 行，拥有全局分配器（`tikv_jemallocator::Jemalloc`，`main.rs:9-11`，三个 jemalloc 包按用途分工：`-sys` 裸 mallctl 用于内存悬崖的 arena purge、`-ctl` 用于堆剖析/统计）、二进制加固（`obfstr` / `cryptify`）与 `test-seams` 特性（**双重锁**：不在 `default`/`default-bazel`/`release-dist` 中，且安装钩子额外要求 `debug_assertions`，清单里写下「Never add this to a shipped feature set.」）；`build.rs` 只做两件事——版本戳（`:29-38`，`:17-27` 有一段重要注释「Never emit a missing path: cargo treats it as always dirty and rebuilds this crate every build」）与 Windows 延迟加载（`:40-48`）。**`pager-bin` 中没有 protoc 相关代码**，protoc 只出现在仓库根的开发工具链里（`bin/protoc` 是 dotslash 脚本，由 `crates/build/xai-proto-build/src/find_protoc.rs:27-46` 按 `$PROTOC` → 逐级向上找 `bin/protoc` → `$PATH` 三级顺序解析）。

**横切约定与共享测试库。** 十余个 crate 声明 `test-support = []` 空特性 + `default-bazel = ["test-support"]`，含义是测试夹具以 `#[cfg(feature = "test-support")]` 编译进库，普通构建与 Bazel 测试构建各取所需；pager 的 dev-dependencies 里就有一整列带该特性的依赖（`pager/Cargo.toml:191-197`）。共享测试库两个：`crates/common/xai-test-utils`（5 个源文件：hermetic git、`crate_root!()` 宏（Bazel runfiles 或 `CARGO_MANIFEST_DIR`）、tracing 计数、`env_usize`；`bazel` 特性在纯 cargo 下关闭并回落）与 `crates/codegen/xai-grok-test-support`（40+ 个源文件：`MockInferenceServer`、SSE 生成器、ACP stdio 客户端、`TestSandbox`、`TestProcess`、`UdsProxy`、`MockOtelServer`、`MockManagedConfigServer`、`run_headless`），后者有两个跨套件统一约定值得抄：`scaled(base: Duration)`（乘 `GROK_TEST_TIMEOUT_SCALE`，让共享 runner 池上的负载表现为变慢而非失败）与 `run_in_local_set`（stdio 客户端的连接不是 `Send`，任务必须 local spawn）。它的测试与源码同目录并置，与 pager 一致。

**「文档即产品」的工程约束。** 两套文档两种节奏：`docs/user-guide/` 27 个文件 + `README.md`（完整使用手册，README 按 Tier 1/2/3 分档并给出表格导航），`docs/tutorial/` 9 个（`/tutorial` 覆盖层的分页入门，严格 opt-in）。两者都经 `include_str!` 编进二进制（`src/docs.rs:36-45` 的 `guide!` 宏把每个 Markdown 变成静态表的一项；`tutorial_docs.rs:19-28` 是同构的 `topic!` 宏），启动时抽取到 `<grok_home>/docs/user-guide/`（`docs.rs:241-252`）并清理已删除的文件（`:272`）。模块注释强调「Single source of truth: two static arrays (`USER_GUIDE`, `REFERENCE_DOCS`) hold every doc. All lookups are zero-allocation」；`find_doc(title)` 是 `go_deeper` 的解析入口，而 `tutorial_docs.rs:15-16` 要求 `go_deeper` 「must match a [`crate::docs`] title」——这条约束由上面的元测试守住。因此「文档即产品」不是口号而是**运行时可观察的行为**：删掉一篇文档会让元测试失败，写错一个跳转标题会让快捷方式静默失效（`go_deeper_titles_resolve_to_real_guides` 的注释：「a typo'd title would turn the shortcut into a silent no-op」）。

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

---

## 8. 附录

> 本附录的取材范围限定在 `docs/research/grok-tui/`（28 篇专题 + 8 篇分节稿）与 `reference/grok-build/` 只读快照两处，**不引入二者之外的新事实**。8.3 只做汇总与去重，不评判、不补充；8.5 的义务是「让读者能自己重跑一遍」，不做任何构建或运行验证。

### 8.1 专题索引

28 篇专题是逐行取证的原料，合计约 1.17 万行；每篇文末都有 `## 关键结论（供合稿引用）` 一节，「一句话内容」即取该节第 1 条归纳。

| 编号 | 专题标题 | 文件 | 一句话内容 |
| --- | --- | --- | --- |
| 01 | 专题 01：Grok Build 的主题与配色系统 | `01-theme-color-system.md` | 颜色只有一个来源：扁平 `Theme` 结构体的 58 个 `Color` + 6 个 `Modifier` 字段，渲染层没有硬编码色。 |
| 02 | 专题 02：字形、图标与边框符号系统 | `02-glyphs-and-iconography.md` | `glyphs.rs` 是全项目唯一的 chrome 字形源（34 个 `pub fn`），被 pager 与 minimal 两个 crate 共同引用。 |
| 03 | 专题 03：排版、留白与信息密度 | `03-typography-spacing-density.md` | 排版分三层，只有滚动区集中可配置；不存在全局设计令牌模块。 |
| 04 | 专题 04：Markdown 渲染美学 | `04-markdown-rendering.md` | 解析复用 `pulldown-cmark 0.13`、渲染完全自研，`xai-grok-markdown-core` 刻意只依赖解析器以便无头复用。 |
| 05 | 专题 05：代码高亮与 diff 呈现 | `05-code-and-diff-rendering.md` | 高亮用 syntect，语言集在 build 期生成 `syntaxes.bin`，运行期靠 `OnceLock` 反序列化。 |
| 06 | 专题 06：动效、加载态与流式呈现 | `06-motion-loading-streaming.md` | 动效由一个全局 tick 计数器驱动（默认 30 fps），但空闲时事件循环零唤醒。 |
| 07 | 专题 07：Grok Build 的图形化内容：Mermaid、内联图片与视频 | `07-mermaid-images-video.md` | Mermaid 是双渲染器：自包含的纯文本制表符渲染器 + 一条完全独立的 PNG 链路。 |
| 08 | 专题 08：整体布局架构与 pane 模型 | `08-layout-and-panes.md` | 主界面单列垂直堆叠，整屏只调用一次 `Layout::vertical`，横向没有分栏。 |
| 09 | 专题 09：Scrollback 块模型与渲染管线 | `09-scrollback-block-model.md` | 块抽象是 `BlockContent` trait + 13 变体 `RenderBlock` 枚举，枚举靠宏转发全部方法。 |
| 10 | 专题 10：Grok Build 的模态、浮层与焦点栈 | `10-modals-overlays-focus.md` | 模态系统没有栈，真正的弹窗是单槽 `Option<ActiveModal>`，浮层 z 序由渲染分支顺序定义。 |
| 11 | 专题 11：输入区（prompt widget）设计 | `11-prompt-widget.md` | 输入区是「可复用组件 + 独立 textarea crate」两层，组件本身不管焦点与 chrome 归属。 |
| 12 | 专题 12：Grok Build 的状态显示面 | `12-status-surfaces.md` | 共 5 个屏幕状态面，另有 `/usage`、`/queue`、`/tasks` 三个提交进 scrollback 的纯文本块。 |
| 13 | 专题 13：侧边与辅助面板 | `13-auxiliary-panes.md` | 辅助面板都在固定侧向位置纵向插入、顺序固定，scrollback 是唯一的弹性行。 |
| 14 | 专题 14：首屏、空状态与渐进式披露 | `14-first-run-onboarding.md` | 首屏是欢迎页上的六道短路门禁，顺序即优先级；三者全 `Done` 才允许创建会话。 |
| 15 | 专题 15：多会话界面与 dashboard | `15-multi-session-dashboard.md` | Dashboard（本进程活会话）与 session picker（磁盘历史会话）的数据源、宿主与状态互不复用。 |
| 16 | 专题 16：Grok Build 的键盘模型与动作注册表 | `16-keyboard-and-actions.md` | 键位只有一处来源 `actions/defaults.rs:56`，全仓库没有其它生产注册路径。 |
| 17 | 专题 17：鼠标、选择与剪贴板 | `17-mouse-selection-clipboard.md` | 鼠标事件只有一个总入口，非滚轮事件按上一帧缓存矩形命中，只有 Scrollback 区分派拖选。 |
| 18 | 专题 18：命令、补全与搜索交互 | `18-commands-completion-search.md` | slash 命令是一个约 25 个方法的 trait，元数据由 `slash_meta!` 宏只生成显式列出的 override。 |
| 19 | 专题 19：审批、提问与确认类交互 | `19-approval-and-elicitation.md` | 五类裁决交互统一收敛为「阻塞卡」，由 `blocking_card()` 给出固定优先级，permission 可抢断其它卡片。 |
| 20 | 专题 20：Agent 轮次交互：流式、插话、队列与取消 | `20-turn-interaction.md` | 一次轮次切成四段；发送后被设界的是「确认（ack）」而不是 RPC（软提示 10 s、硬放弃 120 s）。 |
| 21 | 专题 21：会话生命周期与终端状态管理 | `21-session-lifecycle.md` | 首屏之前的工作全部串行 await，唯一有名字的等待预算是 `EARLY_PREFETCH_WAIT = 2 s`。 |
| 22 | 专题 22：宿主集成——通知、标题、OSC 与终端复用器 | `22-host-integration.md` | 桌面通知只支持 5 种终端协议、没有任何系统级通知 API，Zellij 被硬编码为 BEL。 |
| 23 | 专题 23：无障碍、自适应与终端能力协商 | `23-accessibility-adaptivity.md` | 终端能力是「探测成枚举、再由枚举驱动降级」的两段式，业务代码只读能力位、不读环境变量。 |
| 24 | 专题 24：自研 widget 与组件抽象 | `24-custom-widgets.md` | `xai-ratatui-inline` 是 ratatui 0.29 `Terminal` 的 fork，动机是上游不暴露视口面积、背缓冲与 resize 内部状态。 |
| 25 | 专题 25：Grok Build 的渲染管线与性能架构 | `25-render-pipeline-performance.md` | 主循环是单个 bias 排序的 `tokio::select!`，每轮循环末尾只绘制一次，空闲时终端收到零字节。 |
| 26 | 专题 26：依赖、版本与 feature 门控清单 | `26-dependency-stack.md` | 工具链基线是 Rust 1.94.0 + edition 2024；TUI 依赖面是 ratatui 0.29 + crossterm 0.28 的经典组合。 |
| 27 | 专题 27：工程化——测试、快照、基准与代码组织 | `27-engineering-practices.md` | 工程化核心手法是元测试：把跨模块约定写成会失败的测试，而不是只写行为测试。 |
| 28 | 专题 28：设计意图与原则的证据采集 | `28-design-intent-evidence.md` | 官方没有「选内联滚动而非全屏备用屏」这条信条：默认是全屏 alt-screen，内联是备用屏不可用或不可靠时的降级目标。 |

这 28 篇是逐行取证的原料；正文（§2–§7）没有覆盖到的细节，可在对应专题里按路径与行号查到。

### 8.2 分节稿索引

8 篇分节稿是把专题稿提炼、去重后按主题重组的产物，章节编号已固定，合稿时按号拼接即可。

| 分节稿 | 覆盖章节 | 上游专题 | provenance 情况 |
| --- | --- | --- | --- |
| `final-A.md` | §3、§3.1–§3.4 | 01、02、03、04 | 开头来源行明列 |
| `final-B.md` | §3.5–§3.7（无 `## 3.` 标题） | 05、06、07 | 无来源行，按小节标题对应 * |
| `final-C.md` | §4、§4.1–§4.4 | 08、09、10、11 | 开头来源行明列 |
| `final-D.md` | §4.5–§4.8（无 `## 4.` 标题） | 12、13、14、15 | 开头来源行明列 |
| `final-E.md` | §5、§5.1–§5.4 | 16、17、18、19 | 无来源行，按小节标题对应 * |
| `final-F.md` | §5.5–§5.8（无 `## 5.` 标题） | 20、21、22、23 | 无来源行，按小节标题对应 * |
| `final-G.md` | §6、§6.1–§6.4 | 24、25、26、27 | 开头来源行明列 |
| `final-H.md` | §2、§7（唯一跨两个不相邻章节的一篇：§2 在文件开头、§7 在后段） | 28；§7 另交叉引用 09、24、26、27 | 开头说明证据分级沿用专题 28 的三档约定 |

\* `final-B` / `final-E` / `final-F` 三篇开头没有 provenance 行；上表的对应关系是按该篇小节标题与专题 H1 的逐字对应推出的（例如「3.5 代码高亮与 diff 的呈现」对「专题 05：代码高亮与 diff 呈现」），**正文本身未标注来源**。

### 8.3 不确定项与「未找到证据」汇总

汇总口径：对 `docs/research/grok-tui/` 下 28 篇专题与 8 篇分节稿按 `未找到证据` / `未找到` / `不确定` 三种词面做全量检索，再补读各篇自带的「未找到证据」专章（如专题 03 §7、04 §8、05 §3.7、06 §8、10 §末、11 §10、17 §9、22 §4.7 与 §末、23 §9、26 §8 等），最后把同一事实在专题稿与分节稿中的重复出现合并计数。共 81 条，分 12 组；括号内是出处文件（同一事实以专题稿为准），条目中的代码路径照抄原文，未做改写。

另需说明两点：按上述口径检索，**专题 01（主题与配色）与专题 19（审批与提问）没有记录任何「未找到证据」条目**，因此下表没有对应的独立分组；反之，某些「是明确的『无』而非『未找到』」的项（如无 reduce-motion 开关）仍按上游原标注收在此处，以便合稿统一处理。

#### 主题、配色与设计令牌（6 条）

- 不存在全局设计令牌模块：`spacing` / `padding` / `indent` 三词没有统一的常量模块，`LayoutConfig` 只覆盖滚动区。（`03-typography-spacing-density.md` §7）
- `block_pad_left/right` 的合法区间校验未找到：`LayoutConfig::validated()` 只 clamp `outer_hpad_*`，`block_pad_*` 原样透传。（`03-typography-spacing-density.md` §7）
- 弹窗与主题没有统一常量表：`modal_window.rs` 内的 `footer_lines`、`v_pad` 等没有集中常量，都是各模态各自字面量。（`10-modals-overlays-focus.md`）
- `ModalSizing::medium()` 存在但未找到生产调用点（grep `medium()` 于 `xai-grok-pager/src` 无命中），在用只有 `Default` / `large` / 各模态内联字面量。（`10-modals-overlays-focus.md`）
- `TableBorders::ASCII` / `DOUBLE` 两套常量在仓库内未找到使用点（`format_table` 硬编码 `::BOX`），可能是为未来主题化预留。（`04-markdown-rendering.md` §8）
- 未找到证据表明表格单元格会使用 `code_background` 或任何自定义背景（只有 `Line::style` 的背景色随行传递）。（`04-markdown-rendering.md` §8）

#### 字形与排版（8 条）

- 未找到任何行距参数：只在 `RtOptions.line_ending` 看到 `LineEnding::LF`，没有「行高 > 1」的概念。（`03-typography-spacing-density.md` §7）
- 「代码块后必空行」未找到强制规则：只有「开栅栏前空行」，闭合栅栏后是否有空行由源文档决定。（`03-typography-spacing-density.md` §7）
- 窄屏「文案变短」的映射表未找到：找到字号 / 内容降级（`cascade_truncate`）、rail 隐藏、面板行删除，但没有把 `Thinking...` 缩成 `…` 之类的规则。（`03-typography-spacing-density.md` §7）
- CJK 禁则换行未找到证据，换行行为完全由 `textwrap` + `unicode_width` 决定。（`03-typography-spacing-density.md` §7）
- 脚注定义如何渲染未找到证据：`parser_options()` 未显式启用 `ENABLE_FOOTNOTES`，`Event::FootnoteReference` 是全仓唯一一处相关代码。（`04-markdown-rendering.md` §8）
- `+ item` 列表标记只被推断为「不套色也不替换」，没有测试直接断言其渲染输出。（`04-markdown-rendering.md` §8）
- 任务列表 `[x]` 的最终视觉是推断值（不存在字形替换，只对 `[x]` 范围套色），没有测试直接断言。（`04-markdown-rendering.md` §8）
- 除欢迎屏 logo 外，未发现任何全局水印 / 背景图案绘制代码。（`02-glyphs-and-iconography.md`）

#### 代码、diff 与图形化内容（8 条）

- 语言判定没有 shebang 解析也没有内容嗅探（判定入口只有扩展名与显式 token 两类）。（`05-code-and-diff-rendering.md`）
- 没有「只高亮可见区域」的裁剪，也没有 per-hunk / per-line 的高亮结果缓存。（`05-code-and-diff-rendering.md` §3.7）
- diff 过大没有总行数 / 总字节的显式上限，靠 hunk 粒度、`MAX_CONTEXT` 裁剪、`max_lines` 预算与后台 caps 共同约束。（`05-code-and-diff-rendering.md`）
- 未找到专门的二进制检测或占位渲染：文本 diff 路径没有二进制分支。（`05-code-and-diff-rendering.md`）
- 未找到任何形如「已复制」「按 X 复制」的 UI 提示文案，代码块也没有复制按钮或快捷键提示。（`05-code-and-diff-rendering.md`）
- 明文表格的 ANSI 渲染路径（`render_ansi`）与 ratatui 路径共享排版代码，但只验证了 ratatui 路径，`render_ansi` 的消费方未追查。（`04-markdown-rendering.md` §8）
- Mermaid / LaTeX 的完整渲染算法未覆盖，只覆盖到「Markdown 元素如何触发它们」的接口。（`04-markdown-rendering.md` §8）
- Sixel 与半块字符渲染在本快照中不存在（`sixel` 全仓库零命中，`▀ ▄ █ ▌` 只用于进度条、滚动条、dashboard 光晕等文本语义），降级链止于文本 affordance。（`07-mermaid-images-video.md` §3.4）

#### 布局与 scrollback（6 条）

- 未找到任何「左右分栏 / 侧栏 / 可拖拽分隔条」的布局代码或配置。（`08-layout-and-panes.md`）
- 未找到证据表明 agent 视图存在横向降级：窗口宽度不触发主视图的结构性降级，横向降级只发生在 dashboard、timeline rail 与 welcome 屏。（`08-layout-and-panes.md`）
- 分栏、面板宽度、面板位置、侧栏顺序、面板高度上限均未找到证据：面板宽度永远写死为整宽，高度上限全是硬编码常量。（`08-layout-and-panes.md`）
- catalog 面板在本快照中没有把 `catalog.overlay.visible` 置为 `true` 的路径，打开入口不可达，不宜写成「快捷键 X 打开」。（`13-auxiliary-panes.md`）
- 子代理并发上限在 pager 侧未找到默认值证据（pager 只做计数与呈现）。（`13-auxiliary-panes.md`）
- `xai-grok-active-sessions` 提供「进程级的会话占用事实」，但未找到它在 dashboard 中被直接渲染的证据。（`15-multi-session-dashboard.md`）

#### 模态、浮层与输入区（7 条）

- 模块注释写 `ModalWindow` 是 "accent-colored square border"，代码实际用 `theme.gray_dim`；引用视觉描述时应写「灰色直角边框，标题加粗」。（`10-modals-overlays-focus.md`）
- 未找到「模态打开时整屏变暗」的证据：`Clear` 只重置被覆盖的矩形，弹窗外像素不变，除非该浮层单独调用 `dim_area`。（`10-modals-overlays-focus.md`）
- `extensions_modal` 的 `Enter` / 提交类确认语义未找到可引用的统一约定（8000 行文件未逐条通读）。（`10-modals-overlays-focus.md`）
- `ActiveModal` 的 12 个变体之外，未在 `src/views/` 找到另一个并行的「模态枚举」。（`10-modals-overlays-focus.md`）
- 输入区的 vim / 多模式光标差异未找到证据：没有按模式切换光标形状或颜色，也没有 normal/insert 两态。（`11-prompt-widget.md` §10）
- 任意文件路径的通用高亮未见实现（除 `@` 引用 chip 外；下拉行内高亮属下拉组件而非输入文本）。（`11-prompt-widget.md` §10）
- 占位符 `"Build anything"` 之外的默认值、以及 banner/tip 与输入框的具体遮挡规则没有逐帧验证（`any_dropdown_open()` 的让位语义只读到公开方法层）。（`11-prompt-widget.md` §10）

#### 键盘、鼠标与命令交互（8 条）

- `When::WelcomeScreen` 有枚举分支但零注册（未找到证据），欢迎页键位由 `handle_welcome_input()` 内联字面匹配。（`16-keyboard-and-actions.md`）
- 没有多键序列，也没有 leader key（`LeaderKey` / `leader_key` / `leader key` 全仓零命中）。（`16-keyboard-and-actions.md`）
- `actions/defaults.rs:901-902` 声称键位可经 `~/.grok/config.toml` 重绑，与实现不符——代码里没有任何读取键位配置的路径。（`16-keyboard-and-actions.md`）
- 「按住 Shift 放行鼠标给终端」在 app 代码里没有实现（无 Shift 分支），只能靠终端原生手势与关闭捕获的两个开关。（`17-mouse-selection-clipboard.md` §9）
- 右键在 pager 主视图无功能（全仓仅模态错误条把右键 / 中键当成「任意按下即关闭」）。（`17-mouse-selection-clipboard.md` §9）
- 三击在表格边框 / 分隔行上升级为「整表复制」，其完整行为未逐行阅读；拖选期间向编辑器 / 粘贴板写入图片、语音等其它剪贴板方向未覆盖。（`17-mouse-selection-clipboard.md` §9）
- 「URL 显示截断」没有通用组件，只有 Web Fetch 工具头与浏览器不可用 toast 两处局部处理。（`17-mouse-selection-clipboard.md` §9）
- 补全响应按 wire 顺序直出，未找到 pager 侧按 `priority` 排序的代码（顺序由 shell 决定）。（`18-commands-completion-search.md`）

#### 轮次、生命周期与终端还原（6 条）

- 120 s ack 硬超时的来源只写在注释里（盖过 shell 首个 prompt 的最坏路径），仓库内没有实测分布数据（未找到证据）。（`20-turn-interaction.md`）
- 首屏之前没有全屏 splash、也没有进度条实现（未找到证据）；唯一有名字的等待预算是 `EARLY_PREFETCH_WAIT = 2 s`。（`21-session-lifecycle.md`）
- SIGKILL 未找到证据表明有处理：信号任务只订阅 SIGINT / SIGTERM / SIGHUP。（`21-session-lifecycle.md`）
- 未找到取消 / 中断浏览器授权认证的按键证据。（`14-first-run-onboarding.md`）
- 会话内空状态与 picker 空状态未找到任何插画或大图，唯一的手绘图案是欢迎页的盲文 logo。（`14-first-run-onboarding.md`）
- 会话标题的生成时机与「早几轮重生成后冻结」策略在 pager 侧未找到代码，属 shell 侧行为（未找到证据）。（`15-multi-session-dashboard.md`）

#### 宿主集成：通知、标题、OSC 与复用器（8 条）

- `NotificationEventKind::TaskComplete` 存在于配置枚举与文档中，但未找到发射点（全仓库 grep 无命中）。（`22-host-integration.md`）
- `[ui.notifications.hooks]` 与 `notifications::hooks::run_hook` 未找到生产调用点（除定义外全是该文件的单元测试），应标记为待复核项、不要写成已生效。（`22-host-integration.md` §4.7 与 §末）
- shell 侧的 `SessionCommand::DispatchNotificationHook` 是另一套机制，不能拿来当作 pager 配置项已生效的证据。（`22-host-integration.md`）
- OSC 12 查询不存在：OSC 12 在本仓库只作为写入（设置光标色），全仓库 grep `12;?` 无命中。（`22-host-integration.md` §4.7）
- XTGETTCAP（`CSI > q` 之外的能力查询）不存在，全仓库 grep `gettcap`（大小写不敏感）无命中。（`22-host-integration.md` §4.7）
- 仓库里没有找到独立的声音 / 蜂鸣逻辑（未找到证据）：`Bel` 是通知编码方式之一，不是独立的「响铃」功能。（`20-turn-interaction.md`）
- 通知协议层的实现未逐行核对具体转义序列。（`20-turn-interaction.md`）
- 静音 / 响铃开关（通知协议之外）未找到证据：唯一相关的是终端未识别时通知协议被动回退到 `Bel`。（`23-accessibility-adaptivity.md`）

#### 无障碍与自适应（6 条）

- 色盲（CVD）专用配色与冗余编码未找到证据（全树检索 `colorblind` / `color_blind` / `deuteran` / `protan` / `cvd` 零命中）。（`23-accessibility-adaptivity.md` §9）
- 高对比模式、字体缩放、屏幕阅读器支持未找到证据（`screen_reader` / `screenreader` / `aria` / `a11y` / `accessib` 在 Rust 源码零命中）。（`23-accessibility-adaptivity.md` §9）
- 基于 locale 的字符集能力探测或 ASCII 降级路径未找到证据：`host::collect_unicode_env` 名字容易误导，它只是把环境变量收集成 UTF-8 map。（`23-accessibility-adaptivity.md`）
- agent 视图层面的宽度阈值未找到（宽度自适应是逐组件进行的：模态 `min_width` 地板 + 夹回缓冲区）。（`23-accessibility-adaptivity.md` §9）
- 未找到「终端过小就拒绝渲染并报错」的路径：整屏层面只有 `SCROLLBACK_MIN_ROWS = 5` 这一个布局地板。（`23-accessibility-adaptivity.md` §9）
- 「每个鼠标动作都有键盘等价」这条不变量没有文档声明也没有穷举测试，现有的逐项对照是从快捷键表与代码分支反推的，不构成形式化保证。（`23-accessibility-adaptivity.md`）

#### 技术栈与依赖（7 条）

- `xai-ratatui-textarea` 是 fork 还是完全自研未找到确证：`Cargo.lock` 无 `tui-textarea`，但 `RtOptions` / `word_wrap_line` 等符号在仓库内有两份独立实现，且没有任何指向上游授权的注释。（`24-custom-widgets.md`）
- `emit_to_scrollback` / `resize_purge_rerender` / `resize_viewport_height` 在 `pager`、`pager-render`、`pager-minimal` 三个生产 crate 里没有调用点，只出现在 inline crate 自己的 example 与 guard 测试中（未找到其它证据）。（`24-custom-widgets.md`）
- 拆 crate 的编译时间收益未找到证据（仓库内没有相关说明），可确证收益只有测试与复用。（`24-custom-widgets.md`）
- `third_party/NOTICE` 与目录不一致：列出的 `nfsserve 0.11.0` 与 `fuser 0.18.0` 在快照内未找到对应目录，合稿时不要把这两个 crate 算进 TUI 依赖面。（`26-dependency-stack.md` §8）
- `clippy.toml` 自我描述与位置矛盾：它引用的 repo-root 配置在本快照中不存在。（`26-dependency-stack.md` §8）
- 5 个 pager 可选依赖（`tokio-stream`、`tempfile`、`xai-chat-state`、`xai-grok-sampling-types`、`gethostname`）零引用，无法判断是有意为下游预留还是残留；`xai-grok-pager-render` 的 `notify` 亦无可达使用点。（`26-dependency-stack.md` §8）
- `rustls` 的实际归属未确认（推测经 `xai-grok-http` / `xai-grok-extra-ca` 间接进入，本次未追踪）；且体积与编译时长未测量、`release-dist` 的代价未验证。（`26-dependency-stack.md` §8）

#### 工程化与测试（7 条）

- 仓库中没有 ratatui 全屏快照（无 `TestBackend` 输出落盘为 golden 文件的机制），也没有 insta 的 `glob!` 或自定义 `Settings`。（`27-engineering-practices.md`）
- 未找到 proptest / quickcheck / arbitrary 等 property-testing 库；property 风格由「对一批数据跑一组不变量断言」的函数替代。（`27-engineering-practices.md`）
- `xai-grok-pager-render` 未找到 crate 级文档注释，crate 定位只能从模块名与 `Cargo.toml` 依赖推断。（`27-engineering-practices.md`）
- 仓库中没有外部链接有效性检查脚本，也没有 CI 工作流文件（快照里无 `.github/`）。（`27-engineering-practices.md`）
- `docs/internal/` 目录在本快照中不存在，因此依赖它的文档对账测试在当前快照下无法编译。（`27-engineering-practices.md`）
- `benches/pty_baselines/` 只有 `README.md`，平台基线 JSON 由首次 CI 运行播种；其示例命令与 `[[bench]]` 声明位置不一致（内部不一致）。（`27-engineering-practices.md`）
- `xai-grok-pager-minimal/src/guard.rs` 的源码扫描 guard 有一个已知的、需人工维护的缺口：模块清单要与 `lib.rs` 保持同步，缺一个模块就等于 guard 上的一个洞。（`27-engineering-practices.md`）

#### 设计意图与横向对照（4 条）

- 官方没有「选内联滚动而非全屏备用屏」这条信条：默认形态是全屏 alt-screen，内联是备用屏不可用或不可靠时的降级目标。（`28-design-intent-evidence.md`）
- 未找到 "keyboard-first" 这类字面宣称，键盘优先是从硬约束归纳出来的，不是官方原词。（`28-design-intent-evidence.md`）
- 未找到证据表明 Grok Build 在 TUI 设计层面点名过 codex / opencode / claude code 做取舍对比；扫描命中的只有工具实现移植、配置 / 会话兼容与内置 agent 类型名。（`28-design-intent-evidence.md`）
- 与同类 TUI 的关系是「兼容并收」而非设计对照。（`28-design-intent-evidence.md`）

### 8.4 证据分级与阅读方法

本报告全篇沿用专题 28 §0 与正文 §2 的三档约定，逐条标注：

| 标记 | 含义 | 使用规则 |
| --- | --- | --- |
| **【官方陈述】** | 面向用户 / 贡献者的明示表述（`README`、`docs/user-guide/**`、`docs/tutorial/**`、`CHANGELOG`），或源码注释中显式写明理由的 | 可以作为「Grok Build 的设计立场」引用；引用时最好连同英文原句与路径一起给出 |
| **【归纳】** | 从实现中读出的模式，官方未明说 | **不得当官方立场引用**；只能表述为「从实现看，它这样做」 |
| **【未找到证据】** | 已给出检索路径与关键词但零命中，或无法确证 | 保持原位、不升级为结论；本报告正文与附录 §8.3 均已保留 |

说明两点。其一，专题 28 §0 原设三档时把「源码注释中显式写明理由的」单列为【代码注释】，合稿时并入【官方陈述】（`final-H.md` §2 的处理方式）；上表与正文按合稿后的三档执行。其二，本报告全篇事实来自只读源码调研，**没有做任何构建或运行验证**，因此【官方陈述】指的是「上游自己这么说」，不等于「本报告验证过它成立」。

实践建议：引用本报告的技术结论时，凡标【归纳】的都应回到原始 `路径:行号` 复核。正文各节开头的路径约定行给出了简写展开规则（`final-A` / `final-B` / `final-D` / `final-F` / `final-G` / `final-H` 各有一行，例如 `final-A` 把 `theme/*`、`render/*` 定位到 `crates/codegen/xai-grok-pager-render/src/`）；`final-C` 与 `final-E` 两篇没有路径约定行，引用其路径时需回到对应的上游专题文件确认；【未找到证据】类结论的复核命令见 §8.5。

### 8.5 复现与复核

**快照与版本。**

- `reference/grok-build/SOURCE_REV` 的内容是 `9bb727ccdff0a793ee73bcde4e2e09cbef6b5387`（专题 26 §0 以同一值记录，证据标注为 `SOURCE_REV:1`）。
- 本地核对（2026-09-22）：该目录自身是一个 git 仓库（`git -C reference/grok-build rev-parse --show-toplevel` 指回它自己），`HEAD` 为 `4247f661689354b831191f11eeeac8424993fe3d`（提交信息 `Synced from monorepo`，提交日期 2026-09-19，共 47 个 commit）；`git cat-file -t 9bb727ccdff0a793ee73bcde4e2e09cbef6b5387` 报 `could not get object info`，即 `SOURCE_REV` 里记的那个 SHA 不在该仓库的对象库内。本报告只使用该快照的实际文件内容，未依赖任何 commit 图或历史 diff。
- 只读纪律：本报告的全部调研都是只读阅读，未运行任何构建、测试或 grok 二进制（`git -C reference/grok-build status --short` 无输出，工作树干净）。请保持同样纪律——该目录在文件系统层面是可写的，「只读」靠约定而不是权限位保证。

**可直接运行的复核命令。** 除注明外，cwd 均为 `reference/grok-build/`；全部命令均只读，且不需要编译或运行任何二进制。

- 查某个常量（本仓库该常量只有一处定义）：`grep -rn 'pub const SCROLLBACK_MIN_ROWS' crates/` → `crates/codegen/xai-grok-pager/src/views/agent.rs:94`
- 查该常量在某文件里的全部用法：`grep -n 'SCROLLBACK_MIN_ROWS' crates/codegen/xai-grok-pager/src/views/agent.rs` → 6 处命中
- 查某个主题定义（结构体 / 构造函数 / 枚举）：`grep -n 'pub struct Theme' crates/codegen/xai-grok-pager-render/src/theme/tokyonight.rs` → `:46`；`grep -n 'pub const fn groknight' crates/codegen/xai-grok-pager-render/src/theme/groknight.rs` → `:55`；`grep -n 'RosePineMoon' crates/codegen/xai-grok-pager-render/src/theme/mod.rs` → `:33`、`:48`、`:102`
- 查快捷键注册表：`grep -n 'pub(super) fn default_actions' crates/codegen/xai-grok-pager/src/actions/defaults.rs` → `:56`
- 查内置命令表：`grep -n 'fn builtin_commands' crates/codegen/xai-grok-pager/src/slash/commands/mod.rs` → `:77`
- 复核「未找到证据」类结论，例：语言判定无 shebang / 内容嗅探：`grep -rn 'shebang\|find_syntax_by_first_line' crates/codegen/xai-grok-markdown/src/ crates/codegen/xai-grok-pager-render/src/syntax.rs` → 零命中
- 复核「Sixel 与半块字符不存在」：`grep -rin 'sixel' crates/` → 零命中

复核「未找到证据」类结论时，**检索路径与关键词要与上游一致**：上游的「shebang / 内容嗅探零命中」是就语言判定路径而言，若把检索面放大到整个 crate，`sniff` 一词会命中 `prompt_images.rs` 里与图片字节嗅探相关的行（与语法判定无关）。因此优先照抄 §8.3 各条给出的出处文件，再按同样关键词在对应路径内重跑。

报告自身的检索（cwd 为仓库根 `/home/nirvana/jev-harness`）：

- 读某篇专题的关键结论：`grep -n -A 3 '## 关键结论' docs/research/grok-tui/01-theme-color-system.md`
- 全量列出「未找到证据」条目：`grep -rn '未找到证据' docs/research/grok-tui/`
- 复核 `SOURCE_REV` 的两处记录：`grep -n 'SOURCE_REV' docs/research/grok-tui/26-dependency-stack.md`

本报告未做任何构建验证，因此不提供需要 `cargo`、需要编译或需要运行 grok 二进制的命令。

### 8.6 本项目的下一步

以下 6 条都指向「如果本项目要开始写 TUI，先要决定或先要验证什么」，每条都能回溯到 §7 或某篇专题；它们不是泛泛建议，也不含任何本地验证结论。

1. **先决定「历史归谁持有」：应用内滚动 vs 终端原生滚动。** 上游三条形态路线里默认是全屏 alt-screen，内联与最小模式要额外付出两个 crate 与一条「只追加」约束（§7.2 第 1 条、§7.3 P3；专题 28 §1.2 的前提修正）。这条一旦定下，块模型与 commit pipeline 的形状就基本定了，所以要先定它再写第一行渲染代码（§7.3 P3 明确要求每一项都先回答「谁来持有历史」）。
2. **先定 crate 面与版本，再动手。** 起步组合取自专题 26 §7.1 的事实标准清单：`ratatui` 0.29 + `crossterm` 0.28 + `unicode-width` / `unicode-segmentation` / `textwrap`；`syntect`、`pulldown-cmark`、`supports-color` / `anstyle`、Mermaid 栈都先不引入（§7.3 crate 选型）。上游锁文件里的版本号（`ratatui 0.29.0`、`crossterm 0.28.1`、`textwrap 0.16.2`、`similar 2.7.0`）是 2026-09-22 的取值，落地前须重新核对（§7 末尾的「本节限制」）。
3. **先验证一件本项目特有的事：概率如何上屏而不塌缩成布尔。** 专题 09 §10 点名的做法是把 Jev 的 Choice 概率 / confidence 放进块的 `ctx` 或 `accent` / `background` 决策，用状态行文案表达「不确定」，不新增可视化组件（§7.1 第 2 条、§7.3 P2）。这是全篇唯一与上游不同的设计输入，值得先用最小原型验一次它是否真的可用。
4. **把「单一动作注册表 + 决策分层」一次做对，但键位放进配置。** 确定性路由 → Jev 判定 → LLM 决策三者最终都产出一个 `Action`，测试与遥测同源（§7.1 第 1 条）；键位写进配置则与上游相反，理由是规模而非理念（§7.2 第 6 条）。
5. **保留两条终端护栏：信号触发的恢复与子进程 env 隔离。** 没有前者，SIGINT / SIGTERM / SIGHUP 会把终端留在 raw mode、备用屏、鼠标捕获全开的状态；后者防的是 one-shot env 覆盖泄漏给每个子进程（§7.1 第 10 条）。这与「不做过度防御性编程」不冲突——它们防的是真实故障。
6. **测试只保留一档起步：纯函数单测 + `Buffer` 就地断言。** 元测试与 PTY e2e 全家桶的价值真实但成本也真实（独立 pty-harness crate、按平台维护 JSON 基准），在做出第一个可用 TUI 之前不要抄（§7.2 第 4 条，源自专题 27 §3–§4）。
