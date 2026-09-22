//! 影子运行：新问题**参与判断、不进决策**，读「若采纳会不会改变有价值的行动分支」
//! （`docs/RESEARCH_GOALS.md` §6.2 第 6 步、§6.5）。
//!
//! 影子问题由判断循环里的 [`crate::matching::ShadowPlan`] 驱动：每一轮在主请求之外**额外**发一次
//! 判定，结果落在**影子状态**里——控制路径（前提门、行动复核）只读主状态，所以影子结果结构上进不了
//! 决策。每个影子判断都对着本轮的实际决策算出「换成它会怎样」（同一条门判据
//! [`crate::matching::BranchChange`]），改变方向按独立验收的正当性口径分成
//! 「走向正当动作」（有价值的分支）与「走向不该发出的动作」。
//!
//! 影子运行的成本单独记账（`calls` / token / 延迟）：它属于 §6.5 要求扣除的「后续评估成本」。

use std::path::Path;

use jh_core::{Judge, ObjectId, QuestionId, QuestionSpec, QuestionVersion};
use serde::{Deserialize, Serialize};

use crate::calls::CallLog;
use crate::error::Error;
use crate::family_b::{ABSENT_LABEL, FamilyB, FamilyBQuestions, FamilyBTask};
use crate::judge::JudgeName;
use crate::matching::{
    Arm, BranchChange, DecisionOutcome, LoopContext, ShadowPlan, ShadowQuestion, TaskTrace,
    run_task,
};

/// 构造影子计划：一条候选版本的问题。
pub fn plan(
    question: &QuestionId,
    spec: QuestionSpec,
    reads: Vec<ObjectId>,
    note: &str,
) -> ShadowPlan {
    ShadowPlan {
        questions: vec![ShadowQuestion {
            question: question.clone(),
            spec,
            scope: reads,
            note: note.to_owned(),
        }],
    }
}

/// 一个任务上的影子读数。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShadowTaskReading {
    /// 任务标识。
    pub task: String,
    /// 属于哪一对。
    pub pair: String,
    /// 隐藏侧（这一对里这一侧）。
    pub side: String,
    /// 哪个子集。
    pub subset: String,
    /// 是不是开发案例。
    pub development: bool,
    /// 控制路径（旧问题集）的独立验收是否通过。
    pub control_acceptance_passed: bool,
    /// 影子判断条数。
    pub shadow_judgments: u32,
    /// 影子判断失败条数。
    pub shadow_failures: u32,
    /// 打上分的影子判断条数。
    pub scored: u32,
    /// 与真值一致的条数。
    pub agreements: u32,
    /// 读出正确一侧的条数（「把这成对场景区分开」的口径，与材料充分性无关）。
    pub side_matches: u32,
    /// 材料足够（关键证据在投影里）的影子判断条数：区分率的分母。
    pub side_sufficient: u32,
    /// 材料不足的影子判断条数（关键证据不在它的投影里）。
    pub material_insufficient: u32,
    /// 材料不足却读出候选集内标签的条数（M1 的 A1 教训口径）。
    pub candidate_label_when_insufficient: u32,
    /// 「若采纳会改变的行动分支」。
    pub branch_changes: Vec<BranchChange>,
    /// 轨迹文件（原始数据位置）。
    pub trace: String,
    /// 证据日志文件。
    pub evidence: String,
}

/// 影子运行的汇总读数。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShadowStats {
    /// 任务数。
    pub tasks: u32,
    /// 影子判断条数。
    pub shadow_judgments: u32,
    /// 影子判断失败条数。
    pub shadow_failures: u32,
    /// 影子判断的一致率。
    pub agreement_rate: Option<f64>,
    /// 走向正当动作的分支改变次数（有价值的分支）。
    pub toward_justified: u32,
    /// 走向不该发出的动作的分支改变次数。
    pub toward_unjustified: u32,
    /// 控制路径的验收通过与任务数。
    pub control_acceptance: String,
}

/// 一次影子运行的全部读数。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShadowReadings {
    /// 被影子评估的问题。
    pub question: QuestionId,
    /// 被影子评估的版本。
    pub version: QuestionVersion,
    /// 逐任务读数。
    pub tasks: Vec<ShadowTaskReading>,
    /// 汇总。
    pub stats: ShadowStats,
}

