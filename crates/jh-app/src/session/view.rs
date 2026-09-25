//! 领域对象 → TUI 视图模型：三个研究面板与判定轨迹的翻译层。
//!
//! 设计来源：`docs/tui-design.md` §8 的对照表——`jh-tui` 不认识 `jh-core` 的类型，
//! 由这一层把领域对象翻成它的视图模型；时间戳、哈希、节点名与字段名都在这里格式化好
//! （`jh-tui` 不做时区、日历与命名解析）。
//!
//! 概率与置信度原值搬运，不归一化、不排名、不塌缩（设计 §2.3）。

use jh_core::{
    Answer, EvidenceLog, FieldSelection, Invalidation, InvalidationCause, JudgeSource,
    JudgmentNode, NodeRef, PremiseCheck, Question, QuestionVersion, SemanticState,
};
use jh_tui::blocks;
use jh_tui::components::Tone;
use jh_tui::panels::{
    ActionRow, Distribution, EdgeRow, EvidenceRow, EvidenceView, JudgmentView, PathRow,
    PropagationView, Validity,
};

use crate::session::gate::GateDecision;

/// 判断详情：从判断节点与问题定义组装。
///
/// 没有判断记录时也如实画出来（问题、版本、有效性都在），分布位置由面板自己写
/// 「本次调用没有返回答案」——与「尚无判断数据」是两回事。
pub fn judgment_view(
    node: &JudgmentNode,
    question: &Question,
    current: QuestionVersion,
) -> JudgmentView {
    let record = node.record.as_ref();
    let answer = node.judgment().map(|judgment| &judgment.answer);
    JudgmentView {
        question: node.question.as_str().to_owned(),
        question_version: node.question_version.0,
        current_version: node.question_version == current,
        distribution: answer.map(|answer| distribution(answer, question)),
        confidence: answer.and_then(confidence),
        source: record
            .map(|record| source_label(record.source).to_owned())
            .unwrap_or_else(|| "—".to_owned()),
        executor: record.map(|record| record.version.clone()),
        request_id: record.and_then(|record| record.request_id.clone()),
        snapshot: record.map(|record| short_id(record.snapshot.as_str())),
        validity: validity(node.validity),
        projection: projection_lines(node),
        latency_ms: record.map(|record| record.latency_ms),
        usage: record
            .and_then(|record| record.usage.as_ref())
            .and_then(|usage| Some((usage.input_tokens?, usage.output_tokens?))),
    }
}

/// 分布：与 `jh_core::Answer` 的三个变体一一对应。
pub fn distribution(answer: &Answer, question: &Question) -> Distribution {
    match answer {
        Answer::Choice(choice) => {
            let choices: Vec<blocks::Choice> = choice
                .probabilities
                .iter()
                .map(|(option, probability)| {
                    blocks::Choice::new(
                        option.clone(),
                        choice_label(question, option),
                        *probability as f32,
                    )
                })
                .collect();
            let chosen = choices.iter().position(|entry| entry.id == choice.choice);
            Distribution::Choice { choices, chosen }
        }
        Answer::Score(score) => {
            let levels = score
                .probabilities
                .iter()
                .map(|(level, probability)| {
                    (
                        score
                            .legend
                            .get(level)
                            .cloned()
                            .unwrap_or_else(|| format!("级 {level}")),
                        *probability as f32,
                    )
                })
                .collect();
            Distribution::Score {
                expected: score.score as f32,
                levels,
            }
        }
        Answer::Noul(noul) => Distribution::Noul {
            probability: noul.noul as f32,
        },
    }
}

/// 二选一问题的选项展示名：取问题定义里的选项描述，没有就用选项名本身。
fn choice_label(question: &Question, option: &str) -> String {
    match question {
        Question::Choice(choice) => choice
            .criteria
            .get(option)
            .and_then(|description| description.clone())
            .unwrap_or_else(|| option.to_owned()),
        _ => option.to_owned(),
    }
}

/// 答案自带的置信度；二值分布没有这个字段（类型上就不存在）。
fn confidence(answer: &Answer) -> Option<f32> {
    match answer {
        Answer::Choice(choice) => Some(choice.confidence as f32),
        Answer::Score(score) => Some(score.confidence as f32),
        Answer::Noul(_) => None,
    }
}

