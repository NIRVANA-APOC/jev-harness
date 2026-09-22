//! 官方示例 fixture 的 serde 往返与容错解析断言（全部离线：只读本 crate 的 `fixtures/`）。
//!
//! fixture 来源（核对日期 2026-09-22）：
//!
//! - `quickstart.request.json` / `quickstart.response.json`：<https://docs.typesafe.ai/introduction/quickstart>
//!   的「三类原语同请求」示例；
//! - `choice.*` / `score.*` / `noul.*`：<https://docs.typesafe.ai/api> 的三类原语最小示例。
//!
//! 响应里的数值是官方文档的示意值，不可当基准数据。

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use jh_jev::{
    Answer, ChoiceQuestion, Instructions, NoulQuestion, Question, ScoreQuestion, State,
    SystemOneRequest, SystemOneResponse,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name);
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("读取 {} 失败：{error}", path.display()))
}

/// 反序列化再序列化，形状必须与原 fixture 完全一致。
fn round_trip<T>(name: &str) -> T
where
    T: DeserializeOwned + Serialize,
{
    let raw: Value = serde_json::from_str(&fixture(name)).expect("fixture 是合法 JSON");
    let parsed: T = serde_json::from_value(raw.clone()).expect("fixture 能解析成目标类型");
    let back = serde_json::to_value(&parsed).expect("能序列化回 JSON");
    assert_eq!(back, raw, "{name} 往返后形状不一致");
    parsed
}

fn choice_question(question: &Question) -> &ChoiceQuestion {
    match question {
        Question::Choice(choice) => choice,
        other => panic!("期望 choice 问题，得到 {other:?}"),
    }
}

fn score_question(question: &Question) -> &ScoreQuestion {
    match question {
        Question::Score(score) => score,
        other => panic!("期望 score 问题，得到 {other:?}"),
    }
}

fn noul_question(question: &Question) -> &NoulQuestion {
    match question {
        Question::Noul(noul) => noul,
        other => panic!("期望 noul 问题，得到 {other:?}"),
    }
}

fn noul_question_value(description: &str) -> Question {
    Question::from(NoulQuestion {
        instructions: description.into(),
        criteria: None,
    })
}

#[test]
fn quickstart_request_round_trips() {
    let request: SystemOneRequest = round_trip("quickstart.request.json");
    assert_eq!(request.model, "jev-latest");
    assert_eq!(
        request.state,
        State::from(
            "Hi, I've been trying to connect my Stripe account for 3 days and the integration \
             keeps failing. I'm losing sales. Please help ASAP."
        )
    );

    let choice = choice_question(&request.questions["department"]);
    assert_eq!(choice.criteria.len(), 3);
    assert_eq!(
        choice.criteria["billing"],
        Some(Instructions::from("Payment or subscription issues"))
    );

    let score = score_question(&request.questions["frustration"]);
    assert_eq!(score.criteria.len(), 3);

    let noul = noul_question(&request.questions["is_urgent"]);
    assert_eq!(
        noul.instructions,
        Instructions::from("The message conveys urgency or time-sensitivity")
    );
    assert_eq!(noul.criteria, None);
}

#[test]
fn quickstart_response_round_trips() {
    let response: SystemOneResponse = round_trip("quickstart.response.json");
    assert_eq!(response.model, "jev-1.13.0");
    assert_eq!(response.usage.input_tokens, Some(392));
    assert_eq!(response.usage.output_tokens, Some(65));
    assert_eq!(response.request_id, None, "直接反序列化时没有响应头可读");
    assert_eq!(response.answers.len(), 3);

    let choice = response.choice("department").expect("department 是 choice");
    assert_eq!(choice.choice, "technical");
    assert_eq!(choice.confidence, 0.78);
    assert_eq!(choice.probabilities["technical"], 0.85);
    assert_eq!(choice.probabilities["billing"], 0.15);

    let score = response.score("frustration").expect("frustration 是 score");
    assert_eq!(score.score, 1.0);
    assert_eq!(score.confidence, 1.0);
    assert_eq!(score.probabilities[&1], 1.0);

    let noul = response.noul("is_urgent").expect("is_urgent 是 noul");
    assert_eq!(noul.noul, 1.0);

    assert_eq!(response.noul("department"), None, "类型不符时不串数据");
    assert_eq!(response.choice("is_urgent"), None);
    assert_eq!(response.score("is_urgent"), None);
    assert_eq!(response.answer("missing"), None);
}

