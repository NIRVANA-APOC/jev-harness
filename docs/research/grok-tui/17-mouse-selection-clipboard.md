# 专题 17：鼠标、选择与剪贴板

> 专题：鼠标、选择与剪贴板｜调研日期：2026-09-22｜证据范围：`crates/codegen/xai-grok-pager/src/{app/mouse.rs, app/agent_view/{mod.rs,selection.rs,links.rs,notices.rs}, scrollback/{selection.rs,text_selection.rs,render.rs,scrollback_pane.rs}, views/{drag_select.rs,shortcuts_help.rs,memory_modal.rs}, tips/{word_select.rs,clipboard_focus.rs,ssh_wrap.rs}, hyperlink_route.rs, x10_filter.rs, appearance/（外）}`、`crates/codegen/xai-grok-pager-render/src/{clipboard/{mod.rs,trust.rs}, link_opener.rs, render/osc8.rs, terminal/{mod.rs,hyperlinks.rs}, input/mouse.rs, appearance/text_selection.rs}`、`crates/codegen/xai-ratatui-inline/src/terminal.rs`、用户手册 `docs/user-guide/{03-keyboard-shortcuts.md,21-terminal-support.md}`。全部路径相对 `reference/grok-build/`。

## 1. 总览：一次鼠标手势穿过哪些层

Grok Build 的鼠标链路是「**App 层算滚动 / Pane 层算命中 / 渲染帧提供几何**」三层分工：

- 输入进来先在 `AppView` 归一化（`crates/codegen/xai-grok-pager/src/app/app_view.rs:2367`）——滚轮被吃在这里，转成行数后按指针位置分派给某个 pane（`app_view.rs:2227` `dispatch_scroll`）。
- 非滚轮事件转交 `AgentView::handle_mouse`（`crates/codegen/xai-grok-pager/src/app/mouse.rs:30`），它只做两件事：比对**上一帧缓存的命中矩形**（`hit_*` 字段），以及把 Scrollback 区域分派给文本拖选 / 块拖选。
- 命中的坐标基准来自渲染期产物 `ResolvedSelectionModel`（每帧重建，见 §3.1），所以「命中测试始终假设几何来自上一帧」（`app/mouse.rs:4`）。

```
┌──────────────────────────────────────────────────────────────┐
│ 状态条 / 按钮行（hit_todo_close, hit_bg_status, hit_cancel_button …）│
│   左键单击 → 切 pane / 开关 overlay / 触发 Action             │
├──────────────────────────────────────────────────────────────┤
│ ② 粘性头部（sticky header）                                   │
│   自身发布 ResolvedSelectableLine，插到模型最前（可被拖选）    │
│ ③ 滚动区内容（可选文本区）                                    │
│   ├ 左键单击 → 选块（fold/nav）                               │
│   ├ 左键拖动 → 文本拖选（跨块、跨行）                          │
│   ├ 双击 → 折叠/导航 或 选词（取决于 keep_text_selection）     │
│   └ 三击 → 表格单元格 / 整表 / 整段落                          │
│ ④ 时间线轨道 timeline_rail → 点箭头跳转轮次                    │
│ ⑤ 滚动条 hit_scrollbar → 点击/拖动 = 绝对跳转                  │
├──────────────────────────────────────────────────────────────┤
│ ⑥ 滚动区与输入框之间的空带（"strip"）→ 只延迟武装文本拖选       │
├──────────────────────────────────────────────────────────────┤
│ ⑦ prompt 输入框 → textarea 自管（光标定位、自身选择）           │
└──────────────────────────────────────────────────────────────┘
```

## 2. 鼠标支持哪些交互

### 2.1 交互清单

| 交互 | 触发 | 实现位置 | 行为 |
| --- | --- | --- | --- |
| 点击按钮/状态块 | 左键 Down | `app/mouse.rs:34-180` | 按 `hit_*` 矩形逐个判定，全部要求 `!pos_occluded()`（被模态遮挡则不响应） |
| 点击定焦 | 左键 Down | `app/mouse.rs:423`、`:718` | `pane_areas.hit_test` → `set_active_pane` |
| 点击滚动条 | 左键 Down | `app/mouse.rs:378-383` | `scrollbar_dragging = true` + `apply_scrollbar_click`，随后 Drag 复用同一函数（`mouse.rs:655-658`） |
| 拖动滚动条 | 左键 Drag | `app/mouse.rs:797-799`、`:1225-1256` | 用 `scrollbar_click_to_offset` 反解偏移（与滑块绘制同一套算术） |
| 整块复制按钮 | 左键 Down | `app/mouse.rs:397-399` | `Action::CopyBlockContent` |
| 打开块查看器 | 左键 Down | `app/mouse.rs:400-402` | `Action::OpenBlockViewer` |
| 时间线跳跃 | 左键 Down | `app/mouse.rs:384-396` | `timeline_rail.hit` → `scrollback.jump_to_turn` |
| 拖选文本 | 左键 Down+Drag+Up | `app/agent_view/selection.rs:471`、`:651`、`:870` | pending → active → 复制并保留高亮 |
| 拖选块 | 左键 Down+Drag+Up | `app/agent_view/selection.rs:893`、`:1000` | 按 entry_idx 区间复制整块，用 `"\n\n"` 拼接 |
| 双击/三击 | 左键 Up | `app/mouse.rs:826-913` | 见 §2.3 |
| 滚轮 | ScrollUp/Down | `xai-grok-pager-render/src/input/mouse.rs:152`、`app/app_view.rs:2367` | 见 §2.4 |
| 中键（仅 Linux） | Down(Middle) 且无修饰键 | `app/event_loop.rs:3158-3176` | 读 X11 PRIMARY，转成 `Event::Paste`，不触发文本拖选 |
| 指针悬停 | Moved | `app/mouse.rs:946-1210` | 行 hover、按钮 hover、链接 hover（macOS 还要轮询 Cmd） |

