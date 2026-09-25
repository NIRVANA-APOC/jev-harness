# 0.1.0-rc 发布清单与评估（2026-09-24）

> **性质**：发布前的门槛清单与现状核对。判据分两层——
> **① `0.1.0-rc` 门槛**（本文件定的、可辩护的工程门槛）与 **② 路线图的 `1.0` 完成定义**
> （[`docs/roadmap-1.0.md`](roadmap-1.0.md) §7.2：M0–M4 全达成 + §7.1 九项交付物）。
> 两者**不是一回事**：rc 是「这份代码可以打标签、给人试用、限制写在明处」，
> 1.0 是「路线图自认的交付物全部达成」。当前前者差几项可以立刻做完的事，后者**不成立**。
>
> 读数口径：本文数字都在 2026-09-24 的工作树（`main` @ `ecfe282` + 未提交改动）上实测。
> 独立复核：工程质量审计（只读、外部 target 目录、含 103 个提交的密钥历史扫描）与
> TUI 可用性审计（伪终端真机）各一轮，发现逐条并入 §4 / §5。

## 1. 结论

| 问题 | 结论 |
| --- | --- |
| 现在能发 `0.1.0-rc` 吗？ | **不能直接发**：质量门、功能面、密钥卫生都已经够；两轮独立审计发现的缺陷与用户拍板的三项（空闲 `Esc` 语义、`Enter` 补全语义、列表选中标记）连同 `jev ask` 实现**本轮全部落地并真机验过**（§4 共 17 行）。剩下的是 §3 的发布前事项，头一条（工作树未提交）是硬伤 |
| 做完 §3 之后能发吗？ | **可以**，前提是把 rc 的范围写清楚：**研究机制（M0–M4 的实验核心）+ TUI 预览**，并明确「不含路线图 §7.1 #9 的联合演示」 |
| 能宣称 `1.0` 吗？ | **不能**。路线图自己登记 §7.1 #9（反例 → 新维度/观测能力 → 未见任务效果的联合演示）**未交付**；另有 §12 未确定项与 §9 backlog 在案 |

## 2. 门槛逐项核对

