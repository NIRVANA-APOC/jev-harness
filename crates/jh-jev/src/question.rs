//! 请求侧类型：`state` 与三类问题原语。
//!
//! 对应官方页面 <https://docs.typesafe.ai/api> 与 <https://docs.typesafe.ai/concepts/state>
//! （核对日期 2026-09-22）。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// 请求体的 `state`：交给模型判定的材料。
///
/// 官方允许字符串、对象、数组三种形态：字符串用于单段文本，对象用于带名字的多段上下文
/// （官方推荐），数组用于文本值序列或消息/记录序列。裸数字或布尔值不是合法 state，
/// 服务端会返回 422。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum State {
    /// 单段文本：一条消息、一篇文章。
    Text(String),
    /// 结构化对象：给每段上下文起名字，便于在 `instructions` 里用反引号路径引用嵌套字段。
    Object(Map<String, Value>),
    /// 文本值序列，或消息/记录序列。
    Messages(Vec<StateEntry>),
}

/// [`State::Messages`] 的一个元素。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StateEntry {
    /// 一条文本值。
    Text(String),
    /// 一条结构化记录，例如 `{"role": "user", "content": "..."}`。
    Record(Map<String, Value>),
}

impl From<&str> for State {
    fn from(value: &str) -> Self {
        State::Text(value.to_owned())
    }
}

impl From<String> for State {
    fn from(value: String) -> Self {
        State::Text(value)
    }
}

/// `instructions` 与 `criteria` 里的每处描述：wire 上是 `string | object | array`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Instructions {
    /// 直接一段文字。
    Text(String),
    /// 结构化写法：问题放一个字段、被引用的数据放其他字段，用反引号路径互相引用。
    Structured(Value),
}

impl From<&str> for Instructions {
    fn from(value: &str) -> Self {
        Instructions::Text(value.to_owned())
    }
}

impl From<String> for Instructions {
    fn from(value: String) -> Self {
        Instructions::Text(value)
    }
}

/// Choice：从一组无序选项中择一。
///
/// 官方上限 255 个选项（cookbook 口径为「可靠上限约 240」）。选项名与描述都会发给模型，
/// 描述须能相互区分；选项集合覆盖不了全部输入时，显式加 `other` / `none of the above`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChoiceQuestion {
    /// 要做出的判断。
    pub instructions: Instructions,
    /// 选项名 → 该选项的说明；不需要说明的选项写 `null`。
    pub criteria: BTreeMap<String, Option<Instructions>>,
}

/// Score：在有序标尺上取值。
///
/// `criteria` 至少 2 级、最多 10 级，**下标即级号（从 0 开始）**；模型只看到描述文本，
/// 看不到编号，所以描述要写情境而不是程度。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoreQuestion {
    /// 要做出的判断。
    pub instructions: Instructions,
    /// 有序等级描述数组。
    pub criteria: Vec<Instructions>,
}

/// Noul 的可选 `criteria`：分别描述「接近 1 的是」与「接近 0 的非」。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NoulCriteria {
    /// 描述什么算「是」。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#true: Option<Instructions>,
    /// 描述什么算「否」。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#false: Option<Instructions>,
}

/// Noul：是非判断，返回「答案为是」的概率。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoulQuestion {
    /// 要做出的判断；让高值 = 是。
    pub instructions: Instructions,
    /// 可选的两侧语义说明；不写时该字段不出现在请求体里。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub criteria: Option<NoulCriteria>,
}

/// 一个类型化问题：`type` 字段即判别标签。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Question {
    /// 见 [`ChoiceQuestion`]。
    Choice(ChoiceQuestion),
    /// 见 [`ScoreQuestion`]。
    Score(ScoreQuestion),
    /// 见 [`NoulQuestion`]。
    Noul(NoulQuestion),
    /// 未知 `type` 的兜底：只为反序列化旧数据而存在（未知类型不能发给服务端）。
    ///
    /// 构造请求时不应使用——序列化它会得到 `{"type": "unknown"}`，服务端会以 422 拒绝。
    #[serde(other)]
    Unknown,
}

/// 一次请求的问题集：key 由调用方自取，**不发送给模型、不参与推理**，只用于把答案对回来。
pub type Questions = BTreeMap<String, Question>;

impl From<ChoiceQuestion> for Question {
    fn from(value: ChoiceQuestion) -> Self {
        Question::Choice(value)
    }
}

impl From<ScoreQuestion> for Question {
    fn from(value: ScoreQuestion) -> Self {
        Question::Score(value)
    }
}

impl From<NoulQuestion> for Question {
    fn from(value: NoulQuestion) -> Self {
        Question::Noul(value)
    }
}