/// 依据有效性。
pub fn validity(validity: jh_core::Validity) -> Validity {
    match validity {
        jh_core::Validity::Current => Validity::Current,
        jh_core::Validity::NeedsRecheck => Validity::NeedsRecheck,
        jh_core::Validity::Retracted => Validity::Retracted,
    }
}

/// 这次判断读了哪些对象与字段：`对象 v版本: 全部字段 / 字段 a、b`。
pub fn projection_lines(node: &JudgmentNode) -> Vec<String> {
    node.projection
        .objects()
        .map(|(object, read)| {
            let fields = match &read.fields {
                FieldSelection::All => "全部字段".to_owned(),
                FieldSelection::Named { fields } => format!(
                    "字段 {}",
                    fields.iter().cloned().collect::<Vec<_>>().join("、")
                ),
            };
            format!("{} v{}: {fields}", object.as_str(), read.version.0)
        })
        .collect()
}

/// 事件与证据：追加写的证据序列（撤回是行上的标记，不是删历史）。
pub fn evidence_view(log: &dyn EvidenceLog) -> EvidenceView {
    EvidenceView {
        rows: log
            .entries()
            .iter()
            .map(|entry| EvidenceRow {
                id: entry.id.0,
                at: at_utc(entry.at_unix_ms),
                source: entry.source.clone(),
                object: entry.object.as_str().to_owned(),
                version: entry.version.0,
                hash: short_id(&entry.content_hash.to_string()),
                summary: summary(&entry.content),
                retracted: log.is_retracted(entry.id),
            })
            .collect(),
    }
}

