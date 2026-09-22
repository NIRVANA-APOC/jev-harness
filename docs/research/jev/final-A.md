# 报告片段 A：Jev 是什么、API 契约与三类原语、能力边界与已知限制

> 撰写日期：2026-09-22。本文由上游专题 `01-official-launch.md`、`02-api-contract.md`、`03-question-types.md`、`05-boundaries-limits.md` 提炼、去重、按主题重组，**未引入任何上游之外的新事实**。
> 可信度前缀：**【官方】**＝TypeSafe 官方文档/博客/定价与模型页/评测站/cookbook；**【第三方】**＝有署名的第三方实测、报道或社区讨论；**【未证实】**＝转述、二手、聚合、无法核对或口径不明。上游标注的冲突数字与未证实内容一律保留：凡出现两个口径即并列呈现并注明差异来源。
## 一、Jev 是什么：System One、RLCD、非自回归与 LLM 的本质差异

**要点**

1. Jev 是 TypeSafe AI 的第一个公开模型，也是其自创品类 **System One Model** 的首个实例：不生成文本，输入 **state** + 一组带类型的 **questions**，直接返回类型化答案与概率分布。【官方】
2. 训练方法是 **RLCD（Reinforcement Learning for Calibrated Decisions）**，官方与 RLHF、RLVR 并列为第三条后训练路径，优化目标是"概率要诚实"。【官方】
3. 与 LLM 的根本差别有三层：**输出空间预先枚举**（不会有类型错误）、**非自回归的并行采样**（"增加问题几乎不增加响应时间"）、**每个输出都带概率**（Noul 除外，其余带独立 confidence）。【官方】
4. 发布状态截至 2026-09-22 仍是 **early access + waitlist、hosted API、无自部署**；发布日 2026-09-15，同日宣布结束隐身期与 $40M 种子轮。【官方】+【第三方】
5. 架构与规模**完全未披露**：官方只给"新模型架构 + parallel sampler + RLCD"三句话，无参数量、无 model card、无权重，并明确不发布标准 benchmark 表。【官方】
### 1.1 System One 的定义（官方原文）
- "System One models are a class of AI models built to make fast, structured decisions that software can use directly. A System One model evaluates a state and returns typed answers and probabilities."（<https://docs.typesafe.ai/concepts/system-one>）【官方】
- "Like an LLM, a System One model understands natural-language input. It returns typed decisions and probabilities rather than generated text."（同上）【官方】
- "System One models do not write replies, produce code, or generate explanations of their reasoning."（同上）【官方】
- 命名来源：System 1/2 取自 Kahneman《思考，快与慢》；"Jev" 取自 William Stanley Jevons（杰文斯悖论：单位成本下降导致总需求上升）。（<https://typesafe.ai/blog/introducing-system-one-models-and-jev>，本文件首引后简称"官方发布博客"）【官方】
- 公司叙事："Machine Native Intelligence"、"We're building prod, not God"、预期未来 AI 自动化中约 99% 是机器对机器、1% 是人对人。（<https://docs.typesafe.ai/introduction/machine-learning-primer>、<https://typesafe.ai/manifesto>）【官方】
### 1.2 RLCD：第三条后训练路径
- 三条路径并列：**RLHF** 优化人类偏好（官方自陈会导致 sycophancy、自信的幻觉、mode dropping）；**RLVR** 优化可程序化验证的奖励（数学等，更慢更贵）；**RLCD** 优化"决策 + 校准概率"，不生成文本。（<https://docs.typesafe.ai/introduction/machine-learning-primer>）【官方】
- 校准的官方定义（避免误读）："Higher probability should correspond to a greater chance that the answer is correct."——概率 0.2 的结果应在大约 20% 的情况下出现，0.8 对应约 80%，1.0 对应 100%；且 "**These rates describe groups of predictions, not a guarantee about any single answer.**"（同上、<https://docs.typesafe.ai/confidence>）【官方】
- 架构/训练栈的唯一描述："We built a new stack entirely focused on automation: with a new model architecture, parallel sampler for maximum efficiency, and training method we call Reinforcement Learning for Calibrated Decisions (RLCD)."（官方发布博客）【官方】
- 缩写重名提醒：**RLCD 在 ICLR 2024 已有另一含义**（Reinforcement Learning from Contrastive Distillation，Kevin Yang 等），引用时须写明指哪一个。（<https://www.laura-martel.com/blog/typesafe-jev-system-one-models>）【第三方】
- 数据口径：CEO 播客中称训练数据**全部为合成数据**、刻意不用用户数据；文档侧表述 "Jev is not fine-tuned or LoRA-adapted with customer data… the same weights serve every account"（<https://www.latent.space/p/jev>、<https://docs.typesafe.ai/models>）【官方，播客为 CEO 自述】
- **未披露**：RLCD 的奖励函数形态、训练数据配方、校准度量方法。（官方发布博客、<https://kingy.ai/blog/typesafe-jev-review-the-ai-model-that-doesnt-generate-text/>）【官方未公开】
### 1.3 非自回归：并行采样而非逐 token 生成
- 官方对照表：LLM 是"顺序、逐 token，每个 token 依赖上一个"；System One/Jev 是"并行，单次查询产出全部输出"。（官方发布博客）【官方】
- 一次请求中所有问题**共享同一份 state、彼此独立并行求值**："Every question in a request sees the same state, is evaluated independently"；一个问题不会成为另一个问题的隐性上下文。（<https://docs.typesafe.ai/primitives>、<https://docs.typesafe.ai/concepts/state>）【官方】
- 官方速度口径：端到端 **70ms–500ms**，对比前沿 LLM 的 3–329 秒；同类任务快 40x–200x。（官方发布博客、<https://docs.typesafe.ai/introduction>）【官方】
- 架构推断属第三方：社区把 Jev 描述为"受约束的/类 diffusion 的决策模型，不能产出自由文本，必须预先定义输出格式"。（<https://www.latent.space/p/ainews-jev-a-system-one-model-that>）【第三方，非官方依据】
### 1.4 与 LLM 的本质差异（官方对照表节选，来源：官方发布博客）【官方】
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
- 官方首页头条数字 **193.6x 更快、444.6x 更省**（单次对比展示 TypeSafe $0.000081 / 0.114s vs LLMs $0.013880 / 8.566s），官方自述"我们认为这是真实世界收益的上限区间"。（<https://typesafe.ai/>）【官方，厂商自测】
- 官方明确**不发布标准 benchmark 表**（理由是 benchmaxxing），改用"有日期的快照评测、发布后即退休"，并承诺公开 caveat 与对自己不利的证据。（<https://typesafe.ai/blog/antibenchmaxxing>）【官方】
### 1.5 发布、版本与时间线
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
- 别名：`jev-latest`（最近稳定官方版，SDK 默认）、`jev-preview`（最近一次发布，含 preview build）；版本化 ID 始终可用；响应 `model` 字段回报**实际作答的版本化 ID**。（<https://docs.typesafe.ai/models>、<https://docs.typesafe.ai/api>）【官方】
- 契约演进：`POST /preview/evaluation` → `POST /v1/systemone` 是**破坏性变更**（字段大规模改名，见 §2.7）。（<https://docs.typesafe.ai/migrating-to-v1.md>）【官方】
- 官方 SDK 版本（2026-09-22 核对）：PyPI `typesafe-sdk` **0.7.1**（Python ≥3.10）；npm `@typesafe-ai/sdk` **0.6.0**。（<https://pypi.org/pypi/typesafe-sdk/json>、<https://registry.npmjs.org/@typesafe-ai/sdk>）【官方元数据】
- 未披露（多方检索确认无公开信息）：参数量、网络结构（是否 transformer 之外、是否含 classifier/diffusion 成分）、训练算力与硬件、训练数据构成、model card、权重。（Kingy 评测、<https://xhinker.medium.com/jev-the-most-overrated-model-of-2026-even-before-the-year-end-0f86838ca419>）【第三方】
### 本节不确定项
1. **"非自回归"的精确含义只有官方三句话支撑**（新架构 + parallel sampler + RLCD）；是否有 diffusion/classifier 成分属第三方推测，无官方依据。
2. **$40M 之外的估值（$200M）仅见于 Instagram 帖子**，未在权威来源核实，本报告不采信。【未证实】
3. **官方 FAQ 大面积无答案**：发布博客 FAQ 共 6 问，仅"命名来源"一问有答案；"Is Jev just a smaller LLM?"、"How does Jev perform against public benchmarks?"、"Where does our training data come from?" 等在抓取 HTML 中只有标题、无正文；首页 FAQ 亦仅第一条有答案。这些属**未找到公开信息**。（官方发布博客、<https://typesafe.ai/>）
4. **发布状态会变**：截至 2026-09-22 仍是 early access/waitlist；Rust 项目据此做开发验证时需预留无 API key 的降级路径。
5. **性能数字均为非生产条件**：官方速度多在"公司笔记本 + 美西服务"下测得；第三方样本小（777 条判断、12 段文本），无 p95/p99、并发、可用性、重试率数据。
6. **早期版本与 1.13 的数字不可混用**：官方 cookbook 跨页面出现过 `jev-1.12`、`jev-1.13.0`、`jev-latest`，绝对数字未必同版本同条件。
## 二、API 契约与三类问题原语（Choice / Score / Noul）

