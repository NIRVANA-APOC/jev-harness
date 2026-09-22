# 专题 19：审批、提问与确认类交互

> 专题：审批、提问与确认类交互｜调研日期：2026-09-22｜证据范围：`xai-grok-pager/src/views/{permission_view,plan_approval_view,question_view,rewind}.rs`、`src/views/elicitation_view/{mod,state,render}.rs`、`src/views/welcome/consent.rs`、`src/app/{consent,modals,app_view}.rs`、`src/app/agent_view/{interactions,plan,render,key_owner,input}.rs`、`src/app/dispatch/permissions.rs`、`src/app/acp_handler/permissions.rs`、`src/headless.rs`、`src/views/shortcuts_bar.rs`、`src/views/file_search/line_viewer.rs`、`xai-grok-pager-render/src/{appearance/permission_cursor.rs,input/key.rs,glyphs.rs}`、`xai-grok-workspace/src/permission/{prompter,grants,hub_permission}.rs`、`xai-grok-tools/src/implementations/grok_build/ask_user_question/{mod,format}.rs`、`xai-computer-hub-sdk/src/server.rs`、`docs/user-guide/{19-plan-mode,22-permissions-and-safety,07-mcp-servers}.md`

## 1. 需要用户裁决的交互有哪几类

Grok Build 把所有"等用户决定"的场景收敛成同一个抽象：**阻塞卡（blocking card）**。`blocking_card()` 是唯一的权威判定，且带固定优先级（`src/app/agent_view/key_owner.rs:96-105`）：

```rust
pub(crate) fn blocking_card(&self) -> Option<BlockingCard> {
    if !self.permission_queue.is_empty() {
        Some(BlockingCard::Permission)
    } else if self.cancel_turn_view.is_some() {
        Some(BlockingCard::CancelTurn)
    } else if self.question_view.is_some() {
        Some(BlockingCard::Question)
    } else if self.elicitation_view.is_some() {
        Some(BlockingCard::McpElicitation)
```

`Permission` 说"a permission request interrupts whatever else is waiting"，即工具授权可以抢断任何其他等待中的卡片。计划批准不在这个枚举里，它有自己的 `KeyOwner::PlanApproval`（`src/app/agent_view/render.rs:295-300`）。

| 类型 | 载体 | 源码入口 | 用户可做的决定 |
| --- | --- | --- | --- |
| 工具授权 | `BlockingCard::Permission` | `src/app/acp_handler/permissions.rs:3-58` | 允许一次 / 总是允许 / 拒绝一次（可附理由）/ 永不允许 / 开启 always-approve |
| 计划批准 | `KeyOwner::PlanApproval` | `src/views/plan_approval_view.rs` | `approved` / `cancelled`+feedback / `abandoned` |
| 澄清提问 | `BlockingCard::Question` | `src/views/question_view.rs` | 逐题选答案 / 自由文本 / 整组跳过 |
| MCP elicitation | `BlockingCard::McpElicitation` | `src/views/elicitation_view/` | 填表 Accept / Decline / 取消；URL 表单则是 Open URL / Decline |
| 敏感操作同意 | `RewindPhase::Confirm`；consent gate | `src/views/rewind.rs:446-509`；`src/app/consent.rs` | rewind 二次确认；法律告知接受/退出 |

补充：`BlockingCard::CancelTurn`（`src/views/modal.rs` 的 `CancelTurnChoice`）也是裁决类交互，但它属于"打断正在跑的回合"，不在本专题重点内。

## 2. 统一的视觉语法

五类卡片共享同一套骨架，这是设计上最值得抄的一点：**左侧 1 列 accent 竖条 + 标题 + 正文 + 底部独占的动作行**。

### 2.1 通用骨架

```text
┃                          ← area.x 列，`glyphs::accent_bar()` = U+2503 `┃`（legacy ConHost 降级为 │）
┃
┃  <标题>                    ← BOLD，theme.text_primary
┃  <正文 / 表单 / 预览>       ← theme.gray 或语法高亮
┃  <错误 banner（可选）>      ← theme.accent_error
┃
┃  <动作行，固定在底部>        ← 每行 = 快捷键 + 标记 + 标签
```

权限卡的坐标写死在渲染函数里（`src/views/permission_view.rs:442-457`）：`content_x = area.x + 3`、`content_width = area.width - 5`，且整个 area 填 `theme.bg_light`。

### 2.2 焦点丢失时的处理

卡片不是模态到"不许离开"的程度。当用户把焦点移到 scrollback，卡片仍绘制但整体降饱和：

```rust
if !focused {
    crate::render::color::recede_area(buf, area, theme.bg_light, 0.66);
}
```

出处：`src/views/permission_view.rs:707-709`；`question_view.rs`、`elicitation_view/render.rs`、`rewind.rs:543-545` 用同一模式，且注释互相引用（"Mirrors the unfocused prompt widget pattern"）。

同时**焦点行的背景带会被撤掉**，让界面读起来是"当前没有活动选择"（`src/views/permission_view.rs:665-671`；`question_view.rs` 同规则）。这是一个很克制的防误触设计：失焦后按 Enter 不会误批。

## 3. 工具授权（Permission card）

### 3.1 选项集从哪来

