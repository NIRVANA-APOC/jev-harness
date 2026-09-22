# 专题 24：自研 widget 与组件抽象

> 专题：自研 widget 与组件抽象｜调研日期：2026-09-22｜证据范围：`crates/codegen/xai-ratatui-inline/`（src 全部 + README/NOTICE/Cargo.toml + tests/ + benches/）、`crates/codegen/xai-ratatui-textarea/`（src 全部 + NOTICE/Cargo.toml + examples/）、`crates/codegen/xai-grok-pager-render/src/render/`（mod.rs、draw.rs、safe_buf.rs、renderable.rs、color.rs、line_utils.rs、highlight.rs、preview_overlay.rs、tool_paths.rs、scrollbar.rs、image_overlay*、video_overlay.rs、gboom_overlay.rs、terminal_output.rs、bidi.rs、osc8.rs、wrapping.rs）、`crates/codegen/xai-grok-pager/src/views/`（文件级签名普查）与 `src/scrollback/`（entry_renderer、scrollback_pane）、`crates/codegen/xai-grok-pager-minimal/`（lib/commit/guard）；基线版本取自 workspace `Cargo.toml` 与 `Cargo.lock`

## 1. 分层总览：谁在给 ratatui 打补丁

Grok TUI 的绘制栈有四层，本专题讨论其中「自研」的三层（[`crates/codegen/xai-grok-pager-render/src/render/mod.rs:1`](crates/codegen/xai-grok-pager-render/src/render/mod.rs:1) 把这层定位为 "Generic rendering primitives used by the scrollback and viewport"）：

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

版本基线：`Cargo.toml:233`（`ratatui = { version = "0.29" }`）、`Cargo.toml:234`（`ratatui-core = "0.1"`）、`Cargo.toml:150`（`crossterm = "0.28"`）、`Cargo.toml:296`（`tui-scrollbar = "0.2"`）、`Cargo.toml:273`（`textwrap = "0.16"`）。

## 2. `xai-ratatui-inline`：fork 掉的 `Terminal` + 内联视口

### 2.1 fork 的事实与理由

- 派生声明在文件头：`xai-ratatui-inline/src/terminal.rs:1-3`（"Derived from ratatui's Terminal implementation … Modified for inline viewport support"），授权说明在 `xai-ratatui-inline/NOTICE:1-9`。
- README 自己列了必须 fork 的四项内部访问（`xai-ratatui-inline/README.md:63-71`「Why Fork ratatui's Terminal?」）：view 区域访问、直接设置视口位置、背缓冲重置与前缓冲访问、resize 时的缓冲状态。
- 代码里能对上号的新增公开面**只多不少**，且都带 "HACK"/说明注释：`viewport_area()`（`terminal.rs:1203`，注释写着 "HACK: this is added"）、`set_viewport_area()`（`terminal.rs:1247`，"HACK: this is made pub"）、`last_known_area()`（`terminal.rs:1210`）、`set_viewport()`（`terminal.rs:1216`，支持运行中在 `Inline` / `Fullscreen` 间切换）、`reset_back_buffer()`（`terminal.rs:612`）、`completed_buffer()`（`terminal.rs:312`，`test-support` feature 门控）。

### 2.2 与 ratatui 官方方案（inline viewport / `Terminal::insert_before`）的关系

两者是**同源加强**关系，不是替代：`insert_before` 的实现是照搬上游并加注释（`terminal.rs:723`「Implement `Self::insert_before` using standard backend capabilities.」→ `insert_before_no_scrolling_regions`）。真正的取舍点在于：

| 维度 | ratatui 0.29 上游 | 本 fork |
| --- | --- | --- |
| `insert_before` | 接收 `&mut Buffer` 网格 | 保留原版，另加 `insert_before_rows(&[(String, bool, bool)])`（`terminal.rs:649`），按「折行 / 硬断行 / xenl 挂起」语义发 ANSI 行，避免把 soft wrap 变成硬换行 |
| wrap 细节 | 无 | `xenl_pending`（`terminal.rs:199`）、`mid_chunk_end`（`terminal.rs:223`）、`write_semantic_rows`（`terminal.rs:852`）显式处理 DECAWM/xenl（满行折叠后光标停格）与 `\x1b[?7l` 临时关自动换行 |
| 大终端差分 | `Buffer::diff` 的 `pos_of()` 把扁平索引截成 `u16`，`width*height > 65535` 时整屏错位 | `diff_large`（`terminal.rs:1043`）在 `usize` 里做除法；`flush` 默认走它（`terminal.rs:329-338`，注释给了 420×160 = 67200 的具体反例） |
| 超链接 | 无 | 每帧链接层：`LinkSpan`（`terminal.rs:22`）→ `set_frame_links`（`terminal.rs:343`）→ `diff_large_with_links` + `emit_frame_with_links`（`terminal.rs:1074`、`1114`），链接变化与单元格变化一起参与 diff，并用 OSC 8 `id=` 把换行碎片合成一个链接（`terminal.rs:74-95`） |
| scrolling-regions 分支 | 上游可选特性 | 保留两条实现，`Cargo.toml:32-34` feature `scrolling-regions` 注明 "required for terminal fork only"；pager 不用它（`terminal.rs:666` 注释「Pager builds without this feature」） |
| 全高 inline 视口 | 无此特例 | `resize()` 里当 `Inline` 视口顶在 y=0 且高度 ≥ 终端高时按"占满终端"处理（`terminal.rs:429-434`），修的是改行后仍被旧高度截断的回归；对应测试 `inline_full_height_grows_with_terminal`（`terminal.rs:1283`） |

