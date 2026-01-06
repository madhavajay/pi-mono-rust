use unicode_segmentation::UnicodeSegmentation;

use crate::tui::utils::{is_punctuation_char, is_whitespace_char, visible_width};

#[derive(Clone, Copy)]
pub struct EditorTheme {
    pub border_color: fn(&str) -> String,
}

#[derive(Clone)]
struct EditorState {
    lines: Vec<String>,
    cursor_line: usize,
    cursor_col: usize,
}

struct TextChunk {
    text: String,
    start_index: usize,
    end_index: usize,
}

struct LayoutLine {
    text: String,
    has_cursor: bool,
    cursor_pos: Option<usize>,
}

pub struct Editor {
    state: EditorState,
    theme: EditorTheme,
    last_width: usize,
    history: Vec<String>,
    history_index: i32,
}

impl Editor {
    pub fn new(theme: EditorTheme) -> Self {
        Self {
            state: EditorState {
                lines: vec![String::new()],
                cursor_line: 0,
                cursor_col: 0,
            },
            theme,
            last_width: 80,
            history: Vec::new(),
            history_index: -1,
        }
    }

    pub fn set_theme(&mut self, theme: EditorTheme) {
        self.theme = theme;
    }

    pub fn add_to_history(&mut self, text: &str) {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }
        if self.history.first().map(String::as_str) == Some(trimmed) {
            return;
        }
        self.history.insert(0, trimmed.to_string());
        if self.history.len() > 100 {
            self.history.pop();
        }
    }

    pub fn get_text(&self) -> String {
        self.state.lines.join("\n")
    }

    pub fn get_lines(&self) -> Vec<String> {
        self.state.lines.clone()
    }

    pub fn get_cursor(&self) -> (usize, usize) {
        (self.state.cursor_line, self.state.cursor_col)
    }

    pub fn set_text(&mut self, text: &str) {
        self.history_index = -1;
        self.set_text_internal(text);
    }

    pub fn render(&mut self, width: usize) -> Vec<String> {
        self.last_width = width;
        let horizontal = (self.theme.border_color)("─");
        let border = horizontal.repeat(width);

        let layout_lines = self.layout_text(width);
        let mut result = Vec::with_capacity(layout_lines.len() + 2);
        result.push(border.clone());

        for layout_line in layout_lines {
            let (display_text, line_visible_width) = render_with_cursor(&layout_line, width);
            let padding = " ".repeat(width.saturating_sub(line_visible_width));
            result.push(format!("{display_text}{padding}"));
        }

        result.push(border);
        result
    }

    pub fn handle_input(&mut self, data: &str) {
        match data {
            "\x1b[A" => {
                if self.is_editor_empty()
                    || (self.history_index > -1 && self.is_on_first_visual_line())
                {
                    self.navigate_history(-1);
                } else {
                    self.move_cursor(-1, 0);
                }
            }
            "\x1b[B" => {
                if self.history_index > -1 && self.is_on_last_visual_line() {
                    self.navigate_history(1);
                } else {
                    self.move_cursor(1, 0);
                }
            }
            "\x1b[C" => {
                self.move_cursor(0, 1);
            }
            "\x1b[D" => {
                self.move_cursor(0, -1);
            }
            "\x1b[1;5D" => {
                self.move_word_backwards();
            }
            "\x1b[1;5C" => {
                self.move_word_forwards();
            }
            "\x01" => {
                self.move_to_line_start();
            }
            "\x17" => {
                self.delete_word_backwards();
            }
            "\x1b\x7f" => {
                self.delete_word_backwards();
            }
            "\x7f" => {
                self.handle_backspace();
            }
            "\n" => {
                self.add_new_line();
            }
            _ => {
                self.insert_text(data);
            }
        }
    }

    fn set_text_internal(&mut self, text: &str) {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        let mut lines: Vec<String> = normalized.split('\n').map(String::from).collect();
        if lines.is_empty() {
            lines.push(String::new());
        }
        self.state.lines = lines;
        self.state.cursor_line = self.state.lines.len() - 1;
        self.state.cursor_col = self.state.lines[self.state.cursor_line].len();
    }

    fn insert_text(&mut self, text: &str) {
        self.history_index = -1;
        for grapheme in UnicodeSegmentation::graphemes(text, true) {
            if grapheme == "\n" {
                self.add_new_line();
            } else if !grapheme.is_empty() {
                self.insert_character(grapheme);
            }
        }
    }

    fn insert_character(&mut self, text: &str) {
        let current_line = self.state.lines[self.state.cursor_line].clone();
        let before = &current_line[..self.state.cursor_col];
        let after = &current_line[self.state.cursor_col..];
        let new_line = format!("{before}{text}{after}");
        self.state.lines[self.state.cursor_line] = new_line;
        self.state.cursor_col += text.len();
    }

    fn add_new_line(&mut self) {
        self.history_index = -1;
        let current_line = self.state.lines[self.state.cursor_line].clone();
        let before = current_line[..self.state.cursor_col].to_string();
        let after = current_line[self.state.cursor_col..].to_string();
        self.state.lines[self.state.cursor_line] = before;
        self.state.lines.insert(self.state.cursor_line + 1, after);
        self.state.cursor_line += 1;
        self.state.cursor_col = 0;
    }

    fn handle_backspace(&mut self) {
        self.history_index = -1;
        if self.state.cursor_col > 0 {
            let current_line = self.state.lines[self.state.cursor_line].clone();
            let before_cursor = &current_line[..self.state.cursor_col];
            let graphemes: Vec<&str> =
                UnicodeSegmentation::graphemes(before_cursor, true).collect();
            let last_len = graphemes.last().map(|g| g.len()).unwrap_or(1);
            let before = &current_line[..self.state.cursor_col - last_len];
            let after = &current_line[self.state.cursor_col..];
            self.state.lines[self.state.cursor_line] = format!("{before}{after}");
            self.state.cursor_col -= last_len;
        } else if self.state.cursor_line > 0 {
            let current_line = self.state.lines.remove(self.state.cursor_line);
            self.state.cursor_line -= 1;
            let previous_line = self.state.lines[self.state.cursor_line].clone();
            self.state.lines[self.state.cursor_line] = format!("{previous_line}{current_line}");
            self.state.cursor_col = previous_line.len();
        }
    }

    fn delete_word_backwards(&mut self) {
        self.history_index = -1;
        let current_line = self.state.lines[self.state.cursor_line].clone();
        if self.state.cursor_col == 0 {
            if self.state.cursor_line > 0 {
                let line = self.state.lines.remove(self.state.cursor_line);
                self.state.cursor_line -= 1;
                let prev_line = self.state.lines[self.state.cursor_line].clone();
                self.state.lines[self.state.cursor_line] = format!("{prev_line}{line}");
                self.state.cursor_col = prev_line.len();
            }
            return;
        }

        let old_cursor = self.state.cursor_col;
        self.move_word_backwards();
        let delete_from = self.state.cursor_col;
        self.state.cursor_col = old_cursor;
        let before = &current_line[..delete_from];
        let after = &current_line[self.state.cursor_col..];
        self.state.lines[self.state.cursor_line] = format!("{before}{after}");
        self.state.cursor_col = delete_from;
    }

    fn move_to_line_start(&mut self) {
        self.state.cursor_col = 0;
    }

    fn move_cursor(&mut self, delta_line: i32, delta_col: i32) {
        if delta_line != 0 {
            let visual_lines = self.build_visual_line_map(self.last_width);
            let current_visual_line = self.find_current_visual_line(&visual_lines);
            let current_vl = &visual_lines[current_visual_line];
            let visual_col = self.state.cursor_col.saturating_sub(current_vl.start_col);
            let target_visual_line = current_visual_line as i32 + delta_line;
            if target_visual_line >= 0 && (target_visual_line as usize) < visual_lines.len() {
                let target_vl = &visual_lines[target_visual_line as usize];
                self.state.cursor_line = target_vl.logical_line;
                let target_col = target_vl.start_col + visual_col.min(target_vl.length);
                let logical_line = &self.state.lines[target_vl.logical_line];
                self.state.cursor_col = target_col.min(logical_line.len());
            }
        }

        if delta_col != 0 {
            let current_line = self.state.lines[self.state.cursor_line].clone();
            if delta_col > 0 {
                if self.state.cursor_col < current_line.len() {
                    let after_cursor = &current_line[self.state.cursor_col..];
                    let mut graphemes = UnicodeSegmentation::graphemes(after_cursor, true);
                    if let Some(first) = graphemes.next() {
                        self.state.cursor_col += first.len();
                    } else {
                        self.state.cursor_col += 1;
                    }
                } else if self.state.cursor_line + 1 < self.state.lines.len() {
                    self.state.cursor_line += 1;
                    self.state.cursor_col = 0;
                }
            } else if self.state.cursor_col > 0 {
                let before_cursor = &current_line[..self.state.cursor_col];
                let graphemes: Vec<&str> =
                    UnicodeSegmentation::graphemes(before_cursor, true).collect();
                if let Some(last) = graphemes.last() {
                    self.state.cursor_col -= last.len();
                } else {
                    self.state.cursor_col = self.state.cursor_col.saturating_sub(1);
                }
            } else if self.state.cursor_line > 0 {
                self.state.cursor_line -= 1;
                self.state.cursor_col = self.state.lines[self.state.cursor_line].len();
            }
        }
    }

    fn move_word_backwards(&mut self) {
        let current_line = self.state.lines[self.state.cursor_line].clone();

        if self.state.cursor_col == 0 {
            if self.state.cursor_line > 0 {
                self.state.cursor_line -= 1;
                self.state.cursor_col = self.state.lines[self.state.cursor_line].len();
            }
            return;
        }

        let before_cursor = &current_line[..self.state.cursor_col];
        let mut graphemes: Vec<&str> =
            UnicodeSegmentation::graphemes(before_cursor, true).collect();
        let mut new_col = self.state.cursor_col;

        while let Some(last) = graphemes.last() {
            if is_whitespace_grapheme(last) {
                new_col -= last.len();
                graphemes.pop();
            } else {
                break;
            }
        }

        if let Some(last) = graphemes.last() {
            if is_punctuation_grapheme(last) {
                while let Some(grapheme) = graphemes.last() {
                    if is_punctuation_grapheme(grapheme) {
                        new_col -= grapheme.len();
                        graphemes.pop();
                    } else {
                        break;
                    }
                }
            } else {
                while let Some(grapheme) = graphemes.last() {
                    if !is_whitespace_grapheme(grapheme) && !is_punctuation_grapheme(grapheme) {
                        new_col -= grapheme.len();
                        graphemes.pop();
                    } else {
                        break;
                    }
                }
            }
        }

        self.state.cursor_col = new_col;
    }

    fn move_word_forwards(&mut self) {
        let current_line = self.state.lines[self.state.cursor_line].clone();

        if self.state.cursor_col >= current_line.len() {
            if self.state.cursor_line + 1 < self.state.lines.len() {
                self.state.cursor_line += 1;
                self.state.cursor_col = 0;
            }
            return;
        }

        let after_cursor = &current_line[self.state.cursor_col..];
        let mut iter = UnicodeSegmentation::graphemes(after_cursor, true);

        let mut next = iter.next();
        while let Some(grapheme) = next {
            if is_whitespace_grapheme(grapheme) {
                self.state.cursor_col += grapheme.len();
                next = iter.next();
            } else {
                break;
            }
        }

        if let Some(first) = next {
            if is_punctuation_grapheme(first) {
                let mut current = Some(first);
                while let Some(grapheme) = current {
                    if is_punctuation_grapheme(grapheme) {
                        self.state.cursor_col += grapheme.len();
                        current = iter.next();
                    } else {
                        break;
                    }
                }
            } else {
                let mut current = Some(first);
                while let Some(grapheme) = current {
                    if !is_whitespace_grapheme(grapheme) && !is_punctuation_grapheme(grapheme) {
                        self.state.cursor_col += grapheme.len();
                        current = iter.next();
                    } else {
                        break;
                    }
                }
            }
        }
    }

    fn build_visual_line_map(&self, width: usize) -> Vec<VisualLineMapping> {
        let mut visual_lines = Vec::new();
        for (index, line) in self.state.lines.iter().enumerate() {
            let line_width = visible_width(line);
            if line.is_empty() {
                visual_lines.push(VisualLineMapping {
                    logical_line: index,
                    start_col: 0,
                    length: 0,
                });
            } else if line_width <= width {
                visual_lines.push(VisualLineMapping {
                    logical_line: index,
                    start_col: 0,
                    length: line.len(),
                });
            } else {
                let chunks = word_wrap_line(line, width);
                for chunk in chunks {
                    visual_lines.push(VisualLineMapping {
                        logical_line: index,
                        start_col: chunk.start_index,
                        length: chunk.end_index.saturating_sub(chunk.start_index),
                    });
                }
            }
        }
        visual_lines
    }

    fn find_current_visual_line(&self, visual_lines: &[VisualLineMapping]) -> usize {
        for (index, line) in visual_lines.iter().enumerate() {
            if line.logical_line == self.state.cursor_line {
                let col_in_segment = self.state.cursor_col as i64 - line.start_col as i64;
                let is_last_segment = index + 1 == visual_lines.len()
                    || visual_lines[index + 1].logical_line != line.logical_line;
                if col_in_segment >= 0
                    && (col_in_segment < line.length as i64
                        || (is_last_segment && col_in_segment <= line.length as i64))
                {
                    return index;
                }
            }
        }
        visual_lines.len().saturating_sub(1)
    }

    fn layout_text(&self, content_width: usize) -> Vec<LayoutLine> {
        if self.state.lines.is_empty()
            || (self.state.lines.len() == 1 && self.state.lines[0].is_empty())
        {
            return vec![LayoutLine {
                text: String::new(),
                has_cursor: true,
                cursor_pos: Some(0),
            }];
        }

        let mut layout_lines = Vec::new();
        for (index, line) in self.state.lines.iter().enumerate() {
            let is_current_line = index == self.state.cursor_line;
            let line_width = visible_width(line);
            if line_width <= content_width {
                let cursor_pos = if is_current_line {
                    Some(self.state.cursor_col)
                } else {
                    None
                };
                layout_lines.push(LayoutLine {
                    text: line.clone(),
                    has_cursor: is_current_line,
                    cursor_pos,
                });
            } else {
                let chunks = word_wrap_line(line, content_width);
                for (chunk_index, chunk) in chunks.iter().enumerate() {
                    let cursor_pos = self.state.cursor_col;
                    let is_last_chunk = chunk_index + 1 == chunks.len();
                    let mut has_cursor = false;
                    let mut adjusted_cursor_pos = 0;
                    if is_current_line {
                        if is_last_chunk {
                            has_cursor = cursor_pos >= chunk.start_index;
                            if has_cursor {
                                adjusted_cursor_pos = cursor_pos.saturating_sub(chunk.start_index);
                            }
                        } else if cursor_pos >= chunk.start_index && cursor_pos < chunk.end_index {
                            has_cursor = true;
                            adjusted_cursor_pos = cursor_pos.saturating_sub(chunk.start_index);
                            if adjusted_cursor_pos > chunk.text.len() {
                                adjusted_cursor_pos = chunk.text.len();
                            }
                        }
                    }
                    layout_lines.push(LayoutLine {
                        text: chunk.text.clone(),
                        has_cursor,
                        cursor_pos: if has_cursor {
                            Some(adjusted_cursor_pos)
                        } else {
                            None
                        },
                    });
                }
            }
        }
        layout_lines
    }

    fn is_editor_empty(&self) -> bool {
        self.state.lines.len() == 1 && self.state.lines[0].is_empty()
    }

    fn is_on_first_visual_line(&self) -> bool {
        let visual_lines = self.build_visual_line_map(self.last_width);
        self.find_current_visual_line(&visual_lines) == 0
    }

    fn is_on_last_visual_line(&self) -> bool {
        let visual_lines = self.build_visual_line_map(self.last_width);
        let current = self.find_current_visual_line(&visual_lines);
        current + 1 == visual_lines.len()
    }

    fn navigate_history(&mut self, direction: i32) {
        if self.history.is_empty() {
            return;
        }
        let new_index = self.history_index - direction;
        if new_index < -1 || new_index >= self.history.len() as i32 {
            return;
        }
        self.history_index = new_index;
        if self.history_index == -1 {
            self.set_text_internal("");
        } else {
            let value = self.history[self.history_index as usize].clone();
            self.set_text_internal(&value);
        }
    }
}

