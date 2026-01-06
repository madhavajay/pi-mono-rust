//! Codex prompts and bridge messages
//!
//! Contains the system prompts used to adapt Codex models to the Pi toolset.

use crate::config;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Codex-Pi bridge prompt - adapts Codex CLI expectations to Pi's toolset.
pub const CODEX_PI_BRIDGE: &str = r#"# Codex Running in Pi

You are running Codex through pi, a terminal coding assistant. The tools and rules differ from Codex CLI.

## CRITICAL: Tool Replacements

<critical_rule priority="0">
❌ APPLY_PATCH DOES NOT EXIST → ✅ USE "edit" INSTEAD
- NEVER use: apply_patch, applyPatch
- ALWAYS use: edit for ALL file modifications
</critical_rule>

<critical_rule priority="0">
❌ UPDATE_PLAN DOES NOT EXIST
- NEVER use: update_plan, updatePlan, read_plan, readPlan, todowrite, todoread
- There is no plan tool in this environment
</critical_rule>

## Available Tools (pi)

- read  - Read file contents
- bash  - Execute bash commands
- edit  - Modify files with exact find/replace (requires prior read)
- write - Create or overwrite files
- grep  - Search file contents (read-only)
- find  - Find files by glob pattern (read-only)
- ls    - List directory contents (read-only)

## Usage Rules

- Read before edit; use read instead of cat/sed for file contents
- Use edit for surgical changes; write only for new files or complete rewrites
- Prefer grep/find/ls over bash for discovery
- Be concise and show file paths clearly when working with files

## Verification Checklist

1. Using edit, not apply_patch
2. No plan tools used
3. Only the tools listed above are called

Below are additional system instruction you MUST follow when responding:
"#;

/// Tool remap message for non-codex mode - provides tool replacement instructions.
pub const TOOL_REMAP_MESSAGE: &str = r#"<user_instructions priority="0">
<environment_override priority="0">
YOU ARE IN A DIFFERENT ENVIRONMENT. These instructions override ALL previous tool references.
</environment_override>

<tool_replacements priority="0">
<critical_rule priority="0">
❌ APPLY_PATCH DOES NOT EXIST → ✅ USE "edit" INSTEAD
- NEVER use: apply_patch, applyPatch
- ALWAYS use: edit tool for ALL file modifications
</critical_rule>

<critical_rule priority="0">
❌ UPDATE_PLAN DOES NOT EXIST
- NEVER use: update_plan, updatePlan, read_plan, readPlan, todowrite, todoread
- There is no plan tool in this environment
</critical_rule>
</tool_replacements>

<available_tools priority="0">
File Operations:
  • read  - Read file contents
  • edit  - Modify files with exact find/replace
  • write - Create or overwrite files

Search/Discovery:
  • grep  - Search file contents for patterns (read-only)
  • find  - Find files by glob pattern (read-only)
  • ls    - List directory contents (read-only)

Execution:
  • bash  - Run shell commands
</available_tools>

<verification_checklist priority="0">
Before file modifications:
1. Am I using "edit" NOT "apply_patch"?
2. Am I avoiding plan tools entirely?
3. Am I using only the tools listed above?
</verification_checklist>
</user_instructions>"#;

/// Model family for prompt selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelFamily {
    Gpt52Codex,
    CodexMax,
    Codex,
    Gpt52,
    Gpt51,
}

impl ModelFamily {
    /// Get the model family from a normalized model name
    pub fn from_model(normalized_model: &str) -> Self {
        let lower = normalized_model.to_lowercase();

        if lower.contains("gpt-5.2-codex") || lower.contains("gpt 5.2 codex") {
            ModelFamily::Gpt52Codex
        } else if lower.contains("codex-max") {
            ModelFamily::CodexMax
        } else if lower.contains("codex") || lower.starts_with("codex-") {
            ModelFamily::Codex
        } else if lower.contains("gpt-5.2") {
            ModelFamily::Gpt52
        } else {
            ModelFamily::Gpt51
        }
    }

