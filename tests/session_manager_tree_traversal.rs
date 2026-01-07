mod test_utils;

use pi::{SessionEntry, SessionManager};
use test_utils::{assistant_msg, user_msg};

#[test]
fn append_message_creates_parent_chain() {
    let mut session = SessionManager::in_memory();

    let id1 = session.append_message(user_msg("first"));
    let id2 = session.append_message(assistant_msg("second"));
    let id3 = session.append_message(user_msg("third"));

    let entries = session.get_entries();
    assert_eq!(entries.len(), 3);

    assert_eq!(entries[0].id(), id1);
    assert!(entries[0].parent_id().is_none());

    assert_eq!(entries[1].id(), id2);
    assert_eq!(entries[1].parent_id(), Some(id1.as_str()));

    assert_eq!(entries[2].id(), id3);
    assert_eq!(entries[2].parent_id(), Some(id2.as_str()));
}

#[test]
fn append_thinking_level_change_integrates() {
    let mut session = SessionManager::in_memory();

    let msg_id = session.append_message(user_msg("hello"));
    let thinking_id = session.append_thinking_level_change("high");
    let _ = session.append_message(assistant_msg("response"));

    let entries = session.get_entries();
    let thinking_entry = entries
        .iter()
        .find(|entry| matches!(entry, SessionEntry::ThinkingLevelChange(_)));
    assert!(thinking_entry.is_some());
    assert_eq!(thinking_entry.unwrap().id(), thinking_id);
    assert_eq!(thinking_entry.unwrap().parent_id(), Some(msg_id.as_str()));
}

#[test]
fn append_model_change_integrates() {
    let mut session = SessionManager::in_memory();

    let msg_id = session.append_message(user_msg("hello"));
    let model_id = session.append_model_change("openai", "gpt-4");
    let _ = session.append_message(assistant_msg("response"));

    let entries = session.get_entries();
    let model_entry = entries
        .iter()
        .find(|entry| matches!(entry, SessionEntry::ModelChange(_)))
        .unwrap();
    assert_eq!(model_entry.id(), model_id);
    assert_eq!(model_entry.parent_id(), Some(msg_id.as_str()));
}

#[test]
fn append_compaction_integrates() {
    let mut session = SessionManager::in_memory();

    let id1 = session.append_message(user_msg("1"));
    let _id2 = session.append_message(assistant_msg("2"));
    let compaction_id = session.append_compaction("summary", &id1, 1000);
    let _ = session.append_message(user_msg("3"));

    let entries = session.get_entries();
    let compaction_entry = entries
        .iter()
        .find(|entry| matches!(entry, SessionEntry::Compaction(_)))
        .unwrap();
    assert_eq!(compaction_entry.id(), compaction_id);
}

#[test]
fn append_custom_entry_integrates() {
    let mut session = SessionManager::in_memory();

    let msg_id = session.append_message(user_msg("hello"));
    let custom_id = session.append_custom_entry("my_hook", serde_json::json!({ "key": "value" }));
    let _ = session.append_message(assistant_msg("response"));

    let entries = session.get_entries();
    let custom_entry = entries
        .iter()
        .find(|entry| matches!(entry, SessionEntry::Custom(_)))
        .unwrap();
    assert_eq!(custom_entry.id(), custom_id);
    assert_eq!(custom_entry.parent_id(), Some(msg_id.as_str()));
}

#[test]
fn leaf_pointer_advances() {
    let mut session = SessionManager::in_memory();
    assert!(session.get_leaf_id().is_none());

    let id1 = session.append_message(user_msg("1"));
    assert_eq!(session.get_leaf_id(), Some(id1));

    let id2 = session.append_message(assistant_msg("2"));
    assert_eq!(session.get_leaf_id(), Some(id2));

    let id3 = session.append_thinking_level_change("high");
    assert_eq!(session.get_leaf_id(), Some(id3));
}

#[test]
fn get_branch_empty_session() {
    let session = SessionManager::in_memory();
    assert!(session.get_branch(None).is_empty());
}

#[test]
fn get_branch_single_entry() {
    let mut session = SessionManager::in_memory();
    let id = session.append_message(user_msg("hello"));
    let path = session.get_branch(None);
    assert_eq!(path.len(), 1);
    assert_eq!(path[0].id(), id);
}

#[test]
fn get_branch_full_path() {
    let mut session = SessionManager::in_memory();

    let id1 = session.append_message(user_msg("1"));
    let id2 = session.append_message(assistant_msg("2"));
    let id3 = session.append_thinking_level_change("high");
    let id4 = session.append_message(user_msg("3"));

    let path = session.get_branch(None);
    assert_eq!(path.len(), 4);
    assert_eq!(
        path.iter().map(|entry| entry.id()).collect::<Vec<_>>(),
        vec![id1.as_str(), id2.as_str(), id3.as_str(), id4.as_str()]
    );
}