### 2.3 核心抽象

1. **`TerminalLike` trait（`common.rs:15`）**：把「写 scrollback 需要的终端能力」抽成 6 个方法（`size` / `viewport_area` / `clear` / `reset_back_buffer` / `set_viewport_area` / `writer_mut`）。`scrollback.rs` 与 `resize.rs` 都只依赖这个 trait，因此可以用 `MockTerminal` 断言字节流（`tests.rs:12`）。
2. **`segment`（`segment.rs:8`、`segment.rs:84`）**：`split_into_line_segments(input, term_width) -> Vec<LineSegment>`，零拷贝返回 `&str` 切片 + `ends_with_crlf`。基于 `anstyle-parse` 的 VTE 状态机逐字节推进，只区分四种事件（`Print` / `CR` / `LF` / `Other`，`segment.rs:29-35`），用 `unicode_width` 计视觉宽度决定折行点，裸 `\r` 重置列宽（`segment.rs:132-137`），宽度大于终端的字符单独成段（`segment.rs:156-159`）。
3. **`emit_to_scrollback`（`scrollback.rs:13`）**：把一段文本「自然流」打进原生 scrollback —— 定位到视口顶 → `\x1b[J` 清到屏底 → 打印分段 → 补视口高度的 `\r\n` → 清出新视口区 → 重置背缓冲 → 平移视口。
4. **`resize_purge_rerender`（`resize.rs:11`）**：resize 时整屏清 + **清 scrollback**（`\x1b[2J\x1b[3J\x1b[H`，`resize.rs:17`）再重放历史并重算视口 y。注意 README:108-111 说这是「RIS `ESC c`」，而代码实际不用 RIS，`resize.rs:16` 的注释解释了原因（RIS 在 iTerm/Terminal.app 不清 scrollback）——**README 与代码不同步**。
5. **`resize_viewport_height`（`resize.rs:68`）**：不换终端尺寸，只改视口高度；放大先向下扩、不够再顶上去，缩小时锚顶。非法高度（0 或 ≥ 终端高）返回 `InvalidInput`（`resize.rs:88-96`）。

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

### 2.4 两条互相排斥的「历史归属」策略

`ScreenMode` 有三个值（`xai-grok-pager/src/app/mod.rs:342-349`）：`Fullscreen`、`Inline`（备用屏不可用时整屏一个 inline 视口）、`Minimal`（实验性 `--minimal`，块打印进终端原生 scrollback）。

历史由谁持有，决定了能不能用重放型 helper：

- `Minimal`：只有 `insert_before_rows` / `insert_before` / `set_viewport_height` 合法（`xai-grok-pager-minimal/src/lib.rs:1-6`、`xai-grok-pager-minimal/src/commit.rs:250`）。
- 该约束不是靠注释维持，而是靠**测试**：`xai-grok-pager-minimal/src/guard.rs:9` 用 `include_str!` 把所有源文件读进来，断言其中不出现 `resize_purge_rerender` / `emit_to_scrollback` / `resize_viewport_height`（`guard.rs:10-13`），失败信息写明原因「would double-print committed scrollback」。
- 反过来，`emit_to_scrollback` / `resize_purge_rerender` / `resize_viewport_height` 在 `pager`、`pager-render`、`pager-minimal` 三个生产 crate 里**没有调用点**；全仓库只出现在 inline crate 自带的 `examples/inline.rs:253/270/297` 与该 guard 测试中（未找到其他证据）。

## 3. `xai-ratatui-textarea`：输入组件

### 3.1 与上游 `tui-textarea` 的关系

