# 专题 02：官方 Quickstart 与 API 契约

> 调研日期：2026-09-22。除特别标注外，所有事实均来自 TypeSafe 官方文档原文（Mintlify 站点的 `.md` 版本，可逐条核对），第三方来源单独标注。
> 官方文档索引：<https://docs.typesafe.ai/llms.txt>（全站纯文本版：<https://docs.typesafe.ai/llms-full.txt>，约 903 KB，本报告做过全量关键词检索）

## 一、结论要点

1. **只有一个核心 endpoint**：`POST https://api.typesafe.ai/v1/systemone`，另有辅助的 `GET /v1/models`。**没有** /chat/completions 之类的兼容端点，也没有批量、异步任务或 webhook 端点。
2. **认证方式**：HTTP 头 `Authorization: Bearer <API_KEY>` + `Content-Type: application/json`。官方 Python/JS SDK 从环境变量 `TYPESAFE_API_KEY` 读取密钥；base URL 可用 `TYPESAFE_BASE_URL` 覆盖（默认 `https://api.typesafe.ai`）。
3. **请求体恰好三个顶层字段**：`state`（string | object | array）、`model`（string，如 `jev-latest`）、`questions`（`map<string, Question>`，key 由调用方自取）。**question id 不会发送给模型，也不参与推理**，只用于把答案对应回来。
4. **三种问题类型 + 各自的 `criteria` 形状**：Choice 为 `map<选项, 描述>`（**上限 255 个选项**）；Score 为**有序数组**（**至少 2 级、最多 10 级**，层级即 0 基索引）；Noul 的 `criteria` 可选，为 `{true, false}` 两个分支的语义说明。
5. **响应体三个顶层字段**：`model`（实际作答的**版本化 ID**）、`answers`（按你给的同名 id 回填）、`usage`（`input_tokens` / `output_tokens`）。
6. **答案字段语义**（这是本项目最需要的一手材料）：
   - Noul：`{type, noul}`，`noul ∈ [0,1]` 是「答案为是」的概率，**没有 confidence 字段**；
   - Choice：`{type, choice, probabilities, confidence}`，`probabilities` 是 `选项 → 概率` 的 map（和为 1，官方措辞为 "floats that sum to 1"；SDK schema 措辞为 "approximately 1"），`choice` 是概率最高的选项；
   - Score：`{type, score, legend, probabilities, confidence}`，`score` 是**按概率加权的期望值，可落在两级之间**（文档示例 `1.05`、`1.6`、`1.7`），`legend` 把级号映射回描述，`probabilities` 是 `级号(字符串) → 概率`。
7. **confidence 是概率分布的统计量**，只在 Choice / Score 上返回；官方明确表示这只是「适合多数场景的便捷度量」，**不锁定其定义**，因为完整 `probabilities` 也一并返回，允许调用方自算。
8. **流式返回：未找到任何公开支持**。全站文档（llms-full.txt）与两套官方 SDK 源码中都没有 `stream` / SSE / `text/event-stream` / websocket 的 API 痕迹；JS SDK 内部函数 `bufferResponse` 也是先把响应整体读入再解析。官方在「Jev with coding agents」页明确说 Jev 不生成文本、不做 stream。**注意这是「未找到证据」，不是「官方声明不支持」**。
9. **错误码**：官方 API reference 列出 `401 / 422 / 429 / 529`；两套 SDK 另外显式映射了 `400 / 401 / 403 / 404 / 422 / 429 / 5xx`。**错误响应体的 JSON 结构官方没有写成契约**（详见「不确定之处」）。
10. **限流**：jev-1.13 为 **250,000 tokens/秒 且 1,200 requests/分钟**，任一超限返回 `429`；文档同时以 Warning 形式声明**限流数值会动态调整、可能无预警变化**。SDK 默认指数退避重试，并尊重服务端的 `retry-after` / `retry-after-ms` 头。

## 二、关键事实与数据（逐条附出处）

### 2.1 Endpoint / base URL / 认证

