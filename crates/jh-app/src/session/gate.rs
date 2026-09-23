//! 门控：确定性风险层 + Jev 类型化判定 + 防抖。
//!
//! 设计来源：`docs/tui-design.md` §2.4（决策分层可见）、§2.6（门控带理由，拒绝要有出路）与
//! `AGENTS.md`「决策分层」——能让确定性代码判断的，不让 Jev 判断。
//!
//! 三级在这里的样子：
//!
//! | 层 | 对象 | 产出 |
//! | --- | --- | --- |
//! | 确定性规则 | [`assess`] | 命令的风险类别与理由（不裁决；只读工具到此为止，不问 Jev） |
//! | Jev | [`GateCall`] + [`Debounce`] | 二选一的概率分布、confidence、margin 与采纳结论 |
//! | 人工 | [`crate::session::agent`] 的审批卡 | 最终裁决（允许一次 / 本次会话总是允许 / 拒绝） |
//!
//! 概率是一等数据（`AGENTS.md`「概率优先」）：[`GateDecision`] 原样带着 [`Answer`]，
//! 采纳与否由 [`classify`] 按**声明的**门槛（[`MARGIN_FLOOR`] / [`CONFIDENCE_FLOOR`]，写进
//! 判断记录的 `thresholds`）决定，而不是把概率塌缩成一个布尔后丢掉原值。
//!
//! 防抖：同一份材料（同一条命令、参数一字不差）在窗口内只问一次——模型在被拒绝或失败之后
//! 重试同一条命令是最常见的重复形态，窗口内直接复用上一次判定，把「问了几次」如实记在
//! [`Debounce::asks`] / [`Debounce::reuses`] 上。

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use jh_core::{
    Answer, ChoiceAnswer, ContentHash, Judge, JudgeRequest, JudgeResponse, JudgeSource,
    JudgmentRecord, Question, QuestionId, QuestionSpec, QuestionVersion,
};
use jh_tui::blocks::{Choice, DecisionTrace};

/// 门控问题标识：这条 shell 命令是否可以执行（与 `docs/tui-design.md` §5.1、§5.3 的示例同名）。
pub const QUESTION_ID: &str = "shell_command_allowed";
/// 选项：允许执行。
pub const OPTION_ALLOW: &str = "usr:allow";
/// 选项：拒绝执行。
pub const OPTION_DENY: &str = "usr:deny";
/// 采纳门槛：首选与次选的概率差下限。
pub const MARGIN_FLOOR: f64 = 0.15;
/// 采纳门槛：confidence 下限。
pub const CONFIDENCE_FLOOR: f64 = 0.5;
/// 防抖窗口缺省值：同一份材料在 30 秒内只问一次。
pub const DEFAULT_WINDOW: Duration = Duration::from_secs(30);

/// 门控问题的定义（版本 1）：二选一，选项名就是采纳时上屏的那个标识。
pub fn question() -> QuestionSpec {
    let criteria: BTreeMap<String, Option<String>> = [
        (OPTION_ALLOW, Some("允许执行".to_owned())),
        (OPTION_DENY, Some("拒绝执行".to_owned())),
    ]
    .into_iter()
    .map(|(name, description)| (name.to_owned(), description))
    .collect();
    QuestionSpec {
        version: QuestionVersion(1),
        question: Question::Choice(jh_core::ChoiceQuestion {
            instructions: "这条 shell 命令应当在用户授权后执行吗？\
                           给出两个选项的概率分布；不确定就给接近的概率。"
                .to_owned(),
            criteria,
        }),
    }
}

/// 选项的展示文案（判定块与卡片理由共用一处）。
pub fn option_label(option: &str) -> &str {
    match option {
        OPTION_ALLOW => "允许执行",
        OPTION_DENY => "拒绝执行",
        other => other,
    }
}

/// 确定性风险层：命令的粗分类。**只分类与写理由，不裁决**。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Risk {
    /// 只读命令（`git status` 一类）：不会改动工作区。
    ReadOnly,
    /// 会改动工作区的命令。
    Mutating,
    /// 命中破坏性标记的命令。
    Destructive,
}

