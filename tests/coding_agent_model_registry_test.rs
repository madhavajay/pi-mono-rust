use pi::coding_agent::{AuthStorage, Model, ModelRegistry};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

// Source: packages/coding-agent/test/model-registry.test.ts

#[test]
fn overriding_baseurl_keeps_all_built_in_models() {
    let harness = TestHarness::new();
    write_models_json(
        &harness.models_json_path,
        json!({
            "anthropic": override_config("https://my-proxy.example.com/v1", None)
        }),
    );

    let registry = ModelRegistry::new(
        harness.auth_storage(),
        Some(harness.models_json_path.clone()),
    );
    let anthropic_models = get_models_for_provider(&registry, "anthropic");

    assert!(anthropic_models.len() > 1);
    assert!(anthropic_models.iter().any(|m| m.id.contains("claude")));
}

#[test]
fn overriding_baseurl_changes_url_on_all_built_in_models() {
    let harness = TestHarness::new();
    write_models_json(
        &harness.models_json_path,
        json!({
            "anthropic": override_config("https://my-proxy.example.com/v1", None)
        }),
    );

    let registry = ModelRegistry::new(
        harness.auth_storage(),
        Some(harness.models_json_path.clone()),
    );
    let anthropic_models = get_models_for_provider(&registry, "anthropic");
    assert!(!anthropic_models.is_empty());

    for model in anthropic_models {
        assert_eq!(model.base_url, "https://my-proxy.example.com/v1");
    }
}

#[test]
fn overriding_headers_merges_with_model_headers() {
    let harness = TestHarness::new();
    write_models_json(
        &harness.models_json_path,
        json!({
            "anthropic": override_config(
                "https://my-proxy.example.com/v1",
                Some(json!({ "X-Custom-Header": "custom-value" }))
            )
        }),
    );

    let registry = ModelRegistry::new(
        harness.auth_storage(),
        Some(harness.models_json_path.clone()),
    );
    let anthropic_models = get_models_for_provider(&registry, "anthropic");
    assert!(!anthropic_models.is_empty());

    for model in anthropic_models {
        let headers = model.headers.expect("expected headers");
        assert_eq!(
            headers.get("X-Custom-Header"),
            Some(&"custom-value".to_string())
        );
    }
}

#[test]
fn baseurl_only_override_does_not_affect_other_providers() {
    let harness = TestHarness::new();
    write_models_json(
        &harness.models_json_path,
        json!({
            "anthropic": override_config("https://my-proxy.example.com/v1", None)
        }),
    );

    let registry = ModelRegistry::new(
        harness.auth_storage(),
        Some(harness.models_json_path.clone()),
    );
    let google_models = get_models_for_provider(&registry, "google");
    assert!(!google_models.is_empty());
    assert_ne!(google_models[0].base_url, "https://my-proxy.example.com/v1");
}

#[test]
fn can_mix_baseurl_override_and_full_replacement() {
    let harness = TestHarness::new();
    write_models_json(
        &harness.models_json_path,
        json!({
            "anthropic": override_config("https://anthropic-proxy.example.com/v1", None),
            "google": provider_config("https://google-proxy.example.com/v1", vec!["gemini-custom"], "google-generative-ai")
        }),
    );

    let registry = ModelRegistry::new(
        harness.auth_storage(),
        Some(harness.models_json_path.clone()),
    );

    let anthropic_models = get_models_for_provider(&registry, "anthropic");
    assert!(anthropic_models.len() > 1);
    assert_eq!(
        anthropic_models[0].base_url,
        "https://anthropic-proxy.example.com/v1"
    );

    let google_models = get_models_for_provider(&registry, "google");
    assert_eq!(google_models.len(), 1);
    assert_eq!(google_models[0].id, "gemini-custom");
}

#[test]
fn refresh_picks_up_baseurl_override_changes() {
    let harness = TestHarness::new();
    write_models_json(
        &harness.models_json_path,
        json!({
            "anthropic": override_config("https://first-proxy.example.com/v1", None)
        }),
    );

    let mut registry = ModelRegistry::new(
        harness.auth_storage(),
        Some(harness.models_json_path.clone()),
    );
    let first_url = get_models_for_provider(&registry, "anthropic")[0]
        .base_url
        .clone();
    assert_eq!(first_url, "https://first-proxy.example.com/v1");

    write_models_json(
        &harness.models_json_path,
        json!({
            "anthropic": override_config("https://second-proxy.example.com/v1", None)
        }),
    );
    registry.refresh();

    let second_url = get_models_for_provider(&registry, "anthropic")[0]
        .base_url
        .clone();
    assert_eq!(second_url, "https://second-proxy.example.com/v1");
}

#[test]
fn custom_provider_with_same_name_as_built_in_replaces_built_in_models() {
    let harness = TestHarness::new();
    write_models_json(
        &harness.models_json_path,
        json!({
            "anthropic": provider_config("https://my-proxy.example.com/v1", vec!["claude-custom"], "anthropic-messages")
        }),
    );

    let registry = ModelRegistry::new(
        harness.auth_storage(),
        Some(harness.models_json_path.clone()),
    );
    let anthropic_models = get_models_for_provider(&registry, "anthropic");
    assert_eq!(anthropic_models.len(), 1);
    assert_eq!(anthropic_models[0].id, "claude-custom");
    assert_eq!(
        anthropic_models[0].base_url,
        "https://my-proxy.example.com/v1"
    );
}

