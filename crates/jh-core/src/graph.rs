//! 依赖图：四类边、邻接表与依赖环检测。
//!
//! 研究目标 §5.2 要求依赖图区分**读取关系、支持关系、冲突候选与行动前提**；§5.3 要求
//! **确定性读取依赖与模型提出的语义关联分开记录**，并**检测依赖环**（不让相互引用的判断在没有
//! 新证据时互相抬高可信度）。
//!
//! # 方向约定
//!
//! 边的方向就是失效传播的方向：**起点失效会波及终点**。于是
//!
//! - `Read`：证据对象 → 读它的判断节点（对象变了，读它的判断要重查）；
//! - `Support`：支持者判断 → 被支持者判断；
//! - `Premise`：判断节点 → 以它为前提的行动；
//! - `Conflict`：判断 ↔ 判断，**对称**：登记一条等于登记一对（两个方向都进邻接表）。
//!
//! 冲突候选既不是支持也不是依赖，所以它**不参与失效传播，也不参与环检测**——否则任意一对冲突候选
//! 都会被当成二元环。读取关系与行动前提照检不误。
//!
//! # 环检测的范围
//!
//! [`DependencyGraph::cycles`] 报告被检测到的环（按节点顺序）。有环图必然被报出至少一个环
//! （DFS 回边），但它不枚举全部环——够用来发现「相互引用的判断」，不做穷举。

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::evidence::ObjectId;

/// 判断节点在运行时里的标识：由 [`SemanticState`](crate::SemanticState) 按声明顺序分配，从 1 开始。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeId(pub u64);

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "node-{}", self.0)
    }
}

/// 行动的标识：由调用方给出（如 `restart-cache`），因为它要在报告、工具调用与补偿动作之间对齐。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ActionId(pub String);

impl ActionId {
    /// 借出标识文本。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for ActionId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl From<String> for ActionId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl std::fmt::Display for ActionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// 依赖图的节点。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "node", content = "id")]
pub enum NodeRef {
    /// 证据对象：读取关系的起点。
    Object(ObjectId),
    /// 判断节点。
    Judgment(NodeId),
    /// 行动。
    Action(ActionId),
}

/// 边的类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    /// 读取关系：判断读了某个对象。
    Read,
    /// 支持关系：起点判断为终点判断提供支持。
    Support,
    /// 冲突候选：两个判断可能互相矛盾（对称关系）。
    Conflict,
    /// 行动前提：起点判断是终点行动成立的前提。
    Premise,
}

/// 边的来源：确定性读取依赖与模型提出的语义关联分开记录（研究目标 §5.3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeOrigin {
    /// 确定性：由输入投影或程序规则给出。
    Deterministic,
    /// 模型提出：模型建议的额外依赖或语义关联，只作候选，可靠性另计。
    Proposed,
}

/// 一条边：起点 → 终点，带类型与来源。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Edge {
    /// 起点。
    pub from: NodeRef,
    /// 终点。
    pub to: NodeRef,
    /// 边类型。
    pub kind: EdgeKind,
    /// 边来源。
    pub origin: EdgeOrigin,
}

impl Edge {
    fn hop(&self) -> PropagationHop {
        PropagationHop {
            from: self.from.clone(),
            kind: self.kind,
            to: self.to.clone(),
        }
    }
}

/// 传播路径上的一跳。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PropagationHop {
    /// 这一跳的起点。
    pub from: NodeRef,
    /// 这一跳的边类型。
    pub kind: EdgeKind,
    /// 这一跳的终点。
    pub to: NodeRef,
}

/// 一条完整的传播路径：从起因出发到某个受影响目标。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PropagationPath {
    /// 路径终点。
    pub target: NodeRef,
    /// 从头到尾的每一跳。
    pub hops: Vec<PropagationHop>,
}

/// 依赖图：邻接表（起点 → 终点 → 该对节点上的边类型与来源）。
#[derive(Debug, Clone, Default)]
pub struct DependencyGraph {
    adjacency: BTreeMap<NodeRef, BTreeMap<NodeRef, BTreeSet<(EdgeKind, EdgeOrigin)>>>,
}

