use crate::coding_agent::Model;
use crate::tui::utils::truncate_to_width;

/// A model item for display in the selector.
#[derive(Clone, Debug)]
pub struct ModelItem {
    pub provider: String,
    pub id: String,
    pub name: String,
    pub reasoning: bool,
    pub is_current: bool,
}

impl ModelItem {
    pub fn from_model(model: &Model, current_provider: &str, current_id: &str) -> Self {
        Self {
            provider: model.provider.clone(),
            id: model.id.clone(),
            name: model.name.clone(),
            reasoning: model.reasoning,
            is_current: model.provider == current_provider && model.id == current_id,
        }
    }

    /// Display label for the model.
    pub fn label(&self) -> String {
        if self.name.is_empty() || self.name == self.id {
            format!("{}/{}", self.provider, self.id)
        } else {
            format!("{}/{} ({})", self.provider, self.id, self.name)
        }
    }
}

/// State for the model selector component.
pub struct ModelSelectorState {
    items: Vec<ModelItem>,
    filtered_indices: Vec<usize>,
    selected_index: usize,
    search_query: String,
    max_visible: usize,
}

impl ModelSelectorState {
    pub fn new(models: Vec<ModelItem>, max_visible: usize) -> Self {
        // Sort models: current first, then by provider/id
        let mut items = models;
        items.sort_by(|a, b| {
            if a.is_current && !b.is_current {
                return std::cmp::Ordering::Less;
            }
            if !a.is_current && b.is_current {
                return std::cmp::Ordering::Greater;
            }
            a.provider.cmp(&b.provider).then_with(|| a.id.cmp(&b.id))
        });

        let filtered_indices = (0..items.len()).collect();
        Self {
            items,
            filtered_indices,
            selected_index: 0,
            search_query: String::new(),
            max_visible,
        }
    }

    /// Filter items based on search query.
    fn filter(&mut self) {
        let query = self.search_query.to_lowercase();
        if query.is_empty() {
            self.filtered_indices = (0..self.items.len()).collect();
        } else {
            self.filtered_indices = self
                .items
                .iter()
                .enumerate()
                .filter(|(_, item)| {
                    let search_text = format!("{} {}", item.id, item.provider).to_lowercase();
                    search_text.contains(&query)
                })
                .map(|(i, _)| i)
                .collect();
        }
        if self.selected_index >= self.filtered_indices.len() {
            self.selected_index = self.filtered_indices.len().saturating_sub(1);
        }
    }

    /// Handle keyboard input.
    pub fn handle_input(&mut self, key_data: &str) -> Option<ModelSelectorResult> {
        use crate::tui::matches_key;

        if matches_key(key_data, "up") {
            self.move_up();
        } else if matches_key(key_data, "down") {
            self.move_down();
        } else if matches_key(key_data, "enter") {
            return Some(self.confirm_selection());
        } else if matches_key(key_data, "escape") {
            return Some(ModelSelectorResult::Cancelled);
        } else if matches_key(key_data, "backspace") {
            self.search_query.pop();
            self.filter();
        } else if key_data.len() == 1 {
            let ch = key_data.chars().next().unwrap();
            if ch.is_ascii_graphic() || ch == ' ' {
                self.search_query.push(ch);
                self.filter();
            }
        }
        None
    }

    fn move_up(&mut self) {
        if self.filtered_indices.is_empty() {
            return;
        }
        if self.selected_index == 0 {
            self.selected_index = self.filtered_indices.len() - 1;
        } else {
            self.selected_index -= 1;
        }
    }

    fn move_down(&mut self) {
        if self.filtered_indices.is_empty() {
            return;
        }
        if self.selected_index + 1 >= self.filtered_indices.len() {
            self.selected_index = 0;
        } else {
            self.selected_index += 1;
        }
    }

    fn confirm_selection(&self) -> ModelSelectorResult {
        if let Some(&idx) = self.filtered_indices.get(self.selected_index) {
            let item = &self.items[idx];
            ModelSelectorResult::Selected {
                provider: item.provider.clone(),
                model_id: item.id.clone(),
            }
        } else {
            ModelSelectorResult::Cancelled
        }
    }

