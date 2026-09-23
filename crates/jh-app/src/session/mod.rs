//! `jev chat` 的会话宿主：LLM 主循环 + Jev 判定门控 + 工具执行的副作用落地 + TUI 接线。
//!
//! 分层（与 `docs/tui-design.md` §8 的数据流一致）：
//!
//! ```text
//! jh-tui（App / Effect / 视图模型）      ← 宿主喂状态与视图模型，只拿回 Effect
//!      ▲                                 │
//! [tui]   事件循环：终端事件、在途步骤、按需 tick，执行 Effect
//!      ▲                                 │
//! [agent] 会话状态机：模型 → 门控 → 审批 → 执行 → 回收
//!      ▲
//! [model] / [tools] / [gate] / [approval] / [view]  ← 一层一件事
//!      ▲
//! jh-openai（LLM 通道）/ jh-jev（经 jh-app 的 judge 适配）/ jh-core（判断、证据、失效传播）
//! ```
//!
//! 边界：
//!
//! - `jh-tui` 不认识领域类型：面板与提示框信息行由 [`view`] 翻译成它的视图模型；
//! - 本模块不自己画屏：渲染只有 `jh_tui::render::draw` 一个入口；键盘只经 `App::handle_event`，
//!   Effect 的副作用全部由 [`tui`] 执行；
//! - key 只从环境变量读进内存（`OPENAI_API_KEY` / `TYPESAFE_API_KEY`），不进文件、不进提交；
//!   真调产物落 `var/`（调用记录 `var/calls.jsonl`、证据 `var/chat/<run>/evidence.jsonl`）。
//!
//! 模块地图：
//!
//! | 模块 | 职责 |
//! | --- | --- |
//! | [`model`] | LLM 侧：一次调用的抽象 + Responses 流式通道 |
//! | [`tools`] | 工具集：工作区只读三件 + `bash` 命令执行 |
//! | [`gate`] | 门控：确定性风险层 + Jev 类型化判定 + 防抖 |
//! | [`approval`] | 会话内审批策略：白名单与授权模式 |
//! | [`agent`] | 会话状态机：一步一个状态，界面变化描述成 `Update` |
//! | [`view`] | 领域对象 → 面板与判定轨迹的视图模型 |
//! | [`tui`] | 终端宿主：事件循环、Effect 执行、终端恢复护栏 |

mod agent;
mod approval;
mod gate;
mod model;
mod tools;
mod tui;
mod view;

#[cfg(test)]
pub(crate) mod test_support;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use jh_core::{JsonlEvidenceLog, Judge, SemanticState};
use jh_tui::terminal::Capabilities;

use crate::calls::CallLog;
use crate::judge::{self, JudgeName};

pub use agent::{Agent, AgentConfig, DEFAULT_MAX_ROUNDS, Stats, Step, Update};
pub use approval::ApprovalPolicy;
pub use gate::DEFAULT_WINDOW as DEFAULT_DEBOUNCE;
pub use model::{Model, ResponsesModel};
pub use tools::Tools;

/// `jev chat` 的运行参数（CLI 给，缺省值见 `main.rs` 的 `ChatArgs`）。
#[derive(Debug, Clone)]
pub struct ChatOptions {
    /// 工作区根：工具的读写边界与 `bash` 的工作目录。
    pub workspace: PathBuf,
    /// 门控判断器。
    pub judge: JudgeName,
    /// 本次运行标识。
    pub run: String,
    /// 调用记录落盘路径（JSONL，追加写）。
    pub calls_path: PathBuf,
    /// 运行产物目录（证据文件写这里）。
    pub out_dir: PathBuf,
    /// 防抖窗口。
    pub debounce: Duration,
    /// 本轮模型调用上限。
    pub max_rounds: usize,
}

/// 跑一次会话：装配两条真实通道与语义状态，然后进终端，直到用户退出。
///
/// 缺凭据**不**阻止进入界面：判定器缺凭据时门控如实标「不可用」（交人工裁决），模型缺配置时
/// 提交后如实上屏失败原因，界面本身照常可用（`AGENTS.md`「不虚构」）。
pub async fn chat(options: ChatOptions) -> Result<Stats, crate::Error> {
    std::fs::create_dir_all(&options.out_dir)?;
    let state = SemanticState::new(Box::new(JsonlEvidenceLog::open(
        options.out_dir.join("evidence.jsonl"),
    )?));
    let calls = CallLog::open(options.calls_path.clone())?;

    let channels = channels(options.judge);
    let mut agent = Agent::new(AgentConfig {
        workspace: options.workspace,
        judge_name: options.judge,
        judge: channels.judge,
        model: channels.model,
        model_label: channels.model_label,
        run: options.run,
        state,
        calls,
        debounce: options.debounce,
        max_rounds: options.max_rounds,
    })?;

    let capabilities = Capabilities::detect();
    tui::run(&mut agent, &capabilities).await
}

/// 取通道错误的一句话原因。
///
/// `jh_core::Error` 的调用类变体在 `Display` 里自带前缀（「判断器不可用：」等），会话把原因
/// 嵌进自己的文案里时只要那句话本身，免得套两层前缀。
pub(crate) fn call_reason(error: &jh_core::Error) -> String {
    match error {
        jh_core::Error::Unavailable(detail)
        | jh_core::Error::CallFailed(detail)
        | jh_core::Error::InvalidResponse(detail) => detail.clone(),
        other => other.to_string(),
    }
}

/// 两条通道与模型标识。
struct Channels {
    judge: Option<Arc<dyn Judge>>,
    model: Option<Arc<dyn Model>>,
    model_label: String,
}

/// 两条真实通道：判定器按名字构造，模型按环境变量构造；缺配置如实标「不可用」并把原因写到
/// 标准错误（界面里另有如实的一行）。
fn channels(name: JudgeName) -> Channels {
    let judge: Option<Arc<dyn Judge>> = match build_judge(name) {
        Ok(judge) => Some(judge),
        Err(error) => {
            eprintln!("门控判定器不可用：{}", call_reason(&error));
            None
        }
    };
    match ResponsesModel::from_env() {
        Ok(model) => Channels {
            judge,
            model_label: model.model_id().to_owned(),
            model: Some(Arc::new(model)),
        },
        Err(error) => {
            eprintln!("会话模型不可用：{}", call_reason(&error));
            Channels {
                judge,
                model: None,
                model_label: "模型未配置".to_owned(),
            }
        }
    }
}

/// 按名字构造门控判定器。会话只支持不需要任务集的三个：`jev`（真调）、`llm`、`rule`。
fn build_judge(name: JudgeName) -> Result<Arc<dyn Judge>, jh_core::Error> {
    Ok(match name {
        JudgeName::Jev => Arc::new(judge::JevJudge::from_env()?),
        JudgeName::Llm => Arc::new(judge::LlmJudge::from_env()?),
        JudgeName::Rule => Arc::new(judge::RuleJudge::new()),
        JudgeName::Replay | JudgeName::SimulatedTruth => {
            return Err(jh_core::Error::Unavailable(format!(
                "会话不支持判定器 `{}`：它需要任务集，请换 --judge jev / rule / llm",
                name.cli_name()
            )));
        }
    })
}
