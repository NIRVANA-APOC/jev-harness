# 专题 11：Rust 生态集成视角

> 调研日期：2026-09-22
> 调研范围：是否存在官方/社区 Rust SDK；HTTP API 是否支持流式；从 Rust 调用需注意什么；对本项目 Rust 抽象层的启示。
> 证据分级：**【官方】** = TypeSafe AI 官方文档/博客/官方仓库；**【第三方】** = 社区 crate、第三方集成文档；**【自测】** = 本次调研中我在本机编译运行验证过的结论。

---

## 一、结论要点

1. **TypeSafe 官方没有 Rust SDK，也没有任何 Rust 计划。** 官方只提供 Python 与 JavaScript/TypeScript 两套 SDK，官方口径是「其它语言直接调 HTTP API」。我下载了官方文档全量语料（903,311 字节）全文检索，"Rust" 一词零命中（仅有 `frustration` 这类子串误匹配）；官方文档站 `/sdk/rust/` 返回 HTTP 404。

2. **社区 Rust 生态异常繁荣但极度碎片化。** crates.io 上关键词 `jev` 有 **29 个** crate、`system-one` 有 **13 个**，其中至少 **18 个**是各自独立的「Jev/System One 客户端」。全部自我标注 unofficial，全部诞生于 2026-09-16 至 09-21 的六天内，全部是 `0.x`，下载量 9–329，GitHub star 数 0–11。**没有任何一个可以称为「事实标准」。**

3. **HTTP API 不支持流式，且这是设计使然。** 官方文档全量语料中没有任何 SSE / stream / server-sent 的响应接口描述。官方「Jev with coding agents」页原文直接说明：*"Coding agents rely on an LLM that streams text, calls tools, and edits files based on natural-language instructions. Jev does none of that."* 这符合 System One 的并行采样架构（一次前向出全部概率，而非逐 token 自回归）。

4. **从 Rust 调用是可行的，官方 API 形状对 serde 非常友好。** `Question` 由 `type` 字段区分三型，正好映射到 Rust 的**内部标签枚举**（`#[serde(tag = "type")]`）。我已实际编译验证该映射能无损往返官方示例 JSON。【自测】

5. **真正的「并发」不是并发发请求，而是把问题打包进单请求。** 官方明确推荐一次请求问多个问题（问题并行求解、互不干扰），并给出实测：13 个问题合并为 1 次调用 = **12.2x 更便宜、10.0x 更快**，且答案与分开发送完全一致。限流是 **1,200 requests/min + 250,000 tokens/s**——多打请求会先撞 request 限流，打包提问才是正解。

---

## 二、关键事实与数据

### 2.1 官方 SDK 覆盖面：只有 Python 与 JS/TS