impl Risk {
    /// 上屏用的短标签。
    pub fn label(self) -> &'static str {
        match self {
            Risk::ReadOnly => "只读",
            Risk::Mutating => "会改动工作区",
            Risk::Destructive => "破坏性",
        }
    }
}

/// 破坏性标记：命中任意一条就是 [`Risk::Destructive`]。顺序无关，只看有没有。
const DESTRUCTIVE_MARKERS: [&str; 22] = [
    "rm -rf",
    "rm -r",
    "sudo ",
    "mkfs",
    "dd if=",
    "git push --force",
    "git push -f",
    "git reset --hard",
    "git clean -f",
    "git checkout --",
    "git branch -D",
    "git branch -d",
    "chmod -R",
    "chown -R",
    "shutdown",
    "reboot",
    "kill -9",
    "cargo publish",
    "npm publish",
    "curl | sh",
    "curl -s | sh",
    "wget | sh",
];

/// 只读标记：命令以其中之一开头（含空白归一）就是 [`Risk::ReadOnly`]。
const READ_ONLY_PREFIXES: [&str; 27] = [
    "ls",
    "cat ",
    "head",
    "tail",
    "wc",
    "grep",
    "rg ",
    "find",
    "fd ",
    "stat ",
    "file ",
    "pwd",
    "whoami",
    "echo ",
    "date",
    "uname",
    "hostname",
    "which ",
    "git status",
    "git diff",
    "git log",
    "git show",
    "git branch",
    "git remote",
    "git rev-parse",
    "git stash list",
    "cargo tree",
];

/// 分类一条命令：先看破坏性标记，再看只读前缀，都不命中就是「会改动工作区」。
pub fn assess(command: &str) -> Risk {
    let normalized = command.trim();
    if DESTRUCTIVE_MARKERS
        .iter()
        .any(|marker| normalized.contains(marker))
    {
        return Risk::Destructive;
    }
    if READ_ONLY_PREFIXES
        .iter()
        .any(|prefix| normalized == prefix.trim_end() || normalized.starts_with(prefix))
    {
        return Risk::ReadOnly;
    }
    Risk::Mutating
}

/// 门控结论：采纳了哪个选项。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// 采纳「允许执行」：照常走人工审批卡。
    Allow,
    /// 采纳「拒绝执行」且过了门槛：不上卡，直接拒绝并写明出路。
    Refuse,
    /// 判不了（判定器不可用、分布太接近、confidence 太低、答案原语不符）：交人工裁决。
    Ask,
}

/// 采纳门槛：写进判断记录的 `thresholds`，同时也是上屏理由的一部分。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Thresholds {
    /// 概率差下限。
    pub margin: f64,
    /// 置信度下限。
    pub confidence: f64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Thresholds {
            margin: MARGIN_FLOOR,
            confidence: CONFIDENCE_FLOOR,
        }
    }
}

/// 一次门控判定的结果。
#[derive(Debug, Clone, PartialEq)]
pub struct GateDecision {
    /// 防抖键（工具名 + 参数摘要）。
    pub key: String,
    /// 采纳结论。
    pub verdict: Verdict,
    /// 门控问题的答案（概率原值；判定失败时为 `None`）。
    pub answer: Option<Answer>,
    /// 首选与次选的概率差。
    pub margin: Option<f64>,
    /// 服务端给出的 confidence。
    pub confidence: Option<f64>,
    /// 该通道声明的来源标签（失败也标明是谁失败）。
    pub source: JudgeSource,
    /// 执行者版本（模型 ID 或规则版本串）。
    pub executor: Option<String>,
    /// 通道侧请求 id。
    pub request_id: Option<String>,
    /// 本次判定耗时，毫秒。
    pub latency_ms: u64,
    /// 判定失败时的错误原文；成功为 `None`。
    pub failure: Option<String>,
    /// 门控理由（阶段行与审批卡正文都用它）。
    pub reason: String,
    /// 是否来自防抖复用（没有发新调用）。
    pub reused: bool,
    /// 判断记录（写进会话的语义状态，也是研究视图判断详情的数据源）。
    pub record: Option<JudgmentRecord>,
}

impl GateDecision {
    /// 采纳的是「允许执行」。
    pub fn allowed(&self) -> bool {
        self.verdict == Verdict::Allow
    }