#[test]
fn get_branch_from_specific_entry() {
    let mut session = SessionManager::in_memory();

    let id1 = session.append_message(user_msg("1"));
    let id2 = session.append_message(assistant_msg("2"));
    let _ = session.append_message(user_msg("3"));
    let _ = session.append_message(assistant_msg("4"));

    let path = session.get_branch(Some(&id2));
    assert_eq!(path.len(), 2);
    assert_eq!(path[0].id(), id1);
    assert_eq!(path[1].id(), id2);
}

#[test]
fn get_tree_single_root() {
    let mut session = SessionManager::in_memory();

    let id1 = session.append_message(user_msg("1"));
    let id2 = session.append_message(assistant_msg("2"));
    let id3 = session.append_message(user_msg("3"));

    let tree = session.get_tree();
    assert_eq!(tree.len(), 1);
    let root = &tree[0];
    assert_eq!(root.entry.id(), id1);
    assert_eq!(root.children.len(), 1);
    assert_eq!(root.children[0].entry.id(), id2);
    assert_eq!(root.children[0].children.len(), 1);
    assert_eq!(root.children[0].children[0].entry.id(), id3);
}

#[test]
fn get_tree_branches() {
    let mut session = SessionManager::in_memory();

    let id1 = session.append_message(user_msg("1"));
    let id2 = session.append_message(assistant_msg("2"));
    let id3 = session.append_message(user_msg("3"));

    session.branch(&id2).unwrap();
    let id4 = session.append_message(user_msg("4-branch"));

    let tree = session.get_tree();
    let root = &tree[0];
    assert_eq!(root.entry.id(), id1);
    let node2 = &root.children[0];
    assert_eq!(node2.entry.id(), id2);
    let child_ids: Vec<String> = node2
        .children
        .iter()
        .map(|child| child.entry.id().to_string())
        .collect();
    assert!(child_ids.contains(&id3));
    assert!(child_ids.contains(&id4));
}

#[test]
fn handles_multiple_branches() {
    let mut session = SessionManager::in_memory();

    let _id1 = session.append_message(user_msg("root"));
    let id2 = session.append_message(assistant_msg("response"));

    session.branch(&id2).unwrap();
    let id_a = session.append_message(user_msg("branch-A"));
    session.branch(&id2).unwrap();
    let id_b = session.append_message(user_msg("branch-B"));
    session.branch(&id2).unwrap();
    let id_c = session.append_message(user_msg("branch-C"));

    let tree = session.get_tree();
    let node2 = &tree[0].children[0];
    let branch_ids: Vec<String> = node2
        .children
        .iter()
        .map(|child| child.entry.id().to_string())
        .collect();
    assert!(branch_ids.contains(&id_a));
    assert!(branch_ids.contains(&id_b));
    assert!(branch_ids.contains(&id_c));
}

#[test]
fn handles_deep_branching() {
    let mut session = SessionManager::in_memory();

    let _id1 = session.append_message(user_msg("1"));
    let id2 = session.append_message(assistant_msg("2"));
    let id3 = session.append_message(user_msg("3"));
    let _id4 = session.append_message(assistant_msg("4"));

    session.branch(&id2).unwrap();
    let id5 = session.append_message(user_msg("5"));
    let _id6 = session.append_message(assistant_msg("6"));

    session.branch(&id5).unwrap();
    let _id7 = session.append_message(user_msg("7"));

    let tree = session.get_tree();
    let node2 = &tree[0].children[0];
    assert_eq!(node2.children.len(), 2);
    let node5 = node2
        .children
        .iter()
        .find(|child| child.entry.id() == id5)
        .unwrap();
    assert_eq!(node5.children.len(), 2);
    let node3 = node2
        .children
        .iter()
        .find(|child| child.entry.id() == id3)
        .unwrap();
    assert_eq!(node3.children.len(), 1);
}

#[test]
fn branch_moves_leaf_pointer() {
    let mut session = SessionManager::in_memory();

    let id1 = session.append_message(user_msg("1"));
    let _ = session.append_message(assistant_msg("2"));
    let id3 = session.append_message(user_msg("3"));
    assert_eq!(session.get_leaf_id(), Some(id3));

    session.branch(&id1).unwrap();
    assert_eq!(session.get_leaf_id(), Some(id1));
}

#[test]
fn branch_throws_for_non_existent() {
    let mut session = SessionManager::in_memory();
    session.append_message(user_msg("hello"));
    assert!(session.branch("nonexistent").is_err());
}

