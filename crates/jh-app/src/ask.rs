//! `jev ask`：**一次**判断调用，出人读的读数与可落盘的 JSON。
//!
//! 定位：实验核心（`exp` / `run` / `compare` / `h3` / `h4` / `m4`）都在跑任务集，会话
//! （`chat`）在跑主循环；`ask` 是唯一「就问一句」的入口——用来手动核对某个判断器在某个
//! 问题上的答案与概率，或者给一条判断留一份可复现的记录。
//!
//! 边界（照实写在这儿，也让 `--help` 与报告自洽）：
//!
//! - **这是单问单答，不是门控**：本模块**不采纳**任何答案、不套门槛、不产生行动。
//!   概率、`confidence` 与 `margin` 都是原值照抄（[`AGENTS.md`]「概率优先，不提前塌缩成布尔」）。
//! - **不做任务集**：问题与选项由命令行给；`rule` / `replay` / `simulated-truth` 三个判定器
//!   依赖任务集里的规则表或隐藏真值，所以这里只支持 `jev` / `llm` / `rule`——`rule` 对没覆盖的
//!   问题会显式失败（`规则表没有覆盖问题 …`），这本身就是它该有的样子。
//! - **失败显式**：缺凭据、问题不合法、判定器不支持，一律报清楚并退 2，不退到别的判定器。

use std::collections::BTreeMap;
use std::time::Instant;

use jh_core::{
    ChoiceQuestion, Judge, JudgeRequest, JudgeResponse, JudgeSource, Question, QuestionId,
    QuestionSpec, QuestionVersion, SnapshotId, State,
};
use serde::Serialize;

use crate::error::Error;
use crate::judge::{JevJudge, JudgeName, LlmJudge, RuleJudge};

/// 问一句时用的问题 ID（也出现在报告与错误信息里）。
const QUESTION_ID: &str = "ask";
/// 不给 `--material` 时的占位材料：**如实说明没有材料**，而不是塞一段编出来的上下文。
const NO_MATERIAL: &str = "（本次判定没有附加材料）";

/// 一次 `ask` 的参数。
#[derive(Debug, Clone)]
pub struct AskOptions {
    /// 要判定什么（进 `ChoiceQuestion::instructions`）。
    pub question: String,
    /// 选项名（至少两个，顺序即上屏顺序）。
    pub labels: Vec<String>,
    /// 选项说明：`选项名 → 一句判据`（可空）。
    pub describes: BTreeMap<String, String>,
    /// 交给判定器的材料；`None` 表示没有附加材料。
    pub material: Option<String>,
    /// 判定器。
    pub judge: JudgeName,
    /// 运行标识（报告里用）。
    pub run: String,
}

/// `ask` 的读数（人读的上屏与 `--out` 的 JSON 共用同一份）。
#[derive(Debug, Clone, Serialize)]
pub struct AskReport {
    /// 运行标识（与 `--out` 报告同名）。
    pub run: String,
    /// 报告生成时刻。
    pub generated_at_unix_ms: u64,
    /// 复现用的命令行（材料只写占位，不把长文塞进报告）。
    pub command: String,
    /// 问题侧读数。
    pub question: QuestionReport,
    /// 判定器侧读数。
    pub judge: JudgeReport,
    /// 答案侧读数。
    pub answer: AnswerReport,
    /// token 用量。
    pub usage: UsageReport,
    /// 口径声明：这次 `ask` 做了什么、没做什么。
    pub notes: Vec<String>,
}

/// 问题侧读数。
#[derive(Debug, Clone, Serialize)]
pub struct QuestionReport {
    /// 问题 ID（`ask`）。
    pub id: String,
    /// 问题定义版本（`ask` 固定 1）。
    pub version: u32,
    /// 要判定什么。
    pub instructions: String,
    /// 选项名 → 说明（`null` 表示没写说明）。
    pub criteria: BTreeMap<String, Option<String>>,
    /// 原始材料；`null` = 没有附加材料。
    pub material: Option<String>,
    /// 材料序列化后的字节数。
    pub material_bytes: usize,
}

