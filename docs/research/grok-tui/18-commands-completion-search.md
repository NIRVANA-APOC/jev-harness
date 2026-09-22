# 专题 18：命令、补全与搜索交互

> 专题：命令、补全与搜索交互｜调研日期：2026-09-22｜证据范围：`crates/codegen/xai-grok-pager/src/slash/`（command.rs、registry.rs、matcher.rs、mru.rs、mode_support.rs、mid_text_hoist.rs、commands/、mod.rs）、`src/views/{slash_dropdown.rs,completion_dropdown.rs,history_search.rs,file_search/}`、`src/app/{command_catalog.rs}`、`crates/codegen/xai-fuzzy-file-search/src/lib.rs`、`docs/user-guide/{03-keyboard-shortcuts.md,04-slash-commands.md}`。所有路径省略 `reference/grok-build/` 前缀。

## 1. slash 命令体系

### 1.1 命令对象：`SlashCommand` trait

命令是一个纯元数据 + 同步执行的对象，全部行为通过 trait 方法暴露（`crates/codegen/xai-grok-pager/src/slash/command.rs:213`）。关键契约：

- `name()` / `aliases()` / `description()` / `usage()` —— 身份与展示（`command.rs:215`、`:218`、`:223`、`:236`）。
- `takes_args()` / `takes_args_now(ctx)` —— 静态与动态两档；后者只看「当前这一刻能不能接参数」，被下拉的尾随空格、参数阶段快照与参数建议三处消费（`command.rs:239`、`:246`）。
- `args_required()` —— 参与 Enter 的完整性判定（`command.rs:251`）。
- `visible(ctx)` / `session_scoped()` / `offered_when_session_less()` / `dashboard_only()` —— 会话状态与面板维度（`command.rs:273`、`:280`、`:286`、`:293`）。
- `mode_support()` —— 渲染模式闸门（`command.rs:305`）。
- `required_tools()` —— 工具能力门控，fail-closed（`command.rs:324`）。
- `suggest_args()` / `preselected_arg()` / `supports_preview()` / `preview_arg()` / `cancel_preview()` —— 参数级建议与实时预览（`command.rs:258`、`:265`、`:331`、`:344`、`:349`）。
- `submission_refusal()` —— 在提交路径改动 composer 之前拒绝（`command.rs:231`）。
- `run()` —— 同步执行；需要异步的一律返回 `CommandResult::Action(...)` 交给 dispatch（`command.rs:354`）。

回传结果是一个 8 变体枚举 `CommandResult`：`Handled` / `Doctor` / `Error` / `Message` / `Action` / `QueueCommand` / `InjectSkill` / `PassThrough`（`command.rs:33-64`）。`PassThrough` 是「认不出的命令原样交给 shell/模型」的出口。

元数据用宏 `slash_meta!` 生成，字段必须按 trait 声明顺序书写，只有显式列出的字段才产生 override（`command.rs:357-375`）：

```rust
slash_meta! {
    name: "btw",
    usage: "/btw <question>",
    takes_args: true,
    args_required: true,
    can_hoist_from_mid_text: true,
    arg_placeholder: "<question>",
}
```

（取自 `slash/commands/btw.rs:11-19`。）

### 1.2 命令总数与「菜单顺序即分类」

内置命令的唯一真相源是 `builtin_commands()`（`slash/commands/mod.rs:77`），返回 74 个 `Arc<dyn SlashCommand>`（`grep -c "Arc::new(" commands/mod.rs` = 74）。它没有独立的分类枚举，**分类就是 vec 里的注释分组与顺序**，而这个顺序是菜单的最终 tiebreak（`commands/mod.rs:74-76` 明确写着「moving an entry moves it in the menu」）：

| 注释分组 | 数量 | 代表 |
| --- | --- | --- |
| The rows the dropdown shows before it scrolls. | 8 | `/tutorial` `/settings` `/dashboard` `/workflows` `/plugins` `/btw` `/voice` `/new` |
| Per turn. | 6 | `/effort` `/model` `/context` `/compact` `/fork` `/resume` |
| Steering the work in front of you. | 13 | `/loop` `/plan` `/view-plan` `/remember` `/memory` `/flush` `/dream` `/recap` `/rewind` `/jump` `/expand` `/edit-prompt` `/queue` |
| This session and what came out of it. | 10 | `/session-info` `/share` `/rename` `/history` `/transcript` `/export` `/copy` `/find` `/usage` `/tasks` |
| Extending the agent. | 7 | `/skills` `/mcps` `/hooks` `/marketplace` `/workflow` `/personas` `/config-agents` |
| Settings and display. | 8 | `/theme` `/auto` `/always-approve` `/vim-mode` `/multiline` `/compact-mode` `/timestamps` `/toggle-mouse-reporting` |
| Screen-mode switchers: visible only in the opposite mode. | 2 | `/minimal` `/fullscreen` |
| Reached for occasionally. | 4 | `/timeline` `/cd` `/imagine` `/imagine-video` |
| Docs, account and one-off maintenance. | 13 | `/docs` `/release-notes` `/announcements` `/feedback` `/privacy` `/doctor` `/import-claude` `/login` `/logout` `/home` `/delete` `/help` `/exit` |
| 隐藏条目 | 3 | `/gboom`（彩蛋，永不列出）、`/scroll-debug`、`/debug`（仅 debug 二进制列出） |

