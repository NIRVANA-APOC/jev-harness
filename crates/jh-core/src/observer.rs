//! 观测器契约、独立检验与生命周期（研究目标 §7）。
//!
//! 研究目标 §7 说：现有观察支撑不了关键判断时，系统可以在声明的实验范围内建设新的观测器、可控实验
//! 或工具适配器，并研究这些产物「能否被验证、复用和撤回」。这个模块把 §7.2 的八条契约逐项落成类型，
//! 并把 §7.3 末段的独立检验做成**启用入口上的结构约束**——不是文档里的一句提醒。
//!
//! # 契约八条（§7.2 → 类型）
//!
//! | §7.2 的那一条 | 落点 |
//! | --- | --- |
//! | 对应的原始要求及来源 | [`ObserverContract::requirement`]（[`Requirement`]） |
//! | 要测量的具体命题，以及它与原要求之间的覆盖关系 | [`ObserverContract::proposition`] / [`ObserverContract::coverage`] |
//! | 测量位置、对象标识、时间或顺序语义 | [`ObserverContract::measurement`]（[`Measurement`] / [`Timing`]） |
//! | 支持的版本、配置、输入范围和必要环境条件 | [`ObserverContract::support`]（[`Support`]） |
//! | 会引入的副作用、性能扰动或调度变化 | [`ObserverContract::disturbance`]（[`Disturbance`]） |
//! | 阳性对照、阴性对照和可注入的已知故障 | [`ObserverContract::controls`]（[`Controls`]） |
//! | 输出证据格式、失败状态及失效条件 | [`ObserverContract::output`]（[`OutputContract`]） |
//! | 尚未覆盖的情况 | [`ObserverContract::uncovered`] |
//!
//! 契约字段都是必填项（类型上不是 `Option`），内容不校验：一条观测器能不能用，由独立检验与消费方
//! 判定，不由本模块的文字检查判定（与问题库同一口径）。唯一的结构要求是**测量对象非空**——没有
//! 对象的观测器在失效传播里没有挂点（[`Error::ObserverWithoutObject`]）。
//!
//! # 独立检验（§7.3 末段）
//!
//! 「观测器必须经过独立检验：已知错误实现应触发失败，合理实现应通过；尝试绕过记录器或修改日志时，
//! 独立的隐藏检查仍应发现真实副作用。测量脚本的成功输出不能成为唯一的真实性来源」——这句话落成
//! [`ObserverCheck`] 的四个方向，每一路都同时记「期望观测器给出什么判定」与「实际观测到什么」：
//!
//! 1. [`CheckDirection::Positive`] 阳性对照：合理实现应当通过（期望「命题成立」）；
//! 2. [`CheckDirection::Negative`] 阴性对照：已知错误实现应当被抓住（期望「发现违反」）；
//! 3. [`CheckDirection::InjectedFault`] 可注入的已知故障：注入之后应当被检出；
//! 4. [`CheckDirection::Hidden`] 独立隐藏检查：绕过记录器或篡改日志时，不依赖被测观测器的输出、
//!    直接核对真实副作用。
//!
//! **未通过独立检验的观测器进不了启用态，这是结构上的**：[`ObserverLibrary::enable`] 收一份
//! [`ObserverCheck`]（构造它就必须四路齐备），先把它落库、再逐路核对；只要有一路没对上、或观测器
//! 自己落在失败状态，启用就被显式拒绝（[`Error::ObserverNotVerified`]），生命周期保持原状。失败的
//! 那一条检验记录留在库里——「试过、并失败」本身是审计材料，不悄悄丢掉。
//!
//! 反过来也允许：一个版本改好之后重新检验、再启用是合法的——判定只看**最近一条**检验记录，历史
//! 记录全留。
//!
//! # 生命周期迁移
//!
//! | 操作 | 允许的当前状态 | 迁移后 |
//! | --- | --- | --- |
//! | [`propose`](ObserverLibrary::propose) | 该 ID 与版本未登记 | 候选 |
//! | [`start_verifying`](ObserverLibrary::start_verifying) | 候选 | 验证中 |
//! | [`enable`](ObserverLibrary::enable) | 验证中、被替代 | 启用；同 ID 原先启用的版本 → 被替代 |
//! | [`retire`](ObserverLibrary::retire) | 启用 | 停用（终态） |
//!
//! 其余组合显式失败（[`Error::InvalidObserverLifecycle`]）：候选不能直接启用（§7.3 的独立检验还没
//! 做），停用过的版本不能重新启用（要恢复就回退到被替代的版本，或升一个新版本——与问题库同口径）。
//! 一个 ID 任何时刻只有一个启用版本，由 [`ObserverLibrary::enabled`] 取。
//!
//! # 与判断 / 证据的失效联动
//!
//! 观测器退出使用会牵动证据链：**停用**，或**被新版本替代**，都让读过它测量对象
//! （[`Measurement::objects`]）的判断变成待重查，再沿依赖图传播到以它们为前提的行动
//! （[`SemanticState::invalidate_observer`](crate::SemanticState::invalidate_observer)，起因
//! [`InvalidationCause::ObserverInvalidated`](crate::InvalidationCause::ObserverInvalidated)）；
//! 无关结果原样保留，每个受影响目标带一条完整传播路径。
//!
//! 口径与证据传播一致，并且是刻意保守的：**按对象判定，不按渠道判定**——判断的输入投影只登记
//! 「读了哪些对象与字段」，不登记「从哪个渠道读到的」，所以宁可误失效也不留漏失效（研究目标 §5.3）。
//! 想精确撤回某几条证据本身，走
//! [`SemanticState::retract_evidence`](crate::SemanticState::retract_evidence)，那是另一条路。
//!
//! 全新启用一个观测器（没有替代谁）不触发失效：没有谁的证据因此作废。
//!
//! # 版本号规则（定案；待回写 `docs/roadmap-1.0.md` §12 未确定项 5 的观测器一侧）
//!
//! 与问题库同口径：**每个观测器 ID 各自从 1 开始的单调递增整数**（[`ObserverVersion`]`(u32)`），
//! 由库分配（取该 ID 当前最大版本 +1，调用方不写死）；同一 ID + 同一版本不得换含义，改契约必须拿新
//! 编号；停用与回退不产生新版本号；旧版本永不被覆盖或删除，只改状态，可比较、可回退、可复用。
//!
//! # 本模块刻意不做的事
//!
//! - 不执行观测器、不生成观测器实现、不提供沙箱或校验器 DSL（`docs/roadmap-1.0.md` §8 的 1.0 不做
//!   项）：观测器怎么跑、在哪里跑，由 `jh-app` 的隔离实验环境负责；
//! - 不判测量质量阈值：检验读数按原值保存，够不够用是消费方的事（与 [`ValidationRecord`](crate::ValidationRecord) 同口径）；
//! - 不写证据：观测器产出的证据仍走 [`SemanticState::append_evidence`](crate::SemanticState::append_evidence)，
//!   本模块只声明它的形态（[`OutputContract::channel`] 就是这些证据记录的 `source`）；
//! - 不读时钟：时间与序号由调用方给出。
//!
//! # 示例
//!
//! 从契约到启用，再到撤回时的失效传播：
//!
//! ```
//! use std::collections::{BTreeMap, BTreeSet};
//!
//! use jh_core::{
//!     ActionId, Answer, ControlResult, Error, InputProjection, JudgeSource,
//!     Judgment, JudgmentRecord, Measurement, NewEvidence, NoulAnswer, ObjectId, ObjectVersion,
//!     ObserverCheck, ObserverId, ObserverLibrary, ObserverVerdict, OutputContract, PremiseCheck,
//!     QuestionId, QuestionVersion, Requirement, SemanticState, SnapshotId, Timing,
//! };
//!
//! # fn main() -> Result<(), Error> {
//! let mut state = SemanticState::in_memory();
//! let mut library = ObserverLibrary::default();
//! let writes = ObjectId::from("cache.writes");
//!
//! // §7.2 的八条：这里只写与断言有关的几项，其余照实填。
//! let contract = jh_core::ObserverContract {
//!     requirement: Requirement {
//!         text: "取消确认后不应继续产生缓存写入".to_owned(),
//!         source: "用户要求（研究目标 §7.3）".to_owned(),
//!     },
//!     proposition: "同一任务的写入是否出现在取消确认之后".to_owned(),
//!     coverage: "覆盖「确认后写入」这一类顺序违反；不覆盖写入内容的正确性".to_owned(),
//!     measurement: Measurement {
//!         site: "缓存写入边界的事件记录器".to_owned(),
//!         objects: BTreeSet::from([writes.clone()]),
//!         timing: Timing::Sequence,
//!     },
//!     support: jh_core::Support {
//!         versions: BTreeSet::from(["app-1.4.0".to_owned()]),
//!         configuration: "记录器开关 recorder=on".to_owned(),
//!         inputs: "同一任务的写入事件".to_owned(),
//!         environment: "隔离实验环境，含可暂停点".to_owned(),
//!     },
//!     disturbance: jh_core::Disturbance {
//!         side_effects: "追加写记录文件，不改被测数据".to_owned(),
//!         performance: "每事件一次追加写".to_owned(),
//!         scheduling: "在写入边界同步记录".to_owned(),
//!     },
//!     controls: jh_core::Controls {
//!         positive: "顺序正确的实现（取消后不再写入）".to_owned(),
//!         negative: "已知错误实现（取消后仍写入）".to_owned(),
//!         injected_fault: "在确认后注入一次写入".to_owned(),
//!     },
//!     output: OutputContract {
//!         channel: "observer:cancel-write-recorder".to_owned(),
//!         format: "每条写入事件一条证据，含任务与事件序号".to_owned(),
//!         failure: "记录器读不到时给出显式失败，不给读不到的数".to_owned(),
//!         invalid_when: "记录器被绕过或事件序号不可比时".to_owned(),
//!     },
//!     uncovered: "跨进程重启后的写入顺序".to_owned(),
//! };
//!
//! let version = library.propose(
//!     &ObserverId::from("cancel_write_recorder"),
//!     contract,
//!     1_700_000_000_000,
//!     "观测需求：拿到写入边界的顺序证据".to_owned(),
//! )?;
//! library.start_verifying(
//!     &ObserverId::from("cancel_write_recorder"),
//!     version,
//!     1_700_000_001_000,
//!     "开始独立检验".to_owned(),
//! )?;
//!
//! // 四路齐备的独立检验：阳性对照通过、阴性对照抓住已知错误实现、注入故障被检出、
//! // 隐藏检查不依赖观测器自己的输出。
//! let outcome = |expected, observed, note: &str| ControlResult {
//!     expected,
//!     observed,
//!     readings: BTreeMap::from([("cost_units".to_owned(), 3.0)]),
//!     note: note.to_owned(),
//! };
//! let check = ObserverCheck {
//!     at_unix_ms: 1_700_000_002_000,
//!     positive: outcome(ObserverVerdict::Satisfied, ObserverVerdict::Satisfied, "合理实现"),
//!     negative: outcome(ObserverVerdict::Violated, ObserverVerdict::Violated, "已知错误实现"),
//!     injected_fault: outcome(ObserverVerdict::Violated, ObserverVerdict::Violated, "注入的写入"),
//!     hidden: outcome(ObserverVerdict::Violated, ObserverVerdict::Violated, "独立隐藏检查"),
//!     note: "三路对照 + 隐藏检查".to_owned(),
//! };
//! let (records, invalidation) = library.enable(
//!     &mut state,
//!     &ObserverId::from("cancel_write_recorder"),
//!     version,
//!     check,
//!     1_700_000_003_000,
//!     "独立检验全过".to_owned(),
//! )?;
//! assert_eq!(records[0].lifecycle, jh_core::ObserverLifecycle::Enabled);
//! assert!(invalidation.is_none(), "全新启用没有替代谁，没有可失效的东西");
//!
//! // 启用之后照常取证据、做判断、发行动。
//! state.append_evidence(NewEvidence {
//!     source: "observer:cancel-write-recorder".to_owned(),
//!     at_unix_ms: 1_700_000_010_000,
//!     object: writes.clone(),
//!     version: ObjectVersion(1),
//!     content: serde_json::json!({"task": "T-1", "write_after_confirm": false}),
//! })?;
//! let question = QuestionId::from("write_after_cancel_confirm");
//! let node = state.declare_node(
//!     question.clone(),
//!     QuestionVersion(1),
//!     InputProjection::covering([(writes, ObjectVersion(1))]),
//! );
//! state.record_judgment(
//!     node,
//!     JudgmentRecord {
//!         snapshot: SnapshotId::from("case-001"),
//!         source: JudgeSource::Rule,
//!         version: "rule-v1".to_owned(),
//!         request_id: None,
//!         usage: None,
//!         latency_ms: 1,
//!         judgments: BTreeMap::from([(
//!             question,
//!             Judgment {
//!                 question_version: QuestionVersion(1),
//!                 answer: Answer::Noul(NoulAnswer { noul: 0.02 }),
//!                 thresholds: BTreeMap::new(),
//!             },
//!         )]),
//!     },
//! )?;
//! let action = ActionId::from("declare-cancel-safe");
//! state.declare_action(action.clone(), [node])?;
//! assert_eq!(state.check_action(&action)?, PremiseCheck::Cleared);
//!
//! // 观测器停用：读过它测量对象的判断待重查，以它为前提的行动发不出去，路径完整。
//! let observer = ObserverId::from("cancel_write_recorder");
//! let (record, invalidation) = library.retire(
//!     &mut state,
//!     &observer,
//!     version,
//!     1_700_000_020_000,
//!     "记录器被绕过，观察渠道不再可信".to_owned(),
//! )?;
//! assert_eq!(record.lifecycle, jh_core::ObserverLifecycle::Retired);
//! assert_eq!(invalidation.stale, vec![node]);
//! assert_eq!(invalidation.blocked, vec![action.clone()]);
//! assert_eq!(invalidation.paths[0].hops.len(), 1, "对象 → 判断这一跳");
//! assert_eq!(state.check_action(&action)?, PremiseCheck::Blocked { premises: vec![node] });
//! # Ok(())
//! # }
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::evidence::ObjectId;
use crate::graph::{NodeRef, PropagationPath};
use crate::runtime::{Invalidation, SemanticState};

