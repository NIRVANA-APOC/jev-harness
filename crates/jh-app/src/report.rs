//! JSON 基线报告：质量、延迟、用量、口径与复现信息。
//!
//! 报告是 M0 的交付物本体（`docs/roadmap-1.0.md` §7.2 工程侧：`jev exp` 能跑完至少一个任务族并产出
//! JSON 报告，报告含原始数据位置与复现命令）。字段分三类：
//!
//! - **读数**：[`QualityStats`]（一致率、Brier、对数损失、score 期望值误差）、[`LatencyStats`]、
//!   [`UsageStats`] 与 [`CallStats`]；
//! - **口径**：[`MetricDefinitions`]，与 [`crate::metrics`] 的实现一一对应，读数离开口径不可解释；
//! - **可核查性**：调用记录路径、复现命令、任务集来源、每个判断器的**验证状态**
//!   （[`VerificationStatus`]）——无 key 时真实通道一律是「未验证」，绝不写成模拟的读数。
//!
//! 价格不在报告里：仓库没有权威价格表，报告只报 token 用量与调用次数，不折算成本
//! （`docs/roadmap-1.0.md` §7.2：不虚构价格表）。

use std::collections::BTreeMap;
use std::path::Path;

use jh_core::{JudgeSource, QuestionId, QuestionVersion};
use serde::Serialize;

use crate::error::Error;

/// 报告里的口径声明；文字与 [`crate::metrics`]、[`crate::judge`] 的实现一一对应。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MetricDefinitions {
    /// 隐藏真值的定义。
    pub truth: String,
    /// 一致率的读出规则。
    pub agreement: String,
    /// Brier 分数的公式与范围。
    pub brier: String,
    /// 对数损失的公式与无定义时的处理。
    pub log_loss: String,
    /// score 期望值误差的定义。
    pub score_mae: String,
    /// 延迟分位数的算法。
    pub latency: String,
    /// `confidence` 的推导口径。
    pub confidence: String,
    /// 成本口径（只报用量，不折算价格）。
    pub cost: String,
}

impl MetricDefinitions {
    /// 本项目声明的口径文本。
    pub fn declared() -> Self {
        Self {
            truth:
                "隐藏真值 = 该合成样本的预设标签（choice 记选项名，score 记级号，noul 记是 / 否），\
                    打分时等价于 one-hot 概率标签。真值只用于打分，不进入任何判断请求。"
                    .to_owned(),
            agreement:
                "一致 = 答案的读出标签等于真值标签。读出规则：choice 取答案里的 choice 字段；\
                        score 取概率最大的级号（并列取最小级号）；noul 取 noul ≥ 0.5 为「是」。\
                        边界读数（noul 恰为 0.5，或分布里存在并列最大概率）单独计数。"
                    .to_owned(),
            brier: "Brier = Σ_i (p_i − t_i)²，i 遍历该题答案标签与真值标签的并集，缺的按 0 计。\
                    noul 也按 true / false 两个标签求和，因此上界为 2；报告不使用二值预测口径，\
                    两种口径不混用。"
                .to_owned(),
            log_loss: "对数损失 = −ln p_真值（自然对数）。答案给真值标签分配 0 概率时没有有限值，\
                       报告记 null 并单独计数，不塞封顶值。"
                .to_owned(),
            score_mae: "score_mae = |期望值 − 真值级号| 的均值，只统计 score 题。\
                        等级是序数，只作诊断，不当等距效用或经济收益相加。"
                .to_owned(),
            latency: "延迟分位数用最近秩法：把全部调用（含失败调用）的耗时升序排列，\
                      p 分位取第 ⌈p·n⌉ 个。失败调用不排除在计时之外，另按错误计数与摘要列出。"
                .to_owned(),
            confidence:
                "LLM 与规则判断器的 confidence = 1 − H(p) / ln(支撑集大小)，H 为自然对数熵，\
                         支撑集是该问题声明的选项数 / 级数；只有一项时为 1。Jev 通道的 confidence \
                         由服务端定义，原样透传。"
                    .to_owned(),
            cost: "只报 token 用量与调用次数：仓库没有权威价格表，不折算金额，\
                   也不把未上报的用量写成 0。"
                .to_owned(),
        }
    }
}