行号区间：`commands/mod.rs:79-165`。除内置命令外，shell 通过 ACP `AvailableCommandsUpdate` 广告的命令与 skill 也会进同一个菜单，命令总数因此是运行时的（`slash/registry.rs:392`、`:407`）。

### 1.3 注册表结构：命令 → trigger 的两层映射

`CommandRegistry`（`slash/registry.rs:114`）持有 10 个字段：`commands`（命令对象）、`sources`（Builtin/Acp 来源）、`key_to_index`（name/alias → 下标）、`triggers`（模糊匹配用的扁平条目）、`hidden`、`menu_hidden`、`restricted`、`available_tools`、`saved_workflows`、`skipped_acp_names`（`slash/registry.rs:115-135`）。

```
CommandRegistry
├── commands: Vec<Arc<dyn SlashCommand>>     ← 命令对象，按 builtin_commands() 顺序
├── key_to_index: HashMap<String, usize>     ← canonical + 每个 alias
├── triggers: Vec<CommandTrigger>            ← 模糊匹配的唯一输入
│     每个命令产出：canonical trigger + 每 alias 一个 trigger
│     技能额外产出 bare-suffix sibling（acme:login → 也发一条 login）
└── 三层闸门
      hidden      : 不出现在菜单，也不能执行（/dashboard /recap /voice /auto /memory /flush /dream /hooks /plugins）
      menu_hidden : 不出现在菜单，但完全类型化输入仍可执行（/share）
      restricted  : 仍出现在菜单与 ghost 里（保可发现性），但执行被拒
```

- `CommandTrigger` 的字段即渲染所需的一切：`canonical`、`alias`、`display`（形如 `/exit`）、`match_text`、`description`、`usage`、`takes_args`、`args_required`、`command_index`、`source`、`provenance`（`registry.rs:44-64`）。
- `bare_suffix_sibling()`：只有 skill 才产生裸后缀副本，且它是**独立 trigger**（不是拼接 haystack），目的是让分数与高亮下标保持诚实（`registry.rs:84-104`）。
- `BLOCKED_ACP_NAMES`：`help`、`hooks-*`、`reload-plugins` 等 shell 名字永不被 pager 菜单采用（`registry.rs:21-30`）。
- `SHELL_GATED_COMMANDS = ["memory","flush","dream"]`：跟随 shell 自己的门控，ACP 目录广告了才显形（`registry.rs:110`）。
- `rebuild_triggers()` 在每次门控变化后重建 trigger 列表；hidden 命令跳过，menu-hidden 命令保留 key 但不发 trigger，restricted 命令照常发 trigger（`registry.rs:512` 起）。

两个查询入口的差别是刻意设计的：

- `get()`：完整可见性判定，hidden + menu_hidden + restricted + 工具不足都返回 `None`（`registry.rs:187`）。
- `get_for_dispatch()`：只忽略 menu-only 一层，完全类型化的提交必须能到达 handler（`registry.rs:195`）。

### 1.4 按模式可用性：`ModeSupport`

模式闸门是一个带解药的枚举（`slash/mode_support.rs:19-23`）：

```rust
pub enum ModeSupport { Both, FullscreenOnly(Remedy), MinimalOnly(Remedy) }
pub enum Remedy { SwitchMode { why: &'static str }, UseInstead(&'static str), AlreadyInMode }
```

`supports()` 把 `Inline` 归入 fullscreen 一侧（`mode_support.rs:26-32`）。`refusal()` 负责把拒绝拼成给人看的一句话（`mode_support.rs:34-53`），有两条不同策略：能靠切换模式解决的，指向 `/fullscreen` 或 `/minimal`；本模式已有等价操作的，直接点名替代动作。

7 条内置命令受此限制（`grep mode_support: slash/commands/*.rs`）：

| 命令 | 方向 | 理由（原文） |
| --- | --- | --- |
| `/dashboard` | FullscreenOnly | minimal is single-session（`commands/dashboard.rs:26`） |
| `/find` | FullscreenOnly | minimal has no scrollback pane: use your terminal's own search（`commands/find.rs:19`） |
| `/jump` | FullscreenOnly | minimal scrolls with your terminal's native scrollback（`commands/jump.rs:13`） |
| `/theme` | FullscreenOnly | minimal renders with your terminal's own palette（`commands/theme.rs:35`） |
| `/timeline` | FullscreenOnly | the timeline rail needs the interactive scrollback pane（`commands/timeline.rs:16`） |
| `/tutorial` | FullscreenOnly | the tutorial overlay needs fullscreen（`commands/tutorial.rs:20`） |
| `/expand` | MinimalOnly | UseInstead("press Tab to focus the scrollback, then → on the block")（`commands/expand.rs:21`） |

`/minimal` 与 `/fullscreen` 自身用 `Remedy::AlreadyInMode`，形成「在你不需要它的那一边才出现」的对称（`slash/commands/screen_mode_switch.rs:57-63`）。拒绝文案只在两处产生：直接提交（`src/app/dispatch/prompt.rs:871`）与编辑排队行（`src/app/dispatch/queue.rs:1222`）。

`mode_support_tests.rs:94-101` 是一个「把所有模式的拒绝句钉在测试里」的穷举测试，测试名 `mode_specific_builtin_refusals_are_pinned`。

菜单里另有两层与模式正交的门：`hide_session_scoped`（dashboard 的 dispatch 输入里隐藏会话内命令）与 `dashboard_only`（`/cd` 反过来只在无会话面板出现），三者的与运算在 `command_offered()`（`slash/mod.rs:1285-1300`）。

