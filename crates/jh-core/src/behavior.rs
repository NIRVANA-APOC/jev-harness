//! 行为分类：`Execute` / `Observe` / `BuildObserver` / `Deliberate`（研究目标 §7.1）。
//!
//! 研究目标 §7.1 的原话是：运行时把这四类行为区分开，**分类服务于调度和成本核算，不限定所有未来
//! 行为只能属于这四类**。于是这个类型有两个用途、一条约束：
//!
//! - 用途：给一次循环步骤贴标签，调度与成本核算按标签分桶（「观测花了多少」「建设观测器花了多少」
//!   要分得开——H4 的成本摊薄曲线就建在这上面）；
//! - 用途：报告里可读地印出来（[`Behavior`] 的 `Display` 给中文标签，JSON 走 serde 的 `snake_case`）；
//! - 约束：[`Behavior::Other`] 是出口。四类之外的行为带着自己的标签进账，不必先挤进四类之一才能被
//!   记录，也不需要在将来改这个枚举——分类不封闭。
//!
//! 四类在本 crate 里各自的落点（本模块不调度、不记账，只给标签）：
//!
//! | 分类 | 做什么 | 本 crate 的落点 |
//! | --- | --- | --- |
//! | [`Execute`](Behavior::Execute) | 执行已有动作 | [`Action`](crate::Action) / [`SemanticState::check_action`](crate::SemanticState::check_action) |
//! | [`Observe`](Behavior::Observe) | 用已有渠道取证据 | [`SemanticState::append_evidence`](crate::SemanticState::append_evidence) |
//! | [`BuildObserver`](Behavior::BuildObserver) | 建设新的观察渠道 | [`ObserverLibrary`](crate::ObserverLibrary)（候选 → 验证 → 启用） |
//! | [`Deliberate`](Behavior::Deliberate) | 请求开放式推理或生成 | [`Judge`](crate::Judge) |
//!
//! 例：给一次循环步骤贴标签。
//!
//! ```
//! use jh_core::Behavior;
//!
//! // 四类之内。
//! let step = Behavior::Observe;
//! assert_eq!(step.to_string(), "观测");
//! assert_eq!(serde_json::to_string(&step).unwrap(), r#"{"behavior":"observe"}"#);
//!
//! // 四类之外：不改造枚举，也不硬塞进某一类。
//! let step = Behavior::Other("compress-context".to_owned());
//! assert_eq!(step.to_string(), "compress-context");
//! assert_eq!(
//!     serde_json::to_string(&step).unwrap(),
//!     r#"{"behavior":"other","detail":"compress-context"}"#
//! );
//! ```

use std::fmt;

use serde::{Deserialize, Serialize};

/// 运行时行为的一类（研究目标 §7.1）。
///
/// 枚举不封闭：未来行为的落点是 [`Behavior::Other`]，它不参与四类的调度与成本口径，但仍然可以被
/// 记录与计价。分类描述的是「这一步在做什么」，不是「它属于哪个模块」。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "behavior", content = "detail")]
pub enum Behavior {
    /// 执行已有动作：调用已经存在的能力，不产生新的观察渠道，也不要求开放式推理。
    Execute,
    /// 获取已有渠道的证据：用现成的工具 / 观测方法取一份材料。
    Observe,
    /// 建设新的观察渠道：造出、检验并启用一个新的观测器（研究目标 §7 的核心目标）。
    BuildObserver,
    /// 请求开放式推理或生成：交给模型自由作答的那类步骤（不是类型化判断）。
    Deliberate,
    /// 四类之外的行为，带调用方自己的标签。
    ///
    /// §7.1 明说「不限定所有未来行为只能属于这四类」：新行为先用这个出口记账，等它稳定到值得
    /// 单独统计时再考虑是否需要自己的类型。
    Other(String),
}

impl fmt::Display for Behavior {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Execute => f.write_str("执行"),
            Self::Observe => f.write_str("观测"),
            Self::BuildObserver => f.write_str("建设观测器"),
            Self::Deliberate => f.write_str("开放式推理"),
            Self::Other(label) => f.write_str(label),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_four_classes_round_trip_and_print_a_chinese_label() {
        let cases = [
            (Behavior::Execute, "执行"),
            (Behavior::Observe, "观测"),
            (Behavior::BuildObserver, "建设观测器"),
            (Behavior::Deliberate, "开放式推理"),
        ];
        for (behavior, label) in cases {
            assert_eq!(behavior.to_string(), label);
            let wire = serde_json::to_string(&behavior).unwrap();
            assert_eq!(serde_json::from_str::<Behavior>(&wire).unwrap(), behavior);
        }
        assert_eq!(
            serde_json::to_string(&Behavior::BuildObserver).unwrap(),
            r#"{"behavior":"build_observer"}"#
        );
    }

    #[test]
    fn a_behavior_outside_the_four_classes_keeps_its_own_label() {
        let behavior = Behavior::Other("compress-context".to_owned());
        assert_eq!(behavior.to_string(), "compress-context");
        assert_ne!(behavior, Behavior::Deliberate, "不挤进四类之一");
        let wire = serde_json::to_string(&behavior).unwrap();
        assert_eq!(
            serde_json::from_str::<Behavior>(&wire).unwrap(),
            behavior,
            "出口在 JSON 往返里也成立，报告不会丢标签"
        );
    }
}