/// 任务集摘要。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskSetSummary {
    /// 任务集名字。
    pub name: String,
    /// 任务集在仓库里的位置。
    pub source: String,
    /// 样本数。
    pub cases: usize,
    /// 问题数。
    pub questions: usize,
}

/// 判断器的验证状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    /// 真实通道调用成功，本次运行确有实测数据。
    Verified,
    /// 真实通道没有跑通（缺凭据、调用失败、构造失败）：相关结论保持未验证。
    NotVerified,
    /// 本地判断器（规则 / 回放 / 模拟真值）：没有真实调用，读数只作对照与上限。
    Simulated,
}

/// 验证状态与依据。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Verification {
    /// 状态。
    pub status: VerificationStatus,
    /// 依据说明（未验证时写明失败原因）。
    pub detail: String,
}

/// 调用计数与错误摘要。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CallStats {
    /// 发起的调用数（含失败）。
    pub attempted: u32,
    /// 成功数。
    pub succeeded: u32,
    /// 失败数。
    pub failed: u32,
    /// 失败文本 → 次数。
    pub errors: BTreeMap<String, u32>,
}

/// 延迟分位数（毫秒，全部调用都计时）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LatencyStats {
    /// 样本数。
    pub samples: u32,
    /// 最小值。
    pub min: Option<u64>,
    /// 中位数。
    pub p50: Option<u64>,
    /// 95 分位。
    pub p95: Option<u64>,
    /// 最大值。
    pub max: Option<u64>,
}

/// token 用量：只报通道上报过的。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UsageStats {
    /// 上报了用量的调用数。
    pub calls_with_usage: u32,
    /// 输入 token 总数（未上报过时为 `null`）。
    pub input_tokens: Option<u64>,
    /// 输出 token 总数（未上报过时为 `null`）。
    pub output_tokens: Option<u64>,
}

/// 单个问题的质量读数。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct QuestionQuality {
    /// 问题 ID。
    pub question: QuestionId,
    /// 判分时看到的问题版本。
    pub version: QuestionVersion,
    /// 判分数。
    pub judgments: u32,
    /// 一致数。
    pub agreements: u32,
    /// Brier 均值。
    pub brier_mean: Option<f64>,
    /// 对数损失均值（有定义的那些）。
    pub log_loss_mean: Option<f64>,
    /// 对数损失无定义的判分数。
    pub log_loss_undefined: u32,
}

/// 质量读数：总体与按问题（问题族 / 版本）分列（`docs/RESEARCH_GOALS.md` §10.2）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct QualityStats {
    /// 判分数。
    pub judgments: u32,
    /// 一致数。
    pub agreements: u32,
    /// 一致率；没有判分时为 `null`。
    pub agreement_rate: Option<f64>,
    /// 读不出标签、无法判分的判分数（任务集缺真值时也计入这里）。
    pub unscored: u32,
    /// 边界读数数。
    pub boundary_readings: u32,
    /// Brier 均值。
    pub brier_mean: Option<f64>,
    /// 对数损失均值。
    pub log_loss_mean: Option<f64>,
    /// 对数损失无定义的判分数。
    pub log_loss_undefined: u32,
    /// score 期望值误差均值（没有 score 判分时为 `null`）。
    pub score_mae: Option<f64>,
    /// 参与 score 期望值误差的判分数。
    pub score_mae_samples: u32,
    /// 按问题（族）分列的读数。
    pub per_question: Vec<QuestionQuality>,
}

/// 一个判断器在本次运行里的全部读数。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JudgeReport {
    /// CLI 里的判断器名字。
    pub judge: String,
    /// 声明的来源标签。
    pub source: JudgeSource,
    /// 是否属于真实通道。
    pub live: bool,
    /// 验证状态。
    pub verification: Verification,
    /// 记录里出现过的执行者版本（去重排序）。
    pub versions: Vec<String>,
    /// 调用计数。
    pub calls: CallStats,
    /// 延迟分位数。
    pub latency_ms: LatencyStats,
    /// token 用量。
    pub usage: UsageStats,
    /// 质量读数。
    pub quality: QualityStats,
}

