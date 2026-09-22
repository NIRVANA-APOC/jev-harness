//! 确定性规则判断器：不调模型，按 state 文本里的标记出答案。
//!
//! # 规则形态
//!
//! 规则表按「问题 ID + 标记集合」组织，**首个命中的规则生效**：
//!
//! - 标记是子串匹配，匹配对象是 state 的 JSON 文本（三种形态统一处理，不用为每种形态各写一套）；
//! - 标记集合为空的规则永远命中，用作该题的兜底分支；
//! - 该问题一条规则都没有命中 → 显式失败（[`jh_core::Error::Unavailable`]），不猜。
//!
//! # 规则与样本配套
//!
//! 规则表与初始任务集配套：改样本必须同步改规则，否则规则基线的读数没有意义。
//! 规则给出的分布是**未校准的声明常量**（[`TOP_MASS`]），只作确定性基线，
//! 不代表任何已校准的概率——报告里按此口径标注。
//!
//! 版本串 [`VERSION`] 表示规则表版本：改规则表必须同时改它，判断记录靠它区分规则的两个版本。

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;

use jh_core::{
    Answer, ChoiceAnswer, Error, Judge, JudgeRequest, JudgeResponse, JudgeSource, NoulAnswer,
    Question, ScoreAnswer,
};

use super::derived_confidence;

/// 规则表版本；改规则表必须改这里。
pub const VERSION: &str = "rules-v1";

/// 命中规则时给该标签的名义概率质量，其余标签均分剩下的质量。
///
/// 这是**声明常量**而不是校准结果：规则判断器的读数只用于与真实通道做同一任务集上的对照。
pub const TOP_MASS: f64 = 0.8;

/// 规则的一条分支。
#[derive(Debug, Clone, Copy)]
pub struct Rule {
    /// 该分支服务的问题 ID。
    pub question: &'static str,
    /// 全部出现才算命中的标记；为空表示兜底分支。
    pub markers: &'static [&'static str],
    /// 命中后给出的答案意图。
    pub verdict: Verdict,
}

/// 规则给出的答案意图：由问题定义补全成类型化答案（选项集与级数来自问题本身）。
#[derive(Debug, Clone, Copy)]
pub enum Verdict {
    /// 选某个选项。
    Choice(&'static str),
    /// 选某个级号。
    Score(u8),
    /// 给出「是」的概率。
    Noul(f64),
}

/// 初始任务集配套的规则表（顺序即优先级，首个命中生效）。
pub const RULES: &[Rule] = &[
    // failure_kind：日志给出「取消确认之后仍在写缓存」的时间顺序 → 缓存陈旧。
    Rule {
        question: "failure_kind",
        markers: &["取消确认落库", "缓存写入"],
        verdict: Verdict::Choice("cache_stale"),
    },
    Rule {
        question: "failure_kind",
        markers: &["工具调用超时"],
        verdict: Verdict::Choice("tool_timeout"),
    },
    Rule {
        question: "failure_kind",
        markers: &["验收标准", "校验失败"],
        verdict: Verdict::Choice("spec_mismatch"),
    },
    // 没有决定性标记 → 日志不足以归因。
    Rule {
        question: "failure_kind",
        markers: &[],
        verdict: Verdict::Choice("other"),
    },
    // evidence_strength：两段相互独立的记录才有「强」。
    Rule {
        question: "evidence_strength",
        markers: &["取消确认落库", "缓存写入"],
        verdict: Verdict::Score(3),
    },
    Rule {
        question: "evidence_strength",
        markers: &["工具调用超时"],
        verdict: Verdict::Score(2),
    },
    Rule {
        question: "evidence_strength",
        markers: &["验收标准", "校验失败"],
        verdict: Verdict::Score(2),
    },
    Rule {
        question: "evidence_strength",
        markers: &[],
        verdict: Verdict::Score(0),
    },
    // needs_new_observation：日志直接给出因果时不需要新观测，否则先补观测。
    Rule {
        question: "needs_new_observation",
        markers: &["取消确认落库", "缓存写入"],
        verdict: Verdict::Noul(0.1),
    },
    Rule {
        question: "needs_new_observation",
        markers: &["工具调用超时"],
        verdict: Verdict::Noul(0.1),
    },
    Rule {
        question: "needs_new_observation",
        markers: &[],
        verdict: Verdict::Noul(0.9),
    },
];

/// 规则判断器。
pub struct RuleJudge {
    rules: &'static [Rule],
}

impl RuleJudge {
    /// 用内置规则表构造。
    pub fn new() -> Self {
        Self { rules: RULES }
    }
}

impl Default for RuleJudge {
    fn default() -> Self {
        Self::new()
    }
}

impl Judge for RuleJudge {
    fn judge<'a>(
        &'a self,
        request: &'a JudgeRequest,
    ) -> Pin<Box<dyn Future<Output = Result<JudgeResponse, Error>> + Send + 'a>> {
        Box::pin(async move {
            let material = serde_json::to_string(&request.state)
                .map_err(|error| Error::InvalidResponse(format!("state 无法序列化：{error}")))?;

            let mut answers = BTreeMap::new();
            for (id, spec) in &request.questions {
                let rule = self
                    .rules
                    .iter()
                    .find(|rule| rule.question == id.as_str() && rule.matches(&material))
                    .ok_or_else(|| Error::Unavailable(format!("规则表没有覆盖问题 `{id}`")))?;
                answers.insert(id.clone(), rule.verdict.render(&spec.question)?);
            }

            Ok(JudgeResponse {
                source: JudgeSource::Rule,
                version: VERSION.to_owned(),
                answers,
                usage: None,
                request_id: None,
            })
        })
    }
}

