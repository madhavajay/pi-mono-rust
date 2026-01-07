use crate::agent::{QueueMode, ThinkingLevel};
use crate::cli::file_inputs::FileInputImage;
use crate::cli::session::to_agent_model;
use crate::coding_agent::interactive_mode::format_message_for_interactive;
use crate::coding_agent::{
    available_themes, get_changelog_path, load_theme_or_default, parse_changelog,
    parse_model_pattern, set_active_theme, AgentSession,
};
use crate::core::messages::UserContent;
use crate::tui::{truncate_to_width, wrap_text_with_ansi, Editor};
use std::io::{self, Write};
use std::path::PathBuf;
use std::process;

use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;

use super::build_user_content_from_files;

struct TerminalGuard;

impl TerminalGuard {
    fn enter(stdout: &mut impl Write) -> Result<Self, String> {
        terminal::enable_raw_mode().map_err(|err| err.to_string())?;
        stdout
            .execute(EnterAlternateScreen)
            .map_err(|err| err.to_string())?;
        stdout.execute(Hide).map_err(|err| err.to_string())?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = stdout.execute(LeaveAlternateScreen);
        let _ = stdout.execute(Show);
    }
}

enum EditorAction {
    Submit,
    Exit,
    Continue,
}

fn render_interactive_ui(
    entries: &[String],
    editor: &mut Editor,
    stdout: &mut impl Write,
) -> Result<(), String> {
    let (width, height) = terminal::size().map_err(|err| err.to_string())?;
    let width = width.max(1) as usize;
    let height = height.max(1) as usize;

    let mut chat_lines = Vec::new();
    for (idx, entry) in entries.iter().enumerate() {
        chat_lines.extend(wrap_text_with_ansi(entry, width));
        if idx + 1 < entries.len() {
            chat_lines.push(String::new());
        }
    }
    if chat_lines.is_empty() {
        chat_lines.push(String::new());
    }

    let editor_lines = editor.render(width);
    let available_chat = height.saturating_sub(editor_lines.len());
    let start = chat_lines.len().saturating_sub(available_chat);
    let mut visible_chat = chat_lines[start..].to_vec();
    while visible_chat.len() < available_chat {
        visible_chat.push(String::new());
    }

    let mut lines = Vec::new();
    lines.extend(visible_chat);
    lines.extend(editor_lines);
    if lines.len() > height {
        lines.truncate(height);
    }

    stdout
        .execute(MoveTo(0, 0))
        .map_err(|err| err.to_string())?;
    stdout
        .execute(Clear(ClearType::All))
        .map_err(|err| err.to_string())?;

    for (index, line) in lines.iter().enumerate() {
        let truncated = truncate_to_width(line, width);
        if index + 1 == lines.len() {
            write!(stdout, "{truncated}").map_err(|err| err.to_string())?;
        } else {
            // Use \r\n to ensure we start at column 0 on next line
            write!(stdout, "{truncated}\r\n").map_err(|err| err.to_string())?;
        }
    }
    stdout.flush().map_err(|err| err.to_string())?;
    Ok(())
}

fn build_user_entry(message: Option<&str>, images: &[FileInputImage]) -> String {
    let mut lines = Vec::new();
    if let Some(message) = message {
        if !message.trim().is_empty() {
            lines.push(message.to_string());
        }
    }
    for _ in images {
        lines.push("[image attachment]".to_string());
    }
    if lines.is_empty() {
        "[empty message]".to_string()
    } else {
        lines.join("\n")
    }
}

fn prompt_and_append_text(
    session: &mut AgentSession,
    entries: &mut Vec<String>,
    editor: &mut Editor,
    stdout: &mut impl Write,
    prompt: &str,
) -> Result<(), String> {
    let start_index = session.messages().len();
    entries.push(format!("You:\n{prompt}"));
    entries.push("Assistant:\n...".to_string());
    render_interactive_ui(entries, editor, stdout)?;

    if let Err(err) = session.prompt(prompt) {
        let last = entries.len().saturating_sub(1);
        if let Some(entry) = entries.get_mut(last) {
            *entry = format!("Assistant:\nError: {}", err);
        }
        render_interactive_ui(entries, editor, stdout)?;
        return Err(err.to_string());
    }

    let new_entries = collect_new_interactive_entries(session, start_index);
    entries.pop();
    if new_entries.is_empty() {
        entries.push("Assistant:\n[no response]".to_string());
    } else {
        entries.extend(new_entries);
    }
    render_interactive_ui(entries, editor, stdout)?;
    Ok(())
}

