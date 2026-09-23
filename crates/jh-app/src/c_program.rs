//! 隔离实验环境中的可控程序、宿主钩子点与隐藏检查（研究目标 §7.3、§7.4；路线图 §6.3 的任务族 C）。
//!
//! 任务族 A / B 的环境是「脚本化的证据变化」；族 C 的环境换一类东西：**一个可以停在写入之前的
//! 可控程序**，加上宿主提供的三类钩子点与一条独立证据通道。§7.3 的标志性实验
//! 「到达写入前暂停点 → 请求取消 → 收到确认 → 恢复执行，检查同一任务是否在确认之后写入」就落在这里。
//!
//! # 三条证据通道（这是本模块与 [`crate::c_measure`] 的结构分界）
//!
//! | 通道 | 谁写的 | 谁读 | 能不能被绕过 |
//! | --- | --- | --- | --- |
//! | 记录器 [`RecorderLog`]（`observer:c-write-recorder`） | 宿主挂在写入边界的钩子 | 观测器（[`crate::c_measure`]） | **能**：程序走另一条提交路径或直接抹自己的事件 |
//! | 暂停点轨迹 [`PauseTrace`]（`host:c-pause-points`） | 宿主控制面 | 观测器与隐藏检查 | 不能（程序碰不到控制面） |
//! | 存储日志 [`CacheStore::journal`]（`hidden:c-effect-journal`） | 存储自己（每次提交/失效） | **只有隐藏检查** | 不能：除非程序根本不写 |
//!
//! 隐藏检查（[`hidden_check`]）**只读存储日志**，不看记录器、不看程序自报状态。它与观测器在不同
//! 模块、读不同数据——这是「测量脚本的成功输出不能成为唯一的真实性来源」能成立的结构前提。
//! 反过来，记录器被绕过或被篡改时，存储日志照旧记下真实副作用。
//!
//! # 可控程序与三个故障族（[`CFamily`]）
//!
//! 程序是一段**确定性**的指令解释器（[`Op`]），调度（[`Schedule`]）决定交错顺序。没有随机数：
//! 竞态的重复不是靠掷骰子，而是靠 N 条不同的确定性调度——重复十次仍然可复现。
//!
//! | 族 | 要求的顺序约束 | 隐藏检查从存储日志里认定的违反 |
//! | --- | --- | --- |
//! | `cancel_after_confirm`（标志性任务） | 取消确认之后同一任务不得再写入 | 该任务在确认事件之后仍有提交 |
//! | `lost_update` | 并发交替下不得丢失已提交的更新 | 同一键上有两条以同一基准版本提交的记录 |
//! | `duplicate_after_restart` | 重启重放不得把同一写入意图提交两次 | 同一 `(任务, 意图)` 在日志里提交了两次 |
//!
//! 三条判定都只从存储日志算得，不看记录器、不看程序自报——见「隐藏检查」一节。
//!
//! # 版本变更（[`ProgramVersion`]）
//!
//! v1 只有一条写入边界；v2 新增**批量提交**路径（同一个语义边界下的第二个落地点）。
//! 记录器是否覆盖直接提交路径由 [`Attachment::direct_covered`] 决定——这正是「宿主代码改了、老钩子点
//! 不再覆盖新的写入路径」的对照：不重挂就漏记，重挂才看得全。这不是虚构的差异，见
//! [`crate::h4`] 的版本变更读数。

use std::collections::BTreeMap;
use std::fmt;

use jh_core::{ObjectId, ObserverVerdict};
use serde::{Deserialize, Serialize};

/// 记录器通道名：观测器产出证据时 `source` 字段的取值（§7.2 第 7 条的口径）。
pub const RECORDER_CHANNEL: &str = "observer:c-write-recorder";
/// 暂停点通道名：宿主控制面写下的暂停 / 取消 / 恢复轨迹。
pub const PAUSE_CHANNEL: &str = "host:c-pause-points";
/// 隐藏检查通道名：存储自己写的副作用日志。
pub const JOURNAL_CHANNEL: &str = "hidden:c-effect-journal";

/// 一次 CAS 重试的上限（并发写的正确实现用它收敛；不计费、不随机）。
pub const MAX_CAS_RETRIES: usize = 2;
/// 写入意图的标识：重启重放的幂等键是 `(任务, 这个标识)`。
pub const INTENT_ID: &str = "w1";

/// 宿主在隔离实验环境里提供的钩子点（§7.2 第 3 条的「测量位置」只能从这里选）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookPoint {
    /// 写入边界的事件记录器：每次经它提交写一条事件。
    WriteBoundary,
    /// 可控暂停点：到达 / 恢复 / 请求取消 / 收到确认各一条轨迹事件。
    PreWritePause,
    /// 提交计数器：环境暴露的真实提交次数（只有计数，没有内容）。
    CommitCounter,
}

impl HookPoint {
    /// 全部钩子点，按固定顺序。
    pub const ALL: [Self; 3] = [
        Self::WriteBoundary,
        Self::PreWritePause,
        Self::CommitCounter,
    ];

    /// 契约里写的钩子点标识（`measurement.site` 只许引用这些词）。
    pub fn id(self) -> &'static str {
        match self {
            Self::WriteBoundary => "write_boundary",
            Self::PreWritePause => "pre_write_pause",
            Self::CommitCounter => "commit_counter",
        }
    }

    /// 这个钩子点能看到什么、看不到什么（生成契约的提示词与报告共用一处口径）。
    pub fn description(self) -> &'static str {
        match self {
            Self::WriteBoundary => {
                "写入边界的事件记录器：记录每次经该边界提交的任务、键、值与顺序号；\
                 程序若走另一条提交路径或抹掉自己的事件，它看不到"
            }
            Self::PreWritePause => {
                "可控暂停点：记录到达暂停点、恢复执行、请求取消、收到确认四类事件及其顺序号"
            }
            Self::CommitCounter => {
                "提交计数器：只给出真实提交的条数与顺序号，不给出任务、键与内容\
                 （能发现记录器漏记，不能指出漏的是哪一次）"
            }
        }
    }

    /// 从契约文本里认出钩子点标识。
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|point| text.split_whitespace().any(|word| word == point.id()))
    }
}

impl fmt::Display for HookPoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

/// 任务族 C 的三个故障族。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CFamily {
    /// 标志性任务（§7.3）：取消确认之后不得再写入。
    CancelAfterConfirm,
    /// 并发交错下的丢失更新。
    LostUpdate,
    /// 重启重放造成的重复写入。
    DuplicateAfterRestart,
}

impl CFamily {
    /// 三个族，按固定顺序（标志性任务在前）。
    pub const ALL: [Self; 3] = [
        Self::CancelAfterConfirm,
        Self::LostUpdate,
        Self::DuplicateAfterRestart,
    ];

