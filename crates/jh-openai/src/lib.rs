//! A hand-written native Rust client for the **OpenAI Responses API**
//! (`POST {base_url}/responses`) with SSE streaming.
//!
//! This crate speaks the wire protocol itself — no SDK wrapper — and is the
//! only place in the workspace that knows about HTTP, JSON and SSE framing.
//!
//! ```no_run
//! # async fn run() -> Result<(), jh_openai::Error> {
//! use jh_openai::{Client, Event, InputItem, ResponsesRequest};
//!
//! let client = Client::builder()
//!     .model("gpt-5")
//!     .timeout(std::time::Duration::from_secs(120))
//!     .build()?; // reads OPENAI_API_KEY
//!
//! let request = ResponsesRequest::new(vec![InputItem::user_text("Hello")]);
//! let mut stream = client.stream(&request).await?;
//! while let Some(event) = stream.next_event().await? {
//!     if let Event::OutputTextDelta { delta, .. } = event {
//!         print!("{delta}");
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # Contract sources
//!
//! The wire contract was taken from these documents, **checked on 2026-09-22**:
//!
//! - OpenAI OpenAPI specification (`openai-openapi`, `openapi.yaml`,
//!   `info.version` 2.3.0), <https://raw.githubusercontent.com/openai/openai-openapi/master/openapi.yaml>
//! - Responses streaming event union, `openai-python`
//!   (`src/openai/types/responses/response_stream_event.py`),
//!   <https://raw.githubusercontent.com/openai/openai-python/main/src/openai/types/responses/response_stream_event.py>
//! - Responses streaming events reference,
//!   <https://developers.openai.com/api/reference/resources/responses/streaming-events>
//! - Streaming guide, <https://platform.openai.com/docs/guides/streaming-responses>
//! - Error codes, <https://developers.openai.com/api/docs/guides/error-codes>
//!
//! No request was ever sent to the live API while writing this crate; see
//! *Verification status* below for what that means.
//!
//! # Request fields implemented
//!
//! `model`, `input` (array form only: `message`, `function_call`,
//! `function_call_output` items), `instructions`, `tools` (`function` tools
//! only), `tool_choice` (`none` / `auto` / `required` / `{"type":"function"}`),
//! `temperature`, `max_output_tokens`, `stream`.
//!
//! Not implemented, on purpose: the `"input": "text"` shorthand, content parts
//! (`input_text` / `input_image` / `input_file`), built-in and custom tools,
//! `previous_response_id`, `store`, `include`, `reasoning`, `text.format`,
//! `parallel_tool_calls`, `metadata`, `background`, `truncation`.
//!
//! # Response fields implemented
//!
//! `id`, `object`, `created_at`, `model`, `status`, `output`, `usage`, `error`,
//! `incomplete_details`; output items `message` (with `output_text` and
//! `refusal` content parts), `reasoning` (`summary` / `content` /
//! `encrypted_content`) and `function_call`; `usage` with
//! `input_tokens_details.cached_tokens` and `output_tokens_details.reasoning_tokens`.
//!
//! The server echoes many more fields back (`tools`, `instructions`,
//! `metadata`, `temperature`, `service_tier`, ...). They are dropped, and so are
//! `logprobs` on text parts and output items other than the three above — those
//! deserialize to [`OutputItem::Unknown`] rather than failing the response.
//!
//! # SSE events decoded (20 of the 59 documented)
//!
//! `response.created`, `response.in_progress`, `response.queued`,
//! `response.output_item.added`, `response.output_item.done`,
//! `response.content_part.added`, `response.content_part.done`,
//! `response.output_text.delta`, `response.output_text.done`,
//! `response.refusal.delta`, `response.refusal.done`,
//! `response.function_call_arguments.delta`,
//! `response.function_call_arguments.done`,
//! `response.reasoning_summary_part.added`,
//! `response.reasoning_summary_text.delta`,
//! `response.reasoning_summary_text.done`, `response.completed`,
//! `response.failed`, `response.incomplete`, `error`.
//!
//! # SSE events ignored
//!
//! Every other documented event type deserializes to [`Event::Other`] and is
//! skipped by [`ResponseStream::next_event`] (logged at `debug`), so an API
//! addition cannot break the stream:
//!
//! - `response.output_text.annotation.added`
//! - `response.reasoning_summary_part.done`, `response.reasoning_text.delta`,
//!   `response.reasoning_text.done`
//! - `response.file_search_call.{in_progress,searching,completed}`
//! - `response.web_search_call.{in_progress,searching,completed}`
//! - `response.image_generation_call.{in_progress,generating,completed,partial_image}`
//! - `response.mcp_call.{in_progress,completed,failed}`,
//!   `response.mcp_call_arguments.{delta,done}`,
//!   `response.mcp_list_tools.{in_progress,completed,failed}`
//! - `response.code_interpreter_call.{in_progress,interpreting,completed}`,
//!   `response.code_interpreter_call_code.{delta,done}`
//! - `response.custom_tool_call_input.{delta,done}`
//! - `response.compaction.compacting`
//! - `response.shell_call_command.{added,delta,done}`,
//!   `response.shell_call_output_content.{delta,done}`
//! - `response.audio.{delta,done}`, `response.audio.transcript.{delta,done}`
//!
//! # Deliberate deviations
//!
//! - **`[DONE]`**: the Responses API is *not* documented to end its stream with
//!   `data: [DONE]` — it ends with `response.completed`, and both official SDK
//!   decoders additionally stop at `[DONE]` because their Chat Completions
//!   parser is shared. This crate does the same: `[DONE]`, a clean end of
//!   stream, and a terminal event (`response.completed` / `response.failed` /
//!   `response.incomplete`) each end the stream.
//! - **`output_text`** is an SDK-side convenience property in the API docs, not
//!   a documented wire field, so [`Response::output_text`] computes it from the
//!   message parts instead of reading it.
//! - **Unknown fields are ignored** (serde's default). `deny_unknown_fields` is
//!   deliberately *not* used on server payloads: any field the API adds later
//!   would turn into a hard decode failure for no benefit.
//!
//! # Verification status
//!
//! Everything in this crate is verified **offline**, against fixtures in
//! `crates/jh-openai/fixtures/` that were written by hand from the documents
//! listed above — they are *not* recordings of live traffic. `cargo test` makes
//! no network calls. The live smoke tests are `#[ignore]`d and need
//! `OPENAI_API_KEY`. Consequences to keep in mind:
//!
//! - Real payloads may carry fields this crate drops silently; only the fields
//!   above are asserted to exist.
//! - `sequence_number` is modelled as always present (every documented event
//!   declares it required); a server that omits it would fail decoding.
//! - Error mapping is expressed in HTTP status codes, which are documented,
//!   rather than in `error.type` strings, which are not stable.
//! - Retry behaviour on 429 / 5xx is the caller's business; the client surfaces
//!   [`Error::RateLimited`] and [`Error::Server`] with the parsed envelope.

mod client;
mod error;
mod sse;
mod types;

pub use client::{API_KEY_ENV, BASE_URL, Client, ClientBuilder, ResponseStream};
pub use error::{ApiError, Error, ErrorEnvelope};
pub use sse::SseDecoder;
pub use types::{
    Annotation, ContentPart, Event, IncompleteDetails, IncompleteReason, InputItem,
    InputTokensDetails, ItemStatus, OutputItem, OutputTokensDetails, ReasoningPart, Response,
    ResponseError, ResponseStatus, ResponsesRequest, Role, StreamError, Tool, ToolChoice, Usage,
};
