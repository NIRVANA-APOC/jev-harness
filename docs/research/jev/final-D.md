# 报告片段 D：竞品与替代方案、Rust 集成要点、公司与依赖风险、社区争议与采用动态

> 撰写日期：2026-09-22。本片段由上游 4 份专题文件（`10-alternatives.md`、`11-rust-integration.md`、`12-company.md`、`13-community-debate.md`）忠实提炼、去重、按主题重组，**不新增任何上游未记载的事实**。
> 可信度前缀：**【官方】** = TypeSafe 官方文档 / 博客 / 定价页 / 状态页 / 法律条款（含官方新闻稿与官方 GitHub 组织）；**【第三方】** = 有署名的第三方实测、报道或社区讨论；**【未证实】** = 转述、二手、无法核对或口径不明。
> 引用约定：每个来源**首次出现时给全 URL**，重复引用时用短形式（如「同 evals」）。Jev 处于 2026-09 early access 阶段，多数对比来自发布后数日内的评测，**样本普遍很小，不可当定论**。

---

## 一、竞品与替代方案对比及选择准则

**要点**

- Jev 的对手不是「另一个 LLM」，而是**你已经能用的那条最便宜的判定路径**：规则/正则 → 本地分类器（embedding + 分类头 / 微调编码器）→ 语义路由 → guardrail 专用模型 → 小模型 LLM judge → 前沿 LLM judge。Jev 插在「**免标注 + 需要概率 + 结构化输出**」这个空档里——这个空档在它之前确实只能靠 LLM 填。【第三方/Official 混合，见下表】
- 速度与成本优势是**量级性的**，精度**不是**：官方 workflow evals 上 Jev 为 **67.8% / $0.0004 / 0.4s**，最强对照 Sol 为 **74.1% / $0.0836 / 23.3s**、Opus 5 为 73.1% / $0.1761 / 37.8s——便宜、快 1–2 个数量级，但**精度落后约 5–6pp**；发票场景差距最大（Jev 61.8% vs Sol 79.1%，差 17pp）。【官方】https://evals.typesafe.ai/
- 对照**小模型档**时 Jev 是「精度 + 成本」双重优势，而不只是成本：同一任务 Haiku 4.5 只有 **53.6%**（低 14.2pp）且每案例 $0.0195（**贵约 49 倍**），luna 66.8% / $0.0033（贵约 8 倍）。但若只看**最便宜档单价的推算**，优势只有约 5–18 倍，不是官方首页口径的 400 倍。【官方】同 evals；【本文推算，上游文件所出】
- **有标注数据时传统分类器可以打平甚至超过 Jev**：一个独立开源复现中，Jev 零样本在 18,514 封邮件上 98.33%，TF-IDF 逻辑回归（约 14,800 条标注）98.39%，统计上打平。**Jev 的真正卖点是「不用标注 + 自带概率」，不是「精度更高」。**【第三方】https://github.com/bitnovus/jev-spam-eval
- Jev 三个硬短板：**(a) 不产出解释**（审计/自动诊断必须另接生成模型）；**(b) 不能弃权**（强制二分时挑「最不坏的答案」而不是说不知道）；**(c) 官方承认 context rot**（state 越大越不准）。【第三方整理自官方文档】https://langfuse.com/blog/2026-09-18-using-typesafes-jev-for-evals
- 官方数据里对 Harness 架构最重要的一条**不是「Jev 快」，而是「workflow 优于 prompt」**：把政策分解成 Noul/Choice/Score 原子问题 + 代码决策，**每个被测量的模型都更准、更便宜、更快**（Opus 5 73.1% vs 64.8%；Sol 74.1% vs 63.4%；Haiku 4.5 53.6% vs **18.1%**）。这条与厂商立场无关，是可直接用于设计的结构性证据。【官方】同 evals。

### 1.1 总览对比表

| 方案 | 需要标注数据 | 单次成本 | 延迟 | 可行动概率 | 给解释 | 出处 |
|---|---|---|---|---|---|---|
| 规则 / 正则 | 否 | ≈0 | <1ms | 否（布尔） | 否 | 【第三方】premai 分层表 https://www.premai.io/blog/production-llm-guardrails-nemo-guardrails-ai-llama-guard-compared/ |
| embedding + 逻辑回归 | **是**（数百–数千条） | $0.02–0.12 / MTok | 数十 ms | 是 | 否 | 【第三方】Arize 六路对照 https://arize.com/blog/typesafe-jev-llm-judge/ |
| 微调编码器（ModernBERT 类） | **是** | GPU 时间（单卡约 76,000 tok/s） | 个位数 ms | 是（需温度标定） | 否 | 【第三方】同上；vLLM 论文 |
| cross-encoder 重排 | 否 | 按「每次检索最多 100 文档」计价 | 数十–数百 ms | 仅相关性分 | 否 | 【第三方】Arize 同上 |
| 专用 guardrail（Llama Guard 系） | 否（用出厂 taxonomy） | 自托管 GPU（第三方估 100 万次/月约 $800@A100）；**OpenAI Moderation 免费** | LG 8B 约 800ms（A100）；Moderation 50–100ms | 部分（分档分 + flag） | 否 | 【官方】https://huggingface.co/meta-llama/Llama-Guard-4-12B 、https://developers.openai.com/api/docs/guides/moderation ；【第三方】premai |
| 语义路由（embedding/分类器路由） | 少量 utterance 或偏好数据 | 近零（本地分类） | 个位数–数十 ms | 路由置信度 | 否 | 【官方】https://www.lmsys.org/blog/2024-07-01-routellm/ 、https://arxiv.org/html/2510.08731v1 |
| 小模型 LLM judge（Haiku / Luna 档） | 否 | 单次推算 $0.00046–$0.00225；**官方同任务每案例 $0.0033（luna）/ $0.0195（haiku 4.5）** | 秒级 | 口头置信，常过度自信 | **是** | 【官方】https://platform.claude.com/docs/en/models/haiku-4-5/overview 、https://developers.openai.com/api/docs/pricing 、evals 同上 |
| 前沿 LLM judge | 否 | 单次推算 $0.009–0.0225；**官方同任务每案例 $0.0836（sol）/ $0.1761（opus 5）** | 10–38s | 口头置信，常过度自信 | **是** | 【官方】evals 同上；【第三方】Arize 同上 |
| **Jev** | 否（零样本，写判据即可） | $0.042 / MTok 输入，**输出免费**；实测 $0.00035 / 次；官方同任务每案例 $0.0004 | 70–500ms 官方口径；实测 0.44–0.59s | **是，且是训练目标** | **否** | 【官方】https://docs.typesafe.ai/models 、evals 同上；【第三方】LangChain、NearHere、bitnovus |

### 1.2 官方 workflow evals 全表与口径提醒（一手抓取）

来源：【官方】https://evals.typesafe.ai/ （2026-09-22 抓取；同批文本另存于 `/tmp/jev-research/evals_text.txt`）

评测设定（官方原话要点）：4 个 workflow（Security Incidents / Agent Trace Observability / Invoice Processing / Customer Service），每个数据点为「一个模型配置在 4 个 workflow 上**等权平均**」；**参照标签 = GPT-6 Astra 与 Claude Fable 5.1 在 high thinking 下对 harness 中每个问题的答案的平均**；其余模型用各家默认推理设置；官方明言「我们假设 harness 本身是正确的」并称「**前沿模型里没有一个既更便宜又更准**」。

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

逐 workflow 的 Jev 数据（同一来源）：

| workflow | 案例数 | Jev 准确率 / 成本 / 延迟 | 该 workflow 最强对照 |
|---|---|---|---|
| Security Incidents | 240 | 61.7% / $0.0001 / 0.3s | Opus 5 66.2% / $0.0574 / 15.1s |
| Agent Trace Observability | 117 | 71.6% / $0.0003 / 0.5s | Sol 76.6% / $0.0575 / 40.3s（Luna 76.1% / $0.0025 / 14.5s） |
| Invoice Processing | 150 | 61.8% / $0.0011 / 0.5s | **Sol 79.1% / $0.2152 / 34.3s** |
| Customer Service | 204 | 76.0% / $0.0001 / 0.4s | Sol 78.3% / $0.0323 / 10.1s |
| **汇总** | **711** | **67.8% / $0.0004 / 0.4s** | Sol 74.1% / $0.0836 / 23.3s |

**必须一直带着的两个口径差异**：
1. 「准确率」是**与模型互评参照的一致率**，不是与人工真值的一致率；workflow 由厂商自家能力团队构建，官方自认可能偏置，参照偏向 OpenAI/Anthropic。
2. 每案例成本/耗时覆盖**整个 workflow 的多次模型调用**，与「按 2,000 输入 + 50 输出 token 推算的单次调用成本」**不可混用或加减**；LangChain 的 $0.00035/次 又是第三种口径（不同任务、不同 state 长度，反推约 8,300 输入 token/次）。
3. 「最强对照」命名差异已核实：kingy.ai 与 Arize 各取其一（Sol 74.1% vs Opus 5 73.1%），**不是数据漂移**；但该页会更新，引用必须写明抓取日期。kingy.ai 复核见 https://kingy.ai/blog/typesafe-jev-review-the-ai-model-that-doesnt-generate-text/ 、Arize 见上。

### 1.3 第三方实测：Jev 到底强在哪、弱在哪

