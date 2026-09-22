# 专题 01：Grok Build 的主题与配色系统

> 专题：主题与配色系统｜调研日期：2026-09-22｜证据范围：`crates/codegen/xai-grok-pager-render/src/theme/`（全部 13 个文件，共 5297 行）、`crates/codegen/xai-grok-pager-render/src/terminal/mod.rs`、`src/render/color.rs`、`src/syntax.rs`、`crates/codegen/xai-grok-pager/src/slash/commands/theme.rs`、`src/app/event_loop.rs`、`src/app/mod.rs`、`src/app/dispatch/settings/setters.rs`、`src/theme/` 引用点、`crates/codegen/xai-grok-shell/src/util/config/settings_writes.rs`、`crates/codegen/xai-grok-pager/docs/user-guide/06-theming.md`、`21-terminal-support.md`、工作区根 `Cargo.toml` 与 `xai-grok-pager-render/Cargo.toml`

## 1. 一句话总览

整个 TUI 的颜色只有一个来源：`Theme` 结构体的 58 个 `Color` 字段。所有主题都是编译期常量（`const fn` 构造），启动时按终端色彩能力整体量化一次，之后每帧读取。代码里有明确约束注释：“All colors come from the `Theme` struct. No hardcoded colors elsewhere.”（`crates/codegen/xai-grok-pager-render/src/theme/mod.rs:1`，`.../theme/tokyonight.rs:1`）。

```
┌─ 第 1 层：调色板常量（每主题一个内联 mod palette） ──────────────┐
│  pub const BG: Color = rgb(20, 20, 20);   // #141414             │
│  pub const BLUE: Color = rgb(122, 162, 247); // #7aa2f7          │
└──────────────────────────────┬───────────────────────────────────┘
                               │  use palette::*;
┌──────────────────────────────▼───────────────────────────────────┐
│ 第 2 层：Theme 结构体（58 个语义 role + 6 个 Modifier）           │
│  bg_base / text_primary / accent_tool / diff_insert_bg / md_code  │
└──────────────────────────────┬───────────────────────────────────┘
                               │  Theme::current()  →  quantized(level)
┌──────────────────────────────▼───────────────────────────────────┐
│ 第 3 层：使用点。700 处调用 Theme::current()（其中 pager 633 处） │
│  渲染时按 role 取色；运行时算出的颜色再经 color_support::quantize │
└──────────────────────────────────────────────────────────────────┘
```

## 2. 内置主题清单

主题枚举 `ThemeKind`（`.../theme/mod.rs:28-40`），判别值本身是载荷（缓存按 `u8` 存储，见 `.../theme/cache.rs:18, 36-47`）：

| Kind | 判别值 | 规范名 `display_name` | 别名 | 明/暗 | `requires_truecolor` | 备注 |
| --- | --- | --- | --- | --- | --- | --- |
| `GrokNight` | 0 | `groknight` | `grok-night`, `dark` | 暗 | false | 默认主题（`mod.rs:197-201`） |
| `GrokDay` | 1 | `grokday` | `grok-day`, `light`, `day` | 亮 | false | GrokNight 的亮色对应版 |
| `TokyoNight` | 2 | `tokyonight` | `tokyo-night`, `tokyo` | 暗 | true | 蓝调背景 |
| `RosePineMoon` | 3 | `rosepine-moon` | `rosepine`, `rose-pine`, `rose-pine-moon` | 暗 | true | Rosé Pine 家族 |
| `OscuraMidnight` | 5 | `oscura-midnight` | `oscura` | 暗 | true | 极深底 + 紫强调 |
| `Terminal` | 6 | `terminal` | `terminal-default`, `transparent`, `native` | 中性 | false | 受 rollout gate 控制 |
| `Auto` | 4 | `auto` | `system` | 元主题 | false | 磁盘存 `"auto"`，内存只存解析结果 |

- `ALL` 只含 6 个具体主题，**不含 `Auto`**（`mod.rs:44-51`）；`from_name` 额外把 `Auto` 串进查找链（`mod.rs:148-154`）。
- 别名表见 `mod.rs:131-141`；`display_name` 见 `mod.rs:97-107`。`display_name_for_canonical` 的 match 里没有 `oscura-midnight` 分支，落到 `other => other` 原样返回（`mod.rs:185-195`）。
- 三级过滤：`ALL` → `selectable()`（按 `terminal` gate 剔除）→ `available()`（再按 truecolor 能力剔除）。`available()` 的前置判断是 `color_support::detect().has_truecolor()`（`mod.rs:72-95`）。

**设计取向**（依据各主题文件头注释）：

