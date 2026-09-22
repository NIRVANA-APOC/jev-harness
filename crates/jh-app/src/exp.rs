//! `jev exp` 的执行：逐个判断器跑完整任务集，落调用记录、算读数、汇成报告。
//!
//! 运行规则：
//!
//! - 一个样本一次判断调用（该样本的**全部问题批量发出**）——「一次多问」与「逐问」是同一个接口，
//!   由调用方决定（`docs/roadmap-1.0.md` §3.2）；
//! - 每个判断器按顺序跑完整个任务集；单次调用失败**不中断**本次运行，而是落一行失败记录并计数；
//! - 判断器构造失败（缺凭据、回放文件不存在）时该判断器整段标成「未验证」，不换后端顶上；
//! - 只要有任何判断器没跑通，本次运行的 `clean` 为 `false`，命令以非零码退出（错误显式可见）。
//!
//! # 构造顺序
//!
//! **全部判断器在打开调用记录之前构造**，然后才开始逐样本调用。这不是实现细节：回放在构造时
//! 一次性读入记录并建索引，构造顺序决定了它的索引里只有本次运行**之前**已落盘的行，
//! 因此同一次运行新写入的行（哪怕 `--calls` 与 `--replay` 是同一个文件，那也是默认值）
//! 不会被本次回放消费——回放不是自我实现的预言（`judge/replay.rs` 的契约）。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use jh_core::{Judge, JudgmentRecord, QuestionId, QuestionVersion};

use crate::calls::{CallEntry, CallLog};
use crate::error::Error;
use crate::judge::{self, JudgeContext, JudgeName};
use crate::metrics;
use crate::report::{
    CallStats, ExpReport, JudgeReport, LatencyStats, MetricDefinitions, QualityStats,
    QuestionQuality, TaskSetSummary, UsageStats, Verification, VerificationStatus,
};
use crate::tasks::{INITIAL_TASK_SET_SOURCE, TaskCase, TaskSet};

/// 一次 `jev exp` 运行的全部参数。
#[derive(Debug, Clone)]
pub struct RunOptions {
    /// 要跑的判断器，按顺序。
    pub judges: Vec<JudgeName>,
    /// 本次运行标识（写进调用记录的每一行）。
    pub run: String,
    /// 调用记录落盘路径（JSONL，追加）。
    pub calls_path: PathBuf,
    /// 录制回放读取的记录文件。
    pub replay_path: PathBuf,
    /// 复现命令，原样写进报告。
    pub command: String,
}

/// 运行结果。
#[derive(Debug, Clone)]
pub struct RunOutcome {
    /// 报告。
    pub report: ExpReport,
    /// 是否每个判断器都跑完了每个样本。
    pub clean: bool,
}

/// 新的运行标识：`run-<Unix 毫秒>`。
pub fn new_run_id() -> String {
    format!("run-{}", unix_ms())
}

/// 当前时刻的 Unix 毫秒。
pub fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("系统时钟早于 UNIX 纪元")
        .as_millis() as u64
}

/// 一个判断器的构造结果：名字 + 构造成败（失败者照旧出「未验证」报告）。
type PreparedJudge = (JudgeName, Result<Box<dyn Judge>, Error>);

/// 跑完整任务集并汇出报告；调用记录在运行中逐行落盘。
pub async fn run(task_set: &TaskSet, options: &RunOptions) -> Result<RunOutcome, Error> {
    let context = JudgeContext {
        task_set,
        replay_path: options.replay_path.as_path(),
    };

    // 先把全部判断器构造完，再打开调用记录。顺序是有意的：回放在构造时读入记录，
    // 这样它的索引至多包含本次运行之前已落盘的行，同一次运行新写的行不会被自己回放。
    let prepared: Vec<PreparedJudge> = options
        .judges
        .iter()
        .map(|name| (*name, judge::build(*name, &context)))
        .collect();

    let mut log = CallLog::open(options.calls_path.clone())?;
    let mut judges = Vec::with_capacity(prepared.len());
    let mut clean = true;

    for (name, judge) in prepared {
        let judge = match judge {
            Ok(judge) => judge,
            Err(error) => {
                clean = false;
                judges.push(unavailable_report(name, &error));
                continue;
            }
        };
        let mut accumulator = Accumulator::new(name);
        for case in &task_set.cases {
            let request = task_set.request(case);
            let started = Instant::now();
            let outcome = judge.judge(&request).await;
            let elapsed = started.elapsed();
            let latency_ms = elapsed.as_millis() as u64;

            match outcome {
                Ok(response) => match JudgmentRecord::from_response(&request, &response, elapsed) {
                    Ok(record) => {
                        log.append(&CallEntry::Judgment {
                            run: options.run.clone(),
                            case: case.id.clone(),
                            record: Box::new(record.clone()),
                        })?;
                        accumulator.succeeded(&record, case);
                    }
                    Err(error) => {
                        clean = false;
                        accumulator.failed(&error.to_string(), latency_ms);
                        log.append(&CallEntry::Failure {
                            run: options.run.clone(),
                            case: case.id.clone(),
                            source: name.source(),
                            latency_ms,
                            error: error.to_string(),
                        })?;
                    }
                },
                Err(error) => {
                    clean = false;
                    accumulator.failed(&error.to_string(), latency_ms);
                    log.append(&CallEntry::Failure {
                        run: options.run.clone(),
                        case: case.id.clone(),
                        source: name.source(),
                        latency_ms,
                        error: error.to_string(),
                    })?;
                }
            }
        }
        judges.push(accumulator.finish());
    }

    Ok(RunOutcome {
        report: ExpReport {
            run: options.run.clone(),
            generated_at_unix_ms: unix_ms(),
            command: options.command.clone(),
            task_set: TaskSetSummary {
                name: task_set.name.clone(),
                source: INITIAL_TASK_SET_SOURCE.to_owned(),
                cases: task_set.cases.len(),
                questions: task_set.questions.len(),
            },
            metric_definitions: MetricDefinitions::declared(),
            judges,
            calls_log: absolute(&options.calls_path),
            notes: ExpReport::declared_notes(),
        },
        clean,
    })
}

