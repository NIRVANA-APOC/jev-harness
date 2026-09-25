//! 提示框里的斜杠命令：注册表、解析与帮助文案。
//!
//! 输入框里以 `/` 开头的整行是命令，不是发给模型的话（`App::apply(ActionId::Submit)` 分流）。
//! 命令分两类：
//!
//! - [`CommandScope::Ui`]：界面自己就能办（切屏、清屏、退出、帮助），`jh-tui` 直接执行；
//! - [`CommandScope::Host`]：要会话状态（模型通道、审批白名单、读数），折成 [`crate::app::Effect::Command`]
//!   交给宿主，宿主回一条结果上屏。
//!
//! 口径：
//! - 命令名大小写不敏感（`/QUIT` 等于 `/quit`）；参数原样保留（模型 ID 大小写有意义）。
//! - 参数用空白切开，只取第一个词做参数；多余部分按「参数的一部分」原样带上（`/model a b` 里
//!   模型 ID 就是 `a b` 吗？不是——见 [`Invocation::args`]：整段原样给出，由各命令自己解释）。
//! - 认不出的命令**不静默吞掉**：回一条带提示的通知，输入不再重放给模型。
//! - 输入框里敲命令名时，[`candidates`] 给出补全列表（同一张注册表，不另立一份名单）。

/// 命令的处理方。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandScope {
    /// 界面自处理。
    Ui,
    /// 交给宿主（会话状态在那边）。
    Host,
}

/// 一条命令的定义。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandSpec {
    /// 命令名，不含前导 `/`。
    pub name: &'static str,
    /// 用法串（帮助里原样显示）。
    pub usage: &'static str,
    /// 一句话说明。
    pub summary: &'static str,
    pub scope: CommandScope,
}

impl CommandSpec {
    /// 命令名之后能不能带参数（用法串里写了可选项）。
    ///
    /// 补全填入时据此决定要不要留一个尾空格：`/model` 要，`/help` 不要。
    pub fn takes_args(&self) -> bool {
        self.usage.trim_start_matches('/').trim() != self.name
    }
}

/// 全部命令，顺序即 `/help` 的显示顺序。
pub const COMMANDS: [CommandSpec; 11] = [
    CommandSpec {
        name: "help",
        usage: "/help",
        summary: "列出全部命令",
        scope: CommandScope::Ui,
    },
    CommandSpec {
        name: "quit",
        usage: "/quit",
        summary: "退出会话（等价于 Ctrl+C）",
        scope: CommandScope::Ui,
    },
    CommandSpec {
        name: "clear",
        usage: "/clear",
        summary: "清空回滚区（不动会话上下文）",
        scope: CommandScope::Ui,
    },
    CommandSpec {
        name: "research",
        usage: "/research",
        summary: "在会话与研究视图之间切换",
        scope: CommandScope::Ui,
    },
    CommandSpec {
        name: "model",
        usage: "/model [模型ID]",
        summary: "显示当前模型，或换一个模型继续会话",
        scope: CommandScope::Host,
    },
    CommandSpec {
        name: "permission",
        usage: "/permission [auto|ask|never|reset]",
        summary: "显示或切换授权模式：auto 由 Jev 判定、ask 每次都问、never 不问；reset 清空白名单并回到 auto",
        scope: CommandScope::Host,
    },
    CommandSpec {
        name: "jev",
        usage: "/jev <判定什么> [--label A --label B] [--material 材料]",
        summary: "手工问一次判定器（缺省选项 allow/deny）；只出读数，不采纳、不产生行动",
        scope: CommandScope::Host,
    },
    CommandSpec {
        name: "continue",
        usage: "/continue",
        summary: "继续本轮未完成的工作（达到调用上限后用它续跑）",
        scope: CommandScope::Host,
    },
    CommandSpec {
        name: "sessions",
        usage: "/sessions",
        summary: "列出历史会话（存放在 ~/.jev-harness/sessions）",
        scope: CommandScope::Host,
    },
    CommandSpec {
        name: "resume",
        usage: "/resume <会话ID>",
        summary: "接回一次历史会话的对话上下文；不带参数等于 /sessions",
        scope: CommandScope::Host,
    },
    CommandSpec {
        name: "status",
        usage: "/status",
        summary: "本次会话的读数（轮数、模型调用、判定、工具）",
        scope: CommandScope::Host,
    },
];

