use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ai::AssistantMessageEvent;
use crate::core::messages::{
    AssistantMessage, ContentBlock, ToolResultMessage, UserContent, UserMessage,
};

mod agent_impl;

pub use agent_impl::{
    custom_message, get_model, Agent, AgentError, AgentOptions, AgentState, AgentStateOverride,
    QueueMode, ThinkingLevel,
};

#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub api: String,
    pub provider: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AgentToolResult {
    pub content: Vec<ContentBlock>,
    pub details: Value,
}

pub type ToolExecute = dyn Fn(&str, &Value) -> Result<AgentToolResult, String>;
pub type ConvertToLlmFn = dyn FnMut(&[AgentMessage]) -> Vec<AgentMessage>;
pub type TransformContextFn = dyn FnMut(&[AgentMessage]) -> Vec<AgentMessage>;
pub type SteeringFn = dyn FnMut() -> Vec<AgentMessage>;
pub type ListenerFn = dyn Fn(&AgentEvent);
pub type ApprovalFn = dyn FnMut(&ApprovalRequest) -> ApprovalResponse;

/// Request for tool approval before execution.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ApprovalRequest {
    /// Unique ID for this tool call
    pub tool_call_id: String,
    /// Name of the tool being called
    pub tool_name: String,
    /// Arguments passed to the tool
    pub args: Value,
    /// Shell command if this is a bash tool call
    pub command: Option<String>,
    /// Working directory for command execution
    pub cwd: Option<String>,
    /// Reason why approval is needed (e.g., "potentially destructive command")
    pub reason: Option<String>,
}

/// Response to an approval request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalResponse {
    /// Approve this single tool call
    Approve,
    /// Approve this tool for the rest of the session
    ApproveSession,
    /// Deny this tool call (skip it)
    Deny,
    /// Abort the entire agent loop
    Abort,
}

#[derive(Clone)]
pub struct AgentTool {
    pub name: String,
    pub label: String,
    pub description: String,
    pub execute: Rc<ToolExecute>,
}

impl std::fmt::Debug for AgentTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentTool")
            .field("name", &self.name)
            .field("label", &self.label)
            .field("description", &self.description)
            .finish()
    }
}

