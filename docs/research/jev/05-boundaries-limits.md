# 专题 05：能力边界、限制与失败模式（Jev / TypeSafe AI）

- 调研日期：2026-09-22
- 模型版本口径：官方文档当前为 **jev-1.13 / jev-1.13.0**（别名 `jev-latest`、`jev-preview`）；部分官方 cookbook 实测跑在 **jev-1.12** 上，引用时已标注
- 标注约定：**【官方】**＝TypeSafe 官方文档/博客/评测站；**【第三方】**＝外部评论、实测、社区数据

---

## 一、结论要点（先看这 10 条）

1. **Jev 不是生成模型，官方口径明确**：不写回复、不产代码、不给出推理过程解释；只有三种带概率的类型化答案（Choice / Score / Noul）。【官方】https://docs.typesafe.ai/concepts/system-one
2. **"不能幻觉"是形状保证，不是正确性保证**。官方在博客里自己写明 0% 幻觉率 **"not empirical"**（非实证），是按 schema 保证推导出来的；官方文档同时写明「校准是对一组预测的统计描述，**不保证单个答案正确**」。【官方】https://typesafe.ai/blog/introducing-system-one-models-and-jev ，https://docs.typesafe.ai/introduction/machine-learning-primer
3. **官方自列 9 条失败模式**（"jaggedness" 页，jev-1.13）：字面理解、数学与计数、日期时间比较、多跳间接推理、大而嘈杂的 state、对抗性内容、指令与 criteria 冲突、结构性不变式不成立、生成文本。【官方】https://docs.typesafe.ai/model-jaggedness/jev-1.13
4. **官方明确不保证结构性不变式**：同一问题的 Noul 与 Choice 结果可以差很多（官方给的 refund 例：Noul=0.22 而 Choice “yes” 概率=0.01），且 `P(noul)` 与 `1 - P(not_noul)` 不可比（官方例：0.72 + 0.47 = 1.19）。阈值不能跨问法搬运。【官方】同上
5. **硬限制**：单请求 64k token 预算；**state + 最长问题 ≤ 32k**；Choice 最多 **255** 个选项；Score **2–10** 档；输入仅文本（不支持图像/音频/视频）；英文最佳，CJK 可用但不等价。【官方】https://docs.typesafe.ai/models ，https://docs.typesafe.ai/api ，https://docs.typesafe.ai/primitives/choice ，https://docs.typesafe.ai/primitives/score
6. **置信度只存在于 Choice / Score；Noul 没有 confidence 字段**，其概率本身表示是非不确定性。【官方】https://docs.typesafe.ai/confidence
7. **官方实测给出「置信度能排序难易、但不等于正确」的量化证据**：60 份 SEC 10-K 分类中，confidence ≥ 0.9 的一半准确率 **90%**，另一半只有 **40%**（低置信那一半改报上一层大类可升到 **70%**）。【官方】https://docs.typesafe.ai/cookbooks/classification_using_confidence
8. **官方评测站的真实数字**（Jev workflow 模式）：Security Incidents **61.7%**、Agent Trace Observability **71.6%**、Invoice Processing **61.8%**、Customer Service **76.0%**，四任务等权聚合 **67.8%**；同表 GPT-5.6 Terra（workflow）67.9%、Sol（workflow）74.1%。**Jev 在成本与延迟上压倒性领先，但不是准确率领先者**。【官方】https://evals.typesafe.ai/
9. **第三方对"零幻觉"的批评基本一致**：不能输出非法类型 ≠ 不会输出错误的合法值；且官方**未公布任何独立校准曲线/ECE**，所以「校准」这一最关键主张目前仍是未经验证的官方口径。【第三方】https://www.theregister.com/ai-and-ml/2026/09/16/typesafe-ai-debuts-model-for-machines-that-plays-doom/5296711 ，https://www.laura-martel.com/blog/typesafe-jev-system-one-models ，https://kingy.ai/blog/typesafe-jev-review-the-ai-model-that-doesnt-generate-text/
10. **对 Agent Harness 的关键含义**：Jev 适合当"带概率的语义判定原语"，不适合当推理器、生成器、计数器或安全边界；概率与置信度必须按一等数据保留，但要按**问法各自**校准阈值，并预期**组合后的置信度没有理论保证**。【综合】

---

## 二、Jev 明确不做什么

