//! M4 的离线端到端测试：本地判定器 + 预置契约产物把整条链路跑完，不碰网络。
//!
//! 真调版本见
//! `OPENAI_MODEL=deepseek-flash OPENAI_BASE_URL=https://api.deepseek.com OPENAI_API_KEY=... \
//! cargo run -p jh-app -- m4`。
//!
//! 这里钉住的是口径与结构，而不是会被调参改变的那部分：四组臂只切所属维度、诊断对照把证据差异抹平、
//! 同结构下的两个判断通道读到逐字节相同的材料、固定频率确实改掉了重查时机、没有可用通道的族如实
//! 记成失败态（不折成「成立」）。
//!
//! 预置契约的写法抄自 `tests/h4.rs`（同一份协议核对口径），避免两处契约漂移。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicUsize, Ordering};

use jh_app::c_measure::{self, GeneratedContract, MeasureCall};
use jh_app::c_program::{self, CFamily, ProgramVersion};
use jh_app::judge::JudgeName;
use jh_app::m4::{self, Arm, M4Options};
use jh_app::matching::Cadence;
use jh_core::{Measurement, ObserverContract, OutputContract, Requirement, Timing};
use serde_json::json;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let unique = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path =
            std::env::temp_dir().join(format!("jh-app-m4-{name}-{}-{unique}", process::id()));
        std::fs::create_dir_all(&path).expect("临时目录可建");
        Self(path)
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

/// 一份合用的契约（写法与 `tests/h4.rs` 的 fixture 一致）。
fn contract_of(family: CFamily) -> ObserverContract {
    let proposition = match family {
        CFamily::CancelAfterConfirm => {
            "同一任务在收到取消确认之后是否还产生写入（确认事件与写入事件的先后）"
        }
        CFamily::LostUpdate => {
            "同一键上的两次提交是否都以同一个基准版本为依据（是否有已提交的更新被覆盖）"
        }
        CFamily::DuplicateAfterRestart => {
            "同一任务的写入意图在重启重放之后被提交了几次（次数是否超过一次）"
        }
    };
    ObserverContract {
        requirement: Requirement {
            text: family.requirement().to_owned(),
            source: family.requirement_source().to_owned(),
        },
        proposition: proposition.to_owned(),
        coverage: "覆盖这一族声明的顺序违反；不覆盖写入内容的业务正确性".to_owned(),
        measurement: Measurement {
            site: "write_boundary + pre_write_pause + commit_counter".to_owned(),
            objects: BTreeSet::from([family.object()]),
            timing: Timing::Sequence,
        },
        support: jh_core::Support {
            versions: BTreeSet::from([ProgramVersion::V1.id().to_owned()]),
            configuration: "recorder=on".to_owned(),
            inputs: "同一任务的写入事件与暂停点事件".to_owned(),
            environment: "隔离实验环境，含可注入的暂停点".to_owned(),
        },
        disturbance: jh_core::Disturbance {
            side_effects: "追加写记录文件，不改被测数据".to_owned(),
            performance: "每事件一次追加写".to_owned(),
            scheduling: "在写入边界同步记录".to_owned(),
        },
        controls: jh_core::Controls {
            positive: "合理实现".to_owned(),
            negative: "已知错误实现".to_owned(),
            injected_fault: "在确认之后注入一次写入".to_owned(),
        },
        output: OutputContract {
            channel: c_program::RECORDER_CHANNEL.to_owned(),
            format: "每条写入事件一条证据（任务、键、事件序号、提交序号）".to_owned(),
            failure: "记录器被绕过（提交计数器与记录到的事件数不符）时给出失败状态".to_owned(),
            invalid_when: "记录器被绕过、被篡改或事件序号不可比".to_owned(),
        },
        uncovered: family.uncovered().to_owned(),
    }
}

fn call() -> MeasureCall {
    MeasureCall {
        model: Some("preloaded".to_owned()),
        request_id: Some("preloaded-contract".to_owned()),
        usage: None,
        input_bytes: 0,
        output_bytes: 0,
        latency_ms: 0,
        error: None,
    }
}

/// 预置契约产物：三族各一份，全部通过协议核对。
fn contracts() -> Vec<GeneratedContract> {
    CFamily::ALL
        .into_iter()
        .map(|family| {
            let contract = contract_of(family);
            let check = c_measure::check_contract(&contract, family, ProgramVersion::V1);
            assert!(check.passed, "{family} 的预置契约该通过核对：{check:?}");
            GeneratedContract {
                family,
                call: call(),
                contract: Some(contract),
                check: Some(check),
                rejection: None,
            }
        })
        .collect()
}