- **LangChain（评测场景，agent judge）**【第三方】https://www.langchain.com/blog/jev-agent-evals-langsmith ：5 条固定轨迹 × 重复 100 次 = 500 次判定，人类打标为 oracle。二分类与人类一致率 **Jev 500/500 = 100%**（Terra 99.8%、Luna 96.4%、Claude Sonnet 4.6 80.0%）；连续分重复性均方差 **Jev 0.0000149**（Claude 高 92×、Luna 高 433×、Terra 高 913×）；**$0.00035 / 次、平均 0.44s**（Claude 整场实验 $28.17，推算 ≈$0.056 / 次）。作者自陈：只有 5 条轨迹、未设 temperature/seed、**Jev 服务版本未记录在元数据**、不保证可外推，「低方差 ≠ 正确」。
- **NearHere（活动 listing 校验）**【第三方】https://nearhere.events/blog/typesafe-jev-mistral-gemini-event-validation ：主测试集 50 条，**Jev 96% (48/50)** vs Gemini 3.5 Flash-Lite 86% vs Mistral Small 4 84%；误拒有效活动 Jev **0/13**；平均响应 0.59s；每 1,000 次决策 **$0.043**（Gemini $2.496、Mistral $0.370）。作者自陈：每模型单独调 prompt、开发集参与配置选择、**不是独立盲测**。
- **bitnovus（垃圾邮件，开源可复现）**【第三方】https://github.com/bitnovus/jev-spam-eval ：二分类 18,514 封 Jev 98.33% / AUC 0.9984 vs 监督基线 98.39% / 0.9986；2024–25 新钓鱼邮件 853 封 **Jev recall 95.31% vs 75.26%**；两者 50/50 平均后 **99.22%**（错误从 299 降到 144）；上下文富化把 Jev 从 93.62% → 97.98%、钓鱼 recall 85.71% → 98.43%（监督基线只涨 0.12pp）；学习曲线估计：达到 98.3% 约需 10,000 条标注（Ling-Spam 上约 200 条）；工程数据：19,772 次请求 / 3,206 万输入 token / 并发 16 下约 252 秒 / 约 $1.35。
- **Langfuse 转述的 Good Start Labs（6,003 次 rubric 评测）**【未证实，仅见转述】https://langfuse.com/blog/2026-09-18-using-typesafes-jev-for-evals ：Jev 与 Claude Fable 5.1 一致率 91.5%；每百万条被评分答案成本 Jev $160 vs Fable 5.1 $33,000、GPT-5.6 Luna $400、Gemini 3.8 Flash $1,600、DeepSeek V4.1 Flash $260（一致率 93.5%，比 Jev 高 2pp 但贵 $100）。
- **Every（777 个判断 < 0.7 秒）**【未证实，原文有 JS/订阅墙，数字经三个来源转述且互相一致：kingy.ai https://kingy.ai/blog/typesafe-jev-review-the-ai-model-that-doesnt-generate-text/ 、Arize https://arize.com/blog/typesafe-jev-llm-judge/ 、Firecrawl https://www.firecrawl.dev/blog/what-is-jev 】原文 URL https://every.to/also-true-for-humans/mini-vibe-check-typesafe-s-jev-judged-everything-i-ve-written-in-0-7-seconds ：37 篇文档（27 真实 + 10 刻意 AI 化）× 21 个问题 = 777 个判断 < 0.7 秒、成本约 0.25 美分；12 段人工植入缺陷文本中 Jev 抓到 6/7，Fable 5.1 全中；估计快约 25×、便宜约 580×（中位 0.35s vs 8.83s）。

### 1.4 替代路线各自的代价（为什么不能一刀切换到 LLM judge）

- **LLM-as-judge 的能力是真的**：GPT-4 类 judge 与人类一致率 >80%，与人类-人类一致率相当（MT-Bench / Chatbot Arena 论文，2023）https://arxiv.org/abs/2306.05685 。但**换模型/换档位差异巨大**：文档相关性任务 GPT-4 81%、Claude 3.5 Sonnet 77%、GPT-3.5 Turbo 仅 55%（对「不相关」类 recall 只有 7%）。【第三方】https://arize.com/blog/choosing-the-best-llm-evaluation-model/
- **judge 的置信度不可信**：2025 年研究识别出 **Overconfidence Phenomenon**（预测置信度显著高于真实正确率）并提出 TH-Score https://arxiv.org/abs/2508.06225 ；另有自偏好偏置 https://arxiv.org/abs/2404.13076 与位置偏置 https://aclanthology.org/2025.ijcnlp-long.18.pdf 。
- **judge 的隐藏成本**：一般模型每次 yes/no 要花约 **910 输出 token**，Jev 约 85 且不计费；judge 在官方 workflow 上为 **$0.03–0.18 / 次、10–38 秒 / 次**，若要求解释还要再加。【第三方】Arize 同上。
- **judge 的不可替代价值**：**解释**——可把失败原因成批喂回 coding agent 自动改进；Arize 明确指出这是「替换 LLM judge 时失去的最大东西」。【第三方】同上。
- **guardrail 线 Jev 打不动「免费」**：OpenAI Moderation endpoint **官方免费**、第三方测 50–100ms https://developers.openai.com/api/docs/guides/moderation ；Llama Guard 4 官方英文输出过滤 **recall 69% / FPR 11% / F1 61%**，多语种 43%/3%/51%，单图 41%/9%/38%，多图 61%/9%/52%，相对 LG3 英文 F1 +8pp https://huggingface.co/meta-llama/Llama-Guard-4-12B ；月度成本量级（100 万次请求）：正则 ~$20、LLM Guard ~$150、Guardrails AI ~$200、**Llama Guard（A100 自托管）~$800**、OpenAI Moderation $200、GPT-4 当 judge $3,000–10,000；5 个 90% 准确率的 guard 串起来，**至少一个误报的概率 41%**（要压到 25% 以下每层需 95%+）。【第三方】premai 同上。
- **语义路由是「和 Jev 抢活」而不是「替代 Jev」**：RouteLLM 在 MT Bench 上**省 85% 成本、保 95% GPT-4 质量**（MMLU 45%、GSM8K 35%；数据增强后用 14% 的 GPT-4 调用达到 95%，比商业路由器便宜 40%+）https://www.lmsys.org/blog/2024-07-01-routellm/ 、https://arxiv.org/abs/2406.18665 ；vLLM Semantic Router 在 MMLU-Pro 上**准确率 +10.24pp（p<0.01）、延迟 −47.1%、token −48.5%**，知识密集域提升 >20pp，**其分类核就是 Rust + HuggingFace Candle**（ModernBERT 微调，多任务：类别/PII/jailbreak）https://arxiv.org/html/2510.08731v1 、https://vllm.ai/blog/2025-10-27-semantic-router-modular ；aurelio semantic-router 支持本地全离线与可训练阈值 https://github.com/aurelio-labs/semantic-router 。**意义**：这条线的实现路径（Rust 分类核 + Candle + 多任务并行 + 本地部署）与本项目技术栈几乎重合，Jev 的差异不在「路由得更好」，而在「**免训练新意图、返回概率、能与业务判据并行提问**」。

### 1.5 选择准则：什么时候用 Jev，什么时候用别的

**决策树（按优先级）**

1. **能用确定性代码判断的，两者都不用**（官方文档的态度也是：state 里放数据，规则放代码）。
2. **候选答案是预先已知的有限集合 / 是-否 / 有序档位，且没有标注数据、需要概率门控、延迟敏感** → **Jev**。
3. **同类判定但你有几百到几千条标注、追求个位数毫秒、要自托管可控** → **embedding + 分类头 / 微调编码器**（SetFit 类方法在 8 例/类即可接近全量微调，训练 30 秒、$0.025）https://huggingface.co/blog/setfit 。
4. **候选「意图」本身很稳定，只需把它分到某条链路上** → **语义路由**（RouteLLM / vLLM Semantic Router / aurelio），成本近零。
5. **内容安全、固定 taxonomy、要图像或多语种、要合规可解释的过滤** → **专用 guardrail（OpenAI Moderation 免费、Llama Guard 4 自托管）**，**不要**拿 Jev 替这一类。
6. **需要解释、需要多因素混合推理、需要生成、或判定频率低而单次价值高** → **LLM judge（小模型档起步，不够再升级）**。
7. **混合架构（多数团队的实际最优解，也是 Arize 的建议）**：**Jev/分类器跑全量覆盖做监控，失败与不确定样本抽样送 LLM judge 拿解释**。

**明确不要用 Jev 的场景**：要写文本/代码/摘要；要给出理由；需要数学或日期运算；图像/音频输入；**没有逃生通道的强制二选一**；state 长而杂且无法裁剪；要靠领域微调才能达到精度要求的任务。

**两条来自本专题的定位结论**（与上游 `10-alternatives.md` 一致）：
- 抽象层应按「**决策**」建模而非按「模型调用」建模：输入 state + 类型化 questions，输出「带概率的类型化答案 + 用量」。Jev 是它的一个实现，LLM 走「结构化输出」实现（官方 workflow evals 用的 System One LLM wrapper 就是这个思路，官方承认这条路更慢更贵），于是 Jev 不可用/超限/不擅长时可在同一链路回退。
- 不要把 Jev 当精度天花板：官方参照是模型互评，第三方汇总显示它比最强对照低约 6pp（发票场景低 17pp），而对最便宜小模型档的成本优势只有个位数到两位数倍数。定位应是「**免标注 + 概率校准 + 并行提问 + 无解析失败**的快速判定层」，差异化在工程属性；**关键判定必须在本项目自己的带真值数据集上测过再上线**。

### 1.6 本节不确定项

