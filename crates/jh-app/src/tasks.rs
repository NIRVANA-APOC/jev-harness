//! 初始任务集：合成样本（state + 类型化问题 + 隐藏真值）。
//!
//! M0 的初始任务集是**最小合成集**，用来得到本环境的质量、延迟与成本基线，并能区分真实 API 与
//! 模拟运行（`docs/RESEARCH_GOALS.md` §12 M0）。样本按「固定问题集」组织：全部样本共用同一组问题
//! （choice / score / noul 各一），因此规则判断器可以按题作答而不依赖样本 ID。
//!
//! # 真值隔离
//!
//! 真值写在任务集的 `truth` 字段里，但**只进打分侧**：判断请求由
//! [`TaskSet::request`] 构造，只带快照标识、state 与问题集，真值不参与
//! （`docs/RESEARCH_GOALS.md` §13.2：评测真值与正常输入隔离）。真值可以由日志独立复核，
//! 不依赖任何模型输出。
//!
//! # 样本来源
//!
//! 全部样本是合成材料（不是真实运行记录），场景是「Agent 运行时诊断自身任务失败」，
//! 每条 `log` 都写下足以归因、或明确不足以归因的记录。

use std::collections::BTreeMap;

use jh_core::{
    Answer, ChoiceAnswer, Error, JudgeRequest, NoulAnswer, Question, QuestionId, Questions,
    ScoreAnswer, SnapshotId, State,
};
use serde::{Deserialize, Serialize};

/// 初始任务集的 JSON 原文：编译期内嵌，运行时不依赖工作目录。
pub const INITIAL_TASK_SET_JSON: &str = include_str!("../tasks/initial_tasks.json");

/// 初始任务集在仓库里的位置，写进报告以便复核。
pub const INITIAL_TASK_SET_SOURCE: &str = "crates/jh-app/tasks/initial_tasks.json";

/// 一个样本的真值：该样本在各问题上的预设标签。
pub type TruthMap = BTreeMap<QuestionId, Truth>;

/// 隐藏真值：按原语记下该样本的预设标签。
///
/// 等价于 one-hot 概率标签（`docs/RESEARCH_GOALS.md` §10.2 要求概率标签有独立定义）；
/// 由样本材料定义，不由任何判断器产生。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Truth {
    /// 该样本正确的选项名。
    Choice {
        /// 选项名，必须在该问题的选项集里。
        label: String,
    },
    /// 该样本预设的级号（从 0 开始）。
    Score {
        /// 级号，必须在该问题的级数范围内。
        level: u8,
    },
    /// 该样本的是非真值。
    Noul {
        /// 真值是「是」还是「否」。
        yes: bool,
    },
}

impl Truth {
    /// 把真值渲染成类型化答案（one-hot 分布）。
    ///
    /// 模拟真值判断器用它给出上限；真值与问题原语不符时显式失败，不做兜底。
    pub fn render(&self, question: &Question) -> Result<Answer, Error> {
        match (self, question) {
            (Truth::Choice { label }, Question::Choice(spec)) => {
                if !spec.criteria.contains_key(label) {
                    return Err(Error::InvalidResponse(format!(
                        "真值选项 `{label}` 不在问题的选项集里"
                    )));
                }
                Ok(Answer::Choice(ChoiceAnswer {
                    choice: label.clone(),
                    probabilities: BTreeMap::from([(label.clone(), 1.0)]),
                    confidence: 1.0,
                }))
            }
            (Truth::Score { level }, Question::Score(spec)) => {
                if usize::from(*level) >= spec.criteria.len() {
                    return Err(Error::InvalidResponse(format!(
                        "真值级号 {level} 超出问题的 {} 级标尺",
                        spec.criteria.len()
                    )));
                }
                Ok(Answer::Score(ScoreAnswer {
                    score: f64::from(*level),
                    legend: spec
                        .criteria
                        .iter()
                        .enumerate()
                        .map(|(index, text)| (index as u8, text.clone()))
                        .collect(),
                    probabilities: BTreeMap::from([(*level, 1.0)]),
                    confidence: 1.0,
                }))
            }
            (Truth::Noul { yes }, Question::Noul(_)) => Ok(Answer::Noul(NoulAnswer {
                noul: if *yes { 1.0 } else { 0.0 },
            })),
            (truth, question) => Err(Error::InvalidResponse(format!(
                "真值原语与问题原语不符：真值 {truth:?}，问题 {question:?}"
            ))),
        }
    }
}

