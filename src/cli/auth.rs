use crate::coding_agent::{AuthCredential, AuthStorage};
use crate::config;
use serde_json::Value;
use std::env;

pub fn env_var_non_empty(key: &str) -> Option<String> {
    env::var(key).ok().and_then(|value| {
        if value.trim().is_empty() {
            None
        } else {
            Some(value)
        }
    })
}

pub fn apply_env_api_keys_for_availability(auth_storage: &mut AuthStorage) {
    apply_env_key_if_missing(
        auth_storage,
        "anthropic",
        env_var_non_empty("ANTHROPIC_OAUTH_TOKEN"),
    );
    apply_env_key_if_missing(
        auth_storage,
        "anthropic",
        env_var_non_empty("ANTHROPIC_API_KEY"),
    );
    apply_env_key_if_missing(auth_storage, "openai", env_var_non_empty("OPENAI_API_KEY"));
    apply_env_key_if_missing(auth_storage, "google", env_var_non_empty("GEMINI_API_KEY"));
    apply_env_key_if_missing(auth_storage, "groq", env_var_non_empty("GROQ_API_KEY"));
    apply_env_key_if_missing(
        auth_storage,
        "cerebras",
        env_var_non_empty("CEREBRAS_API_KEY"),
    );
    apply_env_key_if_missing(auth_storage, "xai", env_var_non_empty("XAI_API_KEY"));
    apply_env_key_if_missing(
        auth_storage,
        "openrouter",
        env_var_non_empty("OPENROUTER_API_KEY"),
    );
    apply_env_key_if_missing(auth_storage, "zai", env_var_non_empty("ZAI_API_KEY"));
    apply_env_key_if_missing(
        auth_storage,
        "mistral",
        env_var_non_empty("MISTRAL_API_KEY"),
    );
    apply_env_key_if_missing(
        auth_storage,
        "github-copilot",
        env_var_non_empty("COPILOT_GITHUB_TOKEN")
            .or_else(|| env_var_non_empty("GH_TOKEN"))
            .or_else(|| env_var_non_empty("GITHUB_TOKEN")),
    );
}

fn read_auth_credential(provider: &str) -> Option<AuthCredential> {
    let path = config::get_auth_path();
    let content = std::fs::read_to_string(path).ok()?;
    let data: Value = serde_json::from_str(&content).ok()?;
    let entry = data.get(provider)?;
    serde_json::from_value(entry.clone()).ok()
}

fn apply_env_key_if_missing(auth_storage: &mut AuthStorage, provider: &str, key: Option<String>) {
    if auth_storage.has_auth(provider) {
        return;
    }
    if let Some(key) = key {
        auth_storage.set_runtime_api_key(provider, &key);
    }
}

pub fn resolve_anthropic_credentials(
    api_key_override: Option<&str>,
) -> Result<(String, bool), String> {
    if let Some(key) = api_key_override {
        return Ok((key.to_string(), false));
    }

    if let Some(credential) = read_auth_credential("anthropic") {
        match credential {
            AuthCredential::ApiKey { key } => return Ok((key, false)),
            AuthCredential::OAuth { access, .. } => return Ok((access, true)),
        }
    }

    if let Some(token) = env_var_non_empty("ANTHROPIC_OAUTH_TOKEN") {
        return Ok((token, true));
    }

    if let Some(key) = env_var_non_empty("ANTHROPIC_API_KEY") {
        return Ok((key, false));
    }

    Err(
        "Missing Anthropic credentials. Set ANTHROPIC_OAUTH_TOKEN or ANTHROPIC_API_KEY."
            .to_string(),
    )
}

pub fn resolve_openai_credentials(api_key_override: Option<&str>) -> Result<String, String> {
    if let Some(key) = api_key_override {
        return Ok(key.to_string());
    }

    if let Some(credential) = read_auth_credential("openai") {
        match credential {
            AuthCredential::ApiKey { key } => return Ok(key),
            AuthCredential::OAuth { access, .. } => return Ok(access),
        }
    }

    if let Some(key) = env_var_non_empty("OPENAI_API_KEY") {
        return Ok(key);
    }

    Err("Missing OpenAI credentials. Set OPENAI_API_KEY.".to_string())
}

pub fn resolve_openai_codex_credentials(api_key_override: Option<&str>) -> Result<String, String> {
    if let Some(key) = api_key_override {
        return Ok(key.to_string());
    }

    // OpenAI Codex uses a separate provider key in auth.json
    if let Some(credential) = read_auth_credential("openai-codex") {
        match credential {
            AuthCredential::ApiKey { key } => return Ok(key),
            AuthCredential::OAuth { access, .. } => return Ok(access),
        }
    }

    // Fall back to OPENAI_CODEX_API_KEY env var
    if let Some(key) = env_var_non_empty("OPENAI_CODEX_API_KEY") {
        return Ok(key);
    }

    Err("Missing OpenAI Codex credentials. Set OPENAI_CODEX_API_KEY or add openai-codex to auth.json.".to_string())
}