**要点**

1. 整个 API 面**只有一个核心 endpoint**：`POST https://api.typesafe.ai/v1/systemone`；辅助 `GET /v1/models`。无 `/chat/completions` 兼容端点，也没有批量、异步任务或 webhook 端点。【官方】
2. 请求体恰好三个顶层字段：`state`、`model`、`questions`；**question 的 key（题目 id）不发给模型、不参与推理**，只用于把答案对回来。【官方】
3. 响应体三个顶层字段：`model`（版本化 ID）、`answers`（按同名 id 回填）、`usage`（`input_tokens`/`output_tokens`）。【官方】
4. 只有三种问题原语，用 `type` 区分：**Choice**（无序候选择一，≤255 选项）、**Score**（有序列标尺，2–10 级）、**Noul**（是非概率，无 confidence 字段）；三者共享 `type` + `instructions`，各自带不同形状的 `criteria`。【官方】
5. 计费只看**输入 token**（$0.042 / Mtok ＝ $42 / Btok），**输出 token 免费**；同一请求内多问题共享一次 state 计费，这是批处理收益的来源。【官方】
6. **未找到任何公开的流式返回支持**（既未承诺也未明确否认）；API 证据支持"一次请求—整包响应"模型。【官方文档 + 源码检索，属负向结论】
### 2.1 Endpoint / base URL / 认证
| 事实 | 内容 | 来源 |
| --- | --- | --- |
| 主 endpoint | `POST https://api.typesafe.ai/v1/systemone` | 【官方】<https://docs.typesafe.ai/api>（本文件首引后简称 /api） |
| 辅助 endpoint | `GET /v1/models`（返回账号可用模型名、描述、发布日期） | 【官方】<https://docs.typesafe.ai/models>（下称 /models） |
| 认证 | 头 `Authorization: Bearer <API_KEY>` + `Content-Type: application/json` | 【官方】/api、<https://docs.typesafe.ai/introduction/quickstart> |
| SDK 环境变量 | `TYPESAFE_API_KEY`、`TYPESAFE_BASE_URL`（默认 `https://api.typesafe.ai`）、`TYPESAFE_DEFAULT_MODEL`、`TYPESAFE_LOG_LEVEL` | 【官方】<https://docs.typesafe.ai/sdk/python/api/constants.md> |
| 迁移确认 | "认证不变：继续发送 `Authorization: Bearer <API_KEY>` 与 `Content-Type: application/json`" | 【官方】<https://docs.typesafe.ai/migrating-to-v1.md> |
| 第三方确认 | LiteLLM pass-through 用同一路径（`GET /typesafe/v1/models` 可用，按 `typesafe/<model>` 计费与记日志） | 【第三方】<https://docs.litellm.ai/docs/pass_through/typesafe> |
- **只在 SDK 源码可见、文档未列出的头**（对 harness 日志/追踪有用）：`X-TypeSafe-SDK`、`X-TypeSafe-Runtime`、`X-TypeSafe-Retry-Count`；响应头 `x-typesafe-request-id`（SDK `request_id` 来源）、`retry-after`/`retry-after-ms`。（<https://raw.githubusercontent.com/typesafe-ai/typesafe-sdk-python/main/src/typesafe_sdk/_core/constants.py>）【官方源码，非文档承诺】
### 2.2 请求体字段与约束
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
- **上下文预算**：64k tokens / 请求（`state` + 全部问题合计）；其中 **32k** 适用于 `state` + 单个最长问题。（/models）【官方】
- **state 形状校验**：官方 State 页写 "State must be a string, JSON object, or array of text values."；**裸数字或布尔值（含被序列化成标量的类型）会得到 422**。（/concepts/state；Spring AI 博客）【官方 + 第三方实测】
- **单请求最多多少个问题：官方未声明硬上限**（官方写了选项数 255、层级数 10，却通篇未写题目数上限；已用官方全量语料 `llms-full.txt`（903,311 字节）整库检索复核，确认无相关表述）。（<https://docs.typesafe.ai/llms-full.txt>；第三方独立确认 <https://www.ayautomate.com/blog/how-to-use-jev-guide>）【官方负向结论 + 第三方】
- 同一请求内的问题**互相独立、并行评估**，一个问题的答案不会成为另一个问题的上下文；有真实依赖时须发第二次请求。（<https://docs.typesafe.ai/primitives>）【官方】
### 2.3 响应结构与概率 / 置信度语义
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
### 2.4 Choice / Score / Noul 逐类字段语义
**Choice —— 从一组无序选项中择一**
- 请求字段：`type`（`"choice"`）、`instructions`、`criteria`（**选项名 → 评分说明** 的 map，不需要时用 `null`）；**上限 255 个选项**（官方原文："You can have a maximum of 255 options per Choice"）。（/api、<https://docs.typesafe.ai/primitives/choice>）【官方】
- 选项名与描述**都会发给模型**，描述须能相互区分；当选项集合可能覆盖不了全部输入时应显式加 `other` / `none of the above`——因为**模型必须选一个**。
- **不要用 Choice 做"是否适用"**：Choice 概率必然归一，总会有一个赢家；Spring AI 的做法是另加一个 Noul 问"到底有没有适用的工具"。（Spring AI 博客）【第三方实测】
- 实用上限另有官方 cookbook 口径：**"a Choice works reliably up to roughly 240 options"**（75 类行业组一次问完，远在 255 之内）。（<https://docs.typesafe.ai/cookbooks/classification_using_confidence>）【官方】
- 高于 255 基数的做法：官方内部用"**先独立打分、再显式选择**"的两阶段方式，并承认这会**变慢**。（官方发布博客）【官方，仅见于博客，文档正文未展开】
- 选项描述质量直接影响结果：同一工单，仅给裸标签 `billing/technical/sales` → confidence **0.60**；补上描述 → **0.82**。（Spring AI 博客）【第三方实测】
**Score —— 在有序标尺上取值**
- 请求字段：`type`（`"score"`）、`instructions`、`criteria`（**有序列标尺描述数组**，≥2 级、≤10 级）；**层级编号 = 数组下标，从 0 开始**。（/api、/primitives/score）【官方】
- 模型只看到描述文本，看不到编号、也看不到相邻层级，"比上一级更严重"这类写法无意义。官方实测对照：同一份 Safari 报告，描述式三级 → `score 0.0 / confidence 1.0`；**纯数字三级**（`["0","1","2"]`）→ `score 0.55 / confidence 0.33`，概率被摊到 0 与 1 之间。（/primitives/score）【官方】
- 官方写作建议：**描述情境而不是程度**（"Broken or degraded feature, but workaround exists" 优于 "moderately severe"）；每个 Score 只测一个维度；顶部极端情形单独给一级；3 级也够用。（同上）【官方】
- 结构化层级可提升稳定性（官方对照表）：同报告纯文本描述 → `1.43 / confidence 0.35`；给某一级加**贴切的** `examples` → `1.03 / 0.96`；加**不相关**的 examples → 仍为 `1.43 / 0.35`。官方同时警告：**更高的 confidence 并不证明答案更正确**。（同上）【官方】
- **同一 score 值可有完全不同的分布**（1.0 可能全压在 level 1，也可能 0 级与 2 级各半），必须结合 `probabilities` 与 `confidence` 读。（同上）【官方】
- 跨尺度合并前必须归一化：`score / (len(criteria) - 1)` 再加权求和，官方例 `0.6 × 0.62 + 0.3 × 0.64 + 0.1 × 1.0 = 0.664`。（<https://docs.typesafe.ai/patterns/composite-scoring>）【官方】
- 不要用 Score 期望值**插值还原**档位之间的具体数值；Score levels 数值校准很弱，只能用于判"是否过某个阈值"。（<https://docs.typesafe.ai/model-jaggedness/jev-1.13>，本文件首引后简称 jaggedness 页）【官方】
**Noul —— 是非判断，返回概率**
- 请求字段：`type`（`"noul"`）、`instructions`、可选 `criteria`（`true`/`false` 两侧语义说明）；返回 `{type, noul}`，`noul ∈ [0,1]` 是"答案为是"的概率，**无独立 confidence**。（/api、/primitives/noul）【官方】
- 写作建议：一个问题只问一件事（两个条件拆成两个 Noul，在代码里组合）；措辞要让**高值 = 是**（"message 是否包含个人数据"优于"message 是否不含个人数据"）；边界含糊时用 `criteria` 钉住定义；先不带 criteria 试、再带 criteria 试，保留本数据上更好的那个。（/primitives/noul）【官方】
- **0.5 不是"中等"**，官方对照实验：同一批"候选人 Python 强度"输入，Noul 给 0.03 / 0.14 / 0.81 / 0.92；同批输入改用 4 级 Score 落在 0.0 / 1.0 / 2.05 / 2.89 → **要测程度就用 Score**。（同上）【官方】
- 官方给出的 `jev-1.13.0` 实测记录表（同一 Noul、不同输入）：`"Thanks, that fixed it!"` → **0.02**；`"How do I reset my password?"` → **0.07**；`"I need this sorted today, whatever it takes."` → **0.26**；`"Are you a bot?"` → **0.40**；`"Is there any way to speak to someone about my invoice?"` → **0.84**；`"I have asked three times now. Can I please just talk to a real person?"` → **0.99**。（同上）【官方】
### 2.5 完整请求与响应示例（官方原文）
**A. cURL 最小请求**（<https://docs.typesafe.ai/introduction/quickstart>）【官方】
```bash
curl -X POST https://api.typesafe.ai/v1/systemone \
  -H "Authorization: Bearer $TYPESAFE_API_KEY" -H "Content-Type: application/json" \
  -d '{"state":"Hi, I'\''ve been trying to connect my Stripe account for 3 days and the integration keeps failing. I'\''m losing sales. Please help ASAP.","model":"jev-latest","questions":{"urgency":{"type":"noul","instructions":"Does this message express urgency?"}}}'
```
**B. 三类原语同请求（官方 Quickstart 原文；为紧凑起见合并了部分缩进，字段与取值与原例一致）**【官方】
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
**C. 三类原语各自的官方最小示例（`/api`，`//` 行为阅读标注、非请求内容）**【官方】
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
- Score 同型完整示例（官方 Safari 报告，含官方加权平均验算 `0 × 0.0 + 1 × 0.57 + 2 × 0.43 = 1.43`）：`score 1.43`、`confidence 0.35`、`probabilities {0:0.0, 1:0.57, 2:0.43}`、`usage input_tokens 332 / output_tokens 18`。（<https://docs.typesafe.ai/primitives/score>）【官方】
- 低置信度读法（官方 5 题同请求示例）：工单同时属于两个团队（returns 0.61 / billing 0.35），故 `confidence` 仅 **0.42**；`requested_resolution` 的 confidence 仅 **0.20**，示例代码据此**改为向客户确认**而不是直接行动。（<https://docs.typesafe.ai/primitives/choice>）【官方】
- LangChain 博客给出的同一"是否紧急"问题变体返回 **0.999**，并称"99.9% 概率紧急"；上游注明：不同页面示例值不同（0.95 / 1.0 / 0.999）属各自示例，不代表官方矛盾，**均不可当基准数据**。（<https://www.langchain.com/blog/building-a-harness-with-jev>）【第三方转述】
**D. 实现细节（对 Rust 客户端直接相关）**
- **HTTP 层** `legend`/`probabilities` 的 key 是**字符串**级号（`"0"`,`"1"`,`"2"`）；**Python SDK 层** `ScoreAnswer.legend` 是 `dict[int, ...]`、`probabilities` 是 `dict[int, float]`，即 **SDK 用整数 key**。（/api、responses.md）【官方】
- JS/TS SDK：`answers.category.choice` 的类型由 options 推导成字面量联合类型。（<https://docs.typesafe.ai/sdk/javascript>）【官方】
- 客户端校验：官方 JS SDK `systemOne()` 声明会抛出 **"Questions are empty, or score criteria are not a list of at least two entries."**，即"空 questions"与"Score 层级少于 2 条"在客户端即被拦下。（<https://docs.typesafe.ai/sdk/javascript/api/classes/TypeSafeClient>）【官方】
- Python SDK 签名：`system_one(state, questions, *, model=None, retry=None, timeout=None, extra_headers=None, extra_body=None, response_model=None)`；`extra_body` **浅合并**进顶层请求体且"最后写入优先"（同名 key 会覆盖 `state`/`model`/`questions`）；有 `response.nouls/.choices/.scores` 分组访问器与 `response.request_id`。（<https://docs.typesafe.ai/sdk/python/api/clients/sync.md>）【官方】
### 2.6 并行批处理与计费
**并行机制（官方口径，多来源一致）**
- 一次请求里的所有问题**共用同一份 state、并行且相互隔离地评估**；答案之间不构成上下文，可随意增删问题而不影响其它问题。（<https://docs.typesafe.ai/primitives>）【官方】
- **真依赖必须落在代码里**：若后一问依赖前一答案（用答案取数据、决定 state 构成、或决定下一题选项），须发第二次请求；除此之外官方建议全部放进一次请求、用代码忽略不需要的答案——即 **Speculative fan-out（推测式扇出）**。（<https://docs.typesafe.ai/patterns/fan-out>）【官方】
- 增加问题"几乎不增加响应时间"，只增加问题本身的 token："costs only the tokens for the extra questions, which are cheap"；"Asking a question you might not need is close to free"。（同上、官方发布博客）【官方】

