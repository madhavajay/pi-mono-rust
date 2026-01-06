use pi::coding_agent::{
    CompactionResult, HookAPI, SessionBeforeCompactEvent, SessionBeforeCompactResult,
    SessionCompactEvent,
};

// Source: packages/coding-agent/test/compaction-hooks-example.test.ts

#[test]
fn custom_compaction_example_should_type_check_correctly() {
    let example_hook = |pi: &HookAPI| {
        pi.on_session_before_compact(|event: &SessionBeforeCompactEvent, ctx| {
            let preparation = &event.preparation;
            let _ = &preparation.messages_to_summarize;
            let _ = &preparation.turn_prefix_messages;
            let _ = preparation.tokens_before;
            let _ = &preparation.first_kept_entry_id;
            let _ = preparation.is_split_turn;
            let _ = &event.branch_entries;

            let _ = ctx.session_manager.get_entries();
            let _ = pi::coding_agent::ModelRegistry::get_api_key;

            SessionBeforeCompactResult {
                cancel: None,
                compaction: Some(CompactionResult {
                    summary: format!(
                        "User requests:\n{}",
                        preparation
                            .messages_to_summarize
                            .iter()
                            .filter(|m| matches!(m, pi::core::messages::AgentMessage::User(_)))
                            .map(|m| match m {
                                pi::core::messages::AgentMessage::User(user) => match &user.content
                                {
                                    pi::core::messages::UserContent::Text(text) => {
                                        format!("- {}", text.chars().take(100).collect::<String>())
                                    }
                                    _ => "- [complex]".to_string(),
                                },
                                _ => "- [complex]".to_string(),
                            })
                            .collect::<Vec<_>>()
                            .join("\n")
                    ),
                    first_kept_entry_id: preparation.first_kept_entry_id.clone(),
                    tokens_before: preparation.tokens_before,
                }),
            }
        });
    };

    example_hook(&HookAPI::new());
}

#[test]
fn compact_event_should_have_correct_fields() {
    let check_compact_event = |pi: &HookAPI| {
        pi.on_session_compact(|event: &SessionCompactEvent, _ctx| {
            let entry = &event.compaction_entry;
            let _ = &entry.summary;
            let _ = entry.tokens_before;
            let _ = event.from_hook;
        });
    };

    check_compact_event(&HookAPI::new());
}
