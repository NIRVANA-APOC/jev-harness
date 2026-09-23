//! 声明式测量契约、观测器与独立检验（研究目标 §7.2 / §7.3 末段 / §7.4）。
//!
//! 本模块做三件事，都围绕「观测器这件事能不能被验证」：
//!
//! 1. **声明式测量契约**（§7.2 八条）：由真实生成模型产出，再逐项核对。契约落到
//!    [`jh_core::ObserverContract`] 上——M21 已经把八条做成了类型，这里只负责生成、解析与核对，
//!    不另立一套字段。越界钩子点、缺字段、把要求换成更弱命题一律显式失败（[`check_contract`]）。
//! 2. **观测器实现**（[`Observer`]）：按契约选中的钩子点装证据通道，给出三档判定。它的数据源是
//!    [`crate::c_program::RecorderLog`]（写入边界记录器）与
//!    [`crate::c_program::PauseTrace`]（可控暂停点）——**没有**存储日志。存储日志只有
//!    [`crate::c_program::hidden_check`] 读，两者分属不同模块、不同通道。
//! 3. **独立检验**（[`independent_check`]）：阳性对照、阴性对照、可注入故障、独立隐藏检查四路齐备，
//!    装进 [`jh_core::ObserverCheck`] 交给 [`jh_core::ObserverLibrary::enable`]。观测器没有通过这四路
//!    就进不了启用态，这是 M21 的类型约束；本模块负责**跑出真实的四路读数**，并额外跑一遍
//!    「自证式的伪隐藏检查会被门槛拒绝」，把「不可自证」也变成一条可核的读数
//!    （[`enable_with_independent_check`]）。
//!
//! # 禁止的替换（§7.3）
//!
//! 「不得把要求换成『最终缓存为空』」这条禁令在本模块里是**可读的代码**：
//! [`BANNED_SUBSTITUTIONS`] 列出被禁的弱化命题，[`check_contract`] 的
//! `proposition_not_weakened` 一项按它显式失败；固定测试臂 C1 的检查项**故意**用了其中一条
//! （[`fixed_declaration`] 里标着 `weakened_substitution`），因为它就是「已有观察工具」能做到的
//! 极限——它不是用来满足要求的，是用来当对照的。
//!
//! # 与通用测试生成的边界（§7.4 的第二臂）
//!
//! 通用测试生成臂**不改宿主代码**，因此只能看族 C 之前就存在的可观察量
//! （[`ObservableVocabulary`]）。它产出的断言被编译进一个封闭的枚举（[`Check`]），并且整份计划
//! 要过一遍越界词扫描（[`OUT_OF_SCOPE_TOKENS`]）——引用了钩子点、记录器或存储日志就直接失败。

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use jh_core::{
    CheckDirection, ControlResult, ObjectId, ObserverCheck, ObserverContract, ObserverId,
    ObserverLibrary, ObserverLifecycle, ObserverVerdict, SemanticState, Timing,
};
use serde::{Deserialize, Serialize};

use crate::c_program::{
    Attachment, CFamily, COUNTER_KEY, HookPoint, Implementation, Op, ProgramVersion, RunRecord,
    Schedule,
};
use crate::error::Error;
use crate::generation::{ContractCheck, ContractItem};
use crate::judge::llm::{complete, from_env_client};

/// 被禁止的替换：把原始要求换成更弱、可由终态观测代替的命题（研究目标 §7.3 明令）。
///
/// 「最终缓存为空」「终态无写入」这类命题**不等价**于「取消确认之后不再写入」：一次合法的
/// 暂停前写入（`c1-s02`）会把它判成违反，而一次确认后写入再被合法回收抹掉（`c1-s05`）会把它判成
/// 通过。两个方向都错——见 [`crate::h4`] 的固定测试臂读数。
pub const BANNED_SUBSTITUTIONS: [&str; 5] = [
    "最终缓存为空",
    "终态无写入",
    "最终没有写入",
    "最后没有写入",
    "最终不写入",
];

/// 观测器不许引用的测量位置：它们要么属于隐藏通道，要么根本不存在。
pub const OUT_OF_SCOPE_SITE_TOKENS: [&str; 6] = [
    "store_journal",
    "commit_journal",
    "hidden_check",
    "direct_path",
    "sqlite",
    "filesystem",
];

/// 通用测试生成臂不许引用的词（钩子点、记录器、存储日志都属于「改了宿主代码才有的东西」）。
pub const OUT_OF_SCOPE_TOKENS: [&str; 8] = [
    "recorder",
    "write_boundary",
    "pre_write_pause",
    "commit_counter",
    "journal",
    "hidden",
    "hook",
    "pause",
];

/// 一次模型调用的读数（通道上报什么就记什么，不重试、不折算货币成本）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeasureCall {
    /// 模型执行者版本（通道响应的模型 ID）。
    pub model: Option<String>,
    /// 通道侧请求 id。
    pub request_id: Option<String>,
    /// 上报的 token 用量。
    pub usage: Option<jh_core::Usage>,
    /// 送出的提示词字节数。
    pub input_bytes: u64,
    /// 收回的输出字节数。
    pub output_bytes: u64,
    /// 本次调用耗时（毫秒）。
    pub latency_ms: u64,
    /// 失败原文（成功时为 `None`）。
    pub error: Option<String>,
}

impl MeasureCall {
    /// 调用是否成功。
    pub fn succeeded(&self) -> bool {
        self.error.is_none()
    }
}

/// 一条判定读数：三档判定 + 依据说明（三个臂共用同一个形状）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verdict {
    /// 判定。
    pub verdict: ObserverVerdict,
    /// 依据说明。
    pub detail: String,
}

impl Verdict {
    /// 命题成立。
    pub fn satisfied(detail: impl Into<String>) -> Self {
        Self {
            verdict: ObserverVerdict::Satisfied,
            detail: detail.into(),
        }
    }

    /// 发现违反。
    pub fn violated(detail: impl Into<String>) -> Self {
        Self {
            verdict: ObserverVerdict::Violated,
            detail: detail.into(),
        }
    }

    /// 这次没测出来（不折成「成立」，也不算「违反」）。
    pub fn failed(detail: impl Into<String>) -> Self {
        let detail = detail.into();
        Self {
            verdict: ObserverVerdict::Failed(detail.clone()),
            detail,
        }
    }

    /// 是否给出了一个明确的判定（非失败态）。
    pub fn decided(&self) -> bool {
        !matches!(self.verdict, ObserverVerdict::Failed(_))
    }
}

// ---------------------------------------------------------------------------
// 声明式测量契约：生成、解析、核对
// ---------------------------------------------------------------------------