/// 判定器侧读数。
#[derive(Debug, Clone, Serialize)]
pub struct JudgeReport {
    /// CLI 上的名字（`jev` / `llm` / `rule`）。
    pub name: String,
    /// 来源标签：`real_api` / `llm` / `rule` / `replay` / `simulated_truth`。
    pub source: String,
    /// 执行者版本：真实通道是响应里的模型 ID。
    pub version: String,
    /// 通道侧请求 id；本地判定器为 `null`。
    pub request_id: Option<String>,
    /// 这次调用耗时。
    pub latency_ms: u64,
}

/// 答案侧读数：概率原值照抄。
#[derive(Debug, Clone, Serialize)]
pub struct AnswerReport {
    /// 概率最高的选项。
    pub choice: String,
    /// 选项 → 概率（原值）。
    pub probabilities: BTreeMap<String, f64>,
    /// 服务端给的 confidence（原值）。
    pub confidence: f64,
    /// 首选与次选的概率差；只有一个选项时等于首选自己的概率。
    pub margin: f64,
}

/// token 用量。
#[derive(Debug, Clone, Serialize)]
pub struct UsageReport {
    /// 上报 usage 的调用次数（0 或 1）。
    pub calls_with_usage: u32,
    /// 输入 token；未上报为 `null`。
    pub input_tokens: Option<u64>,
    /// 输出 token；未上报为 `null`。
    pub output_tokens: Option<u64>,
}

/// `answer` 不是二选一时把原始答案写到报告里（`score` / `noul` / `unknown`）。
fn describe_other(answer: &jh_core::Answer) -> String {
    match answer {
        jh_core::Answer::Choice(choice) => format!("二选一：{}", choice.choice),
        jh_core::Answer::Score(score) => {
            format!("有序标尺：{:.3}（{} 级）", score.score, score.legend.len())
        }
        jh_core::Answer::Noul(noul) => format!("Noul：{}", noul.noul),
    }
}
/// 校验问题与选项，装出一次判定请求（CLI 的 `ask` 与会话里的 `/jev` 共用）。
pub fn build_request(
    run: &str,
    question: &str,
    labels: &[String],
    describes: &BTreeMap<String, String>,
    material: Option<&str>,
) -> Result<(JudgeRequest, QuestionReport), Error> {
    if labels.len() < 2 {
        return Err(Error::TaskSet(format!(
            "选项至少要两个（当前 {} 个）：用法 `--label A --label B`",
            labels.len()
        )));
    }
    let mut criteria: BTreeMap<String, Option<String>> = BTreeMap::new();
    for label in labels {
        if label.trim().is_empty() {
            return Err(Error::TaskSet("选项名不能是空白".to_owned()));
        }
        if criteria.contains_key(label) {
            return Err(Error::TaskSet(format!("选项名重复：`{label}`")));
        }
        criteria.insert(label.clone(), None);
    }
    for (name, description) in describes {
        let Some(slot) = criteria.get_mut(name) else {
            return Err(Error::TaskSet(format!(
                "`--describe {name}=…` 指的是一个没有声明的选项；先加 `--label {name}`"
            )));
        };
        *slot = Some(description.clone());
    }

    let state = match material {
        Some(material) => State::Text(material.to_owned()),
        None => State::Text(NO_MATERIAL.to_owned()),
    };
    let material_bytes = serde_json::to_string(&state)?.len();
    let request = JudgeRequest {
        snapshot: SnapshotId::from(run),
        state,
        questions: BTreeMap::from([(
            QuestionId::from(QUESTION_ID),
            QuestionSpec {
                version: QuestionVersion(1),
                question: Question::Choice(ChoiceQuestion {
                    instructions: question.to_owned(),
                    criteria: criteria.clone(),
                }),
            },
        )]),
    };
    let report = QuestionReport {
        id: QUESTION_ID.to_owned(),
        version: 1,
        instructions: question.to_owned(),
        criteria,
        material: material.map(str::to_owned),
        material_bytes,
    };
    Ok((request, report))
}