| 事实 | 出处 |
| --- | --- |
| `POST https://api.typesafe.ai/v1/systemone`，头为 `Authorization: Bearer <API_KEY>` 与 `Content-Type: application/json` | <https://docs.typesafe.ai/api.md>（Evaluation endpoint 一节）；<https://docs.typesafe.ai/introduction/quickstart> |
| 另有 `GET /v1/models` 返回账号可用模型名、描述、发布日期 | <https://docs.typesafe.ai/models.md>（Listing models 一节） |
| SDK 默认 base URL `https://api.typesafe.ai`，环境变量 `TYPESAFE_BASE_URL` | <https://docs.typesafe.ai/sdk/python/api/constants.md> |
| API key 环境变量名 `TYPESAFE_API_KEY`；默认模型环境变量 `TYPESAFE_DEFAULT_MODEL`；日志级别 `TYPESAFE_LOG_LEVEL` | 同上 constants 页 |
| SDK 源码常量：`SYSTEM_ONE_PATH = "/v1/systemone"`、`MODELS_PATH = "/v1/models"`、`AUTHORIZATION_HEADER = "Authorization"` | <https://raw.githubusercontent.com/typesafe-ai/typesafe-sdk-python/main/src/typesafe_sdk/_core/constants.py> |
| 迁移页明确「认证不变：继续发送 `Authorization: Bearer <API_KEY>` 与 `Content-Type: application/json`」 | <https://docs.typesafe.ai/migrating-to-v1.md> |
| 第三方（LiteLLM pass-through）确认同一路径，并说明 `GET /typesafe/v1/models` 可用、按 `typesafe/<model>` 计费与记日志 | <https://docs.litellm.ai/docs/pass_through/typesafe> |
| 第三方（LangChain 博文）确认「发送 state 与 questions」的调用形态，但未涉及 endpoint 与认证 | <https://www.langchain.com/blog/building-a-harness-with-jev> |
| 第三方（dev.to 实践指南）写明「只有一个 endpoint：`POST https://api.typesafe.ai/v1/systemone`」 | <https://dev.to/valyuai/how-to-use-jev-a-practical-guide-to-typesafes-system-one-model-g5e> |

**只在 SDK 源码中可见、文档未列出的请求/响应头**（对 harness 的日志与追踪有用）：
`X-TypeSafe-SDK`、`X-TypeSafe-Runtime`、`X-TypeSafe-Retry-Count`、响应头 `x-typesafe-request-id`（SDK 的 `request_id` 属性来源）、`retry-after` / `retry-after-ms`。
出处：<https://raw.githubusercontent.com/typesafe-ai/typesafe-sdk-python/main/src/typesafe_sdk/_core/constants.py>

### 2.2 请求体字段与约束

| 字段 | 类型 | 约束 / 说明 | 出处 |
| --- | --- | --- | --- |
| `state` | `string \| object \| array` | 必填。要评估的内容：字符串用于普通文本；对象用于命名多段上下文（推荐）；数组用于消息序列或记录序列。**仅纯文本**，不支持图像/音频/视频；非文本输入需先转成文本或结构化字段 | <https://docs.typesafe.ai/api.md>、<https://docs.typesafe.ai/concepts/state.md>、<https://docs.typesafe.ai/models.md> |
| `model` | `string` | 必填。可用 `jev-latest`（SDK 默认）；别名 `jev-preview` 目前同样指向 `jev-1.13.0`；版本化 ID 如 `jev-1.13.0` 始终可用 | <https://docs.typesafe.ai/api.md>、<https://docs.typesafe.ai/models.md> |
| `questions` | `map<string, Question>` | 必填。key 由调用方自取，**不发送给模型、不用于推理**；答案按同一 key 返回。SDK 侧要求非空 mapping | <https://docs.typesafe.ai/api.md>、<https://docs.typesafe.ai/sdk/python/api/clients/sync.md> |
| `‹question›.type` | `"noul" \| "choice" \| "score"` | 判别字段 | <https://docs.typesafe.ai/api.md> |
| `‹question›.instructions` | `string \| object \| array` | 必填。可为结构化对象：把问题放一个字段、把被引用的数据放其他字段，并用反引号按名字引用（或用点号+索引路径指向 `state` 的嵌套字段，如 `ticket.messages[0].text`） | <https://docs.typesafe.ai/api.md>、<https://docs.typesafe.ai/primitives.md> |
| Choice `criteria` | `map<string, string \| object \| array \| null>` | 必填。**每个 Choice 最多 255 个选项**；选项无需额外说明时用 `null` | <https://docs.typesafe.ai/api.md> |
| Score `criteria` | `array<string \| object \| array>` | 必填。**有序**等级描述数组，**至少 2 级，API 最多接受 10 级**；层级顺序即数组顺序，level 0 是第一个元素 | <https://docs.typesafe.ai/api.md>、<https://docs.typesafe.ai/primitives.md> |
| Noul `criteria` | `object`（可选） | `{ "true": ..., "false": ... }`，分别描述「接近 1 的是」与「接近 0 的非」意味着什么 | <https://docs.typesafe.ai/api.md> |