/// 生成一份契约的提示词。
///
/// 材料里给的是**声明的钩子点与要求**，不给答案：选哪些钩子点、命题怎么表述、失败条件怎么写，
/// 都由模型给出，之后由 [`check_contract`] 与四路独立检验判定。
pub fn contract_prompt(family: CFamily, version: ProgramVersion) -> String {
    let hook_points: Vec<serde_json::Value> = HookPoint::ALL
        .into_iter()
        .map(|point| {
            serde_json::json!({
                "id": point.id(),
                "description": point.description(),
            })
        })
        .collect();
    let material = serde_json::json!({
        "requirement": {"text": family.requirement(), "source": family.requirement_source()},
        "object": family.object().to_string(),
        "family": family.id(),
        "program_version": version.id(),
        "program": version.note(),
        "available_hook_points": hook_points,
        "evidence_channels": {
            "observer:c-write-recorder": "写入边界的事件记录器（钩子点 write_boundary）",
            "host:c-pause-points": "可控暂停点的轨迹（钩子点 pre_write_pause）",
        },
        "known_gaps": family.uncovered(),
    });

    format!(
        "你在为一个隔离实验环境写「测量契约」的八条声明。你要回答的是：要判定下面这条要求是否被满足，\n\
         应该在什么地方、量什么命题、怎么表达失败。\n\n\
         只输出一个 JSON 对象，顶层键与取值必须是这八条（缺一条、多一条、空字符串都会被拒绝）：\n\
         {{\n\
         \x20 \"requirement\": {{\"text\": \"原始要求原文\", \"source\": \"要求的来源\"}},\n\
         \x20 \"proposition\": \"要测量的具体命题（一句话，必须能判断真假）\",\n\
         \x20 \"coverage\": \"这个命题覆盖原要求的哪一部分、哪一段情形，哪里不覆盖\",\n\
         \x20 \"measurement\": {{\"site\": \"测量位置，只能引用上面列出的钩子点 id，多个用 + 连接\",\n\
         \x20                 \"objects\": [\"被观测的对象标识\"], \"timing\": \"instant 或 sequence\"}},\n\
         \x20 \"support\": {{\"versions\": [\"支持的版本\"], \"configuration\": \"需要的配置\",\n\
         \x20             \"inputs\": \"能观测哪一类输入\", \"environment\": \"必要环境条件\"}},\n\
         \x20 \"disturbance\": {{\"side_effects\": \"副作用\", \"performance\": \"性能扰动\",\n\
         \x20                 \"scheduling\": \"调度变化\"}},\n\
         \x20 \"controls\": {{\"positive\": \"阳性对照\", \"negative\": \"阴性对照\",\n\
         \x20              \"injected_fault\": \"可注入的已知故障\"}},\n\
         \x20 \"output\": {{\"channel\": \"证据渠道名\", \"format\": \"证据内容与字段\",\n\
         \x20            \"failure\": \"观测器读不到数据时怎么表示失败\",\n\
         \x20            \"invalid_when\": \"什么情况下这个观测器的结论不再成立\"}},\n\
         \x20 \"uncovered\": \"尚未覆盖的情况\"\n\
         }}\n\n\
         约束：\n\
         1) `timing` 必须选 `sequence`：这条要求讲的是事件的先后，单点观测表达不了。\n\
         2) `measurement.site` 里**只许写**上面列出的钩子点 id（一字不差），多个之间用 ` + ` 连接，\n\
         例如 `write_boundary + pre_write_pause + commit_counter`；不要写别的词、不要写函数名或文件路径。\n\
         3) `measurement.objects` **只许写**材料里给出的对象标识那一个（`object` 字段的值），\n\
         不要写钩子点的事件流（那些是 `site` 的事），也不要写描述性文字。\n\
         4) 命题必须讲清「在什么事件之后还发生了写入」这一层顺序关系，不许把它换成\n\
         「最终缓存为空」「终态没有写入」这类只看终态的弱命题——那样两次相反的情形都会判错。\n\
         5) `output.failure` 要写清楚：当记录器被绕过或事件序号不可比时，给出**失败状态**而不是\n\
         「命题成立」。如果你的命题靠某个钩子点才能发现被绕过，请把它列进 `site`。\n\n\
         材料（实验环境向所有人公开的部分）：\n{material}",
        material = serde_json::to_string_pretty(&material).unwrap_or_default()
    )
}

/// 把模型输出解析成契约（严格：缺字段、空字段、非对象一律显式失败）。
pub fn parse_contract(output: &str) -> Result<ObserverContract, Error> {
    let trimmed = output.trim();
    let value: serde_json::Value = serde_json::from_str(trimmed).map_err(|source| {
        Error::TaskSet(format!(
            "契约输出不是约定的 JSON（{source}）：{}",
            preview(trimmed)
        ))
    })?;
    let object = value
        .as_object()
        .ok_or_else(|| Error::TaskSet(format!("契约输出不是 JSON 对象：{}", preview(trimmed))))?;

    for key in [
        "requirement",
        "proposition",
        "coverage",
        "measurement",
        "support",
        "disturbance",
        "controls",
        "output",
        "uncovered",
    ] {
        if !object.contains_key(key) {
            return Err(Error::TaskSet(format!("契约缺少必填字段 `{key}`")));
        }
    }
    for key in ["proposition", "coverage", "uncovered"] {
        if object
            .get(key)
            .and_then(|value| value.as_str())
            .is_none_or(|text| text.trim().is_empty())
        {
            return Err(Error::TaskSet(format!("契约字段 `{key}` 是空的")));
        }
    }

    serde_json::from_value::<ObserverContract>(value)
        .map_err(|source| Error::TaskSet(format!("契约字段与 §7.2 八条对不上（{source}）")))
}

fn preview(text: &str) -> String {
    let head: String = text.chars().take(200).collect();
    if text.chars().count() > 200 {
        format!("{head}…")
    } else {
        head
    }
}

/// 契约的协议核对：八条里可结构化核对的那几项（口径与问题库、候选问题一致：只核对能核的，
/// 内容质量交给四路独立检验与消费方判定）。
pub fn check_contract(
    contract: &ObserverContract,
    family: CFamily,
    version: ProgramVersion,
) -> ContractCheck {
    let mut checks = Vec::new();
    let mut push = |name: &str, passed: bool, detail: String| {
        checks.push(ContractItem {
            name: name.to_owned(),
            passed,
            detail,
        });
    };

    let site = contract.measurement.site.as_str();
    let selected: Vec<HookPoint> = HookPoint::ALL
        .into_iter()
        .filter(|point| site_tokens(site).any(|token| token == point.id()))
        .collect();
    push(
        "site_within_declared_hook_points",
        !selected.is_empty()
            && !OUT_OF_SCOPE_SITE_TOKENS
                .iter()
                .any(|token| site.contains(token)),
        format!(
            "site=`{site}`；认出已声明的钩子点 {:?}；越界词 {}（这一项是词面核对，不是语义证明）",
            selected.iter().map(|point| point.id()).collect::<Vec<_>>(),
            if OUT_OF_SCOPE_SITE_TOKENS
                .iter()
                .any(|token| site.contains(token))
            {
                "命中"
            } else {
                "没有命中"
            }
        ),
    );

    let declared: BTreeSet<ObjectId> = BTreeSet::from([family.object()]);
    let extra: Vec<&ObjectId> = contract
        .measurement
        .objects
        .iter()
        .filter(|object| !declared.contains(*object))
        .collect();
    push(
        "objects_within_declared",
        !contract.measurement.objects.is_empty() && extra.is_empty(),
        format!(
            "声明对象 {:?}，越界对象 {:?}",
            contract
                .measurement
                .objects
                .iter()
                .map(|object| object.to_string())
                .collect::<Vec<_>>(),
            extra
                .iter()
                .map(|object| object.to_string())
                .collect::<Vec<_>>()
        ),
    );

    push(
        "timing_is_sequence",
        contract.measurement.timing == Timing::Sequence,
        format!(
            "timing={:?}；这条要求的顺序语义是「{}」，单点观测表达不了",
            contract.measurement.timing,
            family.timing_note()
        ),
    );

    let banned = BANNED_SUBSTITUTIONS
        .into_iter()
        .find(|pattern| contract.proposition.contains(pattern));
    push(
        "proposition_not_weakened",
        banned.is_none(),
        match banned {
            Some(pattern) => {
                format!("命题被换成了更弱的形式（命中 `{pattern}`）：这不是这条要求的等价表述")
            }
            None => format!("命题 `{}` 没有命中被禁的弱化形式", contract.proposition),
        },
    );

    let mentions_order = family
        .ordering_tokens()
        .iter()
        .any(|token| contract.proposition.contains(token));
    let mentions_write = ["写", "提交"]
        .iter()
        .any(|token| contract.proposition.contains(token));
    let mentions_order = mentions_order && mentions_write;
    push(
        "proposition_keeps_the_ordering",
        mentions_order,
        format!(
            "命题 `{}` 是否讲到了这一族的顺序关系（必须出现 {:?} 之一，且出现「写」或「提交」）：{}",
            contract.proposition,
            family.ordering_tokens(),
            if mentions_order { "是" } else { "不是" }
        ),
    );

    push(
        "failure_state_declared",
        !contract.output.failure.trim().is_empty()
            && !contract.output.invalid_when.trim().is_empty(),
        format!(
            "failure=`{}`；invalid_when=`{}`",
            contract.output.failure, contract.output.invalid_when
        ),
    );

    push(
        "support_covers_the_version",
        contract.support.versions.contains(version.id()),
        format!(
            "契约声明支持的版本 {:?}，本次被测版本 {}",
            contract.support.versions.iter().collect::<Vec<_>>(),
            version.id()
        ),
    );

    push(
        "channel_declared",
        !contract.output.channel.trim().is_empty(),
        format!("证据渠道 `{}`", contract.output.channel),
    );

    push(
        "uncovered_declared",
        contract.uncovered.trim().chars().count() >= 4,
        format!("未覆盖情况 `{}`", contract.uncovered),
    );

    let passed = checks.iter().all(|check| check.passed);
    ContractCheck { passed, checks }
}

/// 一次契约生成的产物（含调用读数与核对结果）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeneratedContract {
    /// 所属故障族。
    pub family: CFamily,
    /// 这次生成的调用读数。
    pub call: MeasureCall,
    /// 解析成功的契约（失败时为 `None`）。
    pub contract: Option<ObserverContract>,
    /// 协议核对结果（解析失败时为 `None`）。
    pub check: Option<ContractCheck>,
    /// 失败原文（解析或核对被拒时记这里）。
    pub rejection: Option<String>,
}