impl PartialEq for AgentTool {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.label == other.label
            && self.description == other.description
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CustomMessage {
    pub role: String,
    pub text: String,
    pub timestamp: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AgentMessage {
    User(UserMessage),
    Assistant(AssistantMessage),
    ToolResult(ToolResultMessage),
    Custom(CustomMessage),
}

impl AgentMessage {
    pub fn role(&self) -> &str {
        match self {
            AgentMessage::User(_) => "user",
            AgentMessage::Assistant(_) => "assistant",
            AgentMessage::ToolResult(_) => "toolResult",
            AgentMessage::Custom(custom) => custom.role.as_str(),
        }
    }

    pub fn user_text(&self) -> Option<&str> {
        match self {
            AgentMessage::User(message) => match &message.content {
                UserContent::Text(text) => Some(text.as_str()),
                _ => None,
            },
            _ => None,
        }
    }
}

pub struct AgentContext {
    pub system_prompt: String,
    pub messages: Vec<AgentMessage>,
    pub tools: Vec<AgentTool>,
    /// Working directory for tool execution (used in approval requests)
    pub cwd: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LlmContext {
    pub system_prompt: String,
    pub messages: Vec<AgentMessage>,
}

pub struct StreamEvents {
    handler: Box<dyn FnMut(AssistantMessageEvent)>,
}

impl StreamEvents {
    pub fn new(handler: Box<dyn FnMut(AssistantMessageEvent)>) -> Self {
        Self { handler }
    }

    pub fn emit(&mut self, event: AssistantMessageEvent) {
        (self.handler)(event);
    }
}

pub type StreamFn = dyn FnMut(&Model, &LlmContext, &mut StreamEvents) -> AssistantMessage;

pub struct AgentLoopConfig {
    pub model: Model,
    pub convert_to_llm: Box<ConvertToLlmFn>,
    pub transform_context: Option<Box<TransformContextFn>>,
    pub get_steering_messages: Option<Box<SteeringFn>>,
    pub get_follow_up_messages: Option<Box<SteeringFn>>,
    /// Optional callback to request approval before tool execution.
    /// If provided, will be called before each tool call. The callback
    /// should return an ApprovalResponse indicating whether to proceed.
    pub on_approval: Option<Box<ApprovalFn>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AgentEvent {
    AgentStart,
    AgentEnd {
        messages: Vec<AgentMessage>,
    },
    TurnStart,
    TurnEnd {
        message: AgentMessage,
        tool_results: Vec<ToolResultMessage>,
    },
    MessageStart {
        message: AgentMessage,
    },
    MessageUpdate {
        message: AgentMessage,
    },
    MessageEnd {
        message: AgentMessage,
    },
    ToolExecutionStart {
        tool_call_id: String,
        tool_name: String,
        args: Value,
    },
    ToolExecutionUpdate {
        tool_call_id: String,
        tool_name: String,
        args: Value,
        partial_result: AgentToolResult,
    },
    ToolExecutionEnd {
        tool_call_id: String,
        tool_name: String,
        result: AgentToolResult,
        is_error: bool,
    },
    /// Emitted when a tool requires approval before execution.
    /// The agent loop will pause until the approval callback responds.
    ApprovalRequest(ApprovalRequest),
}

impl AgentEvent {
    pub fn kind(&self) -> &'static str {
        match self {
            AgentEvent::AgentStart => "agent_start",
            AgentEvent::AgentEnd { .. } => "agent_end",
            AgentEvent::TurnStart => "turn_start",
            AgentEvent::TurnEnd { .. } => "turn_end",
            AgentEvent::MessageStart { .. } => "message_start",
            AgentEvent::MessageUpdate { .. } => "message_update",
            AgentEvent::MessageEnd { .. } => "message_end",
            AgentEvent::ToolExecutionStart { .. } => "tool_execution_start",
            AgentEvent::ToolExecutionUpdate { .. } => "tool_execution_update",
            AgentEvent::ToolExecutionEnd { .. } => "tool_execution_end",
            AgentEvent::ApprovalRequest { .. } => "approval_request",
        }
    }
}

#[derive(Debug)]
pub enum AgentLoopError {
    EmptyContext,
    LastMessageAssistant,
}

impl std::fmt::Display for AgentLoopError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentLoopError::EmptyContext => write!(f, "Cannot continue: no messages in context"),
            AgentLoopError::LastMessageAssistant => {
                write!(f, "Cannot continue from message role: assistant")
            }
        }
    }
}

impl std::error::Error for AgentLoopError {}

#[derive(Debug)]
pub struct AgentStream {
    events: Vec<AgentEvent>,
    result: Vec<AgentMessage>,
}

impl AgentStream {
    fn new() -> Self {
        Self {
            events: Vec::new(),
            result: Vec::new(),
        }
    }

    fn push(&mut self, event: AgentEvent) {
        self.events.push(event);
    }

    fn end(&mut self, messages: Vec<AgentMessage>) {
        self.result = messages;
    }

    pub fn events(&self) -> &[AgentEvent] {
        &self.events
    }

    pub fn result(&self) -> &[AgentMessage] {
        &self.result
    }
}

pub fn agent_loop<F>(
    prompts: Vec<AgentMessage>,
    mut context: AgentContext,
    mut config: AgentLoopConfig,
    stream_fn: &mut F,
) -> AgentStream
where
    F: FnMut(&Model, &LlmContext, &mut StreamEvents) -> AssistantMessage,
{
    let mut stream = AgentStream::new();
    let mut new_messages = prompts.clone();
    context.messages.extend(prompts.clone());

    stream.push(AgentEvent::AgentStart);
    stream.push(AgentEvent::TurnStart);
    for prompt in prompts {
        stream.push(AgentEvent::MessageStart {
            message: prompt.clone(),
        });
        stream.push(AgentEvent::MessageEnd { message: prompt });
    }

    run_loop(
        &mut context,
        &mut new_messages,
        &mut config,
        stream_fn,
        &mut stream,
    );

    stream
}

