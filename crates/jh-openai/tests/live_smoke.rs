//! Live smoke tests against the real Responses API.
//!
//! Every test here is `#[ignore]`d: `cargo test` never sends a request. To run
//! them, provide credentials through the environment — never through a file:
//!
//! ```text
//! OPENAI_API_KEY=... OPENAI_MODEL=gpt-5 \
//!   cargo test -p jh-openai --test live_smoke -- --ignored --nocapture
//! ```
//!
//! `OPENAI_BASE_URL` may override the endpoint. The API key is read by
//! [`jh_openai::ClientBuilder::build`] and never printed.

use std::time::Duration;

use jh_openai::{Client, Event, InputItem, OutputItem, ResponsesRequest, Tool, ToolChoice};
use serde_json::json;

fn client() -> Client {
    let model = std::env::var("OPENAI_MODEL")
        .expect("set OPENAI_MODEL to the model under test, e.g. OPENAI_MODEL=gpt-5");

    let mut builder = Client::builder()
        .model(model)
        .timeout(Duration::from_secs(120));
    if let Ok(base_url) = std::env::var("OPENAI_BASE_URL") {
        builder = builder.base_url(base_url);
    }
    builder
        .build()
        .expect("set OPENAI_API_KEY; it is read from the environment only")
}

fn weather_tool() -> Tool {
    Tool::function(
        "get_weather",
        "Look up the current weather for a location",
        json!({
            "type": "object",
            "properties": {"location": {"type": "string"}},
            "required": ["location"],
            "additionalProperties": false
        }),
    )
    .strict(true)
}

#[tokio::test]
#[ignore = "sends a real request to the Responses API"]
async fn live_create_returns_a_completed_response() {
    let client = client();
    let response = client
        .create(&ResponsesRequest::new(vec![InputItem::user_text(
            "Reply with exactly the word: pong",
        )]))
        .await
        .expect("the request succeeds");

    println!("{} {} {:?}", response.id, response.model, response.status);
    println!("output_text: {:?}", response.output_text());
    println!("usage: {:?}", response.usage);

    assert_eq!(response.object, "response");
    assert!(
        !response.output_text().is_empty(),
        "the model returned no text"
    );
    assert!(response.usage.is_some(), "usage is reported");
}

#[tokio::test]
#[ignore = "sends a real request to the Responses API"]
async fn live_stream_matches_the_final_text() {
    let client = client();
    let request = ResponsesRequest::new(vec![InputItem::user_text(
        "Count from 1 to 5, digits only.",
    )]);

    let mut stream = client.stream(&request).await.expect("the request succeeds");
    let mut streamed = String::new();
    let mut final_text = None;
    while let Some(event) = stream.next_event().await.expect("events decode") {
        match event {
            Event::OutputTextDelta { delta, .. } => {
                print!("{delta}");
                streamed.push_str(&delta);
            }
            Event::Completed { response, .. } => final_text = Some(response.output_text()),
            other => println!("[{}]", other.kind()),
        }
    }
    println!();

    assert!(!streamed.is_empty(), "no text deltas arrived");
    assert_eq!(
        Some(streamed),
        final_text,
        "deltas must rebuild the final text"
    );
}

#[tokio::test]
#[ignore = "sends a real request to the Responses API"]
async fn live_stream_reports_a_forced_tool_call() {
    let client = client();
    let request =
        ResponsesRequest::new(vec![InputItem::user_text("What is the weather in Paris?")])
            .instructions("Use the provided tool.")
            .tools(vec![weather_tool()])
            .tool_choice(ToolChoice::Required);

    let mut stream = client.stream(&request).await.expect("the request succeeds");
    let mut arguments = String::new();
    let mut called = None;
    while let Some(event) = stream.next_event().await.expect("events decode") {
        match event {
            Event::FunctionCallArgumentsDelta { delta, .. } => arguments.push_str(&delta),
            Event::OutputItemDone {
                item: OutputItem::FunctionCall { name, call_id, .. },
                ..
            } => called = Some((name, call_id)),
            _ => {}
        }
    }

    println!(
        "{} {arguments}",
        called
            .as_ref()
            .map(|(name, _)| name.as_str())
            .unwrap_or("-")
    );
    let (name, call_id) = called.expect("tool_choice=required must produce a function call");
    assert_eq!(name, "get_weather");
    assert!(!call_id.is_empty());
    let parsed: serde_json::Value = serde_json::from_str(&arguments).expect("arguments are json");
    assert!(parsed.get("location").is_some(), "arguments: {arguments}");
}