## 2. 补全下拉：视觉与交互

### 2.1 布局（单列）

`views/slash_dropdown.rs` 与 `views/completion_dropdown.rs` 共用同一套几何常量：`PREFIX_W = 2`（`"❯ "` 或 `"  "`）、`LABEL_CAP = 40`、`LABEL_DESC_GAP = 2`（`views/slash_dropdown.rs:24-30`）。**是单列**，没有多列网格。

```
┌──────────────────── 提示符上方的下拉（宽 = 提示符区宽度） ─────────────────────┐
│❯ /model              Switch the active model                    built-in     │  ← 选中行：箭头 + BOLD + bg_visual
│  /mo…                (长名字按标签列宽截断为 …)                                │
│  /btw [new]          Ask a side question without interrupting                 │  ← tag 右对齐在标签列内，tab 后是描述
│  /acme:login         Acme account login                        skill · acme   │  ← provenance badge 右对齐到行尾
│  /history            Browse past prompts                                      │
│                                                                               │
│                                          ▐                                    │  ← 内容溢出时最右 1 列是滚动条
└───────────────────────────────────────────────────────────────────────────────┘
   标签列宽 = min(max(各行 display + " [tag]" 宽), min(内容宽 × 3/5, 40))
   描述从 (2 + 标签列宽 + 2) 列开始；过长描述按词换行，续行缩进到描述列
```

证据：标签列计算 `compute_label_column_w()`（`views/slash_dropdown.rs:55-63`）、行构造 `build_item_lines()`（`:294-395`）、badge 右对齐（`:374-383`）、滚动条（`:205-229`）。completion 版本几乎相同，但它**不换行、不显示 tag/badge**，只把描述截断（`views/completion_dropdown.rs:160-217`）。

- **候选数量上限**：可见行数 8（`MAX_VISIBLE_SUGGESTIONS = 8`，`slash/mod.rs:39`；被 `MAX_DROPDOWN_ROWS` 复用，`views/slash_dropdown.rs:21`）。这不是候选上限——空查询时 `command_suggestions()` 返回全部可见命令，由渲染层滚动（`slash/mod.rs:1021-1023` 注释「No cap here; the dropdown renderer handles scrolling」）。
- **高度按「行」而非「条目」计算**：`desired_item_rows()` 会数描述换行后的物理行数并饱和到 8，避免长描述把后面的候选挤出屏幕（`views/slash_dropdown.rs:32-40`，测试 `desired_item_rows_counts_wrapped_description_lines` 在 `:671`）。
- 选中行：箭头 + `bg_visual` + `Modifier::BOLD`；鼠标悬停：`bg_hover`（25% 混合）。Terminal 主题（fg 全为 Reset）改为叠加 `selection_overlay()/hover_overlay()` 反显（`views/slash_dropdown.rs:83-118`）。
- 匹配字符用 `theme.fuzzy_accent` 上色，相邻同类字符合并为一个 `Span`（`build_highlighted_spans()`，`views/slash_dropdown.rs:400-449`）。
- completion 下拉的可见行数是 6（`views/completion_dropdown.rs:18`），且带 1 行分隔符：`dropdown_height = 1 + items.min(6)`（`:29-35`）。

### 2.2 排序

**裸 `/`（空查询）用 `MenuKey`**（`slash/mod.rs:67-90`），字段按声明顺序比较：

```
1. group        Command < BundledSkill < OtherSkill      ← 技能一定沉在命令之下
2. untagged     false 在前                                ← 有 tag 的行被顶到本组最上
3. recency      MRU 衰减分降序（技能恒为 0）
4. name         技能按 display 小写字母序；命令组为空串（保留注册表顺序）
```

排序是稳定排序，所以同键的行回到 `builtin_commands()` 的顺序（`slash/mod.rs:1056-1068`）。

**带查询时按「分数 + 四级 tiebreak」**（`slash/mod.rs:1172-1193`）：

```
1. nucleo 分数降序
2. owns_typed_name：canonical/alias 精确等于所键入文本者优先
3. MRU 衰减分降序
4. Builtin 先于 Acp
5. display 升序
```

细节：一个命令可能有多条 trigger（alias、技能裸后缀），先按 `command_index` 去重，取胜规则是「分高 > 精确查询 > canonical 优于 alias > display 字典序」（`slash/mod.rs:1090-1121`）；`owns_typed_name` 这个 tiebreak 存在的理由写在 `slash/mod.rs:201-203`——避免 MRU 把同名 skill 顶到刚被完整键入的 builtin 之上。

**MRU 本体**（`slash/mru.rs`）：

- 存储：`$GROK_HOME/slash-mru.json`，扁平 `{命令名: last_used_unix秒}`，最多 256 条（`slash/mru.rs:27-29`、`:75-77`）。
- 平铺不按前缀分桶（旧 schema `by_prefix` 只在迁移时读一次，`slash/mru.rs:36-39`、`:131-140`）。
- 排序用的不是原始时间戳，而是衰减分：7 天半衰期，下限 0.1，使「很久以前用过」永远略高于「从没用过」（`slash/mru.rs:95-104`）：
  ```rust
  const RECENCY_HALF_LIFE_SECS: f64 = 7.0 * 86_400.0;
  const RECENCY_FLOOR: f64 = 0.1;
  let factor = (0.5_f64.powf(age / RECENCY_HALF_LIFE_SECS)).max(RECENCY_FLOOR);
  ```
