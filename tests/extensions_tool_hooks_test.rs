use pi::agent::{
    get_model, Agent, AgentMessage, AgentOptions, AgentStateOverride, AgentTool, AgentToolResult,
};
use pi::coding_agent::{
    AgentSession, AgentSessionConfig, AuthStorage, ExtensionHost, ModelRegistry, SettingsManager,
};
use pi::core::messages::{AssistantMessage, ContentBlock, Cost, Usage};
use pi::core::session_manager::SessionManager;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

type StreamFn = pi::agent::StreamFn;

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!("{}-{}", prefix, uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn write_extension(dir: &Path, name: &str, contents: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, contents).expect("write extension file");
    path
}

fn default_usage() -> Usage {
    Usage {
        input: 1,
        output: 1,
        cache_read: 0,
        cache_write: 0,
        total_tokens: Some(2),
        cost: Some(Cost {
            input: 0.0,
            output: 0.0,
            cache_read: 0.0,
            cache_write: 0.0,
            total: 0.0,
        }),
    }
}

fn assistant_text_message(text: &str, stop_reason: &str) -> AssistantMessage {
    AssistantMessage {
        content: vec![ContentBlock::Text {
            text: text.to_string(),
            text_signature: None,
        }],
        api: "mock".to_string(),
        provider: "mock".to_string(),
        model: "mock".to_string(),
        usage: default_usage(),
        stop_reason: stop_reason.to_string(),
        error_message: None,
        timestamp: 0,
    }
}

fn tool_call_message(tool_name: &str) -> AssistantMessage {
    AssistantMessage {
        content: vec![ContentBlock::ToolCall {
            id: "tool-1".to_string(),
            name: tool_name.to_string(),
            arguments: json!({ "value": 1 }),
            thought_signature: None,
        }],
        api: "mock".to_string(),
        provider: "mock".to_string(),
        model: "mock".to_string(),
        usage: default_usage(),
        stop_reason: "toolUse".to_string(),
        error_message: None,
        timestamp: 0,
    }
}

fn tool_call_stream_fn(tool_name: &'static str) -> Box<StreamFn> {
    Box::new(move |_model, context, _events| {
        let Some(last) = context.messages.last() else {
            return assistant_text_message("ok", "stop");
        };
        match last {
            AgentMessage::User(_) => tool_call_message(tool_name),
            AgentMessage::ToolResult(_) => assistant_text_message("done", "stop"),
            _ => assistant_text_message("ok", "stop"),
        }
    })
}

fn build_test_tool() -> AgentTool {
    AgentTool {
        name: "test_tool".to_string(),
        label: "Test Tool".to_string(),
        description: "Test tool".to_string(),
        execute: std::rc::Rc::new(|_tool_call_id, _args| {
            Ok(AgentToolResult {
                content: vec![ContentBlock::Text {
                    text: "RESULT".to_string(),
                    text_signature: None,
                }],
                details: json!({ "ok": true }),
            })
        }),
    }
}

fn create_session(tool_name: &'static str, tool: AgentTool) -> AgentSession {
    let model = get_model("mock", "test-model");
    let agent = Agent::new(AgentOptions {
        initial_state: Some(AgentStateOverride {
            system_prompt: Some("Test".to_string()),
            model: Some(model),
            tools: Some(vec![tool]),
            ..AgentStateOverride::default()
        }),
        stream_fn: Some(tool_call_stream_fn(tool_name)),
        ..AgentOptions::default()
    });

    let session_manager = SessionManager::in_memory();
    let settings_manager = SettingsManager::create("", "");
    let mut auth_storage = AuthStorage::new(PathBuf::from("auth.json"));
    auth_storage.set_runtime_api_key("mock", "test-key");
    let model_registry = ModelRegistry::new(auth_storage, None);

    AgentSession::new(AgentSessionConfig {
        agent,
        session_manager,
        settings_manager,
        model_registry,
    })
}

fn find_tool_result(messages: &[AgentMessage], tool_name: &str) -> Option<pi::ToolResultMessage> {
    messages.iter().find_map(|message| match message {
        AgentMessage::ToolResult(result) if result.tool_name == tool_name => Some(result.clone()),
        _ => None,
    })
}

#[test]
fn tool_call_can_block_execution() {
    let temp = TempDir::new("pi-tool-hooks");
    let extensions_dir = temp.path().join("extensions");
    fs::create_dir_all(&extensions_dir).expect("create extensions dir");
    let extension_path = write_extension(
        &extensions_dir,
        "block.js",
        r#"
        module.exports = function(pi) {
            pi.on("tool_call", () => ({ block: true, reason: "blocked" }));
        };
        "#,
    );

    let mut session = create_session("test_tool", build_test_tool());
    let (host, _manifest) =
        ExtensionHost::spawn(&[extension_path], temp.path()).expect("spawn extension host");
    session.set_extension_host(host);

    session.prompt("run tool").expect("prompt");
    let messages = session.messages();
    let tool_result = find_tool_result(&messages, "test_tool").expect("tool result");
    assert!(tool_result.is_error);
    let text = tool_result
        .content
        .iter()
        .find_map(|block| match block {
            ContentBlock::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .unwrap_or_default();
    assert!(text.contains("blocked"));
}

#[test]
fn tool_result_can_override_content() {
    let temp = TempDir::new("pi-tool-hooks");
    let extensions_dir = temp.path().join("extensions");
    fs::create_dir_all(&extensions_dir).expect("create extensions dir");
    let extension_path = write_extension(
        &extensions_dir,
        "override.js",
        r#"
        module.exports = function(pi) {
            pi.on("tool_result", () => ({
                content: [{ type: "text", text: "OVERRIDE" }],
            }));
        };
        "#,
    );

    let mut session = create_session("test_tool", build_test_tool());
    let (host, _manifest) =
        ExtensionHost::spawn(&[extension_path], temp.path()).expect("spawn extension host");
    session.set_extension_host(host);

    session.prompt("run tool").expect("prompt");
    let messages = session.messages();
    let tool_result = find_tool_result(&messages, "test_tool").expect("tool result");
    assert!(!tool_result.is_error);
    let text = tool_result
        .content
        .iter()
        .find_map(|block| match block {
            ContentBlock::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .unwrap_or_default();
    assert_eq!(text, "OVERRIDE");
}
