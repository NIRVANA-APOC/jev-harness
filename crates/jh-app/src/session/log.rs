//! 会话日志：`var/chat/<run>/session.jsonl`，一行一个事件，供**真实使用场景**的事后分析。
//!
//! 与另外两份落盘的边界：
//!
//! | 文件 | 记什么 | 给谁看 |
//! | --- | --- | --- |
//! | `var/calls.jsonl` | 每次模型/判断调用的记账（token、延迟、request_id） | 成本与实验口径 |
//! | `var/chat/<run>/evidence.jsonl` | 进入语义状态的证据（`session.ask` / `shell.command` / `tool.result`） | 失效传播与验收 |
//! | **本文件** | **完整会话过程**：用户说了什么、模型回了什么、工具跑了什么、门控与审批怎么裁的、哪里失败、各段耗时 | 使用场景分析 |
//!
//! 口径：
//!
//! - **追加写**、一行一个 JSON 对象、带 `kind` 判别标签；每行写完立即 flush（进程被杀也不丢已落盘的事件）；
//! - 长文本按 [`MAX_TEXT_BYTES`] 截断，但**保留原始字节数**与 `truncated` 标记（分析时知道被截了，而不是
//!   以为模型只说了这么点）；
//! - **不读、也不写任何凭据**：key 只在环境变量里，本模块不碰环境变量；`workspace` 之类路径是用户自己的
//!   数据，照实记；
//! - 写失败不静默：返回 [`crate::error::Error`]，由宿主如实上屏（与调用记录同一套口径）；
//! - 事件里的枚举值（`verdict` / `status` / `decision` / `stage` / `scope`）是**稳定词表**，取值见各字段注释，
//!   分析脚本可以照它们分组。
//!
//! 事件顺序即时间顺序；`turn` 是用户提交的序号（从 1 起），`round` 是这一轮内的模型调用序号（从 1 起），
//! 因此 `(turn, round)` 可以把模型、门控、审批、工具四类事件串起来。

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::Error;

/// 单个文本字段的落盘上限；超出的部分丢掉，只留字节数与截断标记。
pub const MAX_TEXT_BYTES: usize = 16 * 1024;

/// 已经按上限处理过的文本。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoggedText {
    /// 原文（截断后）。
    pub text: String,
    /// 原始 UTF-8 字节数（截断前）。
    pub bytes: u64,
    /// 是否被截断。
    pub truncated: bool,
}

impl LoggedText {
    /// 按 [`MAX_TEXT_BYTES`] 截断（落在字符边界上，不会切出半个 UTF-8）。
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let bytes = text.len() as u64;
        if text.len() <= MAX_TEXT_BYTES {
            return LoggedText {
                text,
                bytes,
                truncated: false,
            };
        }
        let mut end = MAX_TEXT_BYTES;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        LoggedText {
            text: text[..end].to_owned(),
            bytes,
            truncated: true,
        }
    }
}

/// 模型一次回复里请求的一个工具调用。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoggedToolCall {
    /// 工具名（`bash` / `read_file` / …）。
    pub name: String,
    /// 参数 JSON 原文。
    pub arguments: String,
}

/// 门控采纳门槛（写进日志，事后能复算「当时为什么采纳」）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GateThresholds {
    pub margin: f64,
    pub confidence: f64,
}

