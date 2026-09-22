# 专题 07：Jev 的应用场景与生产用法

> 调研日期：2026-09-22。所有数字均附来源 URL 与语境；明确区分「官方口径」与「第三方实测/评论」。
> 本文件只做资料整理，不代表项目已做出的技术决策。

---

## 一、结论要点

1. **Jev 在生产中的用法高度收敛为四类「决策原语」**：分类/分流（Choice）、评分（Score）、是非判定（Noul）、以及由这三者派生的**路由、门控、校验、抽取**。官方自己称之为「AI-Powered Workflows / smart if-statements」。
2. **主流生态已把它接进 Agent 循环的三个位置**：模型路由（LangChain `ModelRouterMiddleware`、LiteLLM `classifier_type: jev`、Pydantic AI `SelectModel`）、工具门控（LangChain `AutoModeMiddleware`、Pydantic AI `Hooks(before_tool_execute=...)`）、以及运行后的评测/判定（LangSmith、Langfuse、Braintrust、Arize）。
3. **官方和集成方都反复强调「状态与问题必须分离」**：state 只放「被判断的材料」，问题放在 JSON 的 `questions` / Pydantic 字段描述里。这是与 LLM 习惯最大的不同，写错不会报错而是静默降质。
4. **动作门控的工程约定是「失败关闭」（fail closed）**：LangChain 的 PR 明确写「tool gate fails closed, which is the only defensible default」；而路由与技能选择是「失败放行」（fail open），避免 TypeSafe 故障导致 agent 停摆。
5. **最大的生产风险是提示注入**：TypeSafe 官方 jaggedness 页、Pydantic 官方文档、独立实测（Octomind）三方一致。LangChain 的缓解手段是**不把工具输出/助手消息送进分类器**，即「agent 取回的内容不能给自己授权」。
6. **成本/延迟收益有真实第三方数据，但远小于官方宣传的 193.6x/444.6x**：LiteLLM 自测为 5.43x（p50 分类延迟）与 96.1% 分类成本节省；NearHere 自测为 0.59s vs 2.9–3.4s、$0.043 vs $0.370/$2.496 每千次决策。官方的高倍数是自家 workflow evals，且官方自己承认偏高。

---

## 二、关键事实与数据（附 URL）

### 2.1 官方口径：定位与用例清单

**来源 [1]：TypeSafe AI 官方发布博客** — https://typesafe.ai/blog/introducing-system-one-models-and-jev

官方给出的用例分类（表格原文）：

- **AI-Powered Workflows / smart if-statements**：「Structured outputs slot into ordinary software as fuzzy decision rules: classify, route, score, extract, or branch where hand-written logic is too brittle.」
- **Map-reducing over big data**：把 PB 级数据转成特征与洞察。
- **Real-time applications**：「100ms speeds means you can use AI in your applications where UX is critical.」
- **Verify everything**：「Score, judge, verify, guardrail, and detect jailbreaks of LLM prompts, reasoning traces, and/or outputs.」

官方性能口径（**自测，官方明确承认有偏**）：

| 指标 | 官方数字 | 语境 |
| --- | --- | --- |
| 端到端响应 | 70ms–500ms | 对比「frontier models 3 到 329 秒」 |
| 相对速度/成本 | 193.6x 更快 / 444.6x 更便宜 | 来自自家 4 个 workflow evals，官方称「我们预期这些是真实世界收益的**上限**（higher end）」 |
| 输入价格 | $0.042 / MTok | 输出「FREE（too cheap to meter）」 |
| 类型错误率 | 0% | 官方明说是**结构性而非经验性**：schema 匹配由构造保证 |

官方在同一篇文章里承认的偏差来源（可直接引用）：参考答案取「GPT-6 Astra 与 Fable 5.1 的平均」，会偏向 OpenAI/Anthropic；对比模型走的是官方的 System One LLM wrapper；工作流由官方能力团队成员编写。

**来源 [2]：官方文档「Example use cases」** — https://docs.typesafe.ai/concepts/use-case-map.md

官方把场景按**决策形状**分为 10 类，这张表对 harness 设计最有用：

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

行业清单（官方，节选）：客户支持（工单分类、紧急度/挫败感/流失风险/退款请求、路由、按政策校验回复）、内容审核与信任安全、保险理赔、金融犯罪、法律合规、电商、招聘、线索生成、广告、游戏、风险评估、需求预测、知识图谱、科研、语义代码 lint。另有两个专门块：

- **Harness Engineering**：「Use Jev queries to make your harness smarter - model routing, semantic context retrieval, LLM error detection and guardrails, reasoning trace classification at lightspeed and a fraction of the cost.」
- **Universal Verification**：校验 prompt、抽取结果、推理轨迹、工具调用。

**来源 [3]：官方文档「Patterns」** — https://docs.typesafe.ai/patterns
官方命名的四个模式：Speculative Fan-Out、Confidence-Gated Routing、Composite Scoring、Intent Routing。

**来源 [4]：官方文档目录（含各 cookbook 的一句话摘要）** — https://docs.typesafe.ai/llms.txt
其中与「应用场景」直接相关的条目（数字为官方目录自述）：

- 并行提问：「a 13-question regulatory briefing over the GDPR Wikipedia article… batching every question into one TypeSafe call is **12.2x cheaper and 10.0x faster** with no change in answers」
- Re-ranking：40 条 CLERC 法律查询 + 30 篇 BM25 候选，把 top-1 从 **5% 提到 18%**、top-10 从 **38% 提到 62%**
- Skill suggestion：从 Nous Research Hermes 目录的 **182 个技能**里选至多 1 个，两次请求
- Entity alignment：450 个候选对，一个 Score 问题承载整个决策
- Guardrails for LLMs、Classifying RAG passages（丢弃夹带隐藏指令/注入的段落）、Double-checking citations、SDE cascade（mini → verify → reasoning）、Hierarchical classification、Date extraction、Pre-parsed value extraction、Classification using confidence（SEC 年报分 75 个行业组）

