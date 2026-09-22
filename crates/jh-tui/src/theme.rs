//! 主题与色彩能力：扁平 `Theme` 结构体 + 一套写死的深色配色 + 色深降级。
//!
//! 设计来源：`docs/grok-tui-design.md` §3.1、§7.1 第 7 条（能力探测成枚举，业务代码只读
//! 能力位）与专题 `docs/research/grok-tui/01-theme-color-system.md`（§1 主题结构、
//! §5.1 GrokNight 深色取值、§6 色深降级与量化表）。
//!
//! 与上游的差异（本项目量级所限）：
//! - 上游 `Theme` 是 58 个 `Color` + 6 个 `Modifier` 字段、7 种主题；这里只保留一套
//!   写死的深色配色与 18 个字段，不做主题热切换（§7.2）。
//! - 上游主路径用 `supports-color` 做探测（再加品牌白名单）；这里不引入该依赖，改用上游
//!   `grok doctor` 路径的环境变量口径（`NO_COLOR` → `COLORTERM` → `TERM`）。
//! - 色值直接取自 GrokNight 深色主题；上游的 `windows_contrast_boost` 与
//!   `ansi16_chrome_overrides` 两套低色重配色不抄。

use ratatui::style::{Color, Modifier, Style};

/// 色彩能力，全序：`None < Basic < Ansi256 < TrueColor`。
///
/// 对应上游 `ColorLevel`（`theme/color_support.rs:17-30`，含 `has_color` / `has_256` /
/// `has_truecolor`）。上游没有 `Auto` 变体——`Auto` 只属于主题选择与备用屏模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ColorLevel {
    /// 不上色：所有颜色塌成 `Color::Reset`，弱化改用 `Modifier::DIM`。
    None,
    /// 16 色：终端调色板由用户决定。
    Basic,
    /// 256 色索引。
    Ansi256,
    /// 24 位真彩。
    TrueColor,
}

impl ColorLevel {
    /// 是否会上色（`None` 之外都算）。
    pub fn has_color(self) -> bool {
        self != ColorLevel::None
    }

    /// 是否有 256 色可用。
    pub fn has_256(self) -> bool {
        self >= ColorLevel::Ansi256
    }

    /// 是否有真彩可用。
    pub fn has_truecolor(self) -> bool {
        self >= ColorLevel::TrueColor
    }

    /// 把任意颜色量化到本能力档位。量化表见专题 01 §6.2：
    ///
    /// | 档位 | `Rgb` | `Indexed` | `Reset` / 具名色 |
    /// | --- | --- | --- | --- |
    /// | TrueColor | 原样 | 原样 | 原样 |
    /// | Ansi256 | → 最近索引 | 原样 | 原样 |
    /// | Basic | → 最近索引 → 最近 16 色 | → 最近 16 色 | 原样 |
    /// | None | → `Reset` | → `Reset` | → `Reset` |
    pub fn quantize(self, color: Color) -> Color {
        match (self, color) {
            (ColorLevel::TrueColor, color) => color,
            (ColorLevel::None, _) => Color::Reset,
            (ColorLevel::Ansi256, Color::Rgb(r, g, b)) => Color::Indexed(nearest_indexed(r, g, b)),
            (ColorLevel::Ansi256, color) => color,
            (ColorLevel::Basic, Color::Rgb(r, g, b)) => indexed_to_ansi16(nearest_indexed(r, g, b)),
            (ColorLevel::Basic, Color::Indexed(idx)) => indexed_to_ansi16(idx),
            (ColorLevel::Basic, color) => color,
        }
    }

    /// 环境探测。口径取自上游 `standalone_from_env`（`color_support.rs:172-203`）：
    /// `NO_COLOR` 存在即认输（显式 opt-out 优先）；随后 `COLORTERM` 认真彩、
    /// `TERM` 含 `256color` 认 256 色、`TERM` 为空或 `dumb` 视为无色彩证据。
    pub fn detect() -> Self {
        Self::from_env_lookup(|key| std::env::var(key).ok())
    }

    /// 同上，但环境变量读取由调用方注入（便于单测，不必改进程环境）。
    pub fn from_env_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        if lookup("NO_COLOR").is_some_and(|v| !v.is_empty()) {
            return ColorLevel::None;
        }
        if let Some(colorterm) = lookup("COLORTERM")
            && (colorterm.contains("truecolor") || colorterm.contains("24bit"))
        {
            return ColorLevel::TrueColor;
        }
        match lookup("TERM").as_deref() {
            Some(term) if term.contains("256color") => ColorLevel::Ansi256,
            Some("") | Some("dumb") | None => ColorLevel::None,
            Some(_) => ColorLevel::Basic,
        }
    }
}

