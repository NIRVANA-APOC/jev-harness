//! LLM provider 预设与配置解析。
//!
//! 项目只跑一种 wire 协议（OpenAI 的 Responses API，实现在 [`jh_openai`]），但**端点、密钥变量
//! 与默认模型**因 provider 而异。这一层把那份差异收成一个类型：[`LlmConfig`]。判断器
//! （[`crate::judge::llm`]）与会话通道（[`crate::session::model`]）都只消费它，不再各自读
//! `OPENAI_*`，也不再要求调用方手写端点。
//!
//! 解析口径（**默认 provider = DeepSeek**，不设 `JEV_LLM_PROVIDER` 时就是它）：
//!
//! | provider | 端点 | 密钥变量（按顺序取第一个有值的） | 默认模型 |
//! | --- | --- | --- | --- |
//! | `deepseek`（默认） | `https://api.deepseek.com` | `DEEPSEEK_API_KEY`、`OPENAI_API_KEY` | `deepseek-flash` |
//! | `openai` | `https://api.openai.com/v1` | `OPENAI_API_KEY` | 无：必须给 `OPENAI_MODEL` |
//! | `custom` | 必须给 `OPENAI_BASE_URL` | `OPENAI_API_KEY`、`DEEPSEEK_API_KEY` | 必须给 `OPENAI_MODEL` |
//!
//! `OPENAI_BASE_URL` / `OPENAI_MODEL` 仍然生效，但它们是**覆盖**而不是必填：DeepSeek 预设自带
//! 端点与模型，所以只配 `DEEPSEEK_API_KEY` 就能跑。反过来，显式覆盖仍然优先——想让默认 provider
//! 走别的兼容端点（或别的 DeepSeek 模型），照旧给 `OPENAI_BASE_URL` / `OPENAI_MODEL` 即可。
//!
//! 缺配置一律**显式失败**并点名缺哪个变量（`AGENTS.md`「错误显式失败」）；不做静默回退到官方端点，
//! 否则 key 会被发到不是本人配置的地方。

use std::time::Duration;

use jh_core::Error;

/// provider 标识。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderId {
    /// DeepSeek：[`DEFAULT_PROVIDER`]，本项目 LLM 侧的默认通道。
    DeepSeek,
    /// OpenAI 官方端点。
    OpenAi,
    /// 自建 / 第三方 Responses 兼容端点，端点与模型都由环境变量显式给出。
    Custom,
}

impl ProviderId {
    /// 从 `JEV_LLM_PROVIDER` 的取值解析；大小写不敏感。
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "deepseek" => Some(ProviderId::DeepSeek),
            "openai" => Some(ProviderId::OpenAi),
            "custom" => Some(ProviderId::Custom),
            _ => None,
        }
    }

    /// 写回环境变量与提示文案的规范写法。
    pub fn as_str(self) -> &'static str {
        match self {
            ProviderId::DeepSeek => "deepseek",
            ProviderId::OpenAi => "openai",
            ProviderId::Custom => "custom",
        }
    }
}

/// 一个 provider 的预设：端点、默认模型与密钥变量候选。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LlmProvider {
    /// 这个预设的标识。
    pub id: ProviderId,
    /// 上屏用的名字（会话信息行、`/model`）。
    pub label: &'static str,
    /// 缺省端点；`None` 表示必须由 `OPENAI_BASE_URL` 给出。
    pub default_base_url: Option<&'static str>,
    /// 缺省模型；`None` 表示必须由 `OPENAI_MODEL` 给出。
    pub default_model: Option<&'static str>,
    /// 密钥变量候选，按顺序取第一个有值且非空的。
    pub key_envs: &'static [&'static str],
}

/// DeepSeek 预设。
pub const DEEPSEEK: LlmProvider = LlmProvider {
    id: ProviderId::DeepSeek,
    label: "DeepSeek",
    default_base_url: Some("https://api.deepseek.com"),
    default_model: Some("deepseek-flash"),
    key_envs: &["DEEPSEEK_API_KEY", "OPENAI_API_KEY"],
};

/// OpenAI 官方预设。
pub const OPENAI: LlmProvider = LlmProvider {
    id: ProviderId::OpenAi,
    label: "OpenAI",
    default_base_url: Some("https://api.openai.com/v1"),
    default_model: None,
    key_envs: &["OPENAI_API_KEY"],
};

/// 自建 / 兼容端点预设。
pub const CUSTOM: LlmProvider = LlmProvider {
    id: ProviderId::Custom,
    label: "自定义端点",
    default_base_url: None,
    default_model: None,
    key_envs: &["OPENAI_API_KEY", "DEEPSEEK_API_KEY"],
};

