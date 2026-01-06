use pi::tui::{visible_width, DefaultTextStyle, Markdown, MarkdownTheme};

struct TestMarkdownTheme;

fn wrap(code: &str, text: &str) -> String {
    format!("\x1b[{}m{}\x1b[0m", code, text)
}

impl MarkdownTheme for TestMarkdownTheme {
    fn heading(&self, text: &str) -> String {
        wrap("1;36", text)
    }
    fn link(&self, text: &str) -> String {
        wrap("34", text)
    }
    fn link_url(&self, text: &str) -> String {
        wrap("2", text)
    }
    fn code(&self, text: &str) -> String {
        wrap("33", text)
    }
    fn code_block(&self, text: &str) -> String {
        wrap("32", text)
    }
    fn code_block_border(&self, text: &str) -> String {
        wrap("2", text)
    }
    fn quote(&self, text: &str) -> String {
        wrap("3", text)
    }
    fn quote_border(&self, text: &str) -> String {
        wrap("2", text)
    }
    fn hr(&self, text: &str) -> String {
        wrap("2", text)
    }
    fn list_bullet(&self, text: &str) -> String {
        wrap("36", text)
    }
    fn bold(&self, text: &str) -> String {
        wrap("1", text)
    }
    fn italic(&self, text: &str) -> String {
        wrap("3", text)
    }
    fn strikethrough(&self, text: &str) -> String {
        wrap("9", text)
    }
    fn underline(&self, text: &str) -> String {
        wrap("4", text)
    }
}

fn strip_ansi(text: &str) -> String {
    let mut output = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            if let Some('[') = chars.peek().copied() {
                chars.next();
                for c in chars.by_ref() {
                    if matches!(c, 'm' | 'G' | 'K' | 'H' | 'J') {
                        break;
                    }
                }
                continue;
            }
        }
        output.push(ch);
    }
    output
}

#[test]
fn should_render_simple_nested_list() {
    let markdown = Markdown::new(
        "- Item 1\n  - Nested 1.1\n  - Nested 1.2\n- Item 2",
        0,
        0,
        Box::new(TestMarkdownTheme),
        None,
    );
    let lines = markdown.render(80);
    assert!(!lines.is_empty());
    let plain: Vec<String> = lines.iter().map(|l| strip_ansi(l)).collect();
    assert!(plain.iter().any(|line| line.contains("- Item 1")));
    assert!(plain.iter().any(|line| line.contains("  - Nested 1.1")));
    assert!(plain.iter().any(|line| line.contains("  - Nested 1.2")));
    assert!(plain.iter().any(|line| line.contains("- Item 2")));
}

#[test]
fn should_render_deeply_nested_list() {
    let markdown = Markdown::new(
        "- Level 1\n  - Level 2\n    - Level 3\n      - Level 4",
        0,
        0,
        Box::new(TestMarkdownTheme),
        None,
    );
    let lines = markdown.render(80);
    let plain: Vec<String> = lines.iter().map(|l| strip_ansi(l)).collect();
    assert!(plain.iter().any(|line| line.contains("- Level 1")));
    assert!(plain.iter().any(|line| line.contains("  - Level 2")));
    assert!(plain.iter().any(|line| line.contains("    - Level 3")));
    assert!(plain.iter().any(|line| line.contains("      - Level 4")));
}

#[test]
fn should_render_ordered_nested_list() {
    let markdown = Markdown::new(
        "1. First\n   1. Nested first\n   2. Nested second\n2. Second",
        0,
        0,
        Box::new(TestMarkdownTheme),
        None,
    );
    let lines = markdown.render(80);
    let plain: Vec<String> = lines.iter().map(|l| strip_ansi(l)).collect();
    assert!(plain.iter().any(|line| line.contains("1. First")));
    assert!(plain.iter().any(|line| line.contains("  1. Nested first")));
    assert!(plain.iter().any(|line| line.contains("  2. Nested second")));
    assert!(plain.iter().any(|line| line.contains("2. Second")));
}

#[test]
fn should_render_mixed_ordered_and_unordered_nested_lists() {
    let markdown = Markdown::new(
        "1. Ordered item\n   - Unordered nested\n   - Another nested\n2. Second ordered\n   - More nested",
        0,
        0,
        Box::new(TestMarkdownTheme),
        None,
    );
    let lines = markdown.render(80);
    let plain: Vec<String> = lines.iter().map(|l| strip_ansi(l)).collect();
    assert!(plain.iter().any(|line| line.contains("1. Ordered item")));
    assert!(plain
        .iter()
        .any(|line| line.contains("  - Unordered nested")));
    assert!(plain.iter().any(|line| line.contains("2. Second ordered")));
}