/// 在给定任务上跑一遍影子运行：控制路径用 `question_set`（旧问题集），影子问题额外问。
///
/// 两臂共用同一批任务、同一份初始证据、同一个判定器与同一个决策时机；差别只有影子问题的存在——
/// 这正是 §9.2 的 H3 消融口径（固定问题集合那一行保留了观察与生成模型）。
#[allow(clippy::too_many_arguments)]
pub async fn run(
    family: &FamilyB,
    tasks: &[&FamilyBTask],
    question_set: &FamilyBQuestions,
    plan: &ShadowPlan,
    judge: &dyn Judge,
    channel: JudgeName,
    run_id: &str,
    endpoint: Option<String>,
    calls: &mut CallLog,
    out_dir: &Path,
) -> Result<ShadowReadings, Error> {
    let traces_dir = out_dir.join("shadow-traces");
    let evidence_dir = out_dir.join("shadow-evidence");
    std::fs::create_dir_all(&traces_dir)?;
    std::fs::create_dir_all(&evidence_dir)?;

    let mut readings = Vec::with_capacity(tasks.len());
    let label = plan.questions[0].question.to_string();
    for entry in tasks {
        let stem = format!("shadow-{label}-{}-{}", channel.cli_name(), entry.task.id);
        let evidence_path = evidence_dir.join(format!("{stem}.jsonl"));
        let trace_path = traces_dir.join(format!("{stem}.json"));
        if evidence_path.exists() {
            // 同一次影子评估里每个任务一份证据日志：重复跑同一对会追加旧材料，必须显式拦住。
            return Err(Error::TaskSet(format!(
                "影子证据日志 {} 已存在：换一个 --out-dir 再跑，别把上一次的材料混进来",
                evidence_path.display()
            )));
        }
        let mut context = LoopContext {
            family: question_set,
            judge,
            channel,
            arm: Arm::ALL[0],
            run: run_id.to_owned(),
            endpoint: endpoint.clone(),
            shadow: Some(plan),
            calls,
        };
        let trace = run_task(&mut context, &entry.task, &evidence_path).await?;
        crate::compare::write_json(&trace_path, &trace)?;
        readings.push(reading_of(
            family,
            entry,
            &trace,
            &trace_path,
            &evidence_path,
        ));
    }

    let stats = summarize(&readings);
    Ok(ShadowReadings {
        question: plan.questions[0].question.clone(),
        version: plan.questions[0].spec.version,
        tasks: readings,
        stats,
    })
}

fn reading_of(
    family: &FamilyB,
    entry: &FamilyBTask,
    trace: &TaskTrace,
    trace_path: &Path,
    evidence_path: &Path,
) -> ShadowTaskReading {
    let key_object = family
        .pair(&entry.pair)
        .map(|pair| pair.key_object.clone())
        .unwrap_or_else(|| ObjectId::from(""));
    let material_insufficient = trace
        .shadows
        .iter()
        .filter(|shadow| !shadow.projection.reads(&key_object))
        .count() as u32;
    let candidate_label_when_insufficient = trace
        .shadows
        .iter()
        .filter(|shadow| {
            !shadow.projection.reads(&key_object)
                && shadow.reading != ABSENT_LABEL
                && !shadow.reading.is_empty()
        })
        .count() as u32;

    ShadowTaskReading {
        task: entry.task.id.clone(),
        pair: entry.pair.clone(),
        side: entry.side.clone(),
        subset: entry.subset.label().to_owned(),
        development: entry.development,
        control_acceptance_passed: trace
            .acceptance
            .as_ref()
            .is_some_and(|acceptance| acceptance.passed),
        shadow_judgments: trace.shadows.len() as u32,
        shadow_failures: trace
            .shadows
            .iter()
            .filter(|shadow| shadow.failure.is_some())
            .count() as u32,
        scored: trace
            .shadows
            .iter()
            .filter(|shadow| shadow.agreement.is_some())
            .count() as u32,
        agreements: trace
            .shadows
            .iter()
            .filter(|shadow| shadow.agreement == Some(true))
            .count() as u32,
        side_matches: trace
            .shadows
            .iter()
            .filter(|shadow| shadow.reading == entry.side)
            .count() as u32,
        side_sufficient: trace
            .shadows
            .iter()
            .filter(|shadow| shadow.projection.reads(&key_object))
            .count() as u32,
        material_insufficient,
        candidate_label_when_insufficient,
        branch_changes: trace
            .shadows
            .iter()
            .flat_map(|shadow| shadow.branches.clone())
            .collect(),
        trace: crate::compare::absolute(trace_path),
        evidence: crate::compare::absolute(evidence_path),
    }
}