    /// 稳定标识（报告、文件名、契约字段共用）。
    pub fn id(self) -> &'static str {
        match self {
            Self::CancelAfterConfirm => "cancel_after_confirm",
            Self::LostUpdate => "lost_update",
            Self::DuplicateAfterRestart => "duplicate_after_restart",
        }
    }

    /// 一句话标题。
    pub fn title(self) -> &'static str {
        match self {
            Self::CancelAfterConfirm => "取消确认之后不应继续产生缓存写入（§7.3 标志性任务）",
            Self::LostUpdate => "并发交错下不应丢失已提交的更新",
            Self::DuplicateAfterRestart => "重启重放不应把同一写入意图提交两次",
        }
    }

    /// 对应的原始要求及来源（§7.2 第 1 条；措辞即 §7.3 的原文）。
    pub fn requirement(self) -> &'static str {
        match self {
            Self::CancelAfterConfirm => "指定任务取消确认后，不应继续产生缓存写入",
            Self::LostUpdate => "并发交错的写入不应丢掉任何已提交的更新",
            Self::DuplicateAfterRestart => "重启之后重放的写入不得把同一写入意图提交两次",
        }
    }

    /// 原始要求的来源。
    pub fn requirement_source(self) -> &'static str {
        match self {
            Self::CancelAfterConfirm => "用户要求（研究目标 §7.3 的标志性实验）",
            Self::LostUpdate => "验收判据（路线图 §6.3 任务族 C：并发/状态故障）",
            Self::DuplicateAfterRestart => "验收判据（路线图 §6.3 任务族 C：并发/状态故障）",
        }
    }

    /// 观测证据描述的对象标识（§7.2 第 3 条；同时是失效传播的挂点）。
    pub fn object(self) -> ObjectId {
        ObjectId::from(format!("cache.writes.{}", self.id()))
    }

    /// 被测命题覆盖不到的相邻情形（§7.2 第 8 条；写进契约的兜底口径）。
    pub fn uncovered(self) -> &'static str {
        match self {
            Self::CancelAfterConfirm => {
                "跨进程重启后的写入顺序、写入内容的正确性、取消请求本身是否及时送达"
            }
            Self::LostUpdate => "跨进程的分布式锁行为、崩溃恢复期间的写入、写入内容的业务正确性",
            Self::DuplicateAfterRestart => {
                "进程外部的持久化重放、网络重试造成的重复请求、写入内容是否正确"
            }
        }
    }

    /// 这条要求的顺序语义：三族都是时序命题，不能用单点观测代替。
    pub fn timing_note(self) -> &'static str {
        match self {
            Self::CancelAfterConfirm => "确认事件与写入事件之间的先后",
            Self::LostUpdate => "两次提交所依据的基准版本之间的关系",
            Self::DuplicateAfterRestart => "同一写入意图在日志里出现的次数",
        }
    }

    /// 命题里必须出现这一类词，才算讲到了这一族的顺序关系（契约核对按它查）。
    ///
    /// 这是**词面**核对，不是语义证明：它拦得住把要求换成「最终缓存为空」这种明显讲不出顺序的
    /// 表述，拦不住措辞相近但含义偏了的表述——后者由四路独立检验与实际检出率兜。
    pub fn ordering_tokens(self) -> &'static [&'static str] {
        match self {
            Self::CancelAfterConfirm => &["确认", "之后", "先后"],
            Self::LostUpdate => &["基准", "版本", "交错", "先后"],
            Self::DuplicateAfterRestart => &["重放", "重启", "重复", "两次", "次数"],
        }
    }
}

impl fmt::Display for CFamily {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

/// 可控程序的一条指令。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    /// 起一个任务。
    Begin {
        /// 任务标识。
        task: String,
    },
    /// 读一个键，记下当时看到的版本（读-改-写的第一步）。
    Read {
        /// 任务标识。
        task: String,
        /// 键。
        key: String,
    },
    /// 挂起一次写入意图（尚未提交）：暂停点之前「正要写」的那一步。
    Prepare {
        /// 任务标识。
        task: String,
        /// 键。
        key: String,
        /// 值。
        value: String,
    },
    /// 显式提交一次写入。
    Write {
        /// 任务标识。
        task: String,
        /// 键。
        key: String,
        /// 值。
        value: String,
    },
    /// 读-改-写一次：把键的当前数值加一后写回。
    Increment {
        /// 任务标识。
        task: String,
        /// 键。
        key: String,
    },
    /// 幂等重写：把键的当前值原样再写一次（内容不变，但提交序列上多一次）。
    Rewrite {
        /// 任务标识。
        task: String,
        /// 键。
        key: String,
    },
    /// 经批量提交路径写入（v2 新增的第二个落地点）。
    Flush {
        /// 任务标识。
        task: String,
        /// 键。
        key: String,
        /// 值。
        value: String,
    },
    /// 到达写入前的暂停点。
    Pause {
        /// 任务标识。
        task: String,
    },
    /// 请求取消。
    RequestCancel {
        /// 任务标识。
        task: String,
    },
    /// 收到取消确认。
    ConfirmCancel {
        /// 任务标识。
        task: String,
    },
    /// 恢复执行：把挂起的写入意图提交出去。
    Resume {
        /// 任务标识。
        task: String,
    },
    /// 重启：丢掉进程内的确认状态（持久化意图还在，恢复时会被重放）。
    Restart {
        /// 任务标识。
        task: String,
    },
    /// 失效一个键（取消之后的资源回收；不是写入，进日志但不算提交）。
    Evict {
        /// 任务标识。
        task: String,
        /// 键。
        key: String,
    },
    /// 结束任务。
    End {
        /// 任务标识。
        task: String,
    },
}

/// 一条确定性调度：一队按序执行的指令。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Schedule {
    /// 调度标识（`c1-s01` 这类）。
    pub id: String,
    /// 所属族。
    pub family: CFamily,
    /// 这条调度想覆盖什么情形。
    pub note: String,
    /// 指令序列。
    pub ops: Vec<Op>,
}

/// 被测程序的实现（阳性对照与已知错误实现）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Implementation {
    /// 合理实现：三族的顺序约束都被守住。
    Correct,
    /// 已知错误实现：不做取消门、不做版本检查、不做幂等账，照写不误，自报真实。
    Uncoordinated,
    /// 已知错误实现且**绕过记录器**：走批量提交路径，并自报「没有写入」。
    QuietBypass,
    /// 已知错误实现且**篡改日志**：正常写入，事后把自己在记录器里的事件抹掉。
    TamperedRecorder,
}

impl Implementation {
    /// 四路实现：一路合理、三路已知错误（其中两路专门针对测量装置）。
    pub const ALL: [Self; 4] = [
        Self::Correct,
        Self::Uncoordinated,
        Self::QuietBypass,
        Self::TamperedRecorder,
    ];

    /// 稳定标识。
    pub fn id(self) -> &'static str {
        match self {
            Self::Correct => "correct",
            Self::Uncoordinated => "uncoordinated",
            Self::QuietBypass => "quiet_bypass",
            Self::TamperedRecorder => "tampered_recorder",
        }
    }

    /// 一句话说明它是什么实现。
    pub fn note(self) -> &'static str {
        match self {
            Self::Correct => "合理实现：取消门 / 版本检查 / 幂等账都在，自报与事实一致",
            Self::Uncoordinated => "已知错误实现：没有顺序约束，照写不误，自报真实",
            Self::QuietBypass => {
                "已知错误实现且绕过记录器：写入走直接提交路径，声明的那条测量位置上看不到它——\
                 只有独立证据通道能发现"
            }
            Self::TamperedRecorder => {
                "已知错误实现且篡改日志：正常写入，事后抹掉自己在记录器里的事件"
            }
        }
    }

    /// 这个实现是否守住顺序约束（期望它不产生违反）。
    pub fn is_expected_clean(self) -> bool {
        matches!(self, Self::Correct)
    }
}

/// 程序版本：v2 新增批量提交路径（同一个语义边界的第二个落地点）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgramVersion {
    /// 只有一条写入边界。
    V1,
    /// 新增批量提交路径。
    V2,
}

impl ProgramVersion {
    /// 两个版本，按顺序。
    pub const ALL: [Self; 2] = [Self::V1, Self::V2];

    /// 版本标识（写进契约的 `support.versions`）。
    pub fn id(self) -> &'static str {
        match self {
            Self::V1 => "c-program-1.0.0",
            Self::V2 => "c-program-2.0.0",
        }
    }

    /// 版本一句话说明。
    pub fn note(self) -> &'static str {
        match self {
            Self::V1 => "单写入边界：批量提交路径还不存在，Flush 落在同一条边界上",
            Self::V2 => "新增批量提交路径 commit_batch：同一个语义边界下的第二个落地点",
        }
    }
}

/// 写入走的是哪一条提交路径。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WritePath {
    /// 写入边界：记录器挂在它上面，@7.2 第 3 条声明的测量位置就是这里。
    Boundary,
    /// 直接提交：同一边界之下的第二条路径（v2 的批量提交 `commit_batch` 走这条）。
    ///
    /// 它**不在**声明的观测范围里——除非宿主在版本变更之后把它也挂上（[`Attachment::reattached`]）。
    /// 「绕过记录器」这一类已知错误实现（[`Implementation::QuietBypass`]）走的正是这条路。
    Direct,
}

/// 存储日志的一条：真实副作用的权威记录，由存储自己写。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalEntry {
    /// 全局事件序号（与记录器、暂停点轨迹共用同一个计数器，先后可比）。
    pub event: u64,
    /// 存储自己的提交序号（只有提交才增加）。
    pub seq: u64,
    /// 这条是什么：提交还是失效。
    pub kind: JournalKind,
    /// 任务标识。
    pub task: String,
    /// 键。
    pub key: String,
    /// 写入意图标识（幂等键的一半）。
    pub op: String,
    /// 写入的值（失效为空串）。
    pub value: String,
    /// 提交时声明的基准版本。
    pub expected_version: u64,
    /// 提交之后的版本。
    pub version: u64,
    /// 走的哪条提交路径。
    pub path: WritePath,
}

