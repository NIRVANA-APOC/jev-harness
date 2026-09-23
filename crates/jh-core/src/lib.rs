//! jev-harness 的运行时对象模型、可失效语义状态与判断器抽象。
//!
//! 本 crate 承载「决策」层的抽象与**可失效的语义状态**：判断器接收一份 state 与一组**带版本的
//! 类型化问题**，返回**类型化答案 + 概率/置信度 + usage**，失败时返回显式错误；判断结果落成判断节点
//! 之后，输入变化会被传播成「哪些判断要重查、哪些行动不能发出」。它不认识任何具体模型通道——
//! `jh-jev` / `jh-openai` 的 wire 类型由 `jh-app` 的适配层转换（`docs/roadmap-1.0.md` §3.3）。
//!
//! # 设计依据
//!
//! - **抽象建在「决策」上，而不是「模型调用」上**：Jev / LLM 结构化判断 / 确定性规则 / 录制回放 /
//!   模拟器真值实现同一组接口，降级 = 换后端而不是改调用点
//!   （`docs/jev-research.md` §13.3.1；`docs/roadmap-1.0.md` §3.2）；
//! - **概率是一等数据**：[`ChoiceAnswer::probabilities`] / [`ChoiceAnswer::confidence`] /
//!   [`NoulAnswer::noul`] 原值保留、可 JSON 往返，不在类型层折成布尔
//!   （`docs/jev-research.md` §13.3.3）；
//! - **错误显式失败**：不重试、不降级、不兜底（`docs/roadmap-1.0.md` §8），失败一律是 [`Error`]；
//! - **判断记录可回放**：快照标识、问题版本、执行者版本、结果与分布、usage、`request_id`、延迟、
//!   阈值与来源标签一起进 [`JudgmentRecord`]（`docs/RESEARCH_GOALS.md` §5.2；`docs/roadmap-1.0.md` §4 #3）；
//! - **原始材料不可变、读取范围留痕**：证据只追加不改写，撤回也只是另记一笔；每个判断节点声明输入
//!   投影（读了哪些对象与字段），失效传播据此判定谁该重查（`docs/RESEARCH_GOALS.md` §5.2、§5.3）；
//! - **执行语义分层**：评估状态（有没有拿到一次有效响应）与依据有效性（还能不能作为依据）分开管；
//!   撤销只改变使用资格，已发生的外部副作用不自动回滚（`docs/RESEARCH_GOALS.md` §5.3）。
//!
//! # 可失效的语义状态
//!
//! - [`Evidence`] / [`EvidenceLog`]：追加写、不可变的原始记录（来源、时间、对象版本、内容摘要哈希），
//!   撤回只是另记一笔，记录本身永远不改；持久化只有内存与 JSONL 两种简单实现；
//! - [`InputProjection`]：每个判断声明读了哪些对象与字段；共享一份请求 state 的多个问题保守依赖该
//!   请求的全部输入；[`verify_snapshot`] 给延迟返回的结果做快照校验；
//! - [`DependencyGraph`]：读取关系、支持关系、冲突候选与行动前提四类边；确定性读取依赖与模型提出的
//!   语义关联分开记录；依赖环可检测；
//! - [`SemanticState`]：判断节点、[`EvaluationStatus`] 与 [`Validity`] 两个维度、失效传播，以及行动
//!   发出前的前提复核（[`SemanticState::check_action`]）。
//!
//! # 反例与问题库
//!
//! 研究目标 §6 那条慢回路由两组对象承载：
//!
//! - [`Counterexample`] / [`CounterexampleLog`]：失败的原始现场（失败来源、当时的证据、问题版本、
//!   候选动作、控制规则），以及 §6.1 四类候选归因；只记录，不自动判定归因；
//! - [`QuestionLibrary`]：新问题的最小契约（§6.3 八字段；证据需求在 1.0 只表达来源与字段）、
//!   生命周期（候选 → 影子评估 → 启用 → 停用 / 被替代）与版本号规则（见 [`QuestionLibrary`] 的模块
//!   文档）；改当前定义的 [adopt](QuestionLibrary::adopt) / [publish](QuestionLibrary::publish) /
//!   [retire](QuestionLibrary::retire) 都直接联动 [`SemanticState::invalidate_question`]，
//!   旧版本一律保留，可比较、可回退。
//!
//! # 观测能力建设与行为分类
//!
//! 研究目标 §7 那条能力在本 crate 里的这一层是**契约与生命周期**，不是观测器的实现：
//!
//! - [`Behavior`]：`Execute` / `Observe` / `BuildObserver` / `Deliberate` 四类，服务于调度与成本
//!   核算；分类不封闭，四类之外的行为走 [`Behavior::Other`]（§7.1）；
//! - [`ObserverContract`]：§7.2 的八条逐项落类型（原始要求及来源、命题与覆盖关系、测量位置与对象与
//!   时序、支持的版本配置与条件、副作用与扰动、阳性 / 阴性对照与可注入故障、输出格式与失败状态与
//!   失效条件、尚未覆盖）；
//! - [`ObserverLibrary`]：候选 → 验证 → 启用 → 停用 / 被替代，一个 ID 只有一个启用版本；**启用要求
//!   一份四路全过的独立检验记录**（[`ObserverCheck`]：阳性对照、阴性对照、可注入的已知故障、独立
//!   隐藏检查），没通过就显式失败、停在原状态（§7.3 末段；详见 [`ObserverLibrary`] 的模块文档）；
//! - 与失效联动：观测器停用或被替代时，读过它测量对象的判断待重查
//!   （[`SemanticState::invalidate_observer`] / [`InvalidationCause::ObserverInvalidated`]）。
//!
//! # 本 crate 刻意不做的事
//!
//! - 不依赖 `jh-jev` / `jh-openai`：对象模型不绑死在某个判断器上，通道 wire 类型在适配层转换；
//! - 不重试、不降级、不缓存、不判阈值：阈值折成行动是消费方的事，本 crate 只保存原值与所用阈值；
//! - 不判定反例归因、不判晋升标准：反例只记 §6.1 的四类可能，晋升阈值属于实验层；
//! - 不发动作、不回滚副作用：[`SemanticState`] 只回答「现在能不能发」，怎么发、发完怎么补偿由
//!   `jh-app` 决定；
//! - 不执行观测器、不生成观测器实现、不做沙箱或校验器 DSL：本 crate 只声明契约与生命周期，观测器
//!   怎么跑、在哪里跑由 `jh-app` 的隔离实验环境负责（`docs/roadmap-1.0.md` §8 的 1.0 不做项）；
//! - 不读时钟：证据与反例的时间、对象版本由调用方给出；问题库与观测器库的版本号由各自的库按定案
//!   规则分配（见 [`QuestionLibrary`]、[`ObserverLibrary`]）；
//! - 判断记录的落盘形态仍由 `jh-app` 决定（`var/calls.jsonl`，`docs/roadmap-1.0.md` §12 未确定项 2）；
//! - 不引入异步运行时：判断器接口返回装箱 future，由调用方自己的 executor 驱动。
//!
//! # 示例
//!
//! 从请求到判断记录的最小流程（判断器实现示例见 [`Judge`]）：
//!
//! ```no_run
//! use std::collections::BTreeMap;
//! use std::time::Duration;
//!
//! use jh_core::{
//!     Error, Judge, JudgeRequest, JudgmentRecord, NoulQuestion, Question, QuestionId,
//!     QuestionSpec, QuestionVersion, SnapshotId, State,
//! };
//!
//! # async fn run(judge: &dyn Judge) -> Result<(), Error> {
//! let question_id = QuestionId::from("cache_written_after_cancel");
//! let mut questions = BTreeMap::new();
//! questions.insert(
//!     question_id.clone(),
//!     QuestionSpec {
//!         version: QuestionVersion(1),
//!         question: Question::Noul(NoulQuestion {
//!             instructions: "取消确认之后是否还有同一任务的缓存写入？".to_owned(),
//!             criteria: None,
//!         }),
//!     },
//! );
//! let request = JudgeRequest {
//!     snapshot: SnapshotId::from("case-001"),
//!     state: State::from("09:00:01 cancel confirmed; 09:00:03 cache write"),
//!     questions,
//! };
//!
//! let response = judge.judge(&request).await?;
//! let mut record = JudgmentRecord::from_response(&request, &response, Duration::from_millis(12))?;
//! // 阈值属于消费方；需要留痕时写进记录。
//! record.judgments.get_mut(&question_id).expect("该问题应在记录里")
//!     .thresholds.insert("yes_min".to_owned(), 0.8);
//! # Ok(())
//! # }
//! ```