| 不做的事 | 原文/出处 |
| --- | --- |
| 不生成文本、不写代码、不做对话 | 【官方】"System One models do not write replies, produce code, or generate explanations of their reasoning." https://docs.typesafe.ai/concepts/system-one ；"Jev is not a chat or code-completion LLM… It does not generate text, write code, or hold a conversation." https://docs.typesafe.ai/introduction/coding-agents |
| 不替代驱动 coding agent 的 LLM | 【官方】"Jev is not a drop-in replacement for the LLM behind Claude Code, Cursor…" 同上 |
| 不做工具调用/文件编辑/流式输出 | 【官方】"Coding agents rely on an LLM that streams text, calls tools, and edits files… Jev does none of that." 同上 |
| 不做算术、计数、日期运算 | 【官方】"Jev is not a calculator. We strongly recommend implementing any mathematical logic in code."；"jev-1.13 does not count reliably… The model recognizes the shape of an answer rather than tallying" https://docs.typesafe.ai/model-jaggedness/jev-1.13 |
| 不做多跳推理（能力受限而非完全不能） | 【官方】"It may struggle with tasks that require additional levels of indirection."；"Instructions carrying double negatives or complex indirection are answered less reliably." 同上 |
| 不保留问题之间的上下文 | 【官方】"All questions see the same state and are evaluated independently."（单个请求内问题互不成为彼此的上下文；有真实依赖需拆成两次请求）https://docs.typesafe.ai/concepts/state |
| 默认不把 state 当作敌意输入 | 【官方】"State is data, and jev-1.13 does not treat it as hostile by default. Content written to adversarially steer the model… can move the answer." https://docs.typesafe.ai/model-jaggedness/jev-1.13 |
| 不做按客户微调 | 【官方】"Jev is not fine-tuned or LoRA-adapted with customer data… the same weights serve every account." 定制只能通过请求里的 state / instructions / criteria https://docs.typesafe.ai/models |

补充：官方演示（Doom、Wikiracing）里作者也标注了**对比条件对自己有利**：Wikiracing 的 LLM 对手被设成 non-reasoning 模式（"to make the demo more bearable to watch"），Doom 输入是结构化文本而非图像。【官方】https://typesafe.ai/blog/introducing-system-one-models-and-jev

---

## 三、不适合交给 Jev 的任务

按官方 jaggedness 页 + 官方 pattern 文档整理，以下任务类型**不应**指望 Jev：

1. **任何需要生成文字/代码/解释**的环节 → 用生成模型（官方第 9 条）。
2. **算术与"档位之间的精确数值"**。官方明确警告：不要用 Score 的期望值（expectation）去**插值还原**两个档位之间的具体数值；Score levels 的数值校准很弱，只能用来判"是否过某个阈值"。【官方】jaggedness 页 "Math using score"
3. **计数**（字符数、词频、长列表条目数）→ 在代码里数；需要按条件计数时，逐个候选各问一个 Noul，再在代码里求和。
4. **日期/时间排序与差值**："jev-1.13 reads dates as text, not as ordered quantities." 官方建议：抽取归模型（把年/月/日做成封闭集合的 Choice），比较归代码。
5. **低层/数值化表征**：hex 颜色、RGB 三元组、汇编/二进制编码，表现明显弱于自然语言名称；颜色"两个值是否接近"这类判断不可靠。
6. **多跳、双重否定、属性之属性** → 减少跳数、点名 state 字段。
7. **大而嘈杂的 state**：无关细节起干扰作用，且出了问题难以定位；官方建议先在代码里筛选/检索，只送问题需要的字段。
8. **高基数选择 > 255**：Choice 上限 255 选项，更高基数需要两级（先独立打分再显式选择），官方承认这会**变慢**。
9. **需要"绝对可靠"的判定**（尤其安全边界）：官方 RAG 注入 cookbook 明确写 "The injection question is a filter, and only one… **Nothing here is a security boundary.**"【官方】https://docs.typesafe.ai/cookbooks/classifying_rag_passages
10. **承受恶意输入的判定**（如以 Jev 为唯一护栏拦截 prompt injection）：官方自述对抗性内容可移动答案，建议写精确 criteria 并自行做边界测试。

第三方总结的"Jev 赢 / LLM 赢"清单可作交叉参考：LLM 仍赢在需要生成文本与代码、多跳推理、算术/计数/日期算术、以及"量少但一次错就很贵"的任务。【第三方】https://www.firecrawl.dev/blog/what-is-jev

---

## 四、state 长度与类型限制（可硬编码进 Rust 类型系统）

