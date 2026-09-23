//! 判断详情面板：问题与版本、分布、来源、输入快照与当前有效性。
//!
//! 设计来源：`docs/tui-design.md` §5.3、§8 的领域映射表——分布是三类问题原语的一一对应
//! （`Choice` / `Score` / `Noul`），概率与置信度原样上屏，不塌缩成布尔或名次。
//! `Noul` 没有独立的 `confidence` 字段：那是 `jh-core` 类型层面就不存在的东西。

use crate::blocks::{Choice, probability_bar};
use crate::components::panel::{Panel, PanelLine, PanelTitle, Tone};
use crate::glyphs::Glyphs;

/// 判断分布：与 `jh_core::answer::Answer` 的三个变体同形。
#[derive(Debug, Clone, PartialEq)]
pub enum Distribution {
    /// 候选项概率（复用块模型的 [`Choice`]）。
    Choice {
        choices: Vec<Choice>,
        chosen: Option<usize>,
    },
    /// 有序等级：期望值 + 各级概率。
    Score {
        expected: f32,
        levels: Vec<(String, f32)>,
    },
    /// 是非概率：只有一个自由度。
    Noul { probability: f32 },
}

/// 依据有效性：与 `jh_core::runtime::Validity` 同形。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Validity {
    #[default]
    Current,
    NeedsRecheck,
    Retracted,
}

impl Validity {
    pub fn label(self) -> &'static str {
        match self {
            Validity::Current => "当前有效",
            Validity::NeedsRecheck => "待重查",
            Validity::Retracted => "已撤回",
        }
    }

    pub fn tone(self) -> Tone {
        match self {
            Validity::Current => Tone::Success,
            Validity::NeedsRecheck => Tone::Warning,
            Validity::Retracted => Tone::Error,
        }
    }
}

/// 判断详情面板的视图模型。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct JudgmentView {
    pub question: String,
    pub question_version: u32,
    /// 是否是该问题 ID 的当前版本（旧版本仍可查看）。
    pub current_version: bool,
    pub distribution: Option<Distribution>,
    /// Jev 返回的整体置信度；`Noul` 不返回。
    pub confidence: Option<f32>,
    /// 判断来源标签（`RealApi` / `Llm` / `Rule` / `Replay` / `SimulatedTruth`）。
    pub source: String,
    /// 执行者版本（模型 ID 或规则版本串）。
    pub executor: Option<String>,
    pub request_id: Option<String>,
    pub snapshot: Option<String>,
    pub validity: Validity,
    /// 这次判断读了哪些对象与字段（适配层已格式化成一行一条）。
    pub projection: Vec<String>,
    pub latency_ms: Option<u64>,
    /// token 用量 `(输入, 输出)`。
    pub usage: Option<(u64, u64)>,
}

/// 概率条的列宽（与判定块同一档视觉密度）。
const BAR_WIDTH: usize = 8;