/// 契约生成器：真调 LLM 通道（`OPENAI_MODEL` / `OPENAI_BASE_URL` / `OPENAI_API_KEY`）。
pub struct ContractGenerator {
    client: jh_openai::Client,
}

impl ContractGenerator {
    /// 从环境变量构造；缺 `OPENAI_MODEL` / `OPENAI_BASE_URL` 显式失败。
    pub fn from_env() -> Result<Self, Error> {
        Ok(Self {
            client: from_env_client()?,
        })
    }

    /// 每个族各生成一份契约。单次失败如实记在该族的产物里，不重试、不拿别的族顶替。
    pub async fn generate(
        &self,
        families: &[CFamily],
        version: ProgramVersion,
    ) -> Result<Vec<GeneratedContract>, Error> {
        let mut out = Vec::with_capacity(families.len());
        for family in families {
            out.push(self.generate_one(*family, version).await?);
        }
        Ok(out)
    }

    /// 生成一个族的契约。
    pub async fn generate_one(
        &self,
        family: CFamily,
        version: ProgramVersion,
    ) -> Result<GeneratedContract, Error> {
        let prompt = contract_prompt(family, version);
        let input_bytes = prompt.len() as u64;
        let outcome = complete(&self.client, prompt).await;
        Ok(finish_contract(family, version, input_bytes, outcome))
    }
}

/// 把一次通道调用的结果收成契约产物（解析与核对都在这里做，失败不重试）。
pub(crate) fn finish_contract(
    family: CFamily,
    version: ProgramVersion,
    input_bytes: u64,
    outcome: Result<crate::judge::llm::Completion, jh_core::Error>,
) -> GeneratedContract {
    let call = match &outcome {
        Ok(completion) => MeasureCall {
            model: Some(completion.model.clone()),
            request_id: Some(completion.request_id.clone()),
            usage: completion.usage.clone(),
            input_bytes,
            output_bytes: completion.text.len() as u64,
            latency_ms: completion.latency.as_millis() as u64,
            error: None,
        },
        Err(error) => MeasureCall {
            model: None,
            request_id: None,
            usage: None,
            input_bytes,
            output_bytes: 0,
            latency_ms: 0,
            error: Some(error.to_string()),
        },
    };

    let Ok(completion) = outcome else {
        let error = call.error.clone().unwrap_or_default();
        return GeneratedContract {
            family,
            call,
            contract: None,
            check: None,
            rejection: Some(format!("生成调用失败：{error}")),
        };
    };

    match parse_contract(&completion.text) {
        Ok(contract) => {
            let check = check_contract(&contract, family, version);
            let rejection = (!check.passed).then(|| {
                format!(
                    "协议核对未通过：{}",
                    check
                        .checks
                        .iter()
                        .filter(|item| !item.passed)
                        .map(|item| format!("{}（{}）", item.name, item.detail))
                        .collect::<Vec<_>>()
                        .join("；")
                )
            });
            GeneratedContract {
                family,
                call,
                contract: Some(contract),
                check: Some(check),
                rejection,
            }
        }
        Err(error) => GeneratedContract {
            family,
            call,
            contract: None,
            check: None,
            rejection: Some(error.to_string()),
        },
    }
}

/// 从落盘的产物里读回契约（离线复跑用；读数自产物里读回，不重复落调用记录）。
pub fn read_contracts(path: &Path) -> Result<Vec<GeneratedContract>, Error> {
    let text = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&text)?)
}

// ---------------------------------------------------------------------------
// 观测器：按契约装通道、给三档判定
// ---------------------------------------------------------------------------

/// 契约声明的测量位置选中了哪些钩子点。
///
/// `measurement.site` 里钩子点 id 的分隔符不敏感（空白、`+`、`、`、`,`、`/` 都算），
/// 但 id 本身必须与声明的一字不差：`write_boundary+commit_counter` 这种连写也认。
fn selected_points(contract: &ObserverContract) -> Vec<HookPoint> {
    let site = contract.measurement.site.as_str();
    HookPoint::ALL
        .into_iter()
        .filter(|point| site_tokens(site).any(|token| token == point.id()))
        .collect()
}

/// 把测量位置文本切成 token（按空白与常见分隔符切）。
fn site_tokens(site: &str) -> impl Iterator<Item = &str> {
    site.split(|character: char| {
        character.is_whitespace() || matches!(character, '+' | '、' | ',' | '，' | '/' | '|')
    })
    .filter(|token| !token.is_empty())
}

/// 观测器：契约声明的测量位置决定它装哪些钩子点。
///
/// 「选钩子点」这件事是有后果的：[`HookPoint::CommitCounter`] 是唯一能发现「记录器被绕过」的通道
/// （它只知道条数、不知道内容），[`HookPoint::WriteBoundary`] 是唯一能指出「哪一次写在确认之后」
/// 的通道。只选一个都会有盲区，这一点在 [`crate::h4`] 的读数里直接看得到。
#[derive(Debug, Clone)]
pub struct Observer {
    family: CFamily,
    contract: ObserverContract,
    points: Vec<HookPoint>,
}

impl Observer {
    /// 按契约装观测器；契约没选中任何已声明的钩子点时显式失败。
    pub fn build(
        family: CFamily,
        contract: ObserverContract,
        version: ProgramVersion,
    ) -> Result<Self, Error> {
        let check = check_contract(&contract, family, version);
        if !check.passed {
            return Err(Error::TaskSet(format!(
                "契约没有通过协议核对，装不出观测器：{}",
                check
                    .checks
                    .iter()
                    .filter(|item| !item.passed)
                    .map(|item| item.name.clone())
                    .collect::<Vec<_>>()
                    .join("、")
            )));
        }
        let points = selected_points(&contract);
        Ok(Self {
            family,
            contract,
            points,
        })
    }

    /// 契约。
    pub fn contract(&self) -> &ObserverContract {
        &self.contract
    }

    /// 这个观测器装了哪些钩子点。
    pub fn points(&self) -> &[HookPoint] {
        &self.points
    }

    /// 换一份契约（版本变更时只改 `support.versions`、或改测量位置的那种更新走这里）。
    ///
    /// 钩子点由契约的 `measurement.site` 重新算：改契约换了测量位置，装的通道就跟着换。
    pub fn with_contract(&self, contract: ObserverContract) -> Self {
        let points = selected_points(&contract);
        Self {
            family: self.family,
            contract,
            points,
        }
    }

    /// 给出这次运行的判定（三档）。
    pub fn observe(&self, run: &RunRecord) -> Verdict {
        if !self.points.contains(&HookPoint::WriteBoundary) {
            return Verdict::failed(format!(
                "契约选的测量位置（{}）里没有 write_boundary：看不到任何一次提交，给不出判定",
                self.contract.measurement.site
            ));
        }
        if !self.contract.support.versions.contains(run.version.id()) {
            return Verdict::failed(format!(
                "被测版本 {} 不在契约声明支持的版本里（{:?}）：这个观测器不适用于它，结论不成立",
                run.version.id(),
                self.contract.support.versions.iter().collect::<Vec<_>>()
            ));
        }
        if self.points.contains(&HookPoint::CommitCounter)
            && run.commit_seq > run.recorded_commits() as u64
        {
            return Verdict::failed(format!(
                "记录器被绕过：存储的真实提交是 {} 条，记录器只记到 {} 条",
                run.commit_seq,
                run.recorded_commits()
            ));
        }
        self.judge(run)
    }