/// 失效传播：起因与路径来自最近一次传播，待重查与动作复核读的是**当前**状态。
///
/// 这样面板既有「刚发生了什么」（起因、路径），也有「现在还有什么没复核」（待重查、动作）。
pub fn propagation_view(state: &SemanticState, last: Option<&Invalidation>) -> PropagationView {
    let stale: Vec<String> = state
        .recheck_set()
        .iter()
        .map(|id| node_label(state, *id))
        .collect();

    let mut blocked = Vec::new();
    let mut actions = Vec::new();
    for action in state.actions() {
        let check = state.check_action(&action.id);
        if matches!(check, Ok(PremiseCheck::Blocked { .. })) {
            blocked.push(action.id.as_str().to_owned());
        }
        let (text, tone) = check_text(state, check);
        actions.push(ActionRow {
            action: action.id.as_str().to_owned(),
            check: text,
            tone,
        });
    }

    PropagationView {
        cause: last.map(|invalidation| cause_text(state, &invalidation.cause)),
        stale,
        blocked,
        paths: last
            .map(|invalidation| {
                invalidation
                    .paths
                    .iter()
                    .map(|path| PathRow {
                        target: node_ref_label(state, &path.target),
                        hops: path
                            .hops
                            .iter()
                            .map(|hop| kind_label(hop.kind).to_owned())
                            .collect::<Vec<_>>()
                            .join(" → "),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        edges: state
            .graph()
            .edges()
            .iter()
            .map(|edge| EdgeRow {
                from: node_ref_label(state, &edge.from),
                kind: kind_label(edge.kind).to_owned(),
                to: node_ref_label(state, &edge.to),
                origin: origin_label(edge.origin).to_owned(),
            })
            .collect(),
        actions,
    }
}

/// 门控阶段行：`判定 12ms · 采纳 usr:allow 0.71 · margin 0.49 · 倾向`。
pub fn decision_line(decision: &GateDecision) -> String {
    if decision.reused {
        format!(
            "判定复用（防抖窗口内同一材料，上次 {}ms）· {}",
            decision.latency_ms, decision.reason
        )
    } else {
        format!("判定 {}ms · {}", decision.latency_ms, decision.reason)
    }
}

/// 证据行的时间：UTC 的 `HH:MM:SSZ`。
///
/// 本 crate 不引日期库（设计 §8）：日期与本地时区看证据文件本身，界面上只给一个稳定的时刻。
pub fn at_utc(at_unix_ms: u64) -> String {
    let seconds_of_day = (at_unix_ms / 1000) % 86_400;
    format!(
        "{:02}:{:02}:{:02}Z",
        seconds_of_day / 3600,
        (seconds_of_day % 3600) / 60,
        seconds_of_day % 60
    )
}

/// 判断来源标签（与判断记录里的 `source` 取值一致）。
pub fn source_label(source: JudgeSource) -> &'static str {
    match source {
        JudgeSource::RealApi => "RealApi",
        JudgeSource::Llm => "Llm",
        JudgeSource::Rule => "Rule",
        JudgeSource::Replay => "Replay",
        JudgeSource::SimulatedTruth => "SimulatedTruth",
    }
}

/// 长标识的短写法：取冒号后的前 8 个字符（内容摘要打印成 `fnv1a64:…` 的形态）。
fn short_id(id: &str) -> String {
    let tail = id.rsplit(':').next().unwrap_or(id);
    let head: String = tail.chars().take(8).collect();
    if tail.chars().count() > 8 {
        format!("{head}…")
    } else {
        head
    }
}

/// 证据内容的一行摘要：优先用写入方给的 `note`，没有就把 JSON 压成一行截断。
fn summary(content: &serde_json::Value) -> String {
    if let Some(note) = content.get("note").and_then(|note| note.as_str()) {
        return clip(note, 80);
    }
    clip(&content.to_string(), 80)
}

fn clip(text: &str, max: usize) -> String {
    let head: String = text.chars().take(max).collect();
    if text.chars().count() > max {
        format!("{head}…")
    } else {
        head
    }
}

/// 判断节点的展示名：`问题 v版本`。
fn node_label(state: &SemanticState, id: jh_core::NodeId) -> String {
    match state.node(id) {
        Some(node) => format!("{} v{}", node.question.as_str(), node.question_version.0),
        None => format!("节点 #{}", id.0),
    }
}

/// 图里一个节点的展示名。
fn node_ref_label(state: &SemanticState, node: &NodeRef) -> String {
    match node {
        NodeRef::Object(object) => object.as_str().to_owned(),
        NodeRef::Judgment(id) => node_label(state, *id),
        NodeRef::Action(action) => action.as_str().to_owned(),
    }
}

/// 起因一行的文案。
fn cause_text(state: &SemanticState, cause: &InvalidationCause) -> String {
    match cause {
        InvalidationCause::EvidenceAppended(id) => format!("追加证据 #{}", id.0),
        InvalidationCause::EvidenceRetracted(id) => format!("撤回证据 #{}", id.0),
        InvalidationCause::JudgmentRetracted(node) => {
            format!("撤销判断 {}", node_label(state, *node))
        }
        InvalidationCause::QuestionVersionChanged { question, version } => {
            format!("{} 改到 v{}", question.as_str(), version.0)
        }
        InvalidationCause::ObserverInvalidated { observer, version } => {
            format!("观测器 {} v{} 退出使用", observer.as_str(), version.0)
        }
        InvalidationCause::FullRecheck => "全量重查".to_owned(),
    }
}

/// 行动前提复核一行的文案与语气。
fn check_text(
    state: &SemanticState,
    check: Result<PremiseCheck, jh_core::Error>,
) -> (String, Tone) {
    match check {
        Ok(PremiseCheck::Cleared) => ("可发出".to_owned(), Tone::Success),
        Ok(PremiseCheck::Blocked { premises }) if premises.is_empty() => {
            ("被挡下（行动自身已失效）".to_owned(), Tone::Warning)
        }
        Ok(PremiseCheck::Blocked { premises }) => (
            format!(
                "被挡下：前提 {}",
                premises
                    .iter()
                    .map(|id| node_label(state, *id))
                    .collect::<Vec<_>>()
                    .join("、")
            ),
            Tone::Warning,
        ),
        Ok(PremiseCheck::Withdrawn) => ("已撤回".to_owned(), Tone::Error),
        Err(error) => (format!("查不了前提：{error}"), Tone::Dim),
    }
}

/// 四类依赖边的展示名。
fn kind_label(kind: jh_core::EdgeKind) -> &'static str {
    match kind {
        jh_core::EdgeKind::Read => "读取",
        jh_core::EdgeKind::Support => "支持",
        jh_core::EdgeKind::Conflict => "冲突",
        jh_core::EdgeKind::Premise => "前提",
    }
}

/// 边来源的展示名。
fn origin_label(origin: jh_core::EdgeOrigin) -> &'static str {
    match origin {
        jh_core::EdgeOrigin::Deterministic => "确定性",
        jh_core::EdgeOrigin::Proposed => "模型提出",
    }
}

/// 对象标识：会话里的三个对象名集中在这里，工具与门控都从这里取（避免拼写分散）。
pub mod objects {
    use jh_core::ObjectId;

    /// 待执行的命令（每次调用一个版本）。
    pub const SHELL_COMMAND: &str = "shell.command";
    /// 用户的每一次请求（每次提交一个版本）。
    pub const SESSION_ASK: &str = "session.ask";
    /// 每次工具执行的结果（每次执行一个版本）。
    pub const TOOL_RESULT: &str = "tool.result";
    /// 要写入或修改的文件与内容摘要（每次写/编辑调用一个版本）。
    pub const FILE_CHANGE: &str = "file.change";

    /// 三个对象的标识。
    pub fn all() -> Vec<ObjectId> {
        [SHELL_COMMAND, SESSION_ASK, TOOL_RESULT, FILE_CHANGE]
            .into_iter()
            .map(ObjectId::from)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::time::Duration;

    use jh_core::{
        Answer, ChoiceAnswer, JudgeRequest, JudgeResponse, JudgeSource, JudgmentRecord,
        NewEvidence, ObjectId, ObjectVersion, QuestionId, SnapshotId,
    };

    use super::*;
    use crate::session::gate::{OPTION_ALLOW, OPTION_DENY, question};

    fn request() -> JudgeRequest {
        JudgeRequest {
            snapshot: SnapshotId::from("fnv1a64:0123456789abcdef"),
            state: jh_core::State::Text("材料".to_owned()),
            questions: BTreeMap::from([(
                QuestionId::from(crate::session::gate::QUESTION_ID),
                question(),
            )]),
        }
    }

    fn record(allow: f64, deny: f64) -> JudgmentRecord {
        let response = JudgeResponse {
            source: JudgeSource::RealApi,
            version: "jev-1.13.0".to_owned(),
            answers: BTreeMap::from([(
                QuestionId::from(crate::session::gate::QUESTION_ID),
                Answer::Choice(ChoiceAnswer {
                    choice: OPTION_ALLOW.to_owned(),
                    probabilities: BTreeMap::from([
                        (OPTION_ALLOW.to_owned(), allow),
                        (OPTION_DENY.to_owned(), deny),
                    ]),
                    confidence: 0.64,
                }),
            )]),
            usage: Some(jh_core::Usage {
                input_tokens: Some(356),
                output_tokens: Some(38),
            }),
            request_id: Some("3f9c1a".to_owned()),
        };
        JudgmentRecord::from_response(&request(), &response, Duration::from_millis(12))
            .expect("记录")
    }

    #[test]
    fn judgment_view_keeps_probabilities_confidence_and_projection() {
        let mut state = SemanticState::in_memory();
        let object = ObjectId::from(objects::SHELL_COMMAND);
        let (_, invalidation) = state
            .append_evidence(NewEvidence {
                source: "session".to_owned(),
                at_unix_ms: 1_790_000_000_000,
                object: object.clone(),
                version: ObjectVersion(1),
                content: serde_json::json!({"note": "bash cargo build"}),
            })
            .expect("追加证据");
        assert!(invalidation.is_empty(), "没人读过这个对象，不该有传播");
        let projection = jh_core::InputProjection::covering([(object, ObjectVersion(1))]);
        let node_id = state.declare_node(
            QuestionId::from(crate::session::gate::QUESTION_ID),
            QuestionVersion(1),
            projection,
        );
        state
            .record_judgment(node_id, record(0.71, 0.22))
            .expect("写判断");

        let node = state.node(node_id).expect("节点在");
        let view = judgment_view(node, &question().question, QuestionVersion(1));
        assert_eq!(view.question, "shell_command_allowed");
        assert_eq!(view.question_version, 1);
        assert!(view.current_version);
        assert_eq!(view.confidence, Some(0.64));
        assert_eq!(view.source, "RealApi");
        assert_eq!(view.executor.as_deref(), Some("jev-1.13.0"));
        assert_eq!(view.snapshot.as_deref(), Some("01234567…"));
        assert_eq!(view.latency_ms, Some(12));
        assert_eq!(view.usage, Some((356, 38)));
        assert_eq!(view.validity, Validity::Current);
        assert_eq!(view.projection, vec!["shell.command v1: 全部字段"]);
        match view.distribution {
            Some(Distribution::Choice { choices, chosen }) => {
                assert_eq!(choices.len(), 2);
                assert_eq!(choices[0].id, OPTION_ALLOW);
                assert_eq!(choices[0].label, "允许执行");
                assert!((choices[0].probability - 0.71).abs() < 1e-6);
                assert_eq!(chosen, Some(0));
            }
            other => panic!("应是二选一分布：{other:?}"),
        }
    }

    #[test]
    fn evidence_view_formats_time_hash_and_retraction() {
        let mut state = SemanticState::in_memory();
        let object = ObjectId::from(objects::TOOL_RESULT);
        let (evidence, _) = state
            .append_evidence(NewEvidence {
                source: "bash".to_owned(),
                at_unix_ms: 1_790_000_045_000,
                object: object.clone(),
                version: ObjectVersion(1),
                content: serde_json::json!({"note": "bash git status --short → 退出码 0"}),
            })
            .expect("追加证据");
        state.retract_evidence(evidence.id).expect("撤回");

        let view = evidence_view(state.evidence());
        assert_eq!(view.rows.len(), 1);
        let row = &view.rows[0];
        assert_eq!(row.at, "14:14:05Z");
        assert_eq!(row.source, "bash");
        assert_eq!(row.object, "tool.result");
        assert_eq!(row.version, 1);
        assert_eq!(row.summary, "bash git status --short → 退出码 0");
        assert_eq!(
            row.hash.chars().count(),
            9,
            "8 位摘要 + 省略号：{}",
            row.hash
        );
        assert!(row.retracted);
        assert_eq!(view.retracted_count(), 1);
    }

    #[test]
    fn propagation_view_reports_cause_paths_stale_and_blocked_actions() {
        let mut state = SemanticState::in_memory();
        let object = ObjectId::from(objects::SHELL_COMMAND);
        state
            .append_evidence(NewEvidence {
                source: "session".to_owned(),
                at_unix_ms: 1,
                object: object.clone(),
                version: ObjectVersion(1),
                content: serde_json::json!({"command": "cargo build"}),
            })
            .expect("追加证据");
        let projection = jh_core::InputProjection::covering([(object.clone(), ObjectVersion(1))]);
        let node_id = state.declare_node(
            QuestionId::from(crate::session::gate::QUESTION_ID),
            QuestionVersion(1),
            projection,
        );
        state
            .record_judgment(node_id, record(0.9, 0.05))
            .expect("写判断");
        state
            .declare_action(jh_core::ActionId::from("bash#1"), [node_id])
            .expect("声明行动");
        assert_eq!(
            state.check_action(&jh_core::ActionId::from("bash#1")),
            Ok(PremiseCheck::Cleared)
        );

        // 同一对象出了新版本：依据变旧，行动被挡下，传播路径一并给出。
        let (_, invalidation) = state
            .append_evidence(NewEvidence {
                source: "session".to_owned(),
                at_unix_ms: 2,
                object: object.clone(),
                version: ObjectVersion(2),
                content: serde_json::json!({"command": "cargo test"}),
            })
            .expect("追加证据");

        let view = propagation_view(&state, Some(&invalidation));
        assert_eq!(view.cause.as_deref(), Some("追加证据 #2"));
        assert_eq!(view.stale, vec!["shell_command_allowed v1"]);
        assert_eq!(view.blocked, vec!["bash#1"]);
        assert!(
            view.paths
                .iter()
                .any(|path| path.target == "shell_command_allowed v1" && path.hops == "读取"),
            "{:?}",
            view.paths
        );
        assert!(
            view.actions
                .iter()
                .any(|action| action.action == "bash#1" && action.check.contains("被挡下")),
            "{:?}",
            view.actions
        );
        assert!(view.summary().is_some(), "有失效内容时状态行应有读数");
    }

    #[test]
    fn noul_answers_have_no_confidence_and_short_ids_are_clipped() {
        let noul = Answer::Noul(jh_core::NoulAnswer { noul: 0.31 });
        assert_eq!(confidence(&noul), None);
        assert_eq!(short_id("fnv1a64:0123456789abcdef"), "01234567…");
        assert_eq!(short_id("7b1e"), "7b1e");
    }
}
