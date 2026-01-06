use pi::agent::{
    get_model, Agent, AgentMessage, AgentOptions, AgentStateOverride, LlmContext, Model,
    ThinkingLevel,
};
use pi::coding_agent::{
    AgentSession, AgentSessionConfig, AuthStorage, ModelRegistry, SettingsManager,
};
use pi::core::messages::{AssistantMessage, ContentBlock, Cost, Usage, UserContent};
use pi::core::session_manager::{FileEntry, SessionManager};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

// Source: packages/coding-agent/test/rpc.test.ts

type StreamFn = Box<dyn FnMut(&Model, &LlmContext) -> AssistantMessage>;
type ConvertToLlmFn = Box<dyn FnMut(&[AgentMessage]) -> Vec<AgentMessage>>;

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
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    dir.push(format!("{prefix}-{millis}-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn create_session(persist: bool, temp_dir: Option<&Path>) -> AgentSession {
    let model = get_model("anthropic", "claude-sonnet-4-5");
    let stream_fn: StreamFn = Box::new(move |_model, context| {
        let mut bash_output = None;
        for message in context.messages.iter().rev() {
            if let AgentMessage::Custom(custom) = message {
                if custom.role == "bashExecution" {
                    bash_output = Some(custom.text.clone());
                    break;
                }
            }
        }
        let last_user = context.messages.iter().rev().find_map(|message| {
            if let AgentMessage::User(user) = message {
                match &user.content {
                    UserContent::Text(text) => Some(text.as_str()),
                    _ => None,
                }
            } else {
                None
            }
        });
        let response = if let Some(user_text) = last_user {
            if user_text.contains("test123") {
                "test123".to_string()
            } else if user_text.contains("echo command") || user_text.contains("echo") {
                bash_output.unwrap_or_else(|| "unknown".to_string())
            } else {
                "ok".to_string()
            }
        } else {
            "ok".to_string()
        };
        make_assistant_message(&response)
    });

    let convert_to_llm: ConvertToLlmFn = Box::new(|messages| messages.to_vec());

    let agent = Agent::new(AgentOptions {
        initial_state: Some(AgentStateOverride {
            model: Some(model),
            system_prompt: Some("Test".to_string()),
            tools: Some(Vec::new()),
            ..Default::default()
        }),
        convert_to_llm: Some(convert_to_llm),
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
fn should_get_state() {
    let session = create_session(false, None);
    let state = session.get_state();
    assert_eq!(state.model.provider, "anthropic");
    assert_eq!(state.model.id, "claude-sonnet-4-5");
    assert!(!state.is_streaming);
    assert_eq!(state.message_count, 0);
}

#[test]
fn should_save_messages_to_session_file() {
    let temp_dir = create_temp_dir("pi-rpc-test");
    let mut session = create_session(true, Some(&temp_dir));

    session.prompt("Reply with just the word 'hello'").unwrap();

    let session_file = session.session_file().expect("session file");
    assert!(session_file.exists());

    let content = fs::read_to_string(session_file).unwrap();
    let entries: Vec<FileEntry> = content
        .trim()
        .split('\n')
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();

    assert!(matches!(entries.first(), Some(FileEntry::Session(_))));
    let messages: Vec<_> = entries
        .iter()
        .filter(|entry| matches!(entry, FileEntry::Message(_)))
        .collect();
    assert!(messages.len() >= 2);

    session.dispose();
    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn should_handle_manual_compaction() {
    let temp_dir = create_temp_dir("pi-rpc-test");
    let mut session = create_session(true, Some(&temp_dir));

    session.prompt("Say hello").unwrap();
    let result = session.compact().unwrap();
    assert!(!result.summary.is_empty());
    assert!(result.tokens_before > 0);

    let session_file = session.session_file().expect("session file");
    let content = fs::read_to_string(session_file).unwrap();
    let entries: Vec<FileEntry> = content
        .trim()
        .split('\n')
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    let compaction_entries: Vec<_> = entries
        .iter()
        .filter(|entry| matches!(entry, FileEntry::Compaction(_)))
        .collect();
    assert_eq!(compaction_entries.len(), 1);

    session.dispose();
    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn should_execute_bash_command() {
    let mut session = create_session(false, None);
    let result = session.execute_bash("echo hello").unwrap();
    assert_eq!(result.output.trim(), "hello");
    assert_eq!(result.exit_code, Some(0));
    assert!(!result.cancelled);
}

#[test]
fn should_add_bash_output_to_context() {
    let mut session = create_session(false, None);
    session.prompt("Say hi").unwrap();

    let unique = format!("test-{}", now_millis());
    session.execute_bash(&format!("echo {unique}")).unwrap();

    let entries = session.session_manager.get_entries();
    let bash_messages = entries.iter().filter(|entry| match entry {
        pi::core::session_manager::SessionEntry::Message(message) => {
            matches!(
                message.message,
                pi::core::messages::AgentMessage::BashExecution(_)
            )
        }
        _ => false,
    });
    assert_eq!(bash_messages.count(), 1);
}

#[test]
fn should_include_bash_output_in_llm_context() {
    let mut session = create_session(false, None);

    let unique = format!("unique-{}", now_millis());
    session.execute_bash(&format!("echo {unique}")).unwrap();

    session
        .prompt("What was the exact output of the echo command I just ran?")
        .unwrap();

    let last_assistant = session.get_last_assistant_text().unwrap_or_default();
    assert!(last_assistant.contains(&unique));
}

#[test]
fn should_set_and_get_thinking_level() {
    let mut session = create_session(false, None);
    session.set_thinking_level(ThinkingLevel::High);
    let state = session.get_state();
    assert_eq!(state.thinking_level, ThinkingLevel::High);
}

#[test]
fn should_cycle_thinking_level() {
    let mut session = create_session(false, None);
    let initial = session.get_state().thinking_level;
    let result = session.cycle_thinking_level();
    assert_ne!(result.level, initial);
    let state = session.get_state();
    assert_eq!(state.thinking_level, result.level);
}

#[test]
fn should_get_available_models() {
    let session = create_session(false, None);
    let models = session.get_available_models();
    assert!(!models.is_empty());
    for model in models {
        assert!(!model.provider.is_empty());
        assert!(!model.id.is_empty());
        assert!(model.context_window > 0);
    }
}

#[test]
fn should_get_session_stats() {
    let mut session = create_session(false, None);
    session.prompt("Hello").unwrap();

    let stats = session.get_session_stats();
    assert!(!stats.session_id.is_empty());
    assert!(stats.user_messages >= 1);
    assert!(stats.assistant_messages >= 1);
}

#[test]
fn should_create_new_session() {
    let mut session = create_session(false, None);
    session.prompt("Hello").unwrap();
    assert!(session.get_state().message_count > 0);

    session.new_session();
    assert_eq!(session.get_state().message_count, 0);
}

#[test]
fn should_export_to_html() {
    let temp_dir = create_temp_dir("pi-rpc-export");
    let mut session = create_session(true, Some(&temp_dir));
    session.prompt("Hello").unwrap();

    let result = session.export_to_html().unwrap();
    assert_eq!(
        result.path.extension().and_then(|value| value.to_str()),
        Some("html")
    );
    assert!(result.path.exists());

    session.dispose();
    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn should_get_last_assistant_text() {
    let mut session = create_session(false, None);
    assert!(session.get_last_assistant_text().is_none());

    session.prompt("Reply with just: test123").unwrap();
    let text = session.get_last_assistant_text().unwrap_or_default();
    assert!(text.contains("test123"));
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}
