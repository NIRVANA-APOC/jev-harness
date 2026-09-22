//! 输入投影与快照校验：每个判断声明它读了哪些对象与字段。
//!
//! 研究目标 §5.3：「每个判断节点声明输入投影，即从完整证据库取出哪些对象和字段。若多个问题共享
//! 一份请求 state，每个问题都保守依赖该请求的全部输入；不能在返回后猜测模型只使用了其中某几项。」
//!
//! 两条由此而来的取舍：
//!
//! - **读取范围按对象记录**：共享一份 state 的整批问题登记同一份投影（
//!   [`InputProjection::covering`]），宁大勿小——漏记录读取范围会漏失效（§10.2 的诊断指标）；
//! - **字段粒度先只记录、不参与失效判定**：证据撤不撤回按对象整体保守处理，字段选择进审计与
//!   后续版本，不在 1.0 拿它缩小失效范围。
//!
//! 投影还带**版本**：请求时读到的是该对象的哪一版。版本**只用于登记时的快照校验**
//! （[`verify_snapshot`]：读到的版本必须仍是当前版本、且该版本上没有已撤回的记录），
//! **不参与失效传播的筛选**——传播按对象走（见 [`SemanticState`](crate::SemanticState) 的传播规则），
//! 所以装配请求 state 时混进同一对象的旧版本记录不会造成静默漏失效。

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::evidence::{EvidenceLog, ObjectId, ObjectVersion};

/// 投影里对一个对象读到的字段范围。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum FieldSelection {
    /// 该对象的全部字段：共享一份请求 state 时的保守选择。
    All,
    /// 只读了列出的字段。
    Named {
        /// 读到的字段名。
        fields: BTreeSet<String>,
    },
}

/// 投影里对一个对象的读取：读到哪一版、读了哪些字段。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ObjectRead {
    /// 读到该对象的版本（通常是请求时仍有效的最大版本）。
    pub version: ObjectVersion,
    /// 读到的字段范围。
    pub fields: FieldSelection,
}

impl ObjectRead {
    /// 读了该对象的全部字段。
    pub fn all(version: ObjectVersion) -> Self {
        Self {
            version,
            fields: FieldSelection::All,
        }
    }

    /// 只读了列出的字段。
    pub fn fields(version: ObjectVersion, fields: impl IntoIterator<Item = String>) -> Self {
        Self {
            version,
            fields: FieldSelection::Named {
                fields: fields.into_iter().collect(),
            },
        }
    }

    /// 某个字段名是否落在读取范围内（[`FieldSelection::All`] 恒为真）。
    pub fn includes(&self, field: &str) -> bool {
        match &self.fields {
            FieldSelection::All => true,
            FieldSelection::Named { fields } => fields.contains(field),
        }
    }
}

/// 一个判断节点的输入投影：它从证据库里读了哪些对象与字段。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
pub struct InputProjection {
    objects: BTreeMap<ObjectId, ObjectRead>,
}

impl InputProjection {
    /// 保守投影：把列出的对象按各自的读取版本整体读入。
    ///
    /// 入参的版本是**这次请求读到的版本**：装了当前版本就写
    /// [`EvidenceLog::current_version`](crate::EvidenceLog::current_version) 给出的那个（推荐只装
    /// [`current_evidence`](crate::EvidenceLog::current_evidence)，别把同一对象的旧版本也塞进 state）。
    /// 版本只用于登记时的快照校验，不参与传播筛选——传播按对象走，混进旧版本记录不会造成漏失效。
    pub fn covering(objects: impl IntoIterator<Item = (ObjectId, ObjectVersion)>) -> Self {
        Self {
            objects: objects
                .into_iter()
                .map(|(object, version)| (object, ObjectRead::all(version)))
                .collect(),
        }
    }

    /// 追加一次读取；同一对象重复登记时以最后一次为准。
    pub fn read(mut self, object: ObjectId, read: ObjectRead) -> Self {
        self.objects.insert(object, read);
        self
    }

    /// 读到的对象与读取范围（按对象排序）。
    pub fn objects(&self) -> impl Iterator<Item = (&ObjectId, &ObjectRead)> {
        self.objects.iter()
    }

