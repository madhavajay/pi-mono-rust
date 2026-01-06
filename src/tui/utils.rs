use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

fn is_pure_ascii_printable(text: &str) -> bool {
    text.bytes().all(|byte| (0x20..=0x7e).contains(&byte))
}

fn extract_ansi_code(text: &str, pos: usize) -> Option<(String, usize)> {
    let bytes = text.as_bytes();
    if pos >= bytes.len() || bytes[pos] != b'\x1b' || pos + 1 >= bytes.len() {
        return None;
    }
    if bytes[pos + 1] != b'[' {
        return None;
    }
    let mut j = pos + 2;
    while j < bytes.len() {
        let byte = bytes[j];
        if matches!(byte, b'm' | b'G' | b'K' | b'H' | b'J') {
            j += 1;
            return Some((text[pos..j].to_string(), j - pos));
        }
        j += 1;
    }
    None
}

fn strip_ansi_codes(text: &str, expand_tabs: bool) -> String {
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
                if next == ']' {
                    chars.next();
                    for c in chars.by_ref() {
                        if c == '\x07' {
                            break;
                        }
                    }
                    continue;
                }
            }
        }
        if ch == '\t' && expand_tabs {
            output.push_str("   ");
        } else {
            output.push(ch);
        }
    }
    output
}

fn grapheme_width(grapheme: &str) -> usize {
    UnicodeWidthStr::width(grapheme)
}

pub fn visible_width(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }

    if is_pure_ascii_printable(text) {
        return text.len();
    }

    let clean = strip_ansi_codes(text, true);
    UnicodeSegmentation::graphemes(clean.as_str(), true)
        .map(grapheme_width)
        .sum()
}

fn update_tracker_from_text(text: &str, tracker: &mut AnsiCodeTracker) {
    let mut i = 0;
    while i < text.len() {
        if let Some((code, len)) = extract_ansi_code(text, i) {
            tracker.process(&code);
            i += len;
        } else {
            let ch = text[i..].chars().next().unwrap();
            i += ch.len_utf8();
        }
    }
}

#[derive(Default)]
struct AnsiCodeTracker {
    bold: bool,
    dim: bool,
    italic: bool,
    underline: bool,
    blink: bool,
    inverse: bool,
    hidden: bool,
    strikethrough: bool,
    fg_color: Option<String>,
    bg_color: Option<String>,
}

impl AnsiCodeTracker {
    fn process(&mut self, ansi_code: &str) {
        if !ansi_code.ends_with('m') {
            return;
        }

        let params = ansi_code
            .strip_prefix("\x1b[")
            .and_then(|rest| rest.strip_suffix('m'))
            .unwrap_or("");

        if params.is_empty() || params == "0" {
            self.reset();
            return;
        }

        let parts: Vec<&str> = params.split(';').collect();
        let mut i = 0;
        while i < parts.len() {
            let code = parts[i].parse::<u32>().unwrap_or(0);
            if code == 38 || code == 48 {
                if parts.get(i + 1) == Some(&"5") && parts.get(i + 2).is_some() {
                    let color_code = format!("{};{};{}", parts[i], parts[i + 1], parts[i + 2]);
                    if code == 38 {
                        self.fg_color = Some(color_code);
                    } else {
                        self.bg_color = Some(color_code);
                    }
                    i += 3;
                    continue;
                }
                if parts.get(i + 1) == Some(&"2") && parts.get(i + 4).is_some() {
                    let color_code = format!(
                        "{};{};{};{};{}",
                        parts[i],
                        parts[i + 1],
                        parts[i + 2],
                        parts[i + 3],
                        parts[i + 4]
                    );
                    if code == 38 {
                        self.fg_color = Some(color_code);
                    } else {
                        self.bg_color = Some(color_code);
                    }
                    i += 5;
                    continue;
                }
            }

            match code {
                0 => self.reset(),
                1 => self.bold = true,
                2 => self.dim = true,
                3 => self.italic = true,
                4 => self.underline = true,
                5 => self.blink = true,
                7 => self.inverse = true,
                8 => self.hidden = true,
                9 => self.strikethrough = true,
                21 => self.bold = false,
                22 => {
                    self.bold = false;
                    self.dim = false;
                }
                23 => self.italic = false,
                24 => self.underline = false,
                25 => self.blink = false,
                27 => self.inverse = false,
                28 => self.hidden = false,
                29 => self.strikethrough = false,
                39 => self.fg_color = None,
                49 => self.bg_color = None,
                _ => {
                    if (30..=37).contains(&code) || (90..=97).contains(&code) {
                        self.fg_color = Some(code.to_string());
                    } else if (40..=47).contains(&code) || (100..=107).contains(&code) {
                        self.bg_color = Some(code.to_string());
                    }
                }
            }
            i += 1;
        }
    }