/// 存储日志里的一条属于哪一类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JournalKind {
    /// 一次提交（写入）。
    Commit,
    /// 一次失效（删除键，不是写入）。
    Evict,
}

/// 记录器写下的一条事件（写入边界钩子）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecorderEvent {
    /// 全局事件序号。
    pub event: u64,
    /// 存储的提交序号。
    pub seq: u64,
    /// 任务标识。
    pub task: String,
    /// 键。
    pub key: String,
    /// 写入意图标识。
    pub op: String,
    /// 值。
    pub value: String,
    /// 提交时声明的基准版本（观测器据此判断「这条是在什么基础上写的」）。
    pub expected_version: u64,
    /// 提交之后的版本。
    pub version: u64,
    /// 走的哪条提交路径。
    pub path: WritePath,
}

/// 暂停点轨迹的一类事件。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PauseKind {
    /// 到达写入前的暂停点。
    Arrived,
    /// 恢复执行。
    Resumed,
    /// 请求取消。
    CancelRequested,
    /// 收到取消确认。
    Cancelled,
}

impl PauseKind {
    /// 事件名（报告与判据共用）。
    pub fn label(self) -> &'static str {
        match self {
            Self::Arrived => "到达暂停点",
            Self::Resumed => "恢复执行",
            Self::CancelRequested => "请求取消",
            Self::Cancelled => "收到确认",
        }
    }
}

/// 暂停点轨迹的一条事件（宿主控制面写，程序碰不到）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PauseEvent {
    /// 全局事件序号。
    pub event: u64,
    /// 任务标识。
    pub task: String,
    /// 事件类型。
    pub kind: PauseKind,
}

/// 记录器挂在哪些钩子点上、直接提交路径是否也被覆盖。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    /// 记录器挂载的钩子点。
    pub points: Vec<HookPoint>,
    /// 记录器是否也覆盖了直接提交路径（版本变更之后**重挂** = true）。
    pub direct_covered: bool,
}

impl Attachment {
    /// 按声明挂载：三个钩子点都在，直接提交路径不在观测范围里。
    ///
    /// 这不是「挂错了」——声明里说的测量位置就是写入边界，直接提交路径本来就不在边界上。
    pub fn at_boundary() -> Self {
        Self {
            points: HookPoint::ALL.to_vec(),
            direct_covered: false,
        }
    }

    /// 重挂：宿主代码改了（v2 多了直接提交路径）之后把它也纳入观测。
    pub fn reattached() -> Self {
        Self {
            points: HookPoint::ALL.to_vec(),
            direct_covered: true,
        }
    }

    /// 记录器是否挂在某个钩子点上。
    pub fn covers(&self, point: HookPoint) -> bool {
        self.points.contains(&point)
    }
}

/// 隔离实验环境与其中的可控程序。
#[derive(Debug, Clone)]
pub struct Environment {
    family: CFamily,
    version: ProgramVersion,
    implementation: Implementation,
    attachment: Attachment,
    store: CacheStore,
    recorder: RecorderLog,
    pause: PauseTrace,
    program: Program,
    event: u64,
}

impl Environment {
    /// 起一个环境：装好存储、记录器与暂停点轨迹。
    pub fn new(
        family: CFamily,
        version: ProgramVersion,
        implementation: Implementation,
        attachment: Attachment,
    ) -> Self {
        Self {
            family,
            version,
            implementation,
            attachment,
            store: CacheStore::default(),
            recorder: RecorderLog::default(),
            pause: PauseTrace::default(),
            program: Program::new(),
            event: 0,
        }
    }

    /// 执行一条指令。
    pub fn step(&mut self, op: &Op) {
        let event = self.next_event();
        match op {
            Op::Begin { task } => {
                self.program.tasks.entry(task.clone()).or_default();
            }
            Op::Read { task, key } => {
                let mark = ReadMark {
                    version: self.store.version_of(key),
                    value: self.store.value_of(key).unwrap_or_else(|| "0".to_owned()),
                };
                self.program.task_mut(task).reads.insert(key.clone(), mark);
            }
            Op::Prepare { task, key, value } => {
                let intent = Intent {
                    task: task.clone(),
                    key: key.clone(),
                    value: value.clone(),
                    op: INTENT_ID.to_owned(),
                };
                self.program.task_mut(task).intent = Some(intent);
                self.program.task_mut(task).intents = 1;
            }
            Op::Write { task, key, value } => {
                self.commit(task, key, value, event, WritePath::Boundary);
            }
            Op::Increment { task, key } => {
                // 读-改-写：加的是**自己那份读**里的值，不是存储的当前值——这正是丢失更新发生的地方。
                let base = self
                    .program
                    .tasks
                    .get(task)
                    .and_then(|runtime| runtime.reads.get(key).cloned())
                    .map(|mark| mark.value)
                    .or_else(|| self.store.value_of(key))
                    .unwrap_or_else(|| "0".to_owned());
                let next = base
                    .parse::<i64>()
                    .map(|value| (value + 1).to_string())
                    .unwrap_or(base);
                self.commit(task, key, &next, event, WritePath::Boundary);
            }
            Op::Rewrite { task, key } => {
                let current = self.store.value_of(key).unwrap_or_default();
                self.commit(task, key, &current, event, WritePath::Boundary);
            }
            Op::Flush { task, key, value } => {
                let path = match self.version {
                    ProgramVersion::V1 => WritePath::Boundary,
                    ProgramVersion::V2 => WritePath::Direct,
                };
                self.commit(task, key, value, event, path);
            }
            Op::Pause { task } => self.pause_event(task, PauseKind::Arrived, event),
            Op::RequestCancel { task } => {
                self.program.task_mut(task).cancel_requested = true;
                self.pause_event(task, PauseKind::CancelRequested, event);
            }
            Op::ConfirmCancel { task } => {
                self.program.task_mut(task).cancel_confirmed = true;
                self.pause_event(task, PauseKind::Cancelled, event);
            }
            Op::Resume { task } => {
                self.pause_event(task, PauseKind::Resumed, event);
                let intent = self.program.task_mut(task).intent.clone();
                if let Some(intent) = intent {
                    self.commit(
                        &intent.task,
                        &intent.key,
                        &intent.value,
                        event,
                        WritePath::Boundary,
                    );
                }
            }
            Op::Restart { task } => {
                self.program.task_mut(task).restarts += 1;
            }
            Op::Evict { task, key } => self.evict(task, key, event),
            Op::End { task } => {
                let runtime = self.program.task_mut(task);
                runtime.ended = true;
                runtime.status = if runtime.cancel_confirmed {
                    "cancelled".to_owned()
                } else {
                    "done".to_owned()
                };
            }
        }
    }

    /// 宿主主动注入一次确认之后的写入（§7.2 第 6 条的「可注入的已知故障」）。
    ///
    /// 走的是带记录器的边界，因此观测器**应当**检出它；这是排除「观测器根本没在工作」的那一路。
    pub fn inject_write(&mut self, task: &str, key: &str, value: &str) {
        let event = self.next_event();
        self.program.tasks.entry(task.to_owned()).or_default();
        let version = self.store.version_of(key);
        let commit = self.store.commit(
            task,
            key,
            value,
            INTENT_ID,
            version,
            WritePath::Boundary,
            event,
        );
        self.recorder.push(&commit, &self.attachment);
    }

