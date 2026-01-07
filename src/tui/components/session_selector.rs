//! Session selector component for interactive mode.
//!
//! Provides a TUI-based session picker with:
//! - Search filtering
//! - Multi-line session display (message + metadata)
//! - Keyboard navigation (up/down, enter, escape)

use crate::core::session_manager::SessionInfo;
use crate::tui::keys::matches_key;
use crate::tui::utils::truncate_to_width;
use std::path::PathBuf;
use std::time::SystemTime;

// Type aliases for callback signatures
type SelectCallback = Box<dyn FnMut(PathBuf)>;
type CancelCallback = Box<dyn FnMut()>;

/// Session list component with selection and search
pub struct SessionList {
    /// All sessions
    all_sessions: Vec<SessionInfo>,
    /// Filtered sessions (after applying search)
    filtered_sessions: Vec<SessionInfo>,
    /// Currently selected index in filtered_sessions
    selected_index: usize,
    /// Current search query
    search_query: String,
    /// Maximum visible sessions
    max_visible: usize,
    /// Callback when a session is selected
    pub on_select: Option<SelectCallback>,
    /// Callback when selection is cancelled
    pub on_cancel: Option<CancelCallback>,
}

impl SessionList {
    /// Create a new session list
    pub fn new(sessions: Vec<SessionInfo>, max_visible: usize) -> Self {
        let filtered = sessions.clone();
        Self {
            all_sessions: sessions,
            filtered_sessions: filtered,
            selected_index: 0,
            search_query: String::new(),
            max_visible,
            on_select: None,
            on_cancel: None,
        }
    }

    /// Filter sessions by search query
    fn filter_sessions(&mut self) {
        let query = self.search_query.to_lowercase();
        if query.is_empty() {
            self.filtered_sessions = self.all_sessions.clone();
        } else {
            self.filtered_sessions = self
                .all_sessions
                .iter()
                .filter(|session| {
                    session.all_messages_text.to_lowercase().contains(&query)
                        || session.first_message.to_lowercase().contains(&query)
                })
                .cloned()
                .collect();
        }
        // Clamp selected index
        if self.filtered_sessions.is_empty() {
            self.selected_index = 0;
        } else if self.selected_index >= self.filtered_sessions.len() {
            self.selected_index = self.filtered_sessions.len() - 1;
        }
    }

    /// Format relative time from SystemTime
    fn format_relative_time(time: SystemTime) -> String {
        let now = SystemTime::now();
        let diff = now.duration_since(time).unwrap_or_default();
        let secs = diff.as_secs();
        let mins = secs / 60;
        let hours = secs / 3600;
        let days = secs / 86400;

        if mins < 1 {
            "just now".to_string()
        } else if mins < 60 {
            format!("{mins} minute{} ago", if mins != 1 { "s" } else { "" })
        } else if hours < 24 {
            format!("{hours} hour{} ago", if hours != 1 { "s" } else { "" })
        } else if days == 1 {
            "1 day ago".to_string()
        } else if days < 7 {
            format!("{days} days ago")
        } else {
            // Format as date
            let datetime: chrono::DateTime<chrono::Local> = time.into();
            datetime.format("%Y-%m-%d").to_string()
        }
    }

    /// Normalize message text to single line
    fn normalize_message(text: &str) -> String {
        text.replace('\n', " ").trim().to_string()
    }