fn write<T: serde::Serialize>(path: &Path, value: &T) {
    std::fs::write(path, serde_json::to_string(value).expect("可序列化")).expect("可写");
}

/// 小规模的离线运行：两臂两族 × 1 次重复、族 C 每族 1 条调度、密度轴两条节奏两个规模。
async fn run_offline(
    dir: &TempDir,
    arms: Vec<Arm>,
    entries: Vec<GeneratedContract>,
    ablate_reuse: bool,
) -> m4::M4Outcome {
    let contracts_path = dir.join("contracts.json");
    write(&contracts_path, &entries);
    let options = M4Options {
        run: "m4-offline".to_owned(),
        calls_path: dir.join("calls.jsonl"),
        out_dir: dir.join("run"),
        command: "cargo test -p jh-app --test m4".to_owned(),
        arms,
        repeats: 1,
        max_tasks: Some(2),
        max_schedules: Some(1),
        cadences: vec![Cadence::Triggered, Cadence::FixedEvery(3)],
        scales: vec![2],
        native: false,
        ablate_reuse,
        channel_override: Some(JudgeName::Rule),
        contracts: Some(contracts_path),
    };
    m4::run(&options).await.expect("离线 M4 应跑到底")
}

fn cell<'a>(cells: &'a [m4::LoopCell], arm: &str, family: &str) -> &'a m4::LoopCell {
    cells
        .iter()
        .find(|cell| cell.arm == arm && cell.family == family)
        .unwrap_or_else(|| {
            panic!(
                "缺 {arm} × 族 {family} 的读数：{:?}",
                cells
                    .iter()
                    .map(|cell| format!("{}|{}", cell.arm, cell.family))
                    .collect::<Vec<String>>()
            )
        })
}

#[tokio::test]
async fn the_two_by_two_reports_both_dimensions_for_both_families() {
    let dir = TempDir::new("matrix");
    let outcome = run_offline(&dir, Arm::ALL.to_vec(), contracts(), false).await;
    let report = &outcome.report;

    assert_eq!(report.main.len(), 8, "四组臂 × 两族");
    assert_eq!(report.diagnostic.len(), 8, "诊断对照单独报告");
    let structures: BTreeSet<&str> = report
        .main
        .iter()
        .map(|cell| cell.structure.as_str())
        .collect();
    assert_eq!(structures, BTreeSet::from(["loop", "maintained"]));
    let judges: BTreeSet<&str> = report.main.iter().map(|cell| cell.judge.as_str()).collect();
    assert_eq!(judges, BTreeSet::from(["llm", "jev"]));

    // 结构维在循环上落到 H1 / H2 两个开关：循环式控制=不传播+全量，依赖维护=传播+增量。
    assert_eq!(cell(&report.main, "loop-llm", "A").loop_arm, "h1off-h2full");
    assert_eq!(
        cell(&report.main, "maintained-llm", "A").loop_arm,
        "h1on-h2inc"
    );

    // 每格都有读数，且验收分母与任务数一致。
    for read in &report.main {
        assert_eq!(read.readings.tasks, 2);
        assert!(read.readings.acceptance_passed <= read.readings.tasks);
        assert!(read.calls.attempted > 0, "{} 该有判断调用", read.arm);
    }

    // 只切一个维度的比较：两族 ×（两通道 + 两结构）各一条。
    assert_eq!(report.contrasts.len(), 8);
    assert!(
        report
            .contrasts
            .iter()
            .all(|pair| pair.note.contains("交互作用"))
    );
}

#[tokio::test]
async fn the_equalized_diagnostic_flattens_the_evidence_difference() {
    let dir = TempDir::new("equalized");
    let outcome = run_offline(&dir, Arm::ALL.to_vec(), contracts(), false).await;
    let report = &outcome.report;

    for family in ["A", "B"] {
        // 循环式控制本来就是全量重问：诊断对照与主对照的询问量逐格相同。
        let loop_main = cell(&report.main, "loop-llm", family);
        let loop_diag = cell(&report.diagnostic, "loop-llm", family);
        assert_eq!(
            loop_main.readings.asks, loop_diag.readings.asks,
            "族 {family}：循环臂两段的询问量该相同"
        );
        assert_eq!(loop_diag.evidence, "equalized");

        // 依赖维护臂在诊断对照里被推成全量：重查次数不低于主对照。
        let maintained_main = cell(&report.main, "maintained-llm", family);
        let maintained_diag = cell(&report.diagnostic, "maintained-llm", family);
        assert!(
            maintained_diag.readings.asks >= maintained_main.readings.asks,
            "族 {family}：诊断对照的依赖维护臂该重查不少于主对照（{} vs {}）",
            maintained_diag.readings.asks,
            maintained_main.readings.asks
        );
        assert!(maintained_diag.readings.cache_reuse_rate.unwrap_or(0.0) < 1.0);
    }
}

