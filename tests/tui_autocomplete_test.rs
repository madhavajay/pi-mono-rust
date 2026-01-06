use pi::tui::CombinedAutocompleteProvider;

// Source: packages/tui/test/autocomplete.test.ts

#[test]
fn extracts_from_hey_when_forced() {
    let provider = CombinedAutocompleteProvider::new("/tmp");
    let lines = vec![String::from("hey /")];
    let cursor_line = 0;
    let cursor_col = 5;

    let result = provider.get_force_file_suggestions(&lines, cursor_line, cursor_col);

    assert!(result.is_some());
    if let Some(result) = result {
        assert_eq!(result.prefix, "/");
    }
}

#[test]
fn extracts_a_from_a_when_forced() {
    let provider = CombinedAutocompleteProvider::new("/tmp");
    let lines = vec![String::from("/A")];
    let cursor_line = 0;
    let cursor_col = 2;

    let result = provider.get_force_file_suggestions(&lines, cursor_line, cursor_col);

    if let Some(result) = result {
        assert_eq!(result.prefix, "/A");
    }
}

#[test]
fn does_not_trigger_for_slash_commands() {
    let provider = CombinedAutocompleteProvider::new("/tmp");
    let lines = vec![String::from("/model")];
    let cursor_line = 0;
    let cursor_col = 6;

    let result = provider.get_force_file_suggestions(&lines, cursor_line, cursor_col);

    assert!(result.is_none());
}

#[test]
fn triggers_for_absolute_paths_after_slash_command_argument() {
    let provider = CombinedAutocompleteProvider::new("/tmp");
    let lines = vec![String::from("/command /")];
    let cursor_line = 0;
    let cursor_col = 10;

    let result = provider.get_force_file_suggestions(&lines, cursor_line, cursor_col);

    assert!(result.is_some());
    if let Some(result) = result {
        assert_eq!(result.prefix, "/");
    }
}
