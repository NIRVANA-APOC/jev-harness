//! 动作注册表：`ActionId` 是编译期枚举，全部默认键位在 [`default_actions()`] 一处注册。
//!
//! 设计来源：`docs/grok-tui-design.md` §5.1、§7.1 第 1 条（单一动作注册表 + 编译期动作枚举）；
//! 专题 `docs/research/grok-tui/16-keyboard-and-actions.md`（§1 动作枚举形状、§2.2 `ActionDef`
//! 字段与 `hint_priority` 语义、`When` 上下文精确匹配、`input/key.rs:46-66` 的修饰键归一化、
//! `input/key.rs:60-62` 的 Release 事件不匹配）。信号源与终端恢复护栏见 [`crate::terminal`]。
//!
//! 与上游的差异：
//! - 上游 `ActionId` 76 个变体、`When` 7 个值、70 条注册；这里分别是 20 / 4 / 20 条。
//! - **键位可覆盖**：上游明确「cannot currently be remapped」
//!   （`docs/user-guide/03-keyboard-shortcuts.md:3`），本项目反其道而行，理由是规模——
//!   可以 [`Keymap::with_overrides`] 传入配置里的键位（设计 §7.2 第 6 条）。
//! - 上游 `Esc` 是「把焦点停到 scrollback」而 `Ctrl+C` 才取消；本项目只有单一焦点模型，
//!   把两者合并成 `Interrupt`（`Esc`）与 `Quit`（`Ctrl+C`）。

use std::fmt;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

/// 编译期动作身份。没有字符串、没有 `FromStr`（上游 `actions/mod.rs:34-36`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionId {
    /// 退出应用。
    Quit,
    /// 取消当前轮（无在途轮次时等价于退出）。
    Interrupt,
    /// 提交提示框内容。
    Submit,
    /// 在提示框里插入硬换行。
    InsertNewline,
    CursorLeft,
    CursorRight,
    CursorUp,
    CursorDown,
    CursorHome,
    CursorEnd,
    DeleteBackward,
    DeleteForward,
    ScrollLineUp,
    ScrollLineDown,
    ScrollPageUp,
    ScrollPageDown,
    ScrollToBottom,
    /// 批准一次（审批卡）。
    ApproveOnce,
    /// 本次会话总是批准（审批卡）。
    ApproveAlways,
    /// 拒绝（审批卡）。
    Deny,
}

impl ActionId {
    /// 全部动作的枚举顺序（供测试与注册表完备性断言）。
    pub const ALL: &'static [ActionId] = &[
        ActionId::Quit,
        ActionId::Interrupt,
        ActionId::Submit,
        ActionId::InsertNewline,
        ActionId::CursorLeft,
        ActionId::CursorRight,
        ActionId::CursorUp,
        ActionId::CursorDown,
        ActionId::CursorHome,
        ActionId::CursorEnd,
        ActionId::DeleteBackward,
        ActionId::DeleteForward,
        ActionId::ScrollLineUp,
        ActionId::ScrollLineDown,
        ActionId::ScrollPageUp,
        ActionId::ScrollPageDown,
        ActionId::ScrollToBottom,
        ActionId::ApproveOnce,
        ActionId::ApproveAlways,
        ActionId::Deny,
    ];
}

/// 动作的上下文。上游 `When` 也是纯枚举 + 精确匹配（`actions/mod.rs:149-167, 277-290`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum When {
    /// 任何焦点下都可用（退出、取消）。
    Always,
    /// 提示框持有键盘时。
    PromptFocused,
    /// 回滚区持有键盘时。
    ScrollbackFocused,
    /// 审批卡持有键盘时。
    ApprovalFocused,
}

/// 这一拍键盘归谁：显式枚举，不靠隐式焦点推断。
///
/// 对应上游 `key_owner.rs` 的排位思想（权限卡抢断其他卡，`key_owner.rs:96-105`），
/// 这里收敛成三档。审批卡在槽位上时，**权限键优先**：只查 `ApprovalFocused` 与 `Always`，
/// 其他上下文里绑到同一键的动作不会遮蔽它。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyOwner {
    Approval,
    Prompt,
    Scrollback,
}

