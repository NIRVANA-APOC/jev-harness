# 专题 14：首屏、空状态与渐进式披露

> 专题：首屏、空状态与渐进式披露｜调研日期：2026-09-22｜证据范围：`crates/codegen/xai-grok-pager/src/{views/welcome/**,views/{tutorial,announcements,privacy_banner,prompt_suggestion,shortcuts_help,picker,session_picker,dashboard/render,agent}.rs,tips/**,settings/{defs,registry}.rs,startup.rs,tutorial_docs.rs,docs.rs,prompt_widget/mod.rs}`、`src/app/{app_view.rs,consent.rs,event_loop.rs,session_startup.rs,agent_view/{notices,render}.rs,dispatch/**}`、`crates/codegen/xai-grok-announcements/src/lib.rs`、`crates/codegen/xai-grok-shell-base/src/util/tips.rs`、`crates/codegen/xai-grok-shell/src/util/config/tips.rs`、`crates/codegen/xai-grok-shared/src/ui_config.rs`、`crates/codegen/xai-grok-config-types/src/lib.rs`、`docs/tutorial/*`、`docs/user-guide/01-getting-started.md`（路径相对 `reference/grok-build/`）

## 1. 一次启动要穿过几道门

首屏不是一个页面，而是**同一个欢迎页上的 6 个短路分支**：`render_welcome` 用一个 `match params.auth_state` 顺序判断，**臂的顺序就是优先级**（`views/welcome/mod.rs:758-903`）。

```text
        grok 启动
            │
            ▼
   AuthState::Pending ───────────► ① 登录菜单屏        Login with grok.com / Quit     (:793)
            │ 已认证/免登录
            ▼
   AuthState::Authenticating ────► ② 认证等待屏        浏览器授权 / 设备码 / 粘贴 token (:822)
            │
            ▼
   is_zdr_blocked ───────────────► ③ ZDR 阻断屏        Switch account / Quit          (:842)
            │ 否
            ▼
   has_access == false ──────────► ④ 订阅闸门屏        Tier + [Refresh] + 文案 + 链接  (:2023)
            │ 是
            ▼
   ConsentState::Pending ────────► ⑤ 同意声明屏        a Accept / q Quit              (:867)
            │ 无待同意
            ▼
   TrustState::Pending ──────────► ⑥ 目录信任屏        y Yes, proceed / n No, quit    (:879)
            │ 已信任
            ▼
   ⑦ 正常欢迎页（hero box 或 stacked 版面）                                          (:890, :1707)
```

门禁顺序在代码注释里被显式固定：同意是账号级，所以先于工作区级的信任问题解析（`views/welcome/mod.rs:866`）。三个状态机都要 `Done` 才允许创建会话：

```rust
// crates/codegen/xai-grok-pager/src/app/app_view.rs:1252
pub fn session_startup_allowed(&self) -> bool {
    matches!(self.auth_state, AuthState::Done)
        && matches!(self.trust_state, TrustState::Done)
        && matches!(self.consent_state, ConsentState::Done)
}
```

- `AuthState`：`Done` / `Pending { error }` / `Authenticating { request_seq, handle, auth_url, mode }`，`AuthMode` 有 `Pending`、`Command`、`Loopback`、`Device` 四种（`app/app_view.rs:375-404`）。
- `TrustState`：`Done` / `Pending { workspace }`，特性开关关闭时 `decide` 直接返回 trusted（`app/app_view.rs:405-418`）。
- 信任判定在首帧渲染前 seed：`seed_trust_state`（`app/event_loop.rs:336-356`，`TrustOutcome::Prompt => Pending` 在 `:354`）；同意同理 `seed_consent_state_from_gate`（`app/event_loop.rs:358-377`），注释写明「必须早于第一次渲染，否则启动意图块会在门禁后面就把会话开起来」。
- **输入捕获的守卫**：门禁未清空时，启动时敲入的 prompt 会被丢弃而不是重放，否则一个以 `n` 开头的 prompt 可能替用户回答信任问题并退出（`app/app_view.rs:1257-1266` `ready_for_startup_typeahead`）。

## 2. 逐屏：视觉、按键与可跳过性

### 2.1 ① 登录菜单屏（`AuthState::Pending`，`:793`）

- 版面：logo，居中一行 `Login with <label>`（label 默认 `grok.com`），菜单两行 `l` / `q`，底行版本徽标——走的是 `render_welcome_blocked` 的「Logo. {prompt}. Version」三段式（`views/welcome/mod.rs:917-1000`）。
- 若上一次认证失败，`error` 会以 `theme.accent_error` 画在同一行槽位（`:796-800`）。
- 登录屏**仍绘制 home 草稿的输入框**（logout 时保留草稿），因此布局用与首页相同的测量（`:955-966`）。
- 可跳过性：不可跳过（无凭证），但可 `q` 退出。

