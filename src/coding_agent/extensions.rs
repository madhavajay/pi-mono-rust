use crate::config;
use serde_json::Value;
use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const EXTENSION_SUFFIXES: [&str; 2] = ["ts", "js"];
const UNICODE_SPACES: [char; 8] = [
    '\u{00A0}', '\u{1680}', '\u{2000}', '\u{2001}', '\u{2002}', '\u{2003}', '\u{2004}', '\u{2005}',
];

fn normalize_unicode_spaces(input: &str) -> String {
    input
        .chars()
        .map(|ch| {
            if UNICODE_SPACES.contains(&ch) {
                ' '
            } else {
                ch
            }
        })
        .collect()
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from)
}

fn expand_path(input: &str) -> PathBuf {
    let normalized = normalize_unicode_spaces(input);
    if let Some(stripped) = normalized.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return home.join(stripped);
        }
    }
    if let Some(stripped) = normalized.strip_prefix('~') {
        if let Some(home) = home_dir() {
            if stripped.is_empty() {
                return home;
            }
            return home.join(stripped);
        }
    }
    PathBuf::from(normalized)
}

fn resolve_path(input: &str, cwd: &Path) -> PathBuf {
    let expanded = expand_path(input);
    if expanded.is_absolute() {
        expanded
    } else {
        cwd.join(expanded)
    }
}

fn is_extension_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| EXTENSION_SUFFIXES.contains(&ext))
}

fn read_pi_manifest(package_json: &Path) -> Option<Vec<String>> {
    let data = fs::read_to_string(package_json).ok()?;
    let json: Value = serde_json::from_str(&data).ok()?;
    let extensions = json.get("pi")?.get("extensions")?.as_array()?;
    let mut entries = Vec::new();
    for item in extensions {
        if let Some(path) = item.as_str() {
            entries.push(path.to_string());
        }
    }
    if entries.is_empty() {
        None
    } else {
        Some(entries)
    }
}

fn resolve_extension_entries(dir: &Path) -> Option<Vec<PathBuf>> {
    let package_json = dir.join("package.json");
    if package_json.exists() {
        if let Some(entries) = read_pi_manifest(&package_json) {
            let mut resolved = Vec::new();
            for entry in entries {
                let candidate = dir.join(entry);
                if candidate.exists() {
                    resolved.push(candidate);
                }
            }
            if !resolved.is_empty() {
                return Some(resolved);
            }
        }
    }

    let index_ts = dir.join("index.ts");
    if index_ts.exists() {
        return Some(vec![index_ts]);
    }
    let index_js = dir.join("index.js");
    if index_js.exists() {
        return Some(vec![index_js]);
    }

    None
}

fn discover_extensions_in_dir(dir: &Path) -> Vec<PathBuf> {
    if !dir.exists() {
        return Vec::new();
    }

    let mut discovered = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(_) => continue,
        };

        if (file_type.is_file() || file_type.is_symlink()) && is_extension_file(&path) {
            discovered.push(path);
            continue;
        }

        if file_type.is_dir() || file_type.is_symlink() {
            if let Some(entries) = resolve_extension_entries(&path) {
                discovered.extend(entries);
            }
        }
    }

    discovered
}

pub fn discover_extension_paths(
    configured_paths: &[String],
    cwd: &Path,
    agent_dir: &Path,
) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut all_paths = Vec::new();

    let mut add_path = |path: PathBuf| {
        let resolved = if path.is_absolute() {
            path
        } else {
            cwd.join(path)
        };
        if seen.insert(resolved.clone()) {
            all_paths.push(resolved);
        }
    };

    let global_ext_dir = agent_dir.join("extensions");
    for path in discover_extensions_in_dir(&global_ext_dir) {
        add_path(path);
    }

    let local_ext_dir = cwd.join(config::config_dir_name()).join("extensions");
    for path in discover_extensions_in_dir(&local_ext_dir) {
        add_path(path);
    }

    for path in configured_paths {
        let resolved = resolve_path(path, cwd);
        if resolved.exists() {
            if let Ok(metadata) = fs::metadata(&resolved) {
                if metadata.is_dir() {
                    if let Some(entries) = resolve_extension_entries(&resolved) {
                        for entry in entries {
                            add_path(entry);
                        }
                        continue;
                    }
                }
            }
        }
        add_path(resolved);
    }

    all_paths
}
