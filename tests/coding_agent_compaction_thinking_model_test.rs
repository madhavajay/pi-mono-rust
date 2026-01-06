use pi::agent::{
    get_model, Agent, AgentOptions, AgentStateOverride, LlmContext, Model, ThinkingLevel,
};
use pi::coding_agent::{
    AgentSession, AgentSessionConfig, AuthStorage, ModelRegistry, SettingsManager,
};
use pi::core::messages::{AssistantMessage, ContentBlock, Cost, Usage};
use pi::core::session_manager::SessionManager;
use std::path::PathBuf;

// Source: packages/coding-agent/test/compaction-thinking-model.test.ts

type StreamFn = Box<dyn FnMut(&Model, &LlmContext) -> AssistantMessage>;

fn make_assistant_message(text: &str) -> AssistantMessage {
    AssistantMessage {
        content: vec![ContentBlock::Text {
            text: text.to_string(),
            text_signature: None,
        }],
        api: "anthropic-messages".to_string(),
        provider: "anthropic".to_string(),
        model: "mock".to_string(),
        usage: Usage {
            input: 10,
            output: 5,
            cache_read: 0,
            cache_write: 0,
            total_tokens: Some(15),
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
        timestamp: 0,
    }
}

fn create_session(provider: &str, model_id: &str, thinking_level: ThinkingLevel) -> AgentSession {
    let model = get_model(provider, model_id);
    let stream_fn: StreamFn = Box::new(move |_model, _context| make_assistant_message("ok"));

    let agent = Agent::new(AgentOptions {
        initial_state: Some(AgentStateOverride {
            model: Some(model),
            system_prompt: Some("Test".to_string()),
            tools: Some(Vec::new()),
            thinking_level: Some(thinking_level),
            ..Default::default()
        }),
        stream_fn: Some(stream_fn),
        ..Default::default()
    });

    let session_manager = SessionManager::in_memory();
    let settings_manager = SettingsManager::create("", "");
    let mut auth_storage = AuthStorage::new(PathBuf::from("auth.json"));
    auth_storage.set_runtime_api_key(provider, "test-key");
    let model_registry = ModelRegistry::new(auth_storage, None);

    AgentSession::new(AgentSessionConfig {
        agent,
        session_manager,
        settings_manager,
        model_registry,
    })
}

#[test]
fn should_compact_successfully_with_claude_opus_4_5_thinking_and_thinking_level_high() {
    let mut session = create_session("anthropic", "claude-opus-4-5-thinking", ThinkingLevel::High);

    session
        .prompt("Write down the first 10 prime numbers.")
        .unwrap();

    let messages = session.messages();
    assert!(!messages.is_empty());
    assert!(messages.iter().any(|msg| msg.role() == "assistant"));

    let result = session.compact().unwrap();
    assert!(!result.summary.is_empty());
    assert!(result.tokens_before > 0);

    let messages_after = session.messages();
    assert!(!messages_after.is_empty());
    assert_eq!(messages_after[0].role(), "compactionSummary");
}

#[test]
fn should_compact_successfully_with_claude_sonnet_4_5_non_thinking_for_comparison() {
    let mut session = create_session("anthropic", "claude-sonnet-4-5", ThinkingLevel::Off);

    session
        .prompt("Write down the first 10 prime numbers.")
        .unwrap();

    let messages = session.messages();
    assert!(!messages.is_empty());

    let result = session.compact().unwrap();
    assert!(!result.summary.is_empty());
}

#[test]
fn should_compact_successfully_with_claude_3_7_sonnet_and_thinking_level_high() {
    let mut session = create_session("anthropic", "claude-3-7-sonnet-latest", ThinkingLevel::High);

    session
        .prompt("Write down the first 10 prime numbers.")
        .unwrap();

    let messages = session.messages();
    assert!(!messages.is_empty());
    assert!(messages.iter().any(|msg| msg.role() == "assistant"));

    let result = session.compact().unwrap();
    assert!(!result.summary.is_empty());
    assert!(result.tokens_before > 0);

    let messages_after = session.messages();
    assert!(!messages_after.is_empty());
    assert_eq!(messages_after[0].role(), "compactionSummary");
}
