use pi::coding_agent::parse_model_pattern;
use pi::coding_agent::Model;

// Source: packages/coding-agent/test/model-resolver.test.ts

#[test]
fn exact_match_returns_model_with_off_thinking_level() {
    let models = mock_models();
    let result = parse_model_pattern("claude-sonnet-4-5", &models);
    assert_eq!(
        result.model.as_ref().map(|m| m.id.as_str()),
        Some("claude-sonnet-4-5")
    );
    assert_eq!(result.thinking_level, "off");
    assert!(result.warning.is_none());
}

#[test]
fn partial_match_returns_best_model() {
    let models = mock_models();
    let result = parse_model_pattern("sonnet", &models);
    assert_eq!(
        result.model.as_ref().map(|m| m.id.as_str()),
        Some("claude-sonnet-4-5")
    );
    assert_eq!(result.thinking_level, "off");
    assert!(result.warning.is_none());
}

#[test]
fn no_match_returns_null_model() {
    let models = mock_models();
    let result = parse_model_pattern("nonexistent", &models);
    assert!(result.model.is_none());
    assert_eq!(result.thinking_level, "off");
    assert!(result.warning.is_none());
}

#[test]
fn sonnet_high_returns_sonnet_with_high_thinking_level() {
    let models = mock_models();
    let result = parse_model_pattern("sonnet:high", &models);
    assert_eq!(
        result.model.as_ref().map(|m| m.id.as_str()),
        Some("claude-sonnet-4-5")
    );
    assert_eq!(result.thinking_level, "high");
    assert!(result.warning.is_none());
}

#[test]
fn gpt_4o_medium_returns_gpt_4o_with_medium_thinking_level() {
    let models = mock_models();
    let result = parse_model_pattern("gpt-4o:medium", &models);
    assert_eq!(result.model.as_ref().map(|m| m.id.as_str()), Some("gpt-4o"));
    assert_eq!(result.thinking_level, "medium");
    assert!(result.warning.is_none());
}

#[test]
fn all_valid_thinking_levels_work() {
    let models = mock_models();
    for level in ["off", "minimal", "low", "medium", "high", "xhigh"] {
        let result = parse_model_pattern(&format!("sonnet:{level}"), &models);
        assert_eq!(
            result.model.as_ref().map(|m| m.id.as_str()),
            Some("claude-sonnet-4-5")
        );
        assert_eq!(result.thinking_level, level);
        assert!(result.warning.is_none());
    }
}

#[test]
fn sonnet_random_returns_sonnet_with_off_and_warning() {
    let models = mock_models();
    let result = parse_model_pattern("sonnet:random", &models);
    assert_eq!(
        result.model.as_ref().map(|m| m.id.as_str()),
        Some("claude-sonnet-4-5")
    );
    assert_eq!(result.thinking_level, "off");
    let warning = result.warning.unwrap_or_default();
    assert!(warning.contains("Invalid thinking level"));
    assert!(warning.contains("random"));
}

#[test]
fn gpt_4o_invalid_returns_gpt_4o_with_off_and_warning() {
    let models = mock_models();
    let result = parse_model_pattern("gpt-4o:invalid", &models);
    assert_eq!(result.model.as_ref().map(|m| m.id.as_str()), Some("gpt-4o"));
    assert_eq!(result.thinking_level, "off");
    let warning = result.warning.unwrap_or_default();
    assert!(warning.contains("Invalid thinking level"));
}

#[test]
fn qwen3_coder_exacto_matches_the_model_with_off() {
    let models = mock_models();
    let result = parse_model_pattern("qwen/qwen3-coder:exacto", &models);
    assert_eq!(
        result.model.as_ref().map(|m| m.id.as_str()),
        Some("qwen/qwen3-coder:exacto")
    );
    assert_eq!(result.thinking_level, "off");
    assert!(result.warning.is_none());
}

#[test]
fn openrouter_qwen_qwen3_coder_exacto_matches_with_provider_prefix() {
    let models = mock_models();
    let result = parse_model_pattern("openrouter/qwen/qwen3-coder:exacto", &models);
    assert_eq!(
        result.model.as_ref().map(|m| m.id.as_str()),
        Some("qwen/qwen3-coder:exacto")
    );
    assert_eq!(
        result.model.as_ref().map(|m| m.provider.as_str()),
        Some("openrouter")
    );
    assert_eq!(result.thinking_level, "off");
    assert!(result.warning.is_none());
}

#[test]
fn qwen3_coder_exacto_high_matches_model_with_high_thinking_level() {
    let models = mock_models();
    let result = parse_model_pattern("qwen/qwen3-coder:exacto:high", &models);
    assert_eq!(
        result.model.as_ref().map(|m| m.id.as_str()),
        Some("qwen/qwen3-coder:exacto")
    );
    assert_eq!(result.thinking_level, "high");
    assert!(result.warning.is_none());
}

