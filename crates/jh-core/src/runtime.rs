//! 可失效的语义状态运行时：判断节点、评估状态与依据有效性、失效传播、行动前提复核。
//!
//! 研究目标 §5.1 的诉求是「外部变化到来时，系统知道哪些判断需要重查，哪些动作应暂停，哪些工作
//! 仍可继续」。这个模块把那句话落成一组对象与规则：
//!
//! - **判断节点**（[`JudgmentNode`]）：某个问题在某个版本上、按一份输入投影做出的一次判断。节点不
//!   可改写——重新判断会得到新节点，旧节点留着当审计与回退；
//! - **两个维度分开管**（§5.3）：[`EvaluationStatus`] 记「有没有拿到一次有效响应」，
//!   [`Validity`] 记「还能不能作为依据」；答案内容本身留在 [`JudgmentRecord`] 里，三者互不覆盖；
//! - **失效传播**：撤回一条证据、撤销一个判断，或追加一条新证据，都从依赖图往下游传播，
//!   把受影响的判断标成待重查、把以它们为前提的行动标成不能发出，无关结果原样保留；
//! - **行动前提复核**（§5.2）：行动发出前调 [`SemanticState::check_action`]；撤销只改变使用资格，
//!   已经发生的外部副作用需要独立的补偿动作，本 crate 不自动回滚。
//!
//! # 传播规则
//!
//! 失效传播按**对象**保守判定，不猜测某个判断究竟看了对象的哪一版记录（§5.3「不能在返回后猜测模型
//! 只使用了其中某几项」）。**依赖图是传播的唯一依据**：只要有一条「对象 → 判断」的读取边
//! （[`SemanticState::declare_node`] 按输入投影登记的，或调用方经 [`SemanticState::graph_mut`]
//! 手工加的，两种来源一视同仁），这条判断就算读过该对象。
//!
//! | 起因 | 谁受影响 |
//! | --- | --- |
//! | 追加了一条属于对象 `O` 的证据 | 所有读过 `O` 的判断——它们没看到这条新记录 |
//! | 撤回了一条属于对象 `O` 的证据 | 所有读过 `O` 的判断——被撤回的记录可能就在它们的材料里 |
//! | 撤销判断 | 由它支持的下游判断、以它为前提的行动 |
//! | 全量重查 | 所有当前有效的判断与行动（保守出口，不沿路径） |
//!
//! 冲突候选是对称关系，两种情形都不传播（见 [`DependencyGraph::cycles`](crate::DependencyGraph)）。
//! 这样选的代价是误失效：撤回一条早被新版本取代的旧记录，也会波及只读了新版本的判断。方向是刻意的
//! ——漏失效会让运行时拿着作废的依据继续行动，误失效只是多查几次；两者都要在实验里分别统计
//! （§5.4、§10.2）。
//!
//! # 请求 state 与读取版本
//!
//! 输入投影里的**版本只用于登记时的快照校验**（[`SemanticState::verify`] 与
//! [`SemanticState::record_judgment`]：读到的版本必须仍是当前版本、且该版本上没有已撤回的记录），
//! **不参与传播筛选**。因此装配 state 时即使混进了同一对象的旧版本记录，也不会出现「撤回了一条
//! 判断其实读过、但投影没写的记录，判断静默保持当前有效」的漏失效——对象级传播兜住。
//!
//! 仍然推荐只把 [`current_evidence`](crate::EvidenceLog::current_evidence) 装进 state：
//!
//! - 装旧版本会让模型看到已被取代的材料，答案本身就是过期的；
//! - 投影里写下的版本应当是这份 state 的读取版本——装了当前版本，就写
//!   [`current_version`](crate::EvidenceLog::current_version) 给出的那个。
//!
//! # 示例
//!
//! ```
//! use std::collections::BTreeMap;
//!
//! use jh_core::{
//!     ActionId, Answer, Error, InputProjection, JudgeSource, Judgment, JudgmentRecord, NewEvidence,
//!     NoulAnswer, ObjectId, ObjectVersion, PremiseCheck, QuestionId, QuestionVersion,
//!     SemanticState, SnapshotId, Validity,
//! };
//!
//! # fn main() -> Result<(), Error> {
//! let mut state = SemanticState::in_memory();
//! let config = ObjectId::from("cache.conf");
//!
//! // 工具输出先落成不可变证据：来源、时间、对象版本与内容摘要都在记录里。
//! let (evidence, _) = state.append_evidence(NewEvidence {
//!     source: "tool:read_config".to_owned(),
//!     at_unix_ms: 1_700_000_000_000,
//!     object: config.clone(),
//!     version: ObjectVersion(1),
//!     content: serde_json::json!({"cache": "off"}),
//! })?;
//!
//! // 判断节点声明输入投影：读了哪些对象与字段（共享一份请求 state 时整批共用一份）。
//! let question = QuestionId::from("cache_disabled");
//! let node = state.declare_node(
//!     question.clone(),
//!     QuestionVersion(1),
//!     InputProjection::covering([(config, ObjectVersion(1))]),
//! );
//!
//! // 写入这次判断（记录怎么由请求 / 响应构造见 JudgmentRecord::from_response）。
//! state.record_judgment(
//!     node,
//!     JudgmentRecord {
//!         snapshot: SnapshotId::from("case-001"),
//!         source: JudgeSource::Rule,
//!         version: "rule-v1".to_owned(),
//!         request_id: None,
//!         usage: None,
//!         latency_ms: 3,
//!         judgments: BTreeMap::from([(
//!             question,
//!             Judgment {
//!                 question_version: QuestionVersion(1),
//!                 answer: Answer::Noul(NoulAnswer { noul: 1.0 }),
//!                 thresholds: BTreeMap::new(),
//!             },
//!         )]),
//!     },
//! )?;
//!
//! // 行动以判断为前提；发出之前复核。
//! let action = ActionId::from("skip-cache-warmup");
//! state.declare_action(action.clone(), [node])?;
//! assert_eq!(state.check_action(&action)?, PremiseCheck::Cleared);
//!
//! // 撤回这条证据：读了它的判断失效，相关行动发不出去，无关结果保留。
//! let invalidation = state.retract_evidence(evidence.id)?;
//! assert_eq!(invalidation.stale, vec![node]);
//! assert_eq!(state.node(node).expect("刚声明过").validity, Validity::NeedsRecheck);
//! assert_eq!(
//!     state.check_action(&action)?,
//!     PremiseCheck::Blocked { premises: vec![node] }
//! );
//! # Ok(())
//! # }
//! ```

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::evidence::{
    Evidence, EvidenceId, EvidenceLog, MemoryEvidenceLog, NewEvidence, ObjectId,
};
use crate::graph::{
    ActionId, DependencyGraph, EdgeKind, EdgeOrigin, NodeId, NodeRef, PropagationPath,
};
use crate::projection::{InputProjection, SnapshotCheck, verify_snapshot};
use crate::question::{QuestionId, QuestionVersion, Questions};
use crate::record::{Judgment, JudgmentRecord};

