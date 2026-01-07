use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutocompleteItem {
    pub value: String,
    pub label: String,
    pub description: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutocompleteSuggestions {
    pub items: Vec<AutocompleteItem>,
    pub prefix: String,
}

/// A slash command that can be autocompleted in the editor.
#[derive(Clone, Debug)]
pub struct SlashCommand {
    pub name: String,
    pub description: Option<String>,
}

impl SlashCommand {
    pub fn new(name: impl Into<String>, description: impl Into<Option<String>>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
        }
    }
}

pub struct CombinedAutocompleteProvider {
    commands: Vec<SlashCommand>,
    base_path: PathBuf,
}

impl CombinedAutocompleteProvider {
    pub fn new(commands: Vec<SlashCommand>, base_path: impl Into<PathBuf>) -> Self {
        Self {
            commands,
            base_path: base_path.into(),
        }
    }

    /// Get autocomplete suggestions for the current editor state.
    /// Returns suggestions for slash commands when at the start of input with `/`,
    /// or file path suggestions otherwise.
    pub fn get_suggestions(
        &self,
        lines: &[String],
        cursor_line: usize,
        cursor_col: usize,
    ) -> Option<AutocompleteSuggestions> {
        let current_line = lines.get(cursor_line).map(String::as_str).unwrap_or("");
        let text_before_cursor = slice_to_boundary(current_line, cursor_col);

        // Check for @ file reference (fuzzy search) - must be after a space or at start
        if let Some(at_match) = extract_at_prefix(text_before_cursor) {
            let items = self.get_file_suggestions(&at_match);
            if items.is_empty() {
                return None;
            }
            return Some(AutocompleteSuggestions {
                items,
                prefix: at_match,
            });
        }

        // Check for slash commands at the start of input
        if let Some(prefix) = text_before_cursor.strip_prefix('/') {
            let space_index = text_before_cursor.find(' ');

            if space_index.is_none() {
                // No space yet - complete command names
                // Remove the "/"
                let prefix_lower = prefix.to_lowercase();

                let items: Vec<AutocompleteItem> = self
                    .commands
                    .iter()
                    .filter(|cmd| cmd.name.to_lowercase().starts_with(&prefix_lower))
                    .map(|cmd| AutocompleteItem {
                        value: cmd.name.clone(),
                        label: cmd.name.clone(),
                        description: cmd.description.clone(),
                    })
                    .collect();

                if items.is_empty() {
                    return None;
                }

                return Some(AutocompleteSuggestions {
                    items,
                    prefix: text_before_cursor.to_string(),
                });
            }

            // Space found - could complete command arguments in the future
            // For now, just return None for command arguments
            return None;
        }

        // Check for file paths
        let path_match = self.extract_path_prefix(text_before_cursor, false)?;
        let items = self.get_file_suggestions(&path_match);
        if items.is_empty() {
            return None;
        }

        Some(AutocompleteSuggestions {
            items,
            prefix: path_match,
        })
    }

    /// Apply a selected autocomplete item to the editor text.
    /// Returns the new lines and cursor position.
    pub fn apply_completion(
        &self,
        lines: &[String],
        cursor_line: usize,
        cursor_col: usize,
        item: &AutocompleteItem,
        prefix: &str,
    ) -> (Vec<String>, usize, usize) {
        let current_line = lines.get(cursor_line).cloned().unwrap_or_default();
        let before_prefix = &current_line[..cursor_col.saturating_sub(prefix.len())];
        let after_cursor = &current_line[cursor_col..];

        // Check if we're completing a slash command
        let is_slash_command = prefix.starts_with('/')
            && before_prefix.trim().is_empty()
            && !prefix[1..].contains('/');

        if is_slash_command {
            // This is a command name completion - add "/" prefix and space after
            let new_line = format!("{before_prefix}/{} {after_cursor}", item.value);
            let mut new_lines = lines.to_vec();
            new_lines[cursor_line] = new_line;
            let new_col = before_prefix.len() + item.value.len() + 2; // +2 for "/" and space
            return (new_lines, cursor_line, new_col);
        }

        // Check if we're completing a file attachment (prefix starts with "@")
        if prefix.starts_with('@') {
            let new_line = format!("{before_prefix}{} {after_cursor}", item.value);
            let mut new_lines = lines.to_vec();
            new_lines[cursor_line] = new_line;
            let new_col = before_prefix.len() + item.value.len() + 1; // +1 for space
            return (new_lines, cursor_line, new_col);
        }

        // For file paths, complete the path
        let new_line = format!("{before_prefix}{}{after_cursor}", item.value);
        let mut new_lines = lines.to_vec();
        new_lines[cursor_line] = new_line;
        let new_col = before_prefix.len() + item.value.len();
        (new_lines, cursor_line, new_col)
    }

