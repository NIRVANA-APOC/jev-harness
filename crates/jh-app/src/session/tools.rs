//! 工具集：工作区内只读（读文件 / 列目录 / 搜内容）+ `bash` 命令执行。
//!
//! 设计来源：`docs/tui-design.md` §5.4 与 §2.6——**每个工具调用都要过审批卡**，
//! 所以工具表只需要如实描述「这个调用要做什么」，裁决权在门控与人工那里（[`crate::session::gate`]、
//! [`crate::session::approval`]）。
//!
//! 边界：
//!
//! - 三个只读工具的路径一律解析到工作区内（`canonicalize` 之后再比对前缀，指到工作区外的软链接
//!   同样拦下），越界显式失败；`search` 遍历时**逐个**解析并比对前缀，且**不跟随目录软链接**
//!   （`self_link -> .` 这类自环会让遍历永不结束），跳过的项在结果里如实计数；
//! - `bash` 以工作区为工作目录，但**不**限制它能读写什么——它的边界由审批卡划定，
//!   不靠代码假装兜住（`AGENTS.md`「禁止过度防御性编程」）。
//! - 工具执行失败**不是**宿主错误：失败原文作为结果回给模型（它要据此改主意），
//!   所以 [`Tools::execute`] 返回 [`ToolOutcome`] 而不是 `Result`。
//! - 子进程不继承本进程的 API key（`OPENAI_API_KEY` / `TYPESAFE_API_KEY` 从环境里摘掉）：
//!   key 只从环境读进内存，不该被一条命令打印进回滚区、更不该落进证据文件。

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use jh_openai::Tool;
use jh_tui::blocks::ToolStatus;
use serde::Deserialize;

use crate::session::model::ToolCall;

/// 读文件的默认行数上限（不给 `limit` 时）。
const READ_DEFAULT_LINES: usize = 200;
/// 读文件的单次行数硬上限：拦住「把整个文件塞进上下文」这种调用。
const READ_MAX_LINES: usize = 1000;
/// 搜索的默认命中上限。
const SEARCH_DEFAULT_HITS: usize = 50;
/// 搜索的命中硬上限。
const SEARCH_MAX_HITS: usize = 200;
/// 搜内容的文件遍历上限：工作区里有 `target/` 这类目录，不设上限会走成几分钟。
const SEARCH_MAX_FILES: usize = 5000;
/// 工具输出进上下文前的字符上限（超出部分截断并注明）。
const MAX_OUTPUT_CHARS: usize = 4000;
/// 遍历时跳过的目录：构建产物与本仓库的忽略目录。
const SKIPPED_DIRS: [&str; 3] = [".git", "target", "var"];
/// 子进程里要摘掉的环境变量。
///
/// 含本项目真调用的两个别名：会话侧走 `OPENAI_*`（本机由 `DEEPSEEK_API_KEY` 提供值），
/// 判定侧走 `TYPESAFE_API_KEY`——一条 `env` 就能把它们打进回滚区与证据文件，所以一律不传。
const SCRUBBED_ENV: [&str; 3] = ["OPENAI_API_KEY", "TYPESAFE_API_KEY", "DEEPSEEK_API_KEY"];

/// 只读工具：读文件。
pub const READ_FILE: &str = "read_file";
/// 只读工具：列目录。
pub const LIST_DIR: &str = "list_dir";
/// 只读工具：搜内容。
pub const SEARCH: &str = "search";
/// 副作用工具：执行一条 shell 命令。
pub const BASH: &str = "bash";

/// 一次工具执行的读回。
#[derive(Debug, Clone, PartialEq)]
pub struct ToolOutcome {
    /// 上屏用的状态。
    pub status: ToolStatus,
    /// 结果原文（已按上限截断）：成功是输出，失败是原因。
    pub text: String,
}

impl ToolOutcome {
    /// 执行成功。
    pub fn ok(text: impl Into<String>) -> Self {
        ToolOutcome {
            status: ToolStatus::Ok,
            text: text.into(),
        }
    }

    /// 执行失败（命令非零退出、参数不合契约、路径越界……）。
    pub fn failed(text: impl Into<String>) -> Self {
        ToolOutcome {
            status: ToolStatus::Failed,
            text: text.into(),
        }
    }
}

/// 工具表：只持有工作区根，可以克隆给宿主异步执行。
#[derive(Debug, Clone)]
pub struct Tools {
    root: PathBuf,
}

