# jev-harness

用 Rust 写的 Agent Harness：在同一个循环里同时使用 **LLM**（推理与文本生成）与 **Jev**
（TypeSafe 的 System One 模型：输入 state + 类型化 questions，返回带概率的类型化答案，不生成文本）。

个人学习项目，**仅供个人开发学习使用，不用于任何商业用途**。

| 读数（2026-09-25，`v0.1.0-rc.1`） | |
| --- | --- |
| 构建 | `cargo build --release`，空 target 目录全量 194 crate / 58 s / exit 0 |
| 测试 | `cargo test --workspace` → 29 个 test target、**666 passed / 0 failed / 7 ignored** |
| 静态检查 | `cargo clippy --workspace --all-targets` 0 告警、`cargo fmt --all -- --check` 干净 |
| 凭据卫生 | 工作区 / 运行产物 / 全部提交历史：**0 命中**（key 只经环境变量） |

---

## 1. 当前状态：`0.1.0-rc.1`（先读范围声明）

这是一份**预发布**，范围是**研究机制 + TUI 预览**。逐项核对见
[`docs/release-checklist-0.1.0-rc.md`](docs/release-checklist-0.1.0-rc.md)。

**包含**

- **研究机制 M0–M4 的实验核心**：五种可替换判断器（Jev 真调 / LLM / 规则 / 录制回放 / 模拟真值）、
  任务族 A/B/C、匹配循环与依赖失效传播、候选问题生成—验证—影子—晋升、观测能力建设与三臂对照、
  M4 综合评估，以及各自的 JSON 报告与离线端到端测试。
- **`jev chat` 的交互会话（TUI 预览）**：LLM 主循环 + Jev 判定门控 + 审批卡 + 工具执行 +
  三个研究面板（判断详情 / 事件与证据 / 失效传播）。
- **单次判定**：命令行 `jev ask`，以及会话内 `/jev`。
- **会话日志与会话历史**：13 类事件的 JSONL 契约（[`docs/session-log.md`](docs/session-log.md)）、
  `/sessions` 与 `/resume`。

**明确不含（未交付或未定）**

- **路线图 §7.1 #9 的联合演示**（同一次运行里既精化问题又建设观测能力）——M4 两批都没做，
  所以**不能宣称 1.0**。详见 [`docs/roadmap-1.0.md`](docs/roadmap-1.0.md) §7.1 与 §12。
- 路线图 §12 的未确定项（`Hypothesis` / `Capability` 是否做成一等对象、观测器库的入库文件形态、
  中文处理策略、批处理取舍、长 state 上限）与 §9 backlog 12 条：只登记，未实现。
- `docs/tui-design.md` §10 的 18 条未确定项（多形态渲染、主题热切换、语法高亮、Mermaid 与内联图片等）。
- 没有正式 `LICENSE` 文件（授权口径见 §8），没有 `rust-toolchain.toml` / MSRV 声明，
  没跑过 `cargo-audit` / `cargo-deny`。

---

## 2. 构建

```bash
cargo build --release          # 产物：target/release/jev
cargo test --workspace         # 离线全绿
cargo clippy --workspace --all-targets
```

- 需要 **Rust 1.85+**（edition 2024；开发机上是 1.97.1）。**没有固定工具链**，
  所以用的是你本机的 stable。
- 不需要任何凭据就能构建与跑测试：7 个 `#[ignore]` 是真调用例，只在显式要求时才跑
  （见 §6）。

## 3. 凭据（只经环境变量，不入库、不进日志与报告）

| 用途 | 变量 | 说明 |
| --- | --- | --- |
| LLM 通道 | `DEEPSEEK_API_KEY` | 缺省 provider 是 DeepSeek，只配它就能跑 |
| LLM 通道（可选） | `OPENAI_API_KEY` | `JEV_LLM_PROVIDER=openai` 时用它 |
| LLM 端点 / 模型（可选） | `OPENAI_BASE_URL`、`OPENAI_MODEL` | 覆盖预设；`custom` provider 下必填 |
| provider 选择（可选） | `JEV_LLM_PROVIDER` | `deepseek`（缺省）/ `openai` / `custom` |
| Jev 判定 | `TYPESAFE_API_KEY` | `--judge jev` 与 `TYPESAFE_BASE_URL`（可选） |
| 历史根目录（可选） | `JEV_HARNESS_HOME` | 缺省 `$HOME/.jev-harness` |

例如：

```bash
export DEEPSEEK_API_KEY=...        # 模型
export TYPESAFE_API_KEY=...        # Jev（不配也能跑：门控判不了就交人工裁决）
```

## 4. 三个入口

### 4.1 交互会话（TUI）

```bash
jev chat --workspace .                 # 缺省判定器 jev；--judge rule/llm 可换
```

| 键 | 作用 |
| --- | --- |
| `Enter` | 发送（补全列表开着且输入框只有一个裸查询时，先采纳高亮候选） |
| `Shift+Enter` / `Ctrl+J` | 换行（不支持 kitty 键盘协议的终端用 `Ctrl+J`） |
| `Esc` | 取消：先关最上面那层（选择 → 卡片 → 列表 → 取消本轮）；**空闲时什么都不做** |
| `Ctrl+C` / `/quit` | 退出 |
| `Ctrl+O` | 在会话视图与研究视图之间切换 |
| `PgUp` / `PgDn`、滚轮 | 翻回滚区 |
| 鼠标拖选 | 选文本；松开即把选中文本按 OSC 52 送进终端剪贴板（终端不支持时不生效） |
| `Ctrl+Shift+V` | 粘贴（粘贴本身是终端的行为；`Ctrl+V` 会提示各终端的正确键位） |
| `/` | 命令候选列表：`↑↓` 选择、`Tab` 采纳、`Enter` 也采纳 |

