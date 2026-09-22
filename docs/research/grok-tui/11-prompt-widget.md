# 专题 11：输入区（prompt widget）设计

> 专题：输入区（prompt widget）设计｜调研日期：2026-09-22｜证据范围：`crates/codegen/xai-grok-pager/src/views/prompt_widget/{mod.rs,image_state.rs}`、`crates/codegen/xai-grok-pager/src/views/{completion_dropdown.rs,slash_dropdown.rs,prompt_suggestion.rs,queue_pane.rs,history_search.rs}`、`crates/codegen/xai-grok-pager/src/app/{queue_edit.rs,inline_edit.rs,external_editor.rs,agent_view/**}`、`crates/codegen/xai-ratatui-textarea/src/{textarea.rs,editor.rs,editor_keys.rs,wrapping.rs,render/}`、`crates/codegen/xai-grok-pager-render/src/{prompt_images.rs,render/image_overlay.rs,theme/mod.rs}`、`crates/codegen/xai-grok-pager/docs/user-guide/03-keyboard-shortcuts.md`、`.../06-theming.md`

## 1. 模块分层：一个可复用的输入组件 + 一个自研 textarea 内核

Grok Build 的输入区由三层组成，边界非常清晰：

| 层 | 位置 | 职责 |
| --- | --- | --- |
| 组件层 | `crates/codegen/xai-grok-pager/src/views/prompt_widget/mod.rs`（3820 行） | chrome 绘制、提交路由、chip 元素、图片、下拉协调、ghost text |
| 编辑内核 | `crates/codegen/xai-ratatui-textarea/src/textarea.rs`（3482 行） | 文本缓冲、wrap 缓存、撤销栈、kill buffer、选区、鼠标、滚动条 |
| 表现层原语 | `crates/codegen/xai-grok-pager-render/src/` | `key`（键位匹配宏）、`prompt_images`、`theme`、`render`（overlay 几何） |

`prompt_widget` 的模块注释明确划了责任边界：**它不管焦点管理（Esc、Tab）也不画 accent 线/选择框，那些由调用方负责**（`views/prompt_widget/mod.rs:3-5`）。这个拆分让同一个组件被 agent 视图、welcome 屏、dashboard dispatch 框、权限/提问面板复用（`views/prompt_widget/mod.rs:240-272` 的 `PromptStyle::overlay()` / `inline(bg)`）。

同目录还有 `image_state.rs`（199 行，图片 chip 的重绑定）与 `tests.rs`（5273 行，测试量是主体的 1.4 倍）。

## 2. 视觉与状态

### 2.1 盒式 chrome 的默认形状

`PromptStyle::default()` 的几何参数（`views/prompt_widget/mod.rs:216-238`）：

```
vpad_top = 1, chrome = true, chrome_pad_left = 2, chrome_pad_right = 1,
show_prefix = true, show_accent_line = false, show_borders = true, image_preview = true
```

绘制顺序（`PromptWidget::draw`，`views/prompt_widget/mod.rs:2971-3474`）得出的实际布局：

```
 ╭─────────────────────────── 会话标题(可省) ───────────────╮   ← 顶边（vpad 行，show_borders 时）
 │                                                          │
 │  ❯ 输入文本在这里，长行自动软换行…                        │   ← ❯ 前缀(2 列) + TextArea
 │    续行                                                   │
 │                                    [Pasted: 12 lines]    │   ← chip 与文本同流排版
 ╰───────────────────────── grok-3 · plan ──────────╯        │   ← 底边内嵌 info 行
   ↑ 若 show_accent_line，最左列整列画 ┃
```

要点与证据：

- 顶边 `╭─╮`（`mod.rs:3049-3054`）、侧边 `│`（`mod.rs:3330-3335`）、底边 `╰─╯`（`mod.rs:3356-3361`）；accent 竖线用 `crate::glyphs::accent_bar()`，默认关闭，关闭时该列归还给内容（`mod.rs:3001-3010`、`mod.rs:176-178`）。
- 前缀默认 `❯`，宽 2 列（`mod.rs:693-694` 的 `PREFIX_WIDTH = crate::glyphs::PROMPT_ARROW_WIDTH`）；三种覆盖：历史搜索时变 `? `、模式覆盖（bash `! ` 黄、remember `# `）、以及普通 `❯`（`mod.rs:3084-3104`）。
- 未聚焦时对盒内内容整体向背景色衰减 0.66（`mod.rs:3378-3389`），边框色切到 `theme.prompt_border`（`mod.rs:2990-2994`）。
- 顶边可以内嵌会话标题（`style.title`），与底边 info 行同款样式（`mod.rs:3060-3081`）；草稿被 stash 时那里的标题是 `Stashed`，再与 `/rename` 标题用 `·` 拼起来（`app/agent_view/prompt_stash.rs:41-58`）。
- 底边 info 行左右两段：左边 `模型名 · flag1 · flag2`（flag 由 `mode_flags()` 生成，plan 用 `accent_plan`、非 Ask 的权限用灰或蓝），右边 `multiline` 与 `usage_warning`（`views/prompt_widget/mod.rs:291-351`、`mod.rs:3489-3594`）。

