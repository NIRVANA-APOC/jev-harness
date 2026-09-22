use serde::{Deserialize, Serialize};

/// Error object returned by the Responses API, both inside the HTTP error
/// envelope (`{"error": {...}}`) and as the payload of the SSE `error` event.
///
/// Only `message` is guaranteed by the documented contract; `type`, `param`
/// and `code` are nullable and are therefore optional here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiError {
    pub message: String,
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub param: Option<String>,
    #[serde(default)]
    pub code: Option<String>,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)?;
        if let Some(code) = &self.code {
            write!(f, " (code: {code})")?;
        }
        if let Some(kind) = &self.r#type {
            write!(f, " (type: {kind})")?;
        }
        Ok(())
    }
}

/// HTTP error envelope: `{"error": {"message": ..., "type": ..., ...}}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorEnvelope {
    pub error: ApiError,
}

/// Every failure mode of this crate.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The API key environment variable was not set when building a client.
    #[error("missing API key: set the {0} environment variable")]
    MissingApiKey(&'static str),

    /// The client was not configured correctly.
    #[error("invalid client configuration: {0}")]
    Config(&'static str),

    /// The request never produced an HTTP response (DNS, TLS, timeout, ...).
    #[error("http transport error: {0}")]
    Transport(#[from] reqwest::Error),

    /// Any other non-2xx status that has no dedicated variant. When the body is
    /// not a parseable error envelope, `message` holds the raw body.
    #[error("api error: {0}")]
    Api(ApiError),

    /// The API rejected the credentials (HTTP 401).
    #[error("unauthorized (401): {0}")]
    Unauthorized(ApiError),

    /// The API throttled the request (HTTP 429). `retry_after` carries the
    /// `Retry-After` header in seconds when it is present and numeric; a
    /// non-numeric header (an HTTP date) is dropped rather than parsed.
    #[error("rate limited (429): {error}")]
    RateLimited {
        error: ApiError,
        retry_after: Option<u64>,
    },

    /// The API failed on its side (HTTP 5xx).
    #[error("server error ({status}): {error}")]
    Server { status: u16, error: ApiError },

    /// A byte chunk of the SSE stream is not valid UTF-8.
    #[error("invalid utf-8 in {context}")]
    Utf8 {
        context: &'static str,
        source: std::str::Utf8Error,
    },

    /// A JSON payload (response body or SSE `data` line) does not match the
    /// types in this crate.
    #[error("failed to decode {context}")]
    Decode {
        context: &'static str,
        source: serde_json::Error,
    },
}

impl Error {
    /// Maps a non-2xx HTTP response to a typed error.
    ///
    /// Non-2xx bodies are expected to be the documented error envelope. When
    /// they are not (a proxy HTML page, an empty body), the raw body is carried
    /// as the `message` so the failure stays visible instead of being swallowed.
    ///
    /// `retry_after` is the raw `Retry-After` header; it is used on 429 when it
    /// is numeric (the header may also be an HTTP date, which is not parsed).
    pub(crate) fn from_http(status: u16, retry_after: Option<&str>, body: &str) -> Self {
        let error = match serde_json::from_str::<ErrorEnvelope>(body) {
            Ok(envelope) => envelope.error,
            Err(_) => ApiError {
                message: body.to_owned(),
                r#type: None,
                param: None,
                code: None,
            },
        };
        let retry_after = retry_after.and_then(|value| value.trim().parse().ok());

        match status {
            401 => Error::Unauthorized(error),
            429 => Error::RateLimited { error, retry_after },
            500..=599 => Error::Server { status, error },
            _ => Error::Api(error),
        }
    }

    /// Checks the status of an HTTP response, consuming nothing.
    ///
    /// `body` is only inspected on failure.
    pub(crate) fn check(
        status: reqwest::StatusCode,
        retry_after: Option<&str>,
        body: &str,
    ) -> Result<(), Error> {
        if status.is_success() {
            return Ok(());
        }
        Err(Error::from_http(status.as_u16(), retry_after, body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENVELOPE: &str = r#"{"error":{"message":"Incorrect API key provided","type":"invalid_request_error","param":null,"code":"invalid_api_key"}}"#;

    #[test]
    fn maps_401_to_unauthorized() {
        let err = Error::from_http(401, None, ENVELOPE);
        let Error::Unauthorized(api) = err else {
            panic!("expected Unauthorized, got {err:?}");
        };
        assert_eq!(api.code.as_deref(), Some("invalid_api_key"));
        assert_eq!(api.param, None);
    }

    #[test]
    fn maps_429_to_rate_limited_with_retry_after() {
        let err = Error::from_http(429, Some("17"), ENVELOPE);
        let Error::RateLimited { retry_after, .. } = err else {
            panic!("expected RateLimited, got {err:?}");
        };
        assert_eq!(retry_after, Some(17));
    }

    #[test]
    fn maps_5xx_to_server_error() {
        for status in [500, 503] {
            let err = Error::from_http(status, None, ENVELOPE);
            let Error::Server { status: got, .. } = err else {
                panic!("expected Server, got {err:?}");
            };
            assert_eq!(got, status);
        }
    }

    #[test]
    fn maps_other_statuses_to_api_error() {
        let err = Error::from_http(400, None, ENVELOPE);
        let Error::Api(api) = err else {
            panic!("expected Api, got {err:?}");
        };
        assert_eq!(api.message, "Incorrect API key provided");
    }

    #[test]
    fn keeps_raw_body_when_not_an_envelope() {
        let err = Error::from_http(502, None, "<html>bad gateway</html>");
        let Error::Server { status, error } = err else {
            panic!("expected Server, got {err:?}");
        };
        assert_eq!(status, 502);
        assert_eq!(error.message, "<html>bad gateway</html>");
    }

    #[test]
    fn parses_retry_after_only_when_numeric() {
        let err = Error::from_http(429, Some(" 5 "), "");
        let Error::RateLimited { retry_after, .. } = err else {
            panic!("expected RateLimited, got {err:?}");
        };
        assert_eq!(retry_after, Some(5));

        // A non-numeric header (HTTP date) must not break the mapping.
        let err = Error::from_http(429, Some("Wed, 21 Oct 2026 07:28:00 GMT"), "");
        let Error::RateLimited { retry_after, .. } = err else {
            panic!("expected RateLimited, got {err:?}");
        };
        assert_eq!(retry_after, None);
    }
}
