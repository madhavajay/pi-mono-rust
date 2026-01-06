use base64::{engine::general_purpose, Engine as _};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

use crate::agent::AgentState;
use crate::core::session_manager::{SessionEntry, SessionHeader, SessionManager};

const DEFAULT_APP_NAME: &str = "pi";

const TEMPLATE_HTML: &str = include_str!("../assets/export-html/template.html");
const TEMPLATE_CSS: &str = include_str!("../assets/export-html/template.css");
const TEMPLATE_JS: &str = include_str!("../assets/export-html/template.js");
const MARKED_JS: &str = include_str!("../assets/export-html/vendor/marked.min.js");
const HIGHLIGHT_JS: &str = include_str!("../assets/export-html/vendor/highlight.min.js");

#[derive(Serialize)]
struct ExportTool {
    name: String,
    description: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionExportData {
    header: Option<SessionHeader>,
    entries: Vec<SessionEntry>,
    leaf_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ExportTool>>,
}

struct ExportColors {
    page_bg: &'static str,
    card_bg: &'static str,
    info_bg: &'static str,
}

const DEFAULT_EXPORT_COLORS: ExportColors = ExportColors {
    page_bg: "#18181e",
    card_bg: "#1e1e24",
    info_bg: "#3c3728",
};

const DEFAULT_TEXT_COLOR: &str = "#e5e5e7";

const DEFAULT_THEME_COLORS: &[(&str, &str)] = &[
    ("accent", "#8abeb7"),
    ("border", "#5f87ff"),
    ("borderAccent", "#00d7ff"),
    ("borderMuted", "#505050"),
    ("success", "#b5bd68"),
    ("error", "#cc6666"),
    ("warning", "#ffff00"),
    ("muted", "#808080"),
    ("dim", "#666666"),
    ("text", DEFAULT_TEXT_COLOR),
    ("thinkingText", "#808080"),
    ("selectedBg", "#3a3a4a"),
    ("userMessageBg", "#343541"),
    ("userMessageText", DEFAULT_TEXT_COLOR),
    ("customMessageBg", "#2d2838"),
    ("customMessageText", DEFAULT_TEXT_COLOR),
    ("customMessageLabel", "#9575cd"),
    ("toolPendingBg", "#282832"),
    ("toolSuccessBg", "#283228"),
    ("toolErrorBg", "#3c2828"),
    ("toolTitle", DEFAULT_TEXT_COLOR),
    ("toolOutput", "#808080"),
    ("mdHeading", "#f0c674"),
    ("mdLink", "#81a2be"),
    ("mdLinkUrl", "#666666"),
    ("mdCode", "#8abeb7"),
    ("mdCodeBlock", "#b5bd68"),
    ("mdCodeBlockBorder", "#808080"),
    ("mdQuote", "#808080"),
    ("mdQuoteBorder", "#808080"),
    ("mdHr", "#808080"),
    ("mdListBullet", "#8abeb7"),
    ("toolDiffAdded", "#b5bd68"),
    ("toolDiffRemoved", "#cc6666"),
    ("toolDiffContext", "#808080"),
    ("syntaxComment", "#6A9955"),
    ("syntaxKeyword", "#569CD6"),
    ("syntaxFunction", "#DCDCAA"),
    ("syntaxVariable", "#9CDCFE"),
    ("syntaxString", "#CE9178"),
    ("syntaxNumber", "#B5CEA8"),
    ("syntaxType", "#4EC9B0"),
    ("syntaxOperator", "#D4D4D4"),
    ("syntaxPunctuation", "#D4D4D4"),
    ("thinkingOff", "#505050"),
    ("thinkingMinimal", "#6e6e6e"),
    ("thinkingLow", "#5f87af"),
    ("thinkingMedium", "#81a2be"),
    ("thinkingHigh", "#b294bb"),
    ("thinkingXhigh", "#d183e8"),
    ("bashMode", "#b5bd68"),
];

fn default_theme_vars() -> String {
    let mut lines = Vec::new();
    for (key, value) in DEFAULT_THEME_COLORS {
        lines.push(format!("--{key}: {value};"));
    }
    lines.join("\n      ")
}

fn generate_html(session_data: &SessionExportData) -> Result<String, String> {
    let session_json = serde_json::to_string(session_data)
        .map_err(|err| format!("Failed to serialize session data: {err}"))?;
    let session_data_base64 = general_purpose::STANDARD.encode(session_json);

    let theme_vars = default_theme_vars();
    let css = TEMPLATE_CSS
        .replace("{{THEME_VARS}}", &theme_vars)
        .replace("{{BODY_BG}}", DEFAULT_EXPORT_COLORS.page_bg)
        .replace("{{CONTAINER_BG}}", DEFAULT_EXPORT_COLORS.card_bg)
        .replace("{{INFO_BG}}", DEFAULT_EXPORT_COLORS.info_bg);

    Ok(TEMPLATE_HTML
        .replace("{{CSS}}", &css)
        .replace("{{JS}}", TEMPLATE_JS)
        .replace("{{SESSION_DATA}}", &session_data_base64)
        .replace("{{MARKED_JS}}", MARKED_JS)
        .replace("{{HIGHLIGHT_JS}}", HIGHLIGHT_JS))
}

fn default_output_path(input_path: &Path) -> PathBuf {
    let basename = input_path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("session");
    let filename = format!("{DEFAULT_APP_NAME}-session-{basename}.html");
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(filename)
}

pub fn export_session_to_html(
    session_manager: &SessionManager,
    state: Option<&AgentState>,
    output_path: Option<PathBuf>,
) -> Result<PathBuf, String> {
    let session_file = session_manager
        .get_session_file()
        .ok_or_else(|| "Cannot export in-memory session to HTML".to_string())?;
    if !session_file.exists() {
        return Err("Nothing to export yet - start a conversation first".to_string());
    }

    let tools = state.map(|agent_state| {
        agent_state
            .tools
            .iter()
            .map(|tool| ExportTool {
                name: tool.name.clone(),
                description: tool.description.clone(),
            })
            .collect::<Vec<_>>()
    });

    let session_data = SessionExportData {
        header: session_manager.get_header(),
        entries: session_manager.get_entries(),
        leaf_id: session_manager.get_leaf_id(),
        system_prompt: state.map(|agent_state| agent_state.system_prompt.clone()),
        tools,
    };

    let html = generate_html(&session_data)?;
    let output = output_path.unwrap_or_else(|| default_output_path(&session_file));

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("Failed to create export directory: {err}"))?;
    }
    fs::write(&output, html).map_err(|err| format!("Failed to write HTML export: {err}"))?;
    Ok(output)
}

pub fn export_from_file(
    input_path: &Path,
    output_path: Option<PathBuf>,
) -> Result<PathBuf, String> {
    if !input_path.exists() {
        return Err(format!("File not found: {}", input_path.display()));
    }
    let session_manager = SessionManager::open(input_path.to_path_buf(), None);
    export_session_to_html(&session_manager, None, output_path)
}