1. Jev 的一切数字都来自 2026-09 的 early access 产品，官方自评占主导；第三方实测样本都极小（5 条 agent 轨迹、50 条 listing、711 个 workflow 案例、1.5 万封邮件、6,003 次 rubric），**没有任何一个是同任务同条件的严格对照实验**。
2. 官方 dashboard 的「真值」是**模型互评**（GPT-6 Astra + Claude Fable 5.1 high thinking 的平均），非人工标注；workflow 由厂商自家团队构建，官方自认可能有偏置。
3. **校准性没有独立验证**：官方未发布校准曲线或指标。bitnovus 仓库有分档校准分析（<0.1 → 0.1% spam；≥0.9 → 99.9%；0.5–0.6 档仅 38%；把 0.3–0.7 的 4.6% 转人工后其余达 99.5%），但**单一数据集、单一模型版本，不能当通用校准保证**。
4. **未找到公开信息**：p95/p99 延迟、并发表现、限流命中率、重试率、版本漂移影响；Jev 与「专门训练的现代分类器/重排器」在同一真值任务上的独立对比；官方 model card / 架构 / 参数 / 训练数据披露。
5. **依赖转述的数字**（引用时须标二次来源）：Every 原文（JS/订阅墙，三处转述一致）、Good Start Labs 的 6,003 次评测（仅 Langfuse 一处转述）、DataCamp 文章（403）。官方 workflow evals 数值**不属于**这一类（已一手核对）。
6. **间接引用需降级使用**：premai 文章中的 ToxiGen（Llama Guard 3 P0.91/R0.88/F1 0.89）与 JailbreakBench（检出 85%、FPR 15%）表格**未注明来源与方法**；Llama Guard 的 800ms 与月度成本也是第三方估算。
7. 语义路由的数字都有明确条件（特定模型、硬件、benchmark），非普适常数；RouteLLM 基于 GPT-4 Turbo + Mixtral 的模型对，与 2026 年模型组合不可直接平移。
8. **未找到「Jev vs 本地 Rust 分类核」的对比**——这是明显空白：vLLM Semantic Router 已证明 Rust + Candle 的 ModernBERT 分类核可行且极快，值得单独验证。

---

## 二、Rust 集成要点

**要点**

- **TypeSafe 官方没有 Rust SDK，也没有任何 Rust 计划**：官方只有 Python 与 JavaScript/TypeScript 两套 SDK，非这两种语言官方口径是「直接调 HTTP API」；官方全量文档语料（903,311 字节）全文检索 "Rust" **零命中**；`https://docs.typesafe.ai/sdk/rust/` 返回 **HTTP 404**；官方 GitHub 组织 `typesafe-ai` 仓库列表无 Rust 仓库。【官方 + 自测】
- **社区 Rust 生态异常繁荣但极度碎片化**：crates.io 上关键词 `jev` 有 **29 个** crate、`system-one` **13 个**，其中至少 **18 个**是各自独立的 Jev/System One 客户端；全部自我标注 unofficial、全部诞生于 2026-09-16～09-21 的六天内、全部 `0.x`、下载量 9–329、GitHub star 0–11。**没有任何一个可称事实标准。**【第三方】
- **命名冲突**：crates.io 上已存在名为 **`jev-harness`** 的 crate（0.1.4，2026-09-21，描述 "Zero-overhead System One decision harness and token optimizer for AI coding agents."），与本项目同名。若本项目日后有意发布 crate，需先处理命名。【第三方】
- **HTTP API 不支持流式，且这是设计使然**：官方文档语料中无任何 SSE / stream / server-sent 响应接口描述；官方「Jev with coding agents」页原文：*"Coding agents rely on an LLM that streams text, calls tools, and edits files based on natural-language instructions. **Jev does none of that.**"* 架构上自洽（Parallel 采样，一次前向出全部概率，非逐 token 自回归）。【官方】https://docs.typesafe.ai/introduction/coding-agents
- **从 Rust 调用可行，官方 API 形状对 serde 非常友好**：`Question` 由 `type` 字段区分三型，正好映射为 Rust **内部标签枚举**（`#[serde(tag = "type")]`）；**已实际编译验证该映射能无损往返官方示例 JSON（8 项断言全部通过）**。【自测】
- **真正的「并发」不是并发发请求，而是把问题打包进单请求**：官方推荐一次请求问多个问题，并给出实测 13 问合并 = **12.2× 更便宜、10.0× 更快**（官方另一处口径为 **11.5× / 9.6×**，两处数字不一致，见下）。限流为 **1,200 requests/min + 250,000 tokens/s**——多打请求会先撞 request 限流，打包提问才是正解。

### 2.1 官方 SDK 覆盖面：只有 Python 与 JS/TS

| 证据 | 内容 | 来源 |
|---|---|---|
| 官方 SDK 索引页 | 只列 Python、JavaScript/TypeScript 两张卡片，原文 *"You can also call the **HTTP API** directly from any language."* | 【官方】https://docs.typesafe.ai/sdk |
| 官方 sitemap | 有 `/sdk/python`、`/sdk/javascript` 及子页，**无 `/sdk/rust`** | 【官方】https://docs.typesafe.ai/sitemap.xml |
| 反向确认 | `https://docs.typesafe.ai/sdk/rust/` → **HTTP 404 Not Found** | 【自测】本次调研 curl 实测 |
| 官方 GitHub 组织 | `typesafe-sdk-python`(186★)、`typesafe-sdk-js`(214★)、`system-one-adapter-python`(241★)、`skills`(1575★) 等，**无 Rust 仓库** | 【官方】https://api.github.com/orgs/typesafe-ai/repos |
| 仓库身份确认 | PyPI `typesafe-sdk` 0.7.1 的 Repository 指向 `github.com/typesafe-ai/typesafe-sdk-python` | 【官方】https://pypi.org/pypi/typesafe-sdk/json |
| 官方全量语料检索 | `llms-full.txt` 903,311 字节，`grep -in "rust"` 无真实命中（仅 `frustration` 等子串） | 【自测】https://docs.typesafe.ai/llms-full.txt |

→ 在 Rust 项目中接入 Jev **没有官方支持的路径**，只能（a）自己调 HTTP API，或（b）选用第三方社区 crate；也意味着**没有官方兼容性承诺兜底**，抽象层要假设 wire 契约会变。

### 2.2 社区 Rust crate 生态（crates.io 实测枚举）

检索方式：crates.io API 按关键词枚举。【第三方】https://crates.io/api/v1/crates?keyword=jev + https://api.github.com 逐个核验。

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
- `system-one`：backend-agnostic 抽象（`JevHttp` / `Mock` / `Replay` / `LocalLogprob` / `Shadow` 多种后端），`#[derive(AsChoice)]`、`#[derive(AsQuestions)]` 把 Rust 类型直接变成问题集并回解析答案，自带 `calibration::CalibrationReport` 检查「标称 0.8 是否真是 80%」。https://docs.rs/system-one/latest/system_one/
- `typesafeai-sdk-community`：自称官方 Python SDK 的移植（*"same primitives, same request/response shapes, same retry and error semantics"*），提供 `ChoiceLabels` / `ScoreLevels` / `Questions` / `Route` / `Composite` 等 derive，feature flag 区分 `rustls`（默认）/`native-tls`/`blocking`/`test-util`。https://docs.rs/typesafeai-sdk-community/latest/typesafeai_sdk_community/
- `jev-sdk`（仓库 404，但 README 的安全清单有参考价值）：API key 存为敏感 `HeaderValue` 并从 `Debug` 脱敏、拒绝含 CRLF 的 key、强制 HTTPS（仅回环允许明文）、内建客户端不跟随重定向、响应体上限（成功 16 MiB / 错误 64 KiB）、退避饱和且带 jitter、`Retry-After` 上限 60s、依赖最小化。https://docs.rs/jev-sdk/latest/jev_sdk/

### 2.3 serde 类型映射与编译验证结论

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

**映射与类型设计建议（上游给出的落地方式）**：
- `Question` 用内部标签枚举；`choice` 的 `criteria` 为 `map<选项, Option<Instructions>>`（**必须允许 `null`**，已自测）；`score` 为 `Vec<Instructions>`（**2–10 级**）；`noul` 为可选 `{true, false}`。
- `Instructions` 用 `#[serde(untagged)]` 表达官方 `string | object | array`（已自测），使「把问题与它引用的数据放进同一对象」的结构化写法可用 Rust 结构体而非裸 `serde_json::Value`。
- **把「问题类型」与「答案类型」在编译期绑定**：用标记类型参数化 id（如 `Id<ChoiceKind>`），使 `Id<NoulKind>` 在类型上取不到 choice 答案——比社区 crate 普遍的 `response.choice("id").unwrap()`（写错只能运行期 `None`）更安全，是本项目相对现有社区实现的**真实改进点**。
- **不要用 `Option<f64>` 抹平三型差异**：官方 `noul` 答复**没有 `confidence`**，只有 choice / score 有；用枚举让「对 Noul 索要 confidence」变成编译错误。这是「类型安全」与「概率优先」总则的落地方式。
- **限制项应在类型或构造期强制**：Choice ≤ 255 选项、Score 2–10 级、`state` + 最长问题 ≤ 32k tokens；越早失败越好，别等到 422。

### 2.4 并发、限流与超时策略

**官方 cookbook 实测（本专题最重要的性能证据）**【官方】https://docs.typesafe.ai/cookbooks/parallel_questions ：测试条件为 GDPR 的 Wikipedia 条目（约 54,000 字符，文档主导型负载）、13 个问题（8 Noul + 2 Choice + 3 Score）、每种策略重复 5 次、使用官方 Python SDK（`typesafe-sdk>=0.5.7`）。

| 策略 | 调用数 | 成本 | 总耗时 |
|---|---|---|---|
| 一次调用，全部 13 问 | 1 | $0.000497 | 0.27s |
| 13 次调用，每问一次 | 13 | $0.006090 | 2.71s |
| — | — | **12.2× 更便宜** | **10.0× 更快** |

- 答案一致性：官方称「大多数答案在 5 次重复中完全一致，运行间标准差正好为 0.0」，即**打包不改变答案质量**。成本机制：文档在每个请求中占主体，N 次单问要重复支付 N 次文档成本与往返。
- **⚠️ 官方内部数字不一致**：官方「Speculative fan-out」页对（推测）同一实验给出的是 *"batching 13 questions into one call is **11.5x cheaper and 9.6x faster** than 13 separate calls"*。【官方】https://docs.typesafe.ai/patterns/fan-out → 引用时应写「**官方两处口径为 11.5×–12.2× 更便宜、9.6×–10.0× 更快**」，或直接引 cookbook 原始表格（有完整测试条件）。