#[tokio::test]
async fn one_judge_channel_change_moves_only_the_same_material() {
    let dir = TempDir::new("channel");
    let outcome = run_offline(&dir, Arm::ALL.to_vec(), contracts(), false).await;
    let report = &outcome.report;

    assert!(!report.channel.is_empty());
    for family in CFamily::ALL {
        let loop_cells: Vec<&m4::ChannelCell> = report
            .channel
            .iter()
            .filter(|cell| cell.family == family.id() && cell.structure == "loop")
            .collect();
        let maintained: Vec<&m4::ChannelCell> = report
            .channel
            .iter()
            .filter(|cell| cell.family == family.id() && cell.structure == "maintained")
            .collect();
        assert_eq!(
            loop_cells.len(),
            2,
            "{} 该有循环式控制的两个通道",
            family.id()
        );
        assert_eq!(
            maintained.len(),
            2,
            "{} 该有依赖维护的两个通道",
            family.id()
        );

        // 同一结构下的两个判断通道读到的材料逐字节相同：判断器差异与证据差异分开。
        assert_eq!(loop_cells[0].material_bytes, loop_cells[1].material_bytes);
        assert_eq!(maintained[0].material_bytes, maintained[1].material_bytes);
        assert!(loop_cells[0].material_bytes > 0);
        assert!(maintained[0].material_bytes > loop_cells[0].material_bytes);
        assert_eq!(loop_cells[0].cases, 4, "1 条调度 × 4 路实现");
        assert_eq!(loop_cells[0].opportunities + loop_cells[0].clean_cases, 4);
        // 本测试用本地规则判定器替代两个通道，而规则表里没有 `m4_requirement_holds` 这条问题：
        // 两次判读都显式失败（不是静默折成「成立」），整个族据此标「未验证」。
        // 这条读数本身就是「本地判定器不许冒充真实通道」的证据。
        assert_eq!(maintained[0].failures, 4, "{}", family.id());
        assert_eq!(maintained[1].failures, 4, "{}", family.id());
        assert_eq!(
            maintained[0].verification.status,
            jh_app::report::VerificationStatus::NotVerified
        );
    }

    // 通道的建立过程逐族留档，且自证式的伪检查都被门槛拒了。
    assert_eq!(report.channel_setup.len(), 3);
    assert!(report.channel_setup.iter().all(|setup| setup.accepted));
    assert!(
        report
            .channel_setup
            .iter()
            .all(|setup| setup.self_certifying_rejected)
    );
    assert!(
        report
            .channel_setup
            .iter()
            .all(|setup| setup.enabled_version == Some(1))
    );
}

#[tokio::test]
async fn the_fixed_cadence_changes_when_the_runner_checks() {
    let dir = TempDir::new("cadence");
    let outcome = run_offline(&dir, Arm::ALL.to_vec(), contracts(), false).await;
    let report = &outcome.report;

    let triggered = report
        .density
        .iter()
        .find(|cell| cell.cadence == "triggered")
        .expect("密度扫描该有依赖触发的格");
    let every_three = report
        .density
        .iter()
        .find(|cell| cell.cadence == "every-3")
        .expect("密度扫描该有 every-3 的格");
    assert_eq!(triggered.arm, every_three.arm);
    assert_eq!(triggered.scale, every_three.scale);
    assert!(
        every_three.readings.asks <= triggered.readings.asks,
        "固定频率每 3 轮才查一次，询问量不该更多（{} vs {}）",
        every_three.readings.asks,
        triggered.readings.asks
    );
    assert!(triggered.readings.judgments > every_three.readings.judgments);

    // H5 判据在跑之前写死，跑后只填读数与布尔值。
    assert_eq!(report.h5.criteria.len(), 2);
    assert!(!report.h5.readings.is_empty());
    assert!(report.h5.conclusion.contains("本环境先导"));
}

