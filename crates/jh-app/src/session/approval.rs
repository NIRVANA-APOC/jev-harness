//! 会话内审批策略：三个授权模式 + 白名单（「本次会话总是允许」）。
//!
//! 审批卡本身在 `jh-tui`（`components/approval.rs`，单槽位模态）；这里只放**裁决的会话语义**：
//!
//! - 「允许一次」：只对这一次生效，不记名；
//! - 「本次会话总是允许」：工具名进白名单，本会话后续同类调用不再上卡（[`ApprovalPolicy::is_preapproved`]）；
//! - 「拒绝」：不执行；上屏与回话由 [`crate::session::agent`] 负责（`docs/tui-design.md` §2.6
//!   的「拒绝要有出路」）。
//!
//! 三个模式（`/permission` 切换，缺省 [`PermissionMode::Auto`]）：
//!
//! | 模式 | 门控 | 何时上卡 |
//! | --- | --- | --- |
//! | [`PermissionMode::Auto`] | 只读工具由确定性层直接放行，其余问 Jev，采纳即执行 | Jev 判不了（不可用 / 分布太接近 / 置信度低），或命令命中**破坏性**标记时 |
//! | [`PermissionMode::AlwaysAsk`] | **不问 Jev** | 除已「总是允许」的工具外，每次都上卡 |
//! | [`PermissionMode::NeverAsk`] | **不问 Jev** | 从不上卡，直接执行（风险分类照旧上屏与记账） |
//!
//! 白名单在三个模式里都先生效：卡片上的「总是允许」是用户自己的明确授权，不该被模式反过来作废。
//! `/permission reset` 同时清空白名单并把模式恢复成 `auto`。

use std::collections::BTreeSet;

use jh_tui::app::ApprovalOutcome;

/// 授权模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PermissionMode {
    /// 自动（缺省）：Jev 判定 + 破坏性命令兜底上卡。
    #[default]
    Auto,
    /// 每次都问：不问 Jev，除白名单外一律上卡。
    AlwaysAsk,
    /// 从不询问：不问 Jev、不上卡，直接执行。
    NeverAsk,
}

impl PermissionMode {
    /// 三种模式的固定顺序（`/permission` 的选项列表按它排）。
    pub const ALL: [PermissionMode; 3] = [
        PermissionMode::Auto,
        PermissionMode::AlwaysAsk,
        PermissionMode::NeverAsk,
    ];

    /// `/permission <参数>` 的解析（大小写不敏感，容忍别名）。
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(PermissionMode::Auto),
            "ask" | "always" | "always-ask" => Some(PermissionMode::AlwaysAsk),
            "never" | "never-ask" | "off" => Some(PermissionMode::NeverAsk),
            _ => None,
        }
    }

    /// 规范写法（`/permission` 回执、帮助与日志用）。
    pub fn as_str(self) -> &'static str {
        match self {
            PermissionMode::Auto => "auto",
            PermissionMode::AlwaysAsk => "ask",
            PermissionMode::NeverAsk => "never",
        }
    }

    /// 提示框信息行里的「授权模式」段。
    pub fn label(self) -> &'static str {
        match self {
            PermissionMode::Auto => "自动（Jev 判定）",
            PermissionMode::AlwaysAsk => "每次询问",
            PermissionMode::NeverAsk => "从不询问",
        }
    }
}

/// `/permission` 的用法串（回执与错误提示共用一份）。
pub const PERMISSION_USAGE: &str = "用法：/permission 看现状；/permission auto|ask|never 换模式；/permission reset 清空白名单并回到 auto";

/// 本次会话的审批策略。
#[derive(Debug, Default)]
pub struct ApprovalPolicy {
    mode: PermissionMode,
    allowed: BTreeSet<String>,
}

impl ApprovalPolicy {
    /// 空白名单 + 缺省模式（`auto`）。
    pub fn new() -> Self {
        ApprovalPolicy::default()
    }

    /// 当前模式。
    pub fn mode(&self) -> PermissionMode {
        self.mode
    }

