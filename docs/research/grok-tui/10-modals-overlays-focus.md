# 专题 10：Grok Build 的模态、浮层与焦点栈

> 专题：主题 10 模态、浮层与焦点栈｜调研日期：2026-09-22｜证据范围：`crates/codegen/xai-grok-pager/src/views/`（`modal.rs`、`modal_window.rs`、`overlay.rs`、`overlay_list.rs`、`picker.rs`、`btw_overlay.rs`、`shortcuts_help.rs`、`block_viewer/mod.rs`、`settings_modal/{render.rs,state.rs}`、`session_picker.rs`、`memory_modal.rs`、`extensions_modal.rs`、`agents_modal.rs`、`feedback_modal/render.rs`、`usage_modal.rs`、`jump.rs`、`rewind.rs`、`new_worktree_dialog.rs`、`managed_connectors_wait.rs`、`file_search/line_viewer.rs`）、`src/app/`（`modals.rs`、`app_view.rs`、`agent_view/{key_owner.rs,input.rs,render.rs,subagent_takeover.rs}`）、`src/actions/{defaults.rs,mod.rs}`、`crates/codegen/xai-grok-pager-render/src/{modal_window_state.rs,render/scrollbar.rs,render/color.rs,appearance/config.rs}`。只读 grep/sed 阅读，未编译。

## 1. 一句话总览

这里没有"浮层栈"这个对象。取而代之的是：**一个 `Option<ActiveModal>` 单槽位承载真正的模态弹窗**（`src/views/modal.rs:180` 起），**一组布尔槽位承载全屏接管与浮层**（`AgentView` 的字段），以及**靠 `if … return` 分支顺序定义的 z 序**（`src/app/agent_view/render.rs`）。焦点不用栈来表示，而用一张显式的**优先级排名表** `KeyOwner`（`src/app/agent_view/key_owner.rs:42`）加一个**ESC 阶梯枚举** `EscStep`（同文件 `:59`）。

```
                     ┌─ 谁的 if 分支在前，谁就在上层 ─┐
  优先级高 ┌───────────────────────────────────────────────────────┐
  (z 序前) │ active_modal      → 模态弹窗（命令面板/设置/扩展/…）  │ render.rs:3211
           │ line_viewer       → 计划预览、文件预览（圆角框 + 半透明蒙层）│ :3244
           │ image_viewer      → 图片查看器                         │ :3430
           │ video_viewer      → 视频查看器                         │ :3600
           │ gboom             → /gboom 动画                       │ :3643
           │ block_viewer      → 块全屏查看（Ctrl-F）              │ :3702
           │ agents_modal      → /config-agents                    │ :3894
           │   └ persona_detail（同一分支内后画 = 叠在其上）       │ :3911
           │ feedback_modal    → /feedback                         │ :3922
           │ extensions_modal  → /hooks /plugins …                 │ :3940
           │ goal_detail       → 目标详情浮层                      │ :4239
  优先级低 │ workflows         → 工作流浮层                        │ :4259
  (z 序后) └───────────────────────────────────────────────────────┘
  更高一层（在这套渲染之前就 return，见 §4.1）：子代理全屏接管 intercept_takeover_input
```

关键点：这些分支**每一个画完就 `return`**（例：`render.rs:3211-3215`、`:3919-3920`、`:3937-3938`），所以它们实际上是**互斥**的，分支顺序只是"同时为真时谁赢"。真正意义上的"叠"只发生在两条路径上：同一分支内先后两次绘制（agents_modal → persona_detail），以及同一个 popup 内换内容（Settings → ResetSettingsConfirm）。

## 2. 五种浮层形态，五种不同容器

| 形态 | 容器 | 是否遮住底层 | 代表 |
| --- | --- | --- | --- |
| 模态弹窗（ModalWindow） | 屏幕居中矩形，`Clear` 清底 | 像素级覆盖，**不做 dim** | 命令面板、设置、扩展、快捷键表（`views/modal_window.rs:236`） |
| prompt 槽位接管 | 直接占用 `layout.prompt` 那块矩形 | 完全替换输入框，不浮在上面 | permission / question / elicitation 卡片、`/rewind`、`/jump`、cancel-turn（`app/agent_view/render.rs:943-965, 2781-2794`） |
| 全屏接管 | 整屏或除 shortcuts bar 外全部 | 是，且多数自带蒙层或全清 | block_viewer、line_viewer、图片/视频、子代理全屏（`render.rs:3702`、`:3244`） |
| 内嵌浮层（在同一 popup 内部） | 复用宿主 popup 的矩形 | 覆盖宿主内容区 | ResetConfirmOverlay、ManagedConnectorsWait、memory 全屏变体 |
| 下拉/横幅 | prompt 上方或紧邻的一行/几行 | 否 | slash_dropdown、completion_dropdown、file_search、privacy_banner、btw_overlay |

`/btw` 是特例：它**不是 popup，而是布局里的一行槽位**。`btw_height` 被算进 `AgentViewLayoutParams`（`app/agent_view/render.rs:1089-1090, 1123`；`views/agent.rs:120, 247-249`），面板高度由内容决定（`views/btw_overlay.rs:182-200`：`Loading` 恒 3 行，`Done` 为 `2 + clamp(行数, 1, DONE_MAX_BODY_LINES)`，`Error` 为 `2 + 折行后的错误行数`）。

## 3. 模态窗口（ModalWindow）的视觉规范

一切有边框的弹窗都走同一个函数 `render_modal_window`（`views/modal_window.rs:236`），配置结构是 `ModalWindowConfig`（`:76-87`：`title` / `tabs` / `shortcuts` / `sizing` / `fold_info`），持久状态是纯数据类型 `ModalWindowState`，放在表现层 crate 里以免下层依赖 views 层（`crates/codegen/xai-grok-pager-render/src/modal_window_state.rs:1-4`）。

### 3.1 尺寸计算

`ModalSizing` 七个字段（`modal_window.rs:89-121`）：

| 字段 | 默认 | 语义 |
| --- | --- | --- |
| `width_pct` | `0.9` | 屏宽比例 |
| `max_width` | `140` | 宽度上限（列） |
| `min_width` | `60` | 宽度下限（列） |
| `v_margin` | `7` | 上下各留的行数 |
| `h_pad` | `2` | 内容左右各内缩列数 |
| `v_pad` | `2` | 内容上方内缩行数（**下方不生效，底部归 footer**） |
| `footer_lines` | `2` | 底部快捷键行数下限 |