选项由 **shell 侧**构造，pager 只负责渲染。`AcpPrompter::build_options` 先按访问类型选基础集合，再剥掉被设置门控的行，最后**把 YOLO 行插到第 0 位**（`xai-grok-workspace/src/permission/prompter.rs:506-520`）：

```rust
fn build_options(&self, access: &AccessKind) -> IndexMap<...> {
    let mut base = self.build_options_inner(access);
    if !self.remember_tool_approvals {
        for id in REMEMBER_TOOL_APPROVALS_GATED_IDS { base.shift_remove(...); }
    }
    prepend_enable_always_approve(self.client_type, base)
}
```

`REMEMBER_TOOL_APPROVALS_GATED_IDS` 共 8 个 id（`prompter.rs:290-299`），由 `[ui] remember_tool_approvals` 统一开关（默认开，手册 22 确认）。

bash 分支的排序理由被明确写在注释里（`prompter.rs:579-581`）："Always-allow leads; persistent deny trails so it never sits between safe options"（always-allow 打头，持久 deny 压在最后，免得它夹在安全选项中间）。

### 3.2 界面示意（bash 授权）

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

标题逻辑（`src/app/acp_handler/permissions.rs:226-270`）：执行类优先取 `BashToolInput.description`，否则退到 ``Allow `<bin>`?``；编辑类是 `Allow Edit to {path}?`；其余是 `Allow {tool}?`，并按 `ToolKind` 兜底成 `Allow Edit?` / `Allow Execute?` / `Allow Delete?` / `Allow?`。在 local workspace 模式下标题会被追加 ` (on your machine)`（`:276-290`）。

选项行的字节布局（`permission_view.rs:1466-1470`）：

```rust
let mut spans = vec![
    Span::styled(format!("{sc} "), num_style),          // "1 " / "2 " … accent_user
    Span::styled(format!("{marker} "), marker_style),   // "(●) " 选中 / "(○) " 未选中
    Span::styled(label_prefix, label_style),            // BOLD 仅当 is_cursor
];
```

`on` 状态下还要追加作用域词（`:1472-1487`），bash 的作用域词走 `tasks_pane::highlight_bash_command` 做语法高亮，MCP 的走纯文本。整行宽度按 `content_width` 截断。

### 3.3 折叠与高度上限

- `PERMISSION_COLLAPSED_ROWS = 5`（`permission_view.rs:348`）；正文或命令超过 5 行时只画 4 行 + 一行 `... Ctrl-F to expand`（`:356-376`、`:1351-1361`）。
- 卡片高度默认被压到屏幕的一半（下限 10 行、上限屏高 80%），展开后解除（`:331-346`）：

```rust
let cap = (screen_h as u32 / 2).max(10).min(screen_h as u32 * 80 / 100) as u16;
```

### 3.4 拒绝时收集理由

`RejectOnce` 行是**行内可编辑的输入行**，不是普通 radio 行。未输入时标签是灰色占位文案（`permission_view.rs:1519-1528`）：

```rust
let (label, label_style) = if has_text {
    let preview = crate::render::line_utils::truncate_str(first_line, 50);
    (preview, Style::default().fg(theme.text_primary).bg(row_bg))
} else {
    ("No, reject (type to add feedback)".to_string(),
     Style::default().fg(theme.gray).bg(row_bg))
};
```

在 Options 焦点下直接敲字符（非数字）会进入 `PermissionFocus::FollowupInput` 并把按键转给 composer（`src/app/agent_view/interactions.rs:174-183`），Enter 提交后理由进 ACP 响应的 `meta.followup_message`（`src/app/dispatch/permissions.rs:213-222`）。若该 prompt 没有 `RejectOnce` 选项，则改为回 `Cancelled`（`:201-211`）。

### 3.5 作用域的收放

`←/→` 调节"总是允许/永不允许"覆盖多少词（`interactions.rs:105-160`）：

- 无 MCP 时，若当前行不是 scoped 行，先把光标跳到 scoped 行（`scoped_row_jump_idx`）。
- 选中 `REJECT_ALWAYS_COMMAND_OPTION_ID` 时改的是 `bash_deny_selection_count`（每按一次 ±1 个词）。
- 选中 `ALLOW_ALWAYS_COMMAND_OPTION_ID` 时走 `step_persisting_allow_scope`，**跳过所有"存了也白存"的作用域**（`permission_view.rs:243-257`）。
- MCP 场景下 `←` 选 Server 作用域、`→` 选 Tool 作用域（`interactions.rs:110-119`）。

`e` 键进入自由 pattern 编辑器，预填当前命令文本（`interactions.rs:161-173`）。编辑器下方的预览行做实时校验，并用颜色区分三种结局（`permission_view.rs:777-846`）：

```text
✗ matches everything, won't be saved     ← theme.accent_error（catch-all glob 被拒）
✓ matches this command  ·  ⚠ very broad  ← theme.accent_success / theme.warning
✗ won't match this command               ← theme.accent_error
```

进入编辑器前会拒绝 catch-all pattern：`bash_glob_is_catchall(p)` 为真时 Enter 不提交（`interactions.rs:203-213`）。

## 4. 计划批准（Plan approval）

### 4.1 状态与 outcome