- **GrokNight**：中性灰底 + TokyoNight Night 强调色。`Grayscale ramp anchored at bg #141414 / fg #f3f3f3. Accents are TokyoNight Night hex.`（`.../theme/groknight.rs:14`）低饱和、对比可预测，因此能在 256/16 色下干净量化。
- **GrokDay**：`Backgrounds and text use a neutral grayscale ramp (no blue/warm tint). Accent colors are the same hue family as GrokNight but deepened for contrast on light backgrounds.`（`.../theme/grokday.rs:2-3`）同一色相族整体加深。
- **TokyoNight**：TokyoNight Night/Storm 原始调色板，蓝调背景（`.../theme/tokyonight.rs:2-4, 12-13`）。
- **RosePineMoon**：背景 `#232136`/`#2a273f`/`#393552`，强调色用 love/gold/rose/pine/foam/iris 命名（`.../theme/rosepine.rs:13-27`）。
- **OscuraMidnight**：`Deep backgrounds with OKLCH hue 265; accents are hand-picked purple to match that tint.`（`.../theme/oscura.rs:9`），palette 注释里逐色标注 OKLCH 三元组（`oscura.rs:15-46`）。
- **Terminal**：所有背景为 `Reset`，几乎不自带颜色（详见第 7 节）。

## 3. 主题数据的组织方式：结构体 + 编译期常量，不是静态表也不是资源文件

1. **`Theme` 是一个扁平结构体**：58 个 `Color` 字段 + 6 个 `Modifier` 字段（`.../theme/tokyonight.rs:46-144`）。用 `grep -c "pub .*: Color,"` / `"pub .*: Modifier,"` 可复核得 58 / 6。
2. **每个主题是一个 `const fn` 构造函数**，逐字段赋值：`tokyonight()`（`tokyonight.rs:148-232`）、`groknight()`（`groknight.rs:55-137`）、`grokday()`（`grokday.rs:53-135`）、`rosepine_moon()`（`rosepine.rs:32-114`）、`oscura_midnight()`（`oscura.rs:51-135`）、`terminal_default()` / `terminal()`（`terminal_default.rs:28-111, 115-126`）。
3. **色值以内联 `mod palette` 组织**，然后是 `use palette::*;`（例：`groknight.rs:15-52`）。`rgb()` 是本地 `const fn rgb(r,g,b) -> Color { Color::Rgb(..) }`（`groknight.rs:10-12`）。
4. **不来自配置文件、不来自运行时资源**。用户能改的只是"选哪个主题"（`[ui].theme`），没有自定义色值入口。
5. **唯一的打包资源是 3 个 `.tmTheme`**（语法高亮，不属于主题系统本体）：通过 `include_bytes!("../assets/…")` 内联进二进制（`.../src/syntax.rs:128-132`），资产目录 `crates/codegen/xai-grok-pager-render/assets/`。工作区根 `Cargo.toml` 没有任何资产拷贝/打包配置，`xai-grok-pager-render` 也没有 `build.rs`（仓库内 6 个 `build.rs` 无一是主题相关）。`xai-grok-markdown/assets/tokyo-night.tmTheme` 只用于该 crate 的测试（`crates/codegen/xai-grok-markdown/src/syntax.rs:167`）。

### 3.1 文件规模与色值数量

| 主题 | 文件行数 | 字节 | `palette` 常量数 | 唯一 RGB 三元组 | `rgb()` 出现次数 |
| --- | --- | --- | --- | --- | --- |
| `groknight.rs` | 138 | 5680 | 26 | 36 | 38 |
| `grokday.rs` | 136 | 5371 | 26 | 37 | 38 |
| `tokyonight.rs` | 363 | 14537 | 26 | 34 | 34 |
| `oscura.rs` | 136 | 5124 | 20 | 23 | 23 |
| `rosepine.rs` | 115 | 3378 | 15 | 17 | 17 |
| `terminal_default.rs` | 414 | 16561 | 0 | 0 | 0（唯一命中是测试里的 `resolve_to_rgb`，`terminal_default.rs:300`） |

`tokyonight.rs` 行数最多是因为它同时承载了 `Theme` 结构体定义、全部字段注释和一批 `const fn` 辅助方法；其余主题文件只有构造函数。

## 4. 颜色分层建模与"token"概念

**没有独立的 token/role 定义文件**，语义化靠两处承载：

1. **字段命名本身就是 role**。`Theme` 的 58 个字段是扁平命名空间，按用途分组并有注释（`tokyonight.rs:47-144`）：背景 6、`accent_*` 9、文本 2、灰阶 3、语义 4、搜索 1、模式强调 3（plan/verify/remember）、边框 4、滚动条 2、diff 6、`bg_visual` 1、paste 3、markdown 13 + 6 个 Modifier。
2. **一组 `const fn` 方法把 role 组合成可复用的"样式 token"**（`tokyonight.rs:234-341`）：

