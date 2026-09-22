# jev-harness 1.0 路线图：研究路线

> **性质**：施工路线——把[研究目标与实验设计](RESEARCH_GOALS.md)（下称「研究目标」）的 RQ/H 与 §12 阶段，落成 1.0 的 crate 边界、运行时规格与实验执行计划。
> **本文不产生研究结论**，也不新增阈值、效应量、工期或性能数字：所有实验设计与指标口径以研究目标 §9–§10 为准，本文只给落点与章节引用。
> **引用约定**：不带文档名的 `§x` 一律指本文件；引用其他文档时写明「研究目标 §x」或「[Jev 调研报告](jev-research.md) §x」。
>
> - 版本：1.0 研究路线（本版重写；2026-09-22 追加 M0 实现回写与 M0 真调基线回写，见 §11）
> - 日期：2026-09-22
> - 上游文档：[研究目标](RESEARCH_GOALS.md)、[Jev 调研报告](jev-research.md)、[项目笔记](project-notes.md)、[Grok TUI 设计调研](grok-tui-design.md)、[项目总纲](../AGENTS.md)
> - 代码基线：`crates/` 下现有五个 crate（`jh-core` 于 M0 落地），事实逐条见 §1.3
> - 研究目标 §12 的 M0–M4 是本文 §5 的直接来源；M5 不在 1.0 范围内（见 §5、§8）

## 1. 前提与现状

### 1.1 本版前置的四个已确认决定

| # | 决定 | 本文落点 |
| --- | --- | --- |
| 1 | 1.0 覆盖研究目标 §12 的 **M0–M4**；M5（真实仓库迁移、多目标共享、能力编译、跨域迁移）列为后续版本 | §5、§8 |
| 2 | 旧版的四个决策点（Gate / Route / Boundary / Judge）**整体移除**，不再作为路线图内容；**Jev 只以「可替换判断器之一」出现**（定位依据：[Jev 调研报告](jev-research.md) §13） | §2.3、§3.2、§4、§11 |
| 3 | 新增 `crates/jh-core` 承载运行时对象模型（事件/证据、判断记录、依赖图、失效传播、问题版本）；`jh-app` 继续做组装层与 CLI；`jh-jev` / `jh-openai` / `jh-tui` 既有公共 API 不动 | §3.1、§3.3 |
| 4 | 1.0 **不做 TUI**：只交付批处理实验 + CLI/JSON 报告；现有 `jh-tui` 组件套件冻结保留，研究视图推迟到 1.x | §3.1、§5、§7.1、§8 |

### 1.2 不可动摇的前提

