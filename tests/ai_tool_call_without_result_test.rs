use pi::ai::{complete, get_model, Context, Message, StreamOptions, Tool};
use pi::{ContentBlock, UserContent, UserMessage};
use std::time::{SystemTime, UNIX_EPOCH};

// Source: packages/ai/test/tool-call-without-result.test.ts

#[test]
fn should_filter_out_tool_calls_without_corresponding_tool_results() {
    let model = get_model("mock", "test-model");
    let mut context = Context {
        system_prompt: Some(
            "You are a helpful assistant. Use the calculate tool when asked to perform calculations."
                .to_string(),
        ),
        messages: Vec::new(),
        tools: Some(vec![Tool {
            name: "calculate".to_string(),
            description: "Evaluate mathematical expressions".to_string(),
        }]),
    };

    context.messages.push(Message::User(UserMessage {
        content: UserContent::Text(
            "Please calculate 25 * 18 using the calculate tool.".to_string(),
        ),
        timestamp: now_millis(),
    }));

    let first_response = complete(
        &model,
        &context,
        StreamOptions {
            signal: None,
            reasoning_effort: None,
        },
    );
    let has_tool_call = first_response
        .content
        .iter()
        .any(|block| matches!(block, ContentBlock::ToolCall { .. }));
    assert!(has_tool_call, "expected assistant to make a tool call");

    context.messages.push(Message::Assistant(first_response));
    context.messages.push(Message::User(UserMessage {
        content: UserContent::Text("Never mind, just tell me what is 2+2?".to_string()),
        timestamp: now_millis(),
    }));

    let second_response = complete(
        &model,
        &context,
        StreamOptions {
            signal: None,
            reasoning_effort: None,
        },
    );
    assert_ne!(second_response.stop_reason, "error");
    assert!(!second_response.content.is_empty());

    let text_content = second_response
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<&str>>()
        .join(" ");
    let tool_calls = second_response
        .content
        .iter()
        .filter(|block| matches!(block, ContentBlock::ToolCall { .. }))
        .count();
    assert!(tool_calls > 0 || !text_content.is_empty());
    assert!(matches!(
        second_response.stop_reason.as_str(),
        "stop" | "toolUse"
    ));
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}
