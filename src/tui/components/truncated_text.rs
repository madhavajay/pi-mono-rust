use crate::tui::utils::{truncate_to_width, visible_width};

pub struct TruncatedText {
    text: String,
    padding_x: usize,
    padding_y: usize,
}

impl TruncatedText {
    pub fn new(text: impl Into<String>, padding_x: usize, padding_y: usize) -> Self {
        Self {
            text: text.into(),
            padding_x,
            padding_y,
        }
    }

    pub fn render(&self, width: usize) -> Vec<String> {
        let mut result = Vec::new();
        let empty_line = " ".repeat(width);

        for _ in 0..self.padding_y {
            result.push(empty_line.clone());
        }

        let available_width = width.saturating_sub(self.padding_x * 2).max(1);

        let single_line_text = match self.text.find('\n') {
            Some(index) => &self.text[..index],
            None => &self.text,
        };

        let display_text = truncate_to_width(single_line_text, available_width);
        let line_with_padding = format!(
            "{}{}{}",
            " ".repeat(self.padding_x),
            display_text,
            " ".repeat(self.padding_x)
        );

        let line_visible_width = visible_width(&line_with_padding);
        let padding_needed = width.saturating_sub(line_visible_width);
        let final_line = format!("{}{}", line_with_padding, " ".repeat(padding_needed));

        result.push(final_line);

        for _ in 0..self.padding_y {
            result.push(empty_line.clone());
        }

        result
    }
}