- **语言与形态**：Rust 全栈（[AGENTS.md](../AGENTS.md) 技术栈要求）；1.0 的可执行形态是批处理实验 + CLI（决定 4）。
- **LLM 通道**：OpenAI **Responses API** 为目标协议，实现见 `crates/jh-openai`（`crates/jh-openai/src/client.rs:65` `Client::create`、`:84` `Client::stream`）。模型必填、无内建默认（`:144` `ClientBuilder::model`）；key 来自环境变量 `OPENAI_API_KEY`（`:16`），base URL 可覆盖（`:139` `ClientBuilder::base_url`）。
  **provider 与配置口径（2026-09-22 回写）**：LLM 侧走 DeepSeek，模型 `deepseek-flash`；`OPENAI_API_KEY` / `OPENAI_MODEL` / `OPENAI_BASE_URL` 三者都必填（缺 `OPENAI_BASE_URL` 显式失败，避免 key 被发往 crate 自带的官方缺省端点；`crates/jh-app/src/judge/llm.rs:57-81`），base URL 写法为 `https://api.deepseek.com`（不带尾斜杠；`ClientBuilder::build` 同时会裁剪尾斜杠，端点即 `{base_url}/responses`，`crates/jh-openai/src/client.rs:169`、`:109`）。
  **`/responses`：实测确认可用（2026-09-22 真调）**。文档证据：DeepSeek 官方[《Using the Responses API》](https://api-docs.deepseek.com/guides/responses_api/)写明 “our API now supports the Responses API format, with the base_url being https://api.deepseek.com”，示例即 `client.responses.create(model="deepseek-flash", …)`；官方 Codex 集成页的 `wire_api = "responses"` 与定价页的 `Responses API ✓` 同向（M11 探测抓取）。**2026-09-22 本环境真调**：`OPENAI_API_KEY=$DEEPSEEK_API_KEY OPENAI_BASE_URL=https://api.deepseek.com OPENAI_MODEL=deepseek-flash cargo test -p jh-openai --test live_smoke -- --ignored --nocapture`——非流式 `live_create_returns_a_completed_response`（`Completed`、usage 38/15）与 SSE 流式 `live_stream_matches_the_final_text`（deltas 重建最终文本）均通过，端点可用（成功路径 2xx）、SSE 事件名与响应体反序列化在运行层面成立；读数与报告位置见 §5 M0。
  **新登记限制（2026-09-22 真调；本批只登记，不改代码）**：
  1. `deepseek-flash` 的 thinking 模式不支持强制 `tool_choice`：`live_stream_reports_a_forced_tool_call` 失败，错误体 `{"message":"Thinking mode does not support this tool_choice","type":"invalid_request_error","code":"invalid_request_error"}`（HTTP 400，见已归档 finding）。finding：`.tower/comms/findings/20260922-tower-bug-deepseek-deepseek-flash-thinking-tool-choice.md`。
  2. thinking 的 reasoning token 计入 `max_output_tokens`：LLM 结构化判断器写死 1024（`crates/jh-app/src/judge/llm.rs:46`），预算被 reasoning 耗尽，4/4 调用返回 `Incomplete`（`incomplete_details.reason=max_output_tokens`），判断无读数；同一提示词抬到 8192 即 `completed` 且输出为纯 JSON（复现命令与读数见 §5 M0）。finding：`.tower/comms/findings/20260922-agent-live-bug-llm-deepseek-flash-4-4-incomplete-1024-max-output-tokens-thi.md`。
  **无状态约束**：DeepSeek 的 Responses 实现无状态（`store`、`previous_response_id` 不支持，未支持参数被静默忽略），因此每次判断请求自带全部材料，判定契约放进用户消息而不是可能被丢弃的 `instructions` 字段（`crates/jh-app/src/judge/llm.rs:18-26`）。
- **Jev 通道**：使用真实 API；key 只经环境变量（`TYPESAFE_API_KEY`，`crates/jh-jev/src/client.rs:69` `Client::from_env`），不入库、不进文档与日志；模型 pin `jev-1.13.0`（`crates/jh-jev/src/client.rs:20` `DEFAULT_MODEL`），请求显式带版本化 ID、不用别名（依据：[Jev 调研报告](jev-research.md) §5.1、§13.3.5）。
- **判定分层与概率优先**：能让确定性代码判断的不调模型，能让 Jev 判断的不调 LLM；`probabilities` / `confidence` / `noul` 原值保留，不提前塌缩成布尔值（[AGENTS.md](../AGENTS.md) 技术栈要求；[Jev 调研报告](jev-research.md) §13.3.3）。
- **项目性质**：个人学习项目、不商用；奥卡姆剃刀、禁止过度防御性编程、错误显式失败（[AGENTS.md](../AGENTS.md) 核心理念）。
- **工具链**：`cargo fmt` / `cargo clippy` / `cargo test` 为提交前标准检查（[AGENTS.md](../AGENTS.md) 技术栈要求）。

### 1.3 现状（2026-09-22 在工作区逐条核对代码，非转述）

| # | 能力 | 现状 | 证据（路径 / 符号） |
| --- | --- | --- | --- |
| 1 | workspace 与依赖锁定 | 已就位 | 根 `Cargo.toml`：`[workspace] members = ["crates/*"]`、`resolver = "2"`、20 项 `[workspace.dependencies]`（3 条内部 path 依赖 `jh-core` / `jh-jev` / `jh-openai`，17 项外部版本：`tokio 1.53`、`reqwest 0.13`、`serde 1.0`、`thiserror 2.0`、`clap 4.6`、`ratatui 0.30`、`crossterm 0.29` 等；其中 `tracing-subscriber` / `dotenvy` / `toml` 目前无 crate 引用）；`Cargo.lock` 已跟踪入库（`git ls-files Cargo.lock` 命中） |
| 2 | Jev 客户端（jh-jev） | 已完成；**真调已通（2026-09-22）** | `crates/jh-jev/src/question.rs:119` `Question::{Choice,Score,Noul}`；`answer.rs:62` `Answer::{Choice,Score,Noul,Unknown}`；`answer.rs:147` `NoulAnswer`（无 `confidence` 字段，类型层面拦住误用）；`client.rs:108` `Client::system_one`（`POST {base_url}/v1/systemone`）；`error.rs:124` `Error`（401/422/429/529 各自变体，`:180` `request_id`、`:193` `retry_after`）；`fixtures/` 8 份官方示例请求/响应，**是官方文档抄件、不是真实录包**（`tests/fixtures.rs` 自述）；真调 smoke 在 `tests/smoke.rs` 标 `#[ignore]`；2026-09-22 真跑通过（model `jev-1.13.0`、request_id、usage 356/38），`exp --judge jev` 4/4 成功，读数见 §5 M0 |
| 3 | OpenAI 客户端（jh-openai） | 已完成；**非流式与 SSE 真调已通（2026-09-22）；强制 tool_choice 用例按 provider 限制失败** | `crates/jh-openai/src/client.rs:48` `Client::builder`、`:65` `create`、`:84` `stream`、`:218` `ResponseStream::next_event`；`sse.rs:18` `SseDecoder`（`:35` `push`，手写 SSE，含跨块多字节与 CRLF 处理）；`types.rs:497` `Event`（文档 59 个流事件中解码 20 个，其余落入 `Event::Other`）；`fixtures/` 7 份手工构造的离线样本（非录包）；`tests/live_smoke.rs` 3 个用例标 `#[ignore]`；2026-09-22 真跑 2 通 1 败（`deepseek-flash`、`Completed`、SSE deltas 重建文本；失败与 LLM 判断器读数见 §1.2 新登记限制与 §5 M0） |
| 4 | TUI 组件库（jh-tui） | **组件库已落地，1.0 不接线（留待 1.x 接线）** | `crates/jh-tui/src/components/mod.rs:11-14` 导出回滚区 / 提示框 / 状态行与快捷键栏 / 审批卡；`app.rs:55` `App`、`actions.rs:22` `ActionId`、`render.rs:65` `RenderGate`、`terminal.rs:81` `TerminalGuard`、`blocks.rs:93` `BlockOutput`（块模型）；快照测试是 `TestBackend` 缓冲区上的内联多行断言（`tests/snapshots.rs` + `tests/common/mod.rs`），不是 insta、无 `.snap` 文件。注意 `RenderGate` 是**重绘闸门**，与旧版路线图的 Gate 决策点同名不同义 |
| 5 | 组装层与 CLI（jh-app） | **M0 已接线**（`exp`）；`chat` / `ask` / `run` 仍为空壳 | `crates/jh-app/src/main.rs:47-65` `ExpArgs`（`--judge` 可重复 / `--calls` / `--replay` / `--out`）、`:78` `run_exp`；`chat` / `ask` / `run` 仍打印「未实现」并返回 `ExitCode::from(2)`（`:70-73`）；实验层模块 `judge/`（五种适配器，`src/judge/{jev,llm,rule,replay,simulated}.rs`）、`tasks`、`calls`、`metrics`、`report`、`exp` 已落地；依赖 `jh-core` / `jh-jev` / `jh-openai` / `tokio` 等（`crates/jh-app/Cargo.toml`） |
| 6 | 运行时对象与实验设施 | **M0 部分落地** | **已有**：判断记录（`crates/jh-core/src/record.rs`）、问题定义与版本（`crates/jh-core/src/question.rs`）、初始任务集与隐藏真值（`crates/jh-app/src/tasks.rs` + `crates/jh-app/tasks/initial_tasks.json`）、调用记录落盘（`crates/jh-app/src/calls.rs`）、指标统计与 JSON 报告（`crates/jh-app/src/metrics.rs`、`crates/jh-app/src/report.rs`）；**仍未建**：事件日志与证据、依赖图、失效传播、行动前提复核、观测器契约（M1–M3 的对象）；`var/` 由运行时创建、不入库（`.gitignore` 已列；无实验脚本，命令行 `jev exp` 承担） |
| 7 | 测试基线 | 全绿（2026-09-22 复核） | `cargo test --workspace`：253 passed / 0 failed / 6 ignored（6 个 ignored = jh-jev 1 + jh-openai 3 + jh-app 2，**全部是真调 smoke**） |
| 8 | crate 内部模块划分 | jh-core / jh-app **已定并回写**（§3.4）；通道 crate 冻结不动 | [AGENTS.md](../AGENTS.md) 的「各 crate 内部的模块划分仍待定」现只适用于未动的通道 crate；jh-core 与 jh-app 的实际模块与类型见 §3.4 |

### 1.4 与「研究目标」的差距

通道侧四个 crate 只提供了**外部边界**：两条模型通道（Jev / OpenAI）与一个 UI 组件套件；M0（M9 / M10）补上了 `crates/jh-core` 的判断器抽象与判断记录、`crates/jh-app` 的五种判断器适配、初始任务集与 `jev exp` 基线报告（落地状态见 §5 M0）。研究目标 §5–§8 要求的**运行时对象大部分仍不存在**（事件日志与证据、依赖图、失效传播、行动前提复核、观测器契约），§9–§10 要求的**实验设施**只落了最小的一层（固定问题集、隐藏真值、指标统计与 JSON 报告；快照与扰动、独立验收、计价工具动作还没有）。1.0 的剩余工程量主要落在：`jh-core` 的依赖图与失效传播（M1）、问题集合精化（M2）、观测能力建设（M3）与 `jh-app` 实验层的对照编排。

## 2. 研究命题与 1.0 野心

### 2.1 命题

研究目标是「探索 Jev 能否使一种可维护、可修订、可扩展自身认知结构的 Agent Harness 成为实用架构」，三条相互连接的能力为：维护认知状态、修订认知结构、扩展观测能力；核心问题与完整表述见研究目标文首与研究目标 §1、§3.1。**本文不重述命题**，只负责把命题落成可执行的建设顺序。

### 2.2 研究问题清单（引用，不复述）

| 编号 | 一句话 | 落点 |
| --- | --- | --- |
| RQ0 / H0 | Jev 是否适合承担本任务族的局部语义判断 | §6.7 |
| RQ1 / H1 | 显式依赖与失效传播能否改善动态任务 | §6.7 |
| RQ2 / H2 | 增量判断是否优于全量重算 | §6.7 |
| RQ3 / H3 | 系统能否通过失败学到新的状态区分维度 | §6.7 |
| RQ4 / H4 | 建设观测能力是否带来可累积优势 | §6.7 |
| RQ5 / H5 | Jev 是否使高密度语义控制变得实用 | §6.7 |

问题、假说与否定信号的原文见研究目标 §4；结论等级的判定原则见研究目标 §10.4。H1 与 H2、H3 与 H4 必须分别比较（研究目标 §4 末段）。

### 2.3 Jev 的位置：可替换判断器之一

- **抽象建在「决策」上，而不是「模型调用」上**：输入 `state + 类型化 questions`，输出类型化答案 + 概率 + usage；Jev / LLM 结构化输出 / 本地分类器 / 确定性规则实现同一组接口，降级 = 换后端而非改调用点（[Jev 调研报告](jev-research.md) §13.3.1，本节最重要的一条架构结论）。
- **Jev 是 harness 内的「快速判定层」，不是主模型、不是安全边界、不是精度天花板**（同报告 §13.5 一句话定位）；角色候选与成立条件、红线见同报告 §13.1、§13.5。
- 因此 1.0 的判断器集合至少包含五种：**Jev（真实 API）**、**LLM 结构化判断**、**确定性规则**、**录制回放**、**模拟器真值**（来源：研究目标 §8.3 组件分工、§12 M0）。
- **概率是一等数据**：保留 `probabilities` / `confidence` / `noul` 原值，并把模型版本、选项顺序、`request_id`、阈值一起写进判断记录（[Jev 调研报告](jev-research.md) §13.3.3）。
- 旧版的四个决策点（Gate / Route / Boundary / Judge）在本版**不再是路线图内容**：判定出现在哪里，由 §4 的运行时对象与 §6 的实验任务定义（变更说明见 §11）。

### 2.4 1.0 的野心与边界

- **野心**：让研究目标 §5–§7 的三个机制（可失效的语义状态、问题集合精化、观测能力建设）**可运行、可观察、可对照**，并在研究目标 §12 的 M0–M4 内产出可复现的对照结论——包括否定结论。
- **按研究目标 §12，各阶段分别回答**：H0（M0）、H1 与 H2（M1）、H3（M2）、H4（M3）、H5（M4）。这不是承诺正向结果，而是承诺「问题被清楚回答」（研究目标 §3.2、§14.2）。
- **1.0 不追求**：研究目标 §3.2 的「累积能力收益」外部有效性（属 M5）、真实仓库迁移、能力编译、多目标共享判断（§8）。
- **工程形态**：批处理实验 + CLI/JSON 报告（决定 4），可用 `jev exp` 复现（`crates/jh-app`）。

## 3. 架构与目录

### 3.1 crate 布局（1.0 为五个）

| crate | 1.0 职责 | 状态 |
| --- | --- | --- |
| `jh-core` | 运行时对象模型：事件与证据、问题定义与版本、判断记录、依赖图与失效传播；判断器抽象；调度与行动前提复核的规则 | **新增**；M0 已落地判断器抽象与判断记录（merge `20386b4`，模块与类型见 §3.4） |
| `jh-app` | 组装层与 CLI：`chat` / `ask` / `run` / `exp`；判断器适配（Jev / LLM / 规则 / 录制 / 模拟真值）；实验层（任务集、快照与扰动、隐藏真值、指标统计、JSON 报告） | 扩展；M0 已落地五种判断器适配、初始任务集与 `jev exp` 基线报告（merge `22da7bb`，复核尖端 `c3e5583`）；`chat` / `ask` / `run` 仍未接线 |
| `jh-jev` | Jev 通道：既有客户端原样使用 | 冻结（公共 API 不动） |
| `jh-openai` | OpenAI Responses 通道：生成模型与 LLM 结构化判断 | 冻结（公共 API 不动） |
| `jh-tui` | TUI 组件库：已落地，1.0 不接线（留待 1.x 接线）；1.0 不引入其依赖与运行时集成 | 冻结（公共 API 不动） |

1.0 目标目录（只列新增与冻结标记，其余沿用现状）：

```
jev-harness/
├── Cargo.toml                # workspace 与统一依赖清单（现状，见 §1.3 #1）
├── crates/
│   ├── jh-core/              # M0 已落地：判断器抽象与判断记录（模块见 §3.4）
│   ├── jh-app/               # M0 已落地：判断器适配、初始任务集、调用记录、`jev exp` 与报告
│   ├── jh-jev/               # 冻结
│   ├── jh-openai/            # 冻结
│   └── jh-tui/               # 冻结（1.x 接研究视图）
├── docs/                     # 本路线图、研究目标、实验报告（文件名未定，见 §12）
└── var/                      # 运行产物：调用记录与实验报告（`var/calls.jsonl`、`var/exp/<run>.json`；.gitignore 已列，运行时创建）
```

### 3.2 判断器抽象

- **接口面（已定，M9 回写）**：`state + 类型化 questions → 类型化 answers + 概率/置信度 + usage 与显式错误`（[Jev 调研报告](jev-research.md) §13.3.1）；Rust 侧即 `jh_core::Judge`（`crates/jh-core/src/judge.rs`）：`JudgeRequest { snapshot, state, questions }` → `JudgeResponse { source, version, answers, usage, request_id }`，接口返回装箱 future 以便放进 `Box<dyn Judge>`（crate 不引入异步运行时）；一次请求放 1 个还是 N 个问题由调用方决定（`Questions` 即 `QuestionId → QuestionSpec` 的映射）；错误只有 `Unavailable` / `CallFailed` / `InvalidResponse` 三档，不重试、不降级、不兜底。
- **实现与归属（M10 落地）**：五种适配器同住 `crates/jh-app/src/judge/`——Jev → `jev.rs`（调 `jh_jev::Client::system_one`，`version` 取响应的 `model`、`request_id` 取 `x-typesafe-request-id`）、LLM 结构化判断 → `llm.rs`（调 `jh_openai` 的 `Client::create`，自带 JSON 输出契约）、规则 → `rule.rs`、录制回放 → `replay.rs`、模拟真值 → `simulated.rs`；`JudgeName` + `build()` 按名字构造，构造失败只标「未验证」，不在实现内部换后端（研究目标 §8.3）。
- **复用条件**：结果只在输入快照、问题含义、模型版本与适用范围全部匹配时可复用（研究目标 §8.4）。
- **发起方式**：同一快照上的兼容问题可批量发送，有依赖的阶段按顺序执行；1.0 必须能同时表达「一次多问」与「逐问」，因为批处理是否会改变判定尚未解决（[Jev 调研报告](jev-research.md) §3.6 与 §8.4 冲突未解，实测见其 §14.3 E3；研究目标 §8.4）。

### 3.3 依赖与构建约束

- 各 crate 一律 `dep = { workspace = true }` 引用根清单，不写死版本（避免版本分叉）；`jh-core` **不新增第三方依赖为默认立场**：预计只用已登记的 `serde` / `serde_json` / `thiserror`，是否引入 `tokio` 视调度实现而定（未确定，§12）。
- **不引入**图数据库、复杂 DSL 或独立服务：研究目标 §8.1 明确初版可用简单持久化与邻接表。
- **依赖方向**：`jh-core` 不依赖 `jh-jev` / `jh-openai`（否则对象模型绑死在某个判断器上）；通道专属的 wire 类型由 `jh-app` 的适配层转换（避免让通道 crate 变厚，也满足决定 3 的「公共 API 不动」）。

### 3.4 模块划分

**已回写（2026-09-22，M9 / M10 落地）**：

- `crates/jh-core/src/`：`answer`（`Answer::{Choice,Score,Noul}`、`Usage`）、`error`（`Error::{Unavailable,CallFailed,InvalidResponse}`）、`judge`（`Judge`、`JudgeRequest`、`JudgeResponse` 与 `validate`）、`question`（`QuestionId`、`QuestionVersion(u32)`、`QuestionSpec`、`Questions`、`Question::{Choice,Score,Noul}`）、`record`（`JudgmentRecord`、`Judgment`、`JudgeSource::{RealApi,Llm,Rule,Replay,SimulatedTruth}`）、`state`（`State`、`StateEntry`、`SnapshotId`）。
- `crates/jh-app/src/`：`judge/`（`mod.rs` 的 `JudgeName` / `build()` 与 `jev` / `llm` / `rule` / `replay` / `simulated` 五种适配器）、`tasks`（任务集、隐藏真值与校验）、`calls`（JSONL 调用记录与回放索引）、`metrics`（一致读法 / Brier / 对数损失 / 分位数）、`report`（报告结构与口径声明）、`exp`（运行编排）、`error`。
- 冻结的 `jh-jev` / `jh-openai` / `jh-tui` 模块划分不动；[AGENTS.md](../AGENTS.md) 的「各 crate 内部的模块划分仍待定」现只适用于这些通道 crate。
- `jh-core` 的对象划分仍以研究目标 §8.1 的一等对象清单为准：M0 只落了判断器抽象与判断记录，其余一等对象随 M1–M3 逐批回写（§12 未确定项 1 已定）。

## 4. 运行时规格

规格按「对象 / 机制」逐条给出 1.0 的**最小要求**与依据；细节设计以研究目标 §6–§8 为准。

| # | 对象 / 机制 | 1.0 最小要求 | 依据 |
| --- | --- | --- | --- |
| 1 | 事件与证据 | 追加写、不可变的原始记录，保留来源、时间、对象版本与内容摘要哈希；简单持久化即可 | 研究目标 §5.2、§8.1 |
| 2 | 问题定义与版本 | 同一 ID 的问题改变含义必须产生新版本；1.0 先固定问题集，M2 起走候选→影子→发布→回退流程 | 研究目标 §5.2、§6.2、§6.3 |
| 3 | 判断记录 | 绑定输入快照、问题版本、模型版本、结果与分布、usage、`request_id` 与阈值；离线可回放 | 研究目标 §5.2；[Jev 调研报告](jev-research.md) §13.3.3 |
| 4 | 依赖图 | 区分读取关系、支持关系、冲突候选与行动前提；邻接表实现；依赖环必须被检测 | 研究目标 §5.2、§5.3 |
| 5 | 变化传播与失效 | 撤回一条证据后标记受影响判断与待执行动作、保留无关结果、可按需重算并保留保守全量重查出口 | 研究目标 §5.2、§5.3、§5.4 |
| 6 | 输入投影与快照校验 | 每个判断声明读了哪些对象与字段；共享同一请求 state 的多个问题**保守依赖该请求的全部输入**；请求期间输入发生变化时结果只作审计、不作行动依据 | 研究目标 §5.3 |
| 7 | 行动前提复核 | 行动发出前复核前提；撤销只改变判断的使用资格，已发生的外部副作用不自动回滚 | 研究目标 §5.2、§5.3 |
| 8 | 执行语义 | 至少区分**评估状态**（未请求/排队/运行中/成功/失败）与**依据有效性**（当前有效/待重查/已撤回） | 研究目标 §5.3 |
| 9 | 认识状态 | 「有支持 / 有反证 / 证据冲突 / 证据不足」可由多项判断与程序规则形成；是否作为独立字段待定 | 研究目标 §5.3（原文明确这**不是**四值逻辑） |
| 10 | 行为分类 | `Execute` / `Observe` / `BuildObserver` / `Deliberate` 四类用于调度与成本核算 | 研究目标 §7.1 |
| 11 | 观测器契约 | 要求来源、命题与覆盖关系、测量位置与对象标识、版本与条件、副作用、正负对照与可注入故障、输出格式与失效条件、未覆盖情况 | 研究目标 §7.2 |
| 12 | 判断器抽象 | 五种判断器同一接口、可配置切换（见 §3.2） | 研究目标 §8.3；[Jev 调研报告](jev-research.md) §13.3.1 |
| 13 | 实验环境与真值 | 提供快照、可控扰动、隐藏真值、独立验收与可计价工具动作；模拟器真值模式只准用于诊断与上限估计 | 研究目标 §8.3、§9.2、§10.2 |
| 14 | 调度 | 按事件与需求触发；优先评估可能改变行动的问题；预算覆盖模型调用、输入序列化、工具执行、观测器建设、验证与存储维护 | 研究目标 §8.4 |
| 15 | 指标与统计 | 按研究目标 §10.1 主要指标与 §10.2 诊断指标采集；晋升阈值与报告脚本在 M4 冻结 | 研究目标 §10.1、§10.2、§10.3 |

## 5. 阶段 M0–M4

**命名与划分直接沿用研究目标 §12**（不是旧版的产品式里程碑）；阶段按依赖推进，**不预设日历工期**（研究目标 §12 首段）。每阶段给出建设内容 / 退出证据 / 未达标处理，并把模块归属到 crate。**1.0 完成的定义 = 下表 M0–M4 全部达成**；研究目标 §12 的 M5 不在 1.0（见 §8）。

> **TUI 偏差（决定 4；依据亦见研究目标 §13.1「核心算法先支持无 TUI 的批量实验，再由 TUI 展示相同事件与状态」；须评审确认）**：研究目标 §12 的 M1 行含「最小 TUI」、M4 行含「完整 TUI 展示」，其 §8.3 与 §14.1 也把 TUI 列为组件与交付物。1.0 一律以 **CLI/JSON 报告**代替，研究视图推迟到 1.x；处置逐条见 §7.1，并已列入本文 §13 待人工裁决项（本批不修改 RESEARCH_GOALS.md）。

### M0 测量基础（对应研究目标 §12 第 1 行）

- **建设内容**：判断器接口（`jh-core`）；Jev 真实调用记录（`jh-app` 经 `jh-jev`）；规则 / 录制 / 模拟真值判断器（`jh-app`）；初始任务集（`jh-app` 实验层）。
- **退出证据**：得到本环境的质量、延迟、成本基线，能区分真实 API 与模拟运行（研究目标 §12）。
- **未达标处理**：无 Jev 访问时继续核心模拟，但 Jev 相关结论保持未验证（研究目标 §12）。
- **模块归属**：`jh-core`（判断器抽象）、`jh-app`（适配器、调用记录、初始任务集）、`jh-jev`（复用，不改）。
- **前置约束**：所选 provider 的 `/responses` 端点：**实测确认可用（2026-09-22，非流式 + SSE；§1.2）**；真调未取得读数时 Jev / LLM 相关结论一律保持未验证，不得用模拟或回放结果代替。
- **开放项**：初始任务集的材料来源与规模（本文 §12 未确定项 8；M0 已用 4 样本最小合成集落地，任务族 A/B/C 的规模仍待先导实验）；所选 provider 的 `/responses` 是否可用——**已关闭（2026-09-22 真调确认，§1.2）**。
- **落地状态（2026-09-22 回写；同日追加真调基线）**：
  - **已交付**：判断器接口与判断记录（`jh-core`，merge `20386b4`）；五种判断器适配、初始任务集（4 个中文合成样本 × 3 个问题）、调用记录落盘、`jev exp` 与 JSON 基线报告（`jh-app`，merge `22da7bb`，复核尖端 `c3e5583`）。
  - **真调读数（2026-09-22，`TYPESAFE_API_KEY` / `DEEPSEEK_API_KEY` 可见）**——原始数据在 `var/exp/<run>.json` 与 `var/calls.jsonl`（运行产物、不入库，teardown 后不保留），关键值抄录如下：
    - **Jev（已验证）**：`cargo test -p jh-jev --test smoke -- --ignored --nocapture` 通过——model `jev-1.13.0`（与 pin 一致）、request_id `req_01a0c889ab9071c9a630f7260b13eb6d`、usage 输入 356 / 输出 38；同端点（官方快速开始样例请求，显式 pin `jev-1.13.0`）裸 curl 复核 **HTTP 200**、1.00 s、usage 425/73（状态码与耗时日志 `var/jev-curl-probe.log`，响应体 `var/jev-curl-response.json`；运行产物、不入库）。`cargo run -p jh-app -- exp --judge jev`（报告 `var/exp/run-1790070888194.json`）：4/4 成功，一致率 0.750（9/12）、Brier 0.3870、对数损失 0.5902、延迟 min 334 / p50 353 / p95 882 / max 882 ms、usage 输入 3010 / 输出 367（4 次调用全部上报）。
    - **LLM 通道（已验证）**：`OPENAI_API_KEY=$DEEPSEEK_API_KEY OPENAI_BASE_URL=https://api.deepseek.com OPENAI_MODEL=deepseek-flash cargo test -p jh-openai --test live_smoke -- --ignored --nocapture`——非流式用例通过（`Completed`、输出 `pong`、usage 38/15；usage 为单次运行读数，输出 token 随采样波动）；SSE 用例通过（deltas 重建最终文本）；强制 tool_choice 用例按限制 1 失败（§1.2）。
    - **LLM 结构化判断器（无读数，保持未验证）**：`cargo run -p jh-app -- exp --judge llm`（报告 `var/exp/run-1790070890299.json`）：4/4 失败，错误「判断响应不可用：响应状态是 Incomplete，不是 completed」，quality 无判分，失败调用耗时 min 4956 / p50 5274 / p95 5381 / max 5381 ms（非超时，服务端正常应答）。原因见限制 2（§1.2）：1024 的 `max_output_tokens` 被 thinking token 耗尽；真调复现——同一提示词（`crates/jh-app/src/judge/llm.rs:145-158` 的契约文本 + case-001 材料）分别以 cap 1024 / 8192 直调：`curl -sS -o var/llm-probe-<cap>.response.json -w 'http_status=%{http_code} time_total=%{time_total}s\n' -X POST https://api.deepseek.com/responses -H "Authorization: Bearer $DEEPSEEK_API_KEY" -H "Content-Type: application/json" --data @var/llm-probe-<cap>.request.json`；cap 1024 → `incomplete (reason=max_output_tokens)`，cap 8192 → `completed`、输出纯 JSON（254 字符）；请求与响应落 `var/llm-probe-{1024,8192}.{request,response}.json`（运行产物、不入库）。**本批不改 crate。**
    - **合并基线（真实与离线同表）**：`cargo run -p jh-app -- exp --judge rule --judge simulated-truth --judge llm --judge jev`（报告 `var/exp/run-1790070911353.json`）——rule 4/4、`simulated`（一致率 1.000、Brier 0.0422）；simulated-truth 4/4、`simulated`（1.000 / 0.0000）；llm 0/4、`not_verified`；jev 4/4、`verified`（0.750 / Brier 0.3912、p50 321 / p95 868 ms、usage 3010/367）。
    - **口径**：以上只有一个 4 样本 × 3 题的最小合成集（12 条判断）、单日单次运行，**只能作本环境基线，不构成研究结论**，也不能回答 RQ0 / H0；真实读数与模拟 / 回放读数已在报告中分开标注。
  - **仍未验证**：LLM 结构化判断器在本 provider 上的质量与 usage 读数（4/4 Incomplete，无成功调用，见上）；Jev / LLM 在更大样本与更多任务上的表现；模拟与回放读数仍只作对照与上限估计。
  - **复现命令**（真调测试保持 `#[ignore]`，`cargo test` 不联网；Jev 命令见 `crates/jh-jev/tests/smoke.rs:1-9`，LLM 命令见 `crates/jh-openai/tests/live_smoke.rs:1-12`）：
    - Jev：`TYPESAFE_API_KEY=… cargo test -p jh-jev --test smoke -- --ignored --nocapture`（模型 pin `jev-1.13.0`）
    - LLM：`OPENAI_API_KEY=… OPENAI_MODEL=deepseek-flash OPENAI_BASE_URL=https://api.deepseek.com cargo test -p jh-openai --test live_smoke -- --ignored --nocapture`
    - 基线报告：`cargo run -p jh-app -- exp --judge rule --judge simulated-truth --judge llm --judge jev`（报告落 `var/exp/<run>.json`，调用记录追加进 `var/calls.jsonl`）

### M1 固定问题运行时（对应研究目标 §12 第 2 行）

- **建设内容**：事件日志、固定问题集、依赖图、版本检查、匹配循环基线（研究目标 §12）；「最小 TUI」→ CLI/JSON 报告（本版偏差）。
- **退出证据**：完成失效传播与 H1/H2 对照，包含过期响应和无关变化案例（研究目标 §12）。
- **未达标处理**：若真值模式仍无优势，优先修订架构或收窄任务（研究目标 §12）。
- **模块归属**：`jh-core`（事件与证据、问题版本、判断记录、依赖图、失效传播、行动前提复核）、`jh-app`（匹配循环基线、判断器适配、任务环境、报告）。

### M2 问题集合精化（对应研究目标 §12 第 3 行）

- **建设内容**：反例记录、候选生成、验证、影子运行、版本发布与回退（研究目标 §12）。
- **退出证据**：在隔离的成对任务和未见变体上检验 H3（研究目标 §12）。
- **未达标处理**：若只记住案例，记录局限并保留固定问题模式（研究目标 §12）。
- **模块归属**：`jh-core`（问题版本与失效联动、反例记录结构）、`jh-app`（候选生成走生成模型、影子运行与晋升流程、成对任务环境）。
- **约束**：精化不得修改用户目标、降低验收要求或扩大工具权限（研究目标 §6.4）；新问题必须声明消费者，没有消费者的问题不常驻（研究目标 §6.3）。

### M3 观测能力建设（对应研究目标 §12 第 4 行）

- **建设内容**：`BuildObserver`、测量契约、对照故障注入、复用与失效（研究目标 §12）。
- **退出证据**：在并发故障族中检验 H4，报告累积成本（研究目标 §12）。
- **未达标处理**：若不能独立验证仪器，不允许其自证任务完成（研究目标 §12）。
- **模块归属**：`jh-core`（观测器契约、与判断/证据的失效联动）、`jh-app`（隔离实验环境中的可控程序、隐藏检查、成本核算）。
- **标志性任务**：取消后写入的并发故障（研究目标 §7.3）；观测器必须经过独立检验，测量脚本的成功输出不能成为唯一真实性来源（研究目标 §7.3、§13.2）。

### M4 综合评估（对应研究目标 §12 第 5 行）

- **建设内容**：判断器与架构主对照、关键消融、密度扫描（研究目标 §12）；「完整 TUI 展示」→ CLI/JSON 报告与文档化结果（本版偏差）。
- **退出证据**：回答 H5，形成适用范围、失败案例与可复现报告（研究目标 §12）。
- **未达标处理**：依据证据给出支持、条件支持、不支持或不足（研究目标 §12；结论等级见研究目标 §10.4）。
- **模块归属**：`jh-app` 实验层（对照与消融编排、指标统计、报告导出）；`jh-core` 只按需补对象能力。
- **完成口径**：报告含原始数据位置与复现命令；判定阈值全部来自本项目自测（[Jev 调研报告](jev-research.md) §13.4 #2、§14.3 E2），概率类指标（Brier score / 对数损失等）按研究目标 §10.2 的口径报告。

**个人项目的首个完整里程碑是 M1**：它应足以回答「显式依据维护是否值得」（研究目标 §12 末段）。M2 与 M3 是更具突破潜力的阶段，不能因原型变大而省略对照与独立验证（研究目标 §12 末段）。

## 6. 实验设计

本节只给**落点与执行纪律**：设计、口径、冻结流程以研究目标 §9–§10 为准，本文只引用章节号，不复述、不新增任何效应量或阈值数字。

### 6.1 主对照（2×2）— 研究目标 §9.1

「循环式控制与重规划 vs 依赖维护与结构精化」×「普通 LLM 结构化判断 vs Jev 判断」四组，共享生成模型、初始证据、环境、工具、权限、开发数据与可比预算；另设「强制提供相同新增证据」的诊断对照与一个强的原生 Agent 基线，均单独报告（研究目标 §9.1）。**每个阶段只比较当时已实现的机制**（研究目标 §9.1 末段）。实现落点：`jh-app` 的 `exp` 子命令。

### 6.2 关键消融 — 研究目标 §9.2

消融表逐条按研究目标 §9.2 执行，1.0 的排期落点如下（**只排期，不预设结果**）：

| 消融（研究目标 §9.2） | 检验 | 1.0 落点 |
| --- | --- | --- |
| 关闭依赖失效传播 | H1 | M1 |
| 每次全量重算 | H2 | M1（增量组与全量组使用相同输入投影规则，研究目标 §8.4） |
| 固定问题集合 | H3 | M2 |
| 只能使用已有观察工具 | H4 | M3 |
| 使用通用测试生成 | H4 | M3 |
| 禁止跨任务复用 | 累积优势 | M4 |
| 固定频率检查 | H5 | M4 |

### 6.3 任务族 A / B / C — 研究目标 §9.3

- **任务族 A**：会变化的故障环境（注入推翻前提的新证据、局部变化、过期结果、未列入候选集的原因）→ 服务 H1/H2，M1 起。
- **任务族 B**：现有问题无法区分的成对场景（如「证书过期 vs 时钟偏差」）；B1（已给关键证据）与 B2（需主动观察获取，观察须计费）**分别报告** → 服务 H3，M2 起。
- **任务族 C**：需要新增观测机制的并发/状态故障（如取消竞态），已有工具取不到证据 → 服务 H4，M3 起。
- 任务先在模拟与可控程序中完成，再考虑真实仓库迁移（研究目标 §9.3 末段；迁移属研究目标 §12 的 M5，不在 1.0）。
- 实现落点：`jh-app` 实验层（快照、扰动、隐藏验收、计价的工具动作）。

### 6.4 数据隔离与持续学习 — 研究目标 §9.5

按研究目标 §9.5 逐条执行：按故障机制/项目/任务族划分开发、验证、测试数据；开发集改系统、验证集选阈值与晋升规则；最终测试前冻结系统版本、初始问题库、分析脚本与主要指标；分开报告「冻结知识库的迁移测试」与「允许在线学习的任务序列测试」；学习产物跨组隔离。实现落点：`jh-app` 实验层与任务环境。

### 6.5 指标 — 研究目标 §10.1、§10.2

- 主要指标（真实任务成功率、旧依据行动率、恢复延迟、总研究执行成本、端到端延迟、未见任务上的精化收益、观测能力累积收益）按研究目标 §10.1 定义采集，**完成声明不构成证据**。
- 诊断指标（漏失效/误失效、重算量与缓存复用率、新问题的消费者与晋升/停用、观测器检出/误报、概率指标按问题族报告等）按研究目标 §10.2 采集；无法获得可信真值的指标必须标明人工标注或代理测量的局限。

### 6.6 统计冻结与结论等级 — 研究目标 §10.3、§10.4

先做小规模先导实验估计方差、成本与可行样本量，随后在独立最终测试前冻结主要指标、最小有意义效应、非劣界限、预算、停止条件、随机种子方案与分析方法（研究目标 §10.3）；结论只在「获得支持 / 条件性支持 / 不支持或被否定 / 证据不足」四档内表述（研究目标 §10.4）。**本文不指定上述数值**：具体数值由先导数据与项目资源确定（研究目标 §10.3 末段）。

### 6.7 RQ / H 的落点映射

| RQ / H | 阶段 | 对照或消融 | 任务族 | 主要读数（研究目标 §10） |
| --- | --- | --- | --- | --- |
| RQ0 / H0 | M0 | 判断器基线（研究目标 §9.1 四组共用的判断层） | 初始任务集 | §10.1 质量/延迟/成本；§10.2 概率指标 |
| RQ1 / H1 | M1 | 关闭依赖失效传播（研究目标 §9.2） | A（研究目标 §9.3） | §10.1 旧依据行动率、恢复延迟；§10.2 漏失效/误失效 |
| RQ2 / H2 | M1 | 每次全量重算（研究目标 §9.2） | A | §10.2 重算节点数/输入量/缓存复用率；§10.1 成本与质量 |
| RQ3 / H3 | M2 | 固定问题集合（研究目标 §9.2） | B（B1/B2 分开报告） | §10.1 未见任务上的精化收益；§10.2 新问题晋升/迁移 |
| RQ4 / H4 | M3 | 只能使用已有观察工具；使用通用测试生成（研究目标 §9.2） | C | §10.1 观测能力累积收益；§10.2 检出/误报/测量扰动 |
| RQ5 / H5 | M4 | 固定频率检查（研究目标 §9.2）+ 密度扫描（研究目标 §9.4） | A/B/C 汇总 | §10.1 质量—成本—延迟折中；结论按 §10.4 |

**归因规则**：若只有 Jev 调用更快，结论限定为经济收益；若架构在多个判断器上都有收益，归因于架构；若只有组合在特定预算下可行，报告交互作用与适用条件（研究目标 §10.4 末段）。

### 6.8 1.0 的实验纪律

不写未冻结的数字；不用刻意削弱的基线制造优势（研究目标 §9.1）；不在最终测试上继续调系统（研究目标 §9.4）；模拟器真值与延迟扫描只用于诊断、不冒充真实部署性能（研究目标 §9.2）；每项结论附原始数据、脚本与版本（[Jev 调研报告](jev-research.md) §14.3 前言的口径）。

### 6.9 不进主线实验的诊断项

- **E4 注入样本改为可选诊断项**：1.0 不含门控场景（决定 2），[Jev 调研报告](jev-research.md) §14.3 E4 的「门控漂移」口径在 1.0 失去对象，因此**不进主线实验清单**；保留为 **M4 可选诊断项**，口径改为「不受信内容进入判定 state 时，判定与概率的漂移」。若不安排，须在实验报告中写明理由，不静默消失（见 §12 未确定项 13）。

## 7. 交付物与完成定义

### 7.1 交付物（对照研究目标 §14.1 逐条处置）

| 研究目标 §14.1 | 交付物 | 1.0 处置 |
| --- | --- | --- |
| 1 | 研究目标文档及版本变更记录 | 已存在（`docs/RESEARCH_GOALS.md`），由后续版本维护，不在本路线图范围 |
| 2 | 核心对象、状态迁移、问题发布、观测器发布与行动复核的架构说明 | 本文件 §3–§4 为 1.0 版；实现落地后按实际回写 |
| 3 | 可批量运行的 Rust 实验核心及其 TUI | **核心在 1.0；TUI 推迟到 1.x**（决定 4） |
| 4 | 五种判断器的适配接口 | **M0 已交付**（`crates/jh-app/src/judge/`，见 §3.2） |
| 5 | 可快照、可扰动的任务环境、开发/验证/测试划分与隐藏验收 | M0 已交付最小一层（固定问题集、隐藏真值与校验、指标统计与 JSON 报告，见 §5 M0 落地状态）；快照与扰动、开发/验证/测试划分、隐藏验收在 M0–M3 补齐 |
| 6 | 主要对照、关键消融、统计分析与复现脚本 | M4 |
| 7 | 按版本保存的问题库、观测器库及其验证记录 | M2/M3；保存形态未定（§12） |
| 8 | 实验报告（效果、成本、适用边界、反例、失败原因、结论等级） | M4；写回 `docs/`（文件名未定，§12）。M0 的基线读数落在 `var/exp/<run>.json`（运行产物，不入库） |
| 9 | 一个完整演示：反例出现后新增区分维度或观测能力，并在未见任务上展示效果或失败 | M2 与 M3 联合，M4 呈现 |

### 7.2 完成定义

- **研究侧**：研究目标 §14.2 的六个问题在 1.0 已实现的机制范围内都有可核查答案，或明确标注为「证据不足」（研究目标 §10.4）；研究目标 §12 的 M5 外部有效性问题除外。负向结果与正向结果同为完成（研究目标 §3.2、§10.4）。
- **工程侧**：`cargo fmt` / `cargo clippy` / `cargo test` 全绿（[AGENTS.md](../AGENTS.md)）；`jev exp` 能跑完至少一个任务族并产出 JSON 报告；报告含原始数据位置与复现命令。
- **文档侧**：本文件的架构与运行时规格按实际实现回写；结论与未确定项同步到[项目笔记](project-notes.md)或研究目标的新版本（具体归属待人工确认，见 §13）。**M0 已经完成两轮回写（2026-09-22：实现回写 + 真调基线回写）**：§3.1 / §3.2 / §3.4 的命名与模块、§1.2 的 provider 与配置口径、§1.3 的现状表、§5 M0 的落地状态与复现命令、§12 未确定项 1 / 2 / 7 已由 M9 / M10 决定并写入；随后真调批次把未确定项 10 升级为「实测确认可用」，并在 §1.2 / §5 M0 写入 Jev / LLM 实测读数、两条新登记限制与 §12 #11 的现状更新（变更逐条见 §11）；M1 起每阶段合并在同一位置继续回写。

## 8. 明确不做（1.0 non-goals）

1. **TUI 研究视图**：`jh-tui` 冻结保留（决定 4）；研究目标 §11 的视图与操作推迟到 1.x。
2. **M5 真实仓库迁移**：外部有效性验证属后续版本（研究目标 §12 M5、§13.1、§9.3 末段）。
3. **多目标共享判断**：作为扩展方向保留（研究目标 §13.1）。
4. **能力编译**：技能编译方向（研究目标 §2.2 SIGIL）不在 1.0（研究目标 §13.1；对应其 §12 M5 的扩展研究）。
5. **插件系统 / MCP / 多智能体**：大规模分布式多 Agent 不属首阶段（研究目标 §13.1）。
6. **RAG / 向量库**：1.0 只用文本与结构化工具结果；长 state 的裁剪阈值作为风险登记（[Jev 调研报告](jev-research.md) §13.4 #10），不引入检索设施。
7. **工具沙箱**：1.0 的受控性来自隔离实验环境与任务协议（研究目标 §7.3），不做通用沙箱机制。
8. **多 provider 生态**：LLM 侧只走 `crates/jh-openai`（Responses API）。
9. **降级 / 容灾 / 重试编排**：[项目笔记](project-notes.md) 决策 2 已接受「无 SLA、单一供应商」风险，错误显式失败。
10. **模型权重训练 / 微调 / 分布式执行**：不属首阶段目标（研究目标 §13.1）。

新增内容一律进 1.x 待办，不进 1.0。

## 9. 风险与前置约束

| 风险 | 依据 | 1.0 对策 |
| --- | --- | --- |
| 中文 / CJK 表现未知（官方零量化数字） | [Jev 调研报告](jev-research.md) §4.3、§8.7、§13.4 #1 | M0 的判断器基线直接用本项目中文样本（口径见该报告 §14.3 E1）；结论不利则改为「英译后再判定」或英文 instructions |
| 校准未经独立验证（唯一 ECE 实测不利） | 同报告 §4.4、§13.4 #2 | 判定阈值一律来自本项目自测（同报告 §14.3 E2）；概率指标按研究目标 §10.2 分问题族报告，禁止照抄文档默认值 |
| 提示注入可移动判定 | 同报告 §7.8、§13.4 #3 | 不受信内容只作为证据数据，权限由运行时执行、评测真值与正常输入隔离（研究目标 §13.2）；涉及不可逆动作必须叠加确定性检查或人工确认（同报告 §13.5 红线 1）；注入类样本按 §6.9 作为 M4 可选诊断项处理（1.0 无门控主线） |
| 供应商版本漂移 | 同报告 §3.7、§5.1、§13.4 #4 | 请求 pin `jev-1.13.0`（`crates/jh-jev/src/client.rs:20`），判断记录保存响应 `model`；新版本需重跑基线（同报告 §14.3 E8） |
| 无 SLA、单一供应商、限流动态 | 同报告 §5.5、§11.2、§13.4 #6 | [项目笔记](project-notes.md) 决策 2：接受风险，1.0 **不实现**降级与容灾；失败必须显式可观测 |
| API key 泄漏 | 同报告 §11.3 | 只经环境变量（`TYPESAFE_API_KEY` / `OPENAI_API_KEY`），不入库、不进文档与日志 |
| 批处理可能改变判定（公开证据冲突） | 同报告 §3.6 vs §8.4 | 判断器抽象同时支持「一次多问」与「逐问」（同报告 §14.3 E3），按实测决定高代价判定是否禁用批处理 |
| 实验设施是最大工程量（任务环境 / 真值 / 隐藏验收大部仍缺） | 研究目标 §12 M0–M3；本文 §1.3 #6 | M0 已交付最小一层（固定问题集、隐藏真值与校验、指标统计与 JSON 报告，见 §5 M0 落地状态）；快照与扰动、开发/验证/测试划分、独立验收与计价工具动作在 M0–M3 补齐；无 Jev 访问时按研究目标 §12 的 M0 退路继续模拟 |
| 完成声明被自证 / 原目标被改写成代理目标 | 研究目标 §7.3、§13.2 失败模式表 | 验收由独立隐藏检查固定（研究目标 §7.3、§9.3）；Agent 完成声明不构成证据（研究目标 §10.1）；精化不得改目标或放宽验收（研究目标 §6.4） |
| 真值 / 评分标签泄漏进正常输入 | 研究目标 §13.2 | 隐藏真值与正常输入隔离；外部内容只作证据，不作权限来源 |
| 长 state 容量与 context rot | [Jev 调研报告](jev-research.md) §4.3、§13.4 #10 | 1.0 优先小规模受控任务；state 规模上限由实测曲线决定（未定，§12） |
| 范围蔓延 | 决定 1/4 + 本文 §8 | §8 为硬边界；新增内容进 1.x 待办 |
| backlog：`jh-openai` 端点拼接的尾斜杠（M11 finding，**复核后不成立**） | 本批复核 2026-09-22；`crates/jh-openai/src/client.rs:109`（拼接点）、`:169`（`ClientBuilder::build` 裁剪点） | 发现「不裁尾斜杠」与现状不符：`build` 早已 `trim_end_matches('/')`（自首个提交 `807e53b` 起存在，M11 的 HEAD `20d20ea` 也在），调用点看不到尾斜杠，`https://api.deepseek.com/` 与不带斜杠等价。**残余项降为「无单测覆盖」**：可补一条带尾斜杠的单测（与 `jh-jev` 的 `systemone_url` 单测同口径）。登记为 backlog，本批不改代码 |
| backlog：`jh-jev` 未建模 403 | M11 真实探测 2026-09-22；`crates/jh-jev/src/error.rs:4`、`:166-177` | 实测：完全缺 `Authorization` 头时服务端返 403，无效 key 才 401；403 现落入 `UnexpectedStatus`，报错丢失「鉴权」语义（正常路径总带 header，仅在代理 / 网关改写时出现）。登记为 backlog：修法是把 403 并入鉴权类错误并同步模块注释；本批不改代码 |

上表两条 backlog 来自 M11 探测（2026-09-22）与本批代码复核，只登记不在本批修；第 1 条经复核不成立，只保留「补单测」的建议。

## 10. 与既有文档的关系

| 文档 | 关系 |
| --- | --- |
| [研究目标](RESEARCH_GOALS.md) | **上游**：本文是研究目标 §12 的 M0–M4 施工路线与 §9–§10 的实验执行计划；研究目标 §5–§8 是运行时规格与对象清单的来源；本文不改该文档 |
| [项目笔记](project-notes.md) | Jev 定位与项目决策来源；本文直接引用决策 2（供应商/容灾）；与决策 3（判定权）的关系见 §13，决策 2 与调研报告红线 3 的冲突同见 §13 |
| [Jev 调研报告](jev-research.md) | 判断器定位依据 §13（§13.3 架构启示、§13.5 红线）；实测清单 §13.4 与实验目录 §14.3 是 M0–M4 的验证来源；本文不新增其结论与数字 |
| [Grok TUI 设计调研](grok-tui-design.md) | 1.x TUI 研究视图的参考（其 §7.3 最小骨架）；1.0 冻结 `jh-tui`，不引入其依赖栈。注意其 §7.3 推荐 ratatui 0.29 / crossterm 0.28，与工作区现有 0.30 / 0.29（`Cargo.toml`）不一致——以工作区为准，该文档不必改 |
| [AGENTS.md](../AGENTS.md) | 项目原则与工具链；§3.1 新增 `jh-core` 与其中「四个 crate」表述的差异见 §13 |
| `Cargo.toml` / `Cargo.lock` | 依赖登记位置与锁定基线（§1.3 #1）；本文不修改它们 |
| `docs/research/grok-tui/`、`reference/grok-build` | 1.x 参考素材，1.0 不引入 |

## 11. 本版变更记录

**2026-09-22 — 按研究目标重写为研究路线（本版）**

- **性质变更**：由「实施规划」改为「研究路线」；章节改为 前提与现状 / 命题与野心 / 架构与目录 / 运行时规格 / M0–M4 / 实验设计 / 交付物与完成定义 / 明确不做 / 风险与前置约束 / 文档关系 + 变更记录 + 未确定项。
- **移除**：旧版 §3 的四个决策点规格（Gate 门控 / Route 路由 / Boundary 边界 / Judge 评分）及其「决策即约束（Decision-as-Constraint）」表述与相关硬约束。判定出现在哪里，改由 §4 的运行时对象与 §6 的实验任务定义。
- **移除**：旧版的产品式里程碑表述（含 M2.5「E1 中文基线提前跑」、M4「TUI」、M5「实验与结论」），阶段改按研究目标 §12 的 M0–M4（§5）。
- **移除**：旧版 §1「1.0 的野心：可运行、可观察、可对比的最小系统」中的 Jev 约束层口径，改为研究目标的三个机制口径（§2.4）。
- **新增**：`crates/jh-core` 承载运行时对象模型（决定 3，§3.1、§4）。
- **新增**：RQ0–RQ5 / H0–H5 的实验落点映射（§6.7）、交付物与完成定义（§7）、未确定项（§12）、待人工裁决的交叉文档一致性项（§13）。
- **冻结**：`jh-tui` 组件套件（决定 4）；1.0 交付批处理实验 + CLI/JSON 报告，研究视图推迟 1.x。
- **保留**：workspace 与依赖清单现状、`Cargo.lock` 已入库、Jev key 并收紧为仅环境变量（旧版为「环境变量 / 本地 `.env`」）、pin `jev-1.13.0`、阈值来自自测、报告需含原始数据与复现命令；旧版 non-goals 的可复用条目并入 §8。

**2026-09-22 — M0 实现回写（M9 / M10 落地，M11 探测）**

- **回写实现命名**：§3.1 / §3.2 / §3.4 写入 `jh-core` 的实际模块与类型（M9，merge `20386b4`：`trait Judge`、`JudgeRequest` / `JudgeResponse`、`QuestionSpec`、`JudgmentRecord`、`JudgeSource` 等）与 `jh-app` 的五种判断器适配、实验层模块（M10，merge `22da7bb`，复核尖端 `c3e5583`）。
- **定下未确定项**：§12 #1（`jh-core` 模块与类型名）、#2（调用记录落盘形态：`var/calls.jsonl`，JSONL、`kind=judgment|failure`）、#7（`exp` 参数与报告字段）；另记 #5 的版本号规则现状（`QuestionVersion(u32)` 单调递增，问题库保存格式仍未定）。
- **升级未确定项**：§12 #10 与 §1.2 从「是否提供 `/responses` 未验证」改为「文档确认提供、实测未完成（无 key）」，并登记 LLM 侧配置口径（`OPENAI_BASE_URL` 必填、写法不带尾斜杠、模型 `deepseek-flash`、Responses 无状态）。
- **记录 M0 落地状态**：§5 M0 增加落地状态与解锁命令；§7.1 / §7.2 记录已交付的交付物与文档回写轮次。判断器接口、五种判断器、4 样本初始任务集、`jev exp` 基线报告均已交付；Jev / LLM 真调读数未取得，一律标「未验证」。
- **登记 backlog**：§9 新增两条——`jh-openai` 尾斜杠（复核后不成立，只保留「补单测」建议）与 `jh-jev` 未建模 403。
- **更新现状**：§1.3 #5–#8、§1.4 按 M9 / M10 的实际改动与当前测试基线（253 passed / 0 failed / 6 ignored，本批复核）更新。
- **为何**：§7.2「文档侧：本文件的架构与运行时规格按实际实现回写」；§3.4 上一版写明「Rust 侧的类型与模块命名在实现时随改动确定并在此文件回写」（§12 未确定项 1）。

**2026-09-22 — M0 真调基线回写（真调 Jev / LLM；本批不改代码）**

- **取得真调读数并回写**：§5 M0 的「未验证（本环境无 API key）」段替换为实测读数——Jev `exp` 4/4（一致率 9/12、Brier 0.3870、p50 353 ms、usage 3010/367，model `jev-1.13.0`）、LLM 通道非流式与 SSE 两用例通过、四判断器合并基线的 attempts 与 verification；同时明确「4 个合成样本、单日单次运行，只能作基线，不构成研究结论」，并把关键读数抄录进文档，因为 `var/` 运行产物不入库、teardown 后不保留（原始数据位置与复现命令同段给出）。
- **升级未确定项**：§1.2 与 §12 #10 从「文档确认提供、实测未完成」改为「实测确认可用」（非流式 + SSE，含具体命令）；§12 #11 更正为「基线已取得，但样本量不足，策略仍未定」。
- **登记两条 provider 限制**（本批只登记）：thinking 模式不支持强制 `tool_choice`（HTTP 400，错误体与已归档 finding 见 §1.2）；thinking 的 reasoning token 计入 `max_output_tokens`，LLM 结构化判断器写死 1024 导致 4/4 `Incomplete`、质量无读数（复现命令、读数与已归档 finding 见 §1.2 / §5 M0）。
- **更新现状表**：§1.3 #2 / #3 的「真实调用记录 0」「从未真跑」改为真调已通（含 LLM 通道 2 通 1 败的说明）。
- **为何**：§7.2 文档侧「按实际实现回写」；研究目标 §12 的 M0 退出证据要求「得到本环境的质量、延迟、成本基线，能区分真实 API 与模拟运行」。实测中出现的失败按纪律如实标「未验证」，不以模拟或回放结果顶替。

## 12. 未确定项

本版明确标注的事项：已由实现决定的条目写明决定与落点（2026-09-22 M0 批次回写，逐条见 §11）；仍未定的一并列出。

1. **已定（M9，2026-09-22）**：`jh-core` 的模块划分、类型名与 trait 签名——模块 `answer` / `error` / `judge` / `question` / `record` / `state`；trait `Judge`（返回装箱 future 以便 `Box<dyn Judge>`，不引入异步运行时）；请求 / 响应 `JudgeRequest` / `JudgeResponse`（后者带 `validate()`）；问题侧 `QuestionId` / `QuestionVersion` / `QuestionSpec` / `Questions` / `Question::{Choice,Score,Noul}`；答案侧 `Answer::{Choice,Score,Noul}` 与 `Usage`；记录 `JudgmentRecord` / `Judgment` / `JudgeSource`；错误 `Error::{Unavailable,CallFailed,InvalidResponse}`。逐项见 §3.4。
2. **已定（M10，2026-09-22）**：判断记录的持久化格式与落盘粒度——判断调用逐行追加进 `var/calls.jsonl`（JSONL，一行一次调用；`kind=judgment` 行内嵌完整 `JudgmentRecord`，`kind=failure` 行只有 run / case / source / latency_ms / error，不写空答案）；文件与 `var/` 由运行时创建、不入库（`.gitignore` 已列 `var/`）；基线报告单独落 `var/exp/<run>.json`（pretty JSON）。实现见 `crates/jh-app/src/calls.rs`、`src/main.rs:52-65`、`:108-114`。**事件日志仍属未定**（M1 对象，本次决定不覆盖）。
3. 「认识状态」是否作为独立字段（§4 第 9 行；研究目标 §5.3 只建议区分三维度）。**仍未定。**
4. 一等对象中 `Hypothesis` 与 `Capability` 在 1.0 是否落地（研究目标 §8.1 全表 vs 1.0 只做 §4 已列的运行时对象）。**仍未定。**
5. 问题库与观测器库的保存格式与版本号规则（研究目标 §14.1 #7）。**部分已定**：问题版本号在 1.0 取 `QuestionVersion(u32)` 单调递增（`crates/jh-core/src/question.rs:40-53`）；问题库 / 观测器库的保存格式仍未定。
6. 实验报告的文件名与放置位置（`docs/` 下）。**仍未定**；M0 的基线读数是运行产物 `var/exp/<run>.json`，不占 `docs/` 位置。
7. **已定（M10，2026-09-22）**：`exp` 子命令的参数形态与 JSON 报告字段——参数 `--judge <NAME>`（可重复；缺省 `rule` + `simulated-truth`）、`--calls <FILE>`（默认 `var/calls.jsonl`）、`--replay <FILE>`（默认 `var/calls.jsonl`）、`--out <FILE>`（缺省 `var/exp/<run>.json`）；报告**顶层字段**固定为 `run` / `generated_at_unix_ms` / `command` / `task_set` / `metric_definitions`（口径声明）/ `judges[]` / `calls_log`（绝对路径）/ `notes`，嵌套读数列**主要字段（非穷举）**：`task_set{name,source,cases,questions}`、`judges[].judge / source / live / versions`、`judges[].verification{status,detail}`、`judges[].calls{attempted,succeeded,failed,errors}`、`judges[].latency_ms{samples,min,p50,p95,max}`、`judges[].usage{calls_with_usage,input_tokens,output_tokens}`、`judges[].quality{judgments,agreements,agreement_rate,unscored,boundary_readings,brier_mean,log_loss_mean,log_loss_undefined,score_mae,score_mae_samples,per_question[]}`；完整字段定义以 `crates/jh-app/src/report.rs` 为准。报告不含价格折算（仓库无权威价格表）。实现见 `crates/jh-app/src/{exp,report,metrics}.rs`。
8. 任务族 A/B/C 的材料来源与规模（先导实验后定，研究目标 §10.3）。**仍未定**；M0 已用 4 样本最小合成集落地（`crates/jh-app/tasks/initial_tasks.json`）。
9. `jh-core` 是否引入 `tokio`，以及调度的具体实现形态（§3.3）。**仍未定**；M0 的 `jh-core` 只依赖 `serde` / `serde_json` / `thiserror`，无异步运行时。
10. **已定（2026-09-22 真调）**：所选 LLM provider 是否提供 `/responses`——**实测确认可用**：非流式与 SSE 两个真调用例通过（`deepseek-flash`、`Completed`、deltas 重建最终文本、usage 正常），端点可用（成功路径 2xx）、SSE 事件与响应体反序列化均吻合；命令与读数见 §1.2 / §5 M0（文档证据：DeepSeek 官方[《Using the Responses API》](https://api-docs.deepseek.com/guides/responses_api/)、Codex 集成页 `wire_api = "responses"`、定价页 `Responses API ✓`）。同时登记两条新限制（本批只登记）：thinking 模式不支持强制 `tool_choice`（HTTP 400 `invalid_request_error`）与 thinking 的 reasoning token 计入 `max_output_tokens`（LLM 判断器 1024 上限下 4/4 `Incomplete`）；两条均有已归档 finding，路径见 §1.2。配置口径见 §1.2。
11. 中文输入的处理策略（原文 / 英译 / 英文 instructions）——由 M0 基线结论决定。**仍未定**：M0 真调基线已取得（2026-09-22，Jev 4 样本；见 §5 M0），但 4 样本不足以支撑策略判断，也不构成研究结论。
12. 批处理与逐问在关键判定上的取舍——由实测决定（本文 §3.2；实验口径见 [Jev 调研报告](jev-research.md) §14.3 E3）。**仍未定**；接口层已同时支持两种表达（`Questions` 放 1 或 N 个问题）。
13. E4 注入诊断是否在 M4 安排；若不安排，须在实验报告中写明理由（见 §6.9）。**仍未定。**

## 13. 待人工裁决的交叉文档一致性项（只列出，本版不修改这些文件）

1. **[AGENTS.md](../AGENTS.md)** 技术栈要求写「workspace 分 `crates/jh-app`、`jh-jev`、`jh-openai`、`jh-tui` 四个 crate」；决定 3 新增 `crates/jh-core` → 需人工决定是否更新其目录布局描述。
2. **`Cargo.toml:5`** 的注释以 `docs/roadmap-1.0.md §2` 指代「依赖版本登记与 Cargo.lock 锁定」；本版重写后该锚点不再指向该内容 → 需人工决定改注释或改指向。
3. **[项目笔记](project-notes.md) 决策 3**「判定权：允许交给 Jev——包括不可逆动作的门控场景」以「门控」为对象；本版整体移除门控决策点后，该决策的落点变为「判定器可用于哪些实验路径」 → 需人工确认其表述范围。
4. **[项目笔记](project-notes.md)「Jev 的定位」小节**把「动作门控」列为循环内判定示例 → 与本版 §2.3 的表述需对齐（改或不改由人工决定）。
5. **[研究目标](RESEARCH_GOALS.md) 的 TUI 条目**：文首「配套 TUI 研究界面」、§11 的 TUI 研究操作、§12 的 M1「最小 TUI」与 M4「完整 TUI 展示」，以及 §8.3 组件分工与 §14.1 交付物中的 TUI；决定 4 的 1.0 不做 TUI（§5 与 §7.1 已显式声明偏差，依据该文档 §13.1）→ 需人工决定是否给该文档加「1.0 范围」标注或改写这些行。
6. **[研究目标](RESEARCH_GOALS.md) §12 的 M5 行**（真实仓库迁移等）在 1.0 之外，本版只在 §5 与 §8 声明 → 是否需在该文档中显式标注「1.0 止于 M4」，由人工裁决。
7. **[Jev 调研报告](jev-research.md) §14.3 E4（注入鲁棒性）与 §7.4（fail open / fail closed）**以「门控」为语境；本版不把门控作为路线图内容 → 这些材料保留为研究素材，是否需要在报告中重述其定位由人工决定。
8. **[Jev 调研报告](jev-research.md) §13.5 红线 1**（不可逆动作不能只靠 Jev 守门）与 **[项目笔记](project-notes.md) 决策 3**（允许交给 Jev）本就相反，项目已声明「项目决策优先于 §13 的默认建议」；本版不复述该冲突，但 1.0 的实验是否会触碰不可逆动作，需人工确认。
9. **[研究目标](RESEARCH_GOALS.md) §14.1 交付物清单**含 TUI 与研究文档版本维护；1.0 的实际处置见 §7.1 → 是否需在该文档中同步处置结果，由人工决定。
10. **[项目笔记](project-notes.md) 决策 2**（接受单一供应商，1.0 不实现降级与容灾）与 **[Jev 调研报告](jev-research.md) §13.5 红线 3**（不能把 Jev 当作唯一判定路径、必须保留降级实现）、**[研究目标](RESEARCH_GOALS.md) §13 失败模式表**（「API 不可用或模型版本变化 → 使用已声明的退路」）相抵 → 需人工裁决。可选处置（本版不替人工决定）：把规则 / 录制 / 模拟真值判断器声明为「已声明的退路」，或明确 1.0 不覆盖该行。