```rust
pub enum PlanApprovalFocus { Preview, Prompt, Commenting }   // plan_approval_view.rs:34-39

/// A revision request is not an outcome: the review reopens and plan mode stays on.
pub enum PlanReviewOutcome { Approved, Abandoned }            // :47-52
```

这是本专题最有价值的一条语义约定：**"请求修改"不是终态**。它映射成 `cancelled` + feedback，界面重开、plan mode 保持开启；只有 `approved` / `abandoned` 会真正关闭 review。三个发送函数（`:229-243`）：

```rust
pub fn send_approved(&mut self) -> bool { send_ext_response(self.take_response_tx(), "approved", None) }
pub fn send_abandoned(&mut self) -> bool { send_ext_response(self.take_response_tx(), "abandoned", None) }
pub fn send_cancelled(&mut self, feedback: Option<String>) -> bool { ... "cancelled", feedback }
```

`oneshot::Sender` 被 `take()` 走，因此**二次发送返回 false**，测试 `test_double_send_returns_false`（`:417-422`）把这条钉死。

### 4.2 界面示意

计划内容画在 line viewer 里，动作按钮居中排在底部分隔线下方（`src/views/file_search/line_viewer.rs:1660-1760`）：

```text
  # Plan
  ## Step 1
  Do something                    ← 预览区，可 j/k/箭头滚动，可选中行范围加评论
  ────────────────────────────────
      a approve  |  s request changes  |  c comment ② ●  |  q quit plan  |  y copy plan
```

按钮宽度不足时**先丢 `y copy plan`**（`line_viewer.rs:1748-1750`，`show_copy` 判定）。有评论时 `a` 的标签变成 `approve w/ comments`（`:1685-1694`）。

### 4.3 一个易被忽略的"拒绝路径"

当注释里出现完整 slash 命令（如 `/feedback <text>`）时，按 `a` 会被**拒绝**并给出 toast，而不是把命令当评论发出去（`src/app/agent_view/plan.rs:416-436`）：

```rust
PlanApprovalFocus::Preview | PlanApprovalFocus::Prompt => {
    pav.focus = PlanApprovalFocus::Prompt;
    "Run the slash command in the notes with Enter, or clear it, before approving."
}
```

## 5. 澄清提问（AskUserQuestion）

### 5.1 界面示意

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

计数器在 `src/app/agent_view/render.rs:541-542`（`format!("[{}/{}] ", …)`）。选项行前缀固定 6 列宽：`X (●) ` 或 `X [✓] `（`question_view.rs:1048-1056`）。自由文本行是 sticky 的，不参与滚动列表（`question_view.rs:1674-1700`），可以用 `no_freeform` 整体关掉（`:189-192`）。

### 5.2 快捷键映射（写得很克制）

```rust
pub fn option_shortcut_label(idx: usize) -> Option<char> {
    match idx {
        0..=8 => Some((b'1' + idx as u8) as char),
        9..=34 => Some((b'a' + (idx - 9) as u8) as char),
        _ => None,
    }
}
```

**注意不对称**：显示时 `a`–`z` 都能当快捷键标签，但**接收时只认 `a`–`f`**（`question_view.rs:1028-1046`）：

```rust
/// Map a pressed key character to an option index. Maps `'1'`..`'9'` to 0..8 and `'a'`..`'f'` to
/// 9..14. Only a-f are mapped as shortcuts to avoid conflicts with navigation keys. (g=top,
/// h=prev-question, j=down, k=up, l=next-question, n=next, s=skip).
pub fn option_index_for_key(c: char) -> Option<usize> {
```

保留给导航的字母：`g/G` 首/末、`h/l`、`[` / `]` / `←/→` 切题、`j/k` 上下、`Ctrl+D/U` 半页、`PageUp/Down`、`z` 跳到自由文本、`y` 复制选项文本、`Tab` 在行间环绕（`interactions.rs:410-560`）。

## 6. MCP elicitation

### 6.1 三阶段状态机

```rust
pub enum ElicitationStage { Form(FormStage), UrlConsent(UrlConsentStage), UrlWaiting(UrlWaitingStage) }
```

设计意图写在文件头（`elicitation_view/state.rs:1-2`）："The stage is a data-carrying enum: `Form` 与 `UrlConsent` 自己持有 pending ACP responder；`UrlWaiting` 只在响应已发出后存在，所以**阶段本身编码了"这张卡是否还欠一个响应"**。"

`UrlWaiting` 阶段只剩本地动作 `y Done`，不许再产生 ACP 响应——类型层面避免双发。

### 6.2 界面示意（Form）

```text
┃
┃  MCP "postgres" requests your input        ← 标题，BOLD
┃  Please provide connection details          ← server message，上限若干行
┃  <schema 解析错误 banner，accent_error，≤2 行>
┃
┃  host       [localhost        ]             ← 字段名 + 值列对齐（form_value_column）
┃  port       <5432>
┃  ssl        [x]                             ← bool/enum 用 Tab 或 Enter 切换
┃  ↑ more                                     ← 滚动标记画在分隔行上
┃  ↓ more
┃
┃  y Accept   d Decline
```

动作行由 `actions()` 决定（`render.rs:141-154`）：

