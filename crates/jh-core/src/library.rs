//! 问题库：新问题的最小契约、生命周期，以及发布/回退与失效的联动（研究目标 §6.2–§6.4）。
//!
//! 版本表本身（同一 ID + 同一版本不得换含义、改含义必须升版本）由 [`QuestionRegistry`] 强制；
//! 这个模块在它上面加三件事：
//!
//! - **最小契约**（§6.3 八字段）：命题与类型、区分目的、证据需求、适用范围、触发条件、下游用途、
//!   验证记录、生命周期。前六项在提出候选时一次给全（[`NewQuestion`]），后两项由库管；
//!   命题与类型仍只存一份（在版本表里），[`QuestionEntry`] 不重复存它，避免两份定义漂移；
//! - **生命周期**（[`Lifecycle`]）：候选 → 影子评估 → 启用 → 停用 / 被替代。
//!   [`QuestionLibrary::publish`] 把某个版本设为当前启用版本——影子评估过的版本是**晋升**，
//!   被替代过的版本是**回退**（§6.2 第 7、8 步）；[`QuestionLibrary::retire`] 让当前启用的版本
//!   退出使用（§6.4 的冗余处理）；[`QuestionLibrary::adopt`] 把已经成立的问题集直接按启用登记进库。
//!   旧版本一律保留，可比较、可回退；
//! - **与失效联动**（§6.2 第 8 步）：会改当前启用版本的三步（`adopt` / `publish` / `retire`）都收
//!   `&mut SemanticState`，调 [`SemanticState::invalidate_question`](crate::SemanticState::invalidate_question)
//!   把回答过这个问题的判断标成待重查、依赖它们的行动标成不能发出，并把这次传播原样返回给调用方。
//!
//! **没有消费者的候选不得常驻**（§6.3）：`consumers` 为空时 [`QuestionLibrary::propose`] 与
//! [`QuestionLibrary::adopt`] 显式失败（[`Error::QuestionWithoutConsumer`]），不落进库里——
//! 没有消费者的候选连记都不记，而不是记一条「非常驻」的标记。
//!
//! **不能只增不减**（§6.4）：停用与替代都不是删除——[`Lifecycle::Retired`] 与
//! [`Lifecycle::Superseded`] 只是状态，定义与历史都还在；合并冗余项不在 1.0（§6.4 只要求允许停用
//! 与替代，不要求自动合并算法）。
//!
//! # 生命周期迁移
//!
//! | 操作 | 允许的当前状态 | 迁移后 |
//! | --- | --- | --- |
//! | [`propose`](QuestionLibrary::propose) | 该 ID 与版本未登记 | 候选 |
//! | [`adopt`](QuestionLibrary::adopt) | 该 ID 与版本未登记 | 启用；同 ID 原先启用的版本 → 被替代 |
//! | [`start_shadow`](QuestionLibrary::start_shadow) | 候选 | 影子评估 |
//! | [`publish`](QuestionLibrary::publish) | 影子评估（晋升）、被替代（回退） | 启用；同 ID 原先启用的版本 → 被替代 |
//! | [`retire`](QuestionLibrary::retire) | 启用 | 停用（终态） |
//!
//! 其余组合显式失败（[`Error::InvalidLifecycle`]）：没经过影子评估的候选不能直接晋升
//! （§6.2 第 6、7 步的顺序），停用过的版本不能再启用（要恢复就回退到被替代的版本，或升一个新版本）。
//! 一个 ID 任何时刻**只有一个启用版本**——它由 [`QuestionLibrary::enabled`] 取，也是「当前定义」的
//! 唯一真身：`adopt` 与 `publish` 都会让原先启用的版本让位，不会留下两个（写入路径保证；从 JSON
//! 读回的库以入库文件为准，见下）。
//!
//! # 版本号规则（定案；待回写 `docs/roadmap-1.0.md` §12 未确定项 5）
//!
//! 1. 版本号是**每个问题 ID 各自从 1 开始的单调递增整数**（[`QuestionVersion`]`(u32)`），由库分配：
//!    提出候选、登记既有问题都取该 ID 当前最大版本 +1（[`QuestionRegistry::revise`]），调用方不写死；
//! 2. 同一 ID + 同一版本**不得换含义**（[`QuestionRegistry::register`] 强制，改含义必须拿到新编号）；
//! 3. **停用与回退不产生新版本号**：回退是把被替代的旧版本重新设为当前版本，不是新版本；
//! 4. 旧版本永不被覆盖或删除，只改生命周期状态——这是「可比较、可回退」的全部保证；
//! 5. 入库的问题库文件（与 `crates/jh-app/tasks/family_a.json` 同级的版本化 JSON）保存的就是这份
//!    编号与状态，后续分配从文件里的最大版本 +1 继续。
//!
//! # 与 §6.3 的差距
//!
//! 「证据需求」在 1.0 只表达**来源与字段**（[`NewQuestion::evidence`]）；§6.3 要求的「时效」与
//! 「获取方法」还没有独立字段——时效目前由判断时的输入投影版本体现，获取方法留在声明的文字里。
//! 这是登记在案的缺口，不是已经做到。其余七项按上文的类型逐项落位。
//!
//! # 落盘
//!
//! 本模块只管数据：[`QuestionLibrary`] 可 JSON 往返（入库的问题库文件），[`LifecycleRecord`] 是运行期
//! 晋升/回退的那一条记录（落 `var/` 的口径由 `jh-app` 决定）。文件读写不在本 crate。

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::evidence::ObjectId;
use crate::projection::FieldSelection;
use crate::question::{Question, QuestionId, QuestionRegistry, QuestionVersion};
use crate::runtime::{Invalidation, SemanticState};

