use std::cell::{Cell, RefCell};
use std::rc::Rc;

use pi::agent::{
    agent_loop, agent_loop_continue, AgentContext, AgentEvent, AgentLoopConfig, AgentMessage,
    AgentTool, AgentToolResult, CustomMessage, LlmContext, Model,
};
use pi::{AssistantMessage, ContentBlock, Cost, Usage, UserContent, UserMessage};
use serde_json::json;

// Source: packages/agent/test/agent-loop.test.ts

#[test]
fn should_emit_events_with_agentmessage_types() {
    let context = AgentContext {
        system_prompt: "You are helpful.".to_string(),
        messages: Vec::new(),
        tools: Vec::new(),
        cwd: None,
    };

    let user_prompt = create_user_message("Hello");

    let config = AgentLoopConfig {
        model: create_model(),
        convert_to_llm: Box::new(identity_converter),
        transform_context: None,
        get_steering_messages: None,
        get_follow_up_messages: None,
        on_approval: None,
    };

    let mut stream_fn: Box<pi::agent::StreamFn> =
        Box::new(|_model: &Model, _ctx: &LlmContext, _events| {
            create_assistant_message(
                vec![ContentBlock::Text {
                    text: "Hi there!".to_string(),
                    text_signature: None,
                }],
                "stop",
            )
        });

    let stream = agent_loop(vec![user_prompt], context, config, &mut stream_fn);
    let events = stream.events().to_vec();
    let messages = stream.result().to_vec();

    assert_eq!(messages.len(), 2);
    assert!(matches!(messages[0], AgentMessage::User(_)));
    assert!(matches!(messages[1], AgentMessage::Assistant(_)));

    let event_types: Vec<&str> = events.iter().map(AgentEvent::kind).collect();
    assert!(event_types.contains(&"agent_start"));
    assert!(event_types.contains(&"turn_start"));
    assert!(event_types.contains(&"message_start"));
    assert!(event_types.contains(&"message_end"));
    assert!(event_types.contains(&"turn_end"));
    assert!(event_types.contains(&"agent_end"));
}

#[test]
fn should_handle_custom_message_types_via_converttollm() {
    let notification = AgentMessage::Custom(CustomMessage {
        role: "notification".to_string(),
        text: "This is a notification".to_string(),
        timestamp: now_millis(),
    });

    let context = AgentContext {
        system_prompt: "You are helpful.".to_string(),
        messages: vec![notification],
        tools: Vec::new(),
        cwd: None,
    };

    let user_prompt = create_user_message("Hello");

    let converted_messages: Rc<RefCell<Vec<AgentMessage>>> = Rc::new(RefCell::new(Vec::new()));
    let converted_messages_ref = converted_messages.clone();

    let config = AgentLoopConfig {
        model: create_model(),
        convert_to_llm: Box::new(move |messages| {
            let filtered: Vec<AgentMessage> = messages
                .iter().filter(|&message| !matches!(message, AgentMessage::Custom(custom) if custom.role == "notification")).cloned()
                .collect();
            *converted_messages_ref.borrow_mut() = filtered.clone();
            filtered
        }),
        transform_context: None,
        get_steering_messages: None,
        get_follow_up_messages: None,
        on_approval: None,
    };

    let mut stream_fn: Box<pi::agent::StreamFn> =
        Box::new(|_model: &Model, _ctx: &LlmContext, _events| {
            create_assistant_message(
                vec![ContentBlock::Text {
                    text: "Response".to_string(),
                    text_signature: None,
                }],
                "stop",
            )
        });

    let stream = agent_loop(vec![user_prompt], context, config, &mut stream_fn);

    let converted = converted_messages.borrow();
    assert_eq!(converted.len(), 1);
    assert!(matches!(converted[0], AgentMessage::User(_)));

    let _events = stream.events();
}

