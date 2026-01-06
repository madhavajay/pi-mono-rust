//! Streaming implementation for OpenAI Codex API
//!
//! Handles streaming SSE responses from the Codex API, translating them into
//! AssistantMessageEvent stream for consumption by the agent.

use super::constants::{header_values, headers, url_paths, CODEX_BASE_URL, JWT_CLAIM_PATH};
use super::prompts::get_codex_instructions;
use super::request_transformer::{
    normalize_model, transform_request_body, CodexRequestBody, CodexRequestOptions,
    ReasoningEffort, ReasoningSummary, TextVerbosity,
};
use super::response_handler::{parse_codex_error, parse_sse_chunk};

use crate::agent::{LlmContext, StreamEvents};
use crate::ai::AssistantMessageEvent;
use crate::coding_agent::Model as RegistryModel;
use crate::core::messages::{AssistantMessage, ContentBlock, Cost, Usage};

use base64::Engine;
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::io::Read;

/// Options for the OpenAI Codex streaming call
#[derive(Debug, Clone, Default)]
pub struct CodexStreamOptions {
    /// Reasoning effort level
    pub reasoning_effort: Option<ReasoningEffort>,
    /// Reasoning summary level
    pub reasoning_summary: Option<ReasoningSummary>,
    /// Text verbosity level
    pub text_verbosity: Option<TextVerbosity>,
    /// Additional fields to include in the response
    pub include: Option<Vec<String>>,
    /// Whether to use codex mode (adds bridge message) or tool remap mode
    pub codex_mode: Option<bool>,
    /// Extra headers to send with the request
    pub extra_headers: Option<HashMap<String, String>>,
}

/// Tool definition for the Codex API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexTool {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub name: String,
    pub description: String,
    pub parameters: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strict: Option<Value>,
}

/// Decode a JWT and extract the payload
fn decode_jwt(token: &str) -> Option<Value> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }

    let payload = parts.get(1)?;
    // Try different base64 encodings - JWT tokens can be encoded with or without padding
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(payload)
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(payload))
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(payload))
        .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload))
        .ok()?;
    let decoded_str = String::from_utf8(decoded).ok()?;
    serde_json::from_str(&decoded_str).ok()
}

/// Extract the ChatGPT account ID from a JWT token
fn get_account_id(access_token: &str) -> Result<String, String> {
    let payload = decode_jwt(access_token).ok_or("Failed to decode JWT token")?;

    let auth = payload
        .get(JWT_CLAIM_PATH)
        .ok_or_else(|| format!("JWT token missing {} claim", JWT_CLAIM_PATH))?;

    let account_id = auth
        .get("chatgpt_account_id")
        .and_then(|v| v.as_str())
        .ok_or("Failed to extract chatgpt_account_id from token")?;

    Ok(account_id.to_string())
}

/// Rewrite the URL to use the Codex-specific endpoint
fn rewrite_url_for_codex(url: &str) -> String {
    url.replace(url_paths::RESPONSES, url_paths::CODEX_RESPONSES)
}

