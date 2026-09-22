//! 审批卡：单槽位模态 + 显式的键盘所有权。
//!
//! 设计来源：`docs/grok-tui-design.md` §5.4、§4.3 与专题
//! `docs/research/grok-tui/19-approval-and-elicitation.md`（左侧 1 列强调轨、内容列
//! `area.x + 3`、底部动作行、失焦整体降饱和；`blocking_card()` 是唯一权威判定且
//! **权限请求抢断其它卡片**）。
//!
//! 与上游的差异：
//! - 上游把「哪张卡拿键盘」写成一个排位函数；本项目只有一个模态槽位（`Option<ActiveModal>`，
//!   合成稿 §4.3：没有浮层栈），排位即「有卡时键盘归 [`KeyOwner::Approval`]」。
//! - 选项只保留三个（允许一次 / 本次会话总是允许 / 拒绝），且标签与键位都从
//!   [`crate::actions::default_actions`] 取——单一来源，卡片不自己写键位。

use crate::actions::{ActionId, KeyChord, Keymap};
use crate::blocks::{BlockCtx, BlockLine, Selectable};
use crate::glyphs;
use crate::width;
use ratatui::text::{Line, Span};

/// 模态槽位：同时最多只有一个（上游 `Option<ActiveModal>`，12 个变体；这里 1 个）。
#[derive(Debug, Clone, PartialEq)]
pub enum ActiveModal {
    Approval(ApprovalRequest),
}

impl ActiveModal {
    pub fn request(&self) -> &ApprovalRequest {
        match self {
            ActiveModal::Approval(request) => request,
        }
    }
}

/// 审批请求：工具名 + 待执行的命令 + 说明。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalRequest {
    /// 工具名，例如 `bash`。
    pub tool: String,
    /// 待执行的命令 / 参数摘要。
    pub command: String,
    /// 面向用户的说明（为什么被拦下、风险在哪）。
    pub body: Option<String>,
}

impl ApprovalRequest {
    pub fn new(tool: impl Into<String>, command: impl Into<String>) -> Self {
        ApprovalRequest {
            tool: tool.into(),
            command: command.into(),
            body: None,
        }
    }

    pub fn with_body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }
}

/// 卡片上的一个动作：键位与标签都来自动作注册表。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApprovalOption {
    pub action: ActionId,
    pub key: KeyChord,
    pub label: &'static str,
}

impl ApprovalOption {
    /// 三个选项从注册表取（键位可被配置覆盖，卡片自动跟着变）。
    pub fn from_keymap(keymap: &Keymap) -> Vec<ApprovalOption> {
        [
            ActionId::ApproveOnce,
            ActionId::ApproveAlways,
            ActionId::Deny,
        ]
        .into_iter()
        .filter_map(|action| {
            keymap.action(action).map(|def| ApprovalOption {
                action,
                key: def.default_key,
                label: def.label,
            })
        })
        .collect()
    }
}

/// 审批卡：内容列从第 3 列开始（`┃` + 2 空格），底部固定动作行。
#[derive(Debug, Clone, Copy, Default)]
pub struct ApprovalCard;

/// 强调轨与内容列的固定偏移（上游 `content_x = area.x + 3`）。
const CONTENT_X: usize = 3;

impl ApprovalCard {
    /// 渲染卡片。`max_rows` 是可用高度，超出时折叠并在末尾标出被折掉的行数；
    /// `focused` 为 false 时整体降饱和（失焦）。
    pub fn render(
        request: &ApprovalRequest,
        keymap: &Keymap,
        ctx: &BlockCtx,
        focused: bool,
        max_rows: usize,
    ) -> Vec<BlockLine> {
        let rail = ctx.glyphs.resolve(glyphs::accent_bar());
        let ellipsis = ctx.glyphs.resolve(glyphs::ellipsis());
        let accent = if focused {
            ctx.theme.accent(ctx.theme.accent_warning)
        } else {
            ctx.theme.dim()
        };
        let body_style = if focused {
            ctx.theme.fg()
        } else {
            ctx.theme.muted()
        };
        let body_width = ctx.width.saturating_sub(CONTENT_X).max(1);

        let mut rows: Vec<(String, ratatui::style::Style)> = Vec::new();
        rows.push((format!("权限请求 · {}", request.tool), accent));
        rows.push((format!("命令  {}", request.command), body_style));
        if let Some(body) = &request.body {
            rows.push((body.clone(), ctx.theme.muted()));
        }
        let options = ApprovalOption::from_keymap(keymap)
            .into_iter()
            .map(|option| format!("{} {}", option.key.display(), option.label))
            .collect::<Vec<_>>()
            .join("   ");
        rows.push((options, accent));

        // 折行：正文按内容列宽换行，装饰轨只画在首行。
        let mut lines: Vec<(String, ratatui::style::Style)> = Vec::new();
        for (text, style) in rows {
            for wrapped in width::wrap_words(&text, body_width) {
                lines.push((wrapped, style));
            }
        }

        let rail_style = if focused {
            ctx.theme.accent(ctx.theme.accent_warning)
        } else {
            ctx.theme.dim()
        };
        let mut output = Vec::new();
        let mut collapsed = 0usize;
        for (index, (text, style)) in lines.iter().enumerate() {
            if max_rows > 0 && index + 1 >= max_rows && lines.len() > max_rows {
                collapsed = lines.len() - index;
                break;
            }
            output.push(card_line(rail, rail_style, ctx, text, *style));
        }
        if collapsed > 0 {
            let text = format!("{ellipsis} 已折叠 {collapsed} 行");
            output.push(card_line(rail, rail_style, ctx, &text, ctx.theme.dim()));
        }
        output
    }
}

