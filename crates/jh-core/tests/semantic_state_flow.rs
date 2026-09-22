//! 端到端验收：研究目标 §5.4「撤回一条证据后，运行时应标记其相关判断与待执行动作，保留无关结果，
//! 并展示完整传播路径」，外加请求期间输入变化时的快照校验与保守全量重查出口。
//!
//! 用真实的 JSONL 日志文件跑一遍，顺带验证撤回在重放之后依然成立。

use std::collections::BTreeMap;

use jh_core::{
    ActionId, Answer, EdgeKind, EdgeOrigin, Evidence, EvidenceLog, InputProjection,
    InvalidationCause, JsonlEvidenceLog, JudgeSource, Judgment, JudgmentRecord, NewEvidence,
    NoulAnswer, NoulQuestion, ObjectId, ObjectVersion, PremiseCheck, Question, QuestionId,
    QuestionRegistry, QuestionSpec, QuestionVersion, Questions, SemanticState, SnapshotId,
    Validity, VersionComparison,
};
use serde_json::json;

/// 追加一条工具输出，返回完整记录（含日志分配的 id 与内容摘要）。
fn observe(
    state: &mut SemanticState,
    object: &str,
    version: u64,
    content: serde_json::Value,
) -> Evidence {
    state
        .append_evidence(NewEvidence {
            source: "tool:probe".to_owned(),
            at_unix_ms: 1_700_000_000_000 + version,
            object: ObjectId::from(object),
            version: ObjectVersion(version),
            content,
        })
        .expect("日志可写")
        .0
}

/// 按固定概率作答一所判断，模拟「拿到一次有效响应」。
fn judge(state: &mut SemanticState, node: jh_core::NodeId, noul: f64) {
    let question = state.node(node).expect("节点已声明").question.clone();
    let question_version = state.node(node).expect("节点已声明").question_version;
    state
        .record_judgment(
            node,
            JudgmentRecord {
                snapshot: SnapshotId::from("case-001"),
                source: JudgeSource::Rule,
                version: "rule-v1".to_owned(),
                request_id: None,
                usage: None,
                latency_ms: 2,
                judgments: BTreeMap::from([(
                    question,
                    Judgment {
                        question_version,
                        answer: Answer::Noul(NoulAnswer { noul }),
                        thresholds: BTreeMap::new(),
                    },
                )]),
            },
        )
        .expect("记录与节点一致");
}

fn noul(instructions: &str) -> Question {
    Question::Noul(NoulQuestion {
        instructions: instructions.to_owned(),
        criteria: None,
    })
}