    /// 按族在**记录器通道**上算判定（隐藏检查在存储日志通道上另有实现，两者不共用代码）。
    fn judge(&self, run: &RunRecord) -> Verdict {
        match self.family {
            CFamily::CancelAfterConfirm => {
                let mut violated = Vec::new();
                for event in &run.recorder {
                    if run
                        .confirmed_at(&event.task)
                        .is_some_and(|confirmed| event.event > confirmed)
                    {
                        violated.push(event);
                    }
                }
                match violated.first() {
                    Some(event) => Verdict::violated(format!(
                        "任务 {} 在确认之后仍被记录到写入（键 {}，事件序号 {}）",
                        event.task, event.key, event.event
                    )),
                    None => Verdict::satisfied(format!(
                        "记录器记到 {} 次写入，没有一次落在所属任务的确认之后",
                        run.recorded_commits()
                    )),
                }
            }
            CFamily::LostUpdate => {
                let mut bases: BTreeMap<(String, u64), Vec<u64>> = BTreeMap::new();
                for event in &run.recorder {
                    bases
                        .entry((event.key.clone(), event.expected_version))
                        .or_default()
                        .push(event.seq);
                }
                match bases.iter().find(|(_, seqs)| seqs.len() > 1) {
                    Some(((key, base), seqs)) => Verdict::violated(format!(
                        "键 {key} 上有 {} 次写入都以基准版本 {base} 为依据（提交序号 {:?}）",
                        seqs.len(),
                        seqs
                    )),
                    None => Verdict::satisfied(format!(
                        "记录器记到 {} 次写入，每次声明的基准版本互不相同",
                        run.recorded_commits()
                    )),
                }
            }
            CFamily::DuplicateAfterRestart => {
                let mut counted: BTreeMap<(String, String), u32> = BTreeMap::new();
                for event in &run.recorder {
                    *counted
                        .entry((event.task.clone(), event.op.clone()))
                        .or_default() += 1;
                }
                match counted.iter().find(|(_, count)| **count > 1) {
                    Some(((task, op), count)) => Verdict::violated(format!(
                        "记录器记到任务 {task} 的写入意图 {op} 提交了 {count} 次"
                    )),
                    None => Verdict::satisfied(format!(
                        "记录器记到 {} 次写入，每个写入意图只出现一次",
                        run.recorded_commits()
                    )),
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 独立检验：四路齐备 + 自证式检查被门槛拒绝
// ---------------------------------------------------------------------------

/// 独立检验的原始读数：四路各自的观测器输出与隐藏检查输出。
///
/// 这是「测量脚本的成功输出不能成为唯一的真实性来源」的落点——四路里有两路（阴性对照、
/// 独立隐藏检查）**期望**观测器与隐藏检查各自抓住已知错误实现，跑不出这两个结果就不能启用。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckRun {
    /// 哪一路。
    pub direction: CheckDirection,
    /// 这一路用的实现。
    pub implementation: Implementation,
    /// 这一路跑的调度。
    pub schedule: String,
    /// 是否注入了故障。
    pub injected: bool,
    /// 观测器给出的判定。
    pub observer: Verdict,
    /// 隐藏检查给出的判定（独立通道）。
    pub hidden: Verdict,
    /// 这一路期望观测器给什么。
    pub expected: ObserverVerdict,
    /// 说明。
    pub note: String,
}

/// 独立检验的产物。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndependentCheck {
    /// 交给 `ObserverLibrary::enable` 的那份四路记录。
    pub check: ObserverCheck,
    /// 四路的原始读数（含观测器自己的输出，失败的那一路也在）。
    pub runs: Vec<CheckRun>,
    /// 观测器在「绕过记录器」那一路漏掉了没有（隐藏检查同时抓到了）。
    pub observer_missed_the_bypass: bool,
    /// 这次检验用哪个版本跑的。
    pub version: ProgramVersion,
}

/// 四路独立检验（§7.3 末段）：已知错误实现必须触发失败、合理实现必须通过，两个方向都跑。
///
/// | 路 | 实现 | 注入 | 期望 |
/// | --- | --- | --- | --- |
/// | 阳性对照 | 合理实现 | 无 | 观测器「命题成立」 |
/// | 阴性对照 | 不守顺序约束的已知错误实现 | 无 | 观测器「发现违反」 |
/// | 可注入故障 | 合理实现 | 有（走带记录器的边界） | 观测器「发现违反」 |
/// | 独立隐藏检查 | 绕过记录器的已知错误实现 | 无 | **隐藏检查**「发现违反」 |
///
/// 第四路期望的是隐藏检查而不是观测器——这正是它独立的意义：观测器可能什么都看不到，
/// 那也无法让这一路变成通过。
pub fn independent_check(
    family: CFamily,
    version: ProgramVersion,
    observer: &Observer,
    at_unix_ms: u64,
) -> IndependentCheck {
    let schedule = control_schedule(family);
    let attachment = Attachment::at_boundary();
    let mut runs = Vec::new();

    let positive_run = crate::c_program::run_case(
        family,
        &schedule,
        Implementation::Correct,
        version,
        attachment.clone(),
        false,
    );
    runs.push(check_run(
        CheckDirection::Positive,
        Implementation::Correct,
        &positive_run,
        &schedule.id,
        false,
        observer.observe(&positive_run),
        ObserverVerdict::Satisfied,
        "合理实现在这条调度上应当没有任何违反",
    ));

    let negative_run = crate::c_program::run_case(
        family,
        &schedule,
        Implementation::Uncoordinated,
        version,
        attachment.clone(),
        false,
    );
    runs.push(check_run(
        CheckDirection::Negative,
        Implementation::Uncoordinated,
        &negative_run,
        &schedule.id,
        false,
        observer.observe(&negative_run),
        ObserverVerdict::Violated,
        "已知错误实现（不守顺序约束）必须被抓到",
    ));

    let injected_run = crate::c_program::run_case(
        family,
        &schedule,
        Implementation::Correct,
        version,
        attachment.clone(),
        true,
    );
    runs.push(check_run(
        CheckDirection::InjectedFault,
        Implementation::Correct,
        &injected_run,
        &schedule.id,
        true,
        observer.observe(&injected_run),
        ObserverVerdict::Violated,
        "宿主注入的已知故障必须被抓到",
    ));

    let bypass_run = crate::c_program::run_case(
        family,
        &schedule,
        Implementation::QuietBypass,
        version,
        attachment,
        false,
    );
    let observer_on_bypass = observer.observe(&bypass_run);
    let hidden_on_bypass = verdict_of(&bypass_run.hidden.verdict, &bypass_run.hidden.detail);
    let observer_missed = !matches!(observer_on_bypass.verdict, ObserverVerdict::Violated)
        && matches!(hidden_on_bypass.verdict, ObserverVerdict::Violated);
    runs.push(CheckRun {
        direction: CheckDirection::Hidden,
        implementation: Implementation::QuietBypass,
        schedule: schedule.id.clone(),
        injected: false,
        observer: observer_on_bypass.clone(),
        hidden: hidden_on_bypass.clone(),
        expected: ObserverVerdict::Violated,
        note: "绕过记录器的已知错误实现：观测器可以看不到它，独立隐藏检查必须看到真实副作用"
            .to_owned(),
    });

    // 第四路按定义用隐藏检查的判定，不用观测器的输出——观测器看不到它，这一路照样成立。
    let hidden_control = control_result(
        ObserverVerdict::Violated,
        &hidden_on_bypass,
        &format!(
            "{}；观测器自己给的是「{}」",
            runs[3].note, observer_on_bypass.verdict
        ),
    );
    let check = ObserverCheck {
        at_unix_ms,
        positive: control_of(&runs[0]),
        negative: control_of(&runs[1]),
        injected_fault: control_of(&runs[2]),
        hidden: hidden_control,
        note: format!(
            "四路独立检验：{family}，版本 {}，调度 {}；观测器装了 {:?}",
            version.id(),
            schedule.id,
            observer.points()
        ),
    };

    IndependentCheck {
        check,
        runs,
        observer_missed_the_bypass: observer_missed,
        version,
    }
}

#[allow(clippy::too_many_arguments)]
fn check_run(
    direction: CheckDirection,
    implementation: Implementation,
    run: &RunRecord,
    schedule: &str,
    injected: bool,
    observer: Verdict,
    expected: ObserverVerdict,
    note: &str,
) -> CheckRun {
    CheckRun {
        direction,
        implementation,
        schedule: schedule.to_owned(),
        injected,
        observer,
        hidden: verdict_of(&run.hidden.verdict, &run.hidden.detail),
        expected,
        note: note.to_owned(),
    }
}

fn verdict_of(verdict: &ObserverVerdict, detail: &str) -> Verdict {
    Verdict {
        verdict: verdict.clone(),
        detail: detail.to_owned(),
    }
}

fn control_of(run: &CheckRun) -> ControlResult {
    control_result(
        run.expected.clone(),
        &run.observer,
        &format!(
            "{}（实现 {}，调度 {}，隐藏检查：{}）",
            run.note,
            run.implementation.id(),
            run.schedule,
            run.hidden.detail
        ),
    )
}

fn control_result(expected: ObserverVerdict, observed: &Verdict, note: &str) -> ControlResult {
    ControlResult {
        expected,
        observed: observed.verdict.clone(),
        readings: BTreeMap::from([
            ("schedules".to_owned(), 1.0),
            (
                "decided".to_owned(),
                if observed.decided() { 1.0 } else { 0.0 },
            ),
        ]),
        note: format!("{note}；实际读数：{}", observed.detail),
    }
}

/// 控制臂用的那条调度：每族的第一条（已知会暴露不守约束的实现）。
pub fn control_schedule(family: CFamily) -> Schedule {
    crate::c_program::schedules(family)
        .into_iter()
        .next()
        .expect("每族都有调度")
}

/// 观测器生命周期：先拿一份自证式的伪隐藏检查去启用（必须被拒），再拿真的四路记录启用。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnableOutcome {
    /// 观测器版本号（登记成功时）。
    pub version: Option<u32>,
    /// 自证式检查被拒绝的原文（`ObserverNotVerified`）。
    pub self_certifying_error: Option<String>,
    /// 自证式检查是否真的被门槛拒绝。
    pub self_certifying_rejected: bool,
    /// 被拒之后生命周期保持原状（写回状态名）。
    pub lifecycle_after_rejection: Option<String>,
    /// 被拒之后库里留下的检验记录数（失败的那一条也留档）。
    pub checks_on_record_after_rejection: usize,
    /// 启用成功时写下的生命周期记录（版本 → 状态）。
    pub enabled_records: Vec<(u32, String)>,
    /// 启用替代旧版本时的失效传播读数。
    pub invalidation: Option<String>,
}

/// 走完「登记 → 验证 → 启用」，并把「自证式的伪隐藏检查会被拒绝」这一步也跑出来。
///
/// `proper` 是 [`independent_check`] 产出的四路记录；`self_certifying` 是同一份记录，但第四路改用
/// **观测器自己的输出**顶替隐藏检查（自证）。第二次启用必须成功，第一次必须被
/// [`jh_core::Error::ObserverNotVerified`] 拒绝，且失败的那一条检验记录留在库里。
pub fn enable_with_independent_check(
    library: &mut ObserverLibrary,
    state: &mut SemanticState,
    family: CFamily,
    contract: ObserverContract,
    proper: ObserverCheck,
    self_certifying: ObserverCheck,
    at_unix_ms: u64,
) -> Result<EnableOutcome, Error> {
    let id = observer_id(family);
    let version = library.propose(
        &id,
        contract,
        at_unix_ms,
        format!("观测需求：{family} 的顺序约束没有现成观察工具能取到证据"),
    )?;
    library.start_verifying(
        &id,
        version,
        at_unix_ms + 1,
        "开始 §7.3 的独立检验".to_owned(),
    )?;

    let rejection = library
        .enable(
            state,
            &id,
            version,
            self_certifying,
            at_unix_ms + 2,
            "试启用：第四路用的是观测器自己的输出".to_owned(),
        )
        .err();
    let self_certifying_error = rejection.as_ref().map(ToString::to_string);
    let self_certifying_rejected =
        matches!(rejection, Some(jh_core::Error::ObserverNotVerified { .. }));
    let lifecycle_after_rejection = library
        .at(&id, version)
        .map(|entry| entry.lifecycle.to_string());
    let checks_on_record_after_rejection = library
        .at(&id, version)
        .map(|entry| entry.checks.len())
        .unwrap_or(0);

    let (records, invalidation) = library.enable(
        state,
        &id,
        version,
        proper,
        at_unix_ms + 3,
        "四路独立检验全过".to_owned(),
    )?;

    Ok(EnableOutcome {
        version: Some(version.0),
        self_certifying_error,
        self_certifying_rejected,
        lifecycle_after_rejection,
        checks_on_record_after_rejection,
        enabled_records: records
            .iter()
            .map(|record| (record.version.0, record.lifecycle.to_string()))
            .collect(),
        invalidation: invalidation.map(|invalidation| {
            format!(
                "起因 {:?}；待重查 {} 个判断、{} 个行动，传播路径 {} 条",
                invalidation.cause,
                invalidation.stale.len(),
                invalidation.blocked.len(),
                invalidation.paths.len()
            )
        }),
    })
}

/// 观测器的标识（每个族一个，与对象标识对齐）。
pub fn observer_id(family: CFamily) -> ObserverId {
    ObserverId::from(format!("c_{}_write_order_recorder", family.id()))
}

/// 把一份独立检验记录改造成「自证式」的：第四路用观测器自己的输出顶替隐藏检查。
pub fn self_certifying(check: &ObserverCheck, observer_on_bypass: &Verdict) -> ObserverCheck {
    let mut forged = check.clone();
    forged.hidden = control_result(
        ObserverVerdict::Violated,
        observer_on_bypass,
        "自证式伪隐藏检查：直接拿观测器自己的输出当独立证据",
    );
    forged.note = format!("{}（第四路被换成自证）", check.note);
    forged
}

/// 记录生命周期状态名（报告里用得到）。
pub fn lifecycle_name(lifecycle: ObserverLifecycle) -> String {
    lifecycle.to_string()
}

// ---------------------------------------------------------------------------
// 第二臂：通用测试生成（LLM 生成测试，不改宿主代码）
// ---------------------------------------------------------------------------

/// 族 C 之前就存在的可观察量：通用测试生成臂只能用这些。
///
/// 「不改宿主代码」这句话在这里是**可核的**——断言被编译进封闭枚举 [`Check`]，
/// 引用了钩子点 / 记录器 / 存储日志的断言在编译期就不存在，词面上还会被
/// [`OUT_OF_SCOPE_TOKENS`] 拦一次。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservableVocabulary {
    /// 可选的任务。
    pub tasks: Vec<String>,
    /// 可选的键。
    pub keys: Vec<String>,
    /// 可用的断言种类。
    pub assertions: Vec<String>,
}

/// 生成测试计划的提示词。
pub fn tests_prompt(family: CFamily, version: ProgramVersion) -> String {
    let vocabulary = vocabulary(family);
    let material = serde_json::json!({
        "requirement": {"text": family.requirement(), "source": family.requirement_source()},
        "family": family.id(),
        "program_version": version.id(),
        "observable_vocabulary": serde_json::to_value(&vocabulary).unwrap_or_default(),
        "note": "你只能使用上面列出的可观察量：宿主代码不会被改动，记录器、暂停点、存储日志都不存在",
    });
    format!(
        "为一个可控程序写一组**回归测试断言**，用来判断下面这条要求有没有被违反。\n\n\
         只输出一个 JSON 对象：\n\
         {{\n\
         \x20 \"family\": \"{}\",\n\
         \x20 \"checks\": [\n\
         \x20   {{\"assertion\": \"status_equals\",   \"task\": \"...\", \"expected\": \"...\"}},\n\
         \x20   {{\"assertion\": \"value_equals\",    \"key\": \"...\",  \"expected\": \"...\"}},\n\
         \x20   {{\"assertion\": \"intents_equals\",  \"task\": \"...\", \"expected\": \"0\"}},\n\
         \x20   {{\"assertion\": \"version_at_least\", \"key\": \"...\", \"expected\": \"1\"}}\n\
         \x20 ],\n\
         \x20 \"note\": \"这组断言为什么能判断这条要求\"\n\
         }}\n\n\
         约束：\n\
         1) `assertion` 只能是上面四种之一；`expected` 一律写成字符串；\n\
         2) 任务与键只能从 observable_vocabulary 里选；\n\
         3) 所有断言必须落在**程序自己报出来的可观察量**上：宿主代码不改，任何内部记录都不存在。\n\n\
         材料：\n{material}",
        family.id(),
        material = serde_json::to_string_pretty(&material).unwrap_or_default()
    )
}

/// 族的可观察量词表。
pub fn vocabulary(family: CFamily) -> ObservableVocabulary {
    ObservableVocabulary {
        tasks: crate::c_program::declared_tasks(family),
        keys: crate::c_program::declared_keys(family),
        assertions: [
            "status_equals",
            "value_equals",
            "intents_equals",
            "version_at_least",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
    }
}

/// 生成模型给出的一条断言（未经编译）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawCheck {
    /// 断言种类。
    pub assertion: String,
    /// 任务（选择类断言用）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    /// 键（状态类断言用）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// 期望值（一律是字符串）。
    pub expected: String,
}

/// 生成模型给出的测试计划（未经编译）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawPlan {
    /// 目标族。
    pub family: String,
    /// 断言列表。
    pub checks: Vec<RawCheck>,
    /// 说明。
    pub note: String,
}

