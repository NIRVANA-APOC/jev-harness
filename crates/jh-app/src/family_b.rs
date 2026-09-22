//! 任务族 B：现有问题无法区分的**成对场景**（`docs/RESEARCH_GOALS.md` §9.3、§6.5）。
//!
//! 一对场景的两侧在旧问题向量上近似相同，但隐藏真值要求不同的处理：
//!
//! | 对 | 两侧 |
//! | --- | --- |
//! | P1 | 证书过期 `expired_credential` / 时钟偏差 `clock_skew` |
//! | P2 | 上游限流 `upstream_throttle` / 客户端重试风暴 `retry_storm` |
//! | P3 | 权威记录缺失 `authority_missing` / 网络分区 `network_partition` |
//!
//! 每对两个**未见变体**（措辞、服务名与初始快照都重写），每对一 B1、一 B2：
//!
//! - **B1**：关键证据已在初始快照里给全（输入获取过程固定），检验表示是否表达了差异；
//! - **B2**：关键证据不在判断可见的材料里，只能用**计费**的观察动作取得（`ActionSpec::effect` 把动作
//!   的产出追加进证据日志），检验问题精化与主动观察调度的交互——观察计费防止「永远读取全部信息」
//!   成为无代价策略。
//!
//! # 材料充分性
//!
//! 固定问题只读 `app.log`（措辞故意与两侧都相容），因此**读不到**关键证据对象
//! （`credential.audit` / `upstream.audit` / `dns.audit`）。「材料是否足够」按这道题**这次读到的输入
//! 投影**判定（[`MaterialGate`]）：关键对象不在投影里时，正确读法是 `other`——这正是 M1 的 A1 教训
//! 的落点，问题定义里的 `other` 判据也写明「不要从候选集里挑一个最像的标签」。
//!
//! # 开发案例与未见变体
//!
//! `development` 是三个开发案例（每对一个），只用于反例提取、候选验证与晋升判据；`variants` 是六个
//! 未见变体，只用于 H3 的对照读数（§9.5：开发集上改东西，测试集上报告）。两边共用同一套问题定义与
//! 同一段循环代码。

use std::collections::{BTreeMap, BTreeSet};

use jh_core::{
    ObjectId, Question, QuestionId, QuestionLibrary, QuestionRegistry, QuestionSpec,
    QuestionVersion, SemanticState,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::Error;
use crate::family::{FamilyTask, MaterialGate, QuestionSet, TaskEntry, validate_task};
use crate::tasks::Truth;

/// 任务族 B 的内嵌 JSON 原文：编译期内嵌，运行时不依赖工作目录。
pub const FAMILY_B_JSON: &str = include_str!("../tasks/family_b.json");

/// 任务族 B 在仓库里的位置，写进报告以便复核。
pub const FAMILY_B_SOURCE: &str = "crates/jh-app/tasks/family_b.json";

/// 材料不足时问题声明的兜底读法（与问题定义里的 `other` 选项同名）。
pub const ABSENT_LABEL: &str = "other";

/// 一个 B 子集：B1（关键证据已给）或 B2（需主动观察、观察计费）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Subset {
    /// 已给关键证据。
    #[serde(rename = "B1")]
    Given,
    /// 需主动观察取得，观察计费。
    #[serde(rename = "B2")]
    Observed,
}

impl Subset {
    /// 报告里用的标签。
    pub fn label(self) -> &'static str {
        match self {
            Self::Given => "B1",
            Self::Observed => "B2",
        }
    }

    /// 中文说明。
    pub fn description(self) -> &'static str {
        match self {
            Self::Given => "B1：关键证据已给（输入获取过程固定）",
            Self::Observed => "B2：关键证据需主动观察取得（观察计费）",
        }
    }

    fn parse(label: &str) -> Option<Self> {
        match label {
            "B1" => Some(Self::Given),
            "B2" => Some(Self::Observed),
            _ => None,
        }
    }
}

