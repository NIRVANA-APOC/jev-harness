# 专题 12：公司与生态背景（TypeSafe AI）

> 调研日期：2026-09-22。所有 URL 均为本次调研中实际抓取/提取过的页面。
> 来源分级：**[官方]** = 公司自有站点/文档/新闻稿；**[播客-创始人]** = 创始人口头陈述（官方人物、非正式）；**[第三方]** = 独立媒体/博客（含观点性分析，已标注可信度）。
> 凡本人亲自核对过原文的条款，标注「已核对原文」；仅见于第三方转述的，标注「未见一手来源」。

---

## 一、结论要点

1. **公司很年轻、很早期。** TypeSafe AI, Inc. 2024 年成立，约两年 stealth，2026-09-15 出 stealth 并发布 Jev。融资约 $40M 种子轮（DCVC 领投）。创始人 Almeida 在播客中自述**上线前几乎没有收入**。团队规模（LinkedIn）11–50 人，仍在密集招聘后端/平台与数据岗。对本项目而言：这是一个**高潜力但单点依赖风险真实存在**的供应商。
2. **无 SLA。** 主合同（MCA）第 3 条对支持的承诺仅为「commercially reasonable efforts」+ 邮件支持，**全文无可用性数字承诺、无赔偿型 SLA**。官方有状态页，但那是自我披露的历史数据，不是合同义务。
3. **可靠性是「已发生过的风险」而非假设。** 官方状态页显示 `api.typesafe.ai` 近 90 天可用率 **99.840%**，6 月以来多次数分钟级中断（9/19 中断 18 分钟，9/20 记录「API issues」）；TechCrunch 报道上线期间因流量过高**一度无法从 API 服务用户**。创始人声称「我们的 uptime 比 Anthropic 还多几个 9」——属官方口径。
4. **数据条款比营销口径粗糙。** 官方确实承诺不拿 Input 训练模型（隐私政策明文 + 文档明文），但 MCA 4.1 留有「客户事先同意」例外、且把「训练」限定为「修改模型权重」；MCA 4.3 的 **Telemetry** 定义含 "learnings"、可**无限制处理**，第 11 条 TypeSafe 拥有 Telemetry 权利，10.4 使其在协议终止后继续存续。ZDR 仅面向企业客户。
5. **API 变更风险写进了合同。** MCA 2.5 明确：TypeSafe 可更新服务，**更新可能导致 API 与客户应用不兼容**，仅承诺「商业上合理努力」对重大不利变更提前通知。这对本项目（harness 需长期稳定对接）是直接风险。
6. **生态接入面已相当宽。** 除官方 Python/JS SDK 外，Pydantic AI、LangChain、LiteLLM、Cloudflare Workers AI、Opper、Requesty、Vercel AI SDK 均已适配；**未发现官方 Rust SDK**。
7. **一个对本项目特别重要的技术限制**：官方文档明示**英文是主要训练语言**，其他语言「包括 CJK 脚本」可用但表现不均，要求团队自行在自己的内容上测试后再依赖。Jev 还**只接受文本输入**（图像/音频/视频需先转成文本或结构化字段）。
8. **媒体热度 ≠ 独立验证。** 官方「193.6x 更快 / 444.6x 更便宜」来自自家 workflow evals（以最强模型平均判断为参考，而非真值）。第三方（Kingy AI）复核汇总 711 例：Jev 聚合 accuracy 67.8%，低于参照模型 Sol 的 74.1%，但成本/时间压倒性领先。

---

## 二、关键事实与数据（逐条附 URL）

### 2.1 公司身份、创始人、成立与融资