/// 问题的生命周期状态（§6.2 第 7、8 步；§6.3「生命周期」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "lifecycle")]
pub enum Lifecycle {
    /// 候选：定义已登记，尚未评估，不参与控制。
    Candidate,
    /// 影子评估：在影子运行里观察它会不会改变有价值的行动分支（§6.2 第 6 步），仍不参与控制。
    Shadow,
    /// 启用：当前版本，消费方按它判定。
    Enabled,
    /// 停用：曾启用，现已退出使用（终态，§6.4）。
    Retired,
    /// 被替代：被 `by` 这个版本换下；发布新版本或回退到旧版本都会产生这一档。
    Superseded {
        /// 接替它成为当前启用版本的那个版本。
        by: QuestionVersion,
    },
}

impl Lifecycle {
    /// 是否正在参与控制：只有 [`Lifecycle::Enabled`] 算。
    pub fn is_enabled(&self) -> bool {
        matches!(self, Self::Enabled)
    }
}

impl std::fmt::Display for Lifecycle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Candidate => f.write_str("候选"),
            Self::Shadow => f.write_str("影子评估"),
            Self::Enabled => f.write_str("启用"),
            Self::Retired => f.write_str("停用"),
            Self::Superseded { by } => write!(f, "被 v{by} 替代"),
        }
    }
}

/// 一次验证的种类（§6.2 第 5、6 步与 §6.5）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationKind {
    /// 开发案例、正例与反例上的检验（§6.2 第 5 步）。
    Development,
    /// 影子运行：观察新问题会不会改变有价值的行动分支（§6.2 第 6 步）。
    ShadowRun,
    /// 未见变体上的迁移检验（§6.5）。
    UnseenVariant,
}

/// 一次验证的记录（§6.3「验证记录」）。
///
/// 读数只记原值，口径由调用方声明（与 [`Judgment::thresholds`](crate::Judgment::thresholds) 同一个
/// 口径）：本 crate 不解释读数，也不判晋升阈值——「满足预先规定的晋升标准后发布」（§6.2 第 7 步）
/// 里的标准属于实验层。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationRecord {
    /// 记录时间，Unix 毫秒；由调用方给出。
    pub at_unix_ms: u64,
    /// 这次验证在哪一步做的。
    pub kind: ValidationKind,
    /// 读数：名字 → 值（一致率、误差、费用……）。
    pub readings: BTreeMap<String, f64>,
    /// 说明：这次验证跑了哪些案例、结论是什么。
    pub note: String,
}

/// 一条生命周期变更的记录（追加写）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LifecycleRecord {
    /// 问题标识。
    pub question: QuestionId,
    /// 被改变的那个版本。
    pub version: QuestionVersion,
    /// 变更后的状态。
    pub lifecycle: Lifecycle,
    /// 变更时间，Unix 毫秒；由调用方给出。
    pub at_unix_ms: u64,
    /// 依据：晋升标准、影子评估读数、停用理由……由调用方写。
    pub reason: String,
}

/// 提出一个候选问题时要给出的最小契约（§6.3 的前六项）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewQuestion {
    /// 命题与类型。
    pub question: Question,
    /// 区分目的：它要区分哪些会导致不同动作的情形。
    pub distinguishes: String,
    /// 证据需求：所需来源与字段。版本在判定时由输入投影给出，这里不写。
    pub evidence: BTreeMap<ObjectId, FieldSelection>,
    /// 适用范围：对哪些任务、工具与环境版本适用。
    pub scope: String,
    /// 触发条件：何时值得评估、何时可跳过。
    pub trigger: String,
    /// 下游用途：哪项策略会消费答案（消费者标签）；**不能为空**（§6.3）。
    pub consumers: BTreeSet<String>,
}

/// 问题库里的一个版本：最小契约里除「命题与类型」外的各项 + 验证记录 + 生命周期。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuestionEntry {
    /// 区分目的。
    pub distinguishes: String,
    /// 证据需求。
    pub evidence: BTreeMap<ObjectId, FieldSelection>,
    /// 适用范围。
    pub scope: String,
    /// 触发条件。
    pub trigger: String,
    /// 下游用途（消费者标签）；不为空。
    pub consumers: BTreeSet<String>,
    /// 验证记录（按追加顺序）。
    pub validation: Vec<ValidationRecord>,
    /// 生命周期。
    pub lifecycle: Lifecycle,
}

/// 问题库：版本表 + 每个版本的契约与验证记录 + 生命周期变更历史。
///
/// 加版本只有 [`QuestionLibrary::propose`] 与 [`QuestionLibrary::adopt`]（都要求声明消费者），改状态
/// 只有 [`QuestionLibrary::start_shadow`] / [`QuestionLibrary::publish`] / [`QuestionLibrary::retire`]，
/// 记验证只有 [`QuestionLibrary::record_validation`]；[`QuestionLibrary::registry`] 只读借出，
/// 避免绕过生命周期直接加版本。与 [`QuestionRegistry`] 一样，从 JSON 读回的库不再重跑这些检查
/// ——入库文件是可信输入，校验在写入路径上。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct QuestionLibrary {
    registry: QuestionRegistry,
    entries: BTreeMap<QuestionId, BTreeMap<QuestionVersion, QuestionEntry>>,
    history: Vec<LifecycleRecord>,
}