| # | 门槛 | 状态 | 实测证据 |
| --- | --- | --- | --- |
| 1 | `cargo fmt --all -- --check` | ✅ | 干净 |
| 2 | `cargo clippy --workspace --all-targets` | ✅ | **0 告警**（复核时在隔离 target 目录全新编译 194 个 crate 跑过一遍，41 s，排除缓存假象） |
| 3 | `cargo test --workspace` | ✅ | 29 个 test target，**666 passed / 0 failed / 7 ignored**；按 crate `jh-tui` 271 / `jh-app` 235 / `jh-core` 92 / `jh-openai` 41 / `jh-jev` 27 |
| 4 | `#[ignore]` 是诚实的 | ✅ | 7 条**全是真调**：3 条 OpenAI Responses、2 条 jh-app 的 LLM/Jev 真判、1 条 gate 真判、1 条 jh-jev quickstart——没有一条是逃避测试 |
| 5 | release 构建 | ✅ | 仓库内 35 s；**空 target 目录全量 194 crate 58 s，exit 0**（构建可重现） |
| 6 | CLI 自描述 | ✅ | `jev --version` → `jev 0.1.0`；`jev --help` 列 8 个子命令；`jev chat --help` 旗标与默认值完整（含 `--out-dir` 缺省 `~/.jev-harness/sessions/<run>`） |
| 7 | 离线端到端可用 | ✅ | `jev exp --judge rule --out …` 退出码 0，报告含 `run` / `command` / `task_set` / `metric_definitions` / `judges[]` / `calls_log` / `notes`，`live=false` 如实标注 |
| 8 | 五个 crate 版本一致 | ✅ | 全部 `0.1.0`，edition 全 `2024` |
| 9 | 仓库 + 运行产物 + **全历史**无密钥 | ✅ | 工作区 0 命中、`var/` 与 `~/.jev-harness/` 0 命中、**103 个提交逐提交扫描 0 命中**；2 个高熵候选经逐行核对是 `std::env::var(...)` 表达式 |
| 10 | 占位与 panic 残留 | ✅ | 生产代码 `todo!` / `unimplemented!` / `TODO` / `FIXME` / `panic!` / 真实 `unwrap()` **全为 0**；唯一 `unreachable!` 有守卫；35 处 `.expect()` 全是不变式断言 |
| 11 | 运行产物不入库 | ✅ | `.gitignore` 覆盖 `/reference/`、`target/`、`.env`、`var/`；`Cargo.lock` 已跟踪（但**仍被 `.gitignore` 列出**——自相矛盾，见 §3 #5） |
| 12 | README / CHANGELOG / LICENSE | ❌ | 三者都不存在；5 个 crate 全无 `license` / `repository` 元数据，`jh-core` / `jh-jev` / `jh-app` 连 `description` 都没有 |
| 13 | 工作树干净、可从提交切 rc | ❌ | **38 项未提交**（28 改 + 10 新）；`provider.rs`、`session/log.rs`、`session/store.rs`、`commands.rs`、`markdown.rs` 这 5 个新源文件**不在任何提交里**——HEAD 上没有批次六的任何功能 |
| 14 | 无标签 | ❌ | `git tag` 为空（需 `v0.1.0-rc.1`） |
| 15 | 文档与实现一致 | ⚠️ 已大幅收敛 | 本轮修了 9 处漂移（§4）；残余见 §3 #4 |
| 16 | 已知限制如实登记 | ✅ | `docs/tui-design.md` §10（18 条未确定项）、`roadmap-1.0.md` §9（backlog 12 条）、`docs/m4-report.md` §11（限制 11 条）+ 本文件 §5 |
| 17 | 外部引用可达 | ⚠️ | roadmap 里 14 处 `.tower/comms/**` 引用指向**仓库外**的文件（工作区没有 `.tower/`）；已加口径说明，处置见 §3 #4 |
| 18 | MSRV / 工具链固定 | ⚠️ | 无 `rust-toolchain.toml`、无 `rust-version`；本地 rustc 1.97.1，edition 2024 隐含 ≥1.85 |
| 19 | 供应链检查 | ⚠️ | 无 `cargo-deny` / `cargo-audit` / 配置文件（只是没装）；`cargo tree -d` 3 组重复版本，全是传递依赖、本仓改不动 |

## 3. 发布前事项（按优先级）

1. **提交这 38 项改动**，再打 `v0.1.0-rc.1`。建议按主题拆（AGENTS.md「一次提交只做一件事」）：
   `feat(provider)` / `feat(tui): 命令补全与鼠标` / `fix(tui): 换行键退路` / `fix(session): 调用上限不再硬截断` /
   `feat(session): 权限三模式` / `feat(tools): 写与编辑文件` / `feat(session): 会话日志与历史 + /resume` /
   `feat(tui): 命令选项列表` / `feat(tui): markdown 管道表格` / `feat(tui): 文本选择与复制（OSC 52）` /
   `docs: 批次六回写 + 发布清单`。
2. **写 `README.md`**：这是什么、怎么构建、怎么配 key（`DEEPSEEK_API_KEY` / `TYPESAFE_API_KEY`，`JEV_LLM_PROVIDER` 预设）、
   怎么跑（`jev chat` / `exp` / `run` / `compare` / `h3` / `h4` / `m4`）、文档索引、已知边界（含 `ask` 未实现）。
3. **写 `CHANGELOG.md`（或 release note）**：0.1.0-rc 的**范围声明**——研究机制 + TUI 预览；
   明确列出**不含** §7.1 #9 联合演示、M5 全部内容、`docs/tui-design.md` §10 的未确定项。