| 事实 | 内容 | 来源与语境 |
| --- | --- | --- |
| 成立年份 / 总部 | 2024 年成立；总部旧金山（注册地址 255 California St, Suite 1300, San Francisco, CA 94117） | **[官方]** 新闻稿（Business Wire，经 Yahoo Finance 全文转载）：*"Founded in 2024 and headquartered in San Francisco"* → https://finance.yahoo.com/technology/ai/articles/typesafe-ai-emerges-stealth-40m-190000776.html ；地址见 MCA 16.5 与 ToU 13(e) 原文 https://typesafe.ai/legal/mca 、https://typesafe.ai/legal/terms |
| 出 stealth 日期 | 2026-09-15，同一日发布 Jev 并开启 early access / waitlist | **[官方]** 上述新闻稿；**[第三方]** TechCrunch 2026-09-18 https://techcrunch.com/2026/09/18/a-new-kind-of-ai-model-from-a-chatgpt-inventor-is-thrilling-developers/ |
| 融资 | **约 $40M 种子轮，DCVC 领投** | **[官方]** 新闻稿（同上）；**[第三方]** finsmes https://www.finsmes.com/2026/09/typesafe-ai-raises-40m-in-seed-funding.html 、HPCwire/AIWire https://www.hpcwire.com/aiwire/2026/09/16/typesafe-ai-emerges-from-stealth-with-40m-in-funding-with-new-model-for-composable-ai/ 、seedtable https://seedtable.com/companies/typesafe-ai/funding-rounds/seed-2026-09 、dealroom https://dealroom.co/news/151032-typesafe-exits-stealth-with-40m-seed-to-build-ai-for-software-not-people/ |
| 投资方表态 | DCVC General Partner James Hardiman：*"Diogo, Erik, and Sasha bring the technical depth and conviction to rethink how models are built for software."* | **[官方]** 新闻稿（同上） |
| 创始人 | **Diogo Almeida**（co-founder & CEO）、**Erik Gafni**、**Sasha Sheng** | **[官方]** 新闻稿；LinkedIn 帖（第三方转载）https://www.linkedin.com/posts/robbroderick_after-two-years-in-stealth-diogo-almeida-activity-7506785550129864704-7UYQ |
| Almeida 履历 | 「co-invented RLHF and InstructGPT，此前在 Google Brain」；官方与媒体多用「ChatGPT 的共同发明人」口径 | **[官方]** https://typesafe.ai/team ；**[第三方更正]** Anthony Maio 指出更准确说法是 InstructGPT 论文的 equal-contribution primary author、并参与 GPT-4 —— https://anthonymaio.substack.com/p/jev-the-language-model-that-wont |
| 团队构成与办公方式 | 来自 OpenAI、Google Brain、Meta/FAIR、Stripe、Airbnb、Plaid、Docker；旧金山 Embarcadero 附近办公室，**每周 5 天现场办公** | **[官方]** https://typesafe.ai/team |
| 团队规模 | LinkedIn 公司页标注 **11–50 employees** | **[第三方]** https://www.linkedin.com/company/typesafe-ai （未登录可见的公开字段） |
| 招聘 | 在招 Member of Technical Staff（Model Capabilities / Backend-Platform），Base $150k–250k + equity | **[第三方]** https://jobs.ashbyhq.com/typesafe-ai 、https://www.glassdoor.com/job-listing/jv?jl=1009970275998 |
| 收入状态 | 创始人自述：**上线前几乎没有收入**（"We had almost no revenue before launch."） | **[播客-创始人]** Latent Space《Jev: System One models for Prod, not God》文字稿/摘要 https://www.latent.space/p/jev |
| 自我定位 | 「We're building prod, not God」；自称 **data lab 而非 model lab**；训练数据全部合成 | **[官方]** https://typesafe.ai/manifesto ；**[播客-创始人]** 同 Latent Space |
| 对外口径的谨慎之处 | 官方博文自认「193.6x / 444.6x 属于偏高端」，且「无法证明定价没有被补贴」 | **[官方]** 发布博文 https://typesafe.ai/blog/introducing-system-one-models-and-jev |

**关键时间线（可核实部分）**

- **2024**：公司成立（新闻稿）。
- **约 2 年 stealth**：官方博文"After two years in stealth"；TechCrunch 引 Almeida "Two years ago, Almeida left OpenAI to start TypeSafe AI"。
- **不晚于 2026-06-24**：`api.typesafe.ai` 已进入官方状态页监控（说明 API 在公开发布前已运行）。
- **2026-09-15**：出 stealth + Jev 发布 + early access/waitlist + $40M 披露。
- **2026-09-15～19**：第三方生态适配密集上线（Every 实测 9/15；LangChain 指南、Pydantic AI provider、Cloudflare Workers AI 文档、LiteLLM pass-through 均在首周内出现）。
- **2026-09-22（本调研日）**：仍处 early access / waitlist 状态，官方文档标注为正式文档体系。

### 2.2 产品线与路线（除 Jev 之外）

