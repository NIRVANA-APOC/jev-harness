# 专题 26：依赖、版本与 feature 门控清单

> 专题：依赖、版本与 feature 门控清单｜调研日期：2026-09-22｜证据范围：`Cargo.toml`（workspace 根）、`Cargo.lock`、`rust-toolchain.toml`、`clippy.toml`、`rustfmt.toml`、`.cargo/config.toml`、`SOURCE_REV`、`crates/codegen/{xai-grok-pager, xai-grok-pager-render, xai-grok-pager-bin, xai-grok-pager-minimal, xai-grok-pager-diff, xai-grok-mermaid, xai-grok-markdown, xai-grok-markdown-core, xai-grok-status-line, xai-ratatui-inline, xai-ratatui-textarea, xai-grok-shell}/Cargo.toml`、`third_party/{README.md, NOTICE, */Cargo.toml}`、`THIRD-PARTY-NOTICES`（结构与统计）、`crates/codegen/xai-grok-tools/THIRD_PARTY_NOTICES.md`。未运行 `cargo build/test/clippy`，二进制体积与编译时长未测量。

## 0. 一页速览

| 项 | 值 | 证据 |
| --- | --- | --- |
| 工具链 | Rust `1.94.0`，含 `rustfmt` + `clippy`，固定两个 linux-gnu target | `rust-toolchain.toml:11-17` |
| edition / license | `2024` / `Apache-2.0` | `Cargo.toml:112-114` |
| workspace 规模 | 101 个 member | `Cargo.toml:8-110` |
| lock 规模 | 1347 个 package：registry 1242 + in-tree path 101 + git 4 | `Cargo.lock`（`version = 4`，`Cargo.lock:3`） |
| pager 直接依赖 | 87 条 = workspace 内部 39 + 第三方 48 | `crates/codegen/xai-grok-pager/Cargo.toml:8-172` |
| pager-render 直接依赖 | 47 条 = 内部 12 + 第三方 35 | `crates/codegen/xai-grok-pager-render/Cargo.toml:7-94` |
| 最关键的 TUI 版本 | ratatui 0.29.0、ratatui-core 0.1.0、crossterm 0.28.1、textwrap 0.16.2、syntect 5.3.0、similar 2.7.0 | `Cargo.lock:8602`、`Cargo.lock:8623`、`Cargo.lock:2312`、`Cargo.lock:10575` |
| vendored 源码 | 4 个 crate（Mermaid 布局栈），无 crates.io 等价物 | `third_party/README.md:16-25`、`Cargo.toml:106-109` |
| SOURCE_REV | `9bb727ccdff0a793ee73bcde4e2e09cbef6b5387` | `SOURCE_REV:1` |

## 1. 工具链、edition、lint 与格式化配置

### 1.1 工具链

```
channel    = "1.94.0"                         # rust-toolchain.toml:11
components = ["rustfmt", "clippy"]            # :12
profile    = "default"                        # :13
targets    = [x86_64-unknown-linux-gnu, aarch64-unknown-linux-gnu]   # :14-17
```

文件内注释要求「手动升级、一次只跳一个小版本、发布后等两周」，并在升级后跑 `cargo check --all-targets --workspace` 与 `cargo clippy --all-targets --workspace`（`rust-toolchain.toml:3-10`）。仓库根 `Cargo.toml` 首行明确写了它是**自动生成**的、改动应落到各 crate 的清单（`Cargo.toml:1`）；这解释了为什么 `[workspace.dependencies]` 集中托管版本，而绝大多数 crate 用 `{ workspace = true }` 继承。

### 1.2 edition 与 profile

`edition = "2024"` 是 workspace 统一的（`Cargo.toml:113`），所有 TUI crate 都是 `edition.workspace = true`（如 `crates/codegen/xai-grok-pager/Cargo.toml:4`）。`resolver = "2"`（`Cargo.toml:7`）。

与二进制体积/发布直接相关的 profile：

| profile | 关键设置 | 证据 |
| --- | --- | --- |
| `release` | `incremental = true`、`panic = "abort"` | `Cargo.toml:387-389` |
| `release-dist` | 继承 release，`lto = "thin"`、`codegen-units = 1`、`strip = false`、`debug = 1` | `Cargo.toml:400-406` |
| `admin-cli-dist` | `opt-level = "z"`、`lto = true`、`strip = true` | `Cargo.toml:430-435` |
| `dev` | `panic = "abort"`、`codegen-units = 128`、`debug = "line-tables-only"`；`[profile.dev.package."*"] debug = false` | `Cargo.toml:437-448` |

注释解释了为什么 `release-dist` 不设为默认：本地构建会慢约 2.2 倍（`Cargo.toml:391-399`，该数字来自注释，本次未复测）。

### 1.3 clippy 配置（`clippy.toml`）

值得注意的是这个文件**躺在仓库根，但自己声明是 `crates/codegen/**` 的配置**，并要求「复制 repo-root //clippy.toml 的任何设置」（`clippy.toml:1-4`）；本次全仓查找只有这一个 `clippy.toml`，被它引用的「//clippy.toml」在快照中不存在。

可迁移到本项目的三条：

1. `large-error-threshold = 256`（`clippy.toml:7`）——为 tonic 生成代码放宽大 `Err` 变体告警。
2. `allow-indexing-slicing-in-tests = true`（`clippy.toml:13`），配套约定是「生产 crate 在 crate 根用 `#![deny(clippy::indexing_slicing)]` 自行开启」（`clippy.toml:9-12`）。
3. `disallowed-methods` 共 12 条（`clippy.toml:40-54`），是**团队级架构约束用 lint 表达**的范例：

```
reqwest::Client::new / ClientBuilder::build      -> 必须走 xai_grok_extra_ca（TLS 策略）  :41-44
std::fs::canonicalize / Path::canonicalize       -> 用 dunce::canonicalize（Windows \\?\）:45-46
tokio::fs::canonicalize                          -> 同上                                 :47
std::env::home_dir / dirs::home_dir              -> 统一走 xai_dirs::home_dir            :48,51
std::process::Command::spawn / tokio::process::… -> 必须 xai_tty_utils::ProcessScope::enroll :52-53
portable_pty::SlavePty::spawn_command            -> 同上（终端 pid 登记）                 :54
```

`allowed-invalid`/`allow-invalid` 与「哪些由 cargo clippy 强制、哪些由 Bazel 强制」的差别也写在注释里（`clippy.toml:37-39`、`clippy.toml:49-51`）。

### 1.4 rustfmt 与 workspace lints