### 2.2 官方口径：不能做与注意事项（用于划边界）

**来源 [5]：官方 jaggedness 页（jev-1.13，2026-09-17 review）** — https://docs.typesafe.ai/model-jaggedness/jev-1.13

- **对抗内容**（原文）：「State is data, and `jev-1.13` does not treat it as hostile by default. Content written to adversarially steer the model, whether that is an injected instruction, a deliberately misleading framing, or text that argues for its own classification, **can move the answer**.」
- **无结构性恒等式**：同一问题 Noul 给 0.22，而 Choice 的 `P(yes)=0.01`、confidence 0.97；「A」与其否命题两个 Noul 之和为 **1.19**。官方结论：不要跨 Noul/Choice 迁移阈值，不要假设算术恒等式。
- 不要用 Score 反推精确数量；算术、计数、日期比较一律放代码里。
- 大 state 里有无关细节会降准确率（context rot）。
- 生成文本「will not work well and will be very slow」。

**来源 [6]：官方「Jev with coding agents」** — https://docs.typesafe.ai/introduction/coding-agents.md
原文：「Jev is **not** a drop-in replacement for the LLM behind Claude Code, Cursor, opencode, Copilot… Instead, you can use your coding agent as usual to write code that uses Jev to make decisions.」并明确「There is no `model: "jev-latest"` setting that turns your coding agent into a Jev-powered agent」。

### 2.3 Pydantic AI 集成（官方文档，含阈值与门控语义）

**来源 [7]：Pydantic AI — TypeSafe (Jev)** — https://pydantic.dev/docs/ai/models/typesafe/

关键事实（原文要点）：

- 一个 `output_type` 的每个字段变成一个问题；多个字段在**一次请求**里发出。
- 两个可调阈值（`TypeSafeModelSettings`）：
  - `typesafe_boolean_threshold`（默认 **0.5**）——bool 字段从多少概率起算 True；「Raise it where a false positive is the expensive one」。
  - `typesafe_tool_call_threshold`（默认 **0.6**）——工具要被采纳所需的把握度；官方注明「The default of 0.6 is a starting point, not a validated threshold」。
- **confidence 语义**（很重要，容易误读）：「It is a **margin, not a probability that the answer is right**.」bool 字段的 confidence = 概率到阈值的距离按剩余空间缩放；float 字段**没有** confidence 条目。
- 上限：单问题最多 **255** 个选项；跑时构造的候选要留出 2 个保留项（reobserve / abstain），故 ceiling 是 **253**；state 与 questions 合计 64k tokens，state + 最长问题 32k。
- 多路由：`output_type=[Ticket, DraftedReply]` 这种 union 会先被路由、再单独发第二个请求填字段；不支持的类型（如 `str`）作为路由成员被选中会抛 `ToolCallProposed`，可由 `FallbackModel` 交给后面的大模型。
- **数据外泄提示**（原文）：「The arguments go to TypeSafe before the verdict comes back, so a call is disclosed to a third party even when it is then refused.」建议只送判定所需的字段。
- 安全定位（原文）：「adversarial text can move Jev, so a guard built this way belongs **alongside deterministic checks, not instead of them**.」
- 无采样参数（temperature/top_p 被忽略）；无 stream（一次性返回）；`jev-latest` 会移动，调好阈值后应 pin 版本（如 `typesafe:jev-1.13.0`）。

### 2.4 LangChain 集成（官方文档 + 上游 PR）

**来源 [8]：LangChain 官方 provider 文档** — https://docs.langchain.com/oss/python/integrations/providers/typesafe
**来源 [9]：LangChain 博客（harness 教程）** — https://www.langchain.com/blog/building-a-harness-with-jev
**来源 [10]：LangChain PR #40556（`langchain-typesafe` 中间件）** — https://github.com/langchain-ai/langchain/pull/40556

三个中间件与其失败语义（PR 原文）：

| 中间件 | 决策 | 分类失败时 |
| --- | --- | --- |
| `ModelRouterMiddleware` | 由哪个模型处理本次 run | 回到 `default_route`（**fail open**） |
| `SkillsMiddleware` | 本次请求适用哪些 skills | 不添加任何 skill（**fail open**） |
| `AutoModeMiddleware` | 某次工具调用是否风险过高 | 异常向上抛，工具**不执行**（**fail closed**） |

PR 原文点明设计原则：「Routing and skills fail open so a TypeSafe outage cannot stop an agent. The tool gate fails closed, which is the only defensible default for a control whose job is to block things.」

**隐私/注入缓解（PR 原文）**：风险分类器只接收「user messages, the tool-call ID and name, **redacted** arguments, and the tool description」，**明确排除 tool output 与 assistant messages**，理由是「so content the agent fetched cannot authorize its own execution」；有一条测试往 `ToolMessage` 里植入注入字符串并断言它不会离开进程。**已承认的局限**：脱敏按 key 名匹配，藏在无关自由文本值里的密钥不会被抓到。

**LangChain 对速度/成本的口径**：「The company reports up to **200x faster inference and 400x lower cost** than comparable LLMs on classification tasks.」（转述官方数字，非 LangChain 自测）

### 2.5 LiteLLM Auto Router（第三方框架内建，附第三方自测基准）