| 方法 | 语义 | 依据 |
| --- | --- | --- |
| `fg(color)` / `primary()` | 单色 / 主文本样式 | `tokyonight.rs:234-236, 282-284` |
| `muted()` / `muted_over_chrome()` | 次级文本；后者额外把 `Reset` 钉住以免继承边框色 | `tokyonight.rs:239-244, 248-253` |
| `dim()` | 最弱装饰文本；bandless 调色板走 `DIM` 而非画亮黑 | `tokyonight.rs:265-271` |
| `faint()` | 比 `dim()` 再弱一档，`gray_dim` 向 `bg_base` 混合 0.66；不能混则退回 `dim()` | `tokyonight.rs:275-280` |
| `bold()` / `link_style()` | 粗体 / 链接（`link_fg` + 下划线） | `tokyonight.rs:339-341, 256-260` |
| `is_bandless()` | `bg_visual == Color::Reset`，是"用反显/装饰替代色块"的总开关 | `tokyonight.rs:288-290` |
| `selection_overlay()` / `hover_overlay()` / `block_cursor_over()` | 选中、悬停、块光标；bandless 下统一 `REVERSED` 反显 | `tokyonight.rs:293-319` |
| `row_hover_bg()` / `panel_border_fg()` | 终端内行悬停（`bg_base→bg_dark` 半混）；分隔线取 `bg_highlight`，bandless 取 `gray_dim` | `tokyonight.rs:325-327, 331-337` |
| `diff_uses_line_fg()` | `diff_*_bg` 双双为 `Reset` 时，diff 改用整行前景色而非色带 | `mod.rs:340-345` |
| `ghost_text_style()` | 补全幽灵文本 = `dim()` + `ITALIC` | `mod.rs:406-411` |

**颜色混合同样集中在 `render/color.rs`**：`blend_color`（`.../src/render/color.rs:160-174`）在任一输入是 `Indexed` 时把结果也量化回索引色（理由注释：256 色终端启动时已把主题全量化，出现 `Indexed` 即说明该终端吃不了裸 RGB）；`nearest_indexed` 在 6×6×6 色立方与 24 级灰阶之间取最近邻（`color.rs:56-91`）。

### 4.1 字段到界面元素的映射示意

下图是按字段注释与 `06-theming.md` 的描述整理的**映射示意**，不是像素级布局还原；每个位置标注了对应 role：

```
 ┌──────────────────────── 全屏画布 bg_base ──────────────────────────┐
 │                                                                    │
 │  ▌ user 消息块          accent_user（竖线）+ text_primary          │
 │    bg_light + md_code_bg = 代码块底色（GrokNight 下比 bg_base 亮）  │
 │                                                                    │
 │  ▌ assistant 正文       accent_assistant + md_text                 │
 │    md_heading_h1..h6 逐级降权；md_code / md_code_bg 行内代码        │
 │    md_muted = 引用/列表/分隔线；link_fg + 下划线 = 链接             │
 │                                                                    │
 │  ▌ thinking 块          accent_thinking（可动画，见 wave_brightness）│
 │  ▌ tool 调用            accent_tool + gray_bright                  │
 │  ▌ 运行中               accent_running / running                    │
 │  ▌ 失败                 accent_error                                │
 │                                                                    │
 │  ┌── 菜单 / 下拉 ──────────────────────────────────────────────┐   │
 │  │ 普通行 gray※  悬停行 bg_hover（bandless: REVERSED）         │   │
 │  │ 选中行 bg_visual（bandless: REVERSED）                      │   │
 │  │ 边框 hover_border / selection_border                        │   │
 │  └──────────────────────────────────────────────────────────────┘  │
 │                                                                    │
 │  ▌ diff 行  diff_insert_bg/fg、diff_delete_bg/fg，行号 diff_gutter_fg│
 │                                                                    │
 │  ┌── 输入框 ────────────────────────────────────────────────────┐  │
 │  │ 边框 prompt_border（空闲）/ prompt_border_active（聚焦）     │  │
 │  │ 文本 text_primary；占位 ghost_text_style()；粘贴 chip paste_*│  │
 │  └──────────────────────────────────────────────────────────────┘  │
 │  状态行/信息行 text_secondary、gray、gray_dim 三档                  │
 └────────────────────────────────────────────────────────────────────┘
  滚动条：轨道 scrollbar_bg，滑块 scrollbar_fg（右侧贴边）
```

## 5. 两个主题的完整语义色表

### 5.1 GrokNight（默认主题，`groknight.rs:55-137`）