```rust
ElicitationStage::Form(_) => vec![(ElicitHit::Accept, 'y', "Accept"),
                                  (ElicitHit::Decline, 'd', "Decline")],
ElicitationStage::UrlConsent(_) => vec![(ElicitHit::Accept, 'y', "Open URL"),
                                        (ElicitHit::Decline, 'd', "Decline")],
ElicitationStage::UrlWaiting(_) => vec![(ElicitHit::Accept, 'y', "Done")],
```

动作行**钉死在底部**，正文在中间滚动（`render.rs:244-288`，注释 "Pin the action rows at the bottom; the body scrolls in between"）。高度策略：软上限屏高 33%、硬上限 80%，但必须给正文留至少 3 行（`render.rs:156-180`）。

默认焦点是 **Accept**（`state.rs:359-367` 的 `action_focus: ElicitationActionFocus::Accept`）——这是本专题里默认值偏"放行"的第二处。URL 表单的焦点则直接落在 Actions（`state.rs:339-352`），不进入 Fields。

### 6.3 URL 同意

URL 表单会先做校验再决定渲染：`check_elicit_url` 失败时 `display.host` 为 `None`、`invalid` 有值，banner 用 `accent_error` 画出原因（`state.rs:333-352`、`render.rs:231-243`）。server 名与 message 都过 `sanitize_server_text` 截断（`state.rs:235-243`），且 `server_name_wire` 单独保留原始串供后续通知做相等比较（`state.rs:34-35`）。

快捷键栏对该卡的提示是（`src/app/agent_view/render.rs:307-313`）：

```rust
KeyOwner::Card(BlockingCard::McpElicitation) => vec![
    HintItem::new(key!(Enter), "accept / toggle"),
    HintItem::new(key!('d'), "decline"),
    HintItem::new(key!('c', CONTROL), "cancel"),
    HintItem::new(key!(Tab), "next field"),
    self.card_esc_hint(),
],
```

`Esc` 在 `UrlWaiting` 阶段是 `EscStep::DismissElicitWaiting`（label `dismiss`），会走 `resolve_elicitation_cancel`——即关掉等待界面而不是再发一次取消（`key_owner.rs:59-91, 247`）。

## 7. 敏感操作同意

### 7.1 rewind 的二次确认

`RewindPhase` 明确区分了"选择点"和"确认"（`src/views/rewind.rs:52-72`）。`Confirm` 阶段是独立一屏，注释把三个选项写清楚了（`:86-88`）：

```rust
const CANCEL_OFFER_OPTIONS: usize = 2;
/// Yes / Yes, and don't ask again / No.
const CONFIRM_OPTIONS: usize = 3;
```

```text
┃
┃  Rewind conversation to "fix the auth bug"?     ← 预览文本按剩余宽度截断加 …
┃  y   (●) Yes
┃  a   (○) Yes, and don't ask again
┃  n   (○) No
```

`y` / `a` / `n` 是单键直接触发，不需要先移动光标（`:148-160`）；同时 `j/k` + Enter 也支持。渲染函数 `render_radio_row` 的 key 列固定 4 列宽（`:600-604`）：

```rust
let line = Line::from(vec![
    Span::styled(format!("{key_display:<4}"), num_style),
    Span::styled(format!("({marker}) "), marker_style),
    Span::styled(label.to_string(), label_style),
]);
```

对 `CancelOffer` 阶段（有回合在跑时先问要不要打断），同样用 `y` / `n` 单键（`:140-147`）。

### 7.2 consent gate（法律告知）

这是一个**与安全门相反的失败策略**：`src/app/consent.rs:1-5` 的开头就写明

```rust
//! Consent gate, modelled on folder trust. ...
//! Every failure path fails open: a client that cannot reach settings must stay usable.
//! Validation is the trust boundary, so a notice that survives it is safe to paint.
```

它的防误触手段是**"没画出来就不许接受"**（`app/consent.rs:83-95`）：

```rust
pub enum ConsentLegibility { #[default] Illegible, Painted }
impl ConsentLegibility { pub fn can_accept(self) -> bool { matches!(self, Self::Painted) } }
```

渲染层据此**直接不画接受行**，而不是画了再忽略（`src/views/welcome/consent.rs:73-79`）："Accept is refused while the body is unread, so the row is withheld rather than offered and ignored. Quit stays, or the screen would show no way out at all."

按键选择也做了避让（`app/consent.rs:507-541`）：

```rust
/// Accept is `a`: `y` accepts on the trust screen one step later, and Enter may be buffered before the notice paints.
/// No decline, so `q` quits and the rest is swallowed.
```

并且**在正文 paint 之前到来的按键一律丢弃**，只放行 `Ctrl+C` / `Ctrl+D`（`consent.rs:529-537`）——因为"这个屏是它们唯一的处理器"。链接编号用 `1`–`9`，`MAX_KEYED_LINKS = 9`（`consent.rs:50-52`）。

## 8. 默认选中项如何决定

这是本专题**最需要谨慎借鉴**的一处。`resolve_initial_cursor` 的注释就是完整规范（`xai-grok-pager-render/src/appearance/permission_cursor.rs:161-185`）：

