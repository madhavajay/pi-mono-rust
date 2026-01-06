use pi::ai::is_context_overflow;
use pi::{AssistantMessage, Cost, Usage};

// Source: packages/ai/test/context-overflow.test.ts

#[test]
fn claude_3_5_haiku_should_detect_overflow_via_iscontextoverflow() {
    let message = error_message("prompt is too long: 213462 tokens > 200000 maximum");
    assert!(is_context_overflow(&message, None));
}

#[test]
fn claude_sonnet_4_should_detect_overflow_via_iscontextoverflow() {
    let message = error_message("prompt is too long: 213462 tokens > 200000 maximum");
    assert!(is_context_overflow(&message, None));
}

#[test]
fn gpt_4o_mini_should_detect_overflow_via_iscontextoverflow() {
    let message = error_message("This model's maximum context length is 128000 tokens.");
    assert!(is_context_overflow(&message, None));
}

#[test]
fn gpt_4o_should_detect_overflow_via_iscontextoverflow() {
    let message = error_message("Your input exceeds the context window of this model.");
    assert!(is_context_overflow(&message, None));
}

#[test]
fn gemini_2_0_flash_should_detect_overflow_via_iscontextoverflow() {
    let message = error_message(
        "The input token count (1196265) exceeds the maximum number of tokens allowed (1048575)",
    );
    assert!(is_context_overflow(&message, None));
}

#[test]
fn grok_3_fast_should_detect_overflow_via_iscontextoverflow() {
    let message = error_message(
        "This model's maximum prompt length is 131072 but the request contains 537812 tokens",
    );
    assert!(is_context_overflow(&message, None));
}

#[test]
fn llama_3_3_70b_versatile_should_detect_overflow_via_iscontextoverflow() {
    let message = error_message("Please reduce the length of the messages or completion");
    assert!(is_context_overflow(&message, None));
}

#[test]
fn qwen_3_235b_should_detect_overflow_via_iscontextoverflow() {
    let message = error_message("400 status code (no body)");
    assert!(is_context_overflow(&message, None));
}

#[test]
fn glm_4_5_flash_should_detect_overflow_via_iscontextoverflow_silent_overflow_or_rate_limit() {
    let message = stop_message_with_usage(140_000);
    assert!(is_context_overflow(&message, Some(128_000)));
}

#[test]
fn devstral_medium_latest_should_detect_overflow_via_iscontextoverflow() {
    let message = error_message("413 status code (no body)");
    assert!(is_context_overflow(&message, None));
}

#[test]
fn anthropic_claude_sonnet_4_via_openrouter_should_detect_overflow_via_iscontextoverflow() {
    let message = error_message("This endpoint's maximum context length is 200000 tokens.");
    assert!(is_context_overflow(&message, None));
}

#[test]
fn deepseek_deepseek_v3_2_via_openrouter_should_detect_overflow_via_iscontextoverflow() {
    let message = error_message("This endpoint's maximum context length is 131072 tokens.");
    assert!(is_context_overflow(&message, None));
}

#[test]
fn mistralai_mistral_large_2512_via_openrouter_should_detect_overflow_via_iscontextoverflow() {
    let message = error_message("This endpoint's maximum context length is 128000 tokens.");
    assert!(is_context_overflow(&message, None));
}

#[test]
fn google_gemini_2_5_flash_via_openrouter_should_detect_overflow_via_iscontextoverflow() {
    let message = error_message("This endpoint's maximum context length is 1048575 tokens.");
    assert!(is_context_overflow(&message, None));
}

#[test]
fn meta_llama_llama_4_maverick_via_openrouter_should_detect_overflow_via_iscontextoverflow() {
    let message = error_message("This endpoint's maximum context length is 131072 tokens.");
    assert!(is_context_overflow(&message, None));
}

#[test]
fn gpt_oss_20b_should_detect_overflow_via_iscontextoverflow_ollama_silently_truncates() {
    let message = error_message("exceeds the available context size");
    assert!(is_context_overflow(&message, None));
}

#[test]
fn should_detect_overflow_via_iscontextoverflow() {
    let message = error_message("token limit exceeded");
    assert!(is_context_overflow(&message, None));
}

fn error_message(text: &str) -> AssistantMessage {
    AssistantMessage {
        content: Vec::new(),
        api: "mock".to_string(),
        provider: "mock".to_string(),
        model: "mock".to_string(),
        usage: default_usage(),
        stop_reason: "error".to_string(),
        error_message: Some(text.to_string()),
        timestamp: 0,
    }
}

fn stop_message_with_usage(input: i64) -> AssistantMessage {
    AssistantMessage {
        content: Vec::new(),
        api: "mock".to_string(),
        provider: "mock".to_string(),
        model: "mock".to_string(),
        usage: Usage {
            input,
            output: 0,
            cache_read: 0,
            cache_write: 0,
            total_tokens: Some(input),
            cost: Some(Cost {
                input: 0.0,
                output: 0.0,
                cache_read: 0.0,
                cache_write: 0.0,
                total: 0.0,
            }),
        },
        stop_reason: "stop".to_string(),
        error_message: None,
        timestamp: 0,
    }
}

fn default_usage() -> Usage {
    Usage {
        input: 1,
        output: 0,
        cache_read: 0,
        cache_write: 0,
        total_tokens: Some(1),
        cost: Some(Cost {
            input: 0.0,
            output: 0.0,
            cache_read: 0.0,
            cache_write: 0.0,
            total: 0.0,
        }),
    }
}