    /// Get the GitHub prompt file name for this model family
    fn prompt_file(&self) -> &'static str {
        match self {
            ModelFamily::Gpt52Codex => "gpt-5.2-codex_prompt.md",
            ModelFamily::CodexMax => "gpt-5.1-codex-max_prompt.md",
            ModelFamily::Codex => "gpt_5_codex_prompt.md",
            ModelFamily::Gpt52 => "gpt_5_2_prompt.md",
            ModelFamily::Gpt51 => "gpt_5_1_prompt.md",
        }
    }

    /// Get the cache file name for this model family
    fn cache_file(&self) -> &'static str {
        match self {
            ModelFamily::Gpt52Codex => "gpt-5.2-codex-instructions.md",
            ModelFamily::CodexMax => "codex-max-instructions.md",
            ModelFamily::Codex => "codex-instructions.md",
            ModelFamily::Gpt52 => "gpt-5.2-instructions.md",
            ModelFamily::Gpt51 => "gpt-5.1-instructions.md",
        }
    }
}

/// Cache metadata for prompt files
#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct CacheMetadata {
    pub etag: Option<String>,
    pub tag: String,
    pub last_checked: u64,
    pub url: String,
}

const GITHUB_API_RELEASES: &str = "https://api.github.com/repos/openai/codex/releases/latest";
const GITHUB_HTML_RELEASES: &str = "https://github.com/openai/codex/releases/latest";
const CACHE_TTL_MS: u64 = 15 * 60 * 1000; // 15 minutes

/// Get the cache directory for Codex prompts
fn get_cache_dir() -> PathBuf {
    config::get_agent_dir().join("cache").join("openai-codex")
}

