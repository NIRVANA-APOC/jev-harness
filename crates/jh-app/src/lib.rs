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
//!   含 `metric_definitions`、真实与本地来源的区分、原始数据位置与复现命令；
//! - [`family_b`]：任务族 B 的成对场景（证书过期 vs 时钟偏差等 3 对 × 每对 2 个未见变体），
//!   B1（关键证据已给）与 B2（需计费观察取得）分开；
//! - [`gen`]：候选问题生成（真实生成模型 + §6.3 八字段契约的严格解析与协议核对）；
//! - [`validation`]：候选在开发案例、正例 / 反例与不同表述上的检验；
//! - [`shadow`]：影子运行——新问题参与判断、不进决策，读「若采纳会不会改变有价值的行动分支」；
//! - [`promotion`]：预声明的晋升标准、发布联动失效、四路变更记录与可核边界、回退演练；
//! - [`h3`]：H3 对照（固定问题集合 vs 允许精化）+ 任务族 B 的 B1/B2 分开报告 + 成本账与复现命令；
//! - [`c_program`]：隔离实验环境中的可控程序——宿主钩子点（写入边界记录器、可控暂停点、提交计数器）
//!   与**只读存储日志**的隐藏检查；任务族 C 的三个故障族与每族 10 条确定性调度；
//! - [`c_measure`]：声明式测量契约（真 LLM 生成 + 严格解析 + 协议核对）、按契约装通道的观测器、
//!   四路独立检验（含「自证式的伪隐藏检查会被门槛拒绝」）与通用测试生成臂的断言编译；
//! - [`h4`]：H4 三臂对照（固定测试 / 通用测试生成 / 针对缺口建设观测器）× 任务族 C × N=10，
//!   含误放行 / 误报 / 迁移矩阵 / 维护开销 / 建设成本摊薄曲线与累积能力假说的如实结论；
//! - [`m4`]：M4 综合评估——主对照 2×2（控制结构 × 判断通道）、相同新增证据的诊断对照、原生基线臂、
//!   两条残余消融（禁止跨任务复用、固定频率检查）与密度扫描，读数按研究目标 §10.1 / §10.2 采集。
//!
//! # 边界
//!
//! - `jh-jev` / `jh-openai` / `jh-tui` 的公共 API 冻结，本 crate 只做适配与组装；
//! - key 只经环境变量（`TYPESAFE_API_KEY` / `OPENAI_API_KEY`），不入库、不进日志与报告；
//! - 没有 key 时真实通道的结论一律标「未验证」，不得用回放或模拟结果替代
//!   （`docs/RESEARCH_GOALS.md` §12 M0 的未达标处理）；
//! - 模拟真值只用于诊断与上限估计，记录与报告用 `source` 标签把它与真实调用区分开
//!   （`docs/RESEARCH_GOALS.md` §9.2、§10.2）；它不出现在任务族 A 与任务族 B 的对照里。
#![warn(missing_docs)]

pub mod c_measure;
pub mod c_program;
pub mod calls;
pub mod compare;
pub mod error;
pub mod exp;
pub mod family;
pub mod family_b;
pub mod generation;
pub mod h3;
pub mod h4;
pub mod judge;
pub mod m4;
pub mod matching;
pub mod metrics;
pub mod promotion;
pub mod report;
pub mod session;
pub mod shadow;
pub mod tasks;
pub mod validation;

pub use error::Error;
