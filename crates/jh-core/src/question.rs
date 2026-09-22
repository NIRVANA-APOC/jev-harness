//! 类型化问题：三类原语、稳定 ID 与版本。

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

/// 问题的稳定标识。
///
/// 用于把请求与答案对回来、并作为判断记录的键；不发送给模型、不参与判定。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct QuestionId(pub String);

impl QuestionId {
    /// 借出标识文本。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for QuestionId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl From<String> for QuestionId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl fmt::Display for QuestionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// 问题定义的版本号。
///
/// 同一个 ID 的问题一旦改变含义就必须升版本：判断记录绑定「ID + 版本」，这样旧记录不会被改写后的
/// 问题解释错（`docs/RESEARCH_GOALS.md` §5.2）。版本号规则未定（`docs/roadmap-1.0.md` §12 未确定项 5），
/// 1.0 用单调递增的整数。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct QuestionVersion(pub u32);

impl fmt::Display for QuestionVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 一个带版本的问题定义。
///
/// `docs/RESEARCH_GOALS.md` §8.1 的 `QuestionSpec` 还要求证据需求、适用范围、触发条件、消费者与
/// 生命周期（同文档 §6.3）；那些属于 M2 的问题库，不在 1.0 的判断接口里。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuestionSpec {
    /// 该问题定义的版本。
    pub version: QuestionVersion,
    /// 问题本身，三类原语之一。
    pub question: Question,
}

/// 一次请求的问题集：ID → 带版本的问题定义。
///
/// 一个请求放 1 个还是 N 个问题由调用方决定——「一次多问」与「逐问」用的是同一个接口
/// （`docs/roadmap-1.0.md` §3.2）。
pub type Questions = BTreeMap<QuestionId, QuestionSpec>;

/// 类型化问题：`type` 字段即判别标签，与三类原语一一对应。
///
/// 没有「未知类型」兜底：本 crate 只承载判定的运行语义，服务端新增原语属于通道的前向兼容问题，
/// 由 `jh-app` 的适配层处理（那里可以对比 `jh_jev::Answer::Unknown`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Question {
    /// 从一组无序选项中择一，见 [`ChoiceQuestion`]。
    Choice(ChoiceQuestion),
    /// 在有序标尺上取值，见 [`ScoreQuestion`]。
    Score(ScoreQuestion),
    /// 是非判断，返回「是」的概率，见 [`NoulQuestion`]。
    Noul(NoulQuestion),
}

/// Choice：从一组无序选项中择一。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChoiceQuestion {
    /// 要做出的判断。
    ///
    /// 1.0 只表达文字；需要引用 state 里具体字段时，靠 [`State::Object`](crate::State::Object) 的名字
    /// 在文字里写清楚，不在这里引入结构化指令。
    pub instructions: String,
    /// 选项名 → 该选项的说明（可为 `None`）。
    ///
    /// 选描述要能相互区分；覆盖不了全部输入时显式加 `other` / `none of the above`。
    pub criteria: BTreeMap<String, Option<String>>,
}

/// Score：在有序标尺上取值。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoreQuestion {
    /// 要做出的判断。
    pub instructions: String,
    /// 有序等级描述：**下标即级号（从 0 开始）**。
    pub criteria: Vec<String>,
}

/// Noul 的可选 `criteria`：分别描述「接近 1 的是」与「接近 0 的非」。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoulCriteria {
    /// 描述什么算「是」。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#true: Option<String>,
    /// 描述什么算「否」。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#false: Option<String>,
}

/// Noul：是非判断，返回「答案为是」的概率。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoulQuestion {
    /// 要做出的判断；让高值 = 是。
    pub instructions: String,
    /// 可选的两侧语义说明；不写时该字段不出现在 JSON 里。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub criteria: Option<NoulCriteria>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn question_primitives_carry_their_type_tag_and_round_trip() {
        let choice = Question::Choice(ChoiceQuestion {
            instructions: "哪一类故障？".to_owned(),
            criteria: BTreeMap::from([
                ("cert_expired".to_owned(), Some("证书过期".to_owned())),
                ("clock_skew".to_owned(), None),
            ]),
        });
        let score = Question::Score(ScoreQuestion {
            instructions: "证据强度？".to_owned(),
            criteria: vec!["无".to_owned(), "弱".to_owned(), "强".to_owned()],
        });
        let noul = Question::Noul(NoulQuestion {
            instructions: "取消确认之后是否还有写入？".to_owned(),
            criteria: Some(NoulCriteria {
                r#true: Some("确认后仍有同一任务的写入".to_owned()),
                r#false: None,
            }),
        });

        for (question, tag) in [(choice, "choice"), (score, "score"), (noul, "noul")] {
            let wire = serde_json::to_value(&question).unwrap();
            assert_eq!(wire["type"], tag);
            assert_eq!(serde_json::from_value::<Question>(wire).unwrap(), question);
        }

        let wire = serde_json::to_value(Question::Noul(NoulQuestion {
            instructions: "？".to_owned(),
            criteria: Some(NoulCriteria {
                r#true: Some("是".to_owned()),
                r#false: Some("否".to_owned()),
            }),
        }))
        .unwrap();
        assert_eq!(wire["criteria"]["true"], json!("是"));
        assert_eq!(wire["criteria"]["false"], json!("否"));
    }

    #[test]
    fn question_id_and_version_are_transparent_on_the_wire() {
        let spec = QuestionSpec {
            version: QuestionVersion(2),
            question: Question::Noul(NoulQuestion {
                instructions: "？".to_owned(),
                criteria: None,
            }),
        };
        let questions = Questions::from([(QuestionId::from("cache_written_after_cancel"), spec)]);

        let wire = serde_json::to_value(&questions).unwrap();
        assert_eq!(wire["cache_written_after_cancel"]["version"], json!(2));
        assert_eq!(
            wire["cache_written_after_cancel"]["question"]["type"],
            json!("noul")
        );
        assert_eq!(
            serde_json::from_value::<Questions>(wire).unwrap(),
            questions
        );
    }
}
