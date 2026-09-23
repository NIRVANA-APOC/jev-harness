//! H4 的离线端到端测试：用**预置契约与预置测试产物**跑完整条链路，不碰网络。
//!
//! 真调版本见
//! `OPENAI_MODEL=deepseek-flash OPENAI_BASE_URL=https://api.deepseek.com OPENAI_API_KEY=... \
//! cargo run -p jh-app -- h4`。
//!
//! 这里钉住的是口径，不是会被调参改变的那部分：三个臂看的是什么、真值从哪来、被禁的替换在两个
//! 方向上都判错、观测器不迁移、便宜维护省掉了什么。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicUsize, Ordering};

use jh_app::c_measure::{
    self, BANNED_SUBSTITUTIONS, GeneratedContract, GeneratedTests, MeasureCall, RawCheck, RawPlan,
};
use jh_app::c_program::{self, CFamily, ProgramVersion};
use jh_app::h4::{
    self, ARM_FIXED, ARM_GENERATED, ARM_GENERATED_V2, ARM_OBSERVER, ARM_OBSERVER_V2_NO_COUNTER,
    ARM_OBSERVER_V2_REVERIFIED, ARM_OBSERVER_V2_SUPPORT_ONLY, ArtifactSource, H4Options,
};
use jh_core::{Measurement, ObserverContract, OutputContract, Requirement, Timing};
use serde_json::json;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let unique = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path =
            std::env::temp_dir().join(format!("jh-app-h4-{name}-{}-{unique}", process::id()));
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

/// 一份合用的契约：三条钩子点全选，命题按族的顺序语义写。
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

fn call(kind: &str) -> MeasureCall {
    MeasureCall {
        model: Some("preloaded".to_owned()),
        request_id: Some(format!("preloaded-{kind}")),
        usage: None,
        input_bytes: 0,
        output_bytes: 0,
        latency_ms: 0,
        error: None,
    }
}

/// 预置的契约产物：三族各一份，全部通过协议核对。
fn contracts() -> Vec<GeneratedContract> {
    CFamily::ALL
        .into_iter()
        .map(|family| {
            let contract = contract_of(family);
            let check = c_measure::check_contract(&contract, family, ProgramVersion::V1);
            assert!(check.passed, "{family} 的预置契约该通过核对：{check:?}");
            GeneratedContract {
                family,
                call: call("contract"),
                contract: Some(contract),
                check: Some(check),
                rejection: None,
            }
        })
        .collect()
}

/// 预置的测试计划：只用程序自报的可观察量——这是「通用测试生成」在能看到的范围内写得最好的样子。
fn tests() -> Vec<GeneratedTests> {
    let plans = [
        (
            CFamily::CancelAfterConfirm,
            vec![
                RawCheck {
                    assertion: "status_equals".to_owned(),
                    task: Some("T1".to_owned()),
                    key: None,
                    expected: "cancelled".to_owned(),
                },
                RawCheck {
                    assertion: "value_equals".to_owned(),
                    task: None,
                    key: Some(c_program::task_key("T1")),
                    expected: String::new(),
                },
            ],
            "自报状态 + 该任务的键在终态里为空",
        ),
        (
            CFamily::LostUpdate,
            vec![RawCheck {
                assertion: "value_equals".to_owned(),
                task: None,
                key: Some(c_program::COUNTER_KEY.to_owned()),
                expected: "2".to_owned(),
            }],
            "计数器的终态等于两条自增链的结果",
        ),
        (
            CFamily::DuplicateAfterRestart,
            vec![RawCheck {
                assertion: "intents_equals".to_owned(),
                task: Some("T1".to_owned()),
                key: None,
                expected: "1".to_owned(),
            }],
            "自报的写入意图数是 1",
        ),
    ];
    plans
        .into_iter()
        .map(|(family, checks, note)| {
            let plan = RawPlan {
                family: family.id().to_owned(),
                checks,
                note: note.to_owned(),
            };
            let compiled = c_measure::compile_plan(&plan, family).expect("预置计划可编译");
            GeneratedTests {
                family,
                call: call("tests"),
                plan: Some(plan),
                checks: compiled,
                rejection: None,
            }
        })
        .collect()
}

