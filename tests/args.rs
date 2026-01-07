use pi::{parse_args, Args, ExtensionFlagType, ExtensionFlagValue, Mode, ThinkingLevel};
use std::collections::HashMap;

fn parse(input: &[&str]) -> Args {
    let args = input
        .iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>();
    parse_args(&args, None)
}

#[test]
fn parses_version_flags() {
    let result = parse(&["--version"]);
    assert!(result.version);

    let result = parse(&["-v"]);
    assert!(result.version);

    let result = parse(&["--version", "--help", "some message"]);
    assert!(result.version);
    assert!(result.help);
    assert!(result.messages.contains(&"some message".to_string()));
}

#[test]
fn parses_help_flags() {
    let result = parse(&["--help"]);
    assert!(result.help);

    let result = parse(&["-h"]);
    assert!(result.help);
}

#[test]
fn parses_print_flags() {
    let result = parse(&["--print"]);
    assert!(result.print);

    let result = parse(&["-p"]);
    assert!(result.print);
}

#[test]
fn parses_continue_flags() {
    let result = parse(&["--continue"]);
    assert!(result.continue_session);

    let result = parse(&["-c"]);
    assert!(result.continue_session);
}

#[test]
fn parses_resume_flags() {
    let result = parse(&["--resume"]);
    assert!(result.resume);

    let result = parse(&["-r"]);
    assert!(result.resume);
}

#[test]
fn parses_flags_with_values() {
    let result = parse(&["--provider", "openai"]);
    assert_eq!(result.provider.as_deref(), Some("openai"));

    let result = parse(&["--model", "gpt-4o"]);
    assert_eq!(result.model.as_deref(), Some("gpt-4o"));

    let result = parse(&["--api-key", "sk-test-key"]);
    assert_eq!(result.api_key.as_deref(), Some("sk-test-key"));

    let result = parse(&["--system-prompt", "You are a helpful assistant"]);
    assert_eq!(
        result.system_prompt.as_deref(),
        Some("You are a helpful assistant")
    );

    let result = parse(&["--append-system-prompt", "Additional context"]);
    assert_eq!(
        result.append_system_prompt.as_deref(),
        Some("Additional context")
    );

    let result = parse(&["--mode", "json"]);
    assert_eq!(result.mode, Some(Mode::Json));

    let result = parse(&["--mode", "rpc"]);
    assert_eq!(result.mode, Some(Mode::Rpc));

    let result = parse(&["--session", "/path/to/session.jsonl"]);
    assert_eq!(result.session.as_deref(), Some("/path/to/session.jsonl"));

    let result = parse(&["--export", "session.jsonl"]);
    assert_eq!(result.export.as_deref(), Some("session.jsonl"));

    let result = parse(&["--thinking", "high"]);
    assert_eq!(result.thinking, Some(ThinkingLevel::High));

    let result = parse(&["--models", "gpt-4o,claude-sonnet,gemini-pro"]);
    assert_eq!(
        result.models,
        Some(vec![
            "gpt-4o".to_string(),
            "claude-sonnet".to_string(),
            "gemini-pro".to_string()
        ])
    );
}

#[test]
fn parses_no_session_flag() {
    let result = parse(&["--no-session"]);
    assert!(result.no_session);
}

#[test]
fn parses_extension_flags() {
    let result = parse(&["--extension", "./my-extension.ts"]);
    assert_eq!(
        result.extensions,
        Some(vec!["./my-extension.ts".to_string()])
    );

    let result = parse(&["-e", "./ext1.ts", "--extension", "./ext2.ts"]);
    assert_eq!(
        result.extensions,
        Some(vec!["./ext1.ts".to_string(), "./ext2.ts".to_string()])
    );
}

#[test]
fn parses_messages_and_file_args() {
    let result = parse(&["hello", "world"]);
    assert_eq!(
        result.messages,
        vec!["hello".to_string(), "world".to_string()]
    );

    let result = parse(&["@README.md", "@src/main.ts"]);
    assert_eq!(
        result.file_args,
        vec!["README.md".to_string(), "src/main.ts".to_string()]
    );

    let result = parse(&["@file.txt", "explain this", "@image.png"]);
    assert_eq!(
        result.file_args,
        vec!["file.txt".to_string(), "image.png".to_string()]
    );
    assert_eq!(result.messages, vec!["explain this".to_string()]);

    let result = parse(&["--unknown-flag", "message"]);
    assert_eq!(result.messages, vec!["message".to_string()]);
}

#[test]
fn parses_extension_defined_flags() {
    let mut extension_flags = HashMap::new();
    extension_flags.insert("plan".to_string(), ExtensionFlagType::Bool);
    extension_flags.insert("profile".to_string(), ExtensionFlagType::String);

    let args = vec![
        "--plan".to_string(),
        "--profile".to_string(),
        "fast".to_string(),
        "message".to_string(),
    ];
    let parsed = parse_args(&args, Some(&extension_flags));

    assert_eq!(
        parsed.extension_flags.get("plan"),
        Some(&ExtensionFlagValue::Bool(true))
    );
    assert_eq!(
        parsed.extension_flags.get("profile"),
        Some(&ExtensionFlagValue::String("fast".to_string()))
    );
    assert_eq!(parsed.messages, vec!["message".to_string()]);
}

#[test]
fn parses_complex_combinations() {
    let result = parse(&[
        "--provider",
        "anthropic",
        "--model",
        "claude-sonnet",
        "--print",
        "--thinking",
        "high",
        "@prompt.md",
        "Do the task",
    ]);
    assert_eq!(result.provider.as_deref(), Some("anthropic"));
    assert_eq!(result.model.as_deref(), Some("claude-sonnet"));
    assert!(result.print);
    assert_eq!(result.thinking, Some(ThinkingLevel::High));
    assert_eq!(result.file_args, vec!["prompt.md".to_string()]);
    assert_eq!(result.messages, vec!["Do the task".to_string()]);
}