- **目前唯一公开模型就是 Jev**：`Jev 1.13`（versioned id `jev-1.13.0`），别名 `jev-latest`（最近稳定版，SDK 默认）与 `jev-preview`（含预览版）。所有模型共用同一端点 `POST /v1/systemone`。**[官方]** https://docs.typesafe.ai/models
- **平台层产品**：三原语（`Choice` / `Score` / `Noul`）+ 控制台 `console.typesafe.ai` + 评测站 `evals.typesafe.ai` + 信任中心 `trust.typesafe.ai`（托管于 Vanta）+ 文档 `docs.typesafe.ai`。**[官方]** https://docs.typesafe.ai/ 、https://status.typesafe.ai/ 、https://trust.typesafe.ai/
- **路线图**：官方仅表"还有很多在管线中"（"We have a lot more in the pipeline"），无日期、无产品名。**[官方]** 发布博文。
- **未来模型**：TechCrunch 引 Almeida"TypeSafe itself will be building more versions of the model, in new modalities"；Latent Space 节目大纲中出现「ReasoningJev?」的设问（**属访谈提问，不是官方产品承诺**）。**[第三方]** TechCrunch（同上）；**[播客]** https://www.latent.space/p/jev
- **未找到公开信息**：OpenAI 兼容的 `/v1/chat/completions` 官方端点（第三方网关 Requesty 提供的是网关侧封装，见其 curl 示例）；本地/私有化部署方案；官方 Rust SDK。

### 2.3 平台能力、定价与限额（官方数据）

| 项目 | 数值 | 来源 |
| --- | --- | --- |
| 端点 | `POST /v1/systemone`（Bearer API key） | https://docs.typesafe.ai/api |
| 定价 | **$42 / Btok = $0.042 / Mtok 输入**；输出 token **免费** | https://docs.typesafe.ai/models ；首页 https://typesafe.ai/ |
| 上下文 | 单请求 **64k tokens**（state + 全部问题）；其中 **32k** 为 state + 最长单个问题 | https://docs.typesafe.ai/models |
| 输入类型 | **仅文本**（string / JSON object / 文本数组）；无图像、音频、视频 | 同上 |
| 并行问题 | 一次请求内问题并行求值，增加问题几乎不增加时延 | https://docs.typesafe.ai/ 、发布博文 |
| 题目规模上限 | `Choice` 最多 **255** 个选项（更高基数需分两阶段）；`Score` 支持 **2–10** 级 | https://docs.typesafe.ai/api |
| 速率限制 | **250,000 tokens/s** 与 **1,200 requests/min**，超限返回 429；SDK 默认退避重试并遵循 `retry-after` | https://docs.typesafe.ai/models |
| 错误码 | 401 无效密钥；422 请求校验失败；429 超限；**529 Overloaded（服务过载，建议退避重试）** | https://docs.typesafe.ai/api |
| 语言支持 | 英文为主训练语言；**其他语言（含 CJK）可用但表现不均**，建议先自测再用 | https://docs.typesafe.ai/models |
| 官方 SDK | Python、JavaScript | https://docs.typesafe.ai/models |
| 数据使用 | "Jev is not trained on customer requests or responses"；不做客户数据微调/LoRA，所有账号共用同一权重 | 同上 |

### 2.4 可靠性、运维与运行区域

- **官方状态页**（已抓取；页面由 Better Stack 驱动，提供 30/60/90 天窗口切换，**静态文本未直接标明当前窗口**，本调研抓取时可见条目自 2026-06-24 起）：

  `api.typesafe.ai` 显示 **99.840% uptime**；`console.typesafe.ai` **99.988%**。可读到的中断包括 2026-06-29（6 分钟）、07-09（20 分钟）、07-10（18 分钟）、07-23（12 分钟）、08-03（**59 分钟**）、08-21（8 分钟）、09-09（6 分钟）、09-16（5 分钟）、**09-19（18 分钟）**，以及 09-20 一条标注为 **"API issues"**（2 分钟，"now operational"）。来源：**[官方]** https://status.typesafe.ai/ （本调研 2026-09-22 抓取）