| role | 值（十六进制） | 来源 |
| --- | --- | --- |
| `bg_base` | `#141414` | `BG_STORM` |
| `bg_light` | `#242424` | `BG_HIGHLIGHT` |
| `bg_dark` | `#1c1c1c` | 内联；注释注明"比 `bg_base` 亮，让代码块可见" |
| `bg_highlight` | `#242424` | `BG_HIGHLIGHT` |
| `bg_hover` | `#2c2c2c` | 内联 |
| `bg_terminal` | `#0a0a0a` | `BG` |
| `accent_user` | `#c8c8c8` | `FG_DARK` |
| `accent_assistant` / `accent_thinking` / `accent_running` | `#bb9af7` | `MAGENTA` |
| `accent_tool` | `#787878` | `DARK5` |
| `accent_system` / `accent_skill` / `fuzzy_accent` | `#7aa2f7` | `BLUE` |
| `accent_error` | `#f7768e` | `RED` |
| `accent_success` | `#9ece6a` | `GREEN` |
| `text_primary` | `#e1e1e1` | `FG` |
| `text_secondary` | `#c8c8c8` | `FG_DARK` |
| `gray_dim` | `#585858` | 内联，注释："比 `FG_GUTTER` 略亮" |
| `gray` | `#6c6c6c` | `COMMENT` |
| `gray_bright` | `#787878` | `DARK5` |
| `command` / `warning` | `#e0af68` | `YELLOW` |
| `path` | `#ff9e64` | `ORANGE` |
| `running` | `#7dcfff` | `CYAN` |
| `accent_plan` | `#FFDB8D` | 内联，golden |
| `accent_verify` | `#bb9af7` | 内联，violet |
| `accent_remember` | `#8BC34A` | 内联，Material Design light green |
| `selection_border` | `#3c3c41` | 内联 |
| `prompt_border` | `#323237` | 内联 |
| `prompt_border_active` | `#505058` | 内联，聚焦时更亮 |
| `hover_border` | `#1e1e22` | 内联 |
| `accent_model` | `#1abc9c` | `TEAL` |
| `scrollbar_bg` | `#111111` | `BG_STORM_DARK` |
| `scrollbar_fg` | `#242424` | `BG_HIGHLIGHT` |
| `diff_delete_bg` | `#420e14` | `RED_DARK`；注释说明它"量化到 256 色红，而不是灰" |
| `diff_delete_fg` | `#f7768e` | `RED` |
| `diff_insert_bg` | `#063806` | `GREEN_DARK` |
| `diff_insert_fg` | `#9ece6a` | `GREEN` |
| `diff_equal_fg` / `diff_gutter_fg` | `#6c6c6c` | `COMMENT` |
| `bg_visual` | `#363636` | 内联 |
| `paste_bg` | `#111111` | `BG_STORM_DARK` |
| `paste_fg` | `#c8c8c8` | `FG_DARK` |
| `paste_dim` | `#414141` | `FG_GUTTER` |
| `md_heading_h1` | `#1abc9c` + BOLD | `TEAL` |
| `md_heading_h2` | `#7aa2f7` + BOLD | `BLUE` |
| `md_heading_h3` | `#9d7cd8` + BOLD | `PURPLE` |
| `md_heading_h4` | `#787878` + BOLD | `DARK5` |
| `md_heading_h5` | `#6c6c6c` + BOLD | `COMMENT` |
| `md_heading_h6` | `#5a5a5a` + 无修饰 | `DARK3` |
| `md_code` | `#3A95AB` | `BLUE1` |
| `md_task_checked` | `#9ece6a` | `GREEN` |
| `md_task_unchecked` | `#c8c8c8` | `FG_DARK` |
| `md_muted` | `#6c6c6c` | `COMMENT` |
| `md_code_bg` | `#1c1c1c` | 内联 |
| `md_text` | `#c8c8c8` | `FG_DARK` |
| `link_fg` | `#7aa6da` | 内联；注释："深底上的柔和蓝"，与 `BLUE` 刻意不同 |

### 5.2 GrokDay（`grokday.rs:53-135`）

与 GrokNight 的差异集中在三处：背景翻转为中性浅灰（`bg_base #eeeeee`、`bg_terminal #f5f5f5`、`bg_dark #e4e4e4`）；前景统一为深灰（`text_primary #262626`、`text_secondary #444444`）；强调色同色相加深，例如 `BLUE #2F64D2`、`MAGENTA #7D4BC6`、`ORANGE #C3691E`、`RED #CD3048`、`GREEN #378E23`、`TEAL #0A8E70`、`YELLOW #A27612`（`grokday.rs:33-45`）。

几个只在 GrokDay 出现的处理：
- 灰阶反转为"越暗越强"：`gray_dim #a5a5a5` → `gray #767676`（`COMMENT`）→ `gray_bright #626262`（`DARK5`）。
- 边框在浅底上靠**更深**表达聚焦：`prompt_border #C8C8CD` → `prompt_border_active #A5A5AF`（注释：`darker (more apparent) when focused`）。
- diff 底色改用浅色染底：`diff_delete_bg #F5DADE`（`RED_LIGHT`）、`diff_insert_bg #DAF2DC`（`GREEN_LIGHT`）。
- `accent_remember #4CAF50`、`accent_verify #7850A0`、`accent_plan #A8780A` 都是为浅底单独挑的值（`grokday.rs:86-90`）。
- `link_fg = BLUE #2F64D2`，注释：`deep blue for light bg`（`grokday.rs:133`）。

