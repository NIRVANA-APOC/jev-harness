//! 显式错误。
//!
//! 本 crate 不实现重试、降级与兜底（`docs/roadmap-1.0.md` §8）：失败一律以 [`Error`] 返回，
//! 由调用方决定继续、记录还是中止。通道专属的错误细节（HTTP 状态码、限流头、超时预算等）
//! 留在各通道 crate，由 `jh-app` 的适配层折进这里的三个判断器类别。
//!
//! 变体分两组：前三类描述**判断调用**的失败（[`Judge`](crate::Judge) 的契约），后几类描述
//! **运行时对象**的失败——证据日志读写不了、引用了不存在的证据 / 节点 / 行动、问题版本冲突。
//! 两组都是显式失败：没有哪一类会被悄悄吞掉、跳过或折成默认值。

use thiserror::Error;

use crate::evidence::EvidenceId;
use crate::graph::{ActionId, NodeId};
use crate::question::QuestionId;

/// 调用判断器或使用运行时对象时可能显式失败的情况。
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum Error {
    // ---- 判断调用 ----
    /// 判断器不可用：缺凭据、未配置或实现侧明确不支持；调用没有真正发生。
    #[error("判断器不可用：{0}")]
    Unavailable(String),
    /// 调用本身失败：网络、HTTP 错误、限流或超时；细节由所属通道给出。
    #[error("判断调用失败：{0}")]
    CallFailed(String),
    /// 响应不可用：答案缺失、答案原语与问题不符，或出现请求之外的问题 ID。
    ///
    /// 属于契约问题，不是暂时性故障：重试不会让它变好。
    #[error("判断响应不可用：{0}")]
    InvalidResponse(String),

    // ---- 运行时对象 ----
    /// 证据日志读写失败：文件打不开 / 写不进，或某一行不是合法记录。
    #[error("证据日志不可用：{0}")]
    Storage(String),
    /// 引用了一条不存在的证据。
    #[error("证据不存在：{0}")]
    UnknownEvidence(EvidenceId),
    /// 引用了一个不存在的判断节点。
    #[error("判断节点不存在：{0}")]
    UnknownNode(NodeId),
    /// 引用了一个不存在的行动。
    #[error("行动不存在：{0}")]
    UnknownAction(ActionId),
    /// 想往一个已撤回的判断节点里写结果。
    ///
    /// 撤销黏着到显式重新授权：要重新判断就声明一个新节点（判断节点本来就不可改写）。
    #[error("判断节点已撤销：{0}")]
    RetractedNode(NodeId),
    /// 问题版本冲突：同一 ID + 版本上已经记着另一份含义不同的定义。
    ///
    /// 改含义必须走 [`QuestionRegistry::revise`](crate::QuestionRegistry::revise) 产生新版本，
    /// 旧版本不可被覆盖。
    #[error("问题版本冲突：{0}")]
    QuestionVersionConflict(QuestionId),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_carry_their_own_message_and_are_std_errors() {
        fn assert_std_error<E: std::error::Error>() {}
        assert_std_error::<Error>();

        let error = Error::Unavailable("缺少 TYPESAFE_API_KEY".to_owned());
        assert!(error.to_string().contains("缺少 TYPESAFE_API_KEY"));
        assert!(
            Error::CallFailed("429".to_owned())
                .to_string()
                .contains("429")
        );
    }

    #[test]
    fn runtime_failures_name_the_object_they_are_about() {
        assert!(
            Error::UnknownEvidence(EvidenceId(3))
                .to_string()
                .contains("evidence-3")
        );
        assert!(Error::UnknownNode(NodeId(7)).to_string().contains("node-7"));
        assert!(
            Error::RetractedNode(NodeId(7))
                .to_string()
                .contains("node-7")
        );
        assert!(
            Error::UnknownAction(ActionId::from("restart-cache"))
                .to_string()
                .contains("restart-cache")
        );
        assert!(
            Error::QuestionVersionConflict(QuestionId::from("cache_disabled"))
                .to_string()
                .contains("cache_disabled")
        );
    }
}
