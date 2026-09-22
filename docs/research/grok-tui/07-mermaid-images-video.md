# 专题 07：Grok Build 的图形化内容：Mermaid、内联图片与视频

> 专题：图形化内容——Mermaid、内联图片与视频｜调研日期：2026-09-22｜证据范围：`crates/codegen/xai-grok-mermaid/`（含 `src/*.rs` 共 6 文件、`tests/pure_engine.rs`）、`crates/codegen/xai-grok-pager/src/{inline_media_ffmpeg.rs, wrap_clipboard_image.rs, app/mermaid_worker.rs, app/agent_view/{media.rs,render.rs}, scrollback/{render.rs, wrappers/entry_renderer.rs, blocks/{mermaid_content.rs, agent.rs, tool/other.rs}}, views/prompt_widget/mod.rs, tests/mermaid_render_subprocess.rs}`、`crates/codegen/xai-grok-pager-render/src/{render/{image_overlay.rs, image_overlay/*, video_overlay.rs}, terminal/{image.rs, image/tests.rs, overlay.rs, mod.rs}, prompt_images.rs}`、`crates/codegen/xai-grok-markdown/src/{mermaid.rs, parse.rs, render.rs, output.rs}`、`third_party/{README.md, NOTICE, mermaid-to-svg/, dagre_rust/, graphlib_rust/}`、`docs/user-guide/21-terminal-support.md`、`26-config-reference.md`、`docs/tutorial/03-attach-and-paste.md`。全仓库检索过 `sixel`、半块字符、`braille`/`quadrant`/`chafa`。

## 1. 一句话总览

Grok 的"图形"不是一种技术，而是**三条互不依赖的路径**：Mermaid 有"文本制表符图（永远可用）"和"点击时生成的 PNG（终端协议渲染）"两套完全独立的渲染器；图片与视频走"ratatui 单元格占位 + 帧后（post-flush）逃逸序列贴像素"的两层模型。**没有任何一条路径把像素转成半块字符**，所以非图形终端的降级结果是"文本 affordance + 在系统看图器里打开"。

```
┌ 第 1 层：文本图形（ratatui Buffer，全终端可用，不依赖任何协议）───────┐
│  ```mermaid  → xai-grok-markdown/src/mermaid.rs  自建 parser + 布局 → │
│                 Unicode 制表符图（┌─┐│╎┃），随 markdown 一起绘制      │
│  图片/视频    → 预留 N 行空白 + 文件路径行 + [Open]/[Play]/[Copy] 行 │
└───────────────────────────┬───────────────────────────────────────────┘
                            │ 只在支持图形协议的终端上叠加第 2 层
┌───────────────────────────▼───────────────────────────────────────────┐
│ 第 2 层：像素图形（post-flush 逃逸序列，绕过 ratatui 单元格）          │
│  Kitty APC (\x1b_G…) 或 iTerm2 OSC 1337；写 stderr，在 cell flush 之后 │
│  位置用「单元格坐标 + 源像素裁剪」描述，不参与单元格绘制/选中/复制     │
└───────────────────────────┬───────────────────────────────────────────┘
                            │ 需要真实像素时另开进程
