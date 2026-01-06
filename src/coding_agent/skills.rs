use glob::Pattern;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const ALLOWED_FRONTMATTER_FIELDS: [&str; 6] = [
    "name",
    "description",
    "license",
    "compatibility",
    "metadata",
    "allowed-tools",
];
const MAX_NAME_LENGTH: usize = 64;
const MAX_DESCRIPTION_LENGTH: usize = 1024;
const CONFIG_DIR_NAME: &str = ".pi";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub file_path: String,
    pub base_dir: String,
    pub source: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillWarning {
    pub skill_path: String,
    pub message: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LoadSkillsResult {
    pub skills: Vec<Skill>,
    pub warnings: Vec<SkillWarning>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadSkillsFromDirOptions {
    pub dir: PathBuf,
    pub source: String,
}

#[derive(Clone, Debug)]
pub struct LoadSkillsOptions {
    pub cwd: Option<PathBuf>,
    pub agent_dir: Option<PathBuf>,
    pub enable_codex_user: bool,
    pub enable_claude_user: bool,
    pub enable_claude_project: bool,
    pub enable_pi_user: bool,
    pub enable_pi_project: bool,
    pub custom_directories: Vec<String>,
    pub ignored_skills: Vec<String>,
    pub include_skills: Vec<String>,
}

impl LoadSkillsOptions {
    pub fn new() -> Self {
        Self {
            cwd: None,
            agent_dir: None,
            enable_codex_user: true,
            enable_claude_user: true,
            enable_claude_project: true,
            enable_pi_user: true,
            enable_pi_project: true,
            custom_directories: Vec::new(),
            ignored_skills: Vec::new(),
            include_skills: Vec::new(),
        }
    }
}

impl Default for LoadSkillsOptions {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy)]
enum SkillFormat {
    Recursive,
    Claude,
}

#[derive(Default)]
struct SkillFrontmatter {
    name: Option<String>,
    description: Option<String>,
}

pub fn load_skills_from_dir(options: LoadSkillsFromDirOptions) -> LoadSkillsResult {
    load_skills_from_dir_internal(&options.dir, &options.source, SkillFormat::Recursive)
}

pub fn format_skills_for_prompt(skills: &[Skill]) -> String {
    if skills.is_empty() {
        return String::new();
    }

    let mut lines = vec![
        "\n\nThe following skills provide specialized instructions for specific tasks.".to_string(),
        "Use the read tool to load a skill's file when the task matches its description."
            .to_string(),
        String::new(),
        "<available_skills>".to_string(),
    ];

    for skill in skills {
        lines.push("  <skill>".to_string());
        lines.push(format!("    <name>{}</name>", escape_xml(&skill.name)));
        lines.push(format!(
            "    <description>{}</description>",
            escape_xml(&skill.description)
        ));
        lines.push(format!(
            "    <location>{}</location>",
            escape_xml(&skill.file_path)
        ));
        lines.push("  </skill>".to_string());
    }

    lines.push("</available_skills>".to_string());
    lines.join("\n")
}

pub fn load_skills(options: LoadSkillsOptions) -> LoadSkillsResult {
    let cwd = options
        .cwd
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let agent_dir = options
        .agent_dir
        .unwrap_or_else(|| home_dir().join(CONFIG_DIR_NAME).join("agent"));

    let mut skill_map: HashMap<String, Skill> = HashMap::new();
    let mut real_paths: HashSet<String> = HashSet::new();
    let mut warnings: Vec<SkillWarning> = Vec::new();
    let mut collision_warnings: Vec<SkillWarning> = Vec::new();

    let include_patterns = compile_patterns(&options.include_skills);
    let ignore_patterns = compile_patterns(&options.ignored_skills);

    let matches_include = |name: &str| -> bool {
        if include_patterns.is_empty() {
            return true;
        }
        include_patterns.iter().any(|p| p.matches(name))
    };

    let matches_ignore = |name: &str| -> bool {
        if ignore_patterns.is_empty() {
            return false;
        }
        ignore_patterns.iter().any(|p| p.matches(name))
    };

    let mut add_skills = |result: LoadSkillsResult| {
        warnings.extend(result.warnings);
        for skill in result.skills {
            if matches_ignore(&skill.name) {
                continue;
            }
            if !matches_include(&skill.name) {
                continue;
            }

            let real_path = fs::canonicalize(&skill.file_path)
                .ok()
                .and_then(|path| path.to_str().map(|s| s.to_string()))
                .unwrap_or_else(|| skill.file_path.clone());
            if real_paths.contains(&real_path) {
                continue;
            }

            if let Some(existing) = skill_map.get(&skill.name) {
                collision_warnings.push(SkillWarning {
                    skill_path: skill.file_path.clone(),
                    message: format!(
                        "name collision: \"{}\" already loaded from {}, skipping this one",
                        skill.name, existing.file_path
                    ),
                });
                continue;
            }

            real_paths.insert(real_path);
            skill_map.insert(skill.name.clone(), skill);
        }
    };

    if options.enable_codex_user {
        add_skills(load_skills_from_dir_internal(
            &home_dir().join(".codex").join("skills"),
            "codex-user",
            SkillFormat::Recursive,
        ));
    }

    if options.enable_claude_user {
        add_skills(load_skills_from_dir_internal(
            &home_dir().join(".claude").join("skills"),
            "claude-user",
            SkillFormat::Claude,
        ));
    }

    if options.enable_claude_project {
        add_skills(load_skills_from_dir_internal(
            &cwd.join(".claude").join("skills"),
            "claude-project",
            SkillFormat::Claude,
        ));
    }

    if options.enable_pi_user {
        add_skills(load_skills_from_dir_internal(
            &agent_dir.join("skills"),
            "user",
            SkillFormat::Recursive,
        ));
    }

    if options.enable_pi_project {
        add_skills(load_skills_from_dir_internal(
            &cwd.join(CONFIG_DIR_NAME).join("skills"),
            "project",
            SkillFormat::Recursive,
        ));
    }

    for custom_dir in &options.custom_directories {
        let expanded = expand_tilde(custom_dir);
        add_skills(load_skills_from_dir_internal(
            Path::new(&expanded),
            "custom",
            SkillFormat::Recursive,
        ));
    }

    warnings.extend(collision_warnings);
    LoadSkillsResult {
        skills: skill_map.into_values().collect(),
        warnings,
    }
}

fn load_skills_from_dir_internal(
    dir: &Path,
    source: &str,
    format: SkillFormat,
) -> LoadSkillsResult {
    let mut skills = Vec::new();
    let mut warnings = Vec::new();

    if !dir.exists() {
        return LoadSkillsResult { skills, warnings };
    }

    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return LoadSkillsResult { skills, warnings },
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy().to_string();
        if name.starts_with('.') || name == "node_modules" {
            continue;
        }

        let full_path = entry.path();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(_) => continue,
        };

        let mut is_dir = file_type.is_dir();
        let mut is_file = file_type.is_file();
        if file_type.is_symlink() {
            match fs::metadata(&full_path) {
                Ok(meta) => {
                    is_dir = meta.is_dir();
                    is_file = meta.is_file();
                }
                Err(_) => continue,
            }
        }

        match format {
            SkillFormat::Recursive => {
                if is_dir {
                    let result = load_skills_from_dir_internal(&full_path, source, format);
                    skills.extend(result.skills);
                    warnings.extend(result.warnings);
                } else if is_file && name == "SKILL.md" {
                    let result = load_skill_from_file(&full_path, source);
                    if let Some(skill) = result.skill {
                        skills.push(skill);
                    }
                    warnings.extend(result.warnings);
                }
            }
            SkillFormat::Claude => {
                if !is_dir {
                    continue;
                }
                let skill_file = full_path.join("SKILL.md");
                if !skill_file.exists() {
                    continue;
                }
                let result = load_skill_from_file(&skill_file, source);
                if let Some(skill) = result.skill {
                    skills.push(skill);
                }
                warnings.extend(result.warnings);
            }
        }
    }

    LoadSkillsResult { skills, warnings }
}

