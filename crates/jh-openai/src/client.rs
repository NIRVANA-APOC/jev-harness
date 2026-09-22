//! The HTTP client: request construction, auth, streaming and error mapping.

use std::collections::VecDeque;
use std::pin::Pin;
use std::time::Duration;

use bytes::Bytes;
use futures::{Stream, StreamExt};
use reqwest::header::{CONTENT_TYPE, RETRY_AFTER};

use crate::error::Error;
use crate::sse::SseDecoder;
use crate::types::{Event, Response, ResponsesRequest};

/// Environment variable the API key is read from when a client is built.
pub const API_KEY_ENV: &str = "OPENAI_API_KEY";

/// Documented default base URL; `POST {base_url}/responses` is the endpoint.
pub const BASE_URL: &str = "https://api.openai.com/v1";

/// The literal payload both official SDK decoders stop at, kept for
/// compatibility even though the Responses API is not documented to send it.
const DONE_SENTINEL: &str = "[DONE]";

/// A client for one OpenAI-compatible Responses endpoint.
///
/// The API key is read from [`API_KEY_ENV`] when the client is built; it is
/// never logged and never stored anywhere else.
#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    base_url: String,
    model: String,
    api_key: String,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("api_key", &"<redacted>")
            .finish()
    }
}

impl Client {
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    /// The configured base URL, without a trailing slash.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// The default model used by requests that do not set one.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Non-streaming `POST /responses`.
    ///
    /// The request's `stream` flag is overridden to `false`.
    pub async fn create(&self, request: &ResponsesRequest) -> Result<Response, Error> {
        let mut request = request.clone();
        request.prepare(&self.model, false);
        let response = self.post(&request).await?;
        let status = response.status();
        let retry_after = retry_after(&response);
        let body = response.text().await?;
        Error::check(status, retry_after.as_deref(), &body)?;
        serde_json::from_str(&body).map_err(|source| Error::Decode {
            context: "response body",
            source,
        })
    }

    /// Streaming `POST /responses` with `"stream": true`.
    ///
    /// The request's `stream` flag is overridden to `true`. The returned stream
    /// yields events until the response completes, the stream ends, or an `SSE
    /// [DONE]` sentinel is read.
    pub async fn stream(&self, request: &ResponsesRequest) -> Result<ResponseStream, Error> {
        let mut request = request.clone();
        request.prepare(&self.model, true);
        let response = self.post(&request).await?;

        let status = response.status();
        if !status.is_success() {
            let retry_after = retry_after(&response);
            let body = response.text().await?;
            return Err(Error::from_http(
                status.as_u16(),
                retry_after.as_deref(),
                &body,
            ));
        }
        Ok(ResponseStream::new(Box::pin(response.bytes_stream())))
    }

    async fn post(&self, request: &ResponsesRequest) -> Result<reqwest::Response, Error> {
        let body = serde_json::to_vec(request).map_err(|source| Error::Decode {
            context: "request body",
            source,
        })?;
        let response = self
            .http
            .post(format!("{}/responses", self.base_url))
            .bearer_auth(&self.api_key)
            .header(CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await?;
        Ok(response)
    }
}

fn retry_after(response: &reqwest::Response) -> Option<String> {
    response
        .headers()
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

/// Configures a [`Client`].
///
/// `model` is mandatory — there is no built-in default model; `base_url` falls
/// back to [`BASE_URL`] and no total request timeout is set unless one is given.
#[derive(Debug, Default)]
pub struct ClientBuilder {
    base_url: Option<String>,
    model: Option<String>,
    timeout: Option<Duration>,
}

impl ClientBuilder {
    pub fn base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Caps the whole request, including the time spent reading a stream.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn build(self) -> Result<Client, Error> {
        let api_key = std::env::var(API_KEY_ENV).map_err(|_| Error::MissingApiKey(API_KEY_ENV))?;
        let model = self.model.ok_or(Error::Config("model is required"))?;

        let mut http = reqwest::Client::builder();
        if let Some(timeout) = self.timeout {
            http = http.timeout(timeout);
        }

        Ok(Client {
            http: http.build()?,
            base_url: self
                .base_url
                .unwrap_or_else(|| BASE_URL.to_owned())
                .trim_end_matches('/')
                .to_owned(),
            model,
            api_key,
        })
    }
}

/// The event stream of a streaming response.
///
/// Chunks are decoded incrementally, so a server may split the byte stream
/// anywhere. Events declared by the API but not modelled by [`Event`] are
/// skipped (logged at `debug`), which keeps the stream usable when the API adds
/// new event types.
pub struct ResponseStream {
    chunks: Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + Send>>,
    decoder: SseDecoder,
    pending: VecDeque<String>,
    done: bool,
}

impl ResponseStream {
    fn new(chunks: Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + Send>>) -> Self {
        Self {
            chunks,
            decoder: SseDecoder::new(),
            pending: VecDeque::new(),
            done: false,
        }
    }

    /// Builds a stream from in-memory chunks instead of a live connection.
    ///
    /// This is the offline replay path used by the tests and by
    /// `examples/stream.rs`; it performs no I/O.
    pub fn from_chunks<S>(chunks: S) -> Self
    where
        S: Stream<Item = reqwest::Result<Bytes>> + Send + 'static,
    {
        Self::new(Box::pin(chunks))
    }

    /// Returns the next event, or `None` once the response is over.
    ///
    /// The stream is finished when the server closes it, when a `[DONE]`
    /// sentinel arrives, or right after a terminal event
    /// ([`Event::Completed`], [`Event::Failed`] or [`Event::Incomplete`]),
    /// whichever comes first. A decode or transport error also ends the stream —
    /// the state machine never resumes after failing.
    pub async fn next_event(&mut self) -> Result<Option<Event>, Error> {
        loop {
            if let Some(data) = self.pending.pop_front() {
                if data == DONE_SENTINEL {
                    self.done = true;
                    return Ok(None);
                }
                let event = match Event::from_json(&data) {
                    Ok(event) => event,
                    Err(error) => {
                        self.done = true;
                        return Err(error);
                    }
                };
                if event == Event::Other {
                    continue;
                }
                if event.is_terminal() {
                    self.done = true;
                }
                return Ok(Some(event));
            }

            if self.done {
                return Ok(None);
            }

            match self.chunks.next().await {
                Some(Ok(bytes)) => match self.decoder.push(&bytes) {
                    Ok(payloads) => self.pending.extend(payloads),
                    Err(error) => {
                        self.done = true;
                        return Err(error);
                    }
                },
                Some(Err(source)) => {
                    self.done = true;
                    return Err(Error::Transport(source));
                }
                None => {
                    self.done = true;
                    return Ok(None);
                }
            }
        }
    }
}

impl std::fmt::Debug for ResponseStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResponseStream")
            .field("pending", &self.pending.len())
            .field("done", &self.done)
            .finish()
    }
}