impl QuestionLibrary {
    /// 提出一个候选问题：分配新版本、记下最小契约字段，落在[`Lifecycle::Candidate`]。
    ///
    /// 没有声明下游消费者时显式失败（[`Error::QuestionWithoutConsumer`]），且不占用版本号。
    pub fn propose(
        &mut self,
        id: &QuestionId,
        question: NewQuestion,
        at_unix_ms: u64,
        reason: String,
    ) -> Result<QuestionVersion, Error> {
        self.add(id, question, Lifecycle::Candidate, at_unix_ms, reason)
    }

    /// 登记一个**已经在用**的问题版本：不经过候选与影子评估，直接落在[`Lifecycle::Enabled`]。
    ///
    /// 入库的既有问题集（本版的任务族 A）用它进库——它们不是这一版新生成的候选，编造一条
    /// 「候选 → 影子 → 晋升」的历史反而会让记录失真。同一个既有问题有多个版本时按版本顺序依次登记
    /// （先 v1、再 v2）：最后登记的那个成为当前版本，前面那些记成被它替代，正是入库问题集
    /// 「老版本 + 当前版本」的形态。消费者同样必须声明。
    ///
    /// 与 [`QuestionLibrary::publish`] 一样，它改动的是「这个问题的当前定义」，所以收 `&mut state`
    /// 并做失效联动，返回新版本号与传播结果（写下的记录在 [`history`](Self::history) 里）。
    pub fn adopt(
        &mut self,
        state: &mut SemanticState,
        id: &QuestionId,
        question: NewQuestion,
        at_unix_ms: u64,
        reason: String,
    ) -> Result<(QuestionVersion, Invalidation), Error> {
        let version = self.add(id, question, Lifecycle::Enabled, at_unix_ms, reason.clone())?;
        self.supersede_enabled(id, version, at_unix_ms, reason)?;
        Ok((version, state.invalidate_question(id, version)))
    }

    /// 进入影子评估：候选 → 影子（§6.2 第 6 步）。
    pub fn start_shadow(
        &mut self,
        id: &QuestionId,
        version: QuestionVersion,
        at_unix_ms: u64,
        reason: String,
    ) -> Result<LifecycleRecord, Error> {
        match self.entry(id, version)?.lifecycle {
            Lifecycle::Candidate => {}
            lifecycle => {
                return Err(Error::InvalidLifecycle {
                    question: id.clone(),
                    version,
                    lifecycle,
                });
            }
        }
        self.set_lifecycle(id, version, Lifecycle::Shadow, at_unix_ms, reason)
    }

    /// 记一次验证（§6.2 第 5、6 步；§6.5 的未见变体）。
    pub fn record_validation(
        &mut self,
        id: &QuestionId,
        version: QuestionVersion,
        record: ValidationRecord,
    ) -> Result<(), Error> {
        self.entry_mut(id, version)?.validation.push(record);
        Ok(())
    }

    /// 发布：把某个版本设为当前启用版本（§6.2 第 7、8 步）。
    ///
    /// 影子评估过的版本是**晋升**，被替代过的版本是**回退**；同 ID 原先启用的版本记成
    /// [`Lifecycle::Superseded`]。发布之后立刻做失效联动：`state` 上回答过这个问题的判断（不管哪个
    /// 版本）都变成待重查、以它们为前提的行动变成不能发出——问题换了定义，旧的答案不能继续当依据。
    ///
    /// 返回这次写下的生命周期记录（新启用版本一条在前，被它换下的旧版本再一条）与失效传播结果。
    pub fn publish(
        &mut self,
        state: &mut SemanticState,
        id: &QuestionId,
        version: QuestionVersion,
        at_unix_ms: u64,
        reason: String,
    ) -> Result<(Vec<LifecycleRecord>, Invalidation), Error> {
        match self.entry(id, version)?.lifecycle {
            Lifecycle::Shadow | Lifecycle::Superseded { .. } => {}
            lifecycle => {
                return Err(Error::InvalidLifecycle {
                    question: id.clone(),
                    version,
                    lifecycle,
                });
            }
        }

        let mut records = vec![self.set_lifecycle(
            id,
            version,
            Lifecycle::Enabled,
            at_unix_ms,
            reason.clone(),
        )?];
        records.extend(self.supersede_enabled(id, version, at_unix_ms, reason)?);

        Ok((records, state.invalidate_question(id, version)))
    }

    /// 停用：让当前启用的版本退出使用（§6.4 的冗余处理）。
    ///
    /// 只接受当前启用的版本——失败候选与影子版本停在原状态就是它们的记录（§8.2「失败候选保留记录，
    /// 不悄悄覆盖有效版本」），不需要再标一次「停用」。停用是终态：要恢复就回退到被替代的版本，
    /// 或升一个新版本。
    ///
    /// 停用同样触发失效联动：这个问题不再有当前定义，回答过它任一版本的判断都不再是行动依据
    /// （判据与发布一致——停用当前版本 = 问题退出当前问题集）。
    pub fn retire(
        &mut self,
        state: &mut SemanticState,
        id: &QuestionId,
        version: QuestionVersion,
        at_unix_ms: u64,
        reason: String,
    ) -> Result<(LifecycleRecord, Invalidation), Error> {
        match self.entry(id, version)?.lifecycle {
            Lifecycle::Enabled => {}
            lifecycle => {
                return Err(Error::InvalidLifecycle {
                    question: id.clone(),
                    version,
                    lifecycle,
                });
            }
        }

        let record = self.set_lifecycle(id, version, Lifecycle::Retired, at_unix_ms, reason)?;
        Ok((record, state.invalidate_question(id, version)))
    }

