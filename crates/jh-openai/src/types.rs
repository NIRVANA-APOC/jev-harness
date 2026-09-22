//! Wire types of the OpenAI Responses API (`POST /v1/responses`).
//!
//! Only the fields this project needs are modelled; every other field of the
//! server's payloads is silently ignored (see the crate docs for the full
//! coverage and ignore lists).
//!
//! ```no_run
//! use jh_openai::{InputItem, ResponsesRequest, Tool, ToolChoice};
//! use serde_json::json;
//!
//! let request = ResponsesRequest::new(vec![InputItem::user_text("Weather in Paris?")])
//!     .model("gpt-5")
//!     .tools(vec![Tool::function(
//!         "get_weather",
//!         "Look up the current weather",
//!         json!({"type": "object", "properties": {"location": {"type": "string"}}}),
//!     )])
//!     .tool_choice(ToolChoice::Auto);
//!
//! let body = serde_json::to_value(&request).unwrap();
//! assert_eq!(body["tool_choice"], "auto");
//! assert_eq!(body["tools"][0]["type"], "function");
//! ```

use serde::{Deserialize, Serialize, Serializer, ser::SerializeMap};
use serde_json::Value;

// ---------------------------------------------------------------- request ---

/// Body of `POST /v1/responses`.
///
/// `model` is left optional here: when it is not set on the request, [`Client`]
/// fills in its configured default. Everything else is serialised as-is.
///
/// [`Client`]: crate::Client
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResponsesRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    input: Vec<InputItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    instructions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<Tool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<ToolChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

impl ResponsesRequest {
    /// A request carrying only the conversation input.
    pub fn new(input: Vec<InputItem>) -> Self {
        Self {
            model: None,
            input,
            instructions: None,
            tools: None,
            tool_choice: None,
            temperature: None,
            max_output_tokens: None,
            stream: None,
        }
    }

    /// Overrides the client's default model for this request.
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// System-level instructions for this response.
    pub fn instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Some(instructions.into());
        self
    }

    /// Tools the model may call.
    pub fn tools(mut self, tools: Vec<Tool>) -> Self {
        self.tools = Some(tools);
        self
    }

    /// How the model may choose a tool.
    pub fn tool_choice(mut self, tool_choice: ToolChoice) -> Self {
        self.tool_choice = Some(tool_choice);
        self
    }

    /// Sampling temperature. `f64`, so that a value like `0.2` serialises as
    /// `0.2` rather than as an `f32`-widened approximation.
    pub fn temperature(mut self, temperature: f64) -> Self {
        self.temperature = Some(temperature);
        self
    }

    pub fn max_output_tokens(mut self, max_output_tokens: u32) -> Self {
        self.max_output_tokens = Some(max_output_tokens);
        self
    }

    /// Sets the `stream` flag. [`Client::stream`] always sends `true` here.
    ///
    /// [`Client::stream`]: crate::Client::stream
    pub fn stream(mut self, stream: bool) -> Self {
        self.stream = Some(stream);
        self
    }

    /// Fills in the client's default model and the streaming flag.
    pub(crate) fn prepare(&mut self, default_model: &str, stream: bool) {
        if self.model.is_none() {
            self.model = Some(default_model.to_owned());
        }
        self.stream = Some(stream);
    }
}

impl From<Vec<InputItem>> for ResponsesRequest {
    fn from(input: Vec<InputItem>) -> Self {
        Self::new(input)
    }
}

/// One item of the `input` array.
///
/// The array is the only form implemented: the alternative `"input": "text"`
/// shorthand would not be able to express tool results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InputItem {
    /// An `EasyInputMessage`: `{"type":"message","role":...,"content":"..."}`.
    ///
    /// `content` is serialised as a plain string. The array-of-content-parts
    /// form (`input_text` / `input_image` / `input_file`) is not implemented.
    Message { role: Role, content: String },

    /// A tool call the model made in an earlier turn, replayed back to it.
    FunctionCall {
        call_id: String,
        name: String,
        arguments: String,
    },

    /// The result of a tool call, keyed by the `call_id` it answers.
    FunctionCallOutput { call_id: String, output: String },
}

