# jev-harness 1.0 路线图：研究路线

> **性质**：施工路线——把[研究目标与实验设计](RESEARCH_GOALS.md)（下称「研究目标」）的 RQ/H 与 §12 阶段，落成 1.0 的 crate 边界、运行时规格与实验执行计划。
> **本文不产生研究结论**，也不新增阈值、效应量、工期或性能数字：所有实验设计与指标口径以研究目标 §9–§10 为准，本文只给落点与章节引用。
> **引用约定**：不带文档名的 `§x` 一律指本文件；引用其他文档时写明「研究目标 §x」或「[Jev 调研报告](jev-research.md) §x」。
>
> - 版本：1.0 研究路线（本版重写；2026-09-22 依次追加 M0 实现回写、M0 真调基线回写、M14 输出预算修复回写、M1 实现回写与 M2 实现回写；2026-09-23 追加 M3 实现回写，以及 **M4 实现回写 + TUI 决策变更**，见 §11）
> - 日期：2026-09-23（本版重写与 M0–M2 四批回写都在 2026-09-22）
> - 上游文档：[研究目标](RESEARCH_GOALS.md)、[Jev 调研报告](jev-research.md)、[项目笔记](project-notes.md)、[Grok TUI 设计调研](grok-tui-design.md)、[TUI 界面设计](tui-design.md)（2026-09-23 的决策变更后它是 1.0 的 TUI 设计基准）、[项目总纲](../AGENTS.md)
> - 代码基线：`crates/` 下现有五个 crate（`jh-core` 于 M0 落地、M15 补可失效语义状态与运行时对象、M18 补反例记录与问题库生命周期、M21 补观测器契约与失效联动、行为分类；`jh-app` 于 M16 补任务族 A 与 H1/H2 对照、M19 补候选生成 / 验证 / 影子 / 晋升回退、任务族 B 与 H3 对照、M22 补任务族 C 与 H4 三臂对照、M24 补 M4 主对照 / 诊断对照 / 原生基线 / 两条残余消融 / 密度扫描（`jev m4`）、M25 补 `jev chat` 会话宿主与 TUI 接线；`jh-tui` 的公共 API 由 M25 按 TUI 设计文档扩展），事实逐条见 §1.3
> - 研究目标 §12 的 M0–M4 是本文 §5 的直接来源；M5 不在 1.0 范围内（见 §5、§8）

## 1. 前提与现状

### 1.1 本版前置的四个已确认决定

| # | 决定 | 本文落点 |
| --- | --- | --- |
| 1 | 1.0 覆盖研究目标 §12 的 **M0–M4**；M5（真实仓库迁移、多目标共享、能力编译、跨域迁移）列为后续版本 | §5、§8 |
| 2 | 旧版的四个决策点（Gate / Route / Boundary / Judge）**整体移除**，不再作为路线图内容；**Jev 只以「可替换判断器之一」出现**（定位依据：[Jev 调研报告](jev-research.md) §13） | §2.3、§3.2、§4、§11 |
| 3 | 新增 `crates/jh-core` 承载运行时对象模型（事件/证据、判断记录、依赖图、失效传播、问题版本）；`jh-app` 继续做组装层与 CLI；`jh-jev` / `jh-openai` / `jh-tui` 既有公共 API 不动 | §3.1、§3.3 |
| 4 | ~~1.0 不做 TUI：只交付批处理实验 + CLI/JSON 报告~~ **已由用户指令变更（2026-09-23）**：TUI 加入 M4，1.0 交付批处理实验 + CLI/JSON 报告 **与 TUI 界面**（`jev chat` 交互会话 + TUI 主界面 + 三个研究面板接真实数据），原条作废 | §3.1、§5 首段（决策变更）、§7.1、§8、§13 |

### 1.2 不可动摇的前提

