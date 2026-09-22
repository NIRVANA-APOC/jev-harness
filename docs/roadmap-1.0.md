# jev-harness 1.0 技术路线图

> 状态：已定稿（2026-09-22）｜性质：实施规划
> 相关文档：[项目总纲](../AGENTS.md)、[项目笔记](project-notes.md)、[Jev 调研报告](jev-research.md)

## 0. 前提

| 项 | 结论 |
| --- | --- |
| 语言 / 界面 | Rust 全栈；TUI（极简：对话 + 决策轨迹 + 审批） |
| LLM 侧 | OpenAI **Responses API**（`POST /v1/responses` + SSE 流式事件），默认 DeepSeek（`deepseek-chat`），`base_url` / `model` 可配 |
| Jev 侧 | 使用真实 API（key 只经环境变量 / 本地 `.env` 注入，绝不入库） |
| 探索重点 | 门控+循环边界 / 路由 / 评估 judge —— **三条都做最小闭环** |
| 项目原则 | 个人学习、奥卡姆剃刀（如无必要勿增实体）、禁止过度防御性编程、不做降级容灾 |

> 术语约定：LLM 侧一律按 **OpenAI Responses API** 口径实现——`POST {base_url}/responses`、`stream: true` + SSE 事件（客户端见 `crates/jh-openai`）；早期文档里的「OpenAI 兼容协议 / `chat/completions`」不再是本项目的目标协议。

> **风险（未验证）**：所选 provider（DeepSeek）是否提供 `/responses` 端点**尚未验证**，M1 需用真实调用确认；若不可用，则需换 provider 或另议协议。

## 1. 核心命题与设计回答

**命题**：Harness 一词意为「约束」。传统 harness 的约束是**静态**的——系统提示、工具白名单、步数上限、人工审批；而 Jev 在「决策」上具备带概率的约束能力。能否用它做出**更好的 Harness 架构**？

**设计回答：决策即约束（Decision-as-Constraint）**——把 harness 中一部分静态约束换成 Jev 的**校准判定**，并保持三者可互换、可度量：

```
一个决策原语：  state + 类型化 questions →（答案 + 概率 / 置信度）
三条策略：      Gate 门控（约束"能不能做"）
                Route 路由（约束"用哪个做"）
                Judge + Boundary（约束"做到什么程度算完"）
三种实现：      Jev（学习型）/ 规则（确定性）/ LLM（语言型）——同一 trait，配置切换
一条记录：      每次判定都落一条 DecisionRecord（问题、概率、阈值、耗时、模型版本、request_id）
                → 既喂给 TUI 的决策轨迹面板，也喂给实验统计
```

**1.0 的野心**：不是做一个功能更全的 agent，而是把「Jev 作为约束层」这一命题做成**可运行、可观察、可对比**的最小系统。

## 2. 架构（4 crate workspace，刻意保持小）

```
jev-harness/
├── Cargo.toml                # workspace：members = ["crates/*"]，resolver = "2"，统一依赖清单
├── crates/
│   ├── jh-app/               # 组装层，bin: jev（chat / ask / run / exp 四个子命令）
│   ├── jh-jev/               # Jev 客户端（POST /v1/systemone，serde 内部标签枚举）
│   ├── jh-openai/            # OpenAI Responses 客户端（POST /v1/responses，SSE 流式，reqwest + 手写 SSE 解析）
│   └── jh-tui/               # ratatui + crossterm：对话流 / 决策轨迹 / 输入与审批
├── docs/                     # 文档（含本路线图与 experiments-1.0.md）
└── var/                      # 运行产物（sessions/*.jsonl），gitignore
```

四个 crate 按「外部边界」切分：三个通道（Jev / OpenAI / TUI）各自独立，`jh-app` 负责把它们组装成 Agent 循环；crate 内部的模块划分（领域类型、决策策略、轨迹记录等）随各自实现落地。