/// 一对成对场景。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pair {
    /// 对标识（`P1`…）。
    pub id: String,
    /// 场景标题（两侧一起说清）。
    pub title: String,
    /// 该对的门控问题 ID（决策器读的那一个统一字段）。
    pub question: QuestionId,
    /// 关键证据对象：材料足够与否按它在不在输入投影里判定。
    pub key_object: ObjectId,
    /// 取得关键证据的观察动作（B2 变体里声明；B1 里关键证据已在初始快照）。
    pub probe_action: jh_core::ActionId,
    /// 两侧的隐藏标签。
    pub sides: Vec<String>,
    /// 标签 → 该侧应当发出的补救动作。
    pub remedies: BTreeMap<String, jh_core::ActionId>,
    /// 开发案例标识。
    pub dev_case: String,
    /// 两个未见变体标识。
    pub variants: Vec<String>,
}

/// 一个可在环境里读到的对象（生成候选问题时给出的可观测清单）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Observable {
    /// 对象。
    pub object: ObjectId,
    /// 取得方式：`initial`（初始快照里就有）或某个观察动作。
    pub obtained_by: String,
    /// 可用字段。
    pub fields: Vec<String>,
    /// 说明。
    pub note: String,
}

/// 一个验证用例（正例 / 反例 / 不同表述）：材料 → 正确读法。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationCase {
    /// 用例标识。
    pub id: String,
    /// 属于哪一对。
    pub pair: String,
    /// 这个用例在验证什么。
    pub context: String,
    /// 材料：对象 → 内容（装成判断请求的 state）。
    pub material: BTreeMap<ObjectId, Value>,
    /// 正确读法。
    pub expected: Truth,
}

/// 一个族 B 任务：任务本体 + 它属于哪一对、哪个子集、哪一侧。
#[derive(Debug, Clone, PartialEq)]
pub struct FamilyBTask {
    /// 任务本体（环境、动作、相位、隐藏真值都由它承载）。
    pub task: FamilyTask,
    /// 属于哪一对。
    pub pair: String,
    /// 哪个子集。
    pub subset: Subset,
    /// 这一侧（隐藏标签）。
    pub side: String,
    /// 这个任务只用于开发（反例提取、验证、晋升判据），不进 H3 的未见变体读数。
    pub development: bool,
}

impl FamilyBTask {
    /// 门控问题（本任务跟踪的那一道）。
    pub fn question(&self) -> &QuestionId {
        &self.task.tracked[0].question
    }

    /// 关键证据对象（该对声明的那个）。
    pub fn key_object<'a>(&self, family: &'a FamilyB) -> &'a ObjectId {
        &family
            .pair(&self.pair)
            .expect("任务的对必然存在")
            .key_object
    }
}

/// 任务族 B：三对场景 + 开发案例 + 未见变体 + 验证用例 + 可观测清单。
pub struct FamilyB {
    /// 任务集名字。
    pub name: String,
    /// 仓库内位置。
    pub source: String,
    /// 任务集说明。
    pub notes: String,
    /// 三对场景。
    pub pairs: Vec<Pair>,
    /// 可观测对象清单（生成候选问题时给出）。
    pub observables: Vec<Observable>,
    /// 验证用例。
    pub validation: Vec<ValidationCase>,
    /// 开发案例（每对一个）。
    pub development: Vec<FamilyBTask>,
    /// 未见变体（每对两个：一 B1 一 B2）。
    pub variants: Vec<FamilyBTask>,
    questions: BTreeMap<QuestionId, QuestionSpec>,
    scopes: BTreeMap<QuestionId, Vec<ObjectId>>,
    registry: QuestionRegistry,
}

impl FamilyB {
    /// 读取内嵌的任务族 B 并校验。
    pub fn load() -> Result<Self, Error> {
        Self::from_json(FAMILY_B_JSON)
    }