/// 评估状态：是否取得过一次有效响应（研究目标 §5.3 的第一个维度）。
///
/// 缺失记录与调用失败都不会自动折成「否」，也不会自动变成「证据不足」：它只是评估状态。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "evaluation", content = "detail")]
pub enum EvaluationStatus {
    /// 未请求。
    NotRequested,
    /// 已排进请求队列。
    Queued,
    /// 请求进行中。
    Running,
    /// 已取得一次有效响应；由 [`SemanticState::record_judgment`] 写入。
    Succeeded,
    /// 调用失败：文本取判断器错误的原文，本 crate 不改写它。
    Failed(String),
}

/// 依据有效性：对指定对象版本能否继续引用（研究目标 §5.3 的第二个维度）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "validity")]
pub enum Validity {
    /// 当前有效：可以作为行动依据。
    Current,
    /// 待重查：输入或依据已变，重新判断之前不能作为行动依据。
    NeedsRecheck,
    /// 已撤回：判断被显式撤销，不再参与引用。已发生的外部副作用另行补偿，不自动回滚。
    Retracted,
}

/// 一个判断节点：某个问题在某个版本上、按某份输入投影做出的一次判断。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JudgmentNode {
    /// 运行时分配的标识。
    pub id: NodeId,
    /// 问题标识。
    pub question: QuestionId,
    /// 作答时所用的问题定义版本。同一 ID 换含义必须换版本，旧版本不会被覆盖
    /// （[`QuestionRegistry`](crate::QuestionRegistry)）。
    pub question_version: QuestionVersion,
    /// 这次判断读了哪些对象与字段。
    pub projection: InputProjection,
    /// 评估状态。
    pub evaluation: EvaluationStatus,
    /// 依据有效性。
    pub validity: Validity,
    /// 取得有效响应时的判断记录（来源、执行者版本、usage、延迟、快照与全部答案）。
    pub record: Option<JudgmentRecord>,
}

impl JudgmentNode {
    /// 本节点的答案：成功时给出该问题的判断（含概率原值与阈值）。
    pub fn judgment(&self) -> Option<&Judgment> {
        self.record.as_ref()?.judgments.get(&self.question)
    }

    /// 能否当作当前的行动依据：既取得过有效响应，依据有效性也是当前有效。
    pub fn is_usable(&self) -> bool {
        self.evaluation == EvaluationStatus::Succeeded && self.validity == Validity::Current
    }
}

/// 一个行动：以若干判断为前提出发。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Action {
    /// 行动标识。
    pub id: ActionId,
    /// 前提判断节点。
    pub premises: BTreeSet<NodeId>,
    /// 使用资格：与判断节点同一套 [`Validity`]——能不能发出，只看前提与自身是否当前有效。
    pub validity: Validity,
}

/// 行动前提复核的结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "check", content = "detail")]
pub enum PremiseCheck {
    /// 前提全部可依赖，行动可以发出。
    Cleared,
    /// 不能发出：列出不再可依赖的前提节点；列表为空表示行动自身已不是当前有效。
    Blocked {
        /// 失效的前提节点。
        premises: Vec<NodeId>,
    },
    /// 行动自身已被撤销。
    Withdrawn,
}

/// 失效传播的起因。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvalidationCause {
    /// 追加了一条新证据。
    EvidenceAppended(EvidenceId),
    /// 撤回了一条证据。
    EvidenceRetracted(EvidenceId),
    /// 撤销了一个判断。
    JudgmentRetracted(NodeId),
    /// 保守全量重查：不沿传播路径，把所有当前有效的判断与行动一并标为待重查。
    FullRecheck,
}

/// 一次失效传播的结果。
///
/// `paths` 只给**这次新标记**的每个目标一条最短路径，与 `stale`、`blocked` 一一对应
/// （先判断节点、后行动，都按标识排序），用来展示完整传播路径（研究目标 §5.4）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Invalidation {
    /// 起因。
    pub cause: InvalidationCause,
    /// 本次被标成待重查的判断节点（含传递闭包）。
    pub stale: Vec<NodeId>,
    /// 本次被标成不能发出的行动。
    pub blocked: Vec<ActionId>,
    /// 每个受影响目标的完整传播路径。
    pub paths: Vec<PropagationPath>,
}

impl Invalidation {
    /// 这次传播没有波及任何判断或行动（例如变化落在没人读过的对象上）。
    pub fn is_empty(&self) -> bool {
        self.stale.is_empty() && self.blocked.is_empty()
    }
}

/// 可失效的语义状态。
///
/// 证据日志只以只读方式暴露：追加与撤回都必须走 [`SemanticState::append_evidence`] /
/// [`SemanticState::retract_evidence`]，否则失效传播会被跳过，运行时就会拿着过时依据继续走。
pub struct SemanticState {
    evidence: Box<dyn EvidenceLog>,
    graph: DependencyGraph,
    nodes: BTreeMap<NodeId, JudgmentNode>,
    actions: BTreeMap<ActionId, Action>,
    next_node: u64,
}

