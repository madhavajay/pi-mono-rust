use std::cell::RefCell;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use pi::agent::{
    get_model, Agent, AgentError, AgentMessage, AgentOptions, AgentStateOverride, AgentTool,
    AgentToolResult, LlmContext, Model, ThinkingLevel,
};
use pi::{
    AssistantMessage, ContentBlock, Cost, ToolResultMessage, Usage, UserContent, UserMessage,
};
use serde_json::{json, Value};

// Source: packages/agent/test/e2e.test.ts

type StreamFn = dyn FnMut(&Model, &LlmContext) -> AssistantMessage;

#[test]
fn should_handle_basic_text_prompt() {
    let model = get_model("mock", "test-model");
    let agent = Agent::new(AgentOptions {
        initial_state: Some(AgentStateOverride {
            system_prompt: Some(
                "You are a helpful assistant. Keep your responses concise.".to_string(),
            ),
            model: Some(model),
            thinking_level: Some(ThinkingLevel::Off),
            tools: Some(Vec::new()),
            ..AgentStateOverride::default()
        }),
        stream_fn: Some(mock_stream_fn()),
        ..AgentOptions::default()
    });

    agent
        .prompt("What is 2+2? Answer with just the number.")
        .expect("prompt");

    let state = agent.state();
    assert!(!state.is_streaming);
    assert_eq!(state.messages.len(), 2);
    assert_eq!(state.messages[0].role(), "user");
    assert_eq!(state.messages[1].role(), "assistant");

    let assistant_message = match &state.messages[1] {
        AgentMessage::Assistant(message) => message,
        _ => panic!("Expected assistant message"),
    };
    let text_content = assistant_message
        .content
        .iter()
        .find_map(|block| match block {
            ContentBlock::Text { text, .. } => Some(text),
            _ => None,
        })
        .expect("text content");
    assert!(text_content.contains('4'));
}