```rust
/// Sticky last-used, then configured default, then the YOLO row by identity, else index 0.
/// A concrete target skips YOLO. A missing target kind degrades to one-shot allow, never to global always-approve.
pub fn resolve_initial_cursor(options: &[acp::PermissionOption]) -> usize {
    let target = match last_used_permission() {
        DefaultSelectedPermission::AlwaysAllowAllSessions => load_default_selected_permission(),
        sticky => sticky,
    };
    if let Some(idx) = options.iter().position(|o| target.matches_kind(&o.kind) && !is_enable_always_approve_option(o)) {
        return idx;
    }
    if target != DefaultSelectedPermission::AlwaysAllowAllSessions
        && let Some(idx) = options.iter().position(|o| o.kind == AllowOnce && !is_enable_always_approve_option(o))
    { return idx; }
    options.iter().position(is_enable_always_approve_option).unwrap_or(0)
}
```

四条事实：

1. **无粘性选择时，光标落在 YOLO 行**。`DefaultSelectedPermission::AlwaysAllowAllSessions` 是"未设置/无法识别"的兜底值（`:16-26`、`:55-62`），而配置项的默认值也是它 —— `src/settings/registry.rs:626-628` 与 `:1125-1138` 的测试注释直接写："**The first prompt's cursor lands on the 'Always allow on all sessions' row, picked explicitly in `enqueue_permission`**"。对应测试 `resolve_unset_lands_on_yolo_row`（`:317-335`）断言 index 1（YOLO 行所在位置）。
2. **粘性选择只记录"允许类"**。记录点在 `src/app/dispatch/permissions.rs:132-151`，注释解释了为什么排除拒绝："a rejection must not steer a later prompt's cursor onto a reject row… would steer an unrelated later prompt onto its 'always allow this command' row, escalating scope"。同时排除 YOLO 行和 `allow-edits-session`（`:135-136`）。
3. **降级方向是安全的**：具体目标行不存在时退到 `AllowOnce`，**绝不**升到全局 always-approve。测试 `unmatched_concrete_target_degrades_to_allow_once_not_yolo`（`:392-418`）的注释："It never escalates to the global always-approve row, which is one Enter away from persisting always-approve mode."
4. 配置键是 `[ui].default_selected_permission`，四个枚举值 `always_allow_all_sessions` / `allow_once` / `allow_command_always` / `reject`（`:28-62`）；env 覆盖 `GROK_DEFAULT_SELECTED_PERMISSION`（`:105-124`）。设置弹窗改动走 `set_default_selected_permission` 做乐观更新（`:126-131`）。

`enqueue_permission` 的调用点：`src/app/acp_handler/permissions.rs:146-148`。

计划批准与提问卡的默认值：`PlanApprovalFocus::Preview`（`plan_approval_view.rs:114`）、提问卡 `QuestionSelection::Single(None)`（无预选，`question_view.rs`）、rewind Confirm `active_idx: 0`（= Yes，`rewind.rs:648`）、elicitation `action_focus: Accept`（`state.rs:359-367`）。**注意 rewind 与 elicitation 的默认项都是"放行"侧**，只有提问卡没有预选。

## 9. 持久化选项

| 行 | option_id | kind | 持久化范围 | 证据 |
| --- | --- | --- | --- | --- |
| `Yes, and don't ask again for anything (always-approve mode)` | `enable-always-approve` | `AllowOnce` | 写 `[ui] permission_mode = "always-approve"`，**跨会话** | `prompter.rs:22-40`；`src/app/dispatch/permissions.rs:166-178` |
| `Always allow: <cmd>` | `allow-always-command` | `AllowAlways` | 项目级 `permission.toml` | `prompter.rs:587-593`；手册 22 |
| `Never allow: <cmd>` | `reject-always-command` | `RejectAlways` | 项目级 | `prompter.rs:598-606` |
| `Always allow: <server>__<tool>` | `allow-always-mcp` | `AllowAlways` | 项目级，可在 Tool/Server 两种作用域间切 | `prompter.rs:677-693`；`interactions.rs:110-119` |
| `Yes, always allow <domain> for this project` | `allow-always-domain` | `AllowAlways` | 项目级 | `prompter.rs:626-634` |
| `Yes, allow all edits during this session` | `allow-edits-session` | `AllowAlways` | **仅内存**，不落盘、不跨重启 | `prompter.rs:18-20` |
| rewind 的 `Yes, and don't ask again` | — | — | 关闭 `confirm_before_rewind` 设置 | `rewind.rs:84-90`；`src/views/settings_modal/state.rs:934` |

`AllowOnce` 的 kind 选择是有讲究的：YOLO 行故意用 `AllowOnce`，因为 "the pager's YOLO auto-approve drain, which answers with the first `AllowOnce`, picks this option. That is safe: the drain bypasses `dispatch_permission_select`, so picking it does not re-fire `set_yolo_mode(true)`"（`prompter.rs:31-33`）。即**用 kind 复用让自动放行逻辑天然选中它，同时靠旁路调用点避免副作用重入**。

"Always allow" 行不是永远出现。`always_allow_row_is_effective(cmd)` 会用一份干净的 scratch 状态模拟"接受默认作用域后整条脚本能否无提示重放"，失败则不显示该行（`xai-grok-workspace/src/permission/grants/bash_grants.rs:134-160`，注释 "the 'Always allow' row is not offered at all rather than saving a rule that would not work"）。

## 10. 风险分级在界面上的表达

