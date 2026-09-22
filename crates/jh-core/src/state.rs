//! 判定材料（`state`）与快照标识。

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 输入 state 的快照标识。
///
/// 判断记录靠它绑定「这份材料」（`docs/RESEARCH_GOALS.md` §5.2）。快照自身的存储形态与哈希口径
/// 未定（`docs/roadmap-1.0.md` §12 未确定项 2），标识由调用方给出，本 crate 只透传。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SnapshotId(pub String);

impl SnapshotId {
    /// 借出标识文本。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for SnapshotId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl From<String> for SnapshotId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl fmt::Display for SnapshotId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// 交给判断器判定的材料。
///
/// 三种形态取各判断通道的公共下限：单段文本、带名字的多段材料（推荐，名字让问题可以引用具体段落）、
/// 有序记录序列。怎么把它摆进请求或提示词是 `jh-app` 适配层的事；本 crate 不裁剪、不校验、不摘要。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum State {
    /// 单段文本。
    Text(String),
    /// 带名字的多段材料：名字 → 内容。
    Object(BTreeMap<String, Value>),
    /// 有序的记录序列，例如一条条带角色字段的消息。
    Records(Vec<StateEntry>),
}

/// [`State::Records`] 里的一条记录。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StateEntry {
    /// 一条文本。
    Text(String),
    /// 一条结构化记录，例如 `{"role": "user", "content": "..."}`。
    Record(BTreeMap<String, Value>),
}

impl From<&str> for State {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

impl From<String> for State {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn state_round_trips_in_all_three_shapes() {
        let text = State::from("取消已确认");
        assert_eq!(serde_json::to_value(&text).unwrap(), json!("取消已确认"));

        for state in [
            text,
            State::Object(BTreeMap::from([("log".to_owned(), json!("cache write"))])),
            State::Records(vec![
                StateEntry::Text("cancel confirmed".to_owned()),
                StateEntry::Record(BTreeMap::from([("role".to_owned(), json!("tool"))])),
            ]),
        ] {
            let wire = serde_json::to_string(&state).unwrap();
            assert_eq!(serde_json::from_str::<State>(&wire).unwrap(), state);
        }
    }

    #[test]
    fn snapshot_id_is_a_transparent_string_on_the_wire() {
        let snapshot = SnapshotId::from("case-001");
        assert_eq!(serde_json::to_value(&snapshot).unwrap(), json!("case-001"));
        assert_eq!(snapshot.to_string(), "case-001");
        assert_eq!(snapshot.as_str(), "case-001");
    }
}
