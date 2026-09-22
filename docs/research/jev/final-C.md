# 报告片段 C：应用场景、Agent Harness 集成模式与提问工程

> 本片段由上游专题 `07-use-cases.md`、`08-prompting-practices.md`、`09-harness-integration.md` 忠实提炼、去重并按主题重组，不引入上游之外的任何事实。
> 标注约定：**【官方】** = TypeSafe 官方文档 / 博客 / 定价页 / 状态页；**【官方·集成方】** = LangChain / Pydantic AI / LiteLLM / Cloudflare / Vercel / Langfuse 等集成方自己的一手文档、源码或博客（其中的性能数字多为该集成方**自测**）；**【第三方】** = 有署名的独立实测、报道或社区讨论；**【未证实】** = 转述、二手、无法核对或口径不明。
> 来源 URL 首次引用时给全，之后只用简称；所有数字均保留其语境与自我限定，冲突数字并列呈现、不取其一。

---

## C1. 应用场景：官方与第三方真实场景、失败代价与风险

- **【官方】** Jev 的生产用法高度收敛为四类「决策原语」——分类 / 分流（Choice）、评分（Score）、是非判定（Noul），以及由其派生的**路由、门控、校验、抽取**；官方把这种用法称作 "AI-Powered Workflows / smart if-statements"：*Structured outputs slot into ordinary software as fuzzy decision rules: classify, route, score, extract, or branch where hand-written logic is too brittle.*（https://typesafe.ai/blog/introducing-system-one-models-and-jev）
- **【官方】** 官方明确 Jev **不能**当 coding agent 的主模型：*Jev is **not** a drop-in replacement for the LLM behind Claude Code, Cursor, opencode, Copilot…*，且 *There is no `model: "jev-latest"` setting that turns your coding agent into a Jev-powered agent*（https://docs.typesafe.ai/introduction/coding-agents.md）。生态里所有落地都把它放在循环的**决策点**上。
- **【官方·集成方】** 主流生态已把它接进 Agent 循环的三个位置：模型路由（LangChain `ModelRouterMiddleware`、LiteLLM `classifier_type: jev`、Pydantic AI `SelectModel`）、工具门控（LangChain `AutoModeMiddleware`、Pydantic AI `Hooks(before_tool_execute=...)`）、运行后的评测 / 判定（LangSmith、Langfuse、Braintrust、Arize）。
- **【第三方】** 真实第三方实测的量级明显小于官方宣传：官方自测 193.6x / 444.6x，而 LiteLLM 自测为 p50 分类延迟 5.43x、分类成本节省 96.1%，NearHere 自测为平均响应 0.59s vs 2.90–3.40s、每千次决策 $0.043 vs $0.370 / $2.496。
- **【第三方】** 第三方与官方一致的最大生产风险是**提示注入**：state 不被当作敌意数据，注入文本可以移动判定（Octomind 单次实验：`rm -rf ~/.ssh` 的 block 概率从 0.76 掉到 0.48），且该实验**不是 benchmark**。

### C1.1 官方场景地图：按「决策形状」而非按行业分类

**【官方】** 官方「Example use cases」页把场景按**决策形状**列表（文档自述为 10 类，表内实际列出 8 行，此处以表内条目为准）（https://docs.typesafe.ai/concepts/use-case-map.md）：

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

- 同页两个专门块与本项目直接相关：**Harness Engineering**——*Use Jev queries to make your harness smarter - model routing, semantic context retrieval, LLM error detection and guardrails, reasoning trace classification at lightspeed and a fraction of the cost.*；**Universal Verification**——校验 prompt、抽取结果、推理轨迹、工具调用。
- 行业清单（官方，节选）：客户支持（工单分类、紧急度/挫败感/流失风险/退款请求、路由、按政策校验回复）、内容审核与信任安全、保险理赔、金融犯罪、法律合规、电商、招聘、线索生成、广告、游戏、风险评估、需求预测、知识图谱、科研、语义代码 lint。
- **【官方】** 官方命名的四个模式：**Speculative Fan-Out、Confidence-Gated Routing、Composite Scoring、Intent Routing**（https://docs.typesafe.ai/patterns）。
- **【官方】** 官方发布博客给出的四类用例：AI-Powered Workflows / smart if-statements；Map-reducing over big data；Real-time applications（*100ms speeds means you can use AI in your applications where UX is critical.*）；Verify everything（*Score, judge, verify, guardrail, and detect jailbreaks of LLM prompts, reasoning traces, and/or outputs.*）（https://typesafe.ai/blog/introducing-system-one-models-and-jev）。
- **【官方·集成方】** Vercel 列出的典型用途可视为同一批场景的工程化表述：选 agent 的下一个工具 / 子 agent、决定工作流继续 / 重试 / 询问用户 / 停止、动作前打分紧急度或风险、校验模型输出 / 执行 guardrail / 把不确定的案例转人工（https://vercel.com/blog/ai-gateway-jev-model-launch）。

### C1.2 官方 cookbook 中的量化场景（均为官方自测）