- **可确证**：`Cargo.lock` / workspace 依赖树里不存在 `tui-textarea`（全仓库检索 `tui-textarea` / `tui_textarea` / `rhysd` 只命中本地 crate 名 `xai-ratatui-textarea`），因此它不是以依赖形式引入的。
- **可确证**：`xai-ratatui-textarea/NOTICE:1-2` 声明 "first-party code maintained by xAI … not a wholesale redistribution of third-party source"。
- **可确证**：本 crate 依赖面很窄（`Cargo.toml:9-20`：crossterm、ratatui、ratatui-core、textwrap、tui-scrollbar、unicode-segmentation、unicode-width，可选 tracing），即编辑能力是自研的。
- **形状相似的旁证（不足以定性）**：`wrapping.rs` 暴露 `RtOptions`（`wrapping.rs:71`）、`word_wrap_line`（`wrapping.rs:177`）、`word_wrap_lines`（`wrapping.rs:271`）、`word_wrap_lines_borrowed`（`wrapping.rs:296`）、`wrap_ranges` / `wrap_ranges_trim`（`wrapping.rs:9/45`）、私有 `slice_line_spans`（`wrapping.rs:318`）——同一组符号名在 `pager-render/src/render/wrapping.rs:148`（`RtOptions`）、`:239`（`word_wrap_line`）、`:43/86` 再次出现了一份**独立实现**，`examples/textarea_demo.rs:34` 也从本 crate 导入 `RtOptions` / `word_wrap_line`。这更像「同一族 wrapping 代码在仓库内被复制多份」，仓库内**没有**任何指向 tui-textarea 或 textwrap 授权的注释。结论：**是 fork 还是自研无法定论，未找到确证**；能确证的是「无上游依赖 + 自有的 `RtOptions` 包装层」。

### 3.2 编辑能力

编辑器核心被抽成与渲染无关的 `EditBuffer` / `EditPlan`（`editor.rs:212` / `editor.rs:158`），`TextArea` 组合它（`textarea.rs:180-181` 的 `text: EditBuffer`）：

- 命令表 `EditCommand`（`editor.rs:20-33`）：插入、字素左右移、词左右移（`WordStyle::Small` / `WhitespaceDelimited`，`editor.rs:14-17`）、逻辑行首/行尾、删除字素/词/到行首/到行尾。
- 键位映射独立成 `editor_keys.rs`：`classify_key_event`（`editor_keys.rs:41`）与 `resolve_movement`（`editor_keys.rs:5`），并把 Shift/折叠边（`Movement::collapse_edge`，`editor.rs:69`）编进类型，避免运行时再推断。
- 计划-校验-应用三段式：`plan_replace_byte_range`（`editor.rs:322`）、`apply_plan`（`editor.rs:480`）、`validate_plan`（`editor.rs:514`），失败类型 `ApplyEditPlanError`（`editor.rs:195`）。
- 单行视图：`SingleLineViewport`（`editor.rs:203`）+ `EditBuffer::single_line_viewport`（`editor.rs:586`）——pager 的搜索框、模态输入都复用这一条路径（`xai-grok-pager/src/scrollback/search.rs:448`、`views/memory_modal.rs:510`、`app/app_view.rs:43`）。
- 撤销：`undo` / `redo`（`textarea.rs:2144/2160`）支持显式分组 `begin_undo_group` / `end_undo_group` / `cancel_undo_group`（`textarea.rs:2186/2196/2223`）。
- Emacs 风 kill ring：`kill_to_end_of_line` / `kill_to_beginning_of_line` / `kill_current_line` / `yank`（`textarea.rs:2316/2320/2326/2341`）。
- 剪贴板：`ClipboardProvider` trait（`textarea.rs:48`）默认 `InternalClipboard`（`textarea.rs:59`），宿主可换成系统剪贴板（`set_clipboard_provider`，`textarea.rs:1006`；pager 侧在 `pager-render/src/clipboard/mod.rs:11` 换成 arboard）。
- 鼠标：`handle_mouse`（`textarea.rs:1076`）、`tick`（`textarea.rs:1040`）返回 `MouseAction`，含拖拽自动滚动的节流与加速字段（`textarea.rs:207-225`）。

### 3.3 渲染能力

- 只实现两个 ratatui 接口：`WidgetRef for &TextArea`（`textarea.rs:3134`）与 `StatefulWidgetRef for &TextArea`（`textarea.rs:3146`，`State = TextAreaState`，只有 `scroll` 一个字段，`textarea.rs:264-267`）。因为用的是 `WidgetRef`，两个 crate 都打开了 ratatui 的 `unstable-widget-ref` feature（`textarea.rs` 的 `Cargo.toml:9`、`pager-render/Cargo.toml:9`）。
- 折行宽度要两枪定：`content_width`（`textarea.rs:3112` 附近）先按满宽折一次判断是否溢出，溢出才把宽度减去 `1 + scrollbar_padding` 给滚动条让位。
- 自带垂直滚动条：`render_scrollbar`（`textarea.rs:3168`）用 `tui_scrollbar::ScrollBar` 渲到 `ratatui_core` 的临时缓冲，再逐格拷回主流缓冲并覆盖样式。
- 逐段渲染 `render_lines`（`textarea.rs:3225`）：按「纯文本 → 元素 → 纯文本」切片，用 **display 坐标**（而非 buffer 字节）累计列宽，保证元素的展示文本比原始文本宽时后续内容不错位。
- 选中样式与保留策略是公开字段：`selection_style`（`textarea.rs:200` 附近）、`keep_selection_after_mouseup`（`textarea.rs:196`）。
- 内嵌元素 `TextElement`（`textarea.rs:88-99`）：`ElementId`（`textarea.rs:29`）+ 字节区间 + 宿主自定义 `ElementKind`（`textarea.rs:39`）+ 可选 `display: Line<'static>`；元素对光标与编辑是原子的（`textarea.rs:86-88`）。挂载/替换/恢复接口：`insert_element`（`textarea.rs:2555`）、`replace_range_with_element`（`textarea.rs:2568`）、`restore_elements`（`textarea.rs:2658`）、`set_element_display`（`textarea.rs:2643`）。交互事件 `poll_element_event`（`textarea.rs:1013`）返回点击/悬停进入/离开（`textarea.rs:73-82`）。