官方 cookbook 量化实测（<https://docs.typesafe.ai/cookbooks/parallel_questions>，本表首次引用后简称 parallel cookbook）【官方】
| 策略 | 调用次数 | 成本 | 耗时 |
| --- | --- | --- | --- |
| 批量（13 题同请求） | 1 | **$0.000497** | **0.27 s** |
| 单题串行（13 次请求） | 13 | $0.006090 | 2.71 s |
- 结论：**12.2× 更便宜、10.0× 更快**，且**批处理没有改变答案、也没有引入额外方差**（`RUNS = 5`；多为 std dev = 0.0，`breach_72h`（0.804 vs 0.814）与 `criminal_penalties`（std 0.0045 vs 0.0084）是题目自身固有采样噪声，两种策略量级相同）。【官方】
- 测试条件：GDPR 的 Wikipedia 固定修订版正文，**53,777 字符**（页面另处写作 ~54,000）；13 题 = 8 Noul + 2 Choice + 3 Score；文档字节级一致；成本按 `PRICE = (0.042, 0.00) # $ per 1M tokens (input, output)` 事后计算；延迟为 13 次单题延迟之和（**顺序调用假设**）。【官方】
- **⚠️ 官方内部数字不一致**：同为官方页面，`/primitives` 总览页写 **"11.5x cheaper / 9.6x faster"**，cookbook 页写 **"12.2x / 10.0x"** 并附原始金额表。按 cookbook 原始数字计算：0.006090/0.000497 ≈ **12.25×**、2.71/0.27 ≈ **10.04×**，即 **cookbook 页自洽、总览页数字疑为过时或笔误**。引用时建议用 12.2×/10.0× 并注明页别。（<https://docs.typesafe.ai/primitives>、parallel cookbook）【官方，内部口径冲突】
- **并发注意（官方自陈）**：单题延迟之和假设顺序调用；若并发发起，**速度差会缩小，但 13× 的 token 成本不变**。【官方】
- 第三方复述与补充：LangChain 博客（并行、增加问题几乎不影响响应时间、成本只增问题 token）；Spring AI 博客给出笔记本实测 **1 题 275ms / 3 题 310ms**（多 2 题多约 35ms），并写 "The service reads the state once and answers every question in parallel (for negligible cost)"。（<https://www.langchain.com/blog/building-a-harness-with-jev>、Spring AI 博客）【第三方】
- **逐文档 = 逐调用**（语用推论，非官方明文）：Spring AI 写 "Per-document work is one call per document. Reranking a top-20 list is twenty calls"，因此他们先用 `JevDocumentFilter` 过滤、只对幸存者排序。（Spring AI 博客）【第三方实测】