/// DeepSeek 官方当前的模型名（2026-09-24 核对自 <https://api-docs.deepseek.com/quick_start/pricing/>）。
///
/// 只是 `/model` 的**便利选项**，不是契约：模型名与可用性由服务方决定，列表过期也不影响
/// `/model <任意ID>`。旧名 `deepseek-v4-flash` / `deepseek-v4-flash-vision-exp` 官方仍接受
/// （请求由新模型服务），但不在推荐列表里。
pub const DEEPSEEK_MODELS: [&str; 2] = ["deepseek-flash", "deepseek-v4-pro"];

/// 全部预设，顺序即 `JEV_LLM_PROVIDER` 的合法取值。
pub const PROVIDERS: [LlmProvider; 3] = [DEEPSEEK, OPENAI, CUSTOM];

/// 默认 provider（用户口径：LLM 侧默认走 DeepSeek）。
pub const DEFAULT_PROVIDER: ProviderId = ProviderId::DeepSeek;

/// 按标识取预设。
pub fn provider(id: ProviderId) -> &'static LlmProvider {
    PROVIDERS
        .iter()
        .find(|provider| provider.id == id)
        .expect("PROVIDERS 覆盖全部 ProviderId")
}

/// 选择 provider 的环境变量名。
pub const PROVIDER_ENV: &str = "JEV_LLM_PROVIDER";
/// 覆盖端点的环境变量名。
pub const BASE_URL_ENV: &str = "OPENAI_BASE_URL";
/// 覆盖模型的环境变量名。
pub const MODEL_ENV: &str = "OPENAI_MODEL";

/// 解析好的 LLM 通道配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmConfig {
    /// 命中的预设。
    pub provider: &'static LlmProvider,
    /// 端点（已去掉尾斜杠）。
    pub base_url: String,
    /// 模型 ID。
    pub model: String,
    /// 密钥所在的环境变量名（值本身不落在这里，构造客户端时才从环境读）。
    pub api_key_env: &'static str,
}

impl LlmConfig {
    /// 这一份配置的 `jh-openai` 客户端构造器。
    pub fn client_builder(&self, timeout: Duration) -> jh_openai::ClientBuilder {
        jh_openai::Client::builder()
            .model(self.model.clone())
            .base_url(self.base_url.clone())
            .api_key_env(self.api_key_env)
            .timeout(timeout)
    }

    /// 换个模型 ID、其余不变（`/model <id>` 用）。
    pub fn with_model(&self, model: impl Into<String>) -> Self {
        LlmConfig {
            model: model.into(),
            ..self.clone()
        }
    }

    /// 一句话描述（会话信息行与 `/model` 上屏用）。
    pub fn describe(&self) -> String {
        format!(
            "{} · {} · {}",
            self.provider.label, self.model, self.base_url
        )
    }
}

/// 从进程环境解析。
pub fn from_env() -> Result<LlmConfig, Error> {
    resolve(|key| std::env::var(key).ok())
}

