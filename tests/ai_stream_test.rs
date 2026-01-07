use pi::ai::{
    complete, get_model, stream, AssistantMessageEvent, Context, Message, StreamOptions, Tool,
};
use pi::{ContentBlock, ToolResultMessage, UserContent, UserMessage};
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

// Source: packages/ai/test/stream.test.ts

#[test]
fn should_complete_basic_text_generation() {
    let model = get_model("mock", "test-model");
    let mut context = Context {
        system_prompt: Some("You are a helpful assistant. Be concise.".to_string()),
        messages: vec![Message::User(UserMessage {
            content: UserContent::Text("Reply with exactly: 'Hello test successful'".to_string()),
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
    assert_eq!(response.stop_reason, "stop");
    assert!(response.usage.input + response.usage.cache_read > 0);
    assert!(response.usage.output > 0);
    assert!(response.error_message.is_none());
    assert!(text_from_blocks(&response.content).contains("Hello test successful"));

    context.messages.push(Message::Assistant(response));
    context.messages.push(Message::User(UserMessage {
        content: UserContent::Text("Now say 'Goodbye test successful'".to_string()),
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
    assert!(second_response.usage.input + second_response.usage.cache_read > 0);
    assert!(second_response.usage.output > 0);
    assert!(second_response.error_message.is_none());
    assert!(text_from_blocks(&second_response.content).contains("Goodbye test successful"));
}

#[test]
fn should_handle_tool_calling() {
    let model = get_model("mock", "test-model");
    let context = Context {
        system_prompt: Some("You are a helpful assistant that uses tools when asked.".to_string()),
        messages: vec![Message::User(UserMessage {
            content: UserContent::Text("Calculate 15 + 27 using the calculator tool.".to_string()),
            timestamp: now_millis(),
        })],
        tools: Some(vec![calculator_tool()]),
    };

    let mut stream = stream(
        &model,
        &context,
        StreamOptions {
            signal: None,
            reasoning_effort: None,
        },
    );
    let mut has_tool_start = false;
    let mut has_tool_delta = false;
    let mut has_tool_end = false;
    let mut accumulated_args = String::new();
    let mut expected_index: Option<usize> = None;

    for event in &mut stream {
        match event {
            AssistantMessageEvent::ToolCallStart {
                partial,
                content_index,
            } => {
                has_tool_start = true;
                expected_index = Some(content_index);
                let block = partial
                    .content
                    .get(content_index)
                    .expect("missing tool call block");
                match block {
                    ContentBlock::ToolCall { name, id, .. } => {
                        assert_eq!(name, "calculator");
                        assert!(!id.is_empty());
                    }
                    _ => panic!("expected tool call block"),
                }
            }
            AssistantMessageEvent::ToolCallDelta {
                delta,
                partial,
                content_index,
            } => {
                has_tool_delta = true;
                assert_eq!(Some(content_index), expected_index);
                accumulated_args.push_str(&delta);
                let block = partial
                    .content
                    .get(content_index)
                    .expect("missing tool call block");
                match block {
                    ContentBlock::ToolCall {
                        name, arguments, ..
                    } => {
                        assert_eq!(name, "calculator");
                        assert!(arguments.is_object());
                    }
                    _ => panic!("expected tool call block"),
                }
            }
            AssistantMessageEvent::ToolCallEnd {
                partial,
                content_index,
            } => {
                has_tool_end = true;
                assert_eq!(Some(content_index), expected_index);
                let parsed: Value =
                    serde_json::from_str(&accumulated_args).expect("invalid tool args json");
                let block = partial
                    .content
                    .get(content_index)
                    .expect("missing tool call block");
                match block {
                    ContentBlock::ToolCall {
                        name, arguments, ..
                    } => {
                        assert_eq!(name, "calculator");
                        assert_eq!(arguments.get("a").and_then(Value::as_i64), Some(15));
                        assert_eq!(arguments.get("b").and_then(Value::as_i64), Some(27));
                        let operation = arguments
                            .get("operation")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        assert!(["add", "subtract", "multiply", "divide"].contains(&operation));
                    }
                    _ => panic!("expected tool call block"),
                }
                assert!(parsed.is_object());
            }
            _ => {}
        }
    }

    assert!(has_tool_start);
    assert!(has_tool_delta);
    assert!(has_tool_end);

    let response = stream.result();
    assert_eq!(response.stop_reason, "toolUse");
    assert!(response
        .content
        .iter()
        .any(|block| matches!(block, ContentBlock::ToolCall { .. })));
}

#[test]
fn should_handle_streaming() {
    let model = get_model("mock", "test-model");
    let context = Context {
        system_prompt: None,
        messages: vec![Message::User(UserMessage {
            content: UserContent::Text("Count from 1 to 3".to_string()),
            timestamp: now_millis(),
        })],
        tools: None,
    };

    let mut stream = stream(
        &model,
        &context,
        StreamOptions {
            signal: None,
            reasoning_effort: None,
        },
    );
    let mut text_started = false;
    let mut text_chunks = String::new();
    let mut text_completed = false;

    for event in &mut stream {
        match event {
            AssistantMessageEvent::TextStart { .. } => text_started = true,
            AssistantMessageEvent::TextDelta { delta, .. } => text_chunks.push_str(&delta),
            AssistantMessageEvent::TextEnd { .. } => text_completed = true,
            _ => {}
        }
    }

    let response = stream.result();
    assert!(text_started);
    assert!(!text_chunks.is_empty());
    assert!(text_completed);
    assert!(response
        .content
        .iter()
        .any(|block| matches!(block, ContentBlock::Text { .. })));
}

#[test]
fn should_handle() {
    let model = get_model("mock", "test-model");
    let context = Context {
        system_prompt: None,
        messages: vec![Message::User(UserMessage {
            content: UserContent::Text(
                "Think long and hard about 10 + 27. Think step by step. Then output the result."
                    .to_string(),
            ),
            timestamp: now_millis(),
        })],
        tools: None,
    };

    let mut stream = stream(
        &model,
        &context,
        StreamOptions {
            signal: None,
            reasoning_effort: None,
        },
    );
    let mut thinking_started = false;
    let mut thinking_chunks = String::new();
    let mut thinking_completed = false;

    for event in &mut stream {
        match event {
            AssistantMessageEvent::ThinkingStart { .. } => thinking_started = true,
            AssistantMessageEvent::ThinkingDelta { delta, .. } => thinking_chunks.push_str(&delta),
            AssistantMessageEvent::ThinkingEnd { .. } => thinking_completed = true,
            _ => {}
        }
    }

    let response = stream.result();
    assert_eq!(response.stop_reason, "stop");
    assert!(thinking_started);
    assert!(!thinking_chunks.is_empty());
    assert!(thinking_completed);
    assert!(response
        .content
        .iter()
        .any(|block| matches!(block, ContentBlock::Thinking { .. })));
}

#[test]
fn should_handle_multi_turn_with_thinking_and_tools() {
    let model = get_model("mock", "test-model");
    let mut context = Context {
        system_prompt: Some("You are a helpful assistant that can use tools to answer questions.".to_string()),
        messages: vec![Message::User(UserMessage {
            content: UserContent::Text(
                "Think about this briefly, then calculate 42 * 17 and 453 + 434 using the calculator tool."
                    .to_string(),
            ),
            timestamp: now_millis(),
        })],
        tools: Some(vec![calculator_tool()]),
    };

    let mut all_text = String::new();
    let mut has_thinking = false;
    let mut has_tool_calls = false;
    let max_turns = 5;

    for _ in 0..max_turns {
        let response = complete(
            &model,
            &context,
            StreamOptions {
                signal: None,
                reasoning_effort: None,
            },
        );
        context.messages.push(Message::Assistant(response.clone()));

        let mut results = Vec::new();
        for block in &response.content {
            match block {
                ContentBlock::Text { text, .. } => all_text.push_str(text),
                ContentBlock::Thinking { .. } => has_thinking = true,
                ContentBlock::ToolCall {
                    id,
                    name,
                    arguments,
                    ..
                } => {
                    has_tool_calls = true;
                    assert_eq!(name, "calculator");
                    let a = arguments.get("a").and_then(Value::as_i64).unwrap_or(0);
                    let b = arguments.get("b").and_then(Value::as_i64).unwrap_or(0);
                    let operation = arguments
                        .get("operation")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let result = match operation {
                        "add" => a + b,
                        "multiply" => a * b,
                        _ => 0,
                    };
                    results.push(Message::ToolResult(ToolResultMessage {
                        tool_call_id: id.clone(),
                        tool_name: name.clone(),
                        content: vec![ContentBlock::Text {
                            text: result.to_string(),
                            text_signature: None,
                        }],
                        details: None,
                        is_error: false,
                        timestamp: now_millis(),
                    }));
                }
                _ => {}
            }
        }
        context.messages.extend(results);

        assert_ne!(response.stop_reason, "error");
        if response.stop_reason == "stop" {
            break;
        }
    }

    assert!(has_thinking || has_tool_calls);
    assert!(all_text.contains("714"));
    assert!(all_text.contains("887"));
}

#[test]
fn should_handle_image_input() {
    let model = get_model("mock", "test-model");
    let context = Context {
        system_prompt: None,
        messages: vec![Message::User(UserMessage {
            content: UserContent::Blocks(vec![
                ContentBlock::Text {
                    text: "Describe the color and shape in the image.".to_string(),
                    text_signature: None,
                },
                ContentBlock::Image {
                    data: "fake-base64".to_string(),
                    mime_type: "image/png".to_string(),
                },
            ]),
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
    let text = text_from_blocks(&response.content).to_lowercase();
    assert!(text.contains("red"));
    assert!(text.contains("circle"));
}

#[test]
fn should_handle_thinking() {
    let model = get_model("mock", "test-model");
    let context = Context {
        system_prompt: None,
        messages: vec![Message::User(UserMessage {
            content: UserContent::Text(
                "Think about 5 + 7 step by step. Then output the result.".to_string(),
            ),
            timestamp: now_millis(),
        })],
        tools: None,
    };

    let mut stream = stream(
        &model,
        &context,
        StreamOptions {
            signal: None,
            reasoning_effort: None,
        },
    );
    let mut saw_thinking = false;

    for event in &mut stream {
        if matches!(event, AssistantMessageEvent::ThinkingDelta { .. }) {
            saw_thinking = true;
        }
    }

    let response = stream.result();
    assert!(saw_thinking);
    assert!(response
        .content
        .iter()
        .any(|block| matches!(block, ContentBlock::Thinking { .. })));
}

#[test]
fn should_handle_thinking_mode() {
    let model = get_model("mock", "test-model");
    let context = Context {
        system_prompt: None,
        messages: vec![Message::User(UserMessage {
            content: UserContent::Text(
                "Think step by step about 9 + 11. Then output the result.".to_string(),
            ),
            timestamp: now_millis(),
        })],
        tools: None,
    };

    let mut stream = stream(
        &model,
        &context,
        StreamOptions {
            signal: None,
            reasoning_effort: None,
        },
    );
    let mut saw_thinking = false;

    for event in &mut stream {
        if matches!(event, AssistantMessageEvent::ThinkingStart { .. }) {
            saw_thinking = true;
        }
    }

    let response = stream.result();
    assert!(saw_thinking);
    assert!(response
        .content
        .iter()
        .any(|block| matches!(block, ContentBlock::Thinking { .. })));
}

fn calculator_tool() -> Tool {
    Tool {
        name: "calculator".to_string(),
        description: "Perform basic arithmetic operations".to_string(),
    }
}

fn text_from_blocks(blocks: &[ContentBlock]) -> String {
    blocks
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect::<Vec<String>>()
        .join("")
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}
