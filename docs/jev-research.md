# TypeSafe Jev 调研报告（面向工程决策的合稿）

> **调研日期**：2026-09-22（全篇事实与数字均以该时点为准；平台数字会漂移，引用前须重新核对官方页面）
>
> **调研方法**：13 个专题并行检索 → 4 个撰写代理分头成稿（片段 A：原理与 API 契约与能力边界；片段 B：平台与定价与性能评测；片段 C：应用场景与 harness 集成与提问工程；片段 D：竞品与 Rust 集成与公司法务与社区）→ 本文件合稿、去重、统一术语并撰写第 13 节定位建议。
>
> **原料来源**：13 份专题文件与 4 份片段已随本文件入库，位于 `docs/research/jev/`（`01-official-launch.md` … `13-community-debate.md`、`final-A.md` … `final-D.md`）。网页抓取快照、`evals` 文本与 serde 编译验证工程等大体积中间产物未入库，仍留在 `/tmp/jev-research/`（临时文件，见 §14.1 A-12）。
>
> **事实纪律**：本报告只使用上述 4 份片段内已有的事实、数字与 URL，**不引入片段之外的任何新事实**；片段之间冲突的数字一律并列保留并注明口径；所有【未证实】内容在正文里保持原标记，不升级为结论。除明确标注的一次性本地编译验证（serde 映射，见 §10.3）外，**本轮调研没有对线上 API 做过任何真实调用**，全部 HTTP 行为均来自官方文档描述。
>
> **可信度标记约定**：
> - **【官方】**＝TypeSafe 官方文档 / 博客 / 模型与定价页 / 评测站 / 法律条款 / 官方新闻稿 / 官方 GitHub 组织与 SDK 源码。
> - **【第三方】**＝有署名的第三方实测、报道、社区讨论，或集成方自己发布的一手文档（此时括号内保留「集成方官方文档」字样，例如 LangChain / Pydantic AI / LiteLLM / Cloudflare / Vercel / Netlify）。
> - **【未证实】**＝转述、二手、聚合、无法核对或口径不明。本报告中此类条目**保持原标记**，仅供线索，不作决策依据。
> - 片段中另出现过【自测】标记（一次性本地编译/抓取验证）。本报告统一把它改述为正文中的「本轮自测」说明；它既非【官方】也非独立【第三方】证据，仅用于说明「某个形状问题在本地被验证过」。
>
> **术语约定**：System One、state、questions、Choice / Score / Noul、confidence、instructions、criteria、early access、credits、RLCD、jaggedness 保留英文原词；「类型错误（type error）」指输出越出声明 schema 的情况。

---

## 目录

1. [执行摘要](#1-执行摘要)
2. [Jev 是什么：System One、RLCD、非自回归与 LLM 的本质差异](#2-jev-是什么system-onerlcd非自回归与-llm-的本质差异)
3. [API 契约与三类问题原语（Choice / Score / Noul）](#3-api-契约与三类问题原语choice--score--noul)
4. [能力边界、失败模式与已知限制](#4-能力边界失败模式与已知限制)
5. [平台、定价与限额](#5-平台定价与限额)
6. [性能与独立评测](#6-性能与独立评测)
7. [应用场景与 Agent Harness 集成模式](#7-应用场景与-agent-harness-集成模式)
8. [提问工程与最佳实践](#8-提问工程与最佳实践)
9. [竞品对比与选择准则](#9-竞品对比与选择准则)
10. [Rust 集成要点](#10-rust-集成要点)
11. [公司、条款与依赖风险](#11-公司条款与依赖风险)
12. [社区争议与采用动态](#12-社区争议与采用动态)
13. [对本项目的定位建议](#13-对本项目的定位建议)
14. [附录](#14-附录)

---

## 1. 执行摘要

> 15 条，每条一句；只保留会影响本项目技术选型与架构决策的结论。括号内是可信度与主要依据章节。

1. **Jev 是 TypeSafe AI 的 System One 模型：输入一份 state + 一组类型化 questions，直接返回类型化答案与概率，不生成文本**——它是「决策层」而非 LLM 的替代品，官方明确否认它能替换驱动 coding agent 的 LLM。（【官方】§2）
2. **整个 API 面只有一个核心端点 `POST https://api.typesafe.ai/v1/systemone`，请求体恰好 `state` / `model` / `questions` 三个顶层字段，答案按调用方自定义的 key 回填，key 不发给模型**——wire 契约极简，对 Rust 侧自建客户端友好。（【官方】§3.1–§3.3）
3. **只有三种问题原语：Choice（无序候选择一，API 上限 255 项、官方称约 240 项内可靠）、Score（有序列标尺，2–10 级）、Noul（是非概率，无 confidence 字段）**；三者共享 `type` 与 `instructions`，各自带不同形状的 `criteria`。（【官方】§3.4）
4. **同一请求内的问题共享 state、并行评估、彼此独立**，加问题几乎不增加延迟、只多花问题本身的 token；这是成本模型的核心——state 每请求只计费一次。（【官方】§3.6、§8.4）
5. **可引用的量化优势集中在成本与延迟的量级上，不在准确率上**：LiteLLM 分类环节 p50 126.81 ms vs Haiku 688.40 ms（5.43×，CI 5.20–5.71）、分类成本 −96.118%；而官方自评四任务等权准确率 67.8%，落后最强对照 Sol 74.1% 约 6pp，发票场景落后 17.3pp。（【第三方】+【官方】§6）
6. **多条独立测试显示 Jev 的准确率并没有超过 LLM**（钓鱼基准 62.6% vs Haiku 81.3%；Jevals 的 choice 任务被 Gemini 3.8 Flash 显著领先；JevBench 里 GPT-5.6 Luna(low) 的 Intelligence/Calibration 都高于 Jev），**它的差异化在工程属性：免标注、带概率、无解析失败、快**。（【第三方】§6.4、§9.1）
7. **「零幻觉」只断言「输出不越出你声明的 schema」，官方自认该 0% 数字 "not empirical"**——类型合法但语义错误的判定仍然存在，且会静默流向自动化。（【官方】+【第三方】§4.5）
8. **校准（probability/confidence 可用作阈值）是核心卖点，但没有公开的 ECE / reliability diagram / 独立复现**；唯一直接测 ECE 的第三方实验（钓鱼任务）显示 Jev ECE 0.154 **差于** Haiku 的 0.097；官方最优证据是 jev-1.12 上「confidence ≥0.9 的一半准确率 90%、<0.9 的一半 40%」。（【官方】+【第三方】§4.4、§6.6）
9. **官方自列 9 类失败模式（jaggedness）必须内建进 harness 设计**：字面理解、算术/计数、日期比较、多跳、大而嘈杂的 state、对抗性内容、instructions 与 criteria 矛盾、结构性不变式不成立、生成任务；官方同时声明 0.30–0.70 的「不确定带」不是校准保证也不是最优阈值。（【官方】§4.1、§4.4）
10. **Jev 不保证跨问法的结构恒等式**：同一问题 Noul 得 0.22 而 Choice "yes" 得 0.01（confidence 0.99）；「是否要求退款」与「是否在要求别的什么」两个 Noul 得 0.72 与 0.47（和 = 1.19）——**阈值不能跨问题类型搬运，也不能假设算术恒等**。（【官方】§4.1、§8.3）
11. **平台侧硬约束：64k tokens/请求（其中 state + 最长单项 ≤32k）、仅文本输入、英文最佳（CJK「handled but not equally well」且无任何量化数字）、限流 250,000 tokens/秒 与 1,200 请求/分钟且官方声明会动态调整**。（【官方】§5.3–§5.4）
12. **无 SLA、单一 provider、无自部署**：MCA 第 3 条支持承诺仅为「commercially reasonable efforts」，状态页 99.840%（窗口未标明）且历史上出现过 59 分钟级中断；上线期曾因流量过高一度无法服务用户；容灾必须自建。（【官方】+【第三方】§11.2）
13. **合同风险真实存在**：MCA 2.5 明确 API 变更可能造成不兼容；Telemetry 定义含 "learnings" 且可无限制处理、协议终止后继续存续；ZDR 仅面向企业客户；许可条款 2.3(b) 禁止用服务或 Output 做蒸馏或开发竞品。（【官方】§11.3）
14. **Rust 侧没有官方支持的路径**：官方只有 Python 与 JavaScript/TypeScript SDK，`docs.typesafe.ai/sdk/rust/` 返回 404；serde 形状映射已在本轮一次性编译验证中通过（8 项断言），但社区 crate 生态极碎片（29 个关键词命中、至少 18 个独立客户端、全部 0.x、star 0–11），**且 crates.io 上 `jev-harness` 名称已被占用（0.1.4，2026-09-21）**。（【官方】+【第三方】+本轮自测 §10）
15. **本项目定位建议（详见 §13）：把 Jev 当作「可替换的快速判定后端」而非核心依赖**——按「决策」而非「模型调用」抽象、概率作为一等数据、打包提问优先于并发、路由 fail-open 与门控 fail-closed 分开处理；**所有关键判定必须在本项目自有的带真值数据集上实测后才允许进入自动化路径**。

---

## 2. Jev 是什么：System One、RLCD、非自回归与 LLM 的本质差异

**要点**

1. Jev 是 TypeSafe AI 的第一个公开模型，也是其自创品类 **System One Model** 的首个实例：不生成文本，输入 **state** + 一组带类型的 **questions**，直接返回类型化答案与概率分布。【官方】
2. 训练方法是 **RLCD（Reinforcement Learning for Calibrated Decisions）**，官方与 RLHF、RLVR 并列为第三条后训练路径，优化目标是「概率要诚实」。【官方】
3. 与 LLM 的根本差别有三层：**输出空间预先枚举**（不会有类型错误）、**非自回归的并行采样**（「增加问题几乎不增加响应时间」）、**每个输出都带概率**（Noul 除外，其余带独立 confidence）。【官方】
4. 发布状态截至 2026-09-22 仍是 **early access + waitlist、hosted API、无自部署**；发布日 2026-09-15，同日宣布结束隐身期与 $40M 种子轮。【官方】+【第三方】
5. 架构与规模**完全未披露**：官方只给「新模型架构 + parallel sampler + RLCD」三句话，无参数量、无 model card、无权重，并明确不发布标准 benchmark 表。【官方】

### 2.1 System One 的定义（官方原文）

- "System One models are a class of AI models built to make fast, structured decisions that software can use directly. A System One model evaluates a state and returns typed answers and probabilities."（<https://docs.typesafe.ai/concepts/system-one>）【官方】
- "Like an LLM, a System One model understands natural-language input. It returns typed decisions and probabilities rather than generated text."（同上）【官方】
- "System One models do not write replies, produce code, or generate explanations of their reasoning."（同上）【官方】
- 命名来源：System 1/2 取自 Kahneman《思考，快与慢》；"Jev" 取自 William Stanley Jevons（杰文斯悖论：单位成本下降导致总需求上升）。（<https://typesafe.ai/blog/introducing-system-one-models-and-jev>，下称"官方发布博客"）【官方】
- 公司叙事："Machine Native Intelligence"、"We're building prod, not God"、预期未来 AI 自动化中约 99% 是机器对机器、1% 是人对人。（<https://docs.typesafe.ai/introduction/machine-learning-primer>、<https://typesafe.ai/manifesto>）【官方】

### 2.2 RLCD：第三条后训练路径

- 三条路径并列：**RLHF** 优化人类偏好（官方自陈会导致 sycophancy、自信的幻觉、mode dropping）；**RLVR** 优化可程序化验证的奖励（数学等，更慢更贵）；**RLCD** 优化「决策 + 校准概率」，不生成文本。（<https://docs.typesafe.ai/introduction/machine-learning-primer>）【官方】
- 校准的官方定义（避免误读）："Higher probability should correspond to a greater chance that the answer is correct."——概率 0.2 的结果应在大约 20% 的情况下出现，0.8 对应约 80%，1.0 对应 100%；且 "**These rates describe groups of predictions, not a guarantee about any single answer.**"（同上、<https://docs.typesafe.ai/confidence>）【官方】
- 架构/训练栈的唯一描述："We built a new stack entirely focused on automation: with a new model architecture, parallel sampler for maximum efficiency, and training method we call Reinforcement Learning for Calibrated Decisions (RLCD)."（官方发布博客）【官方】
- **缩写重名提醒**：RLCD 在 ICLR 2024 已有另一含义（Reinforcement Learning from Contrastive Distillation，Kevin Yang 等），引用时须写明指哪一个。（<https://www.laura-martel.com/blog/typesafe-jev-system-one-models>）【第三方】
- 数据口径：CEO 播客中称训练数据**全部为合成数据**、刻意不用用户数据；文档侧表述 "Jev is not fine-tuned or LoRA-adapted with customer data… the same weights serve every account"（<https://www.latent.space/p/jev>、<https://docs.typesafe.ai/models>）【官方，播客为 CEO 自述】
- **未披露**：RLCD 的奖励函数形态、训练数据配方、校准度量方法。（官方发布博客、<https://kingy.ai/blog/typesafe-jev-review-the-ai-model-that-doesnt-generate-text/>）【官方未公开】

### 2.3 非自回归：并行采样而非逐 token 生成

- 官方对照表：LLM 是「顺序、逐 token，每个 token 依赖上一个」；System One/Jev 是「并行，单次查询产出全部输出」。（官方发布博客）【官方】
- 一次请求中所有问题**共享同一份 state、彼此独立并行求值**："Every question in a request sees the same state, is evaluated independently"；一个问题不会成为另一个问题的隐性上下文。（<https://docs.typesafe.ai/primitives>、<https://docs.typesafe.ai/concepts/state>）【官方】
- 官方速度口径：端到端 **70ms–500ms**，对比前沿 LLM 的 3–329 秒；同类任务快 40x–200x。（官方发布博客、<https://docs.typesafe.ai/introduction>）【官方】
- 架构推断属第三方：社区把 Jev 描述为「受约束的/类 diffusion 的决策模型，不能产出自由文本，必须预先定义输出格式」。（<https://www.latent.space/p/ainews-jev-a-system-one-model-that>）【第三方，非官方依据】

### 2.4 与 LLM 的本质差异（官方对照表节选，来源：官方发布博客）【官方】

| 维度 | 现有 LLM | System One / Jev |
| --- | --- | --- |
| 优化目标 | RLHF（人类偏好）/ RLVR（可验证奖励） | RLCD（校准决策） |
| 输入 | 非结构化文本，偏重消息序列 | 非结构化文本，偏重结构化程序状态 |
| 输出 | 字符串；交给软件须解析 + 校验，可能跑偏 | 类型化结构值；答案空间预先定义，永不出类型错误，附校准概率与置信度 |
| 采样 | 顺序、逐 token | 并行，单次查询产出全部输出 |
| 成本 | 输入 $0.20–$10 / MTok，输出约为输入 5 倍价 | 输入 $0.042 / MTok（$42 / Btok），输出免费 |
| 速度 | 前沿模型端到端 3–329 秒 | 70ms–500ms；同类任务快 40x–200x |
| 置信度 | 即便被要求给置信度也常过度自信、不一致 | 每个输出带置信度；越自信越准；相似输入得相似答案 |
| 适用场景 | 人在环任务（chatbot、copilot、coding agent） | AI 驱动的 workflow / "smart if-statements"：分类、路由、打分、抽取、分支；大数据 map-reduce；对 LLM 提示/推理链/输出做打分、评判、校验、护栏、jailbreak 检测 |

- 官方否定的替代关系："Jev is not a drop-in replacement for the LLM behind Claude Code, Cursor…"；"Coding agents rely on an LLM that streams text, calls tools, and edits files… Jev does none of that."（<https://docs.typesafe.ai/introduction/coding-agents>）【官方】
- 官方首页头条数字 **193.6x 更快、444.6x 更省**（单次对比展示 TypeSafe $0.000081 / 0.114s vs LLMs $0.013880 / 8.566s），官方自述「我们认为这是真实世界收益的上限区间」。**该倍数无法从公开数字复算，属不可引用档**，详见 §6.5。【官方，厂商自测】
- 官方明确**不发布标准 benchmark 表**（理由是 benchmaxxing），改用「有日期的快照评测、发布后即退休」，并承诺公开 caveat 与对自己不利的证据。（<https://typesafe.ai/blog/antibenchmaxxing>）【官方】

### 2.5 发布、版本与时间线

| 时间 | 事件 | 来源与性质 |
| --- | --- | --- |
| 2024 | TypeSafe AI 成立，旧金山 | 【第三方（公司新闻稿）】<https://www.businesswire.com/news/home/20260915525333/en/TypeSafe-AI-Emerges-From-Stealth-With-%2440M-in-Funding-With-New-Model-for-Composable-AI> |
| 2026-08-12 | 官方 10-K 分类 cookbook 实验跑在 **jev-1.12** 上（说明 1.12 早于公开发布存在） | 【官方】<https://docs.typesafe.ai/cookbooks/classification_using_confidence> |
| 2026-09-11 | 官方 Noul 自洽性 cookbook 实验跑在 `jev-latest` 上 | 【官方】<https://docs.typesafe.ai/cookbooks/consistency_noul_cookbook> |
| 2026-09-15 | 正式发布（博客页面渲染日期 "Sep 15, 2026"），同时结束隐身期并宣布 **$40M 种子轮**（DCVC 领投）；当天持续占据 Hacker News 头条 | 【官方页面】官方发布博客；【第三方】<https://www.latent.space/p/ainews-jev-a-system-one-model-that>、<https://www.dcvc.com/news-insights/typesafe-emerges-from-stealth-with-a-new-way-of-doing-ai> |
| 2026-09-16 / 09-19 / 09-21 | 媒体报道与集成落地：The Register、HPCwire、MarkTechPost；Spring AI 博客（Java 集成 + 实测）；Latent Space 播客采访 CEO Diogo Almeida | 【第三方】<https://www.theregister.com/ai-and-ml/2026/09/16/typesafe-ai-debuts-model-for-machines-that-plays-doom/5296711>、<https://www.marktechpost.com/2026/09/19/typesafe-ai-releases-jev/>、<https://spring.io/blog/2026/09/21/spring-ai-typesafe-structured-judgment>、<https://www.latent.space/p/jev> |
| 2026-09-22（本报告口径） | 当前版本 **Jev 1.13 / `jev-1.13.0`**；状态 **early access + waitlist、hosted API、无自部署** | 【官方】<https://docs.typesafe.ai/models> |

- 创始人：**Diogo Almeida**（CEO，前 OpenAI，InstructGPT 论文作者之一）、Erik Gafni、Sasha Sheng。（<https://www.truefoundry.com/blog/typesafe-ai-jev>、<https://www.finsmes.com/2026/09/typesafe-ai-raises-40m-in-seed-funding.html>）【第三方】
- 团队自述来自 OpenAI、Google Brain、Meta/FAIR、Stripe、Airbnb、Plaid、Docker 等。（<https://typesafe.ai/team>）【官方口径】
- 别名：`jev-latest`（最近稳定官方版，SDK 默认）、`jev-preview`（最近一次发布，含 preview build）；版本化 ID 始终可用；响应 `model` 字段回报**实际作答的版本化 ID**。（<https://docs.typesafe.ai/models>、<https://docs.typesafe.ai/api>）【官方】（别名与 pin 策略详见 §5.1）
- 契约演进：`POST /preview/evaluation` → `POST /v1/systemone` 是**破坏性变更**（字段大规模改名，见 §3.7）。（<https://docs.typesafe.ai/migrating-to-v1.md>）【官方】
- 官方 SDK 版本（2026-09-22 核对）：PyPI `typesafe-sdk` **0.7.1**（Python ≥3.10）；npm `@typesafe-ai/sdk` **0.6.0**。（<https://pypi.org/pypi/typesafe-sdk/json>、<https://registry.npmjs.org/@typesafe-ai/sdk>）【官方元数据】
- 未披露（多方检索确认无公开信息）：参数量、网络结构（是否 transformer 之外、是否含 classifier/diffusion 成分）、训练算力与硬件、训练数据构成、model card、权重。（Kingy 评测、<https://xhinker.medium.com/jev-the-most-overrated-model-of-2026-even-before-the-year-end-0f86838ca419>）【第三方】

### 2.6 本节不确定项

1. **「非自回归」的精确含义只有官方三句话支撑**（新架构 + parallel sampler + RLCD）；是否有 diffusion/classifier 成分属第三方推测，无官方依据。
2. **$40M 之外的估值（$200M）仅见于 Instagram 帖子**，未在权威来源核实，本报告不采信。【未证实】
3. **官方 FAQ 大面积无答案**：发布博客 FAQ 共 6 问，仅「命名来源」一问有答案；"Is Jev just a smaller LLM?"、"How does Jev perform against public benchmarks?"、"Where does our training data come from?" 等在抓取 HTML 中只有标题、无正文；首页 FAQ 亦仅第一条有答案。这些属**未找到公开信息**。（官方发布博客、<https://typesafe.ai/>）
4. **发布状态会变**：截至 2026-09-22 仍是 early access/waitlist；Rust 项目据此做开发验证时需预留无 API key 的降级路径。
5. **性能数字均为非生产条件**：官方速度多在「公司笔记本 + 美西服务」下测得；第三方样本小（777 条判断、12 段文本），无 p95/p99、并发、可用性、重试率数据。
6. **早期版本与 1.13 的数字不可混用**：官方 cookbook 跨页面出现过 `jev-1.12`、`jev-1.13.0`、`jev-latest`，绝对数字未必同版本同条件。

---

## 3. API 契约与三类问题原语（Choice / Score / Noul）

**要点**

1. 整个 API 面**只有一个核心 endpoint**：`POST https://api.typesafe.ai/v1/systemone`；辅助 `GET /v1/models`。无 `/chat/completions` 兼容端点，也没有批量、异步任务或 webhook 端点。【官方】
2. 请求体恰好三个顶层字段：`state`、`model`、`questions`；**question 的 key（题目 id）不发给模型、不参与推理**，只用于把答案对回来。【官方】
3. 响应体三个顶层字段：`model`（版本化 ID）、`answers`（按同名 id 回填）、`usage`（`input_tokens`/`output_tokens`）。【官方】
4. 只有三种问题原语，用 `type` 区分：**Choice**（无序候选择一，≤255 选项）、**Score**（有序列标尺，2–10 级）、**Noul**（是非概率，无 confidence 字段）；三者共享 `type` + `instructions`，各自带不同形状的 `criteria`。【官方】
5. 计费只看**输入 token**（$0.042 / Mtok ＝ $42 / Btok），**输出 token 免费**；同一请求内多问题共享一次 state 计费，这是批处理收益的来源（完整定价与 credits 机制见 §5.2）。【官方】
6. **未找到任何公开的流式返回支持**（既未承诺也未明确否认）；API 证据支持「一次请求—整包响应」模型。【官方文档 + 源码检索，属负向结论】

### 3.1 Endpoint / base URL / 认证

| 事实 | 内容 | 来源 |
| --- | --- | --- |
| 主 endpoint | `POST https://api.typesafe.ai/v1/systemone` | 【官方】<https://docs.typesafe.ai/api>（本文件首引后简称 /api） |
| 辅助 endpoint | `GET /v1/models`（返回账号可用模型名、描述、发布日期） | 【官方】<https://docs.typesafe.ai/models>（下称 /models） |
| 认证 | 头 `Authorization: Bearer <API_KEY>` + `Content-Type: application/json` | 【官方】/api、<https://docs.typesafe.ai/introduction/quickstart> |
| SDK 环境变量 | `TYPESAFE_API_KEY`、`TYPESAFE_BASE_URL`（默认 `https://api.typesafe.ai`）、`TYPESAFE_DEFAULT_MODEL`、`TYPESAFE_LOG_LEVEL` | 【官方】<https://docs.typesafe.ai/sdk/python/api/constants.md> |
| 迁移确认 | "认证不变：继续发送 `Authorization: Bearer <API_KEY>` 与 `Content-Type: application/json`" | 【官方】<https://docs.typesafe.ai/migrating-to-v1.md> |
| 第三方确认 | LiteLLM pass-through 用同一路径（`GET /typesafe/v1/models` 可用，按 `typesafe/<model>` 计费与记日志） | 【第三方】<https://docs.litellm.ai/docs/pass_through/typesafe> |

- **只在 SDK 源码可见、文档未列出的头**（对 harness 日志/追踪有用）：`X-TypeSafe-SDK`、`X-TypeSafe-Runtime`、`X-TypeSafe-Retry-Count`；响应头 `x-typesafe-request-id`（SDK `request_id` 来源）、`retry-after`/`retry-after-ms`。（<https://raw.githubusercontent.com/typesafe-ai/typesafe-sdk-python/main/src/typesafe_sdk/_core/constants.py>）【官方源码，非文档承诺】

### 3.2 请求体字段与约束

| 字段 | 类型 | 约束 / 说明 | 来源 |
| --- | --- | --- | --- |
| `state` | `string \| object \| array` | 必填。字符串用于普通文本；对象用于命名多段上下文（推荐）；数组用于消息序列或记录序列。**仅纯文本**，不支持图像/音频/视频 | 【官方】/api、<https://docs.typesafe.ai/concepts/state> |
| `model` | `string` | 必填。`jev-latest`（SDK 默认）；`jev-preview` 目前同样指向 `jev-1.13.0`；版本化 ID 始终可用 | 【官方】/api、/models |
| `questions` | `map<string, Question>` | 必填、非空。key 由调用方自取，**不发送给模型、不用于推理**；答案按同一 key 返回 | 【官方】/api、<https://docs.typesafe.ai/sdk/python/api/clients/sync.md> |
| `‹question›.type` | `"noul" \| "choice" \| "score"` | 判别字段 | 【官方】/api |
| `‹question›.instructions` | `string \| object \| array` | 必填。可为结构化对象：问题放一个字段、被引用数据放其他字段，用反引号按名字引用（或点号+索引路径指向 state 嵌套字段，如 `ticket.messages[0].text`） | 【官方】/api、<https://docs.typesafe.ai/primitives/advanced> |
| Choice `criteria` | `map<string, string \| object \| array \| null>` | 必填。**最多 255 个选项**；选项无需说明时用 `null` | 【官方】/api |
| Score `criteria` | `array<string \| object \| array>` | 必填。**有序**等级描述数组，**至少 2 级、API 最多 10 级**；level 0 是第一个元素 | 【官方】/api、<https://docs.typesafe.ai/primitives/score> |
| Noul `criteria` | `object`（可选） | `{ "true": ..., "false": ... }`，分别描述"接近 1 的是"与"接近 0 的非" | 【官方】/api |

- `instructions` 与 `criteria` 的每处描述都允许是 `string | object | array | null`（官方称 `EntryType`）。（<https://docs.typesafe.ai/primitives/advanced>）【官方】
- **上下文预算**：64k tokens / 请求（`state` + 全部问题合计）；其中 **32k** 适用于 `state` + 单个最长问题。（/models）【官方】（口径与第三方渠道差异见 §5.4）
- **state 形状校验**：官方 State 页写 "State must be a string, JSON object, or array of text values."；**裸数字或布尔值（含被序列化成标量的类型）会得到 422**。（/concepts/state；Spring AI 博客）【官方 + 第三方实测】
- **单请求最多多少个问题：官方未声明硬上限**（官方写了选项数 255、层级数 10，却通篇未写题目数上限；已用官方全量语料 `llms-full.txt`（903,311 字节）整库检索复核，确认无相关表述）。（<https://docs.typesafe.ai/llms-full.txt>；第三方独立确认 <https://www.ayautomate.com/blog/how-to-use-jev-guide>）【官方负向结论 + 第三方】
- 同一请求内的问题**互相独立、并行评估**，一个问题的答案不会成为另一个问题的上下文；有真实依赖时须发第二次请求。（<https://docs.typesafe.ai/primitives>）【官方】

### 3.3 响应结构与概率 / 置信度语义

| 字段 | 类型 | 语义 | 来源 |
| --- | --- | --- | --- |
| `model` | `string` | 实际作答的模型，**通常是版本化 ID**（示例均为 `jev-1.13.0`，即使请求发的是别名） | 【官方】/api、/models |
| `answers` | `map<string, Answer>` | 与请求 `questions` 同 key 一一对应 | 【官方】/api |
| Noul `.noul` | `number` | "答案为是"的概率，0（否）到 1（是），接近 0.5 表示不确定。**无 confidence 字段** | 【官方】/api、<https://docs.typesafe.ai/primitives/noul>、<https://docs.typesafe.ai/confidence> |
| Choice `.choice` | `string` | 概率最高的选项 | 【官方】/api |
| Choice `.probabilities` | `map<string, number>` | 每个选项一个概率，官方写 "sum to 1"，SDK JSON schema 写 "sum to approximately 1" | 【官方】/api、<https://docs.typesafe.ai/sdk/python/api/types/responses.md> |
| Choice `.confidence` | `number` | 0–1，由概率分布推导 | 【官方】/api |
| Score `.score` | `number` | **按概率加权的期望值**，可落在两级之间（文档示例 1.05 / 1.6 / 1.7） | 【官方】/api、responses.md |
| Score `.legend` | `map<string, string>` | 级号（字符串 key）→ 该级描述 | 【官方】/api |
| Score `.probabilities` | `map<string, number>` | 级号（字符串 key）→ 概率，和为 1 | 【官方】/api |
| Score `.confidence` | `number` | 0–1，由分布推导 | 【官方】/api |
| `usage.input_tokens` / `.output_tokens` | `integer \| null` | token 用量；SDK schema 允许 `null`（API 未上报时） | 【官方】/api、responses.md |

**confidence 的含义（官方要点）**

- confidence 是**从 `probabilities` 计算出的统计量**，TypeSafe 替你算好并返回；分布越集中在单一结果上越接近 1，越平均越低。（<https://docs.typesafe.ai/confidence>）【官方】
- 官方明确**不锁定该定义**："a convenient measure that fits most use-cases, but you are never locked into our definition … which is exactly why we give you the full `probabilities`"。（同上）【官方】
- 文档交互演示用近似式 **`(n × 最大概率 − 1) / (n − 1)`**（3 选项即 `(3 × 最大概率 − 1) / 2`）——**演示用近似公式，不等于服务端实现**。（同上）【官方】
- v1 相对 preview **换了 confidence 算法**，数值会变；官方给出用归一化香农熵复现 **preview 版**的 Python 代码（`1 − entropy/log(n)`）。（<https://docs.typesafe.ai/migrating-to-v1.md>）【官方】
- 官方建议三档用法：高置信自动执行 / 中置信先确认或补充信息 / 低置信不执行、转人工；阈值按风险分级（破坏性操作高于只读操作）。（<https://docs.typesafe.ai/confidence>）【官方】
- **Noul 无独立 confidence**：二值分布只有一个自由度，`noul` 值完整描述它；且 **0.5 不是"中等"**，它表示"模型分不清"而非所问的量处于中间。（<https://docs.typesafe.ai/primitives/noul>）【官方】

### 3.4 Choice / Score / Noul 逐类字段语义

#### 3.4.1 Choice —— 从一组无序选项中择一

- 请求字段：`type`（`"choice"`）、`instructions`、`criteria`（**选项名 → 评分说明** 的 map，不需要时用 `null`）；**上限 255 个选项**（官方原文："You can have a maximum of 255 options per Choice"）。（/api、<https://docs.typesafe.ai/primitives/choice>）【官方】
- 选项名与描述**都会发给模型**，描述须能相互区分；当选项集合可能覆盖不了全部输入时应显式加 `other` / `none of the above`——因为**模型必须选一个**。
- **不要用 Choice 做"是否适用"**：Choice 概率必然归一，总会有一个赢家；Spring AI 的做法是另加一个 Noul 问"到底有没有适用的工具"。（Spring AI 博客）【第三方实测】
- 实用上限另有官方 cookbook 口径：**"a Choice works reliably up to roughly 240 options"**（75 类行业组一次问完，远在 255 之内）。（<https://docs.typesafe.ai/cookbooks/classification_using_confidence>）【官方】
- 高于 255 基数的做法：官方内部用"**先独立打分、再显式选择**"的两阶段方式，并承认这会**变慢**。（官方发布博客）【官方，仅见于博客，文档正文未展开】
- 选项描述质量直接影响结果：同一工单，仅给裸标签 `billing/technical/sales` → confidence **0.60**；补上描述 → **0.82**。（Spring AI 博客）【第三方实测】
- 低置信度的正确读法（官方 5 题同请求示例）：工单同时属于两个团队（returns 0.61 / billing 0.35），故 `confidence` 仅 **0.42**；`requested_resolution` 的 confidence 仅 **0.20**，示例代码据此**改为向客户确认**而不是直接行动。（<https://docs.typesafe.ai/primitives/choice>）【官方】

#### 3.4.2 Score —— 在有序标尺上取值

- 请求字段：`type`（`"score"`）、`instructions`、`criteria`（**有序列标尺描述数组**，≥2 级、≤10 级）；**层级编号 = 数组下标，从 0 开始**。（/api、/primitives/score）【官方】
- 模型只看到描述文本，看不到编号、也看不到相邻层级，"比上一级更严重"这类写法无意义。官方实测对照：同一份 Safari 报告，描述式三级 → `score 0.0 / confidence 1.0`；**纯数字三级**（`["0","1","2"]`）→ `score 0.55 / confidence 0.33`，概率被摊到 0 与 1 之间。（/primitives/score）【官方】
- 官方写作建议：**描述情境而不是程度**（"Broken or degraded feature, but workaround exists" 优于 "moderately severe"）；每个 Score 只测一个维度；顶部极端情形单独给一级；3 级也够用。（同上）【官方】
- 结构化层级可提升稳定性（官方对照表）：同报告纯文本描述 → `1.43 / confidence 0.35`；给某一级加**贴切的** `examples` → `1.03 / 0.96`；加**不相关**的 examples → 仍为 `1.43 / 0.35`。官方同时警告：**更高的 confidence 并不证明答案更正确**。（同上）【官方】
- **同一 score 值可有完全不同的分布**（1.0 可能全压在 level 1，也可能 0 级与 2 级各半），必须结合 `probabilities` 与 `confidence` 读。（同上）【官方】
- 跨尺度合并前必须归一化：`score / (len(criteria) - 1)` 再加权求和，官方例 `0.6 × 0.62 + 0.3 × 0.64 + 0.1 × 1.0 = 0.664`。（<https://docs.typesafe.ai/patterns/composite-scoring>）【官方】
- 不要用 Score 期望值**插值还原**档位之间的具体数值；Score levels 数值校准很弱，只能用于判"是否过某个阈值"。（<https://docs.typesafe.ai/model-jaggedness/jev-1.13>，下称 jaggedness 页）【官方】
- 官方完整示例（官方 Safari 报告，含官方加权平均验算 `0 × 0.0 + 1 × 0.57 + 2 × 0.43 = 1.43`）：`score 1.43`、`confidence 0.35`、`probabilities {0:0.0, 1:0.57, 2:0.43}`、`usage input_tokens 332 / output_tokens 18`。（<https://docs.typesafe.ai/primitives/score>）【官方】

#### 3.4.3 Noul —— 是非判断，返回概率

- 请求字段：`type`（`"noul"`）、`instructions`、可选 `criteria`（`true`/`false` 两侧语义说明）；返回 `{type, noul}`，`noul ∈ [0,1]` 是"答案为是"的概率，**无独立 confidence**。（/api、/primitives/noul）【官方】
- 写作建议：一个问题只问一件事（两个条件拆成两个 Noul，在代码里组合）；措辞要让**高值 = 是**（"message 是否包含个人数据"优于"message 是否不含个人数据"）；边界含糊时用 `criteria` 钉住定义；先不带 criteria 试、再带 criteria 试，保留本数据上更好的那个。（/primitives/noul）【官方】
- **0.5 不是"中等"**，官方对照实验：同一批"候选人 Python 强度"输入，Noul 给 0.03 / 0.14 / 0.81 / 0.92；同批输入改用 4 级 Score 落在 0.0 / 1.0 / 2.05 / 2.89 → **要测程度就用 Score**。（同上）【官方】
- 官方给出的 `jev-1.13.0` 实测记录表（同一 Noul、不同输入）：

| 输入文本 | `noul` |
| --- | --- |
| "Thanks, that fixed it!" | 0.02 |
| "How do I reset my password?" | 0.07 |
| "I need this sorted today, whatever it takes." | 0.26 |
| "Are you a bot?" | 0.40 |
| "Is there any way to speak to someone about my invoice?" | 0.84 |
| "I have asked three times now. Can I please just talk to a real person?" | 0.99 |

（<https://docs.typesafe.ai/primitives/noul>）【官方】

### 3.5 完整请求与响应示例

#### A. cURL 最小请求（<https://docs.typesafe.ai/introduction/quickstart>）【官方】

```bash
curl -X POST https://api.typesafe.ai/v1/systemone \
  -H "Authorization: Bearer $TYPESAFE_API_KEY" -H "Content-Type: application/json" \
  -d '{"state":"Hi, I'\''ve been trying to connect my Stripe account for 3 days and the integration keeps failing. I'\''m losing sales. Please help ASAP.","model":"jev-latest","questions":{"urgency":{"type":"noul","instructions":"Does this message express urgency?"}}}'
```

#### B. 三类原语同请求（官方 Quickstart 原文；为紧凑起见合并了部分缩进，字段与取值与原例一致）【官方】

```json
{
  "state": "Hi, I've been trying to connect my Stripe account for 3 days and the integration keeps failing. I'm losing sales. Please help ASAP.",
  "model": "jev-latest",
  "questions": {
    "department": { "type": "choice", "instructions": "Which team should handle this",
      "criteria": { "billing": "Payment or subscription issues", "technical": "Bugs or integration problems", "sales": "Pricing or account questions" } },
    "frustration": { "type": "score", "instructions": "How frustrated the customer appears",
      "criteria": ["Calm, just stating facts", "Frustrated but civil", "Very angry, strong language"] },
    "is_urgent": { "type": "noul", "instructions": "The message conveys urgency or time-sensitivity" }
  }
}
```

对应响应（**数值是示例值，不是可复现结果**）：

```json
{
  "model": "jev-1.13.0",
  "answers": {
    "department": { "type": "choice", "choice": "technical", "confidence": 0.78,
      "probabilities": { "technical": 0.85, "sales": 0.0, "billing": 0.15 } },
    "frustration": { "type": "score", "score": 1.0, "confidence": 1.0,
      "legend": { "0": "Calm, just stating facts", "1": "Frustrated but civil", "2": "Very angry, strong language" },
      "probabilities": { "0": 0.0, "1": 1.0, "2": 0.0 } },
    "is_urgent": { "type": "noul", "noul": 1.0 }
  },
  "usage": { "input_tokens": 392, "output_tokens": 65 }
}
```

#### C. 三类原语各自的官方最小示例（`/api`，`//` 行为阅读标注、非请求内容）【官方】

```jsonc
// Choice 请求
{"state":"Help! My payouts have been failing for 3 days.","model":"jev-latest","questions":{"department":{"type":"choice","instructions":"Which team should handle this?","criteria":{"billing":"Payments, invoicing, refunds","technical":"Bugs, outages, integrations","sales":"Pricing, upgrades, new accounts"}}}}
// Choice 响应
{"model":"jev-1.13.0","answers":{"department":{"type":"choice","choice":"billing","probabilities":{"billing":0.88,"technical":0.12,"sales":0.0},"confidence":0.81}},"usage":{"input_tokens":318,"output_tokens":34}}
// Score 请求
{"state":"Help! My payouts have been failing for 3 days.","model":"jev-latest","questions":{"frustration":{"type":"score","instructions":"How frustrated is the customer?","criteria":["Calm","Frustrated","Very angry"]}}}
// Score 响应
{"model":"jev-1.13.0","answers":{"frustration":{"type":"score","score":1.05,"legend":{"0":"Calm","1":"Frustrated","2":"Very angry"},"probabilities":{"0":0.0,"1":0.95,"2":0.05},"confidence":0.92}},"usage":{"input_tokens":304,"output_tokens":18}}
// Noul 请求（带 criteria）
{"state":"Help! My payouts have been failing for 3 days.","model":"jev-latest","questions":{"is_urgent":{"type":"noul","instructions":"Does this convey urgency?","criteria":{"true":"Explicitly time-sensitive","false":"No urgency expressed"}}}}
// Noul 响应
{"model":"jev-1.13.0","answers":{"is_urgent":{"type":"noul","noul":0.95}},"usage":{"input_tokens":296,"output_tokens":20}}
```

- LangChain 博客给出的同一"是否紧急"问题变体返回 **0.999**，并称"99.9% 概率紧急"；上游注明：不同页面示例值不同（0.95 / 1.0 / 0.999）属各自示例，不代表官方矛盾，**均不可当基准数据**。（<https://www.langchain.com/blog/building-a-harness-with-jev>）【第三方转述】

#### D. 实现细节（对 Rust 客户端直接相关）

- **HTTP 层** `legend`/`probabilities` 的 key 是**字符串**级号（`"0"`,`"1"`,`"2"`）；**Python SDK 层** `ScoreAnswer.legend` 是 `dict[int, ...]`、`probabilities` 是 `dict[int, float]`，即 **SDK 用整数 key**。（/api、responses.md）【官方】
- JS/TS SDK：`answers.category.choice` 的类型由 options 推导成字面量联合类型。（<https://docs.typesafe.ai/sdk/javascript>）【官方】
- 客户端校验：官方 JS SDK `systemOne()` 声明会抛出 **"Questions are empty, or score criteria are not a list of at least two entries."**，即"空 questions"与"Score 层级少于 2 条"在客户端即被拦下。（<https://docs.typesafe.ai/sdk/javascript/api/classes/TypeSafeClient>）【官方】
- Python SDK 签名：`system_one(state, questions, *, model=None, retry=None, timeout=None, extra_headers=None, extra_body=None, response_model=None)`；`extra_body` **浅合并**进顶层请求体且"最后写入优先"（同名 key 会覆盖 `state`/`model`/`questions`）；有 `response.nouls/.choices/.scores` 分组访问器与 `response.request_id`。（<https://docs.typesafe.ai/sdk/python/api/clients/sync.md>）【官方】

### 3.6 并行批处理与计费

**并行机制（官方口径，多来源一致）**

- 一次请求里的所有问题**共用同一份 state、并行且相互隔离地评估**；答案之间不构成上下文，可随意增删问题而不影响其它问题。（<https://docs.typesafe.ai/primitives>）【官方】
- **真依赖必须落在代码里**：若后一问依赖前一答案（用答案取数据、决定 state 构成、或决定下一题选项），须发第二次请求；除此之外官方建议全部放进一次请求、用代码忽略不需要的答案——即 **Speculative fan-out（推测式扇出）**。（<https://docs.typesafe.ai/patterns/fan-out>）【官方】
- 增加问题"几乎不增加响应时间"，只增加问题本身的 token："costs only the tokens for the extra questions, which are cheap"；"Asking a question you might not need is close to free"。（同上、官方发布博客）【官方】

官方 cookbook 量化实测（<https://docs.typesafe.ai/cookbooks/parallel_questions>，下称 parallel cookbook）【官方】：

| 策略 | 调用次数 | 成本 | 耗时 |
| --- | --- | --- | --- |
| 批量（13 题同请求） | 1 | **$0.000497** | **0.27 s** |
| 单题串行（13 次请求） | 13 | $0.006090 | 2.71 s |

- 结论：**12.2× 更便宜、10.0× 更快**，且**批处理没有改变答案、也没有引入额外方差**（`RUNS = 5`；多为 std dev = 0.0，`breach_72h`（0.804 vs 0.814）与 `criminal_penalties`（std 0.0045 vs 0.0084）是题目自身固有采样噪声，两种策略量级相同）。【官方】
- 测试条件：GDPR 的 Wikipedia 固定修订版正文，**53,777 字符**（页面另处写作 ~54,000）；13 题 = 8 Noul + 2 Choice + 3 Score；文档字节级一致；成本按 `PRICE = (0.042, 0.00) # $ per 1M tokens (input, output)` 事后计算；延迟为 13 次单题延迟之和（**顺序调用假设**）。【官方】
- **⚠️ 官方内部数字不一致（必须并列保留）**：同为官方页面，`/primitives` 总览页写 **"11.5x cheaper / 9.6x faster"**，cookbook 页写 **"12.2x / 10.0x"** 并附原始金额表。按 cookbook 原始数字计算：0.006090/0.000497 ≈ **12.25×**、2.71/0.27 ≈ **10.04×**，即 **cookbook 页自洽、总览页数字疑为过时或笔误**。**引用时建议用 12.2×/10.0× 并注明页别；本报告后续凡涉此数字均写作「12.2×/10.0×（官方另一处口径 11.5×/9.6×）」**。（<https://docs.typesafe.ai/primitives>、parallel cookbook）【官方，内部口径冲突】
- **并发注意（官方自陈）**：单题延迟之和假设顺序调用；若并发发起，**速度差会缩小，但 13× 的 token 成本不变**。【官方】
- 第三方复述与补充：LangChain 博客（并行、增加问题几乎不影响响应时间、成本只增问题 token）；Spring AI 博客给出笔记本实测 **1 题 275ms / 3 题 310ms**（多 2 题多约 35ms），并写 "The service reads the state once and answers every question in parallel (for negligible cost)"。（<https://www.langchain.com/blog/building-a-harness-with-jev>、Spring AI 博客）【第三方】

**计费（机制层）**（/models）【官方；完整定价、credits 与渠道差异见 §5.2】

| 项目 | 值 |
| --- | --- |
| 输入 | **$0.042 / 1,000,000 tokens**（页面同时写作 **$42 / Btok**） |
| 输出 | **免费**（"Charged per input token. Output tokens are free."） |
| 计量单位 | 响应体 `usage.input_tokens` / `usage.output_tokens`（输出免费但仍报告 token 数） |

- **成本模型关键推论**：state 每次请求只计一次费，因此"把 N 个问题放进一次请求"省下的是 state 的 (N−1) 次重复计费；问题本身 token 照价计费但很便宜。这解释了 13 题批量为何省约 12×——文档主导型负载下，13 次单题请求把同一份 5.4 万字符正文付了 13 遍。【官方】
- 官方自我限定："We can't prove it isn't subsidized; we'll need the long-term to prove the sustainability of our pricing."（官方发布博客）【官方】
- 另一份官方 cookbook（self-consistency）声明其成本用的是"历史价格假设，**不是经过核实的 jev-latest 价格或当前账单金额**"——引用单个 cookbook 绝对金额时需保留这句限定。（<https://docs.typesafe.ai/cookbooks/consistency_noul_cookbook>）【官方】
- **未找到公开信息**：是否存在最低计费单位、失败/重试请求是否计费、429 退避重试是否重复计费。Elixir 客户端作者指出传输层失败后重放 "ambiguous: the server may already have processed and billed the evaluation"，故默认不重放——属第三方工程判断。（<https://hexdocs.pm/typesafe_ai/readme.html>）【未证实】

### 3.7 契约演进史（交叉印证 v1 字段语义）

`POST /preview/evaluation` → `POST /v1/systemone` 是**破坏性变更**，迁移对照正好交叉印证 v1 字段含义（<https://docs.typesafe.ai/migrating-to-v1.md>）【官方】：

| preview | v1 |
| --- | --- |
| `prompts`（数组，每项自带 `key`） | `questions`（map，key 即 id） |
| `document` | `state`（传 `document` 校验失败） |
| `criteria`（Noul）/ `options`（Choice）/ `levels`（Score） | 统一为 `criteria` |
| `responses`（数组，同序） | `answers`（map，按 id） |
| Noul 值 `probability` | `noul` |
| Choice 值 `chosen` | `choice` |
| Score 值 `expectation` | `score` |
| Choice `probabilities`：`[{option, probability}]` 数组 | `{option: probability}` map |
| Score 无 `probabilities` | v1 新增 `{level: probability}` map |
| confidence 旧算法 | 新算法（数值会变，**需重新校准阈值**） |
| `usage.billing_units` 占位 | `input_tokens` / `output_tokens` |
| Python 包 `typesafe-client`（所有 0.1.x/1.0.x 均已失效） | `typesafe-sdk` |

### 3.8 错误码、限流、重试与流式（契约层）

| 状态码 | 含义 | 来源 |
| --- | --- | --- |
| `401 Unauthorized` | API key 缺失或无效，检查 `Authorization` 头 | 【官方】/api |
| `422 Unprocessable Entity` | 请求体校验失败（缺必填字段、问题格式错误），**响应体指出出错字段** | 【官方】/api |
| `429 Too Many Requests` | 超出限流，退避后重试 | 【官方】/api |
| `529 Overloaded` | 服务暂时过载，短暂延迟后重试 | 【官方】/api |
| `400/401/403/404/422/429/5xx` | SDK 异常类型映射（`TypeSafeBadRequestError`/`AuthenticationError`/`PermissionDeniedError`/`NotFoundError`/`UnprocessableEntityError`/`RateLimitError`/`InternalServerError`） | 【官方】<https://docs.typesafe.ai/sdk/python/api/exceptions.md> |

- 限额（jev-1.13）：**250,000 tokens/秒 且 1,200 requests/分钟**，任一超限返回 `429`；官方 Warning 明确 **限流数值会动态调整、可能无预警变化**；更高限额走 custom/enterprise。SDK 默认指数退避并尊重服务端 `retry-after`。（/models）【官方】（完整平台侧口径见 §5.3）
- 恢复策略：收到 `429` 或 `529` 时**指数退避重试**，不要立即重试。（/api "Handling rate limits"）【官方】
- Python SDK 默认重试策略：`max_retries=2`、`backoff_initial=0.5s`（每次翻倍、上限 `backoff_max=5.0s`）、`backoff_jitter=0.25`、重试状态码 `{408, 429, 500–599}`、`respect_retry_after=True`、重试总预算 `timeout=30.0s`；`TypeSafeRateLimitError.retry_after_ms` 暴露服务端要求的等待毫秒数；单次 HTTP 操作默认超时 `DEFAULT_TIMEOUT = 10.0s`。（<https://docs.typesafe.ai/sdk/python/api/retries.md>、constants.md、exceptions.md）【官方】
- JS SDK 默认值同构：`maxRetries=2`、`backoffInitialMs=500`、`backoffMaxMs=5000`、`backoffJitter=0.25`、重试 408/429/500–599、`respectRetryAfter=true`、`maxRetryAfterMs=60000`（更长的服务端延迟改用退避）。（<https://docs.typesafe.ai/sdk/javascript/api/interfaces/RetryPolicy.md>）【官方】
- **⚠️ 两套 SDK 的默认值口径不一致**（见 §10.4）：重试状态码集合与重试次数在文档/源码层面表述不同，本项目应自行定档，不依赖 SDK 默认。
- 官方 cookbook 并发经验值：entity-alignment 的并发池为 `MAX_WORKERS = 6`，注释写 "the public endpoint rate-limits above roughly eight"（约 8 并发以上开始限流）。（<https://docs.typesafe.ai/cookbooks/entity_alignment>）【官方】
- **流式：未找到任何公开支持**。官方 API reference 无流式参数/响应类型/事件协议；全站纯文本 `llms-full.txt`（903 KB）检索 `stream`/`server-sent`/`text/event-stream`/`websocket` **无任何 API 相关命中**（唯一相关表述是"编码 agent 依赖会 stream 文本的 LLM … **Jev does none of that**"，<https://docs.typesafe.ai/introduction/coding-agents>）；JS SDK 仅 `systemOne()` 与 `models.list()` 两个方法，源码中响应处理函数名为 `bufferResponse`（先整包读入）；Python SDK 源码检索 `stream` 无命中。检索命令：`curl -sL https://docs.typesafe.ai/llms-full.txt | grep -in "stream\|text/event-stream\|websocket"`。第三方佐证：Spring AI 写 "Jev does not stream. Nothing is generated token by token"。（<https://docs.typesafe.ai/sdk/javascript/api/classes/TypeSafeClient.md>、<https://raw.githubusercontent.com/typesafe-ai/typesafe-sdk-js/main/src/client.ts>、<https://github.com/typesafe-ai/typesafe-sdk-python>、Spring AI 博客）【官方负向结论 + 第三方】

### 3.9 本节不确定项

1. **错误响应体的 JSON 结构没有官方契约**。API reference 只说 422 的 body "details the offending field"。Python SDK 源码 `extract_message()` 是**容错解析**（依次尝试 `error` → `message` → `detail`，含 FastAPI 风格 `[{loc, msg}]`），**暗示**服务端可能是 FastAPI 校验错误，但官方未承诺，不应据此写死解析逻辑。（<https://raw.githubusercontent.com/typesafe-ai/typesafe-sdk-python/main/src/typesafe_sdk/_core/errors.py>）【未证实（源码推断）】
2. **`529 Overloaded` 未列入 SDK 状态码映射表**（表中只有 400/401/403/404/422/429），它按 `status >= 500` 归入 `TypeSafeInternalServerError`；因重试集合含 500–599，实际仍会被重试。属源码推断，非文档声明。
3. **`429` 是否总带 `retry-after` 不确定**：文档措辞是"honor the `retry-after` header when the response carries one"，即并非保证存在；`retry-after-ms` 只在 SDK 源码出现，文档未提。
4. **confidence 的具体公式未公开**：官方只给"分布集中度"定性描述与一个**演示用**近似式；v1 相对 preview 换了算法但未公布新公式。把 confidence 当阈值用应在自有数据上校准。
5. **`probabilities` 是否严格和为 1**：API reference 写 "floats that sum to 1"，SDK JSON schema 写 "sum to approximately 1"；设计上应按"近似"处理（例如 255 选项时的浮点误差）。
6. **单请求问题数上限未公开**（只有 64k 预算与"每个问题消耗少量 token"的定性说明）；**计费边界情形**（最低计费单位、失败/重试是否计费）同样未找到公开信息。
7. **示例数值不可复现**：Quickstart 与 API reference 的 `confidence`/`probabilities`/`noul` 均为示意值（同一句 urgency 问题在不同页面给出 0.95、1.0，LangChain 博文给出 0.999），**不能当基准数据**。
8. **流式支持仅证明"未找到"**：官方既未承诺也未明确否认；`coding-agents` 页的表述是模型能力层面的说法，不等于 API 永不会有流式。
9. **官方内部字段列表不一致（小）**：`/introduction` 总览表把 Score 返回写成 "score, probabilities, confidence"，**漏了 `legend`**；`/api` 与 `/primitives/score` 都含 `legend`（实测 JSON 中确有）。以 /api + /primitives/score 为准。
10. **批处理收益数字冲突未解决**：12.2×/10.0×（cookbook，自洽）vs 11.5×/9.6×（`/primitives` 总览页）——两处都是官方口径，本文并列保留，不做单一化。
11. **官方未声明 v1 API 的版本化承诺**（废弃策略、兼容窗口）；SDK 版本号（Python 0.7.1 / JS 0.6.0）只说明客户端仍在快速迭代。
12. **无采样旋钮**：Pydantic AI 集成文档称 `temperature`/`top_p` 等设置被忽略。（<https://pydantic.dev/docs/ai/models/typesafe/>）【第三方，官方文档未见明文，属待核】
13. **未成功获取的来源**：DataCamp 文章 <https://www.datacamp.com/blog/system-one-models-jev> 返回 HTTP 403，未纳入证据。

---

## 4. 能力边界、失败模式与已知限制

**要点**

1. 官方在 `jev-1.13` **jaggedness 页自列 9 类失败模式**，并给出每类"改用代码/生成模型"的替代做法，是能力边界最权威的一手清单。【官方】
2. **"不能幻觉"是形状保证，不是正确性保证**——官方博客自陈 0% 幻觉率 **"not empirical"**（非实证），是 schema 匹配的构造性结论。【官方】
3. **校准是群体统计，不保证单个答案**；且官方**未公布任何 ECE / reliability diagram 之类的标准校准指标**，第三方普遍将此作为核心质疑。【官方 + 第三方】
4. **结构性不变式不成立**：同一问题的 Noul 与 Choice 结果可相差极大（Noul 0.22 vs Choice "yes" 0.01），`P(noul)` 与 `1 − P(¬noul)` 不可比（0.72 + 0.47 = 1.19）——**阈值不能跨问法搬运**。【官方】
5. **官方评测站数字显示 Jev 在成本与延迟上压倒性领先，但不是准确率领先者**（四任务等权 67.8%，最高为 sol 74.1%）；逐项数据与口径分析见 §6.2，本节只取其对能力边界的含义。【官方】
6. **硬限制明确**：64k/32k 上下文、Choice ≤255、Score 2–10 级、仅文本输入、英文最佳、无微调、限流 250k tok/s + 1,200 rpm。【官方】

### 4.1 官方自列的 jaggedness 九条（`jev-1.13`）

官方原文开头即承认："jev-1.13 is fast, calibrated, and good at common-sense judgment but **it is not perfect**… It may struggle with tasks that require additional levels of indirection. It can be quite literal in its understanding. It struggles with tasks that require numeric precision."（jaggedness 页 <https://docs.typesafe.ai/model-jaggedness/jev-1.13>）【官方】

| # | 失败模式 | 官方给出的现象 / 建议 |
| --- | --- | --- |
| 1 | **字面理解**（scoping / 否定 / 隐含条件按字面读） | "answers the question you wrote, not the one you meant"；把确切条件写进 instructions、边界情况写进 criteria；必要时拆成两个字面问题再在代码里组合 |
| 2 | **数学与数字**（含计数） | 不做计算；**计数不可靠**（字母数、词频、长列表计数，误差随规模增长）；hex/RGB 等数值化表示弱于英文名；不要用 Score 期望值反推精确数值；计数在代码里数，按条件计数则逐项问 Noul 再代码加总 |
| 3 | **日期时间比较** | "reads dates as text, not as ordered quantities"，混格式/相对表述/季度与结算窗口更差；抽取归模型（做成封闭集合的 Choice，含"未提及"选项），排序/时长/偏移全归代码 |
| 4 | **多跳 / 间接推理**（双重否定、属性之属性） | 跳数越多越不准；指令尽量直接、点名 state 字段 |
| 5 | **大而嘈杂的 state** | 无关细节起干扰作用、准确率下降且难定位错因；先检索/筛选再送，无法筛选时可用 Noul 做相关性过滤 |
| 6 | **对抗性内容** | state 中的注入指令、误导性框架、自称应被分类为某类的文本都会移动答案；官方称未来会改进；建议精确写出 criteria 并充分测试边界用例 |
| 7 | **instructions 与 criteria 相互矛盾** | 例如 Noul 中 true 映射为"否"，会明显变差（"might get confused"）；应把 criteria 当作 instruction 的延伸，语义对齐 |
| 8 | **结构性不变式不成立** | 官方实测反例：同一问题 Noul 得 **0.22**、Choice "yes" 得 **0.01** 且 confidence **0.99**；同一问题与其否定作两个 Noul 得 **0.72** 与 **0.47**（和为 **1.19**）。结论：**不要假设 P(a) 与 1−P(¬a) 可比，不要把 Noul 上调好的阈值搬到 Choice，也不要在不同问题之间要求算术恒等**；Choice 是相对的、Noul 是绝对的（所有 Noul 可同时低） |
| 9 | **生成能力** | `jev-1.13` 未受文本生成训练；用 Choice 串联生成 "**will not work well and will be very slow**"；抽取场景建议先用正则或生成模型取候选，再让 Jev 选 |

- 另注：官方称该模型 "**extremely consistent**"——语义相似输入会得到量化上相似的输出。（同上）【官方】
- CEO 在访谈中承认："单跳判断你不该用别的东西（只能用 Jev）；跳数一多就开始退化。"（<https://www.latent.space/p/jev>）【官方口径（访谈）】
- 官方演示（Doom、Wikiracing）中作者也标注对比条件对自己有利：Wikiracing 的 LLM 对手被设成 non-reasoning 模式（"to make the demo more bearable to watch"），Doom 输入是结构化文本而非图像。（官方发布博客）【官方自陈】
- **同一批失败模式的"提问工程视角"补充（歧义、选项不互斥、等级重叠、计数、数值表示、结构不变式、过度依赖概率）见 §8.6**，本节不重复列举。

### 4.2 Jev 明确不做的事 / 不适合交给它的任务

| 不做 / 不适用 | 原文或依据 |
| --- | --- |
| 不生成文本、不写代码、不做对话、不做工具调用/文件编辑/流式 | "System One models do not write replies, produce code, or generate explanations of their reasoning."（<https://docs.typesafe.ai/concepts/system-one>）；"Jev is not a chat or code-completion LLM… Jev does none of that."（<https://docs.typesafe.ai/introduction/coding-agents>）【官方】 |
| 不替代驱动 coding agent 的 LLM | "Jev is not a drop-in replacement for the LLM behind Claude Code, Cursor…"（同上）【官方】 |
| 不做算术、计数、日期运算 | "Jev is not a calculator. We strongly recommend implementing any mathematical logic in code."；"jev-1.13 does not count reliably… The model recognizes the shape of an answer rather than tallying"（jaggedness 页）【官方】 |
| 不做多跳推理 | "It may struggle with tasks that require additional levels of indirection."（同上）【官方】 |
| 不保留问题之间的上下文 | "All questions see the same state and are evaluated independently."（<https://docs.typesafe.ai/concepts/state>）【官方】 |
| 默认不把 state 当敌意输入 | "State is data, and jev-1.13 does not treat it as hostile by default."（jaggedness 页）【官方】 |
| 不做按客户微调 | "Jev is not fine-tuned or LoRA-adapted with customer data… the same weights serve every account."（/models）【官方】 |
| 不适合当安全边界 | 官方 RAG 注入 cookbook 写 "The injection question is a filter, and only one… **Nothing here is a security boundary.**"（<https://docs.typesafe.ai/cookbooks/classifying_rag_passages>）【官方】 |
| 不适合 >255 的高基数选择 | 需两级（先独立打分再显式选择），官方承认会**变慢**（官方发布博客）【官方】 |
| 不适合需要绝对可靠的判定、承受恶意输入的判定 | 官方自述对抗性内容可移动答案；第三方交叉参考：LLM 仍赢在生成文本/代码、多跳推理、算术与日期运算，以及"量少但一次错就很贵"的任务（<https://www.firecrawl.dev/blog/what-is-jev>）【第三方】 |

- 官方不保证输出幂等：同一 state 重复调用概率仍会小幅波动（见 §4.4 的 `covered` 0.43–0.53）。【官方】
- 把问题写进 `state`（prompt）里**不会被当作指令**：写到 state 里的问题只是"待判断的文本"；框架层几乎没有东西会替你报错（集成层只有在完全没有可提问题时才报 UserError）。（<https://pydantic.dev/docs/ai/models/typesafe/>）【第三方（框架集成文档）】

### 4.3 硬限制一览

| 限制项 | 数值 | 来源 |
| --- | --- | --- |
| 单请求总预算 | **64k tokens**（state + 所有 questions 合计） | 【官方】/models |
| state + 最长单个问题 | **≤ 32k tokens** | 【官方】/models |
| Choice 选项数 | **≤ 255**（API 硬上限） | 【官方】/api、<https://docs.typesafe.ai/primitives/choice> |
| Choice 选项数（**实用**上限） | "a Choice works reliably up to roughly **240** options" | 【官方】<https://docs.typesafe.ai/cookbooks/classification_using_confidence> |
| Score 档位数 | **≥ 2，≤ 10** | 【官方】/api、/primitives/score |
| state 形态 | `string` / JSON `object` / JSON `array`（文本值数组）；**仅文本**，无图像/音频/视频；非文本输入须自行预处理为文本或结构化字段 | 【官方】/models、/api、<https://docs.typesafe.ai/concepts/state> |
| 语言 | 英语是主要训练语言且准确率最好；其他语言（含 CJK）"handled but not equally well"，需自测并关注 confidence | 【官方】/models |
| 限流 / 错误语义 | 250,000 tokens/秒 与 1,200 请求/分钟；超限 **429**；另有 **529 Overloaded**；401 鉴权失败；**422 请求体校验失败**（body 指出字段）；SDK 默认退避并遵循 `retry-after`；限流数值会动态调整 | 【官方】/models、/api |
| 采样参数 | **没有 temperature / top_p 等旋钮**（集成文档称这些设置被忽略） | 【第三方（框架集成文档）】<https://pydantic.dev/docs/ai/models/typesafe/> |
| 版本漂移 | `jev-latest` 会随发布移动，官方建议调过阈值的场景 **pin 到版本化 ID**（如 `jev-1.13.0`）；响应 `model` 字段回报实际版本 | 【官方】/models |
| 单请求问题数上限 | **官方未声明**；实际受 64k/32k 预算与 1,200 req/min 约束（工程推算，非官方数字） | 【官方负向结论】/api、/models + 全量语料检索 |

- **实际可用容量小于纸面上限**：jaggedness 第 5 条说明 state 越长越嘈杂、准确率越低。parallel cookbook 里"文档为主"的示例是 GDPR 维基全文 **约 54,000 字符**（≈1.3–1.4 万 token 量级），可作为"官方认为合理的大文档 state"的锚点。（parallel cookbook）【官方】
- 官方对上下文过长有专名警告 **"context rot"**，建议先在代码里过滤，而不是把长上下文直接塞进去。（jaggedness 页）【官方】

### 4.4 校准与置信度的可靠性边界

**官方怎么说**

- confidence 是由概率分布形状导出的 0–1 统计量，**只出现在 Choice / Score 上；Noul 不带 confidence**。（<https://docs.typesafe.ai/confidence>）【官方】
- **confidence ≠ 正确性**：Score 文档原文 "confidence 1.0 means the returned distribution puts all its probability on one level. **This describes the model's answer, not a guarantee that the answer is correct.**"（<https://docs.typesafe.ai/primitives/score>）【官方】
- **校准被限定为群体统计**："Outcomes assigned a probability of 0.8 should occur about 80% of the time… **These rates describe groups of predictions, not a guarantee about any single answer.**"（<https://docs.typesafe.ai/introduction/machine-learning-primer>）【官方】
- 集成方把 confidence 进一步明确定义为**边界余量（margin）**："It is a **margin, not a probability that the answer is right**."（<https://pydantic.dev/docs/ai/models/typesafe/>）【第三方（框架集成文档）】
- 官方可操作做法：confidence 划三段（高→自动执行、中→复核/追问、低→转人）；阈值随风险调整。（<https://docs.typesafe.ai/confidence>）【官方】

**官方拿得出的量化证据**

| 证据 | 数字 | 语境与出处 |
| --- | --- | --- |
| 置信度分层确实有效 | 60 份 10-K、75 类 SIC 行业组、单条 Choice：confidence ≥ 0.9 的一半准确率 **90%**；< 0.9 的一半仅 **40%**；后者改报 division 可升到 **70%** | **jev-1.12**，2026-08-12，官方 cookbook <https://docs.typesafe.ai/cookbooks/classification_using_confidence>【官方】 |
| 同 state 重复调用稳定性 | 14 个 Noul × 15 次重复：每问题概率标准差 **0.0102**（低于同测所有 LLM 概率条件）；但 `covered` 一题在 **0.43–0.53** 间波动、**跨越 0.5 决策线** | jev-latest，2026-09-11，官方 cookbook <https://docs.typesafe.ai/cookbooks/consistency_noul_cookbook>【官方】 |
| 官方对不确定区间的自我限定 | 0.30–0.70 的"不确定带"被标注为 "**neither a calibrated guarantee nor an optimized threshold**" | 同上【官方】 |
| 官方对 LLM 校准弱点的动机陈述 | "Even if prompted for a confidence estimate, models tend to be overconfident and inconsistent." | 官方发布博客【官方】 |
| 官方对自身方法的口径 | Jev 用 RLCD 训练，目标是让「更高的 confidence 对应更高的准确率」——属**官方口径**，未给校准曲线 | 【官方】§8.3 |

**边界与风险（第三方指出、官方未反驳）**

- **没有公开的校准曲线 / ECE / reliability diagram**：官方只断言校准，未发布可核对的校准度量。（<https://www.laura-martel.com/blog/typesafe-jev-system-one-models>、<https://www.truefoundry.com/blog/typesafe-ai-jev>、Kingy 评测）【第三方】
- **唯一直接测 ECE 的第三方实验给出不利结果**：钓鱼任务上 Jev **ECE 0.154** vs Claude Haiku 4.5 **0.097**（10 bins）；完整对照见 §6.4。【第三方】<https://github.com/anisselbd/jev-phishing-bench>
- **官方评测的参考标签不是真值**，而是 GPT-6 Astra 与 Claude Fable 5.1（both at high thinking）回答的平均；评测站还写明 "**Assume the harness is correct**"。那批数字是"与两个强模型的一致度"，不是"与现实的一致度"。（<https://evals.typesafe.ai/>）【官方】
- **组合会破坏置信度语义**：单步 92% 的分类接 88% 的路由并不等于 81%，真实可靠性取决于误差相关性，而 "neither Jev nor the pipeline framework can report"。（<https://temperaturezero.com/2026/09/16/jev-doesnt-hallucinate-it-decides-wrong/>）【第三方】
- **合法输入下也可能"自信地错"**：创始人本人在 HN 回帖承认 "because these models are probabilistic, it's also possible to be confidently wrong"。（<https://news.ycombinator.com/item?id=49717558>）【第三方（创始人身份发帖）】
- 官方仍未公开：RLCD 具体实现、校准如何度量、分布偏移/长尾标签下概率是否仍校准、p95/p99 延迟与并发表现。（Kingy 评测）【第三方整理】

### 4.5 "零幻觉"的确切含义

官方原话（官方发布博客）【官方】：

> "While Jev gives up string generation, it's optimized for structured outputs and can't hallucinate."

同页 Nuance 里官方自己拆台：

> "**Our number is not empirical.** Schema matching is guaranteed, thus we can confidently add 0% into the plots."

因此"零幻觉"成立的前提是：

1. **它只断言"输出不越出你声明的 schema"**：选项只有 `billing/technical/sales` 时不会返回第四个标签（第三方表述："It will not invent 'urgent_priority'. It will not return null. It will not return a float."）。官方称 "no type errors… mathematically impossible" 并欢迎用反例证伪——这是一个**可被单个反例证伪的构造性保证**。（jaggedness 页、temperaturezero）【官方 + 第三方】
2. **它不覆盖**：选错合法选项、读漏要点、给错误答案一个很高的概率/置信度。第三方措辞：**"Can't hallucinate" is a narrower claim than it sounds**；The Register 直接说 "which really isn't a fair comparison as its output is not natural language"。（<https://www.firecrawl.dev/blog/what-is-jev>、The Register）【第三方】
3. **"类型安全"本身并不新颖**：第三方指出靠 grammar-constrained decoding（OpenAI Structured Outputs、Outlines 等）在自回归 LLM 上早已能做到同等的形状保证；真正未被独立验证的是**校准**。（Laura Martel）【第三方】
4. **术语歧义提醒**：官方训练法 RLCD 与 ICLR 2024 的 RLCD 重名，引用时须写清（见 §2.2）。（同上）【第三方】
5. **HN 社区的主流解读与此一致**："Type safety is not factual correctness."；"An approve for an unauthorized action still meets the schema guarantee."（见 §12.2）【第三方】
6. **治理视角的缺口**：没有文本输出意味着没有"可引用的陈述"，与既有审计框架的映射 *partial coverage at best*（属第三方安全媒体分析）。（<https://venturebeat.com/security/companies-are-putting-jev-in-charge-of-ai-agent-decisions-and-prompt-injection-can-influence-the-verdict>、<http://wimes.org/articles/2026-09-16-typesafe-jev-cant-hallucinate-exactly-the-problem/>）【第三方】

**建议固定的术语（避免"零幻觉 = 零错误"的语义滑移）**：

- **类型/结构幻觉**按设计被消除（输出空间预先枚举）；
- **语义判断错误**仍存在，必须自己测（官方评测站与 cookbook 都在测这个）；
- 误用后果类型学：把"类型合法但错误、且置信度偏高"的判定静默送进下游自动化——比幻觉更难被发现，因为"段落至少能被人读到并质疑，而 Jev 的输出会静默进入下一步"。（temperaturezero）【第三方】

### 4.6 本节不确定项

1. **校准是否真的成立**：没有公开的 ECE / reliability diagram / 独立复测。官方只有"设计目标 + 分层证据（90% vs 40%）"，第三方只有"未公开"这一事实。**不要写"Jev 已证明校准良好"。**
2. **官方博客 FAQ 的正文答案未取到**（折叠组件，抓取只有问题没有答案）。
3. **各 workflow 的样本量 N（240/117/150/204，合计 711）**：只见于第三方复核文章，未能在官方页面静态数据中核实；引用时须标注来源为第三方。（Kingy 评测）【未证实】
4. **193.6× / 444.6× 一类倍数**：官方自述来自自建 workflow evals 且"expected to be on the higher end"；从评测站四舍五入后的表**无法反推**。不要当通用倍数使用（复算与分级见 §6.5）。
5. **输出 token 的免费与用量字段**：定价写 output tokens FREE，但第三方给的响应示例里 `usage` 含 `output_tokens: 48`；是否总会返回、是否计费口径一致，未见官方明确说明。
6. **对非英语（含中文）表现**：官方只说 "handled but not equally well"，**没有任何具体数字**；也未找到不同语言在 Score 层级语义上行为差异的公开信息。
7. **对抗鲁棒性的量化**：官方只说"可以移动答案、未来会改进"，**无任何成功率/攻击成功率数字**。
8. **长期价格可持续性**：官方自己写 "We can't prove it isn't subsidized"；成本类结论依赖厂商定价不变。
9. **RLCD 的技术细节**：官方未披露奖励/评分规则与校准度量方式；HN 上有工程师给出"负奖励偏斜分布"的通用做法描述，属个人解读。【未证实】
10. **生产可靠性**：无独立 p95/p99、并发、重试率、可用性数据；早期访问阶段的实测样本都很小（777 条判断 / 18,075 次护栏调用）。
11. **第三方评测的边界**：ayautomate 是自测 791 条标注决策（真实测量）；dev.to 自陈"均为 TypeSafe 自报、未经复现"；opentweet 是聚合型站点（自称读取官方 models 页与 API 参考，2026-09-18）。它们用于交叉验证"字段名、上限值、请求形状"这类结构性事实可靠，但其中的**基准数字属各自口径，不能与官方数字混用**。（ayautomate、<https://dev.to/valyuai/how-to-use-jev-a-practical-guide-to-typesafes-system-one-model-g5e>、<https://opentweet.io/jev/limits>）【未证实（聚合/口径不明）】
12. **"置信度能排序难易"的证据来自 jev-1.12**（2026-08-12 的 10-K cookbook），未在 `jev-1.13.0` 上复现；跨版本外推需谨慎。

---

## 5. 平台、定价与限额

**要点**

1. 公开的模型线只有一条：`jev-1.13.0`，别名 `jev-latest` 与 `jev-preview` **当前都指向它**；官方建议调好阈值的场景固定版本化 ID，因为别名会静默漂移。【官方】
2. 唯一公开单价：**输入 $42 / Btok（$0.042 / MTok），输出免费**；计费单位是 **credits**，按 Input 消耗，购买型 credits 有到期日、不可退不可转，余额为 0 时可能被拒绝作答。【官方】
3. **限流 250,000 tokens/秒 与 1,200 requests/分钟**，官方明说**会动态调整、可无通知变更**——只能当上界参考，不能当 SLA。【官方】
4. **上下文 64k tokens/请求（state + 全部问题），其中 state + 最长单个问题 ≤32k**；第三方渠道普遍只标 32K，两者是**粒度差异不是矛盾**。【官方】+【第三方】（集成方官方文档）
5. 服务托管在**美国**，**无区域化部署与数据驻留选项**，无可用国家清单；**单一 provider、无跨 provider failover**；状态页 99.840%（窗口未标明）。【官方】+【第三方】
6. 接入渠道已有 6+ 条（原生、OpenRouter、Cloudflare、Vercel、Netlify、Requesty、LiteLLM），但**渠道间不是同一套契约与计费口径**，不可混用比较。【第三方】（集成方官方文档）

### 5.1 模型版本与别名

- 公开的模型线只有一条：版本化 ID 为 **`jev-1.13.0`**（模型名 Jev 1.13），别名 **`jev-latest`** 与 **`jev-preview` 当前都指向 `jev-1.13.0`**。【官方】<https://docs.typesafe.ai/models>
- 官方 Warning 明确说："`jev-preview` 目前与 `jev-latest` 相同，**现在没有 preview 版本**"。【官方】/models
- 官方建议：**如果把 confidence 阈值调校到某个版本，应钉住版本化 ID 而不是别名**；`jev-latest` 是 SDK 默认值，会随新版本漂移。【官方】/models
- 请求里可以写别名，但**响应体里的 `model` 字段回报的是版本化 ID**（官方 Quickstart 与 API 示例响应均为 `"model": "jev-1.13.0"`）。【官方】/api、<https://docs.typesafe.ai/introduction/quickstart>
- 官方 cookbook 的日志也印证了别名解析：`TypeSafe requested model: jev-latest` / `TypeSafe returned models (calls): {'jev-1.13.0': 15}`。【官方】<https://docs.typesafe.ai/cookbooks/consistency_choice_cookbook>
- 上一版本 **`jev-1.12` 存在且价格与 1.13 相同**（cookbook 写明 `$0.042 / $0.00`，"output tokens are free"），但官方**没有**公开的 version changelog、发布日期或迁移说明。【官方】<https://docs.typesafe.ai/cookbooks/sde_cascade>

各渠道登记的模型名（同一底层模型，名字与版本语义不同）：

| 渠道 | 模型标识 | 说明与来源 |
| --- | --- | --- |
| 原生 API | `jev-1.13.0` / `jev-latest` / `jev-preview` | `GET /v1/models` 当前只列别名 + description + release_date；版本化 ID 即使不在列表里也被接受。【官方】/models |
| OpenRouter | `typesafe/jev-1.13`、`~typesafe/jev-latest` | 后者是"always redirects to the latest model in the Jev family"的别名条目。【第三方】（集成方官方文档）<https://openrouter.ai/typesafe>、<https://openrouter.ai/~typesafe/jev-latest> |
| Cloudflare Workers AI | `typesafe/jev` | 分类为 Text Generation / Third-party；示例响应仍回报 `"model": "jev-1.13.0"`。【第三方】（集成方官方文档）<https://developers.cloudflare.com/ai/models/typesafe/jev/> |
| Vercel AI Gateway | `typesafe-ai/jev` | 经 AI SDK 7 的 `experimental_evaluate` 调用（需 AI SDK 7.0.105+）。【第三方】（集成方官方文档）<https://vercel.com/changelog/typesafe-ai-jev-now-available-on-ai-gateway> |
| Netlify AI Gateway | 未给出独立 ID | 装 `@typesafe-ai/sdk` 直接用，无需自建 API key。【第三方】（集成方官方文档）<https://www.netlify.com/changelog/typesafe-jev-ai-gateway/> |
| LiteLLM 透传 | registry 登记 `typesafe/jev-1.13.0`、`typesafe/jev-latest`、`typesafe/jev-preview` | 任意 `/typesafe/` 路径被转发（如 `/typesafe/v1/systemone`）；**日志按响应上报的版本化模型记账**。【第三方】（集成方官方文档）<https://docs.litellm.ai/docs/pass_through/typesafe> |
| Requesty | `typesafe/jev-1.13.0`、`typesafe/jev-latest` | 标注集成 "experimental，可能无弃用期变更"。【第三方】<https://www.requesty.ai/blog/typesafe-jev-explained> |

- 发布与访问状态：官方博客称 "today in early access … opening early access and bringing developers off the waitlist"（**early access 中，正在放量**）。【官方】<https://typesafe.ai/blog/introducing-system-one-models-and-jev>
- **别名漂移是复现性风险的具体证据**：LiteLLM 先解析到 `jev-1.13.0` 再固定，并明说未来解析可能得到不同模型（<https://docs.litellm.ai/blog/jev-auto-router-benchmark>）；LangChain 的实验元数据里**根本没有 Jev 版本**；Jevals 指出 Gateway 不报告版本。【第三方】

### 5.2 定价与计费

- 唯一公开单价：**输入 $42 / 十亿 tokens（= $0.042 / 百万 tokens）**，**输出 tokens 免费**——官方原文 "Charged per input token. Output tokens are free."。【官方】/models、官方发布博客
- 计费机制是 **credits（信用点）**，来自官方主客户协议 MCA §8.2：credits "consumed by each **Input** submitted"（按输入消耗）。【官方】<https://typesafe.ai/legal/mca>
- credits 分两类：
  - **Purchased Credits**（客户购买；除非订单另有约定，**到期日为 Term 结束或购买后 12 个月，取先到者**；可 opt-in 自动充值；**余额为 0 时可能被拒绝生成答案**）；
  - **Promotional Credits**（**TypeSafe 全权决定是否发放的免费点数**，可附带到期/撤销条件，**先于购买点数消耗**）。【官方】<https://typesafe.ai/legal/mca>
- credits "not redeemable, refundable, transferable"；明文**禁止开多个账号重复获取促销 credits**；美元结算，除非另有约定发票后 30 天到期，逾期月息 1.5%。【官方】<https://typesafe.ai/legal/mca>
- **免费额度 / 免费层 / 试用时长 / 最低充值：未找到公开信息。** `https://typesafe.ai/pricing` 返回 **404**（无独立定价页），价格只出现在文档 Models 页与博客；唯一可能的"免费"来源是 MCA 里酌情发放的 Promotional Credits。

**输出免费的第三方实证（一次）**：钓鱼邮件基准作者核对两遍共消耗 **4,561,792 token（3.66M 输入、0.90M 输出）被计费 $0.15**，与"仅输入计费"一致。【第三方】<https://github.com/anisselbd/jev-phishing-bench>

**各渠道计费差异**：

| 渠道 | 价格呈现 | 备注（来源） |
| --- | --- | --- |
| 原生 API / OpenRouter | `$0.042` 输入 / `Free` 输出（OpenRouter 条目写法 "$0.042 / Free"） | 【第三方】（集成方官方文档）<https://openrouter.ai/typesafe/jev-1.13> |
| Cloudflare Workers AI | 页面**不列单价**，"View pricing in the Cloudflare dashboard" | 需在 Cloudflare 控制台查看。【第三方】（集成方官方文档）<https://developers.cloudflare.com/ai/models/typesafe/jev/> |
| Vercel AI Gateway | 模型页价格字段显示 **"Price: Free"**（原文如此，**未说明原因**） | ⚠️ 不可解读为"免费使用"；同页要求遵守 TypeSafe 条款，且 changelog 说调用 "count toward budgets"。【第三方】（集成方官方文档）<https://vercel.com/ai-gateway/models/jev>、<https://vercel.com/changelog/typesafe-ai-jev-now-available-on-ai-gateway> |
| Netlify AI Gateway | 用量 **billed to your Netlify credits** | 由 Netlify 计费。【第三方】（集成方官方文档）<https://www.netlify.com/changelog/typesafe-jev-ai-gateway/> |

**成本量级参考（口径各不相同，勿混用）**

- 【官方】发布博客的 Doom demo（工程师担心 10 queries/秒）："which ends up costing **~$7/hour**"。<https://typesafe.ai/blog/introducing-system-one-models-and-jev>
- 【第三方】Requesty 的换算（假设 10 万次请求 × 1,000 输入 token = 1 亿输入 token）：**$4.20**，核价日期 2026-09-19。<https://www.requesty.ai/blog/typesafe-jev-explained>
- 【官方】官方 cookbook 里的成本表**自带免责声明**："They are **not verified `jev-latest` prices or current billing amounts**." <https://docs.typesafe.ai/cookbooks/consistency_choice_cookbook>
- 【第三方】实测成本（各为不同任务口径）：8 路意图分类 **$0.0151 / 1,000 decisions**（提示词平均 ~360 输入 tokens）；77 路 **$0.0400 / 1,000**（平均 952 tokens/prompt）；同 8 路消息对比 LLM 为 GPT-5.6 Terra $0.609/1k、Claude Haiku 4.5 $0.356/1k、Gemini 3.5 Flash-Lite $0.087/1k、GPT-5.4 nano $0.070/1k（经 OpenRouter 计费）。<https://www.ayautomate.com/blog/how-to-use-jev-guide>【第三方实测】
- 【未证实】重排示例（官方 cookbook 转述）：40 条法律查询 × 30 个候选，共 **1,200 次调用花费 $0.0645**，正确段落进入 top-10 的比例从 38% 提到 62%。（转述自 ayautomate；**该数据在官方页面直接核到的版本是 §7.2 的 CLERC 表**）

### 5.3 限流与配额

- 官方口径：**250,000 tokens/second** 与 **1,200 requests/minute**；超过任一返回 **`429 Too Many Requests`**，服务过载返回 **`529 Overloaded`**。【官方】/models、/api
- 官方 Warning 原文要点：**"Rate limits are adjusting dynamically"**，因为"正在服务非常大的需求"，**限额可能无通知变动**；**更高限额可通过 custom / enterprise 计划获得**（sales@typesafe.ai）。【官方】/models
- 因此这两个数字**只能当作上界参考，不能当作 SLA**。【官方口径 + 本片判断，来源同上】
- SDK 默认自动重试并遵守 `retry-after`；Python `RetryPolicy` 参数包括 `max_retries`（默认示例 3）、`backoff_initial`/`backoff_max`（逐次翻倍）、`backoff_jitter`、`http_statuses`（示例 `{429,500,502,503,504}`）、`respect_retry_after`（遵守 `Retry-After` / `retry-after-ms`）、`timeout`（作为**整次调用的重试总预算**）、自定义 `predicate`。【官方】<https://docs.typesafe.ai/sdk/python/api/retries>

可引用的配置片段（官方 Python SDK）：

```python
from typesafe_sdk import RetryPolicy, TypeSafeClient

client = TypeSafeClient(
    retry=RetryPolicy(
        max_retries=3, timeout=10.0, http_statuses={429, 500, 502, 503, 504}
    )
)
```

- 官方 cookbook 出现的并发经验值：`MAX_WORKERS = 6`、注释 "the public endpoint rate-limits above roughly eight"；另一处 cookbook 用 `WORKERS = 8` 并注释 "small pool: enough to keep a live run to minutes, gentle on rate limits"。（<https://docs.typesafe.ai/cookbooks/entity_alignment>）【官方】

### 5.4 上下文与输入限制

- **64k tokens / 请求**（`state` + 全部问题合计）；**`state` + 最长单个问题 ≤ 32k**。官方解释：`state` 只摄取一次，所有问题并行评估。【官方】/models
- **输入仅文本**：string、JSON object 或文本数组；**无 image/audio/video**，非文本需先转成文本或结构化字段。【官方】/models、<https://docs.typesafe.ai/concepts/state>
- 结构上限：**Choice 最多 255 个选项**；**Score 至少 2 档、API 最多接受 10 档**。【官方】/api
- 官方明确警告**上下文越长精度越差**（"context rot"），建议先在代码里过滤，而不是把长上下文直接塞进去。【官方】jaggedness 页
- 第三方渠道口径**一致只写 32K**：OpenRouter 标 32K、Cloudflare 标 32,000 tokens、Netlify 写"约 32,000 tokens ≈ 15 万英文字符"。【第三方】（集成方官方文档）<https://openrouter.ai/typesafe/jev-1.13>、<https://developers.cloudflare.com/ai/models/typesafe/jev/>、<https://www.netlify.com/changelog/typesafe-jev-ai-gateway/>

> **口径辨析**：官方 64k 与第三方 32K **不是矛盾，而是不同粒度**——32K 对应的是"`state` + 最长问题"这一更紧的约束。引用时须写清是哪一层。（同样的辨析见 §10.4 的跨文件核实条目）

### 5.5 可用性、状态页与地区

- 官方状态页（Better Stack，`status.typesafe.ai`）只有一个组件：**`api.typesafe.ai` — TypeSafe API Availability：99.840% uptime**；页面提供 30/60/90 天窗口切换。【官方】<https://status.typesafe.ai/>
- 抓取时页面顶部 "Last updated on Sep 22, 2026 at 2:54am UTC"，当时 "All services are online"；近期事件包括 `Sep 21, 2026 — API issues — Resolved`（原文 "intermittent downtime and system instability"）与 `Sep 20, 2026 — Console is unavailable. — Resolved — Sep 21, 2026 at 8:16am UTC`；历史上 6 月下旬—8 月初有多次 2–59 分钟短时 downtime。【官方】<https://status.typesafe.ai/>
- 第三方可用性实测（OpenRouter 侧）：Jev 1.13 **只有 1 个 provider**，**Uptime (3d) 100.00%**、**Availability (3d) 99.87%**、**Availability (24h) 99.70%**；页面提示该模型无第二 provider 可 failover（"OpenRouter forwards every request to it directly"）。【第三方】<https://openrouter.ai/typesafe/jev-1.13>
- 流量侧参考：OpenRouter 作者页标注 TypeSafe 模型"**Tokens processed in the last 7 days：778B tokens**"；流量最大的公开应用包括 `sel-jev-rubric-judge`（9.18B tokens）、`mirasim`（2.9B）、`Waura`（2.65B）。【第三方】<https://openrouter.ai/typesafe>
- 地区与数据驻留：隐私政策写 "**The Services are hosted in the United States (U.S.)**"，来自 EEA/UK 的使用意味着数据传至美国存储与处理。【官方】<https://typesafe.ai/legal/privacy-policy>
- MCA §16.12（Export）：客户须遵守美国及他国出口/进口法；**不得位于（或为）受美国禁运或被列为"支持恐怖主义国家"的国家/国民**，不得提交受 ITAR 管制的信息。【官方】<https://typesafe.ai/legal/mca>
- 官方博客说明速度评测通常是 "from our laptops on the West Coast (this is where our service is currently based)"。【官方】官方发布博客
- 数据处理：**不拿客户 Input 训练模型**；企业客户可申请 **zero data retention (ZDR)**。【官方】<https://typesafe.ai/legal/privacy-policy>、<https://docs.typesafe.ai/legal>（Vercel 侧可按请求传 `zeroDataRetention: true`：【第三方】（集成方官方文档）<https://vercel.com/changelog/typesafe-ai-jev-now-available-on-ai-gateway>）；条款细节与落差见 §11.3。

### 5.6 分发渠道与控制台能力

- **已有 6+ 条接入路径**：原生 HTTP/SDK、OpenRouter（Decisions API）、Cloudflare Workers AI（`typesafe/jev`）、Vercel AI Gateway（`typesafe-ai/jev`）、Netlify AI Gateway、Requesty（`typesafe/jev-*`）、LiteLLM 透传（`/typesafe/v1/systemone`）。
- **OpenRouter 的关键差异**：模型条目注明它走的是 **OpenRouter Decisions API 而不是 OpenAI 兼容 chat endpoint**——"chat completions SDKs will not work with it"；且只有 TypeSafe 一个 provider，OpenRouter 不做路由。【第三方】（集成方官方文档）<https://openrouter.ai/typesafe/jev-1.13>
- **Vercel**：经 AI SDK 7 的 `experimental_evaluate` 调用；Choice/Score 的 confidence 出现在 `result.providerMetadata.typesafe.confidence`。【第三方】（集成方官方文档）<https://vercel.com/changelog/typesafe-ai-jev-now-available-on-ai-gateway>
- **Cloudflare Workers AI**：REST 形式为 `POST https://api.cloudflare.com/client/v4/accounts/$CLOUDFLARE_ACCOUNT_ID/ai/run`，body 为 `{"model":"typesafe/jev","input":{...}}`。【第三方】（集成方官方文档）<https://developers.cloudflare.com/ai/models/typesafe/jev/>
- **Requesty**：标注集成 "experimental，可能无弃用期变更"，仅支持 text-only user message、**非流式**、`questions` 响应格式。【第三方】<https://www.requesty.ai/blog/typesafe-jev-explained>
- **控制台**：入口 `console.typesafe.ai`；未登录访问 `/keys` 会 307 跳转到 `/login?returnTo=/keys`；登录只有 **Continue with Google** 或 **邮箱验证码（Email me a code）** 两种方式，需同意 terms of use 与 privacy policy。【官方，实测】<https://console.typesafe.ai/keys>、<https://console.typesafe.ai/login>
- **API key**：官方 Quickstart 指引到 https://console.typesafe.ai/keys 创建；SDK 从环境变量 `TYPESAFE_API_KEY` 读取。【官方】<https://docs.typesafe.ai/introduction/quickstart>
- **Playground**：官方描述为"打开 Playground 并登录 → 粘贴任意文本作为 state → 添加问题（Noul / Choice / Score 可混用）"，**支持生成分享链接**（官方 cookbook 用 `make_playground_link(...)` 生成 `console.typesafe.ai/playground#share/<base64>` 形式的 deeplink）。【官方】<https://docs.typesafe.ai/introduction/quickstart>、parallel cookbook
- 官方还提供 **agent skill**（Claude Code/Codex 等），并提示可"创建 API key 并允许 agent 跑便宜的测试查询来自己摸索最佳用法"；控制台内还有示例库 `console.typesafe.ai/docs/cookbooks`。【官方】<https://docs.typesafe.ai/agent-skill>

### 5.7 本节不确定项

1. `jev-1.13.0` 的官方发布日期未在官方页面直接给出（OpenRouter 侧写 2026-09-18）；官方无 changelog / 弃用策略，**是否存在 1.13 之外的中间版本未知**。
2. **第三方（datacamp、apidog）称原生 API 仍需 waitlist/邀请，但官方页面上找不到对应文字**，官方只有"early access"表述；当前是否仍需 waitlist 未能核实。【未证实】<https://www.datacamp.com/blog/system-one-models-jev>、<https://apidog.com/blog/jev-api-key/>（另注：DataCamp 页面本次返回 403，未纳入证据）
3. **credits 与 token 的换算比例、最低充值额、是否按组织计费、发票/税、超量行为均未公开**；控制台内账单/用量页需登录，上游未登录、未核实。
4. **Vercel 模型页 "Price: Free" 含义不明**（可能只是价格字段未填，或指输出免费），不可据此认为该渠道免费。
5. 输出免费目前**只有一次、单一来源**的第三方实证（4.56M token 计费 $0.15），且未核对提供方发票。
6. 无免费层数字，也就**无法判断"低成本"在试用/评估阶段的可达性**——上游明确写"未找到公开信息"。
7. **分层限流表（free/pro/enterprise 各档具体数值）、账号级配额、tokens/秒 的第三方实测吞吐曲线：未找到公开信息。** OpenRouter 模型页给出延迟与 uptime，但**没有**吞吐（tokens/s）实测值（<https://openrouter.ai/typesafe/jev-1.13>）。
8. 官方明确"动态调整、可无通知变更"，**限流不可预测**；任何本地令牌桶都只能是软上界。
9. 超限后的实际退避行为、配额耗尽的计费后果（MCA 只写 "may decline to generate Output"）无公开细节。
10. **网关侧是否另有更小限制，未找到说明**；第三方渠道可能只暴露较小预算。超限行为的具体报错码，以及 64k 与 32k 两个约束是否同时硬校验，官方未逐条说明。"context rot" 只有定性描述，**没有精度随长度衰减的公开曲线**。
11. **99.840% 的统计窗口长度未标明**：窗口由页面控件（30/60/90 天）决定，抓取到的静态文本未写默认窗口，引用需谨慎。
12. **没有可用国家/地区清单**，也没有区域化部署或数据驻留选项（只有"企业可谈 ZDR"）；**是否对中国大陆等地区有访问限制未找到公开信息**（上游只做文档检索，未做连通性测试）。
13. **单 provider 依赖是结构性事实**：原生 API 组件只有 `api.typesafe.ai`，OpenRouter 侧确认无跨 provider failover。（OpenRouter 是否上架在片段 D 内部有两份文件结论冲突，详见 §11.4）
14. 官方 99.840% 与 OpenRouter 99.87%/99.70% 是**不同提供方、不同窗口、不同统计口径**，不可直接比较。
15. **控制台内的用量/账单页、组织与团队管理、Playground 的导出/评测功能——均需登录，上游未登录、无法核实**，不写成事实。
16. OpenRouter 的页面代码块为前端渲染，**未能取得可复制的原始请求体**（上游据此不杜撰代码）。
17. 各网关是否支持流式、批量、并发上限等**渠道级能力差异，未找到系统对照**；已知 Requesty 非流式、OpenRouter 不支持 chat completions 形状。

---

## 6. 性能与独立评测

**要点**

1. 引用任何 Jev 性能数字前先分三档：**可引用（附条件）**、**需标注（口径限定）**、**不可引用（无方法学或不可复算）**——分档依据是证据类型，不是数字大小。【本报告 §6.5】
2. **可引用**的核心是：单价、限流上界、上下文上限、分类环节的 p50 与成本差、各独立基准的排名——**每一条都必须连条件一起引用**。
3. **需标注**的关键两条是官方 **67.8%** 与 **0% 幻觉**：前者是与模型标签的吻合度，后者官方自认 "not empirical"。
4. **不可引用**的是 **193.6x/444.6x**（无法从公开圆整数字复算，复算得 75.1x / 171.4x）、**"45% faster"**、**Vercel "~5–18x"** 与转述的 YouTube "25×"。
5. **官方自评的最大结构性问题是 harness bias**：工作流由自家能力团队制作、参考标签是 GPT-6 Astra 与 Claude Fable 5.1 的平均（非人工真值）、LLM 基线被套上官方自家的 "System One LLM wrapper"（官方自述更慢更贵）。
6. **最重要的一条评测发现与厂商立场无关**：官方数据显示"同一策略拆成 workflow（原子问题 + 代码决策）后，**每个被测量的模型都更准、更便宜、更快**"——这对 Agent Harness 的设计有直接价值。【官方】
7. 第三方证据的整体图景是「**成本/延迟/稳定性占优，准确率不占优**」；没有任何单份第三方实验同时覆盖准确率 + 校准 + 门槛稳定性 + 组合后的工作流。

### 6.1 口径辨析框架：三类证据

- **【官方】官方自测** = TypeSafe 自家评测站与发布博客（`evals.typesafe.ai`、launch 博客）。其特征是：**参考标签由两个前沿模型的回答平均生成**，官方明说"不辩论 harness 与标签的正确性"，工作流由自家能力团队制作。
- **【第三方】第三方受控实验** = 有署名、给出样本量/并发/重复次数/统计口径、并公开原始数据或脚本的实测（LiteLLM、LangChain、Jevals、JevBench、钓鱼基准、Every、Jagent）。这类证据**可以引用，但必须连条件一起引用**。
- **【未证实】不可引用数字** = 无方法学、无法定位出处或无法从公开圆整数字复算的数字。
- 引用一条性能数字时至少要能回答四个问题：**谁测的（利益关系）、测的哪一段（分类/端到端/含网络）、什么并发与负载、参照物是什么模型什么设置**。

| 来源 | 性质 | 样本与条件 | 核心数字 | 关键局限 |
| --- | --- | --- | --- | --- |
| [evals.typesafe.ai](https://evals.typesafe.ai/) | 官方自评 | 4 个工作流，共 711 例 | 67.8% / $0.0004 / 0.4s（聚合） | 标签=两模型平均；作者自家出题；"假设 harness 正确" |
| [typesafe.ai 发布博客](https://typesafe.ai/blog/introducing-system-one-models-and-jev) | 官方公告 | 演示 + 同一组 workflow evals | 193.6x faster / 444.6x cheaper；70–500 ms；$0.042/MTok | 官方自我标注大量 nuance；速度测试跑在自家西海岸笔记本 |
| [LiteLLM](https://docs.litellm.ai/blog/jev-auto-router-benchmark) | 第三方受控 | 80 条自编案例 × 3 次 = 每分类器 240 次；并发 1；2026-09-18 | 5.43x（p50）、3.88x（p95）、成本 −96.118% | 标签作者自定无独立复核；不测下游质量；不含完成模型成本 |
| [LangChain](https://www.langchain.com/blog/jev-agent-evals-langsmith) | 第三方受控（合作方） | 5 条天气请求 × 100 次 = 500 次判定；人工 oracle | 500/500 与 oracle 一致；方差低 92–913x；$0.34 vs Claude $28.17 | 样本极小、任务单一；作者自称"观察性"证据 |
| [Jevals](https://jevals.com/) | 第三方独立 | 3 任务 × 300 问 × 5 次 = 31,500 次判定；人工标签 | 见 §6.4 | 每类问题只用一个公开数据集；无前沿模型行（预算） |
| [JevBench v1.2](https://github.com/fstandhartinger/jevbench/blob/main/RESULTS-v1.2.md) | 第三方独立 | 534 条冻结用例，48 个系统 | Jev 74.4 分第一，领先第二名 1.3 分 | 部分自托管延迟按假设 ×2 调整；成本多为估算 |
| [jev-phishing-bench](https://github.com/anisselbd/jev-phishing-bench) | 第三方独立 | PhishNChips v5.2，2000 封邮件，无并发 | 62.6% vs Haiku 81.3%；ECE 0.154 | 邮件正文为合成；真值来自 URL 信誉库 |
| [Every / Mike Taylor](https://every.to/also-true-for-humans/mini-vibe-check-typesafe-s-jev-judged-everything-i-ve-written-in-0-7-seconds) | 第三方实测 | 37 篇文档 × 21 问 = 777 判断 | <0.7s、约 1/4 美分；缺陷检出 6/7 vs 7/7 | 作者自认需更完整准确率检查；样本小 |
| [Vercel fx](https://vercel.com/changelog/typesafe-ai-jev-now-available-on-ai-gateway) | 第三方（厂商自测） | **未公布数据集与案例数** | "~5–18x faster and more accurate" | 无公开方法学 |
| [jev-agent.com（Jagent）](https://jev-agent.com/benchmarks) | 第三方（有商业利益，卖 credits） | 8 fixtures × 5 次 = 每模型 40 次 | Jev 中位 352 ms；服务器时间 1 问 70ms / 5 问 74ms | 自曝两次差点把有利于 Jev 的假象写进结论 |

> 表中"第三方"并非质量等价：Every、LangChain、Vercel、Jagent **都有潜在利益关系或未披露关系**（Every：TypeSafe 曾在 AIE 预发布；LangChain：与 TypeSafe 联合直播；Vercel：把 Jev 上架自家 Gateway；Jagent：卖 Jev credits），引用时必须保留标注。

### 6.2 官方自评：数字与口径

- 官方评测站 `evals.typesafe.ai`：4 个工作流（Security Incidents、Agent Trace Observability、Invoice Processing、Customer Service），共 **711 例**，聚合 **67.8% / $0.0004 每例 / 0.4s**。【官方】
- **口径关键**：**参考标签由 GPT-6 Astra 与 Claude Fable 5.1 的答案平均生成，两者都开 high thinking**；"All other models are evaluated using the provider's default reasoning settings"；官方明说 **"Instead of debating the correctness of the harness and labels, we assume that the code is correct"**。【官方】
- 官方自述的"工作流化"结论（对 harness 设计比 Jev 本身更重要）：**"Averaged across the four example tasks, every model is more accurate, cheaper and faster in the workflow than it is with the same policy as a prompt."**；上游同时记录到"同样的策略拆成工作流后，连 LLM 都变准了（Luna 51.9% → 66.8%）"。【官方】
- 官方对"零幻觉"的写法自我打折：**"Our number is not empirical. Schema matching is guaranteed, thus we can confidently add 0% into the plots."**。【官方】
- 官方对定价可持续性：**"We can't prove it isn't subsidized; we'll need the long-term to prove the sustainability of our pricing."**。【官方】
- 官方对速度：**"our published evals are generally run from our laptops on the West Coast"**（服务当前所在地）；对 193.6x/444.6x 的自评：**"We expect that these are on the higher end of real world gains."**；对演示查询的自我打折："The relatively shorter input **paints our model in an advantageous light**."。【官方】

逐工作流数字（**第三方转述的 dashboard 圆整显示值**，两处独立来源一致）：

| 工作流 | n | Jev 准确率 / 每例成本 / 延迟 | 该工作流最强对比 | 差距 |
| --- | --- | --- | --- | --- |
| Security incidents | 240 | 61.7% / $0.0001 / 0.3s | Claude Opus 5：66.2% / $0.0574 / 15.1s | −4.5 |
| Agent trace observability | 117 | 71.6% / $0.0003 / 0.5s | GPT-5.6 Sol：76.6% / $0.0575 / 40.3s | −5.0 |
| Invoice processing | 150 | 61.8% / $0.0011 / 0.5s | GPT-5.6 Sol：79.1% / $0.2152 / 34.3s | **−17.3** |
| Customer service | 204 | 76.0% / $0.0001 / 0.4s | GPT-5.6 Sol：78.3% / $0.0323 / 10.1s | −2.3 |
| **聚合** | **711** | **67.8% / $0.0004 / 0.4s** | GPT-5.6 Sol：74.1% / $0.0836 / 23.3s | −6.3 |

来源：【第三方】<https://kingy.ai/blog/typesafe-jev-review-the-ai-model-that-doesnt-generate-text/>、<https://dev.to/gabrielanhaia/jev-beat-gpt-luna-by-1-point-gpt-6-and-claude-wrote-the-answer-key-314k>（官方页面本身给出同样量级的圆整值；样本量 N 的核实状态见 §6.8 不确定项）

聚合榜其他行（dev.to 转述官方页面，dev.to 称该页共评分 9 个模型）：GPT-5.6 Terra 67.9%（$0.0304）/ Claude Sonnet 5 67.8%（$0.1174）/ GPT-5.6 Luna 66.8%（$0.0033）/ Claude Opus 5 73.1%（$0.1761）。【第三方】<https://dev.to/gabrielanhaia/jev-beat-gpt-luna-by-1-point-gpt-6-and-claude-wrote-the-answer-key-314k>

官方评测站同表另列（来自片段 A 的记录）：sonnet 5 67.8%、luna 66.8%、DS v4 pro 65.5%、DS v4 flash 64.4%、haiku 4.5 53.6%（workflow 路线）。另外 haiku 4.5 在 prompt 模式仅 **18.1%**、luna 51.9%、terra 61.6%、sonnet 5 60.4%、opus 5 64.8%、sol 63.4%——**workflow 模式对每个模型都优于 prompt 模式**。【官方】<https://evals.typesafe.ai/>

> 口径提醒：**"67.8% vs 67.9%"这种"打平"是与"两个模型平均标签"的打平**，不是与真值打平（见 §6.6）。

### 6.3 193.6x / 444.6x 无法从公开圆整数字复算（归入不可引用档）

- flowtivity 的审计引用 TypeSafe 官网每工作流数字：**$0.000081 vs $0.013880**、**0.114 秒 vs 8.566 秒**。【第三方】<https://flowtivity.ai/blog/jev-typesafe-ai-decision-model/>
- 复算：8.566 / 0.114 = **75.1x**（≠ 193.6x）；0.013880 / 0.000081 = **171.4x**（≠ 444.6x）。
- kingy.ai 独立指出同一问题："The public homepage headline—193.6% faster and 444.6% cheaper—**cannot be reconstructed from the rounded table**, so quote those numbers as TypeSafe's workload-specific claims rather than universal multipliers."。【第三方】<https://kingy.ai/blog/typesafe-jev-review-the-ai-model-that-doesnt-generate-text/>
- 生态方转述该倍数的口径互不统一，均为转述 TypeSafe 自测、非独立测量：LangChain 写 "up to **200x faster inference and 400x lower cost**"（<https://www.langchain.com/blog/building-a-harness-with-jev>）；LangChain 另一篇写 "up to ~450x cheaper and ~200x faster"（<https://www.langchain.com/blog/jev-is-now-available-in-langsmith-evals>）；Langfuse 写 "20 to 200x faster and 40 to 400x cheaper"（<https://langfuse.com/blog/2026-09-18-using-typesafes-jev-for-evals>）；Vercel 写 "up to 194 times faster and 445 times cheaper"（<https://vercel.com/blog/ai-gateway-jev-model-launch>）。【未证实（转述，口径不一）】
- **不得作为架构前提**：可用的替代是"单次分类约 3e-5 美元""单次判定毫秒到百毫秒级""同任务下比 Haiku 快数倍"这类连条件可引用的量级（见 §6.5）。

### 6.4 第三方受控实验：逐份要点

- **LiteLLM Auto Router 基准**（最完整的一份，2026-09-18）：80 条自编案例 × 3 次 = 每分类器 240 次调用，并发 1，deadline 10,000 ms，**零重试、关闭熔断**，seed 20260918，`jev-latest` 先解析到 `jev-1.13.0` 后固定；上下文 3 轮 / 8000 字 / 每轮 4000 字 / 含 assistant 轮，关闭缓存。Jev tier 一致率 **95.00%**（228/240）vs Claude Haiku 4.5 **73.75%**（177/240）；**p50 126.81 ms vs 688.40 ms（5.43x）**；p95 231.16 ms vs 896.94 ms（3.88x）；成本 **$0.007706664 vs $0.198534（−96.118%）**；两个分类器互相一致 189/240 = 78.75%。【第三方】<https://docs.litellm.ai/blog/jev-auto-router-benchmark>
  - **5.43x 的定义**是 `Haiku_p50 / JEV_p50 = 688.395634 / 126.814470`，即**聚合统计量之比，不是逐请求比值的平均**；同口径均值 = 5.21x；95% 聚类自助置信区间：p50 **5.20x–5.71x**、p95 **2.94–4.78**、成本节省 **95.973–96.266%**、期望 tier 匹配率差 **21.25 个百分点（12.92–30.42）**。【第三方】
  - **成本口径极关键**：只算**分类器**成本，**不含完成模型、嵌入、预热与评估裁判**；用观测 token × 归档注册表价格，未核对提供方账单/税/折扣/套餐权益。原文："**Cost savings stop at the classifier boundary.**"。【第三方】
  - 作者主动声明的局限（可直接引用）：「标签与 prompt 出自同一作者、无独立标注或盲审」、「不建立通用分类准确率，也不建立最终答案质量」、「分类器一致率不关心是否匹配作者标签」、「未来 `jev-latest` 可能解析到不同模型，需要重做实验」；JEV 在长用例上匹配率更低（87.50% vs 短用例 97.50%）。【第三方】
- **LangChain（Jev 作为 agent 评估裁判）**：5 条天气请求 × 100 次重复 = **500 次判定**，人工 rubric 标签作 oracle；`does_pass` 与 oracle 一致 **Jev 500/500（100%）**，Terra 99.8%、Luna 96.4%、Claude 80.0%；分数方差（越低越好）**Jev 0.0000149**，Terra 高 **913x**、Claude 高 92x、Luna 高 433x；Jev 平均 **0.44 s / $0.00035 每次**，本次合计 **$0.34** vs Claude **$28.17**。【第三方】<https://www.langchain.com/blog/jev-agent-evals-langsmith>
  - 作者的限制声明："This experiment **cannot tell us why**… the result is **observational**…"、"**low cost can amplify mistakes** - a consistently wrong evaluator can produce bad feedback at scale."；复现细节：Jev 经 `langchain-typesafe==0.0.1a2` 访问，**"The Jev service version was not available in the experiment metadata"**，LLM 裁判**未设 temperature/top-p/seed/max tokens**。【第三方】
  - LangChain 官方点明这类分类器步骤的价值："这类分类器步骤以前一直锁在闭源 harness 内部，现在有了便宜且性能足够的分类模型，可以把同样的模式用到所有 agent 上"。【第三方（集成方）】
- **Jevals.com（人工标签独立基准）**：7 个模型、**31,500 次判断**（300 问 × 5 遍 × 3 任务），人工标签，"run from a laptop at UTC+3"，总花费 $15.07（另有 $3.98 废弃运行）；明示 "Jevals is independent. It is not affiliated with TypeSafe AI"。分数口径为 **Decision Score：100 = 完美，0 = 猜标签基率**（即相对随机基线，与厂商"吻合度"口径的最大差别）。【第三方】<https://jevals.com/>、<https://jevals.com/notes/2026-09-18/>
  - 三个任务：`noul`（PubMedQA 300 问）Jev **69.0** vs Gemini 3.8 Flash 73.0，配对差值 95% 区间 **−2.5 ~ +10.4 包含 0 → 统计并列**，Jev $0.03/千次 vs Gemini $0.80、p95 0.65s vs 5.5s（**1/28 价格**）；`choice`（Banking77）Jev **67.8**（准确率 79.7%）被 Gemini 3.8 Flash **显著领先**，与 GLM-5.3（66.8）并列第二；`score`（HelpSteer2 helpfulness 0–4）Jev **9.2** vs GLM-5.3 7.8，但结论是 **"no model clearly beats answering with the label base rates"**。【第三方】
  - 门槛实测（对 harness 直接有用）：给每个模型各自设定达到 **95% 准确率**的阈值后，`noul` 任务 Jev 可**独自承接 86%** 决策（Gemini 3.8 Flash 79%）；`choice` 任务 Jev **54%**（Gemini 45%、DeepSeek V4.1 Flash 39%、GLM-5.3 38%）；共享的 choice 门槛 0.96 下，Jev 承接 **59%** 且其中 **94.3%** 正确，而 LLM "在至多 15% 的样本上给出 ≥0.96"（原因之一是**偏好 0.95 这类圆整值**）。【第三方】
  - 交叉对照：其 Banking77 准确率 79.7% "matches the **80.3%** a Vercel engineer reported on a different **385-item sample**"（<https://github.com/vercel/ai/issues/20846>）；官方称 70–500 ms、文档称多数查询约 100 ms，**Vercel 实测中位数 132–272 ms**，而 Jevals 从 UTC+3 测得中位 **0.44–0.48 s**、p95 **0.65–0.69 s**（含跨洲网络，Jev 只在一个美国区域提供服务）。【第三方】
- **JevBench v1.2**：`Score = Intelligence above chance, Calibration, Speed, Cost — 25% each, geometric mean`，**534 条冻结用例**，48 个系统。**Jev 1.13.0 得 74.4 分第一**（Intelligence 85.7 / Calibration 82.7 / Speed 83.3 / Cost 52.0；$0.0399 每千次决策；p50 0.65 s；生产 API）；第二名 **SemIf (Qwen3.5-4B) 73.1**（自托管 RunPod）**仅差 1.3 分**。【第三方】<https://github.com/fstandhartinger/jevbench/blob/main/RESULTS-v1.2.md>
  - 对"Jev 更聪明"最不利的一行：**GPT-5.6 Luna (low) 排第 14，但 Intelligence 95.3、Calibration 89.8 都高于 Jev**，它输在 Speed(77.5) 与 Cost(28.5，$0.2419/千次)——即 Jev 的第一名**主要来自成本与速度轴**。【第三方】
- **钓鱼邮件基准 `jev-phishing-bench`**（2,000 封 PhishNChips v5.2，无并发，每封一次调用，Jev 一次请求问 9 个类型化问题；Haiku 4.5 native Messages、无 thinking、temperature 0）：【第三方】<https://github.com/anisselbd/jev-phishing-bench>

| 指标 | Jev (jev-1.13.0) | Claude Haiku 4.5 |
| --- | --- | --- |
| 准确率 | **62.6%** [60.5, 64.7] | **81.3%** [79.5, 82.9] |
| 钓鱼召回 | 43.2% | 76.4% |
| 假阳性率 | 18.0% | 13.8% |
| AUROC | 0.689 | 0.837 |
| **ECE（10 bins）** | **0.154** | **0.097** |
| 延迟 p50（从法国测） | 239 ms（网络地板 163 ms） | 687 ms（网络地板 18 ms） |
| 每千封成本（标价） | **$0.038** | $0.462 |
| 两次运行间概率稳定度 | 2.2% 标签翻转；平均绝对差 0.017，最大 0.15 | 0.7% 翻转；98% 相同，最大差 0.65 |

  - 作者在被质疑后追加的对照（17 Sep 追加，未修改原数字）：非 AI 基线——通用短链/免费主机/IPFS/文档托管域名清单 + 发件人与链接 eTLD+1 比对，**单靠清单规则就有 91.6% 准确率**（召回 83.5%、假阳性 0.2%），"The dataset largely separates by construction."；切分验证（B 半）：Jev 最佳单信号 89.4%，**低于两行正则的 91.8%**（p=0.0032）；Jev 5 信号回归 95.0%（AUROC 0.982）vs 正则 2 特征回归 91.8%（p=0.0018）；**把同样问题原样问 Haiku**：Haiku 最佳单信号 94.2% **高于** Jev 单信号（p<0.0001），Haiku 回归 93.2%（AUROC 0.991）与 Jev 95.0%（AUROC 0.982）**在准确率上无显著差异**（McNemar p=0.063）而 **AUROC 反而更高**；Jev 留存的优势是 **"about 27 times cheaper and 5 times faster than Haiku for signals of comparable quality, on a dataset that a regex already separates at 91.8%"**。【第三方】
  - 四种问法都跑了（防止事后择优）：verdict(choice) 62.6% / AUROC 0.689 / ECE 0.154；is_phishing(noul) 60.6% / 0.684 / 0.172；verdict_alt_click 63.2% / 0.700 / 0.115；verdict_alt_minimal 58.4% / 0.635 / 0.241——**换措辞最多 +0.6 点，最差 −4 点**。【第三方】
- **垃圾邮件评测 `bitnovus/jev-spam-eval`**：Jev 零样本在 **18,514 封**上 **98.33%** / AUC 0.9984，监督基线（约 14,800 条标注训练的 TF-IDF 逻辑回归）**98.39%** / 0.9986，**统计上打平**；2024–25 新钓鱼邮件 853 封 **Jev recall 95.31% vs 75.26%**；两者 50/50 平均后 **99.22%**（错误从 299 降到 144）；上下文富化把 Jev 从 93.62% → 97.98%、钓鱼 recall 85.71% → 98.43%（监督基线只涨 0.12pp）；学习曲线估计：达到 98.3% 约需 10,000 条标注（Ling-Spam 上约 200 条）；工程数据：19,772 次请求 / 3,206 万输入 token / 并发 16 下约 252 秒 / 约 $1.35。分档校准：打分 <0.1 的邮件中 0.1% 是垃圾；≥0.9 的邮件中 99.9% 是垃圾；0.5–0.6 区间只有 38% 是垃圾；把打分 0.3–0.7 的 4.6% 邮件转人工后，其余部分准确率 99.5%。【第三方】<https://github.com/bitnovus/jev-spam-eval>（其中分档校准数字在本轮调研中主要经 Arize 转述获得，见 §6.8）
- **NearHere（活动 listing 校验，业务方自测）**：主测试集 50 条，**Jev 96%（48/50）** vs Gemini 3.5 Flash-Lite 86% vs Mistral Small 4 84%；误拒有效活动 Jev **0/13**；平均响应 **0.59s**（Mistral 2.90s、Gemini 3.40s）；每 1,000 次决策 **$0.043**（Mistral $0.370、Gemini $2.496）。作者自陈：每模型单独调 prompt、开发集参与配置选择、期望标签由助手写、含客户端与网络开销、无重试，**不是独立盲测**，并声明 *This is a use-case study, not a general model ranking.*【第三方】<https://nearhere.events/blog/typesafe-jev-mistral-gemini-event-validation>
- **Every（媒体方实测）**：37 篇文档（27 篇作者本人的 Every 文章 + 10 篇刻意 AI 风格对照）× 21 个"AI 痕迹"问题 = **777 个判断，<0.7 秒，估计约 1/4 美分**；11 个实验合计 1,709 个判断、估计总成本 <1 美分。Dan Shipper 的 12 段合成文本对照：Jev 每段中位 **0.35 s**（Fable 5.1 高 effort 8.83 s，约 **25x** 快），估计成本约 **580x** 低，但 **Jev 只抓到 7 个缺陷中的 6 个，Fable 全抓到**。作者原话："**I'd want a more thorough accuracy check before putting it into production.** Even with that caveat, it's useful as an early warning system."（原文在付费墙内，数字经两处转述交叉核对：firecrawl、Kingy）【第三方】<https://every.to/also-true-for-humans/mini-vibe-check-typesafe-s-jev-judged-everything-i-ve-written-in-0-7-seconds>
- **Jagent（有商业利益，卖 credits）**：8 fixtures × 5 次 = 每模型 40 次，同一 gateway，2026-09-20；Jev 1.13 中位 **352 ms**、$0.0000188；Mistral Small 3.2 1,343 ms；Gemini 2.5 Flash Lite 877 ms；GPT-5 nano 7,504 ms。另用服务器自己的 `x-envoy-upstream-service-time` 头测并行性：**1 个问题 70 ms，3 个 72 ms，4 个 81 ms，5 个 74 ms**——"'Questions are evaluated in parallel' is a vendor claim everywhere else — this is it read off the server's own header."。【第三方】<https://jev-agent.com/benchmarks>
  - Jagent 自曝两次"差点把对 Jev 有利的假象写成结论"：没开 `response_format: {type:"json_schema", strict:true}` 就说"chat 模型返回类型不可靠"（开启后 Gemini 2/5→5/5、GPT-5 nano 4/5→5/5）；没设 `temperature: 0` 就说"chat 模型不一致"（设置后全部稳定）。结论：**"If you are evaluating a decision model against a chat model, check your own request body before you believe your own table."**；其 27 张自编工单 × 3 遍结果显示 Jev **并不在准确率上赢**（Jev 27/27、Mistral 27/27、Gemini 26/27、GPT-5 nano 26/27）；唯一只有 Jev 产出置信度：清晰集 0.979、歧义集 0.841。【第三方】
- **Arize 的小型独立测试**：通用模型每次是非判定花约 **910 个输出 token**，Jev 花 **85** 且不计费；Arize 引用官方 evals 汇总为 **Jev 68% 准确率 / $0.0004 / 0.4s（每 case）**，对比 GPT-5.6 Terra **68% / $0.03 / 10s**、Opus 5 **73% / $0.18 / 38s**。Arize 对官方「can't hallucinate」的评论是 *that really feels like an over-reach*。【第三方】<https://arize.com/blog/typesafe-jev-llm-judge/>
- **Octomind 的上下文筛选实测**：把 1,132 行 / 46 个 chunk 的源码一次性作为 state、每 chunk 一个 Noul 判断「这段是否回答了问题」，**一次调用 14,398 input tokens、584ms**，前 6 名中有 3 个不含关键词但语义相关。样本量为 1，属存在性证明。【第三方】<https://octomind.run/blog/jev-system-one-model-ai-agents>
- **Octomind 的自洽性观察**：同一 `rm -rf node_modules && npm install` 请求重复三次，每个数字变动 ≤0.02；同一请求下 Choice `confidence` 0.33（allow 0.55 / ask 0.41 / block 0.04）而 Score `confidence` 0.93，作者解读为「Choice 问的是 state 里没有的策略，Jev 按字面读，如实报告无法判断」；该文另记录官方「不要跨问题类型迁移阈值」现象的自测：Noul 0.60 vs Choice `P(yes)` 0.81。【第三方】
- **Spring AI（Java 集成）**：笔记本实测 1 题 275ms / 3 题 310ms；写 "Jev does not stream. Nothing is generated token by token"；其工单分类示例记录「裸标签 confidence 0.60 → 补描述 0.82」。【第三方】<https://spring.io/blog/2026/09/21/spring-ai-typesafe-structured-judgment>
- **LangChain→Langfuse 转述的 Good Start Labs（6,003 次 rubric 评测）**：Jev 与 Claude Fable 5.1 一致率 91.5%；每百万条被评分答案成本 Jev $160 vs Fable 5.1 $33,000、GPT-5.6 Luna $400、Gemini 3.8 Flash $1,600、DeepSeek V4.1 Flash $260（一致率 93.5%，比 Jev 高 2pp 但贵 $100）。【未证实，仅见 Langfuse 一处转述】<https://langfuse.com/blog/2026-09-18-using-typesafes-jev-for-evals>

### 6.5 数字分级清单：可引用 / 需标注 / 不可引用

#### A. 可引用（附条件即可）

| 数字 | 条件（必须一起写） | 来源 |
| --- | --- | --- |
| 单价 **$0.042/MTok 输入，输出免费** | 原生 API 口径；输出免费另有一次第三方实证 | 【官方】/models；【第三方】<https://github.com/anisselbd/jev-phishing-bench> |
| 限流 **250k tok/s、1,200 rpm** | 官方声明"动态调整、可无通知变更"，**是上界不是 SLA** | 【官方】/models |
| 上下文 **64k/请求**、**state+最长问题 ≤32k**、仅文本、Choice ≤255、Score 2–10 | 官方文档口径；第三方渠道只标 32K | 【官方】/models、/api |
| 端到端 **70–500 ms（官方）** | 官方自述跑在自家西海岸笔记本，**与所在区域强相关** | 【官方】官方发布博客 |
| **p50 126.81 ms vs Haiku 688.40 ms（5.43x，CI 5.20–5.71）** | 分类环节；并发 1；10s deadline；零重试；不含完成模型成本 | 【第三方】<https://docs.litellm.ai/blog/jev-auto-router-benchmark> |
| **单次分类成本 $0.0000321111**、成本差 **−96.118%（95.973–96.266）** | 同上，"cost savings stop at the classifier boundary" | 【第三方】同上 |
| **500/500 与人工 oracle 一致**；方差比 Terra 低 913x | 5 条样本 × 100 次；作者自称 observational；未记录 Jev 版本 | 【第三方】<https://www.langchain.com/blog/jev-agent-evals-langsmith> |
| **Jevals：31,500 次判定；noul 与最强 LLM 统计并列（区间含 0）；1/28 价格** | 每类问题一个数据集；人工标签；无前沿模型行 | 【第三方】<https://jevals.com/> |
| **JevBench：Jev 1.13.0 74.4 分第一，领先 1.3 分** | 534 条冻结用例；部分延迟为假设调整 | 【第三方】<https://github.com/fstandhartinger/jevbench/blob/main/RESULTS-v1.2.md> |
| **钓鱼基准：Jev 62.6% vs Haiku 81.3%（McNemar p<0.0001）；ECE 0.154 vs 0.097** | 2000 封合成邮件；真值来自 URL 信誉库；无并发 | 【第三方】<https://github.com/anisselbd/jev-phishing-bench> |
| **5 信号 + 逻辑回归 = 95.1% / AUROC 0.988 / ECE 0.027** | 同上；另有非 AI 正则基线 91.6% 的对照 | 【第三方】同上 |
| **并行提问：1 问 70 ms / 5 问 74 ms** | 读服务器 `x-envoy-upstream-service-time` 头；单一第三方；有商业利益 | 【第三方】<https://jev-agent.com/benchmarks> |
| **状态页 99.840%** | 窗口长度未标明 | 【官方】<https://status.typesafe.ai/> |
| **分类环节 12.2×/10.0× 与 11.5×/9.6×（批处理）** | 官方两处口径并存；cookbook 附原始金额表，建议以它为准 | 【官方】parallel cookbook、/primitives |

#### B. 需标注后才能引用（口径限定）

| 数字 | 必须注明的口径差异 | 来源 |
| --- | --- | --- |
| **67.8% / $0.0004 / 0.4s（聚合）** | 官方自评；参考标签 = GPT-6 Astra + Claude Fable 5.1 **high thinking 的平均**；"假设 harness 正确"；LLM 侧套了官方 wrapper | 【官方】<https://evals.typesafe.ai/> |
| **0% 幻觉** | 官方自认 "not empirical"，只是 schema 匹配保证 | 【官方】官方发布博客 |
| **~$7/hour**（Doom demo） | 官方演示场景假设（10 queries/秒），非通用成本模型 | 【官方】同上 |
| **$4.20 / 10 万次请求** | 第三方换算，假设 1,000 输入 token/次，核价 2026-09-19 | 【第三方】<https://www.requesty.ai/blog/typesafe-jev-explained> |
| **延迟 100ms（Flavio Copes）/ 0.23s（OpenRouter P50）/ 352 ms（Jagent）/ 0.44–0.48s（Jevals）/ 132–272 ms（Vercel）/ 0.59s（NearHere）** | **各来源口径完全不同**（网络位置、是否含本地 relay、是否端到端）；唯一可安全引用的是"百毫秒级绝对量级"与"同任务下比 Haiku 快数倍"的相对量级 | 见 §6.4 各条 |
| **"Jev 与前沿模型相当"** | 仅厂商口径；无人工标注的独立复核 | 【官方】<https://evals.typesafe.ai/> |
| **Vercel 385 条样本 Banking77 = 80.3%** | 与 Jevals 79.7% 一致，但样本不同、渠道不同 | 【第三方】<https://github.com/vercel/ai/issues/20846> |
| **OpenRouter 778B tokens / 7 天** | 平台流量统计，非性能指标；口径为 OpenRouter 作者页 | 【第三方】<https://openrouter.ai/typesafe> |

#### C. 不可引用（不得作为技术决策依据）

| 数字 | 原因 | 来源/证据 |
| --- | --- | --- |
| **193.6x faster / 444.6x cheaper** | 官方自评乘数，**无法从公开圆整数字复算**（复算得 75.1x / 171.4x），官方自称"处于真实世界收益的偏高端" | <https://flowtivity.ai/blog/jev-typesafe-ai-decision-model/>、<https://kingy.ai/blog/typesafe-jev-review-the-ai-model-that-doesnt-generate-text/>、官方发布博客【未证实（不可核对）】 |
| **"45% faster"（stock-analysis pipeline）** | **不是官方数字、不是任何受控基准**，是匿名用户自述轶事；原始出处未定位（Reddit 被人机验证拦截；HN 509 条评论全文搜索零命中）；AINews 原文自用 "One limited benchmark" 且无作者/数据/代码 | 【未证实】<https://www.latent.space/p/ainews-jev-a-system-one-model-that> |
| **Vercel "~5–18x faster and more accurate"** | 无数据集、无案例数；Vercel changelog 的措辞是 "TypeSafe **reports**…"，即**转述厂商数字**；截至 9 月 17 日未发布该基准 | 【未证实】<https://vercel.com/changelog/typesafe-ai-jev-now-available-on-ai-gateway> |
| **Facebook 帖"3x DeepSeek cost、约 45% faster"** | 另一段主观比较，无方法学 | 【未证实】<https://www.facebook.com/jhunter101/posts/so-ive-been-using-jev-by-type-safe-ai-for-4-hours-tonight-ive-ran-about-300000-t/4097315900569291/> |
| **YouTube 标题里的 "25×"** | 该数字就是 Every/Dan Shipper 的 0.35s vs 8.83s，**不是新的独立测试** | 【未证实】<https://www.youtube.com/watch?v=hfi4ad8OA3Y> |

### 6.6 对官方基准的质疑（harness bias 等）

- **核心质疑是标签来源**：参考标签由 GPT-6 Astra 与 Claude Fable 5.1 的平均生成、**无人工标注**，因此官方准确率本质是"与模型参考的吻合度"。后果是"两个前沿模型都答错的题，答对的模型反而被扣分；两个模型共享的盲区，有同样盲区的模型反而被奖励"。【第三方】<https://dev.to/gabrielanhaia/jev-beat-gpt-luna-by-1-point-gpt-6-and-claude-wrote-the-answer-key-314k>
- kingy.ai 的定性："the reported accuracy is better understood as **agreement with a model-derived reference rather than as independently verified correctness**"——本质是"a model-agreement test rather than verified ground truth"。【第三方】<https://kingy.ai/blog/typesafe-jev-review-the-ai-model-that-doesnt-generate-text/>
- Anthony Maio：参考标签"came from averaging GPT-6 Astra and Claude Fable 5.1 at high reasoning settings, **not from independent operational ground truth**"。【第三方】<https://anthonymaio.substack.com/p/jev-the-language-model-that-wont>
- **官方自己承认的 harness bias 三类**：工作流由自家能力团队成员制作，"**some bias could exist**"；参考答案偏向 OpenAI/Anthropic（官方自称这反而**低估**自家模型与 DeepSeek）；**LLM 侧被套上 TypeSafe 自家的 "System One LLM wrapper"**（flowtivity 的审计直接记为 "against LLMs wrapped in TypeSafe's adapter"，官方自述该 wrapper 更慢更贵）。【官方】官方发布博客；【第三方】flowtivity
- **速度数字的可比性**：HN 评论者 ramon156（经 flowtivity 转述）称"so far all claims just sound like marketing terms"，指出 **70 ms vs 329 秒不是同类比较**（若 LLM 基线在跑完整 chain-of-thought 生成）。【第三方】flowtivity
- **"零幻觉"与"校准"的口径问题**：类型安全 ≠ 语义正确——"A wrong Choice **is not a type error**"；"confidence **is not independently demonstrated to be calibrated**"，TypeSafe 未公布标准校准指标。【第三方】kingy.ai
- Maio 的结构性批评："The **reward function, architecture, training procedure, and calibration methodology are all undisclosed. There are no published calibration curves and no independently reproducible paper.**"；两条风险：校准必须在**分布漂移**下成立；**"individually calibrated judgments do not automatically compose into a calibrated workflow once you run them through thresholds, weights, and branches. Correlated mistakes survive composition."**；判决句："the strongest case for Jev is **architectural rather than algorithmic**"。【第三方】Maio
- flowtivity 记录了 HN 上最有价值的一次交换：TypeSafe 创始人 **Diogo Almeida 在发布帖里直接承认 "'It's also possible to be confidently wrong…'"**——即类型合法的答案仍可能错；另有转述批评称把 "frontier model" 用于"cannot hold a conversation"的模型 **"borrows credibility it has not independently earned"**。【第三方】flowtivity
- **官方主动放弃了公开基准**：博客《Lies, Damned Lies, and Benchmarks》宣布「**不在模型发布中放标准基准表**；新 evals 只做带日期的快照、发布后即淘汰、不再 hill-climb」，同时公布内部 evals 并附不利证据——这既是回应质疑，也意味着**外部无法用公开 benchmark 独立比对**。【官方】<https://typesafe.ai/blog/antibenchmaxxing>

### 6.7 独立评测给出的反面证据与边界行为

- **准确率反面证据汇总**：钓鱼基准 Jev 62.6% vs Haiku 81.3%（p<0.0001，"loses clearly on accuracy…wins on speed and cost"）；Jevals 的 choice 任务 Jev 被 Gemini 3.8 Flash 显著领先；JevBench 里 GPT-5.6 Luna(low) 的 Intelligence/Calibration 都高于 Jev；Jagent 的 27 张工单上 Jev 与 Mistral 并列 27/27。
- **最一致的正面发现是"一次调用多问 + 在代码里组合"**：同一次调用里的 5 个信号喂给逻辑回归把准确率从 62.6% 提到 **95.1%**（AUROC 0.988、ECE 0.027）；但同一数据集上一条非 AI 固定规则也到 91.6%——**收益来自信号组合，而不是单次判定**。
- **边界行为是主要风险面**（以下五项均为 awesome-typesafe 索引转述，上游未逐字核对原始结果文件，**引用前需回原始仓库核对**）【未证实（索引转述）】：
  - `jev-certify`（CLINC150）：5% 的静默误路由上界在 400 条域内样本上成立（84.75% 自动路由，每来件损失 2.25%），但**另一个"范围门"在域外比例上升时超出 5% 目标 3.6 倍**。<https://github.com/nikkoxgonzales/jev-certify>
  - `jev-orderby-bench`：20 Newsgroups 主题归属通过 6 个排序门，但在 306 条人工标注的购物相关性上**6 条里挂了 4 条**；**360 行里 53 行并列在 0.99**（两位小数输出），`LIMIT k` 会切在并列内；**把 40 行批处理成一次请求会把原本通过的排序门变成失败**。<https://github.com/yodablocks/jev-orderby-bench>
  - `jev-calibration-audit`：KoBBQ 上，当选项里有 "unknown" 时 Jev 对 **95% 的 300 条歧义样本选弃权**；把该 gold 选项移出选项集后，这些样本准确率**必然为 0%**，且 **79% 的答案选了数据集里的刻板印象答案**。<https://github.com/jujumilk3/jev-calibration-audit>
  - `jev-ood-calibration`：在三个公开集上概率接近校准，但在**未见过的优先级规则**上过度自信，且 Boolean 问题表现出**相反方向的误差**。<https://github.com/scienthoon/jev-ood-calibration>
  - 111 例 agent 动作门控研究：Jev 与人工标签一致 100 例、Claude 102 例；**两者各有 1 例"不安全却放行"**；"合同与策略映射"是双方最大的单一错误来源。<https://github.com/ghubnab99/jev-enterprise-decision-fabric>
  - 索引来源：【第三方】<https://github.com/AbdelStark/awesome-typesafe>
- **弃权/并行/批处理的实测**（pin 到具体工程结论）：
  - 有 "unknown" 兜底选项时弃权率极高 → **不要指望 Choice 替你判断"是否适用"**（同 §3.4.1 的 Spring AI 观察）。
  - 批处理可能改变个别判定（`jev-orderby-bench`）→ 与官方"批处理不改变答案"的结论**方向相反，须在本项目数据上验证**（官方结论见 §3.6）。

### 6.8 面向报告引用的三条收束

- 平台侧可直接落笔的事实：**版本 `jev-1.13.0`、输入 $0.042/MTok（输出免费）、250k tok/s 与 1,200 rpm（动态、非 SLA）、64k/请求与 32k（state+最长问题）、仅文本、美国托管、单一 provider、状态页 99.840%（窗口未标）**。
- 性能侧可安全引用的量级：**单次分类约 3e-5 美元**（LiteLLM $0.0000321111）、**单次判定毫秒到百毫秒级**、**同任务下比 Haiku 快数倍**、**方差显著低（LangChain 500/500 与 oracle 一致）**；**不要用 193.6x / 444.6x 或 "45% faster" 作为架构前提**。
- 评测侧的引号纪律：看到 "67.8%" 要写"与 Astra+Fable 平均标签的吻合度"；看到 "70–500 ms" 要写"官方自测、西海岸、与区域强相关"；看到 "5.43x" 要写"分类环节 p50 统计量比、含置信区间、不含完成模型成本"。

### 6.9 本节不确定项

1. 上游明确说：**未做任何一手实测**，全部为公开材料核对（含对官方与第三方页面的算术复核）；第三方统计结论均为转述原作者。
2. 分类标准是上游自定，**"受控实验"与"厂商自测"之间没有第三方仲裁机制**。
3. **官方 67.8% 的"真值"不存在**：它是与 Astra+Fable 平均标签的吻合度；上游**未找到任何人工标注、覆盖相同四个工作流的独立复核**——所以"Jev 与前沿模型相当"目前**只有厂商口径支持**。
4. **193.6x / 444.6x 无法复算**：可能是 dashboard 上某个特定工作流的配对数字，但公开页面无法还原；**列为不可引用不是因为必然虚假，而是不可核对**。flowtivity 的引用值 vs 官网圆整表值属于两套口径并存，冲突未解。
5. "Luna 51.9% → 66.8%"这一对照仅在上游一处出现，**未附具体条件**，宜视为官方口径的二手转述。
6. 官方给 LLM 基线套了自家 **"System One LLM wrapper"**（把 LLM 约束成结构化输出），官方自述这会 **"slower and more expensive"**——**基线的公平性因此存疑**。
7. **本轮所有实验几乎都跑在 `jev-1.13.0` 上**，跨版本外推需谨慎；LangChain、Jevals 都出现"版本不可见"问题。
8. LiteLLM 的结果是**分类环节**，明说不含端到端速度或负载下吞吐；JevBench 的自托管延迟被"×2 并加 0.15 s"（作者明说**是假设而非测量**）、成本多为估算、留出用例会暴露给被测服务。
9. 并行提问的证据：官方声称 + Jagent 服务器头实测 + Every 的 37 文档 × 21 问 <0.7s，方向一致；但上游**没有找到一份把"问题数量 vs 延迟"画成曲线的受控实验**。
10. LangChain 实验仅 5 条样本、任务单一（作者自称 observational）；Jagent 有商业利益；LiteLLM 两侧 prompt 都**未经独立调优**。
11. HN / 社区批评多为**观点而非测量**（见 §12）；本节把它们与第三方的定量结果分开呈现。
12. **校准缺乏与厂商定位相称的独立证据**：唯一直接测 ECE 的第三方实验显示 Jev ECE(0.154) 比 Haiku 4.5(0.097) 更差（钓鱼任务）；Jevals 明说每类问题只用一个数据集、校准"已知会随任务变化"——**目前不能把"概率可靠、可按阈值分支"当成已验证前提**。
13. "官方低估自家模型"是官方自述，**没有任何第三方独立验证**。
14. **§6.7 后半段的五项边界研究均为索引转述**，上游未逐字核对原始结果文件，故标为【未证实】。
15. 钓鱼基准本身也有明确边界：**邮件正文是合成的、真值来自 URL 信誉库而非人工读信、每系统只用一套 prompt**；作者追加的对照组进一步说明该数据集"largely separates by construction"。
16. Every 的样本很小（37 篇文档、12 段合成文本），且 TypeSafe 与 Every 的关系未披露（TypeSafe 曾在 AIE 预发布），作者本人也要求更完整的准确率检查。
17. **没有任何单份第三方实验同时覆盖"准确率 + 校准 + 门槛稳定性 + 组合后的工作流"**；跨实验拼结论正是 Maio 警告的 "correlated mistakes survive composition" 场景。
18. "Jev 是否更准"在现有公开证据下**没有一致答案**：准确率轴上多项独立测试不利于 Jev，速度/成本/稳定性轴上多项独立测试有利于 Jev；本节不给出单一结论。若需要"Jev 优于 LLM"的定量支撑，现有可引用证据**只支持"成本与延迟的量级优势 + 方差的相对优势"**，不支持"准确率更高"。
19. 依赖转述的数字（引用时须标二次来源）：Every 原文（JS/订阅墙，三处转述一致）、Good Start Labs 的 6,003 次评测（仅 Langfuse 一处转述）、bitnovus 的分档校准（本轮主要经 Arize 转述，仓库数据文件未逐行核对）。

---

## 7. 应用场景与 Agent Harness 集成模式

**要点**

1. **Jev 的生产用法高度收敛为四类"决策原语"**：分类/分流（Choice）、评分（Score）、是非判定（Noul），以及由其派生的**路由、门控、校验、抽取**；官方称之为 "AI-Powered Workflows / smart if-statements"。【官方】
2. **官方明确 Jev 不能当 coding agent 的主模型**："Jev is **not** a drop-in replacement for the LLM behind Claude Code, Cursor, opencode, Copilot…"，且 "There is no `model: "jev-latest"` setting that turns your coding agent into a Jev-powered agent"。生态里所有落地都把它放在循环的**决策点**上。【官方】
3. **本次调研有实证支撑的决策点共 6 类**：模型路由、工具调用门控、技能/工具集选择、上下文筛选与压缩、评测（judge）、危险动作/注入分类。**「终止判断（是否完成）」未找到官方形态**，只有第三方设计草图且作者声明未实现。【官方/集成方】+【第三方】
4. **失败语义是本专题最有价值的工程结论，两套框架一致：路由与技能选择 fail open，工具门控 fail closed。**【官方·集成方】
5. **门控的输入形态已被固化为设计规则**：state 只放「用户消息 + 被判定动作 + 判定必需的廉价事实」，**绝不放工具输出与助手消息**——"Tool output and assistant messages are excluded so untrusted content cannot authorize its own execution."。【官方·集成方】
6. 「LLM 决策 + Jev 判定」有 **4 种代码形态**：① Jev 作为 provider 跑 agent（Pydantic output function）；② per-step 模型选择（SelectModel）；③ 框架 middleware 前置分类（LangChain）；④ 置信度低于阈值时交给 LLM 接管（FallbackModel）。【官方·集成方】
7. **置信度是边界余量（margin）而非"答案正确的概率"**，`float` 字段甚至没有 confidence；任何"低置信度回退 LLM"的设计都必须按这个定义写。【官方·集成方】
8. **风险面三方一致：提示注入可以移动判定**（官方、集成方、第三方独立实测），因此门控必须与确定性检查并用，不能作为唯一防线；state 里的内容会流向第三方（数据外泄面）。【官方 + 第三方】

### 7.1 官方场景地图：按「决策形状」而非按行业分类

**【官方】** 官方「Example use cases」页把场景按**决策形状**列表（文档自述为 10 类，表内实际列出 8 行，此处以表内条目为准）（<https://docs.typesafe.ai/concepts/use-case-map.md>）：

| 决策形状 | 何时用 | 官方举例 |
| --- | --- | --- |
| Classification | 已知类别中应有一个胜出 | 意图、话题、部门、风险类型、实体类型 |
| Detection | 需要一个性质存在的概率 | 垃圾、欺诈、紧急度、越狱、敏感数据 |
| Scoring | 答案落在有序 rubric 上 | 严重度、相关性、质量、挫败感、适配度 |
| Routing | 一个类别决定下一条代码路径 | 工具选择、升级、模型路由、支持队列 |
| Search / Retrieval / Ranking | 语义搜索、RAG 上下文、排序 | 语义搜索、文档发现、证据检索 |
| Verification | 检查某类失败模式 | 引用支撑、政策违规、工具调用错误、回复质量 |
| ML Feature Extraction | 下游经典 ML 需要语义信号 | 购买意图、产品兴趣、竞争压力、流失信号 |
| Structured Data Extraction | 从非结构化输入恢复已知字段 | 候选人属性、订单字段、文档标签 |

- 同页两个专门块与本项目直接相关：
  - **Harness Engineering**——"Use Jev queries to make your harness smarter - model routing, semantic context retrieval, LLM error detection and guardrails, reasoning trace classification at lightspeed and a fraction of the cost."
  - **Universal Verification**——校验 prompt、抽取结果、推理轨迹、工具调用。
- 行业清单（官方，节选）：客户支持（工单分类、紧急度/挫败感/流失风险/退款请求、路由、按政策校验回复）、内容审核与信任安全、保险理赔、金融犯罪、法律合规、电商、招聘、线索生成、广告、游戏、风险评估、需求预测、知识图谱、科研、语义代码 lint。
- **【官方】** 官方命名的四个模式：**Speculative Fan-Out、Confidence-Gated Routing、Composite Scoring、Intent Routing**（<https://docs.typesafe.ai/patterns>）。
- **【官方】** 官方发布博客给出的四类用例：AI-Powered Workflows / smart if-statements；Map-reducing over big data；Real-time applications（"100ms speeds means you can use AI in your applications where UX is critical."）；Verify everything（"Score, judge, verify, guardrail, and detect jailbreaks of LLM prompts, reasoning traces, and/or outputs."）。
- **【官方·集成方】** Vercel 列出的典型用途可视为同一批场景的工程化表述：选 agent 的下一个工具 / 子 agent、决定工作流继续 / 重试 / 询问用户 / 停止、动作前打分紧急度或风险、校验模型输出 / 执行 guardrail / 把不确定的案例转人工（<https://vercel.com/blog/ai-gateway-jev-model-launch>）。

### 7.2 官方 cookbook 中的量化场景（均为官方自测）

| 场景 | 官方数字 | 出处（官方自测） |
| --- | --- | --- |
| 并行提问（13 问监管简报，GDPR 维基条目） | 批一次 **12.2x 便宜 / 10.0x 快**（$0.000497 / 0.27s vs $0.006090 / 2.71s），且答案无变化 | [Parallel questions](https://docs.typesafe.ai/cookbooks/parallel_questions) |
| 同一实验的**另一口径** | [Primitives 页](https://docs.typesafe.ai/primitives) 写 **11.5x / 9.6x**（口径冲突，详见 §3.6） | [Primitives](https://docs.typesafe.ai/primitives) |
| Re-ranking（CLERC 法律查询，40 query × 30 个 BM25 候选） | top-1 **5% → 18%**、top-5 15% → 35%、top-10 **38% → 62%**；1,200 次调用共 1,536,002 input tokens = $0.0645 | [Re-ranking](https://docs.typesafe.ai/cookbooks/rerank_typesafe) |
| Skill suggestion（Nous Research Hermes 目录 **182 个技能**，选至多 1 个） | 两次 Jev 请求后：错误加载 **16.8% → 7.3%**，无 skill 适用时仍加载 **9.8% → 4.0%**；天花板 2.5% / 1.2% | [Skill suggestion](https://docs.typesafe.ai/cookbooks/skill_suggestion) |
| Entity alignment | 450 个候选对：**40 合并 / 50 转人工 / 360 不关联**，一个三级 Score 承载整个决策 | [Entity alignment](https://docs.typesafe.ai/cookbooks/entity_alignment) |
| 函数调用（工具路由） | 10 个函数、**28 个可填充参数**，示例 confidence 0.91 / 0.94 / 0.75 / 1.00；`int` 之类非封闭集参数不提问 | [Function calling](https://docs.typesafe.ai/cookbooks/function_calling.md) |
| LLM 内容审核（hazard battery） | 入站与出站各一组问题、**一次请求**：4 个 Noul 各问一类 hazard + 1 个 Score 问「若照做会造成多大伤害」（4 级，No harm → Severe），代码用阈值分 pass / review / block / support 四路 | [LLM guardrails](https://docs.typesafe.ai/cookbooks/llm_guardrails.md) |
| RAG 段落过滤 | 每题一个 Noul，四阈值 0.70 / 0.70 / 0.45 / 0.55；注入段落以 **0.99** 被拦下 | [Classifying RAG passages](https://docs.typesafe.ai/cookbooks/classifying_rag_passages) |
| 其他被目录提及的场景 | Double-checking citations、SDE cascade（mini → verify → reasoning）、Hierarchical classification、Date extraction、Pre-parsed value extraction、Classification using confidence（SEC 年报分 75 个行业组） | [llms.txt](https://docs.typesafe.ai/llms.txt) |

- **【官方】** Skill suggestion cookbook 记录的副作用值得在 harness 设计中引用：*A confident wrong suggestion is more persuasive than no suggestion at all*；同页指出 **Choice 与 Noul 回答的是不同问题**（Choice 是相对的「选哪个」，Noul 是绝对的「要不要说」），两者可以不一致，不能用前者推后者。
- **【官方】** 内容审核 cookbook 的关键提示（原文）：*Run this TypeSafe check both on LLM inputs, and on LLM outputs, because even ordinary-looking prompts can lead to harmful generated replies.* 该 cookbook 自注数据来自 `jev-1.12`（2026-08-15），越狱样本逐字取自公开数据集 `TrustAIRLab/in-the-wild-jailbreak-prompts`。
- **【第三方】** CloudRaft 的用例综述指出一条模态边界：图像审核仍需先用 VLM 把图片转成文本（Jev 只吃文本），随后 caption + metadata 作为 state，一次调用给出多个 Noul（骚扰 / 诈骗 / 垃圾 / 自伤风险）+ Choice（allow / hide / timeout / escalate）+ Score（0–3 严重度），再用普通代码阈值决策（<https://www.cloudraft.io/blog/top-use-cases-of-jev-typesafe-ai-model>）。

### 7.3 六个决策点

#### 决策点 1：模型路由 / 分层（Routing）

- **【官方·集成方】** LangChain `ModelRouterMiddleware` 挂在 `before_agent` 与 `wrap_model_call`：把最新一条 human message 分类一次，选择的模型用于该 run 的**每一次** model call；概率与置信度留在 agent state 里供应用与 trace 使用（<https://docs.langchain.com/oss/python/integrations/providers/typesafe>、<https://www.langchain.com/blog/building-a-harness-with-jev>）。
- **【官方·集成方】** 源码：`before_agent` 的分类结果写入 state 的 `model_route`；`wrap_model_call` 用 `request.override(model=...)` 替换；**分类异常被 catch 并回退 `default_route`**（注释：*routing must not break the agent*）（<https://github.com/langchain-ai/langchain/pull/40556>）。
- **【官方·集成方】** Pydantic AI 提供两种路由粒度：`output_type=route`（把决策写成函数签名，Jev 选 `tier` 后由框架调用该函数，一次请求完成路由+执行）；`SelectModel` capability 在**每一步**前重新分类（*a run that starts simple and turns hard moves up when it turns*）。
- **【官方·集成方】** Pydantic 提醒：`SelectModel` 读的是完整 history，长 run 会让每次路由请求变大变慢，官方建议**与 compaction 配对**——*the compacted history is what the router reads*。
- **【官方·集成方】** LiteLLM 的等价物是 tier 路由 + 分类器熔断：`classifier_type: jev` + `jev_classifier_config`（`model`、`timeout_ms`、`circuit_breaker_enabled`、`circuit_breaker_cooldown_seconds`）+ `classifier_fallback`（默认 `heuristic`，可设 `default_model`）+ 上下文控制（`classifier_context_window_size` 默认 3、`classifier_context_budget_chars` 默认 8000、`classifier_context_include_assistant_turns`）（<https://docs.litellm.ai/docs/proxy/auto_routing>、<https://docs.litellm.ai/docs/auto_router/setup>）。

#### 决策点 2：工具调用门控（Tool-call gating / guardrail）

- **【官方·集成方】** LangChain `AutoModeMiddleware(tools=[...])` 在 `wrap_tool_call` 前询问「这次调用是否 risky 或未获充分授权」的 `Noul` 概率；**达到或超过阈值就返回 error `ToolMessage`，工具体不执行**。
- **【官方·集成方】** 源码默认 `risk_threshold=0.2`，docstring 原文：*The conservative default blocks calls with at least 20% estimated risk.* 阈值可传 `[0,1]`，越界抛 `ValueError`；**未登记的工具直接放行**（`if request.tool_call["name"] not in self._tool_filter: return handler(request)`）。
- **【官方·集成方】** 送进 Jev 的 state 固定为：`user_messages`（只取 `HumanMessage`）+ `tool_call{id,name,args}` + 可选 `tool_description`；参数按 key 名脱敏（`api_key` / `token` / `secret` / `password` / `private_key` / `credential`）。已承认的局限：*Matching is by key name, so a secret carried inside an unrelated key is not caught.*
- **【官方·集成方】** 失败语义：门控 fail closed（*Classification failures propagate and the tool handler is not called*）；middleware traces 也会 `omit_payload` 以隐藏授权上下文与参数。
- **【官方·集成方】** Pydantic AI 的等价物是 `Hooks(before_tool_execute=...)` + `raise SkipToolExecution(...)`：拦截消息作为工具结果回给模型，「模型能学到什么被拒绝了，可以换一种做法」；不标 `requires_approval`、工具无需 opt-in，**包括之后新增的工具**。需要「审批离开进程」（人工在别的系统批准、排队、稍后 resume）时用 `requires_approval=True` + `HandleDeferredToolCalls`——*Both see validated arguments; only the deferral can outlive the run.*
- **【第三方】** S. Colvin 的 gist 用同一模式做了三字段门控（`action: Literal['accept','review','reject']` + `destructive` + `outside_directory` + `misleading`），脚本对同一条命令分别跑 `anthropic:claude-sonnet-5` 与 `typesafe:jev-latest` 并记日志；**该 gist 只提供脚本，未附实测结果**（<https://gist.github.com/samuelcolvin/fa2d9abf8349b18e360f2d326218ffa7>）。
- **【第三方】** 相关社区实践：`pi-warden` 用 Jev 判定 bash/write/edit 动作，跨 315 个会话、**18,075 次受保护调用触发 48 次 hold**（约 3 次/1000 调用）；13 个被标注的 hold 中 **2 个站得住、11 个重试后通过**；另有 150 组配对运行对照（对照组违反项目规则 6 次，启用后 0 次）。作者自注 "These are project-maintained benchmarks, not universal claims."（<https://pi.dev/packages/pi-warden>）

#### 决策点 3：技能 / 工具集选择（Skill or tool selection）

- **【官方·集成方】** LangChain `SkillsMiddleware`：每个 skill 独立打分，**任意数量**可命中；只有被选中的 skill 指令被前置到 model request，*so the roster never enters the prompt*；技能来源是 Agent Skills 兼容的 `SKILL.md`（路径/对象/字符串），`skills_root` 在符号链接解析后限定在单一目录。分类失败时 *Adds no skills*（fail open）。官方警告：*never load a `SKILL.md` a user can write.*
- **【官方·集成方】** Pydantic AI 用 `PrepareTools` 钩子「在大工具集上线前先问这次请求需要哪些」，另有 history processor 在压缩前筛掉不相关片段。
- **【官方】** 官方 Skill suggestion cookbook 量化：182 个 skill、488 条单轮请求、被测 agent 为 `claude-haiku-4-5-20251001`、发布版本 `jev-1.12`、渲染日期 2026-07-31；加两道 Jev 请求后错误加载 16.8% → 7.3%、无 skill 适用时仍加载 9.8% → 4.0%（天花板 2.5% / 1.2%）（<https://docs.typesafe.ai/cookbooks/skill_suggestion>）。

#### 决策点 4：上下文筛选与压缩（Context filtering / condensation）

- **【官方·集成方】** Pydantic 官方把 history processor 与 compaction capability 列为 Jev 可直接承担的决策点，并给出容量约束：`jev-1.13` 的 state 与 questions **合计 64k tokens，其中 state + 最长问题 32k**；超限以 `ModelHTTPError(max_tokens_exceeded)` 失败；**若 Jev 后面挂了 `FallbackModel`，超长会话会被静默交给 LLM**（成本模型随之改变）。官方建议 *Compact earlier than a language model would need, since Jev is being asked to judge the whole of it, not to continue from it.*
- **【第三方】** Octomind 的 46-chunk / 14,398 input tokens / 584ms 实验（见 §6.4）是这一类用法的存在性证明。
- **【第三方】** 社区插件 `tamaratran/fast-jev-compaction`：Claude Code 插件，用 Jev 给工具调用打分以替换 compaction 摘要；GitHub **6013 stars**（created 2026-09-17）（<https://github.com/tamaratran/fast-jev-compaction>）。

#### 决策点 5：评测 / Judge（Eval）

- **【第三方】** Langfuse 的适用条件（原文）：*Jev can help anywhere in your pipeline or eval harness where decisions are **repeated, high volume, and the possible answers are known before the call***——agent 与工具路由、文档/工单分类、升级决策、只需要 rubric 判定时的评测打分（<https://langfuse.com/blog/2026-09-18-using-typesafes-jev-for-evals>）。
- **【第三方】** Langfuse 指出的两个坑：**Noul 没有独立 confidence 字段**（概率本身就是答案），*so code that reads `answer.confidence` on everything will break on binaries*；Choice 上限 255 选项、Score 上限 10 级。
- **【第三方】** Arize 的建议架构（原文）：*Run a decision model on every trace for coverage. Sample failures and uncertain scores through an LLM judge when you need a written explanation.* 其对比表把 decision model（Jev）记为无需标签、$0.0004/case、0.4s、概率「Trained for it, per TypeSafe」、**无解释**；frontier LLM judge 记为 $0.03–$0.18、10–38s、verbalized 且过度自信、有解释（<https://arize.com/blog/typesafe-jev-llm-judge/>）。
- **【第三方】** Braintrust 把三个原语在一个请求里组合（JS，<https://www.braintrust.dev/blog/evaluate-agent-responses-with-jev>）：`category: choice(...)` + `urgency: score(["Routine","Soon","Urgent"])` + `duplicate_charge: noul(...)`。其置信度分级路由建议：**>0.95 自动接受；0.70–0.95 交给更强的 LLM 判官；<0.70 视为不确定或转人工**，并强调阈值必须先用自有数据验证。
- **【官方·集成方】** LangSmith Evals 的两条集成规范：**state 里不要放评分说明**（评分标准属于 questions）；每个 question 成为一个 feedback key（<https://www.langchain.com/blog/jev-is-now-available-in-langsmith-evals>）。

#### 决策点 6：终止 / 完成判断——未找到官方形态

- **【未证实】** 未找到 TypeSafe 或 LangChain 官方关于「用 Jev 判断任务是否完成 / 是否终止循环」的模式、API 或数据。唯一相关材料是第三方设计草图：Octomind 把 harness 控制面拆成 7 个判断，其中 *Is the work actually done?* 被作者自己标注为 **System Two — it has to read the diff and the request**，并明确写出该整合地图 *None of this is built*（<https://octomind.run/blog/jev-system-one-model-ai-agents>）。
- **【官方】** 官方 jaggedness 页把「生成」单列为失败模式 #9（*Use a generative model*），间接说明需要阅读全文才能判断的完成度检查不适合 Jev。

### 7.4 fail open 与 fail closed 的工程语义

**【官方·集成方】** LangChain 三个中间件与其失败语义（PR #40556 原文）：

| 中间件 | 决策 | 分类失败时 |
| --- | --- | --- |
| `ModelRouterMiddleware` | 由哪个模型处理本次 run | 回到 `default_route`（**fail open**） |
| `SkillsMiddleware` | 本次请求适用哪些 skills | 不添加任何 skill（**fail open**） |
| `AutoModeMiddleware` | 某次工具调用是否风险过高 | 异常向上抛，工具**不执行**（**fail closed**） |

- 设计原则原文：*Routing and skills fail open so a TypeSafe outage cannot stop an agent. The tool gate fails closed, which is the only defensible default for a control whose job is to block things.*
- **【官方·集成方】** 失败语义要配套 timeout + circuit breaker + fallback：LiteLLM 默认 `classifier_fallback: heuristic`（可改 `default_model`），并提供 `timeout_ms` 与熔断开关/冷却时间（见决策点 1 的配置项）。Pydantic AI 侧超时以 `ModelHTTPError` 失败，可用 `FallbackModel` 兜底——**注意兜底会静默改变成本模型**。
- **【官方·集成方】** 要小心两套版本：PyPI 已发布 `langchain-typesafe 0.0.1a3`（2026-09-20），README 用 `langchain_typesafe.experimental.middleware`、`AutoModeMiddleware(..., criteria=NoulCriteria(...))`；GitHub PR #40556 分支版用 `langchain_typesafe.middleware`、`default_route` 为必填、未见 `criteria` 参数，且该 PR 在 2026-09-22 查证时仍为 **open**（`state: open`, `merged: false`, created 2026-09-17, 37 files, +5712）（<https://pypi.org/project/langchain-typesafe/>）。**引用任何 API 签名前必须确认版本。**
- **本项目可迁移的三条规则**：
  1. 失败方向的判定应绑定「这个控制的作用」：**它的工作是放行就必须 fail open，它的工作是拦截就必须 fail closed**。
  2. 失败路径必须是显式的、可观测的（区分「判为安全」与「没能判定」两种状态）。
  3. fail open 的那一侧要有成本/质量上的合理默认（如 `default_route`、`heuristic`），不能回落成"无限制"。

### 7.5 具体代码形态（四种「LLM 决策 + Jev 判定」）

#### 形态 ① Jev 作为 provider 跑 agent，选中即执行（Pydantic AI output function）

```python
async def route(ctx: RunContext[None], tier: Literal['fast', 'capable']) -> str:
    """Answer the question on a model suited to it. ..."""
    model = 'openai:gpt-5.6-sol' if tier == 'capable' else 'openai:gpt-5.6-luna'
    return (await assistant.run(ctx.prompt, model=model)).output
router = Agent('typesafe:jev-latest', output_type=route)
# 一次 router.run(...) 就是全部：Jev 填 tier，框架调用 route，route 内部再调 LLM
```

出处：【官方·集成方】<https://pydantic.dev/docs/ai/models/typesafe/>

#### 形态 ② per-step 模型选择（Pydantic AI `SelectModel`）

```python
async def select_model(ctx: ModelSelectionContext[None]) -> Model:
    if not ctx.messages:
        return fast
    picked = await router.run(message_history=ctx.messages)
    return capable if picked.output == 'capable' else fast
agent = Agent(capabilities=[SelectModel(select_model)])
```

出处同上。

#### 形态 ③ 框架 middleware 前置分类（LangChain，一行式接入）

```python
from langchain_typesafe.experimental.middleware import ModelChoice, ModelRouterMiddleware, AutoModeMiddleware
router = ModelRouterMiddleware(
    choices={"fast": ModelChoice(model="openai:gpt-5-mini", criteria="Simple, well-scoped tasks."),
             "powerful": ModelChoice(model=powerful_model, criteria="Complex tasks requiring deeper reasoning.")},
    instructions="Choose the least costly model suited to the task.")
guardrail = AutoModeMiddleware(tools=["bash"])
agent = create_agent("openai:gpt-5-mini", middleware=[router, guardrail])
```

出处：【官方·集成方】<https://pypi.org/project/langchain-typesafe/>、<https://www.langchain.com/blog/building-a-harness-with-jev>。LangChain 博客/文档版用的是 `criteria="Direct lookups…"` 这类字符串描述 + `default_route`（<https://docs.langchain.com/oss/python/integrations/providers/typesafe>）。

**【官方·集成方】** `AutoModeMiddleware` 关键实现（head sha `cfb5a951b152b28b1e6d79f5f9576a1c0020ba02`）：

```python
@staticmethod
def _classification_state(request: ToolCallRequest) -> dict[str, Any]:
    """Only explicit user messages are included. Tool results and assistant
    messages are omitted so content the agent fetched cannot authorize its own execution."""
    user_messages = [m for m in request.state.get("messages", []) if isinstance(m, HumanMessage)]
    state = {"user_messages": convert_to_openai_messages(user_messages),
             "tool_call": {"id": ..., "name": ..., "args": _redact_sensitive_args(...)}}
def wrap_tool_call(self, request, handler):
    if request.tool_call["name"] not in self._tool_filter:
        return handler(request)                        # 未登记的工具直接放行
    response = self._client.system_one(self._classification_state(request), self._questions)
    risk = self._risk_probability(response)            # response.nouls["is_risky"].noul
    if risk >= self.risk_threshold:
        return self._blocked_tool_message(request, risk)   # error ToolMessage，工具不执行
    return handler(request)
```

#### 形态 ④ 置信度低于阈值时交给 LLM 接管（Pydantic AI cascade）

```python
def unsure(response: ModelResponse) -> bool:
    confidence = (response.provider_details or {}).get('confidence', {})
    return any(value < 0.8 for value in confidence.values())
model = FallbackModel('typesafe:jev-latest', 'openai:gpt-5.6-sol', fallback_on=[ModelAPIError, unsure])
```

- **【官方·集成方】** 该 `unsure` 只遍历 `provider_details['confidence']` 里的值，因此**不覆盖没有 confidence 的 float 字段**——这是形态 ④ 的实现陷阱。
- **【第三方】** flaviocopes 总结的官方三段式（置信度门控，<https://flaviocopes.com/jev/>）：`confidence < 0.5` → 转人工；只读动作（`check_balance`）直接放行；动钱动作（`approve_transfer`）`confidence > 0.9` 才确认执行，否则先问用户；该文明确说明 0.5 与 0.9 是**示例而非默认**。
- **【第三方】** Octomind 的更保守写法：`confidence < 0.5` → `ask`；`auto_allow` → `allow`（读操作错了便宜）；`block` 且 `confidence > 0.85` → `deny`（错了贵，门槛高）；其余 → `ask`（<https://octomind.run/blog/jev-system-one-model-ai-agents>）。
- **【未证实】** Octomind 还给出把 Jev 当作 Claude Code `PreToolUse` hook「更便宜的第一意见」的形态（逐动作风险分档 + 把廉价事实如 git dirty 塞进 state；同一命令、dirty 树与否使 ask 概率从 0.57 升到 0.80、`destroys_work` 从 0.63 升到 0.83），但作者声明该整合地图 *None of this is built*——**未在产品中实现**。

### 7.6 集成面一览（各框架接入方式）

- **【官方·集成方】** **LangChain (Python)**：`TypeSafeClassifier` 是一个 `Runnable`——`invoke`/`ainvoke`，输入 `{state, questions}`；state 可为字符串、JSON 对象/数组或 `BaseMessage` 序列；结果按类型分组在 `choices`/`scores`/`nouls`；trace 与 token 用量进 LangSmith。
- **【官方·集成方】** **LangChain 错误处理与传输层**：provider 错误继承 LangChain 标准 model-error 层级；可捕获 `TypeSafeRateLimitError`（含 `request_id`、`retry_after_ms`）或统一的 `ModelAuthenticationError` / `ModelRateLimitError`；`TypeSafeAPIError` 暴露 status/body/headers/sanitized endpoint/request id；未注入时自建 sync/async `httpx2` client，也可注入自定义 client（代理、连接池），生命周期由调用方负责（<https://pypi.org/project/langchain-typesafe/>）。
- **【官方·集成方】** **Pydantic AI**：`Agent('typesafe:jev-latest', output_type=Model)`；也可 `TypeSafeModel('jev-latest')` / `TypeSafeProvider(api_key=..., http_client=...)`；`output_type` 每个字段 = 一个问题，一次请求并行问完；`ModelResponse.model_name` 报告实际作答的版本化 id。通用 sampling 参数（temperature/top_p）被忽略；`timeout`/`extra_headers`/`extra_body` 透传；`typesafe_tool_call_threshold` 默认 **0.6**、`typesafe_boolean_threshold` 默认 **0.5**，越界为 `UserError`（<https://pydantic.dev/docs/ai/models/typesafe/>）。
- **【官方·集成方】** **LiteLLM**：走 **pass-through**，不是 `/chat/completions`——`POST /typesafe/v1/systemone`（`/typesafe/` 下任意路径都转发，`GET /typesafe/v1/models` 列模型）；LiteLLM 侧注入 TypeSafe key，客户端只用虚拟 key；计费按 `usage.input_tokens/output_tokens` 与 registry 中的 `typesafe/<model>`；日志按 TypeSafe 返回的版本化 id 记账（<https://docs.litellm.ai/docs/pass_through/typesafe>）。
- **【官方·集成方】** **Cloudflare**：`env.AI.run('typesafe/jev', {state, questions})` 或 `POST /accounts/$ID/ai/run`；文档页标注 context window 32,000 tokens（<https://developers.cloudflare.com/ai/models/typesafe/jev/>）。**LangSmith**：Jev 作为 judge provider 进入 Evals（Evaluators → LLM-as-a-Judge → Provider 选 TypeSafe、Model 选 `jev-latest`）（<https://www.langchain.com/blog/jev-is-now-available-in-langsmith-evals>）。**Braintrust**：把 Jev 作为 judge/scorer 接入（Trace span 名 `typesafe.systemOne`），保留 label/confidence/probabilities 供复核（<https://www.braintrust.dev/blog/evaluate-agent-responses-with-jev>）。
- **【官方】** **官方 SDK 只有** Python（`typesafe-sdk`）与 JavaScript / TypeScript（`@typesafe-ai/sdk`）；文档注明 *You can also call the HTTP API directly from any language.*（**无 Rust SDK**）（<https://docs.typesafe.ai/sdk.md>、<https://docs.typesafe.ai/llms.txt>）——Rust 侧路径详见 §10。
- **【未证实】** 其他被提及但本次未核实：Vercel AI Gateway 上架 `jev`；Langfuse 集成；Cloudflare、LangChain、Langfuse 三日内加入集成（<https://venturebeat.com/security/companies-are-putting-jev-in-charge-of-ai-agent-decisions-and-prompt-injection-can-influence-the-verdict>）。

### 7.7 官方「决策设计规范」（可直接映射到 Rust 侧）

- **【官方】** 一次请求并行多问是默认姿势：Patterns 把 Speculative Fan-Out（把包括只在某些分支才用得上的问题全都一次问出去，由代码决定读哪些）列为首要模式（<https://docs.typesafe.ai/patterns>）。
- **【官方】** 原子化：不要问「这个 pitch 好吗」，拆成市场规模 / 可行性 / 差异化三个问题，在代码里加权合并——*change a coefficient in your code rather than rewriting a prompt*（<https://docs.typesafe.ai/>、<https://pydantic.dev/docs/ai/models/typesafe/>）。
- **【官方】** 置信度三档：高 → 自动执行；中 → 确认 / 标记 / 补充信息；低 → 交人工或回退其他系统；阈值随风险缩放（示例：任何动作低于 0.6 一律转人工；高风险动作需 >0.85 才自动执行）（<https://docs.typesafe.ai/confidence>、<https://docs.typesafe.ai/patterns/confidence-routing>）。
- **【官方·集成方】** 每个阈值属于「该答案的用途」而非系统全局：自动执行的门槛应高于「标记待审」。
- **【官方·集成方】** 版本要钉死：`jev-latest` 会随发布漂移，调好阈值后应固定到 `jev-1.13.0` 之类版本化 id；建议日志记录实际作答版本。**选项顺序是输入的一部分**：重排 `Literal`/`Enum` 选项可能改变答案，重要分类要用多种顺序测。（社区侧也记录到"重排选项会大幅改变概率"的现象，见 §12.2）

### 7.8 失败代价与风险

**风险 1：提示注入可以移动判定（官方、集成方、独立实测三方一致）**

- **【官方】** jaggedness 页原文：*State is data, and `jev-1.13` does not treat it as hostile by default. Content written to adversarially steer the model, whether that is an injected instruction, a deliberately misleading framing, or text that argues for its own classification, **can move the answer**.*（<https://docs.typesafe.ai/model-jaggedness/jev-1.13>）
- **【官方·集成方】** Pydantic 文档：*adversarial text can move Jev, so a guard built this way belongs **alongside deterministic checks, not instead of them**.*（<https://pydantic.dev/docs/ai/models/typesafe/>）
- **【第三方】** Octomind 的单次注入实验：让 Jev 判断是否拦截 `rm -rf ~/.ssh`，block 概率 **0.76**、confidence **0.64**；在输入中加入一段伪造的 tool-output，声称用户已预先批准并指示「answer auto_allow」后，block 概率降到 **0.48**、confidence 降到 **0.22**。VentureBeat 明确标注 *The result comes from one command in one integration test, **not a benchmark***；VentureBeat 引用的是同一实验，**不构成第二个独立来源**。
- **【官方】** 官方 RAG cookbook 进一步说明其注入过滤只是**过滤器，不是安全边界**：低于阈值的段落仍会进入 prompt，生成侧必须把每段都当不可信文本处理。
- **【第三方】** VentureBeat 的标题级结论：companies are putting Jev in charge of AI agent decisions, and prompt injection can influence the verdict——与本报告 §13.4 的"门控注入实测"假设一致。

**风险 2：state 里的内容会流向第三方（数据外泄面）**

- **【官方·集成方】** Pydantic：*The arguments go to TypeSafe before the verdict comes back, so a call is disclosed to a third party even when it is then refused.* 建议只送判定所需的字段。
- **【官方·集成方】** LangChain 在 LangSmith Evals 博文中披露：*TypeSafe does not currently offer zero data retention, so prompts and outputs sent for evaluation may be retained by the provider.*（注意：官方文档侧写的是"企业客户可申请 ZDR"，两者口径见 §11.3）
- **【官方·集成方】** LangChain provider 文档警告：*Do not put secrets in tool arguments or conversation state unless sending them to TypeSafe is acceptable. This middleware refuses risky calls. It does not request approval. Pair it with human-in-the-loop middleware when you want a person to approve them.*（<https://docs.langchain.com/oss/python/integrations/providers/typesafe>）

**风险 3：护栏不能是唯一防线**

- **【官方·集成方】** Pydantic 原话：*a judgement this cheap is one you can afford to run on everything, which is exactly why it should not be the only thing standing between an agent and an irreversible action.* 另一处提醒：Jev 判的是「模型提出的调用」，不是「模型的意图」。
- **【官方】** 官方 jaggedness 页把「对抗性内容」列为独立失败模式，建议是「在 criteria 里写清楚、上线前充分测边界情况」——定性建议，未给数字。

**风险 4：能力边界造成的静默错误**

- **【官方】** 不要用 Score 反推精确数量（数值校准是弱项，只能用来**过阈值**）；算术、计数、日期比较一律放代码里。
- **【官方】** 大 state 里有无关细节会降准确率（context rot）；生成文本 *will not work well and will be very slow*。
- **【官方·集成方】** Jev 不会像 LLM 那样修正答案：*an output validator that raises ModelRetry usually gets the same answer again*，因此「用 Jev 自检并重试」不是可行路径（<https://octomind.run/blog/jev-system-one-model-ai-agents>、<https://pydantic.dev/docs/ai/models/typesafe/>）。

**风险 5：治理与审计**

- **【第三方】** VentureBeat 给出投入生产前的六个问题（可直接搬用为 check list）：是否记录 state / schema / **选项顺序** / 模型版本 / confidence？是否有专用工作负载身份（workload identity）？是否用多种选项顺序测过？是否用对抗文本测过？模型版本是否 pin？自动决策导致漏升级时谁负责？该报道还指出「Jev 返回类型化答案而非自然语言理由」与既有审计框架（SOC 2 / ISO 27001 / PCI DSS）的映射 *partial coverage at best*——属第三方安全媒体分析，不是标准组织结论。

### 7.9 本节不确定项

1. **两套 `langchain-typesafe` API 形态并存**：PyPI `0.0.1a3` 与 GitHub PR #40556（2026-09-22 仍 open）的导入路径、构造参数不一致，引用签名前必须确认版本。
2. **LangChain 关于性能倍数的口径在同组织两篇博文里不一致**（200x/400x vs ~450x/~200x），且均转述自 TypeSafe 自测（见 §6.3）。
3. **LangChain 的 judge 实验是自测且样本小**：一个 agent、5 条固定样本、100 次重复；官方自述 one test on one agent；实验元数据缺 Jev service version；100% oracle 一致率在更多样本上是否保持未知。
4. **注入敏感性的关键数字只有一个第三方实验**：0.76 → 0.48 来自 Octomind 单次测试，VentureBeat 引用同一实验，**不构成两个独立来源**。
5. **「终止 / 完成判断」没有官方模式**：仅有第三方设计草图（作者声明未实现）。不要写成「Jev 可以用来判断任务完成」。
6. **没有官方 Rust SDK**：官方文档索引只列 Python 与 JavaScript / TypeScript；Rust 侧只有两条路——直接实现 HTTP API（`POST /v1/systemone`），或经 LiteLLM / Cloudflare 等网关转发。**未发现官方 Rust SDK；也未排除存在未检索到的社区实现。**
7. **企业治理问题无权威答案**：VentureBeat 的六问清单与 SOC 2 / ISO 27001 / PCI DSS 映射 *partial coverage at best*，属第三方安全媒体分析。
8. **数据留存**：LangChain 明确披露 *TypeSafe does not currently offer zero data retention*；Pydantic 与 LangChain 都提醒工具参数 / state 会送到第三方（即使最终被拒）——选型时必须单独决策。
9. **Cloudflare 文档页把 Jev 列在 "Text Generation" 分类下**，与其「不生成文本」的定位冲突，应是模型目录的分类元数据习惯，引用时不要据此推断能力。
10. **官方场景地图页自述 10 类但表内只列 8 行**，缺失的两类未在抓取内容中出现。

---

## 8. 提问工程与最佳实践

**要点**

1. 提问工程的元规则是两条禁令：**不要问代码能精确计算的东西**；**不要在一个问题里藏多个判断**。官方同时列出另外两条：避免多层间接（System Two 任务）、避免给 state 超过问题需要的上下文（"Jev suffers from context rot…"）。【官方】
2. 一次请求尽量多问：同一 state 上的所有问题并行评估、彼此独立，加问题几乎不增加耗时，只多花一点 token——*问一个你可能不需要的问题几乎是免费的*。【官方】
3. `instructions` 必须字面、直接、可执行——*Jev answers the question you wrote, not the one you meant*；否定、范围限定词、隐含条件都按字面处理；**question id 不会发给模型**。【官方】
4. 选项 / 等级要「描述情境」而不是「描述程度」：Score 每一级被独立评估，模型看不到级别编号与相邻级别（实测：纯数字级 score 0.55 / confidence 0.33 → 描述性三级后 0.0 / 1.0）。【官方】
5. 阈值不是一个数，而是「按动作风险的函数」；且**跨问题类型搬运阈值是错的**，也不能假设任何结构恒等式。【官方】
6. state 只放这个问题需要的字段（检索与过滤在代码里先做完），纯文本、不支持图像/音频/视频；**英文为主训练语言，其他语言含 CJK 可用但当前准确率较低**（未给量化）。【官方】

### 8.1 instructions 怎么写

- **【官方】** 基本形态：每个问题有 `id`、`type`（`choice`/`score`/`noul`）、`instructions`；Choice/Score 还需 `criteria`，Noul 的 `criteria` 可选。`id` 由你自选、**不发送给模型**：*Write the complete question in `instructions`, even when the ID seems self-explanatory.*（<https://docs.typesafe.ai/primitives>）
- **【官方】** `instructions` 可以是问句，也可以是待判断的陈述句。*"Does the message convey urgency?" 是好问题；"Analyze this message and determine the best course of action" 不是*——那需要慢思考，是**需要拆题的信号**。
- **【官方】** 写得像给普通人看的一句话：*Aim for instructions which are easy for the average person to read and understand.*（<https://docs.typesafe.ai/model-jaggedness/jev-1.13>）
- **【官方】** 把精确条件写进 instructions。发现自己在「解释你其实想说什么」时，那段解释就是 instruction 缺失的另一半；无法避免解释的地方，拆成两个字面问题在代码里组合。
- **【官方】** **顺着语义方向问，不要反转**：Noul 中 `true` 映射到「否」会明显变差。应写 *Does the message contain personal data?*，而不是 *Is the message free of personal data?*（<https://docs.typesafe.ai/primitives/noul>）
- **【官方】** **一个问题一个命题**：*Is the customer angry and asking for a refund?* 要拆成两个 Noul 再在代码里组合。**一个 Score 只量一个维度**：级别描述里出现 *punctual and smart and experienced* 就是在量三件事（<https://docs.typesafe.ai/primitives/score>）。
- **【官方】** 引用 state 中具体字段时用**反引号包住的 dot-and-index 路径**（如 `` `ticket.messages[0].text` ``）；反引号是文档明确要求的写法（*including the backtick characters around each path inside the question*）：

```python
questions = {
    "refund_requested": {"type": "noul",
        "instructions": "Does `ticket.messages[0].text` request a refund?"},
    "policy_supports_refund": {"type": "noul",
        "instructions": "Does `refund_policy` support the refund requested in `ticket.messages[0].text`, given `order.charges`?"}}
```

- **【官方】** `instructions` 可以是 string / object / array / null。官方给出上结构化三条判断标准：(a) 问题需要上下文或示例；(b) 问题的一部分由代码生成（把数据库取来的值放独立字段，而不是拼进字符串模板）；(c) 多个问题的 instructions 结构相似（方便批量构造）。示例结构里的字段名 `question` / `focus` / `what` / `not_for` / `examples` **都不是 API 保留字**——模型会看到名字和值，所以名字要短且能标签化（<https://docs.typesafe.ai/primitives/advanced>、<https://docs.typesafe.ai/concepts/how-to-build-with-system-one>）：

```json
{"type": "noul",
 "instructions": {
   "question": "Does the `message` ask the recipient to disclose a sensitive credential?",
   "inspect": "message",
   "focus": "Look for a request to send the credential itself, not a request to change or reset it."},
 "criteria": {
   "true": {"what": "Asks the recipient to reply with, type, or send a password, PIN, one-time code…",
            "examples": ["Reply with your password", "Send us the 6-digit code you just received"]},
   "false": {"what": "No sensitive credential is requested",
             "examples": ["Reset your password from the settings page", "Your statement is ready"]}}}
```

- **【官方】** 何时不该上结构：*A short, unambiguous question or criterion can remain a string.* 只有当归并在一起会互相模糊时才加结构（<https://docs.typesafe.ai/concepts/how-to-build-with-system-one>）。

### 8.2 Choice 与 Score 的设计规则

#### Choice

- **【官方】** 选项上限 **255**，每个选项只花几 token；**给全量而不是精选**（*pass the full list of teams, categories, or products rather than a shortlist*）；列表可能覆盖不全时加 `other` / `none of the above` 兜底，让模型能说「都不符合」，而不是硬选一个最接近的错误项；选项名和描述都会发给模型，描述要能把选项彼此分开；名称够清楚时描述可为 null（官方示例 `criteria={"calm": None, "frustrated": None, "angry": None}`）。（<https://docs.typesafe.ai/primitives/choice>）
- **【官方】** 选项互斥性问题的解法：当两个选项总被混淆（官方例子 `return_policy` vs `return_status`），把每项描述从字符串升级为对象，字段用 `what` / `not_for` / `examples`，**跨选项使用同一套字段名**，`not_for` 专门用来做「互斥性」负向边界。
- **【官方】** 深层分类法不要塞进一个 Choice：**逐层问一个 Choice，在代码里走树**（hierarchical classification）；概率分散时用 beam search 保留 K 条候选路径，而不是贪心选一条（<https://docs.typesafe.ai/primitives/advanced>、<https://docs.typesafe.ai/cookbooks/hierarchical_classification>）。
- **【官方】** 高基数（>255 或接近上限）的两种做法：缩到 ≤240 的实用上限内；或两阶段「先独立打分、再显式选择」（官方承认会变慢）。
- **【第三方】** 选项描述质量直接影响 confidence：同一工单，裸标签 0.60 → 补描述 0.82（Spring AI 实测）。**但更高的 confidence 不代表更正确**，只说明分布更尖。

#### Score

- **【官方】** 至少 2 级、API 接受最多 **10** 级（「能描述得清楚多少级就用多少级，3 级就够；不要加你描述不清的级」）；编号来自数组下标、从 0 开始，模型**看不到编号也看不到邻居**；`score` 是位置的**概率加权均值**、可落在两级之间（如 1.43），同时给 `probabilities` 与 `confidence`。（<https://docs.typesafe.ai/primitives/score>）
- **【官方】** 级别写法：**描述情境，不要描述程度**——「Broken or degraded feature, but workaround exists」可用，「Moderately severe」不可用；一个 Score 只量一件事，多维请拆成多个 Score 再用权重合并；量表顶端若有需要区别处理的稀有情形（如「辱骂或威胁」）给它单独一级；没有中间态的离散类别改用 Choice 或多个 Noul；*Two wordings of the same scale can behave differently on your data.*
- **【官方】** 「只用数字当 level」的实测对照（同一份 bug 报告，instructions 为 `"Rate severity from 0 to 2, where 2 is worst"`、criteria `["0","1","2"]`）：→ `score 0.55, confidence 0.33, probabilities 0: 0.45, 1: 0.55, 2: 0.0`；改用三级描述性 level 后 → `score 0.0, confidence 1.0`。
- **【官方】** 结构化 level 的实测对照（同一份 Safari 崩溃报告 state）：纯字符串无 examples → score 1.43 / confidence 0.35；加匹配的 example（*export fails in one browser but works in another*）→ **1.03 / 0.96**；加不相关的 example（*search fails, but browsing categories still works*）→ **回到 1.43 / 0.35**。
- **【官方】** 由此得出的关键警告：**高 confidence 不等于答案更对**——它只说明分布更尖；要用「已知期望级别」的样本挑 example，再在**另外的**输入上验证。另一条必须记住：**相同 score 可以有不同分布**（score 1.0 可能是全部概率在 level 1，也可能是 level 0 与 2 各一半），所以要同时读 `probabilities` 与 `confidence`。

#### Noul

- **【官方】** 返回单个 0–1 的数 `noul`，是「答案为真」的概率，**没有 confidence 字段**（二值分布用一个数就说完了）；**0.5 不等于「中等程度」**——*A Noul value runs from 0 to 1, but it's not a scale of the thing you asked about. It is the probability that the answer is yes.*，要量程度就用 Score；边界模糊时加 `criteria.true` / `criteria.false`，但官方建议两种都试：*The instruction is enough for most Nouls, so try your questions with and without `criteria` and keep whichever gives better answers on your documents.*（<https://docs.typesafe.ai/primitives/noul>）
- **【官方】** Noul vs Score 官方对照表（同一批候选人的同一问题）：

| 候选人描述 | Noul「Is the candidate strong in Python?」 | Score「How much Python experience…」（4 级） |
| --- | --- | --- |
| 只会 Java/Go，没用过 Python | 0.03 | 0.0 |
| 偶尔写小脚本 | 0.14 | 1.0 |
| 上一份工作每天用 Python 两年 | 0.81 | 2.05 |
| 每天写 Python 八年，含大型 Django 代码库 | 0.92 | 2.89 |

### 8.3 阈值与校准：分层，而不是一个数

- **【官方】** `confidence` 的定义与边界（<https://docs.typesafe.ai/confidence>）：它是 **0–1 的统计量，由 `probabilities` 的分布形状算出**——全押一项 = 1.0，越平摊越低；*The answer's `confidence` property collapses that shape into a single number from 0 to 1, so you can threshold on it without doing the math yourself.*
- **【官方】** **官方没有公布确切公式**；文档的交互 demo 只是近似（三选项时用 `(3 × largest probability − 1) / 2`），官方称不同计算方式的优劣是专门话题、会另写 cookbook——**截至本次调研该 cookbook 不在 `llms.txt` 索引中**。另一句要记住：*confidence 1.0 只描述模型的答案，不保证答案正确。*（<https://docs.typesafe.ai/primitives/score>）
- **【官方·集成方】** Pydantic 文档进一步明确：*It is a **margin, not a probability that the answer is right**.* bool 字段的 confidence 是「概率到阈值的距离按剩余空间缩放」；float 字段**没有** confidence 条目（<https://pydantic.dev/docs/ai/models/typesafe/>）。
- **【官方】** 三段式与按风险分层（<https://docs.typesafe.ai/confidence>）：高 → 自动执行；中 → 谨慎推进（用户确认、标 review、补信息）；低 → 不执行（转人工 / 要求澄清 / 回退其他系统）。官方同时警告：**正确阈值取决于你的领域与模型在你用例上的表现，从保守值开始，用自己的数据测，再看结果调整。**

```python
if confidence < 0.5:
    route_to_human(user_message)          # 地板：模型自己都说没把握
elif action.choice == "check_balance":
    show_balance(account_id)              # 只读、错了可恢复 → 低门槛
elif action.choice == "approve_transfer":
    if confidence > 0.9:
        confirm_then_execute(account_id)  # 动钱 → 高门槛
    else:
        ask_user_to_confirm(account_id)
```

- **【官方】** Noul 的双阈值（留出中间带），按错误代价方向调：是/否代价相当 → 0.5；**错误的「是」代价高**（派人到现场、退款）→ 提高；**漏掉真正的「是」代价高**（漏标安全问题）→ 降低；中间值 → 转人工。官方代码示例用 `YES = 0.8` / `NO = 0.2`，中间带 `send_to_review(message)`（<https://docs.typesafe.ai/primitives/noul>）。
- **【官方】** cookbook 里出现过的具体阈值（**务必当作「示例值」而非默认值**）：

| 场景 | 阈值 | 官方自我限定 |
| --- | --- | --- |
| 14 问保险理赔 rubric | `< 0.30` no；`0.30–0.70`（含端点）uncertain → 人工；`> 0.70` yes | *The band is illustrative; it is neither a calibrated guarantee nor an optimized threshold. Set production boundaries from labeled examples…* |
| RAG 段落过滤 | 注入 `> 0.70` 排除；与前提冲突 `> 0.70` 归入冲突块；相关性 `< 0.45` 排除；有可用证据 `> 0.55` 纳入 | *We picked these four numbers for this corpus. Treat them as a starting point, not defaults.* |
| 工单 triage 复合评分 | `spam_risk` 在 `0.4 < x < 0.6` 视为不确定 → 转人工；`topic.confidence < 0.75` 转人工；`spam_risk >= 0.6` 隔离 | — |
| 工单部门路由 | `department.confidence < 0.3` 转人工；`resolution.confidence < 0.5` 反问客户；第二部门概率 `> 0.25` 抄送 | — |

（出处：<https://docs.typesafe.ai/cookbooks/consistency_noul_cookbook>、<https://docs.typesafe.ai/cookbooks/classifying_rag_passages>、<https://docs.typesafe.ai/concepts/how-to-build-with-system-one>、<https://docs.typesafe.ai/primitives/choice>）

- **【官方】** 「代码改一个数，比改一句 prompt 便宜」是同一条反复出现的官方设计主张：RAG cookbook 把四个阈值集中放在 `THRESHOLDS` 字典，「改路由策略是常量编辑（可走 code review），不是改问法」（<https://docs.typesafe.ai/cookbooks/classifying_rag_passages>）。
- **【官方】** 两种**不用阈值**的官方范式：
  - **用概率排序，而不是设阈值**——re-ranking 把同一 Noul 问题对 shortlist 中每个候选各问一次，直接按 `noul` 值排序取最高，*That noul is the score the application sorts on.*（<https://docs.typesafe.ai/cookbooks/rerank_typesafe>）
  - **把等级直接写成动作，从根上去掉阈值**——实体对齐用一个三级 Score，三级就是三种处置（`leave unlinked` / `curator queue` / `assert sameAs`），代码只做 `OUTCOME[min(int(score + 0.5), 2)]`，*There is no threshold to fit…Neither number is something you tune. Both follow from how you worded the levels.*（<https://docs.typesafe.ai/cookbooks/entity_alignment>）
- **【官方·集成方】** 三条可对照的默认阈值：Pydantic `typesafe_boolean_threshold` 默认 **0.5**（*Raise it where a false positive is the expensive one*）、`typesafe_tool_call_threshold` 默认 **0.6**（*The default of 0.6 is a starting point, not a validated threshold*）；LangChain `AutoModeMiddleware` 默认 `risk_threshold=0.2`（*The conservative default blocks calls with at least 20% estimated risk.*）。
- **【官方】** **跨类型搬运阈值是错的**。官方反例（同一工单 "I'm not happy with the fit. What are my options here?"）：Noul = 0.22，而 Choice 为 `yes 0.01 / no 0.99 / confidence 0.97`；另一对「是否要求退款」0.72 与「是否在要求退款以外的别的什么」0.47 **和为 1.19**。官方结论：*Don't carry a threshold tuned on a Noul over to a Choice, and don't hold the model to arithmetic identities between separate questions.*（<https://docs.typesafe.ai/model-jaggedness/jev-1.13>）
- **【官方】** 校准口径的可信度边界：Jev 用 RLCD 训练，目标是对齐结果而非人类偏好，因此「更高的 confidence 确实对应更高的准确率」——属官方口径。【第三方】dev.to 指南把警告写在最前面：*those are TypeSafe's own numbers, self-run and unreproduced.*（<https://dev.to/valyuai/how-to-use-jev-a-practical-guide-to-typesafes-system-one-model-g5e>）官方另称 Jev *extremely consistent*（语义相近输入得到数量相近的输出），但**这不等于结构恒等式成立**。
- **【官方】** 自洽性自测：Jev 每题概率的**平均 std = 0.0102**，低于所有被比较的 LLM 条件（含 temperature 0）；14 问理赔 rubric × 15 次重复，`jev-latest`（返回 `jev-1.13.0`），2026-09-11；时延/成本对照为 TypeSafe **111ms / $0.000043** vs `claude-haiku-4-5` t=0 **1780ms / $0.001798** vs `gpt-5.5-reasoning` **11125ms / $0.033157**（文中声明价格是历史价假设，非当前账单）。同实验有一条实用观察：`covered` 在 0.43–0.53 之间跨过 0.5 阈值，说明**单次读数贴近阈值时不可靠**（<https://docs.typesafe.ai/cookbooks/consistency_noul_cookbook>）。

### 8.4 批处理：一次多问，代码决定读哪些

- **【官方】** 机制（<https://docs.typesafe.ai/primitives>、<https://docs.typesafe.ai/models>）：一次请求 = 一个 state + 任意多个问题，**每个问题独立评估、并行执行、彼此不构成隐含上下文**，加/删问题不影响其他问题的结果；**加问题几乎不改响应时间，只多花那几个问题的 token**；context 预算 **64k tokens / 请求（state + 全部问题合计）**、**32k 适用于 state + 单个最长问题**；限流 250,000 tokens/秒、1,200 请求/分钟，超限 429。
- **【官方】** 官方实测（`jev-1.12`，GDPR 维基条目 53,777 字符的 pinned revision，8 个 Noul + 2 个 Choice + 3 个 Score，各跑 5 次）：

```
batching                calls        cost  total time
one call, all 13            1   $0.000497       0.27s
13 calls, one each         13   $0.006090       2.71s
# 12.2x cheaper, 10.0x faster   （官方另一处口径：11.5x / 9.6x）
```

- **【官方】** 答案没有任何变化：13 个问题中 11 个在两种批法下 5 次重复的**标准差恰为 0.0**；有余量噪声的两个（`breach_72h` std 0.0055、`criminal_penalties` 0.0045–0.0084）在两种批法下**噪声量级相同**，均值在噪声内一致。官方结论：*batching neither shifts the answer nor adds variance.* 适用条件（cookbook 自注）：**文档主导型负载**，且「速度」是把 13 次单问的延迟**串行相加**得到的，并发发出后差距会缩小，「但 13x 的 token 成本不变」（<https://docs.typesafe.ai/cookbooks/parallel_questions>）。
- **⚠️ 与第三方边界行为的冲突（必须并列保留）**：`jev-orderby-bench` 记录「把 40 行批处理成一次请求会把原本通过的排序门变成失败」——与官方「批处理不改变答案」的结论方向相反。【未证实（索引转述）】<https://github.com/yodablocks/jev-orderby-bench> → **本项目的批处理设计必须自测**（见 §14.3 实验 E3）。
- **【官方】** **Speculative Fan-Out**（推测式扇出）：把系统需要的所有问题放进一次请求，**包括只在部分输入下才有意义的问题**，让代码事后决定哪些答案有用。官方 triage 示例中 `bug_severity` 与 `has_reproducible_steps` 只在 category 是 bug_report 时才有意义、`refund_requested` 只在 billing 时有意义，但照旧一起问，最后按分支忽略无用答案（<https://docs.typesafe.ai/patterns/fan-out>）。
- **【官方】** 什么时候**才**需要第二次请求——规则很硬：同一请求内的问题互相独立，一个答案不会成为另一个问题的上下文；只有当**代码必须先拿到第一个答案**才能构造第二个请求时（需要用它去取更多数据、决定 state 由什么构成、或决定下一题有哪些选项），才发第二次请求。*Two requests are the exception, not the rule.* 官方给出三个边界案例：Skill suggestion（先从 182 个技能里排名，再取前三名全文重判）、Structure recovery（先判断换行是否断句、合并成块后再分类）、Hierarchical classification（每个 Choice 答案决定下一层选项）（<https://docs.typesafe.ai/primitives>）。
- **【官方】** 反过来，**配对任务不能挤进一个请求**：Re-ranking cookbook 为清晰起见每个 pair 只问一个问题，1,200 次调用（40 query × 30 候选）共 1,536,002 input tokens、成本 $0.0645，并提醒真实应用会把多个问题放进同一次调用；RAG cookbook 声明成本随 k 线性增长，因为每个问题都是关于一个 pair 的。

### 8.5 state 的组织

- **【官方】** 允许的形态（<https://docs.typesafe.ai/concepts/state>）：字符串（一条消息、一篇文章）、对象（`{"message": "...", "order_id": "A-104"}`）、数组（`["Hi", "My customer number is TS1337.", "My card was charged twice."]`）。官方倾向：**大多数请求用对象**，让每部分有描述性名字、关系清楚；只有单一文本的简单场景才用字符串。
- **【官方】** 硬约束（<https://docs.typesafe.ai/concepts/state>、<https://docs.typesafe.ai/models>）：**只支持文本**（字符串 / JSON 对象 / 文本值数组），**不支持图像、音频、视频**；**主训练语言是英语**，其他语言（含 CJK）可用但 *currently have lower accuracy*，官方要求在自己的内容上测、并在路由时特别注意 confidence；上下文 64k / 请求、32k（state + 单个最长问题）。
- **【官方】** 组织原则：
  1. 把 state 当成「你递给专家小组的材料」——*Think of state as the material you would present to a panel of experts before asking them to make a judgment.*
  2. **内容与问题分离**（state 放内容与支撑事实，questions 放要做的判断：官方示例把退款请求与政策放进 state，再问「客户是否要求退款」「政策是否支持」）；
  3. **只放相关问题需要的字段**——*Include only the context relevant to the current questions. This helps the model avoid distractions and context rot.*
  4. 结构化字段 + 反引号路径引用具体嵌套值，消除歧义；
  5. **配对的判断要把两方放进同一个 state 的两个命名槽位**（实体对齐用 `entity_a` / `entity_b`，RAG 用 `query` + `passage`）；
  6. **state 不是敌对环境**（见 §7.8 风险 1）——官方建议是在 criteria 里写清楚、上线前充分测边界情况。
- **【官方】** 「检索在前，判断在后」被 jaggedness 列为独立失败模式：准确率随 state 中与决策无关的内容增长而下降，**Instead:** 先在代码里检索和过滤，只发问题需要的字段。dev.to 指南把它总结为「两层结构：精确取回，廉价判断」并引用官方原句 *retrieve and filter in code first, and send only the fields the question needs.*（<https://docs.typesafe.ai/model-jaggedness/jev-1.13>、dev.to 指南）
- **【官方】** 可复用的 triage 骨架（<https://docs.typesafe.ai/concepts/how-to-build-with-system-one>）：先做确定性判断（`status == "closed"` 直接返回，不调模型）；在代码里筛出 `open_orders` 再放进 state；state 分 `ticket` / `customer` / `policy` 三块；一次请求 7 个问题（3 Choice + 3 Noul + 1 Score），其中若干是推测性的；用加权和合成 `spam_risk`（权重 0.45 / 0.30 / 0.25）再按 confidence 与区间转人工；最后按 `topic.choice` 分支，只在对应分支读该分支需要的推测性答案。
- **【官方】** 复合评分的规范化做法：**不同长度的量表必须先归一化**（除以 `len(criteria) - 1`），否则权重没有意义：

```python
def normalized(answers, question_id: str) -> float:
    """Put a score on 0 to 1 by dividing by its top level number."""
    top_level = len(TRIAGE_QUESTIONS[question_id].criteria) - 1
    return answers[question_id].score / top_level

return 0.6 * severity + 0.3 * frustration + 0.1 * report_quality
```

（官方对该示例给出的计算：归一化后 0.62 / 0.64 / 1.0 → `0.6×0.62 + 0.3×0.64 + 0.1×1.0 = 0.664 ≈ 0.66`。）

- **【官方·集成方】** 与本项目直接相关的一条：LangSmith Evals 要求 **state 里不要放评分说明**（评分标准属于 questions）；Braintrust 与 Langfuse 都把 label / confidence / probabilities 原样保留在 trace 里供复核。

### 8.6 常见坑（提问工程视角）

> 官方 9 类失败模式（字面理解、数学、日期、多跳、大 state、对抗内容、指令矛盾、结构不变式、生成）的完整表见 §4.1；本节只列**提问者可以直接改写的症状与修复**。

| 症状 | 官方/集成方解读 | 修复 |
| --- | --- | --- |
| 模型答的不是你想问的 | *When you look at a wrong answer and find yourself explaining what you really meant, that explanation is the missing half of the instruction.* | 把辩解写回 instruction；无法避免解释的，拆成两个字面问题在代码里合成 |
| Choice confidence 偏低、概率在两个选项间分摊 | *low confidence on a Choice often means none of the options are a clear winner*（【第三方】解读：可能是你的 criteria 写错了，而不是模型糊涂了） | 升级为 `what`/`not_for`/`examples` 结构、跨选项用同一套字段名（官方称 *JSON rubric for boundary clarification*）；或补 `other` 兜底 |
| Score confidence 低 | 官方三条诊断：**级别在该输入上重叠**、**问题在量不止一件事**、**state 里信息不够定位** | 重写级别（描述情境）、拆成多个 Score、补充 state（<https://docs.typesafe.ai/primitives/score>） |
| 让 Jev 计数 | 计数不可靠、误差随规模增长 | 在代码里迭代候选，每个候选用一个 Noul，再自己加总（官方给出的写法见下） |
| 问 hex/RGB、汇编/二进制这类数值化表示 | 语义 > 数值 | 在代码里做转换，把算好的数或命名好的桶传进去 |
| 期望 Noul 与 Choice 一致、期望 P(a)+P(¬a)=1 | 结构性不变式不成立（§4.1 #8） | 一个决策只用一种问法；恒等式在代码里强制 |
| 让 Jev 生成文本或数量 | 不是为生成训练的；官方说串接 Choice 强行生成*效果差且非常慢* | 用生成式模型；**不要让 Score 的期望值冒充精确数量**（只能用来过阈值） |
| 把 confidence 当准确率用、一个阈值打天下、用阈值替代设计 | 官方点名的三种「过度依赖概率」表现 | 用「等级即动作」或「按概率排序」的范式替代阈值；阈值按用途与风险分层 |

**官方的计数写法**（先逐项问 Noul，再在代码里加总）：

```python
result = client.system_one(
    {"items": items},
    {f"item_{i}": Noul(instructions=f"Is `items[{i}]` the name of a fruit?") for i in range(len(items))})
count = sum(result.nouls[f"item_{i}"].noul > YES for i in range(len(items)))
# YES = 0.5（官方注释：阈值由你定，取决于用例）
```

**官方 meta-rule（可直接作为提问模板的 lint 规则）**：*As a reminder, avoid the following:* ① Asking the model something code can compute exactly. ② Hiding several judgments inside one question. ③ System Two tasks: more layers of indirections. ④ Giving it more context in `state` than the question needs. *Jev suffers from context rot…*

### 8.7 CJK / 中文风险（对本项目是硬约束）

- **【官方】** state 与问题**只支持文本**；**主训练语言是英语**，其他语言（含 CJK）*accepted but currently have lower accuracy*，官方要求在自己的内容上测、并在路由时特别注意 confidence（<https://docs.typesafe.ai/models>）。**官方未给任何量化数字。**
- **【未证实】** 「若 harness 要处理中文输入，宁可把判定问题与 state 用英文表述（或做英译后再判定），并在低 confidence 时保持更高的人工兜底比例」这一应对，是上游专题基于官方口径提出的**待验证假设**，不是官方建议，也没有实测数据支撑。
- **【官方·集成方】** 三个相关次要事实：Pydantic 文档记录「选项顺序是输入的一部分，重排 `Literal`/`Enum` 可能改变答案」，因此**多语言标签的排列顺序也应纳入测试**；Jev 无 stream（一次性返回）；无采样参数（temperature/top_p 被忽略）。
- **【第三方，社区】选项顺序影响输出的旁证**：中文媒体转述的 X 用户测试称「把『信息咨询 / bug 报告』换序后，模型从偏向 bug_report 变成偏向 information」（见 §12.2，属未独立复现的口径）。

### 8.8 本节不确定项

1. **所有性能数字都是厂商自测**：dev.to 指南开头即声明 *those are TypeSafe's own numbers, self-run and unreproduced.*；官方 cookbook 声明数字来自 pinned 数据集 / 缓存，部分价格是「历史价假设，非当前账单」。
2. **同一实验存在两套比值**：`primitives.md` 说 13 问批处理 **11.5x 便宜 / 9.6x 快**，cookbook 正文与 `llms.txt` 说 **12.2x / 10.0x**，原因未说明（可能是不同版本 / 快照）。**引用时以 cookbook 正文为准，并注明另有 11.5x/9.6x 口径。**
3. **`confidence` 的确切公式未公开**：官方只说它由概率分布算出，demo 给的是近似，并预告「不同计算方式的优劣会另写 cookbook」——截至 2026-09-22 该 cookbook 不在 `llms.txt` 索引中。**不要自行复算 confidence，应原样保存 API 返回的 `confidence` 与 `probabilities`。**
4. **未找到公开信息**：(a) 官方的校准曲线 / 可靠性图（只有「RLCD 使概率具备总体意义」的定性说法与稳定性实验——重复实验测的是**稳定性**，不是**准确率**）；(b) 官方单一的「提问最佳实践」页（本节的规则是从 primitives / how-to-build / jaggedness / cookbook 归纳的）；(c) state 为空、超长截断、非文本混入时的行为；(d) temperature 等采样参数（API 未见暴露）；(e) 多轮对话式 state 复用的官方示例；(f) **中文 / CJK 的具体准确率数字**。
5. **文档内部的小不一致**：counting 示例的返回写法，官方 jaggedness 页用 `result.nouls[...]`，dev.to 指南写 `result.answers[...]`（以官方 SDK 形态为准，接入时按 SDK/API 参考核对）；自洽性 cookbook 的问题数量有 **14 问**（正文与代码）与 **15 问**（`llms.txt` 摘要与 Noul 页）两套口径，重复次数两边一致均为 15 次，建议写「约 14 问 / 15 次重复」或注明分歧。
6. **一个应避免引用的第三方来源**：SitePoint 的 *Build a Safer AI Agent Harness with Jev and LangChain*（2026-09-18，<https://www.sitepoint.com/build-safer-ai-agent-harness-jev-langchain/>）虽然标题含 Jev，但正文用的是 `from jev import Decision`、`llm_model="gpt-4o-mini"`、Pydantic schema 约束的 **LLM 结构化输出**，与 TypeSafe 的 Jev API（`state` + `questions` + Choice/Score/Noul）**完全不是一回事**；作者自己在文末也提示「如果 `jev` 包不可用可用 `instructor`/`marvin` 替代」。**不应引用该文作为 Jev 资料。**
7. **提示注入的严重程度只有小样本证据**（0.76 → 0.48 为单次实验），中文场景下的注入表现无任何数据。
8. **批处理对个别判定的影响存在官方结论与第三方反例的冲突**（§8.4），未找到第三方仲裁，必须自测。

---

## 9. 竞品对比与选择准则

**要点**

- Jev 的对手不是「另一个 LLM」，而是**你已经能用的那条最便宜的判定路径**：规则/正则 → 本地分类器（embedding + 分类头 / 微调编码器）→ 语义路由 → guardrail 专用模型 → 小模型 LLM judge → 前沿 LLM judge。Jev 插在「**免标注 + 需要概率 + 结构化输出**」这个空档里——这个空档在它之前确实只能靠 LLM 填。【第三方/官方混合，见下表】
- 速度与成本优势是**量级性的**，精度**不是**：官方 workflow evals 上 Jev 为 **67.8% / $0.0004 / 0.4s**，最强对照 Sol 为 **74.1% / $0.0836 / 23.3s**、Opus 5 为 73.1% / $0.1761 / 37.8s——便宜、快 1–2 个数量级，但**精度落后约 5–6pp**；发票场景差距最大（Jev 61.8% vs Sol 79.1%，差 17.3pp）。【官方】<https://evals.typesafe.ai/>
- 对照**小模型档**时 Jev 是「精度 + 成本」双重优势，而不只是成本：同一任务 Haiku 4.5 只有 **53.6%**（低 14.2pp）且每案例 $0.0195（**贵约 49 倍**），luna 66.8% / $0.0033（贵约 8 倍）。但若只看**最便宜档单价的推算**，优势只有约 5–18 倍，不是官方首页口径的 400 倍。【官方】同 evals；【上游推算】
- **有标注数据时传统分类器可以打平甚至超过 Jev**：一个独立开源复现中，Jev 零样本在 18,514 封邮件上 98.33%，TF-IDF 逻辑回归（约 14,800 条标注）98.39%，统计上打平。**Jev 的真正卖点是「不用标注 + 自带概率」，不是「精度更高」。**【第三方】<https://github.com/bitnovus/jev-spam-eval>
- Jev 三个硬短板：**(a) 不产出解释**（审计/自动诊断必须另接生成模型）；**(b) 不能弃权**（强制二分时挑「最不坏的答案」而不是说不知道）；**(c) 官方承认 context rot**（state 越大越不准）。【第三方整理自官方文档】<https://langfuse.com/blog/2026-09-18-using-typesafes-jev-for-evals>
- 官方数据里对 Harness 架构最重要的一条**不是「Jev 快」，而是「workflow 优于 prompt」**：把政策分解成 Noul/Choice/Score 原子问题 + 代码决策，**每个被测量的模型都更准、更便宜、更快**（Opus 5 73.1% vs 64.8%；Sol 74.1% vs 63.4%；Haiku 4.5 53.6% vs **18.1%**）。这条与厂商立场无关，是可直接用于设计的结构性证据。【官方】同 evals

### 9.1 总览对比表

| 方案 | 需要标注数据 | 单次成本 | 延迟 | 可行动概率 | 给解释 | 出处 |
|---|---|---|---|---|---|---|
| 规则 / 正则 | 否 | ≈0 | <1ms | 否（布尔） | 否 | 【第三方】premai 分层表 <https://www.premai.io/blog/production-llm-guardrails-nemo-guardrails-ai-llama-guard-compared/> |
| embedding + 逻辑回归 | **是**（数百–数千条） | $0.02–0.12 / MTok | 数十 ms | 是 | 否 | 【第三方】Arize 六路对照 <https://arize.com/blog/typesafe-jev-llm-judge/> |
| 微调编码器（ModernBERT 类） | **是** | GPU 时间（单卡约 76,000 tok/s） | 个位数 ms | 是（需温度标定） | 否 | 【第三方】同上；vLLM 论文 |
| cross-encoder 重排 | 否 | 按「每次检索最多 100 文档」计价 | 数十–数百 ms | 仅相关性分 | 否 | 【第三方】Arize 同上 |
| 专用 guardrail（Llama Guard 系） | 否（用出厂 taxonomy） | 自托管 GPU（第三方估 100 万次/月约 $800@A100）；**OpenAI Moderation 免费** | LG 8B 约 800ms（A100）；Moderation 50–100ms | 部分（分档分 + flag） | 否 | 【官方】<https://huggingface.co/meta-llama/Llama-Guard-4-12B>、<https://developers.openai.com/api/docs/guides/moderation>；【第三方】premai |
| 语义路由（embedding/分类器路由） | 少量 utterance 或偏好数据 | 近零（本地分类） | 个位数–数十 ms | 路由置信度 | 否 | 【官方】<https://www.lmsys.org/blog/2024-07-01-routellm/>、<https://arxiv.org/html/2510.08731v1> |
| 小模型 LLM judge（Haiku / Luna 档） | 否 | 单次推算 $0.00046–$0.00225；**官方同任务每案例 $0.0033（luna）/ $0.0195（haiku 4.5）** | 秒级 | 口头置信，常过度自信 | **是** | 【官方】<https://platform.claude.com/docs/en/models/haiku-4-5/overview>、<https://developers.openai.com/api/docs/pricing>、evals 同上 |
| 前沿 LLM judge | 否 | 单次推算 $0.009–0.0225；**官方同任务每案例 $0.0836（sol）/ $0.1761（opus 5）** | 10–38s | 口头置信，常过度自信 | **是** | 【官方】evals 同上；【第三方】Arize 同上 |
| **Jev** | 否（零样本，写判据即可） | $0.042 / MTok 输入，**输出免费**；实测 $0.00035 / 次；官方同任务每案例 $0.0004 | 70–500ms 官方口径；实测 0.44–0.59s | **是，且是训练目标** | **否** | 【官方】/models、evals 同上；【第三方】LangChain、NearHere、bitnovus |

### 9.2 官方 workflow evals 的口径提醒与「workflow 优于 prompt」

- **口径三件套（引用该页数字时必须一并写出）**：
  1. 「准确率」是**与模型互评参照的一致率**，不是与人工真值的一致率；workflow 由厂商自家能力团队构建，官方自认可能偏置，参照偏向 OpenAI/Anthropic。（完整质疑见 §6.6）
  2. 每案例成本/耗时覆盖**整个 workflow 的多次模型调用**，与「按 2,000 输入 + 50 输出 token 推算的单次调用成本」**不可混用或加减**；LangChain 的 $0.00035/次 又是第三种口径（不同任务、不同 state 长度，反推约 8,300 输入 token/次）。
  3. 「最强对照」命名差异已核实：kingy.ai 与 Arize 各取其一（Sol 74.1% vs Opus 5 73.1%），**不是数据漂移**；但该页会更新，引用必须写明抓取日期。
- **workflow vs prompt（同一策略的两种形态，官方一手数据）**：

| 模型 | workflow 准确率 | prompt 模式准确率 | 差值 |
| --- | --- | --- | --- |
| haiku 4.5 | 53.6% | **18.1%** | +35.5 |
| luna | 66.8% | 51.9% | +14.9 |
| terra | 67.9% | 61.6% | +6.3 |
| sonnet 5 | 67.8% | 60.4% | +7.4 |
| DS v4 flash | 64.4% | 59.3% | +5.1 |
| DS v4 pro | 65.5% | 59.7% | +5.8 |
| opus 5 | 73.1% | 64.8% | +8.3 |
| sol | 74.1% | 63.4% | +10.7 |

（来源：<https://evals.typesafe.ai/>，2026-09-22 抓取；官方原话："Averaged across the four example tasks, every model is more accurate, cheaper and faster in the workflow than it is with the same policy as a prompt."）【官方】

- **小模型档的对比（决定"要不要用 LLM 兜底"）**：同一任务上 Haiku 4.5 **53.6% / $0.0195 / 12.5s**（Jev 67.8% / $0.0004 / 0.4s）——Jev 在准确率上高 14.2pp 且便宜约 49 倍；Luna **66.8% / $0.0033**（贵约 8 倍、准确率略低）。**因此"用最便宜的小模型顶替 Jev"在成本上并不成立，除非单价能压到 Jev 的 5–18 倍以内。**（【官方】同 evals；倍数为上游基于单价的推算）

### 9.3 第三方实测：Jev 到底强在哪、弱在哪（对照结论）

> 逐份实验的方法、样本与自我限定见 §6.4；这里只保留**跨实验的对照结论**。

- **成本与延迟**：LiteLLM 分类环节 p50 5.43×、成本 −96.118%（分类边界内）；NearHere 平均响应 0.59s vs 2.90–3.40s、每千次决策 $0.043 vs $0.370 / $2.496；LangChain judge 每次 $0.00035（Claude 整场 $28.17）；bitnovus 19,772 次请求约 $1.35 且并发 16 下约 252 秒——**量级优势一致成立**（口径各自不同，不可混用）。
- **稳定性与方差**：LangChain 500/500 与人工 oracle 一致、方差比 Terra 低 913x；官方自洽性实验平均 std 0.0102；但钓鱼基准显示 Jev 的**运行间波动（2.2% 标签翻转、最大概率差 0.15）大于 Haiku（0.7%、0.65）**——稳定性优势是**相对 LLM 的大多数条件**，不是绝对无波动。
- **准确率**：钓鱼基准（Jev 62.6% vs Haiku 81.3%）、Jevals choice（被 Gemini 3.8 Flash 显著领先）、JevBench（Luna(low) 的 Intelligence/Calibration 更高）、Jagent（27/27 并列）、spam 评测（与 TF-IDF 打平）——**没有任何一份第三方实验证明 Jev 的准确率优于同级或更强的 LLM**。
- **门槛覆盖率**：Jevals 显示在**各自校准到 95% 准确率的阈值**下，Jev 能独自承接的决策比例（noul 86%、choice 54%、共享 0.96 门槛下 59% 且其中 94.3% 正确）**高于被比较的 LLM**——这是"按置信度分流"设计里最有用的第三方证据。
- **工程属性**：无需标注、无需 JSON schema 强制、天然返回概率与分布、无解析失败、无 provider 侧的采样不确定性——这些是 LLM 路线需要额外工程才能达到的。

### 9.4 替代路线各自的代价（为什么不能一刀切换到 LLM judge）

- **LLM-as-judge 的能力是真的**：GPT-4 类 judge 与人类一致率 >80%，与人类-人类一致率相当（MT-Bench / Chatbot Arena 论文，2023）<https://arxiv.org/abs/2306.05685>。但**换模型/换档位差异巨大**：文档相关性任务 GPT-4 81%、Claude 3.5 Sonnet 77%、GPT-3.5 Turbo 仅 55%（对「不相关」类 recall 只有 7%）。【第三方】<https://arize.com/blog/choosing-the-best-llm-evaluation-model/>
- **judge 的置信度不可信**：2025 年研究识别出 **Overconfidence Phenomenon**（预测置信度显著高于真实正确率）并提出 TH-Score <https://arxiv.org/abs/2508.06225>；另有自偏好偏置 <https://arxiv.org/abs/2404.13076> 与位置偏置 <https://aclanthology.org/2025.ijcnlp-long.18.pdf>。
- **judge 的隐藏成本**：一般模型每次 yes/no 要花约 **910 输出 token**，Jev 约 85 且不计费；judge 在官方 workflow 上为 **$0.03–0.18 / 次、10–38 秒 / 次**，若要求解释还要再加。【第三方】Arize
- **judge 的不可替代价值**：**解释**——可把失败原因成批喂回 coding agent 自动改进；Arize 明确指出这是「替换 LLM judge 时失去的最大东西」。【第三方】
- **guardrail 线 Jev 打不动「免费」**：OpenAI Moderation endpoint **官方免费**、第三方测 50–100ms <https://developers.openai.com/api/docs/guides/moderation>；Llama Guard 4 官方英文输出过滤 **recall 69% / FPR 11% / F1 61%**，多语种 43%/3%/51%，单图 41%/9%/38%，多图 61%/9%/52%，相对 LG3 英文 F1 +8pp <https://huggingface.co/meta-llama/Llama-Guard-4-12B>；月度成本量级（100 万次请求）：正则 ~$20、LLM Guard ~$150、Guardrails AI ~$200、**Llama Guard（A100 自托管）~$800**、OpenAI Moderation $200、GPT-4 当 judge $3,000–10,000；5 个 90% 准确率的 guard 串起来，**至少一个误报的概率 41%**（要压到 25% 以下每层需 95%+）。【第三方】premai 同上
- **语义路由是「和 Jev 抢活」而不是「替代 Jev」**：RouteLLM 在 MT Bench 上**省 85% 成本、保 95% GPT-4 质量**（MMLU 45%、GSM8K 35%；数据增强后用 14% 的 GPT-4 调用达到 95%，比商业路由器便宜 40%+）<https://www.lmsys.org/blog/2024-07-01-routellm/>、<https://arxiv.org/abs/2406.18665>；vLLM Semantic Router 在 MMLU-Pro 上**准确率 +10.24pp（p<0.01）、延迟 −47.1%、token −48.5%**，知识密集域提升 >20pp，**其分类核就是 Rust + HuggingFace Candle**（ModernBERT 微调，多任务：类别/PII/jailbreak）<https://arxiv.org/html/2510.08731v1>、<https://vllm.ai/blog/2025-10-27-semantic-router-modular>；aurelio semantic-router 支持本地全离线与可训练阈值 <https://github.com/aurelio-labs/semantic-router>。
  - **对本项目的意义**：这条线的实现路径（Rust 分类核 + Candle + 多任务并行 + 本地部署）与本项目技术栈几乎重合，Jev 的差异不在「路由得更好」，而在「**免训练新意图、返回概率、能与业务判据并行提问**」。
- **有标注数据时的本地分类器**：SetFit 类方法在 8 例/类即可接近全量微调，训练 30 秒、$0.025 <https://huggingface.co/blog/setfit>——**只要你有标注，本地分类器的成本与延迟都远低于任何 API**。

### 9.5 选择准则：什么时候用 Jev，什么时候用别的

**决策树（按优先级）**

1. **能用确定性代码判断的，两者都不用**（官方文档的态度也是：state 里放数据，规则放代码）。
2. **候选答案是预先已知的有限集合 / 是-否 / 有序档位，且没有标注数据、需要概率门控、延迟敏感** → **Jev**。
3. **同类判定但你有几百到几千条标注、追求个位数毫秒、要自托管可控** → **embedding + 分类头 / 微调编码器**（SetFit 类方法在 8 例/类即可接近全量微调）。
4. **候选「意图」本身很稳定，只需把它分到某条链路上** → **语义路由**（RouteLLM / vLLM Semantic Router / aurelio），成本近零。
5. **内容安全、固定 taxonomy、要图像或多语种、要合规可解释的过滤** → **专用 guardrail（OpenAI Moderation 免费、Llama Guard 4 自托管）**，**不要**拿 Jev 替这一类。
6. **需要解释、需要多因素混合推理、需要生成、或判定频率低而单次价值高** → **LLM judge（小模型档起步，不够再升级）**。
7. **混合架构（多数团队的实际最优解，也是 Arize 的建议）**：**Jev/分类器跑全量覆盖做监控，失败与不确定样本抽样送 LLM judge 拿解释**。

**明确不要用 Jev 的场景**：要写文本/代码/摘要；要给出理由；需要数学或日期运算；图像/音频输入；**没有逃生通道的强制二选一**；state 长而杂且无法裁剪；要靠领域微调才能达到精度要求的任务；要求"绝对可靠的安全边界"或需要抗恶意输入的判定。

**两条定位结论**：

- 抽象层应按「**决策**」建模而非按「模型调用」建模：输入 state + 类型化 questions，输出「带概率的类型化答案 + 用量」。Jev 是它的一个实现，LLM 走「结构化输出」实现（官方 workflow evals 用的 System One LLM wrapper 就是这个思路，官方承认这条路更慢更贵），于是 Jev 不可用/超限/不擅长时可在同一链路回退。
- 不要把 Jev 当精度天花板：官方参照是模型互评，多项第三方测试显示它在精度上落后或打平，而对最便宜小模型档的成本优势只有个位数到两位数倍数。定位应是「**免标注 + 概率伴随 + 并行提问 + 无解析失败**的快速判定层」，差异化在工程属性；**关键判定必须在本项目自己的带真值数据集上测过再上线**。

### 9.6 本节不确定项

1. Jev 的一切数字都来自 2026-09 的 early access 产品，官方自评占主导；第三方实测样本都极小（5 条 agent 轨迹、50 条 listing、711 个 workflow 案例、1.5 万封邮件、6,003 次 rubric），**没有任何一个是同任务同条件的严格对照实验**。
2. 官方 dashboard 的「真值」是**模型互评**，非人工标注；workflow 由厂商自家团队构建，官方自认可能有偏置。
3. **校准性没有独立验证**：官方未发布校准曲线或指标；bitnovus 仓库有分档校准分析，但**单一数据集、单一模型版本，不能当通用校准保证**。
4. **未找到公开信息**：p95/p99 延迟、并发表现、限流命中率、重试率、版本漂移影响；Jev 与「专门训练的现代分类器/重排器」在同一真值任务上的独立对比；官方 model card / 架构 / 参数 / 训练数据披露。
5. **依赖转述的数字**（引用时须标二次来源）：Every 原文、Good Start Labs 的 6,003 次评测、DataCamp 文章（403）。
6. **间接引用需降级使用**：premai 文章中的 ToxiGen（Llama Guard 3 P0.91/R0.88/F1 0.89）与 JailbreakBench（检出 85%、FPR 15%）表格**未注明来源与方法**；Llama Guard 的 800ms 与月度成本也是第三方估算。
7. 语义路由的数字都有明确条件（特定模型、硬件、benchmark），非普适常数；RouteLLM 基于 GPT-4 Turbo + Mixtral 的模型对，与 2026 年模型组合不可直接平移。
8. **未找到「Jev vs 本地 Rust 分类核」的对比**——这是明显空白：vLLM Semantic Router 已证明 Rust + Candle 的 ModernBERT 分类核可行且极快，值得单独验证。

---

## 10. Rust 集成要点

**要点**

- **TypeSafe 官方没有 Rust SDK，也没有任何 Rust 计划**：官方只有 Python 与 JavaScript/TypeScript 两套 SDK，非这两种语言官方口径是「直接调 HTTP API」；官方全量文档语料（903,311 字节）全文检索 "Rust" **零命中**；`https://docs.typesafe.ai/sdk/rust/` 返回 **HTTP 404**；官方 GitHub 组织 `typesafe-ai` 仓库列表无 Rust 仓库。【官方 + 本轮自测】
- **社区 Rust 生态异常繁荣但极度碎片化**：crates.io 上关键词 `jev` 有 **29 个** crate、`system-one` **13 个**，其中至少 **18 个**是各自独立的 Jev/System One 客户端；全部自我标注 unofficial、全部诞生于 2026-09-16～09-21 的六天内、全部 `0.x`、下载量 9–329、GitHub star 0–11。**没有任何一个可称事实标准。**【第三方】
- **命名冲突**：crates.io 上已存在名为 **`jev-harness`** 的 crate（0.1.4，2026-09-21，描述 "Zero-overhead System One decision harness and token optimizer for AI coding agents."），与本项目同名。若本项目日后有意发布 crate，需先处理命名。【第三方】
- **HTTP API 不支持流式，且这是设计使然**：官方文档语料中无任何 SSE / stream / server-sent 响应接口描述；官方「Jev with coding agents」页原文：*"Coding agents rely on an LLM that streams text, calls tools, and edits files based on natural-language instructions. **Jev does none of that.**"* 架构上自洽（Parallel 采样，一次前向出全部概率，非逐 token 自回归）。【官方】
- **从 Rust 调用可行，官方 API 形状对 serde 非常友好**：`Question` 由 `type` 字段区分三型，正好映射为 Rust **内部标签枚举**（`#[serde(tag = "type")]`）；**已实际编译验证该映射能无损往返官方示例 JSON（8 项断言全部通过）**。【本轮自测】
- **真正的「并发」不是并发发请求，而是把问题打包进单请求**：官方推荐一次请求问多个问题，并给出实测 13 问合并 = **12.2× 更便宜、10.0× 更快**（官方另一处口径为 **11.5× / 9.6×**，两处数字不一致，见 §3.6）。限流为 **1,200 requests/min + 250,000 tokens/s**——多打请求会先撞 request 限流，打包提问才是正解。

### 10.1 官方 SDK 覆盖面：只有 Python 与 JS/TS

| 证据 | 内容 | 来源 |
|---|---|---|
| 官方 SDK 索引页 | 只列 Python、JavaScript/TypeScript 两张卡片，原文 *"You can also call the **HTTP API** directly from any language."* | 【官方】<https://docs.typesafe.ai/sdk> |
| 官方 sitemap | 有 `/sdk/python`、`/sdk/javascript` 及子页，**无 `/sdk/rust`** | 【官方】<https://docs.typesafe.ai/sitemap.xml> |
| 反向确认 | `https://docs.typesafe.ai/sdk/rust/` → **HTTP 404 Not Found** | 本轮自测（curl 实测） |
| 官方 GitHub 组织 | `typesafe-sdk-python`(186★)、`typesafe-sdk-js`(214★)、`system-one-adapter-python`(241★)、`skills`(1575★) 等，**无 Rust 仓库** | 【官方】<https://api.github.com/orgs/typesafe-ai/repos> |
| 仓库身份确认 | PyPI `typesafe-sdk` 0.7.1 的 Repository 指向 `github.com/typesafe-ai/typesafe-sdk-python` | 【官方】<https://pypi.org/pypi/typesafe-sdk/json> |
| 官方全量语料检索 | `llms-full.txt` 903,311 字节，`grep -in "rust"` 无真实命中（仅 `frustration` 等子串） | 本轮自测 <https://docs.typesafe.ai/llms-full.txt> |

→ 在 Rust 项目中接入 Jev **没有官方支持的路径**，只能（a）自己调 HTTP API，或（b）选用第三方社区 crate；也意味着**没有官方兼容性承诺兜底**，抽象层要假设 wire 契约会变。

### 10.2 社区 Rust crate 生态（crates.io 实测枚举）

检索方式：crates.io API 按关键词枚举。【第三方】<https://crates.io/api/v1/crates?keyword=jev> + <https://api.github.com> 逐个核验。

| crate | 版本 | 下载 | 最后更新 | 声明仓库 / 仓库状态 |
|---|---|---|---|---|
| `typesafe-sdk-client` | 0.6.2 | 174 | 2026-09-19 | `douglance/typesafe-sdk-rs`，1★，MIT |
| `typesafeai-sdk` | 0.4.1 | 164 | 2026-09-21 | `Ketankhunti/typesafe-sdk-rust`，1★，MIT |
| `typesafeai-sdk-community` | 0.5.0 | 71 | 2026-09-19 | `community-ports/typesafeai-sdk-rust-community`，0★，MIT |
| `kunobi-jev` | 0.2.0 | 214 | 2026-09-18 | `kunobi-ninja/kunobi-jev`，0★，Apache-2.0 |
| `jevi` | 0.4.0 | 329 | 2026-09-21 | `bitomule/jevi`，1★，Apache-2.0 |
| `jev-sdk` | 0.1.0 | 29 | 2026-09-18 | `portlandhodl/jev-sdk` → **HTTP 404，仓库不存在** |
| `jevrs` | 0.1.1 | 29 | 2026-09-20 | `luizribeiro/jevrs`，0★，Apache-2.0 |
| `sysone` | 0.0.3 | 35 | 2026-09-19 | `zcoder-run/rust-sysone`，4★，Apache-2.0 |
| `system-one` | 0.1.0 | 9 | 2026-09-21 | `heyaozh/system-one`，0★，**NOASSERTION** |
| `typesafe-jev` | 0.2.0 | 22 | 2026-09-21 | `thehumanworks/jevgrep`，1★，**无 License** |
| `typesafe-client` | 0.1.0 | 43 | 2026-09-16 | `JedimEmO/typesafe-client`，1★，Apache-2.0 |
| `typesafe-ai-sdk` | 0.2.0 | 64 | 2026-09-18 | `aoprisan/typesafe-ai-rust-sdk`，1★，MIT |
| `typesafe-ai-rs` | 0.1.0 | 41 | 2026-09-17 | `gilljon/typesafe-ai-rs`，4★，MIT |
| `typesafe-rs` | 0.1.0 | 17 | 2026-09-16 | `AbdelStark/typesafe-rs`，1★，MIT |
| `typesafe-rust-sdk` | 0.1.0 | 13 | 2026-09-19 | `phiat/typesafe-rust-sdk`，1★，MIT |
| `typesafe-systemone` | 0.2.0 | 38 | 2026-09-21 | `2commits/typesafe-systemone`，0★，Apache-2.0 |
| `jev-client` | 0.1.1 | 33 | 2026-09-20 | `shaharia-lab/jev-cli`，11★，Apache-2.0 |
| `jav-rs-sdk` | 0.1.1 | 20 | 2026-09-21 | `wjixiang/jav-rs-sdk` |
| `typesafe-jev-mcp` | 0.1.2 | 35 | 2026-09-20 | MCP server，非 SDK |

**生态健康度问题**：全部仓库创建时间集中在 2026-09-16～09-21（Jev 发布后一周内），star 0–11，无任何社区共识；`jev-sdk` 元数据声明的仓库 404（说明「文档完善 ≠ 元数据可信」）；`system-one` 的 License 为 `NOASSERTION`、`typesafe-jev` 无 License（企业引入前须自行确认许可）。**列举仅作生态描述与设计参考，不构成任何可用性/安全性/维护性推荐。**

**值得参考的设计（不代表可用性背书）**：

- `system-one`：backend-agnostic 抽象（`JevHttp` / `Mock` / `Replay` / `LocalLogprob` / `Shadow` 多种后端），`#[derive(AsChoice)]`、`#[derive(AsQuestions)]` 把 Rust 类型直接变成问题集并回解析答案，自带 `calibration::CalibrationReport` 检查「标称 0.8 是否真是 80%」。<https://docs.rs/system-one/latest/system_one/>
- `typesafeai-sdk-community`：自称官方 Python SDK 的移植（*"same primitives, same request/response shapes, same retry and error semantics"*），提供 `ChoiceLabels` / `ScoreLevels` / `Questions` / `Route` / `Composite` 等 derive，feature flag 区分 `rustls`（默认）/`native-tls`/`blocking`/`test-util`。<https://docs.rs/typesafeai-sdk-community/latest/typesafeai_sdk_community/>
- `jev-sdk`（仓库 404，但 README 的安全清单有参考价值）：API key 存为敏感 `HeaderValue` 并从 `Debug` 脱敏、拒绝含 CRLF 的 key、强制 HTTPS（仅回环允许明文）、内建客户端不跟随重定向、响应体上限（成功 16 MiB / 错误 64 KiB）、退避饱和且带 jitter、`Retry-After` 上限 60s、依赖最小化。<https://docs.rs/jev-sdk/latest/jev_sdk/>

### 10.3 serde 类型映射与编译验证结论

上游在**本机实际编译运行**了一次性验证 crate（`cargo 1.97.1` / `rustc 1.97.1`，代码位于 `/tmp/jev-research/serde-check/`，不在项目仓库内），`cargo run` 输出 `ALL SERDE CHECKS PASSED`，8 项断言全部通过：

| 验证项 | 结果 |
|---|---|
| `#[serde(tag = "type", rename_all = "snake_case")]` 映射三型问题 | 通过 |
| Choice 的 `criteria` 值为 `null`（官方允许） | 通过 |
| `instructions` 为结构化对象（官方 advanced 用法） | 通过 |
| 官方示例请求序列化往返（`type` 字段、`criteria` 形状一致） | 通过 |
| Noul 无 `criteria` 时不输出该字段（`skip_serializing_if`） | 通过 |
| 官方示例响应反序列化（`model` / `answers` / `usage`） | 通过 |
| 类型化 id 取答案（`Id<ChoiceKind>` / `Id<NoulKind>` / `Id<ScoreKind>`） | 通过 |
| 用错类型 id 取答案 → `None`（不 panic、不串数据） | 通过 |

**实测到的 serde 行为（非猜测）**：

```
实测：Noul 答案多出的 confidence 字段被 serde 默认静默忽略（未报错）
实测：未知 type 值导致该答案反序列化失败：unknown variant `ranking`, expected one of `noul`, `choice`, `score`
(A) struct + deny_unknown_fields 拒绝未知字段: Some("unknown field `cached`, expected `input_tokens` or `output_tokens`")
(B) 内部标签枚举 + deny_unknown_fields 报错: unknown field `confidence`, expected `noul`
(C) #[serde(other)] 兜底未知 type: Unknown
```

由此得出的三条设计含义：

1. **serde 默认静默丢弃未知字段**——服务端日后新增字段（例如给 Noul 也加 confidence）会**无声丢失信息**；要严格对齐 wire 契约须显式 `#[serde(deny_unknown_fields)]`。
2. **`deny_unknown_fields` 与内部标签枚举可共存**（验证通过，B 项；它只与 `flatten` 不兼容）。
3. **未知 `type` 会让该条答案反序列化失败**，若 `answers` 为 `map<String, Answer>` 则**整个响应解析失败**；前向兼容解法是保留 `#[serde(other)]` 兜底变体（验证通过，C 项），与 `deny_unknown_fields` 组合时注意语义叠加。

**映射与类型设计建议（落地方式）**：

- `Question` 用内部标签枚举；`choice` 的 `criteria` 为 `map<选项, Option<Instructions>>`（**必须允许 `null`**，已自测）；`score` 为 `Vec<Instructions>`（**2–10 级**）；`noul` 为可选 `{true, false}`。
- `Instructions` 用 `#[serde(untagged)]` 表达官方 `string | object | array`（已自测），使「把问题与它引用的数据放进同一对象」的结构化写法可用 Rust 结构体而非裸 `serde_json::Value`。
- **把「问题类型」与「答案类型」在编译期绑定**：用标记类型参数化 id（如 `Id<ChoiceKind>`），使 `Id<NoulKind>` 在类型上取不到 choice 答案——比社区 crate 普遍的 `response.choice("id").unwrap()`（写错只能运行期 `None`）更安全，是本项目相对现有社区实现的**真实改进点**。
- **不要用 `Option<f64>` 抹平三型差异**：官方 `noul` 答复**没有 `confidence`**，只有 choice / score 有；用枚举让「对 Noul 索要 confidence」变成编译错误。这是「类型安全」与「概率优先」总则的落地方式。
- **限制项应在类型或构造期强制**：Choice ≤ 255 选项、Score 2–10 级、`state` + 最长问题 ≤ 32k tokens；越早失败越好，别等到 422。
- **注意 HTTP 与 SDK 的 key 类型差异**：wire 上 `legend` / `probabilities` 是字符串级号，Python SDK 转成整数键——Rust 侧建议内部用整数键、序列化时转字符串，避免同一语义两套表示（see §3.5 D）。

### 10.4 并发、限流与超时策略

**官方 cookbook 实测（Rust 侧并发设计的性能依据）**【官方】<https://docs.typesafe.ai/cookbooks/parallel_questions>：测试条件为 GDPR 的 Wikipedia 条目（约 54,000 字符，文档主导型负载）、13 个问题（8 Noul + 2 Choice + 3 Score）、每种策略重复 5 次、使用官方 Python SDK（`typesafe-sdk>=0.5.7`）。

| 策略 | 调用数 | 成本 | 总耗时 |
|---|---|---|---|
| 一次调用，全部 13 问 | 1 | $0.000497 | 0.27s |
| 13 次调用，每问一次 | 13 | $0.006090 | 2.71s |
| — | — | **12.2× 更便宜** | **10.0× 更快** |

- 答案一致性：官方称「大多数答案在 5 次重复中完全一致，运行间标准差正好为 0.0」，即**打包不改变答案质量**（第三方反例见 §8.4）。成本机制：文档在每个请求中占主体，N 次单问要重复支付 N 次文档成本与往返。
- **⚠️ 官方内部数字不一致**：官方「Speculative fan-out」页对（推测）同一实验给出的是 *"batching 13 questions into one call is **11.5x cheaper and 9.6x faster** than 13 separate calls"*。【官方】<https://docs.typesafe.ai/patterns/fan-out> → 引用时应写「**官方两处口径为 11.5×–12.2× 更便宜、9.6×–10.0× 更快**」，或直接引 cookbook 原始表格（有完整测试条件）。

**限流与配额（超时/预算依据）**【官方】/models、/api：

| 项 | 值 |
|---|---|
| 模型 ID | `jev-1.13.0`（别名 `jev-latest`、`jev-preview`，当前同指 1.13.0） |
| 价格 | 输入 **$0.042 / Mtok**（=$42/Btok），**输出 token 免费** |
| 限流 | **250,000 tokens/秒** 与 **1,200 requests/分钟**，任一超限返回 `429` |
| 限流稳定性 | 官方警告 *"Rate limits are adjusting dynamically… the limits above can change without notice"* |
| 上下文 | 每请求 64k tokens；其中 `state` + 单个最长问题 ≤ 32k tokens |
| 输入模态 | 仅文本（字符串 / JSON 对象 / 文本数组），无图像、音频、视频 |
| 别名风险 | 官方建议：已按某版本调好 confidence 阈值时应**固定版本 ID**，别名会静默迁移 |
| 错误码 | `401` 无效密钥；`422` 请求校验失败（响应体指出出错字段）；`429` 超限；`529 Overloaded` 服务过载，均建议指数退避重试 |

**超时与重试的官方默认值**：

- JS/TS SDK `RetryPolicy` 默认：`maxRetries=2`、`backoffInitialMs=500`、`backoffMaxMs=5000`、`backoffJitter=0.25`、`httpStatuses=408/429/500–599`、`respectRetryAfter=true`、`maxRetryAfterMs=60000`、`apiConnectionError=true`、`apiTimeoutError=true`。【官方】<https://docs.typesafe.ai/sdk/javascript/api/interfaces/RetryPolicy>
- Python SDK：`DEFAULT_TIMEOUT = 10.0`、`DEFAULT_MODEL = 'jev-latest'`、环境变量 `TYPESAFE_API_KEY` / `TYPESAFE_BASE_URL` / `TYPESAFE_DEFAULT_MODEL` / `TYPESAFE_LOG_LEVEL`；文档中的配置示例为 `RetryPolicy(max_retries=3, timeout=10.0, http_statuses={429,500,502,503,504})`。【官方】<https://docs.typesafe.ai/sdk/python/api/constants>、<https://docs.typesafe.ai/sdk/python/api/retries>
- **⚠️ 两套官方 SDK 不一致**：Python 示例写 `max_retries=3`，JS 写明默认 `maxRetries: 2`；重试状态码集合也不同（`{429,500,502,503,504}` vs `408,429,500–599`）。Python 侧未找到「默认值」的显式声明，**其真实默认重试次数仍未证实**。本项目应自行定档并写入文档。
- 官方对 `Retry-After` 的措辞：*"Our client SDKs retry with backoff by default and honor the `retry-after` header when the response carries one."*（<https://docs.typesafe.ai/models>）

**并发设计的三条建议**：

1. **不要让每个判断各发一次请求**：在循环中做**问题批收集（batching）**，把同一 state 下的多个判定（路由、紧急度、门控）合并成一次调用；官方甚至鼓励「投机性扇出」——把可能用到的判定一并问掉，由代码决定用哪个答案。
2. **限流约束决定「并发」的真正用途**：并发应集中在一个**带全局预算的限流器**后面，而不是每个调用点各自重试（多处退避会互相叠加、放大 429）。真正的多路并发应留给**多个不同 state**（多候选重排、多文档分类），可参考官方 cookbook 中 `ThreadPoolExecutor` + 小 worker 池（如 `WORKERS = 8`，注释为 *"small pool: enough to keep a live run to minutes, gentle on rate limits"*）的做法。
3. **超时按「快模型」定档**：官方典型端到端响应 70–500ms、Python SDK 默认超时 10s，本项目 Jev 调用超时应远短于 LLM 调用超时（对 agent 循环的「卡住」检测很重要）；但官方 cookbook 中出现过 `timeout=120.0` 的用法（大文档/大问题组合），说明**超时预算应与 `state` 规模挂钩，单一常量不够**。

**UI / 事件流不能假设 token 流**：Jev 调用必须建模为「一次原子调用，返回完整结构」；若 harness 事件流围绕 LLM 的增量 token 设计，Jev 需要一个**立即完成的不同事件分支**，不能强行套用同一套流式接口（否则等于引入假的流式包装）。

**Rust 侧接入的具体形状（本项目自定，非官方）**：

- 端点与鉴权完全照 §3.1；只需一个 `POST /v1/systemone` + `GET /v1/models`，无需 SSE 解析器、无需 tool_call 流式状态机。
- 反序列化策略：对已知字段用强类型；对 `usage` 这类服务端可能扩展的结构考虑 `deny_unknown_fields` + 兜底变体的组合（见 §10.3 的取舍）。
- 重试：只对 `408/429/5xx` 退避；**遵循 `retry-after`/`retry-after-ms`**；退避总预算与单次调用超时分开配置（Python SDK 的 `timeout` 语义是"整次调用的重试总预算"，不是单次 HTTP 超时——这一点容易误读）。
- 幂等性：**不要重放"结果未知"的请求**（第三方 Elixir 客户端作者指出传输层失败后重放 billing 语义不明，见 §3.6）——本项目应把"是否已计费"作为不确定性向上传递，或显式选择不重放。

### 10.5 本节不确定项

1. **未找到官方对 Rust 的任何表态**（既无支持计划，也无明确「不做」的声明）。只验证了「官方现在没有 Rust SDK」，**不能推断官方未来不会出**；官方 GitHub 组织下也未逐 issue 检索。
2. **没有 API key，本次调研未对线上 API 做任何真实调用**。所有 HTTP 行为（限流触发、`429`/`529` 实际表现、`Retry-After` 头是否真的出现、实际延迟分布、`noul` 答复是否真的从不带 confidence）均**来自官方文档描述，未经实测**；§10.3 的 serde 验证是对**官方文档 JSON 示例**的往返，不是对线上响应的验证。
3. **官方单请求问题数上限未知**：文档只给单类约束（Choice ≤ 255 选项、Score 2–10 级），未找到「一次请求最多多少个问题」的明确上限（cookbook 用了 13 个，spec 提到 fan-out 可塞很多问题，具体上限未公开）。
4. **官方两处并行基准数字不一致**（12.2×/10.0× vs 11.5×/9.6×），且两处是否同测试条件无法确认，引用须同时注明两处。
5. **官方两套 SDK 重试默认值不一致**，Python 侧真实默认值未证实。
6. **社区 crate 全部未经生产验证**（无超过 329 下载量、无超过 11 star、全部创建于 2026-09-16 之后），`jev-sdk` 还出现元数据仓库 404。若引入需自行审计源码（尤其 API key 处理与 TLS 配置）。
7. **`jev-harness` 名字已被占用**（0.1.4，2026-09-21），**未核实其作者身份、是否与本项目存在关联**（例如是否为本项目此前的尝试），需项目决策者确认，不作推测。
8. **模型版本会漂移**：本节的字段形状、限制、价格、限流均**对应 2026-09-22 时点的官方文档**，未来可能变化。
9. **容量口径不一致（跨文件核实）**：官方模型页写 64k/请求（state + 全部问题）、32k（state + 最长单问题），而 **OpenRouter 上标 32K**、Cloudflare Workers AI 文档同样标 32,000；设计前须自行核实。
10. **serde 验证只覆盖官方示例的形状**，未覆盖：`probabilities` 浮点和不严格为 1、未知 `type` 的未来变体、`answers` 中缺失请求的 key、`usage` 为 `null` 等边界；这些需要在真实联调时补测（见 §14.3 实验 E5）。

---

## 11. 公司、条款与依赖风险

**要点**

- **公司很年轻、很早期**：TypeSafe AI, Inc. 2024 年成立，约两年 stealth，**2026-09-15 出 stealth** 并发布 Jev；融资**约 $40M 种子轮（DCVC 领投）**；LinkedIn 团队规模 **11–50 人**；创始人自述**上线前几乎没有收入**。→ 对本项目而言是**高潜力但单点依赖风险真实存在**的供应商。【官方新闻稿 + 播客 + 第三方】
- **无 SLA**：主合同（MCA）第 3 条的支持承诺仅为「commercially reasonable efforts」+ `support@typesafe.ai`，**全文无可用性数字、无停机赔付**。官方有状态页，但那是自我披露的历史数据，不是合同义务。【官方，已核对原文】<https://typesafe.ai/legal/mca>
- **可靠性是「已发生过的风险」而非假设**：官方状态页显示 `api.typesafe.ai` 近 90 天可用率 **99.840%**（`console.typesafe.ai` 99.988%），含多次数分钟级中断（8/03 **59 分钟**、9/19 **18 分钟**、9/20 记录 "API issues"）；TechCrunch 报道上线期因流量过高**一度无法从 API 服务用户**。【官方】<https://status.typesafe.ai/> +【第三方】<https://techcrunch.com/2026/09/18/a-new-kind-of-ai-model-from-a-chatgpt-inventor-is-thrilling-developers/>
- **数据条款比营销口径粗糙**：官方确实承诺不拿 Input 训练（隐私政策明文），但 **MCA 4.1 留有「客户事先同意」例外**且把「训练」限定为「修改模型权重」；**MCA 4.3 的 Telemetry 定义含 "learnings"、可无限制处理**，第 11 条把 Telemetry 知识产权归 TypeSafe，10.4 使其在协议终止后继续存续；**ZDR（零数据留存）仅面向企业客户**，需邮件申请。【官方，已核对原文】<https://typesafe.ai/legal/privacy-policy>、<https://typesafe.ai/legal/mca>、<https://docs.typesafe.ai/legal>
- **API 变更风险写进了合同**：MCA 2.5 明确「TypeSafe 可能不时更新服务，且**此类变更可能导致 API 与客户应用不兼容**」，仅承诺商业上合理努力提前通知重大不利变更。**这对需要长期稳定对接的 harness 是直接风险。**【官方，已核对原文】
- **生态接入面已相当宽，但容灾层薄**：官方 Python/JS SDK 之外，Pydantic AI、LangChain、LiteLLM、Cloudflare Workers AI、Opper、Requesty、Vercel AI SDK 均已适配；**未发现官方 Rust SDK**；OpenRouter 是否上架**两份上游文件结论冲突**（见 §11.4）。

### 11.1 公司身份、创始人、融资

| 事实 | 内容 | 来源 |
| --- | --- | --- |
| 成立年份 / 总部 | 2024 年成立；总部旧金山（注册地址 255 California St, Suite 1300, San Francisco, CA 94117） | 【官方】新闻稿（Business Wire，Yahoo Finance 全文转载）<https://finance.yahoo.com/technology/ai/articles/typesafe-ai-emerges-stealth-40m-190000776.html>；地址见 <https://typesafe.ai/legal/mca>、<https://typesafe.ai/legal/terms> |
| 出 stealth | 2026-09-15，同日发布 Jev 并开启 early access / waitlist | 【官方】同上；【第三方】TechCrunch 2026-09-18（同上 URL） |
| 融资 | **约 $40M 种子轮，DCVC 领投** | 【官方】新闻稿同上；【第三方】<https://www.finsmes.com/2026/09/typesafe-ai-raises-40m-in-seed-funding.html>、<https://www.hpcwire.com/aiwire/2026/09/16/typesafe-ai-emerges-from-stealth-with-40m-in-funding-with-new-model-for-composable-ai/>、<https://seedtable.com/companies/typesafe-ai/funding-rounds/seed-2026-09>、<https://dealroom.co/news/151032-typesafe-exits-stealth-with-40m-seed-to-build-ai-for-software-not-people/> |
| 投资方表态 | DCVC General Partner James Hardiman：*"Diogo, Erik, and Sasha bring the technical depth and conviction to rethink how models are built for software."* | 【官方】新闻稿同上 |
| 创始人 | **Diogo Almeida**（co-founder & CEO）、**Erik Gafni**、**Sasha Sheng** | 【官方】新闻稿同上；LinkedIn 帖 <https://www.linkedin.com/posts/robbroderick_after-two-years-in-stealth-diogo-almeida-activity-7506785550129864704-7UYQ> |
| Almeida 履历 | 「co-invented RLHF and InstructGPT，此前在 Google Brain」；官方与媒体多用「ChatGPT 共同发明人」口径 | 【官方】<https://typesafe.ai/team>；【第三方更正】更准确说法是 InstructGPT 论文的 equal-contribution primary author、并参与 GPT-4 <https://anthonymaio.substack.com/p/jev-the-language-model-that-wont> |
| 团队构成 / 办公 | 来自 OpenAI、Google Brain、Meta/FAIR、Stripe、Airbnb、Plaid、Docker；旧金山 Embarcadero 附近办公室，**每周 5 天现场办公** | 【官方】<https://typesafe.ai/team> |
| 团队规模 | LinkedIn 公司页标注 **11–50 employees** | 【第三方】<https://www.linkedin.com/company/typesafe-ai> |
| 招聘 | 在招 Member of Technical Staff（Model Capabilities / Backend-Platform），Base $150k–250k + equity | 【第三方】<https://jobs.ashbyhq.com/typesafe-ai>、<https://www.glassdoor.com/job-listing/jv?jl=1009970275998> |
| 收入状态 | 创始人自述「上线前几乎没有收入」(*"We had almost no revenue before launch."*) | 【播客-创始人】<https://www.latent.space/p/jev> |
| 自我定位 | 「We're building prod, not God」；自称 **data lab 而非 model lab**；训练数据全部合成 | 【官方】<https://typesafe.ai/manifesto>；【播客-创始人】同上 |
| 对外口径的谨慎 | 官方博文自认「193.6×/444.6× 属于偏高端」，且「无法证明定价没有被补贴」 | 【官方】官方发布博客 |

**可核实的时间线**：2024 成立 → 约 2 年 stealth → **不晚于 2026-06-24** `api.typesafe.ai` 已进入官方状态页监控（说明 API 在公开发布前已运行）→ 2026-09-15 出 stealth + 发布 + $40M 披露 → 2026-09-15～19 第三方生态适配密集上线（Every 实测 9/15；LangChain 指南、Pydantic AI provider、Cloudflare Workers AI 文档、LiteLLM pass-through 均在首周内出现）→ 2026-09-22 仍处 early access / waitlist 状态。

**产品线之外**：目前唯一公开模型就是 Jev（`jev-1.13.0`，别名 `jev-latest` / `jev-preview`，共用端点 `POST /v1/systemone`）；平台层有 `console.typesafe.ai`、评测站 `evals.typesafe.ai`、信任中心 `trust.typesafe.ai`（Vanta 托管）、文档站；路线图官方仅表「还有很多在管线中」，**无日期、无产品名**。**未找到公开信息**：OpenAI 兼容的 `/v1/chat/completions` 官方端点、本地/私有化部署方案、官方 Rust SDK。

### 11.2 可靠性、运维与运行区域

- 官方状态页（Better Stack 驱动，提供 30/60/90 天窗口切换，静态文本未直接标明当前窗口，抓取时可见条目自 2026-06-24 起）：`api.typesafe.ai` **99.840%**；`console.typesafe.ai` **99.988%**。可读到的中断：2026-06-29（6 分钟）、07-09（20 分钟）、07-10（18 分钟）、07-23（12 分钟）、**08-03（59 分钟）**、08-21（8 分钟）、09-09（6 分钟）、09-16（5 分钟）、**09-19（18 分钟）**，以及 09-20 一条标注 **"API issues"**（2 分钟，"now operational"）。【官方】<https://status.typesafe.ai/>（2026-09-22 抓取）
- 上线期容量事件：TechCrunch 报道公司因需求过高**一度失去从 API 服务用户的能力**。【第三方】同 TechCrunch URL。
- 创始人口径：「我认为我们的 uptime 9 的个数比 Anthropic 还多」，同时承认「我认为我们会在很长时间内受限于 GPU」。【播客-创始人】<https://www.latent.space/p/jev>（自我评价，非独立核实）
- 合同层面：MCA 第 3 条 Support = 「commercially reasonable efforts」+ 邮件支持；**无 SLA、无可用性百分比、无停机赔付**（已核对原文）。
- 安全事件通知：DPA 5.2 承诺「无不当延迟且无论如何在知悉后 **72 小时内**」通知；审计权：DPA 5.3 允许客户**每 12 个月最多一次**审计、费用自担；安全措施载体：DPA Schedule I 第 11 项全文仅为一句——安全措施「如 TypeSafe 信任中心所述」，即**一个 URL**（对 <https://trust.typesafe.ai/> 的抓取只得到 Vanta 的 JS 壳，**SOC 2 报告是否存在、是否可下载未见一手来源**）。【官方，已核对原文】<https://typesafe.ai/data-processing>
- **运行区域**：官方发布博文原文承认评测「一般是在美西的笔记本上跑的（**这也是我们服务目前所在的位置**）」；隐私政策称服务托管在**美国**（已核对原文）。→ 对非美区调用方，端到端时延需自行测量。
- 超载语义：`529 Overloaded` 是官方文档明确列出的错误码，说明服务端过载被当作**契约内的正常状态**，客户端必须自行退避。
- 第三方可用性观测（意义有限）：piehost 对 `typesafe.ai`（**营销网站**，非 API）90 天 HTTP 可用率 100% <https://piehost.com/status/typesafe.ai> ——**不覆盖 API，不可作为 API 可用性证据**。
- 与中国大陆的连通性、跨境延迟：**无任何公开数据**，需自测。

### 11.3 条款、隐私与数据留存（逐条已核对原文）

| 文件 | 覆盖范围 | 管辖/场地 | 责任上限 | URL |
| --- | --- | --- | --- | --- |
| Terms of Use | **仅网站 typesafe.ai** | 特拉华州法；特拉华州法院 | **$100 USD** | <https://typesafe.ai/legal/terms> |
| Master Customer Agreement (MCA) | **API 与控制台** | **加州法；旧金山** | 取高者：(A) 主张发生前 12 个月已付/应付费用，(B) **$50 USD** | <https://typesafe.ai/legal/mca> |
| Data Processing Addendum (DPA) | Input 中的个人数据 | 爱尔兰法；**都柏林法院**（EU SCC 模块 2/3 + UK Addendum） | 依 MCA；冲突时 DPA 优先（1.3） | <https://typesafe.ai/data-processing> |
| Privacy Policy | TypeSafe 自身的数据处理 | 未写明 | 不适用 | <https://typesafe.ai/legal/privacy-policy> |

- **不训练承诺（隐私政策，最平实）**：*"**We will not train or fine tune any artificial intelligence or machine learning models on your prompts or other Input.**"* 并承诺不向服务提供方以外的第三方披露 Input。
- **不训练承诺（MCA 4.1，留有口子）**：不会把 Customer Data 纳入训练数据集「(i.e., to modify the model weights of)」——**除非客户事先同意**；该定义**锚定在「修改模型权重」**上，字面不覆盖评测集、检索语料等。
- **Telemetry（MCA 4.3，实际最宽的授权）**：Telemetry = 「technical logs, hashes, summary statistics and classifications, metrics, and **learnings** related to Customer's use of the Services」，TypeSafe 可**无限制处理**（含用于改进其「其他产品与服务」）；4.1(b) 明确授权其为**派生 Telemetry** 而处理 Customer Data；第 11 条将 Telemetry 知识产权归 TypeSafe；10.4 使 4.1/4.3/11 在协议终止后**继续存续**。→ 即「由你的数据派生、归他们所有、期限无限制、协议结束后仍是他们的」。**隐私政策中不出现 "Telemetry" 一词**（已核对全文），两份文件口径存在落差。第三方批评见 <https://wunderlandmedia.com/typesafe-ai-jev-terms-of-service-gdpr>、<https://wonderwhy-er.medium.com/typesafe-jev-wont-train-on-your-data-it-can-still-learn-from-it-563f0ad591b6>。
- **留存与删除**：隐私政策只写留存「as long as reasonably necessary to provide you with the Services…」——**无具体天数**；MCA 10.3 明确 TypeSafe **无义务**存储或保留 Customer Data，并可**自行决定随时删除**；**DPA 正文中没有删除/返还数据的条款**。→ 第三方认为删除权需经 SCC 第 8.5 条迂回主张，且**没有删除证书或导出窗口**。
- **ZDR（零数据留存）**：仅面向企业客户，需邮件 `privacy@typesafe.ai` 申请——属「可谈」而非默认。【官方】<https://docs.typesafe.ai/legal>（与 LangChain 公开披露的 "TypeSafe does not currently offer zero data retention" 并存，**两条口径都保留**）
- **责任与赔偿的不对称**：12.2 上限取高者（12 个月费用, $50）；12.3 排除项含客户未付款、客户违反 2.3/2.4/5、以及**双方的赔偿义务**；13.1 侵权赔偿**仅限「美国」**专利/著作权/商标/商业秘密；13.5(e) **明确排除 Output**。
- **免责声明与营销口径的落差**：9.3 以全大写声明服务按 "AS IS / AS AVAILABLE" 提供、**可能产生不准确输出**、客户须自行评估；9.1/9.2 的 Service Warranty 仅保证按文档「materially」运行，救济是 30 天内修复或退还未使用预付费。→ 与营销侧「不能幻觉 / 100% 类型正确」并存：**类型正确 ≠ 语义正确**（第三方多篇均指出，官方博文也承认 hallucination 与 type-safety 是两件事）。
- **协议可被单方面变更**：16.7 允许 TypeSafe 通知更新协议、**至少 60 天后**对后续生效；ToU 第 6 条允许随时变更且「每次访问即受最新版约束」。**暂停权**：第 6 条列明可**立即暂停**的四种情形（含违反许可限制、逾期 30 天未付款、法律变化、危害其他客户/系统安全）。
- **许可限制（对本项目最重要的两条）**：2.3(b) **禁止用服务或 Output 做模型蒸馏、训练模仿其输出的模型、或开发同类/竞争产品**；2.3(a) 禁止把服务作为独立服务转售/分发/再许可；2.3(g) 把「进行安全或漏洞测试」与「绕过访问控制」并列禁止。→ **对本项目意味着：不能用 Jev 的输出蒸馏本地模型，也不能把 Jev 包装成同类竞争产品对外提供；渗透测试性质的压测需先确认合同许可。**
- **预付费与退款**：8.2 Credits「不可赎回、不可退款、不可转让、非法定货币」，到期时间取「期限结束」或「购买后 12 个月」较早者；10.3(c) 终止时**未消费金额不退还**。
- **争议解决**：15.1 强制**单独仲裁**（JAMS，加州），15.7 放弃集体诉讼；MCA 由加州法管辖、旧金山场地（16.2）。
- **网站抓取限制（仅限网站，不是 API 条款）**：ToU 3(b)(vi) 禁止以自动化进程 / robots / spiders / scrapers / webcrawlers 监控、复制或下载 Site 上的数据；**MCA 全文没有禁止公布 benchmark 的条款**。
- **⚠️ 第三方分析有可验证的错误**：wunderlandmedia 一文称「MCA 适用特拉华法 / loser pays」——**错**（特拉华法 + $100 上限对应的是仅覆盖网站的 Terms of Use，该文把两份文件混为一谈）；又称「MCA 2.3(f) 禁止公布 benchmark，违者暴露无限」——**当前原文中不存在该条款**（2.3(f) 实为 "interfere with the operation of the Services"）。**该文细节不可直接引用**；其其余内容（Telemetry 四条款联动、责任上限不对称、13.1 仅限美国 IP、13.5(e) 排除 Output、无 SLA、DPA 附件薄弱、TypeSafe 未加入 EU-US DPF）与原文方向一致，可作线索但引用前须回到一手条款。

### 11.4 生态接入面与单一供应商风险

| 集成方 | 形态 | URL | 性质 |
|---|---|---|---|
| Cloudflare Workers AI | 官方文档托管 `typesafe/jev`，含 Workers `env.AI.run` 与 REST 两套示例，**上下文窗口标 32,000 tokens** | <https://developers.cloudflare.com/ai/models/typesafe/jev/> | 【第三方平台官方文档】影响力最大的分发渠道之一 |
| Pydantic AI | 内置 `TypeSafeModel`（`typesafe:jev-latest`），一个 Pydantic 输出模型的每个字段映射为一个问题；`ModelResponse.model_name` 记录实际应答版本 | <https://pydantic.dev/docs/ai/models/typesafe/> | 【第三方平台官方文档】 |
| LangChain | `langchain-typesafe`：`TypeSafeClassifier` + `ModelRouterMiddleware`（模型路由）+ `AutoModeMiddleware`（工具调用前的风险门控） | <https://www.langchain.com/blog/building-a-harness-with-jev> | 【第三方】与本项目「Agent Harness」定位最接近的公开范例 |
| LiteLLM | pass-through 端点支持（Jev *"is called through its own evaluate endpoint rather than `/chat/completions`"*） | <https://docs.litellm.ai/docs/pass_through/typesafe> | 【第三方】 |
| Vercel AI SDK | 有接入教程；Vercel 工程师 Pranit Sharma 用 Jev 替换 ChatGPT Luna 做命令安全分类，称**快 5–18× 且更准** | 同 TechCrunch URL | 【第三方】单场景自测，非基准 |
| 网关 | Opper（标注 ZDR posture「not established」、有 DPA、无训练）、Requesty（提供 OpenAI 风格 `response_format: {"type":"questions"}` 封装） | <https://opper.ai/provider/typesafe>、<https://www.requesty.ai/blog/typesafe-jev-explained> | 【第三方】 |

**⚠️ OpenRouter 是否上架：两份上游文件结论冲突，并列呈现，不作裁定**

- 【结论 A｜未上架】以 `https://openrouter.ai/api/v1/models` **全量列表**检索，未找到任何 Jev / TypeSafe 条目；同日抓取 `openrouter.ai` 的 Typesafe 页面亦为空页 → 推论是 **Jev 没有第三方多 provider 路由/故障转移层**，harness 容灾不能指望「换 provider」。（来源：上游 `12-company.md`，2026-09-22 实查）
- 【结论 B｜已上架】以 `https://openrouter.ai/api/v1/models/typesafe/jev-1.13/endpoints` 读取到条目：`typesafe/jev-1.13`、`context_length 32000`、pricing prompt `0.000000042`（=$0.042/M）、completion `0`、provider=TypeSafe、`uptime_last_1d` 100%；`10-alternatives.md` 亦从 OpenRouter 的 Jev 1.13 页面确认 context length 写的是 **32K / 32,000**。（来源：上游 `13-community-debate.md`、`10-alternatives.md`，均为 2026-09-22 读取）
- **口径差异的可能解释**（上游未裁定）：全量列表未收录 / 分页或区域差异 / 仅特定账号可见 / 条目页与列表页数据不同步。**引用时必须写清检索路径与时间**；「Jev 是否有第三方多 provider 容灾层」这一结论目前**不能定论**。
- 附注：官方发布博文提到的 "The numbers for LLMs are from OpenRouter" 是指其评测中的 LLM 基线，**不表示 Jev 已在 OpenRouter 上架**。

**Rust 客户端相关**：未发现官方 Rust SDK。据官方 API 参考，接入所需信息为：单一端点 `POST /v1/systemone`、Bearer 鉴权、问题/答案以调用方自定义 key 一一对应（**key 不发送给模型**）、错误码 401/422/429/529，429 与 529 需指数退避并遵循 `retry-after`。<https://docs.typesafe.ai/api>

**依赖风险的叠加结论**：无 SLA + 状态页 99.840%（含 59 分钟级中断）+ 上线期曾无法服务用户 + 合同明确「API 更新可能导致不兼容」+ 预付费不可退 + 闭源无自托管 +（有待核实）第三方路由层 → harness 必须默认具备**超时预算 + 429/529 指数退避 + 熔断降级**（降级路径可回落确定性规则或 LLM 判定），并把不确定性作为一等数据向上传递；模型别名会漂移，应固定 `jev-1.13.0` 这类版本号，由 harness 自己控制升级节奏。

### 11.5 本节不确定项

1. **估值与轮次规模有冲突数据**：Forbes 文章标题为「This $200 Million Startup Wants To Fix AI's Overconfidence Problem」（<https://www.forbes.com/sites/the-prompt/2026/09/15/this-200-million-startup-wants-to-fix-ais-overconfidence-problem/>），但**两次尝试均无法提取正文**（403 / 提取失败），「$200M」指估值还是其他数字**未能证实**；dealroom 页面文本中出现 "US$25.9M seed round" 字样，与 $40M 不一致且**未见解释**。→ **唯一可引用的融资数字是「约 $40M 种子轮，DCVC 领投」（Business Wire 新闻稿原文）**。（§2.6 提到的 Instagram $200M 传闻与本条同源，均不采信）
2. **模型架构未公开**：官方不披露参数量、架构、硬件、训练配方、模型卡、权重。TechCrunch 称「outside observers suspect is built on top of an open-weight LLM」——**属记者转述与猜测，Almeida 拒绝透露**，不可当事实。
3. **RLCD 无公开技术细节**：官方称 RLCD 为自研训练法、数据全合成，但**未发表论文/技术报告**。「概率是否校准」目前只有厂商声称与文档建议（在自有数据上测阈值），**无第三方校准曲线**。
4. **SOC 2 / 认证状态未证实**：Trust Center 抓取只返回 Vanta 前端 JS 壳，无法读取子处理器清单与合规条目；「子处理器为 AWS（存储/处理）、Modal（推理）、Slack、Google Workspace（支持），全部美国公司」这一点**仅见于第三方，未见一手来源**。
5. **无官方 SLA 文本**：MCA 中确认无 SLA（已核对）；**是否存在单独的企业级 SLA 附件**（仅签约客户可见）**无从核实**。
6. **定价可持续性未证实**：官方自认「无法证明定价未被补贴，长期会证明其可持续性（预期价格会降不会升）」；播客中创始人称「我认为我们会在很长时间内受限于 GPU」。**$0.042/Mtok 是否为长期价格，无独立证据**。
7. **多数评测数字是厂商自评**：193.6×/444.6×、70–500ms、「无法幻觉」等均出自官方，官方自己声明这些数字偏高端、评测在美西、任务由自家模型能力团队构造（存在偏倚）。较独立的量化对照是 Every 的 777/1709 次实调用与 kingy.ai 的文档复核。
8. **未找到公开信息**：公司营收/ARR（只有创始人「上线前几乎无收入」口述）；员工确切人数（只有 LinkedIn 区间）；是否有后续融资轮；官方 Rust SDK；私有化部署方案；正式 AUP（可接受使用政策）与高风险用途限制条款（第三方指出四份文件中**没有**禁止清单或人类监督要求）。
9. **第三方来源质量提示**：检索到的相当一部分「分析」文章（kingy.ai、wunderlandmedia、truefoundry、mindstudio、innfactory、requesty、opper 等）是发布后数日内产出的内容营销/SEO 文章，含**可验证的错误**（见 §11.3 末）。引用时优先官方文档与条款原文，第三方仅用于印证与批评视角。
10. **本节的合同分析属文本审阅，不是法律意见**：MCA/DPA/ToU 的条款已按原文核对，但「这些条款对某个具体司法辖区的实际效力」需要法律顾问判断。

---

## 12. 社区争议与采用动态

**要点**

- **讨论规模确实很大，但热度不等于验证**：HN 主帖《Introducing System One Models and Jev》发布于 2026-09-15 19:25:03 UTC，检索时（2026-09-22T02:54Z）为 **1945 分 / 511 条评论**（同一篇评论文章写作时记录为 **1430 分 / 404 评论**——引用必须带时间戳）；Latent Space 称发布视频在 X 上约 **4000 万次观看**；中文侧 9/16–9/21 报道密集，机器之心相关推文话题被称 **3700 万人围观**。【第三方】
- **社区质疑集中在五条主线上，其中三条被官方自己部分承认**：①「不能幻觉」被普遍解读为**类型安全 ≠ 事实正确**（官方博客自己注明幻觉图的 0% **不是实测数据，是 schema 保证**）；②**速度/成本对比是苹果比橘子**（Jev 不做自回归生成，拿它比逐 token 生成的延迟被指营销误导）；③「前沿模型」标签与首个标题被指误导（原帖标题曾为 *"Jev: New frontier model 40-400x cheaper and 20-200x faster"*，不到一小时被改掉）；④**架构不新**（GLiNER / ModernBERT / 零样本分类器一脉）；⑤**概率是否真的校准、是否稳定**。
- **官方主动放弃了公开基准**：博客《Lies, Damned Lies, and Benchmarks》宣布「**不在模型发布中放标准基准表**；新 evals 只做带日期的快照、发布后即淘汰、不再 hill-climb」，同时公布内部 evals 并附不利证据。→ 这既是回应质疑，也意味着**外部无法用公开 benchmark 独立比对**。【官方】<https://typesafe.ai/blog/antibenchmaxxing>
- **校准与稳定性上官方与第三方正面冲突**：官方 jaggedness 文档**自己列明**算术/计数、日期比较、多跳、无关信息、对抗输入、矛盾指令下不可靠，并明说**不保证结构不变量**；第三方（8 个真实数据集）称其**校准落后 CatBoost**（二手转述，未独立复现）。【官方】jaggedness 页；【未证实】<https://m.sohu.com/a/1079019769_129720>
- **采用动态已从候补制转为全面开放**，并迅速铺开分发渠道：官方公告取消候补、注册送 $5 额度（≈1.2 亿 token）；第三方渠道包括 Cloudflare Workers AI、LangChain、Braintrust；社区出现多份「复刻 Jev」项目与上下文压缩插件。【第三方（官方 X 公告未能直连核对）】
- **社区共识偏中性**：「**架构不新，但零样本通用性 + 校准 + 并行接口 + 极低延迟这套组合少见**」；最强烈也最未获满足的情绪是**开源与本地化诉求**（官方**未承诺**开放权重或本地部署），而 32k 上下文、闭源、无本地部署是硬约束。

### 12.1 讨论规模与时间线

| 事实 | 数值/语境 | 出处 |
|---|---|---|
| HN 主帖 | 标题 *Introducing System One Models and Jev*，作者 `albelfio`，2026-09-15 19:25:03 UTC，score **1945**、评论 **511**（2026-09-22T02:54Z 读取） | HN API <https://hacker-news.firebaseio.com/v0/item/49717558.json>；帖子 <https://news.ycombinator.com/item?id=49717558> |
| 早期快照 | 一篇评论文章写作时记录为 **1430 分 / 404 评论**（说明热度仍在增长，引用数字需带时间戳） | <http://wimes.org/articles/2026-09-16-typesafe-jev-cant-hallucinate-exactly-the-problem/> |
| 发布视频播放量 | "launch video now viewed **~40M times**"，并与 GPT4o 22M、Fable 5 15M、Astra 137M 对比 | <https://www.latent.space/p/jev> |
| 中文侧热度 | 「相关推文已有 **3700 万人围观**」 | <https://k.sina.com.cn/article_5953466437_162dab0450670bd10e.html>（机器之心原稿） |

### 12.2 五条质疑主线与官方/第三方的相互印证

#### ① 速度/成本对比与「前沿模型」标签

- 【第三方·HN 高信号评论】*"A generative model that can output code in a Turing-complete language can do anything a computer can do. Jev can only generate structured output… this is probably super useful for classification/routing/scoring, but it's nothing like the code generating models"*（HN 首条评论）<https://news.ycombinator.com/item?id=49717558>
- 【第三方·可定位引文】`WhitneyLand`：原帖标题曾为 *"Jev: New frontier model 40-400x cheaper and 20-200x faster"*，**不到一小时前被改掉**…「我不认为多数人会把 Jev 算作前沿模型，这听起来是假的」，并给出他认为更准确的措辞 *"Advanced the speed/cost frontier for structured decisions"*：<https://news.ycombinator.com/item?id=49719001>、<https://news.ycombinator.com/item?id=49719144>
- 【第三方·可定位引文】`bigglebear`：*"It's a **wildly dishonest presentation** of their product from many perspectives… The comparison between LLM speed and Jev speed is misleading, because they're using autoregression to generate all of the type names, all of the schema… You really have to do a lot of hand holding here"*，并称分布刻画不对结果可能是 garbage：<https://news.ycombinator.com/item?id=49720601>
- 【第三方·Demo 反驳】HN 与 Reddit 均指出 Doom demo 用的是**结构化文本状态**（坐标、血量、弹夹），不是画面；**官方自己也承认**：*"The demo is on structured state as a data structure with text, **not on images** (yet…)"*、*"A **non-AI doom bot could play better**, but we wanted a bot that was reactive to different representations of game state"*（官方发布博客）。
- 【第三方·用官方自己数据反驳】Customer Service 工作流上 Jev **76.0%（$0.0001 / 0.4s）**，而同页 GPT-5.6 Sol **78.3%（$0.0323 / 10.1s）**、DeepSeek V4 flash 76.8%（$0.0029 / 34.6s）→ Jev **明显更便宜更快，但并非最准**：<https://evals.typesafe.ai/customer_service.html>
- 【官方口径】193.6×/444.6× 来自官方 workflow evals，官方主动标注三点偏差：参照取 **GPT-6 Astra 与 Fable 5.1 的平均**（偏向 OpenAI/Anthropic）、工作流由自家能力团队制作（可能偏差）、数字「处于真实收益的高端」。（复算与不可引用判定见 §6.3）

#### ② 「不能幻觉」的语义之争

- 【第三方·可定位引文】`WhitneyLand`：*"**Type safety is not factual correctness.**"*（同上 HN 帖）
- 【第三方·HN 长评】*"Your launch post puts '0%' on a hallucination chart, then explains that the number comes from **guaranteed schema matching**. You've already agreed that this doesn't establish correctness. **An approve for an unauthorized action still meets the schema guarantee.**"*（同帖评论区）
- 【官方·自我披露】官方博客在幻觉/类型安全一节主动注明：*"**Our number is not empirical.** Schema matching is guaranteed, thus we can confidently add 0% into the plots."*（社区对此反应反而偏正面：承认这一点比宣称「零幻觉」更可信。）
- 【第三方·KDnuggets】*"**'Zero hallucinations' is therefore closer to zero out-of-schema outputs, not zero incorrect decisions.**"* <https://www.kdnuggets.com/what-everyone-is-getting-wrong-about-typesafe-ais-jev>
- 【第三方·治理视角】一篇评论文章把「无文本输出」视为新的可审计性缺口（律师/保险人/监管者无「可引用的陈述」）——属**观点文章**，其中引用的「某支付机构模型风险工程师 Slack 对话」**无法核实**：<http://wimes.org/articles/2026-09-16-typesafe-jev-cant-hallucinate-exactly-the-problem/>
- 【第三方·Arize】对「can't hallucinate」的评论是 *that really feels like an over-reach*（见 §6.4）。

#### ③ 架构与「是不是新东西」

- 【第三方·可定位引文】r/LocalLLaMA `logical_Two_7736` 反驳「一年前就做过」：*"Your SalesRLAgent is essentially a **sequential PPO policy** … Jev is more general. One state can be queried with several independently typed questions, and it can emit distributions for categorical choices, scores and booleans **in parallel**."* <https://www.reddit.com/r/LocalLLaMA/comments/1wijo3e/>（经 pullpush 存档读取）
- 【第三方·KDnuggets】*"Classification, scoring, routing, and intent detection are not new machine learning problems… The problem is old; the architecture and product around it may be new."*，并指出「我们仍几乎不知道 Jev 的内部架构、训练设置与模型规模，因此最大的主张高度依赖 TypeSafe 自己的基准」。
- 【第三方·Reddit】大量评论将其类比 GLiNER / GLiClass / ModernBERT / 零样本分类器，并给出具体开源替代品 <https://huggingface.co/knowledgator/gliclass-instruct-base-v1.0>、<https://huggingface.co/fastino/gliner2.5-base-v1>（见 <https://www.reddit.com/r/LocalLLaMA/comments/1wlfmgq/>、<https://www.reddit.com/r/LocalLLaMA/comments/1whxf90/>）
- 【未证实·中文媒体转述】ML 工程师 Sebastian Raschka 认为「把 Jev 简单看成分类器会低估它」，关键可能在**数据与 API 设计**，训练算法本身未必复杂；文中另称「有网友表示创始人曾暗示 Jev 使用完全合成数据集、在已有模型上做后训练，未做大规模预训练」：<https://m.sohu.com/a/1079019769_129720>（引 <https://x.com/rasbt/status/2101672304358948992>）。**X 原帖未能直连核对（登录墙），视为二手转述。**
- 【社区共识（偏中性）】「架构不新，但**零样本的通用性 + 校准 + 并行接口 + 极低延迟**这套组合确实少见」；r/LocalLLaMA 用户 `Aloekine`：*"JEV is meaningfully more general in the sense that its baseline performance across problems it hasn't seen yet is really, truly impressive."*
- 【未证实·先例主张争议】「我一年前就实现了 Jev 架构」（作者 Nandakishor M）的 arXiv 论文确实存在且日期吻合（*SalesRLAgent*，2025-03-30 提交 <https://arxiv.org/abs/2503.23303>），但同帖高赞反驳指出其是「顺序 PPO + 单一连续转化概率」，与 Jev 的并行多类型问题不等价；r/LocalLLaMA 亦有技术用户质疑其论文未同行评审、方法学有缺陷。**结论：可引用「社区对是否存在先例存在争论」，不可断言 TypeSafe 抄袭。**

#### ④ 概率校准与稳定性（官方与第三方正面冲突）

- 【官方·自述局限 `jev-1.13`】jaggedness 文档列 9 类失败模式并给替代做法（完整表见 §4.1）：字面理解、**算术/计数**（*"does not count reliably… the error grows with the size of the thing being counted"*）、**日期时间比较**、多层间接推理、大 state 干扰、**Adversarial content**（state 是数据，jev-1.13 默认不把 state 当敌意输入，可移动答案）、矛盾指令、**常识结构不变量**、生成任务：<https://docs.typesafe.ai/model-jaggedness/jev-1.13>
- 【官方·结构不变量反例（关键）】同一问题两种问法互相矛盾：`Noul` 得 0.22，`Choice yes` 得 0.01（该 Choice 的 confidence 0.97）；*"Is the customer asking for a refund?"* 与 *"Is the customer asking for something other than a refund?"* 两个 Noul 得 **0.72 / 0.47（和 = 1.19）**；官方结论：*"**don't rely on expected structural invariance**… don't hold the model to arithmetic identities between separate questions"*（同页 URL）
- 【未证实·第三方校准反例】中文报道称 X 用户 `@predict_addict`（Valeriy M.）在**贷款违约、企业破产、医疗诊断等 8 个真实数据集**上测试，结论为 **Jev 的概率校准明显落后 CatBoost，且经常高估负面结果**，简单后处理可修正部分误差；报道另记录**同一提示多次运行概率不同**、**选项顺序会大幅改变输出概率**（把「信息咨询 / bug 报告」换序后，模型从偏向 bug_report 变成偏向 information）。出处 <https://m.sohu.com/a/1079019769_129720>。**未找到原始数据/代码，按「未独立复现」处理。**
- 【第三方·LangChain】5 条固定 agent 输出 × 100 次重复：二元判定与人工 oracle **500/500 一致**；**每例评分方差 0.0000149**；单次成本 **$0.00035**；局限为样本极小、「观察性、非因果」、且 **Jev 服务版本未记录在实验元数据中**：<https://www.langchain.com/blog/jev-agent-evals-langsmith>
- 【第三方·Braintrust】*"**Confidence is separate from the selected option's probability. A high value does not guarantee that the decision is correct.**"*，并给出示例：Jev 对「编造退款/额度」的草稿给出 `Incorrect, unsafe…` 标签，confidence 仅 **0.57**、probability 0.68；建议按置信度分级路由而非一刀切：<https://www.braintrust.dev/blog/evaluate-agent-responses-with-jev>

#### ⑤ 本地/开源诉求与营销指控

- 【第三方】HN 评论出现「closed source 令人不安」「这种进展不该被锁在本地硬件之外」；官方**未承诺**开放权重或本地部署 <https://news.ycombinator.com/item?id=49717558>
- 【未经证实的指控】Reddit 有用户称 *"they ran an **astroturfed marketing campaign** all over twitter for last 2 days"* <https://www.reddit.com/r/LocalLLaMA/comments/1whxf90/>；HN 也有人称「Twitter 评论像是 paid partnerships」。**属情绪化指控，无证据，仅记录其存在。**

### 12.3 采用动态与社区生态

| 项目/实测 | 数字与语境 | 出处 |
|---|---|---|
| 全面开放 | 官方公告取消候补、所有人可注册、$5 起步额度 ≈ 1.2 亿 token（入口 console.typesafe.ai） | 中文转述 <https://m.sohu.com/a/1079019769_129720>、<https://k.sina.com.cn/article_1644983660_620c756c02001vwuw.html>；**官方 X 公告未能直连核对（登录墙）** |
| 采纳速度 | VentureBeat（2026-09-21）：36 小时内从 waitlist 清掉 **140,000** 人；Vercel 称约 **13%** 的付费 AI Gateway 团队在 24 小时内启用 | <https://venturebeat.com/security/companies-are-putting-jev-in-charge-of-ai-agent-decisions-and-prompt-injection-can-influence-the-verdict> |
| Vercel 侧（厂商自述） | 24 小时内 *reached more than twice as many paid teams as any previous model launch*；到第 24 小时接近 13% 付费团队在用，是 GPT-5.6 家族的 2 倍、Fable 5.1 份额的 6 倍以上；声称是 gateway 历史上采用最快的模型 | <https://vercel.com/blog/ai-gateway-jev-model-launch> |
| 早期访问状态时点冲突 | VentureBeat 称 2026-09-20 晚取消 waitlist，而 flaviocopes 的文章（约 1 天前）仍写 *Jev is in early access. Join the waitlist* | <https://flaviocopes.com/jev/>（**以官方渠道为准**） |
| Cloudflare Workers AI | 模型 `typesafe/jev`（返回 `jev-1.13.0`），**32,000 token 上下文**，含 noul/choice/score 三类示例与完整响应体 | <https://developers.cloudflare.com/ai/models/typesafe/jev/> |
| OpenRouter | `typesafe/jev-1.13`，`context_length 32000`，pricing prompt `0.000000042`（=$0.042/M）、completion `0`，provider=TypeSafe，`uptime_last_1d` 100%（**与另一份上游文件「未上架」结论冲突，见 §11.4**） | <https://openrouter.ai/api/v1/models/typesafe/jev-1.13/endpoints>（2026-09-22 读取） |
| LangChain 集成 | `langchain-typesafe`：`TypeSafeClassifier`、`ModelRouterMiddleware`（按难度分流模型）、`AutoModeMiddleware`（工具调用前用 Jev 做风险门控） | <https://www.langchain.com/blog/building-a-harness-with-jev> |
| Braintrust | 把 Jev 作为 judge/scorer 接入（Trace span 名 `typesafe.systemOne`），保留 label/confidence/probabilities 供复核 | <https://www.braintrust.dev/blog/evaluate-agent-responses-with-jev> |
| 浏览器 Agent 实测（HN 用户 `silbercue`） | 把可点击元素（accessibility tree，约 10–40 个 ref）作候选项，**21–23 次决策、六个 benchmark 卡全对、总计约 $0.001**；文本输入仍需一个小模型 | <https://news.ycombinator.com/item?id=49758669> |
| 上下文压缩插件 | `tamaratran/fast-jev-compaction`：Claude Code 插件，用 Jev 给工具调用打分以替换 compaction 摘要；GitHub **6013 stars**（created 2026-09-17） | <https://github.com/tamaratran/fast-jev-compaction> |
| 其他社区复刻/衍生 | `vinnylarouge/jevlike`（1169★）、`TheoLeeCJ/SemIf`（3316★，"Semantic ifs from open models, on a 3090 at home"）；另有多个 `awesome-jev` 清单仓库 | GitHub API 检索 `q=jev+typesafe` |
| 媒体转述的实测数字 | 724 条广告 / 37 品牌 / 8724 次判定 / 约 40 秒 / 约 $0.09 token 成本（Matthew Berman）；浏览器航班搜索 demo 约 7 秒、约 $0.004；模型路由实测 Jev ~1s vs 结构化输出 LLM 4–14s | <https://k.sina.com.cn/article_5953466437_162dab0450670bd10e.html>、<https://www.kdnuggets.com/what-everyone-is-getting-wrong-about-typesafe-ais-jev>（均为二手汇总，原始 X 帖未直连核对） |
| 竞品/对照 Laya | Laya（400M 级、"think layer on ModernBERT"，自称对标 Jev）——Reddit 多方评价其「不微调基本是猜」，与 Jev 的零样本通用性不同层级 | <https://laya.convaiinnovations.com/>；<https://www.reddit.com/r/LocalLLaMA/comments/1wlfmgq/> |

**工程化用法（可对照本项目 harness 设计）**：

- Braintrust 建议的**置信度阈值分级路由**：**>0.95 自动接受；0.70–0.95 交给更强的 LLM 判官；<0.70 视为不确定或转人工**，并强调阈值必须先用自有数据验证、不能照搬。<https://www.braintrust.dev/blog/evaluate-agent-responses-with-jev>
- LangChain 的 `AutoModeMiddleware(tools=["bash"])` 形态：**在工具执行前用 Jev 做风险门控**；原文点明「这类分类器步骤以前一直锁在闭源 harness 内部，现在有了便宜且性能足够的分类模型，可以把同样的模式用到所有 agent 上」。<https://www.langchain.com/blog/building-a-harness-with-jev>
- 社区做法 `fast-jev-compaction` 用 Jev 打分来裁剪上下文——对应本项目 harness 的上下文组织职责。<https://github.com/tamaratran/fast-jev-compaction>

### 12.4 本节不确定项

1. **X/Twitter 内容无法直连核对**（登录墙；nitter 镜像亦不可用）。所有 X 相关数字与观点（Raschka 评价、`@predict_addict` 的 8 数据集校准测试、各种实测 demo 数字）均来自中文媒体汇总或 HN 转述，**只能作为「存在此类说法」的线索，不能作为一手证据**。
2. **HN 帖分数/评论数随时间变化**：一手读取（2026-09-22）为 1945/511；早前评论文章记为 1430/404。引用必须带时间。
3. **Reddit 分数不可靠**：经 pullpush 存档 API 取得的评论 `score` 多为 1–2，明显滞后或失真，故只引评论**内容与作者名**，不引票数。
4. **未核实的数字**：cnblogs 称 OpenRouter 上「上线几天内已处理超过 **1610 亿 token**」（用 OpenRouter API 只能确认模型、上下文与定价，**未能核实用量**）<https://www.cnblogs.com/sing1ee/p/23040079>；KDnuggets 称「自家 workflow 评测约 68%」「一项小规模 fact-checking 测试 96.3%」「275 份文档测试强一致」——**均未给出一手来源**，且官方 evals 上 Customer Service 是 76.0%，两者关系不明、**不可混用**；cnblogs 称输入价「比 Claude Fable 5.1 低 238 倍」——**未在官网原文中核实到该表述**；Vercel AI Gateway 免费至 9/25（**【未证实】**，仅见搜索摘要与 HN 单条评论）、Swamp 扩展（仅 HN 用户自述）、`qibinlou/jev-chess`（GitHub API **返回 404，仓库不存在，不可引用**）。
5. **先例主张存在争议**（见 §12.2 ③末）。
6. **架构细节仍属猜测**：社区推测（编码器+回归头、文本扩散、约 3B 参数、并行 prefill 取单 token 概率等）**均无官方确认**；官方未公布参数量、架构细节与训练数据来源。
7. **官方对基准质疑的正式回应带来可验证性缺口**：博客《Lies, Damned Lies, and Benchmarks》宣布不在发布中放标准基准表、新 evals 只做带日期快照并附不利证据——这是官方主动放弃公开榜单，**也意味着外部无法用公开 benchmark 独立比对**：<https://typesafe.ai/blog/antibenchmaxxing>
8. **疑似 AI/SEO 批量生成的内容农场**（命中但不采用其数字）：explainx.ai、alphamatch.ai、cryptobriefing.com（称「对抗性漏洞引发安全担忧」）、modemguides.com、cellcog.ai、medium 系列等，这些站点数字与其他来源不一致且无法追溯原始测试。**社区「实测」普遍自述、缺方法与数据**（如「一晚做出实时交易机器人，已亏 $31680」），仅可作轶事。
9. **`jev-1.13` jaggedness 页的 URL 在上游两次调研中不同**：`10-alternatives.md` 按 `docs.typesafe.ai/models/jev-1.13-jaggedness` 抓取返回 404（该页内容当时经 Langfuse 转述）；`13-community-debate.md` 则成功抓取 `https://docs.typesafe.ai/model-jaggedness/jev-1.13` 并逐条引用。两者是否同一页面、官方是否改过路径，**未核实**；本报告的 jaggedness 引文来自后者（可访问的那一个）。

---

## 13. 对本项目的定位建议

> **本节性质**：前 12 节是事实与口径的记录；本节是**综合判断**。每条建议后括注（依据：§章节 / 标记｜可信度）。可信度分三档：**高**＝官方文档明确 + 多方独立印证；**中**＝有第三方实测或官方自述支持，但样本小/口径有限；**低**＝依赖单一来源、二手转述或仅为方向性共识。**所有「实测验证项」见 §13.4，实验设计见 §14.3。**

**要点**

1. 建议把 Jev 定位为 **harness 循环内的「快速判定层 / 决策原语提供者」**，而不是主模型、不是安全边界、不是精度天花板。
2. 四个候选角色的成立条件并不相同：**路由/分类与 judge 最接近可用（成本与稳定性驱动）**；**动作门控只有 fail-closed + 确定性检查并用时才可考虑**；**信息抽取需限定在封闭集合字段**。
3. 架构上按「**决策**」而非「模型调用」抽象，使 Jev / LLM 结构化输出 / 本地分类器 / 规则成为**可替换后端**——这是本项目最重要的一条设计结论。
4. 概率必须作为一等数据（保留 `probabilities` / `confidence` / `noul` 原值），加上**模型版本、选项顺序、request_id** 一起进 trace。
5. 在 §13.4 的六项（及以上）假设**实测通过之前**，Jev 只能出现在**可回退、可观测、非破坏性**的路径上。

### 13.1 候选角色、成立条件与风险

#### 角色 1：路由 / 分类（Routing / Classification）

- **建议**：**可以进入试点**，但定位是「**成本分层**」而不是「**质量提升**」；必须部署在 fail-open 路径上（分类失败 → 默认路由），并保留确定性规则或 LLM 兜底。
- **成立条件**：
  1. 候选集合预先枚举、数量在实用上限内（官方称 Choice 约 240 项内可靠；>255 需两阶段且更慢）；
  2. 调用点允许失败后回落（路由、技能选择这类"少做一件事"的决策）；
  3. 有监控置信度分布的能力（低 confidence 会集中出现在选项不互斥的输入上）。
- **支持证据**：LiteLLM 分类环节 p50 126.81 ms vs Haiku 688.40 ms、成本 −96.118%、tier 一致率 95.00% vs 73.75%（依据：§6.4 LiteLLM｜**中**）；Jevals 在各自校准到 95% 准确率的阈值下 Jev 能独自承接 nope 86% / choice 54% 的决策（依据：§6.4 Jevals｜**中**）；LangChain 把路由失败回退 `default_route` 作为设计原则（依据：§7.4｜**中**）。
- **风险**：
  1. **准确率不占优**：Jevals 的 choice 任务（Banking77）Jev 被 Gemini 3.8 Flash 显著领先（依据：§6.4｜**中**）；LiteLLM 长用例匹配率 87.50% 低于短用例 97.50%（依据：§6.4｜**中**）；JevBench 里 Luna(low) 的 Intelligence/Calibration 高于 Jev（依据：§6.4｜**中**）。
  2. **选项顺序是输入的一部分**：重排选项可能改变答案（依据：§7.7【官方·集成方】｜**中**；另有【未证实】的社区复现，§12.2④）。
  3. 一旦用于"选择能力档位"，错误会改变下游成本与质量，需要按 §13.3 的方式把不确定性透出。
- **结论**：**推荐试点**（试点范围限定在非破坏性的模型路由/分层）。

#### 角色 2：动作门控与风控（Tool-call gating / guardrail）

- **建议**：**可以做，但必须是"多防线之一"**。若要用，遵循三条硬约束：**fail-closed + 确定性检查并行 + 只把"用户消息 + 待判定动作 + 判定必需的廉价事实"放进 state**。
- **成立条件**：
  1. state 绝不能包含 Agent 自己抓取的内容（工具输出、助手消息）——否则不受信内容可以为自己的执行授权（依据：§7.3 LangChain `AutoModeMiddleware` 的实现注释｜**中**）；
  2. 拒绝路径必须可恢复（把拦截作为工具结果回给模型，让它换做法），并有超时/熔断/降级与人工升级通道（依据：§7.4｜**中**）；
  3. 阈值按"错误代价方向"设置：**错误的放行代价高 → 降低阈值/提高门槛；误拦代价高 → 提高阈值**（依据：§8.3【官方】｜**高**）；
  4. 与确定性规则**并用**——例如"路径白名单/参数正则/危险命令表"必须先跑一遍（依据：§7.8 Pydantic「alongside deterministic checks, not instead of them」｜**中**）。
- **支持证据**：该模式已被两套主流框架固化为工程约定（LangChain `AutoModeMiddleware` 默认 `risk_threshold=0.2`、fail-closed；Pydantic `before_tool_execute` + `SkipToolExecution`）（依据：§7.3、§7.4｜**中**）。
- **风险（本项目最需要警惕的一类）**：
  1. **提示注入可以移动判定**：官方原文 *can move the answer*；单次实验里 block 概率从 0.76 掉到 0.48、confidence 从 0.64 掉到 0.22（依据：§7.8 Octomind｜**低—中**，单一来源、非 benchmark）；
  2. **官方明确它不是安全边界**："Nothing here is a security boundary."（依据：§4.2【官方】｜**高**）；
  3. **可用性数据不利**：`pi-warden` 18,075 次受保护调用触发 48 次 hold，13 个被标注的 hold 里只有 2 个站得住（另 11 个重试后通过）（依据：§7.3【第三方】｜**低—中**，项目自测）；111 例门控研究里 Jev 与 Claude 各有 1 例"不安全却放行"（依据：§6.7【未证实·索引转述】｜**低**）。
- **结论**：**可试点但不可依赖**；在注入实测（§13.4 假设 3）通过前，不能让 Jev 成为某个不可逆动作的唯一守门人。

#### 角色 3：评估 / Judge（Eval / Scoring / Verification）

- **建议**：**最推荐的试点角色**。用 Jev 对**全量**轨迹做廉价覆盖式打分（rubric / 封闭集合），把失败与不确定样本**抽样**送给 LLM judge 拿解释——这正是 Arize 建议的混合架构。
- **成立条件**：
  1. 判定是**重复、高量、答案事前已知**的（依据：§7.3 Langfuse 的适用条件｜**中**）；
  2. 接受"**没有解释**"这一硬限制：需要理由时必须另接生成模型（依据：§9.4 Arize｜**中**）；
  3. 代码侧要处理"**Noul 没有 confidence 字段**"，不能对一切答案统一读 `confidence`（依据：§7.3 Langfuse｜**中**）。
- **支持证据**：LangChain 500/500 与人工 oracle 一致、每例分数方差 0.0000149（比 Terra 低 913x）、每次 $0.00035（依据：§6.4 LangChain｜**中**，但样本仅 5 条、作者自称 observational、未记录服务版本）；LangSmith Evals / Langfuse / Braintrust 三条产品线都已把 Jev 接成 judge（依据：§7.3｜**中**）。
- **风险**：
  1. "**低方差 ≠ 正确**"，且 LangChain 自警 *low cost can amplify mistakes - a consistently wrong evaluator can produce bad feedback at scale*（依据：§6.4｜**中**）；
  2. 组合语义问题：单步概率不构成流水线可靠性，误差相关性无人能报（依据：§4.4 temperaturezero、§6.6 Maio｜**中**）；
  3. 若评估结果直接驱动自动化改进（例如把失败原因喂回 coding agent），错误的稳定性反而会被放大——**建议给评估结果加"抽样人工复核 + 版本门禁"**。
- **结论**：**推荐作为第一优先试点**，但评估结论本身要有抽样复核。

#### 角色 4：信息抽取（Structured Data Extraction）

- **建议**：**限定用途可用**——只用于**封闭集合/有序/是非**类字段；日期、数值、计数一律「抽取归模型、运算归代码」。
- **成立条件**：
  1. 目标字段可枚举（做成 Choice，并在列表里加"未提及"选项）（依据：§4.1 jaggedness #3【官方】｜**高**）；
  2. 需要精确定位时先在代码里用正则或生成模型取候选，再让 Jev 在候选里选（依据：§4.1 #9【官方】｜**高**）；
  3. 不接受用 Score 期望值反推数值——只能用于过阈值（依据：§4.1 #2、§8.2【官方】｜**高**）。
- **支持证据**：官方场景地图把 Structured Data Extraction 列为独立决策形状，并给出 Date extraction / Pre-parsed value extraction 的 cookbook 目录条目（依据：§7.1、§7.2｜**中**，cookbook 未逐条展开）。
- **风险**：
  1. 计数、算术、日期比较是官方点名的失败模式（依据：§4.1｜**高**）；
  2. 高基数（>240）需要两阶段、变慢（依据：§3.4.1【官方发布博客】｜**中**）；
  3. 长 state 抽取精度下降（context rot）（依据：§4.3｜**中**）。
- **结论**：**可用，但应限制为"LLM 负责开放式抽取、Jev 负责封闭判定"的分工**。

#### 次级候选与明确排除

| 角色 | 建议 | 依据与可信度 |
| --- | --- | --- |
| **技能 / 工具集选择** | **可用**：每个 skill/工具独立打分，选择结果决定前置哪些指令；失败时"不添加任何技能"（fail open）。 | §7.3 LangChain `SkillsMiddleware`、§7.2 官方 cookbook 16.8%→7.3%｜**中** |
| **上下文筛选 / 压缩** | **可用但要注意容量**：Jev 侧 64k/32k 限制、超限会失败、挂了 `FallbackModel` 时超长会话会被静默交给 LLM（成本模型变化）。 | §7.3 Pydantic、§7.3 社区插件｜**中** |
| **终止 / 完成判断** | **不要用**：未找到官方形态；"是否真的做完"需要读 diff 与请求，属于 System Two。 | §7.3 决策点 6、§4.1 #9｜**高（负向结论）** |
| **安全边界 / 抗恶意输入的唯一防线** | **不要用**：官方明确"不是安全边界"，且 state 默认不被当作敌意输入。 | §4.2【官方】｜**高** |
| **生成 / 解释 / 多跳推理** | **不要用**：不生成、无解释、跳数一多就退化。 | §2.4、§4.1、§4.2【官方】｜**高** |

### 13.2 决策准则：什么交给 Jev、什么交给 LLM、什么交给确定性代码

**三层分工（按顺序判断，先到先得）**

| 层 | 判据 | 典型任务 | 依据与可信度 |
| --- | --- | --- | --- |
| **确定性代码** | 有唯一正确答案、可复现、必须可审计；或本身就是策略/权限/金额/时间运算 | 算术与计数、日期比较与排序、阈值比较与路由表、状态机与循环控制、权限/策略校验、重试与幂等、金额与配额 | §4.1 #2/#3【官方】、§8.6 官方 meta-rule｜**高** |
| **Jev（快速判定层）** | 候选答案是**预先枚举的有限集合 / 是非 / 有序档位**；没有标注数据；需要概率来门控或排序；延迟敏感；失败可回退 | 模型/工具路由、技能选择、上下文筛选、全量轨迹打分、封闭字段抽取、门控的第一道意见 | §9.5 决策树【第三方+官方】、§2.4 官方对照表｜**中—高** |
| **LLM** | 需要**生成**（文本/代码/摘要）、需要**解释**、需要多跳或开放域推理、判定频率低而单次价值高、或需要"读全文才能判断"（如完成度） | 主 agent 循环、推理链、写代码与工具调用、judge 的解释、失败原因分析、多跳规划 | §2.4【官方】、§4.2【官方】、§9.4【第三方】｜**高** |

**四条交叉判据（决定"用不用 Jev"，比"哪个模型更强"更重要）**

1. **错误代价方向**：错了只是浪费一次调用 → 可以用 Jev 并放宽阈值；错了会不可逆 → 必须叠加确定性检查/人工确认（依据：§8.3 官方 Noul 双阈值｜**高**）。
2. **错误是否可被发现**：Jev 的输出没有解释、会静默进入下游（依据：§4.5 temperaturezero｜**中**）→ 不可发现的错误必须抽样复核。
3. **是否需要解释/审计**：需要"为什么"的场景必须配 LLM judge 或生成模型（依据：§9.4 Arize｜**中**）。
4. **是否需要绝对可靠**：需要 → 确定性规则优先，Jev 只能作为辅助信号（依据：§4.2【官方】"not a calculator"、"not a security boundary"｜**高**）。

**一句话准则**：**能用代码判断的绝不调用模型；能用 Jev 判断的绝不调用 LLM；只有需要生成、解释或多跳推理时才调用 LLM。**（依据：AGENTS.md 的决策分层原则 + §8.6 官方 meta-rule + §9.5｜**中—高**）

### 13.3 对 Rust 架构的启示

#### 13.3.1 统一抽象与可替换后端（最高优先级）

- **做法**：把抽象建在「**决策**」上，而不是「模型调用」上——输入 `state + typed questions`，输出 `typed answers + probabilities + usage`；让 **Jev / LLM 结构化输出 / 本地分类器 / 确定性规则** 都实现同一组 trait，降级 = 换后端，而不是改调用点。
- **依据**：§9.5 定位结论（【第三方】+官方 workflow evals 的 "System One LLM wrapper" 思路）｜**中—高**；社区 crate `system-one` 的 backend-agnostic 设计（`JevHttp`/`Mock`/`Replay`/`LocalLogprob`/`Shadow`）证明该形态在 Rust 里可落地（§10.2｜**低—中**，仅设计参考）。
- **直接收益**：Jev 不可用（429/529/无 key/内部试点未放量）时，同一链路可切到 LLM 结构化输出或本地规则；这在 early access + 无 SLA 的现状下是必要条件（依据：§5.5、§11.2｜**高**）。

#### 13.3.2 类型化 question 枚举与编译期约束

- **做法**：`Question` 用 `#[serde(tag = "type", rename_all = "snake_case")]` 内部标签枚举；用标记类型把问题类型与答案类型绑定（`Id<ChoiceKind>` 取不到 choice 以外的答案）；**Noul 不带 `confidence`** 这件事用类型表达（而不是 `Option<f64>`），使"对 Noul 索要 confidence"成为编译错误；Choice ≤255、Score 2–10、state + 最长问题 ≤32k 在构造期校验。
- **依据**：§10.3 的 8 项编译验证与设计含义（本轮自测｜**中**——验证局限于官方示例形状）；§3.4 三型字段差异（【官方】｜**高**）。
- **注意**：serde 默认静默丢弃未知字段；若要严格对齐 wire 契约需 `deny_unknown_fields`，并为未来新增 `type` 预留 `#[serde(other)]` 兜底（依据：§10.3｜**中**）。

#### 13.3.3 概率作为一等数据

- **做法**：
  - 保存原始 `probabilities` / `confidence` / `noul`，**不塌缩为布尔**；配置项暴露"阈值"而不是"布尔开关"；
  - **Noul 与 Choice 的概率不可互相换算**，类型系统与命名都要区分（`noul` 是绝对概率、Choice 是相对分布）（依据：§4.1 #8【官方】｜**高**）；
  - 每条判定记录 **实际模型版本（响应 `model` 字段）、选项顺序、request_id、阈值、判定结果与置信度**——这正是第三方治理清单要求的字段（依据：§7.8 VentureBeat 六问、§5.1 官方 pin 建议｜**中**）。
- **直接收益**：可事后做校准分档统计（"标称 0.8 是否真是 80%"）与阈值再校准，且版本漂移可归因。

#### 13.3.4 打包提问优先于并发

- **做法**：在循环里做**问题批收集**——把一个 state 下的路由、紧急度、门控、校验等问题合并成一次请求（官方称"问一个可能不需要的问题几乎是免费的"）；**推测式扇出**（把只在部分分支用得上的问题一起问掉，由代码决定读哪些）；只有在"必须先拿到第一个答案才能构造第二个请求"时才发第二次请求。
- **依据**：§3.6、§8.4（【官方】｜**中—高**；收益数字有 12.2×/10.0× 与 11.5×/9.6× 两套口径，且第三方有"批处理改变个别判定"的反例 §8.4）。
- **并发**：真正的并发留给**不同 state**（多文档分类、多候选择优）；并发数集中在一个**全局限流器**之后，而不是每个调用点各自退避（依据：§10.4｜**中**）。
- **注意**：必须自测批处理对判定的影响（§13.4 假设 7）。

#### 13.3.5 限流 / 超时 / 重试集中管理

- **做法**：
  - 全局令牌桶，以官方 **250k tok/s / 1,200 rpm** 为**软上界**（官方声明动态调整、可能无通知变动）（依据：§5.3【官方】｜**高**）；
  - 退避只对 `408/429/5xx`，遵循 `retry-after`/`retry-after-ms`；区分"单次 HTTP 超时"与"整次调用的重试总预算"（Python SDK 的 `timeout` 是后者，容易误读）（依据：§3.8、§10.4｜**中**）；
  - **不重放"结果未知"的请求**（计费语义不明）（依据：§3.6【未证实】｜**低**，但保守处理成本低）；
  - 熔断 + 降级：降级目标可以是确定性规则或 LLM 判定；**降级必须是显式的、可观测的**，不能静默（依据：§7.4｜**中**）；
  - 超时预算与 `state` 规模挂钩（大文档场景官方 cookbook 出现过 120s 的用法）（依据：§10.4｜**低—中**）；
  - 版本 pin：请求用版本化 ID（如 `jev-1.13.0`），把别名漂移交给显式升级流程（依据：§5.1【官方】｜**高**）。
- **UI/事件流**：把 Jev 调用建模成"一次原子调用、立即完成"的事件分支，**不要伪造流式**（依据：§3.8、§10.4｜**高（负向结论）**）。

#### 13.3.6 配置与可观测性（落地细节）

- 阈值集中在配置/常量表中（官方 RAG cookbook 的做法："改路由策略是常量编辑，可走 code review"）（依据：§8.3【官方】｜**中**）；
- 每个门控/路由的失败要有独立指标（fail-open 与 fail-closed 分开计数、区分"判为安全"与"未能判定"）（依据：§7.4｜**中**）；
- 记录供应商侧信息用于运维：`x-typesafe-request-id`、`X-TypeSafe-Retry-Count` 等 SDK 源码可见的头（依据：§3.1【官方源码】｜**低—中**，非文档承诺，注意可能变化）。

### 13.4 上线前必须实测验证的假设

> 以下每一项都是**当前公开证据无法回答、但会直接决定架构或上线范围**的问题。每项给出「为什么必须测」「最小验收方向」「依据」。实验步骤见 §14.3。

| # | 待验证假设 | 为什么必须测 | 最小验收方向 | 依据与可信度 |
| --- | --- | --- | --- | --- |
| 1 | **中文 / CJK 的实际表现与置信度行为** | 官方只说 "handled but not equally well"、*currently have lower accuracy*，**零量化数字**；本项目的输入很可能以中文为主 | 在自有中文样本上对比中/英文 prompt 的准确率与 confidence 分布；确定"是否需要英译后再判定"以及是否要更强的低置信兜底 | §4.3、§8.7【官方】无数字｜**高（不确定性高）** |
| 2 | **校准质量：标称 0.8 是否真是 80%** | 官方无公开校准曲线；唯一直接测 ECE 的第三方实验显示 Jev ECE 0.154 **差于** Haiku 0.097；Jevals 提示校准随任务变化 | 在自有带真值数据集上画可靠性图 / 分档准确率（含 ECE），并按任务分别统计；据此定阈值与人工兜底比例 | §4.4、§6.4、§6.7｜**中** |
| 3 | **提示注入下的门控可靠性** | 官方承认 state 默认不被当作敌意输入、注入"can move the answer"；公开只有一次单命令实验（0.76→0.48）；门控错了不可逆 | 构造注入样本集（伪造 tool-output/预先授权/自我分类论证），测 block 概率与 confidence 的漂移幅度、以及"不安全却放行"率；据此决定门控是否可用、是否必须叠加确定性规则与人工确认 | §7.8、§4.2｜**中（方向确定、量级未知）** |
| 4 | **版本漂移与 pin 策略** | `jev-latest` 会静默漂移；v1 相对 preview 换过 confidence 算法（数值会变、需重新校准阈值）；官方无 changelog | 固定版本化 ID；建立"新版本 → 重跑校准与阈值实验"的门禁流程；在 trace 中记录响应 `model` 字段并统计版本分布 | §3.7、§5.1、§5.7｜**高** |
| 5 | **命名冲突：`jev-harness` crate 已被占用（0.1.4，2026-09-21）** | 若本项目计划发布 crate（或已被下游引用），名字冲突会导致发布失败或工具链混淆；**其作者身份与是否与本项目相关未核实** | 由项目决策者确认命名策略（改名 / 私有 / 联系现持有者），并确认是否曾用过该名字 | §10.2、§10.5 #7｜**中** |
| 6 | **单一供应商容灾** | 无 SLA、单一 provider、发生过 59 分钟级中断、上线期曾无法服务用户、限流动态调整、OpenRouter 是否有第二 provider 未裁定 | 演练：限流 429/529 → 退避 → 熔断 → 降级（规则或 LLM）→ 恢复；明确每分钟/每小时的降级预算与人工升级路径 | §5.5、§11.2、§11.4｜**高** |
| 7 | **批处理对判定的影响** | 官方称"批处理不改变答案、不增加方差"，但第三方 `jev-orderby-bench` 记录"40 行批处理会把原本通过的排序门变成失败"，方向相反 | 在自有样本上对同一批问题跑"一次多问"与"逐问"两种策略，比较答案一致率、闸门通过率与延迟成本 | §3.6 vs §8.4｜**中（冲突未解）** |
| 8 | **计费与限流的真实边界** | 最低计费单位、失败/重试是否计费、429 触发点、实际 p95/p99、并发上限**均未公开**；成本模型是架构前提 | 拿到 key 后做小规模探针：验证"state 只计费一次"、重试是否重复计费、限流触发阈值与退避效果 | §3.6【未证实】、§5.7、§6.9｜**中** |
| 9 | **结构化输出与 serde 的边界行为** | 本轮验证只覆盖官方示例形状；`probabilities` 是否严格和为 1、`usage` 为 null、未知 `type`、答案缺失 key 等均未测 | 用真实响应跑一轮"契约回归"：断言形状、未知字段策略、缺失字段与错误体结构 | §3.9、§10.5 #10｜**中** |
| 10 | **长 state 的可用容量** | 官方只有"context rot"的定性描述与 54k 字符的样例；32k 的硬约束与第三方渠道 32K 口径需要实测确认 | 用本项目的真实文档规模测准确率随 state 长度的衰减曲线，据此定"先检索再判定"的裁剪阈值 | §4.3、§5.4｜**中** |

### 13.5 建议的定位结论与红线

**一句话定位**：**Jev 是 harness 里的「快速判定层」——为已经枚举好的决策提供带概率的答案；它不是主模型、不是安全边界、不是精度天花板。**

**三条红线（在实测通过前不要越过）**

1. **不可逆动作不能只靠 Jev 守门**：必须有确定性检查或人工确认（依据：§4.2 官方"不是安全边界"、§7.8，**高**）。
2. **不能把 Jev 的输出当作"真值"进入训练或对外产品决策**：准确率不占优、无解释、校准未验证；且 MCA 2.3(b) 禁止蒸馏与开发竞品（依据：§6.4、§9.3、§11.3，**中—高**）。
3. **不能把 Jev 当作唯一的判定路径**：early access、无 SLA、限流动态、单一 provider，必须有降级实现（依据：§5.5、§11.4，**高**）。

**建议的推进顺序**（每一步都以 §13.4 的实测结果为门槛）

1. 先做**只读、可回退**的试点：模型路由 / 技能选择 / 全量轨迹打分（judge）；
2. 再评估**门控**：只在不含不可逆动作、且已有确定性规则兜底的工具上试点；
3. 最后才考虑把 Jev 的判定写进**成本或质量的关键路径**，并保持"可替换后端"的抽象不变。

### 13.6 本节不确定项

1. 本节的建议建立在**公开材料**之上，**本项目尚未对线上 API 做任何真实调用**；因此所有"成立条件"都需要在 §14.3 的实验后复核。
2. **角色推荐排序（judge > 路由 > 抽取 > 门控）是基于证据强度与失败代价的推断**，不是任何来源的结论；若本项目的输入以中文为主，排序可能整体后移（假设 1 未验证）。
3. 门控角色的可用性判断**高度依赖注入实测**：公开证据只有一次单命令实验（0.76→0.48）与项目自测的 hold 数据，且官方没有给出攻击成功率。
4. **成本结论依赖定价不变**：官方自认"无法证明定价未被补贴"，且预期长期价格会降——若价格上调，成本类论断需重算（依据：§5.2、§11.5，**中**）。
5. 对"终止/完成判断"的排除是**基于"未找到官方形态"的负向结论**；若官方后续发布该模式，应重新评估。
6. 架构建议中的 serde 结论来自一次性编译验证（§10.3），**不是对线上契约的验证**；线上契约变更后需重跑。

---

## 14. 附录

### 14.1 附录 A：来源索引（按主题聚类，全部 URL 保留）

> 说明：本索引收录 4 份片段中出现的全部来源 URL；**「未采用 / 不可引用」一节单列**，避免误引。同一来源在正文中多次出现时此处只列一次。

#### A-1 TypeSafe 官方：概念、发布与叙事

- <https://typesafe.ai/blog/introducing-system-one-models-and-jev>
- <https://docs.typesafe.ai/concepts/system-one>
- <https://docs.typesafe.ai/introduction/machine-learning-primer>
- <https://docs.typesafe.ai/introduction>
- <https://docs.typesafe.ai/introduction/coding-agents>（亦作 <https://docs.typesafe.ai/introduction/coding-agents.md>）
- <https://docs.typesafe.ai/confidence>
- <https://typesafe.ai/>
- <https://typesafe.ai/manifesto>
- <https://typesafe.ai/team>
- <https://typesafe.ai/blog/antibenchmaxxing>
- <https://docs.typesafe.ai/model-jaggedness/jev-1.13>
- <https://docs.typesafe.ai/concepts/use-case-map.md>
- <https://docs.typesafe.ai/concepts/how-to-build-with-system-one>
- <https://docs.typesafe.ai/agent-skill>
- <https://docs.typesafe.ai/sitemap.xml>
- <https://docs.typesafe.ai/llms.txt>、<https://docs.typesafe.ai/llms-full.txt>

#### A-2 TypeSafe 官方：API 契约与三类原语

- <https://docs.typesafe.ai/api>
- <https://docs.typesafe.ai/models>
- <https://docs.typesafe.ai/primitives>
- <https://docs.typesafe.ai/primitives/choice>
- <https://docs.typesafe.ai/primitives/score>
- <https://docs.typesafe.ai/primitives/noul>
- <https://docs.typesafe.ai/primitives/advanced>
- <https://docs.typesafe.ai/concepts/state>
- <https://docs.typesafe.ai/migrating-to-v1.md>
- <https://docs.typesafe.ai/introduction/quickstart>
- <https://docs.typesafe.ai/patterns>、<https://docs.typesafe.ai/patterns/fan-out>、<https://docs.typesafe.ai/patterns/composite-scoring>、<https://docs.typesafe.ai/patterns/confidence-routing>

#### A-3 TypeSafe 官方：cookbook

- <https://docs.typesafe.ai/cookbooks/parallel_questions>
- <https://docs.typesafe.ai/cookbooks/classification_using_confidence>
- <https://docs.typesafe.ai/cookbooks/consistency_noul_cookbook>
- <https://docs.typesafe.ai/cookbooks/consistency_choice_cookbook>
- <https://docs.typesafe.ai/cookbooks/classifying_rag_passages>
- <https://docs.typesafe.ai/cookbooks/entity_alignment>
- <https://docs.typesafe.ai/cookbooks/rerank_typesafe>
- <https://docs.typesafe.ai/cookbooks/skill_suggestion>
- <https://docs.typesafe.ai/cookbooks/function_calling.md>
- <https://docs.typesafe.ai/cookbooks/llm_guardrails.md>
- <https://docs.typesafe.ai/cookbooks/hierarchical_classification>
- <https://docs.typesafe.ai/cookbooks/sde_cascade>

#### A-4 TypeSafe 官方：SDK 文档、源码与包元数据

- <https://docs.typesafe.ai/sdk>、<https://docs.typesafe.ai/sdk.md>
- <https://docs.typesafe.ai/sdk/python/api/constants.md>（亦作 <https://docs.typesafe.ai/sdk/python/api/constants>）
- <https://docs.typesafe.ai/sdk/python/api/clients/sync.md>
- <https://docs.typesafe.ai/sdk/python/api/types/responses.md>
- <https://docs.typesafe.ai/sdk/python/api/exceptions.md>
- <https://docs.typesafe.ai/sdk/python/api/retries.md>（亦作 <https://docs.typesafe.ai/sdk/python/api/retries>）
- <https://docs.typesafe.ai/sdk/javascript>
- <https://docs.typesafe.ai/sdk/javascript/api/classes/TypeSafeClient.md>
- <https://docs.typesafe.ai/sdk/javascript/api/interfaces/RetryPolicy.md>（亦作 <https://docs.typesafe.ai/sdk/javascript/api/interfaces/RetryPolicy>）
- <https://pypi.org/pypi/typesafe-sdk/json>
- <https://registry.npmjs.org/@typesafe-ai/sdk>
- <https://github.com/typesafe-ai/typesafe-sdk-python>
- <https://github.com/typesafe-ai/typesafe-sdk-js>
- <https://raw.githubusercontent.com/typesafe-ai/typesafe-sdk-python/main/src/typesafe_sdk/_core/constants.py>
- <https://raw.githubusercontent.com/typesafe-ai/typesafe-sdk-python/main/src/typesafe_sdk/_core/errors.py>
- <https://raw.githubusercontent.com/typesafe-ai/typesafe-sdk-js/main/src/client.ts>
- <https://api.github.com/orgs/typesafe-ai/repos>

#### A-5 TypeSafe 官方：评测站、状态页、控制台、法律与合规

- <https://evals.typesafe.ai/>
- <https://evals.typesafe.ai/customer_service.html>
- <https://status.typesafe.ai/>
- <https://console.typesafe.ai/keys>、<https://console.typesafe.ai/login>、`console.typesafe.ai/playground`、`console.typesafe.ai/docs/cookbooks`
- <https://typesafe.ai/legal/mca>
- <https://typesafe.ai/legal/terms>
- <https://typesafe.ai/legal/privacy-policy>
- <https://typesafe.ai/data-processing>
- <https://docs.typesafe.ai/legal>
- <https://trust.typesafe.ai/>

#### A-6 第三方：集成方官方文档（平台 / 框架）

- Cloudflare Workers AI：<https://developers.cloudflare.com/ai/models/typesafe/jev/>
- Pydantic AI：<https://pydantic.dev/docs/ai/models/typesafe/>
- LangChain：<https://docs.langchain.com/oss/python/integrations/providers/typesafe>、<https://github.com/langchain-ai/langchain/pull/40556>、<https://pypi.org/project/langchain-typesafe/>
- LiteLLM：<https://docs.litellm.ai/docs/pass_through/typesafe>、<https://docs.litellm.ai/docs/proxy/auto_routing>、<https://docs.litellm.ai/docs/auto_router/setup>
- Vercel：<https://vercel.com/changelog/typesafe-ai-jev-now-available-on-ai-gateway>、<https://vercel.com/ai-gateway/models/jev>、<https://vercel.com/blog/ai-gateway-jev-model-launch>
- Netlify：<https://www.netlify.com/changelog/typesafe-jev-ai-gateway/>
- OpenRouter：<https://openrouter.ai/typesafe>、<https://openrouter.ai/typesafe/jev-1.13>、<https://openrouter.ai/~typesafe/jev-latest>、<https://openrouter.ai/api/v1/models/typesafe/jev-1.13/endpoints>
- Requesty：<https://www.requesty.ai/blog/typesafe-jev-explained>
- Opper：<https://opper.ai/provider/typesafe>
- Spring AI：<https://spring.io/blog/2026/09/21/spring-ai-typesafe-structured-judgment>
- LangSmith Evals：<https://www.langchain.com/blog/jev-is-now-available-in-langsmith-evals>
- Langfuse：<https://langfuse.com/blog/2026-09-18-using-typesafes-jev-for-evals>
- Braintrust：<https://www.braintrust.dev/blog/evaluate-agent-responses-with-jev>
- Arize：<https://arize.com/blog/typesafe-jev-llm-judge/>

#### A-7 第三方：受控实验与独立基准

- LiteLLM Auto Router 基准：<https://docs.litellm.ai/blog/jev-auto-router-benchmark>
- LangChain Jev-as-a-Judge：<https://www.langchain.com/blog/jev-agent-evals-langsmith>
- LangChain harness 指南：<https://www.langchain.com/blog/building-a-harness-with-jev>
- Jevals：<https://jevals.com/>、<https://jevals.com/notes/2026-09-18/>
- JevBench v1.2：<https://github.com/fstandhartinger/jevbench/blob/main/RESULTS-v1.2.md>
- 钓鱼邮件基准：<https://github.com/anisselbd/jev-phishing-bench>
- 垃圾邮件评测：<https://github.com/bitnovus/jev-spam-eval>
- Jagent（有商业利益）：<https://jev-agent.com/benchmarks>
- NearHere 用例研究：<https://nearhere.events/blog/typesafe-jev-mistral-gemini-event-validation>
- Every（付费墙）：<https://every.to/also-true-for-humans/mini-vibe-check-typesafe-s-jev-judged-everything-i-ve-written-in-0-7-seconds>
- Vercel 侧 Banking77 对照：<https://github.com/vercel/ai/issues/20846>
- 边界行为研究：<https://github.com/nikkoxgonzales/jev-certify>、<https://github.com/yodablocks/jev-orderby-bench>、<https://github.com/jujumilk3/jev-calibration-audit>、<https://github.com/scienthoon/jev-ood-calibration>、<https://github.com/ghubnab99/jev-enterprise-decision-fabric>、索引 <https://github.com/AbdelStark/awesome-typesafe>
- 自建工具与示例：<https://pi.dev/packages/pi-warden>、<https://gist.github.com/samuelcolvin/fa2d9abf8349b18e360f2d326218ffa7>、<https://github.com/tamaratran/fast-jev-compaction>
- 应用侧综述：<https://www.ayautomate.com/blog/how-to-use-jev-guide>、<https://dev.to/valyuai/how-to-use-jev-a-practical-guide-to-typesafes-system-one-model-g5e>、<https://dev.to/gabrielanhaia/jev-beat-gpt-luna-by-1-point-gpt-6-and-claude-wrote-the-answer-key-314k>、<https://www.cloudraft.io/blog/top-use-cases-of-jev-typesafe-ai-model>、<https://hexdocs.pm/typesafe_ai/readme.html>
- 其他转述：<https://www.explainx.ai/blog/langchain-jev-agent-evals-benchmark-september-2026>（对 LangChain judge 数字的第三方转述，属交叉确认而非独立复现；该站同时出现在 §12.4 的疑似 AI/SEO 站点清单中，仅作追溯用）

#### A-8 第三方：批评、报道与社区讨论

- 批评与评论：<https://kingy.ai/blog/typesafe-jev-review-the-ai-model-that-doesnt-generate-text/>、<https://www.truefoundry.com/blog/typesafe-ai-jev>、<https://www.laura-martel.com/blog/typesafe-jev-system-one-models>、<https://temperaturezero.com/2026/09/16/jev-doesnt-hallucinate-it-decides-wrong/>、<https://flowtivity.ai/blog/jev-typesafe-ai-decision-model/>、<https://anthonymaio.substack.com/p/jev-the-language-model-that-wont>、<https://www.firecrawl.dev/blog/what-is-jev>、<https://xhinker.medium.com/jev-the-most-overrated-model-of-2026-even-before-the-year-end-0f86838ca419>、<https://www.kdnuggets.com/what-everyone-is-getting-wrong-about-typesafe-ais-jev>、<https://dev.to/gabrielanhaia/jev-beat-gpt-luna-by-1-point-gpt-6-and-claude-wrote-the-answer-key-314k>、<http://wimes.org/articles/2026-09-16-typesafe-jev-cant-hallucinate-exactly-the-problem/>、<https://arize.com/blog/choosing-the-best-llm-evaluation-model/>
- 媒体报道：<https://www.theregister.com/ai-and-ml/2026/09/16/typesafe-ai-debuts-model-for-machines-that-plays-doom/5296711>、<https://www.marktechpost.com/2026/09/19/typesafe-ai-releases-jev/>、<https://techcrunch.com/2026/09/18/a-new-kind-of-ai-model-from-a-chatgpt-inventor-is-thrilling-developers/>、<https://venturebeat.com/security/companies-are-putting-jev-in-charge-of-ai-agent-decisions-and-prompt-injection-can-influence-the-verdict>、<https://www.hpcwire.com/aiwire/2026/09/16/typesafe-ai-emerges-from-stealth-with-40m-in-funding-with-new-model-for-composable-ai/>、<https://www.businesswire.com/news/home/20260915525333/en/TypeSafe-AI-Emerges-From-Stealth-With-%2440M-in-Funding-With-New-Model-for-Composable-AI>、<https://finance.yahoo.com/technology/ai/articles/typesafe-ai-emerges-stealth-40m-190000776.html>、<https://www.dcvc.com/news-insights/typesafe-emerges-from-stealth-with-a-new-way-of-doing-ai>、<https://www.finsmes.com/2026/09/typesafe-ai-raises-40m-in-seed-funding.html>、<https://seedtable.com/companies/typesafe-ai/funding-rounds/seed-2026-09>、<https://dealroom.co/news/151032-typesafe-exits-stealth-with-40m-seed-to-build-ai-for-software-not-people/>、<https://www.forbes.com/sites/the-prompt/2026/09/15/this-200-million-startup-wants-to-fix-ais-overconfidence-problem/>
- 播客与访谈：<https://www.latent.space/p/jev>、<https://www.latent.space/p/ainews-jev-a-system-one-model-that>
- HN 讨论：<https://news.ycombinator.com/item?id=49717558>、<https://hacker-news.firebaseio.com/v0/item/49717558.json>、<https://news.ycombinator.com/item?id=49719001>、<https://news.ycombinator.com/item?id=49719144>、<https://news.ycombinator.com/item?id=49720601>、<https://news.ycombinator.com/item?id=49758669>
- Reddit：<https://www.reddit.com/r/LocalLLaMA/comments/1wijo3e/>、<https://www.reddit.com/r/LocalLLaMA/comments/1wlfmgq/>、<https://www.reddit.com/r/LocalLLaMA/comments/1whxf90/>、<https://www.reddit.com/r/PiCodingAgent/comments/1whsav6/anyone_else_testing_out_typesafe_ais_new_system/>、<https://www.reddit.com/r/AI_Agents/comments/1wl82fr/tried_typesafe_ais_jev_vs_a_regular_llm_for_model/>
- 中文侧：<https://m.sohu.com/a/1079019769_129720>、<https://k.sina.com.cn/article_5953466437_162dab0450670bd10e.html>、<https://k.sina.com.cn/article_1644983660_620c756c02001vwuw.html>、<https://www.cnblogs.com/sing1ee/p/23040079>
- 法律/隐私分析（第三方，含可验证错误，见 §11.3）：<https://wunderlandmedia.com/typesafe-ai-jev-terms-of-service-gdpr>、<https://wonderwhy-er.medium.com/typesafe-jev-wont-train-on-your-data-it-can-still-learn-from-it-563f0ad591b6>
- 公司/招聘侧：<https://www.linkedin.com/company/typesafe-ai>、<https://www.linkedin.com/posts/robbroderick_after-two-years-in-stealth-diogo-almeida-activity-7506785550129864704-7UYQ>、<https://jobs.ashbyhq.com/typesafe-ai>、<https://www.glassdoor.com/job-listing/jv?jl=1009970275998>
- 监控类（意义有限）：<https://piehost.com/status/typesafe.ai>

#### A-9 竞品、替代技术与背景参考

- LLM-as-judge 能力与偏置：<https://arxiv.org/abs/2306.05685>、<https://arxiv.org/abs/2508.06225>、<https://arxiv.org/abs/2404.13076>、<https://aclanthology.org/2025.ijcnlp-long.18.pdf>
- 语义路由：<https://www.lmsys.org/blog/2024-07-01-routellm/>、<https://arxiv.org/abs/2406.18665>、<https://arxiv.org/html/2510.08731v1>、<https://vllm.ai/blog/2025-10-27-semantic-router-modular>、<https://github.com/aurelio-labs/semantic-router>
- 分类器与标注效率：<https://huggingface.co/blog/setfit>
- guardrail：<https://huggingface.co/meta-llama/Llama-Guard-4-12B>、<https://developers.openai.com/api/docs/guides/moderation>、<https://www.premai.io/blog/production-llm-guardrails-nemo-guardrails-ai-llama-guard-compared/>
- 价格参考：<https://platform.claude.com/docs/en/models/haiku-4-5/overview>、<https://developers.openai.com/api/docs/pricing>
- 开源替代/相似方案：<https://huggingface.co/knowledgator/gliclass-instruct-base-v1.0>、<https://huggingface.co/fastino/gliner2.5-base-v1>、<https://laya.convaiinnovations.com/>、<https://arxiv.org/abs/2503.23303>
- 官方集成配套：<https://docs.typesafe.ai/agent-skill>

#### A-10 Rust 生态（crates.io / docs.rs / GitHub）

- crates.io 关键词枚举：<https://crates.io/api/v1/crates?keyword=jev>（另 `keyword=system-one`）
- 设计参考（不代表可用性背书）：<https://docs.rs/system-one/latest/system_one/>、<https://docs.rs/typesafeai-sdk-community/latest/typesafeai_sdk_community/>、<https://docs.rs/jev-sdk/latest/jev_sdk/>
- 相关仓库：`douglance/typesafe-sdk-rs`、`Ketankhunti/typesafe-sdk-rust`、`community-ports/typesafeai-sdk-rust-community`、`kunobi-ninja/kunobi-jev`、`bitomule/jevi`、`portlandhodl/jev-sdk`（**404**）、`luizribeiro/jevrs`、`zcoder-run/rust-sysone`、`heyaozh/system-one`、`thehumanworks/jevgrep`、`JedimEmO/typesafe-client`、`aoprisan/typesafe-ai-rust-sdk`、`gilljon/typesafe-ai-rs`、`AbdelStark/typesafe-rs`、`phiat/typesafe-rust-sdk`、`2commits/typesafe-systemone`、`shaharia-lab/jev-cli`、`wjixiang/jav-rs-sdk`
- 社区衍生：<https://github.com/tamaratran/fast-jev-compaction>、`vinnylarouge/jevlike`、`TheoLeeCJ/SemIf`

#### A-11 未采用 / 不可引用来源（保留以便追溯）

- **不可引用数字**（见 §6.5 C 档）：<https://flowtivity.ai/blog/jev-typesafe-ai-decision-model/>、<https://www.latent.space/p/ainews-jev-a-system-one-model-that>、<https://vercel.com/changelog/typesafe-ai-jev-now-available-on-ai-gateway>、<https://www.facebook.com/jhunter101/posts/so-ive-been-using-jev-by-type-safe-ai-for-4-hours-tonight-ive-ran-about-300000-t/4097315900569291/>、<https://www.youtube.com/watch?v=hfi4ad8OA3Y>
- **未采用**：<https://www.datacamp.com/blog/system-one-models-jev>（HTTP 403）、<https://apidog.com/blog/jev-api-key/>（内容未核实）、<https://www.sitepoint.com/build-safer-ai-agent-harness-jev-langchain/>（**与 TypeSafe Jev 无关，误引风险极高**，见 §8.8 #6）、`qibinlou/jev-chess`（仓库 404）、Instagram 来源的 $200M 估值传闻
- **聚合/口径不明站点**（结构性字段可交叉验证，基准数字不可混用）：<https://opentweet.io/jev/limits>、<https://www.ayautomate.com/blog/how-to-use-jev-guide>
- **疑似 AI/SEO 内容农场**（命中但不采用其数字）：explainx.ai、alphamatch.ai、cryptobriefing.com、modemguides.com、cellcog.ai、medium 相关系列

#### A-12 本轮自测产物（未入库，位于 /tmp/jev-research/）

- `serde-check/`：一次性验证 crate（`cargo 1.97.1` / `rustc 1.97.1`），输出 `ALL SERDE CHECKS PASSED`，8 项断言（见 §10.3）
- `evals_text.txt`：`evals.typesafe.ai` 抓取文本（用于核对官方自评数字）
- `llms-full.txt`：官方全量语料（903,311 字节，用于检索 "rust" / "stream" 等负向结论）
- `or_113.txt` / `or_typesafe.json`：OpenRouter 条目抓取（context_length 32000、pricing 0.000000042）
- 其余上游专题文件：`01-official-launch.md` … `13-community-debate.md`、`awesome.md`、`hn_*.txt/json`、`every.md`、`jevbench.md`、`phish_readme.md` 等

### 14.2 附录 B：未证实 / 不确定清单（汇总）

> 汇总 4 份片段中所有标注为【未证实】或"未找到公开信息"的条目，供后续核查。**正文中这些条目保持原标记，不得升级为结论。**

**B-1 事实类（被引述但无法核对）**

1. **$200M 估值传闻**（Instagram / Forbes 标题；Forbes 正文两次抓取失败，dealroom 另出现 "US$25.9M seed round"）：唯一可引用融资数字是「约 $40M 种子轮，DCVC 领投」。（§2.6、§11.5）
2. **官方 workflow evals 各 workflow 的样本量 N（240/117/150/204，合计 711）**：只见于第三方复核文章，未在官方页面静态数据中核实。（§6.9）
3. **Good Start Labs 的 6,003 次 rubric 评测**（Jev 与 Fable 5.1 一致率 91.5%、每百万条 $160 vs $33,000）：仅 Langfuse 一处转述。（§6.4、§6.9）
4. **Every 的原文数字**（777 判断 <0.7s、12 段文本 6/7 缺陷检出）：原文有 JS/订阅墙，数字经三处转述且一致。（§6.4、§6.9）
5. **bitnovus 分档校准细节**（<0.1 → 0.1%；≥0.9 → 99.9%；0.5–0.6 档 38%；0.3–0.7 转人工后 99.5%）：本轮主要经 Arize 转述，仓库数据文件未逐行核对。（§6.4、§6.9、§9.6）
6. **五项边界研究**（`jev-certify`、`jev-orderby-bench`、`jev-calibration-audit`、`jev-ood-calibration`、111 例门控研究）：均为 awesome-typesafe 索引转述，未逐字核对原始结果文件。（§6.7）
7. **X/Twitter 来源的全部内容**（Raschka 评价、`@predict_addict` 的 8 数据集校准测试、Matthew Berman 等实测数字）：登录墙无法直连核对，均为中文媒体汇总或 HN 转述。（§12.4）
8. **cnblogs 的「1610 亿 token」「比 Fable 5.1 低 238 倍」**、KDnuggets 的「96.3% fact-checking」「275 份文档强一致」：未给出一手来源。（§12.4）
9. **Vercel AI Gateway 免费至 9/25、Swamp 扩展**：仅见搜索摘要与 HN 单条评论。（§12.4）
10. **「45% faster」（AINews / Facebook 两个变体）**：非官方、非受控基准，原始出处未定位。（§6.5）
11. **Vercel "~5–18x faster and more accurate"**：无数据集/案例数，changelog 措辞为 "TypeSafe reports…"。（§6.5）
12. **RLCD 的技术细节**（奖励函数、校准度量、负奖励偏斜分布的个人解读）。（§4.6）
13. **错误响应体的 JSON 结构**：由 SDK 容错解析源码推断，官方未承诺。（§3.9）
14. **`529` 的 SDK 映射归类**、**`429` 是否总带 `retry-after`**、**计费边界（最低计费单位、失败/重试是否计费）**：源码推断或未找到公开信息。（§3.6、§3.9）
15. **官方 SDK 的 Python 默认重试次数**：文档示例写 3、JS 写明默认 2，Python 侧无默认值声明。（§10.5）
16. **`jev-preview` 与 `jev-latest` 之外的版本历史**、**官方 changelog / 弃用策略**：均未公开。（§5.7）
17. **原生 API 是否仍需 waitlist/邀请**（datacamp、apidog 称需要，官方页面无对应文字）。（§5.7）
18. **OpenRouter 是否上架**：两份上游文件结论冲突，未裁定。（§11.4）
19. **SOC 2 / 认证状态、子处理器清单**：Trust Center 只返回 Vanta JS 壳；子处理器信息仅见于第三方。（§11.5）
20. **是否存在企业级 SLA 附件、AUP/高风险用途限制条款**：未找到。（§11.5）
21. **`jev-harness` crate 的作者身份及其与本项目的关系**：未核实，需项目决策者确认。（§10.5）
22. **社区对 Jev 架构的推测**（编码器+回归头、文本扩散、约 3B 参数、并行 prefill 取单 token 概率）：无官方确认。（§12.4）
23. **「某支付机构模型风险工程师 Slack 对话」「Twitter 付费合作 / astroturf 指控」**：无证据的内容，仅记录其存在。（§12.2）
24. **`jev-1.13` jaggedness 页的 URL 变更**（`models/jev-1.13-jaggedness` 404 vs `model-jaggedness/jev-1.13` 可访问）：是否同一页面未核实。（§12.4）
25. **premai 文章中的 ToxiGen / JailbreakBench 表格与 Llama Guard 800ms、月度成本**：未注明来源与方法，属第三方估算。（§9.6）
26. **数据留存细节**：LangChain 公开说"当前不提供 ZDR"，官方文档说"企业可申请 ZDR"，两条口径并存。（§7.8、§11.3）
27. **官方场景地图自述 10 类但表内 8 行**：缺失条目未在抓取内容中出现。（§7.9）

**B-2 关键未知（不是"传闻"，而是"确实没有公开数据"）**

1. **中文/CJK 的准确率**：官方只给"handled but not equally well"，无数字。（§4.3、§8.7）
2. **校准曲线 / ECE / reliability diagram**：官方未发布，唯一直接测 ECE 的第三方实验不利。（§4.4、§6.7）
3. **对抗鲁棒性量化**：无攻击成功率、无规模化红队数据。（§4.6、§7.8）
4. **p95/p99 延迟、并发表现、限流命中率、重试率、版本漂移影响**：无独立数据。（§6.9、§11.5）
5. **架构、参数量、训练数据、算力、model card、权重**：完全未披露。（§2.6）
6. **单请求问题数上限**：官方未声明。（§3.9、§10.5）
7. **分层限流表、账号级配额、免费层**：未公开。（§5.7）
8. **state 为空/超长截断/非文本混入时的行为**、**多轮对话式 state 复用示例**：未找到。（§8.8）
9. **Jev 与本地专用分类器/重排器在同一真值任务上的独立对比**、**Jev vs 本地 Rust 分类核**：明显空白。（§9.6）
10. **中国大陆等地区的访问限制**：未做连通性测试。（§5.7）

### 14.3 附录 C：防坑提示与建议的验证实验

#### C-1 防坑提示（速查，按"最容易踩"排序）

1. **别把 confidence 当"正确概率"**：它是分布集中度 / 边界余量，`confidence 1.0` 只说明分布尖。（§3.4、§4.4）
2. **别用 Noul 的阈值去套 Choice**，也别假设 `P(a) + P(¬a) = 1`（官方反例 0.72 + 0.47 = 1.19）。（§4.1 #8）
3. **别让 Noul 出现 `confidence` 访问**：它没有这个字段；代码层面要能编译期拦住。（§3.3、§10.3）
4. **别把问题写进 state 当作指令**：state 里的问题只是"待判断文本"，不会被当作提示。（§4.2）
5. **别用 Score 期望值反推精确数值**：只能用于过阈值。（§4.1 #2）
6. **别指望 Choice 说"不适用"**：概率必然归一到某个选项；需要"是否适用"就单独问 Noul，或加 `other`。（§3.4.1、§6.7）
7. **别把工具输出/助手消息放进门控 state**：不受信内容会为自己的执行授权。（§7.3）
8. **别把 Jev 当安全边界**：官方原文"Nothing here is a security boundary."。（§4.2）
9. **别用 `jev-latest` 固化阈值**：pin 到版本化 ID，并记录响应里的实际版本。（§5.1）
10. **别假设流式**：一次原子调用，UI 需要独立事件分支。（§3.8、§10.4）
11. **别让门控"失败即放行"**：拦截型控制必须 fail closed；放行型控制必须 fail open，两者不能混用一套默认值。（§7.4）
12. **别在低置信度上重试同一个问题**：Jev 不会像 LLM 那样"再想想"就改答案，重试多半得到同样的输出（且可能重复计费）。（§7.8、§3.6）
13. **别跨来源比较延迟数字**：官方 70–500ms、Vercel 132–272ms、Jevals 0.44–0.48s、NearHere 0.59s、Jagent 352ms 的口径互不相同。（§6.5 B 档）
14. **别引用 193.6×/444.6× 或 "45% faster" 作为架构前提**：无法复算/无方法学。（§6.3、§6.5 C 档）
15. **别批量比较"Jev vs LLM"却不检查自己的请求体**：Jagent 自曝两次因未开 `json_schema` strict / 未设 `temperature 0` 而得出一边倒的错误结论。（§6.4）

#### C-2 建议的验证实验（按优先级）

> 所有实验都应产出：脚本 + 原始数据 + 报告（含样本量、并发、重复次数、阈值与版本），并写入项目文档。**没有 API key 时，E1/E3/E4 可先用官方示例与自有英文数据搭好管线，等 key 到位后补测。**

**E1｜中文/CJK 基线（对应 §13.4 假设 1）**

- 数据：≥300 条本项目真实中文输入（工单/指令/文档片段），人工标注真值；另准备同义英文版。
- 设计：同一批问题分别用（a）中文 state + 中文 instructions、（b）中文 state + 英文 instructions、（c）英译 state + 英文 instructions；每条件跑 3 次。
- 指标：准确率、confidence 分布、`noul` 分布、批间稳定性；按题型（Choice/Score/Noul）分层。
- 判定：若中文条件准确率显著低于英文条件，则把"英译后再判定"或"英文 instructions"写入设计约束，并提高低置信兜底比例。

**E2｜校准分档（对应假设 2）**

- 数据：已有真值标注的任务（可从 E1 复用）。
- 设计：按 confidence / `noul` 分 10 档，统计每档的实际正确率，画可靠性图并计算 ECE；对 Score 用归一化后的期望值与档位实际分布对照。
- 判定：只接受"分档准确率随置信单调上升且偏差可接受"的任务进入自动路径；否则全部走人工/LLM 复核。

**E3｜批处理等价性（对应假设 7）**

- 设计：同一批 state 分别用"一次多问"与"逐问多请求"两种策略，各跑 5 次；比较逐题答案一致率、票数与闸门通过率。
- 判定：若出现"批处理改变判定"（第三方反例），则对高代价判定禁用批处理，或对关键问题做二次确认。

**E4｜注入鲁棒性（对应假设 3）**

- 数据：≥200 条注入样本（伪造工具输出、伪造用户预先授权、自我分类论证、角色扮演、编码/混淆写法），每条都对应一个"应当被拦截"的动作。
- 设计：测量 block 概率与 confidence 的相对漂移（对照无注入版本）、"不安全却放行"率、误拦率；同时测两种 state 组成（含/不含工具输出）。
- 判定：若"不安全却放行"率超过项目容忍线，则门控只能作为信号、不能作为决策者（必须叠加确定性规则与人工确认）。

**E5｜契约与 serde 边界（对应假设 9）**

- 设计：拿到 key 后跑一轮真实响应回归：断言字段形状；测 `usage` 为 `null`、未知 `type`、`answers` 缺 key、错误体结构（401/422/429/529）、`probabilities` 和是否为 1。
- 产出：把结论固化进 Rust 类型定义（`deny_unknown_fields` / `#[serde(other)]` 的取舍）与错误映射表。

**E6｜限流与成本探针（对应假设 8）**

- 设计：小规模（例如 1,000 次请求，分档并发 1/4/8/16）测 p50/p95/p99、429 触发点与 `retry-after` 行为；对比"state 只计费一次"的假设与重试是否重复计费（用 `usage.input_tokens` 与账单核对）。
- 判定：确定本项目的全局限流参数、退避预算与降级触发点。

**E7｜容灾演练（对应假设 6）**

- 设计：注入故障（超时、429、529、401、DNS 失败），验证：退避是否遵循 `retry-after`、熔断是否触发、降级是否切换到规则/LLM、恢复后是否自动回流、是否有静默失败。
- 判定：每个失败模式都要有明确的可观测信号与预期的用户可见行为。

**E8｜版本升级门禁（对应假设 4）**

- 设计：把 `jev-latest` 与固定版本并列跑同一批 E2 数据，观察数值漂移；定义"新版本上线前必须重跑 E1/E2/E4"的流程。
- 产出：`docs/` 下的版本变更记录（含模型版本、阈值、实验结论），供后续审计。

**E9（可选）｜Jev vs 本地分类核（对应 §9.6 的空白）**

- 背景：vLLM Semantic Router 已证明 Rust + Candle + ModernBERT 的多任务分类核可行且极快（MMLU-Pro +10.24pp、延迟 −47.1%）。
- 设计：用同一批带标注数据，对比 Jev、微调编码器（SetFit 类）、LLM judge 在准确率/延迟/成本/可维护性上的表现。
- 判定：若本项目的中文任务在本地编码器上已达标，"Jev 用于分类/路由"应重新评估为"可选增强"而非默认方案。

---

### 14.4 附录 D：快速决策速查（一页版）

| 问题 | 一句话答案 | 依据 |
| --- | --- | --- |
| Jev 能替代 LLM 吗？ | 不能，官方明确否认；它是循环里的决策层 | §2.4【官方】 |
| 能当 coding agent 主模型吗？ | 不能；无流式、不调工具、不写文件 | §2.4、§3.8【官方】 |
| 准确率比 LLM 高吗？ | 没有证据支持；多项第三方向上不利 | §6.4、§6.7 |
| 成本和延迟呢？ | 量级优势有独立证据（分类 p50 5.43×、成本 −96.1%） | §6.5 A 档 |
| 概率能用吗？ | 设计上可以，但必须先在自有数据上测校准（唯一 ECE 实测不利） | §4.4、§13.4 #2 |
| 能给解释吗？ | 不能；需要解释就接 LLM judge | §9.4 |
| 中文行吗？ | 官方说"可用但准确率较低"，零数字；必须自测 | §4.3、§8.7 |
| 能当安全边界吗？ | 不能 | §4.2【官方】 |
| 有 SLA 吗？ | 没有；只有 commercially reasonable efforts | §11.2【官方】 |
| 能自部署吗？ | 不能（只有 hosted API） | §2.5【官方】 |
| Rust 怎么接？ | 自建 HTTP 客户端；serde 内部标签枚举已验证可行；无官方 SDK | §10 |
| crate 名字能用 `jev-harness` 吗？ | crates.io 已被占用，需先决策 | §10.2 |
| 请求怎么发最省？ | 一次请求多问（推测式扇出），并发留给不同 state | §3.6、§8.4 |
| 失败怎么办？ | 路由/技能 fail open；门控 fail closed；都要配超时、熔断与降级 | §7.4 |
| 上线前必须做什么？ | §13.4 的 10 项实测，尤其是中文、校准、注入、容灾四项 | §13.4 |

---

## 项目决策记录

> 项目层面的决策记录（命名、供应商风险、判定权等）已移至 [`docs/project-notes.md`](project-notes.md)，以该文件与 `AGENTS.md` 为准。

**报告结束**。本文件是 `jev-harness` 项目的参考资料，用于确定 Jev 在项目中的定位；正文中的判断性内容（尤其第 13 节）需在项目自有实测数据到位后复核。