fn summarize(readings: &[ShadowTaskReading]) -> ShadowStats {
    let judgments: u32 = readings
        .iter()
        .map(|reading| reading.shadow_judgments)
        .sum();
    let agreements: u32 = readings.iter().map(|reading| reading.agreements).sum();
    let scored: u32 = readings.iter().map(|reading| reading.scored).sum();
    let branches: Vec<&BranchChange> = readings
        .iter()
        .flat_map(|reading| reading.branch_changes.iter())
        .collect();
    let acceptance_passed = readings
        .iter()
        .filter(|reading| reading.control_acceptance_passed)
        .count();

    ShadowStats {
        tasks: readings.len() as u32,
        shadow_judgments: judgments,
        shadow_failures: readings.iter().map(|reading| reading.shadow_failures).sum(),
        agreement_rate: (scored > 0).then(|| agreements as f64 / scored as f64),
        toward_justified: branches
            .iter()
            .filter(|branch| {
                branch.hypothetical == DecisionOutcome::Executed && branch.toward_justified
            })
            .count() as u32,
        toward_unjustified: branches
            .iter()
            .filter(|branch| {
                branch.hypothetical == DecisionOutcome::Executed && !branch.toward_justified
            })
            .count() as u32,
        control_acceptance: format!("{acceptance_passed}/{}", readings.len()),
    }
}

/// 影子运行是否干净：没有失败的影子判断（失败如实计入，不静默重试）。
pub fn clean(readings: &ShadowReadings) -> bool {
    readings.stats.shadow_failures == 0
}

/// 影子计划里那条问题的说明文本（进报告）。
pub fn describe(plan: &ShadowPlan) -> String {
    plan.questions
        .iter()
        .map(|question| {
            format!(
                "{} v{}：{}",
                question.question, question.spec.version, question.note
            )
        })
        .collect::<Vec<_>>()
        .join("；")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::judge::RuleJudge;

    #[tokio::test]
    async fn the_shadow_question_participates_in_judgment_without_touching_decisions() {
        let family = FamilyB::load().unwrap();
        let dev = family.dev_case("P1").expect("P1 的开发案例");
        let question = QuestionId::from("b_pair_cause_p1");
        let plan = plan(
            &question,
            QuestionSpec {
                version: QuestionVersion(2),
                question: family.question(&question).unwrap().question.clone(),
            },
            vec![
                ObjectId::from("app.log"),
                ObjectId::from("credential.audit"),
            ],
            "影子：候选版本只读，不进决策",
        );
        let judge = RuleJudge::new();
        let dir = std::env::temp_dir().join(format!("jh-app-shadow-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut calls = CallLog::open(dir.join("calls.jsonl")).unwrap();

        let with_shadow = run(
            &family,
            &[dev],
            &family.question_set(),
            &plan,
            &judge,
            JudgeName::Rule,
            "shadow-with",
            None,
            &mut calls,
            &dir,
        )
        .await
        .expect("规则判定器不会失败");

        // 控制路径：固定问题读不到关键证据 → 不发补救动作 → 验收不过；影子问题读到它 → 分支会变。
        assert_eq!(with_shadow.tasks.len(), 1);
        assert!(!with_shadow.tasks[0].control_acceptance_passed);
        assert!(with_shadow.tasks[0].shadow_judgments > 0);
        assert_eq!(
            with_shadow.stats.toward_justified, 3,
            "影子会在每一轮把补救动作放出来（控制路径一次都没发出）"
        );
        assert_eq!(with_shadow.stats.toward_unjustified, 0);

        // 对照：不带影子跑同一任务一次，控制路径的决策与验收必须逐字相同。
        let mut control_calls = CallLog::open(dir.join("control-calls.jsonl")).unwrap();
        let mut context = LoopContext {
            family: &family.question_set(),
            judge: &judge,
            channel: JudgeName::Rule,
            arm: Arm::ALL[0],
            run: "shadow-off".to_owned(),
            endpoint: None,
            shadow: None,
            calls: &mut control_calls,
        };
        let control = run_task(&mut context, &dev.task, &dir.join("control-evidence.jsonl"))
            .await
            .expect("规则判定器不会失败");
        let shadowed = serde_json::from_str::<TaskTrace>(
            &std::fs::read_to_string(&with_shadow.tasks[0].trace).expect("轨迹可读"),
        )
        .expect("轨迹可解析");
        let shape = |trace: &TaskTrace| -> Vec<(usize, String, DecisionOutcome)> {
            trace
                .decisions
                .iter()
                .map(|decision| {
                    (
                        decision.round,
                        decision.action.to_string(),
                        decision.outcome,
                    )
                })
                .collect()
        };
        assert_eq!(
            shape(&control),
            shape(&shadowed),
            "影子不进决策（时间戳不参与比较）"
        );
        assert_eq!(control.acceptance, shadowed.acceptance);
        assert!(
            shadowed
                .shadows
                .iter()
                .all(|shadow| !shadow.branches.is_empty())
        );

        std::fs::remove_dir_all(&dir).ok();
    }
}
