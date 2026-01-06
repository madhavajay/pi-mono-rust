use crate::coding_agent::hooks::{
    CompactionResult, SessionBeforeCompactEvent, SessionBeforeCompactResult, SessionCompactEvent,
};
use crate::core::compaction::{CompactionPreparation, CompactionSettings, FileOperations};
use crate::core::messages::{AgentMessage, ContentBlock};
use crate::core::session_manager::SessionEntry;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

const EXTENSION_HOST_JS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/assets/extension-host.js"
));

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionTool {
    pub name: String,
    pub label: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub parameters: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionCommand {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionFlag {
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "type")]
    pub flag_type: Option<String>,
    pub default: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionShortcut {
    pub shortcut: String,
    pub description: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionMessageRenderer {
    pub custom_type: String,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionMetadata {
    pub path: String,
    pub tools: Vec<ExtensionTool>,
    pub commands: Vec<ExtensionCommand>,
    pub flags: Vec<ExtensionFlag>,
    pub shortcuts: Vec<ExtensionShortcut>,
    pub message_renderers: Vec<ExtensionMessageRenderer>,
    pub handler_counts: HashMap<String, usize>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionHostError {
    pub extension_path: String,
    pub error: String,
    pub event: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExtensionManifest {
    pub extensions: Vec<ExtensionMetadata>,
    pub errors: Vec<ExtensionHostError>,
    pub skipped_paths: Vec<PathBuf>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InitRequest<'a> {
    id: u64,
    #[serde(rename = "type")]
    kind: &'static str,
    extensions: &'a [String],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EmitRequest<'a> {
    id: u64,
    #[serde(rename = "type")]
    kind: &'static str,
    event: &'a ExtensionEventPayload<'a>,
    context: ExtensionContextPayload,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SetFlagsRequest<'a> {
    id: u64,
    #[serde(rename = "type")]
    kind: &'static str,
    flags: &'a HashMap<String, Value>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InvokeToolRequest<'a> {
    id: u64,
    #[serde(rename = "type")]
    kind: &'static str,
    name: &'a str,
    #[serde(rename = "toolCallId")]
    tool_call_id: &'a str,
    input: &'a Value,
    context: ExtensionContextPayload,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HostResponse {
    ok: bool,
    error: Option<String>,
    result: Option<Value>,
    extensions: Option<Vec<ExtensionMetadata>>,
    errors: Option<Vec<ExtensionHostError>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExtensionContextPayload {
    cwd: String,
    has_ui: bool,
    is_idle: bool,
    has_pending_messages: bool,
    model: Option<ExtensionModelPayload>,
    session_entries: Vec<SessionEntry>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExtensionModelPayload {
    id: String,
    name: String,
    api: String,
    provider: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExtensionEventPayload<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    preparation: Option<ExtensionCompactionPreparation<'a>>,
    branch_entries: Option<&'a [SessionEntry]>,
    compaction_entry: Option<&'a crate::core::session_manager::CompactionEntry>,
    from_extension: Option<bool>,
    messages: Option<&'a [AgentMessage]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_name: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    input: Option<&'a Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<&'a [ContentBlock]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<&'a Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    is_error: Option<bool>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExtensionCompactionPreparation<'a> {
    first_kept_entry_id: &'a str,
    messages_to_summarize: &'a [AgentMessage],
    turn_prefix_messages: &'a [AgentMessage],
    is_split_turn: bool,
    tokens_before: i64,
    previous_summary: Option<&'a str>,
    file_ops: ExtensionFileOperations,
    settings: ExtensionCompactionSettings,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExtensionFileOperations {
    read: Vec<String>,
    written: Vec<String>,
    edited: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExtensionCompactionSettings {
    enabled: bool,
    reserve_tokens: i64,
    keep_recent_tokens: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExtensionCompactionResult {
    summary: String,
    first_kept_entry_id: String,
    tokens_before: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExtensionBeforeCompactResult {
    cancel: Option<bool>,
    compaction: Option<ExtensionCompactionResult>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExtensionToolInvokeResult {
    content: Option<Vec<ContentBlock>>,
    details: Option<Value>,
    is_error: Option<bool>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionToolCallResult {
    pub block: Option<bool>,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionToolResult {
    pub content: Option<Vec<ContentBlock>>,
    pub details: Option<Value>,
    pub is_error: Option<bool>,
}

pub struct ExtensionToolExecuteResult {
    pub content: Vec<ContentBlock>,
    pub details: Option<Value>,
    pub is_error: bool,
}

pub struct ExtensionHost {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
    script_path: PathBuf,
    cwd: String,
}

impl ExtensionHost {
    pub fn spawn(paths: &[PathBuf], cwd: &Path) -> Result<(Self, ExtensionManifest), String> {
        if paths.is_empty() {
            return Err("No extension paths provided".to_string());
        }

        let mut skipped_paths = Vec::new();
        let mut supported = Vec::new();
        for path in paths {
            match path.extension().and_then(|ext| ext.to_str()) {
                Some("js") | Some("ts") | Some("tsx") => supported.push(path.clone()),
                _ => skipped_paths.push(path.clone()),
            }
        }

        if supported.is_empty() {
            return Err("No supported extension files found (JS/TS only).".to_string());
        }

        let script_path = write_host_script()?;
        let mut child = Command::new("node")
            .arg(&script_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .current_dir(cwd)
            .spawn()
            .map_err(|err| format!("Failed to start node extension host: {err}"))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "Failed to capture extension host stdin".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Failed to capture extension host stdout".to_string())?;
        let mut host = ExtensionHost {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 1,
            script_path: script_path.clone(),
            cwd: cwd.to_string_lossy().to_string(),
        };

        let extension_paths = supported
            .iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        let request = InitRequest {
            id: host.next_id(),
            kind: "init",
            extensions: &extension_paths,
        };
        let response = host.send_request(request)?;
        if !response.ok {
            return Err(response
                .error
                .unwrap_or_else(|| "Extension host init failed".to_string()));
        }

        let manifest = ExtensionManifest {
            extensions: response.extensions.unwrap_or_default(),
            errors: response.errors.unwrap_or_default(),
            skipped_paths,
        };

        Ok((host, manifest))
    }

    pub fn emit_before_compact(
        &mut self,
        event: &SessionBeforeCompactEvent,
    ) -> Result<SessionBeforeCompactResult, String> {
        let payload = ExtensionEventPayload {
            kind: "session_before_compact",
            preparation: Some(to_extension_preparation(&event.preparation)),
            branch_entries: Some(&event.branch_entries),
            compaction_entry: None,
            from_extension: None,
            messages: None,
            tool_name: None,
            tool_call_id: None,
            input: None,
            content: None,
            details: None,
            is_error: None,
        };
        let context = self.build_context(&event.branch_entries);
        let response = self.emit_event(&payload, context)?;
        if !response.ok {
            return Err(response
                .error
                .unwrap_or_else(|| "Extension host emit failed".to_string()));
        }
        let result = match response.result {
            Some(Value::Null) | None => SessionBeforeCompactResult::default(),
            Some(value) => serde_json::from_value::<ExtensionBeforeCompactResult>(value)
                .map_err(|err| format!("Failed to parse extension result: {err}"))
                .map(convert_before_compact_result)?,
        };
        Ok(result)
    }

    pub fn emit_compact(&mut self, event: &SessionCompactEvent) -> Result<(), String> {
        let payload = ExtensionEventPayload {
            kind: "session_compact",
            preparation: None,
            branch_entries: None,
            compaction_entry: Some(&event.compaction_entry),
            from_extension: Some(event.from_hook),
            messages: None,
            tool_name: None,
            tool_call_id: None,
            input: None,
            content: None,
            details: None,
            is_error: None,
        };
        let context = self.build_context(&[]);
        let response = self.emit_event(&payload, context)?;
        if !response.ok {
            return Err(response
                .error
                .unwrap_or_else(|| "Extension host emit failed".to_string()));
        }
        Ok(())
    }

    pub fn emit_context(
        &mut self,
        messages: &[AgentMessage],
    ) -> Result<Vec<ExtensionHostError>, String> {
        let payload = ExtensionEventPayload {
            kind: "context",
            preparation: None,
            branch_entries: None,
            compaction_entry: None,
            from_extension: None,
            messages: Some(messages),
            tool_name: None,
            tool_call_id: None,
            input: None,
            content: None,
            details: None,
            is_error: None,
        };
        let context = self.build_context(&[]);
        let response = self.emit_event(&payload, context)?;
        if !response.ok {
            return Err(response
                .error
                .unwrap_or_else(|| "Extension host emit failed".to_string()));
        }
        Ok(response.errors.unwrap_or_default())
    }

    pub fn emit_tool_call(
        &mut self,
        tool_name: &str,
        tool_call_id: &str,
        input: &Value,
    ) -> Result<ExtensionToolCallResult, String> {
        let payload = ExtensionEventPayload {
            kind: "tool_call",
            preparation: None,
            branch_entries: None,
            compaction_entry: None,
            from_extension: None,
            messages: None,
            tool_name: Some(tool_name),
            tool_call_id: Some(tool_call_id),
            input: Some(input),
            content: None,
            details: None,
            is_error: None,
        };
        let context = self.build_context(&[]);
        let response = self.emit_event(&payload, context)?;
        if !response.ok {
            return Err(response
                .error
                .unwrap_or_else(|| "Extension host emit failed".to_string()));
        }
        if let Some(errors) = response.errors.as_deref() {
            report_extension_errors(errors);
        }
        let result = match response.result {
            Some(Value::Null) | None => ExtensionToolCallResult::default(),
            Some(value) => serde_json::from_value::<ExtensionToolCallResult>(value)
                .map_err(|err| format!("Failed to parse extension result: {err}"))?,
        };
        Ok(result)
    }

    pub fn emit_tool_result(
        &mut self,
        tool_name: &str,
        tool_call_id: &str,
        input: &Value,
        content: &[ContentBlock],
        details: &Value,
        is_error: bool,
    ) -> Result<ExtensionToolResult, String> {
        let payload = ExtensionEventPayload {
            kind: "tool_result",
            preparation: None,
            branch_entries: None,
            compaction_entry: None,
            from_extension: None,
            messages: None,
            tool_name: Some(tool_name),
            tool_call_id: Some(tool_call_id),
            input: Some(input),
            content: Some(content),
            details: Some(details),
            is_error: Some(is_error),
        };
        let context = self.build_context(&[]);
        let response = self.emit_event(&payload, context)?;
        if !response.ok {
            return Err(response
                .error
                .unwrap_or_else(|| "Extension host emit failed".to_string()));
        }
        if let Some(errors) = response.errors.as_deref() {
            report_extension_errors(errors);
        }
        let result = match response.result {
            Some(Value::Null) | None => ExtensionToolResult::default(),
            Some(value) => serde_json::from_value::<ExtensionToolResult>(value)
                .map_err(|err| format!("Failed to parse extension result: {err}"))?,
        };
        Ok(result)
    }

    pub fn set_flag_values(&mut self, flags: &HashMap<String, Value>) -> Result<(), String> {
        if flags.is_empty() {
            return Ok(());
        }
        let request = SetFlagsRequest {
            id: self.next_id(),
            kind: "set_flags",
            flags,
        };
        let response = self.send_request(request)?;
        if !response.ok {
            return Err(response
                .error
                .unwrap_or_else(|| "Extension host set_flags failed".to_string()));
        }
        Ok(())
    }

    pub fn call_tool(
        &mut self,
        tool_name: &str,
        tool_call_id: &str,
        input: &Value,
        session_entries: &[SessionEntry],
    ) -> Result<ExtensionToolExecuteResult, String> {
        let request = InvokeToolRequest {
            id: self.next_id(),
            kind: "invoke_tool",
            name: tool_name,
            tool_call_id,
            input,
            context: self.build_context(session_entries),
        };
        let response = self.send_request(request)?;
        if !response.ok {
            return Err(response
                .error
                .unwrap_or_else(|| "Extension tool invocation failed".to_string()));
        }
        let value = response.result.unwrap_or(Value::Null);
        parse_tool_invoke_result(value)
    }

    fn build_context(&self, session_entries: &[SessionEntry]) -> ExtensionContextPayload {
        ExtensionContextPayload {
            cwd: self.cwd.clone(),
            has_ui: false,
            is_idle: true,
            has_pending_messages: false,
            model: None,
            session_entries: session_entries.to_vec(),
        }
    }

    fn send_request<T: Serialize>(&mut self, request: T) -> Result<HostResponse, String> {
        let line = serde_json::to_string(&request)
            .map_err(|err| format!("Failed to serialize extension request: {err}"))?;
        self.stdin
            .write_all(line.as_bytes())
            .and_then(|_| self.stdin.write_all(b"\n"))
            .map_err(|err| format!("Failed to send extension request: {err}"))?;
        self.stdin
            .flush()
            .map_err(|err| format!("Failed to flush extension request: {err}"))?;

        let mut response_line = String::new();
        let bytes = self
            .stdout
            .read_line(&mut response_line)
            .map_err(|err| format!("Failed to read extension response: {err}"))?;
        if bytes == 0 {
            return Err("Extension host closed unexpectedly".to_string());
        }
        serde_json::from_str(&response_line)
            .map_err(|err| format!("Failed to parse extension response: {err}"))
    }

    fn emit_event(
        &mut self,
        payload: &ExtensionEventPayload<'_>,
        context: ExtensionContextPayload,
    ) -> Result<HostResponse, String> {
        let request = EmitRequest {
            id: self.next_id(),
            kind: "emit",
            event: payload,
            context,
        };
        self.send_request(request)
    }

    fn next_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

fn report_extension_errors(errors: &[ExtensionHostError]) {
    for error in errors {
        if let Some(event) = error.event.as_deref() {
            eprintln!(
                "Warning: Extension error in {} ({}): {}",
                event, error.extension_path, error.error
            );
        } else {
            eprintln!(
                "Warning: Extension error ({}): {}",
                error.extension_path, error.error
            );
        }
    }
}

impl Drop for ExtensionHost {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = fs::remove_file(&self.script_path);
    }
}

fn write_host_script() -> Result<PathBuf, String> {
    let mut path = std::env::temp_dir();
    path.push(format!("pi-extension-host-{}.js", uuid::Uuid::new_v4()));
    fs::write(&path, EXTENSION_HOST_JS)
        .map_err(|err| format!("Failed to write extension host script: {err}"))?;
    Ok(path)
}

fn to_extension_preparation(prep: &CompactionPreparation) -> ExtensionCompactionPreparation<'_> {
    ExtensionCompactionPreparation {
        first_kept_entry_id: &prep.first_kept_entry_id,
        messages_to_summarize: &prep.messages_to_summarize,
        turn_prefix_messages: &prep.turn_prefix_messages,
        is_split_turn: prep.is_split_turn,
        tokens_before: prep.tokens_before,
        previous_summary: prep.previous_summary.as_deref(),
        file_ops: to_extension_file_ops(&prep.file_ops),
        settings: to_extension_settings(prep.settings),
    }
}

fn to_extension_file_ops(file_ops: &FileOperations) -> ExtensionFileOperations {
    ExtensionFileOperations {
        read: sorted_vec(&file_ops.read),
        written: sorted_vec(&file_ops.written),
        edited: sorted_vec(&file_ops.edited),
    }
}

fn sorted_vec(set: &std::collections::HashSet<String>) -> Vec<String> {
    let mut values = set.iter().cloned().collect::<Vec<_>>();
    values.sort();
    values
}

fn to_extension_settings(settings: CompactionSettings) -> ExtensionCompactionSettings {
    ExtensionCompactionSettings {
        enabled: settings.enabled,
        reserve_tokens: settings.reserve_tokens,
        keep_recent_tokens: settings.keep_recent_tokens,
    }
}

fn convert_before_compact_result(
    result: ExtensionBeforeCompactResult,
) -> SessionBeforeCompactResult {
    SessionBeforeCompactResult {
        cancel: result.cancel,
        compaction: result.compaction.map(|compaction| CompactionResult {
            summary: compaction.summary,
            first_kept_entry_id: compaction.first_kept_entry_id,
            tokens_before: compaction.tokens_before,
        }),
    }
}

fn parse_tool_invoke_result(value: Value) -> Result<ExtensionToolExecuteResult, String> {
    if value.is_null() {
        return Ok(ExtensionToolExecuteResult {
            content: Vec::new(),
            details: None,
            is_error: false,
        });
    }
    if let Some(text) = value.as_str() {
        return Ok(ExtensionToolExecuteResult {
            content: vec![ContentBlock::Text {
                text: text.to_string(),
                text_signature: None,
            }],
            details: None,
            is_error: false,
        });
    }
    let parsed = serde_json::from_value::<ExtensionToolInvokeResult>(value)
        .map_err(|err| format!("Failed to parse tool result: {err}"))?;
    Ok(ExtensionToolExecuteResult {
        content: parsed.content.unwrap_or_default(),
        details: parsed.details,
        is_error: parsed.is_error.unwrap_or(false),
    })
}
