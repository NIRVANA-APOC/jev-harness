//! 会话状态机：模型 → 门控 → 审批 → 执行 → 回收，一步一个状态。
//!
//! 设计来源：`docs/tui-design.md` §5.1（一轮在屏上的样子）、§8（数据流与分层）与
//! `docs/grok-tui-design.md` §7.1 第 9 条（决策层与 IO 层分离）。
//!
//! 分层：
//!
//! - 本模块只认领域对象（`jh_core` 的判断 / 证据 / 图）与两类模型通道（[`Model`] / [`jh_core::Judge`]），
//!   **不碰终端、不画屏**：每一次界面变化都描述成 [`Update`]，由 [`crate::session::tui`] 翻译成
//!   `jh_tui::app::App` 的调用；
//! - 需要外部世界干活的地方一律产出 [`Step`]：起模型调用、发一次门控判定、等人工裁决、执行工具。
//!   宿主（`tui.rs`）负责跑这些步骤，把结果送回对应的 `on_*`，并在手上有在途步骤时**不**调
//!   [`Agent::advance`]（否则同一步会再发一次）。
//!
//! 概率是一等数据：门控的 [`GateDecision`] 原样带着答案与 confidence，同时进判断记录
//! （`SemanticState`）、研究视图（判断详情）与状态行的判定轨迹，全程不塌缩成布尔。
//!
//! 对象模型（证据与失效传播的载体，名字在 [`crate::session::view::objects`]）：
//!
//! | 对象 | 每次出新版本 | 谁读它 |
//! | --- | --- | --- |
//! | `session.ask` | 用户提交一句话 | 门控判断 |
//! | `shell.command` | 每次 shell 调用 | 门控判断 |
//! | `tool.result` | 每次工具执行 | 门控判断（下一条命令看得到上一次结果） |
//!
//! 于是「新证据到来 → 依据变旧 → 待重查 → 行动被挡下」这条链在会话里真实发生，研究视图的
//! 失效传播面板画的就是它（`docs/RESEARCH_GOALS.md` §5.4）。

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use jh_core::{
    ActionId, ContentHash, Error, EvaluationStatus, Invalidation, Judge, JudgeRequest, JudgeSource,
    NewEvidence, NodeId, ObjectId, ObjectVersion, PremiseCheck, QuestionId, QuestionSpec,
    SemanticState, SnapshotId, State,
};
use jh_openai::InputItem;
use jh_tui::app::{ApprovalOutcome, CommandCall};

use crate::session::approval::{PERMISSION_USAGE, PermissionMode};
use crate::session::log::{GateThresholds, LoggedText, LoggedToolCall, SessionEvent, SessionLog};
use crate::session::model::ResponsesModel;
use crate::session::store::{ConversationLog, SessionMeta, SessionStore, describe};
use jh_tui::blocks::{DecisionTrace, NoticeTone, ToolStatus};
use jh_tui::components::{ApprovalRequest, StatusModel, TurnPhase, WaitingReason};
use jh_tui::panels::Panels;
use tokio::sync::mpsc::{self, UnboundedReceiver};

use crate::calls::{CallEntry, CallLog};
use crate::judge::JudgeName;
use crate::session::approval::ApprovalPolicy;
use crate::session::gate::{self, Debounce, GateCall, GateDecision, Risk, Thresholds, Verdict};
use crate::session::model::{Model, ModelFuture, ModelRequest, ModelTurn, ToolCall};
use crate::session::tools::{BASH, ToolOutcome, Tools};
use crate::session::view;

/// 本轮最多几次模型调用（一次模型调用 + 它请求的工具执行算一轮）。
///
/// 上限只在**一轮之内**计数：命中后这一轮停在「还有话没答完」的位置，用
/// [`CONTINUE_COMMAND`]（或直接提交新话）就能接着跑。`0` 表示不限（见 [`AgentConfig::max_rounds`]）。
pub const DEFAULT_MAX_ROUNDS: usize = 32;

/// 一轮还在跑时提交被退回的提示：宿主把它压到屏上，并把话放回输入框。
pub const BUSY_NOTICE: &str =
    "这一轮还在跑：等它收尾，或按 Esc 取消后再提交（这句话已退回输入框，没有被丢掉）";

/// 「接着跑」的命令名：注册项在 `jh_tui` 的命令表里（`/continue`），会话这边只按名字分派。
pub const CONTINUE_COMMAND: &str = "continue";

/// 建会话需要的全部外部材料。
pub struct AgentConfig {
    /// 工作区根。
    pub workspace: PathBuf,
    /// 门控判断器的名字（上屏与选择来源标签用）。
    pub judge_name: JudgeName,
    /// 门控判断器；缺凭据时为 `None`（判定不可用，交人工裁决）。
    pub judge: Option<Arc<dyn Judge>>,
    /// 会话模型通道；缺配置时为 `None`（界面照常可进，失败在循环里如实上屏）。
    pub model: Option<Arc<dyn Model>>,
    /// 模型标识（提示框信息行与顶栏用）；模型不可用时写清原因。
    pub model_label: String,
    /// 本次运行标识（调用记录用）。
    pub run: String,
    /// 语义状态：证据、判断节点、依赖图与失效传播。
    pub state: SemanticState,
    /// 调用记录（JSONL）。
    pub calls: CallLog,
    /// 防抖窗口。
    pub debounce: Duration,
    /// 本轮模型调用上限；`0` 表示不限。
    pub max_rounds: usize,
    /// 会话目录：**第一次真正要写盘时**才建目录与文件（启动即退出不该留下空会话）。
    /// `None` = 不记（离线单测用）。
    pub session_dir: Option<std::path::PathBuf>,
    /// 开场事件：延迟到第一次写盘时落，于是日志永远以 `session_start` 开头。
    pub session_start: Option<SessionEvent>,
    /// 会话历史仓库（`/sessions` 与 `/resume` 用）；`None` = 不支持这两个命令。
    pub store: Option<SessionStore>,
    /// 本次会话标识（写进摘要与日志）。
    pub session_id: String,
}

/// 会话对界面的一次更新。宿主把它翻译成 `jh_tui::app::App` 的调用。
///
/// 助手正文的流式增量不走这里：它是纯上屏的东西，宿主拿到模型的增量通道后直接写进界面
/// （`App::stream_assistant`），不必绕一圈经过会话状态机。
#[derive(Debug, Clone, PartialEq)]
pub enum Update {
    /// 回显用户块（时间戳已格式化）。
    User {
        /// 正文。
        text: String,
        /// 时间戳（`HH:MM:SSZ`）。
        at: String,
    },
    /// 助手正文收尾（收起「还在长」的标记）；带上这次调用的最终正文，供宿主在没有
    /// 增量（通道不给 `delta`）时补上正文。
    AssistantEnd(String),
    /// 阶段行（带 `◆` 标记）。
    Notice {
        /// 文案。
        text: String,
        /// 语气。
        tone: NoticeTone,
    },
    /// 无标记的阶段行（`用时 1.5s` 一类）。
    PlainNotice(String),
    /// 工具块：起一块（执行中）。
    ToolBegin {
        /// 工具名。
        name: String,
        /// 参数摘要。
        args: String,
    },
    /// 工具块：回填结果。
    ToolEnd {
        /// 状态。
        status: ToolStatus,
        /// 结果摘要。
        detail: String,
    },
    /// 本轮判定轨迹（画到状态行与判定块）。
    Decision(Box<DecisionTrace>),
    /// 审批卡：开卡等裁决。
    Approval(Box<ApprovalRequest>),
    /// 状态行。
    Status(StatusModel),
    /// 研究视图的三份面板数据。
    Panels(Box<Panels>),
    /// 提示框底边的信息行（模型 · 判定器 · 授权模式）。
    PromptInfo(String),
    /// 命令的选项列表（`/resume`、`/permission`、`/model`）：界面拿它开选择列表
    /// （`↑↓` 选、`Enter` 用选中的值执行 `command`、`Esc` 关）。
    Choices {
        /// `Enter` 时要执行的命令名。
        command: &'static str,
        /// 标题行（含键位提示）。
        title: String,
        /// 每行的值与说明（`vector` 顺序即显示顺序）。
        entries: Vec<jh_tui::app::ChoiceEntry>,
    },
    /// 顶栏左侧标识（工作目录）。
    HeaderTitle(String),
}

/// 宿主要替会话做的一步。每一步都不借用会话，可以放进 `select!` 的一个分支。
pub enum Step {
    /// 空闲：等用户输入（或暂停中）。
    Idle,
    /// 起一次模型调用（完成后回 [`Agent::on_model_result`]）。
    Model(ModelRequest),
    /// 发一次门控判定（完成后回 [`Agent::on_gate_result`]）。
    Gate(GateCall),
    /// 执行一次工具调用（完成后回 [`Agent::on_tool_result`]）。
    Tool(ToolCall),
}

/// 会话累计读数（退出时打一行，也是验收读数）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stats {
    /// 用户提交的次数。
    pub turns: usize,
    /// 模型调用次数。
    pub model_calls: u64,
    /// 实际执行的工具调用次数（不含被门控挡下与被人拒绝的）。
    pub tool_calls: u64,
    /// 发出门控判定的次数（不含防抖复用）。
    pub judgments: u64,
    /// 防抖复用的次数。
    pub judge_reuses: u64,
    /// 门控拒绝的次数。
    pub refusals: u64,
    /// 人工拒绝与「未裁决」的次数。
    pub denials: u64,
}

/// 在途状态：同一时刻只有一个。
enum Phase {
    /// 空闲。
    Idle,
    /// 模型调用在途。
    Modeling,
    /// 门控判定在途（节点已声明、材料证据已落）。
    Gating {
        /// 正在判定的调用。
        call: ToolCall,
        /// 这次判定的判断节点。
        node: NodeId,
    },
    /// 等人工裁决（卡片已开）。
    Awaiting {
        /// 等待裁决的调用。
        call: ToolCall,
        /// 这次调用的判断节点（只读工具没有）。
        node: Option<NodeId>,
    },
    /// 工具执行在途。
    Running {
        /// 正在执行的调用。
        call: ToolCall,
        /// 这次调用登记的行动（只读工具没有前提，也不登记）。
        action: Option<ActionId>,
    },
}

/// 会话产物：两份逐行追加的文件 + 它们所在的目录。
///
/// **第一次真正要写盘时才构造**（见 [`Agent::ensure_session`]）：`SessionLog::open` 与
/// `ConversationLog::open` 都会建父目录，所以「开着不用」的会话不会在磁盘上留痕。
#[derive(Debug)]
struct SessionFiles {
    log: Option<SessionLog>,
    conversation: Option<ConversationLog>,
}

impl SessionFiles {
    /// 建目录、开两个日志、写开场摘要与 `session_start` 事件。
    fn open(
        store: &SessionStore,
        id: &str,
        dir: &std::path::Path,
        workspace: &str,
        model: &str,
        judge: &str,
        session_start: Option<SessionEvent>,
    ) -> Result<Self, crate::Error> {
        let mut log = SessionLog::open(dir.join("session.jsonl"))?;
        let conversation = ConversationLog::open(dir.join("conversation.jsonl"))?;
        store.start(id, workspace, model, judge)?;
        if let Some(event) = session_start {
            log.append(&event)?;
        }
        Ok(SessionFiles {
            log: Some(log),
            conversation: Some(conversation),
        })
    }
}

/// 会话：领域状态机 + 全部可复现读数。
pub struct Agent {
    workspace: PathBuf,
    judge_name: JudgeName,
    judge: Option<Arc<dyn Judge>>,
    model: Option<Arc<dyn Model>>,
    model_label: String,
    tools: Tools,
    approval: ApprovalPolicy,
    debounce: Debounce,
    question: QuestionSpec,
    thresholds: Thresholds,
    state: SemanticState,
    calls: CallLog,
    run: String,
    conversation: Vec<InputItem>,
    queue: VecDeque<ToolCall>,
    phase: Phase,
    /// 有未回应的话要说（用户刚提交，或刚收完工具结果）。
    owes_model: bool,
    /// 本轮已经起过几次模型调用。
    rounds: usize,
    max_rounds: usize,
    /// 上一轮停在了模型调用上限上：还有话没答完，`/continue` 据此接回去。
    limit_paused: bool,
    last_invalidation: Option<Invalidation>,
    last_node: Option<NodeId>,
    turn_started: Option<Instant>,
    stats: Stats,
    /// 上一次模型请求的字节数（调用记录的输入规模）。
    last_request_bytes: u64,
    /// 会话产物（日志 + 对话记录）：第一次要写盘才开（见 `SessionFiles`）。
    files: Option<SessionFiles>,
    /// 会话目录与开场事件：`files` 打开时用它们。
    session_dir: Option<std::path::PathBuf>,
    session_start: Option<SessionEvent>,
    /// 会话历史仓库。
    store: Option<SessionStore>,
    /// 本次会话标识。
    session_id: String,
    /// 会话开始时刻（写摘要用）。
    created_at_unix_ms: u64,
    /// 首条用户消息（摘要里认人用）。
    first_message: String,
    /// 最近一条用户消息（摘要用）。
    last_message: String,
    /// 当前工具调用的起点（算工具耗时用）。
    tool_started: Option<Instant>,
    /// 审批卡弹出时刻（算人工等待时长用）。
    approval_opened_at: Option<Instant>,
}

impl Agent {
    /// 组装会话。
    pub fn new(config: AgentConfig) -> Result<Self, crate::Error> {
        let tools = Tools::new(config.workspace.clone())?;
        Ok(Agent {
            workspace: config.workspace,
            judge_name: config.judge_name,
            judge: config.judge,
            model: config.model,
            model_label: config.model_label,
            tools,
            approval: ApprovalPolicy::new(),
            debounce: Debounce::new(config.debounce),
            question: gate::question(),
            thresholds: Thresholds::default(),
            state: config.state,
            calls: config.calls,
            run: config.run,
            conversation: Vec::new(),
            queue: VecDeque::new(),
            phase: Phase::Idle,
            owes_model: false,
            rounds: 0,
            max_rounds: config.max_rounds,
            limit_paused: false,
            last_invalidation: None,
            last_node: None,
            turn_started: None,
            stats: Stats::default(),
            last_request_bytes: 0,
            files: None,
            session_dir: config.session_dir,
            session_start: config.session_start,
            store: config.store,
            session_id: config.session_id,
            created_at_unix_ms: crate::exp::unix_ms(),
            first_message: String::new(),
            last_message: String::new(),
            tool_started: None,
            approval_opened_at: None,
        })
    }

    /// 工作区根。
    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    /// 累计读数。
    pub fn stats(&self) -> Stats {
        self.stats
    }

    /// 工具表（宿主拿去异步执行）。
    pub fn tools(&self) -> Tools {
        self.tools.clone()
    }

    /// 起一次模型调用：返回不借用会话的 future 与正文增量通道（宿主放进 `select!`）。
    ///
    /// [`Step::Model`] 只在模型可用时产出，因此这里可以直接取用：模型缺配置时
    /// [`Agent::start_model`] 就先退回了「模型不可用」并结束本轮，不会走到这里。
    pub fn model_run(&self, request: &ModelRequest) -> (ModelFuture, UnboundedReceiver<String>) {
        let (deltas, receiver) = mpsc::unbounded_channel();
        let model = self.model.clone().expect("Step::Model 只在模型可用时产出");
        let request = request.clone();
        let future: ModelFuture = Box::pin(async move { model.respond(request, deltas).await });
        (future, receiver)
    }

    /// 首屏右下角的注解：判定器与模型接线状态（如实标注，不假装接了）。
    pub fn startup_note(&self) -> String {
        format!("{} · {}", self.judge_label(), self.model_label)
    }

    /// 提示框底边的信息行：`模型 · 判定器 · 授权模式`（设计 §5.1）。
    pub fn prompt_info(&self) -> String {
        format!(
            "{} · {} · {}",
            self.model_label,
            self.judge_label(),
            self.approval.mode_label()
        )
    }

    /// 顶栏左侧标识：工作目录（家里目录缩写成 `~`）。
    pub fn header_title(&self) -> String {
        let path = self.workspace.display().to_string();
        match std::env::var("HOME") {
            Ok(home) if !home.is_empty() && path.starts_with(&home) => {
                format!("~{}", &path[home.len()..])
            }
            _ => path,
        }
    }

    /// 本次会话的审批策略（只读：信息行文案与命令回执都用它）。
    pub fn approval(&self) -> &ApprovalPolicy {
        &self.approval
    }

    /// 写一条会话事件（宿主用：会话结束）。写失败返回原因，由调用方如实上屏或打日志。
    ///
    /// **会话还没落盘时直接跳过**：没提交过就没有会话可记（见 [`Agent::ensure_session`]）。
    pub fn log_event(&mut self, event: SessionEvent) -> Option<String> {
        let files = self.files.as_mut()?;
        files
            .log
            .as_mut()?
            .append(&event)
            .err()
            .map(|error| error.to_string())
    }

