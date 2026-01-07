use crate::agent::{QueueMode, ThinkingLevel};
use crate::cli::file_inputs::FileInputImage;
use crate::cli::session::to_agent_model;
use crate::coding_agent::interactive_mode::format_message_for_interactive;
use crate::coding_agent::{
    anthropic_exchange_code, anthropic_get_auth_url, available_themes, get_changelog_path,
    get_oauth_providers, load_theme_or_default, open_browser, openai_codex_get_auth_url,
    openai_codex_login_with_input, parse_changelog, parse_model_pattern, set_active_theme,
    AgentSession, AuthCredential, BranchCandidate, OAuthCallbackServer,
};
use crate::core::messages::UserContent;
use crate::core::session_manager::SessionManager;
use crate::tui::{
    bool_values, double_escape_action_values, matches_key, queue_mode_values,
    thinking_level_values, truncate_to_width, wrap_text_with_ansi, CombinedAutocompleteProvider,
    Editor, LoginDialogComponent, LoginDialogResult, ModelItem, ModelSelectorComponent,
    ModelSelectorResult, OAuthSelectorComponent, OAuthSelectorMode, OAuthSelectorResult,
    SessionSelectorComponent, SettingItem, SettingValue, SettingsSelectorComponent,
    SettingsSelectorResult, SlashCommand, TreeSelectorComponent,
};
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
    PasteImage,
}

/// Modal UI state for selectors
enum ModalState {
    None,
    TreeSelector(TreeSelectorState),
    SessionSelector(SessionSelectorState),
    BranchSelector(BranchSelectorState),
    ModelSelector(ModelSelectorState),
    SettingsSelector(SettingsSelectorState),
    OAuthSelector(OAuthSelectorState),
    LoginDialog(LoginDialogSelectorState),
}

struct TreeSelectorState {
    selector: TreeSelectorComponent,
    result: Option<TreeSelectorResult>,
}

enum TreeSelectorResult {
    Selected(String),
    Cancelled,
}

struct SessionSelectorState {
    selector: SessionSelectorComponent,
    result: Option<SessionSelectorResult>,
}

enum SessionSelectorResult {
    Selected(PathBuf),
    Cancelled,
}

struct BranchSelectorState {
    candidates: Vec<BranchCandidate>,
    selected_index: usize,
    search_query: String,
    filtered_indices: Vec<usize>,
    result: Option<BranchSelectorResult>,
}

impl BranchSelectorState {
    fn new(candidates: Vec<BranchCandidate>) -> Self {
        let filtered_indices = (0..candidates.len()).collect();
        Self {
            candidates,
            selected_index: 0,
            search_query: String::new(),
            filtered_indices,
            result: None,
        }
    }

    fn filter(&mut self) {
        let query = self.search_query.to_lowercase();
        if query.is_empty() {
            self.filtered_indices = (0..self.candidates.len()).collect();
        } else {
            self.filtered_indices = self
                .candidates
                .iter()
                .enumerate()
                .filter(|(_, c)| c.text.to_lowercase().contains(&query))
                .map(|(i, _)| i)
                .collect();
        }
        if self.selected_index >= self.filtered_indices.len() {
            self.selected_index = self.filtered_indices.len().saturating_sub(1);
        }
    }

    fn handle_input(&mut self, key_data: &str) {
        if matches_key(key_data, "up") {
            if self.selected_index > 0 {
                self.selected_index -= 1;
            }
        } else if matches_key(key_data, "down") {
            if self.selected_index + 1 < self.filtered_indices.len() {
                self.selected_index += 1;
            }
        } else if matches_key(key_data, "enter") {
            if let Some(&idx) = self.filtered_indices.get(self.selected_index) {
                let entry_id = self.candidates[idx].entry_id.clone();
                self.result = Some(BranchSelectorResult::Selected(entry_id));
            }
        } else if matches_key(key_data, "escape") {
            self.result = Some(BranchSelectorResult::Cancelled);
        } else if matches_key(key_data, "backspace") {
            self.search_query.pop();
            self.filter();
        } else if key_data.len() == 1 {
            let ch = key_data.chars().next().unwrap();
            if ch.is_ascii_graphic() || ch == ' ' {
                self.search_query.push(ch);
                self.filter();
            }
        }
    }

    fn render(&self, width: usize) -> Vec<String> {
        let mut lines = vec![
            "─".repeat(width.min(80)),
            "  Branch from Message".to_string(),
            "  Select a message to create a new branch from that point".to_string(),
            String::new(),
        ];

        // Search
        let search_line = format!(
            "  \x1b[2mSearch:\x1b[0m {}{}",
            self.search_query,
            if self.search_query.is_empty() {
                "\x1b[2m_\x1b[0m"
            } else {
                "_"
            }
        );
        lines.push(truncate_to_width(&search_line, width));
        lines.push(String::new());

        if self.filtered_indices.is_empty() {
            lines.push("  \x1b[2mNo messages found\x1b[0m".to_string());
        } else {
            let max_visible = 10;
            let start = if self.filtered_indices.len() <= max_visible {
                0
            } else {
                let half = max_visible / 2;
                let max_start = self.filtered_indices.len() - max_visible;
                self.selected_index.saturating_sub(half).min(max_start)
            };
            let end = (start + max_visible).min(self.filtered_indices.len());

            for (display_idx, &original_idx) in self.filtered_indices[start..end].iter().enumerate()
            {
                let candidate = &self.candidates[original_idx];
                let is_selected = display_idx + start == self.selected_index;
                let cursor = if is_selected {
                    "\x1b[36m› \x1b[0m"
                } else {
                    "  "
                };
                let text = candidate.text.replace('\n', " ");
                let text = truncate_to_width(&text, width.saturating_sub(4));
                let line = if is_selected {
                    format!("{}\x1b[1m{}\x1b[0m", cursor, text)
                } else {
                    format!("{}{}", cursor, text)
                };
                lines.push(line);
            }

            // Position indicator
            if self.filtered_indices.len() > max_visible {
                lines.push(format!(
                    "  \x1b[2m({}/{})\x1b[0m",
                    self.selected_index + 1,
                    self.filtered_indices.len()
                ));
            }
        }

        lines.push(String::new());
        lines.push("─".repeat(width.min(80)));

        lines
    }
}

enum BranchSelectorResult {
    Selected(String),
    Cancelled,
}

