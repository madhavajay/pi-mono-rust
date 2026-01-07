//! Request transformation for OpenAI Codex API
//!
//! Handles model normalization, reasoning configuration, and input filtering.

use super::prompts::{CODEX_PI_BRIDGE, TOOL_REMAP_MESSAGE};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

/// Reasoning configuration for Codex models
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningConfig {
    pub effort: ReasoningEffort,
    pub summary: ReasoningSummary,
}

/// Reasoning effort level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    None,
    Minimal,
    Low,
    Medium,
    High,
    #[serde(rename = "xhigh")]
    XHigh,
}

impl ReasoningEffort {
    pub fn as_str(&self) -> &'static str {
        match self {
            ReasoningEffort::None => "none",
            ReasoningEffort::Minimal => "minimal",
            ReasoningEffort::Low => "low",
            ReasoningEffort::Medium => "medium",
            ReasoningEffort::High => "high",
            ReasoningEffort::XHigh => "xhigh",
        }
    }
}

/// Reasoning summary level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningSummary {
    Auto,
    Concise,
    Detailed,
    Off,
    On,
}

/// Options for Codex request transformation
#[derive(Debug, Clone, Default)]
pub struct CodexRequestOptions {
    pub reasoning_effort: Option<ReasoningEffort>,
    pub reasoning_summary: Option<ReasoningSummary>,
    pub text_verbosity: Option<TextVerbosity>,
    pub include: Option<Vec<String>>,
}

/// Text verbosity level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum TextVerbosity {
    Low,
    #[default]
    Medium,
    High,
}

/// Input item for the Codex API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexInputItem {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub item_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
}

/// Request body for the Codex API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexRequestBody {
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_key: Option<String>,
    // These are removed during transformation
    #[serde(skip_serializing)]
    pub max_output_tokens: Option<u32>,
    #[serde(skip_serializing)]
    pub max_completion_tokens: Option<u32>,
}

/// Model name mapping for normalization
fn get_model_map() -> HashMap<&'static str, &'static str> {
    let mut map = HashMap::new();

    // GPT-5.1 Codex variants
    map.insert("gpt-5.1-codex", "gpt-5.1-codex");
    map.insert("gpt-5.1-codex-low", "gpt-5.1-codex");
    map.insert("gpt-5.1-codex-medium", "gpt-5.1-codex");
    map.insert("gpt-5.1-codex-high", "gpt-5.1-codex");

    // GPT-5.1 Codex Max variants
    map.insert("gpt-5.1-codex-max", "gpt-5.1-codex-max");
    map.insert("gpt-5.1-codex-max-low", "gpt-5.1-codex-max");
    map.insert("gpt-5.1-codex-max-medium", "gpt-5.1-codex-max");
    map.insert("gpt-5.1-codex-max-high", "gpt-5.1-codex-max");
    map.insert("gpt-5.1-codex-max-xhigh", "gpt-5.1-codex-max");

    // GPT-5.2 variants
    map.insert("gpt-5.2", "gpt-5.2");
    map.insert("gpt-5.2-none", "gpt-5.2");
    map.insert("gpt-5.2-low", "gpt-5.2");
    map.insert("gpt-5.2-medium", "gpt-5.2");
    map.insert("gpt-5.2-high", "gpt-5.2");
    map.insert("gpt-5.2-xhigh", "gpt-5.2");

    // GPT-5.2 Codex variants
    map.insert("gpt-5.2-codex", "gpt-5.2-codex");
    map.insert("gpt-5.2-codex-low", "gpt-5.2-codex");
    map.insert("gpt-5.2-codex-medium", "gpt-5.2-codex");
    map.insert("gpt-5.2-codex-high", "gpt-5.2-codex");
    map.insert("gpt-5.2-codex-xhigh", "gpt-5.2-codex");

    // GPT-5.1 Codex Mini variants
    map.insert("gpt-5.1-codex-mini", "gpt-5.1-codex-mini");
    map.insert("gpt-5.1-codex-mini-medium", "gpt-5.1-codex-mini");
    map.insert("gpt-5.1-codex-mini-high", "gpt-5.1-codex-mini");

    // GPT-5.1 variants
    map.insert("gpt-5.1", "gpt-5.1");
    map.insert("gpt-5.1-none", "gpt-5.1");
    map.insert("gpt-5.1-low", "gpt-5.1");
    map.insert("gpt-5.1-medium", "gpt-5.1");
    map.insert("gpt-5.1-high", "gpt-5.1");
    map.insert("gpt-5.1-chat-latest", "gpt-5.1");

    // Legacy aliases
    map.insert("gpt-5-codex", "gpt-5.1-codex");
    map.insert("codex-mini-latest", "gpt-5.1-codex-mini");
    map.insert("gpt-5-codex-mini", "gpt-5.1-codex-mini");
    map.insert("gpt-5-codex-mini-medium", "gpt-5.1-codex-mini");
    map.insert("gpt-5-codex-mini-high", "gpt-5.1-codex-mini");
    map.insert("gpt-5", "gpt-5.1");
    map.insert("gpt-5-mini", "gpt-5.1");
    map.insert("gpt-5-nano", "gpt-5.1");

    map
}