**限流与配额（超时/预算依据）**【官方】https://docs.typesafe.ai/models 、https://docs.typesafe.ai/api ：

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
- JS/TS SDK `RetryPolicy` 默认：`maxRetries=2`、`backoffInitialMs=500`、`backoffMaxMs=5000`、`backoffJitter=0.25`、`httpStatuses=408/429/500–599`、`respectRetryAfter=true`、`maxRetryAfterMs=60000`、`apiConnectionError=true`、`apiTimeoutError=true`。【官方】https://docs.typesafe.ai/sdk/javascript/api/interfaces/RetryPolicy
- Python SDK：`DEFAULT_TIMEOUT = 10.0`、`DEFAULT_MODEL = 'jev-latest'`、环境变量 `TYPESAFE_API_KEY` / `TYPESAFE_BASE_URL` / `TYPESAFE_DEFAULT_MODEL` / `TYPESAFE_LOG_LEVEL`；文档中的配置示例为 `RetryPolicy(max_retries=3, timeout=10.0, http_statuses={429,500,502,503,504})`。【官方】https://docs.typesafe.ai/sdk/python/api/constants 、https://docs.typesafe.ai/sdk/python/api/retries
- **⚠️ 两套官方 SDK 不一致**：Python 示例写 `max_retries=3`，JS 写明默认 `maxRetries: 2`；重试状态码集合也不同（`{429,500,502,503,504}` vs `408,429,500–599`）。Python 侧未找到「默认值」的显式声明，**其真实默认重试次数仍未证实**。本项目应自行定档并写入文档。
- 官方对 `Retry-After` 的措辞：*"Our client SDKs retry with backoff by default and honor the `retry-after` header when the response carries one."*（https://docs.typesafe.ai/models ）

**并发设计的三条建议**：
1. **不要让每个判断各发一次请求**：在循环中做**问题批收集（batching）**，把同一 state 下的多个判定（路由、紧急度、门控）合并成一次调用；官方甚至鼓励「投机性扇出」——把可能用到的判定一并问掉，由代码决定用哪个答案。
2. **限流约束决定「并发」的真正用途**：并发应集中在一个**带全局预算的限流器**后面，而不是每个调用点各自重试（多处退避会互相叠加、放大 429）。真正的多路并发应留给**多个不同 state**（多候选重排、多文档分类），可参考官方 cookbook 中 `ThreadPoolExecutor` + 小 worker 池（如 `WORKERS = 8`，注释为 *"small pool: enough to keep a live run to minutes, gentle on rate limits"*）的做法。
3. **超时按「快模型」定档**：官方典型端到端响应 70–500ms、Python SDK 默认超时 10s，本项目 Jev 调用超时应远短于 LLM 调用超时（对 agent 循环的「卡住」检测很重要）；但官方 cookbook 中出现过 `timeout=120.0` 的用法（大文档/大问题组合），说明**超时预算应与 `state` 规模挂钩，单一常量不够**。

**UI / 事件流不能假设 token 流**：Jev 调用必须建模为「一次原子调用，返回完整结构」；若 harness 事件流围绕 LLM 的增量 token 设计，Jev 需要一个**立即完成的不同事件分支**，不能强行套用同一套流式接口（否则等于引入假的流式包装）。

### 2.5 本节不确定项

1. **未找到官方对 Rust 的任何表态**（既无支持计划，也无明确「不做」的声明）。只验证了「官方现在没有 Rust SDK」，**不能推断官方未来不会出**；官方 GitHub 组织下也未逐 issue 检索。
2. **没有 API key，本次调研未对线上 API 做任何真实调用**。所有 HTTP 行为（限流触发、`429`/`529` 实际表现、`Retry-After` 头是否真的出现、实际延迟分布、`noul` 答复是否真的从不带 confidence）均**来自官方文档描述，未经实测**；2.3 的 serde 验证是对**官方文档 JSON 示例**的往返，不是对线上响应的验证。
3. **官方单请求问题数上限未知**：文档只给单类约束（Choice ≤ 255 选项、Score 2–10 级），未找到「一次请求最多多少个问题」的明确上限（cookbook 用了 13 个，spec 提到 fan-out 可塞很多问题，具体上限未公开）。
4. **官方两处并行基准数字不一致**（12.2×/10.0× vs 11.5×/9.6×），且两处是否同测试条件无法确认，引用须同时注明两处。
5. **官方两套 SDK 重试默认值不一致**，Python 侧真实默认值未证实。
6. **社区 crate 全部未经生产验证**（无超过 329 下载量、无超过 11 star、全部创建于 2026-09-16 之后），`jev-sdk` 还出现元数据仓库 404。若引入需自行审计源码（尤其 API key 处理与 TLS 配置）。
7. **`jev-harness` 名字已被占用**（0.1.4，2026-09-21），**未核实其作者身份、是否与本项目存在关联**（例如是否为本项目此前的尝试），需项目决策者确认，不作推测。
8. **模型版本会漂移**：本片段中的字段形状、限制、价格、限流均**对应 2026-09-22 时点的官方文档**，未来可能变化。
9. **容量口径不一致（跨文件核实）**：官方模型页写 64k/请求（state + 全部问题）、32k（state + 最长单问题），而 **OpenRouter 上标 32K**（一手抓取 `or_113.txt` 确认写的是 32K / 32,000），Cloudflare Workers AI 文档同样标 32,000；设计前须自行核实。

---

## 三、公司背景、条款与依赖风险

**要点**

- **公司很年轻、很早期**：TypeSafe AI, Inc. 2024 年成立，约两年 stealth，**2026-09-15 出 stealth** 并发布 Jev；融资**约 $40M 种子轮（DCVC 领投）**；LinkedIn 团队规模 **11–50 人**；创始人自述**上线前几乎没有收入**。→ 对本项目而言是**高潜力但单点依赖风险真实存在**的供应商。【官方新闻稿 + 播客 + 第三方】
- **无 SLA**：主合同（MCA）第 3 条的支持承诺仅为「commercially reasonable efforts」+ `support@typesafe.ai`，**全文无可用性数字、无停机赔付**。官方有状态页，但那是自我披露的历史数据，不是合同义务。【官方，已核对原文】https://typesafe.ai/legal/mca
- **可靠性是「已发生过的风险」而非假设**：官方状态页显示 `api.typesafe.ai` 近 90 天可用率 **99.840%**（`console.typesafe.ai` 99.988%），含多次数分钟级中断（8/03 **59 分钟**、9/19 **18 分钟**、9/20 记录 "API issues"）；TechCrunch 报道上线期因流量过高**一度无法从 API 服务用户**。【官方】https://status.typesafe.ai/ +【第三方】https://techcrunch.com/2026/09/18/a-new-kind-of-ai-model-from-a-chatgpt-inventor-is-thrilling-developers/
- **数据条款比营销口径粗糙**：官方确实承诺不拿 Input 训练（隐私政策明文），但 **MCA 4.1 留有「客户事先同意」例外**且把「训练」限定为「修改模型权重」；**MCA 4.3 的 Telemetry 定义含 "learnings"、可无限制处理**，第 11 条把 Telemetry 知识产权归 TypeSafe，10.4 使其在协议终止后继续存续；**ZDR（零数据留存）仅面向企业客户**，需邮件申请。【官方，已核对原文】https://typesafe.ai/legal/privacy-policy 、https://typesafe.ai/legal/mca 、https://docs.typesafe.ai/legal
- **API 变更风险写进了合同**：MCA 2.5 明确「TypeSafe 可能不时更新服务，且**此类变更可能导致 API 与客户应用不兼容**」，仅承诺商业上合理努力提前通知重大不利变更。**这对需要长期稳定对接的 harness 是直接风险。**【官方，已核对原文】
- **生态接入面已相当宽，但容灾层薄**：官方 Python/JS SDK 之外，Pydantic AI、LangChain、LiteLLM、Cloudflare Workers AI、Opper、Requesty、Vercel AI SDK 均已适配；**未发现官方 Rust SDK**；OpenRouter 是否上架**两份上游文件结论冲突**（见 3.4）。

### 3.1 公司身份、创始人、融资

| 事实 | 内容 | 来源 |
|---|---|---|
| 成立年份 / 总部 | 2024 年成立；总部旧金山（注册地址 255 California St, Suite 1300, San Francisco, CA 94117） | 【官方】新闻稿（Business Wire，Yahoo Finance 全文转载）https://finance.yahoo.com/technology/ai/articles/typesafe-ai-emerges-stealth-40m-190000776.html ；地址见 https://typesafe.ai/legal/mca 、https://typesafe.ai/legal/terms |
| 出 stealth | 2026-09-15，同日发布 Jev 并开启 early access / waitlist | 【官方】同上；【第三方】TechCrunch 2026-09-18（同上 URL） |
| 融资 | **约 $40M 种子轮，DCVC 领投** | 【官方】新闻稿同上；【第三方】https://www.finsmes.com/2026/09/typesafe-ai-raises-40m-in-seed-funding.html 、https://www.hpcwire.com/aiwire/2026/09/16/typesafe-ai-emerges-from-stealth-with-40m-in-funding-with-new-model-for-composable-ai/ 、https://seedtable.com/companies/typesafe-ai/funding-rounds/seed-2026-09 、https://dealroom.co/news/151032-typesafe-exits-stealth-with-40m-seed-to-build-ai-for-software-not-people/ |
| 投资方表态 | DCVC General Partner James Hardiman：*"Diogo, Erik, and Sasha bring the technical depth and conviction to rethink how models are built for software."* | 【官方】新闻稿同上 |
| 创始人 | **Diogo Almeida**（co-founder & CEO）、**Erik Gafni**、**Sasha Sheng** | 【官方】新闻稿同上；LinkedIn 帖 https://www.linkedin.com/posts/robbroderick_after-two-years-in-stealth-diogo-almeida-activity-7506785550129864704-7UYQ |
| Almeida 履历 | 「co-invented RLHF and InstructGPT，此前在 Google Brain」；官方与媒体多用「ChatGPT 共同发明人」口径 | 【官方】https://typesafe.ai/team ；【第三方更正】更准确说法是 InstructGPT 论文的 equal-contribution primary author、并参与 GPT-4 https://anthonymaio.substack.com/p/jev-the-language-model-that-wont |
| 团队构成 / 办公 | 来自 OpenAI、Google Brain、Meta/FAIR、Stripe、Airbnb、Plaid、Docker；旧金山 Embarcadero 附近办公室，**每周 5 天现场办公** | 【官方】https://typesafe.ai/team |
| 团队规模 | LinkedIn 公司页标注 **11–50 employees** | 【第三方】https://www.linkedin.com/company/typesafe-ai |
| 招聘 | 在招 Member of Technical Staff（Model Capabilities / Backend-Platform），Base $150k–250k + equity | 【第三方】https://jobs.ashbyhq.com/typesafe-ai 、https://www.glassdoor.com/job-listing/jv?jl=1009970275998 |
| 收入状态 | 创始人自述「上线前几乎没有收入」(*"We had almost no revenue before launch."*) | 【播客-创始人】https://www.latent.space/p/jev |
| 自我定位 | 「We're building prod, not God」；自称 **data lab 而非 model lab**；训练数据全部合成 | 【官方】https://typesafe.ai/manifesto ；【播客-创始人】同上 |
| 对外口径的谨慎 | 官方博文自认「193.6×/444.6× 属于偏高端」，且「无法证明定价没有被补贴」 | 【官方】https://typesafe.ai/blog/introducing-system-one-models-and-jev |

