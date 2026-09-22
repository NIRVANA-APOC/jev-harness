# 专题 03：三类问题原语精读（Choice / Score / Noul）

- 调研日期：2026-09-22（所有页面抓取时间均为该日）
- 调研方式：`FetchURL` + `curl` 直接抓取原始 HTML（含页面内嵌的代码块/JSON 示例），辅以搜索引擎定位；**未只依赖搜索摘要**
- 主要证据源：官方文档站 `docs.typesafe.ai`（sitemap 已枚举全部页面）、官方博客 `typesafe.ai/blog`、以及 LangChain / Spring AI / MarkTechPost / dev.to / HexDocs(Elixir 客户端) / flaviocopes 等第三方
- 当前模型版本：`jev-1.13.0`（别名 `jev-latest`、`jev-preview`）；文档示例的响应体均标 `"model": "jev-1.13.0"`
- 证据等级标注：`【官方】`＝TypeSafe 自家文档/博客；`【第三方实测】`＝第三方跑过真实 API 得到的数字；`【第三方转述】`＝第三方复述官方口径，无独立测量

---

## 0. 结论要点（TL;DR）

1. **整个 API 面只有三种问题原语**，用 `type` 字段区分：`choice`、`score`、`noul`。三者共享 `type` + `instructions`，各自再带自己的 `criteria` 形态；一次请求可任意混用。【官方：docs.typesafe.ai/primitives、/api】
2. **请求 = `{state, model, questions}`，响应 = `{model, answers, usage}`**，`answers` 的 key 与请求里你自选的题目 id 一一对应；**题目 id 不发给模型**，所以语义必须写在 `instructions` 里。【官方：/primitives/choice】
3. **返回值形状严格固定**：
   - Choice → `choice`（概率最高的选项）+ `probabilities`（每个选项一个概率，和为 1）+ `confidence`（0–1）
   - Score → `score`（层级编号的概率加权平均，**可为小数**）+ `legend`（层级编号→描述）+ `probabilities`（按层级字符串 key）+ `confidence`
   - Noul → 单个 `noul`（0–1 的"是"的概率），**没有独立的 confidence**（概率本身就是不确定性）
   【官方：/primitives、/primitives/choice、/primitives/score、/primitives/noul、/api、/sdk/python/api/types/responses】
