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

/// Gemini CLI credentials structure from ~/.gemini/oauth_creds.json
#[derive(serde::Deserialize)]
struct GeminiCliOAuthCreds {
    access_token: String,
    refresh_token: Option<String>,
    // expiry_date can be a float or int depending on how the gemini CLI wrote it
    #[serde(deserialize_with = "deserialize_expiry_date", default)]
    expiry_date: Option<i64>,
}

fn deserialize_expiry_date<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    let value: Option<serde_json::Value> = Option::deserialize(deserializer)?;
    match value {
        Some(serde_json::Value::Number(n)) => {
            // Try to get as i64 first, otherwise convert from f64
            if let Some(i) = n.as_i64() {
                Ok(Some(i))
            } else if let Some(f) = n.as_f64() {
                Ok(Some(f as i64))
            } else {
                Ok(None)
            }
        }
        _ => Ok(None),
    }
}

/// Resolve Google Gemini CLI credentials.
/// First checks auth.json, then ~/.gemini/oauth_creds.json (used by the official gemini CLI).
/// Returns (access_token, project_id) as a JSON string that the provider expects.
pub fn resolve_google_gemini_cli_credentials(
    api_key_override: Option<&str>,
) -> Result<(String, String), String> {
    if let Some(key) = api_key_override {
        // Assume it's a JSON with token and projectId
        if let Ok(parsed) = serde_json::from_str::<Value>(key) {
            if let (Some(token), Some(project)) = (
                parsed.get("token").and_then(Value::as_str),
                parsed.get("projectId").and_then(Value::as_str),
            ) {
                return Ok((token.to_string(), project.to_string()));
            }
        }
        return Err("Invalid google-gemini-cli credentials format. Expected JSON with 'token' and 'projectId'.".to_string());
    }

    // Check auth.json first (has project ID)
    if let Some(credential) = read_auth_credential("google-gemini-cli") {
        match credential {
            AuthCredential::OAuth {
                access, project_id, ..
            } => {
                if let Some(project) = project_id {
                    return Ok((access, project));
                }
            }
            AuthCredential::ApiKey { key: _ } => {
                // API key alone - need to discover project
                // For now, return error asking user to use /login
                return Err("google-gemini-cli requires OAuth with a project ID. \
                     Use /login to authenticate."
                    .to_string());
            }
        }
    }

    // Check google-antigravity too (same API, different endpoint)
    if let Some(AuthCredential::OAuth {
        access,
        project_id: Some(project),
        ..
    }) = read_auth_credential("google-antigravity")
    {
        return Ok((access, project));
    }

    // Try to load from ~/.gemini/oauth_creds.json (official gemini CLI)
    let home = env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    let gemini_creds_path = home.join(".gemini").join("oauth_creds.json");

    if gemini_creds_path.exists() {
        let content = std::fs::read_to_string(&gemini_creds_path)
            .map_err(|e| format!("Failed to read gemini CLI credentials: {e}"))?;

        let creds: GeminiCliOAuthCreds = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse gemini CLI credentials: {e}"))?;

        // Check if token is expired
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);

        if let Some(expiry) = creds.expiry_date {
            if expiry <= now_ms {
                // Token expired, try to refresh
                if let Some(refresh) = &creds.refresh_token {
                    // We need to discover project first, then refresh
                    // For simplicity, try to discover project with current token anyway
                    // (might work for slightly expired tokens)
                    match crate::api::google_gemini_cli::discover_gemini_project(
                        &creds.access_token,
                    ) {
                        Ok(project_id) => {
                            // Try refresh
                            match crate::api::google_gemini_cli::refresh_google_cloud_token(
                                refresh,
                                &project_id,
                            ) {
                                Ok(new_creds) => {
                                    // Update the gemini CLI creds file (optional, for next use)
                                    // For now just return the new credentials
                                    return Ok((new_creds.access, project_id));
                                }
                                Err(_) => {
                                    // Refresh failed, try with existing token anyway
                                }
                            }
                        }
                        Err(_) => {
                            // Can't discover project with expired token
                            return Err(
                                "google-gemini-cli token expired. Please run 'gemini' CLI to refresh, or use /login."
                                    .to_string(),
                            );
                        }
                    }
                }
            }
        }

        // Discover project ID using the access token
        let project_id =
            crate::api::google_gemini_cli::discover_gemini_project(&creds.access_token)
                .map_err(|e| format!("Failed to discover Gemini project: {e}"))?;

        return Ok((creds.access_token, project_id));
    }

    Err("Missing google-gemini-cli credentials. \
         Either run 'gemini' CLI to authenticate, or use /login in pi."
        .to_string())
}