`rustfmt.toml` 只有一行：`use_field_init_shorthand = true`（`rustfmt.toml:1`）——即风格几乎完全服从 rustfmt 默认，符合 `AGENTS.md` 里「代码风格服从工具默认」这一取向。

`[workspace.lints.clippy]`（`Cargo.toml:453-477`）全部是**allow**，没有 deny 项：`doc_lazy_continuation`、`doc_overindented_list_items`、`needless_lifetimes`、`single_range_in_vec_init`、`too_many_arguments`（前四项：与 proto 生成代码/历史代码妥协），以及 `uninlined_format_args`、`useless_format`（后两项：为无 merge queue 的团队流程和 fastrace 宏代码让路，注释写明了理由与解除条件）。83 个 `crates/` 下的 `Cargo.toml` 通过 `[lints] workspace = true` 选择性继承——pager 自己也这么做，并在注释里说明原因（`crates/codegen/xai-grok-pager/Cargo.toml:234-238`）。

### 1.5 `.cargo/config.toml`

只有 rustflags，无 alias：Darwin 加 `-undefined dynamic_lookup` 与 `-ObjC`（`.cargo/config.toml:10-32`）；linux-gnu 加 `-C force-unwind-tables=yes`（`:34-38`），aarch64-gnu 额外加 `-C target-cpu=neoverse-v2`（`:38`）；两个 musl target 加链接硬化 `-Wl,-z,relro,-z,now,-z,noexecstack`（`:40-59`，注释标明这是 Full RELRO + NX stack）。

## 2. TUI 相关依赖总表

版本列格式为「workspace 声明 / Cargo.lock 实际锁定」。只有 `pager`、`pager-render`、`xai-ratatui-inline`、`xai-ratatui-textarea`、`xai-grok-markdown` 五个 crate 里的才算「TUI 运行时依赖」。

### 2.1 渲染框架与终端 I/O

| crate | 版本 | 在 TUI 里的职责 | 可选 | 证据 |
| --- | --- | --- | --- | --- |
| ratatui | `0.29` / 0.29.0 | TUI 框架本体（Buffer / Widget / Layout / Frame）。pager 启用 `crossterm` + `unstable-widget-ref`；render 追加 `unstable-backend-writer` | 否 | `Cargo.toml:233`、`Cargo.lock:8602`、`crates/codegen/xai-grok-pager/Cargo.toml:27`、`.../xai-grok-pager-render/Cargo.toml:13-17` |
| ratatui-core | `0.1` / 0.1.0 | 与 ratatui 0.29 **并存**的下一代核心 API；`tui-scrollbar 0.2.2` 依赖它 | 否 | `Cargo.toml:234`、`Cargo.lock:8623`、`Cargo.lock`（tui-scrollbar → ratatui-core） |
| crossterm | `0.28` / 0.28.1 | 终端后端：raw mode、事件流、鼠标、bracketed paste（feature `event-stream` + `bracketed-paste`） | 否 | `Cargo.toml:150`、`Cargo.lock:2312`、`crates/codegen/xai-grok-pager/Cargo.toml:29` |
| tui-scrollbar | `0.2` / 0.2.2 | 滚动条组件（render 与 textarea 共用） | 否 | `Cargo.toml:296`、`.../xai-grok-pager-render/Cargo.toml:27`、`.../xai-ratatui-textarea/Cargo.toml:13` |
| vte | `0.15.0` / 0.15.0（另有 0.14.1） | VTE 状态机，把命令输出按 SGR/光标/`\r` 原生渲染 | 否 | `Cargo.toml:306`、`.../xai-grok-pager-render/Cargo.toml:44`、`.../src/render/terminal_output.rs:11` |
| ansi-to-tui | `7.0.0` / 7.0.0 | ANSI 文本 → ratatui 文本（tracing 面板、status line 清洗） | 否 | `Cargo.toml:119`、`Cargo.lock:190`、`crates/codegen/xai-grok-pager/Cargo.toml:55`（注释「ANSI parsing (tracing pane)」）、`.../src/tracing.rs`、`.../src/views/status_line/sanitize.rs` |
| ratatui 版本并行 | — | 同一依赖面里 `ratatui`/`ratatui-core`、`compact_str` 0.8.1/0.9.0、`itertools` 0.13/0.14、`lru` 0.12.5/0.16.3、`unicode-truncate` 1.1/2.0.1 均不同代 | — | `Cargo.lock`（见 §6） |

### 2.2 文本、Unicode 与样式

| crate | 版本 | 职责 | 可选 | 证据 |
| --- | --- | --- | --- | --- |
| textwrap | `0.16` / 0.16.2 | 折行（覆盖层、隐私横幅、列表面板、启动失败页） | 否 | `Cargo.toml:273`、`crates/codegen/xai-grok-pager/Cargo.toml:30`、`.../src/views/btw_overlay.rs`、`.../src/views/list_pane/mod.rs` |
| unicode-width | `0.2` / 0.2.0（传递引入 0.1.14） | 显示宽度（CJK/emoji 双宽） | 否 | `Cargo.toml:302`、`crates/codegen/xai-grok-pager/Cargo.toml:32` |
| unicode-segmentation | `1.12.0` / 1.12.0 | 字素簇边界 | 否 | `Cargo.toml:301`、`crates/codegen/xai-grok-pager/Cargo.toml:33` |
| strip-ansi-escapes | `0.2.1` / 0.2.1 | 去转义后再测量/落盘（标题、peek、会话事件） | 否 | `Cargo.toml:265`、`crates/codegen/xai-grok-pager/Cargo.toml:34`、`.../src/views/session_title.rs` |
| unicode-bidi / unicode-bidi-mirroring | `0.3` / `0.4`（分别 0.3.18 / 0.4.0） | 双向文本与括号镜像（仅 render） | 否 | `Cargo.toml:298-299`、`.../xai-grok-pager-render/Cargo.toml:22-23` |
| linkify | `0.10` / 0.10.0 | URL 识别 | 否 | `Cargo.toml:197`、`.../xai-grok-pager-render/Cargo.toml:26` |
| anstyle / anstyle-lossy / anstyle-parse / anstyle-syntect | `1.0` / `1.1.4` / `0.2` / `1.0.4` | ANSI 样式模型与解析、与 syntect 的互转桥 | 否 | `Cargo.toml:121-124`、`.../xai-ratatui-inline/Cargo.toml:12`、`.../xai-grok-markdown/Cargo.toml:13-15` |
| supports-color | `3.0` / 3.0.2 | 颜色能力探测 | 否 | `Cargo.toml:267`、`.../xai-grok-pager-render/Cargo.toml:33` |
| dark-light | `2` / 2.0.0 | 系统深浅色探测（render 直接写版本号，未走 workspace） | 否 | `.../xai-grok-pager-render/Cargo.toml:47` |

