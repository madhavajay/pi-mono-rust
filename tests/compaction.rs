use pi::{
    build_session_context, calculate_context_tokens, find_cut_point, get_last_assistant_usage,
    load_entries_from_file, migrate_session_entries, should_compact, AgentMessage,
    AssistantMessage, CompactionSettings, ContentBlock, Cost, SessionEntry, SessionMessageEntry,
    Usage, UserContent, UserMessage, DEFAULT_COMPACTION_SETTINGS,
};
use std::path::PathBuf;

fn create_mock_usage(input: i64, output: i64, cache_read: i64, cache_write: i64) -> Usage {
    Usage {
        input,
        output,
        cache_read,
        cache_write,
        total_tokens: Some(input + output + cache_read + cache_write),
        cost: Some(Cost {
            input: 0.0,
            output: 0.0,
            cache_read: 0.0,
            cache_write: 0.0,
            total: 0.0,
        }),
    }
}

fn create_user_message(text: &str) -> AgentMessage {
    AgentMessage::User(UserMessage {
        content: UserContent::Text(text.to_string()),
        timestamp: 1,
    })
}

fn create_assistant_message(text: &str, usage: Usage, stop_reason: &str) -> AgentMessage {
    AgentMessage::Assistant(AssistantMessage {
        content: vec![ContentBlock::Text {
            text: text.to_string(),
            text_signature: None,
        }],
        api: "anthropic-messages".to_string(),
        provider: "anthropic".to_string(),
        model: "claude-sonnet-4-5".to_string(),
        usage,
        stop_reason: stop_reason.to_string(),
        error_message: None,
        timestamp: 1,
    })
}

struct EntryBuilder {
    counter: usize,
    last_id: Option<String>,
}

impl EntryBuilder {
    fn new() -> Self {
        Self {
            counter: 0,
            last_id: None,
        }
    }

    fn message_entry(&mut self, message: AgentMessage) -> SessionEntry {
        let id = format!("test-id-{}", self.counter);
        self.counter += 1;
        let entry = SessionMessageEntry {
            id: id.clone(),
            parent_id: self.last_id.clone(),
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            message,
        };
        self.last_id = Some(id);
        SessionEntry::Message(entry)
    }
}

fn load_large_session_entries() -> Vec<SessionEntry> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let path = root.join("packages/coding-agent/test/fixtures/large-session.jsonl");
    let mut file_entries = load_entries_from_file(&path);
    migrate_session_entries(&mut file_entries);
    file_entries
        .into_iter()
        .filter_map(|entry| entry.as_session_entry())
        .collect()
}

#[test]
fn calculate_context_tokens_sums_usage() {
    let usage = create_mock_usage(1000, 500, 200, 100);
    assert_eq!(calculate_context_tokens(&usage), 1800);
}

#[test]
fn calculate_context_tokens_zero() {
    let usage = create_mock_usage(0, 0, 0, 0);
    assert_eq!(calculate_context_tokens(&usage), 0);
}

#[test]
fn last_assistant_usage_ignores_aborted() {
    let mut builder = EntryBuilder::new();
    let entries = vec![
        builder.message_entry(create_user_message("Hello")),
        builder.message_entry(create_assistant_message(
            "Hi",
            create_mock_usage(100, 50, 0, 0),
            "stop",
        )),
        builder.message_entry(create_user_message("How are you?")),
        builder.message_entry(create_assistant_message(
            "Aborted",
            create_mock_usage(300, 150, 0, 0),
            "aborted",
        )),
    ];

    let usage = get_last_assistant_usage(&entries).expect("expected usage");
    assert_eq!(usage.input, 100);
}

#[test]
fn last_assistant_usage_returns_latest() {
    let mut builder = EntryBuilder::new();
    let entries = vec![
        builder.message_entry(create_user_message("Hello")),
        builder.message_entry(create_assistant_message(
            "Hi",
            create_mock_usage(100, 50, 0, 0),
            "stop",
        )),
        builder.message_entry(create_user_message("How are you?")),
        builder.message_entry(create_assistant_message(
            "Good",
            create_mock_usage(200, 100, 0, 0),
            "stop",
        )),
    ];

    let usage = get_last_assistant_usage(&entries).expect("expected usage");
    assert_eq!(usage.input, 200);
}

#[test]
fn last_assistant_usage_none_when_missing() {
    let mut builder = EntryBuilder::new();
    let entries = vec![builder.message_entry(create_user_message("Hello"))];
    assert!(get_last_assistant_usage(&entries).is_none());
}