pub fn agent_loop_continue<F>(
    context: AgentContext,
    mut config: AgentLoopConfig,
    stream_fn: &mut F,
) -> Result<AgentStream, AgentLoopError>
where
    F: FnMut(&Model, &LlmContext, &mut StreamEvents) -> AssistantMessage,
{
    if context.messages.is_empty() {
        return Err(AgentLoopError::EmptyContext);
    }

    if matches!(context.messages.last(), Some(AgentMessage::Assistant(_))) {
        return Err(AgentLoopError::LastMessageAssistant);
    }

    let mut stream = AgentStream::new();
    let mut new_messages = Vec::new();
    let mut current_context = context;

    stream.push(AgentEvent::AgentStart);
    stream.push(AgentEvent::TurnStart);

    run_loop(
        &mut current_context,
        &mut new_messages,
        &mut config,
        stream_fn,
        &mut stream,
    );

    Ok(stream)
}

fn run_loop<F>(
    current_context: &mut AgentContext,
    new_messages: &mut Vec<AgentMessage>,
    config: &mut AgentLoopConfig,
    stream_fn: &mut F,
    stream: &mut AgentStream,
) where
    F: FnMut(&Model, &LlmContext, &mut StreamEvents) -> AssistantMessage,
{
    let mut first_turn = true;
    let mut pending_messages = config
        .get_steering_messages
        .as_mut()
        .map(|f| f())
        .unwrap_or_default();
    // Track tools that have been approved for the session
    let mut session_approved_tools = std::collections::HashSet::new();

    loop {
        let mut has_more_tool_calls = true;
        let mut steering_after_tools: Option<Vec<AgentMessage>> = None;

        while has_more_tool_calls || !pending_messages.is_empty() {
            if !first_turn {
                stream.push(AgentEvent::TurnStart);
            } else {
                first_turn = false;
            }

            if !pending_messages.is_empty() {
                for message in pending_messages.drain(..) {
                    stream.push(AgentEvent::MessageStart {
                        message: message.clone(),
                    });
                    stream.push(AgentEvent::MessageEnd {
                        message: message.clone(),
                    });
                    current_context.messages.push(message.clone());
                    new_messages.push(message);
                }
            }

            let message = stream_assistant_response(current_context, config, stream_fn, stream);
            new_messages.push(AgentMessage::Assistant(message.clone()));

            if message.stop_reason == "error" || message.stop_reason == "aborted" {
                stream.push(AgentEvent::TurnEnd {
                    message: AgentMessage::Assistant(message),
                    tool_results: Vec::new(),
                });
                stream.push(AgentEvent::AgentEnd {
                    messages: new_messages.clone(),
                });
                stream.end(new_messages.clone());
                return;
            }

            let tool_calls = extract_tool_calls(&message);
            has_more_tool_calls = !tool_calls.is_empty();

            let mut tool_results = Vec::new();
            if has_more_tool_calls {
                let tool_execution = execute_tool_calls(
                    &current_context.tools,
                    &message,
                    &mut config.get_steering_messages,
                    &mut config.on_approval,
                    current_context.cwd.as_deref(),
                    &mut session_approved_tools,
                    stream,
                );
                tool_results.extend(tool_execution.tool_results.clone());
                steering_after_tools = tool_execution.steering_messages;

                // If user aborted, end the loop early
                if tool_execution.aborted {
                    stream.push(AgentEvent::TurnEnd {
                        message: AgentMessage::Assistant(message),
                        tool_results,
                    });
                    stream.push(AgentEvent::AgentEnd {
                        messages: new_messages.clone(),
                    });
                    stream.end(new_messages.clone());
                    return;
                }

                for result in tool_execution.tool_results {
                    current_context
                        .messages
                        .push(AgentMessage::ToolResult(result.clone()));
                    new_messages.push(AgentMessage::ToolResult(result));
                }
            }

            stream.push(AgentEvent::TurnEnd {
                message: AgentMessage::Assistant(message),
                tool_results,
            });

            if let Some(steering) = steering_after_tools.take() {
                if !steering.is_empty() {
                    pending_messages = steering;
                }
            } else {
                pending_messages = config
                    .get_steering_messages
                    .as_mut()
                    .map(|f| f())
                    .unwrap_or_default();
            }
        }

        let follow_up_messages = config
            .get_follow_up_messages
            .as_mut()
            .map(|f| f())
            .unwrap_or_default();
        if !follow_up_messages.is_empty() {
            pending_messages = follow_up_messages;
            continue;
        }

        break;
    }

    stream.push(AgentEvent::AgentEnd {
        messages: new_messages.clone(),
    });
    stream.end(new_messages.clone());
}