    pub fn get_force_file_suggestions(
        &self,
        lines: &[String],
        cursor_line: usize,
        cursor_col: usize,
    ) -> Option<AutocompleteSuggestions> {
        let current_line = lines.get(cursor_line).map(String::as_str).unwrap_or("");
        let text_before_cursor = slice_to_boundary(current_line, cursor_col);
        let trimmed = text_before_cursor.trim();

        if trimmed.starts_with('/') && !trimmed.contains(' ') {
            return None;
        }

        let path_match = self.extract_path_prefix(text_before_cursor, true)?;
        let items = self.get_file_suggestions(&path_match);
        if items.is_empty() {
            return None;
        }

        Some(AutocompleteSuggestions {
            items,
            prefix: path_match,
        })
    }

    fn extract_path_prefix(&self, text: &str, force_extract: bool) -> Option<String> {
        if let Some(at_match) = extract_at_prefix(text) {
            return Some(at_match);
        }

        let last_delimiter_index = text
            .rfind([' ', '\t', '"', '\'', '='])
            .map(|idx| idx + 1)
            .unwrap_or(0);
        let path_prefix = text[last_delimiter_index..].to_string();

        if force_extract {
            return Some(path_prefix);
        }

        if path_prefix.contains('/')
            || path_prefix.starts_with('.')
            || path_prefix.starts_with("~/")
        {
            return Some(path_prefix);
        }

        if path_prefix.is_empty() && (text.is_empty() || text.ends_with(' ')) {
            return Some(path_prefix);
        }

        None
    }

    fn expand_home_path(&self, path: &str) -> String {
        if path == "~" {
            return home_dir();
        }

        if let Some(rest) = path.strip_prefix("~/") {
            let mut expanded = PathBuf::from(home_dir());
            expanded.push(rest);
            let mut result = expanded.to_string_lossy().to_string();
            if path.ends_with('/') && !result.ends_with('/') {
                result.push('/');
            }
            return result;
        }

        path.to_string()
    }

    fn get_file_suggestions(&self, prefix: &str) -> Vec<AutocompleteItem> {
        let (fs_prefix, value_prefix) = if let Some(rest) = prefix.strip_prefix('@') {
            (rest, prefix)
        } else {
            (prefix, prefix)
        };

        let expanded_prefix = if fs_prefix.starts_with('~') {
            self.expand_home_path(fs_prefix)
        } else {
            fs_prefix.to_string()
        };

        let (search_dir, search_prefix) = resolve_search_dir(&self.base_path, &expanded_prefix);

        let entries = match fs::read_dir(&search_dir) {
            Ok(entries) => entries,
            Err(_) => return Vec::new(),
        };

        let mut items = Vec::new();
        let search_prefix_lower = search_prefix.to_lowercase();

        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.to_lowercase().starts_with(&search_prefix_lower) {
                continue;
            }

            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(_) => continue,
            };
            let is_dir = file_type.is_dir();
            let mut value = build_value(value_prefix, &name);
            let label = if is_dir {
                value.push('/');
                format!("{name}/")
            } else {
                name.clone()
            };

            items.push(AutocompleteItem {
                value,
                label,
                description: None,
            });
        }

        items.sort_by(|a, b| {
            let a_is_dir = a.value.ends_with('/');
            let b_is_dir = b.value.ends_with('/');
            match (a_is_dir, b_is_dir) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.label.cmp(&b.label),
            }
        });

        items
    }
}

fn resolve_search_dir(base_path: &Path, prefix: &str) -> (PathBuf, String) {
    if prefix.is_empty()
        || prefix == "./"
        || prefix == "../"
        || prefix == "~"
        || prefix == "~/"
        || prefix == "/"
        || prefix.ends_with('/')
    {
        let dir = if Path::new(prefix).is_absolute() {
            PathBuf::from(prefix)
        } else {
            base_path.join(prefix)
        };
        return (dir, String::new());
    }

    let path = Path::new(prefix);
    let search_prefix = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_string();
    let dir = path.parent().unwrap_or_else(|| Path::new(""));
    let search_dir = if path.is_absolute() {
        dir.to_path_buf()
    } else {
        base_path.join(dir)
    };

    (search_dir, search_prefix)
}

fn build_value(prefix: &str, name: &str) -> String {
    if prefix == "@" {
        return format!("@{name}");
    }

    if prefix.ends_with('/') {
        format!("{prefix}{name}")
    } else if let Some(index) = prefix.rfind('/') {
        let head = &prefix[..=index];
        format!("{head}{name}")
    } else {
        name.to_string()
    }
}

fn extract_at_prefix(text: &str) -> Option<String> {
    let at_index = text.rfind('@')?;
    let suffix = &text[at_index..];
    if suffix.chars().any(|ch| ch.is_whitespace()) {
        return None;
    }
    Some(suffix.to_string())
}

fn home_dir() -> String {
    std::env::var("HOME").unwrap_or_else(|_| String::from(""))
}

fn slice_to_boundary(text: &str, index: usize) -> &str {
    if index >= text.len() {
        return text;
    }
    if text.is_char_boundary(index) {
        return &text[..index];
    }
    let mut end = index;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}