/// 会话里的一个事件。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SessionEvent {
    /// 会话开始：环境与配置（**不含凭据**）。
    SessionStart {
        /// 写这条事件的时刻。
        at_unix_ms: u64,
        /// 本次运行标识（与 `var/chat/<run>/` 同名）。
        run: String,
        /// `jh-app` 的版本。
        version: String,
        /// 工作区根。
        workspace: String,
        /// 门控判定器名（`jev` / `llm` / `rule`）。
        judge: String,
        /// 模型标识（如 `deepseek-flash`；未配置时是原因文案）。
        model: String,
        /// 本轮模型调用上限；0 = 不限。
        max_rounds: usize,
        /// 门控防抖窗口（毫秒）。
        debounce_ms: u64,
        /// 色彩能力（`none` / `basic` / `ansi256` / `truecolor`）。
        color: String,
        /// 字形集（`fancy` / `fallback`）。
        glyphs: String,
        /// 标准输出是不是终端。
        tty: bool,
    },
    /// 用户提交了一句话。
    UserMessage {
        /// 事件时刻（Unix 毫秒）。
        at_unix_ms: u64,
        /// 第几次用户提交（从 1 起）。
        turn: usize,
        /// 文本（按上限截断）。
        text: LoggedText,
    },
    /// 一条斜杠命令（界面自处理与宿主命令都记；`/quit` 不记，会话结束事件覆盖它）。
    Command {
        /// 事件时刻（Unix 毫秒）。
        at_unix_ms: u64,
        /// 名称（命令名或工具名）。
        name: String,
        /// 参数原文。
        args: String,
        /// `ui`（界面自处理）或 `host`（交会话状态处理）。
        scope: String,
    },
    /// 起一次模型调用。
    ModelRequest {
        /// 事件时刻（Unix 毫秒）。
        at_unix_ms: u64,
        /// 第几次用户提交（从 1 起）。
        turn: usize,
        /// 该轮内第几次模型调用（从 1 起）。
        round: usize,
        /// 这一轮累计的模型调用序号（与 `round` 相同，留作显式分组键）。
        model: String,
        /// 输入序列化后的字节数。
        input_bytes: u64,
        /// 输入项个数（对话轮数）。
        items: usize,
        /// 这次请求可用的工具名。
        tools: Vec<String>,
    },
    /// 模型调用返回（成功或失败都在这里）。
    ModelResponse {
        /// 事件时刻（Unix 毫秒）。
        at_unix_ms: u64,
        /// 第几次用户提交（从 1 起）。
        turn: usize,
        /// 该轮内第几次模型调用（从 1 起）。
        round: usize,
        /// 执行者版本（响应里的模型 ID）。
        model: String,
        /// 通道侧请求 id。
        request_id: Option<String>,
        /// 本次耗时（毫秒）。
        latency_ms: u64,
        /// 输入 token（未上报为 `None`）。
        input_tokens: Option<u64>,
        /// 输出 token（未上报为 `None`）。
        output_tokens: Option<u64>,
        /// 助手正文。
        text: LoggedText,
        /// 这次回复请求的工具调用。
        tool_calls: Vec<LoggedToolCall>,
        /// 失败原因；成功为 `None`。
        error: Option<String>,
    },
    /// 一次门控判定（含确定性层直接放行的那种，`source` 会写明是 `rule`）。
    GateDecision {
        /// 事件时刻（Unix 毫秒）。
        at_unix_ms: u64,
        /// 第几次用户提交（从 1 起）。
        turn: usize,
        /// 该轮内第几次模型调用（从 1 起）。
        round: usize,
        /// 防抖键（工具名 + 参数摘要）。
        key: String,
        /// `allow`（采纳允许）/ `refuse`（采纳拒绝）/ `ask`（判不了，交人工）。
        verdict: String,
        /// 判定来源（`real_api` / `llm` / `rule` / …）。
        source: String,
        /// 当时的授权模式（`auto` / `ask` / `never`）——分析「用户怎么配权限」用。
        mode: String,
        /// 执行者版本（Jev 是模型 ID；规则是规则版本串）。
        executor: Option<String>,
        /// 通道侧请求 id。
        request_id: Option<String>,
        /// 本次耗时（毫秒）。
        latency_ms: u64,
        /// 是否来自防抖复用（没有发新调用）。
        reused: bool,
        /// 首选与次选的概率差。
        margin: Option<f64>,
        /// 服务端给出的 confidence。
        confidence: Option<f64>,
        /// 选中项（如 `usr:allow`）。
        choice: Option<String>,
        /// 概率原值（不塌缩成布尔）。
        probabilities: Option<BTreeMap<String, f32>>,
        /// 当时生效的采纳门槛。
        thresholds: GateThresholds,
        /// 判定失败时的错误原文。
        failure: Option<String>,
        /// 门控理由（与屏上一致）。
        reason: String,
    },
    /// 审批卡弹出。
    ApprovalRequest {
        /// 事件时刻（Unix 毫秒）。
        at_unix_ms: u64,
        /// 第几次用户提交（从 1 起）。
        turn: usize,
        /// 该轮内第几次模型调用（从 1 起）。
        round: usize,
        /// 工具名。
        tool: String,
        /// 参数摘要（与卡片正文同源）。
        command: String,
        /// 卡片正文（风险提示）；没有就是 `None`。
        body: Option<String>,
    },
    /// 人工裁决（或卡片被关掉）。
    ApprovalResult {
        /// 事件时刻（Unix 毫秒）。
        at_unix_ms: u64,
        /// 第几次用户提交（从 1 起）。
        turn: usize,
        /// 该轮内第几次模型调用（从 1 起）。
        round: usize,
        /// 工具名。
        tool: String,
        /// `allow_once` / `allow_always` / `deny` / `cancel`（Esc 关掉没裁决）。
        decision: String,
        /// 从弹卡到裁决的等待时长。
        waited_ms: u64,
    },
    /// 一次工具执行的完整记录（参数 + 结果 + 耗时）。
    ToolCall {
        /// 事件时刻（Unix 毫秒）。
        at_unix_ms: u64,
        /// 第几次用户提交（从 1 起）。
        turn: usize,
        /// 该轮内第几次模型调用（从 1 起）。
        round: usize,
        /// 名称（命令名或工具名）。
        name: String,
        /// 实际执行的命令或参数摘要（与屏上同源）。
        arguments: String,
        /// `ok` / `failed`（与工具结果状态同词表）。
        status: String,
        /// 工具输出（按上限截断）。
        output: LoggedText,
        /// 本次耗时（毫秒）。
        latency_ms: u64,
    },
    /// 一次显式失败（模型不可用、通道失败、门控失败…）。
    Error {
        /// 事件时刻（Unix 毫秒）。
        at_unix_ms: u64,
        /// 第几次用户提交（从 1 起）。
        turn: usize,
        /// 该轮内第几次模型调用（从 1 起）。
        round: usize,
        /// `model` / `gate` / `tool` / `log`。
        stage: String,
        /// 文本（按上限截断）。
        text: String,
    },
    /// 用户取消在途轮次（`Esc`）。
    Interrupt {
        /// 事件时刻（Unix 毫秒）。
        at_unix_ms: u64,
        /// 第几次用户提交（从 1 起）。
        turn: usize,
        /// 该轮内第几次模型调用（从 1 起）。
        round: usize,
        /// 取消时在途的是哪一步：`model` / `gate` / `tool` / `idle`。
        stage: String,
    },
    /// 会话结束：整场读数。
    SessionEnd {
        /// 事件时刻（Unix 毫秒）。
        at_unix_ms: u64,
        /// 从会话开始到结束的墙钟时长。
        duration_ms: u64,
        /// 用户提交次数。
        turns: usize,
        /// 模型调用次数。
        model_calls: u64,
        /// 这次回复请求的工具调用。
        tool_calls: u64,
        /// 发出的门控判定次数。
        judgments: u64,
        /// 防抖复用次数。
        judge_reuses: u64,
        /// 门控拒绝次数。
        refusals: u64,
        /// 人工拒绝与未裁决次数。
        denials: u64,
    },
    /// `/jev` 手工问了一次判定器：读数原样落盘（`error` 有值表示这次没问成）。
    ManualJudgment {
        /// 事件时刻。
        at_unix_ms: u64,
        /// 问的是什么。
        question: String,
        /// 选项名（顺序即请求里的顺序）。
        labels: Vec<String>,
        /// 材料字节数（材料本体不进日志）。
        material_bytes: usize,
        /// 判定器的 CLI 名字（`jev` / `llm` / `rule`）。
        judge: String,
        /// 来源标签；没问成时为 `None`。
        source: Option<String>,
        /// 执行者版本（真实通道是响应里的模型 ID）。
        version: Option<String>,
        /// 通道侧请求 id。
        request_id: Option<String>,
        /// 这次调用耗时。
        latency_ms: Option<u64>,
        /// 概率最高的选项；没问成时为 `None`。
        choice: Option<String>,
        /// 选项 → 概率（原值）。
        probabilities: BTreeMap<String, f64>,
        /// 服务端给的 confidence（原值）。
        confidence: Option<f64>,
        /// 首选与次选的概率差。
        margin: Option<f64>,
        /// 没问成时的原因（判定器不可用 / 参数不合法）。
        error: Option<String>,
    },
}

