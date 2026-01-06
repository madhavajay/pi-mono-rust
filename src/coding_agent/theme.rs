use crate::config;
use crate::tui::{EditorTheme, MarkdownTheme};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Mutex, OnceLock, RwLock};
use std::thread;
use std::time::Duration;

const BUILTIN_DARK: &str = include_str!("../assets/themes/dark.json");
const BUILTIN_LIGHT: &str = include_str!("../assets/themes/light.json");

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ThemeColor {
    Accent,
    Border,
    BorderAccent,
    BorderMuted,
    Success,
    Error,
    Warning,
    Muted,
    Dim,
    Text,
    ThinkingText,
    UserMessageText,
    CustomMessageText,
    CustomMessageLabel,
    ToolTitle,
    ToolOutput,
    MdHeading,
    MdLink,
    MdLinkUrl,
    MdCode,
    MdCodeBlock,
    MdCodeBlockBorder,
    MdQuote,
    MdQuoteBorder,
    MdHr,
    MdListBullet,
    ToolDiffAdded,
    ToolDiffRemoved,
    ToolDiffContext,
    SyntaxComment,
    SyntaxKeyword,
    SyntaxFunction,
    SyntaxVariable,
    SyntaxString,
    SyntaxNumber,
    SyntaxType,
    SyntaxOperator,
    SyntaxPunctuation,
    ThinkingOff,
    ThinkingMinimal,
    ThinkingLow,
    ThinkingMedium,
    ThinkingHigh,
    ThinkingXhigh,
    BashMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ThemeBg {
    SelectedBg,
    UserMessageBg,
    CustomMessageBg,
    ToolPendingBg,
    ToolSuccessBg,
    ToolErrorBg,
}

const THEME_COLORS: &[(&str, ThemeColor)] = &[
    ("accent", ThemeColor::Accent),
    ("border", ThemeColor::Border),
    ("borderAccent", ThemeColor::BorderAccent),
    ("borderMuted", ThemeColor::BorderMuted),
    ("success", ThemeColor::Success),
    ("error", ThemeColor::Error),
    ("warning", ThemeColor::Warning),
    ("muted", ThemeColor::Muted),
    ("dim", ThemeColor::Dim),
    ("text", ThemeColor::Text),
    ("thinkingText", ThemeColor::ThinkingText),
    ("userMessageText", ThemeColor::UserMessageText),
    ("customMessageText", ThemeColor::CustomMessageText),
    ("customMessageLabel", ThemeColor::CustomMessageLabel),
    ("toolTitle", ThemeColor::ToolTitle),
    ("toolOutput", ThemeColor::ToolOutput),
    ("mdHeading", ThemeColor::MdHeading),
    ("mdLink", ThemeColor::MdLink),
    ("mdLinkUrl", ThemeColor::MdLinkUrl),
    ("mdCode", ThemeColor::MdCode),
    ("mdCodeBlock", ThemeColor::MdCodeBlock),
    ("mdCodeBlockBorder", ThemeColor::MdCodeBlockBorder),
    ("mdQuote", ThemeColor::MdQuote),
    ("mdQuoteBorder", ThemeColor::MdQuoteBorder),
    ("mdHr", ThemeColor::MdHr),
    ("mdListBullet", ThemeColor::MdListBullet),
    ("toolDiffAdded", ThemeColor::ToolDiffAdded),
    ("toolDiffRemoved", ThemeColor::ToolDiffRemoved),
    ("toolDiffContext", ThemeColor::ToolDiffContext),
    ("syntaxComment", ThemeColor::SyntaxComment),
    ("syntaxKeyword", ThemeColor::SyntaxKeyword),
    ("syntaxFunction", ThemeColor::SyntaxFunction),
    ("syntaxVariable", ThemeColor::SyntaxVariable),
    ("syntaxString", ThemeColor::SyntaxString),
    ("syntaxNumber", ThemeColor::SyntaxNumber),
    ("syntaxType", ThemeColor::SyntaxType),
    ("syntaxOperator", ThemeColor::SyntaxOperator),
    ("syntaxPunctuation", ThemeColor::SyntaxPunctuation),
    ("thinkingOff", ThemeColor::ThinkingOff),
    ("thinkingMinimal", ThemeColor::ThinkingMinimal),
    ("thinkingLow", ThemeColor::ThinkingLow),
    ("thinkingMedium", ThemeColor::ThinkingMedium),
    ("thinkingHigh", ThemeColor::ThinkingHigh),
    ("thinkingXhigh", ThemeColor::ThinkingXhigh),
    ("bashMode", ThemeColor::BashMode),
];

const THEME_BACKGROUNDS: &[(&str, ThemeBg)] = &[
    ("selectedBg", ThemeBg::SelectedBg),
    ("userMessageBg", ThemeBg::UserMessageBg),
    ("customMessageBg", ThemeBg::CustomMessageBg),
    ("toolPendingBg", ThemeBg::ToolPendingBg),
    ("toolSuccessBg", ThemeBg::ToolSuccessBg),
    ("toolErrorBg", ThemeBg::ToolErrorBg),
];

#[derive(Clone, Copy, Debug)]
enum ColorMode {
    TrueColor,
    Color256,
}

#[derive(Clone, Debug)]
pub struct Theme {
    fg: HashMap<ThemeColor, String>,
    bg: HashMap<ThemeBg, String>,
    name: String,
}

impl Theme {
    pub fn fg(&self, color: ThemeColor, text: &str) -> String {
        let ansi = self
            .fg
            .get(&color)
            .map(String::as_str)
            .unwrap_or("\x1b[39m");
        format!("{ansi}{text}\x1b[39m")
    }

    pub fn bg(&self, color: ThemeBg, text: &str) -> String {
        let ansi = self
            .bg
            .get(&color)
            .map(String::as_str)
            .unwrap_or("\x1b[49m");
        format!("{ansi}{text}\x1b[49m")
    }

    pub fn bold(&self, text: &str) -> String {
        format!("\x1b[1m{text}\x1b[22m")
    }

    pub fn italic(&self, text: &str) -> String {
        format!("\x1b[3m{text}\x1b[23m")
    }

    pub fn underline(&self, text: &str) -> String {
        format!("\x1b[4m{text}\x1b[24m")
    }

    pub fn strikethrough(&self, text: &str) -> String {
        format!("\x1b[9m{text}\x1b[29m")
    }

    pub fn fg_ansi(&self, color: ThemeColor) -> String {
        self.fg
            .get(&color)
            .cloned()
            .unwrap_or_else(|| "\x1b[39m".to_string())
    }

    pub fn bg_ansi(&self, color: ThemeBg) -> String {
        self.bg
            .get(&color)
            .cloned()
            .unwrap_or_else(|| "\x1b[49m".to_string())
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn editor_theme(&self) -> EditorTheme {
        EditorTheme {
            border_color: editor_border_color,
        }
    }

    pub fn markdown_theme(&self) -> Box<dyn MarkdownTheme> {
        Box::new(ThemeMarkdown {
            theme: self.clone(),
        })
    }
}

struct ThemeMarkdown {
    theme: Theme,
}

impl MarkdownTheme for ThemeMarkdown {
    fn heading(&self, text: &str) -> String {
        self.theme.fg(ThemeColor::MdHeading, text)
    }

    fn link(&self, text: &str) -> String {
        self.theme.fg(ThemeColor::MdLink, text)
    }

    fn link_url(&self, text: &str) -> String {
        self.theme.fg(ThemeColor::MdLinkUrl, text)
    }

    fn code(&self, text: &str) -> String {
        self.theme.fg(ThemeColor::MdCode, text)
    }

    fn code_block(&self, text: &str) -> String {
        self.theme.fg(ThemeColor::MdCodeBlock, text)
    }

    fn code_block_border(&self, text: &str) -> String {
        self.theme.fg(ThemeColor::MdCodeBlockBorder, text)
    }

    fn quote(&self, text: &str) -> String {
        self.theme.fg(ThemeColor::MdQuote, text)
    }

    fn quote_border(&self, text: &str) -> String {
        self.theme.fg(ThemeColor::MdQuoteBorder, text)
    }

    fn hr(&self, text: &str) -> String {
        self.theme.fg(ThemeColor::MdHr, text)
    }

    fn list_bullet(&self, text: &str) -> String {
        self.theme.fg(ThemeColor::MdListBullet, text)
    }

    fn bold(&self, text: &str) -> String {
        self.theme.bold(text)
    }

    fn italic(&self, text: &str) -> String {
        self.theme.italic(text)
    }

    fn strikethrough(&self, text: &str) -> String {
        self.theme.strikethrough(text)
    }

    fn underline(&self, text: &str) -> String {
        self.theme.underline(text)
    }
}

#[derive(Debug, Deserialize)]
struct ThemeJson {
    name: String,
    #[serde(default)]
    vars: HashMap<String, ColorValue>,
    colors: HashMap<String, ColorValue>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
enum ColorValue {
    Text(String),
    Number(u8),
}

#[derive(Clone, Copy, Debug)]
enum ResolvedColor {
    Hex { r: u8, g: u8, b: u8 },
    Index(u8),
    Default,
}

pub fn load_theme(name: Option<&str>) -> Result<Theme, String> {
    let name = name
        .map(str::to_string)
        .unwrap_or_else(|| default_theme_name().to_string());
    let theme_json = load_theme_json(&name)?;
    let resolved = resolve_theme_colors(&theme_json)?;
    let mode = detect_color_mode();
    Ok(build_theme(name, resolved, mode))
}

pub fn load_theme_or_default(name: Option<&str>) -> Theme {
    match load_theme(name) {
        Ok(theme) => theme,
        Err(_) => load_theme(Some("dark")).unwrap_or_else(|_| fallback_theme()),
    }
}

pub fn available_themes() -> Vec<String> {
    let mut names = vec!["dark".to_string(), "light".to_string()];
    let custom_dir = config::get_agent_dir().join("themes");
    if let Ok(entries) = fs::read_dir(custom_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(stem) = path.file_stem().and_then(|value| value.to_str()) {
                if path.extension().and_then(|value| value.to_str()) == Some("json")
                    && !names.iter().any(|name| name == stem)
                {
                    names.push(stem.to_string());
                }
            }
        }
    }
    names.sort();
    names
}

pub fn set_active_theme(theme: Theme) {
    let lock = ACTIVE_THEME.get_or_init(|| RwLock::new(theme.clone()));
    *lock.write().unwrap() = theme;
}

fn load_theme_json(name: &str) -> Result<ThemeJson, String> {
    match name {
        "dark" => serde_json::from_str(BUILTIN_DARK)
            .map_err(|err| format!("Failed to parse theme {name}: {err}")),
        "light" => serde_json::from_str(BUILTIN_LIGHT)
            .map_err(|err| format!("Failed to parse theme {name}: {err}")),
        _ => {
            let custom_path = custom_theme_path(name);
            let content = fs::read_to_string(&custom_path)
                .map_err(|err| format!("Failed to read theme {}: {err}", custom_path.display()))?;
            serde_json::from_str(&content)
                .map_err(|err| format!("Failed to parse theme {name}: {err}"))
        }
    }
}

fn custom_theme_path(name: &str) -> PathBuf {
    config::get_agent_dir()
        .join("themes")
        .join(format!("{name}.json"))
}

fn resolve_theme_colors(theme: &ThemeJson) -> Result<HashMap<String, ResolvedColor>, String> {
    let mut missing = Vec::new();
    for (key, _) in THEME_COLORS.iter() {
        if !theme.colors.contains_key(*key) {
            missing.push(*key);
        }
    }
    for (key, _) in THEME_BACKGROUNDS.iter() {
        if !theme.colors.contains_key(*key) {
            missing.push(*key);
        }
    }
    if !missing.is_empty() {
        return Err(format!(
            "Invalid theme \"{}\": missing required colors: {}",
            theme.name,
            missing.join(", ")
        ));
    }

    let mut resolved = HashMap::new();
    for (key, value) in &theme.colors {
        let color = resolve_color_value(value, &theme.vars, &mut HashSet::new())?;
        resolved.insert(key.clone(), color);
    }
    Ok(resolved)
}

fn resolve_color_value(
    value: &ColorValue,
    vars: &HashMap<String, ColorValue>,
    visited: &mut HashSet<String>,
) -> Result<ResolvedColor, String> {
    match value {
        ColorValue::Number(index) => Ok(ResolvedColor::Index(*index)),
        ColorValue::Text(value) => {
            if value.is_empty() {
                return Ok(ResolvedColor::Default);
            }
            if let Some(hex) = value.strip_prefix('#') {
                return parse_hex(hex);
            }
            if visited.contains(value) {
                return Err(format!("Circular variable reference detected: {value}"));
            }
            let Some(next) = vars.get(value) else {
                return Err(format!("Variable reference not found: {value}"));
            };
            visited.insert(value.clone());
            resolve_color_value(next, vars, visited)
        }
    }
}

fn parse_hex(hex: &str) -> Result<ResolvedColor, String> {
    if hex.len() != 6 {
        return Err(format!("Invalid hex color: #{hex}"));
    }
    let r = u8::from_str_radix(&hex[0..2], 16).map_err(|_| format!("Invalid hex color: #{hex}"))?;
    let g = u8::from_str_radix(&hex[2..4], 16).map_err(|_| format!("Invalid hex color: #{hex}"))?;
    let b = u8::from_str_radix(&hex[4..6], 16).map_err(|_| format!("Invalid hex color: #{hex}"))?;
    Ok(ResolvedColor::Hex { r, g, b })
}

fn build_theme(name: String, colors: HashMap<String, ResolvedColor>, mode: ColorMode) -> Theme {
    let mut fg = HashMap::new();
    let mut bg = HashMap::new();
    for (key, color) in colors {
        if let Some((_, fg_key)) = THEME_COLORS.iter().find(|(k, _)| *k == key) {
            fg.insert(*fg_key, fg_ansi(color, mode));
        } else if let Some((_, bg_key)) = THEME_BACKGROUNDS.iter().find(|(k, _)| *k == key) {
            bg.insert(*bg_key, bg_ansi(color, mode));
        }
    }

    Theme { fg, bg, name }
}

fn detect_color_mode() -> ColorMode {
    if let Ok(colorterm) = env::var("COLORTERM") {
        if colorterm == "truecolor" || colorterm == "24bit" {
            return ColorMode::TrueColor;
        }
    }
    if env::var("WT_SESSION").is_ok() {
        return ColorMode::TrueColor;
    }
    let term = env::var("TERM").unwrap_or_default();
    if term.is_empty() || term == "dumb" || term == "linux" {
        return ColorMode::Color256;
    }
    ColorMode::TrueColor
}

fn default_theme_name() -> &'static str {
    let Ok(colorfgbg) = env::var("COLORFGBG") else {
        return "dark";
    };
    let parts: Vec<&str> = colorfgbg.split(';').collect();
    if parts.len() >= 2 {
        if let Ok(bg) = parts[1].parse::<i32>() {
            return if bg < 8 { "dark" } else { "light" };
        }
    }
    "dark"
}

fn fg_ansi(color: ResolvedColor, mode: ColorMode) -> String {
    match color {
        ResolvedColor::Default => "\x1b[39m".to_string(),
        ResolvedColor::Index(index) => format!("\x1b[38;5;{index}m"),
        ResolvedColor::Hex { r, g, b } => match mode {
            ColorMode::TrueColor => format!("\x1b[38;2;{r};{g};{b}m"),
            ColorMode::Color256 => {
                let index = rgb_to_256(r, g, b);
                format!("\x1b[38;5;{index}m")
            }
        },
    }
}

fn bg_ansi(color: ResolvedColor, mode: ColorMode) -> String {
    match color {
        ResolvedColor::Default => "\x1b[49m".to_string(),
        ResolvedColor::Index(index) => format!("\x1b[48;5;{index}m"),
        ResolvedColor::Hex { r, g, b } => match mode {
            ColorMode::TrueColor => format!("\x1b[48;2;{r};{g};{b}m"),
            ColorMode::Color256 => {
                let index = rgb_to_256(r, g, b);
                format!("\x1b[48;5;{index}m")
            }
        },
    }
}

const CUBE_VALUES: [u8; 6] = [0, 95, 135, 175, 215, 255];

fn gray_values() -> [u8; 24] {
    let mut values = [0u8; 24];
    let mut idx = 0;
    while idx < values.len() {
        values[idx] = 8 + (idx as u8) * 10;
        idx += 1;
    }
    values
}

fn rgb_to_256(r: u8, g: u8, b: u8) -> u8 {
    let (r_idx, cube_r) = closest_cube_index(r);
    let (g_idx, cube_g) = closest_cube_index(g);
    let (b_idx, cube_b) = closest_cube_index(b);
    let cube_index = 16 + 36 * r_idx + 6 * g_idx + b_idx;
    let cube_dist = color_distance(r, g, b, cube_r, cube_g, cube_b);

    let gray = ((0.299 * r as f32) + (0.587 * g as f32) + (0.114 * b as f32)).round() as u8;
    let (gray_idx, gray_val) = closest_gray_index(gray);
    let gray_index = 232 + gray_idx;
    let gray_dist = color_distance(r, g, b, gray_val, gray_val, gray_val);

    let max_c = r.max(g).max(b);
    let min_c = r.min(g).min(b);
    let spread = max_c - min_c;

    if spread < 10 && gray_dist < cube_dist {
        return gray_index;
    }

    cube_index
}

fn closest_cube_index(value: u8) -> (u8, u8) {
    let mut best_idx = 0;
    let mut best_dist = i32::MAX;
    for (idx, cube) in CUBE_VALUES.iter().enumerate() {
        let dist = (i32::from(value) - i32::from(*cube)).abs();
        if dist < best_dist {
            best_idx = idx;
            best_dist = dist;
        }
    }
    (best_idx as u8, CUBE_VALUES[best_idx])
}

fn closest_gray_index(value: u8) -> (u8, u8) {
    let values = gray_values();
    let mut best_idx = 0;
    let mut best_dist = i32::MAX;
    for (idx, gray) in values.iter().enumerate() {
        let dist = (i32::from(value) - i32::from(*gray)).abs();
        if dist < best_dist {
            best_idx = idx;
            best_dist = dist;
        }
    }
    (best_idx as u8, values[best_idx])
}

fn color_distance(r1: u8, g1: u8, b1: u8, r2: u8, g2: u8, b2: u8) -> f32 {
    let dr = (r1 as f32) - (r2 as f32);
    let dg = (g1 as f32) - (g2 as f32);
    let db = (b1 as f32) - (b2 as f32);
    dr * dr * 0.299 + dg * dg * 0.587 + db * db * 0.114
}

fn fallback_theme() -> Theme {
    Theme {
        fg: HashMap::new(),
        bg: HashMap::new(),
        name: "fallback".to_string(),
    }
}

static ACTIVE_THEME: OnceLock<RwLock<Theme>> = OnceLock::new();
static CURRENT_THEME_NAME: OnceLock<RwLock<String>> = OnceLock::new();
static THEME_WATCHER: OnceLock<Mutex<Option<ThemeWatcher>>> = OnceLock::new();

type ThemeChangeCallback = Box<dyn Fn() + Send + Sync>;
static THEME_CHANGE_CALLBACK: OnceLock<Mutex<Option<ThemeChangeCallback>>> = OnceLock::new();

struct ThemeWatcher {
    _watcher: RecommendedWatcher,
    stop_tx: mpsc::Sender<()>,
}

fn editor_border_color(text: &str) -> String {
    if let Some(lock) = ACTIVE_THEME.get() {
        if let Ok(theme) = lock.read() {
            return theme.fg(ThemeColor::BorderMuted, text);
        }
    }
    text.to_string()
}

/// Set the callback to be called when the theme file changes.
pub fn on_theme_change<F: Fn() + Send + Sync + 'static>(callback: F) {
    let lock = THEME_CHANGE_CALLBACK.get_or_init(|| Mutex::new(None));
    *lock.lock().unwrap() = Some(Box::new(callback));
}