┌───────────────────────────▼───────────────────────────────────────────┐
│ 第 3 层：Mermaid PNG（子进程 + 磁盘缓存）                              │
│  xai-grok-pager __mermaid-render → mermaid-to-svg(dagre) → SVG →      │
│  resvg/tiny-skia → PNG 落盘 → 交给 OS 看图器或复制路径                 │
└───────────────────────────────────────────────────────────────────────┘
```

关键取向：**渲染 PNG 的 Mermaid 从不内联显示**（`mermaid_content.rs:6-8`「The rendered PNG is never drawn inline; it is reached only through the affordance row's actions.」），而内联的文本图不参与高保真渲染。两者是两条互不相干的实现，注释明确要求人工保持同步：`LABEL_BREAK_CHARS` 与 `third_party/mermaid-to-svg/src/text_wrap.rs` 的 `TOKEN_BREAK_CHARS` 一致（`xai-grok-markdown/src/mermaid.rs:35-38`）。

## 2. Mermaid 的端到端链路

### 2.1 识别：只认 `mermaid` fence 的第一个 token

- 内联侧：`is_mermaid_info()` 取 info string 的第一个空白分隔 token，大小写不敏感比较 `"mermaid"`（`mermaid_content.rs:87-92`），因此 ` ```Mermaid `、` ```mermaid theme=base ` 都算。数据来自 markdown 渲染器暴露的 `CodeBlockSpan`（`mermaid_content.rs:94-123`）。
- 替换侧：流式解析器在**闭合** fence 上才替换正文（`parse.rs:636-643`：`range.end < self.text.len()`，即"未闭合的 fence 在流式过程中继续显示源码"），且仅在 `pretty` 模式生效（`render.rs:469`）。raw 模式保留源码。
- 检测结果缓存在 message 级别：`AgentBlock` 在构造/结束时一次算出 `MermaidContent`，**不是每个流式 chunk 都算**（`blocks/agent.rs:17-18, 74`）。

### 2.2 内联文本图：零依赖、即时、永远可用

`crates/codegen/xai-grok-markdown/src/mermaid.rs`（5699 行）是一个**自包含的终端 Mermaid 渲染器**，不依赖 `xai-grok-mermaid`、不依赖 dagre、不需要 PNG：

- 支持类型：`graph`/`flowchart`、`stateDiagram`、`classDiagram`、`erDiagram`、`sequenceDiagram`（`mermaid.rs:67-82` 的 `or_else` 链）。
- 布局是自建的 Sugiyama 风格：`compute_ranks` → `order_ranks`（barycenter 交替上下扫，最多 8 轮，取交叉数最少者，`mermaid.rs:2672-2753`）→ `assign_positions`（10 轮松弛，`mermaid.rs:2806-2868`）→ `place_td` / `place_lr`。
- 画布用一个位掩码画布 `Canvas`（上/下/左/右 4 个 bit）最后 `finalize_mask()` 统一选制表符（`mask_char`，`mermaid.rs:1730-1745`），虚线走 `╌╎`、粗线走 `┃━┏┓┗┛`（`mermaid.rs:1747-1770`）。
- `BT`/`RL` 的做法是**先按 TD/LR 排好再整体翻转画布**，翻转后把文本 run 反向回来保证可读（`flip_vertical`/`flip_horizontal`，`mermaid.rs:1621-1669`）。
- 硬上限（防不可信输入膨胀）：`MAX_NODES=128`、`MAX_EDGES=512`、`MAX_GROUPS=24`、`MAX_GROUP_DEPTH=6`、`MAX_CANVAS_CELLS=1<<21`、标签 `WRAP_WIDTH=24` / `MAX_LINES=4` / `MAX_LABEL=28`（`mermaid.rs:28-45`）。
- 画不下或解析不了时一律退化为**带标题的框**：标题 ` mermaid: <首个 token> `，正文是原文换行截断（`fallback()`，`mermaid.rs:3925-3997`）；若原因是"太宽"，额外附一行斜体提示 `This diagram is too wide to display here — open the image to view it in full.`（`mermaid.rs:3922-3923, 3983-3985`）。

### 2.3 按需 PNG：点击才渲染

内联文本图下面会插入一行**不可选中的 affordance 行**（`apply_affordance_rows`，`mermaid_content.rs:373-397`），选中态不参与复制、也不移动 pre-wrap 行号：

```
◇ mermaid   [Open Image]   [Copy Image Path]   [Copy Source]      rendering diagram…
└ dim、不可点    └ 三个按钮都由同一份 AffordanceRow 布局驱动绘制与命中矩形（不会漂移）
```

- 按钮文案与间距：`AFFORDANCE_GAP = 3`（`mermaid_content.rs:31-38`）；行布局单一来源 `affordance_row()`（`mermaid_content.rs:310-325`），绘制与点击命中都读它（`agent_view/media.rs:229-313`）。
- 设置项 `ui.render_mermaid = auto|on|off`（默认 `auto`，`docs/user-guide/26-config-reference.md:621`；读取路径 `appearance/cache.rs:593-607`）：`off` → 只有内联图，无 affordance 行；`auto`/`on` → 有（`mermaid_content.rs:239-246`）。minimal（静态提交）模式强制 `SourceOnly`，因为绘制循环不会运行、按钮会是死行（`mermaid_content.rs:249-257`）。
- 点击 → `request_mermaid_render`（`mermaid_worker.rs:955-1031`）：先查内存 pending → 再查磁盘缓存命中 → 未命中才派发 worker 任务。**两种动作都强制走 Open 画质档**（`mermaid_worker.rs:958-960`）。

时序（生产路径）：

```
用户点 [Open Image]
   │
   ├─ 磁盘命中（read_cached_png：非符号链接 + 能解码）→ 直接 open_media_natively / 复制路径，不渲染
   │
   └─ 未命中 → ensure_mermaid_runtime() 懒启动 worker 线程（"mermaid-render"）
                → MermaidJob{key, source, out_path, theme_dark, quality=Open}
                → 子进程: current_exe() __mermaid-render --out … --theme … --quality open --width … --deadline-ms …
                │            stdin = 图表源码（≤64 KiB+1 字节则拒绝）
                │            子进程内：install_child_backstops() → read_stdin_capped() →
                │                      render_checked(default_engine()) → write_png_atomic()
                → 墙钟超时（3s / 20s）→ killpg(SIGKILL) 清理进程组
                → map_run_result：零退出 + PNG 可解码才算成功
                → mpsc 回主线程 → poll_mermaid_results → complete_mermaid_action
```

- 隐藏子命令 `__mermaid-render` 在 `main` **最早期**被拦截，早于任何 TUI/agent/tokio 初始化（`mermaid_worker.rs:365-382` 与模块文档 19-24 行）。
- 子进程参数：`--out --theme --quality --width --deadline-ms`（`mermaid_worker.rs:291-320`）；`--max-height` 作为 legacy 被接受并忽略（`mermaid_worker.rs:505-508`）。Linux 上额外设 `_RJEM_MALLOC_CONF`/`MALLOC_CONF=narenas:1`，避免多核机器上分配器预留逼近 RLIMIT_AS 导致每次渲染都 abort（`mermaid_worker.rs:312-319`）。
- worker 只有一个线程（不是线程池），并**按 cache key 合并同一批请求**（`drain_coalesced` + `IndexMap` FIFO，`mermaid_worker.rs:208-261`）。

### 2.4 尺寸与画质两档

| 档位 | 宽度来源 | 高度上限 | 背景 | 超时 |
| --- | --- | --- | --- | --- |
| `Terminal`（预留档，当前点击路径不用） | 内容列数 × `APPROX_CELL_W_PX(8)` × `RENDER_SCALE(2)`，clamp 到 320–1600 | 2400 px | 主题表面色（不透明） | 3 s |
| `Open`（`[Open Image]`/`[Copy Image Path]` 实际使用） | `RenderParams::for_os_viewer`：`scale=2.0` + 最小宽度 2560 | 8192 px | 主题表面色 | 20 s |

依据：`mermaid_worker.rs:66-89, 579-602`、`xai-grok-mermaid/src/lib.rs:132-161`。表面色只有两个：`LIGHT_SURFACE = #FAFAFA`、`DARK_SURFACE = #18181B`（`xai-grok-mermaid/src/lib.rs:64-67`），只有 `GrokDay` 判为亮色，`Terminal` 主题也按暗色处理（`mermaid_content.rs:126-129`）。

