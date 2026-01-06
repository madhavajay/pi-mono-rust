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

pub struct CombinedAutocompleteProvider {
    base_path: PathBuf,
}

impl CombinedAutocompleteProvider {
    pub fn new(base_path: impl Into<PathBuf>) -> Self {
        Self {
            base_path: base_path.into(),
        }
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