实际尺寸（`compute_modal_dims`，`modal_window.rs:575-586`）：

```rust
let max_width = area.width.saturating_sub(4).min(sizing.max_width);
let preferred_width = (area.width as f32 * sizing.width_pct) as u16;
let modal_width = preferred_width.min(max_width).max(sizing.min_width).min(area.width);
let modal_height = area.height.saturating_sub(sizing.v_margin * 2);
```

注意顺序：`min_width` 可以顶到 `max_width_eff` 之上，最后再被 `area.width` 收回（注释解释这是为了不让 `Clear`/`Block` 越界，`:579`）。高度**没有下限也没有上限**，只减两个 `v_margin`。

**两道拒绝渲染闸门**：`modal_width < 20 || modal_height < 6` 时直接返回 `None`，并清空 `popup_area`/`close_button_rect`/`shortcut_hits`/`tab_rects`，避免鼠标命中上一帧的陈旧矩形（`modal_window.rs:253-260`，`state.popup_area = None` 的注释在 `:254`）。

三档预设：`default()` = `large()`（0.9 / 140 / 60 / v_margin 7 / h_pad 2 / v_pad 2 / footer 2，`:112-120, 139-141`）；`medium()` = 0.60 / 120 / 44 / 4 / 2 / 1 / 2（`:124-133`）。`with_compact(true)` 把 `v_margin→0`、`h_pad→1`、`v_pad→0`，注释写明目标是"把每个 popup 的可用内容区最大化"（`:146-157`）。

### 3.2 居中与贴底

**水平、垂直都居中**：`x = area.x + (area.width - modal_width) / 2`，`y = area.y + (area.height - modal_height) / 2`（`modal_window.rs:262-271`）。"贴底"的效果由 `v_margin` 决定：默认上下各留 7 行，`with_compact` 下为 0，于是弹窗几乎顶到终端上下边（`:148-149` 注释原话）。**页脚贴的是 popup 内底**，不是屏幕底：`footer_y = inner.y + inner.height - footer_height`（`:381`）。

```
area = 整屏 (W × H)，以 W=100 H=32、ModalSizing::default() 为例
modal_width  = min(100*0.9=90, min(100-4,140)=96) = 90
modal_height = 32 - 7*2 = 18
x = (100-90)/2 = 5      y = (32-18)/2 = 7
col: 0    5                                      94    99
row 0  ┌──────────────────── 上边距 7 行 ────────────────────────┐
row 7  │    ┌──────────────────────────────────────────────┐     │
row 8  │    │ ─ Title ─                            [✗]    │     │  ← 边框行，关闭按钮嵌在上边框
row 9  │    ├──────────────────────────────────────────────┤     │  ← 可选 tab bar 在此
row 10 │    │ ──────────────────────────────────────────── │     │  ← 全宽分隔线
row 11 │    │                                              │     │
       │    │   content（左右各内缩 h_pad=2）              │     │
       │    │                                          ║   │     │  ← 滚动条占 content 最右一列
row 23 │    │  ↑/↓ nav  |  Enter select  |  Esc close      │     │  ← footer 贴内底，水平居中
row 24 │    └──────────────────────────────────────────────┘     │
row 25 └──────────────────── 下边距 7 行 ────────────────────────┘
        x=0  x=5                                          x=94
```

### 3.3 边框、标题、关闭按钮

- **边框**：`Block::default().borders(Borders::ALL)`，未指定 `border_type`（即默认直角），`border_style` = `fg(gray_dim)` + `bg(bg_base)`，整体块样式 `bg(bg_base) fg(text_primary)`（`modal_window.rs:311-314`）。模块注释自称 "accent-colored square border"（`:6`），但代码用的是 `gray_dim`——**以代码为准**。
- **标题画在上边框线上**，装饰破折号用边框色、标题字用 `text_primary + BOLD`：`Span("─ ") + Span(title) + Span(" ─")`（`:277-282, 315-322`）。`title` 为空串则整条上边框连续无装饰（`:310` 注释）。
- **关闭按钮** `[✗]` 是 5 个 cell 的定宽串 `[" ", "[", ballot_x(), "]", " "]`，放在 `x = modal_area.x + modal_area.width - (5 + 2)` 的第 0 行，即上边框、距右边框 2 列（`render_close_button`，`:405-436`）。悬停时只把中间三个非空格 cell 改成 `text_primary + BOLD`，两侧 padding 保持原样（`:426-432`）；命中矩形存进 `state.close_button_rect`（`:435`）。

### 3.4 Tab bar（可选）

`config.tabs` 为 `Some` 时才画（`modal_window.rs:330-352`）。左缩进 2 列（`left_margin = 2`，`:451`），标签间隔两个空格（`:457`），**按显示宽度贪心折行**（`:460-478`），放不下的行整行跳过并把对应 `tab_rects` 置 `None`（`:485-493`）。样式三态（`:508-531`）：选中且 tab 区有键盘焦点 → `text_primary on bg_visual + BOLD`；选中但无焦点 → `accent_user + BOLD`；未选中 → `gray`（注释解释了为何不用 `gray_dim`：ANSI16 下对比度会掉到 ~1.2:1，`:515-517`）。tab bar 下方必画一条与内宽等长的 `─` 分隔线（`:337-348`），**并且当存在 tab 时内容区的 `v_pad` 被强制为 0**，因为分隔线已经充当了间距（`:354-360`）。

### 3.5 内容区与页脚

```rust
content_top    = inner.y + effective_v_pad + tab_bar_height + tab_divider_height
content_height = inner.height - (effective_v_pad + tab_bar_height + tab_divider_height + footer_lines)
content_area   = { x: inner.x + h_pad, width: inner.width - h_pad*2, … }
footer_y       = inner.y + inner.height - footer_height
```