### 2.5 为什么要 vendored 的 dagre/graphlib 栈

`third_party/README.md` 与各 crate 的 `Cargo.toml` 头部注释给出了仓库自己的理由，不是推测：

- **渲染的是不可信模型输出**，vendoring 换来"完整审计面 + 钉死版本 + 免受 crates.io yank"（`third_party/README.md:8-14`；`third_party/mermaid-to-svg/Cargo.toml:7-12`）。
- 依赖链：`xai-grok-mermaid → mermaid-to-svg (MIT) → dagre_rust (Apache-2.0) → graphlib_rust → ordered_hashmap`，上游为 `warpdotdev/mermaid-to-svg` 与 `r3alst/dagre-rust`（`third_party/README.md:17-24, 27-37`）。
- **算法等效 mermaid.js**：mermaid-to-svg 把布局交给 dagre.js 的 Rust 移植，注释写明这"修复了上一个引擎在环形 flowchart 回边上的错误路由"（`third_party/mermaid-to-svg/Cargo.toml:7-12`）。`xai-grok-mermaid` 的测试也用同一个环形登录流回归：8 条边必须各自带一个 `marker-end="url(#arrowhead)"`（`xai-grok-mermaid/src/pure.rs:120-154`）。
- 本地补丁有清单，逐条记录在 vendoring notes 里，例如：不 vendor CLI bin 与 snapshot 测试；`mermaid_port::is_enabled()` 无条件返回 `false`（因为该实验分支读环境变量、对不可信输入不确定，且会错路由环形回边）；整段重写 sequence 解析器；CJK 宽度用 `unicode-width` 计算；open edge-label 语法；单一超长 token 不再被硬切（`third_party/mermaid-to-svg/Cargo.toml:14-96`）。`dagre_rust` 的补丁是把上游 `static mut UNIQUE_STARTER` 换成 `AtomicUsize`，消除多线程渲染的数据竞争（`third_party/dagre_rust/Cargo.toml:18-26`）。
- 升级流程被写成 4 步 checklist（读 vendoring notes → 重放补丁 → 核对许可证 → 刷新 `NOTICE`，`third_party/README.md:39-45`）。

## 3. 终端图形协议支持

### 3.1 只有两族协议：Kitty 与 iTerm2

`GraphicsProtocol` 只有 `Kitty` / `ITerm2` / `None` 三个值（`pager-render/src/terminal/image.rs:29-38`）。探测是**纯环境推断 + 一次缓存**，不做运行时特性协商：

```rust
// terminal/image.rs:144-157（简化）
pub fn protocol_for_brand(brand: TerminalName, is_windows: bool) -> GraphicsProtocol {
    if is_windows { return GraphicsProtocol::None; }   // ConPTY 会吞掉 APC/OSC
    match brand {
        Kitty | Ghostty | WezTerm | WarpTerminal => GraphicsProtocol::Kitty,
        Iterm2 => GraphicsProtocol::None,              // OSC 1337 无 placement/clear，预览另行放开
        _ => GraphicsProtocol::None,
    }
}
```

- 品牌来自 `TerminalName`（20 个变体，`terminal/mod.rs:58-107`）。
- 进程级 `OnceLock` 缓存（`terminal/image.rs:46, 71-83`）；tmux 直接跳过（`terminal/mod.rs:374-379`）。
- **scrollback 内联叠加层比协议支持更窄**：只在 `Kitty` 协议 **且** 品牌 ∈ {Kitty, Ghostty, WezTerm} 时启用，注释说明 Warp 虽然接受部分逃逸但不支持 placement/scrollback 模型，会留下残影（`terminal/image.rs:85-120`）。
- **iTerm2 只在 prompt 预览这一处放开**：条件是 `TERM_FEATURES` 含 `F`，或该变量不存在但处于 SSH（因为该变量不过 SSH，缺失不等于拒绝）；理由注释是"预览框永不落在最后一行，关闭时会重绘所有占位单元格，因此不需要 clear 逃逸"（`terminal/image.rs:159-198`）。
- minimal 模式用原子开关**强制关闭**内联叠加层，原因是它只提交静态文本、不跑绘制循环（`terminal/image.rs:48-62`；启动时由 `app/mod.rs:391` 按 minimal 传入）。

### 3.2 Kitty 实现

头部字段：`a=T,f=100,t=d,q=2,C=1,z=,i=1,p=1,c=,r=`——`a=T` 传输并显示、`f=100` 只收 PNG、`t=d` 直接传数据、`q=2` 静默、`C=1` 保留光标、`z` 决定层级、`i/p` 同一个 placement id 1、`c/r` 是单元格尺寸；数据 base64 后按 4096 字节切块，非末块 `m=1`（`terminal/image.rs:326-386`）。