- 写入路径完全离开 UI 线程：`touch` 只标脏，录制命令时交给唯一的长驻线程 `slash-mru-writer` 做「临时文件 + fsync + rename」（`slash/mru.rs:266-293`、`:240-261`）。读失败则本会话停用持久化，避免覆盖读不出来的文件（`slash/mru.rs:119-127`）。
- 每个 `SlashController` 持 `Rc<RefCell<SlashMru>>`，由 `AppView` 注入共享，agent 提示符与 dashboard dispatch 因此共用一份（`slash/mru.rs:6-8`、`slash/mod.rs:373`）。

### 2.3 键盘语义

三者共用一套「菜单打开时劫持方向键」的结构，但 Tab/Enter 的语义各自不同。slash 下拉（`src/app/agent_view/prompt.rs:158-235`）：

| 键 | 行为 |
| --- | --- |
| `↑` / `Ctrl-P` | 选择上移（环绕） |
| `↓` / `Ctrl-N` | 选择下移（环绕） |
| `PageUp` / `PageDown` | 按可见行数（8）整页滚动，两端夹紧 |
| `Tab` | **只接受文本，不执行**：`accept_slash_completion` 后继续编辑 |
| `Enter` | 三路分支，见下 |
| `Esc` | 回滚实时预览并关闭下拉 |

```
Enter 分支（prompt.rs:207-230）
├─ is_typed_slash_selected(...) 为真        → 提交预览、关闭下拉、直接发送
│   （光标在命令名内 && 命令完整 && 选中行就是所键入的命令或它的别名）
└─ 否则 → 接受补全
        ├─ 选中行 insert_text 以空格结尾（即该命令能接参数）→ 停在参数阶段，不发送
        └─ 否则 → 关闭下拉并发送
```

`accept_slash_completion()` 按光标所在相位改写不同区间：在命令名内替换 `command_range`（`/mo` → `/model `），在参数相位替换 `args_range`；行尾的尾随空格会「吸收」已有的空格，使光标落到分隔符之后（`views/prompt_widget/mod.rs:1336-1396`）。

`is_command_complete()` 是 Enter 的另一半契约（`slash/mod.rs:1423`）：`takes_args=false` 或 `args_required=false` 一律完整；`args_required=true` 时必须已有非空参数；**未注册的命令一律视为完整**，交由 shell `PassThrough`。

### 2.4 行内 ghost：下拉之外的第二条补全路径

光标在一个 `/token` 内、该 token 又尚未被认作完整命令时，快照会带一段 ghost 后缀，用 muted 颜色画在 token 之后（`views/prompt_widget/mod.rs:3197-3213`）。ghost 的来源不是独立匹配器，而是**当前下拉选中的那一行**（`sync_inline_ghost_to_selection()`，`slash/mod.rs:234-248`），因此 ghost 与 Tab 接受的结果永远一致。前缀匹配用与 nucleo `CaseMatching::Smart` 对齐的规则：查询全小写则忽略大小写，含大写则要求精确前缀（`command_prefix_matches_smart()`，`slash/mod.rs:163-180`）。

Tab 在没有下拉时接受 ghost：直接把 token 区间替换成 `/{full_name}`（`src/app/agent_view/prompt.rs:236-253`）。

### 2.5 第三个下拉：shell 命令补全

`views/completion_dropdown.rs` 服务的是 ACP `x.ai/suggest` 返回的补全（bash 模式），与 slash 无关：

- 6 行上限、无换行、无 tag/badge（`views/completion_dropdown.rs:18`）。
- 条目字段 `display` / `description` / `insert_text` / `source`（`None|History|PathExecutable|FilePath|AI`）/ `priority` / `replace_range` + `token_text` / `truncated`（`views/suggestion_controller/mod.rs:53-69`）。
- 响应到达时**按 wire 顺序直出**（`self.dropdown.items = response.completions`，`views/suggestion_controller/mod.rs:685`）——**未找到 pager 侧按 `priority` 排序的代码**；`priority` 由 shell 决定顺序。
- 下拉不会自动打开：「Don't auto-open; Tab opens it whenever items exist」（`views/suggestion_controller/mod.rs:686-687`）。
- 选择环绕（`move_selection`），鼠标滚动夹紧不环绕（`scroll_selection`）（`:235-253`）。

## 3. 模糊匹配算法与参数

slash 侧的匹配器是 nucleo 的薄封装 `FuzzyMatcher`（`slash/matcher.rs:14-31`）：

```rust
pub struct FuzzyMatcher { pattern: MultiPattern, matcher: Matcher }
// MultiPattern::new(1) —— 单列；Matcher::new(Config::DEFAULT)
pattern.reparse(0, trimmed, CaseMatching::Smart, Normalization::Smart, false);
```

| 项 | 取值 | 证据 |
| --- | --- | --- |
| 大小写 | `CaseMatching::Smart`（全小写查询忽略大小写，含大写则敏感） | `slash/matcher.rs:57`、`:104` |
| 归一化 | `Normalization::Smart` | 同上 |
| 列数 | 1（`MultiPattern::new(1)`） | `slash/matcher.rs:28` |
| 排序 | `rank()` 返回 `(index, score)`，按分数降序、同分按 key 文本升序；上限 `limit` | `slash/matcher.rs:74-77` |
| 空查询 | 退化为「前 limit 条、分数 0、插入序」 | `slash/matcher.rs:50-54` |
| 高亮下标 | `indices()` 返回**字符**位置（`Utf32String` 索引） | `slash/matcher.rs:86-95` |
| 匹配失败 | `indices_for()` 返回 `None`（例如 `"fix s"` 对 `"ssh-wrap"`） | `slash/matcher.rs:98-113`，测试 `:123-126` |

