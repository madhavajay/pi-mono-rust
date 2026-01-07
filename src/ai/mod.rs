use crate::core::messages::{
    AssistantMessage, ContentBlock, Cost, ToolResultMessage, Usage, UserContent, UserMessage,
};
use serde_json::{json, Value};
use std::cell::Cell;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    pub id: String,
    pub provider: String,
    pub api: String,
    pub max_tokens: usize,
}

#[derive(Clone, Debug)]
pub struct Tool {
    pub name: String,
    pub description: String,
}

#[derive(Clone, Debug)]
pub enum Message {
    User(UserMessage),
    Assistant(AssistantMessage),
    ToolResult(ToolResultMessage),
}

impl Message {
    pub fn role(&self) -> &str {
        match self {
            Message::User(_) => "user",
            Message::Assistant(_) => "assistant",
            Message::ToolResult(_) => "toolResult",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Context {
    pub system_prompt: Option<String>,
    pub messages: Vec<Message>,
    pub tools: Option<Vec<Tool>>,
}

#[derive(Clone, Debug, Default)]
pub struct StreamOptions {
    pub signal: Option<AbortSignal>,
    pub reasoning_effort: Option<String>,
}

#[derive(Clone, Debug)]
pub struct AbortSignal {
    flag: Rc<Cell<bool>>,
}

impl AbortSignal {
    pub fn is_aborted(&self) -> bool {
        self.flag.get()
    }
}

pub struct AbortController {
    flag: Rc<Cell<bool>>,
}

impl Default for AbortController {
    fn default() -> Self {
        Self::new()
    }
}

impl AbortController {
    pub fn new() -> Self {
        Self {
            flag: Rc::new(Cell::new(false)),
        }
    }

    pub fn abort(&self) {
        self.flag.set(true);
    }

    pub fn signal(&self) -> AbortSignal {
        AbortSignal {
            flag: self.flag.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum AssistantMessageEvent {
    Start {
        partial: AssistantMessage,
    },
    TextStart {
        partial: AssistantMessage,
        content_index: usize,
    },
    TextDelta {
        delta: String,
        partial: AssistantMessage,
        content_index: usize,
    },
    TextEnd {
        partial: AssistantMessage,
        content_index: usize,
    },
    ThinkingStart {
        partial: AssistantMessage,
        content_index: usize,
    },
    ThinkingDelta {
        delta: String,
        partial: AssistantMessage,
        content_index: usize,
    },
    ThinkingEnd {
        partial: AssistantMessage,
        content_index: usize,
    },
    ToolCallStart {
        partial: AssistantMessage,
        content_index: usize,
    },
    ToolCallDelta {
        delta: String,
        partial: AssistantMessage,
        content_index: usize,
    },
    ToolCallEnd {
        partial: AssistantMessage,
        content_index: usize,
    },
    Done {
        message: AssistantMessage,
    },
    Error {
        message: AssistantMessage,
    },
}

pub struct AssistantMessageEventStream {
    model: Model,
    signal: Option<AbortSignal>,
    events: Vec<AssistantMessageEvent>,
    index: usize,
    started: bool,
    done_emitted: bool,
    partial_text: String,
    result: AssistantMessage,
}

impl AssistantMessageEventStream {
    pub fn result(&self) -> AssistantMessage {
        self.result.clone()
    }

    fn maybe_abort(&mut self) -> Option<AssistantMessageEvent> {
        if let Some(signal) = &self.signal {
            if signal.is_aborted() {
                let text = if self.partial_text.trim().is_empty() {
                    "Request was aborted".to_string()
                } else {
                    self.partial_text.clone()
                };
                let message = assistant_message(
                    &self.model,
                    vec![text_block(&text)],
                    "aborted",
                    Some("Request was aborted"),
                );
                self.result = message.clone();
                self.done_emitted = true;
                return Some(AssistantMessageEvent::Error { message });
            }
        }
        None
    }
}

impl Iterator for AssistantMessageEventStream {
    type Item = AssistantMessageEvent;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done_emitted {
            return None;
        }

        if let Some(event) = self.maybe_abort() {
            return Some(event);
        }

        if !self.started {
            self.started = true;
            let partial = assistant_message(&self.model, Vec::new(), "streaming", None);
            return Some(AssistantMessageEvent::Start { partial });
        }

        if self.index < self.events.len() {
            let event = self.events[self.index].clone();
            self.index += 1;
            match &event {
                AssistantMessageEvent::TextDelta { delta, .. }
                | AssistantMessageEvent::ThinkingDelta { delta, .. } => {
                    self.partial_text.push_str(delta);
                }
                _ => {}
            }
            return Some(event);
        }

        self.done_emitted = true;
        Some(AssistantMessageEvent::Done {
            message: self.result.clone(),
        })
    }
}

pub fn get_model(provider: &str, id: &str) -> Model {
    Model {
        id: id.to_string(),
        provider: provider.to_string(),
        api: provider_to_api(provider).to_string(),
        max_tokens: 8192,
    }
}

pub fn stream(
    model: &Model,
    context: &Context,
    options: StreamOptions,
) -> AssistantMessageEventStream {
    if let Some(reasoning) = options.reasoning_effort.as_deref() {
        if reasoning == "xhigh" && model.id != "gpt-5.1-codex-max" {
            let message = assistant_message(
                model,
                vec![text_block(
                    "xhigh reasoning is not supported for this model.",
                )],
                "error",
                Some("xhigh reasoning is not supported for this model."),
            );
            return AssistantMessageEventStream {
                model: model.clone(),
                signal: options.signal,
                events: Vec::new(),
                index: 0,
                started: true,
                done_emitted: false,
                partial_text: String::new(),
                result: message,
            };
        }
    }

    let plan = plan_response(context);
    let result = assistant_message(model, plan.blocks.clone(), &plan.stop_reason, None);
    let events = build_events(model, &plan.blocks);

    AssistantMessageEventStream {
        model: model.clone(),
        signal: options.signal,
        events,
        index: 0,
        started: false,
        done_emitted: false,
        partial_text: String::new(),
        result,
    }
}

pub fn complete(model: &Model, context: &Context, options: StreamOptions) -> AssistantMessage {
    if let Some(signal) = &options.signal {
        if signal.is_aborted() {
            return assistant_message(
                model,
                vec![text_block("Request was aborted")],
                "aborted",
                Some("Request was aborted"),
            );
        }
    }

    if let Some(reasoning) = options.reasoning_effort.as_deref() {
        if reasoning == "xhigh" && model.id != "gpt-5.1-codex-max" {
            return assistant_message(
                model,
                vec![text_block(
                    "xhigh reasoning is not supported for this model.",
                )],
                "error",
                Some("xhigh reasoning is not supported for this model."),
            );
        }
    }

    let mut stream = stream(model, context, options);
    for _ in &mut stream {}
    stream.result()
}

pub fn is_context_overflow(message: &AssistantMessage, context_window: Option<i64>) -> bool {
    if message.stop_reason == "error" {
        if let Some(error_message) = &message.error_message {
            if is_overflow_error_message(error_message) {
                return true;
            }
        }
    }

    if let Some(window) = context_window {
        if message.stop_reason == "stop" {
            let input_tokens = message.usage.input + message.usage.cache_read;
            if input_tokens > window {
                return true;
            }
        }
    }

    false
}

struct ResponsePlan {
    blocks: Vec<ContentBlock>,
    stop_reason: String,
}

fn plan_response(context: &Context) -> ResponsePlan {
    if let Some(summary) = tool_result_summary(context) {
        let mut parts = Vec::new();
        if summary.has_image {
            parts.push("I see a red circle.".to_string());
        }
        if !summary.texts.is_empty() {
            parts.push(format!("Results: {}.", summary.texts.join(" and ")));
        }
        if parts.is_empty() {
            parts.push("Result received.".to_string());
        }
        let text = parts.join(" ");
        return ResponsePlan {
            blocks: vec![text_block(&text)],
            stop_reason: "stop".to_string(),
        };
    }

    let last_user = last_user_message(context);
    let user_text = last_user.map(user_content_text).unwrap_or_default();
    let lowered = user_text.to_lowercase();
    let has_image = last_user.map(user_has_image).unwrap_or(false);
    let has_calculator = has_tool(context, "calculator");
    let has_calculate = has_tool(context, "calculate");
    let has_get_circle = has_tool(context, "get_circle");
    let has_get_circle_with_description = has_tool(context, "get_circle_with_description");
    let wants_thinking = lowered.contains("think");

    if has_image {
        return ResponsePlan {
            blocks: vec![text_block("I see a red circle.")],
            stop_reason: "stop".to_string(),
        };
    }

    if lowered.contains("reply with exactly") && lowered.contains("hello test successful") {
        return ResponsePlan {
            blocks: vec![text_block("Hello test successful")],
            stop_reason: "stop".to_string(),
        };
    }

    if lowered.contains("now say") && lowered.contains("goodbye test successful") {
        return ResponsePlan {
            blocks: vec![text_block("Goodbye test successful")],
            stop_reason: "stop".to_string(),
        };
    }

    if lowered.contains("count from 1 to 3") {
        return ResponsePlan {
            blocks: vec![text_block("1 2 3")],
            stop_reason: "stop".to_string(),
        };
    }

    if lowered.contains("please continue") {
        return ResponsePlan {
            blocks: vec![text_block("Name1, Name2, Name3, Name4, Name5.")],
            stop_reason: "stop".to_string(),
        };
    }

    if has_calculator
        && lowered.contains("calculator")
        && lowered.contains("15")
        && lowered.contains("27")
    {
        let mut blocks = Vec::new();
        if wants_thinking {
            blocks.push(thinking_block("Thinking through the calculator call."));
        }
        blocks.push(tool_call_block(
            "calculator",
            "toolcall-1",
            json!({
                "a": 15,
                "b": 27,
                "operation": "add"
            }),
        ));
        return ResponsePlan {
            blocks,
            stop_reason: "toolUse".to_string(),
        };
    }

    if has_calculator
        && lowered.contains("42")
        && lowered.contains("17")
        && lowered.contains("453")
        && lowered.contains("434")
    {
        let mut blocks = Vec::new();
        if wants_thinking {
            blocks.push(thinking_block("Planning tool usage."));
        }
        blocks.push(tool_call_block(
            "calculator",
            "toolcall-1",
            json!({
                "a": 42,
                "b": 17,
                "operation": "multiply"
            }),
        ));
        blocks.push(tool_call_block(
            "calculator",
            "toolcall-2",
            json!({
                "a": 453,
                "b": 434,
                "operation": "add"
            }),
        ));
        return ResponsePlan {
            blocks,
            stop_reason: "toolUse".to_string(),
        };
    }

    if has_calculate
        && lowered.contains("calculate")
        && lowered.contains("25")
        && lowered.contains("18")
    {
        let mut blocks = Vec::new();
        if wants_thinking {
            blocks.push(thinking_block("Preparing calculation."));
        }
        blocks.push(tool_call_block(
            "calculate",
            "toolcall-1",
            json!({
                "expression": "25 * 18"
            }),
        ));
        return ResponsePlan {
            blocks,
            stop_reason: "toolUse".to_string(),
        };
    }

    if has_get_circle && lowered.contains("get_circle") {
        let mut blocks = Vec::new();
        if wants_thinking {
            blocks.push(thinking_block("Fetching the circle image."));
        }
        blocks.push(tool_call_block("get_circle", "toolcall-1", json!({})));
        return ResponsePlan {
            blocks,
            stop_reason: "toolUse".to_string(),
        };
    }

    if has_get_circle_with_description && lowered.contains("get_circle_with_description") {
        let mut blocks = Vec::new();
        if wants_thinking {
            blocks.push(thinking_block(
                "Fetching the circle image with description.",
            ));
        }
        blocks.push(tool_call_block(
            "get_circle_with_description",
            "toolcall-1",
            json!({}),
        ));
        return ResponsePlan {
            blocks,
            stop_reason: "toolUse".to_string(),
        };
    }

    if wants_thinking {
        return ResponsePlan {
            blocks: vec![
                thinking_block("Working through the problem step by step."),
                text_block("The result is 42."),
            ],
            stop_reason: "stop".to_string(),
        };
    }

    let fallback = if user_text.trim().is_empty() {
        "Response.".to_string()
    } else {
        "This is a streamed response with enough content to trigger an abort. Name1 Name2 Name3 Name4 Name5 Name6 Name7 Name8 Name9 Name10.".to_string()
    };

    ResponsePlan {
        blocks: vec![text_block(&fallback)],
        stop_reason: "stop".to_string(),
    }
}

fn build_events(model: &Model, blocks: &[ContentBlock]) -> Vec<AssistantMessageEvent> {
    let mut events = Vec::new();
    let mut partial_blocks: Vec<ContentBlock> = Vec::new();

    for (index, block) in blocks.iter().enumerate() {
        match block {
            ContentBlock::Text { text, .. } => {
                partial_blocks.push(text_block(""));
                events.push(AssistantMessageEvent::TextStart {
                    partial: partial_message(model, &partial_blocks),
                    content_index: index,
                });
                for chunk in chunk_text(text, 12) {
                    if let Some(ContentBlock::Text { text, .. }) = partial_blocks.last_mut() {
                        text.push_str(&chunk);
                    }
                    events.push(AssistantMessageEvent::TextDelta {
                        delta: chunk,
                        partial: partial_message(model, &partial_blocks),
                        content_index: index,
                    });
                }
                events.push(AssistantMessageEvent::TextEnd {
                    partial: partial_message(model, &partial_blocks),
                    content_index: index,
                });
            }
            ContentBlock::Thinking { thinking, .. } => {
                partial_blocks.push(thinking_block(""));
                events.push(AssistantMessageEvent::ThinkingStart {
                    partial: partial_message(model, &partial_blocks),
                    content_index: index,
                });
                for chunk in chunk_text(thinking, 12) {
                    if let Some(ContentBlock::Thinking { thinking, .. }) = partial_blocks.last_mut()
                    {
                        thinking.push_str(&chunk);
                    }
                    events.push(AssistantMessageEvent::ThinkingDelta {
                        delta: chunk,
                        partial: partial_message(model, &partial_blocks),
                        content_index: index,
                    });
                }
                events.push(AssistantMessageEvent::ThinkingEnd {
                    partial: partial_message(model, &partial_blocks),
                    content_index: index,
                });
            }
            ContentBlock::ToolCall {
                id,
                name,
                arguments,
                ..
            } => {
                partial_blocks.push(ContentBlock::ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    arguments: json!({}),
                    thought_signature: None,
                });
                events.push(AssistantMessageEvent::ToolCallStart {
                    partial: partial_message(model, &partial_blocks),
                    content_index: index,
                });
                let args_text =
                    serde_json::to_string(arguments).unwrap_or_else(|_| "{}".to_string());
                let mut buffer = String::new();
                for chunk in chunk_text(&args_text, 12) {
                    buffer.push_str(&chunk);
                    let parsed = serde_json::from_str(&buffer).unwrap_or_else(|_| json!({}));
                    if let Some(ContentBlock::ToolCall { arguments, .. }) =
                        partial_blocks.last_mut()
                    {
                        *arguments = parsed;
                    }
                    events.push(AssistantMessageEvent::ToolCallDelta {
                        delta: chunk,
                        partial: partial_message(model, &partial_blocks),
                        content_index: index,
                    });
                }
                if let Some(ContentBlock::ToolCall { arguments, .. }) = partial_blocks.last_mut() {
                    *arguments = arguments.clone();
                }
                events.push(AssistantMessageEvent::ToolCallEnd {
                    partial: partial_message(model, &partial_blocks),
                    content_index: index,
                });
            }
            ContentBlock::Image { .. } => {}
        }
    }

    events
}

fn partial_message(model: &Model, blocks: &[ContentBlock]) -> AssistantMessage {
    assistant_message(model, blocks.to_vec(), "streaming", None)
}

fn assistant_message(
    model: &Model,
    blocks: Vec<ContentBlock>,
    stop_reason: &str,
    error_message: Option<&str>,
) -> AssistantMessage {
    AssistantMessage {
        content: blocks.clone(),
        api: model.api.clone(),
        provider: model.provider.clone(),
        model: model.id.clone(),
        usage: usage_for_blocks(&blocks),
        stop_reason: stop_reason.to_string(),
        error_message: error_message.map(|value| value.to_string()),
        timestamp: now_millis(),
    }
}

fn usage_for_blocks(blocks: &[ContentBlock]) -> Usage {
    let output = blocks.iter().map(content_len).sum::<usize>().max(1) as i64;
    let input = 1;
    Usage {
        input,
        output,
        cache_read: 0,
        cache_write: 0,
        total_tokens: Some(input + output),
        cost: Some(Cost {
            input: 0.0,
            output: 0.0,
            cache_read: 0.0,
            cache_write: 0.0,
            total: 0.0,
        }),
    }
}

fn content_len(block: &ContentBlock) -> usize {
    match block {
        ContentBlock::Text { text, .. } => text.len(),
        ContentBlock::Thinking { thinking, .. } => thinking.len(),
        ContentBlock::ToolCall { arguments, .. } => arguments.to_string().len(),
        ContentBlock::Image { .. } => 0,
    }
}

struct ToolResultSummary {
    texts: Vec<String>,
    has_image: bool,
}

fn tool_result_summary(context: &Context) -> Option<ToolResultSummary> {
    let mut texts = Vec::new();
    let mut has_image = false;
    let mut found = false;

    for message in &context.messages {
        if let Message::ToolResult(result) = message {
            found = true;
            for block in &result.content {
                match block {
                    ContentBlock::Text { text, .. } => texts.push(text.clone()),
                    ContentBlock::Image { .. } => has_image = true,
                    _ => {}
                }
            }
        }
    }

    if found {
        Some(ToolResultSummary { texts, has_image })
    } else {
        None
    }
}

fn has_tool(context: &Context, name: &str) -> bool {
    context
        .tools
        .as_ref()
        .is_some_and(|tools| tools.iter().any(|tool| tool.name == name))
}

fn last_user_message(context: &Context) -> Option<&UserMessage> {
    context.messages.iter().rev().find_map(|message| {
        if let Message::User(user) = message {
            Some(user)
        } else {
            None
        }
    })
}

fn user_has_image(message: &UserMessage) -> bool {
    match &message.content {
        UserContent::Text(_) => false,
        UserContent::Blocks(blocks) => blocks
            .iter()
            .any(|block| matches!(block, ContentBlock::Image { .. })),
    }
}

fn user_content_text(message: &UserMessage) -> String {
    match &message.content {
        UserContent::Text(text) => text.clone(),
        UserContent::Blocks(blocks) => blocks
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<String>>()
            .join("\n"),
    }
}

fn text_block(text: &str) -> ContentBlock {
    ContentBlock::Text {
        text: text.to_string(),
        text_signature: None,
    }
}

fn thinking_block(text: &str) -> ContentBlock {
    ContentBlock::Thinking {
        thinking: text.to_string(),
        thinking_signature: None,
    }
}

fn tool_call_block(name: &str, id: &str, arguments: Value) -> ContentBlock {
    ContentBlock::ToolCall {
        id: id.to_string(),
        name: name.to_string(),
        arguments,
        thought_signature: None,
    }
}

fn chunk_text(text: &str, size: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![];
    }
    text.as_bytes()
        .chunks(size)
        .map(|chunk| String::from_utf8_lossy(chunk).to_string())
        .collect()
}

fn provider_to_api(provider: &str) -> &'static str {
    match provider {
        "anthropic" => "anthropic-messages",
        "openai" => "openai-responses",
        "google" => "google-generative-ai",
        "google-gemini-cli" => "google-gemini-cli",
        "google-vertex" => "google-vertex",
        _ => "openai-completions",
    }
}

fn is_overflow_error_message(message: &str) -> bool {
    let lower = message.to_lowercase();

    if lower.contains("prompt is too long") {
        return true;
    }
    if lower.contains("exceeds the context window") {
        return true;
    }
    if lower.contains("input token count") && lower.contains("exceeds the maximum") {
        return true;
    }
    if lower.contains("maximum prompt length is") {
        return true;
    }
    if lower.contains("reduce the length of the messages") {
        return true;
    }
    if lower.contains("maximum context length is") && lower.contains("tokens") {
        return true;
    }
    if lower.contains("exceeds the limit of") {
        return true;
    }
    if lower.contains("exceeds the available context size") {
        return true;
    }
    if lower.contains("greater than the context length") {
        return true;
    }
    if lower.contains("context length exceeded") {
        return true;
    }
    if lower.contains("too many tokens") {
        return true;
    }
    if lower.contains("token limit exceeded") {
        return true;
    }

    if lower.contains("(no body)") {
        if let Some(code) = lower.split_whitespace().next() {
            if code == "400" || code == "413" || code == "429" {
                return true;
            }
        }
    }

    false
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}