    /// Render the session list
    pub fn render(&self, width: usize) -> Vec<String> {
        let mut lines = Vec::new();

        // Render search input
        let search_label = "\x1b[2mSearch:\x1b[0m ";
        let cursor_indicator = if self.search_query.is_empty() {
            "\x1b[2m_\x1b[0m"
        } else {
            "_"
        };
        let search_line = format!("{}{}{}", search_label, self.search_query, cursor_indicator);
        lines.push(truncate_to_width(&search_line, width));
        lines.push(String::new()); // Blank line after search

        if self.filtered_sessions.is_empty() {
            lines.push("\x1b[2m  No sessions found\x1b[0m".to_string());
            return lines;
        }

        // Calculate visible range with scrolling
        let start_index = if self.filtered_sessions.len() <= self.max_visible {
            0
        } else {
            let half = self.max_visible / 2;
            let max_start = self.filtered_sessions.len() - self.max_visible;
            self.selected_index.saturating_sub(half).min(max_start)
        };
        let end_index = (start_index + self.max_visible).min(self.filtered_sessions.len());

        // Render visible sessions (2 lines per session + blank line)
        for i in start_index..end_index {
            let session = &self.filtered_sessions[i];
            let is_selected = i == self.selected_index;

            // Normalize first message to single line
            let normalized_message = Self::normalize_message(&session.first_message);

            // First line: cursor + message (truncate to visible width)
            let cursor = if is_selected {
                "\x1b[36m› \x1b[0m" // cyan accent
            } else {
                "  "
            };
            let max_msg_width = width.saturating_sub(2); // Account for cursor (2 visible chars)
            let truncated_msg = truncate_to_width(&normalized_message, max_msg_width);
            let message_line = if is_selected {
                format!("{}\x1b[1m{}\x1b[0m", cursor, truncated_msg) // bold
            } else {
                format!("{}{}", cursor, truncated_msg)
            };

            // Second line: metadata (dimmed)
            let modified = Self::format_relative_time(session.modified);
            let msg_count = format!(
                "{} message{}",
                session.message_count,
                if session.message_count != 1 { "s" } else { "" }
            );
            let metadata = format!("  {} · {}", modified, msg_count);
            let metadata_line = format!("\x1b[2m{}\x1b[0m", truncate_to_width(&metadata, width));

            lines.push(message_line);
            lines.push(metadata_line);
            lines.push(String::new()); // Blank line between sessions
        }

        // Add scroll indicator if needed
        if start_index > 0 || end_index < self.filtered_sessions.len() {
            let scroll_text = format!(
                "  ({}/{})",
                self.selected_index + 1,
                self.filtered_sessions.len()
            );
            let scroll_info = format!("\x1b[2m{}\x1b[0m", truncate_to_width(&scroll_text, width));
            lines.push(scroll_info);
        }

        lines
    }

    /// Handle keyboard input
    pub fn handle_input(&mut self, key_data: &str) {
        // Up arrow
        if matches_key(key_data, "up") {
            if self.selected_index > 0 {
                self.selected_index -= 1;
            }
        }
        // Down arrow
        else if matches_key(key_data, "down") {
            if self.selected_index + 1 < self.filtered_sessions.len() {
                self.selected_index += 1;
            }
        }
        // Enter - select
        else if matches_key(key_data, "enter") {
            if let Some(session) = self.filtered_sessions.get(self.selected_index) {
                if let Some(ref mut on_select) = self.on_select {
                    on_select(session.path.clone());
                }
            }
        }
        // Escape - cancel
        else if matches_key(key_data, "escape") {
            if let Some(ref mut on_cancel) = self.on_cancel {
                on_cancel();
            }
        }
        // Backspace - remove character from search
        else if matches_key(key_data, "backspace") {
            self.search_query.pop();
            self.filter_sessions();
        }
        // Printable characters - add to search
        else if key_data.len() == 1 {
            let ch = key_data.chars().next().unwrap();
            if ch.is_ascii_graphic() || ch == ' ' {
                self.search_query.push(ch);
                self.filter_sessions();
            }
        }
    }

    /// Get the current search query
    pub fn search_query(&self) -> &str {
        &self.search_query
    }

    /// Get the number of filtered sessions
    pub fn filtered_count(&self) -> usize {
        self.filtered_sessions.len()
    }

    /// Check if there are any sessions
    pub fn is_empty(&self) -> bool {
        self.all_sessions.is_empty()
    }

    /// Get the currently selected session path (if any)
    pub fn get_selected(&self) -> Option<PathBuf> {
        self.filtered_sessions
            .get(self.selected_index)
            .map(|s| s.path.clone())
    }
}

/// Session selector component with header and border
pub struct SessionSelectorComponent {
    session_list: SessionList,
}

impl SessionSelectorComponent {
    /// Create a new session selector
    pub fn new(sessions: Vec<SessionInfo>, max_visible: usize) -> Self {
        Self {
            session_list: SessionList::new(sessions, max_visible),
        }
    }

    /// Set the select callback
    pub fn set_on_select(&mut self, callback: impl FnMut(PathBuf) + 'static) {
        self.session_list.on_select = Some(Box::new(callback));
    }

    /// Set the cancel callback
    pub fn set_on_cancel(&mut self, callback: impl FnMut() + 'static) {
        self.session_list.on_cancel = Some(Box::new(callback));
    }

    /// Render the component
    pub fn render(&self, width: usize) -> Vec<String> {
        let mut lines = vec![
            // Spacer
            String::new(),
            // Header
            "\x1b[1mResume Session\x1b[0m".to_string(),
            // Spacer
            String::new(),
            // Top border
            "─".repeat(width.min(80)),
            // Spacer
            String::new(),
        ];

        // Session list
        lines.extend(self.session_list.render(width));

        // Spacer
        lines.push(String::new());

        // Bottom border
        lines.push("─".repeat(width.min(80)));

        lines
    }