**路径分隔符处理**在参数补全那一层：`argument_highlight_indices()` 取查询的最后一个空白 token，再截掉最后一个 `/` 或 `\`，只拿 basename 去做高亮；若匹配失败，再用最后一个 `.` 之后的扩展名重试（`slash/mod.rs:1227-1239`）。命令名的匹配不做这种截断——匹配对象是 `trigger.match_text`，技能裸后缀走的是独立 trigger。

**CJK 相关**：

- 高亮下标是字符位置而非字节位置（`Utf32String`，`slash/matcher.rs:65`、`:91`）。
- 截断与列宽全部走 `unicode_width`，双宽字符按其显示宽度占列（`views/slash_dropdown.rs:343`、`:454-500`；file_search 侧同样处理宽字符续格，`views/file_search/dropdown.rs:205-215`）。
- 退化几何测试显式覆盖 CJK 与 emoji：条目含 `/日本語コマンド` 与泰文长词，断言 0–8 列宽、0–3 行高下既不 panic 也不死循环（`views/slash_dropdown.rs:636-667`）。
- 行内 token 扫描是**字节级**的：`/` 前一个字节必须是 ASCII 空白才认作 token（`slash/mod.rs:1603-1612`）。因此紧跟中文字符的 `/`（如 `你好/btw`）不会被识别为命令 token——这里要求的是 ASCII 空白，不是泛化的 `char::is_whitespace()`。

文件搜索侧用的是同一「家族」但独立的实现（`crates/codegen/xai-fuzzy-file-search/src/lib.rs`），参数不同：

| 项 | 取值 | 证据 |
| --- | --- | --- |
| 匹配器 | `Nucleo::new(Config::DEFAULT.match_paths(), …, Some(2), 1)` —— 面向路径匹配 | `lib.rs:197-206` |
| 线程 | nucleo 2 线程、ignore walker 8 线程 | `lib.rs:30-31` |
| 大小写/归一化 | `CaseMatching::Smart` + `Normalization::Smart` | `lib.rs:415` |
| 最小分 | `min_score = 7 + len * 14`（`len` = 查询字符数；注释引自 helix） | `lib.rs:479-481` |
| top-k 请求 | UI 侧要 1000 条 | `views/file_search/state.rs:19` |
| 排序 | 依次按 score、路径长度、路径名 | `lib.rs:454` 注释 + `sort_by_key_hrtb` |
| 遍历 | `git_ignore/git_global/git_exclude/ignore/hidden(true)`、`follow_links(false)`、`require_git(false)`，外加 `!.git` 覆盖规则 | `lib.rs:290-310` |
| 空查询浏览 | 串行 depth-1 walk，按文件名排序（可在 matcher 线程池被拒时退化工作） | `lib.rs:268-287` |
| 退化策略 | 线程池拒 → BrowseOnly（空查询可浏览、带 key 查询返回空）；daemon 线程拒 → Disabled（`MatcherMode`） | `lib.rs:38-50`、`:164-193` |

## 4. `@` 文件搜索

### 4.1 token 解析规则

`views/file_search/context.rs` 顶部把规则写死得很清楚（`:7-18`）：

- `@` 前一个字符不能是字母数字或下划线——否则 `user@example.com` 会被当成文件引用（`context.rs:71-76`）。
- token 从 `@` 延伸到第一个空白、逗号或分号（`context.rs:11-13`）。
- 光标必须落在 token 范围内；查询是 `@` 之后到光标之间的文本。
- **dir mode**：查询以 `/` 结尾 → 只匹配目录（`context.rs:33-35`）。
- **hidden mode**：查询以 `!` 开头 → 显示隐藏/gitignore 文件，并**重启遍历**（`context.rs:37-39`、`views/file_search/state.rs:205-243`）。
- `drill_prefix`：钻入目录后记住该目录路径，使其内部的空格不再终止 token（`my dir/` 保持一个 token），且一旦文本前缀不再匹配就自动失效（`context.rs:64-72`、`views/file_search/state.rs:196-215`）。

### 4.2 面板与交互

```
┌──────────────────── 提示符上方，最多 8 行结果 ────────────────────┐
│   src/views/slash_dropdown.rs                    123/4821       │ ← 顶线右侧是 k/n 计数（≥1000 显示 1k+/n）
├─────────────────────────────────────────────────────────────────┤
│❯ src/views/slash_dropdown.rs                                    │ ← 选中行：箭头 + BOLD + bg_visual
│  src/views/slash…（超宽截断为 …）                                 │
│  src/views/file_search/dropdown.rs/                              │ ← dir mode（查询以 / 结尾）给目录补尾部斜杠
└─────────────────────────────────────────────────────────────────┘
   匹配字符用 theme.fuzzy_accent；无图标、无文件类型列，只有路径