#[test]
fn should_render_simple_table() {
    let markdown = Markdown::new(
        "| Name | Age |\n| --- | --- |\n| Alice | 30 |\n| Bob | 25 |",
        0,
        0,
        Box::new(TestMarkdownTheme),
        None,
    );
    let lines = markdown.render(80);
    let plain: Vec<String> = lines.iter().map(|l| strip_ansi(l)).collect();
    assert!(plain.iter().any(|line| line.contains("Name")));
    assert!(plain.iter().any(|line| line.contains("Age")));
    assert!(plain.iter().any(|line| line.contains("Alice")));
    assert!(plain.iter().any(|line| line.contains("Bob")));
    assert!(plain.iter().any(|line| line.contains("│")));
    assert!(plain.iter().any(|line| line.contains("─")));
}

#[test]
fn should_render_table_with_alignment() {
    let markdown = Markdown::new(
        "| Left | Center | Right |\n| :--- | :---: | ---: |\n| A | B | C |\n| Long text | Middle | End |",
        0,
        0,
        Box::new(TestMarkdownTheme),
        None,
    );
    let lines = markdown.render(80);
    let plain: Vec<String> = lines.iter().map(|l| strip_ansi(l)).collect();
    assert!(plain.iter().any(|line| line.contains("Left")));
    assert!(plain.iter().any(|line| line.contains("Center")));
    assert!(plain.iter().any(|line| line.contains("Right")));
    assert!(plain.iter().any(|line| line.contains("Long text")));
}

#[test]
fn should_handle_tables_with_varying_column_widths() {
    let markdown = Markdown::new(
        "| Short | Very long column header |\n| --- | --- |\n| A | This is a much longer cell content |\n| B | Short |",
        0,
        0,
        Box::new(TestMarkdownTheme),
        None,
    );
    let lines = markdown.render(80);
    assert!(!lines.is_empty());
    let plain: Vec<String> = lines.iter().map(|l| strip_ansi(l)).collect();
    assert!(plain
        .iter()
        .any(|line| line.contains("Very long column header")));
    assert!(plain
        .iter()
        .any(|line| line.contains("This is a much longer cell content")));
}

#[test]
fn should_wrap_table_cells_when_table_exceeds_available_width() {
    let markdown = Markdown::new(
        "| Command | Description | Example |\n| --- | --- | --- |\n| npm install | Install all dependencies | npm install |\n| npm run build | Build the project | npm run build |",
        0,
        0,
        Box::new(TestMarkdownTheme),
        None,
    );
    let lines = markdown.render(50);
    let plain: Vec<String> = lines
        .iter()
        .map(|l| strip_ansi(l).trim_end().to_string())
        .collect();
    for line in &plain {
        assert!(visible_width(line) <= 50, "Line exceeds width 50: {line}");
    }
    let all_text = plain.join(" ");
    assert!(all_text.contains("Command"));
    assert!(all_text.contains("Description"));
    assert!(all_text.contains("npm install"));
    assert!(all_text.contains("Install"));
}

#[test]
fn should_wrap_long_cell_content_to_multiple_lines() {
    let markdown = Markdown::new(
        "| Header |\n| --- |\n| This is a very long cell content that should wrap |",
        0,
        0,
        Box::new(TestMarkdownTheme),
        None,
    );
    let lines = markdown.render(25);
    let plain: Vec<String> = lines
        .iter()
        .map(|l| strip_ansi(l).trim_end().to_string())
        .collect();
    let data_rows: Vec<&String> = plain
        .iter()
        .filter(|line| line.starts_with("│") && !line.contains("─"))
        .collect();
    assert!(
        data_rows.len() > 2,
        "Expected wrapped rows, got {}",
        data_rows.len()
    );
    let all_text = plain.join(" ");
    assert!(all_text.contains("very long"));
    assert!(all_text.contains("cell content"));
    assert!(all_text.contains("should wrap"));
}

#[test]
fn should_wrap_long_unbroken_tokens_inside_table_cells_not_only_at_line_start() {
    let url = "https://example.com/this/is/a/very/long/url/that/should/wrap";
    let markdown = Markdown::new(
        format!("| Value |\n| --- |\n| prefix {url} |"),
        0,
        0,
        Box::new(TestMarkdownTheme),
        None,
    );
    let width = 30;
    let lines = markdown.render(width);
    let plain: Vec<String> = lines
        .iter()
        .map(|l| strip_ansi(l).trim_end().to_string())
        .collect();
    for line in &plain {
        assert!(
            visible_width(line) <= width,
            "Line exceeds width {width}: {line}"
        );
    }
    let table_lines: Vec<&String> = plain.iter().filter(|line| line.starts_with("│")).collect();
    for line in table_lines {
        let border_count = line.matches('│').count();
        assert_eq!(
            border_count, 2,
            "Expected 2 borders, got {border_count}: {line}"
        );
    }
    let extracted = plain.join("").replace(['│', '├', '┤', '─', ' '], "");
    assert!(extracted.contains("prefix"));
    assert!(extracted.contains(url));
}