    /// 第一次真正要写盘时开会话产物：建目录、开两个日志、写开场摘要与 `session_start`。
    ///
    /// **为什么延迟**：开着不用的 `jev chat` 不该在 `~/.jev-harness/sessions/` 里留下空会话
    /// （真调里攒了一堆 `turns=0` 的目录）。所以目录与文件都在这里才创建，幂等。
    fn ensure_session(&mut self) {
        let _ = std::fs::write(
            "/tmp/ens.txt",
            format!(
                "called files={} store={} dir={:?}\n",
                self.files.is_some(),
                self.store.is_some(),
                self.session_dir
            ),
        );
        if self.files.is_some() {
            return;
        }
        let (Some(store), Some(dir)) = (self.store.clone(), self.session_dir.clone()) else {
            return;
        };
        match SessionFiles::open(
            &store,
            &self.session_id,
            &dir,
            &self.workspace.display().to_string(),
            &self.model_label,
            &self.judge_label(),
            self.session_start.take(),
        ) {
            Ok(files) => {
                let _ = std::fs::write("/tmp/ens.txt", "OPEN_OK\n");
                self.files = Some(files);
            }
            // 落盘失败不打断会话，但绝不静默：宿主那一侧也会看到同一句。
            Err(error) => {
                let _ = std::fs::write("/tmp/ens.txt", format!("OPEN_ERR {error}\n"));
            }
        }
    }

    /// 开场事件里的环境读数（色彩 / 字形 / TTY）：能力探测在宿主那边，落盘前补进来。
    pub fn set_session_start_env(&mut self, color: String, glyphs: String, tty: bool) {
        if let Some(SessionEvent::SessionStart {
            color: slot_color,
            glyphs: slot_glyphs,
            tty: slot_tty,
            ..
        }) = self.session_start.as_mut()
        {
            *slot_color = color;
            *slot_glyphs = glyphs;
            *slot_tty = tty;
        }
    }

    /// 会话的判定器句柄（宿主跑 `/jev` 时用；`None` = 这次会话没有可用的判定器）。
    pub fn judge_handle(&self) -> Option<std::sync::Arc<dyn Judge>> {
        self.judge.clone()
    }

    /// 会话的运行标识（`/jev` 的请求快照用它）。
    pub fn run_id(&self) -> &str {
        &self.run
    }

    /// 会话判定器的 CLI 名字（报告里写「谁判的」）。
    pub fn judge_name(&self) -> JudgeName {
        self.judge_name
    }

    /// 装一次 `/jev` 手工判定的请求：用会话自己的判定器与运行标识。
    pub fn manual_judgment_request(
        &self,
        args: &str,
    ) -> Result<(jh_core::JudgeRequest, crate::ask::QuestionReport), String> {
        let parsed = crate::ask::parse_jev_args(args)?;
        let (request, question) = crate::ask::build_request(
            &self.run,
            &parsed.question,
            &parsed.labels,
            &BTreeMap::new(),
            parsed.material.as_deref(),
        )
        .map_err(|error| error.to_string())?;
        Ok((request, question))
    }

    /// `/jev` 的结果结算：读数上屏（行长与 CLI 同一份）+ 落一条 `manual_judgment`。
    ///
    /// **不采纳、不套门槛、不产生行动**：它只是把判定器的一次回答摆出来给人看。
    pub fn on_manual_judgment(
        &mut self,
        outcome: Result<jh_core::JudgeResponse, jh_core::Error>,
        latency_ms: u64,
        question: crate::ask::QuestionReport,
        updates: &mut Vec<Update>,
    ) {
        // 手工判定也是一次真实动作：读数值得留档，所以它同样让会话落盘
        // （与「开着不用不留痕」不冲突——用户确实问了）。
        self.ensure_session();
        let judge = self.judge_name.cli_name().to_owned();
        let material_bytes = question.material_bytes;
        match outcome.and_then(|response| {
            crate::ask::report(
                &self.run,
                format!("/jev {}", question.instructions),
                &judge,
                response,
                latency_ms,
                question.clone(),
            )
            .map_err(|error| jh_core::Error::InvalidResponse(error.to_string()))
        }) {
            Ok(report) => {
                for line in crate::ask::lines(&report) {
                    updates.push(Update::Notice {
                        text: line,
                        tone: NoticeTone::Info,
                    });
                }
                self.log(
                    SessionEvent::ManualJudgment {
                        at_unix_ms: now_unix_ms(),
                        question: report.question.instructions.clone(),
                        labels: report.question.criteria.keys().cloned().collect(),
                        material_bytes,
                        judge,
                        source: Some(report.judge.source.clone()),
                        version: Some(report.judge.version.clone()),
                        request_id: report.judge.request_id.clone(),
                        latency_ms: Some(report.judge.latency_ms),
                        choice: Some(report.answer.choice.clone()),
                        probabilities: report.answer.probabilities.clone(),
                        confidence: Some(report.answer.confidence),
                        margin: Some(report.answer.margin),
                        error: None,
                    },
                    updates,
                );
            }
            Err(error) => {
                let text = format!("`/jev` 没问成：{error}");
                updates.push(Update::Notice {
                    text: text.clone(),
                    tone: NoticeTone::Warning,
                });
                self.log(
                    SessionEvent::ManualJudgment {
                        at_unix_ms: now_unix_ms(),
                        question: question.instructions.clone(),
                        labels: question.criteria.keys().cloned().collect(),
                        material_bytes,
                        judge,
                        source: None,
                        version: None,
                        request_id: None,
                        latency_ms: None,
                        choice: None,
                        probabilities: BTreeMap::new(),
                        confidence: None,
                        margin: None,
                        error: Some(error.to_string()),
                    },
                    updates,
                );
            }
        }
    }

    /// 会话是否已经落盘（宿主据此决定要不要写收尾摘要）。
    pub fn session_started(&self) -> bool {
        self.files.is_some()
    }

    /// 往对话里加一条，并落进 `conversation.jsonl`（`/resume` 时原样喂回模型）。
    fn push_conversation(&mut self, item: InputItem, updates: &mut Vec<Update>) {
        if let Some(files) = self.files.as_mut()
            && let Some(log) = files.conversation.as_mut()
            && let Err(error) = log.append(&item)
        {
            updates.push(Update::Notice {
                text: format!("对话记录没写上：{error}"),
                tone: NoticeTone::Warning,
            });
        }
        self.conversation.push(item);
    }

    /// 会话历史仓库（宿主写摘要用；`None` = 不支持历史命令）。
    pub fn store(&self) -> Option<&SessionStore> {
        self.store.as_ref()
    }

    /// 本次会话的摘要（宿主在会话结束时写盘）。
    pub fn meta(&self) -> SessionMeta {
        SessionMeta {
            id: self.session_id.clone(),
            created_at_unix_ms: self.created_at_unix_ms,
            updated_at_unix_ms: crate::exp::unix_ms(),
            workspace: self.workspace.display().to_string(),
            model: self.model_label.clone(),
            judge: self.judge_label(),
            turns: self.stats.turns,
            first_message: clip(&self.first_message, 80),
            last_message: clip(&self.last_message, 80),
        }
    }

    /// 内部写日志；写不进去也不打断会话，但绝不静默。
    fn log(&mut self, event: SessionEvent, updates: &mut Vec<Update>) {
        if let Some(error) = self.log_event(event) {
            updates.push(Update::Notice {
                text: format!("会话日志没写上：{error}"),
                tone: NoticeTone::Warning,
            });
        }
    }

    /// 现在是否停在审批卡上（宿主用它判断「卡片是不是被关掉了」）。
    pub fn awaiting_approval(&self) -> bool {
        matches!(self.phase, Phase::Awaiting { .. })
    }

    /// 是否还有待办：在途步骤、排队中的调用，或欠模型一次回应。
    ///
    /// 宿主据此判断「[`Step::Idle`] 是真闲下来了，还是只是把活安排在了卡片后面」。
    pub fn busy(&self) -> bool {
        !matches!(self.phase, Phase::Idle) || !self.queue.is_empty() || self.owes_model
    }

    /// 执行一条会话命令（`/model`、`/permission`、`/status`），返回要上屏的更新。
    ///
    /// 命令改的都会话状态，所以信息行（模型 / 授权模式）跟着一起刷；命令本身**不进对话上下文**
    /// ——它不是发给模型的话，模型也不该看见管理员操作。
    pub fn on_command(&mut self, call: CommandCall) -> Vec<Update> {
        let mut updates = Vec::new();
        self.log(
            SessionEvent::Command {
                at_unix_ms: now_unix_ms(),
                name: call.name.to_owned(),
                args: call.args.clone(),
                scope: "host".to_owned(),
            },
            &mut updates,
        );
        match call.name {
            "model" => {
                if call.args.is_empty() {
                    updates.push(Update::Notice {
                        text: format!(
                            "当前模型：{}（判定器 {}，工作区 {}）。用法：/model <模型ID> 切换。",
                            self.model_label,
                            self.judge_label(),
                            self.header_title()
                        ),
                        tone: NoticeTone::Info,
                    });
                    // 有固定取值的列出来：当前模型（真配上了才算）+ 各预设的缺省模型 + 官方模型名。
                    let current = self.model_label.clone();
                    let mut values: Vec<String> = Vec::new();
                    if self.model.is_some() {
                        values.push(current.clone());
                    }
                    for provider in crate::provider::PROVIDERS {
                        if let Some(model) = provider.default_model
                            && !values.iter().any(|value| value == model)
                        {
                            values.push(model.to_owned());
                        }
                    }
                    for model in crate::provider::DEEPSEEK_MODELS {
                        if !values.iter().any(|value| value == model) {
                            values.push(model.to_owned());
                        }
                    }
                    let entries: Vec<jh_tui::app::ChoiceEntry> = values
                        .into_iter()
                        .map(|value| {
                            let label = if value == current {
                                "当前".to_owned()
                            } else if crate::provider::DEEPSEEK_MODELS.contains(&value.as_str()) {
                                "DeepSeek 官方模型".to_owned()
                            } else {
                                "预设缺省".to_owned()
                            };
                            jh_tui::app::ChoiceEntry { value, label }
                        })
                        .collect();
                    updates.push(Update::Choices {
                        command: "model",
                        title: "模型 · ↑↓ 选择 · Enter 切换 · Esc 关闭（其他 ID 直接 /model <ID>）"
                            .to_owned(),
                        entries,
                    });
                } else {
                    match ResponsesModel::with_model(&call.args) {
                        Ok(model) => {
                            let previous =
                                std::mem::replace(&mut self.model_label, call.args.clone());
                            self.model = Some(Arc::new(model));
                            updates.push(Update::Notice {
                                text: format!(
                                    "模型已切换：{previous} → {}（下一轮生效）",
                                    self.model_label
                                ),
                                tone: NoticeTone::Info,
                            });
                        }
                        Err(error) => updates.push(Update::Notice {
                            text: format!("切换模型失败：{}", crate::session::call_reason(&error)),
                            tone: NoticeTone::Warning,
                        }),
                    }
                }
                updates.push(Update::HeaderTitle(self.header_title()));
                updates.push(Update::PromptInfo(self.prompt_info()));
            }
            // 切完模式要刷新提示框底边的信息行：状态行还写着旧模式的话，用户根本没法确认
            // 自己现在处于哪种授权模式（安全相关的反馈，真机踩到）。
            "permission" => {
                match call.args.trim() {
                    "" => {
                        let current = self.approval.mode();
                        updates.push(Update::Notice {
                            text: format!(
                                "授权模式：{}。{PERMISSION_USAGE}",
                                self.approval.mode_label()
                            ),
                            tone: NoticeTone::Info,
                        });
                        // 三种模式 + reset 都是固定取值：列出来用选的，不必记着拼。
                        let mut entries: Vec<jh_tui::app::ChoiceEntry> = PermissionMode::ALL
                            .iter()
                            .map(|mode| jh_tui::app::ChoiceEntry {
                                value: mode.as_str().to_owned(),
                                label: if *mode == current {
                                    format!("{} · 当前", mode.label())
                                } else {
                                    mode.label().to_owned()
                                },
                            })
                            .collect();
                        entries.push(jh_tui::app::ChoiceEntry {
                            value: "reset".to_owned(),
                            label: "清空白名单，回到 auto".to_owned(),
                        });
                        updates.push(Update::Choices {
                            command: "permission",
                            title: "授权模式 · ↑↓ 选择 · Enter 切换 · Esc 关闭".to_owned(),
                            entries,
                        });
                    }
                    "reset" => {
                        let had = self.approval.allowed().count();
                        self.approval.reset();
                        updates.push(Update::Notice {
                            text: format!(
                                "已恢复缺省：授权模式 auto，白名单清空（原有 {had} 项）。"
                            ),
                            tone: NoticeTone::Info,
                        });
                    }
                    other => match PermissionMode::parse(other) {
                        Some(mode) => {
                            self.approval.set_mode(mode);
                            let meaning = match mode {
                                PermissionMode::Auto => {
                                    "只读工具直接放行，其余问 Jev，采纳即执行；判不了或破坏性命令上卡。"
                                }
                                PermissionMode::AlwaysAsk => {
                                    "每次工具调用都上卡（已「总是允许」的工具仍直接放行），不问 Jev。"
                                }
                                PermissionMode::NeverAsk => {
                                    "工具调用直接执行、不再上卡；破坏性命令也只上屏提示，请自行确认。"
                                }
                            };
                            updates.push(Update::Notice {
                                text: format!("授权模式已换成 `{}`：{meaning}", mode.as_str()),
                                tone: if mode == PermissionMode::NeverAsk {
                                    NoticeTone::Warning
                                } else {
                                    NoticeTone::Info
                                },
                            });
                        }
                        None => updates.push(Update::Notice {
                            text: format!("不认识的授权模式 `{other}`。{PERMISSION_USAGE}"),
                            tone: NoticeTone::Warning,
                        }),
                    },
                }
                updates.push(Update::PromptInfo(self.prompt_info()));
            }
            "sessions" | "resume" => {
                if call.args.trim().is_empty() {
                    self.list_sessions(&mut updates);
                } else {
                    self.resume_session(call.args.trim(), &mut updates);
                }
            }
            "status" => {
                let stats = self.stats;
                updates.push(Update::Notice {
                    text: format!(
                        "会话读数：{} 轮 · 模型 {} 次 · 判定 {} 次（防抖复用 {}）· 工具 {} 次 · \
                         门控拒绝 {} · 人工拒绝 {}",
                        stats.turns,
                        stats.model_calls,
                        stats.judgments,
                        stats.judge_reuses,
                        stats.tool_calls,
                        stats.refusals,
                        stats.denials
                    ),
                    tone: NoticeTone::Info,
                });
            }
            CONTINUE_COMMAND => {
                if self.busy() {
                    updates.push(Update::Notice {
                        text: "这一轮还在跑：等它收尾，或按 Esc 取消后再 /continue".to_owned(),
                        tone: NoticeTone::Warning,
                    });
                } else if self.limit_paused {
                    // 上限只是把这一轮停在半路：计数归零，「还欠模型一次回应」重新记上，接着跑。
                    self.rounds = 0;
                    self.limit_paused = false;
                    self.owes_model = true;
                    self.turn_started.get_or_insert_with(Instant::now);
                    updates.push(Update::Notice {
                        text: format!(
                            "接着跑：本轮模型调用计数已归零（上限仍是 {} 次，可再次 /continue）",
                            self.max_rounds
                        ),
                        tone: NoticeTone::Info,
                    });
                    updates.push(Update::Status(self.running(WaitingReason::LlmStreaming)));
                } else {
                    updates.push(Update::Notice {
                        text: "没有需要接着跑的活：/continue 只在上一次因模型调用上限停下后有用"
                            .to_owned(),
                        tone: NoticeTone::Info,
                    });
                }
            }
            other => updates.push(Update::Notice {
                text: format!("命令 /{other} 没有宿主实现（这是实现缺口，请登记）。"),
                tone: NoticeTone::Warning,
            }),
        }
        updates
    }

    /// 工具块那一行的参数摘要：与实时执行时同一口径（[`Tools::detail`]）。
    ///
    /// 回放时手上只有对话记录里的原始 JSON 参数，所以包成一个 `ToolCall` 再交给工具表，
    /// 免得历史回放的摘要与当时上屏的写法对不上。
    fn tool_args_display(&self, name: &str, arguments: &str) -> String {
        self.tools.detail(&ToolCall {
            call_id: String::new(),
            name: name.to_owned(),
            arguments: arguments.to_owned(),
        })
    }