impl KeyOwner {
    /// 自身上下文。
    pub fn context(self) -> When {
        match self {
            KeyOwner::Approval => When::ApprovalFocused,
            KeyOwner::Prompt => When::PromptFocused,
            KeyOwner::Scrollback => When::ScrollbackFocused,
        }
    }
}

/// 一次按键：键码 + 修饰键（不含 `KeyEventKind` / `KeyEventState`，那两样不参与匹配）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyChord {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
}

impl KeyChord {
    /// 归一化后构造：`Char('G')` 自带 SHIFT，`Char('g')+SHIFT` 折成 `Char('G')+SHIFT`
    /// （上游 `input/key.rs:46-66`）。两种写法归一化后必须相等，否则同一物理按键会
    /// 因为「事件带不带 SHIFT」而落到不同动作上。
    pub fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        let plain = modifiers - KeyModifiers::SHIFT;
        match code {
            KeyCode::Char(c) if c.is_ascii_uppercase() => KeyChord {
                code,
                modifiers: plain | KeyModifiers::SHIFT,
            },
            KeyCode::Char(c)
                if c.is_ascii_lowercase() && modifiers.contains(KeyModifiers::SHIFT) =>
            {
                KeyChord {
                    code: KeyCode::Char(c.to_ascii_uppercase()),
                    modifiers: plain | KeyModifiers::SHIFT,
                }
            }
            _ => KeyChord { code, modifiers },
        }
    }

    pub const fn plain(code: KeyCode) -> Self {
        KeyChord {
            code,
            modifiers: KeyModifiers::NONE,
        }
    }

    pub const fn ctrl(code: KeyCode) -> Self {
        KeyChord {
            code,
            modifiers: KeyModifiers::CONTROL,
        }
    }

    pub const fn alt(code: KeyCode) -> Self {
        KeyChord {
            code,
            modifiers: KeyModifiers::ALT,
        }
    }

    /// 从 crossterm 事件取和弦；Release 事件返回 `None`（上游 `input/key.rs:60-62`）。
    pub fn from_event(event: KeyEvent) -> Option<Self> {
        if event.kind == KeyEventKind::Release {
            return None;
        }
        Some(KeyChord::new(event.code, event.modifiers))
    }

    /// 解析配置里的键位写法：`ctrl+shift+p`、`alt+enter`、`pageup`、`f5`、`?`。
    ///
    /// 这是「键位可在配置里覆盖」的入口；配置文件的读写留给宿主，本 crate 只认这一种写法。
    pub fn parse(spec: &str) -> Option<Self> {
        let mut modifiers = KeyModifiers::NONE;
        let mut code: Option<KeyCode> = None;
        for token in spec.split('+').map(str::trim).filter(|t| !t.is_empty()) {
            match token.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => modifiers |= KeyModifiers::CONTROL,
                "alt" | "opt" | "option" => modifiers |= KeyModifiers::ALT,
                "shift" => modifiers |= KeyModifiers::SHIFT,
                "super" | "cmd" | "meta" => modifiers |= KeyModifiers::SUPER,
                rest => {
                    if code.is_some() {
                        return None;
                    }
                    code = Some(parse_code(rest)?);
                }
            }
        }
        let code = code?;
        // 单个 `+` 这类写法不会被上面的 token 逻辑接受：`ctrl++` → token 空，直接失败。
        if spec.trim() != spec || spec.is_empty() {
            return None;
        }
        Some(KeyChord::new(code, modifiers))
    }

    /// 快捷键栏用的短写法（上游 `display()`：`Ctrl+c`）。
    pub fn display(&self) -> String {
        let mut out = String::new();
        for (flag, name) in [
            (KeyModifiers::CONTROL, "Ctrl"),
            (KeyModifiers::ALT, "Alt"),
            (KeyModifiers::SHIFT, "Shift"),
            (KeyModifiers::SUPER, "Super"),
        ] {
            if self.modifiers.contains(flag) {
                out.push_str(name);
                out.push('+');
            }
        }
        out.push_str(&display_code(&self.code));
        out
    }
}