/// 构造失败时该判断器的报告：零调用，状态「未验证」，原因写清。
fn unavailable_report(name: JudgeName, error: &Error) -> JudgeReport {
    JudgeReport {
        judge: name.cli_name().to_owned(),
        source: name.source(),
        live: name.live(),
        verification: Verification {
            status: VerificationStatus::NotVerified,
            detail: format!("判断器未构造成功：{error}"),
        },
        versions: Vec::new(),
        calls: CallStats {
            attempted: 0,
            succeeded: 0,
            failed: 0,
            errors: BTreeMap::new(),
        },
        latency_ms: LatencyStats {
            samples: 0,
            min: None,
            p50: None,
            p95: None,
            max: None,
        },
        usage: UsageStats {
            calls_with_usage: 0,
            input_tokens: None,
            output_tokens: None,
        },
        quality: QualityStats {
            judgments: 0,
            agreements: 0,
            agreement_rate: None,
            unscored: 0,
            boundary_readings: 0,
            brier_mean: None,
            log_loss_mean: None,
            log_loss_undefined: 0,
            score_mae: None,
            score_mae_samples: 0,
            per_question: Vec::new(),
        },
    }
}

/// 一次判分的原始读数，按问题分列时再聚合。
#[derive(Debug, Clone)]
struct QuestionOutcome {
    question: QuestionId,
    version: QuestionVersion,
    score: metrics::JudgmentScore,
}

/// 一个判断器在整个任务集上的累加器。
struct Accumulator {
    name: JudgeName,
    attempted: u32,
    succeeded: u32,
    failed: u32,
    errors: BTreeMap<String, u32>,
    latencies_ms: Vec<u64>,
    versions: BTreeSet<String>,
    usage_calls: u32,
    input_tokens: u64,
    output_tokens: u64,
    /// 是否见过任何一次用量上报；没有上报时报告里保持 `null`。
    usage_seen: bool,
    outcomes: Vec<QuestionOutcome>,
    unscored: u32,
}

impl Accumulator {
    fn new(name: JudgeName) -> Self {
        Self {
            name,
            attempted: 0,
            succeeded: 0,
            failed: 0,
            errors: BTreeMap::new(),
            latencies_ms: Vec::new(),
            versions: BTreeSet::new(),
            usage_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            usage_seen: false,
            outcomes: Vec::new(),
            unscored: 0,
        }
    }

    /// 记一次成功的判断：判分、累计用量与版本。
    fn succeeded(&mut self, record: &JudgmentRecord, case: &TaskCase) {
        self.attempted += 1;
        self.succeeded += 1;
        self.latencies_ms.push(record.latency_ms);
        self.versions.insert(record.version.clone());

        if let Some(usage) = &record.usage {
            self.usage_calls += 1;
            self.usage_seen = true;
            self.input_tokens += usage.input_tokens.unwrap_or(0);
            self.output_tokens += usage.output_tokens.unwrap_or(0);
        }

        for (question, judgment) in &record.judgments {
            let Some(truth) = case.truth.get(question) else {
                // 任务集校验保证每个问题都有真值；真缺了就是任务集问题，在这里显式暴露。
                self.unscored += 1;
                continue;
            };
            match metrics::score(&judgment.answer, truth) {
                Some(score) => self.outcomes.push(QuestionOutcome {
                    question: question.clone(),
                    version: judgment.question_version,
                    score,
                }),
                None => self.unscored += 1,
            }
        }
    }

    /// 记一次失败：计时与错误摘要都留下。
    fn failed(&mut self, error: &str, latency_ms: u64) {
        self.attempted += 1;
        self.failed += 1;
        self.latencies_ms.push(latency_ms);
        *self.errors.entry(error.to_owned()).or_insert(0) += 1;
    }

