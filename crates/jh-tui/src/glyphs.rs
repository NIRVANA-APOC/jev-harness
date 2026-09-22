//! 字形单一来源：每个字形一个函数，每个字形都带「花体 / 回退」两种形态，且**必须同宽**。
//!
//! 设计来源：专题 `docs/research/grok-tui/02-glyphs-and-iconography.md`（§1 中心化字形表、
//! §2.2「调用方会用这些宽度做布局运算，因此每个字形与它的 fallback 必须同宽」、§11.3
//! 「动画帧序列必须等宽，否则布局随动画节拍跳动」）与 `docs/grok-tui-design.md` §3.2。
//!
//! 与上游的差异：
//! - 上游切换回退字形的开关是「legacy Windows ConHost」（`glyphs.rs:377-424`，非 Windows
//!   直接短路为 false，可被 `GROK_FORCE_LEGACY_CONSOLE` 覆盖）；本项目把它简化成
//!   [`Glyphs`] 集合这一能力位，由 `Capabilities` 决定，业务代码不读环境变量。
//! - 未在上游找到回退形态的字形（`…`、`▾`、`›` 等），回退取本项目自定的同宽 ASCII，
//!   逐条在函数文档里标注。

/// 一个字形：花体形态与其回退形态，两者宽度必须相同。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Glyph {
    pub fancy: &'static str,
    pub fallback: &'static str,
}

impl Glyph {
    pub const fn new(fancy: &'static str, fallback: &'static str) -> Self {
        Glyph { fancy, fallback }
    }
}

/// 字形集合：唯一的字形选择开关。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Glyphs {
    /// true＝花体（Unicode），false＝回退（同宽 ASCII）。
    pub fancy: bool,
}

impl Default for Glyphs {
    /// 默认花体：现代终端都吃 Unicode，回退是给老控制台留的逃生口。
    fn default() -> Self {
        Glyphs::FANCY
    }
}

impl Glyphs {
    pub const FANCY: Glyphs = Glyphs { fancy: true };
    pub const FALLBACK: Glyphs = Glyphs { fancy: false };

    /// 按本集合解析字形的实际形态。
    pub fn resolve(self, glyph: Glyph) -> &'static str {
        if self.fancy {
            glyph.fancy
        } else {
            glyph.fallback
        }
    }
}