- **上线期容量事件**：TechCrunch 报道「公司因需求过高**一度失去从 API 服务用户的能力**」。**[第三方]** https://techcrunch.com/2026/09/18/a-new-kind-of-ai-model-from-a-chatgpt-inventor-is-thrilling-developers/
- **创始人口径**：「我们的平台团队太强了，我认为我们的 uptime 9 的个数比 Anthropic 还多」；同时承认「我认为我们会在很长时间内受限于 GPU」。**[播客-创始人]** https://www.latent.space/p/jev （属自我评价，非独立核实）
- **合同层面**：MCA 第 3 条 Support = 「commercially reasonable efforts」+ `support@typesafe.ai`；**无 SLA、无可用性百分比、无停机赔付**（已核对原文）。https://typesafe.ai/legal/mca
- **安全事件通知**：DPA 5.2 承诺「无不当延迟且无论如何在知悉后 **72 小时内**」通知。https://typesafe.ai/data-processing （已核对原文）
- **审计权**：DPA 5.3 允许客户**每 12 个月最多一次**审计，费用自担。同上
- **安全措施载体**：DPA Schedule I 第 11 项全文仅为一句——安全措施「如 TypeSafe 信任中心所述」，即 **一个 URL**（我对 https://trust.typesafe.ai/ 的抓取只得到 Vanta 的 JS 壳，未能独立读取条目；SOC 2 报告是否存在、是否可下载**未见一手来源**，仅第三方称其位于"Request access"之后）。
- **运行区域**：官方发布博文原文承认评测「一般是在美西的笔记本上跑的（**这也是我们服务目前所在的位置**）」；隐私政策称服务托管在 **美国**（已核对原文）。→ 对非美区调用方，端到端时延需自行测量。
- **超载语义**：`529 Overloaded` 是官方文档明确列出的错误码，说明服务端过载被当作**契约内的正常状态**，客户端必须自行退避。
- **第三方可用性观测（意义有限，供对照）**：piehost 对 `typesafe.ai`（**营销网站**，非 API）90 天 HTTP 可用率显示 100%。https://piehost.com/status/typesafe.ai —— 注意该监控**不覆盖 API**，不可作为 API 可用性证据。

### 2.5 服务条款、隐私与数据留存（本人已逐条核对原文）

四份文件及适用范围（与第三方整理一致，但**管辖法部分第三方有误，见 2.6**）：

| 文件 | 覆盖范围 | 管辖/场地 | 责任上限 | URL |
| --- | --- | --- | --- | --- |
| Terms of Use | **仅网站 typesafe.ai** | 特拉华州法；特拉华州法院 | **$100 USD** | https://typesafe.ai/legal/terms |
| Master Customer Agreement (MCA) | **API 与控制台** | **加州法；旧金山** | 取高者：(A) 主张发生前 12 个月已付/应付费用，(B) **$50 USD** | https://typesafe.ai/legal/mca |
| Data Processing Addendum (DPA) | Input 中的个人数据 | 爱尔兰法；**都柏林法院**（EU SCC 模块 2/3 + UK Addendum） | 依 MCA；冲突时 DPA 优先（1.3） | https://typesafe.ai/data-processing |
| Privacy Policy | TypeSafe 自身的数据处理 | 未写明 | 不适用 | https://typesafe.ai/legal/privacy-policy |

**隐私与数据留存要点（均已核对原文）**