/// `jev exp` 的 JSON 基线报告。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExpReport {
    /// 本次运行标识。
    pub run: String,
    /// 生成时刻（Unix 毫秒，UTC，无时区歧义）。
    pub generated_at_unix_ms: u64,
    /// 复现命令（原样记录本次调用的命令行，不含环境变量值）。
    pub command: String,
    /// 任务集摘要。
    pub task_set: TaskSetSummary,
    /// 口径声明。
    pub metric_definitions: MetricDefinitions,
    /// 每个判断器的读数。
    pub judges: Vec<JudgeReport>,
    /// 调用记录文件（绝对路径，原始数据位置）。
    pub calls_log: String,
    /// 注意事项（口径之外的边界说明）。
    pub notes: Vec<String>,
}

impl ExpReport {
    /// 报告正文的注意事项：与本次运行的具体读数无关的边界说明。
    pub fn declared_notes() -> Vec<String> {
        vec![
            "无 key 时 Jev / LLM 判断器的结论一律标「未验证」，不得用回放或模拟结果替代（研究目标 §12 M0）。"
                .to_owned(),
            "模拟真值与规则分布只作诊断与上限估计，不代表真实 API 的行为（研究目标 §9.2、§10.2）。"
                .to_owned(),
            "全部判断器在打开调用记录之前构造；回放在构造时一次性读入记录，因此不回放本次运行新写入的行。".to_owned(),
            "延迟与用量都按全部调用统计，失败调用不排除在计时之外。".to_owned(),
            "复现：在仓库根用同一子命令与参数重跑（如 `cargo run -p jh-app -- exp`）；\
             command 记录的是本次运行的完整命令行，环境变量不入报告。"
                .to_owned(),
        ]
    }

    /// 人读摘要：一行一个判断器，末尾给出记录与复核入口。
    pub fn summary_lines(&self) -> Vec<String> {
        let mut lines = vec![format!(
            "任务集 {}：{} 个样本 × {} 个问题（{}）",
            self.task_set.name, self.task_set.cases, self.task_set.questions, self.task_set.source
        )];

        for judge in &self.judges {
            let quality = &judge.quality;
            let agreement = match quality.agreement_rate {
                Some(rate) => format!(
                    "{:.3}（{}/{}）",
                    rate, quality.agreements, quality.judgments
                ),
                None => "无判分".to_owned(),
            };
            let brier = quality
                .brier_mean
                .map(|value| format!("{value:.4}"))
                .unwrap_or_else(|| "-".to_owned());
            let log_loss = quality
                .log_loss_mean
                .map(|value| format!("{value:.4}"))
                .unwrap_or_else(|| "-".to_owned());
            let usage = match (judge.usage.input_tokens, judge.usage.output_tokens) {
                (None, None) => "未上报".to_owned(),
                (input, output) => format!(
                    "输入 {} / 输出 {}（{} 次调用上报）",
                    input.map_or("-".to_owned(), |value| value.to_string()),
                    output.map_or("-".to_owned(), |value| value.to_string()),
                    judge.usage.calls_with_usage
                ),
            };

            lines.push(format!(
                "判断器 {}（source={:?}，versions={}）：调用 {} 成功 {} 失败 {}｜一致率 {}｜Brier {}｜对数损失 {}｜延迟 p50 {}ms p95 {}ms｜usage {}｜{}",
                judge.judge,
                judge.source,
                if judge.versions.is_empty() {
                    "-".to_owned()
                } else {
                    judge.versions.join(",")
                },
                judge.calls.attempted,
                judge.calls.succeeded,
                judge.calls.failed,
                agreement,
                brier,
                log_loss,
                milliseconds(judge.latency_ms.p50),
                milliseconds(judge.latency_ms.p95),
                usage,
                judge.verification.detail
            ));
        }

        lines.push(format!("调用记录：{}", self.calls_log));
        lines.push(format!("命令：{}（复现口径见报告 notes）", self.command));
        lines
    }
}