impl SemanticState {
    /// 用一份证据日志新建运行时（[`MemoryEvidenceLog`] 或 [`JsonlEvidenceLog`](crate::JsonlEvidenceLog)）。
    pub fn new(evidence: Box<dyn EvidenceLog>) -> Self {
        Self {
            evidence,
            graph: DependencyGraph::default(),
            nodes: BTreeMap::new(),
            actions: BTreeMap::new(),
            next_node: 1,
        }
    }

    /// 用内存证据日志新建运行时。
    pub fn in_memory() -> Self {
        Self::new(Box::new(MemoryEvidenceLog::default()))
    }

    /// 证据日志（只读）。
    pub fn evidence(&self) -> &dyn EvidenceLog {
        self.evidence.as_ref()
    }

    /// 依赖图（只读）。
    pub fn graph(&self) -> &DependencyGraph {
        &self.graph
    }

    /// 依赖图（可写）：用于登记模型提出的额外依赖或语义关联。
    ///
    /// 读边请用 [`EdgeOrigin::Proposed`] 标明来源；确定性读取依赖由
    /// [`SemanticState::declare_node`] 自己登记。
    pub fn graph_mut(&mut self) -> &mut DependencyGraph {
        &mut self.graph
    }

    /// 取一个判断节点。
    pub fn node(&self, id: NodeId) -> Option<&JudgmentNode> {
        self.nodes.get(&id)
    }

    /// 全部判断节点（按标识排序）。
    pub fn nodes(&self) -> impl Iterator<Item = &JudgmentNode> {
        self.nodes.values()
    }

    /// 取一个行动。
    pub fn action(&self, id: &ActionId) -> Option<&Action> {
        self.actions.get(id)
    }

    /// 全部行动（按标识排序）。
    pub fn actions(&self) -> impl Iterator<Item = &Action> {
        self.actions.values()
    }

    /// 声明一个判断节点：绑定问题定义版本与输入投影，并登记确定性读取依赖。
    ///
    /// 节点一开始是「未请求 + 当前有效」；拿到结果后由 [`SemanticState::record_judgment`] 写入。
    pub fn declare_node(
        &mut self,
        question: QuestionId,
        question_version: QuestionVersion,
        projection: InputProjection,
    ) -> NodeId {
        let id = NodeId(self.next_node);
        self.next_node += 1;
        for (object, _) in projection.objects() {
            self.graph.add_edge(
                NodeRef::Object(object.clone()),
                NodeRef::Judgment(id),
                EdgeKind::Read,
                EdgeOrigin::Deterministic,
            );
        }
        self.nodes.insert(
            id,
            JudgmentNode {
                id,
                question,
                question_version,
                projection,
                evaluation: EvaluationStatus::NotRequested,
                validity: Validity::Current,
                record: None,
            },
        );
        id
    }

    /// 为一次请求里的全部问题声明判断节点，共用同一份输入投影。
    ///
    /// 共享一份请求 state 的多个问题必须保守依赖该请求的全部输入（研究目标 §5.3），所以这里只收一份
    /// 投影：调用方把 state 里读到的每个对象都写进投影，整批共用即可，不要按问题分别裁剪。
    pub fn declare_batch(
        &mut self,
        questions: &Questions,
        projection: &InputProjection,
    ) -> BTreeMap<QuestionId, NodeId> {
        questions
            .iter()
            .map(|(id, spec)| {
                (
                    id.clone(),
                    self.declare_node(id.clone(), spec.version, projection.clone()),
                )
            })
            .collect()
    }

    /// 设置评估状态。
    ///
    /// 排队、运行中、失败由调用方在循环里写；[`EvaluationStatus::Succeeded`] 正常路径上由
    /// [`SemanticState::record_judgment`] 写入——直接设它不会带上判断记录。
    pub fn set_evaluation(&mut self, id: NodeId, status: EvaluationStatus) -> Result<(), Error> {
        let node = self.nodes.get_mut(&id).ok_or(Error::UnknownNode(id))?;
        node.evaluation = status;
        Ok(())
    }

    /// 写入一次判断：把该问题在这次调用里的答案绑到节点上，并把评估状态置为成功。
    ///
    /// 已撤回的节点不接受写入（[`Error::RetractedNode`]）：撤销黏着到显式重新授权，要重新判断就
    /// 声明一个新节点——判断节点本来就不可改写。待重查的节点可以写，输入没动就回到当前有效，
    /// 这就是重查清账的姿势。
    ///
    /// 若投影读到的输入在请求期间变化了（对象出了新版本，或读到的版本被撤回），记录仍然留下作为
    /// 审计，但依据有效性直接落成待重查——它不能作为当前行动依据（研究目标 §5.3）。
    pub fn record_judgment(&mut self, id: NodeId, record: JudgmentRecord) -> Result<(), Error> {
        let Some(node) = self.nodes.get(&id) else {
            return Err(Error::UnknownNode(id));
        };
        if node.validity == Validity::Retracted {
            return Err(Error::RetractedNode(id));
        }
        let question = node.question.clone();
        let version = node.question_version;
        let projection = node.projection.clone();

        let judgment = record.judgments.get(&question).ok_or_else(|| {
            Error::InvalidResponse(format!("判断记录里没有问题 `{question}` 的答案"))
        })?;
        if judgment.question_version != version {
            return Err(Error::InvalidResponse(format!(
                "问题 `{question}` 的记录版本 {} 与节点声明的版本 {version} 不一致",
                judgment.question_version
            )));
        }
        let validity = match verify_snapshot(self.evidence.as_ref(), &projection) {
            SnapshotCheck::Current => Validity::Current,
            SnapshotCheck::Drifted { .. } => Validity::NeedsRecheck,
        };

        let node = self.nodes.get_mut(&id).ok_or(Error::UnknownNode(id))?;
        node.evaluation = EvaluationStatus::Succeeded;
        node.validity = validity;
        node.record = Some(record);
        Ok(())
    }