fn write<T: serde::Serialize>(path: &Path, value: &T) {
    std::fs::write(path, serde_json::to_string(value).expect("可序列化")).expect("可写");
}

async fn run_offline(dir: &TempDir) -> h4::H4Outcome {
    let contracts_path = dir.join("contracts.json");
    let tests_path = dir.join("tests.json");
    write(&contracts_path, &contracts());
    write(&tests_path, &tests());
    let options = H4Options {
        run: "h4-offline".to_owned(),
        calls_path: dir.join("calls.jsonl"),
        out_dir: dir.join("run"),
        command: "cargo test -p jh-app --test h4".to_owned(),
        contracts: ArtifactSource::File(contracts_path),
        tests: ArtifactSource::File(tests_path),
        repeats: None,
    };
    h4::run(&options).await.expect("离线 H4 应跑到底")
}

fn arm<'a>(report: &'a h4::H4Report, name: &str, family: &str, version: &str) -> &'a h4::ArmReport {
    report
        .arms
        .iter()
        .find(|entry| entry.arm == name && entry.family == family && entry.version == version)
        .unwrap_or_else(|| panic!("臂 {name} × {family} × {version} 应当在报告里"))
}

#[tokio::test]
async fn the_whole_three_arm_chain_runs_offline_and_the_report_is_json() {
    let dir = TempDir::new("chain");
    let outcome = run_offline(&dir).await;
    assert!(outcome.clean, "预置产物不该有失败的调用");
    let report = &outcome.report;

    // 三个臂 × 三个族 × v1 全在报告里；每臂每族的用例数 = 10 条调度 × 4 路实现。
    for family in CFamily::ALL {
        for name in [ARM_FIXED, ARM_GENERATED, ARM_OBSERVER] {
            let entry = arm(report, name, family.id(), ProgramVersion::V1.id());
            assert_eq!(entry.schedules, 10, "每族 N=10");
            assert_eq!(entry.runs, 40, "N × 4 路实现");
            assert_eq!(
                entry.opportunities + entry.clean_runs,
                entry.runs,
                "每个用例都要落到一边"
            );
            assert_eq!(entry.by_schedule.len(), 10);
        }
        // 真值只来自隐藏检查：机会数在各臂之间必须一致（它不由臂自己认定）。
        let opportunities: Vec<u32> = [ARM_FIXED, ARM_GENERATED, ARM_OBSERVER]
            .into_iter()
            .map(|name| arm(report, name, family.id(), ProgramVersion::V1.id()).opportunities)
            .collect();
        assert!(
            opportunities.iter().all(|value| *value == opportunities[0]),
            "{family} 的机会数必须由隐藏检查定：{opportunities:?}"
        );
        assert!(opportunities[0] > 0, "{family} 至少有几路实现会违反");
    }

    // 报告可 JSON 往返，且关键口径都在。
    let wire = serde_json::to_value(report).expect("报告可序列化");
    assert_eq!(wire["run"], json!("h4-offline"));
    assert!(wire["protocol"]["metric_definitions"]["false_pass_rate"].is_string());
    assert!(wire["protocol"]["metric_definitions"]["transfer"].is_string());
    assert!(wire["protocol"]["metric_definitions"]["amortization"].is_string());
    assert!(
        wire["raw"]["runs"]
            .as_str()
            .expect("原始路径")
            .ends_with("runs.jsonl")
    );
    assert_eq!(
        wire["families"][0]["landmark"],
        json!(true),
        "第一位是标志性任务"
    );
    assert!(dir.join("run").join("runs.jsonl").exists());
    assert!(dir.join("run").join("runs-v2.jsonl").exists());
    assert!(dir.join("run").join("verification.json").exists());
    // 迁移矩阵有逐用例原始件，`TransferCell` 的每个数字都能回查。
    assert!(dir.join("run").join("transfer.jsonl").exists());
    assert!(
        wire["raw"]["transfer"]
            .as_str()
            .expect("迁移原始件路径")
            .ends_with("transfer.jsonl")
    );
    assert!(
        wire["raw"]["tests_v2_file"]
            .as_str()
            .expect("v2 测试产物路径")
            .ends_with("tests-v2.json")
    );
    // 固定测试臂的声明表在报告里：§7.3 禁止的那条替换要看得见。
    let declarations = wire["families"][0]["fixed_arm_declaration"]
        .as_array()
        .expect("固定臂声明表在报告里");
    assert!(
        declarations
            .iter()
            .any(|item| item["weakened_substitution"] == json!(true)),
        "被禁的替换必须在报告里标出来：{declarations:?}"
    );
}