## 4. 自定义 widget 清单

判定口径：是否 `impl Widget` / `StatefulWidget`（或 `WidgetRef` / `StatefulWidgetRef`）。**`xai-ratatui-inline` 与 `xai-grok-pager-render` 里一个 `Widget` 实现都没有**（全目录 grep `impl … Widget … for` 无命中）；`pager` 有 4 处，`textarea` 有 2 处。

### 4.1 `pager-render/src/render/` 的绘制原语

| 名称 | 文件 | 职责 | Widget trait |
| --- | --- | --- | --- |
| `render_scrollbar` / `render_scrollbar_styled` | `render/scrollbar.rs:165` / `:210` | 用 `tui-scrollbar` 画平滑滚动条；跟随模式暗、脱离跟随变亮（`scrollbar.rs:195-206`）；含几何辅助 `split_area_for_scrollbar`(`:68`)、`needs_scrollbar`(`:102`)、命中换算 `scrollbar_click_to_offset`(`:130`) | 否 |
| `render_progress_bar` / `progress_bar_spans` | `pager/src/views/progress_bar.rs:73` / `:59` | 1/8 格精度进度条（`▏▎▍▌▋▊▉█`，`:16`）；ConHost 退化用 `░▒▓`（`:20`） | 否 |
| `render_preview_overlay` | `render/preview_overlay.rs:90` | 底部锚定的多行预览框（首 N 行 + `⋮` + 末 N 行）；入参 `PreviewStyle`(`:22`) / `PreviewConfig`(`:46`)，返回 `Option<Rect>` | 否 |
| `render_image_overlay` | `render/image_overlay.rs:43` | 图片 chip 预览浮层（有像素/无像素 × 有路径/无路径 2×2，`:5-10`）；返回 `Option<Escapes>` 供 flush 后直接发像素协议 | 否 |
| `overlay_geometry` / `plan_image_preview` | `render/image_overlay/geometry.rs:52` / `:44` | 纯函数几何规划（最小尺寸常量 `MIN_BOX_WIDTH=28`、`MIN_PIXEL_BOX_HEIGHT=8`、`MIN_META_BOX_HEIGHT=6`） | 否 |
| `render_video_overlay` | `render/video_overlay.rs:16` | 视频浮层框架：90% 居中框、标题、进度条、时间；画面本身由调用方 flush 后发协议 | 否 |
| `render_gboom_overlay` | `render/gboom_overlay.rs:17` | `/gboom` 彩蛋浮层框架 + HUD 条（`render_hud_bar`，`:70`） | 否 |
| `paint_match_highlights` | `render/highlight.rs:14` | 搜索命中后处理：给命中格加 `Modifier::REVERSED`（`invert_cell`，`:91`），不覆盖底层配色 | 否 |
| `render_terminal_lines` / `render_terminal_plain` | `render/terminal_output.rs:27` / `:39` | 把 PTY 原始字节流喂给最小 VTE 解释器，产出带样式 `Line` 与去转义纯文本；上限 `MAX_ROWS=50_000` / `MAX_COLS=8_192`（`:17-18`） | 否 |
| `LinkOverlay` / `scan_lines_for_url_overlays` | `render/osc8.rs:104` / `:397` | 扫描文本产出可点链接区域，交给 inline 的链接层 | 否 |
| `visual_line` / `is_enabled` | `render/bidi.rs:125` / `:26` | RTL 视觉重排；只给需要按视觉列做命中/选择的调用方用（`safe_buf.rs:4-6`） | 否 |
| `dim_area` / `fade_region` / `blend_area` / `recede_area` / `force_area_fg` / `blend_color` | `render/color.rs:281/216/226/251/270/160` | 浮层压暗、粘性头被顶出时的渐隐、256 色/真彩混色（`indexed_to_rgb` `:12`、`nearest_indexed` `:56`） | 否 |
| `truncate_str` / `truncate_line` / `fit_line_to_width` / `cascade_truncate` / `is_unsafe_display_char` | `render/line_utils.rs:58/63/104/177/34` | 宽字符安全的截断与适配；`is_unsafe_display_char` 挡 C0/C1 与 bidi 控制符（防逃逸注入与 Trojan-Source 伪装，`:30-33`） | 否 |
| `Renderable` | `render/renderable.rs:10` | 自研虚拟滚动 trait：`render(area, buf)` + `desired_height(width)`；已实现 `()`/`&str`/`String`/`Span`/`Line`/`Option<R>`/`Arc<R>`/`Box<R>`（`:23-102`） | 自研 trait |
| `SafeBuf` | `render/safe_buf.rs:15` | 给 `Buffer` 加越界安全的 `set_line_safe` / `set_span_safe` / `set_string_safe` / `set_line_safe_bidi` | 扩展 trait |
| `shorten_path` / `path_basename` / `path_for_tool_header` / `path_for_tool_surface` | `render/tool_paths.rs:124/187/198/205` | Read/Edit 工具头的路径显示策略（`ToolPathSurface::{Collapsed,Expanded,Fullscreen}`，`:12-19`），非绘制组件 | 否 |

