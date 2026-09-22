//! 真调 smoke：默认 `#[ignore]`，只有显式加 `--ignored` 才会真的打网络。
//!
//! 每个端点只发**一次**真实请求（初始任务集的第一个样本，三个问题一次问完），
//! 并把判断记录落进 `var/calls.jsonl`。key 只经环境变量，测试里绝不打印 key。
//!
//! ```text
//! TYPESAFE_API_KEY=... cargo test -p jh-app --test live_smoke -- --ignored --nocapture
//!
//! OPENAI_API_KEY=... OPENAI_MODEL=deepseek-flash OPENAI_BASE_URL=https://api.deepseek.com \
//!   cargo test -p jh-app --test live_smoke -- --ignored --nocapture
//! ```
//!
//! 从仓库根跑 `cargo run -p jh-app -- exp` 时默认落在同一个 `var/calls.jsonl`。
//!
//! **缺 key 时这些测试显式失败**并写明「未验证」：不得用回放或模拟结果冒充实测
//! （`docs/RESEARCH_GOALS.md` §12 M0 的未达标处理）。

use std::path::{Path, PathBuf};
use std::time::Instant;

use jh_app::calls::{CallEntry, CallLog};
use jh_app::judge::{self, JudgeContext, JudgeName};
use jh_app::tasks::TaskSet;
use jh_core::JudgmentRecord;

/// 仓库根的 `var/`：`CARGO_MANIFEST_DIR` 是 `crates/jh-app`，往上两级即工作区根。
fn workspace_var() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/jh-app 之上是工作区根")
        .join("var")
}

/// 对指定判断器发一次真实请求：落记录、打印读数、与真值对照（单样本，只作信息）。
async fn smoke(name: JudgeName) {
    let task_set = TaskSet::load_initial().expect("内嵌任务集应自洽");
    let case = task_set.cases.first().expect("任务集应有样本");
    let request = task_set.request(case);
    let truth = &case.truth;

    println!("端点上配置（不含 key）：");
    println!(
        "  TYPESAFE_BASE_URL={:?}",
        std::env::var("TYPESAFE_BASE_URL").ok()
    );
    println!(
        "  OPENAI_BASE_URL={:?}",
        std::env::var("OPENAI_BASE_URL").ok()
    );
    println!("  OPENAI_MODEL={:?}", std::env::var("OPENAI_MODEL").ok());

    let calls_path = workspace_var().join("calls.jsonl");
    let context = JudgeContext {
        task_set: &task_set,
        replay_path: calls_path.as_path(),
    };
    let judge = match judge::build(name, &context) {
        Ok(judge) => judge,
        Err(error) => panic!(
            "{} 未验证：{error}（缺 key 时属预期；不得用回放或模拟结果替代实测）",
            name.cli_name()
        ),
    };

    let started = Instant::now();
    let response = match judge.judge(&request).await {
        Ok(response) => response,
        Err(error) => panic!(
            "{} 真实调用失败，本次结论保持未验证：{error}",
            name.cli_name()
        ),
    };
    let record = JudgmentRecord::from_response(&request, &response, started.elapsed())
        .expect("真实通道的响应必须满足判断器契约");

    let mut log = CallLog::open(calls_path.clone()).expect("调用记录应能打开");
    log.append(&CallEntry::Judgment {
        run: format!("smoke-{}", std::process::id()),
        case: case.id.clone(),
        record: Box::new(record.clone()),
    })
    .expect("调用记录应能落盘");

    println!("样本 {}", case.id);
    println!(
        "source={:?} version={} request_id={:?} latency={}ms usage={:?}",
        response.source, response.version, response.request_id, record.latency_ms, response.usage
    );
    for (question, judgment) in &record.judgments {
        let agreed = truth
            .get(question)
            .and_then(|truth| jh_app::metrics::score(&judgment.answer, truth))
            .map(|score| score.agreement);
        println!(
            "  {question}（v{}）：{:?}｜与真值一致={agreed:?}",
            judgment.question_version, judgment.answer
        );
    }
    println!("调用记录：{}", calls_path.display());
}

/// Jev 真调 smoke：`TYPESAFE_API_KEY`（模型 pin `jev-1.13.0`）。
#[tokio::test]
#[ignore = "需要 TYPESAFE_API_KEY 与真实网络：cargo test -p jh-app --test live_smoke -- --ignored"]
async fn jev_judge_sends_one_real_request() {
    smoke(JudgeName::Jev).await;
}

/// LLM 真调 smoke：`OPENAI_API_KEY` + `OPENAI_MODEL` + `OPENAI_BASE_URL`（三者都必填）。
#[tokio::test]
#[ignore = "需要 OPENAI_API_KEY / OPENAI_MODEL 与真实网络：cargo test -p jh-app --test live_smoke -- --ignored"]
async fn llm_judge_sends_one_real_request() {
    smoke(JudgeName::Llm).await;
}
