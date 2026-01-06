use pi::agent::{
    Agent, AgentMessage, AgentOptions, AgentStateOverride, AgentTool, AgentToolResult, LlmContext,
    Model as AgentModel, QueueMode, ThinkingLevel,
};
use pi::cli::args::{ExtensionFlagType, ExtensionFlagValue, ThinkingLevel as CliThinkingLevel};
use pi::cli::list_models::list_models;
use pi::coding_agent::tools as agent_tools;
use pi::coding_agent::{
    build_system_prompt, export_from_file, load_prompt_templates, AgentSession, AgentSessionConfig,
    AuthCredential, AuthStorage, BuildSystemPromptOptions, ExtensionHost,
    LoadPromptTemplatesOptions, Model as RegistryModel, ModelRegistry, SettingsManager,
};
use pi::coding_agent::{discover_extension_paths, ExtensionManifest};
use pi::config;
use pi::core::messages::{
    AgentMessage as CoreAgentMessage, AssistantMessage, ContentBlock, Cost, ToolResultMessage,
    Usage, UserContent,
};
use pi::core::session_manager::{SessionInfo, SessionManager};
use pi::tools::{default_tools, ToolDefinition};
use pi::tui::{truncate_to_width, wrap_text_with_ansi, Editor, EditorTheme};
use pi::{parse_args, Args, ListModels, Mode};
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::env;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::rc::Rc;

use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;

const DEFAULT_OAUTH_SYSTEM_PROMPT: &str =
    "You are Claude Code, Anthropic's official CLI for Claude.";

fn print_help() {
    println!(
        "pi (rust) minimal CLI

Usage:
  pi [options] [messages...]

Options:
  --help, -h       Show this help
  --version, -v    Show version
  --provider       Provider name (anthropic only for now)
  --model          Model id
  --models         Comma-separated model patterns
  --api-key        Override provider API key
  --system-prompt  Custom system prompt (literal or file path)
  --append-system-prompt  Append text to system prompt (literal or file path)
  --tools          Comma-separated tool allowlist
  --thinking       Set thinking level: off, minimal, low, medium, high, xhigh
  --print, -p      Print mode (single-shot)
  --list-models    List available models
  --export <file>  Export session file to HTML and exit
  --mode <mode>    Output mode: text (default), json, rpc
  --extension, -e  Load an extension file (can be used multiple times)
  --no-skills      Disable skills discovery and loading
  --skills         Comma-separated glob patterns to filter skills
  @file            Include file contents in prompt (text or images)

Notes:
  Interactive mode uses a basic TUI (full parity pending).
  Extensions can register additional CLI flags.
  Extension execution (compaction hooks) is supported for .js files only."
    );
}

fn collect_extension_flags(manifest: &ExtensionManifest) -> HashMap<String, ExtensionFlagType> {
    let mut flags = HashMap::new();
    for extension in &manifest.extensions {
        for flag in &extension.flags {
            let flag_type = match flag.flag_type.as_deref() {
                Some("boolean") => ExtensionFlagType::Bool,
                Some("string") => ExtensionFlagType::String,
                _ => continue,
            };
            flags.insert(flag.name.clone(), flag_type);
        }
    }
    flags
}

fn extension_flag_values_to_json(
    values: &HashMap<String, ExtensionFlagValue>,
) -> HashMap<String, Value> {
    values
        .iter()
        .map(|(name, value)| {
            let json_value = match value {
                ExtensionFlagValue::Bool(flag) => Value::Bool(*flag),
                ExtensionFlagValue::String(text) => Value::String(text.clone()),
            };
            (name.clone(), json_value)
        })
        .collect()
}

fn collect_unsupported_flags(parsed: &Args) -> Vec<&'static str> {
    let _ = parsed;
    Vec::new()
}

type AuthStorageData = HashMap<String, AuthCredential>;
type AgentStreamFn = Box<dyn FnMut(&AgentModel, &LlmContext) -> AssistantMessage>;

struct AnthropicCallOptions<'a> {
    model: &'a str,
    api_key: &'a str,
    use_oauth: bool,
    tools: &'a [AnthropicTool],
    base_url: &'a str,
    extra_headers: Option<&'a HashMap<String, String>>,
    system: Option<&'a str>,
}

struct OpenAICallOptions<'a> {
    model: &'a str,
    api_key: &'a str,
    tools: &'a [OpenAITool],
    base_url: &'a str,
    extra_headers: Option<&'a HashMap<String, String>>,
}

struct FileInputImage {
    mime_type: String,
    data: String,
}

#[derive(Debug, Serialize, Clone)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<AnthropicTool>>,
}

