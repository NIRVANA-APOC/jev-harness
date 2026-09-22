## 3. 视觉语言与设计美学

> 本节由上游专题 `01-theme-color-system.md`、`02-glyphs-and-iconography.md`、`03-typography-spacing-density.md`、`04-markdown-rendering.md` 提炼、去重并按主题重组，**未引入上游之外的新事实**；上游标注的冲突项与「未找到证据」一律保留。证据路径沿用上游简写：`theme/*`、`render/*`、`glyphs.rs`、`util.rs` 属 `crates/codegen/xai-grok-pager-render/src/`；`parse.rs`、`render.rs`、`style.rs`、`checkpoint.rs`、`streaming.rs`、`output.rs`、`hyperlinks.rs` 属 `crates/codegen/xai-grok-markdown/src/`（`lib.rs` 特指 `xai-grok-markdown-core/src/lib.rs`）；`views/*`、`scrollback/*`、`app/*`、`slash/*` 属 `crates/codegen/xai-grok-pager/src/`。

### 3.1 色彩与主题系统

### 本节要点
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

### 本节要点
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

### 本节要点
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

### 本节要点
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