### 2.2 ② 认证等待屏（`AuthState::Authenticating`，`:822`）

四个子臂共享「logo + 居中块 + 退出提示」结构（`render_browser_status_arm`，`:1303`）：

| 子臂 | 屏幕内容 |
| --- | --- |
| `Command` | `A browser window will open for authentication.`（`AUTH_HEADER`，`:1079`）+ 等待文案 + 「点这里复制」 |
| `Loopback` | 同上，另有一个 token 粘贴输入框（`render_auth_input_box`，`:2552`），token 以掩码渲染 |
| `Device` | `Approve in your browser to finish signing in.`（`DEVICE_AUTH_HEADER`，`:1081`）+ 设备码 + `Make sure your browser shows this code.`（`DEVICE_CODE_CAPTION`，`:1083`） |
| 原始 URL 模式 | `render_raw_url_mode`（`:1203`）关闭鼠标捕获，让用户用终端原生选择复制整条 URL |

- 设备码是从验证 URL 里解析出 `user_code` 再上屏，属于反钓鱼设计（`:1085-1094`）。
- 复制提示统一为 `If it doesn't open, click here to copy.`，失败时降级为 `Copying not working? Click here to show full URL.`（`:1096-1099`、`AUTH_FALLBACK_TEXT` `:1128`）。
- 退出提示按终端品牌切换：VS Code 系（VS Code / xterm.js / Zed）显示 `ctrl+d`，其余 `ctrl+q`（`welcome_in_vscode_family` + `quit_hint_spans`，`:49-70`）。
- 可跳过性：必须完成浏览器授权；**未找到**取消/中断认证的按键证据。

### 2.3 ③ ZDR 阻断屏（`:842`）

- 复用 `render_welcome_blocked`，只有一行 `Grok Build is not yet available for this account.` + 菜单 `l Switch account` / `q Quit`。
- 可跳过性：不可跳过，只能换账号或退出（`:842-864`）。

### 2.4 ④ 订阅闸门屏（`:2023`）

- 不画输入框，改为居中的三行：`Tier: <tier>  [Refresh]`（`[Refresh]` 可点击，命中矩形单独计算）、一行 gate 文案（默认 `SuperGrok subscription required`，`:2072`）、一行下划线链接（默认 `https://grok.com/supergrok?referrer=grok-build`）。
- 可跳过性：不可跳过；`[Refresh]` 是唯一的「我已经订阅了」出口。

### 2.5 ⑤ 同意声明屏（`ConsentState::Pending`，`:867` → `views/welcome/consent.rs`）

- 版面：logo、居中标题、按 [`app::consent::wrap`] 排版的正文、菜单行、版本徽标；正文最大 78 列（`MAX_BODY_COLS`，`views/welcome/consent.rs:21`）。
- **可读性是硬门禁**：正文最多 12 行、2000 字节（`app/consent.rs:16-20`），装不下时**撤掉 Accept 只留 Quit**，并改为显示 `Enlarge the window to read this notice`（窄于 40 列时用短版 `Window too small`）（`views/welcome/consent.rs:27-30`、`:65-80`）。
- 布局会优先丢掉 logo 而不是判定不可读（`fit_body` 注释，`views/welcome/consent.rs:110-115`）。
- 校验失败的原因有结构化枚举（`ConsentArmRefusal`：正文过高、未配对的标记、纯文本里的 URL、版本号不合理……`app/consent.rs:47-70`），失败一律 fail-open。
- 可跳过性：不可跳过（`a` 接受 / `q` 退出）；回答持久化在本地配置，不跨机器。

### 2.6 ⑥ 目录信任屏（`TrustState::Pending`，`:879` → `render_welcome_trust`，`:1004`）

```text
            Do you trust the contents of this directory?
                     /home/you/repo
                    （空行）
     Grok Build may run or modify contents in this directory,
                      posing security risks.
                    （空行）
                  y Yes, proceed      n No, quit
```

- 触发条件：特性开启 + 该目录未信任 + 目录里有仓库级可执行配置（`xai_grok_workspace::folder_trust::decide`，`app/event_loop.rs:344-355`）。
- 警告文案被拆成两行，注释说明是为了在窄/紧凑宽度下不截断（`:1024-1026`）。
- 可跳过性：不可跳过；y/N 由欢迎页的输入拦截器处理（`:1002-1003`）。

### 2.7 ⑦ 正常欢迎页（`:890` → `render_welcome_done`，`:1707`）

见第 3 节。另外启动警告（终端诊断、认证、配置迁移等任何子系统）共用**单槽** banner，画在错误槽位；选取规则是「第一个 Warning 级，否则最后一条」，避免旧的 Info 挡住新的 Warning（`startup.rs:66-73` `banner_warning`，绘制 `views/welcome/mod.rs:2601`）。

## 3. 欢迎页版面：hero box 与 stacked

