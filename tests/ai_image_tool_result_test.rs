use pi::ai::{complete, get_model, Context, Message, StreamOptions, Tool};
use pi::{ContentBlock, ToolResultMessage, UserContent, UserMessage};
use std::time::{SystemTime, UNIX_EPOCH};

// Source: packages/ai/test/image-tool-result.test.ts

#[test]
fn should_handle_tool_result_with_only_image() {
    let model = get_model("mock", "test-model");
    let mut context = Context {
        system_prompt: Some("You are a helpful assistant that uses tools when asked.".to_string()),
        messages: vec![Message::User(UserMessage {
            content: UserContent::Text(
                "Call the get_circle tool to get an image, and describe what you see.".to_string(),
            ),
            timestamp: now_millis(),
        })],
        tools: Some(vec![Tool {
            name: "get_circle".to_string(),
            description: "Returns a circle image for visualization".to_string(),
        }]),
    };

    let first_response = complete(
        &model,
        &context,
        StreamOptions {
            signal: None,
            reasoning_effort: None,
        },
    );
    assert_eq!(first_response.stop_reason, "toolUse");
    let tool_call_id = tool_call_id(&first_response).expect("expected tool call");
    context.messages.push(Message::Assistant(first_response));
    context
        .messages
        .push(Message::ToolResult(ToolResultMessage {
            tool_call_id,
            tool_name: "get_circle".to_string(),
            content: vec![ContentBlock::Image {
                data: "fake-base64".to_string(),
                mime_type: "image/png".to_string(),
            }],
            details: None,
            is_error: false,
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
    assert_eq!(second_response.stop_reason, "stop");
    let text = text_from_blocks(&second_response.content).to_lowercase();
    assert!(text.contains("red"));
    assert!(text.contains("circle"));
}

#[test]
fn should_handle_tool_result_with_text_and_image() {
    let model = get_model("mock", "test-model");
    let mut context = Context {
        system_prompt: Some("You are a helpful assistant that uses tools when asked.".to_string()),
        messages: vec![Message::User(UserMessage {
            content: UserContent::Text(
                "Use the get_circle_with_description tool and tell me what you learned."
                    .to_string(),
            ),
            timestamp: now_millis(),
        })],
        tools: Some(vec![Tool {
            name: "get_circle_with_description".to_string(),
            description: "Returns a circle image with a text description".to_string(),
        }]),
    };

    let first_response = complete(
        &model,
        &context,
        StreamOptions {
            signal: None,
            reasoning_effort: None,
        },
    );
    assert_eq!(first_response.stop_reason, "toolUse");
    let tool_call_id = tool_call_id(&first_response).expect("expected tool call");
    context.messages.push(Message::Assistant(first_response));
    context
        .messages
        .push(Message::ToolResult(ToolResultMessage {
            tool_call_id,
            tool_name: "get_circle_with_description".to_string(),
            content: vec![
                ContentBlock::Text {
                    text: "This is a geometric shape with a diameter of 100 pixels.".to_string(),
                    text_signature: None,
                },
                ContentBlock::Image {
                    data: "fake-base64".to_string(),
                    mime_type: "image/png".to_string(),
                },
            ],
            details: None,
            is_error: false,
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
    assert_eq!(second_response.stop_reason, "stop");
    let text = text_from_blocks(&second_response.content).to_lowercase();
    assert!(text.contains("red"));
    assert!(text.contains("circle"));
    assert!(text.contains("diameter") || text.contains("100") || text.contains("pixel"));
}

fn tool_call_id(message: &pi::AssistantMessage) -> Option<String> {
    message.content.iter().find_map(|block| {
        if let ContentBlock::ToolCall { id, name, .. } = block {
            if name == "get_circle" || name == "get_circle_with_description" {
                return Some(id.clone());
            }
        }
        None
    })
}

fn text_from_blocks(blocks: &[ContentBlock]) -> String {
    blocks
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect::<Vec<String>>()
        .join(" ")
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}
