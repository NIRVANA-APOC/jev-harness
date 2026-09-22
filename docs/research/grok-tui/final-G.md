## 6. 技术栈与工程实现

> 本节由上游专题 `24-custom-widgets.md`、`25-render-pipeline-performance.md`、`26-dependency-stack.md`、`27-engineering-practices.md` 提炼、去重并按主题重组，**未引入这四篇之外的新事实**。路径相对 `reference/grok-build/`；上游标注的「未找到证据」与彼此冲突的数字一律保留。

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
