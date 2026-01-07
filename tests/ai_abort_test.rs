use pi::ai::{
    complete, get_model, stream, AbortController, AssistantMessageEvent, Context, Message,
    StreamOptions,
};
use pi::{UserContent, UserMessage};
use std::time::{SystemTime, UNIX_EPOCH};

// Source: packages/ai/test/abort.test.ts

#[test]
fn should_abort_mid_stream() {
    let model = get_model("mock", "test-model");
    let mut context = Context {
        system_prompt: None,
        messages: vec![Message::User(UserMessage {
            content: UserContent::Text(
                "What is 15 + 27? Think step by step. Then list 50 first names.".to_string(),
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

    let mut text = String::new();
    for event in &mut response {
        match event {
            AssistantMessageEvent::TextDelta { delta, .. }
            | AssistantMessageEvent::ThinkingDelta { delta, .. } => {
                text.push_str(&delta);
                if text.len() >= 50 {
                    controller.abort();
                }
            }
            _ => {}
        }
    }

    let msg = response.result();
    assert_eq!(msg.stop_reason, "aborted");
    assert!(!msg.content.is_empty());

    context.messages.push(Message::Assistant(msg));
    context.messages.push(Message::User(UserMessage {
        content: UserContent::Text("Please continue, but only generate 5 names.".to_string()),
        timestamp: now_millis(),
    }));

    let follow_up = complete(
        &model,
        &context,
        StreamOptions {
            signal: None,
            reasoning_effort: None,
        },
    );
    assert_eq!(follow_up.stop_reason, "stop");
    assert!(!follow_up.content.is_empty());
}

#[test]
fn should_handle_immediate_abort() {
    let model = get_model("mock", "test-model");
    let controller = AbortController::new();
    controller.abort();

    let context = Context {
        system_prompt: None,
        messages: vec![Message::User(UserMessage {
            content: UserContent::Text("Hello".to_string()),
            timestamp: now_millis(),
        })],
        tools: None,
    };

    let response = complete(
        &model,
        &context,
        StreamOptions {
            signal: Some(controller.signal()),
            reasoning_effort: None,
        },
    );
    assert_eq!(response.stop_reason, "aborted");
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}