impl Tools {
    /// 建工具表。工作区根在这里解析成绝对路径（后续所有越界判断都按它比对）。
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, crate::Error> {
        let root = root.into();
        let root = root.canonicalize()?;
        Ok(Tools { root })
    }

    /// 工作区根（会话展示与工具展示用）。
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 交给模型的工具表。
    pub fn definitions(&self) -> Vec<Tool> {
        vec![
            Tool::function(
                READ_FILE,
                "读工作区里的一个文本文件，按行返回（可给 offset/limit）。路径相对工作区根。",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "相对工作区根的路径"},
                        "offset": {"type": "integer", "description": "起始行号（从 1 开始，缺省 1）"},
                        "limit": {"type": "integer", "description": "最多读多少行"}
                    },
                    "required": ["path"]
                }),
            ),
            Tool::function(
                LIST_DIR,
                "列出工作区里某个目录的直接子项（目录名带 `/` 后缀）。缺省列工作区根。",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "相对工作区根的目录，缺省 `.`"}
                    }
                }),
            ),
            Tool::function(
                SEARCH,
                "在工作区的文本文件里按子串搜索（区分大小写），返回 `路径:行号: 内容`。",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "pattern": {"type": "string", "description": "要查找的子串"},
                        "path": {"type": "string", "description": "在哪个目录下找，缺省整个工作区"},
                        "max_hits": {"type": "integer", "description": "最多返回多少条命中"}
                    },
                    "required": ["pattern"]
                }),
            ),
            Tool::function(
                BASH,
                "在工作区里执行一条 shell 命令（`sh -c`，工作目录是工作区根）。\
                 每次调用都要经用户授权：被拒绝时如实收到「已拒绝」，请换个做法而不是重复同一条命令。",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": {"type": "string", "description": "要执行的命令原文"}
                    },
                    "required": ["command"]
                }),
            ),
        ]
    }

    /// 一次调用的参数摘要（**不含**工具名：工具块与审批卡的命令行用它）。
    ///
    /// 工具块那一行由块自己画成 `▸ <工具名> <参数摘要>`，审批卡的标题也已经有工具名，
    /// 所以这里不再重复带名字（真调里踩过一次：块上写成 `▸ bash bash ls -1`）。
    pub fn detail(&self, call: &ToolCall) -> String {
        let args = self.args(call).unwrap_or_default();
        match call.name.as_str() {
            BASH => args.command.unwrap_or_default(),
            READ_FILE => match args.offset {
                Some(offset) => format!("{}（从第 {offset} 行起）", args.path.unwrap_or_default()),
                None => args.path.unwrap_or_default(),
            },
            LIST_DIR => args.path.unwrap_or_else(|| ".".to_owned()),
            SEARCH => match args.path {
                Some(path) => format!("{}（{}）", args.pattern.unwrap_or_default(), path),
                None => args.pattern.unwrap_or_default(),
            },
            _ => call.arguments.clone(),
        }
    }

    /// 一行完整摘要（工具名 + 参数摘要）：通知文案与证据的 `note` 用它。
    pub fn display(&self, call: &ToolCall) -> String {
        format!("{} {}", call.name, self.detail(call))
    }

    /// 执行一次调用。不认识的名字、参数不合契约、路径越界都以失败结果回给模型。
    pub async fn execute(&self, call: &ToolCall) -> ToolOutcome {
        let args = match self.args(call) {
            Ok(args) => args,
            Err(message) => return ToolOutcome::failed(message),
        };
        match call.name.as_str() {
            READ_FILE => self.read_file(&args),
            LIST_DIR => self.list_dir(&args),
            SEARCH => self.search(&args),
            BASH => self.bash(&args).await,
            other => ToolOutcome::failed(format!("没有名为 `{other}` 的工具")),
        }
    }

    fn args(&self, call: &ToolCall) -> Result<Args, String> {
        serde_json::from_str(&call.arguments)
            .map_err(|error| format!("参数不是合法 JSON（{}）：{}", call.arguments, error))
    }

    /// 把参数里的路径解析到工作区内：先解析成绝对路径，再看它是否落在工作区前缀下。
    fn resolve(&self, raw: &str) -> Result<PathBuf, String> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Ok(self.root.clone());
        }
        let joined = if Path::new(raw).is_absolute() {
            PathBuf::from(raw)
        } else {
            self.root.join(raw)
        };
        let resolved = joined
            .canonicalize()
            .map_err(|error| format!("路径不可读（{raw}）：{error}"))?;
        if !resolved.starts_with(&self.root) {
            return Err(format!("路径越出工作区：{raw}"));
        }
        Ok(resolved)
    }

    /// 展示用路径：工作区内用相对路径，根自己显示成 `.`。
    fn display_path(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .map(|relative| {
                if relative.as_os_str().is_empty() {
                    ".".to_owned()
                } else {
                    relative.display().to_string()
                }
            })
            .unwrap_or_else(|_| path.display().to_string())
    }

    fn read_file(&self, args: &Args) -> ToolOutcome {
        let Some(raw) = &args.path else {
            return ToolOutcome::failed("缺少参数 path");
        };
        let path = match self.resolve(raw) {
            Ok(path) => path,
            Err(message) => return ToolOutcome::failed(message),
        };
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) => return ToolOutcome::failed(format!("读不了 {raw}：{error}")),
        };
        let offset = args.offset.unwrap_or(1).max(1);
        let limit = args.limit.unwrap_or(READ_DEFAULT_LINES).min(READ_MAX_LINES);
        let lines: Vec<&str> = text.lines().collect();
        let total = lines.len();
        let mut out = String::new();
        for (index, line) in lines.iter().skip(offset - 1).take(limit).enumerate() {
            out.push_str(&format!("{:>5}\t{}\n", offset + index, line));
        }
        if out.is_empty() {
            return ToolOutcome::ok(format!(
                "（{} 共 {total} 行，第 {offset} 行起没有内容）",
                self.display_path(&path)
            ));
        }
        let shown = offset - 1 + out.lines().count();
        let mut text = format!(
            "{}（第 {}–{} 行，共 {} 行）\n{}",
            self.display_path(&path),
            offset,
            shown,
            total,
            out
        );
        truncate(&mut text);
        ToolOutcome::ok(text)
    }

    fn list_dir(&self, args: &Args) -> ToolOutcome {
        let raw = args.path.as_deref().unwrap_or(".");
        let path = match self.resolve(raw) {
            Ok(path) => path,
            Err(message) => return ToolOutcome::failed(message),
        };
        let entries = match std::fs::read_dir(&path) {
            Ok(entries) => entries,
            Err(error) => return ToolOutcome::failed(format!("列不了 {raw}：{error}")),
        };
        let mut names: Vec<String> = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
            names.push(if is_dir { format!("{name}/") } else { name });
        }
        names.sort();
        if names.is_empty() {
            return ToolOutcome::ok(format!("{}（空目录）", self.display_path(&path)));
        }
        let mut text = format!("{}：\n{}", self.display_path(&path), names.join("\n"));
        truncate(&mut text);
        ToolOutcome::ok(text)
    }

    fn search(&self, args: &Args) -> ToolOutcome {
        let Some(pattern) = &args.pattern else {
            return ToolOutcome::failed("缺少参数 pattern");
        };
        let start = match args.path.as_deref().map(|raw| self.resolve(raw)) {
            Some(Ok(path)) => path,
            Some(Err(message)) => return ToolOutcome::failed(message),
            None => self.root.clone(),
        };
        let max_hits = args
            .max_hits
            .unwrap_or(SEARCH_DEFAULT_HITS)
            .min(SEARCH_MAX_HITS);

        let mut hits: Vec<String> = Vec::new();
        let mut examined = 0usize;
        let mut skipped = 0usize;
        let mut outside = 0usize;
        let mut links = 0usize;
        let mut queue = VecDeque::from([start]);
        while let Some(directory) = queue.pop_front() {
            if examined >= SEARCH_MAX_FILES || hits.len() >= max_hits {
                break;
            }
            let Ok(entries) = std::fs::read_dir(&directory) else {
                skipped += 1;
                continue;
            };
            let mut entries: Vec<_> = entries.flatten().collect();
            entries.sort_by_key(|entry| entry.file_name());
            for entry in entries {
                // 上限按**项**算（文件与目录一起），否则目录不入队就等于没有上限。
                if examined >= SEARCH_MAX_FILES || hits.len() >= max_hits {
                    break;
                }
                examined += 1;
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();
                // 逐个解析再比对前缀：工作区里的符号链接同样拦下（只解析起点是不够的，
                // 一条 `link.txt -> /外面/secret` 就能把工作区外的内容搜出来）。
                let Ok(real) = path.canonicalize() else {
                    continue;
                };
                if !real.starts_with(&self.root) {
                    outside += 1;
                    continue;
                }
                let is_link = entry
                    .file_type()
                    .map(|kind| kind.is_symlink())
                    .unwrap_or(false);
                if real.is_dir() {
                    // **不跟随目录软链接**：`self_link -> .` 这种自环会让遍历永不结束
                    // （同步阻塞，Esc 都丢不掉），而目录软链接对「在文本文件里找子串」
                    // 没有别的用处——r1 定的口径本来就是「不跟随符号链接」。
                    if is_link {
                        links += 1;
                        continue;
                    }
                    if !SKIPPED_DIRS.contains(&name.as_str()) {
                        queue.push_back(real);
                    }
                    continue;
                }
                // 非 UTF-8（二进制）文件跳过：这里搜的是文本，不假装能搜二进制。
                let Ok(text) = std::fs::read_to_string(&real) else {
                    continue;
                };
                for (index, line) in text.lines().enumerate() {
                    if line.contains(pattern.as_str()) {
                        hits.push(format!(
                            "{}:{}: {}",
                            self.display_path(&real),
                            index + 1,
                            line.trim()
                        ));
                        if hits.len() >= max_hits {
                            break;
                        }
                    }
                }
            }
        }

        if hits.is_empty() {
            return ToolOutcome::ok(format!(
                "没有命中（子串 `{pattern}`，看了 {examined} 项{note}）",
                note = skip_note(skipped, outside, links, examined)
            ));
        }
        let count = hits.len();
        let capped = if count >= max_hits {
            "（已达上限）"
        } else {
            ""
        };
        let mut text = format!(
            "命中 {count} 条{capped}：\n{}{}",
            hits.join("\n"),
            skip_note(skipped, outside, links, examined)
        );
        truncate(&mut text);
        ToolOutcome::ok(text)
    }

    async fn bash(&self, args: &Args) -> ToolOutcome {
        let Some(command) = &args.command else {
            return ToolOutcome::failed("缺少参数 command");
        };
        let mut process = tokio::process::Command::new("sh");
        process
            .arg("-c")
            .arg(command)
            .current_dir(&self.root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // 会话被取消（Esc）时随 future 一起杀掉子进程，不留野进程。
            .kill_on_drop(true);
        for key in SCRUBBED_ENV {
            process.env_remove(key);
        }

        let output = match process.output().await {
            Ok(output) => output,
            Err(error) => return ToolOutcome::failed(format!("起不了子进程：{error}")),
        };
        let mut text = String::new();
        text.push_str(&String::from_utf8_lossy(&output.stdout));
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        if text.trim().is_empty() {
            text.push_str("（无输出）");
        }
        let code = match output.status.code() {
            Some(code) => format!("退出码 {code}"),
            None => "被信号终止".to_owned(),
        };
        let mut text = format!("{code}\n{text}");
        truncate(&mut text);
        if output.status.success() {
            ToolOutcome::ok(text)
        } else {
            ToolOutcome::failed(text)
        }
    }
}