| 限制项 | 官方数值 | 出处 |
| --- | --- | --- |
| 单请求总预算 | 64k tokens（state + 所有 questions 合计） | https://docs.typesafe.ai/models |
| state + 最长单个问题 | ≤ 32k tokens | 同上 |
| Choice 选项数 | ≤ 255（API 硬上限） | https://docs.typesafe.ai/api ，https://docs.typesafe.ai/primitives/choice |
| Choice 选项数（**实用**上限） | 官方 cookbook 另写 "a Choice works reliably up to roughly **240** options"（75 类行业组一次问完，远在 255 之内） | https://docs.typesafe.ai/cookbooks/classification_using_confidence |
| Score 档位数 | ≥ 2，≤ 10 | https://docs.typesafe.ai/api ，https://docs.typesafe.ai/primitives/score |
| state 形态 | `string` / JSON `object` / JSON `array`（数组为文本值数组）；"Input: Text only… No image, audio, or video input." | https://docs.typesafe.ai/models ，https://docs.typesafe.ai/api |
| 非文本输入 | 必须自行预处理为文本或结构化字段（图像/音频/视频/二进制不支持） | https://docs.typesafe.ai/models |
| 语言 | 英语是主要训练语言且准确率最好；其他语言（含 CJK）"handled but not equally well"，需自测 | 同上 |
| 限流 | 250,000 tokens/秒 与 1,200 请求/分钟；超限返回 **429**；另有 **529 Overloaded**；SDK 默认带退避重试并遵循 `retry-after` | 同上，https://docs.typesafe.ai/api |
| 错误语义 | 401 鉴权失败；**422 请求体校验失败**（缺字段/问题畸形，body 指出字段）；429 / 529 退避重试 | https://docs.typesafe.ai/api |
| 采样参数 | **没有 temperature/top_p 等采样旋钮**（Pydantic AI 集成文档明说这些设置被忽略） | 【第三方（框架集成文档）】https://pydantic.dev/docs/ai/models/typesafe/ |
| 版本漂移 | `jev-latest` 会随发布移动；官方建议调过阈值的场景 pin 到版本化 ID（如 `jev-1.13.0`），响应里 `model` 字段回报实际版本 | https://docs.typesafe.ai/models |

**实际可用容量小于纸面上限**：官方 jaggedness 第 5 条说明 state 越长越嘈杂、准确率越低。官方 parallel-questions cookbook 里"文档为主"的示例是 GDPR 维基全文 **约 54,000 字符**（≈1.3–1.4 万 token 量级），可作为"官方认为合理的大文档 state"的锚点。【官方】https://docs.typesafe.ai/cookbooks/parallel_questions

另有一个实现级细节：官方 entity-alignment cookbook 里并发池设为 `MAX_WORKERS = 6`，注释写 "the public endpoint rate-limits above roughly eight"（约 8 并发以上开始限流）。【官方】https://docs.typesafe.ai/cookbooks/entity_alignment

---

## 五、置信度与校准的可靠性边界

这是本专题最需要谨慎处理的部分，因为**官方主张（校准）与公开证据（无独立校准曲线）之间存在明显缺口**。

### 5.1 官方怎么说

- **confidence 是什么**：由概率分布形状导出的 0–1 统计量，只出现在 Choice / Score 上；分布越集中越高。Noul 不带 confidence。【官方】https://docs.typesafe.ai/confidence
- **confidence ≠ 正确性**：Score 文档原文 "confidence 1.0 means the returned distribution puts all its probability on one level. **This describes the model's answer, not a guarantee that the answer is correct.**"【官方】https://docs.typesafe.ai/primitives/score
- **校准的定义被限定为群体统计**："Outcomes assigned a probability of 0.8 should occur about 80% of the time… **These rates describe groups of predictions, not a guarantee about any single answer.**"【官方】https://docs.typesafe.ai/introduction/machine-learning-primer
- **官方给的可操作做法**：把 confidence 划三段（高→自动执行、中→复核/追问、低→转人）；阈值随风险调整，0.5 只是"模型自报不确定"的地板。【官方】https://docs.typesafe.ai/confidence

### 5.2 官方拿得出的量化证据（最有用，也最有边界感）

| 证据 | 数字 | 语境与出处 |
| --- | --- | --- |
| 置信度分层确实有效 | 60 份 10-K，75 类 SIC 行业组，单条 Choice。confidence ≥ 0.9 的一半准确率 **90%**；< 0.9 的一半仅 **40%**；后者改报 division 升到 **70%** | jev-1.12，2026-08-12，官方 cookbook：https://docs.typesafe.ai/cookbooks/classification_using_confidence |
| 同 state 重复调用稳定性 | 14 个 Noul × 15 次重复，TypeSafe 每问题概率标准差 **0.0102**（低于同测的所有 LLM 概率条件）；但 `covered` 一题在 **0.43–0.53** 间波动、跨越 0.5 决策线 | jev-latest，2026-09-11，官方 cookbook：https://docs.typesafe.ai/cookbooks/consistency_noul_cookbook |
| 官方对不确定区间的自我限定 | 0.30–0.70 的"不确定带"被明确标注为 "**neither a calibrated guarantee nor an optimized threshold**" | 同上 |
| 官方对 LLM 校准弱点的动机陈述 | "Even if prompted for a confidence estimate, models tend to be overconfident and inconsistent." | 博客：https://typesafe.ai/blog/introducing-system-one-models-and-jev |

### 5.3 边界与风险（第三方指出的、官方未反驳的）