/// 最近 256 色索引：在 6×6×6 色立方与 24 级灰阶里取欧氏距离最小者
/// （上游 `render/color.rs:56-91` 的口径，这里不查前 16 色）。
fn nearest_indexed(r: u8, g: u8, b: u8) -> u8 {
    let mut best = 0u8;
    let mut best_distance = u32::MAX;
    for idx in 16..=255u8 {
        let (cr, cg, cb) = indexed_to_rgb(idx);
        let distance = squared_distance(r, g, b, cr, cg, cb);
        if distance < best_distance {
            best_distance = distance;
            best = idx;
        }
    }
    best
}

/// 256 色索引 → RGB：16..=231 是 6×6×6 色立方，232..=255 是 24 级灰阶。
fn indexed_to_rgb(idx: u8) -> (u8, u8, u8) {
    match idx {
        0..=15 => ansi16_rgb(idx),
        16..=231 => {
            let i = idx - 16;
            let steps = [0u8, 95, 135, 175, 215, 255];
            (
                steps[(i / 36) as usize],
                steps[((i % 36) / 6) as usize],
                steps[(i % 6) as usize],
            )
        }
        _ => {
            let level = 8 + (idx - 232) * 10;
            (level, level, level)
        }
    }
}

/// 16 个 ANSI 具名色的参考 RGB，用于 `Basic` 档位做平方欧氏距离匹配
/// （上游把 0–15 直接映射，16–255 走 RGB 再取最近 16 色，见 `color_support.rs:274-336`）。
fn ansi16_rgb(idx: u8) -> (u8, u8, u8) {
    const TABLE: [(u8, u8, u8); 16] = [
        (0, 0, 0),
        (128, 0, 0),
        (0, 128, 0),
        (128, 128, 0),
        (0, 0, 128),
        (128, 0, 128),
        (0, 128, 128),
        (192, 192, 192),
        (128, 128, 128),
        (255, 0, 0),
        (0, 255, 0),
        (255, 255, 0),
        (0, 0, 255),
        (255, 0, 255),
        (0, 255, 255),
        (255, 255, 255),
    ];
    TABLE[idx as usize]
}

/// 索引色 → 16 色具名色：0–15 直连，16–255 先还原 RGB 再取平方欧氏距离最近者。
fn indexed_to_ansi16(idx: u8) -> Color {
    const NAMES: [Color; 16] = [
        Color::Black,
        Color::Red,
        Color::Green,
        Color::Yellow,
        Color::Blue,
        Color::Magenta,
        Color::Cyan,
        Color::Gray,
        Color::DarkGray,
        Color::LightRed,
        Color::LightGreen,
        Color::LightYellow,
        Color::LightBlue,
        Color::LightMagenta,
        Color::LightCyan,
        Color::White,
    ];
    if idx < 16 {
        return NAMES[idx as usize];
    }
    let (r, g, b) = indexed_to_rgb(idx);
    let mut best = 0usize;
    let mut best_distance = u32::MAX;
    for (candidate, (cr, cg, cb)) in ansi16_rgb16().iter().enumerate() {
        let distance = squared_distance(r, g, b, *cr, *cg, *cb);
        if distance < best_distance {
            best_distance = distance;
            best = candidate;
        }
    }
    NAMES[best]
}

fn ansi16_rgb16() -> [(u8, u8, u8); 16] {
    let mut table = [(0u8, 0u8, 0u8); 16];
    for (idx, slot) in table.iter_mut().enumerate() {
        *slot = ansi16_rgb(idx as u8);
    }
    table
}

fn squared_distance(r1: u8, g1: u8, b1: u8, r2: u8, g2: u8, b2: u8) -> u32 {
    let dr = i32::from(r1) - i32::from(r2);
    let dg = i32::from(g1) - i32::from(g2);
    let db = i32::from(b1) - i32::from(b2);
    (dr * dr + dg * dg + db * db) as u32
}