`md_*` 的 Modifier 也有主题级差异：RosePine 的 h2 是 `BOLD ∪ UNDERLINED`、h4 是 `BOLD ∪ ITALIC`（`rosepine.rs:97, 101`），Oscura 的 h4 是 `BOLD ∪ ITALIC`（`oscura.rs:122`），而 GrokNight/GrokDay 只用 BOLD。

## 6. 颜色能力降级链

### 6.1 四档枚举与探测

```
ColorLevel: None < Basic < Ansi256 < TrueColor        （color_support.rs:17-30）
```

`detect()`（`color_support.rs:83-89`）的判定顺序：

1. **`NO_COLOR` 优先**：只要环境里有该变量即返回 `None`（`color_support.rs:99-101`）。
2. **`supports_color::on(Stream::Stdout)`**（`color_support.rs:103`）：`has_16m`→TrueColor、`has_256`→Ansi256、`has_basic`→Basic，否则 `None`。
3. **非 TTY 回退 TrueColor**：`supports-color` 返回 `None` 时（tests/piped）直接判 TrueColor，注释理由是"TUI 总归跑在终端里"（`color_support.rs:115-117`）。
4. **品牌白名单升级**：若上一步只判到 256 或更低，但终端品牌属于 iTerm2 / Ghostty / Kitty / WezTerm / Alacritty / Rio / Warp / VS Code / Windows Terminal / Foot，或宿主是 Windows，则提升为 TrueColor（`color_support.rs:121-123, 246-269`）。动机注释：tmux / SSH / mosh 常剥掉 `COLORTERM`，而 ConHost 自 Win10 1709 起支持 24 位色但不上报。

探测结果缓存在 `OnceLock`，`detect()` 里再叠一层"minimal 模式锁 → 上限 Basic"的封顶（`color_support.rs:54, 85-88`）。

`TERM` / `COLORTERM` 的显式解析**只出现在 `grok doctor` 的独立路径**里：`standalone_from_env` 先看 `NO_COLOR`，再看 `COLORTERM=truecolor|24bit` 或品牌，再看 `TERM` 含 `256color` → Ansi256、空或 `dumb` → 不可用、否则 Basic（`color_support.rs:172-203`）。这条路径刻意不读 stdout（`grok doctor --json` 常被管道化），改用 stderr 是否为终端或能否打开 `/dev/tty`（Unix）/ `CONIN$`（Windows）（`color_support.rs:129-170`）。

### 6.2 量化：`quantize_color`

| 目标档 | `Color::Rgb` 输入 | `Color::Indexed` 输入 | `Color::Reset` / 命名色 |
| --- | --- | --- | --- |
| TrueColor | 原样 | 原样 | 原样 |
| Ansi256 | `Indexed(nearest_indexed(r,g,b))` | 原样 | 原样 |
| Basic | `indexed_to_ansi16(nearest_indexed(..))` | `indexed_to_ansi16(n)` | 原样 |
| None | `Reset` | `Reset` | `Reset` |

依据 `color_support.rs:220-234`。`indexed_to_ansi16` 对 0–15 直接映射命名色，16–255 先转 RGB 再按平方欧氏距离取最近（`color_support.rs:274-336`）。

### 6.3 量化在主题上的应用：`Theme::current()`

`Theme::current()`（`mod.rs:293-333`）每次调用都重新组装主题，顺序是：

1. minimal 模式的 `terminal_native_locked()` → 直接用 `terminal_default().quantized(level)`（`mod.rs:295-297`）。
2. `terminal` 主题走 `Self::terminal().quantized(level)`，**早返回**，理由注释："极性适配和 ANSI16 override 会把不透明背景盖到 `Reset` 上"（`mod.rs:299-302`）。
3. 按 `current_kind()` 构造基础主题 → **先采样极性** `base.is_dark()`（此刻 `bg_base` 还是 RGB）→ Windows 上做 `windows_contrast_boost` → `quantized(level)`（`mod.rs:314-322`）。
4. Basic 档或"legacy Windows 控制台且无 truecolor"时再做 `ansi16_chrome_overrides(dark)`（`mod.rs:325-332`）。此分支额外要求 `level.has_color()`，注释明确理由：否则命名 ANSI 重绘会把 `Reset` 覆盖，从而破坏 `NO_COLOR`。

`quantized()` 逐字段调用 `quantize_color`，共 58 次（`mod.rs:205-289`）；`Modifier` 字段不参与量化。

两个补偿层值得单独记：
- `windows_contrast_boost`（`mod.rs:367-404`）：Windows 显示 gamma 会把 ~12 级 RGB 台阶压扁，于是把若干背景/边框色**推离** `bg_base`，步长 8–60 不等（如 `prompt_border_active` 60、`gray_dim` 40、深色下的用户块 28）。
- `ansi16_chrome_overrides`（`mod.rs:429-556`）：ANSI16 只有两个灰阶可用，于是把 chrome 钉到四个中性槽（`Black/White/Gray/DarkGray`），语义色按"暗底用亮变体、亮底用常规变体"映射，并把紫/青/黄族折叠（magenta 吸收 purple/violet、yellow 吸收 orange/gold/plan）。