#[test]
fn new_appends_are_children_of_branch_point() {
    let mut session = SessionManager::in_memory();

    let id1 = session.append_message(user_msg("1"));
    let _ = session.append_message(assistant_msg("2"));

    session.branch(&id1).unwrap();
    let id3 = session.append_message(user_msg("branched"));

    let entries = session.get_entries();
    let branched_entry = entries.iter().find(|entry| entry.id() == id3).unwrap();
    assert_eq!(branched_entry.parent_id(), Some(id1.as_str()));
}

#[test]
fn branch_with_summary_inserts_entry() {
    let mut session = SessionManager::in_memory();

    let id1 = session.append_message(user_msg("1"));
    let _ = session.append_message(assistant_msg("2"));
    let _ = session.append_message(user_msg("3"));

    let summary_id = session
        .branch_with_summary(Some(&id1), "Summary of abandoned work", None, None)
        .unwrap();
    assert_eq!(session.get_leaf_id(), Some(summary_id));

    let entries = session.get_entries();
    let summary_entry = entries
        .iter()
        .find(|entry| matches!(entry, SessionEntry::BranchSummary(_)))
        .unwrap();
    assert_eq!(summary_entry.parent_id(), Some(id1.as_str()));
}

#[test]
fn branch_with_summary_throws_for_nonexistent() {
    let mut session = SessionManager::in_memory();
    session.append_message(user_msg("hello"));
    assert!(session
        .branch_with_summary(Some("nonexistent"), "summary", None, None)
        .is_err());
}

#[test]
fn get_leaf_entry_returns_none_for_empty() {
    let session = SessionManager::in_memory();
    assert!(session.get_leaf_entry().is_none());
}

#[test]
fn get_leaf_entry_returns_current_leaf() {
    let mut session = SessionManager::in_memory();
    session.append_message(user_msg("1"));
    let id2 = session.append_message(assistant_msg("2"));
    let leaf = session.get_leaf_entry().unwrap();
    assert_eq!(leaf.id(), id2);
}

#[test]
fn get_entry_returns_none_for_missing() {
    let session = SessionManager::in_memory();
    assert!(session.get_entry("nonexistent").is_none());
}

#[test]
fn get_entry_returns_entry_by_id() {
    let mut session = SessionManager::in_memory();
    let id1 = session.append_message(user_msg("first"));
    let id2 = session.append_message(assistant_msg("second"));
    let entry1 = session.get_entry(&id1).unwrap();
    assert_eq!(entry1.id(), id1);
    let entry2 = session.get_entry(&id2).unwrap();
    assert_eq!(entry2.id(), id2);
}

#[test]
fn build_session_context_with_branches() {
    let mut session = SessionManager::in_memory();

    session.append_message(user_msg("msg1"));
    let id2 = session.append_message(assistant_msg("msg2"));
    session.append_message(user_msg("msg3"));

    session.branch(&id2).unwrap();
    session.append_message(assistant_msg("msg4-branch"));

    let ctx = session.build_session_context();
    assert_eq!(ctx.messages.len(), 3);
}

#[test]
fn create_branched_session_throws_for_missing_entry() {
    let mut session = SessionManager::in_memory();
    session.append_message(user_msg("hello"));
    assert!(session.create_branched_session("nonexistent").is_err());
}

#[test]
fn create_branched_session_in_memory() {
    let mut session = SessionManager::in_memory();

    let id1 = session.append_message(user_msg("1"));
    let id2 = session.append_message(assistant_msg("2"));
    let id3 = session.append_message(user_msg("3"));
    session.append_message(assistant_msg("4"));

    session.branch(&id3).unwrap();
    let _ = session.append_message(user_msg("5"));

    let result = session.create_branched_session(&id2).unwrap();
    assert!(result.is_none());

    let entries = session.get_entries();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].id(), id1);
    assert_eq!(entries[1].id(), id2);
}

#[test]
fn create_branched_session_extracts_correct_path() {
    let mut session = SessionManager::in_memory();

    let id1 = session.append_message(user_msg("1"));
    let id2 = session.append_message(assistant_msg("2"));
    session.append_message(user_msg("3"));

    session.branch(&id2).unwrap();
    let id4 = session.append_message(user_msg("4"));
    let id5 = session.append_message(assistant_msg("5"));

    session.create_branched_session(&id5).unwrap();

    let entries = session.get_entries();
    assert_eq!(entries.len(), 4);
    assert_eq!(
        entries.iter().map(|entry| entry.id()).collect::<Vec<_>>(),
        vec![id1.as_str(), id2.as_str(), id4.as_str(), id5.as_str()]
    );
}