    /// 按族注入一次已知故障（走带记录器的边界，观测器应当检出）。
    ///
    /// - `cancel_after_confirm`：确认之后的一次写入；
    /// - `lost_update`：拿一个已经被用过的基准版本再提交一次（覆盖了别人已提交的更新）；
    /// - `duplicate_after_restart`：把已经提交过的 `(任务, 意图)` 再提交一次。
    ///
    /// 返回这次注入的说明（写进检验记录的 `note`）。
    pub fn inject_fault(&mut self, family: CFamily) -> String {
        match family {
            CFamily::CancelAfterConfirm => {
                let task = "T1";
                let key = task_key(task);
                self.inject_write(task, &key, "injected-after-confirm");
                format!("在确认之后向 {key} 注入一次写入")
            }
            CFamily::LostUpdate => {
                let base = self.store.journal().iter().find_map(|entry| {
                    (entry.kind == JournalKind::Commit && entry.key == COUNTER_KEY)
                        .then_some(entry.expected_version)
                });
                let base = base.unwrap_or(0);
                let event = self.next_event();
                let commit = self.store.commit(
                    "Z",
                    COUNTER_KEY,
                    "injected",
                    INTENT_ID,
                    base,
                    WritePath::Boundary,
                    event,
                );
                self.recorder.push(&commit, &self.attachment);
                format!("拿已经被用过的基准版本 {base} 再提交一次（覆盖别人已提交的更新）")
            }
            CFamily::DuplicateAfterRestart => {
                let task = "T1";
                let key = job_key(task);
                let event = self.next_event();
                let commit = self.store.commit(
                    task,
                    &key,
                    "injected",
                    INTENT_ID,
                    self.store.version_of(&key),
                    WritePath::Boundary,
                    event,
                );
                self.recorder.push(&commit, &self.attachment);
                format!("把已经提交过的写入意图 {INTENT_ID} 再提交一次")
            }
        }
    }

    /// 把当前状态收成一条可落盘的运行记录（臂与隐藏检查都在它上面算）。
    pub fn record(&self, schedule: &Schedule) -> RunRecord {
        RunRecord {
            family: self.family,
            schedule: schedule.id.clone(),
            schedule_note: schedule.note.clone(),
            implementation: self.implementation,
            version: self.version,
            attachment: self.attachment.clone(),
            observables: self.observables(),
            recorder: self.recorder.events().to_vec(),
            pause: self.pause.events().to_vec(),
            journal: self.store.journal().to_vec(),
            commit_seq: self.store.commit_seq(),
            hidden: hidden_check(self.family, &self.store, &self.pause),
        }
    }

    /// 跑完一条调度。
    pub fn run(&mut self, schedule: &Schedule) {
        for op in &schedule.ops {
            self.step(op);
        }
    }

    /// 记录器读到的事件。
    pub fn recorder(&self) -> &RecorderLog {
        &self.recorder
    }

    /// 暂停点轨迹。
    pub fn pause(&self) -> &PauseTrace {
        &self.pause
    }

    /// 存储（日志与最终状态都在里面）。
    pub fn store(&self) -> &CacheStore {
        &self.store
    }

    /// 程序自报的、在族 C 之前就存在的可观察量（固定臂与通用测试生成臂只能看这些）。
    pub fn observables(&self) -> Observables {
        Observables {
            statuses: self
                .program
                .tasks
                .iter()
                .map(|(task, runtime)| (task.clone(), runtime.status.clone()))
                .collect(),
            intents: self
                .program
                .tasks
                .iter()
                .map(|(task, runtime)| (task.clone(), runtime.intents))
                .collect(),
            values: self.store.state.clone(),
            versions: self.store.versions.clone(),
        }
    }

    /// 程序版本。
    pub fn version(&self) -> ProgramVersion {
        self.version
    }

    /// 当前记录器挂载。
    pub fn attachment(&self) -> &Attachment {
        &self.attachment
    }

    fn next_event(&mut self) -> u64 {
        self.event += 1;
        self.event
    }

    fn pause_event(&mut self, task: &str, kind: PauseKind, event: u64) {
        self.pause.events.push(PauseEvent {
            event,
            task: task.to_owned(),
            kind,
        });
    }

    fn evict(&mut self, task: &str, key: &str, event: u64) {
        self.store.evict(task, key, event);
    }

    /// 提交一次写入，并按实现与版本决定它是怎么落地的。
    fn commit(&mut self, task: &str, key: &str, value: &str, event: u64, path: WritePath) {
        let implementation = self.implementation;
        let path = match implementation {
            Implementation::QuietBypass => WritePath::Direct,
            _ => path,
        };
        // 顺序约束的门：只有合理实现守着它。三族各自的门都只有这一处分支。
        let mut attempt = 0;
        loop {
            let expected = self.expected_version(task, key);
            let permitted = self.permits(task, key, expected);
            if !permitted {
                if self.family == CFamily::LostUpdate
                    && implementation == Implementation::Correct
                    && attempt < MAX_CAS_RETRIES
                {
                    // 基准版本已过期：重读一次再试（并发写的正确收尾）。
                    attempt += 1;
                    let mark = ReadMark {
                        version: self.store.version_of(key),
                        value: self.store.value_of(key).unwrap_or_else(|| "0".to_owned()),
                    };
                    self.program
                        .task_mut(task)
                        .reads
                        .insert(key.to_owned(), mark);
                    continue;
                }
                return;
            }
            let commit = self
                .store
                .commit(task, key, value, INTENT_ID, expected, path, event);
            self.recorder.push(&commit, &self.attachment);
            if implementation == Implementation::TamperedRecorder {
                // 篡改日志：写完再把自己那一条从记录器里抹掉（存储日志里抹不掉）。
                self.recorder.erase_last();
            }
            return;
        }
    }

    /// 这次提交声明的基准版本。
    fn expected_version(&self, task: &str, key: &str) -> u64 {
        self.program
            .tasks
            .get(task)
            .and_then(|runtime| runtime.reads.get(key))
            .map(|mark| mark.version)
            .unwrap_or_else(|| self.store.version_of(key))
    }

    /// 这个实现允不允许这次提交（三族的顺序约束各一处分支）。
    fn permits(&self, task: &str, key: &str, expected: u64) -> bool {
        if self.implementation != Implementation::Correct {
            return true;
        }
        match self.family {
            CFamily::CancelAfterConfirm => {
                let runtime = self.program.tasks.get(task);
                !runtime.is_some_and(|runtime| runtime.cancel_requested || runtime.cancel_confirmed)
            }
            CFamily::LostUpdate => self.store.version_of(key) == expected,
            CFamily::DuplicateAfterRestart => !self.store.committed(task, INTENT_ID),
        }
    }
}

/// 程序自报的可观察量（族 C 之前就存在的接口）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observables {
    /// 任务 → 自报状态（`done` / `cancelled`）。
    pub statuses: BTreeMap<String, String>,
    /// 任务 → 自报的写入意图数。
    pub intents: BTreeMap<String, u32>,
    /// 键 → 当前值。
    pub values: BTreeMap<String, String>,
    /// 键 → 当前版本。
    pub versions: BTreeMap<String, u64>,
}

impl Observables {
    /// 任务的自报状态。
    pub fn status(&self, task: &str) -> Option<&str> {
        self.statuses.get(task).map(String::as_str)
    }

    /// 任务自报的写入意图数。
    pub fn intents(&self, task: &str) -> Option<u32> {
        self.intents.get(task).copied()
    }

    /// 键的当前值。
    pub fn value(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    /// 键的当前版本。
    pub fn version(&self, key: &str) -> u64 {
        self.versions.get(key).copied().unwrap_or(0)
    }
}

/// 程序内部的一个写入意图。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Intent {
    task: String,
    key: String,
    value: String,
    op: String,
}

/// 一次读留下的印记：当时看到的版本与值（读-改-写与版本检查都靠它）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct ReadMark {
    version: u64,
    value: String,
}

/// 一个任务的运行态。
#[derive(Debug, Clone, Default)]
struct TaskRuntime {
    status: String,
    cancel_requested: bool,
    cancel_confirmed: bool,
    reads: BTreeMap<String, ReadMark>,
    intent: Option<Intent>,
    intents: u32,
    restarts: u32,
    ended: bool,
}

/// 可控程序：解释指令，按实现选择守不守顺序约束。
#[derive(Debug, Clone)]
struct Program {
    tasks: BTreeMap<String, TaskRuntime>,
}

impl Program {
    fn new() -> Self {
        Self {
            tasks: BTreeMap::new(),
        }
    }

    fn task_mut(&mut self, task: &str) -> &mut TaskRuntime {
        self.tasks.entry(task.to_owned()).or_default()
    }
}

/// 存储：真实副作用的权威记录（隐藏检查的数据源）。
#[derive(Debug, Clone, Default)]
pub struct CacheStore {
    state: BTreeMap<String, String>,
    versions: BTreeMap<String, u64>,
    journal: Vec<JournalEntry>,
    seq: u64,
}

impl CacheStore {
    /// 存储自己写下的副作用日志（只有提交与失效，没有程序自报）。
    pub fn journal(&self) -> &[JournalEntry] {
        &self.journal
    }

    /// 当前值。
    pub fn value_of(&self, key: &str) -> Option<String> {
        self.state.get(key).cloned()
    }

