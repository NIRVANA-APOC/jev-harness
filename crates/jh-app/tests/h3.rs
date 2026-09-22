//! H3 对照的离线端到端测试：用确定性规则判定器与**预置候选产物**跑完整条链路
//! （开发失败 → 反例 → 候选 → 验证 → 影子 → 晋升 → 两臂对照 → 报告）。
//!
//! 真调版本见 `jev h3 --judge jev --judge llm`；这里跑的是同一段编排代码，只是把生成换成
//! `--candidates`（真实运行里生成的候选也会落成同一个文件，因此这条路径就是复现路径）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use jh_app::family_b::FamilyB;
use jh_app::generation::{CandidateQuestion, GeneratedCandidate, GenerationCall};
use jh_app::h3::{self, CandidateSource, H3Options, H3Report};
use jh_app::judge::JudgeName;
use serde_json::Value;

/// 临时目录（进程 + 线程唯一）。
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "jh-app-h3-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).expect("临时目录应能创建");
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

/// 预置候选：精化该对的门控问题，读法里加上关键证据，选项标签集合原样保留。
fn candidates(family: &FamilyB) -> Vec<GeneratedCandidate> {
    family
        .pairs
        .iter()
        .map(|pair| {
            let fixed = family
                .question(&pair.question)
                .expect("固定问题存在")
                .question
                .clone();
            let mut question = fixed;
            if let jh_core::Question::Choice(choice) = &mut question {
                choice.instructions =
                    "按这次读到的材料判断：证书窗口与对时结果、上游窗口与重试次数、权威可达性与应答分别说明哪一类原因？证据不足时按 other 的判据作答。"
                        .to_owned();
            }
            GeneratedCandidate {
                pair: pair.id.clone(),
                call: GenerationCall {
                    pair: pair.id.clone(),
                    version: Some("preloaded".to_owned()),
                    request_id: None,
                    usage: None,
                    input_bytes: 0,
                    output_bytes: 0,
                    latency_ms: 0,
                    error: None,
                },
                candidate: Some(CandidateQuestion {
                    refines: pair.question.clone(),
                    question,
                    distinguishes: format!("{}：两侧的补救动作不同", pair.title),
                    evidence: BTreeMap::from([
                        (jh_core::ObjectId::from("app.log"), jh_core::FieldSelection::All),
                        (pair.key_object.clone(), jh_core::FieldSelection::All),
                    ]),
                    evidence_freshness: "观察动作产出的最新一版".to_owned(),
                    evidence_method: format!(
                        "初始快照（B1）或观察动作 {}（B2）",
                        pair.probe_action
                    ),
                    scope: format!("任务族 B 的 {} 对", pair.id),
                    trigger: "出现该对场景的失败信号时评估".to_owned(),
                    consumers: jh_app::promotion::allowed_consumers(),
                    validation_placeholder: "验证记录待填：尚无读数".to_owned(),
                    lifecycle: "candidate".to_owned(),
                }),
            }
        })
        .collect()
}

async fn run_offline(family: &FamilyB, dir: &TempDir) -> H3Report {
    let candidates_path = dir.path().join("candidates.json");
    let text = serde_json::to_string_pretty(&candidates(family)).expect("候选可序列化");
    std::fs::write(&candidates_path, text).expect("候选文件可写");

    let options = H3Options {
        judges: vec![JudgeName::Rule],
        run: "h3-offline".to_owned(),
        calls_path: dir.path().join("calls.jsonl"),
        replay_path: dir.path().join("calls.jsonl"),
        out_dir: dir.path().join("run"),
        command: "cargo test -p jh-app --test h3".to_owned(),
        max_tasks: None,
        candidates: CandidateSource::File(candidates_path),
    };
    let outcome = h3::run(family, &options).await.expect("离线 H3 应跑到底");
    assert!(outcome.clean, "规则判定器不该失败");
    outcome.report
}

fn arm<'a>(report: &'a H3Report, judge: &str, arm: &str) -> &'a jh_app::h3::ArmReport {
    report
        .arms
        .iter()
        .find(|entry| entry.judge == judge && entry.arm == arm)
        .expect("报告里有这条臂")
}