4. **`.tower/comms/**` 的 14 处引用**：入库，或改指到入库位置（roadmap §9 已有口径说明，但引用仍不可达）。
   ~~`jev ask` 空壳~~、~~`Enter` 补全语义~~、~~列表选中标记~~ 三项已按用户口径落地（见 §4 #15–#17）；
   - `jev ask` 空壳：`--help` 宣传「单次 Jev 判定」，实为 `eprintln!("未实现")` + 退 2，且零测试。要么实现（一行 `jh_jev::Client::from_env()?.system_one(...)`），要么 `#[command(hide = true)]` 藏起来；
   - `.tower/comms/**` 的 14 处引用：入库，或改指到入库位置（roadmap §9 已有口径说明，但引用仍不可达）。
5. **顺手可清的卫生问题**：`.gitignore` 里的 `Cargo.lock` 条目（它已入库，留着只误导）；
   3 项零引用依赖（`tracing-subscriber` / `dotenvy` / `toml`）；给 5 个 crate 补 `description` / `license`。
6. **LICENSE**：项目自述「个人学习、不商用」，rc 至少要有一句授权口径（`LICENSE` 或 README 里一段）。

## 4. 本轮已修（评估过程中发现并就地改掉的）

| # | 位置 | 原样 | 改成 |
| --- | --- | --- | --- |
| 1 | `crates/jh-tui/src/lib.rs` | 「不做：…**选择/剪贴板**…」，而同 crate 已实现 `Selection` / `copy_to_clipboard`，自己的设计文档 §7.5 标题就是它 | 从「不做」删掉，并把 markdown / 命令注册表 / 拖选 / 选项列表 / OSC 52 写进「做」清单 |
| 2 | `session/store.rs` + `session/agent.rs` | `meta.json` 损坏的会话被 `.ok()` **静默丢弃**（`/resume` 里会话凭空消失），而同 store 的 `read_conversation` 却显式报错 | `list()` 改回 `SessionList { metas, skipped }`，宿主**如实上屏**「有 N 个会话读不出摘要」；补测试 `a_session_with_a_broken_summary_is_reported_not_dropped` |
| 3 | `jh-tui/src/terminal.rs` | `copy_to_clipboard` 在任何测试里都没被调用，OSC 52 字节**零断言**（批次六唯一没有自动化验证的点） | 抽出 `write_osc52(&mut impl Write, …)` 接缝 + 测试断言 `\x1b]52;c;5L2g5aW9\x07`（含空文本） |
| 4 | `tui-design.md` §9.1 口径 1 / 2 | 「只读工具与 `bash` 都上卡」「`usr:allow` 照常上卡」——与同一文件 §5.4 的三种模式**自相矛盾** | 按授权模式改写，指向 §5.4；补「两个方向都卡门槛」「破坏性即使允许也上卡」 |
| 5 | `tui-design.md` §9.1 验收读数 | 单元 141 / 快照 28 / 会话级 35 | 2026-09-24 重测：29 target / 652 passed，按 crate 与 target 逐项列出 |
| 6 | `tui-design.md` §10 非目标 2 | 把「选择与剪贴板」列为非目标 | 记为**已交付**（批次六），并新增 12–14 条未确定项（选择只有鼠标、选项列表无点击、回退字形档不降级） |
| 7 | `tui-design.md` §9 | 没有批次六的条目 | 新增批次六行 + §9.2「批次六逐条回写」（12 项能力 / 落点 / 口径与边界 + 验收读数 + 真机验证清单） |
| 8 | `roadmap-1.0.md` §1.3 #4 / §1.3 #6 / §3.1 / §5 M4 / §7.2 | 5 处 `jh-tui` 行号全漂移；3 处写「会话产物落 `var/chat/<run>/`」（实际缺省 `~/.jev-harness/sessions/<run>/`，`var/chat` 只是取不到 `HOME` 的退路）；测试基线 512 | 行号按尖端重测；路径按缺省 + 改道 + 退路三层写清；基线改 29 target / 652 passed 并给按 crate 分解 |
| 9 | `roadmap-1.0.md` §9 | 14 处 `.tower/comms/**` 引用没有说明，读者照着路径打不开 | 加「引用口径」段：那是外部工作流产物、不在仓库内；可跟随的证据是 `docs/` 与 `crates/` |
| 10 | `session/store.rs` | 本地文件读不出来却报**「判断器不可用：会话摘要不可读」**（可用性审计的探针实测：`/resume` 给一个不存在的会话 ID 时的回执） | 改用 `Error::Io`（前缀「本地读写失败」），路径与原因留在消息里；补测试 `local_file_failures_do_not_blame_the_judge` |
| 11 | `session/agent.rs` `on_approval` | **批准了却不执行**（可用性审计的阻断项 B1）：判定器判不了（`Ask`）→ 弹卡 → 按「允许一次」→ 那个判断节点是 `Failed`、按定义不可依赖 → 前提复核挡下 → 界面回到同一张卡、模型反复重发。`--judge rule` 或没配 Jev 凭据时**必现**，唯一出路是 `/permission never` | 批准时只在节点**确实可依赖**时才把它当行动前提，否则以人工裁决为据（并上屏一句「这次调用没有可依赖的判定依据：按你的人工裁决执行」）。回归测试逐字复现了旧提示「这次调用的判定依据已失效（bash#1）」，修复后 `--judge rule` 真机跑通 `▸ bash echo ok-b1` → `退出码 0` |
| 12 | `session/mod.rs` `chat()` | **异常结束（SIGHUP / 关窗口）后 `/resume` 接不回来**（可用性审计的阻断项 B3）：`meta.json` 只在正常退出时写，崩溃路径下落盘目录里只有 `conversation.jsonl` 等三份文件 | 会话**开场就写一份摘要**（正常退出时用最终读数覆盖）。真机验：SIGHUP 后目录里 `meta.json` 在位，新会话 `/resume` 成功 |
| 13 | `session/agent.rs` `/permission` 分支 | 切了模式但**状态行仍写旧模式**（可用性审计的摩擦项 F1，安全相关）：`never` 下工具直接跑，底边还写着「自动（Jev 判定）」 | 分支末尾推 `Update::PromptInfo`。真机验：`/permission never` 后底边变 `从不询问` |
| 14 | `app.rs` Esc 阶梯 | 键位栏写「Esc 取消」，而空闲时 Esc **直接退出会话**（草稿一起丢）——文案与行为不一致（可用性审计的阻断项 B2） | **按用户口径改行为**：空闲 `Esc` 什么都不做，退出只走 `Ctrl+C` / `/quit`；栏位文案回到「取消」（字面意思）；阶梯补一条「有选择 → 先清选择」，`tui-design.md` §7.1 同步
| 15 | `app.rs` / `render.rs` / `render.rs` | `Enter` 不采纳补全（提交裸 `/` → 「未知命令」）；列表选中行与输入框同用 `❯`（F2/F3） | **按用户口径落地**：补全列表开着且输入框只有裸查询时 `Enter` 采纳候选（输入已等于选中候选完整名字、或已进参数区时照旧提交）；两个列表的选中标记改 `▸`（带一列空格），快照同步
| 16 | `session/**`（`agent.rs` / `mod.rs`）+ `jh-core/src/evidence.rs` | **开着不用也建会话目录**：启动即 `create_dir_all` + 两个日志 `open` + `JsonlEvidenceLog::open` 的 `create(true)`，于是 `~/.jev-harness/sessions/` 里攒了一堆空会话（用户报告；实测 11 个目录里 3 个全空、4 个是被 kill 的） | **全部改成延迟落盘**：会话目录、`session.jsonl`、`conversation.jsonl`、`evidence.jsonl` 都在「第一次真的开始」（首次提交或首次 `/jev`）时才创建；退出时只在落过盘的前提下写摘要。真机验：只启动+`/status`+`/quit` → **连 `JEV_HARNESS_HOME` 根目录都不存在**；提交一句 → 四份文件齐；`SIGKILL` 后 `/resume` 仍成功。旧目录里 3 个可证明为空的已清掉（`conversation.jsonl` 与 `evidence.jsonl` 均 0 字节） |
| 17 | `/jev`（新增命令 + 事件） | `jev ask` 只能在命令行用；会话内没有手工核验判定的入口 | **按用户口径集成**：`/jev <判定什么> [--label A --label B] [--material 材料]`，走会话自带的判定器，读数与 `ask` 同一份口径（不采纳、不套门槛、不产生行动），结果落 `manual_judgment` 事件。真机验：`/j`+`Enter` 补全成 `/jev`；真调 1045 ms、`jev-1.13.0`、概率 0.18/0.82、request_id 与 usage 都上屏；`rule` 判不了时如实报「没问成」并落 `error` |
| 18 | `main.rs` 子命令说明 | 五个实验子命令与 `chat` / `ask` 并列，看不出谁是研究臂 | 按用户提问「是否保留」的结论：**五个都保留**（它们是 §7.1 #6 的证据，各自有报告与离线端到端测试），但说明里统一标成「**研究臂**」并指向对应读数（M0–M3 与 `docs/m4-report.md`）；`run` 额外标「逐任务排查用」 |
| 19 | 新增 `crates/jh-app/src/ask.rs` + CLI | `jev ask` 是空壳：`--help` 宣传「单次 Jev 判定」，实为 `eprintln!("未实现")` + 退 2，零测试（工程审计的阻塞项 #3） | **按用户口径实现**：`--question` + `--label`（≥2）+ `--describe` + `--material` + `--judge jev\|llm\|rule` + `--out`；单问单答、**不采纳不套门槛**（概率/margin/confidence 原值），报告含 request_id / usage / 复现命令；`replay`/`simulated-truth` 显式拒绝；6 条单元测试 + 真调验证（Jev 1.13.0、1396 ms、usage 357/31）。顺带补上 `lib.rs` 模块地图缺的 `session` 条目（roadmap §9 的一条 backlog） |

