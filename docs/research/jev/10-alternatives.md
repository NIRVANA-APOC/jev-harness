# 专题 10：竞品与替代方案对比

> 调研范围：LLM-as-judge、小模型分类器（Haiku / GPT-mini 档）、embedding + 分类头、guardrail 模型（Llama Guard 等）、语义路由库，以及 Jev 的定位、差异化卖点与短板。
> 调研日期：2026-09-22。所有数字均标注来源 URL 与语境；**官方口径**与**第三方实测/评论**分开标注，本文推算另标。
> 声明：本文件只做资料整理。Jev 处于 2026-09 早期访问阶段，下列多数对比来自一次发布后数日内的评测，样本普遍很小，不可当作定论。

---

## 一、结论要点

1. **Jev 的对手不是"另一个 LLM"，而是"你已经能用的最便宜的那条判定路径"**：规则/正则 → 本地分类器（embedding+分类头 / 微调编码器）→ 语义路由 → guardrail 专用模型 → 小模型 LLM judge → 前沿 LLM judge。Jev 插在"免标注 + 需要概率 + 结构化输出"这个空档里，这个空档在它之前确实只能靠 LLM 填。
2. **速度与成本优势是量级性的，但精度不是**。官方 workflow evals（4 个 workflow 等权平均）上 Jev 为 **67.8% / $0.0004 / 0.4s**，最强对照 Sol 为 **74.1% / $0.0836 / 23.3s**、Opus 5 为 73.1% / $0.1761 / 37.8s——**便宜 1–2 个数量级、快 1–2 个数量级，但精度落后约 5–6 个百分点**；发票场景差距最大（Jev 61.8% vs Sol 79.1%，差 17pp）。（【官方】dashboard 原文，见 §3.2.0）
3. **对照小模型档（Haiku 4.5 / gpt-5.6-luna）时，Jev 的优势是"精度 + 成本"双重，而不只是成本**：官方同一任务上 Haiku 4.5 只有 **53.6%**（比 Jev 低 14.2pp）且每案例 $0.0195（**贵约 49 倍**），Luna 66.8% / $0.0033（贵约 8 倍）。但如果只看**最便宜档的单价推算**（$0.20/$1.20 per MTok），Jev 的单次判定成本优势约 **5–18 倍**，而不是 400 倍——400 倍口径来自官方"前沿模型 $0.20–$10 / MTok"区间的上限。（【官方】dashboard + 【本文推算】，见 §3.2.0 与 §3.4）
4. **有标注数据时，传统分类器可以打平甚至超过 Jev**：一个独立复现实验里，Jev 零样本在 18,514 封垃圾邮件上 98.33%（详细判据），TF-IDF 逻辑回归（约 14,800 条标注）98.39%，统计上打平；三分类主集上 Jev 98.64% vs 回归 98.87%。**Jev 的真正卖点是"不用标注 + 自带概率"，不是"精度更高"。**（第三方开源复现，见 §3.2.3）
5. **Jev 的"不会幻觉"是类型安全声明，不是语义正确性声明**：它跳不出你给的 schema，但 schema 内仍可能给出错误的 Choice；Arize 直接用"over-reach（过度宣传）"描述这个措辞。（第三方评论，见 §3.2.5）
6. **Jev 三个硬短板**：(a) 不产出解释，审计与自动诊断必须另接生成模型；(b) **不能弃权**——强制二分时它会挑"最不坏的答案"而不是说不知道，需要代码自己设计逃生通道；(c) 官方文档承认 **context rot**，state 越大越不准。（第三方整理自官方文档，见 §3.6）
7. **guardrail 这条线 Jev 打不动"免费"**：OpenAI Moderation endpoint 官方免费、第三方测 50–100ms；Llama Guard 4 官方英文输出过滤 recall 69% / FPR 11%，第三方实测 8B 档约 800ms（A100）。内容安全类固定 taxonomy 的活，专用 guardrail 模型仍有成本或合规优势。（见 §3.5）
8. **语义路由是"和 Jev 抢活"而不是"替代 Jev"的技术线**，而且**已经有 Rust 实现可参考**：vLLM Semantic Router 的分类核就是 Rust + Candle，论文报告在 MMLU-Pro 上准确率 +10.24pp、延迟 −47.1%、token −48.5%。（见 §3.7）
9. **官方数据里对本项目架构最重要的一条，不是"Jev 快"，而是"workflow 优于 prompt"**：官方在四个任务上同时测了"分解成 Noul/Choice/Score 原子问题 + 代码决策"与"同一政策写成一段 prompt 直接问"，结论是**每个模型在 workflow 下都更准、更便宜、更快**（Opus 5 73.1% vs 64.8%；Sol 74.1% vs 63.4%；Haiku 4.5 53.6% vs **18.1%**）。这条结论与厂商立场无关，是可直接用于 Harness 设计的结构性证据。（【官方】dashboard 原文，见 §3.2.0）

---

## 二、总览对比表

标注说明：**【官方】**= 厂商自己的文档/博客；**【第三方】**= 独立评测或评论；**【本文推算】**= 用公开价目表算出来的，不是任何来源的原始数字。