### 4.2 `pager/src/views/` 与 scrollback 的组件

| 名称 | 文件 | 职责 | Widget trait |
| --- | --- | --- | --- |
| `StatusBar` | `views/status_bar.rs:39` | 一行状态栏：左/中/右三段对齐，遵守左右留白 | **是**（`Widget`） |
| `ShortcutsBar` | `views/shortcuts_bar.rs:196` | 底部快捷键提示条；渲染前逐格 `reset()` 再上底色（`:205-212`），并有待确认提示位（`with_pending`，`:183`） | **是**（`Widget`） |
| `ListPane<T: ListItem>` | `views/list_pane/render.rs:61` | 通用列表：选中高亮、滚动条、可选底栏/搜索栏；`State = ListPaneState` | **是**（`StatefulWidget`） |
| `ScrollbackPane` | `scrollback/scrollback_pane.rs:125` | 主对话区：折叠、选择、鼠标、搜索高亮、链接；`Widget` 实现只是薄壳，真正的路径是 `render_with_scratch`（`:139`）复用 `ScratchBuffer`（`scrollback/render.rs:69`） | **是**（`StatefulWidget`） |
| `EntryRenderer` | `scrollback/wrappers/entry_renderer.rs`（`Renderable` 实现在 `:543`） | 单条 scrollback 条目的虚拟化渲染：`desired_height`(`:544`) 按宽度算高、`render`(`:563`) 画内容；带 `skip_rows` 支持半可见条目直接落绘（`:25-28`） | 自研 `Renderable` |
| `BlockContent` trait | `scrollback/block.rs:40` | 各 block 类型（tool/diff/…）的内容接口，与 widget 层级配合 | 自研 trait |
| `ModalWindow` | `views/modal_window.rs`（`render_modal_window:236`、`render_modal_shortcuts:627`、`predict_shortcut_rows:560`） | 所有弹窗共用的边框/标题/关闭按钮/Tab 栏/底部快捷键/尺寸协商（`ModalSizing` `:90`） | 否（自由函数 + 配置结构） |
| `ListOverlay` | `views/overlay_list.rs:18` | prompt 区共享的列表浮层（强调条、粗体标题、单行滚动列表 + 光标） | 否 |
| `TimelineRail` | `views/timeline.rs`（`compute_rail:93`、`render_rail:187`、`render_tick_hover_popup:248`） | 用「每回合一 tick」的轨道**替代**滚动条；几何每帧算一次，渲染与命中测试共用（`:1-4`） | 否 |
| `AgentStatusBar` | `views/agent_status.rs:41`（`render:89`） | 右对齐的若干状态片段，返回 `HashMap<&'static str, Rect>` 供点击命中 | 否 |
| 视图内滚动条 | `views/agent.rs:629` | agent view 自己的滚动条绘制（跟随模式把 thumb 混色变暗） | 否 |
| dock | `views/dock/mod.rs:1`、`dock/layout.rs:1` | prompt 上方的固定信息带（每节一个表头、行数上限 `MAX_DOCK_ROWS=8`），布局与绘制分离 | 否 |
| picker 系列 | `views/picker.rs`（`render_picker_frame:127`、`render_search_bar:219`、`render_picker_row:878`、`render_picker_content:1746`、`render_picker:2110` …） | 选择器/命令面板共用的框、搜索栏、行、分页 | 否 |
| 各弹窗视图 | `views/{usage_modal,agents_modal,settings_modal/render.rs,goal_detail,workflows,btw_overlay,question_view,new_worktree_dialog,elicitation_view/render}.rs` | 业务弹窗，逐个 `render_xxx(area, buf, state)` | 否 |
| 调试 HUD | `views/debug_style.rs:30`（chrome）、`views/fps_hud.rs`、`views/scroll_debug_hud.rs` | 主题无关的调试面板外观 + FPS / 滚动诊断读数 | 否 |

