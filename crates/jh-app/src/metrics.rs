//! 指标口径：一致读法、Brier、对数损失、延迟分位数。
//!
//! 全部口径都是**本项目声明的规则**，不是照抄文档默认值；同样的文字会写进报告的
//! `metric_definitions`，让读数与口径一起可复核（`docs/RESEARCH_GOALS.md` §10.1、§10.2；
//! `docs/jev-research.md` §14.3 E2：判定阈值一律来自本项目自测）。
//!
//! # 标签空间
//!
//! 概率分布统一写成「标签 → 概率」：
//!
//! - `choice`：标签是选项名；
//! - `score`：标签是级号的十进制文本；
//! - `noul`：标签是 `true` / `false`（二值分布只有一个自由度，答案里只有 `noul` 一个数）。
//!
//! # Brier 与对数损失
//!
//! - Brier：`Σ_i (p_i − t_i)²`，`i` 遍历答案与真值标签的并集（缺的记 0）。二值 `noul` 也按两个
//!   标签求和，因此其上界是 2，是常见二值预测口径 `(p − t)²` 的两倍——报告只用前一种，不混用。
//! - 对数损失：`−ln p_真值`。答案为真值标签分配 0 概率时它没有有限值，记为 `null` 并单独计数，
//!   不塞一个假的封顶值。

use std::collections::{BTreeMap, BTreeSet};

use jh_core::Answer;

use crate::tasks::Truth;

/// 一个标签空间上的概率分布。
pub type Distribution = BTreeMap<String, f64>;

/// 答案的读出标签与边界标记。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reading {
    /// 读出的标签。
    pub label: String,
    /// 这次读出落在边界上：`noul` 恰为 0.5，或分布里有并列的最大概率。
    pub boundary: bool,
}

/// 一次判断的打分结果。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JudgmentScore {
    /// 读出标签是否等于真值标签。
    pub agreement: bool,
    /// 读出是否落在边界上（与一致与否无关，单独计数）。
    pub boundary: bool,
    /// Brier 分数。
    pub brier: f64,
    /// 对数损失；答案为真值标签分配 0 概率时为 `None`。
    pub log_loss: Option<f64>,
    /// `|期望值 − 真值级号|`，只对 score 题有值（等级是序数，只作诊断，不当经济收益相加）。
    pub score_mae: Option<f64>,
}

/// 答案的概率分布（缺失的标签视为 0，不在这里补位）。
pub fn distribution(answer: &Answer) -> Distribution {
    match answer {
        Answer::Choice(answer) => answer.probabilities.clone(),
        Answer::Score(answer) => answer
            .probabilities
            .iter()
            .map(|(level, mass)| (level.to_string(), *mass))
            .collect(),
        Answer::Noul(answer) => BTreeMap::from([
            ("true".to_owned(), answer.noul),
            ("false".to_owned(), 1.0 - answer.noul),
        ]),
    }
}

/// 真值的 one-hot 概率标签。
pub fn truth_distribution(truth: &Truth) -> Distribution {
    match truth {
        Truth::Choice { label } => BTreeMap::from([(label.clone(), 1.0)]),
        Truth::Score { level } => BTreeMap::from([(level.to_string(), 1.0)]),
        Truth::Noul { yes } => BTreeMap::from([
            ("true".to_owned(), if *yes { 1.0 } else { 0.0 }),
            ("false".to_owned(), if *yes { 0.0 } else { 1.0 }),
        ]),
    }
}

/// 真值的读出标签（与 [`truth_distribution`] 的 one-hot 标签一致）。
pub fn truth_label(truth: &Truth) -> String {
    match truth {
        Truth::Choice { label } => label.clone(),
        Truth::Score { level } => level.to_string(),
        Truth::Noul { yes } => if *yes { "true" } else { "false" }.to_owned(),
    }
}