    /// 采纳的是「拒绝执行」。
    pub fn refused(&self) -> bool {
        self.verdict == Verdict::Refuse
    }

    /// 拒绝了之后的出路：说明这是谁判的、怎么覆盖、模型还能做什么。
    pub fn refuse_text(&self) -> String {
        format!(
            "门控拒绝：{}（{}）。可换判定器重启（`--judge`）或自己在终端执行；\
             请让模型换一个做法，而不是重试同一条命令。",
            self.reason,
            self.executor.as_deref().unwrap_or("判定器未报版本")
        )
    }
}

/// 一次判定调用的材料：已经从会话状态取好，因此判定可以独立于会话异步执行。
pub struct GateCall {
    judge: Arc<dyn Judge>,
    request: Box<JudgeRequest>,
    key: String,
    source: JudgeSource,
    thresholds: Thresholds,
    started: Instant,
}

impl GateCall {
    /// 组装一次判定调用。
    pub fn new(
        judge: Arc<dyn Judge>,
        request: JudgeRequest,
        key: impl Into<String>,
        source: JudgeSource,
        thresholds: Thresholds,
    ) -> Self {
        GateCall {
            judge,
            request: Box::new(request),
            key: key.into(),
            source,
            thresholds,
            started: Instant::now(),
        }
    }

    /// 防抖键。
    pub fn key(&self) -> &str {
        &self.key
    }

    /// 发一次判定并归类。失败不抛给调用方：它同样是「交人工裁决」的一种理由。
    pub async fn ask(self) -> GateDecision {
        let GateCall {
            judge,
            request,
            key,
            source,
            thresholds,
            started,
        } = self;
        match judge.judge(&request).await {
            Ok(response) => {
                let latency_ms = started.elapsed().as_millis() as u64;
                settle(key, source, &request, response, latency_ms, thresholds)
            }
            Err(error) => {
                let reason = crate::session::call_reason(&error);
                GateDecision {
                    key,
                    verdict: Verdict::Ask,
                    answer: None,
                    margin: None,
                    confidence: None,
                    source,
                    executor: None,
                    request_id: None,
                    latency_ms: started.elapsed().as_millis() as u64,
                    failure: Some(reason.clone()),
                    // 「判定不可用」不是「拒绝」：这句文案与 `Verdict::Refuse` 的
                    // `refuse_text()` 在界面上必须一眼分得开（tower 的评审要求）。
                    reason: format!("判定不可用（{reason}）；交人工裁决"),
                    reused: false,
                    record: None,
                }
            }
        }
    }
}

/// 把一次成功的通道响应结算成判定：先落判断记录（不合契约就退回「交人工」），
/// 再按声明的门槛分类，最后给出上屏理由。
fn settle(
    key: String,
    source: JudgeSource,
    request: &JudgeRequest,
    response: JudgeResponse,
    latency_ms: u64,
    thresholds: Thresholds,
) -> GateDecision {
    let answer = response
        .answers
        .get(&QuestionId::from(QUESTION_ID))
        .cloned();
    let executor = Some(response.version.clone());
    let request_id = response.request_id.clone();

    let mut record = match JudgmentRecord::from_response(
        request,
        &response,
        Duration::from_millis(latency_ms),
    ) {
        Ok(record) => record,
        Err(error) => {
            return GateDecision {
                key,
                verdict: Verdict::Ask,
                answer,
                margin: None,
                confidence: None,
                source,
                executor,
                request_id,
                latency_ms,
                failure: Some(error.to_string()),
                reason: format!("响应不合契约（{error}）；交人工裁决"),
                reused: false,
                record: None,
            };
        }
    };
    // 采纳门槛是这次判定的口径，随记录一起落盘（`Judgment::thresholds`）。
    for judgment in record.judgments.values_mut() {
        judgment
            .thresholds
            .insert("margin".to_owned(), thresholds.margin);
        judgment
            .thresholds
            .insert("confidence".to_owned(), thresholds.confidence);
    }

    let classified = classify(answer.as_ref(), thresholds);
    GateDecision {
        key,
        verdict: classified.verdict,
        answer,
        margin: classified.margin,
        confidence: classified.confidence,
        source,
        executor,
        request_id,
        latency_ms,
        failure: None,
        reason: classified.reason,
        reused: false,
        record: Some(record),
    }
}

