use pi::ai::{complete, get_model, Context, Message, StreamOptions, Tool};
use pi::{
    AssistantMessage, ContentBlock, Cost, ToolResultMessage, Usage, UserContent, UserMessage,
};
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};

// Source: packages/ai/test/handoff.test.ts

#[test]
fn should_handle_contexts_from_all_providers() {
    let model = get_model("mock", "test-model");
    let contexts = vec![
        provider_context(
            &model,
            "anthropic-style",
            "tool_1",
            "Tokyo",
            "Weather in Tokyo: 18C, partly cloudy",
            false,
        ),
        provider_context(
            &model,
            "google-style",
            "tool_2",
            "Berlin",
            "Weather in Berlin: 22C, sunny",
            false,
        ),
        provider_context(
            &model,
            "openai-completions",
            "tool_3",
            "London",
            "Weather in London: 15C, rainy",
            false,
        ),
        provider_context(
            &model,
            "openai-responses",
            "tool_4",
            "Sydney",
            "Weather in Sydney: 25C, clear",
            false,
        ),
        provider_context(&model, "aborted", "tool_abort", "none", "", true),
    ];

    for context in contexts {
        let response = complete(
            &model,
            &context.context,
            StreamOptions {
                signal: None,
                reasoning_effort: None,
            },
        );
        if context.is_aborted {
            assert!(matches!(response.stop_reason.as_str(), "stop" | "toolUse"));
            assert!(!response.content.is_empty());
            continue;
        }

        assert_ne!(response.stop_reason, "error");
        let text = text_from_blocks(&response.content).to_lowercase();
        if let Some(expected) = context.expected_token.as_ref() {
            assert!(text.contains(expected));
        }
    }
}

struct HandoffContext {
    context: Context,
    expected_token: Option<String>,
    is_aborted: bool,
}

fn provider_context(
    model: &pi::ai::Model,
    label: &'static str,
    tool_id: &str,
    city: &str,
    tool_text: &str,
    aborted: bool,
) -> HandoffContext {
    let assistant_message = if aborted {
        AssistantMessage {
            content: vec![
                ContentBlock::Thinking {
                    thinking: "Let me start calculating 20 * 30...".to_string(),
                    thinking_signature: None,
                },
                ContentBlock::Text {
                    text: "I was about to calculate 20 * 30 which is".to_string(),
                    text_signature: None,
                },
            ],
            api: "anthropic-messages".to_string(),
            provider: "test".to_string(),
            model: "test-model".to_string(),
            usage: zero_usage(),
            stop_reason: "error".to_string(),
            error_message: Some("Request was aborted".to_string()),
            timestamp: now_millis(),
        }
    } else {
        AssistantMessage {
            content: vec![
                ContentBlock::Thinking {
                    thinking: format!("Calculating 17 * 23 for {label}."),
                    thinking_signature: None,
                },
                ContentBlock::Text {
                    text: format!(
                        "The result is 391. The capital of Austria is Vienna. Checking weather for {city}."
                    ),
                    text_signature: None,
                },
                ContentBlock::ToolCall {
                    id: tool_id.to_string(),
                    name: "get_weather".to_string(),
                    arguments: json!({ "location": city }),
                    thought_signature: None,
                },
            ],
            api: "mock-api".to_string(),
            provider: "mock".to_string(),
            model: model.id.clone(),
            usage: zero_usage(),
            stop_reason: "toolUse".to_string(),
            error_message: None,
            timestamp: now_millis(),
        }
    };

    let mut messages = vec![
        Message::User(UserMessage {
            content: UserContent::Text(
                "Please do some calculations, tell me about capitals, and check the weather."
                    .to_string(),
            ),
            timestamp: now_millis(),
        }),
        Message::Assistant(assistant_message),
    ];

    let expected_token = if aborted {
        None
    } else {
        messages.push(tool_result(tool_id, "get_weather", tool_text));
        Some(city.to_lowercase())
    };

    messages.push(Message::User(UserMessage {
        content: UserContent::Text(
            "Based on our conversation, answer the questions with numbers and names.".to_string(),
        ),
        timestamp: now_millis(),
    }));

    HandoffContext {
        context: Context {
            system_prompt: None,
            messages,
            tools: Some(vec![Tool {
                name: "get_weather".to_string(),
                description: "Get the weather for a location".to_string(),
            }]),
        },
        expected_token,
        is_aborted: aborted,
    }
}

fn tool_result(tool_id: &str, tool_name: &str, text: &str) -> Message {
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

fn zero_usage() -> Usage {
    Usage {
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
    }
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}
