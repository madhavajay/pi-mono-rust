use crate::tui::autocomplete::AutocompleteItem;
use crate::tui::utils::truncate_to_width;

/// Theme for the select list component.
#[derive(Clone, Copy)]
pub struct SelectListTheme {
    pub selected_prefix: fn(&str) -> String,
    pub selected_text: fn(&str) -> String,
    pub description: fn(&str) -> String,
    pub scroll_info: fn(&str) -> String,
    pub no_match: fn(&str) -> String,
}

impl Default for SelectListTheme {
    fn default() -> Self {
        Self {
            selected_prefix: |s| format!("\x1b[1m{s}\x1b[0m"),
            selected_text: |s| format!("\x1b[7m{s}\x1b[0m"),
            description: |s| format!("\x1b[2m{s}\x1b[0m"),
            scroll_info: |s| format!("\x1b[2m{s}\x1b[0m"),
            no_match: |s| format!("\x1b[2m{s}\x1b[0m"),
        }
    }
}

/// A dropdown list for selecting from autocomplete suggestions.
pub struct SelectList {
    items: Vec<AutocompleteItem>,
    selected_index: usize,
    max_visible: usize,
    theme: SelectListTheme,
}

impl SelectList {
    pub fn new(items: Vec<AutocompleteItem>, max_visible: usize, theme: SelectListTheme) -> Self {
        Self {
            items,
            selected_index: 0,
            max_visible,
            theme,
        }
    }

    pub fn render(&self, width: usize) -> Vec<String> {
        let mut lines = Vec::new();

        if self.items.is_empty() {
            lines.push((self.theme.no_match)("  No matching commands"));
            return lines;
        }

        // Calculate visible range with scrolling
        let half_visible = self.max_visible / 2;
        let start_index = if self.items.len() <= self.max_visible {
            0
        } else {
            self.selected_index
                .saturating_sub(half_visible)
                .min(self.items.len().saturating_sub(self.max_visible))
        };
        let end_index = (start_index + self.max_visible).min(self.items.len());

        // Render visible items
        for i in start_index..end_index {
            let Some(item) = self.items.get(i) else {
                continue;
            };

            let is_selected = i == self.selected_index;
            let line = self.render_item(item, is_selected, width);
            lines.push(line);
        }

        // Add scroll indicators if needed
        if start_index > 0 || end_index < self.items.len() {
            let scroll_text = format!("  ({}/{})", self.selected_index + 1, self.items.len());
            lines.push((self.theme.scroll_info)(&truncate_to_width(
                &scroll_text,
                width.saturating_sub(2),
            )));
        }

        lines
    }

    fn render_item(&self, item: &AutocompleteItem, is_selected: bool, width: usize) -> String {
        let display_value = if item.label.is_empty() {
            &item.value
        } else {
            &item.label
        };

        if is_selected {
            // Use arrow indicator for selection
            let prefix_width = 2; // "→ " is 2 characters visually

            if let Some(ref desc) = item.description {
                if width > 40 {
                    // Calculate space for value + description
                    let max_value_width = 30.min(width.saturating_sub(prefix_width + 4));
                    let truncated_value = truncate_to_width(display_value, max_value_width);
                    let spacing = " ".repeat(32usize.saturating_sub(truncated_value.len()).max(1));

                    // Calculate remaining space for description
                    let description_start = prefix_width + truncated_value.len() + spacing.len();
                    let remaining_width = width.saturating_sub(description_start + 2);

                    if remaining_width > 10 {
                        let truncated_desc = truncate_to_width(desc, remaining_width);
                        return (self.theme.selected_text)(&format!(
                            "→ {truncated_value}{spacing}{truncated_desc}"
                        ));
                    }
                }
            }

            // No description or not enough width
            let max_width = width.saturating_sub(prefix_width + 2);
            (self.theme.selected_text)(&format!(
                "→ {}",
                truncate_to_width(display_value, max_width)
            ))
        } else {
            let prefix = "  ";

            if let Some(ref desc) = item.description {
                if width > 40 {
                    let max_value_width = 30.min(width.saturating_sub(prefix.len() + 4));
                    let truncated_value = truncate_to_width(display_value, max_value_width);
                    let spacing = " ".repeat(32usize.saturating_sub(truncated_value.len()).max(1));

                    let description_start = prefix.len() + truncated_value.len() + spacing.len();
                    let remaining_width = width.saturating_sub(description_start + 2);

                    if remaining_width > 10 {
                        let truncated_desc = truncate_to_width(desc, remaining_width);
                        let desc_text =
                            (self.theme.description)(&format!("{spacing}{truncated_desc}"));
                        return format!("{prefix}{truncated_value}{desc_text}");
                    }
                }
            }

            let max_width = width.saturating_sub(prefix.len() + 2);
            format!("{prefix}{}", truncate_to_width(display_value, max_width))
        }
    }

    pub fn move_up(&mut self) {
        if self.items.is_empty() {
            return;
        }
        if self.selected_index == 0 {
            self.selected_index = self.items.len() - 1;
        } else {
            self.selected_index -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if self.items.is_empty() {
            return;
        }
        if self.selected_index + 1 >= self.items.len() {
            self.selected_index = 0;
        } else {
            self.selected_index += 1;
        }
    }

    pub fn get_selected_item(&self) -> Option<&AutocompleteItem> {
        self.items.get(self.selected_index)
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_item(value: &str, desc: Option<&str>) -> AutocompleteItem {
        AutocompleteItem {
            value: value.to_string(),
            label: value.to_string(),
            description: desc.map(String::from),
        }
    }

    #[test]
    fn renders_empty_list_with_no_match() {
        let list = SelectList::new(vec![], 5, SelectListTheme::default());
        let lines = list.render(80);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("No matching commands"));
    }

    #[test]
    fn renders_items_with_selection_indicator() {
        let items = vec![
            make_item("model", Some("Select model")),
            make_item("settings", Some("Open settings")),
        ];
        let list = SelectList::new(items, 5, SelectListTheme::default());
        let lines = list.render(80);
        assert_eq!(lines.len(), 2);
        // First item is selected and has arrow
        assert!(lines[0].contains("→"));
        assert!(lines[0].contains("model"));
    }

    #[test]
    fn move_up_wraps_to_bottom() {
        let items = vec![make_item("a", None), make_item("b", None)];
        let mut list = SelectList::new(items, 5, SelectListTheme::default());
        assert_eq!(list.selected_index, 0);
        list.move_up();
        assert_eq!(list.selected_index, 1);
    }

    #[test]
    fn move_down_wraps_to_top() {
        let items = vec![make_item("a", None), make_item("b", None)];
        let mut list = SelectList::new(items, 5, SelectListTheme::default());
        list.move_down();
        assert_eq!(list.selected_index, 1);
        list.move_down();
        assert_eq!(list.selected_index, 0);
    }

    #[test]
    fn get_selected_item_returns_correct_item() {
        let items = vec![make_item("a", None), make_item("b", None)];
        let mut list = SelectList::new(items, 5, SelectListTheme::default());
        assert_eq!(list.get_selected_item().unwrap().value, "a");
        list.move_down();
        assert_eq!(list.get_selected_item().unwrap().value, "b");
    }
}