```

- 面板装饰（`Clear` + 上下横线 + 计数）由 `AgentView` 画，`views/file_search/dropdown.rs` 只画结果行（`src/app/agent_view/render.rs:2836-2903`；`dropdown.rs:20-22` 注释）。
- **无结果即隐藏整个面板**：`item_rows == 0` 时不渲染（`render.rs:2839`）；可见性判定是「有 @ 上下文 且 topk 非空」（`views/file_search/state.rs:153-155`）。
- 输入即搜 + 增量结果：每次文本/光标变化都 `update_context()` → `start_query()`（`daemon.set_query`），UI 每 tick 轮询；同一 query 只走 `Keep`，只有 hidden 模式切换才 `Restart` 遍历（`views/file_search/state.rs:137-150`、`:205-243`）。
- 防闪烁与过期防护：空结果不覆盖已有结果（除非 `status.done`），并用单调递增的 `min_generation` 挡掉上一轮 query 的迟到结果（`views/file_search/state.rs:257-300`、`:83-88`）。
- **没有多选**：选中是单个 `usize`（`views/file_search/state.rs:72`），接受操作写回一个替换区间（`FileSearchReplacement`，`:26-37`）。

键位（`views/prompt_widget/mod.rs:1946-1997`）：

| 键 | 行为 |
| --- | --- |
| `↑` / `Ctrl-P` / `Ctrl-K` | 上移 |
| `↓` / `Ctrl-N` / `Ctrl-J` | 下移 |
| `PageUp` / `Ctrl-U`、`PageDown` / `Ctrl-D` | 翻半页（`page_move(±1, 8)`） |
| `Tab` / `Enter` | 接受并补一个尾随空格 |
| `→` | 接受**不补空格**；若选中项是目录则「钻入」（替换路径、保持 @ 上下文活着） |
| `:` 或 `Ctrl-L` | 接受并用行内查看器打开（仅文件） |
| `Esc` | 关闭 |

`→` 与 `Tab` 的差别有明确注释：文件下面没有可钻的结构，所以对文件二者等价；对目录，`→` 才是下钻（`views/prompt_widget/mod.rs:2003-2043`）。

## 5. 历史搜索（`/history` 与 ↑）

### 5.1 两个入口，一条面板

`/history` 对应 `Action::OpenHistorySearch`（`slash/commands/history.rs:22`）。设计上**没有** Ctrl-R 之类的键：`src/actions/defaults.rs:687-688` 的注释直接写着「Prompt history has no key chord of its own: `/history` opens the search panel; Up on an empty prompt browses」。

两种模式共用一个覆盖层，靠 `Mode` 区分（`views/history_search.rs:263-272`）：

- **Search（`/history`）**：composer 成为过滤查询框，方向键只移动高亮，Enter/Tab 接受。打开时保存原文（`saved_text`），关闭时恢复。
- **Browse（空提示符按 ↑）**：每次移动**实时把选中项写进 composer**；一旦打字就脱离浏览转为编辑；在最新一条按 ↓ 关闭面板（`views/history_search.rs:385-397`、`:423-428`）。

### 5.2 面板外观与匹配

它是**覆盖在提示符上方的独立面板**，不是替换输入框：

```
┌──────────────────── 提示符上方 ────────────────────┐
│ history                                    37      │ ← 左上 " history "，右上结果数
├────────────────────────────────────────────────────┤
│  …（更早的条目）                                     │
│  fix the retry loop first. /btw what does WBC …     │
│❯ run the tests and report the failing cases         │ ← 选中：箭头 + BOLD + bg_visual
└────────────────────────────────────────────────────┘
   最多 8 行；空结果时第一行显示 "  no matching history"，加载中显示 "  Loading..."
   条目文本中的匹配字符以 fuzzy_accent 高亮