#![warn(missing_docs)]

mod answer;
mod behavior;
mod counterexample;
mod error;
mod evidence;
mod graph;
mod hash;
mod judge;
mod library;
mod observer;
mod projection;
mod question;
mod record;
mod runtime;
mod state;

pub use answer::{Answer, ChoiceAnswer, NoulAnswer, ScoreAnswer, Usage};
pub use behavior::Behavior;
pub use counterexample::{
    Attribution, Counterexample, CounterexampleId, CounterexampleLog, FailureSource,
    JsonlCounterexampleLog, MemoryCounterexampleLog, NewCounterexample,
};
pub use error::Error;
pub use evidence::{
    Evidence, EvidenceId, EvidenceLog, JsonlEvidenceLog, LogRow, MemoryEvidenceLog, NewEvidence,
    ObjectId, ObjectVersion,
};
pub use graph::{
    ActionId, DependencyGraph, Edge, EdgeKind, EdgeOrigin, NodeId, NodeRef, PropagationHop,
    PropagationPath,
};
pub use hash::ContentHash;
pub use judge::{Judge, JudgeRequest, JudgeResponse};
pub use library::{
    Lifecycle, LifecycleRecord, NewQuestion, QuestionEntry, QuestionLibrary, ValidationKind,
    ValidationRecord,
};
pub use observer::{
    CheckDirection, ControlResult, Controls, Disturbance, Measurement, ObserverCheck,
    ObserverContract, ObserverEntry, ObserverId, ObserverLibrary, ObserverLifecycle,
    ObserverLifecycleRecord, ObserverVerdict, OutputContract, Requirement, Support, Timing,
};
pub use projection::{
    Drift, FieldSelection, InputProjection, ObjectRead, SnapshotCheck, verify_snapshot,
};
pub use question::{
    ChoiceQuestion, NoulCriteria, NoulQuestion, Question, QuestionId, QuestionRegistry,
    QuestionSpec, QuestionVersion, Questions, ScoreQuestion, VersionComparison,
};
pub use record::{JudgeSource, Judgment, JudgmentRecord};
pub use runtime::{
    Action, EvaluationStatus, Invalidation, InvalidationCause, JudgmentNode, PremiseCheck,
    SemanticState, Validity,
};
pub use state::{SnapshotId, State, StateEntry};