/// 把一次判定响应汇成读数（CLI 与会话共用同一条出路）。
pub fn report(
    run: &str,
    command: String,
    judge_name: &str,
    response: JudgeResponse,
    latency_ms: u64,
    question: QuestionReport,
) -> Result<AskReport, Error> {
    let answer = response.answers.get(&QuestionId::from(QUESTION_ID));
    let Some(jh_core::Answer::Choice(choice)) = answer else {
        return Err(Error::TaskSet(format!(
            "判定器给的不是二选一答案（{}）：`ask` 只读二选一，换一个判定器或改问题",
            answer.map_or_else(|| "这次没有返回答案".to_owned(), describe_other)
        )));
    };
    let usage = response.usage.as_ref();
    Ok(AskReport {
        run: run.to_owned(),
        generated_at_unix_ms: crate::exp::unix_ms(),
        command,
        question,
        judge: JudgeReport {
            name: judge_name.to_owned(),
            source: source_tag(response.source).to_owned(),
            version: response.version.clone(),
            request_id: response.request_id.clone(),
            latency_ms,
        },
        answer: AnswerReport {
            choice: choice.choice.clone(),
            probabilities: choice.probabilities.clone(),
            confidence: choice.confidence,
            margin: margin_of(&choice.probabilities, &choice.choice),
        },
        usage: UsageReport {
            calls_with_usage: u32::from(usage.is_some()),
            input_tokens: usage.and_then(|usage| usage.input_tokens),
            output_tokens: usage.and_then(|usage| usage.output_tokens),
        },
        notes: vec![
            "单问单答：本次不采纳任何答案、不套门槛、不产生行动（概率与 confidence 是原值）"
                .to_owned(),
            "同一配置重跑读数可能不同：LLM / Jev 的采样有波动，单次读数不得当稳定值".to_owned(),
        ],
    })
}

/// 跑一次 `ask`：构造请求 → 调判定器 → 汇总读数。
pub async fn ask(options: AskOptions) -> Result<AskReport, Error> {
    let (request, question) = build_request(
        &options.run,
        &options.question,
        &options.labels,
        &options.describes,
        options.material.as_deref(),
    )?;
    let judge = build_judge(options.judge)?;
    let started = Instant::now();
    let response = judge.judge(&request).await?;
    let latency_ms = started.elapsed().as_millis() as u64;
    report(
        &options.run,
        command_line(&options),
        options.judge.cli_name(),
        response,
        latency_ms,
        question,
    )
}

/// `/jev` 的参数：`<判定什么> [--label A --label B] [--material 材料]`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JevArgs {
    /// 要判定什么。
    pub question: String,
    /// 选项名；不写 `--label` 时用门控那一对（`allow` / `deny`）。
    pub labels: Vec<String>,
    /// 材料；`None` = 没有附加材料。
    pub material: Option<String>,
}

/// 解析 `/jev` 的参数；错误是给人看的一句话（宿主直接上屏）。
pub fn parse_jev_args(args: &str) -> Result<JevArgs, String> {
    let mut labels = Vec::new();
    let mut material = None;
    let mut question = Vec::new();
    let mut rest = args.split_whitespace().peekable();
    while let Some(token) = rest.next() {
        match token {
            "--label" => {
                let Some(label) = rest.next() else {
                    return Err("`--label` 后面要跟一个选项名".to_owned());
                };
                labels.push(label.to_owned());
            }
            "--material" => {
                let Some(text) = rest.next() else {
                    return Err("`--material` 后面要跟一段材料".to_owned());
                };
                material = Some(text.to_owned());
            }
            other => question.push(other),
        }
    }
    if question.is_empty() {
        return Err(
            "要判定什么？用法 `/jev <判定什么> [--label A --label B] [--material 材料]`".to_owned(),
        );
    }
    if labels.is_empty() {
        labels = vec!["allow".to_owned(), "deny".to_owned()];
    }
    Ok(JevArgs {
        question: question.join(" "),
        labels,
        material,
    })
}

