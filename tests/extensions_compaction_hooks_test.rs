use pi::agent::{get_model, Agent, AgentOptions, AgentStateOverride, LlmContext, Model};
use pi::coding_agent::{
    AgentSession, AgentSessionConfig, AuthStorage, ExtensionHost, ModelRegistry, SettingsManager,
    SettingsOverrides,
};
use pi::core::messages::{AssistantMessage, ContentBlock, Cost, Usage};
use pi::core::session_manager::SessionManager;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

type StreamFn = Box<dyn FnMut(&Model, &LlmContext) -> AssistantMessage>;

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> Self {
        let mut path = env::temp_dir();
        path.push(format!("pi-ext-compaction-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}

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

fn create_session() -> AgentSession {
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

    let session_manager = SessionManager::in_memory();
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

fn spawn_host_or_skip(path: &Path, cwd: &Path) -> Option<ExtensionHost> {
    match ExtensionHost::spawn(&[path.to_path_buf()], cwd) {
        Ok((host, _manifest)) => Some(host),
        Err(err) => {
            eprintln!("Skipping extension host test: {err}");
            None
        }
    }
}

#[test]
fn extension_can_cancel_compaction() {
    let temp = TempDir::new();
    let extension_path = temp.path.join("cancel.js");
    write_file(
        &extension_path,
        "module.exports = (pi) => { pi.on('session_before_compact', async () => ({ cancel: true })); };",
    );

    let host = match spawn_host_or_skip(&extension_path, &temp.path) {
        Some(host) => host,
        None => return,
    };

    let mut session = create_session();
    session.set_extension_host(host);

    session.prompt("What is 2+2?").unwrap();
    let err = session.compact().unwrap_err();
    assert!(err.to_string().contains("Compaction cancelled"));
}

#[test]
fn extension_can_override_compaction_summary() {
    let temp = TempDir::new();
    let extension_path = temp.path.join("summary.js");
    write_file(
        &extension_path,
        "module.exports = (pi) => { pi.on('session_before_compact', async (event) => ({ compaction: { summary: 'From ext', firstKeptEntryId: event.preparation.firstKeptEntryId, tokensBefore: event.preparation.tokensBefore } })); };",
    );

    let host = match spawn_host_or_skip(&extension_path, &temp.path) {
        Some(host) => host,
        None => return,
    };

    let mut session = create_session();
    session.set_extension_host(host);

    session.prompt("What is 2+2?").unwrap();
    session.prompt("What is 3+3?").unwrap();
    let result = session.compact().unwrap();
    assert_eq!(result.summary, "From ext");
}
