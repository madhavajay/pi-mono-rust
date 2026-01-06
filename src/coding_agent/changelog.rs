use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct ChangelogEntry {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub content: String,
}

pub fn get_changelog_path() -> Option<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(exe) = env::current_exe() {
        if let Some(parent) = exe.parent() {
            roots.push(parent.to_path_buf());
        }
    }
    if let Ok(cwd) = env::current_dir() {
        if !roots.iter().any(|path| path == &cwd) {
            roots.push(cwd);
        }
    }

    for root in roots {
        for ancestor in root.ancestors() {
            if let Some(path) = find_changelog_in_dir(ancestor) {
                return Some(path);
            }
        }
    }
    None
}

pub fn parse_changelog(path: &Path) -> Vec<ChangelogEntry> {
    if !path.exists() {
        return Vec::new();
    }

    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(_) => return Vec::new(),
    };
    let mut entries = Vec::new();
    let mut current_version: Option<(u32, u32, u32)> = None;
    let mut current_lines: Vec<String> = Vec::new();

    for line in content.lines() {
        if let Some(version) = parse_version_line(line) {
            if let Some((major, minor, patch)) = current_version.take() {
                if !current_lines.is_empty() {
                    entries.push(ChangelogEntry {
                        major,
                        minor,
                        patch,
                        content: current_lines.join("\n").trim().to_string(),
                    });
                }
            }
            current_version = Some(version);
            current_lines = vec![line.to_string()];
            continue;
        }
        if current_version.is_some() {
            current_lines.push(line.to_string());
        }
    }

    if let Some((major, minor, patch)) = current_version.take() {
        if !current_lines.is_empty() {
            entries.push(ChangelogEntry {
                major,
                minor,
                patch,
                content: current_lines.join("\n").trim().to_string(),
            });
        }
    }

    entries
}

fn parse_version_line(line: &str) -> Option<(u32, u32, u32)> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with("## ") {
        return None;
    }
    let header = trimmed.trim_start_matches("## ").trim();
    let header = header.trim_start_matches('[');
    let version_part = header.split([']', ' ']).next().unwrap_or("");
    let mut parts = version_part.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    Some((major, minor, patch))
}

fn find_changelog_in_dir(dir: &Path) -> Option<PathBuf> {
    let direct = dir.join("CHANGELOG.md");
    if direct.exists() {
        return Some(direct);
    }
    let repo = dir
        .join("pi-mono")
        .join("packages")
        .join("coding-agent")
        .join("CHANGELOG.md");
    if repo.exists() {
        return Some(repo);
    }
    None
}
