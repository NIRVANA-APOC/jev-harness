//! jh-app 的显式错误。
//!
//! 判断器失败原样透传 [`jh_core::Error`]（不可用 / 调用失败 / 响应不可用三类），
//! 本地读写与任务集问题各占一类。不重试、不降级、不兜底（`docs/roadmap-1.0.md` §8）。

use thiserror::Error;

/// jh-app 可能显式失败的四种情况。
#[derive(Debug, Error)]
pub enum Error {
    /// 任务集不可用：JSON 不合法，或真值缺失、真值与问题不匹配。
    #[error("任务集不可用：{0}")]
    TaskSet(String),
    /// 本地读写失败：调用记录、回放文件、报告。
    #[error("本地读写失败：{0}")]
    Io(#[from] std::io::Error),
    /// JSON 编解码失败。
    #[error("JSON 编解码失败：{0}")]
    Json(#[from] serde_json::Error),
    /// 判断器失败；三类错误的意义见 [`jh_core::Error`]。
    #[error(transparent)]
    Judge(#[from] jh_core::Error),
}