/// 观测器的稳定标识（如 `cancel_write_recorder`）。
///
/// 用于把契约、检验记录与生命周期状态对到一起，以及作为失效传播起因里的标识；不进证据记录
/// ——证据记录靠 [`OutputContract::channel`] 认渠道。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ObserverId(pub String);

impl ObserverId {
    /// 借出标识文本。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for ObserverId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl From<String> for ObserverId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl fmt::Display for ObserverId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// 观测器契约的版本号。
///
/// 规则与问题版本号同口径（见模块文档「版本号规则」）：每个 ID 各自从 1 开始的单调递增整数，由
/// [`ObserverLibrary`] 分配；改了契约必须拿新编号，旧版本的证据与检验记录不会被改写后的契约解释错。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ObserverVersion(pub u32);

impl fmt::Display for ObserverVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 观测器对应的原始要求及来源（§7.2 第 1 条）。
///
/// 观测器不是在真空里造的：它总是为了某条已经存在的要求（用户指令、验收判据、规格条款）。`source`
/// 记这条要求从哪来，`text` 记它的原文或要点——观测器被撤回、被替代时，这条要求本身是否仍然成立
/// 要能回看。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Requirement {
    /// 原始要求的原文或要点。
    pub text: String,
    /// 要求的来源：用户指令、验收判据、规格条款……
    pub source: String,
}

/// 读数的时间或顺序语义（§7.2 第 3 条）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Timing {
    /// 单点观测：一次采样给出当时的状态或读数，不表达事件之间的先后。
    Instant,
    /// 顺序观测：保留事件之间的先后——§7.3 的「取消确认之后是否仍有写入」就是这一类。
    Sequence,
}

/// 测量位置、对象标识与时间 / 顺序语义（§7.2 第 3 条）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Measurement {
    /// 测量位置：在哪个边界或环节取样（如「缓存写入边界的事件记录器」）。
    pub site: String,
    /// 观测证据描述的对象（[`ObjectId`]）；**不能为空**——§7.2 要求给出对象标识，它同时是失效传播的
    /// 挂点：观测器退出使用时，读过这些对象的判断都要重查。
    pub objects: BTreeSet<ObjectId>,
    /// 时间或顺序语义。
    pub timing: Timing,
}

/// 支持的版本、配置、输入范围与必要环境条件（§7.2 第 4 条）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Support {
    /// 支持的对象 / 接口版本（如 `app-1.4.0`、`scheduler-v2`）。
    pub versions: BTreeSet<String>,
    /// 需要的配置：从哪读、期望什么值。
    pub configuration: String,
    /// 输入范围：能观测哪一类输入、哪些形态；超出范围时不作结论。
    pub inputs: String,
    /// 必要环境条件：隔离实验环境、可暂停点、日志目录……
    pub environment: String,
}

/// 观测器会引入的副作用、性能扰动与调度变化（§7.2 第 5 条）。
///
/// 观测本身是被观察系统的扰动源：这三项要写清楚，代价才摊得清（H4 要报告测量扰动，研究目标 §10.2）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Disturbance {
    /// 副作用：观测会不会改变被测对象（追加写日志、暂停进程、占用端口……）。
    pub side_effects: String,
    /// 性能扰动：对被测系统与本循环的延迟 / 吞吐影响。
    pub performance: String,
    /// 调度变化：观测占用哪些时机、会不会改变任务顺序。
    pub scheduling: String,
}