- **缩放交给终端**：注释明确"terminal scales to the cell rect (it knows HiDPI)"（`terminal/image.rs:325`）。
- 两种原语：`transmit_kitty_image`（`a=t`，只上传不显示，`terminal/image.rs:358-361`）与 `place_kitty_image`（`a=p`，约 50 字节的纯定位，`terminal/image.rs:391-396`）。带 `x,y,w,h` 的 `place_kitty_image_cropped` 负责源裁剪（`terminal/image.rs:402-416`）。
- 层级：模态叠加用 `z=1`（文本之上），scrollback 内联媒体用 `z=-1`（文本之下、背景之上，下拉菜单能盖住它）（`terminal/image.rs:337-343, 542`）。
- 删除靠 id：`clear_kitty_image(id)` → `a=d,d=i,i=<id>`（`terminal/image.rs:419-421`）。
- **Warp 特别处理**：placement replace 被忽略，所以固定 id 的模态图先删再传，否则会叠影（`terminal/image.rs:441-443`）。
- 编码转换：只有 PNG 能直传；非 PNG 走 macOS `sips`（保留 ICC profile，避免 JPEG→PNG 伪影），其他平台走 `image` crate 重编 PNG（Fast + Adaptive filter）（`terminal/image.rs:233-315`）。

### 3.3 iTerm2 实现

一行 OSC 1337 内联图片，尺寸直接给单元格数并**关闭宽高比保持**（`preserveAspectRatio=0`），因为调用方已经算好矩形、再保持比例会留出黑边（`terminal/image.rs:428-434`）：

```
ESC ] 1337 ; File=inline=1;width=<cols>cells;height=<rows>cells;preserveAspectRatio=0:<base64> BEL
```

差异点：iTerm2 **没有 place-only 逃逸**，所以每次矩形变化都要重发整幅图（`agent_view/media.rs:200-221` 的 `emit_iterm` 逻辑）；而且 OSC 1337 会推进光标，必须用 DECSC/DECRC（`\x1b7`/`\x1b8`）把光标包起来（`terminal/image.rs:464-471`）。

iTerm2 还有一个 Kitty 没有的问题：拖拽改变窗口尺寸时它会**连已提交的像素一起按网格缩放**，而 OSC 1337 没有 clear 原语。所以预览在 resize 后的 300 ms 静默窗口内被隐藏（并保留 100 ms 的额外 tick 边距持续重绘）；Kitty 终端则是丢弃并按新矩形重放（`app/agent_view/notices.rs:13-16, 160-177`）。

### 3.4 未找到证据：Sixel 与半块字符

- 全仓库（Rust 源码、文档、`Cargo.toml`、vendored 源码）检索 `sixel` **零命中**。
- `braille` / `quadrant` / `chafa` / `viu` / `timg` 检索零命中；`▀ ▄ █ ▌` 等块字符的用法集中在进度条（`views/progress_bar.rs:1, 16-20`）、滚动条（`render/scrollbar.rs:185`）、dashboard 光晕（`views/dashboard/render.rs:1104, 1254`）、工具用量图标（`tool_usage.rs:64-70`）等**文本语义**场景。**没有把栅格图转成半块/四分之一块字符的渲染器**。
- 结论：本快照的降级链是 **Kitty → iTerm2（仅 prompt 预览）→ 文本 affordance**，不存在第 4 档字符画。

### 3.5 降级顺序与判定表

| 场景 | Kitty | iTerm2 | None（含 tmux / Windows / 未知终端） |
| --- | --- | --- | --- |
| scrollback 内联图片/视频 | Kitty/Ghostty/WezTerm 才启用（`terminal/image.rs:109-120`） | ✗ 不用 | ✗ |
| prompt 悬停预览（image chip） | ✓ | `TERM_FEATURES` 含 `F` 或 SSH 时 ✓ | ✗ → 元数据文本 |
| 图片/视频查看器模态 | ✓ | ✓ | ✗：被 `guard_image_support()` 拦下并弹 toast |
| Mermaid PNG | 与终端能力**无关**：生成文件后交给 OS 看图器 | 同左 | 同左 |

非图形终端打开图片查看器会被 `guard_image_support()` 提前拦下并弹一句 toast：tmux 下是 `Inline images disabled within tmux.`，其他终端是 `Image rendering not supported in this terminal`（`app/agent_view/notices.rs:271-281`，调用点 `app/agent_view/prompt.rs:468-477`）。

Mermaid 是唯一刻意不看图形能力的部分——`mermaid_content.rs:236-238` 的注释："Terminal graphics capability is intentionally not consulted either: the affordance row is text plus mouse hit-rects, so it works everywhere."

## 4. 尺寸、缩放、留白与滚动

### 4.1 单元格尺寸换算：先测真实 cell 宽高比

`cell_aspect()` 用 crossterm 的 `window_size()` 算出 `(窗口像素宽/列数) ÷ (窗口像素高/行数)`，只在 `0.3..=0.8` 这个"真实等宽字体"带内接受，否则回落到 `DEFAULT_CELL_ASPECT = 0.5`（典型 8×16 cell）（`terminal/image.rs:555-591`）。理由注释：协议会填满单元格矩形，假设 1:2 会把其他字体的图拉变形；tmux/Windows/非 tty 的报告不可信，一律用默认值。

`fit_image_to_cells(img_w, img_h, max_cols, max_rows)` 的做法是：把图像宽高比换算成"每行需要多少列"（`cols_per_row = img_aspect / cell_aspect`），先尝试按宽铺满、行数超了就改按高铺满（`terminal/image.rs:594-621`）。

### 4.2 scrollback 的占位与裁剪

图片/视频在 scrollback 里是**两层分离**：单元格层画占位（可选中、可滚动、可复制），像素层用逃逸序列贴在算好的矩形上。

