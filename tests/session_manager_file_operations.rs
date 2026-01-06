use pi::{find_most_recent_session, load_entries_from_file, FileEntry, SessionHeader};
use std::fs;
use std::path::{Path, PathBuf};

fn temp_dir(name: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "pi-session-test-{}-{}",
        name,
        std::time::SystemTime::now().elapsed().unwrap().as_nanos()
    ));
    let _ = fs::create_dir_all(&dir);
    dir
}

fn write_file(path: &Path, content: &str) {
    let _ = fs::write(path, content);
}

#[test]
fn load_entries_returns_empty_for_nonexistent_file() {
    let dir = temp_dir("missing");
    let entries = load_entries_from_file(&dir.join("missing.jsonl"));
    assert!(entries.is_empty());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn load_entries_returns_empty_for_empty_file() {
    let dir = temp_dir("empty");
    let file = dir.join("empty.jsonl");
    write_file(&file, "");
    assert!(load_entries_from_file(&file).is_empty());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn load_entries_returns_empty_for_file_without_header() {
    let dir = temp_dir("no-header");
    let file = dir.join("no-header.jsonl");
    write_file(&file, "{\"type\":\"message\",\"id\":\"1\"}\n");
    assert!(load_entries_from_file(&file).is_empty());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn load_entries_returns_empty_for_malformed_json() {
    let dir = temp_dir("malformed");
    let file = dir.join("malformed.jsonl");
    write_file(&file, "not json\n");
    assert!(load_entries_from_file(&file).is_empty());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn load_entries_loads_valid_session_file() {
    let dir = temp_dir("valid");
    let file = dir.join("valid.jsonl");
    let header = FileEntry::Session(SessionHeader {
        id: "abc".to_string(),
        timestamp: "2025-01-01T00:00:00Z".to_string(),
        cwd: "/tmp".to_string(),
        version: None,
        parent_session: None,
    });
    let header_line = serde_json::to_string(&header).unwrap();
    let message_line = "{\"type\":\"message\",\"id\":\"1\",\"parentId\":null,\"timestamp\":\"2025-01-01T00:00:01Z\",\"message\":{\"role\":\"user\",\"content\":\"hi\",\"timestamp\":1}}";
    write_file(&file, &format!("{header_line}\n{message_line}\n"));
    let entries = load_entries_from_file(&file);
    assert_eq!(entries.len(), 2);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn load_entries_skips_malformed_lines() {
    let dir = temp_dir("mixed");
    let file = dir.join("mixed.jsonl");
    let header_line =
		"{\"type\":\"session\",\"id\":\"abc\",\"timestamp\":\"2025-01-01T00:00:00Z\",\"cwd\":\"/tmp\"}";
    let message_line = "{\"type\":\"message\",\"id\":\"1\",\"parentId\":null,\"timestamp\":\"2025-01-01T00:00:01Z\",\"message\":{\"role\":\"user\",\"content\":\"hi\",\"timestamp\":1}}";
    write_file(
        &file,
        &format!("{header_line}\nnot valid json\n{message_line}\n"),
    );
    let entries = load_entries_from_file(&file);
    assert_eq!(entries.len(), 2);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn find_most_recent_returns_none_for_empty_dir() {
    let dir = temp_dir("empty-dir");
    assert!(find_most_recent_session(&dir).is_none());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn find_most_recent_returns_none_for_missing_dir() {
    let dir = temp_dir("missing-dir");
    let missing = dir.join("missing");
    assert!(find_most_recent_session(&missing).is_none());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn find_most_recent_ignores_non_jsonl_files() {
    let dir = temp_dir("non-jsonl");
    write_file(&dir.join("file.txt"), "hello");
    write_file(&dir.join("file.json"), "{}");
    assert!(find_most_recent_session(&dir).is_none());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn find_most_recent_ignores_invalid_session_headers() {
    let dir = temp_dir("invalid-header");
    write_file(&dir.join("invalid.jsonl"), "{\"type\":\"message\"}\n");
    assert!(find_most_recent_session(&dir).is_none());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn find_most_recent_returns_single_valid_session_file() {
    let dir = temp_dir("single");
    let file = dir.join("session.jsonl");
    write_file(&file, "{\"type\":\"session\",\"id\":\"abc\",\"timestamp\":\"2025-01-01T00:00:00Z\",\"cwd\":\"/tmp\"}\n");
    let result = find_most_recent_session(&dir).expect("expected session file");
    assert_eq!(result, file);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn find_most_recent_returns_newest_file() {
    let dir = temp_dir("newest");
    let file1 = dir.join("older.jsonl");
    let file2 = dir.join("newer.jsonl");

    write_file(&file1, "{\"type\":\"session\",\"id\":\"old\",\"timestamp\":\"2025-01-01T00:00:00Z\",\"cwd\":\"/tmp\"}\n");
    std::thread::sleep(std::time::Duration::from_millis(10));
    write_file(&file2, "{\"type\":\"session\",\"id\":\"new\",\"timestamp\":\"2025-01-01T00:00:00Z\",\"cwd\":\"/tmp\"}\n");

    let result = find_most_recent_session(&dir).expect("expected session file");
    assert_eq!(result, file2);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn find_most_recent_skips_invalid_files() {
    let dir = temp_dir("skip-invalid");
    let invalid = dir.join("invalid.jsonl");
    let valid = dir.join("valid.jsonl");

    write_file(&invalid, "{\"type\":\"not-session\"}\n");
    std::thread::sleep(std::time::Duration::from_millis(10));
    write_file(&valid, "{\"type\":\"session\",\"id\":\"abc\",\"timestamp\":\"2025-01-01T00:00:00Z\",\"cwd\":\"/tmp\"}\n");

    let result = find_most_recent_session(&dir).expect("expected session file");
    assert_eq!(result, valid);
    let _ = fs::remove_dir_all(dir);
}