/// 一条已解析的命令调用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub spec: &'static CommandSpec,
    /// 命令名之后的那一段原文（已去掉首尾空白；没有参数时是空串）。
    pub args: String,
}

/// 解析结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Parsed {
    /// 不是命令（普通消息，照常发给模型）。
    NotACommand,
    /// 以 `/` 开头但认不出：`/xxx` 里的 `xxx`。
    Unknown(String),
    /// 认出来了。
    Known(Invocation),
}

/// 按名字查命令（大小写不敏感）。
pub fn find(name: &str) -> Option<&'static CommandSpec> {
    COMMANDS
        .iter()
        .find(|spec| spec.name.eq_ignore_ascii_case(name))
}

/// 补全候选：按模糊匹配筛出命令并排好序。
///
/// 口径：
/// - **大小写不敏感**的子序列匹配（`mdl` 命中 `model`）；空查询返回注册表里的全部命令；
/// - 排序：**前缀命中优先** → 首个命中字符越靠前越优先 → 命中位置之和越小越优先 →
///   注册表顺序兜底（[`slice::sort_by_key`] 是稳定排序，前三个键相同就保持注册表顺序）。
///
/// 查询串是 `/` 之后、第一个空白之前的那一段（不含 `/`）；参数区不该传进来
/// （`/model ` 带空白时调用方直接收起列表，见 `App::refresh_completion`）。
pub fn candidates(query: &str) -> Vec<&'static CommandSpec> {
    let needle = query.to_ascii_lowercase();
    if needle.is_empty() {
        return COMMANDS.iter().collect();
    }
    let mut scored: Vec<(u8, usize, usize, &'static CommandSpec)> = COMMANDS
        .iter()
        .filter_map(|spec| {
            let name = spec.name.to_ascii_lowercase();
            let (first, span) = subsequence_positions(&name, &needle)?;
            // 前缀命中排最前（`m` 先给 `model` 再给 `permission`）。
            let tier = u8::from(!name.starts_with(&needle));
            Some((tier, first, span, spec))
        })
        .collect();
    scored.sort_by_key(|(tier, first, span, _)| (*tier, *first, *span));
    scored.into_iter().map(|(_, _, _, spec)| spec).collect()
}

/// 子序列匹配（两边都已小写）：返回（首个命中字符的下标, 全部命中下标之和）；
/// 不匹配返回 `None`。
fn subsequence_positions(name: &str, needle: &str) -> Option<(usize, usize)> {
    let mut rest = needle.chars();
    let mut wanted = rest.next()?;
    let mut first = None;
    let mut span = 0;
    for (index, ch) in name.chars().enumerate() {
        if ch != wanted {
            continue;
        }
        first.get_or_insert(index);
        span += index;
        match rest.next() {
            Some(next) => wanted = next,
            None => return Some((first.unwrap_or_default(), span)),
        }
    }
    None
}

/// 解析一行输入。
pub fn parse(input: &str) -> Parsed {
    let trimmed = input.trim();
    let Some(rest) = trimmed.strip_prefix('/') else {
        return Parsed::NotACommand;
    };
    // `/` 单独一行、或 `/ 模型` 这类：名字是空串 → 认不出。
    let mut parts = rest.splitn(2, char::is_whitespace);
    let name = parts.next().unwrap_or_default();
    let args = parts.next().unwrap_or_default().trim().to_owned();
    match find(name) {
        Some(spec) if !name.is_empty() => Parsed::Known(Invocation { spec, args }),
        _ => Parsed::Unknown(name.to_owned()),
    }
}

