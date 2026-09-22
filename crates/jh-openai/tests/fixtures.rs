//! Offline streaming tests: the recorded SSE fixtures replayed through the real
//! [`ResponseStream`], split into chunks at deliberately awkward boundaries.
//!
//! Nothing here touches the network.

use std::path::PathBuf;

use bytes::Bytes;
use futures::stream;
use jh_openai::{Event, OutputItem, ReasoningPart, ResponseStatus, ResponseStream};

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()))
}

/// Replays `bytes` as a stream of `chunk_size`-byte chunks, the way a server
/// (or a proxy) is free to split the body.
fn replay(bytes: &[u8], chunk_size: usize) -> ResponseStream {
    let chunks: Vec<reqwest::Result<Bytes>> = bytes
        .chunks(chunk_size)
        .map(|chunk| Ok(Bytes::copy_from_slice(chunk)))
        .collect();
    ResponseStream::from_chunks(stream::iter(chunks))
}

async fn collect(bytes: &[u8], chunk_size: usize) -> Vec<Event> {
    let mut stream = replay(bytes, chunk_size);
    let mut events = Vec::new();
    while let Some(event) = stream.next_event().await.expect("stream decodes") {
        events.push(event);
    }
    events
}

/// Every `response.output_text.delta` payload, concatenated.
fn text(events: &[Event]) -> String {
    events
        .iter()
        .filter_map(|event| match event {
            Event::OutputTextDelta { delta, .. } => Some(delta.as_str()),
            _ => None,
        })
        .collect()
}

