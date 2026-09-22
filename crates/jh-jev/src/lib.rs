//! TypeSafe **Jev**（System One）的原生 Rust 客户端。
//!
//! 官方只提供 Python 与 JavaScript/TypeScript SDK，Rust 侧没有官方支持路径
//! （证据见 `docs/jev-research.md` §10.1），所以本 crate 直接实现 HTTP 契约。
//!
//! # wire 契约来源
//!
//! 字段、约束与错误码一律以官方文档为准，**核对日期 2026-09-22**：
//!
//! - API reference（端点、请求/响应字段、状态码）：<https://docs.typesafe.ai/api>
//! - Quickstart：<https://docs.typesafe.ai/introduction/quickstart>
//! - 三类原语：<https://docs.typesafe.ai/primitives/choice>、
//!   <https://docs.typesafe.ai/primitives/score>、<https://docs.typesafe.ai/primitives/noul>
//! - `state` 形态：<https://docs.typesafe.ai/concepts/state>
//! - 超时/重试默认值（本 crate 的超时默认值取自此处）：
//!   <https://docs.typesafe.ai/sdk/python/api/constants>、<https://docs.typesafe.ai/sdk/python/api/retries>
//!
//! 仓库内的对照阅读材料：`docs/jev-research.md` §3（API 契约）、§10（Rust 集成要点）。
//! `fixtures/` 下的示例请求/响应逐字段抄自上述官方页面（只调了缩进），未改字段、未改取值。
//!
//! # 类型映射上的取舍
//!
//! - 问题与答案都用 `#[serde(tag = "type", rename_all = "snake_case")]` 内部标签枚举表达
//!   Choice / Score / Noul——wire 上的 `type` 正好是 Rust 的判别标签；
//! - **未知 `type` 由 `#[serde(other)]` 兜底**：服务端日后新增原语时，整条响应仍能解析，
//!   未知答案落在 [`Answer::Unknown`]；请求侧的 [`Question::Unknown`] 同样只为回放旧数据而存在；
//! - **未知字段默认被忽略**（不启用 `deny_unknown_fields`），换取服务端新增字段时的前向兼容；
//!   代价是服务端扩展字段不会被察觉——由于有 64k/请求的上下文预算与 wire 契约漂移风险，
//!   这里选择「不因单个未知字段丢掉整条响应」，取舍讨论见 `docs/jev-research.md` §10.3；
//! - **Noul 没有 `confidence`**：它不是「可选字段」，而是二值分布只有一个自由度，
//!   因此用独立的 [`NoulAnswer`] 表达，而不是 `Option<f64>`——对 Noul 索要 confidence 是编译错误；
//! - **Score 的 `legend` / `probabilities` 在 HTTP 上是字符串级号**（`"0"`、`"1"`），
//!   本 crate 内部用整数 key（`BTreeMap<u8, _>`），与官方 Python SDK 的整数 key 一致；
//! - 概率与 confidence 原样保留，不在类型层塌缩成布尔值。
//!
//! # 刻意不做的事
//!
//! - **不流式**：官方 API 是「一次请求—整包响应」，无 SSE / 流式接口；
//! - **不内建重试**：429 / 529 只映射为显式错误并暴露 `retry_after`，
//!   是否重试、退避多少由调用方（harness 的限流层）决定；
//! - **不做本地校验**：选项数 ≤ 255、Score 级数 2–10、单请求 64k tokens 等限制不在这里拦截，
//!   越界时由服务端以 422 显式失败。
//!
//! # 离线
//!
//! 除 [`Client::system_one`] 之外，crate 内没有任何会发起网络请求的路径；
//! 仓库内测试全部离线，真调 smoke 标了 `#[ignore]`。
//!
//! # 示例
//!
//! ```no_run
//! use jh_jev::{Client, Question, State, SystemOneRequest};
//!
//! # async fn run() -> Result<(), jh_jev::Error> {
//! let client = Client::from_env()?; // TYPESAFE_API_KEY（必填）、TYPESAFE_BASE_URL（可选）
//! let mut questions = jh_jev::Questions::new();
//! questions.insert(
//!     "urgency".to_owned(),
//!     Question::Noul(jh_jev::NoulQuestion {
//!         instructions: "Does this message express urgency?".into(),
//!         criteria: None,
//!     }),
//! );
//! let request = SystemOneRequest::new(State::from("I'm losing sales. Please help ASAP."), questions);
//!
//! let response = client.system_one(&request).await?;
//! if let Some(answer) = response.noul("urgency") {
//!     println!("urgency = {}", answer.noul);
//! }
//! # Ok(())
//! # }
//! ```

#![warn(missing_docs)]

mod answer;
mod client;
mod error;
mod question;

pub use answer::{Answer, ChoiceAnswer, NoulAnswer, ScoreAnswer, SystemOneResponse, Usage};
pub use client::{Client, DEFAULT_BASE_URL, DEFAULT_MODEL, DEFAULT_TIMEOUT, SystemOneRequest};
pub use error::{ApiError, ApiErrorBody, Error};
pub use question::{
    ChoiceQuestion, Instructions, NoulCriteria, NoulQuestion, Question, Questions, ScoreQuestion,
    State, StateEntry,
};