补充事实：

- **上下文预算**：64k tokens / 请求（`state` + 全部问题合计）；其中 **32k** 适用于 `state` + 单个最长问题。出处：<https://docs.typesafe.ai/models.md>
- **同一请求内的问题互相独立、并行评估**，一个问题的答案不会成为另一个问题的上下文；需要依赖关系时要发第二个请求。出处：<https://docs.typesafe.ai/primitives.md>
- **未找到官方给出的「单请求最多多少个问题」的硬上限**；限制体现为上述 token 预算与每个问题的 token 成本。
- 第三方（dev.to）把上述约束逐条复述并补充了 SDK 端行为，与官方一致：<https://dev.to/valyuai/how-to-use-jev-a-practical-guide-to-typesafes-system-one-model-g5e>

### 2.3 响应结构与概率 / 置信度语义

| 字段 | 类型 | 语义 | 出处 |
| --- | --- | --- | --- |
| `model` | `string` | 实际作答的模型；**通常是版本化 ID**（示例均为 `jev-1.13.0`，即使请求发的是别名），便于记录/回溯 | <https://docs.typesafe.ai/api.md>、<https://docs.typesafe.ai/models.md> |
| `answers` | `map<string, Answer>` | 与请求 `questions` 同 key 一一对应 | <https://docs.typesafe.ai/api.md> |
| Noul answer `.noul` | `number` | 「答案为是」的概率，0（否）到 1（是），接近 0.5 表示不确定。**无 confidence 字段** | <https://docs.typesafe.ai/api.md>、<https://docs.typesafe.ai/primitives.md>、<https://docs.typesafe.ai/confidence.md> |
| Choice answer `.choice` | `string` | 概率最高的选项 | <https://docs.typesafe.ai/api.md> |
| Choice answer `.probabilities` | `map<string, number>` | 每个选项的概率，官方写「sum to 1」，SDK JSON schema 写「sum to approximately 1」 | 同上；schema 见 <https://docs.typesafe.ai/sdk/python/api/types/responses.md> |
| Choice answer `.confidence` | `number` | 0–1，由概率分布推导而来 | <https://docs.typesafe.ai/api.md> |
| Score answer `.score` | `number` | **按概率加权的期望值**，可落在两级之间（文档示例 1.05 / 1.6 / 1.7） | <https://docs.typesafe.ai/api.md>、responses.md 的 schema 描述 |
| Score answer `.legend` | `map<string, string>` | 级号 → 该级描述 | <https://docs.typesafe.ai/api.md> |
| Score answer `.probabilities` | `map<string, number>` | `级号(字符串) → 概率`，和为 1 | 同上 |
| Score answer `.confidence` | `number` | 0–1，由概率分布推导 | 同上 |
| `usage.input_tokens` / `usage.output_tokens` | `integer \| null` | token 用量；SDK schema 允许 `null`（API 未上报时） | <https://docs.typesafe.ai/api.md>、<https://docs.typesafe.ai/sdk/python/api/types/responses.md> |

**confidence 的含义（官方原文要点）**：

