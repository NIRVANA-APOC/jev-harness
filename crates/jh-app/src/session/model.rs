//! LLM 侧：一次调用的抽象与 Responses 通道实现。
//!
//! 设计来源：`docs/tui-design.md` §8（数据流分层）与 `docs/grok-tui-design.md` §7.1 第 9 条
//! （决策层与 IO 层分离）。会话循环只认 [`Model`] 一个接口：真实通道是 [`ResponsesModel`]
//! （`jh-openai` 的 `/responses`，流式），离线测试用桩实现——与 [`jh_core::Judge`] 对判断侧
//! 的抽象同形，两类模型能力走同一套「按接口编程」的约定。
//!
//! 契约：
//!
//! - 一次调用 = 一条指令 + 一串输入项（用户消息 / 函数调用 / 函数调用结果）+ 一张工具表；
//! - 正文按增量推给 `deltas` 通道（宿主据此流式上屏），返回值是这次调用的完整结果；
//! - 缺配置、传输失败、响应状态不是 `completed` 一律显式失败（错误类型是 [`jh_core::Error`]，
//!   与判断侧同一套约定：缺凭据是 `Unavailable`，其余是 `CallFailed` / `InvalidResponse`）。
//!
//! 为什么把 `request` 按值传给 [`Model::respond`]：这样返回的 future 不借用会话状态，
//! 宿主可以把它和终端事件一起放进 `select!` 的一个分支里，而不必先克隆一整份会话。

use std::future::Future;
use std::pin::Pin;
use std::time::{Duration, Instant};

use jh_core::{Error, Usage};
use jh_openai::{Event, InputItem, OutputItem, Response, ResponseStatus, ResponsesRequest, Tool};
use tokio::sync::mpsc::UnboundedSender;

/// 单次调用超时：与 LLM 判断器同一档（`judge/llm.rs` 的 `DEFAULT_TIMEOUT`）。
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

/// 输出上限：一次回复（正文 + 工具调用参数）的 token 预算。
///
/// 取值比判断器小：会话回复是给人看的自然语言与工具调用，不是整包 JSON 契约；
/// 判断器的 16384 是「思考模型把 reasoning 算进预算」逼出来的（`judge/llm.rs` 的实测），
/// 会话侧同样要留 reasoning 的余量，故取 8192。
pub const DEFAULT_MAX_OUTPUT_TOKENS: u32 = 8192;

/// 一次模型调用的请求。
#[derive(Debug, Clone, PartialEq)]
pub struct ModelRequest {
    /// 系统指令（角色、工作区、工具与审批约定）。
    pub instructions: String,
    /// 输入项：本轮之前的全部对话与工具结果（Responses 通道无状态，每次自带）。
    pub input: Vec<InputItem>,
    /// 可用工具表。
    pub tools: Vec<Tool>,
}

/// 模型请求的一次工具调用。
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    /// 通道侧调用标识，结果回填时要带回去。
    pub call_id: String,
    /// 工具名。
    pub name: String,
    /// 参数 JSON 原文；解析交给工具表。
    pub arguments: String,
}

/// 一次模型调用的结果。
#[derive(Debug, Clone, PartialEq)]
pub struct ModelTurn {
    /// 完整正文（流式增量已经上过屏，这里以服务端最终文本为准）。
    pub text: String,
    /// 这次回复里请求的工具调用。
    pub calls: Vec<ToolCall>,
    /// 执行者版本（通道响应的模型 ID）。
    pub model: String,
    /// 通道侧请求 id。
    pub request_id: Option<String>,
    /// 上报的 token 用量。
    pub usage: Option<Usage>,
    /// 本次调用耗时，毫秒。
    pub latency_ms: u64,
}

/// 一次调用的 future：不借用会话状态，宿主可以把它放进 `select!` 的一个分支。
pub type ModelFuture = Pin<Box<dyn Future<Output = Result<ModelTurn, Error>> + Send>>;

/// 模型通道：一次调用一个 future，正文增量走通道。
pub trait Model: Send + Sync {
    /// 发一次调用。正文增量推给 `deltas`；收不到增量（通道已关）不算失败。
    fn respond(&self, request: ModelRequest, deltas: UnboundedSender<String>) -> ModelFuture;
}

/// `jh-openai` 的 Responses 通道（流式）。
pub struct ResponsesModel {
    client: jh_openai::Client,
}