/// 答案的读出标签：choice 取通道声明的 `choice`；score 取概率最大的级号（并列取最小级号）；
/// noul 取 `noul ≥ 0.5` 为「是」。
///
/// 分布为空时没有可读出的标签，返回 `None`——调用方把它记成「无法读出」，
/// 而不是当成答错（`docs/RESEARCH_GOALS.md` §10.1：完成声明不构成证据，缺数据也不等于失败）。
pub fn reading(answer: &Answer) -> Option<Reading> {
    match answer {
        Answer::Choice(answer) => {
            if answer.probabilities.is_empty() {
                return None;
            }
            let best = answer
                .probabilities
                .values()
                .copied()
                .fold(f64::MIN, f64::max);
            let top = answer
                .probabilities
                .values()
                .filter(|mass| **mass >= best)
                .count();
            Some(Reading {
                label: answer.choice.clone(),
                boundary: top > 1,
            })
        }
        Answer::Score(answer) => {
            let (level, best) = argmax(
                answer
                    .probabilities
                    .iter()
                    .map(|(level, mass)| (level.to_string(), *mass)),
            )?;
            let top = answer
                .probabilities
                .values()
                .filter(|mass| **mass >= best)
                .count();
            Some(Reading {
                label: level,
                boundary: top > 1,
            })
        }
        Answer::Noul(answer) => {
            if !(0.0..=1.0).contains(&answer.noul) {
                return None;
            }
            Some(Reading {
                label: if answer.noul >= 0.5 { "true" } else { "false" }.to_owned(),
                boundary: answer.noul == 0.5,
            })
        }
    }
}

/// 升序遍历里第一个最大值即并列中的最小标签。
fn argmax<I, K>(items: I) -> Option<(K, f64)>
where
    I: IntoIterator<Item = (K, f64)>,
{
    items
        .into_iter()
        .fold(None, |best: Option<(K, f64)>, candidate| match best {
            Some((_, best_mass)) if candidate.1 <= best_mass => best,
            _ => Some(candidate),
        })
}

/// Brier 分数：`Σ_i (p_i − t_i)²`，`i` 遍历两边标签的并集。
pub fn brier(answer: &Distribution, truth: &Distribution) -> f64 {
    let mut labels: BTreeSet<&String> = answer.keys().collect();
    labels.extend(truth.keys());

    labels
        .into_iter()
        .map(|label| {
            let predicted = answer.get(label).copied().unwrap_or(0.0);
            let actual = truth.get(label).copied().unwrap_or(0.0);
            (predicted - actual).powi(2)
        })
        .sum()
}

/// 对数损失：`−ln p_真值`；`p_真值 ≤ 0` 时没有有限值。
///
/// `p_真值 = 1` 时结果是 `-0.0`，这里归一成 `0.0`，免得报告里出现 `-0.0000` 这种读数。
pub fn log_loss(probability_of_truth: f64) -> Option<f64> {
    (probability_of_truth > 0.0).then(|| {
        let loss = -probability_of_truth.ln();
        if loss == 0.0 { 0.0 } else { loss }
    })
}

