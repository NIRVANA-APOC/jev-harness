//! 错误类型与 HTTP 状态码映射。
//!
//! 官方状态码语义见 <https://docs.typesafe.ai/api>（核对日期 2026-09-22）：
//! `401` key 缺失或无效、`422` 请求体校验失败（响应体指出出错字段）、`429` 超出限流、
//! `529` 服务过载。

use std::time::Duration;

use reqwest::StatusCode;
use reqwest::header::HeaderMap;
use serde_json::Value;

/// `529 Overloaded`：非标准状态码，`http::StatusCode` 里没有对应常量。
const OVERLOADED: u16 = 529;

/// 服务端错误体。
///
/// 官方**没有**给出错误响应的 JSON 契约（`docs/jev-research.md` §3.9 #1），因此按官方
/// Python SDK `extract_message()` 的思路容错解析：依次尝试 `error` → `message` → `detail`，
/// `detail` 兼容 FastAPI 风格的 `[{loc, msg}]`。任何形状都不会让解析失败。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiErrorBody {
    /// 取出的可读信息；解析不出就是 `None`。
    pub message: Option<String>,
    /// 原始响应体文本。
    pub raw: String,
}

impl ApiErrorBody {
    /// 解析错误响应体。
    pub fn parse(raw: &str) -> Self {
        let message = serde_json::from_str::<Value>(raw)
            .ok()
            .as_ref()
            .and_then(extract_message);
        Self {
            message,
            raw: raw.to_owned(),
        }
    }
}

fn extract_message(body: &Value) -> Option<String> {
    ["error", "message", "detail"]
        .iter()
        .find_map(|key| render(body.get(*key)?))
}

/// 把一个 JSON 值渲染成可读文本：字符串直接用；对象取 `message` / `msg`；
/// 数组逐项渲染后拼接（FastAPI 的校验错误即 `[{loc, msg}]` 形态）。
fn render(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Object(map) => map
            .get("message")
            .or_else(|| map.get("msg"))
            .and_then(render),
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().filter_map(render).collect();
            (!parts.is_empty()).then(|| parts.join("; "))
        }
        _ => None,
    }
}

/// 服务端返回的非 2xx 响应：状态码 + 请求 id + 退避提示 + 容错解析的错误体。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    /// HTTP 状态码。
    pub status: StatusCode,
    /// 服务端请求 id（响应头 `x-typesafe-request-id`），用于对账与日志追踪。
    pub request_id: Option<String>,
    /// 服务端 `retry-after`（秒）；仅在响应带该头时存在，官方不保证 `429` 一定带。
    pub retry_after: Option<Duration>,
    /// 容错解析后的错误体。
    pub body: ApiErrorBody,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HTTP {}", self.status.as_u16())?;
        if let Some(request_id) = &self.request_id {
            write!(f, " (request {request_id})")?;
        }
        match &self.body.message {
            Some(message) => write!(f, ": {message}"),
            None if !self.body.raw.trim().is_empty() => write!(f, ": {}", self.body.raw.trim()),
            None => Ok(()),
        }
    }
}

impl ApiError {
    fn from_parts(status: StatusCode, headers: &HeaderMap, body: &str) -> Self {
        Self {
            status,
            request_id: request_id(headers),
            retry_after: retry_after(headers),
            body: ApiErrorBody::parse(body),
        }
    }
}

/// 读响应头 `x-typesafe-request-id`；服务端未承诺总是携带。
pub(crate) fn request_id(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-typesafe-request-id")?
        .to_str()
        .ok()
        .map(str::to_owned)
}

/// 读响应头 `retry-after`（延迟秒数形式；HTTP-date 形式不支持）。
fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    let raw = headers.get(reqwest::header::RETRY_AFTER)?.to_str().ok()?;
    let seconds: f64 = raw.trim().parse().ok()?;
    seconds
        .is_finite()
        .then(|| Duration::from_secs_f64(seconds.max(0.0)))
}

/// 调用 Jev 时的失败。
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// 环境变量 `TYPESAFE_API_KEY` 未设置。
    #[error("TYPESAFE_API_KEY is not set")]
    MissingApiKey,

    /// 传输层失败：连接、DNS、超时、TLS、读响应体等。
    #[error("transport error: {0}")]
    Transport(#[from] reqwest::Error),

    /// 状态码是 2xx，但响应体不是本 crate 能解析的 Jev 响应。
    #[error("failed to decode systemone response: {source}")]
    Decode {
        /// 原始响应体，用于定位契约漂移。
        body: String,
        /// serde 的解析错误。
        #[source]
        source: serde_json::Error,
        /// 服务端请求 id（若响应带了 `x-typesafe-request-id`）。
        request_id: Option<String>,
    },

    /// `401`：API key 缺失或无效。
    #[error("unauthorized: {0}")]
    Unauthorized(ApiError),

    /// `422`：请求体校验失败，错误体指出出错字段。
    #[error("unprocessable entity: {0}")]
    Unprocessable(ApiError),

    /// `429`：超出限流（250,000 tokens/s 或 1,200 requests/min），应退避后重试。
    #[error("rate limited: {0}")]
    RateLimited(ApiError),

    /// `529`：服务暂时过载，应短暂延迟后重试。
    #[error("service overloaded: {0}")]
    Overloaded(ApiError),

    /// 其它非 2xx 状态码。
    #[error("unexpected status: {0}")]
    UnexpectedStatus(ApiError),
}