- `confidence` 是**从 `probabilities` 计算出的统计量**，TypeSafe 替你算好并返回；分布越集中在单一结果上越接近 1，越平均越低。出处：<https://docs.typesafe.ai/confidence.md>
- 官方明确 **不锁定该定义**：「a convenient measure that fits most use-cases, but you are never locked into our definition ... which is exactly why we give you the full `probabilities`」。出处同上。
- 文档里的交互演示用 `(n × 最大概率 − 1) / (n − 1)` 近似（n 个选项时）计算 confidence —— 这是**演示用近似公式**，不等于服务端实现。出处同上。
- 迁移页说明 v1 的 confidence **换了新算法**，与 preview 版数值不同；并给出用归一化香农熵复现 **preview 版** confidence 的 Python 代码（`1 − entropy/log(n)`）。出处：<https://docs.typesafe.ai/migrating-to-v1.md>
- 官方建议的三档用法：高置信自动执行 / 中置信先确认或补充信息 / 低置信不执行、转人工；阈值应随风险分级。出处：<https://docs.typesafe.ai/confidence.md>

### 2.4 错误码与限流

| 状态码 | 含义 | 出处 |
| --- | --- | --- |
| `401 Unauthorized` | API key 缺失或无效，检查 `Authorization` 头 | <https://docs.typesafe.ai/api.md> |
| `422 Unprocessable Entity` | 请求体校验失败（如缺必填字段、问题格式错误），**响应体会指出出错字段** | 同上 |
| `429 Too Many Requests` | 超出限流，退避后重试 | 同上 |
| `529 Overloaded` | 服务暂时过载，短暂延迟后重试 | 同上 |
| `400 / 401 / 403 / 404 / 422 / 429 / 5xx` | SDK 的异常类型映射（`TypeSafeBadRequestError` / `AuthenticationError` / `PermissionDeniedError` / `NotFoundError` / `UnprocessableEntityError` / `RateLimitError` / `InternalServerError`） | <https://docs.typesafe.ai/sdk/python/api/exceptions.md>，源码 <https://raw.githubusercontent.com/typesafe-ai/typesafe-sdk-python/main/src/typesafe_sdk/_core/errors.py> |

**限流与重试（官方口径）**：

- jev-1.13 限额：**250,000 tokens/秒；1,200 requests/分钟**；任一超限返回 `429 Too Many Requests`。官方 SDK 默认退避重试并尊重 `retry-after` 头。出处：<https://docs.typesafe.ai/models.md>
- 官方 Warning：**限流会动态调整、可能无预警变化**（正在扩容，大型 GPU 交易落地期间）；更高限额走 custom / enterprise 方案，联系 sales@typesafe.ai。出处同上。
- 恢复策略：收到 `429` 或 `529` 时**指数退避重试**，不要立即重试；用官方 SDK 的默认重试策略即可。出处：<https://docs.typesafe.ai/api.md>（Handling rate limits）
- Python SDK 默认重试策略（`RetryPolicy`）：`max_retries=2`、`backoff_initial=0.5s`（每次翻倍，上限 `backoff_max=5.0s`）、`backoff_jitter=0.25`、重试状态码 `{408, 429, 500–599}`、`respect_retry_after=True`、重试总预算 `timeout=30.0s`；`TypeSafeRateLimitError.retry_after_ms` 暴露服务端要求的等待毫秒数。出处：<https://docs.typesafe.ai/sdk/python/api/retries.md>、<https://docs.typesafe.ai/sdk/python/api/exceptions.md>
- JS SDK 默认值同构：`maxRetries=2`、`backoffInitialMs=500`、`backoffMaxMs=5000`、`backoffJitter=0.25`、重试 408/429/500–599、`respectRetryAfter=true`、`maxRetryAfterMs=60000`（更长的服务端延迟改用退避）。出处：<https://docs.typesafe.ai/sdk/javascript/api/interfaces/RetryPolicy.md>
- Python SDK 另有单次 HTTP 操作默认超时 `DEFAULT_TIMEOUT = 10.0s`。出处：<https://docs.typesafe.ai/sdk/python/api/constants.md>

### 2.5 模型、版本与价格（官方口径）