| 场景 | 官方数字 | 出处（官方自测） |
| --- | --- | --- |
| 并行提问（13 问监管简报，GDPR 维基条目） | 批一次 **12.2x 便宜 / 10.0x 快**（$0.000497 / 0.27s vs $0.006090 / 2.71s），且答案无变化 | [Parallel questions](https://docs.typesafe.ai/cookbooks/parallel_questions)、[llms.txt](https://docs.typesafe.ai/llms.txt) |
| 同一实验的**另一口径** | [Primitives 页](https://docs.typesafe.ai/primitives) 写 **11.5x / 9.6x**（口径冲突，见本节不确定项） | [Primitives](https://docs.typesafe.ai/primitives) |
| Re-ranking（CLERC 法律查询，40 query × 30 个 BM25 候选） | top-1 **5% → 18%**、top-5 15% → 35%、top-10 **38% → 62%**；1,200 次调用共 1,536,002 input tokens = $0.0645 | [Re-ranking](https://docs.typesafe.ai/cookbooks/rerank_typesafe) |
| Skill suggestion（Nous Research Hermes 目录 **182 个技能**，选至多 1 个） | 两次 Jev 请求后：错误加载 **16.8% → 7.3%**，无 skill 适用时仍加载 **9.8% → 4.0%**；天花板 2.5% / 1.2% | [Skill suggestion](https://docs.typesafe.ai/cookbooks/skill_suggestion) |
| Entity alignment | 450 个候选对：**40 合并 / 50 转人工 / 360 不关联**，一个三级 Score 承载整个决策 | [Entity alignment](https://docs.typesafe.ai/cookbooks/entity_alignment) |
| 函数调用（工具路由） | 10 个函数、**28 个可填充参数**，示例 confidence 0.91 / 0.94 / 0.75 / 1.00；`int` 之类非封闭集参数不提问 | [Function calling](https://docs.typesafe.ai/cookbooks/function_calling.md) |
| LLM 内容审核（hazard battery） | 入站与出站各一组问题、**一次请求**：4 个 Noul 各问一类 hazard + 1 个 Score 问「若照做会造成多大伤害」（4 级，No harm → Severe），代码用阈值分 pass / review / block / support 四路 | [LLM guardrails](https://docs.typesafe.ai/cookbooks/llm_guardrails.md) |
| RAG 段落过滤 | 每题一个 Noul，四阈值 0.70 / 0.70 / 0.45 / 0.55；注入段落以 **0.99** 被拦下 | [Classifying RAG passages](https://docs.typesafe.ai/cookbooks/classifying_rag_passages) |
| 其他被目录提及的场景 | Double-checking citations、SDE cascade（mini → verify → reasoning）、Hierarchical classification、Date extraction、Pre-parsed value extraction、Classification using confidence（SEC 年报分 75 个行业组） | [llms.txt](https://docs.typesafe.ai/llms.txt) |

- **【官方】** Skill suggestion cookbook 记录的副作用值得在 harness 设计中引用：*A confident wrong suggestion is more persuasive than no suggestion at all*；同页指出 **Choice 与 Noul 回答的是不同问题**（Choice 是相对的「选哪个」，Noul 是绝对的「要不要说」），两者可以不一致，不能用前者推后者。
- **【官方】** 内容审核 cookbook 的关键提示（原文）：*Run this TypeSafe check both on LLM inputs, and on LLM outputs, because even ordinary-looking prompts can lead to harmful generated replies.* 该 cookbook 自注数据来自 `jev-1.12`（2026-08-15），越狱样本逐字取自公开数据集 `TrustAIRLab/in-the-wild-jailbreak-prompts`。
- **【第三方】** CloudRaft 的用例综述指出一条模态边界：图像审核仍需先用 VLM 把图片转成文本（Jev 只吃文本），随后 caption + metadata 作为 state，一次调用给出多个 Noul（骚扰 / 诈骗 / 垃圾 / 自伤风险）+ Choice（allow / hide / timeout / escalate）+ Score（0–3 严重度），再用普通代码阈值决策（https://www.cloudraft.io/blog/top-use-cases-of-jev-typesafe-ai-model）。

### C1.3 官方性能口径（官方自测，官方自己承认有偏）

**【官方】** 来源为官方发布博客（https://typesafe.ai/blog/introducing-system-one-models-and-jev）：

| 指标 | 官方数字 | 语境与限定 |
| --- | --- | --- |
| 端到端响应 | 70ms–500ms | 对比「frontier models 3 到 329 秒」 |
| 相对速度 / 成本 | **193.6x 更快 / 444.6x 更便宜** | 来自自家 4 个 workflow evals；官方称「我们预期这些是真实世界收益的**上限**（higher end）」 |
| 输入价格 | $0.042 / MTok | 输出「FREE（too cheap to meter）」 |
| 类型错误率 | 0% | 官方明说是**结构性而非经验性**：schema 匹配由构造保证 |

- **【官方】** 官方在同一篇文章里列出的偏差来源（可直接引用）：参考答案取「GPT-6 Astra 与 Fable 5.1 的平均」，会偏向 OpenAI / Anthropic；对比模型走官方的 System One LLM wrapper；工作流由官方能力团队成员编写；LLM 侧数字来自 OpenRouter，官方称 *there almost certainly is bias here*；0% schema error 为 *Our number is not empirical*。
- **【未证实】** 生态方转述该倍数的口径不统一：LangChain 写 "up to **200x faster inference and 400x lower cost**"（https://www.langchain.com/blog/building-a-harness-with-jev）；LangChain 另一篇博文写 "up to ~450x cheaper and ~200x faster"（https://www.langchain.com/blog/jev-is-now-available-in-langsmith-evals）；Langfuse 写 "20 to 200x faster and 40 to 400x cheaper"（https://langfuse.com/blog/2026-09-18-using-typesafes-jev-for-evals）；Vercel 写 "up to 194 times faster and 445 times cheaper"（https://vercel.com/blog/ai-gateway-jev-model-launch）。四者均为转述 TypeSafe 自测，非独立测量。

### C1.4 第三方独立实测（含方法论与自我限定）

**【第三方】** LiteLLM Auto Router 基准（2026-09-18，LiteLLM 自跑）——80 个用例 × 3 次重复 = 每分类器 240 次调用，并发 1，10s deadline，零重试、关闭熔断，上下文 3 轮 / 8000 字 / 每轮 4000 字 / 含 assistant 轮，关闭缓存（https://docs.litellm.ai/blog/jev-auto-router-benchmark）：

| 指标 | JEV | Claude Haiku 4.5（`claude-haiku-4-5-20251001`） |
| --- | --- | --- |
| 期望 tier 匹配 | 228/240，**95.00%** | 177/240，**73.75%** |
| p50 分类延迟 | **126.81 ms** | **688.40 ms** |
| p95 分类延迟 | 231.16 ms | 896.94 ms |
| 240 次调用成本（registry 价） | **$0.007706664** | **$0.198534** |
| 两个分类器互相一致 | 189/240 = 78.75% | 同 |

- 标题的「5.43x」= 688.395634 / 126.814470（**p50 之比，不是逐请求比值的均值**）；p95 之比 3.88x；成本节省 96.118%。
- LiteLLM 自己写明的限制：期望标签与 prompt 由同一作者编写、未独立复核；**未评测下游回答质量**（*does not establish general classification accuracy or the quality of the final answers*）；成本只算分类器、不含补全模型 / embedding / 评测 judge；p95 与 tier 匹配的子集置信区间包含 0 的结论（boundary 子集 12.50 点、区间 0–37.50）不能算显著；JEV 在长用例上匹配率更低（87.50% vs 短用例 97.50%）。

**【第三方】** NearHere 事件列表审核对比（2026-09-16，业务方自测，https://nearhere.events/blog/typesafe-jev-mistral-gemini-event-validation）：

| 指标 | Mistral Small 4 | Gemini 3.5 Flash-Lite | TypeSafe Jev |
| --- | --- | --- | --- |
| 主测试集准确率（50 例） | 84%（42/50） | 86%（43/50） | **96%（48/50）** |
| 误拒有效事件 | 5/13 | 1/13 | **0/13** |
| 误放应排除项 | 3/37 | 6/37 | **2/37** |
| 平均响应时间 | 2.90s | 3.40s | **0.59s** |
| 每千次决策成本 | $0.370 | $2.496 | **$0.043** |

- 作者自我限定：prompt 分别调优、同一 50 例同时用于 prompt 选择、样本非随机（来自「保留的拒收项」）、期望标签由助手写且未经独立人工裁决、含客户端与网络开销、无重试，并声明 *This is a use-case study, not a general model ranking.*

**【第三方】** 垃圾邮件评测 `bitnovus/jev-spam-eval`（https://github.com/bitnovus/jev-spam-eval），本次调研仅经 Arize 转述、**未逐行核对仓库内数据文件**：Jev 零样本在 **18,514 封**垃圾邮件上达 **98.3%** 准确率；用约 14,800 封标注邮件训练的 TF-IDF logistic regression 达 **98.4%**；两者在 466 封上分歧且几乎对半，**统计上无显著差异**。校准表现：打分 <0.1 的邮件中 0.1% 是垃圾；≥0.9 的邮件中 99.9% 是垃圾；0.5–0.6 区间只有 38% 是垃圾；把打分 0.3–0.7 的 4.6% 邮件转人工后，其余部分准确率 99.5%（转述口径，见 https://arize.com/blog/typesafe-jev-llm-judge/）。

**【第三方】** Arize 的小型独立测试：通用模型每次是非判定花约 **910 个输出 token**，Jev 花 **85** 且不计费；Arize 引用官方 evals 汇总为 **Jev 68% 准确率 / $0.0004 / 0.4s（每 case）**，对比 GPT-5.6 Terra **68% / $0.03 / 10s**、Opus 5 **73% / $0.18 / 38s**。Arize 对官方「can't hallucinate」的评论是 *that really feels like an over-reach*——不能返回 schema 外的答案 ≠ 答案正确（https://arize.com/blog/typesafe-jev-llm-judge/）。

**【第三方】** LangChain 的 Jev-as-a-Judge 实验（LangChain 自跑，判定场景）：目标 agent 用 Deep Agents 构建，5 条 weather 请求固定捕获为 LangSmith dataset，每条重复 100 次（共 500 次决策），两个信号 `quality`（连续分数）+ `does_pass`（二值），人工标注为 oracle。**Jev 在 500 次决策上全部与 oracle 一致**；GPT-5.6 Terra 99.8%、GPT-5.6 Luna 96.4%、Claude Sonnet 4.6 80.0%。每条 case 方差：Jev 0.0000149，Luna 高 433×、Terra 高 913×、Claude 高 92×。**每次调用 $0.00035**；整组判断 Jev $0.34 vs GPT-5.6 Luna $0.39、Terra $2.90、Claude Sonnet 4.6 **$28.17**；延迟 0.44s vs 2.16–2.83s（https://www.langchain.com/blog/jev-agent-evals-langsmith、https://www.langchain.com/blog/jev-is-now-available-in-langsmith-evals）。复现信息：通过 `langchain-typesafe==0.0.1a2` 访问 Jev；LLM judges 走 LangSmith Gateway；**未设 temperature/top_p/seed/max_tokens**；实验元数据中**没有 Jev 的 service version**。官方自评 *This was one test on one agent*，并警告 *low cost can amplify mistakes — a consistently wrong evaluator can produce bad feedback at scale*。数字另有第三方转述（https://www.explainx.ai/blog/langchain-jev-agent-evals-benchmark-september-2026），属交叉确认而非独立复现。

**【第三方】** Octomind 的上下文筛选实测：把 1,132 行 / 46 个 chunk 的源码一次性作为 state、每 chunk 一个 Noul 判断「这段是否回答了问题」，**一次调用 14,398 input tokens、584ms**，前 6 名中有 3 个不含关键词但语义相关（https://octomind.run/blog/jev-system-one-model-ai-agents）。样本量为 1，属存在性证明。

**【第三方】** Octomind 的自洽性观察：同一 `rm -rf node_modules && npm install` 请求重复三次，每个数字变动 ≤0.02；同一请求下 Choice `confidence` 0.33（allow 0.55 / ask 0.41 / block 0.04）而 Score `confidence` 0.93，作者解读为「Choice 问的是 state 里没有的策略，Jev 按字面读，如实报告无法判断」；该文另记录官方「不要跨问题类型迁移阈值」现象的自测：Noul 0.60 vs Choice `P(yes)` 0.81。

### C1.5 采纳速度与生态

- **【第三方】** VentureBeat（2026-09-21）：36 小时内从 waitlist 清掉 **140,000** 人；Vercel 称约 **13%** 的付费 AI Gateway 团队在 24 小时内启用；Cloudflare、LangChain、Langfuse 三日内加集成；9 月 20 日晚取消 waitlist，开放给任何有 **$5** 额度的用户（https://venturebeat.com/security/companies-are-putting-jev-in-charge-of-ai-agent-decisions-and-prompt-injection-can-influence-the-verdict）。
- **【官方·集成方】** Vercel：24 小时内 *reached more than twice as many paid teams as any previous model launch*；到第 24 小时接近 13% 付费团队在用，是 GPT-5.6 家族的 2 倍、Fable 5.1 份额的 6 倍以上；官方声称 Jev 是 gateway 历史上采用最快的模型（厂商自述）（https://vercel.com/blog/ai-gateway-jev-model-launch）。
- **【第三方】** 早期访问状态存在时点冲突：VentureBeat 称 2026-09-20 晚取消 waitlist，而 flaviocopes 的文章（约 1 天前）仍写 *Jev is in early access. Join the waitlist*（https://flaviocopes.com/jev/）。以官方渠道为准。

### C1.6 失败代价与风险

**风险 1：提示注入可以移动判定（官方、集成方、独立实测三方一致）**
- **【官方】** jaggedness 页原文：*State is data, and `jev-1.13` does not treat it as hostile by default. Content written to adversarially steer the model, whether that is an injected instruction, a deliberately misleading framing, or text that argues for its own classification, **can move the answer**.*（https://docs.typesafe.ai/model-jaggedness/jev-1.13）
- **【官方·集成方】** Pydantic 文档：*adversarial text can move Jev, so a guard built this way belongs **alongside deterministic checks, not instead of them**.*（https://pydantic.dev/docs/ai/models/typesafe/）
- **【第三方】** Octomind 的单次注入实验：让 Jev 判断是否拦截 `rm -rf ~/.ssh`，block 概率 **0.76**、confidence **0.64**；在输入中加入一段伪造的 tool-output，声称用户已预先批准并指示「answer auto_allow」后，block 概率降到 **0.48**、confidence 降到 **0.22**。VentureBeat 明确标注 *The result comes from one command in one integration test, **not a benchmark***；VentureBeat 引用的是同一实验，**不构成第二个独立来源**。
- **【官方】** 官方 RAG cookbook 进一步说明其注入过滤只是**过滤器，不是安全边界**：低于阈值的段落仍会进入 prompt，生成侧必须把每段都当不可信文本处理。

**风险 2：state 里的内容会流向第三方（数据外泄面）**
- **【官方·集成方】** Pydantic：*The arguments go to TypeSafe before the verdict comes back, so a call is disclosed to a third party even when it is then refused.* 建议只送判定所需的字段。
- **【官方·集成方】** LangChain 在 LangSmith Evals 博文中披露：*TypeSafe does not currently offer zero data retention, so prompts and outputs sent for evaluation may be retained by the provider.*
- **【官方·集成方】** LangChain provider 文档警告：*Do not put secrets in tool arguments or conversation state unless sending them to TypeSafe is acceptable. This middleware refuses risky calls. It does not request approval. Pair it with human-in-the-loop middleware when you want a person to approve them.*（https://docs.langchain.com/oss/python/integrations/providers/typesafe）

**风险 3：护栏不能是唯一防线**
- **【官方·集成方】** Pydantic 原话：*a judgement this cheap is one you can afford to run on everything, which is exactly why it should not be the only thing standing between an agent and an irreversible action.* 另一处提醒：Jev 判的是「模型提出的调用」，不是「模型的意图」。
- **【官方】** 官方 jaggedness 页把「对抗性内容」列为独立失败模式，建议是「在 criteria 里写清楚、上线前充分测边界情况」——定性建议，未给数字。

**风险 4：能力边界造成的静默错误**
- **【官方】** 不要用 Score 反推精确数量（数值校准是弱项，只能用来**过阈值**）；算术、计数、日期比较一律放代码里。
- **【官方】** 大 state 里有无关细节会降准确率（context rot）；生成文本 *will not work well and will be very slow*。
- **【官方·集成方】** Jev 不会像 LLM 那样修正答案：*an output validator that raises ModelRetry usually gets the same answer again*，因此「用 Jev 自检并重试」不是可行路径（https://octomind.run/blog/jev-system-one-model-ai-agents、https://pydantic.dev/docs/ai/models/typesafe/）。

**风险 5：治理与审计**
- **【第三方】** VentureBeat 给出投入生产前的六个问题（可直接搬用为 check list）：是否记录 state / schema / **选项顺序** / 模型版本 / confidence？是否有专用工作负载身份（workload identity）？是否用多种选项顺序测过？是否用对抗文本测过？模型版本是否 pin？自动决策导致漏升级时谁负责？该报道还指出「Jev 返回类型化答案而非自然语言理由」与既有审计框架（SOC 2 / ISO 27001 / PCI DSS）的映射 *partial coverage at best*——属第三方安全媒体分析，不是标准组织结论。

**本节不确定项**

1. **官方高倍数（193.6x / 444.6x）不可作为独立事实引用**：来自 TypeSafe 自家 workflow evals，参考答案为 GPT-6 Astra 与 Fable 5.1 的平均，官方自述 *expect that these are on the higher end of real world gains*、*some bias could exist*。第三方量级明显更小（LiteLLM 5.43x p50 分类延迟、NearHere 约 5x 平均响应时间），生态转述口径互不一致（200x/400x、20–200x / 40–400x、194x/445x、450x/200x）。
2. **「can't hallucinate」是有争议的表述**：官方限定为「不能产生类型错误」，并称 0% 类型错误是**结构性**结论；Arize 直接评论这是 over-reach。引用时必须保留限定。
3. **提示注入的严重程度只有小样本证据**：0.76 → 0.48 是单条命令的单次集成测试，无公开的规模化红队数据；官方仅承认方向（can move the answer），未给数字。
4. **第三方基准普遍存在作者偏差，且都不测下游质量**：LiteLLM 的期望标签与 prompt 同作者、未独立复核；NearHere 的 50 例同时用于选 prompt；bitnovus 的 spam 评测本片段只通过 Arize 转述；LangChain 的 judge 实验为单 agent、5 条样本。
5. **未找到公开信息**：Jev 在真实生产规模下的长期稳定性 / 漂移数据；对官方 workflow evals 的独立复现报告；TypeSafe 是否提供自托管 / 私有化部署（LiteLLM 与 Pydantic 只暴露 base URL 覆盖，属「指向兼容网关」，不等于自托管）。官方 Rust SDK 缺失见 C2 不确定项 6。
6. **数字随版本变化**：`jev-latest` 在 LiteLLM 基准时解析为 `jev-1.13.0`；官方 cookbook 部分数字来自 `jev-1.12`；上下文上限口径不同（官方 Pydantic 文档写 64k 合计 / 32k state+最长问题；Cloudflare 目录标 32k）。任何阈值都必须在自己流量上重新校准。

---

## C2. Agent Harness 集成模式

- **【官方·集成方】** 两套主流集成都把 Jev 放进**框架已有的钩子**，不需要新 API：LangChain 用 `before_agent` / `wrap_model_call`（选模型）、`wrap_tool_call`（工具门控）与自定义 hook；Pydantic AI 把每个 decision 写成一个 capability / hook。
- **【第三方】** 本次调研有实证支撑的决策点共 **6 类**：模型路由、工具调用门控、技能/工具集选择、上下文筛选与压缩、评测（judge）、危险动作/注入分类。**「终止判断（是否完成）」未找到官方形态**，只有第三方设计草图且作者自己声明未实现。
- **【官方·集成方】** 失败语义是本专题最有价值的工程结论，两套框架一致：**路由与技能选择 fail open**，**工具门控 fail closed**。LangChain PR 原文：*Routing and skills fail open so a TypeSafe outage cannot stop an agent. The tool gate fails closed, which is the only defensible default for a control whose job is to block things.*
- **【官方·集成方】** 门控的输入形态已被固化为设计规则：state 只放「用户消息 + 被判定动作 + 判定必需的廉价事实」，**绝不放工具输出与助手消息**——*Tool output and assistant messages are excluded so untrusted content cannot authorize its own execution.*
- **【官方·集成方】** 「LLM 决策 + Jev 判定」有 4 种代码形态：① Jev 作为 provider 跑 agent、LLM 只做被路由到的那部分（Pydantic `FallbackModel`）；② Jev 输出函数/工具候选，选中即执行（Pydantic output functions）；③ 框架 middleware 前置分类（LangChain）；④ 置信度低于阈值时交给 LLM 接管。
- **【官方·集成方】** 置信度是**边界余量（margin）**而非「答案正确的概率」，`float` 字段甚至没有 confidence；任何「低置信度回退 LLM」的设计都必须按这个定义写（https://pydantic.dev/docs/ai/models/typesafe/）。

### C2.1 六个决策点

**决策点 1：模型路由 / 分层（Routing）**
- **【官方·集成方】** LangChain `ModelRouterMiddleware` 挂在 `before_agent` 与 `wrap_model_call`：把最新一条 human message 分类一次，选择的模型用于该 run 的**每一次** model call；概率与置信度留在 agent state 里供应用与 trace 使用（https://docs.langchain.com/oss/python/integrations/providers/typesafe、https://www.langchain.com/blog/building-a-harness-with-jev）。
- **【官方·集成方】** 源码：`before_agent` 的分类结果写入 state 的 `model_route`；`wrap_model_call` 用 `request.override(model=...)` 替换；**分类异常被 catch 并回退 `default_route`**（注释：*routing must not break the agent*）（https://github.com/langchain-ai/langchain/pull/40556）。
- **【官方·集成方】** Pydantic AI 提供两种路由粒度：`output_type=route`（把决策写成函数签名，Jev 选 `tier` 后由框架调用该函数，一次请求完成路由+执行）；`SelectModel` capability 在**每一步**前重新分类（*a run that starts simple and turns hard moves up when it turns*）。
- **【官方·集成方】** Pydantic 提醒：`SelectModel` 读的是完整 history，长 run 会让每次路由请求变大变慢，官方建议**与 compaction 配对**——*the compacted history is what the router reads*。
- **【官方·集成方】** LiteLLM 的等价物是 tier 路由 + 分类器熔断：`classifier_type: jev` + `jev_classifier_config`（`model`、`timeout_ms`、`circuit_breaker_enabled`、`circuit_breaker_cooldown_seconds`）+ `classifier_fallback`（默认 `heuristic`，可设 `default_model`）+ 上下文控制（`classifier_context_window_size` 默认 3、`classifier_context_budget_chars` 默认 8000、`classifier_context_include_assistant_turns`）（https://docs.litellm.ai/docs/proxy/auto_routing、https://docs.litellm.ai/docs/auto_router/setup）。

**决策点 2：工具调用门控（Tool-call gating / guardrail）**
- **【官方·集成方】** LangChain `AutoModeMiddleware(tools=[...])` 在 `wrap_tool_call` 前询问「这次调用是否 risky 或未获充分授权」的 `Noul` 概率；**达到或超过阈值就返回 error `ToolMessage`，工具体不执行**。
- **【官方·集成方】** 源码默认 `risk_threshold=0.2`，docstring 原文：*The conservative default blocks calls with at least 20% estimated risk.* 阈值可传 `[0,1]`，越界抛 `ValueError`；**未登记的工具直接放行**（`if request.tool_call["name"] not in self._tool_filter: return handler(request)`）。
- **【官方·集成方】** 送进 Jev 的 state 固定为：`user_messages`（只取 `HumanMessage`）+ `tool_call{id,name,args}` + 可选 `tool_description`；参数按 key 名脱敏（`api_key` / `token` / `secret` / `password` / `private_key` / `credential`）。已承认的局限：*Matching is by key name, so a secret carried inside an unrelated key is not caught.*
- **【官方·集成方】** 失败语义：门控 fail closed（*Classification failures propagate and the tool handler is not called*）；middleware traces 也会 `omit_payload` 以隐藏授权上下文与参数。
- **【官方·集成方】** Pydantic AI 的等价物是 `Hooks(before_tool_execute=...)` + `raise SkipToolExecution(...)`：拦截消息作为工具结果回给模型，「模型能学到什么被拒绝了，可以换一种做法」；不标 `requires_approval`、工具无需 opt-in，**包括之后新增的工具**。需要「审批离开进程」（人工在别的系统批准、排队、稍后 resume）时用 `requires_approval=True` + `HandleDeferredToolCalls`——*Both see validated arguments; only the deferral can outlive the run.*
- **【第三方】** S. Colvin 的 gist 用同一模式做了三字段门控（`action: Literal['accept','review','reject']` + `destructive` + `outside_directory` + `misleading`），脚本对同一条命令分别跑 `anthropic:claude-sonnet-5` 与 `typesafe:jev-latest` 并记日志；**该 gist 只提供脚本，未附实测结果**（https://gist.github.com/samuelcolvin/fa2d9abf8349b18e360f2d326218ffa7）。

**决策点 3：技能 / 工具集选择（Skill or tool selection）**
- **【官方·集成方】** LangChain `SkillsMiddleware`：每个 skill 独立打分，**任意数量**可命中；只有被选中的 skill 指令被前置到 model request，*so the roster never enters the prompt*；技能来源是 Agent Skills 兼容的 `SKILL.md`（路径/对象/字符串），`skills_root` 在符号链接解析后限定在单一目录。分类失败时 *Adds no skills*（fail open）。官方警告：*never load a `SKILL.md` a user can write.*
- **【官方·集成方】** Pydantic AI 用 `PrepareTools` 钩子「在大工具集上线前先问这次请求需要哪些」，另有 history processor 在压缩前筛掉不相关片段。
- **【官方】** 官方 Skill suggestion cookbook 量化：182 个 skill、488 条单轮请求、被测 agent 为 `claude-haiku-4-5-20251001`、发布版本 `jev-1.12`、渲染日期 2026-07-31；加两道 Jev 请求后错误加载 16.8% → 7.3%、无 skill 适用时仍加载 9.8% → 4.0%（天花板 2.5% / 1.2%）（https://docs.typesafe.ai/cookbooks/skill_suggestion）。

**决策点 4：上下文筛选与压缩（Context filtering / condensation）**
- **【官方·集成方】** Pydantic 官方把 history processor 与 compaction capability 列为 Jev 可直接承担的决策点，并给出容量约束：`jev-1.13` 的 state 与 questions **合计 64k tokens，其中 state + 最长问题 32k**；超限以 `ModelHTTPError(max_tokens_exceeded)` 失败；**若 Jev 后面挂了 `FallbackModel`，超长会话会被静默交给 LLM**（成本模型随之改变）。官方建议 *Compact earlier than a language model would need, since Jev is being asked to judge the whole of it, not to continue from it.*
- **【第三方】** Octomind 的 46-chunk / 14,398 input tokens / 584ms 实验（见 C1.4）是这一类用法的存在性证明。

**决策点 5：评测 / Judge（Eval）**
- **【第三方】** Langfuse 的适用条件（原文）：*Jev can help anywhere in your pipeline or eval harness where decisions are **repeated, high volume, and the possible answers are known before the call***——agent 与工具路由、文档/工单分类、升级决策、只需要 rubric 判定时的评测打分（https://langfuse.com/blog/2026-09-18-using-typesafes-jev-for-evals）。
- **【第三方】** Langfuse 指出的两个坑：**Noul 没有独立 confidence 字段**（概率本身就是答案），*so code that reads `answer.confidence` on everything will break on binaries*；Choice 上限 255 选项、Score 上限 10 级。
- **【第三方】** Arize 的建议架构（原文）：*Run a decision model on every trace for coverage. Sample failures and uncertain scores through an LLM judge when you need a written explanation.* 其对比表把 decision model（Jev）记为无需标签、$0.0004/case、0.4s、概率「Trained for it, per TypeSafe」、**无解释**；frontier LLM judge 记为 $0.03–$0.18、10–38s、verbalized 且过度自信、有解释（https://arize.com/blog/typesafe-jev-llm-judge/）。
- **【第三方】** Braintrust 把三个原语在一个请求里组合（JS，https://www.braintrust.dev/blog/evaluate-agent-responses-with-jev）：`category: choice(...)` + `urgency: score(["Routine","Soon","Urgent"])` + `duplicate_charge: noul(...)`。
- **【官方·集成方】** LangSmith Evals 的两条集成规范：**state 里不要放评分说明**（评分标准属于 questions）；每个 question 成为一个 feedback key。

**决策点 6：终止 / 完成判断——未找到官方形态**
- **【未证实】** 未找到 TypeSafe 或 LangChain 官方关于「用 Jev 判断任务是否完成 / 是否终止循环」的模式、API 或数据。唯一相关材料是第三方设计草图：Octomind 把 harness 控制面拆成 7 个判断，其中 *Is the work actually done?* 被作者自己标注为 **System Two — it has to read the diff and the request**，并明确写出该整合地图 *None of this is built*（https://octomind.run/blog/jev-system-one-model-ai-agents）。
- **【官方】** 官方 jaggedness 页把「生成」单列为失败模式 #9（*Use a generative model*），间接说明需要阅读全文才能判断的完成度检查不适合 Jev。

### C2.2 fail open 与 fail closed 的工程语义

**【官方·集成方】** LangChain 三个中间件与其失败语义（PR #40556 原文）：

| 中间件 | 决策 | 分类失败时 |
| --- | --- | --- |
| `ModelRouterMiddleware` | 由哪个模型处理本次 run | 回到 `default_route`（**fail open**） |
| `SkillsMiddleware` | 本次请求适用哪些 skills | 不添加任何 skill（**fail open**） |
| `AutoModeMiddleware` | 某次工具调用是否风险过高 | 异常向上抛，工具**不执行**（**fail closed**） |

- 设计原则原文：*Routing and skills fail open so a TypeSafe outage cannot stop an agent. The tool gate fails closed, which is the only defensible default for a control whose job is to block things.*
- **【官方·集成方】** 失败语义要配套 timeout + circuit breaker + fallback：LiteLLM 默认 `classifier_fallback: heuristic`（可改 `default_model`），并提供 `timeout_ms` 与熔断开关/冷却时间（见决策点 1 的配置项）。
- **【官方·集成方】** 要小心两套版本：PyPI 已发布 `langchain-typesafe 0.0.1a3`（2026-09-20），README 用 `langchain_typesafe.experimental.middleware`、`AutoModeMiddleware(..., criteria=NoulCriteria(...))`；GitHub PR #40556 分支版用 `langchain_typesafe.middleware`、`default_route` 为必填、未见 `criteria` 参数，且该 PR 在 2026-09-22 查证时仍为 **open**（`state: open`, `merged: false`, created 2026-09-17, 37 files, +5712）（https://pypi.org/project/langchain-typesafe/）。**引用任何 API 签名前必须确认版本。**

### C2.3 具体代码形态（四种「LLM 决策 + Jev 判定」）

**形态 ① Jev 作为 provider 跑 agent，选中即执行（Pydantic AI output function）**
```python
async def route(ctx: RunContext[None], tier: Literal['fast', 'capable']) -> str:
    """Answer the question on a model suited to it. ..."""
    model = 'openai:gpt-5.6-sol' if tier == 'capable' else 'openai:gpt-5.6-luna'
    return (await assistant.run(ctx.prompt, model=model)).output
router = Agent('typesafe:jev-latest', output_type=route)
# 一次 router.run(...) 就是全部：Jev 填 tier，框架调用 route，route 内部再调 LLM
```
出处：【官方·集成方】https://pydantic.dev/docs/ai/models/typesafe/

**形态 ② per-step 模型选择（Pydantic AI `SelectModel`）**
```python
async def select_model(ctx: ModelSelectionContext[None]) -> Model:
    if not ctx.messages:
        return fast
    picked = await router.run(message_history=ctx.messages)
    return capable if picked.output == 'capable' else fast
agent = Agent(capabilities=[SelectModel(select_model)])
```
出处同上。

**形态 ③ 框架 middleware 前置分类（LangChain，一行式接入）**
```python
from langchain_typesafe.experimental.middleware import ModelChoice, ModelRouterMiddleware, AutoModeMiddleware
router = ModelRouterMiddleware(
    choices={"fast": ModelChoice(model="openai:gpt-5-mini", criteria="Simple, well-scoped tasks."),
             "powerful": ModelChoice(model=powerful_model, criteria="Complex tasks requiring deeper reasoning.")},
    instructions="Choose the least costly model suited to the task.")
guardrail = AutoModeMiddleware(tools=["bash"])
agent = create_agent("openai:gpt-5-mini", middleware=[router, guardrail])
```
出处：【官方·集成方】https://pypi.org/project/langchain-typesafe/、https://www.langchain.com/blog/building-a-harness-with-jev。LangChain 博客/文档版用的是 `criteria="Direct lookups…"` 这类字符串描述 + `default_route`（https://docs.langchain.com/oss/python/integrations/providers/typesafe）。

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

**形态 ④ 置信度低于阈值时交给 LLM 接管（Pydantic AI cascade）**
```python
def unsure(response: ModelResponse) -> bool:
    confidence = (response.provider_details or {}).get('confidence', {})
    return any(value < 0.8 for value in confidence.values())
model = FallbackModel('typesafe:jev-latest', 'openai:gpt-5.6-sol', fallback_on=[ModelAPIError, unsure])
```

- **【官方·集成方】** 该 `unsure` 只遍历 `provider_details['confidence']` 里的值，因此**不覆盖没有 confidence 的 float 字段**——这是形态 ④ 的实现陷阱。
- **【第三方】** flaviocopes 总结的官方三段式（置信度门控，https://flaviocopes.com/jev/）：`confidence < 0.5` → 转人工；只读动作（`check_balance`）直接放行；动钱动作（`approve_transfer`）`confidence > 0.9` 才确认执行，否则先问用户；该文明确说明 0.5 与 0.9 是**示例而非默认**。
- **【第三方】** Octomind 的更保守写法：`confidence < 0.5` → `ask`；`auto_allow` → `allow`（读操作错了便宜）；`block` 且 `confidence > 0.85` → `deny`（错了贵，门槛高）；其余 → `ask`（https://octomind.run/blog/jev-system-one-model-ai-agents）。
- **【未证实】** Octomind 还给出把 Jev 当作 Claude Code `PreToolUse` hook「更便宜的第一意见」的形态（逐动作风险分档 + 把廉价事实如 git dirty 塞进 state；同一命令、dirty 树与否使 ask 概率从 0.57 升到 0.80、`destroys_work` 从 0.63 升到 0.83），但作者声明该整合地图 *None of this is built*——**未在产品中实现**。
- **【官方】** 官方 quickstart 的工单分流示例（一条 state + Choice/Score/Noul 三问）见 https://docs.typesafe.ai/introduction/quickstart；HTTP wire 形态为 `POST https://api.typesafe.ai/v1/systemone`（`{"model","state","questions"}` → `{"model","answers","usage"}`，见 https://docs.typesafe.ai/models）。

### C2.4 集成面一览（各框架接入方式）

- **【官方·集成方】** **LangChain (Python)**：`TypeSafeClassifier` 是一个 `Runnable`——`invoke`/`ainvoke`，输入 `{state, questions}`；state 可为字符串、JSON 对象/数组或 `BaseMessage` 序列；结果按类型分组在 `choices`/`scores`/`nouls`；trace 与 token 用量进 LangSmith。
- **【官方·集成方】** **LangChain 错误处理与传输层**：provider 错误继承 LangChain 标准 model-error 层级；可捕获 `TypeSafeRateLimitError`（含 `request_id`、`retry_after_ms`）或统一的 `ModelAuthenticationError` / `ModelRateLimitError`；`TypeSafeAPIError` 暴露 status/body/headers/sanitized endpoint/request id；未注入时自建 sync/async `httpx2` client，也可注入自定义 client（代理、连接池），生命周期由调用方负责（https://pypi.org/project/langchain-typesafe/）。
- **【官方·集成方】** **Pydantic AI**：`Agent('typesafe:jev-latest', output_type=Model)`；也可 `TypeSafeModel('jev-latest')` / `TypeSafeProvider(api_key=..., http_client=...)`；`output_type` 每个字段 = 一个问题，一次请求并行问完；`ModelResponse.model_name` 报告实际作答的版本化 id。通用 sampling 参数（temperature/top_p）被忽略；`timeout`/`extra_headers`/`extra_body` 透传；`typesafe_tool_call_threshold` 默认 **0.6**、`typesafe_boolean_threshold` 默认 **0.5**，越界为 `UserError`（https://pydantic.dev/docs/ai/models/typesafe/）。
- **【官方·集成方】** **LiteLLM**：走 **pass-through**，不是 `/chat/completions`——`POST /typesafe/v1/systemone`（`/typesafe/` 下任意路径都转发，`GET /typesafe/v1/models` 列模型）；LiteLLM 侧注入 TypeSafe key，客户端只用虚拟 key；计费按 `usage.input_tokens/output_tokens` 与 registry 中的 `typesafe/<model>`；日志按 TypeSafe 返回的版本化 id 记账（https://docs.litellm.ai/docs/pass_through/typesafe）。
- **【官方·集成方】** **Cloudflare**：`env.AI.run('typesafe/jev', {state, questions})` 或 `POST /accounts/$ID/ai/run`；文档页标注 context window 32,000 tokens（https://developers.cloudflare.com/ai/models/typesafe/jev/）。**LangSmith**：Jev 作为 judge provider 进入 Evals（Evaluators → LLM-as-a-Judge → Provider 选 TypeSafe、Model 选 `jev-latest`）（https://www.langchain.com/blog/jev-is-now-available-in-langsmith-evals）。
- **【官方】** **官方 SDK 只有** Python（`typesafe-sdk`）与 JavaScript / TypeScript（`@typesafe-ai/sdk`）；文档注明 *You can also call the HTTP API directly from any language.*（**无 Rust SDK**）（https://docs.typesafe.ai/sdk.md、https://docs.typesafe.ai/llms.txt）。
- **【未证实】** 其他被提及但本次未核实：Vercel AI Gateway 上架 `jev`；Langfuse 集成；Cloudflare、LangChain、Langfuse 三日内加入集成（https://venturebeat.com/security/companies-are-putting-jev-in-charge-of-ai-agent-decisions-and-prompt-injection-can-influence-the-verdict）。

### C2.5 官方「决策设计规范」（可直接映射到 Rust 侧）

- **【官方】** 一次请求并行多问是默认姿势：Patterns 把 Speculative Fan-Out（把包括只在某些分支才用得上的问题全都一次问出去，由代码决定读哪些）列为首要模式（https://docs.typesafe.ai/patterns）。
- **【官方】** 原子化：不要问「这个 pitch 好吗」，拆成市场规模 / 可行性 / 差异化三个问题，在代码里加权合并——*change a coefficient in your code rather than rewriting a prompt*（https://docs.typesafe.ai/、https://pydantic.dev/docs/ai/models/typesafe/）。
- **【官方】** 置信度三档：高 → 自动执行；中 → 确认 / 标记 / 补充信息；低 → 交人工或回退其他系统；阈值随风险缩放（示例：任何动作低于 0.6 一律转人工；高风险动作需 >0.85 才自动执行）（https://docs.typesafe.ai/confidence、https://docs.typesafe.ai/patterns/confidence-routing）。
- **【官方·集成方】** 每个阈值属于「该答案的用途」而非系统全局：自动执行的门槛应高于「标记待审」。
- **【官方·集成方】** 版本要钉死：`jev-latest` 会随发布漂移，调好阈值后应固定到 `jev-1.13.0` 之类版本化 id；建议日志记录实际作答版本。**选项顺序是输入的一部分**：重排 `Literal`/`Enum` 选项可能改变答案，重要分类要用多种顺序测。

**本节不确定项**

1. **两套 `langchain-typesafe` API 形态并存**：PyPI `0.0.1a3` 与 GitHub PR #40556（2026-09-22 仍 open）的导入路径、构造参数不一致，引用签名前必须确认版本。
2. **LangChain 关于性能倍数的口径在同组织两篇博文里不一致**（200x/400x vs ~450x/~200x），且均转述自 TypeSafe 自测。
3. **LangChain 的 judge 实验是自测且样本小**：一个 agent、5 条固定样本、100 次重复；官方自述 one test on one agent；实验元数据缺 Jev service version；100% oracle 一致率在更多样本上是否保持未知。
4. **注入敏感性的关键数字只有一个第三方实验**：0.76 → 0.48 来自 Octomind 单次测试，VentureBeat 引用同一实验，**不构成两个独立来源**。
5. **「终止 / 完成判断」没有官方模式**：仅有第三方设计草图（作者声明未实现）。不要写成「Jev 可以用来判断任务完成」。
6. **没有官方 Rust SDK**：官方文档索引只列 Python 与 JavaScript / TypeScript；Rust 侧只有两条路——直接实现 HTTP API（`POST /v1/systemone`），或经 LiteLLM / Cloudflare 等网关转发。**未发现官方 Rust SDK；也未排除存在未检索到的社区实现。**
7. **企业治理问题无权威答案**：VentureBeat 的六问清单与 SOC 2 / ISO 27001 / PCI DSS 映射 *partial coverage at best*，属第三方安全媒体分析。
8. **数据留存**：LangChain 明确披露 *TypeSafe does not currently offer zero data retention*；Pydantic 与 LangChain 都提醒工具参数 / state 会送到第三方（即使最终被拒）——选型时必须单独决策。
9. **Cloudflare 文档页把 Jev 列在 "Text Generation" 分类下**，与其「不生成文本」的定位冲突，应是模型目录的分类元数据习惯，引用时不要据此推断能力。

---

## C3. 提问工程与最佳实践

- **【官方】** 提问工程的元规则是两条禁令：**不要问代码能精确计算的东西**；**不要在一个问题里藏多个判断**。官方同时列出另外两条：避免多层间接（System Two 任务）、避免给 state 超过问题需要的上下文（*Jev suffers from context rot…*）（https://docs.typesafe.ai/model-jaggedness/jev-1.13）。
- **【官方】** 一次请求尽量多问：同一 state 上的所有问题并行评估、彼此独立，加问题几乎不增加耗时，只多花一点 token——*问一个你可能不需要的问题几乎是免费的*（https://docs.typesafe.ai/primitives）。
- **【官方】** `instructions` 必须字面、直接、可执行——*Jev answers the question you wrote, not the one you meant*；否定、范围限定词、隐含条件都按字面处理；**question id 不会发给模型**。
- **【官方】** 选项 / 等级要「描述情境」而不是「描述程度」：Score 每一级被独立评估，模型看不到级别编号与相邻级别（实测：纯数字级 score 0.55 / confidence 0.33 → 描述性三级后 0.0 / 1.0）。
- **【官方】** 阈值不是一个数，而是「按动作风险的函数」；且**跨问题类型搬运阈值是错的**，也不能假设任何结构恒等式。
- **【官方】** state 只放这个问题需要的字段（检索与过滤在代码里先做完），纯文本、不支持图像/音频/视频；**英文为主训练语言，其他语言含 CJK 可用但当前准确率较低**（未给量化）。

### C3.1 instructions 怎么写

- **【官方】** 基本形态：每个问题有 `id`、`type`（`choice`/`score`/`noul`）、`instructions`；Choice/Score 还需 `criteria`，Noul 的 `criteria` 可选。`id` 由你自选、**不发送给模型**：*Write the complete question in `instructions`, even when the ID seems self-explanatory.*（https://docs.typesafe.ai/primitives）
- **【官方】** `instructions` 可以是问句，也可以是待判断的陈述句。*"Does the message convey urgency?" 是好问题；"Analyze this message and determine the best course of action" 不是*——那需要慢思考，是**需要拆题的信号**。
- **【官方】** 写得像给普通人看的一句话：*Aim for instructions which are easy for the average person to read and understand.*（https://docs.typesafe.ai/model-jaggedness/jev-1.13）
- **【官方】** 把精确条件写进 instructions。发现自己在「解释你其实想说什么」时，那段解释就是 instruction 缺失的另一半；无法避免解释的地方，拆成两个字面问题在代码里组合。
- **【官方】** **顺着语义方向问，不要反转**：Noul 中 `true` 映射到「否」会明显变差。应写 *Does the message contain personal data?*，而不是 *Is the message free of personal data?*（https://docs.typesafe.ai/primitives/noul）
- **【官方】** **一个问题一个命题**：*Is the customer angry and asking for a refund?* 要拆成两个 Noul 再在代码里组合。**一个 Score 只量一个维度**：级别描述里出现 *punctual and smart and experienced* 就是在量三件事（https://docs.typesafe.ai/primitives/score）。
- **【官方】** 引用 state 中具体字段时用**反引号包住的 dot-and-index 路径**（如 `` `ticket.messages[0].text` ``）；反引号是文档明确要求的写法（*including the backtick characters around each path inside the question*）：

```python
questions = {
    "refund_requested": {"type": "noul",
        "instructions": "Does `ticket.messages[0].text` request a refund?"},
    "policy_supports_refund": {"type": "noul",
        "instructions": "Does `refund_policy` support the refund requested in `ticket.messages[0].text`, given `order.charges`?"}}
```

- **【官方】** `instructions` 可以是 string / object / array / null。官方给出上结构化三条判断标准：(a) 问题需要上下文或示例；(b) 问题的一部分由代码生成（把数据库取来的值放独立字段，而不是拼进字符串模板）；(c) 多个问题的 instructions 结构相似（方便批量构造）。示例结构里的字段名 `question` / `focus` / `what` / `not_for` / `examples` **都不是 API 保留字**——模型会看到名字和值，所以名字要短且能标签化（https://docs.typesafe.ai/primitives/advanced、https://docs.typesafe.ai/concepts/how-to-build-with-system-one）：

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

- **【官方】** 何时不该上结构：*A short, unambiguous question or criterion can remain a string.* 只有当归并在一起会互相模糊时才加结构（https://docs.typesafe.ai/concepts/how-to-build-with-system-one）。

### C3.2 Choice 与 Score 的设计规则

- **【官方】** Choice 规则（https://docs.typesafe.ai/primitives/choice）：选项上限 **255**，每个选项只花几 token；**给全量而不是精选**（*pass the full list of teams, categories, or products rather than a shortlist*）；列表可能覆盖不全时加 `other` / `none of the above` 兜底，让模型能说「都不符合」，而不是硬选一个最接近的错误项；选项名和描述都会发给模型，描述要能把选项彼此分开；名称够清楚时描述可为 null（官方示例 `criteria={"calm": None, "frustrated": None, "angry": None}`）。
- **【官方】** 选项互斥性问题的解法：当两个选项总被混淆（官方例子 `return_policy` vs `return_status`），把每项描述从字符串升级为对象，字段用 `what` / `not_for` / `examples`，**跨选项使用同一套字段名**，`not_for` 专门用来做「互斥性」负向边界。
- **【官方】** 深层分类法不要塞进一个 Choice：**逐层问一个 Choice，在代码里走树**（hierarchical classification）；概率分散时用 beam search 保留 K 条候选路径，而不是贪心选一条（https://docs.typesafe.ai/primitives/advanced、https://docs.typesafe.ai/cookbooks/hierarchical_classification）。
- **【官方】** Score 规则（https://docs.typesafe.ai/primitives/score）：至少 2 级、API 接受最多 **10** 级（「能描述得清楚多少级就用多少级，3 级就够；不要加你描述不清的级」）；编号来自数组下标、从 0 开始，模型**看不到编号也看不到邻居**；`score` 是位置的**概率加权均值**、可落在两级之间（如 1.43），同时给 `probabilities` 与 `confidence`。
- **【官方】** Score 级别写法：**描述情境，不要描述程度**——「Broken or degraded feature, but workaround exists」可用，「Moderately severe」不可用；一个 Score 只量一件事，多维请拆成多个 Score 再用权重合并；量表顶端若有需要区别处理的稀有情形（如「辱骂或威胁」）给它单独一级；没有中间态的离散类别改用 Choice 或多个 Noul；*Two wordings of the same scale can behave differently on your data.*
- **【官方】** 「只用数字当 level」的实测对照（同一份 bug 报告，instructions 为 `"Rate severity from 0 to 2, where 2 is worst"`、criteria `["0","1","2"]`）：→ `score 0.55, confidence 0.33, probabilities 0: 0.45, 1: 0.55, 2: 0.0`；改用三级描述性 level 后 → `score 0.0, confidence 1.0`。
- **【官方】** 结构化 level 的实测对照（同一份 Safari 崩溃报告 state）：纯字符串无 examples → score 1.43 / confidence 0.35；加匹配的 example（*export fails in one browser but works in another*）→ **1.03 / 0.96**；加不相关的 example（*search fails, but browsing categories still works*）→ **回到 1.43 / 0.35**。
- **【官方】** 由此得出的关键警告：**高 confidence 不等于答案更对**——它只说明分布更尖；要用「已知期望级别」的样本挑 example，再在**另外的**输入上验证。另一条必须记住：**相同 score 可以有不同分布**（score 1.0 可能是全部概率在 level 1，也可能是 level 0 与 2 各一半），所以要同时读 `probabilities` 与 `confidence`。
- **【官方】** Noul（https://docs.typesafe.ai/primitives/noul）：返回单个 0–1 的数 `noul`，是「答案为真」的概率，**没有 confidence 字段**（二值分布用一个数就说完了）；**0.5 不等于「中等程度」**——*A Noul value runs from 0 to 1, but it's not a scale of the thing you asked about. It is the probability that the answer is yes.*，要量程度就用 Score；边界模糊时加 `criteria.true` / `criteria.false`，但官方建议两种都试：*The instruction is enough for most Nouls, so try your questions with and without `criteria` and keep whichever gives better answers on your documents.*
- **【官方】** Noul vs Score 官方对照表（同一批候选人的同一问题）：

| 候选人描述 | Noul「Is the candidate strong in Python?」 | Score「How much Python experience…」（4 级） |
| --- | --- | --- |
| 只会 Java/Go，没用过 Python | 0.03 | 0.0 |
| 偶尔写小脚本 | 0.14 | 1.0 |
| 上一份工作每天用 Python 两年 | 0.81 | 2.05 |
| 每天写 Python 八年，含大型 Django 代码库 | 0.92 | 2.89 |

### C3.3 阈值与校准：分层，而不是一个数

- **【官方】** `confidence` 的定义与边界（https://docs.typesafe.ai/confidence）：它是 **0–1 的统计量，由 `probabilities` 的分布形状算出**——全押一项 = 1.0，越平摊越低；*The answer's `confidence` property collapses that shape into a single number from 0 to 1, so you can threshold on it without doing the math yourself.*
- **【官方】** **官方没有公布确切公式**；文档的交互 demo 只是近似（三选项时用 `(3 × largest probability − 1) / 2`），官方称不同计算方式的优劣是专门话题、会另写 cookbook——**截至本次调研该 cookbook 不在 `llms.txt` 索引中**。另一句要记住：*confidence 1.0 只描述模型的答案，不保证答案正确。*（https://docs.typesafe.ai/primitives/score）
- **【官方·集成方】** Pydantic 文档进一步明确：*It is a **margin, not a probability that the answer is right**.* bool 字段的 confidence 是「概率到阈值的距离按剩余空间缩放」；float 字段**没有** confidence 条目（https://pydantic.dev/docs/ai/models/typesafe/）。
- **【官方】** 三段式与按风险分层（https://docs.typesafe.ai/confidence）：高 → 自动执行；中 → 谨慎推进（用户确认、标 review、补信息）；低 → 不执行（转人工 / 要求澄清 / 回退其他系统）。官方同时警告：**正确阈值取决于你的领域与模型在你用例上的表现，从保守值开始，用自己的数据测，再看结果调整。**

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

- **【官方】** Noul 的双阈值（留出中间带），按错误代价方向调：是/否代价相当 → 0.5；**错误的「是」代价高**（派人到现场、退款）→ 提高；**漏掉真正的「是」代价高**（漏标安全问题）→ 降低；中间值 → 转人工。官方代码示例用 `YES = 0.8` / `NO = 0.2`，中间带 `send_to_review(message)`（https://docs.typesafe.ai/primitives/noul）。
- **【官方】** cookbook 里出现过的具体阈值（**务必当作「示例值」而非默认值**）：

| 场景 | 阈值 | 官方自我限定 |
| --- | --- | --- |
| 14 问保险理赔 rubric | `< 0.30` no；`0.30–0.70`（含端点）uncertain → 人工；`> 0.70` yes | *The band is illustrative; it is neither a calibrated guarantee nor an optimized threshold. Set production boundaries from labeled examples…* |
| RAG 段落过滤 | 注入 `> 0.70` 排除；与前提冲突 `> 0.70` 归入冲突块；相关性 `< 0.45` 排除；有可用证据 `> 0.55` 纳入 | *We picked these four numbers for this corpus. Treat them as a starting point, not defaults.* |
| 工单 triage 复合评分 | `spam_risk` 在 `0.4 < x < 0.6` 视为不确定 → 转人工；`topic.confidence < 0.75` 转人工；`spam_risk >= 0.6` 隔离 | — |
| 工单部门路由 | `department.confidence < 0.3` 转人工；`resolution.confidence < 0.5` 反问客户；第二部门概率 `> 0.25` 抄送 | — |

（出处：https://docs.typesafe.ai/cookbooks/consistency_noul_cookbook、https://docs.typesafe.ai/cookbooks/classifying_rag_passages、https://docs.typesafe.ai/concepts/how-to-build-with-system-one、https://docs.typesafe.ai/primitives/choice）

- **【官方】** 「代码改一个数，比改一句 prompt 便宜」是同一条反复出现的官方设计主张：RAG cookbook 把四个阈值集中放在 `THRESHOLDS` 字典，「改路由策略是常量编辑（可走 code review），不是改问法」（https://docs.typesafe.ai/cookbooks/classifying_rag_passages）。
- **【官方】** 两种**不用阈值**的官方范式：① **用概率排序，而不是设阈值**——re-ranking 把同一 Noul 问题对 shortlist 中每个候选各问一次，直接按 `noul` 值排序取最高，*That noul is the score the application sorts on.*（https://docs.typesafe.ai/cookbooks/rerank_typesafe）；② **把等级直接写成动作，从根上去掉阈值**——实体对齐用一个三级 Score，三级就是三种处置（`leave unlinked` / `curator queue` / `assert sameAs`），代码只做 `OUTCOME[min(int(score + 0.5), 2)]`，*There is no threshold to fit…Neither number is something you tune. Both follow from how you worded the levels.*（https://docs.typesafe.ai/cookbooks/entity_alignment）
- **【官方·集成方】** 两条可对照的默认阈值：Pydantic `typesafe_boolean_threshold` 默认 **0.5**（*Raise it where a false positive is the expensive one*）、`typesafe_tool_call_threshold` 默认 **0.6**（*The default of 0.6 is a starting point, not a validated threshold*）；LangChain `AutoModeMiddleware` 默认 `risk_threshold=0.2`。
- **【官方】** **跨类型搬运阈值是错的**。官方反例（同一工单 "I'm not happy with the fit. What are my options here?"）：Noul = 0.22，而 Choice 为 `yes 0.01 / no 0.99 / confidence 0.97`；另一对「是否要求退款」0.72 与「是否在要求退款以外的别的什么」0.47 **和为 1.19**。官方结论：*Don't carry a threshold tuned on a Noul over to a Choice, and don't hold the model to arithmetic identities between separate questions.*（https://docs.typesafe.ai/model-jaggedness/jev-1.13）
- **【官方】** 校准口径的可信度边界：Jev 用 **RLCD（Reinforcement Learning for Calibrated Decisions）** 训练，目标是对齐结果而非人类偏好，因此「更高的 confidence 确实对应更高的准确率」——属官方口径。【第三方】dev.to 指南把警告写在最前面：*those are TypeSafe's own numbers, self-run and unreproduced.*（https://dev.to/valyuai/how-to-use-jev-a-practical-guide-to-typesafes-system-one-model-g5e）官方另称 Jev *extremely consistent*（语义相近输入得到数量相近的输出），但**这不等于结构恒等式成立**。
- **【官方】** 自洽性自测：Jev 每题概率的**平均 std = 0.0102**，低于所有被比较的 LLM 条件（含 temperature 0）；14 问理赔 rubric × 15 次重复，`jev-latest`（返回 `jev-1.13.0`），2026-09-11；时延/成本对照为 TypeSafe **111ms / $0.000043** vs `claude-haiku-4-5` t=0 **1780ms / $0.001798** vs `gpt-5.5-reasoning` **11125ms / $0.033157**（文中声明价格是历史价假设，非当前账单）。同实验有一条实用观察：`covered` 在 0.43–0.53 之间跨过 0.5 阈值，说明单次读数贴近阈值时不可靠（https://docs.typesafe.ai/cookbooks/consistency_noul_cookbook）。

### C3.4 批处理：一次多问，代码决定读哪些

- **【官方】** 机制（https://docs.typesafe.ai/primitives、https://docs.typesafe.ai/models）：一次请求 = 一个 state + 任意多个问题，**每个问题独立评估、并行执行、彼此不构成隐含上下文**，加/删问题不影响其他问题的结果；**加问题几乎不改响应时间，只多花那几个问题的 token**；context 预算 **64k tokens / 请求（state + 全部问题合计）**、**32k 适用于 state + 单个最长问题**；限流 250,000 tokens/秒、1,200 请求/分钟，超限 429。
- **【官方】** 官方实测（`jev-1.12`，GDPR 维基条目 53,777 字符的 pinned revision，8 个 Noul + 2 个 Choice + 3 个 Score，各跑 5 次）：

```
batching                calls        cost  total time
one call, all 13            1   $0.000497       0.27s
13 calls, one each         13   $0.006090       2.71s
# 12.2x cheaper, 10.0x faster
```

- **【官方】** 答案没有任何变化：13 个问题中 11 个在两种批法下 5 次重复的**标准差恰为 0.0**；有余量噪声的两个（`breach_72h` std 0.0055、`criminal_penalties` 0.0045–0.0084）在两种批法下**噪声量级相同**，均值在噪声内一致。官方结论：*batching neither shifts the answer nor adds variance.* 适用条件（cookbook 自注）：**文档主导型负载**，且「速度」是把 13 次单问的延迟**串行相加**得到的，并发发出后差距会缩小，「但 13x 的 token 成本不变」（https://docs.typesafe.ai/cookbooks/parallel_questions）。
- **【官方】** **Speculative Fan-Out**（推测式扇出）：把系统需要的所有问题放进一次请求，**包括只在部分输入下才有意义的问题**，让代码事后决定哪些答案有用。官方 triage 示例中 `bug_severity` 与 `has_reproducible_steps` 只在 category 是 bug_report 时才有意义、`refund_requested` 只在 billing 时有意义，但照旧一起问，最后按分支忽略无用答案（https://docs.typesafe.ai/patterns/fan-out）。
- **【官方】** 什么时候**才**需要第二次请求——规则很硬：同一请求内的问题互相独立，一个答案不会成为另一个问题的上下文；只有当**代码必须先拿到第一个答案**才能构造第二个请求时（需要用它去取更多数据、决定 state 由什么构成、或决定下一题有哪些选项），才发第二次请求。*Two requests are the exception, not the rule.* 官方给出三个边界案例：Skill suggestion（先从 182 个技能里排名，再取前三名全文重判）、Structure recovery（先判断换行是否断句、合并成块后再分类）、Hierarchical classification（每个 Choice 答案决定下一层选项）（https://docs.typesafe.ai/primitives）。
- **【官方】** 反过来，**配对任务不能挤进一个请求**：Re-ranking cookbook 为清晰起见每个 pair 只问一个问题，1,200 次调用（40 query × 30 候选）共 1,536,002 input tokens、成本 $0.0645，并提醒真实应用会把多个问题放进同一次调用；RAG cookbook 声明成本随 k 线性增长，因为每个问题都是关于一个 pair 的。

### C3.5 state 的组织

- **【官方】** 允许的形态（https://docs.typesafe.ai/concepts/state）：字符串（一条消息、一篇文章）、对象（`{"message": "...", "order_id": "A-104"}`）、数组（`["Hi", "My customer number is TS1337.", "My card was charged twice."]`）。官方倾向：**大多数请求用对象**，让每部分有描述性名字、关系清楚；只有单一文本的简单场景才用字符串。
- **【官方】** 硬约束（https://docs.typesafe.ai/concepts/state、https://docs.typesafe.ai/models）：**只支持文本**（字符串 / JSON 对象 / 文本值数组），**不支持图像、音频、视频**；**主训练语言是英语**，其他语言（含 CJK）可用但 *currently have lower accuracy*，官方要求在自己的内容上测、并在路由时特别注意 confidence；上下文 64k / 请求、32k（state + 单个最长问题）。
- **【官方】** 组织原则：① 把 state 当成「你递给专家小组的材料」——*Think of state as the material you would present to a panel of experts before asking them to make a judgment.*；② **内容与问题分离**（state 放内容与支撑事实，questions 放要做的判断：官方示例把退款请求与政策放进 state，再问「客户是否要求退款」「政策是否支持」）；③ **只放相关问题需要的字段**——*Include only the context relevant to the current questions. This helps the model avoid distractions and context rot.*；④ 结构化字段 + 反引号路径引用具体嵌套值，消除歧义；⑤ **配对的判断要把两方放进同一个 state 的两个命名槽位**（实体对齐用 `entity_a` / `entity_b`，RAG 用 `query` + `passage`）；⑥ **state 不是敌对环境**（见 C1.6 风险 1）——官方建议是在 criteria 里写清楚、上线前充分测边界情况。
- **【官方】** 「检索在前，判断在后」被 jaggedness 列为独立失败模式（§Large state full of irrelevant detail）：准确率随 state 中与决策无关的内容增长而下降，**Instead:** 先在代码里检索和过滤，只发问题需要的字段。dev.to 指南把它总结为「两层结构：精确取回，廉价判断」并引用官方原句 *retrieve and filter in code first, and send only the fields the question needs.*（https://docs.typesafe.ai/model-jaggedness/jev-1.13、https://dev.to/valyuai/how-to-use-jev-a-practical-guide-to-typesafes-system-one-model-g5e）
- **【官方】** 可复用的 triage 骨架（https://docs.typesafe.ai/concepts/how-to-build-with-system-one）：先做确定性判断（`status == "closed"` 直接返回，不调模型）；在代码里筛出 `open_orders` 再放进 state；state 分 `ticket` / `customer` / `policy` 三块；一次请求 7 个问题（3 Choice + 3 Noul + 1 Score），其中若干是推测性的；用加权和合成 `spam_risk`（权重 0.45 / 0.30 / 0.25）再按 confidence 与区间转人工；最后按 `topic.choice` 分支，只在对应分支读该分支需要的推测性答案。
- **【官方】** 复合评分的规范化做法：**不同长度的量表必须先归一化**（除以 `len(criteria) - 1`），否则权重没有意义：

```python
def normalized(answers, question_id: str) -> float:
    """Put a score on 0 to 1 by dividing by its top level number."""
    top_level = len(TRIAGE_QUESTIONS[question_id].criteria) - 1
    return answers[question_id].score / top_level

return 0.6 * severity + 0.3 * frustration + 0.1 * report_quality
```

（官方对该示例给出的计算：归一化后 0.62 / 0.64 / 1.0 → `0.6×0.62 + 0.3×0.64 + 0.1×1.0 = 0.664 ≈ 0.66`。）

### C3.6 常见坑（官方 jaggedness 九类失败模式）

**【官方】** jaggedness 页（`jev-1.13`，last reviewed 2026-09-17）列出的 9 类失败模式（https://docs.typesafe.ai/model-jaggedness/jev-1.13）：

| # | 失败模式 | 官方建议 |
| --- | --- | --- |
| 1 | 字面理解（Literal reading） | instructions 写精确条件，边界情况放进 criteria |
| 2 | 数学与数字 | 算术留在代码 |
| 3 | 日期时间比较 | 拆出组成部分，在代码里比较 |
| 4 | 多层间接（Indirection） | 减少跳数；按名字指出相关 state |
| 5 | 大 state 充满无关细节 | 先过滤，只发需要的字段 |
| 6 | 对抗性内容 | 精确写 criteria，上线前测边界 |
| 7 | instructions 与 criteria 矛盾 | 让 criteria 成为 instruction 的延伸 |
| 8 | 常识性结构不变式 | 每个决策只问一种方式；恒等式在代码里强制 |
| 9 | 生成（Generation） | 用生成式模型 |

- **【官方】** 歧义问题 = 漏写了「你其实想说什么」：*When you look at a wrong answer and find yourself explaining what you really meant, that explanation is the missing half of the instruction.* **排查手法**：想为错误答案辩解时，把辩解写回 instruction；无法避免解释的，拆成两个字面问题在代码里合成。
- **【官方】** 选项不互斥：症状是 confidence 偏低、概率在两个选项间分摊。官方对这种低 confidence 的解读是 *low confidence on a Choice often means none of the options are a clear winner*；【第三方】dev.to 指南的解读是「可能是你的 criteria 写错了，而不是模型糊涂了」。解法是升级为 `what`/`not_for`/`examples` 结构、跨选项用同一套字段名（官方在 Advanced 页称之为 *JSON rubric for boundary clarification*）。
- **【官方】** Score 等级重叠 / 多维：官方给出三条低 confidence 诊断——**级别在该输入上重叠**、**问题在量不止一件事**、**state 里信息不够定位**；对应修复是重写级别、拆成多个 Score、补充 state（https://docs.typesafe.ai/primitives/score）。
- **【官方】** 让 Jev 计数——官方替代写法是先在代码里迭代候选，每个候选用一个 Noul，然后自己加总：

```python
result = client.system_one(
    {"items": items},
    {f"item_{i}": Noul(instructions=f"Is `items[{i}]` the name of a fruit?") for i in range(len(items))})
count = sum(result.nouls[f"item_{i}"].noul > YES for i in range(len(items)))
# YES = 0.5（官方注释：阈值由你定，取决于用例）
```

- **【官方】** 数值表示：语义 > 数值——问十六进制 / RGB 相近性不可靠；问「这个颜色读起来像不像警告」这种语义问题表现更好；高级语言比汇编 / 二进制指令好。建议在代码里做转换，把算好的数或命名好的桶传进去。
- **【官方】** 不要指望结构不变式（最容易在 harness 里埋雷）：两个官方反例见 C3.3；结论同前——不跨类型搬阈值、不假设跨问题算术恒等式。
- **【官方】** 让 Jev 生成文本 / 生成数量：Jev 不是为生成训练的，官方说可以通过串接 Choice 强行生成，但*效果差且非常慢*；**不要让 Score 的期望值冒充精确数量**——score 的数值校准是弱项，不能靠两个 level 之间插值重建精确数值，只能用来**过阈值**。
- **【官方】** 过度依赖概率的三种表现：把 confidence 当准确率用；一个阈值打天下；用阈值替代设计（能用「等级即动作」或「按概率排序」时，官方示例都优先不引入阈值）。
- **【官方】** 官方 meta-rule（可直接作为提问模板的 lint 规则）：*As a reminder, avoid the following:* ① Asking the model something code can compute exactly. ② Hiding several judgments inside one question. ③ System Two tasks: more layers of indirections. ④ Giving it more context in `state` than the question needs. *Jev suffers from context rot…*

### C3.7 CJK / 中文风险（对本项目是硬约束）

- **【官方】** state 与问题**只支持文本**；**主训练语言是英语**，其他语言（含 CJK）*accepted but currently have lower accuracy*，官方要求在自己的内容上测、并在路由时特别注意 confidence（https://docs.typesafe.ai/models）。**官方未给任何量化数字。**
- **【未证实】** 「若 harness 要处理中文输入，宁可把判定问题与 state 用英文表述（或做英译后再判定），并在低 confidence 时保持更高的人工兜底比例」这一应对，是上游专题基于官方口径提出的**待验证假设**，不是官方建议，也没有实测数据支撑。
- **【官方·集成方】** 两个相关次要事实：Pydantic 文档记录「选项顺序是输入的一部分，重排 `Literal`/`Enum` 可能改变答案」，因此多语言标签的排列顺序也应纳入测试；Jev 无 stream（一次性返回）、无采样参数（temperature/top_p 被忽略）。

**本节不确定项**

1. **所有性能数字都是厂商自测**：dev.to 指南开头即声明 *those are TypeSafe's own numbers, self-run and unreproduced.*；官方 cookbook 声明数字来自 pinned 数据集 / 缓存，部分价格是「历史价假设，非当前账单」。
2. **同一实验存在两套比值**：`primitives.md` 说 13 问批处理 **11.5x 便宜 / 9.6x 快**，cookbook 正文与 `llms.txt` 说 **12.2x / 10.0x**，原因未说明（可能是不同版本 / 快照）。**引用时以 cookbook 正文为准，并注明另有 11.5x/9.6x 口径。**
3. **`confidence` 的确切公式未公开**：官方只说它由概率分布算出，demo 给的是近似，并预告「不同计算方式的优劣会另写 cookbook」——截至 2026-09-22 该 cookbook 不在 `llms.txt` 索引中。**不要自行复算 confidence，应原样保存 API 返回的 `confidence` 与 `probabilities`。**
4. **未找到公开信息**：(a) 官方的校准曲线 / 可靠性图（只有「RLCD 使概率具备总体意义」的定性说法与稳定性实验——重复实验测的是**稳定性**，不是**准确率**）；(b) 官方单一的「提问最佳实践」页（本片段的规则是从 primitives / how-to-build / jaggedness / cookbook 归纳的）；(c) state 为空、超长截断、非文本混入时的行为；(d) temperature 等采样参数（API 未见暴露；自洽性 cookbook 在 TypeSafe 侧不传 temperature）；(e) 多轮对话式 state 复用的官方示例；(f) **中文 / CJK 的具体准确率数字**。
5. **文档内部的小不一致**：counting 示例的返回写法，官方 jaggedness 页用 `result.nouls[...]`，dev.to 指南写 `result.answers[...]`（以官方 SDK 形态为准，接入时按 SDK/API 参考核对）；自洽性 cookbook 的问题数量有 **14 问**（正文与代码）与 **15 问**（`llms.txt` 摘要与 Noul 页）两套口径，重复次数两边一致均为 15 次，建议写「约 14 问 / 15 次重复」或注明分歧。
6. **一个应避免引用的第三方来源**：SitePoint 的 *Build a Safer AI Agent Harness with Jev and LangChain*（2026-09-18，https://www.sitepoint.com/build-safer-ai-agent-harness-jev-langchain/）虽然标题含 Jev，但正文用的是 `from jev import Decision`、`llm_model="gpt-4o-mini"`、Pydantic schema 约束的 **LLM 结构化输出**，与 TypeSafe 的 Jev API（`state` + `questions` + Choice/Score/Noul）**完全不是一回事**；作者自己在文末也提示「如果 `jev` 包不可用可用 `instructor`/`marvin` 替代」。**不应引用该文作为 Jev 资料。**
7. **与官方口径一致的第三方验证点**（仍属厂商口径的转述）：一次请求多问题、并行、加问题几乎不增时延（LangChain ↔ Primitives）；Jev 不是 LLM 替代品而是 agent 循环里的互补决策层（LangChain ↔ How to build）；「200x 更快 / 400x 更便宜」两个第三方来源都标注为不可复现的自报数字。