### 6.4 运行时生成的色也要量化

- 语法高亮：`syntect_rgb_to_fg` 非终端原生时走 `theme::quantize`（`.../src/syntax.rs:46-51`）。
- 混合/淡出：`blend_color` 在索引色场景下把结果量化回索引（`render/color.rs:160-174`）；`Theme::faint()` 的混合结果同样是量化后的字段参与运算（`tokyonight.rs:275-280`）。
- Markdown：`md_style::style()` 每次都从 `Theme::current()` 重建（`.../theme/md_style.rs:94-101`），并把 ratatui 色桥接到 `anstyle`；`Reset` 映射为 `None`（`md_style.rs:8-31`，含一条回归测试说明它曾错误回退到 `AnsiColor::White`）。

### 6.5 澄清：OSC 11 不属于色彩能力探测

`osc11.rs` 做的是**背景明暗**探测，与 truecolor/256/16 无关：查询串 `\x1b]11;?\x07`（`.../theme/osc11.rs:30`），主超时 500 ms、tmux passthrough 回退 80 ms（`osc11.rs:25-28`），解析同时兼容 4 位与 2 位十六进制通道（`osc11.rs:76-100`），亮度用 ITU-R BT.709 系数 + sRGB 反 gamma，阈值 0.5（`osc11.rs:23, 65-74`）。它是**仅启动期**的探测：crossterm 的 `EventStream` 一旦接管 stdin 就不能再查询（`osc11.rs:13-14`）。

## 7. 系统外观跟随（light/dark）

`SystemAppearance { Light, Dark }`（`.../theme/system_appearance.rs:22-26`）。四级探测链，前一级有结果就不走下一级（`system_appearance.rs:3-11, 53-70, 72-79`）：

1. **桌面 API** `dark_light::detect()`：macOS `AppleInterfaceStyle`、Linux XDG portal、Windows 注册表（`system_appearance.rs:83-90`）。
2. **显式环境变量**：`GROK_APPEARANCE` → `LC_GROK_APPEARANCE`（`env_appearance.rs:20-31`），接受 `dark|night` / `light|day`，大小写不敏感（`env_appearance.rs:45-53`）。`LC_*` 别名是为了穿过 SSH 的 `AcceptEnv LC_*`（`env_appearance.rs:3-4`）。
3. **OSC 11**（仅启动）：结果写入 `OnceLock`，运行时复用而不重探（`system_appearance.rs:18-20, 67-69, 97`）。
4. **`COLORFGBG`**（最后的兜底猜测）：取最后一个字段当背景索引，`0..=6 | 8` → Dark，`7 | 9..=15` → Light，非数字如 `default` → `None`（`env_appearance.rs:55-67`）。

**运行时轮询**：`SystemAppearanceWatcher` 每 5 秒调一次 `detect()`（测试下 50 ms），只有值变化才通过 `watch` 通道发出（`system_appearance.rs:121-171`）。它只调 `detect()`，不触发 OSC 11，也不直接改主题缓存。

**auto → 具体主题**：`to_theme_kind`，`Light → GrokDay`、`Dark → GrokNight`，可被 `[ui].auto_dark_theme` / `[ui].auto_light_theme` 覆盖，读取时过滤掉 `auto` 自身以防循环（`system_appearance.rs:108-119`；`cache.rs:267-289`）。`theme = "auto"` 会在解析时置位 `AUTO_MODE` 并立刻解析成具体 kind（`cache.rs:220-235`）；磁盘上存 `"auto"`，内存 `CURRENT` 只存解析后的具体主题（`mod.rs:38` 注释）。

## 8. 运行时热切换路径

```
用户动作                          状态写入                      落盘
─────────────────────────────────────────────────────────────────────────
/theme <name>  ──► Action::SetTheme(canonical)
                   （slash/commands/theme.rs:115,123）
                   └─► router: dispatch/Action::SetTheme → set_theme(app,v)
                                                    （dispatch/router.rs:1160）
                       ├─► Theme::apply_kind(kind)          ├─► Effect::PersistSetting
                       │   （theme/mod.rs:348-355）         │   → xai-grok-shell
                       │      └─ clamp_to_terminal          │     settings_writes.rs:220-222
                       │         （mod.rs:357-363）         │     update_config(|cfg| cfg.ui.theme=…)
                       │      └─ cache::set → store_kind
                       │         （cache.rs:89-100）
                       │         └─ sync_markdown_polarity  （cache.rs:138-146）
                       │            设置 markdown crate 的
                       │            color level cap + polarity_safe_syntax
                       └─► presenter.request(false) 触发重绘

/theme 预览（方向键）──► preview_arg / cancel_preview
                        （slash/commands/theme.rs:8,49-65）
                        只调 Theme::apply_kind —— 不落盘、不弹 toast

系统外观变化 ──► SystemAppearanceWatcher（5s 轮询）
                 └─► event_loop.rs:2547-2576
                     to_theme_kind → Theme::apply_kind
                     若 effective != current 则 presenter.request(false)

光标色 ──► accent_user 解析成 RGB → OSC 12；退出/复位发 OSC 112
```

