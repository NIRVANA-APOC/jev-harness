# 会话日志（真实使用场景分析用）

> **性质**：数据格式契约。写端在 `crates/jh-app/src/session/log.rs`，读端就是分析脚本（`jq` / Python）。
> 本文件只描述**已经落盘的东西**，不承诺未实现的字段。
> - 版本：1.0（2026-09-24 新增，随「会话历史 + 会话日志」批次）
> - 相关：会话目录布局与 `/resume` 见 `crates/jh-app/src/session/store.rs`；TUI 设计基准见 `docs/tui-design.md`

## 1. 文件位置

每个会话一个目录，**默认** `~/.jev-harness/sessions/<会话ID>/`（`JEV_HARNESS_HOME` 可覆盖根目录，
`jev chat --out-dir` 可整体改道）：

| 文件 | 内容 |
| --- | --- |
| `session.jsonl` | **本文件描述的对象**：一行一个事件 |
| `conversation.jsonl` | **完整对话记录**：用户消息、助手正文、工具调用与输出（`jh_openai::InputItem` 原样），`/resume` 靠它接回上下文 |
| `meta.json` | 摘要：时间、工作区、模型、判定器、轮数、首末句话 |
| `evidence.jsonl` | 语义状态证据（既有格式） |

会话 ID 就是运行标识（`h1h2-<Unix 毫秒>` 一类）；`/sessions` 列的就是这些目录。

## 2. 通用口径

- **追加写、一行一个 JSON 对象**，`kind` 判别标签；每行写完立即 flush。
- **会话目录从第一次「真的开始」才创建**（第一次提交，或第一次 `/jev`）：只打开不用的
  `jev chat` 在磁盘上不留任何东西（真调里攒过一堆空会话，2026-09-24 改成延迟落盘）。
- **长文本按 16 KiB 截断**，并保留原始字节数与标记：
  `{"text": "...", "bytes": 42123, "truncated": true}`。看到 `truncated: true` 就知道这只是前缀。
- **不含任何凭据**：key 只在环境变量里，日志从来不读、不写它。
- `turn` = 用户提交序号（从 1 起），`round` = 该轮内的模型调用序号（从 1 起）。
  `(turn, round)` 把模型 / 门控 / 审批 / 工具四类事件串在一起。
- 枚举值是**稳定词表**（见各 `kind` 的字段说明），分析脚本可以照它们分组。
- 写失败**不静默**：屏上或标准错误会出一句「会话日志没写上：…」。

## 3. 事件表

| `kind` | 何时写 | 关键字段 |
| --- | --- | --- |
| `session_start` | 进终端前 | `run` `version` `workspace` `judge` `model` `max_rounds` `debounce_ms` `color` `glyphs` `tty` |
| `user_message` | 用户提交 | `turn` `text`(bounded) |
| `command` | 宿主命令（`/model` `/permission` `/status` `/continue` `/sessions` `/resume`） | `name` `args` `scope` |
| `model_request` | 起一次模型调用 | `turn` `round` `model` `input_bytes` `items` `tools[]` |
| `model_response` | 调用返回（成功/失败都写） | `turn` `round` `model` `request_id` `latency_ms` `input_tokens` `output_tokens` `text`(bounded) `tool_calls[]` `error` |
| `gate_decision` | 门控有结论（含确定性层直接放行） | `key` `verdict` `source` `mode` `executor` `request_id` `latency_ms` `reused` `margin` `confidence` `choice` `probabilities` `thresholds` `failure` `reason` |
| `approval_request` | 审批卡弹出 | `turn` `round` `tool` `command` `body` |
| `approval_result` | 人工裁决或卡片被关掉 | `turn` `round` `tool` `decision` `waited_ms` |
| `tool_call` | 一次工具执行结束 | `turn` `round` `name` `arguments` `status` `output`(bounded) `latency_ms` |
| `error` | 显式失败 | `turn` `round` `stage` `text` |
| `interrupt` | 用户 `Esc` 取消 | `turn` `round` `stage` |
| `manual_judgment` | `/jev` 手工问了一次判定器 | `question` `labels` `material_bytes` `judge` `source` `version` `request_id` `latency_ms` `choice` `probabilities` `confidence` `margin` `error` |
| `session_end` | 退出会话 | `duration_ms` `turns` `model_calls` `tool_calls` `judgments` `judge_reuses` `refusals` `denials` |

词表：

- `gate_decision.verdict`：`allow`（采纳允许）/ `refuse`（采纳拒绝）/ `ask`（判不了，交人工）
- `gate_decision.source`：`real_api`（Jev）/ `llm` / `rule`（确定性层：只读工具、白名单、
  `never` 模式三条短路路径都记这个）/ `replay` / `simulated_truth`
