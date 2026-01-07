//! Login dialog component for OAuth flows.

use crate::tui::matches_key;

/// State of the login dialog
#[derive(Clone, Debug, PartialEq)]
pub enum LoginDialogState {
    /// Showing authorization URL
    ShowingAuth {
        url: String,
        instructions: Option<String>,
    },
    /// Waiting for user input (e.g., code paste)
    WaitingForInput {
        message: String,
        placeholder: Option<String>,
        input: String,
    },
    /// Showing progress message
    ShowingProgress { message: String },
    /// Waiting for browser callback (GitHub Copilot device flow)
    WaitingForCallback { message: String },
    /// Completed successfully
    Completed { message: String },
    /// Failed
    Failed { error: String },
    /// Cancelled
    Cancelled,
}

/// Result from the login dialog
#[derive(Clone, Debug)]
pub enum LoginDialogResult {
    InputSubmitted(String),
    Cancelled,
}

/// Login dialog component
pub struct LoginDialogComponent {
    provider_name: String,
    state: LoginDialogState,
}

impl LoginDialogComponent {
    pub fn new(provider_name: &str) -> Self {
        Self {
            provider_name: provider_name.to_string(),
            state: LoginDialogState::ShowingProgress {
                message: "Initializing...".to_string(),
            },
        }
    }

    /// Show authorization URL
    pub fn show_auth(&mut self, url: &str, instructions: Option<&str>) {
        self.state = LoginDialogState::ShowingAuth {
            url: url.to_string(),
            instructions: instructions.map(|s| s.to_string()),
        };
    }

    /// Show input prompt
    pub fn show_prompt(&mut self, message: &str, placeholder: Option<&str>) {
        self.state = LoginDialogState::WaitingForInput {
            message: message.to_string(),
            placeholder: placeholder.map(|s| s.to_string()),
            input: String::new(),
        };
    }

    /// Show waiting message for browser callback
    pub fn show_waiting(&mut self, message: &str) {
        self.state = LoginDialogState::WaitingForCallback {
            message: message.to_string(),
        };
    }

    /// Show progress message
    pub fn show_progress(&mut self, message: &str) {
        self.state = LoginDialogState::ShowingProgress {
            message: message.to_string(),
        };
    }

    /// Mark as completed
    pub fn complete(&mut self, message: &str) {
        self.state = LoginDialogState::Completed {
            message: message.to_string(),
        };
    }

    /// Mark as failed
    pub fn fail(&mut self, error: &str) {
        self.state = LoginDialogState::Failed {
            error: error.to_string(),
        };
    }

    /// Check if cancelled
    pub fn is_cancelled(&self) -> bool {
        matches!(self.state, LoginDialogState::Cancelled)
    }

    /// Get current state
    pub fn state(&self) -> &LoginDialogState {
        &self.state
    }

    /// Handle keyboard input
    pub fn handle_input(&mut self, key_data: &str) -> Option<LoginDialogResult> {
        match &mut self.state {
            LoginDialogState::WaitingForInput { input, .. } => {
                if matches_key(key_data, "escape") || matches_key(key_data, "ctrl+c") {
                    self.state = LoginDialogState::Cancelled;
                    return Some(LoginDialogResult::Cancelled);
                } else if matches_key(key_data, "enter") {
                    let value = input.clone();
                    return Some(LoginDialogResult::InputSubmitted(value));
                } else if matches_key(key_data, "backspace") {
                    input.pop();
                } else if key_data.len() == 1 {
                    let ch = key_data.chars().next().unwrap();
                    if ch.is_ascii_graphic() || ch == ' ' {
                        input.push(ch);
                    }
                }
            }
            LoginDialogState::WaitingForCallback { .. }
            | LoginDialogState::ShowingAuth { .. }
            | LoginDialogState::ShowingProgress { .. } => {
                if matches_key(key_data, "escape") || matches_key(key_data, "ctrl+c") {
                    self.state = LoginDialogState::Cancelled;
                    return Some(LoginDialogResult::Cancelled);
                }
            }
            _ => {}
        }

        None
    }

    /// Render the component
    pub fn render(&self, width: usize) -> Vec<String> {
        let max_width = width.min(80);
        let mut lines = Vec::new();

        // Border
        lines.push("─".repeat(max_width));

        // Title
        lines.push(format!("  \x1b[33mLogin to {}\x1b[0m", self.provider_name));
        lines.push(String::new());

        // Content based on state
        match &self.state {
            LoginDialogState::ShowingAuth { url, instructions } => {
                lines.push(format!("  \x1b[36m{}\x1b[0m", url));

                // Add hyperlink hint
                let click_hint = if cfg!(target_os = "macos") {
                    "Cmd+click to open"
                } else {
                    "Ctrl+click to open"
                };
                // OSC 8 hyperlink
                let hyperlink = format!(
                    "  \x1b[2m\x1b]8;;{}\x07{}\x1b]8;;\x07\x1b[0m",
                    url, click_hint
                );
                lines.push(hyperlink);

                if let Some(instructions) = instructions {
                    lines.push(String::new());
                    lines.push(format!("  \x1b[33m{}\x1b[0m", instructions));
                }

                lines.push(String::new());
                lines.push("  \x1b[2m(Escape to cancel)\x1b[0m".to_string());
            }
            LoginDialogState::WaitingForInput {
                message,
                placeholder,
                input,
            } => {
                lines.push(format!("  {}", message));
                if let Some(placeholder) = placeholder {
                    lines.push(format!("  \x1b[2me.g., {}\x1b[0m", placeholder));
                }
                lines.push(String::new());
                lines.push(format!("  > {}_", input));
                lines.push(String::new());
                lines.push("  \x1b[2m(Escape to cancel, Enter to submit)\x1b[0m".to_string());
            }
            LoginDialogState::WaitingForCallback { message } => {
                lines.push(format!("  \x1b[2m{}\x1b[0m", message));
                lines.push(String::new());
                lines.push("  \x1b[2m(Escape to cancel)\x1b[0m".to_string());
            }
            LoginDialogState::ShowingProgress { message } => {
                lines.push(format!("  \x1b[2m{}\x1b[0m", message));
            }
            LoginDialogState::Completed { message } => {
                lines.push(format!("  \x1b[32m{}\x1b[0m", message));
            }
            LoginDialogState::Failed { error } => {
                lines.push(format!("  \x1b[31mError: {}\x1b[0m", error));
            }
            LoginDialogState::Cancelled => {
                lines.push("  \x1b[2mCancelled\x1b[0m".to_string());
            }
        }

        lines.push(String::new());

        // Border
        lines.push("─".repeat(max_width));

        lines
    }
}