#[test]
fn openrouter_qwen_qwen3_coder_exacto_high_matches_with_provider_and_thinking_level() {
    let models = mock_models();
    let result = parse_model_pattern("openrouter/qwen/qwen3-coder:exacto:high", &models);
    assert_eq!(
        result.model.as_ref().map(|m| m.id.as_str()),
        Some("qwen/qwen3-coder:exacto")
    );
    assert_eq!(
        result.model.as_ref().map(|m| m.provider.as_str()),
        Some("openrouter")
    );
    assert_eq!(result.thinking_level, "high");
    assert!(result.warning.is_none());
}

#[test]
fn gpt_4o_extended_matches_the_extended_model() {
    let models = mock_models();
    let result = parse_model_pattern("openai/gpt-4o:extended", &models);
    assert_eq!(
        result.model.as_ref().map(|m| m.id.as_str()),
        Some("openai/gpt-4o:extended")
    );
    assert_eq!(result.thinking_level, "off");
    assert!(result.warning.is_none());
}

#[test]
fn qwen3_coder_exacto_random_returns_model_with_off_and_warning() {
    let models = mock_models();
    let result = parse_model_pattern("qwen/qwen3-coder:exacto:random", &models);
    assert_eq!(
        result.model.as_ref().map(|m| m.id.as_str()),
        Some("qwen/qwen3-coder:exacto")
    );
    assert_eq!(result.thinking_level, "off");
    let warning = result.warning.unwrap_or_default();
    assert!(warning.contains("Invalid thinking level"));
    assert!(warning.contains("random"));
}

#[test]
fn qwen3_coder_exacto_high_random_returns_model_with_off_and_warning() {
    let models = mock_models();
    let result = parse_model_pattern("qwen/qwen3-coder:exacto:high:random", &models);
    assert_eq!(
        result.model.as_ref().map(|m| m.id.as_str()),
        Some("qwen/qwen3-coder:exacto")
    );
    assert_eq!(result.thinking_level, "off");
    let warning = result.warning.unwrap_or_default();
    assert!(warning.contains("Invalid thinking level"));
    assert!(warning.contains("random"));
}

#[test]
fn empty_pattern_matches_via_partial_matching() {
    let models = mock_models();
    let result = parse_model_pattern("", &models);
    assert!(result.model.is_some());
    assert_eq!(result.thinking_level, "off");
}

#[test]
fn pattern_ending_with_colon_treats_empty_suffix_as_invalid() {
    let models = mock_models();
    let result = parse_model_pattern("sonnet:", &models);
    assert_eq!(
        result.model.as_ref().map(|m| m.id.as_str()),
        Some("claude-sonnet-4-5")
    );
    let warning = result.warning.unwrap_or_default();
    assert!(warning.contains("Invalid thinking level"));
}

fn mock_models() -> Vec<Model> {
    vec![
        Model {
            id: "claude-sonnet-4-5".to_string(),
            name: "Claude Sonnet 4.5".to_string(),
            api: "anthropic-messages".to_string(),
            provider: "anthropic".to_string(),
            base_url: "https://api.anthropic.com".to_string(),
            reasoning: true,
            input: vec!["text".to_string(), "image".to_string()],
            cost: pi::Cost {
                input: 3.0,
                output: 15.0,
                cache_read: 0.3,
                cache_write: 3.75,
                total: 22.05,
            },
            context_window: 200_000,
            max_tokens: 8192,
            headers: None,
        },
        Model {
            id: "gpt-4o".to_string(),
            name: "GPT-4o".to_string(),
            api: "anthropic-messages".to_string(),
            provider: "openai".to_string(),
            base_url: "https://api.openai.com".to_string(),
            reasoning: false,
            input: vec!["text".to_string(), "image".to_string()],
            cost: pi::Cost {
                input: 5.0,
                output: 15.0,
                cache_read: 0.5,
                cache_write: 5.0,
                total: 25.5,
            },
            context_window: 128_000,
            max_tokens: 4096,
            headers: None,
        },
        Model {
            id: "qwen/qwen3-coder:exacto".to_string(),
            name: "Qwen3 Coder Exacto".to_string(),
            api: "anthropic-messages".to_string(),
            provider: "openrouter".to_string(),
            base_url: "https://openrouter.ai/api/v1".to_string(),
            reasoning: true,
            input: vec!["text".to_string()],
            cost: pi::Cost {
                input: 1.0,
                output: 2.0,
                cache_read: 0.1,
                cache_write: 1.0,
                total: 4.1,
            },
            context_window: 128_000,
            max_tokens: 8192,
            headers: None,
        },
        Model {
            id: "openai/gpt-4o:extended".to_string(),
            name: "GPT-4o Extended".to_string(),
            api: "anthropic-messages".to_string(),
            provider: "openrouter".to_string(),
            base_url: "https://openrouter.ai/api/v1".to_string(),
            reasoning: false,
            input: vec!["text".to_string(), "image".to_string()],
            cost: pi::Cost {
                input: 5.0,
                output: 15.0,
                cache_read: 0.5,
                cache_write: 5.0,
                total: 25.5,
            },
            context_window: 128_000,
            max_tokens: 4096,
            headers: None,
        },
    ]
}