    /// `/sessions`：列出最近的会话（摘要 + 接回提示）。
    fn list_sessions(&mut self, updates: &mut Vec<Update>) {
        let Some(store) = self.store.as_ref() else {
            updates.push(Update::Notice {
                text: "会话历史不可用：取不到历史目录（设置 HOME 或 JEV_HARNESS_HOME）".to_owned(),
                tone: NoticeTone::Warning,
            });
            return;
        };
        let list = match store.list() {
            Ok(list) => list,
            Err(error) => {
                updates.push(Update::Notice {
                    text: format!("会话历史读不出来：{error}"),
                    tone: NoticeTone::Warning,
                });
                return;
            }
        };
        if list.metas.is_empty() {
            updates.push(Update::Notice {
                text: format!("还没有历史会话（存放在 {}）", store.root().display()),
                tone: NoticeTone::Info,
            });
            return;
        }
        // 读不出摘要的会话如实报一句：坏 `meta.json` 不该让会话「凭空消失」。
        if !list.skipped.is_empty() {
            let detail: Vec<String> = list
                .skipped
                .iter()
                .take(3)
                .map(|(id, reason)| format!("{id}（{reason}）"))
                .collect();
            updates.push(Update::Notice {
                text: format!(
                    "有 {} 个会话读不出摘要，没列进上面：{}",
                    list.skipped.len(),
                    detail.join("；")
                ),
                tone: NoticeTone::Warning,
            });
        }

        // 递一份选项列表给界面：`↑↓` 选、`Enter` 接回；文本回执保留，便于复制 id。
        let entries: Vec<jh_tui::app::ChoiceEntry> = list
            .metas
            .iter()
            .take(20)
            .map(|meta| jh_tui::app::ChoiceEntry {
                value: meta.id.clone(),
                label: crate::session::store::label(meta),
            })
            .collect();
        let total = list.metas.len();
        updates.push(Update::Choices {
            command: "resume",
            title: format!("历史会话（{total} 个）· ↑↓ 选择 · Enter 接回 · Esc 关闭"),
            entries,
        });
        let shown: Vec<String> = list.metas.iter().take(10).map(describe).collect();
        updates.push(Update::Notice {
            text: format!(
                "会话历史存放在 {}（当前会话 {}）：\n{}\n也可以直接 `/resume <会话ID>`",
                store.root().display(),
                self.session_id,
                shown.join("\n"),
            ),
            tone: NoticeTone::Info,
        });
    }

    /// `/resume <ID>`：把历史会话的对话上下文接回来，并在界面上回放最近的消息。
    ///
    /// 恢复的是**模型可见的对话**与界面回放；语义状态（判断节点、依赖图）与授权白名单不跨进程
    /// 恢复（进程内对象，1.0 不做重放），回执里如实说明。
    fn resume_session(&mut self, id: &str, updates: &mut Vec<Update>) {
        if self.busy() {
            updates.push(Update::Notice {
                text: "这一轮还在跑：等它收尾，或按 Esc 取消后再 /resume".to_owned(),
                tone: NoticeTone::Warning,
            });
            return;
        }
        // `/resume` 会把历史补进本次会话的对话记录：这也算「真的开始了」。
        self.ensure_session();
        // `/resume` 会把历史补进本次会话的对话记录：这也算「真的开始了」。
        self.ensure_session();
        let Some(store) = self.store.as_ref() else {
            updates.push(Update::Notice {
                text: "会话历史不可用：取不到历史目录（设置 HOME 或 JEV_HARNESS_HOME）".to_owned(),
                tone: NoticeTone::Warning,
            });
            return;
        };
        let meta = match store.read_meta(id) {
            Ok(meta) => meta,
            Err(error) => {
                updates.push(Update::Notice {
                    text: format!("接不回来：{error}"),
                    tone: NoticeTone::Warning,
                });
                return;
            }
        };
        let items = match store.read_conversation(id) {
            Ok(items) => items,
            Err(error) => {
                updates.push(Update::Notice {
                    text: format!("接不回来：{error}"),
                    tone: NoticeTone::Warning,
                });
                return;
            }
        };

        // 把接回来的对话补进本次会话的对话记录，这样本次目录自包含（将来再 /resume 不丢前文）。
        if let Some(files) = self.files.as_mut()
            && let Some(log) = files.conversation.as_mut()
        {
            for item in &items {
                if let Err(error) = log.append(item) {
                    updates.push(Update::Notice {
                        text: format!("对话记录没写上：{error}"),
                        tone: NoticeTone::Warning,
                    });
                    break;
                }
            }
        }
        self.conversation = items;

        // 界面回放：用户消息、助手正文、每次工具调用（含输出）都按原顺序画出来。
        //
        // 工具调用与输出在对话记录里是分开的项（模型可能一次要多个），所以按 `call_id` 配对，
        // 并且**在输出那一项处**成对画（顺序 = 当时的执行顺序）。状态标
        // [`ToolStatus::Restored`]：记录里没有当时的状态，不假装成功也不假装失败。
        let mut messages = 0usize;
        let mut tool_items = 0usize;
        let mut pending: std::collections::HashMap<String, (String, String)> =
            std::collections::HashMap::new();
        updates.push(Update::Notice {
            text: format!("── 以下是接回的历史（{}） ──", meta.id),
            tone: NoticeTone::Info,
        });
        for item in &self.conversation {
            match item {
                InputItem::Message { role, content } => {
                    messages += 1;
                    if matches!(role, jh_openai::Role::User) {
                        updates.push(Update::User {
                            text: content.clone(),
                            at: String::new(),
                        });
                    } else if !content.trim().is_empty() {
                        updates.push(Update::AssistantEnd(content.clone()));
                    }
                }
                InputItem::FunctionCall {
                    call_id,
                    name,
                    arguments,
                } => {
                    pending.insert(call_id.clone(), (name.clone(), arguments.clone()));
                }
                InputItem::FunctionCallOutput { call_id, output } => {
                    tool_items += 1;
                    let (name, arguments) = pending
                        .remove(call_id)
                        .unwrap_or_else(|| ("工具".to_owned(), String::new()));
                    updates.push(Update::ToolBegin {
                        name: name.clone(),
                        args: self.tool_args_display(&name, &arguments),
                    });
                    updates.push(Update::ToolEnd {
                        status: ToolStatus::Restored,
                        detail: output.clone(),
                    });
                }
            }
        }
        // 有调用没等到输出（那一轮被打断）：如实画出来，不当作成功。
        for (_, (name, arguments)) in pending {
            tool_items += 1;
            updates.push(Update::ToolBegin {
                args: self.tool_args_display(&name, &arguments),
                name,
            });
            updates.push(Update::ToolEnd {
                status: ToolStatus::Restored,
                detail: "（这一轮没有留下结果）".to_owned(),
            });
        }
        self.rounds = 0;
        self.owes_model = false;
        self.limit_paused = false;
        self.queue.clear();
        self.turn_started = None;
        self.phase = Phase::Idle;
        updates.push(Update::Notice {
            text: format!(
                "已接回会话 {}（{} · {} 轮）：回放 {} 条消息、{} 条工具记录；模型上下文已接上。\
                 语义状态与授权白名单不跨进程恢复。",
                meta.id, meta.model, meta.turns, messages, tool_items
            ),
            tone: NoticeTone::Info,
        });
    }

    /// 用户提交一句话：回显、写证据、准备开轮。
    ///
    /// **一轮还在跑时不接受新提交**（[`Agent::busy`]）：否则用户块会变成末尾块，工具结果与
    /// 助手正文都无处回填。宿主在调用前先查 `busy()`，把话放回输入框（[`BUSY_NOTICE`]）。
    pub fn submit(&mut self, text: String) -> Vec<Update> {
        if self.busy() {
            return vec![Update::Notice {
                text: BUSY_NOTICE.to_owned(),
                tone: NoticeTone::Warning,
            }];
        }
        let mut updates = Vec::new();
        // 第一次提交才算「这场会话真的开始了」：此刻建目录与文件（延迟落盘）。
        self.ensure_session();
        let at = now_unix_ms();
        updates.push(Update::User {
            text: text.clone(),
            at: view::at_utc(at),
        });
        self.append_evidence(
            view::objects::SESSION_ASK,
            "用户",
            at,
            serde_json::json!({"note": clip(&text, 60), "ask": text.clone()}),
            &mut updates,
        );
        self.push_conversation(InputItem::user_text(text.clone()), &mut updates);
        self.owes_model = true;
        self.rounds = 0;
        // 新话就是新的活：上一次「停在上限」的欠账不再单独记，交给这次提交一起带走。
        self.limit_paused = false;
        if self.first_message.is_empty() {
            self.first_message = text.clone();
        }
        self.last_message = text.clone();
        self.stats.turns += 1;
        let turn = self.stats.turns;
        self.log(
            SessionEvent::UserMessage {
                at_unix_ms: at,
                turn,
                text: LoggedText::new(text.clone()),
            },
            &mut updates,
        );
        if self.turn_started.is_none() {
            self.turn_started = Some(Instant::now());
        }
        updates.push(Update::Status(self.running(WaitingReason::LlmStreaming)));
        updates.push(self.panels_update());
        updates
    }

    /// 推进一步。**手上有在途步骤时不要调用**（会重复发同一步）。
    pub fn advance(&mut self, now: Instant) -> (Step, Vec<Update>) {
        let mut updates = Vec::new();
        let step = match &self.phase {
            Phase::Idle => {
                if let Some(call) = self.queue.pop_front() {
                    self.start_gate(call, now, &mut updates)
                } else if self.owes_model {
                    self.start_model(&mut updates)
                } else {
                    Step::Idle
                }
            }
            Phase::Running { call, .. } => Step::Tool(call.clone()),
            _ => Step::Idle,
        };
        (step, updates)
    }

    /// 模型调用回来了。
    pub fn on_model_result(&mut self, result: Result<ModelTurn, Error>) -> Vec<Update> {
        let mut updates = Vec::new();
        if !matches!(self.phase, Phase::Modeling) {
            return updates;
        }
        self.phase = Phase::Idle;
        match result {
            Ok(turn) => {
                self.record_model_call(&turn, None, &mut updates);
                let session_turn = self.stats.turns;
                let round = self.rounds;
                let usage = turn.usage.clone();
                self.log(
                    SessionEvent::ModelResponse {
                        at_unix_ms: now_unix_ms(),
                        turn: session_turn,
                        round,
                        model: turn.model.clone(),
                        request_id: turn.request_id.clone(),
                        latency_ms: turn.latency_ms,
                        input_tokens: usage.as_ref().and_then(|usage| usage.input_tokens),
                        output_tokens: usage.as_ref().and_then(|usage| usage.output_tokens),
                        text: LoggedText::new(turn.text.clone()),
                        tool_calls: turn
                            .calls
                            .iter()
                            .map(|call| LoggedToolCall {
                                name: call.name.clone(),
                                arguments: call.arguments.clone(),
                            })
                            .collect(),
                        error: None,
                    },
                    &mut updates,
                );
                updates.push(Update::AssistantEnd(turn.text.clone()));
                if !turn.text.trim().is_empty() {
                    // 走 `push_conversation`：助手正文也要落进 `conversation.jsonl`，
                    // 否则接回来的历史里只剩用户消息（真调里踩过）。
                    self.push_conversation(
                        InputItem::assistant_text(turn.text.clone()),
                        &mut updates,
                    );
                }
                for call in &turn.calls {
                    self.push_conversation(
                        InputItem::function_call(
                            call.name.clone(),
                            call.call_id.clone(),
                            call.arguments.clone(),
                        ),
                        &mut updates,
                    );
                }
                if turn.calls.is_empty() {
                    self.finish_turn(true, &mut updates);
                } else {
                    updates.push(Update::Notice {
                        text: format!("模型请求了 {} 个工具调用", turn.calls.len()),
                        tone: NoticeTone::Info,
                    });
                    self.queue.extend(turn.calls);
                }
            }
            Err(error) => {
                self.record_model_failure(&error, &mut updates);
                let session_turn = self.stats.turns;
                let round = self.rounds;
                let reason = crate::session::call_reason(&error);
                self.log(
                    SessionEvent::ModelResponse {
                        at_unix_ms: now_unix_ms(),
                        turn: session_turn,
                        round,
                        model: self.model_label.clone(),
                        request_id: None,
                        latency_ms: 0,
                        input_tokens: None,
                        output_tokens: None,
                        text: LoggedText::new(String::new()),
                        tool_calls: Vec::new(),
                        error: Some(reason),
                    },
                    &mut updates,
                );
                updates.push(Update::AssistantEnd(String::new()));
                updates.push(Update::Notice {
                    text: format!("模型调用失败：{}", crate::session::call_reason(&error)),
                    tone: NoticeTone::Error,
                });
                self.finish_turn(false, &mut updates);
            }
        }
        updates.push(self.panels_update());
        updates
    }

    /// 门控判定回来了（宿主在 [`Step::Gate`] 之后调用）。
    pub fn on_gate_result(&mut self, decision: GateDecision, now: Instant) -> Vec<Update> {
        let Phase::Gating { call, node } = std::mem::replace(&mut self.phase, Phase::Idle) else {
            return Vec::new();
        };
        let mut updates = Vec::new();
        self.settle_gate(call, node, decision, now, &mut updates);
        updates
    }

    /// 人工裁决回来了。
    pub fn on_approval(&mut self, outcome: ApprovalOutcome) -> Vec<Update> {
        let mut updates = Vec::new();
        let Phase::Awaiting { call, node } = std::mem::replace(&mut self.phase, Phase::Idle) else {
            return updates;
        };
        self.approval.record(&call.name, outcome);
        let decision_word = match outcome {
            ApprovalOutcome::AllowOnce => "allow_once",
            ApprovalOutcome::AllowAlways => "allow_always",
            ApprovalOutcome::Deny => "deny",
        };
        let waited_ms = self
            .approval_opened_at
            .take()
            .map(|at| at.elapsed().as_millis() as u64)
            .unwrap_or(0);
        let session_turn = self.stats.turns;
        let round = self.rounds;
        self.log(
            SessionEvent::ApprovalResult {
                at_unix_ms: now_unix_ms(),
                turn: session_turn,
                round,
                tool: call.name.clone(),
                decision: decision_word.to_owned(),
                waited_ms,
            },
            &mut updates,
        );
        match outcome {
            ApprovalOutcome::AllowOnce | ApprovalOutcome::AllowAlways => {
                // **判定不可用时，前提就是人的决定。**
                //
                // 判定器判不了（`Ask` 路径）时那个判断节点被标成 `Failed`、按定义「不可依赖」；
                // 若仍把它当行动前提，批准之后的前提复核必然挡下 → 界面表现为「批准了却不执行、
                // 模型反复重发同一条命令」（真机踩到，`--judge rule` 或没配 Jev 凭据时必现）。
                // 这里只在节点**确实可依赖**时挂它；否则这条行动不挂判断前提（它的依据就是这次人工裁决），
                // 前提复核于是通过——不可逆动作的红线由确定性层（破坏性标记）与授权模式守着。
                let premise = node.filter(|node| {
                    self.state
                        .node(*node)
                        .is_some_and(jh_core::JudgmentNode::is_usable)
                });
                if premise.is_none() && node.is_some() {
                    updates.push(Update::Notice {
                        text: "这次调用没有可依赖的判定依据：按你的人工裁决执行".to_owned(),
                        tone: NoticeTone::Info,
                    });
                }
                self.begin_run(call, premise, &mut updates);
            }
            ApprovalOutcome::Deny => {
                self.stats.denials += 1;
                let text = "用户拒绝了这次调用；请换一个做法，不要重试同一条命令".to_owned();
                updates.push(Update::ToolBegin {
                    name: call.name.clone(),
                    args: self.tools.detail(&call),
                });
                updates.push(Update::ToolEnd {
                    status: ToolStatus::Failed,
                    detail: "已拒绝".to_owned(),
                });
                updates.push(Update::Notice {
                    text: format!("已拒绝 {}：{text}", self.tools.display(&call)),
                    tone: NoticeTone::Warning,
                });
                self.push_conversation(
                    InputItem::function_call_output(call.call_id.clone(), text),
                    &mut updates,
                );
                self.after_call(&mut updates);
            }
        }
        updates.push(Update::PromptInfo(self.prompt_info()));
        updates.push(self.panels_update());
        updates
    }

    /// 审批卡被关掉但没给裁决（`Esc`）：这次调用不执行，如实告诉模型。
    pub fn on_cancel(&mut self) -> Vec<Update> {
        let mut updates = Vec::new();
        let Phase::Awaiting { call, .. } = std::mem::replace(&mut self.phase, Phase::Idle) else {
            return updates;
        };
        self.stats.denials += 1;
        let waited_ms = self
            .approval_opened_at
            .take()
            .map(|at| at.elapsed().as_millis() as u64)
            .unwrap_or(0);
        let session_turn = self.stats.turns;
        let round = self.rounds;
        self.log(
            SessionEvent::ApprovalResult {
                at_unix_ms: now_unix_ms(),
                turn: session_turn,
                round,
                tool: call.name.clone(),
                decision: "cancel".to_owned(),
                waited_ms,
            },
            &mut updates,
        );
        updates.push(Update::ToolBegin {
            name: call.name.clone(),
            args: self.tools.detail(&call),
        });
        updates.push(Update::ToolEnd {
            status: ToolStatus::Failed,
            detail: "未裁决".to_owned(),
        });
        updates.push(Update::Notice {
            text: "未裁决：这次调用没有执行".to_owned(),
            tone: NoticeTone::Warning,
        });
        self.push_conversation(
            InputItem::function_call_output(
                call.call_id.clone(),
                "用户没有裁决这次调用（卡片被关闭），未执行".to_owned(),
            ),
            &mut updates,
        );
        self.after_call(&mut updates);
        updates
    }