/// 分类结果（`reason` 已经是可以直接上屏的那一行）。
struct Classified {
    verdict: Verdict,
    margin: Option<f64>,
    confidence: Option<f64>,
    reason: String,
}

/// 按门槛分类一份答案：只有「首选允许」「首选拒绝且过了门槛」两类才采纳，其余一律交人工。
fn classify(answer: Option<&Answer>, thresholds: Thresholds) -> Classified {
    let Some(Answer::Choice(choice)) = answer else {
        let what = match answer {
            None => "这次调用没有返回该问题的答案".to_owned(),
            Some(other) => format!("该问题的答案不是二选一（{other:?}）"),
        };
        return Classified {
            verdict: Verdict::Ask,
            margin: None,
            confidence: None,
            reason: format!("{what}；交人工裁决"),
        };
    };
    let (top, top_probability) = match top(choice) {
        Some(top) => top,
        None => {
            return Classified {
                verdict: Verdict::Ask,
                margin: None,
                confidence: Some(choice.confidence),
                reason: "分布是空的；交人工裁决".to_owned(),
            };
        }
    };
    let margin = margin(choice, &top);
    let confidence = choice.confidence;
    let verdict = match top.as_str() {
        OPTION_ALLOW => Verdict::Allow,
        OPTION_DENY if margin >= thresholds.margin && confidence >= thresholds.confidence => {
            Verdict::Refuse
        }
        OPTION_DENY => Verdict::Ask,
        _ => Verdict::Ask,
    };
    let lean = match verdict {
        Verdict::Allow => "倾向允许",
        Verdict::Refuse => "倾向拒绝",
        Verdict::Ask if top == OPTION_DENY => "差距或置信度不足，交人工裁决",
        Verdict::Ask => "分布不清，交人工裁决",
    };
    Classified {
        verdict,
        margin: Some(margin),
        confidence: Some(confidence),
        reason: format!(
            "采纳 {top} {top_probability:.2} · margin {margin:.2} · confidence {confidence:.2} · {lean}"
        ),
    }
}