### 3.1 三种版面

| 版面 | 条件 | 特征 |
| --- | --- | --- |
| 全屏会话选择器 | `session_picker` 可见或加载中 | 整个内容区交给选择器，logo/菜单隐藏，`welcome_compact = true`（`:1713-1715`、`:1879-1899`） |
| hero box | 非紧凑 + 内容宽 ≥ 90 列 + 菜单非空（`HERO_BOX_MIN_WIDTH = 90`，`hero_box.rs:14`；`WelcomeLayout::compute_inner`，`welcome/mod.rs:356+`） | 带边框盒子，logo 在左列、版本/副标题/信息槽/菜单在右列 |
| stacked | 其余情况 | logo 在上、菜单居中、信息槽在下 |

### 3.2 hero box（宽终端）

```text
 ┌──────────────────────────────────────────────────────────────────────┐
 │  ░░░░░░░░░░░          Grok Build v0.x · Team              (:1858)     │
 │  ░ LOGO ░ ░          Thanks for trying Grok Build, …  ← 副标题(可省)  │
 │  ░ braille░          ── 信息槽：公告 或 更新日志（公告优先） ──        │
 │  ░░░░░░░░░                                                           │
 │                      ctrl+w   New worktree                           │
 │                      ctrl+r   Resume session                         │
 │                      (空白)   Changelog      ← 仅 has_access 时插入   │
 │                      ctrl+q   Quit                                   │
 └──────────────────────────────────────────────────────────────────────┘
        （盒下留 1 行 flex gap，然后才是 tip 行 / 输入框 / 版本行）
```

- 盒内边距与列宽：`V_PAD = 1`、`H_INSET = 2`、`LOGO_H_PAD = 3`（`hero_box.rs:17-23`）。
- 副标题 `Thanks for trying Grok Build, give feedback with /feedback!` 只在信息槽为空时占一行，避免盒子变高（`subtitle_rows`，`hero_box.rs:34-38`、`:29`）。
- 盒子放不下时整体回退到 stacked，而不是截断（`min_content_height`，`hero_box.rs:50-61`）。

### 3.3 stacked（窄终端）

```text
      ░░░░░░░░░  ← 按终端高度三档：全高 logo / 小 logo / 不画（logo.rs:16,18）
      ░ braille░
      ░░░░░░░░░
         ctrl+w   New worktree
         ctrl+r   Resume session
         ctrl+q   Quit
      ── 信息槽（公告优先于更新日志）──
   ▸ Tip: <今天的提示>                        ← tip 行（有 tip 时）
   ┌────────────────────────────────┐
   │ > Type a message...            │        ← 输入框（占位符 welcome/prompt.rs:31）
   └────────────────────────────────┘
              Grok Build v0.x · Free          ← 版本徽标
```

- logo 分档：终端高度 ≥ 26 显示全高 braille 图案，≥ 22 显示小图案，低于 22 不画；旧版 Windows 控制台的 ConHost 字体不含 U+2800 盲文块，会渲染成 tofu，因此在那些终端上**整个 logo 隐藏**（`views/welcome/logo.rs:1-3`、`:16-18`、`:36-42`、`:65-67`）。图案本身是 `assets/logo/logo07.txt` / `logo05.txt`（`:12-13`）。
- 菜单统一按 `label … shortcut` 排版，标签左对齐、快捷键右对齐，中间至少留 4 列间隔（`views/welcome/menu.rs:37-47`、`:91-115`）。

### 3.4 行优先级（谁抢谁的槽位）

`render_welcome_done` 先算各块高度再排布，注释把优先级写死在代码里（`welcome/mod.rs:1741-1755`）：

1. 待应用更新提示（`pending_update_version`）；
2. 隐私横幅（高度随宽度换行，`privacy_banner::height`）；
3. 外部会话恢复提示（`foreign_resume_hint`）；
4. 常规 Tip of the Day（`tips::render::tip_height`）。

选择器可见时 tip 高度直接为 0（`:1754-1756`）。菜单行也不是静态的：`Import Claude settings` 行只在检测到待导入的 `.claude/` 配置时插到最前（`:1784-1789`），`Changelog` 行在 `has_access` 时从**第一帧**就在（注释写明是为了等 CDN 拉取时不发生菜单跳动，`:1763-1768`）。

## 4. 空状态与占位符清单