4. **硬性上限**：Choice **最多 255 个选项**；Score **最少 2 级、最多 10 级**；上下文 **64k tokens/请求**，其中 state + 最长单题 **32k**；速率 **250,000 tokens/秒** 与 **1,200 请求/分钟**；输入仅文本。【官方：/api、/models】
5. **"一次请求问 N 个问题"没有找到官方声明的题目数量上限**（官方文档明确写了选项数与层级数上限，却未写题目数上限）。第三方独立验证者亦明确记录"文档未声明单请求题目数上限"，并声明不猜测该值。【第三方：[ayautomate](https://www.ayautomate.com/blog/how-to-use-jev-guide)、[opentweet](https://opentweet.io/jev/limits)】
6. **计费只看输入 token**：$0.042 / 1M input tokens（= $42 / Btok），**输出 token 免费**（响应里仍会报 `output_tokens`）。state 每次请求只计一次，因此把 N 个问题合进一次请求，省的是 state 的重复计费。【官方：/models、/cookbooks/parallel_questions 中的 `PRICE = (0.042, 0.00)`】
7. **并行机制的实测（官方 cookbook）**：5.4 万字符文档 + 13 题（8 Noul / 2 Choice / 3 Score），一次批量调用 vs 13 次单题调用：**$0.000497 / 0.27s vs $0.006090 / 2.71s**，即 **12.2× 更便宜、10.0× 更快**；重复 5 次比较，批处理**没有改变答案、也没有引入额外方差**（std dev 在两种策略下同量级）。【官方：/cookbooks/parallel_questions】
8. **⚠️ 官方内部数字不一致**：同一份 cookbook，`/primitives` 总览页写"11.5x cheaper / 9.6x faster"，cookbook 页写"12.2x / 10.0x"。按 cookbook 自己给出的原始数字计算：0.006090/0.000497 ≈ 12.25×，2.71/0.27 ≈ 10.04×，**cookbook 页自洽，总览页数字是过时/笔误**。引用时请用 12.2× / 10.0× 并注明出处。
9. 本调研面对的**核心不确定性**：官方未公开 confidence 的确切统计量定义（只给了 3 选项演示公式 `(3 × 最大概率 − 1) / 2`），也未公开题目数上限、无最低计费口径、无"重试是否重复计费"的官方说明。

---

## 1. 三类原语总览

官方在 `/introduction` 与 `/primitives` 各给了一张总览表，两者对 Score 的返回字段列法略有差异（见第 10 节）。

| 类型 | 它在回答什么 | 请求字段 | 返回字段 | 来源 |
| --- | --- | --- | --- | --- |
| `choice` | 从给定选项里选一个（选项之间**无序**） | `type` + `instructions` + `criteria`（选项名→描述 **map**） | `choice` + `probabilities` + `confidence` | 【官方】[/primitives](https://docs.typesafe.ai/primitives)、[/primitives/choice](https://docs.typesafe.ai/primitives/choice)、[/api](https://docs.typesafe.ai/api) |
| `score` | 落在一条**有序**标尺的哪个位置 | `type` + `instructions` + `criteria`（层级描述**有序数组**） | `score` + `legend` + `probabilities` + `confidence` | 【官方】[/primitives/score](https://docs.typesafe.ai/primitives/score)、[/api](https://docs.typesafe.ai/api) |
| `noul` | 这个命题是否为真（是/否） | `type` + `instructions` + 可选 `criteria`（`true` / `false` 两侧描述） | `noul`（0–1） | 【官方】[/primitives/noul](https://docs.typesafe.ai/primitives/noul)、[/api](https://docs.typesafe.ai/api) |

第三方对同一张表的复述（用于交叉验证字段名与返回形状）：[LangChain 官方博客](https://www.langchain.com/blog/building-a-harness-with-jev)（`Choice`/`Score`/`Noul` 语义与"多问题单请求"）、[Spring AI 官方博客](https://spring.io/blog/2026/09/21/spring-ai-typesafe-structured-judgment)（含 Java SDK 的 `noulValue` / `choiceValue` / `scoreValue` 取数方法）、[MarkTechPost](https://www.marktechpost.com/2026/09/19/typesafe-ai-releases-jev/)（255 选项、$0.042/1M、并行隔离）、[dev.to 实操指南](https://dev.to/valyuai/how-to-use-jev-a-practical-guide-to-typesafes-system-one-model-g5e)（255 选项、2–10 级）。全部来源 URL 见文末《附录：引用清单》。

---

## 2. 请求的通用结构

```http
POST https://api.typesafe.ai/v1/systemone
Authorization: Bearer <API_KEY>
Content-Type: application/json
```
【官方：[/api](https://docs.typesafe.ai/api)】

顶层三个字段（均为必填）【官方：[/api](https://docs.typesafe.ai/api)、[/primitives](https://docs.typesafe.ai/primitives)】：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `state` | `string \| object \| array` | 被评估的内容。纯文本用字符串；结构化数据（会话、记录、策略、程序状态）用对象/数组。**仅文本**，不含图片/音频/视频，需自行预处理成文本 |
| `model` | `string` | `"jev-latest"`（SDK 默认，当前解析到 `jev-1.13.0`）或直接写版本号 |
| `questions` | `map<string, Question>` | key 由你自选；**该 key 不发给模型、不参与推理**，答案以同一 key 返回 |

`Question` 的公共字段：`type`（`"choice"` / `"score"` / `"noul"`）与 `instructions`。
`instructions` 与 `criteria` 的每一处描述都允许是 **`string | object | array | null`**（官方 Advanced 页称之为 `EntryType`）——即可以把问题与它引用的数据放在同一个 JSON 对象的不同 key 里，并在问题文本中用反引号 + 点号/下标路径引用 state 的字段（例如 `` "Does `policy` cover `order.charges`?" ``）。【官方：[/primitives/advanced](https://docs.typesafe.ai/primitives/advanced)、[/concepts/state](https://docs.typesafe.ai/concepts/state)】

**state 的形状限制（第三方实测记录）**：state 必须是字符串、对象、数组或 null；**裸数字或布尔值（包括 `@JsonValue` 序列化后变成标量的类型）会得到 422**。【第三方实测：Spring AI 博客，警告段】

---

## 3. Choice（从一组无序选项中择一）

### 3.1 字段【官方：[/api](https://docs.typesafe.ai/api)、[/primitives/choice](https://docs.typesafe.ai/primitives/choice)】

| 字段 | 必填 | 类型 | 语义 |
| --- | --- | --- | --- |
| `type` | 是 | `"choice"` | 固定字面量 |
| `instructions` | 是 | `string \| object \| array` | 要让模型决定什么。可写成疑问句或陈述句 |
| `criteria` | 是 | `map<string, string \| object \| array \| null>` | **选项名 → 该选项的评分说明（rubric description）**；不需要额外说明时用 `null`。**上限 255 个选项** |

要点：
- 选项名与选项描述**都会发给模型**，所以描述要能相互区分；文中示例注明"你选的题目 id 不会被模型看到"。
- 当选项集合可能覆盖不了全部输入时，应显式加一个 `other` / `none of the above` 选项——因为模型必须选一个。
- 想要"是否适用"这种判断，不要指望 Choice：官方与第三方都指出 **Choice 的概率必然归一，所以总会有一个赢家**；Spring AI 的做法是另加一个 Noul 问"到底有没有适用的工具"。【官方：[/primitives/choice](https://docs.typesafe.ai/primitives/choice)；第三方实测：Spring AI 博客】

### 3.2 返回值【官方：[/api](https://docs.typesafe.ai/api)、[/primitives/choice](https://docs.typesafe.ai/primitives/choice)】

| 字段 | 类型 | 语义 |
| --- | --- | --- |
| `type` | `"choice"` | 与问题对应 |
| `choice` | `string` | 概率最高的选项名 |
| `probabilities` | `map<string, number>` | 每个选项一个概率，**和为 1** |
| `confidence` | `number` | 0–1，**由 `probabilities` 的分布形状导出**：单峰高、摊平低 |

补充：官方的 confidence 演示页明确写出 **"All of it on one option gives 1.0; the more evenly it spreads, the lower the confidence. This demo uses `(3 × largest probability − 1) / 2` to approximate confidence for three options."**——即该公式只是 3 选项演示用的近似，**不是**通用定义。【官方：[/confidence](https://docs.typesafe.ai/confidence)（公式位于页面交互式 demo 的说明文字内，`curl` 原始 HTML 可见）】

### 3.3 请求示例（HTTP 原文）【官方：[/api](https://docs.typesafe.ai/api)】

```json
{
  "state": "Help! My payouts have been failing for 3 days.",
  "model": "jev-latest",
  "questions": {
    "department": {
      "type": "choice",
      "instructions": "Which team should handle this?",
      "criteria": {
        "billing": "Payments, invoicing, refunds",
        "technical": "Bugs, outages, integrations",
        "sales": "Pricing, upgrades, new accounts"
      }
    }
  }
}
```

### 3.4 响应示例（HTTP 原文）【官方：[/api](https://docs.typesafe.ai/api)】

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

### 3.5 多问题与低置信度示例（节选自 5 题同请求的官方示例）【官方：[/primitives/choice](https://docs.typesafe.ai/primitives/choice)】

```json
{
  "model": "jev-1.13.0",
  "answers": {
    "department": {
      "type": "choice",
      "choice": "returns",
      "confidence": 0.42,
      "probabilities": { "shipping": 0.04, "billing": 0.35, "returns": 0.61 }
    },
    "shipping_issue": {
      "type": "choice",
      "choice": "delayed",
      "confidence": 0.67,
      "probabilities": {
        "wrong_address": 0.0, "other": 0.26, "not_delivered": 0.0,
        "damaged_in_transit": 0.0, "delayed": 0.74
      }
    },
    "tone": {
      "type": "choice",
      "choice": "frustrated",
      "confidence": 0.76,
      "probabilities": { "frustrated": 0.84, "angry": 0.16, "calm": 0.0 }
    }
  },
  "usage": { "input_tokens": 589, "output_tokens": 212 }
}
```
官方对此的读法：工单同时属于两个团队（returns 0.61 / billing 0.35），所以 `confidence` 只有 0.42；`requested_resolution` 的 confidence 仅 0.20，示例代码据此**改为向客户确认**而不是直接行动。

### 3.6 选项描述的质量会影响结果（第三方实测）

同一个工单，用 `Choice.of("Which team?", "billing", "technical", "sales")`（只有裸标签）→ confidence 掉到 **0.60**；加上选项描述后 → **0.82**。【第三方实测：Spring AI 博客】

### 3.7 SDK 形态（Python 原文）【官方：[/introduction/quickstart](https://docs.typesafe.ai/introduction/quickstart)】

```python
from typesafe_sdk import Choice, Noul, Score, TypeSafeClient

client = TypeSafeClient()
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
        # ...
    },
)
print(response.answers["department"].choice)  # "technical"
```
JavaScript / TypeScript SDK：`import { choice, TypeSafeClient } from "@typesafe-ai/sdk"`，`answers.category.choice` 的类型由你的 options 推导成字面量联合类型。【官方：[/sdk/javascript](https://docs.typesafe.ai/sdk/javascript)、[/sdk/javascript/api/type-aliases/ChoiceCriteria](https://docs.typesafe.ai/sdk/javascript/api/type-aliases/ChoiceCriteria)】

---

## 4. Score（在一条有序列标尺上取值）

### 4.1 字段【官方：[/api](https://docs.typesafe.ai/api)、[/primitives/score](https://docs.typesafe.ai/primitives/score)】

| 字段 | 必填 | 类型 | 语义 |
| --- | --- | --- | --- |
| `type` | 是 | `"score"` | 固定字面量 |
| `instructions` | 是 | `string \| object \| array` | 在给什么打分 |
| `criteria` | 是 | `array<string \| object \| array>` | **有序**的层级描述数组，从低到高。**至少 2 级；API 最多接受 10 级** |

**层级编号 = 数组下标，从 0 开始**（数组顺序即编号）。模型只看到描述文本、看不到编号，也看不到相邻层级，因此"比上一级更严重"这种写法无意义；写数字（如 `["0","1","2"]`）更是无参照物，官方给了实测：同一份 Safari 报告，描述式三级 → `score 0.0 / confidence 1.0`；纯数字三级 → `score 0.55 / confidence 0.33`，概率被摊到 0 和 1 之间。【官方：[/primitives/score](https://docs.typesafe.ai/primitives/score)】

官方写作建议：**描述情境而不是程度**（"Broken or degraded feature, but workaround exists" 好过 "moderately severe"）；每个 Score 只测一个维度；顶部若有需要特殊处理的极端情形，单独给一级；能描述清楚的层级才加，最多 10 级、3 级也够用。

### 4.2 返回值【官方：[/api](https://docs.typesafe.ai/api)、[/primitives/score](https://docs.typesafe.ai/primitives/score)】

| 字段 | 类型 | 语义 |
| --- | --- | --- |
| `type` | `"score"` | 与问题对应 |
| `score` | `number` | 层级编号的**概率加权平均**，范围 0 到最高层级编号，**可以落在两级之间** |
| `legend` | `map<string, string>` | 层级编号（字符串 key）→ 该级描述 |
| `probabilities` | `map<string, number>` | 层级编号（字符串 key）→ 概率，和为 1 |
| `confidence` | `number` | 0–1，由分布形状导出 |

注意两处实现细节：
- **HTTP 层**：`legend` / `probabilities` 的 key 是**字符串**形式的层级号（`"0"`, `"1"`, `"2"`）。【官方：/api】
- **Python SDK 层**：`ScoreAnswer` 的 `legend` 是 `dict[int, str|dict|list]`、`probabilities` 是 `dict[int, float]`，即 **SDK 用整数 key**。【官方：[/sdk/python/api/types/responses](https://docs.typesafe.ai/sdk/python/api/types/responses)】
- 官方明示的计算：`0 × 0.0 + 1 × 0.57 + 2 × 0.43 = 1.43`。
- **同一个 score 值可以有不同分布**（1.0 可能是全压在 1 级，也可能是 0 级和 2 级各一半），必须结合 `probabilities` 与 `confidence` 读。【官方：/primitives/score】

### 4.3 请求示例（HTTP 原文）【官方：[/api](https://docs.typesafe.ai/api)】

```json
{
  "state": "Help! My payouts have been failing for 3 days.",
  "model": "jev-latest",
  "questions": {
    "frustration": {
      "type": "score",
      "instructions": "How frustrated is the customer?",
      "criteria": ["Calm", "Frustrated", "Very angry"]
    }
  }
}
```

### 4.4 响应示例（HTTP 原文）【官方：[/api](https://docs.typesafe.ai/api)】

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

### 4.5 层级可在两级之间：完整示例（官方 Safari 报告）【官方：[/primitives/score](https://docs.typesafe.ai/primitives/score)】

```json
{
  "model": "jev-1.13.0",
  "answers": {
    "bug_severity": {
      "type": "score",
      "score": 1.43,
      "confidence": 0.35,
      "legend": {
        "0": "Cosmetic; no impact to functionality",
        "1": "Broken or degraded feature, but workaround exists",
        "2": "Blocking issue; no workaround exists"
      },
      "probabilities": { "0": 0.0, "1": 0.57, "2": 0.43 }
    }
  },
  "usage": { "input_tokens": 332, "output_tokens": 18 }
}
```

### 4.6 多维度复合打分（官方 Composite scoring）

官方给了一个三 Score 同请求的例子（严重度 / 沮丧度 / 报告质量），并给出**跨尺度合并前必须归一化**的做法：`score / (len(criteria) - 1)`，再加权求和：`0.6 × 0.62 + 0.3 × 0.64 + 0.1 × 1.0 = 0.664`。【官方：[/primitives/score](https://docs.typesafe.ai/primitives/score)、[/patterns/composite-scoring](https://docs.typesafe.ai/patterns/composite-scoring)】

### 4.7 结构化层级可提升稳定性（官方给了对照表）

同一份报告，纯文本描述 → `1.43 / confidence 0.35`；给某一级加上**贴切的** `examples` → `1.03 / 0.96`；加上**不相关**的 examples → 仍为 `1.43 / 0.35`。官方同时警告：**更高的 confidence 并不证明答案更正确**，要用已知答案的样本验证。【官方：/primitives/score】

---

## 5. Noul（是非判断，返回概率）

### 5.1 字段【官方：[/api](https://docs.typesafe.ai/api)、[/primitives/noul](https://docs.typesafe.ai/primitives/noul)】

| 字段 | 必填 | 类型 | 语义 |
| --- | --- | --- | --- |
| `type` | 是 | `"noul"` | 固定字面量 |
| `instructions` | 是 | `string \| object \| array` | 要判断的是非问题，或一个待判断真假的陈述 |
| `criteria` | 否 | `object`，含 `true` / `false` 两个键 | **可选**：分别说明"是"和"否"各指什么（各自可为 string/object/array/null） |

写作建议（官方）：一个问题只问一件事（两个条件要拆成两个 Noul，在代码里组合）；措辞要让**高值 = 是**（"message 是否包含个人数据" 好过 "message 是否不含个人数据"）；边界含糊时用 `criteria` 的 true/false 钉住定义；先不带 criteria 试、再带 criteria 试，保留在本数据上更好的那个。

### 5.2 返回值【官方：[/api](https://docs.typesafe.ai/api)、[/primitives/noul](https://docs.typesafe.ai/primitives/noul)】

| 字段 | 类型 | 语义 |
| --- | --- | --- |
| `type` | `"noul"` | 与问题对应 |
| `noul` | `number` | 答案为"是"的概率，0–1。接近 1 强是，接近 0 强否，接近 0.5 = 模型给出的两者概率相近 |

- **没有独立 `confidence`**：二值分布只有一个自由度，`noul` 值本身就完整描述了它。【官方：/primitives/noul、/confidence】
- **0.5 不是"中等"**：它表示"模型分不清"，不表示所问的量处于中间。官方用"候选人的 Python 强度"做了对照：Noul 给 Java/Go 背景候选人 0.03、偶尔写脚本 0.14、每天用两年 0.81、八年深度 0.92；同一批输入用 4 级 Score 时依次落在 0.0 / 1.0 / 2.05 / 2.89——**要测程度就用 Score**。【官方：/primitives/noul】
- 官方给出 `jev-1.13.0` 的实测记录表（同一 Noul，不同输入）：`"Thanks, that fixed it!"` → 0.02；`"How do I reset my password?"` → 0.07；`"I need this sorted today, whatever it takes."` → 0.26；`"Are you a bot?"` → 0.40；`"Is there any way to speak to someone about my invoice?"` → 0.84；`"I have asked three times now. Can I please just talk to a real person?"` → 0.99。
- 官方建议的三种处理：0.5 附近用三档（高置信执行 / 中间交人 / 低置信不动），阈值随"判断错的代价"调整。

### 5.3 请求示例（HTTP 原文，含 criteria）【官方：[/api](https://docs.typesafe.ai/api)】

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

### 5.4 响应示例（官方文档中两处示例分别为 0.95 与 0.999）

```json
{
  "model": "jev-1.13.0",
  "answers": {
    "is_urgent": { "type": "noul", "noul": 0.95 }
  },
  "usage": { "input_tokens": 296, "output_tokens": 20 }
}
```
【官方：[/api](https://docs.typesafe.ai/api)】

LangChain 博客给出的同一问题（"是否紧急"）变体返回 `0.999`，并称其为"99.9% 概率紧急"。【第三方转述：LangChain 博客】

### 5.5 两个 Noul 同请求（官方示例，含 criteria 与阈值代码）【官方：[/primitives/noul](https://docs.typesafe.ai/primitives/noul)】

```json
{
  "model": "jev-1.13.0",
  "answers": {
    "is_human_escalation": { "type": "noul", "noul": 0.99 },
    "is_repeat_contact":     { "type": "noul", "noul": 0.93 }
  },
  "usage": { "input_tokens": 360, "output_tokens": 39 }
}
```

---

## 6. 并行多问题：机制

官方口径（多来源一致）【官方：[/primitives](https://docs.typesafe.ai/primitives)、[/introduction](https://docs.typesafe.ai/introduction)、[/patterns/fan-out](https://docs.typesafe.ai/patterns/fan-out)、[/cookbooks/parallel_questions](https://docs.typesafe.ai/cookbooks/parallel_questions)】：

- **一次请求里的所有问题共用同一份 `state`，并行且相互隔离地评估**。"Every question in a request sees the same state, is evaluated independently"。
- **答案之间不构成上下文**：一个问题不会成为另一个问题的隐性提示，可以随意增删问题而不影响其它问题的结果。
- **真依赖必须落在代码里**：如果后一个问题依赖前一个答案（要用答案去取数据、决定 state 的构成、或决定下一题的选项），就发第二次请求；除此之外官方建议全部放在一次请求里，用代码忽略不需要的答案。这就是官方命名的 **Speculative fan-out（推测式扇出）**。
- **增加问题几乎不增加响应时间**，只增加问题本身的 token（"costs only the tokens for the extra questions, which are cheap"；"Asking a question you might not need is close to free"）。
- 官方给出**量化实测**（cookbook 原文表格，5 次重复取均值）：
  - 1 次调用（13 题同时）：**1 次调用、$0.000497、0.27s**
  - 13 次调用（每次 1 题）：**13 次调用、$0.006090、2.71s**
  - 结论：**12.2× 更便宜、10.0× 更快**；答案不变。
  - 逐题均值和 std dev 表显示：2 个 Choice、3 个 Score、8 个 Noul 中的 6 个在两种策略下 5 次重复完全一致（std dev = 0.0）；`breach_72h`（mean 0.804 vs 0.814）与 `criminal_penalties`（0.108 vs 0.108，std 0.0045 vs 0.0084）有**题目自身固有的采样噪声**，两种策略下量级相同 → "批处理既不改变答案也不增加方差"。
  - 测试条件：GDPR 的 Wikipedia 固定修订版正文，53,777 字符（页面文字另处写作 ~54,000）；13 题 = 8 Noul + 2 Choice + 3 Score；`RUNS = 5`；文档字节级一致；成本按 `PRICE = (0.042, 0.00)` 事后计算；延迟为 13 次单题延迟之和（顺序调用假设）。
- **并发注意（官方自陈）**：单题延迟之和假设顺序调用；若并发发起，速度差会缩小，但 **13× 的 token 成本不变**。
- 第三方对同一机制的复述与补充：LangChain 博客（并行、增加问题几乎不影响响应时间、成本只增问题 token）、Spring AI 博客（"The service reads the state once and answers every question in parallel (for negligible cost)"，并给出笔记本实测：1 题 275ms / 3 题 310ms，即多 2 题多约 35ms）、dev.to 与 MarkTechPost（并行隔离、12.2×/10.0×）。

---

## 7. 并行多问题：限制清单

| 项目 | 限制值 | 来源 | 证据等级 |
| --- | --- | --- | --- |
| Choice 选项数 | **最多 255**（官方 API 参考原文："You can have a maximum of 255 options per Choice"） | [官方 /api](https://docs.typesafe.ai/api)、[/primitives/choice](https://docs.typesafe.ai/primitives/choice) | 官方；第三方交叉验证：MarkTechPost、dev.to、flaviocopes、HexDocs Elixir 客户端（"Choice permits 1–255 options, including nil descriptions"） |
| Score 层级数 | **至少 2、最多 10**（"Should have at least two levels; the API accepts up to 10"） | [官方 /api](https://docs.typesafe.ai/api)、[/primitives/score](https://docs.typesafe.ai/primitives/score) | 官方；第三方交叉验证：dev.to（"Two to ten ordered levels"）、HexDocs（"Score requires 2–10 ordered levels"）、ayautomate、opentweet |
| 单请求题目数 | **官方未声明上限**；实际受上下文与速率限制约束 | [官方 /api](https://docs.typesafe.ai/api)、[/models](https://docs.typesafe.ai/models)（均未提及题目数上限） | 负向结论。**已用官方全量文档导出语料 `llms-full.txt`（903,311 字节）做整库全文检索复核：无任何"最大问题数 / questions per request"表述**。第三方独立确认：ayautomate 明确写"The docs do not state a maximum number of questions per request … We do not guess at them."；opentweet 的 limits 页同样只列 state/选项/层级/速率，不列题目数 |
| 上下文长度 | **64k tokens / 请求**（state + 全部问题合计） | [官方 /models](https://docs.typesafe.ai/models) | 官方；第三方交叉验证：ayautomate、opentweet、dev.to |
| 单题最长 | **32k tokens**（state + 单个最长问题） | [官方 /models](https://docs.typesafe.ai/models) | 官方；第三方交叉验证：ayautomate、opentweet |
| 速率限制 | **250,000 tokens/秒** 与 **1,200 请求/分钟**，任一超限返回 429 | [官方 /models](https://docs.typesafe.ai/models) | 官方；第三方交叉验证：opentweet、flaviocopes（并注明"会随容量动态调整"） |
| 输入模态 | **仅文本**（字符串 / JSON 对象 / 文本数组）；无图像、音频、视频 | [官方 /models](https://docs.typesafe.ai/models) | 官方；第三方：dev.to、flaviocopes、ayautomate |
| 题目 id | 由调用方自选，**不发给模型**，答案按同一 id 返回 | [官方 /primitives/choice](https://docs.typesafe.ai/primitives/choice)、[/api](https://docs.typesafe.ai/api) | 官方；第三方：flaviocopes（"The ID is for your code and is not sent to the model"） |
| 错误码 | `401` 缺少/错误 key；`422` 请求体校验失败（会指出问题字段）；`429` 超速率；`529` 服务过载（建议指数退避，SDK 默认自动重试并遵循 `retry-after`） | [官方 /api](https://docs.typesafe.ai/api) | 官方；第三方：flaviocopes、ayautomate（422 = 空 questions map 或未知 type；400 = "Invalid request."） |
| 输出 | 只有数字与标签，无生成文本；**Jev 不流式输出** | [官方 /models](https://docs.typesafe.ai/models) | 官方 + 第三方实测：Spring AI 博客（"Jev does not stream. Nothing is generated token by token"） |
| 高基数技巧 | 官方博客提到：Jev 支持到 255 基数；**对更高基数的选择，官方内部用"先独立打分、再显式选择"的两阶段系统**，因此偶发变慢 | [官方博客](https://typesafe.ai/blog/introducing-system-one-models-and-jev) | 官方（仅见于博客，文档正文未展开） |

其他实现细节：
- **逐文档 = 逐调用**：Spring AI 博客在实测警告中写道 "Per-document work is one call per document. Reranking a top-20 list is twenty calls"，因此他们的做法是先用 `JevDocumentFilter` 过一遍、只对幸存者排序。（这是一条语用推论，不是官方明文限制；官方 cookbook 的重排示例本身也是"一个 query-candidate 对一个 Noul"。）【第三方实测：Spring AI 博客】
- 官方 cookbook 记录：`rerank_typesafe` 用 40 条法律查询 × 30 个候选做重排，共 **1,200 次调用花费 $0.0645**，把正确段落进入 top-10 的比例从 38% 提到 62%。【第三方转述官方 cookbook：ayautomate】
- **客户端侧校验（对 Rust 实现有直接参考价值）**：官方 JS SDK 的 `TypeSafeClient.systemOne()` 明确声明会抛出 **"Questions are empty, or score criteria are not a list of at least two entries."**，即"空 questions"与"Score 层级少于 2 条"在客户端就被拦下；官方 SDK 参考中另一处写作 "Questions are empty or a score question's criteria list is empty"。【官方：[/sdk/javascript/api/classes/TypeSafeClient](https://docs.typesafe.ai/sdk/javascript/api/classes/TypeSafeClient)，原文见官方全量语料 `llms-full.txt`】
- **state 的类型校验原文**：官方 State 页 Note 段写 "Jev accepts text only. **State must be a string, JSON object, or array of text values.** Images, audio, and video are not supported (yet). Jev's primary training language is English; other languages, including CJK scripts, are accepted but currently have lower accuracy."【官方：[/concepts/state](https://docs.typesafe.ai/concepts/state)、全量语料 `llms-full.txt`】

---

## 8. 计费方式

**官方口径**（[官方 /models](https://docs.typesafe.ai/models)）：

| 项目 | 值 |
| --- | --- |
| 输入 | **$0.042 / 1,000,000 tokens**（页面同时写作 **$42 / Btok**） |
| 输出 | **免费**（"Charged per input token. Output tokens are free."） |
| 计量单位 | 响应体 `usage.input_tokens` / `usage.output_tokens`（输出免费但仍会报告 token 数） |

**成本模型的关键推论**：

1. **state 每次请求只计一次费**，所以"把 N 个问题放进一次请求"省下的是 state 的 (N−1) 次重复计费；问题本身的 token 照价计费，但很便宜。官方原文："costs only the tokens for the extra questions, which are cheap"。这解释了 13 题批量为什么省 12.2×：文档主导型负载下，13 次单题请求把同一份 5.4 万字符正文付了 13 遍。
2. 官方 cookbook 的原始金额（同一负载、5 次重复均值）：批量 **$0.000497**（1 次调用 / 0.27s）vs 单题 **$0.006090**（13 次调用 / 2.71s）。
3. 官方 cookbook 的计费常量写在代码里：`PRICE = (0.042, 0.00)  # $ per 1M tokens (input, output)`，且注明 "Price after cache retrieval, so a price change needs no new calls"。**注意**：另一份官方 cookbook（self-consistency）明确声明其成本用的是"历史价格假设，不是经过核实的 jev-latest 价格或当前账单金额"——引用单个 cookbook 的绝对金额时需保留这句限定。
4. **官方对自家价格的自我限定**：官网博客承认 "We can't prove it isn't subsidized; we'll need the long-term to prove the sustainability of our pricing"。
5. **未找到公开信息**：是否存在最低计费单位（按请求/按次起收）、重试或失败请求是否计费。Elixir 客户端作者指出传输层失败后重放是"ambiguous: the server may already have processed and billed the evaluation"，因此默认不重放——这是第三方工程判断，不是官方口径。

**第三方实测成本（与官方口径区分）**：

| 数据 | 值 | 测试条件 | 来源 |
| --- | --- | --- | --- |
| 8 路意图分类 | **$0.0151 / 1,000 decisions**（提示词平均 ~360 输入 tokens） | 自测 791 条标注决策，2026-09-19；经 OpenRouter | 【第三方实测】[ayautomate](https://www.ayautomate.com/blog/how-to-use-jev-guide) |
| 77 路意图分类 | **$0.0400 / 1,000**（平均 952 tokens/prompt） | 同上 | 【第三方实测】ayautomate |
| 对比 LLM | 同样 8 路消息：GPT-5.6 Terra $0.609/1k、Claude Haiku 4.5 $0.356/1k、Gemini 3.5 Flash-Lite $0.087/1k、GPT-5.4 nano $0.070/1k | 经 OpenRouter 计费 | 【第三方实测】ayautomate |
| 延迟 | 791 次调用中位数 **0.33s**、P95 **0.44s**、最慢 1.42s | 单机单客户端、4 路并发（作者明确说"当作相对值，不是 SLA"） | 【第三方实测】ayautomate |
| 多问题成本 | 1 题 275ms → 3 题 310ms；3 题合计 73 output tokens | 作者笔记本单次运行 | 【第三方实测】Spring AI 博客 |
| 官方自报延迟 | 端到端 **70ms–500ms**（对比前沿 LLM 的 3s–329s） | 官方 eval，注明多在美西笔记本上跑 | 【官方】[博客](https://typesafe.ai/blog/introducing-system-one-models-and-jev)、[/introduction](https://docs.typesafe.ai/introduction) |
| 官方 headline | **193.6× faster / 444.6× cheaper** | 来自官方 workflow evals，官方自己说"预期这是真实收益的高位" | 【官方】博客；【第三方转述并提醒是厂商自测】MarkTechPost、flaviocopes、dev.to |
| 官方自报演示 | Jev 0.114s / $0.000081 vs GPT-5.6 Terra 8.566s / $0.013880 | 官方录制的 side-by-side demo | 【第三方转述官方】MarkTechPost |

---

## 9. 一次请求覆盖三类原语的完整示例（官方 Quickstart 原文）

请求（Python SDK 原文）【官方：[/introduction/quickstart](https://docs.typesafe.ai/introduction/quickstart)】：

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

同义的三类问题纯 HTTP 版本（第三方实测跑通的等价形状，可见三类原语在同一 `questions` map 中共存）【第三方实测：ayautomate；形状与官方 /api 示例一致】：

```json
{
  "state": "Hi, I've been trying to connect my Stripe account for 3 days and the integration keeps failing. I'm losing sales. Please help ASAP.",
  "model": "jev-latest",
  "questions": {
    "department": {
      "type": "choice",
      "instructions": "Which team should handle this?",
      "criteria": {
        "billing": "Payment or subscription issues",
        "technical": "Bugs or integration problems",
        "sales": "Pricing or account questions"
      }
    },
    "frustration": {
      "type": "score",
      "instructions": "How frustrated does the customer appear?",
      "criteria": ["Calm, just stating facts", "Frustrated but civil", "Very angry, strong language"]
    },
    "is_urgent": {
      "type": "noul",
      "instructions": "Does this message express urgency?"
    }
  }
}
```

---

## 10. 不确定与未证实之处（诚实清单）

1. **题目数量上限：官方未声明。** 官方同时给出选项数（255）与层级数（10）上限，但通篇未给单请求题目数上限；**已用官方全量文档导出语料（`llms-full.txt`，903,311 字节）做整库检索，确认不存在任何相关表述**；第三方（ayautomate）也明确指出"文档未声明"，并声明不猜测。**实际可用上限只能由 64k / 32k 上下文预算和 1,200 req/min 的速率上限推出，属于工程推算，不是官方数字。**
2. **confidence 的确切统计量未公开。** 官方只说"由 `probabilities` 的分布形状导出"；唯一给出公式的地方是 `/confidence` 页面交互 demo 的说明："(3 × largest probability − 1) / 2 to approximate confidence for three options"——这是**3 选项近似**。**一般 N 选项 / M 层级的闭式公式：未找到公开信息。**（因此不要在自己的实现里假装复刻官方 confidence。）
3. **官方内部数字不一致**：`/primitives` 总览页写 13 题批量为 "11.5x cheaper, 9.6x faster"，`/cookbooks/parallel_questions` 写 "12.2x cheaper, 10.0x faster"（且带原始金额表格，自洽）。引用时以 cookbook 为准并注明这一分歧。
4. **官方内部字段列表不一致（小）**：`/introduction` 的总览表把 Score 的返回写成 "score, probabilities, confidence"，漏了 `legend`；`/primitives` 与 `/api` 都含 `legend`。以 `/api` + `/primitives/score` 为准（实测 JSON 里确有 `legend`）。
5. **计费的边界情形未找到公开信息**：最低计费单位、失败/重试请求是否计费、429 退避重试是否重复计费。Elixir 客户端关于"传输失败可能已计费"的说法是第三方工程推断。
6. **价格可持续性由官方自己保留**：官方明说无法证明价格未被补贴、需要长期验证；且官方首页的数字（193.6×/444.6×）是**厂商自测**，官方自己承认处于"真实收益的高位"，第三方也反复提醒"vendor-run, test on your own data"（MarkTechPost、dev.to、Spring AI）。
7. **版本漂移风险**：`jev-latest` 会随新版本移动，官方明确说答案可能随之改变，建议在阈值调优后**固定版本号**（如 `jev-1.13.0`）。cookbook 里同时出现过 `jev-1.12`、`jev-1.13.0`、`jev-latest`，跨页面的绝对数字（价格、延迟）未必同版本同条件。
8. **第三方页面本身的性质**：opentweet 是聚合型站点（内容自称读取自官方 models 页与 API 参考，2026-09-18）；dev.to/ayautomate/flaviocopes 是个人或厂商博客。它们用于交叉验证"字段名、上限值、请求形状"这类结构性事实是可靠的，但其中的**基准数字属于各自口径**，不能与官方数字混用。
9. **未找到**：任何官方说明 Noul 是否有内部 confidence、Choice 与 Score 的 confidence 是否可比、以及不同语言（含中文）在 Score 层级语义上的行为差异（官方只说英文是主要训练语言、其它语言可用但不等同，需要自测）。

---

## 11. 对本项目（Rust Agent Harness，LLM + Jev 融合）的 1–3 条启示

1. **把三类原语建成 Rust 的封闭枚举 + 强类型答案，并把"答案 id"设为编译期可见的类型参数或新类型（newtype）**。官方线上形状是"`questions` 是 `map<id, Question>`，`answers` 以同一 id 回传，且 id 不发给模型"；Python SDK 用 `dict`、JS SDK 用字面量联合类型做推导、Elixir 客户端坚持"id 与概率 key 一律是字符串，绝不从服务端输入创建 atom"。对 Rust 而言，`ChoiceQuestion<T>` 用 `T: Serialize` 承载 `criteria`、`ChoiceAnswer` 用 `HashMap<String, f64>` 承载 `probabilities` 是安全的近似；但 `ScoreAnswer.legend/probabilities` 需要注意 **HTTP 层是字符串 key、官方 Python SDK 转成整数 key** 这一分裂——我们应显式选一侧并在文档里写明（建议在反序列化后立刻规范化为 `BTreeMap<u8, _>`，因为层级编号天然有序且 ≤10）。
2. **`score` 的"可为小数 + 概率分布才是原始事实"必须原样穿透进 Harness 的决策层**，不要过早 `round()`。官方反复强调 `score` 是概率加权均值（同一 score 可对应完全不同的分布），并且 Noul 的 0.5 意味着"模型分不清"而非"中等"。这直接支持项目总纲里的"概率优先"：Harness 内部应以 `probabilities` + `confidence` 为一等数据，只有在具体行动点（如工具门控）才按风险选择阈值三档（低置信交人/中间复核/高置信执行）——LangChain 的 `AutoModeMiddleware` 与 Spring AI 的 `JevGuardrailAdvisor` 都是"用 Jev 守危险动作"的现成范式，可直接借鉴其分层阈值设计。
3. **默认把所有"可以预判的问题"塞进同一次请求（Speculative fan-out），并把真实的因果依赖显式建模成第二次调用**。官方实测 13 题单请求比 13 次单题便宜 12.2×、快 10.0×且答案不变，加上"增加问题几乎不增加延迟、只增加少量 token"，意味着在 Harness 里把"路由 / 紧急度 / 工具适用性 / 结果评分"等判定合并进一次调用是**正确默认**。但两条硬约束必须在 Rust 侧做预算守卫：(a) 64k 总量 + 32k 单题的上下文预算，超了要按"先过滤 state 再发问"的原则裁剪（官方明说无关上下文会降低准确率）；(b) **答案之间互不为上下文**——任何"第二问要看到第一问结果"的设计都必须落到两次调用，而不是期望模型自己串起来（官方 cookbook 里只有 skill suggestion / structure recovery / hierarchical classification 三类才是真正的两段式）。

---

## 附录：引用清单（全部来源 URL）

### A. 官方（TypeSafe）

| 页面 | URL | 本专题用到的内容 |
| --- | --- | --- |
| Introduction | https://docs.typesafe.ai/introduction | 三原语总览表、并行与隔离、原子化问题与代码组合 |
| Quickstart | https://docs.typesafe.ai/introduction/quickstart | 三类原语混用的 SDK 示例与打印结果（technical / 1.0 / 1.0） |
| Primitives（总览） | https://docs.typesafe.ai/primitives | 通用字段（ID/type/instructions/criteria）、返回值表、并行与推测式提问、11.5x/9.6x 表述 |
| Advanced（结构） | https://docs.typesafe.ai/primitives/advanced | `EntryType = string\|object\|array\|null` 适用范围、结构化 instructions/选项/层级/criteria |
| Choice | https://docs.typesafe.ai/primitives/choice | Choice 字段与返回、255 选项、5 题同请求的响应 JSON、低置信度用法 |
| Score | https://docs.typesafe.ai/primitives/score | Score 字段、层级编号从 0、≥2/≤10、`score` 加权平均（1.43）、legend、归一化、结构化层级对照表 |
| Noul | https://docs.typesafe.ai/primitives/noul | Noul 字段、可选 criteria、无 confidence、`jev-1.13.0` 实测概率表、Noul vs Score 对照 |
| API reference | https://docs.typesafe.ai/api | 请求/响应完整字段、255 选项、Score 2–10、错误码 401/422/429/529、三类原语的最小 JSON 示例 |
| Models | https://docs.typesafe.ai/models | 价格 $42/Btok = $0.042/Mtok、输出免费、速率 250k tok/s 与 1,200 req/min、64k / 32k 上下文、仅文本、别名与版本 |
| Confidence | https://docs.typesafe.ai/confidence | confidence 由分布形状导出、三档用法、演示公式 `(3 × 最大概率 − 1) / 2`（3 选项） |
| Patterns: Speculative fan-out | https://docs.typesafe.ai/patterns/fan-out | 全量提问 + 代码筛选、并行几乎不影响延迟 |
| Patterns: Composite scoring | https://docs.typesafe.ai/patterns/composite-scoring | 多 Score 拆分与加权组合 |
| Cookbook: Parallel questions | https://docs.typesafe.ai/cookbooks/parallel_questions | 13 题批量实测表（$0.000497/0.27s vs $0.006090/2.71s，12.2× / 10.0×）、逐题 std dev 表、`PRICE=(0.042,0.00)`、测试条件 |
| Cookbook: Self-consistency (nouls) | https://docs.typesafe.ai/cookbooks/consistency_noul_cookbook | 14 Noul / 15 次重复；均差 std 0.0102；TypeSafe 平均 111ms vs LLM 1.1–13.9s；成本为"历史价格假设、未核实"的声明 |
| Cookbook: Consistency (choice) | https://docs.typesafe.ai/cookbooks/consistency_choice_cookbook | 同类稳定性实验（Choice 侧，未在正文引用具体数字） |
| Python SDK: questions / responses | https://docs.typesafe.ai/sdk/python/api/types/questions · https://docs.typesafe.ai/sdk/python/api/types/responses | `Noul` / `Choice` / `Score` 的 pydantic 字段与答案类型（`ScoreAnswer.legend/probabilities` 为整数 key） |
| JavaScript SDK | https://docs.typesafe.ai/sdk/javascript · https://docs.typesafe.ai/sdk/javascript/api/type-aliases/ChoiceCriteria | `choice` / `noul` / `score` 构造器、从选项推导字面量类型、`ChoiceCriteria` 类型别名 |
| JS SDK 客户端类（校验行为） | https://docs.typesafe.ai/sdk/javascript/api/classes/TypeSafeClient | `systemOne()` 抛错条件："Questions are empty, or score criteria are not a list of at least two entries." |
| State 概念页 | https://docs.typesafe.ai/concepts/state | state 的三种格式、`state must be a string, JSON object, or array of text values`、仅文本、英文为主语言 |
| 官方博客（发布） | https://typesafe.ai/blog/introducing-system-one-models-and-jev | 193.6×/444.6×、70ms–500ms、$0.042/Mtok、255 基数与两阶段高基数技巧、价格补贴的自我保留 |
| **官方全量文档导出语料** | `https://docs.typesafe.ai/llms-full.txt`（903,311 字节，另见同站 `llms.txt`） | 整库复核用：确认"255 选项""Score 2–10"多处出现、并确认**全库无单请求题目数上限的表述**；`confidence` 公式不在该语料中（只在 `/confidence` 页 demo 组件里） |

### B. 第三方（按证据用途分组）

| 来源 | URL | 类型 | 本专题用到的内容 |
| --- | --- | --- | --- |
| LangChain 博客 | https://www.langchain.com/blog/building-a-harness-with-jev | 框架厂商，官方合作视角 | 三原语语义、`TypeSafeClassifier`、模型路由中间件、`AutoModeMiddleware` 工具门控、`noul: 0.999` 示例、上游口径 200×/400× |
| Spring AI 博客 | https://spring.io/blog/2026/09/21/spring-ai-typesafe-structured-judgment | 框架厂商 + 真实运行 | 1 题 275ms / 3 题 310ms（笔记本）；裸标签使 confidence 由 0.82 降至 0.60；"Choice 必有赢家，需另加 Noul"；state 为标量 → 422；不流式；JevJudge/Guardrail 分层阈值（0.70/0.35） |
| dev.to（valyuai） | https://dev.to/valyuai/how-to-use-jev-a-practical-guide-to-typesafes-system-one-model-g5e | 第三方实操指南 | 255 选项、2–10 级、12.2×/10.0×、64k/32k、$0.042、并自陈"均为 TypeSafe 自报，未经复现" |
| ayautomate | https://www.ayautomate.com/blog/how-to-use-jev-guide | 第三方实测（自测 791 条标注决策） | 延迟中位 0.33s / P95 0.44s；成本 $0.0151 与 $0.0400 / 1k decisions；与 GPT-5.6 Terra 等对比；**明确记录"文档未声明题目数上限"**；错误码补充（400/422） |
| MarkTechPost | https://www.marktechpost.com/2026/09/19/typesafe-ai-releases-jev/ | 第三方新闻 | 255 选项、$0.042/1M 与免费输出、并行隔离、193.6×/444.6× 与 demo 数字、提醒"benchmarks are vendor-run" |
| flaviocopes | https://flaviocopes.com/jev/ | 第三方深度解读 | 请求/响应形状逐字段拆解、255/2–10、64k/32k、题目 id 不发给模型、Vercel AI SDK 映射（`boolean`→`probability`）、"193.6×/444.6× 来自厂商自测，应视为上限" |
| HexDocs（Elixir 客户端 typesafe_ai） | https://hexdocs.pm/typesafe_ai/readme.html | 独立开源客户端 | 交叉印证："Choice permits 1–255 options, including nil descriptions; Score requires 2–10 ordered levels"、Noul 无 confidence、id/概率 key 一律字符串 |
| opentweet | https://opentweet.io/jev/limits | 第三方聚合（自称读取官方页面） | 限额汇总（64k/32k、250k tok/s、1,200 req/min、255、2–10）、未列题目数上限 |

### C. 检索与抓取方式备注

- 官方站点页面清单来自 `https://docs.typesafe.ai/sitemap.xml`（可复现）。
- `/confidence` 页的 confidence 演示公式不在正文抽取文本中，需 `curl` 原始 HTML 后在 demo 说明文字里检索 `approximate confidence for three options` 才可见——**若后续有人复核此公式，请用这种方法**。
- `/cookbooks/parallel_questions` 的实测表格在页面中以文本形式渲染（非 `<table>` 标签），`curl` 后检索 `batching calls cost total time` 可直接看到三行原始数据。