```text
┌ scrollback 终端窗口 ────────────────────────────┐
│  上一段消息文本…                                 │
│ ┌ tool media 块（entry_renderer 预留 rows+3 行）┐│
│ │ ① 顶部 1 行：文件路径（点击复制路径）          ││ ← content_y_start + 1
│ │ ② 中间 rows 行：图像占位（像素由 post-flush   ││ ← screen_rect（随滚动裁剪）
│ │    逃逸贴在这块矩形上，选择/复制看不到像素）   ││
│ │ ③ 末尾 1 行：空白 spacer                      ││
│ │ ④ 最后 1 行：[Open] / [Play] / [Copy]         ││ ← content_lines - 2
│ └───────────────────────────────────────────────┘│
│  下一段消息文本…                                 │
└──────────────────────────────────────────────────┘
```

- 预留行数取单一来源：`inline_media_reserved_rows()` 返回 `(图像行数, 总行数=图像行数+3)`，被"预留占位"和"实际贴图"两处共用，注释强调两者不能漂移（`inline_media_ffmpeg.rs:121-134`；`entry_renderer.rs:350-364`）。图像行预算：`max_cols = content_width - 2`，`max_rows = (content_width / 2).clamp(4, 20)`。
- 非图形终端返回 0 行，改为输出文本 `[Open]` 行（`entry_renderer.rs:351-353` → 0；`blocks/tool/other.rs:413-427` 的 `inline_open_button()`）。
- **滚动裁剪**：每帧按虚拟坐标算出可见部分，用 `top_crop_rows` 表达"被上方裁掉多少行"，可见高度不足完整高度时转成 Kitty 源裁剪的 `y/w/h`（`scrollback/render.rs:650-712`；换算 `src_y = top_crop_rows * img_h / fit_rows`，`terminal/image.rs:518-543`）。因此滚动是"移动窗口"而不是重新解码图片。可见性门槛：`full_image_h >= 1 && media_width >= 4`。
- 视频海报在 scrollback 里就是普通图片路径；无 ffmpeg 时预留行数缩为 1–2 行横幅（安装提示），其余布局不变（`inline_media_ffmpeg.rs:126-129, 183-194`）。
- Mermaid 的 affordance 行也走同一套虚拟坐标 → 屏幕矩形换算，只画**行内可见**的那一行（`scrollback/render.rs:728-745`）。

### 4.3 帧间重绘与清理

- 每帧收集本帧实际放置的 Kitty id 集合 `this_frame_ids`，与上一帧 `last_placed_ids` 求差：消失的 id 立即发 `clear_kitty_image` 并从 `inline_media_ids` 移除（`app/agent_view/render.rs:4173-4181`）。本帧没有任何放置时整批清理（`render.rs:4183-4206`）。
- 打开任何模态（图片/视频查看器、gboom、block viewer、feedback 等）会先把 scrollback 的像素全部清掉，避免浮在模态之上（`app/agent_view/render.rs:629-650`）；dashboard 切换时也有一次性的"陈旧图像清理"（`app/app_view.rs:4209-4235`）。
- 非 scrollback 的叠加层（图片查看器/prompt 预览）共用 `KITTY_PLACEMENT_ID = 1`，由 `terminal/overlay.rs` 用 `(owner_id, cols, rows, x, y)` 记账：几何完全没变就发**空串**（不重传），变了才"先删再传"（`terminal/overlay.rs:161-184`；测试 `overlay.rs:301-352`）。`commit()` 只在写成功后记账，写失败不会误判像素已在屏上（`overlay.rs:104-110, 355-375`）。

## 5. 视频（帧序列）

- **抽帧在后台线程**：`start_inline_video_playback` 起线程调 `VideoViewerState::open_from_path`，通过 mpsc 回传，UI 先给 "Loading video…" toast（`app/agent_view/media.rs:430-462`）。若 `open_from_path` 失败，tick 里弹 "Video playback requires ffmpeg"（`app/app_view.rs:5454-5465`）。
- **参数**：`VIDEO_FPS = 10.0`（取源帧率与 10 的较小者）、`VIDEO_MAX_WIDTH = 640`（更宽才加 `scale=640:-2`）、`-q:v 5`；输出格式按协议选 **PNG（Kitty）/ JPEG（iTerm2）**，写成临时目录里 `%06d.<ext>`，读完帧就删目录（`pager-render/src/prompt_images.rs:214-217, 263-330`）。帧集合被注释描述为"5–15 秒 10fps ≈ 50–150 帧、约 1–3 秒"。
- **海报帧**（scrollback 静止显示）：`extract_poster_frame` 先 `-ss 1` 取第 1 秒的画面试代表帧，失败再取首帧；协议为 `None` 时直接返回 `None`（`prompt_images.rs:397-437`）。
- **播放节奏**：`app_view` 每 tick 用 `last_frame_time.elapsed() >= 1/fps` 推进一帧，末帧后置 `finished`（`app/app_view.rs:5469-5482`）；`view_tick_demand()` 把"视频未播完 / 正在抽帧 / mermaid 有待办"都算作需要继续 tick 的理由（`app/app_view.rs:5636, 5717-5719`）。
- 每次换视频或关闭查看器都会丢弃整个帧集（几十到几百 MB），通过 `memory_release::request_release_after_draw` 在**帧后**归还内存，绝不在绘制中途同步释放（`app/agent_view/media.rs:370-388, 592-614`；`app/agent_view/render.rs:629-650`）。