**右键未找到交互证据**：`AgentView::handle_mouse` 的 `Down` 分支只匹配 `MouseButton::Left`（`app/mouse.rs:32`）；全仓仅 `app/agent_view/modals.rs:1266-1270` 把右键/中键当成「任意按下即关掉模态错误条」。

两侧还有一个独立实现：模态内的拖选由 `crates/codegen/xai-grok-pager/src/views/drag_select.rs` 提供（模块注释写明用于「按 mouse-up 复制选区的模态」，如内存预览），它只负责「鼠标单元格 ↔ 文本行」的映射与色带绘制，press/drag 状态机与复制都在调用方（`views/drag_select.rs:1-5`、`:43-59`、`:146-180`）。

### 2.2 多击判定的两套计数器

- **文本级**：`TextClickState` 以 `(entry_idx, range_id, block_line_idx)` 为准（`app/agent_view/mod.rs:412-422`），窗口 `MULTI_CLICK_TIMEOUT_MS = 300`（同文件 `:424`），计数逻辑 `app/agent_view/selection.rs:1240-1251`。只有**精确落在可选列**上才算（`hit_test_text_exact`，`scrollback/text_selection.rs:390-410`），点在边框/空隙会落回块级路径。
- **块级**：`last_click` 只比 `entry_idx`（`app/mouse.rs:1082-1095`），用于折叠/导航。
- 输入框内的双击是**纯按时间配对、不比坐标**（`app/mouse.rs:19-26`），注释解释为「配对错误只损失一步 undo」。

### 2.3 单击 / 双击 / 三击的语义

文本路径（`app/mouse.rs:826-868`）：

1. 第 1 击：清掉旧的持久选区，选中该 entry（`mouse.rs:833-841`）。
2. 第 2 击：`select_word_at`（`app/agent_view/selection.rs:1254-1274`）→ 词或 **URL**（`semantic_selection_at`，`scrollback/text_selection.rs:1350`），`origin = DoubleClick`，立即防抖复制（`notices.rs:255-267`，节流 500 ms）。
3. 第 3 击：先试 `select_cell_at`（表格单元格，含整表退化），失败则 `select_paragraph_at`（`app/mouse.rs:846-850`）。段落选择在「单逻辑行」时退化为整行选择（`app/agent_view/selection.rs:1388`、`:1393` → `select_line_at`，`origin = TripleClick`，`:1294-1335`）。
4. 第 4 击起不处理（`_ => false`）。

**但这只在 `word_select` 模式下生效**：`is_text_selection_on_double_click()`（`app/agent_view/mod.rs:520-525`）读 `keep_text_selection` 设置（`xai-grok-pager-render/src/appearance/text_selection.rs:11-52`）。默认 `flash` 时双击是**折叠/导航**，走向 `handle_scrollback_click`（`app/agent_view/selection.rs:1082-1228`）：

- 双击组头/展开头 → 展开或折叠组；双击 bg-task / 子会话 / workflow → 打开对应 viewer；双击用户消息 → 折叠并滚到顶部（in-place 编辑被 `INLINE_EDIT_ENABLED` 门控关掉，`:1197-1207`）；三击 → 折叠并滚到顶部。

### 2.4 滚轮：归一化 + 手写惯性

滚轮不在 `mouse.rs` 里处理，而是 App 层：`ScrollDirection::from_mouse_event` → `MouseScrollState::on_scroll_event_at` → 按行数 `dispatch_scroll`（`app/app_view.rs:2367-2384`）。

归一化参数（`xai-grok-pager-render/src/input/mouse.rs:46-77`）：流间隔 80 ms 切分事件流、刷帧节奏 16 ms、默认 3 事件/格、轨迹板加速度上限 3、带宽区间阈值 8 ms / 20 ms、倍率 1.0 / 1.6 / 2.5、滚动窗口 6 次事件、单次刷屏增量下限 6 行。

「**每个终端每格发几个事件**」是硬编码表（`input/mouse.rs:282-330`）：iTerm2 / WezTerm / VS Code 系 / Zed / Cursor / Windsurf = 1；Apple Terminal / Warp / Alacritty / Rio / foot / Ghostty / Kitty = 3；**经 multiplexer 重新分块时强制 1**。`wheel_lines_per_tick` 在 iTerm2/WezTerm 或 multiplexer 下是 1 行，其余 3 行。VS Code 嵌入终端的轨迹板补偿为 15 行/格、检测窗口放宽到 25/50/60 ms（`:82-124`、`:332-345`）。

模式判定是启发式：「偏向轨迹板（避免过冲），只在第一格事件极快时才升级成滚轮」，用户可用 `scroll_mode` / `GROK_SCROLL_MODE` 强制（`input/mouse.rs:10-12`、`:25-33`）。

### 2.5 拖到边缘的自动滚动

由 30 fps 的 tick 驱动（`app/app_view.rs:5279`、`:5442` 调 `tick_drag_autoscroll`）。判据与速率（`scrollback/text_selection.rs:35-77`）：

