//! HTTP 客户端：`POST {base_url}/v1/systemone`。
//!
//! 端点、鉴权与响应头见 <https://docs.typesafe.ai/api> 与
//! <https://docs.typesafe.ai/introduction/quickstart>（核对日期 2026-09-22）。

use std::fmt;
use std::time::Duration;

use reqwest::header::AUTHORIZATION;
use serde::{Deserialize, Serialize};

use crate::answer::SystemOneResponse;
use crate::error::{Error, request_id};
use crate::question::{Questions, State};

/// 官方 base URL。
pub const DEFAULT_BASE_URL: &str = "https://api.typesafe.ai";

/// 默认模型：pin 到版本化 ID，避免 `jev-latest` 别名静默迁移后 confidence 含义变化。
pub const DEFAULT_MODEL: &str = "jev-1.13.0";

/// 默认单次请求超时：官方 Python SDK 的 `DEFAULT_TIMEOUT`（10s）。
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// 请求体：恰好三个顶层字段 `state` / `model` / `questions`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemOneRequest {
    /// 判定材料。
    pub state: State,
    /// 模型 ID。
    pub model: String,
    /// 问题集；key 由调用方自取，不发给模型，只用于把答案对回来。
    pub questions: Questions,
}

impl SystemOneRequest {
    /// 用 `state` 与问题集构造请求，模型取 pin 的 [`DEFAULT_MODEL`]。
    pub fn new(state: impl Into<State>, questions: Questions) -> Self {
        Self {
            state: state.into(),
            model: DEFAULT_MODEL.to_owned(),
            questions,
        }
    }

    /// 换一个模型（例如 `jev-latest`，或尚未 pin 的新版本）。
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }
}

/// Jev 客户端。
///
/// API key 只从环境变量读取，不出现在构造参数里；`Debug` 实现会脱敏，可安全打进日志。
pub struct Client {
    base_url: String,
    api_key: String,
    timeout: Duration,
    http: reqwest::Client,
}

impl Client {
    /// 从环境变量构造。
    ///
    /// - `TYPESAFE_API_KEY`：必填，作为 `Authorization: Bearer <key>` 发送；
    /// - `TYPESAFE_BASE_URL`：可选，默认 [`DEFAULT_BASE_URL`]；
    /// - 超时默认 [`DEFAULT_TIMEOUT`]，可用 [`Client::with_timeout`] 改。
    pub fn from_env() -> Result<Self, Error> {
        let api_key = std::env::var("TYPESAFE_API_KEY").map_err(|_| Error::MissingApiKey)?;
        let base_url =
            std::env::var("TYPESAFE_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.to_owned());
        Ok(Self {
            base_url,
            api_key,
            timeout: DEFAULT_TIMEOUT,
            http: reqwest::Client::builder().build()?,
        })
    }

    /// 换 base URL（自建网关 / 测试用），末尾斜杠会被忽略。
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// 换单次请求超时。
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// 当前 base URL。
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// 当前单次请求超时。
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// 调一次 `POST {base_url}/v1/systemone`。
    ///
    /// 2xx 时把响应体解析成 [`SystemOneResponse`]，并把响应头 `x-typesafe-request-id`
    /// 填进 [`SystemOneResponse::request_id`]；非 2xx 映射为 [`Error`] 的显式变体，
    /// `retry_after` 由 [`Error::retry_after`] 暴露——**本方法不重试**。
    pub async fn system_one(&self, request: &SystemOneRequest) -> Result<SystemOneResponse, Error> {
        let response = self
            .http
            .post(systemone_url(&self.base_url))
            .header(AUTHORIZATION, format!("Bearer {}", self.api_key))
            .timeout(self.timeout)
            .json(request)
            .send()
            .await?;

        let status = response.status();
        let headers = response.headers().clone();
        let body = response.text().await?;

        if !status.is_success() {
            return Err(Error::from_response(status, &headers, &body));
        }

        let mut parsed: SystemOneResponse =
            serde_json::from_str(&body).map_err(|source| Error::Decode {
                body,
                source,
                request_id: request_id(&headers),
            })?;
        parsed.request_id = request_id(&headers);
        Ok(parsed)
    }
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("base_url", &self.base_url)
            .field("api_key", &"<redacted>")
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

fn systemone_url(base_url: &str) -> String {
    format!("{}/v1/systemone", base_url.trim_end_matches('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn systemone_url_normalises_trailing_slashes() {
        assert_eq!(
            systemone_url("https://api.typesafe.ai"),
            "https://api.typesafe.ai/v1/systemone"
        );
        assert_eq!(
            systemone_url("https://api.typesafe.ai/"),
            "https://api.typesafe.ai/v1/systemone"
        );
        assert_eq!(
            systemone_url("http://127.0.0.1:8080///"),
            "http://127.0.0.1:8080/v1/systemone"
        );
    }

    #[test]
    fn debug_redacts_api_key() {
        // 占位串（不是任何真实形态的 key），只用来验证 Debug 不把它打出来。
        const PLACEHOLDER: &str = "unit-test-placeholder-not-a-key";
        let client = Client {
            base_url: DEFAULT_BASE_URL.to_owned(),
            api_key: PLACEHOLDER.to_owned(),
            timeout: DEFAULT_TIMEOUT,
            http: reqwest::Client::new(),
        };
        let rendered = format!("{client:?}");
        assert!(!rendered.contains(PLACEHOLDER), "{rendered}");
        assert!(rendered.contains("<redacted>"), "{rendered}");
        assert!(rendered.contains(DEFAULT_BASE_URL), "{rendered}");
    }

    #[test]
    fn default_model_is_pinned_version() {
        let request = SystemOneRequest::new(State::from("hello"), Questions::new());
        assert_eq!(request.model, "jev-1.13.0");
    }
}
