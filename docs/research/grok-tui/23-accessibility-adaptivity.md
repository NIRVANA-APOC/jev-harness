# 专题 23：无障碍、自适应与终端能力协商

> 专题：无障碍、自适应与终端能力协商｜调研日期：2026-09-22｜证据范围：`crates/codegen/xai-grok-pager/docs/user-guide/{21-terminal-support,03-keyboard-shortcuts,06-theming,26-config-reference}.md`、`crates/codegen/xai-grok-pager/src/{tips/small_screen.rs,tips/ephemeral.rs,views/agent.rs,views/turn_status.rs,views/fps_hud.rs,views/modal_window.rs,wrap_filter.rs,wrap_restore.rs,app/mod.rs,app/mouse.rs,app/modals.rs,app/mode_switch.rs,app/screen_mode_relaunch.rs,app/x10_filter.rs,app/xt_filter.rs,app/csi_filter.rs,diagnostics/mod.rs,notifications/config.rs,minimal/}`、`crates/codegen/xai-grok-pager-minimal/src/live.rs`、`crates/codegen/xai-grok-pager-render/src/{theme/color_support.rs,theme/cache.rs,theme/mod.rs,terminal/mod.rs,terminal/keyboard.rs,terminal/kitty_keyboard.rs,terminal/tmux.rs,terminal/image.rs,terminal/hyperlinks.rs,glyphs.rs,host/mod.rs,render/bidi.rs,util.rs}`、`crates/codegen/xai-ratatui-textarea/src/`

## 1. 总览：探测 → 分类 → 降级

Grok Build 没有「一个开关适配所有环境」的写法。整条链是三步：启动时**从环境变量探测**，把结果压成**若干枚举与能力位**，再由各功能读枚举做降级决策。消费者从不直接 `match` 环境变量。

```
环境变量 ──► detect_terminal_brand_from_env()   terminal/mod.rs:621
             detect_byobu_from_env()            terminal/mod.rs:741
             tmux -V / control-mode 探测          terminal/mod.rs:913
             DA2 / XTVERSION 回复                 terminal/da2.rs, terminal/xtversion.rs
                        │
                        ▼
             TerminalContext { brand, env_brand, multiplexer,
                               byobu, embedded_editor, is_ssh, ... }   terminal/mod.rs:225
                        │
        ┌───────────────┼────────────────┬──────────────────┐
        ▼               ▼                ▼                  ▼
  KeyboardCaps    HyperlinkCaps     ColorLevel        AltScreenMode
  keyboard.rs     hyperlinks.rs     color_support.rs  terminal/mod.rs:900
```

一个关键设计：`TerminalContext` 同时保留 `brand` 与 `env_brand`（`terminal/mod.rs:228-232`）。原因是原生 Windows 上「未识别」会被乐观地当成 `WindowsTerminal`（`refine_unknown_brand_for_host`，`terminal/mod.rs:725-730`），于是**任何必须默认拒绝的判断都要回查 `env_brand`**——旧版 ConHost 的 ASCII 字形降级与未识别终端的 `Shift+Enter` 门控都走这条路（注释见 `glyphs.rs:381-383`、`terminal/mod.rs:411-413`）。

## 2. 终端能力矩阵

### 2.1 官方明示的清单

`docs/user-guide/21-terminal-support.md:51-76` 列出 15 类**被识别**的终端：Apple Terminal、Ghostty、iTerm2、Warp、WezTerm、Kitty、Alacritty、Rio、foot、VS Code / Cursor / Windsurf / Zed 集成终端、JetBrains IDE 终端、Grok Desktop、VTE 系（GNOME Terminal / GNOME Console / Tilix）、Windows Terminal。同处明确列出三条**识别限制**：tmux 内可能拿不到外层终端变量；SSH 不转发多数终端变量；tmux 的 global environment 反映**第一个**接入的客户端，未必是当前客户端。

### 2.2 代码里的分类比文档更细

`TerminalName` 共 20 个变体（`terminal/mod.rs:58-107`），比文档多出 `Terminator`（Python/GTK，VTE 系）与 `Otty`（macOS，会把 IME 提交包成 bracketed paste），以及 `Unknown`。分类谓词见 `terminal/mod.rs:109-160`：

- `is_vte_based()` / `is_vscode_family()` / `is_xtermjs_embed()`：把「同一类怪癖」的终端归组，避免在每个调用点重写 `match`。
- `is_capability_unclassified()`：`Unknown | Otty`，二者共享 `Unknown` 的 fail-closed 姿态（不探测 KKP、超链接/通知/焦点一律保守）。
- `supports_osc52_clipboard()`：默认 false，只有显式列出的品牌才是 true。
- `delivers_ime_as_bracketed_paste()`：只有 Otty 为 true。

### 2.3 检测优先级

`detect_terminal_brand_from_env` 的顺序（`terminal/mod.rs:621-723`）本身表达了可靠性排序：

1. fork 专属标记（`CURSOR_TRACE_ID`、`VSCODE_GIT_ASKPASS_MAIN`）——它们即使在 SSH / tmux 下也存活，而 `TERM_PROGRAM` 不一定；
2. `TERM_PROGRAM`（最可靠的单信号，归一化映射见 `terminal/mod.rs:871-896`）；
3. `TERMINAL_EMULATOR`（JetBrains / JediTerm，注释说明必须排在 `TERM_SESSION_ID` 之前，否则会误判成 Apple Terminal）；
4. 品牌专属变量：`WEZTERM_VERSION` → `ITERM_SESSION_ID` / `LC_TERMINAL`（注释指出 `LC_TERMINAL` 能穿过 SSH 的 `SendEnv/AcceptEnv LC_*`）→ `TERM_SESSION_ID` → `KITTY_WINDOW_ID` → `ALACRITTY_SOCKET` → `TERM` 精确匹配（`rio`、`foot`/`foot-extra`/`foot-direct`）；
5. `TERMINATOR_UUID`（必须先于 VTE 兜底）；
6. `VTE_VERSION` 兜底为 `Vte`；
7. `WT_SESSION` → `WindowsTerminal`；
8. 否则 `Unknown`。

### 2.4 每类终端上的行为差异