**可核实的时间线**：2024 成立 → 约 2 年 stealth → **不晚于 2026-06-24** `api.typesafe.ai` 已进入官方状态页监控（说明 API 在公开发布前已运行）→ 2026-09-15 出 stealth + 发布 + $40M 披露 → 2026-09-15～19 第三方生态适配密集上线（Every 实测 9/15；LangChain 指南、Pydantic AI provider、Cloudflare Workers AI 文档、LiteLLM pass-through 均在首周内出现）→ 2026-09-22 仍处 early access / waitlist 状态。

**产品线之外**：目前唯一公开模型就是 Jev（`jev-1.13.0`，别名 `jev-latest` / `jev-preview`，共用端点 `POST /v1/systemone`）；平台层有 `console.typesafe.ai`、评测站 `evals.typesafe.ai`、信任中心 `trust.typesafe.ai`（Vanta 托管）、文档站；路线图官方仅表「还有很多在管线中」，**无日期、无产品名**。**未找到公开信息**：OpenAI 兼容的 `/v1/chat/completions` 官方端点、本地/私有化部署方案、官方 Rust SDK。

### 3.2 可靠性、运维与运行区域

- 官方状态页（Better Stack 驱动，提供 30/60/90 天窗口切换，静态文本未直接标明当前窗口，抓取时可见条目自 2026-06-24 起）：`api.typesafe.ai` **99.840%**；`console.typesafe.ai` **99.988%**。可读到的中断：2026-06-29（6 分钟）、07-09（20 分钟）、07-10（18 分钟）、07-23（12 分钟）、**08-03（59 分钟）**、08-21（8 分钟）、09-09（6 分钟）、09-16（5 分钟）、**09-19（18 分钟）**，以及 09-20 一条标注 **"API issues"**（2 分钟，"now operational"）。【官方】https://status.typesafe.ai/ （2026-09-22 抓取）
- 上线期容量事件：TechCrunch 报道公司因需求过高**一度失去从 API 服务用户的能力**。【第三方】同 TechCrunch URL。
- 创始人口径：「我认为我们的 uptime 9 的个数比 Anthropic 还多」，同时承认「我认为我们会在很长时间内受限于 GPU」。【播客-创始人】https://www.latent.space/p/jev （自我评价，非独立核实）
- 合同层面：MCA 第 3 条 Support = 「commercially reasonable efforts」+ 邮件支持；**无 SLA、无可用性百分比、无停机赔付**（已核对原文）。
- 安全事件通知：DPA 5.2 承诺「无不当延迟且无论如何在知悉后 **72 小时内**」通知；审计权：DPA 5.3 允许客户**每 12 个月最多一次**审计、费用自担；安全措施载体：DPA Schedule I 第 11 项全文仅为一句——安全措施「如 TypeSafe 信任中心所述」，即**一个 URL**（对 https://trust.typesafe.ai/ 的抓取只得到 Vanta 的 JS 壳，**SOC 2 报告是否存在、是否可下载未见一手来源**）。【官方，已核对原文】https://typesafe.ai/data-processing
- **运行区域**：官方发布博文原文承认评测「一般是在美西的笔记本上跑的（**这也是我们服务目前所在的位置**）」；隐私政策称服务托管在**美国**（已核对原文）。→ 对非美区调用方，端到端时延需自行测量。
- 超载语义：`529 Overloaded` 是官方文档明确列出的错误码，说明服务端过载被当作**契约内的正常状态**，客户端必须自行退避。
- 第三方可用性观测（意义有限）：piehost 对 `typesafe.ai`（**营销网站**，非 API）90 天 HTTP 可用率 100% https://piehost.com/status/typesafe.ai ——**不覆盖 API，不可作为 API 可用性证据**。

### 3.3 条款、隐私与数据留存（逐条已核对原文）

| 文件 | 覆盖范围 | 管辖/场地 | 责任上限 | URL |
|---|---|---|---|---|
| Terms of Use | **仅网站 typesafe.ai** | 特拉华州法；特拉华州法院 | **$100 USD** | https://typesafe.ai/legal/terms |
| Master Customer Agreement (MCA) | **API 与控制台** | **加州法；旧金山** | 取高者：(A) 主张发生前 12 个月已付/应付费用，(B) **$50 USD** | https://typesafe.ai/legal/mca |
| Data Processing Addendum (DPA) | Input 中的个人数据 | 爱尔兰法；**都柏林法院**（EU SCC 模块 2/3 + UK Addendum） | 依 MCA；冲突时 DPA 优先（1.3） | https://typesafe.ai/data-processing |
| Privacy Policy | TypeSafe 自身的数据处理 | 未写明 | 不适用 | https://typesafe.ai/legal/privacy-policy |

- **不训练承诺（隐私政策，最平实）**：*"**We will not train or fine tune any artificial intelligence or machine learning models on your prompts or other Input.**"* 并承诺不向服务提供方以外的第三方披露 Input。
- **不训练承诺（MCA 4.1，留有口子）**：不会把 Customer Data 纳入训练数据集「(i.e., to modify the model weights of)」——**除非客户事先同意**；该定义**锚定在「修改模型权重」**上，字面不覆盖评测集、检索语料等。
- **Telemetry（MCA 4.3，实际最宽的授权）**：Telemetry = 「technical logs, hashes, summary statistics and classifications, metrics, and **learnings** related to Customer's use of the Services」，TypeSafe 可**无限制处理**（含用于改进其「其他产品与服务」）；4.1(b) 明确授权其为**派生 Telemetry** 而处理 Customer Data；第 11 条将 Telemetry 知识产权归 TypeSafe；10.4 使 4.1/4.3/11 在协议终止后**继续存续**。→ 即「由你的数据派生、归他们所有、期限无限制、协议结束后仍是他们的」。**隐私政策中不出现 "Telemetry" 一词**（已核对全文），两份文件口径存在落差。第三方批评见 https://wunderlandmedia.com/typesafe-ai-jev-terms-of-service-gdpr 、https://wonderwhy-er.medium.com/typesafe-jev-wont-train-on-your-data-it-can-still-learn-from-it-563f0ad591b6 。
- **留存与删除**：隐私政策只写留存「as long as reasonably necessary to provide you with the Services…」——**无具体天数**；MCA 10.3 明确 TypeSafe **无义务**存储或保留 Customer Data，并可**自行决定随时删除**；**DPA 正文中没有删除/返还数据的条款**。→ 第三方认为删除权需经 SCC 第 8.5 条迂回主张，且**没有删除证书或导出窗口**。
- **ZDR（零数据留存）**：仅面向企业客户，需邮件 `privacy@typesafe.ai` 申请——属「可谈」而非默认。【官方】https://docs.typesafe.ai/legal
- **责任与赔偿的不对称**：12.2 上限取高者（12 个月费用, $50）；12.3 排除项含客户未付款、客户违反 2.3/2.4/5、以及**双方的赔偿义务**；13.1 侵权赔偿**仅限「美国」**专利/著作权/商标/商业秘密；13.5(e) **明确排除 Output**。
- **免责声明与营销口径的落差**：9.3 以全大写声明服务按 "AS IS / AS AVAILABLE" 提供、**可能产生不准确输出**、客户须自行评估；9.1/9.2 的 Service Warranty 仅保证按文档「materially」运行，救济是 30 天内修复或退还未使用预付费。→ 与营销侧「不能幻觉 / 100% 类型正确」并存：**类型正确 ≠ 语义正确**（第三方多篇均指出，官方博文也承认 hallucination 与 type-safety 是两件事）。
- **协议可被单方面变更**：16.7 允许 TypeSafe 通知更新协议、**至少 60 天后**对后续生效；ToU 第 6 条允许随时变更且「每次访问即受最新版约束」。**暂停权**：第 6 条列明可**立即暂停**的四种情形（含违反许可限制、逾期 30 天未付款、法律变化、危害其他客户/系统安全）。
- **许可限制（对本项目最重要的两条）**：2.3(b) **禁止用服务或 Output 做模型蒸馏、训练模仿其输出的模型、或开发同类/竞争产品**；2.3(a) 禁止把服务作为独立服务转售/分发/再许可；2.3(g) 把「进行安全或漏洞测试」与「绕过访问控制」并列禁止。
- **预付费与退款**：8.2 Credits「不可赎回、不可退款、不可转让、非法定货币」，到期时间取「期限结束」或「购买后 12 个月」较早者；10.3(c) 终止时**未消费金额不退还**。
- **争议解决**：15.1 强制**单独仲裁**（JAMS，加州），15.7 放弃集体诉讼；MCA 由加州法管辖、旧金山场地（16.2）。
- **网站抓取限制（仅限网站，不是 API 条款）**：ToU 3(b)(vi) 禁止以自动化进程 / robots / spiders / scrapers / webcrawlers 监控、复制或下载 Site 上的数据；**MCA 全文没有禁止公布 benchmark 的条款**。
- **⚠️ 第三方分析有可验证的错误**：wunderlandmedia 一文称「MCA 适用特拉华法 / loser pays」——**错**（特拉华法 + $100 上限对应的是仅覆盖网站的 Terms of Use，该文把两份文件混为一谈）；又称「MCA 2.3(f) 禁止公布 benchmark，违者暴露无限」——**当前原文中不存在该条款**（2.3(f) 实为 "interfere with the operation of the Services"）。**该文细节不可直接引用**；其其余内容（Telemetry 四条款联动、责任上限不对称、13.1 仅限美国 IP、13.5(e) 排除 Output、无 SLA、DPA 附件薄弱、TypeSafe 未加入 EU-US DPF）与原文方向一致，可作线索但引用前须回到一手条款。