- **不训练承诺（隐私政策，最平实）**："**We will not train or fine tune any artificial intelligence or machine learning models on your prompts or other Input.**" 并承诺不向服务提供方以外的第三方披露 Input。https://typesafe.ai/legal/privacy-policy
- **不训练承诺（MCA 4.1，留有口子）**：TypeSafe 不会把 Customer Data 纳入训练数据集「(i.e., to modify the model weights of)」——**除非客户事先同意**；且该定义**锚定在「修改模型权重」**上，字面不覆盖评测集、检索语料等。已核对原文。
- **Telemetry（MCA 4.3，实际最宽的授权）**：Telemetry 定义为「technical logs, hashes, summary statistics and classifications, metrics, and **learnings** related to Customer's use of the Services」，TypeSafe 可**无限制处理**，包括用于改进其「其他产品与服务」；4.1(b) 明确授权其为**派生 Telemetry** 而处理 Customer Data；第 11 条将 Telemetry 的知识产权归 TypeSafe；10.4 使 4.1/4.3/11 在协议终止后**继续存续**。→ 即「由你的数据派生、归他们所有、期限无限制、协议结束后仍是他们的」。已核对原文；第三方对此的批评见 https://wunderlandmedia.com/typesafe-ai-jev-terms-of-service-gdpr 与 https://wonderwhy-er.medium.com/typesafe-jev-wont-train-on-your-data-it-can-still-learn-from-it-563f0ad591b6
- **隐私政策中不出现 "Telemetry" 一词**（已核对全文），两份文件口径存在落差。
- **留存期限**：隐私政策只写「as long as reasonably necessary to provide you with the Services, or otherwise in support of our business or commercial purposes」——**无具体天数**。已核对原文。
- **删除**：MCA 10.3 明确「TypeSafe **无义务**存储或保留 Customer Data，并可**自行决定随时删除**」；**DPA 正文中没有删除/返还数据的条款**（已核对全文，Schedule I 第 8 项仅复述「保留至必要期间」）。→ 第三方（wunderlandmedia）认为删除权需经由 SCC 第 8.5 条迂回主张，且**没有删除证书或导出窗口**。
- **ZDR（零数据留存）**：仅面向企业客户，需邮件 `privacy@typesafe.ai` 申请。**[官方]** https://docs.typesafe.ai/legal —— 属「可谈」而非默认。
- **责任与赔偿的不对称**：12.2 上限取高者(12 个月费用, $50)；12.3 排除项为客户未付款、客户违反 2.3/2.4/5、以及**双方的赔偿义务**；13.1 的侵权赔偿**仅限「美国」专利/著作权/商标/商业秘密**；13.5(e) **明确排除 Output**。已核对原文。
- **免责与营销口径的落差**：9.3 以全大写声明服务按 "AS IS / AS AVAILABLE" 提供、可能产生不准确输出、客户须自行评估输出；9.1/9.2 的「Service Warranty」仅保证按文档「materially」运行，救济是 30 天内修复或退还未使用预付费。已核对原文。→ 与营销侧「不能幻觉 / 100% 类型正确」并存：**类型正确 ≠ 语义正确**（第三方多篇均指出，官方博文本身也承认 hallucination 与 type-safety 是两件事）。
- **协议可被单方面变更**：16.7 允许 TypeSafe 通知更新协议，**至少 60 天后**对后续生效；ToU 第 6 条允许随时变更且「每次访问即受最新版约束」。已核对原文。
- **暂停权**：第 6 条列明可**立即暂停**服务的四种情形（含违反许可限制、逾期 30 天未付款、法律变化、危害其他客户/系统安全）。
- **许可限制（对本项目最重要的两条）**：2.3(b) **禁止用服务或 Output 做模型蒸馏、训练模仿其输出的模型、或开发同类/竞争产品**；2.3(a) 禁止把服务作为独立服务转售/分发/再许可。2.3(g) 把「进行安全或漏洞测试」与「绕过访问控制」并列禁止。已核对原文。
- **API 兼容性风险**：2.5 明确「TypeSafe 可能不时更新服务，且**此类变更可能导致 API 与客户应用不兼容**」，仅承诺对重大不利变更尽力提前通知。已核对原文。
- **网站抓取限制（仅限网站，不是 API 条款）**：ToU 3(b)(vi) 禁止以自动化进程 / robots / spiders / scrapers / webcrawlers 监控、复制或下载 Site 上的数据；ToU 仅覆盖网站。**注意**：MCA 全文**没有**禁止公布 benchmark 的条款（该常见说法见 2.6 纠错）。已核对原文。
- **预付费与退款**：8.2 Credits「不可赎回、不可退款、不可转让、非法定货币」，到期时间取「期限结束」或「购买后 12 个月」较早者；10.3(c) 终止时**未消费金额不退还**。已核对原文。
- **争议解决**：15.1 强制**单独仲裁**（JAMS，加州），15.7 放弃集体诉讼；MCA 由加州法管辖、旧金山场地（16.2）。已核对原文。

### 2.6 ⚠️ 第三方分析中的错误（不要直接引用该来源的细节）

第三方博客 https://wunderlandmedia.com/typesafe-ai-jev-terms-of-service-gdpr 提供了很有价值的整理，但其关键细节与**当前原文不符**：

| 该文主张 | 本人核对 MCA 原文（2026-09-22）的结果 |
| --- | --- |
| MCA 适用 **特拉华法 / loser pays** | **错**。MCA 16.2：加州法；场地为旧金山的州/联邦法院。（特拉华法 + $100 上限对应的是 **Terms of Use（仅网站）**，该文把两份文件混为一谈） |
| MCA **2.3(f) 禁止公布 benchmark**，违者暴露无限 | **当前原文中不存在该条款**。MCA 2.3(f) 实为 "interfere with the operation of the Services"；全文检索无 benchmark 相关禁令。（不排除官方已改版；结论是**该细节不可引用**） |

