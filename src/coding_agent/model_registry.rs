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

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BuiltInModelCost {
    input: f64,
    output: f64,
    cache_read: f64,
    cache_write: f64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BuiltInModelDefinition {
    id: String,
    name: String,
    api: String,
    provider: String,
    base_url: String,
    reasoning: bool,
    input: Vec<String>,
    cost: BuiltInModelCost,
    context_window: i64,
    max_tokens: i64,
    headers: Option<HashMap<String, String>>,
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

    /// Get credential for a provider
    pub fn get_credential(
        &self,
        provider: &str,
    ) -> Option<&crate::coding_agent::auth_storage::AuthCredential> {
        self.auth_storage.get(provider)
    }

    /// Set credential for a provider
    pub fn set_credential(
        &mut self,
        provider: &str,
        credential: crate::coding_agent::auth_storage::AuthCredential,
    ) {
        self.auth_storage.set(provider, credential);
    }

    /// Remove credential for a provider
    pub fn remove_credential(&mut self, provider: &str) {
        self.auth_storage.remove(provider);
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

    let built_ins = load_built_in_models_from_json();

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

fn load_built_in_models_from_json() -> Vec<Model> {
    let content = include_str!("../assets/models.generated.json");
    let parsed =
        serde_json::from_str::<HashMap<String, HashMap<String, BuiltInModelDefinition>>>(content);
    let mut models = Vec::new();

    let Ok(parsed) = parsed else {
        eprintln!("Warning: Failed to parse built-in models JSON.");
        return models;
    };

    for models_by_provider in parsed.values() {
        for model in models_by_provider.values() {
            models.push(model_from_builtin(model));
        }
    }

    models
}

fn model_from_builtin(model: &BuiltInModelDefinition) -> Model {
    let base_url = normalize_base_url(&model.api, &model.base_url);
    let mut headers = model.headers.clone();
    if headers.as_ref().is_some_and(|value| value.is_empty()) {
        headers = None;
    }
    let cost = Cost {
        input: model.cost.input,
        output: model.cost.output,
        cache_read: model.cost.cache_read,
        cache_write: model.cost.cache_write,
        total: model.cost.input
            + model.cost.output
            + model.cost.cache_read
            + model.cost.cache_write,
    };
    Model {
        id: model.id.clone(),
        name: model.name.clone(),
        api: model.api.clone(),
        provider: model.provider.clone(),
        base_url,
        reasoning: model.reasoning,
        input: model.input.clone(),
        cost,
        context_window: model.context_window,
        max_tokens: model.max_tokens,
        headers,
    }
}

fn normalize_base_url(api: &str, base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    if api == "anthropic-messages" && !trimmed.ends_with("/v1") {
        format!("{trimmed}/v1")
    } else {
        trimmed.to_string()
    }
}