**计费**（/models）【官方】
| 项目 | 值 |
| --- | --- |
| 输入 | **$0.042 / 1,000,000 tokens**（页面同时写作 **$42 / Btok**） |
| 输出 | **免费**（"Charged per input token. Output tokens are free."） |
| 计量单位 | 响应体 `usage.input_tokens` / `usage.output_tokens`（输出免费但仍报告 token 数） |
- **成本模型关键推论**：state 每次请求只计一次费，因此"把 N 个问题放进一次请求"省下的是 state 的 (N−1) 次重复计费；问题本身 token 照价计费但很便宜。这解释了 13 题批量为何省 12.2×——文档主导型负载下，13 次单题请求把同一份 5.4 万字符正文付了 13 遍。【官方】
- 官方自我限定："We can't prove it isn't subsidized; we'll need the long-term to prove the sustainability of our pricing."（官方发布博客）【官方】
- 另一份官方 cookbook（self-consistency）声明其成本用的是"历史价格假设，**不是经过核实的 jev-latest 价格或当前账单金额**"——引用单个 cookbook 绝对金额时需保留这句限定。（<https://docs.typesafe.ai/cookbooks/consistency_noul_cookbook>）【官方】
- 第三方实测成本：8 路意图分类 **$0.0151 / 1,000 decisions**（提示词平均 ~360 输入 tokens）；77 路 **$0.0400 / 1,000**（平均 952 tokens/prompt）；同 8 路消息对比 LLM 为 GPT-5.6 Terra $0.609/1k、Claude Haiku 4.5 $0.356/1k、Gemini 3.5 Flash-Lite $0.087/1k、GPT-5.4 nano $0.070/1k（经 OpenRouter 计费）。延迟：791 次调用中位 **0.33 s**、P95 **0.44 s**、最慢 1.42 s（单机单客户端、4 路并发；作者自述"当作相对值，不是 SLA"）。（<https://www.ayautomate.com/blog/how-to-use-jev-guide>）【第三方实测】
- **未找到公开信息**：是否存在最低计费单位、失败/重试请求是否计费、429 退避重试是否重复计费。Elixir 客户端作者指出传输层失败后重放 "ambiguous: the server may already have processed and billed the evaluation"，故默认不重放——属第三方工程判断。（<https://hexdocs.pm/typesafe_ai/readme.html>）【未证实】
- 重排示例（官方 cookbook 转述）：40 条法律查询 × 30 个候选，共 **1,200 次调用花费 $0.0645**，正确段落进入 top-10 的比例从 38% 提到 62%。（转述自 ayautomate）【未证实，二手转述，未在官方页面直接核实】
### 2.7 契约演进史（交叉印证 v1 字段语义）
`POST /preview/evaluation` → `POST /v1/systemone` 是**破坏性变更**，迁移对照正好交叉印证 v1 字段含义（<https://docs.typesafe.ai/migrating-to-v1.md>）【官方】：问题集合 `prompts`（数组，每项自带 `key`）→ `questions`（map，key 即 id）；内容字段 `document` → `state`（传 `document` 校验失败）；描述字段按类型分的 `criteria`/`options`/`levels` → 统一为 `criteria`；答案集合 `responses`（数组，同序）→ `answers`（map，按 id）；Noul 值字段 `probability` → `noul`；Choice 值字段 `chosen` → `choice`；Score 值字段 `expectation` → `score`；Choice `probabilities` 由 `[{option, probability}]` 数组 → `{option: probability}` map；Score `probabilities` 为 v1 新增（`{level: probability}` map，preview 不返回）；`confidence` 换新算法（数值会变，**需重新校准阈值**）；`usage` 由 `billing_units` 占位 → `input_tokens`/`output_tokens`；Python 包由 `typesafe-client`（所有 0.1.x/1.0.x 版均已失效）→ `typesafe-sdk`。
### 2.8 错误码、限流、重试与流式（契约层）
| 状态码 | 含义 | 来源 |
| --- | --- | --- |
| `401 Unauthorized` | API key 缺失或无效，检查 `Authorization` 头 | 【官方】/api |
| `422 Unprocessable Entity` | 请求体校验失败（缺必填字段、问题格式错误），**响应体指出出错字段** | 【官方】/api |
| `429 Too Many Requests` | 超出限流，退避后重试 | 【官方】/api |
| `529 Overloaded` | 服务暂时过载，短暂延迟后重试 | 【官方】/api |
| `400/401/403/404/422/429/5xx` | SDK 异常类型映射（`TypeSafeBadRequestError`/`AuthenticationError`/`PermissionDeniedError`/`NotFoundError`/`UnprocessableEntityError`/`RateLimitError`/`InternalServerError`） | 【官方】<https://docs.typesafe.ai/sdk/python/api/exceptions.md> |
- 限额（jev-1.13）：**250,000 tokens/秒 且 1,200 requests/分钟**，任一超限返回 `429`；官方 Warning 明确 **限流数值会动态调整、可能无预警变化**；更高限额走 custom/enterprise。SDK 默认指数退避并尊重服务端 `retry-after`。（/models）【官方】
- 恢复策略：收到 `429` 或 `529` 时**指数退避重试**，不要立即重试。（/api "Handling rate limits"）【官方】
- Python SDK 默认重试策略：`max_retries=2`、`backoff_initial=0.5s`（每次翻倍、上限 `backoff_max=5.0s`）、`backoff_jitter=0.25`、重试状态码 `{408, 429, 500–599}`、`respect_retry_after=True`、重试总预算 `timeout=30.0s`；`TypeSafeRateLimitError.retry_after_ms` 暴露服务端要求的等待毫秒数；单次 HTTP 操作默认超时 `DEFAULT_TIMEOUT = 10.0s`。（<https://docs.typesafe.ai/sdk/python/api/retries.md>、constants.md、exceptions.md）【官方】
- JS SDK 默认值同构：`maxRetries=2`、`backoffInitialMs=500`、`backoffMaxMs=5000`、`backoffJitter=0.25`、重试 408/429/500–599、`respectRetryAfter=true`、`maxRetryAfterMs=60000`（更长的服务端延迟改用退避）。（<https://docs.typesafe.ai/sdk/javascript/api/interfaces/RetryPolicy.md>）【官方】
- 官方 cookbook 并发经验值：entity-alignment 的并发池为 `MAX_WORKERS = 6`，注释写 "the public endpoint rate-limits above roughly eight"（约 8 并发以上开始限流）。（<https://docs.typesafe.ai/cookbooks/entity_alignment>）【官方】
- **流式：未找到任何公开支持**。官方 API reference 无流式参数/响应类型/事件协议；全站纯文本 `llms-full.txt`（903 KB）检索 `stream`/`server-sent`/`text/event-stream`/`websocket` **无任何 API 相关命中**（唯一相关表述是"编码 agent 依赖会 stream 文本的 LLM … **Jev does none of that**"，<https://docs.typesafe.ai/introduction/coding-agents>）；JS SDK 仅 `systemOne()` 与 `models.list()` 两个方法，源码中响应处理函数名为 `bufferResponse`（先整包读入）；Python SDK 源码检索 `stream` 无命中。检索命令：`curl -sL https://docs.typesafe.ai/llms-full.txt | grep -in "stream\|text/event-stream\|websocket"`。第三方佐证：Spring AI 写 "Jev does not stream. Nothing is generated token by token"。（<https://docs.typesafe.ai/sdk/javascript/api/classes/TypeSafeClient.md>、<https://raw.githubusercontent.com/typesafe-ai/typesafe-sdk-js/main/src/client.ts>、<https://github.com/typesafe-ai/typesafe-sdk-python>、Spring AI 博客）【官方负向结论 + 第三方】
### 本节不确定项
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
11. **第三方性能数字均为厂商自测或小样本自测、未独立复现**：dev.to 指南明确写这些是 "TypeSafe's own numbers, self-run and unreproduced"，并引用官方自述"0% structured-output errors 是断言而非实测"。（<https://dev.to/valyuai/how-to-use-jev-a-practical-guide-to-typesafes-system-one-model-g5e>）【未证实】
12. **未成功获取的来源**：DataCamp 文章 <https://www.datacamp.com/blog/system-one-models-jev> 返回 HTTP 403，未纳入证据。
13. **官方未声明 v1 API 的版本化承诺**（废弃策略、兼容窗口）；SDK 版本号（Python 0.7.1 / JS 0.6.0）只说明客户端仍在快速迭代。
14. **无采样旋钮**：Pydantic AI 集成文档称 `temperature`/`top_p` 等设置被忽略。（<https://pydantic.dev/docs/ai/models/typesafe/>）【第三方，官方文档未见明文，属待核】
## 三、能力边界、失败模式与已知限制

