//! 用官方 quickstart 的例子跑一遍 Jev 判定，两种模式：
//!
//! ```text
//! # 离线（默认）：用 fixtures/ 里的官方示例回放，不碰网络
//! cargo run -p jh-jev --example ask
//!
//! # 真调：需要 TYPESAFE_API_KEY（只从环境变量读，不会被打出来）
//! cargo run -p jh-jev --example ask -- --live
//! ```

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use jh_jev::{
    Answer, ChoiceQuestion, Client, Instructions, NoulQuestion, Question, ScoreQuestion, State,
    SystemOneRequest, SystemOneResponse,
};

const QUICKSTART_STATE: &str = "Hi, I've been trying to connect my Stripe account for 3 days and \
     the integration keeps failing. I'm losing sales. Please help ASAP.";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let request = quickstart_request();

    if std::env::args().any(|argument| argument == "--live") {
        ask_live(&request).await
    } else {
        ask_offline(&request)
    }
}

/// 官方 quickstart 的「三类原语同请求」示例。
fn quickstart_request() -> SystemOneRequest {
    let mut questions = BTreeMap::new();

    questions.insert(
        "department".to_owned(),
        Question::from(ChoiceQuestion {
            instructions: "Which team should handle this".into(),
            criteria: [
                ("billing", "Payment or subscription issues"),
                ("technical", "Bugs or integration problems"),
                ("sales", "Pricing or account questions"),
            ]
            .into_iter()
            .map(|(option, description)| (option.to_owned(), Some(Instructions::from(description))))
            .collect(),
        }),
    );
    questions.insert(
        "frustration".to_owned(),
        Question::from(ScoreQuestion {
            instructions: "How frustrated the customer appears".into(),
            criteria: vec![
                "Calm, just stating facts".into(),
                "Frustrated but civil".into(),
                "Very angry, strong language".into(),
            ],
        }),
    );
    questions.insert(
        "is_urgent".to_owned(),
        Question::from(NoulQuestion {
            instructions: "The message conveys urgency or time-sensitivity".into(),
            criteria: None,
        }),
    );

    // 与官方 fixture 对齐：fixture 里的请求发的是别名 `jev-latest`；
    // 生产代码一般用 `SystemOneRequest::new` 的默认值（pin 的 `jev-1.13.0`）。
    SystemOneRequest::new(State::from(QUICKSTART_STATE), questions).with_model("jev-latest")
}

fn ask_offline(request: &SystemOneRequest) -> Result<(), Box<dyn std::error::Error>> {
    println!("== 离线回放：官方 quickstart fixture（不发起网络请求）==\n");

    let expected: serde_json::Value = serde_json::from_str(&fixture("quickstart.request.json"))?;
    let built = serde_json::to_value(request)?;
    assert_eq!(built, expected, "本地构造的请求与官方 fixture 不一致");
    println!("本地构造的请求与 fixtures/quickstart.request.json 逐字段一致");

    let response: SystemOneResponse = serde_json::from_str(&fixture("quickstart.response.json"))?;
    print_answers(&response);
    println!("\n换成真调：cargo run -p jh-jev --example ask -- --live");
    Ok(())
}

async fn ask_live(request: &SystemOneRequest) -> Result<(), Box<dyn std::error::Error>> {
    println!("== 真调：POST /v1/systemone ==\n");

    let client = Client::from_env()?;
    println!("base_url = {}", client.base_url());
    println!("timeout = {:?}", client.timeout());

    let response = client.system_one(request).await?;
    println!("request_id = {:?}", response.request_id);
    print_answers(&response);
    Ok(())
}

fn print_answers(response: &SystemOneResponse) {
    println!("model = {}  usage = {:?}", response.model, response.usage);
    for (id, answer) in &response.answers {
        match answer {
            Answer::Choice(choice) => println!(
                "{id}: choice = {:?}, confidence = {:.2}, probabilities = {:?}",
                choice.choice,
                choice.confidence,
                sorted(choice.probabilities.iter().map(|(k, v)| (k.clone(), *v)))
            ),
            Answer::Score(score) => println!(
                "{id}: score = {:.2}, confidence = {:.2}, probabilities = {:?}",
                score.score,
                score.confidence,
                sorted(score.probabilities.iter().map(|(k, v)| (*k, *v)))
            ),
            Answer::Noul(noul) => println!("{id}: noul = {:.2}（无 confidence）", noul.noul),
            Answer::Unknown => println!("{id}: 未知类型，本 crate 未解析（响应仍完整）"),
        }
    }
}

fn sorted<K: Ord>(pairs: impl Iterator<Item = (K, f64)>) -> Vec<(K, f64)> {
    let mut pairs: Vec<(K, f64)> = pairs.collect();
    pairs.sort_by(|left, right| left.0.cmp(&right.0));
    pairs
}

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name);
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("读取 {} 失败：{error}", path.display()))
}