/// 编译之后的一条断言。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "assertion")]
pub enum Check {
    /// 任务的自报状态等于期望。
    StatusEquals {
        /// 任务。
        task: String,
        /// 期望值。
        expected: String,
    },
    /// 键的当前值等于期望。
    ValueEquals {
        /// 键。
        key: String,
        /// 期望值。
        expected: String,
    },
    /// 任务自报的写入意图数等于期望。
    IntentsEquals {
        /// 任务。
        task: String,
        /// 期望值。
        expected: u32,
    },
    /// 键的版本不低于期望。
    VersionAtLeast {
        /// 键。
        key: String,
        /// 期望值。
        expected: u64,
    },
}

impl Check {
    /// 这个断言看的是哪一个对象。
    pub fn subject(&self) -> String {
        match self {
            Self::StatusEquals { task, .. } | Self::IntentsEquals { task, .. } => {
                format!("task:{task}")
            }
            Self::ValueEquals { key, .. } | Self::VersionAtLeast { key, .. } => {
                format!("key:{key}")
            }
        }
    }
}

/// 把一份原始计划编译成断言（越界即显式失败：未知断言种类、越界任务或键、引用钩子点）。
pub fn compile_plan(plan: &RawPlan, family: CFamily) -> Result<Vec<Check>, Error> {
    if plan.family != family.id() {
        return Err(Error::TaskSet(format!(
            "测试计划声明的族是 `{}`，与要跑的 `{}` 不一致",
            plan.family,
            family.id()
        )));
    }
    if plan.checks.is_empty() {
        return Err(Error::TaskSet("测试计划里一条断言都没有".to_owned()));
    }

    let declared = vocabulary(family);
    let mut compiled = Vec::with_capacity(plan.checks.len());
    for raw in &plan.checks {
        for (field, value) in [
            ("note", Some(plan.note.as_str())),
            ("task", raw.task.as_deref()),
            ("key", raw.key.as_deref()),
            ("expected", Some(raw.expected.as_str())),
            ("assertion", Some(raw.assertion.as_str())),
        ] {
            let Some(value) = value else { continue };
            if let Some(token) = OUT_OF_SCOPE_TOKENS
                .iter()
                .find(|token| value.to_lowercase().contains(**token))
            {
                return Err(Error::TaskSet(format!(
                    "通用测试生成臂不得引用宿主钩子点：字段 `{field}` 里出现了 `{token}`"
                )));
            }
        }

        let need_task = |task: &Option<String>| -> Result<String, Error> {
            let task = task
                .clone()
                .ok_or_else(|| Error::TaskSet(format!("断言 `{}` 缺少 `task`", raw.assertion)))?;
            if !declared.tasks.contains(&task) {
                return Err(Error::TaskSet(format!(
                    "断言引用了任务 `{task}`，它不在可观察量词表里（{:?}）",
                    declared.tasks
                )));
            }
            Ok(task)
        };
        let need_key = |key: &Option<String>| -> Result<String, Error> {
            let key = key
                .clone()
                .ok_or_else(|| Error::TaskSet(format!("断言 `{}` 缺少 `key`", raw.assertion)))?;
            if !declared.keys.contains(&key) {
                return Err(Error::TaskSet(format!(
                    "断言引用了键 `{key}`，它不在可观察量词表里（{:?}）",
                    declared.keys
                )));
            }
            Ok(key)
        };

        compiled.push(match raw.assertion.as_str() {
            "status_equals" => Check::StatusEquals {
                task: need_task(&raw.task)?,
                expected: raw.expected.clone(),
            },
            "value_equals" => Check::ValueEquals {
                key: need_key(&raw.key)?,
                expected: raw.expected.clone(),
            },
            "intents_equals" => Check::IntentsEquals {
                task: need_task(&raw.task)?,
                expected: raw.expected.parse::<u32>().map_err(|source| {
                    Error::TaskSet(format!(
                        "断言 `intents_equals` 的期望值 `{}` 不是整数（{source}）",
                        raw.expected
                    ))
                })?,
            },
            "version_at_least" => Check::VersionAtLeast {
                key: need_key(&raw.key)?,
                expected: raw.expected.parse::<u64>().map_err(|source| {
                    Error::TaskSet(format!(
                        "断言 `version_at_least` 的期望值 `{}` 不是整数（{source}）",
                        raw.expected
                    ))
                })?,
            },
            other => {
                return Err(Error::TaskSet(format!(
                    "未知的断言种类 `{other}`（可用：{:?}）",
                    declared.assertions
                )));
            }
        });
    }
    Ok(compiled)
}