    /// 换模式。
    pub fn set_mode(&mut self, mode: PermissionMode) {
        self.mode = mode;
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

    /// 清空白名单、模式回到 `auto`（`/permission reset`）。
    pub fn reset(&mut self) {
        self.allowed.clear();
        self.mode = PermissionMode::Auto;
    }

    /// 白名单里的工具名（按名字排序）。
    pub fn allowed(&self) -> impl Iterator<Item = &str> {
        self.allowed.iter().map(String::as_str)
    }

    /// 提示框信息行里的「授权模式」段：模式 + 白名单规模。
    pub fn mode_label(&self) -> String {
        let names: Vec<&str> = self.allowed().collect();
        let whitelist = match names.len() {
            0 => String::new(),
            1 => format!("，白名单 {}", names[0]),
            2 => format!("，白名单 {}、{}", names[0], names[1]),
            count => format!("，白名单 {count} 项"),
        };
        format!("{}{whitelist}", self.mode.label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_always_allow_enters_the_whitelist() {
        let mut policy = ApprovalPolicy::new();
        assert!(!policy.is_preapproved("bash"));
        assert_eq!(policy.mode_label(), "自动（Jev 判定）");

        policy.record("bash", ApprovalOutcome::AllowOnce);
        assert!(!policy.is_preapproved("bash"), "允许一次不该记名");

        policy.record("bash", ApprovalOutcome::AllowAlways);
        assert!(policy.is_preapproved("bash"));
        assert!(!policy.is_preapproved("read_file"));
        assert_eq!(policy.mode_label(), "自动（Jev 判定），白名单 bash");

        policy.record("read_file", ApprovalOutcome::AllowAlways);
        assert_eq!(
            policy.mode_label(),
            "自动（Jev 判定），白名单 bash、read_file"
        );
        policy.record("search", ApprovalOutcome::AllowAlways);
        policy.record("list_dir", ApprovalOutcome::AllowAlways);
        assert_eq!(policy.mode_label(), "自动（Jev 判定），白名单 4 项");

        // 拒绝不动白名单。
        policy.record("bash", ApprovalOutcome::Deny);
        assert!(policy.is_preapproved("bash"));
    }

    #[test]
    fn the_default_mode_is_auto() {
        assert_eq!(ApprovalPolicy::new().mode(), PermissionMode::Auto);
        assert_eq!(PermissionMode::default(), PermissionMode::Auto);
    }

    #[test]
    fn modes_parse_with_aliases_and_labels() {
        assert_eq!(PermissionMode::parse("auto"), Some(PermissionMode::Auto));
        assert_eq!(PermissionMode::parse(" AUTO "), Some(PermissionMode::Auto));
        assert_eq!(
            PermissionMode::parse("ask"),
            Some(PermissionMode::AlwaysAsk)
        );
        assert_eq!(
            PermissionMode::parse("always"),
            Some(PermissionMode::AlwaysAsk)
        );
        assert_eq!(
            PermissionMode::parse("never"),
            Some(PermissionMode::NeverAsk)
        );
        assert_eq!(PermissionMode::parse("off"), Some(PermissionMode::NeverAsk));
        assert_eq!(PermissionMode::parse("sometimes"), None);

        assert_eq!(PermissionMode::Auto.as_str(), "auto");
        assert_eq!(PermissionMode::AlwaysAsk.label(), "每次询问");
        assert_eq!(PermissionMode::NeverAsk.label(), "从不询问");
    }

    #[test]
    fn reset_clears_the_whitelist_and_returns_to_auto() {
        let mut policy = ApprovalPolicy::new();
        policy.set_mode(PermissionMode::NeverAsk);
        policy.record("bash", ApprovalOutcome::AllowAlways);
        policy.reset();
        assert_eq!(policy.mode(), PermissionMode::Auto);
        assert!(!policy.is_preapproved("bash"));
        assert_eq!(policy.mode_label(), "自动（Jev 判定）");
    }
}
