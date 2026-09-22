//! 类型化问题：三类原语、稳定 ID 与版本。

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::Error;

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

/// 同一 ID 两个版本之间的关系。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VersionComparison {
    /// 含义未变：同一个定义被重复登记。
    Same,
    /// 含义已变：按新版本使用，旧版本与绑在旧版本上的判断都不被改写。
    Changed,
}

/// 问题版本表：同一 ID 的历次定义各占一个版本，旧版本不会被覆盖。
///
/// 研究目标 §5.2：「问题定义及版本；同一 ID 的问题改变含义时，必须产生新版本。」这里的规则是：
///
/// - [`QuestionRegistry::register`] 只能补充新版本；同一 ID + 同一版本上想换成别的含义会被显式
///   拒绝（[`Error::QuestionVersionConflict`]），改含义必须走 [`QuestionRegistry::revise`]；
/// - 重复登记完全相同的定义是幂等的，方便回放；
/// - 版本号是单调递增整数（[`QuestionVersion`]），`revise` 取当前最大版本 +1。
///
/// 与失效的联动——问题升版之后旧判断怎么处理——属于 M2 的问题集合精化流程
/// （`docs/roadmap-1.0.md` §5 M2），不在本模块：这里只保证版本本身不被覆盖、可查可比较。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct QuestionRegistry {
    versions: BTreeMap<QuestionId, BTreeMap<QuestionVersion, Question>>,
}

impl QuestionRegistry {
    /// 登记一个带版本的问题定义。
    ///
    /// 返回显式错误只有一种情况：该 ID + 该版本上已经记着含义不同的另一份定义。
    pub fn register(&mut self, id: &QuestionId, spec: &QuestionSpec) -> Result<(), Error> {
        match self
            .versions
            .get(id)
            .and_then(|history| history.get(&spec.version))
        {
            Some(existing) if *existing == spec.question => Ok(()),
            Some(_) => Err(Error::QuestionVersionConflict(id.clone())),
            None => {
                self.versions
                    .entry(id.clone())
                    .or_default()
                    .insert(spec.version, spec.question.clone());
                Ok(())
            }
        }
    }

    /// 登记一个新版本：版本号取该 ID 当前最大版本 +1（首次登记为 1）。
    pub fn revise(&mut self, id: &QuestionId, question: Question) -> QuestionVersion {
        let history = self.versions.entry(id.clone()).or_default();
        let next = history
            .keys()
            .next_back()
            .map_or(QuestionVersion(1), |latest| QuestionVersion(latest.0 + 1));
        history.insert(next, question);
        next
    }

    /// 按 ID + 版本取问题定义。
    pub fn at(&self, id: &QuestionId, version: QuestionVersion) -> Option<&Question> {
        self.versions.get(id)?.get(&version)
    }

    /// 该 ID 当前的版本与定义：版本号最大的那个。
    pub fn current(&self, id: &QuestionId) -> Option<(QuestionVersion, &Question)> {
        self.versions
            .get(id)?
            .iter()
            .next_back()
            .map(|(version, question)| (*version, question))
    }

    /// 该 ID 的版本历史（按版本号升序）。
    pub fn history(&self, id: &QuestionId) -> Vec<(QuestionVersion, &Question)> {
        self.versions
            .get(id)
            .map(|history| {
                history
                    .iter()
                    .map(|(version, question)| (*version, question))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 比较同一 ID 的两个版本；任一侧没登记时为 `None`。
    pub fn compare(
        &self,
        id: &QuestionId,
        left: QuestionVersion,
        right: QuestionVersion,
    ) -> Option<VersionComparison> {
        let history = self.versions.get(id)?;
        let left = history.get(&left)?;
        let right = history.get(&right)?;
        Some(if left == right {
            VersionComparison::Same
        } else {
            VersionComparison::Changed
        })
    }

    /// 已登记的问题 ID（按标识排序）。
    pub fn ids(&self) -> impl Iterator<Item = &QuestionId> {
        self.versions.keys()
    }
}

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

    fn noul(instructions: &str) -> Question {
        Question::Noul(NoulQuestion {
            instructions: instructions.to_owned(),
            criteria: None,
        })
    }

    #[test]
    fn a_question_id_that_changes_meaning_gets_a_new_version() {
        let mut registry = QuestionRegistry::default();
        let id = QuestionId::from("cache_written_after_cancel");
        let first = QuestionSpec {
            version: QuestionVersion(1),
            question: noul("取消确认之后是否还有写入？"),
        };
        registry.register(&id, &first).expect("首次登记");

        // 同一 ID + 同一版本换含义：显式拒绝，旧版本不可被覆盖。
        let error = registry
            .register(
                &id,
                &QuestionSpec {
                    version: QuestionVersion(1),
                    question: noul("换一个含义"),
                },
            )
            .expect_err("同一版本上不能换含义");
        assert!(
            matches!(error, Error::QuestionVersionConflict(_)),
            "{error}"
        );
        assert_eq!(registry.at(&id, QuestionVersion(1)), Some(&first.question));

        // 重复登记同一份定义是幂等的。
        registry.register(&id, &first).expect("重复登记同一份定义");

        let version = registry.revise(&id, noul("取消确认之后是否还有跨任务的写入？"));
        assert_eq!(version, QuestionVersion(2));
        assert_eq!(
            registry.current(&id).map(|(version, _)| version),
            Some(QuestionVersion(2))
        );
        assert_eq!(
            registry.at(&id, QuestionVersion(1)),
            Some(&first.question),
            "旧版本原样留着"
        );

        // 按版本查询与比较。
        assert_eq!(registry.history(&id).len(), 2);
        assert_eq!(
            registry.compare(&id, QuestionVersion(1), QuestionVersion(2)),
            Some(VersionComparison::Changed)
        );
        assert_eq!(
            registry.compare(&id, QuestionVersion(2), QuestionVersion(2)),
            Some(VersionComparison::Same)
        );
        assert_eq!(
            registry.compare(&id, QuestionVersion(1), QuestionVersion(9)),
            None
        );
        assert_eq!(
            registry.at(&QuestionId::from("never_registered"), QuestionVersion(1)),
            None
        );
        assert_eq!(registry.ids().count(), 1);

        // 另一个 ID 从 1 开始，不共享版本号。
        let other = QuestionId::from("cache_enabled");
        assert_eq!(registry.revise(&other, noul("？")), QuestionVersion(1));

        let wire = serde_json::to_value(&registry).unwrap();
        assert_eq!(
            wire["versions"]["cache_written_after_cancel"]["2"]["type"],
            json!("noul")
        );
        assert_eq!(
            serde_json::from_value::<QuestionRegistry>(wire).unwrap(),
            registry
        );
    }
}
