# 专题 06：Jev 独立评测与性能数据

> 调研日期：2026-09-22。本文只记录**已抓取原文**可以支撑的内容，每条数字附出处 URL 与测试条件。
> 明确区分三类证据：**官方口径**（TypeSafe 自评）、**第三方受控实验**（有方法学、可复现）、**轶事/不可引用数字**。
> 厂商名"TypeSafe AI"、模型名"Jev"、竞品名（GPT-5.6 Sol/Terra/Luna、Claude Opus 5、Claude Fable 5.1、GPT-6 Astra、Gemini 3.8 Flash 等）均按原文如实转述；本文不评估这些模型本身。

---

## 一、结论要点

1. **没有任何一份公开评测支持"193.6x faster / 444.6x cheaper"这类头条数字被普遍复用。** 这两个数字来自 TypeSafe 自家 4-workflow 自评（[evals.typesafe.ai](https://evals.typesafe.ai/)），且 TypeSafe 自己在 launch 博客里承认"我们预计这些处于真实世界收益的偏高端"，同时承认工作流由自家能力团队成员制作"可能存在一些偏差"（[typesafe.ai 博客](https://typesafe.ai/blog/introducing-system-one-models-and-jev)）。更关键的是：这些乘数**无法从公开的圆整数字复算出来**（见 §3.3，我做了算术核对）。
2. **官方基准 67.8% 的口径是"与模型生成标签的吻合度"，不是真值正确率。** 参考标签由 GPT-6 Astra 与 Claude Fable 5.1（high thinking）的**平均**生成，且 TypeSafe 明说不辩论 harness 与标签的正确性（[evals.typesafe.ai](https://evals.typesafe.ai/)）。第三方对此的批评已经成型（§5）。
3. **第三方受控实验里最扎实的一份是 LiteLLM 的路由分类器基准**：5.43x（p50 延迟比）与 96.118%（成本差）都有置信区间、并发=1、三次重复、可下载原始数据与脚本。但它测的是**分类环节**，且作者自己列了 4 条局限（标签自编无独立复核、不测下游答案质量、不计完成模型成本等）。[docs.litellm.ai](https://docs.litellm.ai/blog/jev-auto-router-benchmark)
4. **独立评测中，Jev 的"准确率"优势并不稳固，优势主要落在速度、成本与稳定性上。**
   - 钓鱼邮件基准（2000 封，第三方，有完整统计）：Jev 一锤定音式判定 62.6% vs Claude Haiku 4.5 的 81.3%，**显著落败**（McNemar p<0.0001）；校准也更差（ECE 0.154 vs 0.097）。[anisselbd/jev-phishing-bench](https://github.com/anisselbd/jev-phishing-bench)
   - Jevals（人工标签，31,500 次判断）：在 noul（是非）任务上与 6 个 LLM 中的最优者**统计并列**，价格 1/28；在 choice 任务上排第二且被 Gemini 3.8 Flash 显著领先；在 score 任务上"没有任何模型明显胜过按标签基率猜测"。[jevals.com](https://jevals.com/)
   - JevBench（534 条冻结用例、48 个系统）：Jev 1.13.0 综合分第一（74.4），但第二名开源模型 SemIf 只差 1.3 分；GPT-5.6 Luna(low) 的"智能"与"校准"两项都高于 Jev，靠速度与成本才被拉开。[jevbench RESULTS-v1.2](https://github.com/fstandhartinger/jevbench/blob/main/RESULTS-v1.2.md)
5. **最一致的正面发现是"高稳定性 + 低边际成本"**：LangChain 的 Jev-as-a-judge 实验中 Jev 在 500 次重复判定上与人工 oracle 全部一致，分数方差比 GPT-5.6 Terra 低 913x、比 Claude Sonnet 4.6 低 92x。[langchain.com](https://www.langchain.com/blog/jev-agent-evals-langsmith)
6. **"45% faster" 不是官方数字，也不是任何受控基准的结果**，而是一条用户自述轶事，且原帖附带"漏掉了本该报告的细节"的负面注记，原始出处我未能定位（§6）。
7. 对 harness 设计最有价值的一条独立发现：**用 Jev 一次调用问多个正交信号问题、再在代码里组合，比让它一锤定音强得多**——钓鱼基准里把 5 个信号概率喂给逻辑回归，准确率从 62.6% 提到 95.1%（AUROC 0.988，ECE 0.027）。[jev-phishing-bench](https://github.com/anisselbd/jev-phishing-bench)

---

## 二、口径总览（谁测的、测了什么、能不能推广）

| 来源 | 性质 | 样本与条件 | 核心数字 | 关键局限 |
| --- | --- | --- | --- | --- |
| [evals.typesafe.ai](https://evals.typesafe.ai/) | 官方自评 | 4 个工作流，共 711 例 | 67.8% / $0.0004 / 0.4s（聚合） | 标签=两模型平均；作者自家出题；"假设 harness 正确" |
| [typesafe.ai 博客](https://typesafe.ai/blog/introducing-system-one-models-and-jev) | 官方公告 | 演示 + 同一组 workflow evals | 193.6x faster / 444.6x cheaper；70–500 ms；$0.042/MTok，输出免费 | 官方自我标注大量 nuance；速度测试"在我们西海岸的笔记本上"跑 |
| [LiteLLM](https://docs.litellm.ai/blog/jev-auto-router-benchmark) | 第三方受控 | 80 条自编案例 × 3 次重复 = 每分类器 240 次；并发 1；2026-09-18 | 5.43x（p50）、3.88x（p95）、成本 −96.118% | 标签由案例作者自定、无独立复核；不测下游质量；不含完成模型成本 |
| [LangChain](https://www.langchain.com/blog/jev-agent-evals-langsmith) | 第三方受控（合作方） | 5 条天气请求 × 100 次重复 = 500 次判定；人工 oracle | 500/500 与 oracle 一致；方差低 92–913x；$0.34 vs Claude $28.17 | 样本极小（5 条）、任务单一；作者自称"观察性"证据 |
| [Jevals](https://jevals.com/) | 第三方独立 | 3 任务 × 300 问 × 5 次 = 31,500 次判定；人工标签 | 见 §4.3 | 每类问题只用一个公开数据集；无前沿模型行（预算） |
| [JevBench v1.2](https://github.com/fstandhartinger/jevbench/blob/main/RESULTS-v1.2.md) | 第三方独立 | 534 条冻结用例/入场模型，48 个系统 | Jev 74.4 分第一，领先第二名 1.3 分 | 部分自托管延迟按假设 ×2 调整；成本多为估算 |
| [jev-phishing-bench](https://github.com/anisselbd/jev-phishing-bench) | 第三方独立 | PhishNChips v5.2，2000 封邮件，无并发 | 62.6% vs Haiku 81.3%；ECE 0.154 | 邮件正文为合成；真值来自 URL 信誉库而非人工读信 |
| [Every / Mike Taylor](https://every.to/also-true-for-humans/mini-vibe-check-typesafe-s-jev-judged-everything-i-ve-written-in-0-7-seconds) | 第三方实测 | 37 篇文档 × 21 问 = 777 判断；另有 12 段合成文本对比 | <0.7s、约 1/4 美分；缺陷检出 6/7 vs Fable 5.1 的 7/7 | 作者自评"想要更完整的准确率检查"；样本小 |
| [Vercel fx](https://vercel.com/changelog/typesafe-ai-jev-now-available-on-ai-gateway) | 第三方（厂商自测） | **未公布数据集与案例数** | "~5–18x faster and more accurate"（vs gpt-5.6-luna） | 无公开方法学 |
| [jev-agent.com（Jagent）](https://jev-agent.com/benchmarks) | 第三方（有商业利益，卖 credits） | 8 fixtures × 5 次 = 每模型 40 次；另 27 张自编工单 × 3 遍 | Jev 中位 352 ms；服务器时间 1 问 70ms / 5 问 74ms | 自曝两次"差点把有利于 Jev 的假象写进结论" |

---

## 三、官方口径（TypeSafe 自评）

### 3.1 4-workflow 基准的具体数字与口径

官方评测站 [evals.typesafe.ai](https://evals.typesafe.ai/) 的方法学原文要点：

- 4 个工作流：**Security Incidents、Agent Trace Observability、Invoice Processing、Customer Service**。
- **参考标签由 GPT-6 Astra 与 Claude Fable 5.1 的答案平均生成，两者都开 high thinking，回答 harness 中的每一个问题**；"All other models are evaluated using the provider's default reasoning settings"。
- 官方明说：**"Instead of debating the correctness of the harness and labels, we assume that the code is correct"**——即不辩论 harness 与标签的正确性，直接假设代码是对的。
- 官方自述：**"Averaged across the four example tasks, every model is more accurate, cheaper and faster in the workflow than it is with the same policy as a prompt."**（把策略拆成工作流比塞进一个 prompt 更好——**这一点对 harness 设计比 Jev 本身更重要**）

逐工作流数字（第三方转述，数值为 dashboard 圆整显示值；两处独立来源一致）：
[kingy.ai 评测](https://kingy.ai/blog/typesafe-jev-review-the-ai-model-that-doesnt-generate-text/)、[dev.to 审计](https://dev.to/gabrielanhaia/jev-beat-gpt-luna-by-1-point-gpt-6-and-claude-wrote-the-answer-key-314k)

| 工作流 | n | Jev 准确率 / 成本每例 / 延迟 | 该工作流最强对比 | 差距 |
| --- | --- | --- | --- | --- |
| Security incidents | 240 | 61.7% / $0.0001 / 0.3s | Claude Opus 5：66.2% / $0.0574 / 15.1s | −4.5 |
| Agent trace observability | 117 | 71.6% / $0.0003 / 0.5s | GPT-5.6 Sol：76.6% / $0.0575 / 40.3s | −5.0 |
| Invoice processing | 150 | 61.8% / $0.0011 / 0.5s | GPT-5.6 Sol：79.1% / $0.2152 / 34.3s | **−17.3** |
| Customer service | 204 | 76.0% / $0.0001 / 0.4s | GPT-5.6 Sol：78.3% / $0.0323 / 10.1s | −2.3 |
| **聚合** | **711** | **67.8% / $0.0004 / 0.4s** | GPT-5.6 Sol：74.1% / $0.0836 / 23.3s | −6.3 |

聚合榜其他行（dev.to 转述官方页面，dev.to 称该页共评分 9 个模型）：
GPT-5.6 Terra 67.9%（$0.0304）/ Claude Sonnet 5 67.8%（$0.1174）/ GPT-5.6 Luna 66.8%（$0.0033）/ Claude Opus 5 73.1%（$0.1761）。

> 口径提醒：**"67.8% vs 67.9%"这种"打平"是与"两个模型平均标签"的打平**。dev.to 的批评很直接：两个前沿模型都答错的题，答对的模型反而被扣分；两个模型共享的盲区，有同样盲区的模型反而被奖励。

### 3.2 官方自己写下的偏差与限制（可直接引用）

来自 [typesafe.ai launch 博客](https://typesafe.ai/blog/introducing-system-one-models-and-jev)（原文英文，以下为要点转述 + 关键原句）：

- "The content of these workflows were **not deliberately chosen nor constructed to make our model look good**, and are not in our training distribution. However, they were **made by individuals on our model capabilities team, so some bias could exist.**"
- "We use the average of GPT-6 Astra and Fable 5.1 as the reference answer, **which biases answers towards OpenAI and Anthropic's models. We likely underestimate the relative performance of our model** and DeepSeek's models."
- "**The LLMs use our System One LLM wrapper**, which constrains LLMs to output structured decisions compatible with our API. We have found this to be the most accurate way to get decisions from LLMs, but **this tends to be slower and more expensive** than giving decisions without probabilities."（即为 LLM 加了自家适配层——这层对 LLM 而言是额外开销，这一句对"公平性"讨论非常关键）
- 关于"零幻觉"：**"Our number is not empirical. Schema matching is guaranteed, thus we can confidently add 0% into the plots."**
- 关于定价可持续性：**"We can't prove it isn't subsidized; we'll need the long-term to prove the sustainability of our pricing."**
- 关于速度："our **published evals are generally run from our laptops on the West Coast**（this is where our service is currently based）"。
- 关于 193.6x / 444.6x：**"This is where the claims of 193.6x faster, 444.6x cheaper on our home page comes from, and we expect that these are on the higher end of real world gains."**
- 演示查询的自我打折："The relatively shorter input **paints our model in an advantageous light**."

### 3.3 "193.6x / 444.6x" 无法从公开圆整数字复算（我的核对）

[flowtivity 的逐条审计](https://flowtivity.ai/blog/jev-typesafe-ai-decision-model/)引用了 TypeSafe 官网的每工作流数字：**$0.000081 vs $0.013880**，**0.114 秒 vs 8.566 秒**。我做了算术核对：

- 8.566 / 0.114 = **75.1x**（≠ 193.6x）
- 0.013880 / 0.000081 = **171.4x**（≠ 444.6x）

[kingy.ai](https://kingy.ai/blog/typesafe-jev-review-the-ai-model-that-doesnt-generate-text/)也独立指出同一问题："The public homepage headline—193.6% faster and 444.6% cheaper—**cannot be reconstructed from the rounded table**, so quote those numbers as TypeSafe's workload-specific claims rather than universal multipliers."

**结论：头条乘数应视为不可复算的营销数字，不要引入技术决策。**

### 3.4 官方对"并行提问"的说明

[evals.typesafe.ai](https://evals.typesafe.ai/) 与 [typesafe.ai 博客](https://typesafe.ai/blog/introducing-system-one-models-and-jev)：问题被独立、并行求值，"adding questions barely adds response time"。第三方对此做了一次实测（§4.7），结果支持该说法。

---

## 四、第三方受控实验

### 4.1 LiteLLM Auto Router 基准（最完整的一份）

URL：<https://docs.litellm.ai/blog/jev-auto-router-benchmark>（页面标注 Last Updated: 2026-09-18）

**测试条件（原文摘录）**：
- 运行时间 2026-09-18 UTC；`jev-latest` 首先解析到 **jev-1.13.0** 然后固定；对比模型 **anthropic/claude-haiku-4-5-20251001**。
- 语料：**80 条自编案例**，每个分类器跑 **3 次**，共 **240 次**测量调用/分类器。每层 20 例；40 短 / 16 长 / 8 追问 / 8 工具上下文 / 8 边界例。
- 并发 1；两者 deadline 10,000 ms；**零重试、关闭熔断**；seed 20260918；关闭缓存；上下文 3 轮 / 8,000 字符；排除 1 次模型解析调用与 4 对预热。
- 两侧使用**同一份人工编写的 tier rubric 与开场指令**，但线格式不同（LLM 走结构化输出包装，JEV 走 Choice 问题）。作者明说**两侧 prompt 都未经独立调优**。

**结果**：

| 指标 | JEV | Haiku |
| --- | --- | --- |
| 与自编期望 tier 一致 | 228/240 = **95.00%** | 177/240 = 73.75% |
| 平均延迟 | 138.38 ms | 720.71 ms |
| **p50 延迟** | **126.81 ms** | **688.40 ms** |
| p95 延迟 | 231.16 ms | 896.94 ms |
| 最大延迟 | 401.26 ms | 1,914.98 ms |
| 240 次调用注册表价成本 | **$0.007706664** | $0.198534 |
| 单次分类平均成本 | $0.0000321111 | $0.000827225 |
| HTTP 200 | 240/240 | 240/240 |
| 提供方错误 / 超时 / 回退 | 0/0/0 | 0/0/0 |

- **5.43x 的定义**：`Haiku_p50 / JEV_p50 = 688.395634 / 126.814470 = 5.43x`，是**聚合统计量之比，不是逐请求比值平均**；同一口径下 p95 = 3.88x、均值 = 5.21x。
- 成本差：`100 × (1 − 0.007706664/0.198534) = 96.118%`。
- 95% 聚类自助置信区间：p50 比值 5.20x–5.71x；p95 3.88x（2.94–4.78）；成本节省 96.118%（95.973–96.266）；期望 tier 匹配率差 21.25 个百分点（12.92–30.42）。
- 两者互相一致率 78.75%（189/240）。JEV 的 4 个不匹配都是**降到更低 tier**（M07/M13/M15 MEDIUM→SIMPLE，C10 COMPLEX→MEDIUM），"最大差异出现在主观的 MEDIUM 边界"。
- 子集：短 40 例 JEV 97.50% / Haiku 73.33%；**长 16 例 JEV 87.50% / Haiku 62.50%**；追问 100%/87.50%；工具上下文 100%/83.33%；边界 87.50%/75.00%。**"边界"子集的差值区间包含 0**。

**成本口径（关键）**：只用**分类器**成本，**不含完成模型、嵌入、预热与评估裁判**；用观测 token × 归档的注册表价格，未核对提供方账单、税、折扣与套餐权益。原文："Cost savings stop at the classifier boundary."

**作者主动声明的局限（可直接引用）**：
- "The expected tiers were **authored with the synthetic prompts, without independent review**."
- "This comparison **does not establish general classification accuracy or the quality of the final answers**."
- "The classifiers agreed… **Agreement ignores whether either classifier matched the authored label**."
- "Labels and prompts **came from the same author, without independent annotation or blind adjudication**."
- "A **future jev-latest resolution may produce a different model**, requiring a new experiment."
- 另外承认：1 ms deadline 的那次测试"没有保留异常子类型，也没有 JEV 用量返回，**所以在提供方是否计费/入账这一点仍未验证**"。

**可复现性**：提供 `jev-live-evidence-20260918.tar.gz`（含 cases.jsonl、protocol.json、attempts.jsonl、sanitized wire.jsonl、metrics.json、registry、gateway 证据与哈希清单），给出 sha256 与 `python analyze.py`；离线分析需 Python + Pydantic。

**可引用的配置片段（原文）**：

```yaml
- model_name: jev-router
  litellm_params:
    model: auto_router/complexity_router
    complexity_router_default_model: {{openai_large}}
    complexity_router_config:
      tiers:
        SIMPLE: {{openai_small}}
        MEDIUM: {{openai_large}}
        COMPLEX: {{anthropic}}
        REASONING: {{anthropic_large}}
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

注意作者提示的**默认值差异**："This deployment example uses built-in tier criteria, a 3-second deadline and a circuit breaker… **The benchmark used a shared custom rubric, a 10-second deadline, assistant context and disabled breakers**"。

### 4.2 LangChain：Jev 作为 agent 评估裁判

URL：<https://www.langchain.com/blog/jev-agent-evals-langsmith>（2026-09-20，作者 D. Shea、S. Roche）

**条件**：用 Deep Agents 0.7.15 构建被测 agent；5 条"天气请求"构成 LangSmith 数据集；把 agent 每次运行的输出固定成样本，让每个裁判对**同一批固定输出**打分；两个信号 `quality`（连续分）与 `does_pass`（二值）；**人工评审给同一 rubric 打标签作为 oracle**；每条样本重复 **100 次**（5 × 100 = 500 次判定）；对比 GPT-5.6 Luna / Terra / Claude Sonnet 4.6。

**结果**：
- `does_pass` 与 oracle 一致：**Jev 500/500（100%）**；Terra 99.8%；Luna 96.4%；Claude 80.0%。
- 分数方差（越低越好）：**Jev 平均逐例方差 0.0000149**；Luna 高 433x；Terra 高 **913x**；Claude 高 92x。
- 成本与时延：Jev 平均 **0.44 s**、**$0.00035/次**；本次实验 Jev 合计 **$0.34**，Claude 合计 **$28.17**。

**作者自己的限制声明（重要）**：
- "This experiment **cannot tell us why** Jev's scores varied less… the result is **observational, not evidence that its training objective caused the lower variance**."
- "We **still need to see whether the results in this experiment carry over to other agents and production workflows**."
- "**low cost can amplify mistakes** - a consistently wrong evaluator can produce bad feedback at scale."
- 复现细节：Jev 通过 `langchain-typesafe==0.0.1a2` 访问；**"The Jev service version was not available in the experiment metadata"**；LLM 裁判**未设 temperature/top-p/seed/max tokens**。
- 注意利益关系：LangChain 与 TypeSafe 有联合直播活动，本实验属"合作方做的第三方测试"。

### 4.3 Jevals.com：人工标签的独立基准

URL：<https://jevals.com/> 与 findings <https://jevals.com/notes/2026-09-18/>（2026-09-18）

**规模**：7 个模型、**31,500 次评分判断**、300 问 × 5 遍 × 3 任务；**人工标签**；"run from a laptop at UTC+3"；总花费 $15.07（另有 $3.98 的废弃运行）。明示"Jevals is independent. It is not affiliated with TypeSafe AI"。

**分数口径**："Decision Score: 100 = perfect, 0 = guessing the label base rates"——即**相对随机基线的分数**，这是与厂商"吻合度"口径的最大差别。

| 任务（问题类型） | 数据集 | Jev | 最强 LLM | 结论 |
| --- | --- | --- | --- | --- |
| noul（是非） | PubMedQA 300 问 | **69.0** | Gemini 3.8 Flash 73.0 | 配对差值 95% 区间 −2.5 ~ +10.4 **包含 0**，即统计并列；Jev $0.03/千次 vs Gemini $0.80，p95 0.65s vs 5.5s → **1/28 价格** |
| choice（多选一） | Banking77，77 意图 | **67.8**（准确率 79.7%） | Gemini 3.8 Flash 74.1 | Gemini **显著领先**；Jev 与 GLM-5.3（66.8）并列第二 |
| score（有序评分） | HelpSteer2 helpfulness 0–4 | **9.2** | GLM-5.3 7.8 | **"no model clearly beats answering with the label base rates"** |

**门槛/置信度的实测（对 harness 极有价值）**：给每个模型各自设定达到 95% 准确率的阈值后，
- noul：Jev 可**独自承接 86%** 的决策（Gemini 3.8 Flash 79%）；
- choice：Jev 54%（Gemini 3.8 Flash 45%、DeepSeek V4.1 Flash 39%、GLM-5.3 38%）；
- 共享的 choice 门槛 0.96 下，Jev 承接 59% 且其中 94.3% 正确，而 LLM "在至多 15% 的样本上给出 ≥0.96"，原因之一是**偏好 0.95 这类圆整值**。

**作者声明的限制**：每类问题只用**一个**公开数据集；本轮**没有前沿 LLM 行**（一次 GPT-6 Astra 全量约 $39、Fable 5.1 约 $62，超过整轮 $25 预算）；**LLM 的概率是模型在回复里"口头声明"的，不是从 token 概率读出**；延迟来自单条住宅宽带；Gateway 不报告 Jev 版本。

**与厂商数字的交叉对照（Jevals 自己做的）**：
- Jevals 测出 Banking77 准确率 79.7%，"matches the **80.3%** a Vercel engineer reported on a different **385-item sample**"（见 [vercel/ai#20846](https://github.com/vercel/ai/issues/20846)）；
- TypeSafe 宣称 70–500 ms、文档称多数查询约 100 ms，**Vercel 实测中位数 132–272 ms**；Jevals 从 UTC+3 测得中位 0.44–0.48 s、p95 0.65–0.69 s（含跨洲网络，Jev 只在一个美国区域提供服务）。

> 这条很重要：**"Jev 的延迟高度依赖你与它服务区域的网络距离"**。TypeSafe 自己也承认评测跑在西海岸笔记本上。

### 4.4 JevBench v1.2：跨模型、含开源决策模型

URL：<https://github.com/fstandhartinger/jevbench/blob/main/RESULTS-v1.2.md>

**口径**：`JevBench Score = Intelligence above chance, Calibration, Speed, Cost — 25% each, geometric mean`；**534 条冻结用例**（完整入场者），公开打分代码、冻结任务、适配器与结果产物。

**榜首**：**Jev 1.13.0 74.4 分**（Intelligence 85.7 / Calibration 82.7 / Speed 83.3 / Cost 52.0，$0.0399 每千次决策，p50 0.65 s，生产 API）。
第二名 **SemIf (Qwen3.5-4B) 73.1**（自托管 RunPod），**仅差 1.3 分**。

**对"Jev 更聪明"这一说法最不利的一行**：**GPT-5.6 Luna (low) 排第 14，但 Intelligence 95.3、Calibration 89.8，都高于 Jev**；它输在 Speed(77.5) 与 Cost(28.5，$0.2419/千次)。也就是说这个榜单里 Jev 的第一名**主要来自成本与速度轴**。

**作者声明的关键限制**：自托管/演示端点的延迟被**乘以 2 并加 0.15 s** 以"近似生产负载"，作者明说这**是假设而非测量**；成本多为估算；留出用例会暴露给被测服务。

### 4.5 钓鱼邮件基准：最能反驳"Jev 准确率更高"的一份

URL：<https://github.com/anisselbd/jev-phishing-bench>（2026-09-17；仓库说明 Every 那篇是"此前唯一公开的独立测试"）

**条件**：2,000 封来自 **PhishNChips v5.2**（1,000 钓鱼 + 1,000 合法职场邮件，其中 333 封是跨域假阳性陷阱）的邮件；同一顺序、**无并发**、每封一次调用；Jev 收到 JSON 对象 + **一次请求里 9 个类型化问题**；基线 Claude Haiku 4.5（native Messages API，无 thinking，temperature 0）；比例为 95% Wilson 区间，准确率差用 exact McNemar。

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

**结论原文**："Jev's own verdict **loses clearly on accuracy** (McNemar p < 0.0001) and wins on speed and cost."

**最有价值的发现（对 harness 直接可用）**：同一次调用里的 5 个信号问题
- 单看 `sig_free_hosting` 就能达到 AUROC 0.96；
- 在该信号上设固定规则 = 89.5% 准确率（无需拟合）；
- **5 个信号的交叉验证逻辑回归 = 95.1% 准确率、AUROC 0.988、ECE 0.027**。

**作者加的对照（17 Sep，被质疑后追加，未修改原数字）**：
- **非 AI 基线**：一个通用的短链/免费主机/IPFS/文档托管域名清单 + 发件人与链接的 eTLD+1 比对，单靠清单规则就有 **91.6% 准确率**（召回 83.5%、假阳性 0.2%）——"The dataset largely separates by construction."
- **切分验证**：一半选信号与阈值、另一半出数字。在 B 半上，Jev 最佳单信号 89.4%，**低于两行正则的 91.8%**（p=0.0032）；Jev 5 信号回归 95.0%（AUROC 0.982）vs 正则 2 特征回归 91.8%（p=0.0018）。
- **把同样的问题原样问 Haiku**：在 B 半上 Haiku 的最佳单信号 94.2%，**高于 Jev 的单信号**（p<0.0001）；Haiku 的回归 93.2%（AUROC 0.991）与 Jev 的 95.0%（AUROC 0.982）**在准确率上无显著差异**（McNemar p=0.063）而 **AUROC 反而更高**。
- 留存的优势：**"about 27 times cheaper and 5 times faster than Haiku for signals of comparable quality, on a dataset that a regex already separates at 91.8%."**

**四种问法都跑了**（防止"事后择优问法"）：verdict(choice) 62.6% / AUROC 0.689 / ECE 0.154；is_phishing(noul) 60.6% / 0.684 / 0.172；verdict_alt_click 63.2% / 0.700 / 0.115；verdict_alt_minimal 58.4% / 0.635 / 0.241。结论：换措辞最多 +0.6 点，最差 −4 点。

**其他可引用细节**：Jev 只公布输入价（$42/十亿 token）；作者核对两遍共消耗 4,561,792 token（3.66M 输入、0.90M 输出）**被计费 $0.15，与"仅输入计费"一致**——这是对"输出免费"的第三方实证。限制：邮件正文是合成的；真值来自 URL 信誉库而非人工读信；每系统只用一套 prompt。

### 4.6 Every：媒体方的小规模但诚实的实测

URL：<https://every.to/also-true-for-humans/mini-vibe-check-typesafe-s-jev-judged-everything-i-ve-written-in-0-7-seconds>（Mike Taylor，Every 的 head of evals，2026-09-15）

- 37 篇文档（27 篇他本人的 Every 文章 + 10 篇刻意 AI 风格的对照） × 21 个"AI 痕迹"问题 = **777 个判断，<0.7 秒，估计约 1/4 美分**。
- 11 个实验合计 **1,709 个判断，估计总成本 < 1 美分**；场景由 GPT-6 Astra 在 Codex 里生成。
- **Dan Shipper 的对照**：12 段合成文本（6 段清晰、6 段刻意埋缺陷），Jev 与 Fable 5.1 跑同一 4 项写作检查。Jev 每段中位 **0.35 s**（Fable 高 effort 为 8.83 s，约 **25x** 快），估计成本约 **580x** 低；但 **Jev 只抓到 7 个缺陷中的 6 个，Fable 全抓到**——有一个"无法被教授"的日历提案，Fable 抓到而 Jev 三次运行全部漏掉。
- 作者原话："Whether you'd trust those results is another question… **I'd want a more thorough accuracy check before putting it into production.** Even with that caveat, it's useful as an early warning system."
- **注**：TypeSafe 的 launch 博客提到在 AIE（AI Engineer）预发布过，Every 与其关系未见披露；引用时宜标为"媒体方实测"。

### 4.7 Jagent（jev-agent.com）：有商业利益的第三方，但透明

URL：<https://jev-agent.com/benchmarks>（有卖 credits；引用时须标注利益关系）

- **自测**：8 个 fixtures × 5 次重复，每模型 40 次运行，同一 gateway，2026-09-20。Jev 1.13 中位 **352 ms**、$0.0000188；Mistral Small 3.2 1,343ms；Gemini 2.5 Flash Lite 877ms；GPT-5 nano 7,504ms。
- **对官方数字的一次独立测量**：用服务器自己的 `x-envoy-upstream-service-time` 头（而非减去估算网络时延）测并行性——**1 个问题 70 ms，3 个 72 ms，4 个 81 ms，5 个 74 ms**。结论："'Questions are evaluated in parallel' is a vendor claim everywhere else — this is it read off the server's own header."
- **自曝两次"差点把对 Jev 有利的假象写成结论"**：一次是没开 `response_format: {type:"json_schema", strict:true}` 就说"chat 模型返回类型不可靠"（开启后 Gemini 2/5→5/5、GPT-5 nano 4/5→5/5）；一次是没设 `temperature: 0` 就说"chat 模型不一致"（设置后全部稳定）。作者结论：**"If you are evaluating a decision model against a chat model, check your own request body before you believe your own table."**
- **准确率上 Jev 并不赢**：27 张自编工单 × 3 遍，Jev 27/27、Mistral 27/27、Gemini 26/27、GPT-5 nano 26/27。唯一只有 Jev 能产出的是置信度：清晰集 0.979、歧义集 0.841（方向对，但幅度小于按阈值路由所期望的）。
- 对 frontier 模型的同 fixture 对比：Jev 358ms/$0.0000226；GPT-5.6 Sol 1,545ms/32.5x；GPT-5.6 Terra 1,315ms/34.3x；Opus 5 4,267ms/**184.9x**，且 Opus 5 五次全把概率以字符串返回、severity 返回 `'2'`（类型不合规）。

### 4.8 Vercel：最常被引用的第三方，但最不透明

- [Vercel changelog](https://vercel.com/changelog/typesafe-ai-jev-now-available-on-ai-gateway)（2026-09-16）：Jev 上架 AI Gateway；措辞为 **"TypeSafe reports Jev was up to 193.6x faster and 444.6x cheaper"**——即 **Vercel 只是转述厂商数字**，不是自测。
- Vercel CEO Guillermo Rauch 在 X 上称 fx 的默认安全审查器（当时跑 GPT Luna）换成 Jev 后 **"up to 18x faster (p95) and more accurate"**；Vercel 工程师 Pranit 称 **"~5-18x faster and more accurate than gpt-5.6-luna"**。转述见 [dev.to 审计](https://dev.to/gabrielanhaia/jev-beat-gpt-luna-by-1-point-gpt-6-and-claude-wrote-the-answer-key-314k)。
- **dev.to 明确指出**："beyond the speed ranges in those two posts, **Vercel's result has no dataset or case count in public**"，并称截至 9 月 17 日 Vercel 未发布该基准。
- 另一条独立交叉：Vercel 工程师在 [vercel/ai#20846](https://github.com/vercel/ai/issues/20846) 报告 385 条样本上 Banking77 准确率 80.3%，与 Jevals 的 79.7% 一致（见 §4.3）。

---

## 五、第三方对官方基准的质疑（harness bias 等）

### 5.1 标签是模型生成的，不是真值

- [dev.to（Gabriel Anhaia）](https://dev.to/gabrielanhaia/jev-beat-gpt-luna-by-1-point-gpt-6-and-claude-wrote-the-answer-key-314k)：官方页"说参考标签是**由 GPT-6 Astra 和 Claude Fable 5.1 在 high thinking 下的回答平均生成**的……页面**没有描述任何人工标注**"；并指出后果："Where both frontier models get a case wrong, a model that gets it right is **marked down**. Where they share a blind spot, a model with the same blind spot gets **rewarded**."
- [kingy.ai](https://kingy.ai/blog/typesafe-jev-review-the-ai-model-that-doesnt-generate-text/)："the reported accuracy is better understood as **agreement with a model-derived reference rather than as independently verified correctness**"；并给出本质评价——这份表"**a model-agreement test rather than verified ground truth**"。
- [Anthony Maio](https://anthonymaio.substack.com/p/jev-the-language-model-that-wont)：参考标签"came from averaging GPT-6 Astra and Claude Fable 5.1 at high reasoning settings, **not from independent operational ground truth**"。

### 5.2 TypeSafe 自己承认的 harness bias（见 §3.2）

- 工作流由自家能力团队成员制作，"some bias could exist"；
- 参考答案偏向 OpenAI/Anthropic（官方自称这反而**低估**了自家模型）；
- **LLM 侧被套上 TypeSafe 自家的 "System One LLM wrapper"**——[flowtivity](https://flowtivity.ai/blog/jev-typesafe-ai-decision-model/) 的审计直接记下这一条为"against LLMs wrapped in TypeSafe's adapter"。

### 5.3 速度数字的可比性问题

- HN 评论者 ramon156（经 [flowtivity](https://flowtivity.ai/blog/jev-typesafe-ai-decision-model/) 转述）："this sounds good but so far all claims just sound like marketing terms"，指出 **70 ms vs 329 秒不是同类比较**，如果 LLM 基线在跑完整 chain-of-thought 生成的话。
- TypeSafe 官方承认速度评测跑在自家西海岸笔记本、延迟 70–500 ms 与区域强相关（[博客](https://typesafe.ai/blog/introducing-system-one-models-and-jev)）；Jevals 从 UTC+3 测得中位 0.44–0.48 s（[jevals.com](https://jevals.com/notes/2026-09-18/)）；LiteLLM 测的是**分类延迟**且明说不含端到端速度或负载下吞吐（[litellm](https://docs.litellm.ai/blog/jev-auto-router-benchmark)）。

### 5.4 "零幻觉"与"校准"的口径问题

- [kingy.ai](https://kingy.ai/blog/typesafe-jev-review-the-ai-model-that-doesnt-generate-text/)：类型安全 ≠ 语义正确。"A wrong Choice **is not a type error**."；并且"confidence **is not independently demonstrated to be calibrated**"，TypeSafe 未公布标准校准指标。
- [Anthony Maio](https://anthonymaio.substack.com/p/jev-the-language-model-that-wont)（2026-09-15 时点）："The **reward function, architecture, training procedure, and calibration methodology are all undisclosed. There are no published calibration curves and no independently reproducible paper.**"；并给出两条结构性风险：校准必须在**分布漂移**下成立；以及 **"individually calibrated judgments do not automatically compose into a calibrated workflow once you run them through thresholds, weights, and branches. Correlated mistakes survive composition."**
- Maio 的判决句："Until that happens, **the strongest case for Jev is architectural rather than algorithmic**."
- [flowtivity](https://flowtivity.ai/blog/jev-typesafe-ai-decision-model/) 记录了 HN 上最有价值的一次交换：TypeSafe 创始人 Diogo Almeida **在发布帖里直接承认**"'It's also possible to be confidently wrong…'"，即"保证类型合法的答案仍可能是错的"。
- HN 上另一条被转述的批评："'frontier model' for something that cannot hold a conversation **borrows credibility it has not independently earned**."

### 5.5 独立评测给出的反面证据（不只是评论）

- **准确率**：钓鱼基准 Jev 62.6% vs Haiku 81.3%（p<0.0001）；Jevals choice 任务被 Gemini 3.8 Flash 显著领先；JevBench 里 GPT-5.6 Luna(low) 的 Intelligence/Calibration 都高于 Jev。
- **校准**：钓鱼基准 ECE 0.154 vs Haiku 0.097（Jev 更差）——这与"calibrated decisions"的定位形成直接张力。
- **门槛稳定性**：[jev-certify 的 CLINC150 研究](https://github.com/nikkoxgonzales/jev-certify)（经 [awesome-typesafe](https://github.com/AbdelStark/awesome-typesafe) 索引）——5% 的静默误路由上界在 400 条域内样本上成立（84.75% 自动路由，每来件损失 2.25%），但**另一个"范围门"在域外比例上升时超出 5% 目标 3.6 倍**。
- **排序**：[jev-orderby-bench](https://github.com/yodablocks/jev-orderby-bench)（同上索引）——20 Newsgroups 主题归属通过 6 个排序门，但在 306 条人工标注的购物相关性上**6 条里挂了 4 条**；360 行里 **53 行并列在 0.99**（两位小数输出），`LIMIT k` 会切在并列内；**把 40 行批处理成一次请求会把原本通过的排序门变成失败**。
- **弃权行为**：[jev-calibration-audit](https://github.com/jujumilk3/jev-calibration-audit)（同上索引）——KoBBQ 上，当选项里有 "unknown" 时 Jev 对 **95% 的 300 条歧义样本选弃权**；把该 gold 选项移出选项集后，这些样本准确率**必然为 0%**，且 **79% 的答案选了数据集里的刻板印象答案**。
- **分布外**：[jev-ood-calibration](https://github.com/scienthoon/jev-ood-calibration)（同上索引）——在三个公开集上概率接近校准，但在**未见过的优先级规则**上过度自信，且 Boolean 问题表现出**相反方向的误差**。
- **动作门控**：[111 例 agent 动作门控研究](https://github.com/ghubnab99/jev-enterprise-decision-fabric)（同上索引）——Jev 与人工标签一致 100 例、Claude 102 例；**两者各有 1 例"不安全却放行"**；"合同与策略映射"是双方最大的单一错误来源。

> 索引来源说明：[awesome-typesafe](https://github.com/AbdelStark/awesome-typesafe) 是一份社区维护的 Jev 生态清单，上文 §5.5 各项均由其条目转述并指向各自仓库；我在本次调研中**只逐字抓取并核对了钓鱼基准与 JevBench 的原始结果文件**，其余条目属于"索引转述 + 指向原始仓库"，引用前建议再打开对应仓库核对。

---

## 六、"45% faster" 的具体语境（重要：不可引用）

**结论：这不是 TypeSafe 的官方数字，也不是任何受控基准的结果，而是一条匿名用户的自述轶事，且本调研未能定位其原始出处。**

证据链：

1. **唯一的"像样"来源是 AINews 的每日摘要**：[latent.space / AINews](https://www.latent.space/p/ainews-jev-a-system-one-model-that) 写道："One limited benchmark in a stock-analysis pipeline found the model completed the workflow about **45% faster** and produced a **correct overall result** but **missed some details**"。注意该文自己的用词是 "One limited benchmark" 与 "a stock-analysis pipeline"，没有给出作者、数据、代码或链接。
2. **同一说法在聚合站上以更原始的口吻出现**：搜索索引显示 r/apexiqsummary 收录的文本为"…stocks analysis and it did it **45% faster**, and **complited it correctly**, but **missed some details that ideally it should report**. It's faster for sure, but…"（含拼写错误，符合论坛随手评论特征）。我尝试通过 Reddit JSON API 与网页抓取访问该 subreddit 与相关帖子（r/PiCodingAgent、r/singularity），**均被 Reddit 的人机验证拦截**，无法取得原帖与作者。
3. **我在 HN 上做了穷尽式核对**：抓取 HN 发布帖 `id=49717558`（标题 "Introducing System One Models and Jev"，1945 分、172 个顶层子节点）的**全部 509 条评论**，搜索 "45%"、"45 %"、"stock"，**零命中**；另用 Algolia 评论检索多组查询词（"Jev stock analysis"、"Jev 45% faster"、"Jev TypeSafe stock analysis pipeline"、"45% faster Jev"）逐一检查含 "45" 的评论与 story，**均与该轶事无关**。
4. **一条易混淆的干扰项**：另有 Facebook 用户帖称 "It's 3x DeepSeek cost with equal intelligence and about 45% faster"（[链接](https://www.facebook.com/jhunter101/posts/so-ive-been-using-jev-by-type-safe-ai-for-4-hours-tonight-ive-ran-about-300000-t/4097315900569291/)），这是另一个人的另一段主观比较，数字同样无方法学。

**因此**："45% faster" 在报告里只能作为"网络上流传的用户轶事、原始出处不可考"处理；**不得**与 LiteLLM 的 5.43x、Every 的 25x 等并列成"实测数据"。

另外可顺带澄清一条常见误读：有 YouTube 视频标题称"An independent test on real work measured about 25×"（[TypeSafe Jev: A New Kind of Model, or Just Hype?](https://www.youtube.com/watch?v=hfi4ad8OA3Y)）——这个 25x 指的就是 §4.6 中 Every/Dan Shipper 的 0.35s vs 8.83s，**不是**新的独立测试；我未逐帧核对视频内容，仅由标题与数字一致性推断。

---

## 七、代码 / 请求示例（来自已抓取原文）

### 7.1 一次请求里问多个类型化问题（钓鱼基准的 9 问设置）

来自 [jev-phishing-bench README](https://github.com/anisselbd/jev-phishing-bench)（`state` = 邮件对象 JSON，一问一次调用）：

| id | 类型 | 问题 |
| --- | --- | --- |
| `verdict` | choice | phishing 或 legitimate（措辞镜像数据集作者的 balanced prompt） |
| `is_phishing` | noul | 同一问题的是非概率 |
| `sig_domain_mismatch` | noul | 发件域与链接域不一致 |
| `sig_free_hosting` | noul | 链接指向短链或免费托管平台 |
| `sig_lure` | noul | 要求登录、验证账户或打开文档 |
| `sig_urgency` | noul | 施压、期限或威胁 |
| `sig_generic_sender` | noul | 网络邮箱地址却冒充机构 |
| `verdict_alt_click` | choice | "Should the user click the link?" |
| `verdict_alt_minimal` | choice | "Classify this email."（仅选项名） |

关键教训（作者原文）：**"It is not 'a classifier you query once' — it is a cheap, calibrated signal generator you query broadly, because the marginal cost of another question is close to zero. Ask ten questions and do the combining yourself."**

### 7.2 Vercel AI SDK 的 evaluate 调用形态

来自 [Vercel changelog](https://vercel.com/changelog/typesafe-ai-jev-now-available-on-ai-gateway)（原文片段，示例问答以下略）：

```ts
import { experimental_evaluate as evaluate } from 'ai';

const result = await evaluate({
  model: 'typesafe-ai/jev',
  state: 'The support agent issued a full refund to the customer.',
  questions: { /* 命名决策 → 类型化问题 */ },
});
```

### 7.3 LiteLLM 路由配置

见 §4.1 的 YAML 片段。

---

## 八、不确定与未证实之处

1. **"45% faster" 的原始出处未定位**（§6）。Reddit 被人机验证拦截；HN 509 条评论零命中。**不要引用这个数字。**
2. **193.6x / 444.6x 无法复算**（§3.3）。可能是官方 dashboard 上某个特定工作流的配对数字，但公开页面无法还原；kingy.ai 独立得出同一观察。
3. **官方 67.8% 的"真值"不存在**：它是与 Astra+Fable 平均标签的吻合度；**我未找到任何人工标注的、覆盖相同四个工作流的独立复核**，所以"Jev 与前沿模型相当"这一说法目前**只有厂商口径支持**。
4. **Jev 的概率校准缺乏与厂商定位相称的独立证据**：唯一直接测 ECE 的第三方实验显示 Jev 的 ECE(0.154) 比 Haiku 4.5(0.097) 更差（钓鱼任务）；Jevals 明说每类问题只用一个数据集、校准"已知会随任务变化"；Maio 指出校准方法学未公开。**目前不能把"概率可靠、可按阈值分支"当成已验证前提。**
5. **延迟数字不可跨来源直接比较**：TypeSafe（西海岸笔记本）、LiteLLM（分类环节、含本地 relay）、Jevals（UTC+3，中位 0.44–0.48s）、Vercel（中位 132–272ms）、Jagent（服务器头 70–81ms）口径各不相同。**唯一可安全引用的是"绝对量级：百毫秒级"与"相对量级：同任务下比 Haiku 快数倍"。**
6. **`jev-latest` 的版本漂移是复现性风险**：LiteLLM 先解析到 jev-1.13.0 再固定，并明说未来解析可能得到不同模型；LangChain 实验元数据里**根本没有 Jev 版本**；Jevals 也只能说 Gateway 未报告版本而 TypeSafe 只列一个。多个独立实验都在 **jev-1.13.0** 上，跨版本外推需谨慎。
7. **并行提问的证据**：官方声称 + Jagent 的服务器时间头实测（1 问 70ms / 5 问 74ms）+ Every 的 37 文档×21 问 <0.7s，方向一致；但我**没有找到一份把"问题数量 vs 延迟"画成曲线的受控实验**。
8. **"输出 token 免费"有一条第三方实证**（钓鱼基准：4.56M token 被计费 $0.15，与仅输入计费一致），但只有一次、单一来源，且未核对提供方发票。
9. **Every、LangChain、Vercel、Jagent 都有潜在利益关系或未披露关系**（Every：TypeSafe 曾在 AIE 预发布；LangChain：与 TypeSafe 联合直播；Vercel：把 Jev 上架自家 Gateway 并称可能设为默认；Jagent：卖 Jev credits）。**上文已逐条标注**，引用时请保留标注。
10. **我未做任何一手实测**：本专题全部为公开材料核对（含对官方与第三方页面的算术复核）。§4.5 的统计结论、§4.3 的置信区间均为**转述原作者的统计**，我没有重跑其数据。

---

## 九、对本项目（Rust Agent Harness）Jev 定位的启示

1. **把"官方乘数"从需求里删掉，改用保守量级做容量与成本模型。**
   可安全引用的量级：单次分类**约 3e-5 美元**、单次判定**毫秒到百毫秒级**（LiteLLM：JEV 均值 $0.000032/次、p50 126.81ms；Every：0.7 秒内 777 个判断、约 1/4 美分）。不要用 193.6x/444.6x 或"45% faster"作为架构前提。**我们自己的收益上界，更可能来自"把 N 次 LLM 调用换成 N 次 Jev 判定 + Rust 侧组合"，而不是来自换模型。**

2. **在 Rust 侧把"分解 + 组合"做成第一等能力，而不是让 Jev 一锤定音。**
   最有说服力的独立证据不是 Jev 的单次判定准（它常常不赢），而是**一次调用并排问多个正交问题、由确定性代码组合**这件事本身有效：钓鱼基准里单次判定 62.6%，改成 5 信号 + 逻辑回归后 95.1%（且 ECE 从 0.154 降到 0.027）；官方自己的评测也显示**同样的策略拆成工作流后，连 LLM 都变准了**（Luna 51.9% → 66.8%）。
   → 因此类型系统应把 `Choice/Score/Noul` 作为**可组合的信号**建模（概率是一等数据、不提前塌缩成 bool），阈值、权重、分支与副作用全部留在 Rust 代码里；**Jev 不拥有策略**。

3. **把"弃权/无匹配"与"阈值必须自校准"写进接口契约，并假定分布会漂移。**
   独立证据显示这是最危险的失败面：KoBBQ 上**有 unknown 选项时 95% 选弃权**，移除该选项后 **79% 落入刻板印象答案**——即"选项里没有正确答案时，概率仍会落在某个选项上"；jev-certify 的域外比例上升让范围门超标 3.6 倍；排序研究中批处理 40 行就改变了通过的判定门。同时 Jev 的概率校准尚未被独立确证（钓鱼任务 ECE 更差于 Haiku）。
   → Rust 侧应内置显式的 `Unknown/InsufficientEvidence/Escalate` 型别与"低置信度升级到 LLM 或人工"的路径（Jevals 的实测支持这条路：Jev 在 95% 准确率门槛下可独自承接 86% 的 noul 决策），并要求每个上线阈值都来自**自有标注数据的留出集**，而不是照抄厂商数字。
