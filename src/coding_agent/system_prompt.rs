use crate::coding_agent::skills::{
    format_skills_for_prompt, load_skills, LoadSkillsOptions, Skill,
};
use chrono::Local;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct ContextFile {
    pub path: String,
    pub content: String,
}

#[derive(Clone, Debug, Default)]
pub struct LoadContextFilesOptions {
    pub cwd: Option<PathBuf>,
    pub agent_dir: Option<PathBuf>,
}

#[derive(Clone, Debug, Default)]
pub struct BuildSystemPromptOptions {
    pub custom_prompt: Option<String>,
    pub append_system_prompt: Option<String>,
    pub selected_tools: Option<Vec<String>>,
    pub skills_enabled: bool,
    pub skills_include: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub agent_dir: Option<PathBuf>,
    pub context_files: Option<Vec<ContextFile>>,
    pub skills: Option<Vec<Skill>>,
}

pub fn resolve_prompt_input(input: Option<&str>, description: &str) -> Option<String> {
    let input = input?;
    if input.trim().is_empty() {
        return None;
    }

    let path = PathBuf::from(input);
    if path.exists() {
        match fs::read_to_string(&path) {
            Ok(content) => Some(content),
            Err(err) => {
                eprintln!(
                    "Warning: Could not read {} file {}: {}",
                    description, input, err
                );
                Some(input.to_string())
            }
        }
    } else {
        Some(input.to_string())
    }
}

pub fn load_project_context_files(options: LoadContextFilesOptions) -> Vec<ContextFile> {
    let cwd = options
        .cwd
        .or_else(|| env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));

    let mut context_files = Vec::new();
    let mut seen = HashSet::new();

    if let Some(agent_dir) = options.agent_dir {
        if let Some(file) = load_context_file_from_dir(&agent_dir) {
            let key = canonical_key(&file.path);
            seen.insert(key);
            context_files.push(file);
        }
    }

    let mut ancestors = Vec::new();
    let mut current = cwd.clone();
    loop {
        if let Some(file) = load_context_file_from_dir(&current) {
            let key = canonical_key(&file.path);
            if !seen.contains(&key) {
                seen.insert(key);
                ancestors.insert(0, file);
            }
        }

        let parent = current.parent();
        if parent.is_none() || parent == Some(&current) {
            break;
        }
        current = parent.unwrap().to_path_buf();
    }

    context_files.extend(ancestors);
    context_files
}

