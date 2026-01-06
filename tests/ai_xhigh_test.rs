use pi::ai::{get_model, stream, Context, Message, StreamOptions};
use pi::{UserContent, UserMessage};
use std::time::{SystemTime, UNIX_EPOCH};

// Source: packages/ai/test/xhigh.test.ts

#[test]
fn should_work_with_openai_responses() {
    let model = get_model("openai", "gpt-5.1-codex-max");
    let context = make_context();
    let mut stream = stream(
        &model,
        &context,
        StreamOptions {
            signal: None,
            reasoning_effort: Some("xhigh".to_string()),
        },
    );
    let mut has_thinking = false;

    for event in &mut stream {
        if matches!(
            event,
            pi::ai::AssistantMessageEvent::ThinkingStart { .. }
                | pi::ai::AssistantMessageEvent::ThinkingDelta { .. }
        ) {
            has_thinking = true;
        }
    }

    let response = stream.result();
    assert_eq!(response.stop_reason, "stop");
    let has_text = response
        .content
        .iter()
        .any(|block| matches!(block, pi::ContentBlock::Text { .. }));
    assert!(has_text);
    assert!(
        has_thinking
            || response
                .content
                .iter()
                .any(|block| matches!(block, pi::ContentBlock::Thinking { .. }))
    );
}

#[test]
fn should_error_with_openai_responses_when_using_xhigh() {
    let model = get_model("openai", "gpt-5-mini");
    let context = make_context();
    let mut stream = stream(
        &model,
        &context,
        StreamOptions {
            signal: None,
            reasoning_effort: Some("xhigh".to_string()),
        },
    );

    for _ in &mut stream {}

    let response = stream.result();
    assert_eq!(response.stop_reason, "error");
    let error_message = response.error_message.unwrap_or_default();
    assert!(error_message.contains("xhigh"));
}

#[test]
fn should_error_with_openai_completions_when_using_xhigh() {
    let model = get_model("openai", "gpt-5-mini");
    let context = make_context();
    let mut stream = stream(
        &model,
        &context,
        StreamOptions {
            signal: None,
            reasoning_effort: Some("xhigh".to_string()),
        },
    );

    for _ in &mut stream {}

    let response = stream.result();
    assert_eq!(response.stop_reason, "error");
    let error_message = response.error_message.unwrap_or_default();
    assert!(error_message.contains("xhigh"));
}

fn make_context() -> Context {
    Context {
        system_prompt: None,
        messages: vec![Message::User(UserMessage {
            content: UserContent::Text("What is 10 + 32? Think step by step.".to_string()),
            timestamp: now_millis(),
        })],
        tools: None,
    }
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}