/// 一次测试生成的产物。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeneratedTests {
    /// 所属族。
    pub family: CFamily,
    /// 这次生成的调用读数。
    pub call: MeasureCall,
    /// 模型给的原始计划。
    pub plan: Option<RawPlan>,
    /// 编译出来的断言。
    pub checks: Vec<Check>,
    /// 被拒的原文（越界、缺字段、未知断言种类……）。
    pub rejection: Option<String>,
}

/// 测试计划生成器：与契约生成器同一条通道。
pub struct TestGenerator {
    client: jh_openai::Client,
}

impl TestGenerator {
    /// 从环境变量构造。
    pub fn from_env() -> Result<Self, Error> {
        Ok(Self {
            client: from_env_client()?,
        })
    }

    /// 每个族各生成一份测试计划。
    pub async fn generate(
        &self,
        families: &[CFamily],
        version: ProgramVersion,
    ) -> Result<Vec<GeneratedTests>, Error> {
        let mut out = Vec::with_capacity(families.len());
        for family in families {
            let prompt = tests_prompt(*family, version);
            let input_bytes = prompt.len() as u64;
            let outcome = complete(&self.client, prompt).await;
            out.push(finish_tests(*family, input_bytes, outcome));
        }
        Ok(out)
    }
}

/// 把一次通道调用的结果收成测试产物（解析、编译、越界核对都在这里）。
pub(crate) fn finish_tests(
    family: CFamily,
    input_bytes: u64,
    outcome: Result<crate::judge::llm::Completion, jh_core::Error>,
) -> GeneratedTests {
    let call = match &outcome {
        Ok(completion) => MeasureCall {
            model: Some(completion.model.clone()),
            request_id: Some(completion.request_id.clone()),
            usage: completion.usage.clone(),
            input_bytes,
            output_bytes: completion.text.len() as u64,
            latency_ms: completion.latency.as_millis() as u64,
            error: None,
        },
        Err(error) => MeasureCall {
            model: None,
            request_id: None,
            usage: None,
            input_bytes,
            output_bytes: 0,
            latency_ms: 0,
            error: Some(error.to_string()),
        },
    };

    let Ok(completion) = outcome else {
        return GeneratedTests {
            family,
            call,
            plan: None,
            checks: Vec::new(),
            rejection: Some("生成调用失败（不重试）".to_owned()),
        };
    };

    match serde_json::from_str::<RawPlan>(completion.text.trim()) {
        Ok(plan) => match compile_plan(&plan, family) {
            Ok(checks) => GeneratedTests {
                family,
                call,
                plan: Some(plan),
                checks,
                rejection: None,
            },
            Err(error) => GeneratedTests {
                family,
                call,
                plan: Some(plan),
                checks: Vec::new(),
                rejection: Some(error.to_string()),
            },
        },
        Err(error) => GeneratedTests {
            family,
            call,
            plan: None,
            checks: Vec::new(),
            rejection: Some(format!(
                "测试计划不是约定的 JSON（{error}）：{}",
                preview(completion.text.trim())
            )),
        },
    }
}

/// 从落盘的产物里读回测试计划。
pub fn read_tests(path: &Path) -> Result<Vec<GeneratedTests>, Error> {
    let text = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&text)?)
}

// ---------------------------------------------------------------------------
// 三个臂各自的判定
// ---------------------------------------------------------------------------

/// 固定测试臂的声明（写进报告；`weakened_substitution` 标出被禁的那一条）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixedDeclaration {
    /// 检查项名字。
    pub name: String,
    /// 断言原文。
    pub assertion: String,
    /// 这一项是不是 §7.3 明令禁止的弱化替换。
    pub weakened_substitution: bool,
    /// 说明。
    pub note: String,
}

/// 固定测试臂的检查项：只用族 C 之前就存在的可观察量（自报状态、当前值、意图数）。
pub fn fixed_declaration(family: CFamily) -> Vec<FixedDeclaration> {
    match family {
        CFamily::CancelAfterConfirm => vec![
            FixedDeclaration {
                name: "status_is_cancelled".to_owned(),
                assertion: "status(T1) == \"cancelled\"".to_owned(),
                weakened_substitution: false,
                note: "程序自报状态：取消请求被收到了".to_owned(),
            },
            FixedDeclaration {
                name: "final_state_has_no_write".to_owned(),
                assertion: format!("value({}) == 空", crate::c_program::task_key("T1")),
                weakened_substitution: true,
                note: format!(
                    "**这就是 §7.3 禁止的替换**（`{}`）：把「确认之后不得写入」换成「最终没有写入」。\
                     暂停之前的合法写入会把它判成违反，确认之后写入再被合法回收会把它判成通过——两个方向都错",
                    BANNED_SUBSTITUTIONS[0]
                ),
            },
        ],
        CFamily::LostUpdate => vec![FixedDeclaration {
            name: "counter_equals_increments".to_owned(),
            assertion: format!("value({COUNTER_KEY}) == 调度里的自增次数"),
            weakened_substitution: false,
            note: "终态计数：丢失更新会把计数写低，终态看得见".to_owned(),
        }],
        CFamily::DuplicateAfterRestart => vec![FixedDeclaration {
            name: "intents_is_one".to_owned(),
            assertion: "intents(T1) == 1".to_owned(),
            weakened_substitution: false,
            note: "程序自报的写入意图数：它数的是意图，不是真实提交——重放重复提交时它照样是 1"
                .to_owned(),
        }],
    }
}

