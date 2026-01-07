use pi::tui::{visible_width, Editor, EditorTheme};

// Source: packages/tui/test/editor.test.ts

#[test]
fn does_nothing_on_up_arrow_when_history_is_empty() {
    let mut editor = Editor::new(default_editor_theme());

    editor.handle_input("\x1b[A");

    assert_eq!(editor.get_text(), "");
}

#[test]
fn shows_most_recent_history_entry_on_up_arrow_when_editor_is_empty() {
    let mut editor = Editor::new(default_editor_theme());

    editor.add_to_history("first prompt");
    editor.add_to_history("second prompt");

    editor.handle_input("\x1b[A");

    assert_eq!(editor.get_text(), "second prompt");
}

#[test]
fn cycles_through_history_entries_on_repeated_up_arrow() {
    let mut editor = Editor::new(default_editor_theme());

    editor.add_to_history("first");
    editor.add_to_history("second");
    editor.add_to_history("third");

    editor.handle_input("\x1b[A");
    assert_eq!(editor.get_text(), "third");

    editor.handle_input("\x1b[A");
    assert_eq!(editor.get_text(), "second");

    editor.handle_input("\x1b[A");
    assert_eq!(editor.get_text(), "first");

    editor.handle_input("\x1b[A");
    assert_eq!(editor.get_text(), "first");
}

#[test]
fn returns_to_empty_editor_on_down_arrow_after_browsing_history() {
    let mut editor = Editor::new(default_editor_theme());

    editor.add_to_history("prompt");

    editor.handle_input("\x1b[A");
    assert_eq!(editor.get_text(), "prompt");

    editor.handle_input("\x1b[B");
    assert_eq!(editor.get_text(), "");
}

#[test]
fn navigates_forward_through_history_with_down_arrow() {
    let mut editor = Editor::new(default_editor_theme());

    editor.add_to_history("first");
    editor.add_to_history("second");
    editor.add_to_history("third");

    editor.handle_input("\x1b[A");
    editor.handle_input("\x1b[A");
    editor.handle_input("\x1b[A");

    editor.handle_input("\x1b[B");
    assert_eq!(editor.get_text(), "second");

    editor.handle_input("\x1b[B");
    assert_eq!(editor.get_text(), "third");

    editor.handle_input("\x1b[B");
    assert_eq!(editor.get_text(), "");
}

#[test]
fn exits_history_mode_when_typing_a_character() {
    let mut editor = Editor::new(default_editor_theme());

    editor.add_to_history("old prompt");

    editor.handle_input("\x1b[A");
    editor.handle_input("x");

    assert_eq!(editor.get_text(), "old promptx");
}

#[test]
fn exits_history_mode_on_settext() {
    let mut editor = Editor::new(default_editor_theme());

    editor.add_to_history("first");
    editor.add_to_history("second");

    editor.handle_input("\x1b[A");
    editor.set_text("");

    editor.handle_input("\x1b[A");
    assert_eq!(editor.get_text(), "second");
}

#[test]
fn does_not_add_empty_strings_to_history() {
    let mut editor = Editor::new(default_editor_theme());

    editor.add_to_history("");
    editor.add_to_history("   ");
    editor.add_to_history("valid");

    editor.handle_input("\x1b[A");
    assert_eq!(editor.get_text(), "valid");

    editor.handle_input("\x1b[A");
    assert_eq!(editor.get_text(), "valid");
}

#[test]
fn does_not_add_consecutive_duplicates_to_history() {
    let mut editor = Editor::new(default_editor_theme());

    editor.add_to_history("same");
    editor.add_to_history("same");
    editor.add_to_history("same");

    editor.handle_input("\x1b[A");
    assert_eq!(editor.get_text(), "same");

    editor.handle_input("\x1b[A");
    assert_eq!(editor.get_text(), "same");
}