### 2.2 占位符

- 默认文案写死在 `draw` 里：`"Build anything"`，只在文本为空、且（未聚焦或 `placeholder_when_focused`）且没有语音 interim 时显示，并按输入区宽度截断（`mod.rs:3310-3321`）。
- 主 composer 选择「聚焦时隐藏占位符」（默认 `placeholder_when_focused = false`），feedback 框保留（`mod.rs:172-173` 的字段注释）。
- 模式级覆盖走 `PromptStyle::placeholder_override`；remember 模式的文案是 `"Save a memory note... (Shift+Enter for multiline)"`，多行模式下换成 `"... (Enter for newline, Shift+Enter to save)"`（`app/agent_view/mod.rs:370-381`）。

### 2.3 光标：形状、颜色、可见性

三层机制叠在一起，理解这块是复刻的关键：

1. **终端光标默认被隐藏**：初始化时 `execute!(stderr, …, cursor::Hide)`（`app/mod.rs:1503-1508`），程序自己决定什么时候把光标摆到哪里。
2. **位置由渲染结果回传**：`draw` 返回 `PromptRenderResult{ cursor_pos, post_flush_escapes }`，`cursor_pos` 只在 `style.focused` 且没有「空输入 + 语音 interim」时给出，否则 `None`（`mod.rs:3391-3399`；字段语义见 `mod.rs:404-411`）。这个值一路上抛到 `app/app_view.rs:4833-4846` 由终端写入。
3. **颜色用 OSC 12 改成主题色**：`cursor_color_escape()` 产出 `\x1b]12;rgb:RR/GG/BB\x07`，取色是当前主题的 `accent_user`（`xai-grok-pager-render/src/theme/mod.rs:564-568`），启动时 `apply_cursor_color()` 写入（`theme/mod.rs:571-585`），退出/切主题时用 OSC 112 还原，并用 `CURSOR_COLOR_APPLIED` 原子开关保证「只撤销自己画过的」（`theme/mod.rs:559-561`、`theme/mod.rs:590-596`）。用户手册同样记载：Grok 会用 OSC 12 把终端光标设成 `accent_user`，退出用 OSC 112 复位；而 `terminal` 主题（不涂背景）特意不动光标颜色（`docs/user-guide/06-theming.md:141`、`06-theming.md:26`）。

**光标形状**不是按模式切换的，而是启动时一次性策略：`[ui].cursor_blink` 三态 → `CursorStylePolicy::{Inherit, ForceBlinking, ForceSteady}`；未设置时**不发任何转义**，保留终端自己的形状/闪烁（`app/mod.rs:1427-1446`），强制时分别发 `EnableBlinking + SetCursorStyle::BlinkingBlock` 或 `DisableBlinking + SetCursorStyle::SteadyBlock`（`app/mod.rs:1509-1523`）。配置文件项见 `26-config-reference.md`：`ui.cursor_blink`（`boolean`，未设继承终端）。

**vim / 多模式下的光标差异：未找到证据。**输入区没有 normal/insert 之类的模式光标，也不改光标形状。`/vim-mode` 的说明是 "Toggle vim-style scrollback keybindings (j/k, h/l, g/G, y/Y, …)"（`slash/commands/vim_mode.rs:14-16`），实际生效点在 `views/agent.rs:859-1035`，注册表的 `key_for_mode` 也只对 `When::ScrollbackFocused` 的裸字母键做替换（`actions/mod.rs:448-463`）。`xai-ratatui-textarea` 里出现 "vim" 的地方只是双击选词的字符分类语义（`xai-ratatui-textarea/src/textarea.rs:1542-1587`）。

多模式真正改变的是**前缀色与文案**（Normal/Bash/Remember 三态，`app/agent_view/mod.rs:355-388`），而不是光标。

### 2.4 「语法高亮」：有，但只针对命令与 chip

输入区没有通用语法高亮（没有按语言、也没有为任意文件路径着色）。实际着色规则：