| 终端 / 环境 | 代码依据 | 行为差异 |
| --- | --- | --- |
| Apple Terminal | `keyboard.rs:102-104`；`terminal/mod.rs:138-154` | `Cmd+Bsp` 被窗口管理器截获（`Unrecoverable`），`Opt` 与修饰 Enter 为 `Dropped` 但可由 CoreGraphics 抢救；不支持 OSC 52，远端复制退回备份文件 `~/.grok/last-copy.txt`（doc 21:142-147）；用 `Ctrl+O` 代替 `Ctrl+Enter` 插话（doc 21:202-203） |
| VS Code / Cursor / Windsurf / Zed | `terminal/mod.rs:318-324`、`:402-410` | **从不协商** Kitty 键盘协议（xterm.js 对 shifted 可打印键编码有误，issue 5823），`Shift+Enter` 与 `Enter` 同码，UI 改推 `Alt+Enter` |
| WezTerm | doc 21:197-203 | 出厂关闭 Kitty 键盘协议，`/doctor` 给出 `terminal.wezterm-kitty` 设置项 |
| Kitty / Ghostty / foot | `keyboard.rs:88-90`；`terminal/mod.rs:81-84` | 修饰键 `Native`，无需抢救；纳入 truecolor 品牌白名单（`color_support.rs:250-269`） |
| Alacritty / Rio | `keyboard.rs:99-100` | `Cmd` 原生（映射到 `^U`），`Opt` 被丢弃可抢救 |
| Warp | `terminal/image.rs:85-95` | 接受部分 Kitty graphics 转义但不支持 placement/scrollback 模型，会留下残影，故不启用内联图像 |
| JetBrains（JediTerm） | `terminal/mod.rs:85-89`、`keyboard.rs:108-110`、`terminal/mod.rs:736-738` | 无 `TERM_FEATURES`、XTVERSION 会泄漏、DA1 只是裸 VT102，**无法运行时探测**，能力全部 `Unknown`；Windows 上鼠标上报会漏成原始文本，该组合**默认进 minimal 模式**（`app/mod.rs:939-953`） |
| VTE 系 / Terminator | `terminal/mod.rs:388-400` | `VTE_VERSION < 8200` 或无法解析版本时，判定 `Shift+Enter` 不可用 |
| Windows Terminal | `color_support.rs:266-268` | 原生 Windows 默认假定 ConHost 已开 VT 处理，直接认定 truecolor；未识别品牌也如此 |
| 旧版 ConHost | `glyphs.rs:1-11`、`:377-391`、`:148-159` | 字形降级为 ASCII / CP437（`❯`→`> `、`✗`→`x`、braille spinner→`|/-\`）；有 `GROK_FORCE_LEGACY_CONSOLE` 强制开关 |
| Zellij | `terminal/mod.rs:194-197`、`:968-971` | 拦截 CSI 查询（外围探测失效）；auto 模式下**自动关闭备用屏**，退回 inline |
| tmux | `terminal/tmux.rs:55-57`、`terminal/mod.rs:374-379`、`:356-372` | 不进同步输出（synchronized output）；tmux 内**禁用**内联图像；`tmux < 3.3` 或 `extended-keys = off` 时 `Shift+Enter` 退化 |
| GNU screen | `terminal/mod.rs:356-360` | 同上，直接判 `kitty_skip_reason = "screen"` |
| Byobu + GNU screen | doc 21:260-263 | 支持有限，`/doctor` 报 `terminal.byobu-screen` 并建议切 tmux 后端 |
| Otty | `terminal/mod.rs:156-159` | 唯一把 IME 提交包成 bracketed paste 的品牌 |
| 未识别（`Unknown`） | `terminal/mod.rs:340-348`、`:414-416` | 无正证据即跳过 KKP；`/doctor` 报通知协议回退到响铃 |
| 原生 Windows（ConPTY） | `terminal/image.rs:141-144` | ConPTY 会剥掉 Kitty/iTerm2 的 APC 转义，故内联图像协议返回 `None` |

备用屏策略是独立的一个轴，`AltScreenMode { Auto, Always, Never }`（`terminal/mod.rs:898-908`），解析逻辑在 `determine_alt_screen_policy`（`terminal/mod.rs:955-983`）：CLI 的 `--no-alt-screen` 优先级最高；`Auto` 在 Zellij 与 tmux control mode 下**自动退回 inline**，其余情况默认进备用屏。

## 3. 尺寸自适应

### 3.1 三个高度阈值

全部阈值集中在 `views/agent.rs:89-98`，并有编译期断言把它们的关系钉死：

```rust
pub const SHORT_TERMINAL_ROWS: u16 = 16;      // ≤16：砍掉提示区可选行
pub const SCROLLBACK_MIN_ROWS: u16 = 5;       // scrollback 的地板
pub const AUTO_COMPACT_MAX_ROWS: u16 = 20;    // ≤20：强制 compact 渲染
const _: () = assert!(SHORT_TERMINAL_ROWS < AUTO_COMPACT_MAX_ROWS);
```

自动 compact 的判定是纯函数（`views/agent.rs:99-104`），且有一条重要约束：**结果从不回写** `current_ui.compact_mode`、渲染缓存或磁盘，所以窗口拉高后用户自己的选择立刻恢复；`terminal_rows == 0` 表示「还没测量」，永不强制 compact。

### 3.2 高度触发的降级

```
行数   0        16              20            28          ∞
       ├─────────┼───────────────┼─────────────┼───────────┤
       │ 深降级区 │  自动 compact  │  小屏提示区  │  默认布局 │
       │         │  （含 16 区）  │             │           │
