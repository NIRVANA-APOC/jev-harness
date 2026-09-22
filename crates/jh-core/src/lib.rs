//! jev-harness 的运行时对象模型与判断器抽象。
//!
//! 本 crate 承载「决策」层的抽象：判断器接收一份 state 与一组**带版本的类型化问题**，返回
//! **类型化答案 + 概率/置信度 + usage**，失败时返回显式错误。它不认识任何具体模型通道——
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
//!   阈值与来源标签一起进 [`JudgmentRecord`]（`docs/RESEARCH_GOALS.md` §5.2；`docs/roadmap-1.0.md` §4 #3）。
//!
//! # 本 crate 刻意不做的事
//!
//! - 不依赖 `jh-jev` / `jh-openai`：对象模型不绑死在某个判断器上，通道 wire 类型在适配层转换；
//! - 不重试、不降级、不缓存、不判阈值：阈值折成行动是消费方的事，本 crate 只保存原值与所用阈值；
//! - 不做持久化与事件日志：落盘形态由 `jh-app` 决定（`docs/roadmap-1.0.md` §12 未确定项 2）；
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
mod error;
mod judge;
mod question;
mod record;
mod state;

pub use answer::{Answer, ChoiceAnswer, NoulAnswer, ScoreAnswer, Usage};
pub use error::Error;
pub use judge::{Judge, JudgeRequest, JudgeResponse};
pub use question::{
    ChoiceQuestion, NoulCriteria, NoulQuestion, Question, QuestionId, QuestionSpec,
    QuestionVersion, Questions, ScoreQuestion,
};
pub use record::{JudgeSource, Judgment, JudgmentRecord};
pub use state::{SnapshotId, State, StateEntry};