## 5. 已知但**不在**本轮修（rc 之后）

按审计的严重度排序，全部是「低概率 / 内部 / 需要重跑批次」的类型：

| # | 位置 | 问题 | 为什么留到 rc 后 |
| --- | --- | --- | --- |
| 1 | `crates/jh-app/src/h4.rs:2359` | `read_entries(calls_path).unwrap_or_default()`：`calls.jsonl` 损坏 → H4 报告的成本段**静默全 0** | H4 报告已定稿发布，改代码就得重跑那一批；最小修法是 `?` 传播或写进报告的 `failures` 段 |
| 2 | `session/tui.rs:371`、`jh-tui/terminal.rs:152`/`:160` | 终端恢复失败三处 `let _ = …restore()` 无声 | 需要在信号/panic 路径上加 stderr 输出；影响面是「终端留在备用屏」，不改读数 |
| 3 | `session/agent.rs:1640` | `let _ = state.set_evaluation(Failed)`：语义状态写失败被丢（邻居分支会上屏） | 内部一致性，补一句 Notice 即可 |
| 4 | `c_measure.rs:215`/`:1135` | 材料序列化失败 → prompt 里材料静默变空串 | 概率极低；改后需确认不影响已发布读数 |
| 5 | `session/tools.rs:304-313` | `existing()` 解析失败即当「文件不存在」→ 覆盖已有文件的风险标签退化为 `Mutating` | 执行侧 `args()` 仍显式失败，只是标签问题（防御性缺口） |
| 6 | `main.rs`（893 行）、`session/model.rs`（225 行）、`jh-openai/client.rs`（290 行） | 零测试：CLI 装配与 26 个 exit code 分支、SSE 适配层的三个失败分支、HTTP 客户端的非 2xx / `Retry-After` / 超时 | 不影响 rc 正确性，影响后续回归保护 |
| 7 | `session/log.rs` | 「写失败不静默」这条**契约零测试**（成功路径 4 条都有） | 同上；宿主上屏代码已在 |
| 8 | 依赖与工具链 | 3 项零引用依赖、`Cargo.lock` 仍在 `.gitignore`、无 MSRV、无 `cargo-audit` | 卫生项；建议 rc 后跑一次 `cargo install cargo-audit && cargo audit` |
| 9 | 发布形态 | 不是按 crates.io 发布设计的（内部依赖纯 path、`jh-tui` `publish = false`、无 license 元数据） | 与「个人学习项目」的定位一致；rc 用 git tag，不必发布到 crates.io |

