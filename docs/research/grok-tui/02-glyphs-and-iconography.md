# 专题 02：字形、图标与边框符号系统

> 专题：字形、图标与边框符号系统｜调研日期：2026-09-22｜证据范围：`crates/codegen/xai-grok-pager-render/src/glyphs.rs`（全文 683 行）、`xai-grok-pager-render/src/render/{draw,line_utils,scrollbar,wrapping,bidi,safe_buf}.rs`、`xai-grok-pager-render/{src/util.rs,src/theme/mod.rs}`、`xai-grok-pager/src/{views/*,scrollback/blocks/*,app/*}`、`xai-grok-pager-minimal/src/*`、`xai-grok-markdown/src/{parse,render,style}.rs`、`xai-grok-pager/assets/logo/{logo05,logo07}.txt`

## 1. 三十秒总览

整套符号系统有三个层次，**只有第一层是集中的，第二三层是分散的**：

```
┌─ 层 1：chrome 字形（集中，单一事实来源）───────────────────────┐
│  xai-grok-pager-render/src/glyphs.rs                         │
│  → 34 个 pub fn，每个返回 &'static str 或 &'static [&str]    │
│  → 每对 (fancy, fallback) 都有 1 列宽度断言测试              │
└──────────────────────────────────────────────────────────────┘
┌─ 层 2：结构字形（分散，由各视图硬编码）───────────────────────┐
│  框线 ╭─╮│╰╯  → views/prompt_widget/mod.rs:3049-3358        │
│  进度条 ▏▎▍  → views/progress_bar.rs:16-29（含独立 fallback）│
│  表格 ─│┌┐└┘  → xai-grok-markdown/src/style.rs:28-38        │
│  滚动条 █     → render/scrollbar.rs:268                      │
└──────────────────────────────────────────────────────────────┘
┌─ 层 3：Markdown 字符改写（解析期，不是渲染期）────────────────┐
│  `>`→`│`  `- `→`• `  `---`→`───`                            │
│  xai-grok-markdown/src/parse.rs:836,924,1040                │
└──────────────────────────────────────────────────────────────┘
```

**最关键的一条设计事实**：`glyphs.rs` 的降级机制**不是**「非 Unicode 终端 → ASCII」，而是「**legacy Windows ConHost → ASCII / CP437**」。非 Windows 平台永远走 fancy 分支，没有任何探测或开关。

## 2. 集中字形表：`glyphs.rs`

### 2.1 模块定位

模块注释（`glyphs.rs:1-7`）说清了全部动机：

```rust
//! Legacy-console fallbacks for chrome glyphs that don't ship in the legacy Windows ConHost
//! default font (Consolas / Lucida Console).
//!
//! Fallbacks are ASCII where possible (`x`, `o`, `c`, `*`), or a CP437 glyph when one reads
//! better and still renders on the raster font.
//!
//! ConHost does no font fallback, so missing glyphs render as tofu.
```

即：**ConHost 不做字体回退，缺失字形一律显示为豆腐块（tofu）**。Windows Terminal / VS Code / 现代终端自带可覆盖这些 chrome 字形的字体，所以替换只在 `cmd.exe` / `powershell.exe` 上触发。

### 2.2 完整词汇表

宽度列的含义：调用方会用这些宽度做布局运算，因此每个字形与它的 fallback **必须同宽**。

#### 输入与提示（prompt）

| 字形 | 码点 | 用途 | 出处 |
| --- | --- | --- | --- |
| `❯ ` | U+276F +空格 | 用户消息前缀箭头，2 列 | `glyphs.rs:16-25` |
| `> ` | ASCII | 同上，legacy fallback | `glyphs.rs:18` |

#### 状态与判定（status / verdict）

| 字形 | 码点 | 用途 | 出处 |
| --- | --- | --- | --- |
| `◆` | U+25C6 | 实心菱形，容器/工具/运行中 | `glyphs.rs:111-117` |
| `◇` | U+25C7 | 空心菱形，空闲 | `glyphs.rs:120-126` |
| `◈` | U+25C8 | 点状菱形，tools 用量 | `glyphs.rs:129-135` |
| `●` | U+25CF | 实心圆点（选中标记、单选） | `glyphs.rs:237-243` |
| `○` | U+25CB | 空心圆（未完成、idle 标记） | `glyphs.rs:120-126`（hollow 的 fallback） |
| `✓` | U+2713 | 完成 | `glyphs.rs:59-65` |
| `✗` | U+2717 BALLOT X | 失败 / 关闭 / 取消 / kill | `glyphs.rs:47-56` |
| `❙` | U+2759 | 折叠块强调条 | `glyphs.rs:39-45` |
| `◉` / `◎` | U+25C9 / U+25CE | 录音脉冲（实/空环） | `glyphs.rs:28-36` |

#### 动画帧表（有状态的是「帧序列」，不是单个字形）