```
EDGE_THRESHOLD = 2 行：指针进入内容区上下 2 行内即触发
距离 0..=2 → 1 行/拍   3..=5 → 2   6..=10 → 3   更远 → 5
```

自动滚动的生效区域会剔除粘性头部等「非内容 chrome」：头部行的指针位置直接返回 `None`（`app/agent_view/selection.rs:963-998`）。每拍滚动后先用**旧的**模型重算拖选头以保响应，渲染后再对**新**模型重夹一次（`selection.rs:302-310`、`:361`）。

## 3. 选择模型

### 3.1 「行 → 屏幕单元格」的映射

渲染期把每个可见块的每个可选行写成 `ResolvedSelectableLine`（`scrollback/text_selection.rs:142-159`），字段就是映射表：

| 字段 | 含义 |
| --- | --- |
| `screen_x` / `screen_y` | 该行首字符所在屏幕单元格 |
| `selectable_cols` | 行内可选列区间（相对 `screen_x`） |
| `block_line_idx` | **稳定行号**：块内完整输出的行序号，不随视口滚动变化 |
| `text` / `painted_region` | 复制用的逻辑文本 / 实际绘制的单元格文本 |
| `joiner_to_previous` | 软换行拼接符；`None` 表示硬换行 |

写入点：`scrollback/render.rs:479`（合成组头行）与 `:542`（普通行）；粘性头部由自己发布再用 `splice(0..0, …)` 插到模型最前，以保持 `visible_blocks` 按 `entry_idx` 有序（`scrollback/scrollback_pane.rs:56-74`）。

命中有三个粒度（都在 `scrollback/text_selection.rs`）：

- `hit_test_text_exact`（`:390`）——只认距离 0，用于双击选词；
- `hit_test_selectable_range`（`:299`）——取最近行，用于首次按下；
- `hit_test_nearest_in_range`（`:335`）——**锁在锚点区间内**取最近，用于拖动；注释明说这样「拖到死行会继续延伸而不会缩回」（`:332-334`）。

每行的列度量由 `col_metrics`（`:165-183`）统一提供：区内是精确列偏移，区外返回「到最近边的距离 + 被夹到边的列」，注释强调这是「同一行行为不可能分歧」的唯一入口。

### 3.2 跨块选区怎么表示

端点不是屏幕坐标，而是 `(entry_idx, range_id, block_line_idx, col_within_range)` 四元组（`RangeHit`，`text_selection.rs:199-206`）。跨块/跨行能力来自两层：

- 选择对象是「同一条 `ResolvedSelectableRange`（= 一个块的一段可选内容）内的 `block_line_idx` 区间」，复制时按 `joiner_to_previous` 决定拼 `"\n"` 还是直接续接（`text_selection.rs:449-483`、`:460-472`）。
- 块与块之间则由另一套结构 `ActiveBlockDrag { anchor_entry_idx, head_entry_idx }` 表达（`text_selection.rs:430-434`），复制时把区间内每个块的 `copy_visible_text_in_state` 结果用 `"\n\n"` 拼接（`app/agent_view/selection.rs:1000-1077`）。

`RangeHit` 还额外记录 `anchor_content_width`——按下瞬间的块内容宽度。原因是流式重排会让所有 `block_line_idx` 平移，复制必须用「同一几何」重建文本（`text_selection.rs:209-225`、`app/agent_view/selection.rs:717-796`）。

### 3.3 选区高亮样式

统一一个函数（`scrollback/text_selection.rs:848-861`）：

```rust
// 反相色带：bg = text_primary，fg = bg_base
pub(crate) fn apply_selection_highlight(theme: &Theme, cell: &mut ratatui::buffer::Cell) {
    let band = theme.text_primary;
    if band == Color::Reset || theme.bg_base == Color::Reset {
        cell.modifier.insert(Modifier::REVERSED);   // 无色 / 终端原生主题回退
        return;
    }
    cell.modifier.remove(Modifier::REVERSED);        // 清掉搜索高亮设过的 REVERSED
    cell.set_fg(theme.bg_base);
    cell.set_bg(band);
}
```

块拖选用整块矩形铺底色（`text_selection.rs:491-511`）。另一种「选择框」`SelectionBox` 是**块选中态**的边框（不是文本选区）：侧边 `│`、上下角 `┌┐└┘`，越界时用虚线 `┆` 表示被裁切，右上角可替换成关闭控件 `✗`（`scrollback/selection.rs:14-22`、`:24-43`、`:152-242`）。它会跨组件绘制，因此渲染函数不直接画，而是回传 `RenderOutput.selection_box` 让 frame 最后画（`scrollback/selection.rs:1-3`、`:45-71`）。

悬停/高亮持久性由 `SelectionOrigin`（Drag / DoubleClick / TripleClick，`text_selection.rs:233-238`）与设置项共同决定：`flash` 下 `tick_selection_highlight` 在 150 ms 后清除（`app/agent_view/mod.rs:431`、`app/agent_view/selection.rs:46-59`），`hold` / `word_select` 则只靠 Esc / 点击 / 滚动清除。

### 3.4 表格是选区的一等特例

`SelectionKind` 有两档表格语义（`text_selection.rs:240-251`）：

- `TableCell`：头锁在锚点单元格内，按该单元格的列带跨其换行片段；
- `TableGrid { anchor, head }`：矩形单元格区间，按 **TSV** 复制。