    /// 校验一份输入投影读到的输入是否仍是当前输入。
    ///
    /// 延迟返回的结果在被使用之前过一遍这里：漂移了就只能当审计材料（研究目标 §5.2）。
    pub fn verify(&self, projection: &InputProjection) -> SnapshotCheck {
        verify_snapshot(self.evidence.as_ref(), projection)
    }

    /// 追加一条新证据，并传播它引起的失效。
    ///
    /// **所有读过该对象的判断**都会变成待重查——它们没看到这条新记录。返回新记录与传播结果。
    pub fn append_evidence(
        &mut self,
        evidence: NewEvidence,
    ) -> Result<(Evidence, Invalidation), Error> {
        let entry = self.evidence.append(evidence)?;
        let affected = self.readers_of(&entry.object);
        let cause = InvalidationCause::EvidenceAppended(entry.id);
        let origin = NodeRef::Object(entry.object.clone());
        let invalidation = self.propagate(cause, origin, affected);
        Ok((entry, invalidation))
    }

    /// 撤回一条证据，并传播它引起的失效。
    ///
    /// **所有读过该对象的判断**都会变成待重查：被撤回的记录可能就在它们的材料里，本 crate 不猜测谁
    /// 究竟看了哪一版——宁可误失效，不留漏失效。撤回不改写任何记录，已撤回过的再撤回是幂等的。
    pub fn retract_evidence(&mut self, id: EvidenceId) -> Result<Invalidation, Error> {
        let object = self
            .evidence
            .evidence(id)
            .ok_or(Error::UnknownEvidence(id))?
            .object
            .clone();
        self.evidence.retract(id)?;

        let affected = self.readers_of(&object);
        Ok(self.propagate(
            InvalidationCause::EvidenceRetracted(id),
            NodeRef::Object(object),
            affected,
        ))
    }

    /// 撤销一个判断：它自己变成已撤回，下游判断与以它为前途的行动一起失效。
    ///
    /// 撤销只改变使用资格——已经发出去的动作不会因此回滚，需要补偿时由调用方另行发出补偿动作。
    pub fn retract_judgment(&mut self, id: NodeId) -> Result<Invalidation, Error> {
        let affected = self.graph.successors(&NodeRef::Judgment(id));
        let node = self.nodes.get_mut(&id).ok_or(Error::UnknownNode(id))?;
        node.validity = Validity::Retracted;
        Ok(self.propagate(
            InvalidationCause::JudgmentRetracted(id),
            NodeRef::Judgment(id),
            affected,
        ))
    }

    /// 保守全量重查出口：把所有当前有效的判断与行动一并标为待重查。
    ///
    /// 这是「宁可多查」的兜底，也是 H2 对照组（每次全量重算）的入口；它不沿传播路径走，
    /// 因此 [`Invalidation::paths`] 为空。
    pub fn recheck_all(&mut self) -> Invalidation {
        let mut stale = Vec::new();
        for node in self.nodes.values_mut() {
            if node.validity == Validity::Current {
                node.validity = Validity::NeedsRecheck;
                stale.push(node.id);
            }
        }
        let mut blocked = Vec::new();
        for action in self.actions.values_mut() {
            if action.validity == Validity::Current {
                action.validity = Validity::NeedsRecheck;
                blocked.push(action.id.clone());
            }
        }
        Invalidation {
            cause: InvalidationCause::FullRecheck,
            stale,
            blocked,
            paths: Vec::new(),
        }
    }

    /// 按需重算集合：当前待重查的判断节点（按标识排序）。
    ///
    /// 已撤回的节点不在里面——它们是被显式拿掉的，不该被重算；从没取得过结果的节点看
    /// [`JudgmentNode::evaluation`]。
    pub fn recheck_set(&self) -> Vec<NodeId> {
        self.nodes
            .values()
            .filter(|node| node.validity == Validity::NeedsRecheck)
            .map(|node| node.id)
            .collect()
    }

    /// 声明一个行动及其前提，并登记「判断 → 行动」的前提边。
    ///
    /// 重复声明同一个标识等于**换前提**：旧的前提边会摘掉，使用资格回到当前有效，避免旧前提失效时
    /// 误伤这个行动。
    pub fn declare_action(
        &mut self,
        id: ActionId,
        premises: impl IntoIterator<Item = NodeId>,
    ) -> Result<(), Error> {
        let premises: BTreeSet<NodeId> = premises.into_iter().collect();
        for premise in &premises {
            if !self.nodes.contains_key(premise) {
                return Err(Error::UnknownNode(*premise));
            }
        }

        if let Some(previous) = self.actions.remove(&id) {
            for premise in previous.premises {
                self.graph.remove_edge(
                    &NodeRef::Judgment(premise),
                    &NodeRef::Action(id.clone()),
                    EdgeKind::Premise,
                );
            }
        }
        for premise in &premises {
            self.graph.add_edge(
                NodeRef::Judgment(*premise),
                NodeRef::Action(id.clone()),
                EdgeKind::Premise,
                EdgeOrigin::Deterministic,
            );
        }
        self.actions.insert(
            id.clone(),
            Action {
                id,
                premises,
                validity: Validity::Current,
            },
        );
        Ok(())
    }

    /// 行动发出前的前提复核。
    ///
    /// 「可依赖」= 评估状态为成功（真的答过一次）且依据有效性当前有效。前提节点一旦被标成待重查或
    /// 已撤回，行动就发不出去；两者之间的窗口由调用方按需要缩小（本 crate 不做原子化执行）。
    pub fn check_action(&self, id: &ActionId) -> Result<PremiseCheck, Error> {
        let action = self
            .actions
            .get(id)
            .ok_or_else(|| Error::UnknownAction(id.clone()))?;
        if action.validity == Validity::Retracted {
            return Ok(PremiseCheck::Withdrawn);
        }
        let stale: Vec<NodeId> = action
            .premises
            .iter()
            .copied()
            .filter(|premise| self.node(*premise).is_none_or(|node| !node.is_usable()))
            .collect();
        if stale.is_empty() && action.validity == Validity::Current {
            Ok(PremiseCheck::Cleared)
        } else {
            Ok(PremiseCheck::Blocked { premises: stale })
        }
    }

