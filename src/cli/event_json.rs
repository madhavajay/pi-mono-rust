use crate::agent::{AgentEvent, AgentMessage, AgentToolResult};
use crate::coding_agent::AgentSessionEvent;
use crate::core::messages::{AgentMessage as CoreAgentMessage, ToolResultMessage};
use serde_json::{json, Value};

pub fn serialize_session_event(event: &AgentSessionEvent) -> Option<Value> {
    match event {
        AgentSessionEvent::Agent(agent_event) => Some(agent_event_value(agent_event)),
        AgentSessionEvent::AutoCompactionStart { reason } => Some(json!({
            "type": "auto_compaction_start",
            "reason": reason,
        })),
        AgentSessionEvent::AutoCompactionEnd { aborted } => Some(json!({
            "type": "auto_compaction_end",
            "aborted": aborted,
            "result": Value::Null,
            "willRetry": false,
        })),
    }
}

fn agent_event_value(event: &AgentEvent) -> Value {
    match event {
        AgentEvent::AgentStart => json!({ "type": "agent_start" }),
        AgentEvent::AgentEnd { messages } => json!({
            "type": "agent_end",
            "messages": messages.iter().map(agent_message_value).collect::<Vec<_>>(),
        }),
        AgentEvent::TurnStart => json!({ "type": "turn_start" }),
        AgentEvent::TurnEnd {
            message,
            tool_results,
        } => json!({
            "type": "turn_end",
            "message": agent_message_value(message),
            "toolResults": tool_results.iter().map(tool_result_value).collect::<Vec<_>>(),
        }),
        AgentEvent::MessageStart { message } => json!({
            "type": "message_start",
            "message": agent_message_value(message),
        }),
        AgentEvent::MessageUpdate { message } => json!({
            "type": "message_update",
            "message": agent_message_value(message),
        }),
        AgentEvent::MessageEnd { message } => json!({
            "type": "message_end",
            "message": agent_message_value(message),
        }),
        AgentEvent::ToolExecutionStart {
            tool_call_id,
            tool_name,
            args,
        } => json!({
            "type": "tool_execution_start",
            "toolCallId": tool_call_id,
            "toolName": tool_name,
            "args": args,
        }),
        AgentEvent::ToolExecutionUpdate {
            tool_call_id,
            tool_name,
            args,
            partial_result,
        } => json!({
            "type": "tool_execution_update",
            "toolCallId": tool_call_id,
            "toolName": tool_name,
            "args": args,
            "partialResult": agent_tool_result_value(partial_result),
        }),
        AgentEvent::ToolExecutionEnd {
            tool_call_id,
            tool_name,
            result,
            is_error,
        } => json!({
            "type": "tool_execution_end",
            "toolCallId": tool_call_id,
            "toolName": tool_name,
            "result": agent_tool_result_value(result),
            "isError": is_error,
        }),
    }
}

fn agent_message_value(message: &AgentMessage) -> Value {
    match message {
        AgentMessage::User(user) => core_message_value(CoreAgentMessage::User(user.clone())),
        AgentMessage::Assistant(assistant) => {
            core_message_value(CoreAgentMessage::Assistant(assistant.clone()))
        }
        AgentMessage::ToolResult(result) => {
            core_message_value(CoreAgentMessage::ToolResult(result.clone()))
        }
        AgentMessage::Custom(custom) => json!({
            "role": custom.role,
            "text": custom.text,
            "timestamp": custom.timestamp,
        }),
    }
}

pub fn serialize_agent_message(message: &AgentMessage) -> Value {
    agent_message_value(message)
}

fn tool_result_value(result: &ToolResultMessage) -> Value {
    serde_json::to_value(result).unwrap_or(Value::Null)
}

fn agent_tool_result_value(result: &AgentToolResult) -> Value {
    json!({
        "content": result.content,
        "details": result.details,
    })
}

fn core_message_value(message: CoreAgentMessage) -> Value {
    serde_json::to_value(message).unwrap_or(Value::Null)
}