拖动时由 `resolve_table_drag_kind`（`text_selection.rs:734-752`）逐帧求解；几何作为 side-car `TableSelectionGeometry` 挂键在 `(entry_idx, range_id)` 上，键不匹配就整段不画（`text_selection.rs:255-267`）。表格复制前还会**重新探测几何并要求完全相等**，否则退化为线性复制（`app/agent_view/selection.rs:720-744`）。降级时持久高亮记录的 `kind` 是「实际进剪贴板的形状」而不是拖动形状（`selection.rs:798-812`）。

### 3.5 拖选状态机

```
按下：
  命中可选文本 → PendingTextDrag{anchor,start}      （selection.rs:471-508）
  否则          → PendingBlockDrag + deferred_text_press（:893-918）
拖动：
  位移 ≥ 1 单元格即升级（text_selection.rs:436-440）
  ├ PendingTextDrag  → ActiveTextDrag（含表格 kind 求解）  （selection.rs:525-591）
  ├ deferred press   → 一进可选文本立即转文本拖选（:596-624）
  ├ PendingBlockDrag → ActiveBlockDrag（:920-944）
  └ 按指针位置重算 drag_autoscroll（:626-649、:963-998）
抬起：
  ActiveTextDrag  → 重建文本 → 成功才落持久高亮 → 复制 → toast
                    （:870-891，注释明确「复制失败就不留空高亮」）
  ActiveBlockDrag → 逐块拼接复制（:1000-1077）
  未升过级       → 交回点击路径（含链接点击）
```

`handle_scrollback_drag_motion` 的分支顺序本身是设计约束：滚动条 → 活动文本拖选 → deferred 转换 → 活动块拖选 → pending 文本 → pending 块；末尾注释写明「带锚定的无锚按下独占整个手势，避免拖拽漏到 prompt 里变成编辑文本」（`app/agent_view/selection.rs:692-697`）。

## 4. 复制路径

### 4.1 鼠标复制与「终端原生复制」如何共存

有三条并存的路径：

1. **app 内拖选**（默认）：鼠标被捕获，拖选后 mouse-up 即写剪贴板并显示 toast。
2. **整块复制**：`CopyBlockContent` / 块拖选。
3. **交回终端**：两条设计出口——
   - `minimal` 模式直接关闭鼠标捕获（见 §6）；
   - opt-in 的 `/toggle-mouse-reporting`（或 scrollback 聚焦时 `Ctrl+R`）运行时开关捕获（`crates/codegen/xai-grok-pager/src/slash/commands/toggle_mouse_reporting.rs:1-8`、`crates/codegen/xai-grok-pager/src/actions/defaults.rs:858-873`）。

**「按住 Shift 放行」在代码里未找到证据**：`handle_mouse` 全程不检查 `mouse.modifiers` 的 Shift 位，只查平台链接修饰键（`app/mouse.rs:411`、`:727`、`:1201`）。放行是**终端层的手势约定**，用户手册明确写：`Shift+Insert` 是终端原生粘贴，许多终端用 `Shift+中键` 绕过应用鼠标上报（`docs/user-guide/03-keyboard-shortcuts.md:295-301`、`docs/user-guide/21-terminal-support.md:131-135`）。

### 4.2 剪贴板后端与路由

一次复制会**同时写多条 leg**（`xai-grok-pager-render/src/clipboard/mod.rs:3`、`:225-266`）：

```rust
// clipboard/mod.rs:130-148（纯函数，测试可注入 kill-switch）
let is_tmux = ctx.multiplexer == MultiplexerKind::Tmux;
let osc52 = !opts.no_osc52
    && (cfg!(target_os = "linux") || is_tmux || is_remote() || is_container_no_display() || opts.wrap_sink);
ClipboardRoute {
    native: true,                                   // 始终尝试
    tmux_buffer: is_tmux,                           // tmux load-buffer -
    osc52,
    osc52_tmux_passthrough: osc52 && is_tmux && ctx.embedded_editor.is_none(),
}
```

```
copy_to_clipboard(text)                     app/agent_view/notices.rs:248-252
   └─ clipboard::copy_text_or_file(text)    clipboard/mod.rs:565-595
        ├─ copy_text → clipboard_write_with_route（clipboard/mod.rs:225-266）
        │     ├ native : macOS pbcopy / arboard / CLI 工具（wl-copy、xclip…）
        │     ├ tmux   : tmux load-buffer -（stdin 走 spool，2 s 上限等待）
        │     └ osc52  : OSC 52（tmux 下按需包 DCS passthrough）
        │            ↓
        │     trust::resolve_copy_decision → ClipboardFeedback → toast + 遥测
        └─ write_copy_fallback → ~/.grok/last-copy.txt（0600，父目录 0700）
```

关键常量与环境变量：

- `GROK_OSC52_SINK` / `LC_GROK_OSC52_SINK`：`grok wrap` 声明它会在本地接住 OSC 52（走 `LC_*` 是因为 SSH 默认只转发 `LC_*`），(`clipboard/mod.rs:35-43`)。
- `GROK_CLIPBOARD_NO_OSC52`：全局关掉 OSC 52 (同上 `:45-49`)。
- `GROK_COPY_FILE`：备份文件路径覆盖（`clipboard/mod.rs:19-21`、`:471-481`）。
- `GROK_CLIPBOARD_NO_DATA_CONTROL`：禁用 Wayland data-control 后端，退回 CLI 工具（`docs/user-guide/21-terminal-support.md:111-112`）。