impl fmt::Display for KeyChord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.display())
    }
}

fn parse_code(token: &str) -> Option<KeyCode> {
    Some(match token {
        "enter" | "return" => KeyCode::Enter,
        "esc" | "escape" => KeyCode::Esc,
        "tab" => KeyCode::Tab,
        "backtab" => KeyCode::BackTab,
        "backspace" => KeyCode::Backspace,
        "delete" | "del" => KeyCode::Delete,
        "insert" => KeyCode::Insert,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" | "pgup" => KeyCode::PageUp,
        "pagedown" | "pgdn" => KeyCode::PageDown,
        "space" => KeyCode::Char(' '),
        other => {
            if let Some(number) = other.strip_prefix('f')
                && let Ok(number) = number.parse::<u8>()
                && (1..=12).contains(&number)
            {
                return Some(KeyCode::F(number));
            }
            let mut chars = other.chars();
            let first = chars.next()?;
            if chars.next().is_some() {
                return None;
            }
            KeyCode::Char(first)
        }
    })
}

fn display_code(code: &KeyCode) -> String {
    match code {
        KeyCode::Char(' ') => "Space".to_owned(),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Enter => "Enter".to_owned(),
        KeyCode::Esc => "Esc".to_owned(),
        KeyCode::Tab => "Tab".to_owned(),
        KeyCode::BackTab => "BackTab".to_owned(),
        KeyCode::Backspace => "Backspace".to_owned(),
        KeyCode::Delete => "Delete".to_owned(),
        KeyCode::Insert => "Insert".to_owned(),
        KeyCode::Up => "Up".to_owned(),
        KeyCode::Down => "Down".to_owned(),
        KeyCode::Left => "Left".to_owned(),
        KeyCode::Right => "Right".to_owned(),
        KeyCode::Home => "Home".to_owned(),
        KeyCode::End => "End".to_owned(),
        KeyCode::PageUp => "PgUp".to_owned(),
        KeyCode::PageDown => "PgDn".to_owned(),
        KeyCode::F(number) => format!("F{number}"),
        other => format!("{other:?}"),
    }
}

/// 一条动作注册：身份 + 文案 + 上下文 + 键位 + 快捷键栏优先级。
///
/// 字段取自上游 `ActionDef`（`actions/mod.rs:197-221`），已按本项目砍到最小。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionDef {
    pub id: ActionId,
    /// 快捷键栏上的短标签。
    pub label: &'static str,
    /// 完整说明（帮助面板将来用）。
    pub description: &'static str,
    pub context: When,
    pub default_key: KeyChord,
    pub alt_keys: &'static [KeyChord],
    /// `None`＝不上快捷键栏；`Some(0)` 优先级最高（上游 `actions/mod.rs:213`）。
    pub hint_priority: Option<u8>,
}

