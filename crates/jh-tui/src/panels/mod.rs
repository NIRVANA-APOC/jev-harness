//! 研究视图的面板：视图模型 + 组装成 [`Panel`]。
//!
//! 设计来源：`docs/tui-design.md` §5.3、§8——面板数据由宿主（未来的适配层）喂进来，
//! 本 crate 不认识 `jh-core` 的类型；时间戳与哈希都已经格式化好。
//! 本批只做研究目标 §11 的其中三项（判断详情、事件与证据、失效传播），其余留待批次五。

pub mod evidence;
pub mod judgment;
pub mod propagation;

pub use evidence::{EvidenceRow, EvidenceView};
pub use judgment::{Distribution, JudgmentView, Validity};
pub use propagation::{ActionRow, EdgeRow, PathRow, PropagationView};

use crate::components::panel::{Panel, PanelLine, PanelTitle, Tone};
use crate::glyphs::Glyphs;

/// 面板标识：枚举顺序就是 tab 顺序。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PanelId {
    #[default]
    Judgment,
    Evidence,
    Propagation,
}

impl PanelId {
    pub const ALL: &'static [PanelId] =
        &[PanelId::Judgment, PanelId::Evidence, PanelId::Propagation];

    pub fn label(self) -> &'static str {
        match self {
            PanelId::Judgment => "判断详情",
            PanelId::Evidence => "事件与证据",
            PanelId::Propagation => "失效传播",
        }
    }

    pub fn index(self) -> usize {
        PanelId::ALL.iter().position(|id| *id == self).unwrap_or(0)
    }

    pub fn next(self) -> PanelId {
        PanelId::ALL[(self.index() + 1) % PanelId::ALL.len()]
    }

    pub fn prev(self) -> PanelId {
        let len = PanelId::ALL.len();
        PanelId::ALL[(self.index() + len - 1) % len]
    }
}

/// 三个面板的数据：宿主喂入，预览样例给一份离线数据。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Panels {
    /// 判断详情；`None` 表示还没有判断记录。
    pub judgment: Option<JudgmentView>,
    pub evidence: EvidenceView,
    pub propagation: PropagationView,
}

impl Panels {
    /// 组装某个面板：tab 条 + 内容行 + 脚注。
    pub fn panel(&self, id: PanelId, glyphs: Glyphs) -> Panel {
        let title = PanelTitle::Tabs {
            labels: PanelId::ALL
                .iter()
                .map(|id| id.label().to_owned())
                .collect(),
            active: id.index(),
        };
        match id {
            PanelId::Judgment => judgment::panel(&self.judgment, title, glyphs),
            PanelId::Evidence => evidence::panel(&self.evidence, title),
            PanelId::Propagation => propagation::panel(&self.propagation, title),
        }
    }

    /// 状态行用的一句规模读数；没有数据时为 `None`（整段消失，不显示占位值）。
    pub fn summary(&self, id: PanelId) -> Option<String> {
        match id {
            PanelId::Judgment => self
                .judgment
                .as_ref()
                .map(|view| format!("有效性 {}", view.validity.label())),
            PanelId::Evidence => (!self.evidence.rows.is_empty()).then(|| {
                format!(
                    "{} 条证据 · {} 条撤回",
                    self.evidence.rows.len(),
                    self.evidence.retracted_count()
                )
            }),
            PanelId::Propagation => self.propagation.summary(),
        }
    }
}

/// 空态（各面板共用）：先说「没有什么」，再说数据从哪来。
pub(crate) fn empty_panel(title: PanelTitle, what: &str) -> Panel {
    Panel::with_lines(
        title,
        vec![
            PanelLine::Text {
                text: format!("尚无{what}"),
                tone: Tone::Muted,
            },
            PanelLine::Blank,
            PanelLine::Text {
                text: "接线后由适配层把运行时对象翻译成本面板的字段；本批的数据来自离线预览样例。"
                    .to_owned(),
                tone: Tone::Dim,
            },
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_ids_cycle_in_both_directions() {
        assert_eq!(PanelId::Judgment.next(), PanelId::Evidence);
        assert_eq!(PanelId::Evidence.next(), PanelId::Propagation);
        assert_eq!(PanelId::Propagation.next(), PanelId::Judgment);
        assert_eq!(PanelId::Judgment.prev(), PanelId::Propagation);
        assert_eq!(PanelId::ALL.len(), 3);
        assert_eq!(PanelId::Propagation.index(), 2);
    }

    #[test]
    fn empty_panels_say_so_instead_of_going_blank() {
        let panels = Panels::default();
        for id in PanelId::ALL {
            let panel = panels.panel(*id, Glyphs::FANCY);
            assert!(
                panel.lines.iter().any(
                    |line| matches!(line, PanelLine::Text { text, .. } if text.contains("尚无"))
                ),
                "{id:?} 的空态没有说明"
            );
            assert!(matches!(panel.title, PanelTitle::Tabs { .. }));
            assert_eq!(panels.summary(*id), None, "{id:?} 无数据时状态行不该有读数");
        }
    }
}