#[test]
fn allows_non_consecutive_duplicates_in_history() {
    let mut editor = Editor::new(default_editor_theme());

    editor.add_to_history("first");
    editor.add_to_history("second");
    editor.add_to_history("first");

    editor.handle_input("\x1b[A");
    assert_eq!(editor.get_text(), "first");

    editor.handle_input("\x1b[A");
    assert_eq!(editor.get_text(), "second");

    editor.handle_input("\x1b[A");
    assert_eq!(editor.get_text(), "first");
}

#[test]
fn uses_cursor_movement_instead_of_history_when_editor_has_content() {
    let mut editor = Editor::new(default_editor_theme());

    editor.add_to_history("history item");
    editor.set_text("line1\nline2");

    editor.handle_input("\x1b[A");
    editor.handle_input("X");

    assert_eq!(editor.get_text(), "line1X\nline2");
}

#[test]
fn limits_history_to_100_entries() {
    let mut editor = Editor::new(default_editor_theme());

    for i in 0..105 {
        editor.add_to_history(&format!("prompt {i}"));
    }

    for _ in 0..100 {
        editor.handle_input("\x1b[A");
    }

    assert_eq!(editor.get_text(), "prompt 5");

    editor.handle_input("\x1b[A");
    assert_eq!(editor.get_text(), "prompt 5");
}

#[test]
fn allows_cursor_movement_within_multi_line_history_entry_with_down() {
    let mut editor = Editor::new(default_editor_theme());

    editor.add_to_history("line1\nline2\nline3");

    editor.handle_input("\x1b[A");
    assert_eq!(editor.get_text(), "line1\nline2\nline3");

    editor.handle_input("\x1b[B");
    assert_eq!(editor.get_text(), "");
}

#[test]
fn allows_cursor_movement_within_multi_line_history_entry_with_up() {
    let mut editor = Editor::new(default_editor_theme());

    editor.add_to_history("older entry");
    editor.add_to_history("line1\nline2\nline3");

    editor.handle_input("\x1b[A");
    editor.handle_input("\x1b[A");
    assert_eq!(editor.get_text(), "line1\nline2\nline3");

    editor.handle_input("\x1b[A");
    assert_eq!(editor.get_text(), "line1\nline2\nline3");

    editor.handle_input("\x1b[A");
    assert_eq!(editor.get_text(), "older entry");
}

#[test]
fn navigates_from_multi_line_entry_back_to_newer_via_down_after_cursor_movement() {
    let mut editor = Editor::new(default_editor_theme());

    editor.add_to_history("line1\nline2\nline3");

    editor.handle_input("\x1b[A");
    editor.handle_input("\x1b[A");
    editor.handle_input("\x1b[A");

    editor.handle_input("\x1b[B");
    assert_eq!(editor.get_text(), "line1\nline2\nline3");

    editor.handle_input("\x1b[B");
    assert_eq!(editor.get_text(), "line1\nline2\nline3");

    editor.handle_input("\x1b[B");
    assert_eq!(editor.get_text(), "");
}

#[test]
fn returns_cursor_position() {
    let mut editor = Editor::new(default_editor_theme());

    assert_eq!(editor.get_cursor(), (0, 0));

    editor.handle_input("a");
    editor.handle_input("b");
    editor.handle_input("c");

    assert_eq!(editor.get_cursor(), (0, 3));

    editor.handle_input("\x1b[D");
    assert_eq!(editor.get_cursor(), (0, 2));
}

#[test]
fn returns_lines_as_a_defensive_copy() {
    let mut editor = Editor::new(default_editor_theme());
    editor.set_text("a\nb");

    let mut lines = editor.get_lines();
    assert_eq!(lines, vec!["a", "b"]);

    lines[0] = "mutated".to_string();
    assert_eq!(editor.get_lines(), vec!["a", "b"]);
}

#[test]
fn inserts_mixed_ascii_umlauts_and_emojis_as_literal_text() {
    let mut editor = Editor::new(default_editor_theme());

    editor.handle_input("H");
    editor.handle_input("e");
    editor.handle_input("l");
    editor.handle_input("l");
    editor.handle_input("o");
    editor.handle_input(" ");
    editor.handle_input("ä");
    editor.handle_input("ö");
    editor.handle_input("ü");
    editor.handle_input(" ");
    editor.handle_input("😀");

    assert_eq!(editor.get_text(), "Hello äöü 😀");
}

