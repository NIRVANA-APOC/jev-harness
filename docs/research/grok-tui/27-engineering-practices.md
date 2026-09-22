# 专题 27：工程化——测试、快照、基准与代码组织

> 专题：工程化——测试、快照、基准与代码组织｜调研日期：2026-09-22｜证据范围：`crates/codegen/xai-grok-pager/{tests,src,docs,benches}`、`crates/codegen/xai-grok-pager-pty-harness/`、`crates/common/xai-test-utils/`、`crates/codegen/xai-grok-test-support/`、`crates/codegen/{xai-grok-pager-render,xai-grok-pager-minimal,xai-grok-pager-bin,ptyctl}/`、根 `Cargo.toml`/`clippy.toml`/`rust-toolchain.toml`/`CONTRIBUTING.md`

## 0. 总览：约定即测试

Grok Build 的工程化特征不是「测试数量多」，而是**每一条跨模块约定都配了一个会失败的测试**：文档与注册表对账、架构禁区源码扫描、终端宿主环境自动清洗、帧时数值门禁。这些测试大多不是断言业务逻辑，而是断言「另一处文本/另一份清单仍然与这份清单一致」。

全文路径均相对 `reference/grok-build/`。

---

## 1. 量化画像

| 指标 | 数值 | 统计方式 |
| --- | --- | --- |
| `xai-grok-pager` 下 `.rs` 文件 | 652 | `find … -name '*.rs' \| wc -l` |
| `xai-grok-pager` 测试属性 | 10,509 | `rg -c '#\[test\]\|#\[tokio::test\]\|#\[test_with'` 求和 |
| 全仓库测试属性 | 34,077 | 同上，范围 `crates/` |
| 全仓库 `#[ignore` | 390 | 同上 |
| `serial_test` 出现次数 | ~1,002 | `rg -c 'serial_test\|#\[serial\]'` 求和 |
| 含 `#[cfg(test)]` 的 pager 源文件 | 340 | `rg -c '^#\[cfg\(test\)\]'` 文件数 |
| `tests.rs` / `*_tests.rs` | 11 + 74 = 85 | `find` 两个模式分别计数 |
| 有 `tests/` 集成测试目录的 crate | 32 | `find crates -type d -name tests` |

---

## 2. 测试金字塔的实际分层

```
                  ┌──────────────────────────────────────────────┐
  L5 脚本化场景    │ 48 个 YAML（tests/scenarios/*.yaml）          │  #[ignore]
     + 视觉产物    │ ScriptedScenarioRunner + screenshot/styled    │  Bazel opt-in
                  ├──────────────────────────────────────────────┤
  L4 PTY 端到端    │ 12 个独立 [[test]] 家族（pty_e2e_*）           │  #[ignore]
     + 性能基线    │ PtyHarness L1/L2a/L2b + pty-bench JSON 基线    │  Bazel opt-in
                  ├──────────────────────────────────────────────┤
  L3 子进程集成    │ 进程边界用例（mermaid __mermaid-render）        │  #[ignore]
                  │ 以及 pager/tests/ 的 6 个普通集成测试           │  默认跑
                  ├──────────────────────────────────────────────┤
  L2 渲染快照/缓冲 │ Buffer::empty 断言 + 11 个 insta .snap         │  默认跑
                  ├──────────────────────────────────────────────┤
  L1 同目录单元测试│ 85 个 tests.rs / *_tests.rs + 内联 mod tests   │  默认跑
                  └──────────────────────────────────────────────┘
```

关键隔离手段是 `#[ignore]`：进程级用例一律默认跳过，由 Bazel 显式 opt-in。`tests/pty_e2e_smoke.rs:3` 写明「All cases are ignored for ordinary Cargo runs; Bazel opts in and caps this process-heavy family at four concurrent libtest workers」。

---

## 3. 单元测试：与源码同目录

### 3.1 两种并置风格

**风格 A —— 拆分文件 + 显式声明。** `src/views/settings_modal/mod.rs:22-27`：

```rust
mod input;
mod render;
mod state;

#[cfg(test)]
mod tests;
```

同目录的 `tests.rs` 用 `super::` 直接访问三个兄弟模块（`src/views/settings_modal/tests.rs:9-11`）：

```rust
use super::input::*;
use super::render::*;
use super::state::*;
```

**风格 B —— `*_tests.rs` 文件名 + `#[path]`。** `src/scrollback/render.rs:977-979`：

```rust
#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;
```

`#[path = "[a-z_]*tests.rs"]` 这种声明在 pager 中出现 69 次，对应的 `*_tests.rs` 文件有 74 个（差额来自个别文件由父模块以其他形式引入）。测试文件命名有两类：通用 `tests.rs`，以及按被测主题命名的 `*_tests.rs`（如 `views/dashboard/{render_tests,chrome_tests,layout_tests,state_tests,preview_tests,usage_modal_tests}.rs`）——后者在同一目录有多个关注点时使用。

另有 `src/app/status_blocks.rs:274` 这类直接内联 `mod tests { ... }` 的形式，用于只测本文件少量逻辑。

### 3.2 命名与断言风格

测试函数名是完整英文句子，说明**期望的行为而非被测函数名**：

- `every_registered_setting_is_exercised`（`tests/settings_e2e.rs:79`）
- `every_field_on_the_wire_is_named_in_the_guide`（`src/app/status_line_tests.rs:356`）
- `go_deeper_titles_resolve_to_real_guides`（`src/tutorial_docs.rs:108`）
- `minimal_never_uses_ris_rerender_or_emit_to_scrollback`（`xai-grok-pager-minimal/src/guard.rs:9`）

断言依赖 `pretty_assertions`（根 `Cargo.toml:225` 定为 `1`），在 `src/scrollback/render_tests.rs:8` 以 `use pretty_assertions::assert_eq;` 覆盖内置宏。

---

## 4. 快照 / golden 测试：范围刻意收窄

用的是 **insta**（根 `Cargo.toml:192` 为 `insta = "1"`；pager 的 dev-dep 在 `crates/codegen/xai-grok-pager/Cargo.toml:200`）。全仓库 **只有 11 个 `.snap` 文件、0 个 `.snap.new`**，集中两处：

| 快照目录 | 数量 | 来源 |
| --- | --- | --- |
| `src/scrollback/blocks/tool/snapshots/` | 9 | `src/scrollback/blocks/tool/edit.rs:2412,2448,2479,2517,2578,2599,2635,2669,2707` |
| `src/app/snapshots/` | 2 | `src/app/status_blocks.rs:323,351` |