    /// Render the component.
    pub fn render(&self, width: usize) -> Vec<String> {
        let mut lines = Vec::new();
        let border = "─".repeat(width.min(80));

        // Top border
        lines.push(border.clone());
        lines.push(String::new());

        // Title
        lines.push("  \x1b[1mSelect Model\x1b[0m".to_string());
        lines.push(String::new());

        // Search input
        let search_line = format!(
            "  \x1b[2mSearch:\x1b[0m {}{}",
            self.search_query,
            if self.search_query.is_empty() {
                "\x1b[2m_\x1b[0m"
            } else {
                "_"
            }
        );
        lines.push(truncate_to_width(&search_line, width));
        lines.push(String::new());

        // Model list
        if self.filtered_indices.is_empty() {
            lines.push("  \x1b[2mNo models found\x1b[0m".to_string());
        } else {
            // Calculate visible range
            let start = if self.filtered_indices.len() <= self.max_visible {
                0
            } else {
                let half = self.max_visible / 2;
                let max_start = self.filtered_indices.len() - self.max_visible;
                self.selected_index.saturating_sub(half).min(max_start)
            };
            let end = (start + self.max_visible).min(self.filtered_indices.len());

            for (display_idx, &original_idx) in self.filtered_indices[start..end].iter().enumerate()
            {
                let item = &self.items[original_idx];
                let is_selected = display_idx + start == self.selected_index;

                let label = item.label();
                let current_marker = if item.is_current {
                    " \x1b[32m✓\x1b[0m"
                } else {
                    ""
                };
                let reasoning_marker = if item.reasoning {
                    " \x1b[33m⚡\x1b[0m"
                } else {
                    ""
                };

                let line = if is_selected {
                    let text = format!("{}{}{}", label, reasoning_marker, current_marker);
                    format!(
                        "\x1b[36m› \x1b[0m\x1b[1m{}\x1b[0m",
                        truncate_to_width(&text, width.saturating_sub(4))
                    )
                } else {
                    let text = format!("{}{}{}", label, reasoning_marker, current_marker);
                    format!("  {}", truncate_to_width(&text, width.saturating_sub(4)))
                };
                lines.push(line);
            }

            // Scroll indicator
            if self.filtered_indices.len() > self.max_visible {
                lines.push(format!(
                    "  \x1b[2m({}/{})\x1b[0m",
                    self.selected_index + 1,
                    self.filtered_indices.len()
                ));
            }
        }

        lines.push(String::new());

        // Hint
        lines.push("  \x1b[2m↑↓ navigate · Enter select · Esc cancel\x1b[0m".to_string());
        lines.push(String::new());

        // Bottom border
        lines.push(border);

        lines
    }
}

/// Result of the model selector interaction.
pub enum ModelSelectorResult {
    Selected { provider: String, model_id: String },
    Cancelled,
}

/// Component wrapper for the model selector.
pub struct ModelSelectorComponent {
    state: ModelSelectorState,
}

impl ModelSelectorComponent {
    pub fn new(models: Vec<ModelItem>, max_visible: usize) -> Self {
        Self {
            state: ModelSelectorState::new(models, max_visible),
        }
    }

    pub fn handle_input(&mut self, key_data: &str) -> Option<ModelSelectorResult> {
        self.state.handle_input(key_data)
    }

    pub fn render(&self, width: usize) -> Vec<String> {
        self.state.render(width)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_model(provider: &str, id: &str, is_current: bool) -> ModelItem {
        ModelItem {
            provider: provider.to_string(),
            id: id.to_string(),
            name: id.to_string(),
            reasoning: false,
            is_current,
        }
    }

    #[test]
    fn test_model_selector_creation() {
        let models = vec![
            make_model("anthropic", "claude-opus-4", false),
            make_model("anthropic", "claude-sonnet-4", true),
            make_model("openai", "gpt-4o", false),
        ];
        let state = ModelSelectorState::new(models, 10);

        // Current model should be first
        assert!(state.items[0].is_current);
        assert_eq!(state.filtered_indices.len(), 3);
    }

    #[test]
    fn test_model_selector_filtering() {
        let models = vec![
            make_model("anthropic", "claude-opus-4", false),
            make_model("anthropic", "claude-sonnet-4", false),
            make_model("openai", "gpt-4o", false),
        ];
        let mut state = ModelSelectorState::new(models, 10);

        state.search_query = "claude".to_string();
        state.filter();

        assert_eq!(state.filtered_indices.len(), 2);
    }

    #[test]
    fn test_model_selector_navigation() {
        let models = vec![
            make_model("a", "1", false),
            make_model("b", "2", false),
            make_model("c", "3", false),
        ];
        let mut state = ModelSelectorState::new(models, 10);

        assert_eq!(state.selected_index, 0);
        state.move_down();
        assert_eq!(state.selected_index, 1);
        state.move_down();
        assert_eq!(state.selected_index, 2);
        state.move_down(); // wrap
        assert_eq!(state.selected_index, 0);
        state.move_up(); // wrap
        assert_eq!(state.selected_index, 2);
    }

    #[test]
    fn test_model_selector_render() {
        let models = vec![
            make_model("anthropic", "claude-opus-4", true),
            make_model("openai", "gpt-4o", false),
        ];
        let state = ModelSelectorState::new(models, 10);
        let lines = state.render(80);

        // Should have border, title, search, items, hint, border
        assert!(lines.len() >= 8);
        assert!(lines.iter().any(|l| l.contains("Select Model")));
        assert!(lines.iter().any(|l| l.contains("claude-opus-4")));
    }
}
