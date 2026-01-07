use pi::agent::{get_model, Agent, AgentOptions, AgentStateOverride};
use pi::coding_agent::{
    AgentSession, AgentSessionConfig, AuthStorage, ModelRegistry, SettingsManager,
};
use pi::core::messages::{AssistantMessage, ContentBlock, Cost, Usage};
use pi::core::session_manager::SessionManager;
use std::path::PathBuf;

type StreamFn = Box<pi::agent::StreamFn>;

fn make_assistant_message(text: &str, stop_reason: &str) -> AssistantMessage {
    AssistantMessage {
        content: vec![ContentBlock::Text {
            text: text.to_string(),
            text_signature: None,
        }],
        api: "anthropic-messages".to_string(),
        provider: "anthropic".to_string(),
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
        stop_reason: stop_reason.to_string(),
        error_message: None,
        timestamp: 0,
    }
}

fn create_session(streaming: bool) -> AgentSession {
    let model = get_model("anthropic", "claude-sonnet-4-5");
    let stream_fn: StreamFn = Box::new(move |_model, _context, _events| {
        if streaming {
            make_assistant_message("", "streaming")
        } else {
            make_assistant_message("Done", "stop")
        }
    });

    let agent = Agent::new(AgentOptions {
        initial_state: Some(AgentStateOverride {
            model: Some(model),
            system_prompt: Some("Test".to_string()),
            tools: Some(Vec::new()),
            ..Default::default()
        }),
        stream_fn: Some(stream_fn),
        ..Default::default()
    });

    let session_manager = SessionManager::in_memory();
    let settings_manager = SettingsManager::create("", "");
    let mut auth_storage = AuthStorage::new(PathBuf::from("auth.json"));
    auth_storage.set_runtime_api_key("anthropic", "test-key");
    let model_registry = ModelRegistry::new(auth_storage, None);

    AgentSession::new(AgentSessionConfig {
        agent,
        session_manager,
        settings_manager,
        model_registry,
    })
}

#[test]
fn should_throw_when_prompt_called_while_streaming() {
    let mut session = create_session(true);

    session.prompt("First message").unwrap();
    assert!(session.is_streaming());

    let err = session.prompt("Second message").unwrap_err();
    assert!(err.to_string().contains("Agent is already processing"));
}

#[test]
fn should_allow_steer_while_streaming() {
    let mut session = create_session(true);
    session.prompt("First message").unwrap();
    assert!(session.is_streaming());

    session.steer("Steering message");
    assert_eq!(session.pending_message_count(), 1);
}

#[test]
fn should_allow_follow_up_while_streaming() {
    let mut session = create_session(true);
    session.prompt("First message").unwrap();
    assert!(session.is_streaming());

    session.follow_up("Follow-up message");
    assert_eq!(session.pending_message_count(), 1);
}

#[test]
fn should_allow_prompt_after_previous_completes() {
    let mut session = create_session(false);
    session.prompt("First message").unwrap();
    assert!(!session.is_streaming());
    assert!(session.prompt("Second message").is_ok());
}
