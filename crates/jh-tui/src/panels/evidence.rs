//! 事件与证据面板：追加写的证据序列（来源、对象与版本、摘要哈希、撤回标记）。
//!
//! 设计来源：`docs/tui-design.md` §8——时间戳、哈希前缀与内容摘要都由适配层格式化好，
//! 本 crate 只负责排版。撤回在 `jh-core` 里是「另追加一条标记」，所以这里是行上的标记，
//! 不是把历史行删掉。

use crate::components::panel::{Panel, PanelLine, PanelTitle, Tone};

/// 一条证据的展示行。
#[derive(Debug, Clone, PartialEq)]
pub struct EvidenceRow {
    pub id: u64,
    /// 已格式化的时间（适配层负责时区与日历）。
    pub at: String,
    /// 来源（文件名、工具名或通道名）。
    pub source: String,
    pub object: String,
    pub version: u64,
    /// 内容摘要哈希（适配层已截断到可读长度）。
    pub hash: String,
    /// 内容摘要（适配层已压缩成一行）。
    pub summary: String,
    /// 是否已被撤回（撤回是另记一笔，不改写历史）。
    pub retracted: bool,
}

/// 证据面板的数据。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EvidenceView {
    pub rows: Vec<EvidenceRow>,
}

impl EvidenceView {
    pub fn retracted_count(&self) -> usize {
        self.rows.iter().filter(|row| row.retracted).count()
    }
}

/// 组装事件与证据面板。
pub fn panel(view: &EvidenceView, title: PanelTitle) -> Panel {
    if view.rows.is_empty() {
        return super::empty_panel(title, "证据记录");
    }
    let mut lines = Vec::new();
    for row in &view.rows {
        let head = if row.retracted { "撤回 · " } else { "" };
        lines.push(PanelLine::Field {
            label: format!("#{}", row.id),
            text: format!("{head}{} · {} v{}", row.at, row.object, row.version),
            tone: if row.retracted {
                Tone::Warning
            } else {
                Tone::Normal
            },
        });
        // 空标签 + 值列：明细行与上一行的值对齐（标签列宽度由面板统一算）。
        lines.push(PanelLine::Field {
            label: String::new(),
            text: format!("{} · {} · {}", row.hash, row.source, row.summary),
            tone: Tone::Dim,
        });
    }
    let footer = format!(
        "{} 条证据 · {} 条撤回",
        view.rows.len(),
        view.retracted_count()
    );
    Panel::with_lines(title, lines).with_footer(footer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::BlockCtx;
    use crate::glyphs::Glyphs;
    use crate::theme::Theme;

    fn row(id: u64, retracted: bool) -> EvidenceRow {
        EvidenceRow {
            id,
            at: "14:22:01".to_owned(),
            source: "app.log".to_owned(),
            object: "app.log".to_owned(),
            version: 3,
            hash: "3f9c1a".to_owned(),
            summary: "启动时端口 8080 被占用".to_owned(),
            retracted,
        }
    }

    #[test]
    fn evidence_rows_show_time_object_hash_and_source() {
        let theme = Theme::DARK;
        let ctx = BlockCtx {
            width: 72,
            theme: &theme,
            glyphs: Glyphs::FANCY,
            frame: 0,
            decision: None,
        };
        let view = EvidenceView {
            rows: vec![row(1, false), row(2, true)],
        };
        let panel = panel(&view, PanelTitle::Single("事件与证据".to_owned()));
        let text: String = panel
            .render(&ctx, 20, &crate::components::PanelState::default(), true)
            .iter()
            .map(|line| line.text())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("#1"), "{text}");
        assert!(text.contains("14:22:01"), "{text}");
        assert!(text.contains("app.log v3"), "{text}");
        assert!(text.contains("3f9c1a"), "{text}");
        assert!(text.contains("撤回 · "), "{text}");
        assert!(text.contains("2 条证据 · 1 条撤回"), "{text}");
    }

    #[test]
    fn empty_evidence_says_so() {
        let view = EvidenceView::default();
        let panel = panel(&view, PanelTitle::Single("事件与证据".to_owned()));
        assert!(panel.lines.iter().any(
            |line| matches!(line, PanelLine::Text { text, .. } if text.contains("尚无证据记录"))
        ));
    }
}