（`modal_window.rs:362-387`。）`footer_lines` 先取 `sizing.footer_lines`，但若快捷键在当前宽度下放不下就**按贪心折行的预测值抬高**（`:363-365`，`shortcuts_rows_needed` 在 `:591-614`；`predict_shortcut_rows` 在 `:560-573` 复用同一算法，注释强调"预测必须和实际渲染一致"）。页脚行**底部对齐**（`y = area.y + area.height - num_rows + row_idx`，`:671`），每行**水平居中**（`:679-683`），行内快捷键用 `"  |  "` 分隔（`:639`）。单个快捷键按第一个 ASCII 空格切成"键名 + 说明"两段：键名 `text_secondary + BOLD`，说明走 `theme.muted()`（`:616-623, 716-734`）。所有快捷键——包括 `clickable: false` 的纯提示——都产出一条 `ShortcutHitArea`，用于悬停高亮；只有 `clickable: true` 的才会派发 `ShortcutActivated`（`:751-761`，`modal_window_state.rs:67-80`）。

`/settings` 与 how-to 指南还多一层 tip 行：`split_content_for_tip_footer` 在内容高度 ≥ 3 时切出最后一行，高度 ≥ 6 时再空一行（`modal_window.rs:823-839`），tip 文本右对齐居中绘制（`:798-821`），对应地 `footer_lines_with_tip_gap` 把页脚预留成 `预测行数 + 1` 且至少 2 行（`:841-850`）。使用者见 `views/settings_modal/render.rs:118-131`。

### 3.6 遮罩：三种互不相同的机制

Grok Build **没有统一的 dim 遮罩**。实际存在三种：

1. **`Clear` 清底（主流）**：`Clear.render(modal_area, buf)`，只把弹窗覆盖的 cell 重置，弹窗外内容**原样保留、不变暗**（`modal_window.rs:274-275`）。所有 ModalWindow 弹窗都是这一档。
2. **半透明蒙层（少数）**：全屏级浮层用 `dim_area` / `fade_region` 把底色混进去。例如 block_viewer 的窗口在画之前先 `dim_area(buf, overlay_area, theme.bg_base, 0.5)`（`app/agent_view/render.rs:3779`），line_viewer 同样有一条 `should_dim` 分支（`views/file_search/line_viewer.rs:1439, 1465`，注释明确"全屏时跳过 dim"）。混合公式是逐通道线性插值（`crates/codegen/xai-grok-pager-render/src/render/color.rs:152-157`）。
3. **失焦"退后"（recede）**：这是 *焦点* 反馈而非层级遮罩。`ListOverlay` 在 `focused == false` 时把整个区域朝面板底色混合 0.66（`views/overlay_list.rs:152-155`）。`PickerMode` 的文档也把 dim 绑在模式上：`Floating` = "dimmed background and rounded border"，`FullScreen` = "no border, no dim"（`views/picker.rs:104-113`）。

`ModalWindowState` 里没有任何 dim/alpha 字段（`modal_window_state.rs:11-34`），也从侧面印证第 1 档才是主流。

### 3.7 滚动条

列表型模态的滚动条来自 `tui-scrollbar`，在 `content_area` 最右列、宽度 1、高度等于可视行数（`views/picker.rs:2084-2098`）：

```rust
let sb_x = scrollbar_x_override.unwrap_or(content_area.x + content_area.width - 1);
let sb_area = Rect::new(sb_x, content_area.y, 1, max_visible as u16);
render_scrollbar_styled(buf, Some(sb_area), total_visual_rows, max_visible, scroll_visual,
                        Style::default().bg(...), Style::default().fg(theme.gray_dim).bg(...));
```

`render_scrollbar_styled` 的签名参数依次是 `total_lines / viewport_lines / offset`，并在 `SCROLLBARS_HIDDEN` 为真时整体变成 no-op（`render/scrollbar.rs:211-229`）。minimal（scrollback 原生）模式启动时把它置真，让所有列表都不画滚动条，改由页脚承担 "↑/↓ navigate" 提示（`render/scrollbar.rs:44-48`）。滚动窗口策略是**居中跟随选中项**：`compute_scroll_offset` 取 `selected - visible/2` 并夹到 `max_scroll`（`views/picker.rs:151-169`），鼠标滚动则写 `scroll_offset`，一旦键盘导航就把它重置为 `None` 交还自动策略（`views/picker.rs:1487-1490, 2004-2012`）。

### 3.8 embedded（minimal 模式）变体

一个进程级 `AtomicBool` 把整个模态框切成无边框形态：`EMBEDDED`，由 minimal 模式在终端初始化时置位，注释明确"minimal 模式永远不会显示浮动模态框"（`modal_window.rs:25-40`）。embedded 下 `modal_width/height = area`、不画边框、不画关闭按钮、不画 `Clear` 以外的 chrome，标题改成第一行的裸加粗文本（`:246-251, 284-307`）；行选中态也从"画色带"改成"改前景为 `fuzzy_accent`"（`embedded_row_style`，`:64-73`）。

### 3.9 一个"非 ModalWindow"的对照样本

`new_worktree_dialog.rs` 没走 ModalWindow，而是用 ratatui 的 `Flex::Center` 手写居中：`MIN_DIALOG_WIDTH = 50`、`DIALOG_HEIGHT = 5`、内部左右 padding 4（`views/new_worktree_dialog.rs:11-15`），并且**小到放不下时会画一行 `[Esc] to close` 作为降级提示**而不是静默消失（`:18-31`）。这是少数做了"拒绝渲染但给出可退出线索"的浮层。

## 4. 浮层的叠加、优先级与嵌套

### 4.1 渲染层：分支顺序即 z 序

唯一"栈式"的地方是 `AgentView::intercept_takeover_input`（`app/agent_view/subagent_takeover.rs:347`）：子代理全屏接管**在所有其他输入处理之前**跑（`app/agent_view/input.rs:361-363`），非空时直接返回，等价于压在整棵渲染树之上。`key_owner.rs:38-40` 的注释也承认这一点："这些全屏接管（子代理视图、媒体查看器、`/gboom`、模态栈）自己回答输入，因此不参与排名。"

其余浮层的相对顺序就是 §1 图里的 `if` 分支顺序（`render.rs:3211 → 3244 → 3430 → 3600 → 3643 → 3702 → 3894 → 3922 → 3940 → 4239 → 4259`），每个分支画完即 `return`。

### 4.2 prompt 槽位的接管优先级链（更细的一层）