| 方案 | 需要标注数据 | 单次成本 | 延迟 | 能否给出可行动的概率 | 能否给解释 | 主要出处 |
|---|---|---|---|---|---|---|
| 规则 / 正则 | 否 | ≈0 | <1ms | 否（布尔） | 否 | 【第三方】premai 分层表 |
| embedding + 逻辑回归 | **是**（数百–数千条） | 一次 embedding 调用，$0.02–0.12 / MTok | 数十 ms | 是 | 否 | 【第三方】Arize 六路对照表 |
| 微调编码器（ModernBERT 类） | **是** | GPU 时间，单卡约 76,000 tok/s | 个位数 ms | 是（需温度标定） | 否 | 【第三方】Arize 表；vLLM 论文 |
| cross-encoder 重排 | 否 | 按"每次检索最多 100 文档"计价 | 数十–数百 ms | 仅相关性分 | 否 | 【第三方】Arize 表 |
| 专用 guardrail（Llama Guard 系） | 否（用出厂 taxonomy） | 自托管 GPU 成本（第三方估每月 100 万次约 $800@A100）；**OpenAI Moderation 免费** | Llama Guard 8B 约 800ms(A100)；Moderation 50–100ms | 部分（分档分 + flag） | 否 | 【官方】Llama Guard 4 模型卡、OpenAI 文档；【第三方】premai |
| 语义路由（embedding/分类器路由） | 少量 utterance 或偏好数据 | 近零（本地分类） | 个位数–数十 ms | 路由置信度 | 否 | 【官方】RouteLLM、vLLM Semantic Router 论文 |
| 小模型 LLM judge（Haiku / Luna 档） | 否 | 单次推算 $0.00046–$0.00225；**官方同任务每案例 $0.0033（luna）/ $0.0195（haiku 4.5）** | 秒级（带推理更久） | 口头置信，常过度自信 | **是** | 【官方】Claude/OpenAI 价目表 + TypeSafe dashboard；【第三方】Arize、JudgeBench 系论文 |
| 前沿 LLM judge | 否 | 单次推算 $0.009–0.0225；**官方同任务每案例 $0.0836（sol）/ $0.1761（opus 5）** | 10–38s | 口头置信，常过度自信 | **是** | 【官方】TypeSafe workflow evals；【第三方】Arize |
| **Jev** | 否（零样本，写判据即可） | $0.042 / MTok 输入，输出免费；实测 $0.00035 / 次；官方同任务每案例 $0.0004 | 70–500ms 官方口径，实测 0.44–0.59s | **是，且是训练目标** | **否** | 【官方】TypeSafe 文档/博客/workflow evals；【第三方】LangChain、NearHere、bitnovus |

---

## 三、关键事实与数据（逐条附 URL）

### 3.1 Jev 官方口径（务必标注为厂商自评）

| 事实 | 数字 | 语境 | 出处 |
|---|---|---|---|
| 速度 | 端到端 **70ms–500ms**；前沿 LLM 为 **3–329 秒**；同等"System One 形状"的查询上快 40×–200× | 官方博客，自述评测"一般在我们西海岸的笔记本上跑" | 【官方】https://typesafe.ai/blog/introducing-system-one-models-and-jev |
| 成本 | 输入 **$0.042 / MTok**（=$42/Btok），**输出免费**（"too cheap to meter"）；对比对象输入价 $0.20–$10 / MTok，输出约为输入 5 倍 | 同上；官方同时声明"**我们无法证明这不是补贴**" | 同上 |
| 首页头条 | **193.6× 更快、444.6× 更便宜** | 来自官方 4 个 workflow evals；官方自述"我们预期这是真实收益的**上限**"，且 workflow 由自家 model capabilities 团队构建、可能存在偏置 | 同上 |
| 模型与限额 | Jev 1.13（`jev-1.13.0`）；限流 250,000 tok/s、1,200 req/min；**64k** tokens/请求（state + 全部问题），其中 **32k** 用于 state + 最长单问题；**仅文本输入**（无图像/音频/视频）；不按客户数据微调 | 官方模型页 | 【官方】https://docs.typesafe.ai/models |
| Noul 语义 | yes/no 概率即答案与不确定度**合二为一**；**没有独立的 confidence 字段**（Choice/Score 才有） | 官方原语文档 | 【官方】https://docs.typesafe.ai/primitives/noul |
| 关键声明 | "type-safe"是数学保证：模型不可能产生类型错误；官方在图中给 LLM 的 hallucination/type-error 数值来自 OpenRouter，且自认有偏 | 官方博客的 "Nuance" 段 | 【官方】同 launch 博客 |
| 定位用语 | "frontier-intelligence function call：非结构化 state 进，类型化概率决策出"；用例明确包含 classify / route / score / extract / **guardrail / jailbreak 检测** | 同上 | 同上 |

**官方评测方法（重要限制）**：4 个 workflow、参照答案是 **GPT-6 Astra 与 Fable 5.1 的平均**（即模型互评而非人工真值）；所有模型共用同一 workflow；LLM 对照通过官方的 "System One LLM wrapper" 强制输出结构化决策（官方说这最准，但这会**更慢更贵**）。官方自陈"参照偏向 OpenAI/Anthropic，可能低估了我们和 DeepSeek 的相对表现"。

### 3.2 Jev 的实测与评论（官方 dashboard 一手 + 第三方）

#### 3.2.0 官方 workflow evals 全表（一手，直接抓取自官方站点）【官方】

来源：https://evals.typesafe.ai/ （2026-09-22 抓取原文；同批抓取文本另存于 `/tmp/jev-research/evals_text.txt`，供交叉核对）

**评测设定（官方原话要点）**：4 个 workflow（Security Incidents / Agent Trace Observability / Invoice Processing / Customer Service），**每个数据点是一个模型配置在 4 个 workflow 上等权平均后的准确率、成本、时间**；**参照标签 = GPT-6 Astra 与 Claude Fable 5.1 在 high thinking 下对 harness 中每个问题的答案的平均**；其余模型一律使用**各家默认推理设置**；官方明确"我们假设 harness 本身是正确的，不去争论 harness 与标签的正确性"，并称"**前沿模型里没有一个既更便宜又更准**"。

**汇总表（每案例成本 / 每案例耗时，四个 workflow 等权）**：