该文其余内容（Telemetry 的四条款联动、责任上限不对称、13.1 仅限美国 IP、13.5(e) 排除 Output、无 SLA、DPA 附件薄弱、子处理器全为美国公司、TypeSafe 未加入 EU-US DPF）与本人核对到的原文方向一致，可作**线索**使用，但引用前应回到一手条款。其自称"非律师、非法律意见"。

### 2.7 生态与集成（第三方可核实的对接面）

| 集成方 | 形态 | URL | 性质 |
| --- | --- | --- | --- |
| **Cloudflare Workers AI** | 官方 Cloudflare 文档托管 `typesafe/jev`，含 Workers `env.AI.run` 与 REST 两套完整示例、上下文窗口标 32,000 tokens | https://developers.cloudflare.com/ai/models/typesafe/jev/ | **[第三方平台官方文档]** 影响力最大的分发渠道之一 |
| **Pydantic AI** | 内置 `TypeSafeModel`（`typesafe:jev-latest`），一个 Pydantic 输出模型的每个字段映射为一个问题；明确 `ModelResponse.model_name` 记录实际应答的版本号 | https://pydantic.dev/docs/ai/models/typesafe/ | **[第三方平台官方文档]** |
| **LangChain** | `langchain-typesafe`：`TypeSafeClassifier` + `ModelRouterMiddleware`（模型路由）+ `AutoModeMiddleware`（工具调用前的风险门控） | https://www.langchain.com/blog/building-a-harness-with-jev | **[第三方]** 与本项目「Agent Harness」定位最接近的公开范例 |
| **LiteLLM** | pass-through 端点支持 | https://docs.litellm.ai/docs/pass_through/typesafe | **[第三方]** |
| **Vercel AI SDK** | 有官方/社区接入教程；Vercel 工程师 Pranit Sharma 用 Jev 替换 ChatGPT Luna 做命令安全分类，称**快 5–18x 且更准** | https://techcrunch.com/2026/09/18/a-new-kind-of-ai-model-from-a-chatgpt-inventor-is-thrilling-developers/ | **[第三方]** 单场景自测，非基准 |
| **网关** | Opper（标注 ZDR posture「not established」、有 DPA、无训练）、Requesty（提供 OpenAI 风格 `response_format: {"type":"questions"}` 封装） | https://opper.ai/provider/typesafe 、https://www.requesty.ai/blog/typesafe-jev-explained | **[第三方]** |

- **OpenRouter：未上架（本人 2026-09-22 实查）**。`https://openrouter.ai/api/v1/models` 全量列表中未检索到任何 Jev / TypeSafe 条目；同日抓取 `openrouter.ai` 的 Typesafe 页面亦为空页。→ 意味着 **Jev 目前没有第三方多 provider 路由/故障转移层**，只能直连官方 API，或经 Cloudflare Workers AI / Opper / Requesty 等网关；harness 的容灾不能指望「换 provider」。(注：官方发布博文提到 "The numbers for LLMs are from OpenRouter"，那是指其评测中的 LLM 基线，不表示 Jev 已在 OpenRouter 上架。)

**独立第三方实测（对"官方口径"的对照）**

- **Every（Mike Taylor，head of evals）**：37 篇文章（27 篇真实 + 10 篇故意 AI 风格）× 21 个问题 = **777 个判断，< 0.7 秒，估计约 1/4 美分**；另测 11 个实验共 **1,709 个判断，总成本估计 < 1 美分**。作者明确表示**准确性需更严格验证后才敢上生产**。https://every.to/also-true-for-humans/mini-vibe-check-typesafe-s-jev-judged-everything-i-ve-written-in-0-7-seconds
- **Kingy AI（文档/eval 复核，非实测）**：汇总官方 workflow dashboard 711 例，**Jev 聚合 accuracy 67.8% / $0.0004 / 0.4s，对照 Sol 74.1% / $0.0836 / 23.3s**；发票处理差距最大（61.8% vs 79.1%）。同时指出：官方评测是「与模型参考答案的一致度」而非真值；置信度校准**未被独立证明**；"zero hallucination" 是**窄义的类型安全主张**。https://kingy.ai/blog/typesafe-jev-review-the-ai-model-that-doesnt-generate-text/
- **Flavio Copes**：面向工程实践的长文，记录 429 与 529 的错误语义与限额。https://flaviocopes.com/jev/
- **Anthony Maio**：认可「卖点是限制而非价格」的架构判断，指出创始人口径应更精确。https://anthonymaio.substack.com/p/jev-the-language-model-that-wont