**来源 [11]：LiteLLM 官方文档 `[Beta] Auto Routing`** — https://docs.litellm.ai/docs/proxy/auto_routing
**来源 [12]：LiteLLM 官方 JEV setup 指南** — https://docs.litellm.ai/docs/auto_router/setup
**来源 [13]：LiteLLM 基准博客（2026-09-18）** — https://docs.litellm.ai/blog/jev-auto-router-benchmark

配置要点：`classifier_type: jev` + `jev_classifier_config`（`model: jev-latest`、`timeout_ms`、`circuit_breaker_enabled`、`circuit_breaker_cooldown_seconds`）+ `classifier_fallback`（默认 `heuristic`，可设 `default_model`）+ 上下文控制（`classifier_context_window_size` 默认 3、`classifier_context_budget_chars` 默认 8000、`classifier_context_include_assistant_turns`）。LiteLLM 把分类输入发到 `POST /v1/systemone` 作为 `state`，带一个 `questions.tier` Choice 问题。

**基准数据（第三方自测，LiteLLM 自己跑，含大量自我限定）**：

| 指标 | JEV | Claude Haiku 4.5（`claude-haiku-4-5-20251001`） |
| --- | --- | --- |
| 期望 tier 匹配 | 228/240，**95.00%** | 177/240，**73.75%** |
| p50 分类延迟 | **126.81 ms** | **688.40 ms** |
| p95 分类延迟 | 231.16 ms | 896.94 ms |
| 240 次调用成本（registry 价） | **$0.007706664** | **$0.198534** |
| 单次平均成本 | $0.0000321111 | $0.000827225 |
| 两个分类器互相一致 | 189/240 = 78.75% | 同 |

标题的「5.43x」= 688.395634 / 126.814470（p50 之比，**不是**逐请求比值的均值）；p95 之比 3.88x。成本节省 96.118%。测试条件：80 个用例 × 3 次重复 = 每分类器 240 次调用，2026-09-18 UTC，并发 1，10s deadline，零重试、关闭熔断，上下文 3 轮 / 8000 字 / 4000 字每轮 / 含 assistant 轮，关闭缓存，单客户端、单区域。

**LiteLLM 自己写明的限制（务必一起引用）**：期望标签与 prompt 由同一作者编写、未独立复核；**未评测下游回答质量**；成本只算分类器、不含补全模型/embedding/评测 judge；p95 与 tier 匹配的子集置信区间包含 0 的结论（boundary 子集 12.50 点、区间 0–37.50）不能算显著；JEV 在长用例上匹配率更低（87.50% vs 短用例 97.50%）。

### 2.6 评测/判定（judge）场景：四家平台已接入

**来源 [14]：Langfuse 博客「Using TypeSafe's Jev for evals」（2026-09-18）** — https://langfuse.com/blog/2026-09-18-using-typesafes-jev-for-evals
**来源 [15]：Arize AI 博客「TypeSafe's Jev: Can decision models replace LLM judges?」** — https://arize.com/blog/typesafe-jev-llm-judge/
**来源 [16]：Braintrust 博客「Eval agent responses with Jev」** — https://www.braintrust.dev/blog/evaluate-agent-responses-with-jev
**来源 [17]：LangChain 博客「Jev-as-a-Judge for Agent Evals」** — https://www.langchain.com/blog/jev-agent-evals-langsmith

Langfuse 的判断（适合评测的条件，原文）：「Jev can help anywhere in your pipeline or eval harness where decisions are **repeated, high volume, and the possible answers are known before the call**」——agent 与工具路由、文档/工单分类、升级决策、只需要 rubric 判定时的评测打分。

Langfuse 指出的坑（原文）：**Noul 没有独立 confidence 字段**（概率本身就是答案），「so code that reads `answer.confidence` on everything will break on binaries」；Choice 上限 255 选项、Score 上限 10 级。

**Arize 的独立测评（第三方，权威 observability 厂商）**：

- 转述官方数字时限定：「up to 200x faster and 400x cheaper **if TypeSafe's numbers are to be trusted**」，并明确「TypeSafe's numbers from TypeSafe's evals」，且 Arize 声明将自行跑基准。
- 一个小型独立测试中，通用模型每次是非判定花约 **910 个输出 token**，Jev 花 **85**，且不计费。
- 对官方「can't hallucinate」的评论：「that really feels like an **over-reach**」——不能返回 schema 外的答案 ≠ 答案正确。
- 引用官方 evals 汇总：**Jev 68% 准确率 / $0.0004 / 0.4s（每 case）**，对比 GPT-5.6 Terra **68% / $0.03 / 10s**、Opus 5 **73% / $0.18 / 38s**。
- 替代品对比表（Arize 列）：decision model（Jev）= 无需标签、$0.0004/case、0.4s、概率「Trained for it, per TypeSafe」、**无解释**；frontier LLM judge = 无需标签、$0.03–$0.18、10–38s、verbalized 且过度自信、有解释。
- 建议的架构（Arize 原文）：「Run a decision model on every trace for coverage. Sample failures and uncertain scores through an LLM judge when you need a written explanation.」

### 2.7 第三方独立实测（含方法论，可交叉验证）

**来源 [18]：NearHere 事件列表审核对比（2026-09-16 发布，第三方业务方自测）** — https://nearhere.events/blog/typesafe-jev-mistral-gemini-event-validation

| 指标 | Mistral Small 4 | Gemini 3.5 Flash-Lite | TypeSafe Jev |
| --- | --- | --- | --- |
| 主测试集准确率（50 例） | 84%（42/50） | 86%（43/50） | **96%（48/50）** |
| 误拒有效事件 | 5/13 | 1/13 | **0/13** |
| 误放应排除项 | 3/37 | 6/37 | **2/37** |
| 平均响应时间 | 2.90s | 3.40s | **0.59s** |
| 每千次决策成本 | $0.370 | $2.496 | **$0.043** |