| 模型（默认推理档） | workflow 准确率 | 每案例成本 | 每案例耗时 | prompt 模式准确率 |
|---|---|---|---|---|
| haiku 4.5 | 53.6% | $0.0195 | 12.5s | **18.1%** |
| luna | 66.8% | $0.0033 | 12.9s | 51.9% |
| terra | 67.9% | $0.0304 | 10.1s | 61.6% |
| **Jev** | **67.8%** | **$0.0004** | **0.4s** | —（Jev 无 prompt 模式） |
| sonnet 5 | 67.8% | $0.1174 | 78.1s | 60.4% |
| DS v4 flash | 64.4% | $0.0059 | 51.9s | 59.3% |
| DS v4 pro | 65.5% | $0.0413 | 86.5s | 59.7% |
| opus 5 | 73.1% | $0.1761 | 37.8s | 64.8% |
| sol | 74.1% | $0.0836 | 23.3s | 63.4% |

**逐 workflow 的 Jev 数据（同一来源）**：

| workflow | 案例数 | Jev 准确率 / 成本 / 延迟 | 该 workflow 最强对照 |
|---|---|---|---|
| Security Incidents | 240 | 61.7% / $0.0001 / 0.3s | Opus 5 66.2% / $0.0574 / 15.1s |
| Agent Trace Observability | 117 | 71.6% / $0.0003 / 0.5s | Sol 76.6% / $0.0575 / 40.3s（Luna 76.1% / $0.0025 / 14.5s） |
| Invoice Processing | 150 | 61.8% / $0.0011 / 0.5s | Sol 79.1% / $0.2152 / 34.3s |
| Customer Service | 204 | 76.0% / $0.0001 / 0.4s | Sol 78.3% / $0.0323 / 10.1s |

**本专题最该记住的一条官方结论**：官方原话是"**在四个示例任务上平均，每个模型在 workflow（分解成 Noul/Choice/Score 原子问题 + 代码决策）下都比用同一政策写成一段 prompt 时更准、更便宜、更快**"。数据佐证：Opus 5 73.1%（workflow）vs 64.8%（prompt）；Sol 74.1% vs 63.4%；Haiku 4.5 53.6% vs **18.1%**。

> 方法论提醒（写进最终文档时必须保留）：该 dashboard 的"准确率"是**与模型互评参照的一致率**，不是与人工真值的一致率；每案例成本/耗时覆盖的是**整个 workflow 的多次模型调用**，不是单次 API 调用，所以不能直接与 §3.4 的单次调用推算混用。

#### 3.2.1 LangChain：Jev vs LLM judge（评测场景）【第三方实测】

来源：https://www.langchain.com/blog/jev-agent-evals-langsmith

- 设计：Deep Agents 构建 weather agent，取 **5 条固定轨迹**存入 LangSmith dataset，每条**重复 100 次** = **500 次判定**；人类给同一 rubric 打标作为 oracle；对照 GPT-5.6 Luna、GPT-5.6 Terra、Claude Sonnet 4.6。
- 二分类与人类一致率：**Jev 500/500 = 100%**；Terra 99.8%；Luna 96.4%；Claude 80.0%。
- 连续分重复性（每 case 100 次得分的均方差）：**Jev 0.0000149**；Claude 高 92×；Luna 高 433×；Terra 高 913×。
- 成本/延迟：**Jev $0.00035 / 次、平均 0.44s**；Claude 整场实验花费 **$28.17**（本文推算 ≈$0.056 / 次）。
- LangChain 自陈限制：只有 5 条轨迹、未设 temperature/top-p/seed/max tokens（用各家默认）、**Jev 服务版本在实验元数据里不可得**、不保证可外推到其他 agent 与生产环境；"低方差≠正确，一个稳定但错判的 evaluator 会规模化地产生错误反馈"。
- 复现信息：LangSmith 0.12.6、Deep Agents 0.7.15、langchain-typesafe 0.0.1a2。

二次转述与解读（用于交叉验证，非一手）：https://superpowerdaily.com/posts/langchain-publishes-jev-test-matching-human-labels-on-500-agent-decisions （同一组数字，并强调"5 条固定 weather 轨迹，不是生产测试"）。

#### 3.2.2 NearHere：Jev vs Gemini 3.5 Flash-Lite vs Mistral Small 4【第三方实测】

来源：https://nearhere.events/blog/typesafe-jev-mistral-gemini-event-validation （2026-09-16，英国本地活动站点）

| 指标（主测试集 50 条 listing） | Mistral Small 4 | Gemini 3.5 Flash-Lite | TypeSafe Jev |
|---|---|---|---|
| 准确率 | 84% (42/50) | 86% (43/50) | **96% (48/50)** |
| 误拒有效活动 | 5/13 | 1/13 | **0/13** |
| 误放行应排除项 | 3/37 | 6/37 | 2/37 |
| 平均响应时间 | 2.90s | 3.40s | **0.59s** |
| 每 1,000 次决策成本 | $0.370 | $2.496 | **$0.043** |

- 21 条追加样本（非盲、仅 2 条正例）：Jev 19/21、Gemini 20/21、Mistral 19/21；Jev 中位 0.58s，输入 21,528 tokens / 输出 1,775 tokens，21 条约 $0.000904。
- 对照模型在"高推理"设置下每条约 **433–910 个输出 token**，这也是 Arize 引用"910 vs 85"的来源。
- 作者自陈限制：每个模型单独调 prompt、开发集参与了配置选择、期望标签由助手写、**不是独立盲测**、样本小。

#### 3.2.3 bitnovus 开源复现：Jev 零样本 vs TF-IDF 逻辑回归（垃圾邮件）【第三方实测，开源可复现】

来源：https://github.com/bitnovus/jev-spam-eval