## 6. Prompt 侧的视觉呈现

### 6.1 图片 chip：`[Image #N]`

- 粘贴/拖入成功后计数器 +1，占位文本 `[Image #N]`（`prompt_images.rs:744-747`），并作为 textarea 的**原子元素**插入（`ElementKind(3)` = `KIND_IMAGE`，`views/prompt_widget/mod.rs:45, 2363-2373`）。chip 里**永远不含路径**，路径只出现在预览与 overlay（`prompt_images.rs:743-746` 的注释）。
- 视觉：`chip_line()` 把 `[`、`]` 用 `theme.paste_dim` 前景 + `theme.paste_bg` 背景，中间文字用 `theme.paste_fg` + `theme.paste_bg`（`views/prompt_widget/mod.rs:3761-3770`）——即**整块反色底色的 chip**，与粘贴文本块同款。
- 插入前校验：宽或高小于 `MIN_SIDE = 8` 像素直接报错拒收（`views/prompt_widget/mod.rs:2354-2361`）。
- 光标/悬停在 chip 上时，元素交互类型是 `ImagePreview`（`views/prompt_widget/mod.rs:2938`），触发下面的预览 overlay。

### 6.2 悬停预览：一个 2×2 决策矩阵

`render/image_overlay.rs` 的模块注释直接给出矩阵（`image_overlay.rs:1-12`）：

|  | 有文件路径 | 无文件路径 |
| --- | --- | --- |
| **有像素** | 图片 + 路径 footer | 仅有图片 |
| **无像素** | 元数据 + 路径 | 仅元数据 |

```text
        ┌─ Image #3 ─────────────────────┐   ← 标题：` Image #N ` （够宽时追加 "─ 元数据 "）
        │                                │
        │        （像素由 post-flush      │   ← 有像素时先画居中 "Loading..."，
        │          逃逸贴在内部矩形）      │      拿不到 prepared() 就只留文本
        │                                │
        │ ~/shots/error.png              │   ← 路径 footer（inner_height >= 2 才保留）
        └────────────────────────────────┘