- **没有公开的校准曲线 / ECE / reliability diagram**。第三方评论把这一条作为核心质疑：官方只断言校准，未发布可核对的校准度量。【第三方】https://www.laura-martel.com/blog/typesafe-jev-system-one-models ，https://www.truefoundry.com/blog/typesafe-ai-jev ，https://kingy.ai/blog/typesafe-jev-review-the-ai-model-that-doesnt-generate-text/
- **官方自己的评测里，参考标签不是真值**，而是 GPT-6 Astra 与 Claude Fable 5.1（both at high thinking）回答的平均；评测站还写明 "**Assume the harness is correct**"。所以那批数字是"与两个强模型的一致度"，不是"与现实的一致度"。【官方】https://evals.typesafe.ai/
- **组合会破坏置信度语义**：单步 92% 的分类接 88% 的路由，并不等于 81%——真实可靠性取决于误差相关性，而"neither Jev nor the pipeline framework can report"。【第三方】https://temperaturezero.com/2026/09/16/jev-doesnt-hallucinate-it-decides-wrong/
- **结构性不变式不成立（官方自己给反例）**：同一诉求的 Noul 与 Choice 答案不可互换；`P(noul) + P(not_noul) = 1.19`。含义：**不要因为某个阈值在 Noul 上调好过，就搬到 Choice 上当默认值**。【官方】https://docs.typesafe.ai/model-jaggedness/jev-1.13
- **合法输入下也可能"自信地错"**：创始人本人在 HN 回帖承认 "because these models are probabilistic, it's also possible to be confidently wrong"。【第三方（HN 原帖，创始人身份发帖）】https://news.ycombinator.com/item?id=49717558
- **官方未公开**：RLCD 的具体实现、校准如何度量、分布偏移/长尾标签下概率是否仍校准、p95/p99 延迟与并发表现。【第三方整理】https://kingy.ai/blog/typesafe-jev-review-the-ai-model-that-doesnt-generate-text/

---

## 六、"零幻觉"成立的前提

官方原话与限定条件（【官方】https://typesafe.ai/blog/introducing-system-one-models-and-jev ）：

> "While Jev gives up string generation, it's optimized for structured outputs and can't hallucinate."

同页 Nuance 里自己拆台：

> "**Our number is not empirical.** Schema matching is guaranteed, thus we can confidently add 0% into the plots."

所以"零幻觉"成立的前提是：

1. **你问的是"输出是否越出我声明的 schema"**。选项只有 `billing / technical / sales` 时，它不会返回第四个标签（第三方对这条的具体表述是"It will not invent 'urgent_priority'. It will not return null. It will not return a float."）。这是一个可以被单个反例证伪的**构造性**保证——官方说"no type errors… mathematically impossible"，并欢迎用反例来证伪。【官方+第三方】https://docs.typesafe.ai/model-jaggedness/jev-1.13 ，https://temperaturezero.com/2026/09/16/jev-doesnt-hallucinate-it-decides-wrong/
2. **它不覆盖**：选错合法选项、把要点读漏、给错误答案一个很高的概率/置信度。第三方对此的措辞最准确：**"Can't hallucinate" is a narrower claim than it sounds**；The Register 直接说"which really isn't a fair comparison as its output is not natural language"。【第三方】https://www.firecrawl.dev/blog/what-is-jev ，https://www.theregister.com/ai-and-ml/2026/09/16/typesafe-ai-debuts-model-for-machines-that-plays-doom/5296711
3. **"类型安全"这件事本身并不新颖**：第三方指出靠 grammar-constrained decoding（OpenAI Structured Outputs、Outlines 等）在自回归 LLM 上早已能做到同等的形状保证；真正未被独立验证的是**校准**。【第三方】https://www.laura-martel.com/blog/typesafe-jev-system-one-models
4. **命名歧义提醒**：官方把训练法叫 **RLCD = Reinforcement Learning for Calibrated Decisions**，而 **RLCD 在 ICLR 2024 已另有一个含义**（Reinforcement Learning from Contrastive Distillation，Kevin Yang 等）。引用时务必写清是哪一个。【第三方】同上

**推荐在本项目文档里固定使用这组术语**（避免"零幻觉=零错误"的语义滑移）：

- **类型/结构幻觉**：按设计被消除（输出空间预先枚举）。
- **语义判断错误**：仍存在，必须自己测（官方评测站与 cookbook 都在测这个）。

---

## 七、误用会怎样（失败模式与后果）