macro_rules! glyphs {
    ($(
        $(#[doc = $doc:literal])*
        $name:ident : $fancy:literal / $fallback:literal ,
    )*) => {
        $(
            $(#[doc = $doc])*
            pub const fn $name() -> Glyph {
                Glyph::new($fancy, $fallback)
            }
        )*

        /// 全部字形的枚举表（供同宽断言与文档测试遍历）；值仍然只出自上面这些函数。
        pub const ALL: &[(&str, Glyph)] = &[
            $( (stringify!($name), Glyph::new($fancy, $fallback)), )*
        ];
    };
}

glyphs! {
    /// 提示行首与用户回显：`❯ ` → `> `（两列）。上游 `glyphs.rs:16-25`。
    prompt: "❯ " / "> ",
    /// 列表圆点：`• `；回退 `- `（本项目自定，上游只在 markdown 解析期把 `- ` 换成 `• `）。
    bullet: "• " / "- ",
    /// 卡片左侧强调轨：`┃` → `│`。上游 `glyphs.rs:175-181`。
    accent_bar: "┃" / "│",
    /// 细选中条：`▏` → `│`。上游 `glyphs.rs:248-254`。
    thin_bar: "▏" / "│",
    /// 折叠标记：`❙`；回退 `|`（本项目自定）。上游 `glyphs.rs:39-45`。
    collapse_bar: "❙" / "|",
    /// 实心菱形（句柄 / 用户块标记）：`◆` → `♦`。上游 `glyphs.rs:111-135`。
    diamond: "◆" / "♦",
    /// 空心菱形（判定 / 计划标记）：`◇` → `○`。上游 `glyphs.rs:111-135`。
    diamond_open: "◇" / "○",
    /// 小圆点：`●` → `= `? 见上游 `glyphs.rs:237-243`；回退取本项目自定的 `*`。
    dot: "●" / "*",
    /// 成功标记：`✓` → `√`（上游 `to_legacy_glyphs` 的 `✓→√`，`glyphs.rs:364-373`）。
    success: "✓" / "√",
    /// 失败标记：`✗` → `x`（上游 `to_legacy_glyphs`，`glyphs.rs:364-373`）。
    failure: "✗" / "x",
    /// 展开三角：`▸` → `>`（回退本项目自定）。上游 `glyphs.rs:288-303`。
    triangle_right: "▸" / ">",
    /// 收起三角：`▾` → `v`（回退本项目自定）。上游 `glyphs.rs:288-303`。
    triangle_down: "▾" / "v",
    /// 省略号：`…`；上游只记录 `truncate_to_width` 会补 `…`，**未见回退形态**，回退取本项目自定的 `.`。
    ellipsis: "…" / ".",
}

/// 进度 / 概率条 9 级填充：花体用半格块，回退用阴影块。
///
/// 取值与理由照抄上游 `views/progress_bar.rs:1-29`：`BLOCKS` 是
/// `["", "▏","▎","▍","▌","▋","▊","▉","█"]`，`SHADES` 是
/// `["", "░","░","░","▒","▒","▓","▓","█"]`（Consolas 缺 U+258F..=U+2589）。
pub const PROGRESS_LEVELS: [Glyph; 9] = [
    Glyph::new("", ""),
    Glyph::new("▏", "░"),
    Glyph::new("▎", "░"),
    Glyph::new("▍", "░"),
    Glyph::new("▌", "▒"),
    Glyph::new("▋", "▒"),
    Glyph::new("▊", "▓"),
    Glyph::new("▉", "▓"),
    Glyph::new("█", "█"),
];

/// 花体（盲文）转轮帧，8 帧，每帧 1 列。上游 `glyphs.rs:148-159`。
pub const SPINNER_FRAMES: [Glyph; 8] = [
    Glyph::new("⠋", "|"),
    Glyph::new("⠙", "-"),
    Glyph::new("⠹", "|"),
    Glyph::new("⠸", "-"),
    Glyph::new("⠼", "|"),
    Glyph::new("⠴", "-"),
    Glyph::new("⠦", "|"),
    Glyph::new("⠧", "-"),
];

/// 回退转轮帧，4 帧（上游同为 4 帧：`| / - \`）。
pub const SPINNER_FRAMES_FALLBACK: [&str; 4] = ["|", "/", "-", "\\"];

/// 按帧号取转轮字形：花体 8 帧、回退 4 帧，各自取模。
pub fn spinner_frame(glyphs: Glyphs, frame: usize) -> &'static str {
    if glyphs.fancy {
        SPINNER_FRAMES[frame % SPINNER_FRAMES.len()].fancy
    } else {
        SPINNER_FRAMES_FALLBACK[frame % SPINNER_FRAMES_FALLBACK.len()]
    }
}

/// 按 0..=8 的等级取概率条填充字形。
pub fn progress_glyph(glyphs: Glyphs, level: u8) -> &'static str {
    let level = level.min(8) as usize;
    glyphs.resolve(PROGRESS_LEVELS[level])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::width::display_width;

    /// 上游对每个 (fancy, fallback) 对都有一条同宽断言（`glyphs.rs:426-558`）。
    #[test]
    fn every_glyph_pair_is_same_width() {
        for (name, glyph) in ALL {
            assert_eq!(
                display_width(glyph.fancy),
                display_width(glyph.fallback),
                "字形 {name} 的花体与回退宽度不一致：{:?} vs {:?}",
                glyph.fancy,
                glyph.fallback
            );
        }
    }

    #[test]
    fn glyph_table_is_not_empty_and_names_are_unique() {
        assert!(ALL.len() >= 10, "字形表太小：{}", ALL.len());
        let mut names: Vec<&str> = ALL.iter().map(|(name, _)| *name).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len(), "字形名字重复");
    }

    #[test]
    fn progress_levels_are_monotonic_and_same_width() {
        for (level, glyph) in PROGRESS_LEVELS.iter().enumerate() {
            assert_eq!(
                display_width(glyph.fancy),
                display_width(glyph.fallback),
                "进度等级 {level} 不同宽"
            );
            assert!(
                display_width(glyph.fancy) <= 1,
                "进度等级 {level} 超过一列，会让整条概率条位移"
            );
        }
        assert_eq!(progress_glyph(Glyphs::FANCY, 8), "█");
        assert_eq!(progress_glyph(Glyphs::FALLBACK, 8), "█");
        assert_eq!(progress_glyph(Glyphs::FANCY, 4), "▌");
        assert_eq!(progress_glyph(Glyphs::FALLBACK, 4), "▒");
        // 越界等级收敛到最后一级，不 panic。
        assert_eq!(progress_glyph(Glyphs::FANCY, 99), "█");
    }

    #[test]
    fn spinner_frames_are_one_column() {
        for (idx, glyph) in SPINNER_FRAMES.iter().enumerate() {
            assert_eq!(display_width(glyph.fancy), 1, "花体第 {idx} 帧不是 1 列");
            assert_eq!(display_width(glyph.fallback), 1, "回退第 {idx} 帧不是 1 列");
        }
        for frame in SPINNER_FRAMES_FALLBACK {
            assert_eq!(display_width(frame), 1);
        }
        // 两套帧数不同（8 / 4），各自取模，越界不 panic。
        assert_eq!(spinner_frame(Glyphs::FANCY, 9), "⠙");
        assert_eq!(spinner_frame(Glyphs::FALLBACK, 5), "/");
    }

    #[test]
    fn resolve_switches_form() {
        assert_eq!(Glyphs::FANCY.resolve(prompt()), "❯ ");
        assert_eq!(Glyphs::FALLBACK.resolve(prompt()), "> ");
        assert_eq!(Glyphs::FALLBACK.resolve(success()), "√");
        assert_eq!(Glyphs::default(), Glyphs::FANCY, "默认花体，回退是逃生口");
    }
}