    /// 撤销一个行动：只改使用资格，不碰任何已发生的外部副作用。
    pub fn withdraw_action(&mut self, id: &ActionId) -> Result<(), Error> {
        let action = self
            .actions
            .get_mut(id)
            .ok_or_else(|| Error::UnknownAction(id.clone()))?;
        action.validity = Validity::Retracted;
        Ok(())
    }

    /// 登记一条判断之间的关联边（支持关系或冲突候选）。
    ///
    /// 由规则或模型建议给出时，来源标 [`EdgeOrigin::Proposed`]，与确定性读取依赖分开记录
    /// （研究目标 §5.3）；读取关系请用 [`SemanticState::graph_mut`] 直接加边。
    pub fn link(
        &mut self,
        from: NodeId,
        to: NodeId,
        kind: EdgeKind,
        origin: EdgeOrigin,
    ) -> Result<(), Error> {
        for node in [from, to] {
            if !self.nodes.contains_key(&node) {
                return Err(Error::UnknownNode(node));
            }
        }
        self.graph
            .add_edge(NodeRef::Judgment(from), NodeRef::Judgment(to), kind, origin);
        Ok(())
    }

    /// 依赖环：相互引用的判断（研究目标 §5.3 要求必须被检测）。
    pub fn cycles(&self) -> Vec<Vec<NodeRef>> {
        self.graph.cycles()
    }

    /// 所有读过该对象的判断节点。
    ///
    /// 投影登记的确定性读取边与调用方手工加的读取边一视同仁——来源（[`EdgeOrigin`]）只进审计与
    /// 「确定性 / 模型提出」的区分，不改变依赖关系：公共 API 收下的边不能在传播里当空操作。
    fn readers_of(&self, object: &ObjectId) -> BTreeSet<NodeRef> {
        self.graph
            .edges_from(&NodeRef::Object(object.clone()))
            .into_iter()
            .filter(|edge| edge.kind == EdgeKind::Read)
            .map(|edge| edge.to)
            .collect()
    }

    /// 沿依赖图传播失效：把受影响的目标标成待重查/待复核，并收集到它们的路径。
    ///
    /// `affected_first` 决定起点的一跳邻居里哪些算受影响（证据类起因给的就是该对象的全部读者）；
    /// 更远的节点一律沿非冲突边传播。已经失效过的节点不再重复标记，也不会重复出现在结果里。
    fn propagate(
        &mut self,
        cause: InvalidationCause,
        origin: NodeRef,
        affected_first: BTreeSet<NodeRef>,
    ) -> Invalidation {
        let mut stale = Vec::new();
        let mut blocked = Vec::new();
        let mut paths = Vec::new();

        for path in self.graph.paths(&origin, Some(&affected_first)) {
            match path.target.clone() {
                NodeRef::Judgment(id) => {
                    if self.mark_node_stale(id) {
                        stale.push(id);
                        paths.push(path);
                    }
                }
                NodeRef::Action(id) => {
                    if self.mark_action_stale(&id) {
                        blocked.push(id);
                        paths.push(path);
                    }
                }
                NodeRef::Object(_) => {}
            }
        }

        Invalidation {
            cause,
            stale,
            blocked,
            paths,
        }
    }

    fn mark_node_stale(&mut self, id: NodeId) -> bool {
        match self.nodes.get_mut(&id) {
            Some(node) if node.validity == Validity::Current => {
                node.validity = Validity::NeedsRecheck;
                true
            }
            _ => false,
        }
    }