几个关键点：

- **`apply_kind` 是唯一的运行时写入入口**（`mod.rs:348-355`）。它在 minimal 模式锁下直接 no-op（返回当前 kind）；否则先把"要求 truecolor 但不支持"的主题钳到 `GrokNight`（`mod.rs:357-363`），再写缓存。注意钳制只影响**实时显示**，用户仍可把 `tokyonight` 写进配置（`slash/commands/theme.rs:119` 注释）。
- **`cache::set` 连带同步 markdown 极性**：所有对 `CURRENT` 的写入都走 `store_kind`，后者调 `sync_markdown_polarity()`，把 `xai-grok-markdown` 的 color level 上限设为 `Basic`（终端原生）或 `TrueColor`，并切换 polarity-safe syntax（`cache.rs:94-100, 137-146`）。
- **watcher 的启停**由 `AUTO_MODE` 驱动：启动时 `SystemAppearanceWatcher::start_if_auto(theme_cache::is_auto_mode())`（`app/event_loop.rs:1847`），之后 `sync_appearance_watcher` 在一致性时 no-op（`event_loop.rs:3066-3075`）。
- **光标色**：`cursor_color_escape()` 把 `accent_user` 解析回 RGB 并生成 OSC 12（`mod.rs:563-568`）；`Reset` 返回 `None`，表示"不动终端自己的光标色"。启动期由 `apply_cursor_color()` 直接写 stderr（`mod.rs:570-585`，调用点 `app/mod.rs:414`、`app/mod.rs:1536`）；进入事件循环后改为**差分发射**：比较 wanted/on-wire，变化才发 OSC 12，转为 None 时发 OSC 112（`app/event_loop.rs:1971-1982`）。退出时 `reset_cursor_color_if_applied()`（`app/terminal_restore.rs:76`）。`CURSOR_COLOR_APPLIED` 门闩的存在理由有注释：无端复位会让 Ghostty 锁死光标色并不再跟随主题（`mod.rs:559-561, 587-594`）。
- **主题"热"但不完全"即时"**：`Theme::current()` 每帧重建主题（无脏标记、无缓存失效逻辑），因此切主题只需改那一个 `AtomicU8`；代价是每帧 58 次 `quantize_color` 调用。`md_style::style()` 同理每次重建（`md_style.rs:94-98`）。

## 9. `terminal` 主题与 minimal 模式：两条"终端原生"路径

两者共享同一套调色板 `terminal_default()`，但触发方式不同：

| | minimal 模式 | `terminal` 主题 |
| --- | --- | --- |
| 触发 | `TERMINAL_NATIVE_LOCK`（`cache.rs:27-28, 117-120`；由 `set_terminal_native_lock(minimal)` 设置，`app/mod.rs:394`） | 用户选中 `ThemeKind::Terminal` |
| 生效范围 | 整个会话锁死，`current_kind()` 直接报 `GrokNight`（`cache.rs:65-71`） | 可随时切走 |
| 名字可解析 | — | 受 rollout gate `TERMINAL_THEME_ENABLED` 控制，关闭时名字不解析、目录里不出现（`cache.rs:30-32, 124-135`；`mod.rs:53-67, 146-159`） |
| 色彩能力 | 被 `detect()` 封顶到 `Basic`（`color_support.rs:85-88`） | 不受封顶 |

调色板设计（`terminal_default.rs:1-20` 模块注释给了完整理由）：

- **所有背景为 `Reset`**，让终端画布（含半透明/背景图）透出来；有测试断言 `bg_*`、`md_code_bg`、`diff_*_bg`、`paste_bg` 全为 `Reset`（`terminal_default.rs:207-222`）。
- **正文与次级文本一律 `Reset` + `DIM`**，而不是画亮黑（ANSI 8）。注释明确警告：许多暗色 profile 把亮黑调得很暗，画上去会把 tool 输出和信息行洗掉（`terminal_default.rs:10-21`）。因此 `muted()` / `dim()` 在 bandless 调色板上走 `Modifier::DIM`（`tokyonight.rs:239-244, 265-271`，测试 `terminal_default.rs:339-362`）。
- **`terminal()` 变体**在默认值上把装饰槽改为亮黑：`selection_border`/`hover_border`/`prompt_border`/`scrollbar_fg`/`gray_dim` → `Color::DarkGray`，而 `prompt_border_active` 保持 `Reset`（全亮前景）让聚焦仍然醒目（`terminal_default.rs:113-126`）。
- **零不透明单元**：选中/悬停/块光标一律用 `REVERSED` 反显（`tokyonight.rs:293-319`），`is_bandless()` 是判定开关。
- **语法高亮不跟着明暗走**：该模式下 `polarity_safe_syntax_fg` 把 syntect 的低彩度 token 映射为默认前景、彩色 token 映射到 ANSI 16 基色（`.../src/syntax.rs:54-86`），源码 palette 仍加载 night 主题文件，理由注释为"极性无关，只要 palette 完整"（`syntax.rs:119-132`）。

