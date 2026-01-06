use crate::cli::args::{ExtensionFlagType, ExtensionFlagValue};
use crate::cli::auth::apply_env_api_keys_for_availability;
use crate::coding_agent::extension_host::ExtensionTool;
use crate::coding_agent::{
    discover_extension_paths, ExtensionHost, ExtensionManifest, Model as RegistryModel,
    ModelRegistry, SettingsManager,
};
use crate::config;
use crate::core::session_manager::{SessionInfo, SessionManager};
use crate::{Args, ListModels};
use std::cell::RefCell;
use std::collections::HashMap;
use std::env;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;

pub fn print_help() {
    println!(
        "pi (rust) minimal CLI

Usage:
  pi [options] [messages...]

Options:
  --help, -h       Show this help
  --version, -v    Show version
  --provider       Provider name (anthropic only for now)
  --model          Model id
  --models         Comma-separated model patterns
  --api-key        Override provider API key
  --system-prompt  Custom system prompt (literal or file path)
  --append-system-prompt  Append text to system prompt (literal or file path)
  --tools          Comma-separated tool allowlist
  --thinking       Set thinking level: off, minimal, low, medium, high, xhigh
  --print, -p      Print mode (single-shot)
  --list-models    List available models
  --export <file>  Export session file to HTML and exit
  --mode <mode>    Output mode: text (default), json, rpc
  --extension, -e  Load an extension file (can be used multiple times)
  --no-skills      Disable skills discovery and loading
  --skills         Comma-separated glob patterns to filter skills
  @file            Include file contents in prompt (text or images)

Notes:
  Interactive mode uses a basic TUI (full parity pending).
  Extensions can register additional CLI flags.
  Extension execution (compaction hooks) is supported for .js files only."
    );
}

pub fn collect_extension_flags(manifest: &ExtensionManifest) -> HashMap<String, ExtensionFlagType> {
    let mut flags = HashMap::new();
    for extension in &manifest.extensions {
        for flag in &extension.flags {
            let flag_type = match flag.flag_type.as_deref() {
                Some("boolean") => ExtensionFlagType::Bool,
                Some("string") => ExtensionFlagType::String,
                _ => continue,
            };
            flags.insert(flag.name.clone(), flag_type);
        }
    }
    flags
}

pub fn extension_flag_values_to_json(
    values: &HashMap<String, ExtensionFlagValue>,
) -> HashMap<String, serde_json::Value> {
    values
        .iter()
        .map(|(name, value)| {
            let json_value = match value {
                ExtensionFlagValue::Bool(flag) => serde_json::Value::Bool(*flag),
                ExtensionFlagValue::String(text) => serde_json::Value::String(text.clone()),
            };
            (name.clone(), json_value)
        })
        .collect()
}

pub fn collect_unsupported_flags(_parsed: &Args) -> Vec<&'static str> {
    Vec::new()
}

pub fn build_model_registry(
    api_key_override: Option<&str>,
    provider: Option<&str>,
) -> Result<ModelRegistry, String> {
    let auth_path = config::get_auth_path();
    let mut auth_storage = crate::coding_agent::AuthStorage::new(auth_path);
    apply_env_api_keys_for_availability(&mut auth_storage);
    if let Some(api_key) = api_key_override {
        let provider = provider.unwrap_or("anthropic");
        auth_storage.set_runtime_api_key(provider, api_key);
    }
    Ok(ModelRegistry::new(
        auth_storage,
        Some(config::get_models_path()),
    ))
}

pub fn discover_system_prompt_file() -> Option<PathBuf> {
    let cwd = env::current_dir().ok()?;
    let project_path = cwd.join(config::config_dir_name()).join("SYSTEM.md");
    if project_path.exists() {
        return Some(project_path);
    }

    let global_path = config::get_agent_dir().join("SYSTEM.md");
    if global_path.exists() {
        return Some(global_path);
    }

    None
}

pub fn build_session_manager(parsed: &Args, cwd: &Path) -> SessionManager {
    if parsed.no_session {
        return SessionManager::in_memory();
    }
    if let Some(session) = &parsed.session {
        return SessionManager::open(
            PathBuf::from(session),
            parsed.session_dir.as_ref().map(PathBuf::from),
        );
    }
    if parsed.continue_session {
        return SessionManager::continue_recent(
            cwd.to_path_buf(),
            parsed.session_dir.as_ref().map(PathBuf::from),
        );
    }
    if let Some(session_dir) = &parsed.session_dir {
        return SessionManager::create_with_dir(cwd.to_path_buf(), PathBuf::from(session_dir));
    }
    SessionManager::create(cwd.to_path_buf())
}

pub fn select_resume_session(
    cwd: &Path,
    session_dir: Option<&str>,
) -> Result<Option<PathBuf>, String> {
    let sessions = SessionManager::list(cwd, session_dir.map(PathBuf::from));
    if sessions.is_empty() {
        println!("No sessions found");
        return Ok(None);
    }

    let selection = prompt_for_session(&sessions)?;
    if selection.is_none() {
        println!("No session selected");
    }
    Ok(selection)
}