卡片与列表浮层不画在屏幕上，而是**抢占输入框那块矩形**。抢占顺序在高度计算处就写死了（`render.rs:806-880`）：

```
priority (高→低)          判定条件                                    证据
permission card            slot_card == Permission                     :817-829
question card              slot_card == Question                       :830-842
elicitation card           slot_card == McpElicitation                 :843-855
/rewind 浮层               上面三个高度都为 0 且 rewind_state 存在      :856-865
cancel-turn 面板           slot_card == CancelTurn 且 rewind 为 0       :866-871
/jump 浮层                 jump_slot_taken() 为假                       :872-880
```

`prompt_height` 用同一顺序取第一个非 0 的高度（`:943-965`），绘制端也用同一顺序 `if … else if …`（`:2378 → 2460 → 2772 → 2781 → 2790 → 2794`）。**这套"高度求解与绘制必须同序"的写法**保证了不会出现"预留了行数但没人画"。另外槽位接管有全局上限：`max_prompt_height = area.height / 2`（`:806`）。

### 4.3 排队：只有 permission 是真队列

`permission_queue: VecDeque<PermissionViewState>`（`app/agent_view/mod.rs:1327`，初始化 `app/agent_view/session.rs:383`），所有读写都只碰 `front()` / `front_mut()`（`key_owner.rs:96-108, 155-159`）。所以多个授权请求是 **FIFO 排队**，一次只显示一个。其余模态/浮层都是"后开覆盖先开"的单槽位语义，没有队列。

### 4.4 嵌套的三种实现手法

| 手法 | 例子 | 证据 |
| --- | --- | --- |
| 同一 popup 内换内容（状态搬迁） | `ResetSettingsConfirm` 把 `SettingsModalState` 整体搬进自己的枚举变体，"叠"在设置模态上；取消时再搬回去，从而保留过滤/滚动位置 | `views/modal.rs:304-313`；绘制 `app/modals.rs:2376-2400`（同一 popup，只把内容区换成确认行） |
| 后画覆盖前画 | `/config-agents` 画完 agents_modal 后，若 `persona_detail` 存在就再画一层 | `render.rs:3894-3918` |
| 快照 + 回退 | 命令面板 → 参数选择、DocPicker → DocViewer 都把上层状态存进 `previous_palette: Option<PaletteSnapshot>`，Esc 时还原 | `views/modal.rs:331-335, 243-245`；`app/modals.rs:174-180`（DocPicker）、`:278-282`（DocViewer 的注释"Esc in DocViewer goes back to the DocPicker list"） |
| 宿主内部局部浮层 | 扩展模态里的 ManagedConnectorsWait "盖住列表"；`overlay_content_rect` 给出"距 popup 左右各 1 列、从 tab bar 下两行到底部"的内嵌全窗矩形 | `views/managed_connectors_wait.rs:1-5, 126`；`modal_window.rs:1076-1093` |

**不能嵌套的是"同层再来一个模态"**：`active_modal` 只有一个槽位，打开新模态就是覆盖旧的（例：`Ctrl+P` 打开命令面板时直接 `self.active_modal = Some(...)`，`app/agent_view/input.rs:1127`）。

## 5. 焦点栈与键盘路由

### 5.1 `KeyOwner`：一张显式排名表

```rust
// app/agent_view/key_owner.rs:38-53（节选注释）
/// Who the keyboard reaches, in the order [`AgentView::handle_input`] asks for it.
pub(crate) enum KeyOwner { LineViewer, BlockViewer, Card(BlockingCard), PlanApproval, Pane }
```

排名实现在 `key_owner_when_parked`（`:110-132`），逐条 `if/else if`：

1. `Permission` 卡片（**永远最高**，注释："a permission request interrupts whatever else is waiting"，`:96-98`）
2. `LineViewer`
3. `BlockViewer`
4. `PlanApproval`
5. 其余卡片：`CancelTurn` → `Question` → `McpElicitation`（`blocking_card` 的固定优先级，`:96-108`）
6. `Pane`（普通面板 + 全局绑定）

`parked` 这个布尔量（键盘停在 scrollback）会把卡片与 plan approval 摘出排名，使得"卡片还画着但键盘在滚动区"成为一个可表达状态（`:114-116, 142-153`）。

### 5.2 `EscStep`：ESC 是一级一级的阶梯，不是开关

`EscStep`（`key_owner.rs:59-91`）把 ESC 拆成 8 个可枚举档位，并且**每一档都有给快捷键栏显示的用户可见标签**（`label()`，`:78-91`）：

| 档位 | 含义 | 用户可见标签 |
| --- | --- | --- |
| `DismissFileSearch` | 关掉卡片输入框上的 `@` 文件搜索下拉 | `dismiss` |
| `LeaveTextInput` | 从卡片文本框退回其选项行 | `back` |
| `DiscardPatternEdit` | 丢弃半成品的 always-allow 规则编辑 | `cancel` |
| `ClearSelection` | 取消当前问题的选择 | `unselect` |
| `BackOutOverlay` | 退回 dashboard（卡片保持挂起） | `dashboard` |
| `ParkFocus` | 把键盘交给滚动区，卡片继续画着 | `scrollback` |
| `KeepRunning` | 关掉 cancel-turn 面板，回合与子代理继续跑 | `keep running` |
| `DismissElicitWaiting` | 关掉 elicitation 的等待态 | `dismiss` |

设计意图写在注释里：**"每个在卡片上标注 Esc 的提示都读这张表，每个拥有该键的处理函数也都按它分发，因此快捷键栏不可能承诺一个按键到不了的档位"**（`:55-57`）。`card_esc()` 根据当前焦点算出该走哪一档，`handle_card_esc()` 执行（`:167-206, 208-250`）；`KeepRunning` 的实现特意不用 `ContinueToRun`，因为那仍会取消父回合（`:241-244`）。

### 5.3 输入派发顺序

`handle_input_inner`（`app/agent_view/input.rs:329`）是一条很长的"先匹配先返回"链。按代码顺序：