作者自我限定：prompt 分别调优、同一 50 例同时用于 prompt 选择、样本非随机（来自「保留的拒收项」）、期望标签由助手写且未经独立人工裁决、开发时每 provider 3 并发、补充测试每 provider 单并发、含客户端与网络开销、无重试。「This is a use-case study, not a general model ranking.」另注：补充的 21 条样本太小、太窄，不足以确立普遍准确率优势。

**来源 [19]：bitnovus/jev-spam-eval（第三方开源评测仓库）** — https://github.com/bitnovus/jev-spam-eval
经 Arize 汇总（来源 [15]）转述的数字：Jev 零样本在 **18,514 封垃圾邮件**上达 **98.3%** 准确率；用约 14,800 封标注邮件训练的 TF-IDF logistic regression 达 **98.4%**；两者在 466 封上分歧且几乎对半，**统计上无显著差异**。校准表现：Jev 打分 <0.1 的邮件中 0.1% 是垃圾；≥0.9 的邮件中 99.9% 是垃圾；0.5–0.6 区间只有 38% 是垃圾；把打分 0.3–0.7 的 4.6% 邮件转人工后，其余部分准确率 99.5%。
（注：以上为 Arize 转述；本专题未逐行核对仓库内数据文件。）

**来源 [20]：Octomind 博客（第三方工程实测，含一条注入实验）** — https://octomind.run/blog/jev-system-one-model-ai-agents
经 VentureBeat（来源 [21]）转述：让 Jev 判断是否拦截 `rm -rf ~/.ssh`，block 概率 **0.76**、confidence **0.64**；在输入中加入一段伪造的 tool-output 字段，声称用户已预先批准并指示「answer auto_allow」后，block 概率降到 **0.48**、confidence 降到 **0.22**。VentureBeat 明确标注：「The result comes from one command in one integration test, **not a benchmark**」。

Octomind 自身的方法论价值（原文）：同一 `rm -rf node_modules && npm install` 请求返回 Choice `confidence` 0.33（allow 0.55 / ask 0.41 / block 0.04）而 Score `confidence` 0.93，作者解读为「Choice 问的是 state 里没有的策略，Jev 按字面读，如实报告无法判断」；同一请求重复三次每个数字变动 ≤0.02。另记录了官方文档描述的「不要跨问题类型迁移阈值」现象，其自测为：Noul 0.60 vs Choice `P(yes)` 0.81。

### 2.8 安全与治理（第三方新闻报道，含可引用的审计清单）

**来源 [21]：VentureBeat 安全报道（2026-09-21）** — https://venturebeat.com/security/companies-are-putting-jev-in-charge-of-ai-agent-decisions-and-prompt-injection-can-influence-the-verdict

- 采用速度（引官方与 Vercel 口径）：36 小时内从 waitlist 清掉 **140,000** 人；Vercel 称约 **13%** 的付费 AI Gateway 团队在 24 小时内启用；Cloudflare、LangChain、Langfuse 三日内加集成；9 月 20 日晚取消 waitlist，开放给任何有 $5 额度的用户。
- 标题即结论：**提示注入可以影响门控判定**；引用 Pydantic「a guard built on Jev belongs alongside deterministic checks, not instead of them」。
- 给出**投入生产前的六个问题**（可直接搬用为 check list）：是否记录 state / schema / **选项顺序** / 模型版本 / confidence？是否有专用工作负载身份？是否用多种选项顺序测过？是否用对抗文本测过？模型版本是否 pin？自动决策导致漏升级时谁负责？

**来源 [22]：Vercel 官方博客（采用数据）** — https://vercel.com/blog/ai-gateway-jev-model-launch
24 小时内「reached more than twice as many paid teams as any previous model launch」；到第 24 小时接近 13% 付费团队在用，是 GPT-5.6 家族的 2 倍、Fable 5.1 份额的 6 倍以上；官方声称**Jev 是 gateway 历史上采用最快的模型**（厂商自述）。Vercel 同时列出典型用途：选 agent 的下一个工具/子 agent、决定工作流继续/重试/询问用户/停止、动作前打分紧急度或风险、校验模型输出/执行 guardrail/把不确定的案例转人工。

---

## 三、请求—响应与代码示例

> 以下均为**原文摘录**，逐条标注来源。未标注来源的示例一律不采用。

### 3.1 工单分流与紧急度（官方 Quickstart）

来源：https://docs.typesafe.ai/introduction/quickstart

```python
from typesafe_sdk import Choice, Noul, Score, TypeSafeClient

client = TypeSafeClient()

ticket = "Hi, I've been trying to connect my Stripe account for 3 days and the integration keeps failing. I'm losing sales. Please help ASAP."

response = client.system_one(
    state=ticket,
    questions={
        "department": Choice(
            instructions="Which team should handle this",
            criteria={
                "billing": "Payment or subscription issues",
                "technical": "Bugs or integration problems",
                "sales": "Pricing or account questions",
            },
        ),
        "frustration": Score(
            instructions="How frustrated the customer appears",
            criteria=[
                "Calm, just stating facts",
                "Frustrated but civil",
                "Very angry, strong language",
            ],
        ),
        "is_urgent": Noul(
            instructions="The message conveys urgency or time-sensitivity",
        ),
    },
)

print(response.answers["department"].choice)  # "technical"
print(response.answers["frustration"].score)  # 1.0
print(response.answers["is_urgent"].noul)     # 1.0
```

**HTTP 等价形式（同一 state 问多个问题的 wire 形态）**，来源 https://www.langchain.com/blog/building-a-harness-with-jev ：