#[tokio::test]
async fn the_banned_substitution_is_wrong_in_both_directions_on_the_landmark_family() {
    let dir = TempDir::new("banned");
    let outcome = run_offline(&dir).await;
    let report = &outcome.report;
    let family = CFamily::CancelAfterConfirm.id();
    let fixed = arm(report, ARM_FIXED, family, ProgramVersion::V1.id());

    // 固定臂这一族的检查项就是 §7.3 禁止的替换，报告里要能看出来。
    assert!(
        report
            .protocol
            .banned_substitutions
            .iter()
            .any(|pattern| pattern == BANNED_SUBSTITUTIONS[0]),
        "被禁的替换要写进协议声明"
    );
    assert!(
        fixed.false_alarms > 0,
        "方向一：暂停之前的合法写入把它判成违反（误报 {}/{}）",
        fixed.false_alarms,
        fixed.clean_runs
    );
    assert!(
        fixed.false_passes > 0,
        "方向二：确认之后写入再被合法回收，它说没事（误放行 {}/{}）",
        fixed.false_passes,
        fixed.opportunities
    );

    // 同一条调度上：终态与真值对不上，正是被禁替换判错的现场。
    let masked = fixed
        .by_schedule
        .iter()
        .find(|reading| reading.schedule == "c1-s05")
        .expect("c1-s05 在读数里");
    assert!(
        masked.arms[ARM_FIXED].false_passes > 0,
        "确认后写入被合法回收：终态为空，违规是真的"
    );
}

#[tokio::test]
async fn the_observer_wins_where_the_pre_existing_observables_cannot_see() {
    let dir = TempDir::new("gap");
    let outcome = run_offline(&dir).await;
    let report = &outcome.report;

    // C3（重启重放）：程序自报的意图数看不见重复提交，固定臂与通用臂都会误放行。
    let family = CFamily::DuplicateAfterRestart.id();
    let fixed = arm(report, ARM_FIXED, family, ProgramVersion::V1.id());
    let generated = arm(report, ARM_GENERATED, family, ProgramVersion::V1.id());
    let observer = arm(report, ARM_OBSERVER, family, ProgramVersion::V1.id());
    assert_eq!(fixed.detections, 0, "自报口径看不见重复提交");
    assert_eq!(
        fixed.false_passes, fixed.opportunities,
        "自报意图数是 1，重复提交照样说没事"
    );
    assert_eq!(generated.detections, 0, "同一份可观察量，换谁写都一样");
    assert_eq!(generated.false_passes, generated.opportunities);
    assert!(observer.detections > 0, "写入边界看得见重复提交");
    assert_eq!(observer.false_passes, 0, "观测器一次误放行都没有");
    assert_eq!(observer.false_alarms, 0, "观测器一次误报都没有");

    // C1（标志性任务）：固定臂在两个方向上都会判错，观测器一次都没判错。
    let family = CFamily::CancelAfterConfirm.id();
    let observer = arm(report, ARM_OBSERVER, family, ProgramVersion::V1.id());
    assert_eq!(observer.false_passes, 0);
    assert_eq!(observer.false_alarms, 0);
    assert!(observer.detections > 0);

    // 反面：C2（丢失更新）终态计数看得见，固定臂本来就够用——这不是处处都要建观测器。
    let family = CFamily::LostUpdate.id();
    let fixed = arm(report, ARM_FIXED, family, ProgramVersion::V1.id());
    let observer = arm(report, ARM_OBSERVER, family, ProgramVersion::V1.id());
    assert_eq!(fixed.detections, fixed.opportunities, "终态看得见丢失更新");
    assert!(
        observer.detection_rate <= fixed.detection_rate,
        "这一族上建设观测器没有拿到额外检出：{}/{} vs {}/{}",
        observer.detections,
        observer.opportunities,
        fixed.detections,
        fixed.opportunities
    );
}