```
scrollback drag latch (:335)       ─ 拖拽中的鼠标事件先被吞掉
子代理全屏接管 (:361)              ─ intercept_takeover_input
/jump 抑制期的 Esc 吞掉 (:364)
wrap-host 图片粘贴 (:372, :378)
Esc+d 组合 → DumpInputLog (:386)
image_viewer (:399) / video_viewer (:428) / gboom (:439)
workflows 浮层 (:454) / goal_detail (:457)
active_modal (:488)  ←── 模态弹窗在这里，早于 btw、早于卡片
/btw 面板 (:501 起：Esc → dismiss、关闭按钮、悬停、滚动)
… 之后才是扩展模态 (:642)、persona_detail (:646)、agents_modal (:663)、block_viewer (:681)
key_owner() == PlanApproval (:800)、Permission 卡片 (:725) …
```

**路由到最上层的方式**：每一层都返回 `InputOutcome`，`Changed`/`Action` 表示已消费，`Unchanged` 表示"我没吃这个键"。模态弹窗层的两个例外值得记：

- `active_modal` 分支**只放行 `ActionId::Quit`**（`registry.lookup(key, When::Always) == Some(ActionId::Quit) → Unchanged`），其余按键全部交给模态自己（`:490-495`）。也就是说 Ctrl+Q 是唯一能穿透模态的全局键。
- `extensions_modal` / `persona_detail` / `agents_modal` / `block_viewer` 则更宽：**任何 `When::Always` 的全局动作都放行**（`:644-646, 658-660, 678-680, 686-688`）。`views/extensions_modal.rs:3` 的模块注释说它 "Blocks all input until closed with Esc"，严格说只在非全局键上成立。

模态窗口自身的键盘处理极小：`handle_modal_key` 只认 `Esc → CloseRequested`、`Left/h`、`Right/l`（后两者在 `tabs_focused` 时直接 `Unhandled`，否则按 `fold_info` 返回折叠类结果），其余一律 `Unhandled` 交给各模态内容处理器（`modal_window.rs:899-942`）。**Tab 不由 chrome 处理**，而是各模态/Picker 自己实现（见下）。

### 5.4 模态内的焦点循环

以 picker 系模态为样本，模态内有三个焦点区：**tab bar / 搜索框 / 列表**，用 `PickerState` 的两个布尔量表达（`search_active`、`tabs_focused`，`views/picker.rs:1474-1503`）。

```
        ┌────────── Tab / Shift+Tab（非搜索态）→ 循环 ──────────┐
        │                                                      │
   ┌────────┐   Down/Enter   ┌──────────┐   Down   ┌────────┐  │
   │ tab bar │ ────────────► │ 搜索框   │ ───────► │  列表  │ ─┘
   │tabs_foc│ ◄──────────── │search_act│ ◄─────── │selected│
   └────────┘   Up（列表首行或搜索框）  └──────────┘   Up     └────────┘
        Left/Right 或 h/l 换 tab（仅 tabs_focused 时）
```

- 搜索框内按 `Tab`/`BackTab`（或 Shift+Tab）也换 tab（`views/picker.rs:2693-2703`）。
- 搜索框内按 `Up`：若配置了 tab 就把焦点交给 tab bar，否则跳到列表最后一项；按 `Down` 跳到"第一个可选项"（`:2717-2746`）。
- `tabs_focused` 状态下 `Down`/`j` 回到搜索框，完成"列表底 → tab → 搜索 → 列表顶"的环形（`:2762-2770`）。
- 列表底的 `Down` 与列表顶的 `Up` 把焦点交回搜索框/ tab bar，注释直接称之为 "completing the arrow cycle"（`:2758-2762`）。
- `Left`/`Right` 在 `tabs_focused` 时按 `(active_tab ± 1) % tab_count` 环绕（`:2965-2980`）。
- 列表本身的行导航有 `j`/`k` 别名（条件 `config.show_search_hint || config.vim_normal_first`，`:2755-2757`）。

vom 模式下 `i` / `/` 是唯一进入搜索态的方式，普通可打印字符不输入（`views/picker.rs:2994-3001`）；`push_vim_nav_search_hint` 会在页脚补一条 `i search`，非导航态或 vim 关闭时不加（`modal_window.rs:186-197`）。

### 5.5 输入捕获的其它规则

- **粘贴**：`Event::Paste` 按同一个优先级链单独分发（`handle_modal_paste`，`input.rs:497`）；卡片在 `FollowupInput` 焦点下把粘贴路由给输入框，在 `PatternEdit` 下逐字符插入并过滤 `\n\r`（`input.rs:787-802`）。
- **鼠标**：`handle_modal_mouse` 先判关闭按钮 → tab → 可点击页脚 → **点在 popup 之外直接 `CloseRequested`（点击穿透到背景即关闭）**（`modal_window.rs:981-1002`）；`Moved` 只更新悬停并返回 `Handled`，最后落到 `Unhandled` 交给内容（`:1003-1026`）。
- **Ctrl+Q 永远保留**：出现在几乎每一层的最前面（`input.rs:402, 431, 445, 492, 731`），是"卡在某一层出不来"的最后退路。

## 6. 模态与浮层类型清单