| 场景 | 显示 | 证据 |
| --- | --- | --- |
| 无会话（resume 选择器，原生通道） | 弹出 toast：`No sessions found for this directory`（部分加载失败时改用 `picker_notice`） | `app/dispatch/session/foreign.rs:184-195` |
| 无会话（dashboard 面板） | ` No sessions yet. Esc to dispatch one. ` | `views/dashboard/render.rs:534-537` |
| 选择器无匹配（含文件搜索、历史搜索、设置筛选的通用路径） | `  No matches`，颜色 `gray_dim`，画在内容区首行 | `views/picker.rs:1939-1944` |
| 选择器加载中 | 居中点阵 spinner + `Loading…`（帧按 tick/4 轮转） | `views/picker.rs:1922-1934` |
| 选择器空状态与 spinner 的竞态 | 「空状态必须等 native 与 foreign 两条加载通道都停下来」，且渲染/重绘/tick 三处共用同一判据 | `views/session_picker.rs:152-172` |
| 设置搜索无结果 | `No matches for <query>` | `views/settings_modal/render.rs:462-465` |
| 无消息（欢迎页输入框） | 占位符 `Type a message...` | `views/welcome/prompt.rs:31` |
| 无消息（会话内输入框） | `PromptInputMode::Normal/Bash` 的 `placeholder_override` 返回 `None`，且主输入框聚焦时隐藏占位符 | `app/agent_view/mod.rs:370-381`、`views/prompt_widget/mod.rs:169-173` |
| 无公告 | 会话 banner 高度为 0（槽位不占行） | `views/announcements.rs:321-323` |

**品牌图案**：唯一的手绘图案就是欢迎页的盲文 logo（`assets/logo/logo07.txt`、`logo05.txt`），没有别的 ASCII art；会话内空状态、picker 空状态都只画一行灰字，**未找到**任何空状态插画或大图。

## 5. tips（提示）系统：两套独立的机制

代码里有两个都叫 tip 的东西，**不要混为一谈**。

### 5.1 A 类：欢迎页的 Tip of the Day（内容型）

- 内容来源是四层合并：requirements > remote settings > 用户 config > managed config；任一层设 `exclude_default = true` 会丢掉 remote 层，`[cli] show_tips = false` 直接关掉全部（`crates/codegen/xai-grok-shell/src/util/config/tips.rs:29-77`）。
- 每条 tip **每次启动只显示一条**，由持久化游标顺序推进：读 `~/.grok/tip_cursor.json` 的 `cursor`，取 `tips[cursor % len]`，写回 `cursor + 1`，全部轮完再循环（`crates/codegen/xai-grok-shell-base/src/util/tips.rs:11-52`）。
- 调用点在启动装配处（`app/event_loop.rs:1424-1433`，登录/设置变化后同样重取 `app/acp_handler/settings.rs:325-327`）。
- 绘制方式：`Tip: ` 前缀（gray + 粗体）+ 正文（gray），按宽度自动换行并预留行数（`tips/render.rs:13-71`；欢迎页调用见 `views/welcome/mod.rs:638-651`）。
- 骚扰控制很直接：用户在欢迎页**只要一开始打字**，本行的 tip 就永久隐藏到本次进程结束（`welcome_tip_typing_dismissed`，`app/app_view.rs:4471-4478`）。

```text
                    ▸ 已排好版的信息槽 / 更新日志
   Tip: Shift+Tab cycles plan mode          ← tip 行（90 tick 的排版高度已预留）
   ┌────────────────────────────────┐
   │ > Type a message...            │
   └────────────────────────────────┘
```

### 5.2 B 类：会话内的 ephemeral hint（上下文型，`src/tips/`）

核心原语是一个**单槽 + TTL + 去重键**的提示行，画在输入框上方的 banner 行（`tips/ephemeral.rs:18-34`）：

```rust
// src/tips/ephemeral.rs:10
pub const DEFAULT_TIP_TICKS: u16 = 90;   // ≈3s @30fps；到期判定在减 1 之前，所以 N 值实际存活 N+1 tick

pub struct EphemeralTip {
    pub key: &'static str,                  // 去重键：同 key 再 show 只刷新 TTL，不叠加
    pub line: Line<'static>,                // 预着色的文本 + 高亮按键
    pub ticks_remaining: u16,
    pub session_seen: Option<(&'static str, u32)>, // (计数键, 上限)：本次会话展示到上限后不再出现
    pub ambient: bool,                      // 与草稿无关的提示：提交不销毁，被遮挡时暂停倒计时
}
```

`src/tips/` 下每个文件负责一条提示：