/// Build headers for the Codex API request
fn build_codex_headers(
    init_headers: Option<&HashMap<String, String>>,
    account_id: &str,
    access_token: &str,
    prompt_cache_key: Option<&str>,
) -> Result<HeaderMap, String> {
    let mut header_map = HeaderMap::new();

    // Copy initial headers, excluding x-api-key
    if let Some(extra) = init_headers {
        for (key, value) in extra {
            let lower = key.to_lowercase();
            if lower == "x-api-key" {
                continue;
            }
            let header_name = HeaderName::from_bytes(key.as_bytes())
                .map_err(|e| format!("Invalid header name '{}': {}", key, e))?;
            let header_value = HeaderValue::from_str(value)
                .map_err(|e| format!("Invalid header value for '{}': {}", key, e))?;
            header_map.insert(header_name, header_value);
        }
    }

    // Set authorization
    let auth_value = format!("Bearer {}", access_token);
    header_map.insert(
        "authorization",
        HeaderValue::from_str(&auth_value)
            .map_err(|e| format!("Invalid authorization header: {}", e))?,
    );

    // Set Codex-specific headers
    header_map.insert(
        HeaderName::from_static(headers::ACCOUNT_ID),
        HeaderValue::from_str(account_id)
            .map_err(|e| format!("Invalid account ID header: {}", e))?,
    );

    header_map.insert(
        HeaderName::from_static(headers::BETA),
        HeaderValue::from_static(header_values::BETA_RESPONSES),
    );

    header_map.insert(
        HeaderName::from_static(headers::ORIGINATOR),
        HeaderValue::from_static(header_values::ORIGINATOR_CODEX),
    );

    // Set session/conversation IDs if prompt cache key is provided
    if let Some(cache_key) = prompt_cache_key {
        header_map.insert(
            HeaderName::from_static(headers::CONVERSATION_ID),
            HeaderValue::from_str(cache_key)
                .map_err(|e| format!("Invalid conversation ID header: {}", e))?,
        );
        header_map.insert(
            HeaderName::from_static(headers::SESSION_ID),
            HeaderValue::from_str(cache_key)
                .map_err(|e| format!("Invalid session ID header: {}", e))?,
        );
    }

    // Set content type and accept headers
    header_map.insert("accept", HeaderValue::from_static("text/event-stream"));
    header_map.insert("content-type", HeaderValue::from_static("application/json"));

    Ok(header_map)
}

fn empty_object() -> Value {
    Value::Object(Map::new())
}

fn parse_partial_json(value: &str) -> Value {
    serde_json::from_str(value).unwrap_or_else(|_| empty_object())
}

fn now_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
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