---

## 三、请求/响应示例（来自可核查的一手文档）

官方 Quickstart（Python，`typesafe_sdk`）——**直接引自官方文档**：

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

print(response.answers["department"].choice)   # "technical"
print(response.answers["frustration"].score)   # 1.0
print(response.answers["is_urgent"].noul)      # 1.0
```

来源：https://docs.typesafe.ai/introduction/quickstart

HTTP 形态（同一个 `POST /v1/systemone`，字段名与响应形状引自 Cloudflare 官方文档的完整示例，`"model": "jev-1.13.0"` 为实际应答版本）：

```jsonc
// 请求
{
  "model": "jev-latest",
  "state": "Help! My payouts have been failing for 3 days.",
  "questions": {
    "is_urgent":  { "type": "noul",   "instructions": "Does this convey urgency?" },
    "department": { "type": "choice", "instructions": "Which team should handle this?",
                    "criteria": { "billing": "...", "technical": "...", "sales": "..." } },
    "frustration":{ "type": "score",  "instructions": "How frustrated is the customer?",
                    "criteria": ["Calm", "Frustrated", "Very angry"] }
  }
}
```

```jsonc
// 响应
{
  "model": "jev-1.13.0",
  "answers": {
    "is_urgent":  { "type": "noul", "noul": 0.95 },
    "department": { "type": "choice", "choice": "billing", "confidence": 0.8,
                    "probabilities": { "billing": 0.87, "sales": 0, "technical": 0.13 } },
    "frustration":{ "type": "score", "score": 1.04, "confidence": 0.94,
                    "legend": { "0": "Calm", "1": "Frustrated", "2": "Very angry" },
                    "probabilities": { "0": 0, "1": 0.96, "2": 0.04 } }
  },
  "usage": { "input_tokens": 426, "output_tokens": 73 }
}
```

来源：https://developers.cloudflare.com/ai/models/typesafe/jev/ （响应形状与字段命名与官方 API 参考 https://docs.typesafe.ai/api 一致）

**Rust 客户端相关**：未发现官方 Rust SDK。据官方 API 参考，接入所需信息为：单一端点 `POST /v1/systemone`、Bearer 鉴权、问题/答案以调用方自定义的 key 一一对应（**key 不发送给模型**）、错误码 401/422/429/529，429 与 529 需指数退避并遵循 `retry-after`。来源：https://docs.typesafe.ai/api

---

## 四、不确定与未证实之处

1. **估值与轮次规模有冲突数据**：Forbes 文章标题为「This $200 Million Startup Wants To Fix AI's Overconfidence Problem」（https://www.forbes.com/sites/the-prompt/2026/09/15/this-200-million-startup-wants-to-fix-ais-overconfidence-problem/ ），但**本人两次尝试均无法提取正文**（403 / 提取失败），「$200M」指估值还是其他数字**未能证实**。另 dealroom 页面文本中出现 "US$25.9M seed round" 字样，与 $40M 不一致，**未见解释**。→ **唯一可引用的融资数字是「约 $40M 种子轮，DCVC 领投」（Business Wire 新闻稿原文）**。
2. **模型架构未公开**：官方不披露参数量、架构、硬件、训练配方、模型卡、权重。TechCrunch 称「outside observers suspect is built on top of an open-weight LLM」——**这是记者的转述与猜测，Almeida 拒绝透露**，不可当作事实。来源：TechCrunch（同上）；第三方文档复核亦列出同一「publication gap」：https://kingy.ai/blog/typesafe-jev-review-the-ai-model-that-doesnt-generate-text/
3. **RLCD 无公开技术细节**：官方称 RLCD（Reinforcement Learning for Calibrated Decisions）为自研训练法、数据全合成，但**未发表论文/技术报告**。「概率是否校准」目前**只有厂商声称与文档建议（在自有数据上测阈值）**，无第三方校准曲线。
4. **SOC 2 / 认证状态未证实**：Trust Center 为 Vanta 托管，我抓取 https://trust.typesafe.ai/ 与 https://trust.typesafe.ai/subprocessors 时只返回前端 JS 壳，**无法读取子处理器清单与合规条目**。DPA 3.1 确实指向该 URL，因此清单**存在但内容未验证**。「子处理器为 AWS（存储/处理）、Modal（推理）、Slack、Google Workspace（支持），全部美国公司」这一点**仅见于第三方**（wunderlandmedia），**未见一手来源**。
5. **无官方 SLA 文本**：MCA 中确认无 SLA（已核对）。但**是否存在单独的企业级 SLA 附件**（仅签约客户可见）无从核实——「未找到公开信息」。
6. **定价可持续性未证实**：官方自认「无法证明定价未被补贴，且长期会证明其可持续性（我们预期价格会降不会升）」；播客中创始人称「我认为我们会在很长时间内受限于 GPU」。第三方（Requesty/Opper）均照抄官方价格。**$0.042/Mtok 是否为长期价格，无独立证据**。
7. **多数评测数字是厂商自评**：193.6x / 444.6x / 70–500ms / 「无法幻觉」等均出自官方；官方自己声明这些数字偏高端、评测在美西、评测任务由自家模型能力团队构造（存在偏倚）。唯一较独立的量化对照是 Every 的 777/1709 次实调用（时间与成本）与 Kingy 的文档复核（准确率不占优）。
8. **未找到公开信息**：公司营收/ARR（只有创始人「上线前几乎无收入」的口述）；员工确切人数（只有 LinkedIn 区间）；是否有后续融资轮；官方 Rust SDK；私有化部署方案；正式 AUP（可接受使用政策）与高风险用途限制条款（第三方指出四份文件中**没有**禁止清单或人类监督要求）。另：**Jev 未出现在 OpenRouter 公开模型列表中**（2026-09-22 本人实查 `https://openrouter.ai/api/v1/models` 无匹配条目），因此也不存在「按 provider 路由/故障转移」的第三方层；但无法排除后续上架，或仅对特定账号开放。
9. **第三方来源质量提示**：本次检索到的相当一部分"分析"文章（kingy.ai、wunderlandmedia、truefoundry、mindstudio、innfactory、requesty、opper 等）是发布后数日内产出的内容营销/SEO 文章，含**可验证的错误**（见 2.6）。引用时优先官方文档与条款原文，第三方仅用于印证与批评视角。