| 误用 | 会发生什么 | 证据 |
| --- | --- | --- |
| 让 Jev 生成文本（例如靠串联 Choice 拼字符串） | 官方原话：可行但 "**will not work well and will be very slow**"；要抽取值时，正确做法是用正则/生成模型给候选，让 Jev 选 | 【官方】https://docs.typesafe.ai/model-jaggedness/jev-1.13 |
| 把 state 当安全上下文（例如拿网页/邮件原文直接问） | state 被当作数据，注入指令、误导性框架、"为自分类辩护的文字"都能移动答案；官方说未来才改进 | 【官方】同上 |
| 把 Jev 当安全边界（护栏） | 官方明确："Nothing here is a security boundary"；低于阈值的可疑文本仍会进 prompt，下游生成器必须继续把一切当不可信输入 | 【官方】https://docs.typesafe.ai/cookbooks/classifying_rag_passages |
| 问法里塞进真正想问的意图，而 instructions 写得太简 | "answers the question you wrote, not the one you meant"；边界情形应写进 criteria；官方建议：当你发现自己在解释"我其实想问的是…"，那半句话就是漏写的 instruction | 【官方】jaggedness 第 1 条 |
| 用 Score 分数做算术/插值 | Score levels 数值校准弱，不要用期望值反推具体数字；同一分数可能来自完全不同的分布（1.0 可以全是 level 1，也可以 0 和 2 各半） | 【官方】jaggedness；https://docs.typesafe.ai/primitives/score |
| instructions 与 criteria 语义相反（如 true 映射到"否"） | "might get confused"，明确会掉性能 | 【官方】jaggedness 第 7 条 |
| 把一个问法上调好的阈值搬到另一问法 | 结构不变式不成立（见上文 1.19 的例子） | 【官方】jaggedness 第 8 条 |
| 把问题写进 state（prompt）里 | **不会被当作指令**：写到 state 里的问题只是"待判断的文本"；框架层几乎没有东西会替你报错（集成层只有在完全没有可提问题时才报 UserError） | 【第三方（Pydantic AI 集成文档）】https://pydantic.dev/docs/ai/models/typesafe/ |
| 期望固定/幂等输出 | 官方称其"extremely consistent"（语义相近输入得到量级相近输出），但同一 state 的重复调用概率仍会小幅波动（`covered` 0.43–0.53）；且官方注明该实验无法区分"对无关字段的敏感"与"相同请求的固有抖动" | 【官方】jaggedness；consistency cookbook |
| 全量 state 直送（图省事不筛选） | 无关细节当干扰项，准确率下降且难定位错因 | 【官方】jaggedness 第 5 条 |
| 逐条串行调用而不批量 | 官方 Cookbook 自测：同一篇 GDPR 文档 13 问，批到一次调用 **12.2× 更便宜、10.0× 更快**，答案不变；官方 Patterns 页另一处写的是 **11.5× 更便宜、9.6× 更快**（两处都是官方口径，数字不一致，引用时建议注明页别） | 【官方】https://docs.typesafe.ai/cookbooks/parallel_questions ，https://docs.typesafe.ai/patterns/fan-out |

**一个真实的"误用后果"类型学**：把 Jev 输出的类型合法但错误的判定，加上一个偏高的置信度，静默送进下游自动化。第三方的总结值得直接引用：这种失败比幻觉更难被发现，因为"段落至少能被人读到并质疑，而 Jev 的输出会静默进入下一步"。【第三方】https://temperaturezero.com/2026/09/16/jev-doesnt-hallucinate-it-decides-wrong/

---

## 八、官方已知限制清单（jaggedness 页，jev-1.13）

官方原文开头即承认："jev-1.13 is fast, calibrated, and good at common-sense judgment but **it is not perfect**… It may struggle with tasks that require additional levels of indirection. It can be quite literal in its understanding. It struggles with tasks that require numeric precision."

| # | 失败模式 | 官方建议的替代做法 |
| --- | --- | --- |
| 1 | 字面理解（scoping/否定/隐含条件按字面读） | 把确切条件写进 instructions，边界情况写进 criteria；必要时拆成两个字面问题再在代码里组合 |
| 2 | 数学与数字（含计数；且不要用 Score 期望值还原具体数值） | 算术留在代码；计数在代码里数，按条件计数就逐项问 Noul 再在代码里加总 |
| 3 | 日期时间比较（按文本读，混格式/相对表述/季度与结算窗口更差） | 抽取归模型（做成封闭集合的 Choice，含"未提及"选项），排序/时长/偏移全归代码 |
| 4 | 多跳间接推理 | 指令尽量直接，点名 state 字段 |
| 5 | 大而嘈杂的 state | 先检索/筛选再送；无法筛选时可用 Noul 做相关性过滤 |
| 6 | 对抗性内容（注入指令、误导性框架、为自分类辩护的文字） | 精确写出 criteria，上线前充分测试边界用例 |
| 7 | instructions 与 criteria 冲突 | 把 criteria 当作 instruction 的延伸，语义对齐 |
| 8 | 结构性不变式（Choice 与 Noul 之间、正向与反向 Noul 之间不满足算术恒等） | 不要依赖期望的不变性；每个问法各自措辞、各自调阈值 |
| 9 | 生成文本 | 用生成模型；抽取场景改为"给候选 + 让 Jev 选" |

---

## 九、第三方实测与评论（与官方口径分开看）

**独立可核对的实测**