#[test]
fn should_apply_transformcontext_before_converttollm() {
    let context = AgentContext {
        system_prompt: "You are helpful.".to_string(),
        messages: vec![
            create_user_message("old message 1"),
            AgentMessage::Assistant(create_assistant_message(
                vec![ContentBlock::Text {
                    text: "old response 1".to_string(),
                    text_signature: None,
                }],
                "stop",
            )),
            create_user_message("old message 2"),
            AgentMessage::Assistant(create_assistant_message(
                vec![ContentBlock::Text {
                    text: "old response 2".to_string(),
                    text_signature: None,
                }],
                "stop",
            )),
        ],
        tools: Vec::new(),
        cwd: None,
    };

    let user_prompt = create_user_message("new message");

    let transformed_messages: Rc<RefCell<Vec<AgentMessage>>> = Rc::new(RefCell::new(Vec::new()));
    let converted_messages: Rc<RefCell<Vec<AgentMessage>>> = Rc::new(RefCell::new(Vec::new()));
    let transformed_messages_ref = transformed_messages.clone();
    let converted_messages_ref = converted_messages.clone();

    let config = AgentLoopConfig {
        model: create_model(),
        transform_context: Some(Box::new(move |messages| {
            let pruned: Vec<AgentMessage> = messages.iter().cloned().rev().take(2).collect();
            let mut pruned = pruned;
            pruned.reverse();
            *transformed_messages_ref.borrow_mut() = pruned.clone();
            pruned
        })),
        convert_to_llm: Box::new(move |messages| {
            let converted: Vec<AgentMessage> = messages
                .iter()
                .filter(|&m| {
                    matches!(
                        m,
                        AgentMessage::User(_)
                            | AgentMessage::Assistant(_)
                            | AgentMessage::ToolResult(_)
                    )
                })
                .cloned()
                .collect();
            *converted_messages_ref.borrow_mut() = converted.clone();
            converted
        }),
        get_steering_messages: None,
        get_follow_up_messages: None,
        on_approval: None,
    };

    let mut stream_fn: Box<pi::agent::StreamFn> =
        Box::new(|_model: &Model, _ctx: &LlmContext, _events| {
            create_assistant_message(
                vec![ContentBlock::Text {
                    text: "Response".to_string(),
                    text_signature: None,
                }],
                "stop",
            )
        });

    let _stream = agent_loop(vec![user_prompt], context, config, &mut stream_fn);

    assert_eq!(transformed_messages.borrow().len(), 2);
    assert_eq!(converted_messages.borrow().len(), 2);
}

#[test]
fn should_handle_tool_calls_and_results() {
    let executed: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let executed_ref = executed.clone();
    let tool = AgentTool {
        name: "echo".to_string(),
        label: "Echo".to_string(),
        description: "Echo tool".to_string(),
        execute: Rc::new(move |_tool_call_id, params| {
            let value = params
                .get("value")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            executed_ref.borrow_mut().push(value.clone());
            Ok(AgentToolResult {
                content: vec![ContentBlock::Text {
                    text: format!("echoed: {value}"),
                    text_signature: None,
                }],
                details: json!({ "value": value }),
            })
        }),
    };

    let context = AgentContext {
        system_prompt: String::new(),
        messages: Vec::new(),
        tools: vec![tool],
        cwd: None,
    };

    let user_prompt = create_user_message("echo something");

    let config = AgentLoopConfig {
        model: create_model(),
        convert_to_llm: Box::new(identity_converter),
        transform_context: None,
        get_steering_messages: None,
        get_follow_up_messages: None,
        on_approval: None,
    };

    let call_index = Rc::new(Cell::new(0));
    let call_index_ref = call_index.clone();
    let mut stream_fn: Box<pi::agent::StreamFn> =
        Box::new(move |_model: &Model, _ctx: &LlmContext, _events| {
            let index = call_index_ref.get();
            let message = if index == 0 {
                create_assistant_message(
                    vec![ContentBlock::ToolCall {
                        id: "tool-1".to_string(),
                        name: "echo".to_string(),
                        arguments: json!({ "value": "hello" }),
                        thought_signature: None,
                    }],
                    "toolUse",
                )
            } else {
                create_assistant_message(
                    vec![ContentBlock::Text {
                        text: "done".to_string(),
                        text_signature: None,
                    }],
                    "stop",
                )
            };
            call_index_ref.set(index + 1);
            message
        });

    let stream = agent_loop(vec![user_prompt], context, config, &mut stream_fn);

    assert_eq!(executed.borrow().as_slice(), ["hello"]);

    let tool_start = stream
        .events()
        .iter()
        .find(|event| matches!(event, AgentEvent::ToolExecutionStart { .. }));
    let tool_end = stream
        .events()
        .iter()
        .find(|event| matches!(event, AgentEvent::ToolExecutionEnd { .. }));

    assert!(tool_start.is_some());
    assert!(tool_end.is_some());
    if let Some(AgentEvent::ToolExecutionEnd { is_error, .. }) = tool_end {
        assert!(!is_error);
    }
}