| 名称 | fancy 帧 | 帧数 | fallback 帧 | 出处 |
| --- | --- | --- | --- | --- |
| braille 转轮 | U+280B `⠋` `⠙` `⠹` `⠸` `⠼` `⠴` `⠦` `⠧` | 8 | `\|` `/` `-` `\` | `glyphs.rs:148-159` |
| 点转轮 | U+22C5 `:` U+2E2C U+2059（×2） | 8 | `.` `:` U+00B7 | `glyphs.rs:162-172` |
| monitor 脉冲 | U+25CB U+25CE U+25C9 U+25CE | 4 | U+00B7 `○` U+2022 `○` | `glyphs.rs:100-108` |

注意 monitor 脉冲的注释（`glyphs.rs:98-99`）：**「只有 ○ 在 CP437 里，所以 legacy ConHost 靠填充而非靠大小做脉冲。每一帧都是 1 列，后面的标签才不会位移。」** 这是整套 fallback 设计的核心约束——动画不允许改变宽度。

节拍由各视图自己的除数控制：`SPINNER_DIVISOR = 4`（`views/turn_status.rs:31`，~30fps 下约 7.5 帧/秒），`MONITOR_PULSE_DIVISOR = 8`（`views/turn_status.rs:36`，约 3.75 帧/秒，「idle 提示应该缓慢呼吸，而不是读起来像运行中转轮」），`NEEDS_INPUT_BLINK_DIVISOR = 10`（`views/dashboard/animation.rs:7`）。

#### 框线、轨道与条（rule / rail / bar）

| 字形 | 码点 | 用途 | 出处 |
| --- | --- | --- | --- |
| `┃` | U+2503 | accent rail（thinking 正文竖轨、权限视图） | `glyphs.rs:175-181` |
| `│` | U+2502 | 同上 fallback；引用块竖条 | `glyphs.rs:177`、`parse.rs:924` |
| `▏` | U+258F | 选中行左侧细条 | `glyphs.rs:248-254` |
| `─` | U+2500 | 浅横线（唯一无 fallback 的结构字形） | `glyphs.rs:216-218` |
| `━` | U+2501 | 重横线 | `glyphs.rs:206-212` |
| `━━` | U+2501×2 | timeline 活跃刻度（预组合 2 列） | `glyphs.rs:222-228` |
| `──` | U+2500×2 | timeline hover 刻度 | `glyphs.rs:232-234` |
| `═`×2 | U+2550 | `━━` 的 CP437 fallback | `glyphs.rs:224` |

#### 箭头与折叠标记（arrow / chevron / disclosure）

| 字形 | 码点 | 用途 | 出处 |
| --- | --- | --- | --- |
| `⇣` | U+21E3 | token 计数箭头 | `glyphs.rs:90-96` |
| `↗` | U+2197 | 放大 / 后台任务视图 | `glyphs.rs:68-74` |
| `⧉` | U+29C9 | 复制按钮 | `glyphs.rs:79-85` |
| `›` / `‹` | U+203A / U+2039 | chevron 右/左（面包屑、步进器） | `glyphs.rs:256-274` |
| `⌄` | U+2304 | 轻量下 chevron（与 `›` 同字重） | `glyphs.rs:277-283` |
| `▾` / `▸` | U+25BE / U+25B8 | disclosure 展开/折叠 | `glyphs.rs:288-303` |
| `▴` | U+25B4 | disclosure 上；timeline 上 chevron | `glyphs.rs:184-190, 306-312` |
| `▲` / `▼` | U+25B2 / U+25BC | 小三角的 CP437 替代 | `glyphs.rs:186, 197` |

#### 预组合按钮（避免每帧 `format!`）

| 字形 | 宽度 | 出处 |
| --- | --- | --- |
| `[✗]` / `[x]` | 3 列 | `glyphs.rs:317-323` |
| `[↗]` / `[o]` | 3 列 | `glyphs.rs:328-334` |

注释（`glyphs.rs:316`）：「预组合成 `&'static str`，这样每帧渲染路径复用静态字符串，而不是每帧 `format!` 分配一次。」

## 3. ASCII fallback 机制

### 3.1 没有通用的 ASCII fallback

**答案很明确：不存在「非 Unicode 终端 → ASCII」的通用机制。** 唯一的切换开关是 legacy Windows ConHost 这一个布尔量。

### 3.2 判定位置与逻辑

判定函数是 `glyphs.rs:377-424`：

```rust
pub fn is_legacy_windows_console() -> bool {
    static CACHE: OnceLock<bool> = OnceLock::new();
    *CACHE.get_or_init(|| {
        forced_legacy_console_override().unwrap_or_else(|| {
            decide_legacy_windows_console(HostOs::current(), terminal_context().env_brand)
        })
    })
}
```

三个要点：

1. **结果被缓存**（`OnceLock`），一次探测终身有效。
2. **平台短路**：`decide_legacy_windows_console` 第一句就是 `if host != HostOs::Windows { return false; }`（`glyphs.rs:407-409`）。
3. **default-deny**：Windows 上只要终端品牌**不在白名单**就判为 legacy（`glyphs.rs:410-423`）。白名单含 11 个品牌：`WindowsTerminal`、`VsCode`、`Cursor`、`Windsurf`、`Zed`、`WezTerm`、`Kitty`、`Alacritty`、`Ghostty`、`Rio`、`GrokDesktop`。注释（`glyphs.rs:404-405`）解释了原因：「裸 `cmd.exe` / `powershell.exe` 在 ConHost 里**不设置任何终端环境变量**，所以品牌探测恰好在我们要抓的那个场景返回 `Unknown`」——因此不认识就当作 legacy。