> **Cargo.lock 已锁定入库**：四个 crate 全部合并后，由专门任务执行 `cargo generate-lockfile` 生成完整解析，并用 `git add -f` 强制提交（并行开发阶段它一直被 `.gitignore` 忽略，以免各分支的 lockfile 改动互相冲突）。
> 维护提醒：`.gitignore` 里的 `Cargo.lock` 条目保留不动；但已跟踪文件的改动不受 ignore 规则影响，后续更新依赖后需显式 `git add` lockfile。

**依赖选型（刻意的短清单，统一登记在根 `Cargo.toml` 的 `[workspace.dependencies]`，各 crate 一律 `workspace = true` 引用）**：`tokio`（异步运行时）、`reqwest`（rustls；HTTP + SSE 手写解析）、`serde` / `serde_json`、`toml` + `dotenvy`（配置）、`thiserror`（显式错误枚举，不引入 anyhow）、`clap`（子命令）、`tracing` + `tracing-subscriber`（日志）、`ratatui 0.30` + `crossterm 0.29`（TUI）、`unicode-width` / `unicode-segmentation` / `textwrap`（TUI 排版）、`futures` / `bytes`（流式与字节缓冲）。TUI 相关版本取 crates.io 当前稳定版，不再对齐 grok-tui 研究的 0.29 / 0.28。**不引入** `async-openai`：协议层自己写（约 200 行），也更贴合「Jev 客户端需自建」的学习目标。

## 3. 1.0 的四个决策点（规格）

| 决策点 | 原语 | 触发时机 | 失败语义（关键） | 基线实现 | 观测指标 |
| --- | --- | --- | --- | --- | --- |
| **Gate 门控** | Noul ×2（"在用户意图范围内" / "结果可逆"）+ 双阈值 → allow / ask / deny | 每次工具执行前 | **fail-closed**：低置信 → 转人工审批 | 路径 / 命令白名单 | 放行率、误拦率、注入下漂移、延迟、成本 |
| **Route 路由** | Choice（候选工具 ≤8） | 有多个候选工具 / 技能时 | **fail-open**：低置信 → 交给 LLM 决定 | 关键词规则 | 与人工标注的一致率、选项顺序敏感性、延迟 |
| **Boundary 边界** | Noul（"任务已完成"）+ Noul（"再来一轮有进展"） | 每轮工具结果之后 | 低置信 → escalate（问人工） | 步数上限 + LLM 自评 | 提前终止率、漏终止率、平均步数、成本 |
| **Judge 评分** | Score（1–5 档 + criteria） | 每轮结束 | 仅记录（1.0 不驱动自动重试） | LLM judge | 与人工评分相关性、方差、成本 |

硬约束与约定：

- **Gate 的 state 组成**：只放「用户消息 + 待执行动作」，**绝不放工具输出与助手消息**——否则不受信内容能为自己授权（依据：[调研报告](jev-research.md) §7.3）。
- **版本 pin**：调用 `jev-1.13.0`，不用 `jev-latest` 固化阈值；每条记录都带响应里的实际模型版本。
- **阈值来源**：一律来自本项目自测（E2），禁止照抄文档默认值。

## 4. 里程碑（M0–M5）