    /// 工具执行回来了。
    pub fn on_tool_result(&mut self, outcome: ToolOutcome) -> Vec<Update> {
        let mut updates = Vec::new();
        let Phase::Running { call, action } = std::mem::replace(&mut self.phase, Phase::Idle)
        else {
            return updates;
        };
        updates.push(Update::ToolEnd {
            status: outcome.status,
            detail: clip(&outcome.text, 400),
        });
        let latency_ms = self
            .tool_started
            .take()
            .map(|at| at.elapsed().as_millis() as u64)
            .unwrap_or(0);
        let session_turn = self.stats.turns;
        let round = self.rounds;
        let status = match outcome.status {
            ToolStatus::Ok => "ok",
            ToolStatus::Failed => "failed",
            ToolStatus::Running => "running",
            ToolStatus::Restored => "restored",
        };
        self.log(
            SessionEvent::ToolCall {
                at_unix_ms: now_unix_ms(),
                turn: session_turn,
                round,
                name: call.name.clone(),
                arguments: self.tools.detail(&call),
                status: status.to_owned(),
                output: LoggedText::new(outcome.text.clone()),
                latency_ms,
            },
            &mut updates,
        );

        // 证据：工具结果出新版本——读过 `tool.result` 的判断（上一次门控）由此变旧。
        let note = format!(
            "{} {}",
            self.tools.display(&call),
            status_word(outcome.status)
        );
        let at = now_unix_ms();
        self.append_evidence(
            view::objects::TOOL_RESULT,
            &call.name,
            at,
            serde_json::json!({
                "note": note,
                "tool": call.name,
                "status": status_word(outcome.status),
                "output": clip(&outcome.text, 2000),
            }),
            &mut updates,
        );

        // 行动的前提是否还立得住：这次执行的依据有没有在途中变旧（研究视图的「动作」一节读它）。
        if let Some(action) = &action
            && let Ok(PremiseCheck::Blocked { .. }) = self.state.check_action(action)
        {
            updates.push(Update::Notice {
                text: format!("行动 {action} 的前提已失效：这次执行只作历史记录"),
                tone: NoticeTone::Warning,
            });
        }

        self.push_conversation(
            InputItem::function_call_output(call.call_id.clone(), outcome.text),
            &mut updates,
        );
        self.after_call(&mut updates);
        updates.push(self.panels_update());
        updates
    }

    /// 取消当前轮（`Esc`）：在途的调用作废，队列清空，如实上屏。
    pub fn on_interrupt(&mut self) -> Vec<Update> {
        let mut updates = Vec::new();
        let phase = std::mem::replace(&mut self.phase, Phase::Idle);
        let stage = match &phase {
            Phase::Modeling => "model",
            Phase::Gating { .. } => "gate",
            Phase::Running { .. } => "tool",
            Phase::Awaiting { .. } => "approval",
            Phase::Idle => "idle",
        };
        let session_turn = self.stats.turns;
        let round = self.rounds;
        self.log(
            SessionEvent::Interrupt {
                at_unix_ms: now_unix_ms(),
                turn: session_turn,
                round,
                stage: stage.to_owned(),
            },
            &mut updates,
        );
        self.queue.clear();
        self.owes_model = false;
        // 取消就是取消：上限留下的欠账也一并作废（要接着跑就重新说一句）。
        self.limit_paused = false;
        match phase {
            Phase::Modeling => {
                // 收尾这一块，否则屏上留着永久的「还在长」三角。
                updates.push(Update::AssistantEnd(String::new()));
                updates.push(Update::Notice {
                    text: "已取消本轮：模型调用在途，这次回复作废".to_owned(),
                    tone: NoticeTone::Warning,
                });
            }
            Phase::Running { call, .. } => {
                updates.push(Update::ToolEnd {
                    status: ToolStatus::Failed,
                    detail: "已取消".to_owned(),
                });
                self.push_conversation(
                    InputItem::function_call_output(
                        call.call_id.clone(),
                        "用户取消了这一轮，调用未完成".to_owned(),
                    ),
                    &mut updates,
                );
                updates.push(Update::Notice {
                    text: format!("已取消：{}", self.tools.display(&call)),
                    tone: NoticeTone::Warning,
                });
            }
            Phase::Gating { call, .. } | Phase::Awaiting { call, .. } => {
                updates.push(Update::Notice {
                    text: format!("已取消：{} 没有执行", self.tools.display(&call)),
                    tone: NoticeTone::Warning,
                });
            }
            Phase::Idle => updates.push(Update::Notice {
                text: "没有在途的调用".to_owned(),
                tone: NoticeTone::Info,
            }),
        }
        self.finish_turn(false, &mut updates);
        updates.push(self.panels_update());
        updates
    }

    /// 一条「刷新研究面板」的更新（面板数据体积大，装进盒子）。
    fn panels_update(&self) -> Update {
        Update::Panels(Box::new(self.panels()))
    }

    /// 研究面板的三份数据（宿主每次整份替换）。
    pub fn panels(&self) -> Panels {
        Panels {
            judgment: self
                .last_node
                .and_then(|id| self.state.node(id))
                .map(|node| {
                    view::judgment_view(node, &self.question.question, self.question.version)
                }),
            evidence: view::evidence_view(self.state.evidence()),
            propagation: view::propagation_view(&self.state, self.last_invalidation.as_ref()),
        }
    }

    /// 起一次模型调用。
    fn start_model(&mut self, updates: &mut Vec<Update>) -> Step {
        self.owes_model = false;
        // 0 = 不限：上限只在一轮之内计数，命中后停在「还有话没答完」的位置而不是无声收尾。
        if self.max_rounds != 0 && self.rounds >= self.max_rounds {
            updates.push(Update::Notice {
                text: format!(
                    "本轮已达 {} 次模型调用上限；输入 /continue 继续（对话与工作区状态都保留）",
                    self.max_rounds
                ),
                tone: NoticeTone::Warning,
            });
            self.finish_turn(true, updates);
            self.limit_paused = true;
            let turn = self.stats.turns;
            let text = format!("本轮已达 {} 次模型调用上限", self.max_rounds);
            self.log(
                SessionEvent::Error {
                    at_unix_ms: now_unix_ms(),
                    turn,
                    round: self.rounds,
                    stage: "budget".to_owned(),
                    text,
                },
                updates,
            );
            return Step::Idle;
        }
        if self.model.is_none() {
            updates.push(Update::Notice {
                text: format!(
                    "模型不可用（{}）：配好 LLM provider 凭据（默认 DEEPSEEK_API_KEY）后重启会话",
                    self.model_label
                ),
                tone: NoticeTone::Error,
            });
            self.finish_turn(false, updates);
            let turn = self.stats.turns;
            let text = self.model_label.clone();
            self.log(
                SessionEvent::Error {
                    at_unix_ms: now_unix_ms(),
                    turn,
                    round: self.rounds,
                    stage: "model".to_owned(),
                    text,
                },
                updates,
            );
            return Step::Idle;
        }
        let request = ModelRequest {
            instructions: self.instructions(),
            input: self.conversation.clone(),
            tools: self.tools.definitions(),
        };
        self.last_request_bytes = serde_json::to_vec(&request.input)
            .map(|bytes| bytes.len() as u64)
            .unwrap_or(0);
        self.rounds += 1;
        self.limit_paused = false;
        self.stats.model_calls += 1;
        self.phase = Phase::Modeling;
        let turn = self.stats.turns;
        let round = self.rounds;
        let tools: Vec<String> = request
            .tools
            .iter()
            .map(|tool| match tool {
                jh_openai::Tool::Function { name, .. } => name.clone(),
            })
            .collect();
        let items = request.input.len();
        let input_bytes = self.last_request_bytes;
        self.log(
            SessionEvent::ModelRequest {
                at_unix_ms: now_unix_ms(),
                turn,
                round,
                model: self.model_label.clone(),
                input_bytes,
                items,
                tools,
            },
            updates,
        );
        updates.push(Update::Status(self.running(WaitingReason::LlmStreaming)));
        Step::Model(request)
    }

    /// 起一次门控：确定性层先分类，Jev 只看 shell 命令，判定先查防抖缓存。
    fn start_gate(&mut self, call: ToolCall, now: Instant, updates: &mut Vec<Update>) -> Step {
        let display = self.tools.display(&call);

        // 风险由工具自己报（`bash` 看命令，`write_file` 覆盖已有文件算破坏性）。
        let risk = self.tools.risk(&call);

        // 只读**工具**（不是「只读命令」）：确定性层已经能判（工作区内、不改动任何东西），不问 Jev。
        // `bash` 永远走门控——命令恰好只读也不改变它「能改东西」这件事。
        if self.tools.is_read_only(&call.name) {
            let direct = self.bypasses_the_card(&call.name);
            updates.push(Update::Notice {
                text: if direct {
                    format!("确定性层：{display} 是工作区内的只读工具，直接放行")
                } else {
                    format!("确定性层：{display} 是工作区内的只读工具，但当前模式要人工确认")
                },
                tone: NoticeTone::Info,
            });
            if self.bypasses_the_card(&call.name) {
                self.log_gate_shortcut(
                    &call,
                    Risk::ReadOnly,
                    "只读工具：确定性层直接放行".to_owned(),
                    updates,
                );
                self.begin_run(call, None, updates);
                return Step::Idle;
            }
            self.open_card(call, None, None, Risk::ReadOnly, updates);
            return Step::Idle;
        }

        // 非 auto 模式不问 Jev：`ask` 一律上卡，`never` 直接执行——省下的调用是真的，
        // 别为了「流程一致」白花一次判定。
        if self.approval.mode() != PermissionMode::Auto {
            let command = command_of(&call);
            let risk = gate::assess(&command);
            // 白名单先生效：卡片上的「总是允许」是用户自己的授权，任何模式下都算数。
            if self.approval.is_preapproved(&call.name) {
                updates.push(Update::Notice {
                    text: format!("{} 在本次会话白名单里，直接放行", call.name),
                    tone: NoticeTone::Info,
                });
                self.log_gate_shortcut(
                    &call,
                    risk,
                    format!("{} 在本次会话白名单里，直接放行", call.name),
                    updates,
                );
                self.begin_run(call, None, updates);
                return Step::Idle;
            }
            match self.approval.mode() {
                PermissionMode::NeverAsk => {
                    updates.push(Update::Notice {
                        text: format!("授权模式「从不询问」：{display} 直接执行，不上卡"),
                        tone: NoticeTone::Info,
                    });
                    self.log_gate_shortcut(
                        &call,
                        risk,
                        "授权模式「从不询问」：直接执行".to_owned(),
                        updates,
                    );
                    self.begin_run(call, None, updates);
                    return Step::Idle;
                }
                PermissionMode::AlwaysAsk => {
                    updates.push(Update::Notice {
                        text: format!("授权模式「每次询问」：{display} 直接上卡，不问 Jev"),
                        tone: NoticeTone::Info,
                    });
                    self.open_card(call, None, None, risk, updates);
                    return Step::Idle;
                }
                PermissionMode::Auto => unreachable!("auto 走下面的 Jev 路径"),
            }
        }

        // 材料证据：这次调用本身出新版本（读过它的旧判断由此变旧）。
        // `bash` 记 `shell.command`；写/编辑记 `file.change`——两者都进判断材料，但语义不能混。
        let subject = if call.name == BASH {
            command_of(&call)
        } else {
            self.tools.detail(&call)
        };
        let note = clip(&format!("{display}（{}）", risk.label()), 60);
        let payload = if call.name == BASH {
            serde_json::json!({
                "note": note,
                "command": subject,
                "cwd": self.workspace.display().to_string(),
                "risk": risk.label(),
            })
        } else {
            serde_json::json!({
                "note": note,
                "target": subject,
                "risk": risk.label(),
            })
        };
        let object = if call.name == BASH {
            view::objects::SHELL_COMMAND
        } else {
            view::objects::FILE_CHANGE
        };
        self.append_evidence(object, "会话", now_unix_ms(), payload, updates);

        // 材料与投影同源：装进去的对象就是登记读取的对象。
        let (state, projection, snapshot) = self.material();
        let node = self.state.declare_node(
            QuestionId::from(gate::QUESTION_ID),
            self.question.version,
            projection,
        );
        self.last_node = Some(node);

        let key = gate::material_key(&call.name, &call.arguments);
        // 防抖：窗口内同一份材料直接复用上一次判定，不再发调用。
        if let Some(decision) = self.debounce.hit(&key, now) {
            self.settle_gate(call, node, decision, now, updates);
            return Step::Idle;
        }

        let Some(judge) = self.judge.clone() else {
            let decision = GateDecision {
                key,
                verdict: Verdict::Ask,
                answer: None,
                margin: None,
                confidence: None,
                source: self.judge_name.source(),
                executor: None,
                request_id: None,
                latency_ms: 0,
                failure: Some("判定器不可用".to_owned()),
                reason: format!(
                    "判定不可用（判定器 {} 未接线或缺少凭据）；交人工裁决",
                    self.judge_name.cli_name()
                ),
                reused: false,
                record: None,
            };
            self.settle_gate(call, node, decision, now, updates);
            return Step::Idle;
        };

        let request = JudgeRequest {
            snapshot,
            state,
            questions: BTreeMap::from([(
                QuestionId::from(gate::QUESTION_ID),
                self.question.clone(),
            )]),
        };
        self.phase = Phase::Gating {
            call: call.clone(),
            node,
        };
        updates.push(Update::Status(self.running(WaitingReason::JevJudging)));
        Step::Gate(GateCall::new(
            judge,
            request,
            key,
            self.judge_name.source(),
            self.thresholds,
        ))
    }

    /// 结算一次门控：记录、上屏、决定是上卡还是直接跑（都会被 [`Agent::advance`] 接住）。
    fn settle_gate(
        &mut self,
        call: ToolCall,
        node: NodeId,
        decision: GateDecision,
        now: Instant,
        updates: &mut Vec<Update>,
    ) {
        if decision.reused {
            self.stats.judge_reuses += 1;
        } else {
            self.stats.judgments += 1;
            self.debounce.record(decision.clone(), now);
        }
        self.record_judgment_call(&call, &decision, updates);
        let session_turn = self.stats.turns;
        let round = self.rounds;
        let verdict = match decision.verdict {
            Verdict::Allow => "allow",
            Verdict::Refuse => "refuse",
            Verdict::Ask => "ask",
        };
        let (choice, probabilities) = match &decision.answer {
            Some(jh_core::Answer::Choice(choice)) => (
                Some(choice.choice.clone()),
                Some(
                    choice
                        .probabilities
                        .iter()
                        .map(|(label, probability)| (label.clone(), *probability as f32))
                        .collect(),
                ),
            ),
            _ => (None, None),
        };
        self.log(
            SessionEvent::GateDecision {
                at_unix_ms: now_unix_ms(),
                turn: session_turn,
                round,
                key: decision.key.clone(),
                verdict: verdict.to_owned(),
                source: source_tag(decision.source).to_owned(),
                mode: self.approval.mode().as_str().to_owned(),
                executor: decision.executor.clone(),
                request_id: decision.request_id.clone(),
                latency_ms: decision.latency_ms,
                reused: decision.reused,
                margin: decision.margin,
                confidence: decision.confidence,
                choice,
                probabilities,
                thresholds: GateThresholds {
                    margin: self.thresholds.margin,
                    confidence: self.thresholds.confidence,
                },
                failure: decision.failure.clone(),
                reason: decision.reason.clone(),
            },
            updates,
        );

        // 判定落进语义状态：依据有效性从这一刻起按对象版本算。
        match &decision.record {
            Some(record) => {
                if let Err(error) = self.state.record_judgment(node, record.clone()) {
                    updates.push(Update::Notice {
                        text: format!("判断记录没写进语义状态：{error}"),
                        tone: NoticeTone::Error,
                    });
                }
            }
            None => {
                let detail = decision
                    .failure
                    .clone()
                    .unwrap_or_else(|| "判定器没有返回判断记录".to_owned());
                let _ = self
                    .state
                    .set_evaluation(node, EvaluationStatus::Failed(detail));
            }
        }
        self.last_node = Some(node);

        // 上屏：阶段行 + 判定块（概率原样画出来）。
        updates.push(Update::Notice {
            text: view::decision_line(&decision),
            tone: tone_for(decision.verdict),
        });
        let prompt = format!("是否允许执行 `{}`？", command_of(&call));
        if let Some(trace) = gate::trace(&decision, prompt) {
            updates.push(Update::Decision(Box::new(trace)));
        }

        // 风险按**这次调用**算（写/编辑由工具自己报，bash 看命令），与 `start_gate` 同一个来源。
        let risk = self.tools.risk(&call);
        if decision.refused() {
            self.stats.refusals += 1;
            let text = decision.refuse_text();
            updates.push(Update::ToolBegin {
                name: call.name.clone(),
                args: self.tools.detail(&call),
            });
            updates.push(Update::ToolEnd {
                status: ToolStatus::Failed,
                detail: text.clone(),
            });
            updates.push(Update::Notice {
                text: text.clone(),
                tone: NoticeTone::Warning,
            });
            self.conversation
                .push(InputItem::function_call_output(call.call_id.clone(), text));
            self.after_call(updates);
            return;
        }

        if self.bypasses_the_card(&call.name) {
            self.begin_run(call, Some(node), updates);
            return;
        }

        // auto 模式：Jev 采纳「允许」就执行；判不了上卡；**破坏性命令一律上卡**
        // （不可逆动作不交给单一判定层，`docs/jev-research.md` §13.5 红线 1）。
        match self.approval.mode() {
            PermissionMode::AlwaysAsk => {}
            PermissionMode::NeverAsk => {
                self.begin_run(call, Some(node), updates);
                return;
            }
            PermissionMode::Auto => {
                if decision.verdict == Verdict::Allow && risk != Risk::Destructive {
                    self.begin_run(call, Some(node), updates);
                    return;
                }
                if decision.verdict == Verdict::Allow {
                    updates.push(Update::Notice {
                        text: format!(
                            "命令命中破坏性标记（{}）：即使判定允许也交人工确认",
                            risk.label()
                        ),
                        tone: NoticeTone::Warning,
                    });
                }
            }
        }

        self.open_card(call, Some(node), Some(&decision), risk, updates);
    }