struct LoadSkillResult {
    skill: Option<Skill>,
    warnings: Vec<SkillWarning>,
}

fn load_skill_from_file(file_path: &Path, source: &str) -> LoadSkillResult {
    let mut warnings = Vec::new();

    let raw_content = match fs::read_to_string(file_path) {
        Ok(content) => content,
        Err(_) => {
            return LoadSkillResult {
                skill: None,
                warnings,
            }
        }
    };

    let (frontmatter, all_keys) = parse_frontmatter(&raw_content);
    let skill_dir = file_path.parent().unwrap_or(Path::new("")).to_path_buf();
    let parent_dir_name = skill_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_string();

    for error in validate_frontmatter_fields(&all_keys) {
        warnings.push(SkillWarning {
            skill_path: file_path.display().to_string(),
            message: error,
        });
    }

    for error in validate_description(frontmatter.description.as_deref()) {
        warnings.push(SkillWarning {
            skill_path: file_path.display().to_string(),
            message: error,
        });
    }

    let name = frontmatter
        .name
        .clone()
        .unwrap_or_else(|| parent_dir_name.clone());

    for error in validate_name(&name, &parent_dir_name) {
        warnings.push(SkillWarning {
            skill_path: file_path.display().to_string(),
            message: error,
        });
    }

    let description = match frontmatter.description {
        Some(description) if !description.trim().is_empty() => description,
        _ => {
            return LoadSkillResult {
                skill: None,
                warnings,
            }
        }
    };

    LoadSkillResult {
        skill: Some(Skill {
            name,
            description,
            file_path: file_path.display().to_string(),
            base_dir: skill_dir.display().to_string(),
            source: source.to_string(),
        }),
        warnings,
    }
}