/// 扁平主题：字段名即角色，没有独立的 token 表（上游 `theme/tokyonight.rs:46-144` 的同一形状，量级已砍）。
///
/// 色值全部取自上游 GrokNight 深色主题（专题 01 §5.1 列出的 26 个常量）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    /// 本主题按哪一档色深量化过；`None` 档下所有颜色都是 `Reset`。
    pub level: ColorLevel,

    pub bg_base: Color,
    pub bg_light: Color,
    pub bg_dark: Color,

    pub text_primary: Color,
    pub text_secondary: Color,
    pub gray: Color,
    pub gray_dim: Color,

    pub accent_user: Color,
    pub accent_assistant: Color,
    pub accent_tool: Color,
    pub accent_system: Color,
    pub accent_plan: Color,
    pub accent_success: Color,
    pub accent_error: Color,
    pub accent_warning: Color,

    pub prompt_border: Color,
    pub prompt_border_active: Color,
    pub selection_border: Color,
}

/// 取色值的 const 助手，形状与上游 `theme/groknight.rs:10-12` 的 `rgb()` 一致。
const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb(r, g, b)
}

impl Theme {
    /// 写死的一套深色配色（GrokNight 取值）。
    pub const DARK: Theme = Theme {
        level: ColorLevel::TrueColor,
        bg_base: rgb(20, 20, 20),              // #141414
        bg_light: rgb(36, 36, 36),             // #242424
        bg_dark: rgb(28, 28, 28),              // #1c1c1c
        text_primary: rgb(225, 225, 225),      // #e1e1e1
        text_secondary: rgb(200, 200, 200),    // #c8c8c8
        gray: rgb(108, 108, 108),              // #6c6c6c
        gray_dim: rgb(88, 88, 88),             // #585858
        accent_user: rgb(200, 200, 200),       // #c8c8c8
        accent_assistant: rgb(187, 154, 247),  // #bb9af7
        accent_tool: rgb(120, 120, 120),       // #787878
        accent_system: rgb(122, 162, 247),     // #7aa2f7
        accent_plan: rgb(255, 219, 141),       // #FFDB8D
        accent_success: rgb(158, 206, 106),    // #9ece6a
        accent_error: rgb(247, 118, 142),      // #f7768e
        accent_warning: rgb(224, 175, 104),    // #e0af68
        prompt_border: rgb(50, 50, 55),        // #323237
        prompt_border_active: rgb(80, 80, 88), // #505058
        selection_border: rgb(60, 60, 65),     // #3c3c41
    };

    /// 量化到指定色深（逐个字段走 [`ColorLevel::quantize`]）。
    pub fn quantized(mut self, level: ColorLevel) -> Theme {
        let colors = [
            &mut self.bg_base,
            &mut self.bg_light,
            &mut self.bg_dark,
            &mut self.text_primary,
            &mut self.text_secondary,
            &mut self.gray,
            &mut self.gray_dim,
            &mut self.accent_user,
            &mut self.accent_assistant,
            &mut self.accent_tool,
            &mut self.accent_system,
            &mut self.accent_plan,
            &mut self.accent_success,
            &mut self.accent_error,
            &mut self.accent_warning,
            &mut self.prompt_border,
            &mut self.prompt_border_active,
            &mut self.selection_border,
        ];
        for slot in colors {
            *slot = level.quantize(*slot);
        }
        self.level = level;
        self
    }

    /// 正文样式。`None` 档下不上色，回到终端默认前景。
    pub fn fg(&self) -> Style {
        if self.level.has_color() {
            Style::new().fg(self.text_primary)
        } else {
            Style::new()
        }
    }

    /// 次级文本（比正文弱一档）。
    pub fn muted(&self) -> Style {
        if self.level.has_color() {
            Style::new().fg(self.gray)
        } else {
            Style::new().add_modifier(Modifier::DIM)
        }
    }

    /// 最弱一档：提示、参数、次要元数据。
    pub fn dim(&self) -> Style {
        if self.level.has_color() {
            Style::new().fg(self.gray_dim)
        } else {
            Style::new().add_modifier(Modifier::DIM)
        }
    }

