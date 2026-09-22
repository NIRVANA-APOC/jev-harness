# 专题 08：提问工程与最佳实践

> 调研对象：TypeSafe AI 的 Jev（System One 模型）在 API 调用层面的提问设计、阈值/校准使用方法、state 组织与常见错误。
> 调研日期：2026-09-22。所有事实均附 URL；「官方口径」与「第三方来源」严格区分。
> 说明：本项目实现语言为 Rust，但 TypeSafe 官方只提供 Python / JS SDK 与 HTTP API；下文代码示例以官方 Python 与 JSON 报文形式给出，Rust 侧可直接对应到 JSON 报文字段。

---

## 0. 来源清单与可信度分级

| 级别 | 来源 | 说明 |
| --- | --- | --- |
| A 官方文档 | [docs.typesafe.ai/llms.txt](https://docs.typesafe.ai/llms.txt)（全站索引）、[Primitives](https://docs.typesafe.ai/primitives)、[Choice](https://docs.typesafe.ai/primitives/choice)、[Score](https://docs.typesafe.ai/primitives/score)、[Noul](https://docs.typesafe.ai/primitives/noul)、[Advanced: structure](https://docs.typesafe.ai/primitives/advanced)、[State](https://docs.typesafe.ai/concepts/state)、[Confidence](https://docs.typesafe.ai/confidence)、[How to build with TypeSafe](https://docs.typesafe.ai/concepts/how-to-build-with-system-one)、[Jev 1.13 jaggedness](https://docs.typesafe.ai/model-jaggedness/jev-1.13)、[Models](https://docs.typesafe.ai/models)、[Speculative fan-out](https://docs.typesafe.ai/patterns/fan-out) | 直接引用，数字均为官方自测 |
| A 官方 cookbook | [Parallel questions](https://docs.typesafe.ai/cookbooks/parallel_questions)、[Self-consistency: nouls](https://docs.typesafe.ai/cookbooks/consistency_noul_cookbook)、[Classifying RAG passages](https://docs.typesafe.ai/cookbooks/classifying_rag_passages)、[Re-ranking](https://docs.typesafe.ai/cookbooks/rerank_typesafe)、[Entity alignment](https://docs.typesafe.ai/cookbooks/entity_alignment) | 官方自测，含 pinned 数据集/revision 与模型版本 |
| B 第三方（可用） | [LangChain: What Is Jev?](https://www.langchain.com/blog/building-a-harness-with-jev)、[dev.to/valyuai 实操指南](https://dev.to/valyuai/how-to-use-jev-a-practical-guide-to-typesafes-system-one-model-g5e) | 用于交叉验证官方口径；LangChain 自身实现也基于官方 API |
| C 第三方（**不可用，勿引用**） | [SitePoint: Build a Safer AI Agent Harness with Jev and LangChain](https://www.sitepoint.com/build-safer-ai-agent-harness-jev-langchain/) | 见 §10.6：文中 `from jev import Decision`、`llm_model="gpt-4o-mini"` 并非 TypeSafe 的 Jev，是 LLM 结构化输出的包装器，会误导读者 |

---

## 1. 结论要点（TL;DR）

1. **提问工程的本质是「把判断拆到原子」+「把组合留在代码」**。官方给出的元规则是两条禁令：*不要问代码能精确计算的东西*；*不要在一个问题里藏多个判断*。（[jaggedness 页底部 Info 框](https://docs.typesafe.ai/model-jaggedness/jev-1.13)）
2. **一次请求尽量多问**。同一 state 上的所有问题并行评估、彼此独立；加问题几乎不增加耗时，只多花一点 token。官方把这叫 Speculative fan-out，并明确「一次可能用不到的问题几乎免费」。（[Primitives](https://docs.typesafe.ai/primitives)、[Speculative fan-out](https://docs.typesafe.ai/patterns/fan-out)）
3. **instructions 必须字面、直接、可执行**。Jev「读的是你写的字，不是你的意思」；否定、范围限定词、隐含条件都按字面处理。参考 state 中具体字段时用**反引号包住的 dot-and-index 路径**（如 `` `ticket.messages[0].text` ``）。**question id 不会发给模型**，所以问题必须完整写在 instructions 里。（[Primitives](https://docs.typesafe.ai/primitives)、[Choice](https://docs.typesafe.ai/primitives/choice)、[jaggedness](https://docs.typesafe.ai/model-jaggedness/jev-1.13)）
4. **选项/等级要「描述情境」，不要「描述程度」**。Score 每一级是被独立评估的，模型看不到级别编号和相邻级别，所以「比上一级更严重」毫无意义；只用数字当 level 会显著退化（官方实测：同一份报告，纯数字级 score 0.55 / confidence 0.33，改成描述性三级后 score 0.0 / confidence 1.0）。（[Score](https://docs.typesafe.ai/primitives/score)）
5. **阈值不是一个数，是「按动作风险的函数」**。官方示例：confidence < 0.5 一律转人工（地板）；只读操作 0.5 即可放行；动钱的操作要 > 0.9。（[Confidence](https://docs.typesafe.ai/confidence)）
6. **区分「confidence」与「概率」**：confidence 是概率分布**形状**的汇总统计量（越尖越高），不是「模型答对的概率」；Noul 没有 confidence。（[Confidence](https://docs.typesafe.ai/confidence)、[Noul](https://docs.typesafe.ai/primitives/noul)）
7. **state 只放这个问题需要的字段**。准确率随无关内容增多而下降（context rot）；检索与过滤要在代码里先做完。文本 only，不支持图像/音频/视频。（[State](https://docs.typesafe.ai/concepts/state)、[jaggedness](https://docs.typesafe.ai/model-jaggedness/jev-1.13)）
8. **最常见的坑**：把 Jev 当计算器（计数/日期/数学）、指望结构不变式（noul ≠ 1 − 取反 noul；Choice 概率 ≠ noul）、把用户可控内容当可信输入、以及**跨类型搬运阈值**。（[jaggedness](https://docs.typesafe.ai/model-jaggedness/jev-1.13)）

---

## 2. instructions 该怎么写

### 2.1 基本形态

- 每个问题有 `id`、`type`（`choice`/`score`/`noul`）、`instructions`；Choice/Score 还需要 `criteria`，Noul 的 `criteria` 可选。
- `id` 由你自选，**不会发送给模型**：「Write the complete question in `instructions`, even when the ID seems self-explanatory.」（[Primitives](https://docs.typesafe.ai/primitives)：*Question IDs are for your code. They are not sent to the model.*）
- `instructions` 可以写成**问句**，也可以写成**待判断的陈述句**。「'Does the message convey urgency?' 是好问题；'Analyze this message and determine the best course of action' 不是——那需要慢思考，是需要拆题的信号。」（[Primitives](https://docs.typesafe.ai/primitives)）

### 2.2 五条硬规则（均有官方出处）

1. **写得像给普通人看的一句话**。官方原话：*Aim for instructions which are easy for the average person to read and understand.*（[jaggedness §Contradictory instructions and criteria](https://docs.typesafe.ai/model-jaggedness/jev-1.13)）
2. **把精确条件写进 instructions**。发现自己在「解释你其实想说什么」时，那段解释就是 instruction 缺失的另一半；无法避免解释的地方，就拆成两个字面问题在代码里组合。（同上，§Literal reading）
3. **顺着语义方向问，不要反转**。Noul 中 `true` 映射到「否」会明显变差。应写成「Does the message contain personal data?」而不是「Is the message free of personal data?」（[Noul](https://docs.typesafe.ai/primitives/noul)、[jaggedness](https://docs.typesafe.ai/model-jaggedness/jev-1.13)）
4. **一个问题一个命题**。「Is the customer angry and asking for a refund?」这种双条件要拆成两个 Noul 再在代码里组合。（[Noul](https://docs.typesafe.ai/primitives/noul)）
5. **一个 Score 只量一个维度**。级别描述里出现「punctual and smart and experienced」就是在量三件事，得分会失去意义。（[Score](https://docs.typesafe.ai/primitives/score)）

### 2.3 用反引号路径指向 state 的具体字段

当 state 是 JSON 对象时，用**反引号包裹的 dot-and-index 路径**指名要判断的部分，模型据此知道该看哪里：

```python
questions = {
    "refund_requested": {
        "type": "noul",
        "instructions": "Does `ticket.messages[0].text` request a refund?",
    },
    "policy_supports_refund": {
        "type": "noul",
        "instructions": (
            "Does `refund_policy` support the refund requested "
            "in `ticket.messages[0].text`, given `order.charges`?"
        ),
    },
}
```

（出处：[Primitives §Reference specific fields](https://docs.typesafe.ai/primitives)。反引号是文档明确要求的写法；另一处表述为「including the backtick characters around each path inside the question」，见 [How to build §Use structure in the input state](https://docs.typesafe.ai/concepts/how-to-build-with-system-one)。）

### 2.4 结构化 instructions（对象/数组）

`instructions` 可以是 string / object / array / null（[Advanced: structure](https://docs.typesafe.ai/primitives/advanced)）。官方给的三条判断标准（[How to build §Use structure in the questions](https://docs.typesafe.ai/concepts/how-to-build-with-system-one)）：

- 问题需要上下文或示例；
- 问题的一部分由代码生成（把数据库取来的值放独立字段，而不是拼进字符串模板）；
- 多个问题的 instructions 结构相似（方便代码批量构造）。

官方示例结构（注意：字段名 `question`/`focus`/`what`/`not_for`/`examples` **都不是 API 保留字**，是自己选的，模型会看到名字和值，所以名字要短且能标签化）：

```json
{
  "type": "noul",
  "instructions": {
    "question": "Does the `message` ask the recipient to disclose a sensitive credential?",
    "inspect": "message",
    "focus": "Look for a request to send the credential itself, not a request to change or reset it."
  },
  "criteria": {
    "true": {
      "what": "Asks the recipient to reply with, type, or send a password, PIN, one-time code, or other security sensitive answer",
      "examples": ["Reply with your password", "Send us the 6-digit code you just received"]
    },
    "false": {
      "what": "No sensitive credential is requested",
      "examples": ["Reset your password from the settings page", "Your statement is ready"]
    }
  }
}
```

（出处：[Advanced: structure §Structured Noul criteria](https://docs.typesafe.ai/primitives/advanced)。）

**何时不该上结构**：官方明确说「短而无歧义的问题保持字符串即可，只有当归并在一起会互相模糊时才加结构」——*A short, unambiguous question or criterion can remain a string.*（[How to build](https://docs.typesafe.ai/concepts/how-to-build-with-system-one)）

---

## 3. 选项（Choice）与等级（Score）怎么设计

### 3.1 Choice

| 规则 | 内容 | 出处 |
| --- | --- | --- |
| 选项上限 | **最多 255 个选项**，每个选项只花几 token | [Choice §Good practice](https://docs.typesafe.ai/primitives/choice) |
| 给全量而不是精选 | *pass the full list of teams, categories, or products rather than a shortlist* | 同上 |
| 必须有兜底项 | 当列表可能覆盖不全时，加一个 `other` / `none of the above`，让模型能说「都不符合」，而不是硬选一个最接近的错误项 | 同上 |
| 描述要有区分度 | 「选项名和描述都会发给模型，所以描述要能把选项彼此分开」 | [Choice §Request structure](https://docs.typesafe.ai/primitives/choice) |
| 名称够清楚时描述可为 null | 官方示例 `criteria={"calm": None, "frustrated": None, "angry": None}` | [Choice §A more complex example](https://docs.typesafe.ai/primitives/choice) |

**选项互斥性问题的官方解法**：当两个选项总被混淆（官方例子：`return_policy` vs `return_status`），把每项描述从字符串升级为对象，字段用 `what` / `not_for` / `examples`，**跨选项使用同一套字段名**，让模型能做同类对比：

```json
"criteria": {
  "return_policy": {
    "what": "Whether and how an item can be returned",
    "not_for": "Progress of a return already sent",
    "examples": ["Can I return shoes I've worn once?", "How long do I have to return an order?"]
  },
  "return_status": {
    "what": "Progress of a return already sent",
    "not_for": "Whether and how an item can be returned",
    "examples": ["Has my return arrived yet?", "When will my refund be paid?"]
  }
}
```

（出处：[Choice §Structured instructions and criteria](https://docs.typesafe.ai/primitives/choice)。注意 `not_for` 字段就是专门用来做「互斥性」负向边界的；同样的四段式也用于 [How to build](https://docs.typesafe.ai/concepts/how-to-build-with-system-one) 的 `department` 示例。）

**深层分类法**：不要在一个 Choice 里塞整棵树，而是**逐层问一个 Choice，在代码里走树**（hierarchical classification）；当概率分散时用 beam search 保留 K 条候选路径，而不是贪心选一条。（[Choice §Good practice](https://docs.typesafe.ai/primitives/choice)、[Advanced: structure §Walking a taxonomy](https://docs.typesafe.ai/primitives/advanced)、[Hierarchical classification cookbook](https://docs.typesafe.ai/cookbooks/hierarchical_classification)）

### 3.2 Score

| 规则 | 内容 | 出处 |
| --- | --- | --- |
| 级别数量 | 至少 2 级，API 接受最多 10 级；「能描述得清楚多少级就用多少级，3 级就够；不要加你描述不清的级」 | [Score §Levels](https://docs.typesafe.ai/primitives/score) |
| 编号来源 | 数组下标，从 0 开始；模型**看不到编号，也看不到邻居** | 同上 |
| 返回形式 | `score` 是位置的**概率加权均值**，可落在两级之间（如 1.43）；同时给 `probabilities` 与 `confidence` | 同上 |
| 级别写法 | **描述情境，不要描述程度**：「Broken or degraded feature, but workaround exists」可用，「Moderately severe」不可用 | [Score §Writing good levels](https://docs.typesafe.ai/primitives/score) |
| 单一维度 | 一个 Score 只量一件事，多维请拆分成多个 Score 再用权重合并 | 同上 |
| 极端情形单独设级 | 如果量表顶端有个需要区别处理的稀有情形（如「辱骂或威胁」），给它单独一级，否则它和普通「非常生气」挤在同一档 | 同上 |
| 离散类别不要用 Score | 没有中间态、就是几个离散类别时，改用 Choice 或多个 Noul | 同上 |
| 必须在自己的数据上试措辞 | *Two wordings of the same scale can behave differently on your data.* | 同上 |

**「只用数字当 level」的官方实测对照**（同一份 bug 报告）：

```
instructions: "Rate severity from 0 to 2, where 2 is worst"
criteria: ["0", "1", "2"]
→ score 0.55, confidence 0.33, probabilities 0: 0.45, 1: 0.55, 2: 0.0
```

改用三级描述性 level 后：`score 0.0, confidence 1.0`。（出处：[Score §Writing good levels](https://docs.typesafe.ai/primitives/score)）

**结构化 level 的官方实测对照**（Safari 崩溃报告，同一份 state）：

| level 描述 | score | confidence |
| --- | --- | --- |
| 纯字符串，无 examples | 1.43 | 0.35 |
| 加了匹配的 example（"export fails in one browser but works in another"） | 1.03 | 0.96 |
| 加了不相关的 example（"search fails, but browsing categories still works"） | 1.43 | 0.35 |

官方对此的警告很关键：**高 confidence 不等于答案更对**——它只说明分布更尖。要用「已知期望级别」的样本挑 example，再在**另外的**输入上验证。（出处：[Score §Structured level descriptions](https://docs.typesafe.ai/primitives/score)）

另一个必须记住的点：**相同 score 可以有不同分布**。score 1.0 可能是全部概率在 level 1，也可能是 level 0 与 2 各一半；所以要同时读 `probabilities` 与 `confidence`。（[Score §Reading a Score](https://docs.typesafe.ai/primitives/score)）

### 3.3 Noul

- 返回单个 0–1 的数 `noul`，是「答案为真」的概率；**没有 confidence 字段**，因为二值分布用一个数就说完了。（[Noul §Reading a Noul](https://docs.typesafe.ai/primitives/noul)）
- **0.5 不等于「中等程度」**。官方明确区分：Noul 是「是/否」判断，Score 是「在谱上量位置」。「Is the candidate strong in Python?」的 0.5 只表示模型给了是/否相同概率，不表示技能中等；要量技能就用 Score。（[Primitives §Choose a question type](https://docs.typesafe.ai/primitives)、[Noul](https://docs.typesafe.ai/primitives/noul)）
- 官方对照表（同一批候选人的同一问题，Noul vs Score）：

| 候选人描述 | Noul「Is the candidate strong in Python?」 | Score「How much Python experience…」（4 级） |
| --- | --- | --- |
| 只会 Java/Go，没用过 Python | 0.03 | 0.0 |
| 偶尔写小脚本 | 0.14 | 1.0 |
| 上一份工作每天用 Python 两年 | 0.81 | 2.05 |
| 每天写 Python 八年，含大型 Django 代码库 | 0.92 | 2.89 |

（出处：[Noul §Reading a Noul](https://docs.typesafe.ai/primitives/noul)）

- 边界模糊时加 `criteria.true` / `criteria.false` 描述；但官方建议**两种都试**：「The instruction is enough for most Nouls, so try your questions with and without `criteria` and keep whichever gives better answers on your documents.」（[Noul §Writing a Noul question](https://docs.typesafe.ai/primitives/noul)）

---

## 4. 阈值与校准怎么用

### 4.1 confidence 是什么，不是什么

- `confidence` 是 **0–1 的统计量，由 `probabilities` 的分布形状算出**：全押一项 = 1.0，越平摊越低。官方原文：*The answer's `confidence` property collapses that shape into a single number from 0 to 1, so you can threshold on it without doing the math yourself.*（[Confidence](https://docs.typesafe.ai/confidence)）
- **官方没有公布确切公式**。文档里的交互 demo 只是「近似」：三选项时用 `(3 × largest probability − 1) / 2`（[Confidence 的 ConfidenceExplorer 组件说明](https://docs.typesafe.ai/confidence)）。官方明确说「你并不被我们的定义锁死，正因为如此我们才把完整的 `probabilities` 给你；不同计算方式的优劣是专门话题，我们会另写一份 cookbook，写好后再把链接加到这里」——**截至本次调研，该 cookbook 在 llms.txt 索引中不存在**（见 §10.3）。
- 「confidence 1.0 只描述模型的答案，不保证答案正确。」（[Score §Reading a Score](https://docs.typesafe.ai/primitives/score)）

### 4.2 官方推荐的三段式与风险分层

官方给的「有用起点」是把 confidence 分成三段，每段对应不同系统行为（[Confidence §Three paths](https://docs.typesafe.ai/confidence)）：

- **高**：自动执行；
- **中**：谨慎推进（让用户确认、标review、补信息）；
- **低**：不执行（转人工 / 要求澄清 / 回退到别的系统）。

并且**阈值不是一个数**，同一系统内不同动作的门槛不同，按「错了的代价」定：

```python
if confidence < 0.5:
    route_to_human(user_message)          # 地板：模型自己都说没把握
elif action.choice == "check_balance":
    show_balance(account_id)              # 只读、错了可恢复 → 低门槛
elif action.choice == "approve_transfer":
    if confidence > 0.9:
        confirm_then_execute(account_id)  # 动钱 → 高门槛
    else:
        ask_user_to_confirm(account_id)
```

官方同时警告：**正确阈值取决于你的领域与模型在你用例上的表现，从保守值开始，用自己的数据测，再看结果调整。**（[Confidence](https://docs.typesafe.ai/confidence)）

### 4.3 Noul 的双阈值（留出中间带）

Noul 没有 confidence，所以阈值要自己定，官方建议*按错误代价方向*调（[Noul §Reading a Noul](https://docs.typesafe.ai/primitives/noul)）：

- 是/否代价相当 → 0.5；
- **错误的「是」代价高**（派人到现场、退款）→ 提高；
- **漏掉真正的「是」代价高**（漏标安全问题）→ 降低；
- 中间值 → 转人工，而不是塞进任一分支。

官方代码示例用双阈值留出不确定带：

```python
YES = 0.8
NO = 0.2

if NO < wants_human < YES or NO < repeat < YES:
    send_to_review(message)   # 模型两边都不确定 → 交给人
    return
```

（出处：[Noul §Handling multiple Noul answers in code](https://docs.typesafe.ai/primitives/noul)）

### 4.4 cookbook 里出现过的具体阈值（务必当作「示例值」而非默认值）

| 场景 | 阈值 | 出处与语境 |
| --- | --- | --- |
| 14 问保险理赔 rubric，概率 → 动作 | `< 0.30` no；`0.30–0.70`（含端点）uncertain→人工；`> 0.70` yes | [Self-consistency: nouls](https://docs.typesafe.ai/cookbooks/consistency_noul_cookbook)。官方原话：*The band is illustrative; it is neither a calibrated guarantee nor an optimized threshold. Set production boundaries from labeled examples…* |
| RAG 段落过滤（每题一个 Noul） | 注入 `> 0.70` 排除；与前提冲突 `> 0.70` 归入冲突块；相关性 `< 0.45` 排除；有可用证据 `> 0.55` 纳入 | [Classifying RAG passages](https://docs.typesafe.ai/cookbooks/classifying_rag_passages)。官方原话：*We picked these four numbers for this corpus. Treat them as a starting point, not defaults.* |
| 工单 triage 复合评分 | `spam_risk` 在 `0.4 < x < 0.6` 视为不确定→转人工；`topic.confidence < 0.75` 转人工；`spam_risk >= 0.6` 隔离 | [How to build with TypeSafe §Putting it all together](https://docs.typesafe.ai/concepts/how-to-build-with-system-one) |
| 工单部门路由 | `department.confidence < 0.3` 转人工；`resolution.confidence < 0.5` 反问客户；第二部门概率 `> 0.25` 抄送 | [Choice §A more complex example](https://docs.typesafe.ai/primitives/choice) |

**「代码改一个数，比改一句 prompt 便宜」** 是同一条反复出现的官方设计主张：RAG cookbook 把四个阈值集中放在 `THRESHOLDS` 字典，「改路由策略是常量编辑（可走 code review），不是改问法」。（[Classifying RAG passages](https://docs.typesafe.ai/cookbooks/classifying_rag_passages)）

### 4.5 两种「不用阈值」的官方范式（对本项目尤其重要）

- **用概率排序，而不是设阈值**：re-ranking 场景中，把同一 Noul 问题对 shortlist 中每个候选各问一次，**直接按 `noul` 值排序**取最高。「That noul is the score the application sorts on.」（[Re-ranking](https://docs.typesafe.ai/cookbooks/rerank_typesafe)）
- **把等级直接写成动作，从根上去掉阈值**：实体对齐场景用一个三级 Score，三级就是三种处置（`leave unlinked` / `curator queue` / `assert sameAs`），代码只做「取最近级别」（`OUTCOME[min(int(score + 0.5), 2)]`）。官方原话：*There is no threshold to fit, and…the two cut points are not equally crowded…Neither number is something you tune. Both follow from how you worded the levels.*（[Entity alignment](https://docs.typesafe.ai/cookbooks/entity_alignment)）

这一条直接回答了本专题要求的「如何不过度依赖概率」：**把风险决策编码进选项/等级的语言，比事后调一个浮点阈值更稳、可审计、可 code review。**

### 4.6 跨类型搬运阈值是错的

jaggedness 页给了官方反例（同一工单 "I'm not happy with the fit. What are my options here?"）：

| Noul `noul` | Choice `yes` | Choice `no` | Choice `confidence` |
| --- | --- | --- | --- |
| 0.22 | 0.01 | 0.99 | 0.97 |

官方结论：**不要把一个在 Noul 上调好的阈值搬到 Choice 上**；Choice 是相对的（决定「哪一个」），每个 Noul 是绝对的（可以全都低）。（[jaggedness §Common-sense structural invariants](https://docs.typesafe.ai/model-jaggedness/jev-1.13)）

### 4.7 校准的可信度边界

- Jev 用 **RLCD（Reinforcement Learning for Calibrated Decisions）** 训练，目标是对齐结果而非人类偏好；因此「confidence 在总体上是有意义的：更高的 confidence 确实对应更高的准确率」——这是**官方/第三方转述的口径**（[Confidence 页与 dev.to 指南](https://dev.to/valyuai/how-to-use-jev-a-practical-guide-to-typesafes-system-one-model-g5e) 均如此描述）。
- 但**没有任何独立复现**。dev.to 指南把这一点写在最前面：*those are TypeSafe's own numbers, self-run and unreproduced.*（[dev.to/valyuai](https://dev.to/valyuai/how-to-use-jev-a-practical-guide-to-typesafes-system-one-model-g5e)）
- 官方在 jaggedness 页也说 Jev「extremely consistent」（语义相近输入会得到数量相近的输出），但**这不等于结构恒等式成立**（见 §7.7）。

---

## 5. 用多问题并行替代多次调用

### 5.1 机制

- 一次请求 = 一个 state + 任意多个问题；**每个问题独立评估、并行执行、彼此不构成隐含上下文**。加/删问题不影响其他问题的结果。（[Primitives](https://docs.typesafe.ai/primitives)）
- 官方成本/时延模型：**加问题几乎不改响应时间，只多花那几个问题的 token；「问一个你可能不需要的问题几乎是免费的」**。（[Primitives §Ask multiple questions together](https://docs.typesafe.ai/primitives)）
- context 预算：**64k tokens / 请求（state + 全部问题合计）**；**32k tokens 上限适用于 state + 单个最长问题**。（[Models](https://docs.typesafe.ai/models)）

### 5.2 官方实测：批一次 vs 分 13 次

[Parallel questions cookbook](https://docs.typesafe.ai/cookbooks/parallel_questions)（官方自测，`jev-1.12`，GDPR 维基条目 53,777 字符的 pinned revision，8 个 Noul + 2 个 Choice + 3 个 Score，各跑 5 次）：

```
batching                 calls        cost  total time
one call, all 13             1   $0.000497       0.27s
13 calls, one each          13   $0.006090       2.71s

batching: 12.2x cheaper, 10.0x faster
```

- **答案没有任何变化**：13 个问题中 11 个在两种批法下 5 次重复的**标准差恰为 0.0**；有余量噪声的两个（`breach_72h` std 0.0055、`criminal_penalties` 0.0045–0.0084）在两种批法下**噪声量级相同**，均值在噪声内一致。官方结论：*batching neither shifts the answer nor adds variance.*
- **注意数字口径不一致**：[primitives.md](https://docs.typesafe.ai/primitives) 同一 cookbook 写的是 **「11.5x cheaper and 9.6x faster」**，而 [cookbook 正文](https://docs.typesafe.ai/cookbooks/parallel_questions) 与 [llms.txt](https://docs.typesafe.ai/llms.txt) 摘要写 **12.2x / 10.0x**。引用时应以 cookbook 正文为准并注明存在双口径（见 §10.2）。
- cookbook 自己声明了该数字的适用条件：**文档主导型负载**（文档占每次请求的绝大部分），且「速度」是把 13 次单问的延迟**串行相加**得到的，并发发出后差距会缩小，「但 13x 的 token 成本不变」。

### 5.3 Speculative fan-out（推测式扇出）

官方推荐模式（[Speculative fan-out](https://docs.typesafe.ai/patterns/fan-out)、[How to build §Ask a lot of questions](https://docs.typesafe.ai/concepts/how-to-build-with-system-one)）：

> 把系统需要的所有问题放进一次请求，**包括只在部分输入下才有意义的问题**，让代码事后决定哪些答案有用。

官方 triage 示例：`bug_severity` 与 `has_reproducible_steps` 只在 category 是 bug_report 时才有意义，`refund_requested` 只在 billing 时有意义，但**照样一起问**；如果最后不是 bug report，代码直接忽略该答案。（[Speculative fan-out](https://docs.typesafe.ai/patterns/fan-out)）

### 5.4 什么时候**才**需要第二次请求

官方规则很硬（[Primitives §When one question depends on another](https://docs.typesafe.ai/primitives)）：

> 同一请求内的问题互相独立，一个答案不会成为另一个问题的上下文。只有当**代码必须先拿到第一个答案**才能构造第二个请求时（需要用它去取更多数据、决定 state 由什么构成、或决定下一题有哪些选项），才发第二次请求。

并且明确：*Two requests are the exception, not the rule.* 官方给出三个真正需要二次请求的 cookbook 作为边界案例：Skill suggestion（先从 182 个技能里排名，再取前三名全文重判）、Structure recovery（先判断换行是否断句、合并成块后再分类）、Hierarchical classification（每个 Choice 答案决定下一层选项）。

### 5.5 其他并行相关的官方口径

- [Re-ranking cookbook](https://docs.typesafe.ai/cookbooks/rerank_typesafe) 反过来强调**配对任务不能挤进一个请求**：1,200 次调用（40 query × 30 候选）共 1,536,002 input tokens，成本 `$0.0645`；官方提醒「本 walkthrough 为清晰起见每个 pair 只问一个问题，真实应用会把多个问题放进同一次调用」。
- [RAG cookbook](https://docs.typesafe.ai/cookbooks/classifying_rag_passages) 同样声明「每次请求针对一个段落，成本随 k 线性增长，因为每个问题都是关于一个 pair 的」。
- LangChain 的第三方文章把同一特性表述为「并行 + 加问题几乎不改响应时间」，与官方口径一致（[LangChain blog](https://www.langchain.com/blog/building-a-harness-with-jev)）。

---

## 6. state 的组织方式（文本 vs 结构化）

### 6.1 允许的形态

| 形态 | 适用 | 官方示例 |
| --- | --- | --- |
| 字符串 | 一条消息、一篇文章、一段话 | `"My card was charged twice."` |
| 对象 | 命名字段、相关记录、应用状态 | `{"message": "...", "order_id": "A-104"}` |
| 数组 | 消息或记录序列 | `["Hi", "My customer number is TS1337.", "My card was charged twice."]` |

（出处：[State §State can be a simple string or a structured JSON value](https://docs.typesafe.ai/concepts/state)。）

官方倾向：**大多数请求用对象**，让每部分有描述性名字、关系清楚；只有单一文本的简单场景才用字符串。

### 6.2 硬约束

- **只支持文本**。state 必须是字符串、JSON 对象或文本值数组；**不支持图像、音频、视频**。（[State](https://docs.typesafe.ai/concepts/state)、[Models](https://docs.typesafe.ai/models)）
- **主训练语言是英语**；其他语言（含 CJK）可用但**当前准确率较低**，官方要求在自己的内容上测，并在路由时特别注意 confidence。（[Models §Language support](https://docs.typesafe.ai/models)）——这条对本项目的中文场景是硬约束。
- 上下文：64k / 请求（state + 全部问题）；32k 适用于 state + 单个最长问题。（[Models](https://docs.typesafe.ai/models)）

### 6.3 组织原则（全部有官方出处）

1. **把 state 当成「你递给专家小组的材料」**：*Think of state as the material you would present to a panel of experts before asking them to make a judgment.*（[State](https://docs.typesafe.ai/concepts/state)）
2. **内容与问题分离**：state 放内容与支撑事实，questions 放要做的判断。官方例子：把退款请求与政策放进 state，再问「客户是否要求退款」「政策是否支持」。（[State §Separate content from questions](https://docs.typesafe.ai/concepts/state)）
3. **只放相关问题需要的字段**：这是官方「最重要概念」章节里的第 2 步（Decompose the input state）——*Include only the context relevant to the current questions. This helps the model avoid distractions and context rot.*（[How to build](https://docs.typesafe.ai/concepts/how-to-build-with-system-one)）
4. **结构化字段 + 反引号路径**引用具体嵌套值，能消除歧义（见 §2.3）。
5. **配对的判断要把两方放进同一个 state 的两个命名槽位**：官方实体对齐示例用 `entity_a` / `entity_b`，让问题问的是「这一对」，而不是任一侧；RAG 用 `query` + `passage`。（[Entity alignment](https://docs.typesafe.ai/cookbooks/entity_alignment)、[Classifying RAG passages](https://docs.typesafe.ai/cookbooks/classifying_rag_passages)）
6. **state 不是敌对环境**（jaggedness 第 6 条，原文标题即 §Adversarial content）：
   > *State is data, and `jev-1.13` does not treat it as hostile by default. Content written to adversarially steer the model…can move the answer.*
   > **Instead:** 在 criteria 里写清楚；上线前充分测边界情况。
   官方 RAG cookbook 进一步说明其注入过滤只是**过滤器，不是安全边界**：低于阈值的段落仍会进入 prompt，生成侧必须把每段都当不可信文本处理。（[jaggedness](https://docs.typesafe.ai/model-jaggedness/jev-1.13)、[Classifying RAG passages](https://docs.typesafe.ai/cookbooks/classifying_rag_passages)）

### 6.4 「检索在前，判断在后」

jaggedness 把这条列为独立失败模式（§Large state full of irrelevant detail）：

> 准确率随 state 中与决策无关的内容增长而下降…**Instead:** 先在代码里检索和过滤，只发问题需要的字段。

dev.to 指南把它总结为「两层结构：精确取回，廉价判断」，并引用了官方那句 *retrieve and filter in code first, and send only the fields the question needs.*（[dev.to/valyuai](https://dev.to/valyuai/how-to-use-jev-a-practical-guide-to-typesafes-system-one-model-g5e)、[jaggedness](https://docs.typesafe.ai/model-jaggedness/jev-1.13)）

**注意措辞的边界**：这属于**官方文档**的明确建议（jaggedness 与 How to build），但**没有给出量化曲线**（例如「多 10% 无关 token 掉多少准确率」）；dev.to 指南的「context rot is real」是转述官方，不是独立测量。

### 6.5 一个可复用的 state / question 组合示例（官方 triage 全文骨架）

官方给出的完整工单 triage 代码（[How to build §Putting it all together](https://docs.typesafe.ai/concepts/how-to-build-with-system-one)）演示了几条原则的合并用法：

- 先做确定性判断（`status == "closed"` 直接返回，不调模型）；
- 在代码里筛出 `open_orders` 再放进 state；
- state 分三块：`ticket` / `customer` / `policy`；
- 一次请求 7 个问题（3 Choice + 3 Noul + 1 Score），其中若干是推测性的；
- 用加权和合成 `spam_risk`（0.45 / 0.30 / 0.25），再按 confidence 与区间转人工；
- 最后按 `topic.choice` 分支，只在对应分支读取该分支需要的推测性答案。

---

## 7. 常见坑（官方 jaggedness 清单 + 我的归纳）

官方 jaggedness 页（[Jev 1.13 jaggedness](https://docs.typesafe.ai/model-jaggedness/jev-1.13)，标注 *Applies to `jev-1.13`. Last reviewed 2026-09-17*）列了 9 类失败模式，原文表格如下（我加了一列「与本项目的关系」）：

| # | 失败模式 | 官方建议 | 对本项目的意义 |
| --- | --- | --- | --- |
| 1 | 字面理解（Literal reading） | instructions 写精确条件，边界情况放进 criteria | 提问模板要 lint：禁止双重否定、禁止隐含条件 |
| 2 | 数学与数字 | 算术留在代码 | 计数/金额不要让 Jev 做 |
| 3 | 日期时间比较 | 拆出组成部分，在代码里比较 | 时间窗判断必须落到确定性代码 |
| 4 | 多层间接（Indirection） | 减少跳数；按名字指出相关 state | 问题不要问「某个属性的属性」 |
| 5 | 大 state 充满无关细节 | 先过滤，只发需要的字段 | harness 的上下文组织是本项目的核心职责 |
| 6 | 对抗性内容 | 精确写 criteria，上线前测边界 | state 中用户可控内容 = 我们的威胁模型 |
| 7 | instructions 与 criteria 矛盾 | 让 criteria 成为 instruction 的延伸 | 同上「提问模板 lint」 |
| 8 | 常识性结构不变式 | 每个决策只问一种方式；恒等式在代码里强制 | 见 §7.7 |
| 9 | 生成（Generation） | 用生成式模型 | Jev 只做选择/打分/判断 |

下面展开几条最容易踩的。

### 7.1 歧义问题 = 漏写了「你其实想说什么」

官方原话（§Literal reading）：
> Jev answers the question you wrote, not the one you meant. … When you look at a wrong answer and find yourself explaining what you really meant, that explanation is the missing half of the instruction.

**排查手法**：当你想为一个错误答案辩解时，把辩解写回 instruction。无法避免解释的，就拆成两个字面问题在代码里合成。

### 7.2 选项不互斥

- 症状：confidence 偏低、概率在两个选项间分摊。官方对这种低 confidence 的解读是：*low confidence on a Choice often means none of the options are a clear winner*；也可能意味着**「你的 criteria 写错了，而不是模型糊涂了」**（dev.to 指南对同一现象的表述）。
- 解法：把选项描述升级为 `what` / `not_for` / `examples` 结构（§3.1）；跨选项用同一套字段名。（[Choice §Structured instructions and criteria](https://docs.typesafe.ai/primitives/choice)；[Advanced: structure](https://docs.typesafe.ai/primitives/advanced) 称之为 *JSON rubric for boundary clarification*）

### 7.3 Score 等级不互斥 / 多维

官方给出的三条低 confidence 诊断（[Score §Reading a Score](https://docs.typesafe.ai/primitives/score)）：**级别在该输入上重叠**、**问题在量不止一件事**、**state 里信息不够定位**。对应的修复分别是重写级别、拆成多个 Score、补充 state。

### 7.4 把 Noul 当刻度用

见 §3.3。官方原文：*A Noul value runs from 0 to 1, but it's not a scale of the thing you asked about. It is the probability that the answer is yes.*

### 7.5 让 Jev 计数 —— 官方给的替代写法

官方建议：先在代码里迭代候选，每个候选用一个 Noul，然后自己加总：

```python
result = client.system_one(
    {"items": items},
    {f"item_{i}": Noul(instructions=f"Is `items[{i}]` the name of a fruit?")
     for i in range(len(items))},
)
count = sum(result.nouls[f"item_{i}"].noul > YES for i in range(len(items)))
# YES = 0.5（官方注释：阈值由你定，取决于用例）
```

（出处：[jaggedness §Counting](https://docs.typesafe.ai/model-jaggedness/jev-1.13)）注意：docs 页面是 `result.nouls[...]`，dev.to 指南写的是 `result.answers[...]`；**以官方文档为准**（见 §10.5）。

### 7.6 数值表示：语义 > 数值

官方实测口径：问十六进制/RGB 相近性不可靠；问「这个颜色读起来像不像警告」这种语义问题表现更好；高级语言比汇编/二进制指令好。建议是在代码里做转换，把算好的数或命名好的桶传进去。（[jaggedness §Numeric representations](https://docs.typesafe.ai/model-jaggedness/jev-1.13)）

### 7.7 不要指望结构不变式（这一条最容易在 harness 里埋雷）

jaggedness 给了两个官方反例：

- 同一工单上「客户是否在要求退款」：Noul = 0.22，而 Choice 为 `yes 0.01 / no 0.99 / confidence 0.97`。官方：*it is not obvious how to interpret either the Choice output and confidence for the Noul question or vice versa.*
- 同一工单上问「是否要求退款」(0.72) 与「是否在要求退款以外的别的什么」(0.47)，**和为 1.19**。

官方结论（对本项目的启示极强）：
> Don't carry a threshold tuned on a Noul over to a Choice, and don't hold the model to arithmetic identities between separate questions.

（出处：[jaggedness §Common-sense structural invariants](https://docs.typesafe.ai/model-jaggedness/jev-1.13)）

### 7.8 让 Jev 生成文本 / 生成数量

- Jev 不是为生成训练的；官方说明「你可以通过串接 Choice 强行让它生成，但效果差且非常慢」。（[jaggedness §Generation](https://docs.typesafe.ai/model-jaggedness/jev-1.13)）
- **不要让 Score 的期望值冒充精确数量**：官方明确说 score 的数值校准是弱项，不能靠两个 level 之间插值重建精确数值；只能用来**过阈值**。（[jaggedness §Math using score](https://docs.typesafe.ai/model-jaggedness/jev-1.13)）

### 7.9 过度依赖概率的三种具体表现（我的归纳，均已在上文给出官方依据）

1. **把 confidence 当准确率用**：官方明确 confidence 只描述分布形状，不保证正确（§4.1、§3.2 的 0.35→0.96 实测）。
2. **一个阈值打天下**：官方明确要求按动作风险分层（§4.2）。
3. **用阈值替代设计**：能用「等级即动作」的 Score 免掉阈值、能用排序替代阈值的场景，官方示例都优先这么做（§4.5）。

---

## 8. 关键事实与数据（每条附出处）

| 事实 / 数字 | 数值 | 语境与出处 |
| --- | --- | --- |
| 三个原语 | Choice / Score / Noul | [Primitives](https://docs.typesafe.ai/primitives) |
| Choice 选项上限 | **255** | [Choice](https://docs.typesafe.ai/primitives/choice) |
| Score 级数范围 | **2–10**（推荐 3 级起、能区分多少用多少） | [Score](https://docs.typesafe.ai/primitives/score) |
| Noul 返回值 | 0–1 的概率，无 confidence | [Noul](https://docs.typesafe.ai/primitives/noul) |
| 上下文预算 | **64k tokens/请求**（state+全部问题）；**32k**（state+单个最长问题） | [Models](https://docs.typesafe.ai/models) |
| 输入模态 | 纯文本（string / JSON object / 文本数组）；不支持图像/音频/视频 | [State](https://docs.typesafe.ai/concepts/state)、[Models](https://docs.typesafe.ai/models) |
| 语言支持 | 英文为主；其他语言含 CJK 可用但准确率偏低 | [Models §Language support](https://docs.typesafe.ai/models) |
| 13 问批一次 vs 分开问 | **12.2x 便宜、10.0x 快**（$0.000497/0.27s vs $0.006090/2.71s） | [Parallel questions](https://docs.typesafe.ai/cookbooks/parallel_questions)；条件：GDPR 维基 pinned revision 53,777 字符、`jev-1.12`、每条件 5 次重复、速度按串行相加 |
| 同一实验的**另一口径** | primitives 页写 **11.5x 便宜、9.6x 快** | [Primitives](https://docs.typesafe.ai/primitives)（与上一行冲突，见 §10.2） |
| 批量对答案的影响 | 13 问中 11 问两次批法下 5 次重复 std = **0.0**；两问有噪声但两种批法**噪声同级** | [Parallel questions](https://docs.typesafe.ai/cookbooks/parallel_questions) |
| 纯数字 level 的退化 | score 0.55 / confidence 0.33（概率 0.45/0.55/0.0）→ 改为描述性 level 后 **0.0 / 1.0** | [Score §Writing good levels](https://docs.typesafe.ai/primitives/score) |
| 结构化 level 的 example 效果 | 匹配的 example：confidence **0.35 → 0.96**（score 1.43→1.03）；不相关的 example：**回到 0.35** | [Score §Structured level descriptions](https://docs.typesafe.ai/primitives/score)；同一份 Safari 报告 state |
| Noul vs Score 不是刻度 | 同一候选人：noul 0.81 ↔ score 2.05（4 级制） | [Noul §Reading a Noul](https://docs.typesafe.ai/primitives/noul) |
| 结构不变式反例 | noul 0.22 vs Choice yes 0.01 / no 0.99 / conf 0.97；refund 0.72 + not_refund 0.47 = **1.19** | [jaggedness](https://docs.typesafe.ai/model-jaggedness/jev-1.13) |
| 自洽性（官方自测） | Jev 每题概率的**平均 std = 0.0102**，低于所有被比较的 LLM 条件（含 temperature 0）；`covered` 在 0.43–0.53 之间跨过 0.5 阈值 | [Self-consistency: nouls](https://docs.typesafe.ai/cookbooks/consistency_noul_cookbook)；14 问理赔 rubric × 15 次重复，`jev-latest`（返回 `jev-1.13.0`），2026-09-11 |
| 该实验的时延/成本对照 | TypeSafe **111ms / $0.000043** vs `claude-haiku-4-5` t=0 **1780ms / $0.001798** vs `gpt-5.5-reasoning` **11125ms / $0.033157** | 同上（注意：文中声明价格是历史价假设，非当前账单） |
| RAG 段落过滤阈值集 | injection 0.70 / contradicts 0.70 / relevant 0.45 / evidence 0.55；每 query 取 top-12，6 query 共 72 段，注入段落以 **0.99** 被拦下 | [Classifying RAG passages](https://docs.typesafe.ai/cookbooks/classifying_rag_passages)；`jev-1.12` + `claude-sonnet-5`，2026-08-27 |
| Re-ranking 提升 | top-1 **5%→18%**、top-5 **15%→35%**、top-10 **38%→62%** | [Re-ranking](https://docs.typesafe.ai/cookbooks/rerank_typesafe)；CLERC 3,565 段 / 40 query / BM25 shortlist 30，1,200 次调用共 1,536,002 input tokens = **$0.0645**，`jev-1.12` |
| 实体对齐（无阈值范式） | 450 对：**40 合并 / 50 转人工 / 360 不关联**；两处切点是 0.5 与 1.5，由 level 措辞决定 | [Entity alignment](https://docs.typesafe.ai/cookbooks/entity_alignment)；`jev-1.12`，2026-08-11 |
| confidence 近似公式（仅 demo） | 三选项时文档 demo 用 `(3 × 最大概率 − 1) / 2` 近似 | [Confidence 组件说明](https://docs.typesafe.ai/confidence) |
| 官方推荐的默认阈值示例 | confidence < 0.5 转人工；只读放行；转账 > 0.9 | [Confidence](https://docs.typesafe.ai/confidence) |
| Noul 双阈值示例 | `YES = 0.8` / `NO = 0.2`，中间带转人工 | [Noul](https://docs.typesafe.ai/primitives/noul) |
| 不确定带示例 | 0.30–0.70（含端点）→ uncertain → 人工；官方自注「仅示例、非校准保证、非最优」 | [Self-consistency: nouls](https://docs.typesafe.ai/cookbooks/consistency_noul_cookbook) |
| 费率与限流 | 250,000 tokens/秒；1,200 请求/分钟；超限 429 | [Models](https://docs.typesafe.ai/models) |
| 版本固定 | alias 会漂移；`jev-latest` 当时指向 `jev-1.13.0`；响应 `model` 字段回传实际版本，建议记录 | [Models §Aliases](https://docs.typesafe.ai/models) |

---

## 9. 请求-响应与代码示例

### 9.1 最小 HTTP 请求（JSON，语言中立，Rust 可直接对应）

```json
{
  "model": "jev-latest",
  "state": {
    "ticket_message": "My flight was cancelled. Can I get a refund?",
    "refund_policy": "Cancelled flights are eligible for a full refund."
  },
  "questions": {
    "refund_requested": {
      "type": "noul",
      "instructions": "Does `ticket_message` request a refund?"
    },
    "request_type": {
      "type": "choice",
      "instructions": "What is the main request in `ticket_message`?",
      "criteria": {
        "refund": "The customer wants money returned.",
        "rebooking": "The customer wants a replacement flight.",
        "information": "The customer is asking for information only."
      }
    },
    "frustration": {
      "type": "score",
      "instructions": "How frustrated does the customer appear in `ticket_message`?",
      "criteria": [
        "Calm and neutral.",
        "Concerned but civil.",
        "Very angry or using strong language."
      ]
    }
  }
}
```

（结构出处：[Primitives §Ask multiple questions together](https://docs.typesafe.ai/primitives)；端点 `POST https://api.typesafe.ai/v1/systemone`，[Models](https://docs.typesafe.ai/models)。）

### 9.2 响应形状（Choice 与 Score 各自的字段）

```json
{
  "model": "jev-1.13.0",
  "answers": {
    "department": {
      "type": "choice",
      "choice": "returns",
      "confidence": 1.0,
      "probabilities": { "shipping": 0.0, "returns": 1.0, "billing": 0.0 }
    },
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
    },
    "is_human_escalation": { "type": "noul", "noul": 0.99 }
  },
  "usage": { "input_tokens": 360, "output_tokens": 39 }
}
```

（出处：[Choice §Response structure](https://docs.typesafe.ai/primitives/choice)、[Score §Response structure](https://docs.typesafe.ai/primitives/score)、[Noul §Response structure](https://docs.typesafe.ai/primitives/noul)。`score` 的计算方式：各级编号 × 其概率之和，如 `0×0.0 + 1×0.57 + 2×0.43 = 1.43`；Python SDK 中 `probabilities`/`legend` 以**整数**级别为键，HTTP 报文中是字符串键。）

### 9.3 阈值按风险分层的官方参考实现

```python
action = response.answers["action"]
confidence = action.confidence

if confidence < 0.5:
    route_to_human(user_message)          # 地板
elif action.choice == "check_balance":
    show_balance(account_id)              # 只读，低门槛
elif action.choice == "approve_transfer":
    if confidence > 0.9:
        confirm_then_execute(account_id)  # 高风险，高门槛
    else:
        ask_user_to_confirm(account_id)   # 高风险，中置信 → 先确认
```

（出处：[Confidence §Thresholds scale with risk](https://docs.typesafe.ai/confidence)）

### 9.4 复合评分：拆维度 + 代码里加权（官方规范化做法）

关键细节：**不同长度的量表必须先归一化**（除以 `len(criteria) - 1`），否则权重没有意义。

```python
def normalized(answers, question_id: str) -> float:
    """Put a score on 0 to 1 by dividing by its top level number."""
    top_level = len(TRIAGE_QUESTIONS[question_id].criteria) - 1
    return answers[question_id].score / top_level

return 0.6 * severity + 0.3 * frustration + 0.1 * report_quality
```

（出处：[Score §Splitting a complex judgment into several Score questions](https://docs.typesafe.ai/primitives/score)）官方对该示例给出的计算：归一化后 0.62 / 0.64 / 1.0 → `0.6×0.62 + 0.3×0.64 + 0.1×1.0 = 0.664 ≈ 0.66`。

### 9.5 代码侧的组合权重（官方 triage 全文片段）

```python
spam_risk = (
    0.45 * answers["requests_credentials"].noul
    + 0.30 * answers["sender_identity_mismatch"].noul
    + 0.25 * answers["unexpected_reward"].noul
)
spam_is_uncertain = 0.4 < spam_risk < 0.6
if spam_is_uncertain or answers["topic"].confidence < 0.75:
    return route_to_human_review(ticket)
```

（出处：[How to build with TypeSafe §Putting it all together](https://docs.typesafe.ai/concepts/how-to-build-with-system-one)）

### 9.6 官方 meta-rule（可直接作为本项目提问模板的 lint 规则）

> **As a reminder, avoid the following:**
> - Asking the model something code can compute exactly.
> - Hiding several judgments inside one question.
> - System Two tasks: more layers of indirections
> - Giving it more context in `state` than the question needs. Jev suffers from context rot…

（出处：[jaggedness 页底部 Info 框](https://docs.typesafe.ai/model-jaggedness/jev-1.13)）

---

## 10. 不确定与未证实之处

### 10.1 所有性能数字都是厂商自测

dev.to 指南在开头就把警告写明：*Caveat up front…those are TypeSafe's own numbers, self-run and unreproduced.*（[dev.to/valyuai](https://dev.to/valyuai/how-to-use-jev-a-practical-guide-to-typesafes-system-one-model-g5e)）。官方 cookbook 也声明数字来自 pinned 数据集/缓存（`json_cache.json`），且部分价格是「历史价假设，非当前账单」。**本项目在技术决策里不应把这些数字当既定事实。**

### 10.2 同一实验存在两套比值

[primitives.md](https://docs.typesafe.ai/primitives) 说 13 问批处理 **11.5x 便宜 / 9.6x 快**；[cookbook 正文](https://docs.typesafe.ai/cookbooks/parallel_questions) 与 [llms.txt](https://docs.typesafe.ai/llms.txt) 说 **12.2x / 10.0x**。原因未说明，可能是不同版本/不同快照。**引用时以 cookbook 正文为准，并注明另有 11.5x/9.6x 口径。**

### 10.3 `confidence` 的确切公式未公开

官方只说它是「由概率分布算出」的统计量，demo 提供的是近似，且明确预告「不同计算方式的优劣会另写 cookbook，写好后再把链接加到这里」。截至 2026-09-22 的 llms.txt 索引中**没有**这份 cookbook。**结论：Rust 侧不要试图自己复算 confidence，应原样保存 API 返回的 `confidence` 与 `probabilities`。**

### 10.4 未找到公开信息的部分

- **校准曲线/可靠性图**：未找到官方的「confidence 分桶 vs 实际准确率」曲线；只有「RLCD 使概率具备总体意义」的定性说法与自洽性实验（重复实验测的是**稳定性**，不是**准确率**）。
- **官方提问模板/最佳实践 checklist**：官方没有单一的「prompting 最佳实践」页；本文的规则是从 primitives / how-to-build / jaggedness / cookbook 中归纳的。
- **异常输入处理**：state 为空、超长截断、非文本混入时的行为，官方文档未见明确说明（只有 token 上限与「文本 only」）。
- **temperature / 采样参数**：API 未见暴露采样参数；自洽性 cookbook 在 TypeSafe 侧**不传 temperature**（对比 LLM 侧显式设 t=0）。
- **多轮对话式追问**：官方立场是「问题之间独立、二次请求是例外」（§5.4），但没有多轮 state 复用的官方示例。
- **中文/CJK 的具体准确率数字**：只说「accepted but currently have lower accuracy」，**无量化**。

### 10.5 文档内部的小不一致（引用时注意）

- counting 示例的返回写法：官方 jaggedness 页用 `result.nouls[f"item_{i}"].noul`，[dev.to 指南](https://dev.to/valyuai/how-to-use-jev-a-practical-guide-to-typesafes-system-one-model-g5e) 写 `result.answers[...]`。**以官方文档的 SDK 形态为准**，实际字段名应在接入时以 SDK/API 参考核对。
- 自洽性 cookbook 的问题数量存在**两套口径**：[cookbook 正文与代码](https://docs.typesafe.ai/cookbooks/consistency_noul_cookbook) 写 *The rubric is 14 `Noul` questions*（`QUESTIONS` 字典实际 14 项），而 [llms.txt 摘要](https://docs.typesafe.ai/llms.txt) 与 [Noul 页的 cookbook 列表](https://docs.typesafe.ai/primitives/noul) 都说「15-question rubric」。重复次数两边一致，均为 **15 次**。引用时建议写「约 14 问 / 15 次重复」，或注明口径分歧。

### 10.6 一个应避免引用的第三方来源（重要）

SitePoint 的 [Build a Safer AI Agent Harness with Jev and LangChain](https://www.sitepoint.com/build-safer-ai-agent-harness-jev-langchain/)（2026-09-18）虽然标题含 Jev，但正文使用的是 `from jev import Decision`、`decision.run()`、`llm_model="gpt-4o-mini"`、Pydantic schema 约束的 **LLM 结构化输出**，与 TypeSafe 的 Jev API（`state` + `questions` + Choice/Score/Noul）**完全不是一回事**，文中也没有任何 TypeSafe 端点、模型名或概率概念。该文作者自己在文末也提示「如果 `jev` 包不可用，可用 `instructor`/`marvin` 等替代」，说明它描述的是一个通用模式而非 TypeSafe 产品。**本项目不应引用此文作为 Jev 资料。**

### 10.7 与官方口径一致的第三方验证点

以下三点在官方文档与第三方来源之间**互相印证**（可直接引用，但仍属厂商口径的转述）：

1. 一次请求多问题、并行、加问题几乎不增时延（[LangChain blog](https://www.langchain.com/blog/building-a-harness-with-jev) ↔ [Primitives](https://docs.typesafe.ai/primitives)）。
2. Jev 不是 LLM 替代品，而是 agent 循环里的互补决策层；LLM 负责开放式推理与生成（[LangChain blog](https://www.langchain.com/blog/building-a-harness-with-jev) ↔ [How to build](https://docs.typesafe.ai/concepts/how-to-build-with-system-one)）。
3. 「200x 更快 / 400x 更便宜」是**厂商声称**，两个第三方来源都明确标注为不可复现的自报数字（[LangChain blog](https://www.langchain.com/blog/building-a-harness-with-jev)、[dev.to/valyuai](https://dev.to/valyuai/how-to-use-jev-a-practical-guide-to-typesafes-system-one-model-g5e)）。

---

## 11. 对本项目 Jev 定位的启示（3 条）

### 启示 1：把「问题定义」和「阈值/组合」分层建模，前者进类型，后者留在代码

官方反复强调的设计主张是**代码拥有控制流，模型只提供窄而结构化的判断**（[How to build](https://docs.typesafe.ai/concepts/how-to-build-with-system-one)），并且**阈值是常量、不是 prompt**（[RAG cookbook 的 `THRESHOLDS` 字典](https://docs.typesafe.ai/cookbooks/classifying_rag_passages)）。这与本仓库 AGENTS.md 的「决策分层」原则一致。

落到 Rust：`Choice` / `Score` / `Noul` 三种问题应成为类型（`Score` 的级数、`Choice` 的选项集合适合用常量泛型或枚举承载，并让「level 越界」「选项为空」在编译期或构造期就不可表达）；而**阈值、权重、动作映射应放在普通代码/配置里，可 code review、可单元测试**。另外两条硬约束要写进类型或接入层：

- `confidence` **原样保存 API 返回值**，不要自行复算（§10.3 公式未公开）；
- **禁止跨类型搬运阈值**，也不要假设任何结构恒等式（§4.6、§7.7）。

### 启示 2：harness 的内层循环默认应是「一次多问」，而不是「一次一问」

官方明确把「一次一问」标记为 coding agent 的坏习惯：*Coding agents fall into the one question per call habit more than people do.*（[Primitives](https://docs.typesafe.ai/primitives)），并用 Speculative Fan-out 把「可能用不到的问题」也一起问（[Speculative fan-out](https://docs.typesafe.ai/patterns/fan-out)）。

对本项目意味着：工具路由、动作门控、结果评分这类循环内判定，应设计成**「组装一次 state → 发出 N 个问题 → 代码按分支取用」**的批量形态；只有「必须拿到上一答案才能构造下一请求」时才允许第二次调用（§5.4）。相应地，harness 需要有能力对**推测性问题做结果过滤**（拿不到需要的信息就忽略），以及**按 token 预算做 state 打包**（64k/请求、32k state+最长问题，§5.1）。

### 启示 3：概率与置信度作为一等数据传递，但门控策略要「按动作风险分层 + 用自己的数据校准」

本仓库 AGENTS.md 已定下「概率优先，不要过早塌缩成布尔值」；官方文档给出了可直接落地的配套约束：

- 置信度分三段（高用于自动执行 / 中用于谨慎推进 / 低用于不执行），且**同一系统内不同动作阈值不同**（[Confidence](https://docs.typesafe.ai/confidence)）；
- Noul 没有 confidence，要用**双阈值留中间带**转人工（[Noul](https://docs.typesafe.ai/primitives/noul)）；
- 能用「等级即动作」（如实体对齐的 Score）或「按概率排序」（如 re-ranking）的地方，**优先不引入阈值**（§4.5）；
- 所有阈值都必须在**本项目自己的流量**上标定——官方自己反复声明其 cookbook 的阈值只是「该语料上的起点」，且「正确阈值取决于你的领域与模型在你用例上的表现」（§4.4）。

**一个必须提前写进设计的风险**：Jev 的主训练语言是英文，CJK 目前准确率偏低且官方未给量化（§6.2、§10.4）。因此若 harness 要处理中文输入，宁可**把判定问题与 state 用英文表述**（或做英译后再判定），并在路由时对低 confidence 保持更高的人工兜底比例——这一点应作为待验证假设，而不是已经成立的结论。