#[tokio::test]
async fn a_built_observer_does_not_transfer_to_a_different_fault_family() {
    let dir = TempDir::new("transfer");
    let outcome = run_offline(&dir).await;

    // 对角线（为它建的）抓得到、且一次都不误放行；非对角线（没为它建的）都更差。
    for target in CFamily::ALL {
        let diagonal = outcome
            .report
            .transfer
            .iter()
            .find(|cell| cell.same_family && cell.applied_to == target.id())
            .unwrap_or_else(|| panic!("{target} 的对角线在"));
        assert!(
            diagonal.detections > 0,
            "{target} 的对角线得抓到东西：{diagonal:?}"
        );
        assert_eq!(
            diagonal.false_passes, 0,
            "{target} 的对角线一次都不许误放行"
        );
        for cell in outcome
            .report
            .transfer
            .iter()
            .filter(|cell| !cell.same_family && cell.applied_to == target.id())
        {
            assert!(
                cell.detection_rate.unwrap_or(0.0) <= diagonal.detection_rate.unwrap_or(0.0),
                "换个族建的观测器不该比为它建的更好：{cell:?} vs {diagonal:?}"
            );
            assert!(
                cell.false_passes > 0
                    || cell.inconclusive_on_opportunity + cell.inconclusive_on_clean > 0,
                "非对角线要么误放行、要么落在失败态——它看不见这个族的顺序：{cell:?}"
            );
        }
    }
    assert_eq!(outcome.report.transfer.len(), 9, "3 × 3 的矩阵");
}

#[tokio::test]
async fn the_four_way_check_is_required_and_a_self_certifying_check_is_rejected() {
    let dir = TempDir::new("verify");
    let outcome = run_offline(&dir).await;

    for family in &outcome.report.verification.per_family {
        assert!(family.passed, "{} 的四路应当全过", family.family);
        assert_eq!(family.directions.len(), 4);
        let hidden = family
            .directions
            .iter()
            .find(|direction| direction.direction == "独立隐藏检查")
            .expect("第四路在");
        assert_eq!(hidden.expected, "发现违反");
        assert_eq!(hidden.hidden, "发现违反", "绕过记录器时隐藏检查必须抓到");
        assert!(
            hidden.observer == "观测失败" || hidden.observer == "发现违反",
            "观测器要么显式失败、要么自己也抓到，总之不许说「成立」：{}",
            hidden.observer
        );
        assert!(
            !hidden.passed || hidden.hidden == "发现违反",
            "第四路按隐藏检查判"
        );
        assert!(
            family.observer_missed_the_bypass,
            "记录器被绕过时观测器自己漏掉——这正是隐藏检查存在的理由"
        );

        // 「不可自证」：拿观测器自己的输出顶替隐藏检查时，启用必须被门槛拒绝。
        assert!(
            family.self_certifying_rejected,
            "自证式的伪隐藏检查必须被启用门槛拒绝：{:?}",
            family.self_certifying_error
        );
        assert!(
            family
                .self_certifying_error
                .as_deref()
                .is_some_and(|text| text.contains("未通过独立检验")),
            "{:?}",
            family.self_certifying_error
        );
        assert_eq!(family.lifecycle_after_rejection.as_deref(), Some("验证"));
        assert_eq!(
            family.checks_on_record_after_rejection, 1,
            "被拒的那一条检验记录留在库里"
        );
        assert!(family.version.is_some(), "四路全过之后才能启用");
    }
}