pub fn build_system_prompt(options: BuildSystemPromptOptions) -> String {
    let cwd = options
        .cwd
        .or_else(|| env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    let custom_prompt = resolve_prompt_input(options.custom_prompt.as_deref(), "system prompt");
    let append_prompt = resolve_prompt_input(
        options.append_system_prompt.as_deref(),
        "append system prompt",
    );

    let append_section = append_prompt
        .as_deref()
        .map(|text| format!("\n\n{text}"))
        .unwrap_or_default();

    let selected_tools = options.selected_tools.unwrap_or_else(|| {
        vec![
            "read".to_string(),
            "bash".to_string(),
            "edit".to_string(),
            "write".to_string(),
        ]
    });
    let tools_set = selected_tools
        .iter()
        .map(|tool| tool.to_string())
        .collect::<HashSet<String>>();

    let context_files = options.context_files.unwrap_or_else(|| {
        load_project_context_files(LoadContextFilesOptions {
            cwd: Some(cwd.clone()),
            agent_dir: options.agent_dir.clone(),
        })
    });

    let skills = match options.skills {
        Some(skills) => skills,
        None => {
            if options.skills_enabled {
                let mut load_options = LoadSkillsOptions::new();
                load_options.cwd = Some(cwd.clone());
                load_options.agent_dir = options.agent_dir.clone();
                load_options.include_skills = options.skills_include.clone();
                load_skills(load_options).skills
            } else {
                Vec::new()
            }
        }
    };

    let date_time = Local::now()
        .format("%A, %B %-d, %Y, %I:%M:%S %p %Z")
        .to_string();
    let cwd_display = cwd.display();

    if let Some(prompt) = custom_prompt {
        let mut prompt = prompt;
        prompt.push_str(&append_section);

        if !context_files.is_empty() {
            prompt.push_str("\n\n# Project Context\n\n");
            prompt.push_str("The following project context files have been loaded:\n\n");
            for file in &context_files {
                prompt.push_str(&format!("## {}\n\n{}\n\n", file.path, file.content));
            }
        }

        if tools_set.contains("read") && !skills.is_empty() {
            prompt.push_str(&format_skills_for_prompt(&skills));
        }

        prompt.push_str(&format!("\nCurrent date and time: {date_time}"));
        prompt.push_str(&format!("\nCurrent working directory: {cwd_display}"));
        return prompt;
    }

    let tool_descriptions = tool_descriptions();
    let tools_list = selected_tools
        .iter()
        .map(|tool| {
            let desc = tool_descriptions
                .get(tool.as_str())
                .copied()
                .unwrap_or("Tool");
            format!("- {tool}: {desc}")
        })
        .collect::<Vec<_>>()
        .join("\n");

    let has_bash = tools_set.contains("bash");
    let has_edit = tools_set.contains("edit");
    let has_write = tools_set.contains("write");
    let has_grep = tools_set.contains("grep");
    let has_find = tools_set.contains("find");
    let has_ls = tools_set.contains("ls");
    let has_read = tools_set.contains("read");

    let mut guidelines = Vec::new();
    if !has_bash && !has_edit && !has_write {
        guidelines.push(
            "You are in READ-ONLY mode - you cannot modify files or execute arbitrary commands",
        );
    }

    if has_bash && !has_edit && !has_write {
        guidelines.push(
            "Use bash ONLY for read-only operations (git log, gh issue view, curl, etc.) - do NOT modify any files",
        );
    }

    if has_bash && !has_grep && !has_find && !has_ls {
        guidelines.push("Use bash for file operations like ls, grep, find");
    } else if has_bash && (has_grep || has_find || has_ls) {
        guidelines.push(
            "Prefer grep/find/ls tools over bash for file exploration (faster, respects .gitignore)",
        );
    }

    if has_read && has_edit {
        guidelines.push("Use read to examine files before editing. You must use this tool instead of cat or sed.");
    }

    if has_edit {
        guidelines.push("Use edit for precise changes (old text must match exactly)");
    }

    if has_write {
        guidelines.push("Use write only for new files or complete rewrites");
    }

    if has_edit || has_write {
        guidelines.push(
            "When summarizing your actions, output plain text directly - do NOT use cat or bash to display what you did",
        );
    }

    guidelines.push("Be concise in your responses");
    guidelines.push("Show file paths clearly when working with files");

    let guidelines = guidelines
        .iter()
        .map(|line| format!("- {line}"))
        .collect::<Vec<_>>()
        .join("\n");

    let docs = resolve_docs_paths(&cwd);
    let mut prompt = format!(
        "You are an expert coding assistant. You help users with coding tasks by reading files, executing commands, editing code, and writing new files.\n\nAvailable tools:\n{tools_list}\n\nIn addition to the tools above, you may have access to other custom tools depending on the project.\n\nGuidelines:\n{guidelines}\n\nDocumentation:\n- Main documentation: {}\n- Additional docs: {}\n- Examples: {} (extensions, custom tools, SDK)\n- When asked to create: custom models/providers (README.md), extensions (docs/extensions.md, examples/extensions/), themes (docs/theme.md), skills (docs/skills.md)\n- Always read the doc, examples, AND follow .md cross-references before implementing",
        docs.readme, docs.docs, docs.examples
    );

    if !append_section.is_empty() {
        prompt.push_str(&append_section);
    }

    if !context_files.is_empty() {
        prompt.push_str("\n\n# Project Context\n\n");
        prompt.push_str("The following project context files have been loaded:\n\n");
        for file in &context_files {
            prompt.push_str(&format!("## {}\n\n{}\n\n", file.path, file.content));
        }
    }

    if has_read && !skills.is_empty() {
        prompt.push_str(&format_skills_for_prompt(&skills));
    }

    prompt.push_str(&format!("\nCurrent date and time: {date_time}"));
    prompt.push_str(&format!("\nCurrent working directory: {cwd_display}"));
    prompt
}

fn load_context_file_from_dir(dir: &Path) -> Option<ContextFile> {
    let candidates = ["AGENTS.md", "CLAUDE.md"];
    for filename in candidates {
        let path = dir.join(filename);
        if !path.exists() {
            continue;
        }
        match fs::read_to_string(&path) {
            Ok(content) => {
                return Some(ContextFile {
                    path: path.display().to_string(),
                    content,
                })
            }
            Err(err) => {
                eprintln!("Warning: Could not read {}: {}", path.display(), err);
            }
        }
    }
    None
}

fn canonical_key(path: &str) -> String {
    fs::canonicalize(path)
        .ok()
        .and_then(|path| path.to_str().map(|value| value.to_string()))
        .unwrap_or_else(|| path.to_string())
}

struct DocsPaths {
    readme: String,
    docs: String,
    examples: String,
}

fn resolve_docs_paths(cwd: &Path) -> DocsPaths {
    if let Some(root) = find_coding_agent_root(cwd) {
        return DocsPaths {
            readme: root.join("README.md").display().to_string(),
            docs: root.join("docs").display().to_string(),
            examples: root.join("examples").display().to_string(),
        };
    }

    if let Some(root) = find_readme_root(cwd) {
        return DocsPaths {
            readme: root.join("README.md").display().to_string(),
            docs: root.join("docs").display().to_string(),
            examples: root.join("examples").display().to_string(),
        };
    }

    DocsPaths {
        readme: cwd.join("README.md").display().to_string(),
        docs: cwd.join("docs").display().to_string(),
        examples: cwd.join("examples").display().to_string(),
    }
}

fn find_coding_agent_root(start: &Path) -> Option<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        let candidate = current
            .join("pi-mono")
            .join("packages")
            .join("coding-agent");
        if candidate.join("README.md").exists() {
            return Some(candidate);
        }
        if let Some(parent) = current.parent() {
            if parent == current {
                break;
            }
            current = parent.to_path_buf();
        } else {
            break;
        }
    }
    None
}

fn find_readme_root(start: &Path) -> Option<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        if current.join("README.md").exists() {
            return Some(current);
        }
        if let Some(parent) = current.parent() {
            if parent == current {
                break;
            }
            current = parent.to_path_buf();
        } else {
            break;
        }
    }
    None
}

fn tool_descriptions() -> HashMap<&'static str, &'static str> {
    let mut map = HashMap::new();
    map.insert("read", "Read file contents");
    map.insert("bash", "Execute bash commands (ls, grep, find, etc.)");
    map.insert(
        "edit",
        "Make surgical edits to files (find exact text and replace)",
    );
    map.insert("write", "Create or overwrite files");
    map.insert(
        "grep",
        "Search file contents for patterns (respects .gitignore)",
    );
    map.insert("find", "Find files by glob pattern (respects .gitignore)");
    map.insert("ls", "List directory contents");
    map
}
