mod test_utils;

use pi::{SessionEntry, SessionManager};
use serde_json::json;
use test_utils::{assistant_msg, user_msg};

#[test]
fn saves_custom_entries_and_includes_them_in_tree_traversal() {
    let mut session = SessionManager::in_memory();

    let msg_id = session.append_message(user_msg("hello"));
    let custom_id = session.append_custom_entry("my_hook", json!({ "foo": "bar" }));
    let msg2_id = session.append_message(assistant_msg("hi"));

    let entries = session.get_entries();
    assert_eq!(entries.len(), 3);

    let custom_entry = entries.iter().find_map(|entry| match entry {
        SessionEntry::Custom(custom) => Some(custom),
        _ => None,
    });
    let custom_entry = custom_entry.expect("custom entry");
    assert_eq!(custom_entry.custom_type, "my_hook");
    assert_eq!(custom_entry.data, Some(json!({ "foo": "bar" })));
    assert_eq!(custom_entry.id, custom_id);
    assert_eq!(custom_entry.parent_id.as_deref(), Some(msg_id.as_str()));

    let path = session.get_branch(None);
    assert_eq!(path.len(), 3);
    assert_eq!(path[0].id(), msg_id);
    assert_eq!(path[1].id(), custom_id);
    assert_eq!(path[2].id(), msg2_id);

    let ctx = session.build_session_context();
    assert_eq!(ctx.messages.len(), 2);
}