impl InputItem {
    pub fn user_text(text: impl Into<String>) -> Self {
        InputItem::Message {
            role: Role::User,
            content: text.into(),
        }
    }

    pub fn assistant_text(text: impl Into<String>) -> Self {
        InputItem::Message {
            role: Role::Assistant,
            content: text.into(),
        }
    }

    /// Replays a `function_call` output item back into the input.
    pub fn function_call(
        name: impl Into<String>,
        call_id: impl Into<String>,
        arguments: impl Into<String>,
    ) -> Self {
        InputItem::FunctionCall {
            call_id: call_id.into(),
            name: name.into(),
            arguments: arguments.into(),
        }
    }

    /// Wraps the text result of a tool call into an input item.
    pub fn function_call_output(call_id: impl Into<String>, output: impl Into<String>) -> Self {
        InputItem::FunctionCallOutput {
            call_id: call_id.into(),
            output: output.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    Developer,
    User,
    Assistant,
}

/// One entry of `tools`.
///
/// Only function tools are implemented; built-in tools (web search, file
/// search, MCP, code interpreter, ...) and custom tools are not.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Tool {
    Function {
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        /// JSON Schema of the arguments object.
        parameters: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        strict: Option<bool>,
    },
}

impl Tool {
    /// A function tool. Unlike Chat Completions, the Responses API expects the
    /// function fields flat next to `"type":"function"`.
    pub fn function(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: Value,
    ) -> Self {
        Tool::Function {
            name: name.into(),
            description: Some(description.into()),
            parameters,
            strict: None,
        }
    }

    /// Requires the model's arguments to validate strictly against `parameters`.
    pub fn strict(mut self, strict: bool) -> Self {
        let Tool::Function { strict: slot, .. } = &mut self;
        *slot = Some(strict);
        self
    }

    pub fn name(&self) -> &str {
        let Tool::Function { name, .. } = self;
        name
    }
}

/// Value of `tool_choice`.
///
/// The modes serialise as bare strings, the function form as an object; that
/// mix is why this enum implements [`Serialize`] by hand instead of deriving it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolChoice {
    None,
    Auto,
    Required,
    Function { name: String },
}

impl ToolChoice {
    pub fn function(name: impl Into<String>) -> Self {
        ToolChoice::Function { name: name.into() }
    }
}

impl Serialize for ToolChoice {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            ToolChoice::None => serializer.serialize_str("none"),
            ToolChoice::Auto => serializer.serialize_str("auto"),
            ToolChoice::Required => serializer.serialize_str("required"),
            ToolChoice::Function { name } => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("type", "function")?;
                map.serialize_entry("name", name)?;
                map.end()
            }
        }
    }
}

// --------------------------------------------------------------- response ---

/// A `Response` object, as returned by a non-streaming call and embedded in the
/// `response` field of lifecycle events.
///
/// The server echoes more fields back (`tools`, `instructions`, `temperature`,
/// `metadata`, ...); they are dropped, because this crate only models what the
/// harness consumes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub id: String,
    pub object: String,
    pub created_at: i64,
    pub model: String,
    pub status: ResponseStatus,
    #[serde(default)]
    pub output: Vec<OutputItem>,
    /// Null in `response.created` / `response.in_progress` even though the
    /// schema calls it required.
    #[serde(default)]
    pub usage: Option<Usage>,
    #[serde(default)]
    pub error: Option<ResponseError>,
    #[serde(default)]
    pub incomplete_details: Option<IncompleteDetails>,
}

impl Response {
    /// Concatenates the `output_text` parts of every message item.
    ///
    /// The API documents `output_text` as an SDK-side convenience property, so
    /// this is computed here instead of being read off the wire.
    pub fn output_text(&self) -> String {
        let mut text = String::new();
        for item in &self.output {
            let OutputItem::Message { content, .. } = item else {
                continue;
            };
            for part in content {
                if let ContentPart::OutputText { text: chunk, .. } = part {
                    text.push_str(chunk);
                }
            }
        }
        text
    }