    /// Handle keyboard input
    pub fn handle_input(&mut self, key_data: &str) {
        self.session_list.handle_input(key_data);
    }

    /// Check if there are no sessions
    pub fn is_empty(&self) -> bool {
        self.session_list.is_empty()
    }

    /// Get the session list (for accessing callbacks)
    pub fn session_list_mut(&mut self) -> &mut SessionList {
        &mut self.session_list
    }

    /// Get the currently selected session path (if any)
    pub fn get_selected(&self) -> Option<PathBuf> {
        self.session_list.get_selected()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_session(first_message: &str, message_count: usize) -> SessionInfo {
        SessionInfo {
            path: PathBuf::from(format!("/tmp/session-{}.jsonl", message_count)),
            id: format!("session-{}", message_count),
            created: "2024-01-01T00:00:00Z".to_string(),
            modified: SystemTime::now(),
            message_count,
            first_message: first_message.to_string(),
            all_messages_text: first_message.to_string(),
        }
    }

    #[test]
    fn test_session_list_creation() {
        let sessions = vec![
            make_test_session("First session message", 10),
            make_test_session("Second session message", 5),
        ];
        let list = SessionList::new(sessions, 5);
        assert_eq!(list.filtered_count(), 2);
        assert!(!list.is_empty());
    }

    #[test]
    fn test_session_list_filtering() {
        let sessions = vec![
            make_test_session("Hello world", 10),
            make_test_session("Goodbye world", 5),
            make_test_session("Something else", 3),
        ];
        let mut list = SessionList::new(sessions, 5);

        // Initially all sessions visible
        assert_eq!(list.filtered_count(), 3);

        // Filter by "hello"
        list.search_query = "hello".to_string();
        list.filter_sessions();
        assert_eq!(list.filtered_count(), 1);

        // Filter by "world" - matches two
        list.search_query = "world".to_string();
        list.filter_sessions();
        assert_eq!(list.filtered_count(), 2);

        // Filter by non-existent text
        list.search_query = "nonexistent".to_string();
        list.filter_sessions();
        assert_eq!(list.filtered_count(), 0);
    }

    #[test]
    fn test_session_list_navigation() {
        let sessions = vec![
            make_test_session("First", 1),
            make_test_session("Second", 2),
            make_test_session("Third", 3),
        ];
        let mut list = SessionList::new(sessions, 5);

        assert_eq!(list.selected_index, 0);

        // Move down
        list.handle_input("\x1b[B"); // Down arrow (legacy sequence)
        assert_eq!(list.selected_index, 1);

        list.handle_input("\x1b[B");
        assert_eq!(list.selected_index, 2);

        // Can't go past end
        list.handle_input("\x1b[B");
        assert_eq!(list.selected_index, 2);

        // Move up
        list.handle_input("\x1b[A"); // Up arrow
        assert_eq!(list.selected_index, 1);
    }

    #[test]
    fn test_session_list_render() {
        let sessions = vec![make_test_session("Test message here", 5)];
        let list = SessionList::new(sessions, 5);
        let lines = list.render(80);

        // Should have search line, blank, message line, metadata line, blank
        assert!(lines.len() >= 4);
        assert!(lines[0].contains("Search:"));
    }

    #[test]
    fn test_normalize_message() {
        assert_eq!(
            SessionList::normalize_message("hello\nworld\ntest"),
            "hello world test"
        );
        assert_eq!(SessionList::normalize_message("  spaces  "), "spaces");
    }

    #[test]
    fn test_format_relative_time() {
        // Current time should show "just now"
        let now = SystemTime::now();
        let formatted = SessionList::format_relative_time(now);
        assert_eq!(formatted, "just now");
    }

    #[test]
    fn test_session_selector_component() {
        let sessions = vec![
            make_test_session("First session", 10),
            make_test_session("Second session", 5),
        ];
        let component = SessionSelectorComponent::new(sessions, 5);
        let lines = component.render(80);

        // Should have header and borders
        assert!(lines.iter().any(|l| l.contains("Resume Session")));
        assert!(!component.is_empty());
    }

    #[test]
    fn test_empty_session_selector() {
        let component = SessionSelectorComponent::new(vec![], 5);
        assert!(component.is_empty());
        let lines = component.render(80);
        assert!(lines.iter().any(|l| l.contains("No sessions found")));
    }
}