#[test]
fn should_inject_queued_messages_and_skip_remaining_tool_calls() {
    let executed: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let executed_ref = executed.clone();
    let tool = AgentTool {
        name: "echo".to_string(),
        label: "Echo".to_string(),
        description: "Echo tool".to_string(),
        execute: Rc::new(move |_tool_call_id, params| {
            let value = params
                .get("value")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            executed_ref.borrow_mut().push(value.clone());
            Ok(AgentToolResult {
                content: vec![ContentBlock::Text {
                    text: format!("ok:{value}"),
                    text_signature: None,
                }],
                details: json!({ "value": value }),
            })
        }),
    };

    let context = AgentContext {
        system_prompt: String::new(),
        messages: Vec::new(),
        tools: vec![tool],
        cwd: None,
    };

    let user_prompt = create_user_message("start");
    let queued_message = create_user_message("interrupt");

    let call_index = Rc::new(Cell::new(0));
    let saw_interrupt_in_context = Rc::new(Cell::new(false));
    let executed_for_steering = executed.clone();
    let queued_message_for_steering = queued_message.clone();
    let queued_delivered = Rc::new(Cell::new(false));
    let queued_delivered_ref = queued_delivered.clone();
    let config = AgentLoopConfig {
        model: create_model(),
        convert_to_llm: Box::new(identity_converter),
        transform_context: None,
        get_steering_messages: Some(Box::new(move || {
            if !queued_delivered_ref.get() && executed_for_steering.borrow().len() == 1 {
                queued_delivered_ref.set(true);
                vec![queued_message_for_steering.clone()]
            } else {
                Vec::new()
            }
        })),
        get_follow_up_messages: None,
        on_approval: None,
    };

    let call_index_ref = call_index.clone();
    let saw_interrupt_ref = saw_interrupt_in_context.clone();
    let mut stream_fn: Box<pi::agent::StreamFn> = Box::new(
        move |_model: &Model, ctx: &LlmContext, _events: &mut pi::agent::StreamEvents| {
            let index = call_index_ref.get();
            if index == 1 {
                let saw_interrupt = ctx
                    .messages
                    .iter()
                    .any(|message| message.user_text() == Some("interrupt"));
                saw_interrupt_ref.set(saw_interrupt);
            }
            let message = if index == 0 {
                create_assistant_message(
                    vec![
                        ContentBlock::ToolCall {
                            id: "tool-1".to_string(),
                            name: "echo".to_string(),
                            arguments: json!({ "value": "first" }),
                            thought_signature: None,
                        },
                        ContentBlock::ToolCall {
                            id: "tool-2".to_string(),
                            name: "echo".to_string(),
                            arguments: json!({ "value": "second" }),
                            thought_signature: None,
                        },
                    ],
                    "toolUse",
                )
            } else {
                create_assistant_message(
                    vec![ContentBlock::Text {
                        text: "done".to_string(),
                        text_signature: None,
                    }],
                    "stop",
                )
            };
            call_index_ref.set(index + 1);
            message
        },
    );

    let stream = agent_loop(vec![user_prompt], context, config, &mut stream_fn);

    assert_eq!(executed.borrow().as_slice(), ["first"]);

    let tool_ends: Vec<&AgentEvent> = stream
        .events()
        .iter()
        .filter(|event| matches!(event, AgentEvent::ToolExecutionEnd { .. }))
        .collect();
    assert_eq!(tool_ends.len(), 2);
    if let AgentEvent::ToolExecutionEnd { is_error, .. } = tool_ends[0] {
        assert!(!is_error)
    }
    if let AgentEvent::ToolExecutionEnd {
        is_error, result, ..
    } = tool_ends[1]
    {
        assert!(is_error);
        if let Some(ContentBlock::Text { text, .. }) = result.content.first() {
            assert!(text.contains("Skipped due to queued user message"));
        }
    }

    let queued_message_event = stream.events().iter().find(|event| {
        matches!(event, AgentEvent::MessageStart { message } if message.user_text() == Some("interrupt"))
    });
    assert!(queued_message_event.is_some());
    assert!(saw_interrupt_in_context.get());
}