/// 一个样本：快照标识、材料与隐藏真值。
///
/// 问题集属于任务集而不是样本（固定问题集），因此不在这里重复。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskCase {
    /// 样本标识，同时用作判断请求的快照标识。
    pub id: String,
    /// 样本意图说明（合成材料的可复核性）；只给读样本的人看，不进判断请求。
    #[serde(default)]
    pub notes: String,
    /// 交给判断器的材料。
    pub state: State,
    /// 隐藏真值：每个问题一个预设标签。
    pub truth: TruthMap,
}

/// 一组样本共用的固定问题集，外加它的名字。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskSet {
    /// 任务集名字（写进报告）。
    pub name: String,
    /// 全部样本共用的问题集。
    pub questions: Questions,
    /// 样本列表。
    pub cases: Vec<TaskCase>,
}

impl TaskSet {
    /// 读取内嵌的初始任务集并校验。
    pub fn load_initial() -> Result<Self, crate::Error> {
        let task_set: Self = serde_json::from_str(INITIAL_TASK_SET_JSON).map_err(|error| {
            crate::Error::TaskSet(format!("{INITIAL_TASK_SET_SOURCE}：{error}"))
        })?;
        task_set.validate()?;
        Ok(task_set)
    }

    /// 构造某个样本的判断请求：只带快照标识、state 与问题集，**不带真值**。
    pub fn request(&self, case: &TaskCase) -> JudgeRequest {
        JudgeRequest {
            snapshot: SnapshotId::from(case.id.clone()),
            state: case.state.clone(),
            questions: self.questions.clone(),
        }
    }

    /// 真值表：给模拟真值判断器用（按快照取该样本的真值）。
    pub fn truth_map(&self) -> BTreeMap<SnapshotId, TruthMap> {
        self.cases
            .iter()
            .map(|case| (SnapshotId::from(case.id.clone()), case.truth.clone()))
            .collect()
    }

    /// 问题数与样本数的乘积口径：本次运行预期的判断条数。
    pub fn expected_judgments(&self) -> usize {
        self.cases.len() * self.questions.len()
    }

