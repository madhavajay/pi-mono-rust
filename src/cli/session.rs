use crate::agent::{
    Agent, AgentOptions, AgentStateOverride, AgentTool, AgentToolResult, LlmContext,
    Model as AgentModel, ThinkingLevel,
};
use crate::api::openai_codex::{stream_openai_codex_responses, CodexStreamOptions, CodexTool};
use crate::api::{
    assistant_error_message, build_anthropic_messages, openai_context_to_input_items,
    AnthropicCallOptions, AnthropicTool, OpenAICallOptions, OpenAITool,
};
use crate::cli::args::ThinkingLevel as CliThinkingLevel;
use crate::coding_agent::extension_host::ExtensionTool;
use crate::coding_agent::{
    load_prompt_templates, AgentSession, AgentSessionConfig, ExtensionHost,
    LoadPromptTemplatesOptions, Model as RegistryModel, ModelRegistry, SettingsManager,
};
use crate::core::messages::ContentBlock;
use crate::core::session_manager::SessionManager;
use crate::tools::{default_tool_names, default_tools};
use crate::{coding_agent::tools as agent_tools, config};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::HashSet;
use std::env;
use std::path::PathBuf;
use std::rc::Rc;

const DEFAULT_OAUTH_SYSTEM_PROMPT: &str =
    "You are Claude Code, Anthropic's official CLI for Claude.";

type AgentStreamFn = Box<
    dyn FnMut(
        &AgentModel,
        &LlmContext,
        &mut crate::agent::StreamEvents,
    ) -> crate::core::messages::AssistantMessage,
>;

#[derive(Clone, Debug)]
struct ToolSpec {
    name: String,
    description: String,
    input_schema: Value,
}