**结论：颜色几乎不承担风险分级，风险靠"措辞 + 作用域 + 行是否出现"表达。** 通读 `permission_view.rs`（3037 行）后，用到的 theme 字段只有：`accent_user`（键位/竖条）、`text_primary`、`text_secondary`、`gray`、`bg_light`、`bg_hover`、`bg_visual`、`selection_overlay()`、`hover_overlay()`，以及 pattern 预览行的 `accent_error` / `accent_success` / `warning`（`:802, 817, 824`）。**不存在按风险级别切换标题色或整卡底色的分支。**

风险实际表达在三处：

1. **措辞**。所有选项标签是自然语言句子而非动作名：`Yes, proceed` / `No, and tell Grok what to do differently` / `Never allow: git push`。
2. **固定顺序**。always-allow 打头、persistent deny 压尾，deny 绝不夹在安全选项之间（`prompter.rs:579-581`）。
3. **危险命令被 pin 到完整命令**。`always_allow_scope_pinned` 让危险动词（`rm` / `chmod` / `chown` / `chgrp` / `chattr` / `pkill` / `kill` / `killall` / `git push`，见 `grants.rs:439-455`）与 exec 载体（解释器、包管理器、`sudo`/`ssh`）**只能以完整命令持久化**（`grants.rs:356-372`）：

```rust
fn always_allow_scope_pinned(words: &[String]) -> bool {
    is_dangerous_command_words(words)
        || crate::permission::policy::head_is_exec_vehicle(words)
        || crate::permission::policy::normalized_command_head(words).as_deref() == Some("sed")
}
```

`minimum_always_allow_scope` 让 `←` 也**收不到比默认更窄**的位置（`grants.rs:427-435`），注释："Narrowing `gh` broadens the grant"。危险命令清单同时是"记住的前缀不算数、必须重新问"的依据（手册 22 "Dangerous Commands"）。

## 11. 超时与默认行为

**本地 TUI 的权限卡没有超时。** 在 `xai-grok-pager/src/`、`xai-grok-workspace/src/permission/` 内检索 `timeout`，唯一的命中是 `AUTO_CLASSIFIER_TIMEOUT`（`permission/reasons.rs:10`，auto-mode 分类器用）——那是分类器超时，不是等用户。权限队列只在三种情况解阻塞：用户选择（`dispatch_permission_select`）、用户取消（`dispatch_permission_cancel`）、回合结束/取消时批量 drain（`drain_permission_queue`，`src/app/dispatch/permissions.rs:259-276`）。**不响应 = 无限期等待**（fail-closed 的一种朴素形式：不会自动放行）。

三类"非本地卡片"路径策略不同：

| 路径 | 超时 | 到期行为 | 证据 |
| --- | --- | --- | --- |
| 本地 TUI 权限卡 | 无 | 永久等待 | 上述检索；`dispatch/permissions.rs:236-276` |
| `AskUserQuestion` | **30 分钟**（可关） | 返回与"用户跳过"相同的文本，**不算工具失败** | 下方 |
| hub / 远程权限（chat-over-server） | 600 s backstop | **拒绝**（fail-closed） | 下方 |
| consent gate | 无 | 未接受则不进入会话；**所有失败路径 fail open** | `src/app/consent.rs:3-4` |
| headless（`grok -p`） | 不适用 | 非 yolo 一律回 `Cancelled`；yolo 取第一个 `AllowOnce`/`AllowAlways`，都没有也回 `Cancelled` | `src/headless.rs:1766-1786` |

提问卡超时：

```rust
/// Default max time to wait for the user to answer the questionnaire (all questions in this tool
/// call share one timer): 30 minutes. On expiry the tool returns the same skipped/cancel text as a
/// user dismiss (`format::unanswered_text`), not a tool failure.
pub const RESPONSE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30 * 60);
pub const DEFAULT_ASK_USER_QUESTION_TIMEOUT_ENABLED: bool = true;
```

出处：`xai-grok-tools/src/implementations/grok_build/ask_user_question/mod.rs:47-59`。env 覆盖 `GROK_ASK_USER_QUESTION_TIMEOUT_SECS`（`:61-64`）；`timeout_enabled = Some(false)` 表示"永远等待答案"，`timeout_secs = 0` 被显式拒绝并回落到默认值（`:96-124`）。到期/取消的文案区分有无操作者（`ask_user_question/format.rs:21-35`）：

```rust
pub const CANCEL_TEXT: &str = "User declined to answer the questions. Continue with the task using your best judgment, or ask different questions.";
pub const NO_OPERATOR_TEXT: &str = "No user is available to answer questions in this non-interactive session. Continue with your best judgment; do not wait for clarification.";
```

注意措辞里的**显式防呆**："or ask different questions" / "do not wait for clarification" —— 超时文本本身在阻止模型原地重问。

hub 路径（远程/沙箱把权限提示转发给宿主客户端时）：

```rust
Err(e) => {
    tracing::error!(error = %e, "hub permission request failed; rejecting");
    PromptOutcome::Error(format!("hub permission request failed: {e}"))
}
```

`xai-grok-workspace/src/permission/hub_permission.rs:271-275`。超时与其它错误都走这一支（`:88-101` 只把超时单独计入 `grok_workspace_permission_timeout_total` 指标）。600 s 来自 `xai-computer-hub-sdk/src/server.rs:1365-1372`：