struct ModelSelectorState {
    selector: ModelSelectorComponent,
}

struct SettingsSelectorState {
    selector: SettingsSelectorComponent,
}

struct OAuthSelectorState {
    selector: OAuthSelectorComponent,
    mode: OAuthSelectorMode,
}

struct LoginDialogSelectorState {
    dialog: LoginDialogComponent,
    provider_id: String,
    // State for the OAuth flow
    oauth_state: OAuthFlowState,
}

#[allow(dead_code)]
enum OAuthFlowState {
    // Anthropic: waiting for user to paste code
    AnthropicWaitingCode {
        verifier: String,
    },
    // GitHub: polling for device code completion
    GitHubPolling {
        domain: String,
        device_code: String,
        interval: u64,
        expires_in: u64,
    },
    // OpenAI: waiting for callback or manual paste
    OpenAIWaitingCallback {
        verifier: String,
        state: String,
        server: Option<OAuthCallbackServer>,
    },
    // Completed (will be handled outside modal)
    Completed,
    // Failed
    Failed(String),
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

/// Convert crossterm KeyEvent to raw terminal sequence for matches_key.
/// The matches_key function expects raw terminal bytes, not human-readable names.
fn key_event_to_data(key: &KeyEvent) -> String {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let alt = key.modifiers.contains(KeyModifiers::ALT);

    // Calculate modifier parameter for CSI sequences (1 + modifier bits)
    // Shift=1, Alt=2, Ctrl=4
    let modifier_param = if ctrl || shift || alt {
        let mut m = 1u8; // Base is 1
        if shift {
            m += 1;
        }
        if alt {
            m += 2;
        }
        if ctrl {
            m += 4;
        }
        Some(m)
    } else {
        None
    };

    match key.code {
        KeyCode::Up => {
            if let Some(m) = modifier_param {
                format!("\x1b[1;{}A", m)
            } else {
                "\x1b[A".to_string()
            }
        }
        KeyCode::Down => {
            if let Some(m) = modifier_param {
                format!("\x1b[1;{}B", m)
            } else {
                "\x1b[B".to_string()
            }
        }
        KeyCode::Right => {
            if let Some(m) = modifier_param {
                format!("\x1b[1;{}C", m)
            } else {
                "\x1b[C".to_string()
            }
        }
        KeyCode::Left => {
            if let Some(m) = modifier_param {
                format!("\x1b[1;{}D", m)
            } else {
                "\x1b[D".to_string()
            }
        }
        KeyCode::Enter => "\r".to_string(),
        KeyCode::Esc => "\x1b".to_string(),
        KeyCode::Backspace => {
            if alt {
                "\x1b\x7f".to_string()
            } else {
                "\x7f".to_string()
            }
        }
        KeyCode::Tab => {
            if shift {
                "\x1b[Z".to_string()
            } else {
                "\t".to_string()
            }
        }
        KeyCode::Home => "\x1b[H".to_string(),
        KeyCode::End => "\x1b[F".to_string(),
        KeyCode::Delete => "\x1b[3~".to_string(),
        KeyCode::Char(ch) => {
            if ctrl && ch.is_ascii_alphabetic() {
                // Ctrl+letter produces control character (a=1, b=2, ..., z=26)
                let code = (ch.to_ascii_lowercase() as u8) - b'a' + 1;
                String::from(code as char)
            } else if shift && ch.is_ascii_lowercase() {
                // Shift+letter produces uppercase
                ch.to_ascii_uppercase().to_string()
            } else {
                ch.to_string()
            }
        }
        _ => String::new(),
    }
}

fn render_modal_ui(modal: &ModalState, stdout: &mut impl Write) -> Result<(), String> {
    let (width, height) = terminal::size().map_err(|err| err.to_string())?;
    let width = width.max(1) as usize;
    let height = height.max(1) as usize;

    let modal_lines = match modal {
        ModalState::None => return Ok(()),
        ModalState::TreeSelector(state) => state.selector.render(width),
        ModalState::SessionSelector(state) => state.selector.render(width),
        ModalState::BranchSelector(state) => state.render(width),
        ModalState::ModelSelector(state) => state.selector.render(width),
        ModalState::SettingsSelector(state) => state.selector.render(width),
        ModalState::OAuthSelector(state) => state.selector.render(width),
        ModalState::LoginDialog(state) => state.dialog.render(width),
    };

    // Truncate modal to fit screen
    let visible_lines: Vec<&str> = modal_lines
        .iter()
        .take(height)
        .map(|s| s.as_str())
        .collect();

    stdout
        .execute(MoveTo(0, 0))
        .map_err(|err| err.to_string())?;
    stdout
        .execute(Clear(ClearType::All))
        .map_err(|err| err.to_string())?;

    for (index, line) in visible_lines.iter().enumerate() {
        let truncated = truncate_to_width(line, width);
        if index + 1 == visible_lines.len() {
            write!(stdout, "{truncated}").map_err(|err| err.to_string())?;
        } else {
            write!(stdout, "{truncated}\r\n").map_err(|err| err.to_string())?;
        }
    }

    // Fill remaining lines
    for i in visible_lines.len()..height {
        if i + 1 == height {
            write!(stdout, "").map_err(|err| err.to_string())?;
        } else {
            write!(stdout, "\r\n").map_err(|err| err.to_string())?;
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

fn append_status_entry(entries: &mut Vec<String>, message: &str) {
    entries.push(format!("Status:\n{message}"));
}

fn apply_setting_change(
    session: &mut AgentSession,
    setting_id: &str,
    value: &str,
    editor: &mut Editor,
    rebuild: &mut bool,
) {
    match setting_id {
        "autocompact" => {
            if let Some(enabled) = parse_bool(value) {
                session.settings_manager.set_compaction_enabled(enabled);
            }
        }
        "show-images" => {
            if let Some(enabled) = parse_bool(value) {
                session.settings_manager.set_show_images(enabled);
                *rebuild = true;
            }
        }
        "auto-resize-images" => {
            if let Some(enabled) = parse_bool(value) {
                session.settings_manager.set_image_auto_resize(enabled);
            }
        }
        "steering-mode" => {
            if let Some(mode) = parse_queue_mode(value) {
                session.set_steering_mode(mode);
                session.settings_manager.set_steering_mode(value);
            }
        }
        "follow-up-mode" => {
            if let Some(mode) = parse_queue_mode(value) {
                session.set_follow_up_mode(mode);
                session.settings_manager.set_follow_up_mode(value);
            }
        }
        "thinking-level" => {
            if let Some(level) = parse_thinking_level_value(value) {
                session.set_thinking_level(level);
            }
        }
        "theme" => {
            let themes = available_themes();
            if themes.iter().any(|theme| theme == value) {
                session.settings_manager.set_theme(value);
                let theme = load_theme_or_default(Some(value));
                set_active_theme(theme.clone());
                editor.set_theme(theme.editor_theme());
            }
        }
        "hide-thinking" => {
            if let Some(enabled) = parse_bool(value) {
                session.settings_manager.set_hide_thinking_block(enabled);
                *rebuild = true;
            }
        }
        "collapse-changelog" => {
            if let Some(enabled) = parse_bool(value) {
                session.settings_manager.set_collapse_changelog(enabled);
            }
        }
        "double-escape-action" => {
            if matches!(value, "tree" | "branch") {
                session.settings_manager.set_double_escape_action(value);
            }
        }
        _ => {}
    }
}

fn build_settings_items(session: &AgentSession) -> Vec<SettingItem> {
    let theme_values: Vec<SettingValue> = available_themes()
        .into_iter()
        .map(|name| SettingValue {
            value: name.clone(),
            label: name.clone(),
            description: None,
        })
        .collect();

    vec![
        SettingItem {
            id: "autocompact".to_string(),
            label: "Auto-compact".to_string(),
            description: "Automatically compact context when it gets too large".to_string(),
            current_value: session
                .settings_manager
                .get_compaction_enabled()
                .to_string(),
            values: bool_values(),
        },
        SettingItem {
            id: "show-images".to_string(),
            label: "Show images".to_string(),
            description: "Display inline images in terminal (if supported)".to_string(),
            current_value: session.settings_manager.get_show_images().to_string(),
            values: bool_values(),
        },
        SettingItem {
            id: "auto-resize-images".to_string(),
            label: "Auto-resize images".to_string(),
            description: "Automatically resize large images".to_string(),
            current_value: session.settings_manager.get_image_auto_resize().to_string(),
            values: bool_values(),
        },
        SettingItem {
            id: "steering-mode".to_string(),
            label: "Steering mode".to_string(),
            description: "How to handle multiple steering messages".to_string(),
            current_value: session.settings_manager.get_steering_mode().to_string(),
            values: queue_mode_values(),
        },
        SettingItem {
            id: "follow-up-mode".to_string(),
            label: "Follow-up mode".to_string(),
            description: "How to handle follow-up messages".to_string(),
            current_value: session.settings_manager.get_follow_up_mode().to_string(),
            values: queue_mode_values(),
        },
        SettingItem {
            id: "thinking-level".to_string(),
            label: "Thinking level".to_string(),
            description: "Amount of reasoning to use".to_string(),
            current_value: session.agent.state().thinking_level.as_str().to_string(),
            values: thinking_level_values(),
        },
        SettingItem {
            id: "theme".to_string(),
            label: "Theme".to_string(),
            description: "Color theme for the interface".to_string(),
            current_value: session
                .settings_manager
                .get_theme()
                .unwrap_or_else(|| "dark".to_string()),
            values: theme_values,
        },
        SettingItem {
            id: "hide-thinking".to_string(),
            label: "Hide thinking".to_string(),
            description: "Hide thinking blocks in chat".to_string(),
            current_value: session
                .settings_manager
                .get_hide_thinking_block()
                .to_string(),
            values: bool_values(),
        },
        SettingItem {
            id: "collapse-changelog".to_string(),
            label: "Collapse changelog".to_string(),
            description: "Collapse changelog entries by default".to_string(),
            current_value: session
                .settings_manager
                .get_collapse_changelog()
                .to_string(),
            values: bool_values(),
        },
        SettingItem {
            id: "double-escape-action".to_string(),
            label: "Double-escape action".to_string(),
            description: "Action to perform on double Escape key".to_string(),
            current_value: session
                .settings_manager
                .get_double_escape_action()
                .to_string(),
            values: double_escape_action_values(),
        },
    ]
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
    // Handle autocomplete mode first
    if editor.is_autocompleting() {
        match key.code {
            KeyCode::Esc => {
                editor.cancel_autocomplete();
                return EditorAction::Continue;
            }
            KeyCode::Up => {
                editor.autocomplete_up();
                return EditorAction::Continue;
            }
            KeyCode::Down => {
                editor.autocomplete_down();
                return EditorAction::Continue;
            }
            KeyCode::Tab | KeyCode::Enter => {
                editor.apply_autocomplete();
                // If it's a Tab, continue editing
                // If it's Enter and the text starts with a command, let it submit
                if key.code == KeyCode::Enter
                    && !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT)
                {
                    return EditorAction::Submit;
                }
                return EditorAction::Continue;
            }
            KeyCode::Backspace => {
                editor.handle_input("\x7f");
                editor.try_trigger_autocomplete();
                return EditorAction::Continue;
            }
            KeyCode::Char(ch) => {
                editor.handle_input(&ch.to_string());
                editor.try_trigger_autocomplete();
                return EditorAction::Continue;
            }
            _ => {
                // Cancel autocomplete on any other key
                editor.cancel_autocomplete();
            }
        }
    }

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
            // Try to trigger file autocomplete on Tab
            if !editor.try_force_file_autocomplete() {
                // If no autocomplete, insert literal tab or do nothing
            }
        }
        KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            editor.handle_input("\x01");
        }
        KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            editor.handle_input("\x17");
        }
        KeyCode::Char('v') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            return EditorAction::PasteImage;
        }
        KeyCode::Char(ch) => {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                return EditorAction::Continue;
            }
            editor.handle_input(&ch.to_string());
            // Auto-trigger autocomplete for / at line start
            if ch == '/' {
                editor.try_trigger_autocomplete();
            }
        }
        _ => {}
    }
    EditorAction::Continue
}

