//! 终端护栏：能力探测成枚举、进入/退出、可被信号调用的幂等恢复。
//!
//! 设计来源：`docs/grok-tui-design.md` §7.1 第 7 条（能力探测成枚举、业务代码只读能力位）
//! 与第 10 条（信号触发的恢复是少数值得保留的防御）、§5.6；专题
//! `docs/research/grok-tui/21-session-lifecycle.md`、`23-accessibility-adaptivity.md`。
//!
//! 与上游的差异：
//! - 上游有 `pop_fence`（DA1 清退栅栏）与 Kitty 协议标志复位等一整套退出工序；本项目
//!   只做最小集合：离备用屏、显示光标、关 bracketed paste、关 raw mode。**恢复是幂等的**，
//!   因为信号处理器、panic hook 与正常退出路径都会调它。
//! - 上游状态泄漏防线覆盖鼠标捕获、子进程继承等；本项目不启用鼠标捕获，也不自我重启。
//! - 本 crate 不引入信号库：`restore()` 是 `&self` 的幂等函数，宿主（或示例里的
//!   tokio 信号任务）把信号接到它上面即可。
//!
//! 模式的开与关**都由本护栏负责**，宿主不需要自己发转义序列：
//! `enter()` 开 raw mode / 备用屏 / 隐藏光标 / bracketed paste，
//! `restore()` 逐项复位。bracketed paste 是 `Event::Paste`（[`crate::app::App::handle_event`]
//! 的粘贴路径）能收到事件的前提——crossterm 只在应用显式开启该模式后才投递它。

use std::io::{self, IsTerminal};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crossterm::cursor;
use crossterm::event::{DisableBracketedPaste, EnableBracketedPaste};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};

use crate::glyphs::Glyphs;
use crate::theme::{ColorLevel, Theme};

/// 能力位：终端能力探测的唯一出口，业务代码只读这里的位，不读环境变量。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    pub color: ColorLevel,
    /// 输出是不是终端（管道/重定向时宿主可以据此切到纯文本输出）。
    pub is_tty: bool,
    pub glyphs: Glyphs,
}

impl Capabilities {
    /// 进程级探测：环境变量 + `stdout` 是否 TTY + 平台。
    pub fn detect() -> Self {
        Capabilities::from_env_lookup(
            |key| std::env::var(key).ok(),
            io::stdout().is_terminal(),
            cfg!(windows),
        )
    }

    /// 同上，但输入由调用方给定（单测不必改进程环境）。
    ///
    /// `legacy_console` 对应上游的 legacy Windows ConHost 判定（`glyphs.rs:377-424`，
    /// 非 Windows 直接短路为 false）：命中时整库切到回退字形集。
    pub fn from_env_lookup(
        lookup: impl Fn(&str) -> Option<String>,
        is_tty: bool,
        legacy_console: bool,
    ) -> Self {
        Capabilities {
            color: ColorLevel::from_env_lookup(&lookup),
            is_tty,
            glyphs: if legacy_console {
                Glyphs::FALLBACK
            } else {
                Glyphs::FANCY
            },
        }
    }

    /// 按色彩能力量化过的主题。
    pub fn theme(&self) -> Theme {
        Theme::DARK.quantized(self.color)
    }
}

/// 进入终端的护栏：`Drop` 会恢复，进程被信号打断时由宿主调用 [`TerminalGuard::restore`]。
#[derive(Debug)]
pub struct TerminalGuard {
    restored: AtomicBool,
}

impl TerminalGuard {
    /// 进备用屏 + raw mode + 隐藏光标 + bracketed paste。
    pub fn enter() -> io::Result<Self> {
        TerminalGuard::enter_with_screen(true)
    }

    /// 同上，可选是否进备用屏（inline 形态的逃生口）。
    pub fn enter_with_screen(alternate_screen: bool) -> io::Result<Self> {
        let guard = TerminalGuard {
            restored: AtomicBool::new(false),
        };
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        if alternate_screen {
            execute!(stdout, EnterAlternateScreen)?;
        }
        execute!(stdout, cursor::Hide, EnableBracketedPaste)?;
        Ok(guard)
    }

    /// 终端是否还处在「被本护栏接管」的状态。
    pub fn is_active(&self) -> bool {
        !self.restored.load(Ordering::SeqCst)
    }

    /// 恢复终端状态。**幂等**：重复调用只有第一次真正动手，
    /// 因此信号处理器、panic hook 与正常退出可以都调一次而不互相踩。
    pub fn restore(&self) -> io::Result<()> {
        if self.restored.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        let mut stdout = io::stdout();
        let modes = execute!(
            stdout,
            LeaveAlternateScreen,
            cursor::Show,
            DisableBracketedPaste
        );
        let raw = disable_raw_mode();
        modes.and(raw)
    }

    /// 装上 panic hook：panic 时先恢复终端再交给原 hook（否则 panic 信息会盖在备用屏里）。
    pub fn install_panic_hook(self: &Arc<Self>) {
        let previous = std::panic::take_hook();
        let guard = Arc::clone(self);
        std::panic::set_hook(Box::new(move |info| {
            let _ = guard.restore();
            previous(info);
        }));
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guard(restored: bool) -> TerminalGuard {
        TerminalGuard {
            restored: AtomicBool::new(restored),
        }
    }

    #[test]
    fn capabilities_pick_glyphs_and_theme() {
        let fancy = Capabilities::from_env_lookup(
            |key| match key {
                "COLORTERM" => Some("truecolor".to_owned()),
                _ => None,
            },
            true,
            false,
        );
        assert_eq!(fancy.color, ColorLevel::TrueColor);
        assert_eq!(fancy.glyphs, Glyphs::FANCY);
        assert_eq!(fancy.theme().accent_assistant, Theme::DARK.accent_assistant);

        let legacy = Capabilities::from_env_lookup(|_| None, true, true);
        assert_eq!(legacy.glyphs, Glyphs::FALLBACK);
        assert_eq!(legacy.color, ColorLevel::None);
        assert_eq!(legacy.theme().fg(), ratatui::style::Style::new());

        let piped = Capabilities::from_env_lookup(|_| None, false, false);
        assert!(!piped.is_tty);
    }

    #[test]
    fn guard_reports_active_before_restore() {
        // 不真进终端（离线单测不碰 raw mode），只验证状态位。
        assert!(guard(false).is_active());
    }

    #[test]
    fn restore_short_circuits_when_already_restored() {
        // 只覆盖「已恢复」分支：`restored == false` 的分支会真的写转义序列并关 raw mode，
        // 那条路径只能由宿主在真实 TTY 上验证（见 review 的残留风险记录）。
        let guard = guard(true);
        assert!(!guard.is_active());
        assert!(guard.restore().is_ok());
        assert!(guard.restore().is_ok());
        assert!(!guard.is_active());
    }
}