```

- **≤16（`SHORT_TERMINAL_ROWS`）**：底部垂直 padding 归零、插件 CTA 行归零、follow-up 建议行归零（`views/agent.rs:205-217`）；prompt 上方的 gap 行归零（`app/agent_view/render.rs:1076-1084`）；面板 dock 关闭（`app/agent_view/render.rs:1010-1013`）；ephemeral tip 行**不再渲染**（`tips/ephemeral.rs:13-16`）。设计意图写在注释里：保证 prompt 与 scrollback 不被饿死。
- **≤20（`AUTO_COMPACT_MAX_ROWS`）**：渲染层收到 `compact = true`（`views/agent.rs:102-104`），走更密的 padding。
- **21–28**：进入小屏提示带（下节）。

整屏布局用 ratatui 约束求解，`SCROLLBACK_MIN_ROWS` 是布局里**唯一的 `Min`**，优先级高于所有 `Length`，所以「行数不够时被压缩的是别的行，不是 scrollback」（`views/agent.rs:92-94`）。

### 3.3 小屏提示

`tips/small_screen.rs` 是一个完整的、可复用的降级通告范例：

```rust
const SMALL_SCREEN_TIP_MAX_ROWS: u16 = 28;              // :23
pub fn small_screen_band_contains(rows: u16) -> bool {   // :27-29
    (AUTO_COMPACT_MAX_ROWS + 1..=SMALL_SCREEN_TIP_MAX_ROWS).contains(&rows)
}
```

提示文案为 `Tight on space? Try /compact-mode`（`:41-47`），会话内**最多显示一次**（`SMALL_SCREEN_TIP_SEEN_CAP: u32 = 1`，`:20`），且标记为 `ambient()`（`:49`）——提交 prompt 不会让它消失，被遮挡时 TTL 暂停计时，避免「快速回车导致提示闪一下就没了」（模块注释 `:1-4`、`:31-33`）。显示时机是**首次稳定绘制**，不在后续 resize 时重放。

### 3.4 宽度触发什么

未找到 agent 视图层面的宽度阈值。宽度自适应是**逐组件**的：

- 模态窗口有 `min_width` 地板（`views/modal_window.rs:96`），各调用方取值 40 / 44 / 50 / 60（`extensions_modal.rs:3694`、`agents_modal.rs:1014`、`usage_modal.rs:586`、`persona_detail.rs:656`、`tutorial.rs:417`、`modal_window.rs:113/129/1271`）；地板值最后会被**夹回缓冲区**，避免窄终端下越界（`modal_window.rs:579-582`，并有回归测试 `:1308`）。
- 宽度不足以渲染时返回 0 / 跳过绘制（`modal_window.rs:559`），并清掉悬停与拖拽状态，防止上一帧的陈旧矩形继续吃点击（`usage_modal.rs:618`、`file_search/line_viewer.rs:1828`）。
- 仪表板头部在放不下 cwd + 提示时**先丢提示再截断路径**（`views/dashboard/chrome.rs:259` 附近的测试 `width` 说明）。

## 4. 亮度、对比与色觉

### 4.1 四级色彩能力

`ColorLevel` 是有序枚举，从低到高：`None < Basic(ANSI 16) < Ansi256 < TrueColor`（`color_support.rs:17-30`），配套 `has_color / has_256 / has_truecolor`（`:32-44`）。检测结果缓存在全局 `OnceLock`，启动时调 `detect()`，其余处处读 `get()`（模块注释 `:1-4`）。

检测优先级（`color_support.rs:83-127`）：

1. **`NO_COLOR` 直接判 `None`**，注释写明「显式退出优先」（`:98-101`）；
2. 非 TTY（无 `supports_color` 结果）默认 TrueColor，理由是「TUI 总归跑在终端里」（`:115-117`）；
3. 用 `supports-color` crate 读 stdout，映射到四个档位；
4. 若 crate 只报 256 色，但品牌能识别且已知支持 24-bit，**升级为 TrueColor**（`:119-123`）——注释解释 tmux/SSH/mosh 常把 `COLORTERM` 剥掉，不补这一步会让主题被量化到 16 色，导致背景/边框/弱化色的层次全部塌在一起（`:243-245`）。
5. 终端原生锁定（minimal 模式或选了 `terminal` 主题）时，结果**上限压到 `Basic`**（`:85-88`；锁定义在 `theme/cache.rs:105-120`）。

另有一条独立路径 `standalone()`（`color_support.rs:138-203`），专门服务 `grok doctor --json`：它**不看 stdout**（因为 doctor 常被管道接走），只看 stderr 是否为终端或 `/dev/tty`（`CONIN$` on Windows）能否打开；`TERM` 为空或 `dumb` 时老实回答「无色彩证据」。

### 4.2 降级管线

```rust
pub fn quantize_color(color: Color, level: ColorLevel) -> Color {   // color_support.rs:220-234
    match level {
        ColorLevel::TrueColor => color,
        ColorLevel::Ansi256  => RGB → Color::Indexed(nearest_indexed(r,g,b)),
        ColorLevel::Basic    => RGB/Indexed → indexed_to_ansi16(nearest_indexed(..)),
        ColorLevel::None     => Color::Reset,
    }
}
```

`indexed_to_ansi16`（`:274-299`）对 0–15 直接一一对应，16–255 先转 RGB 再按**平方欧氏距离**找最近 ANSI 16（`rgb_to_ansi16`，`:302-336`，调色板值内联在 `:304-321`）。`None` 档把所有颜色压成 `Color::Reset`，这一点在别处被显式防住：主题层注释说必须用 `has_color()` 判断，否则「named-ANSI 重绘会把 `Reset` 又染回去，使 `NO_COLOR` 失效」（`theme/mod.rs:324`）。

主题可用性随档位变化：非 truecolor 时主题选择器**隐藏** TokyoNight / RosePineMoon / OscuraMidnight，因为它们的染色背景量化后会失去特征（`docs/user-guide/06-theming.md:131`）。默认主题 GrokNight 的说明则是「在 256 色与 16 色终端上都能干净地量化」（同文件 `:13`）。

### 4.3 对比增强：两处专门处理

这是本专题最重要的正面发现——**不是单纯降级，而是针对低色彩能力重做配色**。

**(a) Windows 对比度提升**（`theme/mod.rs:367-404`）。在量化**之前**采样主题明暗极性，然后对 Windows 目标把一批半透明/弱对比色**推离**背景：

```rust
let bg = self.bg_base;
let user_block_push = if dark { 28 } else { 8 };
Self {
    bg_light: push_away(bg, self.bg_light, user_block_push),
    gray_dim: push_away(bg, self.gray_dim, 40),
    prompt_border: push_away(bg, self.prompt_border, 40),
    prompt_border_active: push_away(bg, self.prompt_border_active, 60),
    ...
}
```

`push_away` 按通道把颜色推离背景、方向由亮度比较决定，并夹在 0–255（`:372-384`）。注意暗色主题的推幅（28）明显大于亮色（8）。

**(b) ANSI 16 专用替代配色**（`theme/mod.rs:429-560`，调用点 `:325-334`）。触发条件是 `Basic` 档，或旧版 ConHost 上未达 truecolor：

```rust
if level.has_color()
    && (level == color_support::ColorLevel::Basic
        || (crate::glyphs::is_legacy_windows_console() && !level.has_truecolor()))
{
    adapted.ansi16_chrome_overrides(dark)
}
```

它的做法在注释里解释得很清楚（`:429-448`）：

- 以 `Black` / `White` 作为画布底，**升一级**用 `DarkGray` / `Gray` 表达「抬升面」，注释承认「在 ANSI 16 里这比 truecolor 设计读起来更响，但在包括博物馆级 `TERM=ansi` 在内的任何 16 色终端上都保证可见」；
- 语义色按极性选：暗底用亮变体（`LightRed`/`LightGreen`/…），亮底用常规变体（`Red`/`Green`/…），理由是「常规 ANSI 色（idx 1–7）约 50% 亮度，在亮背景上读得清；亮变体（idx 9–15）全饱和，在暗背景上读得清」（`:449-469`）；
- `high_contrast_fg` 取 `White`/`Black`，并专门用于 `prompt_border_active`、`text_primary`、`md_text`、`accent_user` 等焦点与正文位（`:440`、`:488-503`）；
- 另有测试断言 `scrollbar_thumb_contrasts_with_track_in_all_themes`（`theme/mod.rs:1021`）。

### 4.4 色盲友好：未找到证据

在 `crates/` 全树内检索 `colorblind`、`color_blind`、`colour_blind`、`deuteran`、`protan`、`cvd`、`daltoni`，**零命中**。也就是说：

- 没有色觉障碍（CVD）专用调色板，没有红绿之外的冗余编码开关；
- 唯一与「不靠颜色分辨」相关的是 diff 渲染的退化路径：当主题的 `diff_*_bg` 为 `Reset` 时，变更行改用**整行红/绿前景色**而非色带（`theme/mod.rs` 中 `paints_no_diff_row_bands` 的文档注释）。这仍依赖色调，不构成 CVD 方案。
- ANSI 16 替代配色（4.3b）在客观上提高了对比度与极性正确性，但它的设计目标写在注释里是「保证可见」，不是色觉障碍适配。

`color_support.rs` 里那套 16 色安全映射是**能力降级**，不是无障碍配色方案，合稿时不要把它写成前者。

## 5. 无障碍开关全清单

| 开关 | 类型 / 取值 | 作用 | 出处 |
| --- | --- | --- | --- |
| `NO_COLOR` | 环境变量，存在即生效 | 强制 `ColorLevel::None`，全部颜色映射为 `Reset` | `color_support.rs:99`、`:178`；doc 06-theming.md:119 |
| `ui.cursor_blink` | 三态：未设 / `true` / `false` | 未设 = `Inherit`（不发任何样式转义，保留终端自身光标设置）；`true` = 闪烁块，`false` = 常亮块。注释说明强制设置曾导致光标闪烁问题 | `app/mod.rs:1427-1446`；doc 26 `ui` 表 |
| `ui.display_refresh.auto_cadence_enabled` | boolean | 让流式/滚动节奏匹配显示器刷新率（自适应而非减少动效） | doc 26 `ui` 表；探测实现 `host/display_refresh.rs` |
| `ui.screen_mode` | `fullscreen` / `minimal` | 切换渲染模式（重启生效）；minimal 为键盘优先、终端持有历史 | doc 26 `ui` 表；`app/mod.rs:399-411` |
| `ui.compact_mode` + `/compact-mode` | boolean / 命令 | 更密的 padding；小屏提示推荐它 | doc 26 `ui` 表；`tips/small_screen.rs:41-47` |
| `ui.mouse_reporting_toggle` + `Ctrl+R` + `/toggle-mouse-reporting` | boolean | 关闭鼠标捕获，解决鼠标上报坏掉/干扰的场景。`Ctrl+R` 只在 scrollback 聚焦时可用，故 prompt 侧走斜杠命令 | `app/mod.rs:182-187`、`:329`；doc 26 `ui` 表 |
| `ui.simple_mode` / `ui.vim_mode` | boolean | 选择输入编辑与导航范式（readline ↔ vim） | doc 05:71-72；doc 03:9-29 |
| `ui.show_thinking_blocks` | boolean | 是否显示推理块 | doc 26 `ui` 表 |
| `ui.invert_scroll`、`ui.scroll_lines`、`ui.scroll_speed`、`ui.scroll_mode` | bool / int / enum | 滚动方向、步长、速度、输入分类 | doc 26 `ui` 表 |
| `[scrollback.display] rtl_bidi` | boolean，默认 false | 开启应用侧 RTL 重排 | doc 21:274-277；`render/bidi.rs:19-30` |
| `[ui.notifications] method` | `auto` / `osc9` / `osc99` / `osc777` / `bel` / `none` | 通知协议；**`none` 是唯一的「静音」入口** | `notifications/config.rs:52-61` |
| `[ui.notifications] condition` | `unfocused` / `always` / `never` | 何时发通知 | `notifications/config.rs:62-68` |
| `[ui.contextual_hints].*` | boolean 逐项 | 关闭各类上下文提示（`small_screen`、`ssh_wrap`、`image_input`、`plan_mode`、`send_now`、`undo`、`word_select`） | doc 26 `ui` 表 |
| `GROK_CLIPBOARD_NO_OSC52` | 环境变量 | 关掉 OSC 52 粘贴路由 | doc 21:118-120 |
| `GROK_CLIPBOARD_NO_DATA_CONTROL` | 环境变量 | 关掉 Wayland data-control 路由，改用命令行工具 | doc 21:111-113 |
| `GROK_FORCE_LEGACY_CONSOLE` | 环境变量 | 强制旧控制台字形降级（ASCII / CP437） | `glyphs.rs:389-391` |

**明确未找到证据的三项**：

1. **全局「减少动效 / reduce motion」开关**。动画节奏是硬编码常量：spinner 每 `SPINNER_DIVISOR = 4` 个 tick 换帧（`views/turn_status.rs:31`），idle 监视脉冲用 `MONITOR_PULSE_DIVISOR = 8`（`:36`），「等待你」菱形脉冲速度 `USER_WAITING_PULSE_SPEED = 0.08`（`:40`），tip TTL 为 `DEFAULT_TIP_TICKS = 90`（`tips/ephemeral.rs:11`）。没有任何配置项能关掉它们；`ui.cursor_blink` 是本仓库唯一与动效相关的无障碍开关。
2. **静音/响铃开关（除通知协议外）**。搜遍 pager 与 pager-render 源码，未见主动写 BEL 的代码；唯一与响铃相关的是**被动回退**：终端未识别时通知协议落到 `NotificationProtocol::Bel`，`/doctor` 会报 `WarningCategory::NotificationProtocolFallback`，文案「Grok is using the terminal bell because the terminal was not recognized」，并提示把 `[ui.notifications] method` 改成终端支持的协议或 `none`（`diagnostics/mod.rs:522-537`，`WarningCategory` 注释 `:106`）。
3. **高对比模式 / 字体缩放 / 屏幕阅读器支持**。检索 `screen_reader`、`screenreader`、`aria`、`a11y`、`accessib` 在 Rust 源码中零命中。

## 6. 输入流过滤层

### 6.1 为什么需要

终端不只是**键盘输入源**。它会主动往 stdin 塞两类东西：(1) 对应用提问的**应答**（DA2、XTVERSION 的 DCS 回复），(2) 应用自己开启的**上报**（SGR 鼠标、焦点、bracketed paste）。crossterm 把这些字节流按自己的语法解码成 `Key` 事件，于是「终端应答」会伪装成用户按键混进事件流；而在特定坐标下，**鼠标上报本身也会被解码错**。四个过滤器就是为此存在的，彼此是**兄弟模块**，注释里显式交叉引用。

```
终端 ──► reader thread ──► XTVERSION DCS 应答 ──► XtversionFilter     (xt_filter.rs)
                              │
                              ├─► SGR 鼠标 / 焦点片段 ──► CsiFragmentFilter (csi_filter.rs)
                              │
                              └─► X10 鼠标错解重组 ──► X10ReassemblyFilter (x10_filter.rs)
                                                        │
                                                        ▼
                                                    事件循环

