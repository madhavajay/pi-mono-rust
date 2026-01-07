use pi::ai::{
    get_model, stream, AbortController, AssistantMessageEvent, Context, Message, StreamOptions,
};
use pi::{UserContent, UserMessage};
use std::time::{SystemTime, UNIX_EPOCH};

// Source: packages/ai/test/tokens.test.ts

#[test]
fn should_include_token_stats_when_aborted_mid_stream() {
    let model = get_model("mock", "test-model");
    let context = Context {
        system_prompt: None,
        messages: vec![Message::User(UserMessage {
            content: UserContent::Text(
                "Write a long poem with 20 stanzas about the beauty of nature.".to_string(),
            ),
            timestamp: now_millis(),
        })],
        tools: None,
    };

    let controller = AbortController::new();
    let mut response = stream(
        &model,
        &context,
        StreamOptions {
            signal: Some(controller.signal()),
            reasoning_effort: None,
        },
    );

    let mut abort_fired = false;
    let mut text = String::new();
    for event in &mut response {
        if let AssistantMessageEvent::TextDelta { delta, .. }
        | AssistantMessageEvent::ThinkingDelta { delta, .. } = event
        {
            text.push_str(&delta);
            if !abort_fired && text.len() >= 50 {
                abort_fired = true;
                controller.abort();
            }
        }
    }

    let msg = response.result();
    assert_eq!(msg.stop_reason, "aborted");
    assert!(msg.usage.input > 0);
    assert!(msg.usage.output > 0);
    let total = msg.usage.input + msg.usage.output + msg.usage.cache_read + msg.usage.cache_write;
    assert_eq!(msg.usage.total_tokens, Some(total));
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}
