use pi::ai::{complete, get_model, Context, Message, StreamOptions};
use pi::{Usage, UserContent, UserMessage};
use std::time::{SystemTime, UNIX_EPOCH};

// Source: packages/ai/test/total-tokens.test.ts

#[test]
fn claude_3_5_haiku_should_return_totaltokens_equal_to_sum_of_components() {
    run_total_tokens_test("anthropic", "claude-3-5-haiku-20241022");
}

#[test]
fn gpt_4o_mini_should_return_totaltokens_equal_to_sum_of_components() {
    run_total_tokens_test("openai", "gpt-4o-mini");
}

#[test]
fn gpt_4o_should_return_totaltokens_equal_to_sum_of_components() {
    run_total_tokens_test("openai", "gpt-4o");
}

#[test]
fn gemini_2_0_flash_should_return_totaltokens_equal_to_sum_of_components() {
    run_total_tokens_test("google", "gemini-2.0-flash");
}

#[test]
fn grok_3_fast_should_return_totaltokens_equal_to_sum_of_components() {
    run_total_tokens_test("xai", "grok-3-fast");
}

#[test]
fn openai_gpt_oss_120b_should_return_totaltokens_equal_to_sum_of_components() {
    run_total_tokens_test("groq", "openai/gpt-oss-120b");
}

#[test]
fn gpt_oss_120b_should_return_totaltokens_equal_to_sum_of_components() {
    run_total_tokens_test("cerebras", "gpt-oss-120b");
}

#[test]
fn glm_4_5_flash_should_return_totaltokens_equal_to_sum_of_components() {
    run_total_tokens_test("zai", "glm-4.5-flash");
}

#[test]
fn devstral_medium_latest_should_return_totaltokens_equal_to_sum_of_components() {
    run_total_tokens_test("mistral", "devstral-medium-latest");
}

#[test]
fn anthropic_claude_sonnet_4_should_return_totaltokens_equal_to_sum_of_components() {
    run_total_tokens_test("openrouter", "anthropic/claude-sonnet-4");
}

#[test]
fn deepseek_deepseek_chat_should_return_totaltokens_equal_to_sum_of_components() {
    run_total_tokens_test("openrouter", "deepseek/deepseek-chat");
}

#[test]
fn mistralai_mistral_small_3_1_24b_instruct_should_return_totaltokens_equal_to_sum_of_components() {
    run_total_tokens_test("openrouter", "mistralai/mistral-small-3.1-24b-instruct");
}

#[test]
fn google_gemini_2_0_flash_001_should_return_totaltokens_equal_to_sum_of_components() {
    run_total_tokens_test("openrouter", "google/gemini-2.0-flash-001");
}

#[test]
fn meta_llama_llama_4_maverick_should_return_totaltokens_equal_to_sum_of_components() {
    run_total_tokens_test("openrouter", "meta-llama/llama-4-maverick");
}

fn run_total_tokens_test(provider: &str, model_id: &str) {
    let model = get_model(provider, model_id);
    let system_prompt = long_system_prompt();
    let context = Context {
        system_prompt: Some(system_prompt.clone()),
        messages: vec![Message::User(UserMessage {
            content: UserContent::Text("What is 2 + 2? Reply with just the number.".to_string()),
            timestamp: now_millis(),
        })],
        tools: None,
    };

    let response1 = complete(
        &model,
        &context,
        StreamOptions {
            signal: None,
            reasoning_effort: None,
        },
    );
    assert_eq!(response1.stop_reason, "stop");
    assert_total_tokens(&response1.usage);

    let context2 = Context {
        system_prompt: Some(system_prompt),
        messages: vec![
            context.messages[0].clone(),
            Message::Assistant(response1),
            Message::User(UserMessage {
                content: UserContent::Text(
                    "What is 3 + 3? Reply with just the number.".to_string(),
                ),
                timestamp: now_millis(),
            }),
        ],
        tools: None,
    };

    let response2 = complete(
        &model,
        &context2,
        StreamOptions {
            signal: None,
            reasoning_effort: None,
        },
    );
    assert_eq!(response2.stop_reason, "stop");
    assert_total_tokens(&response2.usage);
}

fn assert_total_tokens(usage: &Usage) {
    let computed = usage.input + usage.output + usage.cache_read + usage.cache_write;
    assert_eq!(usage.total_tokens, Some(computed));
}

fn long_system_prompt() -> String {
    let mut prompt = String::from("You are a helpful assistant. Be concise in your responses.\n\n");
    for _ in 0..20 {
        prompt.push_str("Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.\n");
    }
    prompt
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}