### 4.3 成功/失败如何判定与降级

`ClipboardDelivery` 有三态（`clipboard/trust.rs:13-20`）：`Confirmed` / `Unverified` / `Failed`。判定要点：

- 本机正常（非 remote、非 container）且某条 native leg 真写入 → `Confirmed`；Wayland 下必须要么 `wl-copy` 成功，要么「arboard 成功 **且** data-control 可用」（`trust.rs:96-98`、`:162-175`）。
- OSC 52 已发出但外层终端品牌未知且在 SSH/容器里 → `Unverified`（`trust.rs:131-139`）——注释解释：未知的 SSH 边界会把品牌标记抹掉，此时不该断言失败。
- 全部不可用 → `Failed`，remote/container 与本地用不同文案（`trust.rs:212-216`）。

toast 文案（`clipboard/mod.rs:331-357`）：`Copied!` / `Copied to tmux buffer, paste with prefix + ]` / `Copied via OSC 52.` / `Copy sent. If paste fails, use grok wrap or /minimal.` / `Copy failed. Try /doctor or /minimal.`。成功 30 tick（约 1 s），其余 120 tick（约 4 s）。

**备份文件降级链**（`clipboard/mod.rs:418-468`、`:543-595`）：备份文件**无论剪贴板成败都会写**；剪贴板报成功而文件写失败只记 debug，剪贴板失败而文件成功则 toast 变成「`Copy sent, saved to ~/.grok/last-copy.txt`」，两者都失败才是 `CopyDelivery::Failed`。

### 4.4 两个与剪贴板相关的提示

- **双击选词发现提示**：fold/nav 模式下一次「重复的双击助手文本」会弹 `Want double-click to select? /settings → Text selection · Ctrl+Y: enable now`，同一 session 最多 3 次、约 20 s 窗口、可被 `Ctrl+Y` 就地接受（`crates/codegen/xai-grok-pager/src/tips/word_select.rs:16-26`、`:36-52`；触发窗口 10 s，`app/agent_view/selection.rs:23-26`）。
- **剪贴板里有图片的提示**：机会式轮询，节流 1 s 只读一次 changeCount，只有 changeCount 变化才做较重的类型判定；触发冷却 30 s，同内容不重复提示（`crates/codegen/xai-grok-pager/src/tips/clipboard_focus.rs:29-34`、`:118-166`）。

## 5. 超链接

### 5.1 逐终端能力表（判据的唯一来源）

`HyperlinkCapabilities` 把「品牌差异」收敛成一个结构（`crates/codegen/xai-grok-pager-render/src/terminal/hyperlinks.rs:44-57`），渲染与输入都只读它：

| 品牌 | `osc8` | `id_param` | `osc22_cursor` | `native_link_hover` | `native_plain_url_open` |
| --- | --- | --- | --- | --- | --- |
| Apple Terminal | `HostileParser` | ✗ | ✗ | ✗ | ✗ |
| iTerm2 / Ghostty / Kitty / VTE / Terminator / Windows Terminal / Alacritty / Rio / foot / WezTerm | `Native` | ✓ | iTerm2/Ghostty/Kitty 为 ✓ | ✗ | ✗ |
| VS Code / Cursor / Windsurf / Zed | `Native` | ✓ | ✗ | **✓** | ✗ |
| Warp | `Unsupported` | ✗ | ✗ | ✗ | **✓** |
| JetBrains / GrokDesktop / Otty / Unknown | `Unknown` | ✗ | ✗ | ✗ | ✗ |

（`terminal/hyperlinks.rs:59-181`；测试 `:242-257` 断言 `native_plain_url_open` 只有 Warp 为真。）

是否真的发 OSC 8 由 route 决定（`crates/codegen/xai-grok-pager/src/hyperlink_route.rs:28-40`）：`emit_osc8 = (osc8 == Native) && skip_reason.is_none()`，`emit_id = emit_osc8 && id_param`。路由进程内缓存一次（同文件 `:20-26`）。

### 5.2 skip_reason：为什么某些环境不发 OSC 8

`TerminalContext::hyperlink_skip_reason`（`crates/codegen/xai-grok-pager-render/src/terminal/mod.rs:440-468`）按固定顺序给出人类可读原因：

```
apple_terminal（解析器会咬坏未知 OSC）→ unsupported_terminal（Warp）→
vte_old（VTE 版本号 < 5004，先于 multiplexer 判定，避免用户白折腾 tmux 配置）→
unknown_terminal → screen → tmux_old（tmux < 3.4）
```

### 5.3 OSC 8 的生成与消费

生成分两步：扫描期把链接位置收进 `LinkOverlay`，刷帧期发射序列。

- 扫描：`scan_lines_for_url_overlays` 在**软换行重新拼好的逻辑行**上跑 `linkify` 与自写正则，再把命中拆回各绘制行（`crates/codegen/xai-grok-pager-render/src/render/osc8.rs:1-6`、`:395-397`）。扫描分配的 id 从 `1 << 31` 起（`SCANNER_ID_BASE`），避免与 markdown 文档内从 0 递增的源 id 撞号而把两处不同链接合并成一条（同文件 `:97-100`）。
- 发射：`xai_ratatui_inline` 把 `LinkSpan` 折成逐单元格的 link 层参与帧差分，因此**链接受 diff 管理**：新增/移动会重写单元格，移除会重写单元格但不再带 OSC 8（`crates/codegen/xai-ratatui-inline/src/terminal.rs:19-44`、`:343-380`、`:382`、`:1071`）。