    /// 从 JSON 原文构造（测试与探针用同一套校验）。
    pub fn from_json(text: &str) -> Result<Self, Error> {
        let file: FamilyFile = serde_json::from_str(text)
            .map_err(|error| Error::TaskSet(format!("{FAMILY_B_SOURCE}：{error}")))?;

        let mut registry = QuestionRegistry::default();
        for entry in &file.questions {
            if entry.reads.is_empty() {
                return Err(invalid(&format!(
                    "问题 `{}` v{} 没有声明输入对象",
                    entry.id, entry.version
                )));
            }
            registry
                .register(
                    &entry.id,
                    &QuestionSpec {
                        version: entry.version,
                        question: entry.question.clone(),
                    },
                )
                .map_err(|error| invalid(&format!("问题 `{}` 登记失败：{error}", entry.id)))?;
        }
        let mut questions = BTreeMap::new();
        let mut scopes = BTreeMap::new();
        for entry in &file.questions {
            let latest = questions
                .get(&entry.id)
                .map(|spec: &QuestionSpec| spec.version)
                .unwrap_or(QuestionVersion(0));
            if entry.version >= latest {
                questions.insert(
                    entry.id.clone(),
                    QuestionSpec {
                        version: entry.version,
                        question: entry.question.clone(),
                    },
                );
                scopes.insert(entry.id.clone(), entry.reads.clone());
            }
        }
        if questions.len() != file.pairs.len() {
            return Err(invalid(&format!(
                "问题数 {} 与场景对数 {} 不一致（每对一个问题）",
                questions.len(),
                file.pairs.len()
            )));
        }
        if file.pairs.len() != 3 {
            return Err(invalid(&format!(
                "成对场景应为 3 对，实际 {} 对",
                file.pairs.len()
            )));
        }

        let by_id: BTreeMap<&str, &Pair> = file
            .pairs
            .iter()
            .map(|pair| (pair.id.as_str(), pair))
            .collect();
        for pair in &file.pairs {
            let Some(scope) = scopes.get(&pair.question) else {
                return Err(invalid(&format!(
                    "对 {} 的问题 `{}` 不在问题集里",
                    pair.id, pair.question
                )));
            };
            if scope.contains(&pair.key_object) {
                return Err(invalid(&format!(
                    "对 {}：固定问题 `{}` 的读取范围不该包含关键证据 `{}`（它就是要读不到）",
                    pair.id, pair.question, pair.key_object
                )));
            }
            if pair.sides.len() != 2 {
                return Err(invalid(&format!("对 {}：成对场景必须正好两侧", pair.id)));
            }
            let Some(question) = questions.get(&pair.question) else {
                return Err(invalid(&format!("对 {} 的问题缺失", pair.id)));
            };
            let Question::Choice(choice) = &question.question else {
                return Err(invalid(&format!(
                    "对 {} 的门控问题必须是选择类（决策器的前提门要按标签判）",
                    pair.id
                )));
            };
            for side in &pair.sides {
                if !choice.criteria.contains_key(side) {
                    return Err(invalid(&format!(
                        "对 {} 的侧 `{side}` 不在问题选项集里",
                        pair.id
                    )));
                }
                if !pair.remedies.contains_key(side) {
                    return Err(invalid(&format!(
                        "对 {} 的侧 `{side}` 没有补救动作",
                        pair.id
                    )));
                }
            }
            if !choice.criteria.contains_key(ABSENT_LABEL) {
                return Err(invalid(&format!(
                    "对 {} 的问题缺 `{ABSENT_LABEL}` 选项（材料不足时的兜底读法）",
                    pair.id
                )));
            }
        }
        for observable in &file.observables {
            if let Some(id) = observable.obtained_by.strip_prefix("tool:") {
                return Err(invalid(&format!(
                    "可观测对象 {} 的取得方式写成工具名 `{id}`：这里要写动作标识或 `initial`",
                    observable.object
                )));
            }
        }

        let development = build_tasks(&file.development, &by_id, true, &questions, &scopes)?;
        let variants = build_tasks(&file.variants, &by_id, false, &questions, &scopes)?;

        // 每对：一个开发案例 + 两个变体（一 B1 一 B2，两侧各一）。
        for pair in &file.pairs {
            let dev: Vec<&FamilyBTask> = development
                .iter()
                .filter(|task| task.pair == pair.id)
                .collect();
            if dev.len() != 1 {
                return Err(invalid(&format!(
                    "对 {} 的开发案例应为 1 个，实际 {} 个",
                    pair.id,
                    dev.len()
                )));
            }
            if dev[0].task.id != pair.dev_case {
                return Err(invalid(&format!(
                    "对 {} 声明的开发案例是 `{}`，实际是 `{}`",
                    pair.id, pair.dev_case, dev[0].task.id
                )));
            }
            let own: Vec<&FamilyBTask> = variants
                .iter()
                .filter(|task| task.pair == pair.id)
                .collect();
            if own.len() != 2 {
                return Err(invalid(&format!(
                    "对 {} 的未见变体应为 2 个，实际 {} 个",
                    pair.id,
                    own.len()
                )));
            }
            let subsets: BTreeSet<&str> = own.iter().map(|task| task.subset.label()).collect();
            if subsets != BTreeSet::from(["B1", "B2"]) {
                return Err(invalid(&format!(
                    "对 {} 的两个变体必须一 B1 一 B2，实际 {subsets:?}",
                    pair.id
                )));
            }
            let sides: BTreeSet<&str> = own.iter().map(|task| task.side.as_str()).collect();
            if sides.len() != 2 {
                return Err(invalid(&format!(
                    "对 {} 的两个变体必须各占一侧（同一侧不算成对）",
                    pair.id
                )));
            }
        }

        // 验证用例：材料充分性与期望读法必须自洽（有期望标签就必须有那份证据）。
        for case in &file.validation {
            let pair = by_id
                .get(case.pair.as_str())
                .ok_or_else(|| invalid(&format!("验证用例 {} 引用了未知对", case.id)))?;
            let Truth::Choice { label } = &case.expected else {
                return Err(invalid(&format!(
                    "验证用例 {} 的期望读法必须是选项标签",
                    case.id
                )));
            };
            let has_key = case.material.contains_key(&pair.key_object);
            let expects_label = label != ABSENT_LABEL;
            if expects_label != has_key {
                return Err(invalid(&format!(
                    "验证用例 {}：材料{}关键证据，期望读法却是 `{label}`（两者必须自洽）",
                    case.id,
                    if has_key { "有" } else { "没有" }
                )));
            }
            if !case.material.contains_key(&ObjectId::from("app.log")) {
                return Err(invalid(&format!(
                    "验证用例 {} 没有 app.log：固定问题读的就是它",
                    case.id
                )));
            }
        }

        Ok(Self {
            name: file.name,
            source: file.source,
            notes: file.notes,
            pairs: file.pairs,
            observables: file.observables,
            validation: file.validation,
            development,
            variants,
            questions,
            scopes,
            registry,
        })
    }

