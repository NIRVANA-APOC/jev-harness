//! 任务族 A 的端到端测试：四组臂的读数方向、§5.4 受控演示、报告与原始数据位置、回放复现、
//! 失败路径与迟到结果的落记录时机。
//!
//! 全部用离线判定器（`rule` / `replay`）或测试内的桩判定器——真实通道的读数在 `var/compare/`
//! 的报告里，测试不联网。

use std::collections::BTreeMap;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;

use jh_app::calls::{CallLog, read_entries};
use jh_app::compare::{self, CompareOptions};
use jh_app::family::{FamilyA, StepWhen};
use jh_app::judge::JudgeName;
use jh_app::matching::{Arm, DecisionOutcome, LoopContext, run_task};
use jh_app::report::VerificationStatus;
use jh_core::{Error as CoreError, Judge, JudgeRequest, JudgeResponse};

mod common;

use common::TempDir;

/// 只会失败的桩判定器：用来驱动判定失败路径。
struct AlwaysFails;

impl Judge for AlwaysFails {
    fn judge<'a>(
        &'a self,
        _request: &'a JudgeRequest,
    ) -> Pin<Box<dyn Future<Output = Result<JudgeResponse, CoreError>> + Send + 'a>> {
        Box::pin(async move { Err(CoreError::Unavailable("桩判定器：通道不可用".to_owned())) })
    }
}

/// 跑一次四组臂的对照（离线规则判断器）。
async fn run_rule_arms(family: &FamilyA, dir: &TempDir, run: &str) -> compare::CompareReport {
    let options = CompareOptions {
        judges: vec![JudgeName::Rule],
        arms: Arm::ALL.to_vec(),
        run: run.to_owned(),
        calls_path: dir.join("calls.jsonl"),
        replay_path: dir.join("calls.jsonl"),
        out_dir: dir.join("out").join(run),
        command: format!("test compare {run}"),
        max_tasks: None,
    };
    compare::run(family, &options)
        .await
        .expect("离线对照应能跑完")
        .report
}

#[tokio::test]
async fn the_four_arms_show_the_declared_differences_without_missed_invalidations() {
    let family = FamilyA::load().expect("内嵌任务族 A 应自洽");
    let dir = TempDir::new("family-a-arms");
    let report = run_rule_arms(&family, &dir, "run-arms").await;

    assert_eq!(report.arms.len(), 4);
    let arm = |label: &str| {
        report
            .arms
            .iter()
            .find(|arm| arm.arm == label)
            .unwrap_or_else(|| panic!("报告里应有 {label} 臂"))
    };

    let on_inc = arm("h1on-h2inc");
    let off_inc = arm("h1off-h2inc");
    let on_full = arm("h1on-h2full");
    let off_full = arm("h1off-h2full");

    // 判定器来源与验证状态：本地判定器标「模拟」，不冒充真实读数。
    assert_eq!(on_inc.verification.status, VerificationStatus::Simulated);
    assert!(!on_inc.live);
    assert_eq!(on_inc.versions, vec!["rules-v2".to_owned()]);

    // H1：开启臂六个任务全部通过独立验收，也不基于失效前提发出动作。
    assert_eq!(on_inc.acceptance.passed, 6);
    assert_eq!(on_full.acceptance.passed, 6);
    assert_eq!(on_inc.decisions.stale_basis_commitments, 0);
    assert_eq!(on_full.decisions.stale_basis_commitments, 0);
    assert_eq!(on_inc.decisions.wrong_actions, 0);

    // H1 关闭臂：同一个任务集上漏掉了被失效标记牵动的动作、并在过期依据上发出过一次动作。
    assert!(
        off_inc.acceptance.passed < on_inc.acceptance.passed,
        "关闭失效传播应当拿不到同样的验收结果"
    );
    assert!(off_inc.decisions.stale_basis_commitments > 0);
    assert!(off_inc.decisions.wrong_actions > 0);
    assert!(off_inc.recovery.unrecovered.len() >= 4);
    // 关闭臂更省：更少的请求与更高的复用率——省下的正是它没做的重查。
    assert!(off_inc.recompute.asks < on_inc.recompute.asks);
    assert!(off_inc.recompute.cache_reuse_rate > on_inc.recompute.cache_reuse_rate);
    assert!(off_inc.recompute.stale_reuse_events > 0);

    // H2：全量臂每轮把全部问题重算一遍，增量臂复用仍然新鲜的结果。
    assert!(on_full.recompute.judged_nodes > on_inc.recompute.judged_nodes);
    assert!(on_full.recompute.asks > on_inc.recompute.asks);
    assert!(on_full.recompute.input_bytes > on_inc.recompute.input_bytes);
    assert_eq!(on_full.recompute.cache_reuse_rate, Some(0.0));
    assert!(on_inc.recompute.cache_reuse_rate.unwrap() > 0.0);

    // 全量重算不能替代发出前的有效性检查：漂移那一条在关闭臂上照样发出错动作。
    assert_eq!(off_full.decisions.wrong_actions, 1);
    assert!(off_full.acceptance.passed < on_full.acceptance.passed);

    // 失效传播的确定性部分：声明范围 + 对象级传播下没有漏失效；
    // 唯一的误失效来自任务 A2 登记的「模型建议额外依赖」（premise_holds ← runtime.metrics）。
    for arm in &report.arms {
        assert_eq!(arm.invalidation.missed, 0, "{} 不应有漏失效", arm.arm);
        assert_eq!(
            arm.invalidation.extra, 1,
            "{} 的误失效只来自额外依赖",
            arm.arm
        );
        assert!(arm.invalidation.unrelated_preserved > 0, "无关结果应当保留");
    }
}

