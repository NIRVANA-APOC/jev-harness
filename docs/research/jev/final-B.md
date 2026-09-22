# 片段 B：平台细节 + 性能与独立评测

> 本片段由两份上游专题文件提炼、去重、按主题重组：`04-platform-pricing.md`（平台、模型版本与定价）与 `06-benchmarks.md`（独立评测与性能数据）。两份上游的调研日期均为 **2026-09-22**。
>
> 可信度前缀约定：
> - **【官方】** = TypeSafe 官方文档 / 博客 / 定价页 / 状态页；
> - **【第三方】** = 有署名的第三方实测、报道或社区讨论。上游把云厂商/网关的集成方官方文档单列为【集成方】，本片段统一归入【第三方】并在括号内保留"集成方官方文档"字样；
> - **【未证实】** = 转述、二手、无法核对或口径不明。
>
> 上游标注为"不确定 / 未证实 / 相互冲突"的内容一律原样保留：冲突数字并列呈现并注明口径差异，不把未证实内容写成结论。跨节说明：上游的"官方 vs 第三方 vs 不可引用"三类证据在 §B.7 给出辨析框架，§B.8–B.12 逐条落地。
>
> 阅读约定：每节开头的「要点」是该节正文的摘要，**所有来源 URL 均附在紧随其后的正文对应事实处**（同一来源多次引用时只在首次给全 URL）。

---

## 一、平台细节

### B.1 模型版本与别名

- 公开的模型线只有一条：版本化 ID 为 **`jev-1.13.0`**（模型名 Jev 1.13），别名 **`jev-latest`** 与 **`jev-preview` 当前都指向 `jev-1.13.0`**。【官方】https://docs.typesafe.ai/models
- 官方 Warning 明确说："`jev-preview` 目前与 `jev-latest` 相同，**现在没有 preview 版本**"。【官方】https://docs.typesafe.ai/models
- 官方建议：**如果把 confidence 阈值调校到某个版本，应钉住版本化 ID 而不是别名**；`jev-latest` 是 SDK 默认值，会随新版本漂移。【官方】https://docs.typesafe.ai/models
- 请求里可以写别名，但**响应体里的 `model` 字段回报的是版本化 ID**（官方 Quickstart 与 API 示例响应均为 `"model": "jev-1.13.0"`）。【官方】https://docs.typesafe.ai/api 、https://docs.typesafe.ai/introduction/quickstart
- 官方 cookbook 的日志也印证了别名解析：`TypeSafe requested model: jev-latest` / `TypeSafe returned models (calls): {'jev-1.13.0': 15}`。【官方】https://docs.typesafe.ai/cookbooks/consistency_choice_cookbook
- 上一版本 **`jev-1.12` 存在且价格与 1.13 相同**（cookbook 写明 `$0.042 / $0.00`，"output tokens are free"），但官方**没有**公开的 version changelog、发布日期或迁移说明。【官方】https://docs.typesafe.ai/cookbooks/sde_cascade

各渠道登记的模型名（同一底层模型，名字与版本语义不同）：

| 渠道 | 模型标识 | 说明与来源 |
| --- | --- | --- |
| 原生 API | `jev-1.13.0` / `jev-latest` / `jev-preview` | `GET /v1/models` 当前只列别名 + description + release_date；版本化 ID 即使不在列表里也被接受。【官方】https://docs.typesafe.ai/models |
| OpenRouter | `typesafe/jev-1.13`、`~typesafe/jev-latest` | 后者是"always redirects to the latest model in the Jev family"的别名条目。【第三方】（集成方官方文档）https://openrouter.ai/typesafe 、https://openrouter.ai/~typesafe/jev-latest |
| Cloudflare Workers AI | `typesafe/jev` | 分类为 Text Generation / Third-party；示例响应仍回报 `"model": "jev-1.13.0"`。【第三方】（集成方官方文档）https://developers.cloudflare.com/ai/models/typesafe/jev/ |
| Vercel AI Gateway | `typesafe-ai/jev` | 经 AI SDK 7 的 `experimental_evaluate` 调用（需 AI SDK 7.0.105+）。【第三方】（集成方官方文档）https://vercel.com/changelog/typesafe-ai-jev-now-available-on-ai-gateway |
| Netlify AI Gateway | 未给出独立 ID | 装 `@typesafe-ai/sdk` 直接用，无需自建 API key。【第三方】（集成方官方文档）https://www.netlify.com/changelog/typesafe-jev-ai-gateway/ |
| LiteLLM 透传 | registry 登记 `typesafe/jev-1.13.0`、`typesafe/jev-latest`、`typesafe/jev-preview` | 任意 `/typesafe/` 路径被转发（如 `/typesafe/v1/systemone`）；**日志按响应上报的版本化模型记账**。【第三方】（集成方官方文档）https://docs.litellm.ai/docs/pass_through/typesafe |
| Requesty | `typesafe/jev-1.13.0`、`typesafe/jev-latest` | 标注集成 "experimental，可能无弃用期变更"。【第三方】https://www.requesty.ai/blog/typesafe-jev-explained |

发布与访问状态：官方博客称 "today in early access … opening early access and bringing developers off the waitlist"（**early access 中，正在放量**）。【官方】https://typesafe.ai/blog/introducing-system-one-models-and-jev

**本节不确定项**
1. `jev-1.13.0` 的官方发布日期未在官方页面直接给出（OpenRouter 侧写 2026-09-18）；官方无 changelog / 弃用策略，**是否存在 1.13 之外的中间版本未知**。
2. 别名漂移是复现性风险：LiteLLM 先解析到 `jev-1.13.0` 再固定，并明说未来解析可能得到不同模型（https://docs.litellm.ai/blog/jev-auto-router-benchmark ）；LangChain 的实验元数据里**根本没有 Jev 版本**；Jevals 指出 Gateway 不报告版本。
3. 第三方（datacamp、apidog）称原生 API 仍需 waitlist/邀请，但**官方页面上找不到对应文字**，官方只有"early access"表述；当前是否仍需 waitlist 未能核实。【未证实】https://www.datacamp.com/blog/system-one-models-jev 、https://apidog.com/blog/jev-api-key/

### B.2 定价与计费

- 唯一公开单价：**输入 $42 / 十亿 tokens（= $0.042 / 百万 tokens）**，**输出 tokens 免费**——官方原文 "Charged per input token. Output tokens are free."。【官方】https://docs.typesafe.ai/models 、https://typesafe.ai/blog/introducing-system-one-models-and-jev
- 计费机制是 **credits（信用点）**，来自官方主客户协议 MCA §8.2：credits "consumed by each **Input** submitted"（按输入消耗）。【官方】https://typesafe.ai/legal/mca
- credits 分两类：**Purchased Credits**（客户购买；除非订单另有约定，**到期日为 Term 结束或购买后 12 个月，取先到者**；可 opt-in 自动充值；**余额为 0 时可能被拒绝生成答案**）与 **Promotional Credits**（**TypeSafe 全权决定是否发放的免费点数**，可附带到期/撤销条件，**先于购买点数消耗**）。【官方】https://typesafe.ai/legal/mca
- credits "not redeemable, refundable, transferable"；明文**禁止开多个账号重复获取促销 credits**；美元结算，除非另有约定发票后 30 天到期，逾期月息 1.5%。【官方】https://typesafe.ai/legal/mca
- **免费额度 / 免费层 / 试用时长 / 最低充值：未找到公开信息。** `https://typesafe.ai/pricing` 返回 **404**（无独立定价页），价格只出现在文档 Models 页与博客；唯一可能的"免费"来源是 MCA 里酌情发放的 Promotional Credits。

**输出免费的第三方实证（一次）**：钓鱼邮件基准作者核对两遍共消耗 **4,561,792 token（3.66M 输入、0.90M 输出）被计费 $0.15**，与"仅输入计费"一致。【第三方】https://github.com/anisselbd/jev-phishing-bench

**各渠道计费差异**：

