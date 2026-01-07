use crate::tui::utils::{apply_background_to_line, visible_width, wrap_text_with_ansi};
use crate::tui::Component;
use std::any::Any;

type StyleFn = Box<dyn Fn(&str) -> String>;

#[derive(Default)]
pub struct DefaultTextStyle {
    pub color: Option<StyleFn>,
    pub bg_color: Option<StyleFn>,
    pub bold: bool,
    pub italic: bool,
    pub strikethrough: bool,
    pub underline: bool,
}

pub trait MarkdownTheme {
    fn heading(&self, text: &str) -> String;
    fn link(&self, text: &str) -> String;
    fn link_url(&self, text: &str) -> String;
    fn code(&self, text: &str) -> String;
    fn code_block(&self, text: &str) -> String;
    fn code_block_border(&self, text: &str) -> String;
    fn quote(&self, text: &str) -> String;
    fn quote_border(&self, text: &str) -> String;
    fn hr(&self, text: &str) -> String;
    fn list_bullet(&self, text: &str) -> String;
    fn bold(&self, text: &str) -> String;
    fn italic(&self, text: &str) -> String;
    fn strikethrough(&self, text: &str) -> String;
    fn underline(&self, text: &str) -> String;
}

pub struct Markdown {
    text: String,
    padding_x: usize,
    padding_y: usize,
    theme: Box<dyn MarkdownTheme>,
    default_text_style: Option<DefaultTextStyle>,
}

