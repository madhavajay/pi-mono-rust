use pi::agent::AgentMessage;
use pi::coding_agent::interactive_mode::format_message_for_interactive;
use pi::core::messages::{
    AssistantMessage, ContentBlock, ToolResultMessage, Usage, UserContent, UserMessage,
};
use serde_json::json;

fn usage() -> Usage {
    Usage {
        input: 0,
        output: 0,
        cache_read: 0,
        cache_write: 0,
        total_tokens: None,
        cost: None,
    }
}

fn assistant_message(blocks: Vec<ContentBlock>) -> AssistantMessage {
    AssistantMessage {
        content: blocks,
        api: "test".to_string(),
        provider: "test".to_string(),
        model: "test".to_string(),
        usage: usage(),
        stop_reason: "stop".to_string(),
        error_message: None,
        timestamp: 0,
    }
}

#[test]
fn formats_assistant_with_tool_call() {
    let message = AgentMessage::Assistant(assistant_message(vec![
        ContentBlock::Text {
            text: "Hello".to_string(),
            text_signature: None,
        },
        ContentBlock::ToolCall {
            id: "call-1".to_string(),
            name: "calc".to_string(),
            arguments: json!({"a": 1}),
            thought_signature: None,
        },
    ]));

    let formatted = format_message_for_interactive(&message, true).expect("expected output");
    assert!(formatted.contains("Assistant:\n"));
    assert!(formatted.contains("Hello"));
    assert!(formatted.contains("Tool call: calc"));
    assert!(formatted.contains("\"a\": 1"));
}

#[test]
fn formats_tool_result_error_with_details() {
    let message = AgentMessage::ToolResult(ToolResultMessage {
        tool_call_id: "call-1".to_string(),
        tool_name: "calc".to_string(),
        content: vec![ContentBlock::Text {
            text: "oops".to_string(),
            text_signature: None,
        }],
        details: Some(json!({"code": 500})),
        is_error: true,
        timestamp: 0,
    });

    let formatted = format_message_for_interactive(&message, true).expect("expected output");
    assert!(formatted.contains("Tool result (error): calc"));
    assert!(formatted.contains("oops"));
    assert!(formatted.contains("Details:"));
    assert!(formatted.contains("\"code\": 500"));
}

#[test]
fn skips_user_message_when_disabled() {
    let message = AgentMessage::User(UserMessage {
        content: UserContent::Text("hello".to_string()),
        timestamp: 0,
    });

    let formatted = format_message_for_interactive(&message, false);
    assert!(formatted.is_none());
}
