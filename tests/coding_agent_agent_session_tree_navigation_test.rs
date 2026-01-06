use pi::agent::{get_model, Agent, AgentOptions, AgentStateOverride};
use pi::coding_agent::{
    AgentSession, AgentSessionConfig, AuthStorage, ModelRegistry, NavigateTreeOptions,
    SettingsManager,
};
use pi::core::messages::{AssistantMessage, ContentBlock, Cost, Usage};
use pi::core::session_manager::{SessionEntry, SessionManager};
use std::path::PathBuf;

// Source: packages/coding-agent/test/agent-session-tree-navigation.test.ts

type StreamFn = Box<pi::agent::StreamFn>;

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
        },
        stop_reason: "stop".to_string(),
        error_message: None,
        timestamp: 0,
    }
}

fn create_session() -> AgentSession {
    let model = get_model("anthropic", "claude-sonnet-4-5");
    let stream_fn: StreamFn =
        Box::new(move |_model, _context, _events| make_assistant_message("ok"));

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
    let settings_manager = SettingsManager::create("", "");
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

fn entry_type(entry: &SessionEntry) -> &'static str {
    match entry {
        SessionEntry::Message(_) => "message",
        SessionEntry::BranchSummary(_) => "branch_summary",
        SessionEntry::Compaction(_) => "compaction",
        SessionEntry::ThinkingLevelChange(_) => "thinking_level_change",
        SessionEntry::ModelChange(_) => "model_change",
        SessionEntry::Custom(_) => "custom",
        SessionEntry::CustomMessage(_) => "custom_message",
        SessionEntry::Label(_) => "label",
    }
}

#[test]
fn should_navigate_to_user_message_and_put_text_in_editor() {
    let mut session = create_session();

    session.prompt("First message").unwrap();
    session.prompt("Second message").unwrap();

    let tree = session.session_manager.get_tree();
    assert_eq!(tree.len(), 1);

    let root_id = tree[0].entry.id().to_string();
    let result = session
        .navigate_tree(
            &root_id,
            NavigateTreeOptions {
                summarize: false,
                custom_instructions: None,
            },
        )
        .unwrap();

    assert_eq!(result.editor_text, Some("First message".to_string()));
    assert!(!result.cancelled);
    assert!(session.session_manager.get_leaf_id().is_none());
}

#[test]
fn should_navigate_to_non_user_message_without_editor_text() {
    let mut session = create_session();

    session.prompt("Hello").unwrap();

    let entries = session.session_manager.get_entries();
    let assistant_entry = entries.iter().find_map(|entry| match entry {
        SessionEntry::Message(message) => match &message.message {
            pi::core::messages::AgentMessage::Assistant(_) => Some(message),
            _ => None,
        },
        _ => None,
    });
    let assistant_entry = assistant_entry.expect("assistant entry");

    let result = session
        .navigate_tree(
            &assistant_entry.id,
            NavigateTreeOptions {
                summarize: false,
                custom_instructions: None,
            },
        )
        .unwrap();

    assert!(result.editor_text.is_none());
    assert!(!result.cancelled);
    assert_eq!(
        session.session_manager.get_leaf_id().as_deref(),
        Some(assistant_entry.id.as_str())
    );
}

#[test]
fn should_create_branch_summary_when_navigating_with_summarize_true() {
    let mut session = create_session();

    session.prompt("What is 2+2?").unwrap();
    session.prompt("What is 3+3?").unwrap();

    let tree = session.session_manager.get_tree();
    let root_id = tree[0].entry.id().to_string();

    let result = session
        .navigate_tree(
            &root_id,
            NavigateTreeOptions {
                summarize: true,
                custom_instructions: None,
            },
        )
        .unwrap();

    let summary = result.summary_entry.expect("summary entry");
    assert!(!summary.summary.is_empty());
    assert!(summary.parent_id.is_none());
    assert_eq!(
        session.session_manager.get_leaf_id().as_deref(),
        Some(summary.id.as_str())
    );
}

