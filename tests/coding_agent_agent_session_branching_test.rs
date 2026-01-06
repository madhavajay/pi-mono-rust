use pi::agent::{get_model, Agent, AgentOptions, AgentStateOverride, LlmContext, Model};
use pi::coding_agent::{
    AgentSession, AgentSessionConfig, AuthStorage, ModelRegistry, SettingsManager,
};
use pi::core::messages::{AssistantMessage, ContentBlock, Cost, Usage};
use pi::core::session_manager::SessionManager;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

// Source: packages/coding-agent/test/agent-session-branching.test.ts

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
        timestamp: 0,
    }
}

fn create_temp_dir(prefix: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    dir.push(format!("{prefix}-{millis}-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn create_session(persist: bool, temp_dir: &Path) -> AgentSession {
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
        let session_file = temp_dir.join("session.jsonl");
        SessionManager::open(session_file, Some(temp_dir.to_path_buf()))
    } else {
        SessionManager::in_memory()
    };
    let settings_manager = SettingsManager::create(
        temp_dir.to_string_lossy().to_string(),
        temp_dir.to_string_lossy().to_string(),
    );
    let mut auth_storage = AuthStorage::new(temp_dir.join("auth.json"));
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
fn should_allow_branching_from_single_message() {
    let temp_dir = create_temp_dir("pi-branching-test");
    let mut session = create_session(true, &temp_dir);
    let _unsubscribe = session.subscribe(|_| {});

    session.prompt("Say hello").unwrap();

    let user_messages = session.get_user_messages_for_branching();
    assert_eq!(user_messages.len(), 1);
    assert_eq!(user_messages[0].text, "Say hello");

    let result = session.branch(&user_messages[0].entry_id).unwrap();
    assert_eq!(result.selected_text, "Say hello");
    assert!(!result.cancelled);

    assert_eq!(session.messages().len(), 0);

    let session_file = session.session_file().expect("session file");
    assert!(session_file.exists());

    session.dispose();
    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn should_support_in_memory_branching_in_no_session_mode() {
    let temp_dir = create_temp_dir("pi-branching-test");
    let mut session = create_session(false, &temp_dir);
    let _unsubscribe = session.subscribe(|_| {});

    assert!(session.session_file().is_none());

    session.prompt("Say hi").unwrap();

    let user_messages = session.get_user_messages_for_branching();
    assert_eq!(user_messages.len(), 1);

    assert!(!session.messages().is_empty());

    let result = session.branch(&user_messages[0].entry_id).unwrap();
    assert_eq!(result.selected_text, "Say hi");
    assert!(!result.cancelled);

    assert_eq!(session.messages().len(), 0);
    assert!(session.session_file().is_none());

    session.dispose();
    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn should_branch_from_middle_of_conversation() {
    let temp_dir = create_temp_dir("pi-branching-test");
    let mut session = create_session(true, &temp_dir);
    let _unsubscribe = session.subscribe(|_| {});

    session.prompt("Say one").unwrap();
    session.prompt("Say two").unwrap();
    session.prompt("Say three").unwrap();

    let user_messages = session.get_user_messages_for_branching();
    assert_eq!(user_messages.len(), 3);

    let second_message = &user_messages[1];
    let result = session.branch(&second_message.entry_id).unwrap();
    assert_eq!(result.selected_text, "Say two");

    let messages = session.messages();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].role(), "user");
    assert_eq!(messages[1].role(), "assistant");

    session.dispose();
    let _ = fs::remove_dir_all(&temp_dir);
}