| 名称 | 文件 | 用途 | 触发方式 |
| --- | --- | --- | --- |
| `ActiveModal::CommandPalette` | `views/modal.rs:187-193` | 模糊搜索所有动作与 slash 命令 | `Ctrl+P` / `?`（scrollback 焦点）(`actions/defaults.rs:790-799`) |
| `ActiveModal::ArgPicker` | `views/modal.rs:194-212` | 命令参数选择（model、theme、reasoning effort） | 从命令面板选中带参数的命令 |
| `ActiveModal::SessionPicker` | `views/modal.rs:213-246` | `/resume` 会话选择，支持深度内容搜索与 `d` 删除 | `/resume`（`app/agent_view/input.rs:1344`） |
| `ActiveModal::DocPicker` / `DocViewer` | `views/modal.rs:247-271` | how-to 指南列表 / 正文阅读 | 命令面板的 HowTo 项 |
| `ActiveModal::ShortcutsHelp` | `views/modal.rs:272-290` | 按键速查表（可过滤、可展开、可进详情页） | `Ctrl+.` 或 `Ctrl+X`（`actions/defaults.rs:805-817`） |
| `ActiveModal::MemoryBrowser` | `views/modal.rs:291-294` | `/memory` 浏览器，左列表右预览，可全屏 | `/memory` |
| `ActiveModal::Settings` | `views/modal.rs:295-298` | 设置模态（F2） | `F2` / `Ctrl+,` / `Ctrl+,`(SUPER) / `/settings`（`actions/defaults.rs:843-851`） |
| `ActiveModal::UsageInfo` | `views/modal.rs:299-303` | 用量与会话信息（多 tab） | `/usage`、`/session-info`、`/context`、context-bar 点击（`views/usage_modal.rs:1`） |
| `ActiveModal::ResetSettingsConfirm` | `views/modal.rs:304-313` | 叠在设置模态上的"恢复默认"确认 | 设置里对某项按重置 |
| `ActiveModal::RememberNoteReview` | `views/modal.rs:314-330` | `#` 记忆笔记的预览（Tab 切原始/增强） | 提交 `#` 笔记 |
| `ActiveModal::EditConfirm` | `views/modal.rs:181-186` | 放弃未保存的队列编辑（save/discard/delete） | **已不再布防**，绘制端无分支（`app/modals.rs:1718-1719`） |
| Extensions 模态 | `views/extensions_modal.rs` | Hooks/Plugins/Marketplace/Skills/Workflows/MCP 六个 tab | `/hooks`、`/plugins` 等（`views/extensions_modal.rs:1-4`） |
| Agents 模态 | `views/agents_modal.rs` | 列出所有 agent 定义 | `/config-agents`（别名 `/agents`）（`views/agents_modal.rs:1-4`） |
| Persona detail | `views/persona_detail.rs` | persona 详情/内联编辑，**叠在 Agents 模态之上** | Personas tab 上按 Enter（`views/persona_detail.rs:1-4`） |
| Feedback 模态 | `views/feedback_modal/` | 反馈编辑器，用同一套 modal chrome | `/feedback`（`actions/defaults.rs` 的 `OpenFeedbackModal` 路径） |
| Import Claude 模态 | `views/import_claude_modal.rs` | 选择性导入 Claude 设置 | `/import-claude` 或欢迎页按 `i`（`views/import_claude_modal.rs:1-4`） |
| Tutorial 浮层 | `views/tutorial.rs` | 上手教程，两屏 | `/tutorial`（`views/tutorial.rs:1-3`） |
| Goal detail | `views/goal_detail.rs` | 目标进度浮层（预算条 + todo + 事件史） | 目标 chip；`Esc` 或 `g` 关闭（`views/goal_detail.rs:1-3`） |
| Workflows 浮层 | `views/workflows.rs` | 工作流运行列表 | workflows overlay 输入路径（`app/agent_view/input.rs:454`） |
| Block viewer | `views/block_viewer/` | 滚动区块的全屏查看（含搜索、可视选择、复制） | 选中块按 `Ctrl-F`（`views/block_viewer/mod.rs:1-4`） |
| Line viewer | `views/file_search/line_viewer.rs` | 文件/计划预览弹窗，带 `[↗][✗]` 按钮与半透明蒙层 | 计划审批预览、prompt 上的文件预览 |
| Permission 卡片 | `views/permission_view.rs` | 工具授权（选项列表 + followup 输入 + 规则编辑） | 代理请求授权时 |
| Question 卡片 | `views/question_view.rs` | `AskUserQuestion` 结构化提问 | 代理提问时 |
| Elicitation 卡片 | `views/elicitation_view/` | MCP URL/form elicitation | `x.ai/mcp/elicit` |
| Plan approval | `views/plan_approval_view.rs` | 计划审批（预览关闭时占键盘） | plan 模式下提交计划 |
| Cancel-turn 面板 | `views/modal.rs`（`render_cancel_turn_panel`） | 回合运行中询问是否中断 | 回合运行中按取消 |
| `/rewind` 浮层 | `views/rewind.rs:321` | 回退到某回合（Picker → CancelOffer → Confirm → Executing） | `/rewind`，或空闲空输入框下 800ms 内 `Esc Esc`（`actions/defaults.rs:427-436`） |
| `/jump` 浮层 | `views/jump.rs:113` | 跳到某个回合，光标移动即预览 | `/jump` |
| `/btw` 面板 | `views/btw_overlay.rs:205` | 侧问侧答，内容随 Esc 落回滚动区 | `/btw` |
| New worktree 对话框 | `views/new_worktree_dialog.rs:36` | 新 worktree 命名 | 命令面板 New Session in Worktree |
| Managed connectors wait | `views/managed_connectors_wait.rs:126` | 等用户完成连接器的浮层，**盖在扩展模态列表上** | MCP tab 的 Ctrl+O / URL 点击 |
| Jumpless 下拉/横幅 | `views/slash_dropdown.rs`、`views/completion_dropdown.rs`、`views/file_search/`、`views/privacy_banner.rs`、`views/announcements.rs` | 非模态的内联提示 | 输入 `/`、`@`、shell 命令或行程提示 |

`overlay.rs` 里的 `OverlayState` 是**另一类**东西：它服务于可切换的侧栏面板（tracing / todo / 后台任务），三态 `visible / focused / fullscreen`（`views/overlay.rs:46-54`）。语义为：快捷键进入时"显示并聚焦"；`Tab` 退全屏并失焦、焦点交回滚动区；`Space` 同上但焦点交给输入框；`Esc`/`q` "退一层嵌套"——全屏时先退全屏，否则整体隐藏；`Ctrl-F` 切换全屏，且**即使在输入条打开时也生效**（`views/overlay.rs:73-131, 135-142`）。`handle_overlay_key` 与 `handle_overlay_nav_key` 被刻意拆成两个函数，因为调用方需要先判断 `has_input_bar` 再决定是否处理导航键（`:133-156`）。

## 7. 关闭 / 取消 / 确认的交互约定

### 7.1 统一的"关掉"语义

- **`Esc` = 关一层**，且这个"一层"由 §5.2 的 `EscStep` 或模态自身的子模式状态决定。典型例子：设置模态在 `FilterFocused` / `PickingEnum` / `PickingGroup` / `EditingValue` 四个子模式下**先短路**，让子模式处理 Esc，只有顶层才走宿主的 `CloseRequested`（`app/modals.rs:399-431`）；MemoryBrowser 同理（`:369-397`）。
- **点击 popup 之外 = 关闭**（`modal_window.rs:996-1000`），点击 `[✗]` 也是同一个 `CloseRequested`（`:982-984`）。
- **关掉时清理命中矩形**：chrome 拒绝渲染或模态退出时都会重建/清空 hit 状态（`modal_window.rs:253-258`、`settings_modal/render.rs:147-155` 的 `reset_hit_rects`），避免鼠标点到上一帧的幽灵矩形。
- **Esc 在搜索框里的第一跳不是关闭**：命令面板与 DocPicker 的 Esc 会先清空查询（`esc_clears` 标志），只有查询已空才关（`app/modals.rs:130-172`）。