/// 组装判断详情面板。
pub fn panel(view: &Option<JudgmentView>, title: PanelTitle, glyphs: Glyphs) -> Panel {
    let Some(view) = view else {
        return super::empty_panel(title, "判断数据");
    };

    let version = if view.current_version {
        format!("v{}（当前）", view.question_version)
    } else {
        format!("v{}（旧版本）", view.question_version)
    };
    let mut lines = vec![
        PanelLine::Field {
            label: "问题".to_owned(),
            text: view.question.clone(),
            tone: Tone::Normal,
        },
        PanelLine::Field {
            label: "版本".to_owned(),
            text: version,
            tone: if view.current_version {
                Tone::Normal
            } else {
                Tone::Muted
            },
        },
    ];

    lines.push(PanelLine::Blank);
    lines.push(PanelLine::Section("分布".to_owned()));
    match &view.distribution {
        Some(Distribution::Choice { choices, chosen }) => {
            for (index, choice) in choices.iter().enumerate() {
                let bar = probability_bar(glyphs, choice.probability, BAR_WIDTH);
                let mark = if *chosen == Some(index) { "▸ " } else { "  " };
                lines.push(PanelLine::Text {
                    text: format!(
                        "{mark}{:.2} {bar}  {} ({})",
                        choice.probability, choice.label, choice.id
                    ),
                    tone: if *chosen == Some(index) {
                        Tone::Plan
                    } else {
                        Tone::Normal
                    },
                });
            }
        }
        Some(Distribution::Score { expected, levels }) => {
            lines.push(PanelLine::Text {
                text: format!("期望 {expected:.2}"),
                tone: Tone::Plan,
            });
            for (label, probability) in levels {
                let bar = probability_bar(glyphs, *probability, BAR_WIDTH);
                lines.push(PanelLine::Text {
                    text: format!("{probability:.2} {bar}  {label}"),
                    tone: Tone::Normal,
                });
            }
        }
        Some(Distribution::Noul { probability }) => {
            let bar = probability_bar(glyphs, *probability, BAR_WIDTH);
            lines.push(PanelLine::Text {
                text: format!("为真概率 {probability:.2} {bar}"),
                tone: Tone::Plan,
            });
            lines.push(PanelLine::Text {
                text: "二值分布没有独立的置信度字段（需要时读它到两端点的距离）".to_owned(),
                tone: Tone::Dim,
            });
        }
        None => lines.push(PanelLine::Text {
            text: "本次调用没有返回答案".to_owned(),
            tone: Tone::Warning,
        }),
    }
    if let Some(confidence) = view.confidence {
        lines.push(PanelLine::Text {
            text: format!("confidence {confidence:.2}"),
            tone: Tone::Muted,
        });
    }

    lines.push(PanelLine::Blank);
    lines.push(PanelLine::Section("来源".to_owned()));
    let origin = match &view.executor {
        Some(executor) => format!("{} · {executor}", view.source),
        None => view.source.clone(),
    };
    lines.push(PanelLine::Field {
        label: "判断器".to_owned(),
        text: origin,
        tone: Tone::Normal,
    });
    if let Some(request_id) = &view.request_id {
        lines.push(PanelLine::Field {
            label: "请求".to_owned(),
            text: request_id.clone(),
            tone: Tone::Dim,
        });
    }
    if let Some(snapshot) = &view.snapshot {
        lines.push(PanelLine::Field {
            label: "快照".to_owned(),
            text: snapshot.clone(),
            tone: Tone::Dim,
        });
    }
    if let Some(latency) = view.latency_ms {
        lines.push(PanelLine::Field {
            label: "延迟".to_owned(),
            text: format!("{latency} ms"),
            tone: Tone::Muted,
        });
    }
    if let Some((input, output)) = view.usage {
        lines.push(PanelLine::Field {
            label: "用量".to_owned(),
            text: format!("in {input} / out {output}"),
            tone: Tone::Muted,
        });
    }

    if !view.projection.is_empty() {
        lines.push(PanelLine::Blank);
        lines.push(PanelLine::Section("读取范围".to_owned()));
        for entry in &view.projection {
            lines.push(PanelLine::Text {
                text: entry.clone(),
                tone: Tone::Dim,
            });
        }
    }

    lines.push(PanelLine::Blank);
    lines.push(PanelLine::Field {
        label: "有效性".to_owned(),
        text: view.validity.label().to_owned(),
        tone: view.validity.tone(),
    });

    Panel::with_lines(title, lines)
        .with_footer(format!("{} v{}", view.question, view.question_version))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::BlockCtx;
    use crate::theme::Theme;

    fn view(distribution: Distribution) -> JudgmentView {
        JudgmentView {
            question: "shell_command_allowed".to_owned(),
            question_version: 3,
            current_version: true,
            distribution: Some(distribution),
            confidence: Some(0.64),
            source: "RealApi".to_owned(),
            executor: Some("jev-1.13.0".to_owned()),
            request_id: Some("3f9c1a".to_owned()),
            snapshot: Some("7b1e".to_owned()),
            validity: Validity::NeedsRecheck,
            projection: vec!["app.log: 全部字段".to_owned()],
            latency_ms: Some(12),
            usage: Some((356, 38)),
        }
    }

    fn ctx<'a>(theme: &'a Theme, width: usize) -> BlockCtx<'a> {
        BlockCtx {
            width,
            theme,
            glyphs: Glyphs::FANCY,
            frame: 0,
            decision: None,
        }
    }

    fn text_of(panel: &Panel, ctx: &BlockCtx) -> String {
        panel
            .render(ctx, 40, &crate::components::PanelState::default(), true)
            .iter()
            .map(|line| line.text())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn choice_distribution_keeps_every_probability() {
        let theme = Theme::DARK;
        let ctx = ctx(&theme, 60);
        let panel = panel(
            &Some(view(Distribution::Choice {
                choices: vec![
                    Choice::new("usr:allow", "允许一次", 0.71),
                    Choice::new("usr:deny", "拒绝", 0.22),
                ],
                chosen: Some(0),
            })),
            PanelTitle::Single("判断详情".to_owned()),
            Glyphs::FANCY,
        );
        let text = text_of(&panel, &ctx);
        assert!(text.contains("0.71"), "{text}");
        assert!(text.contains("0.22"), "{text}");
        assert!(text.contains("confidence 0.64"), "{text}");
        assert!(text.contains("待重查"), "{text}");
        assert!(text.contains("jev-1.13.0"), "{text}");
        assert!(text.contains("356"), "{text}");
    }

    #[test]
    fn noul_distribution_says_there_is_no_confidence() {
        let theme = Theme::DARK;
        let ctx = ctx(&theme, 60);
        let mut view = view(Distribution::Noul { probability: 0.31 });
        view.confidence = None;
        let panel = panel(
            &Some(view),
            PanelTitle::Single("判断详情".to_owned()),
            Glyphs::FANCY,
        );
        let text = text_of(&panel, &ctx);
        assert!(text.contains("0.31"), "{text}");
        assert!(text.contains("二值分布"), "{text}");
        assert!(!text.contains("confidence"), "{text}");
    }

    #[test]
    fn score_distribution_shows_the_expected_value_and_levels() {
        let theme = Theme::DARK;
        let ctx = ctx(&theme, 60);
        let panel = panel(
            &Some(view(Distribution::Score {
                expected: 2.4,
                levels: vec![("低".to_owned(), 0.1), ("中".to_owned(), 0.7)],
            })),
            PanelTitle::Single("判断详情".to_owned()),
            Glyphs::FANCY,
        );
        let text = text_of(&panel, &ctx);
        assert!(text.contains("期望 2.40"), "{text}");
        assert!(text.contains("0.70"), "{text}");
    }

    #[test]
    fn missing_judgment_renders_an_honest_empty_state() {
        let theme = Theme::DARK;
        let ctx = ctx(&theme, 60);
        let panel = panel(
            &None,
            PanelTitle::Single("判断详情".to_owned()),
            Glyphs::FANCY,
        );
        let text = text_of(&panel, &ctx);
        assert!(text.contains("尚无判断数据"), "{text}");
    }
}