/// Normalize a model name to its canonical form
pub fn normalize_model(model: Option<&str>) -> String {
    let model = match model {
        Some(m) if !m.is_empty() => m,
        _ => return "gpt-5.1".to_string(),
    };

    // Handle provider prefix (e.g., "openai/gpt-5.1-codex")
    let model_id = model.split('/').next_back().unwrap_or(model);

    let model_map = get_model_map();

    // Try exact match
    if let Some(&normalized) = model_map.get(model_id) {
        return normalized.to_string();
    }

    // Try case-insensitive match
    let lower = model_id.to_lowercase();
    for (key, value) in &model_map {
        if key.to_lowercase() == lower {
            return value.to_string();
        }
    }

    // Fuzzy matching
    if lower.contains("gpt-5.2-codex") || lower.contains("gpt 5.2 codex") {
        return "gpt-5.2-codex".to_string();
    }
    if lower.contains("gpt-5.2") || lower.contains("gpt 5.2") {
        return "gpt-5.2".to_string();
    }
    if lower.contains("gpt-5.1-codex-max") || lower.contains("gpt 5.1 codex max") {
        return "gpt-5.1-codex-max".to_string();
    }
    if lower.contains("gpt-5.1-codex-mini") || lower.contains("gpt 5.1 codex mini") {
        return "gpt-5.1-codex-mini".to_string();
    }
    if lower.contains("codex-mini-latest")
        || lower.contains("gpt-5-codex-mini")
        || lower.contains("gpt 5 codex mini")
    {
        return "codex-mini-latest".to_string();
    }
    if lower.contains("gpt-5.1-codex") || lower.contains("gpt 5.1 codex") {
        return "gpt-5.1-codex".to_string();
    }
    if lower.contains("gpt-5.1") || lower.contains("gpt 5.1") {
        return "gpt-5.1".to_string();
    }
    if lower.contains("codex") {
        return "gpt-5.1-codex".to_string();
    }
    if lower.contains("gpt-5") || lower.contains("gpt 5") {
        return "gpt-5.1".to_string();
    }

    "gpt-5.1".to_string()
}

/// Get the reasoning configuration for a model
pub fn get_reasoning_config(model_name: &str, options: &CodexRequestOptions) -> ReasoningConfig {
    let normalized = model_name.to_lowercase();

    let is_gpt52_codex =
        normalized.contains("gpt-5.2-codex") || normalized.contains("gpt 5.2 codex");
    let is_gpt52_general =
        (normalized.contains("gpt-5.2") || normalized.contains("gpt 5.2")) && !is_gpt52_codex;
    let is_codex_max = normalized.contains("codex-max") || normalized.contains("codex max");
    let is_codex_mini = normalized.contains("codex-mini")
        || normalized.contains("codex mini")
        || normalized.contains("codex_mini")
        || normalized.contains("codex-mini-latest");
    let is_codex = normalized.contains("codex") && !is_codex_mini;
    let is_lightweight =
        !is_codex_mini && (normalized.contains("nano") || normalized.contains("mini"));
    let is_gpt51_general = (normalized.contains("gpt-5.1") || normalized.contains("gpt 5.1"))
        && !is_codex
        && !is_codex_max
        && !is_codex_mini;

    let supports_xhigh = is_gpt52_general || is_gpt52_codex || is_codex_max;
    let supports_none = is_gpt52_general || is_gpt51_general;

    // Determine default effort
    let default_effort = if is_codex_mini {
        ReasoningEffort::Medium
    } else if supports_xhigh {
        ReasoningEffort::High
    } else if is_lightweight {
        ReasoningEffort::Minimal
    } else {
        ReasoningEffort::Medium
    };

    let mut effort = options.reasoning_effort.unwrap_or(default_effort);

    // Clamp effort for codex-mini
    if is_codex_mini {
        effort = match effort {
            ReasoningEffort::None | ReasoningEffort::Minimal | ReasoningEffort::Low => {
                ReasoningEffort::Medium
            }
            ReasoningEffort::XHigh => ReasoningEffort::High,
            other => other,
        };
        // Ensure it's medium or high
        if effort != ReasoningEffort::High && effort != ReasoningEffort::Medium {
            effort = ReasoningEffort::Medium;
        }
    }

    // Clamp xhigh for models that don't support it
    if !supports_xhigh && effort == ReasoningEffort::XHigh {
        effort = ReasoningEffort::High;
    }

    // Clamp none for models that don't support it
    if !supports_none && effort == ReasoningEffort::None {
        effort = ReasoningEffort::Low;
    }

    // Codex doesn't support minimal
    if is_codex && effort == ReasoningEffort::Minimal {
        effort = ReasoningEffort::Low;
    }

    ReasoningConfig {
        effort,
        summary: options.reasoning_summary.unwrap_or(ReasoningSummary::Auto),
    }
}