#[tokio::test]
async fn the_reuse_ablation_reports_both_sides_on_the_same_cases() {
    let dir = TempDir::new("reuse");
    let outcome = run_offline(&dir, Arm::ALL.to_vec(), contracts(), true).await;
    let report = &outcome.report;

    assert!(report.reuse.enabled);
    let reuse = report.reuse.reuse.as_ref().expect("复用一侧");
    let rebuild = report.reuse.rebuild.as_ref().expect("逐用例重建一侧");
    assert_eq!(reuse.cases, rebuild.cases);
    assert!(
        reuse.generation_calls <= rebuild.generation_calls,
        "复用侧建的通道次数不该多于逐用例重建（{} vs {}）",
        reuse.generation_calls,
        rebuild.generation_calls
    );
    assert!(report.reuse.curve.len() >= 2);
    assert!(
        report
            .reuse
            .curve
            .iter()
            .all(|point| point.cases_covered > 0)
    );
}

#[tokio::test]
async fn a_rejected_contract_leaves_that_family_without_a_channel() {
    let dir = TempDir::new("rejected");
    let mut entries = contracts();
    for entry in &mut entries {
        if entry.family == CFamily::CancelAfterConfirm {
            let mut contract = contract_of(CFamily::CancelAfterConfirm);
            contract.proposition = c_measure::BANNED_SUBSTITUTIONS[0].to_owned();
            let check = c_measure::check_contract(
                &contract,
                CFamily::CancelAfterConfirm,
                ProgramVersion::V1,
            );
            assert!(!check.passed, "被禁的替换该判不通过");
            entry.contract = Some(contract);
            entry.check = Some(check);
        }
    }
    let outcome = run_offline(&dir, Arm::ALL.to_vec(), entries, false).await;
    let report = &outcome.report;

    let rejected = report
        .channel_setup
        .iter()
        .find(|setup| setup.family == "cancel_after_confirm")
        .expect("该族有建立记录");
    assert!(!rejected.accepted);
    assert!(!rejected.enabled);
    assert!(rejected.rejection.is_some());

    // 没有可用通道时，依赖维护的两个通道在这个族上是失败态（弃权），不折成「成立」。
    let maintained: Vec<&m4::ChannelCell> = report
        .channel
        .iter()
        .filter(|cell| cell.family == "cancel_after_confirm" && cell.structure == "maintained")
        .collect();
    assert_eq!(maintained.len(), 2);
    for read in maintained {
        assert!(read.failures > 0);
        assert_eq!(read.detections, 0);
        assert_eq!(read.false_passes, 0);
    }
    assert!(!outcome.clean, "被拒契约要让退出码非 0");
}

#[tokio::test]
async fn the_report_carries_the_frozen_protocol_and_its_definitions() {
    let dir = TempDir::new("protocol");
    let outcome = run_offline(&dir, vec![Arm::ALL[0]], contracts(), false).await;
    let report = &outcome.report;

    assert_eq!(report.protocol.arms.len(), 1);
    assert_eq!(report.protocol.dimensions.len(), 2);
    assert_eq!(report.protocol.evidence_modes.len(), 2);
    assert_eq!(report.protocol.ablations.len(), 2);
    assert!(
        report
            .protocol
            .metric_definitions
            .contains_key("acceptance")
    );
    assert!(
        report
            .protocol
            .metric_definitions
            .contains_key("hidden_truth")
    );
    assert_eq!(report.notes.len(), m4::declared_notes().len());
    assert!(report.raw.out_dir.starts_with('/'));
    assert!(report.raw.calls_log.ends_with("calls.jsonl"));
    assert_eq!(report.task_set.families.len(), 3);
    assert!(report.task_set.merged_tasks >= 4 + 12);

    let wire = serde_json::to_value(report).expect("报告可序列化");
    for key in [
        "protocol",
        "main",
        "diagnostic",
        "density",
        "channel",
        "channel_setup",
        "reuse",
        "h5",
        "contrasts",
        "cost",
        "raw",
    ] {
        assert!(wire.get(key).is_some(), "报告缺 `{key}`");
    }
    assert!(wire["main"][0]["structure"].as_str().is_some());
    assert!(wire["h5"]["improves_quality"].is_boolean());
    assert_eq!(
        wire["protocol"]["arms"][0]["loop_arm"],
        json!("h1off-h2full")
    );

    let lines = report.summary_lines();
    assert!(lines.iter().any(|line| line.starts_with("主对照｜")));
    assert!(lines.iter().any(|line| line.starts_with("H5｜")));
    assert!(lines.iter().any(|line| line.starts_with("合计：")));
}