    /// 版本表（只读）：比较同一 ID 的两个版本、翻版本历史用它。
    pub fn registry(&self) -> &QuestionRegistry {
        &self.registry
    }

    /// 某个版本的问题定义（存在版本表里）。
    pub fn question(&self, id: &QuestionId, version: QuestionVersion) -> Option<&Question> {
        self.registry.at(id, version)
    }

    /// 某个版本的契约字段、验证记录与生命周期。
    pub fn at(&self, id: &QuestionId, version: QuestionVersion) -> Option<&QuestionEntry> {
        self.entries.get(id)?.get(&version)
    }

    /// 某个 ID 当前的启用版本与它的契约字段；没有启用中的版本时为 `None`。
    pub fn enabled(&self, id: &QuestionId) -> Option<(QuestionVersion, &QuestionEntry)> {
        self.entries
            .get(id)?
            .iter()
            .find(|(_, entry)| entry.lifecycle.is_enabled())
            .map(|(version, entry)| (*version, entry))
    }

    /// 某个 ID 的全部版本（按版本号升序），含已停用与被替代的。
    pub fn versions(&self, id: &QuestionId) -> Vec<(QuestionVersion, &QuestionEntry)> {
        self.entries
            .get(id)
            .map(|versions| versions.iter().map(|(v, e)| (*v, e)).collect())
            .unwrap_or_default()
    }

    /// 库里的问题 ID（按标识排序）。
    pub fn ids(&self) -> impl Iterator<Item = &QuestionId> {
        self.entries.keys()
    }

    /// 生命周期变更历史（按发生顺序追加）。
    pub fn history(&self) -> &[LifecycleRecord] {
        &self.history
    }

    fn add(
        &mut self,
        id: &QuestionId,
        question: NewQuestion,
        lifecycle: Lifecycle,
        at_unix_ms: u64,
        reason: String,
    ) -> Result<QuestionVersion, Error> {
        if question.consumers.is_empty() {
            return Err(Error::QuestionWithoutConsumer(id.clone()));
        }
        let NewQuestion {
            question,
            distinguishes,
            evidence,
            scope,
            trigger,
            consumers,
        } = question;

        let version = self.registry.revise(id, question);
        self.entries.entry(id.clone()).or_default().insert(
            version,
            QuestionEntry {
                distinguishes,
                evidence,
                scope,
                trigger,
                consumers,
                validation: Vec::new(),
                lifecycle,
            },
        );
        self.history.push(LifecycleRecord {
            question: id.clone(),
            version,
            lifecycle,
            at_unix_ms,
            reason,
        });
        Ok(version)
    }

    fn set_lifecycle(
        &mut self,
        id: &QuestionId,
        version: QuestionVersion,
        lifecycle: Lifecycle,
        at_unix_ms: u64,
        reason: String,
    ) -> Result<LifecycleRecord, Error> {
        self.entry_mut(id, version)?.lifecycle = lifecycle;
        let record = LifecycleRecord {
            question: id.clone(),
            version,
            lifecycle,
            at_unix_ms,
            reason,
        };
        self.history.push(record.clone());
        Ok(record)
    }

    /// 让 `version` 之外仍启用的版本让位：记成被 `version` 替代，返回写下的记录。
    ///
    /// 一个 ID 任何时刻只应有一个启用版本；`adopt` 与 `publish` 都从这里保持这条不变量
    /// （调用前应已把 `version` 置为启用，本函数只处理其余的）。
    fn supersede_enabled(
        &mut self,
        id: &QuestionId,
        version: QuestionVersion,
        at_unix_ms: u64,
        reason: String,
    ) -> Result<Vec<LifecycleRecord>, Error> {
        let displaced: Vec<QuestionVersion> = self
            .versions(id)
            .into_iter()
            .filter(|(other, entry)| *other != version && entry.lifecycle.is_enabled())
            .map(|(other, _)| other)
            .collect();

        let mut records = Vec::with_capacity(displaced.len());
        for previous in displaced {
            records.push(self.set_lifecycle(
                id,
                previous,
                Lifecycle::Superseded { by: version },
                at_unix_ms,
                reason.clone(),
            )?);
        }
        Ok(records)
    }

    fn entry(&self, id: &QuestionId, version: QuestionVersion) -> Result<&QuestionEntry, Error> {
        self.at(id, version)
            .ok_or_else(|| Error::UnknownQuestionVersion(id.clone(), version))
    }