#[tokio::test]
async fn h3_runs_the_whole_chain_and_the_refined_arm_wins_on_unseen_variants() {
    let family = FamilyB::load().expect("内嵌任务族 B 应自洽");
    let dir = TempDir::new("end-to-end");
    let report = run_offline(&family, &dir).await;

    // 反例来自开发案例的失败现场；候选来自预置产物。
    assert!(
        report.generation.counterexamples >= 3,
        "每个开发案例至少一条反例"
    );
    assert_eq!(report.generation.per_pair.len(), 3);
    assert!(report.generation.source == "file");

    // 三对全部达标注并发布（v2），每个候选都有四路变更与边界核对。
    let promoted = report
        .promotion
        .decisions
        .iter()
        .filter(|decision| decision.promoted)
        .count();
    assert_eq!(promoted, 3, "{:?}", report.promotion.decisions);
    for decision in &report.promotion.decisions {
        assert!(decision.criteria.iter().all(|item| item.passed));
        let change = decision.change.as_ref().expect("有变更记录");
        assert!(change.question_changed, "问题定义变了");
        assert!(change.input_projection_changed, "输入投影（读取范围）变了");
        assert!(!change.consumer_changed, "消费者没变（固定消费协议）");
        assert!(!change.policy_changed, "策略没变（控制规则冻结）");
        assert!(
            decision.boundaries.iter().all(|check| check.passed),
            "{:?}",
            decision.boundaries
        );
        assert!(decision.invalidation.as_ref().expect("有联动读数").stale >= 1);
    }
    // 发布之后旧版本还在：库状态里每个问题两个版本，当前启用 v2。
    for entry in &report.promotion.library {
        assert_eq!(entry.enabled, Some(2));
        assert_eq!(entry.versions.len(), 2);
    }
    // 回退演练：从 v2 回到 v1，旧版本保留。
    let rollback = report.promotion.rollback.as_ref().expect("有回退演练");
    assert_eq!((rollback.from, rollback.to), (2, 1));
    assert!(rollback.versions.len() == 2);

    // 两臂：固定臂用 v1、精化臂用 v2。
    let fixed = arm(&report, "rule", "fixed");
    let refined = arm(&report, "rule", "refined");
    assert_eq!(fixed.question_versions.values().next(), Some(&1));
    assert_eq!(refined.question_versions.values().next(), Some(&2));

    // 未见变体上的对照：固定臂验收全不过，精化臂全过。
    assert_eq!(fixed.totals.acceptance, "0/6", "{:?}", fixed.tasks);
    assert_eq!(refined.totals.acceptance, "6/6", "{:?}", refined.tasks);

    // B1 / B2 分开报告，两边都给出读数。
    assert_eq!(fixed.subsets.len(), 2);
    for subset in &fixed.subsets {
        assert_eq!(
            subset.totals.tasks, 3,
            "{} 子集应当 3 个变体",
            subset.subset
        );
    }
    let b2 = refined
        .subsets
        .iter()
        .find(|subset| subset.subset == "B2")
        .expect("有 B2 读数");
    assert_eq!(b2.totals.acceptance, "3/3");
    assert_eq!(
        b2.totals.observation_actions, 3,
        "每个 B2 任务发一次观察动作"
    );
    assert!(b2.totals.observation_cost_units > 0.0, "观察动作计费");

    // 材料不足却读出候选集内标签的诊断读数：固定臂上每个任务的第一轮都材料不足（它读不到关键证据）。
    assert!(
        fixed.totals.material_insufficient >= 6,
        "{:?}",
        fixed.totals
    );
    assert_eq!(
        fixed.totals.candidate_label_when_insufficient, 0,
        "规则判定器在材料不足时按 other 作答（真实模型的漂移读数在真调报告里）"
    );
    // 精化臂只在 B2 的第一轮材料不足（观察动作还没发出），之后都读到关键证据。
    assert_eq!(
        refined.totals.material_insufficient, 3,
        "{:?}",
        refined.totals
    );
    assert_eq!(refined.totals.candidate_label_when_insufficient, 0);

    // 成本账按用途分组，生成 / 验证 / 影子 / 开发案例 / 两臂都在。
    let purposes: Vec<&str> = report
        .cost
        .by_purpose
        .iter()
        .map(|entry| entry.purpose.as_str())
        .collect();
    for purpose in [
        "generation",
        "validation",
        "shadow",
        "development",
        "fixed_arm",
        "refined_arm",
    ] {
        assert!(purposes.contains(&purpose), "成本账缺 {purpose}");
    }
    assert!(report.raw.candidates_file.contains("candidates.json"));

    // 报告里写明两臂、统一字段、边界与预声明标准（复核要看得见口径）。
    assert_eq!(report.protocol.arms.len(), 4, "两臂 + 两条结构性声明");
    assert!(report.protocol.arms[2].contains("读取范围"));
    assert!(report.protocol.consumption_protocol.contains("统一字段"));
    assert_eq!(report.protocol.boundaries.len(), 4);
    assert!(!report.protocol.promotion_criteria.is_empty());
    // 复用候选产物时，生成那一笔按产物读数归成 generation_reused 进成本账与收益扣减。
    assert_eq!(report.generation.source, "file");
    assert_eq!(report.generation.calls, 0);
    let reuse = report.generation.reused.as_ref().expect("有复用读数");
    assert_eq!(reuse.calls, 3, "三个候选各一次生成调用");
    assert!(
        report
            .cost
            .by_purpose
            .iter()
            .any(|purpose| purpose.purpose == "generation_reused" && purpose.reused),
        "成本账要有 generation_reused"
    );
    let gains = &report.gains.per_judge[0];
    let purpose_calls = |name: &str| {
        report
            .cost
            .by_purpose
            .iter()
            .find(|purpose| purpose.purpose == name)
            .map(|purpose| purpose.calls)
            .unwrap_or(0)
    };
    assert!(
        gains.refinement_calls
            >= purpose_calls("generation_reused")
                + purpose_calls("validation")
                + purpose_calls("shadow"),
        "精化循环调用数要含生成那一笔"
    );
    assert!(gains.generation_reused);
    assert_eq!(gains.generation_calls, reuse.calls);
    assert_eq!(gains.subsets.len(), 2, "收益要逐子集分列");
    assert!(
        report
            .protocol
            .metric_definitions
            .contains_key("material_sufficiency")
    );
}

