use pi::tui::{visible_width, wrap_text_with_ansi};

// Source: packages/tui/test/wrap-ansi.test.ts

#[test]
fn should_not_apply_underline_style_before_the_styled_text() {
    let underline_on = "\x1b[4m";
    let underline_off = "\x1b[24m";
    let url = "https://example.com/very/long/path/that/will/wrap";
    let text = format!("read this thread {underline_on}{url}{underline_off}");

    let wrapped = wrap_text_with_ansi(&text, 40);

    assert_eq!(wrapped[0], "read this thread ");
    assert!(wrapped[1].starts_with(underline_on));
    assert!(wrapped[1].contains("https://"));
}

#[test]
fn should_not_bleed_underline_to_padding_each_line_should_end_with_reset_for_underline_only() {
    let underline_on = "\x1b[4m";
    let underline_off = "\x1b[24m";
    let url = "https://example.com/very/long/path/that/will/definitely/wrap";
    let text = format!("prefix {underline_on}{url}{underline_off} suffix");

    let wrapped = wrap_text_with_ansi(&text, 30);

    for line in wrapped.iter().take(wrapped.len().saturating_sub(1)) {
        if line.contains(underline_on) {
            assert!(line.ends_with(underline_off));
            assert!(!line.ends_with("\x1b[0m"));
        }
    }
}

#[test]
fn should_preserve_background_color_across_wrapped_lines_without_full_reset() {
    let bg_blue = "\x1b[44m";
    let reset = "\x1b[0m";
    let text = format!("{bg_blue}hello world this is blue background text{reset}");

    let wrapped = wrap_text_with_ansi(&text, 15);

    for line in &wrapped {
        assert!(line.contains(bg_blue));
    }

    for line in wrapped.iter().take(wrapped.len().saturating_sub(1)) {
        assert!(!line.ends_with("\x1b[0m"));
    }
}

#[test]
fn should_reset_underline_but_preserve_background_when_wrapping_underlined_text_inside_background()
{
    let underline_on = "\x1b[4m";
    let underline_off = "\x1b[24m";
    let reset = "\x1b[0m";
    let text = format!(
        "\x1b[41mprefix {underline_on}UNDERLINED_CONTENT_THAT_WRAPS{underline_off} suffix{reset}"
    );

    let wrapped = wrap_text_with_ansi(&text, 20);

    for line in &wrapped {
        let has_bg = line.contains("[41m") || line.contains(";41m") || line.contains("[41;");
        assert!(has_bg);
    }

    for line in wrapped.iter().take(wrapped.len().saturating_sub(1)) {
        let has_underline = line.contains("[4m") || line.contains("[4;") || line.contains(";4m");
        if has_underline && !line.contains(underline_off) {
            assert!(line.ends_with(underline_off));
            assert!(!line.ends_with("\x1b[0m"));
        }
    }
}

#[test]
fn should_wrap_plain_text_correctly() {
    let text = "hello world this is a test";
    let wrapped = wrap_text_with_ansi(text, 10);

    assert!(wrapped.len() > 1);
    for line in wrapped {
        assert!(visible_width(&line) <= 10);
    }
}

#[test]
fn should_preserve_color_codes_across_wraps() {
    let red = "\x1b[31m";
    let reset = "\x1b[0m";
    let text = format!("{red}hello world this is red{reset}");

    let wrapped = wrap_text_with_ansi(&text, 10);

    for line in wrapped.iter().skip(1) {
        assert!(line.starts_with(red));
    }

    for line in wrapped.iter().take(wrapped.len().saturating_sub(1)) {
        assert!(!line.ends_with("\x1b[0m"));
    }
}