| 渠道 | 价格呈现 | 备注（来源） |
| --- | --- | --- |
| 原生 API / OpenRouter | `$0.042` 输入 / `Free` 输出（OpenRouter 条目写法 "$0.042 / Free"） | 【第三方】（集成方官方文档）https://openrouter.ai/typesafe/jev-1.13 |
| Cloudflare Workers AI | 页面**不列单价**，"View pricing in the Cloudflare dashboard" | 需在 Cloudflare 控制台查看。【第三方】（集成方官方文档）https://developers.cloudflare.com/ai/models/typesafe/jev/ |
| Vercel AI Gateway | 模型页价格字段显示 **"Price: Free"**（原文如此，**未说明原因**） | ⚠️ 不可解读为"免费使用"；同页要求遵守 TypeSafe 条款，且 changelog 说调用 "count toward budgets"。【第三方】（集成方官方文档）https://vercel.com/ai-gateway/models/jev 、https://vercel.com/changelog/typesafe-ai-jev-now-available-on-ai-gateway |
| Netlify AI Gateway | 用量 **billed to your Netlify credits** | 由 Netlify 计费。【第三方】（集成方官方文档）https://www.netlify.com/changelog/typesafe-jev-ai-gateway/ |

**成本量级参考（口径各不相同，勿混用）**
- 【官方】发布博客的 Doom demo（工程师担心 10 queries/秒）："which ends up costing **~$7/hour**"。https://typesafe.ai/blog/introducing-system-one-models-and-jev
- 【第三方】Requesty 的换算（假设 10 万次请求 × 1,000 输入 token = 1 亿输入 token）：**$4.20**，核价日期 2026-09-19。https://www.requesty.ai/blog/typesafe-jev-explained
- 【官方】官方 cookbook 里的成本表**自带免责声明**："They are **not verified `jev-latest` prices or current billing amounts**." https://docs.typesafe.ai/cookbooks/consistency_choice_cookbook

**本节不确定项**
1. **credits 与 token 的换算比例、最低充值额、是否按组织计费、发票/税、超量行为均未公开**；控制台内账单/用量页需登录，上游未登录、未核实。
2. **Vercel 模型页 "Price: Free" 含义不明**（可能只是价格字段未填，或指输出免费），不可据此认为该渠道免费。
3. 输出免费目前**只有一次、单一来源**的第三方实证（4.56M token 计费 $0.15），且未核对提供方发票。
4. 无免费层数字，也就**无法判断"低成本"在试用/评估阶段的可达性**——上游明确写"未找到公开信息"。

### B.3 限流与配额

- 官方口径：**250,000 tokens/second** 与 **1,200 requests/minute**；超过任一返回 **`429 Too Many Requests`**，服务过载返回 **`529 Overloaded`**。【官方】https://docs.typesafe.ai/models 、https://docs.typesafe.ai/api
- 官方 Warning 原文要点：**"Rate limits are adjusting dynamically"**，因为"正在服务非常大的需求"，**限额可能无通知变动**；**更高限额可通过 custom / enterprise 计划获得**（sales@typesafe.ai）。【官方】https://docs.typesafe.ai/models
- 因此这两个数字**只能当作上界参考，不能当作 SLA**。【官方口径 + 本片判断，来源同上】
- SDK 默认自动重试并遵守 `retry-after`；Python `RetryPolicy` 参数包括 `max_retries`（默认示例 3）、`backoff_initial`/`backoff_max`（逐次翻倍）、`backoff_jitter`、`http_statuses`（示例 `{429,500,502,503,504}`）、`respect_retry_after`（遵守 `Retry-After` / `retry-after-ms`）、`timeout`（作为**整次调用的重试总预算**）、自定义 `predicate`。【官方】https://docs.typesafe.ai/sdk/python/api/retries
- 错误码集合：`401` 无效 key、`422` 请求校验失败、`429` 限流、`529` 过载。【官方】https://docs.typesafe.ai/api

可引用的配置片段（官方 Python SDK）：

```python
from typesafe_sdk import RetryPolicy, TypeSafeClient

client = TypeSafeClient(
    retry=RetryPolicy(
        max_retries=3, timeout=10.0, http_statuses={429, 500, 502, 503, 504}
    )
)
```

**本节不确定项**
1. **分层限流表（free/pro/enterprise 各档具体数值）、账号级配额、tokens/秒 的第三方实测吞吐曲线：未找到公开信息。** OpenRouter 模型页给出延迟与 uptime，但**没有**吞吐（tokens/s）实测值（https://openrouter.ai/typesafe/jev-1.13 ）。
2. 官方明确"动态调整、可无通知变更"，**限流不可预测**；任何本地令牌桶都只能是软上界。
3. 超限后的实际退避行为、配额耗尽的计费后果（MCA 只写"may decline to generate Output"）无公开细节。

### B.4 上下文与输入限制

- **64k tokens / 请求**（`state` + 全部问题合计）；**`state` + 最长单个问题 ≤ 32k**。官方解释：`state` 只摄取一次，所有问题并行评估。【官方】https://docs.typesafe.ai/models
- **输入仅文本**：string、JSON object 或文本数组；**无 image/audio/video**，非文本需先转成文本或结构化字段。【官方】https://docs.typesafe.ai/models 、https://docs.typesafe.ai/concepts/state
- 结构上限：**Choice 最多 255 个选项**；**Score 至少 2 档、API 最多接受 10 档**。【官方】https://docs.typesafe.ai/api
- 官方明确警告**上下文越长精度越差**（"context rot"），建议先在代码里过滤，而不是把长上下文直接塞进去。【官方】https://docs.typesafe.ai/model-jaggedness/jev-1.13
- 第三方渠道口径**一致只写 32K**：OpenRouter 标 32K、Cloudflare 标 32,000 tokens、Netlify 写"约 32,000 tokens ≈ 15 万英文字符"。【第三方】（集成方官方文档）https://openrouter.ai/typesafe/jev-1.13 、https://developers.cloudflare.com/ai/models/typesafe/jev/ 、https://www.netlify.com/changelog/typesafe-jev-ai-gateway/

> 口径辨析：官方 64k 与第三方 32K **不是矛盾，而是不同粒度**——32K 对应的是"`state` + 最长问题"这一更紧的约束。引用时须写清是哪一层。

**本节不确定项**
1. **网关侧是否另有更小限制，未找到说明**；第三方渠道可能只暴露较小预算。
2. 超限行为的具体报错码（上游只给出 422 请求校验失败的通用描述），以及 64k 与 32k 两个约束是否同时硬校验，官方未逐条说明。
3. "context rot" 只有定性描述，**没有精度随长度衰减的公开曲线**。

### B.5 可用性、状态页与地区

- 官方状态页只监控一个组件 **`api.typesafe.ai`**，显示 **99.840% uptime**（统计窗口未标明），且近期仍有 API 与 Console 事件。
- 服务**托管在美国**，MCA 含美国出口管制条款；**没有可用国家/地区清单**，也没有区域化部署或数据驻留选项。
- 第三方实测确认该模型**只有 1 个 provider、无 failover**（OpenRouter 侧 3 天 Uptime 100.00%、Availability 99.87%）。
- 不拿客户 Input 训练模型；企业客户可申请 ZDR；官方速度评测跑在自家西海岸笔记本上。