---

## 五、对本项目（Rust Agent Harness）Jev 定位的启示

1. **把 Jev 当作「可替换的云端判定原件」，而不是可依赖的基础设施。** 无 SLA、状态页 99.840%（且有过 59 分钟级中断）、上线期曾无法服务用户、合同明确「API 更新可能导致不兼容」、预付费不可退、且没有第三方多 provider 容灾层——这些叠加起来意味着 harness 必须默认具备：**超时预算 + 429/529 指数退避 + 熔断降级**（降级路径可回落到确定性规则或 LLM 判定），并把不确定性作为一等数据向上传递，而不是让 Jev 失败就中断整个 Agent 循环。模型别名（`jev-latest`）会漂移，**应固定 `jev-1.13.0` 这类版本号**，由 harness 自己控制升级节奏。
2. **能力边界与成本模型要写进架构决策，而不是写进宣传语。** Jev 只吃文本、英文最优（CJK 需自测）、单请求 64k/32k 上下文、Choice ≤255 项、按输入 token 计费而输出免费。最匹配的用法是官方自己给出的模式：**把判断拆成原子问题一次并行发出，结果在代码里组合**——这正好对应本项目「决策分层：能让确定性代码判断的，两者都不必用」的总则：Jev 应当吃「模糊判定」，不该吃「可精确计算」和「需要生成」的部分。
3. **数据合规按「默认最小化 + 企业级再谈」设计。** 默认条款下 TypeSafe 可无限制处理由你的数据派生的 Telemetry（含 "learnings"）、无存储义务、无明确留存期与删除条款，不训练承诺也留有「经客户同意」的口子；ZDR 需企业协商。因此 harness 侧应默认**不把敏感原文（密钥、完整用户数据）送进 state**，优先送脱敏/结构化后的状态，并把 `state` 的组织方式（谁能看到什么、哪些字段可外发）作为一等配置；若本项目真的要把 Jev 用在需要数据留存的场景，应把 ZDR/DPA 谈判当作前置条件而非事后补丁。