    /// 校验任务集自洽：样本 ID 唯一，每个问题都有原语匹配的真值，且没有多余真值。
    ///
    /// 显式失败是这里唯一的行为：缺真值的样本无法打分，不能默默跳过。
    pub fn validate(&self) -> Result<(), crate::Error> {
        let invalid = |detail: String| crate::Error::TaskSet(format!("{}：{detail}", self.name));
        if self.questions.is_empty() {
            return Err(invalid("问题集为空".to_owned()));
        }
        let mut seen = BTreeMap::new();
        for case in &self.cases {
            if case.id.trim().is_empty() {
                return Err(invalid("存在空样本 ID".to_owned()));
            }
            if seen.insert(case.id.clone(), ()).is_some() {
                return Err(invalid(format!("样本 ID `{}` 重复", case.id)));
            }
            for (id, spec) in &self.questions {
                let Some(truth) = case.truth.get(id) else {
                    return Err(invalid(format!("样本 `{}` 缺问题 `{id}` 的真值", case.id)));
                };
                if truth.render(&spec.question).is_err() {
                    return Err(invalid(format!(
                        "样本 `{}` 的问题 `{id}` 真值不合法",
                        case.id
                    )));
                }
            }
            for id in case.truth.keys() {
                if !self.questions.contains_key(id) {
                    return Err(invalid(format!(
                        "样本 `{}` 的真值 `{id}` 不在问题集里",
                        case.id
                    )));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_task_set_is_self_consistent() {
        let task_set = TaskSet::load_initial().expect("内嵌任务集应自洽");

        assert_eq!(task_set.name, "initial");
        assert_eq!(task_set.questions.len(), 3);
        assert_eq!(task_set.cases.len(), 4);
        assert_eq!(task_set.expected_judgments(), 12);
        assert!(task_set.cases.iter().all(|case| !case.notes.is_empty()));
    }

    #[test]
    fn requests_carry_state_and_questions_but_never_truth() {
        let task_set = TaskSet::load_initial().expect("内嵌任务集应自洽");

        for case in &task_set.cases {
            let request = task_set.request(case);
            assert_eq!(request.snapshot, SnapshotId::from(case.id.clone()));
            assert_eq!(request.questions, task_set.questions);

            // `JudgeRequest` 本身不实现 Serialize（jh-core 不落盘请求对象），
            // 这里按它携带的三样东西拼出判断器能看到的内容。
            // 选项名与真值标签同名是设计使然（真值就是其中一个选项），所以这里只断言
            // 真值的**结构**不在请求里：`truth` 字段与真值自己的判别标签。
            let material = serde_json::to_string(&(
                request.snapshot.clone(),
                request.state.clone(),
                request.questions.clone(),
            ))
            .expect("材料可序列化");

            assert!(
                !material.contains("truth"),
                "请求里不应出现真值：{material}"
            );
            assert!(
                !material.contains("\"label\""),
                "请求里不应出现真值标签结构"
            );
            assert!(!material.contains("\"yes\""), "请求里不应出现真值结构");
            assert!(!material.contains("\"level\""), "请求里不应出现真值结构");
        }
    }

    #[test]
    fn validation_rejects_a_case_without_truth() {
        let mut task_set = TaskSet::load_initial().expect("内嵌任务集应自洽");
        task_set.cases[0]
            .truth
            .remove(&QuestionId::from("evidence_strength"));

        let error = task_set.validate().expect_err("缺真值必须显式失败");
        assert!(error.to_string().contains("evidence_strength"), "{error}");
    }

    #[test]
    fn validation_rejects_a_truth_label_outside_the_options() {
        let mut task_set = TaskSet::load_initial().expect("内嵌任务集应自洽");
        task_set.cases[0].truth.insert(
            QuestionId::from("failure_kind"),
            Truth::Choice {
                label: "never_declared".to_owned(),
            },
        );

        let error = task_set.validate().expect_err("越界真值必须显式失败");
        assert!(error.to_string().contains("failure_kind"), "{error}");
    }

    #[test]
    fn truth_renders_one_hot_answers_with_the_problem_legend() {
        let task_set = TaskSet::load_initial().expect("内嵌任务集应自洽");
        let spec = &task_set.questions[&QuestionId::from("evidence_strength")];
        let answer = Truth::Score { level: 2 }
            .render(&spec.question)
            .expect("真值应能渲染");

        let Answer::Score(answer) = answer else {
            panic!("score 真值应渲染成 score 答案");
        };
        assert_eq!(answer.score, 2.0);
        assert_eq!(answer.probabilities, BTreeMap::from([(2, 1.0)]));
        assert_eq!(answer.legend.len(), 4);
        assert_eq!(answer.legend[&0], "无直接证据");
    }

    #[test]
    fn truth_render_fails_on_a_mismatched_primitive() {
        let noul = Question::Noul(jh_core::NoulQuestion {
            instructions: "？".to_owned(),
            criteria: None,
        });

        let error = Truth::Choice {
            label: "a".to_owned(),
        }
        .render(&noul)
        .expect_err("原语不符必须显式失败");
        assert!(matches!(error, Error::InvalidResponse(_)), "{error}");
    }
}