**快照是纯文本，不是屏幕像素。** 断言对象是被测函数返回的 `String`：

```rust
insta::assert_snapshot!("diff_basic", diff_outputs_to_string(&outputs));   // edit.rs:2412
insta::assert_snapshot!("session_usage_block_full", text);                 // status_blocks.rs:323
```

文件格式（`src/scrollback/blocks/tool/snapshots/xai_grok_pager__scrollback__blocks__tool__edit__tests__diff_basic.snap`）：

```
---
source: crates/codegen/xai-grok-pager/src/scrollback/blocks/tool/edit.rs
expression: diff_outputs_to_string(&outputs)
---
  10  let x = 1;
  11  let y = 2;
  11  let y = 3;
  12  let z = 4;
```

文件名是 insta 默认规则 `<crate>__<模块路径>__<测试模块>__<快照名>.snap`，即模块路径本身编码进文件名——重命名模块会直接产生孤儿快照。快照内容覆盖的正是「行号栏宽度、双栏对齐、换行重排、hunk 间省略标记」这些用断言写起来很啰嗦的等宽文本输出（例如 `diff_three_digit_lines.snap` 验证三位行号的列宽切换，`diff_merged_hunks_gap_markers.snap` 验证 `… 7 unchanged lines` 这类间隙标记）。

**未找到证据**：仓库中没有 ratatui 全屏快照（无 `TestBackend` 输出落盘为 golden 文件的机制），也没有 `insta` 的 `glob!` 或自定义 `Settings` 配置。

---

## 5. 渲染层测试：缓冲区就地断言

不落盘 golden 的情况下，视图测试的惯用法是构造 `ratatui::buffer::Buffer::empty(Rect)`，把 widget 画进去，再断言单元格文本。按命中次数排序的前几名：

- `src/views/dashboard/render_tests.rs` — 117
- `src/views/settings_modal/tests.rs` — 94
- `src/views/dock/mod.rs` — 38
- `src/views/dashboard/chrome_tests.rs` — 27
- `src/scrollback/render_tests.rs` — 26
- `xai-ratatui-textarea/src/textarea_tests.rs` — 24

`xai-grok-pager-render` 自身有 7 个测试文件、1,216 个 `#[test]`，覆盖 `render/`、`input/mouse/`、`terminal/`（含 `probe_tests.rs`、`pop_fence_tests.rs`）与 `location_path_tests.rs`。

配合的辅助函数风格是在测试文件顶部手写小工厂，例如 `src/scrollback/render_tests.rs:22-30` 的 `make_entries(count)` / `make_markdown_entry(text)`，以及 `:11-20` 的一对 `at` / `at_mut` 取下标辅助——后者是因为生产 crate 用 `#![deny(clippy::indexing_slicing)]` 禁止索引切片。

---

## 6. PTY 端到端：用真正的终端模拟器看屏幕

### 6.1 分层设计

`xai-grok-pager-pty-harness/src/lib.rs:10-20` 自述为四层，同一套 API 服务三类消费者（回归场景、基准、本地复现）：

```
L1  pty        — PTY 管理（spawn / 注入按键 / resize / drain）      pty.rs, pty_spawn.rs
L2a screen     — 通过 alacritty_terminal 的虚拟终端状态「用户看到什么」 screen.rs
L2b timing     — 通过 CSI ? 2026 h/l 标记测每帧耗时                  timing.rs
L3  content    — mock 推理服务，把真实内容喂进 pager                 content.rs
    scenarios  — 命名化、参数化的工作负载，返回 BenchResults         scenarios/*.rs
    results    — 汇总统计与基线比较                                  results.rs
    scroll_matrix — GROK_SCROLL_LOG JSONL 不变量套件                 scroll_matrix/
```

### 6.2 屏幕断言怎么做的

`ScreenTracker`（`src/screen.rs:9-14`）持有一个 ptyctl 的 `Terminal`（底层是 `alacritty_terminal`），把原始 PTY 字节喂进去，提供：

- `contents()` — 可见屏纯文本（无转义码），`screen.rs:46-48`
- `contains(text)` — 子串判定，`screen.rs:50-52`
- `cursor_position()` — 0 基坐标（ptyctl 是 1 基，此处转换），`screen.rs:55-62`
- `scrollback_text()` / `full_text()` — 回滚缓冲与「回滚 + 可见屏」拼接；注释明确「对已提交输出的断言应使用 `full_text()`」，因为 minimal 模式下内容可能落在任一区，`screen.rs:88-110`
- `styled()` / `html()` — 带样式与 HTML 输出，用于视觉产物，`screen.rs:69-75`

它还暴露一个关键细节：`drain_responses()`（`screen.rs:32-38`）把模拟器产生的终端应答（光标位置报告、设备属性查询）回写给子进程，注释写着「**Must be written back or terminal probes hang.** A missed startup cursor-position query downgrades `--minimal` to full-screen inline」——这是虚拟终端做 TUI 测试时最容易踩的坑。

断言原语全部是轮询式的 `wait_*`（`src/lib.rs`）：`wait_for_text`(:447)、`wait_for_text_absent`(:454)、`wait_for_turn_idle`(:465)、`wait_for_full_text`(:542)、`wait_for_full_text_absent`(:552)、`wait_for_kitty_graphics`(:569)、`quit`(:615)、`wait_for_exit_and_drain`(:627)。

### 6.3 一个完整的用例

`crates/codegen/xai-grok-pager-pty-harness/tests/pty_e2e/welcome_screen.rs:7-22`：

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn welcome_screen() {
    let content = ContentController::start().await.expect("start content");
    let binary = pager_binary().expect("resolve pager binary");
    let mut harness =
        PtyHarness::spawn_with_content(&binary, DEFAULT_ROWS, DEFAULT_COLS, &content, &[])
            .expect("spawn pager");
    harness
        .wait_for_text(WELCOME_SCREEN_SENTINEL, WELCOME_TIMEOUT)
        .expect("welcome text");
    harness.quit().expect("clean quit");
}
```

共享常量在 `tests/pty_e2e/common.rs:16-40`：`DEFAULT_ROWS = 50`、`DEFAULT_COLS = 120`、`WELCOME_TIMEOUT = 20s`、`WELCOME_SCREEN_SENTINEL = "Quit"`、`MOCK_RESPONSE_SENTINEL = "MOCKRESPONSE"`。注释解释为何 50×120：「大到 welcome 屏不折行，小到 `screen_contents()` 扫描够便宜」。

测试模块通过 `#[path]` 从 `common.rs` 组装进每个测试根（`tests/pty_e2e_smoke.rs:6-81` 列了 40+ 个 `#[path = "pty_e2e/….rs"] mod …;`），共享的 `common.rs` 有 1,667 行。