### 3.4 生态接入面与单一供应商风险

| 集成方 | 形态 | URL | 性质 |
|---|---|---|---|
| Cloudflare Workers AI | 官方文档托管 `typesafe/jev`，含 Workers `env.AI.run` 与 REST 两套示例，**上下文窗口标 32,000 tokens** | https://developers.cloudflare.com/ai/models/typesafe/jev/ | 【第三方平台官方文档】影响力最大的分发渠道之一 |
| Pydantic AI | 内置 `TypeSafeModel`（`typesafe:jev-latest`），一个 Pydantic 输出模型的每个字段映射为一个问题；`ModelResponse.model_name` 记录实际应答版本 | https://pydantic.dev/docs/ai/models/typesafe/ | 【第三方平台官方文档】 |
| LangChain | `langchain-typesafe`：`TypeSafeClassifier` + `ModelRouterMiddleware`（模型路由）+ `AutoModeMiddleware`（工具调用前的风险门控） | https://www.langchain.com/blog/building-a-harness-with-jev | 【第三方】与本项目「Agent Harness」定位最接近的公开范例 |
| LiteLLM | pass-through 端点支持（Jev *"is called through its own evaluate endpoint rather than `/chat/completions`"*） | https://docs.litellm.ai/docs/pass_through/typesafe | 【第三方】 |
| Vercel AI SDK | 有接入教程；Vercel 工程师 Pranit Sharma 用 Jev 替换 ChatGPT Luna 做命令安全分类，称**快 5–18× 且更准** | 同 TechCrunch URL | 【第三方】单场景自测，非基准 |
| 网关 | Opper（标注 ZDR posture「not established」、有 DPA、无训练）、Requesty（提供 OpenAI 风格 `response_format: {"type":"questions"}` 封装） | https://opper.ai/provider/typesafe 、https://www.requesty.ai/blog/typesafe-jev-explained | 【第三方】 |

**⚠️ OpenRouter 是否上架：两份上游文件结论冲突，并列呈现，不作裁定**
- 【结论 A｜未上架】以 `https://openrouter.ai/api/v1/models` **全量列表**检索，未找到任何 Jev / TypeSafe 条目；同日抓取 `openrouter.ai` 的 Typesafe 页面亦为空页 → 推论是 **Jev 没有第三方多 provider 路由/故障转移层**，harness 容灾不能指望「换 provider」。（来源：上游 `12-company.md`，2026-09-22 实查）
- 【结论 B｜已上架】以 `https://openrouter.ai/api/v1/models/typesafe/jev-1.13/endpoints` 读取到条目：`typesafe/jev-1.13`、`context_length 32000`、pricing prompt `0.000000042`（=$0.042/M）、completion `0`、provider=TypeSafe、`uptime_last_1d` 100%；`10-alternatives.md` 亦从 OpenRouter 的 Jev 1.13 页面确认 context length 写的是 **32K / 32,000**。（来源：上游 `13-community-debate.md`、`10-alternatives.md`，均为 2026-09-22 读取）
- **口径差异的可能解释**（上游未裁定）：全量列表未收录 / 分页或区域差异 / 仅特定账号可见 / 条目页与列表页数据不同步。**引用时必须写清检索路径与时间**；「Jev 是否有第三方多 provider 容灾层」这一结论目前**不能定论**。
- 附注：官方发布博文提到的 "The numbers for LLMs are from OpenRouter" 是指其评测中的 LLM 基线，**不表示 Jev 已在 OpenRouter 上架**。

**Rust 客户端相关**：未发现官方 Rust SDK。据官方 API 参考，接入所需信息为：单一端点 `POST /v1/systemone`、Bearer 鉴权、问题/答案以调用方自定义 key 一一对应（**key 不发送给模型**）、错误码 401/422/429/529，429 与 529 需指数退避并遵循 `retry-after`。https://docs.typesafe.ai/api

**依赖风险的叠加结论**：无 SLA + 状态页 99.840%（含 59 分钟级中断）+ 上线期曾无法服务用户 + 合同明确「API 更新可能导致不兼容」+ 预付费不可退 + 闭源无自托管 + （有待核实）第三方路由层 → harness 必须默认具备**超时预算 + 429/529 指数退避 + 熔断降级**（降级路径可回落确定性规则或 LLM 判定），并把不确定性作为一等数据向上传递；模型别名会漂移，应固定 `jev-1.13.0` 这类版本号，由 harness 自己控制升级节奏。

### 3.5 本节不确定项

1. **估值与轮次规模有冲突数据**：Forbes 文章标题为「This $200 Million Startup Wants To Fix AI's Overconfidence Problem」（https://www.forbes.com/sites/the-prompt/2026/09/15/this-200-million-startup-wants-to-fix-ais-overconfidence-problem/ ），但**两次尝试均无法提取正文**（403 / 提取失败），「$200M」指估值还是其他数字**未能证实**；dealroom 页面文本中出现 "US$25.9M seed round" 字样，与 $40M 不一致且**未见解释**。→ **唯一可引用的融资数字是「约 $40M 种子轮，DCVC 领投」（Business Wire 新闻稿原文）**。
2. **模型架构未公开**：官方不披露参数量、架构、硬件、训练配方、模型卡、权重。TechCrunch 称「outside observers suspect is built on top of an open-weight LLM」——**属记者转述与猜测，Almeida 拒绝透露**，不可当事实。
3. **RLCD 无公开技术细节**：官方称 RLCD（Reinforcement Learning for Calibrated Decisions）为自研训练法、数据全合成，但**未发表论文/技术报告**。「概率是否校准」目前只有厂商声称与文档建议（在自有数据上测阈值），**无第三方校准曲线**。
4. **SOC 2 / 认证状态未证实**：Trust Center 抓取只返回 Vanta 前端 JS 壳，无法读取子处理器清单与合规条目；「子处理器为 AWS（存储/处理）、Modal（推理）、Slack、Google Workspace（支持），全部美国公司」这一点**仅见于第三方，未见一手来源**。
5. **无官方 SLA 文本**：MCA 中确认无 SLA（已核对）；**是否存在单独的企业级 SLA 附件**（仅签约客户可见）**无从核实**。
6. **定价可持续性未证实**：官方自认「无法证明定价未被补贴，长期会证明其可持续性（预期价格会降不会升）」；播客中创始人称「我认为我们会在很长时间内受限于 GPU」。**$0.042/Mtok 是否为长期价格，无独立证据**。
7. **多数评测数字是厂商自评**：193.6×/444.6×、70–500ms、「无法幻觉」等均出自官方，官方自己声明这些数字偏高端、评测在美西、任务由自家模型能力团队构造（存在偏倚）。较独立的量化对照是 Every 的 777/1709 次实调用（时间与成本）与 kingy.ai 的文档复核（准确率不占优）。
8. **未找到公开信息**：公司营收/ARR（只有创始人「上线前几乎无收入」口述）；员工确切人数（只有 LinkedIn 区间）；是否有后续融资轮；官方 Rust SDK；私有化部署方案；正式 AUP（可接受使用政策）与高风险用途限制条款（第三方指出四份文件中**没有**禁止清单或人类监督要求）。
9. **第三方来源质量提示**：检索到的相当一部分「分析」文章（kingy.ai、wunderlandmedia、truefoundry、mindstudio、innfactory、requesty、opper 等）是发布后数日内产出的内容营销/SEO 文章，含**可验证的错误**（见 3.3 末）。引用时优先官方文档与条款原文，第三方仅用于印证与批评视角。

---

## 四、社区争议与采用动态

**要点**