    fn mark_action_stale(&mut self, id: &ActionId) -> bool {
        match self.actions.get_mut(id) {
            Some(action) if action.validity == Validity::Current => {
                action.validity = Validity::NeedsRecheck;
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::answer::{Answer, NoulAnswer};
    use crate::question::{NoulQuestion, Question, QuestionSpec};
    use crate::record::JudgeSource;
    use crate::state::SnapshotId;

    fn observe(state: &mut SemanticState, object: &str, version: u64) -> Evidence {
        state
            .append_evidence(NewEvidence {
                source: "tool:probe".to_owned(),
                at_unix_ms: 1_700_000_000_000 + version,
                object: ObjectId::from(object),
                version: crate::evidence::ObjectVersion(version),
                content: json!({ "version": version }),
            })
            .expect("内存日志不会失败")
            .0
    }

    fn declare(state: &mut SemanticState, question: &str, reads: &[(&str, u64)]) -> NodeId {
        let projection = InputProjection::covering(reads.iter().map(|(object, version)| {
            (
                ObjectId::from(*object),
                crate::evidence::ObjectVersion(*version),
            )
        }));
        state.declare_node(QuestionId::from(question), QuestionVersion(1), projection)
    }

    fn record_for(state: &SemanticState, node: NodeId, noul: f64) -> JudgmentRecord {
        let node = state.node(node).expect("节点已声明");
        let question = node.question.clone();
        let question_version = node.question_version;
        JudgmentRecord {
            snapshot: SnapshotId::from("case-001"),
            source: JudgeSource::Rule,
            version: "rule-v1".to_owned(),
            request_id: None,
            usage: None,
            latency_ms: 1,
            judgments: BTreeMap::from([(
                question,
                Judgment {
                    question_version,
                    answer: Answer::Noul(NoulAnswer { noul }),
                    thresholds: BTreeMap::new(),
                },
            )]),
        }
    }

    fn judge(state: &mut SemanticState, node: NodeId, noul: f64) {
        let record = record_for(state, node, noul);
        state.record_judgment(node, record).expect("记录与节点一致");
    }

    #[test]
    fn retracting_evidence_invalidates_readers_dependents_and_actions() {
        let mut state = SemanticState::in_memory();
        let config = observe(&mut state, "cache.conf", 1);
        observe(&mut state, "app.log", 1);

        // A、B 各读一份材料；C 也读配置且由 A 支持；D 没读配置、只被 A 支持；三个行动各挂一个前提。
        let a = declare(&mut state, "cache_disabled", &[("cache.conf", 1)]);
        let b = declare(&mut state, "cache_hit_after_cancel", &[("app.log", 1)]);
        let c = declare(&mut state, "restart_safe", &[("cache.conf", 1)]);
        let d = declare(&mut state, "rollout_ready", &[]);
        state
            .link(a, c, EdgeKind::Support, EdgeOrigin::Proposed)
            .unwrap();
        state
            .link(a, d, EdgeKind::Support, EdgeOrigin::Proposed)
            .unwrap();
        let from_c = ActionId::from("restart-cache");
        let from_b = ActionId::from("page-on-call");
        let from_d = ActionId::from("warm-cache");
        state.declare_action(from_c.clone(), [c]).unwrap();
        state.declare_action(from_b.clone(), [b]).unwrap();
        state.declare_action(from_d.clone(), [d]).unwrap();
        for node in [a, b, c, d] {
            judge(&mut state, node, 1.0);
        }
        assert_eq!(state.check_action(&from_c).unwrap(), PremiseCheck::Cleared);

        let invalidation = state.retract_evidence(config.id).unwrap();

        assert_eq!(
            invalidation.cause,
            InvalidationCause::EvidenceRetracted(config.id)
        );
        // 读了配置的 A、C 直接失效；没读配置、由 A 支持的 D 沿支持关系失效。
        assert_eq!(invalidation.stale, vec![a, c, d]);
        assert_eq!(invalidation.blocked, vec![from_c.clone(), from_d.clone()]);
        assert_eq!(state.recheck_set(), vec![a, c, d]);

        // 完整传播路径：撤回的证据所在对象 → 读它的判断 →（支持）→ 行动。
        let hops_to = |action: &ActionId| {
            invalidation
                .paths
                .iter()
                .find(|path| path.target == NodeRef::Action(action.clone()))
                .expect("行动也在传播结果里")
                .hops
                .iter()
                .map(|hop| hop.kind)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            hops_to(&from_d),
            vec![EdgeKind::Read, EdgeKind::Support, EdgeKind::Premise]
        );
        // C 自己也读了配置，到它的行动路径更短。
        assert_eq!(hops_to(&from_c), vec![EdgeKind::Read, EdgeKind::Premise]);
        assert_eq!(
            invalidation
                .paths
                .iter()
                .find(|path| path.target == NodeRef::Judgment(a))
                .expect("读它的判断也在传播结果里")
                .hops
                .len(),
            1
        );

        // 无关结果保留：B 与它的行动都没被波及。
        assert_eq!(state.node(b).unwrap().validity, Validity::Current);
        assert_eq!(state.check_action(&from_b).unwrap(), PremiseCheck::Cleared);

        // 标记过的节点不会被重复标记，重放同一次撤回不产生新结果。
        assert!(state.retract_evidence(config.id).unwrap().is_empty());
    }

    #[test]
    fn appending_newer_evidence_invalidates_the_readers_it_left_behind() {
        let mut state = SemanticState::in_memory();
        observe(&mut state, "cache.conf", 1);
        observe(&mut state, "app.log", 1);
        let older = declare(&mut state, "cache_disabled", &[("cache.conf", 1)]);
        let unrelated = declare(&mut state, "cancel_observed", &[("app.log", 1)]);
        judge(&mut state, older, 1.0);
        judge(&mut state, unrelated, 1.0);

        // 对象出新版本：读到旧版本的判断要重查，读别的对象的不受影响。
        let (entry, invalidation) = state
            .append_evidence(NewEvidence {
                source: "tool:read_config".to_owned(),
                at_unix_ms: 1_700_000_001_000,
                object: ObjectId::from("cache.conf"),
                version: crate::evidence::ObjectVersion(2),
                content: json!({ "version": 2 }),
            })
            .unwrap();

        assert_eq!(
            invalidation.cause,
            InvalidationCause::EvidenceAppended(entry.id)
        );
        assert_eq!(invalidation.stale, vec![older]);
        assert_eq!(state.node(unrelated).unwrap().validity, Validity::Current);
        assert_eq!(
            state
                .evidence()
                .current_version(&ObjectId::from("cache.conf")),
            Some(crate::evidence::ObjectVersion(2))
        );
    }

    #[test]
    fn a_result_recorded_while_its_input_moved_on_is_kept_for_audit_only() {
        let mut state = SemanticState::in_memory();
        observe(&mut state, "cache.conf", 1);
        observe(&mut state, "cache.conf", 2);

        // 投影读到的是旧版本：登记成功，但依据有效性直接落成待重查。
        let node = declare(&mut state, "cache_disabled", &[("cache.conf", 1)]);
        judge(&mut state, node, 1.0);

        let recorded = state.node(node).unwrap();
        assert_eq!(recorded.evaluation, EvaluationStatus::Succeeded);
        assert_eq!(recorded.validity, Validity::NeedsRecheck);
        assert!(recorded.record.is_some(), "记录保留作审计");
        assert!(
            matches!(
                state.verify(&recorded.projection),
                SnapshotCheck::Drifted { .. }
            ),
            "延迟返回的结果过一遍快照校验就能看出漂移"
        );
    }

    #[test]
    fn withdrawing_a_judgment_does_not_roll_anything_back() {
        let mut state = SemanticState::in_memory();
        observe(&mut state, "cache.conf", 1);
        let node = declare(&mut state, "cache_disabled", &[("cache.conf", 1)]);
        let action = ActionId::from("restart-cache");
        state.declare_action(action.clone(), [node]).unwrap();
        judge(&mut state, node, 1.0);

        let invalidation = state.retract_judgment(node).unwrap();

        assert_eq!(
            invalidation.cause,
            InvalidationCause::JudgmentRetracted(node)
        );
        assert!(
            invalidation.stale.is_empty(),
            "自己变成已撤回，不在待重查里"
        );
        assert_eq!(invalidation.blocked, vec![action.clone()]);
        assert_eq!(state.node(node).unwrap().validity, Validity::Retracted);
        assert_eq!(state.recheck_set(), vec![], "已撤回的判断不重算");
        assert_eq!(
            state.check_action(&action).unwrap(),
            PremiseCheck::Blocked {
                premises: vec![node]
            }
        );

        state.withdraw_action(&action).unwrap();
        assert_eq!(
            state.check_action(&action).unwrap(),
            PremiseCheck::Withdrawn
        );

        // 撤销只改资格：记录与动作痕迹都还在（外部副作用要另行补偿）。
        assert!(state.node(node).unwrap().record.is_some());
    }

    #[test]
    fn premise_check_needs_a_successful_answer_not_just_a_declaration() {
        let mut state = SemanticState::in_memory();
        observe(&mut state, "cache.conf", 1);
        let node = declare(&mut state, "cache_disabled", &[("cache.conf", 1)]);
        let action = ActionId::from("restart-cache");
        state.declare_action(action.clone(), [node]).unwrap();

        // 还没答过：前提不可依赖。
        assert_eq!(
            state.check_action(&action).unwrap(),
            PremiseCheck::Blocked {
                premises: vec![node]
            }
        );

        state
            .set_evaluation(node, EvaluationStatus::Failed("429".to_owned()))
            .unwrap();
        assert_eq!(
            state.check_action(&action).unwrap(),
            PremiseCheck::Blocked {
                premises: vec![node]
            }
        );

        judge(&mut state, node, 0.42);
        assert_eq!(state.check_action(&action).unwrap(), PremiseCheck::Cleared);

        assert!(matches!(
            state.check_action(&ActionId::from("never-declared")),
            Err(Error::UnknownAction(_))
        ));
    }

    #[test]
    fn the_full_recheck_exit_marks_everything_and_needs_no_paths() {
        let mut state = SemanticState::in_memory();
        observe(&mut state, "cache.conf", 1);
        let one = declare(&mut state, "cache_disabled", &[("cache.conf", 1)]);
        let two = declare(&mut state, "cache_enabled", &[("cache.conf", 1)]);
        let action = ActionId::from("restart-cache");
        state.declare_action(action.clone(), [two]).unwrap();
        for node in [one, two] {
            judge(&mut state, node, 1.0);
        }

        let invalidation = state.recheck_all();

        assert_eq!(invalidation.cause, InvalidationCause::FullRecheck);
        assert_eq!(invalidation.stale, vec![one, two]);
        assert_eq!(invalidation.blocked, vec![action.clone()]);
        assert!(invalidation.paths.is_empty());
        assert_eq!(state.recheck_set(), vec![one, two]);
        assert_eq!(
            state.check_action(&action).unwrap(),
            PremiseCheck::Blocked {
                premises: vec![two]
            }
        );

        // 再跑一次：已经没有当前有效的判断了，传播结果为空。
        assert!(state.recheck_all().is_empty());
    }

    #[test]
    fn model_proposed_dependencies_are_recorded_separately_and_count_for_invalidation() {
        let mut state = SemanticState::in_memory();
        let config = observe(&mut state, "cache.conf", 1);
        observe(&mut state, "app.log", 1);
        let reader = declare(&mut state, "cache_disabled", &[("cache.conf", 1)]);
        let proposed = declare(&mut state, "restart_safe", &[("app.log", 1)]);
        judge(&mut state, reader, 1.0);
        judge(&mut state, proposed, 1.0);

        // 模型说「这个判断其实还看了配置」：记成 Proposed，与投影给出的确定性边分开。
        state.graph_mut().add_edge(
            NodeRef::Object(ObjectId::from("cache.conf")),
            NodeRef::Judgment(proposed),
            EdgeKind::Read,
            EdgeOrigin::Proposed,
        );

        let reads = state
            .graph()
            .edges_from(&NodeRef::Object(ObjectId::from("cache.conf")));
        assert_eq!(
            reads
                .iter()
                .filter(|edge| edge.origin == EdgeOrigin::Deterministic)
                .count(),
            1
        );
        assert_eq!(
            reads
                .iter()
                .filter(|edge| edge.origin == EdgeOrigin::Proposed)
                .count(),
            1
        );

        // 撤回配置：确定性与模型提出的读取依赖都被波及。
        let invalidation = state.retract_evidence(config.id).unwrap();
        assert_eq!(invalidation.stale, vec![reader, proposed]);
    }

    #[test]
    fn a_batch_shares_one_projection_and_redeclaring_an_action_swaps_its_premises() {
        let mut state = SemanticState::in_memory();
        let config = observe(&mut state, "cache.conf", 1);
        observe(&mut state, "app.log", 1);

        // 一次请求两个问题、共享一份 state：整批共用同一份保守投影。
        let projection = InputProjection::covering([
            (
                ObjectId::from("cache.conf"),
                crate::evidence::ObjectVersion(1),
            ),
            (ObjectId::from("app.log"), crate::evidence::ObjectVersion(1)),
        ]);
        let questions: Questions = BTreeMap::from([
            (
                QuestionId::from("cache_disabled"),
                QuestionSpec {
                    version: QuestionVersion(1),
                    question: Question::Noul(NoulQuestion {
                        instructions: "？".to_owned(),
                        criteria: None,
                    }),
                },
            ),
            (
                QuestionId::from("cancel_observed"),
                QuestionSpec {
                    version: QuestionVersion(2),
                    question: Question::Noul(NoulQuestion {
                        instructions: "？".to_owned(),
                        criteria: None,
                    }),
                },
            ),
        ]);
        let nodes = state.declare_batch(&questions, &projection);
        assert_eq!(nodes.len(), 2);
        assert_eq!(
            state
                .node(nodes[&QuestionId::from("cancel_observed")])
                .unwrap()
                .question_version,
            QuestionVersion(2)
        );

        // 整批都读了配置：撤回配置的旧版本会波及整批。
        let invalidation = state.retract_evidence(config.id).unwrap();
        assert_eq!(invalidation.stale.len(), 2);

        let action = ActionId::from("restart-cache");
        state
            .declare_action(action.clone(), [nodes[&QuestionId::from("cache_disabled")]])
            .unwrap();
        assert_eq!(
            state.check_action(&action).unwrap(),
            PremiseCheck::Blocked {
                premises: vec![nodes[&QuestionId::from("cache_disabled")]]
            }
        );

        // 配置换了新版本：用新证据重新判断，再把行动的前提换到新节点上。
        let fresh = observe(&mut state, "cache.conf", 2);
        let replacement = declare(&mut state, "cache_disabled", &[("cache.conf", 2)]);
        judge(&mut state, replacement, 1.0);
        state.declare_action(action.clone(), [replacement]).unwrap();

        assert_eq!(
            state.action(&action).unwrap().premises,
            BTreeSet::from([replacement])
        );
        assert_eq!(state.check_action(&action).unwrap(), PremiseCheck::Cleared);
        assert_eq!(
            state
                .evidence()
                .current_version(&ObjectId::from("cache.conf")),
            Some(crate::evidence::ObjectVersion(2))
        );
        assert!(!state.evidence().is_retracted(fresh.id));
    }

    #[test]
    fn mixed_versions_in_the_request_state_cannot_hide_an_invalidation() {
        // 装配 state 时把同一对象的两个版本都装了、投影只写读到的最新版：撤回那条「投影没写」的旧
        // 记录也不能静默漏失效——传播按对象走。
        let mut state = SemanticState::in_memory();
        let older = observe(&mut state, "cache.conf", 1);
        observe(&mut state, "cache.conf", 2);
        let node = declare(&mut state, "cache_disabled", &[("cache.conf", 2)]);
        let action = ActionId::from("restart-cache");
        state.declare_action(action.clone(), [node]).unwrap();
        judge(&mut state, node, 1.0);
        assert_eq!(state.check_action(&action).unwrap(), PremiseCheck::Cleared);

        let invalidation = state.retract_evidence(older.id).unwrap();

        assert_eq!(invalidation.stale, vec![node]);
        assert_eq!(state.node(node).unwrap().validity, Validity::NeedsRecheck);
        assert_eq!(
            state.check_action(&action).unwrap(),
            PremiseCheck::Blocked {
                premises: vec![node]
            }
        );
    }

    #[test]
    fn hand_added_read_edges_count_for_propagation_whatever_their_origin() {
        // 公共 API 收下的读取边不能在传播里当空操作；来源只区分「确定性 / 模型提出」，不改变依赖关系。
        for origin in [EdgeOrigin::Deterministic, EdgeOrigin::Proposed] {
            let mut state = SemanticState::in_memory();
            let config = observe(&mut state, "cache.conf", 1);
            observe(&mut state, "app.log", 1);
            let node = declare(&mut state, "restart_safe", &[("app.log", 1)]);
            judge(&mut state, node, 1.0);
            state.graph_mut().add_edge(
                NodeRef::Object(ObjectId::from("cache.conf")),
                NodeRef::Judgment(node),
                EdgeKind::Read,
                origin,
            );

            let invalidation = state.retract_evidence(config.id).unwrap();

            assert_eq!(invalidation.stale, vec![node], "{origin:?}");
        }
    }

    #[test]
    fn appending_a_record_invalidates_every_reader_of_that_object() {
        // 追加同样是对象级：迟到的一条旧版本记录也是「读过它的判断没看到」的新材料。
        let mut state = SemanticState::in_memory();
        observe(&mut state, "cache.conf", 2);
        observe(&mut state, "app.log", 1);
        let reader = declare(&mut state, "cache_disabled", &[("cache.conf", 2)]);
        let unrelated = declare(&mut state, "cancel_observed", &[("app.log", 1)]);
        judge(&mut state, reader, 1.0);
        judge(&mut state, unrelated, 1.0);

        let (_, invalidation) = state
            .append_evidence(NewEvidence {
                source: "tool:read_config".to_owned(),
                at_unix_ms: 1_700_000_000_001,
                object: ObjectId::from("cache.conf"),
                version: crate::evidence::ObjectVersion(1),
                content: json!({ "cache": "off" }),
            })
            .unwrap();

        assert_eq!(invalidation.stale, vec![reader]);
        assert_eq!(state.node(unrelated).unwrap().validity, Validity::Current);
    }

    #[test]
    fn a_withdrawn_judgment_stays_withdrawn_until_a_new_node_reauthorizes_it() {
        let mut state = SemanticState::in_memory();
        observe(&mut state, "cache.conf", 1);
        let node = declare(&mut state, "cache_disabled", &[("cache.conf", 1)]);
        let action = ActionId::from("restart-cache");
        state.declare_action(action.clone(), [node]).unwrap();
        judge(&mut state, node, 1.0);
        state.retract_judgment(node).unwrap();

        // 迟到的结果写不进已撤回的节点：撤销黏着到显式重新授权。
        let late = record_for(&state, node, 0.0);
        let error = state
            .record_judgment(node, late)
            .expect_err("已撤回的节点不接受写入");
        assert!(matches!(error, Error::RetractedNode(_)), "{error}");
        assert_eq!(state.node(node).unwrap().validity, Validity::Retracted);
        assert!(!state.node(node).unwrap().is_usable());
        assert_eq!(
            state.check_action(&action).unwrap(),
            PremiseCheck::Blocked {
                premises: vec![node]
            }
        );
        assert!(!state.recheck_set().contains(&node), "已撤回的判断不重算");

        // 显式重新授权：声明新节点、写新结果、把行动前提换过去；旧节点保持已撤回。
        let fresh = declare(&mut state, "cache_disabled", &[("cache.conf", 1)]);
        judge(&mut state, fresh, 0.0);
        state.declare_action(action.clone(), [fresh]).unwrap();
        assert_eq!(state.check_action(&action).unwrap(), PremiseCheck::Cleared);
        assert_eq!(state.node(node).unwrap().validity, Validity::Retracted);
    }
}