#[tokio::test]
async fn a_controlled_retraction_marks_judgments_blocks_actions_and_keeps_unrelated_results() {
    let family = FamilyA::load().expect("内嵌任务族 A 应自洽");
    let task = family.task("A5").expect("A5 是撤回证据的受控演示");
    let dir = TempDir::new("family-a-retraction");

    let trace = compare::run_single(
        &family,
        task,
        JudgeName::Rule,
        Arm::ALL[0],
        "run-retraction",
        &dir.join("calls.jsonl"),
        dir.path(),
    )
    .await
    .expect("离线单任务运行应能跑完");

    // 撤回只发生一次，产生一条失效传播记录。
    let retraction = trace
        .steps
        .iter()
        .find(|step| step.change.object().as_str() == "app.log")
        .expect("撤回 app.log v1 的那条扰动应当在轨迹里");
    assert_eq!(retraction.when, StepWhen::AfterAsk);

    // 被标记的判断：撤回发生**之前**已经落过、且读了 app.log 的两问（cause / premise）。
    let nodes_before = |question: &str, round: usize| -> Vec<u64> {
        trace
            .judgments
            .iter()
            .filter(|judgment| judgment.question.as_str() == question && judgment.round < round)
            .map(|judgment| judgment.node.0)
            .collect()
    };
    let mut marked: Vec<u64> = retraction
        .invalidation
        .stale
        .iter()
        .map(|node| node.0)
        .collect();
    marked.sort_unstable();
    let mut expected = nodes_before("cause_primary", retraction.round);
    expected.extend(nodes_before("premise_holds", retraction.round));
    expected.sort_unstable();
    assert_eq!(marked, expected, "撤回应当只标记读过 app.log 的判断");
    // 无关结果保留：指标一问读的是别的对象，不在标记里。
    for node in nodes_before("metrics_within_baseline", retraction.round) {
        assert!(!marked.contains(&node), "指标一问与这次撤回无关");
    }

    // 待执行动作：以被撤回证据为前提的补救动作被标成不能发出。
    assert!(
        retraction
            .invalidation
            .blocked
            .iter()
            .any(|action| action.as_str() == "rollback-cache"),
        "以失效判断为前提的动作应当被标出来"
    );

    // 完整传播路径：证据所在对象 → 读它的判断 → 以它为前提的行动。
    let path = retraction
        .invalidation
        .paths
        .iter()
        .find(|path| format!("{:?}", path.target).contains("rollback-cache"))
        .expect("被阻断的动作应当带一条传播路径");
    let hops: Vec<String> = path
        .hops
        .iter()
        .map(|hop| format!("{:?}", hop.kind))
        .collect();
    assert_eq!(hops, vec!["Read".to_owned(), "Premise".to_owned()]);

    // 撤回之后仍然恢复：补偿动作在后续轮次发出，独立验收通过。
    assert!(trace.acceptance.as_ref().expect("跑完必然带验收").passed);
}

