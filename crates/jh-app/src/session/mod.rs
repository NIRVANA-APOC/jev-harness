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
mod log;
mod model;
mod store;
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
pub use approval::{ApprovalPolicy, PermissionMode};
pub use gate::DEFAULT_WINDOW as DEFAULT_DEBOUNCE;
pub use log::{SessionEvent, SessionLog, read_events};
pub use model::{Model, ResponsesModel};
pub use store::{ConversationLog, SessionList, SessionMeta, SessionStore};
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
    // **不在这里建目录**：`jev chat` 开着不用时不该在磁盘上留痕（目录由第一次落盘创建，
    // 见 `Agent::ensure_session`）。证据日志的 `open` 只读不建，第一次追加前目录已经在。
    let state = SemanticState::new(Box::new(JsonlEvidenceLog::open(
        options.out_dir.join("evidence.jsonl"),
    )?));
    let calls = CallLog::open(options.calls_path.clone())?;

    // 会话历史：目录由 main.rs 决定（缺省 `~/.jev-harness/sessions/<run>`）。
    // **这里不建任何文件**：Agent 在第一次提交时才落盘（开着不用的会话不该留空目录）。
    // 取不到根目录时不阻断会话——`/sessions` 与 `/resume` 会如实说「历史不可用」。
    let store = SessionStore::open_default().ok();

    let channels = channels(options.judge);
    let model_label = channels.model_label.clone();
    // 日志里的 model 只写模型标识本身（信息行是另一回事，别把整行塞进这个字段）。
    let log_model = model_label.clone();
    let judge_label = options.judge.cli_name().to_owned();

    // 开场事件：延迟到第一次落盘时写，于是日志永远以 `session_start` 开头。
    let session_start = SessionEvent::SessionStart {
        at_unix_ms: crate::exp::unix_ms(),
        run: options.run.clone(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        workspace: options.workspace.display().to_string(),
        judge: judge_label.clone(),
        model: log_model.clone(),
        max_rounds: options.max_rounds,
        debounce_ms: options.debounce.as_millis() as u64,
        color: String::new(),
        glyphs: String::new(),
        tty: false,
    };
    let mut agent = Agent::new(AgentConfig {
        workspace: options.workspace.clone(),
        judge_name: options.judge,
        judge: channels.judge,
        model: channels.model,
        model_label,
        run: options.run.clone(),
        state,
        calls,
        debounce: options.debounce,
        max_rounds: options.max_rounds,
        session_dir: Some(options.out_dir.clone()),
        session_start: Some(session_start),
        store,
        session_id: options.run.clone(),
    })?;

    let capabilities = Capabilities::detect();
    // 开场事件里的环境读数要等能力探测出来才知道：补进 Agent 手里那份待写事件。
    agent.set_session_start_env(
        format!("{:?}", capabilities.color).to_ascii_lowercase(),
        format!("{:?}", capabilities.glyphs).to_ascii_lowercase(),
        capabilities.is_tty,
    );

    let started_at_unix_ms = crate::exp::unix_ms();
    let outcome = tui::run(&mut agent, &capabilities).await;

    // 收尾：结束事件 + 会话摘要（失败也照写，方便事后看出这次是异常结束的）。
    let stats = agent.stats();
    if let Some(error) = agent.log_event(SessionEvent::SessionEnd {
        at_unix_ms: crate::exp::unix_ms(),
        duration_ms: crate::exp::unix_ms().saturating_sub(started_at_unix_ms),
        turns: stats.turns,
        model_calls: stats.model_calls,
        tool_calls: stats.tool_calls,
        judgments: stats.judgments,
        judge_reuses: stats.judge_reuses,
        refusals: stats.refusals,
        denials: stats.denials,
    }) {
        eprintln!("会话日志没写上：{error}");
    }
    // 只有真的落过盘（提交过或 `/resume` 过）才写收尾摘要：
    // 一次没用过的会话本来就没目录，也不该在这一步补一个出来。
    if agent.session_started()
        && let Some(store) = agent.store()
        && let Err(error) = store.write_meta(&agent.meta())
    {
        eprintln!("会话摘要没写上：{error}");
    }
    outcome.map(|_| stats)
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