/// Every `response.function_call_arguments.delta` payload, concatenated.
fn arguments(events: &[Event]) -> String {
    events
        .iter()
        .filter_map(|event| match event {
            Event::FunctionCallArgumentsDelta { delta, .. } => Some(delta.as_str()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn decodes_the_text_fixture_in_order() {
    let bytes = fixture("stream_text.sse");
    let events = collect(&bytes, bytes.len()).await;

    assert!(matches!(events.first(), Some(Event::Created { .. })));
    assert!(matches!(events.last(), Some(Event::Completed { .. })));
    assert_eq!(text(&events), "Rust 的所有权模型会移动值。");

    // 12 frames in the fixture, one of which is the unknown
    // `response.output_text.annotation.added`: it is skipped, not returned and
    // not fatal.
    assert_eq!(events.len(), 11);
    assert!(events.iter().all(|event| *event != Event::Other));

    let Some(Event::Completed { response, .. }) = events.last() else {
        unreachable!()
    };
    assert_eq!(response.status, ResponseStatus::Completed);
    assert_eq!(response.output_text(), "Rust 的所有权模型会移动值。");
    let usage = response.usage.as_ref().expect("usage is reported");
    assert_eq!(
        (usage.input_tokens, usage.output_tokens, usage.total_tokens),
        (36, 87, 123)
    );
}

#[tokio::test]
async fn text_fixture_is_chunk_boundary_agnostic() {
    let bytes = fixture("stream_text.sse");
    // The fixture is one contiguous byte stream — where the network would cut it
    // is up to the server. Chunk size 1 cuts inside every line and inside every
    // multi-byte character; the larger sizes cut at line starts, line ends and
    // frame separators. The decoded events must not depend on any of that.
    for chunk_size in [1, 2, 3, 7, 13, 64, 997, bytes.len()] {
        let events = collect(&bytes, chunk_size).await;
        assert_eq!(events.len(), 11, "chunk size {chunk_size}");
        assert_eq!(
            text(&events),
            "Rust 的所有权模型会移动值。",
            "chunk size {chunk_size}"
        );
    }
}

#[tokio::test]
async fn reassembles_function_call_arguments() {
    let bytes = fixture("stream_function_call.sse");
    let events = collect(&bytes, 5).await;

    assert_eq!(arguments(&events), r#"{"location":"Paris"}"#);

    let reasoning = events
        .iter()
        .find_map(|event| match event {
            Event::ReasoningSummaryTextDone { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .expect("reasoning summary is streamed");
    assert_eq!(reasoning, "用户想查天气，需要调用工具。");

    let Some(Event::Completed { response, .. }) = events.last() else {
        panic!("expected a completed event, got {:?}", events.last());
    };
    let calls: Vec<&OutputItem> = response.function_calls().collect();
    assert_eq!(calls.len(), 1);
    let OutputItem::FunctionCall {
        call_id,
        name,
        arguments,
        status,
        ..
    } = calls[0]
    else {
        panic!("expected a function_call item");
    };
    assert_eq!(
        (call_id.as_str(), name.as_str()),
        ("call_abc", "get_weather")
    );
    assert_eq!(arguments, r#"{"location":"Paris"}"#);
    assert_eq!(*status, Some(jh_openai::ItemStatus::Completed));
    assert!(response.output_text().is_empty());
}

#[tokio::test]
async fn reasoning_item_keeps_summary_and_content() {
    let bytes = fixture("stream_function_call.sse");
    let events = collect(&bytes, bytes.len()).await;

    let summary_part = events
        .iter()
        .find_map(|event| match event {
            Event::ReasoningSummaryPartAdded { part, .. } => Some(part.clone()),
            _ => None,
        })
        .expect("a summary part is announced");
    assert_eq!(
        summary_part,
        ReasoningPart::SummaryText {
            text: String::new()
        }
    );

    let done = events
        .iter()
        .find_map(|event| match event {
            Event::OutputItemDone { item, .. } => Some(item.clone()),
            _ => None,
        })
        .expect("the reasoning item is finished");
    let OutputItem::Reasoning {
        summary, content, ..
    } = done
    else {
        panic!("expected a reasoning item, got {done:?}");
    };
    assert_eq!(
        summary,
        vec![ReasoningPart::SummaryText {
            text: "用户想查天气，需要调用工具。".to_owned()
        }]
    );
    assert_eq!(
        content,
        vec![ReasoningPart::ReasoningText {
            text: "用户询问巴黎的天气。".to_owned()
        }]
    );
}

#[tokio::test]
async fn error_event_and_failed_response_end_the_stream() {
    let bytes = fixture("stream_error.sse");
    let events = collect(&bytes, 11).await;

    // response.created, error, response.failed — the unknown
    // `response.experimental.ping` is skipped.
    assert_eq!(events.len(), 3, "{events:#?}");

    let Some(Event::Error(error)) = events.get(1) else {
        panic!("expected an error event, got {:?}", events.get(1));
    };
    assert_eq!(error.code.as_deref(), Some("server_error"));
    assert_eq!(error.sequence_number, 2);

    let Some(Event::Failed { response, .. }) = events.last() else {
        panic!("expected a failed event, got {:?}", events.last());
    };
    assert_eq!(response.status, ResponseStatus::Failed);
    assert_eq!(
        response
            .error
            .as_ref()
            .and_then(|error| error.code.as_deref()),
        Some("server_error")
    );
    assert!(response.usage.is_none());
}

#[tokio::test]
async fn done_sentinel_terminates_the_stream() {
    let mut stream = replay(b"data: [DONE]\n\n", 4);
    assert_eq!(stream.next_event().await.unwrap(), None);
}

#[tokio::test]
async fn stream_that_stops_without_the_final_blank_line_ends_cleanly() {
    // The SSE specification discards an unterminated final frame, and the
    // stream ends without error either way.
    let payload = br#"data: {"type":"response.completed","sequence_number":1,"response":{"id":"r","object":"response","created_at":1,"model":"m","status":"completed","output":[]}}"#;
    let mut stream = replay(payload, 16);
    assert_eq!(stream.next_event().await.unwrap(), None);
}

#[tokio::test]
async fn malformed_json_is_a_decode_error() {
    let mut stream = replay(b"data: {\"type\": \"response.completed\"\n\n", 8);
    let err = stream.next_event().await.unwrap_err();
    assert!(matches!(
        err,
        jh_openai::Error::Decode {
            context: "sse event",
            ..
        }
    ));
}

#[tokio::test]
async fn a_failing_transport_surfaces_as_a_transport_error() {
    // An unusable URL fails before any request is sent, so no network is used.
    let failure = reqwest::Client::new()
        .get("not a url")
        .send()
        .await
        .expect_err("the URL cannot be parsed");

    let chunks: Vec<reqwest::Result<Bytes>> = vec![Err(failure)];
    let mut stream = ResponseStream::from_chunks(stream::iter(chunks));
    let err = stream.next_event().await.unwrap_err();
    assert!(matches!(err, jh_openai::Error::Transport(_)));
}