#[tokio::test]
async fn the_report_carries_definitions_sources_and_the_raw_data_locations() {
    let family = FamilyA::load().expect("内嵌任务族 A 应自洽");
    let dir = TempDir::new("family-a-report");
    let outcome = {
        let options = CompareOptions {
            judges: vec![JudgeName::Rule],
            arms: vec![Arm::ALL[0]],
            run: "run-report".to_owned(),
            calls_path: dir.join("calls.jsonl"),
            replay_path: dir.join("calls.jsonl"),
            out_dir: dir.join("out"),
            command: "cargo run -p jh-app -- compare --judge rule".to_owned(),
            max_tasks: Some(2),
        };
        compare::run(&family, &options).await.expect("对照应能跑完")
    };
    assert!(outcome.clean);
    let report = outcome.report;

    let path = dir.join("report.json");
    compare::write_json(&path, &report).expect("报告应能落盘");
    let text = std::fs::read_to_string(&path).expect("报告落盘后可读");
    let wire: serde_json::Value = serde_json::from_str(&text).expect("报告是合法 JSON");

    // 口径声明、来源区分、模型版本与调用数都在报告里。
    assert!(wire["metric_definitions"]["stale_basis_action_rate"].is_string());
    assert!(wire["metric_definitions"]["input_projection"].is_string());
    assert_eq!(wire["arms"][0]["source"], serde_json::json!("rule"));
    assert_eq!(wire["arms"][0]["live"], serde_json::json!(false));
    assert_eq!(
        wire["arms"][0]["versions"][0],
        serde_json::json!("rules-v2")
    );
    assert!(wire["arms"][0]["calls"]["attempted"].as_u64().unwrap() > 0);
    assert!(wire["command"].as_str().unwrap().contains("compare"));

    // 问题集版本史：cause_primary 有 v1 与 v2，当前用 v2。
    let questions = wire["family"]["questions"].as_array().unwrap();
    let current: Vec<&serde_json::Value> = questions
        .iter()
        .filter(|entry| entry["question"] == serde_json::json!("cause_primary"))
        .filter(|entry| entry["current"] == serde_json::json!(true))
        .collect();
    assert_eq!(current.len(), 1);
    assert_eq!(current[0]["version"], serde_json::json!(2));

    // 原始数据位置：轨迹、证据日志、调用记录都是绝对路径，而且真的存在。
    let traces_dir = Path::new(report.traces_dir.as_str());
    let evidence_dir = Path::new(report.evidence_dir.as_str());
    assert!(traces_dir.is_absolute() && traces_dir.is_dir());
    assert!(evidence_dir.is_absolute() && evidence_dir.is_dir());
    assert!(Path::new(report.calls_log.as_str()).is_absolute());
    let traces = std::fs::read_dir(traces_dir).expect("轨迹目录可读").count();
    assert_eq!(traces, 2, "两个任务各一条轨迹");
    let evidence = std::fs::read_dir(evidence_dir)
        .expect("证据目录可读")
        .count();
    assert_eq!(evidence, 2, "两个任务各一份证据日志");
}

#[tokio::test]
async fn replay_reproduces_the_recorded_arm_readings() {
    let family = FamilyA::load().expect("内嵌任务族 A 应自洽");
    let dir = TempDir::new("family-a-replay");

    let recorded = run_rule_arms(&family, &dir, "run-replay-source").await;
    let options = CompareOptions {
        judges: vec![JudgeName::Replay],
        arms: Arm::ALL.to_vec(),
        run: "run-replay".to_owned(),
        calls_path: dir.join("calls.jsonl"),
        replay_path: dir.join("calls.jsonl"),
        out_dir: dir.join("out").join("run-replay"),
        command: "test compare replay".to_owned(),
        max_tasks: None,
    };
    let outcome = compare::run(&family, &options)
        .await
        .expect("回放应当能跑完（记录来自上一次运行）");
    assert!(
        outcome.clean,
        "回放复用同快照 + 同问题版本 + 同范围的记录，不该有失败"
    );

    for (recorded, replayed) in recorded.arms.iter().zip(outcome.report.arms.iter()) {
        assert_eq!(recorded.arm, replayed.arm);
        assert_eq!(
            recorded.decisions.executions, replayed.decisions.executions,
            "{}：回放应当复现同一套行动",
            recorded.arm
        );
        assert_eq!(
            recorded.acceptance.passed, replayed.acceptance.passed,
            "{}：回放应当复现同一套验收读数",
            recorded.arm
        );
        assert_eq!(
            recorded.recompute.judged_nodes, replayed.recompute.judged_nodes,
            "{}：回放的判断条数应当一致",
            recorded.arm
        );
    }
}