**QA 逃生舱**：环境变量 `GROK_FORCE_LEGACY_CONSOLE`（`glyphs.rs:388-401`）。`"1"` / `"true"` 强制开，`"0"` / `"false"` 强制关，其余值（含未设置）走正常探测。注释：「让 QA 不必用 ConHost 也能检查 fallback」。

一个易踩的坑记录在 `glyphs.rs:381-382`：必须用 `terminal_context().env_brand` 而**不是** `brand`，因为后者在原生 Windows 上会乐观地变成 `WindowsTerminal`，导致 legacy 终端拿不到 fallback。

### 3.3 各 fallback 表全貌

`glyphs.rs` 里共有 **5 张表格常量**，全部以 `FANCY` / `FALLBACK` 成对出现：

```rust
// glyphs.rs:101-102  monitor_icon_frames
const FANCY:    &[&str] = &["\u{25CB}", "\u{25CE}", "\u{25C9}", "\u{25CE}"];
const FALLBACK: &[&str] = &["\u{00B7}", "\u{25CB}", "\u{2022}", "\u{25CB}"];

// glyphs.rs:149-153  braille_spinner_frames
const FANCY:    &[&str] = &["\u{280b}","\u{2819}","\u{2839}","\u{2838}",
                            "\u{283c}","\u{2834}","\u{2826}","\u{2827}"];
const FALLBACK: &[&str] = &["|", "/", "-", "\\"];

// glyphs.rs:163-166  dot_spinner_frames
const FANCY:    &[&str] = &["\u{22c5}", ":", "\u{2e2c}", "\u{2059}",
                            "\u{22c5}", ":", "\u{2e2c}", "\u{2059}"];
const FALLBACK: &[&str] = &[".", ":", "\u{00b7}"];
```

注意帧数**可以不等**（braille 8→4，dot 8→3，monitor 4→4），注释解释了 dot 的取舍：`glyphs.rs:161`「那些码点不在 CP437 里，所以 legacy ConHost 用一个 1 列的圆点循环。」

**逐字符映射函数**（供 toast 等自由文本使用，`glyphs.rs:364-373`）：

```rust
fn to_legacy_glyphs(s: &str) -> String {
    s.chars().map(|c| match c {
        '\u{2713}' => '\u{221A}',  // ✓ → √
        '\u{2717}' => 'x',          // ✗ → x
        '\u{26A0}' => '!',          // ⚠ → !
        other => other,
    }).collect()
}
```

外层 `legacy_glyph_fallback`（`glyphs.rs:337-345`）先做 `s.contains(['\u{2713}','\u{2717}','\u{26A0}'])` 快速预检，不含这三个字符就 `Cow::Borrowed` 零拷贝返回。`sanitize_toast_message`（`glyphs.rs:349-360`）再叠一层「控制字符 → 空格」。

### 3.4 影响面：`is_legacy_windows_console()` 的全部 6 个调用点

这是 fallback 真正波及的完整清单（`rg` 全仓确认）：

| 位置 | 行为 |
| --- | --- |
| `views/progress_bar.rs:24` | 进度条整表从 `BLOCKS` 换 `SHADES` |
| `views/welcome/logo.rs:77` | Braille logo **整体隐藏**（无 ASCII 替身） |
| `scrollback/blocks/thinking.rs:54` | 去掉 `ITALIC` 修饰符 |
| `scrollback/types.rs:140` | 折叠块不再变暗——「16 色量化后选中/未选的颜色落差读起来像调色板噪声」 |
| `app/agent_view/render.rs:734` | 提示前缀 `●` 换成 `•` |
| `theme/mod.rs:327` | legacy + 非 truecolor 时启用 `ansi16_chrome_overrides` 重映射主题色 |

### 3.5 两处 fallback 漏网（实测发现）

1. **`scrollback/blocks/workflow.rs:66-68`** 直接硬编码字面量：
   ```rust
   let mark = match p.state.as_str() {
       "done" => "✓",
       "active" => "●",
       _ => "○",
   };
   ```
   没走 `glyphs::check_mark()` / `glyphs::filled_dot()`，因此在 legacy ConHost 上不会被替换。

2. **`views/goal_detail.rs:679-681`** 同样是字面量 `"\u{25a1}"` / `"\u{25b6}"`，而同一 `match` 的另外两个分支（`Completed` / `Cancelled`）却走了 `crate::glyphs::check_mark()` / `ballot_x()`。同一个四分支 `match` 里两种取字形方式并存。

对比正面例子：`views/todo_pane.rs:86-91` 的同名枚举**全部**走 `glyphs::`，是正确写法。

## 4. 第二张集中表：表格边框 `TableBorders`

`xai-grok-markdown/src/style.rs:9-38` 定义了用 11 个 `char` 槽位表达表格边框的结构：

```rust
const H: usize = 0;  const V: usize = 1;
const TL: usize = 2; const TR: usize = 3; const BL: usize = 4; const BR: usize = 5;
const T_T: usize = 6; const T_B: usize = 7; const T_L: usize = 8;
const T_R: usize = 9; const X: usize = 10;

pub const BOX: Self = Self {
    chars: ['─', '│', '┌', '┐', '└', '┘', '┬', '┴', '├', '┤', '┼'],
};
pub const ASCII: Self = Self {
    chars: ['-', '|', '+', '+', '+', '+', '+', '+', '+', '+', '+'],
};
pub const DOUBLE: Self = Self {
    chars: ['═', '║', '╔', '╗', '╚', '╝', '╦', '╩', '╠', '╣', '╬'],
};
```

