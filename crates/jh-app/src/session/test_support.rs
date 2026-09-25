//! 会话模块的单测支撑：临时目录、桩判定器、脚本模型（只在 `cfg(test)` 下编译）。
//!
//! 与仓库既有姿势一致（`crates/jh-app/tests/common/mod.rs` 的 `TempDir`、各判断器单测里的
//! 桩判定器），只是这里要给会话状态机用：判定与模型两条通道都能离线驱动。

use std::collections::VecDeque;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use jh_core::{Answer, ChoiceAnswer, Error, Judge, JudgeRequest, JudgeResponse, JudgeSource};
use serde_json::Value;
use tokio::sync::mpsc::UnboundedSender;

use crate::session::gate::{OPTION_ALLOW, OPTION_DENY};
use crate::session::model::{Model, ModelFuture, ModelRequest, ModelTurn, ToolCall};

/// 临时目录：建在系统临时目录下，`Drop` 时删掉。
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// 建一个唯一命名的临时目录。
    pub fn new(name: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!("jh-app-{name}-{nanos}"));
        std::fs::create_dir_all(&path).expect("建临时目录");
        TempDir { path }
    }

    /// 目录下某个文件的路径。
    pub fn join(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// 用一个单线程运行时把 future 跑完。
///
/// 工具执行有真的 IO（子进程、文件），不能用空 waker 顶（那会直接 panic 在 Pending 上）。
pub fn block_on<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("建单线程运行时")
        .block_on(future)
}

/// 桩判定器：永远给同一份二选一分布。
struct ChoiceStub {
    allow: f64,
    deny: f64,
    confidence: f64,
}

impl Judge for ChoiceStub {
    fn judge<'a>(
        &'a self,
        request: &'a JudgeRequest,
    ) -> Pin<Box<dyn Future<Output = Result<JudgeResponse, Error>> + Send + 'a>> {
        Box::pin(async move {
            let probabilities = std::collections::BTreeMap::from([
                (OPTION_ALLOW.to_owned(), self.allow),
                (OPTION_DENY.to_owned(), self.deny),
            ]);
            let choice = if self.allow >= self.deny {
                OPTION_ALLOW
            } else {
                OPTION_DENY
            };
            // 请求里问什么就答什么（真判定器也是这样）：`/jev` 的问题 ID 与门控不同。
            let answers = request
                .questions
                .keys()
                .map(|id| {
                    (
                        id.clone(),
                        Answer::Choice(ChoiceAnswer {
                            choice: choice.to_owned(),
                            probabilities: probabilities.clone(),
                            confidence: self.confidence,
                        }),
                    )
                })
                .collect();
            Ok(JudgeResponse {
                source: JudgeSource::RealApi,
                version: "stub-1".to_owned(),
                answers,
                usage: None,
                request_id: Some("req-stub".to_owned()),
            })
        })
    }
}

/// 桩判定器：永远失败。
struct FailingStub {
    message: String,
}

impl Judge for FailingStub {
    fn judge<'a>(
        &'a self,
        _request: &'a JudgeRequest,
    ) -> Pin<Box<dyn Future<Output = Result<JudgeResponse, Error>> + Send + 'a>> {
        Box::pin(async move { Err(Error::Unavailable(self.message.clone())) })
    }
}

/// 桩判定器：给一份「倾向允许」的分布。
pub fn choice_judge(allow: f64, deny: f64, confidence: f64) -> Arc<dyn Judge> {
    Arc::new(ChoiceStub {
        allow,
        deny,
        confidence,
    })
}

/// 桩判定器：永远以「不可用」失败。
pub fn failing_judge(message: &str) -> Arc<dyn Judge> {
    Arc::new(FailingStub {
        message: message.to_owned(),
    })
}

/// 脚本模型：按顺序吐出预设的回复，用完就以「不可用」失败。
pub struct Scripted {
    turns: Mutex<VecDeque<ModelTurn>>,
}

impl Scripted {
    /// 按给定顺序建脚本。
    pub fn new(turns: Vec<ModelTurn>) -> Arc<Self> {
        Arc::new(Scripted {
            turns: Mutex::new(VecDeque::from(turns)),
        })
    }
}

impl Model for Scripted {
    fn respond(&self, _request: ModelRequest, deltas: UnboundedSender<String>) -> ModelFuture {
        let turn = self.turns.lock().expect("脚本锁").pop_front();
        Box::pin(async move {
            match turn {
                Some(turn) => {
                    for chunk in turn.text.as_bytes().chunks(6) {
                        let _ = deltas.send(String::from_utf8_lossy(chunk).into_owned());
                    }
                    Ok(turn)
                }
                None => Err(Error::Unavailable("脚本用完了".to_owned())),
            }
        })
    }
}

/// 造一次模型回复。
pub fn turn(text: &str, calls: Vec<ToolCall>) -> ModelTurn {
    ModelTurn {
        text: text.to_owned(),
        calls,
        model: "stub-model".to_owned(),
        request_id: Some("resp-stub".to_owned()),
        usage: Some(jh_core::Usage {
            input_tokens: Some(100),
            output_tokens: Some(20),
        }),
        latency_ms: 7,
    }
}

/// 造一次工具调用。
pub fn call(name: &str, arguments: Value) -> ToolCall {
    ToolCall {
        call_id: format!("call-{name}-{}", arguments),
        name: name.to_owned(),
        arguments: arguments.to_string(),
    }
}
