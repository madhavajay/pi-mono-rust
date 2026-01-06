use pi::agent::{get_model, Agent, AgentOptions, AgentStateOverride, LlmContext, Model};
use pi::coding_agent::{
    AgentSession, AgentSessionConfig, AuthStorage, CompactionHook, CompactionResult, ModelRegistry,
    SessionBeforeCompactEvent, SessionBeforeCompactResult, SessionCompactEvent, SettingsManager,
    SettingsOverrides,
};
use pi::core::messages::{AssistantMessage, ContentBlock, Cost, Usage};
use pi::core::session_manager::{SessionEntry, SessionManager};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

// Source: packages/coding-agent/test/compaction-hooks.test.ts

type StreamFn = Box<dyn FnMut(&Model, &LlmContext) -> AssistantMessage>;

fn make_assistant_message(text: &str) -> AssistantMessage {
    AssistantMessage {
        content: vec![ContentBlock::Text {
            text: text.to_string(),
            text_signature: None,
        }],
        api: "anthropic-messages".to_string(),
        provider: "anthropic".to_string(),
        model: "mock".to_string(),
        usage: Usage {
            input: 10,
            output: 5,
            cache_read: 0,
            cache_write: 0,
            total_tokens: Some(15),
            cost: Some(Cost {
                input: 0.0,
                output: 0.0,
                cache_read: 0.0,
                cache_write: 0.0,
                total: 0.0,
            }),
        },
        stop_reason: "stop".to_string(),
        error_message: None,
        timestamp: 0,
    }
}

fn create_session() -> AgentSession {
    let model = get_model("anthropic", "claude-sonnet-4-5");
    let stream_fn: StreamFn = Box::new(move |_model, _context| make_assistant_message("ok"));

    let agent = Agent::new(AgentOptions {
        initial_state: Some(AgentStateOverride {
            model: Some(model),
            system_prompt: Some("Test".to_string()),
            tools: Some(Vec::new()),
            ..Default::default()
        }),
        stream_fn: Some(stream_fn),
        ..Default::default()
    });

    let session_manager = SessionManager::in_memory();
    let mut settings_manager = SettingsManager::create("", "");
    settings_manager.apply_overrides(SettingsOverrides {
        compaction: Some(pi::coding_agent::CompactionOverrides {
            enabled: Some(true),
            reserve_tokens: None,
            keep_recent_tokens: Some(1),
        }),
    });
    let mut auth_storage = AuthStorage::new(PathBuf::from("auth.json"));
    auth_storage.set_runtime_api_key("anthropic", "test-key");
    let model_registry = ModelRegistry::new(auth_storage, None);

    AgentSession::new(AgentSessionConfig {
        agent,
        session_manager,
        settings_manager,
        model_registry,
    })
}

#[test]
fn should_emit_before_compact_and_compact_events() {
    let mut session = create_session();

    let before_events = Rc::new(RefCell::new(Vec::new()));
    let after_events = Rc::new(RefCell::new(Vec::new()));
    let before_ref = before_events.clone();
    let after_ref = after_events.clone();

    let hook = CompactionHook::new(
        Some(Box::new(move |event: &SessionBeforeCompactEvent| {
            before_ref.borrow_mut().push(event.clone());
            SessionBeforeCompactResult::default()
        })),
        Some(Box::new(move |event: &SessionCompactEvent| {
            after_ref.borrow_mut().push(event.clone());
        })),
    );
    session.set_compaction_hooks(vec![hook]);

    session.prompt("What is 2+2?").unwrap();
    session.prompt("What is 3+3?").unwrap();
    session.compact().unwrap();

    assert_eq!(before_events.borrow().len(), 1);
    assert_eq!(after_events.borrow().len(), 1);

    let before_event = before_events.borrow()[0].clone();
    assert!(!before_event.preparation.messages_to_summarize.is_empty());
    assert!(before_event.preparation.tokens_before >= 0);
    assert!(!before_event.branch_entries.is_empty());

    let after_event = after_events.borrow()[0].clone();
    assert!(!after_event.compaction_entry.summary.is_empty());
    assert!(after_event.compaction_entry.tokens_before >= 0);
    assert!(!after_event.from_hook);
}

#[test]
fn should_allow_hooks_to_cancel_compaction() {
    let mut session = create_session();

    let hook = CompactionHook::new(
        Some(Box::new(|_event| SessionBeforeCompactResult {
            cancel: Some(true),
            compaction: None,
        })),
        None,
    );
    session.set_compaction_hooks(vec![hook]);

    session.prompt("What is 2+2?").unwrap();

    let err = session.compact().unwrap_err();
    assert!(err.to_string().contains("Compaction cancelled"));
}