fn build_tool_defs(
    tool_names: Option<&[String]>,
    extension_tools: &[ExtensionTool],
) -> Result<Vec<ToolSpec>, String> {
    let mut specs = Vec::new();
    let default_defs = default_tools();
    for tool in default_defs {
        specs.push(ToolSpec {
            name: tool.name.to_string(),
            description: tool.description.to_string(),
            input_schema: tool.input_schema.clone(),
        });
    }

    for tool in extension_tools {
        if specs.iter().any(|spec| spec.name == tool.name) {
            eprintln!(
                "Warning: Extension tool \"{}\" conflicts with built-in tool name. Skipping.",
                tool.name
            );
            continue;
        }
        specs.push(ToolSpec {
            name: tool.name.clone(),
            description: tool
                .description
                .clone()
                .unwrap_or_else(|| "Extension tool".to_string()),
            input_schema: tool.parameters.clone().unwrap_or_else(|| {
                json!({
                    "type": "object",
                    "properties": {},
                    "additionalProperties": true
                })
            }),
        });
    }

    if let Some(tool_names) = tool_names {
        let missing = tool_names
            .iter()
            .filter(|name| !specs.iter().any(|tool| tool.name == name.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err(format!(
                "Tool(s) not implemented yet: {}",
                missing.join(", ")
            ));
        }
        specs.retain(|tool| tool_names.iter().any(|name| name == &tool.name));
    }
    Ok(specs)
}

pub fn to_agent_model(model: &RegistryModel) -> AgentModel {
    AgentModel {
        id: model.id.clone(),
        name: model.name.clone(),
        api: model.api.clone(),
        provider: model.provider.clone(),
    }
}

pub fn build_agent_tools(
    cwd: &PathBuf,
    tool_names: Option<&[String]>,
    extension_tools: &[ExtensionTool],
    extension_host: Option<Rc<RefCell<ExtensionHost>>>,
) -> Result<Vec<AgentTool>, String> {
    let available = ["read", "write", "edit", "bash", "grep", "find", "ls"];
    let mut available_set = HashSet::new();
    for name in available {
        available_set.insert(name.to_string());
    }
    for tool in extension_tools {
        available_set.insert(tool.name.clone());
    }

    let selected = match tool_names {
        Some(names) => {
            for name in names {
                if !available_set.contains(name) {
                    return Err(format!("Tool \"{name}\" is not supported"));
                }
            }
            names.to_vec()
        }
        None => {
            let mut defaults = default_tool_names();
            for tool in extension_tools {
                defaults.push(tool.name.clone());
            }
            defaults
        }
    };

    let selected_set = selected.iter().cloned().collect::<HashSet<_>>();

    let mut tools = Vec::new();
    for name in available {
        if !selected_set.contains(name) {
            continue;
        }
        match name {
            "read" => {
                let tool = agent_tools::ReadTool::new(cwd);
                tools.push(AgentTool {
                    name: "read".to_string(),
                    label: "read".to_string(),
                    description: "Read file contents".to_string(),
                    execute: Rc::new(move |call_id, params| {
                        let args = parse_read_args(params)?;
                        let result = tool.execute(call_id, args)?;
                        Ok(tool_result_to_agent_result(result))
                    }),
                });
            }
            "write" => {
                let tool = agent_tools::WriteTool::new(cwd);
                tools.push(AgentTool {
                    name: "write".to_string(),
                    label: "write".to_string(),
                    description: "Write file contents".to_string(),
                    execute: Rc::new(move |call_id, params| {
                        let args = parse_write_args(params)?;
                        let result = tool.execute(call_id, args)?;
                        Ok(tool_result_to_agent_result(result))
                    }),
                });
            }
            "edit" => {
                let tool = agent_tools::EditTool::new(cwd);
                tools.push(AgentTool {
                    name: "edit".to_string(),
                    label: "edit".to_string(),
                    description: "Edit file contents".to_string(),
                    execute: Rc::new(move |call_id, params| {
                        let args = parse_edit_args(params)?;
                        let result = tool.execute(call_id, args)?;
                        Ok(tool_result_to_agent_result(result))
                    }),
                });
            }
            "bash" => {
                let tool = agent_tools::BashTool::new(cwd);
                tools.push(AgentTool {
                    name: "bash".to_string(),
                    label: "bash".to_string(),
                    description: "Execute bash commands".to_string(),
                    execute: Rc::new(move |call_id, params| {
                        let args = parse_bash_args(params)?;
                        let result = tool.execute(call_id, args)?;
                        Ok(tool_result_to_agent_result(result))
                    }),
                });
            }
            "grep" => {
                let tool = agent_tools::GrepTool::new(cwd);
                tools.push(AgentTool {
                    name: "grep".to_string(),
                    label: "grep".to_string(),
                    description: "Search file contents".to_string(),
                    execute: Rc::new(move |call_id, params| {
                        let args = parse_grep_args(params)?;
                        let result = tool.execute(call_id, args)?;
                        Ok(tool_result_to_agent_result(result))
                    }),
                });
            }
            "find" => {
                let tool = agent_tools::FindTool::new(cwd);
                tools.push(AgentTool {
                    name: "find".to_string(),
                    label: "find".to_string(),
                    description: "Find files by pattern".to_string(),
                    execute: Rc::new(move |call_id, params| {
                        let args = parse_find_args(params)?;
                        let result = tool.execute(call_id, args)?;
                        Ok(tool_result_to_agent_result(result))
                    }),
                });
            }
            "ls" => {
                let tool = agent_tools::LsTool::new(cwd);
                tools.push(AgentTool {
                    name: "ls".to_string(),
                    label: "ls".to_string(),
                    description: "List directory contents".to_string(),
                    execute: Rc::new(move |call_id, params| {
                        let args = parse_ls_args(params)?;
                        let result = tool.execute(call_id, args)?;
                        Ok(tool_result_to_agent_result(result))
                    }),
                });
            }
            _ => {}
        }
    }

    let Some(host) = extension_host else {
        if extension_tools
            .iter()
            .any(|tool| selected_set.contains(&tool.name))
        {
            return Err(
                "Extension tools requested but extension host is not available.".to_string(),
            );
        }
        return Ok(tools);
    };

    for tool in extension_tools {
        if !selected_set.contains(&tool.name) {
            continue;
        }
        let tool_name = tool.name.clone();
        let label = tool.label.clone().unwrap_or_else(|| tool.name.clone());
        let description = tool
            .description
            .clone()
            .unwrap_or_else(|| "Extension tool".to_string());
        let host_ref = host.clone();

        tools.push(AgentTool {
            name: tool_name.clone(),
            label,
            description,
            execute: Rc::new(move |call_id, params| {
                let result = host_ref
                    .borrow_mut()
                    .call_tool(&tool_name, call_id, params, &[])?;
                if result.is_error {
                    let message = result
                        .content
                        .iter()
                        .find_map(|block| match block {
                            ContentBlock::Text { text, .. } => Some(text.clone()),
                            _ => None,
                        })
                        .unwrap_or_else(|| "Extension tool failed".to_string());
                    return Err(message);
                }
                Ok(AgentToolResult {
                    content: result.content,
                    details: result.details.unwrap_or(Value::Null),
                })
            }),
        });
    }

    Ok(tools)
}

fn parse_read_args(params: &Value) -> Result<agent_tools::ReadToolArgs, String> {
    Ok(agent_tools::ReadToolArgs {
        path: get_required_string(params, "path")?,
        offset: get_optional_usize(params, "offset"),
        limit: get_optional_usize(params, "limit"),
    })
}

fn parse_write_args(params: &Value) -> Result<agent_tools::WriteToolArgs, String> {
    Ok(agent_tools::WriteToolArgs {
        path: get_required_string(params, "path")?,
        content: get_required_string(params, "content")?,
    })
}

fn parse_edit_args(params: &Value) -> Result<agent_tools::EditToolArgs, String> {
    Ok(agent_tools::EditToolArgs {
        path: get_required_string(params, "path")?,
        old_text: get_required_string(params, "oldText")?,
        new_text: get_required_string(params, "newText")?,
    })
}

fn parse_bash_args(params: &Value) -> Result<agent_tools::BashToolArgs, String> {
    Ok(agent_tools::BashToolArgs {
        command: get_required_string(params, "command")?,
        timeout: get_optional_u64(params, "timeout"),
    })
}

fn parse_grep_args(params: &Value) -> Result<agent_tools::GrepToolArgs, String> {
    Ok(agent_tools::GrepToolArgs {
        pattern: get_required_string(params, "pattern")?,
        path: get_optional_string(params, "path"),
        glob: get_optional_string(params, "glob"),
        ignore_case: get_optional_bool(params, "ignoreCase"),
        literal: get_optional_bool(params, "literal"),
        context: get_optional_usize(params, "context"),
        limit: get_optional_usize(params, "limit"),
    })
}

fn parse_find_args(params: &Value) -> Result<agent_tools::FindToolArgs, String> {
    Ok(agent_tools::FindToolArgs {
        pattern: get_required_string(params, "pattern")?,
        path: get_optional_string(params, "path"),
        limit: get_optional_usize(params, "limit"),
    })
}

fn parse_ls_args(params: &Value) -> Result<agent_tools::LsToolArgs, String> {
    Ok(agent_tools::LsToolArgs {
        path: get_optional_string(params, "path"),
        limit: get_optional_usize(params, "limit"),
    })
}

fn get_required_string(params: &Value, key: &str) -> Result<String, String> {
    params
        .get(key)
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
        .ok_or_else(|| format!("Missing or invalid \"{}\" argument", key))
}

fn get_optional_string(params: &Value, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
}

fn get_optional_bool(params: &Value, key: &str) -> Option<bool> {
    params.get(key).and_then(|value| value.as_bool())
}

fn get_optional_usize(params: &Value, key: &str) -> Option<usize> {
    params
        .get(key)
        .and_then(|value| value.as_i64())
        .and_then(|value| {
            if value < 0 {
                None
            } else {
                Some(value as usize)
            }
        })
}

fn get_optional_u64(params: &Value, key: &str) -> Option<u64> {
    params
        .get(key)
        .and_then(|value| value.as_i64())
        .and_then(|value| if value < 0 { None } else { Some(value as u64) })
}

fn tool_result_to_agent_result(result: agent_tools::ToolResult) -> AgentToolResult {
    AgentToolResult {
        content: result.content,
        details: result.details.unwrap_or(Value::Null),
    }
}

fn build_stream_fn(
    model: RegistryModel,
    api_key: String,
    use_oauth: bool,
    tool_specs: Vec<AnthropicTool>,
) -> AgentStreamFn {
    Box::new(move |_agent_model, context, events| {
        // OAuth tokens require the Claude Code identification in the system prompt
        let system_with_oauth_prefix = if use_oauth {
            if context.system_prompt.trim().is_empty() {
                DEFAULT_OAUTH_SYSTEM_PROMPT.to_string()
            } else {
                format!(
                    "{}\n\n{}",
                    DEFAULT_OAUTH_SYSTEM_PROMPT, context.system_prompt
                )
            }
        } else {
            context.system_prompt.clone()
        };
        let system = if system_with_oauth_prefix.trim().is_empty() {
            None
        } else {
            Some(system_with_oauth_prefix.as_str())
        };

        let messages = build_anthropic_messages(context);
        let response = crate::api::stream_anthropic(
            &model,
            messages,
            AnthropicCallOptions {
                model: &model.id,
                api_key: &api_key,
                use_oauth,
                tools: &tool_specs,
                base_url: if model.base_url.is_empty() {
                    "https://api.anthropic.com/v1"
                } else {
                    model.base_url.as_str()
                },
                extra_headers: model.headers.as_ref(),
                system,
            },
            events,
        );

        match response {
            Ok(response) => response,
            Err(err) => assistant_error_message(&model, &err),
        }
    })
}

fn build_openai_stream_fn(
    model: RegistryModel,
    api_key: String,
    tool_specs: Vec<OpenAITool>,
) -> AgentStreamFn {
    Box::new(move |_agent_model, context, events| {
        let input = openai_context_to_input_items(&model, context);
        let response = crate::api::stream_openai_responses(
            &model,
            input,
            OpenAICallOptions {
                model: &model.id,
                api_key: &api_key,
                tools: &tool_specs,
                base_url: if model.base_url.is_empty() {
                    "https://api.openai.com/v1"
                } else {
                    model.base_url.as_str()
                },
                extra_headers: model.headers.as_ref(),
            },
            events,
        );

        match response {
            Ok(response) => response,
            Err(err) => assistant_error_message(&model, &err),
        }
    })
}

fn build_codex_stream_fn(
    model: RegistryModel,
    api_key: String,
    tool_specs: Vec<CodexTool>,
) -> AgentStreamFn {
    Box::new(move |_agent_model, context, events| {
        let response = stream_openai_codex_responses(
            &model,
            context,
            &api_key,
            &tool_specs,
            CodexStreamOptions {
                codex_mode: Some(true),
                extra_headers: model.headers.clone(),
                ..Default::default()
            },
            events,
        );

        match response {
            Ok(response) => response,
            Err(err) => assistant_error_message(&model, &err),
        }
    })
}

fn merge_system_prompt(
    system_prompt: Option<String>,
    append_system_prompt: Option<String>,
) -> Option<String> {
    let mut system = system_prompt;
    if let Some(append) = append_system_prompt {
        system = Some(match system {
            Some(base) => format!("{base}\n\n{append}"),
            None => append,
        });
    }
    system
}

#[allow(clippy::too_many_arguments)]
pub fn create_cli_session(
    model: RegistryModel,
    registry: ModelRegistry,
    system_prompt: Option<String>,
    append_system_prompt: Option<String>,
    tool_names: Option<&[String]>,
    extension_tools: &[ExtensionTool],
    extension_host: Option<Rc<RefCell<ExtensionHost>>>,
    api_key_override: Option<&str>,
    session_manager: SessionManager,
) -> Result<AgentSession, String> {
    let cwd = env::current_dir().map_err(|err| err.to_string())?;
    let agent_tools = build_agent_tools(&cwd, tool_names, extension_tools, extension_host)?;
    let tool_defs = build_tool_defs(tool_names, extension_tools)?;

    let stream_fn = match model.api.as_str() {
        "anthropic-messages" => {
            let (api_key, use_oauth) =
                crate::cli::auth::resolve_anthropic_credentials(api_key_override)?;
            let tool_specs = tool_defs
                .iter()
                .map(|tool| AnthropicTool {
                    name: tool.name.clone(),
                    description: tool.description.clone(),
                    input_schema: tool.input_schema.clone(),
                })
                .collect::<Vec<_>>();
            build_stream_fn(model.clone(), api_key, use_oauth, tool_specs)
        }
        "openai-responses" => {
            let api_key = crate::cli::auth::resolve_openai_credentials(api_key_override)?;
            let tool_specs = tool_defs
                .iter()
                .map(|tool| OpenAITool {
                    tool_type: "function".to_string(),
                    name: tool.name.clone(),
                    description: tool.description.clone(),
                    parameters: tool.input_schema.clone(),
                })
                .collect::<Vec<_>>();
            build_openai_stream_fn(model.clone(), api_key, tool_specs)
        }
        "openai-codex-responses" => {
            let api_key = crate::cli::auth::resolve_openai_codex_credentials(api_key_override)?;
            let tool_specs = tool_defs
                .iter()
                .map(|tool| CodexTool {
                    tool_type: "function".to_string(),
                    name: tool.name.clone(),
                    description: tool.description.clone(),
                    parameters: tool.input_schema.clone(),
                    strict: None,
                })
                .collect::<Vec<_>>();
            build_codex_stream_fn(model.clone(), api_key, tool_specs)
        }
        _ => {
            return Err(format!(
                "Model API \"{}\" is not supported in print mode.",
                model.api
            ))
        }
    };

    let system_value = merge_system_prompt(system_prompt, append_system_prompt).unwrap_or_default();
    let agent_model = to_agent_model(&model);
    let agent = Agent::new(AgentOptions {
        initial_state: Some(AgentStateOverride {
            system_prompt: Some(system_value),
            model: Some(agent_model),
            tools: Some(agent_tools),
            ..Default::default()
        }),
        stream_fn: Some(stream_fn),
        ..Default::default()
    });

    let settings_manager = SettingsManager::create("", "");

    let mut session = AgentSession::new(AgentSessionConfig {
        agent,
        session_manager,
        settings_manager,
        model_registry: registry,
    });
    let templates = load_prompt_templates(LoadPromptTemplatesOptions {
        cwd: Some(cwd),
        agent_dir: Some(config::get_agent_dir()),
    });
    session.set_prompt_templates(templates);
    Ok(session)
}

#[allow(clippy::too_many_arguments)]
pub fn create_rpc_session(
    model: RegistryModel,
    registry: ModelRegistry,
    system_prompt: Option<String>,
    append_system_prompt: Option<String>,
    tool_names: Option<&[String]>,
    extension_tools: &[ExtensionTool],
    extension_host: Option<Rc<RefCell<ExtensionHost>>>,
    api_key_override: Option<&str>,
    session_manager: SessionManager,
) -> Result<AgentSession, String> {
    let cwd = env::current_dir().map_err(|err| err.to_string())?;
    let agent_tools = build_agent_tools(&cwd, tool_names, extension_tools, extension_host)?;
    let tool_defs = build_tool_defs(tool_names, extension_tools)?;
    let stream_fn = match model.api.as_str() {
        "anthropic-messages" => {
            let (api_key, use_oauth) =
                crate::cli::auth::resolve_anthropic_credentials(api_key_override)?;
            let tool_specs = tool_defs
                .iter()
                .map(|tool| AnthropicTool {
                    name: tool.name.clone(),
                    description: tool.description.clone(),
                    input_schema: tool.input_schema.clone(),
                })
                .collect::<Vec<_>>();
            build_stream_fn(model.clone(), api_key, use_oauth, tool_specs)
        }
        "openai-responses" => {
            let api_key = crate::cli::auth::resolve_openai_credentials(api_key_override)?;
            let tool_specs = tool_defs
                .iter()
                .map(|tool| OpenAITool {
                    tool_type: "function".to_string(),
                    name: tool.name.clone(),
                    description: tool.description.clone(),
                    parameters: tool.input_schema.clone(),
                })
                .collect::<Vec<_>>();
            build_openai_stream_fn(model.clone(), api_key, tool_specs)
        }
        "openai-codex-responses" => {
            let api_key = crate::cli::auth::resolve_openai_codex_credentials(api_key_override)?;
            let tool_specs = tool_defs
                .iter()
                .map(|tool| CodexTool {
                    tool_type: "function".to_string(),
                    name: tool.name.clone(),
                    description: tool.description.clone(),
                    parameters: tool.input_schema.clone(),
                    strict: None,
                })
                .collect::<Vec<_>>();
            build_codex_stream_fn(model.clone(), api_key, tool_specs)
        }
        _ => {
            return Err(format!(
                "Model API \"{}\" is not supported in RPC mode.",
                model.api
            ))
        }
    };

    let system_value = merge_system_prompt(system_prompt, append_system_prompt).unwrap_or_default();
    let agent_model = to_agent_model(&model);
    let agent = Agent::new(AgentOptions {
        initial_state: Some(AgentStateOverride {
            system_prompt: Some(system_value),
            model: Some(agent_model),
            tools: Some(agent_tools),
            ..Default::default()
        }),
        stream_fn: Some(stream_fn),
        ..Default::default()
    });

    let settings_manager = SettingsManager::create("", "");

    let mut session = AgentSession::new(AgentSessionConfig {
        agent,
        session_manager,
        settings_manager,
        model_registry: registry,
    });
    let templates = load_prompt_templates(LoadPromptTemplatesOptions {
        cwd: Some(cwd),
        agent_dir: Some(config::get_agent_dir()),
    });
    session.set_prompt_templates(templates);
    Ok(session)
}

fn cli_thinking_level(level: &CliThinkingLevel) -> ThinkingLevel {
    match level {
        CliThinkingLevel::Off => ThinkingLevel::Off,
        CliThinkingLevel::Minimal => ThinkingLevel::Minimal,
        CliThinkingLevel::Low => ThinkingLevel::Low,
        CliThinkingLevel::Medium => ThinkingLevel::Medium,
        CliThinkingLevel::High => ThinkingLevel::High,
        CliThinkingLevel::XHigh => ThinkingLevel::XHigh,
    }
}

pub fn apply_cli_thinking_level(parsed: &crate::Args, session: &mut AgentSession) {
    if let Some(level) = parsed.thinking.as_ref() {
        session.set_thinking_level(cli_thinking_level(level));
    }
}