fn emit_event(events: &mut StreamEvents, event: AssistantMessageEvent) {
    events.emit(event);
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

/// Map OpenAI Codex response status to internal stop reason
fn map_codex_stop_reason(status: Option<&str>) -> String {
    match status {
        Some("completed") | None => "stop",
        Some("incomplete") => "length",
        Some("failed") | Some("cancelled") => "error",
        Some("in_progress") | Some("queued") => "stop",
        _ => "stop",
    }
    .to_string()
}

/// Convert the LLM context to Codex API input items
pub fn codex_context_to_input_items(model: &RegistryModel, context: &LlmContext) -> Vec<Value> {
    use crate::agent::AgentMessage;
    use crate::core::messages::UserContent;

    let mut items = Vec::new();
    let supports_images = model.input.iter().any(|entry| entry == "image");

    for message in &context.messages {
        match message {
            AgentMessage::User(user) => {
                let parts = match &user.content {
                    UserContent::Text(text) => {
                        if text.trim().is_empty() {
                            continue;
                        }
                        vec![json!({"type": "input_text", "text": text})]
                    }
                    UserContent::Blocks(blocks) => {
                        let mut parts = Vec::new();
                        for block in blocks {
                            match block {
                                ContentBlock::Text { text, .. } => {
                                    if !text.trim().is_empty() {
                                        parts.push(json!({"type": "input_text", "text": text}));
                                    }
                                }
                                ContentBlock::Image { data, mime_type } => {
                                    if supports_images {
                                        let image_url =
                                            format!("data:{};base64,{}", mime_type, data);
                                        parts.push(json!({
                                            "type": "input_image",
                                            "image_url": image_url,
                                            "detail": "auto"
                                        }));
                                    }
                                }
                                _ => {}
                            }
                        }
                        parts
                    }
                };

                if !parts.is_empty() {
                    items.push(json!({
                        "type": "message",
                        "role": "user",
                        "content": parts
                    }));
                }
            }
            AgentMessage::Assistant(assistant) => {
                let mut current_content: Vec<Value> = Vec::new();

                for block in &assistant.content {
                    match block {
                        ContentBlock::Text {
                            text,
                            text_signature,
                        } => {
                            // Flush content as message before adding new one
                            if !current_content.is_empty() {
                                items.push(json!({
                                    "type": "message",
                                    "role": "assistant",
                                    "content": current_content,
                                    "status": "completed"
                                }));
                                current_content = Vec::new();
                            }

                            // Add text as an output message
                            let msg_id = text_signature
                                .clone()
                                .unwrap_or_else(|| format!("msg_{}", items.len()));
                            items.push(json!({
                                "type": "message",
                                "role": "assistant",
                                "content": [{"type": "output_text", "text": text, "annotations": []}],
                                "status": "completed",
                                "id": msg_id
                            }));
                        }
                        ContentBlock::Thinking {
                            thinking,
                            thinking_signature,
                        } => {
                            // Skip thinking blocks with errors
                            if assistant.stop_reason == "error" {
                                continue;
                            }

                            // If we have a signature, try to restore the original reasoning item
                            if let Some(sig) = thinking_signature {
                                if let Ok(reasoning_item) = serde_json::from_str::<Value>(sig) {
                                    items.push(reasoning_item);
                                    continue;
                                }
                            }

                            // Fallback: create a reasoning summary
                            items.push(json!({
                                "type": "reasoning",
                                "id": format!("rs_{}", items.len()),
                                "summary": [{"type": "summary_text", "text": thinking}]
                            }));
                        }
                        ContentBlock::ToolCall {
                            id,
                            name,
                            arguments,
                            ..
                        } => {
                            // Skip tool calls with errors
                            if assistant.stop_reason == "error" {
                                continue;
                            }

                            // Parse compound ID format: call_id|item_id
                            let (call_id, item_id) = match id.split_once('|') {
                                Some((c, i)) => (c.to_string(), i.to_string()),
                                None => (id.clone(), id.clone()),
                            };

                            items.push(json!({
                                "type": "function_call",
                                "id": item_id,
                                "call_id": call_id,
                                "name": name,
                                "arguments": serde_json::to_string(arguments).unwrap_or_else(|_| "{}".to_string())
                            }));
                        }
                        ContentBlock::Image { .. } => {
                            // Images in assistant messages are not supported
                        }
                    }
                }

                // Flush remaining content
                if !current_content.is_empty() {
                    items.push(json!({
                        "type": "message",
                        "role": "assistant",
                        "content": current_content,
                        "status": "completed"
                    }));
                }
            }
            AgentMessage::ToolResult(result) => {
                // Parse compound ID format: call_id|item_id
                let call_id = result
                    .tool_call_id
                    .split('|')
                    .next()
                    .unwrap_or(&result.tool_call_id);

                let text_result: String = result
                    .content
                    .iter()
                    .filter_map(|c| match c {
                        ContentBlock::Text { text, .. } => Some(text.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");

                let has_text = !text_result.is_empty();
                let output = if has_text {
                    text_result
                } else {
                    "(see attached image)".to_string()
                };

                items.push(json!({
                    "type": "function_call_output",
                    "call_id": call_id,
                    "output": output
                }));

                // If there are images, add them as a user message
                let has_images = result
                    .content
                    .iter()
                    .any(|c| matches!(c, ContentBlock::Image { .. }));
                if has_images && supports_images {
                    let mut content_parts = vec![
                        json!({"type": "input_text", "text": "Attached image(s) from tool result:"}),
                    ];

                    for block in &result.content {
                        if let ContentBlock::Image { data, mime_type } = block {
                            let image_url = format!("data:{};base64,{}", mime_type, data);
                            content_parts.push(json!({
                                "type": "input_image",
                                "image_url": image_url,
                                "detail": "auto"
                            }));
                        }
                    }

                    items.push(json!({
                        "type": "message",
                        "role": "user",
                        "content": content_parts
                    }));
                }
            }
            AgentMessage::Custom(_) => {}
        }
    }

    items
}

/// Stream a response from the OpenAI Codex API
pub fn stream_openai_codex_responses(
    model: &RegistryModel,
    context: &LlmContext,
    api_key: &str,
    tools: &[CodexTool],
    options: CodexStreamOptions,
    events: &mut StreamEvents,
) -> Result<AssistantMessage, String> {
    // Extract account ID from JWT token
    let account_id = get_account_id(api_key)?;

    // Build the base URL
    let base_url = if model.base_url.is_empty() {
        CODEX_BASE_URL
    } else {
        model.base_url.as_str()
    };
    let base_with_slash = if base_url.ends_with('/') {
        base_url.to_string()
    } else {
        format!("{}/", base_url)
    };

    // Build the endpoint URL
    let url = format!("{}responses", base_with_slash);
    let url = rewrite_url_for_codex(&url);

    // Normalize the model name
    let normalized_model = normalize_model(Some(&model.id));

    // Get the Codex instructions
    let codex_instructions = get_codex_instructions(&normalized_model)
        .unwrap_or_else(|_| "You are a helpful coding assistant.".to_string());

    // Convert context to input items
    let input_items = codex_context_to_input_items(model, context);

    // Build the request body
    let mut body = CodexRequestBody {
        model: model.id.clone(),
        store: None,
        stream: None,
        instructions: None,
        input: Some(input_items),
        tools: if tools.is_empty() {
            None
        } else {
            Some(
                tools
                    .iter()
                    .map(|t| serde_json::to_value(t).unwrap())
                    .collect(),
            )
        },
        temperature: None,
        reasoning: None,
        text: None,
        include: None,
        prompt_cache_key: None,
        max_output_tokens: None,
        max_completion_tokens: None,
    };

    // Transform the request
    let codex_options = CodexRequestOptions {
        reasoning_effort: options.reasoning_effort,
        reasoning_summary: options.reasoning_summary,
        text_verbosity: options.text_verbosity,
        include: options.include,
    };

    let codex_mode = options.codex_mode.unwrap_or(true);
    let system_prompt = if context.system_prompt.trim().is_empty() {
        None
    } else {
        Some(context.system_prompt.as_str())
    };

    transform_request_body(
        &mut body,
        &codex_instructions,
        &codex_options,
        codex_mode,
        system_prompt,
    );

    // Build headers
    let header_map = build_codex_headers(
        options.extra_headers.as_ref(),
        &account_id,
        api_key,
        body.prompt_cache_key.as_deref(),
    )?;

    // Make the request
    let client = Client::new();
    let mut response = client
        .post(&url)
        .headers(header_map)
        .json(&body)
        .send()
        .map_err(|e| format!("Request failed: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        let response_headers = response.headers().clone();
        let text = response.text().unwrap_or_default();
        let error_info = parse_codex_error(status.as_u16(), &response_headers, &text);
        return Err(error_info.friendly_message.unwrap_or(error_info.message));
    }

    // Initialize the partial message
    let mut partial = stream_partial_message(model);
    let mut tool_buffers: Vec<Option<String>> = Vec::new();
    let mut current_index: Option<usize> = None;
    let mut stop_reason: Option<String> = None;

    emit_event(
        events,
        AssistantMessageEvent::Start {
            partial: partial.clone(),
        },
    );

    // Parse the SSE stream
    let mut buffer = String::new();
    let mut buf = [0u8; 8192];

    loop {
        let read = response
            .read(&mut buf)
            .map_err(|e| format!("Stream read failed: {}", e))?;

        if read == 0 {
            break;
        }

        let chunk = String::from_utf8_lossy(&buf[..read]);
        buffer.push_str(&chunk);

        // Handle CRLF normalization
        if buffer.contains('\r') {
            buffer = buffer.replace("\r\n", "\n");
        }

        // Process complete events (delimited by \n\n)
        while let Some(boundary) = buffer.find("\n\n") {
            let raw = buffer[..boundary].to_string();
            buffer = buffer[boundary + 2..].to_string();

            let Some(event) = parse_sse_chunk(&raw) else {
                continue;
            };

            let event_type = event.event_type.as_str();
            match event_type {
                "response.output_item.added" => {
                    let item = event.data.get("item").unwrap_or(&Value::Null);
                    let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");
                    let index = partial.content.len();

                    let new_block = match item_type {
                        "message" => ContentBlock::Text {
                            text: String::new(),
                            text_signature: None,
                        },
                        "reasoning" => ContentBlock::Thinking {
                            thinking: String::new(),
                            thinking_signature: None,
                        },
                        "function_call" => ContentBlock::ToolCall {
                            id: format!(
                                "{}|{}",
                                item.get("call_id").and_then(|c| c.as_str()).unwrap_or(""),
                                item.get("id").and_then(|i| i.as_str()).unwrap_or("")
                            ),
                            name: item
                                .get("name")
                                .and_then(|n| n.as_str())
                                .unwrap_or("")
                                .to_string(),
                            arguments: empty_object(),
                            thought_signature: None,
                        },
                        _ => ContentBlock::Text {
                            text: String::new(),
                            text_signature: None,
                        },
                    };

                    partial.content.push(new_block);
                    tool_buffers.push(if item_type == "function_call" {
                        Some(String::new())
                    } else {
                        None
                    });
                    current_index = Some(index);

                    match item_type {
                        "message" => emit_event(
                            events,
                            AssistantMessageEvent::TextStart {
                                partial: partial.clone(),
                                content_index: index,
                            },
                        ),
                        "reasoning" => emit_event(
                            events,
                            AssistantMessageEvent::ThinkingStart {
                                partial: partial.clone(),
                                content_index: index,
                            },
                        ),
                        "function_call" => emit_event(
                            events,
                            AssistantMessageEvent::ToolCallStart {
                                partial: partial.clone(),
                                content_index: index,
                            },
                        ),
                        _ => {}
                    }
                }
                "response.output_text.delta" | "response.refusal.delta" => {
                    let delta = event
                        .data
                        .get("delta")
                        .and_then(|d| d.as_str())
                        .unwrap_or("");
                    if let Some(index) = current_index {
                        if let Some(ContentBlock::Text { text, .. }) =
                            partial.content.get_mut(index)
                        {
                            text.push_str(delta);
                        }
                        emit_event(
                            events,
                            AssistantMessageEvent::TextDelta {
                                delta: delta.to_string(),
                                partial: partial.clone(),
                                content_index: index,
                            },
                        );
                    }
                }
                "response.reasoning_summary_text.delta" => {
                    let delta = event
                        .data
                        .get("delta")
                        .and_then(|d| d.as_str())
                        .unwrap_or("");
                    if let Some(index) = current_index {
                        if let Some(ContentBlock::Thinking { thinking, .. }) =
                            partial.content.get_mut(index)
                        {
                            thinking.push_str(delta);
                        }
                        emit_event(
                            events,
                            AssistantMessageEvent::ThinkingDelta {
                                delta: delta.to_string(),
                                partial: partial.clone(),
                                content_index: index,
                            },
                        );
                    }
                }
                "response.reasoning_summary_part.done" => {
                    // Add newlines between summary parts
                    if let Some(index) = current_index {
                        if let Some(ContentBlock::Thinking { thinking, .. }) =
                            partial.content.get_mut(index)
                        {
                            thinking.push_str("\n\n");
                        }
                        emit_event(
                            events,
                            AssistantMessageEvent::ThinkingDelta {
                                delta: "\n\n".to_string(),
                                partial: partial.clone(),
                                content_index: index,
                            },
                        );
                    }
                }
                "response.function_call_arguments.delta" => {
                    let delta = event
                        .data
                        .get("delta")
                        .and_then(|d| d.as_str())
                        .unwrap_or("");
                    if let Some(index) = current_index {
                        if let Some(buffer) = tool_buffers.get_mut(index).and_then(Option::as_mut) {
                            buffer.push_str(delta);
                            let parsed = parse_partial_json(buffer);
                            if let Some(ContentBlock::ToolCall { arguments, .. }) =
                                partial.content.get_mut(index)
                            {
                                *arguments = parsed;
                            }
                        }
                        emit_event(
                            events,
                            AssistantMessageEvent::ToolCallDelta {
                                delta: delta.to_string(),
                                partial: partial.clone(),
                                content_index: index,
                            },
                        );
                    }
                }
                "response.output_item.done" => {
                    let item = event.data.get("item").unwrap_or(&Value::Null);
                    let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");

                    if let Some(index) = current_index {
                        match item_type {
                            "message" => {
                                // Extract final text from the item
                                if let Some(content) =
                                    item.get("content").and_then(|c| c.as_array())
                                {
                                    let final_text: String = content
                                        .iter()
                                        .filter_map(|c| {
                                            let ct = c.get("type").and_then(|t| t.as_str())?;
                                            if ct == "output_text" {
                                                c.get("text")
                                                    .and_then(|t| t.as_str())
                                                    .map(|s| s.to_string())
                                            } else if ct == "refusal" {
                                                c.get("refusal")
                                                    .and_then(|r| r.as_str())
                                                    .map(|s| s.to_string())
                                            } else {
                                                None
                                            }
                                        })
                                        .collect::<Vec<_>>()
                                        .join("");

                                    if let Some(ContentBlock::Text {
                                        text,
                                        text_signature,
                                    }) = partial.content.get_mut(index)
                                    {
                                        *text = final_text;
                                        *text_signature = item
                                            .get("id")
                                            .and_then(|i| i.as_str())
                                            .map(|s| s.to_string());
                                    }
                                }

                                emit_event(
                                    events,
                                    AssistantMessageEvent::TextEnd {
                                        partial: partial.clone(),
                                        content_index: index,
                                    },
                                );
                            }
                            "reasoning" => {
                                // Store the entire reasoning item as signature for round-tripping
                                if let Some(ContentBlock::Thinking {
                                    thinking,
                                    thinking_signature,
                                }) = partial.content.get_mut(index)
                                {
                                    // Extract summary text
                                    if let Some(summary) =
                                        item.get("summary").and_then(|s| s.as_array())
                                    {
                                        let summary_text: String = summary
                                            .iter()
                                            .filter_map(|s| s.get("text").and_then(|t| t.as_str()))
                                            .collect::<Vec<_>>()
                                            .join("\n\n");
                                        *thinking = summary_text;
                                    }
                                    *thinking_signature =
                                        Some(serde_json::to_string(item).unwrap_or_default());
                                }

                                emit_event(
                                    events,
                                    AssistantMessageEvent::ThinkingEnd {
                                        partial: partial.clone(),
                                        content_index: index,
                                    },
                                );
                            }
                            "function_call" => {
                                // Parse final arguments
                                if let Some(args_str) =
                                    item.get("arguments").and_then(|a| a.as_str())
                                {
                                    if let Ok(args) = serde_json::from_str::<Value>(args_str) {
                                        if let Some(ContentBlock::ToolCall { arguments, .. }) =
                                            partial.content.get_mut(index)
                                        {
                                            *arguments = args;
                                        }
                                    }
                                }

                                emit_event(
                                    events,
                                    AssistantMessageEvent::ToolCallEnd {
                                        partial: partial.clone(),
                                        content_index: index,
                                    },
                                );
                            }
                            _ => {}
                        }
                    }
                }
                "response.completed" | "response.done" => {
                    if let Some(response_obj) = event.data.get("response") {
                        // Extract usage information
                        if let Some(usage) = response_obj.get("usage") {
                            let cached_tokens = usage
                                .get("input_tokens_details")
                                .and_then(|d| d.get("cached_tokens"))
                                .and_then(|c| c.as_i64())
                                .unwrap_or(0);
                            let input_tokens = usage
                                .get("input_tokens")
                                .and_then(|i| i.as_i64())
                                .unwrap_or(0);
                            let output_tokens = usage
                                .get("output_tokens")
                                .and_then(|o| o.as_i64())
                                .unwrap_or(0);
                            let total_tokens = usage
                                .get("total_tokens")
                                .and_then(|t| t.as_i64())
                                .unwrap_or(0);

                            partial.usage = Usage {
                                input: input_tokens - cached_tokens,
                                output: output_tokens,
                                cache_read: cached_tokens,
                                cache_write: 0,
                                total_tokens: Some(total_tokens),
                                cost: Some(Cost {
                                    input: 0.0,
                                    output: 0.0,
                                    cache_read: 0.0,
                                    cache_write: 0.0,
                                    total: 0.0,
                                }),
                            };
                        }

                        // Extract status
                        let status_str = response_obj.get("status").and_then(|s| s.as_str());
                        stop_reason = Some(map_codex_stop_reason(status_str));
                    }
                }
                "response.error" | "error" => {
                    let code = event
                        .data
                        .get("code")
                        .and_then(|c| c.as_str())
                        .unwrap_or("");
                    let message = event
                        .data
                        .get("message")
                        .and_then(|m| m.as_str())
                        .unwrap_or("Unknown error");

                    let error_msg = if code.is_empty() {
                        message.to_string()
                    } else {
                        format!("Error Code {}: {}", code, message)
                    };

                    let error_message = assistant_error_message(model, &error_msg);
                    emit_event(
                        events,
                        AssistantMessageEvent::Error {
                            message: error_message.clone(),
                        },
                    );
                    return Ok(error_message);
                }
                "response.failed" => {
                    let error_message = assistant_error_message(model, "Unknown error");
                    emit_event(
                        events,
                        AssistantMessageEvent::Error {
                            message: error_message.clone(),
                        },
                    );
                    return Ok(error_message);
                }
                _ => {
                    // Ignore other event types
                }
            }
        }
    }

    // Apply final stop reason
    if let Some(reason) = stop_reason {
        partial.stop_reason = reason;
    }
    apply_stream_stop_reason(&mut partial);

    // Check for tool calls and update stop reason if needed
    if partial
        .content
        .iter()
        .any(|b| matches!(b, ContentBlock::ToolCall { .. }))
        && partial.stop_reason == "stop"
    {
        partial.stop_reason = "toolUse".to_string();
    }

    emit_event(
        events,
        AssistantMessageEvent::Done {
            message: partial.clone(),
        },
    );

    Ok(partial)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_jwt_extracts_account_id() {
        // Create a mock JWT payload with the account ID
        let payload = serde_json::json!({
            "https://api.openai.com/auth": {
                "chatgpt_account_id": "acc_test123"
            }
        });
        let payload_str = serde_json::to_string(&payload).unwrap();
        let encoded = base64::engine::general_purpose::STANDARD.encode(&payload_str);
        let token = format!("header.{}.signature", encoded);

        let account_id = get_account_id(&token).unwrap();
        assert_eq!(account_id, "acc_test123");
    }

    #[test]
    fn test_rewrite_url_for_codex() {
        let url = "https://chatgpt.com/backend-api/responses";
        let rewritten = rewrite_url_for_codex(url);
        assert_eq!(rewritten, "https://chatgpt.com/backend-api/codex/responses");
    }

    #[test]
    fn test_map_codex_stop_reason() {
        assert_eq!(map_codex_stop_reason(Some("completed")), "stop");
        assert_eq!(map_codex_stop_reason(Some("incomplete")), "length");
        assert_eq!(map_codex_stop_reason(Some("failed")), "error");
        assert_eq!(map_codex_stop_reason(Some("cancelled")), "error");
        assert_eq!(map_codex_stop_reason(None), "stop");
    }
}