#[test]
fn should_allow_hooks_to_provide_custom_compaction() {
    let mut session = create_session();

    let custom_summary = "Custom summary from hook".to_string();
    let hook = CompactionHook::new(
        Some(Box::new(move |event| SessionBeforeCompactResult {
            cancel: None,
            compaction: Some(CompactionResult {
                summary: custom_summary.clone(),
                first_kept_entry_id: event.preparation.first_kept_entry_id.clone(),
                tokens_before: event.preparation.tokens_before,
            }),
        })),
        None,
    );
    session.set_compaction_hooks(vec![hook]);

    session.prompt("What is 2+2?").unwrap();
    session.prompt("What is 3+3?").unwrap();

    let result = session.compact().unwrap();
    assert_eq!(result.summary, "Custom summary from hook");
}

#[test]
fn should_include_entries_in_compact_event_after_compaction_is_saved() {
    let mut session = create_session();

    let after_events = Rc::new(RefCell::new(Vec::new()));
    let after_ref = after_events.clone();
    let hook = CompactionHook::new(
        None,
        Some(Box::new(move |event| {
            after_ref.borrow_mut().push(event.clone());
        })),
    );
    session.set_compaction_hooks(vec![hook]);

    session.prompt("What is 2+2?").unwrap();
    session.compact().unwrap();

    assert_eq!(after_events.borrow().len(), 1);
    let entries = session.session_manager.get_entries();
    let has_compaction = entries
        .iter()
        .any(|entry| matches!(entry, SessionEntry::Compaction(_)));
    assert!(has_compaction);
}

#[test]
fn should_continue_with_default_compaction_if_hook_throws_error() {
    let mut session = create_session();

    let hook = CompactionHook::new(
        Some(Box::new(|_event| {
            panic!("Hook intentionally throws");
        })),
        None,
    );
    session.set_compaction_hooks(vec![hook]);

    session.prompt("What is 2+2?").unwrap();
    let result = session.compact().unwrap();
    assert!(!result.summary.is_empty());
}

#[test]
fn should_call_multiple_hooks_in_order() {
    let mut session = create_session();

    let call_order = Rc::new(RefCell::new(Vec::new()));
    let call_order_ref = call_order.clone();
    let hook1 = CompactionHook::new(
        Some(Box::new(move |_event| {
            call_order_ref.borrow_mut().push("hook1-before".to_string());
            SessionBeforeCompactResult::default()
        })),
        Some(Box::new({
            let call_order_ref = call_order.clone();
            move |_event| {
                call_order_ref.borrow_mut().push("hook1-after".to_string());
            }
        })),
    );

    let call_order_ref = call_order.clone();
    let hook2 = CompactionHook::new(
        Some(Box::new(move |_event| {
            call_order_ref.borrow_mut().push("hook2-before".to_string());
            SessionBeforeCompactResult::default()
        })),
        Some(Box::new({
            let call_order_ref = call_order.clone();
            move |_event| {
                call_order_ref.borrow_mut().push("hook2-after".to_string());
            }
        })),
    );

    session.set_compaction_hooks(vec![hook1, hook2]);

    session.prompt("What is 2+2?").unwrap();
    session.compact().unwrap();

    assert_eq!(
        *call_order.borrow(),
        vec!["hook1-before", "hook2-before", "hook1-after", "hook2-after"]
    );
}

#[test]
fn should_pass_correct_data_in_before_compact_event() {
    let mut session = create_session();

    let captured_event = Rc::new(RefCell::new(None));
    let captured_ref = captured_event.clone();
    let hook = CompactionHook::new(
        Some(Box::new(move |event| {
            *captured_ref.borrow_mut() = Some(event.clone());
            SessionBeforeCompactResult::default()
        })),
        None,
    );
    session.set_compaction_hooks(vec![hook]);

    session.prompt("What is 2+2?").unwrap();
    session.prompt("What is 3+3?").unwrap();
    session.compact().unwrap();

    let event = captured_event.borrow().clone().expect("event");
    assert!(!event.preparation.first_kept_entry_id.is_empty());
    assert!(event.preparation.tokens_before >= 0);
    assert!(!event.branch_entries.is_empty());
}

#[test]
fn should_use_hook_compaction_even_with_different_values() {
    let mut session = create_session();

    let custom_summary = "Custom summary with modified values".to_string();
    let hook = CompactionHook::new(
        Some(Box::new(move |event| SessionBeforeCompactResult {
            cancel: None,
            compaction: Some(CompactionResult {
                summary: custom_summary.clone(),
                first_kept_entry_id: event.preparation.first_kept_entry_id.clone(),
                tokens_before: 999,
            }),
        })),
        None,
    );
    session.set_compaction_hooks(vec![hook]);

    session.prompt("What is 2+2?").unwrap();
    let result = session.compact().unwrap();
    assert_eq!(result.summary, "Custom summary with modified values");
    assert_eq!(result.tokens_before, 999);
}