| 证据 | 内容 | 来源 |
| --- | --- | --- |
| 官方 SDK 索引页 | 只列出两张卡片：Python、JavaScript / TypeScript。原文：*"You can also call the **HTTP API** directly from any language."* | 【官方】[docs.typesafe.ai/sdk](https://docs.typesafe.ai/sdk) |
| 官方文档站 sitemap | `/sdk/python`、`/sdk/javascript` 及其完整 API 子页；**无任何 `/sdk/rust` 条目** | 【官方】[docs.typesafe.ai/sitemap.xml](https://docs.typesafe.ai/sitemap.xml) |
| 反向确认 | `https://docs.typesafe.ai/sdk/rust/` → **HTTP 404 Not Found** | 【自测】本次调研 curl 实测 |
| 官方 GitHub 组织 `typesafe-ai` 的仓库列表 | `typesafe-sdk-python`(Python, 186★)、`typesafe-sdk-js`(TypeScript, 214★)、`system-one-adapter-python`(Python, 241★)、`skills`(1575★)、`typesafe-ai.github.io`、`vllm`、`LLaDA`、`daggerverse`、`pulumi-clickhouse`、`Overwatch` — **无 Rust 仓库** | 【官方】[github.com/orgs/typesafe-ai/repos](https://api.github.com/orgs/typesafe-ai/repos) |
| 官方仓库身份确认 | PyPI `typesafe-sdk` 0.7.1 的 `Repository` 字段指向 `github.com/typesafe-ai/typesafe-sdk-python`，`Homepage` 指向 `typesafe.ai`，文档指向 `docs.typesafe.ai/sdk/python/` | 【官方】[pypi.org/pypi/typesafe-sdk](https://pypi.org/pypi/typesafe-sdk/json) |
| 官方全量语料检索 | `llms-full.txt` 共 903,311 字节，`grep -in "rust"` 无真实命中（仅 `frustration` / `Frustrated` 子串） | 【自测】[docs.typesafe.ai/llms-full.txt](https://docs.typesafe.ai/llms-full.txt) |

> **结论**：在 Rust 项目中接入 Jev，**没有官方支持的路径**，只能（a）自己调 HTTP API，或（b）选用第三方社区 crate。这本身是一个需要向项目决策者明示的风险点。

### 2.2 社区 Rust crate 生态（crates.io 实测枚举）

检索方式：crates.io API 按关键词与名称枚举。【第三方】

`keyword=jev` 共 **29 个** crate，`keyword=system-one` 共 **13 个**。下表列出其中定位为「Jev / System One 客户端或 SDK」的主体（已剔除纯应用类工具）:

| crate | 版本 | 下载 | 最后更新 | 声明仓库 | 仓库状态 |
| --- | --- | --- | --- | --- | --- |
| `typesafe-sdk-client` | 0.6.2 | 174 | 2026-09-19 | `douglance/typesafe-sdk-rs` | 1★, MIT |
| `typesafeai-sdk` | 0.4.1 | 164 | 2026-09-21 | `Ketankhunti/typesafe-sdk-rust` | 1★, MIT |
| `typesafeai-sdk-community` | 0.5.0 | 71 | 2026-09-19 | `community-ports/typesafeai-sdk-rust-community` | 0★, MIT |
| `kunobi-jev` | 0.2.0 | 214 | 2026-09-18 | `kunobi-ninja/kunobi-jev` | 0★, Apache-2.0 |
| `jevi` | 0.4.0 | 329 | 2026-09-21 | `bitomule/jevi` | 1★, Apache-2.0 |
| `jev-sdk` | 0.1.0 | 29 | 2026-09-18 | `portlandhodl/jev-sdk` | **HTTP 404，仓库不存在** |
| `jevrs` | 0.1.1 | 29 | 2026-09-20 | `luizribeiro/jevrs` | 0★, Apache-2.0 |
| `sysone` | 0.0.3 | 35 | 2026-09-19 | `zcoder-run/rust-sysone` | 4★, Apache-2.0 |
| `system-one` | 0.1.0 | 9 | 2026-09-21 | `heyaozh/system-one` | 0★, NOASSERTION |
| `typesafe-jev` | 0.2.0 | 22 | 2026-09-21 | `thehumanworks/jevgrep` | 1★, 无 License |
| `typesafe-client` | 0.1.0 | 43 | 2026-09-16 | `JedimEmO/typesafe-client` | 1★, Apache-2.0 |
| `typesafe-ai-sdk` | 0.2.0 | 64 | 2026-09-18 | `aoprisan/typesafe-ai-rust-sdk` | 1★, MIT |
| `typesafe-ai-rs` | 0.1.0 | 41 | 2026-09-17 | `gilljon/typesafe-ai-rs` | 4★, MIT |
| `typesafe-rs` | 0.1.0 | 17 | 2026-09-16 | `AbdelStark/typesafe-rs` | 1★, MIT |
| `typesafe-rust-sdk` | 0.1.0 | 13 | 2026-09-19 | `phiat/typesafe-rust-sdk` | 1★, MIT |
| `typesafe-systemone` | 0.2.0 | 38 | 2026-09-21 | `2commits/typesafe-systemone` | 0★, Apache-2.0 |
| `jev-client` | 0.1.1 | 33 | 2026-09-20 | `shaharia-lab/jev-cli` | 11★, Apache-2.0 |
| `jav-rs-sdk` | 0.1.1 | 20 | 2026-09-21 | `wjixiang/jav-rs-sdk` | — |
| `typesafe-jev-mcp` | 0.1.2 | 35 | 2026-09-20 | — | MCP server，非 SDK |

数据来源：【第三方】[crates.io API](https://crates.io/api/v1/crates?keyword=jev) + [GitHub API](https://api.github.com) 逐个核验。

**需要注意的生态健康度问题**：

- 全部 crate 的 GitHub 仓库创建时间都在 **2026-09-16 ~ 2026-09-21** 之间，即 Jev 发布后的一周内；star 数 0–11，无任何一个形成社区共识。
- `jev-sdk` 的 crates.io 元数据声明的仓库 `github.com/portlandhodl/jev-sdk` **返回 404**（仓库不存在或不可见）。该 crate 的 docs.rs 文档质量却相当高（含安全加固说明，见下），说明「文档完善 ≠ 元数据可信」。
- `system-one` 的 License 字段为 `NOASSERTION`，`typesafe-jev` 无 License——企业项目引入前必须自行确认许可。
- **⚠️ 命名冲突**：crates.io 上已存在名为 **`jev-harness`** 的 crate（0.1.4，2026-09-21，描述 *"Zero-overhead System One decision harness and token optimizer for AI coding agents."*），与本项目同名。若本项目日后有意发布 crate，需先处理命名。来源：【第三方】[crates.io](https://crates.io/api/v1/crates?keyword=jev)

**社区 crate 中值得参考的设计（不代表可用性背书）**：

- `system-one`（backend-agnostic 设计）：把「System One 式模型」抽象成后端无关接口，提供 `backend::JevHttp` / `Mock` / `Replay` / `LocalLogprob` / `Shadow` 多种后端；并用 `#[derive(AsChoice)]`（枚举 → choice 选项）与 `#[derive(AsQuestions)]`（结构体 → 整组问题）生成 schema 并回解析答案；自带 `calibration::CalibrationReport` 检查「标称 0.8 是否真的意味着 80%」。来源：【第三方】[docs.rs/system-one](https://docs.rs/system-one/latest/system_one/)
- `typesafeai-sdk-community`：明确说明自己是官方 Python SDK 的移植（*"a port of the official Python SDK: same primitives, same request/response shapes, same retry and error semantics"*），提供 `#[derive(ChoiceLabels)]` / `#[derive(ScoreLevels)]` / `#[derive(Questions)]` / `#[derive(Route)]` / `#[derive(Composite)]`，feature flag 区分 `rustls`（默认）/ `native-tls` / `blocking` / `test-util`（含 mock transport 与 record/replay）。来源：【第三方】[docs.rs/typesafeai-sdk-community](https://docs.rs/typesafeai-sdk-community/latest/typesafeai_sdk_community/)
- `jev-sdk`（仓库虽 404，但其 README 的安全清单有参考价值）：API key 存为敏感 `HeaderValue` 并从 `Debug` 输出中脱敏、拒绝含 CRLF 的 key、强制 HTTPS（仅回环允许明文）、内建客户端不跟随重定向（防 `Authorization` 头被转发到意外主机）、响应体大小上限（成功 16 MiB / 错误 64 KiB）、重试退避饱和且 jitter、`Retry-After` 上限 60s、依赖最小化（reqwest/serde/serde_json/indexmap/thiserror/tokio）。来源：【第三方】[docs.rs/jev-sdk](https://docs.rs/jev-sdk/latest/jev_sdk/)

### 2.3 官方 HTTP API 形状（Rust 直接调用所需）

以下全部来自官方 API 参考原文。【官方】[docs.typesafe.ai/api](https://docs.typesafe.ai/api)

**端点与鉴权**

```http
POST https://api.typesafe.ai/v1/systemone
Authorization: Bearer <API_KEY>
Content-Type: application/json
```

另有 `GET /v1/models` 返回账号可用的模型名（含描述与发布日期）。来源：【官方】[docs.typesafe.ai/models](https://docs.typesafe.ai/models)

**请求体**

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `state` | `string \| object \| array` | 是 | 被评估的内容。纯文本用字符串；聊天记录、数据库记录、应用状态用结构化对象/数组 |
| `model` | `string` | 是 | 官方旗舰别名 `"jev-latest"` |
| `questions` | `map<string, Question>` | 是 | 键由调用方自取；答案以同样的键返回。**键不会送给模型、不参与推理** |

**问题三型（`Question` 由 `type` 字段区分）**

| 类型 | 附加字段 | 约束 |
| --- | --- | --- |
| `noul` | `criteria`（可选）：`{ "true": …, "false": … }` | 是非判断 |
| `choice` | `criteria`（必填）：`map<选项, 描述 \| null>` | **最多 255 个选项**；不需要描述时用 `null` |
| `score` | `criteria`（必填）：有序数组 | **至少 2 级，最多 10 级** |

三型的 `instructions` 都接受 `string | object | array`。结构化写法可把「问题」与「它引用的数据」放在同一对象的不同字段里，并在问题中用反引号引用字段名：

```json
"instructions": {
  "potential_duplicate": { "name": "John Smith", "location": "Oakland, California" },
  "question": "Is the resume for the same person as `potential_duplicate`?"
}
```

**响应体**

```json
{
  "model": "jev-1.13.0",
  "answers": {
    "department": {
      "type": "choice",
      "choice": "billing",
      "probabilities": { "billing": 0.88, "technical": 0.12, "sales": 0.0 },
      "confidence": 0.81
    },
    "is_urgent": { "type": "noul", "noul": 0.95 },
    "frustration": {
      "type": "score",
      "score": 1.5,
      "legend": { "0": "Calm", "1": "Frustrated", "2": "Very angry" },
      "probabilities": { "0": 0.1, "1": 0.3, "2": 0.6 },
      "confidence": 0.72
    }
  },
  "usage": { "input_tokens": 318, "output_tokens": 34 }
}
```

> 示例中的 `score`/`legend`/`probabilities` 组合按官方字段定义构造；官方原文给出的 noul 与 choice 示例为逐字引用，score 的完整数值样例未在官方文档中给出完整一份，此处按官方 Score answer 字段说明（`score` 为概率加权值、`legend` 为层级序号→描述、`probabilities` 为层级序号→概率）合成，**已用于本次 serde 往返自测**。

**答复类型的字段差异（关键）**

| 答复 | 字段 |
| --- | --- |
| noul | `type`、`noul`（0–1） |
| choice | `type`、`choice`、`probabilities`（和为 1）、`confidence` |
| score | `type`、`score`（可为小数，落在层级之间）、`legend`、`probabilities`、`confidence` |

**⚠️ `noul` 答复没有 `confidence` 字段。** Choice 与 Score 才有 `confidence`。这是三型之间的重要不对称，Rust 类型设计应当如实表达（见第四节）。

**错误码**

| 状态码 | 含义 |
| --- | --- |
| `401 Unauthorized` | API key 缺失或无效，检查 `Authorization` 头 |
| `422 Unprocessable Entity` | 请求体校验失败（如缺必填字段、问题格式错误），响应体指出出错字段 |
| `429 Too Many Requests` | 超出限流，退避后重试 |
| `529 Overloaded` | 服务暂时过载，延后重试 |

**模型与配额（Rust 侧超时/限流预算的依据）**

| 项 | 值 | 来源 |
| --- | --- | --- |
| 模型 ID | `jev-1.13.0`（别名 `jev-latest`、`jev-preview`，当前二者同指 1.13.0） | 【官方】[docs.typesafe.ai/models](https://docs.typesafe.ai/models) |
| 价格 | 输入 $42 / Btok = **$0.042 / Mtok**；**输出 token 免费** | 同上 |
| 限流 | **250,000 tokens/秒** 与 **1,200 requests/分钟**，任一超限返回 `429` | 同上 |
| 限流稳定性 | 官方警告：*"Rate limits are adjusting dynamically… the limits above can change without notice"* | 同上 |
| 上下文 | 每请求 64k tokens；其中 `state` + 单个最长问题 ≤ 32k tokens | 同上 |
| 输入模态 | 仅文本（字符串 / JSON 对象 / 文本数组），无图像、音频、视频 | 同上 |
| 别名风险 | 官方建议：若已按某版本调好 confidence 阈值，**请固定版本 ID 而非用别名**，因为别名会静默迁移 | 同上 |

### 2.4 流式支持：不支持（设计使然）

- **官方文档全量语料（903,311 字节）全文检索 `stream` / `SSE` / `server-sent`：无任何响应流式接口的描述。** 唯一命中的三处是无关语境（HuggingFace 数据集 `streaming=True` 的 Python 代码、以及下述设计声明）。【自测 + 官方】
- **官方明确声明**（「Jev with coding agents」页，原文）：*"Coding agents rely on an LLM that streams text, calls tools, and edits files based on natural-language instructions. **Jev does none of that.**"* 并进一步说明 *"There is no `model: "jev-latest"` setting that turns your coding agent into a Jev-powered agent."*【官方】[docs.typesafe.ai/introduction/coding-agents](https://docs.typesafe.ai/introduction/coding-agents)
- 架构上也自洽：官方博客说明 Jev 的采样是 **Parallel（"Generates all outputs in a single query"）**，而非逐 token 自回归，所以没有 token 流可流。官方给出端到端响应时间 **70ms–500ms**。【官方】[typesafe.ai/blog/introducing-system-one-models-and-jev](https://typesafe.ai/blog/introducing-system-one-models-and-jev)
- 第三方集成侧一致：LiteLLM 的 TypeSafe 适配页说明 Jev *"is called through its own evaluate endpoint rather than `/chat/completions`"*，属于 pass-through 转发，未涉及流式。【第三方】[docs.litellm.ai/docs/pass_through/typesafe](https://docs.litellm.ai/docs/pass_through/typesafe)

> 交叉验证结论：**两个独立来源（官方设计声明 + 第三方便利层集成方式）一致，且官方语料无相反证据 → HTTP API 不支持流式响应。**

### 2.5 超时与重试：官方 SDK 的默认参数（可直接作为 Rust 实现的对齐目标）

**JS/TS 官方 SDK 的 `RetryPolicy` 默认值**（最完整的一份官方默认值披露）：【官方】[docs.typesafe.ai/sdk/javascript/api/interfaces/RetryPolicy](https://docs.typesafe.ai/sdk/javascript/api/interfaces/RetryPolicy)

| 字段 | 默认值 |
| --- | --- |
| `maxRetries` | 2（在首次尝试之外） |
| `backoffInitialMs` | 500 |
| `backoffMaxMs` | 5000 |
| `backoffJitter` | 0.25（每次退避随机减去的比例） |
| `httpStatuses` | 408、429、500–599 |
| `respectRetryAfter` | true（遵守 `Retry-After` / `retry-after-ms`） |
| `maxRetryAfterMs` | 60000（服务端要求更长则改用退避） |
| `apiConnectionError` | true |
| `apiTimeoutError` | true |

**Python 官方 SDK**：【官方】[docs.typesafe.ai/sdk/python/api/constants](https://docs.typesafe.ai/sdk/python/api/constants) / [retries](https://docs.typesafe.ai/sdk/python/api/retries)

- `DEFAULT_TIMEOUT = 10.0`（每次 HTTP 操作的默认超时，单位秒）
- `DEFAULT_MODEL = 'jev-latest'`
- 环境变量：`TYPESAFE_API_KEY`、`TYPESAFE_BASE_URL`、`TYPESAFE_DEFAULT_MODEL`、`TYPESAFE_LOG_LEVEL`
- 文档中的配置示例为 `RetryPolicy(max_retries=3, timeout=10.0, http_statuses={429, 500, 502, 503, 504})`

> **注意一处官方 SDK 之间的不一致**：Python 文档示例写 `max_retries=3`，JS 文档写明默认 `maxRetries: 2`。该示例是「配置范例」而非「默认值声明」，故不构成直接矛盾，但**两套官方 SDK 的重试次数并不一致**，本项目应自行定档（建议对齐 JS 的 2 次，或按自身 SLA 决定）。另外 Python 示例的重试状态码集合 `{429,500,502,503,504}` 与 JS 的 `408,429,500–599` 也不一致。
>
> 官方对 `Retry-After` 的措辞：*"Our client SDKs retry with backoff by default and honor the `retry-after` header when the response carries one. If you call the HTTP API directly, see Handling rate limits."*【官方】[docs.typesafe.ai/models](https://docs.typesafe.ai/models)

### 2.6 并发与批量提问：官方主张「打包，而非并发」

**官方 cookbook 实测**（这是本专题最重要的一条性能证据）：【官方】[docs.typesafe.ai/cookbooks/parallel_questions](https://docs.typesafe.ai/cookbooks/parallel_questions)

- **实测条件**：文档为 GDPR 的 Wikipedia 条目（约 54,000 字符，属「文档主导型」负载）；13 个问题 = 8 个 Noul + 2 个 Choice + 3 个 Score；每种策略重复 5 次比较运行间标准差；使用官方 Python SDK（`typesafe-sdk>=0.5.7`）。

| 策略 | 调用数 | 成本 | 总耗时 |
| --- | --- | --- | --- |
| 一次调用，全部 13 问 | 1 | $0.000497 | 0.27s |
| 13 次调用，每问一次 | 13 | $0.006090 | 2.71s |
| — | — | **12.2x 更便宜** | **10.0x 更快** |

- **答案一致性**：官方称「大多数答案在 5 次重复中完全一致，运行间标准差正好为 0.0」，即**打包不改变答案质量**（每个问题独立对着文档评分，互不影响）。
- **成本机制**：文档在每个请求中占主体，N 次单问请求要重复支付 N 次文档成本、N 次往返；打包只付一次。文档越大，节省越接近 N 倍。

**⚠️ 官方内部数字不一致**：官方「Speculative fan-out」模式页对（推测）同一实验给出的是 *"batching 13 questions into one call is **11.5x cheaper and 9.6x faster** than 13 separate calls"*，与 cookbook 页的 12.2x / 10.0x 不同。【官方】[docs.typesafe.ai/patterns/fan-out](https://docs.typesafe.ai/patterns/fan-out)
> 两处均为官方口径但数字不符，本项目引用时应说明「官方两处口径为 11.5x–12.2x 更便宜、9.6x–10.0x 更快」，或直接引用 cookbook 的原始表格（有完整测试条件）。

**官方并发相关的其他口径**：

- 官方在部分 cookbook 中使用 `ThreadPoolExecutor` 并发处理多文档（如 `WORKERS = 8`，注释为 *"small pool: enough to keep a live run to minutes, gentle on rate limits"*）。【官方】[docs.typesafe.ai/llms-full.txt](https://docs.typesafe.ai/llms-full.txt)
- 官方对限流的处理建议：*"When you receive a `429`… or `529`… retry the request with exponential backoff instead of retrying immediately."*【官方】[docs.typesafe.ai/api](https://docs.typesafe.ai/api)

---

## 三、代码示例

### 3.1 官方 Python 示例（作为 API 形状的对照基准）【官方】

来源：[docs.typesafe.ai/introduction/quickstart](https://docs.typesafe.ai/introduction/quickstart)

```python
from typesafe_sdk import Choice, Noul, Score, TypeSafeClient

client = TypeSafeClient()
response = client.system_one(
    state=ticket,
    questions={
        "department": Choice(
            instructions="Which team should handle this",
            criteria={"billing": "Payment or subscription issues",
                       "technical": "Bugs or integration problems",
                       "sales": "Pricing or account questions"},
        ),
        "frustration": Score(
            instructions="How frustrated the customer appears",
            criteria=["Calm, just stating facts", "Frustrated but civil", "Very angry, strong language"],
        ),
        "is_urgent": Noul(instructions="The message conveys urgency or time-sensitivity"),
    },
)
print(response.answers["department"].choice)
```

### 3.2 社区 Rust crate 的调用写法（引用 docs.rs 原文，非本项目代码）【第三方】

`typesafeai-sdk-community` 0.5.0 —— 风格最接近官方 Python SDK 的链式 API，来源：[docs.rs/typesafeai-sdk-community](https://docs.rs/typesafeai-sdk-community/latest/typesafeai_sdk_community/)

```rust
let response = client
    .system_one(json!({"document": "I was charged twice. Please help."}))
    .question("billing", Noul::new("Is this message about billing?"))
    .question("tone", Choice::new("What is the tone of this message?")
        .option("angry", "An upset or hostile message")
        .option("calm", "A neutral or polite message"))
    .question("urgency", Score::new("How urgent is this?", ["Can wait", "This week", "Today"]))
    .send()
    .await?;
```

`jev-sdk` 0.1.0（**其声明仓库 404，仅作写法参考**），来源：[docs.rs/jev-sdk](https://docs.rs/jev-sdk/latest/jev_sdk/)

```rust
let client = TypeSafeClient::from_env()?;
let response = client.system_one(ticket, [
    ("department", Question::from(Choice::new("Which team should handle this",
        [("billing", Some("Payment or subscription issues")), ("technical", None)]))),
    ("is_urgent", Question::from(Noul::new("The message conveys urgency or time-sensitivity"))),
]).await?;
let department = response.choice("department").unwrap();
println!("route to {} (confidence {:.2})", department.choice, department.confidence);
```

`system-one` 0.1.0 —— 用 derive 宏把 Rust 类型直接变成问题集，来源：[docs.rs/system-one](https://docs.rs/system-one/latest/system_one/)

```rust
#[derive(AsChoice)]
enum Department { Billing, Technical, Sales, Spam }

#[derive(AsQuestions)]
struct Triage {
    #[ask("Which department should handle this ticket?")]
    department: Choice<Department>,
    #[ask("Is the customer asking for a refund?")]
    wants_refund: Noul,
    #[ask("How urgent is this?", levels = ["Can wait", "Today", "Right now"])]
    urgency: Score,
}
let t: Triage = engine.ask("Hi, my payouts have failed for 3 days…").await?;
```

### 3.3 【自测】官方 JSON ↔ Rust serde 映射验证

我在本机（`cargo 1.97.1` / `rustc 1.97.1`）写了一个一次性 crate 并实际编译运行，验证官方 API 的 JSON 能否用 serde 精确映射。**代码位于 `/tmp/jev-research/serde-check/`，不在项目仓库内。**

**验证结果（`cargo run` 实际输出 `ALL SERDE CHECKS PASSED`）**：全部 8 项断言通过

| 验证项 | 结果 |
| --- | --- |
| `#[serde(tag = "type", rename_all = "snake_case")]` 映射三型问题 | 通过 |
| Choice 的 `criteria` 值为 `null`（官方允许） | 通过 |
| `instructions` 为结构化对象（官方 advanced 用法） | 通过 |
| 官方示例请求序列化往返（`type` 字段、`criteria` 形状一致） | 通过 |
| Noul 无 `criteria` 时不输出该字段（`skip_serializing_if`） | 通过 |
| 官方示例响应反序列化（`model` / `answers` / `usage`） | 通过 |
| 类型化 id 取答案（`Id<ChoiceKind>` / `Id<NoulKind>` / `Id<ScoreKind>`） | 通过 |
| 用错类型 id 取答案 → `None`（不 panic、不串数据） | 通过 |

**验证过程中实测出的 serde 行为（重要，非猜测）**：

```
实测：Noul 答案多出的 confidence 字段被 serde 默认静默忽略（未报错）
实测：未知 type 值导致该答案反序列化失败：unknown variant `ranking`, expected one of `noul`, `choice`, `score`
```

补充验证（`cargo run --bin strict`）：

```
(A) struct + deny_unknown_fields 拒绝未知字段: Some("unknown field `cached`, expected `input_tokens` or `output_tokens`")
(B) 内部标签枚举 + deny_unknown_fields 报错: unknown field `confidence`, expected `noul`
(C) #[serde(other)] 兜底未知 type: Unknown
```

**这三点对 Rust 抽象层有直接设计含义**：

1. **serde 默认静默丢弃未知字段**——如果服务端日后新增字段（例如给 noul 也加 confidence），默认配置下会**无声地丢失信息**。要严格对齐 wire 契约需显式 `#[serde(deny_unknown_fields)]`。
2. **`deny_unknown_fields` 在内部标签枚举上确实可用**（验证通过，B 项；它只与 `flatten` 不兼容）——因此可以既用内部标签枚举，又保持严格校验。
3. **未知 `type` 会让该条答案反序列化失败**，若 `answers` 是 `map<String, Answer>` 则会导致**整个响应解析失败**。前向兼容的解法是在枚举上保留一个 `#[serde(other)]` 的兜底单元变体（验证通过，C 项）——但若用了 `deny_unknown_fields` 与之组合需注意语义叠加。

---

## 四、对本项目 Rust 抽象层的启示

### 启示 1：Jev 抽象层是「自研资产」，且要按可替换来设计——但别急着发布

TypeSafe 官方没有 Rust SDK，且官方明确把非 Python/JS 语言的接入方式定为「直接调 HTTP API」。这意味着本项目的 Jev 客户端**不可能靠引入官方 crate 解决**，必须自研；同时也意味着**没有官方兼容性承诺兜底**，抽象层要假设 wire 契约会变。

设计建议：
- 把「模型调用抽象」做成 trait（对应项目总纲的「两类模型能力统一抽象」），让 Jev 与 LLM 走同一套调用与错误约定。社区 `system-one` crate 的 backend-agnostic 做法（同一 trait 下挂 `JevHttp` / `Mock` / `Replay` / `LocalLogprob`）值得借鉴——**尤其是 `Mock` 与 `Replay`**：没有官方 SDK 意味着没有官方 mock，测试用的假后端必须自己准备，这会直接决定 agent 循环的可测性。
- 严格区分「问题定义（编译期已知）」与「问题 id（运行期字符串）」。官方要求 `questions` 的键由调用方自取、且**键不参与推理**，所以键名是纯本地的路由信息——这给「用 Rust 类型而非字符串做键」留出了空间。

⚠️ **不要过早承诺把抽象层发布为 crate**：crates.io 上已有 29 个 `jev` 关键字 crate 与同名 `jev-harness`，碎片化严重且都未成熟。是否发布应由项目决策，不是调研结论。

### 启示 2：question 三型应映射为「内部标签枚举 + 类型化 id」，并如实表达三型的不对称

官方 wire 格式用 `type` 字段区分三型，正好对应 Rust 内部标签枚举。**已通过自测验证可行**（第一节 3.3）。建议的映射要点：

- `Question` 用 `#[serde(tag = "type", rename_all = "snake_case")]`，三变体携带各自 `criteria` 类型：`choice` 是 `map<选项, Option<Instructions>>`（**必须允许 `null`**——已自测）、`score` 是 `Vec<Instructions>`（**2–10 级**，可在构造期用类型或校验函数强制）、`noul` 是可选 `{true, false}`。
- `Instructions` 用 `#[serde(untagged)]` 表达官方的 `string | object | array`（已自测），这样「把问题与它引用的数据放进同一对象」的官方结构化写法可以直接用 Rust 结构体而非裸 `serde_json::Value` 表达。
- **把「问题类型」与「答案类型」在编译期绑定**：用标记类型参数化的 id（如 `Id<ChoiceKind>`），使 `Id<NoulKind>` 在类型上就取不到 choice 答案。这比社区 crate 普遍使用的 `response.choice("id").unwrap()` 更安全——后者在问题类型写错时只能运行期返回 `None`。这是本项目相对现有社区实现可以做的一个**真实改进点**。
- **不要用 `Option<f64>` 抹平三型差异**：官方 `noul` 答复**没有 `confidence`**，只有 choice / score 有。用枚举让「对 Noul 索要 confidence」变成编译错误，而不是运行期 `None`——这正是项目总纲「类型安全」与「概率优先（不要过早塌缩成布尔值）」的落地方式。
- **限制项应在类型或构造期强制**：Choice ≤ 255 选项、Score 2–10 级、`state` + 最长问题 ≤ 32k tokens。这些都是官方硬约束，越早失败越好，别等到 422。

### 启示 3：并发设计要「打包优先、限流集中、超时按快模型定档」

官方证据表明 Jev 的性能模型与 LLM 完全相反：**延迟低（70–500ms）、不支持流式、并且一次请求塞进多个问题几乎不增加延迟**（官方实测 13 问打包 = 12.2x 便宜 / 10.0x 快，答案不变）。所以：

- **不要让每个判断各发一次请求。** 项目总纲要求「决策分层，能让 Jev 判断的不要用 LLM」，但同样要注意「能让一次 Jev 请求解决的，不要拆成多次」。应在循环中做**问题批收集（batching）**：把同一 state 下的多个判定（路由、紧急度、门控）合并成一次调用。这既省钱（文档只付一次 input token）又省时（一次往返）。对应用官方 cookbook 的说法，甚至鼓励「投机性扇出」：把可能用到的判定一并问掉，由代码决定用哪个答案。
- **限流约束决定了「并发」的真正用途。** 官方限流是 **1,200 req/min 与 250,000 tokens/s**，任一超限即 429；且官方明确限流会**动态调整、可能不作通知地变化**。因此并发应集中在一个带全局预算的限流器后面，而不是每个调用点各自重试——否则多处退避会互相叠加，反而放大 429。（本项目若要多 agent 并行，这一点尤其关键。）真正的「多路并发」应留给**多个不同 state**（多候选重排、多文档分类）的场景，可参考官方 cookbook 中用 `ThreadPoolExecutor` + 小 worker 池（如 8）的做法。
- **超时按「快模型」定档，不要沿用 LLM 的分钟级预算。** 官方 Python SDK 的 `DEFAULT_TIMEOUT = 10.0` 秒，官方给出的典型端到端响应是 70–500ms。本项目的 Jev 调用超时应远短于 LLM 调用超时（这对 agent 循环的「卡住」检测很重要）——但要注意官方 cookbook 中出现过 `timeout=120.0` 的用法（大文档 / 大问题组合），说明**超时预算应与 `state` 规模挂钩**，单一常量不够。
- **重试策略可对齐官方 JS SDK 默认值**：`maxRetries=2`、初始退避 500ms、上限 5000ms、jitter 0.25、重试 408/429/5xx、遵守 `Retry-After`（上限 60s）。注意官方两套 SDK 的重试默认并不一致（Python 示例为 3），本项目需自行定档并写入文档。
- **UI / 事件流不能假设 token 流。** Jev 调用必须建模为「一次原子调用，返回完整结构」；如果 harness 的事件流是围绕 LLM 的增量 token 设计的，Jev 需要一个与之不同的、立即完成的事件分支，不能强行套用同一套流式接口——否则会为了统一而引入假的流式包装。

---

## 五、不确定与未证实之处

1. **未找到官方对 Rust 的任何表态（既无支持计划，也无明确「不做」的声明）。** 我只验证了「官方现在没有 Rust SDK」，**不能推断官方未来不会出**。官方 GitHub 组织下也无相关讨论（未逐 issue 检索）。

2. **没有 API key，因此本次调研未对线上 API 做任何真实调用。** 所有 HTTP 行为（限流触发、`429`/`529` 实际表现、`Retry-After` 头是否真的出现、实际延迟分布、`noul` 答复是否真的从不带 `confidence`）均**来自官方文档描述，未经我实测**。第 3.3 节的 serde 验证是对**官方文档中的 JSON 示例**做的往返，不是对线上响应的验证。

3. **官方单请求问题数上限未知。** 文档只给出单类约束（Choice ≤ 255 选项、Score 2–10 级），**未找到「一次请求最多多少个问题」的明确上限**。官方 cookbook 用了 13 个，另有 spec 提到 fan-out 可塞很多问题，但具体上限未公开。

4. **官方两处并行基准数字不一致**（12.2x/10.0x vs 11.5x/9.6x），且这两处是否有相同测试条件无法确认。引用时须同时注明两处。

5. **官方两套 SDK 的重试默认值不一致**（JS `maxRetries: 2` vs Python 示例 `max_retries=3`；重试状态码集合也不同）。Python 侧我未能找到「默认值」的显式声明（只有配置示例），因此**Python SDK 的真实默认重试次数仍未证实**。

6. **社区 crate 全部未经生产验证。** 无一有超过 329 的下载量、无一有超过 11 的 star、全部创建于 2026-09-16 之后。`jev-sdk` 更是出现「元数据声明的仓库 404」这种情况。**本报告列举社区 crate 仅作生态描述与设计参考，不构成任何可用性、安全性或维护性推荐。** 若项目考虑引入，需自行审计源码（尤其 API key 处理与 TLS 配置）。

7. **`jev-harness` 这个名字在 crates.io 上已被占用**（0.1.4，2026-09-21），未核实其作者身份与是否与本项目存在关联（例如是否为本项目此前尝试的发布）。这一点需要项目决策者确认，我不做推测。

8. **模型版本会漂移。** `jev-latest` / `jev-preview` 当前都指向 `jev-1.13.0`，官方明确建议固定版本 ID。本报告中的行为描述（字段形状、限制、价格、限流）**对应 2026-09-22 时点的官方文档**，未来可能变化。

---

## 附：本次调研的验证环境与可复现命令

```bash
# 官方文档全量语料（903,311 字节，本报告多次引用）
curl -sSL https://docs.typesafe.ai/llms-full.txt -o /tmp/jev-research/llms-full.txt
grep -in "stream\|rust\|SSE" /tmp/jev-research/llms-full.txt

# crates.io 生态枚举
curl -sS "https://crates.io/api/v1/crates?keyword=jev&per_page=60"
curl -sS "https://crates.io/api/v1/crates?keyword=system-one&per_page=60"

# serde 映射验证（8 项断言全部通过）
cd /tmp/jev-research/serde-check && cargo run
cd /tmp/jev-research/serde-check && cargo run --bin strict
```

环境：`cargo 1.97.1 (c980f4866 2026-06-30)`、`rustc 1.97.1 (8bab26f4b 2026-07-14)`；serde 1、serde_json 1。
