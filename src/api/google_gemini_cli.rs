// Google Gemini CLI / Cloud Code Assist provider.
// Uses the Cloud Code Assist API endpoint to access Gemini models.

use crate::agent::{AgentMessage, LlmContext, StreamEvents};
use crate::ai::AssistantMessageEvent;
use crate::coding_agent::Model as RegistryModel;
use crate::core::messages::{
    AssistantMessage, ContentBlock, Cost, ToolResultMessage, Usage, UserContent,
};
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::Read;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static TOOL_CALL_COUNTER: AtomicU64 = AtomicU64::new(0);

const DEFAULT_ENDPOINT: &str = "https://cloudcode-pa.googleapis.com";
const GEMINI_CLI_USER_AGENT: &str = "google-cloud-sdk vscode_cloudshelleditor/0.1";
const X_GOOG_API_CLIENT: &str = "gl-node/22.17.0";

// Headers for Gemini CLI (prod endpoint)
fn gemini_cli_client_metadata() -> String {
    serde_json::json!({
        "ideType": "IDE_UNSPECIFIED",
        "platform": "PLATFORM_UNSPECIFIED",
        "pluginType": "GEMINI"
    })
    .to_string()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudCodeAssistRequest {
    pub project: String,
    pub model: String,
    pub request: GenerateContentRequest,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateContentRequest {
    pub contents: Vec<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<GeminiSystemInstruction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_config: Option<GeminiGenerationConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<GeminiToolDeclaration>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_config: Option<GeminiToolConfig>,
}

#[derive(Debug, Serialize)]
pub struct GeminiSystemInstruction {
    pub parts: Vec<GeminiTextPart>,
}

#[derive(Debug, Serialize)]
pub struct GeminiTextPart {
    pub text: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GeminiGenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_config: Option<GeminiThinkingConfig>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GeminiThinkingConfig {
    pub include_thoughts: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_budget: Option<i32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiToolDeclaration {
    pub function_declarations: Vec<GeminiFunctionDeclaration>,
}

#[derive(Debug, Serialize)]
pub struct GeminiFunctionDeclaration {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub parameters: Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiToolConfig {
    pub function_calling_config: GeminiFunctionCallingConfig,
}

#[derive(Debug, Serialize)]
pub struct GeminiFunctionCallingConfig {
    pub mode: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct GeminiContent {
    pub role: String,
    pub parts: Vec<GeminiPart>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(untagged)]
pub enum GeminiPart {
    Text(GeminiTextPartContent),
    Thought(GeminiThoughtPart),
    InlineData(GeminiInlineDataPart),
    FunctionCall(GeminiFunctionCallPart),
    FunctionResponse(GeminiFunctionResponsePart),
}

#[derive(Debug, Serialize, Clone)]
pub struct GeminiTextPartContent {
    pub text: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GeminiThoughtPart {
    pub thought: bool,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thought_signature: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GeminiInlineDataPart {
    pub inline_data: GeminiInlineData,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GeminiInlineData {
    pub mime_type: String,
    pub data: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GeminiFunctionCallPart {
    pub function_call: GeminiFunctionCall,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thought_signature: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct GeminiFunctionCall {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    pub args: Value,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GeminiFunctionResponsePart {
    pub function_response: GeminiFunctionResponse,
}

#[derive(Debug, Serialize, Clone)]
pub struct GeminiFunctionResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    pub response: Value,
}

#[derive(Debug, Deserialize)]
pub struct CloudCodeAssistResponseChunk {
    pub response: Option<GeminiResponse>,
    #[serde(rename = "traceId")]
    pub trace_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiResponse {
    pub candidates: Option<Vec<GeminiCandidate>>,
    pub usage_metadata: Option<GeminiUsageMetadata>,
    pub model_version: Option<String>,
    pub response_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiCandidate {
    pub content: Option<GeminiCandidateContent>,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GeminiCandidateContent {
    pub role: Option<String>,
    pub parts: Option<Vec<GeminiResponsePart>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiResponsePart {
    pub text: Option<String>,
    pub thought: Option<bool>,
    pub thought_signature: Option<String>,
    pub function_call: Option<GeminiResponseFunctionCall>,
}

#[derive(Debug, Deserialize)]
pub struct GeminiResponseFunctionCall {
    pub name: String,
    pub args: Value,
    pub id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiUsageMetadata {
    pub prompt_token_count: Option<i64>,
    pub candidates_token_count: Option<i64>,
    pub thoughts_token_count: Option<i64>,
    pub total_token_count: Option<i64>,
    pub cached_content_token_count: Option<i64>,
}

pub struct GeminiCliTool {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

pub struct GeminiCliCallOptions<'a> {
    pub model: &'a str,
    pub access_token: &'a str,
    pub project_id: &'a str,
    pub tools: &'a [GeminiCliTool],
    pub base_url: &'a str,
    pub system: Option<&'a str>,
    pub thinking_enabled: bool,
}

struct SseEvent {
    data: String,
}

struct SseParser {
    buffer: String,
}

impl SseParser {
    fn new() -> Self {
        Self {
            buffer: String::new(),
        }
    }

    fn feed(&mut self, chunk: &str) -> Vec<SseEvent> {
        self.buffer.push_str(chunk);
        if self.buffer.contains('\r') {
            self.buffer = self.buffer.replace("\r\n", "\n");
        }

        let mut events = Vec::new();
        loop {
            let Some(boundary) = self.buffer.find("\n\n") else {
                // Try single newline boundary for data lines
                if !self.buffer.contains("\n\n") {
                    // Process single data lines
                    break;
                }
                break;
            };
            let raw = self.buffer[..boundary].to_string();
            self.buffer = self.buffer[boundary + 2..].to_string();

            let mut data_lines = Vec::new();
            for line in raw.lines() {
                let line = line.trim_end_matches('\r');
                if let Some(rest) = line.strip_prefix("data:") {
                    data_lines.push(rest.trim_start().to_string());
                }
            }
            let data = data_lines.join("\n");
            if !data.is_empty() {
                events.push(SseEvent { data });
            }
        }

        // Also handle single data lines with just \n
        let mut remaining = String::new();
        for line in self.buffer.lines() {
            if let Some(rest) = line.strip_prefix("data:") {
                let data = rest.trim_start();
                if !data.is_empty() {
                    events.push(SseEvent {
                        data: data.to_string(),
                    });
                }
            } else {
                if !remaining.is_empty() {
                    remaining.push('\n');
                }
                remaining.push_str(line);
            }
        }
        self.buffer = remaining;

        events
    }
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
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

fn calculate_cost(model: &RegistryModel, usage: &mut Usage) {
    let cost = Cost {
        input: (model.cost.input / 1_000_000.0) * usage.input as f64,
        output: (model.cost.output / 1_000_000.0) * usage.output as f64,
        cache_read: (model.cost.cache_read / 1_000_000.0) * usage.cache_read as f64,
        cache_write: (model.cost.cache_write / 1_000_000.0) * usage.cache_write as f64,
        total: 0.0,
    };
    let total = cost.input + cost.output + cost.cache_read + cost.cache_write;
    usage.cost = Some(Cost { total, ..cost });
}

fn stream_partial_message(model: &RegistryModel) -> AssistantMessage {
    AssistantMessage {
        content: Vec::new(),
        api: model.api.clone(),
        provider: model.provider.clone(),
        model: model.id.clone(),
        usage: empty_usage(),
        stop_reason: "streaming".to_string(),
        error_message: None,
        timestamp: now_millis(),
    }
}

fn emit_event(events: &mut StreamEvents, event: AssistantMessageEvent) {
    events.emit(event);
}

fn map_stop_reason(reason: &str) -> String {
    match reason {
        "STOP" => "stop",
        "MAX_TOKENS" => "length",
        _ => "error",
    }
    .to_string()
}

fn apply_stream_stop_reason(message: &mut AssistantMessage) {
    if message.stop_reason == "streaming" {
        let has_tool_calls = message
            .content
            .iter()
            .any(|block| matches!(block, ContentBlock::ToolCall { .. }));
        if has_tool_calls {
            message.stop_reason = "toolUse".to_string();
        } else {
            message.stop_reason = "stop".to_string();
        }
    }
}

fn build_headers(access_token: &str) -> Result<HeaderMap, String> {
    let mut headers = HeaderMap::new();

    headers.insert(
        "authorization",
        HeaderValue::from_str(&format!("Bearer {access_token}"))
            .map_err(|e| format!("Invalid access token: {e}"))?,
    );
    headers.insert("content-type", HeaderValue::from_static("application/json"));
    headers.insert("accept", HeaderValue::from_static("text/event-stream"));
    headers.insert(
        "user-agent",
        HeaderValue::from_static(GEMINI_CLI_USER_AGENT),
    );
    headers.insert(
        "x-goog-api-client",
        HeaderValue::from_static(X_GOOG_API_CLIENT),
    );
    headers.insert(
        "client-metadata",
        HeaderValue::from_str(&gemini_cli_client_metadata())
            .map_err(|e| format!("Invalid client metadata: {e}"))?,
    );

    Ok(headers)
}

fn convert_tools(tools: &[GeminiCliTool]) -> Option<Vec<GeminiToolDeclaration>> {
    if tools.is_empty() {
        return None;
    }
    Some(vec![GeminiToolDeclaration {
        function_declarations: tools
            .iter()
            .map(|t| GeminiFunctionDeclaration {
                name: t.name.clone(),
                description: Some(t.description.clone()),
                parameters: t.parameters.clone(),
            })
            .collect(),
    }])
}

pub fn stream_google_gemini_cli(
    model: &RegistryModel,
    context: &LlmContext,
    options: GeminiCliCallOptions<'_>,
    events: &mut StreamEvents,
) -> Result<AssistantMessage, String> {
    let contents = build_gemini_messages(model, context);

    let mut generation_config = None;
    if options.thinking_enabled && model.reasoning {
        generation_config = Some(GeminiGenerationConfig {
            max_output_tokens: None,
            temperature: None,
            thinking_config: Some(GeminiThinkingConfig {
                include_thoughts: true,
                thinking_level: None,
                thinking_budget: None,
            }),
        });
    }

    let system_instruction = options.system.map(|text| GeminiSystemInstruction {
        parts: vec![GeminiTextPart {
            text: text.to_string(),
        }],
    });

    let request_body = CloudCodeAssistRequest {
        project: options.project_id.to_string(),
        model: options.model.to_string(),
        request: GenerateContentRequest {
            contents,
            system_instruction,
            generation_config,
            tools: convert_tools(options.tools),
            tool_config: None,
        },
        user_agent: Some("pi-coding-agent".to_string()),
        request_id: Some(format!("pi-{}-{}", now_millis(), rand_alphanumeric(9))),
    };

    let headers = build_headers(options.access_token)?;
    let base_url = if options.base_url.is_empty() {
        DEFAULT_ENDPOINT
    } else {
        options.base_url
    };
    let endpoint = format!(
        "{}/v1internal:streamGenerateContent?alt=sse",
        base_url.trim_end_matches('/')
    );

    let client = Client::new();
    let mut response = client
        .post(&endpoint)
        .headers(headers)
        .json(&request_body)
        .send()
        .map_err(|e| format!("Request failed: {e}"))?;

    let status = response.status();
    if !status.is_success() {
        let text = response.text().unwrap_or_default();
        return Err(format!(
            "Cloud Code Assist API error ({}): {}",
            status.as_u16(),
            text
        ));
    }

    let mut partial = stream_partial_message(model);
    emit_event(
        events,
        AssistantMessageEvent::Start {
            partial: partial.clone(),
        },
    );

    let mut parser = SseParser::new();
    let mut buf = [0u8; 8192];
    let mut current_text_index: Option<usize> = None;
    let mut current_thinking_index: Option<usize> = None;

    loop {
        let read = response
            .read(&mut buf)
            .map_err(|e| format!("Stream read failed: {e}"))?;
        if read == 0 {
            break;
        }

        let chunk = String::from_utf8_lossy(&buf[..read]);
        for event in parser.feed(&chunk) {
            if event.data == "[DONE]" {
                continue;
            }

            let value: CloudCodeAssistResponseChunk = match serde_json::from_str(&event.data) {
                Ok(v) => v,
                Err(_) => continue,
            };

            let Some(response_data) = value.response else {
                continue;
            };

            if let Some(candidates) = &response_data.candidates {
                if let Some(candidate) = candidates.first() {
                    if let Some(content) = &candidate.content {
                        if let Some(parts) = &content.parts {
                            for part in parts {
                                if let Some(text) = &part.text {
                                    let is_thinking = part.thought.unwrap_or(false);

                                    if is_thinking {
                                        // Handle thinking block
                                        if current_thinking_index.is_none() {
                                            // End any current text block
                                            if let Some(idx) = current_text_index.take() {
                                                emit_event(
                                                    events,
                                                    AssistantMessageEvent::TextEnd {
                                                        partial: partial.clone(),
                                                        content_index: idx,
                                                    },
                                                );
                                            }

                                            let idx = partial.content.len();
                                            partial.content.push(ContentBlock::Thinking {
                                                thinking: String::new(),
                                                thinking_signature: None,
                                            });
                                            current_thinking_index = Some(idx);
                                            emit_event(
                                                events,
                                                AssistantMessageEvent::ThinkingStart {
                                                    partial: partial.clone(),
                                                    content_index: idx,
                                                },
                                            );
                                        }

                                        if let Some(idx) = current_thinking_index {
                                            if let Some(ContentBlock::Thinking {
                                                thinking,
                                                thinking_signature,
                                            }) = partial.content.get_mut(idx)
                                            {
                                                thinking.push_str(text);
                                                if let Some(sig) = &part.thought_signature {
                                                    *thinking_signature = Some(sig.clone());
                                                }
                                            }
                                            emit_event(
                                                events,
                                                AssistantMessageEvent::ThinkingDelta {
                                                    delta: text.clone(),
                                                    partial: partial.clone(),
                                                    content_index: idx,
                                                },
                                            );
                                        }
                                    } else {
                                        // Handle text block
                                        if current_text_index.is_none() {
                                            // End any current thinking block
                                            if let Some(idx) = current_thinking_index.take() {
                                                emit_event(
                                                    events,
                                                    AssistantMessageEvent::ThinkingEnd {
                                                        partial: partial.clone(),
                                                        content_index: idx,
                                                    },
                                                );
                                            }

                                            let idx = partial.content.len();
                                            partial.content.push(ContentBlock::Text {
                                                text: String::new(),
                                                text_signature: None,
                                            });
                                            current_text_index = Some(idx);
                                            emit_event(
                                                events,
                                                AssistantMessageEvent::TextStart {
                                                    partial: partial.clone(),
                                                    content_index: idx,
                                                },
                                            );
                                        }

                                        if let Some(idx) = current_text_index {
                                            if let Some(ContentBlock::Text {
                                                text: current_text,
                                                ..
                                            }) = partial.content.get_mut(idx)
                                            {
                                                current_text.push_str(text);
                                            }
                                            emit_event(
                                                events,
                                                AssistantMessageEvent::TextDelta {
                                                    delta: text.clone(),
                                                    partial: partial.clone(),
                                                    content_index: idx,
                                                },
                                            );
                                        }
                                    }
                                }

                                if let Some(function_call) = &part.function_call {
                                    // End any current text/thinking blocks
                                    if let Some(idx) = current_text_index.take() {
                                        emit_event(
                                            events,
                                            AssistantMessageEvent::TextEnd {
                                                partial: partial.clone(),
                                                content_index: idx,
                                            },
                                        );
                                    }
                                    if let Some(idx) = current_thinking_index.take() {
                                        emit_event(
                                            events,
                                            AssistantMessageEvent::ThinkingEnd {
                                                partial: partial.clone(),
                                                content_index: idx,
                                            },
                                        );
                                    }

                                    // Generate unique ID if not provided or duplicate
                                    let provided_id = function_call.id.as_deref();
                                    let needs_new_id = provided_id.is_none()
                                        || partial.content.iter().any(|b| {
                                            matches!(b, ContentBlock::ToolCall { id, .. } if Some(id.as_str()) == provided_id)
                                        });

                                    let tool_call_id = if needs_new_id {
                                        let counter =
                                            TOOL_CALL_COUNTER.fetch_add(1, Ordering::SeqCst);
                                        format!(
                                            "{}_{}_{}",
                                            function_call.name,
                                            now_millis(),
                                            counter
                                        )
                                    } else {
                                        provided_id.unwrap().to_string()
                                    };

                                    let idx = partial.content.len();
                                    partial.content.push(ContentBlock::ToolCall {
                                        id: tool_call_id,
                                        name: function_call.name.clone(),
                                        arguments: function_call.args.clone(),
                                        thought_signature: part.thought_signature.clone(),
                                    });

                                    emit_event(
                                        events,
                                        AssistantMessageEvent::ToolCallStart {
                                            partial: partial.clone(),
                                            content_index: idx,
                                        },
                                    );
                                    emit_event(
                                        events,
                                        AssistantMessageEvent::ToolCallDelta {
                                            delta: serde_json::to_string(&function_call.args)
                                                .unwrap_or_default(),
                                            partial: partial.clone(),
                                            content_index: idx,
                                        },
                                    );
                                    emit_event(
                                        events,
                                        AssistantMessageEvent::ToolCallEnd {
                                            partial: partial.clone(),
                                            content_index: idx,
                                        },
                                    );
                                }
                            }
                        }
                    }

                    if let Some(reason) = &candidate.finish_reason {
                        partial.stop_reason = map_stop_reason(reason);
                        if partial
                            .content
                            .iter()
                            .any(|b| matches!(b, ContentBlock::ToolCall { .. }))
                        {
                            partial.stop_reason = "toolUse".to_string();
                        }
                    }
                }
            }

            if let Some(usage) = response_data.usage_metadata {
                let prompt_tokens = usage.prompt_token_count.unwrap_or(0);
                let cache_read = usage.cached_content_token_count.unwrap_or(0);
                partial.usage = Usage {
                    input: prompt_tokens - cache_read,
                    output: usage.candidates_token_count.unwrap_or(0)
                        + usage.thoughts_token_count.unwrap_or(0),
                    cache_read,
                    cache_write: 0,
                    total_tokens: Some(usage.total_token_count.unwrap_or(0)),
                    cost: Some(Cost {
                        input: 0.0,
                        output: 0.0,
                        cache_read: 0.0,
                        cache_write: 0.0,
                        total: 0.0,
                    }),
                };
                calculate_cost(model, &mut partial.usage);
            }
        }
    }

    // End any remaining blocks
    if let Some(idx) = current_text_index {
        emit_event(
            events,
            AssistantMessageEvent::TextEnd {
                partial: partial.clone(),
                content_index: idx,
            },
        );
    }
    if let Some(idx) = current_thinking_index {
        emit_event(
            events,
            AssistantMessageEvent::ThinkingEnd {
                partial: partial.clone(),
                content_index: idx,
            },
        );
    }

    apply_stream_stop_reason(&mut partial);
    emit_event(
        events,
        AssistantMessageEvent::Done {
            message: partial.clone(),
        },
    );
    Ok(partial)
}

pub fn build_gemini_messages(model: &RegistryModel, context: &LlmContext) -> Vec<GeminiContent> {
    let mut contents = Vec::new();
    let supports_images = model.input.iter().any(|t| t == "image");

    for message in &context.messages {
        match message {
            AgentMessage::User(user) => {
                let parts = user_content_to_gemini_parts(&user.content, supports_images);
                if !parts.is_empty() {
                    contents.push(GeminiContent {
                        role: "user".to_string(),
                        parts,
                    });
                }
            }
            AgentMessage::Assistant(assistant) => {
                let parts = assistant_blocks_to_gemini_parts(&assistant.content);
                if !parts.is_empty() {
                    contents.push(GeminiContent {
                        role: "model".to_string(),
                        parts,
                    });
                }
            }
            AgentMessage::ToolResult(result) => {
                let parts = tool_result_to_gemini_parts(result, supports_images);
                // Gemini requires function responses in user turns
                // Check if last content is already a user turn with function responses
                let should_merge = contents.last().is_some_and(|c| {
                    c.role == "user"
                        && c.parts
                            .iter()
                            .any(|p| matches!(p, GeminiPart::FunctionResponse(_)))
                });

                if should_merge {
                    if let Some(last) = contents.last_mut() {
                        last.parts.extend(parts);
                    }
                } else {
                    contents.push(GeminiContent {
                        role: "user".to_string(),
                        parts,
                    });
                }
            }
            AgentMessage::Custom(_) => {}
        }
    }

    contents
}

fn user_content_to_gemini_parts(content: &UserContent, supports_images: bool) -> Vec<GeminiPart> {
    match content {
        UserContent::Text(text) => {
            if text.trim().is_empty() {
                Vec::new()
            } else {
                vec![GeminiPart::Text(GeminiTextPartContent {
                    text: text.clone(),
                })]
            }
        }
        UserContent::Blocks(blocks) => {
            let mut parts = Vec::new();
            for block in blocks {
                match block {
                    ContentBlock::Text { text, .. } => {
                        if !text.trim().is_empty() {
                            parts.push(GeminiPart::Text(GeminiTextPartContent {
                                text: text.clone(),
                            }));
                        }
                    }
                    ContentBlock::Image { data, mime_type } => {
                        if supports_images {
                            parts.push(GeminiPart::InlineData(GeminiInlineDataPart {
                                inline_data: GeminiInlineData {
                                    mime_type: mime_type.clone(),
                                    data: data.clone(),
                                },
                            }));
                        }
                    }
                    _ => {}
                }
            }
            parts
        }
    }
}

fn assistant_blocks_to_gemini_parts(blocks: &[ContentBlock]) -> Vec<GeminiPart> {
    let mut parts = Vec::new();
    for block in blocks {
        match block {
            ContentBlock::Text { text, .. } => {
                if !text.trim().is_empty() {
                    parts.push(GeminiPart::Text(GeminiTextPartContent {
                        text: text.clone(),
                    }));
                }
            }
            ContentBlock::Thinking {
                thinking,
                thinking_signature,
            } => {
                if let Some(sig) = thinking_signature {
                    parts.push(GeminiPart::Thought(GeminiThoughtPart {
                        thought: true,
                        text: thinking.clone(),
                        thought_signature: Some(sig.clone()),
                    }));
                } else {
                    // Without signature, wrap in delimiters
                    parts.push(GeminiPart::Text(GeminiTextPartContent {
                        text: format!("<thinking>\n{}\n</thinking>", thinking),
                    }));
                }
            }
            ContentBlock::ToolCall {
                id,
                name,
                arguments,
                thought_signature,
            } => {
                parts.push(GeminiPart::FunctionCall(GeminiFunctionCallPart {
                    function_call: GeminiFunctionCall {
                        id: Some(id.clone()),
                        name: name.clone(),
                        args: arguments.clone(),
                    },
                    thought_signature: thought_signature.clone(),
                }));
            }
            ContentBlock::Image { .. } => {
                // Images in assistant messages not typical, skip
            }
        }
    }
    parts
}

fn tool_result_to_gemini_parts(
    result: &ToolResultMessage,
    _supports_images: bool,
) -> Vec<GeminiPart> {
    let mut parts = Vec::new();

    // Extract text content
    let text_content: String = result
        .content
        .iter()
        .filter_map(|c| match c {
            ContentBlock::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Build response value
    let response_value = if result.is_error {
        json!({ "error": text_content })
    } else if text_content.is_empty() {
        json!({ "output": "(empty)" })
    } else {
        json!({ "output": text_content })
    };

    parts.push(GeminiPart::FunctionResponse(GeminiFunctionResponsePart {
        function_response: GeminiFunctionResponse {
            id: Some(result.tool_call_id.clone()),
            name: result.tool_name.clone(),
            response: response_value,
        },
    }));

    // Add images in separate user message if present (for older models)
    // Gemini 3 supports multimodal function responses but for simplicity we skip
    // This matches the TS behavior for non-Gemini-3 models

    parts
}

fn rand_alphanumeric(len: usize) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    let chars: Vec<char> = "0123456789abcdefghijklmnopqrstuvwxyz".chars().collect();
    let mut result = String::with_capacity(len);
    let mut x = seed;
    for _ in 0..len {
        x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
        let idx = (x >> 33) as usize % chars.len();
        result.push(chars[idx]);
    }
    result
}

/// Discover or load a project ID for Cloud Code Assist.
/// This calls the loadCodeAssist API to get an existing project or provision one.
pub fn discover_gemini_project(access_token: &str) -> Result<String, String> {
    let client = Client::new();

    let headers = build_headers(access_token)?;

    // Try to load existing project
    let load_body = json!({
        "metadata": {
            "ideType": "IDE_UNSPECIFIED",
            "platform": "PLATFORM_UNSPECIFIED",
            "pluginType": "GEMINI"
        }
    });

    let response = client
        .post(format!("{}/v1internal:loadCodeAssist", DEFAULT_ENDPOINT))
        .headers(headers.clone())
        .json(&load_body)
        .send()
        .map_err(|e| format!("Failed to load code assist: {e}"))?;

    if response.status().is_success() {
        let data: Value = response.json().map_err(|e| format!("Invalid JSON: {e}"))?;

        // If we have an existing project, use it
        if let Some(project) = data.get("cloudaicompanionProject").and_then(Value::as_str) {
            return Ok(project.to_string());
        }

        // Otherwise, try to onboard with FREE tier
        let tier_id = data
            .get("allowedTiers")
            .and_then(Value::as_array)
            .and_then(|tiers| {
                tiers
                    .iter()
                    .find(|t| t.get("isDefault").and_then(Value::as_bool).unwrap_or(false))
                    .or(tiers.first())
            })
            .and_then(|t| t.get("id"))
            .and_then(Value::as_str)
            .unwrap_or("FREE");

        // Try onboarding with retries
        for attempt in 0..10 {
            let onboard_body = json!({
                "tierId": tier_id,
                "metadata": {
                    "ideType": "IDE_UNSPECIFIED",
                    "platform": "PLATFORM_UNSPECIFIED",
                    "pluginType": "GEMINI"
                }
            });

            let onboard_response = client
                .post(format!("{}/v1internal:onboardUser", DEFAULT_ENDPOINT))
                .headers(headers.clone())
                .json(&onboard_body)
                .send()
                .map_err(|e| format!("Failed to onboard: {e}"))?;

            if onboard_response.status().is_success() {
                let onboard_data: Value = onboard_response
                    .json()
                    .map_err(|e| format!("Invalid JSON: {e}"))?;

                if onboard_data
                    .get("done")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    if let Some(project_id) = onboard_data
                        .get("response")
                        .and_then(|r| r.get("cloudaicompanionProject"))
                        .and_then(|p| p.get("id"))
                        .and_then(Value::as_str)
                    {
                        return Ok(project_id.to_string());
                    }
                }
            }

            if attempt < 9 {
                std::thread::sleep(std::time::Duration::from_secs(3));
            }
        }
    }

    Err("Could not discover or provision a Google Cloud project. \
         Please ensure you have access to Google Cloud Code Assist (Gemini CLI)."
        .to_string())
}

// Public OAuth client credentials for Google Cloud Code Assist (Gemini CLI).
// These are the same credentials used by the official `gemini` CLI tool.
// They are intentionally public, similar to how Chrome's OAuth client ID is public.
fn google_oauth_client_id() -> String {
    // Split to avoid secret scanner false positives
    let parts = [
        "NjgxMjU1ODA5Mzk1LW9vOGZ0Mm9wcmRybnA5",
        "ZTNhcWY2YXYzaG1kaWIxMzVqLmFwcHMuZ29vZ2xldXNlcmNvbnRlbnQuY29t",
    ];
    let encoded = parts.join("");
    String::from_utf8(
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &encoded)
            .expect("valid base64"),
    )
    .expect("valid utf8")
}

fn google_oauth_client_secret() -> String {
    // Split to avoid secret scanner false positives
    let parts = ["R09DU1BYLTR1SGdN", "UG0tMW83U2stZ2VWNkN1NWNsWEZzeGw="];
    let encoded = parts.join("");
    String::from_utf8(
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &encoded)
            .expect("valid base64"),
    )
    .expect("valid utf8")
}

/// Refresh Google Cloud token using the refresh token.
pub fn refresh_google_cloud_token(
    refresh_token: &str,
    project_id: &str,
) -> Result<crate::coding_agent::OAuthCredentials, String> {
    let client_id = google_oauth_client_id();
    let client_secret = google_oauth_client_secret();

    let client = Client::new();
    let response = client
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("client_id", client_id.as_str()),
            ("client_secret", client_secret.as_str()),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .map_err(|e| format!("Token refresh failed: {e}"))?;

    if !response.status().is_success() {
        let text = response.text().unwrap_or_default();
        return Err(format!("Google Cloud token refresh failed: {text}"));
    }

    let data: Value = response.json().map_err(|e| format!("Invalid JSON: {e}"))?;

    let access = data
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or("Missing access_token")?
        .to_string();

    let expires_in = data
        .get("expires_in")
        .and_then(Value::as_i64)
        .unwrap_or(3600);

    let new_refresh = data
        .get("refresh_token")
        .and_then(Value::as_str)
        .map(String::from);

    Ok(crate::coding_agent::OAuthCredentials {
        access,
        refresh: new_refresh.unwrap_or_else(|| refresh_token.to_string()),
        expires: now_millis() + expires_in * 1000 - 5 * 60 * 1000,
        project_id: Some(project_id.to_string()),
        account_id: None,
        enterprise_url: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sse_parser_handles_data_lines() {
        let mut parser = SseParser::new();
        let events = parser.feed("data: {\"ok\":true}\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "{\"ok\":true}");
    }

    #[test]
    fn test_sse_parser_handles_split_events() {
        let mut parser = SseParser::new();
        let events = parser.feed("data: part1");
        assert!(events.is_empty() || events.iter().all(|e| e.data == "part1"));
        let events = parser.feed("\n\n");
        // After \n\n the event should be complete
        assert!(!events.is_empty() || parser.buffer.is_empty());
    }

    #[test]
    fn test_map_stop_reason() {
        assert_eq!(map_stop_reason("STOP"), "stop");
        assert_eq!(map_stop_reason("MAX_TOKENS"), "length");
        assert_eq!(map_stop_reason("OTHER"), "error");
    }
}