fn card_line(
    rail: &str,
    rail_style: ratatui::style::Style,
    ctx: &BlockCtx,
    text: &str,
    style: ratatui::style::Style,
) -> BlockLine {
    let clipped = width::clip_with_ellipsis(
        text,
        ctx.width.saturating_sub(CONTENT_X),
        ctx.glyphs.resolve(glyphs::ellipsis()),
    );
    // 卡片整行铺满：既铺底色（上游把权限卡区域填成 `theme.bg_light`），也保证不透明——
    // 行尾用空格盖住，否则下面的回滚区会从卡片右边透上来（无色档下更要紧）。
    let background = ctx.theme.bg_light;
    let filled = CONTENT_X + width::display_width(&clipped);
    let padding = ctx.width.saturating_sub(filled);
    BlockLine::new(
        Line::from(vec![
            Span::styled(rail.to_owned(), rail_style.bg(background)),
            Span::styled("  ".to_owned(), ctx.theme.dim().bg(background)),
            Span::styled(clipped, style.bg(background)),
            Span::styled(
                " ".repeat(padding),
                ratatui::style::Style::new().bg(background),
            ),
        ]),
        Selectable::Spans(CONTENT_X as u16..ctx.width as u16),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::glyphs::Glyphs;
    use crate::theme::Theme;

    fn ctx<'a>(theme: &'a Theme, width: usize) -> BlockCtx<'a> {
        BlockCtx {
            width,
            theme,
            glyphs: Glyphs::FANCY,
            frame: 0,
            decision: None,
        }
    }

    #[test]
    fn card_starts_with_an_accent_rail_and_offsets_content() {
        let theme = Theme::DARK;
        let request = ApprovalRequest::new("bash", "git push --force origin main")
            .with_body("该命令会改写远端历史");
        let keymap = Keymap::from_defaults();
        let lines = ApprovalCard::render(&request, &keymap, &ctx(&theme, 60), true, 10);
        assert!(lines.len() >= 4);
        for line in &lines {
            assert!(
                line.text().starts_with('┃'),
                "每行都有强调轨：{:?}",
                line.text()
            );
            assert_eq!(&line.text()[3..4], " ", "内容列从第 3 列开始");
        }
        let text: String = lines
            .iter()
            .map(BlockLine::text)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("权限请求 · bash"), "{text}");
        assert!(text.contains("git push --force origin main"), "{text}");
        assert!(text.contains("该命令会改写远端历史"), "{text}");
    }

    #[test]
    fn options_come_from_the_action_registry() {
        let theme = Theme::DARK;
        let keymap = Keymap::from_defaults();
        let request = ApprovalRequest::new("bash", "ls");
        let text: String = ApprovalCard::render(&request, &keymap, &ctx(&theme, 80), true, 10)
            .iter()
            .map(BlockLine::text)
            .collect::<Vec<_>>()
            .join("\n");
        for def in keymap.defs() {
            if matches!(
                def.id,
                ActionId::ApproveOnce | ActionId::ApproveAlways | ActionId::Deny
            ) {
                assert!(text.contains(def.label), "缺少选项 {}：{text}", def.label);
                assert!(
                    text.contains(&def.default_key.display()),
                    "缺少键位：{text}"
                );
            }
        }
        // 覆盖键位后卡片跟着变（单一来源的证据）。
        let remapped = Keymap::with_overrides(&[(
            ActionId::ApproveOnce,
            KeyChord::plain(crossterm::event::KeyCode::Char('o')),
        )]);
        let text: String = ApprovalCard::render(&request, &remapped, &ctx(&theme, 80), true, 10)
            .iter()
            .map(BlockLine::text)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("o 允许一次"), "{text}");
    }

    #[test]
    fn unfocused_card_recedes() {
        let theme = Theme::DARK;
        let request = ApprovalRequest::new("bash", "ls");
        let keymap = Keymap::from_defaults();
        let focused = ApprovalCard::render(&request, &keymap, &ctx(&theme, 60), true, 10);
        let unfocused = ApprovalCard::render(&request, &keymap, &ctx(&theme, 60), false, 10);
        assert_eq!(focused.len(), unfocused.len());
        // 降饱和：强调轨不再是警示色，而是弱化样式。
        assert_eq!(
            focused[0].content.spans[0].style.fg,
            Some(theme.accent_warning)
        );
        assert_eq!(unfocused[0].content.spans[0].style.fg, Some(theme.gray_dim));
    }

    #[test]
    fn overlong_card_collapses_with_a_marker() {
        let theme = Theme::DARK;
        let keymap = Keymap::from_defaults();
        let request =
            ApprovalRequest::new("bash", "a".repeat(400)).with_body("很长的说明 ".repeat(20));
        let lines = ApprovalCard::render(&request, &keymap, &ctx(&theme, 40), true, 5);
        assert!(lines.len() <= 5, "折叠后不得超过可用高度：{}", lines.len());
        assert!(
            lines.last().unwrap().text().contains("已折叠"),
            "{:?}",
            lines.last().unwrap().text()
        );
    }

    #[test]
    fn modal_slot_holds_at_most_one_request() {
        let modal = ActiveModal::Approval(ApprovalRequest::new("bash", "ls"));
        assert_eq!(modal.request().tool, "bash");
        // 槽位是 `Option<ActiveModal>`：换卡就是替换，不存在浮层栈。
        let slot: Option<ActiveModal> = Some(modal);
        let replaced = slot.map(|_| ActiveModal::Approval(ApprovalRequest::new("read", "x")));
        assert_eq!(replaced.unwrap().request().tool, "read");
    }
}