| 任务 | Jev（`jev-1.13.0`，零样本） | 监督基线（TF-IDF + 逻辑回归） |
|---|---|---|
| 二分类 18,514 封邮件（详细判据） | 98.33%，AUC 0.9984 | 98.39%，AUC 0.9986 |
| 三分类主集 5,733 封（富上下文 + 判据措辞优化） | **98.64%** | **98.87%**（约 4,600 标注/折，5 折） |
| "fresh" 集 3,300 封（同期不同邮件） | 95.76% | 96.33% |
| 2024–25 新钓鱼邮件 853 封（仅 recall） | **95.31%** | 75.26% |
| 两者 50/50 平均（二分类） | **99.22%**（错误从 299 降到 144） | — |

- 上下文富化（加 Reply-To、链接真实域名、附件元数据）把 Jev 准确率从 **93.62% → 97.98%**，钓鱼 recall 从 85.71% → 98.43%；监督基线同样富化只涨 0.12pp。
- 学习曲线估计：要达到 Jev 的 98.3%（email-dataset 详细判据）约需 **10,000 条标注**；Ling-Spam 上只需约 200 条。
- 工程数据：19,772 次请求、3,206 万输入 token、并发 16 下约 **252 秒**跑完，按 $0.042/MTok 约 **$1.35**。
- 作者的诚实声明：判据措辞被标注错误"污染"过，所以是 zero-shot 推理而非"全程无监督"；公开语料无法保证不在预训练里；单一模型版本、基本单次运行。

#### 3.2.4 Every：777 个判断 0.7 秒（小样本独立测试）

**【第三方，本文未能抓取原文（页面需 JS/订阅），数字来自 3 处转述】**
- 转述来源：https://kingy.ai/blog/typesafe-jev-review-the-ai-model-that-doesnt-generate-text/ 、https://arize.com/blog/typesafe-jev-llm-judge/ 、https://www.firecrawl.dev/blog/what-is-jev
- 内容：37 篇文档（27 篇真实文章 + 10 篇刻意 AI 化的对照）×21 个问题 = **777 个判断 < 0.7 秒，成本约 0.25 美分**；12 段人工植入缺陷的文本中 Jev 抓到 **6/7**，Fable 5.1 全中，估计快约 **25×**、便宜约 **580×**（中位 0.35s vs 8.83s）。
- 原文 URL：https://every.to/also-true-for-humans/mini-vibe-check-typesafe-s-jev-judged-everything-i-ve-written-in-0-7-seconds

#### 3.2.5 Kingy.ai 的批评性复核（对官方 dashboard 的再解读）【第三方评论】

来源：https://kingy.ai/blog/typesafe-jev-review-the-ai-model-that-doesnt-generate-text/ （2026-09-21）

- 汇总官方 workflow dashboard（**711 案例 / 4 个 workflow**）：

| workflow | N | Jev 准确率 / 成本 / 延迟 | 最好可见对照 |
|---|---|---|---|
| Security incidents | 240 | 61.7% / $0.0001 / 0.3s | Opus 66.2% / $0.0574 / 15.1s |
| Agent trace observability | 117 | 71.6% / $0.0003 / 0.5s | Sol 76.6% / $0.0575 / 40.3s |
| Invoice processing | 150 | 61.8% / $0.0011 / 0.5s | **Sol 79.1% / $0.2152 / 34.3s** |
| Customer service | 204 | 76.0% / $0.0001 / 0.4s | Sol 78.3% / $0.0323 / 10.1s |
| **汇总** | **711** | **67.8% / $0.0004 / 0.4s** | Sol 74.1% / $0.0836 / 23.3s |

- 该文的核心批评：(1) 参照是模型互评，不是真值；(2) "零幻觉"只是类型安全，**语义上仍可给出错误答案**；(3) **校准未被独立验证**，官方未发布校准指标；(4) 首页 193.6×/444.6× 无法从四舍五入后的表格复算；(5) 作者明确声明自己没有 API key，**没有做实测**（诚实边界）。
- 交叉来源：Arize 对同一 dashboard 的记述为"Jev 68% / $0.0004 / 0.4s；Terra 68% / $0.03 / 10s；**Opus 5 73% / $0.18 / 38s**"（https://arize.com/blog/typesafe-jev-llm-judge/ ）。**现已用官方原文核对（§3.2.0）：该 dashboard 上 Sol 74.1% 与 Opus 5 73.1% 同为"最强对照"，两处只是各取其一，并非数据漂移**；但官方页会更新，引用时仍应写明抓取日期。

#### 3.2.6 Good Start Labs 的 6,003 次 rubric 评测

**【第三方，仅见转述，未找到原始报告】** 来源：https://langfuse.com/blog/2026-09-18-using-typesafes-jev-for-evals
- Jev 与 Claude Fable 5.1 的判定一致率 **91.5%**；成本按"每百万条被评分答案"计：**Jev $160** vs Fable 5.1 **$33,000**、GPT-5.6 Luna **$400**、Gemini 3.8 Flash **$1,600**；开源模型 DeepSeek V4.1 Flash **$260**、一致率 **93.5%**（比 Jev 高 2 个点、贵 $100）。

### 3.3 LLM-as-judge 这条路线的能力与代价