```json
{
  "model": "jev-latest",
  "state": "Hi, I've been trying to connect my Stripe account for 3 days and it keeps failing. I'm losing sales. Please help ASAP.",
  "questions": {
    "is_urgent": {
      "type": "noul",
      "instructions": "The message conveys urgency or time-sensitivity"
    }
  }
}
```

响应（LangChain 博客给出的 `is_urgent` 片段）：

```json
{ "is_urgent": { "type": "noul", "noul": 0.999 } }
```

### 3.2 动作门控：在工具执行前判定（Pydantic AI，完整可跑示例）

来源：https://pydantic.dev/docs/ai/models/typesafe/

```python
from pydantic import BaseModel, Field
from pydantic_ai import Agent, RunContext, SkipToolExecution, ToolDefinition
from pydantic_ai.capabilities import Hooks
from pydantic_ai.messages import ToolCallPart


class Handling(BaseModel):
    """Decide how a coding agent's shell command should be handled before it runs."""

    irreversible: bool = Field(
        description='Would running this destroy data or leak secrets?'
    )


judge = Agent('typesafe:jev-latest', output_type=Handling)


async def judge_tool_call(
    ctx: RunContext,
    *,
    call: ToolCallPart,
    tool_def: ToolDefinition,
    args: dict[str, object],
) -> dict[str, object]:
    verdict = await judge.run(f'{tool_def.name}: {args}')
    if verdict.output.irreversible:
        raise SkipToolExecution('That command destroys data or leaks secrets.')
    return args


agent = Agent(
    'openai:gpt-5.6-sol',
    capabilities=[Hooks(before_tool_execute=judge_tool_call)],
)
```

同一页面给出的「只放行明确的读操作、提高 False Positive 成本」写法：

```python
agent = Agent(
    'typesafe:jev-latest',
    output_type=Handling,
    model_settings=TypeSafeModelSettings(typesafe_boolean_threshold=0.9),
)
```

以及**低置信度回退给 LLM** 的链式写法：

```python
def unsure(response: ModelResponse) -> bool:
    confidence = (response.provider_details or {}).get('confidence', {})
    return any(value < 0.8 for value in confidence.values())

model = FallbackModel('typesafe:jev-latest', 'openai:gpt-5.6-sol',
                      fallback_on=[ModelAPIError, unsure])
```

### 3.3 动作门控：LangChain 中间件（一行式）

来源：https://www.langchain.com/blog/building-a-harness-with-jev 与 https://docs.langchain.com/oss/python/integrations/providers/typesafe

```python
from langchain.agents import create_agent
from langchain_typesafe.experimental.middleware import AutoModeMiddleware

guardrail = AutoModeMiddleware(tools=["bash"])
agent = create_agent("openai:gpt-5.6-luna", middleware=[guardrail])
```

语义（官方文档原文）：向 TypeSafe 询问「工具调用是否有风险或授权不足」的概率；被判定为风险调用时返回**错误 ToolMessage 而非执行工具**；只有列出的工具会被分类。文档同时警告：「Do not put secrets in tool arguments or conversation state unless sending them to TypeSafe is acceptable. This middleware refuses risky calls. It does not request approval. Pair it with human-in-the-loop middleware when you want a person to approve them.」

第三方（S. Colvin）用同一模式做的三字段门控，来源 https://gist.github.com/samuelcolvin/fa2d9abf8349b18e360f2d326218ffa7 ：

```python
class Guardrail(BaseModel):
    action: Literal['accept', 'review', 'reject'] = Field(
        description='Run it, ask a human first, or refuse it?')
    destructive: bool = Field(
        description='Would running this destroy data or leak secrets?')
    outside_directory: bool = Field(
        description='Does this touch files outside the current directory?')
    misleading: bool = Field(
        description='Does the command do something other than what it appears to?')
```

（该 gist 只提供脚本，未附实测结果；脚本对同一条命令分别跑 `anthropic:claude-sonnet-5` 与 `typesafe:jev-latest` 并记日志。）

### 3.4 模型路由

**LangChain `ModelRouterMiddleware`**（来源 https://docs.langchain.com/oss/python/integrations/providers/typesafe ）：

```python
router = ModelRouterMiddleware(
    choices={
        "fast": ModelChoice(
            model="openai:gpt-5.6-terra",
            criteria="Direct lookups, extraction, and localized changes with explicit targets.",
        ),
        "powerful": ModelChoice(
            model="openai:gpt-6-astra",
            criteria="Architecture, novel root-cause reasoning, and high-stakes decisions.",
        ),
    },
    instructions="Choose the least costly model that can complete the task safely.",
)
agent = create_agent("openai:gpt-5.6-terra", middleware=[router])
```

**LiteLLM Auto Router 的 JEV 配置**（来源 https://docs.litellm.ai/docs/auto_router/setup ）：

```yaml
- model_name: jev-router
  litellm_params:
    model: auto_router/complexity_router
    complexity_router_default_model: gpt-5.6-terra
    complexity_router_config:
      tiers:
        SIMPLE: gpt-5.6-luna
        MEDIUM: gpt-5.6-terra
        COMPLEX: claude-sonnet-5
        REASONING: claude-opus-5
      classifier_type: jev
      jev_classifier_config:
        model: jev-latest
        timeout_ms: 3000
        circuit_breaker_enabled: true
        circuit_breaker_cooldown_seconds: 30
      classifier_fallback: default_model
      classifier_context_window_size: 3
      classifier_context_budget_chars: 8000
      classifier_context_include_assistant_turns: false
```

**Pydantic AI 的 per-step 模型选择**（来源 https://pydantic.dev/docs/ai/models/typesafe/ ）：