**重要限制（已核实）**：`ASCII` 与 `DOUBLE` 两个预设**在本快照中从未被选中**。`rg` 全仓只有：

- `xai-grok-markdown/src/parse.rs:1589`：`let borders = TableBorders::BOX;` —— `format_table` 里硬编码。
- `xai-grok-pager-render/src/render/wrapping.rs:309`：只在注释里提到「ASCII table borders (TableBorders::ASCII uses '+' and '|')」。

`MarkdownStyle`（`style.rs:124-149`）也**没有** `table_borders` 字段（只有 `table_outer: Style`），`Default for TableBorders` 返回 `BOX`（`style.rs:115-119`）。所以这是「API 预留了 3 套、实际只接线了 1 套」的接口，**不是**由终端能力驱动的降级路径——它和 `glyphs.rs` 的 ConHost 判定是两条互不相干的机制。

## 5. Markdown 字符改写发生在解析期

Markdown 渲染里的符号**不是**渲染层挑的，而是解析阶段写进 `Transform` 缓冲区的：

| 源字符 | 目标 | 出处 |
| --- | --- | --- |
| `>`（引用） | `│` U+2502 | `parse.rs:903,924` |
| `- ` / `* `（无序列表） | `• ` U+2022 | `parse.rs:1040-1044` |
| `---`（分隔线） | `───` U+2500×3 | `parse.rs:835-845` |

引用条的改写在 `parse.rs:903` 有明确注释：「Transform the `>` belonging to THIS blockquote level to `│`」。有序列表（`1. ` / `1) `）**不改写**，只加 `list_item` 高亮（`parse.rs:1022-1036`）。

下游因此要做两件配套工作：

- **拖选排除**：`scrollback/blocks/quote_bar.rs:6-10` 说明引用条会「泄漏进拖选复制内容」，所以按「条必须是 `bar_style` 着色」精确识别前缀并标成 `Selectable::Spans`（`quote_bar.rs:54-92`）。
- **换行判定**：`render/wrapping.rs:288-311` `is_table_line()` 靠框线字符本身区分表格与引用块：

```rust
Some(ch) if ('\u{2500}'..='\u{257F}').contains(&ch) && ch != '\u{2502}' && ch != '\u{2503}' => true,
// │ (U+2502) at line start is a table content row only if │ appears after the leading prefix region
Some('\u{2502}') => { /* 扫描行首 │ 与空格组成的 prefix，之后再见 │ 才算表格 */ }
Some('|') => true,   // ASCII table borders
```

即 **U+2500–U+257F 区间的任何非竖线字符一律判为表格**——这是个用码点区间替代语法分析的巧妙捷径。

## 6. 品牌与装饰

### 6.1 logo 资产

两份 Braille 点阵资产，用 `include_str!` 在编译期内嵌：

- `assets/logo/logo07.txt` → `LOGO`（7 行，`views/welcome/logo.rs:12`）
- `assets/logo/logo05.txt` → `LOGO_SMALL`（5 行，`views/welcome/logo.rs:13`）

实际内容是 U+2800 区块的 Braille 图案（`logo07.txt:1-7` 形如 `⠀⠀⠀⠀⠀⠀⣀⣀⡀⠀⠀⠀⢄`）。

### 6.2 三档高度策略

`views/welcome/logo.rs:16-51`：

```rust
const SMALL_LOGO_MIN_HEIGHT: u16 = 22;
const FULL_LOGO_MIN_HEIGHT: u16 = 26;

if hidden || window_height < SMALL_LOGO_MIN_HEIGHT { Self::Hidden }
else if window_height < FULL_LOGO_MIN_HEIGHT { Self::Compact }
else { Self::Full }
```

`LOGO` 输出 → `LOGO_SMALL` → `None`，且有 `step_down()`（`logo.rs:58-64`）让堆叠布局在放不下时逐档退让。另一种入口 `render_full_logo`（`logo.rs:229-233`）用于 hero box：它**永远用大图且不看高度**，因为「hero box 是排在菜单旁边的，box 放得下它就放得下」（`logo.rs:210-212`）。

### 6.3 legacy ConHost 上整体隐藏

```rust
/// The braille art has no ASCII stand-in; see the module doc.
fn logo_hidden() -> bool {
    crate::glyphs::is_legacy_windows_console()
}
```

`logo.rs:1` 的模块文档：「logo 在 legacy Windows 控制台上**完全隐藏**：ConHost 的栅格字体不覆盖 U+2800 Braille 区块，会渲染成豆腐块。」这里**没有** ASCII 替身，是整套系统里唯一「降级 = 不显示」的情况。

### 6.4 shimmer 动画

logo 不是静态的。`shine_opacity()`（`logo.rs:121-141`）用一条升余弦波带沿对角线扫过：