#[test]
fn should_throw_when_context_has_no_messages() {
    let context = AgentContext {
        system_prompt: "You are helpful.".to_string(),
        messages: Vec::new(),
        tools: Vec::new(),
        cwd: None,
    };

    let config = AgentLoopConfig {
        model: create_model(),
        convert_to_llm: Box::new(identity_converter),
        transform_context: None,
        get_steering_messages: None,
        get_follow_up_messages: None,
        on_approval: None,
    };

    let mut stream_fn: Box<pi::agent::StreamFn> =
        Box::new(|_model: &Model, _ctx: &LlmContext, _events| {
            create_assistant_message(
                vec![ContentBlock::Text {
                    text: "Response".to_string(),
                    text_signature: None,
                }],
                "stop",
            )
        });

    let err = agent_loop_continue(context, config, &mut stream_fn).unwrap_err();
    assert_eq!(err.to_string(), "Cannot continue: no messages in context");
}

#[test]
fn should_continue_from_existing_context_without_emitting_user_message_events() {
    let user_message = create_user_message("Hello");
    let context = AgentContext {
        system_prompt: "You are helpful.".to_string(),
        messages: vec![user_message],
        tools: Vec::new(),
        cwd: None,
    };

    let config = AgentLoopConfig {
        model: create_model(),
        convert_to_llm: Box::new(identity_converter),
        transform_context: None,
        get_steering_messages: None,
        get_follow_up_messages: None,
        on_approval: None,
    };

    let mut stream_fn: Box<pi::agent::StreamFn> =
        Box::new(|_model: &Model, _ctx: &LlmContext, _events| {
            create_assistant_message(
                vec![ContentBlock::Text {
                    text: "Response".to_string(),
                    text_signature: None,
                }],
                "stop",
            )
        });

    let stream = agent_loop_continue(context, config, &mut stream_fn).unwrap();
    let messages = stream.result().to_vec();

    assert_eq!(messages.len(), 1);
    assert!(matches!(messages[0], AgentMessage::Assistant(_)));

    let message_end_events: Vec<&AgentEvent> = stream
        .events()
        .iter()
        .filter(|event| matches!(event, AgentEvent::MessageEnd { .. }))
        .collect();
    assert_eq!(message_end_events.len(), 1);
    if let AgentEvent::MessageEnd { message } = message_end_events[0] {
        assert!(matches!(message, AgentMessage::Assistant(_)));
    }
}

#[test]
fn should_allow_custom_message_types_as_last_message_caller_responsibility() {
    let hook_message = AgentMessage::Custom(CustomMessage {
        role: "hookMessage".to_string(),
        text: "Hook content".to_string(),
        timestamp: now_millis(),
    });

    let context = AgentContext {
        system_prompt: "You are helpful.".to_string(),
        messages: vec![hook_message],
        tools: Vec::new(),
        cwd: None,
    };

    let config = AgentLoopConfig {
        model: create_model(),
        convert_to_llm: Box::new(|messages| {
            messages
                .iter()
                .cloned()
                .flat_map(|message| match message {
                    AgentMessage::Custom(custom) if custom.role == "hookMessage" => {
                        vec![AgentMessage::User(UserMessage {
                            content: UserContent::Text(custom.text),
                            timestamp: custom.timestamp,
                        })]
                    }
                    other => vec![other],
                })
                .filter(|message| {
                    matches!(
                        message,
                        AgentMessage::User(_)
                            | AgentMessage::Assistant(_)
                            | AgentMessage::ToolResult(_)
                    )
                })
                .collect()
        }),
        transform_context: None,
        get_steering_messages: None,
        get_follow_up_messages: None,
        on_approval: None,
    };

    let mut stream_fn: Box<pi::agent::StreamFn> =
        Box::new(|_model: &Model, _ctx: &LlmContext, _events| {
            create_assistant_message(
                vec![ContentBlock::Text {
                    text: "Response to hook".to_string(),
                    text_signature: None,
                }],
                "stop",
            )
        });

    let stream = agent_loop_continue(context, config, &mut stream_fn).unwrap();
    let messages = stream.result().to_vec();

    assert_eq!(messages.len(), 1);
    assert!(matches!(messages[0], AgentMessage::Assistant(_)));
}

