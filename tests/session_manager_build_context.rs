use pi::{
    build_session_context, AgentMessage, BranchSummaryEntry, CompactionEntry, ContentBlock, Cost,
    ModelChangeEntry, SessionEntry, SessionMessageEntry, ThinkingLevelChangeEntry, Usage,
    UserContent, UserMessage,
};

fn msg(id: &str, parent_id: Option<&str>, role: &str, text: &str) -> SessionEntry {
    let timestamp = "2025-01-01T00:00:00Z".to_string();
    let message = match role {
        "user" => AgentMessage::User(UserMessage {
            content: UserContent::Text(text.to_string()),
            timestamp: 1,
        }),
        "assistant" => AgentMessage::Assistant(pi::AssistantMessage {
            content: vec![ContentBlock::Text {
                text: text.to_string(),
                text_signature: None,
            }],
            api: "anthropic-messages".to_string(),
            provider: "anthropic".to_string(),
            model: "claude-test".to_string(),
            usage: Usage {
                input: 1,
                output: 1,
                cache_read: 0,
                cache_write: 0,
                total_tokens: Some(2),
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
            timestamp: 1,
        }),
        _ => panic!("unknown role"),
    };
    SessionEntry::Message(SessionMessageEntry {
        id: id.to_string(),
        parent_id: parent_id.map(|value| value.to_string()),
        timestamp,
        message,
    })
}

fn compaction(
    id: &str,
    parent_id: Option<&str>,
    summary: &str,
    first_kept_entry_id: &str,
) -> SessionEntry {
    SessionEntry::Compaction(CompactionEntry {
        id: id.to_string(),
        parent_id: parent_id.map(|value| value.to_string()),
        timestamp: "2025-01-01T00:00:00Z".to_string(),
        summary: summary.to_string(),
        first_kept_entry_id: first_kept_entry_id.to_string(),
        first_kept_entry_index: None,
        tokens_before: 1000,
        details: None,
        from_hook: None,
    })
}

fn branch_summary(id: &str, parent_id: Option<&str>, summary: &str, from_id: &str) -> SessionEntry {
    SessionEntry::BranchSummary(BranchSummaryEntry {
        id: id.to_string(),
        parent_id: parent_id.map(|value| value.to_string()),
        timestamp: "2025-01-01T00:00:00Z".to_string(),
        summary: summary.to_string(),
        from_id: from_id.to_string(),
        details: None,
        from_hook: None,
    })
}

fn thinking_level(id: &str, parent_id: Option<&str>, level: &str) -> SessionEntry {
    SessionEntry::ThinkingLevelChange(ThinkingLevelChangeEntry {
        id: id.to_string(),
        parent_id: parent_id.map(|value| value.to_string()),
        timestamp: "2025-01-01T00:00:00Z".to_string(),
        thinking_level: level.to_string(),
    })
}

fn model_change(id: &str, parent_id: Option<&str>, provider: &str, model_id: &str) -> SessionEntry {
    SessionEntry::ModelChange(ModelChangeEntry {
        id: id.to_string(),
        parent_id: parent_id.map(|value| value.to_string()),
        timestamp: "2025-01-01T00:00:00Z".to_string(),
        provider: provider.to_string(),
        model_id: model_id.to_string(),
    })
}

#[test]
fn empty_entries_returns_empty_context() {
    let ctx = build_session_context(&[], None);
    assert!(ctx.messages.is_empty());
    assert_eq!(ctx.thinking_level, "off");
    assert!(ctx.model.is_none());
}

#[test]
fn single_user_message() {
    let entries = vec![msg("1", None, "user", "hello")];
    let ctx = build_session_context(&entries, None);
    assert_eq!(ctx.messages.len(), 1);
    match &ctx.messages[0] {
        AgentMessage::User(_) => {}
        _ => panic!("expected user message"),
    }
}

#[test]
fn simple_conversation() {
    let entries = vec![
        msg("1", None, "user", "hello"),
        msg("2", Some("1"), "assistant", "hi there"),
        msg("3", Some("2"), "user", "how are you"),
        msg("4", Some("3"), "assistant", "great"),
    ];
    let ctx = build_session_context(&entries, None);
    assert_eq!(ctx.messages.len(), 4);
}

#[test]
fn tracks_thinking_level_changes() {
    let entries = vec![
        msg("1", None, "user", "hello"),
        thinking_level("2", Some("1"), "high"),
        msg("3", Some("2"), "assistant", "thinking hard"),
    ];
    let ctx = build_session_context(&entries, None);
    assert_eq!(ctx.thinking_level, "high");
    assert_eq!(ctx.messages.len(), 2);
}

#[test]
fn tracks_model_from_assistant_message() {
    let entries = vec![
        msg("1", None, "user", "hello"),
        msg("2", Some("1"), "assistant", "hi"),
    ];
    let ctx = build_session_context(&entries, None);
    let model = ctx.model.expect("model should be set");
    assert_eq!(model.provider, "anthropic");
    assert_eq!(model.model_id, "claude-test");
}

#[test]
fn tracks_model_from_model_change_entry() {
    let entries = vec![
        msg("1", None, "user", "hello"),
        model_change("2", Some("1"), "openai", "gpt-4"),
        msg("3", Some("2"), "assistant", "hi"),
    ];
    let ctx = build_session_context(&entries, None);
    let model = ctx.model.expect("model should be set");
    assert_eq!(model.provider, "anthropic");
    assert_eq!(model.model_id, "claude-test");
}

#[test]
fn includes_summary_before_kept_messages() {
    let entries = vec![
        msg("1", None, "user", "first"),
        msg("2", Some("1"), "assistant", "response1"),
        msg("3", Some("2"), "user", "second"),
        msg("4", Some("3"), "assistant", "response2"),
        compaction("5", Some("4"), "Summary of first two turns", "3"),
        msg("6", Some("5"), "user", "third"),
        msg("7", Some("6"), "assistant", "response3"),
    ];
    let ctx = build_session_context(&entries, None);
    assert_eq!(ctx.messages.len(), 5);
    match &ctx.messages[0] {
        AgentMessage::CompactionSummary(msg) => {
            assert!(msg.summary.contains("Summary of first two turns"))
        }
        _ => panic!("expected compaction summary"),
    }
}

#[test]
fn handles_compaction_keeping_from_first_message() {
    let entries = vec![
        msg("1", None, "user", "first"),
        msg("2", Some("1"), "assistant", "response"),
        compaction("3", Some("2"), "Empty summary", "1"),
        msg("4", Some("3"), "user", "second"),
    ];
    let ctx = build_session_context(&entries, None);
    assert_eq!(ctx.messages.len(), 4);
    match &ctx.messages[0] {
        AgentMessage::CompactionSummary(msg) => assert!(msg.summary.contains("Empty summary")),
        _ => panic!("expected compaction summary"),
    }
}

#[test]
fn multiple_compactions_uses_latest() {
    let entries = vec![
        msg("1", None, "user", "a"),
        msg("2", Some("1"), "assistant", "b"),
        compaction("3", Some("2"), "First summary", "1"),
        msg("4", Some("3"), "user", "c"),
        msg("5", Some("4"), "assistant", "d"),
        compaction("6", Some("5"), "Second summary", "4"),
        msg("7", Some("6"), "user", "e"),
    ];
    let ctx = build_session_context(&entries, None);
    assert_eq!(ctx.messages.len(), 4);
    match &ctx.messages[0] {
        AgentMessage::CompactionSummary(msg) => assert!(msg.summary.contains("Second summary")),
        _ => panic!("expected compaction summary"),
    }
}

#[test]
fn follows_path_to_specified_leaf() {
    let entries = vec![
        msg("1", None, "user", "start"),
        msg("2", Some("1"), "assistant", "response"),
        msg("3", Some("2"), "user", "branch A"),
        msg("4", Some("2"), "user", "branch B"),
    ];

    let ctx_a = build_session_context(&entries, Some("3"));
    assert_eq!(ctx_a.messages.len(), 3);

    let ctx_b = build_session_context(&entries, Some("4"));
    assert_eq!(ctx_b.messages.len(), 3);
}

#[test]
fn includes_branch_summary_in_path() {
    let entries = vec![
        msg("1", None, "user", "start"),
        msg("2", Some("1"), "assistant", "response"),
        msg("3", Some("2"), "user", "abandoned path"),
        branch_summary("4", Some("2"), "Summary of abandoned work", "3"),
        msg("5", Some("4"), "user", "new direction"),
    ];
    let ctx = build_session_context(&entries, Some("5"));
    assert_eq!(ctx.messages.len(), 4);
    match &ctx.messages[2] {
        AgentMessage::BranchSummary(msg) => {
            assert!(msg.summary.contains("Summary of abandoned work"))
        }
        _ => panic!("expected branch summary"),
    }
}

#[test]
fn complex_tree_with_multiple_branches_and_compaction() {
    let entries = vec![
        msg("1", None, "user", "start"),
        msg("2", Some("1"), "assistant", "r1"),
        msg("3", Some("2"), "user", "q2"),
        msg("4", Some("3"), "assistant", "r2"),
        compaction("5", Some("4"), "Compacted history", "3"),
        msg("6", Some("5"), "user", "q3"),
        msg("7", Some("6"), "assistant", "r3"),
        msg("8", Some("3"), "user", "wrong path"),
        msg("9", Some("8"), "assistant", "wrong response"),
        branch_summary("10", Some("3"), "Tried wrong approach", "9"),
        msg("11", Some("10"), "user", "better approach"),
    ];

    let ctx_main = build_session_context(&entries, Some("7"));
    assert_eq!(ctx_main.messages.len(), 5);
    match &ctx_main.messages[0] {
        AgentMessage::CompactionSummary(msg) => assert!(msg.summary.contains("Compacted history")),
        _ => panic!("expected compaction summary"),
    }

    let ctx_branch = build_session_context(&entries, Some("11"));
    assert_eq!(ctx_branch.messages.len(), 5);
    match &ctx_branch.messages[3] {
        AgentMessage::BranchSummary(msg) => assert!(msg.summary.contains("Tried wrong approach")),
        _ => panic!("expected branch summary"),
    }
}

#[test]
fn uses_last_entry_when_leaf_id_not_found() {
    let entries = vec![
        msg("1", None, "user", "hello"),
        msg("2", Some("1"), "assistant", "hi"),
    ];
    let ctx = build_session_context(&entries, Some("nonexistent"));
    assert_eq!(ctx.messages.len(), 2);
}

#[test]
fn handles_orphaned_entries_gracefully() {
    let entries = vec![
        msg("1", None, "user", "hello"),
        msg("2", Some("missing"), "assistant", "orphan"),
    ];
    let ctx = build_session_context(&entries, Some("2"));
    assert_eq!(ctx.messages.len(), 1);
}
