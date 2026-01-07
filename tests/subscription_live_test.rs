// Live subscription tests for Anthropic and OpenAI Codex.
// Run with: PI_LIVE_TESTS=1 cargo test --test subscription_live_test -- --ignored --nocapture

use pi::agent::{AgentMessage, LlmContext, StreamEvents};
use pi::api::{build_anthropic_messages, stream_anthropic, AnthropicCallOptions, AnthropicTool};
use pi::api::openai_codex::{
    stream_openai_codex_responses, CodexStreamOptions, CodexTool,
};
use pi::coding_agent::{
    anthropic_refresh_token, openai_codex_refresh_token, AuthCredential, AuthStorage, Model,
    ModelRegistry, OAuthCredentials,
};
use pi::config::get_auth_path;
use pi::{ContentBlock, UserContent, UserMessage};
use serde_json::json;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

const ANTHROPIC_MODEL: &str = "claude-3-5-haiku-20241022";
const CODEX_MODEL: &str = "gpt-5.1-codex";

#[derive(Clone)]
struct ProviderAuth {
    api_key: String,
    use_oauth: bool,
}

fn live_tests_enabled() -> bool {
    matches!(
        std::env::var("PI_LIVE_TESTS")
            .ok()
            .as_deref()
            .map(|value| value.to_lowercase())
            .as_deref(),
        Some("1") | Some("true") | Some("yes")
    )
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

fn user_message(text: &str) -> AgentMessage {
    AgentMessage::User(UserMessage {
        content: UserContent::Text(text.to_string()),
        timestamp: now_millis(),
    })
}

fn text_from_blocks(blocks: &[ContentBlock]) -> String {
    blocks
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

fn calculator_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "a": { "type": "number", "description": "First number" },
            "b": { "type": "number", "description": "Second number" },
            "operation": {
                "type": "string",
                "enum": ["add", "subtract", "multiply", "divide"],
                "description": "The operation to perform"
            }
        },
        "required": ["a", "b", "operation"]
    })
}

fn refresh_oauth(provider: &str, refresh: &str) -> Result<OAuthCredentials, String> {
    match provider {
        "anthropic" => anthropic_refresh_token(refresh),
        "openai-codex" => openai_codex_refresh_token(refresh),
        _ => Err(format!("Unsupported OAuth refresh provider: {provider}")),
    }
}

fn resolve_provider_auth(
    storage: &mut AuthStorage,
    provider: &str,
) -> Option<ProviderAuth> {
    let credential = storage.get(provider).cloned();
    match credential {
        Some(AuthCredential::ApiKey { key }) => Some(ProviderAuth {
            api_key: key,
            use_oauth: false,
        }),
        Some(AuthCredential::OAuth {
            access,
            refresh,
            expires,
            ..
        }) => {
            let expired = expires.map_or(false, |value| value <= now_millis());
            if expired {
                if let Some(refresh) = refresh.as_deref() {
                    match refresh_oauth(provider, refresh) {
                        Ok(updated) => {
                            storage.set(provider, updated.to_auth_credential());
                            return Some(ProviderAuth {
                                api_key: updated.access,
                                use_oauth: true,
                            });
                        }
                        Err(err) => {
                            eprintln!("Failed to refresh {provider} OAuth token: {err}");
                            return None;
                        }
                    }
                }
            }
            Some(ProviderAuth {
                api_key: access,
                use_oauth: true,
            })
        }
        None => storage.get_api_key(provider).map(|key| ProviderAuth {
            api_key: key,
            use_oauth: false,
        }),
    }
}

fn require_live() -> bool {
    if !live_tests_enabled() {
        eprintln!("Skipping live tests. Set PI_LIVE_TESTS=1 to enable.");
        return false;
    }
    true
}

fn resolve_model(registry: &ModelRegistry, provider: &str, model_id: &str) -> Option<Model> {
    let model = registry.find(provider, model_id);
    if model.is_none() {
        eprintln!("Missing model {provider}/{model_id} in registry.");
    }
    model
}

