//! Offline contract tests for the non-streaming response bodies, against the
//! hand-written fixtures in `crates/jh-openai/fixtures/`.

use std::path::PathBuf;

use jh_openai::{
    ErrorEnvelope, IncompleteReason, ItemStatus, OutputItem, Response, ResponseStatus, Role,
};
use serde_json::Value;

fn fixture_text(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()))
}

fn fixture_value(name: &str) -> Value {
    serde_json::from_str(&fixture_text(name)).expect("fixture is valid json")
}

fn parse(name: &str) -> Response {
    serde_json::from_str(&fixture_text(name))
        .unwrap_or_else(|err| panic!("{name} does not decode: {err}"))
}

/// Asserts that every field `actual` carries exists in `expected` with the same
/// value.
///
/// The crate models a deliberate subset of the payload, so the round-tripped
/// value may have *fewer* fields than the fixture — but every field it does
/// produce must match the documented shape exactly.
fn assert_subset(actual: &Value, expected: &Value, path: &str) {
    match (actual, expected) {
        (Value::Object(actual), Value::Object(expected)) => {
            for (key, value) in actual {
                let Some(expected_value) = expected.get(key) else {
                    panic!("`{path}.{key}` is not present in the fixture");
                };
                assert_subset(value, expected_value, &format!("{path}.{key}"));
            }
        }
        (Value::Array(actual), Value::Array(expected)) => {
            assert_eq!(
                actual.len(),
                expected.len(),
                "`{path}` has a different length"
            );
            for (index, (actual, expected)) in actual.iter().zip(expected).enumerate() {
                assert_subset(actual, expected, &format!("{path}[{index}]"));
            }
        }
        _ => assert_eq!(actual, expected, "`{path}` differs"),
    }
}

#[test]
fn fixtures_round_trip_as_a_subset_of_the_recorded_body() {
    for name in [
        "response_text.json",
        "response_function_call.json",
        "response_incomplete.json",
    ] {
        let response = parse(name);
        let round_tripped = serde_json::to_value(&response).expect("serialises");
        assert_subset(&round_tripped, &fixture_value(name), name);
    }
}

#[test]
fn text_fixture_exposes_message_annotations_and_usage() {
    let response = parse("response_text.json");

    assert_eq!(response.object, "response");
    assert_eq!(response.model, "gpt-5");
    assert_eq!(response.created_at, 1758500100);
    assert_eq!(response.status, ResponseStatus::Completed);
    assert_eq!(response.output_text(), "Rust 的所有权模型会移动值。");
    assert_eq!(response.function_calls().count(), 0);

    let OutputItem::Message {
        role,
        content,
        status,
        ..
    } = &response.output[0]
    else {
        panic!("expected a message item");
    };
    assert_eq!(*role, Role::Assistant);
    assert_eq!(*status, Some(ItemStatus::Completed));
    let jh_openai::ContentPart::OutputText { annotations, .. } = &content[0] else {
        panic!("expected an output_text part");
    };
    assert_eq!(
        annotations,
        &vec![jh_openai::Annotation::UrlCitation {
            url: "https://doc.rust-lang.org/book/ch04-00-understanding-ownership.html".to_owned(),
            title: "Understanding Ownership".to_owned(),
            start_index: 0,
            end_index: 4,
        }]
    );

    let usage = response.usage.expect("usage is reported");
    assert_eq!(usage.input_tokens, 36);
    assert_eq!(usage.output_tokens, 87);
    assert_eq!(usage.total_tokens, 123);
    assert_eq!(usage.input_tokens_details.unwrap().cached_tokens, 0);
    assert_eq!(usage.output_tokens_details.unwrap().reasoning_tokens, 0);
    assert!(response.error.is_none());
    assert!(response.incomplete_details.is_none());
}

#[test]
fn function_call_fixture_keeps_reasoning_and_the_call() {
    let response = parse("response_function_call.json");

    assert_eq!(response.output.len(), 2);
    let OutputItem::Reasoning {
        summary, content, ..
    } = &response.output[0]
    else {
        panic!("expected a reasoning item");
    };
    assert_eq!(summary.len(), 1);
    assert_eq!(content.len(), 1);
    assert_eq!(response.function_calls().count(), 1);
    assert!(response.output_text().is_empty());

    let usage = response.usage.expect("usage is reported");
    assert_eq!(usage.output_tokens_details.unwrap().reasoning_tokens, 19);
}

#[test]
fn incomplete_fixture_carries_the_reason() {
    let response = parse("response_incomplete.json");

    assert_eq!(response.status, ResponseStatus::Incomplete);
    assert_eq!(
        response
            .incomplete_details
            .as_ref()
            .map(|details| details.reason),
        Some(IncompleteReason::MaxOutputTokens)
    );
    // A truncated answer is still readable.
    assert_eq!(response.output_text(), "The answer is 42 because");
}

#[test]
fn error_envelope_fixture_decodes() {
    let envelope: ErrorEnvelope =
        serde_json::from_str(&fixture_text("error_401.json")).expect("decodes");

    assert_eq!(envelope.error.code.as_deref(), Some("invalid_api_key"));
    assert_eq!(
        envelope.error.r#type.as_deref(),
        Some("invalid_request_error")
    );
    assert_eq!(envelope.error.param, None);
    assert!(
        envelope
            .error
            .message
            .starts_with("Incorrect API key provided")
    );
}

#[test]
fn unknown_item_and_status_types_are_survivable() {
    let response: Response = serde_json::from_value(serde_json::json!({
        "id": "resp_x", "object": "response", "created_at": 1, "model": "gpt-5",
        "status": "compacting",
        "output": [
            {"type": "web_search_call", "id": "ws_1", "status": "completed",
             "action": {"type": "search", "query": "rust"}},
            {"type": "image_generation_call", "id": "ig_1", "status": "completed", "result": "aGk="}
        ],
        "usage": null,
        "error": null,
        "incomplete_details": null
    }))
    .expect("unknown item types must not fail the response");

    assert_eq!(response.status, ResponseStatus::Unknown);
    assert_eq!(
        response.output,
        vec![OutputItem::Unknown, OutputItem::Unknown]
    );
}