### 2.3 高亮、diff、搜索、Markdown

| crate | 版本 | 职责 | 可选 | 证据 |
| --- | --- | --- | --- | --- |
| syntect | `5.3`（`default-features = false`，`features = ["default-onig"]`）/ 5.3.0 | TextMate 语法高亮；**走 onig 而非纯 Rust 后端**，因此引入 C 依赖 `onig_sys 69.9.3` | 否 | `Cargo.toml:268`、`.../xai-grok-pager-render/src/syntax.rs`、`Cargo.lock` |
| two-face | `0.4`（`default-features = false`，`features = ["syntect-onig"]`）/ 0.4.5 | 扩充语法/主题集；仅被 `xai-grok-markdown` 的 **build-dependency** 使用 | 否（构建期） | `Cargo.toml:297`、`.../xai-grok-markdown/Cargo.toml:37-39`、`.../build.rs:1-24` |
| similar | `2.7` / 2.7.0 | 行级 diff（edit 块、block viewer、edit_highlight_worker） | 否 | `Cargo.toml:262`、`crates/codegen/xai-grok-pager/Cargo.toml:45`、`.../src/scrollback/blocks/tool/edit.rs` |
| indexmap | `2`（`serde`）/ 2.14.0 | 有序 map，保证面板/行顺序稳定 | 否 | `Cargo.toml:189`、`crates/codegen/xai-grok-pager/Cargo.toml:46` |
| nucleo | git `helix-editor/nucleo` rev `5b74652` / 0.5.0（+ `nucleo-matcher` 0.3.1） | 模糊匹配：slash 命令、历史搜索、文件搜索 | 否 | `Cargo.toml:209`、`Cargo.lock`（git 源）、`crates/codegen/xai-grok-pager/Cargo.toml:87-88`、`.../src/slash/matcher.rs:3-10` |
| pulldown-cmark | `0.13` / 0.13.0 | Markdown 解析；`xai-grok-markdown-core` 存在的目的是「共享 Grok Build 完全相同的 pulldown-cmark 配置」 | 否 | `Cargo.toml:230`、`.../xai-grok-markdown-core/Cargo.toml:6-10` |

### 2.4 输入、PTY、进程与工具链

| crate | 版本 | 职责 | 可选 | 证据 |
| --- | --- | --- | --- | --- |
| portable-pty | `0.9` / 0.9.0 | PTY：`grok wrap` 包装器、e2e、benchmark | 否（但 pager 里**直接写版本号**而非 `workspace = true`，尽管 workspace 也声明了 0.9） | `crates/codegen/xai-grok-pager/Cargo.toml:57-58`、`.../src/pty_wrap.rs`、`Cargo.toml:223` |
| alacritty_terminal | `0.26.0` | 终端仿真内核；**不在 pager 二进制内**，只在 `ptyctl` 与 `xai-grok-pager-pty-harness`（spawn-only 测试/基准）中 | 否（工具链/测试） | `Cargo.toml:118`、`crates/codegen/ptyctl/Cargo.toml:10`、`crates/codegen/xai-grok-pager-pty-harness/Cargo.toml:28` |
| termwiz | `0.23`（`default-features = false`）/ 0.23.3 | 仅 `xai-ratatui-inline` 的 **dev-dependency**：分割器的差分参考实现 | dev only | `Cargo.toml:272`、`.../xai-ratatui-inline/Cargo.toml:24-26` |
| ansi-width | `0.1` / 0.1.0 | 仅 `xai-ratatui-inline` 的 dev-dependency（宽度基准） | dev only | `Cargo.toml:120`、`.../xai-ratatui-inline/Cargo.toml:18` |
| clap / clap_complete | `4`（`derive`,`env`）/ 4.5.53 | 子命令与参数解析（doctor、sessions、mcp、completions 等）、shell 补全 | 否 | `Cargo.toml:143-144`、`crates/codegen/xai-grok-pager/Cargo.toml:64-65`、`.../src/doctor_cmd/mod.rs` |
| tokio | `1`（workspace 声明 `features = ["full"]`）/ 1.52.3 | 异步运行时 | 否 | `Cargo.toml:280`、`crates/codegen/xai-grok-pager/Cargo.toml:67-75` |
| tokio-util | `0.7`（`compat`） | futures ⇄ tokio 互操作，接 crossterm 事件流 | 否 | `Cargo.toml:284`、`crates/codegen/xai-grok-pager/Cargo.toml:77` |
| obfstr | `0.4` / 0.4.4 | 编译期字符串混淆（token、auth 机制名、密钥环境变量名） | 否 | `Cargo.toml:212`、`crates/codegen/xai-grok-pager/Cargo.toml:82-85`、`.../src/obf.rs:3,13` |
| image | `0.25.10` + `png/jpeg/tiff/gif/webp` | 剪贴板图片解码/缩放校验 | 否 | `Cargo.toml:188`、`crates/codegen/xai-grok-pager/Cargo.toml:146` |
| git2 | `0.20` + `vendored-libgit2` | 启动时一次性采集 git 信息（`spawn_blocking`，渲染路径不碰 git） | 否 | `Cargo.toml:177`、`crates/codegen/xai-grok-pager/Cargo.toml:154-158` |
| shellexpand | `3.1` / 3.1.2 | `~` 展开（pager/render 各自直接写版本） | 否 | `crates/codegen/xai-grok-pager/Cargo.toml:152`、`.../xai-grok-pager-render/Cargo.toml:91` |
| resvg / tiny-skia / fontdb / which / wait-timeout | `0.47` / `0.12` / `0.23` / `8` / `0.2` | Mermaid SVG→PNG 光栅化、字体、外部渲染器查找与超时 | 否 | `Cargo.toml:241,279,170,311,307`、`crates/codegen/xai-grok-mermaid/Cargo.toml:12-21` |

### 2.5 平台相关依赖（按 target 分节）

| target | crate | 职责 | 证据 |
| --- | --- | --- | --- |
| unix | `nix`（`signal`）/ `signal-hook` / `libc` | 信号处理、`SIGWINCH`（`grok wrap`）、`theme/osc11.rs` 的非阻塞终端读 | `crates/codegen/xai-grok-pager/Cargo.toml:170-176` |
| macOS | `core-foundation` | IOKit 电源断言（`SleepInhibitor`） | `crates/codegen/xai-grok-pager/Cargo.toml:178-180` |
| windows | `windows-sys 0.59` | `SetConsoleCtrlHandler`（`/minimal`、`/fullscreen` 重启动）；render 侧 `EnumDisplaySettingsW`、`ShellExecuteW` | `crates/codegen/xai-grok-pager/Cargo.toml:182-187`、`.../xai-grok-pager-render/Cargo.toml:103-110` |
| unix | `libc`（render） | `theme/osc11.rs` 的 poll/read | `.../xai-grok-pager-render/Cargo.toml:99-101` |