/// 从任意查找函数解析（单测不必改进程环境）。
///
/// 空字符串按「没给」处理：`OPENAI_MODEL=` 这种写法很常见，不该被当成有效配置。
pub fn resolve(lookup: impl Fn(&str) -> Option<String>) -> Result<LlmConfig, Error> {
    let value = |key: &str| lookup(key).filter(|value| !value.trim().is_empty());

    let id = match value(PROVIDER_ENV) {
        Some(raw) => ProviderId::parse(&raw).ok_or_else(|| {
            Error::Unavailable(format!(
                "{PROVIDER_ENV} 取值 `{raw}` 不认识：可选 {}",
                PROVIDERS
                    .iter()
                    .map(|provider| provider.id.as_str())
                    .collect::<Vec<_>>()
                    .join(" / ")
            ))
        })?,
        None => DEFAULT_PROVIDER,
    };
    let provider = provider(id);

    let base_url = match value(BASE_URL_ENV) {
        Some(url) => url,
        None => provider
            .default_base_url
            .map(str::to_owned)
            .ok_or_else(|| {
                Error::Unavailable(format!(
                    "provider `{}` 没有自带端点：请给 {BASE_URL_ENV}",
                    provider.id.as_str()
                ))
            })?,
    };
    let base_url = base_url.trim_end_matches('/').to_owned();

    let model = match value(MODEL_ENV) {
        Some(model) => model,
        None => provider.default_model.map(str::to_owned).ok_or_else(|| {
            Error::Unavailable(format!(
                "provider `{}` 没有自带模型：请给 {MODEL_ENV}",
                provider.id.as_str()
            ))
        })?,
    };

    let api_key_env = provider
        .key_envs
        .iter()
        .copied()
        .find(|key| value(key).is_some())
        .ok_or_else(|| {
            Error::Unavailable(format!(
                "provider `{}` 缺凭据：请设置 {}（只经环境变量，不入库、不进日志）",
                provider.id.as_str(),
                provider.key_envs.join(" 或 ")
            ))
        })?;

    Ok(LlmConfig {
        provider,
        base_url,
        model,
        api_key_env,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// 用一张表当环境：`None` 表示变量不存在。
    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: BTreeMap<String, String> = pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect();
        move |key: &str| map.get(key).cloned()
    }

    #[test]
    fn deepseek_is_the_default_and_needs_only_its_own_key() {
        let config = resolve(env(&[("DEEPSEEK_API_KEY", "k")])).expect("默认 provider 应当可用");
        assert_eq!(config.provider.id, ProviderId::DeepSeek);
        assert_eq!(config.base_url, "https://api.deepseek.com");
        assert_eq!(config.model, "deepseek-flash");
        assert_eq!(config.api_key_env, "DEEPSEEK_API_KEY");
        assert_eq!(
            config.describe(),
            "DeepSeek · deepseek-flash · https://api.deepseek.com"
        );
    }

    #[test]
    fn the_old_openai_key_still_works_for_the_default_provider() {
        let config = resolve(env(&[("OPENAI_API_KEY", "k")])).expect("回退到 OPENAI_API_KEY");
        assert_eq!(config.api_key_env, "OPENAI_API_KEY");
        assert_eq!(config.base_url, "https://api.deepseek.com");
    }

    #[test]
    fn explicit_base_url_and_model_override_the_preset() {
        let config = resolve(env(&[
            ("DEEPSEEK_API_KEY", "k"),
            ("OPENAI_BASE_URL", "https://example.test/v1/"),
            ("OPENAI_MODEL", "some-model"),
        ]))
        .expect("覆盖生效");
        assert_eq!(config.base_url, "https://example.test/v1");
        assert_eq!(config.model, "some-model");
    }

    #[test]
    fn blank_values_count_as_missing() {
        let config = resolve(env(&[
            ("DEEPSEEK_API_KEY", "  "),
            ("OPENAI_API_KEY", "k"),
            ("OPENAI_MODEL", ""),
        ]))
        .expect("空值不算配置");
        assert_eq!(config.api_key_env, "OPENAI_API_KEY");
        assert_eq!(config.model, "deepseek-flash");
    }

    #[test]
    fn openai_provider_uses_its_own_endpoint_and_needs_a_model() {
        let missing = resolve(env(&[
            ("JEV_LLM_PROVIDER", "openai"),
            ("OPENAI_API_KEY", "k"),
        ]))
        .expect_err("openai 没有默认模型");
        assert!(
            missing.to_string().contains("OPENAI_MODEL"),
            "错误要点名缺哪个变量：{missing}"
        );

        let config = resolve(env(&[
            ("JEV_LLM_PROVIDER", "OPENAI"),
            ("OPENAI_API_KEY", "k"),
            ("OPENAI_MODEL", "gpt-x"),
        ]))
        .expect("给了模型就能用");
        assert_eq!(config.base_url, "https://api.openai.com/v1");
        assert_eq!(config.api_key_env, "OPENAI_API_KEY");
    }

    #[test]
    fn custom_provider_needs_an_explicit_endpoint() {
        let missing = resolve(env(&[
            ("JEV_LLM_PROVIDER", "custom"),
            ("OPENAI_API_KEY", "k"),
            ("OPENAI_MODEL", "m"),
        ]))
        .expect_err("custom 没有自带端点");
        assert!(missing.to_string().contains("OPENAI_BASE_URL"), "{missing}");

        let config = resolve(env(&[
            ("JEV_LLM_PROVIDER", "custom"),
            ("OPENAI_BASE_URL", "https://compat.test"),
            ("OPENAI_MODEL", "m"),
            ("DEEPSEEK_API_KEY", "k"),
        ]))
        .expect("端点、模型、密钥齐了");
        assert_eq!(config.api_key_env, "DEEPSEEK_API_KEY");
    }

    #[test]
    fn an_unknown_provider_name_fails_with_the_legal_values() {
        let error = resolve(env(&[
            ("JEV_LLM_PROVIDER", "gemini"),
            ("DEEPSEEK_API_KEY", "k"),
        ]))
        .expect_err("不认识的 provider 要显式失败");
        let text = error.to_string();
        assert!(text.contains("gemini"), "{text}");
        assert!(
            text.contains("deepseek") && text.contains("openai"),
            "{text}"
        );
    }

    #[test]
    fn nothing_configured_names_every_accepted_variable() {
        let error = resolve(env(&[])).expect_err("没有任何凭据");
        let text = error.to_string();
        assert!(
            text.contains("DEEPSEEK_API_KEY") && text.contains("OPENAI_API_KEY"),
            "{text}"
        );
    }

    #[test]
    fn with_model_keeps_the_rest_of_the_configuration() {
        let config = resolve(env(&[("DEEPSEEK_API_KEY", "k")])).expect("默认配置");
        let switched = config.with_model("deepseek-reasoner");
        assert_eq!(switched.model, "deepseek-reasoner");
        assert_eq!(switched.base_url, config.base_url);
        assert_eq!(switched.api_key_env, config.api_key_env);
    }
}