impl Rule {
    /// 标记全部出现（且空标记集合恒成立）才算命中。
    pub fn matches(&self, material: &str) -> bool {
        self.markers.iter().all(|marker| material.contains(marker))
    }
}

impl Verdict {
    /// 按问题定义补全成类型化答案；意图与问题原语不符时显式失败（规则表与样本必须同步）。
    pub fn render(self, question: &Question) -> Result<Answer, Error> {
        match (self, question) {
            (Verdict::Choice(option), Question::Choice(spec)) => {
                let options: Vec<&String> = spec.criteria.keys().collect();
                if !spec.criteria.contains_key(option) {
                    return Err(Error::InvalidResponse(format!(
                        "规则给出的选项 `{option}` 不在问题的选项集里"
                    )));
                }
                let other_mass = (1.0 - TOP_MASS) / (options.len().max(2) - 1) as f64;
                let probabilities = options
                    .iter()
                    .map(|name| {
                        let mass = if name.as_str() == option {
                            TOP_MASS
                        } else {
                            other_mass
                        };
                        ((*name).clone(), mass)
                    })
                    .collect::<BTreeMap<_, _>>();
                let confidence =
                    derived_confidence(probabilities.values().copied(), spec.criteria.len());
                Ok(Answer::Choice(ChoiceAnswer {
                    choice: option.to_owned(),
                    probabilities,
                    confidence,
                }))
            }
            (Verdict::Score(level), Question::Score(spec)) => {
                if usize::from(level) >= spec.criteria.len() {
                    return Err(Error::InvalidResponse(format!(
                        "规则给出的级号 {level} 超出问题的 {} 级标尺",
                        spec.criteria.len()
                    )));
                }
                let levels: Vec<u8> = (0..spec.criteria.len() as u8).collect();
                let other_mass = (1.0 - TOP_MASS) / (levels.len().max(2) - 1) as f64;
                let probabilities = levels
                    .iter()
                    .map(|candidate| {
                        let mass = if *candidate == level {
                            TOP_MASS
                        } else {
                            other_mass
                        };
                        (*candidate, mass)
                    })
                    .collect::<BTreeMap<_, _>>();
                let score = probabilities
                    .iter()
                    .map(|(candidate, mass)| f64::from(*candidate) * mass)
                    .sum();
                let confidence =
                    derived_confidence(probabilities.values().copied(), spec.criteria.len());
                Ok(Answer::Score(ScoreAnswer {
                    score,
                    legend: spec
                        .criteria
                        .iter()
                        .enumerate()
                        .map(|(index, text)| (index as u8, text.clone()))
                        .collect(),
                    probabilities,
                    confidence,
                }))
            }
            (Verdict::Noul(noul), Question::Noul(_)) => {
                if !(0.0..=1.0).contains(&noul) {
                    return Err(Error::InvalidResponse(format!(
                        "规则给出的 noul 值 {noul} 不在 0–1 之间"
                    )));
                }
                Ok(Answer::Noul(NoulAnswer { noul }))
            }
            (verdict, question) => Err(Error::InvalidResponse(format!(
                "规则意图与问题原语不符：意图 {verdict:?}，问题 {question:?}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jh_core::{
        ChoiceQuestion, NoulQuestion, QuestionId, QuestionSpec, QuestionVersion, ScoreQuestion,
        SnapshotId, State,
    };

    fn choice_question() -> Question {
        Question::Choice(ChoiceQuestion {
            instructions: "哪一类原因？".to_owned(),
            criteria: BTreeMap::from([
                ("cache_stale".to_owned(), None),
                ("tool_timeout".to_owned(), None),
                ("spec_mismatch".to_owned(), None),
                ("other".to_owned(), None),
            ]),
        })
    }

    fn score_question() -> Question {
        Question::Score(ScoreQuestion {
            instructions: "证据强度？".to_owned(),
            criteria: vec![
                "无".to_owned(),
                "弱".to_owned(),
                "中".to_owned(),
                "强".to_owned(),
            ],
        })
    }

    fn noul_question() -> Question {
        Question::Noul(NoulQuestion {
            instructions: "是否必须新增观测？".to_owned(),
            criteria: None,
        })
    }

    fn request(state: &str, question: Question) -> JudgeRequest {
        JudgeRequest {
            snapshot: SnapshotId::from("case-001"),
            state: State::from(state),
            questions: jh_core::Questions::from([(
                QuestionId::from("q"),
                QuestionSpec {
                    version: QuestionVersion(1),
                    question,
                },
            )])
            .into_iter()
            .collect(),
        }
    }

    /// 测试用的最小 executor：规则判断器不挂起。
    fn block_on<F: Future>(future: F) -> F::Output {
        use std::pin::pin;
        use std::task::{Context, Poll, Waker};

        let mut future = pin!(future);
        let mut context = Context::from_waker(Waker::noop());
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => output,
            Poll::Pending => panic!("规则判断器不应挂起"),
        }
    }

    #[test]
    fn markers_are_matched_against_the_state_json() {
        let rule = Rule {
            question: "q",
            markers: &["取消确认落库", "缓存写入"],
            verdict: Verdict::Choice("cache_stale"),
        };
        let state = State::Object(BTreeMap::from([(
            "log".to_owned(),
            serde_json::json!("09:00:01 取消确认落库\n09:00:05 缓存写入队列仍然写入"),
        )]));
        let material = serde_json::to_string(&state).expect("state 可序列化");

        assert!(rule.matches(&material));
        assert!(
            !Rule {
                markers: &["取消确认落库", "工具调用超时"],
                ..rule
            }
            .matches(&material)
        );
        assert!(
            Rule {
                markers: &[],
                ..rule
            }
            .matches("")
        );
    }

    #[test]
    fn an_uncovered_question_fails_explicitly() {
        let judge = RuleJudge::new();
        // 规则表按问题 ID 索引；`q` 不在表里，因此这份请求必须显式失败。
        let outcome = block_on(judge.judge(&request(
            "09:00:01 取消确认落库\n09:00:05 缓存写入队列仍然写入",
            choice_question(),
        )));

        let error = outcome.expect_err("未覆盖的问题必须显式失败");
        assert!(matches!(error, Error::Unavailable(_)), "{error}");
        assert!(
            error.to_string().contains("规则表没有覆盖问题 `q`"),
            "{error}"
        );
    }

    #[test]
    fn the_full_rule_table_answers_the_initial_questions() {
        let judge = RuleJudge::new();
        let cases: [(&str, &str, &str); 4] = [
            (
                "failure_kind",
                "09:00:01 取消确认落库\n09:00:05 任务 A 的缓存写入队列仍然写入",
                "cache_stale",
            ),
            (
                "failure_kind",
                "09:10:30 工具调用超时：30s 预算内没有返回",
                "tool_timeout",
            ),
            (
                "failure_kind",
                "09:20:01 验收标准要求字段 replicas\n09:20:02 校验失败：清单缺少 replicas",
                "spec_mismatch",
            ),
            ("failure_kind", "09:30:01 任务 D 中止，原因未记录", "other"),
        ];

        for (question, state, expected) in cases {
            let request = JudgeRequest {
                snapshot: SnapshotId::from("case"),
                state: State::from(state),
                questions: jh_core::Questions::from([(
                    QuestionId::from(question),
                    QuestionSpec {
                        version: QuestionVersion(1),
                        question: choice_question(),
                    },
                )]),
            };
            let response = block_on(judge.judge(&request)).expect("规则应覆盖这个问题");
            let Answer::Choice(answer) = &response.answers[&QuestionId::from(question)] else {
                panic!("failure_kind 应是 choice 答案");
            };
            assert_eq!(answer.choice, expected, "state：{state}");
            assert_eq!(response.source, JudgeSource::Rule);
            assert_eq!(response.version, VERSION);
        }
    }

    #[test]
    fn verdicts_are_rendered_against_the_question_definition() {
        let Answer::Choice(choice) = Verdict::Choice("cache_stale")
            .render(&choice_question())
            .expect("选项在问题里")
        else {
            panic!("应渲染成 choice 答案");
        };
        assert_eq!(choice.probabilities["cache_stale"], TOP_MASS);
        let others: Vec<f64> = choice
            .probabilities
            .iter()
            .filter(|(option, _)| option.as_str() != "cache_stale")
            .map(|(_, mass)| *mass)
            .collect();
        assert_eq!(others.len(), 3, "其余 {} 个选项均分剩余质量", others.len());
        for mass in others {
            assert!((mass - (1.0 - TOP_MASS) / 3.0).abs() < 1e-12, "{mass}");
        }

        let Answer::Score(score) = Verdict::Score(3)
            .render(&score_question())
            .expect("级号在标尺内")
        else {
            panic!("应渲染成 score 答案");
        };
        assert_eq!(score.probabilities.len(), 4);
        assert!(score.score > 2.0 && score.score < 3.0);
        assert_eq!(score.legend[&3], "强");

        let Answer::Noul(noul) = Verdict::Noul(0.9)
            .render(&noul_question())
            .expect("noul 值合法")
        else {
            panic!("应渲染成 noul 答案");
        };
        assert_eq!(noul.noul, 0.9);
    }

    #[test]
    fn drifted_rules_fail_explicitly_instead_of_guessing() {
        let error = Verdict::Choice("never_declared")
            .render(&choice_question())
            .expect_err("越界选项必须显式失败");
        assert!(error.to_string().contains("never_declared"), "{error}");

        let error = Verdict::Score(9)
            .render(&score_question())
            .expect_err("越界级号必须显式失败");
        assert!(error.to_string().contains("级号 9"), "{error}");

        let error = Verdict::Choice("cache_stale")
            .render(&noul_question())
            .expect_err("原语不符必须显式失败");
        assert!(error.to_string().contains("原语不符"), "{error}");
    }

    #[test]
    fn the_rule_table_covers_every_initial_question() {
        let task_set = crate::tasks::TaskSet::load_initial().expect("内嵌任务集应自洽");

        for id in task_set.questions.keys() {
            assert!(
                RULES.iter().any(|rule| rule.question == id.as_str()),
                "规则表没有覆盖问题 `{id}`"
            );
        }
    }
}
