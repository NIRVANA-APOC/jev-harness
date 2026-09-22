//! 响应侧类型：`answers` 与 `usage`。
//!
//! 对应官方页面 <https://docs.typesafe.ai/api> 与 <https://docs.typesafe.ai/confidence>
//! （核对日期 2026-09-22）。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// 一次 `system_one` 调用的响应体，外加从响应头捕获的请求 id。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemOneResponse {
    /// 实际作答的模型，通常是版本化 ID（示例均为 `jev-1.13.0`，即使请求发的是别名）。
    pub model: String,
    /// 与请求 `questions` 同 key 一一对应的答案。
    pub answers: BTreeMap<String, Answer>,
    /// token 用量；服务端未上报时为 `null`。
    #[serde(default)]
    pub usage: Usage,
    /// 响应头 `x-typesafe-request-id` 的值。
    ///
    /// 不是响应体字段（`#[serde(skip)]`），由 [`crate::Client::system_one`] 填上；
    /// 直接反序列化响应体时保持 `None`。
    #[serde(skip)]
    pub request_id: Option<String>,
}

impl SystemOneResponse {
    /// 按 key 取原始答案（可能是 [`Answer::Unknown`]）。
    pub fn answer(&self, id: &str) -> Option<&Answer> {
        self.answers.get(id)
    }

    /// 按 key 取 Choice 答案；答案类型不符时返回 `None`（不 panic、不串数据）。
    pub fn choice(&self, id: &str) -> Option<&ChoiceAnswer> {
        match self.answers.get(id)? {
            Answer::Choice(answer) => Some(answer),
            _ => None,
        }
    }

    /// 按 key 取 Score 答案；答案类型不符时返回 `None`。
    pub fn score(&self, id: &str) -> Option<&ScoreAnswer> {
        match self.answers.get(id)? {
            Answer::Score(answer) => Some(answer),
            _ => None,
        }
    }

    /// 按 key 取 Noul 答案；答案类型不符时返回 `None`。
    pub fn noul(&self, id: &str) -> Option<&NoulAnswer> {
        match self.answers.get(id)? {
            Answer::Noul(answer) => Some(answer),
            _ => None,
        }
    }
}

/// 一个类型化答案：`type` 字段即判别标签。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Answer {
    /// 见 [`ChoiceAnswer`]。
    Choice(ChoiceAnswer),
    /// 见 [`ScoreAnswer`]。
    Score(ScoreAnswer),
    /// 见 [`NoulAnswer`]。
    Noul(NoulAnswer),
    /// 未知 `type` 的兜底：该条答案保持未解析，其余答案照常可用，整条响应不会解析失败。
    #[serde(other)]
    Unknown,
}

/// Choice 答案。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChoiceAnswer {
    /// 概率最高的选项。
    pub choice: String,
    /// 每个选项一个概率；官方写 "sum to 1"，SDK schema 写 "sum to approximately 1"。
    pub probabilities: BTreeMap<String, f64>,
    /// 0–1，由概率分布推导；官方不锁定具体算法，阈值须在自有数据上校准。
    pub confidence: f64,
}

/// Score 答案。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoreAnswer {
    /// 按概率加权的期望值，可落在两级之间（官方示例 1.05 / 1.43 / 1.6 / 1.7）。
    pub score: f64,
    /// 级号 → 该级描述；wire 上是字符串级号（`"0"`、`"1"`），类型层是整数 key。
    #[serde(with = "level_map")]
    pub legend: BTreeMap<u8, String>,
    /// 级号 → 概率，和为 1。
    #[serde(with = "level_map")]
    pub probabilities: BTreeMap<u8, f64>,
    /// 0–1，由概率分布推导。
    pub confidence: f64,
}

/// `legend` / `probabilities` 的 key 映射。
///
/// HTTP 上是字符串级号（官方 API reference），而官方 Python SDK 的类型层用整数 key；
/// 这里同样用整数 key，避免同一语义两套表示，序列化时再写回字符串。
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

/// Noul 答案。
///
/// 只有 `noul` 一个字段：它是「答案为是」的概率（0 否到 1 是，接近 0.5 表示模型分不清），
/// 二值分布只有一个自由度，**官方不返回也不存在独立的 confidence**。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoulAnswer {
    /// 「答案为是」的概率。
    pub noul: f64,
}

/// token 用量。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    /// 输入 token 数（计费依据）。
    #[serde(default)]
    pub input_tokens: Option<u64>,
    /// 输出 token 数（免费，但会报告）。
    #[serde(default)]
    pub output_tokens: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 官方 Score 示例（<https://docs.typesafe.ai/primitives/score>）：wire 上是不带引号的级号 key。
    #[test]
    fn score_level_keys_are_strings_on_the_wire() {
        let answer: ScoreAnswer = serde_json::from_str(
            r#"{"score":1.43,"legend":{"0":"A","1":"B","2":"C"},
                "probabilities":{"0":0.0,"1":0.57,"2":0.43},"confidence":0.35}"#,
        )
        .expect("官方示例的形状应能解析");

        assert_eq!(answer.probabilities[&1], 0.57);
        assert_eq!(answer.legend[&2], "C");

        let wire = serde_json::to_value(&answer).expect("能序列化");
        assert_eq!(wire["probabilities"]["1"], 0.57);
        assert_eq!(wire["legend"]["2"], "C");
    }

    #[test]
    fn non_integer_level_key_is_rejected() {
        let error = serde_json::from_str::<ScoreAnswer>(
            r#"{"score":1.0,"legend":{"first":"A"},"probabilities":{},"confidence":1.0}"#,
        )
        .expect_err("非整数级号应显式失败");

        assert!(error.to_string().contains("级号"), "{error}");
    }
}