- **Every（Mike Taylor，head of evals）**：37 篇文档 × 21 问 = **777 条判断**，**<0.7 秒**，估算成本约 **四分之一美分**；他本人表示投产前还需要更严谨的准确率验证。另一组 12 段写作检查中，Jev 抓到 7 个植入缺陷中的 **6** 个，Fable 5.1 抓到 **7/7**；估算 Jev 约 **580× 更便宜、约 25× 更快**（单段中位 0.35s vs 8.83s）。注：Every 原文在付费墙内，本条目依据 firecrawl 与 kingy 两篇转述交叉核对，数字一致。【第三方】https://every.to/also-true-for-humans/mini-vibe-check-typesafe-s-jev-judged-everything-i-ve-written-in-0-7-seconds ，转述：https://www.firecrawl.dev/blog/what-is-jev ，https://kingy.ai/blog/typesafe-jev-review-the-ai-model-that-doesnt-generate-text/
- **HN 原帖**：1945 分、511 评论；最高赞质疑即"不能输出非法类型 ≠ 不会输出错误的合法值"；有工程师要求看到"置信度与实际正确率对齐"的证据（0.9 的 1000 个答案里应有约 900 个正确）。【第三方】https://news.ycombinator.com/item?id=49717558
- **pi-warden（coding agent 的动作护栏，用 Jev 判定 bash/write/edit）**：作者自报跨 315 个会话、**18,075 次受保护调用**共触发 **48 次 hold**（约 3 次/1000 调用）；在 13 个被标注的 hold 中，**2 个站得住、11 个在重试后通过**；另有 150 组配对 agent 运行的对照实验（对照组违反项目规则 6 次，启用后 0 次）。作者自己标注："**These are project-maintained benchmarks, not universal claims.**"【第三方（项目自测，非独立第三方）】https://pi.dev/packages/pi-warden
- **openjev（接口复刻，非模型）**：在 102 个与官方评测对齐的用例上，其公开模型与 Jev 公布的模态一致度为 **0.845 vs Jev 的 0.883**（两者接近但不相等）。【第三方】https://www.firecrawl.dev/blog/what-is-jev

**评论与质疑（无实测，但论点有引用价值）**

- The Register：所谓"无幻觉"与自然语言输出不可比；Jev 返回结构化概率，"does not preclude the possibility of being incorrect"。【第三方】https://www.theregister.com/ai-and-ml/2026/09/16/typesafe-ai-debuts-model-for-machines-that-plays-doom/5296711
- TemperatureZero：把 0% 幻觉率"非实证"这一自我披露与宣传口径的落差作为核心批评；指出 invoice 类任务上 Jev 61.8% vs 79.1%（**该数字与官方评测站一致，可放心引用**）；并指出 pipeline 组合后置信度不可靠。【第三方】https://temperaturezero.com/2026/09/16/jev-doesnt-hallucinate-it-decides-wrong/
- Laura Martel：类型保证在 2024 年起就已是 LLM + 受限解码的既有能力；真正难的是内容层面的校准，官方未给曲线；RLCD 缩写与 ICLR 2024 的既有方法重名。【第三方】https://www.laura-martel.com/blog/typesafe-jev-system-one-models
- TrueFoundry / Kingy：都强调"厂商可核对项"与"自报项"的界线；Kingy 明确声明**未做在线实测**（无 API key），只是文档与评测站审查，并列出官方仍未公开的六项（架构差异、RLCD 实现、分布偏移下的校准、p95/p99 与并发、加上下游 LLM 后还剩多少成本优势、与经典分类器/重排器的对比）。【第三方】https://www.truefoundry.com/blog/typesafe-ai-jev ，https://kingy.ai/blog/typesafe-jev-review-the-ai-model-that-doesnt-generate-text/
- Firecrawl：把官方 jaggedness 页称为"文档里最有用的一页，因为它告诉你别造什么"；并提醒 Jev 自身也可被对抗性 state 影响。【第三方】https://www.firecrawl.dev/blog/what-is-jev

---

## 十、可直接引用的示例

### 10.1 关键反例：类型安全 ≠ 数值一致（官方 jaggedness 页）

同一工单「I'm not happy with the fit. What are my options here?」，问"客户是否在要求退款"：

```
Noul  noul           0.22
Choice yes           0.01
Choice no            0.99
Choice confidence    0.97
```

同工单改为「I was charged twice for the same order. Can someone look into this?」问"是否在要求退款"与"是否在要求退款以外的东西"：

```
refund       0.72
not_refund   0.47
sum          1.19   ← 明显不是 1
```

### 10.2 快速开始（官方 quickstart，Python SDK）

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

print(response.answers["department"].choice)     # "technical"
print(response.answers["frustration"].score)     # 1.0
print(response.answers["is_urgent"].noul)        # 1.0
```

出处：https://docs.typesafe.ai/introduction/quickstart

### 10.3 置信度分段与"不要跨问法搬阈值"（官方 how-to-build 示例节选）

```python
spam_risk = (
    0.45 * answers["requests_credentials"].noul
    + 0.30 * answers["sender_identity_mismatch"].noul
    + 0.25 * answers["unexpected_reward"].noul
)