    /// 当前版本。
    pub fn version_of(&self, key: &str) -> u64 {
        self.versions.get(key).copied().unwrap_or(0)
    }

    /// 提交序号：真实**提交**的条数（失效不计）。
    pub fn commit_seq(&self) -> u64 {
        self.seq
    }

    /// 某个 `(任务, 意图)` 是否已经提交过（幂等账只认日志）。
    pub fn committed(&self, task: &str, op: &str) -> bool {
        self.journal
            .iter()
            .any(|entry| entry.kind == JournalKind::Commit && entry.task == task && entry.op == op)
    }

    #[allow(clippy::too_many_arguments)]
    fn commit(
        &mut self,
        task: &str,
        key: &str,
        value: &str,
        op: &str,
        expected_version: u64,
        path: WritePath,
        event: u64,
    ) -> JournalEntry {
        self.seq += 1;
        let version = self.version_of(key) + 1;
        self.state.insert(key.to_owned(), value.to_owned());
        self.versions.insert(key.to_owned(), version);
        let entry = JournalEntry {
            event,
            seq: self.seq,
            kind: JournalKind::Commit,
            task: task.to_owned(),
            key: key.to_owned(),
            op: op.to_owned(),
            value: value.to_owned(),
            expected_version,
            version,
            path,
        };
        self.journal.push(entry.clone());
        entry
    }

    fn evict(&mut self, task: &str, key: &str, event: u64) {
        // 失效不是提交：提交序号不动（提交计数器钩子数的就是提交条数，不能把它算进去）。
        let version = self.version_of(key) + 1;
        self.state.remove(key);
        self.versions.insert(key.to_owned(), version);
        self.journal.push(JournalEntry {
            event,
            seq: self.seq,
            kind: JournalKind::Evict,
            task: task.to_owned(),
            key: key.to_owned(),
            op: String::new(),
            value: String::new(),
            expected_version: version,
            version,
            path: WritePath::Boundary,
        });
    }
}

/// 写入边界的事件记录器（观测器的数据源；可被绕过或篡改）。
#[derive(Debug, Clone, Default)]
pub struct RecorderLog {
    events: Vec<RecorderEvent>,
}

impl RecorderLog {
    /// 记录到的事件，按发生顺序。
    pub fn events(&self) -> &[RecorderEvent] {
        &self.events
    }

    fn push(&mut self, entry: &JournalEntry, attachment: &Attachment) {
        if !attachment.covers(HookPoint::WriteBoundary) {
            return;
        }
        if entry.path == WritePath::Direct && !attachment.direct_covered {
            return;
        }
        self.events.push(RecorderEvent {
            event: entry.event,
            seq: entry.seq,
            task: entry.task.clone(),
            key: entry.key.clone(),
            op: entry.op.clone(),
            value: entry.value.clone(),
            expected_version: entry.expected_version,
            version: entry.version,
            path: entry.path,
        });
    }

    fn erase_last(&mut self) {
        self.events.pop();
    }
}

/// 可控暂停点的轨迹（宿主控制面写）。
#[derive(Debug, Clone, Default)]
pub struct PauseTrace {
    events: Vec<PauseEvent>,
}

impl PauseTrace {
    /// 轨迹事件，按发生顺序。
    pub fn events(&self) -> &[PauseEvent] {
        &self.events
    }

    /// 任务收到取消确认时的事件序号。
    pub fn confirmed_at(&self, task: &str) -> Option<u64> {
        self.events
            .iter()
            .find(|event| event.task == task && event.kind == PauseKind::Cancelled)
            .map(|event| event.event)
    }
}

/// 隐藏检查的读数：判定 + 它据以判定的那条日志证据。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HiddenReading {
    /// 判定（只可能是「命题成立」或「发现违反」，读不到数据时才失败——这里的数据源不可绕过）。
    pub verdict: ObserverVerdict,
    /// 证据：违反时给出具体那一条日志，成立时给出核对过的条数。
    pub detail: String,
}

/// 一次运行（一条调度 × 一个实现 × 一个版本）留下的全部读数。
///
/// 三条通道在这里**分字段**存着：记录器 [`RunRecord::recorder`] 给观测器，暂停点轨迹
/// [`RunRecord::pause`] 是控制面事实，存储日志 [`RunRecord::journal`] 只给隐藏检查
/// （[`RunRecord::hidden`] 就是它的判定）。哪些字段属于哪条通道，看一眼就分得出来。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunRecord {
    /// 所属故障族。
    pub family: CFamily,
    /// 调度标识。
    pub schedule: String,
    /// 这条调度覆盖的情形。
    pub schedule_note: String,
    /// 被测实现。
    pub implementation: Implementation,
    /// 程序版本。
    pub version: ProgramVersion,
    /// 记录器当时的挂载。
    pub attachment: Attachment,
    /// 执行完之后的自报可观察量。
    pub observables: Observables,
    /// 记录器通道：写入边界的事件。
    pub recorder: Vec<RecorderEvent>,
    /// 控制面通道：暂停点轨迹。
    pub pause: Vec<PauseEvent>,
    /// 隐藏通道：存储自己的副作用日志。
    pub journal: Vec<JournalEntry>,
    /// 存储的真实提交条数（提交计数器钩子的读数）。
    pub commit_seq: u64,
    /// 隐藏检查的判定（只在评分与独立检验里用，观测器看不到）。
    pub hidden: HiddenReading,
}

impl RunRecord {
    /// 记录器记下的提交条数。
    pub fn recorded_commits(&self) -> usize {
        self.recorder.len()
    }

    /// 任务收到取消确认时的事件序号。
    pub fn confirmed_at(&self, task: &str) -> Option<u64> {
        self.pause
            .iter()
            .find(|event| event.task == task && event.kind == PauseKind::Cancelled)
            .map(|event| event.event)
    }
}

/// 跑一个用例并收成一条运行记录。
pub fn run_case(
    family: CFamily,
    schedule: &Schedule,
    implementation: Implementation,
    version: ProgramVersion,
    attachment: Attachment,
    inject: bool,
) -> RunRecord {
    let mut environment = Environment::new(family, version, implementation, attachment);
    environment.run(schedule);
    if inject {
        environment.inject_fault(family);
    }
    environment.record(schedule)
}

/// 隐藏检查：只读存储日志与最终状态，不看记录器、不看程序自报（§7.3 末段）。
///
/// 三族各自的违反判定都只从存储日志算得：
///
/// - `cancel_after_confirm`：确认事件序号之后仍有该任务的提交；
/// - `lost_update`：同一键上有两条提交声明的基准版本相同（后一条覆盖了前一条已提交的更新）；
/// - `duplicate_after_restart`：同一 `(任务, 意图)` 提交了两次以上。
///
/// 它与 [`crate::c_measure`] 的观测器读的不是同一份数据：观测器读记录器（会被绕过、会被篡改），
/// 这里读存储自己写的日志。
pub fn hidden_check(family: CFamily, store: &CacheStore, pause: &PauseTrace) -> HiddenReading {
    let commits: Vec<&JournalEntry> = store
        .journal()
        .iter()
        .filter(|entry| entry.kind == JournalKind::Commit)
        .collect();

    match family {
        CFamily::CancelAfterConfirm => {
            let mut after = Vec::new();
            for entry in &commits {
                if pause
                    .confirmed_at(&entry.task)
                    .is_some_and(|confirmed| entry.event > confirmed)
                {
                    after.push((*entry).clone());
                }
            }
            if after.is_empty() {
                HiddenReading {
                    verdict: ObserverVerdict::Satisfied,
                    detail: format!(
                        "存储日志 {} 条提交，没有一条落在所属任务取消确认之后",
                        commits.len()
                    ),
                }
            } else {
                let first = &after[0];
                HiddenReading {
                    verdict: ObserverVerdict::Violated,
                    detail: format!(
                        "任务 {} 在确认之后仍提交了 {} 次（首次：键 {} 值 {}，提交序号 {}）",
                        first.task,
                        after.len(),
                        first.key,
                        first.value,
                        first.seq
                    ),
                }
            }
        }
        CFamily::LostUpdate => {
            let mut bases: BTreeMap<(String, u64), Vec<&JournalEntry>> = BTreeMap::new();
            for entry in &commits {
                bases
                    .entry((entry.key.clone(), entry.expected_version))
                    .or_default()
                    .push(entry);
            }
            let lost: Vec<(&(String, u64), &Vec<&JournalEntry>)> = bases
                .iter()
                .filter(|(_, entries)| entries.len() > 1)
                .collect();
            if lost.is_empty() {
                HiddenReading {
                    verdict: ObserverVerdict::Satisfied,
                    detail: format!(
                        "存储日志 {} 条提交，每条声明的基准版本互不相同",
                        commits.len()
                    ),
                }
            } else {
                let ((key, base), entries) = lost[0];
                HiddenReading {
                    verdict: ObserverVerdict::Violated,
                    detail: format!(
                        "键 {} 上有 {} 条提交都以基准版本 {} 为依据（提交序号 {}），其中至少一条已提交的更新被覆盖",
                        key,
                        entries.len(),
                        base,
                        entries
                            .iter()
                            .map(|entry| entry.seq.to_string())
                            .collect::<Vec<_>>()
                            .join("、")
                    ),
                }
            }
        }
        CFamily::DuplicateAfterRestart => {
            let mut counted: BTreeMap<(String, String), u32> = BTreeMap::new();
            for entry in &commits {
                *counted
                    .entry((entry.task.clone(), entry.op.clone()))
                    .or_default() += 1;
            }
            let duplicated: Vec<(&(String, String), &u32)> =
                counted.iter().filter(|(_, count)| **count > 1).collect();
            if duplicated.is_empty() {
                HiddenReading {
                    verdict: ObserverVerdict::Satisfied,
                    detail: format!(
                        "存储日志 {} 条提交，每个写入意图只提交了一次",
                        commits.len()
                    ),
                }
            } else {
                let ((task, op), count) = duplicated[0];
                HiddenReading {
                    verdict: ObserverVerdict::Violated,
                    detail: format!(
                        "任务 {} 的写入意图 {} 被提交了 {} 次（重启重放没有做幂等）",
                        task, op, count
                    ),
                }
            }
        }
    }
}