#[test]
fn choice_fixture_round_trips() {
    let request: SystemOneRequest = round_trip("choice.request.json");
    let question = choice_question(&request.questions["department"]);
    assert_eq!(question.criteria.len(), 3);

    let response: SystemOneResponse = round_trip("choice.response.json");
    let answer = response.choice("department").expect("department 是 choice");
    assert_eq!(answer.choice, "billing");
    assert_eq!(answer.confidence, 0.81);
    assert_eq!(answer.probabilities.len(), 3);
    assert_eq!(answer.probabilities["sales"], 0.0);
}

#[test]
fn score_fixture_round_trips() {
    let request: SystemOneRequest = round_trip("score.request.json");
    let question = score_question(&request.questions["frustration"]);
    assert_eq!(question.criteria.len(), 3);
    assert_eq!(question.criteria[0], Instructions::from("Calm"));

    let response: SystemOneResponse = round_trip("score.response.json");
    let answer = response.score("frustration").expect("frustration 是 score");
    assert_eq!(answer.score, 1.05);
    assert_eq!(answer.confidence, 0.92);

    // wire 上是字符串级号，类型层是整数 key（官方 Python SDK 同样用整数 key）。
    assert_eq!(answer.legend.keys().copied().collect::<Vec<_>>(), [0, 1, 2]);
    assert_eq!(answer.legend[&2], "Very angry");
    assert_eq!(
        answer.probabilities.keys().copied().collect::<Vec<_>>(),
        [0, 1, 2]
    );
    assert_eq!(answer.probabilities[&1], 0.95);

    let serialized = serde_json::to_value(&response).expect("能序列化");
    assert_eq!(
        serialized["answers"]["frustration"]["probabilities"]["0"],
        0.0
    );
    assert_eq!(
        serialized["answers"]["frustration"]["legend"]["1"],
        "Frustrated"
    );
}

#[test]
fn noul_fixture_round_trips() {
    let request: SystemOneRequest = round_trip("noul.request.json");
    let question = noul_question(&request.questions["is_urgent"]);
    let criteria = question.criteria.as_ref().expect("fixture 带 criteria");
    assert_eq!(
        criteria.r#true,
        Some(Instructions::from("Explicitly time-sensitive"))
    );
    assert_eq!(
        criteria.r#false,
        Some(Instructions::from("No urgency expressed"))
    );

    let response: SystemOneResponse = round_trip("noul.response.json");
    let answer = response.noul("is_urgent").expect("is_urgent 是 noul");
    assert_eq!(answer.noul, 0.95);
    assert!(matches!(
        response.answer("is_urgent"),
        Some(Answer::Noul(_))
    ));
}

#[test]
fn unknown_answer_type_does_not_fail_the_response() {
    let raw = json!({
        "model": "jev-1.14.0",
        "answers": {
            "department": {
                "type": "choice",
                "choice": "billing",
                "probabilities": { "billing": 1.0 },
                "confidence": 1.0
            },
            "ranking": { "type": "ranking", "order": ["a", "b"] }
        },
        "usage": { "input_tokens": 1, "output_tokens": 2 }
    });

    let response: SystemOneResponse =
        serde_json::from_value(raw).expect("未知 type 不该让整条响应解析失败");

    assert_eq!(response.answer("ranking"), Some(&Answer::Unknown));
    assert_eq!(
        response
            .choice("department")
            .expect("其余答案照常可用")
            .choice,
        "billing"
    );
}

#[test]
fn unknown_question_type_falls_back_too() {
    let raw = json!({
        "state": "text",
        "model": "jev-1.13.0",
        "questions": {
            "topic": { "type": "choice", "instructions": "pick", "criteria": { "a": null } },
            "future": { "type": "ranking", "instructions": "order these" }
        }
    });

    let request: SystemOneRequest =
        serde_json::from_value(raw).expect("未知 type 不该让整条请求解析失败");

    assert_eq!(request.questions["future"], Question::Unknown);
    assert!(matches!(request.questions["topic"], Question::Choice(_)));
}