| 文件 | 提示内容 | 触发条件 | 上限/会话 | TTL（tick） | ambient |
| --- | --- | --- | --- | --- | --- |
| `clear_detector.rs` | `Input cleared · ctrl+z to undo` | 草稿峰值 ≥ 20 字符后被清空到 ≤ 5 字符（`FIRE_PEAK_LEN`/`FIRE_RESIDUE_LEN`，`:18-24`） | 3 | 90 | 否 |
| `plan_nudge.rs` | `Planning? Check out plan mode via shift+tab` | prompt 里出现整词命中的规划关键词（`PLANNING_KEYWORDS`，`:22-33`）；且非 plan 模式、会话空闲、输入模式为 Normal（`agent_view/notices.rs:62-69`） | 3 | 90 | 否 |
| `clipboard_focus.rs` | （描述）ctrl+v 可粘贴剪贴板里的图片 | 终端聚焦 + 当前模型接受图片 + 未附图片；机会式轮询：1s 节流（`POLL_INTERVAL:31`）+ 30s 冷却（`FIRE_COOLDOWN:34`）+ changeCount 去重 | 不用计数（冷却+去重） | 90 | 否 |
| `send_now.rs` | `Queued · Enter to send now` | 回合进行中排队跟进后 | 3 | 90 | 否 |
| `small_screen.rs` | `Tight on space? Try /compact-mode` | 终端行数落在 `(AUTO_COMPACT_MAX_ROWS=20, 28]` 区间且未开 compact mode（`views/agent.rs:97`、`small_screen.rs:23-28`） | 1 | 90 | **是** |
| `ssh_wrap.rs` | （描述）推荐 `grok wrap ssh` / 引导 `/doctor` | 首个稳定 agent 绘制时由 `diagnostics::ssh_wrap_hint` 判定环境 | 1 | 300 | **是** |
| `word_select.rs` | （描述）双击选词可改设置项 | text selection 仍为 fold/nav 时在 scrollback 双击 | 3 | 600 | **是** |
| `export_copy.rs` | （描述）拖拽复制后提示导出/复制 | 15s 窗口内 ≥ 3 次拖拽复制（`WINDOW:18`、`MIN_COPIES:19`），先 armed 再于 toast 结束后落屏（`:25-26`） | 3 | 600 | 否 |

（TTL 与上限的常量位置：`clear_detector.rs:18`、`plan_nudge.rs:18`、`send_now.rs:11`、`small_screen.rs:20`、`ssh_wrap.rs:21,26`、`word_select.rs:16,21`、`export_copy.rs:14,16`。）

### 5.3 防骚扰的四层机制

1. **展示门禁（不白烧配额）**：`show_ephemeral_tip` 先过 `ephemeral_tip_renderable`，不可绘制就直接返回 `false`——不计 seen、不烧 TTL、不发遥测（`app/agent_view/notices.rs:28-37`）。不可绘制的条件包括：权限询问队列非空、question view、任何 active modal、隐私横幅激活、subagent 全屏接管、line/image/video/block viewer、gboom、extensions/agents modal、goal detail、workflows、prompt 下拉打开、会话 announcement banner 占用，以及终端尺寸未知/过矮（`notices.rs:107-137`；矮终端阈值由 `tip_row_renderable` 与 `SHORT_TERMINAL_ROWS` 共享，16 行及以下不显示、17 行起显示，见测试 `tips/ephemeral.rs:361-373`）。
2. **TTL 只在可见时燃烧**：`ephemeral_tip_needs_tick` 在会话 banner 或隐私横幅占用槽位时停表；ambient 提示在行不可绘制时暂停而非扣减（`notices.rs:73-102`）。
3. **会话级配额**：`session_seen = Some((key, cap))`，计数存在 `AppView::tip_seen_counts`（`HashMap<&'static str, u32>`），**只在内存、每次运行重置**；同 key 重放只刷新 TTL，不重复计数（`tips/ephemeral.rs:27-29`、`:77-101`）。
4. **用户级开关**：8 个 `[ui.contextual_hints]` 布尔项（`undo` / `plan_mode` / `image_input` / `send_now` / `small_screen` / `word_select` / `export_copy` / `ssh_wrap`），`None` 继承（默认开），`Some(bool)` 是用户显式选择并压过 remote 默认（`crates/codegen/xai-grok-shared/src/ui_config.rs:184-230`；remote 侧同形状 `crates/codegen/xai-grok-config-types/src/lib.rs:935-960`；读取默认值 `settings/registry.rs:481-504`）。每个 dispatch 入口都先查开关（`app/dispatch/prompt.rs:428-431`、`:451-454`、`:472-475`，export/copy 用 gate 参数 `:528-535`）。

遥测上，每次落屏记 `tip.shown`，离开槽位记 `tip.dismissed` 并带原因 `replaced` / `expired` / `cleared`（`tips/ephemeral.rs:180-197`）；印象（impression）只在真正落屏时记录（`dispatch/prompt.rs:438-447`）。

## 6. announcements（公告）

### 6.1 两个严重级与槽位抢占

- 只有两个严重级会进入槽位：`severity == "critical"` 与 `severity == "promo"`；`info` / `warning` 只上欢迎页，不开会话槽（`views/announcements.rs:109-115`、`:203`）。
- 会话槽的选择是**critical 永远压过 promo**，且每帧重新推导，所以 promo 显示中来了 critical 会自动换（`:236-243`）。
- 槽位排序：critical 压过隐私横幅，promo 不压（注释：故障通知不能被长期 nag 挡掉，`:245-248`）。
- 高度：critical 2 行（标题行 + 消息行），promo 1 行，无公告为 0（`:321-333`）。