/// 备用键位：写成 `const` 而不是内联数组字面量——内联数组是临时值，借不出去 `'static`。
/// 放在一处也让「同一键位的第二种写法」看得见。
const ALT_NEWLINE: &[KeyChord] = &[
    KeyChord {
        code: KeyCode::Enter,
        modifiers: KeyModifiers::SHIFT,
    },
    KeyChord {
        code: KeyCode::Char('j'),
        modifiers: KeyModifiers::CONTROL,
    },
];
const ALT_CURSOR_UP: &[KeyChord] = &[KeyChord {
    code: KeyCode::Char('p'),
    modifiers: KeyModifiers::CONTROL,
}];
const ALT_CURSOR_DOWN: &[KeyChord] = &[KeyChord {
    code: KeyCode::Char('n'),
    modifiers: KeyModifiers::CONTROL,
}];
const ALT_CURSOR_HOME: &[KeyChord] = &[KeyChord {
    code: KeyCode::Char('a'),
    modifiers: KeyModifiers::CONTROL,
}];
const ALT_CURSOR_END: &[KeyChord] = &[KeyChord {
    code: KeyCode::Char('e'),
    modifiers: KeyModifiers::CONTROL,
}];
const ALT_DELETE_BACKWARD: &[KeyChord] = &[KeyChord {
    code: KeyCode::Char('h'),
    modifiers: KeyModifiers::CONTROL,
}];
const ALT_DELETE_FORWARD: &[KeyChord] = &[KeyChord {
    code: KeyCode::Char('d'),
    modifiers: KeyModifiers::CONTROL,
}];
const ALT_SCROLL_TO_BOTTOM: &[KeyChord] = &[KeyChord {
    code: KeyCode::Char('g'),
    modifiers: KeyModifiers::CONTROL,
}];
const ALT_APPROVE_ONCE: &[KeyChord] = &[KeyChord {
    code: KeyCode::Char('1'),
    modifiers: KeyModifiers::NONE,
}];
const ALT_APPROVE_ALWAYS: &[KeyChord] = &[
    KeyChord {
        code: KeyCode::Char('2'),
        modifiers: KeyModifiers::NONE,
    },
    KeyChord {
        code: KeyCode::Char('o'),
        modifiers: KeyModifiers::CONTROL,
    },
];
const ALT_DENY: &[KeyChord] = &[KeyChord {
    code: KeyCode::Char('3'),
    modifiers: KeyModifiers::NONE,
}];