```

- 尺寸常量：框最小宽 28、有像素最小高 8、纯元数据最小高 6；元数据框宽 = 区域宽 × 0.75、内容 4 行 + 2 行边框（`render/image_overlay/geometry.rs:8-14, 89-100`）。
- 有像素时：内部尺寸按 `fit_image_to_cells` 反推框大小，图片在内部再居中（`geometry.rs:61-86`）；框外整块区域做 50% 变暗（`image_overlay.rs:86`）。
- 无像素时打印 `Format:` / `Dimensions:` / `Size:`（或 `Preview unavailable` / `Preview pending`）/ `Path:`（`image_overlay.rs:142-186`）。
- 像素通过 `overlay::static_image_for_protocol` 走共享 id 记账（`image_overlay.rs:202-213`）。

### 6.3 图片 / 视频查看器模态

- 两者都是**居中 90% 的圆角框**（宽至少 28，高至少 8），外圈 50% 变暗；视频版在底边框上画进度条（`▶`/`⏸` + `━` 已播 / `─` 未播 + `m:ss/m:ss`）（`render/video_overlay.rs:24-49, 77-122`）。
- 图片查看器的显示字节在打开时就按协议准备好（JPEG→PNG for Kitty）（`prompt_images.rs:56-82`），持有的 `overlay_owner_id` 与 prompt 预览共用同一套 owner 记账。
- 键盘：`Esc`/`q` 关闭并先清 Kitty 图（`app/agent_view/media.rs:76-100`）；视频查看器另有空格播放/暂停、`←/→`（或 `h/l`）快退/快进（`app/agent_view/media.rs:585-615`）。关闭时同步归还内存（查看器帧集可达 50–300 MB）。

### 6.4 `@` 附件与图片按钮的视觉差异

- `@` 文件引用是另一套元素（`KIND_FILE_REF`），显示形如 `@foo/bar.rs` 或 `@foo/bar.rs:10-12`：`@` 与 `:` 用灰色、路径用 `theme.path`、行号用 `text_primary`（`views/prompt_widget/mod.rs:3620-3634`；`file_ref_display`）。交互是把元素展开成文本（`ElementInteraction::Inlined`），**没有图片预览**（`views/prompt_widget/mod.rs:2933-2940`）。
- 文档口径一致：`docs/tutorial/03-attach-and-paste.md:5-23` 把"`@` 提及文件（支持行号、`@!` 取隐藏文件）"、"粘贴图片（macOS `Cmd+V` / Linux `Ctrl+V` / Windows `Alt+V`）"、"`!` 直接跑 shell 命令"列为入口的三种方式。

### 6.5 剪贴板图片在远程/包裹场景的传输

`wrap_clipboard_image.rs` 解决的是 `grok wrap` 这种"本地终端包住远程 pager"时剪贴板图片拿不到的问题：

- 本地全 miss（没有图片、文本、file URL）且 OSC 52 sink 已激活时，向 stderr 发私有 OSC `ESC ] 999;GrokWrapClipboardImage? BEL`（`wrap_clipboard_image.rs:17-28, 49-87`）。
- wrapper 用**括号粘贴帧**回注：`GROK_WRAP_IMG\n<mime>\n<base64>` 或 `GROK_WRAP_NONE`（`wrap_clipboard_image.rs:30-34, 124-138`）。`GROK_WRAP_NONE` 不是 `GROK_WRAP_IMG` 的前缀，正是为了区分"明确没有图片"和"帧被截断"。
- 预算 `MAX_WRAP_IMAGE_BYTES = 20 MiB`（4 MiB 太小会让 retina 截图静默变成 NONE，`wrap_clipboard_image.rs:36-39`）；超预算先按 JPEG 质量阶梯 `[85, 70, 55, 40, 25]` 重压，仍不行再半分辨率 JPEG 60（`wrap_clipboard_image.rs:144-185`）；解码前先用 base64 长度估算拒绝，避免超大分配（`wrap_clipboard_image.rs:106-117`）。
- 安全注释把这条路径定性为"等价于一次图片剪贴板读取"，明确写"不要把这个模式泛化到不可信的多路复用器上"（`wrap_clipboard_image.rs:6-12`）。

## 7. 性能与失败处理

### 7.1 超时与进程隔离（`panic = "abort"` 下唯一的真实防线）

| 机制 | 值 | 位置 |
| --- | --- | --- |
| 父进程墙钟预算（terminal / open 档） | 3 s / 20 s | `mermaid_worker.rs:82-96` |
| 子进程自毁看门狗 | 预算 + 3 s 后 `exit(2)` | `mermaid_worker.rs:384-427` |
| 看门狗退出码（与普通失败 exit 1 区分） | 2 | `mermaid_worker.rs:389-392` |
| 超时后清理 | `killpg(SIGKILL)` 整个进程组 | `xai-grok-mermaid/src/subprocess.rs:95-141` |
| 父子同生共死 | `prctl(PR_SET_PDEATHSIG, SIGKILL)`（Linux） | `mermaid_worker.rs:458-468` |
| 地址空间上限 | `RLIMIT_AS = 2 GiB`（Linux，只降不升） | `mermaid_worker.rs:401-456` |
| 源码大小上限 | 64 KiB（父进程先拒一次，子进程再拒一次） | `xai-grok-mermaid/src/engine.rs:36-50` |
| 光栅面积 / 单轴上限 | 32 MP / 16 384 px | `xai-grok-mermaid/src/raster.rs:22-27` |

为什么必须出进程：shipped profile 用 `panic = "abort"`，`render_checked` 里的 `catch_unwind` 在那里是空操作；同步渲染也无法被超时打断。模块文档把"真实崩溃隔离在进程之外"写成了设计结论（`xai-grok-mermaid/src/lib.rs:13-21`；`mermaid_worker.rs:14-34`）。

### 7.2 缓存

| 缓存 | 键 / 容量 | 位置 |
| --- | --- | --- |
| Mermaid 磁盘 PNG | `blake3(source)` + 主题 + 宽度桶 + 画质档 + `RENDER_REVISION=3` | `mermaid_content.rs:134-195` |
| 宽度分桶 | 终端档每 8 列一桶；Open 档用哨兵 `u16::MAX`（不与终端档共用文件） | `mermaid_content.rs:39-53, 170-186` |
| 会话磁盘上限 | 200 MiB，超限按 mtime 由旧到新删；每 8 次新写入扫一次 | `mermaid_worker.rs:62-64, 98-101, 725-774` |
| 缓存命中的判定 | 非符号链接 + 能解码成图（半截/损坏文件视为 miss 重渲染） | `mermaid_worker.rs:263-277` |
| 内联媒体编码字节 | 64 MiB 上限，超限条目只作临时使用不入缓存（随机淘汰） | `app/agent_view/media.rs:46-70, 176-186` |
| 失败负缓存 | 按 `(len, mtime, ino, ctime)` 指纹记录读取失败；文件一变就重试 | `app/agent_view/media.rs:13-44, 149-175` |
| Kitty 上传记账 | id 一旦分配就保留在 `inline_media_ids`，即使 CPU 侧字节被淘汰也不必重传 | `app/agent_view/media.rs:176-178` |
| 预览结果 | `PromptImagePreview`（`OnceLock` + `identity()`）供同一张图跨帧复用 | `pager-render/src/prompt_images.rs:613-630` |

一条值得抄的不变式：**先拿到字节，再分配 Kitty id**。否则"读文件失败但已记 id"会让下一帧只发 `place` 不发 `transmit`，永久留一块空白（`app/agent_view/media.rs:135-138, 187-196`）。

### 7.3 失败降级表

| 失败 | 用户看到 | 证据 |
| --- | --- | --- |
| Mermaid 解析/布局/光栅化失败、超时、子进程崩溃 | toast `Could not render diagram`，内联文本图仍在 | `mermaid_worker.rs:341-360, 1048-1066` |
| 源码超 64 KiB | 父进程直接判失败，不启动子进程 | `mermaid_worker.rs:662-672` |
| 还没有 session 目录可缓存 | toast `Diagram not ready yet` | `mermaid_worker.rs:961-971` |
| 渲染进行中 | toast `Rendering diagram…` + 行尾 `rendering diagram…` | `mermaid_worker.rs:89, 1018`；`mermaid_content.rs:26-28` |
| 终端不支持图形 | 无像素，仅文本 affordance（`[Open]` 行） | `entry_renderer.rs:351-353`；`blocks/tool/other.rs:413-427` |
| 视频缺 ffmpeg | 1–2 行 `Install ffmpeg to view inline` 横幅（有包管理器时附可执行的 `! …install ffmpeg`） | `inline_media_ffmpeg.rs:5, 50-70, 117-129` |
| 媒体文件读不到 / 解码失败 | 不显示像素，且按指纹负缓存避免每帧重试 | `app/agent_view/media.rs:143-175` |
| 非图形终端点开图片查看器 | toast `Image rendering not supported in this terminal`（tmux：`Inline images disabled within tmux.`） | `app/agent_view/notices.rs:271-281` |
| iTerm2 预览缺 `TERM_FEATURES` 的 `F` | 降为元数据文本预览 | `terminal/image.rs:181-198` |

## 8. 复刻时的边角（踩过才知道）

- **两套 Mermaid 渲染器意味着两套 bug 面**：内联文本图与 PNG 的换行/断词规则靠注释纪律同步，不是共享代码（`mermaid.rs:35-38`）。
- **不要在绘制中途释放大内存**：帧集/大图的释放全部走 `request_release_after_draw`，输入路径（关闭查看器）才允许同步释放（`app/agent_view/media.rs:370-388`）。
- **最小模式（minimal）与图形能力是两把锁**：`set_inline_overlay_force_off` 与 truecolor/协议探测互不影响，任何"是否会画像素"的判断都要同时看这两者（`terminal/image.rs:48-62`）。
- **iTerm2 的 `preserveAspectRatio=0` 与 Kitty 的 `C=1`** 是两族协议最容易被忽略的差异点，直接决定观感（`terminal/image.rs:428-434, 325`）。
- **子进程路径要先 `current_exe()`**：worker 用当前可执行文件重新执行自己；`current_exe()` 失败时渲染功能整体关闭而不是回落到进程内渲染（`mermaid_worker.rs:178-203`）。

## 关键结论（供合稿引用）

1. Mermaid 是**双渲染器**：内联是自包含的纯文本制表符渲染器（`xai-grok-markdown/src/mermaid.rs:1-4, 67-82`，上限见 `:28-45`），高保真线是另一条完全独立的 PNG 链路（`xai-grok-mermaid/src/lib.rs:1-21`），两者只在 `LABEL_BREAK_CHARS` 上靠注释保持同步（`mermaid.rs:35-38`）。
2. **PNG 从不内联显示**：只有点击 `◇ mermaid [Open Image] [Copy Image Path] [Copy Source]` 才惰性渲染，然后交给 OS 看图器或复制路径（`mermaid_content.rs:1-8, 293-325`；`mermaid_worker.rs:955-1031`）。
3. Mermaid 渲染走**隐藏子命令 `__mermaid-render` 的短期子进程**（`mermaid_worker.rs:54-56, 282-324, 365-382`），因为 shipped profile 是 `panic = "abort"`、进程内 `catch_unwind` 无效（`xai-grok-mermaid/src/lib.rs:13-21`）。
4. 容错是**四层叠加**：父进程 3 s/20 s 墙钟 kill + 子进程 `budget+3s` 自毁（exit 2）+ `PR_SET_PDEATHSIG` + Linux `RLIMIT_AS=2 GiB`（`mermaid_worker.rs:82-96, 384-468`）。
5. vendored `mermaid-to-svg → dagre_rust → graphlib_rust` 的官方理由是"渲染不可信模型输出、需要完整审计面与免受 yank"，且 dagre 修复了环形 flowchart 回边路由（`third_party/README.md:8-24`；`third_party/mermaid-to-svg/Cargo.toml:7-12`；回归测试 `xai-grok-mermaid/src/pure.rs:120-154`）。
6. 终端图形协议**只有 Kitty 与 iTerm2 两族**（`terminal/image.rs:29-38, 144-157`）；scrollback 内联仅限 Kitty 协议 × {Kitty, Ghostty, WezTerm}（`:109-120`），iTerm2 只在 prompt 预览放开（`:181-198`），tmux 与 Windows 直接判为不支持（`terminal/mod.rs:374-379`；`terminal/image.rs:141-147`）。
7. **Sixel 与半块字符渲染在本快照中不存在**：全仓库 `sixel` 零命中，`▀▄█` 只用于进度条/滚动条/光晕等文本语义（`views/progress_bar.rs:1, 16-20`、`render/scrollbar.rs:185`），因此降级链止于"文本 affordance"。
8. 尺寸换算的基准是**实测 cell 宽高比**（`0.3..=0.8` 之外回落到 0.5），再按"每行需要多少列"决定铺满宽还是铺满高（`terminal/image.rs:555-621`）。
9. scrollback 媒体是"**单元格占位 + 帧后逃逸贴像素**"：预留 `rows+3` 行（`entry_renderer.rs:350-364`、`inline_media_ffmpeg.rs:121-134`），滚动用 `top_crop_rows` 转成 Kitty 源裁剪而不是重解码（`scrollback/render.rs:650-712`、`terminal/image.rs:518-543`）。
10. 帧间用 `last_placed_ids` 差集清理消失的 Kitty 图，打开任何模态前先清空 scrollback 像素（`app/agent_view/render.rs:629-650, 4173-4206`）；模态与预览共用 placement id 1 的 owner 记账，几何未变就不重传（`terminal/overlay.rs:161-184`）。
11. 视频是把**全部帧预抽到内存**的帧序列：10 fps、宽 ≤640、PNG（Kitty）/JPEG（iTerm2）、海报帧优先 `-ss 1`，无 ffmpeg 时退为安装横幅（`pager-render/src/prompt_images.rs:214-217, 263-330, 397-437`；`inline_media_ffmpeg.rs:117-134`）。
12. Prompt 侧视觉是两套元素：图片是**反色底色的 `[Image #N]` chip**（路径永不出现在 chip 里），`@` 文件引用是彩色路径文本、无预览（`views/prompt_widget/mod.rs:2354-2373, 3761-3770, 3620-3634, 2933-2940`）。