    fn entry_mut(
        &mut self,
        id: &QuestionId,
        version: QuestionVersion,
    ) -> Result<&mut QuestionEntry, Error> {
        self.entries
            .get_mut(id)
            .and_then(|versions| versions.get_mut(&version))
            .ok_or_else(|| Error::UnknownQuestionVersion(id.clone(), version))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::graph::ActionId;
    use crate::question::{NoulQuestion, QuestionSpec};
    use crate::record::{JudgeSource, Judgment, JudgmentRecord};
    use crate::state::SnapshotId;
    use crate::{Answer, InputProjection, NoulAnswer};

    fn noul(instructions: &str) -> Question {
        Question::Noul(NoulQuestion {
            instructions: instructions.to_owned(),
            criteria: None,
        })
    }

    fn candidate(instructions: &str) -> NewQuestion {
        NewQuestion {
            question: noul(instructions),
            distinguishes: "证书过期还是时钟偏差".to_owned(),
            evidence: BTreeMap::from([(ObjectId::from("cert.json"), FieldSelection::All)]),
            scope: "B 族任务、probe 工具、环境版本 1".to_owned(),
            trigger: "轮询里出现证书类告警时评估".to_owned(),
            consumers: BTreeSet::from(["strategy:renew-or-fix-clock".to_owned()]),
        }
    }

    /// 在 `state` 上声明一个问题节点并写入一次判断（供失效联动的断言用）。
    fn judge(state: &mut SemanticState, id: &str, version: u32) -> crate::NodeId {
        let node = state.declare_node(
            QuestionId::from(id),
            QuestionVersion(version),
            InputProjection::default(),
        );
        let record = JudgmentRecord {
            snapshot: SnapshotId::from("case-001"),
            source: JudgeSource::Rule,
            version: "rule-v1".to_owned(),
            request_id: None,
            usage: None,
            latency_ms: 1,
            judgments: BTreeMap::from([(
                QuestionId::from(id),
                Judgment {
                    question_version: QuestionVersion(version),
                    answer: Answer::Noul(NoulAnswer { noul: 1.0 }),
                    thresholds: BTreeMap::new(),
                },
            )]),
        };
        state.record_judgment(node, record).expect("记录与节点一致");
        node
    }

    #[test]
    fn propose_allocates_versions_and_refuses_a_candidate_without_consumers() {
        let mut library = QuestionLibrary::default();
        let id = QuestionId::from("cause_b_pair");

        let first = library
            .propose(
                &id,
                candidate("证书过期还是时钟偏差？"),
                1_700_000_000_000,
                "从反例 cx-1 生成".to_owned(),
            )
            .expect("候选人可登记");
        assert_eq!(first, QuestionVersion(1));
        assert_eq!(
            library.at(&id, first).map(|entry| entry.lifecycle),
            Some(Lifecycle::Candidate)
        );
        assert_eq!(library.enabled(&id), None, "候选不参与控制");

        // 同一 ID 再提一个：版本号取当前最大 +1，旧版本原样留着。
        let second = library
            .propose(
                &id,
                candidate("证书还有多久到期？"),
                1_700_000_001_000,
                "换一个区分目的".to_owned(),
            )
            .expect("候选人可登记");
        assert_eq!(second, QuestionVersion(2));
        assert_eq!(library.versions(&id).len(), 2);
        assert_eq!(
            library.registry().compare(&id, first, second),
            Some(crate::VersionComparison::Changed)
        );

        // 没有消费者的候选连记都不记，也不占用版本号。
        let bare = QuestionId::from("no_consumer");
        let error = library
            .propose(
                &bare,
                NewQuestion {
                    consumers: BTreeSet::new(),
                    ..candidate("没人消费的问题")
                },
                1_700_000_002_000,
                "生成模型漏了消费者".to_owned(),
            )
            .expect_err("没有消费者的候选必须显式失败");
        assert!(
            matches!(error, Error::QuestionWithoutConsumer(_)),
            "{error}"
        );
        assert!(error.to_string().contains("no_consumer"), "{error}");
        assert!(
            library.registry().history(&bare).is_empty(),
            "版本号没被占掉"
        );
        assert_eq!(library.enabled(&bare), None);
    }

    #[test]
    fn adopt_lands_on_enabled_and_still_requires_a_consumer() {
        let mut library = QuestionLibrary::default();
        let mut state = SemanticState::in_memory();
        let id = QuestionId::from("cause_primary");

        let (version, invalidation) = library
            .adopt(
                &mut state,
                &id,
                candidate("主因是什么？"),
                1_700_000_000_000,
                "既有问题集入库".to_owned(),
            )
            .expect("既有问题可登记");
        assert_eq!(
            library.enabled(&id).map(|(version, _)| version),
            Some(version)
        );
        assert_eq!(library.question(&id, version), Some(&noul("主因是什么？")));
        assert!(invalidation.is_empty(), "还没有判断，没有可失效的东西");

        let error = library
            .adopt(
                &mut state,
                &QuestionId::from("bare"),
                NewQuestion {
                    consumers: BTreeSet::new(),
                    ..candidate("没人消费的问题")
                },
                1_700_000_000_000,
                "入库".to_owned(),
            )
            .expect_err("没有消费者同样不许入库");
        assert!(
            matches!(error, Error::QuestionWithoutConsumer(_)),
            "{error}"
        );
    }

    #[test]
    fn adopting_the_versions_of_an_existing_question_set_leaves_one_enabled_version() {
        // 入库的既有问题集里同一个问题可能有多个版本（任务族 A 的 `cause_primary` v1 → v2）：
        // 按版本顺序依次登记，最后那个成为当前版本，前面的记成被它替代。
        let mut library = QuestionLibrary::default();
        let mut state = SemanticState::in_memory();
        let id = QuestionId::from("cause_primary");

        let (v1, _) = library
            .adopt(&mut state, &id, candidate("旧问法"), 100, "入库".to_owned())
            .unwrap();
        let on_v1 = judge(&mut state, "cause_primary", v1.0);
        let (v2, invalidation) = library
            .adopt(&mut state, &id, candidate("新问法"), 110, "入库".to_owned())
            .unwrap();

        assert_eq!((v1, v2), (QuestionVersion(1), QuestionVersion(2)));
        assert_eq!(
            library.at(&id, v1).unwrap().lifecycle,
            Lifecycle::Superseded { by: v2 }
        );
        assert_eq!(
            library.enabled(&id).map(|(version, _)| version),
            Some(v2),
            "一个 ID 只有一个启用版本，取到的是最后登记的那个"
        );
        // 登记新版本就是换当前定义：答过旧版本的判断待重查（与 publish 同一口径）。
        assert_eq!(invalidation.stale, vec![on_v1]);
        assert_eq!(
            state.node(on_v1).unwrap().validity,
            crate::Validity::NeedsRecheck
        );
        // 旧版本可比较、可回退。
        assert_eq!(library.question(&id, v1), Some(&noul("旧问法")));
        assert_eq!(library.versions(&id).len(), 2);
        let (records, invalidation) = library
            .publish(&mut state, &id, v1, 120, "回退到入库时的旧问法".to_owned())
            .unwrap();
        assert_eq!(records[0].lifecycle, Lifecycle::Enabled);
        assert!(
            invalidation.is_empty(),
            "唯一答过的那个判断已经待重查，没有新的失效"
        );
        assert_eq!(library.enabled(&id).map(|(version, _)| version), Some(v1));
    }

    #[test]
    fn the_lifecycle_walks_candidate_shadow_enabled_retired() {
        let mut library = QuestionLibrary::default();
        let mut state = SemanticState::in_memory();
        let id = QuestionId::from("cause_b_pair");
        let version = library
            .propose(
                &id,
                candidate("证书过期还是时钟偏差？"),
                100,
                "生成".to_owned(),
            )
            .unwrap();
        library
            .record_validation(
                &id,
                version,
                ValidationRecord {
                    at_unix_ms: 110,
                    kind: ValidationKind::Development,
                    readings: BTreeMap::from([
                        ("positive_cases".to_owned(), 3.0),
                        ("negative_cases".to_owned(), 3.0),
                    ]),
                    note: "B1 的 3 对正反例".to_owned(),
                },
            )
            .unwrap();

        // 候选不能直接晋升：§6.2 第 6 步的影子运行还没做。
        let error = library
            .publish(&mut state, &id, version, 120, "跳过影子".to_owned())
            .expect_err("候选不能直接晋升");
        assert!(matches!(error, Error::InvalidLifecycle { .. }), "{error}");

        let shadow = library
            .start_shadow(&id, version, 130, "进影子运行".to_owned())
            .expect("候选可进影子");
        assert_eq!(shadow.lifecycle, Lifecycle::Shadow);
        let error = library
            .start_shadow(&id, version, 140, "再进一次影子".to_owned())
            .expect_err("已在影子里的版本不能再进影子");
        assert!(matches!(error, Error::InvalidLifecycle { .. }), "{error}");

        let (records, invalidation) = library
            .publish(&mut state, &id, version, 150, "影子读数达标".to_owned())
            .expect("影子评估过的版本可晋升");
        assert_eq!(records.len(), 1, "没有旧版本让位：{records:?}");
        assert_eq!(records[0].lifecycle, Lifecycle::Enabled);
        assert!(invalidation.is_empty(), "还没有判断，没有可失效的东西");
        assert_eq!(
            library.enabled(&id).map(|(version, _)| version),
            Some(version)
        );

        let error = library
            .publish(&mut state, &id, version, 160, "已经是当前版本".to_owned())
            .expect_err("已经启用的版本不能再发布");
        assert!(matches!(error, Error::InvalidLifecycle { .. }), "{error}");

        let (retired, invalidation) = library
            .retire(&mut state, &id, version, 170, "被新观测器取代".to_owned())
            .expect("启用中的版本可停用");
        assert_eq!(retired.lifecycle, Lifecycle::Retired);
        assert!(invalidation.is_empty());
        assert_eq!(library.enabled(&id), None);

        let error = library
            .retire(&mut state, &id, version, 180, "再停一次".to_owned())
            .expect_err("停用是终态");
        assert!(matches!(error, Error::InvalidLifecycle { .. }), "{error}");
        let error = library
            .publish(&mut state, &id, version, 190, "停用后再启用".to_owned())
            .expect_err("停用过的版本不能回到当前版本");
        assert!(matches!(error, Error::InvalidLifecycle { .. }), "{error}");
    }

    #[test]
    fn publishing_a_new_version_supersedes_the_enabled_one_and_invalidates_its_judgments() {
        let mut library = QuestionLibrary::default();
        let mut state = SemanticState::in_memory();
        let id = QuestionId::from("cause_b_pair");

        let (first, _) = library
            .adopt(
                &mut state,
                &id,
                candidate("旧问法"),
                100,
                "既有问题".to_owned(),
            )
            .unwrap();
        let older = judge(&mut state, "cause_b_pair", first.0);
        let unrelated = judge(&mut state, "premise_holds", 1);
        let action = ActionId::from("fix-clock");
        state.declare_action(action.clone(), [older]).unwrap();

        // 新版本走完候选 → 影子 → 晋升。
        let second = library
            .propose(&id, candidate("新问法"), 110, "从反例 cx-1 生成".to_owned())
            .unwrap();
        library
            .start_shadow(&id, second, 120, "影子运行".to_owned())
            .unwrap();
        let (records, invalidation) = library
            .publish(&mut state, &id, second, 130, "未见变体 6/6".to_owned())
            .unwrap();

        assert_eq!(
            records
                .iter()
                .map(|record| (record.version, record.lifecycle))
                .collect::<Vec<_>>(),
            vec![
                (second, Lifecycle::Enabled),
                (first, Lifecycle::Superseded { by: second })
            ],
            "先记新启用版本，再记被它换下的旧版本"
        );
        // 判据换版本：回答过这个问题的判断（旧版本那条）待重查，以它为前提的行动不能发出。
        assert_eq!(invalidation.stale, vec![older]);
        assert_eq!(invalidation.blocked, vec![action.clone()]);
        assert_eq!(
            invalidation.cause,
            crate::InvalidationCause::QuestionVersionChanged {
                question: id.clone(),
                version: second,
            }
        );
        assert_eq!(
            state.node(unrelated).unwrap().validity,
            crate::Validity::Current
        );
        assert_eq!(
            state.check_action(&action).unwrap(),
            crate::PremiseCheck::Blocked {
                premises: vec![older]
            }
        );
        // 旧版本没被删：可比较、可作为回退目标。
        assert_eq!(library.versions(&id).len(), 2);
        assert_eq!(library.question(&id, first), Some(&noul("旧问法")));
        assert_eq!(
            library.enabled(&id).map(|(version, _)| version),
            Some(second)
        );
    }

    #[test]
    fn rolling_back_re_enables_a_superseded_version_and_invalidates_again() {
        let mut library = QuestionLibrary::default();
        let mut state = SemanticState::in_memory();
        let id = QuestionId::from("cause_b_pair");

        let (first, _) = library
            .adopt(
                &mut state,
                &id,
                candidate("旧问法"),
                100,
                "既有问题".to_owned(),
            )
            .unwrap();
        let second = library
            .propose(&id, candidate("新问法"), 110, "生成".to_owned())
            .unwrap();
        library
            .start_shadow(&id, second, 120, "影子".to_owned())
            .unwrap();
        library
            .publish(&mut state, &id, second, 130, "晋升".to_owned())
            .unwrap();

        // 新版本上的判断，以及撤回之后重新判断的那条旧版本判断。
        let on_second = judge(&mut state, "cause_b_pair", second.0);
        let (records, invalidation) = library
            .publish(
                &mut state,
                &id,
                first,
                140,
                "新版本在未见变体上退化，回退".to_owned(),
            )
            .unwrap();

        assert_eq!(
            records
                .iter()
                .map(|record| (record.version, record.lifecycle))
                .collect::<Vec<_>>(),
            vec![
                (first, Lifecycle::Enabled),
                (second, Lifecycle::Superseded { by: first }),
            ],
            "回退把被替代的旧版本重新设为当前版本，新版本让位"
        );
        assert_eq!(invalidation.stale, vec![on_second]);
        assert_eq!(
            library.enabled(&id).map(|(version, _)| version),
            Some(first)
        );
        // 回退不产生新版本号：库里仍然只有两个版本。
        assert_eq!(library.versions(&id).len(), 2);

        // 生命周期变更都留在历史里，按发生顺序可查。
        // 登记在用版本 1 条 + 候选 1 条 + 进影子 1 条 + 晋升时的新旧各 1 条 + 回退时的新旧各 1 条。
        assert_eq!(library.history().len(), 7, "{:?}", library.history());
        let rollback = &library.history()[5..];
        assert_eq!(
            rollback
                .iter()
                .map(|record| (record.version, record.lifecycle))
                .collect::<Vec<_>>(),
            vec![
                (first, Lifecycle::Enabled),
                (second, Lifecycle::Superseded { by: first }),
            ]
        );
        assert!(rollback.iter().all(|record| record.at_unix_ms == 140));
    }

    #[test]
    fn only_the_enabled_version_can_be_retired_and_it_leaves_the_question_set_alone_until_then() {
        let mut library = QuestionLibrary::default();
        let mut state = SemanticState::in_memory();
        let id = QuestionId::from("cause_b_pair");

        let (enabled, _) = library
            .adopt(
                &mut state,
                &id,
                candidate("在用问法"),
                100,
                "入库".to_owned(),
            )
            .unwrap();
        let candidate = library
            .propose(&id, candidate("候选问法"), 110, "生成".to_owned())
            .unwrap();
        let node = judge(&mut state, "cause_b_pair", enabled.0);

        // 候选没在用：停用它没有意义，显式失败，也不牵动当前版本的判断。
        let error = library
            .retire(&mut state, &id, candidate, 120, "不要这个候选".to_owned())
            .expect_err("只有当前启用的版本能停用");
        assert!(matches!(error, Error::InvalidLifecycle { .. }), "{error}");
        assert_eq!(
            library.at(&id, candidate).unwrap().lifecycle,
            Lifecycle::Candidate
        );
        assert_eq!(state.node(node).unwrap().validity, crate::Validity::Current);

        // 停用在用的版本：该问题退出当前问题集，回答过它的判断不再作为依据。
        let (record, invalidation) = library
            .retire(
                &mut state,
                &id,
                enabled,
                130,
                "冗余：与观测器 X 重复".to_owned(),
            )
            .unwrap();
        assert_eq!(record.lifecycle, Lifecycle::Retired);
        assert_eq!(invalidation.stale, vec![node]);
        assert_eq!(
            state.node(node).unwrap().validity,
            crate::Validity::NeedsRecheck
        );
        assert_eq!(state.recheck_set(), vec![node]);
        assert_eq!(library.enabled(&id), None, "问题已退出当前问题集");
        // 不能只增不减：停用只是状态，定义与历史都还在。
        assert_eq!(library.versions(&id).len(), 2);
    }

    #[test]
    fn validation_records_are_kept_per_version_and_readings_stay_raw() {
        let mut library = QuestionLibrary::default();
        let id = QuestionId::from("cause_b_pair");
        let version = library
            .propose(
                &id,
                candidate("证书过期还是时钟偏差？"),
                100,
                "生成".to_owned(),
            )
            .unwrap();

        library
            .record_validation(
                &id,
                version,
                ValidationRecord {
                    at_unix_ms: 110,
                    kind: ValidationKind::ShadowRun,
                    readings: BTreeMap::from([
                        ("action_branch_changed".to_owned(), 0.25),
                        ("cost_units".to_owned(), 42.0),
                    ]),
                    note: "影子运行：3 对 × 2 变体".to_owned(),
                },
            )
            .unwrap();
        library
            .record_validation(
                &id,
                version,
                ValidationRecord {
                    at_unix_ms: 120,
                    kind: ValidationKind::UnseenVariant,
                    readings: BTreeMap::from([("agreement".to_owned(), 0.833)]),
                    note: "未见变体".to_owned(),
                },
            )
            .unwrap();

        let entry = library.at(&id, version).unwrap();
        assert_eq!(entry.validation.len(), 2);
        assert_eq!(entry.validation[0].kind, ValidationKind::ShadowRun);
        assert_eq!(entry.validation[0].readings["cost_units"], 42.0);
        assert_eq!(
            entry.validation[1].readings["agreement"], 0.833,
            "读数按原值保存，不折成布尔也不判阈值"
        );

        let error = library
            .record_validation(
                &id,
                QuestionVersion(9),
                ValidationRecord {
                    at_unix_ms: 130,
                    kind: ValidationKind::Development,
                    readings: BTreeMap::new(),
                    note: String::new(),
                },
            )
            .expect_err("没有这个版本");
        assert!(
            matches!(error, Error::UnknownQuestionVersion(_, _)),
            "{error}"
        );
        assert!(error.to_string().contains("cause_b_pair"), "{error}");
    }

    #[test]
    fn the_library_round_trips_through_json_with_its_lifecycle_and_history() {
        let mut library = QuestionLibrary::default();
        let mut state = SemanticState::in_memory();
        let id = QuestionId::from("cause_b_pair");
        let (first, _) = library
            .adopt(
                &mut state,
                &id,
                candidate("在用问法"),
                100,
                "入库".to_owned(),
            )
            .unwrap();
        let second = library
            .propose(&id, candidate("候选问法"), 110, "生成".to_owned())
            .unwrap();
        library
            .start_shadow(&id, second, 120, "影子".to_owned())
            .unwrap();
        library
            .publish(&mut state, &id, second, 130, "晋升".to_owned())
            .unwrap();

        let wire = serde_json::to_value(&library).unwrap();
        assert_eq!(
            wire["registry"]["versions"]["cause_b_pair"]["1"]["type"],
            json!("noul")
        );
        assert_eq!(
            wire["entries"]["cause_b_pair"]["2"]["lifecycle"]["lifecycle"],
            json!("enabled")
        );
        assert_eq!(
            wire["entries"]["cause_b_pair"]["1"]["lifecycle"],
            json!({"lifecycle": "superseded", "by": 2})
        );
        assert_eq!(
            wire["entries"]["cause_b_pair"]["2"]["consumers"],
            json!(["strategy:renew-or-fix-clock"])
        );
        assert_eq!(
            wire["history"].as_array().unwrap().len(),
            5,
            "登记 1 + 候选 1 + 进影子 1 + 晋升 2"
        );

        let round_tripped: QuestionLibrary = serde_json::from_value(wire).unwrap();
        assert_eq!(round_tripped, library);
        assert_eq!(
            round_tripped.enabled(&id).map(|(version, _)| version),
            Some(second)
        );
        assert_eq!(
            round_tripped.at(&id, first).unwrap().lifecycle,
            Lifecycle::Superseded { by: second }
        );
        // 库的查询面在同一份数据上给出的答案一致。
        assert_eq!(round_tripped.ids().count(), 1);
        assert_eq!(round_tripped.registry().ids().count(), 1);
    }

    #[test]
    fn a_question_spec_can_be_built_from_the_library_for_a_request() {
        // 判断请求要的是 QuestionSpec（版本 + 定义）；库里取一份就是这两样。
        let mut library = QuestionLibrary::default();
        let mut state = SemanticState::in_memory();
        let id = QuestionId::from("cause_b_pair");
        let (adopted, _) = library
            .adopt(
                &mut state,
                &id,
                candidate("在用问法"),
                100,
                "入库".to_owned(),
            )
            .unwrap();

        let (version, entry) = library.enabled(&id).expect("有启用版本");
        assert_eq!(version, adopted, "启用版本就是刚登记的那个");
        let spec = QuestionSpec {
            version,
            question: library
                .question(&id, version)
                .expect("定义在版本表里")
                .clone(),
        };

        assert_eq!(spec.version, version);
        assert_eq!(spec.question, noul("在用问法"));
        assert_eq!(entry.consumers.len(), 1);
        assert_eq!(
            entry.evidence[&ObjectId::from("cert.json")],
            FieldSelection::All,
            "证据需求只声明来源与字段，不写版本（版本在判定时由输入投影给出）"
        );
        assert_eq!(entry.evidence.len(), 1);
    }
}