/// 单一注册表：所有默认键位只在这里出现一次。
pub fn default_actions() -> Vec<ActionDef> {
    vec![
        ActionDef {
            id: ActionId::Quit,
            label: "退出",
            description: "退出 TUI 并恢复终端",
            context: When::Always,
            default_key: KeyChord::ctrl(KeyCode::Char('c')),
            alt_keys: &[],
            hint_priority: Some(0),
        },
        ActionDef {
            id: ActionId::Interrupt,
            label: "取消",
            description: "取消当前轮（无在途轮次时退出）",
            context: When::Always,
            default_key: KeyChord::plain(KeyCode::Esc),
            alt_keys: &[],
            hint_priority: Some(1),
        },
        ActionDef {
            id: ActionId::Submit,
            label: "发送",
            description: "提交提示框内容",
            context: When::PromptFocused,
            default_key: KeyChord::plain(KeyCode::Enter),
            alt_keys: &[],
            hint_priority: Some(2),
        },
        ActionDef {
            id: ActionId::InsertNewline,
            label: "换行",
            description: "在提示框里插入硬换行",
            context: When::PromptFocused,
            default_key: KeyChord::alt(KeyCode::Enter),
            alt_keys: ALT_NEWLINE,
            hint_priority: Some(3),
        },
        ActionDef {
            id: ActionId::CursorLeft,
            label: "左移",
            description: "光标左移一个 grapheme 簇",
            context: When::PromptFocused,
            default_key: KeyChord::plain(KeyCode::Left),
            alt_keys: &[],
            hint_priority: None,
        },
        ActionDef {
            id: ActionId::CursorRight,
            label: "右移",
            description: "光标右移一个 grapheme 簇",
            context: When::PromptFocused,
            default_key: KeyChord::plain(KeyCode::Right),
            alt_keys: &[],
            hint_priority: None,
        },
        ActionDef {
            id: ActionId::CursorUp,
            label: "上一可视行",
            description: "光标上移一个可视行",
            context: When::PromptFocused,
            default_key: KeyChord::plain(KeyCode::Up),
            alt_keys: ALT_CURSOR_UP,
            hint_priority: None,
        },
        ActionDef {
            id: ActionId::CursorDown,
            label: "下一可视行",
            description: "光标下移一个可视行",
            context: When::PromptFocused,
            default_key: KeyChord::plain(KeyCode::Down),
            alt_keys: ALT_CURSOR_DOWN,
            hint_priority: None,
        },
        ActionDef {
            id: ActionId::CursorHome,
            label: "行首",
            description: "光标移到逻辑行首",
            context: When::PromptFocused,
            default_key: KeyChord::plain(KeyCode::Home),
            alt_keys: ALT_CURSOR_HOME,
            hint_priority: None,
        },
        ActionDef {
            id: ActionId::CursorEnd,
            label: "行尾",
            description: "光标移到逻辑行尾",
            context: When::PromptFocused,
            default_key: KeyChord::plain(KeyCode::End),
            alt_keys: ALT_CURSOR_END,
            hint_priority: None,
        },
        ActionDef {
            id: ActionId::DeleteBackward,
            label: "退格",
            description: "删除光标前一个 grapheme 簇",
            context: When::PromptFocused,
            default_key: KeyChord::plain(KeyCode::Backspace),
            alt_keys: ALT_DELETE_BACKWARD,
            hint_priority: None,
        },
        ActionDef {
            id: ActionId::DeleteForward,
            label: "删除",
            description: "删除光标所在 grapheme 簇",
            context: When::PromptFocused,
            default_key: KeyChord::plain(KeyCode::Delete),
            alt_keys: ALT_DELETE_FORWARD,
            hint_priority: None,
        },
        ActionDef {
            id: ActionId::ScrollLineUp,
            label: "上滚一行",
            description: "回滚区上滚一行",
            context: When::ScrollbackFocused,
            default_key: KeyChord::plain(KeyCode::Up),
            alt_keys: &[],
            hint_priority: None,
        },
        ActionDef {
            id: ActionId::ScrollLineDown,
            label: "下滚一行",
            description: "回滚区下滚一行",
            context: When::ScrollbackFocused,
            default_key: KeyChord::plain(KeyCode::Down),
            alt_keys: &[],
            hint_priority: None,
        },
        ActionDef {
            id: ActionId::ScrollPageUp,
            label: "上翻",
            description: "回滚区上翻一屏",
            context: When::ScrollbackFocused,
            default_key: KeyChord::plain(KeyCode::PageUp),
            alt_keys: &[],
            hint_priority: Some(4),
        },
        ActionDef {
            id: ActionId::ScrollPageDown,
            label: "下翻",
            description: "回滚区下翻一屏",
            context: When::ScrollbackFocused,
            default_key: KeyChord::plain(KeyCode::PageDown),
            alt_keys: &[],
            hint_priority: Some(5),
        },
        ActionDef {
            id: ActionId::ScrollToBottom,
            label: "回到底部",
            description: "回滚区跳到底部并跟随输出",
            context: When::ScrollbackFocused,
            default_key: KeyChord::plain(KeyCode::End),
            alt_keys: ALT_SCROLL_TO_BOTTOM,
            hint_priority: None,
        },
        ActionDef {
            id: ActionId::ApproveOnce,
            label: "允许一次",
            description: "只批准这一次工具调用",
            context: When::ApprovalFocused,
            default_key: KeyChord::plain(KeyCode::Char('y')),
            alt_keys: ALT_APPROVE_ONCE,
            hint_priority: Some(0),
        },
        ActionDef {
            id: ActionId::ApproveAlways,
            label: "总是允许",
            description: "本次会话内总是允许该工具",
            context: When::ApprovalFocused,
            default_key: KeyChord::plain(KeyCode::Char('a')),
            alt_keys: ALT_APPROVE_ALWAYS,
            hint_priority: Some(1),
        },
        ActionDef {
            id: ActionId::Deny,
            label: "拒绝",
            description: "拒绝这次工具调用",
            context: When::ApprovalFocused,
            default_key: KeyChord::plain(KeyCode::Char('n')),
            alt_keys: ALT_DENY,
            hint_priority: Some(2),
        },
    ]
}

/// 键位查找失败的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeymapError {
    /// 未知动作（`ActionId` 是枚举，理论上不会发生；配置按字符串反查时才会）。
    UnknownAction,
    /// 同一上下文里同一键位绑了两个动作。
    DuplicateBinding(KeyChord),
}