struct VisualLineMapping {
    logical_line: usize,
    start_col: usize,
    length: usize,
}

fn is_whitespace_grapheme(grapheme: &str) -> bool {
    grapheme.chars().all(is_whitespace_char)
}

fn is_punctuation_grapheme(grapheme: &str) -> bool {
    grapheme.chars().all(is_punctuation_char)
}

fn word_wrap_line(line: &str, max_width: usize) -> Vec<TextChunk> {
    if line.is_empty() || max_width == 0 {
        return vec![TextChunk {
            text: String::new(),
            start_index: 0,
            end_index: 0,
        }];
    }

    if visible_width(line) <= max_width {
        return vec![TextChunk {
            text: line.to_string(),
            start_index: 0,
            end_index: line.len(),
        }];
    }

    let mut tokens: Vec<Token> = Vec::new();
    let mut current_token = String::new();
    let mut token_start = 0;
    let mut in_whitespace = false;
    let mut char_index = 0;

    for grapheme in UnicodeSegmentation::graphemes(line, true) {
        let grapheme_is_whitespace = is_whitespace_grapheme(grapheme);
        if current_token.is_empty() {
            in_whitespace = grapheme_is_whitespace;
            token_start = char_index;
        } else if grapheme_is_whitespace != in_whitespace {
            tokens.push(Token {
                text: current_token.clone(),
                start_index: token_start,
                end_index: char_index,
                is_whitespace: in_whitespace,
            });
            current_token.clear();
            token_start = char_index;
            in_whitespace = grapheme_is_whitespace;
        }

        current_token.push_str(grapheme);
        char_index += grapheme.len();
    }

    if !current_token.is_empty() {
        tokens.push(Token {
            text: current_token,
            start_index: token_start,
            end_index: char_index,
            is_whitespace: in_whitespace,
        });
    }

    let mut chunks: Vec<TextChunk> = Vec::new();
    let mut current_chunk = String::new();
    let mut current_width = 0;
    let mut chunk_start_index = 0;
    let mut at_line_start = true;

    for token in tokens {
        let token_width = visible_width(&token.text);
        if at_line_start && token.is_whitespace {
            chunk_start_index = token.end_index;
            continue;
        }
        at_line_start = false;

        if token_width > max_width {
            if !current_chunk.is_empty() {
                chunks.push(TextChunk {
                    text: current_chunk.clone(),
                    start_index: chunk_start_index,
                    end_index: token.start_index,
                });
                current_chunk.clear();
                current_width = 0;
                chunk_start_index = token.start_index;
            }

            let mut token_chunk = String::new();
            let mut token_chunk_width = 0;
            let mut token_chunk_start = token.start_index;
            let mut token_char_index = token.start_index;

            for grapheme in UnicodeSegmentation::graphemes(token.text.as_str(), true) {
                let grapheme_width = visible_width(grapheme);
                if token_chunk_width + grapheme_width > max_width && !token_chunk.is_empty() {
                    chunks.push(TextChunk {
                        text: token_chunk.clone(),
                        start_index: token_chunk_start,
                        end_index: token_char_index,
                    });
                    token_chunk.clear();
                    token_chunk.push_str(grapheme);
                    token_chunk_width = grapheme_width;
                    token_chunk_start = token_char_index;
                } else {
                    token_chunk.push_str(grapheme);
                    token_chunk_width += grapheme_width;
                }
                token_char_index += grapheme.len();
            }

            if !token_chunk.is_empty() {
                current_chunk = token_chunk;
                current_width = token_chunk_width;
                chunk_start_index = token_chunk_start;
            }
            continue;
        }

        if current_width + token_width > max_width {
            let trimmed = current_chunk.trim_end();
            if !trimmed.is_empty() || chunks.is_empty() {
                chunks.push(TextChunk {
                    text: trimmed.to_string(),
                    start_index: chunk_start_index,
                    end_index: chunk_start_index + current_chunk.len(),
                });
            }

            at_line_start = true;
            if token.is_whitespace {
                current_chunk.clear();
                current_width = 0;
                chunk_start_index = token.end_index;
            } else {
                current_chunk = token.text;
                current_width = token_width;
                chunk_start_index = token.start_index;
                at_line_start = false;
            }
        } else {
            current_chunk.push_str(&token.text);
            current_width += token_width;
        }
    }

    if !current_chunk.is_empty() {
        chunks.push(TextChunk {
            text: current_chunk,
            start_index: chunk_start_index,
            end_index: line.len(),
        });
    }

    if chunks.is_empty() {
        chunks.push(TextChunk {
            text: String::new(),
            start_index: 0,
            end_index: 0,
        });
    }

    chunks
}