/// 阳性对照、阴性对照与可注入的已知故障（§7.2 第 6 条）。
///
/// 这里声明的是「打算怎么检验」；实际跑出来的结果记在 [`ObserverCheck`] 里。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Controls {
    /// 阳性对照：合理实现应当通过的那种情形。
    pub positive: String,
    /// 阴性对照：已知错误实现应当被抓住的那种情形。
    pub negative: String,
    /// 可注入的已知故障：能主动注入、并要求被观测器检出的故障。
    pub injected_fault: String,
}

/// 观测器输出的证据格式、失败状态与失效条件（§7.2 第 7 条）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputContract {
    /// 证据渠道名：观测器产出的证据记录里 [`Evidence`](crate::Evidence) 的 `source` 字段值。
    ///
    /// 它是「哪些证据出自这个观测器」的唯一凭据，也让观测器的输出在日志里可追溯。
    pub channel: String,
    /// 证据格式：内容里有哪些字段、什么语义。
    pub format: String,
    /// 失败状态：读不到、环境不符、被判超时等情况下怎么表示失败，而不是默默给出读不到的数。
    pub failure: String,
    /// 失效条件：什么情况下这份观测器的结论不再成立，必须重新检验或停用。
    pub invalid_when: String,
}

/// 观测器契约：研究目标 §7.2 的八条逐项落类型。
///
/// 提出候选时一次给全（[`ObserverLibrary::propose`]）；生命周期与检验记录由库管，不在这里。
/// [`proposition`](Self::proposition) 与 [`coverage`](Self::coverage) 是 §7.2 第 2 条的两半：命题
/// 本身，以及它对原要求的覆盖关系——这个观测器覆盖原要求的哪一部分、哪一段情形，哪里不覆盖。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObserverContract {
    /// 第 1 条：对应的原始要求及来源。
    pub requirement: Requirement,
    /// 第 2 条（前一半）：要测量的具体命题。
    pub proposition: String,
    /// 第 2 条（后一半）：这个命题与原要求之间的覆盖关系。
    pub coverage: String,
    /// 第 3 条：测量位置、对象标识、时间或顺序语义。
    pub measurement: Measurement,
    /// 第 4 条：支持的版本、配置、输入范围和必要环境条件。
    pub support: Support,
    /// 第 5 条：会引入的副作用、性能扰动或调度变化。
    pub disturbance: Disturbance,
    /// 第 6 条：阳性对照、阴性对照和可注入的已知故障。
    pub controls: Controls,
    /// 第 7 条：输出证据格式、失败状态及失效条件。
    pub output: OutputContract,
    /// 第 8 条：尚未覆盖的情况。
    pub uncovered: String,
}

/// 一次观测（或一路对照）给出的判定（§7.2 第 7 条的「输出 …… 失败状态」）。
///
/// 三档，不含「未知」这类模糊档：观测器要么给出命题成立 / 被违反的判定，要么显式落在失败状态。
/// 失败不会被折成「命题成立」，也不会被悄悄当成「没有违反」。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "verdict", content = "detail")]
pub enum ObserverVerdict {
    /// 满足所测命题：这次观测没有发现违反。
    Satisfied,
    /// 发现违反：命题被违反（阳性检出）。
    Violated,
    /// 观测器自身失败：读不到、环境不符、被绕过……文本取原始错误，本 crate 不改写它。
    Failed(String),
}

impl fmt::Display for ObserverVerdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Satisfied => f.write_str("命题成立"),
            Self::Violated => f.write_str("发现违反"),
            Self::Failed(detail) => write!(f, "观测器失败：{detail}"),
        }
    }
}

/// 一路对照的检验结果：期望观测器给出什么判定，实际给出了什么。
///
/// 读数按原值保存（检出次数、误报数、延迟增量、计价单位……），口径由调用方声明；本 crate 不解释
/// 读数、不判阈值——够不够用是消费方的事（与 [`ValidationRecord`](crate::ValidationRecord) 同口径）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ControlResult {
    /// 期望的判定：这一路对照下观测器**应当**给出的判定。
    pub expected: ObserverVerdict,
    /// 实际观测到的判定。
    pub observed: ObserverVerdict,
    /// 读数：名字 → 值。
    pub readings: BTreeMap<String, f64>,
    /// 说明：这一路用的是什么实现 / 什么故障、怎么跑的。
    pub note: String,
}

impl ControlResult {
    /// 这一路是否通过：观测到的判定与期望一致，且**不是**观测器自身的失败状态。
    ///
    /// 「观测器失败」永远不算通过——包括期望恰好也是失败的场合：失败状态的含义是「这次没测出来」，
    /// 不能拿来当结论（§7.3 末段：测量脚本的成功输出不能成为唯一的真实性来源）。
    pub fn passed(&self) -> bool {
        !matches!(self.observed, ObserverVerdict::Failed(_)) && self.observed == self.expected
    }
}

/// 独立检验的四个方向（§7.2 第 6 条 + §7.3 末段）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckDirection {
    /// 阳性对照：合理实现应当通过。
    Positive,
    /// 阴性对照：已知错误实现应当触发失败（观测器必须报出违反）。
    Negative,
    /// 可注入的已知故障：注入之后应当被检出。
    InjectedFault,
    /// 独立隐藏检查：绕过记录器或篡改日志时，不依赖被测观测器的输出、直接核对真实副作用。
    Hidden,
}

impl fmt::Display for CheckDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Positive => f.write_str("阳性对照"),
            Self::Negative => f.write_str("阴性对照"),
            Self::InjectedFault => f.write_str("注入故障"),
            Self::Hidden => f.write_str("独立隐藏检查"),
        }
    }
}

/// 一次独立检验的记录（§7.3 末段）：四路齐备，缺一路就构造不出来。
///
/// 四路的结果都按「期望 / 观测到」记；一路是否通过只看 [`ControlResult::passed`]。整份记录是否通过看
/// [`ObserverCheck::passed`]，没通过的那一路看 [`ObserverCheck::first_failure`]。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObserverCheck {
    /// 记录时间，Unix 毫秒；由调用方给出。
    pub at_unix_ms: u64,
    /// 阳性对照的结果。
    pub positive: ControlResult,
    /// 阴性对照的结果。
    pub negative: ControlResult,
    /// 注入的已知故障的结果。
    pub injected_fault: ControlResult,
    /// 独立隐藏检查的结果。
    pub hidden: ControlResult,
    /// 说明：这一轮检验用的是什么对照实现、什么故障，隐藏检查核对的是什么。
    pub note: String,
}

impl ObserverCheck {
    /// 按固定顺序列出四路对照；校验与报告都按这个顺序走。
    pub fn directions(&self) -> [(CheckDirection, &ControlResult); 4] {
        [
            (CheckDirection::Positive, &self.positive),
            (CheckDirection::Negative, &self.negative),
            (CheckDirection::InjectedFault, &self.injected_fault),
            (CheckDirection::Hidden, &self.hidden),
        ]
    }

    /// 四路是否全部通过。
    pub fn passed(&self) -> bool {
        self.first_failure().is_none()
    }

    /// 没通过的那一路（按 [`ObserverCheck::directions`] 的顺序）；全过时为 `None`。
    pub fn first_failure(&self) -> Option<(CheckDirection, &ControlResult)> {
        self.directions()
            .into_iter()
            .find(|(_, result)| !result.passed())
    }
}

/// 观测器的生命周期状态（候选 → 验证 → 启用 → 停用 / 被替代）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "lifecycle")]
pub enum ObserverLifecycle {
    /// 候选：契约已登记，尚未检验，不参与观测。
    Candidate,
    /// 验证：正在做 §7.3 的独立检验，仍不参与观测。
    Verifying,
    /// 启用：当前版本，观测与判断可以依赖它。
    Enabled,
    /// 停用：曾启用，现已退出使用（终态）。
    Retired,
    /// 被替代：被 `by` 这个版本换下；不再作为依据，但可以回退复用。
    Superseded {
        /// 接替它成为当前启用版本的那个版本。
        by: ObserverVersion,
    },
}

impl ObserverLifecycle {
    /// 是否正在参与观测：只有 [`ObserverLifecycle::Enabled`] 算。
    pub fn is_enabled(&self) -> bool {
        matches!(self, Self::Enabled)
    }
}

impl fmt::Display for ObserverLifecycle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Candidate => f.write_str("候选"),
            Self::Verifying => f.write_str("验证"),
            Self::Enabled => f.write_str("启用"),
            Self::Retired => f.write_str("停用"),
            Self::Superseded { by } => write!(f, "被 v{by} 替代"),
        }
    }
}

/// 一条生命周期变更的记录（追加写）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObserverLifecycleRecord {
    /// 观测器标识。
    pub observer: ObserverId,
    /// 被改变的那个版本。
    pub version: ObserverVersion,
    /// 变更后的状态。
    pub lifecycle: ObserverLifecycle,
    /// 变更时间，Unix 毫秒；由调用方给出。
    pub at_unix_ms: u64,
    /// 依据：检验结论、替代关系、停用理由……由调用方写。
    pub reason: String,
}

