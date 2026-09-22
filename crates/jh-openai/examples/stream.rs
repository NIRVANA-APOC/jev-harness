//! Streaming demo: offline replay of a fixture, or a live request.
//!
//! ```text
//! # 离线回放（默认）：不联网，不需要任何密钥
//! cargo run -p jh-openai --example stream
//! cargo run -p jh-openai --example stream -- replay stream_function_call.sse
//!
//! # 真调：需要环境变量
//! OPENAI_API_KEY=... OPENAI_MODEL=gpt-5 \
//!   cargo run -p jh-openai --example stream -- live "What is the weather in Paris?"
//! ```
//!
//! Both modes feed the same [`ResponseStream`]; replay just substitutes the byte
//! chunks of a file for the ones an HTTP body would deliver.

use std::path::PathBuf;

use bytes::Bytes;
use futures::stream;
use jh_openai::{
    Client, Event, InputItem, OutputItem, ReasoningPart, ResponseStream, ResponsesRequest, Tool,
    ToolChoice,
};
use serde_json::json;

/// Deliberately small and not line-aligned: frames arrive split mid-line.
const REPLAY_CHUNK_SIZE: usize = 13;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("live") => {
            let prompt = args
                .get(1)
                .map(String::as_str)
                .unwrap_or("What is the weather in Paris?");
            live(prompt).await
        }
        Some("replay") => {
            replay(args.get(1).map(String::as_str).unwrap_or("stream_text.sse")).await
        }
        Some(other) => {
            Err(format!("unknown mode `{other}`; use `replay [fixture]` or `live [prompt]`").into())
        }
        None => replay("stream_text.sse").await,
    }
}

/// Offline: replays a recorded fixture through the real stream type.
async fn replay(name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name);
    let bytes = std::fs::read(&path)?;
    println!(
        "replaying {} ({} bytes, {REPLAY_CHUNK_SIZE}-byte chunks)\n",
        path.display(),
        bytes.len()
    );

    let chunks: Vec<reqwest::Result<Bytes>> = bytes
        .chunks(REPLAY_CHUNK_SIZE)
        .map(|chunk| Ok(Bytes::copy_from_slice(chunk)))
        .collect();

    consume(ResponseStream::from_chunks(stream::iter(chunks))).await
}

/// Live: one streaming request with a function tool attached.
async fn live(prompt: &str) -> Result<(), Box<dyn std::error::Error>> {
    let model = std::env::var("OPENAI_MODEL")?;
    let mut builder = Client::builder()
        .model(model)
        .timeout(std::time::Duration::from_secs(120));
    if let Ok(base_url) = std::env::var("OPENAI_BASE_URL") {
        builder = builder.base_url(base_url);
    }
    let client = builder.build()?;

    println!("streaming from {} ({})", client.base_url(), client.model());
    let request = ResponsesRequest::new(vec![InputItem::user_text(prompt)])
        .instructions(
            "Answer in one short sentence. Use the weather tool when the user asks about weather.",
        )
        .tools(vec![
            Tool::function(
                "get_weather",
                "Look up the current weather for a location",
                json!({
                    "type": "object",
                    "properties": { "location": { "type": "string" } },
                    "required": ["location"],
                    "additionalProperties": false
                }),
            )
            .strict(true),
        ])
        .tool_choice(ToolChoice::Auto);

    consume(client.stream(&request).await?).await
}

/// Prints the stream and keeps the interesting bits.
async fn consume(mut stream: ResponseStream) -> Result<(), Box<dyn std::error::Error>> {
    let mut text = String::new();
    let mut arguments = String::new();

    while let Some(event) = stream.next_event().await? {
        match &event {
            Event::OutputTextDelta { delta, .. } => {
                print!("{delta}");
                text.push_str(delta);
            }
            Event::ReasoningSummaryTextDelta { delta, .. } => println!("[reasoning] {delta}"),
            Event::FunctionCallArgumentsDelta { delta, .. } => arguments.push_str(delta),
            Event::OutputItemDone { item, .. } => println!("\n[item done] {}", describe(item)),
            Event::Error(error) => println!("\n[error] {error:?}"),
            other => println!("[{}] seq {}", other.kind(), sequence_number(other)),
        }
    }

    println!("\n--- text: {text:?}");
    if !arguments.is_empty() {
        println!("--- function arguments: {arguments}");
    }
    Ok(())
}

fn sequence_number(event: &Event) -> u64 {
    match event {
        Event::Created {
            sequence_number, ..
        }
        | Event::InProgress {
            sequence_number, ..
        }
        | Event::Queued {
            sequence_number, ..
        }
        | Event::OutputItemAdded {
            sequence_number, ..
        }
        | Event::OutputItemDone {
            sequence_number, ..
        }
        | Event::ContentPartAdded {
            sequence_number, ..
        }
        | Event::ContentPartDone {
            sequence_number, ..
        }
        | Event::OutputTextDelta {
            sequence_number, ..
        }
        | Event::OutputTextDone {
            sequence_number, ..
        }
        | Event::RefusalDelta {
            sequence_number, ..
        }
        | Event::RefusalDone {
            sequence_number, ..
        }
        | Event::FunctionCallArgumentsDelta {
            sequence_number, ..
        }
        | Event::FunctionCallArgumentsDone {
            sequence_number, ..
        }
        | Event::ReasoningSummaryTextDelta {
            sequence_number, ..
        }
        | Event::ReasoningSummaryTextDone {
            sequence_number, ..
        }
        | Event::ReasoningSummaryPartAdded {
            sequence_number, ..
        }
        | Event::Completed {
            sequence_number, ..
        }
        | Event::Failed {
            sequence_number, ..
        }
        | Event::Incomplete {
            sequence_number, ..
        } => *sequence_number,
        Event::Error(error) => error.sequence_number,
        Event::Other => 0,
    }
}

fn describe(item: &OutputItem) -> String {
    match item {
        OutputItem::Message { id, .. } => format!("message {id}"),
        OutputItem::Reasoning { id, summary, .. } => {
            let text = summary
                .iter()
                .map(|part| match part {
                    ReasoningPart::SummaryText { text } => text.as_str(),
                    ReasoningPart::ReasoningText { text } => text.as_str(),
                    ReasoningPart::Unknown => "?",
                })
                .collect::<String>();
            format!("reasoning {id} {text:?}")
        }
        OutputItem::FunctionCall { name, call_id, .. } => {
            format!("function_call {name} ({call_id})")
        }
        OutputItem::Unknown => "unknown item type".to_owned(),
    }
}