    /// 某个对象读到的版本。
    pub fn version_of(&self, object: &ObjectId) -> Option<ObjectVersion> {
        self.objects.get(object).map(|read| read.version)
    }

    /// 是否读了某个对象。
    pub fn reads(&self, object: &ObjectId) -> bool {
        self.objects.contains_key(object)
    }

    /// 读到的对象数。
    pub fn len(&self) -> usize {
        self.objects.len()
    }

    /// 没有任何读取：这个判断不依赖证据（例如纯规则判定）。
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }
}

/// 快照校验发现的一处漂移。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Drift {
    /// 对象当前的版本不再是读到的那个：出了新版本，或读到的那一版被撤回。
    VersionMoved {
        /// 对象。
        object: ObjectId,
        /// 投影读到的版本。
        read: ObjectVersion,
        /// 该对象当前的版本。
        current: ObjectVersion,
    },
    /// 读到的版本上已经没有仍有效的记录（被撤回了）。
    Retracted {
        /// 对象。
        object: ObjectId,
        /// 投影读到的版本。
        version: ObjectVersion,
    },
}

/// 投影的快照校验结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotCheck {
    /// 读到的输入仍是当前输入，结果可以当作当前依据。
    Current,
    /// 输入已经变化：结果只作审计，不作行动依据（研究目标 §5.3）。
    Drifted {
        /// 每一处漂移（按对象排序）。
        drifts: Vec<Drift>,
    },
}