```rust
const BAND: f32 = 0.38;      // 光带半宽
const CYCLE: f32 = 4.0;      // 一次扫过 + 静止的总秒数
const SWEEP_FRAC: f32 = 0.32; // 周期中用于扫过的比例（~1.3s 闪光，其余静止）
const SHINE: f32 = 0.33;      // 峰值闪耀强度
const PULSE: f32 = 0.06;      // 全局呼吸幅度
const PULSE_SECS: f32 = 5.0;  // 呼吸周期
```

- **相位基于墙钟**（`anim_phase_secs()`，`logo.rs:97-102`），所以「闪烁速度与帧率无关」。
- **节流到 12 FPS**：`SHIMMER_FPS = 12.0`，`shimmer_frame()`（`logo.rs:109-116`）返回量化帧号，欢迎屏只在它前进时才重绘。
- **对角坐标**：`diag = (col + (rows - 1 - row)) / (cols + rows)`（`logo.rs:167`），从**左下到右上**。
- **颜色混合**：`blend_color(theme.gray, theme.text_primary, opacity)`（`logo.rs:168`）——静止态是灰、高光是主文本色。
- **相邻同色合并成 Span**（`logo.rs:165-181`）：「降低每帧分配」。

**水印：未找到证据。** 除 welcome 屏的 logo 外，未发现任何全局水印 / 背景图案绘制代码。

## 7. 宽度处理

### 7.1 基础工具

全部宽度运算走 `unicode_width` crate，三处入口：

| 函数 | 出处 | 说明 |
| --- | --- | --- |
| `truncate_to_width` | `util.rs:204-215` | 截到 N 显示列，切断时补 `…`（U+2026） |
| `byte_offset_at_width` | `util.rs:219-229` | 返回会超出 N 列的字节偏移 |
| `pad_to_width` | `util.rs:233-238` | 左对齐补齐。注释：「`format!` 的 `{:<width$}` 按 char 数补齐，宽字形（如 CJK）之后列会错位」 |

`truncate_to_width` 的实现有个易错细节（`util.rs:209`）：预算 N 列时先按 `max_width - 1` 取前缀再拼 `…`，**给省略号留一列**。

### 7.2 宽字符与 emoji：按 grapheme 簇裁剪 + 补空格

核心在 `render/line_utils.rs:104-166` `fit_line_to_width`。文档注释（`line_utils.rs:102-103`）给出理由：

> 在 grapheme 边界裁剪（不加省略号）或用空格补齐，让每个列都是真实单元格。否则**更宽的字形会遗留一个陈旧单元格（表格旁边的幽灵字形）**。

裁剪逻辑（`line_utils.rs:138-158`）：

```rust
// This span straddles the boundary: take whole graphemes that fit
for g in span.content.graphemes(true) {
    let gw = g.width();
    if taken_width + gw > remaining { break; }
    taken_width += gw;
    taken.push_str(g);
}
// A straddling wide grapheme leaves a 1-column gap; pad it.
if used < width { out.push(Span::raw(" ".repeat(width - used))); }
```

关键行为：**放不下的宽 grapheme 整簇丢弃，空出的列用空格补齐**。测试固化了这一点（`line_utils.rs:327-334`）：

```rust
// The line is 4 columns wide: a and b, then the width-2 ⚠️ grapheme
// Clipping to 3 drops that grapheme whole and pads with one space
let line = Line::from(vec![Span::raw("ab\u{26A0}\u{FE0F}")]);
let out = fit_line_to_width(line, 3);
assert_eq!(line_text(&out), "ab ");
```

注意这里用的是 `graphemes(true)`——**`⚠` + U+FE0F 变体选择符算一个 grapheme**，避免把变体选择符留在行尾。

### 7.3 两套宽度模型不一致的问题

`views/status_line/tests.rs:129-139` 明确指出「两种宽度模型的每一个分歧点」：

```rust
// One case per way the two width models disagree, ASCII being the control:
// under `Line::width` two of these cases fit and would assert nothing
for (name, cluster) in [
    ("ascii", "hello world "),
    ("variation selector", "\u{26a0}\u{fe0f}"),
    ("zwj", "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}"),
] { ... }
```

于是 status line 自己实现了「实际绘制宽度」模型（`views/status_line/mod.rs:25-35`）：

```rust
/// The clusters ratatui paints; it drops control-bearing and zero-width ones.
fn painted_clusters(text: &str) -> impl Iterator<Item = (&str, usize)> {
    text.graphemes(true)
        .filter(|cluster| !cluster.contains(char::is_control))
        .map(|cluster| (cluster, UnicodeWidthStr::width(cluster)))
        .filter(|(_, columns)| *columns > 0)   // 丢弃零宽簇
}
```

**零宽字符被直接丢弃**，不参与宽度累计。

### 7.4 零宽与 bidi 字符：安全清洗

`render/line_utils.rs:31-44` 定义了「不可安全显示的字符」集合：

```rust
pub fn is_unsafe_display_char(c: char) -> bool {
    c.is_control()
        || matches!(c,
            '\u{061C}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}'
            | '\u{FEFF}')
}
```

文档注释（`line_utils.rs:32-33`）：「C0/C1 控制字符可以注入终端转义序列；bidi 控制符与零宽格式字符会开启 Trojan-Source 欺骗。」并说明「每一处清洗不可信文本的地方（chip 标签、toast 错误清洗、设置编辑器输入）都调用它，让集合不会漂移」。