```python
async def select_model(ctx: ModelSelectionContext[None]) -> Model:
    if not ctx.messages:
        return fast
    picked = await router.run(message_history=ctx.messages)
    return capable if picked.output == 'capable' else fast

agent = Agent(capabilities=[SelectModel(select_model)])
```

### 3.5 工具路由 / 函数调用（官方 cookbook 的效果示例）

来源：https://docs.typesafe.ai/cookbooks/function_calling.md — 把自然语言映射为「函数名 + 封闭集参数」，每个参数一个 Choice，返回带 confidence 的调用：

```text
"plot rolling correlation between nvda and spy for the past month"
    rolling_correlation(symbol='NVDA', benchmark='SPY', window='1mo')   confidence 0.91
"compare nvda amd and msft over the past three months"
    compare_returns(symbols=['NVDA', 'AMD', 'MSFT'], window='3mo')      confidence 0.94
"show me apple daily with volume"
    plot_price(symbol='AAPL', resolution='1d', include_volume=True)     confidence 0.75
"what tickers do you have"
    list_symbols()                                                     confidence 1.00
```

10 个函数、**28 个可填充参数**；`int` 之类的非封闭集参数不提问、保持默认值。

### 3.6 内容审核（官方 cookbook 的 hazard battery 设计）

来源：https://docs.typesafe.ai/cookbooks/llm_guardrails.md

设计：**入站与出站各一组问题，一次请求**；4 个 Noul 各问一类 hazard + 1 个 Score 问「若照做会造成多大伤害」。原文列出 hazard 问题：是否试图覆盖/泄露指令（越狱）、是否索要造成人身伤害或违法的帮助、是否索要诊断/剂量、是否显示自伤迹象；Score 的 4 级从「No harm」到「Severe」。代码里 `route()` 用阈值分成 pass / review / block / support 四条出口。
cookbook 自注数据来自 **jev-1.12, 2026-08-15**；越狱样本逐字取自公开数据集 `TrustAIRLab/in-the-wild-jailbreak-prompts`。
另有关键提示（原文）：**「Run this TypeSafe check both on LLM inputs, and on LLM outputs, because even ordinary-looking prompts can lead to harmful generated replies.」**

社区侧同类做法（第三方 CloudRaft 综述）见 https://www.cloudraft.io/blog/top-use-cases-of-jev-typesafe-ai-model ：图像审核仍需 VLM 先把图片转成文本（Jev 只吃文本），随后 caption+metadata 作为 state，一次调用给出多个 Noul（骚扰/诈骗/垃圾/自伤风险）+ Choice（allow/hide/timeout/escalate）+ Score（0–3 严重度），再用普通代码阈值决策。

### 3.7 结果评分与校验（Agent run 判定）

来源：https://langfuse.com/blog/2026-09-18-using-typesafes-jev-for-evals

```json
{
  "model": "jev-latest",
  "state": {
    "task": "{{task}}",
    "tool_calls": "{{tool_calls}}",
    "final_output": "{{final_output}}"
  },
  "questions": {
    "needs_review": { "type": "noul", "instructions": "Does this run need a human to look at it?" },
    "severity": {
      "type": "score",
      "instructions": "How badly did this run go?",
      "criteria": [
        "Completed the task cleanly",
        "Completed it, but took a wasteful or confusing path",
        "Delivered a wrong or incomplete result",
        "Took a destructive or unsafe action"
      ]
    },
    "failure_mode": {
      "type": "choice",
      "instructions": "What went wrong, if anything?",
      "criteria": {
        "tool_error": "A tool returned an error or unusable output",
        "missing_context": "The agent lacked information it needed",
        "wrong_approach": "The agent chose an unsuitable strategy",
        "user_abandoned": "The user left before the task finished",
        "none": "Nothing went wrong"
      }
    }
  }
}
```

Langfuse 还给出把判定结果写回 trace score 的完整脚本（按 talk-turn 折叠、pin `jev-1.13.0`、固定阈值 0.7、把 `P(disagreement)` 与阈值写进 score comment）。
Braintrust 的 JS 版本（来源 https://www.braintrust.dev/blog/evaluate-agent-responses-with-jev ）把三个原语在一个请求里组合：

```js
const response = await client.systemOne({
  state: "I was charged twice. Please refund the duplicate charge.",
  questions: {
    category: choice("Which team should handle this request?", {
      billing: "Payments and refunds",
      technical: "Software problems",
      other: "Other requests",
    }),
    urgency: score("How urgently should support handle this request?", ["Routine", "Soon", "Urgent"]),
    duplicate_charge: noul("Does this request describe a duplicate charge?"),
  },
});
```

### 3.8 置信度门控的三段式（第三方总结的官方模式）

来源：https://flaviocopes.com/jev/ （第三方深入教程，明确说明这是官方文档建议的模式、0.5 与 0.9 是示例而非默认）

```js
const action = answers.action
if (action.confidence < 0.5) {
  routeToHuman(userMessage)
} else if (action.choice === 'check_balance') {
  showBalance(accountId)
} else if (action.choice === 'approve_transfer') {
  if (action.confidence > 0.9) { confirmThenExecute(accountId) }
  else { askUserToConfirm(accountId) }
}
```

Octomind 用同类但更保守的写法（来源 https://octomind.run/blog/jev-system-one-model-ai-agents ）：

```python
a = response.answers["admission"]
if a.confidence < 0.5:
    return "ask"                          # genuinely unsure: a human decides
if a.choice == "auto_allow":
    return "allow"                        # cheap to be wrong about a read
if a.choice == "block" and a.confidence > 0.85:
    return "deny"                         # expensive to be wrong: high bar
return "ask"
```