## 3. pager 依赖的 workspace 内部 crate（39 个）

`crates/codegen/xai-grok-pager/Cargo.toml:8-172` 里共 87 条依赖，其中 39 条是 `xai-*`/内部 crate。按层次分组（括号内为该清单行号）：

**表现层原语（可直接搬走的那一层）**
- `xai-grok-pager-render`（18）——被抽出的「表现层原语」crate
- `xai-ratatui-textarea`（91）、`xai-ratatui-inline`（92）——输入框与 inline 滚动区终端
- `xai-grok-markdown`（37）、`xai-grok-gboom`（21）、`xai-grok-pager-diff`（24）

**agent / 协议 / 工具**
- `xai-grok-agent`（134）、`xai-grok-shell`（116）、`xai-acp-lib`（99）、`agent-client-protocol`（98）
- `xai-tool-types`（107）、`xai-grok-tools`（133）、`xai-prompt-queue`（17-18，「共享 prompt-queue wire types」）
- `xai-chat-state`（100，可选）、`xai-grok-sampling-types`（106，可选）

**配置 / 数据 / 会话**
- `xai-grok-config`（104）、`xai-grok-status-line`（120，注释说明把它做成叶子 crate 是为了让客户端从上游读取契约）
- `xai-grok-active-sessions`（103，「崩溃恢复用的 TUI 会话登记表」）、`xai-grok-dashboard-store`(109)、`xai-grok-foreign-sessions`（123）
- `xai-token-estimation`（124）、`xai-dirs`（108）、`xai-file-utils`（101）、`xai-hooks-plugins-types`（138）、`xai-grok-hooks`（137）、`xai-grok-plugin-marketplace`（135）

**运维 / 平台适配**
- `xai-crash-handler`（165）、`xai-grok-telemetry`（125）、`xai-grok-otel`（126）、`xai-grok-update`（95）、`xai-grok-version`（12）
- `xai-grok-feedback`（105）、`xai-grok-announcements`（28）、`xai-grok-login`（115）、`xai-grok-workspace`（121）、`xai-grok-sandbox`（140，`default-features = false`）
- `xai-fast-worktree`（127，`metadata` feature）、`xai-tty-utils`（11，「TTY 安全：把子进程从控制终端上摘下来」）、`xai-grok-voice`（114，`default-features = false` + `audio`）

依赖方向（含组合根）：

```
xai-grok-pager-bin  (组合根 binary；`pager-bin/Cargo.toml:9-13,20,22,59-60,70-77`)
├── xai-grok-pager                     ← 应用逻辑与视图模型
│   ├── xai-grok-pager-render          ← 表现层原语 (pager:18)
│   │   ├── xai-ratatui-inline / xai-ratatui-textarea / xai-grok-markdown (render:30,64-65)
│   ├── xai-grok-pager-diff (pager:24) ; xai-grok-status-line (pager:120)
│   └── xai-grok-mermaid (pager:41) ──→ third_party/mermaid-to-svg ──→ dagre_rust ──→ graphlib_rust ──→ ordered_hashmap
└── xai-grok-pager-minimal             ← 可选滚动区渲染模式；反向依赖 pager，pager 不可回指它（pager-minimal:16-22）
```

`xai-grok-pager-minimal` 用 `[package.metadata.cargo-shear] ignored = ["xai-ratatui-inline"]` 保留一个只在文档链接里出现的依赖（`crates/codegen/xai-grok-pager-minimal/Cargo.toml:8-14`）——这是「cargo-shear 扫描与 intra-doc link 冲突」的一个现实注脚。

## 4. feature 门控一览

### 4.1 各 crate 的 feature 表

| crate | feature | 作用 / 开的东西 | 证据 |
| --- | --- | --- | --- |
| xai-grok-pager | `default = ["jemalloc", "sandbox-enforce"]` | 默认集合 | `pager:241` |
| | `jemalloc` | **空 feature**：真正的 `#[global_allocator]` 与 `tikv-jemallocator` 在 `xai-grok-pager-bin`，这里只为让统一默认集解得出来 | `pager:248-254` |
| | `sandbox-enforce` | 转发 `xai-grok-sandbox/enforce` | `pager:255` |
| | `local-workspace` | 转发 `xai-grok-shell/local-workspace`；源码中 217 处 `cfg`（最大的一支） | `pager:248`；`.../src/views/welcome/hero_box.rs:278` |
| | `release-dist` | 发布版专用分支（`src/app/mod.rs` 5 处） | `pager:256`；`.../src/app/mod.rs:476,1973` |
| | `test-support` | 暴露测试用 view-model 构造器给兄弟 crate；「生产构建永不启用」 | `pager:257-260` |
| | `default-bazel` | CI/Bazel 专用默认集（jemalloc + sandbox-enforce + test-support + local-workspace） | `pager:242-247` |
| xai-grok-pager-bin | `jemalloc` | `dep:tikv-jemallocator` + `dep:tikv-jemalloc-sys` + `dep:tikv-jemalloc-ctl`（`stats`） | `pager-bin:86-95` |
| | `test-seams` | 测试签名策略密钥的注入缝，明确「永不加入发布 feature 集」且额外要求 `debug_assertions` | `pager-bin:99-103` |
| | `release-dist` | 转发 `xai-grok-pager/release-dist` | `pager-bin:104-106` |
| xai-grok-pager-render | `test-support` | 暴露 mock setter/测试守卫 | `render:117-120` |
| | `notify`（隐式） | 由 `notify = { optional = true }` 产生；**无任何使用点**（见 §8） | `render:97` |
| xai-ratatui-inline | `scrolling-regions` | 转发 `ratatui/scrolling-regions`，注释「required for terminal fork only」；源码在 `terminal.rs` 用 `cfg(not(...))` 提供回退实现 | `inline:32-35`；`.../src/terminal.rs:192,637` |
| | `test-support` | 暴露帧检查辅助 `Terminal::completed_buffer` | `inline:36-37` |
| xai-ratatui-textarea | `debug-logs = ["dep:tracing"]` | 让 `tracing` 成为可选依赖，仅调试日志用 | `textarea:29-31`；`.../src/textarea.rs:2069` |
| xai-grok-markdown | `playground = ["dep:crossterm", "dep:xai-ratatui-textarea"]` | 打开两个 playground 二进制 `md-table-test` / `md-mermaid-test`（`required-features`） | `markdown:45-57` |
| xai-grok-status-line | `test-support` | 暴露 `[ui.status_line]` 逐字段构造的 fixture | `status-line:18-22` |
| xai-grok-mermaid | **无 `[features]`** | Mermaid 恒编译，仅在运行时由 `appearance.render_mermaid` 设置控制 | `pager:38-41`（注释） |
| xai-grok-markdown-core | **无 `[features]`** | 2 个依赖的纯解析 crate | `markdown-core:8-10` |

