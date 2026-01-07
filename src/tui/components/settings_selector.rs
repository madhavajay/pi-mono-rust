use crate::tui::utils::truncate_to_width;

/// A setting item for display in the selector.
#[derive(Clone, Debug)]
pub struct SettingItem {
    pub id: String,
    pub label: String,
    pub description: String,
    pub current_value: String,
    pub values: Vec<SettingValue>,
}

/// A possible value for a setting.
#[derive(Clone, Debug)]
pub struct SettingValue {
    pub value: String,
    pub label: String,
    pub description: Option<String>,
}

/// Mode for the settings selector.
#[derive(Clone, Debug, PartialEq)]
enum SelectorMode {
    /// Selecting which setting to change.
    SettingList,
    /// Selecting a value for the current setting.
    ValueList,
}

/// State for the settings selector component.
pub struct SettingsSelectorState {
    items: Vec<SettingItem>,
    selected_index: usize,
    max_visible: usize,
    mode: SelectorMode,
    value_index: usize,
}

impl SettingsSelectorState {
    pub fn new(items: Vec<SettingItem>, max_visible: usize) -> Self {
        Self {
            items,
            selected_index: 0,
            max_visible,
            mode: SelectorMode::SettingList,
            value_index: 0,
        }
    }

    /// Handle keyboard input.
    pub fn handle_input(&mut self, key_data: &str) -> Option<SettingsSelectorResult> {
        use crate::tui::matches_key;

        match self.mode {
            SelectorMode::SettingList => {
                if matches_key(key_data, "up") {
                    self.move_up_setting();
                } else if matches_key(key_data, "down") {
                    self.move_down_setting();
                } else if matches_key(key_data, "enter") {
                    // Enter value selection mode for current setting
                    if let Some(item) = self.items.get(self.selected_index) {
                        // Find current value index
                        self.value_index = item
                            .values
                            .iter()
                            .position(|v| v.value == item.current_value)
                            .unwrap_or(0);
                        self.mode = SelectorMode::ValueList;
                    }
                } else if matches_key(key_data, "escape") {
                    return Some(SettingsSelectorResult::Cancelled);
                }
            }
            SelectorMode::ValueList => {
                if matches_key(key_data, "up") {
                    self.move_up_value();
                } else if matches_key(key_data, "down") {
                    self.move_down_value();
                } else if matches_key(key_data, "enter") {
                    // Apply the selected value
                    if let Some(item) = self.items.get(self.selected_index) {
                        if let Some(value) = item.values.get(self.value_index) {
                            return Some(SettingsSelectorResult::Changed {
                                setting_id: item.id.clone(),
                                value: value.value.clone(),
                            });
                        }
                    }
                    self.mode = SelectorMode::SettingList;
                } else if matches_key(key_data, "escape") {
                    // Go back to settings list
                    self.mode = SelectorMode::SettingList;
                }
            }
        }
        None
    }

    fn move_up_setting(&mut self) {
        if self.items.is_empty() {
            return;
        }
        if self.selected_index == 0 {
            self.selected_index = self.items.len() - 1;
        } else {
            self.selected_index -= 1;
        }
    }

    fn move_down_setting(&mut self) {
        if self.items.is_empty() {
            return;
        }
        if self.selected_index + 1 >= self.items.len() {
            self.selected_index = 0;
        } else {
            self.selected_index += 1;
        }
    }

    fn move_up_value(&mut self) {
        if let Some(item) = self.items.get(self.selected_index) {
            if item.values.is_empty() {
                return;
            }
            if self.value_index == 0 {
                self.value_index = item.values.len() - 1;
            } else {
                self.value_index -= 1;
            }
        }
    }

    fn move_down_value(&mut self) {
        if let Some(item) = self.items.get(self.selected_index) {
            if item.values.is_empty() {
                return;
            }
            if self.value_index + 1 >= item.values.len() {
                self.value_index = 0;
            } else {
                self.value_index += 1;
            }
        }
    }

    /// Render the component.
    pub fn render(&self, width: usize) -> Vec<String> {
        match self.mode {
            SelectorMode::SettingList => self.render_settings_list(width),
            SelectorMode::ValueList => self.render_value_list(width),
        }
    }