#[tokio::test]
async fn the_refined_version_gets_its_answers_from_the_observation_action_in_b2() {
    let family = FamilyB::load().expect("内嵌任务族 B 应自洽");
    let dir = TempDir::new("observation");
    let report = run_offline(&family, &dir).await;

    let refined = arm(&report, "rule", "refined");
    let b2_tasks: Vec<&jh_app::h3::TaskSummary> = refined
        .tasks
        .iter()
        .filter(|task| task.subset == "B2")
        .collect();
    assert_eq!(b2_tasks.len(), 3);
    for task in b2_tasks {
        // 每个 B2 任务：先观察（计费动作），再用新问题读出正确一侧，最后发出补救动作。
        assert!(task.acceptance_passed, "{task:?}");
        assert_eq!(task.observations.len(), 1, "{task:?}");
        assert!(
            task.executions.iter().any(|line| line.contains("第 1 轮")),
            "{task:?}"
        );
        assert!(task.unrecovered.is_empty());
        assert!(task.wrong_actions.is_empty());
    }

    // 轨迹文件在报告里点名，报告本身可复核。
    for task in &refined.tasks {
        assert!(task.trace.ends_with(".json"));
        assert!(task.evidence.ends_with(".jsonl"));
    }
    // 报告可 JSON 往返（报告的落盘形态就是它）。
    let wire = serde_json::to_value(&report).expect("报告可序列化");
    assert_eq!(wire["arms"].as_array().map(Vec::len), Some(2));
    assert!(wire["cost"]["total"]["calls"].as_u64().unwrap() > 0);
    let _: Value = wire;
}

