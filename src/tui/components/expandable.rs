//! Expandable component trait and implementations for collapsible content.
//!
//! This module provides the foundation for tool output expansion/collapse
//! in the interactive TUI mode.

/// Trait for components that can be expanded/collapsed.
pub trait Expandable {
    /// Set the expanded state of the component.
    fn set_expanded(&mut self, expanded: bool);

    /// Get the current expanded state.
    fn is_expanded(&self) -> bool;

    /// Toggle the expanded state.
    fn toggle_expanded(&mut self) {
        let current = self.is_expanded();
        self.set_expanded(!current);
    }
}

/// A text component that can be expanded/collapsed.
///
/// When collapsed, shows only the first N lines with a "... (N more lines)" indicator.
/// When expanded, shows all content.
pub struct ExpandableText {
    /// The full content.
    content: String,
    /// Whether the content is currently expanded.
    expanded: bool,
    /// Maximum number of lines to show when collapsed.
    preview_lines: usize,
    /// Optional title to show before the content.
    title: Option<String>,
}

impl ExpandableText {
    /// Create a new expandable text component.
    ///
    /// # Arguments
    /// * `content` - The full text content
    /// * `preview_lines` - Number of lines to show when collapsed (default: 5)
    pub fn new(content: impl Into<String>, preview_lines: usize) -> Self {
        Self {
            content: content.into(),
            expanded: false,
            preview_lines,
            title: None,
        }
    }

    /// Set the title shown before the content.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set the content.
    pub fn set_content(&mut self, content: impl Into<String>) {
        self.content = content.into();
    }

    /// Get the content.
    pub fn content(&self) -> &str {
        &self.content
    }

    /// Render the expandable text as a string.
    ///
    /// Returns the formatted output based on expanded state.
    pub fn render(&self) -> String {
        let mut result = String::new();

        if let Some(title) = &self.title {
            result.push_str(title);
            if !self.content.is_empty() {
                result.push('\n');
            }
        }

        if self.content.is_empty() {
            return result;
        }

        let lines: Vec<&str> = self.content.lines().collect();
        let total_lines = lines.len();

        if self.expanded || total_lines <= self.preview_lines {
            // Show all lines
            result.push_str(&self.content);
        } else {
            // Show preview lines
            let preview: Vec<&str> = lines.iter().take(self.preview_lines).copied().collect();
            result.push_str(&preview.join("\n"));

            let remaining = total_lines - self.preview_lines;
            result.push_str(&format!("\n... ({} more lines)", remaining));
        }

        result
    }

    /// Get the total number of lines in the content.
    pub fn total_lines(&self) -> usize {
        self.content.lines().count()
    }

    /// Check if the content would be truncated when collapsed.
    pub fn would_truncate(&self) -> bool {
        self.total_lines() > self.preview_lines
    }
}

impl Expandable for ExpandableText {
    fn set_expanded(&mut self, expanded: bool) {
        self.expanded = expanded;
    }

    fn is_expanded(&self) -> bool {
        self.expanded
    }
}

/// Preview configuration for different tool types.
pub struct ToolPreviewConfig {
    /// Number of preview lines for bash output.
    pub bash_preview_lines: usize,
    /// Number of preview lines for read output.
    pub read_preview_lines: usize,
    /// Number of preview lines for write content.
    pub write_preview_lines: usize,
    /// Number of preview lines for ls output.
    pub ls_preview_lines: usize,
    /// Number of preview lines for find output.
    pub find_preview_lines: usize,
    /// Number of preview lines for grep output.
    pub grep_preview_lines: usize,
    /// Number of preview lines for generic tool output.
    pub default_preview_lines: usize,
}

impl Default for ToolPreviewConfig {
    fn default() -> Self {
        Self {
            bash_preview_lines: 5,
            read_preview_lines: 10,
            write_preview_lines: 10,
            ls_preview_lines: 20,
            find_preview_lines: 20,
            grep_preview_lines: 15,
            default_preview_lines: 10,
        }
    }
}

impl ToolPreviewConfig {
    /// Get the preview lines for a tool by name.
    pub fn get_preview_lines(&self, tool_name: &str) -> usize {
        match tool_name {
            "bash" => self.bash_preview_lines,
            "read" => self.read_preview_lines,
            "write" => self.write_preview_lines,
            "ls" => self.ls_preview_lines,
            "find" => self.find_preview_lines,
            "grep" => self.grep_preview_lines,
            _ => self.default_preview_lines,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expandable_text_collapsed() {
        let content = "line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7";
        let expandable = ExpandableText::new(content, 3);

        let rendered = expandable.render();
        assert!(rendered.contains("line 1"));
        assert!(rendered.contains("line 2"));
        assert!(rendered.contains("line 3"));
        assert!(!rendered.contains("line 4"));
        assert!(rendered.contains("... (4 more lines)"));
    }

    #[test]
    fn test_expandable_text_expanded() {
        let content = "line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7";
        let mut expandable = ExpandableText::new(content, 3);
        expandable.set_expanded(true);

        let rendered = expandable.render();
        assert!(rendered.contains("line 1"));
        assert!(rendered.contains("line 7"));
        assert!(!rendered.contains("more lines"));
    }

    #[test]
    fn test_expandable_text_no_truncation_needed() {
        let content = "line 1\nline 2";
        let expandable = ExpandableText::new(content, 5);

        let rendered = expandable.render();
        assert!(rendered.contains("line 1"));
        assert!(rendered.contains("line 2"));
        assert!(!rendered.contains("more lines"));
    }

    #[test]
    fn test_expandable_text_with_title() {
        let content = "output content";
        let expandable = ExpandableText::new(content, 5).with_title("$ my command");

        let rendered = expandable.render();
        assert!(rendered.starts_with("$ my command"));
        assert!(rendered.contains("output content"));
    }

    #[test]
    fn test_toggle_expanded() {
        let mut expandable = ExpandableText::new("test", 5);
        assert!(!expandable.is_expanded());

        expandable.toggle_expanded();
        assert!(expandable.is_expanded());

        expandable.toggle_expanded();
        assert!(!expandable.is_expanded());
    }

    #[test]
    fn test_would_truncate() {
        let short = ExpandableText::new("line 1\nline 2", 5);
        assert!(!short.would_truncate());

        let long = ExpandableText::new("1\n2\n3\n4\n5\n6", 5);
        assert!(long.would_truncate());
    }

    #[test]
    fn test_tool_preview_config() {
        let config = ToolPreviewConfig::default();
        assert_eq!(config.get_preview_lines("bash"), 5);
        assert_eq!(config.get_preview_lines("read"), 10);
        assert_eq!(config.get_preview_lines("unknown_tool"), 10);
    }
}
