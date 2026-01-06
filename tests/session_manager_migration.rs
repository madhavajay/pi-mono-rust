use pi::{migrate_session_entries, FileEntry, SessionHeader, SessionMessageEntry};

#[test]
fn adds_id_and_parent_id_to_v1_entries() {
    let mut entries = vec![
        FileEntry::Session(SessionHeader {
            id: "sess-1".to_string(),
            version: None,
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            cwd: "/tmp".to_string(),
            parent_session: None,
        }),
        FileEntry::Message(SessionMessageEntry {
            id: String::new(),
            parent_id: None,
            timestamp: "2025-01-01T00:00:01Z".to_string(),
            message: pi::AgentMessage::User(pi::UserMessage {
                content: pi::UserContent::Text("hi".to_string()),
                timestamp: 1,
            }),
        }),
        FileEntry::Message(SessionMessageEntry {
            id: String::new(),
            parent_id: None,
            timestamp: "2025-01-01T00:00:02Z".to_string(),
            message: pi::AgentMessage::Assistant(pi::AssistantMessage {
                content: vec![pi::ContentBlock::Text {
                    text: "hello".to_string(),
                    text_signature: None,
                }],
                api: "test".to_string(),
                provider: "test".to_string(),
                model: "test".to_string(),
                usage: pi::Usage {
                    input: 1,
                    output: 1,
                    cache_read: 0,
                    cache_write: 0,
                    total_tokens: None,
                    cost: None,
                },
                stop_reason: "stop".to_string(),
                error_message: None,
                timestamp: 2,
            }),
        }),
    ];

    migrate_session_entries(&mut entries);

    let header = match &entries[0] {
        FileEntry::Session(header) => header,
        _ => panic!("expected session header"),
    };
    assert_eq!(header.version, Some(2));

    let msg1 = match &entries[1] {
        FileEntry::Message(message) => message,
        _ => panic!("expected message"),
    };
    let msg2 = match &entries[2] {
        FileEntry::Message(message) => message,
        _ => panic!("expected message"),
    };

    assert!(!msg1.id.is_empty());
    assert_eq!(msg1.id.len(), 8);
    assert!(msg1.parent_id.is_none());

    assert!(!msg2.id.is_empty());
    assert_eq!(msg2.id.len(), 8);
    assert_eq!(msg2.parent_id.as_deref(), Some(msg1.id.as_str()));
}

#[test]
fn migration_is_idempotent() {
    let mut entries = vec![
        FileEntry::Session(SessionHeader {
            id: "sess-1".to_string(),
            version: Some(2),
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            cwd: "/tmp".to_string(),
            parent_session: None,
        }),
        FileEntry::Message(SessionMessageEntry {
            id: "abc12345".to_string(),
            parent_id: None,
            timestamp: "2025-01-01T00:00:01Z".to_string(),
            message: pi::AgentMessage::User(pi::UserMessage {
                content: pi::UserContent::Text("hi".to_string()),
                timestamp: 1,
            }),
        }),
        FileEntry::Message(SessionMessageEntry {
            id: "def67890".to_string(),
            parent_id: Some("abc12345".to_string()),
            timestamp: "2025-01-01T00:00:02Z".to_string(),
            message: pi::AgentMessage::Assistant(pi::AssistantMessage {
                content: vec![pi::ContentBlock::Text {
                    text: "hello".to_string(),
                    text_signature: None,
                }],
                api: "test".to_string(),
                provider: "test".to_string(),
                model: "test".to_string(),
                usage: pi::Usage {
                    input: 1,
                    output: 1,
                    cache_read: 0,
                    cache_write: 0,
                    total_tokens: None,
                    cost: None,
                },
                stop_reason: "stop".to_string(),
                error_message: None,
                timestamp: 2,
            }),
        }),
    ];

    migrate_session_entries(&mut entries);

    let msg1 = match &entries[1] {
        FileEntry::Message(message) => message,
        _ => panic!("expected message"),
    };
    let msg2 = match &entries[2] {
        FileEntry::Message(message) => message,
        _ => panic!("expected message"),
    };

    assert_eq!(msg1.id, "abc12345");
    assert_eq!(msg2.id, "def67890");
    assert_eq!(msg2.parent_id.as_deref(), Some("abc12345"));
}