## 5.5 五个研究臂的取舍（`exp` / `compare` / `h3` / `h4` / `m4` + `run`）

**结论：全部保留。** 理由按保留价值从高到低：

| 命令 | 保留理由 | 删掉的代价 |
| --- | --- | --- |
| `m4` | 路线图 §7.1 #8 的产物就是它的报告（`docs/m4-report.md` + `m4-freeze.md`）；`tests/m4.rs`（8 个用例）是离线端到端证据 | 1.0 的结论段没有可复现入口 |
| `h3` / `h4` | 分别是 M2 / M3 的唯一读数来源，各有离线端到端测试（`tests/h3.rs` 4、`tests/h4.rs` 12） | M5 的 H5 与「观测能力建设」这一整章失去可复现路径 |
| `compare` | M1 的四组臂对照（H1 × H2），是「依赖失效传播」这条主线的唯一对照 | 主线假说失去原始读数 |
| `exp` | M0 的最小端到端（`tests/initial_task_set.rs` 6），也是**没凭据时唯一能跑通全链路的离线入口**；`docs/roadmap-1.0.md` §5 M0 直接引用它的读数 | 冒烟路径与 M0 出证链断掉 |
| `run` | 单任务排查（`--task A1`），排障时比 `compare` 直接得多 | 排障要多绕一层 |

