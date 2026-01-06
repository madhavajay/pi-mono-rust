//! OpenAI Codex provider tests - ported from:
//! - pi-mono/packages/ai/test/openai-codex.test.ts
//! - pi-mono/packages/ai/test/openai-codex-include.test.ts
//! - pi-mono/packages/ai/test/openai-codex-stream.test.ts

use pi::api::openai_codex::{
    add_codex_bridge_message, filter_input, handle_orphaned_outputs, normalize_model,
    parse_codex_error, transform_request_body, CodexRequestBody, CodexRequestOptions,
    ReasoningEffort, CODEX_PI_BRIDGE,
};
use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::json;

// ============================================================================
// openai-codex.test.ts - Request Transformer Tests
// ============================================================================

#[test]
fn test_filters_item_reference_strips_ids_and_inserts_bridge_message() {
    // Source: openai-codex.test.ts - "filters item_reference, strips ids, and inserts bridge message"
    let input = vec![
        json!({
            "type": "message",
            "role": "developer",
            "id": "sys-1",
            "content": [{"type": "input_text", "text": "You are an expert..."}]
        }),
        json!({
            "type": "message",
            "role": "user",
            "id": "user-1",
            "content": [{"type": "input_text", "text": "hello"}]
        }),
        json!({"type": "item_reference", "id": "ref-1"}),
        json!({
            "type": "function_call_output",
            "call_id": "missing",
            "name": "tool",
            "output": "result"
        }),
    ];

    // Test filter_input removes item_reference and strips IDs
    let filtered = filter_input(&input);
    assert_eq!(filtered.len(), 3);
    assert!(!filtered
        .iter()
        .any(|item| item.get("type").and_then(|t| t.as_str()) == Some("item_reference")));
    assert!(!filtered.iter().any(|item| item.get("id").is_some()));

    // Test add_codex_bridge_message adds bridge as first message
    let with_bridge = add_codex_bridge_message(&filtered, true, None);
    assert_eq!(
        with_bridge[0].get("role").and_then(|r| r.as_str()),
        Some("developer")
    );
    let bridge_text = with_bridge[0]
        .get("content")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|c| c.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("");
    assert!(bridge_text.contains("Codex Running in Pi"));

    // Test transform_request_body sets store=false, stream=true, and includes encrypted_content
    let mut body = CodexRequestBody {
        model: "gpt-5.1-codex".to_string(),
        store: None,
        stream: None,
        instructions: None,
        input: Some(input),
        tools: Some(vec![
            json!({"type": "function", "name": "tool", "description": "", "parameters": {}}),
        ]),
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
        &CodexRequestOptions::default(),
        true,
        None,
    );

    assert_eq!(body.store, Some(false));
    assert_eq!(body.stream, Some(true));
    assert_eq!(body.instructions, Some("CODEX_INSTRUCTIONS".to_string()));
    let include = body.include.unwrap();
    assert!(include.contains(&"reasoning.encrypted_content".to_string()));
}

#[test]
fn test_handle_orphaned_outputs() {
    // Source: openai-codex.test.ts - orphaned function_call_output gets converted to assistant message
    let mut input = vec![
        json!({
            "type": "function_call",
            "id": "fc1",
            "call_id": "call_123",
            "name": "read",
            "arguments": "{}"
        }),
        json!({
            "type": "function_call_output",
            "call_id": "call_123",
            "name": "read",
            "output": "file contents"
        }),
        json!({
            "type": "function_call_output",
            "call_id": "orphan_call",
            "name": "tool",
            "output": "orphan result"
        }),
    ];

    handle_orphaned_outputs(&mut input);

    // First function_call_output should remain unchanged (has matching call_id)
    assert_eq!(
        input[1].get("type").and_then(|t| t.as_str()),
        Some("function_call_output")
    );

    // Second function_call_output should be converted to assistant message
    assert_eq!(
        input[2].get("type").and_then(|t| t.as_str()),
        Some("message")
    );
    assert_eq!(
        input[2].get("role").and_then(|r| r.as_str()),
        Some("assistant")
    );
    let content = input[2]
        .get("content")
        .and_then(|c| c.as_str())
        .unwrap_or("");
    assert!(content.contains("Previous tool result"));
}

// ============================================================================
// openai-codex.test.ts - Model Normalization Tests
// ============================================================================

#[test]
fn test_maps_space_separated_codex_mini_names() {
    // Source: openai-codex.test.ts - "maps space-separated codex-mini names to codex-mini-latest"
    assert_eq!(
        normalize_model(Some("gpt 5 codex mini")),
        "codex-mini-latest"
    );
}

#[test]
fn test_normalizes_standard_model_names() {
    assert_eq!(normalize_model(Some("gpt-5.1-codex")), "gpt-5.1-codex");
    assert_eq!(normalize_model(Some("gpt-5.2-codex")), "gpt-5.2-codex");
    assert_eq!(normalize_model(None), "gpt-5.1");
    assert_eq!(normalize_model(Some("")), "gpt-5.1");
    assert_eq!(
        normalize_model(Some("openai/gpt-5.1-codex")),
        "gpt-5.1-codex"
    );
}

// ============================================================================
// openai-codex.test.ts - Error Parsing Tests
// ============================================================================