#[tokio::test]
async fn the_cheap_maintenance_step_is_what_the_observer_pays_for() {
    let dir = TempDir::new("maintenance");
    let outcome = run_offline(&dir).await;
    let report = &outcome.report;

    let support_only = report
        .maintenance
        .arms
        .iter()
        .find(|reading| reading.arm == ARM_OBSERVER_V2_SUPPORT_ONLY)
        .expect("只改支持版本的那一行在");
    let reverified = report
        .maintenance
        .arms
        .iter()
        .find(|reading| reading.arm == ARM_OBSERVER_V2_REVERIFIED)
        .expect("重挂的那一行在");
    let generated = report
        .maintenance
        .arms
        .iter()
        .find(|reading| reading.arm == ARM_GENERATED_V2)
        .expect("通用臂的那一行在");

    // 观测器臂的维护不需要生成调用，代价是重挂 + 重跑四路检验。
    assert_eq!(support_only.regeneration_calls, 0);
    assert_eq!(reverified.regeneration_calls, 0);
    assert!(!support_only.reattached, "只改支持版本，钩子点没重挂");
    assert!(reverified.reattached && reverified.reverified);
    // 预置产物下通用臂没有真的重生成；报告如实标 false，不假装生成过。
    assert!(!generated.regenerated);
    assert_eq!(generated.regeneration_calls, 0);

    // 省掉「重挂」那一步的代价：v2 新增的直接提交路径记录不到，能检出的反而更少。
    assert!(
        reverified.detection_rate_after.unwrap_or(0.0)
            > support_only.detection_rate_after.unwrap_or(0.0),
        "重挂之后在 v2 上必须检出更多：{:?} vs {:?}",
        reverified.detection_rate_after,
        support_only.detection_rate_after
    );
    assert_eq!(
        reverified.false_pass_rate_after,
        Some(0.0),
        "重挂之后一次误放行都没有"
    );

    // 契约没选提交计数器：记录器被绕过时它就照着空的记录说「成立」，误放行直接出现。
    let no_counter = report
        .maintenance
        .arms
        .iter()
        .find(|reading| reading.arm == ARM_OBSERVER_V2_NO_COUNTER)
        .expect("拿掉计数器的那一行在");
    assert!(
        no_counter.false_pass_rate_after.unwrap_or(0.0)
            > support_only.false_pass_rate_after.unwrap_or(1.0),
        "少选一个钩子点就多出误放行：{:?} vs {:?}",
        no_counter.false_pass_rate_after,
        support_only.false_pass_rate_after
    );
}

#[tokio::test]
async fn the_amortization_curve_and_the_hypothesis_verdict_are_written_down() {
    let dir = TempDir::new("amortization");
    let outcome = run_offline(&dir).await;
    let report = &outcome.report;

    let points = &report.amortization.points;
    assert_eq!(points.len(), 3, "覆盖 1 / 2 / 3 个族各一个点");
    for point in points {
        assert_eq!(point.cumulative_calls[ARM_FIXED], 0, "固定臂不生成任何东西");
        assert_eq!(
            point.cumulative_calls[ARM_OBSERVER], point.families_covered as u32,
            "观测器臂每族一份契约"
        );
        assert!(point.detections[ARM_OBSERVER] > 0);
    }

    // 假说判据在报告里写全了，结论与之自洽。
    assert_eq!(
        report.hypothesis.criteria.len(),
        3,
        "判据冻结成三条判定规则；报告方式单独记在 reporting 里"
    );
    assert!(!report.hypothesis.reporting.is_empty());
    assert!(!report.hypothesis.conclusion.is_empty());
    assert_eq!(
        report.hypothesis.supported,
        report.hypothesis.improves_results || report.hypothesis.improves_budget
    );
    // 判据与结论自洽；判据用的是误放行率，两种读法都在 readings 里。
    assert!(
        report
            .hypothesis
            .readings
            .iter()
            .any(|reading| reading.contains("误放行率")),
        "{:?}",
        report.hypothesis.readings
    );
    assert!(
        report
            .hypothesis
            .readings
            .iter()
            .any(|reading| reading.contains("检出率")),
        "另一种读法也要给出：{:?}",
        report.hypothesis.readings
    );
    let fixed = arm(
        report,
        ARM_FIXED,
        CFamily::CancelAfterConfirm.id(),
        ProgramVersion::V1.id(),
    );
    let observer = arm(
        report,
        ARM_OBSERVER,
        CFamily::CancelAfterConfirm.id(),
        ProgramVersion::V1.id(),
    );
    assert!(
        observer.false_pass_rate.unwrap_or(1.0) < fixed.false_pass_rate.unwrap_or(0.0),
        "标志性任务上观测器的误放行率必须低于固定臂"
    );
    if !report.hypothesis.supported {
        assert!(
            report.hypothesis.conclusion.contains("不支持累积能力假说"),
            "判据不成立时结论必须原话写出来：{}",
            report.hypothesis.conclusion
        );
    }
    // 保留意见里要能看出「固定臂在 C2 上本来就够用」这件事没有被抹平。
    assert!(
        report
            .hypothesis
            .caveats
            .iter()
            .any(|caveat| caveat.contains("C2")),
        "{:?}",
        report.hypothesis.caveats
    );
    assert!(
        report
            .hypothesis
            .caveats
            .iter()
            .any(|caveat| caveat.contains("迁移")),
        "跨族套用基本检出不到东西，要写进保留意见"
    );
}