| 事实 | 数字 | 语境 | 出处 |
|---|---|---|---|
| 强 LLM judge 与人类一致率 | GPT-4 类 judge **>80%**，与人类-人类一致率相当 | MT-Bench / Chatbot Arena 论文（2023，被引 1.3 万+） | 【论文】https://arxiv.org/abs/2306.05685 |
| 但"换模型/换档位"差异巨大 | 文档相关性任务上 GPT-4 81%、Claude 3.5 Sonnet 77%、**GPT-3.5 Turbo 仅 55%**（对"不相关"类的 recall 只有 7%） | Arize 的实务评测（旧文，模型为 2024 年档） | 【第三方】https://arize.com/blog/choosing-the-best-llm-evaluation-model/ |
| 置信度不可信 | 2025 年研究系统性地识别出 **Overconfidence Phenomenon**：judge 的预测置信度显著高于其真实正确率，并提出 TH-Score 度量（Arize 转述为"14 个模型在 JudgeBench 上把预测聚在 90%–100% 置信，而准确率远低于此"） | 论文 2508.06225；Arize 的转述与本专题相关 | 【论文】https://arxiv.org/abs/2508.06225 ；【第三方转述】Arize 上文 |
| 自偏好偏置 | LLM evaluator 会给自己生成的文本更高分，且模型越能识别自己的输出，偏置越强 | Panickssery et al., NeurIPS 2024 | 【论文】https://arxiv.org/abs/2404.13076 |
| LLM judge 的位置偏置 | 成对/列表比较中存在系统性位置偏置（已有专门度量方法） | IJCNLP 2025 | 【论文】https://aclanthology.org/2025.ijcnlp-long.18.pdf |
| judge 的成本量级 | TypeSafe 自家 workflow 上，前沿模型 **$0.03–0.18 / 次、10–38 秒 / 次**；若要求解释，成本再加 | Arize 转述并用作六路对照的一行 | 【第三方】https://arize.com/blog/typesafe-jev-llm-judge/ |
| judge 退出/解释的价值 | LLM judge 的**解释**是 Jev 给不了的：可以把失败原因成批喂回 coding agent 自动改进；Jev 只给概率，方向性信号弱得多 | Arize 明确指出这是"替换 LLM judge 时失去的最大东西" | 【第三方】同上 |

### 3.4 小模型分类器（Haiku / GPT-mini 档）—— 官方价目表与推算

**官方价目表（每 1M tokens）**：

| 模型 | 输入 | 缓存输入 | 输出 | 出处 |
|---|---|---|---|---|
| Claude Haiku 4.5（官方标"Fastest"，200K 上下文） | **$1** | — | **$5** | 【官方】https://platform.claude.com/docs/en/models/haiku-4-5/overview |
| gpt-5.6-luna | **$0.20** | $0.02 | **$1.20** | 【官方】https://developers.openai.com/api/docs/pricing |
| gpt-5.6-terra | $2.00 | $0.20 | $12.00 | 同上 |
| gpt-5.6-sol | $4.00 | $0.40 | $20.00 | 同上 |
| gpt-6-astra | $10.00 | $1.00 | $50.00 | 同上 |
| **Jev 1.13** | **$0.042** | — | **免费** | 【官方】https://docs.typesafe.ai/models |

注：抓取当天（2026-09-22）官方价格页的 Standard 档最便宜模型是 `gpt-5.6-luna`（$0.20/$1.20），页面上**没有单列"GPT-mini"这一代**（早期 `gpt-5-mini` 系列在当前价目页已不出现）。因此"小模型档"在本专题一律指 luna / Haiku 4.5 这一类，不要用历史型号的旧价格代替。

**【本文推算】单次判定的成本对比**（假设 2,000 输入 token + 50 输出 token，纯算术，可自行验算）：

| 模型 | 单次成本 | 相对 Jev 的倍数 |
|---|---|---|
| Jev | $0.000084 | 1× |
| gpt-5.6-luna | $0.00046 | 5.5× |
| Claude Haiku 4.5 | $0.00225 | 26.8× |
| gpt-5.6-terra | $0.0046 | 54.8× |
| gpt-5.6-sol | $0.009 | 107× |
| gpt-6-astra | $0.0225 | 268× |

如果按 NearHere 实测的"高推理"输出量（每判断约 **900 输出 token**）重算，则 luna ≈ $0.00148（**17.6×**）、Haiku 4.5 ≈ $0.0065（**77×**）。**结论：对最便宜的小模型档，Jev 的优势是"个位数到两位数倍数"；对前沿模型才是"两个数量级"。**

**但"单价低"不等于"每案例便宜"——官方 dashboard 的实测（§3.2.0，四个 workflow 等权、含整个 harness 的多次调用）**：

| | 平均准确率 | 每案例成本 | 相对 Jev 的每案例成本 |
|---|---|---|---|
| **Jev** | 67.8% | **$0.0004** | 1× |
| luna | 66.8% | $0.0033 | 8.3× |
| haiku 4.5 | **53.6%** | **$0.0195** | **48.8×** |
| terra | 67.9% | $0.0304 | 76× |
| DS v4 flash | 64.4% | $0.0059 | 14.8× |
| sol | 74.1% | $0.0836 | 209× |
| opus 5 | 73.1% | $0.1761 | 440× |

即：**同等任务下，Haiku 4.5 比 Jev 贵约 49 倍、低 14.2 个百分点**；最接近"便宜小模型"定位的 luna 贵约 8 倍、低 1 个百分点。这比单纯看单价更有说服力，但仍要记住口径差异：dashboard 的成本是**每案例**（整个 workflow 多次调用），§3.4 上半部分是**单次调用**推算。

**小模型路线被低估的地方**：它们能同时给出**解释**，能处理多因素混合任务，且可直接换成同一家的更强模型；而它们的劣势是串行解码与输出 token 计费（Arize 引用的实测：一般模型每次 yes/no 要花约 **910 输出 token**，Jev 约 85 且不计费）。

### 3.5 guardrail 模型（Llama Guard / Moderation / 分层 guardrail）