| # | 里程碑 | 交付物 | 验收标准 |
| --- | --- | --- | --- |
| **M0** | 工作区骨架（半天） | workspace 根配置（`members = ["crates/*"]`、`resolver = "2"`、统一 `[workspace.dependencies]` 清单）+ bin `jev`（`--version` 与 chat / ask / run / exp 四个子命令壳，均未实现）+ `.gitignore`（`target/`、`Cargo.lock`、`.env`、`var/`；Cargo.lock 当时暂不入库，批次末尾已锁定入库，见 §2）+ 路线图同步；四个 crate（jh-app / jh-jev / jh-openai / jh-tui）的实现由 M1–M3 补齐，领域类型 / `Decider` / `Tool` trait / 配置（`jev.toml`）/ Jev fixture 随之延后 | `cargo test` 全绿；`jev --version` 可运行；`git status` 无残留未跟踪文件；**无任何网络调用** |
| **M1** | LLM 通道（1 天） | OpenAI Responses 客户端（`POST /v1/responses`，流式）+ `jev chat` 纯 CLI 对话 | 先确认 provider 的 `/responses` 可用（见 §0 未验证风险），再与 DeepSeek 真实对话跑通；401/429 显式报错、不吞 |
| **M2** | Jev 通道（1 天） | Jev HTTP 客户端 + `jev ask --noul/--choice/--score`；pin 版本；录制真实响应为 fixture | 真实调用成功并打印答案 / 概率 / 延迟 / request_id；fixture 离线回放可复现 |
| **M2.5** | **E1 中文基线（提前跑）** | 30–50 条中文样本 × 三种 prompt 条件的最小对照 | 得出「中文是否可用」的初步结论，避免后程返工 |
| **M3** | 循环 + 三条闭环（2–3 天） | 三个工具；Agent 循环；Gate / Route / Boundary / Judge 各自三种实现可切换；JSONL 轨迹 | 一个真实任务端到端跑通；轨迹里能看到每条判定的概率与阈值 |
| **M4** | TUI（1–2 天） | ratatui 极简三区：对话流 / 决策轨迹 / 输入与审批；状态行（模型、token、成本、Jev 延迟） | 一次完整会话（含一次门控拦截 + 一次人工审批）在 TUI 内完成 |
| **M5** | 实验与结论（1–2 天） | 固定任务集 ≥20 个；三配置（jev / rule / llm）A/B；E2 校准分档；E4 注入样本 ≥20 条；`docs/experiments-1.0.md` | 报告含原始数据位置与复现命令；阈值全部来自自测；更新 [项目笔记](project-notes.md) |

**依赖顺序**：M0 → M1 → M2 → (M2.5) → M3 → M4 → M5；M4 与 M5 可部分并行。

## 5. 实验设计

1. **E1 中文基线**：中文 state + 中文 instructions / 中文 state + 英文 instructions / 英译 state + 英文 instructions 三条件对照；指标为准确率 + 置信度分布。
2. **E2 校准分档**：用真实调用按 confidence / noul 分 10 档，画可靠性图并算 ECE —— **阈值由它定**。
3. **E4 注入鲁棒性**：伪造工具输出 / 伪造预先授权 / 自我论证等 ≥20 条样本，测 Gate 的 block 概率漂移与「不安全却放行」率。
4. **三实现对比**：同一批决策场景下 Jev vs 规则 vs LLM 的准确率 / 延迟 / 成本 / 一致性。

## 6. 1.0 明确不做（non-goals）

- 多 provider 生态（只 OpenAI Responses API 一家，DeepSeek 默认）
- TUI 的主题体系、多面板、鼠标、多会话（留给 1.x）
- MCP、子代理 / 多智能体、插件系统
- 降级 / 容灾 / 重试编排（项目已决定接受该风险；错误一律显式暴露）
- RAG / 向量库、会话数据库（只写 JSONL 文本）
- 工具沙箱（1.0 的「安全」就是 Gate + 人工审批这一条路径）

## 7. 风险与前置约束

| 风险 | 对策 |
| --- | --- |
| 中文表现未知（官方只说「可用但准确率较低」） | E1 提前到 M2 后立即跑；结论不好则改用「英译后再判定」 |
| 提示注入可移动 Gate 判定 | Gate 的 state 组成约束写进 §3，并由 E4 实测 |
| 校准未经独立验证（唯一 ECE 实测不利） | 阈值必须先过 E2 |
| API key 泄漏 | 只走环境变量 / `.env`（gitignore）；不在任何文档、提交或日志中出现 |
| 供应商无 SLA、单一供应商 | 已接受；`jev-latest` 漂移 → pin `jev-1.13.0` |
| 范围蔓延 | 本文件 §6 为硬边界；新增内容一律进 1.x 待办，不进 1.0 |

## 8. 参考素材

- `docs/research/grok-tui/`（28 篇 TUI 研究）与 `reference/grok-build`（本地参考，已 gitignore）：仅作 1.x 的参考素材，1.0 只取极简形态，不引入其依赖与复杂度。
- `docs/jev-research.md` §13（定位建议）与 §14.3（E1–E9 实验清单）是本路线图实验设计的来源。
