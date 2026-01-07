use pi::tui::{visible_width, TruncatedText};

// Source: packages/tui/test/truncated-text.test.ts

#[test]
fn pads_output_lines_to_exactly_match_width() {
    let text = TruncatedText::new("Hello world", 1, 0);
    let lines = text.render(50);

    assert_eq!(lines.len(), 1);
    assert_eq!(visible_width(&lines[0]), 50);
}

#[test]
fn pads_output_with_vertical_padding_lines_to_width() {
    let text = TruncatedText::new("Hello", 0, 2);
    let lines = text.render(40);

    assert_eq!(lines.len(), 5);
    for line in &lines {
        assert_eq!(visible_width(line), 40);
    }
}

#[test]
fn truncates_long_text_and_pads_to_width() {
    let long_text =
        "This is a very long piece of text that will definitely exceed the available width";
    let text = TruncatedText::new(long_text, 1, 0);
    let lines = text.render(30);

    assert_eq!(lines.len(), 1);
    assert_eq!(visible_width(&lines[0]), 30);
    let stripped = strip_ansi(&lines[0]);
    assert!(stripped.contains("..."));
}

#[test]
fn preserves_ansi_codes_in_output_and_pads_correctly() {
    let styled_text = "\x1b[31mHello\x1b[0m \x1b[34mworld\x1b[0m";
    let text = TruncatedText::new(styled_text, 1, 0);
    let lines = text.render(40);

    assert_eq!(lines.len(), 1);
    assert_eq!(visible_width(&lines[0]), 40);
    assert!(lines[0].contains("\x1b["));
}

#[test]
fn truncates_styled_text_and_adds_reset_code_before_ellipsis() {
    let long_styled_text = "\x1b[31mThis is a very long red text that will be truncated\x1b[0m";
    let text = TruncatedText::new(long_styled_text, 1, 0);
    let lines = text.render(20);

    assert_eq!(lines.len(), 1);
    assert_eq!(visible_width(&lines[0]), 20);
    assert!(lines[0].contains("\x1b[0m..."));
}

#[test]
fn handles_text_that_fits_exactly() {
    let text = TruncatedText::new("Hello world", 1, 0);
    let lines = text.render(30);

    assert_eq!(lines.len(), 1);
    assert_eq!(visible_width(&lines[0]), 30);
    let stripped = strip_ansi(&lines[0]);
    assert!(!stripped.contains("..."));
}

#[test]
fn handles_empty_text() {
    let text = TruncatedText::new("", 1, 0);
    let lines = text.render(30);

    assert_eq!(lines.len(), 1);
    assert_eq!(visible_width(&lines[0]), 30);
}

#[test]
fn stops_at_newline_and_only_shows_first_line() {
    let multiline_text = "First line\nSecond line\nThird line";
    let text = TruncatedText::new(multiline_text, 1, 0);
    let lines = text.render(40);

    assert_eq!(lines.len(), 1);
    assert_eq!(visible_width(&lines[0]), 40);
    let stripped = strip_ansi(&lines[0]).trim().to_string();
    assert!(stripped.contains("First line"));
    assert!(!stripped.contains("Second line"));
    assert!(!stripped.contains("Third line"));
}

#[test]
fn truncates_first_line_even_with_newlines_in_text() {
    let long_multiline_text = "This is a very long first line that needs truncation\nSecond line";
    let text = TruncatedText::new(long_multiline_text, 1, 0);
    let lines = text.render(25);

    assert_eq!(lines.len(), 1);
    assert_eq!(visible_width(&lines[0]), 25);
    let stripped = strip_ansi(&lines[0]);
    assert!(stripped.contains("..."));
    assert!(!stripped.contains("Second line"));
}

fn strip_ansi(text: &str) -> String {
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