/// Filter input items to remove item_reference and strip IDs
pub fn filter_input(input: &[Value]) -> Vec<Value> {
    input
        .iter()
        .filter(|item| {
            item.get("type")
                .and_then(|t| t.as_str())
                .map(|t| t != "item_reference")
                .unwrap_or(true)
        })
        .map(|item| {
            let mut item = item.clone();
            if let Value::Object(ref mut map) = item {
                map.remove("id");
            }
            item
        })
        .collect()
}

/// Add the Codex-Pi bridge message to the input
pub fn add_codex_bridge_message(
    input: &[Value],
    has_tools: bool,
    system_prompt: Option<&str>,
) -> Vec<Value> {
    if !has_tools {
        return input.to_vec();
    }

    let bridge_text = match system_prompt {
        Some(prompt) if !prompt.is_empty() => format!("{}\n\n{}", CODEX_PI_BRIDGE, prompt),
        _ => CODEX_PI_BRIDGE.to_string(),
    };

    let bridge_message = json!({
        "type": "message",
        "role": "developer",
        "content": [{
            "type": "input_text",
            "text": bridge_text
        }]
    });

    let mut result = vec![bridge_message];
    result.extend_from_slice(input);
    result
}

/// Add the tool remap message to the input (for non-codex mode)
pub fn add_tool_remap_message(input: &[Value], has_tools: bool) -> Vec<Value> {
    if !has_tools {
        return input.to_vec();
    }

    let tool_remap_message = json!({
        "type": "message",
        "role": "developer",
        "content": [{
            "type": "input_text",
            "text": TOOL_REMAP_MESSAGE
        }]
    });

    let mut result = vec![tool_remap_message];
    result.extend_from_slice(input);
    result
}

/// Handle orphaned function call outputs (outputs without matching call_id)
pub fn handle_orphaned_outputs(input: &mut [Value]) {
    // First, collect all function call IDs
    let function_call_ids: HashSet<String> = input
        .iter()
        .filter_map(|item| {
            let item_type = item.get("type").and_then(|t| t.as_str())?;
            if item_type == "function_call" {
                item.get("call_id")
                    .and_then(|c| c.as_str())
                    .map(String::from)
            } else {
                None
            }
        })
        .collect();

    // Transform orphaned function_call_output to assistant messages
    for item in input.iter_mut() {
        let item_type = item.get("type").and_then(|t| t.as_str());
        if item_type != Some("function_call_output") {
            continue;
        }

        let call_id = match item.get("call_id").and_then(|c| c.as_str()) {
            Some(id) => id.to_string(),
            None => continue,
        };

        if function_call_ids.contains(&call_id) {
            continue;
        }

        // This is an orphaned output - convert to assistant message
        let tool_name = item.get("name").and_then(|n| n.as_str()).unwrap_or("tool");

        let output = item.get("output");
        let mut text = match output {
            Some(Value::String(s)) => s.clone(),
            Some(v) => serde_json::to_string(v).unwrap_or_else(|_| v.to_string()),
            None => String::new(),
        };

        // Truncate long outputs
        if text.len() > 16000 {
            text.truncate(16000);
            text.push_str("\n...[truncated]");
        }

        let content = format!(
            "[Previous {} result; call_id={}]: {}",
            tool_name, call_id, text
        );

        *item = json!({
            "type": "message",
            "role": "assistant",
            "content": content
        });
    }
}

