use pi::tui::{truncate_to_width, visible_width};

// Source: packages/coding-agent/test/truncate-to-width.test.ts

#[test]
fn should_truncate_messages_with_unicode_characters_correctly() {
    let message = "✔ script to run › dev $ concurrently \"vite\" \"node --import tsx ./";
    let width = 67;
    let max_msg_width = width - 2;

    let truncated = truncate_to_width(message, max_msg_width);
    let truncated_width = visible_width(&truncated);

    assert!(truncated_width <= max_msg_width);
}

#[test]
fn should_handle_emoji_characters() {
    let message = "🎉 Celebration! 🚀 Launch 📦 Package ready for deployment now";
    let width = 40;
    let max_msg_width = width - 2;

    let truncated = truncate_to_width(message, max_msg_width);
    let truncated_width = visible_width(&truncated);

    assert!(truncated_width <= max_msg_width);
}

#[test]
fn should_handle_mixed_ascii_and_wide_characters() {
    let message = "Hello 世界 Test 你好 More text here that is long";
    let width = 30;
    let max_msg_width = width - 2;

    let truncated = truncate_to_width(message, max_msg_width);
    let truncated_width = visible_width(&truncated);

    assert!(truncated_width <= max_msg_width);
}

#[test]
fn should_not_truncate_messages_that_fit() {
    let message = "Short message";
    let width = 50;
    let max_msg_width = width - 2;

    let truncated = truncate_to_width(message, max_msg_width);

    assert_eq!(truncated, message);
    assert!(visible_width(&truncated) <= max_msg_width);
}

#[test]
fn should_add_ellipsis_when_truncating() {
    let message = "This is a very long message that needs to be truncated";
    let width = 30;
    let max_msg_width = width - 2;

    let truncated = truncate_to_width(message, max_msg_width);

    assert!(truncated.contains("..."));
    assert!(visible_width(&truncated) <= max_msg_width);
}

#[test]
fn should_handle_the_exact_crash_case_from_issue_report() {
    let message = "✔ script to run › dev $ concurrently \"vite\" \"node --import tsx ./server.ts\"";
    let terminal_width = 67;
    let cursor_width = 2;
    let max_msg_width = terminal_width - cursor_width;

    let truncated = truncate_to_width(message, max_msg_width);
    let final_width = visible_width(&truncated);

    assert!(final_width + cursor_width <= terminal_width);
}