/// 观测器库里的一个版本：契约（§7.2 八条）+ 独立检验记录 + 生命周期。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObserverEntry {
    /// §7.2 的八条契约。
    pub contract: ObserverContract,
    /// 独立检验记录，按追加顺序；**含未通过的那些**（试过并失败的凭据也留着）。
    pub checks: Vec<ObserverCheck>,
    /// 生命周期。
    pub lifecycle: ObserverLifecycle,
}

/// 观测器库：版本表 + 每个版本的契约与检验记录 + 生命周期变更历史。
///
/// 加版本只有 [`ObserverLibrary::propose`]，改状态只有 [`ObserverLibrary::start_verifying`] /
/// [`ObserverLibrary::enable`] / [`ObserverLibrary::retire`]；启用并要求一份四路全过的
/// [`ObserverCheck`]。可 JSON 往返（入库的观测器库文件形态属于 `jh-app` 的口径，本 crate 只管数据）。
/// 与问题库一样，从 JSON 读回的库不再重跑这些检查——入库文件是可信输入，校验在写入路径上。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ObserverLibrary {
    entries: BTreeMap<ObserverId, BTreeMap<ObserverVersion, ObserverEntry>>,
    history: Vec<ObserverLifecycleRecord>,
}

impl ObserverLibrary {
    /// 提出一个候选观测器：分配新版本、记下 §7.2 的八条契约，落在 [`ObserverLifecycle::Candidate`]。
    ///
    /// 没有声明测量对象的契约显式失败（[`Error::ObserverWithoutObject`]），且不占用版本号。
    pub fn propose(
        &mut self,
        id: &ObserverId,
        contract: ObserverContract,
        at_unix_ms: u64,
        reason: String,
    ) -> Result<ObserverVersion, Error> {
        if contract.measurement.objects.is_empty() {
            return Err(Error::ObserverWithoutObject(id.clone()));
        }

        let version = self.next_version(id);
        self.entries.entry(id.clone()).or_default().insert(
            version,
            ObserverEntry {
                contract,
                checks: Vec::new(),
                lifecycle: ObserverLifecycle::Candidate,
            },
        );
        self.history.push(ObserverLifecycleRecord {
            observer: id.clone(),
            version,
            lifecycle: ObserverLifecycle::Candidate,
            at_unix_ms,
            reason,
        });
        Ok(version)
    }

    /// 进入验证：候选 → 验证（§7.3 的独立检验在这一档里做）。
    pub fn start_verifying(
        &mut self,
        id: &ObserverId,
        version: ObserverVersion,
        at_unix_ms: u64,
        reason: String,
    ) -> Result<ObserverLifecycleRecord, Error> {
        match self.entry(id, version)?.lifecycle {
            ObserverLifecycle::Candidate => {}
            lifecycle => {
                return Err(Error::InvalidObserverLifecycle {
                    observer: id.clone(),
                    version,
                    lifecycle,
                });
            }
        }
        self.set_lifecycle(
            id,
            version,
            ObserverLifecycle::Verifying,
            at_unix_ms,
            reason,
        )
    }

    /// 启用：把某个版本设为当前启用的观测器版本。
    ///
    /// 只接受**验证**中或被替代的版本——候选没有做过 §7.3 的独立检验，直接启用会被显式拒绝
    /// （[`Error::InvalidObserverLifecycle`]）；被替代的版本允许回退复用，但检验记录要重新给。
    ///
    /// `check` 是这一轮的独立检验记录，四路必须全过（[`ObserverCheck`]）：**先落库、后核对**，
    /// 没通过的那一份也留在 [`ObserverEntry::checks`] 里，再以 [`Error::ObserverNotVerified`] 显式
    /// 失败——启用被拒绝，生命周期保持原状。判定只看最近一条记录：改好之后重新检验、再启用是合法的。
    ///
    /// 启用成功时，同 ID 原先启用的版本记成 [`ObserverLifecycle::Superseded`]，并触发失效传播：
    /// 读过**每个被换下版本**测量对象的判断全部待重查、以它们为前提的行动不能发出
    /// （[`InvalidationCause::ObserverInvalidated`](crate::InvalidationCause::ObserverInvalidated)）。
    /// 返回这次写下的生命周期记录（新启用版本一条在前，被它换下的旧版本各一条），以及那一路传播
    /// ——全新启用（没有替代谁）时为 `None`，因为没有谁的证据因此作废。
    ///
    /// 写入路径上被换下的版本只有一个（一个 ID 一个启用版本）；从 JSON 读回的库是可信输入、不重跑
    /// 这条不变量，里面可能有两个——那就**每个都发一次传播再合并**：`stale`、`blocked` 与 `paths`
    /// 是各次传播的并集，起因记第一个退出使用的版本（逐版本的完整轨迹在
    /// [`ObserverLibrary::history`] 里）。
    pub fn enable(
        &mut self,
        state: &mut SemanticState,
        id: &ObserverId,
        version: ObserverVersion,
        check: ObserverCheck,
        at_unix_ms: u64,
        reason: String,
    ) -> Result<(Vec<ObserverLifecycleRecord>, Option<Invalidation>), Error> {
        match self.entry(id, version)?.lifecycle {
            ObserverLifecycle::Verifying | ObserverLifecycle::Superseded { .. } => {}
            lifecycle => {
                return Err(Error::InvalidObserverLifecycle {
                    observer: id.clone(),
                    version,
                    lifecycle,
                });
            }
        }

        let failure = check
            .first_failure()
            .map(|(direction, result)| (direction, result.clone()));
        self.entry_mut(id, version)?.checks.push(check);
        if let Some((direction, result)) = failure {
            return Err(Error::ObserverNotVerified {
                observer: id.clone(),
                version,
                detail: format!(
                    "{direction}未通过：期望「{}」，观测到「{}」",
                    result.expected, result.observed
                ),
            });
        }

        let mut records = vec![self.set_lifecycle(
            id,
            version,
            ObserverLifecycle::Enabled,
            at_unix_ms,
            reason.clone(),
        )?];
        let displaced = self.supersede_enabled(id, version, at_unix_ms, reason)?;

        // 每个被换下的版本各发一次失效传播再合并：写入路径上只有一个（一个 ID 一个启用版本），
        // 但读回的库文件是可信输入、不重跑这条不变量，里面可能有多个——那就一个都不漏。
        let mut invalidation = None;
        for record in &displaced {
            let objects = self
                .entry(id, record.version)?
                .contract
                .measurement
                .objects
                .clone();
            let next = state.invalidate_observer(id, record.version, objects);
            invalidation = Some(match invalidation {
                None => next,
                Some(previous) => merge_invalidations(previous, next),
            });
        }
        records.extend(displaced);

        Ok((records, invalidation))
    }

    /// 停用：让当前启用的版本退出使用（终态）。
    ///
    /// 只接受当前启用的版本；停用过的版本不能重新启用——要恢复就回退到被替代的版本（再走一次
    /// [`enable`](Self::enable)），或升一个新版本。
    ///
    /// 停用一定触发失效传播：观测器不再可信，读过它测量对象
    /// （[`Measurement::objects`]）的判断待重查，沿依赖图连带行动
    /// （[`InvalidationCause::ObserverInvalidated`](crate::InvalidationCause::ObserverInvalidated)）。
    /// 停用不改写任何证据记录：已经取到的证据留在日志里，只是不再作为当前依据。
    pub fn retire(
        &mut self,
        state: &mut SemanticState,
        id: &ObserverId,
        version: ObserverVersion,
        at_unix_ms: u64,
        reason: String,
    ) -> Result<(ObserverLifecycleRecord, Invalidation), Error> {
        match self.entry(id, version)?.lifecycle {
            ObserverLifecycle::Enabled => {}
            lifecycle => {
                return Err(Error::InvalidObserverLifecycle {
                    observer: id.clone(),
                    version,
                    lifecycle,
                });
            }
        }

        let record =
            self.set_lifecycle(id, version, ObserverLifecycle::Retired, at_unix_ms, reason)?;
        let objects = self
            .entry(id, version)?
            .contract
            .measurement
            .objects
            .clone();
        Ok((record, state.invalidate_observer(id, version, objects)))
    }

    /// 某个版本的契约、检验记录与生命周期。
    pub fn at(&self, id: &ObserverId, version: ObserverVersion) -> Option<&ObserverEntry> {
        self.entries.get(id)?.get(&version)
    }

    /// 某个 ID 当前的启用版本与它的契约；没有启用中的版本时为 `None`。
    pub fn enabled(&self, id: &ObserverId) -> Option<(ObserverVersion, &ObserverEntry)> {
        self.entries
            .get(id)?
            .iter()
            .find(|(_, entry)| entry.lifecycle.is_enabled())
            .map(|(version, entry)| (*version, entry))
    }

    /// 某个 ID 的全部版本（按版本号升序），含已停用与被替代的。
    pub fn versions(&self, id: &ObserverId) -> Vec<(ObserverVersion, &ObserverEntry)> {
        self.entries
            .get(id)
            .map(|versions| versions.iter().map(|(v, e)| (*v, e)).collect())
            .unwrap_or_default()
    }