#[test]
fn deletes_single_code_unit_unicode_characters_umlauts_with_backspace() {
    let mut editor = Editor::new(default_editor_theme());

    editor.handle_input("ä");
    editor.handle_input("ö");
    editor.handle_input("ü");

    editor.handle_input("\x7f");

    assert_eq!(editor.get_text(), "äö");
}

#[test]
fn deletes_multi_code_unit_emojis_with_single_backspace() {
    let mut editor = Editor::new(default_editor_theme());

    editor.handle_input("😀");
    editor.handle_input("👍");

    editor.handle_input("\x7f");

    assert_eq!(editor.get_text(), "😀");
}

#[test]
fn inserts_characters_at_the_correct_position_after_cursor_movement_over_umlauts() {
    let mut editor = Editor::new(default_editor_theme());

    editor.handle_input("ä");
    editor.handle_input("ö");
    editor.handle_input("ü");

    editor.handle_input("\x1b[D");
    editor.handle_input("\x1b[D");

    editor.handle_input("x");

    assert_eq!(editor.get_text(), "äxöü");
}

#[test]
fn moves_cursor_across_multi_code_unit_emojis_with_single_arrow_key() {
    let mut editor = Editor::new(default_editor_theme());

    editor.handle_input("😀");
    editor.handle_input("👍");
    editor.handle_input("🎉");

    editor.handle_input("\x1b[D");
    editor.handle_input("\x1b[D");

    editor.handle_input("x");

    assert_eq!(editor.get_text(), "😀x👍🎉");
}

#[test]
fn preserves_umlauts_across_line_breaks() {
    let mut editor = Editor::new(default_editor_theme());

    editor.handle_input("ä");
    editor.handle_input("ö");
    editor.handle_input("ü");
    editor.handle_input("\n");
    editor.handle_input("Ä");
    editor.handle_input("Ö");
    editor.handle_input("Ü");

    assert_eq!(editor.get_text(), "äöü\nÄÖÜ");
}

#[test]
fn replaces_the_entire_document_with_unicode_text_via_settext_paste_simulation() {
    let mut editor = Editor::new(default_editor_theme());

    editor.set_text("Hällö Wörld! 😀 äöüÄÖÜß");

    assert_eq!(editor.get_text(), "Hällö Wörld! 😀 äöüÄÖÜß");
}

#[test]
fn moves_cursor_to_document_start_on_ctrl_a_and_inserts_at_the_beginning() {
    let mut editor = Editor::new(default_editor_theme());

    editor.handle_input("a");
    editor.handle_input("b");
    editor.handle_input("\x01");
    editor.handle_input("x");

    assert_eq!(editor.get_text(), "xab");
}

#[test]
fn deletes_words_correctly_with_ctrl_w_and_alt_backspace() {
    let mut editor = Editor::new(default_editor_theme());

    editor.set_text("foo bar baz");
    editor.handle_input("\x17");
    assert_eq!(editor.get_text(), "foo bar ");

    editor.set_text("foo bar   ");
    editor.handle_input("\x17");
    assert_eq!(editor.get_text(), "foo ");

    editor.set_text("foo bar...");
    editor.handle_input("\x17");
    assert_eq!(editor.get_text(), "foo bar");

    editor.set_text("line one\nline two");
    editor.handle_input("\x17");
    assert_eq!(editor.get_text(), "line one\nline ");

    editor.set_text("line one\n");
    editor.handle_input("\x17");
    assert_eq!(editor.get_text(), "line one");

    editor.set_text("foo 😀😀 bar");
    editor.handle_input("\x17");
    assert_eq!(editor.get_text(), "foo 😀😀 ");
    editor.handle_input("\x17");
    assert_eq!(editor.get_text(), "foo ");

    editor.set_text("foo bar");
    editor.handle_input("\x1b\x7f");
    assert_eq!(editor.get_text(), "foo ");
}