    fn reset(&mut self) {
        self.bold = false;
        self.dim = false;
        self.italic = false;
        self.underline = false;
        self.blink = false;
        self.inverse = false;
        self.hidden = false;
        self.strikethrough = false;
        self.fg_color = None;
        self.bg_color = None;
    }

    fn get_active_codes(&self) -> String {
        let mut codes = Vec::new();
        if self.bold {
            codes.push("1");
        }
        if self.dim {
            codes.push("2");
        }
        if self.italic {
            codes.push("3");
        }
        if self.underline {
            codes.push("4");
        }
        if self.blink {
            codes.push("5");
        }
        if self.inverse {
            codes.push("7");
        }
        if self.hidden {
            codes.push("8");
        }
        if self.strikethrough {
            codes.push("9");
        }
        if let Some(ref fg) = self.fg_color {
            codes.push(fg);
        }
        if let Some(ref bg) = self.bg_color {
            codes.push(bg);
        }

        if codes.is_empty() {
            return String::new();
        }

        format!("\x1b[{}m", codes.join(";"))
    }

    fn get_line_end_reset(&self) -> &'static str {
        if self.underline {
            "\x1b[24m"
        } else {
            ""
        }
    }
}

fn split_into_tokens_with_ansi(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut pending_ansi = String::new();
    let mut in_whitespace = false;
    let mut i = 0;

    while i < text.len() {
        if let Some((code, len)) = extract_ansi_code(text, i) {
            pending_ansi.push_str(&code);
            i += len;
            continue;
        }

        let ch = text[i..].chars().next().unwrap();
        let char_is_space = ch == ' ';

        if char_is_space != in_whitespace && !current.is_empty() {
            tokens.push(current);
            current = String::new();
        }

        if !pending_ansi.is_empty() {
            current.push_str(&pending_ansi);
            pending_ansi.clear();
        }

        in_whitespace = char_is_space;
        current.push(ch);
        i += ch.len_utf8();
    }

    if !pending_ansi.is_empty() {
        current.push_str(&pending_ansi);
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

#[derive(Debug)]
enum Segment {
    Ansi(String),
    Grapheme(String),
}

fn split_into_segments(text: &str) -> Vec<Segment> {
    let mut segments = Vec::new();
    let mut i = 0;

    while i < text.len() {
        if let Some((code, len)) = extract_ansi_code(text, i) {
            segments.push(Segment::Ansi(code));
            i += len;
            continue;
        }

        let mut end = i;
        while end < text.len() {
            if extract_ansi_code(text, end).is_some() {
                break;
            }
            let ch = text[end..].chars().next().unwrap();
            end += ch.len_utf8();
        }

        let portion = &text[i..end];
        for grapheme in UnicodeSegmentation::graphemes(portion, true) {
            if grapheme.is_empty() {
                continue;
            }
            segments.push(Segment::Grapheme(grapheme.to_string()));
        }
        i = end;
    }

    segments
}

fn break_long_word(word: &str, width: usize, tracker: &mut AnsiCodeTracker) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current_line = tracker.get_active_codes();
    let mut current_width = 0;

    for segment in split_into_segments(word) {
        match segment {
            Segment::Ansi(code) => {
                current_line.push_str(&code);
                tracker.process(&code);
            }
            Segment::Grapheme(grapheme) => {
                let grapheme_width = visible_width(&grapheme);
                if current_width + grapheme_width > width {
                    let line_end_reset = tracker.get_line_end_reset();
                    if !line_end_reset.is_empty() {
                        current_line.push_str(line_end_reset);
                    }
                    lines.push(current_line);
                    current_line = tracker.get_active_codes();
                    current_width = 0;
                }
                current_line.push_str(&grapheme);
                current_width += grapheme_width;
            }
        }
    }

    if !current_line.is_empty() {
        lines.push(current_line);
    }

    if lines.is_empty() {
        lines.push(String::new());
    }

    lines
}

fn wrap_single_line(line: &str, width: usize) -> Vec<String> {
    if line.is_empty() {
        return vec![String::new()];
    }

    let visible_length = visible_width(line);
    if visible_length <= width {
        return vec![line.to_string()];
    }

    let mut wrapped = Vec::new();
    let mut tracker = AnsiCodeTracker::default();
    let tokens = split_into_tokens_with_ansi(line);

    let mut current_line = String::new();
    let mut current_visible_length = 0;

    for token in tokens {
        let token_visible_length = visible_width(&token);
        let token_clean = strip_ansi_codes(&token, true);
        let is_whitespace = token_clean.trim().is_empty();

        if token_visible_length > width && !is_whitespace {
            if !current_line.is_empty() {
                let line_end_reset = tracker.get_line_end_reset();
                if !line_end_reset.is_empty() {
                    current_line.push_str(line_end_reset);
                }
                wrapped.push(current_line);
            }

            let broken = break_long_word(&token, width, &mut tracker);
            if broken.len() > 1 {
                for part in &broken[..broken.len() - 1] {
                    wrapped.push(part.clone());
                }
            }
            current_line = broken.last().cloned().unwrap_or_default();
            current_visible_length = visible_width(&current_line);
            continue;
        }

        let total_needed = current_visible_length + token_visible_length;
        if total_needed > width && current_visible_length > 0 {
            let mut line_to_wrap = current_line.trim_end().to_string();
            let line_end_reset = tracker.get_line_end_reset();
            if !line_end_reset.is_empty() {
                line_to_wrap.push_str(line_end_reset);
            }
            wrapped.push(line_to_wrap);

            if is_whitespace {
                current_line = tracker.get_active_codes();
                current_visible_length = 0;
            } else {
                current_line = tracker.get_active_codes();
                current_line.push_str(&token);
                current_visible_length = token_visible_length;
            }
        } else {
            current_line.push_str(&token);
            current_visible_length += token_visible_length;
        }

        update_tracker_from_text(&token, &mut tracker);
    }

    if !current_line.is_empty() {
        wrapped.push(current_line);
    }

    if wrapped.is_empty() {
        wrapped.push(String::new());
    }

    wrapped
}

pub fn wrap_text_with_ansi(text: &str, width: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }

    let input_lines: Vec<&str> = text.split('\n').collect();
    let mut result = Vec::new();
    let mut tracker = AnsiCodeTracker::default();

    for input_line in input_lines {
        let prefix = if result.is_empty() {
            String::new()
        } else {
            tracker.get_active_codes()
        };
        let line = format!("{}{}", prefix, input_line);
        let wrapped = wrap_single_line(&line, width);
        result.extend(wrapped);
        update_tracker_from_text(input_line, &mut tracker);
    }

    if result.is_empty() {
        result.push(String::new());
    }

    result
}