> 观察：`pager` 只在「长期存在、需要被 ratatui 布局系统按 `Widget` 调用」的四个组件上实现 trait，其余全部走自由函数 —— 组件不是靠 trait 组合的。

## 5. 绘制基础设施

### 5.1 `draw.rs`：绕过 `try_draw` 的帧管线

动机写在模块头（`render/draw.rs:1-38`）：ratatui 的 `try_draw()` 每帧无条件发光标转义，`Show` / `MoveTo` 会重置终端 500ms 闪烁计时器，30fps 下光标永远看起来是实心的。于是改成手写管线并把光标状态外置：

```rust
// render/draw.rs:501-540（节选）
pub fn draw_frame(terminal, cursor: &mut CursorState, ctx: &TerminalContext, render_fn) {
    let synchronized = should_emit_synchronized_output(ctx);
    if synchronized { let _ = terminal.backend_mut().queue(BeginSynchronizedUpdate); }
    let _ = terminal.autoresize();
    let (cursor_pos, post_flush_escapes) = { let mut frame = terminal.get_frame(); render_fn(&mut frame, &mut link_spans) };
    terminal.set_frame_links(&link_spans);
    let has_changes = terminal.flush_with_links().unwrap_or(false);
    terminal.swap_buffers();
    if !has_changes && !post_flush_wrote_cursor && action == CursorAction::None {
        terminal.backend_mut().writer_mut().discard();   // 空帧零字节
        return;
    }
    if let Some(post_flush) = post_flush_escapes { let _ = post_flush.write_to(terminal.backend_mut()); }
    cursor.apply(action, terminal.backend_mut());
    ...
}
```

- 光标去重：`CursorState::action`（`draw.rs:463`）把「位置没变 + 无单元格变化 → `None`」作为一等结果，`CursorAction` 四态见 `draw.rs:445-455`。
- 空帧优化有测试兜底：`idle_frame_emits_zero_bytes`（`draw.rs:691`）。
- DCS 同步输出的开关由终端上下文决定：`should_emit_synchronized_output` 在 tmux 为直接终端时**跳过**包装（`pager-render/src/terminal/tmux.rs:50-57`），`draw.rs:37-38` 说明原因。
- 写线程：`TermWriter`（`draw.rs:194`）把字节攒进 `Vec`，flush 时经 `mpsc` 交后台 `term-writer` 线程写 stderr（`draw.rs:374-435`），`WriterSync`（`draw.rs:66`）用序号保证帧与带外转义严格同序（测试 `frames_and_escapes_interleave_in_call_order`，`draw.rs:547`）；退出用有界 join（`join_within`，`:301`）。
- `PostFlush`（`pager-render/src/terminal/overlay.rs:73`）承载图片/视频等「diff 之后才能发」的字节，保证同一 DCS 块内原子呈现（`draw.rs:499`）。

### 5.2 `safe_buf.rs`：越界安全

`Buffer::set_line` / `set_span` / `set_string` 在上游遇越界坐标会 panic（`index_of`）；resize 竞态下算出的区域可能瞬时越界，于是 `SafeBuf` 统一先判 `y` 与 `x` 再写（`render/safe_buf.rs:1-3`、`:33`）。RTL 重排只挂在 `set_line_safe_bidi` 上，`set_line_safe` 保持逻辑顺序，因为下拉/弹窗的命中测试基于逻辑列（`safe_buf.rs:4-6`）。

### 5.3 样式与宽字符

- 样式：没有自造样式合并层，直接用 ratatui `Style` + `Buffer::set_style`；语义化的「压暗/渐隐/强制前景」集中在 `color.rs`（`dim_area:281`、`fade_region:216`、`force_area_fg:270`）。
- 宽字符：`line_utils.rs:58-104` 的 `truncate_str` / `truncate_line` / `fit_line_to_width` 按 `unicode_width` 切；`textarea` 侧的列宽累计同样用 display 宽度（`textarea.rs:3225` 起）；`pager-render/src/render/wrapping.rs` 的 `RtOptions` 承载 initial/subsequent indent 与换行算法（`:148-166`）。
- 不可信文本：`is_unsafe_display_char`（`render/line_utils.rs:34`）是唯一判定点，注释明确要求所有清洗处都调它，避免规则漂移。

### 5.4 组件复用方式：trait ≠ 唯一手段

三种复用形态并存，按「谁需要知道尺寸」选择：

1. **trait**：需要被 ratatui 布局按 trait 调用 → `Widget` / `StatefulWidget`（4 处）；需要虚拟滚动按宽度问高度 → 自研 `Renderable`（`render/renderable.rs:10`，`desired_height` 要求"ideally O(1)"）。
2. **自由函数 + `Rect`/`Buffer` 入参 + `Option<Rect>`/`Option<Escapes>` 出参**：绝大多数浮层。典型签名：

