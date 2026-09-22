# 专题 04：平台、模型版本与定价

- 调研日期：2026-09-22（所有"今天/最近"均指该日期）
- 主要官方来源：`https://docs.typesafe.ai/*`（Mintlify 文档，全文经 `https://docs.typesafe.ai/llms-full.txt` 抓取）、`https://typesafe.ai/blog/...`、`https://typesafe.ai/legal/*`、`https://status.typesafe.ai/`、`https://console.typesafe.ai/*`
- 第三方来源：OpenRouter、Cloudflare AI 文档、Vercel AI Gateway、Netlify AI Gateway、LiteLLM、Requesty、The New Stack 等（文内逐条标注）
- 标注约定：**【官方】** = TypeSafe 自有渠道；**【集成方】** = 云厂商/网关/框架的官方文档；**【第三方】** = 媒体报道、博客、聚合平台的实测或转述

---

## 一、结论要点

1. **只有一个公开版本线**：当前模型版本化 ID 为 `jev-1.13.0`，别名 `jev-latest` 与 `jev-preview` **当前都指向 `jev-1.13.0`**（官方明确说"现在没有 preview build"）。**【官方】** [Models](https://docs.typesafe.ai/models)
2. **定价**：输入 **$42 / 十亿 tokens（= $0.042 / 百万 tokens）**，**输出 tokens 免费**；计费只发生在输入侧。**【官方】** [Models](https://docs.typesafe.ai/models)、[发布博客](https://typesafe.ai/blog/introducing-system-one-models-and-jev)
3. **限流**：官方口径 **250,000 tokens/秒 + 1,200 请求/分钟**，且**明确声明"动态调整、可无通知变更"**；超限 `429`，过载 `529`，SDK 默认指数退避并遵守 `retry-after`。**【官方】** [Models](https://docs.typesafe.ai/models)、[API](https://docs.typesafe.ai/api)
4. **上下文**：**64k tokens/请求**（`state` + 全部问题合计），其中 **`state` + 最长单个问题 ≤ 32k**；**仅接受文本**（无图像/音频/视频）。**【官方】** [Models](https://docs.typesafe.ai/models)
5. **可用性**：官方状态页（Better Stack）显示 `api.typesafe.ai` **99.840% 可用率**（页面提供 30/60/90 天窗口切换，静态文本未标明当前窗口；页面时间 2026-09-22 02:54 UTC）。**【官方】** [status.typesafe.ai](https://status.typesafe.ai/)
6. **接入渠道已有 6+ 条**：原生 HTTP/SDK、OpenRouter（Decisions API）、Cloudflare Workers AI（`typesafe/jev`）、Vercel AI Gateway（`typesafe-ai/jev`）、Netlify AI Gateway、Requesty（`typesafe/jev-*`）、LiteLLM 透传（`/typesafe/v1/systemone`）。
7. **免费额度/免费层：未找到公开信息。** 官方计费是 **credits 机制**（购买 credits + TypeSafe 自行决定发放的"促销 credits"），但**没有公开的免费额度数字、试用时长或最低充值**。**【官方】** [MCA §8.2](https://typesafe.ai/legal/mca)
8. **地区**：服务**托管在美国**（隐私政策"International Visitors"段），MCA 包含美国出口管制条款；官方博客说评测"一般在美国西海岸的笔记本上跑"。**未找到明确的地区封锁清单**。**【官方】** [隐私政策](https://typesafe.ai/legal/privacy-policy)、[MCA](https://typesafe.ai/legal/mca)、[发布博客](https://typesafe.ai/blog/introducing-system-one-models-and-jev)

---

## 二、关键事实与数据（逐条附来源）

### 2.1 可用模型与版本

**官方模型表**（`docs.typesafe.ai/models`，原表为 HTML 表格，此处转录关键行）：**【官方】**
| 项 | 值 |
| --- | --- |
| 模型 | Jev 1.13，ID `jev-1.13.0` |
| Price（每 Btok / 每 Mtok） | \$42 / \$0.042 |
| Rate limits | 250,000 tokens per second / 1,200 requests per minute |
| Context length | 64k tokens per request；32k tokens for `state` plus the longest question |
| Input | 仅文本：string、JSON object 或文本数组。无 image/audio/video |

- 官方补充说明：**"Charged per input token. Output tokens are free."**；限流"measured in tokens per second and requests per minute"，任一超限即 `429`，SDK 会重试并遵守 `retry-after`。URL: https://docs.typesafe.ai/models
- **别名规则**（官方向量表）：**【官方】**
  - `jev-latest` → `jev-1.13.0`："The most recent stable, official release"，SDK 默认值
  - `jev-preview` → `jev-1.13.0`："The most recent release, whether or not it is an official one"
  - 官方 Warning：`jev-preview` 当前与 `jev-latest` 相同，**现在没有 preview 版本**
  - 官方建议：**如果把 confidence 阈值调校到某个版本，应钉住版本化 ID，而不是用别名**
  - URL: https://docs.typesafe.ai/models
- `GET /v1/models` 返回账号可用的 model 名（当前只列别名）+ description + release_date；版本化 ID 即使不在列表里也被接受。URL: https://docs.typesafe.ai/models
- **响应里的 `model` 字段是版本化 ID**：官方 API/Quickstart 示例响应均为 `"model": "jev-1.13.0"`（即使请求里写 `jev-latest`）。URL: https://docs.typesafe.ai/api 、https://docs.typesafe.ai/introduction/quickstart
- 官方 cookbook 里能观察到别名解析的日志：`TypeSafe requested model: jev-latest` / `TypeSafe returned models (calls): {'jev-1.13.0': 15}`。URL: https://docs.typesafe.ai/cookbooks/consistency_choice_cookbook
- **历史版本 jev-1.12 存在**：官方 cookbook 明确写 "verifier: TypeSafe `jev-1.12` at \$0.042 / \$0.00（output tokens are free）"，说明**上一版本价格与 1.13 相同**。URL: https://docs.typesafe.ai/cookbooks/sde_cascade 。但**官方文档没有 jev-1.12 的发布日期、能力差异或迁移说明**（见"不确定"节）
- **第三方渠道的同一模型**：
  - **OpenRouter**：`~typesafe/jev-latest`（别名，"always redirects to the latest model in the Jev family"）与 `typesafe/jev-1.13` 两个条目，均**2026-09-18 上线**、**32K context**、`$0.042/M input`、`$0/M output`；页面注明**该模型只有一个 provider（TypeSafe 直连），OpenRouter 不做路由**，且**走的是 OpenRouter Decisions API，不是 `/chat/completions`——chat completions SDK 不可用**。**【集成方】** https://openrouter.ai/typesafe 、https://openrouter.ai/typesafe/jev-1.13 、https://openrouter.ai/~typesafe/jev-latest
  - **Cloudflare Workers AI**：模型 ID **`typesafe/jev`**，分类"Text Generation / Third-party"，**Context Window 32,000 tokens**，价格标注为"View pricing in the Cloudflare dashboard"（页面不列单价）；示例响应 `"model": "jev-1.13.0"`。**【集成方】** https://developers.cloudflare.com/ai/models/typesafe/jev/
  - **Vercel AI Gateway**：模型 ID **`typesafe-ai/jev`**，通过 AI SDK 7 的 `experimental_evaluate` 调用（AI SDK 7.0.105+）。**【集成方】** https://vercel.com/changelog/typesafe-ai-jev-now-available-on-ai-gateway 、https://vercel.com/ai-gateway/models/jev
  - **Netlify AI Gateway**：安装 `@typesafe-ai/sdk` 直接用，**无需自建 API key**，用量"billed to your Netlify credits"；文中重复官方口径 32,000 token 预算、70–500ms 延迟。**【集成方】** https://www.netlify.com/changelog/typesafe-jev-ai-gateway/
  - **LiteLLM 透传**：任意 `/typesafe/` 路径被转发（如 `/typesafe/v1/systemone`、`GET /typesafe/v1/models`）；其 model registry 登记 `typesafe/jev-1.13.0`、`typesafe/jev-latest`、`typesafe/jev-preview`；**日志按响应上报的版本化模型记账**（如 `typesafe/jev-1.13.0`），即使请求用了别名。**【集成方】** https://docs.litellm.ai/docs/pass_through/typesafe
  - **Requesty**：以 `typesafe/jev-1.13.0` 与 `typesafe/jev-latest` 提供，标注集成"experimental，可能无弃用期变更"，仅支持 text-only user message、非流式、`questions` 响应格式。**【第三方】** https://www.requesty.ai/blog/typesafe-jev-explained

**发布/访问状态**：官方博客称"today in early access … opening early access and bringing developers off the waitlist"（**早期访问中，正在放量**）。**【官方】** https://typesafe.ai/blog/introducing-system-one-models-and-jev
第三方（2026-09-19 左右）称原生 API 仍需 waitlist/邀请，但**经网关（OpenRouter/Vercel/Netlify/Cloudflare）可直接使用**。**【第三方】** https://www.datacamp.com/blog/system-one-models-jev 、https://apidog.com/blog/jev-api-key/ （注意：这两篇的 waitlist 描述我们未在官方页面上找到对应文字，官方只有博客里的"early access"表述）

### 2.2 计费方式与免费额度

- **官方定价（唯一公开单价）**：**输入 $42/Btok = $0.042/MTok；输出免费**。**【官方】** https://docs.typesafe.ai/models 、https://typesafe.ai/blog/introducing-system-one-models-and-jev
- **计费机制 = credits（信用点）**，来自官方主客户协议 MCA §8.2：**【官方】** https://typesafe.ai/legal/mca
  - 使用服务需先获得 TypeSafe 管理的 credits，"consumed by each **Input** submitted"（按输入消耗）；
  - 分两类：**Purchased Credits**（客户购买；除非订单另有约定，**到期日为 Term 结束或购买后 12 个月，取先到者**；可 opt-in 自动充值；余额为 0 时可能被拒绝生成答案）与 **Promotional Credits**（**TypeSafe 全权决定是否发放的免费点数**，可附加到期/撤销等条件，**先于购买点数消耗**）；
  - credits "not redeemable, refundable, transferable"；余额可在账户中查看（**具体页面需登录，本次未能核实**）；
  - 明文禁止开多个账号以重复获取促销 credits；
  - 付款条款：美元结算，除非订单另有约定，发票后 30 天到期；逾期月息 1.5%。
- **免费额度：未找到公开信息。** 官方文档、博客、定价相关页面均未给出免费 token 额度、试用期或赠送金额；`https://typesafe.ai/pricing` 返回 **404**（无独立定价页），价格只出现在文档 Models 页与博客。可预期的唯一"免费"来源是 MCA 里的 **Promotional Credits（酌情发放）**。
- **各渠道计费差异**：
  - 原生 API / OpenRouter：$0.042/M 输入、$0/M 输出（OpenRouter 条目为 "$0.042 / Free"）。**【集成方】** https://openrouter.ai/typesafe/jev-1.13
  - Cloudflare Workers AI：页面不列价，**需在 Cloudflare 控制台查看**。**【集成方】** https://developers.cloudflare.com/ai/models/typesafe/jev/
  - Vercel AI Gateway 模型页：价格字段显示 **"Price: Free"**（原文如此，**未说明原因**；该页同时要求遵守 TypeSafe 条款）。**【集成方】** https://vercel.com/ai-gateway/models/jev 。⚠️ 不要把这条解读为"免费使用"，见"不确定"节。
  - Netlify AI Gateway：用量计入 **Netlify credits**（即由 Netlify 计费）。**【集成方】** https://www.netlify.com/changelog/typesafe-jev-ai-gateway/
  - Vercel AI Gateway：评估调用"appear in logs and custom reporting, **count toward budgets**"，支持按请求开启 `zeroDataRetention`。**【集成方】** https://vercel.com/changelog/typesafe-ai-jev-now-available-on-ai-gateway
- **成本量级参考（区分口径）**：
  - 官方博客（Doom demo，工程师担心 10 queries/秒）："which ends up costing **~$7/hour**"。**【官方】** https://typesafe.ai/blog/introducing-system-one-models-and-jev
  - 第三方换算（假设 10 万次请求 × 1,000 输入 token = 1 亿输入 token）：**$4.20**，核价日期 2026-09-19。**【第三方】** https://www.requesty.ai/blog/typesafe-jev-explained
  - 官方 cookbook 里的成本表**自带免责声明**："They are **not verified `jev-latest` prices or current billing amounts**."（不要把 cookbook 里的成本数字当成现行账单依据）**【官方】** https://docs.typesafe.ai/cookbooks/consistency_choice_cookbook

### 2.3 上下文与请求规模限制

- **64k tokens/请求**（`state` + 所有问题合计）；**`state` + 单个最长问题 ≤ 32k**。官方解释：`state` 只摄取一次，所有问题并行评估。URL: https://docs.typesafe.ai/models**【官方】**
- **输入仅文本**；非文本（图像/音频/视频/二进制）需先转成文本或结构化字段。URL: https://docs.typesafe.ai/models**【官方】**
- `state` 可为 string / object / array。URL: https://docs.typesafe.ai/api 、https://docs.typesafe.ai/concepts/state**【官方】**
- **结构上限**：Choice **最多 255 个选项**；Score **至少 2 档、API 最多接受 10 档**。URL: https://docs.typesafe.ai/api**【官方】**
- 第三方渠道口径一致写 32K（OpenRouter、Cloudflare、Netlify 的"约 32,000 tokens ≈ 15 万英文字符"），需注意这与官方"64k/请求"是**不同粒度的描述**（32K 是 `state`+最长问题的约束）。**【集成方】** https://openrouter.ai/typesafe/jev-1.13 、https://developers.cloudflare.com/ai/models/typesafe/jev/ 、https://www.netlify.com/changelog/typesafe-jev-ai-gateway/
- 官方明确警告**上下文越长精度越差**（"context rot"），建议先在代码里过滤。URL: https://docs.typesafe.ai/model-jaggedness/jev-1.13**【官方】**

### 2.4 吞吐与限流

- 官方：**250,000 tokens/second** 与 **1,200 requests/minute**；超过任一返回 `429 Too Many Requests`；服务过载返回 `529 Overloaded`；重试应使用指数退避；SDK 默认自动处理并遵守 `retry-after`。**【官方】** https://docs.typesafe.ai/models 、https://docs.typesafe.ai/api
- 官方 Warning 原文要点：**"Rate limits are adjusting dynamically"**，因为"正在服务非常大的需求"，**限额可能无通知变动**；**更高限额可通过 custom / enterprise 计划获得**（sales@typesafe.ai）。URL: https://docs.typesafe.ai/models**【官方】**
- SDK 重试策略（Python，`RetryPolicy`）：`max_retries`（默认示例 3）、`backoff_initial`/`backoff_max`（逐次翻倍）、`backoff_jitter`、`http_statuses`（示例 `{429,500,502,503,504}`）、`respect_retry_after`（遵守 `Retry-After` / `retry-after-ms`）、`timeout` 作为**整次调用的重试总预算**、可自定义 `predicate`。URL: https://docs.typesafe.ai/sdk/python/api/retries**【官方】**
- 错误码集合：`401` 无效 key、`422` 请求校验失败、`429` 限流、`529` 过载。URL: https://docs.typesafe.ai/api**【官方】**
- **未找到**：分层限流表（free/pro/enterprise 各档具体数值）、账号级配额、tokens/秒 的第三方实测吞吐曲线。OpenRouter 模型页给出了延迟与 uptime，但**没有**吞吐（tokens/s）实测值。**【集成方】** https://openrouter.ai/typesafe/jev-1.13

### 2.5 可用性状态与地区限制

- **官方状态页**（Better Stack，`status.typesafe.ai`）：**【官方】** https://status.typesafe.ai/
  - 组件只有一个：**`api.typesafe.ai` — TypeSafe API Availability：99.840% uptime**（页面提供 30/60/90 天窗口切换）
  - 页面顶部"Last updated on Sep 22, 2026 at 2:54am UTC"，当时"All services are online"
  - 近期事件（原文）：`Sep 21, 2026 — API issues — Resolved — Sep 21, 2026 at 11:40pm UTC`（"intermittent downtime and system instability"）；`Sep 20, 2026 — Console is unavailable. — Resolved — Sep 21, 2026 at 8:16am UTC`
  - 历史：6 月下旬—8 月初多次 2–59 分钟不等的短时 downtime
- **第三方可用性实测（OpenRouter）**：Jev 1.13 只有 1 个 provider；**Uptime (3d) 100.00%**（可达性）、**Availability (3d) 99.87%**、**Availability(24h) 99.70%**；页面提示该模型无第二 provider 可 failover（"hosted by one provider. OpenRouter forwards every request to it directly"）。**【第三方实测】** https://openrouter.ai/typesafe/jev-1.13
- **流量侧参考**：OpenRouter 作者页标注 TypeSafe 模型"**Tokens processed in the last 7 days**：**778B tokens**"，该模型下流量最大的公开应用包括 `sel-jev-rubric-judge`（9.18B tokens）、`mirasim`（2.9B）、`Waura`（2.65B）等。**【第三方】** https://openrouter.ai/typesafe
- **地区 / 数据驻留**：**【官方】**
  - 隐私政策："**The Services are hosted in the United States (U.S.)**"，来自 EEA/UK 等的使用即意味着数据传输到美国存储与处理。URL: https://typesafe.ai/legal/privacy-policy
  - MCA §16.12（Export）：客户须遵守美国及他国出口/进口法；**不得位于（或为）受美国禁运或被列为"支持恐怖主义国家"的国家/国民**，不得违反美国出口禁令使用服务，也不得提交受 ITAR 管制的信息。URL: https://typesafe.ai/legal/mca
  - 官方博客说明速度评测通常是"from our laptops on the West Coast (this is where our service is currently based)"。URL: https://typesafe.ai/blog/introducing-system-one-models-and-jev
  - **未找到**：明确的可用国家/地区清单、区域化部署或数据驻留选项（只有"企业可谈 ZDR"：https://docs.typesafe.ai/legal ）
- **数据处理**：不拿客户 Input 训练模型；企业客户可申请 **zero data retention (ZDR)**。URL: https://typesafe.ai/legal/privacy-policy 、https://docs.typesafe.ai/legal 。Vercel 侧可按请求传 `zeroDataRetention: true`。**【集成方】** https://vercel.com/changelog/typesafe-ai-jev-now-available-on-ai-gateway

### 2.6 控制台 / Playground 能力

- **入口**：`console.typesafe.ai`。实测未登录访问 `/keys` 会 307 跳转到 `/login?returnTo=/keys`；登录页只有 **Continue with Google** 或 **邮箱验证码（Email me a code）** 两种方式，并需同意 terms of use 与 privacy policy。**【官方，实测】** https://console.typesafe.ai/keys 、https://console.typesafe.ai/login
- **API key**：官方 Quickstart 指引到 **https://console.typesafe.ai/keys** 创建 key；SDK 从环境变量 `TYPESAFE_API_KEY` 读取。**【官方】** https://docs.typesafe.ai/introduction/quickstart
- **Playground**：官方描述为"Open the [Playground](https://console.typesafe.ai/playground) and log in → paste any text as the state → 添加问题（Noul / Choice / Score 可混用）"，**并支持生成分享链接**（官方 cookbook 用 `make_playground_link(...)` 生成 `console.typesafe.ai/playground#share/<base64>` 形式的 deeplink）。**【官方】** https://docs.typesafe.ai/introduction/quickstart 、https://docs.typesafe.ai/introduction/coding-agents 、https://docs.typesafe.ai/cookbooks/parallel_questions
- **控制台内文档/示例库**：`https://console.typesafe.ai/docs/cookbooks`（agent skill 页面引用）。**【官方】** https://docs.typesafe.ai/agent-skill
- **官方还提供 agent skill**（Claude Code/Codex 等），提示可"创建 API key 并允许 agent 跑便宜的测试查询来自己摸索最佳用法"。**【官方】** https://docs.typesafe.ai/agent-skill
- **未获证实**：控制台内的用量/账单页、组织与团队管理、playground 的导出/评测功能——均需登录，本次**未登录、无法核实**（不写成事实）。
- OpenRouter 侧的模型页提供自己的 **Playground / Providers / Pricing / Performance / Uptime 标签页**，可用于试用该模型。**【集成方】** https://openrouter.ai/typesafe/jev-1.13

### 2.7 延迟与吞吐的实测口径

- **官方**：端到端 **70ms–500ms**（对比 LLM 3–329 秒），并注明"published evals are generally run from our laptops on the West Coast"。**【官方】** https://typesafe.ai/blog/introducing-system-one-models-and-jev
- **Netlify**（集成方）复述："TypeSafe reports end-to-end response times of 70–500ms"。**【集成方】** https://www.netlify.com/changelog/typesafe-jev-ai-gateway/
- **OpenRouter 实测**：**Latency P50 = 0.23 秒**（"best provider, all locations, 1 week"）。**【第三方实测】** https://openrouter.ai/typesafe/jev-1.13
- **第三方转述**：Flavio Copes 称"大多数调用约 100 毫秒"（转述官方性能水平）；Reddit 用户称"~150ms"。**【第三方】** https://thenewstack.io/typesafe-jev-system-one/ 、https://www.reddit.com/r/PiCodingAgent/comments/1whsav6/ （用户口径，非基准测试）
- **反例/边界**：官方 demo 里提到高基数 Choice 会退化（Jev 支持最多 255 选项，更高基数要用"先独立打分再显式选择"的两段式，故"occasional slowdown"）。**【官方】** https://typesafe.ai/blog/introducing-system-one-models-and-jev

---

## 三、请求 / 响应与代码示例（全部来自可核实来源）

### 3.1 原生 HTTP（官方 Quickstart）

```bash
curl -X POST https://api.typesafe.ai/v1/systemone \
  -H "Authorization: Bearer $TYPESAFE_API_KEY" \
  -H "Content-Type: application/json" \
  -d @- <<'EOF'
  {
    "state": "Hi, I've been trying to connect my Stripe account for 3 days and the integration keeps failing. I'm losing sales. Please help ASAP.",
    "model": "jev-latest",
    "questions": {
      "urgency": {
        "type": "noul",
        "instructions": "Does this message express urgency?"
      }
    }
  }
EOF
```
响应（官方原文，注意 `model` 回报版本化 ID、`usage` 同时给 input/output tokens）：**【官方】** https://docs.typesafe.ai/introduction/quickstart

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

### 3.2 Cloudflare Workers AI（`typesafe/jev`）

```js
const response = await env.AI.run(
  'typesafe/jev',
  {
    state: 'Help! My payouts have been failing for 3 days.',
    questions: {
      is_urgent: { type: 'noul', instructions: 'Does this convey urgency?',
        criteria: { true: 'Explicitly time-sensitive', false: 'No urgency expressed' } },
    },
  },
)
```
对应 REST 形式：`POST https://api.cloudflare.com/client/v4/accounts/$CLOUDFLARE_ACCOUNT_ID/ai/run`，body 为 `{"model":"typesafe/jev","input":{...}}`。**【集成方】** https://developers.cloudflare.com/ai/models/typesafe/jev/

### 3.3 Vercel AI Gateway（`typesafe-ai/jev`，AI SDK 7）

```js
import { experimental_evaluate as evaluate } from 'ai';

const result = await evaluate({
  model: 'typesafe-ai/jev',
  state: 'The support agent issued a full refund to the customer.',
  questions: {
    refunded: { type: 'boolean', instructions: 'Was a refund issued?' },
  },
  providerOptions: { gateway: { zeroDataRetention: true } },
});
console.log(result.answers.refunded);
```
Choice/Score 的 confidence 出现在 `result.providerMetadata.typesafe.confidence`。**【集成方】** https://vercel.com/changelog/typesafe-ai-jev-now-available-on-ai-gateway

### 3.4 重试配置（官方 Python SDK）

```python
from typesafe_sdk import RetryPolicy, TypeSafeClient

client = TypeSafeClient(
    retry=RetryPolicy(
        max_retries=3, timeout=10.0, http_statuses={429, 500, 502, 503, 504}
    )
)
```
**【官方】** https://docs.typesafe.ai/sdk/python/api/retries

### 3.5 走 OpenRouter 时的注意点（无代码样例，仅原文说明）

OpenRouter 模型页原文："It runs on the **OpenRouter Decisions API** rather than the OpenAI-compatible chat endpoint, so the request and response shapes below are specific to structured decisions — **chat completions SDKs will not work with it**." 页面同时给出 `~typesafe/jev-latest` 作为 model ID。**【集成方】** https://openrouter.ai/typesafe/jev-1.13
（本次抓取的 OpenRouter 页面代码块为前端渲染，未能取得可复制的原始请求体，故不杜撰代码。）

---

## 四、不确定与未证实之处

1. **免费额度 / 免费层 / 试用**：完全未找到公开数字。已检索官方文档全文（llms-full.txt）、博客、MCA、隐私政策、状态页、`typesafe.ai/pricing`（404）。只能确认 MCA 里的 **Promotional Credits** 机制（由 TypeSafe 酌情发放）。
2. **计费细节**：credits 与 token 的换算比例、最低充值额、是否按组织计费、发票/税、超量行为（MCA 说余额为 0 时"may decline to generate Output"）——**未公开**；控制台内页面需登录，未核实。
3. **Vercel 模型页显示 "Price: Free"**：含义不明（可能只是价格字段未填或指输出免费）。**不可据此认为通过 Vercel 调用免费**；Vercel 渠道还提到调用"count toward budgets"。
4. **上下文口径差异**：官方写 64k/请求（`state`+全部问题）与 32k（`state`+最长问题）；OpenRouter/Cloudflare/Netlify 只写 32K。两者不矛盾，但第三方渠道可能只暴露较小预算——**在网关侧是否另有更小限制，未找到说明**。
5. **版本历史**：官方仅在 cookbook 中提到 `jev-1.12`（同价），**没有公开的 version changelog、发布日期或弃用策略**；`jev-1.13.0` 的发布日期也未在官方页面直接给出（OpenRouter 写 09-18）。**是否存在 `jev-1.13` 之外的中间版本，未知。**
6. **地区可用性**：只有"托管在美国"+"出口管制"两条，**没有可用国家清单**；是否对中国大陆等地区有访问限制，**未找到公开信息**（本次仅做文档检索，未做实际连通性测试）。
7. **状态页统计窗口**：99.840% 的窗口长度由页面控件决定（页面提供 30/60/90 天切换），本次抓取到的静态文本未标明默认窗口，引用时需谨慎。
8. **限流的可预测性**：官方明确"动态调整、可无通知变更"，因此 **250k tok/s / 1,200 rpm 只能当上界参考，不能当作 SLA**。
9. **单 provider 依赖**：OpenRouter 明确该模型只有 TypeSafe 一个 provider，**无跨 provider failover**；原生 API 同样单点（状态页组件只有 `api.typesafe.ai`）。
10. **早期的 waitlist 说法**：由第三方（datacamp、apidog）提出；官方只有"early access / bringing developers off the waitlist"的表述，**当前是否仍需 waitlist 未能核实**（未实际注册）。

---

## 五、对本项目（Rust Agent Harness）Jev 定位的 1–3 条启示

1. **钉版本、记版本**：把模型 ID 固定为 `jev-1.13.0` 而不是 `jev-latest`（别名会漂移，官方自己建议"调过 confidence 阈值就钉版本"），并把响应里的 `model` 字段写进 harness 的审计日志——这样评分/阈值的行为变化可追溯到具体模型版本。见 2.1 官方向量与建议。
2. **成本与限流建模要按"输入侧 + 动态上限"设计**：成本 ≈ `input_tokens × $0.042/M`，输出免费 → 应尽量**一次请求塞多个问题（fan-out）**，因为并行问题"几乎不增加响应时间"、也不再额外计输出成本；同时受 `state`+最长问题 ≤32k 的硬约束，harness 需要**在发送前做 state 裁剪/检索**。限流侧按 `250k tok/s`、`1,200 rpm` 作为**软上界**实现本地令牌桶，并实现 429/529 + `retry-after` 退避（可参考官方 `RetryPolicy` 的语义：`http_statuses={429,500,502,503,504}`、总超时预算、抖动）。
3. **把"单一供应商 + 99.84% 可用率"当作架构前提**：官方状态页 90 天 99.840%、最近 3 天仍有 API/Console 事件，且 OpenRouter 侧确认该模型只有 1 个 provider、无 failover。因此 harness 的 Jev 通道应设计**降级路径**（回落到 LLM 判定、走确定性规则、或"暂缓判断/请求人工"），并把"Jev 不可用/限流"当成一等错误类型而不是例外；如果走网关（OpenRouter/Vercel/Netlify/Cloudflare）接入，还需注意 **OpenRouter 不支持 chat completions 形状**、Cloudflare 价格在控制台、Netlify 计入 credits、Vercel 标注 "Free" 但计入预算等差异（见 2.1/2.2）。

---

## 附：来源清单（全部为本次实际抓取）

官方（TypeSafe）：
- https://docs.typesafe.ai/models （版本、价格、限流、上下文、别名、语言支持、数据）
- https://docs.typesafe.ai/api （端点、请求/响应、Choice 255 上限、Score 2–10、429/529、退避）
- https://docs.typesafe.ai/introduction/quickstart （cURL、响应示例、console keys、Playground）
- https://docs.typesafe.ai/introduction/coding-agents 、https://docs.typesafe.ai/introduction （Playground、定位）
- https://docs.typesafe.ai/model-jaggedness/jev-1.13 （上下文长度提示、context rot、Last reviewed 2026-09-17）
- https://docs.typesafe.ai/sdk/python/api/retries （重试策略）
- https://docs.typesafe.ai/cookbooks/sde_cascade （jev-1.12 与 $0.042/$0.00；别名解析日志）
- https://docs.typesafe.ai/cookbooks/consistency_choice_cookbook （成本表免责声明）
- https://docs.typesafe.ai/legal （ZDR 企业选项）
- https://typesafe.ai/blog/introducing-system-one-models-and-jev （定价、70–500ms、early access、美西）
- https://typesafe.ai/legal/mca （credits、促销 credits、出口管制、支付条款）
- https://typesafe.ai/legal/privacy-policy （美国托管、不用于训练）
- https://status.typesafe.ai/ （99.840%、事件）
- https://console.typesafe.ai/keys 、https://console.typesafe.ai/login （登录方式、跳转行为，未登录实测）

集成方（云厂商/网关/框架）：
- https://openrouter.ai/typesafe 、https://openrouter.ai/typesafe/jev-1.13 、https://openrouter.ai/~typesafe/jev-latest （32K、$0.042/Free、P50 0.23s、uptime/availability、单 provider、Decisions API、778B tokens/7 天）
- https://developers.cloudflare.com/ai/models/typesafe/jev/ （typesafe/jev、32,000 tokens、控制台定价）
- https://vercel.com/changelog/typesafe-ai-jev-now-available-on-ai-gateway 、https://vercel.com/ai-gateway/models/jev （typesafe-ai/jev、AI SDK 7、ZDR、Price: Free）
- https://www.netlify.com/changelog/typesafe-jev-ai-gateway/ （Netlify credits、32k≈15 万字符、70–500ms）
- https://docs.litellm.ai/docs/pass_through/typesafe （model registry 三个名字、按版本记账）

第三方（媒体/博客/实测）：
- https://www.requesty.ai/blog/typesafe-jev-explained （2026-09-19 核价、$4.20 示例、Requesty model ID、experimental 提示）
- https://thenewstack.io/typesafe-jev-system-one/ （2026-09-15 发布、$40M DCVC 种子轮、Vercel 采纳数据、~100ms 转述、非文字输入测试）
- https://www.datacamp.com/blog/system-one-models-jev 、https://apidog.com/blog/jev-api-key/ （waitlist 说法，未在官方核实）
- https://www.reddit.com/r/PiCodingAgent/comments/1whsav6/ （用户称 ~150ms，非基准）