#[test]
fn navigates_words_correctly_with_ctrl_left_right() {
    let mut editor = Editor::new(default_editor_theme());

    editor.set_text("foo bar... baz");

    editor.handle_input("\x1b[1;5D");
    assert_eq!(editor.get_cursor(), (0, 11));

    editor.handle_input("\x1b[1;5D");
    assert_eq!(editor.get_cursor(), (0, 7));

    editor.handle_input("\x1b[1;5D");
    assert_eq!(editor.get_cursor(), (0, 4));

    editor.handle_input("\x1b[1;5C");
    assert_eq!(editor.get_cursor(), (0, 7));

    editor.handle_input("\x1b[1;5C");
    assert_eq!(editor.get_cursor(), (0, 10));

    editor.handle_input("\x1b[1;5C");
    assert_eq!(editor.get_cursor(), (0, 14));

    editor.set_text("   foo bar");
    editor.handle_input("\x01");
    editor.handle_input("\x1b[1;5C");
    assert_eq!(editor.get_cursor(), (0, 6));
}

#[test]
fn wraps_lines_correctly_when_text_contains_wide_emojis() {
    let mut editor = Editor::new(default_editor_theme());
    let width = 20;

    editor.set_text("Hello ✅ World");
    let lines = editor.render(width);

    for line in &lines[1..lines.len() - 1] {
        let line_width = visible_width(line);
        assert_eq!(line_width, width);
    }
}

#[test]
fn wraps_long_text_with_emojis_at_correct_positions() {
    let mut editor = Editor::new(default_editor_theme());
    let width = 10;

    editor.set_text("✅✅✅✅✅✅");
    let lines = editor.render(width);

    for line in &lines[1..lines.len() - 1] {
        let line_width = visible_width(line);
        assert_eq!(line_width, width);
    }
}

#[test]
fn wraps_cjk_characters_correctly_each_is_2_columns_wide() {
    let mut editor = Editor::new(default_editor_theme());
    let width = 10;

    editor.set_text("日本語テスト");
    let lines = editor.render(width);

    for line in &lines[1..lines.len() - 1] {
        let line_width = visible_width(line);
        assert_eq!(line_width, width);
    }

    let content_lines: Vec<String> = lines[1..lines.len() - 1]
        .iter()
        .map(|line| strip_vt_control_characters(line).trim().to_string())
        .collect();
    assert_eq!(content_lines.len(), 2);
    assert_eq!(content_lines[0], "日本語テス");
    assert_eq!(content_lines[1], "ト");
}

#[test]
fn handles_mixed_ascii_and_wide_characters_in_wrapping() {
    let mut editor = Editor::new(default_editor_theme());
    let width = 15;

    editor.set_text("Test ✅ OK 日本");
    let lines = editor.render(width);

    let content_lines = &lines[1..lines.len() - 1];
    assert_eq!(content_lines.len(), 1);
    assert_eq!(visible_width(&content_lines[0]), width);
}

#[test]
fn renders_cursor_correctly_on_wide_characters() {
    let mut editor = Editor::new(default_editor_theme());
    let width = 20;

    editor.set_text("A✅B");
    let lines = editor.render(width);

    let content_line = &lines[1];
    assert!(content_line.contains("\x1b[7m"));
    assert_eq!(visible_width(content_line), width);
}

#[test]
fn does_not_exceed_terminal_width_with_emoji_at_wrap_boundary() {
    let mut editor = Editor::new(default_editor_theme());
    let width = 11;

    editor.set_text("0123456789✅");
    let lines = editor.render(width);

    for line in &lines[1..lines.len() - 1] {
        let line_width = visible_width(line);
        assert!(line_width <= width);
    }
}