#[test]
fn should_wrap_styled_inline_code_inside_table_cells_without_breaking_borders() {
    let markdown = Markdown::new(
        "| Code |\n| --- |\n| `averyveryveryverylongidentifier` |",
        0,
        0,
        Box::new(TestMarkdownTheme),
        None,
    );
    let width = 20;
    let lines = markdown.render(width);
    let joined_output = lines.join("\n");
    assert!(joined_output.contains("\x1b[33m"));
    let plain: Vec<String> = lines
        .iter()
        .map(|l| strip_ansi(l).trim_end().to_string())
        .collect();
    for line in &plain {
        assert!(
            visible_width(line) <= width,
            "Line exceeds width {width}: {line}"
        );
    }
    let table_lines: Vec<&String> = plain.iter().filter(|line| line.starts_with("│")).collect();
    for line in table_lines {
        let border_count = line.matches('│').count();
        assert_eq!(
            border_count, 2,
            "Expected 2 borders, got {border_count}: {line}"
        );
    }
}

#[test]
fn should_handle_extremely_narrow_width_gracefully() {
    let markdown = Markdown::new(
        "| A | B | C |\n| --- | --- | --- |\n| 1 | 2 | 3 |",
        0,
        0,
        Box::new(TestMarkdownTheme),
        None,
    );
    let lines = markdown.render(15);
    let plain: Vec<String> = lines
        .iter()
        .map(|l| strip_ansi(l).trim_end().to_string())
        .collect();
    assert!(!lines.is_empty());
    for line in &plain {
        assert!(visible_width(line) <= 15, "Line exceeds width 15: {line}");
    }
}

#[test]
fn should_render_table_correctly_when_it_fits_naturally() {
    let markdown = Markdown::new(
        "| A | B |\n| --- | --- |\n| 1 | 2 |",
        0,
        0,
        Box::new(TestMarkdownTheme),
        None,
    );
    let lines = markdown.render(80);
    let plain: Vec<String> = lines
        .iter()
        .map(|l| strip_ansi(l).trim_end().to_string())
        .collect();
    let header = plain
        .iter()
        .find(|line| line.contains("A") && line.contains("B"));
    assert!(header.is_some());
    assert!(header.unwrap().contains("│"));
    let separator = plain
        .iter()
        .find(|line| line.contains("├") && line.contains("┼"));
    assert!(separator.is_some());
    let data = plain
        .iter()
        .find(|line| line.contains("1") && line.contains("2"));
    assert!(data.is_some());
}

#[test]
fn should_respect_paddingx_when_calculating_table_width() {
    let markdown = Markdown::new(
        "| Column One | Column Two |\n| --- | --- |\n| Data 1 | Data 2 |",
        2,
        0,
        Box::new(TestMarkdownTheme),
        None,
    );
    let lines = markdown.render(40);
    let plain: Vec<String> = lines
        .iter()
        .map(|l| strip_ansi(l).trim_end().to_string())
        .collect();
    for line in &plain {
        assert!(visible_width(line) <= 40, "Line exceeds width 40: {line}");
    }
    let table_row = plain.iter().find(|line| line.contains("│"));
    assert!(table_row.is_some());
    assert!(table_row.unwrap().starts_with("  "));
}

#[test]
fn should_render_lists_and_tables_together() {
    let markdown = Markdown::new(
        "# Test Document\n\n- Item 1\n  - Nested item\n- Item 2\n\n| Col1 | Col2 |\n| --- | --- |\n| A | B |",
        0,
        0,
        Box::new(TestMarkdownTheme),
        None,
    );
    let lines = markdown.render(80);
    let plain: Vec<String> = lines.iter().map(|l| strip_ansi(l)).collect();
    assert!(plain.iter().any(|line| line.contains("Test Document")));
    assert!(plain.iter().any(|line| line.contains("- Item 1")));
    assert!(plain.iter().any(|line| line.contains("  - Nested item")));
    assert!(plain.iter().any(|line| line.contains("Col1")));
    assert!(plain.iter().any(|line| line.contains("│")));
}

#[test]
fn should_preserve_gray_italic_styling_after_inline_code() {
    let style = DefaultTextStyle {
        color: Some(Box::new(|text| wrap("90", text))),
        italic: true,
        ..Default::default()
    };
    let markdown = Markdown::new(
        "This is thinking with `inline code` and more text after",
        1,
        0,
        Box::new(TestMarkdownTheme),
        Some(style),
    );
    let lines = markdown.render(80);
    let joined = lines.join("\n");
    assert!(joined.contains("inline code"));
    assert!(joined.contains("\x1b[90m"));
    assert!(joined.contains("\x1b[3m"));
    assert!(joined.contains("\x1b[33m"));
}