    fn render_settings_list(&self, width: usize) -> Vec<String> {
        let mut lines = Vec::new();
        let border = "─".repeat(width.min(80));

        // Top border
        lines.push(border.clone());
        lines.push(String::new());

        // Title
        lines.push("  \x1b[1mSettings\x1b[0m".to_string());
        lines.push(String::new());

        if self.items.is_empty() {
            lines.push("  \x1b[2mNo settings available\x1b[0m".to_string());
        } else {
            // Calculate visible range
            let start = if self.items.len() <= self.max_visible {
                0
            } else {
                let half = self.max_visible / 2;
                let max_start = self.items.len() - self.max_visible;
                self.selected_index.saturating_sub(half).min(max_start)
            };
            let end = (start + self.max_visible).min(self.items.len());

            for (display_idx, item) in self.items[start..end].iter().enumerate() {
                let is_selected = display_idx + start == self.selected_index;

                // Format: label: value
                let label_value = format!("{}: {}", item.label, item.current_value);

                let line = if is_selected {
                    format!(
                        "\x1b[36m› \x1b[0m\x1b[1m{}\x1b[0m",
                        truncate_to_width(&label_value, width.saturating_sub(4))
                    )
                } else {
                    format!(
                        "  {}",
                        truncate_to_width(&label_value, width.saturating_sub(4))
                    )
                };
                lines.push(line);

                // Show description for selected item
                if is_selected && !item.description.is_empty() {
                    let desc = format!("    \x1b[2m{}\x1b[0m", item.description);
                    lines.push(truncate_to_width(&desc, width));
                }
            }

            // Scroll indicator
            if self.items.len() > self.max_visible {
                lines.push(format!(
                    "  \x1b[2m({}/{})\x1b[0m",
                    self.selected_index + 1,
                    self.items.len()
                ));
            }
        }

        lines.push(String::new());

        // Hint
        lines.push("  \x1b[2m↑↓ navigate · Enter edit · Esc cancel\x1b[0m".to_string());
        lines.push(String::new());

        // Bottom border
        lines.push(border);

        lines
    }

    fn render_value_list(&self, width: usize) -> Vec<String> {
        let mut lines = Vec::new();
        let border = "─".repeat(width.min(80));

        let Some(item) = self.items.get(self.selected_index) else {
            lines.push(border.clone());
            lines.push("  \x1b[31mError: Invalid setting\x1b[0m".to_string());
            lines.push(border);
            return lines;
        };

        // Top border
        lines.push(border.clone());
        lines.push(String::new());

        // Title with setting name
        lines.push(format!("  \x1b[1m{}\x1b[0m", item.label));
        if !item.description.is_empty() {
            lines.push(format!("  \x1b[2m{}\x1b[0m", item.description));
        }
        lines.push(String::new());

        // Value list
        for (idx, value) in item.values.iter().enumerate() {
            let is_selected = idx == self.value_index;
            let is_current = value.value == item.current_value;

            let current_marker = if is_current {
                " \x1b[32m✓\x1b[0m"
            } else {
                ""
            };
            let display_text = format!("{}{}", value.label, current_marker);

            let line = if is_selected {
                format!(
                    "\x1b[36m› \x1b[0m\x1b[1m{}\x1b[0m",
                    truncate_to_width(&display_text, width.saturating_sub(4))
                )
            } else {
                format!(
                    "  {}",
                    truncate_to_width(&display_text, width.saturating_sub(4))
                )
            };
            lines.push(line);

            // Show description for selected value
            if is_selected {
                if let Some(desc) = &value.description {
                    let desc_line = format!("    \x1b[2m{}\x1b[0m", desc);
                    lines.push(truncate_to_width(&desc_line, width));
                }
            }
        }

        lines.push(String::new());

        // Hint
        lines.push("  \x1b[2m↑↓ navigate · Enter select · Esc back\x1b[0m".to_string());
        lines.push(String::new());

        // Bottom border
        lines.push(border);

        lines
    }
}

/// Result of the settings selector interaction.
pub enum SettingsSelectorResult {
    Changed { setting_id: String, value: String },
    Cancelled,
}

/// Component wrapper for the settings selector.
pub struct SettingsSelectorComponent {
    state: SettingsSelectorState,
}

impl SettingsSelectorComponent {
    pub fn new(items: Vec<SettingItem>, max_visible: usize) -> Self {
        Self {
            state: SettingsSelectorState::new(items, max_visible),
        }
    }

    pub fn handle_input(&mut self, key_data: &str) -> Option<SettingsSelectorResult> {
        self.state.handle_input(key_data)
    }

    pub fn render(&self, width: usize) -> Vec<String> {
        self.state.render(width)
    }
}

/// Helper to create boolean setting values.
pub fn bool_values() -> Vec<SettingValue> {
    vec![
        SettingValue {
            value: "true".to_string(),
            label: "true".to_string(),
            description: Some("Enable this setting".to_string()),
        },
        SettingValue {
            value: "false".to_string(),
            label: "false".to_string(),
            description: Some("Disable this setting".to_string()),
        },
    ]
}