`default-bazel` 是一个跨 crate 复现的模式，注释解释了动机：CI 只构建**单一 rlib 变体**，所以把 test-only 辅助放进 CI 默认集，而 cargo 的 `default` 保持干净（`render:121-126`、`inline:38-40`、`status-line:19-22`）。

### 4.2 被声明为可选、但源码零引用的依赖（重要）

pager 有 5 个 `optional = true` 依赖，且**没有任何显式 feature 引用它们**，因此它们只能作为「隐式 feature」存在：

```
tokio-stream   = { workspace = true, optional = true }   pager:76
tempfile       = { workspace = true, optional = true }   pager:78
xai-chat-state = { path = "../xai-chat-state", optional = true }        pager:100
xai-grok-sampling-types = { path = "../xai-grok-sampling-types", optional = true }  pager:106
gethostname    = { workspace = true, optional = true }   pager:128
```

对整个 `crates/codegen/xai-grok-pager/` 目录（含 `src/`、`tests/`、`benches/`、`build.rs`）搜索 `tokio-stream` / `tokio_stream`、`xai-chat-state` / `xai_chat_state`、`sampling-types` / `sampling_types`、`gethostname`，唯一命中就是上面 5 行清单本身；pager 也没有 `build.rs`。也就是说这 5 项在默认构建里**不参与编译**，但也**没有任何代码路径会用到它们**——要么是为下游 opt-in 预留，要么是历史残留。`tempfile` 略有不同：`[dev-dependencies]` 里另有一条 `tempfile`（`pager:202`），测试里的 `tempfile::tempdir()` 走的是 dev-dependency。

### 4.3 对依赖面/体积的实际影响

- **默认关掉的东西**：上面 5 项 + `xai-grok-pager-render` 的 `notify`（无人开启：全仓只有 pager 转发 render 的 `test-support`，见 `pager:260`）+ `xai-ratatui-textarea` 的 `tracing` + `xai-grok-markdown` 的 `crossterm`/`xai-ratatui-textarea`。
- **默认无法关掉的重依赖**：Mermaid 栈（`mermaid-to-svg` + `dagre_rust` + `resvg`/`tiny-skia`/`fontdb`）在 pager 侧无 feature 门（`pager:38-41`）；`syntect` 走 `default-onig`（`Cargo.toml:268`）会编译 C 依赖 `onig_sys`；`git2` 用 `vendored-libgit2`（`pager:156-158`）。
- **`workspace = true` 不会收窄 feature**：workspace 把 tokio 声明为 `features = ["full"]`（`Cargo.toml:280`），而 pager 只列了 6 个 feature（`pager:67-75`）——由于 feature 并集语义，pager 的这份清单是**叠加**而非裁剪；想真正瘦身必须改 workspace 声明。
- 未测量：没有构建，所以「这些 feature 省了多少体积」本次**未找到证据**。

## 5. 第三方源码移植（vendored）

### 5.1 `third_party/` 的定位

`third_party/README.md` 明确它只放 **in-tree vendored 的上游源码**、不是一方代码，理由写在文件头：完整审计面、锁定精确源、免疫 crates.io yank（`third_party/README.md:1-14`）。workspace 里只有 4 个 vendored member（`Cargo.toml:106-109`）。

| crate | 版本 | 许可 | 上游 | 证据 |
| --- | --- | --- | --- | --- |
| mermaid-to-svg | path | MIT | warpdotdev/mermaid-to-svg（rev `40cecf2be376e47e15053eadbfb782a531777420`，见其 Cargo.toml 头） | `third_party/README.md:20`、`third_party/mermaid-to-svg/Cargo.toml:3-7` |
| dagre_rust | 0.0.5 | Apache-2.0 | r3alst/dagre-rust（经 Warp re-vendor） | `third_party/README.md:21`、`third_party/NOTICE:21-29` |
| graphlib_rust | 0.0.2 | Apache-2.0 | r3alst/graphlib-rust | `third_party/README.md:22`、`third_party/NOTICE:32-39` |
| ordered_hashmap | 0.0.3 | Apache-2.0 | r3alst/ordered-hashmap | `third_party/README.md:23`、`third_party/NOTICE:42-49` |

Mermaid 布局栈（`third_party/README.md:27-46` 原图）：

```text
xai-grok-mermaid
  └── mermaid-to-svg          (MIT)
        ├── dagre_rust        (Apache-2.0)
        │     ├── graphlib_rust
        │     └── ordered_hashmap
        └── graphlib_rust     (Apache-2.0)
              └── ordered_hashmap
```

值得本项目借鉴的是**vendoring 纪律**：每个 vendored crate 的 `Cargo.toml` 头部就是「本地改动 + 重审计清单」的唯一真相源，升级时按它逐条重放（`third_party/README.md:58-63`）。`mermaid-to-svg/Cargo.toml` 记了 **13 条**本地改动，其中几条与安全直接相关：

- 不 vendor 上游 CLI 二进制与全部快照测试（`mermaid-to-svg/Cargo.toml:20-33`）
- **hermetic 补丁**：`mermaid_port::is_enabled()` 恒返回 `false`，因为上游读环境变量来决定是否走实验性布局端口，而它渲染的是不可信模型输出，读环境即不确定（`mermaid-to-svg/Cargo.toml:34-42`）
- 删掉未被引用的 `petgraph`、`regex`（`:43-44`）；`thiserror` 从 `1.0` 对齐到 workspace 的 `2`（`:45-47`）；对 vendored 源统一 `cargo fmt`（`:48-49`）；另有若干 `sequence_diagram.rs`、`parser.rs`、`text_wrap.rs`、`layout.rs` 的本地改写（`:50-123`，共 7、8、9、10、11、12、13 条）

`dagre_rust` 的 2 条改动里有一条是**把上游的 `static mut UNIQUE_STARTER`（无同步的 `unsafe` 自增）换成 `AtomicUsize::fetch_add`**，理由是并行 `cargo test` 下的数据竞争（`dagre_rust/Cargo.toml:19-33`）。

