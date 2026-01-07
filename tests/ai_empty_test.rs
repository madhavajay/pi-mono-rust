use pi::ai::{complete, get_model, Context, Message, StreamOptions};
use pi::{AssistantMessage, Cost, Usage, UserContent, UserMessage};
use std::time::{SystemTime, UNIX_EPOCH};

// Source: packages/ai/test/empty.test.ts

#[test]
fn should_handle_empty_content_array() {
    let model = get_model("mock", "test-model");
    let context = Context {
        system_prompt: None,
        messages: vec![Message::User(UserMessage {
            content: UserContent::Blocks(Vec::new()),
            timestamp: now_millis(),
        })],
        tools: None,
    };

    let response = complete(
        &model,
        &context,
        StreamOptions {
            signal: None,
            reasoning_effort: None,
        },
    );
    assert_response(&response);
}

#[test]
fn should_handle_empty_string_content() {
    let model = get_model("mock", "test-model");
    let context = Context {
        system_prompt: None,
        messages: vec![Message::User(UserMessage {
            content: UserContent::Text(String::new()),
            timestamp: now_millis(),
        })],
        tools: None,
    };

    let response = complete(
        &model,
        &context,
        StreamOptions {
            signal: None,
            reasoning_effort: None,
        },
    );
    assert_response(&response);
}

#[test]
fn should_handle_whitespace_only_content() {
    let model = get_model("mock", "test-model");
    let context = Context {
        system_prompt: None,
        messages: vec![Message::User(UserMessage {
            content: UserContent::Text("   \n\t  ".to_string()),
            timestamp: now_millis(),
        })],
        tools: None,
    };

    let response = complete(
        &model,
        &context,
        StreamOptions {
            signal: None,
            reasoning_effort: None,
        },
    );
    assert_response(&response);
}

#[test]
fn should_handle_empty_assistant_message_in_conversation() {
    let model = get_model("mock", "test-model");
    let empty_assistant = AssistantMessage {
        content: Vec::new(),
        api: model.api.clone(),
        provider: model.provider.clone(),
        model: model.id.clone(),
        usage: default_usage(),
        stop_reason: "stop".to_string(),
        error_message: None,
        timestamp: now_millis(),
    };

    let context = Context {
        system_prompt: None,
        messages: vec![
            Message::User(UserMessage {
                content: UserContent::Text("Hello, how are you?".to_string()),
                timestamp: now_millis(),
            }),
            Message::Assistant(empty_assistant),
            Message::User(UserMessage {
                content: UserContent::Text("Please respond this time.".to_string()),
                timestamp: now_millis(),
            }),
        ],
        tools: None,
    };

    let response = complete(
        &model,
        &context,
        StreamOptions {
            signal: None,
            reasoning_effort: None,
        },
    );
    assert_response(&response);
    assert!(!response.content.is_empty());
}

fn assert_response(response: &AssistantMessage) {
    assert_eq!(response.stop_reason, "stop");
    assert!(!response.content.is_empty());
}

fn default_usage() -> Usage {
    Usage {
        input: 10,
        output: 0,
        cache_read: 0,
        cache_write: 0,
        total_tokens: Some(10),
        cost: Some(Cost {
            input: 0.0,
            output: 0.0,
            cache_read: 0.0,
            cache_write: 0.0,
            total: 0.0,
        }),
    }
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}