- **讨论规模确实很大，但热度不等于验证**：HN 主帖《Introducing System One Models and Jev》发布于 2026-09-15 19:25:03 UTC，检索时（2026-09-22T02:54Z）为 **1945 分 / 511 条评论**（同一篇评论文章写作时记录为 **1430 分 / 404 评论**——引用必须带时间戳）；Latent Space 称发布视频在 X 上约 **4000 万次观看**；中文侧 9/16–9/21 报道密集，机器之心相关推文话题被称 **3700 万人围观**。【第三方】
- **社区质疑集中在五条主线上，其中三条被官方自己部分承认**：①「不能幻觉」被普遍解读为**类型安全 ≠ 事实正确**（官方博客自己注明幻觉图的 0% **不是实测数据，是 schema 保证**）；②**速度/成本对比是苹果比橘子**（Jev 不做自回归生成，拿它比逐 token 生成的延迟被指营销误导）；③「前沿模型」标签与首个标题被指误导（原帖标题曾为 *"Jev: New frontier model 40-400x cheaper and 20-200x faster"*，不到一小时被改掉）；④**架构不新**（GLiNER / ModernBERT / 零样本分类器一脉）；⑤**概率是否真的校准、是否稳定**。
- **官方主动放弃了公开基准**：博客《Lies, Damned Lies, and Benchmarks》宣布「**不在模型发布中放标准基准表**；新 evals 只做带日期的快照、发布后即淘汰、不再 hill-climb」，同时公布内部 evals 并附不利证据。→ 这既是回应质疑，也意味着**外部无法用公开 benchmark 独立比对**。【官方】https://typesafe.ai/blog/antibenchmaxxing
- **校准与稳定性上官方与第三方正面冲突**：官方 jaggedness 文档**自己列明**算术/计数、日期比较、多跳、无关信息、对抗输入、矛盾指令下不可靠，并明说**不保证结构不变量**（同一问题用 Noul 与 Choice 问会给出互相矛盾的数与置信度）；第三方（8 个真实数据集）称其**校准落后 CatBoost**（二手转述，未独立复现）。【官方】https://docs.typesafe.ai/model-jaggedness/jev-1.13 ；【未证实】https://m.sohu.com/a/1079019769_129720
- **采用动态已从候补制转为全面开放**，并迅速铺开分发渠道：官方公告取消候补、注册送 $5 额度（≈1.2 亿 token）；第三方渠道包括 Cloudflare Workers AI（`typesafe/jev`）、LangChain（`langchain-typesafe`）、Braintrust；社区出现多份「复刻 Jev」项目与上下文压缩插件。【第三方（官方 X 公告未能直连核对）】
- **社区共识偏中性**：「**架构不新，但零样本通用性 + 校准 + 并行接口 + 极低延迟这套组合少见**」；最强烈也最未获满足的情绪是**开源与本地化诉求**（官方**未承诺**开放权重或本地部署），而 32k 上下文、闭源、无本地部署是硬约束。

### 4.1 讨论规模与时间线

| 事实 | 数值/语境 | 出处 |
|---|---|---|
| HN 主帖 | 标题 *Introducing System One Models and Jev*，作者 `albelfio`，2026-09-15 19:25:03 UTC，score **1945**、评论 **511**（2026-09-22T02:54Z 读取） | HN API https://hacker-news.firebaseio.com/v0/item/49717558.json ；帖子 https://news.ycombinator.com/item?id=49717558 |
| 早期快照 | 一篇评论文章写作时记录为 **1430 分 / 404 评论**（说明热度仍在增长，引用数字需带时间戳） | http://wimes.org/articles/2026-09-16-typesafe-jev-cant-hallucinate-exactly-the-problem/ |
| 发布视频播放量 | "launch video now viewed **~40M times**"，并与 GPT4o 22M、Fable 5 15M、Astra 137M 对比 | https://www.latent.space/p/jev |
| 中文侧热度 | 「相关推文已有 **3700 万人围观**」 | https://k.sina.com.cn/article_5953466437_162dab0450670bd10e.html （机器之心原稿） |

### 4.2 五条质疑主线与官方/第三方的相互印证

**① 速度/成本对比与「前沿模型」标签**
- 【第三方·HN 高信号评论】*"A generative model that can output code in a Turing-complete language can do anything a computer can do. Jev can only generate structured output… this is probably super useful for classification/routing/scoring, but it's nothing like the code generating models"*（HN 首条评论）https://news.ycombinator.com/item?id=49717558
- 【第三方·可定位引文】`WhitneyLand`：原帖标题曾为 *"Jev: New frontier model 40-400x cheaper and 20-200x faster"*，**不到一小时前被改掉**…「我不认为多数人会把 Jev 算作前沿模型，这听起来是假的」，并给出他认为更准确的措辞 *"Advanced the speed/cost frontier for structured decisions"*：https://news.ycombinator.com/item?id=49719001 、https://news.ycombinator.com/item?id=49719144
- 【第三方·可定位引文】`bigglebear`：*"It's a **wildly dishonest presentation** of their product from many perspectives… The comparison between LLM speed and Jev speed is misleading, because they're using autoregression to generate all of the type names, all of the schema… You really have to do a lot of hand holding here"*，并称分布刻画不对结果可能是 garbage：https://news.ycombinator.com/item?id=49720601
- 【第三方·Demo 反驳】HN 与 Reddit 均指出 Doom demo 用的是**结构化文本状态**（坐标、血量、弹夹），不是画面；**官方自己也承认**：*"The demo is on structured state as a data structure with text, **not on images** (yet…)"*、*"A **non-AI doom bot could play better**, but we wanted a bot that was reactive to different representations of game state"*：https://typesafe.ai/blog/introducing-system-one-models-and-jev
- 【第三方·用官方自己数据反驳】Customer Service 工作流上 Jev **76.0%（$0.0001 / 0.4s）**，而同页 GPT-5.6 Sol **78.3%（$0.0323 / 10.1s）**、DeepSeek V4 flash 76.8%（$0.0029 / 34.6s）→ Jev **明显更便宜更快，但并非最准**：https://evals.typesafe.ai/customer_service.html
- 【官方口径】193.6×/444.6× 来自官方 workflow evals，官方主动标注三点偏差：参照取 **GPT-6 Astra 与 Fable 5.1 的平均**（偏向 OpenAI/Anthropic）、工作流由自家能力团队制作（可能偏差）、数字「处于真实收益的高端」：https://typesafe.ai/blog/introducing-system-one-models-and-jev

**②「不能幻觉」的语义之争**
- 【第三方·可定位引文】`WhitneyLand`：*"**Type safety is not factual correctness.**"*（同上 HN 帖）
- 【第三方·HN 长评】*"Your launch post puts '0%' on a hallucination chart, then explains that the number comes from **guaranteed schema matching**. You've already agreed that this doesn't establish correctness. **An approve for an unauthorized action still meets the schema guarantee.**"*（同帖评论区）
- 【官方·自我披露】官方博客在幻觉/类型安全一节主动注明：*"**Our number is not empirical.** Schema matching is guaranteed, thus we can confidently add 0% into the plots."*（社区对此反应反而偏正面：承认这一点比宣称「零幻觉」更可信。）
- 【第三方·KDnuggets】*"**'Zero hallucinations' is therefore closer to zero out-of-schema outputs, not zero incorrect decisions.**"* https://www.kdnuggets.com/what-everyone-is-getting-wrong-about-typesafe-ais-jev
- 【第三方·治理视角】一篇评论文章把「无文本输出」视为新的可审计性缺口（律师/保险人/监管者无「可引用的陈述」）——属**观点文章**，其中引用的「某支付机构模型风险工程师 Slack 对话」**无法核实**：http://wimes.org/articles/2026-09-16-typesafe-jev-cant-hallucinate-exactly-the-problem/

**③ 架构与「是不是新东西」**
- 【第三方·可定位引文】r/LocalLLaMA `logical_Two_7736` 反驳「一年前就做过」：*"Your SalesRLAgent is essentially a **sequential PPO policy** … Jev is more general. One state can be queried with several independently typed questions, and it can emit distributions for categorical choices, scores and booleans **in parallel**."* https://www.reddit.com/r/LocalLLaMA/comments/1wijo3e/ （经 pullpush 存档读取）
- 【第三方·KDnuggets】*"Classification, scoring, routing, and intent detection are not new machine learning problems… The problem is old; the architecture and product around it may be new."*，并指出「我们仍几乎不知道 Jev 的内部架构、训练设置与模型规模，因此最大的主张高度依赖 TypeSafe 自己的基准」。
- 【第三方·Reddit】大量评论将其类比 GLiNER / GLiClass / ModernBERT / 零样本分类器，并给出具体开源替代品 https://huggingface.co/knowledgator/gliclass-instruct-base-v1.0 、https://huggingface.co/fastino/gliner2.5-base-v1 （见 https://www.reddit.com/r/LocalLLaMA/comments/1wlfmgq/ 、https://www.reddit.com/r/LocalLLaMA/comments/1whxf90/ ）
- 【未证实·中文媒体转述】ML 工程师 Sebastian Raschka 认为「把 Jev 简单看成分类器会低估它」，关键可能在**数据与 API 设计**，训练算法本身未必复杂；文中另称「有网友表示创始人曾暗示 Jev 使用完全合成数据集、在已有模型上做后训练，未做大规模预训练」：https://m.sohu.com/a/1079019769_129720 （引 https://x.com/rasbt/status/2101672304358948992 ）。**X 原帖未能直连核对（登录墙），视为二手转述。**
- 【社区共识（偏中性）】「架构不新，但**零样本的通用性 + 校准 + 并行接口 + 极低延迟**这套组合确实少见」；r/LocalLLaMA 用户 `Aloekine`：*"JEV is meaningfully more general in the sense that its baseline performance across problems it hasn't seen yet is really, truly impressive."*

**④ 概率校准与稳定性（官方与第三方正面冲突）**
- 【官方·自述局限 `jev-1.13`】jaggedness 文档列 9 类失败模式并给替代做法：字面理解、**算术/计数**（*"does not count reliably… the error grows with the size of the thing being counted"*）、**日期时间比较**、多层间接推理、大 state 干扰、**Adversarial content**（state 是数据，jev-1.13 默认不把 state 当敌意输入，可移动答案）、矛盾指令、**常识结构不变量**、生成任务：https://docs.typesafe.ai/model-jaggedness/jev-1.13
- 【官方·结构不变量反例（关键）】同一问题两种问法互相矛盾：`Noul` 得 0.22，`Choice yes` 得 0.01（该 Choice 的 confidence 0.97）；*"Is the customer asking for a refund?"* 与 *"Is the customer asking for something other than a refund?"* 两个 Noul 得 **0.72 / 0.47（和 = 1.19）**；官方结论：*"**don't rely on expected structural invariance**… don't hold the model to arithmetic identities between separate questions"*（同页 URL）
- 【未证实·第三方校准反例】中文报道称 X 用户 `@predict_addict`（Valeriy M.）在**贷款违约、企业破产、医疗诊断等 8 个真实数据集**上测试，结论为 **Jev 的概率校准明显落后 CatBoost，且经常高估负面结果**，简单后处理可修正部分误差；报道另记录**同一提示多次运行概率不同**、**选项顺序会大幅改变输出概率**（把「信息咨询 / bug 报告」换序后，模型从偏向 bug_report 变成偏向 information）。出处 https://m.sohu.com/a/1079019769_129720 。**未找到原始数据/代码，按「未独立复现」处理。**
- 【第三方·LangChain】5 条固定 agent 输出 × 100 次重复：二元判定与人工 oracle **500/500 一致**；**每例评分方差 0.0000149**；单次成本 **$0.00035**；局限为样本极小、「观察性、非因果」、且 **Jev 服务版本未记录在实验元数据中**：https://www.langchain.com/blog/jev-agent-evals-langsmith
- 【第三方·Braintrust】*"**Confidence is separate from the selected option's probability. A high value does not guarantee that the decision is correct.**"*，并给出示例：Jev 对「编造退款/额度」的草稿给出 `Incorrect, unsafe…` 标签，confidence 仅 **0.57**、probability 0.68；建议按置信度分级路由而非一刀切：https://www.braintrust.dev/blog/evaluate-agent-responses-with-jev