#[test]
fn should_execute_tools_correctly() {
    let model = get_model("mock", "test-model");
    let agent = Agent::new(AgentOptions {
        initial_state: Some(AgentStateOverride {
            system_prompt: Some(
                "You are a helpful assistant. Always use the calculator tool for math.".to_string(),
            ),
            model: Some(model),
            thinking_level: Some(ThinkingLevel::Off),
            tools: Some(vec![calculate_tool()]),
            ..AgentStateOverride::default()
        }),
        stream_fn: Some(mock_stream_fn()),
        ..AgentOptions::default()
    });

    agent
        .prompt("Calculate 123 * 456 using the calculator tool.")
        .expect("prompt");

    let state = agent.state();
    assert!(!state.is_streaming);
    assert!(state.messages.len() >= 3);

    let tool_result = state
        .messages
        .iter()
        .find_map(|message| match message {
            AgentMessage::ToolResult(result) => Some(result),
            _ => None,
        })
        .expect("tool result message");
    let tool_text = tool_result_text(tool_result);
    assert!(tool_text.contains("56088"));

    let final_message = state.messages.last().expect("final message");
    let final_text = match final_message {
        AgentMessage::Assistant(message) => message
            .content
            .iter()
            .find_map(|block| match block {
                ContentBlock::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .unwrap_or_default(),
        _ => panic!("Expected assistant message"),
    };
    let has_number = final_text.contains("56088") || final_text.contains("56,088");
    assert!(has_number);
}

#[test]
fn should_handle_abort_during_execution() {
    let model = get_model("mock", "test-model");
    let agent_holder: Rc<RefCell<Option<Rc<Agent>>>> = Rc::new(RefCell::new(None));
    let holder_ref = agent_holder.clone();
    let stream_fn = Box::new(move |_model: &Model, context: &LlmContext| {
        if let Some(agent) = holder_ref.borrow().as_ref() {
            agent.abort();
        }
        respond_for_context(context)
    });

    let agent = Rc::new(Agent::new(AgentOptions {
        initial_state: Some(AgentStateOverride {
            system_prompt: Some("You are a helpful assistant.".to_string()),
            model: Some(model),
            thinking_level: Some(ThinkingLevel::Off),
            tools: Some(vec![calculate_tool()]),
            ..AgentStateOverride::default()
        }),
        stream_fn: Some(stream_fn),
        ..AgentOptions::default()
    }));
    *agent_holder.borrow_mut() = Some(agent.clone());

    agent
        .prompt("Calculate 100 * 200, then 300 * 400, then sum the results.")
        .expect("prompt");

    let state = agent.state();
    assert!(!state.is_streaming);
    assert!(state.messages.len() >= 2);

    let last_message = state.messages.last().expect("last message");
    let assistant = match last_message {
        AgentMessage::Assistant(message) => message,
        _ => panic!("Expected assistant message"),
    };
    assert_eq!(assistant.stop_reason, "aborted");
    assert!(assistant.error_message.is_some());
    assert!(state.error.is_some());
    assert_eq!(state.error.as_deref(), assistant.error_message.as_deref());
}

#[test]
fn should_emit_state_updates_during_streaming() {
    let model = get_model("mock", "test-model");
    let agent = Agent::new(AgentOptions {
        initial_state: Some(AgentStateOverride {
            system_prompt: Some("You are a helpful assistant.".to_string()),
            model: Some(model),
            thinking_level: Some(ThinkingLevel::Off),
            tools: Some(Vec::new()),
            ..AgentStateOverride::default()
        }),
        stream_fn: Some(mock_stream_fn()),
        ..AgentOptions::default()
    });

    let events: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let events_ref = events.clone();
    let _unsubscribe = agent.subscribe(move |event| {
        events_ref.borrow_mut().push(event.kind().to_string());
    });

    agent.prompt("Count from 1 to 5.").expect("prompt");

    let events = events.borrow();
    assert!(events.contains(&"agent_start".to_string()));
    assert!(events.contains(&"agent_end".to_string()));
    assert!(events.contains(&"message_start".to_string()));
    assert!(events.contains(&"message_end".to_string()));
    assert!(events.contains(&"message_update".to_string()));

    let state = agent.state();
    assert!(!state.is_streaming);
    assert_eq!(state.messages.len(), 2);
}

#[test]
fn should_maintain_context_across_multiple_turns() {
    let model = get_model("mock", "test-model");
    let agent = Agent::new(AgentOptions {
        initial_state: Some(AgentStateOverride {
            system_prompt: Some("You are a helpful assistant.".to_string()),
            model: Some(model),
            thinking_level: Some(ThinkingLevel::Off),
            tools: Some(Vec::new()),
            ..AgentStateOverride::default()
        }),
        stream_fn: Some(mock_stream_fn()),
        ..AgentOptions::default()
    });

    agent.prompt("My name is Alice.").expect("prompt");
    assert_eq!(agent.state().messages.len(), 2);

    agent.prompt("What is my name?").expect("prompt");
    assert_eq!(agent.state().messages.len(), 4);

    let last_message = agent.state().messages[3].clone();
    let assistant = match last_message {
        AgentMessage::Assistant(message) => message,
        _ => panic!("Expected assistant message"),
    };
    let text = assistant
        .content
        .iter()
        .find_map(|block| match block {
            ContentBlock::Text { text, .. } => Some(text.to_lowercase()),
            _ => None,
        })
        .unwrap_or_default();
    assert!(text.contains("alice"));
}

#[test]
fn should_throw_when_no_messages_in_context() {
    let agent = Agent::new(AgentOptions {
        initial_state: Some(AgentStateOverride {
            system_prompt: Some("Test".to_string()),
            model: Some(get_model("mock", "test-model")),
            ..AgentStateOverride::default()
        }),
        stream_fn: Some(mock_stream_fn()),
        ..AgentOptions::default()
    });

    let err = agent.continue_prompt().unwrap_err();
    assert!(matches!(err, AgentError::NoMessages));
}

#[test]
fn should_throw_when_last_message_is_assistant() {
    let agent = Agent::new(AgentOptions {
        initial_state: Some(AgentStateOverride {
            system_prompt: Some("Test".to_string()),
            model: Some(get_model("mock", "test-model")),
            ..AgentStateOverride::default()
        }),
        stream_fn: Some(mock_stream_fn()),
        ..AgentOptions::default()
    });

    let assistant_message = assistant_text_message("Hello", "stop");
    agent.replace_messages(vec![AgentMessage::Assistant(assistant_message)]);

    let err = agent.continue_prompt().unwrap_err();
    assert!(matches!(err, AgentError::LastMessageAssistant));
}

#[test]
fn should_continue_and_get_response_when_last_message_is_user() {
    let agent = Agent::new(AgentOptions {
        initial_state: Some(AgentStateOverride {
            system_prompt: Some(
                "You are a helpful assistant. Follow instructions exactly.".to_string(),
            ),
            model: Some(get_model("mock", "test-model")),
            thinking_level: Some(ThinkingLevel::Off),
            tools: Some(Vec::new()),
            ..AgentStateOverride::default()
        }),
        stream_fn: Some(mock_stream_fn()),
        ..AgentOptions::default()
    });

    let user_message = AgentMessage::User(UserMessage {
        content: UserContent::Text("Say exactly: HELLO WORLD".to_string()),
        timestamp: now_millis(),
    });
    agent.replace_messages(vec![user_message]);

    agent.continue_prompt().expect("continue");

    let state = agent.state();
    assert!(!state.is_streaming);
    assert_eq!(state.messages.len(), 2);
    let assistant = match &state.messages[1] {
        AgentMessage::Assistant(message) => message,
        _ => panic!("Expected assistant message"),
    };
    let text = assistant
        .content
        .iter()
        .find_map(|block| match block {
            ContentBlock::Text { text, .. } => Some(text.to_uppercase()),
            _ => None,
        })
        .unwrap_or_default();
    assert!(text.contains("HELLO WORLD"));
}

#[test]
fn should_continue_and_process_tool_results() {
    let agent = Agent::new(AgentOptions {
        initial_state: Some(AgentStateOverride {
            system_prompt: Some(
                "You are a helpful assistant. After getting a calculation result, state the answer clearly."
                    .to_string(),
            ),
            model: Some(get_model("mock", "test-model")),
            thinking_level: Some(ThinkingLevel::Off),
            tools: Some(vec![calculate_tool()]),
            ..AgentStateOverride::default()
        }),
        stream_fn: Some(mock_stream_fn()),
        ..AgentOptions::default()
    });

    let user_message = AgentMessage::User(UserMessage {
        content: UserContent::Text("What is 5 + 3?".to_string()),
        timestamp: now_millis(),
    });

    let assistant_message = AssistantMessage {
        content: vec![
            ContentBlock::Text {
                text: "Let me calculate that.".to_string(),
                text_signature: None,
            },
            ContentBlock::ToolCall {
                id: "calc-1".to_string(),
                name: "calculate".to_string(),
                arguments: json!({ "expression": "5 + 3" }),
                thought_signature: None,
            },
        ],
        api: "anthropic-messages".to_string(),
        provider: "anthropic".to_string(),
        model: "claude-haiku-4-5".to_string(),
        usage: default_usage(),
        stop_reason: "toolUse".to_string(),
        error_message: None,
        timestamp: now_millis(),
    };

    let tool_result = ToolResultMessage {
        tool_call_id: "calc-1".to_string(),
        tool_name: "calculate".to_string(),
        content: vec![ContentBlock::Text {
            text: "5 + 3 = 8".to_string(),
            text_signature: None,
        }],
        details: None,
        is_error: false,
        timestamp: now_millis(),
    };

    agent.replace_messages(vec![
        user_message,
        AgentMessage::Assistant(assistant_message),
        AgentMessage::ToolResult(tool_result),
    ]);

    agent.continue_prompt().expect("continue");

    let state = agent.state();
    assert!(!state.is_streaming);
    assert!(state.messages.len() >= 4);
    let last_message = state.messages.last().expect("last message");
    match last_message {
        AgentMessage::Assistant(message) => {
            let text = message
                .content
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::Text { text, .. } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<&str>>()
                .join(" ");
            assert!(text.contains('8'));
        }
        _ => panic!("Expected assistant message"),
    }
}

fn mock_stream_fn() -> Box<StreamFn> {
    Box::new(move |_model, context| respond_for_context(context))
}

fn respond_for_context(context: &LlmContext) -> AssistantMessage {
    if let Some(last_message) = context.messages.last() {
        match last_message {
            AgentMessage::User(user) => {
                let text = user_text(user).to_lowercase();
                if text.contains("what is 2+2") {
                    return assistant_text_message("4", "stop");
                }
                if text.contains("calculate 123 * 456") {
                    return AssistantMessage {
                        content: vec![
                            ContentBlock::Text {
                                text: "Let me calculate that.".to_string(),
                                text_signature: None,
                            },
                            ContentBlock::ToolCall {
                                id: "calc-1".to_string(),
                                name: "calculate".to_string(),
                                arguments: json!({ "expression": "123 * 456" }),
                                thought_signature: None,
                            },
                        ],
                        api: "anthropic-messages".to_string(),
                        provider: "anthropic".to_string(),
                        model: "claude-haiku-4-5".to_string(),
                        usage: default_usage(),
                        stop_reason: "toolUse".to_string(),
                        error_message: None,
                        timestamp: now_millis(),
                    };
                }
                if text.contains("count from 1 to 5") {
                    return assistant_text_message("1 2 3 4 5", "stop");
                }
                if text.contains("my name is alice") {
                    return assistant_text_message("Nice to meet you, Alice.", "stop");
                }
                if text.contains("what is my name") {
                    return assistant_text_message("Your name is Alice.", "stop");
                }
                if text.contains("say exactly: hello world") {
                    return assistant_text_message("HELLO WORLD", "stop");
                }
                return assistant_text_message("ok", "stop");
            }
            AgentMessage::ToolResult(result) => {
                let text = tool_result_text(result);
                if text.contains("56088") {
                    return assistant_text_message("The result is 56088.", "stop");
                }
                if text.contains("= 8") || text.contains(" 8") {
                    return assistant_text_message("The result is 8.", "stop");
                }
            }
            _ => {}
        }
    }

    assistant_text_message("ok", "stop")
}

fn calculate_tool() -> AgentTool {
    AgentTool {
        name: "calculate".to_string(),
        label: "Calculator".to_string(),
        description: "Evaluate mathematical expressions".to_string(),
        execute: Rc::new(|_tool_call_id, args| {
            let expression = args
                .get("expression")
                .and_then(|value| value.as_str())
                .ok_or_else(|| "Missing expression".to_string())?;
            let result = evaluate_expression(expression)?;
            Ok(AgentToolResult {
                content: vec![ContentBlock::Text {
                    text: format!("{expression} = {result}"),
                    text_signature: None,
                }],
                details: Value::Null,
            })
        }),
    }
}

fn evaluate_expression(expression: &str) -> Result<i64, String> {
    let parts: Vec<&str> = expression.split_whitespace().collect();
    if parts.len() != 3 {
        return Err(format!("Unsupported expression: {expression}"));
    }
    let left: i64 = parts[0]
        .parse()
        .map_err(|_| format!("Invalid number: {}", parts[0]))?;
    let right: i64 = parts[2]
        .parse()
        .map_err(|_| format!("Invalid number: {}", parts[2]))?;
    match parts[1] {
        "+" => Ok(left + right),
        "-" => Ok(left - right),
        "*" => Ok(left * right),
        "/" => Ok(left / right),
        _ => Err(format!("Unsupported operator: {}", parts[1])),
    }
}

fn assistant_text_message(text: &str, stop_reason: &str) -> AssistantMessage {
    assistant_message(
        vec![ContentBlock::Text {
            text: text.to_string(),
            text_signature: None,
        }],
        stop_reason,
        None,
    )
}

fn assistant_message(
    content: Vec<ContentBlock>,
    stop_reason: &str,
    error_message: Option<String>,
) -> AssistantMessage {
    AssistantMessage {
        content,
        api: "openai-responses".to_string(),
        provider: "openai".to_string(),
        model: "mock".to_string(),
        usage: default_usage(),
        stop_reason: stop_reason.to_string(),
        error_message,
        timestamp: now_millis(),
    }
}

fn user_text(message: &UserMessage) -> String {
    match &message.content {
        UserContent::Text(text) => text.clone(),
        UserContent::Blocks(blocks) => blocks
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<String>>()
            .join("\n"),
    }
}

fn tool_result_text(message: &ToolResultMessage) -> String {
    message
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect::<Vec<String>>()
        .join("\n")
}

fn default_usage() -> Usage {
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