    /// 库里的观测器 ID（按标识排序）。
    pub fn ids(&self) -> impl Iterator<Item = &ObserverId> {
        self.entries.keys()
    }

    /// 生命周期变更历史（按发生顺序追加）。
    pub fn history(&self) -> &[ObserverLifecycleRecord] {
        &self.history
    }

    fn next_version(&self, id: &ObserverId) -> ObserverVersion {
        let max = self
            .entries
            .get(id)
            .and_then(|versions| versions.keys().next_back())
            .map_or(0, |version| version.0);
        ObserverVersion(max + 1)
    }

    fn set_lifecycle(
        &mut self,
        id: &ObserverId,
        version: ObserverVersion,
        lifecycle: ObserverLifecycle,
        at_unix_ms: u64,
        reason: String,
    ) -> Result<ObserverLifecycleRecord, Error> {
        self.entry_mut(id, version)?.lifecycle = lifecycle;
        let record = ObserverLifecycleRecord {
            observer: id.clone(),
            version,
            lifecycle,
            at_unix_ms,
            reason,
        };
        self.history.push(record.clone());
        Ok(record)
    }

    /// 让 `version` 之外仍启用的版本让位：记成被 `version` 替代，返回写下的记录。
    ///
    /// 一个 ID 任何时刻只应有一个启用版本；[`ObserverLibrary::enable`] 从这里保持这条不变量
    /// （调用前应已把 `version` 置为启用，本函数只处理其余的）。
    fn supersede_enabled(
        &mut self,
        id: &ObserverId,
        version: ObserverVersion,
        at_unix_ms: u64,
        reason: String,
    ) -> Result<Vec<ObserverLifecycleRecord>, Error> {
        let displaced: Vec<ObserverVersion> = self
            .versions(id)
            .into_iter()
            .filter(|(other, entry)| *other != version && entry.lifecycle.is_enabled())
            .map(|(other, _)| other)
            .collect();

        let mut records = Vec::with_capacity(displaced.len());
        for previous in displaced {
            records.push(self.set_lifecycle(
                id,
                previous,
                ObserverLifecycle::Superseded { by: version },
                at_unix_ms,
                reason.clone(),
            )?);
        }
        Ok(records)
    }

    fn entry(&self, id: &ObserverId, version: ObserverVersion) -> Result<&ObserverEntry, Error> {
        self.at(id, version)
            .ok_or_else(|| Error::UnknownObserverVersion(id.clone(), version))
    }

    fn entry_mut(
        &mut self,
        id: &ObserverId,
        version: ObserverVersion,
    ) -> Result<&mut ObserverEntry, Error> {
        self.entries
            .get_mut(id)
            .and_then(|versions| versions.get_mut(&version))
            .ok_or_else(|| Error::UnknownObserverVersion(id.clone(), version))
    }
}