- **`/命令` 青色高亮**：颜色取 `theme.accent_skill`，最小模式（嵌入式）退化为 `text_primary`，因为最小模式是单色的（`mod.rs:3137-3143`）。何时着色有严格条件：行首的 `/` token 在「下拉打开或命令已识别」时着色；文本中段的 token 只在被真正识别（hoist）或是 skill/plugin 提及时着色（`mod.rs:3145-3165`、`mod.rs:3212-3222`）。着色函数是 `paint_slash_token_highlight`（`mod.rs:2946`）。
- **参数占位 ghost**：命令声明了 `arg_placeholder` 且参数还没输入时，在光标后以 `theme.muted()` 画占位文本（`mod.rs:3180-3194`）。
- **`/命令` 不完整个后缀 ghost**：partial token 之后补 `theme.muted()` 后缀（`mod.rs:3196-3210`）。
- **`@文件引用` chip**：接受 @补全后不是普通文本，而是一个 `KIND_FILE_REF` 原子元素，缓冲区文本是 `@path`，显示由 `file_ref_display()` 生成——`@` 与 `:` 灰、路径用 `theme.path`、行号用 `text_primary`，支持 `@foo/bar.rs:10-12` 形式（`mod.rs:3620-3633`、`mod.rs:2076-2095`）。
- **粘贴 chip**：多行或超大粘贴变 chip，标签是 `[Pasted: N lines]` 或 `[Pasted: 1.0 MB]`（十进单位），样式是方括号用 `theme.paste_dim`、内容用 `theme.paste_fg`、背景 `theme.paste_bg`（`mod.rs:3760-3810`）。
- **图片 chip**：`[Image #N]`，同一套 chip 样式（`mod.rs:2347-2372`、`pager-render/src/prompt_images.rs:745-747`）。

chip 是「显示文本 ≠ 缓冲文本」的通用机制：`TextArea` 为每个元素保存可选的 `display: Line`，渲染时按显示宽度重新计算 x 坐标（`xai-ratatui-textarea/src/textarea.rs:3242-3307`），显示超宽时截断且**保留右括号**，于是 `[Pasted ~10 lines]` 变成 `[Pasted ~1…]` 而不是 `[Pasted ~10…`（`textarea.rs:3408-3446`）。多个元素重叠一行时按显示宽度逐段推进，这也是「文本里的 `[Image #1]` 文本不会破坏命令解析」的原因——斜杠系统看到的是剥离了所有元素占位符的干净文本（`mod.rs:3654-3695`）。

## 3. 自动增高与行数上限

高度公式（`PromptWidget::desired_height`，`mod.rs:1601-1627`）：

```
height = vpad_top + textarea.desired_height(text_width) + info_block   // info_block: 有 info 行则 1
height = height.clamp(min.min(max_height), max_height)
min    = vpad_top + 1 + info_block
```

**上限不是组件常量，而是调用方给的 `max_height`**：

- 全屏 agent 视图：`max_prompt_height = area.height / 2`——输入框最多占视口一半（`app/agent_view/render.rs:806`）；未聚焦且开了 `collapse_unfocused` 时折叠到 `vpad_top + 1 + info_block`（`app/agent_view/render.rs:807-814`）。
- welcome 屏：`PROMPT_HEIGHT = 3` 是起始高度，`prompt_max_height = (content_height / 2).min(column_fit).max(PROMPT_HEIGHT)`（`views/welcome/mod.rs:177-194`）。

两个抑制抖动的细节：

- **浏览历史时冻结为 1 行**：`history_search.is_browse()` 时 `text_height = 1`，否则在不同高度的历史条目间上下移动会导致每按一次键布局就重排（`mod.rs:1614-1621`）。
- **内容超出可视高度时出现滚动条**：`TextArea.show_scrollbar` 默认 `true`，启用时最右一列留给轨道，正文按 `width - 1` 换行（`xai-ratatui-textarea/src/textarea.rs:224-237`、`textarea.rs:3165-3223`）。

行高由 wrap 缓存计算：宽度变化才重算；无自定义显示的元素走 `textwrap` 的 `WrapAlgorithm::FirstFit`（按词换行，`textarea.rs:2885-2913`），有 chip 元素时改走 `element_aware_wrap_ranges`，**把元素当作原子单位**——放不下就整体挪到下一行，元素内部的换行符不产生新视觉行（`textarea.rs:2915-2930`）。

## 4. 多行编辑能力

### 4.1 软换行 vs 硬换行