#[tokio::test]
async fn a_weakened_contract_leaves_that_family_without_an_observer() {
    let dir = TempDir::new("weakened");
    // 把标志性任务的契约换成被禁的弱化命题：观测器臂在这一族上必须不可用。
    let mut contracts = contracts();
    for entry in &mut contracts {
        if entry.family == CFamily::CancelAfterConfirm {
            let mut contract = contract_of(CFamily::CancelAfterConfirm);
            contract.proposition = BANNED_SUBSTITUTIONS[0].to_owned();
            let check = c_measure::check_contract(
                &contract,
                CFamily::CancelAfterConfirm,
                ProgramVersion::V1,
            );
            assert!(!check.passed, "弱化命题必须过不了核对");
            entry.check = Some(check.clone());
            entry.contract = Some(contract);
            entry.rejection = Some(format!("协议核对未通过：{}", check.checks.len()));
        }
    }
    let contracts_path = dir.join("contracts.json");
    let tests_path = dir.join("tests.json");
    write(&contracts_path, &contracts);
    write(&tests_path, &tests());
    let options = H4Options {
        run: "h4-offline-weakened".to_owned(),
        calls_path: dir.join("calls.jsonl"),
        out_dir: dir.join("run"),
        command: "cargo test -p jh-app --test h4".to_owned(),
        contracts: ArtifactSource::File(contracts_path),
        tests: ArtifactSource::File(tests_path),
        repeats: Some(2),
    };
    let outcome = h4::run(&options).await.expect("离线 H4 应跑到底");
    let report = &outcome.report;

    assert_eq!(report.generation.rejections, 1, "被拒的产物计一笔");
    assert!(
        report
            .verification
            .per_family
            .iter()
            .all(|family| family.family != CFamily::CancelAfterConfirm.id()),
        "被拒的契约不该装出观测器"
    );
    let observer = arm(
        report,
        ARM_OBSERVER,
        CFamily::CancelAfterConfirm.id(),
        ProgramVersion::V1.id(),
    );
    assert_eq!(
        observer.verification.status,
        jh_app::report::VerificationStatus::NotVerified,
        "这一族没有可用契约：结论保持未验证，不拿别的臂顶替"
    );
    assert_eq!(observer.detections, 0);
    assert_eq!(observer.false_passes, 0, "没有观测器时就出不了「命题成立」");
    assert_eq!(
        observer.inconclusive(),
        observer.runs,
        "没有可用观测器的每一例都落在失败态"
    );
}

#[tokio::test]
async fn reusing_preloaded_artifacts_still_runs_the_generated_arm_on_v2() {
    // 复现评审实测到的那条回归：`--tests` 复用预置产物时，`generated@v2` 那一臂曾经整个消失。
    let dir = TempDir::new("reuse-v2");
    let outcome = run_offline(&dir).await;
    let report = &outcome.report;

    for family in CFamily::ALL {
        let entry = arm(
            report,
            ARM_GENERATED_V2,
            family.id(),
            ProgramVersion::V2.id(),
        );
        assert!(
            entry.detection_rate.is_some(),
            "{family} 的 generated@v2 必须有读数（回退没写就会是 null）"
        );
        assert_eq!(entry.runs, 40, "{family}：N × 4 路实现");
        let reading = report
            .maintenance
            .arms
            .iter()
            .find(|reading| reading.arm == ARM_GENERATED_V2)
            .expect("维护行在");
        assert!(
            reading.detection_rate_after.is_some(),
            "维护读数不能是 null：{reading:?}"
        );
        assert!(!reading.regenerated, "复用预置产物时如实标「没有重生成」");
        assert_eq!(reading.regeneration_calls, 0);
    }
    // 版本变更阶段用的那份计划也落了盘，维护读数可以只靠产物复核。
    assert!(dir.join("run").join("tests-v2.json").exists());
}

