use pi::agent::{get_model, Agent, AgentOptions, AgentStateOverride, LlmContext, Model};
use pi::coding_agent::{
    AgentSession, AgentSessionConfig, AgentSessionEvent, AuthStorage, ModelRegistry,
    SettingsManager, SettingsOverrides,
};
use pi::core::messages::{AssistantMessage, ContentBlock, Cost, Usage};
use pi::core::session_manager::{SessionEntry, SessionManager};
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use uuid::Uuid;

// Source: packages/coding-agent/test/agent-session-compaction.test.ts

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

fn create_temp_dir(prefix: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    let suffix = Uuid::new_v4();
    dir.push(format!("{prefix}-{suffix}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn create_session(persist: bool, temp_dir: Option<&Path>) -> AgentSession {
    let model = get_model("anthropic", "claude-sonnet-4-5");
    let stream_fn: StreamFn = Box::new(move |_model, _context| make_assistant_message("ok"));

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

    let session_manager = if persist {
        let temp_dir = temp_dir.expect("temp dir");
        let session_file = temp_dir.join("session.jsonl");
        SessionManager::open(session_file, Some(temp_dir.to_path_buf()))
    } else {
        SessionManager::in_memory()
    };

    let mut settings_manager = SettingsManager::create("", "");
    settings_manager.apply_overrides(SettingsOverrides {
        compaction: Some(pi::coding_agent::CompactionOverrides {
            enabled: Some(true),
            reserve_tokens: None,
            keep_recent_tokens: Some(1),
        }),
    });

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
fn should_trigger_manual_compaction_via_compact() {
    let temp_dir = create_temp_dir("pi-compaction-test");
    let mut session = create_session(true, Some(&temp_dir));

    session.prompt("What is 2+2?").unwrap();
    session.prompt("What is 3+3?").unwrap();

    let result = session.compact().unwrap();
    assert!(!result.summary.is_empty());
    assert!(result.tokens_before > 0);

    let messages = session.messages();
    assert!(!messages.is_empty());
    assert_eq!(messages[0].role(), "compactionSummary");

    session.dispose();
    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn should_maintain_valid_session_state_after_compaction() {
    let temp_dir = create_temp_dir("pi-compaction-test");
    let mut session = create_session(true, Some(&temp_dir));

    session.prompt("What is the capital of France?").unwrap();
    session.prompt("What is the capital of Germany?").unwrap();

    session.compact().unwrap();

    session.prompt("What is the capital of Italy?").unwrap();
    let messages = session.messages();
    assert!(!messages.is_empty());
    let assistant_count = messages
        .iter()
        .filter(|message| message.role() == "assistant")
        .count();
    assert!(assistant_count > 0);

    session.dispose();
    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn should_persist_compaction_to_session_file() {
    let temp_dir = create_temp_dir("pi-compaction-test");
    let mut session = create_session(true, Some(&temp_dir));

    session.prompt("Say hello").unwrap();
    session.prompt("Say goodbye").unwrap();

    session.compact().unwrap();

    let entries = session.session_manager.get_entries();
    let compaction_entries: Vec<_> = entries
        .iter()
        .filter(|entry| matches!(entry, SessionEntry::Compaction(_)))
        .collect();
    assert_eq!(compaction_entries.len(), 1);

    session.dispose();
    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn should_work_with_no_session_mode_in_memory_only() {
    let mut session = create_session(false, None);

    session.prompt("What is 2+2?").unwrap();
    session.prompt("What is 3+3?").unwrap();

    let result = session.compact().unwrap();
    assert!(!result.summary.is_empty());

    let entries = session.session_manager.get_entries();
    let compaction_entries: Vec<_> = entries
        .iter()
        .filter(|entry| matches!(entry, SessionEntry::Compaction(_)))
        .collect();
    assert_eq!(compaction_entries.len(), 1);
}

#[test]
fn should_emit_correct_events_during_auto_compaction() {
    let temp_dir = create_temp_dir("pi-compaction-test");
    let mut session = create_session(true, Some(&temp_dir));

    let events = Rc::new(RefCell::new(Vec::new()));
    let events_ref = events.clone();
    let _unsubscribe = session.subscribe(move |event| {
        events_ref.borrow_mut().push(event.clone());
    });

    session.prompt("Say hello").unwrap();
    session.compact().unwrap();

    let events_snapshot = events.borrow();
    let auto_compaction_events: Vec<_> = events_snapshot
        .iter()
        .filter(|event| {
            matches!(
                event,
                AgentSessionEvent::AutoCompactionStart { .. }
                    | AgentSessionEvent::AutoCompactionEnd { .. }
            )
        })
        .collect();
    assert!(auto_compaction_events.is_empty());

    let message_events: Vec<_> = events_snapshot
        .iter()
        .filter(|event| matches!(event, AgentSessionEvent::Agent(_)))
        .collect();
    assert!(!message_events.is_empty());

    session.dispose();
    let _ = fs::remove_dir_all(&temp_dir);
}