impl fmt::Display for KeymapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeymapError::UnknownAction => f.write_str("未知动作"),
            KeymapError::DuplicateBinding(chord) => {
                write!(f, "同一上下文里键位重复：{chord}")
            }
        }
    }
}

impl std::error::Error for KeymapError {}

/// 键位表：以 [`default_actions()`] 为底，允许配置覆盖。
#[derive(Debug, Clone)]
pub struct Keymap {
    defs: Vec<ActionDef>,
}

impl Default for Keymap {
    fn default() -> Self {
        Keymap::from_defaults()
    }
}

impl Keymap {
    pub fn from_defaults() -> Self {
        Keymap {
            defs: default_actions(),
        }
    }

    /// 用配置里的键位覆盖默认值（覆盖 `default_key`，并清掉 `alt_keys`，避免留下意料外的老键位）。
    ///
    /// 配置文件的解析留给宿主：这里只接受已解析好的 [`KeyChord`]，
    /// 字符串写法走 [`KeyChord::parse`]。
    pub fn with_overrides(overrides: &[(ActionId, KeyChord)]) -> Self {
        let mut keymap = Keymap::from_defaults();
        for (id, chord) in overrides {
            if let Some(def) = keymap.defs.iter_mut().find(|def| def.id == *id) {
                def.default_key = *chord;
                def.alt_keys = &[];
            }
        }
        keymap
    }

    pub fn defs(&self) -> &[ActionDef] {
        &self.defs
    }

    pub fn action(&self, id: ActionId) -> Option<&ActionDef> {
        self.defs.iter().find(|def| def.id == id)
    }

    /// 某动作当前生效的键位（覆盖后也正确）。
    pub fn key_for(&self, id: ActionId) -> Option<KeyChord> {
        self.action(id).map(|def| def.default_key)
    }

    /// 键位查表：先查本上下文与 `Always`，`Prompt` 再下溢一档到 `ScrollbackFocused`
    /// （这样翻页键在输入时依然可用，形状取自上游「冒泡顺序由调用方写死」的做法，
    /// `actions/mod.rs:8-15`）。`Approval` 不下溢，保证权限键优先。
    pub fn lookup(&self, owner: KeyOwner, chord: KeyChord) -> Option<ActionId> {
        let context = owner.context();
        self.lookup_in(context, chord).or_else(|| {
            if owner == KeyOwner::Prompt {
                self.lookup_in(When::ScrollbackFocused, chord)
            } else {
                None
            }
        })
    }

    fn lookup_in(&self, context: When, chord: KeyChord) -> Option<ActionId> {
        self.defs
            .iter()
            .find(|def| {
                (def.context == context || def.context == When::Always)
                    && (def.default_key == chord || def.alt_keys.contains(&chord))
            })
            .map(|def| def.id)
    }

    /// 快捷键栏数据源：按 `hint_priority` 升序（`Some(0)` 最靠前），只取本上下文可见的条目。
    ///
    /// 上游同样是「哪些动作上栏由 `hint_priority` 决定」（`actions/defaults.rs` 里只有 8 条设了它）。
    pub fn hints(&self, owner: KeyOwner) -> Vec<(KeyChord, &'static str)> {
        let mut items: Vec<&ActionDef> = self
            .defs
            .iter()
            .filter(|def| {
                def.hint_priority.is_some()
                    && (def.context == owner.context()
                        || def.context == When::Always
                        || (owner == KeyOwner::Prompt && def.context == When::ScrollbackFocused))
            })
            .collect();
        items.sort_by_key(|def| (def.hint_priority.unwrap_or(u8::MAX), def.label));
        items
            .into_iter()
            .map(|def| (def.default_key, def.label))
            .collect()
    }