### 6.4 环境清洗：可复现性的关键

`src/pty_spawn.rs:19-70` 定义三张必须从子进程环境中摘掉的变量表：

- `CLIPBOARD_SINK_ENV_VARS`（`GROK_OSC52_SINK`、`LC_GROK_OSC52_SINK`）
- `APPEARANCE_ENV_VARS`（`GROK_APPEARANCE`、`GROK_THEME`、`COLORFGBG`…）——注释：「would make `theme=auto` non-deterministic in PTY tests（layout depends on the resolved palette）」
- `HOST_TERMINAL_ENV_VARS`——终端品牌链（Cursor/VSCode/WezTerm/iTerm/Kitty…）、复用器标记（`TMUX`、`ZELLIJ`、`STY`、`BYOBU_*`、`CMUX_*`）、嵌入编辑器标记（`NVIM`、`INSIDE_EMACS`）。注释：「**A leak reclassifies the child**（tmux, Cursor before TERM_PROGRAM, nvim OSC 52）」

环境变量修改走类型化的 `EnvOp`（Set/Remove），而不是裸字符串覆盖——`lib.rs:122-137` 的 `new_in_sandbox` 只允许 Set，`:139+` 的 `new_in_sandbox_ops` 才允许 Remove。

### 6.5 依赖边界：harness 刻意不被 pager 依赖

`crates/codegen/xai-grok-pager-pty-harness/Cargo.toml:10-13` 的注释是这条边界的理由：

> Not imported into the production pager binary. Spawn-only pager tests live here so `cargo test -p xai-grok-pager` / `cargo test -p xai-grok-pager-bin` never compile this crate, and the spawn tests never link the pager library.

每个测试家族单独声明 `[[test]]`（`Cargo.toml:69-117`，共 12 个），注释（`:73-74`）：「Each root is its own Cargo/Bazel process so CI can schedule them independently」。该 crate 有 42,234 行、48 个 YAML 场景、4 个 `[[bin]]`（`pty-scenario`、`pty_orphan_holder`、`scroll-matrix`、`startup-tui-probe`）与 2 个 `[[bench]]`。

---

## 7. 子进程集成测试：进程边界留在被它测试的 crate 里

`crates/codegen/xai-grok-pager/tests/mermaid_render_subprocess.rs` 是「测试要 spawn 真实二进制，但断言的是库代码」的范式。它用 pager 生产代码里的同一函数 `render_via_subprocess`，把 **真实构建出的 pager 二进制**当作隐藏子命令 `__mermaid-render` 启动（子命令常量在 `src/app/mermaid_worker.rs:56`）。

定位二进制（`:23-35`）同时认 `PAGER_BINARY` 与 Cargo 的 `CARGO_BIN_EXE_xai-grok-pager`，并 `std::path::absolute` 化（Bazel 给的是 runfiles 相对路径）。四个用例覆盖真实进程行为：

| 用例 | 断言 |
| --- | --- |
| `child_renders_login_flow_to_png` (:50) | 返回 `Ok`，输出文件存在，且 `image::load_from_memory` 能解码、宽高 > 0 |
| `oversized_source_is_contained` (:77) | 超 64 KiB 上限的源：父进程返回 `Err`，**且不留 PNG** |
| `invalid_diagram_is_contained` (:104) | 非法图同样被「容纳」，无 PNG |
| `tight_timeout_kills_child_and_returns_err` (:130) | 1 ms 预算下父进程必须按时返回（< 10 s）并杀掉子进程，无 PNG |

文件头（`:12-15`）解释为什么这个用例留在 pager crate 而非 harness：「Unlike the spawn-only suites in `xai-grok-pager-pty-harness`, this test stays here because it exercises pager library code.」全部 `#[ignore]`，通过 `PAGER_BINARY=target/debug/xai-grok-pager cargo test … -- --ignored` 启用。

---

## 8. 声明式场景与「飞行记录仪」

### 8.1 YAML 场景

`crates/codegen/xai-grok-pager-pty-harness/tests/scenarios/` 有 **48 个 YAML**，由 `ScriptedScenarioRunner` 回放（`src/scripted.rs`，2,300 行）。`tests/scenarios/welcome.yaml` 全文即一个可读规格：

```yaml
name: welcome-screen
terminal: { rows: 32, cols: 100 }
mock: { response: "SCRIPTED_WELCOME_RESPONSE from the mock server." }
steps:
  - { action: wait_for_text, text: Quit, timeout_ms: 20000 }
  - { action: assert_contains, text: Quit }
  - { action: assert_not_contains, text: panicked }
  - { action: screenshot, name: welcome, note: Welcome screen after startup. }
  - { action: resize, rows: 28, cols: 90 }
  - { action: assert_running }
  - { action: screenshot, name: resized-welcome }
```

驱动它的 Rust 壳极薄（`tests/scripted_scenarios.rs:19-51`），且强制三条元约定：`report.status == Passed`、`report.bugs.is_empty()`、报告中**必须存在至少一个带产物的 `screenshot` 步骤**（`:43-50`）。尺寸类断言支持精确值与区间两种模式（`src/scripted.rs:373-379` 的 `DimensionAssertion::Exact` / `Range { min, max }`），产物落 `text` / `html` / `svg` / `styled_json` 四种格式（`scripted.rs:1058-1061`）。

### 8.2 滚动矩阵：生产者侧遥测 + 消费者侧不变量

`src/scroll_matrix/mod.rs:1-16` 描述了一套更硬核的机制：让 pager 打开 `GROK_SCROLL_LOG` 写 JSONL「飞行记录仪」，测试端按手势重放鼠标 SGR 报告，再用不变量判定日志。

关键设计选择写在注释里：

- **不依赖 pager 类型**：「This module deliberately re-declares the wire schema instead of importing pager types. `log::ScrollLogLine` keeps every always-emitted producer field **required**, so a pager-side rename or removal fails deserialization here.」——用反序列化失败当 schema 漂移警报。生产者侧有一对偶测试：`xai-grok-pager/src/input/mouse/tests.rs`（`:16` 提到）。
- **抗负载**（`invariants.rs:3-5`）：「every timing predicate reads the RECORDER's clock (`ts_ms`, `ms_since_prev_flush`, `avg_interval_ms`), never the test process's… So no invariant here can false-fail on a loaded machine.」