/// Get the current timestamp in milliseconds
fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Get the latest release tag from GitHub
fn get_latest_release_tag() -> Result<String, String> {
    use reqwest::blocking::Client;

    let client = Client::new();

    // Try API first
    if let Ok(response) = client.get(GITHUB_API_RELEASES).send() {
        if response.status().is_success() {
            if let Ok(json) = response.json::<serde_json::Value>() {
                if let Some(tag) = json.get("tag_name").and_then(|v| v.as_str()) {
                    return Ok(tag.to_string());
                }
            }
        }
    }

    // Fallback to HTML scraping
    let response = client
        .get(GITHUB_HTML_RELEASES)
        .send()
        .map_err(|e| format!("Failed to fetch releases: {}", e))?;

    // Check for redirect URL
    let final_url = response.url().to_string();
    if let Some(tag) = final_url.split("/tag/").last() {
        if !tag.contains('/') && !tag.is_empty() {
            return Ok(tag.to_string());
        }
    }

    // Try regex on HTML
    let html = response.text().map_err(|e| e.to_string())?;
    let pattern = regex::Regex::new(r#"/openai/codex/releases/tag/([^"]+)"#).unwrap();
    if let Some(captures) = pattern.captures(&html) {
        if let Some(tag) = captures.get(1) {
            return Ok(tag.as_str().to_string());
        }
    }

    Err("Failed to determine latest release tag from GitHub".to_string())
}

/// Get the Codex instructions for a model.
///
/// This function fetches and caches the Codex instructions from GitHub.
/// It uses ETag-based caching to minimize network requests.
pub fn get_codex_instructions(normalized_model: &str) -> Result<String, String> {
    let model_family = ModelFamily::from_model(normalized_model);
    let prompt_file = model_family.prompt_file();
    let cache_dir = get_cache_dir();
    let cache_file = cache_dir.join(model_family.cache_file());
    let cache_meta_file = cache_dir.join(format!(
        "{}-meta.json",
        model_family.cache_file().trim_end_matches(".md")
    ));

    // Load existing cache metadata
    let mut cached_etag: Option<String> = None;
    let mut cached_tag: Option<String> = None;
    let mut cached_timestamp: Option<u64> = None;

    if cache_meta_file.exists() {
        if let Ok(content) = fs::read_to_string(&cache_meta_file) {
            if let Ok(metadata) = serde_json::from_str::<CacheMetadata>(&content) {
                cached_etag = metadata.etag;
                cached_tag = Some(metadata.tag);
                cached_timestamp = Some(metadata.last_checked);
            }
        }
    }

    // Check if cache is fresh
    if let Some(timestamp) = cached_timestamp {
        if now_millis() - timestamp < CACHE_TTL_MS && cache_file.exists() {
            if let Ok(content) = fs::read_to_string(&cache_file) {
                return Ok(content);
            }
        }
    }

    // Try to fetch from GitHub
    match fetch_codex_instructions(
        prompt_file,
        &cache_file,
        &cache_meta_file,
        &cache_dir,
        cached_etag.as_deref(),
        cached_tag.as_deref(),
    ) {
        Ok(instructions) => Ok(instructions),
        Err(e) => {
            eprintln!(
                "[openai-codex] Failed to fetch {:?} instructions from GitHub: {}",
                model_family, e
            );

            // Try cached file
            if cache_file.exists() {
                eprintln!(
                    "[openai-codex] Using cached {:?} instructions",
                    model_family
                );
                if let Ok(content) = fs::read_to_string(&cache_file) {
                    return Ok(content);
                }
            }

            // Try bundled fallback
            let fallback_path = get_fallback_prompt_path();
            if fallback_path.exists() {
                eprintln!(
                    "[openai-codex] Falling back to bundled instructions for {:?}",
                    model_family
                );
                if let Ok(content) = fs::read_to_string(&fallback_path) {
                    return Ok(content);
                }
            }

            Err(format!(
                "No cached Codex instructions available for {:?}",
                model_family
            ))
        }
    }
}

fn get_fallback_prompt_path() -> PathBuf {
    // In a real implementation, this would be an embedded resource
    // For now, we'll return a path that might exist
    config::get_agent_dir().join("codex-instructions.md")
}

fn fetch_codex_instructions(
    prompt_file: &str,
    cache_file: &PathBuf,
    cache_meta_file: &PathBuf,
    cache_dir: &PathBuf,
    cached_etag: Option<&str>,
    cached_tag: Option<&str>,
) -> Result<String, String> {
    use reqwest::blocking::Client;
    use reqwest::header::{HeaderMap, IF_NONE_MATCH};

    let latest_tag = get_latest_release_tag()?;
    let instructions_url = format!(
        "https://raw.githubusercontent.com/openai/codex/{}/codex-rs/core/{}",
        latest_tag, prompt_file
    );

    // Clear cached ETag if tag changed
    let effective_etag = if cached_tag == Some(latest_tag.as_str()) {
        cached_etag
    } else {
        None
    };

    let client = Client::new();
    let mut headers = HeaderMap::new();
    if let Some(etag) = effective_etag {
        headers.insert(IF_NONE_MATCH, etag.parse().unwrap());
    }

    let response = client
        .get(&instructions_url)
        .headers(headers)
        .send()
        .map_err(|e| format!("Failed to fetch instructions: {}", e))?;

    // Handle 304 Not Modified
    if response.status() == reqwest::StatusCode::NOT_MODIFIED && cache_file.exists() {
        return fs::read_to_string(cache_file).map_err(|e| e.to_string());
    }

    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }

    let new_etag = response
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let instructions = response.text().map_err(|e| e.to_string())?;

    // Save to cache
    if !cache_dir.exists() {
        fs::create_dir_all(cache_dir).map_err(|e| e.to_string())?;
    }

    fs::write(cache_file, &instructions).map_err(|e| e.to_string())?;

    let metadata = CacheMetadata {
        etag: new_etag,
        tag: latest_tag,
        last_checked: now_millis(),
        url: instructions_url,
    };
    let metadata_json = serde_json::to_string(&metadata).map_err(|e| e.to_string())?;
    fs::write(cache_meta_file, metadata_json).map_err(|e| e.to_string())?;

    Ok(instructions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_family_detection() {
        assert_eq!(
            ModelFamily::from_model("gpt-5.2-codex"),
            ModelFamily::Gpt52Codex
        );
        assert_eq!(
            ModelFamily::from_model("gpt-5.1-codex-max"),
            ModelFamily::CodexMax
        );
        assert_eq!(ModelFamily::from_model("gpt-5.1-codex"), ModelFamily::Codex);
        assert_eq!(ModelFamily::from_model("gpt-5.2"), ModelFamily::Gpt52);
        assert_eq!(ModelFamily::from_model("gpt-5.1"), ModelFamily::Gpt51);
        assert_eq!(ModelFamily::from_model("unknown-model"), ModelFamily::Gpt51);
    }
}