### 5.2 `xai-grok-tools` 中对 codex / opencode 的移植（概述）

`crates/codegen/xai-grok-tools/THIRD_PARTY_NOTICES.md` 是 crate 级移植声明，`THIRD-PARTY-NOTICES` 里也有对应条目（「PART I (continued) — IN-TREE SOURCE PORTS」，2 条）：

| 上游 | 许可 | 本仓目录 | 覆盖的工具 | 证据 |
| --- | --- | --- | --- | --- |
| openai/codex | Apache-2.0 | `src/implementations/codex/` | `apply_patch`、`grep_files`、`list_dir`、`read_file`（源自 `codex-rs/core/src/tools/handlers/`） | `xai-grok-tools/THIRD_PARTY_NOTICES.md:16-28` |
| sst/opencode | MIT | `src/implementations/opencode/` | `bash`、`edit`、`glob`、`grep`、`read`、`skill`、`todowrite`、`write`（源自 `packages/opencode/src/tool/`） | `xai-grok-tools/THIRD_PARTY_NOTICES.md:33-41` |

声明方式值得抄：文件开头即说明「移植文件已被修改（语言转译、适配本 crate 的 `Tool` trait 与运行时、并做了扩展），本文件构成 Apache-2.0 §4(b) 所要求的醒目变更通知」（`xai-grok-tools/THIRD_PARTY_NOTICES.md:1-13`）。移植后的命名空间隔离机制可在代码里看到：`ToolNamespace::Codex`，且注释说明 Codex 与 Grok Build 的同名工具「namespace-exclusive，消费者二选一，绝不并存」（`crates/codegen/xai-grok-tools/src/implementations/codex/grep_files/tool.rs:59-63,141`）。

### 5.3 打包进二进制的 UI / 语法资产

`THIRD-PARTY-NOTICES` 有专门的「PART I (continued) — BUNDLED UI / SYNTAX THEMES」节，3 条：Tokyo Night（Apache-2.0，UI 调色板 + 打包 `.tmTheme`）、Rosé Pine Moon（MIT）、Oscura Midnight。一方主题 `groknight`、`grokday`、`auto` 不算第三方，但 `groknight` 复用 Tokyo Night 的强调色十六进制值。对应资产确实存在：`crates/codegen/xai-grok-pager-render/assets/{grok-day,grok-night,tokyo-night}.tmTheme`、`crates/codegen/xai-grok-markdown/assets/{Swift.sublime-syntax,tokyo-night.tmTheme`，且 `xai-grok-markdown` 的 `build.rs` 会在构建期把语法集 dump 成 `syntaxes.bin`，注释写明原因是「运行时 `SyntaxSet::build` 会卡住首次 Read paint（GB-5513 PTY fold）」（`crates/codegen/xai-grok-markdown/build.rs:10-23`）。

## 6. 依赖规模量级（Cargo.lock）

| 指标 | 值 | 说明 |
| --- | --- | --- |
| lock 格式 | `version = 4` | `Cargo.lock:3` |
| package 总数 | **1347** | `[[package]]` 计数 |
| registry | 1242 | `source = "registry+…"` 计数 |
| in-tree path | **101**（= workspace member 数，完全吻合） | 无 `source` 字段的条目 |
| git | 4 | `async-openai` + `async-openai-macros`（`Cargo.toml:3-4` 的 `[patch.crates-io]`）、`nucleo` + `nucleo-matcher`（`Cargo.toml:209`，rev `5b74652`） |
| 同名多版本 | **110** 个 crate 名存在 ≥2 个版本 | 前几名：`windows-sys` 6、`nix` 5、`phf` 系 4、`itertools` 4、`hashbrown` 4、`strum` 3、`rand` 3、`syn` 3 |
| 许可证分布（NOTICES 口径） | MIT 996、Apache-2.0 97、BSD-3-Clause 19、Unicode-3.0 18、ISC 15、MPL-2.0 9 | `THIRD-PARTY-NOTICES`，共 1172 个 package 条目 / 18925 行 |

各层的直接依赖数：

| crate | 直接依赖 | 其中内部 / 第三方 |
| --- | --- | --- |
| `xai-grok-pager` | 87 | 39 / 48（另有 unix 3、macOS 1、windows 1、dev 16） |
| `xai-grok-pager-render` | 47 | 12 / 35 |
| `xai-ratatui-textarea` | 9（1 个可选） | 0 / 9 |
| `xai-ratatui-inline` | 4（+6 dev） | 0 / 4 |
| `xai-grok-markdown` | 13（+2 可选，+2 build） | 1 / 12 |
| `xai-grok-status-line` | 2 | 0 / 2 |
| `xai-grok-markdown-core` | 2 | 0 / 2 |

**重量级依赖**（会显著贡献编译时间/体积，均已在 lock 中确证）：`tikv-jemalloc-sys 0.7.1+5.3.1`、`aws-lc-sys 0.39.1`（+ `aws-lc-rs 1.16.2`）、`zstd-sys 2.0.16+zstd.1.5.7`、`libgit2-sys 0.18.2+1.9.1`（GitHub 上是 `vendored-libgit2` 直启）、`onig_sys 69.9.3`、`openssl-sys 0.9.117`、`tree-sitter 0.25.10`、`resvg 0.47.0` / `usvg 0.47.0` / `tiny-skia 0.12.0` / `fontdb 0.23.0`、`tonic 0.14.3` / `prost 0.14.3`、`gcloud-storage 1.3.0`、`pdf_oxide 0.3.74`。

TUI 栈内部的「代差」现象（同一构建里并存两代 API）：`ratatui 0.29.0` + `ratatui-core 0.1.0`、`compact_str 0.8.1`/`0.9.0`、`unicode-truncate 1.1.0`/`2.0.1`、`itertools 0.13.0`/`0.14.0`、`lru 0.12.5`/`0.16.3`、`strum 0.26.3`/`0.27.2`；另外 `strip-ansi-escapes 0.2.1` 仍依赖 **老的 `vte 0.14.1`**，而 workspace 自己用 `vte 0.15.0`。

单个依赖的传递面也很可观：`termwiz` 34 个依赖、`alacritty_terminal` 17、`ratatui` 13、`syntect` 13、`portable-pty` 13、`ratatui-core` 12、`crossterm` 9；反过来 `similar` 是 **0 依赖**（纯算法库），`textwrap` 3 个（`smawk`、`unicode-linebreak`、`unicode-width`）。

## 7. 与本项目（jev-harness）的对照

### 7.1 「事实标准」——Rust TUI 的通用底座，可放心直接采用