不变量用 `I-` 编号命名并带文档注释：`I-ORD`（时间戳非递减）、`I-CAP`（`|flushed| ≤ cap`）、`I-DROP-EQ`（finalize 的 `dropped == backlog_after`）、`I-CADENCE`、`I-CONS-W`（强制滚轮总量精确）等（`invariants.rs:23-40`）。

---

## 9. 基准测试

### 9.1 两套并行的性能体系

**A. criterion 微基准（criterion 0.6，根 `Cargo.toml:148`）。** pager 声明四个 target，全部 `harness = false`（`crates/codegen/xai-grok-pager/Cargo.toml:211-225`）：`render`、`search`、`edit_highlight`、`resize`。`xai-grok-markdown` 与 `xai-ratatui-inline` 各有一个 `bench`。这些 bench 的文件头本身就是设计文档，例如 `benches/resize.rs:1-6` 列出它守护的三条已修复回归（每宽度重推导源文本、每 entry 克隆 `AppearanceConfig`、每次 resize 都跑 `warm_measure_pages_above`），`benches/search.rs:1-7` 说明 `scan` 为什么从输入线程搬到后台守护进程。

**B. PTY 帧时基准 + 数值门禁。** `pty_bench`（`pty-harness/Cargo.toml:143-146`，`harness = false`）spawn 真实 release pager，产出 JSON：

```rust
pub struct BenchResults {                // src/results.rs:13-24
    pub scenario: String, pub total_frames: u64, pub avg_fps: f64,
    pub p50_ms: f64, pub p99_ms: f64, pub max_ms: f64,
    pub jank_count: u64, pub jank_rate: f64, pub chars_per_frame_avg: f64,
}
pub const DEFAULT_REGRESSION_THRESHOLD: f64 = 0.15;   // results.rs:86
```

jank 的定义是「帧时 > 2 × p50」（`results.rs:61-63`）。基线按平台分文件（`benches/pty_baselines/README.md`：`linux-x86_64`、`linux-aarch64`、`macos-aarch64`），CI 在 p99 增长超过 15% 时失败；有意改变性能的 PR 必须同 PR 更新基线，并把干净的 `pty-bench` 输出贴进 PR 正文。快照中 `benches/pty_baselines/` **只有 `README.md`**，平台基线 JSON 由首次 CI 运行播种。

帧时从哪来：`src/timing.rs:1-4` 用 VTE 解析 `CSI ? 2026 h/l` 同步更新标记，即 crossterm 的 `BeginSynchronizedUpdate` / `EndSynchronizedUpdate`，把「一帧」的墙钟时长量出来。

**注意一处内部不一致**：`benches/pty_baselines/README.md` 的示例命令写作 `cargo run -p xai-grok-pager --release --bin pty-bench`，但 `[[bench]] name = "pty_bench"` 声明在 `xai-grok-pager-pty-harness/Cargo.toml:143-146`，且 `xai-grok-pager/Cargo.toml` 里没有任何 `[[bin]]`。

### 9.2 property 测试

**未找到证据**：`rg -l proptest --glob '*.rs' crates` 无命中；根 `Cargo.toml` 中也无 `proptest` / `quickcheck` / `arbitrary` 依赖。仓库里的 property 风格检查以「对一批数据跑一组不变量的断言函数」形式存在（即 §8.2 的 `scroll_matrix/invariants.rs`），而非专门的 property-testing 库。

---

## 10. 元测试：文档即契约

这是 Grok Build 最有辨识度的工程化手法：**用 `include_str!` 把 Markdown 读进测试，解析表格里的反引号单元，与代码注册表做双向对账。**

### 10.1 feature 注册表 ↔ 运维文档

`crates/codegen/xai-grok-pager/tests/registered_features_are_documented.rs:1-23` 全文 23 行，头部注释就是它的存在理由：

```rust
//! `FEATURES` is the source of truth and the operator tables are hand-maintained
//! mirrors with no compile-time check of their own. This test is that check.
const ENTERPRISE: &str = include_str!("../docs/internal/25-enterprise.md");
const ENV_VARS:  &str = include_str!("../docs/internal/22-environment-variables.md");

#[test]
fn every_registered_feature_reaches_the_operator() {
    for spec in FEATURES {
        assert!(ENTERPRISE.contains(&format!("`{}`", spec.key)), …);
        assert!(ENV_VARS.contains(&format!("`{}`", spec.env)), …);
    }
}
```

**快照缺口**：`docs/internal/` 目录在本快照中不存在（`ls docs/internal` → No such file or directory），所以这个测试文件在当前快照下无法编译。

### 10.2 配置参考页 ↔ 配置解析器（双向）

`crates/codegen/xai-grok-shell/src/config_docs/mod.rs:1-7` 的模块文档明确规格：「CI fails when a registered key has no row, or an MCP or `features.*` row **names an unknown key**. It also fails when a Requirements or Managed cell disagrees with the resolver metadata.」对应的测试：

- `registered_features_have_pin_rows`（`:253`）——每个 `FEATURES` 路径必须有行，且 Requirements 单元格必须是 `pin`
- `unmirrored_features_have_rows`（`:272`）——`UNMIRRORED_BOOLEAN_FEATURES` 逐项断言期望值（`unmirrored_requirements`，`:231-240`，未知 key 直接 panic）
- `mcp_known_fields_have_rows`（`:289`）、`telemetry_otel_keys_have_rows`（`:305`）——`TELEMETRY_OTEL_KEYS` 是硬编码的 25 个路径常量（`:21-47`）
- `rows_name_real_keys`（`:320`）——**反向方向**：文档里每一行都必须是一个真实键

### 10.3 status line ↔ 用户手册

`crates/codegen/xai-grok-pager/src/app/status_line_tests.rs` 三连：

- `every_field_on_the_wire_is_named_in_the_guide`（`:356`）——遍历 wire fixture 的 JSON，**只校验叶子值路径**（容器由其成员代言），收集缺失项并断言为空
- `every_builtin_item_is_named_in_the_guide`（`:401`）——`StatusLineItem::ALL` 每个名字必须在 `## Set up` 段的表格里
- `the_numbers_the_guide_states_are_pinned`（`:418`）——把文档写死的数字钉在代码常量上（如 `REFRESH_FAILURES_TO_PAINT == 3`），注释：「so none of them can move without editing 25-status-line.md in the same change」