#[test]
fn custom_provider_with_same_name_as_built_in_does_not_affect_other_built_in_providers() {
    let harness = TestHarness::new();
    write_models_json(
        &harness.models_json_path,
        json!({
            "anthropic": provider_config("https://my-proxy.example.com/v1", vec!["claude-custom"], "anthropic-messages")
        }),
    );

    let registry = ModelRegistry::new(
        harness.auth_storage(),
        Some(harness.models_json_path.clone()),
    );
    assert!(!get_models_for_provider(&registry, "google").is_empty());
    assert!(!get_models_for_provider(&registry, "openai").is_empty());
}

#[test]
fn multiple_built_in_providers_can_be_overridden() {
    let harness = TestHarness::new();
    write_models_json(
        &harness.models_json_path,
        json!({
            "anthropic": provider_config("https://anthropic-proxy.example.com/v1", vec!["claude-proxy"], "anthropic-messages"),
            "google": provider_config("https://google-proxy.example.com/v1", vec!["gemini-proxy"], "google-generative-ai")
        }),
    );

    let registry = ModelRegistry::new(
        harness.auth_storage(),
        Some(harness.models_json_path.clone()),
    );
    let anthropic_models = get_models_for_provider(&registry, "anthropic");
    let google_models = get_models_for_provider(&registry, "google");

    assert_eq!(anthropic_models.len(), 1);
    assert_eq!(anthropic_models[0].id, "claude-proxy");
    assert_eq!(
        anthropic_models[0].base_url,
        "https://anthropic-proxy.example.com/v1"
    );

    assert_eq!(google_models.len(), 1);
    assert_eq!(google_models[0].id, "gemini-proxy");
    assert_eq!(
        google_models[0].base_url,
        "https://google-proxy.example.com/v1"
    );
}

#[test]
fn refresh_reloads_overrides_from_disk() {
    let harness = TestHarness::new();
    write_models_json(
        &harness.models_json_path,
        json!({
            "anthropic": provider_config("https://first-proxy.example.com/v1", vec!["claude-first"], "anthropic-messages")
        }),
    );
    let mut registry = ModelRegistry::new(
        harness.auth_storage(),
        Some(harness.models_json_path.clone()),
    );
    assert_eq!(
        get_models_for_provider(&registry, "anthropic")[0].id,
        "claude-first"
    );

    write_models_json(
        &harness.models_json_path,
        json!({
            "anthropic": provider_config("https://second-proxy.example.com/v1", vec!["claude-second"], "anthropic-messages")
        }),
    );
    registry.refresh();

    let anthropic_models = get_models_for_provider(&registry, "anthropic");
    assert_eq!(anthropic_models[0].id, "claude-second");
    assert_eq!(
        anthropic_models[0].base_url,
        "https://second-proxy.example.com/v1"
    );
}

#[test]
fn removing_override_from_models_json_restores_built_in_provider() {
    let harness = TestHarness::new();
    write_models_json(
        &harness.models_json_path,
        json!({
            "anthropic": provider_config("https://proxy.example.com/v1", vec!["claude-custom"], "anthropic-messages")
        }),
    );
    let mut registry = ModelRegistry::new(
        harness.auth_storage(),
        Some(harness.models_json_path.clone()),
    );
    assert_eq!(get_models_for_provider(&registry, "anthropic").len(), 1);

    write_models_json(&harness.models_json_path, json!({}));
    registry.refresh();

    let anthropic_models = get_models_for_provider(&registry, "anthropic");
    assert!(anthropic_models.len() > 1);
    assert!(anthropic_models.iter().any(|m| m.id.contains("claude")));
}

struct TestHarness {
    temp_dir: PathBuf,
    models_json_path: PathBuf,
    auth_path: PathBuf,
}

impl TestHarness {
    fn new() -> Self {
        let temp_dir = make_temp_dir();
        let models_json_path = temp_dir.join("models.json");
        let auth_path = temp_dir.join("auth.json");
        Self {
            temp_dir,
            models_json_path,
            auth_path,
        }
    }

    fn auth_storage(&self) -> AuthStorage {
        AuthStorage::new(self.auth_path.clone())
    }
}

impl Drop for TestHarness {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.temp_dir);
    }
}

fn make_temp_dir() -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("pi-test-model-registry-{now}-{count}"));
    let _ = fs::create_dir_all(&dir);
    dir
}

fn provider_config(base_url: &str, models: Vec<&str>, api: &str) -> serde_json::Value {
    json!({
        "baseUrl": base_url,
        "apiKey": "TEST_KEY",
        "api": api,
        "models": models.into_iter().map(|id| json!({
            "id": id,
            "name": id,
            "reasoning": false,
            "input": ["text"],
            "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0 },
            "contextWindow": 100000,
            "maxTokens": 8000
        })).collect::<Vec<_>>()
    })
}

fn override_config(base_url: &str, headers: Option<serde_json::Value>) -> serde_json::Value {
    let mut value = json!({ "baseUrl": base_url });
    if let Some(headers_value) = headers {
        if let Some(obj) = value.as_object_mut() {
            obj.insert("headers".to_string(), headers_value);
        }
    }
    value
}

fn write_models_json(path: &Path, providers: serde_json::Value) {
    let content = json!({ "providers": providers });
    let _ = fs::write(path, serde_json::to_string(&content).unwrap());
}

fn get_models_for_provider(registry: &ModelRegistry, provider: &str) -> Vec<Model> {
    registry
        .get_all()
        .into_iter()
        .filter(|model| model.provider == provider)
        .collect()
}