#[tokio::test]
async fn the_simulator_truth_judge_is_refused_for_the_family() {
    let family = FamilyA::load().expect("内嵌任务族 A 应自洽");
    let dir = TempDir::new("family-a-simulated");
    let options = CompareOptions {
        judges: vec![JudgeName::SimulatedTruth],
        arms: vec![Arm::ALL[0]],
        run: "run-simulated".to_owned(),
        calls_path: dir.join("calls.jsonl"),
        replay_path: dir.join("calls.jsonl"),
        out_dir: dir.join("out"),
        command: "test compare simulated".to_owned(),
        max_tasks: Some(1),
    };
    let report = compare::run(&family, &options)
        .await
        .expect("对照本身应当跑完（该判定器整段标未验证）")
        .report;

    // 模拟真值不参展：该（判定器 × 臂）零调用、未验证，且不计入任何读数。
    let arm = &report.arms[0];
    assert_eq!(arm.verification.status, VerificationStatus::NotVerified);
    assert!(arm.verification.detail.contains("模拟真值不参与"));
    assert_eq!(arm.calls.attempted, 0);
    assert_eq!(arm.decisions.executions, 0);
    assert_eq!(arm.acceptance.passed, 0);
}

/// 判定失败路径：失败如实落进调用记录与节点评估状态，不重试、不换后端、不当成「答了否」。
#[tokio::test]
async fn a_failing_judge_lands_in_the_call_log_and_the_node_evaluation() {
    let family = FamilyA::load().expect("内嵌任务族 A 应自洽");
    let task = family.task("A1").expect("A1 在任务族里");
    let dir = TempDir::new("family-a-failure");
    let judge = AlwaysFails;
    let mut calls = CallLog::open(dir.join("calls.jsonl")).expect("调用记录可写");

    let trace = {
        let mut context = LoopContext {
            family: &family,
            judge: &judge,
            channel: JudgeName::Jev,
            arm: Arm::ALL[0],
            run: "run-failure".to_owned(),
            endpoint: Some("https://example.invalid".to_owned()),
            calls: &mut calls,
        };
        run_task(&mut context, task, &dir.join("evidence.jsonl"))
            .await
            .expect("判定失败不该让循环崩掉（失败本身就是结果）")
    };

    // 轨迹里每次请求都带失败原文；没有任何判断记录。
    assert!(!trace.asks.is_empty());
    assert!(trace.asks.iter().all(|ask| ask.outcome != "成功"));
    assert!(trace.judgments.is_empty());
    assert!(
        trace
            .asks
            .iter()
            .all(|ask| ask.outcome.contains("桩判定器：通道不可用")),
        "失败原文要原样留下"
    );

    // 决策：手上没有答案 → 弃权，不是「前提不成立」。
    assert!(
        trace
            .decisions
            .iter()
            .all(|decision| decision.outcome == DecisionOutcome::Unresolved),
        "{:?}",
        trace.decisions
    );
    assert!(trace.executions().is_empty());

    // 独立验收不通过，且缺的是必需动作。
    let acceptance = trace.acceptance.clone().expect("跑完必然带验收结果");
    assert!(!acceptance.passed);
    assert!(
        acceptance
            .checks
            .iter()
            .any(|check| check.name == "required_actions_executed" && !check.passed)
    );

    // 调用记录：每一次请求都有一行 failure，没有 judgment 行。
    let entries = read_entries(&dir.join("calls.jsonl")).expect("调用记录可读");
    let failures = entries
        .iter()
        .filter(|entry| matches!(entry, jh_app::calls::CallEntry::Failure { .. }))
        .count();
    assert_eq!(failures, trace.asks.len());
    assert!(
        entries
            .iter()
            .all(|entry| !matches!(entry, jh_app::calls::CallEntry::Judgment { .. }))
    );
}

