//! # jh-tui —— jev-harness 的终端 UI 组件套件
//!
//! 按 Grok Build TUI 的**机制**做的单 crate 组件套件：单一来源（主题 / 字形 / 动作
//! 注册表）、块是纯函数 `output(ctx)`、行级元数据一次产出、按需渲染、保留信号恢复护栏。
//!
//! ## 设计来源
//!
//! - 取舍口径：`docs/grok-tui-design.md` §7（「方向可抄、量级要砍」，§7.1 十条可借鉴、
//!   §7.2 八条代价过高的做法、§7.3 建议的最小骨架）。
//! - 逐条事实（含 `路径:行号`）：`docs/research/grok-tui/` 的专题
//!   01（主题与色深降级）、02（字形与同宽回退）、09（块模型与行级元数据）、
//!   11（提示框）、16（键盘与动作注册表）、19（审批与阻塞卡）、
//!   21（会话生命周期与终端状态）、25（渲染管线性能）、26（依赖栈）、27（测试策略）。
//!
//! ## 本批次范围边界（设计 §7.2）
//!
//! 做：主题（一套写死的深色配色 + 色深降级）、字形（每字形一个函数 + 同宽回退）、
//! 宽度工具、块模型（用户 / 助手 / 工具 / 决策轨迹 / 阶段行）、组件（顶栏 / 回滚区 /
//! 四边盒提示框 / 面板壳 / 状态行 / 快捷键栏 / 审批卡）、三屏（首屏 / 主界面 / 研究视图）、
//! 研究视图的三个面板（判断详情 / 事件与证据 / 失效传播）、动作注册表（键位可覆盖）、
//! 斜杠命令注册表与候选列表（模糊匹配）、助手正文的轻量 markdown（含 GFM 管道表格）、
//! 鼠标（点击命中区 + 滚轮翻页 + **拖选文本**）、命令的固定选项列表（`↑↓` + `Enter`）、
//! 剪贴板写入（**OSC 52**，[`terminal::copy_to_clipboard`]）、三条件重绘闸门、终端护栏、
//! 离线快照测试。
//!
//! **不接线**：本 crate 不依赖 `jh-core` / `jh-app`，面板数据与提示框信息行由宿主喂入；
//! 单步与暂停只产出 [`app::Effect`]，真实运行状态在宿主那里。宿主（`jev chat` 的会话）
//! 落在 `jh-app/src/session/**`，实现口径见 `docs/tui-design.md` §9.1。
//!
//! 不做：多形态渲染（只做 alt-screen 全屏）、主题热切换、命令面板全屏页、
//! 折叠分组、语法高亮（不引入 `syntect`）、**完整** Markdown 解析（不引入
//! `pulldown-cmark`；助手正文只做 [`markdown`] 的轻量子集：标题 / 列表 / 围栏代码块 /
//! 引用 / 分隔线 / 四种行内标记 / GFM 管道表格，不做嵌套列表与语法高亮）、Mermaid 与内联图片
//! （上游最该砍的依赖栈）、元测试与 PTY e2e（只保留 `TestBackend` 缓冲区快照这一档）。
//!
//! ## 模块地图
//!
//! | 模块 | 职责 |
//! | --- | --- |
//! | [`theme`] | 扁平 `Theme` + `ColorLevel`（None < Basic < Ansi256 < TrueColor）+ 量化 |
//! | [`glyphs`] | 每个字形一个函数，每个 (花体, 回退) 对同宽 |
//! | [`width`] | 显示列度量、grapheme 整簇裁剪与折行 |
//! | [`markdown`] | 助手正文的轻量 markdown 子集：解析成带样式的行 + 按显示宽度折行 |
//! | [`blocks`] | 块 = 纯函数 `output(ctx)`，产出已换行的屏幕行 + 行级元数据 |
//! | [`actions`] | `ActionId` 编译期枚举 + `default_actions()` 单一注册表 + 键位覆盖 |
//! | [`commands`] | 斜杠命令注册表、解析与候选（模糊匹配给补全列表用） |
//! | [`components`] | 顶栏 / 回滚区 / 提示框 / 面板 / 状态行 / 快捷键栏 / 审批卡 |
//! | [`screens`] | `Screen`（首屏 / 主界面 / 研究视图）与三屏的纯函数布局 |
//! | [`panels`] | 研究视图的三个面板：判断详情 / 事件与证据 / 失效传播 |
//! | [`startup`] | 首屏面板：字标 + 版本 + 概览 + 菜单 |
//! | [`render`] | 三条件重绘闸门 + 唯一绘制入口（按屏分发） |
//! | [`app`] | 应用状态与纯调度（副作用描述成 [`app::Effect`]） |
//! | [`terminal`] | 能力位 + 进入/退出/幂等恢复护栏 |
//!
//! 屏幕与交互的设计基准见 `docs/tui-design.md`。
//!
//! ## 用法
//!
//! ```
//! use jh_tui::app::App;
//! use jh_tui::render;
//! use jh_tui::terminal::Capabilities;
//! use ratatui::{Terminal, backend::TestBackend};
//!
//! // 能力位决定主题与字形；业务代码不读环境变量。
//! let capabilities = Capabilities::from_env_lookup(|_| None, true, false);
//! let mut app = App::new(&capabilities);
//! app.prompt_mut().insert_str("你好");
//!
//! let mut terminal = Terminal::new(TestBackend::new(40, 8)).unwrap();
//! terminal.draw(|frame| render::draw(frame, &mut app)).unwrap();
//! let screen = render::screen_lines(terminal.backend().buffer());
//! assert!(screen.iter().any(|line| line.contains("你好")));
//! ```
//!
//! 真实终端里的进入/退出、模式开关（raw mode / 备用屏 / bracketed paste）、
//! 信号恢复与 panic hook 见 [`terminal::TerminalGuard`] 与 `examples/design.rs`
//! （`cargo run -p jh-tui --example design` 在非 TTY 下逐屏打印设计预览）。

pub mod actions;
pub mod app;
pub mod blocks;
pub mod commands;
pub mod components;
pub mod glyphs;
pub mod markdown;
pub mod panels;
pub mod render;
pub mod screens;
pub mod startup;
pub mod terminal;
pub mod theme;
pub mod width;