#[test]
#[ignore = "requires live subscription credentials"]
fn anthropic_live_streaming_text() {
    if !require_live() {
        return;
    }

    let mut storage = AuthStorage::new(get_auth_path());
    let auth = match resolve_provider_auth(&mut storage, "anthropic") {
        Some(auth) => auth,
        None => {
            eprintln!("Missing Anthropic auth in auth.json.");
            return;
        }
    };

    let registry = ModelRegistry::new(storage, None);
    let model = match resolve_model(&registry, "anthropic", ANTHROPIC_MODEL) {
        Some(model) => model,
        None => return,
    };

    let context = LlmContext {
        system_prompt: "You are a helpful assistant. Be concise.".to_string(),
        messages: vec![user_message("Reply with exactly: 'Hello test successful'.")],
    };

    let messages = build_anthropic_messages(&context);
    let saw_text = Rc::new(RefCell::new(false));
    let saw_text_ref = saw_text.clone();
    let mut events = StreamEvents::new(Box::new(move |event| {
        if matches!(
            event,
            pi::ai::AssistantMessageEvent::TextStart { .. }
                | pi::ai::AssistantMessageEvent::TextDelta { .. }
        ) {
            *saw_text_ref.borrow_mut() = true;
        }
    }));

    let response = stream_anthropic(
        &model,
        messages,
        AnthropicCallOptions {
            model: &model.id,
            api_key: &auth.api_key,
            use_oauth: auth.use_oauth,
            tools: &[],
            base_url: &model.base_url,
            extra_headers: model.headers.as_ref(),
            system: Some(&context.system_prompt),
        },
        &mut events,
    )
    .expect("anthropic stream");

    assert!(text_from_blocks(&response.content).contains("Hello test successful"));
    assert!(*saw_text.borrow());
    assert!(response.usage.total_tokens.unwrap_or(0) > 0);
    assert_eq!(response.stop_reason, "stop");
}

#[test]
#[ignore = "requires live subscription credentials"]
fn anthropic_live_tool_call() {
    if !require_live() {
        return;
    }

    let mut storage = AuthStorage::new(get_auth_path());
    let auth = match resolve_provider_auth(&mut storage, "anthropic") {
        Some(auth) => auth,
        None => {
            eprintln!("Missing Anthropic auth in auth.json.");
            return;
        }
    };

    let registry = ModelRegistry::new(storage, None);
    let model = match resolve_model(&registry, "anthropic", ANTHROPIC_MODEL) {
        Some(model) => model,
        None => return,
    };

    let context = LlmContext {
        system_prompt: "Always call the calculator tool for arithmetic. Do not answer directly."
            .to_string(),
        messages: vec![user_message("Calculate 15 + 27 using the calculator tool.")],
    };
    let tools = vec![AnthropicTool {
        name: "calculator".to_string(),
        description: "Perform basic arithmetic operations".to_string(),
        input_schema: calculator_schema(),
    }];

    let messages = build_anthropic_messages(&context);
    let saw_tool = Rc::new(RefCell::new(false));
    let saw_tool_ref = saw_tool.clone();
    let mut events = StreamEvents::new(Box::new(move |event| {
        if matches!(
            event,
            pi::ai::AssistantMessageEvent::ToolCallStart { .. }
                | pi::ai::AssistantMessageEvent::ToolCallDelta { .. }
                | pi::ai::AssistantMessageEvent::ToolCallEnd { .. }
        ) {
            *saw_tool_ref.borrow_mut() = true;
        }
    }));

    let response = stream_anthropic(
        &model,
        messages,
        AnthropicCallOptions {
            model: &model.id,
            api_key: &auth.api_key,
            use_oauth: auth.use_oauth,
            tools: &tools,
            base_url: &model.base_url,
            extra_headers: model.headers.as_ref(),
            system: Some(&context.system_prompt),
        },
        &mut events,
    )
    .expect("anthropic tool stream");

    let has_tool_call = response
        .content
        .iter()
        .any(|block| matches!(block, ContentBlock::ToolCall { .. }));
    assert!(has_tool_call);
    assert!(*saw_tool.borrow());
    assert_eq!(response.stop_reason, "toolUse");
}