    /// 某一对。
    pub fn pair(&self, id: &str) -> Option<&Pair> {
        self.pairs.iter().find(|pair| pair.id == id)
    }

    /// 某个门控问题属于哪一对。
    pub fn pair_of_question(&self, question: &QuestionId) -> Option<&Pair> {
        self.pairs.iter().find(|pair| pair.question == *question)
    }

    /// 固定问题集（初始版本）。
    pub fn questions(&self) -> &BTreeMap<QuestionId, QuestionSpec> {
        &self.questions
    }

    /// 某个问题固定版本的读取范围。
    pub fn scope(&self, question: &QuestionId) -> Option<&[ObjectId]> {
        self.scopes.get(question).map(Vec::as_slice)
    }

    /// 问题版本表。
    pub fn registry(&self) -> &QuestionRegistry {
        &self.registry
    }

    /// 某个任务的包装（按标识在开发案例与变体里找）。
    pub fn task(&self, id: &str) -> Option<&FamilyBTask> {
        self.development
            .iter()
            .chain(self.variants.iter())
            .find(|task| task.task.id == id)
    }

    /// 开发案例。
    pub fn dev_case(&self, pair: &str) -> Option<&FamilyBTask> {
        self.development.iter().find(|task| task.pair == pair)
    }

    /// 问题集视图：固定版本（H3 的固定臂用它）。
    pub fn question_set(&self) -> FamilyBQuestions {
        FamilyBQuestions::fixed(self)
    }