    /// Every function call the model asked for, in output order.
    pub fn function_calls(&self) -> impl Iterator<Item = &OutputItem> {
        self.output
            .iter()
            .filter(|item| matches!(item, OutputItem::FunctionCall { .. }))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseStatus {
    Queued,
    InProgress,
    Completed,
    Incomplete,
    Failed,
    Cancelled,
    #[serde(other)]
    Unknown,
}

/// One entry of `output`.
///
/// `message`, `reasoning` and `function_call` are modelled; every other item
/// type deserialises to [`OutputItem::Unknown`] instead of failing the whole
/// response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OutputItem {
    Message {
        id: String,
        role: Role,
        content: Vec<ContentPart>,
        #[serde(default)]
        status: Option<ItemStatus>,
    },
    Reasoning {
        id: String,
        #[serde(default)]
        summary: Vec<ReasoningPart>,
        #[serde(default)]
        content: Vec<ReasoningPart>,
        #[serde(default)]
        encrypted_content: Option<String>,
        #[serde(default)]
        status: Option<ItemStatus>,
    },
    FunctionCall {
        #[serde(default)]
        id: Option<String>,
        call_id: String,
        name: String,
        /// A JSON document, as a string (the API does not parse it).
        arguments: String,
        #[serde(default)]
        status: Option<ItemStatus>,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    OutputText {
        text: String,
        #[serde(default)]
        annotations: Vec<Annotation>,
    },
    Refusal {
        refusal: String,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ReasoningPart {
    SummaryText {
        text: String,
    },
    ReasoningText {
        text: String,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Annotation {
    UrlCitation {
        url: String,
        title: String,
        start_index: u32,
        end_index: u32,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    InProgress,
    Completed,
    Incomplete,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    #[serde(default)]
    pub input_tokens_details: Option<InputTokensDetails>,
    #[serde(default)]
    pub output_tokens_details: Option<OutputTokensDetails>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputTokensDetails {
    pub cached_tokens: u64,
    #[serde(default)]
    pub cache_write_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputTokensDetails {
    pub reasoning_tokens: u64,
}

/// The `error` field of a failed `Response` (a different shape from the HTTP
/// error envelope, which is [`ErrorEnvelope`](crate::ErrorEnvelope)).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponseError {
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncompleteDetails {
    pub reason: IncompleteReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IncompleteReason {
    MaxOutputTokens,
    MaxMessages,
    ContentFilter,
    Steered,
    #[serde(other)]
    Unknown,
}

// ----------------------------------------------------------------- events ---

/// A decoded SSE event of a streaming response.
///
/// Payloads are modelled for the events the agent loop consumes. Every other
/// documented event deserialises to [`Event::Other`], which
/// [`ResponseStream::next_event`](crate::ResponseStream::next_event) skips.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type")]
pub enum Event {
    #[serde(rename = "response.created")]
    Created {
        response: Response,
        sequence_number: u64,
    },
    #[serde(rename = "response.in_progress")]
    InProgress {
        response: Response,
        sequence_number: u64,
    },
    #[serde(rename = "response.queued")]
    Queued {
        response: Response,
        sequence_number: u64,
    },

    #[serde(rename = "response.output_item.added")]
    OutputItemAdded {
        output_index: u32,
        item: OutputItem,
        sequence_number: u64,
    },
    #[serde(rename = "response.output_item.done")]
    OutputItemDone {
        output_index: u32,
        item: OutputItem,
        sequence_number: u64,
    },

    #[serde(rename = "response.content_part.added")]
    ContentPartAdded {
        item_id: String,
        output_index: u32,
        content_index: u32,
        part: ContentPart,
        sequence_number: u64,
    },
    #[serde(rename = "response.content_part.done")]
    ContentPartDone {
        item_id: String,
        output_index: u32,
        content_index: u32,
        part: ContentPart,
        sequence_number: u64,
    },

    #[serde(rename = "response.output_text.delta")]
    OutputTextDelta {
        item_id: String,
        output_index: u32,
        content_index: u32,
        delta: String,
        sequence_number: u64,
    },
    #[serde(rename = "response.output_text.done")]
    OutputTextDone {
        item_id: String,
        output_index: u32,
        content_index: u32,
        text: String,
        sequence_number: u64,
    },

    #[serde(rename = "response.refusal.delta")]
    RefusalDelta {
        item_id: String,
        output_index: u32,
        content_index: u32,
        delta: String,
        sequence_number: u64,
    },
    #[serde(rename = "response.refusal.done")]
    RefusalDone {
        item_id: String,
        output_index: u32,
        content_index: u32,
        refusal: String,
        sequence_number: u64,
    },

    #[serde(rename = "response.function_call_arguments.delta")]
    FunctionCallArgumentsDelta {
        item_id: String,
        output_index: u32,
        delta: String,
        sequence_number: u64,
    },
    #[serde(rename = "response.function_call_arguments.done")]
    FunctionCallArgumentsDone {
        item_id: String,
        output_index: u32,
        arguments: String,
        sequence_number: u64,
    },

    #[serde(rename = "response.reasoning_summary_text.delta")]
    ReasoningSummaryTextDelta {
        item_id: String,
        output_index: u32,
        summary_index: u32,
        delta: String,
        sequence_number: u64,
    },
    #[serde(rename = "response.reasoning_summary_text.done")]
    ReasoningSummaryTextDone {
        item_id: String,
        output_index: u32,
        summary_index: u32,
        text: String,
        sequence_number: u64,
    },
    #[serde(rename = "response.reasoning_summary_part.added")]
    ReasoningSummaryPartAdded {
        item_id: String,
        output_index: u32,
        summary_index: u32,
        part: ReasoningPart,
        sequence_number: u64,
    },

    #[serde(rename = "response.completed")]
    Completed {
        response: Response,
        sequence_number: u64,
    },
    #[serde(rename = "response.failed")]
    Failed {
        response: Response,
        sequence_number: u64,
    },
    #[serde(rename = "response.incomplete")]
    Incomplete {
        response: Response,
        sequence_number: u64,
    },

    /// The standalone `error` event: the error object *is* the payload, it is
    /// not wrapped in an `{"error": ...}` envelope.
    #[serde(rename = "error")]
    Error(StreamError),

    #[serde(other)]
    Other,
}

impl Event {
    /// Parses one SSE `data` payload.
    ///
    /// A payload without a `type` field is an error; an unrecognised `type` is
    /// not, it becomes [`Event::Other`].
    pub fn from_json(data: &str) -> Result<Self, crate::Error> {
        let value: Value = serde_json::from_str(data).map_err(|source| crate::Error::Decode {
            context: "sse event",
            source,
        })?;
        let kind = value.get("type").and_then(Value::as_str).map(str::to_owned);
        let event: Self = serde_json::from_value(value).map_err(|source| crate::Error::Decode {
            context: "sse event",
            source,
        })?;
        if event == Event::Other {
            tracing::debug!(
                event_type = kind.as_deref().unwrap_or("<missing>"),
                "ignoring undocumented SSE event type"
            );
        }
        Ok(event)
    }

    /// Whether this event ends the response.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Event::Completed { .. } | Event::Failed { .. } | Event::Incomplete { .. }
        )
    }

    /// The wire `type` of this event, for logging and traces.
    ///
    /// Returns `"<other>"` for the fallback variant, which has no wire type of
    /// its own; use [`Event::from_json`] directly if the exact name of an
    /// unrecognised event matters.
    pub fn kind(&self) -> &'static str {
        match self {
            Event::Created { .. } => "response.created",
            Event::InProgress { .. } => "response.in_progress",
            Event::Queued { .. } => "response.queued",
            Event::OutputItemAdded { .. } => "response.output_item.added",
            Event::OutputItemDone { .. } => "response.output_item.done",
            Event::ContentPartAdded { .. } => "response.content_part.added",
            Event::ContentPartDone { .. } => "response.content_part.done",
            Event::OutputTextDelta { .. } => "response.output_text.delta",
            Event::OutputTextDone { .. } => "response.output_text.done",
            Event::RefusalDelta { .. } => "response.refusal.delta",
            Event::RefusalDone { .. } => "response.refusal.done",
            Event::FunctionCallArgumentsDelta { .. } => "response.function_call_arguments.delta",
            Event::FunctionCallArgumentsDone { .. } => "response.function_call_arguments.done",
            Event::ReasoningSummaryTextDelta { .. } => "response.reasoning_summary_text.delta",
            Event::ReasoningSummaryTextDone { .. } => "response.reasoning_summary_text.done",
            Event::ReasoningSummaryPartAdded { .. } => "response.reasoning_summary_part.added",
            Event::Completed { .. } => "response.completed",
            Event::Failed { .. } => "response.failed",
            Event::Incomplete { .. } => "response.incomplete",
            Event::Error(_) => "error",
            Event::Other => "<other>",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct StreamError {
    pub message: String,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub param: Option<String>,
    pub sequence_number: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn serialises_a_full_request() {
        let request = ResponsesRequest::new(vec![
            InputItem::user_text("Weather in Paris?"),
            InputItem::function_call("get_weather", "call_1", "{\"location\":\"Paris\"}"),
            InputItem::function_call_output("call_1", "{\"temp_c\":18}"),
        ])
        .model("gpt-5")
        .instructions("Be brief")
        .tools(vec![
            Tool::function("get_weather", "Weather", json!({"type": "object"})).strict(true),
        ])
        .tool_choice(ToolChoice::function("get_weather"))
        .temperature(0.2)
        .max_output_tokens(1024)
        .stream(true);

        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            json!({
                "model": "gpt-5",
                "input": [
                    {"type": "message", "role": "user", "content": "Weather in Paris?"},
                    {"type": "function_call", "call_id": "call_1", "name": "get_weather",
                     "arguments": "{\"location\":\"Paris\"}"},
                    {"type": "function_call_output", "call_id": "call_1", "output": "{\"temp_c\":18}"},
                ],
                "instructions": "Be brief",
                "tools": [{"type": "function", "name": "get_weather", "description": "Weather",
                           "parameters": {"type": "object"}, "strict": true}],
                "tool_choice": {"type": "function", "name": "get_weather"},
                "temperature": 0.2,
                "max_output_tokens": 1024,
                "stream": true,
            })
        );
    }

    #[test]
    fn omits_unset_request_fields() {
        let request = ResponsesRequest::new(vec![InputItem::user_text("hi")]);
        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            json!({"input": [{"type": "message", "role": "user", "content": "hi"}]})
        );
    }

    #[test]
    fn serialises_tool_choice_modes_as_strings() {
        assert_eq!(
            serde_json::to_value(ToolChoice::None).unwrap(),
            json!("none")
        );
        assert_eq!(
            serde_json::to_value(ToolChoice::Auto).unwrap(),
            json!("auto")
        );
        assert_eq!(
            serde_json::to_value(ToolChoice::Required).unwrap(),
            json!("required")
        );
    }

    #[test]
    fn function_tool_without_description_omits_it() {
        let tool = Tool::Function {
            name: "t".to_owned(),
            description: None,
            parameters: json!({}),
            strict: None,
        };
        assert_eq!(
            serde_json::to_value(tool).unwrap(),
            json!({"type": "function", "name": "t", "parameters": {}})
        );
    }

    #[test]
    fn output_text_concatenates_message_parts() {
        let response: Response = serde_json::from_value(json!({
            "id": "resp_1", "object": "response", "created_at": 1, "model": "gpt-5",
            "status": "completed",
            "output": [
                {"type": "reasoning", "id": "rs_1", "summary": [{"type": "summary_text", "text": "thinking"}]},
                {"type": "message", "id": "msg_1", "role": "assistant", "status": "completed",
                 "content": [
                    {"type": "output_text", "text": "Hello", "annotations": []},
                    {"type": "output_text", "text": ", world", "annotations": [
                        {"type": "url_citation", "url": "https://example.com", "title": "Example",
                         "start_index": 0, "end_index": 4}
                    ]}
                 ]}
            ]
        }))
        .unwrap();

        assert_eq!(response.output_text(), "Hello, world");
        assert_eq!(response.function_calls().count(), 0);
    }

    #[test]
    fn unknown_output_item_type_is_not_fatal() {
        let response: Response = serde_json::from_value(json!({
            "id": "resp_1", "object": "response", "created_at": 1, "model": "gpt-5",
            "status": "completed",
            "output": [
                {"type": "web_search_call", "id": "ws_1", "status": "completed",
                 "action": {"type": "search", "query": "rust"}},
                {"type": "message", "id": "msg_1", "role": "assistant", "content": []}
            ]
        }))
        .unwrap();

        assert_eq!(response.output[0], OutputItem::Unknown);
        assert!(matches!(response.output[1], OutputItem::Message { .. }));
    }

    #[test]
    fn unknown_status_and_part_types_fall_back() {
        let response: Response = serde_json::from_value(json!({
            "id": "resp_1", "object": "response", "created_at": 1, "model": "gpt-5",
            "status": "compacting", "usage": null, "error": null, "incomplete_details": null,
        }))
        .unwrap();
        assert_eq!(response.status, ResponseStatus::Unknown);

        let part: ContentPart =
            serde_json::from_value(json!({"type": "image", "url": "https://example.com/a.png"}))
                .unwrap();
        assert_eq!(part, ContentPart::Unknown);
    }

    #[test]
    fn unknown_fields_are_dropped() {
        let response: Response = serde_json::from_value(json!({
            "id": "resp_1", "object": "response", "created_at": 1, "model": "gpt-5",
            "status": "completed", "output": [], "parallel_tool_calls": true,
            "metadata": {}, "service_tier": "default", "future_field": {"nested": [1, 2]},
        }))
        .unwrap();
        assert!(response.output.is_empty());
    }

    #[test]
    fn parses_reasoning_and_function_call_items() {
        let response: Response = serde_json::from_value(json!({
            "id": "resp_1", "object": "response", "created_at": 1, "model": "gpt-5",
            "status": "completed",
            "output": [
                {"type": "reasoning", "id": "rs_1", "status": "completed",
                 "summary": [{"type": "summary_text", "text": "Need the weather"}],
                 "content": [{"type": "reasoning_text", "text": "Call the tool"}]},
                {"type": "function_call", "id": "fc_1", "call_id": "call_1",
                 "name": "get_weather", "arguments": "{\"location\":\"Paris\"}", "status": "completed"}
            ]
        }))
        .unwrap();

        let OutputItem::Reasoning {
            summary, content, ..
        } = &response.output[0]
        else {
            panic!("expected reasoning item");
        };
        assert_eq!(
            summary[0],
            ReasoningPart::SummaryText {
                text: "Need the weather".to_owned()
            }
        );
        assert_eq!(
            content[0],
            ReasoningPart::ReasoningText {
                text: "Call the tool".to_owned()
            }
        );

        let calls: Vec<&OutputItem> = response.function_calls().collect();
        assert_eq!(calls.len(), 1);
        let OutputItem::FunctionCall {
            call_id,
            name,
            arguments,
            ..
        } = calls[0]
        else {
            panic!("expected function_call item");
        };
        assert_eq!((call_id.as_str(), name.as_str()), ("call_1", "get_weather"));
        assert_eq!(
            serde_json::from_str::<Value>(arguments).unwrap()["location"],
            "Paris"
        );
    }
}