    /// 强调文本（粗体 + 指定颜色）。
    pub fn accent(&self, color: Color) -> Style {
        let style = Style::new().add_modifier(Modifier::BOLD);
        if self.level.has_color() {
            style.fg(color)
        } else {
            style
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_level_is_ordered() {
        assert!(ColorLevel::None < ColorLevel::Basic);
        assert!(ColorLevel::Basic < ColorLevel::Ansi256);
        assert!(ColorLevel::Ansi256 < ColorLevel::TrueColor);
        assert!(!ColorLevel::None.has_color());
        assert!(ColorLevel::Basic.has_color());
        assert!(ColorLevel::Ansi256.has_256());
        assert!(!ColorLevel::Basic.has_256());
        assert!(ColorLevel::TrueColor.has_truecolor());
    }

    #[test]
    fn quantize_table_matches_spec() {
        let rgb_color = Color::Rgb(122, 162, 247);
        assert_eq!(ColorLevel::TrueColor.quantize(rgb_color), rgb_color);
        assert!(matches!(
            ColorLevel::Ansi256.quantize(rgb_color),
            Color::Indexed(_)
        ));
        assert_eq!(
            ColorLevel::Ansi256.quantize(Color::Indexed(7)),
            Color::Indexed(7)
        );
        // Basic 档位只能落到 16 具名色之一（具体是哪一个由平方欧氏距离决定）。
        assert!(!matches!(
            ColorLevel::Basic.quantize(rgb_color),
            Color::Rgb(..) | Color::Indexed(_) | Color::Reset
        ));
        assert_eq!(
            ColorLevel::Basic.quantize(Color::Rgb(255, 255, 255)),
            Color::White
        );
        assert_eq!(
            ColorLevel::Basic.quantize(Color::Rgb(0, 0, 0)),
            Color::Black
        );
        assert_eq!(ColorLevel::None.quantize(rgb_color), Color::Reset);
        assert_eq!(ColorLevel::None.quantize(Color::Indexed(200)), Color::Reset);
    }

    #[test]
    fn nearest_indexed_picks_expected_cube_and_gray_slots() {
        // #7aa2f7 落在色立方里，量化后必须离原色很近。
        let idx = nearest_indexed(122, 162, 247);
        assert!((16..=231).contains(&idx), "expected cube slot, got {idx}");
        // 纯灰落进 24 级灰阶。
        let gray = nearest_indexed(128, 128, 128);
        assert!(
            (232..=255).contains(&gray),
            "expected gray slot, got {gray}"
        );
        assert_eq!(nearest_indexed(0, 0, 0), 16);
    }

    #[test]
    fn indexed_to_ansi16_is_direct_below_16() {
        assert_eq!(indexed_to_ansi16(0), Color::Black);
        assert_eq!(indexed_to_ansi16(9), Color::LightRed);
        // 16 = #000000（色立方原点）→ 最近 16 色是黑。
        assert_eq!(indexed_to_ansi16(16), Color::Black);
    }

    #[test]
    fn no_color_wins_over_everything() {
        let level = ColorLevel::from_env_lookup(|key| match key {
            "NO_COLOR" => Some("1".to_owned()),
            "COLORTERM" => Some("truecolor".to_owned()),
            _ => None,
        });
        assert_eq!(level, ColorLevel::None);
    }

    #[test]
    fn env_detection_order() {
        let truecolor = ColorLevel::from_env_lookup(|key| match key {
            "COLORTERM" => Some("truecolor".to_owned()),
            _ => None,
        });
        assert_eq!(truecolor, ColorLevel::TrueColor);

        let ansi256 = ColorLevel::from_env_lookup(|key| match key {
            "TERM" => Some("xterm-256color".to_owned()),
            _ => None,
        });
        assert_eq!(ansi256, ColorLevel::Ansi256);

        let dumb = ColorLevel::from_env_lookup(|key| match key {
            "TERM" => Some("dumb".to_owned()),
            _ => None,
        });
        assert_eq!(dumb, ColorLevel::None);

        let basic = ColorLevel::from_env_lookup(|key| match key {
            "TERM" => Some("xterm".to_owned()),
            _ => None,
        });
        assert_eq!(basic, ColorLevel::Basic);

        let missing = ColorLevel::from_env_lookup(|_| None);
        assert_eq!(missing, ColorLevel::None);
    }

    #[test]
    fn dark_theme_degrades_to_reset_without_color() {
        let plain = Theme::DARK.quantized(ColorLevel::None);
        assert_eq!(plain.accent_assistant, Color::Reset);
        assert_eq!(plain.bg_base, Color::Reset);
        assert_eq!(plain.fg(), Style::new());
        // 无色档下弱化靠 DIM 而不是画深灰（否则 Reset 会被重新上色）。
        assert_eq!(plain.muted(), Style::new().add_modifier(Modifier::DIM));

        let ansi = Theme::DARK.quantized(ColorLevel::Ansi256);
        assert!(matches!(ansi.accent_assistant, Color::Indexed(_)));

        let basic = Theme::DARK.quantized(ColorLevel::Basic);
        assert!(!matches!(basic.accent_assistant, Color::Rgb(_, _, _)));

        let fancy = Theme::DARK.quantized(ColorLevel::TrueColor);
        assert_eq!(fancy.accent_user, Theme::DARK.accent_user);
    }
}