    fn finish(self) -> JudgeReport {
        let quality = self.quality();
        let verification = self.verification();

        JudgeReport {
            judge: self.name.cli_name().to_owned(),
            source: self.name.source(),
            live: self.name.live(),
            verification,
            versions: self.versions.into_iter().collect(),
            calls: CallStats {
                attempted: self.attempted,
                succeeded: self.succeeded,
                failed: self.failed,
                errors: self.errors,
            },
            latency_ms: LatencyStats {
                samples: self.latencies_ms.len() as u32,
                min: self.latencies_ms.iter().copied().min(),
                p50: metrics::percentile(&self.latencies_ms, 0.5),
                p95: metrics::percentile(&self.latencies_ms, 0.95),
                max: self.latencies_ms.iter().copied().max(),
            },
            usage: UsageStats {
                calls_with_usage: self.usage_calls,
                input_tokens: self.usage_seen.then_some(self.input_tokens),
                output_tokens: self.usage_seen.then_some(self.output_tokens),
            },
            quality,
        }
    }

    /// 验证状态。
    ///
    /// - 真实通道：有成功调用才是「已验证」；一次都没成功时保持「未验证」，写明尝试次数；
    /// - 本地判断器：有成功读数记「模拟」（只作对照与上限），一次都没成功则记「未验证」——
    ///   失败就是没有读数，不能拿「模拟」的标签掩盖它。
    fn verification(&self) -> Verification {
        let failed_note = if self.failed > 0 {
            format!("，另失败 {} 次", self.failed)
        } else {
            String::new()
        };

        match (self.name.live(), self.succeeded > 0) {
            (true, true) => Verification {
                status: VerificationStatus::Verified,
                detail: format!("真实调用成功 {} 次{failed_note}", self.succeeded),
            },
            (true, false) => Verification {
                status: VerificationStatus::NotVerified,
                detail: format!(
                    "真实调用未成功（{} 次尝试全部失败）：相关结论保持未验证",
                    self.attempted
                ),
            },
            (false, true) => Verification {
                status: VerificationStatus::Simulated,
                detail: format!("本地判断器：没有真实调用，读数只作对照与上限估计{failed_note}"),
            },
            (false, false) => Verification {
                status: VerificationStatus::NotVerified,
                detail: format!(
                    "本地判断器 {} 次调用全部失败：本次没有任何可用读数",
                    self.attempted
                ),
            },
        }
    }

    fn quality(&self) -> QualityStats {
        let judgments = self.outcomes.len() as u32;
        let agreements = self.outcomes.iter().filter(|o| o.score.agreement).count() as u32;
        let boundaries = self.outcomes.iter().filter(|o| o.score.boundary).count() as u32;
        let undefined = self
            .outcomes
            .iter()
            .filter(|o| o.score.log_loss.is_none())
            .count() as u32;

        let mean = |values: Vec<f64>| -> Option<f64> {
            (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
        };

        let score_mae_values: Vec<f64> = self
            .outcomes
            .iter()
            .filter_map(|o| o.score.score_mae)
            .collect();

        let mut by_question: BTreeMap<(QuestionId, QuestionVersion), Vec<&QuestionOutcome>> =
            BTreeMap::new();
        for outcome in &self.outcomes {
            by_question
                .entry((outcome.question.clone(), outcome.version))
                .or_default()
                .push(outcome);
        }

        QualityStats {
            judgments,
            agreements,
            agreement_rate: (judgments > 0).then(|| f64::from(agreements) / f64::from(judgments)),
            unscored: self.unscored,
            boundary_readings: boundaries,
            brier_mean: mean(self.outcomes.iter().map(|o| o.score.brier).collect()),
            log_loss_mean: mean(
                self.outcomes
                    .iter()
                    .filter_map(|o| o.score.log_loss)
                    .collect(),
            ),
            log_loss_undefined: undefined,
            score_mae: mean(score_mae_values.clone()),
            score_mae_samples: score_mae_values.len() as u32,
            per_question: by_question
                .into_iter()
                .map(|((question, version), outcomes)| QuestionQuality {
                    question,
                    version,
                    judgments: outcomes.len() as u32,
                    agreements: outcomes.iter().filter(|o| o.score.agreement).count() as u32,
                    brier_mean: mean(outcomes.iter().map(|o| o.score.brier).collect()),
                    log_loss_mean: mean(
                        outcomes
                            .iter()
                            .filter_map(|o| o.score.log_loss)
                            .collect::<Vec<_>>(),
                    ),
                    log_loss_undefined: outcomes
                        .iter()
                        .filter(|o| o.score.log_loss.is_none())
                        .count() as u32,
                })
                .collect(),
        }
    }
}

/// 相对路径转绝对路径（用于报告里的原始数据位置）。
fn absolute(path: &Path) -> String {
    if path.is_absolute() {
        return path.display().to_string();
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(path).display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}