| 事实 | 数值 / 说明 | 出处 |
| --- | --- | --- |
| 当前模型 | `jev-1.13.0`（Jev 1.13） | <https://docs.typesafe.ai/models.md> |
| 价格 | **$42 / Btok = $0.042 / Mtok；仅按输入 token 计费，输出 token 免费** | 同上 |
| 限流 | 250,000 tokens/s；1,200 requests/min | 同上 |
| 上下文 | 64k tokens/请求；其中 32k 为 `state` + 最长单个问题 | 同上 |
| 输入模态 | 仅文本（string / JSON object / 文本数组），无图像/音频/视频 | 同上 |
| 别名 | `jev-latest` → `jev-1.13.0`（SDK 默认）；`jev-preview` → 当前也指向 `jev-1.13.0` | 同上 |
| 语言 | 英文为主要训练语言、当前准确率最好；其他语言（含 CJK）可用但准确率较低，需自行测试并关注 confidence | 同上 |
| 数据使用 | **官方称不会用客户请求/响应训练**；企业版有 ZDR 选项 | 同上 |
| 定制 | **不做客户数据微调或 LoRA**；同一套权重服务所有账号，通过 `state` + `instructions` + `criteria` 塑造行为 | 同上 |

### 2.6 契约演进史（用于确认字段语义与稳定性）

`/preview/evaluation` → `/v1/systemone` 是一次**破坏性变更**，迁移页逐条列出对照，正好交叉印证了 v1 的字段含义。出处：<https://docs.typesafe.ai/migrating-to-v1.md>

| 维度 | preview | v1（当前） |
| --- | --- | --- |
| Endpoint | `POST /preview/evaluation` | `POST /v1/systemone` |
| 问题集合 | `prompts` **数组**，每项自带 `key` | `questions` **map**，key 即 id |
| 内容字段 | `document` | `state`（v1 只接受 `state`，传 `document` 校验失败） |
| 描述字段 | 按类型分 `criteria` / `options` / `levels` | 统一为 `criteria`（形状按类型不同） |
| 答案集合 | `responses` **数组**（同序） | `answers` **map**（按 id） |
| Noul 值字段 | `probability` | `noul` |
| Choice 值字段 | `chosen` | `choice` |
| Score 值字段 | `expectation` | `score` |
| Choice `probabilities` | `[{option, probability}]` 数组 | `{option: probability}` map |
| Score `probabilities` | 不返回 | 新增，`{level: probability}` map |
| `confidence` | 旧算法 | 新算法（数值会变，需重新校准阈值） |
| `usage` | `billing_units` 占位 | `input_tokens` / `output_tokens` |
| Python 包 | `typesafe-client`（所有 0.1.x / 1.0.x 版均已失效） | `typesafe-sdk` |

### 2.7 流式返回：证据与结论

- 官方 API reference 未列出任何流式参数、流式响应类型或事件协议。出处：<https://docs.typesafe.ai/api.md>
- 全站纯文本（llms-full.txt，903 KB）检索 `stream` / `server-sent` / `text/event-stream` / `websocket`：**没有任何 API 相关命中**；唯一与 stream 相关的表述是「编码 agent 依赖会 stream 文本的 LLM ... **Jev does none of that**」（<https://docs.typesafe.ai/introduction/coding-agents>）。检索命令：`curl -sL https://docs.typesafe.ai/llms-full.txt | grep -in "stream\|text/event-stream\|websocket"`
- JS SDK `TypeSafeClient` 只有 `systemOne()` 与 `models.list()` 两个方法，无 streaming 变体。出处：<https://docs.typesafe.ai/sdk/javascript/api/classes/TypeSafeClient.md>
- JS SDK 源码中响应处理函数名为 `bufferResponse`（先把整个响应读入），无任何流式解析分支。出处：<https://raw.githubusercontent.com/typesafe-ai/typesafe-sdk-js/main/src/client.ts>
- Python SDK 源码（`_core/transport.py`、`_core/endpoints.py`、各 client）中检索 `stream` 无命中；请求构造是「一次性 POST 一个 JSON body」。出处：<https://github.com/typesafe-ai/typesafe-sdk-python>

**结论**：截至 2026-09-22，**未找到任何公开的流式返回支持**。这是「未找到证据」，不是官方明示的「不支持」。