impl Error {
    /// 把非 2xx 响应映射成显式错误（纯函数，便于离线单测）。
    pub(crate) fn from_response(status: StatusCode, headers: &HeaderMap, body: &str) -> Self {
        let api = ApiError::from_parts(status, headers, body);
        match status {
            StatusCode::UNAUTHORIZED => Error::Unauthorized(api),
            StatusCode::UNPROCESSABLE_ENTITY => Error::Unprocessable(api),
            StatusCode::TOO_MANY_REQUESTS => Error::RateLimited(api),
            _ if status.as_u16() == OVERLOADED => Error::Overloaded(api),
            _ => Error::UnexpectedStatus(api),
        }
    }

    /// 服务端请求 id；成功响应上的 id 在 [`crate::SystemOneResponse::request_id`]。
    pub fn request_id(&self) -> Option<&str> {
        match self {
            Error::Decode { request_id, .. } => request_id.as_deref(),
            Error::Unauthorized(api)
            | Error::Unprocessable(api)
            | Error::RateLimited(api)
            | Error::Overloaded(api)
            | Error::UnexpectedStatus(api) => api.request_id.as_deref(),
            Error::MissingApiKey | Error::Transport(_) => None,
        }
    }

    /// 服务端要求的等待时间（`retry-after` 头）。
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Error::Unauthorized(api)
            | Error::Unprocessable(api)
            | Error::RateLimited(api)
            | Error::Overloaded(api)
            | Error::UnexpectedStatus(api) => api.retry_after,
            Error::MissingApiKey | Error::Transport(_) | Error::Decode { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.insert(
                reqwest::header::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                value.parse().unwrap(),
            );
        }
        headers
    }

    #[test]
    fn unauthorized_is_mapped_with_request_id() {
        let error = Error::from_response(
            StatusCode::UNAUTHORIZED,
            &headers(&[("x-typesafe-request-id", "req_123")]),
            r#"{"error":"Invalid API key"}"#,
        );
        assert!(matches!(&error, Error::Unauthorized(_)));
        assert_eq!(error.request_id(), Some("req_123"));
        assert_eq!(error.retry_after(), None);
        assert_eq!(
            error.to_string(),
            "unauthorized: HTTP 401 (request req_123): Invalid API key"
        );
    }

    #[test]
    fn unprocessable_uses_fastapi_style_detail() {
        let error = Error::from_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            &HeaderMap::new(),
            r#"{"detail":[{"loc":["body","questions"],"msg":"field required"}]}"#,
        );
        assert!(matches!(&error, Error::Unprocessable(_)));
        let Error::Unprocessable(api) = &error else {
            unreachable!()
        };
        assert_eq!(api.body.message.as_deref(), Some("field required"));
        assert_eq!(
            api.body.raw,
            r#"{"detail":[{"loc":["body","questions"],"msg":"field required"}]}"#
        );
    }

    #[test]
    fn rate_limited_exposes_retry_after() {
        let error = Error::from_response(
            StatusCode::TOO_MANY_REQUESTS,
            &headers(&[("retry-after", "2")]),
            r#"{"message":"slow down"}"#,
        );
        assert!(matches!(&error, Error::RateLimited(_)));
        assert_eq!(error.retry_after(), Some(Duration::from_secs(2)));
        assert_eq!(error.to_string(), "rate limited: HTTP 429: slow down");
    }

    #[test]
    fn retry_after_accepts_fractional_seconds() {
        let error = Error::from_response(
            StatusCode::TOO_MANY_REQUESTS,
            &headers(&[("retry-after", " 1.5 ")]),
            "",
        );
        assert_eq!(error.retry_after(), Some(Duration::from_millis(1500)));
    }

    #[test]
    fn overloaded_529_is_its_own_variant() {
        let error = Error::from_response(
            StatusCode::from_u16(OVERLOADED).unwrap(),
            &HeaderMap::new(),
            "server busy",
        );
        assert!(matches!(&error, Error::Overloaded(_)));
        assert_eq!(
            error.to_string(),
            "service overloaded: HTTP 529: server busy"
        );
    }

    #[test]
    fn other_statuses_are_unexpected() {
        let error = Error::from_response(StatusCode::INTERNAL_SERVER_ERROR, &HeaderMap::new(), "");
        assert!(matches!(&error, Error::UnexpectedStatus(_)));
        assert_eq!(error.to_string(), "unexpected status: HTTP 500");
    }

    #[test]
    fn error_body_parses_plain_text_and_missing_fields() {
        assert_eq!(
            ApiErrorBody::parse("boom"),
            ApiErrorBody {
                message: None,
                raw: "boom".to_owned()
            }
        );
        assert_eq!(
            ApiErrorBody::parse(r#"{"unexpected":"shape"}"#),
            ApiErrorBody {
                message: None,
                raw: r#"{"unexpected":"shape"}"#.to_owned()
            }
        );
    }

    #[test]
    fn error_body_prefers_error_then_message() {
        let body = ApiErrorBody::parse(r#"{"error":"first","message":"second"}"#);
        assert_eq!(body.message.as_deref(), Some("first"));
    }
}