fn prompt_for_session(sessions: &[SessionInfo]) -> Result<Option<PathBuf>, String> {
    println!("Select a session to resume:");
    for (idx, session) in sessions.iter().enumerate() {
        let preview = truncate_preview(&session.first_message, 80);
        let modified = format_modified_time(session.modified);
        println!(
            "{:>2}) {} (messages: {}, modified: {})",
            idx + 1,
            preview,
            session.message_count,
            modified
        );
    }

    loop {
        print!("Enter number to resume (or press Enter to cancel): ");
        io::stdout()
            .flush()
            .map_err(|err| format!("Failed to prompt: {err}"))?;
        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .map_err(|err| format!("Failed to read input: {err}"))?;
        let trimmed = input.trim();
        if trimmed.is_empty()
            || trimmed.eq_ignore_ascii_case("q")
            || trimmed.eq_ignore_ascii_case("quit")
        {
            return Ok(None);
        }
        let parsed = trimmed.parse::<usize>();
        let Ok(index) = parsed else {
            println!(
                "Invalid selection. Enter a number between 1 and {}.",
                sessions.len()
            );
            continue;
        };
        if index == 0 || index > sessions.len() {
            println!(
                "Invalid selection. Enter a number between 1 and {}.",
                sessions.len()
            );
            continue;
        }
        return Ok(Some(sessions[index - 1].path.clone()));
    }
}

fn format_modified_time(time: std::time::SystemTime) -> String {
    let datetime: chrono::DateTime<chrono::Local> = time.into();
    datetime.format("%Y-%m-%d %H:%M").to_string()
}

fn truncate_preview(text: &str, max_len: usize) -> String {
    if text.chars().count() <= max_len {
        return text.to_string();
    }
    let mut truncated = text.chars().take(max_len).collect::<String>();
    truncated.push_str("...");
    truncated
}

pub fn select_model(parsed: &Args, registry: &ModelRegistry) -> Result<RegistryModel, String> {
    if let (Some(provider), Some(model_id)) = (&parsed.provider, &parsed.model) {
        return registry
            .find(provider, model_id)
            .ok_or_else(|| format!("Model {provider}/{model_id} not found"));
    }
    if let Some(patterns) = &parsed.models {
        let available = registry.get_available();
        let scoped = crate::coding_agent::resolve_model_scope(patterns, &available);
        if let Some(first) = scoped.first() {
            return Ok(first.model.clone());
        }
        return Err(format!(
            "No models match pattern(s): {}",
            patterns.join(", ")
        ));
    }

    if let Some(model) = registry
        .get_available()
        .iter()
        .find(|model| model.provider == "anthropic" && model.id == "claude-opus-4-5")
    {
        return Ok(model.clone());
    }

    registry
        .get_available()
        .first()
        .cloned()
        .ok_or_else(|| "No models available. Set an API key in auth.json or env.".to_string())
}

pub fn attach_extensions_with_host(
    session: &mut crate::coding_agent::AgentSession,
    cwd: &Path,
    preloaded: Option<PreloadedExtensions>,
) {
    if let Some(preloaded) = preloaded {
        let PreloadedExtensions { host, manifest } = preloaded;
        report_extension_manifest(&manifest);
        session.set_extension_host_shared(host);
        return;
    }
    attach_extensions(session, cwd);
}

fn attach_extensions(session: &mut crate::coding_agent::AgentSession, cwd: &Path) {
    let agent_dir = config::get_agent_dir();
    let configured = session.settings_manager.get_extension_paths();
    let discovered = discover_extension_paths(&configured, cwd, &agent_dir);
    if discovered.is_empty() {
        return;
    }
    match ExtensionHost::spawn(&discovered, cwd) {
        Ok((host, manifest)) => {
            report_extension_manifest(&manifest);
            session.set_extension_host_shared(Rc::new(RefCell::new(host)));
        }
        Err(err) => {
            eprintln!("Warning: Failed to load extensions: {err}");
        }
    }
}

pub fn collect_extension_tools(manifest: &ExtensionManifest) -> Vec<ExtensionTool> {
    let mut tools = Vec::new();
    for extension in &manifest.extensions {
        tools.extend(extension.tools.clone());
    }
    tools
}

pub fn report_extension_manifest(manifest: &ExtensionManifest) {
    if !manifest.skipped_paths.is_empty() {
        let skipped = manifest
            .skipped_paths
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        eprintln!("Warning: Skipping unsupported extensions (JS only): {skipped}");
    }
    for error in &manifest.errors {
        eprintln!(
            "Warning: Extension {} failed to load: {}",
            error.extension_path, error.error
        );
    }
}

pub struct PreloadedExtensions {
    pub host: Rc<RefCell<ExtensionHost>>,
    pub manifest: ExtensionManifest,
}

pub fn preload_extensions(
    parsed: &Args,
    cwd: &Path,
) -> (
    Option<PreloadedExtensions>,
    HashMap<String, ExtensionFlagType>,
) {
    let settings_manager = SettingsManager::create("", "");
    let mut extension_paths = settings_manager.get_extension_paths();
    if let Some(paths) = parsed.extensions.as_deref() {
        extension_paths.extend(paths.iter().cloned());
    }
    let discovered = discover_extension_paths(&extension_paths, cwd, &config::get_agent_dir());
    if discovered.is_empty() {
        return (None, HashMap::new());
    }

    match ExtensionHost::spawn(&discovered, cwd) {
        Ok((host, manifest)) => {
            let flag_types = collect_extension_flags(&manifest);
            (
                Some(PreloadedExtensions {
                    host: Rc::new(RefCell::new(host)),
                    manifest,
                }),
                flag_types,
            )
        }
        Err(err) => {
            eprintln!("Warning: Failed to load extensions: {err}");
            (None, HashMap::new())
        }
    }
}

pub fn list_models_mode(parsed: &Args, registry: &ModelRegistry) {
    if let Some(list_models_mode) = &parsed.list_models {
        let search_pattern = match list_models_mode {
            ListModels::All => None,
            ListModels::Pattern(pattern) => Some(pattern.as_str()),
        };
        crate::cli::list_models::list_models(registry, search_pattern);
    }
}