fn get_slash_commands() -> Vec<SlashCommand> {
    vec![
        SlashCommand::new("branch", Some("Create branch from message".to_string())),
        SlashCommand::new("changelog", Some("Show version changelog".to_string())),
        SlashCommand::new("clear", Some("Clear the screen".to_string())),
        SlashCommand::new("compact", Some("Compact the session".to_string())),
        SlashCommand::new("copy", Some("Copy last message to clipboard".to_string())),
        SlashCommand::new("exit", Some("Exit the session".to_string())),
        SlashCommand::new("export", Some("Export session as HTML".to_string())),
        SlashCommand::new("help", Some("Show available commands".to_string())),
        SlashCommand::new("hotkeys", Some("Show keyboard shortcuts".to_string())),
        SlashCommand::new("login", Some("Login to OAuth provider".to_string())),
        SlashCommand::new("logout", Some("Logout from OAuth provider".to_string())),
        SlashCommand::new("model", Some("Select AI model".to_string())),
        SlashCommand::new("new", Some("Start new session".to_string())),
        SlashCommand::new("quit", Some("Exit the session".to_string())),
        SlashCommand::new("reset", Some("Reset session".to_string())),
        SlashCommand::new("resume", Some("Resume different session".to_string())),
        SlashCommand::new("session", Some("Show session info".to_string())),
        SlashCommand::new("settings", Some("Configure settings".to_string())),
        SlashCommand::new("share", Some("Share session as GitHub Gist".to_string())),
        SlashCommand::new("theme", Some("Change theme".to_string())),
        SlashCommand::new("tree", Some("Navigate session tree".to_string())),
    ]
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

    // Set up autocomplete with slash commands + prompt templates + extension commands
    let cwd = std::env::current_dir().unwrap_or_default();
    let mut all_commands = get_slash_commands();

    // Add prompt templates as slash commands for autocomplete
    for template in session.prompt_templates() {
        all_commands.push(SlashCommand::new(
            template.name.clone(),
            Some(template.description.clone()),
        ));
    }

    // Add extension commands for autocomplete
    for cmd in session.extension_commands() {
        all_commands.push(SlashCommand::new(cmd.name.clone(), cmd.description.clone()));
    }

    let autocomplete_provider = CombinedAutocompleteProvider::new(all_commands, cwd);
    editor.set_autocomplete_provider(autocomplete_provider);

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

    let mut modal_state = ModalState::None;

    loop {
        // Handle modal state rendering and input
        if !matches!(modal_state, ModalState::None) {
            render_modal_ui(&modal_state, &mut stdout)?;

            match event::read().map_err(|err| err.to_string())? {
                Event::Key(key) => {
                    // Check for Ctrl+C to exit regardless of modal state
                    if key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        break;
                    }

                    let key_data = key_event_to_data(&key);
                    match &mut modal_state {
                        ModalState::TreeSelector(state) => {
                            // Handle tree selector input
                            if matches_key(&key_data, "escape") {
                                state.result = Some(TreeSelectorResult::Cancelled);
                            } else if matches_key(&key_data, "enter") {
                                if let Some(id) = state.selector.selected_entry_id() {
                                    state.result =
                                        Some(TreeSelectorResult::Selected(id.to_string()));
                                }
                            } else {
                                state.selector.handle_input(&key_data);
                            }
                        }
                        ModalState::SessionSelector(state) => {
                            // Handle session selector input
                            if matches_key(&key_data, "escape") {
                                state.result = Some(SessionSelectorResult::Cancelled);
                            } else if matches_key(&key_data, "enter") {
                                if let Some(path) = state.selector.get_selected() {
                                    state.result = Some(SessionSelectorResult::Selected(path));
                                }
                            } else {
                                state.selector.handle_input(&key_data);
                            }
                        }
                        ModalState::BranchSelector(state) => {
                            state.handle_input(&key_data);
                        }
                        ModalState::ModelSelector(state) => {
                            // Model selector handles its own input and returns result
                            if let Some(result) = state.selector.handle_input(&key_data) {
                                match result {
                                    ModelSelectorResult::Selected { provider, model_id } => {
                                        modal_state = ModalState::None;
                                        if let Some(model) = session
                                            .get_available_models()
                                            .iter()
                                            .find(|m| m.provider == provider && m.id == model_id)
                                        {
                                            session.set_model(to_agent_model(model));
                                            session
                                                .settings_manager
                                                .set_default_model_and_provider(
                                                    &provider, &model_id,
                                                );
                                            append_status_entry(
                                                &mut entries,
                                                &format!("Model set to {}/{}", provider, model_id),
                                            );
                                        }
                                        render_interactive_ui(&entries, &mut editor, &mut stdout)?;
                                        continue;
                                    }
                                    ModelSelectorResult::Cancelled => {
                                        modal_state = ModalState::None;
                                        render_interactive_ui(&entries, &mut editor, &mut stdout)?;
                                        continue;
                                    }
                                }
                            }
                        }
                        ModalState::SettingsSelector(state) => {
                            // Settings selector handles its own input and returns result
                            if let Some(result) = state.selector.handle_input(&key_data) {
                                match result {
                                    SettingsSelectorResult::Changed { setting_id, value } => {
                                        let mut rebuild = false;
                                        apply_setting_change(
                                            session,
                                            &setting_id,
                                            &value,
                                            &mut editor,
                                            &mut rebuild,
                                        );
                                        if rebuild {
                                            entries = rebuild_interactive_entries(session, true);
                                        }
                                        append_status_entry(
                                            &mut entries,
                                            &format!("Updated {} to {}", setting_id, value),
                                        );
                                        // Stay in settings modal for more changes
                                        // User can press Esc to exit
                                    }
                                    SettingsSelectorResult::Cancelled => {
                                        modal_state = ModalState::None;
                                        render_interactive_ui(&entries, &mut editor, &mut stdout)?;
                                        continue;
                                    }
                                }
                            }
                        }
                        ModalState::OAuthSelector(state) => {
                            if let Some(result) = state.selector.handle_input(&key_data) {
                                match result {
                                    OAuthSelectorResult::Selected(provider_id) => {
                                        // Start the login/logout flow for this provider
                                        let mode = state.mode;
                                        modal_state = ModalState::None;

                                        if mode == OAuthSelectorMode::Logout {
                                            // Logout: remove credentials
                                            session.model_registry.remove_credential(&provider_id);
                                            session.model_registry.refresh();
                                            append_status_entry(
                                                &mut entries,
                                                &format!("Logged out of {}", provider_id),
                                            );
                                            render_interactive_ui(
                                                &entries,
                                                &mut editor,
                                                &mut stdout,
                                            )?;
                                            continue;
                                        }

                                        // Login: start the OAuth flow
                                        let provider_name = get_oauth_providers()
                                            .iter()
                                            .find(|p| p.id == provider_id)
                                            .map(|p| p.name.clone())
                                            .unwrap_or_else(|| provider_id.clone());

                                        let (dialog, oauth_state) =
                                            start_oauth_login(&provider_id, &provider_name);

                                        modal_state =
                                            ModalState::LoginDialog(LoginDialogSelectorState {
                                                dialog,
                                                provider_id,
                                                oauth_state,
                                            });
                                        continue;
                                    }
                                    OAuthSelectorResult::Cancelled => {
                                        modal_state = ModalState::None;
                                        render_interactive_ui(&entries, &mut editor, &mut stdout)?;
                                        continue;
                                    }
                                }
                            }
                        }
                        ModalState::LoginDialog(state) => {
                            if let Some(result) = state.dialog.handle_input(&key_data) {
                                match result {
                                    LoginDialogResult::InputSubmitted(input) => {
                                        // Process the input based on OAuth flow state
                                        let provider_id = state.provider_id.clone();
                                        match &state.oauth_state {
                                            OAuthFlowState::AnthropicWaitingCode { verifier } => {
                                                match anthropic_exchange_code(&input, verifier) {
                                                    Ok(creds) => {
                                                        session.model_registry.set_credential(
                                                            &provider_id,
                                                            creds.to_auth_credential(),
                                                        );
                                                        session.model_registry.refresh();
                                                        modal_state = ModalState::None;
                                                        append_status_entry(
                                                            &mut entries,
                                                            &format!(
                                                                "Logged in to {}. Credentials saved.",
                                                                provider_id
                                                            ),
                                                        );
                                                        render_interactive_ui(
                                                            &entries,
                                                            &mut editor,
                                                            &mut stdout,
                                                        )?;
                                                        continue;
                                                    }
                                                    Err(e) => {
                                                        state.dialog.fail(&e);
                                                    }
                                                }
                                            }
                                            OAuthFlowState::OpenAIWaitingCallback {
                                                verifier,
                                                state: oauth_state_str,
                                                ..
                                            } => {
                                                match openai_codex_login_with_input(
                                                    &input,
                                                    verifier,
                                                    oauth_state_str,
                                                ) {
                                                    Ok(creds) => {
                                                        session.model_registry.set_credential(
                                                            &provider_id,
                                                            creds.to_auth_credential(),
                                                        );
                                                        session.model_registry.refresh();
                                                        modal_state = ModalState::None;
                                                        append_status_entry(
                                                            &mut entries,
                                                            &format!(
                                                                "Logged in to {}. Credentials saved.",
                                                                provider_id
                                                            ),
                                                        );
                                                        render_interactive_ui(
                                                            &entries,
                                                            &mut editor,
                                                            &mut stdout,
                                                        )?;
                                                        continue;
                                                    }
                                                    Err(e) => {
                                                        state.dialog.fail(&e);
                                                    }
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                    LoginDialogResult::Cancelled => {
                                        modal_state = ModalState::None;
                                        render_interactive_ui(&entries, &mut editor, &mut stdout)?;
                                        continue;
                                    }
                                }
                            }
                        }
                        ModalState::None => {}
                    }

                    // Check and process modal completion
                    // Tree selector result
                    if let ModalState::TreeSelector(state) = &mut modal_state {
                        if let Some(result) = state.result.take() {
                            match result {
                                TreeSelectorResult::Selected(entry_id) => {
                                    modal_state = ModalState::None;
                                    match session.navigate_tree(
                                        &entry_id,
                                        crate::coding_agent::NavigateTreeOptions::default(),
                                    ) {
                                        Ok(_result) => {
                                            entries = rebuild_interactive_entries(session, true);
                                            append_status_entry(
                                                &mut entries,
                                                "Navigated to selected entry.",
                                            );
                                        }
                                        Err(err) => {
                                            append_status_entry(
                                                &mut entries,
                                                &format!("Navigation failed: {err}"),
                                            );
                                        }
                                    }
                                }
                                TreeSelectorResult::Cancelled => {
                                    modal_state = ModalState::None;
                                }
                            }
                            render_interactive_ui(&entries, &mut editor, &mut stdout)?;
                            continue;
                        }
                    }

                    // Session selector result
                    if let ModalState::SessionSelector(state) = &mut modal_state {
                        if let Some(result) = state.result.take() {
                            match result {
                                SessionSelectorResult::Selected(path) => {
                                    modal_state = ModalState::None;
                                    match session.switch_session(path) {
                                        Ok(_) => {
                                            entries = rebuild_interactive_entries(session, true);
                                            append_status_entry(
                                                &mut entries,
                                                &format!(
                                                    "Resumed session: {}",
                                                    session.session_id()
                                                ),
                                            );
                                        }
                                        Err(err) => {
                                            append_status_entry(
                                                &mut entries,
                                                &format!("Failed to resume session: {err}"),
                                            );
                                        }
                                    }
                                }
                                SessionSelectorResult::Cancelled => {
                                    modal_state = ModalState::None;
                                }
                            }
                            render_interactive_ui(&entries, &mut editor, &mut stdout)?;
                            continue;
                        }
                    }

                    // Branch selector result
                    if let ModalState::BranchSelector(state) = &mut modal_state {
                        if let Some(result) = state.result.take() {
                            match result {
                                BranchSelectorResult::Selected(entry_id) => {
                                    modal_state = ModalState::None;
                                    match session.branch(&entry_id) {
                                        Ok(result) => {
                                            entries = rebuild_interactive_entries(session, true);
                                            let msg = if result.selected_text.is_empty() {
                                                "Created new branch.".to_string()
                                            } else {
                                                format!(
                                                    "Created branch from: {}",
                                                    truncate_to_width(&result.selected_text, 50)
                                                )
                                            };
                                            append_status_entry(&mut entries, &msg);
                                        }
                                        Err(err) => {
                                            append_status_entry(
                                                &mut entries,
                                                &format!("Failed to create branch: {err}"),
                                            );
                                        }
                                    }
                                }
                                BranchSelectorResult::Cancelled => {
                                    modal_state = ModalState::None;
                                }
                            }
                            render_interactive_ui(&entries, &mut editor, &mut stdout)?;
                            continue;
                        }
                    }
                }
                Event::Resize(_, _) => {
                    render_modal_ui(&modal_state, &mut stdout)?;
                }
                _ => {}
            }
            continue;
        }

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
                    // Handle bash command (! for normal, !! for excluded from context)
                    if let Some(rest) = trimmed.strip_prefix('!') {
                        let is_excluded = rest.starts_with('!');
                        let command = if is_excluded {
                            rest[1..].trim()
                        } else {
                            rest.trim()
                        };
                        if !command.is_empty() {
                            editor.add_to_history(&prompt);
                            match session.execute_bash(command) {
                                Ok(result) => {
                                    // Format output like a shell
                                    let mut display = format!("$ {command}\n");
                                    if !result.output.is_empty() {
                                        display.push_str(&result.output);
                                        if !result.output.ends_with('\n') {
                                            display.push('\n');
                                        }
                                    }
                                    if let Some(code) = result.exit_code {
                                        if code != 0 {
                                            display.push_str(&format!("[exit code: {code}]\n"));
                                        }
                                    }
                                    if result.cancelled {
                                        display.push_str("[cancelled]\n");
                                    }
                                    if is_excluded {
                                        // For !!, just show output without adding to context
                                        append_status_entry(&mut entries, display.trim_end());
                                    } else {
                                        // For !, show output and potentially add to context
                                        // (though execute_bash doesn't add to session context)
                                        append_status_entry(&mut entries, display.trim_end());
                                    }
                                }
                                Err(err) => {
                                    append_status_entry(
                                        &mut entries,
                                        &format!("Bash error: {err}"),
                                    );
                                }
                            }
                            render_interactive_ui(&entries, &mut editor, &mut stdout)?;
                            continue;
                        }
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
                            // Open model selector UI
                            if available.is_empty() {
                                append_status_entry(
                                    &mut entries,
                                    "No models available. Set an API key in auth.json or env.",
                                );
                                render_interactive_ui(&entries, &mut editor, &mut stdout)?;
                                continue;
                            }
                            let (_, height) = terminal::size().unwrap_or((80, 24));
                            let max_visible = ((height as usize).saturating_sub(10)).clamp(5, 15);
                            let model_items: Vec<ModelItem> = available
                                .iter()
                                .map(|m| {
                                    ModelItem::from_model(
                                        m,
                                        &current_model.provider,
                                        &current_model.id,
                                    )
                                })
                                .collect();
                            let selector = ModelSelectorComponent::new(model_items, max_visible);
                            modal_state =
                                ModalState::ModelSelector(ModelSelectorState { selector });
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
                            // Open settings selector UI
                            let (_, height) = terminal::size().unwrap_or((80, 24));
                            let max_visible = ((height as usize).saturating_sub(10)).clamp(5, 15);
                            let settings_items = build_settings_items(session);
                            let selector =
                                SettingsSelectorComponent::new(settings_items, max_visible);
                            modal_state =
                                ModalState::SettingsSelector(SettingsSelectorState { selector });
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
                            "Ctrl+V: paste image from clipboard",
                            "Arrow keys: move cursor / history",
                            "Ctrl+Left/Right: move by word",
                            "Ctrl+A: start of line",
                            "Ctrl+W or Alt+Backspace: delete word",
                            "Tab: file autocomplete",
                            "! command: run shell command",
                            "!! command: run shell command (excluded from context)",
                            "/ commands: type / to see autocomplete suggestions",
                        ]
                        .join("\n");
                        append_status_entry(&mut entries, &hotkeys);
                        render_interactive_ui(&entries, &mut editor, &mut stdout)?;
                        continue;
                    }
                    if matches!(trimmed, "/exit" | "/quit") {
                        break;
                    }
                    if trimmed == "/clear" {
                        entries.clear();
                        render_interactive_ui(&entries, &mut editor, &mut stdout)?;
                        continue;
                    }
                    if trimmed == "/help" {
                        let help_text = [
                            "Available commands:",
                            "  /branch       - Create branch from message",
                            "  /changelog    - Show version changelog",
                            "  /clear        - Clear the screen",
                            "  /compact      - Compact the session",
                            "  /copy         - Copy last assistant message to clipboard",
                            "  /export       - Export session as HTML",
                            "  /help         - Show this help",
                            "  /hotkeys      - Show keyboard shortcuts",
                            "  /login        - Login to OAuth provider",
                            "  /logout       - Logout from OAuth provider",
                            "  /model        - Select AI model",
                            "  /new          - Start new session",
                            "  /reset        - Reset/clear the session",
                            "  /resume       - Resume different session",
                            "  /session      - Show session information",
                            "  /settings     - Configure settings",
                            "  /share        - Share session as GitHub Gist",
                            "  /theme <name> - Change theme",
                            "  /tree         - Navigate session tree",
                            "  /exit, /quit  - Exit the session",
                            "",
                            "Type / to see autocomplete suggestions.",
                        ]
                        .join("\n");
                        append_status_entry(&mut entries, &help_text);
                        render_interactive_ui(&entries, &mut editor, &mut stdout)?;
                        continue;
                    }
                    if trimmed.starts_with("/theme") {
                        let rest = trimmed.trim_start_matches("/theme").trim();
                        let themes = available_themes();
                        if rest.is_empty() {
                            let current_theme = session
                                .settings_manager
                                .get_theme()
                                .unwrap_or_else(|| "dark".to_string());
                            let theme_list = themes
                                .iter()
                                .map(|t| {
                                    if t == &current_theme {
                                        format!("  * {t} (current)")
                                    } else {
                                        format!("    {t}")
                                    }
                                })
                                .collect::<Vec<_>>()
                                .join("\n");
                            append_status_entry(
                                &mut entries,
                                &format!("Available themes:\n{theme_list}\n\nUsage: /theme <name>"),
                            );
                        } else if themes.iter().any(|t| t == rest) {
                            session.settings_manager.set_theme(rest);
                            let theme = load_theme_or_default(Some(rest));
                            set_active_theme(theme.clone());
                            editor.set_theme(theme.editor_theme());
                            append_status_entry(&mut entries, &format!("Theme changed to: {rest}"));
                        } else {
                            append_status_entry(
                                &mut entries,
                                &format!(
                                    "Unknown theme: {rest}. Run /theme to see available themes."
                                ),
                            );
                        }
                        render_interactive_ui(&entries, &mut editor, &mut stdout)?;
                        continue;
                    }
                    if trimmed == "/reset" {
                        session.new_session();
                        entries.clear();
                        append_status_entry(&mut entries, "Session reset.");
                        render_interactive_ui(&entries, &mut editor, &mut stdout)?;
                        continue;
                    }
                    if trimmed == "/session" {
                        let session_id = session.session_id();
                        let message_count = session.messages().len();
                        let model = &session.agent.state().model;
                        let info = format!(
                            "Session Info:\n  ID: {session_id}\n  Messages: {message_count}\n  Model: {}/{}",
                            model.provider, model.id
                        );
                        append_status_entry(&mut entries, &info);
                        render_interactive_ui(&entries, &mut editor, &mut stdout)?;
                        continue;
                    }
                    if trimmed == "/copy" {
                        // Get the last assistant message text
                        if let Some(text) = session.get_last_assistant_text() {
                            if text.is_empty() {
                                append_status_entry(
                                    &mut entries,
                                    "No text content in last assistant message.",
                                );
                            } else {
                                match copy_to_clipboard(&text) {
                                    Ok(()) => {
                                        append_status_entry(&mut entries, "Copied to clipboard.")
                                    }
                                    Err(err) => append_status_entry(
                                        &mut entries,
                                        &format!("Failed to copy: {err}"),
                                    ),
                                }
                            }
                        } else {
                            append_status_entry(&mut entries, "No assistant messages to copy.");
                        }
                        render_interactive_ui(&entries, &mut editor, &mut stdout)?;
                        continue;
                    }
                    if trimmed == "/new" {
                        // Create a new session by resetting and generating a new ID
                        session.new_session();
                        entries.clear();
                        append_status_entry(
                            &mut entries,
                            &format!("New session started: {}", session.session_id()),
                        );
                        render_interactive_ui(&entries, &mut editor, &mut stdout)?;
                        continue;
                    }
                    if trimmed == "/tree" {
                        let tree = session.session_manager.get_tree();
                        if tree.is_empty() {
                            append_status_entry(&mut entries, "Session tree is empty.");
                            render_interactive_ui(&entries, &mut editor, &mut stdout)?;
                            continue;
                        }
                        let (_, height) = terminal::size().unwrap_or((80, 24));
                        let leaf_id = session.session_manager.get_leaf_id();
                        let selector = TreeSelectorComponent::new(tree, leaf_id, height as usize);
                        modal_state = ModalState::TreeSelector(TreeSelectorState {
                            selector,
                            result: None,
                        });
                        continue;
                    }
                    if trimmed == "/branch" {
                        let candidates = session.get_user_messages_for_branching();
                        if candidates.is_empty() {
                            append_status_entry(&mut entries, "No user messages to branch from.");
                            render_interactive_ui(&entries, &mut editor, &mut stdout)?;
                            continue;
                        }
                        modal_state =
                            ModalState::BranchSelector(BranchSelectorState::new(candidates));
                        continue;
                    }
                    if trimmed == "/resume" {
                        let cwd = std::env::current_dir().unwrap_or_default();
                        let session_dir = Some(session.session_manager.get_session_dir());
                        let sessions = SessionManager::list(&cwd, session_dir);
                        if sessions.is_empty() {
                            append_status_entry(&mut entries, "No sessions found.");
                            render_interactive_ui(&entries, &mut editor, &mut stdout)?;
                            continue;
                        }
                        let (_, height) = terminal::size().unwrap_or((80, 24));
                        let max_visible = (height as usize / 3).max(3);
                        let selector = SessionSelectorComponent::new(sessions, max_visible);
                        modal_state = ModalState::SessionSelector(SessionSelectorState {
                            selector,
                            result: None,
                        });
                        continue;
                    }
                    if trimmed == "/login" || trimmed.starts_with("/login ") {
                        // Show OAuth provider selector for login
                        let selector = OAuthSelectorComponent::new(
                            OAuthSelectorMode::Login,
                            &session.model_registry,
                        );
                        modal_state = ModalState::OAuthSelector(OAuthSelectorState {
                            selector,
                            mode: OAuthSelectorMode::Login,
                        });
                        continue;
                    }
                    if trimmed == "/logout" || trimmed.starts_with("/logout ") {
                        // Check if any providers are logged in
                        let has_logged_in = get_oauth_providers().iter().any(|p| {
                            matches!(
                                session.model_registry.get_credential(&p.id),
                                Some(AuthCredential::OAuth { .. })
                            )
                        });
                        if !has_logged_in {
                            append_status_entry(
                                &mut entries,
                                "No OAuth providers logged in. Use /login first.",
                            );
                            render_interactive_ui(&entries, &mut editor, &mut stdout)?;
                            continue;
                        }

                        // Show OAuth provider selector for logout
                        let selector = OAuthSelectorComponent::new(
                            OAuthSelectorMode::Logout,
                            &session.model_registry,
                        );
                        modal_state = ModalState::OAuthSelector(OAuthSelectorState {
                            selector,
                            mode: OAuthSelectorMode::Logout,
                        });
                        continue;
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
                EditorAction::PasteImage => {
                    // Handle Ctrl+V image paste
                    if let Some(path) = paste_image_from_clipboard() {
                        editor.insert_text_at_cursor(&path);
                    }
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

/// Start OAuth login flow for a provider
fn start_oauth_login(
    provider_id: &str,
    provider_name: &str,
) -> (LoginDialogComponent, OAuthFlowState) {
    let mut dialog = LoginDialogComponent::new(provider_name);

    match provider_id {
        "anthropic" => {
            let (url, verifier) = anthropic_get_auth_url();
            dialog.show_auth(
                &url,
                Some("Complete authorization and paste the code (code#state):"),
            );
            open_browser(&url);
            dialog.show_prompt("Paste authorization code:", Some("abc123#xyz789"));
            (dialog, OAuthFlowState::AnthropicWaitingCode { verifier })
        }
        "github-copilot" => {
            // GitHub uses device code flow - we'd need to poll in background
            // For now, show instructions
            dialog.show_progress("GitHub Copilot login requires browser authentication.");
            dialog.show_auth(
                "https://github.com/login/device",
                Some("Device code flow not yet implemented in TUI. Use manual auth.json configuration."),
            );
            (
                dialog,
                OAuthFlowState::Failed(
                    "GitHub Copilot device flow not implemented in TUI".to_string(),
                ),
            )
        }
        "openai-codex" => {
            let (url, verifier, state) = openai_codex_get_auth_url();
            let server = OAuthCallbackServer::start(&state);
            let has_server = server.is_available();

            dialog.show_auth(
                &url,
                if has_server {
                    Some("A browser window should open. Complete login to finish.")
                } else {
                    Some("Complete login and paste the redirect URL:")
                },
            );
            open_browser(&url);

            if !has_server {
                dialog.show_prompt("Paste redirect URL or code:", None);
            }

            (
                dialog,
                OAuthFlowState::OpenAIWaitingCallback {
                    verifier,
                    state,
                    server: Some(server),
                },
            )
        }
        _ => {
            dialog.fail(&format!("Unknown provider: {}", provider_id));
            (
                dialog,
                OAuthFlowState::Failed(format!("Unknown provider: {}", provider_id)),
            )
        }
    }
}

fn copy_to_clipboard(text: &str) -> Result<(), String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    // Try various clipboard commands based on what's available
    // On Linux: xclip, xsel, or wl-copy (Wayland)
    // On macOS: pbcopy
    // On Windows: clip.exe

    #[cfg(target_os = "macos")]
    let clipboard_commands = [("pbcopy", &[] as &[&str])];

    #[cfg(target_os = "windows")]
    let clipboard_commands = [("clip.exe", &[] as &[&str])];

    #[cfg(target_os = "linux")]
    let clipboard_commands = [
        ("wl-copy", &[] as &[&str]),
        ("xclip", &["-selection", "clipboard"]),
        ("xsel", &["--clipboard", "--input"]),
    ];

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    let clipboard_commands: [(&str, &[&str]); 0] = [];

    for (cmd, args) in clipboard_commands {
        if let Ok(mut child) = Command::new(cmd)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            if let Some(ref mut stdin) = child.stdin {
                if stdin.write_all(text.as_bytes()).is_ok() {
                    if let Ok(status) = child.wait() {
                        if status.success() {
                            return Ok(());
                        }
                    }
                }
            }
        }
    }

    Err("No clipboard command available. Install xclip, xsel, or wl-copy.".to_string())
}

/// Check if clipboard contains an image and paste it to a temp file.
/// Returns the file path if successful.
fn paste_image_from_clipboard() -> Option<String> {
    use std::fs;
    use std::process::Command;
    use uuid::Uuid;

    // Try to detect if clipboard has image content
    // On Wayland: wl-paste --list-types
    // On X11: xclip -selection clipboard -t TARGETS -o

    #[cfg(target_os = "linux")]
    {
        // Try Wayland first (wl-paste)
        if let Ok(output) = Command::new("wl-paste").args(["--list-types"]).output() {
            if output.status.success() {
                let types = String::from_utf8_lossy(&output.stdout);
                let has_image = types.lines().any(|t| {
                    t == "image/png" || t == "image/jpeg" || t == "image/gif" || t == "image/webp"
                });
                if has_image {
                    // Paste the image
                    if let Ok(output) = Command::new("wl-paste")
                        .args(["--type", "image/png"])
                        .output()
                    {
                        if output.status.success() && !output.stdout.is_empty() {
                            let tmp_dir = std::env::temp_dir();
                            let file_name = format!("pi-clipboard-{}.png", Uuid::new_v4());
                            let file_path = tmp_dir.join(file_name);
                            if fs::write(&file_path, &output.stdout).is_ok() {
                                return Some(file_path.to_string_lossy().to_string());
                            }
                        }
                    }
                }
            }
        }

        // Try X11 (xclip)
        if let Ok(output) = Command::new("xclip")
            .args(["-selection", "clipboard", "-t", "TARGETS", "-o"])
            .output()
        {
            if output.status.success() {
                let types = String::from_utf8_lossy(&output.stdout);
                let has_image = types.lines().any(|t| {
                    t == "image/png" || t == "image/jpeg" || t == "image/gif" || t == "image/webp"
                });
                if has_image {
                    // Paste the image
                    if let Ok(output) = Command::new("xclip")
                        .args(["-selection", "clipboard", "-t", "image/png", "-o"])
                        .output()
                    {
                        if output.status.success() && !output.stdout.is_empty() {
                            let tmp_dir = std::env::temp_dir();
                            let file_name = format!("pi-clipboard-{}.png", Uuid::new_v4());
                            let file_path = tmp_dir.join(file_name);
                            if fs::write(&file_path, &output.stdout).is_ok() {
                                return Some(file_path.to_string_lossy().to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        // macOS: Use osascript to check clipboard type and pngpaste for image
        // First check if pngpaste is available
        if let Ok(output) = Command::new("pngpaste").args(["-"]).output() {
            if output.status.success() && !output.stdout.is_empty() {
                let tmp_dir = std::env::temp_dir();
                let file_name = format!("pi-clipboard-{}.png", Uuid::new_v4());
                let file_path = tmp_dir.join(file_name);
                if fs::write(&file_path, &output.stdout).is_ok() {
                    return Some(file_path.to_string_lossy().to_string());
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        // Windows: Use PowerShell to get clipboard image
        let script = r#"
            Add-Type -AssemblyName System.Windows.Forms
            $img = [System.Windows.Forms.Clipboard]::GetImage()
            if ($img -ne $null) {
                $ms = New-Object System.IO.MemoryStream
                $img.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
                [Convert]::ToBase64String($ms.ToArray())
            }
        "#;
        if let Ok(output) = Command::new("powershell")
            .args(["-Command", script])
            .output()
        {
            if output.status.success() {
                let base64_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !base64_str.is_empty() {
                    use base64::{engine::general_purpose, Engine};
                    if let Ok(data) = general_purpose::STANDARD.decode(&base64_str) {
                        let tmp_dir = std::env::temp_dir();
                        let file_name = format!("pi-clipboard-{}.png", Uuid::new_v4());
                        let file_path = tmp_dir.join(file_name);
                        if fs::write(&file_path, &data).is_ok() {
                            return Some(file_path.to_string_lossy().to_string());
                        }
                    }
                }
            }
        }
    }

    None
}