struct Token {
    text: String,
    start_index: usize,
    end_index: usize,
    is_whitespace: bool,
}

fn render_with_cursor(layout_line: &LayoutLine, width: usize) -> (String, usize) {
    let mut display_text = layout_line.text.clone();
    let mut line_visible_width = visible_width(&layout_line.text);

    if layout_line.has_cursor {
        let cursor_pos = layout_line.cursor_pos.unwrap_or(display_text.len());
        let before = &display_text[..cursor_pos];
        let after = &display_text[cursor_pos..];
        if !after.is_empty() {
            let mut graphemes = UnicodeSegmentation::graphemes(after, true);
            if let Some(first) = graphemes.next() {
                let rest = &after[first.len()..];
                display_text = format!("{before}\x1b[7m{first}\x1b[0m{rest}");
            }
        } else if line_visible_width < width {
            display_text = format!("{before}\x1b[7m \x1b[0m");
            line_visible_width += 1;
        } else {
            let graphemes: Vec<&str> = UnicodeSegmentation::graphemes(before, true).collect();
            if let Some(last) = graphemes.last() {
                let mut before_without_last = String::new();
                for grapheme in &graphemes[..graphemes.len() - 1] {
                    before_without_last.push_str(grapheme);
                }
                display_text = format!("{before_without_last}\x1b[7m{last}\x1b[0m");
            }
        }
    }

    (display_text, line_visible_width)
}
