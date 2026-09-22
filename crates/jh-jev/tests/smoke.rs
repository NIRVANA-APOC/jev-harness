//! 真调 smoke：默认 `#[ignore]`，只有显式加 `--ignored` 才会真的打网络。
//!
//! key 只从环境变量 `TYPESAFE_API_KEY` 读，测试里绝不打印 key。
//!
//! ```text
//! TYPESAFE_API_KEY=... cargo test -p jh-jev --test smoke -- --ignored --nocapture
//! ```

use std::collections::BTreeMap;

use jh_jev::{Client, NoulQuestion, Question, ScoreQuestion, State, SystemOneRequest};

#[tokio::test]
#[ignore = "需要 TYPESAFE_API_KEY 与真实网络：cargo test -p jh-jev --test smoke -- --ignored"]
async fn quickstart_live_round_trip() {
    let client = Client::from_env().expect("需要 TYPESAFE_API_KEY 环境变量");

    let mut questions = BTreeMap::new();
    questions.insert(
        "is_urgent".to_owned(),
        Question::from(NoulQuestion {
            instructions: "The message conveys urgency or time-sensitivity".into(),
            criteria: None,
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

    let request = SystemOneRequest::new(
        State::from(
            "Hi, I've been trying to connect my Stripe account for 3 days and the integration \
             keeps failing. I'm losing sales. Please help ASAP.",
        ),
        questions,
    );

    let response = client.system_one(&request).await.expect("真调失败");

    println!("base_url = {}", client.base_url());
    println!("model = {}", response.model);
    println!("request_id = {:?}", response.request_id);
    println!("usage = {:?}", response.usage);

    let noul = response
        .noul("is_urgent")
        .expect("is_urgent 应是 noul 答案");
    assert!((0.0..=1.0).contains(&noul.noul), "noul 应在 0–1 之间");

    let score = response
        .score("frustration")
        .expect("frustration 应是 score 答案");
    assert!(
        (0.0..=2.0).contains(&score.score),
        "score 应落在 3 级标尺内"
    );
    assert!((0.0..=1.0).contains(&score.confidence));
    assert_eq!(
        score.probabilities.keys().copied().collect::<Vec<_>>(),
        [0, 1, 2]
    );
}