/// 任务族 A 的最小片段：配置被改写推翻了旧判断，系统要标记相关判断与待执行动作。
#[test]
fn retracting_an_evidence_record_marks_judgments_actions_and_shows_the_paths() {
    let dir = std::env::temp_dir().join(format!("jh-core-flow-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let log_path = dir.join("evidence.jsonl");
    let _ = std::fs::remove_file(&log_path);

    let mut state = SemanticState::new(Box::new(
        JsonlEvidenceLog::open(&log_path).expect("新建证据日志"),
    ));

    // 问题集合带版本登记：改含义必须出新版本。
    let mut questions_registry = QuestionRegistry::default();
    let cache_disabled = QuestionId::from("cache_disabled");
    let cancel_after_write = QuestionId::from("cancel_after_write");
    questions_registry
        .register(
            &cache_disabled,
            &QuestionSpec {
                version: QuestionVersion(1),
                question: noul("缓存是否已关闭？"),
            },
        )
        .expect("首次登记");
    questions_registry
        .register(
            &cancel_after_write,
            &QuestionSpec {
                version: QuestionVersion(1),
                question: noul("取消之后是否还有写入？"),
            },
        )
        .expect("首次登记");
    assert_eq!(
        questions_registry.revise(&cache_disabled, noul("缓存是否已关闭且不再回填？")),
        QuestionVersion(2),
        "改含义出新版本"
    );
    assert_eq!(
        questions_registry.compare(&cache_disabled, QuestionVersion(1), QuestionVersion(2)),
        Some(VersionComparison::Changed)
    );

    // 一手材料：配置与日志各一条。
    let config = observe(&mut state, "cache.conf", 1, json!({"cache": "off"}));
    observe(&mut state, "deploy.log", 1, json!({"lines": 120}));

    // 一次请求问两个问题、共享一份 state：整批共用同一份保守投影。
    let projection = InputProjection::covering([
        (ObjectId::from("cache.conf"), ObjectVersion(1)),
        (ObjectId::from("deploy.log"), ObjectVersion(1)),
    ]);
    let questions: Questions = BTreeMap::from([
        (
            cache_disabled.clone(),
            QuestionSpec {
                version: QuestionVersion(1),
                question: noul("缓存是否已关闭？"),
            },
        ),
        (
            cancel_after_write.clone(),
            QuestionSpec {
                version: QuestionVersion(1),
                question: noul("取消之后是否还有写入？"),
            },
        ),
    ]);
    let batch = state.declare_batch(&questions, &projection);
    // 只读日志的第三方判断：它与配置无关，撤回配置不该动它。
    let log_only = state.declare_node(
        QuestionId::from("log_has_cancel"),
        QuestionVersion(1),
        InputProjection::covering([(ObjectId::from("deploy.log"), ObjectVersion(1))]),
    );
    for node in [batch[&cache_disabled], batch[&cancel_after_write], log_only] {
        judge(&mut state, node, 1.0);
    }

    let restart = ActionId::from("restart-cache");
    state
        .declare_action(restart.clone(), [batch[&cache_disabled]])
        .expect("前提节点存在");
    assert_eq!(state.check_action(&restart).unwrap(), PremiseCheck::Cleared);

    // 撤回那条配置证据。
    let invalidation = state.retract_evidence(config.id).expect("证据存在");

    assert_eq!(
        invalidation.cause,
        InvalidationCause::EvidenceRetracted(config.id)
    );
    assert_eq!(
        invalidation.stale,
        vec![batch[&cache_disabled], batch[&cancel_after_write]],
        "读了这份 state 的判断都保守地算受影响"
    );
    assert_eq!(invalidation.blocked, vec![restart.clone()]);
    assert_eq!(
        state.recheck_set(),
        vec![batch[&cache_disabled], batch[&cancel_after_write]]
    );

    // 完整传播路径：撤回的证据所在对象 → 读它的判断 →（前提）→ 行动。
    let to_action = invalidation
        .paths
        .iter()
        .find(|path| path.target == jh_core::NodeRef::Action(restart.clone()))
        .expect("行动在传播结果里");
    assert_eq!(
        to_action
            .hops
            .iter()
            .map(|hop| hop.kind)
            .collect::<Vec<_>>(),
        vec![EdgeKind::Read, EdgeKind::Premise]
    );
    assert_eq!(
        to_action.hops[0].from,
        jh_core::NodeRef::Object(ObjectId::from("cache.conf"))
    );

    // 无关结果保留。
    assert_eq!(state.node(log_only).unwrap().validity, Validity::Current);
    assert!(state.node(log_only).unwrap().is_usable());

    // 行动发不出去；撤销只改资格，判断记录还在。
    assert_eq!(
        state.check_action(&restart).unwrap(),
        PremiseCheck::Blocked {
            premises: vec![batch[&cache_disabled]]
        }
    );
    assert!(state.node(batch[&cache_disabled]).unwrap().record.is_some());

    // 撤回与判断都写进了日志：重开一次仍能看到撤回。
    let replayed = JsonlEvidenceLog::open(&log_path).expect("重放已存在的日志");
    assert!(replayed.is_retracted(config.id));
    assert_eq!(
        replayed.current_version(&ObjectId::from("cache.conf")),
        None,
        "配置上唯一一条记录被撤回"
    );
    assert_eq!(replayed.entries().len(), 2, "记录本身没被删掉");

    // 依赖环：两个判断互相支持时能被检测出来。
    state
        .link(
            batch[&cache_disabled],
            batch[&cancel_after_write],
            EdgeKind::Support,
            EdgeOrigin::Proposed,
        )
        .expect("节点存在");
    state
        .link(
            batch[&cancel_after_write],
            batch[&cache_disabled],
            EdgeKind::Support,
            EdgeOrigin::Proposed,
        )
        .expect("节点存在");
    let cycles = state.cycles();
    assert_eq!(cycles.len(), 1, "{cycles:?}");
    assert_eq!(cycles[0].len(), 2, "互相引用的两个判断");

    let _ = std::fs::remove_file(&log_path);
    let _ = std::fs::remove_dir(&dir);
}

/// 请求期间输入变化：结果只作审计；随后可以用保守全量重查兜底。
#[test]
fn a_result_whose_inputs_moved_during_the_request_is_audited_and_not_acted_on() {
    let mut state = SemanticState::in_memory();
    let config = observe(&mut state, "cache.conf", 1, json!({"cache": "off"}));

    // 请求时读到的是第 1 版。
    let projection = InputProjection::covering([(ObjectId::from("cache.conf"), ObjectVersion(1))]);
    let node = state.declare_node(
        QuestionId::from("cache_disabled"),
        QuestionVersion(1),
        projection.clone(),
    );

    // 结果还没回来，配置已经换成第 2 版。
    observe(&mut state, "cache.conf", 2, json!({"cache": "on"}));

    // 延迟返回的结果：记录留下，依据有效性只能是待重查。
    judge(&mut state, node, 1.0);
    let recorded = state.node(node).expect("刚记过");
    assert_eq!(recorded.validity, Validity::NeedsRecheck);
    assert!(recorded.record.is_some());
    assert!(!recorded.is_usable(), "旧的输入得出的结论不能当作当前依据");
    assert!(
        state.verify(&projection) != jh_core::SnapshotCheck::Current,
        "快照校验能看出漂移"
    );

    // 用新版材料重新判断：新节点是当前有效的。
    let fresh = state.declare_node(
        QuestionId::from("cache_disabled"),
        QuestionVersion(2),
        InputProjection::covering([(ObjectId::from("cache.conf"), ObjectVersion(2))]),
    );
    judge(&mut state, fresh, 0.0);
    let fresh_node = state.node(fresh).expect("刚记过");
    assert_eq!(fresh_node.validity, Validity::Current);
    assert!(fresh_node.is_usable());
    assert_eq!(
        fresh_node.judgment().expect("答过").answer,
        Answer::Noul(NoulAnswer { noul: 0.0 })
    );

    // 保守全量重查出口：所有当前有效的判断一并标为待重查。
    let invalidation = state.recheck_all();
    assert_eq!(invalidation.stale, vec![fresh]);
    assert!(invalidation.paths.is_empty());
    assert!(state.recheck_set().contains(&fresh));
    assert_eq!(
        state
            .evidence()
            .current_version(&ObjectId::from("cache.conf")),
        Some(ObjectVersion(2))
    );
    assert!(!state.evidence().is_retracted(config.id));
}