```rust
// render/preview_overlay.rs:90
pub fn render_preview_overlay(
    buf: &mut Buffer, area: Rect, content: &str,
    style: PreviewStyle, config: PreviewConfig,
) -> Option<Rect>;

// render/scrollbar.rs:165
pub fn render_scrollbar(
    buf: &mut Buffer, scrollbar_area: Option<Rect>,
    total_lines: u16, viewport_lines: u16, offset: u16, is_following: bool,
);

// pager/src/views/progress_bar.rs:73
pub fn render_progress_bar(
    buf: &mut Buffer, x: u16, y: u16, width: u16, value: f32, fg: Color, bg: Color,
);
```

3. **纯几何/纯行构造**：`overlay_geometry`（`image_overlay/geometry.rs:52`）、`TimelineRail` + `compute_rail`（`views/timeline.rs:93`）、`progress_bar_spans`（`views/progress_bar.rs:59`）——把「算在哪画」与「画」分开，几何可单测、可被命中测试复用（`timeline.rs:1-4` 明说这是为了渲染与命中不漂移）。行构造则主要靠 `Line` / `Span` + `line_utils`（`line_to_static`、`push_owned_lines` 在 `pager-render/src/render/line_utils.rs:9/25` 与 `xai-ratatui-textarea/src/render/line_utils.rs:5/21` 各有一份），没有 DSL。

浮层合成的统一顺序（三种 overlay 一致）：

```text
dim_area(buf, area, bg, 0.5)                 ← 先整体压暗
  → overlay_geometry(area, …) -> Rect        ← 纯函数定位（含最小尺寸早退 → None）
    → 边框 / 标题 / 内容 / 底部提示
      → 需要像素时返回 Escapes，由 draw_frame 在 diff 之后发出
```

## 6. 为什么拆成独立 crate

- **工作区显式登记**：`Cargo.toml:86-87` 把 `crates/codegen/xai-ratatui-inline` 与 `crates/codegen/xai-ratatui-textarea` 列为 workspace 成员；二者依赖面很窄（inline 4 个依赖，textarea 7 个），处于依赖图叶子位置，与 `xai-grok-status-line`（自述 "This is a leaf crate, upstream of the agent and of every client"，`xai-grok-status-line/src/lib.rs:1-5`）同类。
- **跨渲染模式复用**：同一个 `Terminal` 服务三种 `ScreenMode`；minimal 模式靠 `insert_before_rows` 提交并**复用 `EntryRenderer`**（`xai-grok-pager-minimal/src/lib.rs:4`）；`xai-grok-pager-minimal/Cargo.toml:6-13` 记载了刻意的依赖倒置——minimal 只经由 pager 再导出的 `PagerTerminal` 触达该 crate，并因此为 `cargo-shear` 加了 `ignored = ["xai-ratatui-inline"]`。
- **可测试性**（证据最充分的一项）：
  - `tests.rs:12` 的 `MockTerminal` 实现 `TerminalLike`，让 `scrollback.rs` / `resize.rs` 可以断言写出的字节与视口 y（例如 `scrollback.rs:104-147` 的三个用例、`resize.rs:284-304` 断言 `\x1b[2J\x1b[3J\x1b[H`）。
  - 差分测试：`tests/segment_differential.rs:1-12` 把**上一版基于 termwiz 的实现逐字内嵌**为参照，断言新 `anstyle-parse` 实现输出一致；`termwiz` 只作为 dev-dependency（`Cargo.toml:21-23` 注释 "Not linked into shipped binaries"）。
  - criterion 基准：`benches/bench.rs` 针对彩色 JSON / 纯文本的分段与视口操作。
  - 下游测试支持：`test-support` feature 暴露 `completed_buffer`（`terminal.rs:309-314`、`Cargo.toml:36-43`），并说明 CI 下 pager 的测试目标与生产链接同一个 rlib 变体。
- **行为约束固化在测试里**：minimal 的 `guard.rs` 用源码扫描禁止危险 helper（见 §2.4）。
- **编译时间**：**未找到直接证据**（仓库内没有关于拆 crate 以缩短编译时间的说明）。只能给出结构性事实：两个 widget crate 依赖极少、处于依赖图叶子；而 `pager-render` 自身依赖 40 余项（`Cargo.toml:8-70`，含 syntect、vte、image、tokio、notify 等），并不是编译时间的收益点。

## 7. 对本项目（jev-harness）的可用之处（按证据强度排序）

1. 内联视口 + 原生 scrollback 的「自然流」方案（`emit_to_scrollback` / `insert_before_rows`）可整体借鉴，但必须先想清**历史归属**：让终端持有历史就不能重放（§2.4 的 guard 测试是这个约束的模板）。
2. 帧管线不要用 `Terminal::draw`，改用 `get_frame → flush → swap_buffers` 并外置光标状态，才能保住光标闪烁与空帧零字节（`draw.rs:1-38`）。
3. `SafeBuf` 式越界安全包装是低成本的防御（resize 竞态下尤其），`is_unsafe_display_char` 式的单点清洗规则能防规则漂移。
4. 浮层用「压暗 + 纯函数几何 + 自由函数绘制 + `Option<Rect>` 返回值」四件套，几何与命中测试共用同一份计算。
5. 若上游组件不够用，优先 fork 最小面（这里只 fork `Terminal`）并写清 NOTICE 与派生命名，比自行重写整个框架划算。