    /// 记一条「没问 Jev 就放行」的门控记录。
    ///
    /// 三条短路路径（只读工具、白名单、`never` 模式）都要留账：否则日志里只有 `tool_call`、
    /// 没有 `gate_decision`，事后分不清「这次没判定」是省了判定还是漏了记录。
    fn log_gate_shortcut(
        &mut self,
        call: &ToolCall,
        risk: Risk,
        reason: String,
        updates: &mut Vec<Update>,
    ) {
        let session_turn = self.stats.turns;
        let round = self.rounds;
        let key = gate::material_key(&call.name, &call.arguments);
        let mode = self.approval.mode().as_str().to_owned();
        let thresholds = GateThresholds {
            margin: self.thresholds.margin,
            confidence: self.thresholds.confidence,
        };
        let reason = format!("{reason}（确定性层：{}）", risk.label());
        self.log(
            SessionEvent::GateDecision {
                at_unix_ms: now_unix_ms(),
                turn: session_turn,
                round,
                key,
                verdict: "allow".to_owned(),
                source: "rule".to_owned(),
                mode,
                executor: Some("确定性层".to_owned()),
                request_id: None,
                latency_ms: 0,
                reused: false,
                margin: None,
                confidence: None,
                choice: None,
                probabilities: None,
                thresholds,
                failure: None,
                reason,
            },
            updates,
        );
    }

    /// 这次调用是否可以直接执行、不必上卡。
    ///
    /// - `never`：都不上卡；
    /// - `ask`：只有白名单里的工具不上卡（「总是允许」是用户自己的授权）；
    /// - `auto`：白名单 + **只读工具**（确定性层已经判过，没有可判的副作用）。
    ///   `bash` 与写 / 编辑工具仍按判定结果走：判不了或破坏性才上卡。
    fn bypasses_the_card(&self, tool: &str) -> bool {
        if self.approval.is_preapproved(tool) {
            return true;
        }
        match self.approval.mode() {
            PermissionMode::NeverAsk => true,
            PermissionMode::Auto => self.tools.is_read_only(tool),
            PermissionMode::AlwaysAsk => false,
        }
    }

    /// 弹审批卡（唯一入口：上屏状态、正文与卡片的理由都从这里出）。
    fn open_card(
        &mut self,
        call: ToolCall,
        node: Option<NodeId>,
        decision: Option<&GateDecision>,
        risk: Risk,
        updates: &mut Vec<Update>,
    ) {
        let request = self.approval_request(&call, decision, risk);
        let session_turn = self.stats.turns;
        let round = self.rounds;
        self.log(
            SessionEvent::ApprovalRequest {
                at_unix_ms: now_unix_ms(),
                turn: session_turn,
                round,
                tool: call.name.clone(),
                command: self.tools.detail(&call),
                body: request.body.clone(),
            },
            updates,
        );
        self.approval_opened_at = Some(Instant::now());
        self.phase = Phase::Awaiting { call, node };
        updates.push(Update::Status(self.running(WaitingReason::Approval)));
        updates.push(Update::Approval(Box::new(request)));
    }

    /// 开始执行一次调用：登记行动、按前提复核、起工具块。
    fn begin_run(&mut self, call: ToolCall, node: Option<NodeId>, updates: &mut Vec<Update>) {
        self.stats.tool_calls += 1;
        let action = node.map(|node| {
            let action = ActionId::from(format!("{}#{}", call.name, self.stats.tool_calls));
            if let Err(error) = self.state.declare_action(action.clone(), [node]) {
                updates.push(Update::Notice {
                    text: format!("行动没登记进语义状态：{error}"),
                    tone: NoticeTone::Error,
                });
            }
            action
        });
        // 前提复核：依据变旧就不发出（确定性层，不是模型说了算）。
        if let Some(action) = &action
            && let Ok(PremiseCheck::Blocked { .. }) = self.state.check_action(action)
        {
            let text = format!("这次调用的判定依据已失效（{action}），请重新判定后再试");
            updates.push(Update::Notice {
                text: text.clone(),
                tone: NoticeTone::Warning,
            });
            self.conversation
                .push(InputItem::function_call_output(call.call_id.clone(), text));
            self.after_call(updates);
            return;
        }
        updates.push(Update::ToolBegin {
            name: call.name.clone(),
            args: self.tools.detail(&call),
        });
        updates.push(Update::Status(self.running(WaitingReason::ToolRunning)));
        self.tool_started = Some(Instant::now());
        self.phase = Phase::Running { call, action };
    }

    /// 一次调用收尾：队列里还有就接着办，没有就再问模型。
    fn after_call(&mut self, updates: &mut Vec<Update>) {
        self.phase = Phase::Idle;
        if self.queue.is_empty() {
            self.owes_model = true;
            updates.push(Update::Status(self.running(WaitingReason::LlmStreaming)));
        } else {
            updates.push(Update::Status(self.running(WaitingReason::ToolRunning)));
        }
    }

    /// 一轮收尾：阶段行给出用时，状态行给出结果。
    fn finish_turn(&mut self, ok: bool, updates: &mut Vec<Update>) {
        self.phase = Phase::Idle;
        self.owes_model = false;
        self.queue.clear();
        self.rounds = 0;
        if let Some(started) = self.turn_started.take() {
            updates.push(Update::PlainNotice(format!(
                "用时 {:.1}s",
                started.elapsed().as_secs_f32()
            )));
        }
        updates.push(Update::Status(StatusModel {
            phase: Some(if ok {
                TurnPhase::Done
            } else {
                TurnPhase::Failed
            }),
            waiting: None,
            model: Some(self.model_label.clone()),
            tokens: None,
        }));
    }

    /// 会话指令：角色、工作区与工具约定（通道无状态，每次调用都自带）。
    fn instructions(&self) -> String {
        format!(
            "你是 jev-harness 的会话代理，在用户的机器上替他干活。\n\
             工作区：{}（工具的相对路径都相对它）。\n\
             约定：\n\
             - 要看代码或文件就用 read_file / list_dir / search；改文件用 edit_file（锚点要唯一）\
               或 write_file（整文件覆盖）；要跑命令就用 bash。\n\
             - 每一次工具调用都要经用户授权（shell 命令还会先过一遍类型化判定）：\
               被拒绝时如实说明并换做法，不要重复同一条命令。\n\
             - 回答用中文，先说结论再给依据，简短直接。\n\
             - 没有做过的事不要说做过：工具没跑成功就说没跑成功。",
            self.workspace.display()
        )
    }

    /// 上卡用的请求：命令、确定性风险、门控理由与概率都在正文里（设计 §5.4 要求有理由）。
    fn approval_request(
        &self,
        call: &ToolCall,
        decision: Option<&GateDecision>,
        risk: Risk,
    ) -> ApprovalRequest {
        let mut body = format!("确定性层：{}", risk.label());
        match decision {
            Some(decision) => {
                body.push_str(&format!("；门控：{}", decision.reason));
                if let Some(failure) = &decision.failure {
                    body.push_str(&format!("（{failure}）"));
                }
            }
            None if risk == Risk::ReadOnly => body.push_str("（只读工具，不占用 Jev）"),
            None => body.push_str(&format!(
                "（授权模式「{}」：这次不问 Jev）",
                self.approval.mode().label()
            )),
        }
        ApprovalRequest::new(call.name.clone(), self.tools.detail(call)).with_body(body)
    }

    /// 材料与投影一次装出：装进去的对象就是登记读取的对象（否则快照校验会报漂移）。
    fn material(&self) -> (State, jh_core::InputProjection, SnapshotId) {
        let mut objects = BTreeMap::new();
        let mut reads = Vec::new();
        for object in view::objects::all() {
            let Some(version) = self.state.evidence().current_version(&object) else {
                continue;
            };
            let records: Vec<serde_json::Value> = self
                .state
                .evidence()
                .current_evidence(&object)
                .iter()
                .map(|entry| {
                    serde_json::json!({
                        "source": entry.source,
                        "at_unix_ms": entry.at_unix_ms,
                        "version": entry.version.0,
                        "content": entry.content,
                    })
                })
                .collect();
            objects.insert(
                object.as_str().to_owned(),
                serde_json::json!({"records": records}),
            );
            reads.push((object, version));
        }
        // 快照标识取材料的稳定摘要：同一份材料在记录里可核对（`ContentHash` 是非加密摘要）。
        let snapshot = SnapshotId(format!(
            "{}",
            ContentHash::of_json(&serde_json::to_value(&objects).expect("JSON 值总能序列化"))
        ));
        (
            State::Object(objects),
            jh_core::InputProjection::covering(reads),
            snapshot,
        )
    }

    /// 落一条证据（证据文件与失效传播都只有这一条路径）。
    fn append_evidence(
        &mut self,
        object: &str,
        source: &str,
        at_unix_ms: u64,
        content: serde_json::Value,
        updates: &mut Vec<Update>,
    ) {
        let object = ObjectId::from(object);
        let version = ObjectVersion(
            self.state
                .evidence()
                .current_version(&object)
                .map_or(1, |version| version.0 + 1),
        );
        match self.state.append_evidence(NewEvidence {
            source: source.to_owned(),
            at_unix_ms,
            object,
            version,
            content,
        }) {
            Ok((_, invalidation)) => self.last_invalidation = Some(invalidation),
            Err(error) => updates.push(Update::Notice {
                text: format!("证据没落上：{error}"),
                tone: NoticeTone::Error,
            }),
        }
    }

    /// 记一次成功的模型调用。
    fn record_model_call(
        &mut self,
        turn: &ModelTurn,
        error: Option<String>,
        updates: &mut Vec<Update>,
    ) {
        let entry = CallEntry::ModelCall {
            run: self.run.clone(),
            purpose: "session".to_owned(),
            case: format!("model-{}", self.stats.model_calls),
            source: JudgeSource::Llm,
            version: Some(turn.model.clone()),
            request_id: turn.request_id.clone(),
            usage: turn.usage.clone(),
            input_bytes: self.last_request_bytes,
            output_bytes: turn.text.len() as u64,
            latency_ms: turn.latency_ms,
            error,
        };
        self.append_call(&entry, updates);
    }

    /// 记一次失败的模型调用：没有响应可记，只记失败本身。
    fn record_model_failure(&mut self, error: &Error, updates: &mut Vec<Update>) {
        let entry = CallEntry::ModelCall {
            run: self.run.clone(),
            purpose: "session".to_owned(),
            case: format!("model-{}", self.stats.model_calls),
            source: JudgeSource::Llm,
            version: None,
            request_id: None,
            usage: None,
            input_bytes: self.last_request_bytes,
            output_bytes: 0,
            latency_ms: 0,
            error: Some(error.to_string()),
        };
        self.append_call(&entry, updates);
    }

    /// 记一次门控判定（成功落判断记录，失败落失败记录）。
    fn record_judgment_call(
        &mut self,
        call: &ToolCall,
        decision: &GateDecision,
        updates: &mut Vec<Update>,
    ) {
        let case = format!("{}#{}", call.name, self.stats.judgments.max(1));
        let entry = match &decision.record {
            Some(record) => CallEntry::Judgment {
                run: self.run.clone(),
                case,
                record: Box::new(record.clone()),
            },
            None => CallEntry::Failure {
                run: self.run.clone(),
                case,
                source: decision.source,
                latency_ms: decision.latency_ms,
                error: decision
                    .failure
                    .clone()
                    .unwrap_or_else(|| "判定器没有返回答案".to_owned()),
            },
        };
        self.append_call(&entry, updates);
    }

    /// 写调用记录；写不进去也不打断会话，但绝不静默。
    fn append_call(&mut self, entry: &CallEntry, updates: &mut Vec<Update>) {
        if let Err(error) = self.calls.append(entry) {
            updates.push(Update::Notice {
                text: format!("调用记录没写上：{error}"),
                tone: NoticeTone::Warning,
            });
        }
    }

    fn running(&self, reason: WaitingReason) -> StatusModel {
        StatusModel::running(reason).with_model(self.model_label.clone())
    }

    fn judge_label(&self) -> String {
        match &self.judge {
            Some(_) => self.judge_name.cli_name().to_owned(),
            None => format!("{}（不可用）", self.judge_name.cli_name()),
        }
    }
}

/// 裁决语气：拒绝用警示色，其余用信息色。
fn tone_for(verdict: Verdict) -> NoticeTone {
    match verdict {
        Verdict::Refuse => NoticeTone::Warning,
        _ => NoticeTone::Info,
    }
}

/// 工具状态的上屏词。
fn status_word(status: ToolStatus) -> &'static str {
    match status {
        ToolStatus::Running => "执行中",
        ToolStatus::Ok => "成功",
        ToolStatus::Failed => "失败",
        ToolStatus::Restored => "接回",
    }
}

/// shell 调用的命令原文；参数不是 JSON 时退回参数原文（展示与防抖键用）。
fn command_of(call: &ToolCall) -> String {
    #[derive(serde::Deserialize)]
    struct Command {
        #[serde(default)]
        command: Option<String>,
    }
    serde_json::from_str::<Command>(&call.arguments)
        .ok()
        .and_then(|args| args.command)
        .unwrap_or_else(|| call.arguments.clone())
}