    /// 问题集视图：以问题库的当前启用版本为准（H3 的精化臂用它）。
    ///
    /// 库里没有某道题的启用版本时回落到固定版本；H3 两臂的差别只在这里的取值，循环代码与前提门
    /// 一字不改（研究目标 §6.4）。
    pub fn question_set_from_library(&self, library: &QuestionLibrary) -> FamilyBQuestions {
        let mut view = FamilyBQuestions::fixed(self);
        for (question, fixed) in &view.fixed {
            let Some((version, entry)) = library.enabled(question) else {
                continue;
            };
            let Some(definition) = library.question(question, version) else {
                continue;
            };
            let reads: Vec<ObjectId> = entry.evidence.keys().cloned().collect();
            if reads.is_empty() {
                continue;
            }
            view.overrides.insert(
                question.clone(),
                FixedQuestion {
                    spec: QuestionSpec {
                        version,
                        question: definition.clone(),
                    },
                    reads,
                },
            );
            let _ = fixed;
        }
        view
    }

    /// 把某个问题的固定版本换成指定版本（影子/反事实臂用；不动问题库）。
    pub fn question_set_with_override(
        &self,
        question: &QuestionId,
        spec: QuestionSpec,
        reads: Vec<ObjectId>,
    ) -> FamilyBQuestions {
        let mut view = FamilyBQuestions::fixed(self);
        view.overrides
            .insert(question.clone(), FixedQuestion { spec, reads });
        view
    }

    /// 某个问题的当前定义（固定版本）。
    pub fn question(&self, question: &QuestionId) -> Option<&QuestionSpec> {
        self.questions.get(question)
    }

    /// 用族 B 的任务初始化一个语义状态：写初始快照，返回状态。
    ///
    /// 与判断循环里的 [`crate::family::Environment::seed`] 同一件事，单独留一个入口给「不跑循环、
    /// 只想在既有状态上做一次发布/回退联动」的演练用。
    pub fn seed(&self, task: &FamilyTask) -> Result<(SemanticState, usize), Error> {
        let mut state = SemanticState::in_memory();
        let environment = crate::family::Environment::new(task);
        environment.seed(&mut state)?;
        Ok((state, environment.phase()))
    }
}

/// 族 B 的问题集视图：固定版本 + 运行期覆盖（问题库的启用版本或影子版本）。
///
/// 这正是 §6.4 要求的「统一字段」：决策器只认 `(问题 ID, 版本, 读取范围)`，两臂共用同一段循环，
/// 换的只是覆盖表里的内容。
pub struct FamilyBQuestions {
    fixed: BTreeMap<QuestionId, FixedQuestion>,
    overrides: BTreeMap<QuestionId, FixedQuestion>,
}

/// 一道题的当前版本与读取范围。
struct FixedQuestion {
    spec: QuestionSpec,
    reads: Vec<ObjectId>,
}

impl FamilyBQuestions {
    fn fixed(family: &FamilyB) -> Self {
        let fixed = family
            .questions
            .iter()
            .map(|(question, spec)| {
                (
                    question.clone(),
                    FixedQuestion {
                        spec: spec.clone(),
                        reads: family.scopes.get(question).cloned().unwrap_or_default(),
                    },
                )
            })
            .collect();
        Self {
            fixed,
            overrides: BTreeMap::new(),
        }
    }

    /// 某个问题当前生效的版本（覆盖优先）。
    pub fn current(&self, question: &QuestionId) -> Option<&QuestionSpec> {
        self.overrides
            .get(question)
            .or_else(|| self.fixed.get(question))
            .map(|fixed| &fixed.spec)
    }

    /// 某个问题当前生效的读取范围。
    pub fn current_reads(&self, question: &QuestionId) -> Option<&[ObjectId]> {
        self.overrides
            .get(question)
            .or_else(|| self.fixed.get(question))
            .map(|fixed| fixed.reads.as_slice())
    }

    /// 全部问题当前生效的版本（报告里核对「这一臂用的是哪一版」）。
    pub fn versions(&self) -> BTreeMap<QuestionId, QuestionVersion> {
        self.fixed
            .keys()
            .filter_map(|question| {
                self.current(question)
                    .map(|spec| (question.clone(), spec.version))
            })
            .collect()
    }
}

