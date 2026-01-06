use crate::agent::{QueueMode, ThinkingLevel};
use crate::cli::event_json::{serialize_agent_message, serialize_session_event};
use crate::coding_agent::extension_host::{ExtensionUiRequest, ExtensionUiResponse};
use crate::coding_agent::AgentSession;
use crate::core::messages::{ContentBlock, UserContent};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcImage {
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub data: String,
    pub mime_type: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RpcPromptCommand {
    pub id: Option<String>,
    pub message: String,
    #[serde(default)]
    pub images: Vec<RpcImage>,
    #[serde(default)]
    pub streaming_behavior: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RpcSimpleCommand {
    pub id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RpcSetModelCommand {
    pub id: Option<String>,
    pub provider: String,
    pub model_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RpcSetThinkingLevelCommand {
    pub id: Option<String>,
    pub level: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RpcSetQueueModeCommand {
    pub id: Option<String>,
    pub mode: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RpcCompactCommand {
    pub id: Option<String>,
    #[serde(default)]
    pub custom_instructions: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RpcSetAutoCommand {
    pub id: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RpcBashCommand {
    pub id: Option<String>,
    pub command: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RpcExportHtmlCommand {
    pub id: Option<String>,
    #[serde(default)]
    pub output_path: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RpcSwitchSessionCommand {
    pub id: Option<String>,
    pub session_path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RpcBranchCommand {
    pub id: Option<String>,
    pub entry_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RpcExtensionUiResponse {
    pub id: String,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub confirmed: Option<bool>,
    #[serde(default)]
    pub cancelled: Option<bool>,
}

pub fn run_rpc_mode(mut session: AgentSession) -> Result<(), String> {
    let pending_ui: Arc<Mutex<HashMap<String, mpsc::Sender<ExtensionUiResponse>>>> =
        Arc::new(Mutex::new(HashMap::new()));

    let pending_ui_handler = pending_ui.clone();
    session.set_extension_ui_handler(move |request| {
        let value = extension_ui_request_to_value(request);
        let needs_response = matches!(
            request.method.as_str(),
            "select" | "confirm" | "input" | "editor"
        );
        if !needs_response {
            emit_json(&value);
            return ExtensionUiResponse::default();
        }

        let (tx, rx) = mpsc::channel();
        if let Ok(mut pending) = pending_ui_handler.lock() {
            pending.insert(request.id.clone(), tx);
        }
        emit_json(&value);
        let response = rx.recv().unwrap_or_default();
        if let Ok(mut pending) = pending_ui_handler.lock() {
            pending.remove(&request.id);
        }
        response
    });

    let _subscription = session.subscribe(|event| {
        if let Some(value) = serialize_session_event(event) {
            emit_json(&value);
        }
    });

    let mut stdin = io::stdin().lock();
    loop {
        let mut line = String::new();
        let bytes = stdin.read_line(&mut line).map_err(|err| err.to_string())?;
        if bytes == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let value: Value = match serde_json::from_str(trimmed) {
            Ok(value) => value,
            Err(err) => {
                let error = response_error(None, "parse", &format!("Invalid JSON: {err}"));
                emit_json(&error);
                continue;
            }
        };
        let kind = value
            .get("type")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string();

        if kind == "extension_ui_response" {
            if let Ok(response) = serde_json::from_value::<RpcExtensionUiResponse>(value.clone()) {
                if let Ok(mut pending) = pending_ui.lock() {
                    if let Some(sender) = pending.remove(&response.id) {
                        let extension_response = ExtensionUiResponse {
                            value: response.value,
                            confirmed: response.confirmed,
                            cancelled: response.cancelled,
                        };
                        let _ = sender.send(extension_response);
                    }
                }
                continue;
            }
        }

        match kind.as_str() {
            "prompt" => {
                let command: RpcPromptCommand = match serde_json::from_value(value) {
                    Ok(command) => command,
                    Err(err) => {
                        emit_json(&response_error(
                            None,
                            "prompt",
                            &format!("Invalid payload: {err}"),
                        ));
                        continue;
                    }
                };
                let id = command.id.as_deref();
                if session.is_streaming() {
                    match command.streaming_behavior.as_deref() {
                        Some("steer") => session.steer(&command.message),
                        Some("followUp") => session.follow_up(&command.message),
                        _ => {
                            emit_json(&response_error(
                                id,
                                "prompt",
                                "Agent is already streaming. Specify streamingBehavior ('steer' or 'followUp').",
                            ));
                            continue;
                        }
                    }
                    emit_json(&response_success(id, "prompt", None));
                    continue;
                }

                let content = match build_user_content(&command.message, &command.images) {
                    Ok(content) => content,
                    Err(err) => {
                        emit_json(&response_error(id, "prompt", &err));
                        continue;
                    }
                };
                match session.prompt_content(content) {
                    Ok(()) => emit_json(&response_success(id, "prompt", None)),
                    Err(err) => emit_json(&response_error(id, "prompt", &err.to_string())),
                }
            }
            "steer" => {
                let command: RpcPromptCommand = match serde_json::from_value(value) {
                    Ok(command) => command,
                    Err(err) => {
                        emit_json(&response_error(
                            None,
                            "steer",
                            &format!("Invalid payload: {err}"),
                        ));
                        continue;
                    }
                };
                session.steer(&command.message);
                emit_json(&response_success(command.id.as_deref(), "steer", None));
            }
            "follow_up" => {
                let command: RpcPromptCommand = match serde_json::from_value(value) {
                    Ok(command) => command,
                    Err(err) => {
                        emit_json(&response_error(
                            None,
                            "follow_up",
                            &format!("Invalid payload: {err}"),
                        ));
                        continue;
                    }
                };
                session.follow_up(&command.message);
                emit_json(&response_success(command.id.as_deref(), "follow_up", None));
            }
            "abort" => {
                let command: RpcSimpleCommand = match serde_json::from_value(value) {
                    Ok(command) => command,
                    Err(err) => {
                        emit_json(&response_error(
                            None,
                            "abort",
                            &format!("Invalid payload: {err}"),
                        ));
                        continue;
                    }
                };
                session.abort();
                emit_json(&response_success(command.id.as_deref(), "abort", None));
            }
            "new_session" => {
                let command: RpcSimpleCommand = match serde_json::from_value(value) {
                    Ok(command) => command,
                    Err(err) => {
                        emit_json(&response_error(
                            None,
                            "new_session",
                            &format!("Invalid payload: {err}"),
                        ));
                        continue;
                    }
                };
                session.new_session();
                emit_json(&response_success(
                    command.id.as_deref(),
                    "new_session",
                    Some(json!({ "cancelled": false })),
                ));
            }
            "get_state" => {
                let command: RpcSimpleCommand = match serde_json::from_value(value) {
                    Ok(command) => command,
                    Err(err) => {
                        emit_json(&response_error(
                            None,
                            "get_state",
                            &format!("Invalid payload: {err}"),
                        ));
                        continue;
                    }
                };
                let state = session.get_state();
                let data = json!({
                    "model": agent_model_value(&state.model),
                    "thinkingLevel": state.thinking_level.as_str(),
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
                    Some(data),
                ));
            }
            "set_model" => {
                let command: RpcSetModelCommand = match serde_json::from_value(value) {
                    Ok(command) => command,
                    Err(err) => {
                        emit_json(&response_error(
                            None,
                            "set_model",
                            &format!("Invalid payload: {err}"),
                        ));
                        continue;
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
                        continue;
                    }
                };
                session.set_model(crate::agent::Model {
                    id: model.id.clone(),
                    name: model.name.clone(),
                    api: model.api.clone(),
                    provider: model.provider.clone(),
                });
                emit_json(&response_success(
                    command.id.as_deref(),
                    "set_model",
                    Some(serde_json::to_value(model).unwrap_or(Value::Null)),
                ));
            }
            "cycle_model" => {
                let command: RpcSimpleCommand = match serde_json::from_value(value) {
                    Ok(command) => command,
                    Err(err) => {
                        emit_json(&response_error(
                            None,
                            "cycle_model",
                            &format!("Invalid payload: {err}"),
                        ));
                        continue;
                    }
                };
                let result = session.cycle_model();
                let data = result.map(|cycle| {
                    json!({
                        "model": cycle.model,
                        "thinkingLevel": cycle.thinking_level.as_str(),
                        "isScoped": cycle.is_scoped,
                    })
                });
                emit_json(&response_success(
                    command.id.as_deref(),
                    "cycle_model",
                    data,
                ));
            }
            "get_available_models" => {
                let command: RpcSimpleCommand = match serde_json::from_value(value) {
                    Ok(command) => command,
                    Err(err) => {
                        emit_json(&response_error(
                            None,
                            "get_available_models",
                            &format!("Invalid payload: {err}"),
                        ));
                        continue;
                    }
                };
                let models = session.get_available_models();
                emit_json(&response_success(
                    command.id.as_deref(),
                    "get_available_models",
                    Some(json!({ "models": models })),
                ));
            }
            "set_thinking_level" => {
                let command: RpcSetThinkingLevelCommand = match serde_json::from_value(value) {
                    Ok(command) => command,
                    Err(err) => {
                        emit_json(&response_error(
                            None,
                            "set_thinking_level",
                            &format!("Invalid payload: {err}"),
                        ));
                        continue;
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
                        continue;
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
                let command: RpcSimpleCommand = match serde_json::from_value(value) {
                    Ok(command) => command,
                    Err(err) => {
                        emit_json(&response_error(
                            None,
                            "cycle_thinking_level",
                            &format!("Invalid payload: {err}"),
                        ));
                        continue;
                    }
                };
                let result = session.cycle_thinking_level();
                emit_json(&response_success(
                    command.id.as_deref(),
                    "cycle_thinking_level",
                    Some(json!({ "level": result.level.as_str() })),
                ));
            }
            "set_steering_mode" | "set_follow_up_mode" => {
                let command: RpcSetQueueModeCommand = match serde_json::from_value(value) {
                    Ok(command) => command,
                    Err(err) => {
                        emit_json(&response_error(
                            None,
                            &kind,
                            &format!("Invalid payload: {err}"),
                        ));
                        continue;
                    }
                };
                let mode = match queue_mode_from_str(&command.mode) {
                    Some(mode) => mode,
                    None => {
                        emit_json(&response_error(
                            command.id.as_deref(),
                            &kind,
                            "Invalid queue mode",
                        ));
                        continue;
                    }
                };
                if kind == "set_steering_mode" {
                    session.set_steering_mode(mode);
                } else {
                    session.set_follow_up_mode(mode);
                }
                emit_json(&response_success(command.id.as_deref(), &kind, None));
            }
            "compact" => {
                let command: RpcCompactCommand = match serde_json::from_value(value) {
                    Ok(command) => command,
                    Err(err) => {
                        emit_json(&response_error(
                            None,
                            "compact",
                            &format!("Invalid payload: {err}"),
                        ));
                        continue;
                    }
                };
                match session.compact_with_instructions(command.custom_instructions.as_deref()) {
                    Ok(result) => emit_json(&response_success(
                        command.id.as_deref(),
                        "compact",
                        Some(json!({
                            "summary": result.summary,
                            "firstKeptEntryId": result.first_kept_entry_id,
                            "tokensBefore": result.tokens_before,
                        })),
                    )),
                    Err(err) => emit_json(&response_error(
                        command.id.as_deref(),
                        "compact",
                        &err.to_string(),
                    )),
                }
            }
            "set_auto_compaction" => {
                let command: RpcSetAutoCommand = match serde_json::from_value(value) {
                    Ok(command) => command,
                    Err(err) => {
                        emit_json(&response_error(
                            None,
                            "set_auto_compaction",
                            &format!("Invalid payload: {err}"),
                        ));
                        continue;
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
                let command: RpcSetAutoCommand = match serde_json::from_value(value) {
                    Ok(command) => command,
                    Err(err) => {
                        emit_json(&response_error(
                            None,
                            "set_auto_retry",
                            &format!("Invalid payload: {err}"),
                        ));
                        continue;
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
                let command: RpcSimpleCommand = match serde_json::from_value(value) {
                    Ok(command) => command,
                    Err(err) => {
                        emit_json(&response_error(
                            None,
                            "abort_retry",
                            &format!("Invalid payload: {err}"),
                        ));
                        continue;
                    }
                };
                session.abort_retry();
                emit_json(&response_success(
                    command.id.as_deref(),
                    "abort_retry",
                    None,
                ));
            }
            "bash" => {
                let command: RpcBashCommand = match serde_json::from_value(value) {
                    Ok(command) => command,
                    Err(err) => {
                        emit_json(&response_error(
                            None,
                            "bash",
                            &format!("Invalid payload: {err}"),
                        ));
                        continue;
                    }
                };
                match session.execute_bash(&command.command) {
                    Ok(result) => emit_json(&response_success(
                        command.id.as_deref(),
                        "bash",
                        Some(json!({
                            "output": result.output,
                            "exitCode": result.exit_code,
                            "cancelled": result.cancelled,
                            "truncated": false,
                        })),
                    )),
                    Err(err) => emit_json(&response_error(
                        command.id.as_deref(),
                        "bash",
                        &err.to_string(),
                    )),
                }
            }
            "abort_bash" => {
                let command: RpcSimpleCommand = match serde_json::from_value(value) {
                    Ok(command) => command,
                    Err(err) => {
                        emit_json(&response_error(
                            None,
                            "abort_bash",
                            &format!("Invalid payload: {err}"),
                        ));
                        continue;
                    }
                };
                session.abort_bash();
                emit_json(&response_success(command.id.as_deref(), "abort_bash", None));
            }
            "get_session_stats" => {
                let command: RpcSimpleCommand = match serde_json::from_value(value) {
                    Ok(command) => command,
                    Err(err) => {
                        emit_json(&response_error(
                            None,
                            "get_session_stats",
                            &format!("Invalid payload: {err}"),
                        ));
                        continue;
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
                let command: RpcExportHtmlCommand = match serde_json::from_value(value) {
                    Ok(command) => command,
                    Err(err) => {
                        emit_json(&response_error(
                            None,
                            "export_html",
                            &format!("Invalid payload: {err}"),
                        ));
                        continue;
                    }
                };
                let output_path = command.output_path.map(PathBuf::from);
                match session.export_to_html_with_path(output_path.as_ref()) {
                    Ok(result) => emit_json(&response_success(
                        command.id.as_deref(),
                        "export_html",
                        Some(json!({ "path": result.path.to_string_lossy() })),
                    )),
                    Err(err) => emit_json(&response_error(
                        command.id.as_deref(),
                        "export_html",
                        &err.to_string(),
                    )),
                }
            }
            "switch_session" => {
                let command: RpcSwitchSessionCommand = match serde_json::from_value(value) {
                    Ok(command) => command,
                    Err(err) => {
                        emit_json(&response_error(
                            None,
                            "switch_session",
                            &format!("Invalid payload: {err}"),
                        ));
                        continue;
                    }
                };
                match session.switch_session(PathBuf::from(command.session_path)) {
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
                let command: RpcBranchCommand = match serde_json::from_value(value) {
                    Ok(command) => command,
                    Err(err) => {
                        emit_json(&response_error(
                            None,
                            "branch",
                            &format!("Invalid payload: {err}"),
                        ));
                        continue;
                    }
                };
                match session.branch(&command.entry_id) {
                    Ok(result) => emit_json(&response_success(
                        command.id.as_deref(),
                        "branch",
                        Some(json!({
                            "text": result.selected_text,
                            "cancelled": result.cancelled,
                        })),
                    )),
                    Err(err) => emit_json(&response_error(
                        command.id.as_deref(),
                        "branch",
                        &err.to_string(),
                    )),
                }
            }
            "get_branch_messages" => {
                let command: RpcSimpleCommand = match serde_json::from_value(value) {
                    Ok(command) => command,
                    Err(err) => {
                        emit_json(&response_error(
                            None,
                            "get_branch_messages",
                            &format!("Invalid payload: {err}"),
                        ));
                        continue;
                    }
                };
                let candidates = session.get_user_messages_for_branching();
                let messages = candidates
                    .into_iter()
                    .map(|candidate| {
                        json!({
                            "entryId": candidate.entry_id,
                            "text": candidate.text,
                        })
                    })
                    .collect::<Vec<_>>();
                emit_json(&response_success(
                    command.id.as_deref(),
                    "get_branch_messages",
                    Some(json!({ "messages": messages })),
                ));
            }
            "get_last_assistant_text" => {
                let command: RpcSimpleCommand = match serde_json::from_value(value) {
                    Ok(command) => command,
                    Err(err) => {
                        emit_json(&response_error(
                            None,
                            "get_last_assistant_text",
                            &format!("Invalid payload: {err}"),
                        ));
                        continue;
                    }
                };
                let message = session.get_last_assistant_text();
                emit_json(&response_success(
                    command.id.as_deref(),
                    "get_last_assistant_text",
                    Some(json!({ "text": message })),
                ));
            }
            "get_messages" => {
                let command: RpcSimpleCommand = match serde_json::from_value(value) {
                    Ok(command) => command,
                    Err(err) => {
                        emit_json(&response_error(
                            None,
                            "get_messages",
                            &format!("Invalid payload: {err}"),
                        ));
                        continue;
                    }
                };
                let messages = session
                    .messages()
                    .iter()
                    .map(serialize_agent_message)
                    .collect::<Vec<_>>();
                emit_json(&response_success(
                    command.id.as_deref(),
                    "get_messages",
                    Some(json!({ "messages": messages })),
                ));
            }
            _ => {
                emit_json(&response_error(None, &kind, "Unknown command"));
            }
        }
    }

    Ok(())
}

fn response_success(id: Option<&str>, command: &str, data: Option<Value>) -> Value {
    let mut map = Map::new();
    map.insert("type".to_string(), Value::String("response".to_string()));
    if let Some(id) = id {
        map.insert("id".to_string(), Value::String(id.to_string()));
    }
    map.insert("command".to_string(), Value::String(command.to_string()));
    map.insert("success".to_string(), Value::Bool(true));
    if let Some(data) = data {
        map.insert("data".to_string(), data);
    }
    Value::Object(map)
}

fn response_error(id: Option<&str>, command: &str, error: &str) -> Value {
    let mut map = Map::new();
    map.insert("type".to_string(), Value::String("response".to_string()));
    if let Some(id) = id {
        map.insert("id".to_string(), Value::String(id.to_string()));
    }
    map.insert("command".to_string(), Value::String(command.to_string()));
    map.insert("success".to_string(), Value::Bool(false));
    map.insert("error".to_string(), Value::String(error.to_string()));
    Value::Object(map)
}

fn emit_json(value: &Value) {
    let output = serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string());
    println!("{output}");
    let _ = io::stdout().flush();
}

fn extension_ui_request_to_value(request: &ExtensionUiRequest) -> Value {
    let mut map = Map::new();
    map.insert(
        "type".to_string(),
        Value::String("extension_ui_request".to_string()),
    );
    map.insert("id".to_string(), Value::String(request.id.clone()));
    map.insert("method".to_string(), Value::String(request.method.clone()));
    if let Some(title) = request.title.as_ref() {
        map.insert("title".to_string(), Value::String(title.clone()));
    }
    if let Some(message) = request.message.as_ref() {
        map.insert("message".to_string(), Value::String(message.clone()));
    }
    if let Some(options) = request.options.as_ref() {
        map.insert(
            "options".to_string(),
            Value::Array(
                options
                    .iter()
                    .map(|opt| Value::String(opt.clone()))
                    .collect(),
            ),
        );
    }
    if let Some(placeholder) = request.placeholder.as_ref() {
        map.insert(
            "placeholder".to_string(),
            Value::String(placeholder.clone()),
        );
    }
    if let Some(prefill) = request.prefill.as_ref() {
        map.insert("prefill".to_string(), Value::String(prefill.clone()));
    }
    if let Some(notify_type) = request.notify_type.as_ref() {
        map.insert("notifyType".to_string(), Value::String(notify_type.clone()));
    }
    if let Some(status_key) = request.status_key.as_ref() {
        map.insert("statusKey".to_string(), Value::String(status_key.clone()));
    }
    if let Some(status_text) = request.status_text.as_ref() {
        map.insert("statusText".to_string(), Value::String(status_text.clone()));
    }
    if let Some(widget_key) = request.widget_key.as_ref() {
        map.insert("widgetKey".to_string(), Value::String(widget_key.clone()));
    }
    if let Some(widget_lines) = request.widget_lines.as_ref() {
        map.insert(
            "widgetLines".to_string(),
            Value::Array(
                widget_lines
                    .iter()
                    .map(|line| Value::String(line.clone()))
                    .collect(),
            ),
        );
    }
    if let Some(text) = request.text.as_ref() {
        map.insert("text".to_string(), Value::String(text.clone()));
    }
    Value::Object(map)
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

fn agent_model_value(model: &crate::agent::Model) -> Value {
    json!({
        "id": model.id,
        "name": model.name,
        "api": model.api,
        "provider": model.provider,
    })
}