## 10. 边角细节（可能对复刻有用）

- `bg_terminal` 字段注释写着 `currently unused, using bg_dark instead`（`tokyonight.rs:53`）——字段仍在结构体里被量化与测试覆盖。
- 主题 rollout gate 独立于 minimal 锁：`selected_kind()` 存在就是为了看穿 minimal 锁的伪装，防止被 gate 关闭的主题在锁解除后复活（`cache.rs:72-86`）。
- 测试基础设施值得一提：`color_support::set_level_for_test` 在 `OnceLock` 写入前做覆盖，注释说明理由是"环境里的 `NO_COLOR` 会因为调度顺序而赢"（`color_support.rs:56-66`）；`cache::pin_theme()` 同时钉住主题与色深，理由同样是 agent shell 会导出 `NO_COLOR`（`cache.rs:324-334`）。
- 主题测试规模不小：`mod.rs` 共 1234 行，其中测试从第 630 行起；`ansi16_chrome_overrides` 单独就有十余个断言用例（`mod.rs:736-1094`）。

## 关键结论（供合稿引用）

1. 颜色只有一个来源：扁平 `Theme` 结构体的 58 个 `Color` + 6 个 `Modifier` 字段，没有任何硬编码色散落在渲染层（`.../theme/tokyonight.rs:46-144`、`.../theme/mod.rs:1`）。
2. 主题是编译期常量，不是静态表也不是资源文件：每个主题一个 `const fn` 构造函数 + 内联 `mod palette` 色值表（`.../theme/groknight.rs:15-52, 55-137`）。
3. 内置 6 个主题 + `auto` 元主题；`Auto` 不在 `ALL` 中，`Auto=4` 夹在 `RosePineMoon=3` 与 `OscuraMidnight=5` 之间，判别值被缓存当 `u8` 使用（`.../theme/mod.rs:28-51`、`.../theme/cache.rs:18, 36-47`）。
4. 主题可用性有三道过滤：rollout gate 剔 `terminal`、truecolor 能力剔 TokyoNight/RosePine/Oscura（`.../theme/mod.rs:44-95, 110-122`）。
5. 降级链是四档 `None < Basic < Ansi256 < TrueColor`，探测顺序为 `NO_COLOR` → `supports-color` → 非 TTY 假设 truecolor → 终端品牌白名单升级（`.../theme/color_support.rs:83-127, 246-269`）。
6. `TERM` / `COLORTERM` 的显式解析只存在于 `grok doctor` 的独立路径，主 TUI 路径依赖 `supports-color` + 品牌推断（`.../theme/color_support.rs:172-203`）。
7. `ColorLevel::None` 的实现方式是"所有色值塌缩为 `Color::Reset`"，配合 `muted()`/`dim()` 的 `Modifier::DIM` 达到无彩效果（`.../theme/color_support.rs:232`、`.../theme/tokyonight.rs:239-244`）。
8. 量化发生在 `Theme::quantized()`（58 次逐字段）与 `Theme::current()` 中；后者还叠加了"先采极性再量化"、Windows 对比度补偿、Basic/legacy ConHost 的 ANSI16 chrome override 三层修正（`.../theme/mod.rs:205-289, 293-333`）。
9. OSC 11 探测的是**背景明暗**而非色彩能力，且严格限于启动期（crossterm `EventStream` 接管 stdin 之前），亮度判定用 BT.709 + sRGB 反 gamma、阈值 0.5（`.../theme/osc11.rs:13-14, 23, 65-74`）。
10. 系统外观四级链为 desktop API → `GROK_APPEARANCE`/`LC_GROK_APPEARANCE` → 启动期 OSC 11（结果缓存）→ `COLORFGBG`；运行时以 5 秒轮询、值变化才通知（`.../theme/system_appearance.rs:3-11, 53-79, 121-171`）。
11. 热切换的唯一状态写入入口是 `Theme::apply_kind`：先按 truecolor 能力钳制到 GrokNight，再 `cache::set` → `sync_markdown_polarity`，预览路径刻意不落盘（`.../theme/mod.rs:348-363`、`.../theme/cache.rs:89-100, 137-146`、`crates/codegen/xai-grok-pager/src/slash/commands/theme.rs:8, 49-65`）。
12. `terminal` 主题用"全 `Reset` 背景 + named ANSI16 强调 + 反显选中"实现极性无关，并为内容文本保留 `Reset + DIM` 路径以免被亮黑洗白；它和 minimal 模式共用同一调色板但由不同开关控制（`.../theme/terminal_default.rs:1-20, 113-126`、`.../theme/cache.rs:27-32`）。