```rust
// xai-ratatui-inline/src/terminal.rs:55-72
fn write_osc8_open<W: Write>(w: &mut W, url: &str, id: Option<u32>) -> io::Result<()> {
    // url 中的控制字符会被剥掉，避免提前终止序列
    Some(id) => write!(w, "\x1b]8;id={id};{sanitized}\x07"),
    None     => write!(w, "\x1b]8;;{sanitized}\x07"),
}
fn write_osc8_close<W: Write>(w: &mut W) -> io::Result<()> { w.write_all(b"\x1b]8;;\x07") }
```

`id=` 会被**重铸**成进程内单调 id，因为 markdown id 是按文档从 0 计数的、而 OSC 8 的 `id=` 是终端全局的（`terminal.rs:74-92`）。跨行折行的同一链接靠这个 id 在 hover 时被视为一体。

### 5.4 点击打开链接

- **按下时武装**：`try_arm_link_click`（`crates/codegen/xai-grok-pager/src/app/agent_view/links.rs:168-179`）要求持有平台链接修饰键、未被遮挡、命中 `visible_link_map`；有原生 hover 的终端直接返回 false（让终端自己处理）。修饰键判定：macOS 走 CoreGraphics 快照判 Cmd，其他平台用事件里的 Ctrl（`app/agent_view/mod.rs:443-452`）。
- **抬起时打开**：只有「按下与抬起是同一单元格」才触发 `Action::OpenLink`（`app/mouse.rs:817-822`）。
- **谁能打开**：`app_should_open_link_on_click`（`app/agent_view/mod.rs:489-519`）——Warp 之外一律由 app 打开；Warp 上只有「标准 scheme 且显示文本就是裸 URL」的链接交给终端。
- **怎么打开**：`link_opener::open_url`（`crates/codegen/xai-grok-pager-render/src/link_opener.rs:60-98`）→ macOS `open` / 其他 `xdg-open`，分离 stdio；无 `DISPLAY`/`WAYLAND_DISPLAY`/`BROWSER` 时**不 spawn**，改为提示 `Could not open a browser. Open this URL manually` 并附完整 URL（`:41-56`、`:78-83`）。安全上先过 scheme 过滤（`Standard` = http/https/mailto；`EditorExtended` 另含 file/vscode/cursor/idea/zed，`terminal/hyperlinks.rs:20-42`），Windows 走 `ShellExecuteW` 而不是 `cmd /c start`（注释说明 `start` 会被 `&` 与 `%VAR%` 劫持，`:100-127`）。
- **hover 高亮**：无原生 hover 的终端上由 app 自绘；macOS 需要轮询 Cmd 状态，且只在指针近期移动过（3 s 窗口）或链接高亮仍激活时才轮询，避免空闲占用动画循环（`app/agent_view/links.rs:216-252`）；切换 screen mode 后还会用「屏幕外 Moved」重放一遍清掉残留 hover（`app/mouse.rs:1211-1221`）。
- **OSC 22 光标**：iTerm2 / Ghostty / Kitty 且 `osc22_cursor` 为真时，用 `\x1b]22;pointer\x1b\\` / `\x1b]22;default\x1b\\` 切换手型指针（`terminal/hyperlinks.rs:183-213`、`app/app_view.rs:4330-4345`）。

### 5.5 URL 与路径的识别范围

识别不止 URL，还包括**文件路径**（`render/osc8.rs:199-250`）：无空格路径段、含内部空格但必须以 `.ext` 结尾的段（macOS app bundle）、相对路径（必须带扩展名，避免 "and/or"、"TCP/IP" 被误判）、带引号的路径。相对路径只在「能唯一匹配本会话生成的媒体文件」或「join 到 cwd 且不逃出 cwd」时才成链接，歧义匹配保持纯文本（`:332-360`）。邮箱会补 `mailto:`，但 scp 风格的 `git@github.com:org/repo` 故意不识别（`:181-183`）。

**「URL 截断显示」未找到通用实现**。相关证据只有一处：Web Fetch 工具头会按可用宽度截断 URL 文本用于显示，但链接目标仍保留完整 URL（`scrollback/blocks/tool/web_fetch.rs:110-124`，`display_url = truncate_str(&self.url, ...)`）。浏览器不可用时的 toast 则反过来「把 URL 放最前面，让前缀截断只影响后面的提示文字」（`link_opener.rs:48-56`）。

## 6. 鼠标模式开关：什么时候捕获、什么时候放行

进程级单一状态位 `MOUSE_CAPTURE_ENABLED`（`crates/codegen/xai-grok-pager/src/app/mod.rs:134-135`，注释说明它覆盖 crossterm `EnableMouseCapture` 打开的 5 个 DEC 模式 + bracketed paste）。切换点：