```text
critical（dismissible）
 ! 服务中断说明                                          hide: /announcements hide  [hide]
   消息正文…
promo（pinned，不可隐藏）
 [Get SuperGrok]  or use Ctrl+O
promo（dismissible）
 [Get SuperGrok]                    hide: /announcements hide  [hide]
```

（布局细节与「消息行缩进对齐标题列」「按钮宽度先预留、文本先截断」的规则见模块文档 `views/announcements.rs:1-23` 与 `:383-410`。）

### 6.2 去重与持久化

- 隐藏键：优先用 `id`（去空格后非空），否则用 `content:<title>\u{1f}<message>` 的合成键，保证无 id 的公告也能被记住（`crates/codegen/xai-grok-announcements/src/lib.rs:78-88`）。
- 持久化形状是 `{"hidden_ids": [...]}` 的 `BTreeSet<String>`，确定性顺序保证文件稳定；旧版 `{"hidden": bool}` 会退化成空集并重新显示一次（`:92-112`）。
- 过期公告的键只在真实更新路径上清理（`prune_hidden_announcement_ids`，注释明确禁止逐帧清理，`:115-125`）。
- `dismissible: false` 会抑制两种公告上的所有隐藏入口，并让文本收回预留列；隐藏集合只对可隐藏项生效，服务端把标记翻回 `true` 就能「复活」之前被隐藏的 banner（`views/announcements.rs:144-159`）。
- 逃生口：`/announcements show` 清空所有存活公告的隐藏键，且该命令的可用性判据**故意忽略隐藏集合**，因此全隐藏时仍能自救（`:290-318`）。
- 点击 `[hide]` 与 `/announcements hide` 完全等价（`app/agent_view/mod.rs:1091-1092`、`app/agent_view/links.rs:576-578`）。

### 6.3 欢迎页与会话内的分工

欢迎页的公告取值链是三级兜底：promo 的 CTA 所有者 → 会话槽选择（critical 优先）→ 随机一条 active announcement（`app/app_view.rs:4486-4498`，随机挑选见 `app/event_loop.rs:1420`）。呈现分工：

- **欢迎页 hero**：空间充裕，画 promo 的 `message` 正文，可点击展开长公告（`hero_box.rs:465-466`），并把升级 CTA 占两行预留；
- **会话内 banner**：promo 只画 1 行按钮/隐藏入口，**不画 message 正文**（`views/announcements.rs:20`）；
- 窄版欢迎页也有信息槽：公告优先于更新日志，高度按槽宽测量（`welcome/mod.rs:1981-2010`、`render_announcement_section` `:1666`）。

## 7. 隐私横幅（coding-data sharing upsell）

- 可见性由 6 重门禁决定：非 minimal 屏、命中 rollout 分群、非 ZDR 且非 team 非管理员、没有正在进行的写入、当前是 opt-out 状态、且认证/权限/信任全部就绪（`app/app_view.rs:1219-1248`）。
- 已确认（acked）后按天数重新显示：`None` 或 `0` 表示永不再现；解析失败按「该重现」处理（`privacy_banner_reshow_elapsed`，`app/app_view.rs:1140-1152`）。
- 版面：标题 `Help improve Grok` 与右侧 `[Opt out]` / `[Opt in]` 共享一行，下面是要点正文，再下面是法务行（`views/privacy_banner.rs:12-19`）。

```text
 Help improve Grok                            [Opt out] [Opt in]
 Off by default. Opt-in to allow SpaceXAI to retain coding data, …
 Read Terms and Privacy Policy.
```

- 三个法务行变体**从宽到窄挑第一个能整行放下的**，绝不裁剪，避免命中矩形压在被裁掉的链接文字上；按钮要么整块画要么不画，防止出现「空白处的幽灵点击区」把用户静默 opt-in（`views/privacy_banner.rs:25-43`、`:88-98`）。
- 正文最多 4 行，超出用 `…` 省略；若按钮列会让正文超过 3 行，就放弃按钮列改用全宽（`:53-58`、`:108-120`）。
- 槽位与欢迎页 tip 行共用（`welcome/mod.rs:1746-1750`），在会话内它是 ephemeral tip 行的**长期遮挡者**（`agent_view/notices.rs:111-112`）。

## 8. 渐进式披露：代码里的具体体现