#[tokio::test]
async fn a_candidate_that_reads_the_key_evidence_but_answers_wrong_is_rejected_pipeline_wide() {
    // 管线级的行为型拒绝：把 P1 一个正例的期望读法翻到另一侧（材料说着「证书窗口已过期」，
    // 期望却是 clock_skew）。候选读得到关键证据（契约、证据对象、消费者都合规），但它按材料读出
    // expired_credential，与期望相左——判据「正例一致率」必须把它拒掉，而不是让它带着错读数晋升。
    let mut family = FamilyB::load().expect("内嵌任务族 B 应自洽");
    let flipped = jh_app::tasks::Truth::Choice {
        label: "clock_skew".to_owned(),
    };
    family
        .validation
        .iter_mut()
        .find(|case| case.id == "P1-positive-a")
        .expect("P1 的正例存在")
        .expected = flipped;

    let dir = TempDir::new("wrong-side-pipeline");
    let report = run_offline(&family, &dir).await;

    let p1 = report
        .promotion
        .decisions
        .iter()
        .find(|decision| decision.pair == "P1")
        .expect("P1 有晋升决定");
    assert!(!p1.promoted, "读得到关键证据却答错侧的候选不能晋升");
    let failed: Vec<&str> = p1
        .criteria
        .iter()
        .filter(|criterion| !criterion.passed)
        .map(|criterion| criterion.id.as_str())
        .collect();
    assert!(failed.contains(&"positive_cases"), "{failed:?}");
    // 形状是好的：契约、协议与权限三条仍然通过——拒绝的理由是读数，不是形状。
    for id in [
        "contract_complete",
        "consumption_protocol",
        "evidence_within_permissions",
    ] {
        assert!(
            p1.criteria
                .iter()
                .any(|criterion| criterion.id == id && criterion.passed),
            "{id} 应当通过：{failed:?}"
        );
    }
    // 这一对停在固定版本；另外两对不受影响。
    let p1_library = report
        .promotion
        .library
        .iter()
        .find(|entry| entry.question == "b_pair_cause_p1")
        .expect("P1 有库状态");
    assert_eq!(p1_library.enabled, Some(1));
    assert_eq!(p1_library.versions.len(), 2, "候选仍留在库里，只是没启用");
    assert_eq!(
        report
            .promotion
            .decisions
            .iter()
            .filter(|decision| decision.promoted)
            .count(),
        2,
        "只有 P1 被拒"
    );
}

#[tokio::test]
async fn a_candidate_that_does_not_pass_the_declared_criteria_is_not_promoted() {
    // 把候选改成读不到关键证据（只读 app.log）：契约核对与判据都会不过，问题集保持固定版本。
    let family = FamilyB::load().expect("内嵌任务族 B 应自洽");
    let dir = TempDir::new("no-promotion");
    let broken: Vec<GeneratedCandidate> = candidates(&family)
        .into_iter()
        .map(|mut entry| {
            if let Some(candidate) = &mut entry.candidate {
                candidate.evidence = BTreeMap::from([(
                    jh_core::ObjectId::from("app.log"),
                    jh_core::FieldSelection::All,
                )]);
            }
            entry
        })
        .collect();
    let candidates_path = dir.path().join("candidates.json");
    std::fs::write(
        &candidates_path,
        serde_json::to_string_pretty(&broken).unwrap(),
    )
    .unwrap();

    let options = H3Options {
        judges: vec![JudgeName::Rule],
        run: "h3-no-promotion".to_owned(),
        calls_path: dir.path().join("calls.jsonl"),
        replay_path: dir.path().join("calls.jsonl"),
        out_dir: dir.path().join("run"),
        command: "cargo test -p jh-app --test h3".to_owned(),
        max_tasks: None,
        candidates: CandidateSource::File(candidates_path),
    };
    let outcome = h3::run(&family, &options).await.expect("离线 H3 应跑到底");

    assert!(
        outcome
            .report
            .promotion
            .decisions
            .iter()
            .all(|decision| !decision.promoted),
        "{:?}",
        outcome.report.promotion.decisions
    );
    assert!(
        outcome.report.promotion.rollback.is_none(),
        "没晋升就没有回退演练"
    );
    for entry in &outcome.report.promotion.library {
        assert_eq!(entry.enabled, Some(1), "问题集保持在固定版本");
    }
    // 精化臂与固定臂用同一版：读数相同。
    let fixed = arm(&outcome.report, "rule", "fixed");
    let refined = arm(&outcome.report, "rule", "refined");
    assert_eq!(fixed.totals.acceptance, refined.totals.acceptance);
    assert_eq!(fixed.question_versions, refined.question_versions);
}