/// 校验一份输入投影读到的输入是否仍是当前输入。
///
/// 用在两处：判断结果落记录的瞬间（请求期间输入变了，结果只留档），以及延迟返回的结果在被使用前
/// （`docs/RESEARCH_GOALS.md` §5.2「延迟返回结果的快照校验」）。空投影恒为 [`SnapshotCheck::Current`]。
pub fn verify_snapshot(log: &dyn EvidenceLog, projection: &InputProjection) -> SnapshotCheck {
    let mut drifts = Vec::new();
    for (object, read) in projection.objects() {
        match log.current_version(object) {
            Some(current) if current == read.version => {}
            Some(current) => drifts.push(Drift::VersionMoved {
                object: object.clone(),
                read: read.version,
                current,
            }),
            None => {
                // 该对象已经没有仍有效的记录：这一条已经说明了「读到的版本不可用」，
                // 不必再走下面的「该版本上有没有被撤回的记录」判定，否则同一处漂移报两次。
                drifts.push(Drift::Retracted {
                    object: object.clone(),
                    version: read.version,
                });
                continue;
            }
        }
        if log.retracted_at(object, read.version) {
            drifts.push(Drift::Retracted {
                object: object.clone(),
                version: read.version,
            });
        }
    }

    if drifts.is_empty() {
        SnapshotCheck::Current
    } else {
        SnapshotCheck::Drifted { drifts }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::evidence::{MemoryEvidenceLog, NewEvidence};

    fn append(log: &mut MemoryEvidenceLog, object: &str, version: u64) {
        log.append(NewEvidence {
            source: "tool:probe".to_owned(),
            at_unix_ms: 1_700_000_000_000,
            object: ObjectId::from(object),
            version: ObjectVersion(version),
            content: json!({ "version": version }),
        })
        .expect("内存日志不会失败");
    }

    #[test]
    fn projection_records_objects_and_fields() {
        let config = ObjectId::from("cache.conf");
        let log = ObjectId::from("app.log");
        let projection = InputProjection::covering([
            (config.clone(), ObjectVersion(3)),
            (log.clone(), ObjectVersion(1)),
        ])
        .read(
            config.clone(),
            ObjectRead::fields(ObjectVersion(3), ["cache".to_owned(), "ttl".to_owned()]),
        );

        assert_eq!(projection.len(), 2);
        assert!(!projection.is_empty());
        assert!(projection.reads(&config) && projection.reads(&log));
        assert_eq!(projection.version_of(&config), Some(ObjectVersion(3)));
        assert_eq!(projection.version_of(&ObjectId::from("nope")), None);

        let read = projection
            .objects()
            .find(|(id, _)| **id == config)
            .unwrap()
            .1;
        assert!(read.includes("cache") && read.includes("ttl"));
        assert!(!read.includes("retries"));
        // 没被 read() 覆盖过的对象保持整体读取。
        let whole = projection.objects().find(|(id, _)| **id == log).unwrap().1;
        assert!(whole.includes("anything"));

        let wire = serde_json::to_value(&projection).unwrap();
        assert_eq!(wire["objects"]["cache.conf"]["version"], json!(3));
        assert_eq!(
            wire["objects"]["cache.conf"]["fields"]["kind"],
            json!("named")
        );
        assert_eq!(wire["objects"]["app.log"]["fields"]["kind"], json!("all"));
        assert_eq!(
            serde_json::from_value::<InputProjection>(wire).unwrap(),
            projection
        );
    }

    #[test]
    fn empty_projection_is_always_current() {
        let log = MemoryEvidenceLog::default();
        assert_eq!(
            verify_snapshot(&log, &InputProjection::default()),
            SnapshotCheck::Current
        );
    }

    #[test]
    fn snapshot_check_reports_a_moved_version_and_a_retracted_read() {
        let mut log = MemoryEvidenceLog::default();
        append(&mut log, "cache.conf", 1);
        append(&mut log, "app.log", 1);
        let config = ObjectId::from("cache.conf");
        let log_object = ObjectId::from("app.log");

        let older = InputProjection::covering([
            (config.clone(), ObjectVersion(1)),
            (log_object.clone(), ObjectVersion(1)),
        ]);
        assert_eq!(verify_snapshot(&log, &older), SnapshotCheck::Current);

        // 对象出了新版本（id 3）：读旧版本（id 1）的投影漂移。
        append(&mut log, "cache.conf", 2);
        let SnapshotCheck::Drifted { drifts } = verify_snapshot(&log, &older) else {
            panic!("读到的版本已经不是当前版本");
        };
        assert_eq!(
            drifts,
            vec![Drift::VersionMoved {
                object: config.clone(),
                read: ObjectVersion(1),
                current: ObjectVersion(2),
            }]
        );
        // 读到的是最新版时没有漂移。
        let newest = InputProjection::covering([(config.clone(), ObjectVersion(2))]);
        assert_eq!(verify_snapshot(&log, &newest), SnapshotCheck::Current);

        // 撤回第 2 版：读它的投影既看到版本不再匹配，也看到读到的版本本身被撤回。
        log.retract(crate::evidence::EvidenceId(3))
            .expect("撤回第 2 版");
        let SnapshotCheck::Drifted { drifts } = verify_snapshot(&log, &newest) else {
            panic!("读到的版本已被撤回");
        };
        assert!(drifts.contains(&Drift::Retracted {
            object: config.clone(),
            version: ObjectVersion(2),
        }));
        assert!(drifts.contains(&Drift::VersionMoved {
            object: config.clone(),
            read: ObjectVersion(2),
            current: ObjectVersion(1),
        }));
        // 读过第 1 版的旧投影不受影响：它读的不是被撤回的那条记录。
        assert_eq!(verify_snapshot(&log, &older), SnapshotCheck::Current);

        assert_eq!(
            serde_json::to_value(SnapshotCheck::Current).unwrap(),
            json!("current")
        );
        assert_eq!(
            serde_json::to_value(&older).unwrap()["objects"]["cache.conf"]["fields"]["kind"],
            json!("all")
        );
    }

    #[test]
    fn a_fully_retracted_object_reports_its_drift_once() {
        let mut log = MemoryEvidenceLog::default();
        let only = log
            .append(NewEvidence {
                source: "tool:probe".to_owned(),
                at_unix_ms: 1_700_000_000_000,
                object: ObjectId::from("solo"),
                version: ObjectVersion(1),
                content: json!({ "x": 1 }),
            })
            .expect("内存日志不会失败");
        log.retract(only.id).expect("撤回唯一一条记录");

        let check = verify_snapshot(
            &log,
            &InputProjection::covering([(ObjectId::from("solo"), ObjectVersion(1))]),
        );

        // 「该对象已无仍有效记录」与「读到的版本被撤回」是同一处漂移，只能报一次。
        assert_eq!(
            check,
            SnapshotCheck::Drifted {
                drifts: vec![Drift::Retracted {
                    object: ObjectId::from("solo"),
                    version: ObjectVersion(1),
                }],
            }
        );
    }
}