impl QuestionSet for FamilyBQuestions {
    fn enabled_question(&self, question: &QuestionId) -> Option<QuestionSpec> {
        self.current(question).cloned()
    }

    fn reads(&self, question: &QuestionId) -> Option<Vec<ObjectId>> {
        self.current_reads(question).map(<[ObjectId]>::to_vec)
    }
}

#[derive(Debug, Deserialize)]
struct FamilyFile {
    name: String,
    source: String,
    #[serde(default)]
    notes: String,
    questions: Vec<QuestionEntry>,
    pairs: Vec<Pair>,
    observables: Vec<Observable>,
    validation: Vec<ValidationCase>,
    development: Vec<TaskEntryFile>,
    variants: Vec<TaskEntryFile>,
}

#[derive(Debug, Clone, Deserialize)]
struct QuestionEntry {
    id: QuestionId,
    version: QuestionVersion,
    reads: Vec<ObjectId>,
    question: Question,
}

#[derive(Debug, Deserialize)]
struct TaskEntryFile {
    #[serde(flatten)]
    task: TaskEntry,
    pair: String,
    subset: String,
    side: String,
}

fn build_tasks(
    entries: &[TaskEntryFile],
    pairs: &BTreeMap<&str, &Pair>,
    development: bool,
    questions: &BTreeMap<QuestionId, QuestionSpec>,
    scopes: &BTreeMap<QuestionId, Vec<ObjectId>>,
) -> Result<Vec<FamilyBTask>, Error> {
    let mut tasks = Vec::with_capacity(entries.len());
    for entry in entries {
        let id = entry.task.id.clone();
        let fail = |detail: String| invalid(&format!("任务 {id}：{detail}"));
        let pair = pairs
            .get(entry.pair.as_str())
            .ok_or_else(|| fail(format!("引用了未知对 `{}`", entry.pair)))?;
        let subset = Subset::parse(&entry.subset)
            .ok_or_else(|| fail(format!("子集 `{}` 不是 B1 / B2", entry.subset)))?;
        if !pair.sides.contains(&entry.side) {
            return Err(fail(format!(
                "侧 `{}` 不在对 {} 的两侧 {:?} 里",
                entry.side, pair.id, pair.sides
            )));
        }

        let mut task = entry.task.clone();
        // 材料充分性判据由场景声明，不由任务自己写：同一对的两个变体共用一条判据。
        task.material_gate.insert(
            pair.question.clone(),
            MaterialGate {
                key_object: pair.key_object.clone(),
                absent_label: ABSENT_LABEL.to_owned(),
            },
        );
        let task = validate_task(task, questions, scopes)?;

        // B1/B2 的形态差异（在任务本体上核，而不是在 JSON 里核）。
        let probe = task
            .actions
            .iter()
            .find(|action| action.id == pair.probe_action);
        match (subset, probe) {
            (Subset::Observed, Some(probe)) => {
                let effect = probe.effect.as_ref().ok_or_else(|| {
                    fail(format!(
                        "B2 的观察动作 `{}` 必须声明产出（effect），否则关键证据永远进不了证据日志",
                        pair.probe_action
                    ))
                })?;
                if effect.object != pair.key_object {
                    return Err(fail(format!(
                        "B2 的观察动作产出 `{}`，与关键证据 `{}` 不一致",
                        effect.object, pair.key_object
                    )));
                }
                let gated_on_other = probe.requires.iter().any(|gate| {
                    gate.question == pair.question && gate.equals.as_deref() == Some(ABSENT_LABEL)
                });
                if !gated_on_other {
                    return Err(fail(format!(
                        "B2 的观察动作 `{}` 必须由「{ABSENT_LABEL}（材料不足）」把关",
                        pair.probe_action
                    )));
                }
                if task
                    .initial
                    .iter()
                    .any(|entry| entry.object == pair.key_object)
                {
                    return Err(fail(format!(
                        "B2 的关键证据 `{}` 不该出现在初始快照里（那就不需要观察了）",
                        pair.key_object
                    )));
                }
            }
            (Subset::Observed, None) => {
                return Err(fail(format!("B2 必须声明观察动作 `{}`", pair.probe_action)));
            }
            (Subset::Given, Some(_)) => {
                return Err(fail(format!(
                    "B1 不声明观察动作（输入获取过程固定），却带了 `{}`",
                    pair.probe_action
                )));
            }
            (Subset::Given, None) => {
                if !task
                    .initial
                    .iter()
                    .any(|entry| entry.object == pair.key_object)
                {
                    return Err(fail(format!(
                        "B1 的关键证据 `{}` 必须在初始快照里给全",
                        pair.key_object
                    )));
                }
            }
        }

        tasks.push(FamilyBTask {
            task,
            pair: entry.pair.clone(),
            subset,
            side: entry.side.clone(),
            development,
        });
    }
    Ok(tasks)
}