#[derive(Debug, Serialize, Clone)]
struct AnthropicMessage {
    role: String,
    content: Vec<AnthropicContentBlock>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicContentBlock {
    Text {
        text: String,
    },
    Image {
        source: AnthropicImageSource,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        tool_use_id: String,
        content: Vec<AnthropicToolResultContent>,
        is_error: bool,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicToolResultContent {
    Text { text: String },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct AnthropicImageSource {
    #[serde(rename = "type")]
    source_type: String,
    media_type: String,
    data: String,
}

#[derive(Debug, Deserialize)]
struct RpcImage {
    #[serde(rename = "type")]
    _image_type: Option<String>,
    data: String,
    #[serde(rename = "mimeType")]
    mime_type: String,
}

#[derive(Debug, Deserialize)]
struct RpcCommandEnvelope {
    #[serde(rename = "type")]
    command_type: String,
    #[serde(default)]
    id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RpcPromptCommand {
    message: String,
    #[serde(default)]
    images: Vec<RpcImage>,
    #[serde(rename = "streamingBehavior")]
    streaming_behavior: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RpcSteerCommand {
    #[serde(default)]
    id: Option<String>,
    message: String,
}

#[derive(Debug, Deserialize)]
struct RpcFollowUpCommand {
    #[serde(default)]
    id: Option<String>,
    message: String,
}

#[derive(Debug, Deserialize)]
struct RpcAbortCommand {
    #[serde(default)]
    id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RpcNewSessionCommand {
    #[serde(default)]
    id: Option<String>,
    #[serde(rename = "parentSession")]
    parent_session: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RpcGetStateCommand {
    #[serde(default)]
    id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RpcSetThinkingLevelCommand {
    #[serde(default)]
    id: Option<String>,
    level: String,
}

#[derive(Debug, Deserialize)]
struct RpcCycleThinkingLevelCommand {
    #[serde(default)]
    id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RpcGetAvailableModelsCommand {
    #[serde(default)]
    id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RpcSetModelCommand {
    #[serde(default)]
    id: Option<String>,
    provider: String,
    #[serde(rename = "modelId")]
    model_id: String,
}

#[derive(Debug, Deserialize)]
struct RpcCycleModelCommand {
    #[serde(default)]
    id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RpcGetSessionStatsCommand {
    #[serde(default)]
    id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RpcCompactCommand {
    #[serde(default)]
    id: Option<String>,
    #[serde(rename = "customInstructions")]
    custom_instructions: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RpcSetAutoCompactionCommand {
    #[serde(default)]
    id: Option<String>,
    enabled: bool,
}

#[derive(Debug, Deserialize)]
struct RpcSetAutoRetryCommand {
    #[serde(default)]
    id: Option<String>,
    enabled: bool,
}

#[derive(Debug, Deserialize)]
struct RpcAbortRetryCommand {
    #[serde(default)]
    id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RpcExportHtmlCommand {
    #[serde(default)]
    id: Option<String>,
    #[serde(rename = "outputPath")]
    output_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RpcBashCommand {
    #[serde(default)]
    id: Option<String>,
    command: String,
}

#[derive(Debug, Deserialize)]
struct RpcAbortBashCommand {
    #[serde(default)]
    id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RpcGetLastAssistantTextCommand {
    #[serde(default)]
    id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RpcSwitchSessionCommand {
    #[serde(default)]
    id: Option<String>,
    #[serde(rename = "sessionPath")]
    session_path: String,
}

#[derive(Debug, Deserialize)]
struct RpcBranchCommand {
    #[serde(default)]
    id: Option<String>,
    #[serde(rename = "entryId")]
    entry_id: String,
}

#[derive(Debug, Deserialize)]
struct RpcGetBranchMessagesCommand {
    #[serde(default)]
    id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RpcGetMessagesCommand {
    #[serde(default)]
    id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RpcSetSteeringModeCommand {
    #[serde(default)]
    id: Option<String>,
    mode: String,
}

#[derive(Debug, Deserialize)]
struct RpcSetFollowUpModeCommand {
    #[serde(default)]
    id: Option<String>,
    mode: String,
}

#[derive(Debug, Serialize, Clone)]
struct AnthropicTool {
    name: String,
    description: String,
    input_schema: Value,
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContentBlock>,
    stop_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicErrorResponse {
    error: AnthropicError,
}

#[derive(Debug, Deserialize)]
struct AnthropicError {
    message: String,
}

#[derive(Debug, Serialize, Clone)]
struct OpenAITool {
    #[serde(rename = "type")]
    tool_type: String,
    name: String,
    description: String,
    parameters: Value,
}

#[derive(Debug, Serialize, Clone)]
struct OpenAIRequest {
    model: String,
    input: Vec<OpenAIInputItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAITool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
enum OpenAIInputItem {
    Message {
        role: String,
        content: Vec<OpenAIMessageContent>,
    },
    FunctionCall {
        id: String,
        call_id: String,
        name: String,
        arguments: String,
    },
    FunctionCallOutput {
        call_id: String,
        output: String,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
enum OpenAIMessageContent {
    InputText {
        text: String,
    },
    OutputText {
        text: String,
    },
    InputImage {
        image_url: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
struct OpenAIResponse {
    output: Vec<OpenAIOutputItem>,
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum OpenAIOutputItem {
    Message {
        role: String,
        content: Vec<OpenAIOutputContent>,
    },
    FunctionCall {
        id: String,
        call_id: String,
        name: String,
        arguments: String,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum OpenAIOutputContent {
    OutputText {
        text: String,
    },
    Refusal {
        refusal: String,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Serialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
enum OpenAIContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
}

#[derive(Debug, Deserialize)]
struct OpenAIErrorResponse {
    error: OpenAIError,
}

#[derive(Debug, Deserialize)]
struct OpenAIError {
    message: String,
}

fn build_anthropic_headers(
    api_key: &str,
    use_oauth: bool,
    extra_headers: Option<&HashMap<String, String>>,
) -> Result<HeaderMap, String> {
    let mut headers = HeaderMap::new();
    headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
    if use_oauth {
        headers.insert(
            "anthropic-beta",
            HeaderValue::from_static("oauth-2025-04-20"),
        );
        let value = HeaderValue::from_str(&format!("Bearer {api_key}"))
            .map_err(|err| format!("Invalid OAuth token: {err}"))?;
        headers.insert("authorization", value);
    } else {
        let value =
            HeaderValue::from_str(api_key).map_err(|err| format!("Invalid API key: {err}"))?;
        headers.insert("x-api-key", value);
    }
    if let Some(extra) = extra_headers {
        for (key, value) in extra {
            let header_name = HeaderName::from_bytes(key.as_bytes())
                .map_err(|err| format!("Invalid header name \"{key}\": {err}"))?;
            let header_value = HeaderValue::from_str(value)
                .map_err(|err| format!("Invalid header value: {err}"))?;
            headers.insert(header_name, header_value);
        }
    }
    Ok(headers)
}

fn build_openai_headers(
    api_key: &str,
    extra_headers: Option<&HashMap<String, String>>,
) -> Result<HeaderMap, String> {
    let mut headers = HeaderMap::new();
    let value = HeaderValue::from_str(&format!("Bearer {api_key}"))
        .map_err(|err| format!("Invalid OpenAI API key: {err}"))?;
    headers.insert("authorization", value);
    if let Some(extra) = extra_headers {
        for (key, value) in extra {
            let header_name = HeaderName::from_bytes(key.as_bytes())
                .map_err(|err| format!("Invalid header name \"{key}\": {err}"))?;
            let header_value = HeaderValue::from_str(value)
                .map_err(|err| format!("Invalid header value: {err}"))?;
            headers.insert(header_name, header_value);
        }
    }
    Ok(headers)
}

fn env_var_non_empty(key: &str) -> Option<String> {
    env::var(key).ok().and_then(|value| {
        if value.trim().is_empty() {
            None
        } else {
            Some(value)
        }
    })
}

fn apply_env_api_keys_for_availability(auth_storage: &mut AuthStorage) {
    if !auth_storage.has("anthropic") {
        if let Some(token) = env_var_non_empty("ANTHROPIC_OAUTH_TOKEN") {
            auth_storage.set_runtime_api_key("anthropic", &token);
        } else if let Some(key) = env_var_non_empty("ANTHROPIC_API_KEY") {
            auth_storage.set_runtime_api_key("anthropic", &key);
        }
    }

    if !auth_storage.has("openai") {
        if let Some(key) = env_var_non_empty("OPENAI_API_KEY") {
            auth_storage.set_runtime_api_key("openai", &key);
        }
    }

    if !auth_storage.has("google") {
        if let Some(key) = env_var_non_empty("GEMINI_API_KEY") {
            auth_storage.set_runtime_api_key("google", &key);
        }
    }
}

fn build_model_registry(
    api_key_override: Option<&str>,
    provider: Option<&str>,
) -> Result<ModelRegistry, String> {
    let auth_path = config::get_auth_path();
    let mut auth_storage = AuthStorage::new(auth_path);
    apply_env_api_keys_for_availability(&mut auth_storage);
    if let Some(api_key) = api_key_override {
        let provider = provider.unwrap_or("anthropic");
        auth_storage.set_runtime_api_key(provider, api_key);
    }
    Ok(ModelRegistry::new(
        auth_storage,
        Some(config::get_models_path()),
    ))
}

fn read_auth_credential(provider: &str) -> Option<AuthCredential> {
    let path = config::get_auth_path();
    let content = std::fs::read_to_string(path).ok()?;
    let data: AuthStorageData = serde_json::from_str(&content).ok()?;
    data.get(provider).cloned()
}

fn resolve_anthropic_credentials(api_key_override: Option<&str>) -> Result<(String, bool), String> {
    if let Some(key) = api_key_override {
        return Ok((key.to_string(), false));
    }

    if let Some(credential) = read_auth_credential("anthropic") {
        match credential {
            AuthCredential::ApiKey { key } => return Ok((key, false)),
            AuthCredential::OAuth { access, .. } => return Ok((access, true)),
        }
    }

    if let Some(token) = env_var_non_empty("ANTHROPIC_OAUTH_TOKEN") {
        return Ok((token, true));
    }

    if let Some(key) = env_var_non_empty("ANTHROPIC_API_KEY") {
        return Ok((key, false));
    }

    Err(
        "Missing Anthropic credentials. Set ANTHROPIC_OAUTH_TOKEN or ANTHROPIC_API_KEY."
            .to_string(),
    )
}

fn resolve_openai_credentials(api_key_override: Option<&str>) -> Result<String, String> {
    if let Some(key) = api_key_override {
        return Ok(key.to_string());
    }

    if let Some(credential) = read_auth_credential("openai") {
        match credential {
            AuthCredential::ApiKey { key } => return Ok(key),
            AuthCredential::OAuth { access, .. } => return Ok(access),
        }
    }

    if let Some(key) = env_var_non_empty("OPENAI_API_KEY") {
        return Ok(key);
    }

    Err("Missing OpenAI credentials. Set OPENAI_API_KEY.".to_string())
}

fn discover_system_prompt_file() -> Option<PathBuf> {
    let cwd = env::current_dir().ok()?;
    let project_path = cwd.join(config::config_dir_name()).join("SYSTEM.md");
    if project_path.exists() {
        return Some(project_path);
    }

    let global_path = config::get_agent_dir().join("SYSTEM.md");
    if global_path.exists() {
        return Some(global_path);
    }

    None
}

fn resolve_file_arg(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }

    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else if let Ok(cwd) = env::current_dir() {
        cwd.join(path)
    } else {
        path
    }
}

struct FileInputs {
    text_prefix: String,
    images: Vec<FileInputImage>,
}

fn build_file_inputs(file_args: &[String]) -> Result<FileInputs, String> {
    let mut text = String::new();
    let mut images = Vec::new();
    for file_arg in file_args {
        let path = resolve_file_arg(file_arg);
        let data = std::fs::read(&path)
            .map_err(|err| format!("Error: Could not read file {}: {}", path.display(), err))?;
        if data.is_empty() {
            continue;
        }

        if let Some(mime_type) = detect_image_mime_type(&data) {
            let encoded = base64_encode(&data);
            images.push(FileInputImage {
                mime_type: mime_type.to_string(),
                data: encoded,
            });
            text.push_str(&format!("<file name=\"{}\"></file>\n", path.display()));
            continue;
        }

        let content = String::from_utf8(data)
            .map_err(|err| format!("Error: Could not read file {}: {}", path.display(), err))?;
        if content.trim().is_empty() {
            continue;
        }
        text.push_str(&format!("<file name=\"{}\">\n", path.display()));
        text.push_str(&content);
        if !content.ends_with('\n') {
            text.push('\n');
        }
        text.push_str("</file>\n");
    }
    Ok(FileInputs {
        text_prefix: text,
        images,
    })
}

fn call_anthropic(
    messages: Vec<AnthropicMessage>,
    options: AnthropicCallOptions<'_>,
) -> Result<AnthropicResponse, String> {
    let request = AnthropicRequest {
        model: options.model.to_string(),
        max_tokens: 1024,
        messages,
        system: options.system.map(|value| value.to_string()),
        tools: if options.tools.is_empty() {
            None
        } else {
            Some(options.tools.to_vec())
        },
    };

    let headers =
        build_anthropic_headers(options.api_key, options.use_oauth, options.extra_headers)?;
    let endpoint = format!("{}/messages", options.base_url.trim_end_matches('/'));
    let client = Client::new();
    let response = client
        .post(endpoint)
        .headers(headers)
        .json(&request)
        .send()
        .map_err(|err| format!("Request failed: {err}"))?;

    let status = response.status();
    if !status.is_success() {
        let text = response.text().unwrap_or_default();
        if let Ok(error_response) = serde_json::from_str::<AnthropicErrorResponse>(&text) {
            return Err(format!("Anthropic error: {}", error_response.error.message));
        }
        return Err(format!("Anthropic error: {} {}", status.as_u16(), text));
    }

    response
        .json::<AnthropicResponse>()
        .map_err(|err| format!("Failed to parse response: {err}"))
}

fn call_openai(
    input: Vec<OpenAIInputItem>,
    options: OpenAICallOptions<'_>,
) -> Result<OpenAIResponse, String> {
    let request = OpenAIRequest {
        model: options.model.to_string(),
        input,
        tools: if options.tools.is_empty() {
            None
        } else {
            Some(options.tools.to_vec())
        },
        stream: Some(false),
    };

    let headers = build_openai_headers(options.api_key, options.extra_headers)?;
    let endpoint = format!("{}/responses", options.base_url.trim_end_matches('/'));
    let client = Client::new();
    let response = client
        .post(endpoint)
        .headers(headers)
        .json(&request)
        .send()
        .map_err(|err| format!("Request failed: {err}"))?;

    let status = response.status();
    if !status.is_success() {
        let text = response.text().unwrap_or_default();
        if let Ok(error_response) = serde_json::from_str::<OpenAIErrorResponse>(&text) {
            return Err(format!("OpenAI error: {}", error_response.error.message));
        }
        return Err(format!("OpenAI error: {} {}", status.as_u16(), text));
    }

    response
        .json::<OpenAIResponse>()
        .map_err(|err| format!("Failed to parse response: {err}"))
}

fn build_tool_defs(tool_names: Option<&[String]>) -> Result<Vec<ToolDefinition>, String> {
    let mut tool_defs = default_tools();
    if let Some(tool_names) = tool_names {
        let missing = tool_names
            .iter()
            .filter(|name| !tool_defs.iter().any(|tool| tool.name == name.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err(format!(
                "Tool(s) not implemented yet: {}",
                missing.join(", ")
            ));
        }
        tool_defs.retain(|tool| tool_names.iter().any(|name| name == tool.name));
    }
    Ok(tool_defs)
}

fn parse_openai_tool_arguments(arguments: &str) -> Value {
    serde_json::from_str(arguments).unwrap_or_else(|_| Value::String(arguments.to_string()))
}

fn openai_output_to_content_blocks(output: &[OpenAIOutputItem]) -> Vec<OpenAIContentBlock> {
    let mut blocks = Vec::new();
    for item in output {
        match item {
            OpenAIOutputItem::Message { role, content } => {
                let _ = role;
                let mut text = String::new();
                for part in content {
                    match part {
                        OpenAIOutputContent::OutputText { text: chunk } => text.push_str(chunk),
                        OpenAIOutputContent::Refusal { refusal } => text.push_str(refusal),
                        OpenAIOutputContent::Other => {}
                    }
                }
                if !text.is_empty() {
                    blocks.push(OpenAIContentBlock::Text { text });
                }
            }
            OpenAIOutputItem::FunctionCall {
                id,
                call_id,
                name,
                arguments,
            } => {
                blocks.push(OpenAIContentBlock::ToolUse {
                    id: format!("{call_id}|{id}"),
                    name: name.clone(),
                    input: parse_openai_tool_arguments(arguments),
                });
            }
            OpenAIOutputItem::Other => {}
        }
    }
    blocks
}

fn run_print_mode_session(
    mode: Mode,
    session: &mut AgentSession,
    messages: &[String],
    initial_message: Option<String>,
    initial_images: &[FileInputImage],
) -> Result<(), String> {
    if matches!(mode, Mode::Json) {
        let _ = session.subscribe(|event| {
            if let Some(value) = serialize_agent_event(event) {
                emit_json(&value);
            }
        });
    }

    let mut sent_any = false;
    if initial_message.is_some() || !initial_images.is_empty() {
        let content = build_user_content_from_files(initial_message.as_deref(), initial_images)?;
        session
            .prompt_content(content)
            .map_err(|err| err.to_string())?;
        sent_any = true;
    }

    for message in messages {
        if message.trim().is_empty() {
            continue;
        }
        session.prompt(message).map_err(|err| err.to_string())?;
        sent_any = true;
    }

    if !sent_any {
        return Err("No messages provided.".to_string());
    }

    if matches!(mode, Mode::Text) {
        print_last_assistant_text(session)?;
    }

    Ok(())
}

struct TerminalGuard;

impl TerminalGuard {
    fn enter(stdout: &mut impl Write) -> Result<Self, String> {
        terminal::enable_raw_mode().map_err(|err| err.to_string())?;
        stdout
            .execute(EnterAlternateScreen)
            .map_err(|err| err.to_string())?;
        stdout.execute(Hide).map_err(|err| err.to_string())?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = stdout.execute(LeaveAlternateScreen);
        let _ = stdout.execute(Show);
    }
}

enum EditorAction {
    Submit,
    Exit,
    Continue,
}

fn render_interactive_ui(
    entries: &[String],
    editor: &mut Editor,
    stdout: &mut impl Write,
) -> Result<(), String> {
    let (width, height) = terminal::size().map_err(|err| err.to_string())?;
    let width = width.max(1) as usize;
    let height = height.max(1) as usize;

    let mut chat_lines = Vec::new();
    for (idx, entry) in entries.iter().enumerate() {
        chat_lines.extend(wrap_text_with_ansi(entry, width));
        if idx + 1 < entries.len() {
            chat_lines.push(String::new());
        }
    }
    if chat_lines.is_empty() {
        chat_lines.push(String::new());
    }

    let editor_lines = editor.render(width);
    let available_chat = height.saturating_sub(editor_lines.len());
    let start = chat_lines.len().saturating_sub(available_chat);
    let mut visible_chat = chat_lines[start..].to_vec();
    while visible_chat.len() < available_chat {
        visible_chat.push(String::new());
    }

    let mut lines = Vec::new();
    lines.extend(visible_chat);
    lines.extend(editor_lines);
    if lines.len() > height {
        lines.truncate(height);
    }

    stdout
        .execute(MoveTo(0, 0))
        .map_err(|err| err.to_string())?;
    stdout
        .execute(Clear(ClearType::All))
        .map_err(|err| err.to_string())?;

    for (index, line) in lines.iter().enumerate() {
        let truncated = truncate_to_width(line, width);
        if index + 1 == lines.len() {
            write!(stdout, "{truncated}").map_err(|err| err.to_string())?;
        } else {
            writeln!(stdout, "{truncated}").map_err(|err| err.to_string())?;
        }
    }
    stdout.flush().map_err(|err| err.to_string())?;
    Ok(())
}

fn build_user_entry(message: Option<&str>, images: &[FileInputImage]) -> String {
    let mut lines = Vec::new();
    if let Some(message) = message {
        if !message.trim().is_empty() {
            lines.push(message.to_string());
        }
    }
    for _ in images {
        lines.push("[image attachment]".to_string());
    }
    if lines.is_empty() {
        "[empty message]".to_string()
    } else {
        lines.join("\n")
    }
}

fn last_assistant_text(session: &AgentSession) -> Result<String, String> {
    let messages = session.messages();
    let assistant = messages.iter().rev().find_map(|message| {
        if let AgentMessage::Assistant(assistant) = message {
            Some(assistant)
        } else {
            None
        }
    });

    let assistant = assistant.ok_or_else(|| "No assistant response.".to_string())?;
    if assistant.stop_reason == "error" || assistant.stop_reason == "aborted" {
        return Err(assistant
            .error_message
            .clone()
            .unwrap_or_else(|| format!("Request {}", assistant.stop_reason)));
    }
    let mut text = String::new();
    for block in &assistant.content {
        if let ContentBlock::Text { text: chunk, .. } = block {
            text.push_str(chunk);
        }
    }
    Ok(text)
}

fn prompt_and_append_text(
    session: &mut AgentSession,
    entries: &mut Vec<String>,
    editor: &mut Editor,
    stdout: &mut impl Write,
    prompt: &str,
) -> Result<(), String> {
    entries.push(format!("You:\n{prompt}"));
    entries.push("Assistant:\n...".to_string());
    render_interactive_ui(entries, editor, stdout)?;

    if let Err(err) = session.prompt(prompt) {
        let last = entries.len().saturating_sub(1);
        if let Some(entry) = entries.get_mut(last) {
            *entry = format!("Assistant:\nError: {}", err);
        }
        render_interactive_ui(entries, editor, stdout)?;
        return Err(err.to_string());
    }

    let response = last_assistant_text(session)?;
    if let Some(entry) = entries.last_mut() {
        *entry = format!("Assistant:\n{response}");
    }
    render_interactive_ui(entries, editor, stdout)?;
    Ok(())
}

fn prompt_and_append_content(
    session: &mut AgentSession,
    entries: &mut Vec<String>,
    editor: &mut Editor,
    stdout: &mut impl Write,
    prompt: &str,
    content: UserContent,
) -> Result<(), String> {
    entries.push(format!("You:\n{prompt}"));
    entries.push("Assistant:\n...".to_string());
    render_interactive_ui(entries, editor, stdout)?;

    if let Err(err) = session.prompt_content(content) {
        let last = entries.len().saturating_sub(1);
        if let Some(entry) = entries.get_mut(last) {
            *entry = format!("Assistant:\nError: {}", err);
        }
        render_interactive_ui(entries, editor, stdout)?;
        return Err(err.to_string());
    }

    let response = last_assistant_text(session)?;
    if let Some(entry) = entries.last_mut() {
        *entry = format!("Assistant:\n{response}");
    }
    render_interactive_ui(entries, editor, stdout)?;
    Ok(())
}

fn handle_key_event(key: KeyEvent, editor: &mut Editor) -> EditorAction {
    match key.code {
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            return EditorAction::Exit;
        }
        KeyCode::Enter => {
            if key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT)
            {
                editor.handle_input("\n");
            } else {
                return EditorAction::Submit;
            }
        }
        KeyCode::Backspace => {
            if key.modifiers.contains(KeyModifiers::ALT) {
                editor.handle_input("\x1b\x7f");
            } else {
                editor.handle_input("\x7f");
            }
        }
        KeyCode::Up => {
            editor.handle_input("\x1b[A");
        }
        KeyCode::Down => {
            editor.handle_input("\x1b[B");
        }
        KeyCode::Left => {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                editor.handle_input("\x1b[1;5D");
            } else {
                editor.handle_input("\x1b[D");
            }
        }
        KeyCode::Right => {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                editor.handle_input("\x1b[1;5C");
            } else {
                editor.handle_input("\x1b[C");
            }
        }
        KeyCode::Tab => {
            editor.handle_input("\t");
        }
        KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            editor.handle_input("\x01");
        }
        KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            editor.handle_input("\x17");
        }
        KeyCode::Char(ch) => {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                return EditorAction::Continue;
            }
            editor.handle_input(&ch.to_string());
        }
        _ => {}
    }
    EditorAction::Continue
}

fn run_interactive_mode_session(
    session: &mut AgentSession,
    messages: &[String],
    initial_message: Option<String>,
    initial_images: &[FileInputImage],
) -> Result<(), String> {
    let mut entries = Vec::new();
    let mut editor = Editor::new(EditorTheme {
        border_color: |text| text.to_string(),
    });

    let mut stdout = io::stdout();
    let _guard = TerminalGuard::enter(&mut stdout)?;

    if initial_message.is_some() || !initial_images.is_empty() {
        let prompt = build_user_entry(initial_message.as_deref(), initial_images);
        let content = build_user_content_from_files(initial_message.as_deref(), initial_images)?;
        prompt_and_append_content(
            session,
            &mut entries,
            &mut editor,
            &mut stdout,
            &prompt,
            content,
        )?;
    }

    for message in messages {
        if message.trim().is_empty() {
            continue;
        }
        prompt_and_append_text(session, &mut entries, &mut editor, &mut stdout, message)?;
    }

    render_interactive_ui(&entries, &mut editor, &mut stdout)?;

    loop {
        match event::read().map_err(|err| err.to_string())? {
            Event::Key(key) => match handle_key_event(key, &mut editor) {
                EditorAction::Exit => break,
                EditorAction::Submit => {
                    let text = editor.get_text();
                    let prompt = text.trim_end().to_string();
                    let trimmed = prompt.trim();
                    editor.set_text("");
                    if trimmed.is_empty() {
                        render_interactive_ui(&entries, &mut editor, &mut stdout)?;
                        continue;
                    }
                    if matches!(trimmed, "/exit" | "/quit") {
                        break;
                    }
                    editor.add_to_history(&prompt);
                    prompt_and_append_text(
                        session,
                        &mut entries,
                        &mut editor,
                        &mut stdout,
                        &prompt,
                    )?
                }
                EditorAction::Continue => {
                    render_interactive_ui(&entries, &mut editor, &mut stdout)?;
                }
            },
            Event::Resize(_, _) => {
                render_interactive_ui(&entries, &mut editor, &mut stdout)?;
            }
            _ => {}
        }
    }

    Ok(())
}

fn print_last_assistant_text(session: &AgentSession) -> Result<(), String> {
    let messages = session.messages();
    let assistant = messages.iter().rev().find_map(|message| {
        if let AgentMessage::Assistant(assistant) = message {
            Some(assistant)
        } else {
            None
        }
    });

    let assistant = assistant.ok_or_else(|| "No assistant response.".to_string())?;
    if assistant.stop_reason == "error" || assistant.stop_reason == "aborted" {
        return Err(assistant
            .error_message
            .clone()
            .unwrap_or_else(|| format!("Request {}", assistant.stop_reason)));
    }
    for block in &assistant.content {
        if let ContentBlock::Text { text, .. } = block {
            println!("{text}");
        }
    }
    Ok(())
}

fn build_user_content_from_files(
    message: Option<&str>,
    images: &[FileInputImage],
) -> Result<UserContent, String> {
    let mut blocks = Vec::new();
    if let Some(message) = message {
        if !message.trim().is_empty() {
            blocks.push(ContentBlock::Text {
                text: message.to_string(),
                text_signature: None,
            });
        }
    }
    for image in images {
        blocks.push(ContentBlock::Image {
            data: image.data.clone(),
            mime_type: image.mime_type.clone(),
        });
    }
    if blocks.is_empty() {
        return Err("No prompt content provided.".to_string());
    }
    Ok(UserContent::Blocks(blocks))
}

fn openai_context_to_input_items(
    model: &RegistryModel,
    context: &LlmContext,
) -> Vec<OpenAIInputItem> {
    let mut items = Vec::new();
    if !context.system_prompt.trim().is_empty() {
        let role = if model.reasoning {
            "developer"
        } else {
            "system"
        };
        items.push(OpenAIInputItem::Message {
            role: role.to_string(),
            content: vec![OpenAIMessageContent::InputText {
                text: context.system_prompt.clone(),
            }],
        });
    }

    let supports_images = model.input.iter().any(|entry| entry == "image");
    for message in &context.messages {
        match message {
            AgentMessage::User(user) => {
                let parts = openai_user_content_parts(&user.content, supports_images);
                if !parts.is_empty() {
                    items.push(OpenAIInputItem::Message {
                        role: "user".to_string(),
                        content: parts,
                    });
                }
            }
            AgentMessage::Assistant(assistant) => {
                items.extend(openai_assistant_items(assistant));
            }
            AgentMessage::ToolResult(result) => {
                let (call_id, _) = split_openai_tool_call_id(&result.tool_call_id);
                items.push(OpenAIInputItem::FunctionCallOutput {
                    call_id,
                    output: tool_result_text(&result.content),
                });
            }
            AgentMessage::Custom(_) => {}
        }
    }

    items
}

fn openai_user_content_parts(
    content: &UserContent,
    supports_images: bool,
) -> Vec<OpenAIMessageContent> {
    match content {
        UserContent::Text(text) => {
            if text.trim().is_empty() {
                Vec::new()
            } else {
                vec![OpenAIMessageContent::InputText { text: text.clone() }]
            }
        }
        UserContent::Blocks(blocks) => {
            let mut parts = Vec::new();
            for block in blocks {
                match block {
                    ContentBlock::Text { text, .. } => {
                        if !text.trim().is_empty() {
                            parts.push(OpenAIMessageContent::InputText { text: text.clone() });
                        }
                    }
                    ContentBlock::Image { data, mime_type } => {
                        if supports_images {
                            parts.push(OpenAIMessageContent::InputImage {
                                image_url: format!("data:{};base64,{}", mime_type, data),
                                detail: Some("auto".to_string()),
                            });
                        }
                    }
                    _ => {}
                }
            }
            parts
        }
    }
}

fn openai_assistant_items(assistant: &AssistantMessage) -> Vec<OpenAIInputItem> {
    let mut items = Vec::new();
    let mut content = Vec::new();

    for block in &assistant.content {
        match block {
            ContentBlock::Text { text, .. } => {
                content.push(OpenAIMessageContent::OutputText { text: text.clone() });
            }
            ContentBlock::Thinking { thinking, .. } => {
                content.push(OpenAIMessageContent::OutputText {
                    text: thinking.clone(),
                });
            }
            ContentBlock::ToolCall {
                id,
                name,
                arguments,
                ..
            } => {
                if !content.is_empty() {
                    items.push(OpenAIInputItem::Message {
                        role: "assistant".to_string(),
                        content,
                    });
                    content = Vec::new();
                }
                let (call_id, tool_id) = split_openai_tool_call_id(id);
                items.push(OpenAIInputItem::FunctionCall {
                    id: tool_id,
                    call_id,
                    name: name.clone(),
                    arguments: openai_arguments_string(arguments),
                });
            }
            ContentBlock::Image { .. } => {}
        }
    }

    if !content.is_empty() {
        items.push(OpenAIInputItem::Message {
            role: "assistant".to_string(),
            content,
        });
    }

    items
}

fn openai_arguments_string(arguments: &Value) -> String {
    match arguments {
        Value::String(value) => value.clone(),
        _ => serde_json::to_string(arguments).unwrap_or_else(|_| arguments.to_string()),
    }
}

fn split_openai_tool_call_id(value: &str) -> (String, String) {
    match value.split_once('|') {
        Some((call_id, tool_id)) => (call_id.to_string(), tool_id.to_string()),
        None => (value.to_string(), value.to_string()),
    }
}

fn tool_result_text(content: &[ContentBlock]) -> String {
    let mut text = String::new();
    for block in content {
        if let ContentBlock::Text { text: chunk, .. } = block {
            text.push_str(chunk);
        }
    }
    text
}

fn openai_assistant_message_from_response(
    model: &RegistryModel,
    response: OpenAIResponse,
) -> Result<AssistantMessage, String> {
    let content_blocks = openai_output_to_content_blocks(&response.output);
    let content = content_blocks
        .into_iter()
        .map(|block| match block {
            OpenAIContentBlock::Text { text } => ContentBlock::Text {
                text,
                text_signature: None,
            },
            OpenAIContentBlock::ToolUse { id, name, input } => ContentBlock::ToolCall {
                id,
                name,
                arguments: input,
                thought_signature: None,
            },
        })
        .collect::<Vec<_>>();

    let has_tool_calls = content
        .iter()
        .any(|block| matches!(block, ContentBlock::ToolCall { .. }));
    let stop_reason = match response.status.as_deref() {
        Some("completed") | None => "stop",
        Some("incomplete") => "length",
        Some("failed") | Some("cancelled") => "error",
        Some("queued") | Some("in_progress") => "stop",
        Some(other) => {
            return Err(format!("Unhandled OpenAI response status: {other}"));
        }
    };
    let stop_reason = if has_tool_calls && stop_reason == "stop" {
        "toolUse"
    } else {
        stop_reason
    };

    Ok(AssistantMessage {
        content,
        api: model.api.clone(),
        provider: model.provider.clone(),
        model: model.id.clone(),
        usage: empty_usage(),
        stop_reason: stop_reason.to_string(),
        error_message: None,
        timestamp: now_millis(),
    })
}

fn build_session_manager(parsed: &Args, cwd: &Path) -> SessionManager {
    if parsed.no_session {
        return SessionManager::in_memory();
    }
    if let Some(session) = &parsed.session {
        return SessionManager::open(
            PathBuf::from(session),
            parsed.session_dir.as_ref().map(PathBuf::from),
        );
    }
    if parsed.continue_session {
        return SessionManager::continue_recent(
            cwd.to_path_buf(),
            parsed.session_dir.as_ref().map(PathBuf::from),
        );
    }
    if let Some(session_dir) = &parsed.session_dir {
        return SessionManager::create_with_dir(cwd.to_path_buf(), PathBuf::from(session_dir));
    }
    SessionManager::create(cwd.to_path_buf())
}

fn select_resume_session(cwd: &Path, session_dir: Option<&str>) -> Result<Option<PathBuf>, String> {
    let sessions = SessionManager::list(cwd, session_dir.map(PathBuf::from));
    if sessions.is_empty() {
        println!("No sessions found");
        return Ok(None);
    }

    let selection = prompt_for_session(&sessions)?;
    if selection.is_none() {
        println!("No session selected");
    }
    Ok(selection)
}

fn prompt_for_session(sessions: &[SessionInfo]) -> Result<Option<PathBuf>, String> {
    println!("Select a session to resume:");
    for (idx, session) in sessions.iter().enumerate() {
        let preview = truncate_preview(&session.first_message, 80);
        let modified = format_modified_time(session.modified);
        println!(
            "{:>2}) {} (messages: {}, modified: {})",
            idx + 1,
            preview,
            session.message_count,
            modified
        );
    }

    loop {
        print!("Enter number to resume (or press Enter to cancel): ");
        io::stdout()
            .flush()
            .map_err(|err| format!("Failed to prompt: {err}"))?;
        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .map_err(|err| format!("Failed to read input: {err}"))?;
        let trimmed = input.trim();
        if trimmed.is_empty()
            || trimmed.eq_ignore_ascii_case("q")
            || trimmed.eq_ignore_ascii_case("quit")
        {
            return Ok(None);
        }
        let parsed = trimmed.parse::<usize>();
        let Ok(index) = parsed else {
            println!(
                "Invalid selection. Enter a number between 1 and {}.",
                sessions.len()
            );
            continue;
        };
        if index == 0 || index > sessions.len() {
            println!(
                "Invalid selection. Enter a number between 1 and {}.",
                sessions.len()
            );
            continue;
        }
        return Ok(Some(sessions[index - 1].path.clone()));
    }
}

fn format_modified_time(time: std::time::SystemTime) -> String {
    let datetime: chrono::DateTime<chrono::Local> = time.into();
    datetime.format("%Y-%m-%d %H:%M").to_string()
}

fn truncate_preview(text: &str, max_len: usize) -> String {
    if text.chars().count() <= max_len {
        return text.to_string();
    }
    let mut truncated = text.chars().take(max_len).collect::<String>();
    truncated.push_str("...");
    truncated
}

fn cli_thinking_level(level: &CliThinkingLevel) -> ThinkingLevel {
    match level {
        CliThinkingLevel::Off => ThinkingLevel::Off,
        CliThinkingLevel::Minimal => ThinkingLevel::Minimal,
        CliThinkingLevel::Low => ThinkingLevel::Low,
        CliThinkingLevel::Medium => ThinkingLevel::Medium,
        CliThinkingLevel::High => ThinkingLevel::High,
        CliThinkingLevel::XHigh => ThinkingLevel::XHigh,
    }
}

fn apply_cli_thinking_level(parsed: &Args, session: &mut AgentSession) {
    if let Some(level) = parsed.thinking.as_ref() {
        session.set_thinking_level(cli_thinking_level(level));
    }
}

fn attach_extensions(session: &mut AgentSession, cwd: &Path) {
    let agent_dir = config::get_agent_dir();
    let configured = session.settings_manager.get_extension_paths();
    let discovered = discover_extension_paths(&configured, cwd, &agent_dir);
    if discovered.is_empty() {
        return;
    }
    match ExtensionHost::spawn(&discovered, cwd) {
        Ok((host, manifest)) => {
            report_extension_manifest(&manifest);
            session.set_extension_host(host);
        }
        Err(err) => {
            eprintln!("Warning: Failed to load extensions: {err}");
        }
    }
}

fn attach_extensions_with_host(
    session: &mut AgentSession,
    cwd: &Path,
    preloaded: Option<(ExtensionHost, ExtensionManifest)>,
) {
    if let Some((host, manifest)) = preloaded {
        report_extension_manifest(&manifest);
        session.set_extension_host(host);
        return;
    }
    attach_extensions(session, cwd);
}

fn report_extension_manifest(manifest: &ExtensionManifest) {
    if !manifest.skipped_paths.is_empty() {
        let skipped = manifest
            .skipped_paths
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        eprintln!("Warning: Skipping unsupported extensions (JS only): {skipped}");
    }
    for error in &manifest.errors {
        eprintln!(
            "Warning: Extension {} failed to load: {}",
            error.extension_path, error.error
        );
    }
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let first_pass = parse_args(&args, None);

    let cwd = match env::current_dir() {
        Ok(cwd) => cwd,
        Err(err) => {
            eprintln!("Error: Failed to read cwd: {err}");
            process::exit(1);
        }
    };

    let settings_manager = SettingsManager::create("", "");
    let mut extension_paths = settings_manager.get_extension_paths();
    if let Some(paths) = first_pass.extensions.as_deref() {
        extension_paths.extend(paths.iter().cloned());
    }
    let discovered = discover_extension_paths(&extension_paths, &cwd, &config::get_agent_dir());
    let mut preloaded_extension: Option<(ExtensionHost, ExtensionManifest)> = None;
    let mut extension_flag_types = HashMap::new();
    if !discovered.is_empty() {
        match ExtensionHost::spawn(&discovered, &cwd) {
            Ok((host, manifest)) => {
                extension_flag_types = collect_extension_flags(&manifest);
                preloaded_extension = Some((host, manifest));
            }
            Err(err) => {
                eprintln!("Warning: Failed to load extensions: {err}");
            }
        }
    }

    let parsed = parse_args(&args, Some(&extension_flag_types));
    if let Some((host, _)) = preloaded_extension.as_mut() {
        let flag_values = extension_flag_values_to_json(&parsed.extension_flags);
        if let Err(err) = host.set_flag_values(&flag_values) {
            eprintln!("Warning: Failed to apply extension flags: {err}");
        }
    }

    if parsed.version {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return;
    }

    if parsed.help {
        print_help();
        return;
    }

    if let Some(list_models_mode) = &parsed.list_models {
        let registry = match build_model_registry(None, None) {
            Ok(registry) => registry,
            Err(message) => {
                eprintln!("Error: {message}");
                process::exit(1);
            }
        };
        let search_pattern = match list_models_mode {
            ListModels::All => None,
            ListModels::Pattern(pattern) => Some(pattern.as_str()),
        };
        list_models(&registry, search_pattern);
        return;
    }

    if let Some(export_path) = &parsed.export {
        let output_path = parsed.messages.first().map(PathBuf::from);
        match export_from_file(Path::new(export_path), output_path) {
            Ok(path) => {
                println!("Exported to: {}", path.display());
                return;
            }
            Err(message) => {
                eprintln!("Error: {message}");
                process::exit(1);
            }
        }
    }

    let unsupported = collect_unsupported_flags(&parsed);
    if !unsupported.is_empty() {
        eprintln!(
            "Error: unsupported flag(s) in rust CLI: {}",
            unsupported.join(", ")
        );
        process::exit(1);
    }
    let is_interactive = !parsed.print && parsed.mode.is_none();

    let mode = parsed.mode.clone().unwrap_or(Mode::Text);

    let provider = parsed.provider.as_deref().unwrap_or("anthropic");
    if provider != "anthropic" && provider != "openai" {
        eprintln!(
            "Error: unsupported provider \"{provider}\". Only \"anthropic\" and \"openai\" are supported."
        );
        process::exit(1);
    }

    let registry = match build_model_registry(parsed.api_key.as_deref(), Some(provider)) {
        Ok(registry) => registry,
        Err(message) => {
            eprintln!("Error: {message}");
            process::exit(1);
        }
    };

    let model = match select_model(&parsed, &registry) {
        Ok(model) => model,
        Err(message) => {
            eprintln!("Error: {message}");
            process::exit(1);
        }
    };

    if model.api != "anthropic-messages" && model.api != "openai-responses" {
        eprintln!(
            "Error: unsupported model API \"{}\". Only \"anthropic-messages\" and \"openai-responses\" are supported.",
            model.api
        );
        process::exit(1);
    }

    let system_prompt_source = if parsed.system_prompt.is_some() {
        parsed.system_prompt.clone()
    } else {
        discover_system_prompt_file().map(|path| path.to_string_lossy().to_string())
    };
    let skill_patterns = parsed.skills.clone().unwrap_or_default();
    let prompt_tools = parsed.tools.clone().unwrap_or_else(|| {
        default_tools()
            .iter()
            .map(|tool| tool.name.to_string())
            .collect()
    });
    let system_prompt = build_system_prompt(BuildSystemPromptOptions {
        custom_prompt: system_prompt_source,
        append_system_prompt: parsed.append_system_prompt.clone(),
        selected_tools: Some(prompt_tools),
        skills_enabled: !parsed.no_skills,
        skills_include: skill_patterns,
        cwd: Some(cwd.clone()),
        agent_dir: Some(config::get_agent_dir()),
        ..Default::default()
    });
    let session_manager = if parsed.resume {
        match select_resume_session(&cwd, parsed.session_dir.as_deref()) {
            Ok(Some(path)) => SessionManager::open(path, None),
            Ok(None) => return,
            Err(message) => {
                eprintln!("Error: {message}");
                process::exit(1);
            }
        }
    } else {
        build_session_manager(&parsed, &cwd)
    };

    if matches!(mode, Mode::Rpc) {
        if !parsed.file_args.is_empty() {
            eprintln!("Error: @file arguments are not supported in RPC mode.");
            process::exit(1);
        }
        if model.api != "anthropic-messages" && model.api != "openai-responses" {
            eprintln!(
                "Error: RPC mode currently supports only \"anthropic-messages\" and \"openai-responses\" models."
            );
            process::exit(1);
        }
        let mut session = match create_rpc_session(
            model,
            registry,
            Some(system_prompt),
            None,
            parsed.tools.as_deref(),
            parsed.api_key.as_deref(),
            session_manager,
        ) {
            Ok(session) => session,
            Err(message) => {
                eprintln!("Error: {message}");
                process::exit(1);
            }
        };
        if let Some(paths) = parsed.extensions.as_deref() {
            session.settings_manager.set_extension_paths(paths.to_vec());
        }
        apply_cli_thinking_level(&parsed, &mut session);
        attach_extensions_with_host(&mut session, &cwd, preloaded_extension.take());
        if let Err(message) = run_rpc_mode(session) {
            eprintln!("Error: {message}");
            process::exit(1);
        }
        return;
    }

    let mut messages = parsed.messages.clone();
    let mut initial_message = None;
    let mut initial_images = Vec::new();
    if !parsed.file_args.is_empty() {
        let inputs = match build_file_inputs(&parsed.file_args) {
            Ok(inputs) => inputs,
            Err(message) => {
                eprintln!("{message}");
                process::exit(1);
            }
        };
        if !inputs.text_prefix.is_empty() || !inputs.images.is_empty() {
            initial_message = if messages.is_empty() {
                Some(inputs.text_prefix)
            } else {
                let first = messages.remove(0);
                Some(format!("{}{}", inputs.text_prefix, first))
            };
            if !inputs.images.is_empty() {
                initial_images = inputs.images;
            }
        }
    }

    let mut session = match create_cli_session(
        model,
        registry,
        Some(system_prompt),
        None,
        parsed.tools.as_deref(),
        parsed.api_key.as_deref(),
        session_manager,
    ) {
        Ok(session) => session,
        Err(message) => {
            eprintln!("Error: {message}");
            process::exit(1);
        }
    };
    if let Some(paths) = parsed.extensions.as_deref() {
        session.settings_manager.set_extension_paths(paths.to_vec());
    }
    apply_cli_thinking_level(&parsed, &mut session);
    attach_extensions_with_host(&mut session, &cwd, preloaded_extension.take());

    let result = if is_interactive {
        run_interactive_mode_session(&mut session, &messages, initial_message, &initial_images)
    } else {
        run_print_mode_session(
            mode,
            &mut session,
            &messages,
            initial_message,
            &initial_images,
        )
    };

    if let Err(message) = result {
        eprintln!("Error: {message}");
        process::exit(1);
    }
}

fn select_model(parsed: &Args, registry: &ModelRegistry) -> Result<RegistryModel, String> {
    if let (Some(provider), Some(model_id)) = (&parsed.provider, &parsed.model) {
        return registry
            .find(provider, model_id)
            .ok_or_else(|| format!("Model {provider}/{model_id} not found"));
    }

    if let Some(patterns) = &parsed.models {
        let available = registry.get_available();
        let scoped = pi::coding_agent::resolve_model_scope(patterns, &available);
        if let Some(first) = scoped.first() {
            return Ok(first.model.clone());
        }
        return Err(format!(
            "No models match pattern(s): {}",
            patterns.join(", ")
        ));
    }

    let available = registry.get_available();
    if let Some(model) = available
        .iter()
        .find(|model| model.provider == "anthropic" && model.id == "claude-opus-4-5")
    {
        return Ok(model.clone());
    }

    available
        .into_iter()
        .next()
        .ok_or_else(|| "No models available. Set an API key in auth.json or env.".to_string())
}

fn run_rpc_mode(mut session: pi::coding_agent::AgentSession) -> Result<(), String> {
    let _unsubscribe = session.subscribe(|event| {
        if let Some(value) = serialize_agent_event(event) {
            emit_json(&value);
        }
    });

    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(err) => return Err(format!("Failed to read stdin: {err}")),
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let value: Value = match serde_json::from_str(trimmed) {
            Ok(value) => value,
            Err(err) => {
                emit_json(&response_error(
                    None,
                    "parse",
                    &format!("Failed to parse command: {err}"),
                ));
                continue;
            }
        };

        let envelope: RpcCommandEnvelope = match serde_json::from_value(value.clone()) {
            Ok(envelope) => envelope,
            Err(err) => {
                emit_json(&response_error(
                    None,
                    "parse",
                    &format!("Failed to parse command envelope: {err}"),
                ));
                continue;
            }
        };

        if envelope.command_type == "hook_ui_response" {
            continue;
        }

        handle_rpc_command(
            &mut session,
            envelope.command_type.as_str(),
            value,
            envelope.id,
        );
    }

    Ok(())
}

fn handle_rpc_command(
    session: &mut pi::coding_agent::AgentSession,
    command_type: &str,
    payload: Value,
    id: Option<String>,
) {
    match command_type {
        "prompt" => {
            let command: RpcPromptCommand = match serde_json::from_value(payload) {
                Ok(command) => command,
                Err(err) => {
                    emit_json(&response_error(id.as_deref(), "prompt", &err.to_string()));
                    return;
                }
            };
            emit_json(&response_success(id.as_deref(), "prompt", None));

            let mut handle_prompt = || {
                if let Some(behavior) = command.streaming_behavior.as_deref() {
                    if session.is_streaming() {
                        match behavior {
                            "steer" => {
                                session.steer(&command.message);
                                return Ok(());
                            }
                            "followUp" | "follow_up" => {
                                session.follow_up(&command.message);
                                return Ok(());
                            }
                            other => {
                                return Err(format!(
                                    "Unknown streamingBehavior \"{other}\". Use \"steer\" or \"followUp\"."
                                ));
                            }
                        }
                    }
                }

                if command.images.is_empty() {
                    session
                        .prompt(&command.message)
                        .map_err(|err| err.to_string())
                } else {
                    let content = build_user_content(&command.message, &command.images)?;
                    session
                        .prompt_content(content)
                        .map_err(|err| err.to_string())
                }
            };

            if let Err(err) = handle_prompt() {
                emit_json(&response_error(id.as_deref(), "prompt", &err));
            }
        }
        "steer" => {
            let command: RpcSteerCommand = match serde_json::from_value(payload) {
                Ok(command) => command,
                Err(err) => {
                    emit_json(&response_error(id.as_deref(), "steer", &err.to_string()));
                    return;
                }
            };
            session.steer(&command.message);
            emit_json(&response_success(command.id.as_deref(), "steer", None));
        }
        "follow_up" => {
            let command: RpcFollowUpCommand = match serde_json::from_value(payload) {
                Ok(command) => command,
                Err(err) => {
                    emit_json(&response_error(
                        id.as_deref(),
                        "follow_up",
                        &err.to_string(),
                    ));
                    return;
                }
            };
            session.follow_up(&command.message);
            emit_json(&response_success(command.id.as_deref(), "follow_up", None));
        }
        "abort" => {
            let command: RpcAbortCommand = match serde_json::from_value(payload) {
                Ok(command) => command,
                Err(err) => {
                    emit_json(&response_error(id.as_deref(), "abort", &err.to_string()));
                    return;
                }
            };
            session.abort();
            emit_json(&response_success(command.id.as_deref(), "abort", None));
        }
        "new_session" => {
            let command: RpcNewSessionCommand = match serde_json::from_value(payload) {
                Ok(command) => command,
                Err(err) => {
                    emit_json(&response_error(
                        id.as_deref(),
                        "new_session",
                        &err.to_string(),
                    ));
                    return;
                }
            };
            let _ = command.parent_session;
            session.new_session();
            emit_json(&response_success(
                command.id.as_deref(),
                "new_session",
                Some(json!({ "cancelled": false })),
            ));
        }
        "get_state" => {
            let command: RpcGetStateCommand = match serde_json::from_value(payload) {
                Ok(command) => command,
                Err(err) => {
                    emit_json(&response_error(
                        id.as_deref(),
                        "get_state",
                        &err.to_string(),
                    ));
                    return;
                }
            };
            let state = session.get_state();
            let value = json!({
                "model": agent_model_value(&state.model),
                "thinkingLevel": thinking_level_to_str(state.thinking_level),
                "isStreaming": state.is_streaming,
                "isCompacting": false,
                "steeringMode": queue_mode_to_str(session.steering_mode()),
                "followUpMode": queue_mode_to_str(session.follow_up_mode()),
                "sessionFile": session.session_file().map(|path| path.to_string_lossy().to_string()),
                "sessionId": session.session_id(),
                "autoCompactionEnabled": session.auto_compaction_enabled(),
                "messageCount": state.message_count,
                "pendingMessageCount": session.pending_message_count(),
            });
            emit_json(&response_success(
                command.id.as_deref(),
                "get_state",
                Some(value),
            ));
        }
        "set_thinking_level" => {
            let command: RpcSetThinkingLevelCommand = match serde_json::from_value(payload) {
                Ok(command) => command,
                Err(err) => {
                    emit_json(&response_error(
                        id.as_deref(),
                        "set_thinking_level",
                        &err.to_string(),
                    ));
                    return;
                }
            };
            let level = match thinking_level_from_str(&command.level) {
                Some(level) => level,
                None => {
                    emit_json(&response_error(
                        command.id.as_deref(),
                        "set_thinking_level",
                        "Invalid thinking level",
                    ));
                    return;
                }
            };
            session.set_thinking_level(level);
            emit_json(&response_success(
                command.id.as_deref(),
                "set_thinking_level",
                None,
            ));
        }
        "cycle_thinking_level" => {
            let command: RpcCycleThinkingLevelCommand = match serde_json::from_value(payload) {
                Ok(command) => command,
                Err(err) => {
                    emit_json(&response_error(
                        id.as_deref(),
                        "cycle_thinking_level",
                        &err.to_string(),
                    ));
                    return;
                }
            };
            let result = session.cycle_thinking_level();
            emit_json(&response_success(
                command.id.as_deref(),
                "cycle_thinking_level",
                Some(json!({ "level": thinking_level_to_str(result.level) })),
            ));
        }
        "set_steering_mode" => {
            let command: RpcSetSteeringModeCommand = match serde_json::from_value(payload) {
                Ok(command) => command,
                Err(err) => {
                    emit_json(&response_error(
                        id.as_deref(),
                        "set_steering_mode",
                        &err.to_string(),
                    ));
                    return;
                }
            };
            let mode = match queue_mode_from_str(&command.mode) {
                Some(mode) => mode,
                None => {
                    emit_json(&response_error(
                        command.id.as_deref(),
                        "set_steering_mode",
                        "Invalid steering mode",
                    ));
                    return;
                }
            };
            session.set_steering_mode(mode);
            emit_json(&response_success(
                command.id.as_deref(),
                "set_steering_mode",
                None,
            ));
        }
        "set_follow_up_mode" => {
            let command: RpcSetFollowUpModeCommand = match serde_json::from_value(payload) {
                Ok(command) => command,
                Err(err) => {
                    emit_json(&response_error(
                        id.as_deref(),
                        "set_follow_up_mode",
                        &err.to_string(),
                    ));
                    return;
                }
            };
            let mode = match queue_mode_from_str(&command.mode) {
                Some(mode) => mode,
                None => {
                    emit_json(&response_error(
                        command.id.as_deref(),
                        "set_follow_up_mode",
                        "Invalid follow-up mode",
                    ));
                    return;
                }
            };
            session.set_follow_up_mode(mode);
            emit_json(&response_success(
                command.id.as_deref(),
                "set_follow_up_mode",
                None,
            ));
        }
        "get_available_models" => {
            let command: RpcGetAvailableModelsCommand = match serde_json::from_value(payload) {
                Ok(command) => command,
                Err(err) => {
                    emit_json(&response_error(
                        id.as_deref(),
                        "get_available_models",
                        &err.to_string(),
                    ));
                    return;
                }
            };
            let models = session.get_available_models();
            emit_json(&response_success(
                command.id.as_deref(),
                "get_available_models",
                Some(json!({ "models": models })),
            ));
        }
        "set_model" => {
            let command: RpcSetModelCommand = match serde_json::from_value(payload) {
                Ok(command) => command,
                Err(err) => {
                    emit_json(&response_error(
                        id.as_deref(),
                        "set_model",
                        &err.to_string(),
                    ));
                    return;
                }
            };
            let model = match session
                .model_registry
                .find(&command.provider, &command.model_id)
            {
                Some(model) => model,
                None => {
                    emit_json(&response_error(
                        command.id.as_deref(),
                        "set_model",
                        "Model not found",
                    ));
                    return;
                }
            };
            let agent_model = to_agent_model(&model);
            session.set_model(agent_model);
            emit_json(&response_success(
                command.id.as_deref(),
                "set_model",
                Some(json!(model)),
            ));
        }
        "cycle_model" => {
            let command: RpcCycleModelCommand = match serde_json::from_value(payload) {
                Ok(command) => command,
                Err(err) => {
                    emit_json(&response_error(
                        id.as_deref(),
                        "cycle_model",
                        &err.to_string(),
                    ));
                    return;
                }
            };
            let result = session.cycle_model();
            let data = result.map(|cycle| {
                json!({
                    "model": cycle.model,
                    "thinkingLevel": thinking_level_to_str(cycle.thinking_level),
                    "isScoped": cycle.is_scoped,
                })
            });
            emit_json(&response_success(
                command.id.as_deref(),
                "cycle_model",
                Some(data.unwrap_or(Value::Null)),
            ));
        }
        "compact" => {
            let command: RpcCompactCommand = match serde_json::from_value(payload) {
                Ok(command) => command,
                Err(err) => {
                    emit_json(&response_error(id.as_deref(), "compact", &err.to_string()));
                    return;
                }
            };
            let _ = command.custom_instructions;
            match session.compact() {
                Ok(result) => emit_json(&response_success(
                    command.id.as_deref(),
                    "compact",
                    Some(serde_json::to_value(result).unwrap_or(Value::Null)),
                )),
                Err(err) => emit_json(&response_error(
                    command.id.as_deref(),
                    "compact",
                    &err.to_string(),
                )),
            }
        }
        "set_auto_compaction" => {
            let command: RpcSetAutoCompactionCommand = match serde_json::from_value(payload) {
                Ok(command) => command,
                Err(err) => {
                    emit_json(&response_error(
                        id.as_deref(),
                        "set_auto_compaction",
                        &err.to_string(),
                    ));
                    return;
                }
            };
            session.set_auto_compaction_enabled(command.enabled);
            emit_json(&response_success(
                command.id.as_deref(),
                "set_auto_compaction",
                None,
            ));
        }
        "set_auto_retry" => {
            let command: RpcSetAutoRetryCommand = match serde_json::from_value(payload) {
                Ok(command) => command,
                Err(err) => {
                    emit_json(&response_error(
                        id.as_deref(),
                        "set_auto_retry",
                        &err.to_string(),
                    ));
                    return;
                }
            };
            session.set_auto_retry_enabled(command.enabled);
            emit_json(&response_success(
                command.id.as_deref(),
                "set_auto_retry",
                None,
            ));
        }
        "abort_retry" => {
            let command: RpcAbortRetryCommand = match serde_json::from_value(payload) {
                Ok(command) => command,
                Err(err) => {
                    emit_json(&response_error(
                        id.as_deref(),
                        "abort_retry",
                        &err.to_string(),
                    ));
                    return;
                }
            };
            session.abort_retry();
            emit_json(&response_success(
                command.id.as_deref(),
                "abort_retry",
                None,
            ));
        }
        "get_session_stats" => {
            let command: RpcGetSessionStatsCommand = match serde_json::from_value(payload) {
                Ok(command) => command,
                Err(err) => {
                    emit_json(&response_error(
                        id.as_deref(),
                        "get_session_stats",
                        &err.to_string(),
                    ));
                    return;
                }
            };
            let stats = session.get_session_stats();
            emit_json(&response_success(
                command.id.as_deref(),
                "get_session_stats",
                Some(serde_json::to_value(stats).unwrap_or(Value::Null)),
            ));
        }
        "export_html" => {
            let command: RpcExportHtmlCommand = match serde_json::from_value(payload) {
                Ok(command) => command,
                Err(err) => {
                    emit_json(&response_error(
                        id.as_deref(),
                        "export_html",
                        &err.to_string(),
                    ));
                    return;
                }
            };
            let output_path = command.output_path.map(PathBuf::from);
            match session.export_to_html_with_path(output_path.as_ref()) {
                Ok(result) => emit_json(&response_success(
                    command.id.as_deref(),
                    "export_html",
                    Some(json!({ "path": result.path })),
                )),
                Err(err) => emit_json(&response_error(
                    command.id.as_deref(),
                    "export_html",
                    &err.to_string(),
                )),
            }
        }
        "bash" => {
            let command: RpcBashCommand = match serde_json::from_value(payload) {
                Ok(command) => command,
                Err(err) => {
                    emit_json(&response_error(id.as_deref(), "bash", &err.to_string()));
                    return;
                }
            };
            match session.execute_bash(&command.command) {
                Ok(result) => emit_json(&response_success(
                    command.id.as_deref(),
                    "bash",
                    Some(serde_json::to_value(result).unwrap_or(Value::Null)),
                )),
                Err(err) => emit_json(&response_error(
                    command.id.as_deref(),
                    "bash",
                    &err.to_string(),
                )),
            }
        }
        "abort_bash" => {
            let command: RpcAbortBashCommand = match serde_json::from_value(payload) {
                Ok(command) => command,
                Err(err) => {
                    emit_json(&response_error(
                        id.as_deref(),
                        "abort_bash",
                        &err.to_string(),
                    ));
                    return;
                }
            };
            session.abort_bash();
            emit_json(&response_success(command.id.as_deref(), "abort_bash", None));
        }
        "switch_session" => {
            let command: RpcSwitchSessionCommand = match serde_json::from_value(payload) {
                Ok(command) => command,
                Err(err) => {
                    emit_json(&response_error(
                        id.as_deref(),
                        "switch_session",
                        &err.to_string(),
                    ));
                    return;
                }
            };
            let path = PathBuf::from(command.session_path);
            match session.switch_session(path) {
                Ok(cancelled) => emit_json(&response_success(
                    command.id.as_deref(),
                    "switch_session",
                    Some(json!({ "cancelled": !cancelled })),
                )),
                Err(err) => emit_json(&response_error(
                    command.id.as_deref(),
                    "switch_session",
                    &err.to_string(),
                )),
            }
        }
        "branch" => {
            let command: RpcBranchCommand = match serde_json::from_value(payload) {
                Ok(command) => command,
                Err(err) => {
                    emit_json(&response_error(id.as_deref(), "branch", &err.to_string()));
                    return;
                }
            };
            match session.branch(&command.entry_id) {
                Ok(result) => emit_json(&response_success(
                    command.id.as_deref(),
                    "branch",
                    Some(json!({ "text": result.selected_text, "cancelled": result.cancelled })),
                )),
                Err(err) => emit_json(&response_error(
                    command.id.as_deref(),
                    "branch",
                    &err.to_string(),
                )),
            }
        }
        "get_branch_messages" => {
            let command: RpcGetBranchMessagesCommand = match serde_json::from_value(payload) {
                Ok(command) => command,
                Err(err) => {
                    emit_json(&response_error(
                        id.as_deref(),
                        "get_branch_messages",
                        &err.to_string(),
                    ));
                    return;
                }
            };
            let messages = session
                .get_user_messages_for_branching()
                .into_iter()
                .map(|message| json!({ "entryId": message.entry_id, "text": message.text }))
                .collect::<Vec<_>>();
            emit_json(&response_success(
                command.id.as_deref(),
                "get_branch_messages",
                Some(json!({ "messages": messages })),
            ));
        }
        "get_last_assistant_text" => {
            let command: RpcGetLastAssistantTextCommand = match serde_json::from_value(payload) {
                Ok(command) => command,
                Err(err) => {
                    emit_json(&response_error(
                        id.as_deref(),
                        "get_last_assistant_text",
                        &err.to_string(),
                    ));
                    return;
                }
            };
            let text = session.get_last_assistant_text();
            emit_json(&response_success(
                command.id.as_deref(),
                "get_last_assistant_text",
                Some(json!({ "text": text })),
            ));
        }
        "get_messages" => {
            let command: RpcGetMessagesCommand = match serde_json::from_value(payload) {
                Ok(command) => command,
                Err(err) => {
                    emit_json(&response_error(
                        id.as_deref(),
                        "get_messages",
                        &err.to_string(),
                    ));
                    return;
                }
            };
            let messages = session
                .messages()
                .into_iter()
                .filter_map(|message| agent_message_to_core(&message))
                .collect::<Vec<_>>();
            emit_json(&response_success(
                command.id.as_deref(),
                "get_messages",
                Some(json!({ "messages": messages })),
            ));
        }
        _ => {
            emit_json(&response_error(
                id.as_deref(),
                command_type,
                "Unknown command",
            ));
        }
    }
}

fn response_success(id: Option<&str>, command: &str, data: Option<Value>) -> Value {
    match data {
        Some(data) => json!({
            "id": id,
            "type": "response",
            "command": command,
            "success": true,
            "data": data
        }),
        None => json!({
            "id": id,
            "type": "response",
            "command": command,
            "success": true
        }),
    }
}

fn response_error(id: Option<&str>, command: &str, error: &str) -> Value {
    json!({
        "id": id,
        "type": "response",
        "command": command,
        "success": false,
        "error": error
    })
}

fn emit_json(value: &Value) {
    let mut stdout = io::stdout().lock();
    let _ = writeln!(stdout, "{value}");
    let _ = stdout.flush();
}

fn serialize_agent_event(event: &pi::coding_agent::AgentSessionEvent) -> Option<Value> {
    match event {
        pi::coding_agent::AgentSessionEvent::Agent(agent_event) => match agent_event.as_ref() {
            pi::agent::AgentEvent::AgentStart => Some(json!({ "type": "agent_start" })),
            pi::agent::AgentEvent::AgentEnd { messages } => Some(json!({
                "type": "agent_end",
                "messages": messages.iter().filter_map(agent_message_to_core).collect::<Vec<_>>()
            })),
            pi::agent::AgentEvent::TurnStart => Some(json!({ "type": "turn_start" })),
            pi::agent::AgentEvent::TurnEnd {
                message,
                tool_results,
            } => Some(json!({
                "type": "turn_end",
                "message": agent_message_to_core(message),
                "toolResults": tool_results
            })),
            pi::agent::AgentEvent::MessageStart { message } => Some(json!({
                "type": "message_start",
                "message": agent_message_to_core(message),
            })),
            pi::agent::AgentEvent::MessageUpdate { message } => Some(json!({
                "type": "message_update",
                "message": agent_message_to_core(message),
            })),
            pi::agent::AgentEvent::MessageEnd { message } => Some(json!({
                "type": "message_end",
                "message": agent_message_to_core(message),
            })),
            pi::agent::AgentEvent::ToolExecutionStart {
                tool_call_id,
                tool_name,
                args,
            } => Some(json!({
                "type": "tool_execution_start",
                "toolCallId": tool_call_id,
                "toolName": tool_name,
                "args": args
            })),
            pi::agent::AgentEvent::ToolExecutionUpdate {
                tool_call_id,
                tool_name,
                args,
                partial_result,
            } => Some(json!({
                "type": "tool_execution_update",
                "toolCallId": tool_call_id,
                "toolName": tool_name,
                "args": args,
                "partialResult": agent_tool_result_value(partial_result),
            })),
            pi::agent::AgentEvent::ToolExecutionEnd {
                tool_call_id,
                tool_name,
                result,
                is_error,
            } => Some(json!({
                "type": "tool_execution_end",
                "toolCallId": tool_call_id,
                "toolName": tool_name,
                "result": agent_tool_result_value(result),
                "isError": is_error,
            })),
        },
        pi::coding_agent::AgentSessionEvent::AutoCompactionStart { reason } => {
            Some(json!({ "type": "auto_compaction_start", "reason": reason }))
        }
        pi::coding_agent::AgentSessionEvent::AutoCompactionEnd { aborted } => {
            Some(json!({ "type": "auto_compaction_end", "aborted": aborted }))
        }
    }
}

fn agent_tool_result_value(result: &AgentToolResult) -> Value {
    json!({
        "content": result.content,
        "details": result.details,
    })
}

fn agent_message_to_core(message: &AgentMessage) -> Option<CoreAgentMessage> {
    match message {
        AgentMessage::User(user) => Some(CoreAgentMessage::User(user.clone())),
        AgentMessage::Assistant(assistant) => Some(CoreAgentMessage::Assistant(assistant.clone())),
        AgentMessage::ToolResult(result) => Some(CoreAgentMessage::ToolResult(result.clone())),
        AgentMessage::Custom(custom) => Some(CoreAgentMessage::HookMessage(
            pi::core::messages::HookMessage {
                custom_type: custom.role.clone(),
                content: UserContent::Text(custom.text.clone()),
                display: true,
                details: None,
                timestamp: custom.timestamp,
            },
        )),
    }
}

fn queue_mode_from_str(mode: &str) -> Option<QueueMode> {
    match mode {
        "all" => Some(QueueMode::All),
        "one-at-a-time" => Some(QueueMode::OneAtATime),
        _ => None,
    }
}

fn queue_mode_to_str(mode: QueueMode) -> &'static str {
    match mode {
        QueueMode::All => "all",
        QueueMode::OneAtATime => "one-at-a-time",
    }
}

fn thinking_level_from_str(level: &str) -> Option<ThinkingLevel> {
    match level {
        "off" => Some(ThinkingLevel::Off),
        "minimal" => Some(ThinkingLevel::Minimal),
        "low" => Some(ThinkingLevel::Low),
        "medium" => Some(ThinkingLevel::Medium),
        "high" => Some(ThinkingLevel::High),
        "xhigh" => Some(ThinkingLevel::XHigh),
        _ => None,
    }
}

fn thinking_level_to_str(level: ThinkingLevel) -> &'static str {
    level.as_str()
}

fn agent_model_value(model: &AgentModel) -> Value {
    json!({
        "id": model.id,
        "name": model.name,
        "api": model.api,
        "provider": model.provider,
    })
}

fn build_user_content(message: &str, images: &[RpcImage]) -> Result<UserContent, String> {
    let mut blocks = Vec::new();
    if !message.trim().is_empty() {
        blocks.push(ContentBlock::Text {
            text: message.to_string(),
            text_signature: None,
        });
    }
    for image in images {
        blocks.push(ContentBlock::Image {
            data: image.data.clone(),
            mime_type: image.mime_type.clone(),
        });
    }
    if blocks.is_empty() {
        return Err("No prompt content provided.".to_string());
    }
    Ok(UserContent::Blocks(blocks))
}

fn to_agent_model(model: &RegistryModel) -> AgentModel {
    AgentModel {
        id: model.id.clone(),
        name: model.name.clone(),
        api: model.api.clone(),
        provider: model.provider.clone(),
    }
}

fn build_agent_tools(
    cwd: &PathBuf,
    tool_names: Option<&[String]>,
) -> Result<Vec<AgentTool>, String> {
    let available = ["read", "write", "edit", "bash", "grep", "find", "ls"];
    let selected = match tool_names {
        Some(names) => {
            for name in names {
                if !available.iter().any(|item| item == name) {
                    return Err(format!("Tool \"{name}\" is not supported"));
                }
            }
            names.to_vec()
        }
        None => available.iter().map(|name| name.to_string()).collect(),
    };

    let mut tools = Vec::new();
    for name in selected {
        match name.as_str() {
            "read" => {
                let tool = agent_tools::ReadTool::new(cwd);
                tools.push(AgentTool {
                    name: "read".to_string(),
                    label: "read".to_string(),
                    description: "Read file contents".to_string(),
                    execute: Rc::new(move |call_id, params| {
                        let args = parse_read_args(params)?;
                        let result = tool.execute(call_id, args)?;
                        Ok(tool_result_to_agent_result(result))
                    }),
                });
            }
            "write" => {
                let tool = agent_tools::WriteTool::new(cwd);
                tools.push(AgentTool {
                    name: "write".to_string(),
                    label: "write".to_string(),
                    description: "Write file contents".to_string(),
                    execute: Rc::new(move |call_id, params| {
                        let args = parse_write_args(params)?;
                        let result = tool.execute(call_id, args)?;
                        Ok(tool_result_to_agent_result(result))
                    }),
                });
            }
            "edit" => {
                let tool = agent_tools::EditTool::new(cwd);
                tools.push(AgentTool {
                    name: "edit".to_string(),
                    label: "edit".to_string(),
                    description: "Edit file contents".to_string(),
                    execute: Rc::new(move |call_id, params| {
                        let args = parse_edit_args(params)?;
                        let result = tool.execute(call_id, args)?;
                        Ok(tool_result_to_agent_result(result))
                    }),
                });
            }
            "bash" => {
                let tool = agent_tools::BashTool::new(cwd);
                tools.push(AgentTool {
                    name: "bash".to_string(),
                    label: "bash".to_string(),
                    description: "Execute bash commands".to_string(),
                    execute: Rc::new(move |call_id, params| {
                        let args = parse_bash_args(params)?;
                        let result = tool.execute(call_id, args)?;
                        Ok(tool_result_to_agent_result(result))
                    }),
                });
            }
            "grep" => {
                let tool = agent_tools::GrepTool::new(cwd);
                tools.push(AgentTool {
                    name: "grep".to_string(),
                    label: "grep".to_string(),
                    description: "Search file contents".to_string(),
                    execute: Rc::new(move |call_id, params| {
                        let args = parse_grep_args(params)?;
                        let result = tool.execute(call_id, args)?;
                        Ok(tool_result_to_agent_result(result))
                    }),
                });
            }
            "find" => {
                let tool = agent_tools::FindTool::new(cwd);
                tools.push(AgentTool {
                    name: "find".to_string(),
                    label: "find".to_string(),
                    description: "Find files by pattern".to_string(),
                    execute: Rc::new(move |call_id, params| {
                        let args = parse_find_args(params)?;
                        let result = tool.execute(call_id, args)?;
                        Ok(tool_result_to_agent_result(result))
                    }),
                });
            }
            "ls" => {
                let tool = agent_tools::LsTool::new(cwd);
                tools.push(AgentTool {
                    name: "ls".to_string(),
                    label: "ls".to_string(),
                    description: "List directory contents".to_string(),
                    execute: Rc::new(move |call_id, params| {
                        let args = parse_ls_args(params)?;
                        let result = tool.execute(call_id, args)?;
                        Ok(tool_result_to_agent_result(result))
                    }),
                });
            }
            _ => {}
        }
    }

    Ok(tools)
}

fn parse_read_args(params: &Value) -> Result<agent_tools::ReadToolArgs, String> {
    Ok(agent_tools::ReadToolArgs {
        path: get_required_string(params, "path")?,
        offset: get_optional_usize(params, "offset"),
        limit: get_optional_usize(params, "limit"),
    })
}

fn parse_write_args(params: &Value) -> Result<agent_tools::WriteToolArgs, String> {
    Ok(agent_tools::WriteToolArgs {
        path: get_required_string(params, "path")?,
        content: get_required_string(params, "content")?,
    })
}

fn parse_edit_args(params: &Value) -> Result<agent_tools::EditToolArgs, String> {
    Ok(agent_tools::EditToolArgs {
        path: get_required_string(params, "path")?,
        old_text: get_required_string(params, "oldText")?,
        new_text: get_required_string(params, "newText")?,
    })
}

fn parse_bash_args(params: &Value) -> Result<agent_tools::BashToolArgs, String> {
    Ok(agent_tools::BashToolArgs {
        command: get_required_string(params, "command")?,
        timeout: get_optional_u64(params, "timeout"),
    })
}

fn parse_grep_args(params: &Value) -> Result<agent_tools::GrepToolArgs, String> {
    Ok(agent_tools::GrepToolArgs {
        pattern: get_required_string(params, "pattern")?,
        path: get_optional_string(params, "path"),
        glob: get_optional_string(params, "glob"),
        ignore_case: get_optional_bool(params, "ignoreCase"),
        literal: get_optional_bool(params, "literal"),
        context: get_optional_usize(params, "context"),
        limit: get_optional_usize(params, "limit"),
    })
}

fn parse_find_args(params: &Value) -> Result<agent_tools::FindToolArgs, String> {
    Ok(agent_tools::FindToolArgs {
        pattern: get_required_string(params, "pattern")?,
        path: get_optional_string(params, "path"),
        limit: get_optional_usize(params, "limit"),
    })
}

fn parse_ls_args(params: &Value) -> Result<agent_tools::LsToolArgs, String> {
    Ok(agent_tools::LsToolArgs {
        path: get_optional_string(params, "path"),
        limit: get_optional_usize(params, "limit"),
    })
}

fn get_required_string(params: &Value, key: &str) -> Result<String, String> {
    params
        .get(key)
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
        .ok_or_else(|| format!("Missing or invalid \"{}\" argument", key))
}

fn get_optional_string(params: &Value, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
}

fn get_optional_bool(params: &Value, key: &str) -> Option<bool> {
    params.get(key).and_then(|value| value.as_bool())
}

fn get_optional_usize(params: &Value, key: &str) -> Option<usize> {
    params
        .get(key)
        .and_then(|value| value.as_i64())
        .and_then(|value| {
            if value < 0 {
                None
            } else {
                Some(value as usize)
            }
        })
}

fn get_optional_u64(params: &Value, key: &str) -> Option<u64> {
    params
        .get(key)
        .and_then(|value| value.as_i64())
        .and_then(|value| if value < 0 { None } else { Some(value as u64) })
}

fn tool_result_to_agent_result(result: agent_tools::ToolResult) -> AgentToolResult {
    AgentToolResult {
        content: result.content,
        details: result.details.unwrap_or(Value::Null),
    }
}

fn build_stream_fn(
    model: RegistryModel,
    api_key: String,
    use_oauth: bool,
    tool_specs: Vec<AnthropicTool>,
) -> AgentStreamFn {
    Box::new(move |_agent_model, context| {
        let system = if !context.system_prompt.trim().is_empty() {
            Some(context.system_prompt.as_str())
        } else if use_oauth {
            Some(DEFAULT_OAUTH_SYSTEM_PROMPT)
        } else {
            None
        };

        let messages = build_anthropic_messages(context);
        let response = call_anthropic(
            messages,
            AnthropicCallOptions {
                model: &model.id,
                api_key: &api_key,
                use_oauth,
                tools: &tool_specs,
                base_url: if model.base_url.is_empty() {
                    "https://api.anthropic.com/v1"
                } else {
                    model.base_url.as_str()
                },
                extra_headers: model.headers.as_ref(),
                system,
            },
        );

        match response {
            Ok(response) => assistant_message_from_anthropic(&model, response),
            Err(err) => assistant_error_message(&model, &err),
        }
    })
}

fn build_openai_stream_fn(
    model: RegistryModel,
    api_key: String,
    tool_specs: Vec<OpenAITool>,
) -> AgentStreamFn {
    Box::new(move |_agent_model, context| {
        let input = openai_context_to_input_items(&model, context);
        let response = call_openai(
            input,
            OpenAICallOptions {
                model: &model.id,
                api_key: &api_key,
                tools: &tool_specs,
                base_url: if model.base_url.is_empty() {
                    "https://api.openai.com/v1"
                } else {
                    model.base_url.as_str()
                },
                extra_headers: model.headers.as_ref(),
            },
        );

        match response {
            Ok(response) => match openai_assistant_message_from_response(&model, response) {
                Ok(message) => message,
                Err(err) => assistant_error_message(&model, &err),
            },
            Err(err) => assistant_error_message(&model, &err),
        }
    })
}

fn merge_system_prompt(
    system_prompt: Option<String>,
    append_system_prompt: Option<String>,
) -> Option<String> {
    let mut system = system_prompt;
    if let Some(append) = append_system_prompt {
        system = Some(match system {
            Some(base) => format!("{base}\n\n{append}"),
            None => append,
        });
    }
    system
}

fn create_cli_session(
    model: RegistryModel,
    registry: ModelRegistry,
    system_prompt: Option<String>,
    append_system_prompt: Option<String>,
    tool_names: Option<&[String]>,
    api_key_override: Option<&str>,
    session_manager: SessionManager,
) -> Result<AgentSession, String> {
    let cwd = env::current_dir().map_err(|err| err.to_string())?;
    let agent_tools = build_agent_tools(&cwd, tool_names)?;
    let tool_defs = build_tool_defs(tool_names)?;

    let stream_fn = match model.api.as_str() {
        "anthropic-messages" => {
            let (api_key, use_oauth) = resolve_anthropic_credentials(api_key_override)?;
            let tool_specs = tool_defs
                .iter()
                .map(|tool| AnthropicTool {
                    name: tool.name.to_string(),
                    description: tool.description.to_string(),
                    input_schema: tool.input_schema.clone(),
                })
                .collect::<Vec<_>>();
            build_stream_fn(model.clone(), api_key, use_oauth, tool_specs)
        }
        "openai-responses" => {
            let api_key = resolve_openai_credentials(api_key_override)?;
            let tool_specs = tool_defs
                .iter()
                .map(|tool| OpenAITool {
                    tool_type: "function".to_string(),
                    name: tool.name.to_string(),
                    description: tool.description.to_string(),
                    parameters: tool.input_schema.clone(),
                })
                .collect::<Vec<_>>();
            build_openai_stream_fn(model.clone(), api_key, tool_specs)
        }
        _ => {
            return Err(format!(
                "Model API \"{}\" is not supported in print mode.",
                model.api
            ))
        }
    };

    let system_value = merge_system_prompt(system_prompt, append_system_prompt).unwrap_or_default();
    let agent_model = to_agent_model(&model);
    let agent = Agent::new(AgentOptions {
        initial_state: Some(AgentStateOverride {
            system_prompt: Some(system_value),
            model: Some(agent_model),
            tools: Some(agent_tools),
            ..Default::default()
        }),
        stream_fn: Some(stream_fn),
        ..Default::default()
    });

    let settings_manager = SettingsManager::create("", "");

    let mut session = AgentSession::new(AgentSessionConfig {
        agent,
        session_manager,
        settings_manager,
        model_registry: registry,
    });
    let templates = load_prompt_templates(LoadPromptTemplatesOptions {
        cwd: Some(cwd),
        agent_dir: Some(config::get_agent_dir()),
    });
    session.set_prompt_templates(templates);
    Ok(session)
}

fn create_rpc_session(
    model: RegistryModel,
    registry: ModelRegistry,
    system_prompt: Option<String>,
    append_system_prompt: Option<String>,
    tool_names: Option<&[String]>,
    api_key_override: Option<&str>,
    session_manager: SessionManager,
) -> Result<AgentSession, String> {
    let cwd = env::current_dir().map_err(|err| err.to_string())?;
    let agent_tools = build_agent_tools(&cwd, tool_names)?;
    let tool_defs = build_tool_defs(tool_names)?;
    let stream_fn = match model.api.as_str() {
        "anthropic-messages" => {
            let (api_key, use_oauth) = resolve_anthropic_credentials(api_key_override)?;
            let tool_specs = tool_defs
                .iter()
                .map(|tool| AnthropicTool {
                    name: tool.name.to_string(),
                    description: tool.description.to_string(),
                    input_schema: tool.input_schema.clone(),
                })
                .collect::<Vec<_>>();
            build_stream_fn(model.clone(), api_key, use_oauth, tool_specs)
        }
        "openai-responses" => {
            let api_key = resolve_openai_credentials(api_key_override)?;
            let tool_specs = tool_defs
                .iter()
                .map(|tool| OpenAITool {
                    tool_type: "function".to_string(),
                    name: tool.name.to_string(),
                    description: tool.description.to_string(),
                    parameters: tool.input_schema.clone(),
                })
                .collect::<Vec<_>>();
            build_openai_stream_fn(model.clone(), api_key, tool_specs)
        }
        _ => {
            return Err(format!(
                "Model API \"{}\" is not supported in RPC mode.",
                model.api
            ))
        }
    };

    let system_value = merge_system_prompt(system_prompt, append_system_prompt).unwrap_or_default();
    let agent_model = to_agent_model(&model);
    let agent = Agent::new(AgentOptions {
        initial_state: Some(AgentStateOverride {
            system_prompt: Some(system_value),
            model: Some(agent_model),
            tools: Some(agent_tools),
            ..Default::default()
        }),
        stream_fn: Some(stream_fn),
        ..Default::default()
    });

    let settings_manager = SettingsManager::create("", "");

    let mut session = AgentSession::new(AgentSessionConfig {
        agent,
        session_manager,
        settings_manager,
        model_registry: registry,
    });
    let templates = load_prompt_templates(LoadPromptTemplatesOptions {
        cwd: Some(cwd),
        agent_dir: Some(config::get_agent_dir()),
    });
    session.set_prompt_templates(templates);
    Ok(session)
}

fn build_anthropic_messages(context: &LlmContext) -> Vec<AnthropicMessage> {
    let mut messages = Vec::new();
    for message in &context.messages {
        match message {
            AgentMessage::User(user) => {
                let content = user_content_to_anthropic_blocks(&user.content);
                messages.push(AnthropicMessage {
                    role: "user".to_string(),
                    content,
                });
            }
            AgentMessage::Assistant(assistant) => {
                let content = assistant_blocks_to_anthropic_blocks(&assistant.content);
                messages.push(AnthropicMessage {
                    role: "assistant".to_string(),
                    content,
                });
            }
            AgentMessage::ToolResult(result) => {
                let content = tool_result_to_anthropic_blocks(result);
                messages.push(AnthropicMessage {
                    role: "user".to_string(),
                    content,
                });
            }
            AgentMessage::Custom(_) => {}
        }
    }
    messages
}

fn user_content_to_anthropic_blocks(content: &UserContent) -> Vec<AnthropicContentBlock> {
    match content {
        UserContent::Text(text) => vec![AnthropicContentBlock::Text { text: text.clone() }],
        UserContent::Blocks(blocks) => blocks
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text, .. } => {
                    Some(AnthropicContentBlock::Text { text: text.clone() })
                }
                ContentBlock::Image { data, mime_type } => Some(AnthropicContentBlock::Image {
                    source: AnthropicImageSource {
                        source_type: "base64".to_string(),
                        media_type: mime_type.clone(),
                        data: data.clone(),
                    },
                }),
                _ => None,
            })
            .collect(),
    }
}

fn assistant_blocks_to_anthropic_blocks(blocks: &[ContentBlock]) -> Vec<AnthropicContentBlock> {
    blocks
        .iter()
        .map(|block| match block {
            ContentBlock::Text { text, .. } => AnthropicContentBlock::Text { text: text.clone() },
            ContentBlock::Thinking { thinking, .. } => AnthropicContentBlock::Text {
                text: thinking.clone(),
            },
            ContentBlock::ToolCall {
                id,
                name,
                arguments,
                ..
            } => AnthropicContentBlock::ToolUse {
                id: id.clone(),
                name: name.clone(),
                input: arguments.clone(),
            },
            ContentBlock::Image { data, mime_type } => AnthropicContentBlock::Image {
                source: AnthropicImageSource {
                    source_type: "base64".to_string(),
                    media_type: mime_type.clone(),
                    data: data.clone(),
                },
            },
        })
        .collect()
}

fn tool_result_to_anthropic_blocks(result: &ToolResultMessage) -> Vec<AnthropicContentBlock> {
    let content = result
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text, .. } => {
                Some(AnthropicToolResultContent::Text { text: text.clone() })
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    vec![AnthropicContentBlock::ToolResult {
        tool_use_id: result.tool_call_id.clone(),
        content,
        is_error: result.is_error,
    }]
}

fn assistant_message_from_anthropic(
    model: &RegistryModel,
    response: AnthropicResponse,
) -> AssistantMessage {
    let content = response
        .content
        .into_iter()
        .filter_map(|block| match block {
            AnthropicContentBlock::Text { text } => Some(ContentBlock::Text {
                text,
                text_signature: None,
            }),
            AnthropicContentBlock::ToolUse { id, name, input } => Some(ContentBlock::ToolCall {
                id,
                name,
                arguments: input,
                thought_signature: None,
            }),
            AnthropicContentBlock::Image { source } => Some(ContentBlock::Image {
                data: source.data,
                mime_type: source.media_type,
            }),
            AnthropicContentBlock::ToolResult { .. } => None,
        })
        .collect::<Vec<_>>();

    AssistantMessage {
        content,
        api: model.api.clone(),
        provider: model.provider.clone(),
        model: model.id.clone(),
        usage: empty_usage(),
        stop_reason: response.stop_reason.unwrap_or_else(|| "stop".to_string()),
        error_message: None,
        timestamp: now_millis(),
    }
}

fn assistant_error_message(model: &RegistryModel, message: &str) -> AssistantMessage {
    AssistantMessage {
        content: vec![ContentBlock::Text {
            text: message.to_string(),
            text_signature: None,
        }],
        api: model.api.clone(),
        provider: model.provider.clone(),
        model: model.id.clone(),
        usage: empty_usage(),
        stop_reason: "error".to_string(),
        error_message: Some(message.to_string()),
        timestamp: now_millis(),
    }
}

fn empty_usage() -> Usage {
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
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

fn detect_image_mime_type(data: &[u8]) -> Option<&'static str> {
    let png_magic: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if data.len() >= png_magic.len() && data[..png_magic.len()] == png_magic {
        return Some("image/png");
    }

    if data.len() >= 3 && data[0] == 0xFF && data[1] == 0xD8 && data[2] == 0xFF {
        return Some("image/jpeg");
    }

    if data.len() >= 6 {
        let header = &data[..6];
        if header == b"GIF87a" || header == b"GIF89a" {
            return Some("image/gif");
        }
    }

    if data.len() >= 12 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        return Some("image/webp");
    }

    None
}

fn base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(data.len().div_ceil(3) * 4);
    let mut i = 0;
    while i < data.len() {
        let b0 = data[i];
        let b1 = if i + 1 < data.len() { data[i + 1] } else { 0 };
        let b2 = if i + 2 < data.len() { data[i + 2] } else { 0 };
        output.push(TABLE[(b0 >> 2) as usize] as char);
        output.push(TABLE[((b0 & 0x03) << 4 | (b1 >> 4)) as usize] as char);
        if i + 1 < data.len() {
            output.push(TABLE[((b1 & 0x0f) << 2 | (b2 >> 6)) as usize] as char);
        } else {
            output.push('=');
        }
        if i + 2 < data.len() {
            output.push(TABLE[(b2 & 0x3f) as usize] as char);
        } else {
            output.push('=');
        }
        i += 3;
    }
    output
}