/// 最近秩分位数：把样本升序排列后取第 `⌈p·n⌉` 个（1-based）；空样本为 `None`。
pub fn percentile(samples: &[u64], p: f64) -> Option<u64> {
    if samples.is_empty() {
        return None;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let rank = (p * sorted.len() as f64).ceil().max(1.0) as usize;
    sorted.get(rank - 1).copied()
}

/// 给一次判断打分；答案分布读不出标签时为 `None`（调用方记成「无法读出」）。
pub fn score(answer: &Answer, truth: &Truth) -> Option<JudgmentScore> {
    let reading = reading(answer)?;
    let distribution = distribution(answer);
    let truth_label = truth_label(truth);

    Some(JudgmentScore {
        agreement: reading.label == truth_label,
        boundary: reading.boundary,
        brier: brier(&distribution, &truth_distribution(truth)),
        log_loss: log_loss(distribution.get(&truth_label).copied().unwrap_or(0.0)),
        score_mae: match (answer, truth) {
            (Answer::Score(answer), Truth::Score { level }) => {
                Some((answer.score - f64::from(*level)).abs())
            }
            _ => None,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use jh_core::{ChoiceAnswer, NoulAnswer, ScoreAnswer};

    fn choice(label: &str, probabilities: Distribution) -> Answer {
        Answer::Choice(ChoiceAnswer {
            choice: label.to_owned(),
            probabilities,
            confidence: 1.0,
        })
    }

    #[test]
    fn readings_follow_the_declared_rules() {
        let answer = choice(
            "cache_stale",
            BTreeMap::from([("cache_stale".to_owned(), 0.8), ("other".to_owned(), 0.2)]),
        );
        assert_eq!(
            reading(&answer),
            Some(Reading {
                label: "cache_stale".to_owned(),
                boundary: false
            })
        );

        // 通道声明的 choice 就是读出标签，即使它不是概率最大的选项。
        let inconsistent = choice(
            "other",
            BTreeMap::from([("cache_stale".to_owned(), 0.8), ("other".to_owned(), 0.2)]),
        );
        assert_eq!(
            reading(&inconsistent).expect("应能读出").label,
            "other".to_owned()
        );

        // noul 以 0.5 为界，恰好 0.5 记成边界读数。
        assert_eq!(
            reading(&Answer::Noul(NoulAnswer { noul: 0.5 })),
            Some(Reading {
                label: "true".to_owned(),
                boundary: true
            })
        );
        assert_eq!(
            reading(&Answer::Noul(NoulAnswer { noul: 0.49 })).expect("应能读出"),
            Reading {
                label: "false".to_owned(),
                boundary: false
            }
        );
    }

    #[test]
    fn a_score_reading_takes_the_heaviest_level_and_flags_ties() {
        let answer = Answer::Score(ScoreAnswer {
            score: 1.5,
            legend: BTreeMap::new(),
            probabilities: BTreeMap::from([(1, 0.5), (2, 0.5)]),
            confidence: 0.0,
        });
        assert_eq!(
            reading(&answer),
            Some(Reading {
                label: "1".to_owned(),
                boundary: true
            })
        );
    }

    #[test]
    fn an_empty_distribution_has_no_reading() {
        assert_eq!(reading(&choice("a", BTreeMap::new())), None);
        assert_eq!(
            reading(&Answer::Score(ScoreAnswer {
                score: 1.0,
                legend: BTreeMap::new(),
                probabilities: BTreeMap::new(),
                confidence: 1.0,
            })),
            None
        );
        assert_eq!(reading(&Answer::Noul(NoulAnswer { noul: 1.5 })), None);
    }

    #[test]
    fn brier_sums_over_the_union_of_labels() {
        let answer = BTreeMap::from([
            ("a".to_owned(), 0.7),
            ("b".to_owned(), 0.3),
            ("c".to_owned(), 0.0),
        ]);
        let truth = BTreeMap::from([("a".to_owned(), 1.0)]);

        // (0.7−1)² + (0.3−0)² + (0−0)²
        assert!((brier(&answer, &truth) - (0.09 + 0.09)).abs() < 1e-12);
    }

    #[test]
    fn log_loss_is_undefined_when_the_truth_gets_zero_probability() {
        assert_eq!(log_loss(0.0), None);
        assert_eq!(log_loss(-1.0), None);
        assert!(log_loss(0.5).expect("有定义").abs() < 0.694);
        assert_eq!(log_loss(1.0), Some(0.0));
    }

    #[test]
    fn percentile_uses_the_nearest_rank() {
        let samples = [40, 10, 30, 20];
        assert_eq!(percentile(&samples, 0.5), Some(20));
        assert_eq!(percentile(&samples, 0.95), Some(40));
        assert_eq!(percentile(&[], 0.5), None);
        assert_eq!(percentile(&[7], 0.0), Some(7));
    }

    #[test]
    fn scoring_agrees_with_the_truth_and_reports_score_error() {
        let scored = score(
            &Answer::Score(ScoreAnswer {
                score: 1.4,
                legend: BTreeMap::new(),
                probabilities: BTreeMap::from([(1, 0.6), (2, 0.4)]),
                confidence: 0.0,
            }),
            &Truth::Score { level: 2 },
        )
        .expect("应能打分");

        assert!(!scored.agreement);
        assert!((scored.score_mae.expect("score 题应有 MAE") - 0.6).abs() < 1e-12);
        assert!((scored.brier - 0.72).abs() < 1e-12, "{scored:?}");
        assert!(scored.log_loss.expect("真值有概率").abs() > 0.0);
    }

    #[test]
    fn a_perfect_reading_scores_zero_brier_and_zero_log_loss() {
        let scored = score(
            &Answer::Noul(NoulAnswer { noul: 1.0 }),
            &Truth::Noul { yes: true },
        )
        .expect("应能打分");

        assert!(scored.agreement);
        assert_eq!(scored.brier, 0.0);
        assert_eq!(scored.log_loss, Some(0.0));
    }
}