    /// 注册表完备性检查：同一上下文里不允许同键双绑（上游没有这道检查，冲突只在
    /// 快捷键手册渲染时去重，`views/shortcuts_help.rs:137-148`；本项目在构建时就认输）。
    ///
    /// `Always` 动作在所有上下文里可见，因此它与任何上下文里的同键绑定都算冲突；
    /// 两个不同上下文（且都不含 `Always`）里的同键绑定不算冲突——那不是冲突而是分支。
    pub fn validate(&self) -> Result<(), KeymapError> {
        for (index, def) in self.defs.iter().enumerate() {
            for chord in std::iter::once(def.default_key).chain(def.alt_keys.iter().copied()) {
                for other in &self.defs[index + 1..] {
                    let overlapping = def.context == other.context
                        || def.context == When::Always
                        || other.context == When::Always;
                    if overlapping
                        && (other.default_key == chord || other.alt_keys.contains(&chord))
                    {
                        return Err(KeymapError::DuplicateBinding(chord));
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(code: KeyCode, modifiers: KeyModifiers) -> KeyChord {
        KeyChord::new(code, modifiers)
    }

    #[test]
    fn registry_covers_every_action_id() {
        let defs = default_actions();
        for id in ActionId::ALL {
            assert!(defs.iter().any(|def| def.id == *id), "注册表缺少 {id:?}");
        }
        assert_eq!(defs.len(), ActionId::ALL.len());
    }

    #[test]
    fn default_registry_has_no_duplicate_bindings() {
        Keymap::from_defaults().validate().unwrap();
    }

    #[test]
    fn lookup_is_context_scoped() {
        let keymap = Keymap::from_defaults();
        let up = chord(KeyCode::Up, KeyModifiers::NONE);
        // 同一个键在不同上下文里是两个动作——上游 `When` 精确匹配的同一思路。
        assert_eq!(
            keymap.lookup(KeyOwner::Prompt, up),
            Some(ActionId::CursorUp)
        );
        assert_eq!(
            keymap.lookup(KeyOwner::Scrollback, up),
            Some(ActionId::ScrollLineUp)
        );
        // 翻页键只注册在 scrollback 下，输入时下溢一档仍然可用。
        let page_up = chord(KeyCode::PageUp, KeyModifiers::NONE);
        assert_eq!(
            keymap.lookup(KeyOwner::Prompt, page_up),
            Some(ActionId::ScrollPageUp)
        );
        assert_eq!(
            keymap.lookup(KeyOwner::Scrollback, page_up),
            Some(ActionId::ScrollPageUp)
        );
    }

    #[test]
    fn quit_is_always_available() {
        let keymap = Keymap::from_defaults();
        let quit = chord(KeyCode::Char('c'), KeyModifiers::CONTROL);
        for owner in [KeyOwner::Prompt, KeyOwner::Scrollback, KeyOwner::Approval] {
            assert_eq!(keymap.lookup(owner, quit), Some(ActionId::Quit));
        }
    }

    #[test]
    fn permission_keys_win_and_do_not_leak_into_the_prompt() {
        // 把「发送」也绑到 `y` 上：审批卡在时 `y` 仍是权限键，不被遮蔽。
        let keymap = Keymap::with_overrides(&[(
            ActionId::Submit,
            chord(KeyCode::Char('y'), KeyModifiers::NONE),
        )]);
        let y = chord(KeyCode::Char('y'), KeyModifiers::NONE);
        assert_eq!(
            keymap.lookup(KeyOwner::Approval, y),
            Some(ActionId::ApproveOnce),
            "权限键必须优先于其他上下文的同键绑定"
        );
        assert_eq!(keymap.lookup(KeyOwner::Prompt, y), Some(ActionId::Submit));
        assert_eq!(
            keymap.lookup(
                KeyOwner::Approval,
                chord(KeyCode::Enter, KeyModifiers::NONE)
            ),
            None,
            "审批卡不继承提示框的提交键"
        );
    }

    #[test]
    fn overrides_replace_keys_and_clear_alt_keys() {
        let keymap = Keymap::with_overrides(&[(
            ActionId::Submit,
            chord(KeyCode::Char('s'), KeyModifiers::CONTROL),
        )]);
        assert_eq!(
            keymap.key_for(ActionId::Submit),
            Some(chord(KeyCode::Char('s'), KeyModifiers::CONTROL))
        );
        // 老键位不再命中（`alt_keys` 被清掉）。
        assert_eq!(
            keymap.lookup(KeyOwner::Prompt, chord(KeyCode::Enter, KeyModifiers::NONE)),
            None
        );
        assert_eq!(
            keymap.lookup(
                KeyOwner::Prompt,
                chord(KeyCode::Char('s'), KeyModifiers::CONTROL)
            ),
            Some(ActionId::Submit)
        );
    }

    #[test]
    fn hints_are_ordered_and_context_aware() {
        let keymap = Keymap::from_defaults();
        let prompt_hints = keymap.hints(KeyOwner::Prompt);
        assert_eq!(prompt_hints[0].1, "退出");
        assert!(prompt_hints.iter().any(|(_, label)| *label == "发送"));
        assert!(prompt_hints.iter().any(|(_, label)| *label == "上翻"));

        let approval_hints = keymap.hints(KeyOwner::Approval);
        assert_eq!(approval_hints[0].1, "允许一次");
        assert!(!approval_hints.iter().any(|(_, label)| *label == "换行"));
    }

    #[test]
    fn chord_normalization_folds_shift_into_uppercase() {
        // `Shift+g` 折成 `G`（上游 input/key.rs:46-66）。
        let shifted = chord(KeyCode::Char('g'), KeyModifiers::SHIFT);
        assert_eq!(shifted, chord(KeyCode::Char('G'), KeyModifiers::SHIFT));
        // 大写字符自带 SHIFT。
        let upper = chord(KeyCode::Char('G'), KeyModifiers::NONE);
        assert_eq!(upper, chord(KeyCode::Char('G'), KeyModifiers::SHIFT));
    }

    #[test]
    fn release_events_never_match() {
        let mut event = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        event.kind = KeyEventKind::Release;
        assert_eq!(KeyChord::from_event(event), None);
        let press = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(
            KeyChord::from_event(press),
            Some(KeyChord::plain(KeyCode::Enter))
        );
    }

    #[test]
    fn parse_accepts_config_written_chords() {
        assert_eq!(
            KeyChord::parse("ctrl+shift+p"),
            Some(KeyChord::new(
                KeyCode::Char('P'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT
            ))
        );
        assert_eq!(
            KeyChord::parse("alt+enter"),
            Some(KeyChord::alt(KeyCode::Enter))
        );
        assert_eq!(
            KeyChord::parse("pgup"),
            Some(KeyChord::plain(KeyCode::PageUp))
        );
        assert_eq!(KeyChord::parse("f5"), Some(KeyChord::plain(KeyCode::F(5))));
        assert_eq!(
            KeyChord::parse("?"),
            Some(KeyChord::plain(KeyCode::Char('?')))
        );
        assert_eq!(KeyChord::parse(""), None);
        assert_eq!(KeyChord::parse("ctrl+"), None);
        assert_eq!(KeyChord::parse("f99"), None);
        assert_eq!(KeyChord::parse("ctrl+enter+esc"), None);
        assert_eq!(KeyChord::parse(" ctrl+p"), None);
    }

    #[test]
    fn display_matches_shortcut_bar_shape() {
        assert_eq!(KeyChord::ctrl(KeyCode::Char('c')).display(), "Ctrl+c");
        assert_eq!(KeyChord::alt(KeyCode::Enter).display(), "Alt+Enter");
        assert_eq!(KeyChord::plain(KeyCode::PageUp).display(), "PgUp");
        assert_eq!(KeyChord::plain(KeyCode::Char('y')).display(), "y");
    }

    #[test]
    fn validate_catches_duplicates() {
        // 手工构造一个冲突表：把 Up 同时绑给两个 PromptFocused 动作。
        let mut keymap = Keymap::from_defaults();
        for def in keymap.defs.iter_mut() {
            if def.id == ActionId::CursorDown {
                def.default_key = KeyChord::plain(KeyCode::Up);
            }
        }
        assert!(matches!(
            keymap.validate(),
            Err(KeymapError::DuplicateBinding(_))
        ));
    }
}
