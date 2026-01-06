use crate::coding_agent::slash_commands::{parse_command_args, substitute_args};
use crate::config;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct PromptTemplate {
    pub name: String,
    pub description: String,
    pub content: String,
    pub source: String,
}

#[derive(Clone, Debug, Default)]
pub struct LoadPromptTemplatesOptions {
    pub cwd: Option<PathBuf>,
    pub agent_dir: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug)]
enum TemplateSource {
    User,
    Project,
}

pub fn load_prompt_templates(options: LoadPromptTemplatesOptions) -> Vec<PromptTemplate> {
    let cwd = options
        .cwd
        .or_else(|| env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    let agent_dir = options.agent_dir.or_else(|| Some(config::get_agent_dir()));
    let mut templates = Vec::new();

    if let Some(agent_dir) = agent_dir {
        let global_dir = agent_dir.join("prompts");
        templates.extend(load_templates_from_dir(
            &global_dir,
            TemplateSource::User,
            "",
        ));
    }

    let project_dir = cwd.join(config::config_dir_name()).join("prompts");
    templates.extend(load_templates_from_dir(
        &project_dir,
        TemplateSource::Project,
        "",
    ));

    templates
}

pub fn expand_prompt_template(text: &str, templates: &[PromptTemplate]) -> String {
    if !text.starts_with('/') {
        return text.to_string();
    }

    let (name, args_string) = match text.find(' ') {
        Some(index) => (&text[1..index], &text[index + 1..]),
        None => (&text[1..], ""),
    };

    if name.is_empty() {
        return text.to_string();
    }

    let template = templates.iter().find(|template| template.name == name);
    if let Some(template) = template {
        let args = parse_command_args(args_string);
        return substitute_args(&template.content, &args);
    }

    text.to_string()
}

fn parse_frontmatter(content: &str) -> (HashMap<String, String>, String) {
    let mut frontmatter = HashMap::new();
    if !content.starts_with("---") {
        return (frontmatter, content.to_string());
    }

    let end_index = content[3..].find("\n---").map(|index| index + 3);
    let Some(end_index) = end_index else {
        return (frontmatter, content.to_string());
    };

    if content.len() < 4 || end_index < 4 {
        return (frontmatter, content.to_string());
    }

    let frontmatter_block = &content[4..end_index];
    let remaining = content[end_index + 4..].trim().to_string();

    for line in frontmatter_block.lines() {
        if let Some((key, value)) = parse_frontmatter_line(line) {
            frontmatter.insert(key.to_string(), value.to_string());
        }
    }

    (frontmatter, remaining)
}

fn parse_frontmatter_line(line: &str) -> Option<(&str, &str)> {
    let mut parts = line.splitn(2, ':');
    let key = parts.next()?.trim();
    let value = parts.next()?.trim();
    if key.is_empty() {
        return None;
    }
    if !key
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        return None;
    }
    Some((key, value))
}

fn load_templates_from_dir(
    dir: &Path,
    source: TemplateSource,
    subdir: &str,
) -> Vec<PromptTemplate> {
    let mut templates = Vec::new();
    if !dir.exists() {
        return templates;
    }

    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return templates,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(_) => continue,
        };

        if file_type.is_dir() {
            let new_subdir = if subdir.is_empty() {
                name
            } else {
                format!("{subdir}:{name}")
            };
            templates.extend(load_templates_from_dir(&path, source, &new_subdir));
            continue;
        }

        if !(file_type.is_file() || file_type.is_symlink()) || !name.ends_with(".md") {
            continue;
        }

        let raw_content = match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(_) => continue,
        };

        let (frontmatter, content) = parse_frontmatter(&raw_content);
        let template_name = name.trim_end_matches(".md").to_string();
        let source_str = format_source(source, subdir);
        let description = build_description(frontmatter.get("description"), &content, &source_str);

        templates.push(PromptTemplate {
            name: template_name,
            description,
            content,
            source: source_str,
        });
    }

    templates
}

fn format_source(source: TemplateSource, subdir: &str) -> String {
    let prefix = match source {
        TemplateSource::User => "user",
        TemplateSource::Project => "project",
    };
    if subdir.is_empty() {
        format!("({prefix})")
    } else {
        format!("({prefix}:{subdir})")
    }
}

fn build_description(
    frontmatter_description: Option<&String>,
    content: &str,
    source: &str,
) -> String {
    let mut description = frontmatter_description
        .map(|value| value.to_string())
        .unwrap_or_default();

    if description.trim().is_empty() {
        if let Some(line) = content.lines().find(|line| !line.trim().is_empty()) {
            description = truncate_to_length(line.trim(), 60);
        }
    }

    if description.is_empty() {
        return source.to_string();
    }

    format!("{description} {source}")
}

fn truncate_to_length(value: &str, max_len: usize) -> String {
    let mut truncated = String::new();
    for (idx, ch) in value.chars().enumerate() {
        if idx >= max_len {
            break;
        }
        truncated.push(ch);
    }

    if value.chars().count() > max_len {
        truncated.push_str("...");
    }

    truncated
}