下方这些没有任何 Grok 特有的封装痕迹，就是生态默认答案，且它们之间已被证明能协同（`crates/codegen/xai-ratatui-inline/Cargo.toml:9-15` 的注释直接说 `anstyle-parse` 与「已通过 anstream/clap 挂在已发布依赖树里」）：

- **框架与后端**：`ratatui` 0.29 + `crossterm` 0.28（`pager:27,29`；`inline:13-14`）。注意 0.29 与 `ratatui-core` 0.1 并存是当前生态过渡期的真实状态，不是本项目该模仿的设计。
- **宽度与折行**：`unicode-width` + `unicode-segmentation` + `textwrap`（`pager:30,32,33`）——终端 UI 的必备三件套。
- **颜色与能力探测**：`anstyle` / `anstyle-parse` / `supports-color`（`render:32-33`、`inline:12`）。
- **纯文本处理**：`strip-ansi-escapes`（`pager:34`）、`linkify`（`render:26`）、`vte`（`render:44`）、`pulldown-cmark`（`markdown:18`）。
- **diff 与有序容器**：`similar`（`pager-diff:11`）、`indexmap`（`pager:46`）。
- **模糊搜索**：`nucleo`（Helix 出品，`pager:88`）——若需要「快速筛选列表」，这是生态里现成的答案。
- **运行时与 CLI**：`tokio`（`Cargo.toml:280`）、`clap` + `clap_complete`（`pager:64-65`）。
- **PTY**：`portable-pty`（`pager:58`）。
- **ANSI → 文本**：`ansi-to-tui`（`pager:55`）、`tui-scrollbar`（`render:27`）。

### 7.2 Grok 特有——不要照抄，但设计取舍值得读

| Grok 特有 | 是什么 | 为什么不该照抄 / 该学什么 | 证据 |
| --- | --- | --- | --- |
| `xai-ratatui-inline` | 自研 inline / scrollback 终端层，带 `insert_before`、私有帧检查钩子、`scrolling-regions` fork | 这是「原生滚动区（不占 alt screen）」这条产品路线的核心自研资产；jev-harness 若走 alt-screen 全屏 TUI，用 ratatui 自带 `Terminal` 即可 | `inline:1-40`、`pager-minimal:8-14,31-34` |
| `xai-ratatui-textarea` | 自研输入框 + 鼠标/粘贴/滚动条，配 `debug-logs` 可选日志 | 通用需求，先用上游 `tui-textarea` 之类评估；其 feature 门（`dep:tracing`）写法可借鉴 | `textarea:8-31` |
| `xai-grok-mermaid` + vendored dagre 栈 | 把模型输出的 Mermaid 图渲染成 PNG，进程外执行 | 只有「要在 TUI 里显示图」时才需要，代价是 4 个 vendored crate + resvg/tiny-skia/fontdb | `mermaid:8-25`、`third_party/README.md:27-46` |
| `obfstr` / `cryptify` | 编译期字符串混淆，用于 token/环境变量名等 | 属于分发硬化，闭源/分发型 CLI 才划算；`pager-bin:59-60` | `pager:82-85`、`pager-bin:58-60` |
| `alacritty_terminal` + `ptyctl` | 用真终端仿真内核做 TUI e2e 测试与基准（spawn-only，不在发布二进制里） | 这是**测试方法论**上最值得学的一点：用独立 crate + PTY 把「需要真终端」的测试从单测里隔离出去 | `pager-pty-harness:1-16,28`、`ptyctl:10` |
| `xai-grok-status-line` | 把「用户写的配置」与「agent 发来的载荷」抽成 2 依赖的叶子 crate | 契约与实现分离，让客户端从上游读、agent 从下游填；对 LLM + Jev 双模型契约同样适用 | `status-line:6,8-10`、`pager:117-120` |
| `xai-grok-pager-bin` | 组合根 binary：拥有 `main.rs`、全局分配器、字符串混淆、可选渲染模式的 fn-pointer 注入 | 用 cargo 循环限制反向逼出「依赖单向」的架构，值得抄 | `pager-bin:9-13,20-22,70-77` |
| `xai-grok-workspace` / `-client` / `-daemon` / `-types` | 把工作区能力拆到 daemon + 类型 crate | jev-harness 早期不需要 | `Cargo.toml:77-80,121` |

### 7.3 一个可直接照搬的 feature 组织法

Grok 用 `default-bazel` 把「CI 需要的 test-only 能力」与「cargo 生产 `default`」分开，并在注释里写明原因（单一 rlib 变体 + 避免生产构建带上 mock）（`render:121-126`、`inline:38-40`、`status-line:19-22`、`pager:257-260`）。jev-harness 若不使用 Bazel，仍可保留同样的**双默认集**约定，只是把它命名为 CI 专用集即可。

## 8. 未找到证据 / 存疑点

1. **`third_party/NOTICE` 与目录不一致**：`NOTICE` 列了 `nfsserve` 0.11.0（BSD-3-Clause）与 `fuser` 0.18.0（MIT）（`third_party/NOTICE:52-68`），`README.md` 的表里也有（`third_party/README.md:24-25`），但本次在快照内**未找到这两个目录**，它们也不在 workspace members（`Cargo.toml:106-109` 只有 4 个第三方面员）。合稿时不要把这两个 crate 算进 TUI 依赖面。
2. **`clippy.toml` 自我描述与位置矛盾**：文件头称自己是 `crates/codegen/**` 的配置并要求复制「repo-root //clippy.toml」的设置（`clippy.toml:1-4`），但全仓只有这一个 `clippy.toml`（`find -maxdepth 3`），其引用的根配置在本快照中不存在。
3. **5 个 pager 可选依赖无使用点**（§4.2）：`tokio-stream`、`tempfile`、`xai-chat-state`、`xai-grok-sampling-types`、`gethostname` 在 pager 全目录零引用，只以隐式 feature 形式存在；**无法判断**是有意为下游预留还是残留。
4. **`xai-grok-pager-render` 的 `notify` 无可达使用点**：`appearance/watcher.rs` 只有 `start_static()`（注释「prod mode, no hot-reload」），全 crate 无 `cfg(feature = …)`（`render:97`；`.../src/appearance/watcher.rs:15,33-49`），且没有任何 crate 开启 `xai-grok-pager-render/notify`。
5. **`rustls` 的实际归属未确认**：pager 的注释说 `cryptify / rustls / semver / tikv-jemallocator` 已移到 `xai-grok-pager-bin`（`pager:83-84`），但 `rustls` 并未出现在 `pager-bin` 的 `[dependencies]` 段（`pager-bin:18-63`）——它应该是从 `xai-grok-http`/`xai-grok-extra-ca` 间接进入的，本次**未追踪**。
6. **体积与编译时长未测量**：未运行任何 `cargo` 命令，故 feature 的瘦身收益、`release-dist` 的 LTO/`codegen-units=1` 代价（注释称本地构建慢约 2.2 倍，`Cargo.toml:391-399`）均**未验证**。
7. 未展开：`xai-grok-shell`、`xai-grok-workspace`、`xai-grok-ocr-sampler` 等非 TUI 侧的一百多个成员 crate 的依赖细节（`xai-grok-shell` 的 feature 结构只在 §2 中顺带引用）。