斜杠命令：`/help` `/status` `/model` `/permission` `/sessions` `/resume` `/continue` `/jev`
`/research` `/clear` `/quit`。

授权三模式（`/permission auto|ask|never`，缺省 `auto`）：白名单在所有模式下生效；
破坏性命令即使判定允许也上卡；`auto` 下只读工具不打扰、判定采纳「允许」才执行、判不了就上卡。

### 4.2 单次判定

```bash
jev ask --question "这段材料里用户是否明确要求执行 shell 命令？" \
        --label allow --describe "allow=材料里明确要求跑命令" \
        --label deny  --describe "deny=材料里没有执行命令的要求" \
        --material "请用 bash 跑 ls -1 看工作区。" \
        --out /tmp/ask.json
```

单问单答：**不采纳、不套门槛、不产生行动**，概率 / `margin` / `confidence` 都是原值；
报告含 `request_id`、`usage` 与复现命令行。会话里同一份口径是 `/jev <判定什么>`（缺省选项 `allow` / `deny`）。

### 4.3 研究臂

```bash
jev run --task A1 ...        # 单任务匹配循环（逐任务排查用）
jev exp ...                  # M0：初始任务集基线（最小的一条端到端）
jev compare ...              # M1：H1（失效传播开/关）× H2（增量/全量）四组臂
jev h3 ...                   # M2：固定问题集合 vs 允许精化（任务族 B）
jev h4 ...                   # M3：三臂对照（固定测试 / 通用测试生成 / 建设观测器）
jev m4 ...                   # M4：主对照 2×2 + 诊断对照 + 原生基线 + 两条残余消融 + 密度扫描
```

六个都是**研究臂**（不是日常命令）：各自的读数与结论见
[`docs/m4-report.md`](docs/m4-report.md)、[`docs/m4-freeze.md`](docs/m4-freeze.md)；
为什么全部保留见发布清单 §5.5。每个子命令的 `--help` 里有完整旗标。

## 5. 会话产物落在哪

缺省写 `~/.jev-harness/sessions/<run>/`（`JEV_HARNESS_HOME` 改根，`--out-dir` 改道；
取不到 `HOME` 时才退回 `var/chat/<run>`）：

| 文件 | 内容 |
| --- | --- |
| `session.jsonl` | 13 类事件（提交 / 模型 / 门控 / 审批 / 工具 / `/jev` / 打断 / 收尾…），契约见 [`docs/session-log.md`](docs/session-log.md) |
| `conversation.jsonl` | 模型可见的对话项，`/resume` 靠它接回上下文 |
| `evidence.jsonl` | 语义状态的证据日志 |
| `meta.json` | 摘要：工作区、模型、判定器、轮次、首末消息 |

**这些文件从「第一次真的开始」才创建**（首次提交，或首次 `/jev`）：只打开不用的 `jev chat`
在磁盘上不留任何东西；异常结束（`SIGTERM` / 关窗口）也能 `/resume`，因为开场摘要当场就写了。

## 6. 真调用例

7 个 `#[ignore]` 全是真调用例（3 个 OpenAI Responses、2 个 jh-app 的 LLM/Jev 真判、
1 个门控真判、1 个 jh-jev quickstart）：

```bash
cargo test --workspace -- --ignored        # 需要凭据与网络，会产生真实调用
```

## 7. 文档索引

| 文档 | 内容 |
| --- | --- |
| [`docs/roadmap-1.0.md`](docs/roadmap-1.0.md) | 路线图与 1.0 的完成定义（M0–M4、§7.1 九项交付物、§9 backlog、§12 未确定项） |
| [`docs/release-checklist-0.1.0-rc.md`](docs/release-checklist-0.1.0-rc.md) | 本次 rc 的门槛核对、发布前事项、可用性审计与 1.0 差距（**权威清单**） |
| [`docs/tui-design.md`](docs/tui-design.md) | TUI 设计：焦点模型、键位、三屏、组件、拖选与复制、未确定项 |
| [`docs/session-log.md`](docs/session-log.md) | 会话日志的格式契约与分析口径 |
| [`docs/m4-report.md`](docs/m4-report.md) / [`docs/m4-freeze.md`](docs/m4-freeze.md) | M4 读数与统计冻结 |
| [`docs/RESEARCH_GOALS.md`](docs/RESEARCH_GOALS.md) | 研究目标与红线 |
| [`docs/project-notes.md`](docs/project-notes.md) / [`docs/jev-research.md`](docs/jev-research.md) | 项目决策记录与 Jev 调研 |
| [`AGENTS.md`](AGENTS.md) | 协作约定：奥卡姆剃刀、不虚构、不夹带、一次提交只做一件事 |

## 8. 使用与授权

个人学习项目，**仅供个人开发学习使用，不用于任何商业用途**。
仓库还没有正式 `LICENSE` 文件——在那之前，请不要把它当作可再分发或可商用的软件
（这也是发布清单里 rc 之后的一项）。

## 9. 已知限制

- **1.0 不成立**：§7.1 #9 联合演示未交付（见 §1）。
- 默认不固定工具链；没有 MSRV 声明；没跑过供应链检查（`cargo-audit` / `cargo-deny`）。
- `docs/roadmap-1.0.md` §9 里 14 处 `.tower/comms/**` 引用指向仓库外的过程产物，仓库内打不开
  （该节已加口径说明）；可跟随的证据是 `docs/` 与 `crates/`。
- 几个已知但未修的静默失败与零测试文件（`h4.rs` 报告里的成本段、终端恢复失败、`main.rs` 的
  CLI 分支等）逐条登记在发布清单 §5。