#[test]
fn should_preserve_gray_italic_styling_after_bold_text() {
    let style = DefaultTextStyle {
        color: Some(Box::new(|text| wrap("90", text))),
        italic: true,
        ..Default::default()
    };
    let markdown = Markdown::new(
        "This is thinking with **bold text** and more after",
        1,
        0,
        Box::new(TestMarkdownTheme),
        Some(style),
    );
    let lines = markdown.render(80);
    let joined = lines.join("\n");
    assert!(joined.contains("bold text"));
    assert!(joined.contains("\x1b[90m"));
    assert!(joined.contains("\x1b[3m"));
    assert!(joined.contains("\x1b[1m"));
}

#[test]
fn should_have_only_one_blank_line_between_code_block_and_following_paragraph() {
    let markdown = Markdown::new(
        "hello world\n\n```js\nconst hello = \"world\";\n```\n\nagain, hello world",
        0,
        0,
        Box::new(TestMarkdownTheme),
        None,
    );
    let lines = markdown.render(80);
    let plain: Vec<String> = lines
        .iter()
        .map(|l| strip_ansi(l).trim_end().to_string())
        .collect();
    let closing = plain.iter().position(|line| line == "```");
    assert!(closing.is_some());
    let after = &plain[closing.unwrap() + 1..];
    let empty_index = after
        .iter()
        .position(|line| !line.is_empty())
        .unwrap_or(after.len());
    assert_eq!(empty_index, 1);
}

#[test]
fn should_have_only_one_blank_line_between_divider_and_following_paragraph() {
    let markdown = Markdown::new(
        "hello world\n\n---\n\nagain, hello world",
        0,
        0,
        Box::new(TestMarkdownTheme),
        None,
    );
    let lines = markdown.render(80);
    let plain: Vec<String> = lines
        .iter()
        .map(|l| strip_ansi(l).trim_end().to_string())
        .collect();
    let divider = plain.iter().position(|line| line.contains("─"));
    assert!(divider.is_some());
    let after = &plain[divider.unwrap() + 1..];
    let empty_index = after
        .iter()
        .position(|line| !line.is_empty())
        .unwrap_or(after.len());
    assert_eq!(empty_index, 1);
}

#[test]
fn should_have_only_one_blank_line_between_heading_and_following_paragraph() {
    let markdown = Markdown::new(
        "# Hello\n\nThis is a paragraph",
        0,
        0,
        Box::new(TestMarkdownTheme),
        None,
    );
    let lines = markdown.render(80);
    let plain: Vec<String> = lines
        .iter()
        .map(|l| strip_ansi(l).trim_end().to_string())
        .collect();
    let heading = plain.iter().position(|line| line.contains("Hello"));
    assert!(heading.is_some());
    let after = &plain[heading.unwrap() + 1..];
    let empty_index = after
        .iter()
        .position(|line| !line.is_empty())
        .unwrap_or(after.len());
    assert_eq!(empty_index, 1);
}

#[test]
fn should_have_only_one_blank_line_between_blockquote_and_following_paragraph() {
    let markdown = Markdown::new(
        "hello world\n\n> This is a quote\n\nagain, hello world",
        0,
        0,
        Box::new(TestMarkdownTheme),
        None,
    );
    let lines = markdown.render(80);
    let plain: Vec<String> = lines
        .iter()
        .map(|l| strip_ansi(l).trim_end().to_string())
        .collect();
    let quote = plain
        .iter()
        .position(|line| line.contains("This is a quote"));
    assert!(quote.is_some());
    let after = &plain[quote.unwrap() + 1..];
    let empty_index = after
        .iter()
        .position(|line| !line.is_empty())
        .unwrap_or(after.len());
    assert_eq!(empty_index, 1);
}

#[test]
fn should_render_content_with_html_like_tags_as_text() {
    let markdown = Markdown::new(
        "This is text with <thinking>hidden content</thinking> that should be visible",
        0,
        0,
        Box::new(TestMarkdownTheme),
        None,
    );
    let lines = markdown.render(80);
    let joined = strip_ansi(&lines.join(" "));
    assert!(joined.contains("hidden content") || joined.contains("<thinking>"));
}

#[test]
fn should_render_html_tags_in_code_blocks_correctly() {
    let markdown = Markdown::new(
        "```html\n<div>Some HTML</div>\n```",
        0,
        0,
        Box::new(TestMarkdownTheme),
        None,
    );
    let lines = markdown.render(80);
    let joined = strip_ansi(&lines.join("\n"));
    assert!(joined.contains("<div>") && joined.contains("</div>"));
}
