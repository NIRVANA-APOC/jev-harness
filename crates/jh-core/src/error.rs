//! 判断器的显式错误。
//!
//! 本 crate 不实现重试、降级与兜底（`docs/roadmap-1.0.md` §8）：失败一律以 [`Error`] 返回，
//! 由调用方决定继续、记录还是中止。通道专属的错误细节（HTTP 状态码、限流头、超时预算等）
//! 留在各通道 crate，由 `jh-app` 的适配层折进这里的三个类别。

use thiserror::Error;

/// 判断器调用可能显式失败的三种情况。
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum Error {
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
}