- 软换行是显示层的，缓存为字节区间 `Vec<Range<usize>>`（`WrapCache{width, lines}`，`textarea.rs:250-254`），上下移动按**视觉行**走并带 sticky 列（`preferred_col`，`textarea.rs:2375-2501`）；Home/End 默认按视觉行，Ctrl+A/Ctrl+E 按逻辑行（`textarea.rs:2503-2533`、`editor_keys.rs:15-24`）。
- 硬换行只有这几条来源：`Shift+Enter`/`Alt+Enter`（`crate::input::is_mod_enter`）、Kitty 送达的 `SUPER+Enter`、Apple Terminal 的 CoreGraphics 修饰键补救、裸 `Ctrl+J`/`Ctrl+M`、以及**反斜杠续行**（光标前是 `\` 时按 Enter 用换行替换它）（`mod.rs:1813-1819`、`mod.rs:2136-2146`、`mod.rs:2150-2188`、`textarea.rs:2005-2014`）。
- 裸 Enter 是**提交**，不是换行：`route_enter` 的判定顺序是「文件搜索下拉打开 → PassThrough」「Apple Terminal 换行修饰键 → 插入换行」「反斜杠续行 → 插入换行」「裸 Enter → Submit」（`mod.rs:2157-2188`，语义枚举见 `mod.rs:92-103`）。
- `/multiline`（`Ctrl+M` 在输入框聚焦时）把 Enter 与 `Shift+Enter` 语义对调，并在 info 行右侧显示 `multiline`；切换动作是 `ActionId::ToggleMultiline`（`app/agent_view/prompt.rs:598-600`、`mod.rs:848-849`、`app/agent_view/prompt.rs:486-529`）。

### 4.2 历史记录：完整 `String`，多行原样保留

- 条目类型就是 `HistoryEntry { text: String }`（`views/history_search.rs:25`）——多行提示词在历史里是一个含 `\n` 的字符串，没有按行拆分。模糊匹配在后台 nucleo 线程做，前端快照最多 `MAX_RESULTS = 100` 条（`history_search.rs:4-28`、`history_search.rs:55`）。
- 两个入口共享同一个面板（`Mode`：`Search` / `Browse`，`history_search.rs:264-301`）：
  - `/history` 是 Search：输入框当过滤词，导航只高亮，Enter/Tab 接受（`history_search.rs:381-387`）。
  - 空输入框按 `↑` 是 Browse：每移动一格就把整条历史灌回输入框，输入即「脱离」变成编辑，到底部最新一条再按 `↓` 关闭（`history_search.rs:389-395`、`app/agent_view/prompt.rs:709-737`）。
- Browse 打开时选中项吸附在最新一条（`stick_to_bottom = true`），用户一旦导航就清除（`history_search.rs:286-290`、`history_search.rs:404-418`）。
- `↑` 的完整路由：仅当模式为 Normal、输入为空、无文件搜索时生效；**队列非空时优先切焦点进 queue pane**，否则才开历史面板（`app/agent_view/prompt.rs:407-424`，queue 分支见 `app/agent_view/queue.rs:63-78`）。`↓` 永远不会打开历史面板（`app/agent_view/prompt.rs:408`）。
- 另有独立的 **stash**（`Ctrl+S`/`Alt+S`）：整个草稿（含图片、chip、`!` 模式）存一个槽位，空输入框按同一键恢复；被替换掉的旧草稿**只有文本**进 `↑` 历史（`app/agent_view/prompt_stash.rs:1-26`，手册 `03-keyboard-shortcuts.md:266`）。

### 4.3 撤销 / 重做

- 快照式撤销：`UndoEntry{text, cursor, elements}`，栈深上限 **100**（`UndoState.max_depth = 100`，`textarea.rs:269-330`）。
- 分组规则（`MutationKind`，`textarea.rs:279-293`、`textarea.rs:2096-2126`）：连续同类输入合并为一步；`Kill`/`Element`/`Replace` 永远单独成步；**光标跳变也断开分组**（比较 `last_cursor`）。宿主驱动的 `set_text` 故意留一个检查点，所以误替换可撤销；`clear_history()` 才是硬重置（`textarea.rs:2134-2138`）。
- 键位：Ctrl/Cmd+Z 撤销（`is_undo_input`，`textarea.rs:332-340`）；重做是 Ctrl+Shift+Z、Alt+Z（为那些把 Ctrl+Shift+Z 送成 Ctrl+Z 字节的终端准备）、Ctrl+R（`textarea.rs:2023-2055`）。
- chip 也进撤销快照（`elements` 一起存），图片则另有 `image_undo_stash` 在撤销后重挂（`mod.rs:656-666`、`views/prompt_widget/image_state.rs:30-40`）。

### 4.4 kill / yank：单槽缓冲，不是 kill-ring

- `TextArea` 只有一个 `kill_buffer: String`（`textarea.rs:184-186`），没有环、没有 append 语义。`yank()` 从该槽插入，空则不动作（`textarea.rs:2341-2363`）。
- kill 命令：Ctrl+K（到行尾）、Ctrl+U（到行首）、Ctrl+W（空白分词向后删）、Alt/⌘+D 与 Alt+Ctrl+H（向前/向后删词）；`kill_current_line` 在空行上吞掉前一个换行以合并行（`textarea.rs:2290-2339`、`editor_keys.rs:46-74`、`editor_keys.rs:129-155`）。
- Ctrl+Y 是 yank；撤销**不恢复** `kill_buffer`（注释明确写了 yank 与 undo 分离，`textarea.rs:2092-2093`）。

### 4.5 其他编辑行为

- 选区：鼠标拖拽/双击/三击、Shift+方向键扩展；选中后 `selection_style` 默认是 tokyonight 蓝底 `rgb(49,62,115)` + 亮前景 `rgb(192,202,245)`，为的是在任何终端配色下都可读（`textarea.rs:188-208`）。`Cmd+A` 全选被**显式限制在 Ghostty**（其他终端的 Cmd+A 属于终端自身的 Select All，不该抢），且需要用户在 Ghostty 里写 `keybind = cmd+a=unbind`（`mod.rs:133-138`、`mod.rs:1861-1873`、手册 `03-keyboard-shortcuts.md:499-508`）。
- `Ctrl+C`：文本非空时清空草稿并返回 `Edited`，空则 `Ignored` 交给上层（`mod.rs:1821-1829`）。
- 粘贴分流：`Ctrl+V`/`Cmd+V` 被拦截走 `handle_paste()`（否则多行内容会被内联插入而不是变成 chip）；`Ctrl+Shift+V` 是「内联粘贴」，不做 chip（`mod.rs:1831-1859`）。
- Tab 展开：默认 `tab_width = 4`，显示与插入时都把 `\t` 展开成空格，由外观设置热同步（`textarea.rs:244-247`、`textarea.rs:3347-3353`、`mod.rs:769-771`）。
- 鼠标滚轮可临时脱离光标独立滚动（`scroll_override`），一旦光标移动就弹回（`textarea.rs:220-223`）；拖拽到区域外会节流加速滚动（`textarea.rs:211-217`、`textarea.rs:354-376`）。

## 5. 附件与图片在输入区的呈现

图片不是「附件栏」，而是**与文本同流的原子 chip**：

```
 │  ❯ 看一下这个报错 [Image #1] 和 [Image #2] 的差异        │
 │                    └────┬───┘                            │
 │              内联 chip，随文本一起软换行                  │
 └──────── 光标停在 chip 上或其后时，输入框上方浮出预览 ────┘
```

- 常量：单条提示最多 `IMAGE_CAP = 10` 张图；后端拒绝任一边 < 8 px，插入前先校验并给出可直接显示的文案（`mod.rs:2338-2361`）。
- 插入过程：`begin_undo_group()` → 插入 `KIND_IMAGE` 元素 → 补一个空格 → `end_undo_group()`，即**一次撤销就整体消失**；同时记录 `post_insert_image_preview` 让预览立刻出现（`mod.rs:2363-2395`）。
- 缓冲文本是规范的 `[Image #N]`（`pager-render/src/prompt_images.rs:745-747`）。图片 chip 会把编号显示成 `[Image #N]` 而**不带路径**；路径只在预览浮层里出现。
- 预览触发条件（任一）：光标在 chip 上或正好在其右边界、鼠标悬停该 chip、或刚插入且光标未移动（`mod.rs:2594-2607`）。渲染走 `render_image_overlay`，返回**flush 之后**要写的终端转义（Kitty/iTerm2 内联图片），并且每帧若不重画就主动清掉 overlay 的 ID 1（`mod.rs:3441-3468`，函数在 `xai-grok-pager-render/src/render/image_overlay.rs:42-43`，几何常量 `MIN_BOX_WIDTH` / `MIN_META_BOX_HEIGHT` / `MIN_PIXEL_BOX_HEIGHT` 见 `render/image_overlay.rs:24-33`）。
- 「chip 文本变回纯文本」的情况被显式兜底：yank、撤销、纯文本粘贴后如果 `[Image #N]` 还在而记录仍在 `images`/撤销暂存里，`rechip_orphan_image_placeholders` 会重新注册 chip；找不到记录的占位符就作为纯文本发送并弹 toast（`views/prompt_widget/image_state.rs:1-40`、手册 `03-keyboard-shortcuts.md:508-511`）。
- 粘贴 chip 的阈值：行数 ≥ 4（compact 模式 ≥ 2）或字节数 > `PASTE_CHIP_DISPLAY_BYTES = 10_000`（`mod.rs:47-48`、`mod.rs:2306-2325`）。另有一个有趣的交互：**再粘一次相同内容 = 就地展开**该 chip 为全文（`mod.rs:2288-2299`）。
- 「有附件」会挡住外部编辑器（见 §7）：`ATTACHMENT_MESSAGE = "External prompt editing is not available while the draft has attachments."`（`app/external_editor.rs:12-13`）。

## 6. 排队消息（queue）的界面

queue pane 是一个独立的 `ListPane`（`views/queue_pane.rs:1-5`），**只在非空且可见时占位**（`is_visible()` = `overlay.visible && !entries.is_empty()`，`queue_pane.rs:583-585`）。

```
 │  #1 重构一下解析器                          [edit] [cancel] │   ← 选中/悬停行才显示按钮
 │  #2 跑一下测试 (+3 lines)                 [Send now] [edit] [cancel] │
 │  #3 加个启动脚本                                            │
 └─ 最多 3 行；超出时 ListPane 出现滚动条，故意把列表画宽 1 列 ─┘
```

- **高度上限 3 行**：`MAX_QUEUE_HEIGHT: u16 = 3`，`desired_height() = entries.len().clamp(1, 3)`，隐藏或为空返回 0（`queue_pane.rs:348-349`、`queue_pane.rs:599-606`）。
- **行内容**：前缀是位置号 `#N`（1-based，灰色），内容取**第一个非空行（trim 后）**；多行提示在尾部加 ` (+N lines)` / ` (+1 line)`（`queue_pane.rs:296-305`、`queue_pane.rs:104-134`、`queue_pane.rs:182-200`）。前缀宽度按最大位置号动态算：`2 + digit_count(max_pos)`（`queue_pane.rs:868-875`）。
- **三种动作按钮**，右对齐从右往左摆：`[cancel]`、`[edit]`、`[Send now]`；只在**鼠标悬停的那一行**或**面板聚焦时的选中行**上绘制（悬停优先），并且当一行放不下 `[Send now] + [edit]` 时会主动放弃 `[edit]` 而保住 `[Send now]`（`queue_pane.rs:921-1009`）。按钮的 rect 每帧重绑，悬停状态跨帧保留（`queue_pane.rs:351-416`）。
- **键盘**（面板聚焦时，`QueuePane::handle_key`，`queue_pane.rs:608-657`）：interject 键 → `ForceInterject`；`x`/`Delete`/`Backspace` → 删除；`e`/`Enter` → 编辑；`Shift+J` / `Shift+K` → 与下/上相邻行交换。所有动作都先过 `capabilities`（`can_delete` / `can_edit` / `can_reorder` / `can_send_now`），不可编辑的行尾部标 ` · protected`（`queue_pane.rs:90-91`）。
- 面板**从不直接改业务状态**，只发 `QueueEvent` 意图，由 `AgentView::handle_queue_key` 落到 `pending_prompts`（`queue_pane.rs:331-346`）。删除后选中邻居而不是跳回顶部（`queue_pane.rs:700-712`）。
- 数据有两个来源，删除/重排按 origin 路由：本地 `pending_prompts`（Local）与服务器权威共享队列（Server，走 `x.ai/queue/*`），server 行的 id 是其 `prompt_id` 的稳定 hash 并置最高位以免与本地自增 id 冲突（`queue_pane.rs:33-50`、`queue_pane.rs:80-88`）。
- **与输入框的空间竞争**：面板最多吃 3 行，且属于输入框上方的弹出层；选中行若为多行，还会在 overlay 区域画全文预览（`queue_pane.rs:1011-1021`）。最小模式另有 dock 布局用 `queue_body_rows` 分配队列区域（`views/dock/layout.rs:73`、`views/dock/mod.rs:153`）。
- **编辑流程**（`app/queue_edit.rs`）：进入时把当前草稿 stash 一次（`stash invariant`，只在 `enter_queue_edit` 设置、只在退出时 `take()` 一次，`app/queue_edit.rs:1-8`），提示模式切到 `PromptMode::EditingQueued{id, original, server_id, kind}`（`app/queue_edit.rs:27-46`）；`Shift/Alt+Enter` 仍是插换行、裸 Enter 保存（`app/queue_edit.rs:47-60`）；乐观队列行（enqueue RPC 未确认）按下编辑会提示 `"Still queueing, try again in a moment"`（`app/queue_edit.rs:16-18`）；脏草稿时切换面板会被挡住，且**不会**去打开一个「画不出来却吞输入」的确认弹窗（`app/queue_edit.rs:4-8`）。
- 相关联的另一处编辑：**就地编辑历史用户消息**（双击/Enter 进入、Enter 提交会 rewind 后重发）——注意总开关 `INLINE_EDIT_ENABLED: bool = false`，功能当前是关闭的（`app/inline_edit.rs:1-6`、`app/inline_edit.rs:25-27`），它复用了同一套 `TextArea` + `TextAreaState`（`app/inline_edit.rs:35-45`）。

## 7. 外部编辑器集成（`$EDITOR`）

用户可见的入口：最小模式绑 `Ctrl+G`；全屏 TUI 走 `/edit-prompt` 或命令面板里的 **Edit Prompt in External Editor**（手册 `03-keyboard-shortcuts.md:259`、`03-keyboard-shortcuts.md:273`；动作定义 `actions/mod.rs:102`、`actions/defaults.rs:20`）。

流程（`app/external_editor.rs` + `app/event_loop.rs`）：

```
/ edit-prompt  ──► dispatch_edit_prompt_external        （app/dispatch/external_editor.rs:8-41）
                     │ 已经有一个 pending_editor 就直接返回
                     │ 语音进行中 / 粘贴探测未完成 / 有附件 → 直接拒绝并给出文案
                     ▼
                PendingEditorRequest::PromptDraft{agent_id, original_text}
                     ▼
   prepare(): revalidate（可能被别的输入面抢走）→ 解析 $VISUAL/$EDITOR/$vi
              → 写 grok-prompt-<uuid>.md（0600, create_new）
                     ▼
   suspend_for_child(): 让子进程接管 TTY，输入读取暂停；超时会 requeue 并报「等待中」
                     ▼
   finish(): 成功则读回并校验；失败/非零退出/非法 UTF-8/超 4 MiB/草稿已被改动 → 全部保留原草稿并提示
```

要点与证据：

- 编辑器解析顺序 `$VISUAL` → `$EDITOR` → `vi`，用 `shlex` 切分（允许带引号的参数），解析失败会提示 `could not parse $VISUAL or $EDITOR`（`app/external_editor.rs:123-141`、`app/external_editor.rs:474`）。
- 临时文件：`std::env::temp_dir()/grok-prompt-<uuid>.md`，unix 下 `mode(0o600)`，`Drop` 里删除（`app/external_editor.rs:72-121`）。
- 读回契约：上限 `PROMPT_EDITOR_MAX_BYTES = 4 * 1024 * 1024`；非 UTF-8 拒绝；**剥掉编辑器自动追加的那一个末尾换行**——判定依据是「原草稿本来就不以 `\n` 结尾」，所以草稿自己的换行会原样保留（`app/external_editor.rs:11`、`app/external_editor.rs:97-110`、`app/external_editor.rs:289-301`、测试 `app/external_editor.rs:600-646`）。
- **stale 保护**：编辑器期间草稿被改过就丢弃编辑器结果并提示 `"The draft changed while the external editor was open; the newer draft was kept."`（`app/external_editor.rs:29-30`、`app/external_editor.rs:313-335`）。
- 成功路径 `apply_prompt_text()`：`history_search.deactivate()` → `set_text` → **`clear_history()`**（外部编辑器写入是一个新的起点，旧撤销栈作废）→ 光标移到末尾 → `refresh_slash()` → 清空 prompt suggestion（`app/external_editor.rs:337-347`）。
- 四种拒绝理由各有一条用户可见文案（附件 / 语音 / 粘贴处理中 / 输入面已被占用，`app/external_editor.rs:12-19`）；最小模式下失败信息 push 进 scrollback，全屏下走 toast（`app/external_editor.rs:244-260`）。
- 挂起/恢复由 `suspend_for_child` + `restore_after_child` 处理，并有超时重试与「正在等待」提示（`app/event_loop.rs:769-840`）。

## 8. 提示语 / 建议 / 幽灵文本

输入区有两种 ghost（都在文本之后渲染，**从不修改文本**）：

| 类型 | 来源 | 触发与行为 |
| --- | --- | --- |
| shell 补全 ghost | `SuggestionController`，ACP `x.ai/suggest` | 打字去抖后请求，带 generation 丢弃过期响应；命中前缀时本地裁剪 ghost（progressive matching）而不发请求（`views/suggestion_controller/mod.rs:1-6`、`mod.rs:34-46`、`mod.rs:849-853`） |
| 下一句提示 ghost | `PromptSuggestionController`，turn 结束后问 shell `suggestPrompt` | 同 generation 机制；**可见性每帧从当前文本推导**，不按键维护状态机（`views/prompt_suggestion.rs:1-14`、`views/prompt_suggestion.rs:32-70`） |

渲染与「不打断打字」的规则：

- 两者都画在光标处、用 `theme.ghost_text_style()` 并截断到行尾（`mod.rs:3401-3426`）。shell ghost 额外要求**光标必须在文末**、且没有 slash 高亮/内联 ghost（`mod.rs:3404-3415`）。
- 预测 ghost 的门：`prompt_suggestion_active`（每帧由 AgentView 在绘制/按键前设置，turn 运行中、bash/remember 模式、编辑队列行时为 false）→ 没有 shell ghost → 没有文件搜索/历史面板 → 光标在文末 → 没有 slash 活动（`mod.rs:807-825`、`mod.rs:651-654`）。文本不匹配时 ghost 直接消失，清空后回来；按 Esc 在空输入框上会 dismiss 直到下一条建议（`views/prompt_suggestion.rs:75-102`、`app/agent_view/prompt.rs:374-396`）。
- 接受方式：预测 ghost 用 `Tab` 或 `→`（`→` 在文末本来就是空操作，沿用 fish/zsh autosuggest 惯例），且**优先于动作注册表**，以免 Tab 被抢去切到 scrollback（`app/agent_view/prompt.rs:342-373`）；shell ghost 支持整词或整段接受（`AcceptMode`，`mod.rs:790-800`）。
- 打断保护：任何下拉/面板打开时 `any_dropdown_open()` 为真，临时提示条（ephemeral tip）会主动让位（`mod.rs:860-867`）；ghost 与提示都不会插入或改写缓冲区内容。另有按配置开关的上下文提示（undo 提示 / plan 关键词提示），在 `handle_key` 里按门控短路，默认两个都关时整段 per-keystroke 扫描被跳过（`mod.rs:1682-1691`）。

## 关键结论（供合稿引用）

1. 输入区是「可复用组件 + 独立 textarea crate」两层：`prompt_widget` 明确不管焦点与 chrome 归属，`xai-ratatui-textarea` 提供文本/撤销/元素/滚动条（`views/prompt_widget/mod.rs:1-16`、`xai-ratatui-textarea/src/lib.rs:4-16`）。
2. 视觉是一个四边盒：顶边 `╭─╮`（可内嵌标题，stash 时显示 `Stashed`）、左边 `┃`（默认关）、侧边 `│`、底边 `╰─╯`（内嵌 `模型 · flags`，右侧 `multiline`）（`views/prompt_widget/mod.rs:3040-3361`、`app/agent_view/prompt_stash.rs:41-58`）。
3. 光标方案是「隐藏终端光标 + 程序按渲染结果摆放 + OSC 12 把颜色改成主题 `accent_user`」；形状由 `[ui].cursor_blink` 三态决定，未设置则完全不发转义（`app/mod.rs:1425-1446`、`app/mod.rs:1503-1523`、`xai-grok-pager-render/src/theme/mod.rs:564-585`）。
4. 输入区**没有 vim 模式**：`/vim-mode` 只改 scrollback 键位，注册表的 `key_for_mode` 也只对 `When::ScrollbackFocused` 生效（`slash/commands/vim_mode.rs:14-16`、`actions/mod.rs:448-463`）。
5. 「语法高亮」只覆盖命令与 chip：`/命令` 用 `accent_skill` 青色（行首 vs 文本中段有不同判定）、参数与命令后缀画 muted ghost、`@路径` 与 `[Pasted: …]`/`[Image #N]` 走 chip 元素（`views/prompt_widget/mod.rs:3137-3222`、`mod.rs:3620-3633`、`mod.rs:3760-3810`）。
6. 自动增高没有固定行数上限：`height = vpad_top + 文本行数 + info 行`，上限由调用方给——全屏是 `area.height / 2`，welcome 是 `(内容高/2).min(列高)` 且起始 3 行（`views/prompt_widget/mod.rs:1601-1627`、`app/agent_view/render.rs:806`、`views/welcome/mod.rs:177-194`）。
7. 硬换行只来自 `Shift/Alt+Enter`、`Ctrl+J/M`、Apple Terminal 修饰键补救、反斜杠续行，以及多行模式下被对调的 Enter；裸 Enter 默认是提交（`views/prompt_widget/mod.rs:1813-1819`、`mod.rs:2136-2188`）。
8. 撤销是快照式、上限 100 步、按 MutationKind 分组且光标跳变即断组；kill 只有一个 `kill_buffer`，**不是 kill-ring**，且撤销不恢复它（`xai-ratatui-textarea/src/textarea.rs:296-330`、`textarea.rs:2096-2126`、`textarea.rs:2341-2363`）。
9. 历史条目就是含换行的完整 `String`（多行原样保留），`/history` 与空输入框 `↑` 共用面板但语义不同；`↑` 在队列非空时优先进 queue pane（`views/history_search.rs:25`、`history_search.rs:264-271`、`app/agent_view/prompt.rs:407-424`）。
10. 图片与粘贴都是**内联原子 chip**（`[Image #N]`、`[Pasted: N lines]`/`[Pasted: 1.0 MB]`），上限 10 张且任一边 ≥ 8 px；光标停/悬停/刚插入时在输入框上方浮出预览，内联图片走 flush 后的 Kitty/iTerm2 转义（`views/prompt_widget/mod.rs:2338-2395`、`mod.rs:2594-2607`、`mod.rs:3441-3463`）。
11. queue pane 最多 3 行、行内右对齐 `[Send now] [edit] [cancel]`、只在悬停或选中行显示，键盘是 `e/Enter` 编辑、`x/Delete` 删除、`Shift+J/K` 重排，且面板只发意图不改状态（`views/queue_pane.rs:348-349`、`queue_pane.rs:608-657`、`queue_pane.rs:921-1009`）。
12. 外部编辑器流程是「临时文件（0600、4 MiB 上限）→ 子进程接管 TTY → 读回校验并剥末尾换行 → stale/非法输入一律保留原草稿」，成功后 `set_text` + `clear_history`；有附件/语音/粘贴在飞时会直接拒绝（`app/external_editor.rs:72-141`、`app/external_editor.rs:289-347`、`app/event_loop.rs:769-840`）。

## 10. 未找到证据 / 存疑项

- 输入区的「vim / 多模式光标差异」：未找到任何按模式切换光标形状或颜色的代码，也没有 normal/insert 两态（见 §2.3）。
- 任意文件路径的通用高亮：除 `@` 引用 chip 之外未见实现；`@` 补全下拉行内的高亮属于下拉组件而非输入文本。
- 占位符文案 `"Build anything"` 之外的默认值、以及 banner/tip 与输入框的具体遮挡规则没有逐帧验证（`any_dropdown_open()` 的让位语义只读到公开方法层）。