- 官方状态页（Better Stack，`status.typesafe.ai`）只有一个组件：**`api.typesafe.ai` — TypeSafe API Availability：99.840% uptime**；页面提供 30/60/90 天窗口切换。【官方】https://status.typesafe.ai/
- 抓取时页面顶部 "Last updated on Sep 22, 2026 at 2:54am UTC"，当时 "All services are online"；近期事件包括 `Sep 21, 2026 — API issues — Resolved`（原文 "intermittent downtime and system instability"）与 `Sep 20, 2026 — Console is unavailable. — Resolved — Sep 21, 2026 at 8:16am UTC`；历史上 6 月下旬—8 月初有多次 2–59 分钟短时 downtime。【官方】https://status.typesafe.ai/
- 第三方可用性实测（OpenRouter 侧）：Jev 1.13 **只有 1 个 provider**，**Uptime (3d) 100.00%**、**Availability (3d) 99.87%**、**Availability (24h) 99.70%**；页面提示该模型无第二 provider 可 failover（"OpenRouter forwards every request to it directly"）。【第三方】https://openrouter.ai/typesafe/jev-1.13
- 流量侧参考：OpenRouter 作者页标注 TypeSafe 模型"**Tokens processed in the last 7 days：778B tokens**"；流量最大的公开应用包括 `sel-jev-rubric-judge`（9.18B tokens）、`mirasim`（2.9B）、`Waura`（2.65B）。【第三方】https://openrouter.ai/typesafe
- 地区与数据驻留：隐私政策写 "**The Services are hosted in the United States (U.S.)**"，来自 EEA/UK 的使用意味着数据传至美国存储与处理。【官方】https://typesafe.ai/legal/privacy-policy
- MCA §16.12（Export）：客户须遵守美国及他国出口/进口法；**不得位于（或为）受美国禁运或被列为"支持恐怖主义国家"的国家/国民**，不得提交受 ITAR 管制的信息。【官方】https://typesafe.ai/legal/mca
- 官方博客说明速度评测通常是 "from our laptops on the West Coast (this is where our service is currently based)"。【官方】https://typesafe.ai/blog/introducing-system-one-models-and-jev
- 数据处理：**不拿客户 Input 训练模型**；企业客户可申请 **zero data retention (ZDR)**。【官方】https://typesafe.ai/legal/privacy-policy 、https://docs.typesafe.ai/legal （Vercel 侧可按请求传 `zeroDataRetention: true`：【第三方】（集成方官方文档）https://vercel.com/changelog/typesafe-ai-jev-now-available-on-ai-gateway ）

**本节不确定项**
1. **99.840% 的统计窗口长度未标明**：窗口由页面控件（30/60/90 天）决定，抓取到的静态文本未写默认窗口，引用需谨慎。
2. **没有可用国家/地区清单**，也没有区域化部署或数据驻留选项（只有"企业可谈 ZDR"）；**是否对中国大陆等地区有访问限制未找到公开信息**（上游只做文档检索，未做连通性测试）。
3. **单 provider 依赖是结构性事实**：原生 API 组件只有 `api.typesafe.ai`，OpenRouter 侧确认无跨 provider failover。
4. 官方 99.840% 与 OpenRouter 99.87%/99.70% 是**不同提供方、不同窗口、不同统计口径**，不可直接比较。

### B.6 分发渠道与控制台能力

- 接入渠道已有 6+ 条，但**渠道间不是同一套契约**：OpenRouter 走 Decisions API（chat completions SDK 不可用）、Vercel 走 AI SDK 7 `experimental_evaluate`、Netlify 无需自建 key、Requesty 明示 experimental 且非流式。
- **同一模型在渠道侧的成本口径不可混用**：Cloudflare 不列单价（控制台看）、Netlify 计入 Netlify credits、Vercel 价格字段写 "Price: Free" 但调用 "count toward budgets"。
- 控制台入口为 `console.typesafe.ai`，登录只有 Google 或邮箱验证码两种方式；API key 自行创建，SDK 从 `TYPESAFE_API_KEY` 读取。
- Playground 支持把 state + 问题组合生成分享 deeplink；官方另提供 agent skill，允许编码 agent 自行试跑廉价查询来摸索用法。

- **已有 6+ 条接入路径**：原生 HTTP/SDK、OpenRouter（Decisions API）、Cloudflare Workers AI（`typesafe/jev`）、Vercel AI Gateway（`typesafe-ai/jev`）、Netlify AI Gateway、Requesty（`typesafe/jev-*`）、LiteLLM 透传（`/typesafe/v1/systemone`）。
- **OpenRouter 的关键差异**：模型条目注明它走的是 **OpenRouter Decisions API 而不是 OpenAI 兼容 chat endpoint**——"chat completions SDKs will not work with it"；且只有 TypeSafe 一个 provider，OpenRouter 不做路由。【第三方】（集成方官方文档）https://openrouter.ai/typesafe/jev-1.13
- **Vercel**：经 AI SDK 7 的 `experimental_evaluate` 调用；Choice/Score 的 confidence 出现在 `result.providerMetadata.typesafe.confidence`。【第三方】（集成方官方文档）https://vercel.com/changelog/typesafe-ai-jev-now-available-on-ai-gateway
- **Cloudflare Workers AI**：REST 形式为 `POST https://api.cloudflare.com/client/v4/accounts/$CLOUDFLARE_ACCOUNT_ID/ai/run`，body 为 `{"model":"typesafe/jev","input":{...}}`。【第三方】（集成方官方文档）https://developers.cloudflare.com/ai/models/typesafe/jev/
- **Requesty**：标注集成 "experimental，可能无弃用期变更"，仅支持 text-only user message、**非流式**、`questions` 响应格式。【第三方】https://www.requesty.ai/blog/typesafe-jev-explained
- **控制台**：入口 `console.typesafe.ai`；未登录访问 `/keys` 会 307 跳转到 `/login?returnTo=/keys`；登录只有 **Continue with Google** 或 **邮箱验证码（Email me a code）** 两种方式，需同意 terms of use 与 privacy policy。【官方，实测】https://console.typesafe.ai/keys 、https://console.typesafe.ai/login
- **API key**：官方 Quickstart 指引到 https://console.typesafe.ai/keys 创建；SDK 从环境变量 `TYPESAFE_API_KEY` 读取。【官方】https://docs.typesafe.ai/introduction/quickstart
- **Playground**：官方描述为"打开 Playground 并登录 → 粘贴任意文本作为 state → 添加问题（Noul / Choice / Score 可混用）"，**支持生成分享链接**（官方 cookbook 用 `make_playground_link(...)` 生成 `console.typesafe.ai/playground#share/<base64>` 形式的 deeplink）。【官方】https://docs.typesafe.ai/introduction/quickstart 、https://docs.typesafe.ai/cookbooks/parallel_questions
- 官方还提供 **agent skill**（Claude Code/Codex 等），并提示可"创建 API key 并允许 agent 跑便宜的测试查询来自己摸索最佳用法"；控制台内还有示例库 `console.typesafe.ai/docs/cookbooks`。【官方】https://docs.typesafe.ai/agent-skill

**本节不确定项**
1. **控制台内的用量/账单页、组织与团队管理、Playground 的导出/评测功能——均需登录，上游未登录、无法核实**，不写成事实。
2. OpenRouter 的页面代码块为前端渲染，**未能取得可复制的原始请求体**（上游据此不杜撰代码）。
3. 各网关是否支持流式、批量、并发上限等**渠道级能力差异，未找到系统对照**；已知 Requesty 非流式、OpenRouter 不支持 chat completions 形状。

---

## 二、性能与独立评测

### B.7 口径辨析框架：三类证据

- **【官方】官方自测** = TypeSafe 自家评测站与发布博客（`evals.typesafe.ai`、launch 博客）。其特征是：**参考标签由两个前沿模型的回答平均生成**，官方明说"不辩论 harness 与标签的正确性"，工作流由自家能力团队制作。
- **【第三方】第三方受控实验** = 有署名、给出样本量/并发/重复次数/统计口径、并公开原始数据或脚本的实测（LiteLLM、LangChain、Jevals、JevBench、钓鱼基准、Every、Jagent）。这类证据**可以引用，但必须连条件一起引用**。
- **【未证实】不可引用数字** = 无方法学、无法定位出处或无法从公开圆整数字复算的数字（193.6x/444.6x、"45% faster"、Vercel 的 "~5–18x"）。
- 本节要辨析的三条"跨类别陷阱"：① 官方头条乘数**无法复算**（§B.8）；② 官方准确率是**与模型标签的吻合度**，不是真值正确率（§B.8、§B.11）；③ 第三方渠道（Vercel changelog 等）常**只是转述厂商数字**，转述≠实测（§B.9）。
- 引用一条性能数字时至少要能回答四个问题：**谁测的（利益关系）、测的哪一段（分类/端到端/含网络）、什么并发与负载、参照物是什么模型什么设置**。