#[test]
fn wraps_at_word_boundaries_instead_of_mid_word() {
    let mut editor = Editor::new(default_editor_theme());
    let width = 40;

    editor.set_text("Hello world this is a test of word wrapping functionality");
    let lines = editor.render(width);

    let content_lines: Vec<String> = lines[1..lines.len() - 1]
        .iter()
        .map(|line| strip_vt_control_characters(line).trim().to_string())
        .collect();

    assert!(!content_lines[0].ends_with('-'));

    for line in content_lines {
        let trimmed = line.trim_end();
        let last_char = trimmed.chars().last().unwrap_or('\0');
        if !trimmed.is_empty() {
            assert!(last_char.is_ascii_alphanumeric() || ".,!?;:".contains(last_char));
        }
    }
}

#[test]
fn does_not_start_lines_with_leading_whitespace_after_word_wrap() {
    let mut editor = Editor::new(default_editor_theme());
    let width = 20;

    editor.set_text("Word1 Word2 Word3 Word4 Word5 Word6");
    let lines = editor.render(width);

    for line in &lines[1..lines.len() - 1] {
        let stripped = strip_vt_control_characters(line);
        let trimmed_start = stripped.trim_start();
        if !trimmed_start.is_empty() {
            assert!(!stripped.trim_end().starts_with(' '));
        }
    }
}

#[test]
fn breaks_long_words_urls_at_character_level() {
    let mut editor = Editor::new(default_editor_theme());
    let width = 30;

    editor.set_text("Check https://example.com/very/long/path/that/exceeds/width here");
    let lines = editor.render(width);

    for line in &lines[1..lines.len() - 1] {
        let line_width = visible_width(line);
        assert_eq!(line_width, width);
    }
}

#[test]
fn preserves_multiple_spaces_within_words_on_same_line() {
    let mut editor = Editor::new(default_editor_theme());
    let width = 50;

    editor.set_text("Word1   Word2    Word3");
    let lines = editor.render(width);

    let content_line = strip_vt_control_characters(&lines[1]).trim().to_string();
    assert!(content_line.contains("Word1   Word2"));
}

#[test]
fn handles_empty_string() {
    let mut editor = Editor::new(default_editor_theme());
    let width = 40;

    editor.set_text("");
    let lines = editor.render(width);

    assert_eq!(lines.len(), 3);
}

#[test]
fn handles_single_word_that_fits_exactly() {
    let mut editor = Editor::new(default_editor_theme());
    let width = 10;

    editor.set_text("1234567890");
    let lines = editor.render(width);

    assert_eq!(lines.len(), 3);
    let content_line = strip_vt_control_characters(&lines[1]);
    assert!(content_line.contains("1234567890"));
}

fn default_editor_theme() -> EditorTheme {
    EditorTheme {
        border_color: identity_color,
    }
}

fn identity_color(text: &str) -> String {
    text.to_string()
}

fn strip_vt_control_characters(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            if let Some(next) = chars.peek().copied() {
                if next == '[' {
                    chars.next();
                    for c in chars.by_ref() {
                        if matches!(c, 'm' | 'G' | 'K' | 'H' | 'J') {
                            break;
                        }
                    }
                    continue;
                }
            }
        }
        output.push(ch);
    }
    output
}

// Bracketed paste mode tests
#[test]
fn handles_bracketed_paste_mode_single_chunk() {
    let mut editor = Editor::new(default_editor_theme());

    // Complete paste in one chunk: \x1b[200~ ... \x1b[201~
    editor.handle_input("\x1b[200~pasted text\x1b[201~");

    assert_eq!(editor.get_text(), "pasted text");
}

#[test]
fn handles_bracketed_paste_mode_multiple_chunks() {
    let mut editor = Editor::new(default_editor_theme());

    // Paste split across multiple chunks
    editor.handle_input("\x1b[200~first ");
    editor.handle_input("second ");
    editor.handle_input("third\x1b[201~");

    assert_eq!(editor.get_text(), "first second third");
}

#[test]
fn handles_bracketed_paste_with_remaining_input() {
    let mut editor = Editor::new(default_editor_theme());

    // Paste followed by regular input
    editor.handle_input("\x1b[200~pasted\x1b[201~extra");

    assert_eq!(editor.get_text(), "pastedextra");
}