注意 `'\u{200D}'`（ZWJ）**不在**这个不安全集合里——因为它需要参与 emoji 簇的宽度计算。

### 7.5 越界保护

`render/safe_buf.rs` 提供 `set_line_safe` / `set_span_safe` / `set_string_safe`，模块注释（`safe_buf.rs:1-3`）：「ratatui 的 `Buffer::set_line` / `set_span` / `set_string` 在收到越界坐标时会 panic（经由 `index_of`）。终端 resize 竞态下算出的 widget 区域可能瞬时超出缓冲区；这些辅助函数跳过写入而不是 panic。」

## 8. 滚动条与进度条：两个独立的块字符用户

### 8.1 滚动条：整格 `█`

`render/scrollbar.rs` 把 ratatui-core 的 `ScrollBar` 渲染到临时 buffer，再逐格复制并改写符号（`scrollbar.rs:246-270`）：

```rust
if src.symbol() == " " {
    dst.set_symbol(" ");            // 轨道
    dst.set_style(track_style);
} else {
    dst.set_symbol("\u{2588}");     // U+2588 全块 = thumb
    dst.set_style(thumb_fill);
}
```

不做 1/8 分辨率——thumb 永远是整格 `█`。有个 macOS Terminal.app 的兼容处理（`scrollbar.rs:410-411`）：thumb 单元格的 `bg` **必须等于** `fg`，否则「行间空隙像素没被涂到，thumb 会呈现深色横条纹」。

### 8.2 进度条：1/8 格分辨率 + 独立 fallback

`views/progress_bar.rs:1-29` 是两个含 fallback 的常量表：

```rust
/// LEFT-fractional block glyphs, indexed 0..=8 (0 is empty, 8 is full).
const BLOCKS: [&str; 9] = ["", "▏", "▎", "▍", "▌", "▋", "▊", "▉", "█"];
/// Shade substitutes used on hosts that can't render the LEFT-fractional blocks.
const SHADES: [&str; 9] = ["", "░", "░", "░", "▒", "▒", "▓", "▓", "█"];

fn partial_blocks() -> &'static [&'static str; 9] {
    if crate::glyphs::is_legacy_windows_console() { &SHADES } else { &BLOCKS }
}
```

模块注释（`progress_bar.rs:3-5`）解释了原因与代价：**「Consolas（ConHost 默认字体）缺少那些窄字形（U+258F..=U+2589，microsoft/terminal#387）。legacy ConHost 上我们用 CP437 的灰度字形 `░▒▓` 替代。同样是 1/8 分辨率输入，只是单元格读起来像密度图案，而不是真正的左对齐进度条。」**

分辨率算法在 `cell_breakdown`（`progress_bar.rs:32-38`）：先把填充比例换算成总八分数 `(value * width * 8).round()`，再拆成整格 + 余数。

另有 `views/usage_modal.rs:817-822` 直接手写 `"█".repeat(filled)` + `"░".repeat(...)` 的 30 列用量条，**未走** `progress_bar` 模块。

## 9. 符号与颜色的组合约定

### 9.1 集中定义只有两处，且都是局部的

**没有任何全局的「符号 × 颜色」表。** 最接近集中的是 dashboard 的一对相邻函数（`views/dashboard/render.rs:3027-3051`）：

```rust
fn state_icon(state: RowState, tick: u64) -> &'static str {
    match state {
        RowState::Working => { /* dot_spinner_frames 按 tick 取帧 */ }
        // Hollow diamond for idle rows; filled diamond for every state that needs visual presence
        // (needs-input, completed, failed)
        // The foreground colour disambiguates (accent_user for needs-input, accent_success for done,
        // accent_error for failed)
        RowState::Idle | RowState::Inactive => crate::glyphs::diamond_hollow(),
        RowState::NeedsInput | RowState::Completed | RowState::Failed => crate::glyphs::diamond_filled(),
    }
}

fn state_color(state: RowState, theme: &Theme) -> Color {
    match state {
        RowState::Working     => theme.accent_running,
        RowState::NeedsInput  => theme.warning,
        RowState::Idle | RowState::Inactive => theme.gray_dim,
        RowState::Completed   => theme.accent_success,
        RowState::Failed      => theme.accent_error,
    }
}
```

这是**代码注释里明确书写的「同符不同色」契约**：`NeedsInput` / `Completed` / `Failed` **共用 `◆`**，仅靠前景色区分。

同文件顶部（`views/dashboard/render.rs:24-27`）还记录了跨视图的形状契约：

> Row markers 用 `crate::glyphs` 的实心 `◆` / 空心 `◇`（legacy 控制台有 CP437 fallback）。**dashboard 用菱形而不是圆形，这样它读起来与兄弟 activity 视图（那些用圆形）不同。**

### 9.2 「等待用户」的统一脉冲色

`views/turn_status.rs:38-47` 是唯一被文档化要求「必须统一调用」的颜色约定：

```rust
/// Pulse speed for every "waiting on you" diamond. Always route diamond rendering through
/// [`pending_diamond_color`] so the three call sites can never silently drift apart.
pub(crate) const USER_WAITING_PULSE_SPEED: f32 = 0.08;

pub(crate) fn pending_diamond_color(theme: &Theme, accent: Color, tick: u64) -> Color {
    let brightness = crate::theme::pulse_brightness(tick, USER_WAITING_PULSE_SPEED);
    crate::render::color::blend_color(theme.bg_base, accent, 0.3 + brightness * 0.7)
        .unwrap_or(accent)
}
```