| 功能 | 何时才出现 | 证据 |
| --- | --- | --- |
| 菜单项 `Import Claude settings` | 仅在检测到待导入的 `.claude/` 配置时插到菜单最前 | `views/welcome/mod.rs:1784-1789` |
| 菜单项 `Changelog` | 仅 `has_access`；但**从第一帧就在**，以免 CDN 回包后菜单跳动 | `welcome/mod.rs:1763-1768` |
| 更新日志区块 | 终端太矮/紧凑/被 tip 抢空间时整块消失 | `WelcomeLayout::effective_changelog`，`welcome/mod.rs:327-340` |
| 公告长文本 | 默认折叠到 2 行，点开才内联展开 | `hero_box.rs:465-466`、`welcome/mod.rs:1752` |
| hero 副标题 | 信息槽（公告/更新日志）有内容时让位 | `hero_box.rs:34-38` |
| 工作区模式分段控件 | 仅 chat 模式且 `has_access` | `welcome/mod.rs:1802-1806`（`local-workspace` feature） |
| `/tutorial` | 纯手动；minimal 模式直接不可用（`FullscreenOnly`） | `slash/commands/tutorial.rs:20-23` |
| 设置页的 8 个 contextual hints | 顶层只显示一个 `Show contextual hints` 组行，子项藏在子页里，靠关键词索引串起来 | `settings/defs.rs:456-467`、`:1259-1301` |
| 设置项在 minimal 模式的可见性 | 每项带 `hidden_in_minimal` 标记 | `settings/defs.rs:489`、`:1257` |
| 快捷键手册 | 注册表里**所有** action 都在，只是当前上下文不生效的绑定变暗而非隐藏；分类按新手友好顺序 | `views/shortcuts_help.rs:127-130`、`:63-72` |
| 下一句 prompt 幽灵补全 | 回合结束后才有；Esc 当回合不再提示；输入分歧文本即隐藏 | `views/prompt_suggestion.rs:1-13`、`:78-104`（env `GROK_PROMPT_SUGGESTIONS`，`:18`） |
| 8 条上下文提示本身 | 默认开启但可逐条关闭；展示次数按会话封顶 | 见第 5.3 节 |

`prompt_suggestion` 的可见性刻意做成**每帧从当前输入推导**（`ghost_for`），而不是按键驱动状态机，避免状态漂移；接受键是 Tab / 右方向键，仅当光标在文末时出现（`views/prompt_suggestion.rs:1-13`、`:78-85`）。

## 9. 引导与文档的联动

- **教程内容是编译期嵌入的 markdown**：`tutorial_docs.rs` 用 `include_str!("../docs/tutorial/…")` 把 9 个主题打进二进制，每项由 `title` / `blurb` / `content` / `go_deeper` 组成（`tutorial_docs.rs:19-28`、`:32-87`）。
- 主题顺序被显式设计成一条线性流程：从别的工具迁移 → 第一条 prompt → 附加上下文 → 认识界面 → 更大的功能（`tutorial_docs.rs:30-31`），overlay 里 `→` 就按这个顺序推进。
- 测试强制：每页 ≤ 50 行（`topics_stay_bite_size`，`:130-142`）、`go_deeper` 的标题必须能解析到真实指南（`:108-119`）、标题唯一（`:122-127`）。
- overlay 有两个屏 + 一个跳转屏：`List` / `Topic { index }` / `Guide { topic }`（`views/tutorial.rs:33-40`）；列表显示 `N/9 explored` 进度（`:387`），主题页 footer 有 `→ next: <标题>`（末页为 `→ done`）、`d go deeper`、`Esc list`（`:320-355`）；主题正文会剥掉首个 `# ` 标题，避免与 modal 标题重复（`topic_body`，`:305-311`）。
- `/tutorial` 是唯一入口（别名 `tour`、`onboarding`），命令注释写明「绝不自动弹出」（`slash/commands/tutorial.rs:1-8`、`:10-11`）。
- 更深的文档走另一套嵌入表：`docs.rs` 里 `USER_GUIDE` / `REFERENCE_DOCS` 同样用 `include_str!` 指向 `docs/user-guide/*.md`，第一条就是 `Getting Started`（`docs.rs:1-8`、`:40-60`）；`/docs` 打开 DocPicker、`/docs web` 跳浏览器、`/docs <title>` 直达单篇（`slash/commands/docs.rs:1-10`）。
- 教程的最后一页负责把用户推向这些出口：`/help`（或 Ctrl+P）、`/docs`、直接问模型、`grok -c` 续会话、`/tutorial` 可随时重开（`docs/tutorial/09-where-next.md`）。
- 官方用户指南也把首启写进文档：首次启动会打开浏览器完成 grok.com 认证，凭证存 `~/.grok/auth.json` 并自动刷新（`docs/user-guide/01-getting-started.md:74`）。

## 关键结论（供合稿引用）