#[test]
fn should_attach_summary_to_correct_parent_when_navigating_to_nested_user_message() {
    let mut session = create_session();

    session.prompt("Message one").unwrap();
    session.prompt("Message two").unwrap();
    session.prompt("Message three").unwrap();

    let entries = session.session_manager.get_entries();
    let user_entries: Vec<_> = entries
        .iter()
        .filter_map(|entry| match entry {
            SessionEntry::Message(message) => match &message.message {
                pi::core::messages::AgentMessage::User(_) => Some(message),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(user_entries.len(), 3);

    let u2 = user_entries[1];
    let a1_id = u2.parent_id.as_deref().expect("parent id");

    let result = session
        .navigate_tree(
            &u2.id,
            NavigateTreeOptions {
                summarize: true,
                custom_instructions: None,
            },
        )
        .unwrap();

    let summary = result.summary_entry.expect("summary entry");
    assert_eq!(summary.parent_id.as_deref(), Some(a1_id));

    let children = session.session_manager.get_children(a1_id);
    assert_eq!(children.len(), 2);
    let child_types: Vec<_> = children.iter().map(entry_type).collect();
    assert!(child_types.contains(&"branch_summary"));
    assert!(child_types.contains(&"message"));
}

#[test]
fn should_attach_summary_to_selected_node_when_navigating_to_assistant_message() {
    let mut session = create_session();

    session.prompt("Hello").unwrap();
    session.prompt("Goodbye").unwrap();

    let entries = session.session_manager.get_entries();
    let assistant_entries: Vec<_> = entries
        .iter()
        .filter_map(|entry| match entry {
            SessionEntry::Message(message) => match &message.message {
                pi::core::messages::AgentMessage::Assistant(_) => Some(message),
                _ => None,
            },
            _ => None,
        })
        .collect();
    let a1 = assistant_entries[0];

    let result = session
        .navigate_tree(
            &a1.id,
            NavigateTreeOptions {
                summarize: true,
                custom_instructions: None,
            },
        )
        .unwrap();

    let summary = result.summary_entry.expect("summary entry");
    assert_eq!(summary.parent_id.as_deref(), Some(a1.id.as_str()));
    assert_eq!(
        session.session_manager.get_leaf_id().as_deref(),
        Some(summary.id.as_str())
    );
}

#[test]
fn should_handle_abort_during_summarization() {
    let mut session = create_session();

    session.prompt("Tell me about something").unwrap();
    session.prompt("Continue").unwrap();

    let entries_before = session.session_manager.get_entries().len();
    let leaf_before = session.session_manager.get_leaf_id();

    let tree = session.session_manager.get_tree();
    let root_id = tree[0].entry.id().to_string();

    session.abort_branch_summary();
    let result = session
        .navigate_tree(
            &root_id,
            NavigateTreeOptions {
                summarize: true,
                custom_instructions: None,
            },
        )
        .unwrap();

    assert!(result.cancelled);
    assert!(result.aborted);
    assert!(result.summary_entry.is_none());
    assert_eq!(session.session_manager.get_entries().len(), entries_before);
    assert_eq!(session.session_manager.get_leaf_id(), leaf_before);
}

#[test]
fn should_not_create_summary_when_navigating_without_summarize_option() {
    let mut session = create_session();

    session.prompt("First").unwrap();
    session.prompt("Second").unwrap();

    let entries_before = session.session_manager.get_entries().len();
    let tree = session.session_manager.get_tree();
    let root_id = tree[0].entry.id().to_string();

    session
        .navigate_tree(
            &root_id,
            NavigateTreeOptions {
                summarize: false,
                custom_instructions: None,
            },
        )
        .unwrap();

    let entries_after = session.session_manager.get_entries().len();
    assert_eq!(entries_before, entries_after);
    let entries = session.session_manager.get_entries();
    let summaries: Vec<_> = entries
        .iter()
        .filter(|entry| matches!(entry, SessionEntry::BranchSummary(_)))
        .collect();
    assert!(summaries.is_empty());
}

#[test]
fn should_handle_navigation_to_same_position_no_op() {
    let mut session = create_session();

    session.prompt("Hello").unwrap();

    let leaf_before = session.session_manager.get_leaf_id();
    let entries_before = session.session_manager.get_entries().len();

    let target_id = leaf_before.clone().expect("leaf id");
    let result = session
        .navigate_tree(
            &target_id,
            NavigateTreeOptions {
                summarize: false,
                custom_instructions: None,
            },
        )
        .unwrap();

    assert!(!result.cancelled);
    assert_eq!(session.session_manager.get_leaf_id(), leaf_before);
    assert_eq!(session.session_manager.get_entries().len(), entries_before);
}

#[test]
fn should_support_custom_summarization_instructions() {
    let mut session = create_session();

    session.prompt("What is TypeScript?").unwrap();

    let tree = session.session_manager.get_tree();
    let root_id = tree[0].entry.id().to_string();

    let result = session
        .navigate_tree(
            &root_id,
            NavigateTreeOptions {
                summarize: true,
                custom_instructions: Some("Summarize in exactly 3 words.".to_string()),
            },
        )
        .unwrap();

    let summary = result.summary_entry.expect("summary entry");
    assert!(!summary.summary.is_empty());
    assert!(summary.summary.split_whitespace().count() < 20);
}

#[test]
fn should_navigate_between_branches_correctly() {
    let mut session = create_session();

    session.prompt("Main branch start").unwrap();
    session.prompt("Main branch continue").unwrap();

    let entries_before_branch = session.session_manager.get_entries();
    let a1 = entries_before_branch
        .iter()
        .find_map(|entry| match entry {
            SessionEntry::Message(message) => match &message.message {
                pi::core::messages::AgentMessage::Assistant(_) => Some(message),
                _ => None,
            },
            _ => None,
        })
        .expect("assistant entry");
    let u2 = entries_before_branch
        .iter()
        .filter_map(|entry| match entry {
            SessionEntry::Message(message) => match &message.message {
                pi::core::messages::AgentMessage::User(_) => Some(message),
                _ => None,
            },
            _ => None,
        })
        .nth(1)
        .expect("second user entry");

    session.session_manager.branch(&a1.id).unwrap();
    session.prompt("Branch path").unwrap();

    let result = session
        .navigate_tree(
            &u2.id,
            NavigateTreeOptions {
                summarize: true,
                custom_instructions: None,
            },
        )
        .unwrap();

    let summary = result.summary_entry.expect("summary entry");
    assert!(!summary.summary.is_empty());
}
