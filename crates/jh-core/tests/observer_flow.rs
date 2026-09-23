//! 端到端验收：研究目标 §7.3 的场景——观测器要先经过独立检验才允许取证据，撤回它时读过它测量
//! 对象的判断与待执行动作一并失效，无关结果保留、传播路径完整；观测器库可落盘再读回。
//!
//! 用真实的 JSONL 证据日志跑一遍，顺带验证「停用不改写任何证据记录」。

use std::collections::{BTreeMap, BTreeSet};

use jh_core::{
    ActionId, Answer, ControlResult, EdgeKind, Error, EvidenceLog, InputProjection,
    JsonlEvidenceLog, JudgeSource, Judgment, JudgmentRecord, Measurement, NewEvidence, NoulAnswer,
    ObjectId, ObjectVersion, ObserverCheck, ObserverId, ObserverLibrary, ObserverLifecycle,
    ObserverVerdict, ObserverVersion, OutputContract, PremiseCheck, QuestionId, QuestionVersion,
    Requirement, SemanticState, SnapshotId, Timing, Validity,
};
use serde_json::json;

/// 一份填满 §7.2 八条的观测器契约。
fn contract(objects: &[&str]) -> jh_core::ObserverContract {
    jh_core::ObserverContract {
        requirement: Requirement {
            text: "取消确认后不应继续产生缓存写入".to_owned(),
            source: "用户要求（研究目标 §7.3）".to_owned(),
        },
        proposition: "同一任务的写入是否出现在取消确认之后".to_owned(),
        coverage: "覆盖「确认后写入」这一类顺序违反；不覆盖写入内容是否正确".to_owned(),
        measurement: Measurement {
            site: "缓存写入边界的事件记录器".to_owned(),
            objects: objects
                .iter()
                .map(|object| ObjectId::from(*object))
                .collect(),
            timing: Timing::Sequence,
        },
        support: jh_core::Support {
            versions: BTreeSet::from(["app-1.4.0".to_owned()]),
            configuration: "recorder=on".to_owned(),
            inputs: "同一任务的写入事件".to_owned(),
            environment: "隔离实验环境，含可暂停点".to_owned(),
        },
        disturbance: jh_core::Disturbance {
            side_effects: "追加写记录文件，不改被测数据".to_owned(),
            performance: "每事件一次追加写".to_owned(),
            scheduling: "在写入边界同步记录".to_owned(),
        },
        controls: jh_core::Controls {
            positive: "顺序正确的实现".to_owned(),
            negative: "已知错误实现（取消后仍写入）".to_owned(),
            injected_fault: "在确认之后注入一次写入".to_owned(),
        },
        output: OutputContract {
            channel: "observer:cancel-write-recorder".to_owned(),
            format: "每条写入事件一条证据（任务、事件序号）".to_owned(),
            failure: "读不到时显式失败，不给读不到的数".to_owned(),
            invalid_when: "记录器被绕过或事件序号不可比".to_owned(),
        },
        uncovered: "跨进程重启后的写入顺序".to_owned(),
    }
}

fn control(expected: ObserverVerdict, observed: ObserverVerdict) -> ControlResult {
    ControlResult {
        expected,
        observed,
        readings: BTreeMap::from([("cost_units".to_owned(), 3.0)]),
        note: "对照".to_owned(),
    }
}

/// 四路全过的独立检验记录。
fn passing_check() -> ObserverCheck {
    ObserverCheck {
        at_unix_ms: 1_700_000_002_000,
        positive: control(ObserverVerdict::Satisfied, ObserverVerdict::Satisfied),
        negative: control(ObserverVerdict::Violated, ObserverVerdict::Violated),
        injected_fault: control(ObserverVerdict::Violated, ObserverVerdict::Violated),
        hidden: control(ObserverVerdict::Violated, ObserverVerdict::Violated),
        note: "三路对照 + 隐藏检查".to_owned(),
    }
}

/// 追加一条证据；返回完整记录。
fn observe(
    state: &mut SemanticState,
    source: &str,
    object: &str,
    content: serde_json::Value,
) -> jh_core::Evidence {
    state
        .append_evidence(NewEvidence {
            source: source.to_owned(),
            at_unix_ms: 1_700_000_010_000,
            object: ObjectId::from(object),
            version: ObjectVersion(1),
            content,
        })
        .expect("日志可写")
        .0
}