/// Start watching the current theme file for changes.
/// Only watches custom themes (not built-in dark/light).
pub fn start_theme_watcher() {
    let current_name = {
        let lock = CURRENT_THEME_NAME.get_or_init(|| RwLock::new("dark".to_string()));
        lock.read().unwrap().clone()
    };

    // Only watch custom themes, not built-ins
    if current_name == "dark" || current_name == "light" {
        return;
    }

    let theme_path = custom_theme_path(&current_name);
    if !theme_path.exists() {
        return;
    }

    // Stop existing watcher
    stop_theme_watcher();

    let (stop_tx, stop_rx) = mpsc::channel();

    // Create the notify watcher with a debounced event handler
    let (event_tx, event_rx) = mpsc::channel();

    let watcher_result = notify::recommended_watcher(move |res: Result<notify::Event, _>| {
        if let Ok(event) = res {
            let _ = event_tx.send(event);
        }
    });

    let Ok(mut watcher) = watcher_result else {
        return;
    };

    // Watch the theme file
    if watcher
        .watch(&theme_path, RecursiveMode::NonRecursive)
        .is_err()
    {
        return;
    }

    // Spawn a thread to handle events with debouncing
    let theme_name = current_name.clone();
    let path_clone = theme_path.clone();
    thread::spawn(move || {
        let debounce_duration = Duration::from_millis(100);
        let mut last_event_time = std::time::Instant::now()
            .checked_sub(debounce_duration)
            .unwrap_or_else(std::time::Instant::now);

        loop {
            // Check for stop signal
            if stop_rx.try_recv().is_ok() {
                break;
            }

            // Check for file events with timeout
            match event_rx.recv_timeout(Duration::from_millis(50)) {
                Ok(event) => {
                    // Only handle modify and remove events
                    let should_handle = matches!(
                        event.kind,
                        EventKind::Modify(_) | EventKind::Remove(_) | EventKind::Create(_)
                    );

                    if should_handle {
                        let now = std::time::Instant::now();
                        if now.duration_since(last_event_time) >= debounce_duration {
                            last_event_time = now;

                            // Check if file was deleted
                            if matches!(event.kind, EventKind::Remove(_)) || !path_clone.exists() {
                                // Fall back to dark theme
                                if let Ok(dark_theme) = load_theme(Some("dark")) {
                                    set_active_theme(dark_theme);
                                    {
                                        let lock = CURRENT_THEME_NAME
                                            .get_or_init(|| RwLock::new("dark".to_string()));
                                        *lock.write().unwrap() = "dark".to_string();
                                    }
                                    // Notify callback
                                    if let Some(lock) = THEME_CHANGE_CALLBACK.get() {
                                        if let Ok(guard) = lock.lock() {
                                            if let Some(callback) = guard.as_ref() {
                                                callback();
                                            }
                                        }
                                    }
                                }
                                // Stop watching since file is gone
                                break;
                            }

                            // Try to reload the theme
                            if let Ok(new_theme) = load_theme(Some(&theme_name)) {
                                set_active_theme(new_theme);
                                // Notify callback
                                if let Some(lock) = THEME_CHANGE_CALLBACK.get() {
                                    if let Ok(guard) = lock.lock() {
                                        if let Some(callback) = guard.as_ref() {
                                            callback();
                                        }
                                    }
                                }
                            }
                            // If reload fails, silently ignore (file might be mid-edit)
                        }
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });

    let theme_watcher = ThemeWatcher {
        _watcher: watcher,
        stop_tx,
    };

    let lock = THEME_WATCHER.get_or_init(|| Mutex::new(None));
    *lock.lock().unwrap() = Some(theme_watcher);
}

/// Stop watching the theme file.
pub fn stop_theme_watcher() {
    if let Some(lock) = THEME_WATCHER.get() {
        let mut guard = lock.lock().unwrap();
        if let Some(watcher) = guard.take() {
            // Send stop signal
            let _ = watcher.stop_tx.send(());
        }
    }
}

/// Set the theme and optionally enable the file watcher.
/// Returns Ok(()) on success, or an error message if the theme is invalid.
pub fn set_theme(name: &str, enable_watcher: bool) -> Result<(), String> {
    let theme = load_theme(Some(name))?;

    {
        let lock = CURRENT_THEME_NAME.get_or_init(|| RwLock::new("dark".to_string()));
        *lock.write().unwrap() = name.to_string();
    }

    set_active_theme(theme);

    if enable_watcher {
        start_theme_watcher();
    }

    Ok(())
}

/// Initialize the theme system with an optional theme name and watcher.
pub fn init_theme(theme_name: Option<&str>, enable_watcher: bool) {
    let name = theme_name.unwrap_or_else(|| default_theme_name());

    {
        let lock = CURRENT_THEME_NAME.get_or_init(|| RwLock::new("dark".to_string()));
        *lock.write().unwrap() = name.to_string();
    }

    let theme = match load_theme(Some(name)) {
        Ok(t) => t,
        Err(_) => {
            // Fall back to dark theme silently
            let lock = CURRENT_THEME_NAME.get_or_init(|| RwLock::new("dark".to_string()));
            *lock.write().unwrap() = "dark".to_string();
            load_theme(Some("dark")).unwrap_or_else(|_| fallback_theme())
        }
    };

    set_active_theme(theme);

    if enable_watcher {
        start_theme_watcher();
    }
}

/// Get the currently active theme.
pub fn get_active_theme() -> Option<Theme> {
    ACTIVE_THEME
        .get()
        .and_then(|lock| lock.read().ok().map(|t| t.clone()))
}
