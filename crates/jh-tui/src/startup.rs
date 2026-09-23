//! 首屏面板：字标 + 版本 + 概览条目 + 菜单。
//!
//! 设计来源：`docs/tui-design.md` §5.2 与参考截图 `startup.png`（居中面板：左侧图形、
//! 右侧标题与条目、下面一份带键位的菜单）。字标是**占位**图形（设计文档 §10 未确定项 4），
//! 花体与回退各一份且同宽。

use ratatui::text::{Line, Span};

use crate::blocks::{BlockCtx, BlockLine, Selectable};
use crate::components::panel::{Panel, PanelLine, PanelTitle, Tone};
use crate::glyphs::Glyphs;

/// 花体字标（3 行 × 3 列）。
const MARK_FANCY: [&str; 3] = ["╭─╮", "│◇│", "╰─╯"];
/// 回退字标：同宽同高，纯 ASCII。
const MARK_FALLBACK: [&str; 3] = ["+-+", "|o|", "+-+"];

/// 首屏面板要显示的三条事实。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartupFacts<'a> {
    pub version: &'a str,
    /// 当前判定器（如 `rule`）与接线状态。
    pub judge: &'a str,
    /// 工作区路径（`~` 缩写）。
    pub workspace: &'a str,
}

impl StartupFacts<'_> {
    /// 小屏退化时的一行式概览。
    pub fn summary(&self) -> String {
        format!(
            "{} · 判定器 {} · {}",
            self.version, self.judge, self.workspace
        )
    }
}

/// 菜单项：标签与键位（键位文案与动作注册表的显示口径一致）。
pub const MENU: [(&str, &str); 3] = [
    ("新建会话", "Ctrl+N"),
    ("研究视图", "Ctrl+O"),
    ("退出", "Ctrl+C"),
];

/// 组装首屏面板。
pub fn panel(facts: &StartupFacts, glyphs: Glyphs) -> Panel {
    let mark = if glyphs.fancy {
        MARK_FANCY
    } else {
        MARK_FALLBACK
    };
    let mut lines = vec![
        PanelLine::Split {
            left: mark[0].to_owned(),
            left_tone: Tone::Plan,
            right: format!("jev-harness {}", facts.version),
            right_tone: Tone::Normal,
        },
        PanelLine::Split {
            left: mark[1].to_owned(),
            left_tone: Tone::Plan,
            right: format!("判定器  {}", facts.judge),
            right_tone: Tone::Muted,
        },
        PanelLine::Split {
            left: mark[2].to_owned(),
            left_tone: Tone::Plan,
            right: format!("工作区  {}", facts.workspace),
            right_tone: Tone::Dim,
        },
        PanelLine::Blank,
    ];
    for (label, key) in MENU {
        lines.push(PanelLine::Menu {
            label: label.to_owned(),
            key: key.to_owned(),
        });
    }
    Panel::with_lines(
        PanelTitle::Single(format!("jev-harness {}", facts.version)),
        lines,
    )
}

/// 小屏退化：不画面板与字标，把概览与菜单平铺成几行。
pub fn plain(facts: &StartupFacts, ctx: &BlockCtx) -> Vec<BlockLine> {
    let mut lines = vec![BlockLine::new(
        Line::from(Span::styled(
            format!("jev-harness {}", facts.version),
            ctx.theme.fg(),
        )),
        Selectable::All,
    )];
    lines.push(BlockLine::new(
        Line::from(Span::styled(
            format!("判定器 {} · 工作区 {}", facts.judge, facts.workspace),
            ctx.theme.muted(),
        )),
        Selectable::All,
    ));
    let menu = MENU
        .iter()
        .map(|(label, key)| format!("{label} {key}"))
        .collect::<Vec<_>>()
        .join(" · ");
    lines.push(BlockLine::new(
        Line::from(Span::styled(menu, ctx.theme.dim())),
        Selectable::All,
    ));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::PanelState;
    use crate::theme::Theme;

    fn facts() -> StartupFacts<'static> {
        StartupFacts {
            version: "0.1.0",
            judge: "rule · 未接线",
            workspace: "~/Documents/jev-harness",
        }
    }

    fn ctx<'a>(theme: &'a Theme, width: usize, glyphs: Glyphs) -> BlockCtx<'a> {
        BlockCtx {
            width,
            theme,
            glyphs,
            frame: 0,
            decision: None,
        }
    }

    #[test]
    fn panel_shows_version_judge_workspace_and_menu() {
        let theme = Theme::DARK;
        let ctx = ctx(&theme, 52, Glyphs::FANCY);
        let panel = panel(&facts(), Glyphs::FANCY);
        let text: String = panel
            .render(&ctx, 12, &PanelState::default(), true)
            .iter()
            .map(|line| line.text())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("jev-harness 0.1.0"), "{text}");
        assert!(text.contains("判定器"), "{text}");
        assert!(text.contains("工作区"), "{text}");
        assert!(text.contains("新建会话"), "{text}");
        assert!(text.contains("Ctrl+O"), "{text}");
        assert!(text.contains('◇'), "花体字标：{text}");
    }

    #[test]
    fn fallback_mark_is_ascii_and_same_width() {
        let theme = Theme::DARK;
        let ctx = ctx(&theme, 52, Glyphs::FALLBACK);
        let panel = panel(&facts(), Glyphs::FALLBACK);
        let text: String = panel
            .render(&ctx, 12, &PanelState::default(), true)
            .iter()
            .map(|line| line.text())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("|o|"), "{text}");
        assert!(!text.contains('◇'), "{text}");
        for (fancy, fallback) in MARK_FANCY.iter().zip(MARK_FALLBACK.iter()) {
            assert_eq!(
                crate::width::display_width(fancy),
                crate::width::display_width(fallback)
            );
        }
    }

    #[test]
    fn plain_form_list_keeps_the_menu_keys() {
        let theme = Theme::DARK;
        let ctx = ctx(&theme, 60, Glyphs::FANCY);
        let lines = plain(&facts(), &ctx);
        let text: String = lines
            .iter()
            .map(BlockLine::text)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("jev-harness 0.1.0"), "{text}");
        assert!(text.contains("研究视图 Ctrl+O"), "{text}");
    }
}
