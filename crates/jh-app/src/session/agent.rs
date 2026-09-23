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
use jh_tui::app::ApprovalOutcome;
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
pub const DEFAULT_MAX_ROUNDS: usize = 8;

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
    /// 本轮模型调用上限。
    pub max_rounds: usize,
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
    /// 助手正文收尾（收起「还在长」的标记）。
    AssistantEnd,
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
    last_invalidation: Option<Invalidation>,
    last_node: Option<NodeId>,
    turn_started: Option<Instant>,
    stats: Stats,
    /// 上一次模型请求的字节数（调用记录的输入规模）。
    last_request_bytes: u64,
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
            max_rounds: config.max_rounds.max(1),
            last_invalidation: None,
            last_node: None,
            turn_started: None,
            stats: Stats::default(),
            last_request_bytes: 0,
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

    /// 用户提交一句话：回显、写证据、准备开轮。
    pub fn submit(&mut self, text: String) -> Vec<Update> {
        let mut updates = Vec::new();
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
        self.conversation.push(InputItem::user_text(text));
        self.owes_model = true;
        self.rounds = 0;
        self.stats.turns += 1;
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
                updates.push(Update::AssistantEnd);
                if !turn.text.trim().is_empty() {
                    self.conversation
                        .push(InputItem::assistant_text(turn.text.clone()));
                }
                for call in &turn.calls {
                    self.conversation.push(InputItem::function_call(
                        call.name.clone(),
                        call.call_id.clone(),
                        call.arguments.clone(),
                    ));
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
                updates.push(Update::AssistantEnd);
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
        match outcome {
            ApprovalOutcome::AllowOnce | ApprovalOutcome::AllowAlways => {
                self.begin_run(call, node, &mut updates);
            }
            ApprovalOutcome::Deny => {
                self.stats.denials += 1;
                let text = "用户拒绝了这次调用；请换一个做法，不要重试同一条命令".to_owned();
                updates.push(Update::ToolBegin {
                    name: call.name.clone(),
                    args: self.tools.display(&call),
                });
                updates.push(Update::ToolEnd {
                    status: ToolStatus::Failed,
                    detail: "已拒绝".to_owned(),
                });
                updates.push(Update::Notice {
                    text: format!("已拒绝 {}：{text}", self.tools.display(&call)),
                    tone: NoticeTone::Warning,
                });
                self.conversation
                    .push(InputItem::function_call_output(call.call_id.clone(), text));
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
        updates.push(Update::ToolBegin {
            name: call.name.clone(),
            args: self.tools.display(&call),
        });
        updates.push(Update::ToolEnd {
            status: ToolStatus::Failed,
            detail: "未裁决".to_owned(),
        });
        updates.push(Update::Notice {
            text: "未裁决：这次调用没有执行".to_owned(),
            tone: NoticeTone::Warning,
        });
        self.conversation.push(InputItem::function_call_output(
            call.call_id.clone(),
            "用户没有裁决这次调用（卡片被关闭），未执行".to_owned(),
        ));
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

        self.conversation.push(InputItem::function_call_output(
            call.call_id.clone(),
            outcome.text,
        ));
        self.after_call(&mut updates);
        updates.push(self.panels_update());
        updates
    }

    /// 取消当前轮（`Esc`）：在途的调用作废，队列清空，如实上屏。
    pub fn on_interrupt(&mut self) -> Vec<Update> {
        let mut updates = Vec::new();
        let phase = std::mem::replace(&mut self.phase, Phase::Idle);
        self.queue.clear();
        self.owes_model = false;
        match phase {
            Phase::Modeling => updates.push(Update::Notice {
                text: "已取消本轮：模型调用在途，这次回复作废".to_owned(),
                tone: NoticeTone::Warning,
            }),
            Phase::Running { call, .. } => {
                updates.push(Update::ToolEnd {
                    status: ToolStatus::Failed,
                    detail: "已取消".to_owned(),
                });
                self.conversation.push(InputItem::function_call_output(
                    call.call_id.clone(),
                    "用户取消了这一轮，调用未完成".to_owned(),
                ));
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
        if self.rounds >= self.max_rounds {
            updates.push(Update::Notice {
                text: format!("本轮已达模型调用上限（{} 次）", self.max_rounds),
                tone: NoticeTone::Warning,
            });
            self.finish_turn(true, updates);
            return Step::Idle;
        }
        if self.model.is_none() {
            updates.push(Update::Notice {
                text: format!(
                    "模型不可用（{}）：配好 OPENAI_API_KEY / OPENAI_MODEL / OPENAI_BASE_URL 后重启会话",
                    self.model_label
                ),
                tone: NoticeTone::Error,
            });
            self.finish_turn(false, updates);
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
        self.stats.model_calls += 1;
        self.phase = Phase::Modeling;
        updates.push(Update::Status(self.running(WaitingReason::LlmStreaming)));
        Step::Model(request)
    }

    /// 起一次门控：确定性层先分类，Jev 只看 shell 命令，判定先查防抖缓存。
    fn start_gate(&mut self, call: ToolCall, now: Instant, updates: &mut Vec<Update>) -> Step {
        let display = self.tools.display(&call);

        // 只读工具：确定性层已经能判（工作区内、不改动任何东西），不问 Jev。
        if call.name != BASH {
            updates.push(Update::Notice {
                text: format!("确定性层：{display} 是工作区内的只读工具"),
                tone: NoticeTone::Info,
            });
            if self.approval.is_preapproved(&call.name) {
                updates.push(Update::Notice {
                    text: format!("{} 在本次会话白名单里，直接放行", call.name),
                    tone: NoticeTone::Info,
                });
                self.begin_run(call, None, updates);
                return Step::Idle;
            }
            let request = self.approval_request(&call, None, Risk::ReadOnly);
            self.phase = Phase::Awaiting { call, node: None };
            updates.push(Update::Status(self.running(WaitingReason::Approval)));
            updates.push(Update::Approval(Box::new(request)));
            updates.push(self.panels_update());
            return Step::Idle;
        }

        // 材料证据：这次命令本身出新版本（读过 `shell.command` 的旧判断由此变旧）。
        let command = command_of(&call);
        let risk = gate::assess(&command);
        self.append_evidence(
            view::objects::SHELL_COMMAND,
            "会话",
            now_unix_ms(),
            serde_json::json!({
                "note": clip(&format!("{display}（{}）", risk.label()), 60),
                "command": command,
                "cwd": self.workspace.display().to_string(),
                "risk": risk.label(),
            }),
            updates,
        );

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

        let risk = gate::assess(&command_of(&call));
        if decision.refused() {
            self.stats.refusals += 1;
            let text = decision.refuse_text();
            updates.push(Update::ToolBegin {
                name: call.name.clone(),
                args: command_of(&call),
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

        if self.approval.is_preapproved(&call.name) {
            updates.push(Update::Notice {
                text: format!("{} 在本次会话白名单里，直接放行", call.name),
                tone: NoticeTone::Info,
            });
            self.begin_run(call, Some(node), updates);
            return;
        }

        let request = self.approval_request(&call, Some(&decision), risk);
        self.phase = Phase::Awaiting {
            call,
            node: Some(node),
        };
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
            args: self.tools.display(&call),
        });
        updates.push(Update::Status(self.running(WaitingReason::ToolRunning)));
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
             - 要看代码或文件就用 read_file / list_dir / search；要跑命令就用 bash。\n\
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
            None => body.push_str("（只读工具，不占用 Jev）"),
        }
        ApprovalRequest::new(call.name.clone(), self.tools.display(call)).with_body(body)
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
    use jh_tui::app::ApprovalOutcome;

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
            max_rounds: 4,
        })
        .expect("建会话");
        (agent, dir)
    }

    /// 把一轮跑到收尾：模型按脚本、门控按桩、工具真跑、审批用给定裁决。
    fn run_turn(agent: &mut Agent, approval: ApprovalOutcome, now: Instant) -> Vec<Update> {
        let mut all = Vec::new();
        for _ in 0..24 {
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

    fn has_approval(updates: &[Update]) -> bool {
        updates
            .iter()
            .any(|update| matches!(update, Update::Approval(_)))
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
        assert!(has_approval(&updates), "shell 命令要先上卡");
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

    #[test]
    fn read_only_tools_skip_jev_but_still_ask_a_human() {
        let model = Scripted::new(vec![
            turn(
                "",
                vec![call("read_file", serde_json::json!({"path": "notes.txt"}))],
            ),
            turn("看完了。", Vec::new()),
        ]);
        let (mut agent, _dir) = session_agent(Some(failing_judge("不该被调用")), Some(model));
        let mut updates = agent.submit("读一下 notes".to_owned());
        updates.extend(run_turn(
            &mut agent,
            ApprovalOutcome::AllowOnce,
            Instant::now(),
        ));

        let text = notices(&updates);
        assert!(text.contains("确定性层"), "{text}");
        assert!(!text.contains("判定不可用"), "只读工具不占用 Jev：{text}");
        assert!(has_approval(&updates), "只读工具同样要过审批卡");
        assert_eq!(agent.stats().judgments, 0, "只读工具不发判定");
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
        let now = Instant::now();
        agent.submit("构建".to_owned());

        // 跑到门控结算（卡片打开）就打断。
        let (step, _) = agent.advance(now);
        assert!(matches!(step, Step::Model(_)));
        agent.on_model_result(Ok(turn(
            "",
            vec![call("bash", serde_json::json!({"command": "cargo build"}))],
        )));
        let (step, _) = agent.advance(now);
        let Step::Gate(call) = step else {
            panic!("应进门控")
        };
        let decision = crate::session::test_support::block_on(call.ask());
        agent.on_gate_result(decision, now);
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
    fn closing_the_card_without_a_verdict_counts_as_not_run() {
        let model = Scripted::new(vec![
            turn(
                "",
                vec![call("bash", serde_json::json!({"command": "git status"}))],
            ),
            turn("好。", Vec::new()),
        ]);
        let (mut agent, _dir) = session_agent(Some(choice_judge(0.9, 0.1, 0.8)), Some(model));
        let now = Instant::now();
        agent.submit("看一下".to_owned());
        let (step, _) = agent.advance(now);
        let Step::Model(request) = step else {
            panic!("先问模型")
        };
        let (future, _) = agent.model_run(&request);
        agent.on_model_result(crate::session::test_support::block_on(future));
        let (step, _) = agent.advance(now);
        let Step::Gate(call) = step else {
            panic!("再进门控")
        };
        let decision = crate::session::test_support::block_on(call.ask());
        agent.on_gate_result(decision, now);
        assert!(agent.awaiting_approval(), "卡片开着");

        let updates = agent.on_cancel();
        let text = notices(&updates);
        assert!(text.contains("未裁决"), "{text}");
        assert_eq!(agent.stats().tool_calls, 0);
        assert_eq!(agent.stats().denials, 1);
    }
}