impl Markdown {
    pub fn new(
        text: impl Into<String>,
        padding_x: usize,
        padding_y: usize,
        theme: Box<dyn MarkdownTheme>,
        default_text_style: Option<DefaultTextStyle>,
    ) -> Self {
        Self {
            text: text.into(),
            padding_x,
            padding_y,
            theme,
            default_text_style,
        }
    }

    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
    }

    pub fn render(&self, width: usize) -> Vec<String> {
        self.render_inner(width)
    }

    fn apply_default_style(&self, text: &str) -> String {
        let Some(style) = &self.default_text_style else {
            return text.to_string();
        };

        let mut styled = text.to_string();
        if let Some(color) = &style.color {
            styled = color(&styled);
        }
        if style.bold {
            styled = self.theme.bold(&styled);
        }
        if style.italic {
            styled = self.theme.italic(&styled);
        }
        if style.strikethrough {
            styled = self.theme.strikethrough(&styled);
        }
        if style.underline {
            styled = self.theme.underline(&styled);
        }
        styled
    }

    fn get_default_style_prefix(&self) -> String {
        if self.default_text_style.is_none() {
            return String::new();
        }

        let sentinel = "\u{0}";
        let mut styled = sentinel.to_string();
        if let Some(style) = &self.default_text_style {
            if let Some(color) = &style.color {
                styled = color(&styled);
            }
            if style.bold {
                styled = self.theme.bold(&styled);
            }
            if style.italic {
                styled = self.theme.italic(&styled);
            }
            if style.strikethrough {
                styled = self.theme.strikethrough(&styled);
            }
            if style.underline {
                styled = self.theme.underline(&styled);
            }
        }

        styled
            .find(sentinel)
            .map(|index| styled[..index].to_string())
            .unwrap_or_default()
    }

    fn render_inline(&self, text: &str) -> String {
        let mut result = String::new();
        let mut i = 0;
        let bytes = text.as_bytes();

        while i < bytes.len() {
            if text[i..].starts_with("**") {
                if let Some(end) = text[i + 2..].find("**") {
                    let content = &text[i + 2..i + 2 + end];
                    let rendered = self.theme.bold(content);
                    result.push_str(&rendered);
                    result.push_str(&self.get_default_style_prefix());
                    i += 2 + end + 2;
                    continue;
                }
            }

            if text[i..].starts_with('`') {
                if let Some(end) = text[i + 1..].find('`') {
                    let content = &text[i + 1..i + 1 + end];
                    let rendered = self.theme.code(content);
                    result.push_str(&rendered);
                    result.push_str(&self.get_default_style_prefix());
                    i += 1 + end + 1;
                    continue;
                }
            }

            let ch = text[i..].chars().next().unwrap();
            result.push_str(&self.apply_default_style(&ch.to_string()));
            i += ch.len_utf8();
        }

        result
    }

    fn is_heading(line: &str) -> Option<(usize, String)> {
        let trimmed = line.trim_start();
        if !trimmed.starts_with('#') {
            return None;
        }
        let level = trimmed.chars().take_while(|c| *c == '#').count();
        if level == 0 || level > 6 {
            return None;
        }
        let text = trimmed[level..].trim_start().to_string();
        Some((level, text))
    }

    fn is_hr(line: &str) -> bool {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return false;
        }
        trimmed.chars().all(|c| c == '-') && trimmed.len() >= 3
    }

    fn parse_list_line(line: &str) -> Option<(usize, String, String)> {
        let indent = line.chars().take_while(|c| *c == ' ').count();
        let trimmed = &line[indent..];
        if let Some(rest) = trimmed.strip_prefix("- ") {
            return Some((indent, "- ".to_string(), rest.to_string()));
        }
        if let Some(rest) = trimmed.strip_prefix("* ") {
            return Some((indent, "* ".to_string(), rest.to_string()));
        }
        let mut chars = trimmed.chars();
        let mut digits = String::new();
        while let Some(ch) = chars.next() {
            if ch.is_ascii_digit() {
                digits.push(ch);
            } else {
                let remaining = format!("{}{}", ch, chars.collect::<String>());
                if !digits.is_empty() && remaining.starts_with(". ") {
                    let rest = remaining[2..].to_string();
                    return Some((indent, format!("{digits}. "), rest));
                }
                break;
            }
        }
        None
    }

    fn parse_table_row(line: &str) -> Vec<String> {
        let trimmed = line.trim();
        let mut content = trimmed.to_string();
        if content.starts_with('|') {
            content.remove(0);
        }
        if content.ends_with('|') {
            content.pop();
        }
        content
            .split('|')
            .map(|cell| cell.trim().to_string())
            .collect()
    }

    fn is_table_separator(line: &str) -> bool {
        let trimmed = line.trim();
        if !trimmed.contains('|') {
            return false;
        }
        trimmed
            .chars()
            .all(|ch| ch == '|' || ch == '-' || ch == ':' || ch == ' ')
    }

    fn wrap_cell_text(&self, text: &str, max_width: usize) -> Vec<String> {
        wrap_text_with_ansi(text, max_width.max(1))
    }

    fn render_table(
        &self,
        header: &[String],
        rows: &[Vec<String>],
        raw: &str,
        available_width: usize,
    ) -> Vec<String> {
        let mut lines = Vec::new();
        let num_cols = header.len();
        if num_cols == 0 {
            return lines;
        }

        let border_overhead = 3 * num_cols + 1;
        let min_table_width = border_overhead + num_cols;
        if available_width < min_table_width {
            let mut fallback = wrap_text_with_ansi(raw, available_width);
            fallback.push(String::new());
            return fallback;
        }

        let mut natural_widths = vec![0; num_cols];
        for (i, cell) in header.iter().enumerate() {
            let text = self.render_inline(cell);
            natural_widths[i] = visible_width(&text);
        }
        for row in rows {
            for (i, cell) in row.iter().enumerate() {
                let text = self.render_inline(cell);
                natural_widths[i] = natural_widths[i].max(visible_width(&text));
            }
        }

        let total_natural: usize = natural_widths.iter().sum::<usize>() + border_overhead;
        let mut column_widths = vec![1; num_cols];
        if total_natural <= available_width {
            column_widths = natural_widths;
        } else {
            let available_for_cells = available_width.saturating_sub(border_overhead);
            if available_for_cells <= num_cols {
                column_widths = vec![1; num_cols];
            } else {
                let total = natural_widths.iter().sum::<usize>().max(1);
                for (i, width) in natural_widths.iter().enumerate() {
                    let proportion = *width as f64 / total as f64;
                    column_widths[i] = (proportion * available_for_cells as f64).floor() as usize;
                    if column_widths[i] == 0 {
                        column_widths[i] = 1;
                    }
                }
                let allocated: usize = column_widths.iter().sum();
                let mut remaining = available_for_cells.saturating_sub(allocated);
                for width in &mut column_widths {
                    if remaining == 0 {
                        break;
                    }
                    *width += 1;
                    remaining -= 1;
                }
            }
        }

        let top_cells: Vec<String> = column_widths.iter().map(|w| "─".repeat(*w)).collect();
        lines.push(format!("┌─{}─┐", top_cells.join("─┬─")));

        let header_cells: Vec<Vec<String>> = header
            .iter()
            .enumerate()
            .map(|(i, cell)| {
                let text = self.render_inline(cell);
                self.wrap_cell_text(&text, column_widths[i])
            })
            .collect();
        let header_line_count = header_cells.iter().map(|c| c.len()).max().unwrap_or(1);
        for line_idx in 0..header_line_count {
            let row_parts: Vec<String> = header_cells
                .iter()
                .enumerate()
                .map(|(col_idx, cell_lines)| {
                    let text = cell_lines.get(line_idx).cloned().unwrap_or_default();
                    let padding = column_widths[col_idx].saturating_sub(visible_width(&text));
                    self.theme.bold(&format!("{}{}", text, " ".repeat(padding)))
                })
                .collect();
            lines.push(format!("│ {} │", row_parts.join(" │ ")));
        }

        let sep_cells: Vec<String> = column_widths.iter().map(|w| "─".repeat(*w)).collect();
        lines.push(format!("├─{}─┤", sep_cells.join("─┼─")));

        for row in rows {
            let row_cells: Vec<Vec<String>> = row
                .iter()
                .enumerate()
                .map(|(i, cell)| {
                    let text = self.render_inline(cell);
                    self.wrap_cell_text(&text, column_widths[i])
                })
                .collect();
            let row_line_count = row_cells.iter().map(|c| c.len()).max().unwrap_or(1);
            for line_idx in 0..row_line_count {
                let row_parts: Vec<String> = row_cells
                    .iter()
                    .enumerate()
                    .map(|(col_idx, cell_lines)| {
                        let text = cell_lines.get(line_idx).cloned().unwrap_or_default();
                        let padding = column_widths[col_idx].saturating_sub(visible_width(&text));
                        format!("{}{}", text, " ".repeat(padding))
                    })
                    .collect();
                lines.push(format!("│ {} │", row_parts.join(" │ ")));
            }
        }

        let bottom_cells: Vec<String> = column_widths.iter().map(|w| "─".repeat(*w)).collect();
        lines.push(format!("└─{}─┘", bottom_cells.join("─┴─")));
        lines.push(String::new());
        lines
    }
}