## 三、请求-响应与代码示例

### 3.1 cURL（官方 Quickstart 原文）

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

出处：<https://docs.typesafe.ai/introduction/quickstart>

### 3.2 三型问题合一的请求体（官方 Quickstart 原文）

```json
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
      "criteria": [
        "Calm, just stating facts",
        "Frustrated but civil",
        "Very angry, strong language"
      ]
    },
    "is_urgent": {
      "type": "noul",
      "instructions": "The message conveys urgency or time-sensitivity"
    }
  }
}
```

对应响应（官方 Quickstart 原文，**数值是示例值，不是可复现结果**）：

```json
{
  "model": "jev-1.13.0",
  "answers": {
    "department": {
      "type": "choice",
      "choice": "technical",
      "confidence": 0.78,
      "probabilities": {
        "technical": 0.85,
        "sales": 0.0,
        "billing": 0.15
      }
    },
    "frustration": {
      "type": "score",
      "score": 1.0,
      "confidence": 1.0,
      "legend": {
        "0": "Calm, just stating facts",
        "1": "Frustrated but civil",
        "2": "Very angry, strong language"
      },
      "probabilities": { "0": 0.0, "1": 1.0, "2": 0.0 }
    },
    "is_urgent": { "type": "noul", "noul": 1.0 }
  },
  "usage": { "input_tokens": 392, "output_tokens": 65 }
}
```

出处：<https://docs.typesafe.ai/introduction/quickstart>

### 3.3 API reference 的单问题示例（含 error 与 Noul `criteria` 用法）

请求：

```json
{
  "state": "Help! My payouts have been failing for 3 days.",
  "model": "jev-latest",
  "questions": {
    "is_urgent": {
      "type": "noul",
      "instructions": "Does this convey urgency?",
      "criteria": {
        "true": "Explicitly time-sensitive",
        "false": "No urgency expressed"
      }
    }
  }
}
```

响应：

```json
{
  "model": "jev-1.13.0",
  "answers": {
    "is_urgent": { "type": "noul", "noul": 0.95 }
  },
  "usage": { "input_tokens": 296, "output_tokens": 20 }
}
```

Choice 答案示例（注意 `probabilities` 是 map，`confidence` 是另一个字段）：

```json
{
  "model": "jev-1.13.0",
  "answers": {
    "department": {
      "type": "choice",
      "choice": "billing",
      "probabilities": { "billing": 0.88, "technical": 0.12, "sales": 0.0 },
      "confidence": 0.81
    }
  },
  "usage": { "input_tokens": 318, "output_tokens": 34 }
}
```

Score 答案示例（`score` 落在两级之间）：

```json
{
  "model": "jev-1.13.0",
  "answers": {
    "frustration": {
      "type": "score",
      "score": 1.05,
      "legend": { "0": "Calm", "1": "Frustrated", "2": "Very angry" },
      "probabilities": { "0": 0.0, "1": 0.95, "2": 0.05 },
      "confidence": 0.92
    }
  },
  "usage": { "input_tokens": 304, "output_tokens": 18 }
}
```

出处：<https://docs.typesafe.ai/api.md>

### 3.4 Python SDK（官方 Quickstart 原文）

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

出处：<https://docs.typesafe.ai/introduction/quickstart>

SDK 行为要点（`system_one` 签名与语义）：

- 形态：`system_one(state, questions, *, model=None, retry=None, timeout=None, extra_headers=None, extra_body=None, response_model=None)`。`state` 为 `JSONContent`（字符串 / 对象 / 数组）；`questions` 为 `Mapping[str, Question]`，**非空**，可传 `Choice/Noul/Score` 对象或原始字典；`model` 为空时继承客户端默认值。
- `extra_body`：**浅合并**进顶层请求体，且「最后写入优先」——与 `state` / `model` / `questions` 同名的 key 会覆盖它们，对象值不会被深合并。这解释了为什么「额外字段」在 SDK 层可行，但服务端是否接受未文档化字段未做承诺。
- 便捷访问器：`response.answers[key]`，或按类型分组的 `response.nouls[key]` / `.choices[key]` / `.scores[key]`；`response.model` 与 `response.usage` 为必填字段。
- 客户端可读 `response.request_id`（即 `x-typesafe-request-id` 响应头）与 `raw_http_response`。
- 错误类型：`TypeSafeError`（基类）/ `TypeSafeAPIError`（含 `status`、`body`、`headers`、`endpoint`、`request_id`）/ `TypeSafeRateLimitError`（含 `retry_after_ms`）/ `TypeSafeAPIConnectionError` / `TypeSafeAPITimeoutError` / `TypeSafeAPIResponseValidationError`（含 `field_path`，如 `answers.tone.confidence`）。

