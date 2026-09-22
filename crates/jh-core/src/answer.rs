//! 类型化答案：三类原语、概率/置信度与 token 用量。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::question::Question;

/// 类型化答案：与 [`Question`] 的三类原语一一对应。
///
/// 刻意没有「未知类型」兜底：未知或与问题不符的答案必须在适配层或
/// [`JudgeResponse::validate`](crate::JudgeResponse::validate) 处显式失败，不允许以「空答案」的
/// 名义流进运行时（通道侧的前向兼容由 `jh-jev` 的 `Answer::Unknown` 负责，与这里无关）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Answer {
    /// 见 [`ChoiceAnswer`]。
    Choice(ChoiceAnswer),
    /// 见 [`ScoreAnswer`]。
    Score(ScoreAnswer),
    /// 见 [`NoulAnswer`]。
    Noul(NoulAnswer),
}

impl Answer {
    /// 答案与问题是否是同一类原语。
    pub fn matches(&self, question: &Question) -> bool {
        matches!(
            (self, question),
            (Answer::Choice(_), Question::Choice(_))
                | (Answer::Score(_), Question::Score(_))
                | (Answer::Noul(_), Question::Noul(_))
        )
    }
}

/// Choice 答案。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChoiceAnswer {
    /// 概率最高的选项。
    pub choice: String,
    /// 每个选项一个概率，和为 1（近似）。
    pub probabilities: BTreeMap<String, f64>,
    /// 0–1，由概率分布推导；具体算法由服务端定义，阈值必须在自有数据上校准。
    pub confidence: f64,
}

/// Score 答案。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoreAnswer {
    /// 按概率加权的期望值，可落在两级之间。
    pub score: f64,
    /// 级号 → 该级描述。
    #[serde(with = "level_map")]
    pub legend: BTreeMap<u8, String>,
    /// 级号 → 概率，和为 1。
    #[serde(with = "level_map")]
    pub probabilities: BTreeMap<u8, f64>,
    /// 0–1，由概率分布推导。
    pub confidence: f64,
}

/// Noul 答案。
///
/// 只有 `noul` 一个字段：它是「答案为是」的概率（0 否到 1 是，接近 0.5 表示分不清）。二值分布
/// 只有一个自由度，因此不存在独立的 `confidence`——需要置信度时读 `noul` 到两端点的距离，
/// 但那是消费方的解读，不在类型里预先塌缩。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoulAnswer {
    /// 「答案为是」的概率。
    pub noul: f64,
}

/// token 用量。
///
/// 真实通道返回什么就存什么，不做换算、不折算成本：供应商未上报时为 `None`。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    /// 输入 token 数。
    #[serde(default)]
    pub input_tokens: Option<u64>,
    /// 输出 token 数。
    #[serde(default)]
    pub output_tokens: Option<u64>,
}

/// `legend` / `probabilities` 的 key 映射：内存里是整数级号，JSON 里是字符串键。
///
/// JSON 的对象键只能是字符串，而级号在类型层是整数（`crates/jh-jev/src/answer.rs` 出于同一理由
/// 也做了这层映射）。
mod level_map {
    use std::collections::BTreeMap;

    use serde::de::Error as _;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub(super) fn deserialize<'de, D, V>(deserializer: D) -> Result<BTreeMap<u8, V>, D::Error>
    where
        D: Deserializer<'de>,
        V: Deserialize<'de>,
    {
        BTreeMap::<String, V>::deserialize(deserializer)?
            .into_iter()
            .map(|(level, value)| {
                level
                    .parse::<u8>()
                    .map(|level| (level, value))
                    .map_err(|_| D::Error::custom(format_args!("级号 `{level}` 不是整数")))
            })
            .collect()
    }

    pub(super) fn serialize<S, V>(
        levels: &BTreeMap<u8, V>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        V: Serialize,
    {
        levels
            .iter()
            .map(|(level, value)| (level.to_string(), value))
            .collect::<BTreeMap<_, _>>()
            .serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::question::{NoulQuestion, Question, ScoreQuestion};

    #[test]
    fn choice_and_noul_answers_keep_raw_probabilities_through_json() {
        let choice = Answer::Choice(ChoiceAnswer {
            choice: "clock_skew".to_owned(),
            probabilities: BTreeMap::from([
                ("cert_expired".to_owned(), 0.43),
                ("clock_skew".to_owned(), 0.57),
            ]),
            confidence: 0.35,
        });
        let noul = Answer::Noul(NoulAnswer { noul: 0.42 });

        for answer in [choice, noul] {
            let wire = serde_json::to_string(&answer).unwrap();
            assert_eq!(serde_json::from_str::<Answer>(&wire).unwrap(), answer);
        }

        let wire = serde_json::to_value(Answer::Noul(NoulAnswer { noul: 0.42 })).unwrap();
        assert_eq!(wire["noul"], json!(0.42));
        assert!(
            wire.get("confidence").is_none(),
            "Noul 没有 confidence 字段：{wire}"
        );

        let wire = serde_json::to_value(Answer::Choice(ChoiceAnswer {
            choice: "b".to_owned(),
            probabilities: BTreeMap::from([("a".to_owned(), 0.43), ("b".to_owned(), 0.57)]),
            confidence: 0.14,
        }))
        .unwrap();
        assert_eq!(wire["probabilities"]["a"], json!(0.43));
        assert_eq!(wire["type"], json!("choice"));
    }

    #[test]
    fn score_answer_uses_string_level_keys_on_the_wire() {
        let answer = ScoreAnswer {
            score: 1.43,
            legend: BTreeMap::from([
                (0, "无".to_owned()),
                (1, "弱".to_owned()),
                (2, "强".to_owned()),
            ]),
            probabilities: BTreeMap::from([(0, 0.0), (1, 0.57), (2, 0.43)]),
            confidence: 0.35,
        };

        let wire = serde_json::to_value(Answer::Score(answer.clone())).unwrap();
        assert_eq!(wire["probabilities"]["1"], json!(0.57));
        assert_eq!(wire["legend"]["2"], json!("强"));
        assert_eq!(
            serde_json::from_value::<Answer>(wire).unwrap(),
            Answer::Score(answer)
        );
    }

    #[test]
    fn non_integer_score_level_key_is_rejected() {
        let error = serde_json::from_str::<ScoreAnswer>(
            r#"{"score":1.0,"legend":{"first":"无"},"probabilities":{},"confidence":1.0}"#,
        )
        .expect_err("非整数级号应显式失败");

        assert!(error.to_string().contains("级号"), "{error}");
    }

    #[test]
    fn answers_only_match_their_own_primitive() {
        let choice = Answer::Choice(ChoiceAnswer {
            choice: "a".to_owned(),
            probabilities: BTreeMap::new(),
            confidence: 1.0,
        });
        let noul = Answer::Noul(NoulAnswer { noul: 1.0 });
        let noul_question = Question::Noul(NoulQuestion {
            instructions: "？".to_owned(),
            criteria: None,
        });
        let score_question = Question::Score(ScoreQuestion {
            instructions: "？".to_owned(),
            criteria: vec!["低".to_owned(), "高".to_owned()],
        });

        assert!(noul.matches(&noul_question));
        assert!(!noul.matches(&score_question));
        assert!(!choice.matches(&noul_question));
    }
}