# 不确定 -> 转人工，而不是硬判
if 0.4 < spam_risk < 0.6 or answers["topic"].confidence < 0.75:
    return route_to_human_review(ticket)
```

出处：https://docs.typesafe.ai/concepts/how-to-build-with-system-one （注意：这是官方示例的**可读形态**，不是官方 API 文档里的字面代码；`route_to_human_review` 为示例中假定的应用函数）

### 10.4 低置信度就"退一层粒度"的手写逻辑（官方 cookbook 的核心 4 行思想）

```python
# 置信度 >= 0.9 报行业组；否则报它所属的 division
label = answer.choice if answer.confidence >= 0.9 else division_of(answer.choice)
```

出处与数字：https://docs.typesafe.ai/cookbooks/classification_using_confidence

### 10.5 官方评测站原始数据（可核对）

| workflow | Jev workflow | 最佳可比对象（workflow 模式） |
| --- | --- | --- |
| Security Incidents | 61.7% / $0.0001 / 0.3s | opus 5 66.2% / sol 62.5% |
| Agent Trace Observability | 71.6% / $0.0003 / 0.5s | sol 76.6% / luna 76.1% |
| Invoice Processing | 61.8% / $0.0011 / 0.5s | sol 79.1% / opus 5 78.4% |
| Customer Service | 76.0% / $0.0001 / 0.4s | sol 78.3% / terra 72.7% |
| 四任务等权聚合 | 67.8% / $0.0004 / 0.4s | sol 74.1% / terra 67.9% |

出处：https://evals.typesafe.ai/ （数字由页面各 workflow 图表直接读取；参考标签 = GPT-6 Astra 与 Fable 5.1 于 high thinking 回答的平均，其他模型用各自默认推理设置）。第三方 Kingy 复核给出的每任务样本量为 240 / 117 / 150 / 204，合计 **711** 例——该样本量我没在官方页面抓到，引用时请标注来源为第三方。【第三方】https://kingy.ai/blog/typesafe-jev-review-the-ai-model-that-doesnt-generate-text/

---

## 十一、不确定与未证实之处（请勿当成事实引用）

1. **校准是否真的成立**：没有公开的 ECE / reliability diagram / 独立复测。官方只有"设计目标 + 分层证据（90% vs 40%）"，第三方只有"未公开"这一事实。**不要写"Jev 已证明校准良好"。**
2. **官方博客 FAQ 的正文答案**：博客页里 "Is Jev just a smaller LLM?"、"How does Jev perform against public benchmarks?" 等 6 个问题是折叠组件，抓取的 HTML 与正文提取都只有问题、没有答案，**未取到原文**。
3. **各 workflow 的样本量 N（240/117/150/204/711）**：目前只见于第三方复核文章，官方页面我没抓到，待官方页面的完整数据或 PDF 佐证。
4. **193.6× / 444.6× 一类倍数**：官方自述来自其自建 workflow evals，且"expected to be on the higher end"；Kingy 指出从评测站四舍五入后的表**无法反推**这些倍数。不要当通用倍数使用。
5. **输出 token 的免费与用量字段**：定价页写 output tokens FREE，但第三方给的响应示例里 `usage` 含 `output_tokens: 48`。是否总会返回、是否计费口径一致，未见官方明确说明。
6. **对非英语（含中文）表现**：官方只说"handled but not equally well"，**没有任何具体数字**。
7. **对抗鲁棒性的量化**：官方只说"可以移动答案、未来会改进"，无任何成功率/攻击成功率数字。
8. **长期价格可持续性**：官方自己写 "We can't prove it isn't subsidized"。成本类结论依赖厂商定价不变。
9. **RLCD 的技术细节**：官方未披露奖励/评分规则与校准度量方式；第三方只能推测（HN 上有工程师给出"负奖励偏斜分布"的通用做法描述，属个人解读）。
10. **生产可靠性**：无独立 p95/p99、并发、重试率、可用性数据；早期访问阶段的实测样本都很小（777 条判断 / 18,075 次护栏调用）。

---

## 十二、对本项目 Jev 定位的启示（3 条）

1. **把 Jev 建模为"带概率的语义判定原语"，而不是"更快的 LLM"**。在 Rust 侧对应 `Choice` / `Score` / `Noul` 三个枚举类型 + 概率分布 + confidence，并**在类型层面就禁止**它去做生成、计数、算术、日期比较、多跳推理——这些由确定性代码或 LLM 承担。官方 jaggedness 页给出的正是"哪些判断属于代码、哪些属于模型"的划分线，可以直接变成 harness 的路由规则表。
2. **概率与置信度按一等数据向上传递，但阈值必须"逐问法"校准，且组合结论不得假装有置信度**。官方反例（Noul 0.22 vs Choice yes 0.01；0.72+0.47=1.19）说明跨问法/跨方向搬运阈值是错的；第三方指出多步串联后的整体可靠性取决于误差相关性、无框架可报。因此在 harness 里：保留 `probabilities` 与 `confidence`，把阈值放进配置且与具体问题 ID 绑定，多步判定的"整体置信度"要么不声明，要么由我们自己的标注数据实测得出。
3. **把 state 当作不可信数据、把 Jev 的判定当作"过滤器"而非"边界"**。官方明说 state 默认非敌意、注入可移动答案，且其 RAG 注入 cookbook 自陈"Nothing here is a security boundary"。对本项目意味着：(a) 送进 Jev 的 state 要按需裁剪（大而嘈杂掉点）；(b) 任何基于 Jev 的危险动作门控（action gating）都必须与确定性规则叠加，并且**绝不能把 Jev 的高置信度当成放行依据**；(c) 需要为"拒答/不确定"设计第三条路径（转人工或升级到 LLM），这与官方 confidence 三段的建议一致。

**附：可写进文档的"能力对照表"（一句话版）**

> Jev 擅长：有限选项内的语义选择、有序量表打分、是非概率判断，且可在一次请求里并行问很多个、极快极便宜。
> Jev 不擅长：生成任何文本、算术/计数/日期运算、多跳推理、处理大而嘈杂或带对抗性的 state、被当作安全边界。
> Jev 的"零幻觉"= 不会越出你声明的 schema；**不等于不会给出一个高置信度的错误答案。**

---

## 参考来源清单

**官方**

- 发布博客（含全部 Nuance 自陈）：https://typesafe.ai/blog/introducing-system-one-models-and-jev
- 模型与限制（context / rate limit / 输入类型 / 语言）：https://docs.typesafe.ai/models
- HTTP API 参考（255 选项、2–10 档、错误码、限流）：https://docs.typesafe.ai/api
- Choice：https://docs.typesafe.ai/primitives/choice ｜ Score：https://docs.typesafe.ai/primitives/score ｜ State：https://docs.typesafe.ai/concepts/state
- Confidence：https://docs.typesafe.ai/confidence
- System One 概念（不写回复/不产代码/不解释）：https://docs.typesafe.ai/concepts/system-one
- AI primer（校准的群体统计定义）：https://docs.typesafe.ai/introduction/machine-learning-primer
- **Jev 1.13 jaggedness（失败模式权威清单）**：https://docs.typesafe.ai/model-jaggedness/jev-1.13
- 与 coding agent 的关系（不是 drop-in 替代）：https://docs.typesafe.ai/introduction/coding-agents
- 官方评测站（workflow evals，方法与全部数字）：https://evals.typesafe.ai/
- cookbook：分类+置信度 https://docs.typesafe.ai/cookbooks/classification_using_confidence ｜ Noul 自洽性 https://docs.typesafe.ai/cookbooks/consistency_noul_cookbook ｜ 并行提问 https://docs.typesafe.ai/cookbooks/parallel_questions ｜ RAG 注入过滤 https://docs.typesafe.ai/cookbooks/classifying_rag_passages ｜ 重排 https://docs.typesafe.ai/cookbooks/rerank_typesafe ｜ SDE 级联校验 https://docs.typesafe.ai/cookbooks/sde_cascade ｜ 实体对齐（并发经验值）https://docs.typesafe.ai/cookbooks/entity_alignment

**第三方**

- Hacker News 发布讨论（1945 分 / 511 评论，含创始人回帖）：https://news.ycombinator.com/item?id=49717558
- The Register：https://www.theregister.com/ai-and-ml/2026/09/16/typesafe-ai-debuts-model-for-machines-that-plays-doom/5296711
- Every（付费墙内，数字经两处转述交叉核对）：https://every.to/also-true-for-humans/mini-vibe-check-typesafe-s-jev-judged-everything-i-ve-written-in-0-7-seconds
- Firecrawl 综述（jaggedness 汇总、pi-warden、openjev）：https://www.firecrawl.dev/blog/what-is-jev
- TemperatureZero（"决定错"而非"幻觉"；pipeline 置信度）：https://temperaturezero.com/2026/09/16/jev-doesnt-hallucinate-it-decides-wrong/
- Laura Martel（类型安全非新事、无校准曲线、RLCD 重名）：https://www.laura-martel.com/blog/typesafe-jev-system-one-models
- Kingy 审查（逐条区分可核对/自报，含 711 例样本量）：https://kingy.ai/blog/typesafe-jev-review-the-ai-model-that-doesnt-generate-text/
- TrueFoundry（可核对 vs 厂商主张）：https://www.truefoundry.com/blog/typesafe-ai-jev
- Pydantic AI 集成文档（无采样旋钮、问题必须写在字段上）：https://pydantic.dev/docs/ai/models/typesafe/
- pi-warden 现场数据（项目自测）：https://pi.dev/packages/pi-warden