#[test]
fn should_compact_honors_settings() {
    let settings = CompactionSettings {
        enabled: true,
        reserve_tokens: 10_000,
        keep_recent_tokens: 20_000,
    };

    assert!(should_compact(95_000, 100_000, settings));
    assert!(!should_compact(89_000, 100_000, settings));
}

#[test]
fn should_compact_disabled() {
    let settings = CompactionSettings {
        enabled: false,
        reserve_tokens: 10_000,
        keep_recent_tokens: 20_000,
    };

    assert!(!should_compact(95_000, 100_000, settings));
}

#[test]
fn find_cut_point_returns_message() {
    let mut builder = EntryBuilder::new();
    let mut entries = Vec::new();
    for i in 0..10 {
        entries.push(builder.message_entry(create_user_message(&format!("User {i}"))));
        entries.push(builder.message_entry(create_assistant_message(
            &format!("Assistant {i}"),
            create_mock_usage(0, 100, (i + 1) * 1000, 0),
            "stop",
        )));
    }

    let result = find_cut_point(&entries, 0, entries.len(), 2500);
    let entry = &entries[result.first_kept_entry_index];
    match entry {
        SessionEntry::Message(message) => match message.message {
            AgentMessage::User(_) | AgentMessage::Assistant(_) => {}
            _ => panic!("expected user or assistant message"),
        },
        _ => panic!("expected message entry"),
    }
}

#[test]
fn find_cut_point_returns_start_when_no_cut_points() {
    let mut builder = EntryBuilder::new();
    let entries = vec![builder.message_entry(create_assistant_message(
        "Hello",
        create_mock_usage(1, 1, 0, 0),
        "stop",
    ))];
    let result = find_cut_point(&entries, 0, entries.len(), 1000);
    assert_eq!(result.first_kept_entry_index, 0);
}

#[test]
fn find_cut_point_keeps_all_when_under_budget() {
    let mut builder = EntryBuilder::new();
    let entries = vec![
        builder.message_entry(create_user_message("1")),
        builder.message_entry(create_assistant_message(
            "a",
            create_mock_usage(0, 50, 500, 0),
            "stop",
        )),
        builder.message_entry(create_user_message("2")),
        builder.message_entry(create_assistant_message(
            "b",
            create_mock_usage(0, 50, 1000, 0),
            "stop",
        )),
    ];

    let result = find_cut_point(&entries, 0, entries.len(), 50_000);
    assert_eq!(result.first_kept_entry_index, 0);
}

#[test]
fn find_cut_point_marks_split_turn() {
    let mut builder = EntryBuilder::new();
    let long_text = "a".repeat(4800);
    let entries = vec![
        builder.message_entry(create_user_message("Turn 1")),
        builder.message_entry(create_assistant_message(
            "A1",
            create_mock_usage(0, 100, 1000, 0),
            "stop",
        )),
        builder.message_entry(create_user_message("Turn 2")),
        builder.message_entry(create_assistant_message(
            &long_text,
            create_mock_usage(0, 100, 0, 0),
            "stop",
        )),
        builder.message_entry(create_assistant_message(
            &long_text,
            create_mock_usage(0, 100, 0, 0),
            "stop",
        )),
        builder.message_entry(create_assistant_message(
            &long_text,
            create_mock_usage(0, 100, 0, 0),
            "stop",
        )),
    ];

    let result = find_cut_point(&entries, 0, entries.len(), 3000);
    let cut_entry = &entries[result.first_kept_entry_index];
    if let SessionEntry::Message(message) = cut_entry {
        if matches!(message.message, AgentMessage::Assistant(_)) {
            assert!(result.is_split_turn);
            assert_eq!(result.turn_start_index, Some(2));
        }
    }
}

#[test]
fn large_session_parses_and_builds_context() {
    let entries = load_large_session_entries();
    assert!(entries.len() > 100);
    let message_count = entries
        .iter()
        .filter(|entry| matches!(entry, SessionEntry::Message(_)))
        .count();
    assert!(message_count > 100);

    let loaded = build_session_context(&entries, None);
    assert!(loaded.messages.len() > 100);
    assert!(loaded.model.is_some());
}

#[test]
fn large_session_cut_point_is_message() {
    let entries = load_large_session_entries();
    let result = find_cut_point(
        &entries,
        0,
        entries.len(),
        DEFAULT_COMPACTION_SETTINGS.keep_recent_tokens,
    );
    let entry = &entries[result.first_kept_entry_index];
    match entry {
        SessionEntry::Message(message) => match message.message {
            AgentMessage::User(_) | AgentMessage::Assistant(_) => {}
            _ => panic!("expected user or assistant message"),
        },
        _ => panic!("expected message entry"),
    }
}