出处：<https://docs.typesafe.ai/sdk/python/api/clients/sync.md>、<https://docs.typesafe.ai/sdk/python/api/types/responses.md>、<https://docs.typesafe.ai/sdk/python/api/exceptions.md>

### 3.5 JS/TS SDK（第三方示例，字段与官方一致）

```ts
import { choice, noul, TypeSafeClient } from "@typesafe-ai/sdk";

const client = new TypeSafeClient();

const response = await client.systemOne({
  state: { document: "I was charged twice. Please fix this ASAP." },
  questions: {
    category: choice("What is this ticket about?", {
      billing: "Payment or subscription issues",
      technical: "Bugs or integration problems",
      other: "Anything else",
    }),
    urgent: noul("The message conveys urgency"),
  },
});

console.log(response.answers.category.choice);
```

出处（第三方实践指南）：<https://dev.to/valyuai/how-to-use-jev-a-practical-guide-to-typesafes-system-one-model-g5e>；JS SDK 的方法签名 `systemOne<Q>(request, options?): APIPromise<SystemOneResult<Q>>` 见官方 <https://docs.typesafe.ai/sdk/javascript/api/classes/TypeSafeClient.md>

## 四、不确定与未证实之处

1. **错误响应体的 JSON 结构没有官方契约。** API reference 只说 422 的 body「details the offending field」。Python SDK 源码的 `extract_message()` 是**容错解析**，依次尝试 `error`（字符串或含 `message` 的对象）→ `message` → `detail`（字符串 / 含 `message` 的对象 / FastAPI 风格的 `[{loc, msg}]` 列表）。这**暗示**服务端可能返回 FastAPI 风格的校验错误，但官方未承诺，不应据此写死解析逻辑。出处：<https://raw.githubusercontent.com/typesafe-ai/typesafe-sdk-python/main/src/typesafe_sdk/_core/errors.py>
2. **`529 Overloaded` 未列入 SDK 的状态码映射表**（表中只有 400/401/403/404/422/429），它按 `status >= 500` 归入 `TypeSafeInternalServerError`；因重试集合包含 500–599，实际仍会被重试。这是源码推断，非文档声明。出处同上。
3. **限流数值不稳定**：官方 Warning 明确说会随容量变化而无预警调整。记录任何基于 250k tok/s、1,200 rpm 的容量假设时，必须标注日期与「可能变动」。出处：<https://docs.typesafe.ai/models.md>
4. **`429` 是否总带 `retry-after` 不确定**：文档措辞是「honor the `retry-after` header when the response carries one」，即并非保证存在；`retry-after-ms` 更是只在 SDK 源码中出现，文档未提。出处：<https://docs.typesafe.ai/models.md>、SDK constants 源码。
5. **confidence 的具体公式未公开**：官方只给「分布集中度」的定性描述和一个**演示用**近似式；v1 相对 preview 换了算法但未公布新公式。任何把 confidence 当阈值用的设计都应在自有数据上校准。出处：<https://docs.typesafe.ai/confidence.md>、<https://docs.typesafe.ai/migrating-to-v1.md>
6. **`probabilities` 是否严格和为 1**：API reference 写 "floats that sum to 1"，SDK 的 JSON schema 写 "sum to approximately 1"。设计上应按「近似」处理（例如 255 选项时的浮点误差）。
7. **单请求问题数上限未公开**；只有 64k token 总预算与「每个问题消耗少量 token」的定性说明。
8. **流式支持：仅证明「未找到」**，官方既未承诺也未明确否认；`coding-agents` 页的表述是「Jev 不生成文本、不做 stream」这一模型能力层面的说法，不等于 API 永远不会有流式。
9. **示例中的数值不可复现**：Quickstart 与 API reference 的 `confidence` / `probabilities` / `noul` 取值均为**示意值**（例如同一句 urgency 问题在不同页面给出 0.95、1.0，LangChain 博文给出 0.999）。这些是不同页面各自的示例，不代表官方矛盾，但**不能当作基准数据**。出处：<https://docs.typesafe.ai/introduction/quickstart>、<https://docs.typesafe.ai/api.md>、<https://www.langchain.com/blog/building-a-harness-with-jev>
10. **第三方性能数字均为厂商自测、未独立复现**：dev.to 指南明确写道这些是「TypeSafe's own numbers, self-run and unreproduced」，并引用官方自述「0% structured-output errors 是断言而非实测」。本报告不把 70–500ms 延迟、200x/400x 之类数字当作事实。出处：<https://dev.to/valyuai/how-to-use-jev-a-practical-guide-to-typesafes-system-one-model-g5e>
11. **未成功获取的来源**：DataCamp 文章 <https://www.datacamp.com/blog/system-one-models-jev> 返回 HTTP 403，未纳入证据；本次仅使用了官方文档、官方 SDK 源码/PyPI/npm 元数据、LiteLLM、LangChain 博文、dev.to 指南。
12. **版本核对（2026-09-22）**：PyPI `typesafe-sdk` 最新版 **0.7.1**（要求 Python ≥3.10）；npm `@typesafe-ai/sdk` 最新版 **0.6.0**。官方文档未声明 v1 API 的版本化承诺（如废弃策略或兼容窗口）。来源：<https://pypi.org/pypi/typesafe-sdk/json>、<https://registry.npmjs.org/@typesafe-ai/sdk>、仓库 <https://github.com/typesafe-ai/typesafe-sdk-python>、<https://github.com/typesafe-ai/typesafe-sdk-js>