impl SessionEvent {
    /// 事件标签（`kind` 的取值）；分析脚本与测试都用它分组。
    pub fn kind(&self) -> &'static str {
        match self {
            SessionEvent::SessionStart { .. } => "session_start",
            SessionEvent::UserMessage { .. } => "user_message",
            SessionEvent::Command { .. } => "command",
            SessionEvent::ModelRequest { .. } => "model_request",
            SessionEvent::ModelResponse { .. } => "model_response",
            SessionEvent::GateDecision { .. } => "gate_decision",
            SessionEvent::ApprovalRequest { .. } => "approval_request",
            SessionEvent::ApprovalResult { .. } => "approval_result",
            SessionEvent::ToolCall { .. } => "tool_call",
            SessionEvent::Error { .. } => "error",
            SessionEvent::Interrupt { .. } => "interrupt",
            SessionEvent::ManualJudgment { .. } => "manual_judgment",
            SessionEvent::SessionEnd { .. } => "session_end",
        }
    }
}

/// 会话日志的写入端（进程内单写者）。
#[derive(Debug)]
pub struct SessionLog {
    file: File,
    path: PathBuf,
}

impl SessionLog {
    /// 打开（必要时建父目录），追加写。
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, Error> {
        let path = path.into();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Self { file, path })
    }

    /// 追加一个事件并立即 flush。
    pub fn append(&mut self, event: &SessionEvent) -> Result<(), Error> {
        let line = serde_json::to_string(event)?;
        self.file.write_all(line.as_bytes())?;
        self.file.write_all(b"\n")?;
        self.file.flush()?;
        Ok(())
    }

    /// 日志文件路径（上屏与回报里用）。
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// 读回会话日志（分析脚本与测试用）；空行跳过，坏行显式失败。
pub fn read_events(path: &Path) -> Result<Vec<SessionEvent>, Error> {
    let text = std::fs::read_to_string(path).map_err(|error| {
        Error::Judge(jh_core::Error::Unavailable(format!(
            "会话日志不可读（{}）：{error}",
            path.display()
        )))
    })?;
    let mut events = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let event = serde_json::from_str(line).map_err(|error| {
            Error::Judge(jh_core::Error::InvalidResponse(format!(
                "会话日志第 {} 行不是合法事件：{error}",
                index + 1
            )))
        })?;
        events.push(event);
    }
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::test_support::TempDir;

    #[test]
    fn short_text_is_kept_verbatim() {
        let logged = LoggedText::new("甲乙丙");
        assert_eq!(logged.text, "甲乙丙");
        assert_eq!(logged.bytes, 9);
        assert!(!logged.truncated);
    }

    #[test]
    fn long_text_is_truncated_on_a_char_boundary_and_reports_the_original_size() {
        // 每个汉字 3 字节：造一个刚好越过上限的字符串。
        let text = "汉".repeat(MAX_TEXT_BYTES / 3 + 10);
        let logged = LoggedText::new(text.clone());
        assert!(logged.truncated);
        assert_eq!(logged.bytes, text.len() as u64, "原始字节数照实记");
        assert!(logged.text.len() <= MAX_TEXT_BYTES);
        assert!(logged.text.len() < text.len());
        assert!(text.starts_with(&logged.text), "截断只是砍尾巴，不改内容");
    }

    #[test]
    fn events_round_trip_through_the_file_in_order() {
        let dir = TempDir::new("session-log");
        let path = dir.join("session.jsonl");
        let mut log = SessionLog::open(&path).expect("开日志");

        log.append(&SessionEvent::SessionStart {
            at_unix_ms: 1,
            run: "run-1".to_owned(),
            version: "0.1.0".to_owned(),
            workspace: "/tmp/ws".to_owned(),
            judge: "jev".to_owned(),
            model: "deepseek-flash".to_owned(),
            max_rounds: 32,
            debounce_ms: 30_000,
            color: "truecolor".to_owned(),
            glyphs: "fancy".to_owned(),
            tty: true,
        })
        .expect("写开始事件");
        log.append(&SessionEvent::UserMessage {
            at_unix_ms: 2,
            turn: 1,
            text: LoggedText::new("你好"),
        })
        .expect("写用户消息");
        log.append(&SessionEvent::SessionEnd {
            at_unix_ms: 3,
            duration_ms: 2,
            turns: 1,
            model_calls: 1,
            tool_calls: 0,
            judgments: 1,
            judge_reuses: 0,
            refusals: 0,
            denials: 0,
        })
        .expect("写结束事件");

        let events = read_events(&path).expect("读回日志");
        assert_eq!(events.len(), 3);
        assert_eq!(
            events.iter().map(SessionEvent::kind).collect::<Vec<_>>(),
            vec!["session_start", "user_message", "session_end"]
        );
        assert!(matches!(
            &events[1],
            SessionEvent::UserMessage { text, .. } if text.text == "你好"
        ));
    }

    #[test]
    fn every_kind_has_a_stable_tag() {
        // 词表是分析脚本的契约：改动会被这条测试挡住。
        let kinds = [
            "session_start",
            "user_message",
            "command",
            "model_request",
            "model_response",
            "gate_decision",
            "approval_request",
            "approval_result",
            "tool_call",
            "error",
            "interrupt",
            "manual_judgment",
            "session_end",
        ];
        assert_eq!(kinds.len(), 13);
        for kind in kinds {
            let json = format!("{{\"kind\":\"{kind}\"}}");
            assert!(
                serde_json::from_str::<SessionEvent>(&json).is_err(),
                "空事件应当解析失败（缺字段），但 kind `{kind}` 被认成了合法事件"
            );
        }
    }
}