- **语言与形态**：Rust 全栈（[AGENTS.md](../AGENTS.md) 技术栈要求）；1.0 的可执行形态是批处理实验 + CLI/JSON 报告 **加 TUI 界面**（决定 4 已由 2026-09-23 的用户指令变更，见 §5 首段）。
- **LLM 通道**：OpenAI **Responses API** 为目标协议，实现见 `crates/jh-openai`（`crates/jh-openai/src/client.rs:65` `Client::create`、`:84` `Client::stream`）。模型必填、无内建默认（`:144` `ClientBuilder::model`）；key 来自环境变量 `OPENAI_API_KEY`（`:16`），base URL 可覆盖（`:139` `ClientBuilder::base_url`）。
  **provider 与配置口径（2026-09-22 回写）**：LLM 侧走 DeepSeek，模型 `deepseek-flash`；`OPENAI_API_KEY` / `OPENAI_MODEL` / `OPENAI_BASE_URL` 三者都必填（缺 `OPENAI_BASE_URL` 显式失败，避免 key 被发往 crate 自带的官方缺省端点；`crates/jh-app/src/judge/llm.rs:70-95`），base URL 写法为 `https://api.deepseek.com`（不带尾斜杠；`ClientBuilder::build` 同时会裁剪尾斜杠，端点即 `{base_url}/responses`，`crates/jh-openai/src/client.rs:169`、`:109`）。
  **`/responses`：实测确认可用（2026-09-22 真调）**。文档证据：DeepSeek 官方[《Using the Responses API》](https://api-docs.deepseek.com/guides/responses_api/)写明 “our API now supports the Responses API format, with the base_url being https://api.deepseek.com”，示例即 `client.responses.create(model="deepseek-flash", …)`；官方 Codex 集成页的 `wire_api = "responses"` 与定价页的 `Responses API ✓` 同向（M11 探测抓取）。**2026-09-22 本环境真调**：`OPENAI_API_KEY=$DEEPSEEK_API_KEY OPENAI_BASE_URL=https://api.deepseek.com OPENAI_MODEL=deepseek-flash cargo test -p jh-openai --test live_smoke -- --ignored --nocapture`——非流式 `live_create_returns_a_completed_response`（`Completed`、usage 38/15）与 SSE 流式 `live_stream_matches_the_final_text`（deltas 重建最终文本）均通过，端点可用（成功路径 2xx）、SSE 事件名与响应体反序列化在运行层面成立；读数与报告位置见 §5 M0。
  **新登记限制（2026-09-22 真调登记；第 2 条的判断器默认值已由 M14 处理，见正文）**：
  1. `deepseek-flash` 的 thinking 模式不支持强制 `tool_choice`：`live_stream_reports_a_forced_tool_call` 失败，错误体 `{"message":"Thinking mode does not support this tool_choice","type":"invalid_request_error","code":"invalid_request_error"}`（HTTP 400，见已归档 finding）。finding：`.tower/comms/findings/20260922-tower-bug-deepseek-deepseek-flash-thinking-tool-choice.md`。
  2. thinking 的 reasoning token 计入 `max_output_tokens`（**provider 事实，未变**）：原 LLM 结构化判断器写死 1024（`crates/jh-app/src/judge/llm.rs`），预算被 reasoning 耗尽，4/4 调用返回 `Incomplete`（`incomplete_details.reason=max_output_tokens`）、判断无读数。**M14（2026-09-22）已按实测把默认预算抬到 16384**（`crates/jh-app/src/judge/llm.rs:59`）：8192 仍会在 `case-003` 上被截断，16384 覆盖实测所需的 11425 token reasoning 且跑满预算约 74 s、仍在 120 s 超时之内；cap 取值依据、截断复现与修复后的真调读数见 §5 M0。原 finding：`.tower/comms/findings/20260922-agent-live-bug-llm-deepseek-flash-4-4-incomplete-1024-max-output-tokens-thi.md`（其「提高 1024 上限」的修复项已由 M14 落地）。
  **无状态约束**：DeepSeek 的 Responses 实现无状态（`store`、`previous_response_id` 不支持，未支持参数被静默忽略），因此每次判断请求自带全部材料，判定契约放进用户消息而不是可能被丢弃的 `instructions` 字段（`crates/jh-app/src/judge/llm.rs:18-26`）。
- **Jev 通道**：使用真实 API；key 只经环境变量（`TYPESAFE_API_KEY`，`crates/jh-jev/src/client.rs:69` `Client::from_env`），不入库、不进文档与日志；模型 pin `jev-1.13.0`（`crates/jh-jev/src/client.rs:20` `DEFAULT_MODEL`），请求显式带版本化 ID、不用别名（依据：[Jev 调研报告](jev-research.md) §5.1、§13.3.5）。
- **判定分层与概率优先**：能让确定性代码判断的不调模型，能让 Jev 判断的不调 LLM；`probabilities` / `confidence` / `noul` 原值保留，不提前塌缩成布尔值（[AGENTS.md](../AGENTS.md) 技术栈要求；[Jev 调研报告](jev-research.md) §13.3.3）。
- **项目性质**：个人学习项目、不商用；奥卡姆剃刀、禁止过度防御性编程、错误显式失败（[AGENTS.md](../AGENTS.md) 核心理念）。
- **工具链**：`cargo fmt` / `cargo clippy` / `cargo test` 为提交前标准检查（[AGENTS.md](../AGENTS.md) 技术栈要求）。

### 1.3 现状（2026-09-23 在工作区逐条核对代码，非转述；M0–M2 的核对在 2026-09-22）

| # | 能力 | 现状 | 证据（路径 / 符号） |
| --- | --- | --- | --- |
| 1 | workspace 与依赖锁定 | 已就位 | 根 `Cargo.toml`：`[workspace] members = ["crates/*"]`、`resolver = "2"`、21 项 `[workspace.dependencies]`（4 条内部 path 依赖 `jh-core` / `jh-jev` / `jh-openai` / `jh-tui`——`jh-tui` 由 M25 登记，17 项外部版本：`tokio 1.53`、`reqwest 0.13`、`serde 1.0`、`thiserror 2.0`、`clap 4.6`、`ratatui 0.30`、`crossterm 0.29`、`futures 0.3` 等；其中 `tracing-subscriber` / `dotenvy` / `toml` 目前无 crate 引用）；`Cargo.lock` 已跟踪入库（`git ls-files Cargo.lock` 命中） |
| 2 | Jev 客户端（jh-jev） | 已完成；**真调已通（2026-09-22）** | `crates/jh-jev/src/question.rs:119` `Question::{Choice,Score,Noul}`；`answer.rs:62` `Answer::{Choice,Score,Noul,Unknown}`；`answer.rs:147` `NoulAnswer`（无 `confidence` 字段，类型层面拦住误用）；`client.rs:108` `Client::system_one`（`POST {base_url}/v1/systemone`）；`error.rs:124` `Error`（401/422/429/529 各自变体，`:180` `request_id`、`:193` `retry_after`）；`fixtures/` 8 份官方示例请求/响应，**是官方文档抄件、不是真实录包**（`tests/fixtures.rs` 自述）；真调 smoke 在 `tests/smoke.rs` 标 `#[ignore]`；2026-09-22 真跑通过（model `jev-1.13.0`、request_id、usage 356/38），`exp --judge jev` 4/4 成功，读数见 §5 M0 |
| 3 | OpenAI 客户端（jh-openai） | 已完成；**非流式与 SSE 真调已通（2026-09-22）；强制 tool_choice 用例按 provider 限制失败** | `crates/jh-openai/src/client.rs:48` `Client::builder`、`:65` `create`、`:84` `stream`、`:218` `ResponseStream::next_event`；`sse.rs:18` `SseDecoder`（`:35` `push`，手写 SSE，含跨块多字节与 CRLF 处理）；`types.rs:497` `Event`（文档 59 个流事件中解码 20 个，其余落入 `Event::Other`）；`fixtures/` 7 份手工构造的离线样本（非录包）；`tests/live_smoke.rs` 3 个用例标 `#[ignore]`；2026-09-22 真跑 2 通 1 败（`deepseek-flash`、`Completed`、SSE deltas 重建文本；失败与 LLM 判断器读数见 §1.2 新登记限制与 §5 M0） |
| 4 | TUI 组件库（jh-tui） | **组件库已落地，并已由 `jev chat` 接线（M25；决策变更见 §5 首段）** | `crates/jh-tui/src/components/mod.rs:11-14` 导出回滚区 / 提示框 / 状态行与快捷键栏 / 审批卡；`app.rs:55` `App`、`actions.rs:22` `ActionId`、`render.rs:65` `RenderGate`、`terminal.rs:81` `TerminalGuard`、`blocks.rs:93` `BlockOutput`（块模型）；快照测试是 `TestBackend` 缓冲区上的内联多行断言（`tests/snapshots.rs` + `tests/common/mod.rs`），不是 insta、无 `.snap` 文件。注意 `RenderGate` 是**重绘闸门**，与旧版路线图的 Gate 决策点同名不同义。**M25 按 [TUI 界面设计](tui-design.md) 扩展了两屏与研究视图**（首屏 / 主界面 / 三个研究面板 / 研究焦点与键位），接线宿主落在 `crates/jh-app/src/session/**`（`model` / `tools` / `gate` / `approval` / `agent` / `view` / `tui`），`jh-tui` 仍不反向依赖 `jh-app` / `jh-core` |
| 5 | 组装层与 CLI（jh-app） | **M0 接 `exp`，M16 接 `run` / `compare`，M19 接 `h3`，M22 接 `h4`，M24 接 `m4`，M25 接 `chat`（真终端 TUI 会话）**；`ask` 仍为空壳 | `crates/jh-app/src/main.rs:50` 子命令枚举 `Command`（`chat` / `ask` / `run` / `exp` / `compare` / `h3` / `h4` / `m4`；`:52` `Chat(ChatArgs)`、`:54` `Ask`、`:66` `M4(M4Args)`）；`:265-267` `ask` 仍打印「未实现」并返回 `ExitCode::from(2)`；`:186` `RunArgs`（`--task` / `--judge` / `--arm` / `--calls` / `--out-dir`）、`:555` `run_task`（单任务匹配循环）；`:210` `CompareArgs`（`--judge` 可重复 / `--arm` / `--max-tasks` / `--calls` / `--replay` / `--out-dir` / `--out` / `--regenerate`）、`:726` `run_compare`（四组臂对照）；`:242` `ExpArgs`、`:834` `run_exp`；`:157` `H3Args`（`--judge` 可重复 / `--max-tasks` / `--candidates` / `--calls` / `--replay` / `--out-dir` / `--out`）、`:432` `run_h3`（M19）；`:135` `H4Args`（`--contracts` / `--tests` / `--max-schedules` / `--calls` / `--out-dir` / `--out`）、`:496` `run_h4`（M22）；`:70` `M4Args`、`:279` `run_m4`（M24）；`:113` `ChatArgs`、`:378` `run_chat`（M25）；本行的 `main.rs` 行号在 2026-09-23 按尖端重新量过一遍（M23 复核一轮，M24 / M25 又各插入一个子命令，旧行号整体下移）；实验层模块 `judge/`（五种适配器，`src/judge/{jev,llm,rule,replay,simulated}.rs`）、`tasks`、`calls`、`metrics`、`report`、`exp`、`family`、`matching`、`compare`，以及 M19 新增的 `generation` / `validation` / `shadow` / `promotion` / `family_b` / `h3`、M22 新增的 `c_program` / `c_measure` / `h4`、**M24 新增的 `m4`、M25 新增的 `session`**（`crates/jh-app/src/lib.rs` 的模块清单）已落地；依赖 `jh-core` / `jh-jev` / `jh-openai` / `tokio` 等，**M25 新增 `jh-tui` / `ratatui` / `crossterm` / `futures` 四个依赖**（`crates/jh-app/Cargo.toml`；M21 / M22 / M24 未新增依赖） |
| 6 | 运行时对象与实验设施 | **M0 + M15 + M16 + M18 + M19 + M21 + M22 已落地（各一层）** | **已有**：判断记录（`crates/jh-core/src/record.rs`）、问题定义与版本（`crates/jh-core/src/question.rs`，M15 增版本表 `QuestionRegistry`）、事件与证据日志（`crates/jh-core/src/evidence.rs`：追加写、不可变、撤回另记一笔，内存 / JSONL 两种持久化）、内容摘要哈希（`crates/jh-core/src/hash.rs`）、依赖图（`crates/jh-core/src/graph.rs`：读取 / 支持 / 冲突候选 / 行动前提四类边 + 环检测）、输入投影与快照校验（`crates/jh-core/src/projection.rs`）、可失效语义状态与行动前提复核（`crates/jh-core/src/runtime.rs`：`SemanticState` / `EvaluationStatus` / `Validity` / `check_action`）——以上 M15，merge `b099109`（复核尖端 `0e7ada2`）；任务族 A 与固定问题集（`crates/jh-app/src/family.rs` + `crates/jh-app/tasks/family_a.json`）、匹配循环基线（`crates/jh-app/src/matching.rs`）、H1/H2 四组臂与配对报告（`crates/jh-app/src/compare.rs`）、独立验收（`crates/jh-app/src/family.rs:1002` `accept`）——以上 M16，merge `a32b611`（复核尖端 `bc60233`）；调用记录落盘（`crates/jh-app/src/calls.rs`）、指标统计与 JSON 报告（`crates/jh-app/src/metrics.rs`、`src/report.rs`）（M0）；**M18 新增**（merge `9acb5dd`，复核尖端 `0c71380`）：反例日志（`crates/jh-core/src/counterexample.rs`：`Counterexample` / `NewCounterexample` / `CounterexampleId` / `FailureSource` / `Attribution` 四类候选归因 / `CounterexampleLog` 与 `MemoryCounterexampleLog` / `JsonlCounterexampleLog`）与问题库（`crates/jh-core/src/library.rs`：见 §3.4 的类型清单与版本号规则）；**M19 新增**（merge `84c3319`，复核尖端 `330fdf2`）：候选生成（`crates/jh-app/src/generation.rs`）、验证（`src/validation.rs`）、影子运行（`src/shadow.rs` + `src/matching.rs` 的 `ShadowPlan` / `BranchChange`）、晋升与回退（`src/promotion.rs`）、任务族 B（`src/family_b.rs` + `crates/jh-app/tasks/family_b.json`）、H3 对照（`src/h3.rs`）与离线端到端测试（`crates/jh-app/tests/h3.rs`）；**M21 新增**（merge `aad5fb1`，复核尖端 `219982a`）：观测器契约与行为分类（`crates/jh-core/src/observer.rs`：§7.2 八条逐项落类型、`ObserverLibrary` 的候选 → 验证 → 启用 → 停用 / 被替代、启用要求一份四路全过的独立检验记录 `ObserverCheck`；`src/behavior.rs`：`Behavior` 四类 + `Other` 出口）与失效联动（`src/runtime.rs:600` `invalidate_observer` + `InvalidationCause::ObserverInvalidated`）；**M22 新增**（merge `c9d9f16`，复核尖端 `083d334`）：隔离实验环境（`crates/jh-app/src/c_program.rs`：可控程序 + 三类钩子点 + 只读存储日志的隐藏检查 + 族 C 三个故障族 × 每族 10 条确定性调度）、声明式测量契约与四路独立检验（`src/c_measure.rs`）、H4 三臂对照与摊薄曲线（`src/h4.rs`）与离线端到端测试（`crates/jh-app/tests/h4.rs`）；**M24 新增**（merge `4dcd485`）：M4 综合评估的编排（`crates/jh-app/src/m4.rs`：2×2 主对照 / 诊断对照 / 原生基线 / 两条残余消融 / 密度扫描 / 族 C 判读与复用消融）与离线端到端测试 `crates/jh-app/tests/m4.rs`（8 个用例，另有 9 个模块内单元测试）；**M25 新增**（merge `bc66d18`）：`jev chat` 的会话宿主（`crates/jh-app/src/session/{model,tools,gate,approval,agent,view,tui}.rs`——LLM 主循环 + Jev 判定门控 + 审批卡 + 工具执行），产物落 `var/chat/<run>/evidence.jsonl`（实现口径与真调读数见 [TUI 界面设计](tui-design.md) §9.1）；**仍未建**：族 A 的开发 / 验证 / 测试划分（族 B 已在 M19 落地这一层：开发案例只用于反例提取、候选验证与晋升判据，对照读数只在未见变体上采集）；`var/` 由运行时创建、不入库（`.gitignore` 已列；无实验脚本，命令行 `jev exp` / `run` / `compare` / `h3` / `h4` / `m4` 承担） |
| 7 | 测试基线 | 全绿（2026-09-23 M26 复核） | `cargo test --workspace`：26 个 test target，**512 passed / 0 failed / 7 ignored**（出处 `docs/m4-report.md` §7 的本批复核，基线提交 `bc66d18`）；`crates/jh-app/tests/m4.rs` 是离线端到端、8 个用例全绿；`crates/jh-app/tests/h4.rs` 12 个用例全绿 |
| 8 | crate 内部模块划分 | jh-core / jh-app **已定并回写**（§3.4）；`jh-jev` / `jh-openai` 冻结不动，`jh-tui` 由 M25 按 [TUI 界面设计](tui-design.md) 扩展 | [AGENTS.md](../AGENTS.md) 的「各 crate 内部的模块划分仍待定」现只适用于未动的通道 crate；jh-core 与 jh-app 的实际模块与类型见 §3.4 |

### 1.4 与「研究目标」的差距

通道侧四个 crate 只提供了**外部边界**：两条模型通道（Jev / OpenAI）与一个 UI 组件套件；M0（M9 / M10）补上了 `crates/jh-core` 的判断器抽象与判断记录、`crates/jh-app` 的五种判断器适配、初始任务集与 `jev exp` 基线报告（落地状态见 §5 M0）。M15 补上研究目标 §5–§8 的运行时对象主干（事件与证据、依赖图、输入投影与快照校验、失效传播、行动前提复核，模块与类型见 §3.4），M16 补上实验设施的一层（任务族 A 的快照 / 脚本扰动 / 隐藏真值 / 独立验收 / 计价动作，加上匹配循环与 H1/H2 四组臂对照；落地状态与读数见 §5 M1），M18 补上慢回路的两个对象（反例记录与问题库生命周期：候选 → 影子评估 → 启用 → 停用 / 被替代，发布与回退都联动失效），M19 补上问题集合精化的一整层（候选生成 → 验证 → 影子运行 → 晋升 / 回退）与任务族 B、H3 两臂对照（落地状态与读数见 §5 M2），M21 补上观测能力建设在 `jh-core` 的一层（§7.2 八条契约的类型化、观测器库的候选 → 验证 → 启用 → 停用 / 被替代、启用要求四路全过的独立检验、观测器退出使用时的失效传播），M22 补上 `jh-app` 的隔离实验环境与任务族 C、三臂对照与 H4 读数（可控程序 + 三类钩子点 + 只读存储日志的隐藏检查、声明式测量契约的生成与独立检验、误放行 / 误报 / 迁移 / 维护 / 摊薄曲线；落地状态与读数见 §5 M3）。M24 补上 M4 综合评估的一层（`jh-app` 的 2×2 主对照 / 诊断对照 / 原生基线 / 两条残余消融 / 密度扫描与族 C 判读、复用消融编排；统计冻结与权威报告见 §5 M4 与 `docs/m4-report.md`），M25 补上 TUI 接线（`jev chat` 会话宿主 + 首屏 / 主界面 / 三个研究面板接真实数据，见 §5 M4 与 §7.1 #3）。**仍未建**：族 A 的开发 / 验证 / 测试划分（族 B 已落地这一层）；**§7.1 #9 的联合演示（同一次运行里既精化问题又建设观测器）M4 未交付**（见 §7.1 #9、§9）。1.0 的剩余工程量集中在 §7.1 未达成的条目与 §12 的未确定项；`jh-core` 侧按需补对象（观测器库的入库文件形态仍未定，见 §12 #5）。

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
- **工程形态**：批处理实验 + CLI/JSON 报告（决定 4）**加 TUI 界面**（2026-09-23 的决策变更，见 §5 首段）；实验侧可用 `jev exp` / `compare` / `h3` / `h4` / `m4` 复现，交互侧是 `jev chat`（`crates/jh-app`）。

## 3. 架构与目录

### 3.1 crate 布局（1.0 为五个）

| crate | 1.0 职责 | 状态 |
| --- | --- | --- |
| `jh-core` | 运行时对象模型：事件与证据、问题定义与版本、判断记录、依赖图与失效传播；判断器抽象；反例记录与问题库生命周期；观测能力建设的对象层（观测器契约与生命周期、行为分类）；调度与行动前提复核的规则 | **新增**；M0 已落地判断器抽象与判断记录（merge `20386b4`）；M15 已落地事件与证据日志、依赖图、输入投影与快照校验、可失效语义状态与行动前提复核（merge `b099109`，复核尖端 `0e7ada2`）；M18 已落地反例记录与问题库生命周期、发布 / 回退的失效联动（merge `9acb5dd`，复核尖端 `0c71380`）；M21 已落地观测器契约、独立检验的启用门槛、行为分类与观测器退出使用时的失效传播（merge `aad5fb1`，复核尖端 `219982a`）；模块与类型见 §3.4 |
| `jh-app` | 组装层与 CLI：`chat` / `ask` / `run` / `exp` / `compare` / `h3` / `h4` / `m4`；判断器适配（Jev / LLM / 规则 / 录制 / 模拟真值）；实验层（任务集、快照与扰动、隐藏真值、指标统计、JSON 报告）；问题集合精化（候选生成、验证、影子运行、晋升与回退）；观测能力建设的实验层（隔离实验环境与可控程序、隐藏检查、测量契约的生成与独立检验、三臂对照与成本核算）；M4 综合评估（主对照 / 诊断对照 / 原生基线 / 残余消融 / 密度扫描）；TUI 会话宿主（`session/**`：LLM 主循环 + Jev 门控 + 审批卡 + 工具执行） | 扩展；M0 已落地五种判断器适配、初始任务集与 `jev exp` 基线报告（merge `22da7bb`，复核尖端 `c3e5583`）；M16 已落地任务族 A、匹配循环基线与 H1/H2 四组臂对照（merge `a32b611`，复核尖端 `bc60233`）；M19 已落地候选生成 / 验证 / 影子 / 晋升回退、任务族 B 与 H3 两臂对照（merge `84c3319`，复核尖端 `330fdf2`）；M22 已落地隔离实验环境与任务族 C、测量契约与四路独立检验、H4 三臂对照与摊薄曲线（merge `c9d9f16`，复核尖端 `083d334`）；M24 已落地 M4 综合评估编排与 `jev m4`（merge `4dcd485`）；M25 已落地 `jev chat` 会话宿主与 TUI 接线（merge `bc66d18`，`jh-app` 随之新增 `jh-tui` / `ratatui` / `crossterm` / `futures` 四个依赖）；`ask` 仍为空壳 |
| `jh-jev` | Jev 通道：既有客户端原样使用 | 冻结（公共 API 不动） |
| `jh-openai` | OpenAI Responses 通道：生成模型与 LLM 结构化判断 | 冻结（公共 API 不动） |
| `jh-tui` | TUI 组件库：已落地，**并已由 `jev chat` 接线（M25；决策变更后 1.0 做 TUI，见 §5 首段）**；首屏 / 主界面 / 三个研究面板由 M25 按 [TUI 界面设计](tui-design.md) 落地 | 冻结旧的组件 API；M25 按设计文档扩展（`App` / `Effect` / `Screen` / 面板与 `KeyOwner::Research`），`jh-app` 侧新增依赖 |

1.0 目标目录（只列新增与冻结标记，其余沿用现状）：

```
jev-harness/
├── Cargo.toml                # workspace 与统一依赖清单（现状，见 §1.3 #1）
├── crates/
│   ├── jh-core/              # M0 落地：判断器抽象与判断记录；M15 落地：证据日志、依赖图、输入投影、失效传播；M18 落地：反例记录、问题库生命周期与发布/回退联动；M21 落地：观测器契约与生命周期、行为分类、观测器退出使用的失效传播（模块见 §3.4）
│   ├── jh-app/               # M0 落地：判断器适配、初始任务集、调用记录、`jev exp` 与报告；M16 落地：任务族 A、匹配循环、`jev run` / `jev compare`；M19 落地：候选生成、验证、影子、晋升回退、任务族 B、`jev h3`；M22 落地：隔离实验环境与任务族 C、声明式测量契约、`jev h4`；M24 落地：M4 综合评估编排（主对照 / 残余消融 / 密度扫描）、`jev m4`；M25 落地：`jev chat` 会话宿主（`src/session/**`）与 TUI 接线
│   ├── jh-jev/               # 冻结
│   ├── jh-openai/            # 冻结
│   └── jh-tui/               # 冻结的组件库；M25 已接线（会话 + 研究视图）
├── docs/                     # 本路线图、研究目标、实验报告（M4 定案：`docs/m4-report.md` + 冻结记录 `docs/m4-freeze.md`，见 §12 #6）
└── var/                      # 运行产物：调用记录、实验报告、对照报告与逐任务轨迹证据（`var/calls.jsonl`、`var/exp/<run>.json`、`var/compare/<run>.json` + `var/compare/<run>/{traces,evidence}/`、`var/h3/<run>.json` + `var/h3/<run>/{traces,shadow-traces,shadow-evidence}/`、`var/h4/<run>.json` + `var/h4/<run>/{contracts,tests,tests-v2,verification}.json` 与 `{runs,runs-v2,transfer}.jsonl`、M4 定稿的 `var/m4/final/<块>.json` + `var/m4/final/<块>/{traces,evidence,channel-cases,reuse}/` 与 `var/m4/final/c1a/contracts.json`、会话的 `var/chat/<run>/evidence.jsonl`；.gitignore 已列，运行时创建）
```

### 3.2 判断器抽象

- **接口面（已定，M9 回写）**：`state + 类型化 questions → 类型化 answers + 概率/置信度 + usage 与显式错误`（[Jev 调研报告](jev-research.md) §13.3.1）；Rust 侧即 `jh_core::Judge`（`crates/jh-core/src/judge.rs`）：`JudgeRequest { snapshot, state, questions }` → `JudgeResponse { source, version, answers, usage, request_id }`，接口返回装箱 future 以便放进 `Box<dyn Judge>`（crate 不引入异步运行时）；一次请求放 1 个还是 N 个问题由调用方决定（`Questions` 即 `QuestionId → QuestionSpec` 的映射）；错误只有 `Unavailable` / `CallFailed` / `InvalidResponse` 三档，不重试、不降级、不兜底。
- **实现与归属（M10 落地）**：五种适配器同住 `crates/jh-app/src/judge/`——Jev → `jev.rs`（调 `jh_jev::Client::system_one`，`version` 取响应的 `model`、`request_id` 取 `x-typesafe-request-id`）、LLM 结构化判断 → `llm.rs`（调 `jh_openai` 的 `Client::create`，自带 JSON 输出契约）、规则 → `rule.rs`、录制回放 → `replay.rs`、模拟真值 → `simulated.rs`；`JudgeName` + `build()` 按名字构造，构造失败只标「未验证」，不在实现内部换后端（研究目标 §8.3）。
- **复用条件**：结果只在输入快照、问题含义、模型版本与适用范围全部匹配时可复用（研究目标 §8.4）。
- **发起方式**：同一快照上的兼容问题可批量发送，有依赖的阶段按顺序执行；1.0 必须能同时表达「一次多问」与「逐问」，因为批处理是否会改变判定尚未解决（[Jev 调研报告](jev-research.md) §3.6 与 §8.4 冲突未解，实测见其 §14.3 E3；研究目标 §8.4）。

### 3.3 依赖与构建约束

- 各 crate 一律 `dep = { workspace = true }` 引用根清单，不写死版本（避免版本分叉）；`jh-core` **不新增第三方依赖**——**已定（2026-09-22，M15，§12 未确定项 9 结案）**：实际只用已登记的 `serde` / `serde_json` / `thiserror`（`crates/jh-core/Cargo.toml`），**不引入 `tokio`**。理由：判断器接口返回装箱 future，由调用方的 executor 驱动；运行时对象（证据日志、依赖图、失效传播、行动前提复核）本身是同步的，没有需要异步驱动的 I/O。调度形态落在 `jh-app` 的匹配循环（`crates/jh-app/src/matching.rs`），`tokio` 只被 `jh-app` 的 CLI（`src/main.rs` 的 `#[tokio::main]`）用来跑 async 判定器。
- **不引入**图数据库、复杂 DSL 或独立服务：研究目标 §8.1 明确初版可用简单持久化与邻接表。
- **依赖方向**：`jh-core` 不依赖 `jh-jev` / `jh-openai`（否则对象模型绑死在某个判断器上）；通道专属的 wire 类型由 `jh-app` 的适配层转换（避免让通道 crate 变厚，也满足决定 3 的「公共 API 不动」）。

### 3.4 模块划分

**已回写（2026-09-22，M9 / M10 落地）**：

- `crates/jh-core/src/`：`answer`（`Answer::{Choice,Score,Noul}`、`Usage`）、`error`（`Error::{Unavailable,CallFailed,InvalidResponse}`）、`judge`（`Judge`、`JudgeRequest`、`JudgeResponse` 与 `validate`）、`question`（`QuestionId`、`QuestionVersion(u32)`、`QuestionSpec`、`Questions`、`Question::{Choice,Score,Noul}`）、`record`（`JudgmentRecord`、`Judgment`、`JudgeSource::{RealApi,Llm,Rule,Replay,SimulatedTruth}`）、`state`（`State`、`StateEntry`、`SnapshotId`）。
- `crates/jh-app/src/`：`judge/`（`mod.rs` 的 `JudgeName` / `build()` 与 `jev` / `llm` / `rule` / `replay` / `simulated` 五种适配器）、`tasks`（任务集、隐藏真值与校验）、`calls`（JSONL 调用记录与回放索引）、`metrics`（一致读法 / Brier / 对数损失 / 分位数）、`report`（报告结构与口径声明）、`exp`（运行编排）、`error`。

**已回写（2026-09-22，M15 / M16 落地；逐条对照代码，非转述）**：

- `crates/jh-core/src/` 新增五个模块（M15，merge `b099109`，复核尖端 `0e7ada2`）：
  - `evidence`：`Evidence` / `NewEvidence` / `EvidenceId` / `ObjectId` / `ObjectVersion` 与 `EvidenceLog`（追加写、不可变；撤回是另外追加一条标记，不改写记录），实现只有 `MemoryEvidenceLog` 与 `JsonlEvidenceLog` 两种。
  - `hash`：`ContentHash`（FNV-1a 64 位；非加密用途，只做变更检测与审计标识——标准库 `DefaultHasher` 跨版本不稳定，所以不用）。
  - `graph`：`DependencyGraph` 与四类边 `EdgeKind::{Read,Support,Premise,Conflict}`、边来源 `EdgeOrigin::{Deterministic,Proposed}`（确定性读取依赖与模型提出的语义关联分开记）、`NodeRef` / `NodeId` / `ActionId` / `Edge` / `PropagationPath`；`cycles()` 报依赖环，冲突候选不参与传播与环检测。
  - `projection`：`InputProjection` / `ObjectRead` / `FieldSelection` / `Drift` / `SnapshotCheck` 与 `verify_snapshot`。共享一份请求 state 的整批问题登记同一份投影（`InputProjection::covering`），宁大勿小；字段粒度只记录、不参与失效筛选；投影里的版本只用于登记时的快照校验。
  - `runtime`：`SemanticState`（`declare_node` / `declare_batch` / `record_judgment` / `set_evaluation` / `append_evidence` / `retract_evidence` / `retract_judgment` / `recheck_all` / `recheck_set` / `declare_action` / `check_action` / `withdraw_action` / `link` / `cycles`）、判断节点 `JudgmentNode`、两个正交维度 `EvaluationStatus::{NotRequested,Queued,Running,Succeeded,Failed}` 与 `Validity::{Current,NeedsRecheck,Retracted}`、`Action` / `PremiseCheck::{Cleared,Blocked,Withdrawn}` / `Invalidation` / `InvalidationCause`。失效传播按**对象**保守判定（不猜模型看了对象的哪一版记录），依赖图是唯一依据；撤销只改变使用资格，已发生的外部副作用不自动回滚。
  - 既有模块的增量：`question` 增版本表 `QuestionRegistry`（`register` / `revise` / `at` / `current` / `history` / `compare`）与 `VersionComparison::{Same,Changed}`；`error` 增运行时变体 `Storage` / `UnknownEvidence` / `UnknownNode` / `UnknownAction` / `RetractedNode` / `QuestionVersionConflict`。
- `crates/jh-app/src/` 新增三个模块与一份任务集（M16，merge `a32b611`，复核尖端 `bc60233`）：
  - `family`：任务族 A 的加载与校验（`crates/jh-app/tasks/family_a.json`，6 个任务 × 3 轮）、相位模型（相位 0 = 初始快照，每应用一条扰动前进一相；判断落在提问相位、行动落在该轮全部扰动之后的决策相位）、固定问题集与 `FamilyA::scope`（问题声明的读取范围只有一个真身）、环境脚本 `Environment::{seed,apply_round}`、独立验收 `accept`（只读证据日志与隐藏真值）。
  - `matching`：匹配循环基线——一轮的固定顺序是「注入提问前扰动 → 按本臂调度发请求 → 注入提问后扰动 → 落判断记录（迟到结果由快照校验标过期）→ 决定行动 → 写完成声明」；`Arm` 的两个开关 `uses_propagation()` / `recompute()` 是 H1/H2 的唯一分叉点（`Recompute::{Incremental,Full}`）；调度规则是「按问题声明的读取范围分组、一组一次请求」，增量臂只问「从没答上来的问题 + 依据已失效的问题」，全量臂每轮重问全部问题；轨迹类型 `TaskTrace` / `AskTrace` / `JudgmentTrace` / `DecisionTrace` / `ReuseTrace`。
  - `compare`：四组臂（`h1on-h2inc` / `h1off-h2inc` / `h1on-h2full` / `h1off-h2full`）× 全部任务，汇成 JSON 报告；`PairingReading` 把同一口径内的两条臂并排，`CompareMetricDefinitions::declared` 逐条写明口径；`regenerate` 支持只从轨迹重建报告（判定器一次都不调用）。
- **问题集与版本规则（§4 第 2 行的实现）**：`crates/jh-app/tasks/family_a.json` 是本版的问题库（入库、带版本），当前登记 5 条 = 4 个问题 ID：`cause_primary` v1（只读 `app.log`，已非当前版本）与 v2（读 `app.log` + `config`，当前版本）、`premise_holds` v1（`app.log` + `config`）、`metrics_within_baseline` v1（`runtime.metrics` + `config`）、`deliverable_ready` v1（`fix-report.md` + `app.log`）。规则由 `QuestionRegistry` 强制：同一 ID + 同一版本上想换成别的含义 → `Error::QuestionVersionConflict`；改含义必须走 `revise`（取当前最大版本 +1）；重复登记同一份定义是幂等的（便于回放）。问题升版后旧判断如何处置属 M2 精化流程——M18 起由 `QuestionLibrary` 的生命周期与 `SemanticState::invalidate_question` 的发布 / 回退联动承担（见本节下一块与 §5 M2），本模块只保证版本不被覆盖、可查可比较；任务族 A 里 `cause_primary` 的 v1 → v2 就是这条规则的一次实际使用。
- 冻结的 `jh-jev` / `jh-openai` / `jh-tui` 模块划分不动；[AGENTS.md](../AGENTS.md) 的「各 crate 内部的模块划分仍待定」现只适用于这些通道 crate。

**已回写（2026-09-22，M18 / M19 落地；逐条对照代码，非转述）**：

- `crates/jh-core/src/` 新增两个模块（M18，merge `9acb5dd`，复核尖端 `0c71380`）：
  - `counterexample`：`Counterexample`（一次失败的原始现场：失败来源、当时的证据、问题版本、候选动作与控制规则）、`NewCounterexample`、`CounterexampleId`（由日志按追加顺序分配，从 1 起）、失败来源 `FailureSource::{VerificationFailed,ExpectedEffectMissed,ContradictoryEvidence}`、§6.1 的四类候选归因 `Attribution`（可多选、可留空，只记录不判定）、日志 `CounterexampleLog` 与两种持久化 `MemoryCounterexampleLog` / `JsonlCounterexampleLog`（追加写、无撤回动作，失败候选与已处理的反例都留在日志里）。反例与候选问题之间不强制存边，溯源由生成步骤自己记。
  - `library`：问题库——`NewQuestion`（§6.3 的前六项：命题与类型、区分目的、证据需求、适用范围、触发条件、下游用途）、`QuestionEntry`（除命题与类型外的各项 + 验证记录 + 生命周期）、`Lifecycle::{Candidate,Shadow,Enabled,Retired,Superseded{by}}`、`LifecycleRecord`（追加写的状态变更史）、`ValidationRecord` / `ValidationKind::{Development,ShadowRun,UnseenVariant}`，以及 `QuestionLibrary::{propose,adopt,start_shadow,record_validation,publish,retire,enabled,versions,history}`。`propose` / `adopt` 在消费者为空时显式失败（`Error::QuestionWithoutConsumer`）；没经过影子评估的候选不能直接晋升、停用过的版本不能再启用（`Error::InvalidLifecycle`）；`adopt` / `publish` / `retire` 都收 `&mut SemanticState` 并调 `invalidate_question`（`crates/jh-core/src/runtime.rs:511`），把回答过该问题的判断标成待重查、以它们为前提的行动标成不能发出，并原样返回这次传播。
- **版本号规则（定案，§12 未确定项 5）**：① 版本号是**每个问题 ID 各自从 1 开始的单调递增整数**（`QuestionVersion(u32)`），由库分配（取该 ID 当前最大版本 +1，`QuestionRegistry::revise`），调用方不写死；② 同一 ID + 同一版本不得换含义（改含义必须拿新编号）；③ 停用与回退**不产生新版本号**——回退是把被替代的旧版本重新设为当前版本；④ 旧版本永不被覆盖或删除，只改生命周期状态（这是「可比较、可回退」的全部保证；§6.4 的「删除冗余项」在 1.0 由停用承担，合并算法不在 1.0）；⑤ 入库的问题库文件保存的正是这份编号与状态，后续分配从文件里的最大版本 +1 继续。
- `crates/jh-app/src/` 新增六个模块、一份任务集与一个端到端测试（M19，merge `84c3319`，复核尖端 `330fdf2`）：
  - `generation`（`:44` `CandidateQuestion`、`:216` `check_contract`）：真实生成模型按「开发案例的失败现场 + 反例记录 + 可观测清单」产出候选；八字段缺一项、占位缺失、`lifecycle != candidate`、`consumers` / `evidence` 为空都显式失败；`refines` 必须是被门控的那个问题 ID、选项标签集合必须原样保留（改标签就是改控制规则）、证据对象必须落在声明清单里且含该对的关键证据。提示词里只放开发案例的失败现场，不放未见变体的隐藏真值（研究目标 §13.2）。
  - `validation`（`:36` `CaseKind::{Positive,Negative,Unseen}`、`:164` `run`）：每个用例给一份材料与一个正确读法，候选（某一版本）判一次；材料按**被验证版本声明的读取范围**裁剪（固定版本读不到关键证据，正例必错）；读数（一致率 / Brier / 对数损失 / 延迟 / token）按原值进报告与库里的验证记录。
  - `shadow`（`:27` `plan`、`:45` `ShadowTaskReading`）：影子判断落在**独立状态**里，控制路径只读主状态（结构上隔离）；每条影子判断对着本轮实际决策按**同一条门判据**重算「若采纳会不会改变行动分支」（`crates/jh-app/src/matching.rs:436` `BranchChange`），方向按独立验收的正当性口径分成「走向正当动作」与「走向不该发出的动作」；影子成本单独记账。
  - `promotion`（`:62` `DECLARED_CRITERIA` 8 条、`:113` `declared_thresholds`、`:290` `BoundaryDigest`、`:466` `four_way_change`）：晋升标准与阈值**跑之前写死**，没达标就不发布；发布走 `QuestionLibrary::publish`，回退是同一条路（把被替代的旧版本重新设为启用），两者都联动失效；每次晋升记「问题 / 输入投影 / 消费者 / 策略」四路变更，并逐项核边界（用户目标、验收要求、工具权限、控制规则）在发布前后摘要一致。
  - `family_b`（`:45` `FAMILY_B_JSON` 编译期内嵌、`:51` `ABSENT_LABEL = "other"`、`Pair` / `Observable` / `ValidationCase` / `FamilyB` / `FamilyBQuestions`）：任务族 B 的加载与校验（`crates/jh-app/tasks/family_b.json`：3 对 × 每对 1 个开发案例 + 2 个未见变体，每对一 B1 一 B2，**每个任务 3 轮**（`rounds: 3`；同文件的 `steps` 是扰动脚本、`phases` 是隐藏真值的分相，都不是轮数））；问题的 `other` 判据写进定义（「需要的那份证据不在这次读到的材料里」）；材料充分性由 `MaterialGate`（`crates/jh-app/src/family.rs:182`）声明；B2 的关键证据由计价观察动作产出（`crates/jh-app/src/family.rs:163` `ActionEffect`）。
  - `h3`（`:64` `H3Options`、`:85` `CandidateSource::{Generate,File}`、`:103` `H3Report`、`:633` `run`）：一条命令跑完「开发失败 → 反例 → 候选 → 验证 → 影子 → 晋升 → 两臂对照 → JSON 报告」；两臂共用同一段循环、同一套前提门与同一批任务 / 初始证据 / 判定器配置 / 决策时机，差别只有问题库当前启用的版本；`--candidates <file>` 复用已落盘的候选产物（不重发生成调用）。
  - 既有模块的增量：`family` 增 `MaterialGate` 与 `ActionEffect`；`matching` 增 `ShadowPlan` / `ShadowQuestion` / `BranchChange`；`calls` 增 `CallEntry::ModelCall`（`:53`，生成 / 验证 / 影子这些**非判断**模型调用的记账：用途标签、token 用量、输入输出字节数、延迟与失败）与 `run()` / `case()` 的对应分支；`compare` 增 `logged_calls_of_run`（按 `{run}-` 前缀统计本次运行的调用数，用于成本账与收益扣减；唯一定义在 `crates/jh-app/src/compare.rs:1429`，调用点是 `crates/jh-app/src/h3.rs:2365`）；`judge/llm` 抽出 `from_env_client` / `complete`，判断与生成共用同一套通道口径（模型、端点、缺配置即显式失败）；`main.rs` 增 `h3` 子命令。冻结的 `jh-jev` / `jh-openai` / `jh-tui` 一字未动。
**已回写（2026-09-23，M21 / M22 落地；逐条对照代码，非转述）**：

- `crates/jh-core/src/` 新增两个模块（M21，merge `aad5fb1`，复核尖端 `219982a`）：
  - `observer`：观测器契约、独立检验与生命周期。§7.2 的八条逐项落成字段（`ObserverContract`：`requirement`（`Requirement`）/ `proposition` / `coverage` / `measurement`（`Measurement` + `Timing`）/ `support`（`Support`）/ `disturbance`（`Disturbance`）/ `controls`（`Controls`）/ `output`（`OutputContract`）/ `uncovered`），字段全必填、内容不校验（唯一的结构要求是测量对象非空，否则 `Error::ObserverWithoutObject`）；独立检验落成 `ObserverCheck` 的四路（`CheckDirection::{Positive,Negative,InjectedFault,Hidden}`，每路同时记「期望观测器给出什么」与「实际观测到什么」，读数类型是 `ControlResult` / `ObserverVerdict`）；库是 `ObserverLibrary`（`propose` → `start_verifying` → `enable` → `retire`，`ObserverLifecycle::{Candidate,Verifying,Enabled,Retired,Superseded{by}}`、追加写的 `ObserverLifecycleRecord`、`ObserverEntry` 带契约 + 检验记录 + 生命周期），**启用要求一份四路全过的检验记录**：没通过就 `Error::ObserverNotVerified` 显式拒绝、生命周期停在原状，失败的那条记录留在库里；其余非法迁移是 `Error::InvalidObserverLifecycle`（候选不能直接启用、停用过的版本不再启用）。版本号（**定案**，§12 未确定项 5 的观测器一侧）：`ObserverVersion(u32)`，每个观测器 ID 各自从 1 起单调递增、由库分配；同一 ID + 同一版本不得换含义；停用与回退不产生新版本号；旧版本永不覆盖或删除、只改状态。一个 ID 任何时刻只有一个启用版本（`ObserverLibrary::enabled`）。
  - `behavior`：§7.1 的四类行为 `Behavior::{Execute,Observe,BuildObserver,Deliberate}` 加出口 `Behavior::Other(String)`（分类不封闭；`Display` 给中文标签，JSON 走 `snake_case` 加 `behavior` / `detail` 标签）。**在本 crate 里它只是标签**：模块文档写明四类各自的落点是 `Action` / `check_action`（执行）、`append_evidence`（观测）、`ObserverLibrary`（建设观测器）、`Judge`（开放式推理或生成），调度与记账由消费方做。
  - 既有模块的增量：`runtime` 增 `SemanticState::invalidate_observer`（`crates/jh-core/src/runtime.rs:600`）与 `InvalidationCause::ObserverInvalidated{observer,version}`——观测器停用或被新版本替代时，读过它测量对象（`Measurement::objects`）的判断标待重查、以它们为前提的行动标不能发出，**按对象判定、不按渠道判定**（刻意保守，与证据传播同口径），无关结果原样保留，每个受影响目标带一条完整传播路径；一次替代多个版本时逐版本传播再合并。`lib.rs` 把 19 个新公共类型全部导出。
- `crates/jh-app/src/` 新增三个模块与一个离线端到端测试（M22，merge `c9d9f16`，复核尖端 `083d334`）：
  - `c_program`（`:46` `RECORDER_CHANNEL`、`:50` `JOURNAL_CHANNEL`、`:60` `HookPoint`、`:120` `CFamily`、`:222` `Op`、`:317` `Schedule`、`:331` `Implementation`、`:385` `ProgramVersion`、`:523` `Attachment`、`:1082` `RecorderLog`、`:1119` `PauseTrace`、`:1222` `hidden_check`）：隔离实验环境——**可控程序**是一段确定性的指令解释器（`Op` + `Schedule`，没有随机数），三个故障族 `CFamily::{CancelAfterConfirm,LostUpdate,DuplicateAfterRestart}` 各带 10 条确定性调度、4 路实现（`Implementation::{Correct,Uncoordinated,QuietBypass,TamperedRecorder}`）与只从存储日志算得的隐藏判据；程序版本 `ProgramVersion::{V1,V2}`（v2 新增批量提交路径 `commit_batch`），挂载形态由 `Attachment` 描述（三个钩子点 + `direct_covered`：版本变更之后**重挂**才把新增的落地点纳入观测）。**三条证据通道**（本模块与测量层的结构分界）：写入边界记录器 `RecorderLog`（通道 `observer:c-write-recorder`，宿主钩子写，**程序能绕过或篡改**）、暂停点轨迹 `PauseTrace`（`host:c-pause-points`，宿主控制面写、程序碰不到）、存储日志 `CacheStore::journal`（`hidden:c-effect-journal`，**只有隐藏检查读**）。三类**宿主钩子点** `HookPoint::{WriteBoundary,PreWritePause,CommitCounter}` 是契约里 `measurement.site` 唯一允许引用的词（`:104` `HookPoint::parse`）。
  - `c_measure`（`:54` `BANNED_SUBSTITUTIONS`、`:272` `check_contract`、`:577` `Observer`、`:777` `independent_check`、`:990` `enable_with_independent_check`、`:1185` `Check`、`:1451` `FixedDeclaration`、`:1590` `hidden_verdict`）：**声明式测量契约**由真实生成模型产出再逐项核对（越界钩子点、缺字段、把要求换成更弱的命题都显式失败——被禁的替换 `BANNED_SUBSTITUTIONS` 是代码里的常量，固定测试臂 C1 的检查项故意用其中一条并标 `weakened_substitution`）；**观测器**按契约选中的钩子点装通道（只读记录器与暂停点，**拿不到存储日志**），给三档判定（命题成立 / 发现违反 / 观测失败）；**四路独立检验**跑出真实读数、装进 `ObserverCheck` 交给 `ObserverLibrary::enable`，并额外跑一遍「自证式的伪隐藏检查会被门槛拒绝」；**通用测试生成臂**的断言编译进封闭枚举 `Check` 并过一遍越界词扫描（引用钩子点 / 记录器 / 存储日志直接失败）。
  - `h4`（`:63-77` 八个臂标识、`:95` `H4Options`、`:123` `H4Report`、`:394` `TransferCell`、`:492` `MaintenanceSection`、`:587` `AmortizationPoint`、`:618` `HypothesisSection`、`:743` `CostSection`、`:829` `run`）：H4 三臂对照的编排与读数——固定测试 / 通用测试生成 / 建设观测器；每个用例只跑一次环境、三臂各出一个判定，**机会数与真值全部来自隐藏检查**；外加迁移矩阵、版本变更后的五种维护形态、摊薄曲线与跑前冻结的判据。
  - 既有模块的增量：`main.rs` 增 `h4` 子命令（`--contracts` / `--tests` / `--max-schedules` / `--calls` / `--out-dir` / `--out`）；`calls` 的 `CallEntry::ModelCall` 承担**生成**调用的记账（用途标签 `contract` / `tests` / `tests_regenerated`）；离线端到端测试 `crates/jh-app/tests/h4.rs`（12 个用例，钉住口径而不是会被调参改变的那部分）。冻结的 `jh-jev` / `jh-openai` / `jh-tui` 一字未动，M21 / M22 都没有新增依赖。

**已回写（2026-09-23，M24 / M25 落地；逐条对照代码，非转述）**：

- `crates/jh-app/src/m4.rs`（M24，merge `4dcd485`）：M4 综合评估的编排——臂 = 控制结构（`Loop` / `Maintained`）× 判断通道（LLM / Jev），结构维落到 H1/H2 两个既有开关（`loop-*` = `h1off-h2full`、`maintained-*` = `h1on-h2inc`，见 `docs/research/m4-pilot.md` §2），循环层新增检查节奏 `Cadence`（`crates/jh-app/src/matching.rs`，M1–M3 的默认行为不变）；同一模块里还有诊断对照（各臂强制全量重问、材料逐字节相同）、原生 Agent 基线臂、两条残余消融（跨任务复用：一族建一次通道 vs 每用例重建；固定频率检查）、密度扫描（节奏 × 规模）与族 C 判读 / 复用消融的编排；离线端到端测试 `crates/jh-app/tests/m4.rs`（8 个用例，另有 9 个模块内单元测试）。**M4 未改 `jh-core`**（臂与检查节奏都落在 `jh-app` 的匹配循环里，`git diff 4dcd485^1 4dcd485 -- crates/jh-core` 为空）：读数用的是 M15 / M21 已有的对象（`SemanticState` / `ObserverLibrary` / `DependencyGraph` 等）。
- `crates/jh-app/src/session/**`（M25，merge `bc66d18`）与 `crates/jh-tui/` 的扩展：会话宿主 = `model`（LLM 侧一次调用的抽象 + `jh-openai` 的 `/responses` 流式实现）、`tools`（工作区内只读 + `bash`）、`gate`（确定性风险层 + Jev 类型化判定 `shell_command_allowed` + 防抖 + 采纳门槛）、`approval`（会话内白名单与授权模式）、`agent`（模型 → 门控 → 审批 → 执行 → 回收的状态机）、`view`（领域对象 → 三份视图模型）、`tui`（终端宿主与 `Effect` 执行）；`jh-tui` 侧新增 `Screen` / 布局 / 顶栏 / 面板壳 / 研究焦点与键位（设计基准 = [TUI 界面设计](tui-design.md)，实现口径见其 §9.1）。冻结的 `jh-jev` / `jh-openai` 一字未动；**M25 新增依赖**：`jh-app` 取用 `jh-tui` / `ratatui` / `crossterm` / `futures`（都已登记在根 `Cargo.toml`）。
- 一处已知的文字层缺口（登记在 §9，不在本批修）：`crates/jh-app/src/lib.rs` 的模块地图注释里没有 `session` 条目（`pub mod session;` 本身在 `:61`）。
- `jh-core` 的对象划分以研究目标 §8.1 的一等对象清单为准：M0 落判断器抽象与判断记录，M15 落事件与证据、问题版本表、依赖图、输入投影、失效传播、行动前提复核，M18 落反例记录与问题库（候选 → 影子评估 → 启用 → 停用 / 被替代），M21 落观测器契约与生命周期、行为分类、观测器退出使用的失效传播；`Hypothesis` / `Capability` 仍未定（§12 未确定项 4），观测器库的**入库文件形态**仍未定（观测器的版本号规则已定案，见本节 M21 条目；§12 未确定项 5）。

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
| 9 | 认识状态 | **已定（2026-09-22，M15，§12 未确定项 3 结案）**：「有支持 / 有反证 / 证据冲突 / 证据不足」**不新增独立字段**——由两个正交维度（`EvaluationStatus` 评估状态 / `Validity` 依据有效性）加上依赖图上的支持边 `Support` 与冲突候选 `Conflict`（`DependencyGraph::conflicts_with`）与程序规则共同表达；某一档结论是消费方按这些原值算出的读数，不是运行时里的第三个可漂移字段 | 研究目标 §5.3（原文明确这**不是**四值逻辑） |
| 10 | 行为分类 | `Execute` / `Observe` / `BuildObserver` / `Deliberate` 四类用于调度与成本核算。**已落地（2026-09-23，M21）**：四类落成 `jh_core::Behavior`（`crates/jh-core/src/behavior.rs`），另带出口 `Behavior::Other(String)`——分类不封闭，四类之外的行为带自己的标签进账；模块文档写明四类各自的落点（执行 → `Action` / `check_action`、观测 → `append_evidence`、建设观测器 → `ObserverLibrary`、开放式推理或生成 → `Judge`）。**未落地的一半**：`jh-app` 目前没有按它分桶记账（H4 的生成成本按 `CallEntry::ModelCall` 的 purpose 标签记），调度也仍由 `matching` / `h4` 的固定轮序决定 | 研究目标 §7.1 |
| 11 | 观测器契约 | 要求来源、命题与覆盖关系、测量位置与对象标识、版本与条件、副作用、正负对照与可注入故障、输出格式与失效条件、未覆盖情况。**已落地（2026-09-23，M21）**：八条逐项落成 `ObserverContract` 的必填字段（`requirement` / `proposition` / `coverage` / `measurement` / `support` / `disturbance` / `controls` / `output` / `uncovered`——「命题与覆盖关系」对应两个字段，其余逐条对应），生命周期 `ObserverLifecycle::{Candidate,Verifying,Enabled,Retired,Superseded}`，**启用要求四路全过的独立检验记录**（`ObserverCheck`，否则 `Error::ObserverNotVerified`）；观测器退出使用时的失效传播见第 5 行同名机制（`SemanticState::invalidate_observer`）。类型与迁移表见 §3.4 | 研究目标 §7.2 |
| 12 | 判断器抽象 | 五种判断器同一接口、可配置切换（见 §3.2） | 研究目标 §8.3；[Jev 调研报告](jev-research.md) §13.3.1 |
| 13 | 实验环境与真值 | 提供快照、可控扰动、隐藏真值、独立验收与可计价工具动作；模拟器真值模式只准用于诊断与上限估计 | 研究目标 §8.3、§9.2、§10.2；**M16 已落任务族 A 的一层**（§5 M1）；**M19 已落任务族 B 的一层**（成对场景、计价观察动作、开发案例与未见变体的读数分开，§5 M2）；**M22 已落任务族 C 的一层**（隔离实验环境：可控程序 + 三类宿主钩子点 + 只读存储日志的隐藏检查；三个故障族 × 每族 10 条确定性调度 × 4 路实现 = 120 例，§5 M3） |
| 14 | 调度 | 按事件与需求触发；优先评估可能改变行动的问题；预算覆盖模型调用、输入序列化、工具执行、观测器建设、验证与存储维护 | 研究目标 §8.4；**M16 落成匹配循环的固定轮序与调度规则**（§3.4、§5 M1），**M19 加「证据需求未满足就重问」与影子计划**（§3.4、§5 M2），**M22 加三臂的用例级调度与版本变更阶段**（每用例只跑一次环境、三臂各出一个判定，§5 M3）；「优先评估可能改变行动的问题」与预算口径未实现 |
| 15 | 指标与统计 | 按研究目标 §10.1 主要指标与 §10.2 诊断指标采集；晋升阈值与报告脚本在 M4 冻结 | 研究目标 §10.1、§10.2、§10.3；M16 落了 H1/H2 用到的读数与逐条口径（§5 M1、§6.7）；M19 的 H3 报告落了「收益 / 成本并排 + 逐对逐版本逐子集」的净读数与**场景内事前声明**的 8 条晋升判据与 5 个阈值（§5 M2）；M22 的 H4 报告落了 §7.4 要求报告的五项读数（误放行率、误报、迁移、维护开销、建设成本摊薄曲线）与逐条口径，判据（三条判定规则）在跑前冻结（§5 M3）——这两批的预声明都不是研究目标 §10.3 的统计冻结；**统计冻结已由 M4 落地**（`docs/m4-freeze.md`，冻结提交 `f72d62f`，写在定稿主跑之前），定稿读数与逐块复现命令见 `docs/m4-report.md`（§5 M4） |
| 16 | 完成声明的证据要求（**2026-09-22 新增行**，认领 M15 评审 finding） | 完成声明必须绑定**实际交付物的当前版本**证据才构成证据；声明本身只计数、不参与判定 | 研究目标 §5.2 末条、§10.1；落点 `crates/jh-app/src/family.rs:1002` `accept`（M16 落地，见 §5 M1） |
| 17 | 观测能力的证据通道（**2026-09-23 新增行**，M22 落地） | 观测器只读宿主钩子点（写入边界记录器 / 可控暂停点 / 提交计数器），**只读存储日志的隐藏检查**是唯一真值来源；两者不同模块、不同数据通道，测量脚本的成功输出不得成为唯一真实性来源 | 研究目标 §7.3 末段、§13.2；落点 `crates/jh-app/src/c_program.rs:1222` `hidden_check` 与 `crates/jh-app/src/c_measure.rs:1590` `hidden_verdict`（见 §5 M3） |

**落地进度（2026-09-23 回写；M0–M2 的进度在 2026-09-22）**：第 1 / 2 / 4 / 5 / 6 / 7 / 8 行由 M15 落进 `jh-core`（§3.4），第 9 行按「不新增独立字段」定案，第 13 行与第 16 行由 M16 的实验层覆盖，第 3 行与第 12 行 M0 已交付且此后未改（第 3 行 = 判断记录，第 12 行 = 五种判断器同一接口，见 §3.2、§7.1 #4）。**M18 / M19（M2）补第 2 行与第 15 行的另一半**：问题从「候选 → 影子评估 → 启用 → 停用 / 被替代」的生命周期、发布与回退的失效联动（第 2 行，§3.4、§5 M2），以及先导批次的晋升判据、指标口径与验证记录落库（第 15 行，§5 M2）。**M21 / M22（M3）补第 10 / 11 / 17 行与第 13 行的一半**：四类行为落成类型（第 10 行）、§7.2 八条的契约类型与生命周期（第 11 行，§3.4）、隐藏检查作为独立真值通道（第 17 行），第 5 行的失效传播多一个起因「观测器停用或被替代」，第 13 行多任务族 C 的一层（§5 M3）。**M24 / M26（M4）补第 15 行的另一半**：主要指标口径、H5 两条判据与 δ_NI=0.10、最小有意义效应 Δ=0.15、预算与五条停止条件、种子方案与分析方法在定稿主跑**之前**冻结（`docs/m4-freeze.md`，冻结提交 `f72d62f`），定稿读数、适用范围、失败案例与逐块复现命令落 `docs/m4-report.md`（§5 M4；两个读数提取脚本是临时脚本、不入库）。**仍未落地**：第 10 行的**调度与成本分桶**（`Behavior` 目前只是标签，`jh-app` 未按它记账）、第 14 行的「优先评估可能改变行动的问题」与**运行时**预算口径（M4 冻结的是批次预算，不是运行时口径）；第 16 行的机制目前只在任务族 A 的独立验收里，尚未推广到其它阶段。另：§7.1 #9 的联合演示 M4 未交付（见 §7.1 #9、§5 M4）。

## 5. 阶段 M0–M4

**命名与划分直接沿用研究目标 §12**（不是旧版的产品式里程碑）；阶段按依赖推进，**不预设日历工期**（研究目标 §12 首段）。每阶段给出建设内容 / 退出证据 / 未达标处理，并把模块归属到 crate。**1.0 完成的定义 = 下表 M0–M4 全部达成**；研究目标 §12 的 M5 不在 1.0（见 §8）。

> **TUI 决策变更（2026-09-23，用户指令；取代原决定 4）**：本版原写「1.0 不做 TUI：研究目标 §12 的 M1 行含『最小 TUI』、M4 行含『完整 TUI 展示』，1.0 一律以 CLI/JSON 报告代替，研究视图推迟到 1.x」——**这条偏差已被用户的指令取消**。
> 用户原话：「规划 M4，同时将 TUI 也加入到 M4 工作中。M4 结束后应该给出一个能初步运行，且具备 TUI 界面的 Agent 系统」。
> 据此，研究目标 §12 的 M4 行「完整 TUI 展示」**已按期在 M4 交付**：交付形态 = `jev chat` 交互会话（LLM 主循环 + Jev 判定门控）+ TUI 主界面与首屏 + 三个研究面板接真实数据；工具含只读与 `bash`，**副作用全走审批卡**。
> 会话侧的真调读数（1 轮、2 次模型调用、1 次 `shell_command_allowed` 门控 `usr:allow 0.99`、`bash ls -1` 经审批卡真实执行、退出码 0）与实现口径见 [TUI 界面设计](tui-design.md) §9.1；M1 行的「最小 TUI」在当时仍以 CLI/JSON 报告代替（历史事实，见 §5 M1）。
> 逐条处置：§7.1 #3（已交付）、§8 第 1 条（已移出非目标）、§13 第 5 条（由本次用户指令裁决；**本批仍不修改 `docs/RESEARCH_GOALS.md`**）。**这是决策变更，不是原计划顺延**——本版 2026-09-22 的重写稿写的是「1.0 不做 TUI」。

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
    - **LLM 结构化判断器（已验证；截断问题由 M14 修好，残留偶发失败见下）**：M14 把 `DEFAULT_MAX_OUTPUT_TOKENS` 由 1024 抬到 16384（依据见本节末「cap 取值依据」），`cargo run -p jh-app -- exp --judge llm` 连跑两次（报告 `var/exp/run-1790071861270.json`、`var/exp/run-1790071925436.json`）：
      - 第 1 次：调用 4 次（成功 3 / 失败 1，失败是契约漂移而非截断），一致率 0.889（8/9）、Brier 0.2222、对数损失 0.0000（9 条里 1 条无有限值，见口径）、score 期望值误差 0.3333（n=3）、延迟 min 7089 / p50 14177 / p95 22581 / max 22581 ms、usage 输入 1914 / 输出 8692（3 次上报）。
      - 第 2 次：调用 4 次全部成功（命令干净退出），一致率 0.750（9/12）、Brier 0.3437、对数损失 0.3045（12 条里 1 条无有限值）、score 期望值误差 0.4700（n=4）、延迟 min 5447 / p50 5551 / p95 44974 / max 44974 ms、usage 输入 2567 / 输出 13197（4 次上报）。
      - 分题看：两次运行里 `evidence_strength`（score 题）都是弱项（第 2 次一致 2/4、Brier 0.7329），`failure_kind` 第 2 次 4/4 一致（Brier 0.0115）；第 1 次里唯一 1 条无有限对数损失的读数出现在 `evidence_strength`——模型给出的分布接近 one-hot，答错时直接给真值标签 0 概率（Brier 2.0、对数损失无定义），这正是「概率原值保留、不塌缩成布尔」要暴露的东西。
      - **cap 取值依据（M14 真调：同一提示词直调 `/responses`）**：cap 1024 → `incomplete (reason=max_output_tokens)`；cap 8192 → 4 次调用 3 成 1 败（`case-003` 仍 `Incomplete`、39.1 s；该次报告 `var/exp/run-1790071565046.json`），同一提示词 `case-003` 在 cap 16384 下两轮 reasoning 分别用到 8481 / 4493 token 后 `completed`、在 cap 32768 下用到 11425 token 后 `completed`——即 8192 是「按当时一次成功读下来的」偏小值，而实测所需 reasoning 最长 11425；16384 覆盖它，跑满预算按实测速率 4.38–5.23 ms/token（中位约 4.7）算约 74 s（乐观端）到 86 s（最慢端），仍在 [`DEFAULT_TIMEOUT`] 的 120 s 内，再往上跑满预算会超时、失败形态从截断变成超时。探针请求 / 响应、提示词重建脚本落 `var/probe/`（运行产物、不入库）；判断器现在把 `incomplete_details.reason` 一并写进失败记录，报告自身就能分清截断与内容过滤。
      - **残留失败（M14 新发现，与输出预算无关）**：16384 下三次运行合计 12 次调用、2 次失败，错误都是「模型输出不是约定的 JSON：missing field `probabilities`」——模型把契约里的 `probabilities` 一层省掉，直接给出 `{"<问题ID>": {"<标签>": <概率>}}`（本身是合法 JSON、概率和为 1，只是形状与契约不符）；命中 `case-003`（第 1 次运行）与 `case-002`（合并运行）各一次。这是 prompt 遵循的偶发漂移，不是截断、也不是通道错误；本批不改契约与解析层（解析层按「形状不符即显式失败」处理），已归档 finding：`.tower/comms/findings/20260922-agent-cap-improve-llm-probabilities-12-2.md`。
    - **合并基线（真实与离线同表；来源以 `source` 字段区分）**：`cargo run -p jh-app -- exp --judge rule --judge simulated-truth --judge llm --judge jev`（报告 `var/exp/run-1790071988015.json`）——rule 4/4、`source=rule`、`simulated`（一致率 1.000、Brier 0.0422、对数损失 0.1839、score 期望值误差 0.2667）；simulated-truth 4/4、`source=simulated_truth`、`simulated`（1.000 / 0.0000，是隐藏真值回灌的上限估计，不是读数）；llm 4 次调用成功 3 失败 1（契约漂移）、`source=llm`、`verified`（一致率 0.778（7/9）、Brier 0.2702、对数损失 0.4001、score 期望值误差 0.3600、延迟 p50 13601 / p95 36379 ms、usage 输入 1928 / 输出 16454）；jev 4/4、`source=real_api`、`verified`（0.750（9/12）、Brier 0.3927、对数损失 0.6027、延迟 p50 326 / p95 825 ms、usage 3010/367、model `jev-1.13.0`）。
    - **口径**：以上只有一个 4 样本 × 3 题的最小合成集（12 条判断），2026-09-22 单日三次运行（LLM 两次 + 合并一次），**只能作本环境基线，不构成研究结论**，也不能回答 RQ0 / H0；真实读数（`source=llm` / `real_api`，`live=true`）与模拟 / 回放读数（`source=rule` / `simulated_truth`）已在报告的 `source` / `live` / `verification` 字段里分开标注。同一配置重跑读数就有差异（LLM 三次运行的一致率是 0.889 / 0.750 / 0.778），单次读数不得当作稳定值。
  - **仍未验证**：LLM 结构化判断器在更大样本与更多任务上的表现，以及契约漂移的触发条件（12 次里 2 次，样本相关还是抽样波动，4 样本分不出来）；Jev / LLM 在更大样本与更多任务上的表现；模拟与回放读数仍只作对照与上限估计。
  - **复现命令**（真调测试保持 `#[ignore]`，`cargo test` 不联网；Jev 命令见 `crates/jh-jev/tests/smoke.rs:1-9`，LLM 命令见 `crates/jh-openai/tests/live_smoke.rs:1-12`）：
    - Jev：`TYPESAFE_API_KEY=… cargo test -p jh-jev --test smoke -- --ignored --nocapture`（模型 pin `jev-1.13.0`）
    - LLM：`OPENAI_API_KEY=… OPENAI_MODEL=deepseek-flash OPENAI_BASE_URL=https://api.deepseek.com cargo test -p jh-openai --test live_smoke -- --ignored --nocapture`
    - LLM 判断器基线：`OPENAI_API_KEY=$DEEPSEEK_API_KEY OPENAI_BASE_URL=https://api.deepseek.com OPENAI_MODEL=deepseek-flash cargo run -p jh-app -- exp --judge llm`（报告落 `var/exp/<run>.json`，调用记录追加进 `var/calls.jsonl`）
    - 基线报告：`cargo run -p jh-app -- exp --judge rule --judge simulated-truth --judge llm --judge jev`（报告落 `var/exp/<run>.json`，调用记录追加进 `var/calls.jsonl`）

### M1 固定问题运行时（对应研究目标 §12 第 2 行）

- **建设内容**：事件日志、固定问题集、依赖图、版本检查、匹配循环基线（研究目标 §12）；「最小 TUI」→ CLI/JSON 报告（本版原偏差；**2026-09-23 决策变更**：TUI 已由 M25 接线，该偏差不再成立，见 §5 首段）。
- **退出证据**：完成失效传播与 H1/H2 对照，包含过期响应和无关变化案例（研究目标 §12）。
- **未达标处理**：若真值模式仍无优势，优先修订架构或收窄任务（研究目标 §12）。
- **模块归属**：`jh-core`（事件与证据、问题版本、判断记录、依赖图、失效传播、行动前提复核）、`jh-app`（匹配循环基线、判断器适配、任务环境、报告）。
- **落地状态（2026-09-22 回写；M15 / M16 真调批次）**：
  - **已交付**：`jh-core` 的事件与证据日志、问题版本表、依赖图、输入投影与快照校验、可失效语义状态（失效传播 + 行动前提复核）（M15，merge `b099109`，复核尖端 `0e7ada2`）；`jh-app` 的任务族 A（6 任务 × 3 轮的固定问题集）、匹配循环基线、`jev run` 单任务端到端与 `jev compare` 四组臂对照（M16，merge `a32b611`，复核尖端 `bc60233`）。模块与类型见 §3.4。「过期响应」由快照校验覆盖（迟到结果标过期，M16 修了同轮各请求组共用提问相位的时序并补了判别性测试），「无关变化」由无关结果保留计数体现。「最小 TUI」在 M1 批次里仍按当时的偏差用 CLI/JSON 报告代替——该偏差已由 2026-09-23 的决策变更取消（TUI 组件套件与两屏 / 研究视图随后落地，`jev chat` 在 M25 接线并带审批卡；见 §5 首段、§7.1 #3）。
  - **H1 / H2 对照读数（真实判定器：`jev-1.13.0` 与 `deepseek-flash`）**：报告 `var/compare/h1h2-1790076206875.json`，逐任务轨迹与证据日志在同名目录的 `traces/`（48 个）与 `evidence/`（48 个）下（运行产物、不入库，teardown 后不保留）。四组臂 × 两个判定器，同一份 6 任务 × 3 轮 = **每臂 18 个决策机会**：

    | 臂（H1 / H2） | 判定器 | 调用 | 输入字节 | 判断节点 | 缓存复用率 | 验收 | 恢复 | 旧依据行动率 |
    | --- | --- | --- | --- | --- | --- | --- | --- | --- |
    | `h1on-h2inc` | jev | 21 | 9615 | 29 | 0.447 | 5/6 | 8/8 | 0.000 |
    | `h1off-h2inc` | jev | 14 | 6317 | 19 | 0.659 | 0/6 | 4/8 | 0.056 |
    | `h1on-h2full` | jev | 31 | 14277 | 46 | 0.000 | 5/6 | 8/8 | 0.000 |
    | `h1off-h2full` | jev | 31 | 14277 | 46 | 0.000 | 5/6 | 8/8 | 0.056 |
    | `h1on-h2inc` | llm | 21 | 9615 | 29 | 0.447 | 6/6 | 8/8 | 0.000 |
    | `h1off-h2inc` | llm | 14 | 6317 | 19 | 0.659 | 0/6 | 4/8 | 0.056 |
    | `h1on-h2full` | llm | 31 | 14277 | 46 | 0.000 | 6/6 | 8/8 | 0.000 |
    | `h1off-h2full` | llm | 31 | 14277 | 46 | 0.000 | 5/6 | 8/8 | 0.056 |

  - **结论只能按配对读**（报告 `pairings` 与 notes 6 写死这条口径）：
    - **H1（依赖失效传播）**：增量配对里关掉它 → 验收 0/6、恢复 4/8（未恢复的是 A1 / A2 / A3 / A5 这 4 个必需动作）；全量配对里关掉它 → 恢复 8/8、验收 5/6（jev 上与开启臂完全相同；llm 是 5/6 vs 6/6，差别只在那一个错误动作）。**H1 在全部四个配对里都看得见的独立痕迹只有两处**：旧依据行动率 0.000（开）vs 0.056 = 1/18（关），以及关闭臂里 A4 第 0 轮凭失效前提发出的 `rollback-cache`。其余差别随 H2 口径变化——**跨配对的差异是交互作用，不得当主效应读**。
    - **H2（增量 vs 全量）**：H1 开启的配对里增量省约 32% 调用（21 vs 31）与 33% 输入（9615 vs 14277 字节），判断节点 29 vs 46、缓存复用率 0.447 vs 0，而质量与结果相同——两臂的分歧条数相同（jev 各 5 条、llm 各 0 条；jev 的一致率 0.828 vs 0.891 只因分母是 29 vs 46）、验收与恢复相同（jev 5/6 与 8/8、llm 6/6 与 8/8）。H1 关闭时方向反转：增量 14 次调用 / 6317 字节 / 19 节点、验收 0/6、恢复 4/8，而全量 31 次调用 / 14277 字节、验收 5/6、恢复 8/8——全量重算在没有失效传播时把「输入变了要重查」这个触发补了回来。
    - **失效传播口径（八臂相同）**：漏失效 0、误失效 1（只来自模型建议的额外依赖：A2 的 `premise_holds` 额外声明读 `runtime.metrics`）、无关结果保留 22 次。
    - **恢复延迟**：四个开启臂都是 8/8 恢复、都是 7 个在要求轮内发出 + A5 的 `restore-cache` 多 1 轮；墙钟按配对分列（不跨配对混读）——**开启的增量臂** jev 363–783 ms/动作、llm 1266–23694 ms，**开启的全量臂** jev 365–1673 ms、llm 1769–35747 ms。关闭的增量臂只有 4/8 恢复，其余 4 个（A1 `rollback-cache`、A2 `throttle-writes`、A3 `escalate-manual-review`、A5 `restore-cache`）从第 1 轮起被要求但一直没发出。
    - **成本与失败**：该 run 合计 **194 次真实判断调用**（8 臂 × 6 任务）、失败 0。调用数按调度决定、与判定器无关（21 / 14 / 31 / 31），输入字节 9615 / 6317 / 14277 / 14277；token 与延迟按判定器给：jev 输入 14079 / 9231 / 21503 / 21503、输出 978 / 626 / 1617 / 1619、延迟 p50 368 / 370 / 371 / 372 ms（max 1200 ms）；llm 输入 12254 / 8078 / 18657 / 18657、输出 23365 / 15379 / 46491 / 33289、延迟 p50 3116 / 2019 / 4085 / 3093 ms（max 42596 ms）。引用「194 次真调」必须带 run 号：`var/calls.jsonl` 此时混有两次离线 `rule` 校验跑（各 97 行），读数按 run 过滤不串味。
    - **provenance**：这份报告由 `compare --regenerate` 从该次运行的轨迹**重建**（判定器一次都没调用），`report_environment.recorded_at = regenerate` 标的是取值时机——它的模型 / 端点字段是**重建时刻**的 shell 环境，不是那次运行的环境；那次运行真正用到的模型按每条判断的 `version` 核对（这批轨迹产自 `e337f8e`，该版本还没有 `judge_endpoint` 字段，因此为空；自 `bc60233` 起轨迹随运行记 `judge_endpoint`）。口径提醒：base URL 不要内嵌凭据——轨迹会把配置的 base URL 原样记下（key 只记布尔）。
    - **口径与限制**：先导 6 任务、单日单次运行、真实判定器（`jev-1.13.0` / `deepseek-flash`，前提问题 `premise_holds` 只作判定质量的诊断读数、不进行动前提门）。按研究目标 §10.4，这些**只作本环境读数，不构成统计结论**，也不回答 H1 / H2 本身；单次运行读数不得当稳定值。
    - **已知限制（随批登记，详见 §9）**：任务族 A 的材料有候选集诱导（材料不足以归因时两个真实判定器都倾向从候选集挑标签而不选 `other`）——**M2 已处置**（判据写进族 B 的问题定义 + `MaterialGate`，见 §5 M2、§9）；jev 在 H1 开启的增量臂上仍有一个与失效无关的提前动作（A4 第 1 轮 `rollback-cache`，M19 的族 B 批次未复现该形态，仍留待 M4 在更大样本上复核）。
  - **复现命令**（与报告 `command` 字段同口径；真调 key 只经环境变量）：
    - 四组臂真调：`TYPESAFE_API_KEY=… cargo run -p jh-app -- compare --judge jev`；`OPENAI_API_KEY=$DEEPSEEK_API_KEY OPENAI_BASE_URL=https://api.deepseek.com OPENAI_MODEL=deepseek-flash cargo run -p jh-app -- compare --judge llm`（两个判定器一起给就是并排的八臂）
    - 离线（不联网）：把 `--judge` 换成 `rule`（或 `replay --replay var/calls.jsonl`）
    - 单任务端到端：`cargo run -p jh-app -- run --task A5 --judge rule`（缺省臂 `h1on-h2inc`，`--arm` 可换另三组）
    - 报告重建（不调判定器）：`cargo run -p jh-app -- compare --regenerate --judge jev --judge llm --out-dir var/compare/h1h2-1790076206875`

### M2 问题集合精化（对应研究目标 §12 第 3 行）

- **建设内容**：反例记录、候选生成、验证、影子运行、版本发布与回退（研究目标 §12）。
- **退出证据**：在隔离的成对任务和未见变体上检验 H3（研究目标 §12）。
- **未达标处理**：若只记住案例，记录局限并保留固定问题模式（研究目标 §12）。
- **模块归属**：`jh-core`（问题版本与失效联动、反例记录结构）、`jh-app`（候选生成走生成模型、影子运行与晋升流程、成对任务环境）。
- **约束**：精化不得修改用户目标、降低验收要求或扩大工具权限（研究目标 §6.4）；新问题必须声明消费者，没有消费者的问题不常驻（研究目标 §6.3）。
- **落地状态（2026-09-22 回写；M18 / M19 真调批次）**：
  - **已交付**：`jh-core` 的反例记录与问题库生命周期、发布与回退的失效联动（`counterexample` / `library`；M18，merge `9acb5dd`，复核尖端 `0c71380`）；`jh-app` 的候选生成、验证、影子运行、晋升与回退、任务族 B 与 `jev h3` 两臂对照（`generation` / `validation` / `shadow` / `promotion` / `family_b` / `h3` + `tasks/family_b.json` + `tests/h3.rs`；M19，merge `84c3319`，复核尖端 `330fdf2`）。模块、类型与版本号规则见 §3.4。
  - **流程与规模**：开发案例的失败现场 → 反例（`var/h3/final-r3/counterexamples.jsonl`，3 条）→ 候选（生成模型 `deepseek-flash`，按 §6.3 八字段严格解析，3/3 成功）→ 验证（每对 4 例 × 3 对 = 12 例：正例 ×2、反例、不同表述 / 不同服务的未见用例）→ 影子运行（独立状态里判、只在开发案例上）→ 晋升（8 条判据 + 阈值事前声明）→ 两臂对照。任务族 B = 3 对成对场景（证书过期 vs 时钟偏差、上游限流 vs 重试风暴、权威记录缺失 vs 网络分区）× 每对 1 个开发案例 + 2 个未见变体（一 B1 一 B2）；**每个任务 3 轮**，即对照侧每臂 **6 任务 × 3 轮 = 18 个决策轮**（口径与族 A 一致）；对照读数只在 6 个未见变体上采集，开发案例只用于反例提取、候选验证与晋升判据。
  - **H3 对照读数（真实判定器 `jev-1.13.0` / `deepseek-flash`，两个判定器各跑两臂）**：报告 `var/h3/final.json`，轨迹与证据在同名目录 `var/h3/final-r3/`（运行产物、不入库，teardown 后不保留）。两臂共用同一段循环、同一套前提门与同一批任务 / 初始证据 / 判定器配置 / 决策时机，差别只有问题库当前启用的版本；同一份 6 个未见变体 = **每臂 6 个任务 × 3 轮 = 18 个决策轮**：

    | 臂（启用的版本） | 判定器 | 调用 | 输入 / 输出 token | 延迟 p50（min–max）ms | 验收 | 恢复 | 不该发出的动作 |
    | --- | --- | --- | --- | --- | --- | --- | --- |
    | 固定（只启用 v1） | jev | 6 | 3915 / 288 | 296（283–753） | 0/6 | 3/9 | 0 |
    | 精化（启用 v2） | jev | 9 | 6281 / 447 | 297（262–326） | 6/6 | 9/9 | 0 |
    | 固定（只启用 v1） | llm | 6 | 3250 / 2660 | 2319（2045–3182） | 0/6 | 3/9 | 0 |
    | 精化（启用 v2） | llm | 9 | 5310 / 2237 | 1429（940–2868） | 6/6 | 9/9 | 0 |

  - **B1 / B2 分开报告（研究目标 §9.3）**：B1（关键证据已在初始快照里给全）固定臂 **0/3** → 精化臂 **3/3**，判断调用 3 vs 3、恢复 0/3 vs 3/3、无观察动作；B2（关键证据只能靠计价观察取得）固定臂 **0/3** → 精化臂 **3/3**，判断调用 3 vs 6、恢复 3/6 vs 6/6；两臂都发出 3 次观察 × 3 计价单位 = **9 单位**（观察成本两臂相同），动作计价成本 9 vs 33 单位（精化臂把补救动作真的发了出来）。两臂的「材料不足却读出候选集内标签」都是 0 次（固定臂材料不足 6 次、精化臂 3 次）。
  - **晋升：三对全部 v2**。`DECLARED_CRITERIA` 的 8 条判据全过——契约八字段齐全、消费协议不变、证据需求在可观测清单内且含关键证据、开发案例区分率 1.000 vs 固定版本 0.000（增益 1.000）、正例一致率 1.000、反例一致率 1.000、未见表述一致率 1.000、影子运行「走向正当动作」P1 3 次 / P2 2 次 / P3 3 次且「走向不该发出的动作」0 次（阈值：开发案例区分率 ≥ 0.75 且增益 ≥ 0.25、正例 / 反例 ≥ 0.75、未见用例 ≥ 0.6）。四路变更每次都是「问题 true / 输入投影 true / 消费者 false / 策略 false」：v1 只读 `app.log`，v2 分别改读 `credential.audit` / `upstream.audit` / `dns.audit`，选项标签集合与控制规则摘要不变（`bc3331db42c7508d`），边界四项（用户目标 / 验收要求 / 工具清单 / 控制规则）发布前后摘要一致。发布联动失效：每对 stale 1 / blocked 1 / 2 条传播路径，原因 `QuestionVersionChanged`；旧版本转 `Superseded`（只改状态、不删除、不重编号）。回退演练在库的副本上跑（P3 由 v2 回退到 v1，走同一个发布函数）。
  - **两个「固定版本在开发案例上的率」口径不同（报告 `validation.note`）**：按材料调整后的一致率 **1.000**（把「读不到关键对象时正确读法就是 `other`」算作答对）vs 区分率 **0.000**（只数「材料足够时读出正确一侧」的比例）；晋升判据用的是后者。
  - **成本口径（收益须按 §6.5 扣除）**：精化循环 = 生成 3 + 验证 12 + 影子 12 = **27 次调用 / 32104 token**（生成那一笔按复用候选产物里的原值计入，`generation_reused`，含当年那次运行的 run 标识 `h3-1790089502590-generation`：3 次调用、4992 / 8708 token、延迟 10209–16518 ms）；本次运行合计 **60 次调用、0 失败**（本运行的 57 次 = 开发 3 + 验证 12 + 影子 12 + 固定臂 12 + 精化臂 18，另加复用的 3 次生成），合计输入 42923 / 输出 15675 token。影子那一笔含同一次运行里控制路径的请求，因此大于影子判断条数（9 条）。
  - **结构性前提（读上面这些数字之前先读这一条；报告的 `protocol.arms` 与 `notes` 原文同此）**：族 B 的固定版本 v1 只读 `app.log`，而 `app.log` 的措辞对两侧刻意相容；材料充分性判据（`MaterialGate`）把「关键对象不在该次输入投影里」的正确读法定为 `other`。于是固定臂在未见变体上按材料调整后的真值全对（`agreements` 6/6、材料不足 6 次），但前提门要的是侧标签，补救动作一次也发不出；B2 的观察动作由 `other` 把关，所以固定臂唯一能发出的必需动作就是那几次观察。**固定臂 0/6 与「恢复 = 观察次数」是读取范围 + 这条判据的推论，不是经验性的能力差距**；H3 的差值口径因此等价于「启用的读取范围是否包含决定性对象」。精化臂能走通还有第二个机制成分：`matching` 的「证据需求未满足就重问」（问题声明的读取对象里，有哪个现在有当前版本、而手上的答案当时没读到它，就必须重问）——这条规则**两臂都生效**，只有精化臂的读取范围里包含关键对象才会被它触发。因此这批读数**不是「精化一般有价值」的证据，也不能外推到其它任务族**（§6.7 的 H3 行同此口径）。
  - **未出现「只记住案例」**：迁移类用例（不同表述 / 不同服务的未见用例）一致率 1.000（每对 1 例、3 对全过，三对的 `unseen_rate` 都是 1.000），因此本批**不触发** §5 M2 的未达标处理；固定问题模式作为对照臂保留（固定臂就是「只启用固定问题集 v1」那条臂），也是后续批次判断「只记住案例」的基准。
  - **口径与限制**：先导规模——3 对成对场景 × 每对 2 个未见变体、单日单次运行、真实判定器、候选产物复用同一批；按研究目标 §10.4，这些**只作本环境读数，不构成研究结论**，也不回答 RQ3 / H3 本身，单次运行读数不得当稳定值。随批登记的限制（详见 §9）：影子的「若采纳」只按门判据重算、不含发出前的前提复核（H1 机制）；`MaterialGate` 是族 B 场景声明的事实（族 A 不受影响，判据为空）；§6.3「证据需求」的时效与获取方法在 `jh-core` 仍未建模，由 M19 的候选契约承载；观察动作靠 `ActionSpec::effect` 产出证据（族 A 的动作无产出、行为不变）。
  - **provenance**：报告 `command` 字段是原始命令行（key 只以环境变量名出现）；`cost` / `gains` 的账与 `var/calls.jsonl` 里 `h3-1790091168258-*` 的 57 条记录逐条对得上；`generation.reused` 的 `source_file` / `source_run_id` 是从调用记录里查回来的，不是写死的。
  - **复现命令**（与报告 `command` 字段同口径；真调 key 只经环境变量）：
    - 真调两臂：`OPENAI_MODEL=deepseek-flash OPENAI_BASE_URL=https://api.deepseek.com OPENAI_API_KEY=$OPENAI_API_KEY TYPESAFE_API_KEY=$TYPESAFE_API_KEY cargo run -p jh-app -- h3 --judge jev --judge llm --candidates var/h3/live/candidates.json`（报告落 `var/h3/<run>.json`、产物落 `var/h3/<run>/`）
    - 离线（不联网）：`cargo run -p jh-app -- h3 --judge rule --candidates <候选产物>`；端到端测试 `cargo test -p jh-app --test h3`（确定性规则判定器 + 预置候选产物跑完整条链路）

### M3 观测能力建设（对应研究目标 §12 第 4 行）

- **建设内容**：`BuildObserver`、测量契约、对照故障注入、复用与失效（研究目标 §12）。
- **退出证据**：在并发故障族中检验 H4，报告累积成本（研究目标 §12）。
- **未达标处理**：若不能独立验证仪器，不允许其自证任务完成（研究目标 §12）。
- **模块归属**：`jh-core`（观测器契约、与判断/证据的失效联动）、`jh-app`（隔离实验环境中的可控程序、隐藏检查、成本核算）。
- **标志性任务**：取消后写入的并发故障（研究目标 §7.3）；观测器必须经过独立检验，测量脚本的成功输出不能成为唯一真实性来源（研究目标 §7.3、§13.2）。
- **落地状态（2026-09-23 回写；M21 / M22 真调批次）**：
  - **已交付**：`jh-core` 的观测器契约类型、独立检验的启用门槛、观测器生命周期与失效联动、四类行为枚举（M21，merge `aad5fb1`，复核尖端 `219982a`）；`jh-app` 的隔离实验环境与任务族 C、声明式测量契约的生成与四路独立检验、H4 三臂对照与 JSON 报告（M22，merge `c9d9f16`，复核尖端 `083d334`）。模块、类型与迁移表见 §3.4、§4 第 10 / 11 / 17 行；标志性任务（取消确认后写入）就是族 C 的第一族 `cancel_after_confirm`。
  - **权威样本声明（读下面的数字前先读这一条）**：本节引用的是 `var/h4/h4-1790143399409.json`（三份契约全部被接受）。**`var/h4/final.json` 是另一次真调样本**（run `h4-1790144545734`，三份契约里有一份被拒），只用来说明契约生成的不稳定性，**两份样本不得混读**；`var/h4/replay.json` 是同一批契约与测试产物的离线复跑（不联网、不重发生成调用），读数与权威样本逐项相同；摊薄曲线那一列以 `replay.json` / `replay-rejected.json` 为准（`final.json` 的该列是修复前的旧口径 `[1,1,2]` / 0.1176，同一批被拒产物按新口径的渲染在 `replay-rejected.json`，为 `[1,2,3]` / 0.1765）。**这两份离线报告的产物路径有已知限制**（M22 r3 评审留给本批的登记项）：复用产物跑出的 `replay.json` / `replay-rejected.json`，其 `raw.contracts_file` / `raw.tests_file` 指向各自 `out_dir` 下**不存在**的文件（它们只复用输入、不重写产物）——要拿可直接复用的产物路径，看权威样本或 `final.json` 的 `raw` 段（那两个目录里 `contracts.json` / `tests.json` 都在）；本批**只登记、不改代码**（`crates/jh-app/src/h4.rs` 不在本批 scope，见 §9）。`var/` 运行产物不入库、teardown 后不保留，关键读数抄录如下。
  - **规模与口径**：任务族 C = 三个故障族（`cancel_after_confirm` 标志性任务、`lost_update`、`duplicate_after_restart`）× **每族 10 条确定性调度（N = 10）× 4 路实现**（1 路合理 + 3 路已知错误：不协调、绕过记录器、篡改记录器）= 每族 40 例、**全批 120 例**；同一个用例只跑一次环境，三臂各出一个判定，**机会数与真值全部来自隐藏检查**（存储日志通道，不看记录器、不看程序自报）。机会 = 隐藏检查认定确实发生了违反的用例数：24 / 21 / 27，**全批 72**；无违反用例 16 / 19 / 13（按 `cancel_after_confirm` / `lost_update` / `duplicate_after_restart` 的族序），全批 48。竞态不靠掷骰子重复：10 条不同的确定性调度可复现；**一次没检出只读作「这条调度上没检出」，不读作「不存在」**，检出率一律给 n/N（机会数为 0 时记 null、不记 0）。
  - **三臂读数（真实 LLM 生成；判据用的那一项是误放行率）**：

    | 臂 | 机会数 | 检出 | 误放行 | 弃权（失败态） | 无违反用例 | 误报 | 判对 | 检出率 | 误放行率 | 误报率 |
    | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
    | `fixed`（只能用族 C 之前就存在的可观察量） | 72 | 39 | **33** | 0 | 48 | 16 | 32 | 54.2% | **45.8%** | 33.3% |
    | `generated`（同样的可观察量，断言由真实 LLM 现生成） | 72 | 72 | **0** | 0 | 48 | 48 | 0 | 100% | **0.0%** | 100% |
    | `observer`（声明式测量契约 + 写入边界记录器 + 可控暂停点 + 提交计数器） | 72 | 24 | **0** | **60** | 48 | 0 | 36 | 33.3% | **0.0%** | 0% |

    - 逐族：`fixed` 在 C1 是 18/24 检出（25% 误放行）、C2 是 21/21（0%）、C3 是 0/27（100% 误放行）；`observer` 三族都是 33.3% 检出、0% 误放行（C1 / C3 的检出各 8/24、9/27，C2 7/21）。**C2 上固定臂本来就够用**（终态计数看得见丢失更新），这正是「本对照不回答『观测器普遍更好』」的读数依据。
    - **两侧读数**（检出率与「无违反时判对率」的平均，用来检查判据站不站得住）：`fixed` 60.4%｜`generated` 50.0%｜`observer` 54.2%。它**在跑后加进报告**（`hypothesis.reporting` 里写明「不是判据的一部分」）——只用一侧错误率会被「一律判违反」的臂骗过去；`reporting` 这个键是评审 r1 修正（`2400278`）之后才写进报告的，**权威样本里没有它**，要看这段原文请读 `replay.json`（`final.json` / `replay-rejected.json` 也有）。
  - **结构性前提（读上面这些数字之前先读这一条；报告 `protocol` 与 `notes` 原文同此）**：分母取自隐藏检查（`crates/jh-app/src/c_program.rs:1222` `hidden_check`）——**臂说「通过」不等于没有违反**；观测器与隐藏检查在**不同模块、不同数据源**（观测器只读写入边界记录器与暂停点轨迹，隐藏检查只读存储自己的日志）；**独立检验的第四路按隐藏检查判**（观测器可以什么都看不到，那也不算这一路通过），自证式的伪隐藏检查会被启用门槛以 `ObserverNotVerified` 拦下并把失败记录留档（实测：三族的自证式检查都被拒、生命周期停在「验证」、库里留一条检验记录）；§7.3 的「不得把要求换成『最终缓存为空』」由 `BANNED_SUBSTITUTIONS`（代码里的常量）与报告里的 `weakened_substitution` 承担——固定测试臂 C1 的检查项故意用其中一条并如实标注，因为那就是「已有观察工具」能做到的极限。
  - **诚实性要点（不许省略）**：① 通用测试生成臂 0% 误放行是**退化的**——它对无违反的干净用例一次都没判对（C1 16/16、C2 19/19、C3 13/13，共 48/48 全判违反），误报率 100%，所以「0% 误放行」不是能力证据；② 观测器臂的弃权（60/120）是失败态（记录器被绕过或被篡改时**拒绝作证**），既不折成「成立」也不算「违反」——它正是扣掉观测器检出率的那部分；③ 契约与测试计划**各只有一次真实生成**，没有做多次采样的稳定性估计；跨运行观察：尖端另一次真调样本（`final.json`，run `h4-1790144545734`）里 3 份契约有 1 份被拒（该族的观测器臂标「未验证」，检出随之掉到 17/72 = 23.6%、弃权 80 次）——这是**实测观察，不是代码属性**，样本量下分不清是抽样波动还是系统性；④ 摊薄曲线的生成调用按**实际发起数**计（一族一个产物条目 = 一次调用，含失败与被拒的那次，不按「那一臂最后可不可用」过滤）；⑤ 三个族是同一实验环境里的三条顺序约束，**不是三个独立领域**，迁移读数只能说明「换个故障族就得重新建设」。
  - **迁移（没有为它建、直接把同一个观测器套上去）**：对角线三格检出 33.3%（8/24、7/21、9/27）；非对角线六格检出 0.0% / 0.0% / 0.0% / 0.0% / 20.8%（5/24）/ 14.3%（3/21），误放行 33.3% / 33.3% / 33.3% / 33.3% / 12.5% / 19.0%。**为单个族建的观测器不会迁移到别的故障族**；要跨族就得另建一份契约，那笔账记在摊薄曲线的第二个账期里。
  - **维护（宿主代码从 v1 到 v2：新增批量提交路径 `commit_batch`）**：固定测试臂 0 次生成调用、读数一字不变（断言写死在代码里，也就不会跟着改）；通用测试生成臂要重新生成测试计划（3 次调用）；观测器臂的代价是**重挂钩子点 + 重跑四路独立检验**。实测：只改契约的支持版本、不重挂 → 全批检出 **30.56%（22/72）**；重挂并重跑检验 → **66.67%（48/72）**；同一份契约再拿掉 **`commit_counter`** → 检出不变而误放行率 **0% → 66.67%（48/72）**——提交计数器是唯一能发现「记录器被绕过」的通道，少了它观测器只能照着（被记空的）事件流说「命题成立」。
  - **成本与摊薄**：本次运行 **9 次真实生成调用**（3 份契约 + 3 份测试计划 + 版本变更后重生成 3 份）、**0 失败**，输入 5561 / 输出 68883 token，三笔的延迟 p50 分别是 47.4 s / 30.9 s / 32.2 s；验证与重复运行不产生调用，按次数单独记（主用例 120、版本变更 240、四路独立检验 12、迁移 360、真值重算 120）。摊薄曲线：覆盖到第 3 个族时累积生成调用数 固定 0 / 通用 3 / 观测器 3，**每检出一次所花的生成调用数 0.0417（3/72）vs 0.125（3/24）**；货币成本不虚构（仓库里没有权威价格表），动作与观察的自定计价单位不参与本对照。
  - **假说判定（照报告写，不调参凑正向）**：判据（**三条判定规则**）在跑之前冻结（提交 `b05f1e4`），M22 的评审在提交历史里核实「判定规则未变」；跑后只把报告方式（`hypothesis.reporting` 里的「两侧读数」）从判据里单独拆出并如实标注。读数：`improves_results = false`、`improves_budget = false` → 报告结论原文 **「不支持累积能力假说」**——建设观测器既没有改善后续任务的结果（误放行率不低于两个基线臂），也没有改善预算利用（每检出一次所花的生成调用数不低于通用测试生成臂）。**按原始检出率排序，观测器臂在这一批读数里输给两个基线臂**（33.3% vs 54.2% / 100%）：两种读法都在报告里，别只看一个。
  - **口径与限制**：先导规模——三个故障族 × 10 条确定性调度 × 4 路实现、**单日单次真调**、真实生成模型（`deepseek-flash`）；按研究目标 §10.4，这些**只作本环境读数，不构成研究结论**，也不回答 RQ4 / H4 本身，单次运行读数不得当稳定值。**五条限制随批登记**（详见 §9）：契约与测试各只有一次真实生成（无稳定性采样）、观测器的弃权按失败态计、通用臂的 0% 误放行是退化读数、不重挂与拿掉计数器两种维护形态各有独立代价、跨族不可外推。
  - **复现命令**（真调 key 只经环境变量）：
    - 真调三臂：`OPENAI_MODEL=deepseek-flash OPENAI_BASE_URL=https://api.deepseek.com OPENAI_API_KEY=$OPENAI_API_KEY cargo run -p jh-app -- h4`（报告落 `var/h4/<run>.json`、产物落 `var/h4/<run>/`）
    - 离线复跑（不联网、不重发生成调用）：`cargo run -p jh-app -- h4 --contracts var/h4/<run>/contracts.json --tests var/h4/<run>/tests.json`
    - 端到端测试（确定性、离线）：`cargo test -p jh-app --test h4`（12 个用例）

### M4 综合评估（对应研究目标 §12 第 5 行）

- **建设内容**：判断器与架构主对照、关键消融、密度扫描（研究目标 §12）；「完整 TUI 展示」→ **已交付**（2026-09-23 决策变更：`jev chat` 交互会话 + TUI 主界面 + 三个研究面板接真实数据；见 §5 首段与 §7.1 #3），报告与文档化结果照 §7.1 #8 交付。
- **退出证据**：回答 H5，形成适用范围、失败案例与可复现报告（研究目标 §12）。**已达成**：H5 获得支持、适用范围与失败案例、限制清单与逐条复现命令都在 `docs/m4-report.md`（见下面的落地状态）。
- **未达标处理**：依据证据给出支持、条件支持、不支持或不足（研究目标 §12；结论等级见研究目标 §10.4）。
- **模块归属**：`jh-app` 实验层（对照与消融编排、指标统计、报告导出）；`jh-core` 只按需补对象能力（M4 未新增对象）。
- **完成口径**：报告含原始数据位置与复现命令；判定阈值全部来自本项目自测（[Jev 调研报告](jev-research.md) §13.4 #2、§14.3 E2），概率类指标（Brier score / 对数损失等）按研究目标 §10.2 的口径报告。

- **落地状态（2026-09-23 回写；M24 先导 + M26 定稿批次 + M25 TUI 接线）**：
  - **已交付（实验侧）**：`jh-app` 的 `m4` 子命令（M24，merge `4dcd485`，`crates/jh-app/src/m4.rs`）——2×2 主对照（控制结构 × 判断通道）、强制提供相同新增证据的诊断对照、原生 Agent 基线臂、两条残余消融（禁止跨任务复用、固定频率检查）、密度扫描，以及族 C 判读与复用消融的编排；`jh-core` 未改。先导读数（每格 2 次、族 C 前 2 条调度）落在 `docs/research/m4-pilot.md`（1141 次调用 / 1.90M token / 失败 5）。
  - **已交付（统计冻结与报告）**：`docs/m4-freeze.md`（M26，冻结提交 `f72d62f`，写在定稿主跑**之前**）冻结主要指标口径、H5 两条判据、δ_NI=0.10、最小有意义效应 Δ=0.15、预算与五条停止条件、种子方案与分析方法；`docs/m4-report.md` 是定稿批次的权威报告（文件名定案见 §12 #6），含适用范围、失败案例、11 条限制与逐块复现命令。
  - **定稿批次规模与预算遵守**：2026-09-23 单日单批、按臂 / 用途分 7 块（1 冒烟 + 6 定稿）；**1858 次调用（失败 5，0.27%）/ 2.588M token（入 1,221,357、出 1,366,610）/ 墙钟约 1 小时 55 分**——调用数上限 2200、token 上限 3.2M、墙钟上限 3.5 小时、失败率停止线 20% **全部合规**；唯一超指导值的是单块时长（C1a ≈ 55 分钟 > 指导值 35 分钟，未触发任何停止条件，报告 §12 登记）。
  - **主检验 H5 = 获得支持（本环境）**：依赖触发调度（`triggered`）在质量不劣的前提下判断请求与重算输入都不高于定频臂（`every-2`）——规模 3 验收 50.0% vs 0.0%、规模 6 验收 50.0% vs 16.7%（LLM）/ 8.3%（Jev）；判断请求 14 < 17、27 < 35，重算输入 6549 < 14213 B、11544 < 26378 B；四条 Newcombe 区间下界（+0.000、+0.000、−0.041、+0.051）都 ≥ −δ_NI（报告 §2）。**边界**：每格 1 次、只跑两个依赖维护臂、只覆盖族 A / 族 B 与 `every-2` 这一档定频（报告 §2.3）。
  - **其余四档（探索性，只在研究目标 §10.4 的四档内，照报告 §6 写）**：**结构维 = 条件性支持**（族 A 15/18 → 18/18，+0.167，两个判断通道逐格相同；请求 −32%、重算输入 −33%、复用率 0 → 44.7%、不该发出的动作 3 → 0、旧依据承诺 3 → 0；**但区间下界 −0.040 未排除零**，族 B 上质量维度不可读）；**判断通道维 = 条件性支持（仅经济收益）**（质量打平：各 15/18、各 18/18；Jev 延迟 p50 低一个数量级、输出 token 低两个数量级，但 Brier 高一个数量级——0.24–0.26 对 0.004–0.007）；**跨任务复用 = 条件性支持（仅成本）**（生成调用 3 对 36；**只有 `cancel_after_confirm` 一族的 12 例有判别力**，另两族的原因与出处见 §9 backlog）；**观测能力累积收益（族 C）= 不支持或被否定**（依赖维护臂在 `cancel_after_confirm` 上误放行 55.6%–66.7%、`lost_update` 整族没有通道；与 M3 同向）；**原生基线是否被接口限制 = 证据不足**（10/18 对 `loop-llm` 的 15/18，方向是「没有观察到限制」）。
  - **未交付**：§7.1 #9 的联合演示（同一次运行里既精化问题又建设观测器）——M4 的两批都没做（问题库固定 v1、族 C 只做当日判读与复用消融），先导文档自己已把它列入「本轮没做的」（`docs/research/m4-pilot.md` §8.9 第 9 条），报告 §3.4 也写明「未见任务上的精化收益」与「观测能力累积收益」两项 §10.1 指标**本批不产生读数**。
  - **TUI 侧（决策变更的交付物）**：`jev chat` 交互会话（LLM 主循环 + Jev 判定门控 + 审批卡）、TUI 主界面与首屏、三个研究面板接真实数据（M25，merge `bc66d18`）；会话真调读数与实现口径见 [TUI 界面设计](tui-design.md) §9.1（1 轮、模型 2 次、门控 1 次 353 ms、`bash ls -1` 退出码 0、进程退出码 0，证据 `var/chat/h1h2-1790162050626/evidence.jsonl`）。
  - **复现命令与原始数据位置**：见 `docs/m4-report.md` §7（逐块 `jev m4 …` 命令、离线回放核验与端到端测试命令，以及 `var/m4/final/` 的产物路径表）；运行产物不入库、worktree 拆除后不保留，关键读数已抄进报告。

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
| 固定问题集合 | H3 | M2；**已落地**：`jev h3` 的固定臂（只启用固定问题集 v1）vs 精化臂（启用晋升后的 v2），保留观察与生成模型（§5 M2、§6.7） |
| 只能使用已有观察工具 | H4 | M3；**已落地**：`jev h4` 的 `fixed` 臂（族 C 之前就存在的可观察量：自报状态、当前值、自报写入意图数），C1 的检查项故意用 §7.3 禁止的弱化替换、并如实标 `weakened_substitution`——它是「已有观察工具」能做到的极限，作为对照存在（§5 M3） |
| 使用通用测试生成 | H4 | M3；**已落地**：`jev h4` 的 `generated` 臂（同样那些可观察量，断言由真实 LLM 现生成、宿主代码不改动，断言编译进封闭枚举 `Check` 并过一遍越界词扫描）（§5 M3） |
| 禁止跨任务复用 | 累积优势 | M4；**已落地**：`jev m4` 的复用消融（`reuse`＝一族建一次通道 vs `rebuild-per-case`＝每用例重建），生成调用 3 对 36，**只有 `cancel_after_confirm` 一族的 12 例有判别力**（§5 M4、报告 §4.4 与 §10 第 6 条） |
| 固定频率检查 | H5 | M4；**已落地（主检验）**：`jev m4` 的密度扫描（`triggered` vs `every-2` × 规模 3 / 6，每格 1 次），H5 = 获得支持（§5 M4、报告 §2） |

### 6.3 任务族 A / B / C — 研究目标 §9.3

- **任务族 A**：会变化的故障环境（注入推翻前提的新证据、局部变化、过期结果、未列入候选集的原因）→ 服务 H1/H2，M1 起。
- **任务族 B**：现有问题无法区分的成对场景（如「证书过期 vs 时钟偏差」）；B1（已给关键证据）与 B2（需主动观察获取，观察须计费）**分别报告** → 服务 H3，M2 起。**已在 M19 落地**：3 对成对场景 × 每对 2 个未见变体（一 B1 一 B2），加 3 个开发案例与 12 个验证用例，入库在 `crates/jh-app/tasks/family_b.json`（落地状态与读数见 §5 M2）。
- **任务族 C**：需要新增观测机制的并发/状态故障（如取消竞态），已有工具取不到证据 → 服务 H4，M3 起。**已在 M22 落地**：3 个故障族（取消确认后写入 / 丢失更新 / 重启重放重复写入）× 每族 10 条确定性调度 × 4 路实现 = 120 例，环境是可控程序的确定性解释器 + 三类宿主钩子点 + 只读存储日志的隐藏检查；**族与调度写在代码里**（`crates/jh-app/src/c_program.rs`，不像族 A / B 走入库 JSON），落地状态与读数见 §5 M3。**N = 10 是先导规模，不是冻结规模**。
- 任务先在模拟与可控程序中完成，再考虑真实仓库迁移（研究目标 §9.3 末段；迁移属研究目标 §12 的 M5，不在 1.0）。
- 实现落点：`jh-app` 实验层（快照、扰动、隐藏验收、计价的工具动作）；族 A 见 `crates/jh-app/src/family.rs` + `tasks/family_a.json`（M16），族 B 见 `crates/jh-app/src/family_b.rs` + `tasks/family_b.json`（M19），族 C 见 `crates/jh-app/src/c_program.rs` + `src/c_measure.rs` + `src/h4.rs`（M22，三个模块一组，没有单独的任务集文件）。

### 6.4 数据隔离与持续学习 — 研究目标 §9.5

按研究目标 §9.5 逐条执行：按故障机制/项目/任务族划分开发、验证、测试数据；开发集改系统、验证集选阈值与晋升规则；最终测试前冻结系统版本、初始问题库、分析脚本与主要指标；分开报告「冻结知识库的迁移测试」与「允许在线学习的任务序列测试」；学习产物跨组隔离。实现落点：`jh-app` 实验层与任务环境。

### 6.5 指标 — 研究目标 §10.1、§10.2

- 主要指标（真实任务成功率、旧依据行动率、恢复延迟、总研究执行成本、端到端延迟、未见任务上的精化收益、观测能力累积收益）按研究目标 §10.1 定义采集，**完成声明不构成证据**。
- 诊断指标（漏失效/误失效、重算量与缓存复用率、新问题的消费者与晋升/停用、观测器检出/误报、概率指标按问题族报告等）按研究目标 §10.2 采集；无法获得可信真值的指标必须标明人工标注或代理测量的局限。**M19 的 H3 报告已按这条口径给出「收益与成本并排」的净读数**（验收 / 错动作 / 恢复 / 观察成本 / 调用数，成本侧含生成、验证与影子；逐判定器、逐对、逐版本、逐 B1/B2 子集分列，§5 M2）。**M22 的 H4 报告按 §7.4 报告了误放行率、误报率、迁移矩阵、维护开销与建设成本摊薄曲线**（分母取自隐藏检查，失败态单列；逐族、逐臂、逐调度、逐迁移格与逐维护形态分列，§5 M3）。观测器的检出率与误报率**没有可信真值的替代来源**——真值只能是独立隐藏检查，报告里写明它与观测器不同模块、不同数据通道（§5 M3 的结构性前提、§4 第 17 行）。**M26 的 M4 报告按这两条口径给出主对照（四臂）、诊断对照、原生基线、族 C 判读、复用消融与逐块成本账**（逐格 / 逐块分列，失败案例与限制单列），并明确「未见任务上的精化收益」与「观测能力累积收益」两项 §10.1 指标在本批**不产生读数**（§5 M4、报告 §3–§6）。

### 6.6 统计冻结与结论等级 — 研究目标 §10.3、§10.4

先做小规模先导实验估计方差、成本与可行样本量，随后在独立最终测试前冻结主要指标、最小有意义效应、非劣界限、预算、停止条件、随机种子方案与分析方法（研究目标 §10.3）；结论只在「获得支持 / 条件性支持 / 不支持或被否定 / 证据不足」四档内表述（研究目标 §10.4）。**本文不指定上述数值**：具体数值由先导数据与项目资源确定（研究目标 §10.3 末段）。**已落（2026-09-23，M4）**：先导 = `docs/research/m4-pilot.md`（估方差与成本），随后在定稿主跑**之前**冻结 = `docs/m4-freeze.md`（冻结提交 `f72d62f`；主要指标口径、H5 两条判据与 δ_NI=0.10、最小有意义效应 Δ=0.15、预算与五条停止条件、种子方案、分析方法），定稿读数与结论档位 = `docs/m4-report.md`（§5 M4）；**冻结之后没有改动任何口径**（执行编排上的实际做法登记在报告 §12）。

### 6.7 RQ / H 的落点映射

| RQ / H | 阶段 | 对照或消融 | 任务族 | 主要读数（研究目标 §10） |
| --- | --- | --- | --- | --- |
| RQ0 / H0 | M0 | 判断器基线（研究目标 §9.1 四组共用的判断层） | 初始任务集 | §10.1 质量/延迟/成本；§10.2 概率指标 |
| RQ1 / H1 | M1 | 关闭依赖失效传播（研究目标 §9.2） | A（研究目标 §9.3） | §10.1 旧依据行动率、恢复延迟；§10.2 漏失效/误失效。**本环境先导读数（2026-09-22，仅配对内比）**：旧依据行动率 0.000（开）vs 0.056（关）；恢复在增量配对 8/8 vs 4/8、在全量配对 8/8 vs 8/8；漏失效 0、误失效 1、无关结果保留 22（八臂相同） |
| RQ2 / H2 | M1 | 每次全量重算（研究目标 §9.2） | A | §10.2 重算节点数/输入量/缓存复用率；§10.1 成本与质量。**本环境先导读数（同批）**：H1 开启时增量省约 32% 调用（21 vs 31）与 33% 输入（9615 vs 14277 字节）、判断节点 29 vs 46、复用率 0.447 vs 0，质量与验收/恢复相同；H1 关闭时方向反转（验收 0/6 vs 5/6、恢复 4/8 vs 8/8） |
| RQ3 / H3 | M2 | 固定问题集合（研究目标 §9.2） | B（B1/B2 分开报告） | §10.1 未见任务上的精化收益；§10.2 新问题晋升/迁移。**本环境先导读数（2026-09-22，M18 / M19 真调，两个判定器读数一致）**：固定臂（只启用 v1）验收 0/6、恢复 3/9（只有 3 次观察）；精化臂（晋升 v2）6/6、9/9；B1 各 0/3 → 3/3（判断调用 3 vs 3）、B2 各 0/3 → 3/3（3 vs 6，观察 3 次 × 3 单位两臂相同）；精化循环成本 27 次调用 / 32104 token（生成 3 + 验证 12 + 影子 12），本次运行合计 60 次调用 0 失败；三对全部晋升 v2、迁移用例一致率 1.000。**口径**：0/6 与「恢复 = 观察次数」是读取范围 + 材料充分性判据（`MaterialGate`）的推论，差值口径 = 启用的读取范围是否包含决定性对象，不是「精化一般有价值」的证据，也不能外推到其它任务族（§5 M2 的结构性前提） |
| RQ4 / H4 | M3 | 只能使用已有观察工具；使用通用测试生成（研究目标 §9.2） | C | §10.1 观测能力累积收益；§10.2 检出/误报/测量扰动。**本环境先导读数（2026-09-23，M21 / M22 真调，权威样本 `h4-1790143399409`）**：判据用的误放行率——固定臂 **45.8%（33/72）**、通用测试生成臂 **0.0%（0/72）**、观测器臂 **0.0%（0/72）**；另一种读法（检出率）——54.2% / 100% / 33.3%（观测器 **60/120 弃权**，是失败态而不是检出，也不是放行）；两侧读数 60.4% / 50.0% / 54.2%；迁移（没为它建、直接套用）非对角线六格检出 0.0–20.8%、误放行 12.5–33.3%；维护——重挂钩子点后全批检出 **30.56%（22/72）→ 66.67%（48/72）**，拿掉 `commit_counter` 后误放行 **0% → 66.67%（48/72）**。**结论（报告原文）：「不支持累积能力假说」**——`improves_results = false`、`improves_budget = false`（每检出一次所花生成调用数 0.0417 vs 0.125）。**口径**：分母取自隐藏检查；N = 10 只是先导规模，一次没检出只读作「这条调度上没检出」；通用臂的 0% 误放行是退化读数（干净用例 48/48 全判违反）；契约与测试各只有一次真实生成、无稳定性采样（跨运行观察：另一次样本 3 份契约有 1 份被拒、观测器检出随之 23.6%）；三个族是同环境里的三条顺序约束，不可外推（§5 M3） |
| RQ5 / H5 | M4 | 固定频率检查（研究目标 §9.2）+ 密度扫描（研究目标 §9.4） | A/B/C 汇总 | §10.1 质量—成本—延迟折中；结论按 §10.4。**本环境读数（2026-09-23，M24 / M26 定稿批次，冻结之后、按研究目标 §10.4 的四档）**：**H5 = 获得支持**——两判据（质量非劣、成本更省）跑前写死在代码里、跑后都满足，四条量化比较的区间下界（+0.000、+0.000、−0.041、+0.051）都 ≥ −δ_NI=−0.10；读数：规模 3 验收 50.0% vs 0.0%、规模 6 验收 50.0% vs 16.7%（LLM）/ 8.3%（Jev），判断请求 14 < 17 与 27 < 35、重算输入 6549 < 14213 B 与 11544 < 26378 B；定频臂还多出 3–4 个「不该发出的动作」（失效那次重查没做）。**边界**：每格 1 次、只两个依赖维护臂、只族 A / 族 B、只 `every-2` 这一档定频（报告 §2.3）。**同批其余档次（探索性，不参与 H5 判定）**：结构维条件性支持（区间下界 −0.040 未排除零）、判断通道维仅经济收益、跨任务复用仅成本且只一族有判别力、族 C 观测能力累积不支持、原生基线证据不足（报告 §6、§5 M4） |

**归因规则**：若只有 Jev 调用更快，结论限定为经济收益；若架构在多个判断器上都有收益，归因于架构；若只有组合在特定预算下可行，报告交互作用与适用条件（研究目标 §10.4 末段）。

**读数状态**：上表 H1 / H2 行括注的是 M16 先导批次的**本环境读数**（6 任务、单日单次运行、真实判定器 `jev-1.13.0` / `deepseek-flash`；出处、成本、provenance 与限制见 §5 M1）——只作基线，不构成研究结论，也不构成对 RQ1 / RQ2 的回答；结论按报告 `pairings` 的配对读，跨配对差异是交互作用。H3 行括注的是 M19 先导批次的**本环境读数**（3 对 × 2 个未见变体、单日单次运行、同一批候选产物；出处、成本、provenance 与限制见 §5 M2）——同样只作基线，不构成研究结论，也不回答 RQ3 / H3 本身；该批的差值口径受结构性前提约束（见 §5 M2），跨任务族不可外推。H4 行括注的是 M22 先导批次的**本环境读数**（3 个故障族 × 10 条确定性调度 × 4 路实现 = 120 例、单日单次真调、权威样本 `var/h4/h4-1790143399409.json`；出处、成本、provenance 与限制见 §5 M3）——只作基线，不构成研究结论，也不回答 RQ4 / H4 本身；该批结论是**否定结论**（「不支持累积能力假说」），按研究目标 §3.2 / §10.4，否定结果与正向结果同为完成，不得调参凑正向。H5 行括注的是 M24 / M26 定稿批次的**本环境读数**（2026-09-23；冻结之后、按冻结口径执行，7 块 / 1858 次调用 / 失败 5；出处、成本、provenance 与限制见 §5 M4 与 `docs/m4-report.md`）——它是本表唯一按 §10.3 统计冻结跑出来的读数，且 H5 是本批**唯一的确认性检验**；同批其余档次（结构维、判断通道维、复用、族 C、原生基线）都标**探索性**、不据此声称「获得支持」（报告 §6 与冻结文档 §2.4）。四档结论表述只在研究目标 §10.4 的框架内做，且**统计冻结已落**（`docs/m4-freeze.md`）。

### 6.8 1.0 的实验纪律

不写未冻结的数字；不用刻意削弱的基线制造优势（研究目标 §9.1）；不在最终测试上继续调系统（研究目标 §9.4）；模拟器真值与延迟扫描只用于诊断、不冒充真实部署性能（研究目标 §9.2）；每项结论附原始数据、脚本与版本（[Jev 调研报告](jev-research.md) §14.3 前言的口径）。

### 6.9 不进主线实验的诊断项

- **E4 注入样本改为可选诊断项**：1.0 不含门控场景（决定 2），[Jev 调研报告](jev-research.md) §14.3 E4 的「门控漂移」口径在 1.0 失去对象，因此**不进主线实验清单**；保留为 **M4 可选诊断项**，口径改为「不受信内容进入判定 state 时，判定与概率的漂移」。若不安排，须在实验报告中写明理由，不静默消失（见 §12 未确定项 13）。**M4 的处置（2026-09-23，M26）：未安排**，理由逐条写进 `docs/m4-report.md` §11（1.0 不含门控场景使原始口径失去对象；改后的口径要一对「同一事实、附加 / 不附加当前计划」的提示词，本批 runner 没有构造该变量的入口；额度已被主检验占满），并留作 1.x 的**独立离线诊断**（同一 state 两版提示词，N=2 通道 × M≥10 样本），不静默消失。

## 7. 交付物与完成定义

### 7.1 交付物（对照研究目标 §14.1 逐条处置）

| 研究目标 §14.1 | 交付物 | 1.0 处置 |
| --- | --- | --- |
| 1 | 研究目标文档及版本变更记录 | 已存在（`docs/RESEARCH_GOALS.md`），由后续版本维护，不在本路线图范围 |
| 2 | 核心对象、状态迁移、问题发布、观测器发布与行动复核的架构说明 | 本文件 §3–§4 为 1.0 版；实现落地后按实际回写 |
| 3 | 可批量运行的 Rust 实验核心及其 TUI | **两者都在 1.0 交付**（2026-09-23 决策变更，见 §5 首段）：核心 = `jev exp` / `run` / `compare` / `h3` / `h4` / `m4`（§5 各阶段的落地状态），TUI = `jev chat` 交互会话（LLM 主循环 + Jev 门控 + 审批卡）+ TUI 主界面与首屏 + 三个研究面板接真实数据（M25，merge `bc66d18`；读数见 [TUI 界面设计](tui-design.md) §9.1）。原「TUI 推迟到 1.x」（决定 4）作废 |
| 4 | 五种判断器的适配接口 | **M0 已交付**（`crates/jh-app/src/judge/`，见 §3.2） |
| 5 | 可快照、可扰动的任务环境、开发/验证/测试划分与隐藏验收 | M0 已交付最小一层（固定问题集、隐藏真值与校验、指标统计与 JSON 报告，见 §5 M0 落地状态）；**M16 交付任务族 A 的快照 + 脚本扰动 + 隐藏真值 + 独立验收 + 计价动作**（`crates/jh-app/src/family.rs` + `tasks/family_a.json`，见 §5 M1）；**M19 交付任务族 B 与族 B 的开发 / 验证 / 测试划分**（3 对成对场景、开发案例 vs 未见变体、计价的观察动作，见 §5 M2）；**M22 交付任务族 C 与它的隐藏验收**（隔离实验环境：可控程序 + 三类宿主钩子点 + 只读存储日志的隐藏检查，3 个故障族 × 每族 10 条确定性调度 × 4 路实现 = 120 例，见 §5 M3）；族 A 的开发 / 验证 / 测试划分仍未做 |
| 6 | 主要对照、关键消融、统计分析与复现脚本 | **M4 已交付**：`jev m4` 的主对照 / 诊断对照 / 原生基线 / 两条残余消融 / 密度扫描（M24，见 §5 M4）；**统计分析与参数冻结** = `docs/m4-freeze.md`（冻结提交 `f72d62f`，写在定稿主跑之前，见 §6.6）；**复现脚本与原始数据位置** = `docs/m4-report.md` §7（逐块 `jev m4 …` 命令、离线回放核验与端到端测试命令；两个读数提取脚本是临时脚本、不入库）。此前的三条：**M16 已交付 H1/H2 的两项消融与四组臂对照**（`jev compare`，见 §5 M1）；**M19 已交付 H3 的「固定问题集合」消融与两臂对照**（`jev h3`，见 §5 M2）；**M22 已交付 H4 的两项消融与三臂对照**（`jev h4`，含迁移、维护与摊薄读数，见 §5 M3） |
| 7 | 按版本保存的问题库、观测器库及其验证记录 | **问题库已交付（M18 / M19）**：形态 = 入库的版本化 JSON（`crates/jh-app/tasks/family_a.json` / `tasks/family_b.json`）＋运行期的 `QuestionLibrary`（候选 / 影子评估 / 启用 / 停用 / 被替代，发布与回退联动失效，验证记录落在 `ValidationRecord` 里，见 §3.4、§12 未确定项 5）；**观测器库已交付（M21 / M22）**：数据形态 = `ObserverLibrary` 的 JSON 往返（契约 + 检验记录 + 生命周期历史），生命周期与版本号规则见 §3.4，启用要求四路全过的独立检验记录（读数落在 `ObserverEntry::checks` 与报告的 `verification` 段）；**入库的观测器库文件形态仍未定**——1.0 的 h4 批次没有把观测器库单独落盘，M4 的族 C 契约产物同样只落运行目录（`var/m4/final/c1a/contracts.json`，全批复用同一份）（§12 未确定项 5） |
| 8 | 实验报告（效果、成本、适用边界、反例、失败原因、结论等级） | **M4 已交付，文件名定案**：`docs/m4-report.md`（M4 定稿批次的权威报告——效果、成本、适用边界、失败案例、限制清单与按 §10.4 四档的结论）；配套的**冻结记录**是 `docs/m4-freeze.md`（写在定稿主跑之前，见 §12 #6）。M0 / M1 / M2 / M3 的读数落在 `var/exp/<run>.json`、`var/compare/<run>.json`、`var/h3/<run>.json` 与 `var/h4/<run>.json`（运行产物，不入库，关键值已抄进本文件 §5）；H4 报告按 §7.4 给出了误放行 / 误报 / 迁移 / 维护 / 摊薄的逐格读数与否定结论 |
| 9 | 一个完整演示：反例出现后新增区分维度或观测能力，并在未见任务上展示效果或失败 | M2 与 M3 联合，M4 呈现；**M2 的一半已交付**：反例 → 新候选问题（读法里加上关键证据）→ 晋升 → 在未见变体上展示效果（§5 M2）；**M3 的另一半也已交付**：声明式测量契约 → 四路独立检验 → 在三个并发故障族上对照 → 结论是否定的（「不支持累积能力假说」，§5 M3）。**联合演示（同一次运行里既精化问题又建设观测器）= M4 未交付**：M4 的两批都没有这一项——定稿批次按臂 / 用途分块（`docs/m4-report.md` §1.2 的七块），不含问题集合精化（问题库固定 v1）也不含观测器建设（族 C 只做当日判读与复用消融）；先导文档自己已把它列入「本轮没做的」（`docs/research/m4-pilot.md` §8.9 第 9 条），报告 §3.4 同时写明「未见任务上的精化收益」与「观测能力累积收益」两项 §10.1 指标**本批不产生读数**。因此 1.0 的这一项交付物**未达成**，登记在 §9 与 §11 |

### 7.2 完成定义

- **研究侧**：研究目标 §14.2 的六个问题在 1.0 已实现的机制范围内都有可核查答案，或明确标注为「证据不足」（研究目标 §10.4）；研究目标 §12 的 M5 外部有效性问题除外。负向结果与正向结果同为完成（研究目标 §3.2、§10.4）。
- **工程侧**：`cargo fmt` / `cargo clippy` / `cargo test` 全绿（[AGENTS.md](../AGENTS.md)）；`jev exp` 能跑完至少一个任务族并产出 JSON 报告；报告含原始数据位置与复现命令。**M16 起 `jev compare` 已能跑完任务族 A 的全部四个对照臂并产出配对报告**（见 §5 M1）；**M19 起 `jev h3` 已能跑完任务族 B 的固定 / 精化两臂并产出含收益与成本净读数的 JSON 报告**（见 §5 M2）；**M22 起 `jev h4` 已能跑完任务族 C 的三臂并产出含误放行 / 误报 / 迁移 / 维护开销与建设成本摊薄的 JSON 报告**（见 §5 M3）；**M24 起 `jev m4` 已能跑完主对照 / 诊断对照 / 原生基线 / 两条残余消融 / 密度扫描并产出含 §10.1 / §10.2 读数的 JSON 报告**（见 §5 M4）；**M25 起 `jev chat` 已能在真实终端跑起带 TUI 的交互会话**（模型主循环 + Jev 门控 + 审批卡 + 三个研究面板接真实数据，见 §5 M4 与 §7.1 #3）；当前测试基线 26 个 test target、**512 passed / 0 failed / 7 ignored**（读数出处 `docs/m4-report.md` §7 的本批复核，基线提交 `bc66d18`）。
- **文档侧**：本文件的架构与运行时规格按实际实现回写；结论与未确定项同步到[项目笔记](project-notes.md)或研究目标的新版本（具体归属待人工确认，见 §13）。**M0 已经完成三轮回写（2026-09-22：实现回写 + 真调基线回写 + LLM 输出预算修复回写）**：§3.1 / §3.2 / §3.4 的命名与模块、§1.2 的 provider 与配置口径、§1.3 的现状表、§5 M0 的落地状态与复现命令、§12 未确定项 1 / 2 / 7 已由 M9 / M10 决定并写入；随后真调批次把未确定项 10 升级为「实测确认可用」，并在 §1.2 / §5 M0 写入 Jev / LLM 实测读数、两条新登记限制与 §12 #11 的现状更新；第三轮（M14）改了 `jh-app` 的 LLM 判断器默认输出预算、把 LLM 侧真调读数写回 §5 M0 并新登记契约漂移这一残留失败（变更逐条见 §11）。**M1 已完成回写（2026-09-22，M17）**：§3.1 / §3.3 / §3.4 写入 M15 / M16 的实际模块与类型、§4 补落地进度与「完成声明的证据要求」行、§5 M1 写入落地状态与 H1/H2 配对读数、§6.7 补读数、§1.3 / §1.4 / §7.1 更新现状，§12 未确定项 1 / 2 / 3 / 5 / 8 / 9 结案或回填（变更逐条见 §11）。**M2 已完成回写（2026-09-22，M20）**：§3.1 / §3.4 写入 M18 / M19 的实际模块与类型、§4 补落地进度、§5 M2 写入落地状态与 H3 两臂读数（含结构性前提与成本口径）、§6.2 / §6.3 / §6.5 / §6.7 补落点与读数、§1.3 / §1.4 / §3.1 目录树 / §7.1 更新现状，§12 未确定项 5 结案、2 回填落盘形态，§9 更新 M1 遗留与 M2 新限制（变更逐条见 §11）。**M3 已完成回写（2026-09-23，M23）**：§3.1 / §3.4 写入 M21 / M22 的实际模块与类型（观测器契约与生命周期、行为分类、可控程序与三类钩子点、测量契约与四路独立检验、H4 三臂编排）、§4 补第 10 / 11 / 17 行与落地进度、§5 M3 写入落地状态与 H4 三臂 / 迁移 / 维护 / 摊薄读数（含权威样本声明、结构性前提与诚实性五条）、§6.2 / §6.3 / §6.5 / §6.7 补落点与读数、§1.3 / §1.4 / §3.1 目录树 / §7.1 更新现状，§12 未确定项 5 的观测器一侧结案、2 / 8 回填落盘形态与先导规模，§9 更新观测器自证与竞态两类风险与 M2 遗留（变更逐条见 §11）；每阶段合并在同一位置继续回写。**M4 已完成回写（2026-09-23，M27，含 TUI 决策变更）**：§5 首段的「TUI 偏差（决定 4）」改写为「**TUI 决策变更（用户指令）**」并写明交付形态与读数出处，§1.1 / §1.2 / §1.3 / §1.4 / §2.4 / §3.1（表与目录树）/ §7.1 #3 / §8 #1 / §10 / §13 #5、#9 同步；§3.4 写入 M24 / M25 的实际模块（`m4`、`session/**` 与 `jh-tui` 的扩展）；§4 落地进度把第 15 行的统计冻结结案；§5 M4 写入落地状态与四档结论（H5 获得支持；结构维条件性支持；判断通道维仅经济收益；复用仅成本且只有一族 12 例有判别力；族 C 不支持；原生基线证据不足）、定稿批次规模与预算遵守、未交付的联合演示与 TUI 交付读数；§6.2 / §6.5 / §6.6 / §6.7 / §6.9 补落点与读数；§7.1 #6 / #7 / #8 / #9 更新（报告文件名定案、`docs/m4-freeze.md` 是冻结记录、**联合演示未交付**）；§9 新增 M4 的五条 backlog 并把 A4 第 1 轮 `rollback-cache` 的复核结论写成「未覆盖」；§12 #6 结案、#5 / #8 / #13 按 M4 落地更新（变更逐条见 §11）。

## 8. 明确不做（1.0 non-goals）

1. ~~**TUI 研究视图**：`jh-tui` 冻结保留（决定 4）；研究目标 §11 的视图与操作推迟到 1.x。~~ **已移出非目标（2026-09-23 用户指令，见 §5 首段）**：TUI 加入 M4 并**已交付**——`jev chat` 交互会话 + TUI 主界面/首屏 + 三个研究面板接真实数据（§7.1 #3）。1.0 **做** TUI；但研究目标 §11 里尚未设计的操作（问题版本比较、目标与验收条件、观测器契约、成本与执行时间、环境分叉）仍推迟到 1.x（[TUI 界面设计](tui-design.md) §9 批次五「未开始」、§10 未确定项 1）。
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
| 实验设施是最大工程量（任务环境 / 真值 / 隐藏验收大部仍缺） | 研究目标 §12 M0–M3；本文 §1.3 #6 | M0 已交付最小一层（固定问题集、隐藏真值与校验、指标统计与 JSON 报告，见 §5 M0 落地状态）；**M16 已交付任务族 A 的一层**（快照 + 脚本扰动 + 隐藏真值 + 独立验收 + 计价动作，见 §5 M1）；**M19 已交付任务族 B 的一层与全流程编排**（成对场景、计价的观察动作、开发案例 / 未见变体分开、候选生成到晋升回退，见 §5 M2）；**M22 已交付任务族 C 的一层与观测能力的实验设施**（隔离实验环境 + 三类宿主钩子点 + 只读存储日志的隐藏检查 + 声明式测量契约的生成与独立检验，见 §5 M3）；**族 A 的开发 / 验证 / 测试划分仍缺**；无 Jev 访问时按研究目标 §12 的 M0 退路继续模拟 |
| 完成声明被自证 / 原目标被改写成代理目标 | 研究目标 §7.3、§13.2 失败模式表 | 验收由独立隐藏检查固定（研究目标 §7.3、§9.3）；Agent 完成声明不构成证据（研究目标 §10.1）；精化不得改目标或放宽验收（研究目标 §6.4）。**M16 已把这条机制化**：任务族 A 的独立验收（`crates/jh-app/src/family.rs:1002` `accept`）只读证据日志与隐藏真值，完成声明只计数；交付物被取代后，只按旧版本判定过交付物的运行拿不到验收（§4 第 16 行、§5 M1）。**M19 把同一条约束接到精化流程上**：候选契约里没有任何字段能改用户目标或验收要求，发布前后逐项核摘要（用户目标 / 验收要求 / 工具清单 / 控制规则），选项标签集合原样保留（§5 M2）。**M21 / M22 把它接到观测能力上**：§7.2 的八条契约在类型上是必填字段，观测器**进不了启用态**除非四路独立检验全过（`Error::ObserverNotVerified`）；第四路按隐藏检查判、不是按观测器自己的输出判；§7.3 的「不得把要求换成『最终缓存为空』」由 `BANNED_SUBSTITUTIONS`（代码常量）与报告里的 `weakened_substitution` 承担（§4 第 17 行、§5 M3） |
| 观测器自证风险：仪器自己说自己行（研究目标 §7.3 末段、§13.2） | M21 的类型约束（`crates/jh-core/src/observer.rs:688` `ObserverLibrary::enable`）；M22 的实测（§5 M3 的结构性前提） | 启用门槛要求 `ObserverCheck` 四路齐备且全过：阳性对照（合理实现必须通过）、阴性对照（已知错误实现必须被抓到）、可注入故障（注入之后必须被检出）、**独立隐藏检查**（绕过记录器或篡改日志时，由存储日志通道发现真实副作用）。第四路**期望的是隐藏检查而不是观测器**——观测器可以什么都看不到，那也不能让这一路变成通过；自证式的伪隐藏检查被拒绝并把失败记录留档（M22 实测：三族的自证式检查都被 `ObserverNotVerified` 拦下、生命周期停在「验证」、库里留一条检验记录）。实测读数也照实报：记录器被绕过时观测器**拒绝作证**（60/120 弃权按失败态计，既不折成「成立」也不折成「违反」） |
| 竞态的不确定性：一次没检出不能读作「不存在」 | 研究目标 §10.4；M22 的重复协议（§5 M3） | 族 C 的竞态**不靠掷骰子重复**：每族 10 条不同的确定性调度，重复即可复现；机会数（分母）由隐藏检查认定，逐族逐调度给 n/N，机会数为 0 时记 null 而不是 0。**单次运行读数不得当稳定值**；N = 10 是先导规模，不是冻结规模（统计冻结仍属 M4）。报告同时给三种读法（误放行率 / 检出率 / 两侧读数），理由写在报告 `caveats` 里：只看一侧错误率会被「一律判违反」的臂骗过去（本次通用测试生成臂对干净用例 48/48 全判违反、误报率 100%） |
| 真值 / 评分标签泄漏进正常输入 | 研究目标 §13.2 | 隐藏真值与正常输入隔离；外部内容只作证据，不作权限来源 |
| 长 state 容量与 context rot | [Jev 调研报告](jev-research.md) §4.3、§13.4 #10 | 1.0 优先小规模受控任务；state 规模上限由实测曲线决定（未定，§12） |
| 范围蔓延 | 决定 1/4 + 本文 §8 | §8 为硬边界；新增内容进 1.x 待办 |
| 已知限制：任务族 A 的材料有候选集诱导（M16 先导发现）——**M2 已处置** | `.tower/comms/findings/20260922-agent-m1app-idea-a.md`；§5 M1、§5 M2 | 材料不足以归因时，两个真实判定器都倾向从候选集挑标签而不选 `other`。M16 先把行动前提门收窄到 `cause_primary`、`premise_holds` 只作诊断读数；**M2（M19）把「未列入候选集」的判据写进了问题定义**：族 B 的三个门控问题的 `other` 判据写明「读到的记录里没有任何一条能把候选区分开（含关键证据尚未取得）时必须选 other；判据是『需要的那份证据不在这次读到的材料里』，不要从候选集里挑一个最像的标签」（`crates/jh-app/tasks/family_b.json` 的 `questions[].question.criteria.other`），并由 `MaterialGate`（`crates/jh-app/src/family.rs:182`）把「关键对象不在该题输入投影 → 正确读法是 `other`」变成可判据的事实；「材料不足却读出候选集内标签」作为诊断读数随族 B 报告（M19 批次：两臂都是 0 次，固定臂材料不足 6 次、精化臂 3 次）。注意族 A 的材料措辞未改（其问题定义自 M16 起只写「证据不足以归入以上任何一类」，且族 A 未声明 `MaterialGate`），这条处置落在族 B |
| 已知限制：先导批次里有一个与缺陷传播无关的错误动作 | M16 读数（§5 M1）；M19 读数（§5 M2）；**M4 复核（2026-09-23）** | jev 在 H1 开启的增量臂上 A4 第 1 轮发出 `rollback-cache`（不是过期依据，而是提前行动）；先导样本量下分不清是判定漂移还是门控口径问题。M2 的 H3 批次里两臂的「不该发出的动作」都是 0（族 B、6 个未见变体），仍不足以定性，留待 M4 在更大样本上复核。**M4 复核结论：未覆盖**——定稿报告只给格汇总读数（族 A 的「不该发出的动作」：依赖维护两臂 **0**、循环式两臂 3；「被有效性检查挡下的动作」依赖维护臂 3、循环式臂 0，报告 §3.1 / §4.1），没有逐任务拆到「A4 第 1 轮」这一形态；且 M4 的循环式臂是 `h1off-h2full`（`docs/research/m4-pilot.md` §2 的臂定义），与 M16 那条 H1 开启的增量臂不是同一配置。因此这条**仍不足以定性**，留给 1.x（登记见 §11 本批条目） |
| 已知限制（M2 新登记）：固定臂的 0/6 是结构性推论，不是能力差距 | §5 M2 的结构性前提；报告 `protocol.arms` 与 `notes` | 族 B 的固定版本 v1 只读 `app.log`（措辞对两侧相容），材料充分性判据把「关键对象不在投影里」读作 `other`：固定臂的判断按材料调整后的真值全对，但前提门要侧标签，补救动作发不出；B2 的观察由 `other` 把关，固定臂唯一能发的必需动作就是那几次观察。**「恢复 = 观察次数」与差值口径（= 启用的读取范围是否包含决定性对象）都由此而来**，不得当作「精化一般有价值」的证据读，也不能外推到其它任务族；精化臂还依赖「证据需求未满足就重问」这一机制成分（两臂都生效） |
| 已知限制（M2 新登记）：影子判断的「若采纳」只按门判据重算 | §5 M2；报告每条 `branches[].detail`（「`Executed` 不含发出前的前提复核」）与 M19 登记的已知限制 | 影子判断落在独立状态里、控制路径只读主状态（结构隔离成立），但「若采纳影子答案会怎样」只按**门判据**重算，**不含发出前的前提复核**（H1 的机制）。它不影响 H3 的臂读数（影子只喂晋升判据 8 条），但读影子分支改变时要带上这条限定。**2026-09-23 M23 复核：M21 / M22 没有触碰这条路径**——`crates/jh-app/src/matching.rs:1063` / `:1138` 的口径原文未变，现状不变（M22 的族 C 对照不走影子） |
| 已知限制（M2 新登记）：`MaterialGate` 是族 B 场景声明的事实；「证据需求」的时效与获取方法仍未建模 | §5 M2；`crates/jh-app/src/family.rs:182`；M18 登记的缺口（`crates/jh-core/src/library.rs` 模块文档） | 材料充分性判据由任务族自己声明（族 B 声明、族 A 为空），不是运行时从证据库里推出来的；B2 的观察动作靠 `ActionSpec::effect` 产出证据（族 A 的动作无产出、行为不变）。研究目标 §6.3 要求的「时效」与「获取方法」在 `jh-core` 的 `NewQuestion` 里仍只有来源与字段，M19 把这两项放在候选契约的 `evidence_freshness` / `evidence_method` 里承载并在报告里说明——**这是登记在案的缺口，不是已经做到**。**2026-09-23 M23 复核：现状不变**——`evidence_freshness` / `evidence_method` 仍只出现在 `crates/jh-app/src/generation.rs:54` / `:56` 的候选契约里，`jh-core` 侧未建模；族 C 的观测需求走的是 `ObserverContract`（契约八条），不经过 `NewQuestion` |
| backlog（M2 遗留，2026-09-23 M23 复核仍成立）：`compare::logged_calls` 已无调用点 | `.tower/comms/findings/20260922-tower-improve-jh-app-compare-logged-calls.md`；`crates/jh-app/src/compare.rs:1417`（定义，仍是 `pub`）、`crates/jh-app/src/h3.rs:2365`（唯一调用点，用的是 `logged_calls_of_run`） | `logged_calls`（裸 run 标识精确匹配）在 M19 之后就没有调用点，`logged_calls_of_run`（按 `{run}-` 前缀匹配）取代了它的用途；两者的语义差一个前缀，留着容易被误用（精确匹配对带 `-dev` / `-validation` 等后缀的 run 标识恒为 0，正是 M19 round 1 报出的那个 bug 的来源）。本批**只复核、不改代码**（本批只改文档）：M21 / M22 没有动 `compare.rs`，两个函数都在，调用点仍是零 |
| backlog（M3 新登记，2026-09-23，认领 M22 r3 评审留给 M23 的 (a)）：离线复跑的 H4 报告里产物路径指向不存在的文件 | M22 r3 评审的「另记」项 (a)；`var/h4/replay.json` / `var/h4/replay-rejected.json` 的 `raw.contracts_file` / `raw.tests_file`；实现见 `crates/jh-app/src/h4.rs`（复用产物分支） | 用 `--contracts` / `--tests` 复用已落盘产物跑出来的报告，其 `raw.contracts_file` / `raw.tests_file` 指向**它自己的** `out_dir` 下并不存在的文件（路径按本次运行的 out_dir 拼，而本次运行只复用输入、不重写产物）；`h4-1790143399409.json` 与 `final.json` 的这两个路径存在（真调批次写了产物）。影响仅限「照报告里的路径去取产物」，任何读数都不受影响。本批**只改文档**：§5 M3 的权威样本声明与 §12 #2 各写一句限定（取可直接复用的产物路径看权威样本或 `final.json` 的 `raw` 段）；若将来要修代码，最小改法是复用分支沿用输入文件的路径而不是 out_dir |
| backlog（M4 新登记，2026-09-23）：策略回放的快照标识不含臂标识 | `.tower/comms/findings/20260923-agent-m4app-bug-item.md`；`crates/jh-app/src/matching.rs:807`（快照标识 `{任务}-r{轮}-{读取范围}`）、`crates/jh-app/src/calls.rs:160`（`replay_index` 后写覆盖先写）；报告 §5.2 第 6 条 / §10 第 1 条 | 回放索引的键是（快照, 问题, 问题版本），**既不含臂、也不含检查节奏与证据口径** → 回放不复现臂间差异：索引只放整块 C1a 记录时四臂读数与真调一致（15/18、18/18），并入 C2a + C3 的记录后主对照被改写成 `loop-*` 6/18、`maintained-*` 9/18。影响面：`jev compare --regenerate`（不吃回放）、`jev h3` / `jev h4` 的 `--replay` 与 M4 的 `--offline-judge replay`。**逐臂读数只能来自真调**；本批只登记、不改代码（修法见 finding：把臂标识纳入快照命名，或至少在报告里写明「不复现臂间差异」） |
| backlog（M4 新登记，2026-09-23）：复用消融对非首族问错需求问题 | `.tower/comms/findings/20260923-reviewer-m4report-bug-duplicate-after-restart.md`；`crates/jh-app/src/m4.rs:2607`（`run_reuse_side` 取 `requirement_question(families[0])`），对照 `:1509`（判读臂按族出题）；报告 §4.4 / §10 第 6 条 | 三族都问第一族（`cancel_after_confirm`）的判读题 → `duplicate_after_restart` 的 12 例在复用两侧全部读 `undetermined`（同一批 12 例在按族出题的族 C 判读段读出「干净 6 / 检出 2 / 证据不足 4」），`lost_update` 12 例无材料。后果：**复用消融的「省成本没有换来质量损失」只在 `cancel_after_confirm` 一族的 12 例上有判别力**（报告 §4.4 / §6 已按 12 例口径改写）。本批只登记、不改代码；修法是按 `case.family` 出题并补一条断言，修后重跑需另起一批 |
| backlog（M4 新登记，2026-09-23）：契约取自文件时被拒的族在 `channel_setup` 段整体消失 | `.tower/comms/findings/20260923-agent-m4report-bug-channel-setup.md`（M26 作者报，M26 r1 评审复核确认）；`crates/jh-app/src/m4.rs:2372`（`Contracts::File::of`）/ `:2411`（`setup_channel` 的早退） | `jev m4 --contracts <file>` 运行时，生成阶段被拒的族不在契约文件里 → 报告 `channel_setup` 只剩 2 条，读者无法从该报告知道 `lost_update` 为什么没有通道（生成它的那次运行里它带着拒绝原因出现）。影响仅限「报告不自含」，读数不错（该族 12 个用例仍按失败态计入）。本批只登记、不改代码 |
| backlog（M4 新登记，2026-09-23）：H5 判定文案把「先导」写死 | `.tower/comms/findings/20260923-agent-m4report-improve-h5.md`；报告 §2.2 末段与 §10 第 11 条 | 运行器把 H5 的三档结论文案与报告 `notes` 的「先导规模…」写成固定字符串（`crates/jh-app/src/m4.rs` 的 `hypothesis_section`），定稿批次（R=3、冻结之后）原样输出这两句，引用报告原文时容易把定稿批次读成先导批次。**不影响读数与判据逻辑**；本批只登记、不改代码（改法是让文案随批次身份变化，或去掉「先导」二字） |
| backlog（M4 新登记，2026-09-23）：`crates/jh-app/src/lib.rs` 的模块地图缺 `session` 条目 | M25 评审 r5 的非阻塞项（`.tower/comms/reviews/review-feat-runnable-agent-session-and-tui-wiring-reviewer-sessiontui-r5.md` §4）；`crates/jh-app/src/lib.rs:5-39` 的模块清单 | 模块地图逐个列了 `judge` / `tasks` / `calls` / `metrics` / `report` / `exp` / … / `m4`，`session` 是唯一没有条目的模块（`pub mod session;` 在 `:61`；模块自身文档与 [TUI 界面设计](tui-design.md) §9.1 都有说明）。纯文档注释、不影响编译与行为；按评审建议，在任何再碰 `lib.rs` 的提交里补一句即可 |
| backlog：LLM 契约漂移（`missing field probabilities`） | `.tower/comms/findings/20260922-agent-cap-improve-llm-probabilities-12-2.md`；§5 M0 | M14 的三次运行里 12 次调用有 2 次失败（模型省掉契约里的 `probabilities` 一层，输出仍是合法 JSON、概率和为 1）；解析层维持「形状不符即显式失败」，不改契约、不加兜底。**现状（2026-09-22 M20 复核）**：M19 的 H3 批次里 LLM 侧 15 次判断调用全部成功；生成那一笔（3 次调用）来自同 mission 的候选产物生成批（run 标识 `h3-1790089502590-generation`，本批复用其读数），也是 0 失败；该次运行合计 60 次调用 0 失败。漂移未在本批复现——但样本量太小，不能据此认为它消失了；仍登记为 backlog |
| backlog：`jh-openai` 端点拼接的尾斜杠（M11 finding，**复核后不成立**） | M11 probe 的原始断言与 2026-09-22 复核；`crates/jh-openai/src/client.rs:109`（拼接点）、`:169`（`ClientBuilder::build` 裁剪点）；同类单测的现成样板是 `crates/jh-jev/src/client.rs:156` 的 `systemone_url_normalises_trailing_slashes` | M11 finding 称「端点拼接不裁剪尾斜杠」，复核后与现状不符：`build` 早已 `trim_end_matches('/')`（自首个提交 `807e53b` 起存在，M11 的 HEAD `20d20ea` 也在），调用点看不到尾斜杠，`https://api.deepseek.com/` 与不带斜杠等价——**行为正确**。残余项只剩**缺一条尾斜杠单测**（照 `jh-jev` 的 `systemone_url_normalises_trailing_slashes` 写即可）。2026-09-22 M17 与本批（M20）复核后仍登记为 backlog：`jh-openai` 在 M18 / M19 一字未改，测试树里仍没有这条单测；本批不改代码 |
| backlog：`jh-jev` 未建模 403 | M11 真实探测 2026-09-22；`crates/jh-jev/src/error.rs:4`、`:166-177`；`.tower/comms/findings/20260922-agent-probe-improve-jh-jev-typesafe-403-authorization-403-401.md` | 实测：完全缺 `Authorization` 头时服务端返 403，无效 key 才 401；403 现落入 `UnexpectedStatus`，报错丢失「鉴权」语义（正常路径总带 header，仅在代理 / 网关改写时出现）。修法是把 403 并入鉴权类错误并同步 `error.rs` 顶部注释。2026-09-22 M17 与本批（M20）复核后仍登记为 backlog：`error.rs` 的状态码映射（`:170-176`）里没有 403 这一档——401 → `Unauthorized`、422 / 429 / 529 各有变体、其余（含 403）落 `UnexpectedStatus`；本批不改代码 |
| backlog（文字层，M14 / M15 评审的非阻塞项） | `.tower/comms/reviews/review-feat-fix-llm-judge-output-cap-and-rerun-live-reviewer-cap-r1.md`；`.tower/comms/reviews/review-feat-jh-core-invalidatable-semantic-state-run-reviewer-m1core-r2.md` | (a) `crates/jh-app/src/judge/llm.rs` 的 cap 注释是压缩写法：「8192 → …重跑时 reasoning 用到 8481、11425 token」里的 8481 / 4493 是 **cap 16384** 下的两轮实测、11425 是 **cap 32768** 下的实测——本文件 §5 M0 已写准，代码注释本批不改。(d) 已归档 finding 里「报告…记 `clean=false`」措辞略不准：`clean` 是 CLI outcome / 退出码语义，报告的序列化字段里没有 `clean`（报告侧读数是 `calls.failed` 与退出码）——归档记录不改，口径以 §5 M0 与本节为准。(e) M15 评审的同类 nit：`crates/jh-core/src/error.rs:7-8` 的模块文档枚举仍写「证据日志读写不了、引用了不存在的证据 / 节点 / 行动、问题版本冲突」，未把新增的「往已撤回节点写结果」列进去，下次触碰该文件时补一句 |

上表 backlog 与已知限制都只登记、不在本批修（本批只改文档）：两条 M11 backlog 里，第 1 条经复核「行为正确、只缺单测」（本批再次复核，仍缺），第 2 条（403 未建模）本批复核仍成立；文字层三项见末行，其中 M14 评审的 (b)「约 4.5 ms/token」与 (c)「少引一份报告路径」已在 M17 的 M1 回写里修正（§5 M0 为实测区间 4.38–5.23 ms/token，并补上 `var/exp/run-1790071565046.json`）。M2（M18 / M19）随批登记的限制见上表四行「（M2 新登记）」与 `missing field probabilities` 一行：影子不含发出前的前提复核、`MaterialGate` 是族 B 场景声明、「证据需求」的时效与获取方法未建模、固定臂 0/6 只是结构性推论、真调只跑一次（同一批候选产物）。**M23（2026-09-23）复核**：上表每一行都在当前尖端正核对过一遍——两条 M11 backlog、`missing field probabilities`、`logged_calls` 死代码、影子的前提复核缺口、时效与获取方法未建模，**全部仍成立且本批不改代码**（M21 / M22 没有触碰这些位置）；M3 随批新登记两类风险（观测器自证、竞态不确定性）与一条口径提醒（跨运行观察不得写成代码属性，见本节前两行与 §5 M3）。另有一条口径提醒：轨迹会把配置的 base URL 原样记下（`judge_endpoint`），**base URL 不要内嵌凭据**；key 仍只记布尔（§5 M1）。**M27（2026-09-23）复核**：上表 M4 新登记的五条与 A4 那一行都在当前尖端按代码 / 文档逐条核对过（`matching.rs:807`、`calls.rs:160`、`m4.rs:2607`、`m4.rs:2372` / `:2411`、`lib.rs:5-39` 与 `:61` 均在位；A4 的复核结论按报告只给格汇总读数这一事实写成「未覆盖」），**本批只改文档、不改代码**；M24 / M25 / M26 随批新登记的失败案例与限制见报告 §5.2 / §10 与 [TUI 界面设计](tui-design.md) §10。

## 10. 与既有文档的关系

| 文档 | 关系 |
| --- | --- |
| [研究目标](RESEARCH_GOALS.md) | **上游**：本文是研究目标 §12 的 M0–M4 施工路线与 §9–§10 的实验执行计划；研究目标 §5–§8 是运行时规格与对象清单的来源；本文不改该文档 |
| [项目笔记](project-notes.md) | Jev 定位与项目决策来源；本文直接引用决策 2（供应商/容灾）；与决策 3（判定权）的关系见 §13，决策 2 与调研报告红线 3 的冲突同见 §13 |
| [Jev 调研报告](jev-research.md) | 判断器定位依据 §13（§13.3 架构启示、§13.5 红线）；实测清单 §13.4 与实验目录 §14.3 是 M0–M4 的验证来源；本文不新增其结论与数字 |
| [Grok TUI 设计调研](grok-tui-design.md) | TUI 研究视图的参考（其 §7.3 最小骨架）；**2026-09-23 决策变更后 TUI 已在 M4 交付**（设计基准换成了 [TUI 界面设计](tui-design.md)，`jh-tui` 由 `jev chat` 接线、`jh-app` 侧新增 `jh-tui` / `ratatui` / `crossterm` / `futures` 依赖，见 §5 首段与 §7.1 #3）。注意其 §7.3 推荐 ratatui 0.29 / crossterm 0.28，与工作区现有 0.30 / 0.29（`Cargo.toml`）不一致——以工作区为准，该文档不必改 |
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
- **冻结**：`jh-tui` 组件套件（决定 4）；1.0 交付批处理实验 + CLI/JSON 报告，研究视图推迟 1.x。**（该条已由 2026-09-23 的 M4 回写取消——TUI 加入 M4 并已交付，见下一条变更记录与 §5 首段。）**
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

**2026-09-22 — M14 LLM 判断器输出预算修复与 LLM 真调基线回写（本批改代码）**

- **改 `jh-app` 的 LLM 判断器默认输出预算**：`DEFAULT_MAX_OUTPUT_TOKENS` 由 1024 抬到 16384（`crates/jh-app/src/judge/llm.rs:59`）。依据是实测而不是「防啰嗦」的保守取值：该 provider 的 thinking 模型把 reasoning token 计入 `max_output_tokens`，cap 1024 必然截断（4/4 `Incomplete`）；cap 8192 仍会在 `case-003` 上被截断（重跑时 4 次调用败 1 次），同一提示词在 16384 / 32768 下 `completed`，实测所需 reasoning 最长 11425 token；取 16384 还因为跑满预算约 74 s、仍在 120 s 超时内（32768 跑满会超时，失败形态反而从截断变成超时）。顺带把 `incomplete_details.reason` 写进 `Incomplete` 的失败信息，否则报告自身分不清截断与内容过滤。
- **回写 LLM 真调读数**：§5 M0 的「LLM 结构化判断器（无读数，保持未验证）」段替换为修复后的读数——单跑两次（`var/exp/run-1790071861270.json`：4 次调用 3 成 1 败、一致率 0.889（8/9）、Brier 0.2222、score 期望值误差 0.3333、usage 1914/8692；`var/exp/run-1790071925436.json`：4/4 成功、0.750（9/12）、Brier 0.3437、对数损失 0.3045、score 期望值误差 0.4700、usage 2567/13197）与四判断器合并基线（`var/exp/run-1790071988015.json`：rule 4/4 `simulated`、simulated-truth 4/4 `simulated`、llm 3/4 `verified`、jev 4/4 `verified`）；同时保留「4 样本 × 3 题、单日运行、只作基线、不构成研究结论」的限定。
- **登记残留失败（新 finding）**：16384 下三次运行合计 12 次调用有 2 次失败，原因是模型省掉契约里的 `probabilities` 一层（`missing field probabilities`，输出本身是合法 JSON、概率和为 1）——与输出预算无关，属 prompt 遵循的偶发漂移；本批不改契约与解析层（解析层维持「形状不符即显式失败」），已归档 finding：`.tower/comms/findings/20260922-agent-cap-improve-llm-probabilities-12-2.md`（含三条候选处置路线）。
- **为何**：§7.2 文档侧「按实际实现回写」；研究目标 §12 的 M0 退出证据要求「得到本环境的质量、延迟、成本基线，能区分真实 API 与模拟运行」。截断是判断器默认值的缺陷，用真调证据改掉；改不掉的部分（契约漂移）如实登记，不用外推或模拟读数掩盖。

**2026-09-22 — M1 实现回写（M15 / M16 落地；本批只改文档）**

- **回写实现命名与机制**：§3.1 / §3.3 / §3.4 写入 M15 的 `jh-core` 实际模块与类型（`evidence` / `hash` / `graph` / `projection` / `runtime`，以及 `question` 的版本表 `QuestionRegistry` 与 `error` 的运行时变体；merge `b099109`，复核尖端 `0e7ada2`）与 M16 的 `jh-app`（`family` / `matching` / `compare` + `tasks/family_a.json`；merge `a32b611`，复核尖端 `bc60233`），并写明匹配循环与调度形态（`Arm` 的两个开关是 H1/H2 的唯一分叉点、按读取范围分组、增量与全量各自的询问规则）与问题集与版本规则（同一 ID 改含义必须升版本）。
- **定下未确定项**：§12 #1（M15 的模块与类型名）、#2（事件日志形态：`JsonlEvidenceLog`，落 `--out-dir` 下的 `evidence/` 与 `traces/`）、#3（认识状态**不新增独立字段**，由评估状态 / 依据有效性两个维度加支持与冲突边表达；§4 第 9 行）、#5（问题库在 1.0 的形态 = `crates/jh-app/tasks/family_a.json`）、#9（`jh-core` **不引 `tokio`**）；#8 回填先导规模与依据（6 任务 × 3 轮 = 每臂 18 个决策机会）。
- **回写 M1 落地状态与读数**：§5 M1 增加落地状态——M15 / M16 的交付清单、四组臂 × 两个判定器的读数表（验收 / 恢复 / 旧依据行动率 / 调用 / 输入字节 / 判断节点 / 复用率）、成本与失败（该 run 194 次真调、失败 0）、provenance（报告由 `compare --regenerate` 从该次运行的轨迹重建，`recorded_at` 只标取值时机；该批轨迹产自 `e337f8e`、`judge_endpoint` 为空）、复现命令；§6.7 的 H1 / H2 行补上同样的读数与「只作基线」的状态说明。
- **登记结论口径（按配对读）**：H1 的独立痕迹只有旧依据行动率 0.000 vs 0.056（1/18）与关闭臂 A4 的过期依据动作；H2 的成本优势（约 32% 调用、33% 输入）只在 H1 开启的配对成立，H1 关闭时结果方向反转（验收 0/6 vs 5/6、恢复 4/8 vs 8/8）；跨配对差异是交互作用，不得当主效应读（与报告 `pairings` / notes 6 同口径）。
- **补一行归属**：§4 新增第 16 行「完成声明的证据要求」（研究目标 §5.2 末条，认领 M15 评审 finding）——落点是 M16 的独立验收 `accept`（只读证据日志与隐藏真值，完成声明只计数），§5 M1 / §7.1 / §9 同步。
- **收尾 M0 遗留**：§9 的两条 M11 backlog 复核后维持（`jh-openai` 尾斜杠：行为正确、仍缺单测；`jh-jev` 未建模 403：仍成立），并把 M14 评审的四条非阻塞文字项一并处理——(b)「约 4.5 ms/token」改为实测区间 4.38–5.23 ms/token（中位约 4.7；跑满 16384 约 74–86 s）、(c) cap 依据段补上 `var/exp/run-1790071565046.json`，(a) 代码注释压缩写法与 (d) 归档 finding 的 `clean=false` 措辞只登记（本批不改代码、不改归档记录）；§9 另新增两条 M16 已知限制（材料诱导、与失效无关的错误动作）。
- **更新现状**：§1.3 #5–#7、§1.4、§7.1 #5–#8、§7.2 按 M15 / M16 的实际改动与当前测试基线（**301 passed / 0 failed / 6 ignored**，M17 复核）更新。
- **为何**：§7.2「文档侧：本文件的架构与运行时规格按实际实现回写；结论与未确定项同步到[项目笔记](project-notes.md)或研究目标的新版本」；§3.4 上一版写明「Rust 侧的类型与模块命名在实现时随改动确定并在此文件回写」（§12 未确定项 1）；研究目标 §12 的 M1 退出证据要求「完成失效传播与 H1/H2 对照，包含过期响应和无关变化案例」。先导读数按研究目标 §10.4 只作本环境基线，不冒充研究结论。

**2026-09-22 — M2 实现回写（M18 / M19 落地；本批只改文档）**

- **回写实现命名与机制**：§3.1 / §3.4 写入 M18 的 `jh-core` 实际模块与类型（`counterexample`：`Counterexample` / `NewCounterexample` / `CounterexampleId` / `FailureSource` / `Attribution` / `CounterexampleLog` 与内存 / JSONL 两种持久化；`library`：`NewQuestion` 六项契约、`QuestionEntry`、`Lifecycle`、`LifecycleRecord`、`ValidationRecord` / `ValidationKind`、`QuestionLibrary` 的六个写入入口，以及 `SemanticState::invalidate_question` 的发布 / 回退联动；merge `9acb5dd`，复核尖端 `0c71380`）与 M19 的 `jh-app`（`generation` / `validation` / `shadow` / `promotion` / `family_b` / `h3` + `tasks/family_b.json` + `tests/h3.rs`，以及 `family` / `matching` / `calls` / `compare` / `judge/llm` / `main.rs` 的增量；merge `84c3319`，复核尖端 `330fdf2`）。
- **定下未确定项**：§12 #5 的问题库一侧结案——版本号规则写成五条（每 ID 各自从 1 起单调递增、由库分配；同一版本不得换含义；停用与回退不产生新版本号；旧版本永不覆盖或删除、只改状态；入库文件保存这份编号与状态），观测器库一侧仍未定（M3）；§12 #2 回填反例日志与问题库的落盘形态（`counterexamples.jsonl`、`question-library-{fixed,refined}.json`）；§12 #8 回填任务族 B 的先导规模（3 对 × 2 个未见变体 + 3 个开发案例）。
- **回写 M2 落地状态与读数**：§5 M2 增加落地状态——M18 / M19 的交付清单、H3 两臂 × 两个判定器的读数表（验收 / 恢复 / 调用 / token / 延迟）、B1 与 B2 分列（各 0/3 → 3/3；调用 3 vs 3 / 3 vs 6；观察 3 次 × 3 单位、两臂相同）、三对全部晋升 v2（8 条判据事前声明、四路变更 = 问题 true / 投影 true / 消费者 false / 策略 false、P3 回退演练）、成本口径（精化循环 27 次调用 / 32104 token；本次运行合计 60 次调用 0 失败）与复现命令；§6.2 消融行、§6.3 任务族 B 行、§6.5 指标口径、§6.7 的 H3 行与读数状态、§7.1 #5–#9、§7.2 同步。
- **写清结构性前提（不许省略、也不许读成「精化一般有价值」）**：固定臂 v1 只读 `app.log`、措辞对两侧相容，材料充分性判据（`MaterialGate`）把「关键对象不在该次投影里」的正确读法定为 `other`——固定臂按材料调整后的真值全对，但前提门要侧标签，补救动作发不出；B2 的观察由 `other` 把关。因此「固定臂 0/6」与「恢复 = 观察次数」是读取范围 + 这条判据的推论，差值口径 = 启用的读取范围是否包含决定性对象；精化臂还依赖 `matching` 的「证据需求未满足就重问」（两臂都生效）。
- **登记 M2 限制与未达标判定**：§9 新增四行「（M2 新登记）」——影子「若采纳」只按门判据重算、不含发出前的前提复核；`MaterialGate` 是族 B 场景声明的事实（族 A 判据为空、不受影响）；§6.3「证据需求」的时效与获取方法仍未建模（由 M19 的候选契约承载）；真调只跑一次（同一批候选产物）、规模是先导基线。**「只记住案例」未出现**（迁移用例一致率 1.000、三对 `unseen_rate` 全 1.000），本批不触发未达标处理，固定问题模式作为对照臂保留。
- **收尾 M1 遗留**：§9 的 A1 行改为「M2 已处置」并写明落点（族 B 问题定义里的 `other` 判据 + `MaterialGate`；族 A 材料措辞未改，处置落在族 B）；`missing field probabilities` 契约漂移按现状登记（M19 批次 LLM 侧 15 次判断调用 0 失败、未复现，但样本量不足，仍为 backlog）；两条 M11 backlog（`jh-openai` 尾斜杠仍缺单测、`jh-jev` 未建模 403）本批复核后维持。
- **更新现状**：§1.3 #5–#7、§1.4、§3.1 crate 表与目录树、§4 落地进度与第 13 / 14 / 15 行、§7.1 #5–#9、§7.2 按 M18 / M19 的实际改动与当前测试基线（**23 个 test target、346 passed / 0 failed / 6 ignored**，M20 复核）更新；顺带把 M19 之后失准的代码行号引用改正（`main.rs` 的子命令与各参数结构、`family.rs:823` → `:1002`、`compare.rs` 的 `run_single` 与证据日志文件名行号）。
- **评审 round 1 修正（2026-09-22，同批）**：族 B 的轮数由「每个变体 2 轮」改正为 **每任务 3 轮**（`family_b.json` 的 `rounds: 3`；6 任务 × 3 轮 = **每臂 18 个决策轮**，§3.4 / §5 M2 / §12 #8 三处同改，口径与族 A 一致）；§3.4 把 `logged_calls_of_run` 的归属从 `calls` 改到 `compare`（唯一定义在 `compare.rs:1429`、调用点在 `h3.rs:2365`），并补上 `calls` 在 M19 的真实增量 `CallEntry::ModelCall` 与 `run()` / `case()` 分支。
- **为何**：§7.2「文档侧：本文件的架构与运行时规格按实际实现回写；结论与未确定项同步到[项目笔记](project-notes.md)或研究目标的新版本」；研究目标 §12 的 M2 退出证据要求「在隔离的成对任务和未见变体上检验 H3」。先导读数按研究目标 §10.4 只作本环境基线，不冒充研究结论；`var/` 产物不入库、teardown 后不保留，因此关键读数抄进本文件 §5 M2。

**2026-09-23 — M3 实现回写（M21 / M22 落地；本批只改文档）**

- **回写实现命名与机制**：§3.1 / §3.4 写入 M21 的 `jh-core` 实际模块与类型（`observer`：§7.2 八条逐项落成 `ObserverContract` 的字段、`ObserverCheck` 的四路独立检验、`ObserverLibrary` 的候选 → 验证 → 启用 → 停用 / 被替代、`ObserverVersion(u32)` 的版本号规则；`behavior`：`Behavior` 四类 + `Other` 出口；`runtime.rs:600` 的 `invalidate_observer` 与 `InvalidationCause::ObserverInvalidated`；merge `aad5fb1`，复核尖端 `219982a`）与 M22 的 `jh-app`（`c_program` / `c_measure` / `h4` + `tests/h4.rs` + `main.rs` 的 `h4` 子命令，以及三条证据通道、三类宿主钩子点与只从存储日志算得的隐藏检查；merge `c9d9f16`，复核尖端 `083d334`）。
- **定下未确定项**：§12 #5 的观测器一侧结案——版本号规则与问题库同口径（每个观测器 ID 各自从 1 起单调递增、由库分配；同一 ID + 同一版本不得换含义；停用与回退不产生新版本号；旧版本永不覆盖或删除、只改状态），观测器库的数据形态是可 JSON 往返的 `ObserverLibrary`，**入库文件形态仍未定**；§12 #2 回填 h4 的落盘形态（`var/h4/<run>.json` + `var/h4/<run>/{contracts,tests,tests-v2,verification}.json` 与 `{runs,runs-v2,transfer}.jsonl`）；§12 #8 回填任务族 C 的先导规模。
- **回写 M3 落地状态与读数**：§5 M3 增加落地状态——M21 / M22 的交付清单、**权威样本声明**（引用 `h4-1790143399409`；`final.json` 是另一次真调样本、只用来说明契约生成的不稳定性；`replay.json` 是同一批契约与测试产物的离线复跑；摊薄列以 `replay` / `replay-rejected` 为准）、规模与口径（3 个故障族 × 10 条确定性调度 × 4 路实现 = 120 例，机会 72、无违反 48，分母取自隐藏检查）、三臂读数表（误放行 45.8% / 0.0% / 0.0%；检出 54.2% / 100% / 33.3%；观测器弃权 60/120；两侧读数 60.4% / 50.0% / 54.2%）、迁移（非对角线检出 0.0–20.8%、误放行 12.5–33.3%）、维护（不重挂 30.56%(22/72) → 重挂 66.67%(48/72)；拿掉 `commit_counter` 后误放行 0% → 66.67%）、成本与摊薄（9 次生成调用 0 失败；每检出一次的生成调用数 0.0417 vs 0.125）、假说判定（三条判定规则跑前冻结、评审在提交历史核实；结论原文「不支持累积能力假说」）与复现命令；§6.2 / §6.3 / §6.5 / §6.7 补落点与读数，§7.1 #5–#9 与 §7.2 同步。
- **写清结构性前提（不许省略，也不许读成「观测器普遍更好」）**：机会数与真值全部取自隐藏检查；观测器与隐藏检查不同模块、不同数据通道；独立检验的第四路按隐藏检查判；自证式的伪隐藏检查被 `ObserverNotVerified` 拦住并把失败记录留档；§7.3 的「不得把要求换成『最终缓存为空』」由 `BANNED_SUBSTITUTIONS`（代码常量）与报告里的 `weakened_substitution` 承担。诚实性四条（通用臂 0% 误放行是退化读数、弃权按失败态计、契约与测试各只有一次真实生成且无稳定性采样、摊薄列按实际发起数计）逐条写进 §5 M3，并把跨运行观察与代码属性分开写。
- **登记 M3 风险**：§9 新增两行——观测器自证风险（对策 = 启用门槛上的四路独立检验 + 隐藏检查做第四路）与竞态的不确定性（N = 10 的确定性调度、检出率给 n/N、一次没检出不得读作「不存在」）；「完成声明被自证」一行补上 M21 / M22 的机制。**收尾 M2 遗留**：`logged_calls` 死代码、影子不含发出前的前提复核、§6.3 的时效与获取方法未建模三行按现状复核登记（本批不改代码）。
- **更新现状**：§1.3 #5–#7、§1.4、§3.1 crate 表与目录树、§4 落地进度与第 10 / 11 / 13 / 14 / 15 行、§7.1 #5–#9、§7.2 按 M21 / M22 的实际改动与当前测试基线（**25 个 test target、397 passed / 0 failed / 6 ignored**，M23 复核）更新；§4 另新增第 17 行「观测能力的证据通道」。
- **为何**：§7.2「文档侧：本文件的架构与运行时规格按实际实现回写；结论与未确定项同步到[项目笔记](project-notes.md)或研究目标的新版本」；研究目标 §12 的 M3 退出证据要求「在并发故障族中检验 H4，报告累积成本」，其未达标处理写明「若不能独立验证仪器，不允许其自证任务完成」。先导读数按研究目标 §10.4 只作本环境基线，不冒充研究结论；**否定结论照实写**（研究目标 §3.2 / §10.4：负向结果与正向结果同为完成）；`var/` 产物不入库、teardown 后不保留，因此关键读数抄进本文件 §5 M3。
- **评审 round 1 修正（2026-09-23，同批）**：M23 评审判 `p2-4items`（读数独立复算全对、三份产物未混读、§3.4 抽查 46 处引用全命中），四处**标注层**失准按建议改正——① §5 M3 的逐族「无违反用例」按 `cancel_after_confirm` / `lost_update` / `duplicate_after_restart` 的族序改为 **16 / 19 / 13**（原 16 / 13 / 19 是 JSON `arms[]` 的族序；全批 48 不变）；② §3.4 的 `h4.rs:63-77` 是**八个**（不是九个）臂标识；③ §1.3 #5 里 M0–M19 的 `main.rs` 行号按尖端逐条重测（`enum Command` :44–60、chat / ask 臂 :191–194、`RunArgs` :113 / `run_task` :327、`CompareArgs` :137 / `run_compare` :498、`ExpArgs` :169 / `run_exp` :606、`H3Args` :84 / `run_h3` :204），§12 #2 同源的三处行号一并改准；④ M22 r3 留给本批的 (a) 闭合：`replay.json` / `replay-rejected.json` 的 `raw.contracts_file` / `raw.tests_file` 指向不存在文件这一条写成 §5 M3 的限定句 + §9 一行 backlog（§12 #2 同步）。另按非阻塞建议把「两侧读数不是判据的一部分」注明该字段见 `replay.json`（权威样本里还没有 `hypothesis.reporting`）。**只改文档、不碰代码。**

**2026-09-23 — M4 实现 + TUI 决策变更回写（M24 / M25 / M26 落地；本批只改文档）**

- **TUI 决策变更（本批最重要的一处）**：§5 首段的「TUI 偏差（决定 4）」改写为「**TUI 决策变更（用户指令）**」——用户原话「规划 M4，同时将 TUI 也加入到 M4 工作中。M4 结束后应该给出一个能初步运行，且具备 TUI 界面的 Agent 系统」；研究目标 §12 的 M4 行「完整 TUI 展示」**已按期在 M4 交付**：交付形态 = `jev chat` 交互会话（LLM 主循环 + Jev 判定门控）+ TUI 主界面与首屏 + 三个研究面板接真实数据；工具含只读与 `bash`，副作用全走审批卡（M25，merge `bc66d18`；会话真调读数见 `docs/tui-design.md` §9.1）。同步改动的「相应位置」：§1.1 决定 4 行、§1.2 语言与形态、§1.3 #4 / #5 / #8、§1.4、§2.4 工程形态、§3.1 的 `jh-app` / `jh-tui` 行与目录树、§3.4 的 M25 条目、§7.1 #3、§8 第 1 条（移出非目标）、§10 的 Grok TUI 行、§13 #5 / #9。**这是决策变更，不是原计划顺延**：本版 2026-09-22 的重写稿写的是「1.0 不做 TUI」。
- **回写 M4 落地状态与四档结论**：§5 M4 增加落地状态——M24 的 `jev m4` 编排、M26 的统计冻结（`docs/m4-freeze.md`，提交 `f72d62f`，写在定稿主跑之前）与权威报告（`docs/m4-report.md`）、定稿批次规模与预算遵守（1858 次调用 / 失败 5 / 2.588M token / 约 1 小时 55 分，全部在冻结上限内）、四档结论（H5 获得支持；结构维条件性支持；判断通道维仅经济收益；跨任务复用仅成本且只有 `cancel_after_confirm` 一族的 12 例有判别力；族 C 观测能力累积不支持；原生基线证据不足），并写明**未交付**的联合演示与 TUI 侧的交付读数。§6.2 两条 M4 消融行补「已落地」、§6.5 / §6.6 补落点（统计冻结与报告的出处）、§6.7 的 H5 行补本环境读数与边界、§6.9 的 E4 项写明 M4 未安排（理由见报告 §11）。
- **§7.1 的三行**：#6 改为已交付并指向冻结 + 报告 + 复现命令；#8 文件名定案 `docs/m4-report.md`（`docs/m4-freeze.md` 是它的冻结记录）；#9 **按实写「未交付」**——联合演示两批都没做（`docs/research/m4-pilot.md` §8.9 第 9 条、报告 §3.4 的「本批不产生读数」），不含糊成「已交付」。
- **登记 M4 的 backlog 与遗留复核**：§9 新增五行（策略回放的快照标识不含臂 → 回放不复现臂间差异；`run_reuse_side` 问错族 → 复用消融只有一族有判别力；契约取自文件时被拒族从 `channel_setup` 消失；H5 判定文案写死「先导」；`lib.rs` 模块地图缺 `session`），每行给出 finding / 评审出处与代码行号；A4 第 1 轮 `rollback-cache` 那一行的复核结论写成**「未覆盖」**（报告只给格汇总，没有拆到该形态；M4 的循环式臂是 `h1off-h2full`，与 M16 那条 H1 开启的增量臂不是同一配置）。
- **更新现状与完成定义**：§1.3 #1（依赖 21 项、`jh-tui` 入列）/ #5（`m4` / `chat` 子命令、`main.rs` 行号按尖端重测）/ #6（M24 / M25 的模块）/ #7（26 个 test target、512 passed / 7 ignored，出处报告 §7）、§1.4（剩余工程量改写为 §7.1 #9 与 §12 的未确定项）、§4 落地进度（第 15 行的统计冻结结案）、§7.2 工程侧补 `jev m4` 与 `jev chat`。
- **§12**：#6（实验报告文件名）**结案**为 `docs/m4-report.md` + 冻结记录 `docs/m4-freeze.md`；#5 的观测器库入库文件形态仍**未定**（M4 的族 C 契约产物同样只落运行目录）、#8 回填定稿批次的任务集与规模、#13（E4 诊断项）按 M4 **未安排**定案（理由见报告 §11）。
- **为何**：§7.2「文档侧：本文件的架构与运行时规格按实际实现回写；结论与未确定项同步到[项目笔记](project-notes.md)或研究目标的新版本」；研究目标 §12 的 M4 退出证据要求「回答 H5，形成适用范围、失败案例与可复现报告」。结论只在研究目标 §10.4 的四档内表述，**已知限制与未交付项照实登记、不静默消失**；`var/` 产物不入库、worktree 拆除后不保留，因此关键读数抄进 `docs/m4-report.md`，本文件 §5 M4 只引结论档位与出处。

## 12. 未确定项

本版明确标注的事项：已由实现决定的条目写明决定与落点（2026-09-22 的 M0 批次、M1 批次与 M2 批次回写，2026-09-23 的 M3 批次回写，逐条见 §11）；仍未定的一并列出。

1. **已定（M9 / M15，2026-09-22）**：`jh-core` 的模块划分、类型名与 trait 签名。M9：模块 `answer` / `error` / `judge` / `question` / `record` / `state`；trait `Judge`（返回装箱 future 以便 `Box<dyn Judge>`，不引入异步运行时）；请求 / 响应 `JudgeRequest` / `JudgeResponse`（后者带 `validate()`）；问题侧 `QuestionId` / `QuestionVersion` / `QuestionSpec` / `Questions` / `Question::{Choice,Score,Noul}`；答案侧 `Answer::{Choice,Score,Noul}` 与 `Usage`；记录 `JudgmentRecord` / `Judgment` / `JudgeSource`；错误 `Error::{Unavailable,CallFailed,InvalidResponse}`。M15 新增：模块 `evidence` / `hash` / `graph` / `projection` / `runtime`；`Evidence` / `NewEvidence` / `EvidenceId` / `ObjectId` / `ObjectVersion` / `EvidenceLog`（`MemoryEvidenceLog` / `JsonlEvidenceLog`）；`ContentHash`；`DependencyGraph` + `EdgeKind::{Read,Support,Premise,Conflict}` + `EdgeOrigin` / `NodeRef` / `NodeId` / `ActionId` / `PropagationPath`；`InputProjection` / `ObjectRead` / `FieldSelection` / `Drift` / `SnapshotCheck` / `verify_snapshot`；`SemanticState` / `JudgmentNode` / `EvaluationStatus` / `Validity` / `Action` / `PremiseCheck` / `Invalidation` / `InvalidationCause`；`QuestionRegistry` / `VersionComparison`；错误新增 `Storage` / `UnknownEvidence` / `UnknownNode` / `UnknownAction` / `RetractedNode` / `QuestionVersionConflict`。**`jh-app` 的模块划分同样已定（M16）**：新增 `family` / `matching` / `compare` 与入库任务集 `crates/jh-app/tasks/family_a.json`。**M21 新增（2026-09-23）**：`jh-core` 的模块 `observer` / `behavior`，类型 `ObserverContract` 与 `Requirement` / `Measurement` / `Timing` / `Support` / `Disturbance` / `Controls` / `OutputContract`、`ObserverCheck` / `CheckDirection` / `ControlResult`、`ObserverLibrary` / `ObserverEntry` / `ObserverLifecycle` / `ObserverLifecycleRecord` / `ObserverId` / `ObserverVersion` / `ObserverVerdict`、`Behavior`，以及 `runtime` 的 `SemanticState::invalidate_observer` 与 `InvalidationCause::ObserverInvalidated`。**M22 新增（`jh-app`）**：`c_program` / `c_measure` / `h4` 与离线端到端测试 `crates/jh-app/tests/h4.rs`（族 C 的故障族与调度写在代码里，不像族 A / B 那样入库 JSON）。逐项见 §3.4。
2. **已定（M10 / M15，2026-09-22；M19 回填）**：判断记录的持久化格式与落盘粒度——判断调用逐行追加进 `var/calls.jsonl`（JSONL，一行一次调用；`kind=judgment` 行内嵌完整 `JudgmentRecord`，`kind=failure` 行只有 run / case / source / latency_ms / error，不写空答案）；文件与 `var/` 由运行时创建、不入库（`.gitignore` 已列 `var/`）；基线报告单独落 `var/exp/<run>.json`（pretty JSON）与 `var/compare/<run>.json`。实现见 `crates/jh-app/src/calls.rs`、`src/main.rs:169`、`:606`。**事件日志已定（M15）**：证据日志按**逐任务**落 `--out-dir` 下的 `evidence/<judge>-<arm>-<task>.jsonl`（`JsonlEvidenceLog`：追加写、不可变，撤回另记一笔），轨迹落 `traces/<judge>-<arm>-<task>.json`；`compare` 的缺省 `--out-dir` 是 `var/compare/<run>`。单任务 `run` 落 `<judge>-<arm>-<task>.evidence.jsonl` 与 `<judge>-<arm>-<task>.trace.json`，**两者都在 `--out-dir` 根下、不分子目录**（缺省 `var/run`）。实现见 `crates/jh-core/src/evidence.rs`、`crates/jh-app/src/compare.rs:740-768`（`run_single`，证据日志文件名在 `:750-755`）、`src/main.rs:327`（`run_task`，轨迹落盘与「轨迹：…」打印在 `:374`）。**M18 / M19 的落盘形态（2026-09-22，本批回填）**：反例日志走 `JsonlCounterexampleLog`，落 `--out-dir` 下的 `counterexamples.jsonl`；问题库走 `QuestionLibrary` 的 JSON 往返（`question-library-fixed.json` / `question-library-refined.json`，两臂各一份，字段 `registry` / `entries` / `history`）；候选产物落 `candidates.json`（`--candidates` 复用它即可不重发生成调用）；H3 报告落 `var/h3/<run>.json`，轨迹 / 证据落 `var/h3/<run>/{traces,shadow-traces,shadow-evidence}/`（`jev h3`，实现见 `crates/jh-app/src/h3.rs`、`src/promotion.rs:687` `write_library`）。**M21 / M22 的落盘形态（2026-09-23，本批回填）**：观测器**不落单独的库文件**（见 #5）；H4 报告落 `var/h4/<run>.json`，产物落 `var/h4/<run>/`——`contracts.json`（生成的测量契约）、`tests.json` / `tests-v2.json`（通用测试计划，v2 是版本变更后重生成的）、`verification.json`（四路独立检验的逐族读数与自证式检查被拒的记录）、`runs.jsonl`（主用例逐例读数）、`runs-v2.jsonl`（版本变更阶段）、`transfer.jsonl`（迁移矩阵逐格）；生成与判断调用仍逐行追加进 `var/calls.jsonl`（`CallEntry::ModelCall`，用途标签 `contract` / `tests` / `tests_regenerated`）。实现见 `crates/jh-app/src/h4.rs:852-969`、`src/c_measure.rs`。**注意产物按批次演进**：权威样本那一批（`h4-1790143399409`）的目录里还没有 `transfer.jsonl` 与 `tests-v2.json`（它们是 M22 评审修正时补上的），离线复跑的那几批目录里齐备。**另一条已知限制（2026-09-23 登记，见 §9）**：复用产物跑出来的报告（`replay.json` / `replay-rejected.json`）里 `raw.contracts_file` / `raw.tests_file` 指向各自 `out_dir` 下**并不存在**的文件（那两条路径按本次运行的 out_dir 拼，而本次运行只复用输入、不重写产物）；要取可直接复用的产物路径，看权威样本或 `final.json` 的 `raw` 段。
3. **已定（2026-09-22，M15）**：「认识状态」**不作为独立字段**落进运行时（§4 第 9 行）。用可核查的原值表达：两个正交维度 `EvaluationStatus`（是否拿到过一次有效响应）与 `Validity`（还能不能作为依据），加上依赖图上的支持边 `EdgeKind::Support` 与冲突候选 `EdgeKind::Conflict`（`DependencyGraph::conflicts_with`）；「有支持 / 有反证 / 证据冲突 / 证据不足」是消费方按这些原值算出的读数。理由：研究目标 §5.3 明说这**不是**四值逻辑；再立第三个可漂移的字段会让失效传播与验收各有一套真身，与「按对象保守、宁大勿小」的传播口径相抵。
4. 一等对象中 `Hypothesis` 与 `Capability` 在 1.0 是否落地（研究目标 §8.1 全表 vs 1.0 只做 §4 已列的运行时对象）。**仍未定。**
5. 问题库与观测器库的保存格式与版本号规则（研究目标 §14.1 #7）。**问题库已定案（2026-09-22，M15 / M16 起、M18 收口；§3.4 逐条）**：版本号取 `QuestionVersion(u32)`、**每个问题 ID 各自从 1 开始的单调递增整数**，由库分配（取该 ID 当前最大版本 +1；`crates/jh-core/src/question.rs:125` `QuestionRegistry::revise`）；同一 ID + 同一版本不得换含义（改含义必须拿新编号，`Error::QuestionVersionConflict`）；`QuestionLibrary` 的生命周期是候选 → 影子评估 → 启用 → 停用 / 被替代，**停用与回退不产生新版本号**（回退 = 把被替代的旧版本重新设为启用），**旧版本永不被覆盖或删除、只改状态**；入库的问题库文件保存的正是这份编号与状态，后续分配从文件里的最大版本 +1 继续。1.0 的问题库形态是**入库的**版本化 JSON（`crates/jh-app/tasks/family_a.json`、`crates/jh-app/tasks/family_b.json`，加载时校验并登记进版本表）+ 运行期的 `QuestionLibrary`（见 §3.4）。**观测器库已定案（2026-09-23，M21 / M22；§3.4 逐条）**：版本号规则与问题库同口径——`ObserverVersion(u32)`、每个观测器 ID 各自从 1 起的单调递增整数、由 `ObserverLibrary` 分配（调用方不写死）；同一 ID + 同一版本不得换含义；停用与回退不产生新版本号；旧版本永不覆盖或删除、只改状态。数据形态是可 JSON 往返的 `ObserverLibrary`（契约 + 检验记录 + 生命周期历史），生命周期是候选 → 验证 → 启用 → 停用 / 被替代，**启用要求一份四路全过的独立检验记录**。**仍未定**：入库的观测器库**文件形态与位置**（1.0 的 h4 批次没有把观测器库单独落盘——逐族的契约、检验读数与生命周期在报告里以 `verification` 段与 `raw.verification_file` 呈现）。**M4（2026-09-23，M27）复核：仍未定**——M4 的族 C 通道建立与复用都发生在运行期，契约产物只落 `var/m4/final/c1a/contracts.json`（全批复用同一份），没有入库文件（报告 §7 的产物路径表；契约取自文件时的另一处缺陷见 §9 backlog）。
6. **已定（2026-09-23，M26 / M27 结案）**：实验报告的文件名与放置位置——**M4 定稿报告 = `docs/m4-report.md`**，配套的**冻结记录 = `docs/m4-freeze.md`**（写在定稿主跑之前）；两者都在 `docs/` 下并入库。M0 / M1 / M2 / M3 没有单独的 `docs/` 报告文件，读数都是运行产物（`var/exp/<run>.json`、`var/compare/<run>.json`、`var/h3/<run>.json`、`var/h4/<run>.json`；关键值已抄进本文件 §5）。运行产物仍不入库（`.gitignore` 已列 `var/`）。
7. **已定（M10，2026-09-22）**：`exp` 子命令的参数形态与 JSON 报告字段——参数 `--judge <NAME>`（可重复；缺省 `rule` + `simulated-truth`）、`--calls <FILE>`（默认 `var/calls.jsonl`）、`--replay <FILE>`（默认 `var/calls.jsonl`）、`--out <FILE>`（缺省 `var/exp/<run>.json`）；报告**顶层字段**固定为 `run` / `generated_at_unix_ms` / `command` / `task_set` / `metric_definitions`（口径声明）/ `judges[]` / `calls_log`（绝对路径）/ `notes`，嵌套读数列**主要字段（非穷举）**：`task_set{name,source,cases,questions}`、`judges[].judge / source / live / versions`、`judges[].verification{status,detail}`、`judges[].calls{attempted,succeeded,failed,errors}`、`judges[].latency_ms{samples,min,p50,p95,max}`、`judges[].usage{calls_with_usage,input_tokens,output_tokens}`、`judges[].quality{judgments,agreements,agreement_rate,unscored,boundary_readings,brier_mean,log_loss_mean,log_loss_undefined,score_mae,score_mae_samples,per_question[]}`；完整字段定义以 `crates/jh-app/src/report.rs` 为准。报告不含价格折算（仓库无权威价格表）。实现见 `crates/jh-app/src/{exp,report,metrics}.rs`。
8. 任务族 A/B/C 的材料来源与规模（先导实验后定，研究目标 §10.3）。**任务族 A 的先导规模已回填（2026-09-22，M16）**：6 个合成任务 × 3 轮 = **每臂 18 个决策机会**，材料与扰动脚本入库在 `crates/jh-app/tasks/family_a.json`（任务族说明见该文件 `notes` 与 `crates/jh-app/src/family.rs`）；M0 的 4 样本最小合成集仍在（`crates/jh-app/tasks/initial_tasks.json`，供 `jev exp` 基线用）。**任务族 B 的先导规模已回填（2026-09-22，M19）**：**3 对成对场景 × 每对 2 个未见变体**（一 B1 一 B2，**每个任务 3 轮**）= 6 个对照任务、**每臂 6 任务 × 3 轮 = 18 个决策轮**（口径与族 A 相同），另有 3 个开发案例（各 3 轮）与 12 个验证用例（每对「正例 ×2 + 反例 + 不同表述 / 不同服务的未见用例」），材料与隐藏真值入库在 `crates/jh-app/tasks/family_b.json`（实现见 `crates/jh-app/src/family_b.rs`；落地状态与读数见 §5 M2）。这些规模**都是先导基线、不是冻结规模**：依据是研究目标 §10.3「先做小规模先导实验估计方差、成本与可行样本量」，先导读数（单日、单次运行）只作本环境基线（§5 M1、§5 M2）。**任务族 C 的先导规模已回填（2026-09-23，M22）**：**3 个故障族 × 每族 10 条确定性调度（N = 10）× 4 路实现 = 每族 40 例、全批 120 例**（机会 72、无违反 48）；材料与调度**写在代码里**（`crates/jh-app/src/c_program.rs`：故障族、调度与隐藏判据都是程序的一部分，隐藏判据只从存储日志算得），不像族 A / B 那样有任务集 JSON（落地状态与读数见 §5 M3）。这批规模同样是**先导基线**，不是冻结规模。**M4 定稿批次的任务集与规模已回填（2026-09-23，M26；`docs/m4-freeze.md` §6 的冻结规模）**：族 A 全部 6 个任务 + 族 B 全部 6 个未见变体（主对照与诊断对照**每格 3 次重复**，族 A 每臂 18 格）+ 族 C 3 个故障族 × **每族前 3 条**调度 × 4 路实现（判读段每格 3 次、C4 切片每格 1 次）+ 复用消融 3 族 × 3 条调度 × 4 路 = 36 个用例 + 密度扫描（`triggered` / `every-2` × 规模 3 / 6，每格 1 次）；族 C 第 4–10 条调度仍是留出，任务集与产物路径见报告 §1.2 / §7。**族 A 的开发 / 验证 / 测试划分仍未定。**
9. `jh-core` 是否引入 `tokio`，以及调度的具体实现形态（§3.3）。**已定（2026-09-22，M15 / M16）**：`jh-core` **不引入 `tokio`**，也不新增任何第三方依赖——只有 `serde` / `serde_json` / `thiserror`（`crates/jh-core/Cargo.toml`）；理由是运行时对象与失效传播都是同步的，判断器接口返回装箱 future、由调用方的 executor 驱动。调度的实现形态落在 `jh-app` 的匹配循环（`crates/jh-app/src/matching.rs`：固定轮序 + `Arm` 的调度规则），`tokio` 只被 `jh-app` 的 CLI 用来跑 async 判定器。
10. **已定（2026-09-22 真调）**：所选 LLM provider 是否提供 `/responses`——**实测确认可用**：非流式与 SSE 两个真调用例通过（`deepseek-flash`、`Completed`、deltas 重建最终文本、usage 正常），端点可用（成功路径 2xx）、SSE 事件与响应体反序列化均吻合；命令与读数见 §1.2 / §5 M0（文档证据：DeepSeek 官方[《Using the Responses API》](https://api-docs.deepseek.com/guides/responses_api/)、Codex 集成页 `wire_api = "responses"`、定价页 `Responses API ✓`）。同时登记两条新限制（本批只登记）：thinking 模式不支持强制 `tool_choice`（HTTP 400 `invalid_request_error`）与 thinking 的 reasoning token 计入 `max_output_tokens`（LLM 判断器 1024 上限下 4/4 `Incomplete`）；两条均有已归档 finding，路径见 §1.2。配置口径见 §1.2。**其中第 2 条已由 M14 处理**：LLM 判断器的默认输出预算改为 16384 并回写 LLM 真调读数（见 §5 M0、§11），判断器另在失败信息里带上 `incomplete_details.reason`，报告自身即可区分截断与内容过滤。
11. 中文输入的处理策略（原文 / 英译 / 英文 instructions）——由 M0 基线结论决定。**仍未定**：M0 真调基线已取得（2026-09-22，Jev 4 样本；见 §5 M0），LLM 侧的真调基线与一次中文语料上的读数也已取得（同日 M14，4 样本 × 3 题；见 §5 M0）；M16 的任务族 A 材料同为中文，先导批次（6 任务 × 3 轮、真实判定器）又给出一次读数（见 §5 M1）——但这三批的样本量都太小，不足以支撑该策略判断，也不构成研究结论。
12. 批处理与逐问在关键判定上的取舍——由实测决定（本文 §3.2；实验口径见 [Jev 调研报告](jev-research.md) §14.3 E3）。**仍未定**；接口层已同时支持两种表达（`Questions` 放 1 或 N 个问题）。
13. E4 注入诊断是否在 M4 安排；若不安排，须在实验报告中写明理由（见 §6.9）。**已定（2026-09-23，M26 / M27）：M4 未安排**——理由逐条写进 `docs/m4-report.md` §11（1.0 不含门控场景使原始口径失去对象；改后的口径要一对「同一事实、附加 / 不附加当前计划」的提示词，本批 runner 没有构造该变量的入口；额度已被主检验占满），并留作 1.x 的**独立离线诊断**（同一 state 两版提示词，N=2 通道 × M≥10 样本），不静默消失。

## 13. 待人工裁决的交叉文档一致性项（只列出，本版不修改这些文件）

1. **[AGENTS.md](../AGENTS.md)** 技术栈要求写「workspace 分 `crates/jh-app`、`jh-jev`、`jh-openai`、`jh-tui` 四个 crate」；决定 3 新增 `crates/jh-core` → 需人工决定是否更新其目录布局描述。
2. **`Cargo.toml:5`** 的注释以 `docs/roadmap-1.0.md §2` 指代「依赖版本登记与 Cargo.lock 锁定」；本版重写后该锚点不再指向该内容 → 需人工决定改注释或改指向。
3. **[项目笔记](project-notes.md) 决策 3**「判定权：允许交给 Jev——包括不可逆动作的门控场景」以「门控」为对象；本版整体移除门控决策点后，该决策的落点变为「判定器可用于哪些实验路径」 → 需人工确认其表述范围。
4. **[项目笔记](project-notes.md)「Jev 的定位」小节**把「动作门控」列为循环内判定示例 → 与本版 §2.3 的表述需对齐（改或不改由人工决定）。
5. **[研究目标](RESEARCH_GOALS.md) 的 TUI 条目**：文首「配套 TUI 研究界面」、§11 的 TUI 研究操作、§12 的 M1「最小 TUI」与 M4「完整 TUI 展示」，以及 §8.3 组件分工与 §14.1 交付物中的 TUI。**该项已由用户 2026-09-23 的指令裁决**：TUI 加入 M4 并**已按期交付**（§5 首段的决策变更、§7.1 #3、§8 第 1 条），本版原写「决定 4 的 1.0 不做 TUI」的偏差不再成立。**仍由人工决定**的是：是否给该文档加「1.0 范围」标注或改写这些行（本文件不改 `docs/RESEARCH_GOALS.md`）。
6. **[研究目标](RESEARCH_GOALS.md) §12 的 M5 行**（真实仓库迁移等）在 1.0 之外，本版只在 §5 与 §8 声明 → 是否需在该文档中显式标注「1.0 止于 M4」，由人工裁决。
7. **[Jev 调研报告](jev-research.md) §14.3 E4（注入鲁棒性）与 §7.4（fail open / fail closed）**以「门控」为语境；本版不把门控作为路线图内容 → 这些材料保留为研究素材，是否需要在报告中重述其定位由人工决定。
8. **[Jev 调研报告](jev-research.md) §13.5 红线 1**（不可逆动作不能只靠 Jev 守门）与 **[项目笔记](project-notes.md) 决策 3**（允许交给 Jev）本就相反，项目已声明「项目决策优先于 §13 的默认建议」；本版不复述该冲突，但 1.0 的实验是否会触碰不可逆动作，需人工确认。
9. **[研究目标](RESEARCH_GOALS.md) §14.1 交付物清单**含 TUI 与研究文档版本维护；1.0 的实际处置见 §7.1 → 是否需在该文档中同步处置结果，由人工决定。**2026-09-23 更新**：TUI 那一项的处置已由用户指令改为「交付」（§7.1 #3），而 §7.1 #9 的联合演示**未交付**（§7.1 #9）；是否把这两条同步进该文档，仍由人工决定。
10. **[项目笔记](project-notes.md) 决策 2**（接受单一供应商，1.0 不实现降级与容灾）与 **[Jev 调研报告](jev-research.md) §13.5 红线 3**（不能把 Jev 当作唯一判定路径、必须保留降级实现）、**[研究目标](RESEARCH_GOALS.md) §13 失败模式表**（「API 不可用或模型版本变化 → 使用已声明的退路」）相抵 → 需人工裁决。可选处置（本版不替人工决定）：把规则 / 录制 / 模拟真值判断器声明为「已声明的退路」，或明确 1.0 不覆盖该行。