### 7.2 不可逆动作的差异化处理

Grok Build 用**四套不同重量**的机制，按代价递增：

1. **双次按键 + TTL（最轻）**：`requires_confirmation` 的动作第一次按下只装填 `PendingAction`，在 `PendingAction::TTL = 1000ms` 内再按一次才真正执行；快捷键栏显示 "press again to {label}"（`app/app_view.rs:460-476`；`actions/defaults.rs:786` 的长帮助原文："Exits the app. Requires confirmation: press twice in quick succession; a lone press is treated as a stray key and ignored."）。空闲态 `Esc Esc` 的窗口单独定义为 `ESC_DOUBLE_PRESS_TTL = 800ms`（`app_view.rs:475-476`，与 `actions/defaults.rs:436` 的说明一致）。
2. **装填 + `y`/`n` 二次确认（中）**：会话删除用 `PendingDelete { source, session_id, cwd }` 记住目标——注释特意解释"即使列表发生位移，`y` 仍绑到正确目标"（`views/session_picker.rs:57-64`）。`handle_pending_delete_key` 规定：`y` 确认、`n` 取消、**任何其它无修饰键先把装填清掉但仍继续处理该键**（`Disarmed`），并且只接受 `KeyEventKind::Press` 且无修饰（`:103-132`）。
3. **阻塞式确认对话框（重）**：`ModalConfirmation<R>` 把"键 → 结果"写进类型里，一次只允许匹配到的键产生结果（`views/modal.rs:14-44`）；`EditConfirmResult` 有 `Save/Discard/Delete/Cancel` 四态，且标签会随上下文变化（`save` vs `save & send`、`discard changes` vs `discard & send`，`:60-82`）。恢复默认设置的确认**叠在设置模态上**，并且 `Esc`、`F2`、`Ctrl+,`、`Cmd+,` 全部映射到 `Cancel` 而不是"关模态"（`app/modals.rs:439-459`），还明确注释"只有无修饰的按键才算：`Ctrl+Y` 不得触发 Reset"（`:451-452`）。
4. **把危险选项排除出默认路径**：`/rewind` 的 `RewindPhase` 里 `Confirm` 是一个**独立的阶段枚举值**而不是模态（`views/rewind.rs:57-78`），并把"Yes, and don't ask again"做成会写回设置的一等选项（`actions/defaults.rs:436` 的长帮助）。dashboard 覆盖层的 Ctrl+X 也有自己的一致性处理：**自动重复（按住键）不得驱动破坏性动作**（`views/dashboard/state.rs:3338-3340`），并且它的确认窗口用 `CONFIRM_WINDOW` 而不是默认 TTL（`app/app_view.rs:480-483`）。

反面样本也值得记：`app/modals.rs:1718-1719` 留了一条注释——`EditConfirm` 已经没有绘制分支也不再被布防，理由是"布防它会不可见地捕获所有输入"。**一个没有渲染分支的模态等于键盘黑洞**，这是这套架构里最实用的一条防御性教训。

## 8. 小屏自适应

1. **两档屏幕高度阈值**（`views/agent.rs:91-97`）：`SHORT_TERMINAL_ROWS = 16`（硬砍：CTA 行、follow-up 行、外侧上下 padding 归零，`:205-213`），`AUTO_COMPACT_MAX_ROWS = 20`（`effective_compact` 强制 compact）。两者之间在编译期就断言了大小关系：`const _: () = assert!(SHORT_TERMINAL_ROWS < AUTO_COMPACT_MAX_ROWS);`（`:98`）。返回值**从不写回用户设置、渲染缓存或磁盘**，窗口变大即恢复用户选择；`terminal_rows == 0` 表示"尚未测量"，永不强制（`:99-104`）。
2. **compact 传导到模态**的方式是每个模态自己在 config 里调 `ModalSizing::with_compact(compact)`，把 `v_margin/h_pad/v_pad` 压到 0/1/0（`modal_window.rs:146-157`）。模态的 `compact` 参数统一取自 `self.scrollback.appearance().prompt.compact`（例：`app/modals.rs:1762, 2179, 2192, 2248`），而该字段本身是**派生渲染值**，注释明确"应用可以因终端过矮而强制打开，持久化的用户设置是 `UiConfig::compact_mode`"（`crates/codegen/xai-grok-pager-render/src/appearance/config.rs:76-80`）。
3. **放不下就不画，并且把 hit 状态清干净**：模态弹窗 `width < 20 || height < 6`（`modal_window.rs:253-260`）；`ListOverlay` 在 `height == 0 || width < 10` 时整体不画（`overlay_list.rs:56-58, 90-92`）；`render_btw_panel` 在 `width < 12 || height < 3` 时返回（`btw_overlay.rs:219-221`）；settings 在内容区小于 `CONTENT_MIN_WIDTH = 10` 时 `reset_hit_rects` 并返回（`settings_modal/render.rs:152-155`，常量在 `settings_modal/state.rs:28`）。
4. **小屏也有用户可用的"放大"入口**：memory 模态有全屏变体（`width_pct: 1.0, max_width: u16::MAX, v_margin: 0`，`views/memory_modal.rs:886-895`），并且这个偏好会持久化（`:253-254, 302`）；overlay 面板用 `Ctrl-F` 切全屏（`views/overlay.rs:114`）；line_viewer 有 `[↗]` 全屏按钮，全屏时**主动跳过 dim**（`views/file_search/line_viewer.rs:1439-1465, 1552-1600`）。
5. **小屏有专门的提示机制**：设置项 `contextual_hints.small_screen` 描述为"终端行数偏少时，每次运行提示一次 `/compact-mode`"（`settings/defs.rs:1489-1501`），配套 tip 在 `src/tips/small_screen.rs`。
6. **模态自身的高度弹性**：页脚行数会随宽度反向增长（窄了要多折行），并由 `shortcuts_rows_needed` 在布局前预测（`modal_window.rs:363-365, 591-614`）；tab bar 也在窄屏折成多行（`:438-478`）。