fn stream_assistant_response<F>(
    context: &mut AgentContext,
    config: &mut AgentLoopConfig,
    stream_fn: &mut F,
    stream: &mut AgentStream,
) -> AssistantMessage
where
    F: FnMut(&Model, &LlmContext, &mut StreamEvents) -> AssistantMessage,
{
    let mut messages = context.messages.clone();
    if let Some(transform) = config.transform_context.as_mut() {
        messages = transform(&messages);
    }

    let llm_messages = (config.convert_to_llm)(&messages);
    let llm_context = LlmContext {
        system_prompt: context.system_prompt.clone(),
        messages: llm_messages,
    };

    let saw_event = Rc::new(Cell::new(false));
    let started = Rc::new(Cell::new(false));
    let last_partial: Rc<RefCell<Option<AssistantMessage>>> = Rc::new(RefCell::new(None));
    let stream_ptr: *mut AgentStream = stream as *mut _;
    let saw_event_ref = saw_event.clone();
    let started_ref = started.clone();
    let last_partial_ref = last_partial.clone();

    let handle_event = move |event: AssistantMessageEvent| {
        saw_event_ref.set(true);
        let partial = match event {
            AssistantMessageEvent::Start { partial }
            | AssistantMessageEvent::TextStart { partial, .. }
            | AssistantMessageEvent::TextDelta { partial, .. }
            | AssistantMessageEvent::TextEnd { partial, .. }
            | AssistantMessageEvent::ThinkingStart { partial, .. }
            | AssistantMessageEvent::ThinkingDelta { partial, .. }
            | AssistantMessageEvent::ThinkingEnd { partial, .. }
            | AssistantMessageEvent::ToolCallStart { partial, .. }
            | AssistantMessageEvent::ToolCallDelta { partial, .. }
            | AssistantMessageEvent::ToolCallEnd { partial, .. } => Some(partial),
            AssistantMessageEvent::Done { message } | AssistantMessageEvent::Error { message } => {
                last_partial_ref.replace(Some(message));
                None
            }
        };

        let Some(partial) = partial else {
            return;
        };

        last_partial_ref.replace(Some(partial.clone()));
        let agent_message = AgentMessage::Assistant(partial.clone());
        unsafe {
            let stream = &mut *stream_ptr;
            if !started_ref.get() {
                started_ref.set(true);
                stream.push(AgentEvent::MessageStart {
                    message: agent_message.clone(),
                });
            }
            stream.push(AgentEvent::MessageUpdate {
                message: agent_message,
            });
        }
    };

    let mut stream_events = StreamEvents::new(Box::new(handle_event));
    let message = stream_fn(&config.model, &llm_context, &mut stream_events);
    context
        .messages
        .push(AgentMessage::Assistant(message.clone()));

    if !saw_event.get() {
        stream.push(AgentEvent::MessageStart {
            message: AgentMessage::Assistant(message.clone()),
        });
        stream.push(AgentEvent::MessageUpdate {
            message: AgentMessage::Assistant(message.clone()),
        });
    } else if !started.get() {
        let partial = last_partial
            .borrow()
            .clone()
            .unwrap_or_else(|| message.clone());
        stream.push(AgentEvent::MessageStart {
            message: AgentMessage::Assistant(partial.clone()),
        });
        stream.push(AgentEvent::MessageUpdate {
            message: AgentMessage::Assistant(partial),
        });
    }
    stream.push(AgentEvent::MessageEnd {
        message: AgentMessage::Assistant(message.clone()),
    });

    message
}