/// 人读的上屏（一行一条，报告里同名字段与它一一对应）。
pub fn lines(report: &AskReport) -> Vec<String> {
    let mut out = vec![
        format!(
            "问题（{}）：{}",
            report.question.id, report.question.instructions
        ),
        format!(
            "选项：{}",
            report
                .question
                .criteria
                .iter()
                .map(|(label, description)| match description {
                    Some(description) => format!("{label}（{description}）"),
                    None => label.clone(),
                })
                .collect::<Vec<_>>()
                .join(" / ")
        ),
        format!(
            "材料：{}",
            match &report.question.material {
                Some(_) => format!("{} 字节", report.question.material_bytes),
                None => "没有附加材料".to_owned(),
            }
        ),
        format!(
            "判定器：{}（source={}，{}，{}ms{}{}）",
            report.judge.name,
            report.judge.source,
            report.judge.version,
            report.judge.latency_ms,
            report
                .judge
                .request_id
                .as_ref()
                .map_or(String::new(), |id| format!("，request_id {id}")),
            report.usage.input_tokens.map_or(String::new(), |_| format!(
                "，usage 入 {} / 出 {}",
                report.usage.input_tokens.unwrap_or(0),
                report.usage.output_tokens.unwrap_or(0)
            )),
        ),
        format!(
            "答案：{}｜margin {:.4}｜confidence {:.4}",
            report.answer.choice, report.answer.margin, report.answer.confidence
        ),
        format!(
            "概率：{}",
            report
                .answer
                .probabilities
                .iter()
                .map(|(label, probability)| format!("{label} {probability:.4}"))
                .collect::<Vec<_>>()
                .join(" · ")
        ),
    ];
    out.extend(report.notes.iter().map(|note| format!("口径：{note}")));
    out
}

/// 首选与次选的概率差；只有一个选项时等于它自己的概率。
fn margin_of(probabilities: &BTreeMap<String, f64>, top: &str) -> f64 {
    let top_probability = probabilities.get(top).copied().unwrap_or(0.0);
    let runner_up = probabilities
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

/// `JudgeSource` 的稳定词表（与 `docs/session-log.md` §3 同一口径）。
fn source_tag(source: JudgeSource) -> &'static str {
    match source {
        JudgeSource::RealApi => "real_api",
        JudgeSource::Llm => "llm",
        JudgeSource::Rule => "rule",
        JudgeSource::Replay => "replay",
        JudgeSource::SimulatedTruth => "simulated_truth",
    }
}

/// 按名字构造判定器；`ask` 不支持依赖任务集的两个。
fn build_judge(name: JudgeName) -> Result<Box<dyn Judge>, Error> {
    Ok(match name {
        JudgeName::Jev => Box::new(JevJudge::from_env()?),
        JudgeName::Llm => Box::new(LlmJudge::from_env()?),
        JudgeName::Rule => Box::new(RuleJudge::new()),
        JudgeName::Replay | JudgeName::SimulatedTruth => {
            return Err(Error::TaskSet(format!(
                "`ask` 不支持判定器 `{}`：它要任务集里的规则表或隐藏真值，换 `jev` / `llm` / `rule`",
                name.cli_name()
            )));
        }
    })
}

