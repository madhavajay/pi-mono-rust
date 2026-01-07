use pi::core::session_manager::{FileEntry, SessionHeader, SessionManager, SessionMessageEntry};
use pi::{AgentMessage, AssistantMessage, ContentBlock, Cost, Usage, UserContent, UserMessage};
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> Self {
        let mut path = std::env::temp_dir();
        let since_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        path.push(format!("{prefix}-{since_epoch}-{}", std::process::id()));
        fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    fn join(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn assistant_msg(text: &str) -> AgentMessage {
    AgentMessage::Assistant(AssistantMessage {
        content: vec![ContentBlock::Text {
            text: text.to_string(),
            text_signature: None,
        }],
        api: "anthropic-messages".to_string(),
        provider: "anthropic".to_string(),
        model: "test".to_string(),
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
    })
}

fn user_msg(text: &str) -> AgentMessage {
    AgentMessage::User(UserMessage {
        content: UserContent::Text(text.to_string()),
        timestamp: 1,
    })
}

fn write_session_file(path: &Path, id: &str, messages: Vec<AgentMessage>) {
    let header = SessionHeader {
        id: id.to_string(),
        timestamp: "2025-01-01T00:00:00Z".to_string(),
        cwd: ".".to_string(),
        version: Some(2),
        parent_session: None,
    };
    let mut entries = Vec::new();
    entries.push(FileEntry::Session(header));
    for (idx, message) in messages.into_iter().enumerate() {
        entries.push(FileEntry::Message(SessionMessageEntry {
            id: format!("msg-{id}-{idx}"),
            parent_id: None,
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            message,
        }));
    }
    let lines = entries
        .into_iter()
        .map(|entry| serde_json::to_string(&entry).expect("serialize session entry"))
        .collect::<Vec<_>>();
    fs::write(path, lines.join("\n")).expect("write session file");
}

#[test]
fn list_sessions_returns_message_info() {
    let temp = TempDir::new("pi-session-list");
    let session_one = temp.join("session-one.jsonl");
    let session_two = temp.join("session-two.jsonl");

    write_session_file(
        &session_one,
        "session-one",
        vec![user_msg("First session"), assistant_msg("Assistant reply")],
    );
    thread::sleep(Duration::from_millis(10));
    write_session_file(
        &session_two,
        "session-two",
        vec![user_msg("Second session"), assistant_msg("Another reply")],
    );

    let sessions = SessionManager::list(&temp.path, Some(temp.path.clone()));
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0].id, "session-two");
    assert_eq!(sessions[0].message_count, 2);
    assert_eq!(sessions[0].first_message, "Second session");
    assert!(sessions[0].all_messages_text.contains("Second session"));
    assert!(sessions[0].all_messages_text.contains("Another reply"));
}