struct ToolExecutionResult {
    tool_results: Vec<ToolResultMessage>,
    steering_messages: Option<Vec<AgentMessage>>,
    /// True if user aborted via approval callback
    aborted: bool,
}

fn execute_tool_calls(
    tools: &[AgentTool],
    assistant_message: &AssistantMessage,
    get_steering_messages: &mut Option<Box<dyn FnMut() -> Vec<AgentMessage>>>,
    on_approval: &mut Option<Box<ApprovalFn>>,
    cwd: Option<&str>,
    session_approved_tools: &mut std::collections::HashSet<String>,
    stream: &mut AgentStream,
) -> ToolExecutionResult {
    let tool_calls = extract_tool_calls(assistant_message);
    let mut results = Vec::new();
    let mut steering_messages: Option<Vec<AgentMessage>> = None;
    #[allow(unused_assignments)]
    let aborted = false;

    for (index, tool_call) in tool_calls.iter().enumerate() {
        let tool = tools.iter().find(|tool| tool.name == tool_call.name);

        // Check if approval is needed
        if let Some(approval_fn) = on_approval.as_mut() {
            // Skip approval if tool was already approved for session
            if !session_approved_tools.contains(&tool_call.name) {
                // Extract bash command if this is a bash tool
                let command = if tool_call.name == "bash" {
                    tool_call
                        .arguments
                        .get("command")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                } else {
                    None
                };

                let approval_request = ApprovalRequest {
                    tool_call_id: tool_call.id.clone(),
                    tool_name: tool_call.name.clone(),
                    args: tool_call.arguments.clone(),
                    command,
                    cwd: cwd.map(|s| s.to_string()),
                    reason: None,
                };

                // Emit approval request event
                stream.push(AgentEvent::ApprovalRequest(approval_request.clone()));

                // Call the approval callback and handle response
                let response = approval_fn(&approval_request);
                match response {
                    ApprovalResponse::Approve => {
                        // Continue with this tool call
                    }
                    ApprovalResponse::ApproveSession => {
                        // Remember this tool is approved for session
                        session_approved_tools.insert(tool_call.name.clone());
                    }
                    ApprovalResponse::Deny => {
                        // Skip this tool call with a denied message
                        results.push(deny_tool_call(tool_call, stream));
                        continue;
                    }
                    ApprovalResponse::Abort => {
                        // Skip remaining tool calls and abort the loop
                        results.push(deny_tool_call(tool_call, stream));
                        for skipped in tool_calls.iter().skip(index + 1) {
                            results.push(deny_tool_call(skipped, stream));
                        }
                        return ToolExecutionResult {
                            tool_results: results,
                            steering_messages: None,
                            aborted: true,
                        };
                    }
                }
            }
        }

        stream.push(AgentEvent::ToolExecutionStart {
            tool_call_id: tool_call.id.clone(),
            tool_name: tool_call.name.clone(),
            args: tool_call.arguments.clone(),
        });

        let mut is_error = false;
        let result = match tool {
            Some(tool) => match (tool.execute)(&tool_call.id, &tool_call.arguments) {
                Ok(result) => result,
                Err(err) => {
                    is_error = true;
                    AgentToolResult {
                        content: vec![ContentBlock::Text {
                            text: err,
                            text_signature: None,
                        }],
                        details: Value::Null,
                    }
                }
            },
            None => {
                is_error = true;
                AgentToolResult {
                    content: vec![ContentBlock::Text {
                        text: format!("Tool {} not found", tool_call.name),
                        text_signature: None,
                    }],
                    details: Value::Null,
                }
            }
        };

        stream.push(AgentEvent::ToolExecutionEnd {
            tool_call_id: tool_call.id.clone(),
            tool_name: tool_call.name.clone(),
            result: result.clone(),
            is_error,
        });

        let tool_result_message = ToolResultMessage {
            tool_call_id: tool_call.id.clone(),
            tool_name: tool_call.name.clone(),
            content: result.content.clone(),
            details: Some(result.details.clone()),
            is_error,
            timestamp: now_millis(),
        };

        results.push(tool_result_message.clone());
        stream.push(AgentEvent::MessageStart {
            message: AgentMessage::ToolResult(tool_result_message.clone()),
        });
        stream.push(AgentEvent::MessageEnd {
            message: AgentMessage::ToolResult(tool_result_message),
        });

        if let Some(get_steering_messages) = get_steering_messages.as_mut() {
            let steering = get_steering_messages();
            if !steering.is_empty() {
                steering_messages = Some(steering);
                for skipped in tool_calls.iter().skip(index + 1) {
                    results.push(skip_tool_call(skipped, stream));
                }
                break;
            }
        }
    }

    ToolExecutionResult {
        tool_results: results,
        steering_messages,
        aborted,
    }
}