/// 迟到结果的落记录时机（语义版）：一轮里的每条判断看到的是同一份输入快照。
///
/// 注意：A4 第 0 轮只有一个读取组，所以「发完这一组 → 注入扰动 → 落记录」的旧实现同样会绿——
/// 判别这处修复的是 [`a_late_perturbation_does_not_leak_into_later_request_groups`]。
#[tokio::test]
async fn a_late_perturbation_marks_every_judgment_of_the_round() {
    let family = FamilyA::load().expect("内嵌任务族 A 应自洽");
    let task = family.task("A4").expect("A4 是过期结果那条");
    let dir = TempDir::new("family-a-late");

    let trace = compare::run_single(
        &family,
        task,
        JudgeName::Rule,
        Arm::ALL[0],
        "run-late",
        &dir.join("calls.jsonl"),
        dir.path(),
    )
    .await
    .expect("离线单任务运行应能跑完");

    // A4 第 0 轮的提问后扰动：这一轮问过的问题都要带上过期标记。
    let drift = trace
        .steps
        .iter()
        .find(|step| step.when == StepWhen::AfterAsk)
        .expect("A4 有提问后扰动");
    let round_zero: Vec<&jh_app::matching::JudgmentTrace> = trace
        .judgments
        .iter()
        .filter(|judgment| judgment.round == drift.round)
        .collect();
    assert!(!round_zero.is_empty());
    assert!(
        round_zero.iter().all(|judgment| judgment.drift),
        "同一轮的迟到结果应当一起落成待重查：{:?}",
        round_zero
            .iter()
            .map(|judgment| (&judgment.question, judgment.drift))
            .collect::<BTreeMap<_, _>>()
    );
    // 提问相位早于扰动相位：判断看到的是变化前的材料。
    assert!(
        round_zero
            .iter()
            .all(|judgment| judgment.phase < drift.phase)
    );
}

/// 迟到结果的落记录时机（判别版）：一轮里有多个读取组时，提问后扰动不得漏进后面的组。
///
/// 全量臂下 A5 第 1 轮会把两个读取组（`app.log`+`config`、`runtime.metrics`+`config`）都问一遍，
/// 撤回提在两组之间注入。修复前的实现是「发一组 → 注入 → 落记录」，第二组因此是在扰动之后才装配与
/// 记录的（它的 ask/judgment 相位等于扰动相位）；修复后两组共用同一个提问相位，材料都取扰动前版本。
/// 本测试在 `e337f8e` 之前的代码上会失败（第二组相位 = 扰动相位）。
#[tokio::test]
async fn a_late_perturbation_does_not_leak_into_later_request_groups() {
    let family = FamilyA::load().expect("内嵌任务族 A 应自洽");
    let task = family.task("A5").expect("A5 是撤回证据那条");
    let dir = TempDir::new("family-a-late-groups");

    let trace = compare::run_single(
        &family,
        task,
        JudgeName::Rule,
        Arm::ALL[2], // 全量臂：这一轮会把全部跟踪问题重问一遍（两个读取组）
        "run-late-groups",
        &dir.join("calls.jsonl"),
        dir.path(),
    )
    .await
    .expect("离线单任务运行应能跑完");

    let retraction = trace
        .steps
        .iter()
        .find(|step| step.when == StepWhen::AfterAsk && step.change.object().as_str() == "app.log")
        .expect("A5 有撤回扰动");
    let round = retraction.round;
    let ask_phase = task.ask_phase(round);

    let asks: Vec<&jh_app::matching::AskTrace> =
        trace.asks.iter().filter(|ask| ask.round == round).collect();
    assert!(
        asks.len() >= 2,
        "全量臂这一轮应当有多个读取组：{:?}",
        asks.iter().map(|ask| ask.scope.clone()).collect::<Vec<_>>()
    );
    for ask in &asks {
        assert_eq!(
            ask.phase, ask_phase,
            "本轮每组请求都应当在同一个提问相位发出：{:?}",
            ask.scope
        );
        assert!(
            ask.phase < retraction.phase,
            "提问必须早于提问后的扰动：{:?}",
            ask.scope
        );
    }

    // 第二个读取组（runtime.metrics）的判断同样落在提问相位上——旧实现下它会等于扰动相位。
    let metrics = jh_core::QuestionId::from("metrics_within_baseline");
    let late = trace
        .judgments
        .iter()
        .find(|judgment| judgment.round == round && judgment.question == metrics)
        .expect("全量臂这一轮会问指标一问");
    assert_eq!(late.phase, ask_phase);
    assert!(late.phase < retraction.phase);

    // 撤回只波及读了 app.log 的那一组：它落成待重查，指标一问不受影响。
    let cause = jh_core::QuestionId::from("cause_primary");
    let touched = trace
        .judgments
        .iter()
        .find(|judgment| judgment.round == round && judgment.question == cause)
        .expect("全量臂这一轮会问原因一问");
    assert!(touched.drift, "读了被撤回对象的判断要落成待重查");
    assert!(!late.drift, "读别的对象的判断不受这次撤回影响");
}