/// 固定测试臂的判定。
pub fn fixed_verdict(family: CFamily, schedule: &Schedule, run: &RunRecord) -> Verdict {
    let observables = &run.observables;
    match family {
        CFamily::CancelAfterConfirm => {
            let task = "T1";
            let key = crate::c_program::task_key(task);
            let status_ok = observables.status(task) == Some("cancelled");
            let state_ok = observables.value(&key).is_none();
            if status_ok && state_ok {
                Verdict::satisfied(format!(
                    "自报状态是 cancelled，且 {key} 在终态里为空（这条断言就是被禁的弱化替换）"
                ))
            } else {
                Verdict::violated(format!(
                    "自报状态 {}，{key} 的终态 {}（这条断言就是被禁的弱化替换）",
                    observables.status(task).unwrap_or("—"),
                    match observables.value(&key) {
                        Some(value) => format!("是 `{value}`"),
                        None => "为空".to_owned(),
                    }
                ))
            }
        }
        CFamily::LostUpdate => {
            let increments = schedule
                .ops
                .iter()
                .filter(|op| matches!(op, Op::Increment { .. }))
                .count();
            let expected = increments.to_string();
            match observables.value(COUNTER_KEY) {
                Some(value) if value == expected => Verdict::satisfied(format!(
                    "{COUNTER_KEY} 的终态是 {value}，等于调度里的 {increments} 次自增"
                )),
                other => Verdict::violated(format!(
                    "{COUNTER_KEY} 的终态是 {}，调度里有 {increments} 次自增",
                    other.unwrap_or("空")
                )),
            }
        }
        CFamily::DuplicateAfterRestart => {
            let task = "T1";
            match observables.intents(task) {
                Some(1) => Verdict::satisfied("自报的写入意图数是 1（它数的是意图）"),
                other => Verdict::violated(format!(
                    "自报的写入意图数是 {}（期望 1）",
                    other.map_or_else(|| "—".to_owned(), |value| value.to_string())
                )),
            }
        }
    }
}

/// 通用测试生成臂的判定：跑那些编译出来的断言。
pub fn generated_verdict(checks: &[Check], run: &RunRecord) -> Verdict {
    let observables = &run.observables;
    let mut failures = Vec::new();
    for check in checks {
        let outcome = match check {
            Check::StatusEquals { task, expected } => {
                observables.status(task) == Some(expected.as_str())
            }
            Check::ValueEquals { key, expected } => {
                observables.value(key) == Some(expected.as_str())
            }
            Check::IntentsEquals { task, expected } => observables.intents(task) == Some(*expected),
            Check::VersionAtLeast { key, expected } => observables.version(key) >= *expected,
        };
        if !outcome {
            failures.push(check.subject());
        }
    }
    if failures.is_empty() {
        Verdict::satisfied(format!("{} 条断言全部通过", checks.len()))
    } else {
        Verdict::violated(format!(
            "{} 条断言里有 {} 条不通过：{}",
            checks.len(),
            failures.len(),
            failures.join("、")
        ))
    }
}

/// 观测器臂的判定（把观测器包一层，附上它装了哪些通道）。
pub fn observer_verdict(observer: &Observer, run: &RunRecord) -> Verdict {
    observer.observe(run)
}