- `gate_decision.mode`：`auto` / `ask` / `never`（当时的授权模式）
- `approval_result.decision`：`allow_once` / `allow_always` / `deny` / `cancel`（Esc 关掉没裁决）
- `tool_call.status`：`ok` / `failed`
- `error.stage`：`budget`（撞上轮内调用上限）/ `model`（模型不可用）/ `gate` / `tool` / `log`
- `interrupt.stage`：`model` / `gate` / `tool` / `approval` / `idle`

## 4. 示例（一次真实会话的骨架）

```jsonl
{"kind":"session_start","at_unix_ms":1790300000000,"run":"h1h2-1790300000000","version":"0.1.0","workspace":"/home/u/proj","judge":"jev","model":"deepseek-flash","max_rounds":32,"debounce_ms":30000,"color":"truecolor","glyphs":"fancy","tty":true}
{"kind":"user_message","at_unix_ms":1790300001200,"turn":1,"text":{"text":"看一下工作区","bytes":18,"truncated":false}}
{"kind":"model_request","at_unix_ms":1790300001201,"turn":1,"round":1,"model":"deepseek-flash","input_bytes":117,"items":1,"tools":["read_file","list_dir","search","write_file","edit_file","bash"]}
{"kind":"model_response","at_unix_ms":1790300002100,"turn":1,"round":1,"model":"deepseek-flash","request_id":"...","latency_ms":899,"input_tokens":798,"output_tokens":97,"text":{"text":"","bytes":0,"truncated":false},"tool_calls":[{"name":"bash","arguments":"{\"command\":\"ls -1\"}"}],"error":null}
{"kind":"gate_decision","at_unix_ms":1790300003400,"turn":1,"round":1,"key":"bash:ls -1","verdict":"allow","source":"real_api","mode":"auto","executor":"jev-1.13.0","request_id":"req_...","latency_ms":1259,"reused":false,"margin":0.98,"confidence":0.97,"choice":"usr:allow","probabilities":{"usr:allow":0.99,"usr:deny":0.01},"thresholds":{"margin":0.15,"confidence":0.5},"failure":null,"reason":"采纳 usr:allow 0.99"}
{"kind":"tool_call","at_unix_ms":1790300004500,"turn":1,"round":1,"name":"bash","arguments":"bash ls -1","status":"ok","output":{"text":"退出码 0\na.rs\nb.md\n","bytes":20,"truncated":false},"latency_ms":42}
{"kind":"session_end","at_unix_ms":1790300009100,"duration_ms":9100,"turns":1,"model_calls":2,"tool_calls":1,"judgments":1,"judge_reuses":0,"refusals":0,"denials":0}
```

## 5. 常见分析问法（`jq` 起步）

```bash
# 一次会话里每轮用了多少次模型调用、多少次工具、卡了多久
jq -r 'select(.kind=="model_request") | "\(.turn)\t\(.round)\t\(.input_bytes)"' session.jsonl

# 授权模式怎么被切换的（谁在用什么档）
jq -r 'select(.kind=="command" and .name=="permission") | "\(.args)"' session.jsonl

# 门控质量：判定与被采纳情况
jq -r 'select(.kind=="gate_decision") | [.mode,.source,.verdict,.reused,.latency_ms,.reason] | @tsv' session.jsonl

# 人工审批的等待与结果分布
jq -r 'select(.kind=="approval_result") | [.tool,.decision,.waited_ms] | @tsv' session.jsonl

# 失败在哪一步
jq -r 'select(.kind=="error" or (.kind=="model_response" and .error!=null)) | [.kind,.stage // "-",.turn,.round,.text // .error] | @tsv' session.jsonl

# 全部会话的用户输入（语料）
jq -r 'select(.kind=="user_message") | .text.text' ~/.jev-harness/sessions/*/session.jsonl
```

## 6. 未做 / 已知边界（照实登记）

1. **`/quit` 不单独记 `command` 事件**：它在界面层直接退出，不进宿主；`session_end` 覆盖了这次结束。
2. **界面自处理命令**（`/help` `/clear` `/research` `/quit`）目前只记 `/quit` 之外的那些吗？——不，
   当前只有宿主命令进日志：`/help` `/clear` `/research` **不写** `command` 事件（它们在 `jh-tui` 内部
   处理，没有回到宿主）。想补齐需要让界面把命令回执也交给宿主（登记为待办）。
3. **语义状态不跨进程恢复**：`/resume` 只接回对话上下文与界面回放；判断节点、依赖图、失效传播与
   授权白名单是进程内对象，1.0 不做重放。
4. `manual_judgment` 的概率是**原值**：`/jev` 不采纳、不套门槛、不产生行动，它只是把判定器的一次回答
   摆出来（`error` 有值表示这次没问成：判定器不可用或参数不合法）。
5. **不改写历史**：日志只有追加，没有轮转与压缩；会话多了要自己清 `~/.jev-harness/sessions/`
   （目录本身从第一次提交或第一次 `/jev` 才创建，空会话不会再自己冒出来）。
6. 会话时长取 `session_end.duration_ms`（进终端到退出终端的墙钟；不含启动前的装配时间）。
