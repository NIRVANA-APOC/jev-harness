//! 五种判断器：同一 [`jh_core::Judge`] 接口，按名字构造、可配置切换。
//!
//! | 名字 | 来源标签 | 依据 |
//! | --- | --- | --- |
//! | `rule` | `Rule` | 确定性规则，不调模型 |
//! | `replay` | `Replay` | 复用已落盘的判断记录 |
//! | `simulated-truth` | `SimulatedTruth` | 任务集隐藏真值，只作诊断与上限估计 |
//! | `jev` | `RealApi` | `jh_jev::Client::system_one` 真调，模型 pin `jev-1.13.0` |
//! | `llm` | `Llm` | `jh_openai` 的 Responses 通道 + 自带 JSON 契约 |
//!
//! 构造失败（缺凭据、回放文件不存在）一律显式失败，不在实现内部退到别的判断器：
//! 「降级 = 换后端」指的是调用方换一个 [`JudgeName`]（`docs/roadmap-1.0.md` §3.2）。

use std::path::Path;

use clap::ValueEnum;
use jh_core::{Judge, JudgeSource};

use crate::error::Error;
use crate::tasks::TaskSet;

mod jev;
pub(crate) mod llm;
mod replay;
mod rule;
mod simulated;

pub use jev::JevJudge;
pub use llm::LlmJudge;
pub use replay::ReplayJudge;
pub use rule::RuleJudge;
pub use simulated::SimulatedTruthJudge;

/// 判断器的名字：CLI 取值、来源标签与构造入口共用一处。
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum JudgeName {
    /// 确定性规则，不调模型。
    Rule,
    /// 录制回放：复用已落盘的判断记录，不发起调用。
    Replay,
    /// 模拟真值：只用于诊断与上限估计。
    SimulatedTruth,
    /// Jev 真调（环境变量 `TYPESAFE_API_KEY`）。
    Jev,
    /// LLM 结构化判断（环境变量 `OPENAI_API_KEY` 与 `OPENAI_MODEL`）。
    Llm,
}

impl JudgeName {
    /// 无 key 时的离线默认组合：规则基线 + 模拟真值上限。
    pub const OFFLINE_DEFAULT: [Self; 2] = [Self::Rule, Self::SimulatedTruth];

    /// CLI 里使用的名字；与 [`ValueEnum`] 的取值保持一致（有测试守着）。
    pub fn cli_name(self) -> &'static str {
        match self {
            Self::Rule => "rule",
            Self::Replay => "replay",
            Self::SimulatedTruth => "simulated-truth",
            Self::Jev => "jev",
            Self::Llm => "llm",
        }
    }

    /// 该判断器在判断记录里声明的来源标签。
    pub fn source(self) -> JudgeSource {
        match self {
            Self::Rule => JudgeSource::Rule,
            Self::Replay => JudgeSource::Replay,
            Self::SimulatedTruth => JudgeSource::SimulatedTruth,
            Self::Jev => JudgeSource::RealApi,
            Self::Llm => JudgeSource::Llm,
        }
    }

    /// 是否发起真实网络调用。
    pub fn live(self) -> bool {
        matches!(self, Self::Jev | Self::Llm)
    }
}

/// 构造判断器所需的外部材料。
pub struct JudgeContext<'a> {
    /// 任务集：模拟真值判断器从这里取隐藏真值。
    pub task_set: &'a TaskSet,
    /// 录制回放读取的调用记录文件。
    pub replay_path: &'a Path,
}

/// 按名字构造判断器：五种判断器同一个接口，配置只是换一个名字。
///
/// 缺凭据、缺回放文件等一律以 [`jh_core::Error::Unavailable`] 显式失败——调用方据此把该判断器
/// 标成「未验证」，而不是拿别的判断器的结果顶上。
pub fn build(name: JudgeName, context: &JudgeContext<'_>) -> Result<Box<dyn Judge>, Error> {
    Ok(match name {
        JudgeName::Rule => Box::new(RuleJudge::new()),
        JudgeName::Replay => Box::new(ReplayJudge::load(context.replay_path)?),
        JudgeName::SimulatedTruth => {
            Box::new(SimulatedTruthJudge::new(context.task_set.truth_map()))
        }
        JudgeName::Jev => Box::new(JevJudge::from_env()?),
        JudgeName::Llm => Box::new(LlmJudge::from_env()?),
    })
}

/// 由概率分布推导 confidence：`1 − H(p) / ln(支撑集大小)`。
///
/// 服务端不为 LLM / 规则判断器给 confidence（`jh_jev` 的 confidence 由 Jev 服务端定义），
/// 本适配层用这个声明的、可复现的口径推导：分布越集中越接近 1，均匀分布为 0。
/// 支撑集只有一项时为 1（没有不确定性可言）。口径同时写进报告的 `metric_definitions.confidence`。
pub(crate) fn derived_confidence(
    probabilities: impl IntoIterator<Item = f64>,
    support: usize,
) -> f64 {
    if support < 2 {
        return 1.0;
    }
    let entropy: f64 = probabilities
        .into_iter()
        .filter(|probability| *probability > 0.0)
        .map(|probability| -probability * probability.ln())
        .sum();
    (1.0 - entropy / (support as f64).ln()).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_cli_name_matches_its_value_enum_spelling() {
        for name in JudgeName::value_variants() {
            let rendered = name
                .to_possible_value()
                .expect("每个变体都应有 CLI 取值")
                .get_name()
                .to_owned();
            assert_eq!(rendered, name.cli_name());
        }
    }

    #[test]
    fn live_flag_marks_only_the_two_real_channels() {
        assert!(JudgeName::Jev.live());
        assert!(JudgeName::Llm.live());
        assert!(!JudgeName::Rule.live());
        assert!(!JudgeName::Replay.live());
        assert!(!JudgeName::SimulatedTruth.live());
    }

    #[test]
    fn derived_confidence_spans_zero_to_one() {
        // 四项均匀分布：最大不确定性。
        assert!(derived_confidence([0.25, 0.25, 0.25, 0.25], 4).abs() < 1e-12);
        // one-hot：没有不确定性。
        assert!((derived_confidence([1.0, 0.0, 0.0, 0.0], 4) - 1.0).abs() < 1e-12);
        // 单一支撑集：定义为 1，且不出现除零。
        assert_eq!(derived_confidence([1.0], 1), 1.0);
    }
}