/// 声明一个读过这些对象的判断节点并写入一次判断。
fn judge_reading(state: &mut SemanticState, question: &str, reads: &[&str]) -> jh_core::NodeId {
    let projection = InputProjection::covering(
        reads
            .iter()
            .map(|object| (ObjectId::from(*object), ObjectVersion(1))),
    );
    let node = state.declare_node(QuestionId::from(question), QuestionVersion(1), projection);
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
                    QuestionId::from(question),
                    Judgment {
                        question_version: QuestionVersion(1),
                        answer: Answer::Noul(NoulAnswer { noul: 0.02 }),
                        thresholds: BTreeMap::new(),
                    },
                )]),
            },
        )
        .expect("记录与节点一致");
    node
}

#[test]
fn a_verified_observer_earns_its_evidence_and_losing_it_invalidates_the_judgments() {
    let dir = std::env::temp_dir().join(format!("jh-core-observer-flow-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let log_path = dir.join("evidence.jsonl");
    let _ = std::fs::remove_file(&log_path);

    let mut state = SemanticState::new(Box::new(
        JsonlEvidenceLog::open(&log_path).expect("新建证据日志"),
    ));
    let mut library = ObserverLibrary::default();
    let observer = ObserverId::from("cancel_write_recorder");

    // 候选 → 验证 → 启用：启用要求一份四路全过的独立检验记录（§7.3 末段）。
    // 版本号写成显式类型：它在公共签名上出现，crate 根必须导出，外部才存得进自己的结构体。
    let version: ObserverVersion = library
        .propose(
            &observer,
            contract(&["cache.writes"]),
            1_700_000_000_000,
            "现有工具取不到写入边界的顺序证据".to_owned(),
        )
        .expect("候选可登记");
    library
        .start_verifying(
            &observer,
            version,
            1_700_000_001_000,
            "开始独立检验".to_owned(),
        )
        .expect("候选可进验证");
    let (records, invalidation) = library
        .enable(
            &mut state,
            &observer,
            version,
            passing_check(),
            1_700_000_003_000,
            "阳性 / 阴性 / 注入故障 / 隐藏检查四路全过".to_owned(),
        )
        .expect("检验全过即可启用");
    assert_eq!(records[0].lifecycle, ObserverLifecycle::Enabled);
    assert!(invalidation.is_none(), "全新启用没有替代谁");

    // 启用之后才用这条渠道取证据：来源就是契约里声明的渠道名。
    let writes = observe(
        &mut state,
        "observer:cancel-write-recorder",
        "cache.writes",
        json!({"task": "T-1", "write_after_confirm": false}),
    );
    observe(
        &mut state,
        "tool:read_log",
        "app.log",
        json!({"lines": 120}),
    );

    let read = judge_reading(&mut state, "write_after_cancel_confirm", &["cache.writes"]);
    let unrelated = judge_reading(&mut state, "cause_b_pair", &["app.log"]);
    let action = ActionId::from("declare-cancel-safe");
    state
        .declare_action(action.clone(), [read])
        .expect("前提节点存在");
    assert_eq!(state.check_action(&action).unwrap(), PremiseCheck::Cleared);

    // 隐藏检查复核时发现记录器被绕过（真实副作用没进日志）：撤回这条观察渠道。
    let (record, invalidation) = library
        .retire(
            &mut state,
            &observer,
            version,
            1_700_000_020_000,
            "隐藏检查发现记录器被绕过，测量不再可信".to_owned(),
        )
        .expect("启用中的版本可停用");

    assert_eq!(record.lifecycle, ObserverLifecycle::Retired);
    assert_eq!(
        invalidation.cause,
        jh_core::InvalidationCause::ObserverInvalidated {
            observer: observer.clone(),
            version,
        }
    );
    assert_eq!(invalidation.stale, vec![read]);
    assert_eq!(invalidation.blocked, vec![action.clone()]);
    assert_eq!(state.node(read).unwrap().validity, Validity::NeedsRecheck);
    assert_eq!(state.node(unrelated).unwrap().validity, Validity::Current);
    assert_eq!(state.recheck_set(), vec![read]);
    assert_eq!(
        state.check_action(&action).unwrap(),
        PremiseCheck::Blocked {
            premises: vec![read]
        },
        "以失效判断为前提的行动发不出去"
    );

    // 完整传播路径：被测对象 → 读它的判断 →（前提）→ 行动。
    let to_action = invalidation
        .paths
        .iter()
        .find(|path| path.target == jh_core::NodeRef::Action(action.clone()))
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
        jh_core::NodeRef::Object(ObjectId::from("cache.writes"))
    );
    assert_eq!(
        invalidation.paths.len(),
        invalidation.stale.len() + invalidation.blocked.len(),
        "每个目标一条路径"
    );

    // 停用不改写证据：记录还在日志里，来源仍是这条渠道，重放之后一条不少。
    let replayed = JsonlEvidenceLog::open(&log_path).expect("重放日志");
    assert_eq!(replayed.entries().len(), 2);
    assert_eq!(replayed.entries()[0].id, writes.id);
    assert_eq!(
        replayed.entries()[0].source,
        "observer:cancel-write-recorder"
    );
    assert!(!replayed.is_retracted(writes.id));
    assert_eq!(state.evidence().entries().len(), 2);
    assert_eq!(library.enabled(&observer), None, "观测器已退出使用");

    let _ = std::fs::remove_file(&log_path);
    let _ = std::fs::remove_dir(&dir);
}

#[test]
fn a_library_on_disk_comes_back_with_its_checks_and_still_invalidates() {
    let dir = std::env::temp_dir().join(format!("jh-core-observer-library-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let library_path = dir.join("observers.json");

    let mut state = SemanticState::in_memory();
    let mut library = ObserverLibrary::default();
    let observer = ObserverId::from("cancel_write_recorder");
    let version = library
        .propose(
            &observer,
            contract(&["cache.writes"]),
            1_700_000_000_000,
            "观测需求".to_owned(),
        )
        .expect("候选可登记");
    library
        .start_verifying(
            &observer,
            version,
            1_700_000_001_000,
            "开始独立检验".to_owned(),
        )
        .expect("候选可进验证");
    library
        .enable(
            &mut state,
            &observer,
            version,
            passing_check(),
            1_700_000_003_000,
            "四路全过".to_owned(),
        )
        .expect("检验全过即可启用");

    // 入库（观测器库的形态与问题库同口径：可 JSON 往返的库文件）。
    std::fs::write(&library_path, serde_json::to_vec_pretty(&library).unwrap()).expect("可写入");
    let mut loaded: ObserverLibrary =
        serde_json::from_slice(&std::fs::read(&library_path).unwrap()).expect("读回库文件");
    assert_eq!(loaded, library);
    assert_eq!(
        loaded.enabled(&observer).map(|(version, _)| version),
        Some(version)
    );
    assert_eq!(
        loaded.at(&observer, version).unwrap().checks.len(),
        1,
        "检验记录一并入库"
    );

    // 读回的库照常参与失效联动：撤回它，读过它测量对象的判断待重查。
    let mut fresh = SemanticState::in_memory();
    observe(
        &mut fresh,
        "observer:cancel-write-recorder",
        "cache.writes",
        json!({"event": 1}),
    );
    let read = judge_reading(&mut fresh, "write_after_cancel_confirm", &["cache.writes"]);

    let (record, invalidation) = loaded
        .retire(
            &mut fresh,
            &observer,
            version,
            1_700_000_020_000,
            "观测器被绕过".to_owned(),
        )
        .expect("启用中的版本可停用");
    assert_eq!(record.lifecycle, ObserverLifecycle::Retired);
    assert_eq!(invalidation.stale, vec![read]);

    // 停用是终态：读回的库里也不能再启用这个版本。
    let error = loaded
        .enable(
            &mut fresh,
            &observer,
            version,
            passing_check(),
            1_700_000_030_000,
            "停用后再启用".to_owned(),
        )
        .expect_err("停用过的版本不能重新启用");
    assert!(
        matches!(error, Error::InvalidObserverLifecycle { .. }),
        "{error}"
    );
    assert!(
        error.to_string().contains("cancel_write_recorder"),
        "{error}"
    );

    let _ = std::fs::remove_file(&library_path);
    let _ = std::fs::remove_dir(&dir);
}
