use pi::tui::{CombinedAutocompleteProvider, SlashCommand};

// Source: packages/tui/test/autocomplete.test.ts

#[test]
fn extracts_from_hey_when_forced() {
    let provider = CombinedAutocompleteProvider::new(vec![], "/tmp");
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
    let provider = CombinedAutocompleteProvider::new(vec![], "/tmp");
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
    let provider = CombinedAutocompleteProvider::new(vec![], "/tmp");
    let lines = vec![String::from("/model")];
    let cursor_line = 0;
    let cursor_col = 6;

    let result = provider.get_force_file_suggestions(&lines, cursor_line, cursor_col);

    assert!(result.is_none());
}

#[test]
fn triggers_for_absolute_paths_after_slash_command_argument() {
    let provider = CombinedAutocompleteProvider::new(vec![], "/tmp");
    let lines = vec![String::from("/command /")];
    let cursor_line = 0;
    let cursor_col = 10;

    let result = provider.get_force_file_suggestions(&lines, cursor_line, cursor_col);

    assert!(result.is_some());
    if let Some(result) = result {
        assert_eq!(result.prefix, "/");
    }
}

// Slash command autocomplete tests
#[test]
fn suggests_slash_commands_when_typing_slash() {
    let commands = vec![
        SlashCommand::new("model", Some("Select model".to_string())),
        SlashCommand::new("settings", Some("Open settings".to_string())),
        SlashCommand::new("export", Some("Export session".to_string())),
    ];
    let provider = CombinedAutocompleteProvider::new(commands, "/tmp");
    let lines = vec![String::from("/")];
    let cursor_line = 0;
    let cursor_col = 1;

    let result = provider.get_suggestions(&lines, cursor_line, cursor_col);

    assert!(result.is_some());
    let result = result.unwrap();
    assert_eq!(result.prefix, "/");
    assert_eq!(result.items.len(), 3);
}

#[test]
fn filters_slash_commands_by_prefix() {
    let commands = vec![
        SlashCommand::new("model", Some("Select model".to_string())),
        SlashCommand::new("settings", Some("Open settings".to_string())),
        SlashCommand::new("session", Some("Show session info".to_string())),
    ];
    let provider = CombinedAutocompleteProvider::new(commands, "/tmp");
    let lines = vec![String::from("/se")];
    let cursor_line = 0;
    let cursor_col = 3;

    let result = provider.get_suggestions(&lines, cursor_line, cursor_col);

    assert!(result.is_some());
    let result = result.unwrap();
    assert_eq!(result.prefix, "/se");
    assert_eq!(result.items.len(), 2);
    assert!(result.items.iter().any(|i| i.value == "settings"));
    assert!(result.items.iter().any(|i| i.value == "session"));
}

#[test]
fn slash_command_filter_is_case_insensitive() {
    let commands = vec![SlashCommand::new("Model", Some("Select model".to_string()))];
    let provider = CombinedAutocompleteProvider::new(commands, "/tmp");
    let lines = vec![String::from("/mod")];
    let cursor_line = 0;
    let cursor_col = 4;

    let result = provider.get_suggestions(&lines, cursor_line, cursor_col);

    assert!(result.is_some());
    let result = result.unwrap();
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].value, "Model");
}

#[test]
fn no_suggestions_for_unknown_slash_command() {
    let commands = vec![SlashCommand::new("model", None)];
    let provider = CombinedAutocompleteProvider::new(commands, "/tmp");
    let lines = vec![String::from("/xyz")];
    let cursor_line = 0;
    let cursor_col = 4;

    let result = provider.get_suggestions(&lines, cursor_line, cursor_col);

    assert!(result.is_none());
}

#[test]
fn apply_completion_for_slash_command() {
    let commands = vec![SlashCommand::new("model", None)];
    let provider = CombinedAutocompleteProvider::new(commands, "/tmp");
    let lines = vec![String::from("/mo")];
    let item = pi::tui::AutocompleteItem {
        value: "model".to_string(),
        label: "model".to_string(),
        description: None,
    };

    let (new_lines, new_line, new_col) = provider.apply_completion(&lines, 0, 3, &item, "/mo");

    assert_eq!(new_lines[0], "/model ");
    assert_eq!(new_line, 0);
    assert_eq!(new_col, 7);
}