impl DependencyGraph {
    /// 登记一条边。冲突候选是对称关系：登记一条等于登记一对。
    pub fn add_edge(&mut self, from: NodeRef, to: NodeRef, kind: EdgeKind, origin: EdgeOrigin) {
        self.insert(from.clone(), to.clone(), kind, origin);
        if kind == EdgeKind::Conflict {
            self.insert(to, from, kind, origin);
        }
    }

    /// 移除一条边。冲突候选同样按对称处理。
    pub fn remove_edge(&mut self, from: &NodeRef, to: &NodeRef, kind: EdgeKind) {
        self.remove_one(from, to, kind);
        if kind == EdgeKind::Conflict {
            self.remove_one(to, from, kind);
        }
    }

    fn insert(&mut self, from: NodeRef, to: NodeRef, kind: EdgeKind, origin: EdgeOrigin) {
        self.adjacency
            .entry(from)
            .or_default()
            .entry(to)
            .or_default()
            .insert((kind, origin));
    }

    fn remove_one(&mut self, from: &NodeRef, to: &NodeRef, kind: EdgeKind) {
        let Some(targets) = self.adjacency.get_mut(from) else {
            return;
        };
        let Some(relations) = targets.get_mut(to) else {
            return;
        };
        relations.retain(|(existing, _)| *existing != kind);
        if relations.is_empty() {
            targets.remove(to);
        }
        if targets.is_empty() {
            self.adjacency.remove(from);
        }
    }

    /// 图里出现过的全部节点。
    pub fn nodes(&self) -> BTreeSet<NodeRef> {
        let mut nodes = BTreeSet::new();
        for (from, targets) in &self.adjacency {
            nodes.insert(from.clone());
            nodes.extend(targets.keys().cloned());
        }
        nodes
    }

    /// 全部边（按起点与终点排序）。
    pub fn edges(&self) -> Vec<Edge> {
        self.adjacency
            .keys()
            .flat_map(|from| self.edges_from(from))
            .collect()
    }