fn parse_frontmatter(content: &str) -> (SkillFrontmatter, Vec<String>) {
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
    if !normalized.starts_with("---") {
        return (SkillFrontmatter::default(), Vec::new());
    }

    let remainder = &normalized[3..];
    let end_offset = match remainder.find("\n---") {
        Some(offset) => offset,
        None => return (SkillFrontmatter::default(), Vec::new()),
    };
    let end_index = 3 + end_offset;
    if normalized.len() < 4 || end_index < 4 || end_index > normalized.len() {
        return (SkillFrontmatter::default(), Vec::new());
    }

    let frontmatter_block = &normalized[4..end_index];
    let mut frontmatter = SkillFrontmatter::default();
    let mut all_keys = Vec::new();

    for line in frontmatter_block.lines() {
        let mut parts = line.splitn(2, ':');
        let key = match parts.next() {
            Some(key) => key.trim(),
            None => continue,
        };
        let value = match parts.next() {
            Some(value) => value.trim(),
            None => continue,
        };

        if !is_valid_frontmatter_key(key) {
            continue;
        }

        let value = strip_quotes(value);
        all_keys.push(key.to_string());
        if key == "name" {
            frontmatter.name = Some(value);
        } else if key == "description" {
            frontmatter.description = Some(value);
        }
    }

    (frontmatter, all_keys)
}

fn strip_quotes(value: &str) -> String {
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        if (bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\'')
        {
            return value[1..value.len() - 1].to_string();
        }
    }
    value.to_string()
}

fn is_valid_frontmatter_key(key: &str) -> bool {
    let mut chars = key.chars();
    let first = match chars.next() {
        Some(ch) => ch,
        None => return false,
    };
    if !(first.is_ascii_alphanumeric() || first == '_') {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

fn validate_name(name: &str, parent_dir_name: &str) -> Vec<String> {
    let mut errors = Vec::new();

    if name != parent_dir_name {
        errors.push(format!(
            "name \"{}\" does not match parent directory \"{}\"",
            name, parent_dir_name
        ));
    }

    if name.len() > MAX_NAME_LENGTH {
        errors.push(format!(
            "name exceeds {} characters ({})",
            MAX_NAME_LENGTH,
            name.len()
        ));
    }

    if !name
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    {
        errors.push(
            "name contains invalid characters (must be lowercase a-z, 0-9, hyphens only)"
                .to_string(),
        );
    }

    if name.starts_with('-') || name.ends_with('-') {
        errors.push("name must not start or end with a hyphen".to_string());
    }

    if name.contains("--") {
        errors.push("name must not contain consecutive hyphens".to_string());
    }

    errors
}

fn validate_description(description: Option<&str>) -> Vec<String> {
    let mut errors = Vec::new();

    match description {
        Some(text) if !text.trim().is_empty() => {
            if text.len() > MAX_DESCRIPTION_LENGTH {
                errors.push(format!(
                    "description exceeds {} characters ({})",
                    MAX_DESCRIPTION_LENGTH,
                    text.len()
                ));
            }
        }
        _ => {
            errors.push("description is required".to_string());
        }
    }

    errors
}

fn validate_frontmatter_fields(keys: &[String]) -> Vec<String> {
    let mut errors = Vec::new();
    for key in keys {
        if !ALLOWED_FRONTMATTER_FIELDS.contains(&key.as_str()) {
            errors.push(format!("unknown frontmatter field \"{}\"", key));
        }
    }
    errors
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn home_dir() -> PathBuf {
    env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(""))
}

fn expand_tilde(path: &str) -> String {
    if path == "~" {
        return home_dir().to_string_lossy().to_string();
    }
    if let Some(rest) = path.strip_prefix("~/") {
        return home_dir().join(rest).to_string_lossy().to_string();
    }
    if let Some(rest) = path.strip_prefix("~\\") {
        return home_dir().join(rest).to_string_lossy().to_string();
    }
    path.to_string()
}

fn compile_patterns(patterns: &[String]) -> Vec<Pattern> {
    patterns
        .iter()
        .filter_map(|pattern| Pattern::new(pattern).ok())
        .collect()
}
