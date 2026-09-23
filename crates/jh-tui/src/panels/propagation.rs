//! 失效传播面板：四类依赖边、传播路径、待重查判断与待复核动作。
//!
//! 设计来源：`docs/tui-design.md` §8——对应 `jh_core::graph` 的 `EdgeKind` / `EdgeOrigin`
//! 与 `jh_core::runtime` 的 `Invalidation` / `PremiseCheck`；节点与动作的名字由适配层渲染成
//! 字符串（本 crate 不认 `NodeRef`）。

use crate::components::panel::{Panel, PanelLine, PanelTitle, Tone};

/// 一条依赖边。
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeRow {
    /// 起点（对象或判断）。
    pub from: String,
    /// 边类型（`read` / `support` / `conflict` / `premise`）。
    pub kind: String,
    /// 终点（判断或行动）。
    pub to: String,
    /// 边来源（`确定性` / `模型提出`）。
    pub origin: String,
}

/// 一条传播路径：起点判断失效后波及到谁。
#[derive(Debug, Clone, PartialEq)]
pub struct PathRow {
    pub target: String,
    /// 已经渲染成一行的一跳一跳（例如 `读取 → 支持 → 前提`）。
    pub hops: String,
}

/// 一个受影响的行动与它的前提复核结果。
#[derive(Debug, Clone, PartialEq)]
pub struct ActionRow {
    pub action: String,
    /// `可发出` / `被挡下：前提 a、b` / `已撤回`。
    pub check: String,
    pub tone: Tone,
}

/// 失效传播面板的数据。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PropagationView {
    /// 触发这次失效的原因（证据追加、判断撤销、问题版本变更……）。
    pub cause: Option<String>,
    /// 待重查的判断节点。
    pub stale: Vec<String>,
    /// 被挡下的行动。
    pub blocked: Vec<String>,
    pub paths: Vec<PathRow>,
    pub edges: Vec<EdgeRow>,
    pub actions: Vec<ActionRow>,
}

impl PropagationView {
    /// 状态行用的一句读数：没有任何失效影响时为 `None`。
    pub fn summary(&self) -> Option<String> {
        (self != &PropagationView::default()).then(|| {
            format!(
                "{} 条待重查 · {} 个动作待复核",
                self.stale.len(),
                self.blocked.len()
            )
        })
    }
}

/// 组装失效传播面板。
pub fn panel(view: &PropagationView, title: PanelTitle) -> Panel {
    if *view == PropagationView::default() {
        return super::empty_panel(title, "失效传播记录");
    }
    let mut lines = Vec::new();
    if let Some(cause) = &view.cause {
        lines.push(PanelLine::Field {
            label: "起因".to_owned(),
            text: cause.clone(),
            tone: Tone::Normal,
        });
    }

    lines.push(PanelLine::Blank);
    lines.push(PanelLine::Section("待重查判断".to_owned()));
    if view.stale.is_empty() {
        lines.push(PanelLine::Text {
            text: "（无）".to_owned(),
            tone: Tone::Dim,
        });
    }
    for node in &view.stale {
        lines.push(PanelLine::Text {
            text: format!("▸ {node}"),
            tone: Tone::Warning,
        });
    }

    lines.push(PanelLine::Blank);
    lines.push(PanelLine::Section("待复核动作".to_owned()));
    if view.blocked.is_empty() {
        lines.push(PanelLine::Text {
            text: "（无）".to_owned(),
            tone: Tone::Dim,
        });
    }
    for action in &view.blocked {
        lines.push(PanelLine::Text {
            text: format!("▸ {action}"),
            tone: Tone::Warning,
        });
    }

    if !view.paths.is_empty() {
        lines.push(PanelLine::Blank);
        lines.push(PanelLine::Section("传播路径".to_owned()));
        for path in &view.paths {
            lines.push(PanelLine::Field {
                label: path.target.clone(),
                text: path.hops.clone(),
                tone: Tone::Muted,
            });
        }
    }

    if !view.edges.is_empty() {
        lines.push(PanelLine::Blank);
        lines.push(PanelLine::Section("依赖边".to_owned()));
        for edge in &view.edges {
            lines.push(PanelLine::Text {
                text: format!(
                    "{} ─[{} · {}]→ {}",
                    edge.from, edge.kind, edge.origin, edge.to
                ),
                tone: Tone::Muted,
            });
        }
    }

    if !view.actions.is_empty() {
        lines.push(PanelLine::Blank);
        lines.push(PanelLine::Section("动作".to_owned()));
        for action in &view.actions {
            lines.push(PanelLine::Field {
                label: action.action.clone(),
                text: action.check.clone(),
                tone: action.tone,
            });
        }
    }

    let footer = format!(
        "{} 条待重查 · {} 个动作待复核",
        view.stale.len(),
        view.blocked.len()
    );
    Panel::with_lines(title, lines).with_footer(footer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::BlockCtx;
    use crate::glyphs::Glyphs;
    use crate::theme::Theme;

    fn sample() -> PropagationView {
        PropagationView {
            cause: Some("追加证据 #3（app.log v3）".to_owned()),
            stale: vec!["cause_primary v2".to_owned()],
            blocked: vec!["apply_fix".to_owned()],
            paths: vec![PathRow {
                target: "cause_primary".to_owned(),
                hops: "读取 → 支持".to_owned(),
            }],
            edges: vec![EdgeRow {
                from: "app.log".to_owned(),
                kind: "read".to_owned(),
                to: "cause_primary".to_owned(),
                origin: "确定性".to_owned(),
            }],
            actions: vec![ActionRow {
                action: "apply_fix".to_owned(),
                check: "被挡下：前提 cause_primary".to_owned(),
                tone: Tone::Warning,
            }],
        }
    }

    fn render(view: &PropagationView, width: usize) -> String {
        let theme = Theme::DARK;
        let ctx = BlockCtx {
            width,
            theme: &theme,
            glyphs: Glyphs::FANCY,
            frame: 0,
            decision: None,
        };
        panel(view, PanelTitle::Single("失效传播".to_owned()))
            .render(&ctx, 40, &crate::components::PanelState::default(), true)
            .iter()
            .map(|line| line.text())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn propagation_shows_cause_stale_blocked_paths_and_edges() {
        let text = render(&sample(), 72);
        assert!(text.contains("追加证据 #3"), "{text}");
        assert!(text.contains("cause_primary v2"), "{text}");
        assert!(text.contains("apply_fix"), "{text}");
        assert!(text.contains("读取 → 支持"), "{text}");
        assert!(text.contains("─[read · 确定性]→"), "{text}");
        assert!(text.contains("1 条待重查 · 1 个动作待复核"), "{text}");
    }

    #[test]
    fn empty_sections_say_none_instead_of_going_blank() {
        let view = PropagationView {
            cause: Some("撤销判断 #2".to_owned()),
            ..PropagationView::default()
        };
        let text = render(&view, 60);
        assert!(text.contains("待重查判断"), "{text}");
        assert!(text.contains("（无）"), "{text}");
        assert!(text.contains("0 条待重查"), "{text}");
    }
}