**要点**

1. 官方在 `jev-1.13` **jaggedness 页自列 9 类失败模式**，并给出每类"改用代码/生成模型"的替代做法，是能力边界最权威的一手清单。【官方】
2. **"不能幻觉"是形状保证，不是正确性保证**——官方博客自陈 0% 幻觉率 **"not empirical"**（非实证），是 schema 匹配的构造性结论。【官方】
3. **校准是群体统计，不保证单个答案**；且官方**未公布任何 ECE / reliability diagram 之类的标准校准指标**，第三方普遍将此作为核心质疑。【官方 + 第三方】
4. **结构性不变式不成立**：同一问题的 Noul 与 Choice 结果可相差极大（Noul 0.22 vs Choice "yes" 0.01），`P(noul)` 与 `1 − P(¬noul)` 不可比（0.72 + 0.47 = 1.19）——**阈值不能跨问法搬运**。【官方】
5. **官方评测站数字显示 Jev 在成本与延迟上压倒性领先，但不是准确率领先者**（四任务等权 67.8%，最高为 sol 74.1%）。【官方】
6. **硬限制明确**：64k/32k 上下文、Choice ≤255、Score 2–10 级、仅文本输入、英文最佳、无微调、限流 250k tok/s + 1,200 rpm。【官方】
### 3.1 官方自列的 jaggedness 九条（`jev-1.13`）
官方原文开头即承认："jev-1.13 is fast, calibrated, and good at common-sense judgment but **it is not perfect**… It may struggle with tasks that require additional levels of indirection. It can be quite literal in its understanding. It struggles with tasks that require numeric precision."（jaggedness 页）【官方】
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
### 3.2 Jev 明确不做的事 / 不适合交给它的任务
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
- 官方不保证输出幂等：同一 state 重复调用概率仍会小幅波动（见 §3.4 的 `covered` 0.43–0.53）。【官方】
- 把问题写进 `state`（prompt）里**不会被当作指令**：写到 state 里的问题只是"待判断的文本"；框架层几乎没有东西会替你报错（集成层只有在完全没有可提问题时才报 UserError）。（<https://pydantic.dev/docs/ai/models/typesafe/>）【第三方（框架集成文档）】
### 3.3 硬限制一览
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
### 3.4 校准与置信度的可靠性边界
**官方怎么说**
- confidence 是由概率分布形状导出的 0–1 统计量，**只出现在 Choice / Score 上；Noul 不带 confidence**。（<https://docs.typesafe.ai/confidence>）【官方】
- **confidence ≠ 正确性**：Score 文档原文 "confidence 1.0 means the returned distribution puts all its probability on one level. **This describes the model's answer, not a guarantee that the answer is correct.**"（<https://docs.typesafe.ai/primitives/score>）【官方】
- **校准被限定为群体统计**："Outcomes assigned a probability of 0.8 should occur about 80% of the time… **These rates describe groups of predictions, not a guarantee about any single answer.**"（<https://docs.typesafe.ai/introduction/machine-learning-primer>）【官方】
- 官方可操作做法：confidence 划三段（高→自动执行、中→复核/追问、低→转人）；阈值随风险调整。（<https://docs.typesafe.ai/confidence>）【官方】