/// 把同一批替代里的多次失效传播并成一次结果。
///
/// 起因取先发生的那次（即第一个退出使用的版本）；`stale` / `blocked` 取并集，各按标识排序；
/// `paths` 重建为与之一一对应的顺序（先判断节点、后行动，每个目标一条路径）。
/// 只在同一次 [`ObserverLibrary::enable`] 换下多个版本时用到——那要求库文件里同一个 ID 有两个
/// 启用版本，是注入态而不是写入路径能走到的形态；即便如此也不留漏失效。
fn merge_invalidations(first: Invalidation, next: Invalidation) -> Invalidation {
    let mut stale = first.stale;
    stale.extend(next.stale);
    stale.sort();
    stale.dedup();

    let mut blocked = first.blocked;
    blocked.extend(next.blocked);
    blocked.sort();
    blocked.dedup();

    let mut paths: BTreeMap<NodeRef, PropagationPath> = BTreeMap::new();
    for path in first.paths.into_iter().chain(next.paths) {
        paths.entry(path.target.clone()).or_insert(path);
    }

    let mut ordered = Vec::with_capacity(stale.len() + blocked.len());
    for node in &stale {
        if let Some(path) = paths.remove(&NodeRef::Judgment(*node)) {
            ordered.push(path);
        }
    }
    for action in &blocked {
        if let Some(path) = paths.remove(&NodeRef::Action(action.clone())) {
            ordered.push(path);
        }
    }

    Invalidation {
        cause: first.cause,
        stale,
        blocked,
        paths: ordered,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::answer::{Answer, NoulAnswer};
    use crate::evidence::{Evidence, NewEvidence, ObjectVersion};
    use crate::graph::ActionId;
    use crate::projection::InputProjection;
    use crate::question::{QuestionId, QuestionVersion};
    use crate::record::{JudgeSource, Judgment, JudgmentRecord};
    use crate::runtime::{InvalidationCause, PremiseCheck, Validity};
    use crate::state::SnapshotId;

    /// 一份填满 §7.2 八条的契约；测量对象由参数给出。
    fn contract(objects: &[&str]) -> ObserverContract {
        ObserverContract {
            requirement: Requirement {
                text: "取消确认后不应继续产生缓存写入".to_owned(),
                source: "用户要求（研究目标 §7.3）".to_owned(),
            },
            proposition: "同一任务的写入是否出现在取消确认之后".to_owned(),
            coverage: "覆盖「确认后写入」这一类顺序违反；不覆盖写入内容是否正确".to_owned(),
            measurement: Measurement {
                site: "缓存写入边界的事件记录器".to_owned(),
                objects: objects
                    .iter()
                    .map(|object| ObjectId::from(*object))
                    .collect(),
                timing: Timing::Sequence,
            },
            support: Support {
                versions: BTreeSet::from(["app-1.4.0".to_owned()]),
                configuration: "recorder=on".to_owned(),
                inputs: "同一任务的写入事件".to_owned(),
                environment: "隔离实验环境，含可暂停点".to_owned(),
            },
            disturbance: Disturbance {
                side_effects: "追加写记录文件，不改被测数据".to_owned(),
                performance: "每事件一次追加写".to_owned(),
                scheduling: "在写入边界同步记录".to_owned(),
            },
            controls: Controls {
                positive: "顺序正确的实现".to_owned(),
                negative: "已知错误实现（取消后仍写入）".to_owned(),
                injected_fault: "在确认之后注入一次写入".to_owned(),
            },
            output: OutputContract {
                channel: "observer:cancel-write-recorder".to_owned(),
                format: "每条写入事件一条证据（任务、事件序号）".to_owned(),
                failure: "读不到时显式失败，不给读不到的数".to_owned(),
                invalid_when: "记录器被绕过或事件序号不可比".to_owned(),
            },
            uncovered: "跨进程重启后的写入顺序".to_owned(),
        }
    }

    fn control(expected: ObserverVerdict, observed: ObserverVerdict) -> ControlResult {
        ControlResult {
            expected,
            observed,
            readings: BTreeMap::from([("cost_units".to_owned(), 3.0)]),
            note: "对照".to_owned(),
        }
    }

    /// 四路全过的检验记录。
    fn passing_check(at_unix_ms: u64) -> ObserverCheck {
        ObserverCheck {
            at_unix_ms,
            positive: control(ObserverVerdict::Satisfied, ObserverVerdict::Satisfied),
            negative: control(ObserverVerdict::Violated, ObserverVerdict::Violated),
            injected_fault: control(ObserverVerdict::Violated, ObserverVerdict::Violated),
            hidden: control(ObserverVerdict::Violated, ObserverVerdict::Violated),
            note: "三路对照 + 隐藏检查".to_owned(),
        }
    }

    fn observer_id() -> ObserverId {
        ObserverId::from("cancel_write_recorder")
    }

    /// 走完候选 → 验证 → 启用，返回版本号。
    fn enable_first_version(
        library: &mut ObserverLibrary,
        state: &mut SemanticState,
    ) -> ObserverVersion {
        let id = observer_id();
        let version = library
            .propose(&id, contract(&["cache.writes"]), 100, "观测需求".to_owned())
            .expect("候选可登记");
        library
            .start_verifying(&id, version, 110, "开始独立检验".to_owned())
            .expect("候选可进验证");
        let (_, invalidation) = library
            .enable(
                &mut *state,
                &id,
                version,
                passing_check(120),
                130,
                "检验全过".to_owned(),
            )
            .expect("四路全过即可启用");
        assert!(
            invalidation.is_none(),
            "全新启用没有替代谁，没有可失效的东西"
        );
        version
    }

    /// 追加一条证据（供判断节点读到当前版本）。
    fn observe(state: &mut SemanticState, object: &str, version: u64) -> Evidence {
        state
            .append_evidence(NewEvidence {
                source: "observer:cancel-write-recorder".to_owned(),
                at_unix_ms: 1_700_000_000_000 + version,
                object: ObjectId::from(object),
                version: ObjectVersion(version),
                content: json!({ "version": version }),
            })
            .expect("内存日志不会失败")
            .0
    }

    /// 声明一个读过这些对象的判断节点并写入一次判断。
    fn judge_reading(
        state: &mut SemanticState,
        question: &str,
        reads: &[(&str, u64)],
    ) -> crate::NodeId {
        let projection = InputProjection::covering(
            reads
                .iter()
                .map(|(object, version)| (ObjectId::from(*object), ObjectVersion(*version))),
        );
        let node = state.declare_node(QuestionId::from(question), QuestionVersion(1), projection);
        state
            .record_judgment(
                node,
                JudgmentRecord {
                    snapshot: SnapshotId::from("case-001"),
                    source: JudgeSource::Rule,
                    version: "rule-v1".to_owned(),
                    request_id: None,
                    usage: None,
                    latency_ms: 1,
                    judgments: BTreeMap::from([(
                        QuestionId::from(question),
                        Judgment {
                            question_version: QuestionVersion(1),
                            answer: Answer::Noul(NoulAnswer { noul: 1.0 }),
                            thresholds: BTreeMap::new(),
                        },
                    )]),
                },
            )
            .expect("记录与节点一致");
        node
    }

    #[test]
    fn proposing_a_candidate_allocates_versions_and_requires_a_measured_object() {
        let mut library = ObserverLibrary::default();
        let id = observer_id();

        let first = library
            .propose(&id, contract(&["cache.writes"]), 100, "观测需求".to_owned())
            .expect("候选可登记");
        assert_eq!(first, ObserverVersion(1));
        let entry = library.at(&id, first).expect("候选在库里");
        assert_eq!(entry.lifecycle, ObserverLifecycle::Candidate);
        assert_eq!(entry.checks, Vec::new());
        assert_eq!(entry.contract.measurement.timing, Timing::Sequence);
        assert_eq!(library.enabled(&id), None, "候选不参与观测");

        // 同一 ID 再提一个：版本号取当前最大 +1，旧版本原样留着。
        let second = library
            .propose(
                &id,
                contract(&["cache.writes", "scheduler.events"]),
                110,
                "换测量范围".to_owned(),
            )
            .expect("候选可登记");
        assert_eq!(second, ObserverVersion(2));
        assert_eq!(library.versions(&id).len(), 2);
        assert_eq!(library.ids().count(), 1);

        // 没有测量对象的契约连记都不记，也不占用版本号。
        let bare = ObserverId::from("no_object");
        let error = library
            .propose(&bare, contract(&[]), 120, "生成模型漏了对象".to_owned())
            .expect_err("没有测量对象必须显式失败");
        assert!(matches!(error, Error::ObserverWithoutObject(_)), "{error}");
        assert!(error.to_string().contains("no_object"), "{error}");
        assert_eq!(library.versions(&bare).len(), 0);
        assert_eq!(library.enabled(&bare), None);
    }

    #[test]
    fn the_lifecycle_walks_candidate_verifying_enabled_retired() {
        let mut library = ObserverLibrary::default();
        let mut state = SemanticState::in_memory();
        let id = observer_id();
        let version = library
            .propose(&id, contract(&["cache.writes"]), 100, "观测需求".to_owned())
            .unwrap();

        // 候选不能直接启用：§7.3 的独立检验还没做（哪怕手里已经有一份全过的记录）。
        let error = library
            .enable(
                &mut state,
                &id,
                version,
                passing_check(110),
                120,
                "跳过检验".to_owned(),
            )
            .expect_err("候选不能直接启用");
        assert!(
            matches!(
                error,
                Error::InvalidObserverLifecycle {
                    lifecycle: ObserverLifecycle::Candidate,
                    ..
                }
            ),
            "{error}"
        );
        assert_eq!(
            library.at(&id, version).unwrap().checks,
            Vec::new(),
            "被拒绝的启用不落检验记录"
        );

        let verifying = library
            .start_verifying(&id, version, 130, "开始独立检验".to_owned())
            .expect("候选可进验证");
        assert_eq!(verifying.lifecycle, ObserverLifecycle::Verifying);
        let error = library
            .start_verifying(&id, version, 140, "再进一次验证".to_owned())
            .expect_err("已在验证里的版本不能再进验证");
        assert!(
            matches!(error, Error::InvalidObserverLifecycle { .. }),
            "{error}"
        );

        let (records, invalidation) = library
            .enable(
                &mut state,
                &id,
                version,
                passing_check(150),
                160,
                "四路全过".to_owned(),
            )
            .expect("验证过的版本可启用");
        assert_eq!(records.len(), 1, "没有旧版本让位：{records:?}");
        assert_eq!(records[0].lifecycle, ObserverLifecycle::Enabled);
        assert!(invalidation.is_none());
        assert_eq!(library.at(&id, version).unwrap().checks.len(), 1);
        assert_eq!(
            library.enabled(&id).map(|(version, _)| version),
            Some(version)
        );

        let error = library
            .enable(
                &mut state,
                &id,
                version,
                passing_check(170),
                180,
                "再启用一次".to_owned(),
            )
            .expect_err("已经启用的版本不能再启用");
        assert!(
            matches!(error, Error::InvalidObserverLifecycle { .. }),
            "{error}"
        );

        let (retired, invalidation) = library
            .retire(&mut state, &id, version, 190, "记录器被绕过".to_owned())
            .expect("启用中的版本可停用");
        assert_eq!(retired.lifecycle, ObserverLifecycle::Retired);
        assert_eq!(
            invalidation.cause,
            InvalidationCause::ObserverInvalidated {
                observer: id.clone(),
                version,
            }
        );
        assert!(invalidation.is_empty(), "还没有判断读过它的对象");
        assert_eq!(library.enabled(&id), None);

        let error = library
            .retire(&mut state, &id, version, 200, "再停一次".to_owned())
            .expect_err("停用是终态");
        assert!(
            matches!(error, Error::InvalidObserverLifecycle { .. }),
            "{error}"
        );
        let error = library
            .enable(
                &mut state,
                &id,
                version,
                passing_check(210),
                220,
                "停用后再启用".to_owned(),
            )
            .expect_err("停用过的版本不能回到当前版本");
        assert!(
            matches!(error, Error::InvalidObserverLifecycle { .. }),
            "{error}"
        );

        // 生命周期变更都留在历史里，按发生顺序可查。
        assert_eq!(
            library
                .history()
                .iter()
                .map(|record| record.lifecycle)
                .collect::<Vec<_>>(),
            vec![
                ObserverLifecycle::Candidate,
                ObserverLifecycle::Verifying,
                ObserverLifecycle::Enabled,
                ObserverLifecycle::Retired,
            ]
        );
    }

    #[test]
    fn a_check_that_did_not_pass_cannot_enable_and_stays_on_record() {
        let mut library = ObserverLibrary::default();
        let mut state = SemanticState::in_memory();
        let id = observer_id();
        let version = library
            .propose(&id, contract(&["cache.writes"]), 100, "观测需求".to_owned())
            .unwrap();
        library
            .start_verifying(&id, version, 110, "开始独立检验".to_owned())
            .unwrap();

        // 已知错误实现没被抓住：阴性对照这一路没对上。
        let slipped = ObserverCheck {
            negative: control(ObserverVerdict::Violated, ObserverVerdict::Satisfied),
            ..passing_check(120)
        };
        let error = library
            .enable(&mut state, &id, version, slipped, 130, "试启用".to_owned())
            .expect_err("未通过独立检验的观测器不能启用");
        assert!(
            matches!(error, Error::ObserverNotVerified { .. }),
            "{error}"
        );
        assert!(error.to_string().contains("阴性对照"), "{error}");
        assert!(error.to_string().contains("发现违反"), "{error}");
        assert_eq!(
            library.at(&id, version).unwrap().lifecycle,
            ObserverLifecycle::Verifying,
            "生命周期保持原状"
        );
        assert_eq!(
            library.at(&id, version).unwrap().checks.len(),
            1,
            "失败的检验也留档"
        );
        assert_eq!(library.enabled(&id), None);

        // 观测器自己失败（读不到）同样不算通过。
        let broken = ObserverCheck {
            hidden: control(
                ObserverVerdict::Violated,
                ObserverVerdict::Failed("记录器被绕过".to_owned()),
            ),
            ..passing_check(140)
        };
        let error = library
            .enable(&mut state, &id, version, broken, 150, "试启用".to_owned())
            .expect_err("观测器自己失败不能算通过");
        assert!(error.to_string().contains("独立隐藏检查"), "{error}");

        // 改好之后重新检验、再启用是合法的：判定只看最近一条。
        library
            .enable(
                &mut state,
                &id,
                version,
                passing_check(160),
                170,
                "修好后重跑检验".to_owned(),
            )
            .expect("最近的检验全过即可启用");
        assert_eq!(library.at(&id, version).unwrap().checks.len(), 3);
        assert_eq!(
            library.enabled(&id).map(|(version, _)| version),
            Some(version)
        );
    }

    #[test]
    fn control_results_pass_only_when_the_observer_agrees_and_does_not_fail() {
        assert!(control(ObserverVerdict::Satisfied, ObserverVerdict::Satisfied).passed());
        assert!(control(ObserverVerdict::Violated, ObserverVerdict::Violated).passed());
        assert!(!control(ObserverVerdict::Violated, ObserverVerdict::Satisfied).passed());
        assert!(!control(ObserverVerdict::Satisfied, ObserverVerdict::Violated).passed());
        // 观测器失败永远不算通过，哪怕期望恰好也是失败——失败的含义是「这次没测出来」。
        assert!(
            !control(
                ObserverVerdict::Failed("读不到".to_owned()),
                ObserverVerdict::Failed("读不到".to_owned())
            )
            .passed()
        );

        let check = ObserverCheck {
            negative: control(ObserverVerdict::Violated, ObserverVerdict::Satisfied),
            ..passing_check(1)
        };
        assert!(!check.passed());
        assert_eq!(
            check.first_failure().map(|(direction, _)| direction),
            Some(CheckDirection::Negative)
        );
        assert!(passing_check(2).passed());
    }

    #[test]
    fn a_freshly_enabled_observer_disowns_nothing() {
        let mut state = SemanticState::in_memory();
        let mut library = ObserverLibrary::default();
        observe(&mut state, "cache.writes", 1);
        let node = judge_reading(&mut state, "write_after_cancel", &[("cache.writes", 1)]);

        enable_first_version(&mut library, &mut state);

        assert_eq!(state.node(node).unwrap().validity, Validity::Current);
        assert_eq!(state.recheck_set(), Vec::new());
    }

    #[test]
    fn enabling_a_new_version_supersedes_the_old_one_and_invalidates_its_readers() {
        let mut state = SemanticState::in_memory();
        let mut library = ObserverLibrary::default();
        let id = observer_id();
        observe(&mut state, "cache.writes", 1);
        observe(&mut state, "app.log", 1);
        let first = enable_first_version(&mut library, &mut state);

        // 观测器启用后照常取证据、做判断、发行动。
        let read = judge_reading(&mut state, "write_after_cancel", &[("cache.writes", 1)]);
        let unrelated = judge_reading(&mut state, "cause_b_pair", &[("app.log", 1)]);
        let action = ActionId::from("declare-cancel-safe");
        state.declare_action(action.clone(), [read]).unwrap();
        assert_eq!(state.check_action(&action).unwrap(), PremiseCheck::Cleared);

        // 记录器被绕过：造一个修好的 v2 顶上。
        let second = library
            .propose(
                &id,
                contract(&["cache.writes"]),
                200,
                "修掉被绕过的那条路径".to_owned(),
            )
            .unwrap();
        library
            .start_verifying(&id, second, 210, "重跑独立检验".to_owned())
            .unwrap();
        let (records, invalidation) = library
            .enable(
                &mut state,
                &id,
                second,
                passing_check(220),
                230,
                "v2 四路全过".to_owned(),
            )
            .unwrap();

        assert_eq!(
            records
                .iter()
                .map(|record| (record.version, record.lifecycle))
                .collect::<Vec<_>>(),
            vec![
                (second, ObserverLifecycle::Enabled),
                (first, ObserverLifecycle::Superseded { by: second }),
            ],
            "先记新启用版本，再记被它换下的旧版本"
        );
        let invalidation = invalidation.expect("替代旧版本一定有失效传播");
        assert_eq!(
            invalidation.cause,
            InvalidationCause::ObserverInvalidated {
                observer: id.clone(),
                version: first,
            },
            "起因是退出使用的那个版本"
        );
        assert_eq!(invalidation.stale, vec![read]);
        assert_eq!(invalidation.blocked, vec![action.clone()]);
        assert_eq!(
            invalidation.paths.len(),
            invalidation.stale.len() + invalidation.blocked.len()
        );
        assert_eq!(
            invalidation
                .paths
                .iter()
                .find(|path| path.target == crate::graph::NodeRef::Action(action.clone()))
                .expect("行动在结果里")
                .hops
                .iter()
                .map(|hop| hop.kind)
                .collect::<Vec<_>>(),
            vec![crate::EdgeKind::Read, crate::EdgeKind::Premise]
        );
        // 无关结果保留。
        assert_eq!(state.node(unrelated).unwrap().validity, Validity::Current);
        assert_eq!(state.recheck_set(), vec![read]);
        assert_eq!(
            state.check_action(&action).unwrap(),
            PremiseCheck::Blocked {
                premises: vec![read]
            }
        );
        // 旧版本没被删：可比较、可作为回退目标（回退要重新给一份检验记录）。
        assert_eq!(library.versions(&id).len(), 2);
        assert_eq!(
            library.enabled(&id).map(|(version, _)| version),
            Some(second)
        );
        assert!(matches!(
            library.at(&id, first).unwrap().lifecycle,
            ObserverLifecycle::Superseded { by } if by == second
        ));
    }

    #[test]
    fn retiring_an_observer_invalidates_the_judgments_that_read_its_objects() {
        let mut state = SemanticState::in_memory();
        let mut library = ObserverLibrary::default();
        let id = observer_id();
        observe(&mut state, "cache.writes", 1);
        observe(&mut state, "app.log", 1);
        let version = enable_first_version(&mut library, &mut state);

        let read = judge_reading(&mut state, "write_after_cancel", &[("cache.writes", 1)]);
        let unrelated = judge_reading(&mut state, "cause_b_pair", &[("app.log", 1)]);
        let action = ActionId::from("declare-cancel-safe");
        state.declare_action(action.clone(), [read]).unwrap();

        let (record, invalidation) = library
            .retire(
                &mut state,
                &id,
                version,
                300,
                "隐藏检查发现记录器被绕过".to_owned(),
            )
            .unwrap();

        assert_eq!(record.lifecycle, ObserverLifecycle::Retired);
        assert_eq!(record.at_unix_ms, 300);
        assert_eq!(
            invalidation.cause,
            InvalidationCause::ObserverInvalidated {
                observer: id.clone(),
                version,
            }
        );
        assert_eq!(invalidation.stale, vec![read]);
        assert_eq!(invalidation.blocked, vec![action.clone()]);
        assert_eq!(state.node(read).unwrap().validity, Validity::NeedsRecheck);
        assert_eq!(state.node(unrelated).unwrap().validity, Validity::Current);
        assert_eq!(state.recheck_set(), vec![read]);
        assert_eq!(
            state.check_action(&action).unwrap(),
            PremiseCheck::Blocked {
                premises: vec![read]
            }
        );
        // 停用不改写证据：记录还都在。
        assert_eq!(state.evidence().entries().len(), 2);
        assert!(
            !state
                .evidence()
                .is_retracted(observe_id_of(&state, "cache.writes"))
        );
    }

    /// 第一条属于某对象的证据的标识（用来断言停用不改证据）。
    fn observe_id_of(state: &SemanticState, object: &str) -> crate::EvidenceId {
        state
            .evidence()
            .entries()
            .iter()
            .find(|entry| entry.object == ObjectId::from(object))
            .expect("这条证据刚追加过")
            .id
    }

    #[test]
    fn the_eight_contract_items_are_all_written_to_the_wire() {
        let mut library = ObserverLibrary::default();
        let id = observer_id();
        let version = library
            .propose(&id, contract(&["cache.writes"]), 100, "观测需求".to_owned())
            .unwrap();

        let wire = serde_json::to_value(library.at(&id, version).unwrap()).unwrap();
        // §7.2 的八条逐项落位（第 2 条拆成命题与覆盖关系两半）。
        assert_eq!(
            wire["contract"]["requirement"]["text"],
            json!("取消确认后不应继续产生缓存写入")
        );
        assert_eq!(
            wire["contract"]["requirement"]["source"],
            json!("用户要求（研究目标 §7.3）")
        );
        assert_eq!(
            wire["contract"]["proposition"],
            json!("同一任务的写入是否出现在取消确认之后")
        );
        assert!(
            wire["contract"]["coverage"]
                .as_str()
                .unwrap()
                .contains("覆盖")
        );
        assert_eq!(
            wire["contract"]["measurement"]["site"],
            json!("缓存写入边界的事件记录器")
        );
        assert_eq!(
            wire["contract"]["measurement"]["objects"],
            json!(["cache.writes"])
        );
        assert_eq!(wire["contract"]["measurement"]["timing"], json!("sequence"));
        assert_eq!(
            wire["contract"]["support"]["versions"],
            json!(["app-1.4.0"])
        );
        assert_eq!(
            wire["contract"]["support"]["configuration"],
            json!("recorder=on")
        );
        assert_eq!(
            wire["contract"]["support"]["inputs"],
            json!("同一任务的写入事件")
        );
        assert_eq!(
            wire["contract"]["support"]["environment"],
            json!("隔离实验环境，含可暂停点")
        );
        assert_eq!(
            wire["contract"]["disturbance"]["side_effects"],
            json!("追加写记录文件，不改被测数据")
        );
        assert_eq!(
            wire["contract"]["disturbance"]["performance"],
            json!("每事件一次追加写")
        );
        assert_eq!(
            wire["contract"]["disturbance"]["scheduling"],
            json!("在写入边界同步记录")
        );
        assert_eq!(
            wire["contract"]["controls"]["positive"],
            json!("顺序正确的实现")
        );
        assert!(
            wire["contract"]["controls"]["negative"]
                .as_str()
                .unwrap()
                .contains("已知错误实现")
        );
        assert_eq!(
            wire["contract"]["controls"]["injected_fault"],
            json!("在确认之后注入一次写入")
        );
        assert_eq!(
            wire["contract"]["output"]["channel"],
            json!("observer:cancel-write-recorder")
        );
        assert!(
            wire["contract"]["output"]["format"]
                .as_str()
                .unwrap()
                .contains("证据")
        );
        assert!(
            wire["contract"]["output"]["failure"]
                .as_str()
                .unwrap()
                .contains("显式失败")
        );
        assert!(
            wire["contract"]["output"]["invalid_when"]
                .as_str()
                .unwrap()
                .contains("绕过")
        );
        assert_eq!(
            wire["contract"]["uncovered"],
            json!("跨进程重启后的写入顺序")
        );

        // 生命周期落在候选档，检验记录还是空的。
        assert_eq!(
            wire["lifecycle"],
            json!({"lifecycle": "candidate"}),
            "生命周期与问题库同形"
        );
        assert_eq!(wire["checks"], json!([]));
    }

    #[test]
    fn the_library_round_trips_through_json_with_its_lifecycle_checks_and_history() {
        let mut state = SemanticState::in_memory();
        let mut library = ObserverLibrary::default();
        let id = observer_id();
        observe(&mut state, "cache.writes", 1);

        let first = enable_first_version(&mut library, &mut state);
        let second = library
            .propose(
                &id,
                contract(&["cache.writes", "scheduler.events"]),
                200,
                "扩测量范围".to_owned(),
            )
            .unwrap();
        library
            .start_verifying(&id, second, 210, "开始独立检验".to_owned())
            .unwrap();
        let (records, _) = library
            .enable(
                &mut state,
                &id,
                second,
                passing_check(220),
                230,
                "检验全过".to_owned(),
            )
            .unwrap();
        assert_eq!(records.len(), 2, "新版本 + 被它换下的旧版本");

        let wire = serde_json::to_value(&library).unwrap();
        assert_eq!(
            wire["entries"]["cancel_write_recorder"]["1"]["lifecycle"],
            json!({"lifecycle": "superseded", "by": 2})
        );
        assert_eq!(
            wire["entries"]["cancel_write_recorder"]["2"]["lifecycle"],
            json!({"lifecycle": "enabled"})
        );
        assert_eq!(
            wire["entries"]["cancel_write_recorder"]["2"]["checks"][0]["negative"]["observed"],
            json!({"verdict": "violated"}),
            "四路对照的结果按「期望 / 观测到」记"
        );
        assert_eq!(
            wire["entries"]["cancel_write_recorder"]["2"]["checks"][0]["positive"]["expected"],
            json!({"verdict": "satisfied"})
        );
        assert_eq!(
            wire["history"].as_array().unwrap().len(),
            7,
            "候选 2 + 验证 2 + 启用 3（首版 1 条，新版与被换下的旧版各 1 条）"
        );

        let round_tripped: ObserverLibrary = serde_json::from_value(wire).unwrap();
        assert_eq!(round_tripped, library);
        assert_eq!(
            round_tripped.enabled(&id).map(|(version, _)| version),
            Some(second)
        );
        assert_eq!(round_tripped.versions(&id).len(), 2);
        assert!(matches!(
            round_tripped.at(&id, first).unwrap().lifecycle,
            ObserverLifecycle::Superseded { by } if by == second
        ));
        assert_eq!(round_tripped.at(&id, second).unwrap().checks.len(), 1);
        assert_eq!(round_tripped.ids().count(), 1);
    }

    #[test]
    fn a_superseded_version_can_be_rolled_back_to_with_a_fresh_check() {
        let mut state = SemanticState::in_memory();
        let mut library = ObserverLibrary::default();
        let id = observer_id();
        observe(&mut state, "cache.writes", 1);
        let first = enable_first_version(&mut library, &mut state);

        let second = library
            .propose(
                &id,
                contract(&["cache.writes"]),
                200,
                "换测量范围".to_owned(),
            )
            .unwrap();
        library
            .start_verifying(&id, second, 210, "重跑独立检验".to_owned())
            .unwrap();
        library
            .enable(
                &mut state,
                &id,
                second,
                passing_check(220),
                230,
                "v2 四路全过".to_owned(),
            )
            .unwrap();

        // v1 更稳：带一份新的检验记录回退过去（被替代的版本允许复用，但检验要重新给）。
        let read = judge_reading(&mut state, "write_after_cancel", &[("cache.writes", 1)]);
        let (records, invalidation) = library
            .enable(
                &mut state,
                &id,
                first,
                passing_check(240),
                250,
                "回退到 v1".to_owned(),
            )
            .unwrap();

        assert_eq!(
            records
                .iter()
                .map(|record| (record.version, record.lifecycle))
                .collect::<Vec<_>>(),
            vec![
                (first, ObserverLifecycle::Enabled),
                (second, ObserverLifecycle::Superseded { by: first }),
            ],
            "回退把被替代的旧版本重新设为当前版本，新版本让位"
        );
        assert_eq!(
            invalidation.expect("替代 v2 一定有失效传播").cause,
            InvalidationCause::ObserverInvalidated {
                observer: id.clone(),
                version: second,
            }
        );
        assert_eq!(
            library.enabled(&id).map(|(version, _)| version),
            Some(first)
        );
        assert_eq!(library.versions(&id).len(), 2, "回退不产生新版本号");
        assert_eq!(
            library.at(&id, first).unwrap().checks.len(),
            2,
            "首版 1 条 + 回退这次 1 条"
        );
        assert_eq!(state.recheck_set(), vec![read]);
    }

    #[test]
    fn a_hand_edited_library_with_two_enabled_versions_invalidates_both() {
        let mut state = SemanticState::in_memory();
        let mut library = ObserverLibrary::default();
        let id = observer_id();
        observe(&mut state, "cache.writes", 1);
        observe(&mut state, "scheduler.events", 1);
        let first = enable_first_version(&mut library, &mut state);

        let second = library
            .propose(
                &id,
                contract(&["scheduler.events"]),
                200,
                "换测量对象".to_owned(),
            )
            .unwrap();
        library
            .start_verifying(&id, second, 210, "重跑独立检验".to_owned())
            .unwrap();

        // 入库文件是可信输入：手工把 v2 也改成启用，造出「同 ID 两个启用版本」的注入态
        // （写入路径走不到，只有手改库文件能造出来）。
        let mut wire = serde_json::to_value(&library).unwrap();
        wire["entries"]["cancel_write_recorder"]["2"]["lifecycle"] =
            json!({ "lifecycle": "enabled" });
        let mut library: ObserverLibrary = serde_json::from_value(wire).unwrap();
        assert_eq!(
            library.at(&id, first).unwrap().lifecycle,
            ObserverLifecycle::Enabled
        );
        assert_eq!(
            library.at(&id, second).unwrap().lifecycle,
            ObserverLifecycle::Enabled
        );

        // 两个旧版本各有自己的读者。
        let reads_writes = judge_reading(&mut state, "write_after_cancel", &[("cache.writes", 1)]);
        let reads_events =
            judge_reading(&mut state, "cancel_confirmed", &[("scheduler.events", 1)]);

        let third = library
            .propose(&id, contract(&["cache.writes"]), 300, "再换一版".to_owned())
            .unwrap();
        library
            .start_verifying(&id, third, 310, "独立检验".to_owned())
            .unwrap();
        let (records, invalidation) = library
            .enable(
                &mut state,
                &id,
                third,
                passing_check(320),
                330,
                "四路全过".to_owned(),
            )
            .unwrap();

        assert_eq!(
            records
                .iter()
                .map(|record| (record.version, record.lifecycle))
                .collect::<Vec<_>>(),
            vec![
                (third, ObserverLifecycle::Enabled),
                (first, ObserverLifecycle::Superseded { by: third }),
                (second, ObserverLifecycle::Superseded { by: third }),
            ],
            "两个启用版本都让位"
        );
        let invalidation = invalidation.expect("替代了版本就一定有待失效的东西");
        assert_eq!(
            invalidation.cause,
            InvalidationCause::ObserverInvalidated {
                observer: id.clone(),
                version: first,
            },
            "起因记第一个退出使用的版本，逐版本轨迹在 history 里"
        );
        assert_eq!(
            invalidation.stale,
            vec![reads_writes, reads_events],
            "两个退出使用的版本各自的读者都不漏"
        );
        assert_eq!(
            invalidation.paths.len(),
            invalidation.stale.len(),
            "合并之后 paths 仍与 stale 一一对应"
        );
        assert_eq!(state.recheck_set(), vec![reads_writes, reads_events]);
        assert_eq!(
            library.enabled(&id).map(|(version, _)| version),
            Some(third)
        );
    }
}
