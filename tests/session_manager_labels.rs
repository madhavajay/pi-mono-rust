mod test_utils;

use pi::{LabelEntry, SessionEntry, SessionManager};
use test_utils::{assistant_msg, user_msg};

#[test]
fn sets_and_gets_labels() {
    let mut session = SessionManager::in_memory();
    let msg_id = session.append_message(user_msg("hello"));

    assert!(session.get_label(&msg_id).is_none());

    let label_id = session
        .append_label_change(&msg_id, Some("checkpoint"))
        .unwrap();
    assert_eq!(session.get_label(&msg_id), Some("checkpoint".to_string()));

    let entries = session.get_entries();
    let label_entry = entries.iter().find_map(|entry| match entry {
        SessionEntry::Label(label) => Some(label),
        _ => None,
    });
    let label_entry = label_entry.expect("label entry");
    assert_eq!(label_entry.id, label_id);
    assert_eq!(label_entry.target_id, msg_id);
    assert_eq!(label_entry.label, Some("checkpoint".to_string()));
}

#[test]
fn clears_labels_with_undefined() {
    let mut session = SessionManager::in_memory();
    let msg_id = session.append_message(user_msg("hello"));

    session
        .append_label_change(&msg_id, Some("checkpoint"))
        .unwrap();
    assert_eq!(session.get_label(&msg_id), Some("checkpoint".to_string()));

    session.append_label_change(&msg_id, None).unwrap();
    assert!(session.get_label(&msg_id).is_none());
}

#[test]
fn last_label_wins() {
    let mut session = SessionManager::in_memory();
    let msg_id = session.append_message(user_msg("hello"));

    session.append_label_change(&msg_id, Some("first")).unwrap();
    session
        .append_label_change(&msg_id, Some("second"))
        .unwrap();
    session.append_label_change(&msg_id, Some("third")).unwrap();

    assert_eq!(session.get_label(&msg_id), Some("third".to_string()));
}

#[test]
fn labels_are_included_in_tree_nodes() {
    let mut session = SessionManager::in_memory();

    let msg1_id = session.append_message(user_msg("hello"));
    let msg2_id = session.append_message(assistant_msg("hi"));

    session
        .append_label_change(&msg1_id, Some("start"))
        .unwrap();
    session
        .append_label_change(&msg2_id, Some("response"))
        .unwrap();

    let tree = session.get_tree();
    let msg1_node = tree.iter().find(|node| node.entry.id() == msg1_id).unwrap();
    assert_eq!(msg1_node.label, Some("start".to_string()));

    let msg2_node = msg1_node
        .children
        .iter()
        .find(|node| node.entry.id() == msg2_id)
        .unwrap();
    assert_eq!(msg2_node.label, Some("response".to_string()));
}

#[test]
fn labels_preserved_in_create_branched_session() {
    let mut session = SessionManager::in_memory();

    let msg1_id = session.append_message(user_msg("hello"));
    let msg2_id = session.append_message(assistant_msg("hi"));

    session
        .append_label_change(&msg1_id, Some("important"))
        .unwrap();
    session
        .append_label_change(&msg2_id, Some("also-important"))
        .unwrap();

    let _ = session.create_branched_session(&msg2_id).unwrap();

    assert_eq!(session.get_label(&msg1_id), Some("important".to_string()));
    assert_eq!(
        session.get_label(&msg2_id),
        Some("also-important".to_string())
    );

    let entries = session.get_entries();
    let label_entries: Vec<&LabelEntry> = entries
        .iter()
        .filter_map(|entry| match entry {
            SessionEntry::Label(label) => Some(label),
            _ => None,
        })
        .collect();
    assert_eq!(label_entries.len(), 2);
}

#[test]
fn labels_not_on_path_are_not_preserved() {
    let mut session = SessionManager::in_memory();

    let msg1_id = session.append_message(user_msg("hello"));
    let msg2_id = session.append_message(assistant_msg("hi"));
    let msg3_id = session.append_message(user_msg("followup"));

    session
        .append_label_change(&msg1_id, Some("first"))
        .unwrap();
    session
        .append_label_change(&msg2_id, Some("second"))
        .unwrap();
    session
        .append_label_change(&msg3_id, Some("third"))
        .unwrap();

    let _ = session.create_branched_session(&msg2_id).unwrap();

    assert_eq!(session.get_label(&msg1_id), Some("first".to_string()));
    assert_eq!(session.get_label(&msg2_id), Some("second".to_string()));
    assert!(session.get_label(&msg3_id).is_none());
}

#[test]
fn labels_not_included_in_build_session_context() {
    let mut session = SessionManager::in_memory();
    let msg_id = session.append_message(user_msg("hello"));
    session
        .append_label_change(&msg_id, Some("checkpoint"))
        .unwrap();

    let ctx = session.build_session_context();
    assert_eq!(ctx.messages.len(), 1);
}

#[test]
fn throws_when_labeling_non_existent_entry() {
    let mut session = SessionManager::in_memory();
    assert!(session
        .append_label_change("non-existent", Some("label"))
        .is_err());
}
