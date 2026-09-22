//! jh-app：组装层与 CLI。
//!
//! 本 crate 把 [`jh_core`] 的判断器抽象接到具体通道上，并提供 M0 的实验设施
//! （`docs/roadmap-1.0.md` §5 M0）与 M1 的固定问题运行时（同文件 §5 M1）：
//!
//! - [`judge`]：五种判断器适配同一接口、可配置切换——Jev 真调（`jh-jev`）、LLM 结构化判断
//!   （`jh-openai`）、确定性规则、录制回放、模拟真值；
//! - [`tasks`]：初始任务集（中文合成样本：state + 类型化问题 + 隐藏真值）；
//! - [`calls`]：调用记录落盘（JSONL：来源标签、版本、延迟、usage、request_id）；
//! - [`metrics`]：一致率、Brier、对数损失与延迟分位数的口径实现；
//! - [`report`] 与 [`exp`]：`jev exp` 的运行与 JSON 基线报告；
//! - [`family`]：任务族 A 的会变化故障环境——快照、可控扰动、隐藏真值、独立验收、可计价动作，
//!   外加绑定它的 M1 固定问题集（问题带版本与输入范围）；模拟真值不参与本族对照；
//! - [`matching`]：匹配循环基线——装配 state（只用 `current_evidence`）→ 调判定器 → 落判断记录
//!   → 传播失效 → 依据有效性检查 → 决定行动；H1/H2 两个开关各只有一处分支；
//! - [`compare`]：H1（依赖失效传播开 / 关）× H2（增量 / 全量）四组臂的对照运行与 JSON 报告，
//!   含 `metric_definitions`、真实与本地来源的区分、原始数据位置与复现命令。
//!
//! # 边界
//!
//! - `jh-jev` / `jh-openai` / `jh-tui` 的公共 API 冻结，本 crate 只做适配与组装；
//! - key 只经环境变量（`TYPESAFE_API_KEY` / `OPENAI_API_KEY`），不入库、不进日志与报告；
//! - 没有 key 时真实通道的结论一律标「未验证」，不得用回放或模拟结果替代
//!   （`docs/RESEARCH_GOALS.md` §12 M0 的未达标处理）；
//! - 模拟真值只用于诊断与上限估计，记录与报告用 `source` 标签把它与真实调用区分开
//!   （`docs/RESEARCH_GOALS.md` §9.2、§10.2）；它不出现在任务族 A 的对照里。

#![warn(missing_docs)]

pub mod calls;
pub mod compare;
pub mod error;
pub mod exp;
pub mod family;
pub mod judge;
pub mod matching;
pub mod metrics;
pub mod report;
pub mod tasks;

pub use error::Error;