/// Helper to create queue mode setting values.
pub fn queue_mode_values() -> Vec<SettingValue> {
    vec![
        SettingValue {
            value: "one-at-a-time".to_string(),
            label: "one-at-a-time".to_string(),
            description: Some("Process one message at a time".to_string()),
        },
        SettingValue {
            value: "all".to_string(),
            label: "all".to_string(),
            description: Some("Process all queued messages".to_string()),
        },
    ]
}

/// Helper to create thinking level setting values.
pub fn thinking_level_values() -> Vec<SettingValue> {
    vec![
        SettingValue {
            value: "off".to_string(),
            label: "off".to_string(),
            description: Some("No reasoning".to_string()),
        },
        SettingValue {
            value: "minimal".to_string(),
            label: "minimal".to_string(),
            description: Some("Very brief reasoning (~1k tokens)".to_string()),
        },
        SettingValue {
            value: "low".to_string(),
            label: "low".to_string(),
            description: Some("Light reasoning (~2k tokens)".to_string()),
        },
        SettingValue {
            value: "medium".to_string(),
            label: "medium".to_string(),
            description: Some("Moderate reasoning (~8k tokens)".to_string()),
        },
        SettingValue {
            value: "high".to_string(),
            label: "high".to_string(),
            description: Some("Deep reasoning (~16k tokens)".to_string()),
        },
        SettingValue {
            value: "xhigh".to_string(),
            label: "xhigh".to_string(),
            description: Some("Maximum reasoning (~32k tokens)".to_string()),
        },
    ]
}

/// Helper to create double-escape action setting values.
pub fn double_escape_action_values() -> Vec<SettingValue> {
    vec![
        SettingValue {
            value: "tree".to_string(),
            label: "tree".to_string(),
            description: Some("Open session tree navigator".to_string()),
        },
        SettingValue {
            value: "branch".to_string(),
            label: "branch".to_string(),
            description: Some("Create a new branch".to_string()),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_bool_setting(id: &str, label: &str, current: bool) -> SettingItem {
        SettingItem {
            id: id.to_string(),
            label: label.to_string(),
            description: format!("Description for {}", label),
            current_value: current.to_string(),
            values: bool_values(),
        }
    }

    #[test]
    fn test_settings_selector_creation() {
        let items = vec![
            make_bool_setting("autocompact", "Auto-compact", true),
            make_bool_setting("show-images", "Show images", false),
        ];
        let state = SettingsSelectorState::new(items, 10);

        assert_eq!(state.items.len(), 2);
        assert_eq!(state.selected_index, 0);
        assert_eq!(state.mode, SelectorMode::SettingList);
    }

    #[test]
    fn test_settings_selector_navigation() {
        let items = vec![
            make_bool_setting("a", "A", true),
            make_bool_setting("b", "B", false),
            make_bool_setting("c", "C", true),
        ];
        let mut state = SettingsSelectorState::new(items, 10);

        assert_eq!(state.selected_index, 0);
        state.move_down_setting();
        assert_eq!(state.selected_index, 1);
        state.move_down_setting();
        assert_eq!(state.selected_index, 2);
        state.move_down_setting(); // wrap
        assert_eq!(state.selected_index, 0);
        state.move_up_setting(); // wrap
        assert_eq!(state.selected_index, 2);
    }

    #[test]
    fn test_settings_selector_value_mode() {
        let items = vec![make_bool_setting("test", "Test", true)];
        let mut state = SettingsSelectorState::new(items, 10);

        // Enter value mode (use "\r" which is the raw terminal input for enter)
        let _ = state.handle_input("\r");
        assert_eq!(state.mode, SelectorMode::ValueList);

        // Navigate values
        state.move_down_value();
        assert_eq!(state.value_index, 1);

        // Go back (use "\x1b" which is the raw terminal input for escape)
        let _ = state.handle_input("\x1b");
        assert_eq!(state.mode, SelectorMode::SettingList);
    }

    #[test]
    fn test_settings_selector_render() {
        let items = vec![
            make_bool_setting("autocompact", "Auto-compact", true),
            make_bool_setting("show-images", "Show images", false),
        ];
        let state = SettingsSelectorState::new(items, 10);
        let lines = state.render(80);

        // Should have border, title, items, hint, border
        assert!(lines.len() >= 6);
        assert!(lines.iter().any(|l| l.contains("Settings")));
        assert!(lines.iter().any(|l| l.contains("Auto-compact")));
    }
}
