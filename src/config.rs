use serde_json::Value;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppConfig {
    pub app_name: String,
    pub config_dir_name: String,
    pub env_agent_dir: String,
}

impl AppConfig {
    fn new(app_name: String, config_dir_name: String) -> Self {
        let app_name = normalize_value(app_name, "pi");
        let config_dir_name = normalize_value(config_dir_name, ".pi");
        let env_agent_dir = format!("{}_CODING_AGENT_DIR", app_name.to_uppercase());
        Self {
            app_name,
            config_dir_name,
            env_agent_dir,
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self::new("pi".to_string(), ".pi".to_string())
    }
}

static APP_CONFIG: OnceLock<AppConfig> = OnceLock::new();

pub fn app_config() -> &'static AppConfig {
    APP_CONFIG.get_or_init(load_app_config)
}

pub fn app_name() -> &'static str {
    &app_config().app_name
}

pub fn config_dir_name() -> &'static str {
    &app_config().config_dir_name
}

pub fn env_agent_dir_name() -> &'static str {
    &app_config().env_agent_dir
}

pub fn get_agent_dir() -> PathBuf {
    if let Ok(dir) = env::var(env_agent_dir_name()) {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }
    home_dir().join(config_dir_name()).join("agent")
}

pub fn get_auth_path() -> PathBuf {
    get_agent_dir().join("auth.json")
}

pub fn get_models_path() -> PathBuf {
    get_agent_dir().join("models.json")
}

pub fn get_settings_path() -> PathBuf {
    get_agent_dir().join("settings.json")
}

pub fn app_config_from_package_json(path: &Path) -> Option<AppConfig> {
    let content = fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&content).ok()?;
    let pi_config = value.get("piConfig").and_then(Value::as_object);
    let app_name = pi_config
        .and_then(|cfg| cfg.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("pi")
        .to_string();
    let config_dir_name = pi_config
        .and_then(|cfg| cfg.get("configDir"))
        .and_then(Value::as_str)
        .unwrap_or(".pi")
        .to_string();
    Some(AppConfig::new(app_name, config_dir_name))
}

fn load_app_config() -> AppConfig {
    let mut fallback = None;
    for start in search_roots() {
        let mut current = Some(start.as_path());
        while let Some(dir) = current {
            let candidate = dir.join("package.json");
            if candidate.exists() {
                if let Some(config) = app_config_from_package_json(&candidate) {
                    if package_json_has_pi_config(&candidate) {
                        return config;
                    }
                    fallback.get_or_insert(config);
                }
            }
            current = dir.parent();
        }
    }
    fallback.unwrap_or_default()
}

fn package_json_has_pi_config(path: &Path) -> bool {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(_) => return false,
    };
    let value: Value = match serde_json::from_str(&content) {
        Ok(value) => value,
        Err(_) => return false,
    };
    value.get("piConfig").is_some()
}

fn search_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(exe) = env::current_exe() {
        if let Some(parent) = exe.parent() {
            roots.push(parent.to_path_buf());
        }
    }
    if let Ok(cwd) = env::current_dir() {
        if roots.last().map(|path| path != &cwd).unwrap_or(true) {
            roots.push(cwd);
        }
    }
    roots
}

fn normalize_value(value: String, default_value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        default_value.to_string()
    } else {
        trimmed.to_string()
    }
}

fn home_dir() -> PathBuf {
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}