#[test]
fn test_produces_friendly_usage_limit_messages_and_rate_limits() {
    // Source: openai-codex.test.ts - "produces friendly usage-limit messages and rate limits"
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-codex-primary-used-percent",
        HeaderValue::from_static("99"),
    );
    headers.insert(
        "x-codex-primary-window-minutes",
        HeaderValue::from_static("60"),
    );
    let reset_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 600;
    headers.insert(
        "x-codex-primary-reset-at",
        HeaderValue::from_str(&reset_at.to_string()).unwrap(),
    );

    let body = serde_json::json!({
        "error": {
            "code": "usage_limit_reached",
            "plan_type": "Plus",
            "resets_at": reset_at
        }
    })
    .to_string();

    let info = parse_codex_error(429, &headers, &body);

    assert!(info.friendly_message.is_some());
    assert!(info
        .friendly_message
        .as_ref()
        .unwrap()
        .to_lowercase()
        .contains("usage limit"));
    assert!(info.rate_limits.is_some());
    assert_eq!(
        info.rate_limits
            .as_ref()
            .unwrap()
            .primary
            .as_ref()
            .unwrap()
            .used_percent,
        Some(99.0)
    );
}

#[test]
fn test_handles_rate_limit_exceeded_error() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-codex-primary-used-percent",
        HeaderValue::from_static("100"),
    );

    let body = serde_json::json!({
        "error": {
            "code": "rate_limit_exceeded",
            "message": "Rate limit exceeded"
        }
    })
    .to_string();

    let info = parse_codex_error(429, &headers, &body);

    assert!(info.friendly_message.is_some());
    assert!(
        info.friendly_message
            .as_ref()
            .unwrap()
            .to_lowercase()
            .contains("usage limit")
            || info.message.to_lowercase().contains("rate limit")
    );
}

// ============================================================================
// openai-codex-include.test.ts - Include Handling Tests
// ============================================================================

#[test]
fn test_always_includes_reasoning_encrypted_content() {
    // Source: openai-codex-include.test.ts - "always includes reasoning.encrypted_content when caller include is custom"
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

#[test]
fn test_does_not_duplicate_reasoning_encrypted_content() {
    // Source: openai-codex-include.test.ts - "does not duplicate reasoning.encrypted_content"
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
            include: Some(vec![
                "foo".to_string(),
                "reasoning.encrypted_content".to_string(),
            ]),
            ..Default::default()
        },
        true,
        None,
    );

    let include = body.include.unwrap();
    let count = include
        .iter()
        .filter(|s| *s == "reasoning.encrypted_content")
        .count();
    assert_eq!(count, 1);
}

// ============================================================================
// Additional Tests for Reasoning Config
// ============================================================================

#[test]
fn test_reasoning_config_with_explicit_effort() {
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
            reasoning_effort: Some(ReasoningEffort::High),
            ..Default::default()
        },
        true,
        None,
    );

    let reasoning = body.reasoning.unwrap();
    assert_eq!(
        reasoning.get("effort").and_then(|e| e.as_str()),
        Some("high")
    );
}

#[test]
fn test_reasoning_config_clamps_xhigh_for_codex_mini() {
    // Codex mini doesn't support xhigh, should be clamped to high
    let mut body = CodexRequestBody {
        model: "gpt-5.1-codex-mini".to_string(),
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
            reasoning_effort: Some(ReasoningEffort::XHigh),
            ..Default::default()
        },
        true,
        None,
    );

    let reasoning = body.reasoning.unwrap();
    assert_eq!(
        reasoning.get("effort").and_then(|e| e.as_str()),
        Some("high")
    );
}

#[test]
fn test_bridge_message_contains_codex_pi_bridge() {
    // The bridge message should contain the CODEX_PI_BRIDGE content
    assert!(CODEX_PI_BRIDGE.contains("Codex Running in Pi"));
    assert!(CODEX_PI_BRIDGE.contains("APPLY_PATCH DOES NOT EXIST"));
    assert!(CODEX_PI_BRIDGE.contains("edit"));
}

// ============================================================================
// Prompt Caching Tests
// Note: These require network access to verify, so we test the structure only
// ============================================================================

#[test]
fn test_model_family_detection() {
    use pi::api::openai_codex::ModelFamily;

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

// ============================================================================
// SSE Parsing Tests
// ============================================================================

#[test]
fn test_parse_sse_chunk_with_event_type() {
    use pi::api::openai_codex::parse_sse_chunk;

    let chunk = r#"event: response.output_text.delta
data: {"type": "response.output_text.delta", "delta": "Hello"}"#;

    let event = parse_sse_chunk(chunk).unwrap();
    assert_eq!(event.event_type, "response.output_text.delta");
    assert_eq!(
        event.data.get("delta").and_then(|d| d.as_str()),
        Some("Hello")
    );
}

#[test]
fn test_parse_sse_chunk_done_returns_none() {
    use pi::api::openai_codex::parse_sse_chunk;

    let chunk = "data: [DONE]";
    assert!(parse_sse_chunk(chunk).is_none());
}

#[test]
fn test_parse_sse_chunk_extracts_type_from_json() {
    use pi::api::openai_codex::parse_sse_chunk;

    let chunk = r#"data: {"type": "response.completed", "status": "completed"}"#;

    let event = parse_sse_chunk(chunk).unwrap();
    assert_eq!(event.event_type, "response.completed");
}