impl ResponsesModel {
    /// 从环境变量构造：`OPENAI_API_KEY`、`OPENAI_MODEL`、`OPENAI_BASE_URL` 三者都必填。
    ///
    /// 端点不设默认值，理由与 `judge/llm.rs::from_env_client` 相同：`jh-openai` 的缺省端点是
    /// 官方 `https://api.openai.com/v1`，只给模型不给端点会把 key 发到不是本人配置的端点上。
    pub fn from_env() -> Result<Self, Error> {
        let model = std::env::var("OPENAI_MODEL").map_err(|_| {
            Error::Unavailable("缺少 OPENAI_MODEL：会话通道必须显式给出模型 ID".to_owned())
        })?;
        let base_url = std::env::var("OPENAI_BASE_URL").map_err(|_| {
            Error::Unavailable(
                "缺少 OPENAI_BASE_URL：会话通道必须显式给出端点，\
                 否则 key 会被发往 jh-openai 的官方缺省端点"
                    .to_owned(),
            )
        })?;

        Ok(Self {
            client: jh_openai::Client::builder()
                .model(model)
                .base_url(base_url)
                .timeout(DEFAULT_TIMEOUT)
                .build()
                .map_err(map_error)?,
        })
    }

    /// 通道的模型 ID（提示框信息行与顶栏用）。
    pub fn model_id(&self) -> &str {
        self.client.model()
    }
}

impl Model for ResponsesModel {
    fn respond(&self, request: ModelRequest, deltas: UnboundedSender<String>) -> ModelFuture {
        let client = self.client.clone();
        Box::pin(async move {
            let mut body = ResponsesRequest::new(request.input)
                .instructions(request.instructions)
                .max_output_tokens(DEFAULT_MAX_OUTPUT_TOKENS);
            if !request.tools.is_empty() {
                body = body
                    .tools(request.tools)
                    .tool_choice(jh_openai::ToolChoice::Auto);
            }

            let started = Instant::now();
            let mut stream = client.stream(&body).await.map_err(map_error)?;
            while let Some(event) = stream.next_event().await.map_err(map_error)? {
                match event {
                    Event::OutputTextDelta { delta, .. } => {
                        // 增量只服务上屏：上屏那条链路没了不是这次调用的失败。
                        let _ = deltas.send(delta);
                    }
                    Event::Error(error) => {
                        return Err(Error::CallFailed(format!(
                            "通道返回错误事件：{}",
                            error.message
                        )));
                    }
                    Event::Failed { response, .. } => return Err(status_error("失败", &response)),
                    Event::Incomplete { response, .. } => {
                        return Err(status_error("被截断", &response));
                    }
                    Event::Completed { response, .. } => {
                        return Ok(turn(response, started.elapsed().as_millis() as u64));
                    }
                    _ => {}
                }
            }
            Err(Error::CallFailed("通道在完成事件之前关闭".to_owned()))
        })
    }
}

/// 把完成响应读成一次调用的结果。
fn turn(response: Response, latency_ms: u64) -> ModelTurn {
    let calls = response
        .function_calls()
        .filter_map(|item| match item {
            OutputItem::FunctionCall {
                call_id,
                name,
                arguments,
                ..
            } => Some(ToolCall {
                call_id: call_id.clone(),
                name: name.clone(),
                arguments: arguments.clone(),
            }),
            _ => None,
        })
        .collect();
    ModelTurn {
        text: response.output_text(),
        calls,
        model: response.model.clone(),
        request_id: Some(response.id.clone()),
        usage: response.usage.as_ref().map(|usage| Usage {
            input_tokens: Some(usage.input_tokens),
            output_tokens: Some(usage.output_tokens),
        }),
        latency_ms,
    }
}

/// 非 `completed` 的终态：把状态与截断原因写进错误文本，不让调用方去猜。
fn status_error(what: &str, response: &Response) -> Error {
    let detail = response
        .error
        .as_ref()
        .and_then(|error| error.message.clone())
        .or_else(|| {
            response
                .incomplete_details
                .as_ref()
                .map(|details| format!("reason={:?}", details.reason))
        })
        .unwrap_or_else(|| format!("status={:?}", response.status));
    if response.status == ResponseStatus::Incomplete {
        return Error::InvalidResponse(format!("响应{what}：{detail}"));
    }
    Error::CallFailed(format!("响应{what}：{detail}"))
}

/// 通道错误折进判断侧同一套三类错误：缺凭据 / 缺配置是「不可用」，其余是「调用失败」。
fn map_error(error: jh_openai::Error) -> Error {
    match error {
        jh_openai::Error::MissingApiKey(name) => Error::Unavailable(format!("缺少 {name}")),
        jh_openai::Error::Config(detail) => {
            Error::Unavailable(format!("会话通道配置无效：{detail}"))
        }
        other => Error::CallFailed(other.to_string()),
    }
}