官方拿得出的量化证据
| 证据 | 数字 | 语境与出处 |
| --- | --- | --- |
| 置信度分层确实有效 | 60 份 10-K、75 类 SIC 行业组、单条 Choice：confidence ≥ 0.9 的一半准确率 **90%**；< 0.9 的一半仅 **40%**；后者改报 division 可升到 **70%** | **jev-1.12**，2026-08-12，官方 cookbook <https://docs.typesafe.ai/cookbooks/classification_using_confidence>【官方】 |
| 同 state 重复调用稳定性 | 14 个 Noul × 15 次重复：每问题概率标准差 **0.0102**（低于同测所有 LLM 概率条件）；但 `covered` 一题在 **0.43–0.53** 间波动、**跨越 0.5 决策线** | jev-latest，2026-09-11，官方 cookbook <https://docs.typesafe.ai/cookbooks/consistency_noul_cookbook>【官方】 |
| 官方对不确定区间的自我限定 | 0.30–0.70 的"不确定带"被标注为 "**neither a calibrated guarantee nor an optimized threshold**" | 同上【官方】 |
| 官方对 LLM 校准弱点的动机陈述 | "Even if prompted for a confidence estimate, models tend to be overconfident and inconsistent." | 官方发布博客【官方】 |

**边界与风险（第三方指出、官方未反驳）**
- **没有公开的校准曲线 / ECE / reliability diagram**：官方只断言校准，未发布可核对的校准度量。（<https://www.laura-martel.com/blog/typesafe-jev-system-one-models>、<https://www.truefoundry.com/blog/typesafe-ai-jev>、Kingy 评测）【第三方】
- **官方评测的参考标签不是真值**，而是 GPT-6 Astra 与 Claude Fable 5.1（both at high thinking）回答的平均；评测站还写明 "**Assume the harness is correct**"。那批数字是"与两个强模型的一致度"，不是"与现实的一致度"。（<https://evals.typesafe.ai/>）【官方】
- **组合会破坏置信度语义**：单步 92% 的分类接 88% 的路由并不等于 81%，真实可靠性取决于误差相关性，而 "neither Jev nor the pipeline framework can report"。（<https://temperaturezero.com/2026/09/16/jev-doesnt-hallucinate-it-decides-wrong/>）【第三方】
- **合法输入下也可能"自信地错"**：创始人本人在 HN 回帖承认 "because these models are probabilistic, it's also possible to be confidently wrong"。（<https://news.ycombinator.com/item?id=49717558>）【第三方（创始人身份发帖）】
- 官方仍未公开：RLCD 具体实现、校准如何度量、分布偏移/长尾标签下概率是否仍校准、p95/p99 延迟与并发表现。（Kingy 评测）【第三方整理】
### 3.5 "零幻觉"的确切含义
官方原话（官方发布博客）【官方】：
> "While Jev gives up string generation, it's optimized for structured outputs and can't hallucinate."
同页 Nuance 里官方自己拆台：
> "**Our number is not empirical.** Schema matching is guaranteed, thus we can confidently add 0% into the plots."
因此"零幻觉"成立的前提是：
1. **它只断言"输出不越出你声明的 schema"**：选项只有 `billing/technical/sales` 时不会返回第四个标签（第三方表述："It will not invent 'urgent_priority'. It will not return null. It will not return a float."）。官方称 "no type errors… mathematically impossible" 并欢迎用反例证伪——这是一个**可被单个反例证伪的构造性保证**。（jaggedness 页、temperaturezero）【官方 + 第三方】
2. **它不覆盖**：选错合法选项、读漏要点、给错误答案一个很高的概率/置信度。第三方措辞：**"Can't hallucinate" is a narrower claim than it sounds**；The Register 直接说 "which really isn't a fair comparison as its output is not natural language"。（<https://www.firecrawl.dev/blog/what-is-jev>、The Register）【第三方】
3. **"类型安全"本身并不新颖**：第三方指出靠 grammar-constrained decoding（OpenAI Structured Outputs、Outlines 等）在自回归 LLM 上早已能做到同等的形状保证；真正未被独立验证的是**校准**。（Laura Martel）【第三方】
4. **术语歧义提醒**：官方训练法 RLCD 与 ICLR 2024 的 RLCD 重名，引用时须写清（见 §1.2）。（同上）【第三方】
**建议固定的术语（避免"零幻觉 = 零错误"的语义滑移）**：**类型/结构幻觉**按设计被消除（输出空间预先枚举）；**语义判断错误**仍存在，必须自己测（官方评测站与 cookbook 都在测这个）。误用后果类型学：把"类型合法但错误、且置信度偏高"的判定静默送进下游自动化——比幻觉更难被发现，因为"段落至少能被人读到并质疑，而 Jev 的输出会静默进入下一步"。（temperaturezero）【第三方】
### 3.6 官方评测：Jev 领先成本与延迟，不领先准确率
官方评测站（<https://evals.typesafe.ai/>，本段简称评测站）方法：4 个真实任务各拆成"代码规则 + 原子问题"的 workflow，所有模型跑同一套 workflow；参考标签为 GPT-6 Astra 与 Claude Fable 5.1（high thinking）对每个问题作答后的**平均**；指标是"与参考的一致率"；每个点 = 4 个 workflow 的等权平均。【官方】
| 工作流 | Jev（workflow） | 表内最强对手（workflow） | 差距 |
| --- | --- | --- | --- |
| Security Incidents | 61.7% / $0.0001 / 0.3 s | opus 5：66.2% / $0.0574 / 15.1 s | −4.5 pp，便宜 ~570x、快 ~50x |
| Agent Trace Observability | 71.6% / $0.0003 / 0.5 s | sol：76.6% / $0.0575 / 40.3 s | −5.0 pp，便宜 ~190x、快 ~80x |
| Invoice Processing | 61.8% / $0.0011 / 0.5 s | sol：79.1% / $0.2152 / 34.3 s | **−17.3 pp（差距最大，官方未回避）** |
| Customer Service | 76.0% / $0.0001 / 0.4 s | sol：78.3% / $0.0323 / 10.1 s | −2.3 pp，便宜 ~320x、快 ~25x |
| **4 项等权平均** | **67.8% / $0.0004 / 0.4 s** | sol 74.1% / $0.0836 / 23.3 s；opus 5 73.1% / $0.1761 / 37.8 s；terra 67.9% / $0.0304 / 10.1 s | Jev 不领先准确率 |
同表其他模型（workflow 路线）：sonnet 5 67.8%、luna 66.8%、DS v4 pro 65.5%、DS v4 flash 64.4%、haiku 4.5 53.6%。（评测站）【官方】
**官方自陈的偏差（引用上述数字时必须一并带出，官方发布博客）**【官方】
1. 这些 workflow 由 TypeSafe 自己的 model capabilities 团队编写，可能存在偏差（虽不在训练分布内）；2. 参考答案是 GPT-6 Astra 与 Fable 5.1 的平均，偏向 OpenAI/Anthropic，可能低估自家与 DeepSeek 的相对表现；3. 对比的 LLM 走 TypeSafe 的 "System One LLM wrapper"（约束成结构化决策输出），是官方认为最准但更慢更贵的做法；4. speed 评测多在公司笔记本电脑上跑（服务部署在美国西海岸）；5. 价格是否被补贴无法自证，官方预期长期价格会降而非涨；6. 幻觉对比图中 LLM 的数字来自 OpenRouter 流量，存在"更难的问题流向更强模型"的偏差；**自家 0% 类型错误不是实测，是构造性保证**。
- 头条倍数 **193.6x / 444.6x** 无法从评测页四舍五入后的数据复现（Kingy 亦指出），官方自己说"预期这是真实收益的高位"；**不要当作通用倍数**。（<https://typesafe.ai/>、Kingy 评测）【官方 + 第三方】
### 3.7 第三方实测与社区评价
| 来源 | 关键内容 |
| --- | --- |
| <https://every.to/also-true-for-humans/mini-vibe-check-typesafe-s-jev-judged-everything-i-ve-written-in-0-7-seconds>【第三方】独立实测（小样本） | 37 篇文档 × 21 问 = **777 条判断，不到 0.7 秒**，估算成本约 **1/4 美分**；整套实验 1,709 条判断不到 1 美分。12 段合成文本对照：Jev 中位 **0.35 s** vs Fable 5.1（high effort）**8.83 s**（约 25x 快）、估算成本低约 **580x**；但 **Jev 只抓到 7 个故意缺陷中的 6 个，Fable 全中**。作者结论：适合当"早期预警系统"，投产前需更严谨的准确率验证。（原文在付费墙内，数字经两处转述交叉核对：<https://www.firecrawl.dev/blog/what-is-jev>、Kingy 评测） |
| <https://news.ycombinator.com/item?id=49717558>【第三方】社区讨论 | 1945 分、511 评论；最高赞质疑即"不能输出非法类型 ≠ 不会输出错误的合法值"；有工程师要求看到"置信度与实际正确率对齐"的证据（0.9 的 1000 个答案里应有约 900 个正确） |
| <https://pi.dev/packages/pi-warden>【第三方】项目自测（非独立第三方） | coding agent 动作护栏（用 Jev 判定 bash/write/edit）：跨 315 个会话、**18,075 次受保护调用**触发 **48 次 hold**（约 3 次/1000 调用）；13 个被标注的 hold 中 **2 个站得住、11 个重试后通过**；另有 150 组配对运行对照（对照组违反项目规则 6 次，启用后 0 次）。作者自注 "These are project-maintained benchmarks, not universal claims." |
| <https://www.firecrawl.dev/blog/what-is-jev>【第三方】 | openjev（接口复刻、非模型）在 102 个与官方评测对齐的用例上，其公开模型与 Jev 公布的模态一致度为 **0.845 vs Jev 的 0.883** |
| The Register【第三方】媒体 | "无幻觉"与自然语言输出不可比；返回结构化概率 "does not preclude the possibility of being incorrect" |
| Laura Martel【第三方】 | 类型保证自 2024 年起已是 LLM + 受限解码的既有能力；真正难的是内容层面的校准，官方未给曲线；RLCD 与 ICLR 2024 既有方法重名 |
| TrueFoundry / Kingy【第三方】 | 都强调"厂商可核对项"与"自报项"的界线；Kingy 明确声明**未做在线实测**（无 API key），只做文档与评测站审查，并列出官方仍未公开的六项：架构差异、RLCD 实现、分布偏移下的校准、p95/p99 与并发、加上下游 LLM 后还剩多少成本优势、与经典分类器/重排器的对比 |
| <https://www.reddit.com/r/PiCodingAgent/comments/1whsav6/anyone_else_testing_out_typesafe_ais_new_system/>、<https://www.reddit.com/r/AI_Agents/comments/1wl82fr/tried_typesafe_ais_jev_vs_a_regular_llm_for_model/>【第三方】社区轶事 | 社区称"约 150ms"级别的判断延迟；有人做了模型路由对比测试。**属轶事级，未采用为结论** |
### 本节不确定项
1. **校准是否真的成立**：没有公开的 ECE / reliability diagram / 独立复测。官方只有"设计目标 + 分层证据（90% vs 40%）"，第三方只有"未公开"这一事实。**不要写"Jev 已证明校准良好"。**
2. **官方博客 FAQ 的正文答案未取到**（折叠组件，抓取只有问题没有答案）。
3. **各 workflow 的样本量 N（240/117/150/204，合计 711）**：只见于第三方复核文章，未能在官方页面静态数据中核实；引用时须标注来源为第三方。（Kingy 评测）【未证实】
4. **193.6× / 444.6× 一类倍数**：官方自述来自自建 workflow evals 且"expected to be on the higher end"；从评测站四舍五入后的表**无法反推**。不要当通用倍数使用。
5. **输出 token 的免费与用量字段**：定价写 output tokens FREE，但第三方给的响应示例里 `usage` 含 `output_tokens: 48`；是否总会返回、是否计费口径一致，未见官方明确说明。
6. **对非英语（含中文）表现**：官方只说 "handled but not equally well"，**没有任何具体数字**；也未找到不同语言在 Score 层级语义上行为差异的公开信息。
7. **对抗鲁棒性的量化**：官方只说"可以移动答案、未来会改进"，**无任何成功率/攻击成功率数字**。
8. **长期价格可持续性**：官方自己写 "We can't prove it isn't subsidized"；成本类结论依赖厂商定价不变。
9. **RLCD 的技术细节**：官方未披露奖励/评分规则与校准度量方式；HN 上有工程师给出"负奖励偏斜分布"的通用做法描述，属个人解读。【未证实】
10. **生产可靠性**：无独立 p95/p99、并发、重试率、可用性数据；早期访问阶段的实测样本都很小（777 条判断 / 18,075 次护栏调用）。
11. **第三方评测的边界**：ayautomate 是自测 791 条标注决策（真实测量）；dev.to 自陈"均为 TypeSafe 自报、未经复现"；opentweet 是聚合型站点（自称读取官方 models 页与 API 参考，2026-09-18）。它们用于交叉验证"字段名、上限值、请求形状"这类结构性事实可靠，但其中的**基准数字属各自口径，不能与官方数字混用**。（ayautomate、<https://dev.to/valyuai/how-to-use-jev-a-practical-guide-to-typesafes-system-one-model-g5e>、<https://opentweet.io/jev/limits>）【未证实（聚合/口径不明）】
12. **"置信度能排序难易"的证据来自 jev-1.12**（2026-08-12 的 10-K cookbook），未在 `jev-1.13.0` 上复现；跨版本外推需谨慎。
## 附：本片段使用的主要来源（全 URL，按首引顺序）
**官方**：<https://docs.typesafe.ai/concepts/system-one>、<https://docs.typesafe.ai/introduction/machine-learning-primer>、<https://typesafe.ai/blog/introducing-system-one-models-and-jev>、<https://docs.typesafe.ai/models>、<https://docs.typesafe.ai/api>、<https://docs.typesafe.ai/primitives>、<https://docs.typesafe.ai/primitives/choice>、<https://docs.typesafe.ai/primitives/score>、<https://docs.typesafe.ai/primitives/noul>、<https://docs.typesafe.ai/primitives/advanced>、<https://docs.typesafe.ai/concepts/state>、<https://docs.typesafe.ai/confidence>、<https://docs.typesafe.ai/introduction/quickstart>、<https://docs.typesafe.ai/introduction>、<https://docs.typesafe.ai/introduction/coding-agents>、<https://docs.typesafe.ai/migrating-to-v1.md>、<https://docs.typesafe.ai/model-jaggedness/jev-1.13>、<https://docs.typesafe.ai/patterns/fan-out>、<https://docs.typesafe.ai/patterns/composite-scoring>、<https://docs.typesafe.ai/cookbooks/parallel_questions>、<https://docs.typesafe.ai/cookbooks/classification_using_confidence>、<https://docs.typesafe.ai/cookbooks/consistency_noul_cookbook>、<https://docs.typesafe.ai/cookbooks/classifying_rag_passages>、<https://docs.typesafe.ai/cookbooks/entity_alignment>、<https://docs.typesafe.ai/sdk/python/api/constants.md>、<https://docs.typesafe.ai/sdk/python/api/clients/sync.md>、<https://docs.typesafe.ai/sdk/python/api/types/responses.md>、<https://docs.typesafe.ai/sdk/python/api/exceptions.md>、<https://docs.typesafe.ai/sdk/python/api/retries.md>、<https://docs.typesafe.ai/sdk/javascript>、<https://docs.typesafe.ai/sdk/javascript/api/classes/TypeSafeClient.md>、<https://docs.typesafe.ai/sdk/javascript/api/interfaces/RetryPolicy.md>、<https://docs.typesafe.ai/llms-full.txt>、<https://evals.typesafe.ai/>、<https://typesafe.ai/>、<https://typesafe.ai/blog/antibenchmaxxing>、<https://typesafe.ai/team>、<https://typesafe.ai/manifesto>、SDK 源码 <https://github.com/typesafe-ai/typesafe-sdk-python>、<https://github.com/typesafe-ai/typesafe-sdk-js>、<https://pypi.org/pypi/typesafe-sdk/json>、<https://registry.npmjs.org/@typesafe-ai/sdk>
**第三方**：<https://www.latent.space/p/jev>、<https://www.latent.space/p/ainews-jev-a-system-one-model-that>、<https://www.businesswire.com/news/home/20260915525333/en/TypeSafe-AI-Emerges-From-Stealth-With-%2440M-in-Funding-With-New-Model-for-Composable-AI>、<https://www.dcvc.com/news-insights/typesafe-emerges-from-stealth-with-a-new-way-of-doing-ai>、<https://www.finsmes.com/2026/09/typesafe-ai-raises-40m-in-seed-funding.html>、<https://www.hpcwire.com/aiwire/2026/09/16/typesafe-ai-emerges-from-stealth-with-40m-in-funding-with-new-model-for-composable-ai/>、<https://www.theregister.com/ai-and-ml/2026/09/16/typesafe-ai-debuts-model-for-machines-that-plays-doom/5296711>、<https://news.ycombinator.com/item?id=49717558>、<https://every.to/also-true-for-humans/mini-vibe-check-typesafe-s-jev-judged-everything-i-ve-written-in-0-7-seconds>、<https://www.firecrawl.dev/blog/what-is-jev>、<https://temperaturezero.com/2026/09/16/jev-doesnt-hallucinate-it-decides-wrong/>、<https://www.laura-martel.com/blog/typesafe-jev-system-one-models>、<https://kingy.ai/blog/typesafe-jev-review-the-ai-model-that-doesnt-generate-text/>、<https://www.truefoundry.com/blog/typesafe-ai-jev>、<https://www.marktechpost.com/2026/09/19/typesafe-ai-releases-jev/>、<https://spring.io/blog/2026/09/21/spring-ai-typesafe-structured-judgment>、<https://www.langchain.com/blog/building-a-harness-with-jev>、<https://dev.to/valyuai/how-to-use-jev-a-practical-guide-to-typesafes-system-one-model-g5e>、<https://www.ayautomate.com/blog/how-to-use-jev-guide>、<https://flaviocopes.com/jev/>、<https://hexdocs.pm/typesafe_ai/readme.html>、<https://opentweet.io/jev/limits>、<https://pydantic.dev/docs/ai/models/typesafe/>、<https://docs.litellm.ai/docs/pass_through/typesafe>、<https://pi.dev/packages/pi-warden>、<https://xhinker.medium.com/jev-the-most-overrated-model-of-2026-even-before-the-year-end-0f86838ca419>、<https://www.reddit.com/r/PiCodingAgent/comments/1whsav6/anyone_else_testing_out_typesafe_ais_new_system/>、<https://www.reddit.com/r/AI_Agents/comments/1wl82fr/tried_typesafe_ais_jev_vs_a_regular_llm_for_model/>
**未采用**：<https://www.datacamp.com/blog/system-one-models-jev>（HTTP 403，未纳入证据）；Instagram 来源的 $200M 估值传闻（未核实，未采信）。