| 事实 | 数字 | 语境 | 出处 |
|---|---|---|---|
| Llama Guard 4 官方性能 | 英文 **recall 69% / FPR 11% / F1 61%**；多语种 43%/3%/51%；单图 41%/9%/38%；多图 61%/9%/52%。相对 LG3 英文 F1 +8pp | 官方模型卡，**输出过滤**设定，S1–S13 类别等权平均 | 【官方】https://huggingface.co/meta-llama/Llama-Guard-4-12B |
| Llama Guard 4 定位 | 12B 多模态安全分类器，一个 GPU 可跑；官方明确说"不要拿它跟竞品比，因为安全策略不同" | 同上 | 同上 |
| 延迟分层 | 正则 <1ms；BERT 类分类器 20–100ms；**Llama Guard 3-8B ≈800ms（A100）**（文中另处写 500ms–2s）；NeMo 输入检查 150–400ms（需一次 LLM 调用）；**OpenAI Moderation API 50–100ms** | 第三方工程指南，作者自述在 4 vCPU/16GB/无 GPU（除非注明）上测 P50 | 【第三方】https://www.premai.io/blog/production-llm-guardrails-nemo-guardrails-ai-llama-guard-compared/ |
| 假阳性会累积 | 5 个 90% 准确率的 guard 串起来，**至少一个误报的概率 41%**；要压到 25% 以下，每层需 95%+ | 同上（组合数学，非实测） | 同上 |
| 月度成本量级（100 万次请求） | 正则 ~$20；LLM Guard 自托管 ~$150；Guardrails AI ~$200；**Llama Guard（A100 自托管）~$800**；**OpenAI Moderation API $200**；GPT-4 当 judge **$3,000–10,000** | 同上（估算表，未注明方法细节） | 同上 |
| OpenAI Moderation 免费 | `omni-moderation-latest` 支持文本与图像（≤20MB），**endpoint 免费使用**；返回 `flagged` 与 `category_scores`（0–1） | 官方文档 | 【官方】https://developers.openai.com/api/docs/guides/moderation |
| 需要谨慎引用的一组数字 | 该第三方文章另给出 ToxiGen（Llama Guard 3 P0.91/R0.88/F1 0.89）与 JailbreakBench（Llama Guard 3 检出 85%、FPR 15%）等表，**未注明来源与方法**，我只能标为"第三方汇总，待核" | — | 同上 |

**对本项目的直接含义**：如果目标是"通用内容安全 + 图像 + 多语种"，OpenAI Moderation（免费）与 Llama Guard 4 是更省钱的默认档；Jev 要在"自定义策略、原子判据、返回概率、可组合进业务逻辑"这些维度上竞争，而不是在固定 taxonomy 的基准分上竞争。

### 3.6 Jev 的短板（第三方整理 + 官方文档原文）

第三方（Langfuse，2026-09-18，https://langfuse.com/blog/2026-09-18-using-typesafes-jev-for-evals ）明确列出：

1. **不能弃权**："强制二分且没有 unknown/needs_review 选项时，Jev 会挑最不坏的答案，而不是说它不知道。"必须自己设计逃生通道。
2. **没有 rationale**：不给理由. 审计/面向客户的场景仍需生成模型在上一层。
3. **Context rot**：官方文档原话是 "Jev suffers from context rot"——state 里塞了与问题无关的材料就会掉准确率，这对长 agent trace 尤其尴尬。
4. **容量口径不一致**：官方模型页写 64k/请求（state+问题）、32k（state+最长单问题），而 **OpenRouter 上标 32K**——设计前要自行核实。**已交叉验证**：抓取 OpenRouter 的 Jev 1.13 页面确认其 context length 写的是 **32K / 32,000**（一手抓取文本见 `/tmp/jev-research/or_113.txt`），与官方口径不一致的事实成立。
5. **Noul 没有独立 confidence 字段**：读 `answer.confidence` 的通用代码会在二分题上炸。
6. **不能生成任何文本**："useless at other tasks"（原文措辞）。

其他来自第三方复核与官方文档的边界：
- **官方自陈的"jaggedness"页**（每版本列出 Jev 不擅长什么）提到它**按字面理解、不能做算术、把日期当文本**——**此条经 Langfuse 转述**，我按 `docs.typesafe.ai/models/jev-1.13-jaggedness` 抓取时返回 404，未能一手核对该页；
- Choice 上限 **255** 个选项（更高基数需要两阶段；【官方】launch 博客），Score 上限 **10 档**（Langfuse 转述）；
- **文本输入 only**（图像/音频/视频必须先转文本）——【官方】模型页；
- **英文最佳**："English is the primary training language and where accuracy is currently best. Other languages, including CJK scripts, are handled but not equally well"——【官方】模型页原文；
- `jev-latest` 别名会漂移，阈值调好的话要 pin 到 `jev-1.13.0`——【官方】模型页建议；
- **无权重、不可自托管**：所有账户共用同一权重，不按客户数据微调（对希望私有化部署或领域微调的团队是硬约束）——【官方】模型页原文；
- 官方未披露参数量、架构、硬件、训练数据、model card、SLA、p95/p99（【官方】文档确实没有这些区块，属"公开信息缺口"）。

### 3.7 语义路由（这一条与我们用 Rust 写 Harness 最相关）