配套三个解析辅助（`:476`、`:510`、`:519`），其中 `table_cells` 的实现揭示了文档格式的隐式契约：

```rust
fn table_cells(section: &str) -> impl Iterator<Item = &str> {
    section.lines()
        .filter(|line| line.starts_with('|'))
        .flat_map(|row| row.split('`').skip(1).step_by(2))
}
```

即：**表格单元必须用反引号包裹**，反引号是机器可读标记而非排版装饰。`section` 刻意按 `##` 切段并注释「Scoped, because a name in a neighbouring section would otherwise vouch for a row that was deleted」。`documented_paths` 还支持 `head.{a,b}` 展开与行首 `.` 续接父路径两种表格写法。

### 10.4 settings 矩阵 ↔ 测试覆盖率

`crates/codegen/xai-grok-pager/tests/settings_e2e.rs:1-5` 声明规则：「Every new `SettingMeta` in `default_settings()` MUST add an entry to `ALL_SETTINGS_EXERCISED` and a test for its keyboard AND mouse paths.」`ALL_SETTINGS_EXERCISED` 有 49 项（`:23-76`，含 `contextual_hints.*` 子开关），由两个测试守住：

- `every_registered_setting_is_exercised`（`:79`）——注册表 `all()` 减矩阵必须为空，失败消息直接给出补丁指引
- `matrix_is_subset_of_registry`（`:95`）——反向：矩阵不能有幽灵条目

这个文件 564 个测试函数、274 KB，是仓库最大的单个测试文件。

### 10.5 架构禁区扫描