- 启动/内联：`MOUSE_CAPTURE_ENABLED.store(!want_minimal, …)`（`app/mod.rs:1526`），全屏路径显式置 true 并 `execute!(EnableMouseCapture)`（`:1611-1613`）。
- 切到 **minimal**：先 swap 成 false 再发 `DisableMouseCapture`；若是「鼠标上报会泄漏成文本」的终端，额外补一发 ANSI 复位（`app/mode_switch.rs:114-128`）；切回 **fullscreen** 时重新 `EnableMouseCapture`（`:179-186`）。切换失败会回滚这个位（`:160`）。
- 启动前就判定为「鼠标泄漏」的终端（JetBrains + Windows，即 JediTerm）会被自动选成 minimal 并记一个标志，供空闲提示文案使用（`app/mod.rs:136-142`、`terminal/mod.rs:734-738`）。
- **原生选择持有期**：欢迎页展示认证 URL 时临时关闭捕获，让终端接管拖选；窗口结束再恢复，并且「只有我们自己拿掉的才恢复」——用户本来手动关掉的不动（`app/app_view.rs:4287-4315`）。
- **重聚焦重发**：ConPTY 转发可能吞掉 DEC 私有模式，把 SGR 鼠标上报降级成 X10 进而被解析成乱码字符，因此每次重聚焦在**已开启**的前提下重发一次 capture（`app/event_loop.rs:655-670`）。
- **恢复效果**：`Effect::ResetMouseReporting` 用「先 Disable 再 Enable」重置上报状态（`app/event_loop.rs:3946-3954`）。
- 退出/崩溃恢复：`terminal_restore` 只在标志为真时发 `DisableMouseCapture`（`app/terminal_restore.rs:80-86`、`:131-133`）。
- Windows 特例：`EnableMouseCapture` 在 Windows 上是 winapi 而非 ANSI，且 `SetConsoleMode` **会跨进程残留**；因此「终端拥有鼠标」时显式打开 QuickEdit、清掉 `ENABLE_MOUSE_INPUT`、保留窗口尺寸事件（`app/mod.rs:1309-1326`、`:1357-1375`）。

用户可用的运行时开关：`/toggle-mouse-reporting`（opt-in，配置 `[ui] mouse_reporting_toggle` / `GROK_MOUSE_REPORTING_TOGGLE`），以及 scrollback 聚焦时的 `Ctrl+R`；两条路都汇成同一个 `Action::ToggleMouseCapture`（`app/mod.rs:182-189`、`slash/commands/toggle_mouse_reporting.rs:24-39`、`actions/defaults.rs:858-873`）。

## 7. ssh / tmux / 特殊终端的兼容处理

| 场景 | 处理 | 证据 |
| --- | --- | --- |
| tmux < 3.4 / screen | 不发 OSC 8，原因分别记 `tmux_old` / `screen` | `terminal/mod.rs:461-466` |
| VTE < 0.50.4 | 不发 OSC 8，原因 `vte_old`，且优先于 tmux 判定（避免用户只升级 tmux） | `terminal/mod.rs:449-457` |
| tmux 内复制 | 额外写 tmux paste buffer + OSC 52 包 DCS passthrough（编辑器 `:terminal` 内不包） | `clipboard/mod.rs:81-88`、`:130-148` |
| 滚轮 | multiplexer 重新分块时强制 `events_per_tick = 1`、每格 1 行，避免过滚 | `input/mouse.rs:287-330` |
| SSH 复制不可达 | 仍写备份文件；toast 报路径；OSC 52 记 `Unverified` 而非失败 | `clipboard/mod.rs:441-458`、`trust.rs:131-139` |
| Apple Terminal over SSH | 不支持 OSC 52，只能靠备份文件 / `/copy` / `/minimal` / `grok wrap` | `docs/user-guide/21-terminal-support.md:141-152` |
| 未用 wrap 的 SSH 会话 | 一次性提示 `Run /doctor for details and fixes.`，`/doctor fix ssh-wrap` 可生成别名 | `tips/ssh_wrap.rs:23-49`、`docs/user-guide/21-terminal-support.md:154-164` |
| 终端把鼠标上报泄漏成文本 | 见 §6（自动 minimal / 补发 ANSI 复位） | `app/mod.rs:136-142`、`app/mode_switch.rs:121-126` |
| 上报被截断成 X10 形态 | 专门的 `X10ReassemblyFilter` 把「魔数形状的鼠标事件 + 紧随其后的位移行字节」在 50 ms 内重组成一条真实事件 | `app/x10_filter.rs:11-70`（`MAX_COMPLETION_GAP = 50ms`，`:28`） |
| 鼠标滚不动了 | 手册给出各终端重新允许鼠标上报的位置 | `docs/user-guide/21-terminal-support.md:233-238` |
| tmux 剪贴板配置 | `/doctor fix` 可持久化 `set -g set-clipboard on` 等选项；**故意不** source-file 或改运行中的 server | `docs/user-guide/21-terminal-support.md:26-43` |

## 8. 值得借鉴的设计取舍

1. **命中几何只来自上一帧**：不做「实时重算布局」，引用上一帧的 `visible_blocks` / `selectable_cols`，代价是流式输出下的重排需要靠 `anchor_content_width` 快照来回退（`app/mouse.rs:4`、`text_selection.rs:209-216`）。
2. **能力表 + 路由缓存**：品牌差异集中到 `HyperlinkCapabilities` / `ClipboardRoute`，其余代码不再 `match brand`，且一次性缓存避免每帧探测（`hyperlink_route.rs:20-26`、`clipboard/mod.rs:52-58`）。
3. **交付证据优先于「调用成功」**：写剪贴板只回报「某条 leg 真的落到被信任的目的地」，不可验证时明确降级为 `Unverified` 并把备份路径写进 toast，而不是假报成功（`trust.rs:10-38`、`clipboard/mod.rs:440-458`）。
4. **交互冲突用「延迟武装」而不是模态**：滚动区按下未命中文本时不立刻决定是块拖选还是文本拖选，先挂 `deferred_text_press`，指针进入文本再一次性转换（`app/agent_view/selection.rs:592-624`）。
5. **把一切「会烧掉用户决策窗口」的提示做成 ambient + 遮挡暂停**：选词提示 20 s 但被遮挡时暂停 TTL（`tips/word_select.rs:17-22`）。

