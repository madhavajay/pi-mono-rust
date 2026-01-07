use pi::ai::{complete, get_model, Context, Message, StreamOptions, Tool};
use pi::{
    AssistantMessage, ContentBlock, Cost, ToolResultMessage, Usage, UserContent, UserMessage,
};
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};

// Source: packages/ai/test/unicode-surrogate.test.ts

#[test]
fn should_handle_emoji_in_tool_results() {
    let model = get_model("mock", "test-model");
    let mut context = base_context(&model, "test_tool", "test_1", "Use the test tool");

    let tool_text = "Test with emoji 🙈 and other characters:\n\
- Monkey emoji: 🙈\n\
- Thumbs up: 👍\n\
- Heart: ❤️\n\
- Thinking face: 🤔\n\
- Rocket: 🚀\n\
- Mixed text: Mario Zechner wann? Wo? Bin grad äußersr eventuninformiert 🙈\n\
- Japanese: こんにちは\n\
- Chinese: 你好\n\
- Mathematical symbols: ∑∫∂√\n\
- Special quotes: \"curly\" 'quotes'";
    context
        .messages
        .push(tool_result("test_tool", "test_1", tool_text));
    context.messages.push(Message::User(UserMessage {
        content: UserContent::Text("Summarize the tool result briefly.".to_string()),
        timestamp: now_millis(),
    }));

    let response = complete(
        &model,
        &context,
        StreamOptions {
            signal: None,
            reasoning_effort: None,
        },
    );
    assert_ne!(response.stop_reason, "error");
    assert!(response.error_message.is_none());
    assert!(!response.content.is_empty());
}

#[test]
fn should_handle_real_world_linkedin_comment_data_with_emoji() {
    let model = get_model("mock", "test-model");
    let mut context = base_context(
        &model,
        "linkedin_skill",
        "linkedin_1",
        "Use the linkedin tool to get comments",
    );

    let tool_text = "Post: Hab einen \"Generative KI fur Nicht-Techniker\" Workshop gebaut.\n\
Unanswered Comments: 2\n\n\
=> {\n  \"comments\": [\n    {\n      \"author\": \"Matthias Neumayer's  graphic link\",\n      \"text\": \"Leider nehmen das viel zu wenige Leute ernst\"\n    },\n    {\n      \"author\": \"Matthias Neumayer's  graphic link\",\n      \"text\": \"Mario Zechner wann? Wo? Bin grad aeuessr eventuninformiert 🙈\"\n    }\n  ]\n}";
    context
        .messages
        .push(tool_result("linkedin_skill", "linkedin_1", tool_text));
    context.messages.push(Message::User(UserMessage {
        content: UserContent::Text("How many comments are there?".to_string()),
        timestamp: now_millis(),
    }));

    let response = complete(
        &model,
        &context,
        StreamOptions {
            signal: None,
            reasoning_effort: None,
        },
    );
    assert_ne!(response.stop_reason, "error");
    assert!(response.error_message.is_none());
    assert!(response
        .content
        .iter()
        .any(|block| matches!(block, ContentBlock::Text { .. })));
}

#[test]
fn should_handle_unpaired_high_surrogate_0xd83d_in_tool_results() {
    let model = get_model("mock", "test-model");
    let mut context = base_context(&model, "test_tool", "test_2", "Use the test tool");
    let surrogate = String::from_utf16_lossy(&[0xD83D]);
    let tool_text = format!(
        "Text with unpaired surrogate: {} <- should be sanitized",
        surrogate
    );
    context
        .messages
        .push(tool_result("test_tool", "test_2", &tool_text));
    context.messages.push(Message::User(UserMessage {
        content: UserContent::Text("What did the tool return?".to_string()),
        timestamp: now_millis(),
    }));

    let response = complete(
        &model,
        &context,
        StreamOptions {
            signal: None,
            reasoning_effort: None,
        },
    );
    assert_ne!(response.stop_reason, "error");
    assert!(response.error_message.is_none());
    assert!(!response.content.is_empty());
}

fn base_context(model: &pi::ai::Model, tool_name: &str, tool_id: &str, prompt: &str) -> Context {
    Context {
        system_prompt: Some("You are a helpful assistant.".to_string()),
        messages: vec![
            Message::User(UserMessage {
                content: UserContent::Text(prompt.to_string()),
                timestamp: now_millis(),
            }),
            Message::Assistant(tool_call_message(model, tool_name, tool_id)),
        ],
        tools: Some(vec![Tool {
            name: tool_name.to_string(),
            description: "A test tool".to_string(),
        }]),
    }
}

fn tool_call_message(model: &pi::ai::Model, tool_name: &str, tool_id: &str) -> AssistantMessage {
    AssistantMessage {
        content: vec![ContentBlock::ToolCall {
            id: tool_id.to_string(),
            name: tool_name.to_string(),
            arguments: json!({}),
            thought_signature: None,
        }],
        api: model.api.clone(),
        provider: model.provider.clone(),
        model: model.id.clone(),
        usage: Usage {
            input: 0,
            output: 0,
            cache_read: 0,
            cache_write: 0,
            total_tokens: Some(0),
            cost: Some(Cost {
                input: 0.0,
                output: 0.0,
                cache_read: 0.0,
                cache_write: 0.0,
                total: 0.0,
            }),
        },
        stop_reason: "toolUse".to_string(),
        error_message: None,
        timestamp: now_millis(),
    }
}

fn tool_result(tool_name: &str, tool_id: &str, text: &str) -> Message {
    Message::ToolResult(ToolResultMessage {
        tool_call_id: tool_id.to_string(),
        tool_name: tool_name.to_string(),
        content: vec![ContentBlock::Text {
            text: text.to_string(),
            text_signature: None,
        }],
        details: None,
        is_error: false,
        timestamp: now_millis(),
    })
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}
