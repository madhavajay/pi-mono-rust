pub mod openai_codex;

use crate::agent::{AgentMessage, LlmContext, StreamEvents};
use crate::ai::AssistantMessageEvent;
use crate::coding_agent::Model as RegistryModel;
use crate::core::messages::{
    AssistantMessage, ContentBlock, Cost, ToolResultMessage, Usage, UserContent,
};
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::io::Read;

#[derive(Debug, Serialize, Clone)]
pub struct AnthropicRequest {
    pub model: String,
    pub max_tokens: u32,
    pub messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<Vec<AnthropicSystemContent>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<AnthropicTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

#[derive(Debug, Serialize, Clone)]
pub struct AnthropicSystemContent {
    #[serde(rename = "type")]
    pub content_type: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<AnthropicCacheControl>,
}

#[derive(Debug, Serialize, Clone)]
pub struct AnthropicCacheControl {
    #[serde(rename = "type")]
    pub control_type: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct AnthropicMessage {
    pub role: String,
    pub content: Vec<AnthropicContentBlock>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnthropicContentBlock {
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
pub enum AnthropicToolResultContent {
    Text { text: String },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AnthropicImageSource {
    #[serde(rename = "type")]
    pub source_type: String,
    pub media_type: String,
    pub data: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AnthropicTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AnthropicResponse {
    pub content: Vec<AnthropicContentBlock>,
    #[serde(default)]
    pub stop_reason: Option<String>,
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
pub struct OpenAIRequest {
    pub model: String,
    pub input: Vec<OpenAIInputItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<OpenAITool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OpenAIInputItem {
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
pub enum OpenAIMessageContent {
    InputText {
        text: String,
    },
    InputImage {
        image_url: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    OutputText {
        text: String,
    },
}

#[derive(Debug, Deserialize)]
pub struct OpenAIResponse {
    pub output: Vec<OpenAIOutputItem>,
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OpenAIOutputItem {
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
pub enum OpenAIOutputContent {
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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OpenAITool {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Deserialize)]
struct OpenAIErrorResponse {
    error: OpenAIError,
}

#[derive(Debug, Deserialize)]
struct OpenAIError {
    message: String,
}

pub struct AnthropicCallOptions<'a> {
    pub model: &'a str,
    pub api_key: &'a str,
    pub use_oauth: bool,
    pub tools: &'a [AnthropicTool],
    pub base_url: &'a str,
    pub extra_headers: Option<&'a HashMap<String, String>>,
    pub system: Option<&'a str>,
}

pub struct OpenAICallOptions<'a> {
    pub model: &'a str,
    pub api_key: &'a str,
    pub tools: &'a [OpenAITool],
    pub base_url: &'a str,
    pub extra_headers: Option<&'a HashMap<String, String>>,
}

fn build_anthropic_headers(
    api_key: &str,
    use_oauth: bool,
    extra_headers: Option<&HashMap<String, String>>,
) -> Result<HeaderMap, String> {
    let mut headers = HeaderMap::new();
    headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
    headers.insert(
        "anthropic-dangerous-direct-browser-access",
        HeaderValue::from_static("true"),
    );
    if use_oauth {
        headers.insert(
            "anthropic-beta",
            HeaderValue::from_static(
                "oauth-2025-04-20,fine-grained-tool-streaming-2025-05-14,interleaved-thinking-2025-05-14",
            ),
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

fn build_system_content(
    system: Option<&str>,
    use_oauth: bool,
) -> Option<Vec<AnthropicSystemContent>> {
    if use_oauth {
        // For OAuth tokens, Claude Code identification MUST be the first separate element
        let claude_code_id = AnthropicSystemContent {
            content_type: "text".to_string(),
            text: "You are Claude Code, Anthropic's official CLI for Claude.".to_string(),
            cache_control: Some(AnthropicCacheControl {
                control_type: "ephemeral".to_string(),
            }),
        };

        // Strip the Claude Code identification from the main system prompt if present
        let main_prompt = system.map(|s| {
            s.trim_start_matches("You are Claude Code, Anthropic's official CLI for Claude.")
                .trim_start_matches("\n\n")
                .trim_start()
        });

        match main_prompt {
            Some(text) if !text.is_empty() => Some(vec![
                claude_code_id,
                AnthropicSystemContent {
                    content_type: "text".to_string(),
                    text: text.to_string(),
                    cache_control: Some(AnthropicCacheControl {
                        control_type: "ephemeral".to_string(),
                    }),
                },
            ]),
            _ => Some(vec![claude_code_id]),
        }
    } else {
        system.map(|text| {
            vec![AnthropicSystemContent {
                content_type: "text".to_string(),
                text: text.to_string(),
                cache_control: None,
            }]
        })
    }
}

pub fn call_anthropic(
    messages: Vec<AnthropicMessage>,
    options: AnthropicCallOptions<'_>,
) -> Result<AnthropicResponse, String> {
    let request = AnthropicRequest {
        model: options.model.to_string(),
        max_tokens: 1024,
        messages,
        system: build_system_content(options.system, options.use_oauth),
        tools: if options.tools.is_empty() {
            None
        } else {
            Some(options.tools.to_vec())
        },
        stream: None,
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

pub fn call_openai(
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

struct SseEvent {
    name: Option<String>,
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
                break;
            };
            let raw = self.buffer[..boundary].to_string();
            self.buffer = self.buffer[boundary + 2..].to_string();

            let mut name = None;
            let mut data_lines = Vec::new();
            for line in raw.lines() {
                let line = line.trim_end_matches('\r');
                if let Some(rest) = line.strip_prefix("event:") {
                    let value = rest.trim();
                    if !value.is_empty() {
                        name = Some(value.to_string());
                    }
                } else if let Some(rest) = line.strip_prefix("data:") {
                    data_lines.push(rest.trim_start().to_string());
                }
            }
            let data = data_lines.join("\n");
            if !data.is_empty() {
                events.push(SseEvent { name, data });
            }
        }
        events
    }
}

fn empty_object() -> Value {
    Value::Object(Map::new())
}

fn parse_partial_json(value: &str) -> Value {
    serde_json::from_str(value).unwrap_or_else(|_| empty_object())
}

fn map_anthropic_stop_reason(reason: &str) -> String {
    match reason {
        "end_turn" => "stop",
        "max_tokens" => "length",
        "tool_use" => "toolUse",
        "refusal" => "error",
        "pause_turn" => "stop",
        "stop_sequence" => "stop",
        _ => "stop",
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

pub fn stream_anthropic(
    model: &RegistryModel,
    messages: Vec<AnthropicMessage>,
    options: AnthropicCallOptions<'_>,
    events: &mut StreamEvents,
) -> Result<AssistantMessage, String> {
    let request = AnthropicRequest {
        model: options.model.to_string(),
        max_tokens: 1024,
        messages,
        system: build_system_content(options.system, options.use_oauth),
        tools: if options.tools.is_empty() {
            None
        } else {
            Some(options.tools.to_vec())
        },
        stream: Some(true),
    };

    let headers =
        build_anthropic_headers(options.api_key, options.use_oauth, options.extra_headers)?;
    let endpoint = format!("{}/messages", options.base_url.trim_end_matches('/'));
    let client = Client::new();
    let mut response = client
        .post(&endpoint)
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

    let mut partial = stream_partial_message(model);
    let mut tool_buffers: Vec<Option<String>> = Vec::new();
    emit_event(
        events,
        AssistantMessageEvent::Start {
            partial: partial.clone(),
        },
    );

    let mut parser = SseParser::new();
    let mut buf = [0u8; 8192];
    loop {
        let read = response
            .read(&mut buf)
            .map_err(|err| format!("Stream read failed: {err}"))?;
        if read == 0 {
            break;
        }
        let chunk = String::from_utf8_lossy(&buf[..read]);
        for event in parser.feed(&chunk) {
            let event_name = event.name.unwrap_or_default();
            if event.data == "[DONE]" {
                continue;
            }
            let value: Value = match serde_json::from_str(&event.data) {
                Ok(value) => value,
                Err(_) => continue,
            };
            match event_name.as_str() {
                "message_start" => {}
                "message_delta" => {
                    if let Some(reason) = value
                        .get("delta")
                        .and_then(|delta| delta.get("stop_reason"))
                        .and_then(Value::as_str)
                    {
                        partial.stop_reason = map_anthropic_stop_reason(reason);
                    }
                }
                "content_block_start" => {
                    let index = value
                        .get("index")
                        .and_then(Value::as_u64)
                        .unwrap_or(partial.content.len() as u64)
                        as usize;
                    let block = value.get("content_block").unwrap_or(&Value::Null);
                    let block_type = block.get("type").and_then(Value::as_str).unwrap_or("");
                    while partial.content.len() < index {
                        partial.content.push(ContentBlock::Text {
                            text: String::new(),
                            text_signature: None,
                        });
                        tool_buffers.push(None);
                    }
                    let new_block = match block_type {
                        "text" => ContentBlock::Text {
                            text: String::new(),
                            text_signature: None,
                        },
                        "thinking" => ContentBlock::Thinking {
                            thinking: String::new(),
                            thinking_signature: None,
                        },
                        "tool_use" => ContentBlock::ToolCall {
                            id: block
                                .get("id")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string(),
                            name: block
                                .get("name")
                                .and_then(Value::as_str)
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
                    if index >= partial.content.len() {
                        partial.content.push(new_block);
                        tool_buffers.push(if block_type == "tool_use" {
                            Some(String::new())
                        } else {
                            None
                        });
                    } else {
                        partial.content[index] = new_block;
                        if block_type == "tool_use" {
                            if tool_buffers.len() <= index {
                                tool_buffers.resize(index + 1, None);
                            }
                            tool_buffers[index] = Some(String::new());
                        }
                    }
                    match block_type {
                        "text" => emit_event(
                            events,
                            AssistantMessageEvent::TextStart {
                                partial: partial.clone(),
                                content_index: index,
                            },
                        ),
                        "thinking" => emit_event(
                            events,
                            AssistantMessageEvent::ThinkingStart {
                                partial: partial.clone(),
                                content_index: index,
                            },
                        ),
                        "tool_use" => emit_event(
                            events,
                            AssistantMessageEvent::ToolCallStart {
                                partial: partial.clone(),
                                content_index: index,
                            },
                        ),
                        _ => {}
                    }
                }
                "content_block_delta" => {
                    let index = value.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                    let delta = value.get("delta").unwrap_or(&Value::Null);
                    let delta_type = delta.get("type").and_then(Value::as_str).unwrap_or("");
                    match delta_type {
                        "text_delta" => {
                            let text = delta.get("text").and_then(Value::as_str).unwrap_or("");
                            if let Some(ContentBlock::Text { text: current, .. }) =
                                partial.content.get_mut(index)
                            {
                                current.push_str(text);
                            }
                            emit_event(
                                events,
                                AssistantMessageEvent::TextDelta {
                                    delta: text.to_string(),
                                    partial: partial.clone(),
                                    content_index: index,
                                },
                            );
                        }
                        "thinking_delta" => {
                            let chunk = delta.get("thinking").and_then(Value::as_str).unwrap_or("");
                            if let Some(ContentBlock::Thinking { thinking, .. }) =
                                partial.content.get_mut(index)
                            {
                                thinking.push_str(chunk);
                            }
                            emit_event(
                                events,
                                AssistantMessageEvent::ThinkingDelta {
                                    delta: chunk.to_string(),
                                    partial: partial.clone(),
                                    content_index: index,
                                },
                            );
                        }
                        "input_json_delta" => {
                            let chunk = delta
                                .get("partial_json")
                                .and_then(Value::as_str)
                                .unwrap_or("");
                            if let Some(buffer) =
                                tool_buffers.get_mut(index).and_then(Option::as_mut)
                            {
                                buffer.push_str(chunk);
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
                                    delta: chunk.to_string(),
                                    partial: partial.clone(),
                                    content_index: index,
                                },
                            );
                        }
                        _ => {}
                    }
                }
                "content_block_stop" => {
                    let index = value.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                    let block = partial.content.get(index).cloned();
                    if let Some(block) = block {
                        match block {
                            ContentBlock::Text { .. } => emit_event(
                                events,
                                AssistantMessageEvent::TextEnd {
                                    partial: partial.clone(),
                                    content_index: index,
                                },
                            ),
                            ContentBlock::Thinking { .. } => emit_event(
                                events,
                                AssistantMessageEvent::ThinkingEnd {
                                    partial: partial.clone(),
                                    content_index: index,
                                },
                            ),
                            ContentBlock::ToolCall { .. } => {
                                if let Some(buffer) =
                                    tool_buffers.get_mut(index).and_then(Option::as_mut)
                                {
                                    let parsed = parse_partial_json(buffer);
                                    if let Some(ContentBlock::ToolCall { arguments, .. }) =
                                        partial.content.get_mut(index)
                                    {
                                        *arguments = parsed;
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
                "error" => {
                    let message = value
                        .get("error")
                        .and_then(|error| error.get("message"))
                        .and_then(Value::as_str)
                        .unwrap_or("Anthropic stream error");
                    let error_message = assistant_error_message(model, message);
                    emit_event(
                        events,
                        AssistantMessageEvent::Error {
                            message: error_message.clone(),
                        },
                    );
                    return Ok(error_message);
                }
                _ => {}
            }
        }
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

pub fn stream_openai_responses(
    model: &RegistryModel,
    input: Vec<OpenAIInputItem>,
    options: OpenAICallOptions<'_>,
    events: &mut StreamEvents,
) -> Result<AssistantMessage, String> {
    let request = OpenAIRequest {
        model: options.model.to_string(),
        input,
        tools: if options.tools.is_empty() {
            None
        } else {
            Some(options.tools.to_vec())
        },
        stream: Some(true),
    };

    let headers = build_openai_headers(options.api_key, options.extra_headers)?;
    let endpoint = format!("{}/responses", options.base_url.trim_end_matches('/'));
    let client = Client::new();
    let mut response = client
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

    let mut parser = SseParser::new();
    let mut buf = [0u8; 8192];
    loop {
        let read = response
            .read(&mut buf)
            .map_err(|err| format!("Stream read failed: {err}"))?;
        if read == 0 {
            break;
        }
        let chunk = String::from_utf8_lossy(&buf[..read]);
        for event in parser.feed(&chunk) {
            let event_name = event.name.unwrap_or_default();
            if event.data == "[DONE]" {
                continue;
            }
            let value: Value = match serde_json::from_str(&event.data) {
                Ok(value) => value,
                Err(_) => continue,
            };
            match event_name.as_str() {
                "response.output_item.added" => {
                    let item = value.get("item").unwrap_or(&Value::Null);
                    let item_type = item.get("type").and_then(Value::as_str).unwrap_or("");
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
                                item.get("call_id").and_then(Value::as_str).unwrap_or(""),
                                item.get("id").and_then(Value::as_str).unwrap_or("")
                            ),
                            name: item
                                .get("name")
                                .and_then(Value::as_str)
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
                    let delta = value.get("delta").and_then(Value::as_str).unwrap_or("");
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
                    let delta = value.get("delta").and_then(Value::as_str).unwrap_or("");
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
                "response.function_call_arguments.delta" => {
                    let delta = value.get("delta").and_then(Value::as_str).unwrap_or("");
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
                    if let Some(index) = current_index {
                        if let Some(block) = partial.content.get(index) {
                            match block {
                                ContentBlock::Text { .. } => emit_event(
                                    events,
                                    AssistantMessageEvent::TextEnd {
                                        partial: partial.clone(),
                                        content_index: index,
                                    },
                                ),
                                ContentBlock::Thinking { .. } => emit_event(
                                    events,
                                    AssistantMessageEvent::ThinkingEnd {
                                        partial: partial.clone(),
                                        content_index: index,
                                    },
                                ),
                                ContentBlock::ToolCall { .. } => emit_event(
                                    events,
                                    AssistantMessageEvent::ToolCallEnd {
                                        partial: partial.clone(),
                                        content_index: index,
                                    },
                                ),
                                _ => {}
                            }
                        }
                    }
                }
                "response.completed" => {
                    if let Some(status) = value
                        .get("response")
                        .and_then(|response| response.get("status"))
                        .and_then(Value::as_str)
                    {
                        let mapped = match status {
                            "completed" => "stop",
                            "incomplete" => "length",
                            "failed" | "cancelled" => "error",
                            _ => "stop",
                        };
                        stop_reason = Some(mapped.to_string());
                    }
                }
                "response.error" => {
                    let message = value
                        .get("error")
                        .and_then(|error| error.get("message"))
                        .and_then(Value::as_str)
                        .unwrap_or("OpenAI stream error");
                    let error_message = assistant_error_message(model, message);
                    emit_event(
                        events,
                        AssistantMessageEvent::Error {
                            message: error_message.clone(),
                        },
                    );
                    return Ok(error_message);
                }
                _ => {}
            }
        }
    }

    if let Some(reason) = stop_reason {
        partial.stop_reason = reason;
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

pub fn build_anthropic_messages(context: &LlmContext) -> Vec<AnthropicMessage> {
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

pub fn assistant_message_from_anthropic(
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

pub fn assistant_error_message(model: &RegistryModel, message: &str) -> AssistantMessage {
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

pub fn openai_context_to_input_items(
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

pub fn openai_assistant_message_from_response(
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

#[cfg(test)]
mod tests {
    use super::SseParser;

    #[test]
    fn sse_parser_handles_complete_event() {
        let mut parser = SseParser::new();
        let events = parser.feed("event: test\ndata: {\"ok\":true}\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].name.as_deref(), Some("test"));
        assert_eq!(events[0].data, "{\"ok\":true}");
    }

    #[test]
    fn sse_parser_handles_split_events() {
        let mut parser = SseParser::new();
        let events = parser.feed("event: chunk\ndata: part1");
        assert!(events.is_empty());
        let events = parser.feed("\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].name.as_deref(), Some("chunk"));
        assert_eq!(events[0].data, "part1");
    }
}