## 关键结论（供合稿引用）

1. 模态系统没有栈：真正的弹窗是一个 `Option<ActiveModal>` 单槽位（`views/modal.rs:180`），多浮层的"z 序"完全由 `app/agent_view/render.rs` 的 `if … return` 分支顺序定义（`:3211 → 3244 → 3430 → 3600 → 3643 → 3702 → 3894 → 3922 → 3940 → 4239 → 4259`）。
2. 所有带边框弹窗共用一套 chrome：`ModalWindowConfig` + `render_modal_window`（`views/modal_window.rs:76-87, 236`），尺寸由 7 个字段的 `ModalSizing` 决定，默认 0.9 / 140 / 60 / v_margin 7（`:89-121`），水平垂直双居中（`:262-271`）。
3. 两道"拒绝渲染"闸门保证小屏不会画出破碎的框：尺寸小于 20×6 直接返回 `None` 并清空全部鼠标命中矩形（`modal_window.rs:253-260`）；`with_compact` 在小屏把边距压到 `v_margin=0 / h_pad=1 / v_pad=0`（`:146-157`）。
4. 遮罩不统一：主流是 `Clear` 清底、**不 dim 背景**（`modal_window.rs:274-275`）；只有少数全屏浮层用 `dim_area(...,0.5)`（`app/agent_view/render.rs:3779`）或 `recede_area(...,0.66)` 做失焦退后（`views/overlay_list.rs:152-155`）。
5. 页脚是"底部对齐 + 水平居中 + `  |  ` 分隔"，键名加粗、说明用 muted；行数会按宽度贪心折行，并**先预测再布局**以免预留行数与实画不一致（`modal_window.rs:363-365, 591-614, 671-734`）。
6. 焦点不是栈而是排名表：`KeyOwner` 固定为 Permission > LineViewer > BlockViewer > PlanApproval > 其它卡片(CancelTurn/Question/Elicitation) > Pane（`app/agent_view/key_owner.rs:110-132`），排在更前的全屏接管（子代理/媒体/`/gboom`/模态）自行处理输入（`:38-40`）。
7. ESC 被建模成枚举阶梯 `EscStep`（8 档，每档带用户可见标签），并且"快捷键栏上的提示"与"按键实际行为"读同一张表，从机制上杜绝了提示与行为不一致（`key_owner.rs:55-91, 167-250`）。
8. 模态内焦点区（tab bar / 搜索框 / 列表）靠 `search_active` + `tabs_focused` 两个布尔量与方向键的"边界转移"实现环形循环，`Tab`/`Shift+Tab` 在非搜索态换 tab、在搜索态也换 tab（`views/picker.rs:2693-2703, 2717-2746, 2762-2770, 2965-2980`）。
9. 关键键穿透规则只有两条：模态层只放行 `ActionId::Quit`（`app/agent_view/input.rs:490-495`），而扩展模态/persona/agents/block_viewer 放行全部 `When::Always` 全局动作（`:644-688`）；`Ctrl+Q` 在每一层最前面被保留为最后退路。
10. 真正的"排队"只有 permission 队列：`permission_queue: VecDeque<PermissionViewState>`，所有判定只看 `front()`（`app/agent_view/mod.rs:1327`、`key_owner.rs:96-108`）；其余模态是覆盖式单槽位。
11. 嵌套靠三种手法而非栈：同一 popup 内换内容（`ResetSettingsConfirm` 搬移 `SettingsModalState`，`views/modal.rs:304-313`）、后画覆盖前画（agents_modal 之上的 persona_detail，`render.rs:3894-3918`）、快照回退（`previous_palette`，`app/modals.rs:174-180, 278-282`）。
12. 不可逆动作分四档处理：`PendingAction` 双次按键（TTL 1000ms，`app/app_view.rs:460-476`）、`PendingDelete` 的 `d`→`y`/`n` 装填确认（`views/session_picker.rs:57-132`）、阻塞式 `ModalConfirmation<R>` 对话框（`views/modal.rs:14-44`）、以及把确认做成独立阶段枚举（`RewindPhase::Confirm`，`views/rewind.rs:57-78`）；另有一条硬教训：没有渲染分支的模态（`EditConfirm`）必须停止布防，否则就是键盘黑洞（`app/modals.rs:1718-1719`）。

## 不确定项 / 未找到证据

- **`ModalWindow` 边框的"accent 色"说法**：模块注释写 "accent-colored square border"（`modal_window.rs:6`），但代码用的是 `theme.gray_dim`（`:278, 314`）。文档以代码为准；若合稿要引用视觉描述，建议写"灰色直角边框，标题加粗"。
- **`render_modal_window` 的 `Clear` 是否算遮罩**：代码只重置被覆盖的矩形，弹窗外像素不变（`:275`）。未找到"模态打开时整屏变暗"的证据，除非该浮层单独调用 `dim_area`。
- **`ActiveModal` 变体的完整数量**：`views/modal.rs:180-330` 共 12 个变体（`EditConfirm`、`CommandPalette`、`ArgPicker`、`SessionPicker`、`DocPicker`、`DocViewer`、`ShortcutsHelp`、`MemoryBrowser`、`Settings`、`UsageInfo`、`ResetSettingsConfirm`、`RememberNoteReview`）。未在 `src/views/` 之外找到另一个并行的"模态枚举"。
- **`extensions_modal` 的 `Enter`/提交类确认语义**：只核到它用 `ModalWindowState::with_tabs`（`views/extensions_modal.rs:1947`）与"Esc 关闭"（`:1-4`），其 tab 内部的具体确认流程在 8000 行文件里未逐条通读，未找到可引用的统一约定。
- **统一主题/常量表**：`modal_window.rs` 内的 `footer_lines`、`v_pad` 等没有集中常量，都是各模态各自字面量；`ModalSizing::medium()` 存在但未找到生产调用点（仅 `Default`/`large`/各模态内联字面量在用，grep `medium()` 于 `xai-grok-pager/src` 无命中）。
