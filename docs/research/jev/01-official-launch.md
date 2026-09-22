# 专题 01：Jev 官方发布与技术原理

调研日期：2026-09-22。所有事实均附来源 URL；明确区分「官方口径」与「第三方/实测」。

---

## 一、结论要点

1. **Jev 是 TypeSafe AI 的第一个公开模型，属于其自创品类 "System One Model"**。它不走"文本生成 + 解析"路线，而是输入 state（文本 / JSON object / 数组）+ 一组带类型的 questions，直接返回类型化答案与概率分布，官方定位为"给软件调用的 frontier-intelligence function call"。([官方博客](https://typesafe.ai/blog/introducing-system-one-models-and-jev)、[官方文档 System One 概念页](https://docs.typesafe.ai/concepts/system-one))
2. **训练方法是 RLCD（Reinforcement Learning for Calibrated Decisions）**，与 RLHF（优化人类偏好）、RLVR（优化可程序化验证的奖励）并列的第三条后训练路径，目标函数是"概率要诚实"——即让模型给出的概率与真实正确率相符。([官方 AI primer](https://docs.typesafe.ai/introduction/machine-learning-primer))
3. **"不生成字符串"的确切含义**：模型不产出自由文本，不写回复、不写代码、不解释推理过程；输出被约束在预先声明的答案空间内（Choice / Score / Noul），因此结构上不可能出现类型错误；官方称"无法幻觉（can't hallucinate）"指的是这一点。([官方文档 System One](https://docs.typesafe.ai/concepts/system-one)、[官方博客](https://typesafe.ai/blog/introducing-system-one-models-and-jev))
4. **并行采样而非自回归逐 token 生成**是速度来源：一次请求中所有问题并行求值，"增加问题几乎不增加响应时间"。([官方博客](https://typesafe.ai/blog/introducing-system-one-models-and-jev)、[官方文档 Introduction](https://docs.typesafe.ai/introduction))
5. **架构与规模未披露**。官方只给出三句话："新模型架构 + 并行采样器（parallel sampler）+ RLCD"。"参数量、架构细节、硬件、训练数据配方、model card"均未公开，官方也明确表示不发布标准 benchmark 表。([官方首页](https://typesafe.ai/)、[官方博客《Lies, Damned Lies, and Benchmarks》](https://typesafe.ai/blog/antibenchmaxxing)、[第三方 Kingy 评测](https://kingy.ai/blog/typesafe-jev-review-the-ai-model-that-doesnt-generate-text/))
6. **发布状态（截至 2026-09-22）：early access + waitlist**，hosted API，无自部署。发布日 2026-09-15，同时宣布结束隐身期与 $40M 种子轮（DCVC 领投）。([官方博客](https://typesafe.ai/blog/introducing-system-one-models-and-jev)、[Businesswire 新闻稿](https://www.businesswire.com/news/home/20260915525333/en/TypeSafe-AI-Emerges-From-Stealth-With-%2440M-in-Funding-With-New-Model-for-Composable-AI)、[DCVC](https://www.dcvc.com/news-insights/typesafe-emerges-from-stealth-with-a-new-way-of-doing-ai))

---

## 二、关键事实与数据

### 2.1 发布、公司与时间线

| 事实 | 内容 | 来源 | 性质 |
| --- | --- | --- | --- |
| 发布时间 | 2026-09-15 | 博客页面渲染日期 "Sep 15, 2026"；[Latent Space AINews](https://www.latent.space/p/ainews-jev-a-system-one-model-that) 亦载 "Sep 15, 2026" | 官方页面 + 第三方 |
| 公司 | TypeSafe AI，2024 年成立，旧金山 | [Businesswire 新闻稿](https://www.businesswire.com/news/home/20260915525333/en/TypeSafe-AI-Emerges-From-Stealth-With-%2440M-in-Funding-With-New-Model-for-Composable-AI)（标题与摘要）、[HPCwire 转载](https://www.hpcwire.com/aiwire/2026/09/16/typesafe-ai-emerges-from-stealth-with-40m-in-funding-with-new-model-for-composable-ai/) | 第三方（公司新闻稿） |
| 融资 | $40M 种子轮，DCVC 领投（DCVC 自述为 Series Seed） | [DCVC](https://www.dcvc.com/news-insights/typesafe-emerges-from-stealth-with-a-new-way-of-doing-ai)、[Businesswire](https://www.businesswire.com/news/home/20260915525333/en/TypeSafe-AI-Emerges-From-Stealth-With-%2440M-in-Funding-With-New-Model-for-Composable-AI)、[Finsmes](https://www.finsmes.com/2026/09/typesafe-ai-raises-40m-in-seed-funding.html) | 第三方（官方新闻稿） |
| 创始人 | Diogo Almeida（CEO，前 OpenAI，InstructGPT 论文作者之一）、Erik Gafni、Sasha Sheng | [TrueFoundry](https://www.truefoundry.com/blog/typesafe-ai-jev)、[Finsmes](https://www.finsmes.com/2026/09/typesafe-ai-raises-40m-in-seed-funding.html) | 第三方 |
| 团队/公司自述 | 团队来自 OpenAI、Google Brain、Meta/FAIR、Stripe、Airbnb、Plaid、Docker 等；SF Embarcadero 附近办公室、每周 5 天坐班 | [官方 Team 页](https://typesafe.ai/team) | 官方口径 |
| 发布热度 | 发布当天持续占据 Hacker News 头条 | [Latent Space AINews](https://www.latent.space/p/ainews-jev-a-system-one-model-that) | 第三方 |

### 2.2 System One 模型的定义（官方原文）

- "System One models are a class of AI models built to make fast, structured decisions that software can use directly. A System One model evaluates a state and returns typed answers and probabilities."([官方文档](https://docs.typesafe.ai/concepts/system-one))
- "Like an LLM, a System One model understands natural-language input. It returns typed decisions and probabilities rather than generated text."(同上)
- "System One models do not write replies, produce code, or generate explanations of their reasoning."(同上)
- 命名来源：Kahneman《思考，快与慢》的 System 1（快、直觉）vs System 2（慢、审慎）；"Jev" 取自 William Stanley Jevons（杰文斯悖论：单位成本下降导致总需求上升）。([官方博客 FAQ](https://typesafe.ai/blog/introducing-system-one-models-and-jev))
- 公司层面叙事："Machine Native Intelligence"（AI 具备结构、可靠、可观测、可测试、快、便宜等软件属性）、"We're building prod, not God"、预期未来 AI 自动化中约 99% 是机器对机器、1% 是人对人。([官方 AI primer](https://docs.typesafe.ai/introduction/machine-learning-primer)、[官方 Manifesto](https://typesafe.ai/manifesto))

### 2.3 RLCD（Reinforcement Learning for Calibrated Decisions）

- 定义：官方把三条后训练路径并列——RLHF 优化人类偏好（并自陈其会导致 sycophancy、自信的幻觉、mode dropping）；RLVR 优化可程序化验证的奖励（数学等，但更慢更贵）；**RLCD 优化"决策 + 校准概率"**，不生成文本。([官方 AI primer](https://docs.typesafe.ai/introduction/machine-learning-primer))
- 校准的官方定义（重要，避免误读）："Higher probability should correspond to a greater chance that the answer is correct."；概率 0.2 的结果应在大约 20% 的情况下发生，0.8 对应约 80%，1.0 对应 100%。**"These rates describe groups of predictions, not a guarantee about any single answer."**([官方 AI primer](https://docs.typesafe.ai/introduction/machine-learning-primer)、[官方 Confidence 页](https://docs.typesafe.ai/confidence))
- 官方未公开 RLCD 的奖励函数形态、训练数据配方与校准度量方法（详见第四节）。
- 官方口径（访谈）：Diogo Almeida 表示公司的训练数据**全部为合成数据**，且刻意不用用户数据训练；文档侧表述为 "Jev is not fine-tuned or LoRA-adapted with customer data… the same weights serve every account"，以及 "Jev is not trained on customer requests or responses"。前者是播客访谈口径，后者是文档口径。([Latent Space 访谈转录](https://www.latent.space/p/jev)、[官方 Models 页](https://docs.typesafe.ai/models))

### 2.4 与 LLM 的本质差异（官方对照表，节选翻译）

来源：[官方发布博客](https://typesafe.ai/blog/introducing-system-one-models-and-jev)

| 维度 | 现有 LLM | System One / Jev |
| --- | --- | --- |
| 优化目标 | RLHF（人类偏好）/ RLVR（可验证奖励） | RLCD（校准决策） |
| 输入 | 非结构化数据（文本），偏重消息序列 | 非结构化数据（文本），偏重结构化程序状态 |
| 输出 | 字符串；要交给软件用必须解析 + 校验，且可能跑偏 | 类型化结构值；可能输出与结构预先定义，永不出类型错误，答案附带校准概率与置信度 |
| 采样 | 顺序，逐 token，每个 token 依赖上一个 | 并行，单次查询产出全部输出 |
| 成本 | 输入 $0.20–$10 / MTok，输出约为输入的 5 倍价 | 输入 $0.042 / MTok（$42/Btok），输出免费（"too cheap to meter"） |
| 速度 | 前沿模型端到端 3–329 秒 | 70ms–500ms；同类 System One 任务上快 40x–200x |
| 置信度 | 即便被要求给置信度也常过度自信、不一致 | 每个输出都带置信度；越自信越准；对相似输入返回相似答案 |
| 适用场景 | 人在环任务（chatbot、copilot、coding agent） | AI 驱动的 workflow /「smart if-statements」：分类、路由、打分、抽取、分支；大数据 map-reduce；实时应用；对 LLM 提示/推理链/输出做打分、评判、校验、护栏、jailbreak 检测 |

补充（第三方对官方措辞的解读，属于**第三方观点**）：官方"can't hallucinate / zero hallucinations"应理解为"不可能产出 schema 之外的畸形输出"，**不等于语义正确**——模型仍可在允许的选项里选错，也可以给出过度自信的概率。([TrueFoundry](https://www.truefoundry.com/blog/typesafe-ai-jev)、[Kingy 评测](https://kingy.ai/blog/typesafe-jev-review-the-ai-model-that-doesnt-generate-text/))

### 2.5 官方披露的架构与规模（关键：披露极少）

官方原话：

> "We built a new stack entirely focused on automation: with a new model architecture, parallel sampler for maximum efficiency, and training method we call Reinforcement Learning for Calibrated Decisions (RLCD)."([官方博客](https://typesafe.ai/blog/introducing-system-one-models-and-jev))

> "We're building with a new architecture, a new sampler, and a new training algorithm: Reinforcement Learning for Calibrated Decisions (RLCD)."([官方首页](https://typesafe.ai/))

访谈中 CEO 的补充（仍无规模信息）：Jev 属于 "machine-native, System 1, large programmable" 这一类，"goal is for code to be the consumer"；"我们连模型深层内部结构都在为软件优化"；公司自认是"data lab"、很看重合成数据。([Latent Space 访谈转录](https://www.latent.space/p/jev))

**未披露（经多方检索确认无公开信息）**：参数量、模型规模、具体网络结构（是否 transformer 之外的结构、是否有 classifier/diffusion 成分）、训练算力与硬件、训练数据构成细节、model card、权重。第三方评测同样把这几项列为"publication gap"。([Kingy 评测](https://kingy.ai/blog/typesafe-jev-review-the-ai-model-that-doesnt-generate-text/)、[Medium 批评文](https://xhinker.medium.com/jev-the-most-overrated-model-of-2026-even-before-the-year-end-0f86838ca419))

### 2.6 技术规格（官方文档，逐条可查）

来源：[官方 Models 页](https://docs.typesafe.ai/models)、[Quick start](https://docs.typesafe.ai/introduction/quickstart)、[Confidence](https://docs.typesafe.ai/confidence)

- 当前版本：**Jev 1.13 / `jev-1.13.0`**；别名 `jev-latest`（最近的稳定官方版，SDK 默认）、`jev-preview`（最近一次发布，含 preview build）。
- 端点：`POST /v1/systemone`，`Authorization: Bearer <API_KEY>`；官方提供 Python / JavaScript SDK。
- 价格：**$42 / Btok（即 $0.042 / Mtok）输入，输出 token 免费**；按输入 token 计费。
- 限流：**250,000 tokens/秒；1,200 requests/分钟**；超限返回 429，SDK 默认带退避重试并遵守 `retry-after`。
- 上下文预算：**每请求 64k tokens**（= state + 全部问题合计）；state + 单个最长问题不得超过 **32k**。Jev 一次性摄入 state，所有问题并行求值。
- 输入形态：仅文本——字符串、JSON object、或文本数组；**不支持图像/音频/视频/二进制**，需自行预处理为文本或结构化字段。
- 语言：英文为主要训练语言、准确率最好；CJK 等其他语言可用但"不等价"，建议自行测试并关注 confidence。
- 自定义：**不做客户级微调/LoRA**，所有账号共享同一套权重；领域适配靠 state + questions 的 instructions/criteria + 代码里组合。
- 数据：不用客户请求/响应训练；企业客户可谈 ZDR（zero data retention）。
- 三种原语（question 类型）与返回：

| 原语 | 语义 | 返回 |
| --- | --- | --- |
| `Choice` | 从一组候选项中择一 | `choice` + `probabilities`（各选项分布）+ `confidence` |
| `Score` | 按有序等级评分 | `score` + `probabilities`（各等级分布）+ `confidence` + `legend` |
| `Noul` | 是非判断 | `noul`（0–1 的概率），**不含 confidence** |

- `confidence` 的定义：由概率分布"形状"派生的统计量，0–1；分布越平越不自信。官方建议的架构模式是三档：高置信自动执行 / 中置信谨慎或复核 / 低置信不执行（转人工或换模型），且阈值按风险分级（破坏性操作阈值高于只读操作）。
- `Choice` 选项上限 **255**；更高基数官方用"先独立打分、再做显式选择"的两阶段方式，并承认这会变慢。([官方博客](https://typesafe.ai/blog/introducing-system-one-models-and-jev))
- 官方明确建议：**数学、计数、日期比较等确定性逻辑留在代码里**，模型只做语义判断（详见 2.8 的 jaggedness 页）。

### 2.7 官方自建评测与主要数字

官方立场（重要）：TypeSafe 明确**不发布标准 benchmark 表**，理由是 benchmaxxing；官方采用"有日期的快照评测、发布后即退休"，并承诺公开 caveat、cherry-picking 情况与对自己不利的证据。([官方博客《Lies, Damned Lies, and Benchmarks》](https://typesafe.ai/blog/antibenchmaxxing))

工作流评测（workflow evals）方法（[evals.typesafe.ai](https://evals.typesafe.ai/)）：
- 4 个真实任务（Security Incidents、Agent Trace Observability、Invoice Processing、Customer Service），每个任务先被拆成"代码规则 + 原子问题（Noul/Choice/Score）"的 workflow；所有模型跑同一套 workflow（对比组另有 "prompt" 路线：把整套策略塞进一个提示）。
- 参考标签：用 GPT-6 Astra 与 Claude Fable 5.1（高思考档）对每个问题作答后的平均作为 consensus；其他模型用各自 provider 的默认推理档；指标是"与参考的一致率"。
- 每个点 = 一个模型配置在 4 个 workflow 上等权平均的 accuracy / cost / time。

主表数据（抓取自 [evals.typesafe.ai](https://evals.typesafe.ai/)，2026-09-22）：

| 模型 / 配置 | 平均准确率 | 单例成本 | 单例耗时 |
| --- | --- | --- | --- |
| **Jev · workflow** | **67.8%** | **$0.0004** | **0.4 s** |
| sol · workflow（表中准确率最高） | 74.1% | $0.0836 | 23.3 s |
| opus 5 · workflow | 73.1% | $0.1761 | 37.8 s |
| terra · workflow | 67.9% | $0.0304 | 10.1 s |
| sonnet 5 · workflow | 67.8% | $0.1174 | 78.1 s |
| luna · workflow | 66.8% | $0.0033 | 12.9 s |
| DS v4 pro · workflow | 65.5% | $0.0413 | 86.5 s |
| DS v4 flash · workflow | 64.4% | $0.0059 | 51.9 s |
| haiku 4.5 · workflow | 53.6% | $0.0195 | 12.5 s |

分工作流（同页 / 分页，标题即图中标注值）：

| 工作流 | Jev 准确率 / 成本 / 耗时 | 数字来源 |
| --- | --- | --- |
| Security Incidents | 61.7% / $0.0001 / 0.3 s | [security_incidents.html](https://evals.typesafe.ai/security_incidents.html) |
| Agent Trace Observability | 71.6% / $0.0003 / 0.5 s | [agent_trace_observability.html](https://evals.typesafe.ai/agent_trace_observability.html) |
| Invoice Processing | 61.8% / $0.0011 / 0.5 s | [invoice_processing.html](https://evals.typesafe.ai/invoice_processing.html) |
| Customer Service | 76.0% / $0.0001 / 0.4 s | [customer_service.html](https://evals.typesafe.ai/customer_service.html) |
| 4 项等权平均 | 67.8% / $0.0004 / 0.4 s | [evals.typesafe.ai](https://evals.typesafe.ai/)（与上面 4 个数字自洽） |

同页可比对手（同样逐页直接核实，均为 "workflow" 路线）：

| 工作流 | Jev | 表内最强对手（workflow） | 差距 |
| --- | --- | --- | --- |
| Security Incidents | 61.7% / $0.0001 / 0.3s | opus 5：66.2% / $0.0574 / 15.1s | −4.5pp，便宜 ~570x、快 ~50x |
| Agent Trace Observability | 71.6% / $0.0003 / 0.5s | sol：76.6% / $0.0575 / 40.3s | −5.0pp，便宜 ~190x、快 ~80x |
| Invoice Processing | 61.8% / $0.0011 / 0.5s | sol：79.1% / $0.2152 / 34.3s | −17.3pp（差距最大，官方未回避） |
| Customer Service | 76.0% / $0.0001 / 0.4s | sol：78.3% / $0.0323 / 10.1s | −2.3pp，便宜 ~320x、快 ~25x |

首页头条数字：**193.6x 更快、444.6x 更省**（对比对象是一组 LLM 配置；官方展示的单次对比为 TypeSafe $0.000081 / 0.114s vs LLMs $0.013880 / 8.566s）。官方自己说明："这就是 193.6x faster / 444.6x cheaper 的来源，我们认为这是真实世界收益的上限区间。"([官方首页](https://typesafe.ai/)、[官方博客](https://typesafe.ai/blog/introducing-system-one-models-and-jev))

**官方自陈的偏差（必须在引用数字时一并带出）**（[官方博客](https://typesafe.ai/blog/introducing-system-one-models-and-jev)）：
1. 这些 workflow 由 TypeSafe 自己的 model capabilities 团队编写，可能存在偏差（虽不在训练分布内）；
2. 参考答案是 GPT-6 Astra 与 Fable 5.1 的平均，偏向 OpenAI/Anthropic，可能低估自家与 DeepSeek 的相对表现；
3. 对比的 LLM 走 TypeSafe 的 "System One LLM wrapper"（约束成结构化决策输出），这是官方认为最准但更慢更贵的做法；
4. speed 评测多在公司笔记本电脑上跑（服务目前部署在美国西海岸）；
5. 价格是否被补贴无法自证，官方预期长期价格会降而非涨；
6. 幻觉对比图中 LLM 的数字来自 OpenRouter 流量，存在"更难的问题流向更强模型"的偏差；而自家 0% 类型错误不是实测，是构造性保证。

**公开 benchmark：官方 FAQ 中有"How does Jev perform against public benchmarks?"一问，但页面 HTML 中该问答内容为空**（官方博客 FAQ 共 6 问，仅"命名来源"一问有答案，其余 5 问为空；首页 FAQ 亦仅"What are System One Models? What is Jev?"第一条有答案，包括"Is Jev just a smaller LLM?"、"How can Jev be so fast and inexpensive?"、"Is Jev deterministic?"在内的其余问题在抓取到的页面中均为空）。截至 2026-09-22 未见官方公开 benchmark 分数。([官方博客](https://typesafe.ai/blog/introducing-system-one-models-and-jev)、[官方首页](https://typesafe.ai/))

### 2.8 官方主动披露的局限（`jev-1.13` jaggedness 页）

这是官方文档中信息量最大的一页，直接列出 9 类失败模式与建议做法（[官方 Model jaggedness 页](https://docs.typesafe.ai/model-jaggedness/jev-1.13)）：

1. **字面理解**：只按你写的字面回答，不读你的"本意"；建议把条件写死、边界写进 criteria。
2. **数学与数字**：不做计算；**计数不可靠**（字母数、词频、长列表计数，误差随规模增长）；十六进制/RGB 等数值化表示比英文名表现差；不要用 Score 期望值反推精确数值。
3. **日期时间比较**：把日期当文本读，不能可靠比较先后/区间；建议把日期各部分做成小集合的 Choice 抽取，算术留给代码。
4. **多跳/间接推理**（双重否定、属性之属性）：跳数越多越不准。
5. **大而杂的 state**：无关细节会干扰准确率；建议先检索/过滤。
6. **对抗性内容**：state 中的注入指令、误导性框架、自称应被分类为某类的文本都会影响答案（官方称未来会改进）。
7. **指令与 criteria 相互矛盾**：例如 Noul 中 true 映射为"否"，会明显变差。
8. **常识性结构不变式不成立**：官方给了实测例子——同一问题用 Noul 得 0.22、用 Choice "yes" 得 0.01 且 confidence 0.99；同一问题与其否定作两个 Noul 得 0.72 与 0.47（和为 1.19）。官方结论：**不要假设 P(a) 与 1−P(¬a) 可比，不要把 Noul 上调好的阈值搬到 Choice，也不要在不同问题之间要求算术恒等**；并指出 Choice 是相对的、Noul 是绝对的（所有 Noul 都可以同时低）。
9. **生成能力**：`jev-1.13` 未受文本生成训练；强行用 Choice 串联生成"效果差且非常慢"；抽取任务建议先用正则或生成模型取候选，再让 Jev 选。
10. 另注：官方称该模型"extremely consistent"——语义相似输入会得到量化上相似的输出。

CEO 在访谈中也承认："单跳判断你不该用别的东西（只能用 Jev）；跳数一多就开始退化。"([Latent Space 访谈转录](https://www.latent.space/p/jev))

### 2.9 第三方实测与评价（与官方口径区分）

| 来源 | 性质 | 关键内容 |
| --- | --- | --- |
| [Every（Mike Taylor, "Mini-Vibe Check"）](https://every.to/also-true-for-humans/mini-vibe-check-typesafe-s-jev-judged-everything-i-ve-written-in-0-7-seconds) | 独立实测（小样本） | 37 篇文档 × 每篇 21 个问题 = **777 条判断，不到 0.7 秒完成，估算成本约 1/4 美分**；整套实验 1,709 条判断成本不到 1 美分。Dan Shipper 用 12 段合成文本做对照：Jev 中位耗时 0.35 s vs Fable 5.1（high effort）8.83 s（**约 25x 快**），估算成本低 **约 580x**；但 **Jev 只抓到 7 个故意缺陷中的 6 个，Fable 全中**。作者结论：适合当"早期预警系统"，投入生产前需更严谨的准确率验证。 |
| [Kingy 评测（文档级审阅，无 API 实测）](https://kingy.ai/blog/typesafe-jev-review-the-ai-model-that-doesnt-generate-text/) | 第三方评述 | 汇总官方 dashboard 并逐工作流列出最佳可比模型（Security Incidents：Opus 5 66.2% / $0.0574 / 15.1s；Agent Trace：Sol 76.6% / $0.0575 / 40.3s；Invoice：Sol 79.1% / $0.2152 / 34.3s；Customer Service：Sol 78.3% / $0.0323 / 10.1s）——**这四个对手数字我们已在官方评测分页的图表数据中逐一核实**；其统计的总用例 **711**（240/117/150/204）则未能从评测页静态数据核实。给出两个重要保留：① 官方评测是"与模型参考答案的一致率"，不是真值；② **官方尚未公布标准校准指标（reliability/ECE 曲线）**，"zero hallucinations"只等于"零畸形输出"。作者明确声明未做 API 实测。 |
| [Latent Space AINews](https://www.latent.space/p/ainews-jev-a-system-one-model-that) | 第三方报道 | 记录发布当天 HN 头条、官方宣称（>100x 更快、>200x 更省）以及社区的反向解读："Jev 不是通用语言模型，更像受约束的/类 diffusion 的决策模型，不能产出自由文本，必须预先定义输出格式"——正确的心理模型是"廉价的、可校准的结构化选择推理引擎"，而非 GPT 替代品。 |
| [Latent Space 播客（Diogo Almeida, 2026-09-21）](https://www.latent.space/p/jev) | 一手访谈 | CEO 自述：System 1 是"描述 LLM 擅长什么"的最贴切说法；RLHF 是"取悦人类"、RLVR 是"优化 benchmark"、RLCD 是"为程序化使用而可靠"；公司是 data lab，全部训练数据为合成；不会用用户数据；对多跳推理会退化。 |
| [DataCamp](https://www.datacamp.com/blog/system-one-models-jev)、[TrueFoundry](https://www.truefoundry.com/blog/typesafe-ai-jev) | 第三方报道 | 与官方口径一致的事实复述（早期访问 + waitlist、$0.042/MTok、输出免费、70–500ms、255 选项上限、二阶段高基数选择）。TrueFoundry 又补充："Jev 是 only-hosted API，处于 early access，没有可部署的东西"；并对 193.6x/444.6x 明确标注为"self-reported, not independently reproduced"。 |
| [Medium 批评文（xhinker）](https://xhinker.medium.com/jev-the-most-overrated-model-of-2026-even-before-the-year-end-0f86838ca419) | 个人评论（低权威，仅供反方视角） | 指出"没有参数量、没有超越'非自回归 + 自家并行采样器'的架构说明、没有训练数据说明"。 |
| [Reddit r/PiCodingAgent](https://www.reddit.com/r/PiCodingAgent/comments/1whsav6/anyone_else_testing_out_typesafe_ais_new_system/)、[Reddit r/AI_Agents](https://www.reddit.com/r/AI_Agents/comments/1wl82fr/tried_typesafe_ais_jev_vs_a_regular_llm_for_model/) | 社区实测（轶事级） | 社区称"约 150ms"级别的判断延迟；有人做了模型路由对比测试。属轶事，未采用为结论。 |

---

## 三、请求-响应示例（官方文档原文）

端点与请求（[官方 Quick start](https://docs.typesafe.ai/introduction/quickstart)）：

```bash
curl -X POST https://api.typesafe.ai/v1/systemone \
  -H "Authorization: Bearer $TYPESAFE_API_KEY" \
  -H "Content-Type: application/json" \
  -d @- << 'EOF'
{
  "state": "Hi, I've been trying to connect my Stripe account for 3 days and the integration keeps failing. I'm losing sales. Please help ASAP.",
  "model": "jev-latest",
  "questions": {
    "department": {
      "type": "choice",
      "instructions": "Which team should handle this",
      "criteria": {
        "billing": "Payment or subscription issues",
        "technical": "Bugs or integration problems",
        "sales": "Pricing or account questions"
      }
    },
    "frustration": {
      "type": "score",
      "instructions": "How frustrated the customer appears",
      "criteria": ["Calm, just stating facts", "Frustrated but civil", "Very angry, strong language"]
    },
    "is_urgent": {
      "type": "noul",
      "instructions": "The message conveys urgency or time-sensitivity"
    }
  }
}
EOF
```

官方示例响应（同上）：

```json
{
  "model": "jev-1.13.0",
  "answers": {
    "department": {
      "type": "choice",
      "choice": "technical",
      "confidence": 0.78,
      "probabilities": { "technical": 0.85, "sales": 0.0, "billing": 0.15 }
    },
    "frustration": {
      "type": "score",
      "score": 1.0,
      "confidence": 1.0,
      "legend": { "0": "Calm, just stating facts", "1": "Frustrated but civil", "2": "Very angry, strong language" },
      "probabilities": { "0": 0.0, "1": 1.0, "2": 0.0 }
    },
    "is_urgent": { "type": "noul", "noul": 1.0 }
  },
  "usage": { "input_tokens": 392, "output_tokens": 65 }
}
```

Python SDK 形态（[官方 Quick start](https://docs.typesafe.ai/introduction/quickstart)）：

```python
from typesafe_sdk import Choice, Noul, Score, TypeSafeClient

client = TypeSafeClient()
response = client.system_one(
    state=ticket,
    questions={
        "department": Choice(instructions="Which team should handle this", criteria={...}),
        "frustration": Score(instructions="How frustrated the customer appears", criteria=[...]),
        "is_urgent": Noul(instructions="The message conveys urgency or time-sensitivity"),
    },
)
response.answers["department"].choice      # "technical"
response.answers["frustration"].score      # 1.0
response.answers["is_urgent"].noul         # 1.0
```

官方推荐的使用模式（[How to build with TypeSafe](https://docs.typesafe.ai/concepts/how-to-build-with-system-one)）：确定性状态先由代码短路处理；把原子问题一次性发出去并行求值；在代码里用权重合成信号（示例中 `spam_risk = 0.45*x + 0.30*y + 0.25*z`）；`0.4 < risk < 0.6` 或 `confidence < 0.75` 时转人工；再按 `Choice` 结果决定哪些"投机性"问题的答案有意义——即**投机式扇出（speculative fan-out）+ 代码决策**（[官方 Patterns: Speculative fan-out](https://docs.typesafe.ai/patterns/fan-out)）。

---

## 四、不确定与未证实之处

1. **架构与规模完全未公开**：无参数量、无结构说明（除"非自回归 + 并行采样器"）、无训练算力/硬件、无训练数据构成、无 model card、无权重。所有"它是不是小模型/是否类似 diffusion"的说法都是第三方推测，无官方依据。
2. **无任何独立校准验证**：官方宣称 RLCD 让概率"诚实"，但未公布 ECE / reliability diagram 之类的标准校准指标；第三方（Kingy）明确指出"confidence 的校准尚未被独立证明"。
3. **官方评测不是真值基准**：参考标签来自 Astra + Fable 5.1 的平均，指标是与参考的一致率；4 个工作流由官方团队自建；LLM 基线跑在官方自家结构化解码 wrapper 上。**用例数（711 = 240+117+150+204）来自第三方 Kingy 的统计**，我们未能从评测页静态 HTML 直接核实（页面为 JS 渲染），仅核实了各工作流的 Jev 数值与聚合 67.8%。
4. **首页头条 193.6x / 444.6x 无法从评测页 rounded 数据复现**（Kingy 也指出这点）；应按"官方自建工作流、特定负载下的厂商声明"引用，不要当作通用倍数。
5. **官方 FAQ 大面积无答案**：博客 FAQ 的 6 问中仅"命名来源"有答案，其余 5 问（"Why was a new training algorithm needed?"、"Is Jev just a smaller LLM?"、"How does Jev perform against public benchmarks?"、"Where does our training data come from?" 等）在抓取的 HTML 中只有问题标题、无答案内容；首页 FAQ 亦仅第一条有答案（"Is Jev just a smaller LLM?"、"How can Jev be so fast and inexpensive?"、"Is Jev deterministic?" 等为空）。这些问题的答案属于**未找到公开信息**。
6. **发布状态会变**：截至 2026-09-22 仍是 early access / waitlist（hosted API）；第三方 Medium 与 chatmaxima 于 1–2 天前的文章仍称"waitlist 中"。Rust 项目若依赖它做开发验证，需要预留无 API key 的降级路径。
7. **$40M 轮次之外的估值（$200M）仅见于 Instagram 帖子**，未在权威来源核实，故未采信。
8. **性能数据均为非生产条件**：官方速度数字多在"公司笔记本 + 美国西海岸服务"下测得；第三方实测样本极小（777 条判断 / 12 段文本），无 p95/p99、并发、可用性、重试率、版本漂移等生产数据。

---

## 五、对本项目（Rust Agent Harness，融合 LLM + Jev）的启示

1. **Jev 的官方定位与我们的"决策分层"原则完全一致，可以作为 Jev 侧的权威依据**：官方明确 Jev 适合分类/路由/打分/抽取/校验/护栏这类"smart if-statements"，并反复强调**数学、计数、日期比较、权限与副作用留在代码里**。因此 harness 里的路由、紧急度、动作门控、结果评分应对应 `Choice` / `Score` / `Noul` 三种原语；而多跳推理、文本生成、解释性输出留给 LLM——多跳任务上 Jev 会退化（官方文档与 CEO 均确认）。
2. **概率与置信度必须作为一等数据保留，且不能假设跨问题的结构不变式**：把 `Choice/Score` 的 `probabilities` 与 `confidence` 原样传入 harness 决策层，用三档置信度阈值（按风险分级）而不是布尔化；同时不要写"P(¬a)=1−P(a)"、不要把 Noul 阈值套到 Choice、不要用 Choice 的 confidence 解释 Noul——官方已给出反例（0.72 vs 0.47，和为 1.19）。另外注意 `Noul` 不返回 `confidence`，只有原始概率。
3. **工程接口应按"一次 state + 多问并行"设计，并把 64k/32k 预算与英文偏向写进架构约束**：单请求 state+全部问题 ≤ 64k tokens、state+单个最长问题 ≤ 32k；增加问题几乎不增加延迟，因此应默认批量扇出（含投机性问题）再由代码挑选；同时避免大而杂的 state（无关细节会掉准确率），预过滤后再送模型。鉴于早期访问 + waitlist 状态与版本别名会漂移（`jev-latest` → `jev-1.13.0`），harness 的模型抽象层应把 Jev 调用收敛到一个 trait/端口后面，并记录响应里的 `model` 版本字段，便于换实现、写 mock 测试与复现结果。
