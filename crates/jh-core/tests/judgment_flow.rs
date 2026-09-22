//! 端到端流程：判断器可替换（`Box<dyn Judge>`）、来源标签随记录落盘、概率原值不被塌缩。

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use jh_core::{
    Answer, Error, Judge, JudgeRequest, JudgeResponse, JudgeSource, JudgmentRecord, NoulAnswer,
    NoulQuestion, Question, QuestionId, QuestionSpec, QuestionVersion, SnapshotId, State,
};

/// 测试用 executor：仓库不依赖异步运行时，这里的判断器都在首次 poll 就绪。
fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    match future.as_mut().poll(&mut context) {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("测试用的判断器不应该挂起"),
    }
}

/// 按固定概率作答的 Noul 判断器；只用来验证「换后端」与来源标签，不代表任何真实通道。
struct FixedNoul {
    source: JudgeSource,
    version: &'static str,
    noul: f64,
}

impl Judge for FixedNoul {
    fn judge<'a>(
        &'a self,
        request: &'a JudgeRequest,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<JudgeResponse, Error>> + Send + 'a>> {
        Box::pin(async move {
            let mut answers = BTreeMap::new();
            for (id, spec) in &request.questions {
                match &spec.question {
                    Question::Noul(_) => {
                        answers.insert(id.clone(), Answer::Noul(NoulAnswer { noul: self.noul }));
                    }
                    _ => return Err(Error::Unavailable("只支持 Noul".to_owned())),
                }
            }
            Ok(JudgeResponse {
                source: self.source,
                version: self.version.to_owned(),
                answers,
                usage: None,
                request_id: None,
            })
        })
    }
}

/// 什么都不答的判断器：用来验证缺答案时记录构造显式失败。
struct SilentJudge;

impl Judge for SilentJudge {
    fn judge<'a>(
        &'a self,
        _request: &'a JudgeRequest,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<JudgeResponse, Error>> + Send + 'a>> {
        Box::pin(async move {
            Ok(JudgeResponse {
                source: JudgeSource::Llm,
                version: "silent-v1".to_owned(),
                answers: BTreeMap::new(),
                usage: None,
                request_id: None,
            })
        })
    }
}

fn request() -> JudgeRequest {
    JudgeRequest {
        snapshot: SnapshotId::from("case-001"),
        state: State::from("09:00:01 cancel confirmed; 09:00:03 cache write"),
        questions: BTreeMap::from([(
            QuestionId::from("cache_written_after_cancel"),
            QuestionSpec {
                version: QuestionVersion(1),
                question: Question::Noul(NoulQuestion {
                    instructions: "取消确认之后是否还有同一任务的缓存写入？".to_owned(),
                    criteria: None,
                }),
            },
        )]),
    }
}

#[test]
fn judges_are_swappable_and_source_labels_reach_the_record() {
    let request = request();
    let judges: [Box<dyn Judge + Send + Sync>; 2] = [
        Box::new(FixedNoul {
            source: JudgeSource::Rule,
            version: "rule-1",
            noul: 0.42,
        }),
        Box::new(FixedNoul {
            source: JudgeSource::SimulatedTruth,
            version: "sim-1",
            noul: 1.0,
        }),
    ];

    let mut records = Vec::new();
    for judge in judges {
        let response = block_on(judge.judge(&request)).expect("测试判断器不会失败");
        records.push(
            JudgmentRecord::from_response(&request, &response, Duration::from_millis(3))
                .expect("响应满足契约"),
        );
    }

    let question_id = QuestionId::from("cache_written_after_cancel");
    let rule = &records[0];
    let simulated = &records[1];

    assert_eq!(rule.source, JudgeSource::Rule);
    assert_eq!(
        rule.judgments[&question_id].answer,
        Answer::Noul(NoulAnswer { noul: 0.42 })
    );
    assert_eq!(simulated.source, JudgeSource::SimulatedTruth);
    assert_eq!(
        simulated.judgments[&question_id].answer,
        Answer::Noul(NoulAnswer { noul: 1.0 })
    );

    // 落盘后仍能区分真实通道与模拟：来源标签是 JSON 里的显式字段。
    let rule_wire = serde_json::to_value(rule).unwrap();
    let simulated_wire = serde_json::to_value(simulated).unwrap();
    assert_eq!(rule_wire["source"], "rule");
    assert_eq!(simulated_wire["source"], "simulated_truth");
    assert_eq!(
        rule_wire["judgments"]["cache_written_after_cancel"]["answer"]["noul"],
        0.42
    );
}

#[test]
fn incomplete_responses_fail_instead_of_producing_a_partial_record() {
    let error = block_on(SilentJudge.judge(&request()))
        .and_then(|response| {
            JudgmentRecord::from_response(&request(), &response, Duration::ZERO).map(|_| ())
        })
        .expect_err("缺答案必须显式失败");

    assert!(matches!(error, Error::InvalidResponse(_)), "{error}");
    assert!(
        error.to_string().contains("cache_written_after_cancel"),
        "{error}"
    );
}

#[test]
fn unsupported_primitives_fail_explicitly() {
    let request = JudgeRequest {
        snapshot: SnapshotId::from("case-002"),
        state: State::from("..."),
        questions: BTreeMap::from([(
            QuestionId::from("verdict"),
            QuestionSpec {
                version: QuestionVersion(1),
                question: Question::Choice(jh_core::ChoiceQuestion {
                    instructions: "哪一类故障？".to_owned(),
                    criteria: BTreeMap::from([("cert_expired".to_owned(), None)]),
                }),
            },
        )]),
    };

    let error = block_on(
        FixedNoul {
            source: JudgeSource::Rule,
            version: "rule-1",
            noul: 0.0,
        }
        .judge(&request),
    )
    .expect_err("规则判断器不支持 Choice");

    assert!(matches!(error, Error::Unavailable(_)), "{error}");
}