`views/turn_status.rs:1576` 补充说明 drain-blocked / pending-user-input / plan-approval 三个 cue 共享同一个常量。而 `turn_status.rs:366-368` 记录了符号替换规则：「工具阻塞在权限提示或 `ask_user_question` 上时，把运行中的 braille 转轮换成脉冲的 `◆`」——**同一个 `◆`，从「等待用户」的角度统一了三种不同来源的阻塞**。

### 9.3 「同符同色」的显式依赖

`glyphs.rs:128-135` 的注释承认了一个设计依赖：

> Dotted diamond; legacy ConHost **shares [`diamond_filled`]'s `♦` because call sites already distinguish by color.**

即：legacy 回退时 `◈` 与 `◆` 变成同一个字形，**完全依赖颜色区分**。同理 `glyphs.rs:220-221`：

> Prefer [`timeline_tick_active`] for the sidebar rail: on legacy ConHost this falls back to the same light stroke used for hover. （`━` → `─`，与 hover 的 `─` 重合）

### 9.4 实例：上下文构成条

`scrollback/blocks/context_info.rs:248-264` 是最典型的「符号 + 颜色共同编码」案例：

```rust
// Per-category colors used in both the bar and the legend so the two visualizations are
// scannable side-by-side
// System prompt uses the same diamond glyph as messages but in muted gray so it reads as a
// quiet base layer
let system_glyph    = crate::glyphs::diamond_filled(); // ◆ (gray)
let tools_glyph     = crate::glyphs::diamond_dotted(); // ◈
let messages_glyph  = crate::glyphs::diamond_filled(); // ◆ (primary)
let free_glyph      = crate::glyphs::diamond_hollow(); // ◇
let overhead_glyph  = crate::glyphs::diamond_filled(); // ◆ (violet)
```

视觉层次（同一行 bar 内的列序列）：

```
◆ ◆ ◆ ◈ ◈ ◈ ◆ ◆ ◆ ◇ ◇ ◇ ◇ ◇
└─gray──┘└teal─┘└violet┘└─dim──┘
 system  tools  overhead  free
```

5 个类别只用了 **3 种字形**，靠 5 色区分；注释明确说明这是刻意选择——「Messages 拿到最亮的处理（primary），因为它们主导构成分解，也是用户真正在引导的对话」。

## 10. 光标

**光标形状不走字形，走终端转义序列。** `app/mod.rs:1430-1447` 定义了 tri-state 策略：

```rust
enum CursorStylePolicy {
    Inherit,        // 不动终端光标样式（默认）
    ForceBlinking,  // EnableBlinking + SetCursorStyle::BlinkingBlock
    ForceSteady,    // DisableBlinking + SetCursorStyle::SteadyBlock
}
```

由 `[ui].cursor_blink` 三态配置映射（`app/mod.rs:1446-1452`），在 `init_terminal` 中执行（`app/mod.rs:1509-1524`）。**默认 `Inherit`，即尊重用户终端自己的光标设置。**

主输入框（prompt）用**真实终端光标**：`views/prompt_widget/mod.rs:3394` 计算 `cursor_pos` 交给终端的光标定位转义。而模态窗口里的文本输入用**假光标**——直接画一个 `selection_bar()` 字形（`views/settings_modal/render.rs:1667,1686`）。

`CursorAction`（`render/draw.rs:436-497`）会跟踪上次写入的终端光标位置，只发出必要的转义：「多余的 `Show`/`Hide`/`MoveTo` 会重置终端的闪烁计时器」（`draw.rs:437-438`）。

## 11. 对 jev-harness 的可借鉴点

1. **把 chrome 字形收进一个 `glyphs` 模块，每个字形一个函数**，而不是散落的字符串字面量。这使 `workflow.rs` 那种漏网可以被编译器/搜索发现。
2. **对每个 (fancy, fallback) 对写「宽度相同」的单元测试**。`glyphs.rs:455-525` 的 `icon_fallback_variants_are_one_column` / `spinner_frames_are_one_column` 等测试是低成本的回归护栏——动画帧宽度一变，后面所有标签都会抖动。
3. **动画帧序列必须等宽**，否则布局随动画节拍跳动。
4. **对不可信文本用白名单式清洗**（`is_unsafe_display_char`），而不是黑名单——bidi 与零宽格式字符合集会变。
5. **宽字符裁剪按 grapheme 簇整簇丢弃 + 补空格**，不要切成半个字符，否则宽字符旁边会留下幽灵单元格。
6. **明确写清「哪些字形靠颜色消歧」**。本项目的注释做到了（`glyphs.rs:128`、`dashboard/render.rs:3034-3035`），这是避免后续维护者「顺手改个字形」破坏语义的关键。

---

## 关键结论（供合稿引用）