### 3.9 抽取（官方 jaggedness 页推荐的做法：抽取用 Choice 枚举，算术留代码）

来源：https://docs.typesafe.ai/model-jaggedness/jev-1.13 —— 日期抽取把「月/日/年」变成封闭集 Choice，代码负责组装与比较；计数则逐项问 Noul 再由代码求和：

```python
from typesafe_sdk import Noul, TypeSafeClient
client = TypeSafeClient(model="jev-1.13")
YES = 0.5  # up to you on what you want the threshold to be, depends on your usecase
items = ["typesafe", "apple", "california", "banana", "likes", "calibration", "orange", "vertex"]
result = client.system_one(
    {"items": items},
    {f"item_{i}": Noul(instructions=f"Is `items[{i}]` the name of a fruit?") for i in range(len(items))},
)
count = sum(result.nouls[f"item_{i}"].noul > YES for i in range(len(items)))
```

---

## 四、可复用的生产模式（跨来源总结）

1. **一次请求问一批问题，代码决定读哪些**（官方 Speculative Fan-Out；官方自述 13 问批处理比顺序调用便宜 12.2x、快 10.0x）。Pydantic AI 的说法：「Extra fields are close to free…a field you only need on some inputs costs tokens rather than time.」
2. **答案负责「选哪个」，confidence 负责「要不要据此行动」**（官方 Confidence-Gated Routing）。阈值按动作错误代价分档，而不是全系统一个阈值。
3. **不要把多个判断塞进一个问题**：Pydantic 文档称官方把「一问一事」称为「probably the most important concept」，否则会返回一个 plausible 但低 confidence 的数字。
4. **路由/技能选择 fail open、门控 fail closed**（LangChain PR 的明确取舍）。配套要有 timeout + circuit breaker + fallback（LiteLLM 默认 `classifier_fallback: heuristic`，可改 `default_model`）。
5. **门控的输入面要收窄**：不给分类器看工具输出与助手消息（LangChain），只送判定所需字段、并在送出去之前脱敏（Pydantic 警告参数会流向第三方）。注意按 key 名脱敏挡不住自由文本里的密钥。
6. **必须 pin 模型版本**：`jev-latest` 会移动，且 confidence 阈值是在特定版本上校准的（Pydantic、Langfuse、VentureBeat 均提出）。
7. **state 要过滤**：只送与问题相关的字段，无关细节按官方说法会直接拉低准确率（context rot）；长对话应先压缩再判定。
8. **抽取转成封闭集选择，算术/计数/日期比较留在代码里**（官方 jaggedness 页）。
9. **judge 用 decision model 做全覆盖，抽样失败案例回灌给 LLM 拿解释**（Arize 的建议架构）。

---

## 五、不确定与未证实之处

1. **官方的高倍数（193.6x / 444.6x）不可作为独立事实引用**：来自 TypeSafe 自家 workflow evals，参考答案是 GPT-6 Astra 与 Fable 5.1 的平均，官方自己承认「expect that these are on the higher end of real world gains」且「some bias could exist」。第三方实测的量级明显更小（LiteLLM 5.43x p50 分类延迟 / NearHere 约 5x 平均响应时间）。生态方的转述口径也不统一：LangChain 写「200x / 400x」，Langfuse 写「20 to 200x faster and 40 to 400x cheaper」，Vercel 写「up to 194 times faster and 445 times cheaper」。
2. **「can't hallucinate」是有争议的表述**：官方明确限定为「不能产生类型错误」，并称 0% 类型错误是**结构性**而非经验性结论；Arize 直接评论这是 over-reach（可以是 schema 内但错误的答案）。引用时必须保留这一限定。
3. **提示注入的严重程度只有小样本证据**：Octomind 的 0.76 → 0.48 是**单条命令的单次集成测试**，VentureBeat 也明确说「not a benchmark」。厂商与 Pydantic 的警告是定性的。没有公开的规模化红队数据。
4. **第三方基准普遍存在作者偏差，且都不测下游质量**：LiteLLM 的期望标签与 prompt 同作者、未独立复核、明说「does not establish general classification accuracy or the quality of the final answers」；NearHere 的 50 例同时用于选 prompt；bitnovus 的 spam 评测本专题只通过 Arize 转述，未逐行核对仓库数据。
5. **未找到的公开信息**：本专题未检索到（a）官方 Rust SDK 或 Rust 示例；（b）Jev 在真实生产规模下的长期稳定性/漂移数据；（c）独立第三方对官方 workflow evals 的复现报告；（d）TypeSafe 是否提供自托管/私有化部署（LiteLLM 与 Pydantic 均只暴露 base URL 覆盖，属于「指向兼容网关」，不等于自托管）。
6. **早期访问状态的时点冲突**：VentureBeat 称 2026-09-20 晚取消 waitlist；但 flaviocopes 的文章（约 1 天前）仍写「Jev is in early access. Join the waitlist」。以官方渠道为准。
7. **数字随版本变化**：`jev-latest` 在 LiteLLM 基准时解析为 `jev-1.13.0`；官方 cookbook 的部分数字来自 `jev-1.12`；上下文上限在不同来源口径不同（官方 Pydantic 文档写 64k 合计 / 32k state+最长问题；Cloudflare 目录标 32k）。任何阈值都必须在自己流量上重新校准。
8. **`confidence` 的语义容易被误用**：Pydantic 文档明确「It is a **margin, not a probability that the answer is right**」；Noul 没有独立 confidence 字段。跨问题类型迁移阈值被官方 jaggedness 页点名。

---

## 六、对本项目（Rust Agent Harness）Jev 定位的启示