## 五、对本项目 Jev 定位的启示（1–3 条）

1. **Rust 客户端可以直接对 HTTP 契约建模，不必先有官方 SDK。** 契约面很窄：一个 `POST /v1/systemone` + 一个 `GET /v1/models`，请求/响应都是可穷举的 tagged union（`type` 兼容判别，`questions`/`answers` 是 map）。建议在 Rust 侧定义 `enum Question { Noul{..}, Choice{..}, Score{..} }` 与 `enum Answer { Noul{noul}, Choice{choice, probabilities, confidence}, Score{score, legend, probabilities, confidence} }`，用 `serde` 的 internally-tagged 表示；**注意 Score 的 `legend`/`probabilities` 在 wire 上是「字符串级号」键，而 SDK 侧转成整数键**，Rust 侧要显式选定一种表示并做转换。
2. **把「概率分布 + confidence」原样带进 Agent 循环，而不是在客户端塌缩成枚举/布尔。** 契约已经把完整分布给出来了（Noul 的标量本身就是概率；Choice/Score 有 `probabilities` 与 `confidence`），且官方明确 confidence 定义可替换。建议 harness 内的判定结果保留 `noul: f32` / `probabilities: BTreeMap<String, f32>` / `confidence: f32`，把阈值与「哪个动作用哪个阈值」放在确定性代码层，这样也天然满足 AGENTS.md 的「概率优先」原则。
3. **错误与限流语义需要在 harness 内显式建模，且不要把流式当默认假设。** `429 / 529` 必须走指数退避并尊重 `retry-after`（SDK 默认 `max_retries=2`、500ms 起、上限 5s、约 25% 抖动、单次操作 10s / 重试总预算 30s 是可直接照搬的保守基线）；同时把 `422` 当作「请求构造 bug」而不是可重试错误，把 `x-typesafe-request-id` 落进日志便于追查。由于**未发现流式接口**，harness 的工具/判定层应按「一次请求-响应、整包返回」设计超时与取消（例如依赖 `timeout` + abort），不要为增量 token 预留抽象；若未来出现流式，再在传输层扩展。