/// 现在时刻（Unix 毫秒）：证据的可读时间与用户块的时间戳都用它。
/// `JudgeSource` 的稳定词表（日志契约，见 `docs/session-log.md` §3）。
///
/// 不用 `Debug` 直接转小写：那样 `RealApi` 会变成 `realapi`，与文档和既有报告里的 `real_api` 不一致。
fn source_tag(source: JudgeSource) -> &'static str {
    match source {
        JudgeSource::RealApi => "real_api",
        JudgeSource::Llm => "llm",
        JudgeSource::Rule => "rule",
        JudgeSource::Replay => "replay",
        JudgeSource::SimulatedTruth => "simulated_truth",
    }
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// 按字符数截断一行摘要。
fn clip(text: &str, max: usize) -> String {
    let head: String = text.chars().take(max).collect();
    if text.chars().count() > max {
        format!("{head}…")
    } else {
        head
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Instant;

    use jh_core::{Judge, SemanticState};
    use jh_tui::app::{ApprovalOutcome, CommandCall};

    use super::*;
    use crate::calls::read_entries;
    use crate::session::test_support::{
        Scripted, TempDir, call, choice_judge, failing_judge, turn,
    };

    /// 建一个工作区里放好样例文件的会话。
    fn session_agent(
        judge: Option<Arc<dyn Judge>>,
        model: Option<Arc<dyn Model>>,
    ) -> (Agent, TempDir) {
        session_agent_with_max_rounds(judge, model, 4)
    }

    /// 同上，但本轮模型调用上限自己给（上限相关的用例要卡到很小的数）。
    fn session_agent_with_max_rounds(
        judge: Option<Arc<dyn Judge>>,
        model: Option<Arc<dyn Model>>,
        max_rounds: usize,
    ) -> (Agent, TempDir) {
        session_agent_full(judge, model, max_rounds, None, "test-run")
    }

    /// 同上，另外挂上会话历史仓库（`/sessions` 与 `/resume` 的用例要它）。
    fn session_agent_full(
        judge: Option<Arc<dyn Judge>>,
        model: Option<Arc<dyn Model>>,
        max_rounds: usize,
        store: Option<SessionStore>,
        session_id: &str,
    ) -> (Agent, TempDir) {
        let dir = TempDir::new("session-agent");
        let workspace = dir.join("workspace");
        std::fs::create_dir_all(&workspace).expect("建工作区");
        std::fs::write(workspace.join("notes.txt"), "端口 8080 被占用\n").expect("写文件");
        let calls = CallLog::open(dir.join("calls.jsonl")).expect("调用记录");
        let model_label = match &model {
            Some(_) => "stub-model".to_owned(),
            // 与真实装配同一口径：模型没接上时标识就写「未配置」。
            None => "模型未配置".to_owned(),
        };
        let agent = Agent::new(AgentConfig {
            workspace,
            judge_name: JudgeName::Jev,
            judge,
            model,
            model_label,
            run: "test-run".to_owned(),
            state: SemanticState::in_memory(),
            calls,
            debounce: Duration::from_secs(30),
            max_rounds,
            // 给了会话仓库就顺手给上会话目录：于是「第一次提交才落盘」在测试里也被覆盖。
            session_dir: store.as_ref().map(|store| store.paths(session_id).dir),
            session_start: None,
            store,
            session_id: session_id.to_owned(),
        })
        .expect("建会话");
        (agent, dir)
    }

    /// 把会话切到「每次都问」：卡片路径的用例不再依赖「判定是否够自信」。
    fn force_ask_mode(agent: &mut Agent) {
        agent
            .approval_mut_for_test()
            .set_mode(PermissionMode::AlwaysAsk);
    }

    /// 跑到「弹卡」为止（不代替用户裁决），返回这一路产生的更新。
    fn run_until_card(agent: &mut Agent, steps: usize) -> Vec<Update> {
        let now = Instant::now();
        let mut all = Vec::new();
        for _ in 0..steps {
            if agent.awaiting_approval() {
                break;
            }
            let (step, updates) = agent.advance(now);
            all.extend(updates);
            match step {
                Step::Model(request) => {
                    let (future, _deltas) = agent.model_run(&request);
                    let result = crate::session::test_support::block_on(future);
                    all.extend(agent.on_model_result(result));
                }
                Step::Gate(call) => {
                    let decision = crate::session::test_support::block_on(call.ask());
                    all.extend(agent.on_gate_result(decision, now));
                }
                _ => {}
            }
        }
        all
    }

    /// 把一轮跑到收尾：模型按脚本、门控按桩、工具真跑、审批用给定裁决。
    fn run_turn(agent: &mut Agent, approval: ApprovalOutcome, now: Instant) -> Vec<Update> {
        run_steps(agent, approval, now, 24)
    }

    /// 同上，但步数上限自己给（要把模型调用次数顶过缺省上限的用例得跑很多步）。
    fn run_steps(
        agent: &mut Agent,
        approval: ApprovalOutcome,
        now: Instant,
        steps: usize,
    ) -> Vec<Update> {
        let mut all = Vec::new();
        for _ in 0..steps {
            if agent.awaiting_approval() {
                all.extend(agent.on_approval(approval));
                continue;
            }
            let (step, updates) = agent.advance(now);
            all.extend(updates);
            match step {
                // 空闲**且**没有待办：这一轮真的收尾了（`Step::Idle` 也可能只是把活
                // 排在了卡片或工具后面）。
                Step::Idle if !agent.busy() => break,
                Step::Idle => {}
                Step::Model(request) => {
                    let (future, _deltas) = agent.model_run(&request);
                    let result = crate::session::test_support::block_on(future);
                    all.extend(agent.on_model_result(result));
                }
                Step::Gate(call) => {
                    let decision = crate::session::test_support::block_on(call.ask());
                    all.extend(agent.on_gate_result(decision, now));
                }
                Step::Tool(call) => {
                    let tools = agent.tools();
                    let outcome = crate::session::test_support::block_on(tools.execute(&call));
                    all.extend(agent.on_tool_result(outcome));
                }
            }
        }
        all
    }

    fn notices(updates: &[Update]) -> String {
        updates
            .iter()
            .filter_map(|update| match update {
                Update::Notice { text, .. } | Update::PlainNotice(text) => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// 测试专用：直接往白名单里记一笔（正常路径是用户按 `a`）。
    trait ApprovalMutForTest {
        fn approval_mut_for_test(&mut self) -> &mut ApprovalPolicy;
    }
    impl ApprovalMutForTest for Agent {
        fn approval_mut_for_test(&mut self) -> &mut ApprovalPolicy {
            &mut self.approval
        }
    }

    /// **判定不可用时，人工批准必须真的执行。**
    ///
    /// 真机踩到：判定器判不了（`Ask`）→ 弹卡 → 按「允许一次」→ 判定节点是 `Failed`、按定义
    /// 不可依赖 → 前提复核挡下 → 界面「批准了却不执行」，模型反复重发同一条命令。
    #[test]
    fn an_approved_call_runs_even_when_the_judge_could_not_decide() {
        let model = Scripted::new(vec![
            turn(
                "",
                vec![call("bash", serde_json::json!({"command": "ls -1"}))],
            ),
            turn("跑完了", Vec::new()),
        ]);
        // 判定器说「判不了」（桩判定器没有这个问题的覆盖）——走 Ask → 上卡。
        let (mut agent, _dir) = session_agent(Some(failing_judge("规则表没有覆盖")), Some(model));
        let mut updates = agent.submit("跑一下".to_owned());
        updates.extend(run_turn(
            &mut agent,
            ApprovalOutcome::AllowOnce,
            Instant::now(),
        ));

        let text = notices(&updates);
        assert!(
            text.contains("人工裁决"),
            "要说明这次按人工裁决执行：{text}"
        );
        assert!(
            updates
                .iter()
                .any(|update| matches!(update, Update::ToolEnd { .. })),
            "批准之后工具必须真的跑：{text}"
        );
        assert_eq!(agent.stats().tool_calls, 1, "执行过一次");
        assert!(
            !text.contains("判定依据已失效"),
            "不能再被前提复核挡下：{text}"
        );
    }

    /// `auto`（缺省）：Jev 明确采纳「允许」就直接执行，不再上卡。
    #[test]
    fn auto_runs_a_confident_allow_without_the_card() {
        let model = Scripted::new(vec![
            turn(
                "",
                vec![call("bash", serde_json::json!({"command": "ls -1"}))],
            ),
            turn("看到了", Vec::new()),
        ]);
        let (mut agent, _dir) = session_agent(Some(choice_judge(0.95, 0.05, 0.9)), Some(model));
        let now = Instant::now();
        let mut updates = agent.submit("看一下工作区".to_owned());
        updates.extend(run_turn(&mut agent, ApprovalOutcome::AllowOnce, now));
        assert!(
            !has_approval(&updates),
            "auto 模式下 Jev 采纳即执行，不该弹卡：{:?}",
            notices(&updates)
        );
        assert!(
            updates
                .iter()
                .any(|update| matches!(update, Update::ToolEnd { .. })),
            "工具照旧要跑：{:?}",
            notices(&updates)
        );
        assert_eq!(agent.stats().tool_calls, 1, "确实执行了一次");
    }

    /// `auto`：判定不确定（低于采纳门槛）时仍然上卡。
    #[test]
    fn auto_asks_when_the_judge_is_not_sure() {
        let model = Scripted::new(vec![turn(
            "",
            vec![call("bash", serde_json::json!({"command": "ls -1"}))],
        )]);
        // 概率接近 + confidence 低 → 判不了，交人工
        let (mut agent, _dir) = session_agent(Some(choice_judge(0.55, 0.45, 0.2)), Some(model));
        agent.submit("跑一下".to_owned());
        let updates = run_until_card(&mut agent, 8);
        assert!(
            agent.awaiting_approval(),
            "判不了就该上卡：{:?}",
            notices(&updates)
        );
    }

    /// `auto`：破坏性命令即使 Jev 采纳「允许」也上卡（不可逆动作不交给单一判定层）。
    #[test]
    fn auto_still_asks_for_a_destructive_command() {
        let model = Scripted::new(vec![turn(
            "",
            vec![call("bash", serde_json::json!({"command": "rm -rf build"}))],
        )]);
        let (mut agent, _dir) = session_agent(Some(choice_judge(0.95, 0.05, 0.9)), Some(model));
        agent.submit("清一下".to_owned());
        let updates = run_until_card(&mut agent, 8);
        assert!(agent.awaiting_approval(), "破坏性命令要人工确认");
        assert!(
            notices(&updates).contains("破坏性"),
            "要写明为什么仍然上卡：{:?}",
            notices(&updates)
        );
    }

    /// `ask`：不问 Jev，每次都上卡；`never`：不问 Jev、不上卡，直接执行。
    #[test]
    fn ask_and_never_modes_change_the_gate_path() {
        // ask 模式
        let model = Scripted::new(vec![
            turn(
                "",
                vec![call("bash", serde_json::json!({"command": "ls -1"}))],
            ),
            turn("看到了", Vec::new()),
        ]);
        let (mut agent, _dir) = session_agent(Some(choice_judge(0.95, 0.05, 0.9)), Some(model));
        let updates = agent.on_command(CommandCall {
            name: "permission",
            args: "ask".to_owned(),
        });
        assert!(
            notices(&updates).contains("授权模式已换成 `ask`"),
            "{:?}",
            notices(&updates)
        );
        assert_eq!(agent.approval().mode(), PermissionMode::AlwaysAsk);

        agent.submit("跑一下".to_owned());
        let updates = run_until_card(&mut agent, 8);
        assert!(agent.awaiting_approval(), "ask 模式一定上卡");
        assert!(
            notices(&updates).contains("不问 Jev"),
            "要说明这一步没花判定：{:?}",
            notices(&updates)
        );
        // 批准后这一轮收尾
        let now = Instant::now();
        let _ = run_turn(&mut agent, ApprovalOutcome::AllowOnce, now);

        // never 模式
        let model = Scripted::new(vec![
            turn(
                "",
                vec![call("bash", serde_json::json!({"command": "ls -1"}))],
            ),
            turn("看到了", Vec::new()),
        ]);
        let (mut agent, _dir) = session_agent(Some(choice_judge(0.95, 0.05, 0.9)), Some(model));
        agent.on_command(CommandCall {
            name: "permission",
            args: "never".to_owned(),
        });
        let now = Instant::now();
        let mut updates = agent.submit("跑一下".to_owned());
        updates.extend(run_turn(&mut agent, ApprovalOutcome::AllowOnce, now));
        assert!(
            !has_approval(&updates),
            "never 模式不上卡：{:?}",
            notices(&updates)
        );
        assert!(
            notices(&updates).contains("ls -1"),
            "{:?}",
            notices(&updates)
        );
    }

    /// `/permission` 的表态口径：看现状、换模式、reset、认不出的参数。
    #[test]
    fn the_permission_command_reports_and_switches() {
        let (mut agent, _dir) = session_agent(None, None);
        let ask = |agent: &mut Agent, args: &str| {
            notices(&agent.on_command(CommandCall {
                name: "permission",
                args: args.to_owned(),
            }))
        };
        assert!(ask(&mut agent, "").contains("自动（Jev 判定）"));
        // 无参数时给固定选项：三种模式 + reset，且标出当前是哪一个
        let updates = agent.on_command(CommandCall {
            name: "permission",
            args: String::new(),
        });
        let (command, entries) = choices(&updates).expect("要给出选项列表");
        assert_eq!(command, "permission");
        assert_eq!(
            entries
                .iter()
                .map(|(value, _)| value.as_str())
                .collect::<Vec<_>>(),
            vec!["auto", "ask", "never", "reset"]
        );
        assert!(entries[0].1.contains("当前"), "{entries:?}");
        assert!(entries[3].1.contains("白名单"), "{entries:?}");

        assert!(ask(&mut agent, "never").contains("`never`"));
        assert_eq!(agent.approval().mode(), PermissionMode::NeverAsk);
        assert!(ask(&mut agent, "").contains("从不询问"));
        // 切到 never 之后，「当前」标记跟着走
        let updates = agent.on_command(CommandCall {
            name: "permission",
            args: String::new(),
        });
        let (_, entries) = choices(&updates).expect("选项列表");
        assert!(entries[2].1.contains("当前"), "{entries:?}");
        agent
            .approval_mut_for_test()
            .record("bash", ApprovalOutcome::AllowAlways);
        assert!(ask(&mut agent, "").contains("白名单 bash"));
        assert!(ask(&mut agent, "reset").contains("已恢复缺省"));
        assert_eq!(agent.approval().mode(), PermissionMode::Auto);
        assert!(!agent.approval().is_preapproved("bash"));
        assert!(ask(&mut agent, "sometimes").contains("不认识的授权模式"));
    }

    /// 写文件与编辑文件：都走门控，不再是「只读」快路径。
    /// 新文件在 auto 下判定过门槛即执行；**覆盖已有文件算破坏性**，仍要人工确认。
    #[test]
    fn file_tools_go_through_the_gate() {
        // 新文件：判定过门槛 → 直接写
        let model = Scripted::new(vec![
            turn(
                "",
                vec![call(
                    "write_file",
                    serde_json::json!({"path": "new.txt", "content": "你好\n"}),
                )],
            ),
            turn("写好了", Vec::new()),
        ]);
        let (mut agent, dir) = session_agent(Some(choice_judge(0.95, 0.05, 0.9)), Some(model));
        let mut updates = agent.submit("建个文件".to_owned());
        updates.extend(run_turn(
            &mut agent,
            ApprovalOutcome::AllowOnce,
            Instant::now(),
        ));
        assert!(
            !has_approval(&updates),
            "auto 下新文件采纳即执行：{:?}",
            notices(&updates)
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("workspace/new.txt")).expect("文件写出来了"),
            "你好\n"
        );

        // 覆盖已有文件：确定性层算破坏性 → 即使判定允许也上卡
        std::fs::write(dir.join("workspace/exists.txt"), "旧的\n").expect("铺一个已有文件");
        let model = Scripted::new(vec![turn(
            "",
            vec![call(
                "write_file",
                serde_json::json!({"path": "exists.txt", "content": "新的\n"}),
            )],
        )]);
        let (mut agent, dir2) = session_agent(Some(choice_judge(0.95, 0.05, 0.9)), Some(model));
        std::fs::write(dir2.join("workspace/exists.txt"), "旧的\n").expect("铺一个已有文件");
        agent.submit("改一下".to_owned());
        let updates = run_until_card(&mut agent, 8);
        assert!(
            agent.awaiting_approval(),
            "覆盖已有文件要人工确认：{:?}",
            notices(&updates)
        );
        assert!(
            notices(&updates).contains("破坏性"),
            "{:?}",
            notices(&updates)
        );

        // edit_file：改动工作区 → 也走门控；auto 下过门槛即执行
        let model = Scripted::new(vec![
            turn(
                "",
                vec![call(
                    "edit_file",
                    serde_json::json!({"path": "edit.txt", "old_string": "旧", "new_string": "新"}),
                )],
            ),
            turn("改好了", Vec::new()),
        ]);
        let (mut agent, dir3) = session_agent(Some(choice_judge(0.95, 0.05, 0.9)), Some(model));
        std::fs::write(dir3.join("workspace/edit.txt"), "旧内容\n").expect("铺文件");
        let mut updates = agent.submit("改一处".to_owned());
        updates.extend(run_turn(
            &mut agent,
            ApprovalOutcome::AllowOnce,
            Instant::now(),
        ));
        assert!(!has_approval(&updates), "{:?}", notices(&updates));
        assert_eq!(
            std::fs::read_to_string(dir3.join("workspace/edit.txt")).expect("读回"),
            "新内容\n"
        );
    }

    /// **开着不用的会话不在磁盘上留痕**：第一次提交才建目录与文件。
    ///
    /// 真调里踩到：每次启动 `jev chat` 都会在 `~/.jev-harness/sessions/` 留下一个空会话，
    /// 攒了一堆 `turns=0` 的目录。
    #[test]
    fn a_session_leaves_nothing_on_disk_until_the_first_submit() {
        let dir = TempDir::new("lazy-session");
        let store = SessionStore::at(dir.join("home"));
        let (mut agent, _dir) = session_agent_full(None, None, 4, Some(store.clone()), "lazy-1");
        let session_dir = store.paths("lazy-1").dir.clone();
        assert!(!session_dir.exists(), "建好会话（= 开着不用）不该建目录");

        // 只读命令同样不该造会话：它没有对话内容可留。
        agent.on_command(CommandCall {
            name: "status",
            args: String::new(),
        });
        assert!(!session_dir.exists(), "只读命令不该造会话目录");

        // 第一次提交：目录、两份逐行文件与开场摘要都到位（异常结束也能 `/resume`）。
        agent.submit("第一句".to_owned());
        assert!(agent.session_started(), "提交之后会话才算开始");
        assert!(
            session_dir.join("session.jsonl").is_file(),
            "会话日志要开出来"
        );
        assert!(
            session_dir.join("conversation.jsonl").is_file(),
            "对话记录要开出来"
        );
        assert!(
            store.read_meta("lazy-1").is_ok(),
            "开场摘要要写，供 `/resume` 用"
        );
        let log = std::fs::read_to_string(session_dir.join("session.jsonl")).expect("读日志");
        assert!(log.contains("user_message"), "第一条事件是用户消息：{log}");
    }

    /// `/jev`：手工问一次判定器，读数上屏 + 落一条 `manual_judgment`。
    #[tokio::test]
    async fn a_manual_judgment_prints_the_reading_and_logs_it() {
        // 带会话目录：这条用例要验的正是「读数落进日志」。
        let dir = TempDir::new("manual-judgment");
        let store = SessionStore::at(dir.join("home"));
        let (mut agent, _dir) = session_agent_full(
            Some(choice_judge(0.9, 0.1, 0.95)),
            None,
            4,
            Some(store),
            "manual-1",
        );

        let (request, question) = agent
            .manual_judgment_request("这条命令该不该跑")
            .expect("装请求");
        assert_eq!(
            question.criteria.keys().cloned().collect::<Vec<_>>(),
            vec!["allow".to_owned(), "deny".to_owned()],
            "不写 --label 时用门控那一对"
        );
        let judge = agent.judge_handle().expect("会话有判定器");
        let response = judge.judge(&request).await.expect("桩判定器不会失败");

        let mut updates = Vec::new();
        agent.on_manual_judgment(Ok(response), 12, question, &mut updates);
        let text: Vec<String> = updates
            .iter()
            .filter_map(|update| match update {
                Update::Notice { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        let joined = text.join("\n");
        // 桩判定器照它自己的选项名作答（`usr:allow`）：这里只断言读数该有的形状。
        assert!(joined.contains("答案："), "{joined}");
        assert!(joined.contains("margin"), "{joined}");
        assert!(joined.contains("概率："), "{joined}");
        assert!(joined.contains("不采纳"), "口径要写在读数里：{joined}");
        assert!(joined.contains("判定器："), "{joined}");

        // 读数要落进会话日志（`manual_judgment`）：它不做任何其它事，留档是它唯一的意义。
        assert!(agent.session_started(), "/jev 也要让会话落盘");
        let logged = agent
            .store()
            .expect("测试里有会话仓库")
            .paths(&agent.meta().id)
            .log;
        let text = std::fs::read_to_string(&logged).expect("读日志");
        assert!(
            text.contains("manual_judgment"),
            "日志里要有手工判定：{text}"
        );
    }

    /// 判定器不可用时 `/jev` 如实报错，并把它落进同一条日志（`error` 有值）。
    #[tokio::test]
    async fn a_failed_manual_judgment_is_reported_and_logged_as_an_error() {
        let dir = TempDir::new("manual-judgment-failed");
        let store = SessionStore::at(dir.join("home"));
        let (mut agent, _dir) = session_agent_full(None, None, 4, Some(store), "manual-2");
        let (_request, question) = agent.manual_judgment_request("随便问问").expect("装请求");
        let mut updates = Vec::new();
        agent.on_manual_judgment(
            Err(jh_core::Error::Unavailable("没有凭据".to_owned())),
            0,
            question,
            &mut updates,
        );
        let joined = updates
            .iter()
            .filter_map(|update| match update {
                Update::Notice { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("没问成"), "{joined}");
        assert!(joined.contains("没有凭据"), "{joined}");
    }

    /// 一轮带工具调用的对话结束后，**助手正文与工具调用都要落进 `conversation.jsonl`**。
    ///
    /// 真调里踩过：助手正文只 push 进了内存，接回来的历史里只剩用户消息。
    #[test]
    fn a_tool_turn_persists_the_assistant_text_and_the_call() {
        let dir = TempDir::new("persist");
        let store = SessionStore::at(dir.join("home"));
        store
            .start("run-p", "/tmp/ws", "deepseek-flash", "jev")
            .expect("建会话");
        let model = Scripted::new(vec![
            turn(
                "我先看一眼",
                vec![call("bash", serde_json::json!({"command": "ls -1"}))],
            ),
            turn("看完了", Vec::new()),
        ]);
        let (mut agent, _dir) = session_agent_full(
            Some(choice_judge(0.95, 0.05, 0.9)),
            Some(model),
            4,
            Some(store.clone()),
            "run-p",
        );
        agent.submit("看看工作区".to_owned());
        run_turn(&mut agent, ApprovalOutcome::AllowOnce, Instant::now());

        let items = store.read_conversation("run-p").expect("读回对话记录");
        let assistant: Vec<&str> = items
            .iter()
            .filter_map(|item| match item {
                jh_openai::InputItem::Message {
                    role: jh_openai::Role::Assistant,
                    content,
                } => Some(content.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            assistant.contains(&"我先看一眼"),
            "工具轮里的助手正文：{assistant:?}"
        );
        assert!(
            assistant.contains(&"看完了"),
            "收尾的助手正文：{assistant:?}"
        );
        assert!(
            items.iter().any(|item| matches!(
                item,
                jh_openai::InputItem::FunctionCall { name, .. } if name == "bash"
            )),
            "工具调用要落盘"
        );
        assert!(
            items
                .iter()
                .any(|item| matches!(item, jh_openai::InputItem::FunctionCallOutput { .. })),
            "工具输出要落盘"
        );
    }

    /// `/resume`：把历史会话的对话接回来，界面按顺序回放，模型下一次请求带上历史。
    #[test]
    fn resume_restores_the_conversation_and_replays_it() {
        let dir = TempDir::new("resume");
        let store = SessionStore::at(dir.join("home"));
        let paths = store
            .start("run-old", "/tmp/ws", "deepseek-flash", "jev")
            .expect("建历史会话");
        let mut log = ConversationLog::open(&paths.conversation).expect("对话记录");
        log.append(&jh_openai::InputItem::user_text("上次问的问题"))
            .expect("写历史");
        log.append(&jh_openai::InputItem::assistant_text("上次的答复"))
            .expect("写历史");
        log.append(&jh_openai::InputItem::function_call(
            "bash",
            "call-old".to_owned(),
            serde_json::json!({"command": "ls -1"}).to_string(),
        ))
        .expect("写历史");
        log.append(&jh_openai::InputItem::function_call_output(
            "call-old".to_owned(),
            "退出码 0\na.rs\n".to_owned(),
        ))
        .expect("写历史");
        log.append(&jh_openai::InputItem::assistant_text("上次看完了"))
            .expect("写历史");
        let mut meta = store.read_meta("run-old").expect("读摘要");
        meta.turns = 1;
        meta.first_message = "上次问的问题".to_owned();
        store.write_meta(&meta).expect("写摘要");

        let (mut agent, _dir) = session_agent_full(
            None,
            Some(Scripted::new(vec![turn("这次的回话", Vec::new())])),
            4,
            Some(store),
            "run-new",
        );
        let updates = agent.on_command(CommandCall {
            name: "resume",
            args: "run-old".to_owned(),
        });
        let text = notices(&updates);
        assert!(text.contains("已接回会话 run-old"), "{text}");
        assert!(
            updates.iter().any(
                |update| matches!(update, Update::User { text, .. } if text == "上次问的问题")
            ),
            "用户消息要回放到界面"
        );
        assert!(
            updates
                .iter()
                .any(|update| matches!(update, Update::AssistantEnd(text) if text == "上次的答复")),
            "助手消息要回放到界面"
        );
        // 工具调用与输出也要回放：行首按「接回」画（记录里没有当时的状态）。
        assert!(
            updates.iter().any(|update| matches!(
                update,
                Update::ToolBegin { name, args } if name == "bash" && args == "ls -1"
            )),
            "工具调用要回放：{updates:?}"
        );
        assert!(
            updates.iter().any(|update| matches!(
                update,
                Update::ToolEnd { status: ToolStatus::Restored, detail } if detail.contains("退出码 0")
            )),
            "工具输出要回放，状态标「接回」：{updates:?}"
        );
        assert!(
            updates
                .iter()
                .any(|update| matches!(update, Update::AssistantEnd(text) if text == "上次看完了")),
            "工具轮之后的助手正文也要回放"
        );

        // 接回来的上下文要真的进下一次模型请求。
        agent.submit("接着聊".to_owned());
        let (step, _) = agent.advance(Instant::now());
        let Step::Model(request) = step else {
            panic!("恢复后提交应当起模型调用");
        };
        assert!(
            request.input.iter().any(|item| matches!(
                item,
                jh_openai::InputItem::Message { content, .. } if content == "上次问的问题"
            )),
            "模型请求要带上历史对话"
        );
    }

    /// `/sessions`：没有历史时说清楚，有历史时列出来。
    #[test]
    fn the_sessions_command_lists_history() {
        let (mut agent, _dir) = session_agent(None, None);
        let text = notices(&agent.on_command(CommandCall {
            name: "sessions",
            args: String::new(),
        }));
        assert!(text.contains("会话历史不可用"), "没挂仓库时如实说：{text}");

        let dir = TempDir::new("sessions-list");
        let store = SessionStore::at(dir.join("home"));
        store
            .start("run-a", "/tmp/ws", "deepseek-flash", "jev")
            .expect("建会话");
        let mut meta = store.read_meta("run-a").expect("读摘要");
        meta.turns = 2;
        meta.first_message = "第一句".to_owned();
        store.write_meta(&meta).expect("写摘要");

        let (mut agent, _dir) = session_agent_full(None, None, 4, Some(store), "run-b");
        let updates = agent.on_command(CommandCall {
            name: "sessions",
            args: String::new(),
        });
        let text = notices(&updates);
        assert!(text.contains("run-a"), "{text}");
        // 列表形式的固定选项：命令名 resume + 会话 ID 作为值
        let (command, entries) = choices(&updates).expect("要给出选项列表");
        assert_eq!(command, "resume");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, "run-a");
        assert!(entries[0].1.contains("2 轮"), "{entries:?}");
        assert!(text.contains("/resume <会话ID>"), "{text}");
    }

    /// 从更新里取选项列表（命令名 + 每行的「值 → 说明」）。
    fn choices(updates: &[Update]) -> Option<(&'static str, Vec<(String, String)>)> {
        updates.iter().find_map(|update| match update {
            Update::Choices {
                command, entries, ..
            } => Some((
                *command,
                entries
                    .iter()
                    .map(|entry| (entry.value.clone(), entry.label.clone()))
                    .collect(),
            )),
            _ => None,
        })
    }

    fn has_approval(updates: &[Update]) -> bool {
        updates
            .iter()
            .any(|update| matches!(update, Update::Approval(_)))
    }

    /// `/status` 与 `/permission` 不需要网络：直接读会话状态与审批策略。
    #[test]
    fn session_commands_report_state_without_touching_the_model() {
        let (mut agent, _dir) = session_agent(None, None);

        let updates = agent.on_command(CommandCall {
            name: "status",
            args: String::new(),
        });
        let text = notices(&updates);
        assert!(text.contains("会话读数"), "{text}");
        assert!(text.contains("0 轮"), "{text}");

        // 白名单：先「总是允许」再问，再 reset。
        agent
            .approval_mut_for_test()
            .record("bash", ApprovalOutcome::AllowAlways);
        let updates = agent.on_command(CommandCall {
            name: "permission",
            args: String::new(),
        });
        assert!(
            notices(&updates).contains("白名单 bash"),
            "{:?}",
            notices(&updates)
        );

        let updates = agent.on_command(CommandCall {
            name: "permission",
            args: "reset".to_owned(),
        });
        assert!(
            notices(&updates).contains("白名单清空"),
            "{:?}",
            notices(&updates)
        );
        assert!(agent.prompt_info().contains("自动（Jev 判定）"));
    }

    /// `/model` 不带参数只报现状；带参数要真的换通道——测试环境没有凭据时如实报错，
    /// 不虚构一个换成功的结果。
    #[test]
    fn the_model_command_reports_or_switches_honestly() {
        let (mut agent, _dir) = session_agent(None, None);

        let updates = agent.on_command(CommandCall {
            name: "model",
            args: String::new(),
        });
        let text = notices(&updates);
        assert!(text.contains("当前模型"), "{text}");
        assert!(text.contains("用法：/model <模型ID>"), "{text}");
        // 无参数时给固定选项：官方模型名都在，且「当前」标记只出现在当前那个上
        let (command, entries) = choices(&updates).expect("要给出选项列表");
        assert_eq!(command, "model");
        assert!(
            entries.iter().any(|(value, _)| value == "deepseek-v4-pro"),
            "{entries:?}"
        );
        assert!(
            !entries.iter().any(|(_, label)| label.contains("当前")),
            "模型没配上时不该标「当前」：{entries:?}"
        );

        // 配上模型的会话：当前模型进列表并标「当前」（与进程有没有凭据无关，只看会话自己的状态）。
        let (mut agent, _dir) = session_agent(
            Some(failing_judge("不该被调用")),
            Some(Scripted::new(Vec::new())),
        );
        let updates = agent.on_command(CommandCall {
            name: "model",
            args: String::new(),
        });
        let (_, entries) = choices(&updates).expect("选项列表");
        assert!(
            entries
                .iter()
                .any(|(value, label)| value == "stub-model" && label.contains("当前")),
            "当前模型要进列表并标出来：{entries:?}"
        );

        // 没有 provider 凭据时必须显式失败并保留原模型标识。
        if crate::provider::from_env().is_err() {
            let before = agent.prompt_info();
            let updates = agent.on_command(CommandCall {
                name: "model",
                args: "some-other-model".to_owned(),
            });
            assert!(
                notices(&updates).contains("切换模型失败"),
                "{:?}",
                notices(&updates)
            );
            assert_eq!(agent.prompt_info(), before, "失败不改模型标识");
        }
    }

    #[test]
    fn a_full_turn_runs_model_gate_approval_then_tool() {
        let model = Scripted::new(vec![
            turn(
                "先落一个文件。",
                vec![call(
                    "bash",
                    serde_json::json!({"command": "touch out.txt"}),
                )],
            ),
            turn("工作区是干净的。", Vec::new()),
        ]);
        let (mut agent, dir) = session_agent(Some(choice_judge(0.9, 0.1, 0.8)), Some(model));
        let now = Instant::now();

        let mut updates = agent.submit("帮我看看工作区".to_owned());
        updates.extend(run_turn(&mut agent, ApprovalOutcome::AllowOnce, now));

        // 界面：用户块、判定轨迹、审批卡、工具块、收尾阶段行。
        assert!(matches!(updates[0], Update::User { .. }));
        assert!(
            updates
                .iter()
                .any(|update| matches!(update, Update::Decision(trace) if trace.chosen == Some(0))),
            "判定轨迹要标出被采纳的选项"
        );
        assert!(
            !has_approval(&updates),
            "auto 模式下判定过门槛就直接执行：{:?}",
            notices(&updates)
        );
        assert!(
            updates.iter().any(|u| matches!(u, Update::ToolBegin { name, args } if name == "bash" && args.contains("touch out.txt"))),
            "{:?}",
            notices(&updates)
        );
        assert!(
            updates
                .iter()
                .any(|u| matches!(u, Update::ToolEnd { status, .. } if *status == ToolStatus::Ok)),
            "工具结果要回填状态"
        );
        let text = notices(&updates);
        assert!(text.contains("采纳 usr:allow 0.90"), "{text}");
        assert!(text.contains("用时"), "{text}");

        // 读数：两次模型调用、一次判定、一次工具执行，没有拒绝也没有防抖复用。
        let stats = agent.stats();
        assert_eq!(stats.model_calls, 2);
        assert_eq!(stats.judgments, 1);
        assert_eq!(stats.tool_calls, 1);
        assert_eq!(stats.judge_reuses, 0);
        assert_eq!(stats.refusals, 0);

        // 证据与判断都落了盘：三个对象里有两条证据（用户请求、命令）、一条工具结果。
        let panels = agent.panels();
        assert!(panels.judgment.is_some(), "判断详情要有数据");
        let objects: Vec<&str> = panels
            .evidence
            .rows
            .iter()
            .map(|row| row.object.as_str())
            .collect();
        assert!(objects.contains(&"session.ask"), "{objects:?}");
        assert!(objects.contains(&"shell.command"), "{objects:?}");
        assert!(objects.contains(&"tool.result"), "{objects:?}");
        assert!(
            panels.propagation.summary().is_some(),
            "同一对象出新版本后应有失效传播读数"
        );

        // 调用记录：模型两次 + 判定一次，都在同一个 JSONL 里。
        let entries = read_entries(&dir.join("calls.jsonl")).expect("读调用记录");
        assert_eq!(entries.len(), 3, "{entries:?}");
        assert_eq!(
            entries
                .iter()
                .filter(|entry| matches!(entry, CallEntry::Judgment { .. }))
                .count(),
            1
        );
        assert_eq!(
            entries
                .iter()
                .filter(|entry| matches!(entry, CallEntry::ModelCall { .. }))
                .count(),
            2
        );
    }

    #[test]
    fn a_confident_deny_refuses_without_a_card_and_tells_the_model() {
        let model = Scripted::new(vec![
            turn(
                "",
                vec![call("bash", serde_json::json!({"command": "rm -rf build"}))],
            ),
            turn("那我换个做法。", Vec::new()),
        ]);
        let (mut agent, _dir) = session_agent(Some(choice_judge(0.05, 0.95, 0.9)), Some(model));
        let mut updates = agent.submit("清一下构建产物".to_owned());
        updates.extend(run_turn(
            &mut agent,
            ApprovalOutcome::AllowOnce,
            Instant::now(),
        ));

        assert!(!has_approval(&updates), "门控拒绝时不打开审批卡");
        let text = notices(&updates);
        assert!(text.contains("门控拒绝"), "{text}");
        assert!(text.contains("换一个做法"), "拒绝要有出路：{text}");
        assert!(
            updates.iter().any(
                |u| matches!(u, Update::ToolEnd { status, .. } if *status == ToolStatus::Failed)
            ),
            "被拒绝的路径也要上屏"
        );
        assert_eq!(agent.stats().refusals, 1);
        assert_eq!(agent.stats().tool_calls, 0, "被挡下的调用不算执行过");
    }

    /// 工具结果会让「它自己的判定依据」变旧：执行完之后复核，如实说「只作历史记录」。
    ///
    /// 机制：门控判定读过的材料里有 `tool.result`（第一次调用之后就有了），这次工具执行完
    /// 再追加一条 `tool.result` 新证据 → 那个判断节点变「待重查」→ 依赖它的行动不再可发出。
    /// 副作用已经发生、改不了，所以只当历史记录：后续不能再拿它当依据，要动就得重新判定。
    #[test]
    fn a_later_tool_result_invalidates_its_own_judgments_premise() {
        let model = Scripted::new(vec![
            turn(
                "",
                vec![call("bash", serde_json::json!({"command": "ls -1"}))],
            ),
            turn(
                "",
                vec![call("bash", serde_json::json!({"command": "ls -1"}))],
            ),
            turn("两次都跑完了", Vec::new()),
        ]);
        let (mut agent, _dir) = session_agent(Some(choice_judge(0.95, 0.05, 0.9)), Some(model));
        let mut updates = agent.submit("跑两遍".to_owned());
        updates.extend(run_turn(
            &mut agent,
            ApprovalOutcome::AllowOnce,
            Instant::now(),
        ));

        assert_eq!(agent.stats().tool_calls, 2, "两次都真的执行了");
        let text = notices(&updates);
        assert!(
            text.contains("前提已失效：这次执行只作历史记录"),
            "第二次调用的依据被它自己的结果弄旧：{text}"
        );
        assert!(text.contains("bash#2"), "要点名是哪一个行动：{text}");
        assert!(
            !text.contains("bash#1 的前提已失效"),
            "第一次判定时还没有 tool.result 证据，事后复核仍可依赖：{text}"
        );
    }

    /// 只读工具：`auto` 下确定性层直接放行（不问 Jev、不上卡）；`ask` 下照旧要人工确认。
    #[test]
    fn read_only_tools_skip_jev_and_only_ask_in_ask_mode() {
        let model = || {
            Scripted::new(vec![
                turn(
                    "",
                    vec![call("read_file", serde_json::json!({"path": "notes.txt"}))],
                ),
                turn("看完了。", Vec::new()),
            ])
        };

        // auto（缺省）：直接放行
        let (mut agent, _dir) = session_agent(Some(failing_judge("不该被调用")), Some(model()));
        let mut updates = agent.submit("读一下 notes".to_owned());
        updates.extend(run_turn(
            &mut agent,
            ApprovalOutcome::AllowOnce,
            Instant::now(),
        ));
        let text = notices(&updates);
        assert!(text.contains("直接放行"), "{text}");
        assert!(!text.contains("判定不可用"), "只读工具不占用 Jev：{text}");
        assert!(!has_approval(&updates), "auto 下只读工具不上卡：{text}");
        assert_eq!(agent.stats().judgments, 0, "只读工具不发判定");
        assert_eq!(agent.stats().tool_calls, 1);

        // ask：只读工具也要人工确认
        let (mut agent, _dir) = session_agent(Some(failing_judge("不该被调用")), Some(model()));
        force_ask_mode(&mut agent);
        let mut updates = agent.submit("读一下 notes".to_owned());
        updates.extend(run_turn(
            &mut agent,
            ApprovalOutcome::AllowOnce,
            Instant::now(),
        ));
        assert!(has_approval(&updates), "ask 模式下只读工具要人工确认");
        assert_eq!(agent.stats().judgments, 0);
        assert_eq!(agent.stats().tool_calls, 1);
    }

    #[test]
    fn the_whitelist_makes_later_calls_card_free() {
        let model = Scripted::new(vec![
            turn(
                "",
                vec![
                    call("bash", serde_json::json!({"command": "git status"})),
                    call("bash", serde_json::json!({"command": "git diff"})),
                ],
            ),
            turn("都看过了。", Vec::new()),
        ]);
        let (mut agent, _dir) = session_agent(Some(choice_judge(0.9, 0.1, 0.8)), Some(model));
        force_ask_mode(&mut agent);
        let mut updates = agent.submit("看两样".to_owned());
        updates.extend(run_turn(
            &mut agent,
            ApprovalOutcome::AllowAlways,
            Instant::now(),
        ));

        let cards = updates
            .iter()
            .filter(|update| matches!(update, Update::Approval(_)))
            .count();
        assert_eq!(cards, 1, "「本次会话总是允许」之后同类调用不再上卡");
        assert!(
            notices(&updates).contains("白名单"),
            "{}",
            notices(&updates)
        );
        assert_eq!(agent.stats().tool_calls, 2);
        assert!(
            agent.prompt_info().contains("白名单 bash"),
            "{}",
            agent.prompt_info()
        );
    }

    #[test]
    fn the_same_command_within_the_window_reuses_the_judgment() {
        let model = Scripted::new(vec![
            turn(
                "",
                vec![
                    call("bash", serde_json::json!({"command": "cargo test"})),
                    call("bash", serde_json::json!({"command": "cargo test"})),
                ],
            ),
            turn("跑完了。", Vec::new()),
        ]);
        let (mut agent, _dir) = session_agent(Some(choice_judge(0.9, 0.1, 0.8)), Some(model));
        let mut updates = agent.submit("跑测试".to_owned());
        updates.extend(run_turn(
            &mut agent,
            ApprovalOutcome::AllowOnce,
            Instant::now(),
        ));

        let text = notices(&updates);
        assert!(
            text.contains("判定复用"),
            "第二条相同命令应复用判定：{text}"
        );
        assert_eq!(agent.stats().judgments, 1, "只发一次判定");
        assert_eq!(agent.stats().judge_reuses, 1);
        assert_eq!(agent.stats().tool_calls, 2, "复用判定不等于不执行");
    }

    #[test]
    fn a_missing_judge_and_a_missing_model_report_instead_of_pretending() {
        // 判定器缺凭据：上卡时如实写「判定不可用」，仍交人工裁决。
        let model = Scripted::new(vec![turn(
            "",
            vec![call("bash", serde_json::json!({"command": "cargo build"}))],
        )]);
        let (mut agent, _dir) = session_agent(None, Some(model));
        let mut updates = agent.submit("构建一下".to_owned());
        updates.extend(run_turn(&mut agent, ApprovalOutcome::Deny, Instant::now()));
        let text = notices(&updates);
        assert!(text.contains("判定不可用"), "{text}");
        assert!(text.contains("判定器 jev"), "{text}");
        assert!(text.contains("已拒绝"), "人工拒绝要上屏：{text}");
        assert_eq!(agent.stats().denials, 1);
        assert_eq!(agent.stats().tool_calls, 0);

        // 模型缺配置：提交后如实上屏，界面自己还在。
        let (mut agent, _dir) = session_agent(None, None);
        let mut updates = agent.submit("在吗".to_owned());
        updates.extend(run_turn(
            &mut agent,
            ApprovalOutcome::AllowOnce,
            Instant::now(),
        ));
        let text = notices(&updates);
        assert!(text.contains("模型不可用"), "{text}");
        assert!(
            text.contains("DEEPSEEK_API_KEY"),
            "口径与 provider 一致（默认 DeepSeek，不再是 OPENAI_* 三者必填）：{text}"
        );
        assert!(
            updates.iter().any(
                |update| matches!(update, Update::Status(status) if status.phase == Some(TurnPhase::Failed))
            ),
            "失败要落到状态行"
        );
        assert!(
            agent.startup_note().contains("模型未配置"),
            "{}",
            agent.startup_note()
        );
    }

    #[test]
    fn interrupt_clears_the_pending_work_without_pretending_it_ran() {
        let model = Scripted::new(vec![turn(
            "",
            vec![call("bash", serde_json::json!({"command": "cargo build"}))],
        )]);
        let (mut agent, _dir) = session_agent(Some(choice_judge(0.9, 0.1, 0.8)), Some(model));
        force_ask_mode(&mut agent);
        let now = Instant::now();
        agent.submit("构建".to_owned());

        // 跑到门控结算（卡片打开）就打断。
        let (step, _) = agent.advance(now);
        assert!(matches!(step, Step::Model(_)));
        agent.on_model_result(Ok(turn(
            "",
            vec![call("bash", serde_json::json!({"command": "cargo build"}))],
        )));
        // 「每次都问」不问 Jev：`advance` 直接开卡（没有 `Step::Gate`）。
        let (step, _) = agent.advance(now);
        assert!(matches!(step, Step::Idle));
        assert!(agent.awaiting_approval());

        let updates = agent.on_interrupt();
        assert!(!agent.awaiting_approval(), "打断后不再等裁决");
        assert_eq!(agent.stats().tool_calls, 0, "没有执行过任何工具");
        let text = notices(&updates);
        assert!(text.contains("已取消"), "{text}");
        assert!(
            matches!(agent.advance(now).0, Step::Idle),
            "打断后没有在途步骤"
        );
    }

    #[test]
    fn a_submit_while_a_turn_is_running_is_refused_not_queued() {
        let model = Scripted::new(vec![
            turn("第一轮的回复", Vec::new()),
            turn("第二轮", Vec::new()),
        ]);
        let (mut agent, _dir) = session_agent(Some(choice_judge(0.9, 0.1, 0.8)), Some(model));
        let now = Instant::now();
        agent.submit("第一句".to_owned());
        let (step, _) = agent.advance(now);
        assert!(matches!(step, Step::Model(_)), "第一句先进模型");
        assert!(agent.busy());

        // 在途时再提交：拒绝并如实说明，不改状态、不开第二轮。
        let updates = agent.submit("第二句".to_owned());
        assert_eq!(updates.len(), 1, "{updates:?}");
        assert!(
            notices(&updates).contains("还在跑"),
            "{}",
            notices(&updates)
        );
        assert_eq!(agent.stats().turns, 1, "第二次提交没有开新一轮");
        assert!(matches!(&updates[0], Update::Notice { tone, .. } if *tone == NoticeTone::Warning));

        // 第一轮照常收尾，没有被新提交搅乱。
        agent.on_model_result(Ok(turn("答完了", Vec::new())));
        assert!(!agent.busy());
        assert_eq!(agent.stats().turns, 1);
    }

    #[test]
    fn interrupt_during_the_model_call_closes_the_growing_text() {
        let model = Scripted::new(vec![turn("很长的正文", Vec::new())]);
        let (mut agent, _dir) = session_agent(None, Some(model));
        let now = Instant::now();
        agent.submit("在吗".to_owned());
        let (step, _) = agent.advance(now);
        assert!(matches!(step, Step::Model(_)));

        let updates = agent.on_interrupt();
        assert!(
            updates
                .iter()
                .any(|update| matches!(update, Update::AssistantEnd(text) if text.is_empty())),
            "打断也要收起「还在长」的标记：{updates:?}"
        );
        assert!(
            notices(&updates).contains("已取消本轮"),
            "{}",
            notices(&updates)
        );
        assert!(matches!(agent.advance(now).0, Step::Idle));
    }

    #[test]
    fn closing_the_card_without_a_verdict_counts_as_not_run() {
        let model = Scripted::new(vec![
            turn(
                "",
                vec![call("bash", serde_json::json!({"command": "git status"}))],
            ),
            turn("好。", Vec::new()),
        ]);
        let (mut agent, _dir) = session_agent(Some(choice_judge(0.9, 0.1, 0.8)), Some(model));
        force_ask_mode(&mut agent);
        let now = Instant::now();
        agent.submit("看一下".to_owned());
        let (step, _) = agent.advance(now);
        let Step::Model(request) = step else {
            panic!("先问模型")
        };
        let (future, _) = agent.model_run(&request);
        agent.on_model_result(crate::session::test_support::block_on(future));
        // 「每次都问」：再次 `advance` 直接开卡（`Step::Idle` — 没有 Jev 调用）。
        let (step, _) = agent.advance(now);
        assert!(matches!(step, Step::Idle));
        assert!(agent.awaiting_approval(), "卡片开着");

        let updates = agent.on_cancel();
        let text = notices(&updates);
        assert!(text.contains("未裁决"), "{text}");
        assert_eq!(agent.stats().tool_calls, 0);
        assert_eq!(agent.stats().denials, 1);
    }

    /// 缺省上限：一个会话轮次内允许 32 次模型调用（`main.rs` 的 CLI 缺省值也取它）。
    #[test]
    fn the_default_round_budget_is_32() {
        assert_eq!(DEFAULT_MAX_ROUNDS, 32);
    }

    /// 一个只读文件的工具调用：结果无副作用，参数按序号变（调用 ID 不重复）。
    fn read_call(index: usize) -> ToolCall {
        call(
            "read_file",
            serde_json::json!({"path": "notes.txt", "offset": index + 1}),
        )
    }

    /// `max_rounds = 0` 是不限：模型调用顶过缺省上限也不因上限停下。
    #[test]
    fn zero_max_rounds_means_no_limit() {
        let mut turns: Vec<ModelTurn> = (0..44)
            .map(|index| turn("", vec![read_call(index)]))
            .collect();
        turns.push(turn("读完了。", Vec::new()));
        let model = Scripted::new(turns);
        // 判定器不接线也够：read_file 走确定性层，不问 Jev。
        let (mut agent, _dir) = session_agent_with_max_rounds(None, Some(model), 0);
        let now = Instant::now();

        let mut updates = agent.submit("一直读".to_owned());
        updates.extend(run_steps(
            &mut agent,
            ApprovalOutcome::AllowAlways,
            now,
            400,
        ));

        assert_eq!(
            agent.stats().model_calls,
            45,
            "44 次工具轮 + 1 次收尾回复都该跑完"
        );
        let text = notices(&updates);
        assert!(
            !text.contains("模型调用上限"),
            "0 = 不限，不该撞上限：{text}"
        );
        assert!(!agent.busy(), "脚本跑完正常收尾");
    }

    /// 命中上限不是无声收尾：通知给出 `/continue` 这条可继续的路。
    #[test]
    fn hitting_the_round_limit_offers_a_continue_path() {
        let model = Scripted::new(vec![
            turn("", vec![read_call(0)]),
            turn("", vec![read_call(1)]),
            turn("本来还能接着说。", Vec::new()),
        ]);
        let (mut agent, _dir) = session_agent_with_max_rounds(None, Some(model), 2);
        let now = Instant::now();

        let mut updates = agent.submit("读两遍".to_owned());
        updates.extend(run_turn(&mut agent, ApprovalOutcome::AllowAlways, now));

        let text = notices(&updates);
        assert!(text.contains("本轮已达 2 次模型调用上限"), "{text}");
        assert!(text.contains("/continue"), "上限要有出路：{text}");
        assert!(text.contains("对话与工作区状态都保留"), "{text}");
        assert_eq!(agent.stats().model_calls, 2, "上限是硬的：第 3 次不起");
        assert!(!agent.busy(), "停下等用户决定，不空转");
        assert!(
            matches!(agent.advance(now).0, Step::Idle),
            "没有 /continue 就不再起模型调用"
        );
    }

    /// 命中上限后 `/continue` 把这一轮接回去：计数归零，下一次 `advance` 再起模型调用。
    #[test]
    fn continue_resumes_after_the_limit_is_hit() {
        let model = Scripted::new(vec![
            turn("", vec![read_call(0)]),
            turn("", vec![read_call(1)]),
            turn("续上了。", Vec::new()),
        ]);
        let (mut agent, _dir) = session_agent_with_max_rounds(None, Some(model), 2);
        let now = Instant::now();
        agent.submit("读两遍".to_owned());
        run_turn(&mut agent, ApprovalOutcome::AllowAlways, now);
        assert_eq!(agent.stats().model_calls, 2, "先撞到上限");

        // `/continue`：计数归零、欠模型的那次回应重新记上。
        let updates = agent.on_command(CommandCall {
            name: CONTINUE_COMMAND,
            args: String::new(),
        });
        assert!(
            notices(&updates).contains("接着跑"),
            "{:?}",
            notices(&updates)
        );
        assert!(agent.busy(), "接着跑时是忙的");

        let (step, _) = agent.advance(now);
        let Step::Model(request) = step else {
            panic!("/continue 之后要再起一次模型调用");
        };
        let (future, _deltas) = agent.model_run(&request);
        let result = crate::session::test_support::block_on(future);
        let updates = agent.on_model_result(result);
        assert!(
            updates
                .iter()
                .any(|update| matches!(update, Update::AssistantEnd(text) if text == "续上了。")),
            "接上的是脚本里下一条回复：{updates:?}"
        );
        assert_eq!(agent.stats().model_calls, 3);
        assert!(!agent.busy(), "这一轮收尾了");

        // 没有欠账时 `/continue` 不虚构活干。
        let updates = agent.on_command(CommandCall {
            name: CONTINUE_COMMAND,
            args: String::new(),
        });
        let text = notices(&updates);
        assert!(text.contains("没有需要接着跑的活"), "{text}");
        assert!(matches!(agent.advance(now).0, Step::Idle));
    }
}