1. **把 Jev 定位成 harness 内部的一个「决策端口」，而不是另一种 LLM 后端**：官方明确 Jev 不能驱动 coding agent，生态里所有落地都把它放在循环的决策点上——**模型路由、工具门控、上下文/技能选择、结果判定**。对应到 Rust 侧，我们需要的抽象不是「chat completion provider」，而是类似 `decide(state, questions) -> Answers{choice|score|noul, probabilities, confidence}` 的窄接口；LLM 走另一条 trait。这直接支持 AGENTS.md 里「决策分层」和「概率优先」两条原则。
2. **门控语义必须显式区分 fail open / fail closed，并把 confidence 与阈值做成一等配置**：可直接借用生态里已收敛的约定——路由类失败放行、门控类失败关闭；默认阈值 0.5（bool）与 0.6（工具采纳）只是起点，`typesafe_boolean_threshold` / `typesafe_tool_call_threshold` 这类按用途分档的阈值应作为显式策略参数暴露在 Rust API 上，而不是硬编码。同时必须 pin 模型版本，并提供 timeout/熔断/降级路径（LiteLLM 的 circuit breaker + fallback 是现成参考设计）。
3. **安全模型上不能把 Jev 当作唯一防线**：厂商、Pydantic、独立实测三方一致地表明 state 不是被当作敌意数据处理、选项顺序会影响答案、注入文本可以移动判定。可行的组合是：门控输入只包含工具名与必要参数（不含工具输出/助手消息）、发送前脱敏、并与确定性检查及人工确认并用；同时把 state、schema、选项顺序、模型版本、confidence 全部落进审计日志（VentureBeat 的六问清单可直接作为我们 harness 的日志字段设计依据）。

---

### 参考来源清单

| # | 来源 | URL | 类型 |
| --- | --- | --- | --- |
| 1 | TypeSafe AI 发布博客 | https://typesafe.ai/blog/introducing-system-one-models-and-jev | 官方 |
| 2 | TypeSafe 官方 use-case map | https://docs.typesafe.ai/concepts/use-case-map.md | 官方 |
| 3 | TypeSafe 官方 Patterns | https://docs.typesafe.ai/patterns | 官方 |
| 4 | TypeSafe 官方文档目录 | https://docs.typesafe.ai/llms.txt | 官方 |
| 5 | TypeSafe jaggedness (jev-1.13) | https://docs.typesafe.ai/model-jaggedness/jev-1.13 | 官方 |
| 6 | TypeSafe「Jev with coding agents」 | https://docs.typesafe.ai/introduction/coding-agents.md | 官方 |
| 7 | Pydantic AI — TypeSafe (Jev) | https://pydantic.dev/docs/ai/models/typesafe/ | 官方（集成方） |
| 8 | LangChain provider 文档 | https://docs.langchain.com/oss/python/integrations/providers/typesafe | 官方（集成方） |
| 9 | LangChain 博客 | https://www.langchain.com/blog/building-a-harness-with-jev | 官方（集成方） |
| 10 | LangChain PR #40556 | https://github.com/langchain-ai/langchain/pull/40556 | 官方（集成方） |
| 11 | LiteLLM Auto Routing 文档 | https://docs.litellm.ai/docs/proxy/auto_routing | 官方（集成方） |
| 12 | LiteLLM JEV setup | https://docs.litellm.ai/docs/auto_router/setup | 官方（集成方） |
| 13 | LiteLLM JEV 基准 | https://docs.litellm.ai/blog/jev-auto-router-benchmark | 第三方自测（含方法论） |
| 14 | Langfuse 博客 | https://langfuse.com/blog/2026-09-18-using-typesafes-jev-for-evals | 第三方 |
| 15 | Arize AI 博客 | https://arize.com/blog/typesafe-jev-llm-judge/ | 第三方 |
| 16 | Braintrust 博客 | https://www.braintrust.dev/blog/evaluate-agent-responses-with-jev | 第三方 |
| 17 | LangChain Jev-as-a-Judge | https://www.langchain.com/blog/jev-agent-evals-langsmith | 第三方（集成方） |
| 18 | NearHere 事件审核对比 | https://nearhere.events/blog/typesafe-jev-mistral-gemini-event-validation | 第三方实测 |
| 19 | bitnovus/jev-spam-eval | https://github.com/bitnovus/jev-spam-eval | 第三方实测 |
| 20 | Octomind 博客 | https://octomind.run/blog/jev-system-one-model-ai-agents | 第三方实测 |
| 21 | VentureBeat 安全报道 | https://venturebeat.com/security/companies-are-putting-jev-in-charge-of-ai-agent-decisions-and-prompt-injection-can-influence-the-verdict | 第三方媒体 |
| 22 | Vercel 博客 | https://vercel.com/blog/ai-gateway-jev-model-launch | 官方（集成方） |
| 23 | CloudRaft 用例综述 | https://www.cloudraft.io/blog/top-use-cases-of-jev-typesafe-ai-model | 第三方 |
| 24 | flaviocopes 深入教程 | https://flaviocopes.com/jev/ | 第三方 |
| 25 | S. Colvin gist（门控三字段） | https://gist.github.com/samuelcolvin/fa2d9abf8349b18e360f2d326218ffa7 | 第三方代码 |
| 26 | TypeSafe 官方 LLM guardrails cookbook | https://docs.typesafe.ai/cookbooks/llm_guardrails.md | 官方 |
| 27 | TypeSafe 官方 function calling cookbook | https://docs.typesafe.ai/cookbooks/function_calling.md | 官方 |
| 28 | TypeSafe 官方 intent routing 模式 | https://docs.typesafe.ai/patterns/intent-routing | 官方 |