/// 首选选项（并列取字典序最小，与 `judge/llm.rs` 的读法同口径）。
fn top(choice: &ChoiceAnswer) -> Option<(String, f64)> {
    choice
        .probabilities
        .iter()
        .max_by(|left, right| {
            left.1
                .partial_cmp(right.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                // `max_by` 在相等时取后者，这里反转比较让相等时留下字典序更小的那个。
                .then_with(|| right.0.cmp(left.0))
        })
        .map(|(label, probability)| (label.clone(), *probability))
}

/// 首选与次选的概率差（只有一个选项时为该选项本身的概率）。
fn margin(choice: &ChoiceAnswer, top: &str) -> f64 {
    let top_probability = choice.probabilities.get(top).copied().unwrap_or(0.0);
    let runner_up = choice
        .probabilities
        .iter()
        .filter(|(label, _)| label.as_str() != top)
        .map(|(_, probability)| *probability)
        .fold(f64::NEG_INFINITY, f64::max);
    if runner_up.is_finite() {
        top_probability - runner_up
    } else {
        top_probability
    }
}

/// 组装判定轨迹：状态行、判定块与研究视图共用它把概率画到屏上。
pub fn trace(decision: &GateDecision, prompt: impl Into<String>) -> Option<DecisionTrace> {
    let Some(Answer::Choice(choice)) = &decision.answer else {
        return None;
    };
    let choices: Vec<Choice> = choice
        .probabilities
        .iter()
        .map(|(label, probability)| {
            Choice::new(
                label.clone(),
                option_label(label).to_owned(),
                *probability as f32,
            )
        })
        .collect();
    let chosen = decision
        .answer
        .as_ref()
        .and_then(chosen_option)
        .and_then(|label| choices.iter().position(|choice| choice.id == *label));
    let mut trace = DecisionTrace::new(QUESTION_ID, choices)
        .with_prompt(prompt)
        .with_confidence(decision.confidence.unwrap_or(0.0) as f32);
    if let Some(chosen) = chosen {
        trace = trace.with_chosen(chosen);
    }
    Some(trace)
}

/// 答案里被采纳的选项名（`Answer::Choice` 的 `choice` 字段）。
fn chosen_option(answer: &Answer) -> Option<&String> {
    match answer {
        Answer::Choice(choice) => Some(&choice.choice),
        _ => None,
    }
}

/// 防抖：同一份材料在窗口内只问一次判定。
#[derive(Debug, Clone)]
pub struct Debounce {
    window: Duration,
    last: Option<(String, Instant, Box<GateDecision>)>,
}

impl Debounce {
    /// 建防抖缓存；窗口为 0 表示不防抖。
    pub fn new(window: Duration) -> Self {
        Debounce { window, last: None }
    }

    /// 窗口内同一材料的判定直接复用（返回的那份 `reused = true`）。
    pub fn hit(&mut self, key: &str, now: Instant) -> Option<GateDecision> {
        let (last_key, at, decision) = self.last.as_ref()?;
        if last_key != key || now.duration_since(*at) >= self.window {
            return None;
        }
        let mut reused = (**decision).clone();
        reused.reused = true;
        Some(reused)
    }

    /// 记下一次真的判定。
    pub fn record(&mut self, decision: GateDecision, now: Instant) {
        self.last = Some((decision.key.clone(), now, Box::new(decision)));
    }
}

/// 防抖键：工具名 + 参数内容的稳定摘要。
pub fn material_key(name: &str, arguments: &str) -> String {
    format!("{name}:{}", ContentHash::of_bytes(arguments.as_bytes()))
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;

    use super::*;

    fn request(command: &str) -> JudgeRequest {
        JudgeRequest {
            snapshot: jh_core::SnapshotId("test".to_owned()),
            state: jh_core::State::Object(BTreeMap::from([(
                "shell.command".to_owned(),
                serde_json::json!({"command": command}),
            )])),
            questions: BTreeMap::from([(QuestionId::from(QUESTION_ID), question())]),
        }
    }

    /// 桩判定器：给一份固定的分布（或一个固定失败）。
    struct Stub {
        probabilities: Vec<(&'static str, f64)>,
        confidence: f64,
        failure: Option<String>,
    }

    impl Stub {
        fn allows() -> Arc<Self> {
            Arc::new(Stub {
                probabilities: vec![(OPTION_ALLOW, 0.9), (OPTION_DENY, 0.1)],
                confidence: 0.8,
                failure: None,
            })
        }

        fn denies() -> Arc<Self> {
            Arc::new(Stub {
                probabilities: vec![(OPTION_ALLOW, 0.05), (OPTION_DENY, 0.95)],
                confidence: 0.9,
                failure: None,
            })
        }

        fn toss_up() -> Arc<Self> {
            Arc::new(Stub {
                probabilities: vec![(OPTION_ALLOW, 0.52), (OPTION_DENY, 0.48)],
                confidence: 0.3,
                failure: None,
            })
        }

        /// 倾向拒绝但没到门槛（差距与置信度都不够）：交人工。
        fn hesitant_deny() -> Arc<Self> {
            Arc::new(Stub {
                probabilities: vec![(OPTION_ALLOW, 0.45), (OPTION_DENY, 0.55)],
                confidence: 0.3,
                failure: None,
            })
        }

        fn broken() -> Arc<Self> {
            Arc::new(Stub {
                probabilities: Vec::new(),
                confidence: 0.0,
                failure: Some("通道不可用".to_owned()),
            })
        }
    }

    impl Judge for Stub {
        fn judge<'a>(
            &'a self,
            _request: &'a JudgeRequest,
        ) -> Pin<Box<dyn Future<Output = Result<JudgeResponse, jh_core::Error>> + Send + 'a>>
        {
            Box::pin(async move {
                if let Some(failure) = &self.failure {
                    return Err(jh_core::Error::Unavailable(failure.clone()));
                }
                let probabilities: BTreeMap<String, f64> = self
                    .probabilities
                    .iter()
                    .map(|(label, probability)| ((*label).to_owned(), *probability))
                    .collect();
                let choice = self
                    .probabilities
                    .iter()
                    .max_by(|left, right| left.1.partial_cmp(&right.1).expect("非 NaN"))
                    .map(|(label, _)| (*label).to_owned())
                    .expect("桩至少要给一个选项");
                Ok(JudgeResponse {
                    source: JudgeSource::RealApi,
                    version: "stub-1".to_owned(),
                    answers: BTreeMap::from([(
                        QuestionId::from(QUESTION_ID),
                        Answer::Choice(ChoiceAnswer {
                            choice,
                            probabilities,
                            confidence: self.confidence,
                        }),
                    )]),
                    usage: None,
                    request_id: Some("req-1".to_owned()),
                })
            })
        }
    }

    fn call(judge: Arc<dyn Judge>, command: &str) -> GateCall {
        GateCall::new(
            judge,
            request(command),
            material_key("bash", command),
            JudgeSource::RealApi,
            Thresholds::default(),
        )
    }

    #[test]
    fn risk_classification_has_three_marks() {
        assert_eq!(assess("git status --short"), Risk::ReadOnly);
        assert_eq!(assess("cargo build"), Risk::Mutating);
        assert_eq!(assess("rm -rf build"), Risk::Destructive);
        assert_eq!(assess("git push --force origin main"), Risk::Destructive);
        // 破坏性标记优先于只读前缀。
        assert_eq!(assess("git branch -D old"), Risk::Destructive);
    }

    #[test]
    fn a_confident_allow_is_adopted_with_its_probabilities() {
        let decision =
            crate::session::test_support::block_on(call(Stub::allows(), "cargo build").ask());
        assert_eq!(decision.verdict, Verdict::Allow);
        assert!(decision.allowed() && !decision.refused());
        assert_eq!(decision.executor.as_deref(), Some("stub-1"));
        assert_eq!(decision.request_id.as_deref(), Some("req-1"));
        assert!(!decision.reused);
        assert!(
            decision.reason.contains("采纳 usr:allow 0.90"),
            "{}",
            decision.reason
        );
        assert!(
            decision.reason.contains("margin 0.80"),
            "{}",
            decision.reason
        );
        assert!(
            decision.reason.contains("confidence 0.80"),
            "{}",
            decision.reason
        );
        // 概率作为一等数据留在判定里，且判断记录带上了采纳门槛。
        let record = decision.record.expect("成功判定应有记录");
        let judgment = record
            .judgments
            .get(&QuestionId::from(QUESTION_ID))
            .expect("该问题有记录");
        assert_eq!(judgment.thresholds.get("margin"), Some(&MARGIN_FLOOR));
        assert_eq!(
            judgment.thresholds.get("confidence"),
            Some(&CONFIDENCE_FLOOR)
        );
    }

    #[test]
    fn a_confident_deny_refuses_and_a_coin_flip_asks_for_a_human() {
        let refused =
            crate::session::test_support::block_on(call(Stub::denies(), "rm -rf /").ask());
        assert_eq!(refused.verdict, Verdict::Refuse);
        assert!(
            refused.refuse_text().contains("门控拒绝"),
            "{}",
            refused.refuse_text()
        );
        assert!(
            refused.refuse_text().contains("换一个做法"),
            "{}",
            refused.refuse_text()
        );

        // 分布太接近：首选是允许，但差距只有 0.04——照旧上卡，理由里带着这两个读数。
        let toss =
            crate::session::test_support::block_on(call(Stub::toss_up(), "cargo build").ask());
        assert_eq!(toss.verdict, Verdict::Allow);
        assert!(toss.reason.contains("margin 0.04"), "{}", toss.reason);

        // 首选是拒绝但 confidence 没过门槛：不采纳，交人工。
        let asked = crate::session::test_support::block_on(
            call(Stub::hesitant_deny(), "cargo build").ask(),
        );
        assert_eq!(asked.verdict, Verdict::Ask);
        assert!(asked.reason.contains("交人工裁决"), "{}", asked.reason);
    }

    #[test]
    fn a_broken_judge_asks_a_human_instead_of_pretending() {
        let decision =
            crate::session::test_support::block_on(call(Stub::broken(), "cargo build").ask());
        assert_eq!(decision.verdict, Verdict::Ask);
        assert!(decision.answer.is_none());
        assert!(decision.record.is_none());
        assert!(
            decision
                .failure
                .as_deref()
                .unwrap_or_default()
                .contains("通道不可用"),
            "{:?}",
            decision.failure
        );
        assert!(
            decision.reason.contains("判定不可用"),
            "{}",
            decision.reason
        );
    }

    #[test]
    fn debounce_reuses_the_same_material_inside_the_window() {
        let mut debounce = Debounce::new(Duration::from_secs(30));
        let start = Instant::now();
        let key = material_key("bash", "cargo build");

        let first =
            crate::session::test_support::block_on(call(Stub::allows(), "cargo build").ask());
        assert!(!first.reused, "第一次是真的发出去的判定");
        debounce.record(first, start);

        // 窗口内同一材料：不再发调用，直接把上一次的判定标成复用还回来。
        let reused = debounce
            .hit(&key, start + Duration::from_secs(5))
            .expect("应命中");
        assert!(reused.reused);
        assert_eq!(reused.verdict, Verdict::Allow);
        assert_eq!(
            reused.reason,
            "采纳 usr:allow 0.90 · margin 0.80 · confidence 0.80 · 倾向允许"
        );

        // 窗口外、或另一份材料：不命中。
        assert!(
            debounce
                .hit(&key, start + Duration::from_secs(31))
                .is_none()
        );
        assert!(
            debounce
                .hit(&material_key("bash", "cargo test"), start)
                .is_none()
        );
    }

    /// 真调一遍门控问题（需要 `TYPESAFE_API_KEY`，默认忽略）：
    /// `cargo test -p jh-app --lib -- --ignored session::gate::the_gate_question_is_accepted`
    ///
    /// 只验证一件事：这份问题定义能被服务端接受，回来的分布与 `confidence` 按线上口径可读。
    /// 它不写产物、不改状态——会话的真调产物落在 `var/`（`chat` 运行时）。
    #[tokio::test]
    #[ignore = "真调：需要 TYPESAFE_API_KEY"]
    async fn the_gate_question_is_accepted_by_the_real_service() {
        let judge = crate::judge::JevJudge::from_env().expect("TYPESAFE_API_KEY 应在环境里");
        let request = JudgeRequest {
            snapshot: jh_core::SnapshotId::from("smoke"),
            state: jh_core::State::Object(BTreeMap::from([
                (
                    "shell.command".to_owned(),
                    serde_json::json!({
                        "command": "git status --short",
                        "cwd": "/tmp/chat-demo",
                        "risk": "只读",
                    }),
                ),
                (
                    "session.ask".to_owned(),
                    serde_json::json!({"ask": "帮我看看工作区有没有未提交的改动"}),
                ),
            ])),
            questions: BTreeMap::from([(QuestionId::from(QUESTION_ID), question())]),
        };
        let decision = GateCall::new(
            Arc::new(judge),
            request,
            material_key("bash", "git status --short"),
            JudgeSource::RealApi,
            Thresholds::default(),
        )
        .ask()
        .await;

        assert!(decision.failure.is_none(), "{:?}", decision.failure);
        assert!(decision.record.is_some(), "真调应给出判断记录");
        assert!(decision.answer.is_some(), "真调应给出答案");
        eprintln!(
            "门控真调读数：{}（{} ms，执行者 {}）",
            decision.reason,
            decision.latency_ms,
            decision.executor.as_deref().unwrap_or("—")
        );
    }

    #[test]
    fn the_trace_keeps_every_probability_and_marks_the_adopted_option() {
        let decision =
            crate::session::test_support::block_on(call(Stub::allows(), "cargo build").ask());
        let trace = trace(&decision, "是否允许执行 `cargo build`？").expect("二选一应能画轨迹");
        assert_eq!(trace.question, QUESTION_ID);
        assert_eq!(trace.choices.len(), 2);
        assert_eq!(trace.chosen, Some(0), "采纳的是 usr:allow");
        assert!(
            trace.summary("·").contains("usr:allow 0.90"),
            "{}",
            trace.summary("·")
        );
    }
}