grok wrap: 子进程 PTY 输出 ──► Osc52Filter ──► 本地剪贴板 / stdout (wrap_filter.rs)
                                     └──► ModeTracker 记录 DEC 模式 (wrap_restore.rs)
```

### 6.2 四个过滤器

**`X10ReassemblyFilter`（`app/x10_filter.rs`）**——修复被错解的鼠标坐标。当坐标字节恰好构成 UTF-8 多字节序列时，解码器会把两个字节拆成「列 = lead 字节 − 33，行 = continuation − 33」，于是 `0xC2`/`0xC3`（161/162）配 `0x80..0xBF`（95..158）成为**魔数形状**：

```rust
fn is_mangled_shape(m: &MouseEvent) -> bool {                       // :80-82
    matches!(m.column, 161 | 162) && (95..=158).contains(&m.row)
}
```

命中后把事件**扣住**，等紧随其后那个「被挤出的行字节」——它以普通 `Char` 按下或 `Backspace` 的形式到达（`:87-101`），两个事件必须在一个批次内相邻到达，因为「它们来自同一次 read 的相邻字节」，超时上限定为 `MAX_COMPLETION_GAP = 50ms`（`:28`）；超时后的真实击键会原样放行（测试 `stale_completion_is_not_consumed`，`:262`）。`reconstruct`（`:105-118`）反解出真实列，行号直接取被挤出的字节减 33。这里有一处重要的设计演进：早期用**坐标是否越界**来判断，结果在 180×120 这样的大终端上「错解形状恰好落在边界内」而漏过（回归测试 `reconstructs_shape_that_is_in_bounds_on_large_terminals`，`:218-228`）；现在形状即判据，与终端尺寸无关（`:11-13`）。

**`CsiFragmentFilter`（`app/csi_filter.rs`）**——重组被拆成片段的两类 CSI：SGR 鼠标 `[<d;d;d{M,m}` 与焦点上报 `[I` / `[O`。状态机在 `:121-159`（`Idle → Bracket → LessThan → Digits1 → Semi1 → …`）。它最有价值的一条是**焦点上报的翻译**：当裸 `\e` 与 `[I`/`[O` 落在同一批次时，过滤器撤回那个裸 Esc，改吐 `Event::FocusGained` / `FocusLost`（`:57-75`）——注释说明这样「焦点驱动的功能（prompt 重新聚焦、recap 离开计时器、`/gboom` 松键）在 SSH 下仍然可用」。代价与边界都写得很诚实：单个 `[` 不能跨批次持有（否则用户打的 `[` 会卡住不显示），所以**上一批次孤立了 `\e` 的焦点上报仍会漏过**（`:9-11`）；同理 `Bracket` 状态在函数末尾会被强制冲刷（`:109-115`）。类型击键误触发有防护：只有 `\e[` 前缀才进状态机，所以 `arr[I]` 这类文本正常通过（`:76-81`）。

**`XtversionFilter`（`app/xt_filter.rs`）**——吞掉终端对 XTVERSION 的 DCS 应答。注释直接点出根因：crossterm 会把 `ESC P` 呈现为 `Alt+Shift+P`，把 payload 呈现为一串普通 `Char` 按下，把 ST 呈现为 `Alt+\`、BEL 呈现为 `Ctrl+G`（`:22-24`）。多个时间常量各有理由：

| 常量 | 值 | 理由（源码注释） |
| --- | --- | --- |
| `XT_ARM_WINDOW` | 5 s | 对标 opentui 的非阻塞 5 s 窗口；解除后零开销（`:10-11`） |
| `XT_FRAGMENT_TIMEOUT` | 150 ms | 其他 TUI 栈用的值（`:13-14`） |
| `XT_MAX_HOLD` | 1 s | 总持有上限，防止终端「细水长流」地吐合法 payload 字符拖死事件循环（`:16-17`） |
| `XT_MAX_PAYLOAD` | 64 | 真实应答很短（示例 `kitty 0.35.2`）（`:19-20`） |

两个细节值得借鉴：armed 状态来自 `reply_pending()`（`:55`），且 `deadline` 是**首次 `filter()` 调用时**才设置而不是构造时——「启动加载可能耗时数秒，那段时间不能烧掉 armed 窗口」（`:27-28`）。另外，一旦 intro（`ESC P` 之后的部分）被确认，就不再中途过期，因为「在途应答必须走完（要么 Complete，要么按死持有丢弃），否则尾巴会作为用户输入漏出去」（`:124-126`）。

**`Osc52Filter`（`src/wrap_filter.rs`）**——只服务 `grok wrap`，与上面三个不同层：它在**子进程 PTY 输出**与 stdout 之间，而不是输入侧。职责是拦截 OSC 52 剪贴板序列（含 tmux DCS passthrough 形式）与私有 host-image 请求 OSC，其余全部**逐字节透传**，同时把每个 CSI 报告给 wrap 模式追踪器（模块注释 `:1-11`）。三个容量常量都注明推导关系：

```
MAX_ESC_BUFFER         = 1 MiB   // 要容纳 768 KiB payload 的 base64（~1.33x）加转义外壳   :13-16
MAX_CSI_BUFFER         = 128     // 单条 DECSET 列出全部被跟踪模式约 69 字节，有单测钉住关系  :18-20
MAX_CLIPBOARD_PAYLOAD  = 768 KiB // 与 MAX_ESC_BUFFER 对齐；SSH 场景下不现实有更大的剪贴板    :22-25
```

base64 解码用 `DecodePaddingMode::Indifferent`，理由写得很具体：「现实中（含部分 Go 实现与终端）会省略 `=` 填充」（`:33-38`）。另有一条与「过滤器不能留下半开序列」相关的约束：被子进程 EOF 截断的 CSI **故意不冲刷**，因为「发出半开的 CSI 会让真实终端解析器停在序列中间，吃掉退出路径紧接着写的恢复字节」（`FilterState::Csi` 的文档，`:51-55`）。

### 6.3 wrap 模式下的模式追踪与恢复

`ModeTracker`（`src/wrap_restore.rs`）解决一个具体问题：`grok wrap` **区分不了子进程正常退出与连接中断**——SSH 传输死亡到达时只是一个普通 PTY EOF 加退出码。它能知道的是子进程在本地终端上**开了哪些 DEC 私有模式却没关**，因为重置字节随链路一起死了（模块注释 `:1-8`）。

跟踪的模式位共 14 个（`:19-47`），除常规鼠标编码（`?1000/1002/1003/1005/1015/1016`）、`?2004` bracketed paste、`?1004` 焦点上报、`?2026` 同步更新外，还包括**老式备用屏** `?47`、`?1047`、`?1049`。`?25` 是**反向**跟踪的：DECTCEM 的 set 侧是「显示光标」，所以需要修复的态是「见过 `?25l` 而之后没有 `?25h`」（`:44-47`）。恢复字节按 `DISABLE_ORDER` 发出（`:50`），顺序与 `xai_crash_handler::terminal` 的 `RESTORE_SEQ` 用单测钉在一起。文件末尾还老实记录了一个已知局限：Kitty 协议每个屏幕缓冲区各自维护一层栈，而这里只有一个净深度计数，所以「在一个屏幕 push、在另一个屏幕死掉」的子进程是按计数而非按屏幕恢复的。

## 7. CJK / RTL / 组合字符

### 7.1 RTL：默认不管，理由明确

`render/bidi.rs` 的模块头一句话讲清了定位（`:1-9`）：

> **Off by default.** Many terminals already run implicit bidi; reordering in the app double-flips text on those hosts.

实现方案是**双翻转**——VTE 系终端、Terminal.app、Konsole、mlterm 等自己会重排，应用再排一次就反了。所以默认关闭，只在终端不重排时由用户显式打开 `[scrollback.display] rtl_bidi = true`（doc 21:266-281）。开关是进程级原子门闩（`bidi.rs:19-31`），随 appearance 配置加载生效，无需重启（doc 21:279）。

### 7.2 启用后的行为

- **判定**：只对含强 RTL 或阿拉伯数字字符的文本重排——`BidiClass::R | AL | AN`（`:33-42`）。
- **剥离控制符**：只剥 bidi override / isolate / embedding 控制符（`\u{061C}`、`\u{200E}`、`\u{200F}`、`\u{202A}..=\u{202E}`、`\u{2066}..=\u{2069}`），注释特意说明**不含 ZWJ/ZWNJ 连接符**（`:43-56`）。启用后即使纯 LTR 也会剥离控制符（`:74-77`）。
- **表格行不动**：`is_table_row` 的行直接跳过重排，以保持列对齐（`:78-80`、`:164-166`、doc 21:289）。
- **按 grapheme 反转**：RTL run 以字素簇为最小单位反转，并**镜像配对标点**（`:494-495`、模块头 `:7`）。
- **逐行判定基方向**：不是按段落，而是按**实际绘制的每一行**解析，所以「以英文开头的软换行续行可以拥有与段落首行不同的基方向」（`:88-101`、doc 21:294-295）。
- **命中测试的双向映射**：这是配套成本最高的一块。`logical_slice_for_visual_cols`（`:199-275`）由视觉列区间取回逻辑文本；`visual_col_to_logical_col`（`:280-…`）是绘制重排的逆运算，用于「选择端点存逻辑列、但命中测试打在绘制单元」的场景（注释点名了 block viewer 拖拽）。关闭重排、命中表格行、或该行无 RTL 时，两者都是恒等函数。doc 21:290-292 说明了用户可见效果：搜索高亮、选择/拖拽复制、双击选词/选 URL、链接命中区域都在「绘制的视觉单元」与「同一行的逻辑文本」之间映射，因此屏幕高亮落在正确字形上，而粘贴回剪贴板仍是逻辑序。
- **适用范围受限**：只覆盖 scrollback、列表内容、全屏块查看器（外加镜像 scrollback 的 dashboard peek 预览与 hook 弹窗）；**chrome、下拉、模态保持逻辑序**，以便命中测试保持一致（doc 21:286-288）。文件名末尾也承认「This is not a full mirrored RTL UI」（doc 21:297）。

### 7.3 CJK 与组合字符

宽度计算统一走 `unicode-width` 的显示宽度，而不是字符数：

- `crates/codegen/xai-grok-pager-render/src/util.rs:203` 的截断函数按显示列截断，「CJK 计 2」；
- 同文件 `:232` 的注释解释了一个具体的坑：`format!` 的 `{:<width$}` 按**字符数**补齐，遇到宽字形（如 CJK）后列会错位，所以这里另写了对齐助手；
- 表格输出的测试 `print_table_pads_cjk_labels_by_display_width`（`worktree_cmd/display.rs:233-235`）钉住了这一行为；
- 无关字形（em dash、CJK）在字形替换模块中**原样透传**（`glyphs.rs:569`）。

**组合字符与字素簇当作原子**。编辑器侧的测试集合直接以 `["e\u{301}", "👩🏽\u{200d}💻", "🇺🇸", "界"]` 为用例（`crates/codegen/xai-ratatui-textarea/src/editor_tests/editing.rs:35-40`），断言光标移动与删除**从不切开字素簇**（`:55-67`、`:84-104`）；换行按显示宽度而非字节（`wrapping.rs:512`），行高会算上双宽 emoji（`:588-589`）。pager 侧的行编辑器与搜索框有对应的重复用例（`input/line_editor.rs:245-299`、`scrollback/search.rs:799-815`）。

**未找到证据**：没有基于 `LANG` / `LC_ALL` / `LC_CTYPE` 的字符集能力探测或 ASCII 降级路径。`host::collect_unicode_env`（`host/mod.rs:11-23`）名字容易误导，它只是把环境变量收集成 UTF-8 map（跳过非 Unicode 项），供品牌/主题/显示服务器检测使用，并非字符集能力探测。字形降级只有一个触发条件：旧版 Windows ConHost。

### 7.4 IME

只有一处显式的输入法相关处理：`TerminalName::Otty` 会把 macOS 的 IME 提交包进 bracketed paste，代码为此提供 `delivers_ime_as_bracketed_paste()`（`terminal/mod.rs:156-159`），并有测试断言**其他所有品牌都不得为 true**（`terminal/test.rs:55-70`）。剪贴板模块里另有一处注释用 IME 作为换行符差异的可能解释：「终端会把 `\n` 改写成 `\r`，不匹配说明是 IME 提交或 tmux 缓冲被改了，而不是剪贴板粘贴」（`clipboard/mod.rs:798`）。未见预编辑（preedit）渲染、光标定位或组合态 UI。

## 8. 键盘可访问性

### 8.1 鼠标能做什么

官方列举的鼠标能力只有五项（doc 03:412-421）：点击 scrollback 条目选中、滚轮滚动、点击 prompt 聚焦、悬停高亮、Linux X11/XWayland 中键粘贴 PRIMARY。

### 8.2 键盘等价路径逐项对照

| 鼠标动作 | 键盘等价 | 出处 |
| --- | --- | --- |
| 滚轮上/下 | `Ctrl+K` / `Ctrl+J`（一行）、`PageUp` / `PageDown`（一页）、`Ctrl+U` / `Ctrl+D`（半页） | doc 03:47-56 |
| 点击条目选中 | `j` / `k` 或 `Up` / `Down` | doc 03:41-42 |
| 悬停高亮 | 无等价（纯装饰） | — |
| 点击 prompt 聚焦 | `Tab` / `Space`（vim 模式还有 `i`） | doc 03:104-110 |
| 折叠/展开 | `h` / `l` 或 `Left` / `Right`；`e` 切换；`⇧E` 全展开/全折叠 | doc 03:70-75 |
| 全屏查看 | `Enter` 或 `Ctrl+F` | doc 03:88-90 |
| 复制块内容 | `y`；复制元数据 `⇧Y` | doc 03:85-86 |
| 中键粘贴 PRIMARY | `Shift+Insert`（终端原生） | doc 03:299 |
| 模态关闭按钮 | `Esc` / `F2` / `Ctrl+,`，注释明写「X 按钮的点击取消，与这几条键盘路径镜像」 | `app/modals.rs:1588` |
| 鼠标捕获本身 | `Ctrl+R`（scrollback 聚焦）或 `/toggle-mouse-reporting`（任意面板） | `app/mod.rs:182-187`、`:329` |

整屏拖动/点击的热区并非都能靠键盘复制，但**面向操作的点击**都有对应键路径，且在 `PageUp`/`PageDown` 上做了额外照顾：这两个键在普通 prompt 聚焦时也能滚动会话，且**不移动焦点、不改草稿**——不过 prompt 历史、`@` 文件搜索、斜杠菜单或补全下拉打开时，按键优先归它们（doc 03:58-61）。

### 8.3 代码层：存在「键盘专用宿主」这一等公民概念

这是比文档更有力的证据。`views/turn_status.rs` 用 `Option` 表达鼠标可用性，而不是用一个 bool 去逐个分支：

```rust
/// `None` on keyboard-only hosts.                              :61
/// `Some(_)` renders them; `None` marks a keyboard-only host
/// (minimal mode, no mouse capture) and suppresses all.        :66
```

`:212` 的注释说明：`None` 时**同时**抑制按钮与悬停状态上报；`:298` 说明 `[stop]` 按钮「空闲时或在键盘专用宿主上不显示」；`:1217` 说明键盘专用宿主**连命中矩形都不生成**。minimal 模式的渲染复用同一个 widget，明确以 `None` 传入（`crates/codegen/xai-grok-pager-minimal/src/live.rs:526-527`）。模式切换时也有对应处理：「Minimal 会关掉鼠标捕获；没有移动事件的话上一帧的悬停会粘住」，于是重置 `app.last_mouse_pos = None`（`app/mode_switch.rs:38-40`）。

### 8.4 阻塞式卡片与焦点契约

四类会阻塞 agent 的界面（question 卡、MCP elicitation 卡、权限提示、取消轮次面板）共享一份显式契约（doc 03:109-125）：

- `Tab` / `Shift+Tab` 只在**本卡内部**的行走，两端环绕，**永不把焦点挪出去**——「光标总在你看得见的地方」；
- `Esc` 逐级退出：先清空卡片待处理项，清无可清才离开；question 卡与权限提示把键盘**停在 scrollback**（卡片留在屏上）以便回看上下文；
- 多卡同时打开时有固定优先级：权限提示 > 取消面板 > question > elicitation，快捷键栏始终显示**当前真正接收按键**的那一张卡的键位。

### 8.5 局部键盘优先证据

- elicitation 卡：为了让**纯键盘用户**也能看到长 URL 的尾部，提供 `Tab` / `Left` / `Right` 的水平滚动（`app/agent_view/elicitation.rs:26`）。
- 帮助面板搜索：点击提示进入详情时会**同步清掉已提交的查询**，注释明写「以对齐键盘路径」（`views/shortcuts_help.rs:994`，测试 `views/shortcuts_help_tests.rs:1012`）。
- 模态点击取消与键盘取消语义一致（`app/agent_view/modals.rs:1273`）。

**未找到证据**：全仓库没有「每个鼠标动作都有键盘等价」这条不变量本身——既没有文档声明，也没有对应的穷举测试。上面逐项对照是**从快捷键表与代码分支反推**出来的，覆盖操作类点击，但不构成形式化保证。

## 9. 未找到证据的部分（供合稿标注）

- **全局减少动效开关**：无。动画节奏为硬编码常量（`views/turn_status.rs:31/36/40`、`tips/ephemeral.rs:11`）。
- **色盲（CVD）专用配色与冗余编码**：无（全树检索 `colorblind` / `deuteran` / `protan` / `cvd` 零命中）。
- **高对比模式、字体缩放、屏幕阅读器支持**：无（`screen_reader` / `aria` / `a11y` / `accessib` 零命中）。
- **基于 locale 的字符集降级**：无。
- **agent 视图的宽度阈值**：无；宽度自适应逐组件进行（模态 `min_width` 地板 + 夹回缓冲区）。
- **硬性最小尺寸拒绝绘制**：无。整屏层面只有 `SCROLLBACK_MIN_ROWS = 5` 这一个布局地板，没有「终端过小就拒绝渲染并报错」的路径。
- **鼠标/键盘完全对等的形式化保证**：无（见 8.5）。

## 关键结论（供合稿引用）

1. 终端能力是**探测成枚举、再由枚举驱动降级**的两段式结构；业务代码从不读环境变量，只读 `TerminalContext` 上的能力位。`terminal/mod.rs:225-265`、`:109-160`
2. `TerminalContext` 同时保留乐观的 `brand` 与原始的 `env_brand`，所有「必须默认拒绝」的判断都回查后者，避免原生 Windows 把 `Unknown` 假定成 `WindowsTerminal` 后误放行。`terminal/mod.rs:228-232`、`:725-730`
3. 高度自适应有**三级阈值**且互相咬合：`SHORT_TERMINAL_ROWS = 16`（砍可选行）、`AUTO_COMPACT_MAX_ROWS = 20`（强制 compact）、小屏提示带 21–28；三者的相对关系有编译期断言。`views/agent.rs:89-98`、`tips/small_screen.rs:23-29`
4. 自动 compact **只影响渲染，不回写配置或磁盘**，窗口变大即恢复用户选择；`terminal_rows == 0` 表示未测量，永不强制。`views/agent.rs:99-104`
5. 色彩能力分四级，`NO_COLOR` 拥有最高优先级并直接判 `None`；终端原生锁（minimal 或 `terminal` 主题）会把上限压到 ANSI 16。`color_support.rs:17-30`、`:83-89`、`:98-101`
6. 低色彩能力下**不是简单降级而是重做配色**：ANSI 16 专用替代配色按极性选语义色、以 `high_contrast_fg` 承载焦点与正文；Windows 另有量化前的对比度提升。`theme/mod.rs:429-560`、`:367-404`、`:325-334`
7. **不存在色盲专用方案**；`color_support.rs` 的 16 色映射是能力降级，不是无障碍配色，合稿时不可混用这两个概念。
8. 四个过滤器解决「终端应答污染输入流」的不同侧面：X10 重组靠魔数形状而非坐标越界（大终端上的回归教训）、CSI 过滤器把 `\e` + `[I`/`[O` 翻译成焦点事件从而让焦点功能在 SSH 下可用、XTVERSION 过滤器用 5 s armed 窗口 + 150 ms 片段超时 + 1 s 总持有上限防拖死、Osc52 过滤器只管 wrap 的输出侧。`x10_filter.rs:80-82`、`csi_filter.rs:57-75`、`xt_filter.rs:10-28`、`wrap_filter.rs:13-38`
9. RTL **默认关闭**，因为在已做隐式 bidi 的终端上会双重翻转；开启后按 grapheme 反转、镜像标点、表格行不动、逐行判定基方向，并额外维护视觉列 ↔ 逻辑列的双向映射以保证高亮与复制顺序都正确。`render/bidi.rs:1-9`、`:78-80`、`:88-101`、`:199-320`
10. CJK 与组合字符按**显示宽度**与**字素簇**原子处理（`unicode-width` + `unicode-segmentation`），测试直接用「组合符 + ZWJ emoji + 国旗 + 汉字」四类用例；但**没有** locale 层的字符集降级。`util.rs:203/232`、`xai-ratatui-textarea/src/editor_tests/editing.rs:35-40`
11. 无障碍开关的真实清单以配置项为主：`NO_COLOR`、`ui.cursor_blink`、`ui.mouse_reporting_toggle`（`Ctrl+R` / `/toggle-mouse-reporting`）、`ui.screen_mode`、`ui.compact_mode`、`ui.simple_mode`/`ui.vim_mode`、`[scrollback.display] rtl_bidi`、`[ui.notifications] method`（唯一的静音入口）、`[ui.contextual_hints].*`、`GROK_FORCE_LEGACY_CONSOLE`；**没有**全局减少动效开关。
12. 键盘可访问性有真实工程投入：代码里存在「键盘专用宿主」概念（`Option` 为 `None` 时连鼠标按钮与命中矩形都不生成）、minimal 模式强制关掉鼠标捕获、`Ctrl+R` 只在 scrollback 生效故另配斜杠命令兜底、阻塞卡片保证 `Tab` 不逃出卡片且 `Esc` 逐级退出；但「所有鼠标操作都有键盘等价」**没有形式化保证**，只找到零散的镜像断言。`views/turn_status.rs:61/66/212/1217`、`app/mod.rs:329`、`minimal/live.rs:526`、`app/modals.rs:1588`