| 来源 | 性质 | 样本与条件 | 核心数字 | 关键局限 |
| --- | --- | --- | --- | --- |
| [evals.typesafe.ai](https://evals.typesafe.ai/) | 官方自评 | 4 个工作流，共 711 例 | 67.8% / $0.0004 / 0.4s（聚合） | 标签=两模型平均；作者自家出题；"假设 harness 正确" |
| [typesafe.ai 发布博客](https://typesafe.ai/blog/introducing-system-one-models-and-jev) | 官方公告 | 演示 + 同一组 workflow evals | 193.6x faster / 444.6x cheaper；70–500 ms；$0.042/MTok | 官方自我标注大量 nuance；速度测试跑在自家西海岸笔记本 |
| [LiteLLM](https://docs.litellm.ai/blog/jev-auto-router-benchmark) | 第三方受控 | 80 条自编案例 × 3 次 = 每分类器 240 次；并发 1；2026-09-18 | 5.43x（p50）、3.88x（p95）、成本 −96.118% | 标签作者自定无独立复核；不测下游质量；不含完成模型成本 |
| [LangChain](https://www.langchain.com/blog/jev-agent-evals-langsmith) | 第三方受控（合作方） | 5 条天气请求 × 100 次 = 500 次判定；人工 oracle | 500/500 与 oracle 一致；方差低 92–913x；$0.34 vs Claude $28.17 | 样本极小、任务单一；作者自称"观察性"证据 |
| [Jevals](https://jevals.com/) | 第三方独立 | 3 任务 × 300 问 × 5 次 = 31,500 次判定；人工标签 | 见 §B.9 | 每类问题只用一个公开数据集；无前沿模型行（预算） |
| [JevBench v1.2](https://github.com/fstandhartinger/jevbench/blob/main/RESULTS-v1.2.md) | 第三方独立 | 534 条冻结用例，48 个系统 | Jev 74.4 分第一，领先第二名 1.3 分 | 部分自托管延迟按假设 ×2 调整；成本多为估算 |
| [jev-phishing-bench](https://github.com/anisselbd/jev-phishing-bench) | 第三方独立 | PhishNChips v5.2，2000 封邮件，无并发 | 62.6% vs Haiku 81.3%；ECE 0.154 | 邮件正文为合成；真值来自 URL 信誉库 |
| [Every / Mike Taylor](https://every.to/also-true-for-humans/mini-vibe-check-typesafe-s-jev-judged-everything-i-ve-written-in-0-7-seconds) | 第三方实测 | 37 篇文档 × 21 问 = 777 判断 | <0.7s、约 1/4 美分；缺陷检出 6/7 vs 7/7 | 作者自认需更完整准确率检查；样本小 |
| [Vercel fx](https://vercel.com/changelog/typesafe-ai-jev-now-available-on-ai-gateway) | 第三方（厂商自测） | **未公布数据集与案例数** | "~5–18x faster and more accurate" | 无公开方法学 |
| [jev-agent.com（Jagent）](https://jev-agent.com/benchmarks) | 第三方（有商业利益，卖 credits） | 8 fixtures × 5 次 = 每模型 40 次 | Jev 中位 352 ms；服务器时间 1 问 70ms / 5 问 74ms | 自曝两次差点把有利于 Jev 的假象写进结论 |

**本节不确定项**
1. 上游明确说：**未做任何一手实测**，全部为公开材料核对（含对官方与第三方页面的算术复核）；第三方统计结论均为转述原作者。
2. 表中"第三方"并非质量等价：Every、LangChain、Vercel、Jagent **都有潜在利益关系或未披露关系**（Every：TypeSafe 曾在 AIE 预发布；LangChain：与 TypeSafe 联合直播；Vercel：把 Jev 上架自家 Gateway；Jagent：卖 Jev credits），引用时必须保留标注。
3. 分类标准是上游自定，**"受控实验"与"厂商自测"之间没有第三方仲裁机制**。

### B.8 官方自评：数字与口径

- 官方评测站 `evals.typesafe.ai`：4 个工作流（Security Incidents、Agent Trace Observability、Invoice Processing、Customer Service），共 **711 例**，聚合 **67.8% / $0.0004 每例 / 0.4s**。【官方】https://evals.typesafe.ai/
- **口径关键**：**参考标签由 GPT-6 Astra 与 Claude Fable 5.1 的答案平均生成，两者都开 high thinking**；"All other models are evaluated using the provider's default reasoning settings"；官方明说 **"Instead of debating the correctness of the harness and labels, we assume that the code is correct"**。【官方】https://evals.typesafe.ai/
- 官方自述的"工作流化"结论（对 harness 设计比 Jev 本身更重要）：**"Averaged across the four example tasks, every model is more accurate, cheaper and faster in the workflow than it is with the same policy as a prompt."**；上游同时记录到"同样的策略拆成工作流后，连 LLM 都变准了（Luna 51.9% → 66.8%）"。【官方】https://evals.typesafe.ai/
- 官方对"零幻觉"的写法自我打折：**"Our number is not empirical. Schema matching is guaranteed, thus we can confidently add 0% into the plots."**。【官方】https://typesafe.ai/blog/introducing-system-one-models-and-jev
- 官方对定价可持续性：**"We can't prove it isn't subsidized; we'll need the long-term to prove the sustainability of our pricing."**。【官方】同上
- 官方对速度：**"our published evals are generally run from our laptops on the West Coast"**（服务当前所在地）；对 193.6x/444.6x 的自评：**"We expect that these are on the higher end of real world gains."**；对演示查询的自我打折："The relatively shorter input **paints our model in an advantageous light**."。【官方】https://typesafe.ai/blog/introducing-system-one-models-and-jev

逐工作流数字（**第三方转述的 dashboard 圆整显示值**，两处独立来源一致）：

| 工作流 | n | Jev 准确率 / 每例成本 / 延迟 | 该工作流最强对比 | 差距 |
| --- | --- | --- | --- | --- |
| Security incidents | 240 | 61.7% / $0.0001 / 0.3s | Claude Opus 5：66.2% / $0.0574 / 15.1s | −4.5 |
| Agent trace observability | 117 | 71.6% / $0.0003 / 0.5s | GPT-5.6 Sol：76.6% / $0.0575 / 40.3s | −5.0 |
| Invoice processing | 150 | 61.8% / $0.0011 / 0.5s | GPT-5.6 Sol：79.1% / $0.2152 / 34.3s | **−17.3** |
| Customer service | 204 | 76.0% / $0.0001 / 0.4s | GPT-5.6 Sol：78.3% / $0.0323 / 10.1s | −2.3 |
| **聚合** | **711** | **67.8% / $0.0004 / 0.4s** | GPT-5.6 Sol：74.1% / $0.0836 / 23.3s | −6.3 |

来源：【第三方】https://kingy.ai/blog/typesafe-jev-review-the-ai-model-that-doesnt-generate-text/ 、https://dev.to/gabrielanhaia/jev-beat-gpt-luna-by-1-point-gpt-6-and-claude-wrote-the-answer-key-314k

聚合榜其他行（dev.to 转述官方页面，dev.to 称该页共评分 9 个模型）：GPT-5.6 Terra 67.9%（$0.0304）/ Claude Sonnet 5 67.8%（$0.1174）/ GPT-5.6 Luna 66.8%（$0.0033）/ Claude Opus 5 73.1%（$0.1761）。【第三方】https://dev.to/gabrielanhaia/jev-beat-gpt-luna-by-1-point-gpt-6-and-claude-wrote-the-answer-key-314k

> 口径提醒：**"67.8% vs 67.9%"这种"打平"是与"两个模型平均标签"的打平**，不是与真值打平（见 §B.11）。

**193.6x / 444.6x 无法从公开圆整数字复算**（上游做的算术核对）：
- flowtivity 的审计引用 TypeSafe 官网每工作流数字：**$0.000081 vs $0.013880**、**0.114 秒 vs 8.566 秒**。【第三方】https://flowtivity.ai/blog/jev-typesafe-ai-decision-model/
- 复算：8.566 / 0.114 = **75.1x**（≠ 193.6x）；0.013880 / 0.000081 = **171.4x**（≠ 444.6x）。
- kingy.ai 独立指出同一问题："The public homepage headline—193.6% faster and 444.6% cheaper—**cannot be reconstructed from the rounded table**, so quote those numbers as TypeSafe's workload-specific claims rather than universal multipliers."。【第三方】https://kingy.ai/blog/typesafe-jev-review-the-ai-model-that-doesnt-generate-text/

**本节不确定项**
1. **官方 67.8% 的"真值"不存在**：它是与 Astra+Fable 平均标签的吻合度；上游**未找到任何人工标注、覆盖相同四个工作流的独立复核**——所以"Jev 与前沿模型相当"目前**只有厂商口径支持**。
2. **193.6x / 444.6x 无法复算**：可能是 dashboard 上某个特定工作流的配对数字，但公开页面无法还原；**不要引入技术决策**。（flowtivity 的引用值 vs 官网圆整表值属于两套口径并存，冲突未解。）
3. "Luna 51.9% → 66.8%"这一对照仅在上游 §9 出现，**未附具体条件**，宜视为官方口径的二手转述。
4. 官方给 LLM 基线套了自家 **"System One LLM wrapper"**（把 LLM 约束成结构化输出），官方自述这会 **"slower and more expensive"**——**基线的公平性因此存疑**（见 §B.11）。

### B.9 第三方受控实验：逐份要点

- 最完整的一份是 **LiteLLM 的路由分类器基准**：5.43x（p50）与成本 −96.118% 都带置信区间、并发 1、三次重复、可下载原始数据与脚本——但**只覆盖分类环节**。
- **LangChain** 的裁判实验给出最一致的正面发现（500/500 与人工 oracle 一致、方差比 Terra 低 913x），但仅 5 条样本、作者自称 observational。
- **Jevals**（人工标签、31,500 次判定）显示 Jev 的优势落在价格与阈值覆盖率上，**不在准确率**；**JevBench** 的第一名也主要来自速度与成本轴。
- **Jagent** 用服务器自己的 `x-envoy-upstream-service-time` 头独立验证了"并行提问"；同时自曝两次险些把有利于 Jev 的假象写成结论。
- 所有实验都跑在 `jev-1.13.0`（或版本不可见）上，**跨版本外推需谨慎**。

- **LiteLLM Auto Router 基准**（最完整的一份，2026-09-18）：80 条自编案例 × 3 次 = 每分类器 240 次调用，并发 1，deadline 10,000 ms，**零重试、关闭熔断**，seed 20260918，`jev-latest` 先解析到 `jev-1.13.0` 后固定；Jev tier 一致率 **95.00%** vs Claude Haiku 4.5 **73.75%**；**p50 126.81 ms vs 688.40 ms（5.43x）**；成本 **$0.007706664 vs $0.198534（−96.118%）**。【第三方】https://docs.litellm.ai/blog/jev-auto-router-benchmark
- **5.43x 的定义**是 `Haiku_p50 / JEV_p50 = 688.395634 / 126.814470`，即**聚合统计量之比，不是逐请求比值的平均**；同口径 p95 = 3.88x、均值 = 5.21x；95% 聚类自助置信区间：p50 **5.20x–5.71x**、p95 **2.94–4.78**、成本节省 **95.973–96.266%**、期望 tier 匹配率差 **21.25 个百分点（12.92–30.42）**。【第三方】同上
- **成本口径极关键**：只算**分类器**成本，**不含完成模型、嵌入、预热与评估裁判**；用观测 token × 归档注册表价格，未核对提供方账单/税/折扣/套餐权益。原文："**Cost savings stop at the classifier boundary.**"。【第三方】同上
- 作者主动声明的局限（可直接引用）：「标签与 prompt 出自同一作者、无独立标注或盲审」、「不建立通用分类准确率，也不建立最终答案质量」、「分类器一致率不关心是否匹配作者标签」、「未来 `jev-latest` 可能解析到不同模型，需要重做实验」。【第三方】同上
- **LangChain（Jev 作为 agent 评估裁判）**：5 条天气请求 × 100 次重复 = **500 次判定**，人工 rubric 标签作 oracle；`does_pass` 与 oracle 一致 **Jev 500/500（100%）**，Terra 99.8%、Luna 96.4%、Claude 80.0%；分数方差（越低越好）**Jev 0.0000149**，Terra 高 **913x**、Claude 高 92x、Luna 高 433x；Jev 平均 **0.44 s / $0.00035 每次**，本次合计 **$0.34** vs Claude **$28.17**。【第三方】https://www.langchain.com/blog/jev-agent-evals-langsmith
- LangChain 作者的限制声明："This experiment **cannot tell us why**… the result is **observational**…"、"**low cost can amplify mistakes** - a consistently wrong evaluator can produce bad feedback at scale."；复现细节：Jev 经 `langchain-typesafe==0.0.1a2` 访问，**"The Jev service version was not available in the experiment metadata"**，LLM 裁判**未设 temperature/top-p/seed/max tokens**。【第三方】同上
- **Jevals.com（人工标签独立基准）**：7 个模型、**31,500 次判断**（300 问 × 5 遍 × 3 任务），人工标签，"run from a laptop at UTC+3"，总花费 $15.07（另有 $3.98 废弃运行）；明示 "Jevals is independent. It is not affiliated with TypeSafe AI"。分数口径为 **Decision Score：100 = 完美，0 = 猜标签基率**（即相对随机基线，与厂商"吻合度"口径的最大差别）。【第三方】https://jevals.com/ 、https://jevals.com/notes/2026-09-18/
- Jevals 三个任务：`noul`（PubMedQA 300 问）Jev **69.0** vs Gemini 3.8 Flash 73.0，配对差值 95% 区间 **−2.5 ~ +10.4 包含 0 → 统计并列**，Jev $0.03/千次 vs Gemini $0.80、p95 0.65s vs 5.5s（**1/28 价格**）；`choice`（Banking77）Jev **67.8**（准确率 79.7%）被 Gemini 3.8 Flash **显著领先**，与 GLM-5.3（66.8）并列第二；`score`（HelpSteer2 helpfulness 0–4）Jev **9.2** vs GLM-5.3 7.8，但结论是 **"no model clearly beats answering with the label base rates"**。【第三方】同上
- Jevals 的门槛实测（对 harness 直接有用）：给每个模型各自设定达到 **95% 准确率**的阈值后，`noul` 任务 Jev 可**独自承接 86%** 决策（Gemini 3.8 Flash 79%）；`choice` 任务 Jev **54%**（Gemini 45%、DeepSeek V4.1 Flash 39%、GLM-5.3 38%）；共享的 choice 门槛 0.96 下，Jev 承接 **59%** 且其中 **94.3%** 正确，而 LLM "在至多 15% 的样本上给出 ≥0.96"（原因之一是**偏好 0.95 这类圆整值**）。【第三方】同上
- Jevals 做的交叉对照：其 Banking77 准确率 79.7% "matches the **80.3%** a Vercel engineer reported on a different **385-item sample**"（https://github.com/vercel/ai/issues/20846 ）；官方称 70–500 ms、文档称多数查询约 100 ms，**Vercel 实测中位数 132–272 ms**，而 Jevals 从 UTC+3 测得中位 **0.44–0.48 s**、p95 **0.65–0.69 s**（含跨洲网络，Jev 只在一个美国区域提供服务）。【第三方】https://jevals.com/notes/2026-09-18/
- **JevBench v1.2**：`Score = Intelligence above chance, Calibration, Speed, Cost — 25% each, geometric mean`，**534 条冻结用例**，48 个系统。**Jev 1.13.0 得 74.4 分第一**（Intelligence 85.7 / Calibration 82.7 / Speed 83.3 / Cost 52.0；$0.0399 每千次决策；p50 0.65 s；生产 API）；第二名 **SemIf (Qwen3.5-4B) 73.1**（自托管 RunPod）**仅差 1.3 分**。【第三方】https://github.com/fstandhartinger/jevbench/blob/main/RESULTS-v1.2.md
- JevBench 里对"Jev 更聪明"最不利的一行：**GPT-5.6 Luna (low) 排第 14，但 Intelligence 95.3、Calibration 89.8 都高于 Jev**，它输在 Speed(77.5) 与 Cost(28.5，$0.2419/千次)——即 Jev 的第一名**主要来自成本与速度轴**。【第三方】同上
- **Jagent（有商业利益，卖 credits）**：8 fixtures × 5 次 = 每模型 40 次，同一 gateway，2026-09-20；Jev 1.13 中位 **352 ms**、$0.0000188；Mistral Small 3.2 1,343 ms；Gemini 2.5 Flash Lite 877 ms；GPT-5 nano 7,504 ms。另用服务器自己的 `x-envoy-upstream-service-time` 头测并行性：**1 个问题 70 ms，3 个 72 ms，4 个 81 ms，5 个 74 ms**——"'Questions are evaluated in parallel' is a vendor claim everywhere else — this is it read off the server's own header."。【第三方】https://jev-agent.com/benchmarks
- Jagent 自曝两次"差点把对 Jev 有利的假象写成结论"：没开 `response_format: {type:"json_schema", strict:true}` 就说"chat 模型返回类型不可靠"（开启后 Gemini 2/5→5/5、GPT-5 nano 4/5→5/5）；没设 `temperature: 0` 就说"chat 模型不一致"（设置后全部稳定）。结论：**"If you are evaluating a decision model against a chat model, check your own request body before you believe your own table."**；其 27 张自编工单 × 3 遍结果显示 Jev **并不在准确率上赢**（Jev 27/27、Mistral 27/27、Gemini 26/27、GPT-5 nano 26/27）；唯一只有 Jev 产出置信度：清晰集 0.979、歧义集 0.841。【第三方】同上

**本节不确定项**
1. **本轮所有实验几乎都跑在 `jev-1.13.0` 上**，跨版本外推需谨慎；LangChain、Jevals 都出现"版本不可见"问题。
2. LiteLLM 的结果是**分类环节**，明说不含端到端速度或负载下吞吐；JevBench 的自托管延迟被"×2 并加 0.15 s"（作者明说**是假设而非测量**）、成本多为估算、留出用例会暴露给被测服务。
3. 并行提问的证据：官方声称 + Jagent 服务器头实测 + Every 的 37 文档 × 21 问 <0.7s，方向一致；但上游**没有找到一份把"问题数量 vs 延迟"画成曲线的受控实验**。
4. LangChain 实验仅 5 条样本、任务单一（作者自称 observational）；Jagent 有商业利益；LiteLLM 两侧 prompt 都**未经独立调优**。

### B.10 数字分级清单：可引用 / 需标注 / 不可引用

- 引用任何性能数字前先分三档：**可引用（附条件）**、**需标注（口径限定）**、**不可引用（无方法学或不可复算）**——分档依据是证据类型，不是数字大小。
- A 类可引用的核心是：单价、限流上界、上下文上限、分类环节的 p50 与成本差、以及各独立基准的排名——**每一条都必须连条件一起引用**。
- B 类需标注的关键两条是官方 **67.8%** 与 **0% 幻觉**：前者是与模型标签的吻合度，后者官方自认 "not empirical"。
- C 类不可引用的是 **193.6x/444.6x**、**"45% faster"**、**Vercel "~5–18x"** 与转述的 YouTube "25×"（后者只是 Every 数字的翻版）。

**A. 可引用（附条件即可）**

| 数字 | 条件（必须一起写） | 来源 |
| --- | --- | --- |
| 单价 **$0.042/MTok 输入，输出免费** | 原生 API 口径；输出免费另有一次第三方实证 | 【官方】https://docs.typesafe.ai/models ；【第三方】https://github.com/anisselbd/jev-phishing-bench |
| 限流 **250k tok/s、1,200 rpm** | 官方声明"动态调整、可无通知变更"，**是上界不是 SLA** | 【官方】https://docs.typesafe.ai/models |
| 上下文 **64k/请求**、**state+最长问题 ≤32k**、仅文本、Choice ≤255、Score 2–10 | 官方文档口径；第三方渠道只标 32K | 【官方】https://docs.typesafe.ai/models 、https://docs.typesafe.ai/api |
| 端到端 **70–500 ms（官方）** | 官方自述跑在自家西海岸笔记本，**与所在区域强相关** | 【官方】https://typesafe.ai/blog/introducing-system-one-models-and-jev |
| **p50 126.81 ms vs Haiku 688.40 ms（5.43x，CI 5.20–5.71）** | 分类环节；并发 1；10s deadline；零重试；不含完成模型成本 | 【第三方】https://docs.litellm.ai/blog/jev-auto-router-benchmark |
| **单次分类成本 $0.0000321111**、成本差 **−96.118%（95.973–96.266）** | 同上，"cost savings stop at the classifier boundary" | 【第三方】同上 |
| **500/500 与人工 oracle 一致**；方差比 Terra 低 913x | 5 条样本 × 100 次；作者自称 observational；未记录 Jev 版本 | 【第三方】https://www.langchain.com/blog/jev-agent-evals-langsmith |
| **Jevals：31,500 次判定；noul 与最强 LLM 统计并列（区间含 0）；1/28 价格** | 每类问题一个数据集；人工标签；无前沿模型行 | 【第三方】https://jevals.com/ |
| **JevBench：Jev 1.13.0 74.4 分第一，领先 1.3 分** | 534 条冻结用例；部分延迟为假设调整 | 【第三方】https://github.com/fstandhartinger/jevbench/blob/main/RESULTS-v1.2.md |
| **钓鱼基准：Jev 62.6% vs Haiku 81.3%（McNemar p<0.0001）；ECE 0.154 vs 0.097** | 2000 封合成邮件；真值来自 URL 信誉库；无并发 | 【第三方】https://github.com/anisselbd/jev-phishing-bench |
| **5 信号 + 逻辑回归 = 95.1% / AUROC 0.988 / ECE 0.027** | 同上；另有非 AI 正则基线 91.6% 的对照 | 【第三方】同上 |
| **并行提问：1 问 70 ms / 5 问 74 ms** | 读服务器 `x-envoy-upstream-service-time` 头；单一第三方；有商业利益 | 【第三方】https://jev-agent.com/benchmarks |
| **状态页 99.840%** | 窗口长度未标明 | 【官方】https://status.typesafe.ai/ |

**B. 需标注后才能引用（口径限定）**

| 数字 | 必须注明的口径差异 | 来源 |
| --- | --- | --- |
| **67.8% / $0.0004 / 0.4s（聚合）** | 官方自评；参考标签 = GPT-6 Astra + Claude Fable 5.1 **high thinking 的平均**；"假设 harness 正确"；LLM 侧套了官方 wrapper | 【官方】https://evals.typesafe.ai/ |
| **0% 幻觉** | 官方自认 "not empirical"，只是 schema 匹配保证 | 【官方】https://typesafe.ai/blog/introducing-system-one-models-and-jev |
| **~$7/hour**（Doom demo） | 官方演示场景假设（10 queries/秒），非通用成本模型 | 【官方】同上 |
| **$4.20 / 10 万次请求** | 第三方换算，假设 1,000 输入 token/次，核价 2026-09-19 | 【第三方】https://www.requesty.ai/blog/typesafe-jev-explained |
| **延迟 100ms（Flavio Copes）/ 0.23s（OpenRouter P50）/ 352 ms（Jagent）/ 0.44–0.48s（Jevals）/ 132–272 ms（Vercel）** | **各来源口径完全不同**（网络位置、是否含本地 relay、是否端到端）；唯一可安全引用的是"百毫秒级绝对量级"与"同任务下比 Haiku 快数倍"的相对量级 | 见 §B.9、§B.12 各条 |
| **"Jev 与前沿模型相当"** | 仅厂商口径；无人工标注的独立复核 | 【官方】https://evals.typesafe.ai/ |
| **Vercel 385 条样本 Banking77 = 80.3%** | 与 Jevals 79.7% 一致，但样本不同、渠道不同 | 【第三方】https://github.com/vercel/ai/issues/20846 |
| **OpenRouter 778B tokens / 7 天** | 平台流量统计，非性能指标；口径为 OpenRouter 作者页 | 【第三方】https://openrouter.ai/typesafe |

**C. 不可引用（不得作为技术决策依据）**

| 数字 | 原因 | 来源/证据 |
| --- | --- | --- |
| **193.6x faster / 444.6x cheaper** | 官方自评乘数，**无法从公开圆整数字复算**（复算得 75.1x / 171.4x），官方自称"处于真实世界收益的偏高端" | 【未证实】https://flowtivity.ai/blog/jev-typesafe-ai-decision-model/ 、https://kingy.ai/blog/typesafe-jev-review-the-ai-model-that-doesnt-generate-text/ 、https://typesafe.ai/blog/introducing-system-one-models-and-jev |
| **"45% faster"（stock-analysis pipeline）** | **不是官方数字、不是任何受控基准**，是匿名用户自述轶事；原始出处未定位（Reddit 被人机验证拦截；HN 509 条评论全文搜索零命中）；AINews 原文自用 "One limited benchmark" 且无作者/数据/代码 | 【未证实】https://www.latent.space/p/ainews-jev-a-system-one-model-that |
| **Vercel "~5–18x faster and more accurate"** | 无数据集、无案例数；Vercel changelog 的措辞是 "TypeSafe **reports**…"，即**转述厂商数字**；截至 9 月 17 日未发布该基准 | 【未证实】https://vercel.com/changelog/typesafe-ai-jev-now-available-on-ai-gateway 、https://dev.to/gabrielanhaia/jev-beat-gpt-luna-by-1-point-gpt-6-and-claude-wrote-the-answer-key-314k |
| **Facebook 帖"3x DeepSeek cost、约 45% faster"** | 另一段主观比较，无方法学 | 【未证实】https://www.facebook.com/jhunter101/posts/so-ive-been-using-jev-by-type-safe-ai-for-4-hours-tonight-ive-ran-about-300000-t/4097315900569291/ |
| **YouTube 标题里的 "25×"** | 该数字就是 Every/Dan Shipper 的 0.35s vs 8.83s，**不是新的独立测试** | 【未证实】https://www.youtube.com/watch?v=hfi4ad8OA3Y |

**本节不确定项**
1. C 类里 193.6x/444.6x 与 flowtivity 引用的每工作流值（$0.000081 vs $0.013880、0.114s vs 8.566s）**可能对应 dashboard 上某个特定工作流的配对行**，但公开页面无法还原——**列为不可引用不是因为必然虚假，而是不可核对**。
2. "45% faster" 的 Facebook 变体与 AINews 版本**是两个人对两件不同事的描述**，上游明确说不可以合并成一个数字。
3. B 类中"延迟"一行是最容易误用的：**上游结论是"延迟数字不可跨来源直接比较"**。

### B.11 对官方基准的质疑（harness bias 等）

- 核心质疑是**标签来源**：参考标签由 GPT-6 Astra 与 Claude Fable 5.1 的平均生成、**无人工标注**，因此官方准确率本质是"与模型参考的吻合度"。
- 官方自己承认三类偏差：**工作流由自家能力团队制作**（"some bias could exist"）、参考答案偏向 OpenAI/Anthropic、**LLM 基线被套上自家 "System One LLM wrapper"**（官方自述这会变慢变贵）。
- **校准与可复现性未公开**：reward function、架构、训练过程、校准方法学均未披露，无公开校准曲线、无独立可复现论文。
- **类型安全 ≠ 语义正确**："A wrong Choice is not a type error"；创始人 Diogo Almeida 本人在 HN 承认 "It's also possible to be confidently wrong…"。
- 速度数字被质疑**不是同类比较**（70 ms vs 329 秒；LLM 侧可能在跑完整 chain-of-thought）。

- **标签是模型生成的，不是真值**：官方页"参考标签由 GPT-6 Astra 与 Claude Fable 5.1 在 high thinking 下的回答平均生成"，**页面没有描述任何人工标注**；后果是"两个前沿模型都答错的题，答对的模型反而被扣分；两个模型共享的盲区，有同样盲区的模型反而被奖励"。【第三方】https://dev.to/gabrielanhaia/jev-beat-gpt-luna-by-1-point-gpt-6-and-claude-wrote-the-answer-key-314k
- kingy.ai 的定性："the reported accuracy is better understood as **agreement with a model-derived reference rather than as independently verified correctness**"——本质是"a model-agreement test rather than verified ground truth"。【第三方】https://kingy.ai/blog/typesafe-jev-review-the-ai-model-that-doesnt-generate-text/
- Anthony Maio：参考标签"came from averaging GPT-6 Astra and Claude Fable 5.1 at high reasoning settings, **not from independent operational ground truth**"。【第三方】https://anthonymaio.substack.com/p/jev-the-language-model-that-wont
- **官方自己承认的 harness bias**：工作流由自家能力团队成员制作，"**some bias could exist**"；参考答案偏向 OpenAI/Anthropic（官方自称这反而**低估**自家模型与 DeepSeek）；**LLM 侧被套上 TypeSafe 自家的 "System One LLM wrapper"**（flowtivity 的审计直接记为 "against LLMs wrapped in TypeSafe's adapter"）。【官方】https://typesafe.ai/blog/introducing-system-one-models-and-jev ；【第三方】https://flowtivity.ai/blog/jev-typesafe-ai-decision-model/
- **速度数字的可比性**：HN 评论者 ramon156（经 flowtivity 转述）称"so far all claims just sound like marketing terms"，指出 **70 ms vs 329 秒不是同类比较**（若 LLM 基线在跑完整 chain-of-thought 生成）。【第三方】https://flowtivity.ai/blog/jev-typesafe-ai-decision-model/
- **"零幻觉"与"校准"的口径问题**：类型安全 ≠ 语义正确——"A wrong Choice **is not a type error**"；"confidence **is not independently demonstrated to be calibrated**"，TypeSafe 未公布标准校准指标。【第三方】https://kingy.ai/blog/typesafe-jev-review-the-ai-model-that-doesnt-generate-text/
- Maio 的结构性批评："The **reward function, architecture, training procedure, and calibration methodology are all undisclosed. There are no published calibration curves and no independently reproducible paper.**"；两条风险：校准必须在**分布漂移**下成立；**"individually calibrated judgments do not automatically compose into a calibrated workflow once you run them through thresholds, weights, and branches. Correlated mistakes survive composition."**；判决句："the strongest case for Jev is **architectural rather than algorithmic**"。【第三方】https://anthonymaio.substack.com/p/jev-the-language-model-that-wont
- flowtivity 记录了 HN 上最有价值的一次交换：TypeSafe 创始人 **Diogo Almeida 在发布帖里直接承认 "'It's also possible to be confidently wrong…'"**——即类型合法的答案仍可能错；另有转述批评称把 "frontier model" 用于"cannot hold a conversation"的模型 **"borrows credibility it has not independently earned"**。【第三方】https://flowtivity.ai/blog/jev-typesafe-ai-decision-model/

**本节不确定项**
1. HN / 社区批评多为**观点而非测量**，本节把它们与第三方定量结果（§B.12）分开呈现。
2. **校准缺乏与厂商定位相称的独立证据**：唯一直接测 ECE 的第三方实验显示 Jev ECE(0.154) 比 Haiku 4.5(0.097) 更差（钓鱼任务）；Jevals 明说每类问题只用一个数据集、校准"已知会随任务变化"——**目前不能把"概率可靠、可按阈值分支"当成已验证前提**。
3. "官方低估自家模型"是官方自述，**没有任何第三方独立验证**。

### B.12 独立评测给出的反面证据与边界行为

- **准确率反面证据**：钓鱼基准 Jev 62.6% vs Haiku 81.3%（p<0.0001，"loses clearly on accuracy…wins on speed and cost"）；Jevals 的 choice 任务 Jev 被 Gemini 3.8 Flash 显著领先；JevBench 里 GPT-5.6 Luna(low) 的 Intelligence/Calibration 都高于 Jev。
- **最一致的正面发现是"一次调用多问 + 在代码里组合"**：同一次调用里的 5 个信号喂给逻辑回归把准确率从 62.6% 提到 **95.1%**（AUROC 0.988、ECE 0.027）；但同一数据集上一条非 AI 固定规则也到 91.6%——**收益来自信号组合，而不是单次判定**。
- **边界行为是主要风险面**：弃权（有 "unknown" 选项时 95% 选弃权）、分布外过度自信、排序并列导致切分、批处理把通过的判定门变成失败——**多为索引转述，引用前需回原始仓库核对**。
- **钓鱼基准完整对照**（2,000 封 PhishNChips v5.2，无并发，每封一次调用，Jev 一次请求问 9 个类型化问题；Haiku 4.5 native Messages、无 thinking、temperature 0）：【第三方】https://github.com/anisselbd/jev-phishing-bench

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

- **钓鱼基准作者在被质疑后追加的对照（17 Sep 追加，未修改原数字）**：非 AI 基线——通用短链/免费主机/IPFS/文档托管域名清单 + 发件人与链接 eTLD+1 比对，**单靠清单规则就有 91.6% 准确率**（召回 83.5%、假阳性 0.2%），"The dataset largely separates by construction."；切分验证（B 半）：Jev 最佳单信号 89.4%，**低于两行正则的 91.8%**（p=0.0032）；Jev 5 信号回归 95.0%（AUROC 0.982）vs 正则 2 特征回归 91.8%（p=0.0018）；**把同样问题原样问 Haiku**：Haiku 最佳单信号 94.2% **高于** Jev 单信号（p<0.0001），Haiku 回归 93.2%（AUROC 0.991）与 Jev 95.0%（AUROC 0.982）**在准确率上无显著差异**（McNemar p=0.063）而 **AUROC 反而更高**；Jev 留存的优势是 **"about 27 times cheaper and 5 times faster than Haiku for signals of comparable quality, on a dataset that a regex already separates at 91.8%"**。【第三方】同上
- 钓鱼基准的四种问法都跑了（防止事后择优）：verdict(choice) 62.6% / AUROC 0.689 / ECE 0.154；is_phishing(noul) 60.6% / 0.684 / 0.172；verdict_alt_click 63.2% / 0.700 / 0.115；verdict_alt_minimal 58.4% / 0.635 / 0.241——**换措辞最多 +0.6 点，最差 −4 点**。【第三方】同上
- **Every（媒体方实测）**：37 篇文档（27 篇作者本人的 Every 文章 + 10 篇刻意 AI 风格对照）× 21 个"AI 痕迹"问题 = **777 个判断，<0.7 秒，估计约 1/4 美分**；11 个实验合计 1,709 个判断、估计总成本 < 1 美分。Dan Shipper 的 12 段合成文本对照：Jev 每段中位 **0.35 s**（Fable 5.1 高 effort 8.83 s，约 **25x** 快），估计成本约 **580x** 低，但 **Jev 只抓到 7 个缺陷中的 6 个，Fable 全抓到**。作者原话："**I'd want a more thorough accuracy check before putting it into production.** Even with that caveat, it's useful as an early warning system."。【第三方】https://every.to/also-true-for-humans/mini-vibe-check-typesafe-s-jev-judged-everything-i-ve-written-in-0-7-seconds
- **门槛/分布外/弃权/排序等边界行为**（均经 awesome-typesafe 索引转述、指向各自仓库；上游**只逐字核对了钓鱼基准与 JevBench 的原始结果文件**，引用前建议再打开仓库核对）：
  - `jev-certify`（CLINC150）：5% 的静默误路由上界在 400 条域内样本上成立（84.75% 自动路由，每来件损失 2.25%），但**另一个"范围门"在域外比例上升时超出 5% 目标 3.6 倍**。【未证实（索引转述）】https://github.com/nikkoxgonzales/jev-certify
  - `jev-orderby-bench`：20 Newsgroups 主题归属通过 6 个排序门，但在 306 条人工标注的购物相关性上**6 条里挂了 4 条**；**360 行里 53 行并列在 0.99**（两位小数输出），`LIMIT k` 会切在并列内；**把 40 行批处理成一次请求会把原本通过的排序门变成失败**。【未证实（索引转述）】https://github.com/yodablocks/jev-orderby-bench
  - `jev-calibration-audit`：KoBBQ 上，当选项里有 "unknown" 时 Jev 对 **95% 的 300 条歧义样本选弃权**；把该 gold 选项移出选项集后，这些样本准确率**必然为 0%**，且 **79% 的答案选了数据集里的刻板印象答案**。【未证实（索引转述）】https://github.com/jujumilk3/jev-calibration-audit
  - `jev-ood-calibration`：在三个公开集上概率接近校准，但在**未见过的优先级规则**上过度自信，且 Boolean 问题表现出**相反方向的误差**。【未证实（索引转述）】https://github.com/scienthoon/jev-ood-calibration
  - 111 例 agent 动作门控研究：Jev 与人工标签一致 100 例、Claude 102 例；**两者各有 1 例"不安全却放行"**；"合同与策略映射"是双方最大的单一错误来源。【未证实（索引转述）】https://github.com/ghubnab99/jev-enterprise-decision-fabric
  - 索引来源：【第三方】https://github.com/AbdelStark/awesome-typesafe

**本节不确定项**
1. §B.12 后半段的五项边界研究均为**索引转述**，上游未逐字核对原始结果文件，故标为【未证实】。
2. 钓鱼基准本身也有明确边界：**邮件正文是合成的、真值来自 URL 信誉库而非人工读信、每系统只用一套 prompt**；作者追加的对照组进一步说明该数据集"largely separates by construction"。
3. Every 的样本很小（37 篇文档、12 段合成文本），且 TypeSafe 与 Every 的关系未披露（TypeSafe 曾在 AIE 预发布），作者本人也要求更完整的准确率检查。
4. **没有任何单份第三方实验同时覆盖"准确率 + 校准 + 门槛稳定性 + 组合后的工作流"**；跨实验拼结论（例如"Jev 准确"+"Jev 校准差"）正是 Maio 警告的 "correlated mistakes survive composition" 场景。

### B.13 面向报告引用的三条收束

- 平台侧可直接落笔的事实：**版本 `jev-1.13.0`、输入 $0.042/MTok（输出免费）、250k tok/s 与 1,200 rpm（动态、非 SLA）、64k/请求与 32k（state+最长问题）、仅文本、美国托管、单一 provider、状态页 99.840%（窗口未标）**。
- 性能侧可安全引用的量级：**单次分类约 3e-5 美元**（LiteLLM $0.0000321111）、**单次判定毫秒到百毫秒级**、**同任务下比 Haiku 快数倍**、**方差显著低（LangChain 500/500 与 oracle 一致）**；**不要用 193.6x / 444.6x 或 "45% faster" 作为架构前提**。
- 评测侧的引号纪律：看到 "67.8%" 要写"与 Astra+Fable 平均标签的吻合度"；看到 "70–500 ms" 要写"官方自测、西海岸、与区域强相关"；看到 "5.43x" 要写"分类环节 p50 统计量比、含置信区间、不含完成模型成本"。

**本节不确定项**
1. 上游把"193.6x/444.6x"与"45% faster"列为不可引用，但**并未断言其为虚假**，只说明不可核对/不可复算——报告行文时应保持同样的措辞强度。
2. "Jev 是否更准"在现有公开证据下**没有一致答案**：准确率轴上多项独立测试不利于 Jev，速度/成本/稳定性轴上多项独立测试有利于 Jev；本片段不给出单一结论。
3. 若报告需要"Jev 优于 LLM"的定量支撑，现有可引用证据**只支持"成本与延迟的量级优势 + 方差的相对优势"**，不支持"准确率更高"。
