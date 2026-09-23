//! 会话内审批策略：白名单（「本次会话总是允许」）与授权模式文案。
//!
//! 审批卡本身在 `jh-tui`（`components/approval.rs`，单槽位模态）；这里只放**裁决的会话语义**：
//!
//! - 「允许一次」：只对这一次生效，不记名；
//! - 「本次会话总是允许」：工具名进白名单，本会话后续同类调用不再上卡（[`ApprovalPolicy::is_preapproved`]）；
//! - 「拒绝」：不执行；上屏与回话由 [`crate::session::agent`] 负责（`docs/tui-design.md` §2.6
//!   的「拒绝要有出路」）。
//!
//! 白名单按**工具名**记：这就是「本次会话总是允许 bash」的字面意思。提示框信息行的「授权模式」
//! 段（设计 §5.1 的 `模型 · 判定器 · 授权模式`）由 [`ApprovalPolicy::mode_label`] 给。

use std::collections::BTreeSet;

use jh_tui::app::ApprovalOutcome;

/// 本次会话的审批策略。
#[derive(Debug, Default)]
pub struct ApprovalPolicy {
    allowed: BTreeSet<String>,
}

impl ApprovalPolicy {
    /// 空白名单：每次都要问。
    pub fn new() -> Self {
        ApprovalPolicy::default()
    }

    /// 这个工具是否已被「本次会话总是允许」。
    pub fn is_preapproved(&self, tool: &str) -> bool {
        self.allowed.contains(tool)
    }

    /// 记下一次裁决。
    pub fn record(&mut self, tool: &str, outcome: ApprovalOutcome) {
        if outcome == ApprovalOutcome::AllowAlways {
            self.allowed.insert(tool.to_owned());
        }
    }

    /// 白名单里的工具名（按名字排序）。
    pub fn allowed(&self) -> impl Iterator<Item = &str> {
        self.allowed.iter().map(String::as_str)
    }

    /// 提示框信息行里的「授权模式」段。
    pub fn mode_label(&self) -> String {
        let names: Vec<&str> = self.allowed().collect();
        match names.len() {
            0 => "每次询问".to_owned(),
            1 => format!("白名单 {}", names[0]),
            2 => format!("白名单 {}、{}", names[0], names[1]),
            count => format!("白名单 {count} 项"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_always_allow_enters_the_whitelist() {
        let mut policy = ApprovalPolicy::new();
        assert!(!policy.is_preapproved("bash"));
        assert_eq!(policy.mode_label(), "每次询问");

        policy.record("bash", ApprovalOutcome::AllowOnce);
        assert!(!policy.is_preapproved("bash"), "允许一次不该记名");

        policy.record("bash", ApprovalOutcome::AllowAlways);
        assert!(policy.is_preapproved("bash"));
        assert!(!policy.is_preapproved("read_file"));
        assert_eq!(policy.mode_label(), "白名单 bash");

        policy.record("read_file", ApprovalOutcome::AllowAlways);
        assert_eq!(policy.mode_label(), "白名单 bash、read_file");
        policy.record("search", ApprovalOutcome::AllowAlways);
        policy.record("list_dir", ApprovalOutcome::AllowAlways);
        assert_eq!(policy.mode_label(), "白名单 4 项");

        // 拒绝不动白名单。
        policy.record("bash", ApprovalOutcome::Deny);
        assert!(policy.is_preapproved("bash"));
    }
}
