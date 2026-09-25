//! 终端护栏：能力探测成枚举、进入/退出、可被信号调用的幂等恢复。
//!
//! 设计来源：`docs/grok-tui-design.md` §7.1 第 7 条（能力探测成枚举、业务代码只读能力位）
//! 与第 10 条（信号触发的恢复是少数值得保留的防御）、§5.6；专题
//! `docs/research/grok-tui/21-session-lifecycle.md`、`23-accessibility-adaptivity.md`。
//!
//! 与上游的差异：
//! - 上游有 `pop_fence`（DA1 清退栅栏）与 Kitty 协议标志复位等一整套退出工序；本项目
//!   只做最小集合：离备用屏、显示光标、关 bracketed paste、关鼠标捕获、弹掉键盘增强标志、关
//!   raw mode。**恢复是幂等的**，因为信号处理器、panic hook 与正常退出路径都会调它。
//! - 上游状态泄漏防线覆盖子进程继承等；本项目不自我重启。
//! - 本 crate 不引入信号库：`restore()` 是 `&self` 的幂等函数，宿主（或示例里的
//!   tokio 信号任务）把信号接到它上面即可。
//!
//! 模式的开与关**都由本护栏负责**，宿主不需要自己发转义序列：
//! `enter()` 开 raw mode / 备用屏 / 隐藏光标 / bracketed paste / 鼠标捕获 /
//! 键盘增强标志，`restore()` 逐项复位。
//!
//! - **bracketed paste** 是 `Event::Paste`（[`crate::app::App::handle_event`] 的粘贴路径）
//!   能收到事件的前提——crossterm 只在应用显式开启该模式后才投递它。
//! - **鼠标捕获**是 `Event::Mouse`（点击与滚轮）的前提；代价是终端自己的文本选择要让位给
//!   应用，多数终端仍可用 Shift+拖选绕开。
//! - **键盘增强标志**（Kitty 协议的 `DISAMBIGUATE_ESCAPE_CODES`）让 `Shift+Enter` 与
//!   `Enter` 在字节层可分——不开启时两者同码，换行键就失效。这里**不先查询**再推：查询在
//!   不支持该协议的终端上要等满 2 s 超时才返回（crossterm 的实现在此处 poll 2000 ms），
//!   而一条不被理解的私有序列按终端惯例会被忽略。因此：支持的终端按 Shift+Enter 换行，
//!   不支持的终端仍有 `Ctrl+J` 这条备用键（注册表里的 alt key）。

use std::io::{self, IsTerminal};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crossterm::cursor;
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
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
    /// 进备用屏 + raw mode + 隐藏光标 + bracketed paste + 鼠标捕获 + 键盘增强标志。
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
        execute!(
            stdout,
            cursor::Hide,
            EnableBracketedPaste,
            EnableMouseCapture,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )?;
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
            DisableBracketedPaste,
            DisableMouseCapture,
            PopKeyboardEnhancementFlags
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

/// 把一段文本送进系统剪贴板：**OSC 52**（`ESC ] 52 ; c ; <base64> BEL`）。
///
/// 为什么用它而不引依赖：`arboard` 之类要把 X11/Wayland 的整套依赖拖进来，而且在本机
/// 是 Windows Terminal 的环境里未必更可靠；OSC 52 由终端自己写剪贴板，**不需要新依赖**，
/// 隔着 SSH 也能用。代价是**终端得支持**（Windows Terminal、iTerm2、kitty、WezTerm、
/// alacritty 等支持；tmux 需要 `set-clipboard on`）——不支持时终端会静默忽略，
/// 所以调用方要把「送出去了」和「终端真的写进剪贴板了」分开说（本 crate 无法探测后者）。
pub fn copy_to_clipboard(text: &str) -> io::Result<()> {
    use std::io::Write;

    let mut out = io::stdout();
    write_osc52(&mut out, text)?;
    out.flush()
}

/// 只负责写那一条序列，输出流由调用方给——于是「字节到底长什么样」可以被测试钉住
/// （`copy_to_clipboard` 直接写 stdout，自动化测试碰不到它）。
fn write_osc52(out: &mut impl io::Write, text: &str) -> io::Result<()> {
    out.write_all(format!("\x1b]52;c;{}\x07", base64(text.as_bytes())).as_bytes())
}

/// 标准 base64 编码（RFC 4648，带 `=` 补齐）。
///
/// 只为 OSC 52 服务：为这一步引一个依赖不划算，实现只有十几行。
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
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
    fn the_clipboard_sequence_is_the_documented_osc52_form() {
        // OSC 52：`ESC ] 52 ; c ; <base64> BEL`——`c` 是系统剪贴板选择区。
        // 这里断言的是**字节**：宿主把它写进终端，终端不支持时静默忽略。
        let mut out = Vec::new();
        super::write_osc52(&mut out, "你好").expect("写序列");
        assert_eq!(out, b"\x1b]52;c;5L2g5aW9\x07");
        // 空文本也要写出完整序列（不 panic、不越界）
        let mut empty = Vec::new();
        super::write_osc52(&mut empty, "").expect("写空序列");
        assert_eq!(empty, b"\x1b]52;c;\x07");
    }

    #[test]
    fn base64_matches_the_standard_alphabet_and_padding() {
        // RFC 4648 的官方测试向量（含三种补齐长度与 UTF-8 多字节）。
        assert_eq!(super::base64(b""), "");
        assert_eq!(super::base64(b"f"), "Zg==");
        assert_eq!(super::base64(b"fo"), "Zm8=");
        assert_eq!(super::base64(b"foo"), "Zm9v");
        assert_eq!(super::base64(b"foob"), "Zm9vYg==");
        assert_eq!(super::base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(super::base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(super::base64("中文".as_bytes()), "5Lit5paH");
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
