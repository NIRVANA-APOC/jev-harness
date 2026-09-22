//! 离线端到端：初始任务集 × 规则 / 录制 / 模拟真值判断器 → 调用记录 + JSON 报告。
//!
//! 这些测试不发任何网络请求。真调 smoke 在 `tests/live_smoke.rs` 里，默认 `#[ignore]`。

mod common;

use jh_app::calls::{CallEntry, read_entries};
use jh_app::exp::{self, RunOptions};
use jh_app::judge::JudgeName;
use jh_app::report;
use jh_app::tasks::TaskSet;
use jh_core::JudgeSource;

use common::TempDir;

fn options(dir: &TempDir, run: &str, judges: Vec<JudgeName>) -> RunOptions {
    RunOptions {
        judges,
        run: run.to_owned(),
        calls_path: dir.join("calls.jsonl"),
        replay_path: dir.join("calls.jsonl"),
        command: format!("cargo test -p jh-app --test initial_task_set（{run}）"),
    }
}

#[tokio::test]
async fn the_offline_judges_run_the_initial_task_set() {
    let dir = TempDir::new("offline");
    let task_set = TaskSet::load_initial().expect("内嵌任务集应自洽");
    let options = options(
        &dir,
        "run-offline",
        vec![JudgeName::Rule, JudgeName::SimulatedTruth],
    );

    let outcome = exp::run(&task_set, &options)
        .await
        .expect("离线运行应能跑完");
    assert!(outcome.clean, "离线判断器不应有失败");
    assert_eq!(outcome.report.judges.len(), 2);
    assert_eq!(outcome.report.task_set.cases, 4);
    assert_eq!(outcome.report.task_set.questions, 3);
    assert_eq!(outcome.report.run, "run-offline");
    assert!(outcome.report.command.contains("initial_task_set"));

    let cases = task_set.cases.len() as u32;
    let judgments = task_set.expected_judgments() as u32;

    // 规则判断器：与样本配套，合成集上一致率满分是预期读数（真实样本上不成立）。
    let rule = &outcome.report.judges[0];
    assert_eq!(
        (rule.judge.as_str(), rule.source),
        ("rule", JudgeSource::Rule)
    );
    assert_eq!(rule.calls.attempted, cases);
    assert_eq!(rule.calls.succeeded, cases);
    assert_eq!(rule.calls.failed, 0);
    assert_eq!(rule.versions, vec!["rules-v1".to_owned()]);
    assert_eq!(rule.quality.judgments, judgments);
    assert_eq!(rule.quality.agreements, judgments);
    assert_eq!(rule.quality.agreement_rate, Some(1.0));
    assert_eq!(rule.quality.per_question.len(), 3);
    assert!(
        rule.quality
            .brier_mean
            .is_some_and(|value| value > 0.0 && value < 0.1),
        "规则分布是未校准常量，Brier 应是小正数：{:?}",
        rule.quality.brier_mean
    );
    assert!(
        rule.quality.log_loss_mean.is_some(),
        "规则给真值标签的概率非零，对数损失应有定义"
    );
    assert!(rule.latency_ms.p50.is_some() && rule.latency_ms.p95.is_some());
    assert_eq!(rule.latency_ms.samples, cases);
    assert_eq!(rule.usage.calls_with_usage, 0, "本地判断器没有 token 用量");
    assert_eq!(rule.usage.input_tokens, None);

    // 模拟真值：它掌握真值，读数必须是完美的上界。
    let truth = &outcome.report.judges[1];
    assert_eq!(truth.source, JudgeSource::SimulatedTruth);
    assert_eq!(truth.quality.agreement_rate, Some(1.0));
    assert_eq!(truth.quality.brier_mean, Some(0.0));
    assert_eq!(truth.quality.log_loss_mean, Some(0.0));
    assert_eq!(truth.quality.boundary_readings, 0);

    // 调用记录：一次判断一行，来源标签与延迟都在里面。
    let entries = read_entries(&options.calls_path).expect("调用记录应可读");
    assert_eq!(entries.len(), 2 * cases as usize);
    let sources: Vec<JudgeSource> = entries
        .iter()
        .filter_map(|entry| match entry {
            CallEntry::Judgment { record, .. } => Some(record.source),
            CallEntry::Failure { .. } => None,
        })
        .collect();
    assert!(sources.contains(&JudgeSource::Rule));
    assert!(sources.contains(&JudgeSource::SimulatedTruth));
    assert!(
        entries.iter().all(|entry| match entry {
            CallEntry::Judgment { record, .. } => record.judgments.len() == 3,
            CallEntry::Failure { .. } => false,
        }),
        "每个样本的判断记录应含三个问题"
    );
}

#[tokio::test]
async fn replay_reuses_recorded_answers_and_keeps_its_own_source_label() {
    let dir = TempDir::new("replay");
    let task_set = TaskSet::load_initial().expect("内嵌任务集应自洽");

    // 第一次：规则判断器落记录。
    let rule_run = options(&dir, "run-rule", vec![JudgeName::Rule]);
    let first = exp::run(&task_set, &rule_run)
        .await
        .expect("规则运行应能跑完");
    assert!(first.clean);

    // 第二次：回放第一次的记录（同一份 JSONL）。
    let replay_run = options(&dir, "run-replay", vec![JudgeName::Replay]);
    let second = exp::run(&task_set, &replay_run)
        .await
        .expect("回放运行应能跑完");

    assert!(second.clean, "记录齐全时回放不应失败");
    let replay = &second.report.judges[0];
    assert_eq!(
        (replay.judge.as_str(), replay.source),
        ("replay", JudgeSource::Replay)
    );
    assert_eq!(
        replay.versions,
        vec!["rules-v1".to_owned()],
        "回放保留原记录的执行者版本"
    );
    assert_eq!(replay.quality.agreement_rate, Some(1.0));
    assert_eq!(replay.usage.calls_with_usage, 0, "回放不产生 token 用量");

    let entries = read_entries(&replay_run.calls_path).expect("调用记录应可读");
    let replay_records = entries
        .iter()
        .filter(|entry| matches!(entry, CallEntry::Judgment { record, .. } if record.source == JudgeSource::Replay))
        .count();
    assert_eq!(replay_records, task_set.cases.len());
}