`crates/codegen/xai-grok-pager-minimal/src/guard.rs:1-40` 把设计约束写成可运行的源码扫描：用 `include_str!` 读入**本 crate 的每个模块源文件**，断言不含三个禁用标识符（`resize_purge_rerender`、`emit_to_scrollback`、`resize_viewport_height`），失败消息解释后果：「it would double-print committed scrollback (design K6 / risk #2)」。文件顶部注释还提醒「Keep in sync with `lib.rs`'s module list: a module missing here is a hole in the guard」——即这个 guard 有一个已知的、需要人工维护的缺口。

### 10.6 文档自身的结构校验

- `src/docs.rs:279-370`：`user_guide_entries_are_valid`（内容非空、首字符必须是 `#`）、`user_guide_entries_have_no_duplicates`、`default_howto_entries_includes_all_user_guide_docs`、`all_titles_covers_both_tables`、`extract_writes_docs_and_cleans_stale`（抽取到 `GROK_HOME` 时会清理陈旧文件，但保留用户自建的 `notes.md`）
- `src/tutorial_docs.rs:89-140`：`topics_are_valid`、`go_deeper_titles_resolve_to_real_guides`（「a typo'd title would turn the shortcut into a silent no-op」）、`topics_have_unique_titles`、`topics_stay_bite_size`（每页 ≤ 50 行，注释：「Bump this limit only after re-checking a page still reads in under a minute」）

---

## 11. 结构与依赖分层

### 11.1 四个 crate 的边界

```
xai-grok-pager-bin  (composition root, [[bin]] name = "xai-grok-pager")
      │  main.rs 3,774 行 + build.rs
      ├──────────────► xai-grok-pager      (库：全部应用逻辑)
      │                     │
      │                     └──► xai-grok-pager-render  (表现层原语，被 re-export)
      └──────────────► xai-grok-pager-minimal  (可选渲染模式)
                            │
                            └──► xai-grok-pager  ← 反向引用，构成循环
                                 ⇒ 二者不能互相依赖，改用函数指针 seam
```

**`xai-grok-pager`**（`src/lib.rs:1-73`）：652 个文件的库，crate 根有 `#![deny(clippy::indexing_slicing)]`（`:11`），且开头有 `#![allow(unused_imports, unused_variables, unused_mut, unreachable_code, dead_code)]`（`:1-7`）——把死代码告警在库级别整体关掉。它公开 60 个左右模块（`acp`、`actions`、`app`、`headless`、`scrollback`、`settings`、`slash`、`views`、`voice`、`worktree_cmd`…），并把 pager-render 的模块整体转出（`:61-65`）：

```rust
pub use xai_grok_pager_render::key;
pub use xai_grok_pager_render::{
    appearance, clipboard, glyphs, host, input, link_opener, modal_window_state,
    prompt_images, render, search, syntax, terminal, theme, util,
};
```

注意 `#[path = "minimal/api.rs"] pub mod minimal_api;` 与 `#[path = "minimal/hook.rs"] pub mod minimal_hook;`（`:35-38`）——minimal 模式的**契约**住在 pager 里，实现住在另一个 crate。

测试专用模块也在 crate 根显式声明：`#[cfg(test)] pub mod test_util;`（`:66-67`，328 行）。

**`xai-grok-pager-render`**（`src/lib.rs:1-17`）：只暴露 17 个模块，第一个语句就是 `#![deny(clippy::indexing_slicing)]`。**未找到 crate 级文档注释**（第 1 行即 `#![deny(...)]`，第 3 行起直接是 `pub mod`），crate 定位只能从模块名与 `Cargo.toml` 依赖推断。依赖画像是纯表现层：`ratatui`/`ratatui-core`/`crossterm`/`textwrap`/`unicode-bidi`/`unicode-width`/`linkify`/`tui-scrollbar` + `xai-grok-markdown`/`syntect`/`anstyle`/`supports-color` + `vte`（用于把命令输出里的 SGR/光标/`\r` 原生渲染）+ `dark-light`（系统明暗检测）。它**不依赖** pager，因此可被 pager 单向依赖并转出。

**`xai-grok-pager-minimal`**（`src/lib.rs:1-21`）：模块文档把机制说清楚——最小模式下定稿的对话块通过 `xai_ratatui_inline::Terminal::insert_before_rows` 打进终端**原生 scrollback**，交互式 `ScrollbackPane` 不参与，「the terminal owns history」。`# Wiring` 段写明反向依赖会成环、pager 暴露 `xai_grok_pager::minimal_hook` 函数指针、由 binary 调用 `install()` 注册，未安装时 pager 的 minimal 分支是惰性的。`draw()`（`:49-66`）逐行标注了每个 pass 的存在理由（同步更新先把 commit 滚动与 live 重绘批在一起，避免多块提交闪烁）。它也有 `#![deny(clippy::indexing_slicing)]`（`:23`）与 `#[cfg(test)] mod guard;`（`:36-37`）。

**`xai-grok-pager-bin`**：见下节。

### 11.2 横切约定：`test-support` 特性

十余个 crate 声明 `test-support = []` 空特性 + `default-bazel = ["test-support"]`（如 `xai-grok-pager-render/Cargo.toml:120,126`、`xai-ratatui-inline/Cargo.toml:37,40`、`xai-grok-status-line/Cargo.toml:21-22`）。含义是：测试夹具（fixture builder）以 `#[cfg(feature = "test-support")]` 编译进库，普通构建与 Bazel 测试构建各取所需。pager 的 dev-dependencies 里就有一整列带该特性的依赖（`crates/codegen/xai-grok-pager/Cargo.toml:191-197`），例如 `xai-grok-status-line = { workspace = true, features = ["test-support"] }` 后面注释「The fixture that builds a `[ui.status_line]` section field by field」。

共享测试库有两个，分工不同：

- `crates/common/xai-test-utils`（5 个源文件）：hermetic git（把 Bazel 提供的静态 `git` 前插 `PATH`）、`crate_root!()` 宏（Bazel runfiles 或 `CARGO_MANIFEST_DIR`）、tracing 计数、`env_usize`。`Cargo.toml:8-13` 说明 `bazel` 特性在纯 cargo 下关闭并回落到 `CARGO_MANIFEST_DIR`。
- `crates/codegen/xai-grok-test-support`（40+ 个源文件）：`MockInferenceServer`、SSE 生成器、ACP stdio 客户端、`TestSandbox`、`TestProcess`、`UdsProxy`、`MockOtelServer`、`MockManagedConfigServer`、`run_headless`（清单见 `src/lib.rs:8-28`）。两个跨套件统一约定值得注意：
  - `scaled(base: Duration)`（`src/lib.rs:30-39`）——乘 `GROK_TEST_TIMEOUT_SCALE`，让共享 runner 池上的负载表现为变慢而非失败
  - `run_in_local_set`（`:40-48`）——stdio 客户端的连接不是 `Send`，任务必须 local spawn

它的测试与源码同目录并置（`*_tests.rs`：`mock_otel_server_tests.rs`、`acp_policy_tests.rs`、`model_reply_tests.rs` 等），与 pager 一致。

---

## 12. 二进制组成：`pager-bin` 作为 composition root

`crates/codegen/xai-grok-pager-bin/Cargo.toml:9-13` 直说存在理由：

> Composition-root binary for the Grok Build TUI. The artifact is still named `xai-grok-pager`. This package exists so the binary can link both the pager library and the optional `xai-grok-pager-minimal` render mode: `minimal` depends on `xai-grok-pager`, so the pager library cannot depend back on it (cargo cycle). The binary installs the minimal-mode IoC hooks at startup.

`src/main.rs` 3,774 行 + `src/agent_command.rs` 100 行 + `build.rs` 49 行。

### 12.1 分配器

```rust
#[cfg(all(feature = "jemalloc", unix))]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;   // main.rs:9-11
```

`jemalloc` 在 `default` 与 `default-bazel` 特性集里（`Cargo.toml:86-95`）。三个 jemalloc 包按用途分工，注释都写明了（`Cargo.toml:66-77`）：

- `tikv-jemallocator`（`stats` 特性，二进制局部开启，不走工作区）——为了 `stats.allocated` / `stats.resident`
- `tikv-jemalloc-sys`——裸 mallctl，用于「内存悬崖」时的 arena purge 钩子：`purge_jemalloc_retained_pages`（`main.rs:1878-1900`，经 `install_release_hook` 注册）
- `tikv-jemalloc-ctl`——堆剖析/统计（`prof.active`、`prof.dump`、`epoch`、`stats.*`）

配套函数：`jemalloc_allocator_stats`（`:1902-1925`，先推进 epoch 再读 gauge）、`jemalloc_stats_dump`（`:1927-1946`，裸 `malloc_stats_print`，因为 ctl 无回调形式）、`jemalloc_heap_stats`（`:1948-1958`）、`jemalloc_set_prof_active` / `jemalloc_read_prof_active` / `jemalloc_prof_available` / `jemalloc_dump_to_path`（`:1960-1979`）。

### 12.2 二进制加固

`obfstr = { workspace = true }` 与 `cryptify = { workspace = true }`（`Cargo.toml:58-60`），注释：「Binary hardening: compile-time string + control-flow obfuscation.」对应 pager 库里的 `pub mod obf;`（`crates/codegen/xai-grok-pager/src/lib.rs:41-42`，带 `#[allow(unused_imports, unused_macros)]`）。这是 CLI 二进制层的做法，不影响库 API。

### 12.3 特性矩阵与「测试缝」

```
default        = [jemalloc, sandbox-enforce]
default-bazel  = [jemalloc, sandbox-enforce, local-workspace]
test-seams     = []          ← 明确声明永不进入发行集
release-dist   = [xai-grok-pager/release-dist]
```

`test-seams` 的注释（`Cargo.toml:99-103`）值得引用：「Test-only, prod-inert seam: lets the e2e harness install a TEST-signed managed-policy key the spawned pager trusts. **Double locked out of production**: absent from `default`, `default-bazel`, and `release-dist`, and the install hook is additionally gated on `debug_assertions`. Never add this to a shipped feature set.」——测试与生产之间的口子被两层条件锁住，并在清单里写下禁令。

### 12.4 构建脚本

`build.rs` 只做两件事：

1. **版本戳**（`:29-38`）——把 `GROK_VERSION` 或 `CARGO_PKG_VERSION` 与 12 位 `HEAD` 拼成 `VERSION_WITH_COMMIT`。`:17-27` 有一段重要注释：「Never emit a missing path: cargo treats it as always dirty and rebuilds this crate every build」，即对 git 路径的 `rerun-if-changed` 必须先过滤存在性。
2. **Windows 延迟加载**（`:40-48`）——`/DELAYLOAD:ProjectedFSLib.dll` + `delayimp.lib`，因为 `grove-projfs` 导入的 `ProjectedFSLib.dll` 在未启用 `Client-ProjFS` 时会以 `STATUS_DLL_NOT_FOUND` 直接杀死启动，且其 `build.rs` 的 link args 不会传播到这个 exe。

### 12.5 protoc

**`pager-bin` 中没有 protoc 相关代码或构建逻辑**（在 `build.rs` / `main.rs` 中 `rg protoc` 无命中）。protoc 只出现在仓库根的开发工具链里：`bin/protoc` 是一个 dotslash 脚本（`file bin/protoc` → "a dotslash script, ASCII text executable"），由 `crates/build/xai-proto-build/src/find_protoc.rs:27-46` 按三级顺序解析：`$PROTOC` → 逐级向上找 `bin/protoc` → `$PATH` 上的 `protoc`。根 `README.md:70-71` 描述为 proto codegen 的解析路径。

---

## 13. 文档即产品

### 13.1 两套文档、两种节奏

| 目录 | 文件数 | 定位 |
| --- | --- | --- |
| `crates/codegen/xai-grok-pager/docs/user-guide/` | 27 + `README.md` | 完整使用手册，README 按 Tier 1/2/3 分档并给出表格导航 |
| `crates/codegen/xai-grok-pager/docs/tutorial/` | 9 | `/tutorial` 覆盖层的分页入门，严格 opt-in |

### 13.2 文档编译进二进制

`src/docs.rs:36-45` 用宏把每个 Markdown 以 `include_str!` 编进静态表：

```rust
macro_rules! guide {
    ($file:literal, $title:literal, $desc:literal) => {
        Doc { filename: $file, title: $title, description: $desc,
              content: include_str!(concat!("../docs/user-guide/", $file)) }
    };
}
```

启动时抽取到 `<grok_home>/docs/user-guide/`（`docs.rs:241-252`），并清理已删除的文件（`:272`）。`src/tutorial_docs.rs:19-28` 是同构的 `topic!` 宏。模块注释强调「Single source of truth: two static arrays (`USER_GUIDE`, `REFERENCE_DOCS`) hold every doc. All lookups are zero-allocation」。

注意 `find_doc(title)` 是 `go_deeper` 的解析入口，而 `tutorial_docs.rs:15-16` 明确要求 `go_deeper` 「must match a [`crate::docs`] title」——这条约束由 §10.6 的测试守住。

### 13.3 写作风格

**tutorial 页**（`docs/tutorial/04-navigation.md`）：以「屏幕有三个部分」起手，正文用 `**`Ctrl+T`**` 加粗快捷键、列表组织，结尾一行 `*Go deeper: /docs Keyboard Shortcuts*`（`:32`）；全文 ≤ 50 行受测试约束。

**user-guide 页**（`docs/user-guide/25-status-line.md`）：首段一句话定义功能 + 指出 opt-in 方式（`[ui.status_line]` in `~/.grok/config.toml`），随后 `## Set up` 三级场景（Built-in / Command / Disabled / Options），每节配 toml 代码块与表格：

```markdown
| Item | Shows |
| --- | --- |
| `cwd` | Current directory (basename) |
| `cost` | Session cost, hidden below $0.005 so it never shows a misleading `$0.00` |
```

表格单元里的反引号是**机器可读契约**（§10.3 的 `table_cells` 按 `\`` 切片），行文里的具体数值（`$0.005`、`40 columns`、`30`）也常被测试钉住。

### 13.4 一致性校验清单

| 校验对象 | 机制 | 位置 |
| --- | --- | --- |
| feature 键/env 变量 ↔ 运维文档 | `include_str!` + 子串包含 | `tests/registered_features_are_documented.rs:10-22` |
| 配置参考页 ↔ 解析器（双向） | 解析表格 | `xai-grok-shell/src/config_docs/mod.rs:253+` |
| status line wire 字段 ↔ 指南 | 解析表格 + JSON fixture 遍历 | `src/app/status_line_tests.rs:356-421` |
| settings 注册表 ↔ 测试矩阵（双向） | 集合差 | `tests/settings_e2e.rs:79,95` |
| 指南结构 | 首字符 `#`、标题唯一、无重名文件 | `src/docs.rs:283-320` |
| 教程页长度与跳转 | ≤ 50 行、`go_deeper` 必须可解析 | `src/tutorial_docs.rs:93-140` |
| 文档抽取的幂等性与清理 | 临时目录端到端 | `src/docs.rs:346+` |

**未找到证据**：仓库中没有外部链接有效性检查脚本，也没有 CI 工作流文件（快照里无 `.github/`）。

---

## 14. 对本项目的可借鉴清单

### 14.1 建议照抄（成本低、收益直接）

1. **同目录 `tests.rs` + `#[cfg(test)] mod tests;`**，或 `*_tests.rs` + `#[path]`。零基础设施成本，直接解决 TUI 项目「源码 3,000 行、测试写在另一个目录」的可读性问题。
2. **测试函数名写成完整英文断言句**（`every_registered_setting_is_exercised`）。文档价值高于命名成本。
3. **`include_str!` + 反引号表格的元测试。** 我们的 `/help`、快捷键表、问题类型表（Choice/Score/Noul）与 `FEATURES` 类注册表天然适合这套对账，且不需要额外依赖。
4. **架构禁区源码扫描**（`pager-minimal/src/guard.rs` 模式）。对 jev-harness 立即有用：例如「判定不得绕过概率层直接塌缩成 bool」「不得在循环内直接发起 LLM 调用而跳过 Jev 路由」都可以写成源码扫描测试，比 Code Review 可靠。
5. **`pretty_assertions` + 缓冲区就地断言**（`Buffer::empty` + 断言单元格文本）。这是 TUI 渲染层性价比最高的测试形式，不需要 golden 文件维护成本。
6. **`scaled(Duration)` 超时缩放**（`GROK_TEST_TIMEOUT_SCALE`）。任何有超时的异步/进程测试都该有，一行代码换掉一整类 flaky。
7. **`test-support` 空特性 + `default-bazel`** 的模式：夹具进库、默认不编译。我们没有 Bazel，但「`test-support` 特性」这一半仍然可用。
8. **纯文本快照而非像素快照**。insta 只用在等宽文本（diff 行号栏、状态块）这类「断言写起来啰嗦」的地方，全仓库 11 个文件。TUI 不必追求全屏 golden。

### 14.2 值得借鉴但需要条件

9. **PTY + 虚拟终端做端到端。** 收益大，但依赖链重（`portable-pty` + `alacritty_terminal`/`ptyctl`），且必须自己处理两件难事：终端应答回写（`drain_responses`，否则探测挂起）与宿主环境清洗（§6.4 的三张表）。建议早期只做一条最小烟雾路径（启动 → 见哨兵文本 → 退出）。
10. **harness 独立成 crate，且不被库依赖。** 这条边界让 `cargo test -p <lib>` 永不编译进程级夹具，是我们应该从第一天就定的规则。
11. **声明式 YAML 场景。** 48 个场景的写法很优雅，但需要先有稳定的事件注入原语与产物系统；早期可以只做 Rust 侧的步骤结构体，不上 YAML。
12. **帧时通过 `CSI ? 2026 h/l` 同步更新标记采集。** 我们若用 ratatui/crossterm，这对标记是白送的；把 `BeginSynchronizedUpdate`/`EndSynchronizedUpdate` 当作帧边界即可获得 p50/p99 与 jank 统计。
13. **性能数值门禁（p99 +15%、按平台分基线）。** 价值高，但必须有自己的 CI 机器池；没有稳定 runner 时会造成误报。

### 14.3 成本过高或当前阶段不建议

14. **`pty-bench` + 平台基线体系**（12 个测试根 + 4 个辅助二进制 + 42k 行 harness + 平台 JSON）。这是有专职基础设施团队才维持得住的投入。
15. **`scroll_matrix` 级别的飞行记录仪 + 形式化不变量（`I-ORD`/`I-CAP`/…）**。属于「已经踩过滚动疑难 bug」之后的追认性投资，我们尚无对应的 bug 历史去驱动它。
16. **jemalloc 三包 + mallctl 内存剖析钩子**。TUI 长会话的内存悬崖确实是真问题，但 `tikv-jemalloc-sys` + `-ctl` + 裸 mallctl 的组合是可观的维护面；建议先用更粗的 RSS 采样（`xai-grok-test-support::ResourceSnapshot` 的做法）观察，确认有必要再上。
17. **`obfstr` + `cryptify` 二进制加固**。与「保持私有、不自发布」的定位不匹配，暂不需要。
18. **`#![deny(clippy::indexing_slicing)]` + 测试侧全局放开**。这套「生产禁索引、测试放开」的策略很干净，但需要配套 `clippy.toml`（注意 clippy 取**最近**的配置、不合并，仓库根 `clippy.toml:4-5` 特别提醒了这点）与自写的 `at`/`at_mut` 辅助蔓延。可作为后期收紧项，不建议起步就引入。

---

## 关键结论（供合稿引用）

1. Grok Build 的工程化核心手法是**元测试**：把跨模块约定写成会失败的测试，而非只写行为测试（`crates/codegen/xai-grok-pager/tests/registered_features_are_documented.rs:1-22`、`crates/codegen/xai-grok-shell/src/config_docs/mod.rs:1-7`）。
2. 文档与代码对账的实现方式是 `include_str!` 读入 Markdown、再按反引号切表格单元做集合运算；**反引号是机器可读标记，不是排版装饰**（`crates/codegen/xai-grok-pager/src/app/status_line_tests.rs:510-515`）。
3. 快照测试被刻意限制在极小范围：全仓库只有 11 个 insta `.snap`，全部是被测函数返回的**纯文本**，不存在像素级或全屏 golden 快照（`crates/codegen/xai-grok-pager/src/scrollback/blocks/tool/edit.rs:2412` 等 9 处 + `src/app/status_blocks.rs:323,351`）。
4. 渲染层测试的默认形式是 `ratatui::buffer::Buffer` 就地断言文本，而非落盘比对；最密集的单文件有 117 处（`crates/codegen/xai-grok-pager/src/views/dashboard/render_tests.rs`）。
5. PTY 端到端由独立 crate 承载，且该 crate **刻意不被 pager 依赖**，以保证 `cargo test -p xai-grok-pager` 永不编译进程级夹具（`crates/codegen/xai-grok-pager-pty-harness/Cargo.toml:10-13`）。
6. 屏幕断言的底座是 `alacritty_terminal` 虚拟终端（经 ptyctl），并把模拟器产生的终端应答回写给子进程——不回写会导致终端探测挂起、`--minimal` 退化为全屏（`.../src/screen.rs:31-38`）。
7. PTY 子进程启动时主动清洗宿主终端环境：品牌标记、复用器标记、`COLORFGBG`/`GROK_APPEARANCE` 等一律摘除，注释直言「一个泄漏就会把子进程重新归类」（`.../src/pty_spawn.rs:19-70`）。
8. `#[ignore]` 是所有进程级用例的统一闸门（全仓库 390 处），由 Bazel 显式 opt-in 并对该家族限流到 4 个 libtest worker（`.../tests/pty_e2e_smoke.rs:3`）。
9. 基准分两套：criterion 微基准（pager 四个 target：`render`/`search`/`edit_highlight`/`resize`）与 spawn 真实二进制的 `pty-bench`，后者按平台维护 JSON 基线、p99 回归 >15% 即失败（`.../src/results.rs:86`、`.../benches/pty_baselines/README.md`）。
10. 仓库中**未找到 proptest/quickcheck 等 property-testing 库**；property 风格由对遥测日志的不变量判定替代（`.../src/scroll_matrix/invariants.rs:23-40`）。
11. `pager-bin` 是纯 composition root，存在的唯一结构性理由是打破 `pager-minimal → pager` 造成的 cargo 循环，二者用函数指针 seam（`minimal_hook` / `install()`）连接（`crates/codegen/xai-grok-pager-bin/Cargo.toml:9-13`、`crates/codegen/xai-grok-pager-minimal/src/lib.rs:15-21`）。
12. 测试专用能力通过 `test-support` 空特性编译进库（十几个 crate 一致），二进制侧则用 `test-seams` 特性配「永不加入发行集」的明文禁令 + `debug_assertions` 双重锁（`crates/codegen/xai-grok-pager-bin/Cargo.toml:99-103`）。
13. 用户手册与教程都经 `include_str!` 编进二进制并在启动时抽取到 `GROK_HOME`，因此「文档即产品」不是口号而是运行时可观察的行为（`crates/codegen/xai-grok-pager/src/docs.rs:36-45,241-272`）。
14. 快照缺口提醒：`tests/registered_features_are_documented.rs` 依赖的 `docs/internal/` 目录不在本快照中，该测试在当前快照下无法编译；另 `benches/pty_baselines/README.md` 的示例命令（`-p xai-grok-pager --bin pty-bench`）与清单声明（`[[bench]] name = "pty_bench"` 在 pty-harness）不一致。