    /// 某个节点的全部出边（含冲突候选的两个方向）。
    pub fn edges_from(&self, from: &NodeRef) -> Vec<Edge> {
        self.adjacency
            .get(from)
            .map(|targets| {
                targets
                    .iter()
                    .flat_map(|(to, relations)| {
                        relations.iter().map(|(kind, origin)| Edge {
                            from: from.clone(),
                            to: to.clone(),
                            kind: *kind,
                            origin: *origin,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 某个节点的后继：沿边方向、不含冲突候选。
    pub fn successors(&self, from: &NodeRef) -> BTreeSet<NodeRef> {
        self.edges_from(from)
            .into_iter()
            .filter(|edge| edge.kind != EdgeKind::Conflict)
            .map(|edge| edge.to)
            .collect()
    }

    /// 某个节点上的冲突候选（对称关系的另一半）。
    pub fn conflicts_with(&self, node: &NodeRef) -> BTreeSet<NodeRef> {
        self.edges_from(node)
            .into_iter()
            .filter(|edge| edge.kind == EdgeKind::Conflict)
            .map(|edge| edge.to)
            .collect()
    }

    /// 从起点出发、按边方向传播的路径：每个可达目标一条最短路径。
    ///
    /// `first` 限制**只有哪些一跳邻居算受影响**（传 `None` 表示不过滤）；更远的节点一律沿非冲突边
    /// 继续。失效传播靠它表达「读了被撤回那一版的对象才算受影响」这类精细筛选。
    pub fn paths(
        &self,
        origin: &NodeRef,
        first: Option<&BTreeSet<NodeRef>>,
    ) -> Vec<PropagationPath> {
        let mut paths: BTreeMap<NodeRef, PropagationPath> = BTreeMap::new();
        let mut seen = BTreeSet::from([origin.clone()]);
        let mut queue: VecDeque<PropagationPath> = VecDeque::new();

        for edge in self.edges_from(origin) {
            if edge.kind == EdgeKind::Conflict {
                continue;
            }
            if !first.is_none_or(|first| first.contains(&edge.to)) {
                continue;
            }
            queue.push_back(PropagationPath {
                target: edge.to.clone(),
                hops: vec![edge.hop()],
            });
        }

        while let Some(path) = queue.pop_front() {
            if !seen.insert(path.target.clone()) {
                continue;
            }
            for edge in self.edges_from(&path.target) {
                if edge.kind == EdgeKind::Conflict || seen.contains(&edge.to) {
                    continue;
                }
                let mut next = path.clone();
                next.target = edge.to.clone();
                next.hops.push(edge.hop());
                queue.push_back(next);
            }
            paths.insert(path.target.clone(), path);
        }

        paths.into_values().collect()
    }

    /// 从起点出发的全部传播路径（不做一跳筛选）。
    pub fn paths_from(&self, origin: &NodeRef) -> Vec<PropagationPath> {
        self.paths(origin, None)
    }

    /// 依赖环：每个被检测到的环按节点顺序列出（末尾节点回到首个节点）。
    pub fn cycles(&self) -> Vec<Vec<NodeRef>> {
        let mut cycles = Vec::new();
        let mut visited = BTreeSet::new();
        for start in self.nodes() {
            let mut stack = Vec::new();
            self.walk(&start, &mut visited, &mut stack, &mut cycles);
        }
        cycles.sort();
        cycles.dedup();
        cycles
    }

    /// 三色 DFS：遇到指回当前路径的回边就从中截出一个环。
    fn walk(
        &self,
        node: &NodeRef,
        visited: &mut BTreeSet<NodeRef>,
        stack: &mut Vec<NodeRef>,
        cycles: &mut Vec<Vec<NodeRef>>,
    ) {
        if let Some(position) = stack.iter().position(|item| item == node) {
            cycles.push(stack[position..].to_vec());
            return;
        }
        if !visited.insert(node.clone()) {
            return;
        }
        stack.push(node.clone());
        for next in self.successors(node) {
            self.walk(&next, visited, stack, cycles);
        }
        stack.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn judgment(id: u64) -> NodeRef {
        NodeRef::Judgment(NodeId(id))
    }

    #[test]
    fn the_four_edge_kinds_are_recorded_separately_with_their_origin() {
        let mut graph = DependencyGraph::default();
        let object = NodeRef::Object(ObjectId::from("cache.conf"));
        let (one, two, three) = (judgment(1), judgment(2), judgment(3));

        graph.add_edge(
            object.clone(),
            one.clone(),
            EdgeKind::Read,
            EdgeOrigin::Deterministic,
        );
        graph.add_edge(
            one.clone(),
            two.clone(),
            EdgeKind::Support,
            EdgeOrigin::Proposed,
        );
        graph.add_edge(
            two.clone(),
            three.clone(),
            EdgeKind::Conflict,
            EdgeOrigin::Proposed,
        );
        graph.add_edge(
            two.clone(),
            NodeRef::Action(ActionId::from("restart-cache")),
            EdgeKind::Premise,
            EdgeOrigin::Deterministic,
        );

        // 确定性读取依赖与模型提出的语义关联分开可查。
        let reads: Vec<Edge> = graph
            .edges_from(&object)
            .into_iter()
            .filter(|edge| edge.origin == EdgeOrigin::Deterministic)
            .collect();
        assert_eq!(reads.len(), 1);
        assert_eq!(reads[0].to, one);

        // 冲突候选是对称的：登记一条，两个方向都能查到。
        assert_eq!(graph.conflicts_with(&two), BTreeSet::from([three.clone()]));
        assert_eq!(graph.conflicts_with(&three), BTreeSet::from([two.clone()]));

        // 后继不含冲突候选：冲突不是依赖。
        assert!(
            graph
                .successors(&two)
                .contains(&NodeRef::Action(ActionId::from("restart-cache")))
        );

        // 移除一条边之后，邻接表里不留空壳。
        graph.remove_edge(&object, &one, EdgeKind::Read);
        assert!(graph.edges_from(&object).is_empty());
        graph.remove_edge(&two, &three, EdgeKind::Conflict);
        assert!(graph.conflicts_with(&two).is_empty() && graph.conflicts_with(&three).is_empty());
    }

    #[test]
    fn paths_follow_the_edges_and_can_be_filtered_at_the_first_hop() {
        let mut graph = DependencyGraph::default();
        let object = NodeRef::Object(ObjectId::from("cache.conf"));
        let (one, two) = (judgment(1), judgment(2));
        let action = NodeRef::Action(ActionId::from("restart-cache"));

        graph.add_edge(
            object.clone(),
            one.clone(),
            EdgeKind::Read,
            EdgeOrigin::Deterministic,
        );
        graph.add_edge(
            object.clone(),
            two.clone(),
            EdgeKind::Read,
            EdgeOrigin::Deterministic,
        );
        graph.add_edge(
            one.clone(),
            action.clone(),
            EdgeKind::Premise,
            EdgeOrigin::Deterministic,
        );

        let all = graph.paths_from(&object);
        assert_eq!(
            all.iter()
                .map(|path| path.target.clone())
                .collect::<Vec<_>>(),
            vec![one.clone(), two.clone(), action.clone()]
        );
        let action_path = all.iter().find(|path| path.target == action).unwrap();
        assert_eq!(
            action_path
                .hops
                .iter()
                .map(|hop| hop.kind)
                .collect::<Vec<_>>(),
            vec![EdgeKind::Read, EdgeKind::Premise]
        );

        // 一跳只收 2 号判断：1 号判断与它下游的行动都不在结果里。
        let filtered = graph.paths(&object, Some(&BTreeSet::from([two.clone()])));
        assert_eq!(
            filtered
                .iter()
                .map(|path| path.target.clone())
                .collect::<Vec<_>>(),
            vec![two]
        );
    }

    #[test]
    fn cycles_are_detected_and_conflicts_are_not_mistaken_for_cycles() {
        let mut graph = DependencyGraph::default();
        let (one, two, three, four) = (judgment(1), judgment(2), judgment(3), judgment(4));

        assert!(graph.cycles().is_empty());

        // 1 → 2 → 3 → 1：相互引用的判断。
        graph.add_edge(
            one.clone(),
            two.clone(),
            EdgeKind::Support,
            EdgeOrigin::Proposed,
        );
        graph.add_edge(
            two.clone(),
            three.clone(),
            EdgeKind::Support,
            EdgeOrigin::Proposed,
        );
        graph.add_edge(
            three.clone(),
            one.clone(),
            EdgeKind::Support,
            EdgeOrigin::Proposed,
        );
        // 4 只在环外，不参与。
        graph.add_edge(
            two.clone(),
            four.clone(),
            EdgeKind::Support,
            EdgeOrigin::Proposed,
        );

        let cycles = graph.cycles();
        assert_eq!(cycles.len(), 1, "{cycles:?}");
        assert_eq!(cycles[0].len(), 3, "{cycles:?}");
        assert!(cycles[0].contains(&one) && cycles[0].contains(&two) && cycles[0].contains(&three));

        // 一对冲突候选永远是对称的两条边，但不能被当成二元环。
        let mut conflicting = DependencyGraph::default();
        conflicting.add_edge(
            one.clone(),
            four.clone(),
            EdgeKind::Conflict,
            EdgeOrigin::Proposed,
        );
        assert!(conflicting.cycles().is_empty());

        // 自环也算环。
        let mut looping = DependencyGraph::default();
        looping.add_edge(
            one.clone(),
            one.clone(),
            EdgeKind::Support,
            EdgeOrigin::Proposed,
        );
        assert_eq!(looping.cycles(), vec![vec![one]]);
    }
}