/// 报告里的复现命令行（照抄用户给的参数，`--material` 只写字节数，避免把长文塞进报告）。
fn command_line(options: &AskOptions) -> String {
    let mut parts = vec![
        "jev ask".to_owned(),
        format!("--question {:?}", options.question),
        format!("--judge {}", options.judge.cli_name()),
    ];
    for label in &options.labels {
        parts.push(format!("--label {label}"));
    }
    for (name, description) in &options.describes {
        parts.push(format!("--describe {name}={description}"));
    }
    if options.material.is_some() {
        parts.push("--material <FILE|TEXT>".to_owned());
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(labels: &[&str]) -> AskOptions {
        AskOptions {
            question: "该不该继续？".to_owned(),
            labels: labels.iter().map(|label| (*label).to_owned()).collect(),
            describes: BTreeMap::new(),
            material: None,
            judge: JudgeName::Rule,
            run: "ask-test".to_owned(),
        }
    }

    #[tokio::test]
    async fn a_single_label_is_refused_with_the_usage_line() {
        let error = ask(options(&["allow"]))
            .await
            .expect_err("一个选项要显式失败");
        let text = error.to_string();
        assert!(text.contains("至少要两个"), "{text}");
        assert!(text.contains("--label"), "要把用法写出来：{text}");
    }

    #[tokio::test]
    async fn a_describe_without_its_label_is_refused() {
        let mut options = options(&["allow", "deny"]);
        options
            .describes
            .insert("maybe".to_owned(), "说不清".to_owned());
        let error = ask(options).await.expect_err("指到没声明的选项要失败");
        assert!(error.to_string().contains("maybe"), "{error}");
    }

    #[tokio::test]
    async fn a_task_set_only_judge_is_refused_by_name() {
        let mut options = options(&["allow", "deny"]);
        options.judge = JudgeName::SimulatedTruth;
        let error = ask(options)
            .await
            .expect_err("依赖任务集的判定器要显式拒绝");
        let text = error.to_string();
        assert!(text.contains("simulated-truth"), "{text}");
        assert!(text.contains("任务集"), "{text}");
    }

    /// 规则表没覆盖这个问题时，`rule` 会显式失败——`ask` 原样透传，不退到别的判定器。
    #[tokio::test]
    async fn an_uncovered_question_fails_loudly_instead_of_guessing() {
        let error = ask(options(&["allow", "deny"]))
            .await
            .expect_err("规则表没覆盖就要报错");
        assert!(error.to_string().contains("规则表没有覆盖"), "{error}");
    }

    #[test]
    fn jev_args_default_to_the_gate_pair_and_keep_the_question() {
        let args = parse_jev_args("这条命令该不该跑").expect("解析");
        assert_eq!(args.question, "这条命令该不该跑");
        assert_eq!(args.labels, vec!["allow".to_owned(), "deny".to_owned()]);
        assert_eq!(args.material, None);

        let args = parse_jev_args("这算不算破坏性 --label safe --label risky --material 材料在这")
            .expect("解析");
        assert_eq!(args.question, "这算不算破坏性");
        assert_eq!(args.labels, vec!["safe".to_owned(), "risky".to_owned()]);
        assert_eq!(args.material.as_deref(), Some("材料在这"));
    }

    #[test]
    fn jev_args_refuse_an_empty_question_or_a_dangling_flag() {
        let error = parse_jev_args("").expect_err("空参数要报错");
        assert!(error.contains("/jev"), "要把用法写出来：{error}");
        let error = parse_jev_args("问题 --label").expect_err("悬空的 --label 要报错");
        assert!(error.contains("--label"), "{error}");
        let error = parse_jev_args("问题 --material").expect_err("悬空的 --material 要报错");
        assert!(error.contains("--material"), "{error}");
    }

    #[test]
    fn margin_is_the_gap_to_the_runner_up() {
        let probabilities = BTreeMap::from([("allow".to_owned(), 0.9), ("deny".to_owned(), 0.1)]);
        assert!((margin_of(&probabilities, "allow") - 0.8).abs() < 1e-9);
        // 只有一个选项时等于它自己的概率
        let single = BTreeMap::from([("allow".to_owned(), 0.7)]);
        assert!((margin_of(&single, "allow") - 0.7).abs() < 1e-9);
    }

    #[test]
    fn the_command_line_keeps_the_reproduction_arguments() {
        let mut options = options(&["allow", "deny"]);
        options
            .describes
            .insert("allow".to_owned(), "证据齐".to_owned());
        options.material = Some("材料".to_owned());
        let line = command_line(&options);
        assert!(line.starts_with("jev ask --question"), "{line}");
        assert!(line.contains("--label allow"), "{line}");
        assert!(line.contains("--label deny"), "{line}");
        assert!(line.contains("--describe allow=证据齐"), "{line}");
        assert!(line.contains("--material <FILE|TEXT>"), "{line}");
    }
}