| 方案 | 关键结果 | 语境 | 出处 |
|---|---|---|---|
| RouteLLM（LMSYS，开源） | MT Bench 上**省 85% 成本、保 95% GPT-4 质量**；MMLU 45%、GSM8K 35%；数据增强后用 **14% 的 GPT-4 调用**达到 95% GPT-4（比随机基线便宜 75%）；**比商业路由器便宜 40%+** | GPT-4 Turbo 强模型 vs Mixtral 8x7B 弱模型，公开偏好数据训练 | 【官方/论文】https://www.lmsys.org/blog/2024-07-01-routellm/ ；https://arxiv.org/abs/2406.18665 |
| vLLM Semantic Router | MMLU-Pro 上**准确率 +10.24pp（p<0.01）、延迟 −47.1%、token −48.5%**；知识密集域（商业/经济）提升 >20pp | Qwen3-30B-A3B on vLLM 0.10.1、TP=4、NVIDIA L4；**分类核是 Rust + HuggingFace Candle**（ModernBERT 微调，多任务：类别/PII/jailbreak），Envoy ext_proc 接入 | 【论文】https://arxiv.org/html/2510.08731v1 ；【官方博文】https://vllm.ai/blog/2025-10-27-semantic-router-modular |
| aurelio semantic-router | 用 embedding 相似度替代 LLM 生成式工具选择；支持 Cohere/OpenAI/HF/FastEmbed 编码器、多模态、可本地全离线（HFEncoder + LlamaCppLLM）、可训练阈值 | 无官方统一 benchmark 数字；README 有 "10ms 热线挑战" 之类第三方文章链接 | 【官方】https://github.com/aurelio-labs/semantic-router |
| FrugalGPT（背景） | 论文报告最高 **98% 成本下降**（跨基准，级联模型选择） | 被 vLLM 论文引用为相关工作 | 【论文，经 vLLM 论文转引】同上 |

**为什么重要**：语义路由证明了"用一个小分类器在链路前面拦掉大部分简单请求"这件事在 2024–2025 年就已经有可复现的收益；**而它的实现路径（Rust 分类核 + Candle + 多任务并行 + 本地部署）与本项目技术栈几乎重合**。Jev 在这条线上的差异不是"路由得更好"，而是"**免训练新意图、返回概率、且能和业务判据一起并行提问**"。

---

## 四、请求/响应与代码示例（来自公开文档，非我实测）

### 4.1 Jev：一次请求问三个问题（Langfuse 给出的完整形态）

来源：https://langfuse.com/blog/2026-09-18-using-typesafes-jev-for-evals

> 下面请求中 `"..."` 是我为节省篇幅做的省略标记，**不是 API 接受的占位符**；完整原始示例（含每个 Score/Choice 档位的真实文字）见上述 Langfuse 博客。响应块是原文的真实数值。

```json
{
  "model": "jev-latest",
  "state": { "task": "...", "tool_calls": "...", "final_output": "..." },
  "questions": {
    "needs_review": { "type": "noul", "instructions": "Does this run need a human to look at it?" },
    "severity": {
      "type": "score",
      "instructions": "How badly did this run go?",
      "criteria": ["Completed the task cleanly", "...", "...", "Took a destructive or unsafe action"]
    },
    "failure_mode": {
      "type": "choice",
      "instructions": "What went wrong, if anything?",
      "criteria": { "tool_error": "...", "missing_context": "...", "wrong_approach": "...", "user_abandoned": "...", "none": "..." }
    }
  }
}
```

响应（节选）：

```json
{
  "model": "jev-1.13.0",
  "answers": {
    "needs_review": { "type": "noul", "noul": 0.88 },
    "severity": { "type": "score", "score": 1.89, "confidence": 0.44,
                  "probabilities": { "0": 0.02, "1": 0.21, "2": 0.63, "3": 0.14 } },
    "failure_mode": { "type": "choice", "choice": "missing_context", "confidence": 0.51,
                      "probabilities": { "tool_error": 0.11, "missing_context": 0.58,
                                         "wrong_approach": 0.24, "user_abandoned": 0.05, "none": 0.02 } }
  },
  "usage": { "input_tokens": 1840, "output_tokens": 27 }
}
```

注意 `confidence: 0.51` 这个语义：**这不是"模型有多自信"，而是概率分布的集中程度**——0.58 的最高项与 0.24 的次高项接近，所以 confidence 低。业务代码应当同时读概率与 confidence，并按三路分派（高置信执行 / 中间带转人 / 低置信转 LLM）。

### 4.2 Jev 作为 agent 中间件（LangChain 集成）

来源：https://www.langchain.com/blog/building-a-harness-with-jev

```python
from langchain_typesafe import Noul, TypeSafeClassifier

classifier = TypeSafeClassifier()
response = classifier.invoke({
    "state": "The deploy failed twice and customers are seeing 500s. Can someone look now?",
    "questions": {"urgent": Noul(instructions="Does this need attention right now?")},
})
urgency = response.nouls["urgent"].noul
```

同一篇文章还给出两条对本项目直接可用的形态：`ModelRouterMiddleware`（Jev 按 criteria 在 fast/powerful 模型间路由）与 `AutoModeMiddleware(tools=["bash"])`（**用 Jev 在工具执行前拦截危险调用**）。原文点明："这类分类器步骤以前一直锁在闭源 harness 内部，现在有了便宜且性能足够的分类模型，可以把同样的模式用到所有 agent 上。"

### 4.3 LLM-as-judge 的等价写法（Langfuse 给的同任务 prompt）

同一博客对比了"用 LLM judge 判用户是否在反驳助手"与"用 Jev 的 Noul + true/false 判据"，指出 Jev 的写法**迫使你把判据写清楚**，代价是必须自己定义 true/false 的 `includes` 清单，且答案没有理由可读。

---

## 五、什么时候用 Jev，什么时候用别的

**优先顺序（本项目建议的决策树）**