/// Transform the request body for the Codex API
pub fn transform_request_body(
    body: &mut CodexRequestBody,
    codex_instructions: &str,
    options: &CodexRequestOptions,
    codex_mode: bool,
    system_prompt: Option<&str>,
) {
    // Normalize model
    let normalized_model = normalize_model(Some(&body.model));
    body.model = normalized_model.clone();

    // Set fixed fields
    body.store = Some(false);
    body.stream = Some(true);
    body.instructions = Some(codex_instructions.to_string());

    // Process input
    if let Some(ref mut input) = body.input {
        // Filter out item_reference and strip IDs
        *input = filter_input(input);

        let has_tools = body.tools.as_ref().is_some_and(|t| !t.is_empty());

        // Add bridge or remap message
        if codex_mode {
            *input = add_codex_bridge_message(input, has_tools, system_prompt);
        } else {
            *input = add_tool_remap_message(input, has_tools);
        }

        // Handle orphaned outputs
        handle_orphaned_outputs(input);
    }

    // Set reasoning config if specified
    if options.reasoning_effort.is_some() {
        let reasoning_config = get_reasoning_config(&normalized_model, options);
        body.reasoning = Some(json!({
            "effort": reasoning_config.effort.as_str(),
            "summary": match reasoning_config.summary {
                ReasoningSummary::Auto => "auto",
                ReasoningSummary::Concise => "concise",
                ReasoningSummary::Detailed => "detailed",
                ReasoningSummary::Off => "off",
                ReasoningSummary::On => "on",
            }
        }));
    } else {
        body.reasoning = None;
    }

    // Set text verbosity
    let verbosity = options.text_verbosity.unwrap_or(TextVerbosity::Medium);
    body.text = Some(json!({
        "verbosity": match verbosity {
            TextVerbosity::Low => "low",
            TextVerbosity::Medium => "medium",
            TextVerbosity::High => "high",
        }
    }));

    // Set include array with reasoning.encrypted_content
    let mut include: Vec<String> = options.include.clone().unwrap_or_default();
    let encrypted_content = "reasoning.encrypted_content".to_string();
    if !include.contains(&encrypted_content) {
        include.push(encrypted_content);
    }
    // Deduplicate
    let unique: Vec<String> = include
        .into_iter()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    body.include = Some(unique);

    // Remove max tokens fields (not supported by Codex API)
    body.max_output_tokens = None;
    body.max_completion_tokens = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_model() {
        assert_eq!(normalize_model(Some("gpt-5.1-codex")), "gpt-5.1-codex");
        assert_eq!(
            normalize_model(Some("gpt 5 codex mini")),
            "codex-mini-latest"
        );
        assert_eq!(normalize_model(Some("gpt-5.2-codex")), "gpt-5.2-codex");
        assert_eq!(normalize_model(None), "gpt-5.1");
        assert_eq!(normalize_model(Some("")), "gpt-5.1");
        assert_eq!(
            normalize_model(Some("openai/gpt-5.1-codex")),
            "gpt-5.1-codex"
        );
    }

    #[test]
    fn test_reasoning_config_codex_mini() {
        let options = CodexRequestOptions {
            reasoning_effort: Some(ReasoningEffort::XHigh),
            ..Default::default()
        };
        let config = get_reasoning_config("gpt-5.1-codex-mini", &options);
        assert_eq!(config.effort, ReasoningEffort::High); // Clamped from xhigh
    }

    #[test]
    fn test_reasoning_config_gpt52() {
        let options = CodexRequestOptions {
            reasoning_effort: Some(ReasoningEffort::XHigh),
            ..Default::default()
        };
        let config = get_reasoning_config("gpt-5.2-codex", &options);
        assert_eq!(config.effort, ReasoningEffort::XHigh); // Supports xhigh
    }

    #[test]
    fn test_filter_input() {
        let input = vec![
            json!({"type": "message", "id": "1", "content": "hello"}),
            json!({"type": "item_reference", "id": "ref-1"}),
            json!({"type": "function_call", "id": "2", "call_id": "call-1"}),
        ];

        let filtered = filter_input(&input);
        assert_eq!(filtered.len(), 2);
        assert!(filtered[0].get("id").is_none());
        assert!(filtered[1].get("id").is_none());
    }

    #[test]
    fn test_transform_adds_encrypted_content() {
        let mut body = CodexRequestBody {
            model: "gpt-5.1-codex".to_string(),
            store: None,
            stream: None,
            instructions: None,
            input: None,
            tools: None,
            temperature: None,
            reasoning: None,
            text: None,
            include: None,
            prompt_cache_key: None,
            max_output_tokens: None,
            max_completion_tokens: None,
        };

        transform_request_body(
            &mut body,
            "CODEX_INSTRUCTIONS",
            &CodexRequestOptions {
                include: Some(vec!["foo".to_string()]),
                ..Default::default()
            },
            true,
            None,
        );

        let include = body.include.unwrap();
        assert!(include.contains(&"foo".to_string()));
        assert!(include.contains(&"reasoning.encrypted_content".to_string()));
    }
}