/// 三族各自的调度（每族 10 条；`N=10` 的重复维度就是这里）。
pub fn schedules(family: CFamily) -> Vec<Schedule> {
    match family {
        CFamily::CancelAfterConfirm => cancel_schedules(),
        CFamily::LostUpdate => lost_update_schedules(),
        CFamily::DuplicateAfterRestart => duplicate_schedules(),
    }
}

fn schedule(family: CFamily, index: usize, note: &str, ops: Vec<Op>) -> Schedule {
    Schedule {
        id: format!("{}-s{index:02}", short_id(family)),
        family,
        note: note.to_owned(),
        ops,
    }
}

fn short_id(family: CFamily) -> &'static str {
    match family {
        CFamily::CancelAfterConfirm => "c1",
        CFamily::LostUpdate => "c2",
        CFamily::DuplicateAfterRestart => "c3",
    }
}

/// 标志性任务与幂等重放任务的键前缀（键名口径写在一处）。
pub const TASK_KEY_PREFIX: &str = "task:";
/// 幂等重放任务的键前缀。
pub const JOB_KEY_PREFIX: &str = "job:";

/// 标志性任务的键：被取消的作业正准备写的那个。
pub fn task_key(task: &str) -> String {
    format!("{TASK_KEY_PREFIX}{task}")
}

/// 幂等重放任务的键。
pub fn job_key(task: &str) -> String {
    format!("{JOB_KEY_PREFIX}{task}")
}

/// 这一族里有那些任务（可观察量词表用；也就是调度里出现过的任务）。
pub fn declared_tasks(family: CFamily) -> Vec<String> {
    let mut tasks: Vec<String> = schedules(family)
        .iter()
        .flat_map(|schedule| schedule.ops.iter())
        .map(|op| match op {
            Op::Begin { task }
            | Op::Read { task, .. }
            | Op::Prepare { task, .. }
            | Op::Write { task, .. }
            | Op::Increment { task, .. }
            | Op::Rewrite { task, .. }
            | Op::Flush { task, .. }
            | Op::Pause { task }
            | Op::RequestCancel { task }
            | Op::ConfirmCancel { task }
            | Op::Resume { task }
            | Op::Restart { task }
            | Op::Evict { task, .. }
            | Op::End { task } => task.clone(),
        })
        .collect();
    tasks.sort();
    tasks.dedup();
    tasks
}

/// 这一族里有那些键（可观察量词表用）。
pub fn declared_keys(family: CFamily) -> Vec<String> {
    let mut keys: Vec<String> = schedules(family)
        .iter()
        .flat_map(|schedule| schedule.ops.iter())
        .filter_map(|op| match op {
            Op::Read { key, .. }
            | Op::Prepare { key, .. }
            | Op::Write { key, .. }
            | Op::Increment { key, .. }
            | Op::Rewrite { key, .. }
            | Op::Flush { key, .. }
            | Op::Evict { key, .. } => Some(key.clone()),
            _ => None,
        })
        .collect();
    keys.sort();
    keys.dedup();
    keys
}

/// 并发计数的键。
pub const COUNTER_KEY: &str = "counter";

/// 标志性任务里已经提交的值（暂停之前的那一次合法写入）。
pub const DRAFT_VALUE: &str = "draft";

fn begin(task: &str) -> Op {
    Op::Begin {
        task: task.to_owned(),
    }
}

fn end(task: &str) -> Op {
    Op::End {
        task: task.to_owned(),
    }
}

fn cancel_schedules() -> Vec<Schedule> {
    let task = "T1";
    let key = task_key(task);
    let notes = [
        "确认先于恢复的基本形：恢复时不得再写",
        "暂停点之前已有一次合法写入（同一个键）：终态本来就有值，要看的是顺序",
        "取消输掉竞态：恢复发生在请求取消与确认之前，那一次写入在确认之前",
        "确认之后先恢复、再来一次幂等重写（写回与当前值相同的值）",
        "确认之后写入，随后被合法的资源回收抹掉：终态看不出任何痕迹",
        "恢复与批量提交路径同时出现（v2 新增的第二个落地点）",
        "两个任务交错：另一个任务在确认之前写自己的键，不算违反",
        "恢复之后再做一次显式写入，最后回收",
        "先恢复、后请求取消：确认最晚，恢复那一次写入落在确认之前",
        "确认之后恢复两次",
    ];
    let mut out = Vec::new();
    for (index, note) in notes.iter().enumerate() {
        let ops = match index {
            0 => vec![
                begin(task),
                Op::Prepare {
                    task: task.into(),
                    key: key.clone(),
                    value: DRAFT_VALUE.into(),
                },
                Op::Pause { task: task.into() },
                Op::RequestCancel { task: task.into() },
                Op::ConfirmCancel { task: task.into() },
                Op::Resume { task: task.into() },
                end(task),
            ],
            1 => vec![
                begin(task),
                Op::Write {
                    task: task.into(),
                    key: key.clone(),
                    value: DRAFT_VALUE.into(),
                },
                Op::Prepare {
                    task: task.into(),
                    key: key.clone(),
                    value: DRAFT_VALUE.into(),
                },
                Op::Pause { task: task.into() },
                Op::RequestCancel { task: task.into() },
                Op::ConfirmCancel { task: task.into() },
                Op::Resume { task: task.into() },
                end(task),
            ],
            2 => vec![
                begin(task),
                Op::Prepare {
                    task: task.into(),
                    key: key.clone(),
                    value: DRAFT_VALUE.into(),
                },
                Op::Pause { task: task.into() },
                Op::RequestCancel { task: task.into() },
                Op::Resume { task: task.into() },
                Op::ConfirmCancel { task: task.into() },
                end(task),
            ],
            3 => vec![
                begin(task),
                Op::Write {
                    task: task.into(),
                    key: key.clone(),
                    value: DRAFT_VALUE.into(),
                },
                Op::Prepare {
                    task: task.into(),
                    key: key.clone(),
                    value: DRAFT_VALUE.into(),
                },
                Op::Pause { task: task.into() },
                Op::RequestCancel { task: task.into() },
                Op::ConfirmCancel { task: task.into() },
                Op::Resume { task: task.into() },
                Op::Rewrite {
                    task: task.into(),
                    key: key.clone(),
                },
                end(task),
            ],
            4 => vec![
                begin(task),
                Op::Prepare {
                    task: task.into(),
                    key: key.clone(),
                    value: DRAFT_VALUE.into(),
                },
                Op::Pause { task: task.into() },
                Op::RequestCancel { task: task.into() },
                Op::ConfirmCancel { task: task.into() },
                Op::Resume { task: task.into() },
                Op::Evict {
                    task: task.into(),
                    key: key.clone(),
                },
                end(task),
            ],
            5 => vec![
                begin(task),
                Op::Prepare {
                    task: task.into(),
                    key: key.clone(),
                    value: DRAFT_VALUE.into(),
                },
                Op::Pause { task: task.into() },
                Op::RequestCancel { task: task.into() },
                Op::ConfirmCancel { task: task.into() },
                Op::Flush {
                    task: task.into(),
                    key: key.clone(),
                    value: DRAFT_VALUE.into(),
                },
                Op::Resume { task: task.into() },
                end(task),
            ],
            6 => vec![
                begin(task),
                Op::Prepare {
                    task: task.into(),
                    key: key.clone(),
                    value: DRAFT_VALUE.into(),
                },
                Op::Pause { task: task.into() },
                begin("T2"),
                Op::Write {
                    task: "T2".into(),
                    key: task_key("T2"),
                    value: "done".into(),
                },
                Op::RequestCancel { task: task.into() },
                Op::ConfirmCancel { task: task.into() },
                Op::Resume { task: task.into() },
                end("T2"),
                end(task),
            ],
            7 => vec![
                begin(task),
                Op::Prepare {
                    task: task.into(),
                    key: key.clone(),
                    value: DRAFT_VALUE.into(),
                },
                Op::Pause { task: task.into() },
                Op::RequestCancel { task: task.into() },
                Op::ConfirmCancel { task: task.into() },
                Op::Resume { task: task.into() },
                Op::Write {
                    task: task.into(),
                    key: key.clone(),
                    value: DRAFT_VALUE.into(),
                },
                Op::Evict {
                    task: task.into(),
                    key: key.clone(),
                },
                end(task),
            ],
            8 => vec![
                begin(task),
                Op::Prepare {
                    task: task.into(),
                    key: key.clone(),
                    value: DRAFT_VALUE.into(),
                },
                Op::Pause { task: task.into() },
                Op::Resume { task: task.into() },
                Op::RequestCancel { task: task.into() },
                Op::ConfirmCancel { task: task.into() },
                end(task),
            ],
            _ => vec![
                begin(task),
                Op::Prepare {
                    task: task.into(),
                    key: key.clone(),
                    value: DRAFT_VALUE.into(),
                },
                Op::Pause { task: task.into() },
                Op::RequestCancel { task: task.into() },
                Op::ConfirmCancel { task: task.into() },
                Op::Resume { task: task.into() },
                Op::Resume { task: task.into() },
                end(task),
            ],
        };
        out.push(schedule(CFamily::CancelAfterConfirm, index + 1, note, ops));
    }
    out
}

