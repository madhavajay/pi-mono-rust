//! OAuth provider selector component for login/logout.

use crate::coding_agent::{get_oauth_providers, AuthCredential, ModelRegistry};
use crate::tui::matches_key;

/// Result from the OAuth selector
#[derive(Clone, Debug)]
pub enum OAuthSelectorResult {
    Selected(String),
    Cancelled,
}

/// OAuth provider selector component
pub struct OAuthSelectorComponent {
    mode: OAuthSelectorMode,
    providers: Vec<ProviderItem>,
    selected_index: usize,
    result: Option<OAuthSelectorResult>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OAuthSelectorMode {
    Login,
    Logout,
}

struct ProviderItem {
    id: String,
    name: String,
    is_logged_in: bool,
    available: bool,
}

impl OAuthSelectorComponent {
    pub fn new(mode: OAuthSelectorMode, model_registry: &ModelRegistry) -> Self {
        let oauth_providers = get_oauth_providers();

        let providers: Vec<ProviderItem> = oauth_providers
            .into_iter()
            .map(|p| {
                let is_logged_in = matches!(
                    model_registry.get_credential(&p.id),
                    Some(AuthCredential::OAuth { .. })
                );
                ProviderItem {
                    id: p.id,
                    name: p.name,
                    is_logged_in,
                    available: p.available,
                }
            })
            .collect();

        Self {
            mode,
            providers,
            selected_index: 0,
            result: None,
        }
    }

    /// Handle keyboard input
    pub fn handle_input(&mut self, key_data: &str) -> Option<OAuthSelectorResult> {
        if matches_key(key_data, "up") {
            if self.selected_index > 0 {
                self.selected_index -= 1;
            }
        } else if matches_key(key_data, "down") {
            if self.selected_index + 1 < self.providers.len() {
                self.selected_index += 1;
            }
        } else if matches_key(key_data, "enter") {
            if let Some(provider) = self.providers.get(self.selected_index) {
                if provider.available {
                    // For logout, only allow if logged in
                    if self.mode == OAuthSelectorMode::Logout && !provider.is_logged_in {
                        return None;
                    }
                    return Some(OAuthSelectorResult::Selected(provider.id.clone()));
                }
            }
        } else if matches_key(key_data, "escape") || matches_key(key_data, "ctrl+c") {
            return Some(OAuthSelectorResult::Cancelled);
        }

        None
    }

    /// Get the current result (if any)
    pub fn result(&self) -> Option<OAuthSelectorResult> {
        self.result.clone()
    }

    /// Render the component
    pub fn render(&self, width: usize) -> Vec<String> {
        let max_width = width.min(80);
        let mut lines = Vec::new();

        // Border
        lines.push("─".repeat(max_width));
        lines.push(String::new());

        // Title
        let title = match self.mode {
            OAuthSelectorMode::Login => "Select provider to login:",
            OAuthSelectorMode::Logout => "Select provider to logout:",
        };
        lines.push(format!("  \x1b[1m{}\x1b[0m", title));
        lines.push(String::new());

        // Provider list
        if self.providers.is_empty() {
            let message = match self.mode {
                OAuthSelectorMode::Login => "No OAuth providers available",
                OAuthSelectorMode::Logout => "No OAuth providers logged in. Use /login first.",
            };
            lines.push(format!("  \x1b[2m{}\x1b[0m", message));
        } else {
            for (idx, provider) in self.providers.iter().enumerate() {
                let is_selected = idx == self.selected_index;
                let cursor = if is_selected { "→ " } else { "  " };

                let mut line = String::new();

                if is_selected {
                    line.push_str(&format!("\x1b[36m{}\x1b[0m", cursor));
                    if provider.available {
                        line.push_str(&format!("\x1b[36m{}\x1b[0m", provider.name));
                    } else {
                        line.push_str(&format!("\x1b[2m{}\x1b[0m", provider.name));
                    }
                } else {
                    line.push_str(cursor);
                    if provider.available {
                        line.push_str(&provider.name);
                    } else {
                        line.push_str(&format!("\x1b[2m{}\x1b[0m", provider.name));
                    }
                }

                // Show login status
                if provider.is_logged_in {
                    line.push_str(" \x1b[32m✓ logged in\x1b[0m");
                }

                lines.push(line);
            }
        }

        lines.push(String::new());

        // Help text
        lines.push("  \x1b[2mPress Enter to select, Escape to cancel\x1b[0m".to_string());
        lines.push(String::new());

        // Border
        lines.push("─".repeat(max_width));

        lines
    }
}