/// `/help` 的正文：一行一条，用法与说明对齐。
pub fn help_text() -> String {
    let width = COMMANDS
        .iter()
        .map(|spec| spec.usage.chars().count())
        .max()
        .unwrap_or(0);
    let mut text = String::from("可用命令：");
    for spec in COMMANDS {
        text.push('\n');
        text.push_str(&format!(
            "{:width$}  {}",
            spec.usage,
            spec.summary,
            width = width
        ));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_not_a_command() {
        assert_eq!(parse("你好"), Parsed::NotACommand);
        assert_eq!(parse(" 解释一下 app.log "), Parsed::NotACommand);
        assert_eq!(parse("/"), Parsed::Unknown(String::new()));
    }

    #[test]
    fn a_command_with_arguments_keeps_them_verbatim() {
        let Parsed::Known(invocation) = parse("/model deepseek-reasoner") else {
            panic!("应当认出 /model");
        };
        assert_eq!(invocation.spec.name, "model");
        assert_eq!(invocation.args, "deepseek-reasoner");

        let Parsed::Known(invocation) = parse("  /MODEL   deepseek-chat  ") else {
            panic!("大小写与空白都该容忍");
        };
        assert_eq!(invocation.spec.name, "model");
        assert_eq!(invocation.args, "deepseek-chat");
    }

    #[test]
    fn a_command_without_arguments_has_empty_args() {
        let Parsed::Known(invocation) = parse("/quit") else {
            panic!("应当认出 /quit");
        };
        assert!(invocation.args.is_empty());
        assert_eq!(invocation.spec.scope, CommandScope::Ui);
    }

    #[test]
    fn unknown_commands_report_their_name() {
        assert_eq!(parse("/nope"), Parsed::Unknown("nope".to_owned()));
        assert_eq!(parse("/nope a b"), Parsed::Unknown("nope".to_owned()));
    }

    #[test]
    fn help_lists_every_command_with_its_usage() {
        let text = help_text();
        for spec in COMMANDS {
            assert!(
                text.contains(spec.usage),
                "帮助里少了 {}：{text}",
                spec.usage
            );
            assert!(text.contains(spec.summary), "帮助里少了说明：{text}");
        }
    }

    #[test]
    fn the_scope_split_matches_where_the_command_is_handled() {
        // 界面自处理的四个：不需要会话状态。
        for name in ["help", "quit", "clear", "research"] {
            assert_eq!(find(name).expect("命令存在").scope, CommandScope::Ui);
        }
        // 需要会话状态的六个（要模型通道、授权策略、会话历史或轮内计数）。
        for name in [
            "model",
            "permission",
            "status",
            "continue",
            "sessions",
            "resume",
        ] {
            assert_eq!(find(name).expect("命令存在").scope, CommandScope::Host);
        }
    }

    /// 只取候选的名字，断言排序口径时读起来清楚些。
    fn names(query: &str) -> Vec<&'static str> {
        candidates(query).iter().map(|spec| spec.name).collect()
    }

    #[test]
    fn an_empty_query_lists_every_command_in_registry_order() {
        assert_eq!(
            names(""),
            COMMANDS.iter().map(|spec| spec.name).collect::<Vec<_>>()
        );
        assert_eq!(candidates("").len(), COMMANDS.len());
    }

    #[test]
    fn candidates_match_subsequences_case_insensitively() {
        // `mdl` 命中 `model`（子序列），其余命令都接不上。
        assert_eq!(names("mdl"), vec!["model"]);
        // `pm` 命中 `permission`（p → … → m）。
        assert_eq!(names("pm"), vec!["permission"]);
        // 大小写不敏感：查询串大小写不该改变结果。
        assert_eq!(names("MDL"), names("mdl"));
        assert_eq!(names("Continue"), names("continue"));
    }

    #[test]
    fn a_prefix_hit_comes_before_a_scattered_one() {
        // `m` 既是 `model` 的前缀、又是 `permission` / `resume` 里的一个字符：前缀优先，
        // 其余按首个命中位置排（子序列匹配本来就宽松，`resume` 的 `m` 在第 4 位）。
        assert_eq!(names("m"), vec!["model", "permission", "resume"]);
        assert_eq!(names("h"), vec!["help", "research"]);
    }

    #[test]
    fn earlier_subsequence_positions_win_when_no_prefix_hits() {
        // `er` 在三条命令里都是子序列，按「首个命中字符越靠前越优先」排：
        // permission(e@1,r@2) → research(e@1,r@5) → clear(e@2,r@4)。
        assert_eq!(names("er"), vec!["permission", "research", "clear"]);
    }

    #[test]
    fn a_query_that_matches_nothing_has_no_candidates() {
        assert!(candidates("zzz").is_empty());
        assert!(candidates("help me").is_empty(), "空白不该出现在命令名里");
        // 顺序反了不算命中：`ledom` 不是 `model` 的子序列。
        assert!(candidates("ledom").is_empty());
    }

    #[test]
    fn takes_args_follows_the_usage_string() {
        for name in ["model", "permission"] {
            assert!(find(name).expect("命令存在").takes_args(), "{name} 带参数");
        }
        for name in ["help", "quit", "clear", "research", "status", "continue"] {
            assert!(
                !find(name).expect("命令存在").takes_args(),
                "{name} 不带参数"
            );
        }
    }
}