1. **`xai-grok-pager-render/src/glyphs.rs` 是全项目唯一的 chrome 字形源**，含 34 个 `pub fn`，被 `xai-grok-pager` 与 `xai-grok-pager-minimal` 两个 crate 共同引用（`glyphs.rs:16-334`；跨 crate 用法见 `xai-grok-pager-minimal/src/todo.rs:102-104`）。
2. **不存在「非 Unicode 终端 → ASCII」的通用降级**；唯一开关是 legacy Windows ConHost，非 Windows 平台在 `decide_legacy_windows_console` 第一句就返回 `false`（`glyphs.rs:406-424`）。
3. **判定是 default-deny**：Windows + 终端品牌不在 11 项白名单内即视为 legacy，因为裸 `cmd.exe` / `powershell.exe` 不设任何终端环境变量（`glyphs.rs:404-423`）；结果用 `OnceLock` 缓存，并提供 `GROK_FORCE_LEGACY_CONSOLE` 供 QA 强制覆盖（`glyphs.rs:377-401`）。
4. **fallback 是「ASCII 优先、必要时用 CP437」而非纯 ASCII**：`✓`→`√`(U+221A)、`◆`→`♦`(U+2666)、`◇`→`○`(U+25CB)、`┃`→`│`(U+2502)、`━`→`─`(U+2500)、`▏`→`│`（`glyphs.rs:59-65, 111-126, 175-181, 206-218, 248-254`）。
5. **所有 (fancy, fallback) 对都被断言同宽**，因为动画/按钮布局依赖它：5 张帧表 + 6 组字符表共覆盖 20 余对（`glyphs.rs:426-558`，尤其 `spinner_frames_are_one_column` 在 `glyphs.rs:511-525`）。
6. **帧数可以不等**（braille 8→4、dot 8→3、monitor 4→4），但每帧恒为 1 列，这样后续标签不会随动画位移（`glyphs.rs:98-108, 147-172`）。
7. **Braille logo 是唯一「降级 = 不显示」的字形**：三档高度 22/26 行阈值（`views/welcome/logo.rs:16-51`），legacy ConHost 上整体隐藏，因为 U+2800 区块无 ASCII 替身（`views/welcome/logo.rs:1, 76-78`）。
8. **logo 带对角线 shimmer 动画**，12 FPS 节流、相位基于墙钟（与帧率解耦）、相邻同色合并 Span 以减少分配（`views/welcome/logo.rs:104-187`）。**未发现任何全局水印机制。**
9. **存在第二张集中的字形表 `TableBorders`**，用 11 个槽位配 `BOX`/`ASCII`/`DOUBLE` 三套预设，但**快照中只接线了 `BOX`**（`parse.rs:1589` 硬编码），`ASCII`/`DOUBLE` 从未被选中，`MarkdownStyle` 也没有对应字段（`xai-grok-markdown/src/style.rs:9-119`）。
10. **Markdown 的 `>`→`│`、`- `→`• `、`---`→`───` 是解析期字符改写**，渲染层需额外识别该前缀以排除拖选复制（`xai-grok-markdown/src/parse.rs:836,924,1040`；`scrollback/blocks/quote_bar.rs:6-10,54-92`）。
11. **表格与引用块的区分靠码点区间**：行首命中 U+2500–U+257F 且非竖线即判为表格，`│` 需扫描过前缀后再遇 `│` 才认表格，ASCII 兜底只看 `|`（`render/wrapping.rs:288-311`）。
12. **宽字符按 grapheme 簇整簇裁剪，放不下就丢弃并补空格**，以防「宽字形遗留陈旧单元格、在表格旁留下幽灵字形」（`render/line_utils.rs:102-166`）；`⚠`+U+FE0F 这类变体选择符序列算作单簇。
13. **零宽字符在宽度模型中被直接丢弃**（`views/status_line/mod.rs:25-35`），但 ZWJ（U+200D）不列入不安全集合，因为它需参与 emoji 簇宽度计算（`render/line_utils.rs:34-44`）。
14. **「符号 × 颜色」没有全局表，集中度最高的是 dashboard 的相邻函数对** `state_icon()` / `state_color()`，其注释明写「needs-input / completed / failed 三种状态共用 `◆`，靠前景色消歧」（`views/dashboard/render.rs:3027-3051`）。
15. **legacy 回退会让部分符号退化成同一字形，此时语义完全依赖颜色**——`◈` 与 `◆` 在 ConHost 上都变 `♦`，`━` 与 `─` 都变 `─`，代码注释显式承认这一依赖（`glyphs.rs:128, 220-221`）。
16. **进度条有独立的 1/8 分辨率 fallback 表**：`BLOCKS = ["",▏▎▍▌▋▊▉█]` → `SHADES = ["",░░░▒▒▓▓█]`，原因是 Consolas 缺 U+258F..=U+2589（`views/progress_bar.rs:1-29`，引 microsoft/terminal#387）。
17. **光标形状走终端转义而非字形**，三态策略 `Inherit`/`ForceBlinking`/`ForceSteady` 默认 `Inherit`（`app/mod.rs:1430-1452, 1509-1524`）；模态输入框例外，用 `selection_bar()` 画假光标（`views/settings_modal/render.rs:1667`）。
18. **发现两处 fallback 漏网**：`scrollback/blocks/workflow.rs:66-68` 与 `views/goal_detail.rs:679-681` 硬编码 `✓ ● ○` / `□ ▶` 字面量绕过 `glyphs::`（后者同一 `match` 内另两分支却走了 `glyphs::`）。