#[tokio::test]
async fn the_cost_total_agrees_with_its_own_parts() {
    let dir = TempDir::new("cost");
    let outcome = run_offline(&dir).await;
    let cost = &outcome.report.cost;

    let by_parts: u32 = cost.by_purpose.iter().map(|entry| entry.calls).sum();
    assert_eq!(cost.total.calls, by_parts, "合计的调用数要等于分项之和");
    assert_eq!(
        cost.total.stats.attempted, cost.total.calls,
        "同一个对象里 `calls` 与调用统计的样本数不能各说各话"
    );
    assert_eq!(
        cost.total.latency.samples, cost.total.calls,
        "延迟样本数要与调用数一致"
    );
    let with_usage: u32 = cost
        .by_purpose
        .iter()
        .map(|entry| entry.usage.calls_with_usage)
        .sum();
    assert_eq!(cost.total.usage.calls_with_usage, with_usage);
    assert_eq!(
        cost.total.input_tokens.is_some(),
        with_usage > 0,
        "有上报过用量的调用时合计不能是 null"
    );
}

#[tokio::test]
async fn the_transfer_cells_can_be_closed_against_their_own_counts() {
    let dir = TempDir::new("transfer-counts");
    let outcome = run_offline(&dir).await;
    for cell in &outcome.report.transfer {
        assert_eq!(
            cell.opportunities + cell.clean_runs,
            cell.runs,
            "机会 + 无违反 = 用例数，每个用例只落一边：{cell:?}"
        );
        assert_eq!(
            cell.opportunities,
            cell.detections + cell.false_passes + cell.inconclusive_on_opportunity,
            "机会上的三种结果要加起来等于机会数：{cell:?}"
        );
        assert_eq!(
            cell.clean_runs,
            cell.false_alarms
                + cell.inconclusive_on_clean
                + (cell.clean_runs - cell.false_alarms - cell.inconclusive_on_clean),
            "{cell:?}"
        );
        assert!(
            cell.inconclusive_on_clean <= cell.clean_runs,
            "干净用例上的弃权不可能超过干净用例数：{cell:?}"
        );
    }
    assert!(dir.join("run").join("transfer.jsonl").exists());
}

#[tokio::test]
async fn the_maintenance_totals_close_over_the_whole_batch() {
    let dir = TempDir::new("maintenance-totals");
    let outcome = run_offline(&dir).await;
    let totals = &outcome.report.maintenance.totals;
    assert!(!totals.is_empty());

    let reverified = totals
        .iter()
        .find(|total| total.arm == ARM_OBSERVER_V2_REVERIFIED)
        .expect("重挂那一行在");
    let support_only = totals
        .iter()
        .find(|total| total.arm == ARM_OBSERVER_V2_SUPPORT_ONLY)
        .expect("只改支持版本那一行在");
    assert_eq!(reverified.families, 3, "合计覆盖三个族");
    assert_eq!(
        reverified.opportunities + reverified.clean_runs,
        reverified.runs
    );
    assert!(
        reverified.detection_rate_after.unwrap_or(0.0)
            > support_only.detection_rate_after.unwrap_or(1.0),
        "全批口径下重挂也要带来更多检出：{:?} vs {:?}",
        reverified.detection_rate_after,
        support_only.detection_rate_after
    );
    let no_counter = totals
        .iter()
        .find(|total| total.arm == ARM_OBSERVER_V2_NO_COUNTER)
        .expect("拿掉计数器那一行在");
    assert!(
        no_counter.false_pass_rate_after.unwrap_or(0.0)
            > support_only.false_pass_rate_after.unwrap_or(1.0),
        "少选一个钩子点在全批口径下也要多出误放行"
    );
}