1. **能用确定性代码判断的，两者都不用**（and 这就是官方文档的态度：state 里放数据，规则放代码）。
2. **候选答案是预先已知的有限集合 / 是/否 / 有序档位，且没有标注数据、需要概率门控、延迟敏感** → **Jev**。
3. **同类判定但你有几百到几千条标注、追求个位数毫秒、要自托管/可控** → **embedding + 分类头 / 微调编码器**（SetFit 类方法在 8 例/类即可接近全量微调，训练 30 秒、$0.025；见 https://huggingface.co/blog/setfit ）。
4. **判定的候选"意图"本身很稳定，你只需要把它分到某条链路上** → **语义路由**（RouteLLM / vLLM Semantic Router / aurelio），成本近零。
5. **内容安全、固定 taxonomy、要图像或多语种、要合规可解释的过滤** → **专用 guardrail**（OpenAI Moderation 免费、Llama Guard 4 自托管），**不要**拿 Jev 去替这一类。
6. **需要解释、需要多因素混合推理、需要生成、或者判定频率很低而单次价值很高** → **LLM judge（小模型档起步，不够再升级）**。
7. **混合架构（多数团队的实际最优解，也是 Arize 的建议）**：**Jev/分类器跑全量覆盖做监控，失败与不确定样本抽样送 LLM judge 拿解释**（https://arize.com/blog/typesafe-jev-llm-judge/ ）。

**不要用 Jev 的场景**：要写文本/代码/摘要；要给出理由；需要数学或日期运算；图像/音频输入；没有逃生通道的强制二选一；状态长而杂且无法裁剪；要靠领域微调才能达到精度要求的任务。

---

## 六、对本项目 Jev 定位的启示（3 条）

1. **抽象层按"决策"建模，而不是按"模型调用"建模。** Harness 里应有一个统一接口：输入 state + 类型化 questions，输出"带概率的类型化答案 + 用量"。Jev 是它的一个实现；LLM 走"结构化输出"实现（官方 workflow evals 用的 "System One LLM wrapper" 就是这个思路，且官方承认这条路更慢更贵）。这样 Jev 不可用、超限、或在不擅长的任务上时，可以在同一条链路上回退，而调用方代码不需要改。
2. **把 Jev 用在"答案已知 + 可分解为原子问题 + 有代码阈值门控"的环节，并强制设计逃生通道。** 依据：Jev 不能弃权（会挑最不坏答案）、无解释、有 context rot。因此本项目应约定三路分派：`P ≥ 高阈值 → 直接执行`；`中间带 → 转 LLM / 人工`；`P ≤ 低阈值 → 直接否决/丢弃`。同时对长上下文做裁剪，不要把整条 trace 无差别塞进 state。
3. **不要把 Jev 当成精度天花板，评测必须自建带真值的 holdout。** 官方口径的参照是 Astra + Fable 5.1 的模型互评，第三方汇总显示 Jev 比最强对照低约 6pp（发票场景低 17pp），而与最便宜的小模型档相比成本优势只有个位数到两位数倍数。所以定位应是"**免标注 + 概率校准 + 并行提问 + 无解析失败**的快速判定层"——差异化在工程属性而非绝对智能；**凡是关键判定，都要在本项目自己的带真值数据集上测过再上线**。

---

## 七、不确定与未证实之处

1. **Jev 的一切数字都来自 2026-09 发布的早期访问产品**，官方自评占主导；第三方实测样本都极小（5 条 agent 轨迹、50 条 listing、711 个 workflow 案例、1.5 万封邮件、6,003 次 rubric）。**没有任何一个是同任务同条件的严格对照实验。**
2. **官方 workflow dashboard 的"真值"是模型互评**（GPT-6 Astra + Claude Fable 5.1 high thinking 的平均），不是人工标注；且 workflow 由厂商自家能力团队构建，官方自认可能存在偏置。**我已通过同批抓取的官方原文核对全部数值（见 §3.2.0）**，kingy.ai 与 Arize 对"最强对照"的命名差异（Sol 74.1% vs Opus 5 73.1%）已确认为取材不同而非数据漂移；但该页会更新，引用时必须写明抓取日期。
3. **校准性没有独立验证**：官方未发布校准曲线或指标；bitnovus 仓库有分档校准分析（<0.1 → 0.1% spam；≥0.9 → 99.9%；0.5–0.6 档仅 38%；把 0.3–0.7 的 4.6% 转人工后其余达 99.5%），但那是单一数据集、单一模型版本，**不能当作通用校准保证**。
4. **未找到**：p95/p99 延迟、并发表现、限流命中率、重试率、版本漂移影响的公开数据；Jev 与"专门训练的现代分类器/重排器"在同一真值任务上的独立对比；官方 model card / 架构 / 参数 / 训练数据披露。
5. **本文未能抓到原文、依赖转述的数字**（引用时务必标二次来源）：DataCamp 文章（403）、Every 的原文（JS/订阅墙，数字经 kingy.ai、Arize、firecrawl 三处转述且互相一致）、Good Start Labs 的 6,003 次评测（只见 Langfuse 一处转述）。**TypeSafe 官方 workflow evals 的数值不属于这一类**——已由一手抓取核对（§3.2.0）。
6. **间接引用需降级使用**：premai 文章中的 ToxiGen / JailbreakBench 表格未注明来源与方法；Llama Guard 的 800ms 与月度成本也是第三方估算，我未实测（本机无 GPU 环境）。
7. **语义路由的数字都有明确条件**（特定模型、特定硬件、特定 benchmark），不是普适常数；RouteLLM 的结果基于 GPT-4 Turbo + Mixtral 的模型对，与 2026 年的模型组合不可直接平移。
8. **未找到针对本项目的"Jev vs 本地 Rust 分类核"对比**——这是一个明显空白：vLLM Semantic Router 已证明 Rust + Candle 的 ModernBERT 分类核可行且极快，本项目若要自建判定层，这是一条值得单独验证的替代路线。
9. **口径差异必须一直带着走**：官方 dashboard 的"每案例成本/耗时"覆盖整个 workflow 的多次调用，而 §3.4 的单次调用推算是按 2,000 输入 + 50 输出 token 假设的纯算术。二者**不可混用或相互加减**；LangChain 的 $0.00035/次 又是第三种口径（不同任务、不同 state 长度，反推约 8,300 输入 token/次）。