```rust
/// Backstop deadline for [`Self::request_hook`].
/// Deliberately long: the hook is normally released by the real reply or requester teardown;
/// this only bounds a request whose reply is lost (e.g. a dead connection), so it sits above any turn deadline.
pub const HOOK_REQUEST_BACKSTOP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);
```

（注意：`is_timeout_err` 只做子串匹配 `"timed out"`，`hub_permission.rs:38-40`；其测试用 "request timed out after 600s" 这个串，与上述 600 s 常量吻合，但两者在代码中没有直接引用关系——这一条属于强相关推断，非直接证据。）

## 12. 键盘流：最少击键完成批准

| 场景 | 最少击键 | 键 |
| --- | --- | --- |
| 权限卡（接受当前光标项） | **1** | `Enter` |
| 权限卡（接受第 n 项） | **1** | `1`–`9`（直接选中并派发，不经过光标移动） |
| 权限卡（拒绝并说明理由） | 2 | 直接敲字符进 FollowupInput → `Enter` |
| 权限卡（缩小"总是允许"作用域后接受） | 3+ | `←` × n → `Enter` |
| 权限卡（手写 glob） | 2+ | `e` → 编辑 → `Enter` |
| 权限卡（开 always-approve） | 1 | `Ctrl+O`（全局切换，不必选中第 1 行） |
| 计划批准 | 1 | `a`（**前提：prompt 输入框为空**） |
| 计划批准（请求修改） | 2+ | `s` → 输入 → `Enter` |
| 提问卡（选第 n 项） | **1** | `1`–`9` / `a`–`f`（若是最后一题，直接提交整组） |
| 提问卡（跳过整组） | 1 | `Shift+X` |
| elicitation（Accept） | 1 | `y` 或 `Enter` |
| elicitation（Decline） | 1 | `d` |
| rewind 确认 | 1 | `y` / `a` / `n` |
| consent 接受 | 1 | `a`（仅在正文已绘制后） |

数字直选的三处实现：权限卡 `interactions.rs:88-96`（`KeyCode::Char(ch @ '1'..='9')`）、提问卡 `interactions.rs:520-545`（走 `option_index_for_key`）、`CancelTurnChoice` 用 `1`–`4`（`interactions.rs:255-262`）。

`Ctrl+O` 在任何 `PermissionFocus::Options` 状态下都可用（`interactions.rs:99-101`），并出现在快捷键栏（`render.rs:249`）：

```rust
hints.push(HintItem::new(key!('o', CONTROL), "always-approve"));
```

`a` approve 的前置条件值得注意（`plan.rs:789-796`）——**输入框非空时 `a` 不生效**，会落进 composer 变成普通字符：

```rust
if !is_commenting
    && key.code == KeyCode::Char('a')
    && key.modifiers.is_empty()
    && self.prompt.text_without_image_chips().trim().is_empty()
    && !self.prompt.file_search_visible()
{
    return self.approve_plan();
}
```

这是一条隐式的"防止带着半截评论误批"保护。

## 13. 这些界面如何避免误触

按机制强度从强到弱排列。

### 13.1 Esc 不是拒绝（最强的结构性保护）

权限卡的 `Esc` 映射到 `EscStep::ParkFocus`，快捷键栏标签是 `scrollback`——**交出键盘焦点，卡片继续挂着**，绝不等于拒绝（`src/app/agent_view/key_owner.rs:167-173`）：

```rust
BlockingCard::Permission => match self.permission_queue.front()?.focus {
    PermissionFocus::FollowupInput => EscStep::LeaveTextInput,   // label "back"
    PermissionFocus::PatternEdit => EscStep::DiscardPatternEdit, // label "cancel"
    PermissionFocus::Options => EscStep::ParkFocus,              // label "scrollback"
},
```

对比提问卡的 `Esc` 也不会交答案：无选择时 ParkFocus，有选择时先 `ClearSelection`（label `unselect`）（`key_owner.rs:177-192`）。取消权限是**只有 `Ctrl+C` 一个入口**（`interactions.rs:28-29` 的注释："`Esc` steps back down through them and `Ctrl+C` is the only cancel"，实现见 `:46-48, 72-74, 192-194`）。

### 13.2 双击确认（PendingAction）

需要确认的动作，第一次按下只是"arm"，快捷键栏被整条替换为提示（`src/views/shortcuts_bar.rs:1-3`）：

```rust
//! When a `PendingAction` is active (double-press confirmation), the bar replaces all hints with "press again to {label}".
```

```rust
pub struct PendingAction {
    pub action: Action,
    pub shortcut: KeyShortcut,
    /// When `Some`, shortcuts bar shows "press again to {label}".
    pub label: Option<&'static str>,
    pub expires_at: Instant,
}
impl PendingAction {
    pub const TTL: Duration = Duration::from_millis(1000);
    pub const ESC_DOUBLE_PRESS_TTL: Duration = Duration::from_millis(800);
```

出处：`src/app/app_view.rs:459-478`。空闲态 `Esc`（清空/rewind）用 800 ms 窗口，可由 `GROK_ESC_DOUBLE_PRESS_MS` 覆盖（`:519-531`）。`label: None` 可"静默 arm"（不显示提示），见 `InputOutcome::ArmPending` 的注释（`:425-434`）。