**⑤（优先级最低但存在）本地/开源诉求与营销指控**
- 【第三方】HN 评论出现「closed source 令人不安」「这种进展不该被锁在本地硬件之外」；官方**未承诺**开放权重或本地部署 https://news.ycombinator.com/item?id=49717558
- 【未经证实的指控】Reddit 有用户称 *"they ran an **astroturfed marketing campaign** all over twitter for last 2 days"* https://www.reddit.com/r/LocalLLaMA/comments/1whxf90/ ；HN 也有人称「Twitter 评论像是 paid partnerships」。**属情绪化指控，无证据，仅记录其存在。**

### 4.3 采用动态与社区生态

| 项目/实测 | 数字与语境 | 出处 |
|---|---|---|
| 全面开放 | 官方公告取消候补、所有人可注册、$5 起步额度 ≈ 1.2 亿 token（入口 console.typesafe.ai） | 中文转述 https://m.sohu.com/a/1079019769_129720 、https://k.sina.com.cn/article_1644983660_620c756c02001vwuw.html ；**官方 X 公告未能直连核对（登录墙）** |
| Cloudflare Workers AI | 模型 `typesafe/jev`（返回 `jev-1.13.0`），**32,000 token 上下文**，含 noul/choice/score 三类示例与完整响应体 | https://developers.cloudflare.com/ai/models/typesafe/jev/ |
| OpenRouter | `typesafe/jev-1.13`，`context_length 32000`，pricing prompt `0.000000042`（=$0.042/M）、completion `0`，provider=TypeSafe，`uptime_last_1d` 100%（**与 12-company.md「未上架」结论冲突，见 3.4**） | https://openrouter.ai/api/v1/models/typesafe/jev-1.13/endpoints （2026-09-22 读取） |
| LangChain 集成 | `langchain-typesafe`：`TypeSafeClassifier`、`ModelRouterMiddleware`（按难度分流模型）、`AutoModeMiddleware`（工具调用前用 Jev 做风险门控） | https://www.langchain.com/blog/building-a-harness-with-jev |
| Braintrust | 把 Jev 作为 judge/scorer 接入（Trace span 名 `typesafe.systemOne`），保留 label/confidence/probabilities 供复核 | https://www.braintrust.dev/blog/evaluate-agent-responses-with-jev |
| 浏览器 Agent 实测（HN 用户 `silbercue`） | 把可点击元素（accessibility tree，约 10–40 个 ref）作候选项，**21–23 次决策、六个 benchmark 卡全对、总计约 $0.001**；文本输入仍需一个小模型 | https://news.ycombinator.com/item?id=49758669 |
| 上下文压缩插件 | `tamaratran/fast-jev-compaction`：Claude Code 插件，用 Jev 给工具调用打分以替换 compaction 摘要；GitHub **6013 stars**（created 2026-09-17） | https://github.com/tamaratran/fast-jev-compaction |
| 其他社区复刻/衍生 | `vinnylarouge/jevlike`（1169★）、`TheoLeeCJ/SemIf`（3316★，"Semantic ifs from open models, on a 3090 at home"）；另有多个 `awesome-jev` 清单仓库 | GitHub API 检索 `q=jev+typesafe` |
| 媒体转述的实测数字 | 724 条广告 / 37 品牌 / 8724 次判定 / 约 40 秒 / 约 $0.09 token 成本（Matthew Berman）；浏览器航班搜索 demo 约 7 秒、约 $0.004；模型路由实测 Jev ~1s vs 结构化输出 LLM 4–14s | https://k.sina.com.cn/article_5953466437_162dab0450670bd10e.html 、https://www.kdnuggets.com/what-everyone-is-getting-wrong-about-typesafe-ais-jev （均为二手汇总，原始 X 帖未直连核对） |
| 竞品/对照 Laya | Laya（400M 级、"think layer on ModernBERT"，自称对标 Jev）——Reddit 多方评价其「不微调基本是猜」，与 Jev 的零样本通用性不同层级 | https://laya.convaiinnovations.com/ ；https://www.reddit.com/r/LocalLLaMA/comments/1wlfmgq/ |

**工程化用法（可对照本项目 harness 设计）**：
- Braintrust 建议的**置信度阈值分级路由**：**>0.95 自动接受；0.70–0.95 交给更强的 LLM 判官；<0.70 视为不确定或转人工**，并强调阈值必须先用自有数据验证、不能照搬。https://www.braintrust.dev/blog/evaluate-agent-responses-with-jev
- LangChain 的 `AutoModeMiddleware(tools=["bash"])` 形态：**在工具执行前用 Jev 做风险门控**；原文点明「这类分类器步骤以前一直锁在闭源 harness 内部，现在有了便宜且性能足够的分类模型，可以把同样的模式用到所有 agent 上」。https://www.langchain.com/blog/building-a-harness-with-jev
- 社区做法 `fast-jev-compaction` 用 Jev 打分来裁剪上下文——对应本项目 harness 的上下文组织职责。https://github.com/tamaratran/fast-jev-compaction

### 4.4 本节不确定项

1. **X/Twitter 内容无法直连核对**（登录墙；nitter 镜像亦不可用）。所有 X 相关数字与观点（Raschka 评价、`@predict_addict` 的 8 数据集校准测试、各种实测 demo 数字）均来自中文媒体汇总或 HN 转述，**只能作为「存在此类说法」的线索，不能作为一手证据**。
2. **HN 帖分数/评论数随时间变化**：一手读取（2026-09-22）为 1945/511；早前评论文章记为 1430/404。引用必须带时间。
3. **Reddit 分数不可靠**：经 pullpush 存档 API 取得的评论 `score` 多为 1–2，明显滞后或失真，故只引评论**内容与作者名**，不引票数。
4. **未核实的数字**：cnblogs 称 OpenRouter 上「上线几天内已处理超过 **1610 亿 token**」（用 OpenRouter API 只能确认模型、上下文与定价，**未能核实用量**）https://www.cnblogs.com/sing1ee/p/23040079 ；KDnuggets 称「自家 workflow 评测约 68%」「一项小规模 fact-checking 测试 96.3%」「275 份文档测试强一致」——**均未给出一手来源**，且官方 evals 上 Customer Service 是 76.0%，两者关系不明、**不可混用**；cnblogs 称输入价「比 Claude Fable 5.1 低 238 倍」——**未在官网原文中核实到该表述**；Vercel AI Gateway 免费至 9/25（**【未证实】**，仅见搜索摘要与 HN 单条评论）、Swamp 扩展（仅 HN 用户自述）、`qibinlou/jev-chess`（GitHub API **返回 404，仓库不存在，不可引用**）。
5. **「我一年前就实现了 Jev 架构」这一先例主张存在争议**：作者 Nandakishor M 的 arXiv 论文确实存在且日期吻合（*SalesRLAgent*，2025-03-30 提交 https://arxiv.org/abs/2503.23303 ），但同帖高赞反驳指出其是「顺序 PPO + 单一连续转化概率」，与 Jev 的并行多类型问题不等价；r/LocalLLaMA 亦有技术用户质疑其论文未同行评审、方法学有缺陷。**结论：可引用「社区对是否存在先例存在争论」，不可断言 TypeSafe 抄袭。**
6. **架构细节仍属猜测**：社区推测（编码器+回归头、文本扩散、约 3B 参数、并行 prefill 取单 token 概率等）**均无官方确认**；官方未公布参数量、架构细节与训练数据来源。
7. **官方对基准质疑的正式回应带来可验证性缺口**：博客《Lies, Damned Lies, and Benchmarks》宣布不在发布中放标准基准表、新 evals 只做带日期快照并附不利证据——这是官方主动放弃公开榜单，**也意味着外部无法用公开 benchmark 独立比对**：https://typesafe.ai/blog/antibenchmaxxing
8. **疑似 AI/SEO 批量生成的内容农场**（命中但不采用其数字）：explainx.ai、alphamatch.ai、cryptobriefing.com（称「对抗性漏洞引发安全担忧」）、modemguides.com、cellcog.ai、medium 系列等，这些站点数字与其他来源不一致且无法追溯原始测试。**社区「实测」普遍自述、缺方法与数据**（如「一晚做出实时交易机器人，已亏 $31680」），仅可作轶事。
9. **`jev-1.13` jaggedness 页的 URL 在上游两次调研中不同**：`10-alternatives.md` 按 `docs.typesafe.ai/models/jev-1.13-jaggedness` 抓取返回 404（该页内容当时经 Langfuse 转述）；`13-community-debate.md` 则成功抓取 `https://docs.typesafe.ai/model-jaggedness/jev-1.13` 并逐条引用。两者是否同一页面、官方是否改过路径，**未核实**；本节 jaggedness 的引文来自后者（可访问的那一个）。