**为什么不合并**：五条命令共用同一套 `judge` / `tasks` / `calls` / `metrics` / `report` 底层，各自的
差异只在「跑哪些臂、采集哪些读数」；合并成 `jev exp --arm …` 会把四份已经定稿、被文档逐条引用的
读数入口全部改道，收益只是 `--help` 少四行。**结论是保留，改动只做在说明文字上**（见 §4 #18）：
把它们统一标成「研究臂」并各自指向读数，读者不会再把它们当成日常命令。
**rc 之后可以再议**：`exp` 与 `run` 有重叠（都以任务族 A 为单位），若将来要收，先收 `run`
（把单任务排查并进 `exp --task`），`exp` 的 M0 读数入口保留。

## 6. 可用性审计（伪终端真机，独立一轮）

**总评**（审计者原话，逐条有证据）：
- **有凭据 + 默认 `--judge jev`：能**不看文档走完「提问 → 批准工具 → 拿到结果 → 接着追问」（实测走通）；
- **`--judge rule` + 需要批准工具：原本不能**（B1：批准被确认却不执行）——**本轮已修并真机验过**；
- **崩溃/关窗口后找回：原本不能**（B3：内容在盘上但 `/resume` 拒绝）——**本轮已修并真机验过**。

**测了且正常的**（摘要）：冷启动首字节 0.016 s / 首屏 0.023 s / 稳定 0.71 s；无凭据时点名缺哪个变量且离线命令全可用；
非 TTY 干净退 2；空输入无副作用；5000 字括号粘贴不卡且完整落库；多行与 `Ctrl+J` 正常；三模式上卡行为正确；
`never` 下工具真跑并如实报 `退出码 127`；拒绝审批有清楚回执；OSC 52 单行拖选**逐字节一致**（点一下不拖 = 0 载荷、拖出窗口不崩）；
markdown 全要素渲染、管道表格逐格列对齐；`Ctrl+O` 研究视图 0 残留字符；`/quit`、`Ctrl+C`、`SIGHUP` 都写全恢复序列（`?1049l ?25h ?2004l …`）；
`/sessions` → 选择器 `↑↓/Enter/Esc` 正确、`/resume` 回放含助手正文与工具块并如实声明不恢复语义状态；120×40→60×18→40×12→120×40 缩放无越界/重叠/残留。

**仍开着（本轮只登记，未改）**：

