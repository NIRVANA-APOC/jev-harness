# 项目笔记：Jev 的定位与项目决策

> 本文件承载从 `AGENTS.md` 移出的细节内容；`AGENTS.md` 只保留项目目标、核心理念与技术栈要求。
> 完整调研见 [`docs/jev-research.md`](jev-research.md)，原始专题材料在 [`docs/research/jev/`](research/jev/)。

## Jev 的定位

Jev 是 TypeSafe AI 发布的 **System One 模型**（System One model）。它不是传统 LLM，不生成文本；它的输出是**带概率的类型化答案**。

调用方式是发送 **state**（上下文，可以是文本、结构化数据或消息序列）与 **questions**（针对该 state 的问题），返回每个问题的答案。问题类型有三类：

| 类型 | 语义 | 返回值 |
| --- | --- | --- |
| `Choice` | 从一组候选项中择一 | 每个选项的概率 + 整体置信度 |
| `Score` | 按有序等级评分 | 连续分数 + 分布 + 置信度 |
| `Noul` | 是非判断 | 该命题为真的概率 |

关键特性：

- **类型安全**：输出结构由调用方预先定义，模型只在该结构内作答，不会产生类型错误。
- **可校准的概率**：每个答案都附带置信度，代码可以按概率分支，而不是把模型输出再解析、校验一遍。
- **并行问题**：一次请求可针对同一 state 并行提问多个问题，增加问题几乎不增加响应时间。

Jev 与 LLM 在本项目中是**互补**关系，而不是替代关系：

- **LLM 负责推理与文本生成**——开放式思考、代码与自然语言产出、复杂问题的分解。
- **Jev 在 Agent 循环中承担快速结构化判定**——例如模型/工具路由、紧急度评估、动作门控（是否放行某个危险操作）、结果评分与校验。

设计上的直接结论是：**不要让每一次判断都回落到完整 LLM 调用**。凡是“在有限选项里选一个、打一个分、回答一个是非”的决策，都应优先交给 Jev，把 LLM 的调用留给真正需要生成与推理的环节。

参考资料：[TypeSafe AI: Introducing System One Models & Jev](https://typesafe.ai/blog/introducing-system-one-models-and-jev)、[LangChain: Building a Harness with Jev](https://www.langchain.com/blog/building-a-harness-with-jev)、[TypeSafe 文档 Quickstart](https://docs.typesafe.ai/introduction/quickstart)。厂商声称在分类类任务上比同类 LLM 快最多 200x、成本低 400x，属于官方口径，本项目不将其视为既定事实。

## 项目决策

> 以下决策优先于 `docs/jev-research.md` 第 13 节的默认建议；研究结论保留不改，供后续复核。

**2026-09-22**

1. **命名：保持私有**——本项目不发布到 crates.io（`jev-harness` 名称已被他人占用，不影响私有使用）；报告 §13.4 #5 与 §14.4 速查表中的名称冲突项到此关闭。
2. **供应商风险：接受**——接受 TypeSafe 当前无 SLA、单一供应商、限流动态调整；**暂不实现降级与容灾路径**（报告 §13.4 #6 的演练项延期，风险自担）。
3. **判定权：允许交给 Jev**——包括不可逆动作的门控场景；覆盖报告 §13.5 红线 1 中“必须先叠加确定性检查/人工确认”的默认立场。相关风险提示（提示注入可移动判定、校准无独立验证）仍见报告 §7.8 与 §4.4，供后续实测复核。