### 13.3 不可接受的行根本不画

consent gate 的接受行在正文不可读时**直接不渲染**（`views/welcome/consent.rs:73-79`），而不是"画了但不响应"。同理，作用域行不可用时 `has_adjustable_scope()` 为 false，提示行与 `←/→` 处理一起消失（`permission_view.rs:178-196` + `interactions.rs:105`），避免出现"按键没反应"的死键。

### 13.4 危险项的默认作用域被 pin 死

`default_always_allow_scope` / `minimum_always_allow_scope` 让 `←` 无法把危险命令的作用域收窄成一条能匹配任意参数的规则（`grants.rs:356-372, 427-435`）；`bash_glob_is_catchall` 让 catch-all glob 无法被保存（`permission_view.rs:798-806`，`policy.rs:623`）。

### 13.5 失焦即"无选择"

卡片失焦时撤掉焦点行的背景带，整体降饱和（见 §2.2）。这让"视觉上看起来没有选中项"与"按键不会命中某项"保持一致。

### 13.6 其它

- 每个卡片的 `Esc` 语义是**栈式的**：`EscStep` 有 8 个变体，逐层退出而不是一步取消（`key_owner.rs:59-91`）。
- 单选与自由文本互斥：选选项会清掉 freeform 的 selected 标记，反之亦然（`interactions.rs:466-472`、`:510-514`）。
- 多客户端下另一个客户端已解决同一交互时，本地卡会被撤回：`dismiss_resolved_interaction(tool_call_id)`（`interactions.rs:1060-1072`），注释："Retract an interaction modal (permission, question, or plan approval) that another connected client already resolved."
- 切换到下一个权限请求时，焦点被重置为 `Options`，并清掉上一个请求残留的 followup 文本（`dispatch/permissions.rs:285-294`）；pattern 编辑缓冲是无条件丢弃的（`:283-284`）。

## 关键结论（供合稿引用）

1. 五类裁决交互（工具授权 / 计划批准 / 澄清提问 / MCP elicitation / 敏感操作同意）统一收敛为"阻塞卡"，由 `blocking_card()` 给出固定优先级，permission 可抢断其他卡片——`src/app/agent_view/key_owner.rs:96-105`。
2. 所有卡片共享同一骨架：左侧 1 列 `accent_bar`（U+2503）、`content_x = area.x + 3`、底部固定动作行、失焦整体降饱和 0.66——`src/views/permission_view.rs:442-457, 707-709`；`src/views/elicitation_view/render.rs:194-206, 244-288`。
3. **默认光标在无粘性选择时会落到 YOLO（"always allow on all sessions"）行**，这是配置项的显式默认值——`xai-grok-pager-render/src/appearance/permission_cursor.rs:161-185`；`src/settings/registry.rs:1125-1138`。
4. 但降级方向是安全的：具体目标行缺失时退到 `AllowOnce`，**绝不**升级到全局 always-approve；且粘性记录排除拒绝类、YOLO 行与 allow-edits-session——`permission_cursor.rs:174-180, 392-418`；`src/app/dispatch/permissions.rs:132-151`。
5. **Esc 不是拒绝**：权限卡 `Esc` = `EscStep::ParkFocus`（交出焦点、卡片存留），取消只有 `Ctrl+C`——`src/app/agent_view/key_owner.rs:167-173`；`src/app/agent_view/interactions.rs:28-29, 46-48`。
6. 需要确认的动作走 `PendingAction` 双击机制，TTL 1000 ms / Esc 800 ms，期间快捷键栏整条替换为 "press again to {label}"——`src/app/app_view.rs:463-478, 519-531`；`src/views/shortcuts_bar.rs:1-3`。
7. 风险分级**不靠颜色**，靠措辞、固定顺序（always-allow 打头 / persistent deny 压尾）与危险命令的完整命令 pin；整卡没有按风险切色的分支——`xai-grok-workspace/src/permission/prompter.rs:579-581`；`grants.rs:356-372, 439-455`。
8. 计划批准把"请求修改"建模为**非终态**：`approved` / `abandoned` 才关闭 review，修改走 `cancelled` + feedback 并重开——`src/views/plan_approval_view.rs:47-52, 229-243`。
9. 提问卡的快捷键**显示与接收不对称**：标签支持 `1`–`9` 与 `a`–`z`，但只接收 `1`–`9` 与 `a`–`f`，其余字母留给导航（`g/G/h/j/k/l/n/s`）——`src/views/question_view.rs:1028-1046`。
10. **本地 TUI 权限卡没有超时，无限等待**；提交/取消/回合结束 drain 是仅有的三条解阻塞路径——`src/app/dispatch/permissions.rs:236-276`。
11. 提问卡有 **30 分钟**超时（默认开，`GROK_ASK_USER_QUESTION_TIMEOUT_SECS` 可覆盖），到期返回与"用户跳过"相同的文本并显式指示模型不要原地等待——`xai-grok-tools/.../ask_user_question/mod.rs:47-64`；`format.rs:21-35`。
12. 失败策略按路径分裂：consent gate **fail open**（`src/app/consent.rs:1-5`）；headless 非 yolo 与 hub 远程权限路径 **fail closed / 拒绝**（`src/headless.rs:1766-1786`；`hub_permission.rs:271-275`）。