## 关键结论（供合稿引用）

1. `xai-ratatui-inline` 是 ratatui 0.29 `Terminal` 的 fork，动机是上游不暴露视口面积/位置、背缓冲与 resize 内部状态；证据：`xai-ratatui-inline/NOTICE:1-9`、`README.md:63-71`、`src/terminal.rs:1-3`。
2. 它与官方 `Terminal::insert_before` 同源而非替代，新增的是语义化行插入 `insert_before_rows`（处理 xenl/soft-wrap）、OSC 8 链接层、`diff_large`（修 `pos_of()` 的 u16 截断）；证据：`src/terminal.rs:649`、`343`、`1043`、`326-338`。
3. `xai-ratatui-inline` 的核心抽象只有四个：`TerminalLike` trait、`LineSegment` 零拷贝分段、`emit_to_scrollback` 自然流、`resize_purge_rerender` 清屏重放；证据：`src/common.rs:15`、`src/segment.rs:8/84`、`src/scrollback.rs:13`、`src/resize.rs:11`。
4. `emit_to_scrollback` / `resize_purge_rerender` / `resize_viewport_height` 在三个生产 crate 中**无调用者**，minimal 模式还用一个源码扫描测试禁止它们；证据：`xai-grok-pager-minimal/src/guard.rs:9-13`，其余仅见 `xai-ratatui-inline/examples/inline.rs:253/270/297`。
5. `xai-ratatui-textarea` 不是上游依赖（`Cargo.lock` 无 `tui-textarea`），NOTICE 声明为 xAI 一方代码；但 `RtOptions` / `word_wrap_line` 等符号与 `pager-render/src/render/wrapping.rs` 存在同名重复实现，是否源自某个 fork **未找到确证**；证据：`xai-ratatui-textarea/NOTICE:1-2`、`src/wrapping.rs:71/177`、`pager-render/src/render/wrapping.rs:148/239`。
6. `TextArea` 的自研编辑核心是 `EditBuffer` + `EditPlan`（可计划-校验-应用、支持撤销分组与 kill/yank），渲染只经 `WidgetRef` / `StatefulWidgetRef` 两个接口；证据：`editor.rs:158/212/322/480`、`textarea.rs:3134/3146`。
7. 全仓库只有 6 处 ratatui widget trait 实现（pager 4 + textarea 2），`pager-render` 与 `xai-ratatui-inline` 为零；组件复用主要靠自由函数与自研 `Renderable` / `SafeBuf` trait；证据：`views/status_bar.rs:39`、`views/shortcuts_bar.rs:196`、`views/list_pane/render.rs:61`、`scrollback/scrollback_pane.rs:125`、`render/renderable.rs:10`、`render/safe_buf.rs:15`。
8. 浮层的统一契约是 `fn(&mut Buffer, Rect, …) -> Option<Rect>`（或 `Option<Escapes>`），几何由纯函数 `overlay_geometry` / `compute_rail` 先算，渲染与命中测试共用；证据：`render/preview_overlay.rs:90`、`render/image_overlay/geometry.rs:52`、`views/timeline.rs:93`。
9. 绘制不走 ratatui `try_draw`：`draw_frame` 手工编排 `autoresize → get_frame → set_frame_links → flush_with_links → swap_buffers`，只为保住光标闪烁并让空帧零字节；证据：`render/draw.rs:1-38`、`501-540`、测试 `draw.rs:691`。
10. `SafeBuf` 是对 ratatui `Buffer::set_*` 越界 panic 的防御（resize 竞态），RTL 重排只开在 `set_line_safe_bidi` 上；证据：`render/safe_buf.rs:1-6`、`:33`、`:39-49`。
11. 拆 crate 的可确证收益是**测试与复用**：`MockTerminal` + termwiz 差分测试 + `test-support` feature + minimal 复用 `EntryRenderer`；编译时间收益**未找到证据**；证据：`xai-ratatui-inline/src/tests.rs:12`、`tests/segment_differential.rs:1-12`、`xai-ratatui-inline/Cargo.toml:36-43`、`xai-grok-pager-minimal/src/lib.rs:4`。
12. README 与代码存在一处不一致：文档称 resize 用 RIS `ESC c`，代码实际发 `\x1b[2J\x1b[3J\x1b[H` 并注释说明 RIS 不清 iTerm/Terminal.app 的 scrollback；证据：`xai-ratatui-inline/README.md:108-111` vs `src/resize.rs:16-17`。