fn invalid(detail: &str) -> Error {
    Error::TaskSet(detail.to_owned())
}

impl FamilyB {
    /// 初始快照里的证据条目数（报告里的规模读数）。
    pub fn initial_evidence_count(&self, task: &FamilyTask) -> usize {
        task.initial.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::family::StepWhen;
    use jh_core::NewEvidence;

    #[test]
    fn the_embedded_family_b_loads_with_three_pairs_and_two_variants_each() {
        let family = FamilyB::load().expect("内嵌任务族 B 应自洽");

        assert_eq!(family.name, "family-b");
        assert_eq!(family.pairs.len(), 3);
        assert_eq!(family.development.len(), 3);
        assert_eq!(family.variants.len(), 6);
        assert_eq!(family.questions().len(), 3);

        // 每对一 B1 一 B2、两侧各一。
        for pair in &family.pairs {
            let own: Vec<&FamilyBTask> = family
                .variants
                .iter()
                .filter(|task| task.pair == pair.id)
                .collect();
            assert_eq!(own.len(), 2);
            assert_ne!(own[0].side, own[1].side);
            assert_ne!(own[0].subset, own[1].subset);
        }
    }

    #[test]
    fn the_fixed_question_cannot_read_the_key_evidence() {
        let family = FamilyB::load().unwrap();
        for pair in &family.pairs {
            let reads = family.scope(&pair.question).expect("固定问题有读取范围");
            assert_eq!(reads, &[ObjectId::from("app.log")]);
            assert!(!reads.contains(&pair.key_object));
        }
    }

    #[test]
    fn the_failure_scene_really_fails_under_the_fixed_question_set() {
        // 材料不足 → 正确读法是 other；固定问题读的就是 app.log，所以「读出候选集标签」就是错的。
        let family = FamilyB::load().unwrap();
        let variant = family.task("P1-V1").expect("变体存在");
        let task = &variant.task;
        let question = variant.question().clone();
        let gate = task.material_gate.get(&question).expect("有材料判据");

        // B1：关键证据在初始快照里，但固定问题读不到它 → 真值按兜底读法。
        let without_key = |object: &ObjectId| object != &gate.key_object;
        assert_eq!(
            task.truth_for(0, &question, without_key),
            Some(Truth::Choice {
                label: gate.absent_label.clone()
            }),
            "读不到关键证据时正确读法是兜底标签"
        );
        // 精化版本读到关键证据时，真值是对应那一侧。
        assert_eq!(
            task.truth_for(0, &question, |_| true),
            Some(Truth::Choice {
                label: variant.side.clone()
            })
        );
    }

    #[test]
    fn the_b2_variant_starts_without_the_key_evidence_and_the_probe_produces_it() {
        let family = FamilyB::load().unwrap();
        let variant = family.task("P2-V1").expect("B2 变体存在");
        assert_eq!(variant.subset, Subset::Observed);

        let pair = family.pair("P2").unwrap();
        assert!(
            !variant
                .task
                .initial
                .iter()
                .any(|entry| entry.object == pair.key_object)
        );
        let probe = variant
            .task
            .actions
            .iter()
            .find(|action| action.id == pair.probe_action)
            .expect("观察动作存在");
        let effect = probe.effect.as_ref().expect("观察动作有产出");
        assert_eq!(effect.object, pair.key_object);
        assert!(probe.cost_units > 0.0, "观察动作必须计费");
        assert_eq!(
            variant.task.phases[0].required,
            vec![pair.probe_action.clone()],
            "B2 的第一相位就要求发出观察动作"
        );
        assert!(
            variant.task.phases[1]
                .required
                .contains(&pair.remedies[&variant.side]),
            "观察之后应当要求发出补救动作"
        );
    }

    #[test]
    fn the_material_gate_is_wired_into_the_round_loop() {
        // 端到端（离线规则判定器）见 tests/h3.rs；这里只核环境本身：B2 的观察动作确实把关键证据
        // 追加进证据日志，且它引起的失效传播被记下来。
        let family = FamilyB::load().unwrap();
        let variant = family.task("P3-V2").expect("B2 变体存在");
        let pair = family.pair("P3").unwrap();
        let (mut state, phase) = family.seed(&variant.task).expect("初始快照应能落库");
        assert_eq!(phase, 0);

        let probe = variant
            .task
            .actions
            .iter()
            .find(|action| action.id == pair.probe_action)
            .expect("观察动作存在");
        let effect = probe.effect.clone().expect("观察动作有产出");
        let (_, invalidation) = state
            .append_evidence(NewEvidence {
                source: effect.source,
                at_unix_ms: 1,
                object: effect.object.clone(),
                version: effect.version,
                content: effect.content,
            })
            .expect("追加证据应成功");
        assert_eq!(
            state.evidence().current_version(&pair.key_object),
            Some(effect.version)
        );

        // 再看环境脚本：提问前的无关变化确实生效（相位推进靠它）。
        let mut environment = crate::family::Environment::new(&variant.task);
        let effects = environment
            .apply_round(1, StepWhen::BeforeAsk, &mut state)
            .expect("脚本应能应用");
        assert_eq!(effects.len(), 1);
        assert_eq!(environment.phase(), 1);
        let _ = invalidation;
    }

    #[test]
    fn validation_cases_are_self_consistent() {
        let family = FamilyB::load().unwrap();
        assert_eq!(family.validation.len(), 12, "3 对 × 4 个验证用例");
        for case in &family.validation {
            let pair = family.pair(&case.pair).unwrap();
            match &case.expected {
                Truth::Choice { label } if label == ABSENT_LABEL => {
                    assert!(!case.material.contains_key(&pair.key_object));
                }
                Truth::Choice { label } => {
                    assert!(pair.sides.contains(label));
                    assert!(case.material.contains_key(&pair.key_object));
                }
                other => panic!("验证用例的期望读法应是选项标签：{other:?}"),
            }
        }
    }

    #[test]
    fn a_library_backed_view_switches_the_version_without_touching_the_loop() {
        let family = FamilyB::load().unwrap();
        let question = QuestionId::from("b_pair_cause_p1");
        let fixed = family.question_set();
        assert_eq!(
            fixed.enabled_question(&question).unwrap().version,
            QuestionVersion(1)
        );
        assert_eq!(
            fixed.reads(&question).unwrap(),
            vec![ObjectId::from("app.log")]
        );

        // 覆盖一份新版本（消费者声明与门控问题同名）：视图换版本、换读取范围，别的不动。
        let refined = family.question_set_with_override(
            &question,
            QuestionSpec {
                version: QuestionVersion(2),
                question: Question::Noul(jh_core::NoulQuestion {
                    instructions: "换一个问法".to_owned(),
                    criteria: None,
                }),
            },
            vec![
                ObjectId::from("app.log"),
                ObjectId::from("credential.audit"),
            ],
        );
        assert_eq!(
            refined.enabled_question(&question).unwrap().version,
            QuestionVersion(2)
        );
        assert_eq!(refined.reads(&question).unwrap().len(), 2);
        // 固定视图没被改动（两臂用的是两份视图）。
        assert_eq!(
            family
                .question_set()
                .enabled_question(&question)
                .unwrap()
                .version,
            QuestionVersion(1)
        );
    }
}