1. 首屏是**一个欢迎页上的六道短路门禁**，顺序即优先级：登录 → 认证中 → ZDR → 订阅闸门 → 账号级同意 → 工作区级信任 → 正常欢迎页；三者全 `Done` 才允许创建会话。证据：`crates/codegen/xai-grok-pager/src/views/welcome/mod.rs:758-903`、`src/app/app_view.rs:1252-1256`。
2. 门禁屏的「可读性」是硬门禁而非尽力而为：同意声明正文限 12 行 / 78 列，装不下就撤掉 Accept 只留 Quit 并提示放大窗口。证据：`src/views/welcome/consent.rs:21-30`、`:65-80`、`src/app/consent.rs:16-20`。
3. 欢迎页有两套版面：内容宽 ≥ 90 列且高度够时用带边框 hero box（logo 左、版本/信息槽/菜单右），否则退化为 stacked；放不下时**降级而不是截断**。唯一的手绘品牌图案是 braille logo，按终端高度分全高 / 小 / 不画三档（≥26 / ≥22 / 更低），旧版 Windows 控制台因字体缺 U+2800 而整体隐藏。证据：`src/views/welcome/hero_box.rs:14,50-61`、`src/views/welcome/mod.rs:356-495`、`:1941-1960`、`src/views/welcome/logo.rs:1-3,16-18,36-42`。
4. 欢迎页的 tip 行有明确的槽位优先级：待更新提示 > 隐私横幅 > 外部会话恢复提示 > 常规 tip；选择器打开时一律让位。证据：`src/views/welcome/mod.rs:1741-1756`。
5. Tip of the Day（欢迎页，四层配置合并 + `~/.grok/tip_cursor.json` 持久游标，每次启动一条）与会话内 ephemeral hint（`src/tips/`）是**两套互不相干的系统**；前者用户一开始打字就隐藏。证据：`crates/codegen/xai-grok-shell-base/src/util/tips.rs:41-52`、`crates/codegen/xai-grok-shell/src/util/config/tips.rs:29-77`、`src/app/app_view.rs:4471-4478`。
6. ephemeral tip 的防骚扰是四层：不可绘制时不计配额不烧 TTL（`ephemeral_tip_renderable` 的十余种遮挡判定）→ TTL 只在可见时燃烧（ambient 提示被遮挡时暂停）→ 会话内 seen 上限（内存计数，3 或 1 次）→ 8 个 `[ui.contextual_hints]` 用户开关。证据：`src/app/agent_view/notices.rs:28-37,73-137`、`src/tips/ephemeral.rs:10,27-33`、`crates/codegen/xai-grok-shared/src/ui_config.rs:184-230`。
7. 每条提示的文案里的按键**从真实绑定派生**（如 `ctrl+z` 由 `is_undo_key` 认可的 `KeyShortcut` 生成、`shift+tab` 取 `shift_tab_keys()[0]`），避免文档与实现漂移。证据：`src/tips/clear_detector.rs:29-35`、`src/tips/plan_nudge.rs:37-43`。
8. 公告只有 `critical` / `promo` 两个严重级进入槽位，critical 永远赢；`info`/`warning` 仅上欢迎页。证据：`src/views/announcements.rs:109-115,203,236-248`。
9. 公告去重键优先用 `id`、否则用 `content:<title>\x1f<message>` 合成，持久化为 `{"hidden_ids": [...]}`；`dismissible:false` 会关掉所有隐藏入口，而 `/announcements show` 故意忽略隐藏集合以保证自救路径。证据：`crates/codegen/xai-grok-announcements/src/lib.rs:78-112,115-125`、`src/views/announcements.rs:144-159,290-318`。
10. 隐私横幅有 6 重门禁 + 「acked 后 N 天再现」的重提醒策略（`None`/`0` = 永不）；法务行与按钮「整块放不下就不画」，防止幽灵点击区导致静默 opt-in。证据：`src/app/app_view.rs:1140-1152,1219-1248`、`src/views/privacy_banner.rs:25-43,88-98`。
11. 空状态统一走「一行灰字 + 通用选择器」：无匹配是 `No matches`，无会话在 dashboard 是 `No sessions yet. Esc to dispatch one.`、在选择器里是 toast `No sessions found for this directory`；加载中显示居中点阵 spinner，且空状态要等 native / foreign 两条加载通道都停。证据：`src/views/picker.rs:1922-1944`、`src/views/dashboard/render.rs:534-537`、`src/app/dispatch/session/foreign.rs:184-195`、`src/views/session_picker.rs:152-172`。
12. 渐进式披露的三种手法：**变暗而非隐藏**（快捷键手册把上下文不生效的绑定 dim 掉，注册表全量可见）、**置位才出现**（Import Claude 行、工作区分段控件、`Changelog` 行）、**子项收进子页**（设置里的 contextual hints 组）。教程则相反地走「绝不自动弹出 + 编译期嵌入 markdown + 强约束测试（每页 ≤50 行、`go_deeper` 必须解析到真实指南）」。证据：`src/views/shortcuts_help.rs:63-72,127-130`、`src/views/welcome/mod.rs:1763-1768,1784-1789,1802-1806`、`src/settings/defs.rs:456-467,1259-1301`、`src/tutorial_docs.rs:19-28,32-87,108-142`、`src/slash/commands/tutorial.rs:1-23`。