fn create_usage() -> Usage {
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

fn create_model() -> Model {
    Model {
        id: "mock".to_string(),
        name: "mock".to_string(),
        api: "openai-responses".to_string(),
        provider: "openai".to_string(),
    }
}

fn create_assistant_message(content: Vec<ContentBlock>, stop_reason: &str) -> AssistantMessage {
    AssistantMessage {
        content,
        api: "openai-responses".to_string(),
        provider: "openai".to_string(),
        model: "mock".to_string(),
        usage: create_usage(),
        stop_reason: stop_reason.to_string(),
        error_message: None,
        timestamp: now_millis(),
    }
}

fn create_user_message(text: &str) -> AgentMessage {
    AgentMessage::User(UserMessage {
        content: UserContent::Text(text.to_string()),
        timestamp: now_millis(),
    })
}

fn identity_converter(messages: &[AgentMessage]) -> Vec<AgentMessage> {
    messages
        .iter()
        .filter(|&message| {
            matches!(
                message,
                AgentMessage::User(_) | AgentMessage::Assistant(_) | AgentMessage::ToolResult(_)
            )
        })
        .cloned()
        .collect()
}

fn now_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

// ============================================================================
// Approval hook tests
// ============================================================================

use pi::agent::{ApprovalRequest, ApprovalResponse};

#[test]
fn should_call_approval_callback_for_tool_calls() {
    use std::cell::Cell;

    let approval_called = Rc::new(Cell::new(false));
    let approval_called_ref = approval_called.clone();

    let tool = AgentTool {
        name: "test_tool".to_string(),
        label: "Test Tool".to_string(),
        description: "A test tool".to_string(),
        execute: Rc::new(|_id, _args| {
            Ok(AgentToolResult {
                content: vec![ContentBlock::Text {
                    text: "Tool executed".to_string(),
                    text_signature: None,
                }],
                details: serde_json::Value::Null,
            })
        }),
    };

    let context = AgentContext {
        system_prompt: "You are helpful.".to_string(),
        messages: Vec::new(),
        tools: vec![tool],
        cwd: None,
    };

    let user_prompt = create_user_message("Call the test tool");

    // Create approval callback that records it was called
    let approval_fn: Box<pi::agent::ApprovalFn> =
        Box::new(move |request: &ApprovalRequest| -> ApprovalResponse {
            approval_called_ref.set(true);
            assert_eq!(request.tool_name, "test_tool");
            ApprovalResponse::Approve
        });

    let config = AgentLoopConfig {
        model: create_model(),
        convert_to_llm: Box::new(identity_converter),
        transform_context: None,
        get_steering_messages: None,
        get_follow_up_messages: None,
        on_approval: Some(approval_fn),
    };

    // Stream function that returns a tool call on first invocation, then done
    let call_index = Rc::new(Cell::new(0));
    let call_index_ref = call_index.clone();
    let mut stream_fn: Box<pi::agent::StreamFn> =
        Box::new(move |_model: &Model, _ctx: &LlmContext, _events| {
            let index = call_index_ref.get();
            let message = if index == 0 {
                create_assistant_message(
                    vec![ContentBlock::ToolCall {
                        id: "call_1".to_string(),
                        name: "test_tool".to_string(),
                        arguments: json!({}),
                        thought_signature: None,
                    }],
                    "toolUse",
                )
            } else {
                create_assistant_message(
                    vec![ContentBlock::Text {
                        text: "Done".to_string(),
                        text_signature: None,
                    }],
                    "stop",
                )
            };
            call_index_ref.set(index + 1);
            message
        });

    let _stream = agent_loop(vec![user_prompt], context, config, &mut stream_fn);

    assert!(
        approval_called.get(),
        "Approval callback should have been called"
    );
}

#[test]
fn should_deny_tool_call_when_approval_denied() {
    use std::cell::Cell;

    let tool_executed = Rc::new(Cell::new(false));
    let tool_executed_ref = tool_executed.clone();

    let tool = AgentTool {
        name: "denied_tool".to_string(),
        label: "Denied Tool".to_string(),
        description: "A tool that will be denied".to_string(),
        execute: Rc::new(move |_id, _args| {
            tool_executed_ref.set(true);
            Ok(AgentToolResult {
                content: vec![ContentBlock::Text {
                    text: "Tool executed".to_string(),
                    text_signature: None,
                }],
                details: serde_json::Value::Null,
            })
        }),
    };

    let context = AgentContext {
        system_prompt: "You are helpful.".to_string(),
        messages: Vec::new(),
        tools: vec![tool],
        cwd: None,
    };

    let user_prompt = create_user_message("Call the denied tool");

    // Create approval callback that denies
    let approval_fn: Box<pi::agent::ApprovalFn> =
        Box::new(|_request: &ApprovalRequest| -> ApprovalResponse { ApprovalResponse::Deny });

    let config = AgentLoopConfig {
        model: create_model(),
        convert_to_llm: Box::new(identity_converter),
        transform_context: None,
        get_steering_messages: None,
        get_follow_up_messages: None,
        on_approval: Some(approval_fn),
    };

    // Stream function that returns a tool call on first invocation, then done
    let call_index = Rc::new(Cell::new(0));
    let call_index_ref = call_index.clone();
    let mut stream_fn: Box<pi::agent::StreamFn> =
        Box::new(move |_model: &Model, _ctx: &LlmContext, _events| {
            let index = call_index_ref.get();
            let message = if index == 0 {
                create_assistant_message(
                    vec![ContentBlock::ToolCall {
                        id: "call_1".to_string(),
                        name: "denied_tool".to_string(),
                        arguments: json!({}),
                        thought_signature: None,
                    }],
                    "toolUse",
                )
            } else {
                create_assistant_message(
                    vec![ContentBlock::Text {
                        text: "Done".to_string(),
                        text_signature: None,
                    }],
                    "stop",
                )
            };
            call_index_ref.set(index + 1);
            message
        });

    let stream = agent_loop(vec![user_prompt], context, config, &mut stream_fn);

    // Tool should NOT have been executed
    assert!(
        !tool_executed.get(),
        "Tool should not have been executed when denied"
    );

    // Should have a tool result with denied message
    let events = stream.events();
    let denied_event = events.iter().find(|e| {
        if let AgentEvent::ToolExecutionEnd { is_error, .. } = e {
            *is_error
        } else {
            false
        }
    });
    assert!(denied_event.is_some(), "Should have a denied tool result");
}

#[test]
fn should_remember_session_approved_tools() {
    use std::cell::Cell;

    let approval_count = Rc::new(Cell::new(0));
    let approval_count_ref = approval_count.clone();

    let tool = AgentTool {
        name: "session_tool".to_string(),
        label: "Session Tool".to_string(),
        description: "A tool that will be approved for session".to_string(),
        execute: Rc::new(|_id, _args| {
            Ok(AgentToolResult {
                content: vec![ContentBlock::Text {
                    text: "Tool executed".to_string(),
                    text_signature: None,
                }],
                details: serde_json::Value::Null,
            })
        }),
    };

    let context = AgentContext {
        system_prompt: "You are helpful.".to_string(),
        messages: Vec::new(),
        tools: vec![tool],
        cwd: None,
    };

    let user_prompt = create_user_message("Call the session tool twice");

    // Create approval callback that returns ApproveSession on first call
    let approval_fn: Box<pi::agent::ApprovalFn> =
        Box::new(move |_request: &ApprovalRequest| -> ApprovalResponse {
            let count = approval_count_ref.get();
            approval_count_ref.set(count + 1);
            ApprovalResponse::ApproveSession
        });

    let config = AgentLoopConfig {
        model: create_model(),
        convert_to_llm: Box::new(identity_converter),
        transform_context: None,
        get_steering_messages: None,
        get_follow_up_messages: None,
        on_approval: Some(approval_fn),
    };

    // Stream function that returns TWO tool calls on first invocation, then done
    let call_index = Rc::new(Cell::new(0));
    let call_index_ref = call_index.clone();
    let mut stream_fn: Box<pi::agent::StreamFn> =
        Box::new(move |_model: &Model, _ctx: &LlmContext, _events| {
            let index = call_index_ref.get();
            let message = if index == 0 {
                create_assistant_message(
                    vec![
                        ContentBlock::ToolCall {
                            id: "call_1".to_string(),
                            name: "session_tool".to_string(),
                            arguments: json!({}),
                            thought_signature: None,
                        },
                        ContentBlock::ToolCall {
                            id: "call_2".to_string(),
                            name: "session_tool".to_string(),
                            arguments: json!({}),
                            thought_signature: None,
                        },
                    ],
                    "toolUse",
                )
            } else {
                create_assistant_message(
                    vec![ContentBlock::Text {
                        text: "Done".to_string(),
                        text_signature: None,
                    }],
                    "stop",
                )
            };
            call_index_ref.set(index + 1);
            message
        });

    let _stream = agent_loop(vec![user_prompt], context, config, &mut stream_fn);

    // Approval should only be called ONCE because ApproveSession was returned
    assert_eq!(
        approval_count.get(),
        1,
        "Approval should only be called once when ApproveSession is returned"
    );
}
