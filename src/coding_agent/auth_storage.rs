use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthCredential {
    ApiKey {
        key: String,
    },
    #[serde(rename = "oauth")]
    OAuth {
        access: String,
        #[serde(default)]
        refresh: Option<String>,
        #[serde(default)]
        expires: Option<i64>,
        #[serde(default, alias = "enterpriseUrl")]
        enterprise_url: Option<String>,
        #[serde(default, alias = "projectId")]
        project_id: Option<String>,
        #[serde(default, alias = "email")]
        email: Option<String>,
        #[serde(default, alias = "accountId")]
        account_id: Option<String>,
    },
}

type FallbackResolver = Box<dyn Fn(&str) -> Option<String>>;

static VERTEX_ADC_EXISTS: OnceLock<bool> = OnceLock::new();

pub struct AuthStorage {
    path: PathBuf,
    data: HashMap<String, AuthCredential>,
    runtime_overrides: HashMap<String, String>,
    fallback_resolver: Option<FallbackResolver>,
}

impl AuthStorage {
    pub fn new<P: Into<PathBuf>>(path: P) -> Self {
        let mut storage = Self {
            path: path.into(),
            data: HashMap::new(),
            runtime_overrides: HashMap::new(),
            fallback_resolver: None,
        };
        storage.reload();
        storage
    }

    pub fn set_runtime_api_key(&mut self, provider: &str, api_key: &str) {
        self.runtime_overrides
            .insert(provider.to_string(), api_key.to_string());
    }

    pub fn remove_runtime_api_key(&mut self, provider: &str) {
        self.runtime_overrides.remove(provider);
    }

    pub fn set_fallback_resolver(&mut self, resolver: impl Fn(&str) -> Option<String> + 'static) {
        self.fallback_resolver = Some(Box::new(resolver));
    }

    pub fn reload(&mut self) {
        let path = self.path.clone();
        if !path.exists() {
            self.data.clear();
            return;
        }
        match fs::read_to_string(path) {
            Ok(contents) => {
                let parsed = serde_json::from_str::<HashMap<String, AuthCredential>>(&contents);
                self.data = parsed.unwrap_or_default();
            }
            Err(_) => {
                self.data.clear();
            }
        }
    }

    pub fn get(&self, provider: &str) -> Option<&AuthCredential> {
        self.data.get(provider)
    }

    pub fn set(&mut self, provider: &str, credential: AuthCredential) {
        self.data.insert(provider.to_string(), credential);
        let _ = self.save();
    }

    pub fn remove(&mut self, provider: &str) {
        self.data.remove(provider);
        let _ = self.save();
    }

    pub fn list(&self) -> Vec<String> {
        self.data.keys().cloned().collect()
    }

    pub fn has(&self, provider: &str) -> bool {
        self.data.contains_key(provider)
    }

    pub fn has_auth(&self, provider: &str) -> bool {
        if self.runtime_overrides.contains_key(provider) {
            return true;
        }
        if self.data.contains_key(provider) {
            return true;
        }
        if env_api_key(provider).is_some() {
            return true;
        }
        if let Some(resolver) = &self.fallback_resolver {
            return resolver(provider).is_some();
        }
        false
    }

    pub fn get_api_key(&self, provider: &str) -> Option<String> {
        if let Some(key) = self.runtime_overrides.get(provider) {
            return Some(key.clone());
        }

        match self.data.get(provider) {
            Some(AuthCredential::ApiKey { key }) => Some(key.clone()),
            Some(AuthCredential::OAuth { access, .. }) => Some(access.clone()),
            None => {
                if let Some(env_key) = env_api_key(provider) {
                    return Some(env_key);
                }
                self.fallback_resolver
                    .as_ref()
                    .and_then(|resolver| resolver(provider))
            }
        }
    }

    fn save(&self) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_string_pretty(&self.data).unwrap_or_else(|_| "{}".to_string());
        fs::write(&self.path, data)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn env_api_key(provider: &str) -> Option<String> {
    if provider == "github-copilot" {
        return env_var_non_empty("COPILOT_GITHUB_TOKEN")
            .or_else(|| env_var_non_empty("GH_TOKEN"))
            .or_else(|| env_var_non_empty("GITHUB_TOKEN"));
    }

    if provider == "anthropic" {
        return env_var_non_empty("ANTHROPIC_OAUTH_TOKEN")
            .or_else(|| env_var_non_empty("ANTHROPIC_API_KEY"));
    }

    if provider == "google-vertex" && has_vertex_adc_credentials() {
        let has_project =
            env::var("GOOGLE_CLOUD_PROJECT").is_ok() || env::var("GCLOUD_PROJECT").is_ok();
        let has_location = env::var("GOOGLE_CLOUD_LOCATION").is_ok();
        if has_project && has_location {
            return Some("<authenticated>".to_string());
        }
    }

    let env_var = match provider {
        "openai" => "OPENAI_API_KEY",
        "google" => "GEMINI_API_KEY",
        "groq" => "GROQ_API_KEY",
        "cerebras" => "CEREBRAS_API_KEY",
        "xai" => "XAI_API_KEY",
        "openrouter" => "OPENROUTER_API_KEY",
        "zai" => "ZAI_API_KEY",
        "mistral" => "MISTRAL_API_KEY",
        _ => return None,
    };
    env_var_non_empty(env_var)
}

fn env_var_non_empty(key: &str) -> Option<String> {
    env::var(key).ok().and_then(|value| {
        if value.trim().is_empty() {
            None
        } else {
            Some(value)
        }
    })
}

fn has_vertex_adc_credentials() -> bool {
    *VERTEX_ADC_EXISTS.get_or_init(|| {
        let home_dir = env::var_os("HOME")
            .or_else(|| env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let path = home_dir
            .join(".config")
            .join("gcloud")
            .join("application_default_credentials.json");
        path.exists()
    })
}