/// 报告序列化：pretty JSON，结尾带换行（便于追加查看）。
pub fn render(report: &ExpReport) -> Result<String, Error> {
    let mut text = serde_json::to_string_pretty(report)?;
    text.push('\n');
    Ok(text)
}

/// 毫秒读数的显示：没有样本时写 `-`。
fn milliseconds(value: Option<u64>) -> String {
    value.map_or_else(|| "-".to_owned(), |value| value.to_string())
}

/// 写报告到文件；父目录一并创建。
pub fn write(path: &Path, report: &ExpReport) -> Result<(), Error> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, render(report)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> ExpReport {
        ExpReport {
            run: "run-1".to_owned(),
            generated_at_unix_ms: 1_758_500_000_000,
            command: "cargo run -p jh-app -- exp".to_owned(),
            task_set: TaskSetSummary {
                name: "initial".to_owned(),
                source: "crates/jh-app/tasks/initial_tasks.json".to_owned(),
                cases: 4,
                questions: 3,
            },
            metric_definitions: MetricDefinitions::declared(),
            judges: vec![JudgeReport {
                judge: "rule".to_owned(),
                source: JudgeSource::Rule,
                live: false,
                verification: Verification {
                    status: VerificationStatus::Simulated,
                    detail: "本地判断器".to_owned(),
                },
                versions: vec!["rules-v1".to_owned()],
                calls: CallStats {
                    attempted: 4,
                    succeeded: 4,
                    failed: 0,
                    errors: BTreeMap::new(),
                },
                latency_ms: LatencyStats {
                    samples: 4,
                    min: Some(0),
                    p50: Some(0),
                    p95: Some(1),
                    max: Some(1),
                },
                usage: UsageStats {
                    calls_with_usage: 0,
                    input_tokens: None,
                    output_tokens: None,
                },
                quality: QualityStats {
                    judgments: 12,
                    agreements: 12,
                    agreement_rate: Some(1.0),
                    unscored: 0,
                    boundary_readings: 0,
                    brier_mean: Some(0.03),
                    log_loss_mean: Some(0.2),
                    log_loss_undefined: 0,
                    score_mae: Some(0.2),
                    score_mae_samples: 4,
                    per_question: Vec::new(),
                },
            }],
            calls_log: "/repo/var/calls.jsonl".to_owned(),
            notes: ExpReport::declared_notes(),
        }
    }

    #[test]
    fn the_report_round_trips_through_json_with_its_definitions() {
        let report = report();
        let text = render(&report).expect("报告可序列化");
        let wire: serde_json::Value = serde_json::from_str(&text).expect("报告是合法 JSON");

        assert_eq!(wire["task_set"]["cases"], serde_json::json!(4));
        assert_eq!(wire["judges"][0]["source"], serde_json::json!("rule"));
        assert_eq!(
            wire["judges"][0]["verification"]["status"],
            serde_json::json!("simulated")
        );
        assert!(
            wire["metric_definitions"]["brier"]
                .as_str()
                .is_some_and(|text| text.contains("Σ"))
        );
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn the_summary_states_agreement_latency_and_usage_per_judge() {
        let lines = report().summary_lines();
        let rendered = lines.join("\n");

        assert!(rendered.contains("判断器 rule"));
        assert!(rendered.contains("1.000（12/12）"));
        assert!(rendered.contains("usage 未上报"));
        assert!(rendered.contains("命令：cargo run -p jh-app -- exp"));
    }

    #[test]
    fn writing_a_report_creates_its_directory() {
        let dir = std::env::temp_dir().join(format!("jh-app-report-{}", std::process::id()));
        let path = dir.join("nested").join("run-1.json");

        write(&path, &report()).expect("应能写入报告");
        let written = std::fs::read_to_string(&path).expect("报告应已落盘");
        assert!(written.contains("\"run\": \"run-1\""));

        std::fs::remove_dir_all(&dir).ok();
    }
}