#[test]
fn unknown_fields_are_ignored() {
    // 服务端扩展字段（例如以后给 Noul 补上 confidence）不会让响应解析失败；
    // 代价是这类扩展不会被察觉，取舍见 docs/jev-research.md §10.3。
    let raw = json!({
        "model": "jev-1.13.0",
        "answers": {
            "is_urgent": { "type": "noul", "noul": 0.4, "confidence": 0.9 }
        },
        "usage": { "input_tokens": 10, "output_tokens": 1, "cached_tokens": 3 },
        "future_field": true
    });

    let response: SystemOneResponse =
        serde_json::from_value(raw).expect("未知字段不该让响应解析失败");
    assert_eq!(response.noul("is_urgent").expect("noul 仍在").noul, 0.4);
    assert_eq!(response.usage.input_tokens, Some(10));
}

#[test]
fn choice_criteria_allows_null_descriptions() {
    let raw = json!({
        "state": "text",
        "model": "jev-1.13.0",
        "questions": {
            "topic": {
                "type": "choice",
                "instructions": "pick one",
                "criteria": { "billing": null, "technical": "Bugs or outages" }
            }
        }
    });

    let request: SystemOneRequest = serde_json::from_value(raw.clone()).expect("null 描述合法");
    let question = choice_question(&request.questions["topic"]);
    assert_eq!(question.criteria["billing"], None);
    assert_eq!(
        question.criteria["technical"],
        Some(Instructions::from("Bugs or outages"))
    );
    assert_eq!(serde_json::to_value(&request).expect("能序列化"), raw);
}

#[test]
fn noul_without_criteria_omits_the_field() {
    let mut questions = BTreeMap::new();
    questions.insert("is_urgent".to_owned(), noul_question_value("urgent?"));

    let request = SystemOneRequest::new(State::from("help"), questions);

    assert_eq!(request.model, "jev-1.13.0", "默认模型 pin 到版本化 ID");
    assert_eq!(
        serde_json::to_value(&request).expect("能序列化"),
        json!({
            "state": "help",
            "model": "jev-1.13.0",
            "questions": { "is_urgent": { "type": "noul", "instructions": "urgent?" } }
        })
    );
}

#[test]
fn structured_instructions_and_object_state_round_trip() {
    let raw = json!({
        "state": { "ticket": { "messages": [{ "text": "refund please" }] }, "policy": "refunds within 30 days" },
        "model": "jev-1.13.0",
        "questions": {
            "refund": {
                "type": "noul",
                "instructions": {
                    "question": "Does `ticket.messages[0].text` ask for a refund?",
                    "policy": "`policy`"
                }
            }
        }
    });

    let request: SystemOneRequest = serde_json::from_value(raw.clone()).expect("结构化写法合法");
    assert!(matches!(request.state, State::Object(_)));
    assert!(matches!(
        noul_question(&request.questions["refund"]).instructions,
        Instructions::Structured(_)
    ));
    assert_eq!(serde_json::to_value(&request).expect("能序列化"), raw);
}

#[test]
fn state_messages_accept_text_values_and_records() {
    let raw = json!(["Hi", { "role": "user", "content": "my card was charged twice" }]);

    let state: State = serde_json::from_value(raw.clone()).expect("消息序列合法");
    let State::Messages(entries) = &state else {
        panic!("期望 Messages，得到 {state:?}");
    };
    assert_eq!(entries.len(), 2);
    assert_eq!(serde_json::to_value(&state).expect("能序列化"), raw);
}

#[test]
fn usage_is_optional_and_nullable() {
    let raw = json!({
        "model": "jev-1.13.0",
        "answers": { "is_urgent": { "type": "noul", "noul": 0.5 } },
        "usage": { "input_tokens": null, "output_tokens": null }
    });

    let response: SystemOneResponse = serde_json::from_value(raw).expect("usage 允许 null");
    assert_eq!(response.usage.input_tokens, None);
    assert_eq!(response.usage.output_tokens, None);
}