#[test]
#[ignore = "requires live subscription credentials"]
fn codex_live_streaming_text() {
    if !require_live() {
        return;
    }

    let mut storage = AuthStorage::new(get_auth_path());
    let auth = match resolve_provider_auth(&mut storage, "openai-codex") {
        Some(auth) => auth,
        None => {
            eprintln!("Missing OpenAI Codex auth in auth.json.");
            return;
        }
    };

    let registry = ModelRegistry::new(storage, None);
    let model = match resolve_model(&registry, "openai-codex", CODEX_MODEL) {
        Some(model) => model,
        None => return,
    };

    let context = LlmContext {
        system_prompt: "You are a helpful assistant. Be concise.".to_string(),
        messages: vec![user_message("Reply with exactly: 'Hello codex test successful'.")],
    };

    let saw_text = Rc::new(RefCell::new(false));
    let saw_text_ref = saw_text.clone();
    let mut events = StreamEvents::new(Box::new(move |event| {
        if matches!(
            event,
            pi::ai::AssistantMessageEvent::TextStart { .. }
                | pi::ai::AssistantMessageEvent::TextDelta { .. }
        ) {
            *saw_text_ref.borrow_mut() = true;
        }
    }));

    let response = stream_openai_codex_responses(
        &model,
        &context,
        &auth.api_key,
        &[],
        CodexStreamOptions {
            extra_headers: model.headers.clone(),
            ..Default::default()
        },
        &mut events,
    )
    .expect("codex stream");

    assert!(text_from_blocks(&response.content).contains("Hello codex test successful"));
    assert!(*saw_text.borrow());
    assert!(response.usage.total_tokens.unwrap_or(0) > 0);
    assert_eq!(response.stop_reason, "stop");
}

#[test]
#[ignore = "requires live subscription credentials"]
fn codex_live_tool_call() {
    if !require_live() {
        return;
    }

    let mut storage = AuthStorage::new(get_auth_path());
    let auth = match resolve_provider_auth(&mut storage, "openai-codex") {
        Some(auth) => auth,
        None => {
            eprintln!("Missing OpenAI Codex auth in auth.json.");
            return;
        }
    };

    let registry = ModelRegistry::new(storage, None);
    let model = match resolve_model(&registry, "openai-codex", CODEX_MODEL) {
        Some(model) => model,
        None => return,
    };

    let context = LlmContext {
        system_prompt: "Always call the calculator tool for arithmetic. Do not answer directly."
            .to_string(),
        messages: vec![user_message("Calculate 15 + 27 using the calculator tool.")],
    };

    let tools = vec![CodexTool {
        tool_type: "function".to_string(),
        name: "calculator".to_string(),
        description: "Perform basic arithmetic operations".to_string(),
        parameters: calculator_schema(),
        strict: None,
    }];

    let saw_tool = Rc::new(RefCell::new(false));
    let saw_tool_ref = saw_tool.clone();
    let mut events = StreamEvents::new(Box::new(move |event| {
        if matches!(
            event,
            pi::ai::AssistantMessageEvent::ToolCallStart { .. }
                | pi::ai::AssistantMessageEvent::ToolCallDelta { .. }
                | pi::ai::AssistantMessageEvent::ToolCallEnd { .. }
        ) {
            *saw_tool_ref.borrow_mut() = true;
        }
    }));

    let response = stream_openai_codex_responses(
        &model,
        &context,
        &auth.api_key,
        &tools,
        CodexStreamOptions {
            extra_headers: model.headers.clone(),
            ..Default::default()
        },
        &mut events,
    )
    .expect("codex tool stream");

    let has_tool_call = response
        .content
        .iter()
        .any(|block| matches!(block, ContentBlock::ToolCall { .. }));
    assert!(has_tool_call);
    assert!(*saw_tool.borrow());
    assert_eq!(response.stop_reason, "toolUse");
}
