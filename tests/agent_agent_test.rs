use std::cell::Cell;
use std::rc::Rc;

use pi::agent::{
    get_model, Agent, AgentError, AgentOptions, AgentStateOverride, AgentTool, ThinkingLevel,
};
use pi::{AssistantMessage, ContentBlock, Cost, Usage, UserContent, UserMessage};
use serde_json::Value;

// Source: packages/agent/test/agent.test.ts

#[test]
fn should_create_an_agent_instance_with_default_state() {
    let agent = Agent::new(AgentOptions::default());
    let state = agent.state();

    assert_eq!(state.system_prompt, "");
    assert_eq!(
        state.model,
        get_model("google", "gemini-2.5-flash-lite-preview-06-17")
    );
    assert_eq!(state.thinking_level, ThinkingLevel::Off);
    assert_eq!(state.tools, Vec::<AgentTool>::new());
    assert_eq!(state.messages, Vec::new());
    assert!(!state.is_streaming);
    assert!(state.stream_message.is_none());
    assert!(state.pending_tool_calls.is_empty());
    assert!(state.error.is_none());
}

#[test]
fn should_create_an_agent_instance_with_custom_initial_state() {
    let custom_model = get_model("openai", "gpt-4o-mini");
    let agent = Agent::new(AgentOptions {
        initial_state: Some(AgentStateOverride {
            system_prompt: Some("You are a helpful assistant.".to_string()),
            model: Some(custom_model.clone()),
            thinking_level: Some(ThinkingLevel::Low),
            ..AgentStateOverride::default()
        }),
        ..AgentOptions::default()
    });

    let state = agent.state();
    assert_eq!(state.system_prompt, "You are a helpful assistant.");
    assert_eq!(state.model, custom_model);
    assert_eq!(state.thinking_level, ThinkingLevel::Low);
}

#[test]
fn should_subscribe_to_events() {
    let agent = Agent::new(AgentOptions::default());

    let event_count = Rc::new(Cell::new(0));
    let event_count_ref = event_count.clone();
    let unsubscribe = agent.subscribe(move |_event| {
        event_count_ref.set(event_count_ref.get() + 1);
    });

    assert_eq!(event_count.get(), 0);

    agent.set_system_prompt("Test prompt");
    assert_eq!(event_count.get(), 0);

    unsubscribe();
    agent.set_system_prompt("Another prompt");
    assert_eq!(event_count.get(), 0);
}

#[test]
fn should_update_state_with_mutators() {
    let agent = Agent::new(AgentOptions::default());

    agent.set_system_prompt("Custom prompt");
    assert_eq!(agent.state().system_prompt, "Custom prompt");

    let new_model = get_model("google", "gemini-2.5-flash");
    agent.set_model(new_model.clone());
    assert_eq!(agent.state().model, new_model);

    agent.set_thinking_level(ThinkingLevel::High);
    assert_eq!(agent.state().thinking_level, ThinkingLevel::High);

    let tool = AgentTool {
        name: "test".to_string(),
        label: "Test".to_string(),
        description: "test tool".to_string(),
        execute: Rc::new(|_id, _params| {
            Ok(pi::agent::AgentToolResult {
                content: vec![ContentBlock::Text {
                    text: "ok".to_string(),
                    text_signature: None,
                }],
                details: Value::Null,
            })
        }),
    };
    agent.set_tools(vec![tool.clone()]);
    assert_eq!(agent.state().tools, vec![tool]);

    let message = pi::agent::AgentMessage::User(UserMessage {
        content: UserContent::Text("Hello".to_string()),
        timestamp: now_millis(),
    });
    agent.replace_messages(vec![message.clone()]);
    assert_eq!(agent.state().messages, vec![message.clone()]);

    let new_message = pi::agent::AgentMessage::Assistant(assistant_message("Hi"));
    agent.append_message(new_message.clone());
    assert_eq!(agent.state().messages.len(), 2);
    assert_eq!(agent.state().messages[1], new_message);

    agent.clear_messages();
    assert!(agent.state().messages.is_empty());
}

#[test]
fn should_support_steering_message_queue() {
    let agent = Agent::new(AgentOptions::default());

    let message = pi::agent::AgentMessage::User(UserMessage {
        content: UserContent::Text("Steering message".to_string()),
        timestamp: now_millis(),
    });
    agent.steer(message);

    assert!(agent.state().messages.is_empty());
}

#[test]
fn should_support_follow_up_message_queue() {
    let agent = Agent::new(AgentOptions::default());

    let message = pi::agent::AgentMessage::User(UserMessage {
        content: UserContent::Text("Follow-up message".to_string()),
        timestamp: now_millis(),
    });
    agent.follow_up(message);

    assert!(agent.state().messages.is_empty());
}

#[test]
fn should_handle_abort_controller() {
    let agent = Agent::new(AgentOptions::default());

    agent.abort();
    assert!(!agent.state().is_streaming);
}

#[test]
fn should_throw_when_prompt_called_while_streaming() {
    let agent = Agent::new(AgentOptions {
        stream_fn: Some(Box::new(|_model, _ctx| streaming_message())),
        ..AgentOptions::default()
    });

    agent.prompt("First message").expect("first prompt");
    assert!(agent.state().is_streaming);

    let err = agent.prompt("Second message").unwrap_err();
    assert!(matches!(err, AgentError::AlreadyStreaming));

    agent.abort();
    assert!(!agent.state().is_streaming);
}

#[test]
fn should_throw_when_continue_called_while_streaming() {
    let agent = Agent::new(AgentOptions {
        stream_fn: Some(Box::new(|_model, _ctx| streaming_message())),
        ..AgentOptions::default()
    });

    agent.prompt("First message").expect("first prompt");
    assert!(agent.state().is_streaming);

    let err = agent.continue_prompt().unwrap_err();
    assert!(matches!(err, AgentError::AlreadyStreamingContinue));

    agent.abort();
    assert!(!agent.state().is_streaming);
}

fn assistant_message(text: &str) -> AssistantMessage {
    AssistantMessage {
        content: vec![ContentBlock::Text {
            text: text.to_string(),
            text_signature: None,
        }],
        api: "openai-responses".to_string(),
        provider: "openai".to_string(),
        model: "mock".to_string(),
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
        stop_reason: "stop".to_string(),
        error_message: None,
        timestamp: now_millis(),
    }
}

fn streaming_message() -> AssistantMessage {
    let mut message = assistant_message("");
    message.stop_reason = "streaming".to_string();
    message
}

fn now_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}
