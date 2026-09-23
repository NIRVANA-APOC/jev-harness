//! 显式错误。
//!
//! 本 crate 不实现重试、降级与兜底（`docs/roadmap-1.0.md` §8）：失败一律以 [`Error`] 返回，
//! 由调用方决定继续、记录还是中止。通道专属的错误细节（HTTP 状态码、限流头、超时预算等）
//! 留在各通道 crate，由 `jh-app` 的适配层折进这里的三个判断器类别。
//!
//! 变体分两组：前三类描述**判断调用**的失败（[`Judge`](crate::Judge) 的契约），后几类描述
//! **运行时对象**的失败——日志读写不了（证据日志与反例日志共用 [`Error::Storage`]）、引用了不存在的
//! 证据 / 节点 / 行动、往已撤回的节点写结果、问题版本冲突、问题版本不存在、候选问题没有消费者、
//! 生命周期迁移不合法（问题库与观测器库各一套）、观测器没有测量对象或没通过独立检验。两组都是显式
//! 失败：没有哪一类会被悄悄吞掉、跳过或折成默认值。

use thiserror::Error;

use crate::evidence::EvidenceId;
use crate::graph::{ActionId, NodeId};
use crate::library::Lifecycle;
use crate::observer::{ObserverId, ObserverLifecycle, ObserverVersion};
use crate::question::{QuestionId, QuestionVersion};

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
    /// 日志读写失败：文件打不开 / 写不进，或某一行不是合法记录。
    ///
    /// 证据日志（[`EvidenceLog`](crate::EvidenceLog)）与反例日志
    /// （[`CounterexampleLog`](crate::CounterexampleLog)）共用这一档——报错文案因此不点名是哪一种日志，
    /// 具体是哪个文件写在细节里。
    #[error("日志不可用：{0}")]
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
    /// 引用了不存在的问题版本。
    #[error("问题版本不存在：{0} v{1}")]
    UnknownQuestionVersion(QuestionId, QuestionVersion),
    /// 候选问题没有声明下游消费者（研究目标 §6.3「没有消费者的问题不默认常驻」）。
    ///
    /// 库不记这条候选、也不给它分配版本号：先补上消费者，或别提。
    #[error("候选问题没有下游消费者：{0}")]
    QuestionWithoutConsumer(QuestionId),
    /// 问题库的生命周期迁移不合法：该版本当前的状态不允许这个操作。
    ///
    /// 允许的迁移见 [`QuestionLibrary`](crate::QuestionLibrary) 的迁移表。
    #[error("问题 `{question}` v{version} 当前的生命周期是「{lifecycle}」，不允许这次迁移")]
    InvalidLifecycle {
        /// 问题标识。
        question: QuestionId,
        /// 版本。
        version: QuestionVersion,
        /// 该版本当前的状态。
        lifecycle: Lifecycle,
    },
    /// 引用了不存在的观测器版本。
    #[error("观测器版本不存在：{0} v{1}")]
    UnknownObserverVersion(ObserverId, ObserverVersion),
    /// 候选观测器的契约没有声明测量对象（研究目标 §7.2 第 3 条）。
    ///
    /// 库不记这条候选、也不给它分配版本号：没有测量对象的观测器在失效传播里没有挂点，撤回它时
    /// 找不到该重查哪些判断——先补上对象，或别提。
    #[error("候选观测器没有声明测量对象：{0}")]
    ObserverWithoutObject(ObserverId),
    /// 观测器库的生命周期迁移不合法：该版本当前的状态不允许这个操作。
    ///
    /// 允许的迁移见 [`ObserverLibrary`](crate::ObserverLibrary) 的迁移表。
    #[error("观测器 `{observer}` v{version} 当前的生命周期是「{lifecycle}」，不允许这次迁移")]
    InvalidObserverLifecycle {
        /// 观测器标识。
        observer: ObserverId,
        /// 版本。
        version: ObserverVersion,
        /// 该版本当前的状态。
        lifecycle: ObserverLifecycle,
    },
    /// 观测器未通过独立检验，不能启用（研究目标 §7.3 末段）。
    ///
    /// 检验记录已经落库（试过并失败也是凭据），`detail` 说明是哪一路没对上；生命周期保持原状。
    #[error("观测器 `{observer}` v{version} 未通过独立检验，不能启用：{detail}")]
    ObserverNotVerified {
        /// 观测器标识。
        observer: ObserverId,
        /// 版本。
        version: ObserverVersion,
        /// 没通过的那一路，以及期望与观测到的判定。
        detail: String,
    },
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

    #[test]
    fn question_library_failures_name_the_version_or_the_missing_consumer() {
        let question = QuestionId::from("cause_b_pair");
        assert!(
            Error::UnknownQuestionVersion(question.clone(), QuestionVersion(9))
                .to_string()
                .contains("cause_b_pair v9")
        );
        assert!(
            Error::QuestionWithoutConsumer(question.clone())
                .to_string()
                .contains("cause_b_pair")
        );
        let invalid = Error::InvalidLifecycle {
            question,
            version: QuestionVersion(2),
            lifecycle: Lifecycle::Retired,
        };
        assert!(invalid.to_string().contains("cause_b_pair"), "{invalid}");
        assert!(invalid.to_string().contains("v2"), "{invalid}");
        assert!(invalid.to_string().contains("停用"), "{invalid}");
    }

    #[test]
    fn observer_failures_name_the_object_and_the_failed_check() {
        let observer = ObserverId::from("cancel_write_recorder");
        assert!(
            Error::UnknownObserverVersion(observer.clone(), ObserverVersion(3))
                .to_string()
                .contains("cancel_write_recorder v3")
        );
        assert!(
            Error::ObserverWithoutObject(observer.clone())
                .to_string()
                .contains("cancel_write_recorder")
        );

        let invalid = Error::InvalidObserverLifecycle {
            observer: observer.clone(),
            version: ObserverVersion(1),
            lifecycle: ObserverLifecycle::Candidate,
        };
        assert!(invalid.to_string().contains("候选"), "{invalid}");
        assert!(invalid.to_string().contains("v1"), "{invalid}");

        let unverified = Error::ObserverNotVerified {
            observer,
            version: ObserverVersion(1),
            detail: "阴性对照未通过：期望「发现违反」，观测到「命题成立」".to_owned(),
        };
        assert!(unverified.to_string().contains("不能启用"), "{unverified}");
        assert!(unverified.to_string().contains("阴性对照"), "{unverified}");
    }
}