## 9. 未找到证据 / 不确定项

- 「按住 Shift 让出鼠标给终端」在 app 代码里**没有实现**（无 Shift 分支），只能靠终端原生手势与关闭捕获的两个开关；手册的说法见 §4.1。
- 右键在 pager 主视图**无功能**（仅模态错误条的任意键关闭）。
- 三击的 `select_cell_at` 在表格**边框/分隔行**上会升级为「整表复制」（`app/agent_view/selection.rs:1449-1456` 注释与实现），但整表复制的完整行为未逐行阅读。
- 「URL 显示截断」没有通用组件；只有 Web Fetch 头与浏览器不可用 toast 两处局部处理（§5.5）。
- `views/shortcuts_help.rs` 的鼠标只有两件事：详情模式下滚轮翻 1 行（`:960-975`），以及把事件转交 picker（`:978-987`），未做独立拖选。
- 拖选期间向编辑器/粘贴板写入图片（`wrap_clipboard_image.rs`）、语音等其它剪贴板方向未在本专题覆盖。

## 关键结论（供合稿引用）

1. 鼠标事件只有一个总入口 `AgentView::handle_mouse`，非滚轮事件全部按**上一帧缓存矩形**命中，Scrollback 区才分派拖选；滚轮在 App 层被归一化后按指针位置分派给 pane（`app/mouse.rs:30`、`app/app_view.rs:2367`、`:2227`）。
2. 多击有两个独立计数器：文本级以 `(entry_idx, range_id, block_line_idx)` 为单位、块级只比 `entry_idx`，窗口都是 300 ms（`app/agent_view/mod.rs:412-424`、`app/agent_view/selection.rs:1240-1251`）。
3. 双击的语义由设置项 `keep_text_selection` 决定：默认 `flash` 是折叠/导航，`word_select` 才变成选词（三击选表格单元格→段落），且该能力通过可 `Ctrl+Y` 就地接受的提示灰度投放（`appearance/text_selection.rs:11-52`、`app/agent_view/mod.rs:520-525`、`tips/word_select.rs:16-26`）。
4. 选择端点是稳定的 `(entry_idx, range_id, block_line_idx, col_within_range)`，屏幕坐标只是每帧从 `ResolvedSelectionModel` 反查；跨块拖选由单独的 `ActiveBlockDrag` 表达并按 `"\n\n"` 拼接（`scrollback/text_selection.rs:199-251`、`app/agent_view/selection.rs:1000-1077`）。
5. 软换行与硬换行的区别保存在 `joiner_to_previous`，复制时决定拼接还是换行——这是「复制出来是段落还是多行」的关键（`scrollback/text_selection.rs:158`、`:460-472`）。
6. 选区高亮就一个函数：`bg = text_primary`、`fg = bg_base` 的反相带；无色/终端原生主题回退 `Modifier::REVERSED`（`scrollback/text_selection.rs:848-861`）。
7. 拖到边缘的自动滚动由 30 fps tick 驱动，边缘阈值 2 行、速率分四档 1/2/3/5 行每拍，且会剔除粘性头部等非内容区（`scrollback/text_selection.rs:35-77`、`app/app_view.rs:5442`、`app/agent_view/selection.rs:963-998`）。
8. 复制一次同时写三条 leg（native / tmux load-buffer / OSC 52），`ClipboardDelivery` 只按「有 leg 真的到达被信任目的地」判 Confirmed，SSH 未识别品牌时降级为 Unverified（`clipboard/mod.rs:225-266`、`clipboard/trust.rs:131-139`、`:178-217`）。
9. 剪贴板不可达的兜底是始终写备份文件 `~/.grok/last-copy.txt`（0600，可用 `GROK_COPY_FILE` 改路径），toast 直接报该路径；`GROK_CLIPBOARD_NO_OSC52` 是 OSC 52 的一键关停（`clipboard/mod.rs:471-541`、`:441-458`）。
10. OSC 8 是否发射由「品牌原生支持 + skip_reason 为空」共同决定，skip 顺序为 apple_terminal → unsupported_terminal → vte_old → unknown_terminal → screen → tmux_old(<3.4)（`hyperlink_route.rs:28-40`、`terminal/mod.rs:440-468`）。
11. OSC 8 序列在帧差分阶段发射（`\x1b]8;id=N;url\x07`），`id=` 会被重铸成进程内单调 id，以便折行链接在终端侧被识别为同一条（`xai-ratatui-inline/src/terminal.rs:55-92`、`:343-380`）。
12. 鼠标捕获是进程级布尔位：minimal 模式与「原生选择持有期」会临时关闭并回滚，opt-in 的 `/toggle-mouse-reporting` 与 `Ctrl+R` 是同一 Action；Windows 还需额外断言 QuickEdit 模式（`app/mod.rs:134-135`、`app/mode_switch.rs:114-128`、`app/app_view.rs:4287-4315`、`actions/defaults.rs:858-873`）。
