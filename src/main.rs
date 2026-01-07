use pi::cli::file_inputs::build_file_inputs;
use pi::cli::list_models::list_models;
use pi::cli::runtime::{
    attach_extensions_with_host, build_model_registry, build_session_manager,
    collect_extension_tools, collect_unsupported_flags, discover_system_prompt_file,
    extension_flag_values_to_json, preload_extensions, print_help, select_model,
    select_resume_session,
};
use pi::cli::session::{apply_cli_thinking_level, create_cli_session, create_rpc_session};
use pi::coding_agent::{build_system_prompt, export_from_file, BuildSystemPromptOptions};
use pi::config;
use pi::modes::{run_interactive_mode_session, run_print_mode_session};
use pi::rpc::run_rpc_mode;
use pi::{parse_args, ListModels, Mode};
use std::env;
use std::path::{Path, PathBuf};
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let first_pass = parse_args(&args, None);

    let cwd = match env::current_dir() {
        Ok(cwd) => cwd,
        Err(err) => {
            eprintln!("Error: Failed to read cwd: {err}");
            process::exit(1);
        }
    };

    let (mut preloaded_extension, extension_flag_types) = preload_extensions(&first_pass, &cwd);

    let parsed = parse_args(&args, Some(&extension_flag_types));
    if let Some(preloaded) = preloaded_extension.as_ref() {
        let flag_values = extension_flag_values_to_json(&parsed.extension_flags);
        if let Err(err) = preloaded.host.borrow_mut().set_flag_values(&flag_values) {
            eprintln!("Warning: Failed to apply extension flags: {err}");
        }
    }

    if parsed.version {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return;
    }

    if parsed.help {
        print_help();
        return;
    }

    if let Some(list_models_mode) = &parsed.list_models {
        let registry = match build_model_registry(None, None) {
            Ok(registry) => registry,
            Err(message) => {
                eprintln!("Error: {message}");
                process::exit(1);
            }
        };
        let search_pattern = match list_models_mode {
            ListModels::All => None,
            ListModels::Pattern(pattern) => Some(pattern.as_str()),
        };
        list_models(&registry, search_pattern);
        return;
    }

    if let Some(export_path) = &parsed.export {
        let output_path = parsed.messages.first().map(PathBuf::from);
        match export_from_file(Path::new(export_path), output_path) {
            Ok(path) => {
                println!("Exported to: {}", path.display());
                return;
            }
            Err(message) => {
                eprintln!("Error: {message}");
                process::exit(1);
            }
        }
    }

    let unsupported = collect_unsupported_flags(&parsed);
    if !unsupported.is_empty() {
        eprintln!(
            "Error: unsupported flag(s) in rust CLI: {}",
            unsupported.join(", ")
        );
        process::exit(1);
    }
    let is_interactive = !parsed.print && parsed.mode.is_none();

    let mode = parsed.mode.clone().unwrap_or(Mode::Text);

    let provider = parsed.provider.as_deref().unwrap_or("anthropic");
    let supported_providers = [
        "anthropic",
        "openai",
        "openai-codex",
        "google-gemini-cli",
        "google-antigravity",
    ];
    if !supported_providers.contains(&provider) {
        eprintln!(
            "Error: unsupported provider \"{provider}\". Supported providers: {}",
            supported_providers.join(", ")
        );
        process::exit(1);
    }

    let registry = match build_model_registry(parsed.api_key.as_deref(), Some(provider)) {
        Ok(registry) => registry,
        Err(message) => {
            eprintln!("Error: {message}");
            process::exit(1);
        }
    };

    let model = match select_model(&parsed, &registry) {
        Ok(model) => model,
        Err(message) => {
            eprintln!("Error: {message}");
            process::exit(1);
        }
    };

    let supported_apis = [
        "anthropic-messages",
        "openai-responses",
        "openai-codex-responses",
        "google-gemini-cli",
    ];
    if !supported_apis.contains(&model.api.as_str()) {
        eprintln!(
            "Error: unsupported model API \"{}\". Supported APIs: {}",
            model.api,
            supported_apis.join(", ")
        );
        process::exit(1);
    }

    let system_prompt_source = if parsed.system_prompt.is_some() {
        parsed.system_prompt.clone()
    } else {
        discover_system_prompt_file().map(|path| path.to_string_lossy().to_string())
    };
    let skill_patterns = parsed.skills.clone().unwrap_or_default();
    let extension_tools = preloaded_extension
        .as_ref()
        .map(|preloaded| collect_extension_tools(&preloaded.manifest))
        .unwrap_or_default();
    let extension_host = preloaded_extension
        .as_ref()
        .map(|preloaded| preloaded.host.clone());
    let mut selected_tools = parsed
        .tools
        .clone()
        .unwrap_or_else(pi::tools::default_tool_names);
    if parsed.tools.is_none() {
        for tool in &extension_tools {
            selected_tools.push(tool.name.clone());
        }
    }
    let system_prompt = build_system_prompt(BuildSystemPromptOptions {
        custom_prompt: system_prompt_source,
        append_system_prompt: parsed.append_system_prompt.clone(),
        selected_tools: Some(selected_tools.clone()),
        skills_enabled: !parsed.no_skills,
        skills_include: skill_patterns,
        cwd: Some(cwd.clone()),
        agent_dir: Some(config::get_agent_dir()),
        ..Default::default()
    });
    let session_manager = if parsed.resume {
        match select_resume_session(&cwd, parsed.session_dir.as_deref()) {
            Ok(Some(path)) => pi::core::session_manager::SessionManager::open(path, None),
            Ok(None) => return,
            Err(message) => {
                eprintln!("Error: {message}");
                process::exit(1);
            }
        }
    } else {
        build_session_manager(&parsed, &cwd)
    };

    if matches!(mode, Mode::Rpc) {
        if !parsed.file_args.is_empty() {
            eprintln!("Error: @file arguments are not supported in RPC mode.");
            process::exit(1);
        }
        if model.api != "anthropic-messages" && model.api != "openai-responses" {
            eprintln!(
                "Error: RPC mode currently supports only \"anthropic-messages\" and \"openai-responses\" models."
            );
            process::exit(1);
        }
        let mut session = match create_rpc_session(
            model,
            registry,
            Some(system_prompt),
            None,
            Some(selected_tools.as_slice()),
            &extension_tools,
            extension_host.clone(),
            parsed.api_key.as_deref(),
            session_manager,
        ) {
            Ok(session) => session,
            Err(message) => {
                eprintln!("Error: {message}");
                process::exit(1);
            }
        };
        if let Some(paths) = parsed.extensions.as_deref() {
            session.settings_manager.set_extension_paths(paths.to_vec());
        }
        apply_cli_thinking_level(&parsed, &mut session);
        attach_extensions_with_host(&mut session, &cwd, preloaded_extension.take());
        if let Err(message) = run_rpc_mode(session) {
            eprintln!("Error: {message}");
            process::exit(1);
        }
        return;
    }

    let mut messages = parsed.messages.clone();
    let mut initial_message = None;
    let mut initial_images = Vec::new();
    if !parsed.file_args.is_empty() {
        let inputs = match build_file_inputs(&parsed.file_args) {
            Ok(inputs) => inputs,
            Err(message) => {
                eprintln!("{message}");
                process::exit(1);
            }
        };
        if !inputs.text_prefix.is_empty() || !inputs.images.is_empty() {
            initial_message = if messages.is_empty() {
                Some(inputs.text_prefix)
            } else {
                let first = messages.remove(0);
                Some(format!("{}{}", inputs.text_prefix, first))
            };
            if !inputs.images.is_empty() {
                initial_images = inputs.images;
            }
        }
    }

    let mut session = match create_cli_session(
        model,
        registry,
        Some(system_prompt),
        None,
        Some(selected_tools.as_slice()),
        &extension_tools,
        extension_host.clone(),
        parsed.api_key.as_deref(),
        session_manager,
    ) {
        Ok(session) => session,
        Err(message) => {
            eprintln!("Error: {message}");
            process::exit(1);
        }
    };
    if let Some(paths) = parsed.extensions.as_deref() {
        session.settings_manager.set_extension_paths(paths.to_vec());
    }
    apply_cli_thinking_level(&parsed, &mut session);
    attach_extensions_with_host(&mut session, &cwd, preloaded_extension.take());

    let result = if is_interactive {
        run_interactive_mode_session(&mut session, &messages, initial_message, &initial_images)
    } else {
        run_print_mode_session(
            mode,
            &mut session,
            &messages,
            initial_message,
            &initial_images,
        )
    };

    if let Err(message) = result {
        eprintln!("Error: {message}");
        process::exit(1);
    }
}