fn prompt_and_append_content(
    session: &mut AgentSession,
    entries: &mut Vec<String>,
    editor: &mut Editor,
    stdout: &mut impl Write,
    prompt: &str,
    content: UserContent,
) -> Result<(), String> {
    let start_index = session.messages().len();
    entries.push(format!("You:\n{prompt}"));
    entries.push("Assistant:\n...".to_string());
    render_interactive_ui(entries, editor, stdout)?;

    if let Err(err) = session.prompt_content(content) {
        let last = entries.len().saturating_sub(1);
        if let Some(entry) = entries.get_mut(last) {
            *entry = format!("Assistant:\nError: {}", err);
        }
        render_interactive_ui(entries, editor, stdout)?;
        return Err(err.to_string());
    }

    let new_entries = collect_new_interactive_entries(session, start_index);
    entries.pop();
    if new_entries.is_empty() {
        entries.push("Assistant:\n[no response]".to_string());
    } else {
        entries.extend(new_entries);
    }
    render_interactive_ui(entries, editor, stdout)?;
    Ok(())
}

fn collect_new_interactive_entries(session: &AgentSession, start_index: usize) -> Vec<String> {
    let messages = session.messages();
    let mut entries = Vec::new();
    let hide_thinking = session.settings_manager.get_hide_thinking_block();
    let show_images = session.settings_manager.get_show_images();
    for message in messages.iter().skip(start_index) {
        if let Some(entry) =
            format_message_for_interactive(message, false, hide_thinking, show_images)
        {
            entries.push(entry);
        }
    }
    entries
}

fn rebuild_interactive_entries(session: &AgentSession, include_user: bool) -> Vec<String> {
    let mut entries = Vec::new();
    let messages = session.messages();
    let hide_thinking = session.settings_manager.get_hide_thinking_block();
    let show_images = session.settings_manager.get_show_images();
    for message in messages.iter() {
        if let Some(entry) =
            format_message_for_interactive(message, include_user, hide_thinking, show_images)
        {
            entries.push(entry);
        }
    }
    entries
}