fn lost_update_schedules() -> Vec<Schedule> {
    let read = |task: &str| Op::Read {
        task: task.into(),
        key: COUNTER_KEY.into(),
    };
    let inc = |task: &str| Op::Increment {
        task: task.into(),
        key: COUNTER_KEY.into(),
    };
    let notes = [
        "两次读都看到同一个基准，随后各写一次（经典交错）",
        "完全串行：A 写完 B 才读",
        "两次读都看到同一个基准，写的顺序反过来",
        "交错之后双方各自再多写一次（陈旧基准被反复使用）",
        "A 的读与写之间夹着 B 的两次读，但 B 的写仍在 A 之后",
        "先起 B 再起 A，读与写仍然交错",
        "先起 B 再起 A，但 B 先写完",
        "交错之后 A 重读再写",
        "三个写者：A、B、C 同时读到同一个基准",
        "同一个任务连续写两次，第二次仍用自己那份陈旧读",
    ];
    let plans: Vec<Vec<Op>> = vec![
        vec![
            begin("A"),
            begin("B"),
            read("A"),
            read("B"),
            inc("A"),
            inc("B"),
            end("B"),
            end("A"),
        ],
        vec![
            begin("A"),
            begin("B"),
            read("A"),
            inc("A"),
            read("B"),
            inc("B"),
            end("B"),
            end("A"),
        ],
        vec![
            begin("A"),
            begin("B"),
            read("A"),
            read("B"),
            inc("B"),
            inc("A"),
            end("B"),
            end("A"),
        ],
        vec![
            begin("A"),
            begin("B"),
            read("A"),
            read("B"),
            inc("A"),
            inc("B"),
            inc("A"),
            inc("B"),
            end("B"),
            end("A"),
        ],
        vec![
            begin("A"),
            begin("B"),
            read("A"),
            inc("A"),
            read("B"),
            read("B"),
            inc("B"),
            end("B"),
            end("A"),
        ],
        vec![
            begin("B"),
            begin("A"),
            read("B"),
            read("A"),
            inc("A"),
            inc("B"),
            end("B"),
            end("A"),
        ],
        vec![
            begin("B"),
            begin("A"),
            read("B"),
            inc("B"),
            read("A"),
            inc("A"),
            end("B"),
            end("A"),
        ],
        vec![
            begin("A"),
            begin("B"),
            read("A"),
            read("B"),
            inc("A"),
            inc("B"),
            read("A"),
            inc("A"),
            end("B"),
            end("A"),
        ],
        vec![
            begin("A"),
            begin("B"),
            begin("C"),
            read("A"),
            read("B"),
            read("C"),
            inc("A"),
            inc("B"),
            inc("C"),
            end("C"),
            end("B"),
            end("A"),
        ],
        vec![
            begin("A"),
            begin("B"),
            read("A"),
            inc("A"),
            inc("A"),
            read("B"),
            inc("B"),
            end("B"),
            end("A"),
        ],
    ];
    plans
        .into_iter()
        .enumerate()
        .map(|(index, plan)| schedule(CFamily::LostUpdate, index + 1, notes[index], plan))
        .collect()
}

fn duplicate_schedules() -> Vec<Schedule> {
    let task = "T1";
    let key = job_key(task);
    let prepare = Op::Prepare {
        task: task.into(),
        key: key.clone(),
        value: "done".into(),
    };
    let write = Op::Write {
        task: task.into(),
        key: key.clone(),
        value: "done".into(),
    };
    let pause = Op::Pause { task: task.into() };
    let resume = Op::Resume { task: task.into() };
    let restart = Op::Restart { task: task.into() };
    let notes = [
        "提交之后重启，恢复时重放同一个意图",
        "重启发生在提交之前：恢复只提交一次",
        "恢复两次",
        "两次重启，每次重启后都恢复",
        "重启之后经批量提交路径再提交一次",
        "没有暂停点的直接提交，之后重启重放",
        "两个任务交错：另一个任务在 T1 重放之前完成自己的写入",
        "重启之后又显式写了一次",
        "重启之后才第一次提交，随后恢复重放",
        "恢复之后再来一次幂等重写",
    ];
    let mut out = Vec::new();
    for (index, note) in notes.iter().enumerate() {
        let ops = match index {
            0 => vec![
                begin(task),
                prepare.clone(),
                pause.clone(),
                write.clone(),
                restart.clone(),
                resume.clone(),
                end(task),
            ],
            1 => vec![
                begin(task),
                prepare.clone(),
                pause.clone(),
                restart.clone(),
                resume.clone(),
                end(task),
            ],
            2 => vec![
                begin(task),
                prepare.clone(),
                pause.clone(),
                write.clone(),
                restart.clone(),
                resume.clone(),
                resume.clone(),
                end(task),
            ],
            3 => vec![
                begin(task),
                prepare.clone(),
                pause.clone(),
                restart.clone(),
                resume.clone(),
                restart.clone(),
                resume.clone(),
                end(task),
            ],
            4 => vec![
                begin(task),
                prepare.clone(),
                pause.clone(),
                write.clone(),
                restart.clone(),
                Op::Flush {
                    task: task.into(),
                    key: key.clone(),
                    value: "done".into(),
                },
                resume.clone(),
                end(task),
            ],
            5 => vec![
                begin(task),
                prepare.clone(),
                write.clone(),
                restart.clone(),
                resume.clone(),
                end(task),
            ],
            6 => vec![
                begin(task),
                prepare.clone(),
                pause.clone(),
                write.clone(),
                restart.clone(),
                begin("T2"),
                prepare.clone(),
                resume.clone(),
                Op::Write {
                    task: "T2".into(),
                    key: job_key("T2"),
                    value: "done".into(),
                },
                end("T2"),
                end(task),
            ],
            7 => vec![
                begin(task),
                prepare.clone(),
                pause.clone(),
                write.clone(),
                restart.clone(),
                write.clone(),
                resume.clone(),
                end(task),
            ],
            8 => vec![
                begin(task),
                prepare.clone(),
                pause.clone(),
                restart.clone(),
                write.clone(),
                resume.clone(),
                end(task),
            ],
            _ => vec![
                begin(task),
                prepare.clone(),
                pause.clone(),
                write.clone(),
                restart.clone(),
                resume.clone(),
                Op::Rewrite {
                    task: task.into(),
                    key: key.clone(),
                },
                end(task),
            ],
        };
        out.push(schedule(
            CFamily::DuplicateAfterRestart,
            index + 1,
            note,
            ops,
        ));
    }
    out
}