#[test]
fn handles_bracketed_paste_normalizes_line_endings() {
    let mut editor = Editor::new(default_editor_theme());

    // Paste with Windows and Mac line endings
    editor.handle_input("\x1b[200~line1\r\nline2\rline3\x1b[201~");

    assert_eq!(editor.get_text(), "line1\nline2\nline3");
}

#[test]
fn handles_bracketed_paste_preserves_newlines_in_multiline_editor() {
    let mut editor = Editor::new(default_editor_theme());

    // Multi-line paste
    editor.handle_input("\x1b[200~line1\nline2\nline3\x1b[201~");

    assert_eq!(editor.get_text(), "line1\nline2\nline3");
}

#[test]
fn handles_bracketed_paste_at_cursor_position() {
    let mut editor = Editor::new(default_editor_theme());

    editor.handle_input("before ");
    editor.handle_input("\x1b[200~middle\x1b[201~");
    editor.handle_input(" after");

    assert_eq!(editor.get_text(), "before middle after");
}

// Control character filtering tests
#[test]
fn rejects_control_characters_c0_range() {
    let mut editor = Editor::new(default_editor_theme());

    // C0 control characters (0x00-0x1F) except newline (0x0A)
    // Note: Some control chars like \x01 (Ctrl+A) are handled specially before text insertion
    // Here we test filtering via bracketed paste mode to bypass special handling
    editor.handle_input("\x1b[200~a\x00\x02\x03b\x1b[201~"); // NUL, STX, ETX filtered
    editor.handle_input("c");

    // Should only have "abc" - control chars are filtered
    assert_eq!(editor.get_text(), "abc");
}

#[test]
fn rejects_del_character() {
    let mut editor = Editor::new(default_editor_theme());

    // DEL (0x7F) is a special control character
    // Note: \x7f is handled as backspace in handle_input match
    // but if it comes as part of text insertion, it should be filtered
    editor.handle_input("test");
    // The direct \x7f is handled as backspace
    // Let's test filtering in insert_text context
    editor.set_text("a\x7fb");
    // set_text_internal doesn't filter, but if we test via insert_text...
    // Actually set_text goes through set_text_internal which doesn't filter
    // Control char filtering is in insert_text which is called from handle_input fallback

    // For this test, we verify that direct text input filtering works
    let mut editor2 = Editor::new(default_editor_theme());
    editor2.set_text(""); // Reset
                          // Simulate inserting text with DEL embedded (via paste)
    editor2.handle_input("\x1b[200~a\x7fb\x1b[201~");
    assert_eq!(editor2.get_text(), "ab");
}

#[test]
fn rejects_c1_control_characters() {
    let mut editor = Editor::new(default_editor_theme());

    // C1 control characters (0x80-0x9F)
    // These are multi-byte in UTF-8, so we need to be careful
    // 0x80 = \u{0080}, 0x9F = \u{009F}
    editor.handle_input("\x1b[200~a\u{0080}b\u{009F}c\x1b[201~");

    assert_eq!(editor.get_text(), "abc");
}

#[test]
fn allows_printable_unicode_characters() {
    let mut editor = Editor::new(default_editor_theme());

    // Various printable unicode should be allowed
    editor.handle_input("Hello ");
    editor.handle_input("Ä"); // German umlaut
    editor.handle_input("ö");
    editor.handle_input(" ");
    editor.handle_input("日本語"); // Japanese
    editor.handle_input(" ");
    editor.handle_input("😀"); // Emoji
    editor.handle_input(" ");
    editor.handle_input("αβγ"); // Greek

    assert_eq!(editor.get_text(), "Hello Äö 日本語 😀 αβγ");
}

#[test]
fn allows_newline_but_filters_other_c0() {
    let mut editor = Editor::new(default_editor_theme());

    // Newline (0x0A) is explicitly allowed and creates a new line
    // But other C0 chars should be filtered
    editor.handle_input("\x1b[200~line1\n\x08line2\x1b[201~"); // \x08 is backspace char

    // Should have two lines, with backspace filtered
    assert_eq!(editor.get_text(), "line1\nline2");
}