fn parse_thinking_level_value(value: &str) -> Option<ThinkingLevel> {
    match value {
        "off" => Some(ThinkingLevel::Off),
        "minimal" => Some(ThinkingLevel::Minimal),
        "low" => Some(ThinkingLevel::Low),
        "medium" => Some(ThinkingLevel::Medium),
        "high" => Some(ThinkingLevel::High),
        "xhigh" => Some(ThinkingLevel::XHigh),
        _ => None,
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    match value {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn parse_queue_mode(value: &str) -> Option<QueueMode> {
    match value {
        "all" => Some(QueueMode::All),
        "one-at-a-time" => Some(QueueMode::OneAtATime),
        _ => None,
    }
}

fn extract_thinking_level_suffix(pattern: &str) -> Option<ThinkingLevel> {
    let idx = pattern.rfind(':')?;
    parse_thinking_level_value(pattern[idx + 1..].trim())
}

fn sort_models_for_display(
    models: &[crate::coding_agent::Model],
    current: &crate::agent::Model,
) -> Vec<crate::coding_agent::Model> {
    let mut choices = models.to_vec();
    choices.sort_by(|a, b| {
        let a_current = a.provider == current.provider && a.id == current.id;
        let b_current = b.provider == current.provider && b.id == current.id;
        if a_current && !b_current {
            return std::cmp::Ordering::Less;
        }
        if !a_current && b_current {
            return std::cmp::Ordering::Greater;
        }
        a.provider.cmp(&b.provider).then_with(|| a.id.cmp(&b.id))
    });
    choices
}

fn format_model_overview(
    models: &[crate::coding_agent::Model],
    current: &crate::agent::Model,
) -> String {
    if models.is_empty() {
        return "No models available. Set an API key in auth.json or env.".to_string();
    }

    let choices = sort_models_for_display(models, current);

    let mut lines = Vec::new();
    lines.push(format!(
        "Current model: {}/{}",
        current.provider, current.id
    ));
    lines.push("Available models:".to_string());
    for (idx, model) in choices.iter().enumerate() {
        let current_marker = model.provider == current.provider && model.id == current.id;
        let label = if model.name.is_empty() || model.name == model.id {
            format!("{}/{}", model.provider, model.id)
        } else {
            format!("{}/{} ({})", model.provider, model.id, model.name)
        };
        let marker = if current_marker { "*" } else { " " };
        lines.push(format!("{marker} {:>2}) {label}", idx + 1));
    }
    lines.push("Use /model <pattern> or /model <number> to select.".to_string());
    lines.push("Patterns accept provider/model and optional :thinking suffix.".to_string());
    lines.join("\n")
}

fn format_settings_overview(session: &AgentSession) -> String {
    let theme = session
        .settings_manager
        .get_theme()
        .unwrap_or_else(|| "dark".to_string());
    let thinking_level = session.agent.state().thinking_level.as_str();
    let available = available_themes().join(", ");

    let mut lines = Vec::new();
    lines.push("Current settings:".to_string());
    lines.push(format!(
        "autocompact: {}",
        session.settings_manager.get_compaction_enabled()
    ));
    lines.push(format!(
        "show-images: {}",
        session.settings_manager.get_show_images()
    ));
    lines.push(format!(
        "auto-resize-images: {}",
        session.settings_manager.get_image_auto_resize()
    ));
    lines.push(format!(
        "steering-mode: {}",
        session.settings_manager.get_steering_mode()
    ));
    lines.push(format!(
        "follow-up-mode: {}",
        session.settings_manager.get_follow_up_mode()
    ));
    lines.push(format!("thinking-level: {thinking_level}"));
    lines.push(format!("theme: {theme}"));
    lines.push(format!(
        "hide-thinking: {}",
        session.settings_manager.get_hide_thinking_block()
    ));
    lines.push(format!(
        "collapse-changelog: {}",
        session.settings_manager.get_collapse_changelog()
    ));
    lines.push(format!(
        "double-escape-action: {}",
        session.settings_manager.get_double_escape_action()
    ));
    lines.push(format!("Available themes: {available}"));
    lines.push("Usage: /settings <key> <value>".to_string());
    lines.push(
        "Keys: autocompact, show-images, auto-resize-images, steering-mode, follow-up-mode, thinking-level, theme, hide-thinking, collapse-changelog, double-escape-action"
            .to_string(),
    );
    lines.join("\n")
}

fn append_status_entry(entries: &mut Vec<String>, message: &str) {
    entries.push(format!("Status:\n{message}"));
}

fn ensure_gh_available() -> Result<(), String> {
    match process::Command::new("gh")
        .args(["auth", "status"])
        .output()
    {
        Ok(output) => {
            if output.status.success() {
                Ok(())
            } else {
                Err("GitHub CLI is not logged in. Run 'gh auth login' first.".to_string())
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Err(
            "GitHub CLI (gh) is not installed. Install it from https://cli.github.com/".to_string(),
        ),
        Err(err) => Err(format!("Failed to run GitHub CLI: {err}")),
    }
}

fn create_share_links(session: &AgentSession) -> Result<(String, String), String> {
    ensure_gh_available()?;
    let tmp_path = std::env::temp_dir().join(format!("pi-session-{}.html", now_millis()));
    if let Err(err) = session.export_to_html_with_path(Some(&tmp_path)) {
        return Err(format!("Failed to export session: {err}"));
    }

    let output = process::Command::new("gh")
        .args(["gist", "create", "--public=false"])
        .arg(&tmp_path)
        .output()
        .map_err(|err| format!("Failed to create gist: {err}"))?;
    let _ = std::fs::remove_file(&tmp_path);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let message = stderr.trim();
        let message = if message.is_empty() {
            stdout.trim()
        } else {
            message
        };
        let message = if message.is_empty() {
            "Unknown error"
        } else {
            message
        };
        return Err(format!("Failed to create gist: {message}"));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let gist_url = stdout
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(|line| line.trim().to_string())
        .unwrap_or_default();
    if gist_url.is_empty() {
        return Err("Failed to parse gist URL from gh output".to_string());
    }
    let gist_id = gist_url.rsplit('/').next().unwrap_or("");
    if gist_id.is_empty() {
        return Err("Failed to parse gist ID from gh output".to_string());
    }
    let preview_url = format!("https://shittycodingagent.ai/session?{gist_id}");
    Ok((preview_url, gist_url))
}

fn handle_key_event(key: KeyEvent, editor: &mut Editor) -> EditorAction {
    match key.code {
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            return EditorAction::Exit;
        }
        KeyCode::Enter => {
            if key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT)
            {
                editor.handle_input("\n");
            } else {
                return EditorAction::Submit;
            }
        }
        KeyCode::Backspace => {
            if key.modifiers.contains(KeyModifiers::ALT) {
                editor.handle_input("\x1b\x7f");
            } else {
                editor.handle_input("\x7f");
            }
        }
        KeyCode::Up => {
            editor.handle_input("\x1b[A");
        }
        KeyCode::Down => {
            editor.handle_input("\x1b[B");
        }
        KeyCode::Left => {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                editor.handle_input("\x1b[1;5D");
            } else {
                editor.handle_input("\x1b[D");
            }
        }
        KeyCode::Right => {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                editor.handle_input("\x1b[1;5C");
            } else {
                editor.handle_input("\x1b[C");
            }
        }
        KeyCode::Tab => {
            editor.handle_input("\t");
        }
        KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            editor.handle_input("\x01");
        }
        KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            editor.handle_input("\x17");
        }
        KeyCode::Char(ch) => {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                return EditorAction::Continue;
            }
            editor.handle_input(&ch.to_string());
        }
        _ => {}
    }
    EditorAction::Continue
}