/// 固定测试臂要看的键（每族一个；报告里写清楚是哪几个，别人才核得动）。
pub fn fixed_test_subject(family: CFamily) -> (String, String) {
    match family {
        CFamily::CancelAfterConfirm => ("T1".to_owned(), task_key("T1")),
        CFamily::LostUpdate => ("A".to_owned(), COUNTER_KEY.to_owned()),
        CFamily::DuplicateAfterRestart => ("T1".to_owned(), job_key("T1")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(family: CFamily, implementation: Implementation) -> Environment {
        Environment::new(
            family,
            ProgramVersion::V1,
            implementation,
            Attachment::at_boundary(),
        )
    }

    fn run(family: CFamily, implementation: Implementation) -> (Environment, HiddenReading) {
        let schedule = schedules(family).remove(0);
        let mut environment = env(family, implementation);
        environment.run(&schedule);
        let reading = hidden_check(family, environment.store(), environment.pause());
        (environment, reading)
    }

    #[test]
    fn every_family_has_ten_schedules_with_stable_ids() {
        for family in CFamily::ALL {
            let schedules = schedules(family);
            assert_eq!(schedules.len(), 10, "每族 N=10：{family}");
            for (index, schedule) in schedules.iter().enumerate() {
                assert_eq!(
                    schedule.id,
                    format!("{}-s{:02}", short_id(family), index + 1)
                );
                assert_eq!(schedule.family, family);
                assert!(!schedule.note.is_empty());
                assert!(!schedule.ops.is_empty());
            }
        }
    }

    #[test]
    fn the_landmark_schedule_violates_only_for_the_faulty_implementations() {
        let (correct, reading) = run(CFamily::CancelAfterConfirm, Implementation::Correct);
        assert_eq!(reading.verdict, ObserverVerdict::Satisfied);
        assert!(correct.store().journal().is_empty(), "合理实现一次都没写");

        for implementation in [
            Implementation::Uncoordinated,
            Implementation::QuietBypass,
            Implementation::TamperedRecorder,
        ] {
            let (environment, reading) = run(CFamily::CancelAfterConfirm, implementation);
            assert_eq!(
                reading.verdict,
                ObserverVerdict::Violated,
                "{implementation:?} 必须在确认之后写入"
            );
            assert_eq!(
                environment.store().journal().len(),
                1,
                "存储日志记下了那一次真实写入"
            );
            if implementation == Implementation::QuietBypass
                || implementation == Implementation::TamperedRecorder
            {
                assert!(
                    environment.recorder().events().is_empty(),
                    "{implementation:?} 的记录器里不该有东西（这正是它要掩盖的）"
                );
            }
        }
    }

    #[test]
    fn the_hidden_check_sees_what_a_state_only_check_cannot() {
        // 确认之后写入，随后被合法的资源回收抹掉：终态为空，但日志里有一次确认之后的提交。
        let schedule = schedules(CFamily::CancelAfterConfirm)
            .into_iter()
            .find(|schedule| schedule.id == "c1-s05")
            .expect("第五条调度在");
        let mut environment = env(CFamily::CancelAfterConfirm, Implementation::Uncoordinated);
        environment.run(&schedule);

        let observables = environment.observables();
        assert_eq!(observables.value(&task_key("T1")), None, "终态里看不出痕迹");
        assert_eq!(
            hidden_check(
                CFamily::CancelAfterConfirm,
                environment.store(),
                environment.pause()
            )
            .verdict,
            ObserverVerdict::Violated,
            "存储日志里看得见确认之后的那一次写入"
        );
    }

    #[test]
    fn a_legal_write_before_the_confirmation_keeps_the_state_non_empty() {
        let schedule = schedules(CFamily::CancelAfterConfirm)
            .into_iter()
            .find(|schedule| schedule.id == "c1-s02")
            .expect("第二条调度在");
        let mut environment = env(CFamily::CancelAfterConfirm, Implementation::Correct);
        environment.run(&schedule);

        assert_eq!(
            environment
                .observables()
                .value(&task_key("T1"))
                .map(str::to_owned),
            Some(DRAFT_VALUE.to_owned()),
            "暂停之前的合法写入留在终态里"
        );
        assert_eq!(
            hidden_check(
                CFamily::CancelAfterConfirm,
                environment.store(),
                environment.pause()
            )
            .verdict,
            ObserverVerdict::Satisfied,
            "顺序上它没违反要求"
        );
    }

    #[test]
    fn the_declared_attachment_misses_the_direct_path_but_the_journal_does_not() {
        let schedule = schedules(CFamily::CancelAfterConfirm)
            .into_iter()
            .find(|schedule| schedule.id == "c1-s06")
            .expect("第六条调度在");
        let mut environment = Environment::new(
            CFamily::CancelAfterConfirm,
            ProgramVersion::V2,
            Implementation::Uncoordinated,
            Attachment::at_boundary(),
        );
        environment.run(&schedule);

        assert_eq!(
            environment
                .recorder()
                .events()
                .iter()
                .filter(|event| event.path == WritePath::Direct)
                .count(),
            0,
            "声明里的测量位置是写入边界，直接提交路径不在上面，记录器看不到"
        );
        assert_eq!(
            environment.store().journal().len(),
            2,
            "存储日志里两次提交都在（直接提交 + 恢复时的边界提交）"
        );
        assert_eq!(
            environment.recorder().events().len(),
            1,
            "记录器只看到其中一次"
        );
        assert_eq!(
            hidden_check(
                CFamily::CancelAfterConfirm,
                environment.store(),
                environment.pause()
            )
            .verdict,
            ObserverVerdict::Violated,
            "存储日志照样记着"
        );

        let mut reattached = Environment::new(
            CFamily::CancelAfterConfirm,
            ProgramVersion::V2,
            Implementation::Uncoordinated,
            Attachment::reattached(),
        );
        reattached.run(&schedule);
        assert_eq!(
            reattached.recorder().events().len(),
            2,
            "重挂之后记录器看得见两条提交"
        );
    }

    #[test]
    fn each_faulty_implementation_is_caught_by_the_hidden_check_on_every_family() {
        for family in CFamily::ALL {
            for schedule in schedules(family) {
                for implementation in Implementation::ALL {
                    let mut environment = env(family, implementation);
                    environment.run(&schedule);
                    let reading = hidden_check(family, environment.store(), environment.pause());
                    if implementation.is_expected_clean() {
                        assert_eq!(
                            reading.verdict,
                            ObserverVerdict::Satisfied,
                            "{family} {implementation:?} {}：合理实现不该被认定违反（{}）",
                            schedule.id,
                            reading.detail
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_injected_fault_lands_on_the_recorder_and_the_journal() {
        let mut environment = env(CFamily::CancelAfterConfirm, Implementation::Correct);
        environment.inject_write("T1", &task_key("T1"), DRAFT_VALUE);
        assert_eq!(environment.recorder().events().len(), 1);
        assert_eq!(environment.store().journal().len(), 1);
        assert_eq!(environment.store().commit_seq(), 1);
    }

    #[test]
    fn hook_points_are_addressable_by_their_declared_ids() {
        for point in HookPoint::ALL {
            assert_eq!(HookPoint::parse(point.id()), Some(point));
            assert!(!point.description().is_empty());
        }
        assert_eq!(HookPoint::parse("nonexistent_point"), None);
        assert_eq!(
            HookPoint::parse("write_boundary"),
            Some(HookPoint::WriteBoundary)
        );
    }
}