| 编号 | 现象 | 影响 | 建议 |
| --- | --- | --- | --- |
| F2 | 选项列表的选中行用**与输入框相同的 `❯` 标记**，看着像输入框被列表占了（实测打字其实正常，字符确实进了输入框） | 易误读，不敢打字 | 列表选中行换一个标记（`▸`），补全列表同理 |
| F3 | `/` 候选列表 `↑↓` 会动，但 **`Enter` 不采纳**（只有 `Tab`/点击可用），于是 `Enter` 提交了裸 `/` → 「未知命令 /」 | 与「高亮 = 可回车」的直觉相反 | 列表可见且输入框内容恰为 `/{查询}` 时，`Enter` 采纳候选（第二次 `Enter` 才提交） |
| F4 | 首屏菜单 `新建会话 Ctrl+N` 是空操作（`Ctrl+N` 未绑定，也没有会话轮换） | 首屏明示的键按了没反应 | 要么绑上（新会话 = 换 run 目录），要么从菜单去掉这一行的键位 |
| F5 | 40×12 下幽灵文案**硬截断且无省略号**（`Ctrl+Shift+V 粘贴 · /help 看命令` 整段消失），首屏菜单截断成 `… · 退出` | 小窗口丢提示 | 截断处补 `…`（与状态行/键位栏同一口径）或按宽度选用短文案 |
| F7 | 审批卡理由重复两遍；`auto` 下 `echo alpha` 仍上卡时写「确定性层：只读」与「只读直接放行」自相矛盾 | 文案冗余、自相矛盾 | 卡片正文去重；只读工具上卡的场景（`ask` 模式）换一句准确的理由 |
| F8 | 非 TTY 报错与退出码都对，但 `jev --help` 把 `ask`（空壳）与真命令并列；`jev run … \| head -3` 触发 `panicked … Broken pipe`（退出码 101） | CLI 表面积不诚实 + 管道下 panic | 见 §3 #4；管道 panic 建议在 `println!` 处忽略 `BrokenPipe` |
| P1–P7 | 顶栏徽章写的是**另一个**视图名（会话屏写 `[研究视图]`）；输入框只提 `Ctrl+Shift+V` 而 Ctrl+V 的说明列了各终端键位；`/clear` 无回执；复制回执「行/字节」混用；审批卡键位行把卡片键与全局键混一行；`Ctrl+D` 无反应；「判定不可用（规则表没有覆盖…）」措辞易读成判定器故障 | 打磨级 | 逐条小改 |
| 未测成 5 项 | Esc 打断在途模型调用（预算内没抓到在途窗口）、Windows Terminal 的 `Shift+Enter`（本机 Linux + pty）、`ask` 模式的完整闭环（原本被 B1 挡住，现已修但未复跑）、接回后真调续聊、真实终端剪贴板/鼠标上报（无 X/Wayland） | — | 修 B1 后值得补一次 `ask` 模式闭环；其余需要真机环境 |

## 7. 与 `1.0` 的差距（rc 之后）

| 差距 | 出处 | 说明 |
| --- | --- | --- |
| §7.1 #9 联合演示未交付 | roadmap §7.1 #9、§5 M4 | 同一次运行里既精化问题又建设观测器，M4 两批都没做 |
| 族 A 的开发 / 验证 / 测试划分 | roadmap §7.1 #5、§12 #8 | 族 B 有，族 A 没有（实测：`family_a.json` 无 split 字段） |
| 观测器库的入库文件形态 | roadmap §12 #5 | 只有运行期 `ObserverLibrary`，没有入库文件（实测：`git ls-files` 无 observer 数据文件） |
| 实验复现脚本不入库 | roadmap §7.1 #6 | 报告 §7 给了命令；两个读数提取脚本是临时的（实测：仓库里没有任何 `.sh` / `.py`） |
| `Hypothesis` / `Capability` 一等对象 | roadmap §12 #4 | 仍未定（实测：代码里只有报告用的 `HypothesisSection`，不是运行时对象） |
| 中文处理策略、批处理取舍、长 state 上限 | roadmap §12 #11 / #12 | 样本量不足，未定 |
| §9 backlog 12 条 | roadmap §9 | 全部「只登记、不改代码」 |