impl Component for Markdown {
    fn render(&self, width: usize) -> Vec<String> {
        self.render_inner(width)
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl Markdown {
    fn render_inner(&self, width: usize) -> Vec<String> {
        let content_width = (width as isize - (self.padding_x as isize * 2)).max(1) as usize;
        if self.text.trim().is_empty() {
            return Vec::new();
        }

        let normalized = self.text.replace('\t', "   ");
        let lines: Vec<&str> = normalized.lines().collect();
        let mut rendered_lines = Vec::new();
        let mut i = 0;

        while i < lines.len() {
            let line = lines[i];
            let next_line = lines.get(i + 1).copied().unwrap_or("");
            if line.trim().is_empty() {
                rendered_lines.push(String::new());
                i += 1;
                continue;
            }

            if line.trim_start().starts_with("```") {
                let lang = line
                    .trim_start()
                    .trim_start_matches("```")
                    .trim()
                    .to_string();
                rendered_lines.push(self.theme.code_block_border(&format!("```{lang}")));
                i += 1;
                while i < lines.len() {
                    let code_line = lines[i];
                    if code_line.trim_start().starts_with("```") {
                        rendered_lines.push(self.theme.code_block_border("```"));
                        i += 1;
                        break;
                    }
                    rendered_lines.push(format!("  {}", self.theme.code_block(code_line)));
                    i += 1;
                }
                let after_block = lines.get(i).copied().unwrap_or("");
                if !after_block.trim().is_empty() {
                    rendered_lines.push(String::new());
                }
                continue;
            }

            if let Some((level, text)) = Self::is_heading(line) {
                let heading_text = self.render_inline(&text);
                let styled = if level == 1 {
                    self.theme
                        .heading(&self.theme.bold(&self.theme.underline(&heading_text)))
                } else if level == 2 {
                    self.theme.heading(&self.theme.bold(&heading_text))
                } else {
                    let prefix = "#".repeat(level);
                    self.theme
                        .heading(&self.theme.bold(&format!("{prefix} {heading_text}")))
                };
                rendered_lines.push(styled);
                if !next_line.trim().is_empty() {
                    rendered_lines.push(String::new());
                }
                i += 1;
                continue;
            }

            if Self::is_hr(line) {
                let hr_line = "─".repeat(width.min(80));
                rendered_lines.push(self.theme.hr(&hr_line));
                if !next_line.trim().is_empty() {
                    rendered_lines.push(String::new());
                }
                i += 1;
                continue;
            }

            if line.trim_start().starts_with('>') {
                while i < lines.len() {
                    let quote_line = lines[i];
                    if !quote_line.trim_start().starts_with('>') {
                        break;
                    }
                    let mut trimmed = quote_line.trim_start().trim_start_matches('>').to_string();
                    if trimmed.starts_with(' ') {
                        trimmed.remove(0);
                    }
                    let rendered = self.render_inline(&trimmed);
                    rendered_lines.push(
                        self.theme.quote_border("│ ")
                            + &self.theme.quote(&self.theme.italic(&rendered)),
                    );
                    i += 1;
                }
                let next = lines.get(i).copied().unwrap_or("");
                if !next.trim().is_empty() {
                    rendered_lines.push(String::new());
                }
                continue;
            }

            if line.contains('|') && Self::is_table_separator(next_line) {
                let header = Self::parse_table_row(line);
                let mut rows = Vec::new();
                let mut raw_lines = vec![line.to_string(), next_line.to_string()];
                i += 2;
                while i < lines.len() {
                    let row_line = lines[i];
                    if row_line.trim().is_empty() || !row_line.contains('|') {
                        break;
                    }
                    raw_lines.push(row_line.to_string());
                    rows.push(Self::parse_table_row(row_line));
                    i += 1;
                }
                let raw = raw_lines.join("\n");
                let table_lines = self.render_table(&header, &rows, &raw, content_width);
                rendered_lines.extend(table_lines);
                continue;
            }

            if Self::parse_list_line(line).is_some() {
                let mut list_lines = Vec::new();
                let mut current_index = i;
                while current_index < lines.len() {
                    let current_line = lines[current_index];
                    if let Some((cur_indent, cur_bullet, cur_text)) =
                        Self::parse_list_line(current_line)
                    {
                        let content = self.render_inline(&cur_text);
                        let indent_str = " ".repeat(cur_indent);
                        list_lines.push(format!(
                            "{}{}{}",
                            indent_str,
                            self.theme.list_bullet(&cur_bullet),
                            content
                        ));
                        current_index += 1;
                    } else {
                        break;
                    }
                }
                rendered_lines.extend(list_lines);
                i = current_index;
                continue;
            }

            let mut paragraph_lines = Vec::new();
            let mut j = i;
            while j < lines.len() {
                let current = lines[j];
                if current.trim().is_empty()
                    || current.trim_start().starts_with('#')
                    || current.trim_start().starts_with('>')
                    || current.trim_start().starts_with("```")
                    || Self::is_hr(current)
                    || Self::parse_list_line(current).is_some()
                    || (current.contains('|')
                        && lines
                            .get(j + 1)
                            .is_some_and(|next| Self::is_table_separator(next)))
                {
                    break;
                }
                paragraph_lines.push(current.trim().to_string());
                j += 1;
            }
            let paragraph_text = paragraph_lines.join(" ");
            rendered_lines.push(self.render_inline(&paragraph_text));
            i = j;
        }

        let mut wrapped_lines = Vec::new();
        for line in rendered_lines {
            wrapped_lines.extend(wrap_text_with_ansi(&line, content_width));
        }

        let left_margin = " ".repeat(self.padding_x);
        let right_margin = " ".repeat(self.padding_x);
        let mut content_lines = Vec::new();

        for line in wrapped_lines {
            let line_with_margins = format!("{left_margin}{line}{right_margin}");
            if let Some(style) = &self.default_text_style {
                if let Some(bg) = &style.bg_color {
                    content_lines.push(apply_background_to_line(&line_with_margins, width, bg));
                    continue;
                }
            }
            let visible_len = visible_width(&line_with_margins);
            let padding_needed = width.saturating_sub(visible_len);
            content_lines.push(format!(
                "{}{}",
                line_with_margins,
                " ".repeat(padding_needed)
            ));
        }

        let empty_line = " ".repeat(width);
        let mut padded_lines = Vec::new();
        for _ in 0..self.padding_y {
            padded_lines.push(empty_line.clone());
        }
        padded_lines.extend(content_lines);
        for _ in 0..self.padding_y {
            padded_lines.push(empty_line.clone());
        }

        if padded_lines.is_empty() {
            padded_lines.push(String::new());
        }

        padded_lines
    }
}
