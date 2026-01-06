use crate::coding_agent::auth_storage::AuthStorage;
use crate::core::messages::Cost;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub api: String,
    pub provider: String,
    pub base_url: String,
    pub reasoning: bool,
    pub input: Vec<String>,
    pub cost: Cost,
    pub context_window: i64,
    pub max_tokens: i64,
    pub headers: Option<HashMap<String, String>>,
}

#[derive(Clone, Debug, Default)]
struct ProviderOverride {
    base_url: Option<String>,
    headers: Option<HashMap<String, String>>,
}

#[derive(Clone, Debug, Default)]
struct CustomModelsResult {
    models: Vec<Model>,
    replaced_providers: HashSet<String>,
    overrides: HashMap<String, ProviderOverride>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelsConfig {
    providers: HashMap<String, ProviderConfig>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderConfig {
    base_url: Option<String>,
    api_key: Option<String>,
    api: Option<String>,
    headers: Option<HashMap<String, String>>,
    models: Option<Vec<ModelDefinition>>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelDefinition {
    id: String,
    name: Option<String>,
    api: Option<String>,
    reasoning: Option<bool>,
    input: Option<Vec<String>>,
    cost: Option<ModelCost>,
    context_window: Option<i64>,
    max_tokens: Option<i64>,
    headers: Option<HashMap<String, String>>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelCost {
    input: f64,
    output: f64,
    cache_read: f64,
    cache_write: f64,
}

pub struct ModelRegistry {
    auth_storage: AuthStorage,
    models_json_path: Option<PathBuf>,
    models: Vec<Model>,
    custom_provider_api_keys: HashMap<String, String>,
}

impl ModelRegistry {
    pub fn new(auth_storage: AuthStorage, models_json_path: impl Into<Option<PathBuf>>) -> Self {
        let mut registry = Self {
            auth_storage,
            models_json_path: models_json_path.into(),
            models: Vec::new(),
            custom_provider_api_keys: HashMap::new(),
        };
        registry.load_models();
        registry
    }

    pub fn refresh(&mut self) {
        self.custom_provider_api_keys.clear();
        self.load_models();
    }

    pub fn get_all(&self) -> Vec<Model> {
        self.models.clone()
    }

    pub fn get_available(&self) -> Vec<Model> {
        self.models
            .iter()
            .filter(|&model| self.auth_storage.has_auth(&model.provider))
            .cloned()
            .collect()
    }

    pub fn find(&self, provider: &str, model_id: &str) -> Option<Model> {
        self.models
            .iter()
            .find(|model| model.provider == provider && model.id == model_id)
            .cloned()
    }

    pub fn get_api_key(&self, model: &Model) -> Option<String> {
        self.auth_storage.get_api_key(&model.provider)
    }

    pub fn is_using_oauth(&self, model: &Model) -> bool {
        matches!(
            self.auth_storage.get(&model.provider),
            Some(crate::coding_agent::auth_storage::AuthCredential::OAuth { .. })
        )
    }

    fn load_models(&mut self) {
        let custom = if let Some(path) = self.models_json_path.clone() {
            self.load_custom_models(&path).unwrap_or_default()
        } else {
            CustomModelsResult::default()
        };

        let built_in = load_built_in_models(&custom.replaced_providers, &custom.overrides);
        self.models = built_in.into_iter().chain(custom.models).collect();

        let custom_keys = self.custom_provider_api_keys.clone();
        self.auth_storage
            .set_fallback_resolver(move |provider| custom_keys.get(provider).cloned());
    }

    fn load_custom_models(&mut self, path: &Path) -> Option<CustomModelsResult> {
        if !path.exists() {
            return None;
        }

        let contents = fs::read_to_string(path).ok()?;
        let parsed = serde_json::from_str::<ModelsConfig>(&contents).ok()?;

        let mut result = CustomModelsResult::default();
        for (provider, config) in parsed.providers {
            let models = config.models.clone().unwrap_or_default();
            if models.is_empty() {
                result.overrides.insert(
                    provider.clone(),
                    ProviderOverride {
                        base_url: config.base_url,
                        headers: config.headers,
                    },
                );
                if let Some(api_key) = config.api_key {
                    self.custom_provider_api_keys.insert(provider, api_key);
                }
                continue;
            }

            result.replaced_providers.insert(provider.clone());
            if let Some(api_key) = config.api_key.clone() {
                self.custom_provider_api_keys
                    .insert(provider.clone(), api_key);
            }
            for model in models {
                if let Some(parsed_model) = model_from_definition(&provider, &config, &model) {
                    result.models.push(parsed_model);
                }
            }
        }

        Some(result)
    }
}

fn model_from_definition(
    provider: &str,
    config: &ProviderConfig,
    definition: &ModelDefinition,
) -> Option<Model> {
    let api = definition
        .api
        .clone()
        .or_else(|| config.api.clone())
        .unwrap_or_else(|| "anthropic-messages".to_string());
    let name = definition
        .name
        .clone()
        .unwrap_or_else(|| definition.id.clone());
    let cost = definition.cost.as_ref().map_or_else(
        || Cost {
            input: 0.0,
            output: 0.0,
            cache_read: 0.0,
            cache_write: 0.0,
            total: 0.0,
        },
        |cost| Cost {
            input: cost.input,
            output: cost.output,
            cache_read: cost.cache_read,
            cache_write: cost.cache_write,
            total: cost.input + cost.output + cost.cache_read + cost.cache_write,
        },
    );
    let mut headers = merge_headers(config.headers.clone(), definition.headers.clone());
    if headers.as_ref().is_some_and(|h| h.is_empty()) {
        headers = None;
    }

    Some(Model {
        id: definition.id.clone(),
        name,
        api,
        provider: provider.to_string(),
        base_url: config.base_url.clone().unwrap_or_default(),
        reasoning: definition.reasoning.unwrap_or(false),
        input: definition
            .input
            .clone()
            .unwrap_or_else(|| vec!["text".to_string()]),
        cost,
        context_window: definition.context_window.unwrap_or(100_000),
        max_tokens: definition.max_tokens.unwrap_or(8_000),
        headers,
    })
}

fn merge_headers(
    base: Option<HashMap<String, String>>,
    extra: Option<HashMap<String, String>>,
) -> Option<HashMap<String, String>> {
    match (base, extra) {
        (None, None) => None,
        (Some(base), None) => Some(base),
        (None, Some(extra)) => Some(extra),
        (Some(mut base), Some(extra)) => {
            base.extend(extra);
            Some(base)
        }
    }
}

fn load_built_in_models(
    replaced: &HashSet<String>,
    overrides: &HashMap<String, ProviderOverride>,
) -> Vec<Model> {
    let mut models = Vec::new();

    let built_ins = vec![
        built_in_model(
            "anthropic",
            "claude-sonnet-4-5",
            "Claude Sonnet 4.5",
            "anthropic-messages",
            "https://api.anthropic.com/v1",
            true,
        ),
        built_in_model(
            "anthropic",
            "claude-3-5-haiku-20241022",
            "Claude 3.5 Haiku",
            "anthropic-messages",
            "https://api.anthropic.com/v1",
            false,
        ),
        built_in_model(
            "google",
            "gemini-2.5-flash",
            "Gemini 2.5 Flash",
            "google-generative-ai",
            "https://generativelanguage.googleapis.com",
            true,
        ),
        built_in_model(
            "openai",
            "gpt-4o-mini",
            "GPT-4o mini",
            "openai-responses",
            "https://api.openai.com/v1",
            false,
        ),
    ];

    for model in built_ins {
        if replaced.contains(&model.provider) {
            continue;
        }

        if let Some(override_cfg) = overrides.get(&model.provider) {
            let mut updated = model.clone();
            if let Some(base_url) = &override_cfg.base_url {
                updated.base_url = base_url.clone();
            }
            if let Some(headers) = &override_cfg.headers {
                updated.headers = merge_headers(updated.headers, Some(headers.clone()));
            }
            models.push(updated);
        } else {
            models.push(model);
        }
    }

    models
}

fn built_in_model(
    provider: &str,
    id: &str,
    name: &str,
    api: &str,
    base_url: &str,
    reasoning: bool,
) -> Model {
    Model {
        id: id.to_string(),
        name: name.to_string(),
        api: api.to_string(),
        provider: provider.to_string(),
        base_url: base_url.to_string(),
        reasoning,
        input: vec!["text".to_string(), "image".to_string()],
        cost: Cost {
            input: 0.0,
            output: 0.0,
            cache_read: 0.0,
            cache_write: 0.0,
            total: 0.0,
        },
        context_window: 100_000,
        max_tokens: 8_000,
        headers: None,
    }
}