pub fn is_whitespace_char(ch: char) -> bool {
    ch.is_whitespace()
}

pub fn is_punctuation_char(ch: char) -> bool {
    matches!(
        ch,
        '(' | ')'
            | '{'
            | '}'
            | '['
            | ']'
            | '<'
            | '>'
            | '.'
            | ','
            | ';'
            | ':'
            | '\''
            | '"'
            | '!'
            | '?'
            | '+'
            | '-'
            | '='
            | '*'
            | '/'
            | '\\'
            | '|'
            | '&'
            | '%'
            | '^'
            | '$'
            | '#'
            | '@'
            | '~'
            | '`'
    )
}

pub fn apply_background_to_line<F>(line: &str, width: usize, bg_fn: F) -> String
where
    F: Fn(&str) -> String,
{
    let visible_len = visible_width(line);
    let padding_needed = width.saturating_sub(visible_len);
    let with_padding = format!("{}{}", line, " ".repeat(padding_needed));
    bg_fn(&with_padding)
}

pub fn truncate_to_width(text: &str, max_width: usize) -> String {
    truncate_to_width_with_ellipsis(text, max_width, "...")
}

pub fn truncate_to_width_with_ellipsis(text: &str, max_width: usize, ellipsis: &str) -> String {
    let text_visible_width = visible_width(text);
    if text_visible_width <= max_width {
        return text.to_string();
    }

    let ellipsis_width = visible_width(ellipsis);
    let target_width = max_width.saturating_sub(ellipsis_width);

    if target_width == 0 {
        return ellipsis.chars().take(max_width).collect();
    }

    let mut result = String::new();
    let mut current_width = 0;

    for segment in split_into_segments(text) {
        match segment {
            Segment::Ansi(code) => result.push_str(&code),
            Segment::Grapheme(grapheme) => {
                let grapheme_width = visible_width(&grapheme);
                if current_width + grapheme_width > target_width {
                    break;
                }
                result.push_str(&grapheme);
                current_width += grapheme_width;
            }
        }
    }

    format!("{result}\x1b[0m{ellipsis}")
}