fn skip_tool_call(tool_call: &ToolCall, stream: &mut AgentStream) -> ToolResultMessage {
    let result = AgentToolResult {
        content: vec![ContentBlock::Text {
            text: "Skipped due to queued user message.".to_string(),
            text_signature: None,
        }],
        details: Value::Null,
    };

    stream.push(AgentEvent::ToolExecutionStart {
        tool_call_id: tool_call.id.clone(),
        tool_name: tool_call.name.clone(),
        args: tool_call.arguments.clone(),
    });
    stream.push(AgentEvent::ToolExecutionEnd {
        tool_call_id: tool_call.id.clone(),
        tool_name: tool_call.name.clone(),
        result: result.clone(),
        is_error: true,
    });

    let tool_result_message = ToolResultMessage {
        tool_call_id: tool_call.id.clone(),
        tool_name: tool_call.name.clone(),
        content: result.content.clone(),
        details: Some(result.details),
        is_error: true,
        timestamp: now_millis(),
    };

    stream.push(AgentEvent::MessageStart {
        message: AgentMessage::ToolResult(tool_result_message.clone()),
    });
    stream.push(AgentEvent::MessageEnd {
        message: AgentMessage::ToolResult(tool_result_message.clone()),
    });

    tool_result_message
}

fn deny_tool_call(tool_call: &ToolCall, stream: &mut AgentStream) -> ToolResultMessage {
    let result = AgentToolResult {
        content: vec![ContentBlock::Text {
            text: "Tool call denied by user.".to_string(),
            text_signature: None,
        }],
        details: Value::Null,
    };

    stream.push(AgentEvent::ToolExecutionStart {
        tool_call_id: tool_call.id.clone(),
        tool_name: tool_call.name.clone(),
        args: tool_call.arguments.clone(),
    });
    stream.push(AgentEvent::ToolExecutionEnd {
        tool_call_id: tool_call.id.clone(),
        tool_name: tool_call.name.clone(),
        result: result.clone(),
        is_error: true,
    });

    let tool_result_message = ToolResultMessage {
        tool_call_id: tool_call.id.clone(),
        tool_name: tool_call.name.clone(),
        content: result.content.clone(),
        details: Some(result.details),
        is_error: true,
        timestamp: now_millis(),
    };

    stream.push(AgentEvent::MessageStart {
        message: AgentMessage::ToolResult(tool_result_message.clone()),
    });
    stream.push(AgentEvent::MessageEnd {
        message: AgentMessage::ToolResult(tool_result_message.clone()),
    });

    tool_result_message
}

#[derive(Clone, Debug)]
struct ToolCall {
    id: String,
    name: String,
    arguments: Value,
}

fn extract_tool_calls(message: &AssistantMessage) -> Vec<ToolCall> {
    message
        .content
        .iter()
        .filter_map(|content| match content {
            ContentBlock::ToolCall {
                id,
                name,
                arguments,
                ..
            } => Some(ToolCall {
                id: id.clone(),
                name: name.clone(),
                arguments: arguments.clone(),
            }),
            _ => None,
        })
        .collect()
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}