pub fn run_interactive_mode_session(
    session: &mut AgentSession,
    messages: &[String],
    initial_message: Option<String>,
    initial_images: &[FileInputImage],
) -> Result<(), String> {
    let mut entries = Vec::new();
    let theme = load_theme_or_default(session.settings_manager.get_theme().as_deref());
    set_active_theme(theme.clone());
    let mut editor = Editor::new(theme.editor_theme());

    let mut stdout = io::stdout();
    let _guard = TerminalGuard::enter(&mut stdout)?;

    if initial_message.is_some() || !initial_images.is_empty() {
        let prompt = build_user_entry(initial_message.as_deref(), initial_images);
        let content = build_user_content_from_files(initial_message.as_deref(), initial_images)?;
        prompt_and_append_content(
            session,
            &mut entries,
            &mut editor,
            &mut stdout,
            &prompt,
            content,
        )?;
    }

    for message in messages {
        if message.trim().is_empty() {
            continue;
        }
        prompt_and_append_text(session, &mut entries, &mut editor, &mut stdout, message)?;
    }

    render_interactive_ui(&entries, &mut editor, &mut stdout)?;

    loop {
        match event::read().map_err(|err| err.to_string())? {
            Event::Key(key) => match handle_key_event(key, &mut editor) {
                EditorAction::Exit => break,
                EditorAction::Submit => {
                    let text = editor.get_text();
                    let prompt = text.trim_end().to_string();
                    let trimmed = prompt.trim();
                    editor.set_text("");
                    if trimmed.is_empty() {
                        render_interactive_ui(&entries, &mut editor, &mut stdout)?;
                        continue;
                    }
                    if trimmed.starts_with("/export") {
                        let rest = trimmed.trim_start_matches("/export").trim();
                        let output_path = if rest.is_empty() { None } else { Some(rest) };
                        let output_path = output_path.map(PathBuf::from);
                        match session.export_to_html_with_path(output_path.as_ref()) {
                            Ok(result) => append_status_entry(
                                &mut entries,
                                &format!("Session exported to: {}", result.path.display()),
                            ),
                            Err(err) => append_status_entry(
                                &mut entries,
                                &format!("Failed to export session: {err}"),
                            ),
                        }
                        render_interactive_ui(&entries, &mut editor, &mut stdout)?;
                        continue;
                    }
                    if trimmed.starts_with("/compact") {
                        let rest = trimmed.trim_start_matches("/compact").trim();
                        let custom_instructions = if rest.is_empty() { None } else { Some(rest) };
                        if session.messages().len() < 2 {
                            append_status_entry(
                                &mut entries,
                                "Nothing to compact (no messages yet)",
                            );
                            render_interactive_ui(&entries, &mut editor, &mut stdout)?;
                            continue;
                        }
                        match session.compact_with_instructions(custom_instructions) {
                            Ok(result) => {
                                entries = rebuild_interactive_entries(session, true);
                                append_status_entry(
                                    &mut entries,
                                    &format!(
                                        "Compaction complete (tokens before: {})",
                                        result.tokens_before
                                    ),
                                );
                            }
                            Err(err) => append_status_entry(
                                &mut entries,
                                &format!("Compaction failed: {err}"),
                            ),
                        }
                        render_interactive_ui(&entries, &mut editor, &mut stdout)?;
                        continue;
                    }
                    if trimmed.starts_with("/share") {
                        match create_share_links(session) {
                            Ok((preview_url, gist_url)) => append_status_entry(
                                &mut entries,
                                &format!("Share URL: {preview_url}\nGist: {gist_url}"),
                            ),
                            Err(err) => append_status_entry(&mut entries, &err),
                        }
                        render_interactive_ui(&entries, &mut editor, &mut stdout)?;
                        continue;
                    }
                    if trimmed.starts_with("/model") {
                        let rest = trimmed.trim_start_matches("/model").trim();
                        let available = session.get_available_models();
                        let current_model = session.agent.state().model;
                        if rest.is_empty() {
                            append_status_entry(
                                &mut entries,
                                &format_model_overview(&available, &current_model),
                            );
                            render_interactive_ui(&entries, &mut editor, &mut stdout)?;
                            continue;
                        }
                        if available.is_empty() {
                            append_status_entry(
                                &mut entries,
                                "No models available. Set an API key in auth.json or env.",
                            );
                            render_interactive_ui(&entries, &mut editor, &mut stdout)?;
                            continue;
                        }

                        let (selected, warning) = if let Ok(index) = rest.parse::<usize>() {
                            let choices = sort_models_for_display(&available, &current_model);
                            if index > 0 && index <= choices.len() {
                                (Some(choices[index - 1].clone()), None)
                            } else {
                                append_status_entry(
                                    &mut entries,
                                    "Model index out of range. Run /model to see options.",
                                );
                                render_interactive_ui(&entries, &mut editor, &mut stdout)?;
                                continue;
                            }
                        } else {
                            let parsed = parse_model_pattern(rest, &available);
                            if parsed.model.is_none() {
                                append_status_entry(
                                    &mut entries,
                                    "No model matched that pattern. Run /model to see options.",
                                );
                                render_interactive_ui(&entries, &mut editor, &mut stdout)?;
                                continue;
                            }
                            (parsed.model, parsed.warning)
                        };

                        let selected = selected.expect("model should be selected");
                        session.set_model(to_agent_model(&selected));
                        session
                            .settings_manager
                            .set_default_model_and_provider(&selected.provider, &selected.id);
                        let mut message =
                            format!("Model set to {}/{}", selected.provider, selected.id);
                        if let Some(level) = extract_thinking_level_suffix(rest) {
                            session.set_thinking_level(level);
                            message.push_str(&format!(" (thinking: {})", level.as_str()));
                        }
                        append_status_entry(&mut entries, &message);
                        if let Some(warning) = warning {
                            append_status_entry(&mut entries, &warning);
                        }
                        render_interactive_ui(&entries, &mut editor, &mut stdout)?;
                        continue;
                    }
                    if trimmed.starts_with("/settings") {
                        let rest = trimmed.trim_start_matches("/settings").trim();
                        if rest.is_empty() {
                            append_status_entry(&mut entries, &format_settings_overview(session));
                            render_interactive_ui(&entries, &mut editor, &mut stdout)?;
                            continue;
                        }
                        let mut parts = rest.split_whitespace();
                        let key = match parts.next() {
                            Some(key) => key.to_ascii_lowercase(),
                            None => {
                                append_status_entry(&mut entries, "Usage: /settings <key> <value>");
                                render_interactive_ui(&entries, &mut editor, &mut stdout)?;
                                continue;
                            }
                        };
                        let value = match parts.next() {
                            Some(value) => value,
                            None => {
                                append_status_entry(&mut entries, "Usage: /settings <key> <value>");
                                render_interactive_ui(&entries, &mut editor, &mut stdout)?;
                                continue;
                            }
                        };

                        let mut rebuild = false;
                        let mut error = None;
                        match key.as_str() {
                            "autocompact" => match parse_bool(value) {
                                Some(enabled) => {
                                    session.settings_manager.set_compaction_enabled(enabled);
                                }
                                None => error = Some("Expected true/false for autocompact."),
                            },
                            "show-images" => match parse_bool(value) {
                                Some(enabled) => {
                                    session.settings_manager.set_show_images(enabled);
                                    rebuild = true;
                                }
                                None => error = Some("Expected true/false for show-images."),
                            },
                            "auto-resize-images" => match parse_bool(value) {
                                Some(enabled) => {
                                    session.settings_manager.set_image_auto_resize(enabled);
                                }
                                None => error = Some("Expected true/false for auto-resize-images."),
                            },
                            "steering-mode" => {
                                if let Some(mode) = parse_queue_mode(value) {
                                    session.set_steering_mode(mode);
                                    session.settings_manager.set_steering_mode(value);
                                } else {
                                    error = Some("steering-mode must be 'all' or 'one-at-a-time'.");
                                }
                            }
                            "follow-up-mode" => {
                                if let Some(mode) = parse_queue_mode(value) {
                                    session.set_follow_up_mode(mode);
                                    session.settings_manager.set_follow_up_mode(value);
                                } else {
                                    error =
                                        Some("follow-up-mode must be 'all' or 'one-at-a-time'.");
                                }
                            }
                            "thinking-level" => match parse_thinking_level_value(value) {
                                Some(level) => session.set_thinking_level(level),
                                None => {
                                    error = Some(
                                        "thinking-level must be off/minimal/low/medium/high/xhigh.",
                                    );
                                }
                            },
                            "theme" => {
                                let themes = available_themes();
                                if !themes.iter().any(|theme| theme == value) {
                                    error = Some("Unknown theme name. Run /settings to list.");
                                } else {
                                    session.settings_manager.set_theme(value);
                                    let theme = load_theme_or_default(Some(value));
                                    set_active_theme(theme.clone());
                                    editor.set_theme(theme.editor_theme());
                                }
                            }
                            "hide-thinking" => match parse_bool(value) {
                                Some(enabled) => {
                                    session.settings_manager.set_hide_thinking_block(enabled);
                                    rebuild = true;
                                }
                                None => error = Some("Expected true/false for hide-thinking."),
                            },
                            "collapse-changelog" => match parse_bool(value) {
                                Some(enabled) => {
                                    session.settings_manager.set_collapse_changelog(enabled);
                                }
                                None => error = Some("Expected true/false for collapse-changelog."),
                            },
                            "double-escape-action" => {
                                if matches!(value, "tree" | "branch") {
                                    session.settings_manager.set_double_escape_action(value);
                                } else {
                                    error =
                                        Some("double-escape-action must be 'tree' or 'branch'.");
                                }
                            }
                            _ => error = Some("Unknown settings key. Run /settings to list."),
                        }

                        if let Some(error) = error {
                            append_status_entry(&mut entries, error);
                            render_interactive_ui(&entries, &mut editor, &mut stdout)?;
                            continue;
                        }
                        if rebuild {
                            entries = rebuild_interactive_entries(session, true);
                        }
                        append_status_entry(&mut entries, &format!("Updated {} to {}", key, value));
                        render_interactive_ui(&entries, &mut editor, &mut stdout)?;
                        continue;
                    }
                    if trimmed == "/changelog" {
                        let message = match get_changelog_path() {
                            Some(path) => {
                                let mut changelog_entries = parse_changelog(&path);
                                if changelog_entries.is_empty() {
                                    format!("No changelog entries found at {}.", path.display())
                                } else {
                                    changelog_entries.reverse();
                                    let content = changelog_entries
                                        .into_iter()
                                        .map(|entry| entry.content)
                                        .collect::<Vec<_>>()
                                        .join("\n\n");
                                    format!("Changelog ({}):\n{content}", path.display())
                                }
                            }
                            None => "No CHANGELOG.md found.".to_string(),
                        };
                        append_status_entry(&mut entries, &message);
                        render_interactive_ui(&entries, &mut editor, &mut stdout)?;
                        continue;
                    }
                    if trimmed == "/hotkeys" {
                        let hotkeys = [
                            "Enter: send message",
                            "Ctrl/Alt/Shift+Enter: new line",
                            "Ctrl+C: exit",
                            "Arrow keys: move cursor / history",
                            "Ctrl+Left/Right: move by word",
                            "Ctrl+A: start of line",
                            "Ctrl+W or Alt+Backspace: delete word",
                            "Tab: insert tab",
                            "/ commands: /export /compact /share /model /settings /changelog /hotkeys",
                        ]
                        .join("\n");
                        append_status_entry(&mut entries, &hotkeys);
                        render_interactive_ui(&entries, &mut editor, &mut stdout)?;
                        continue;
                    }
                    if matches!(trimmed, "/exit" | "/quit") {
                        break;
                    }
                    editor.add_to_history(&prompt);
                    prompt_and_append_text(
                        session,
                        &mut entries,
                        &mut editor,
                        &mut stdout,
                        &prompt,
                    )?
                }
                EditorAction::Continue => {
                    render_interactive_ui(&entries, &mut editor, &mut stdout)?;
                }
            },
            Event::Resize(_, _) => {
                render_interactive_ui(&entries, &mut editor, &mut stdout)?;
            }
            _ => {}
        }
    }

    Ok(())
}

fn now_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}