## 关键结论（供合稿引用）

1. **工具链基线是 Rust `1.94.0` + `edition = "2024"` + `resolver = "2"`**，`rustfmt` 几乎全默认（只有 `use_field_init_shorthand`），风格问题交给工具而非规范文档。证据：`rust-toolchain.toml:11-12`、`Cargo.toml:113,7`、`rustfmt.toml:1`。
2. **TUI 依赖面是「ratatui 0.29 + crossterm 0.28」的经典组合**，配套 `textwrap` / `unicode-width` / `unicode-segmentation` / `anstyle` / `supports-color` / `vte` / `strip-ansi-escapes`，全部无 feature 门、属生态事实标准。证据：`crates/codegen/xai-grok-pager/Cargo.toml:27-34,55`、`.../xai-grok-pager-render/Cargo.toml:13-33,44`。
3. **同一构建里并存两代 ratatui API**（`ratatui 0.29.0` 与 `ratatui-core 0.1.0`），还并存 `compact_str`/`itertools`/`lru`/`strum`/`unicode-truncate` 的两代版本，说明这是生态过渡期的真实状态，不是刻意设计。证据：`Cargo.lock:8602,8623`。
4. **架构上最有价值的一条：表现层原语被抽成独立 crate**（`xai-grok-pager-render`，47 个直接依赖、12 个内部），其中再嵌 `xai-ratatui-inline`（4 依赖）、`xai-ratatui-textarea`、`xai-grok-markdown`；`xai-grok-pager` 只是它们的消费者。证据：`crates/codegen/xai-grok-pager/Cargo.toml:17-18,91-92`、`.../xai-grok-pager-render/Cargo.toml:64-65`。
5. **组合根被单独拆成 `xai-grok-pager-bin`**，用来拥有 `main.rs`、全局分配器（`tikv-jemallocator` + `-sys` + `-ctl`）、`obfstr`/`cryptify` 硬化，并靠「`minimal` 依赖 pager、pager 不得回指」的 cargo 循环约束保证依赖单向。证据：`crates/codegen/xai-grok-pager-bin/Cargo.toml:9-13,20-22,58-60,70-77`、`.../xai-grok-pager-minimal/Cargo.toml:16-22`。
6. **feature 门控主要服务三件事**：测试专用辅助（`test-support` + `default-bazel` 双默认集模式）、平台/环境分支（`local-workspace` 在 pager 里有 217 处 `cfg`、`release-dist` 5 处）、以及把重依赖推到边界（`scrolling-regions` 只为终端 fork、`playground` 只为两个实验二进制、`debug-logs` 只为 `dep:tracing`）。证据：`crates/codegen/xai-grok-pager/Cargo.toml:240-260`、`.../xai-ratatui-inline/Cargo.toml:32-40`、`.../xai-grok-markdown/Cargo.toml:45-57`。
7. **`jemalloc` 在 pager 上是空 feature**（真正的分配器在 bin），这类「为让统一默认集解得出来而留的空 feature」是一种可复用的多 crate 构建技巧。证据：`crates/codegen/xai-grok-pager/Cargo.toml:248-254`。
8. **5 个 pager 可选依赖在源码里零引用**（`tokio-stream`、`tempfile`、`xai-chat-state`、`xai-grok-sampling-types`、`gethostname`），`xai-grok-pager-render` 的 `notify` 同样无可达使用点——合稿引用「可选依赖」时不要把它们当成真实能力开关。证据：`crates/codegen/xai-grok-pager/Cargo.toml:76,78,100,106,128`、`.../xai-grok-pager-render/Cargo.toml:97`、`.../src/appearance/watcher.rs:15,33-49`。
9. **vendored 源码只有 Mermaid 布局栈 4 个 crate**（`mermaid-to-svg` MIT + `dagre_rust`/`graphlib_rust`/`ordered_hashmap` Apache-2.0），理由是渲染不可信模型输出需要审计面与免 yank；每个 vendored crate 的 `Cargo.toml` 头就是升级清单（`mermaid-to-svg` 记了 13 条本地改动，含「恒关实验性端口」的 hermetic 补丁）。证据：`third_party/README.md:16-46,58-63`、`third_party/mermaid-to-svg/Cargo.toml:20-42`。
10. **`xai-grok-tools` 移植了 codex 与 opencode 的工具实现**（codex 4 个：apply_patch/grep_files/list_dir/read_file，opencode 8 个：bash/edit/glob/grep/read/skill/todowrite/write），并用 `ToolNamespace::Codex` 做命名空间互斥 + crate 级 `THIRD_PARTY_NOTICES.md` 履行 Apache-2.0 §4(b) 变更通知。证据：`crates/codegen/xai-grok-tools/THIRD_PARTY_NOTICES.md:1-13,16-28,33-41`、`.../src/implementations/codex/grep_files/tool.rs:59-63,141`。
11. **依赖规模量级：Cargo.lock 1347 个 package（registry 1242 + in-tree 101 + git 4），101 个 in-tree 与 101 个 workspace member 完全吻合**；110 个 crate 名存在多版本（`windows-sys` 6、`nix` 5 居多），许可证以 MIT 996 / Apache-2.0 97 为主。证据：`Cargo.lock:3`、`Cargo.toml:8-110`、`THIRD-PARTY-NOTICES`（1172 条 package 条目）。
12. **重量级原生依赖集中在「安全 + 高亮 + 图形」三处**：`tikv-jemalloc-sys`、`aws-lc-sys`、`zstd-sys`、`libgit2-sys`（`vendored-libgit2`）、`onig_sys`（`syntect` 的 `default-onig`）、`resvg`/`usvg`/`tiny-skia`/`fontdb`；jev-harness 若要轻量 TUI，最后这组（Mermaid 栈）是最应先砍的。证据：`Cargo.toml:268,241,279,170`、`crates/codegen/xai-grok-mermaid/Cargo.toml:12-25`、`Cargo.lock`。