/// 隐藏检查的判定（独立通道，只在评分与独立检验里用）。
pub fn hidden_verdict(run: &RunRecord) -> Verdict {
    verdict_of(&run.hidden.verdict, &run.hidden.detail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::c_program::{run_case, schedules};

    fn contract_of(family: CFamily) -> ObserverContract {
        ObserverContract {
            requirement: jh_core::Requirement {
                text: family.requirement().to_owned(),
                source: family.requirement_source().to_owned(),
            },
            proposition: format!(
                "同一任务在收到取消确认之后是否还产生写入（{}）",
                family.timing_note()
            ),
            coverage: "覆盖「确认之后的写入」这一类顺序违反；不覆盖写入内容的正确性".to_owned(),
            measurement: jh_core::Measurement {
                site: "write_boundary + pre_write_pause + commit_counter".to_owned(),
                objects: BTreeSet::from([family.object()]),
                timing: Timing::Sequence,
            },
            support: jh_core::Support {
                versions: BTreeSet::from([ProgramVersion::V1.id().to_owned()]),
                configuration: "recorder=on".to_owned(),
                inputs: "同一任务的写入事件与暂停点事件".to_owned(),
                environment: "隔离实验环境，含可注入的暂停点".to_owned(),
            },
            disturbance: jh_core::Disturbance {
                side_effects: "追加写记录文件，不改被测数据".to_owned(),
                performance: "每事件一次追加写".to_owned(),
                scheduling: "在写入边界同步记录".to_owned(),
            },
            controls: jh_core::Controls {
                positive: "合理实现".to_owned(),
                negative: "已知错误实现".to_owned(),
                injected_fault: "注入一次故障".to_owned(),
            },
            output: jh_core::OutputContract {
                channel: crate::c_program::RECORDER_CHANNEL.to_owned(),
                format: "每条写入事件一条证据（任务、键、事件序号）".to_owned(),
                failure: "读不到时显式失败，不给读不到的数".to_owned(),
                invalid_when: "记录器被绕过或事件序号不可比".to_owned(),
            },
            uncovered: family.uncovered().to_owned(),
        }
    }

    #[test]
    fn a_complete_contract_passes_the_protocol_check() {
        for family in CFamily::ALL {
            let check = check_contract(&contract_of(family), family, ProgramVersion::V1);
            assert!(check.passed, "{family}：{:?}", check.checks);
        }
    }

    #[test]
    fn the_banned_substitution_is_rejected_by_name() {
        let family = CFamily::CancelAfterConfirm;
        let mut contract = contract_of(family);
        contract.proposition = BANNED_SUBSTITUTIONS[0].to_owned();
        let check = check_contract(&contract, family, ProgramVersion::V1);
        assert!(!check.passed);
        let item = check
            .checks
            .iter()
            .find(|item| item.name == "proposition_not_weakened")
            .expect("这一项在");
        assert!(!item.passed);
        assert!(item.detail.contains(BANNED_SUBSTITUTIONS[0]), "{item:?}");
    }

    #[test]
    fn out_of_scope_hook_points_and_objects_are_rejected() {
        let family = CFamily::LostUpdate;
        let mut contract = contract_of(family);
        contract.measurement.site = "store_journal".to_owned();
        assert!(!check_contract(&contract, family, ProgramVersion::V1).passed);

        let mut contract = contract_of(family);
        contract.measurement.site = "some_undeclared_point".to_owned();
        assert!(!check_contract(&contract, family, ProgramVersion::V1).passed);

        let mut contract = contract_of(family);
        contract.measurement.objects = BTreeSet::from([ObjectId::from("cache.writes.elsewhere")]);
        let check = check_contract(&contract, family, ProgramVersion::V1);
        assert!(!check.passed);
        assert!(
            !check
                .checks
                .iter()
                .find(|item| item.name == "objects_within_declared")
                .unwrap()
                .passed
        );
    }

    #[test]
    fn an_instant_measurement_cannot_carry_an_ordering_requirement() {
        let family = CFamily::CancelAfterConfirm;
        let mut contract = contract_of(family);
        contract.measurement.timing = Timing::Instant;
        let check = check_contract(&contract, family, ProgramVersion::V1);
        assert!(!check.passed);
        assert!(
            !check
                .checks
                .iter()
                .find(|item| item.name == "timing_is_sequence")
                .unwrap()
                .passed
        );
    }

    #[test]
    fn parsing_rejects_missing_fields_and_prose() {
        assert!(parse_contract("没有 JSON").is_err());
        let mut wire = serde_json::to_value(contract_of(CFamily::LostUpdate)).unwrap();
        wire.as_object_mut().unwrap().remove("coverage");
        let error = parse_contract(&wire.to_string()).unwrap_err();
        assert!(error.to_string().contains("coverage"), "{error}");

        let complete = serde_json::to_value(contract_of(CFamily::LostUpdate)).unwrap();
        assert!(parse_contract(&complete.to_string()).is_ok());
    }

    #[test]
    fn the_observer_sees_what_the_confirmation_ordering_requires() {
        let family = CFamily::CancelAfterConfirm;
        let observer =
            Observer::build(family, contract_of(family), ProgramVersion::V1).expect("契约可装");
        assert_eq!(observer.points().len(), 3);

        let clean = run_case(
            family,
            &schedules(family)[0],
            Implementation::Correct,
            ProgramVersion::V1,
            Attachment::at_boundary(),
            false,
        );
        assert!(matches!(
            observer.observe(&clean).verdict,
            ObserverVerdict::Satisfied
        ));

        let faulty = run_case(
            family,
            &schedules(family)[0],
            Implementation::Uncoordinated,
            ProgramVersion::V1,
            Attachment::at_boundary(),
            false,
        );
        assert!(matches!(
            observer.observe(&faulty).verdict,
            ObserverVerdict::Violated
        ));

        // 篡改日志：记录器上看不见，观测器不能给出「命题成立」——它落在失败态，隐藏检查照样抓到。
        let tampered = run_case(
            family,
            &schedules(family)[0],
            Implementation::TamperedRecorder,
            ProgramVersion::V1,
            Attachment::at_boundary(),
            false,
        );
        match observer.observe(&tampered).verdict {
            ObserverVerdict::Failed(detail) => assert!(detail.contains("被绕过"), "{detail}"),
            other => panic!("篡改日志时观测器必须显式失败，而不是 `{other}`"),
        }
        assert!(matches!(tampered.hidden.verdict, ObserverVerdict::Violated));
    }

    #[test]
    fn the_observer_without_the_commit_counter_is_blind_to_a_bypass() {
        let family = CFamily::CancelAfterConfirm;
        let schedule = schedules(family).remove(0);
        let run = run_case(
            family,
            &schedule,
            Implementation::QuietBypass,
            ProgramVersion::V1,
            Attachment::at_boundary(),
            false,
        );

        let mut blind_contract = contract_of(family);
        blind_contract.measurement.site = "write_boundary + pre_write_pause".to_owned();
        let blind = Observer::build(family, blind_contract, ProgramVersion::V1).unwrap();
        assert!(matches!(
            blind.observe(&run).verdict,
            ObserverVerdict::Satisfied
        ));
        assert!(matches!(run.hidden.verdict, ObserverVerdict::Violated));

        let covered = Observer::build(family, contract_of(family), ProgramVersion::V1).unwrap();
        assert!(matches!(
            covered.observe(&run).verdict,
            ObserverVerdict::Failed(_)
        ));
    }

    #[test]
    fn the_four_way_check_passes_for_a_properly_built_observer() {
        for family in CFamily::ALL {
            let observer =
                Observer::build(family, contract_of(family), ProgramVersion::V1).unwrap();
            let check = independent_check(family, ProgramVersion::V1, &observer, 1_700_000_000_000);
            assert!(
                check.check.passed(),
                "{family}：{:?}",
                check
                    .check
                    .directions()
                    .map(|(direction, result)| (direction.to_string(), result.clone()))
            );
            assert!(
                check.observer_missed_the_bypass,
                "自建观测器本来就看不到绕过"
            );
            assert_eq!(check.check.hidden.observed, ObserverVerdict::Violated);
        }
    }

    #[test]
    fn a_self_certifying_check_is_rejected_by_the_enable_gate() {
        let family = CFamily::CancelAfterConfirm;
        let observer = Observer::build(family, contract_of(family), ProgramVersion::V1).unwrap();
        let proper = independent_check(family, ProgramVersion::V1, &observer, 1);
        let forged = self_certifying(&proper.check, &Verdict::satisfied("观测器说没问题"));

        let mut library = ObserverLibrary::default();
        let mut state = SemanticState::in_memory();
        let outcome = enable_with_independent_check(
            &mut library,
            &mut state,
            family,
            contract_of(family),
            proper.check,
            forged,
            1_700_000_000_000,
        )
        .expect("第二次启用应当成功");

        assert!(outcome.self_certifying_rejected, "{outcome:?}");
        assert!(
            outcome
                .self_certifying_error
                .as_deref()
                .is_some_and(|text| text.contains("独立隐藏检查")),
            "{outcome:?}"
        );
        assert_eq!(
            outcome.checks_on_record_after_rejection, 1,
            "被拒的那一条检验记录留在库里"
        );
        assert_eq!(
            library
                .enabled(&observer_id(family))
                .map(|(version, _)| version.0),
            outcome.version
        );
    }

    #[test]
    fn a_plan_that_reaches_for_the_hook_points_is_rejected() {
        let family = CFamily::CancelAfterConfirm;
        let plan = RawPlan {
            family: family.id().to_owned(),
            checks: vec![RawCheck {
                assertion: "status_equals".to_owned(),
                task: Some("T1".to_owned()),
                key: None,
                expected: "cancelled".to_owned(),
            }],
            note: "顺便读一下 recorder 里的写入".to_owned(),
        };
        let error = compile_plan(&plan, family).unwrap_err();
        assert!(error.to_string().contains("recorder"), "{error}");

        let unknown = RawPlan {
            family: family.id().to_owned(),
            checks: vec![RawCheck {
                assertion: "journal_equals".to_owned(),
                task: Some("T1".to_owned()),
                key: None,
                expected: "1".to_owned(),
            }],
            note: "门".to_owned(),
        };
        assert!(compile_plan(&unknown, family).is_err());

        let out_of_range = RawPlan {
            family: family.id().to_owned(),
            checks: vec![RawCheck {
                assertion: "value_equals".to_owned(),
                task: None,
                key: Some("somewhere:else".to_owned()),
                expected: "x".to_owned(),
            }],
            note: "门".to_owned(),
        };
        assert!(compile_plan(&out_of_range, family).is_err());
    }

    #[test]
    fn the_generated_tests_run_against_the_pre_existing_observables() {
        let family = CFamily::DuplicateAfterRestart;
        let plan = RawPlan {
            family: family.id().to_owned(),
            checks: vec![RawCheck {
                assertion: "intents_equals".to_owned(),
                task: Some("T1".to_owned()),
                key: None,
                expected: "1".to_owned(),
            }],
            note: "自报的意图数".to_owned(),
        };
        let checks = compile_plan(&plan, family).expect("计划可编译");

        let clean = run_case(
            family,
            &schedules(family)[0],
            Implementation::Correct,
            ProgramVersion::V1,
            Attachment::at_boundary(),
            false,
        );
        assert!(matches!(
            generated_verdict(&checks, &clean).verdict,
            ObserverVerdict::Satisfied
        ));

        let faulty = run_case(
            family,
            &schedules(family)[0],
            Implementation::Uncoordinated,
            ProgramVersion::V1,
            Attachment::at_boundary(),
            false,
        );
        assert!(
            matches!(
                generated_verdict(&checks, &faulty).verdict,
                ObserverVerdict::Satisfied
            ),
            "自报口径的断言看不见重复提交"
        );
        assert!(matches!(faulty.hidden.verdict, ObserverVerdict::Violated));
    }

    #[test]
    fn the_fixed_arm_can_be_wrong_in_both_directions_on_the_landmark_family() {
        let family = CFamily::CancelAfterConfirm;
        let schedules = schedules(family);
        let declaration = fixed_declaration(family);
        assert_eq!(
            declaration
                .iter()
                .filter(|item| item.weakened_substitution)
                .count(),
            1,
            "被禁的弱化替换必须在声明里标出来"
        );

        // 方向一：暂停之前的合法写入让它误报。
        let legal = run_case(
            family,
            &schedules[1],
            Implementation::Correct,
            ProgramVersion::V1,
            Attachment::at_boundary(),
            false,
        );
        assert!(matches!(
            fixed_verdict(family, &schedules[1], &legal).verdict,
            ObserverVerdict::Violated
        ));
        assert!(matches!(legal.hidden.verdict, ObserverVerdict::Satisfied));

        // 方向二：确认之后写入再被合法回收，它误放行。
        let masked = run_case(
            family,
            &schedules[4],
            Implementation::Uncoordinated,
            ProgramVersion::V1,
            Attachment::at_boundary(),
            false,
        );
        assert!(matches!(
            fixed_verdict(family, &schedules[4], &masked).verdict,
            ObserverVerdict::Satisfied
        ));
        assert!(matches!(masked.hidden.verdict, ObserverVerdict::Violated));
    }
}