/// 工具参数：四个工具共用一份宽松结构，缺字段由各自的检查负责。
#[derive(Debug, Default, Deserialize)]
struct Args {
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    pattern: Option<String>,
    #[serde(default)]
    max_hits: Option<usize>,
    #[serde(default)]
    command: Option<String>,
}

/// 按字符上限截断（截断处注明被截掉多少字符，不静默）。
fn truncate(text: &mut String) {
    if text.chars().count() <= MAX_OUTPUT_CHARS {
        return;
    }
    let head: String = text.chars().take(MAX_OUTPUT_CHARS).collect();
    let dropped = text.chars().count() - MAX_OUTPUT_CHARS;
    *text = format!("{head}\n…（已截断 {dropped} 个字符）");
}

/// 搜内容时跳过的东西：不可读目录、越出工作区的路径（含指向外面的软链接）、
/// 不跟随的目录软链接、以及走到遍历上限的情况。有跳过就如实说，没有就什么都不写。
fn skip_note(skipped: usize, outside: usize, links: usize, examined: usize) -> String {
    let mut parts = Vec::new();
    if skipped > 0 {
        parts.push(format!("跳过 {skipped} 个不可读目录"));
    }
    if outside > 0 {
        parts.push(format!("跳过 {outside} 个越出工作区的路径"));
    }
    if links > 0 {
        parts.push(format!("跳过 {links} 个目录软链接（不跟随）"));
    }
    if examined >= SEARCH_MAX_FILES {
        parts.push(format!("已到遍历上限 {SEARCH_MAX_FILES} 项"));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("，{}", parts.join("、"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tools() -> (Tools, crate::session::test_support::TempDir) {
        let dir = crate::session::test_support::TempDir::new("session-tools");
        let workspace = dir.join("workspace");
        std::fs::create_dir_all(workspace.join("src")).expect("建目录");
        std::fs::write(
            workspace.join("src/main.rs"),
            "fn main() {}\n// 端口 8080\n",
        )
        .expect("写文件");
        std::fs::write(workspace.join("README.md"), "jev-harness 示例\n").expect("写文件");
        // 工作区之外、临时目录之内的一个真文件：越界检查看的是前缀，不是「文件在不在」。
        std::fs::write(dir.join("outside.txt"), "外面的文件\n").expect("写文件");
        (Tools::new(&workspace).expect("工具表"), dir)
    }

    fn call(name: &str, arguments: serde_json::Value) -> ToolCall {
        ToolCall {
            call_id: "call-1".to_owned(),
            name: name.to_owned(),
            arguments: arguments.to_string(),
        }
    }

    #[test]
    fn every_tool_has_a_definition_and_a_display_line() {
        let (tools, _dir) = tools();
        let names: Vec<String> = tools
            .definitions()
            .iter()
            .map(|tool| tool.name().to_owned())
            .collect();
        assert_eq!(names, vec![READ_FILE, LIST_DIR, SEARCH, BASH]);
        assert_eq!(
            tools.display(&call(
                BASH,
                serde_json::json!({"command": "git status --short"})
            )),
            "bash git status --short"
        );
        assert_eq!(
            tools.display(&call(READ_FILE, serde_json::json!({"path": "src/main.rs"}))),
            "read_file src/main.rs"
        );
    }

    #[test]
    fn read_file_numbers_lines_and_honours_offset() {
        let (tools, _dir) = tools();
        let outcome = crate::session::test_support::block_on(tools.execute(&call(
            READ_FILE,
            serde_json::json!({"path": "src/main.rs", "offset": 2}),
        )));
        assert_eq!(outcome.status, ToolStatus::Ok);
        assert!(
            outcome.text.contains("第 2–2 行，共 2 行"),
            "{}",
            outcome.text
        );
        assert!(outcome.text.contains("端口 8080"), "{}", outcome.text);
    }

    #[test]
    fn paths_outside_the_workspace_are_refused() {
        let (tools, _dir) = tools();
        let outcome = crate::session::test_support::block_on(tools.execute(&call(
            READ_FILE,
            serde_json::json!({"path": "../outside.txt"}),
        )));
        assert_eq!(outcome.status, ToolStatus::Failed);
        assert!(outcome.text.contains("越出工作区"), "{}", outcome.text);
    }

    #[test]
    fn search_reports_path_line_and_content() {
        let (tools, _dir) = tools();
        let outcome = crate::session::test_support::block_on(
            tools.execute(&call(SEARCH, serde_json::json!({"pattern": "8080"}))),
        );
        assert_eq!(outcome.status, ToolStatus::Ok);
        assert!(outcome.text.contains("src/main.rs:2:"), "{}", outcome.text);
        assert!(outcome.text.contains("命中 1 条"), "{}", outcome.text);
    }

    #[test]
    #[cfg(unix)]
    fn search_does_not_follow_symlinks_out_of_the_workspace() {
        let (tools, dir) = tools();
        // 工作区外放一个带标记的文件，工作区内放一个指向它的软链接。
        std::fs::write(dir.join("outside.txt"), "SECRET-OUTSIDE-CONTENT\n").expect("写文件");
        std::os::unix::fs::symlink(dir.join("outside.txt"), tools.root().join("link.txt"))
            .expect("建软链接");

        let outcome = crate::session::test_support::block_on(tools.execute(&call(
            SEARCH,
            serde_json::json!({"pattern": "SECRET-OUTSIDE"}),
        )));
        assert_eq!(outcome.status, ToolStatus::Ok);
        // 「没有命中」的回文里会带上搜索词本身，所以断言的是**内容**没被搜出来。
        assert!(
            !outcome.text.contains("SECRET-OUTSIDE-CONTENT"),
            "{}",
            outcome.text
        );
        assert!(outcome.text.contains("没有命中"), "{}", outcome.text);
        assert!(outcome.text.contains("越出工作区"), "{}", outcome.text);

        // 拦截软链接不等于把搜索弄瘫：工作区内的文件照样搜得到。
        let inside = crate::session::test_support::block_on(
            tools.execute(&call(SEARCH, serde_json::json!({"pattern": "8080"}))),
        );
        assert!(inside.text.contains("src/main.rs:2:"), "{}", inside.text);
    }

    #[test]
    #[cfg(unix)]
    fn search_terminates_on_a_directory_symlink_loop() {
        let (tools, _dir) = tools();
        // 自环与指向祖先的环：`self_link -> .`、`sub/up_link -> 工作区根`。
        std::os::unix::fs::symlink(tools.root(), tools.root().join("self_link")).expect("建软链接");
        std::fs::create_dir_all(tools.root().join("sub")).expect("建目录");
        std::os::unix::fs::symlink(tools.root(), tools.root().join("sub/up_link"))
            .expect("建软链接");

        let outcome = crate::session::test_support::block_on(
            tools.execute(&call(SEARCH, serde_json::json!({"pattern": "8080"}))),
        );
        // 能返回（自环没有把遍历拖死），同一条命中只算一次，跳过的软链接如实报出来。
        assert_eq!(outcome.status, ToolStatus::Ok);
        assert_eq!(
            outcome.text.matches("src/main.rs:2:").count(),
            1,
            "{}",
            outcome.text
        );
        assert!(outcome.text.contains("目录软链接"), "{}", outcome.text);
        assert!(!outcome.text.contains("遍历上限"), "{}", outcome.text);
    }

    #[test]
    fn list_dir_marks_directories() {
        let (tools, _dir) = tools();
        let outcome = crate::session::test_support::block_on(
            tools.execute(&call(LIST_DIR, serde_json::json!({}))),
        );
        assert!(outcome.text.contains("src/"), "{}", outcome.text);
        assert!(outcome.text.contains("README.md"), "{}", outcome.text);
    }

    #[test]
    fn unknown_tools_and_broken_arguments_fail_explicitly() {
        let (tools, _dir) = tools();
        let unknown = crate::session::test_support::block_on(
            tools.execute(&call("rm_rf", serde_json::json!({"path": "/"}))),
        );
        assert_eq!(unknown.status, ToolStatus::Failed);
        assert!(unknown.text.contains("没有名为"), "{}", unknown.text);

        let broken = crate::session::test_support::block_on(tools.execute(&ToolCall {
            call_id: "call-2".to_owned(),
            name: BASH.to_owned(),
            arguments: "not json".to_owned(),
        }));
        assert_eq!(broken.status, ToolStatus::Failed);
        assert!(broken.text.contains("参数不是合法 JSON"), "{}", broken.text);
    }

    #[test]
    fn bash_reports_the_exit_code_and_scrubs_the_api_keys() {
        let (tools, _dir) = tools();
        unsafe {
            // 三个名字都要摘：会话侧的真 key 就在 DEEPSEEK_API_KEY 里。
            for name in SCRUBBED_ENV {
                std::env::set_var(name, "should-not-leak");
            }
        }
        let outcome = crate::session::test_support::block_on(tools.execute(&call(
            BASH,
            serde_json::json!({
                "command": "for v in OPENAI_API_KEY DEEPSEEK_API_KEY TYPESAFE_API_KEY; do \
                    eval \"value=\\${$v:-none}\"; [ \"$value\" = none ] || echo \"LEAK:$v\"; done; \
                    echo \"key=${OPENAI_API_KEY:-none}/${DEEPSEEK_API_KEY:-none}\"; pwd"
            }),
        )));
        assert_eq!(outcome.status, ToolStatus::Ok, "{}", outcome.text);
        assert!(outcome.text.contains("key=none/none"), "{}", outcome.text);
        assert!(
            !outcome.text.contains("should-not-leak"),
            "{}",
            outcome.text
        );
        // 名单里的每个名字都要断言（只写在数组里不算验证）。
        for name in SCRUBBED_ENV {
            assert!(
                !outcome.text.contains(name),
                "子进程不该看到 {name}：{}",
                outcome.text
            );
        }
        assert!(outcome.text.contains(&tools.root().display().to_string()));

        let failed = crate::session::test_support::block_on(
            tools.execute(&call(BASH, serde_json::json!({"command": "exit 3"}))),
        );
        assert_eq!(failed.status, ToolStatus::Failed);
        assert!(failed.text.contains("退出码 3"), "{}", failed.text);
    }
}
