use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

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
}

#[derive(Clone, Debug, PartialEq)]
pub struct LlmContext {
    pub system_prompt: String,
    pub messages: Vec<AgentMessage>,
}

pub type StreamFn = dyn FnMut(&Model, &LlmContext) -> AssistantMessage;

pub struct AgentLoopConfig {
    pub model: Model,
    pub convert_to_llm: Box<ConvertToLlmFn>,
    pub transform_context: Option<Box<TransformContextFn>>,
    pub get_steering_messages: Option<Box<SteeringFn>>,
    pub get_follow_up_messages: Option<Box<SteeringFn>>,
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

pub fn agent_loop(
    prompts: Vec<AgentMessage>,
    mut context: AgentContext,
    mut config: AgentLoopConfig,
    stream_fn: &mut StreamFn,
) -> AgentStream {
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

pub fn agent_loop_continue(
    context: AgentContext,
    mut config: AgentLoopConfig,
    stream_fn: &mut StreamFn,
) -> Result<AgentStream, AgentLoopError> {
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

fn run_loop(
    current_context: &mut AgentContext,
    new_messages: &mut Vec<AgentMessage>,
    config: &mut AgentLoopConfig,
    stream_fn: &mut StreamFn,
    stream: &mut AgentStream,
) {
    let mut first_turn = true;
    let mut pending_messages = config
        .get_steering_messages
        .as_mut()
        .map(|f| f())
        .unwrap_or_default();

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
                    stream,
                );
                tool_results.extend(tool_execution.tool_results.clone());
                steering_after_tools = tool_execution.steering_messages;

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

fn stream_assistant_response(
    context: &mut AgentContext,
    config: &mut AgentLoopConfig,
    stream_fn: &mut StreamFn,
    stream: &mut AgentStream,
) -> AssistantMessage {
    let mut messages = context.messages.clone();
    if let Some(transform) = config.transform_context.as_mut() {
        messages = transform(&messages);
    }

    let llm_messages = (config.convert_to_llm)(&messages);
    let llm_context = LlmContext {
        system_prompt: context.system_prompt.clone(),
        messages: llm_messages,
    };

    let message = stream_fn(&config.model, &llm_context);
    context
        .messages
        .push(AgentMessage::Assistant(message.clone()));

    stream.push(AgentEvent::MessageStart {
        message: AgentMessage::Assistant(message.clone()),
    });
    stream.push(AgentEvent::MessageUpdate {
        message: AgentMessage::Assistant(message.clone()),
    });
    stream.push(AgentEvent::MessageEnd {
        message: AgentMessage::Assistant(message.clone()),
    });

    message
}

struct ToolExecutionResult {
    tool_results: Vec<ToolResultMessage>,
    steering_messages: Option<Vec<AgentMessage>>,
}

fn execute_tool_calls(
    tools: &[AgentTool],
    assistant_message: &AssistantMessage,
    get_steering_messages: &mut Option<Box<dyn FnMut() -> Vec<AgentMessage>>>,
    stream: &mut AgentStream,
) -> ToolExecutionResult {
    let tool_calls = extract_tool_calls(assistant_message);
    let mut results = Vec::new();
    let mut steering_messages: Option<Vec<AgentMessage>> = None;

    for (index, tool_call) in tool_calls.iter().enumerate() {
        let tool = tools.iter().find(|tool| tool.name == tool_call.name);

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