#[tokio::test]
async fn replay_with_a_missing_file_is_marked_not_verified_and_does_not_borrow_answers() {
    let dir = TempDir::new("no-records");
    let task_set = TaskSet::load_initial().expect("内嵌任务集应自洽");
    let mut run = options(
        &dir,
        "run-no-records",
        vec![JudgeName::Replay, JudgeName::Rule],
    );
    run.replay_path = dir.join("never-written.jsonl");

    let outcome = exp::run(&task_set, &run)
        .await
        .expect("判断器不可用也应产出报告");

    assert!(!outcome.clean, "有判断器没跑通时运行不算干净");
    let replay = &outcome.report.judges[0];
    assert_eq!(replay.calls.attempted, 0, "构造失败就没有调用发生");
    assert_eq!(replay.calls.succeeded, 0);
    assert_eq!(replay.quality.judgments, 0);
    assert_eq!(
        replay.verification.status,
        report::VerificationStatus::NotVerified
    );
    assert!(
        replay.verification.detail.contains("回放记录不存在"),
        "{}",
        replay.verification.detail
    );

    // 另选的规则判断器照常跑完：回放不可用没有被别的判断器顶替。
    let rule = &outcome.report.judges[1];
    assert_eq!(rule.calls.succeeded, task_set.cases.len() as u32);
}

#[tokio::test]
async fn replay_over_an_empty_log_fails_every_call_without_borrowing_answers() {
    let dir = TempDir::new("empty-log");
    let task_set = TaskSet::load_initial().expect("内嵌任务集应自洽");
    // 调用记录与回放文件是同一个路径：CallLog 先建出空文件，回放因此没有可复用的记录。
    let run = options(&dir, "run-empty-log", vec![JudgeName::Replay]);

    let outcome = exp::run(&task_set, &run)
        .await
        .expect("判断器不可用也应产出报告");

    assert!(!outcome.clean);
    let replay = &outcome.report.judges[0];
    assert_eq!(replay.calls.attempted, task_set.cases.len() as u32);
    assert_eq!(replay.calls.succeeded, 0);
    assert_eq!(replay.calls.failed, replay.calls.attempted);
    assert_eq!(replay.quality.judgments, 0, "失败调用不产出任何判分");
    assert_eq!(replay.usage.calls_with_usage, 0);
    assert_eq!(
        replay.verification.status,
        report::VerificationStatus::NotVerified
    );

    // 每次失败都落了失败行：来源标签仍是回放，错误文本保留原因。
    let entries = read_entries(&run.calls_path).expect("调用记录应可读");
    assert_eq!(entries.len(), task_set.cases.len());
    for entry in &entries {
        let CallEntry::Failure { source, error, .. } = entry else {
            panic!("空记录文件不应产生判断行：{entry:?}");
        };
        assert_eq!(*source, JudgeSource::Replay);
        assert!(error.contains("没有可回放的记录"), "{error}");
    }
}

#[tokio::test]
async fn the_report_is_json_with_definitions_locations_and_reproduction_command() {
    let dir = TempDir::new("report");
    let task_set = TaskSet::load_initial().expect("内嵌任务集应自洽");
    let options = options(&dir, "run-report", vec![JudgeName::SimulatedTruth]);

    let outcome = exp::run(&task_set, &options)
        .await
        .expect("离线运行应能跑完");
    let path = dir.join("var/exp/run-report.json");
    report::write(&path, &outcome.report).expect("报告应能落盘");

    let text = std::fs::read_to_string(&path).expect("报告应可读");
    let wire: serde_json::Value = serde_json::from_str(&text).expect("报告应是合法 JSON");

    assert_eq!(wire["run"], serde_json::json!("run-report"));
    assert!(
        wire["generated_at_unix_ms"]
            .as_u64()
            .is_some_and(|ms| ms > 0)
    );
    assert!(
        wire["calls_log"]
            .as_str()
            .is_some_and(|path| path.starts_with('/'))
    );
    assert_eq!(
        wire["calls_log"],
        serde_json::json!(options.calls_path.display().to_string())
    );
    assert!(!wire["notes"].as_array().expect("notes 是数组").is_empty());

    let definitions = &wire["metric_definitions"];
    for key in [
        "truth",
        "agreement",
        "brier",
        "log_loss",
        "score_mae",
        "latency",
        "confidence",
        "cost",
    ] {
        assert!(
            definitions[key]
                .as_str()
                .is_some_and(|text| !text.is_empty()),
            "口径 `{key}` 应有非空文本：{definitions}"
        );
    }

    let judge = &wire["judges"][0];
    assert_eq!(judge["source"], serde_json::json!("simulated_truth"));
    assert_eq!(judge["live"], serde_json::json!(false));
    assert_eq!(
        judge["verification"]["status"],
        serde_json::json!("simulated")
    );
    assert_eq!(judge["quality"]["judgments"], serde_json::json!(12));
    assert_eq!(judge["latency_ms"]["samples"], serde_json::json!(4));
    assert_eq!(judge["usage"]["input_tokens"], serde_json::Value::Null);
    assert!(
        judge["quality"]["per_question"]
            .as_array()
            .expect("按问题分列的读数")
            .iter()
            .all(|question| question["judgments"] == serde_json::json!(4))
    );
}