```

证据：面板高度 = `min(结果数, 8) + 2`，上下各一条横线（`src/app/agent_view/render.rs:2984-2992`、`:3018-3043`）；空结果文案与 `Loading...`（`render.rs:3060-3072`）；选择前缀与高亮（`:3080-3130`）。

- 匹配同样跑后台线程：`Daemon` 拥有 nucleo `Matcher` + `MultiPattern(1)`，UI 通过 channel 送 query、每 tick `poll()` 取 `Arc<Mutex<Snapshot>>`（`views/history_search.rs:1-23`、`:454-470`）。
- 结果上限 `MAX_RESULTS = 100`（`views/history_search.rs:55`）。
- 空查询时全部条目**倒序**（最近的在最后），并让选择粘在底部，直到用户主动导航（`stick_to_bottom`，`views/history_search.rs:279-290`、测试 `empty_query_returns_all_reversed_most_recent_last` 在 `:655`、`opens_with_most_recent_selected_at_bottom` 在 `:731`）。
- **循环浏览**：`↑/↓` 在两端**停住不环绕**（测试 `move_up_selects_earlier_then_stops_at_top` `:741`、`move_down_at_bottom_reports_end_instead_of_wrapping` `:755`）；真正环绕的是 `completion_dropdown` 的 `move_selection`（`views/suggestion_controller/mod.rs:235-242`）。滚动偏移是「选中项贴底」而非居中（`render.rs:3074-3079`）。
- 只在首次激活时才 spawn 守护线程，且每个 `PromptWidget` 一个——注释解释了为什么不能急切创建：每个 subagent 子视图都会永久占一个 parked 线程（`views/history_search.rs:11-16`）。

## 6. `mid_text_hoist`：句中出现 `/btw`

问题：提示符里写「先看错误处理，/btw WBC 是什么」时，命令不在开头，按常规语义应当作普通文本发给模型；但用户显然想要一次旁问。

方案：允许**个别命令**声明自己可以被「抬起」（hoist）到行首（`slash/mid_text_hoist.rs`）：

```rust
// prose /btw q   →   /btw prose q
let before = text.get(..token.range.start)?;
let after  = text.get(token.range.end..)?.trim_start();
let question = format!("{before}{after}").trim();
Some(format!("/{} {question}", command.name()))
```

（`slash/mid_text_hoist.rs:42-61`。）

- 只有 `can_hoist_from_mid_text() == true` 或 `is_skill()` 的命令能这样用（`mid_text_hoist.rs:15-17`）；实现该声明的内置命令**只有 `/btw`**（`slash/commands/btw.rs:18`）。
- 重写只在**整条消息不算命令**（不以 `/` 开头）时发生，且 token 只吞掉紧随其后的空白串，换行得以保留（`mid_text_hoist.rs:1-4`、`:52-54`）。
- 另一个安全阀：`token_is_armed_inline()` 决定一个 `/token` 是否被认作「可内联触发」——行首（前面只有空白）永远算，句中出现则要求 hoist 或 skill（`mid_text_hoist.rs:19-30`）。这也是 `recognized_token_ranges()`（青色高亮）的过滤条件（`slash/mod.rs:937-954`）。
- 端到端语义在用户手册里有一句概括：「`/btw` can also appear mid-message: the whole message (minus the token) becomes the side question and nothing goes to the main turn. Only `/btw` works this way」（`docs/user-guide/04-slash-commands.md:347`）。
- 与之相对的负面提示留给 `/goal`：`MID_TEXT_GOAL_NOTICE = "Slash commands only work at the start of the message — put \`/goal …\` first."`（`mid_text_hoist.rs:10-11`）。

它解决的体验问题是：**不必为了问一句旁支问题而清空或拆分手上的草稿**——把 token 插到句子里任意位置即可，代价是必须显式 opt-in，避免普通散文里的 `/word` 被误执行。

## 7. 键盘可发现性：没有补全时怎么找到命令

| 层次 | 机制 | 证据 |
| --- | --- | --- |
| 边打字边提示 | 下拉 + 行内 ghost（选中行的后缀）+ 参数占位符（如 `[context]` 以 muted 画在光标后） | `slash/mod.rs:234`、`views/prompt_widget/mod.rs:3177-3195` |
| 命令目录 | `/help` 打开命令面板（Ctrl+P 同一入口）；minimal 模式没有常驻快捷键脚注，`/help` 因此是官方发现入口并在状态行广告 | `slash/commands/help.rs:1-5` |
| 面板 | `Ctrl+P` 或 `?` 打开命令面板，每行显示键位或对应 slash 命令 | `docs/user-guide/03-keyboard-shortcuts.md:247-248`、`:396` |
| 全部快捷键 | `Ctrl+.` / `Ctrl+X` 打开快捷键速查（按 `Category` 分组，支持搜索与两级展开） | `src/views/shortcuts_help.rs:1-16` |
| 模式受限时的反馈 | `ModeSupport::refusal()` 给出「为什么 + 出路」的原话，如 `/theme isn't available in minimal mode (minimal renders with your terminal's own palette). Run /fullscreen to switch this session.` | `slash/mode_support.rs:34-53`、`docs/user-guide/04-slash-commands.md:160`，调用点 `src/app/dispatch/prompt.rs:871` |
| 参数缺失时的反馈 | 必填参数缺失时 Enter 不会发送，而是接受补全停在参数相位；真正执行时空参数由命令自己报错，如 `/model` 返回 `Usage: /model <name> [effort]` | `slash/commands/model.rs:56-58` |
| 未注册命令 | 不报错，原样 `PassThrough` 交给 shell/模型 | `src/app/dispatch/prompt.rs:885-887`、`slash/mod.rs:1430-1432` |
| 被拒命令仍可发现 | restricted 命令**照常出现在下拉/ghost/面板**（保可发现性），只是执行时改走套餐上行提示 | `slash/registry.rs:512` 起注释、测试 `restricted_commands_stay_visible_in_dropdown`（`slash/mod.rs:2563`） |
| 提交前拒绝 | `submission_refusal()` 在提交路径动 composer 之前返回原因（内置命令里只有 `/feedback` 覆写） | `slash/command.rs:231`、`slash/commands/feedback.rs:19` |
| 命名冲突可见 | 当 skill 与 builtin 撞裸名时，两行都带 provenance badge：`built-in` 与 `skill · <plugin>` | `slash/command.rs:200-209`、`views/slash_dropdown.rs:374-383` |

## 8. 与本项目（jev-harness）相关的取舍提示

- **命令与菜单条目分离**值得照搬：命令对象只描述「是什么」，trigger 描述「菜单里怎么显示、怎么被匹配」；这样别名、技能限定名（`acme:login`）与裸后缀补全可以在不改命令实现的前提下叠加（`slash/registry.rs:44`、`:84`）。
- **可见性用三档而不是一个 bool**：`hidden` / `menu_hidden` / `restricted` 分别对应「不存在」「不推荐但可用」「能看见但会被拒」，第三种刻意保留可发现性（`slash/registry.rs:512`）。
- **MRU 用衰减分而非原始时间戳或使用次数**，并在排序链里排在「精确名」之后，避免热命令压过刚被完整键入的命令（`slash/mru.rs:95-104`、`slash/mod.rs:1172-1180`）。
- **异步匹配一律走后台线程 + generation 栅栏 + 空结果不覆盖**，这套模式在文件搜索与历史搜索里各实现了一遍（`xai-fuzzy-file-search/src/lib.rs:38-50`、`views/history_search.rs:454-470`）。
- **模式相关的拒绝必须给「出路」**：`Remedy` 的两分法（切换模式 vs 用别的操作）比一句「不支持」信息量大得多（`slash/mode_support.rs:5-15`）。

## 关键结论（供合稿引用）

1. slash 命令是一个 ~25 个方法的 `SlashCommand` trait，元数据由 `slash_meta!` 宏只生成显式列出的 override；执行是同步的，异步一律通过 `CommandResult::Action` 交给 dispatch（`crates/codegen/xai-grok-pager/src/slash/command.rs:213`、`:354`、`:360`）。
2. 内置命令共 74 个，定义在单个 `builtin_commands()` vec 中；**分类即注释分组、菜单顺序即 vec 顺序**，这个顺序是空查询排序的最终 tiebreak（`slash/commands/mod.rs:77`、`:74-76`）。
3. 注册表把「命令对象」与「菜单条目」分离：每个 canonical/alias 生成一条 `CommandTrigger`，技能额外生成裸后缀 trigger，模糊匹配只作用于 trigger 的 `match_text`（`slash/registry.rs:44`、`:84`、`:512`）。
4. 可见性有三档闸门——`hidden`（不可见不可执行）、`menu_hidden`（不进菜单但类型化提交仍可执行，如 `/share`）、`restricted`（仍显示、执行时被拒）（`slash/registry.rs:147-158`、`:187`、`:195`）。
5. 按模式可用性由 `ModeSupport{Bolter,FullscreenOnly(Remedy),MinimalOnly(Remedy)}` 表达，共 7 条内置命令受限（6 条 fullscreen-only、`/expand` minimal-only），拒绝文案必带出路，`Inline` 归入 fullscreen 一侧（`slash/mode_support.rs:19-32`、`slash/commands/expand.rs:21`、`slash/mode_support_tests.rs:94`）。
6. 补全下拉是**单列**布局：标签列宽 = min(最长标签, min(内容宽×3/5, 40))，描述从固定列开始并按词换行，可见行数上限 8，溢出显示滚动条（`views/slash_dropdown.rs:21-30`、`:55-63`、`:294-395`）。
7. 排序分两条链：空查询用 `MenuKey`（分组 Command→BundledSkill→OtherSkill → 带 tag 优先 → MRU 衰减分 → 名称，稳定排序回落到注册表顺序）；有查询用「nucleo 分数 → 精确名 → MRU 衰减分 → builtin 优先 → display 升序」（`slash/mod.rs:67-90`、`:1056-1068`、`:1172-1193`）。
8. MRU 存 `$GROK_HOME/slash-mru.json`，扁平 `{命令: 时间戳}`、上限 256，排序用 7 天半衰期 + 0.1 下限的衰减分；写入走独立线程的临时文件 + fsync + rename（`slash/mru.rs:27-29`、`:95-104`、`:240-293`）。
9. 模糊匹配是 nucleo `MultiPattern(1)` 的薄封装，`CaseMatching::Smart` + `Normalization::Smart`，返回字符级高亮下标；参数补全的高亮只取最后一个空白 token 的 basename（按 `/`、`\` 切分，失败再试最后一个 `.` 的后缀）（`slash/matcher.rs:28-31`、`:57`、`:86-95`；`slash/mod.rs:1227-1239`）。
10. Tab/Enter 语义不同：Tab 只接受不执行；Enter 在「光标在命令名内且命令完整且选中行就是所键入命令」时直接发送，否则接受补全——若选中行可接参数（`insert_text` 以空格结尾）则停在参数相位，否则发送（`src/app/agent_view/prompt.rs:195-230`、`slash/mod.rs:1423`、`:1461`）。
11. `@` 文件搜索由后台 daemon 驱动「输入即搜 + 增量 top-k 1000」，用 `min_generation` 挡过期结果、空结果不覆盖以防闪烁；UI 只有单选，`Tab/Enter` 接受并补空格、`→` 接受不补空格并对目录下钻（`views/file_search/state.rs:19`、`:257-300`、`views/prompt_widget/mod.rs:1946-1997`）。
12. 历史搜索是提示符上方的独立覆盖面板，**没有 Ctrl-R 绑定**，入口是 `/history`（composer 兼作查询框）与空提示符按 `↑`（选中即时写回 composer）；匹配在后台线程做，上限 100 条，方向键两端停住不环绕（`src/actions/defaults.rs:687-688`、`views/history_search.rs:55`、`:263-272`、`src/app/agent_view/render.rs:2984-3000`）。
13. `mid_text_hoist` 让明确 opt-in 的命令在句中出现时被重写成行首调用（`prose /btw q` → `/btw prose q`），内置命令里只有 `/btw` 声明了该能力（skill 自动具备），且行首 `/` 永远优先（`slash/mid_text_hoist.rs:15-30`、`:42-61`、`slash/commands/btw.rs:18`）。
14. 没有补全时的发现路径是分层的：行内 ghost → `/help`（命令面板，minimal 模式的官方入口）→ `Ctrl+P`/`?` 命令面板 → `Ctrl+.`/`Ctrl+X` 快捷键速查；模式受限与被拒命令都会给出指名道姓的文本反馈而不是静默失败（`slash/commands/help.rs:1-5`、`docs/user-guide/03-keyboard-shortcuts.md:247-248`、`slash/mode_support.rs:34-53`、`src/app/dispatch/prompt.rs:885`）。
