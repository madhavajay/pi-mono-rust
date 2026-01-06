use crate::core::messages::{
    create_branch_summary_message, create_compaction_summary_message, create_hook_message,
    AgentMessage, UserContent,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub const CURRENT_SESSION_VERSION: i64 = 2;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionHeader {
    pub id: String,
    pub timestamp: String,
    pub cwd: String,
    #[serde(default)]
    pub version: Option<i64>,
    #[serde(default)]
    pub parent_session: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMessageEntry {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    pub timestamp: String,
    pub message: AgentMessage,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThinkingLevelChangeEntry {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    pub timestamp: String,
    pub thinking_level: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelChangeEntry {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    pub timestamp: String,
    pub provider: String,
    pub model_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionEntry {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    pub timestamp: String,
    pub summary: String,
    #[serde(default)]
    pub first_kept_entry_id: String,
    #[serde(default)]
    pub first_kept_entry_index: Option<usize>,
    pub tokens_before: i64,
    #[serde(default)]
    pub details: Option<Value>,
    #[serde(default)]
    pub from_hook: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchSummaryEntry {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    pub timestamp: String,
    pub from_id: String,
    pub summary: String,
    #[serde(default)]
    pub details: Option<Value>,
    #[serde(default)]
    pub from_hook: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomEntry {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    pub timestamp: String,
    pub custom_type: String,
    #[serde(default)]
    pub data: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomMessageEntry {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    pub timestamp: String,
    pub custom_type: String,
    pub content: UserContent,
    pub display: bool,
    #[serde(default)]
    pub details: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelEntry {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    pub timestamp: String,
    pub target_id: String,
    #[serde(default)]
    pub label: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionEntry {
    Message(SessionMessageEntry),
    ThinkingLevelChange(ThinkingLevelChangeEntry),
    ModelChange(ModelChangeEntry),
    Compaction(CompactionEntry),
    BranchSummary(BranchSummaryEntry),
    Custom(CustomEntry),
    CustomMessage(CustomMessageEntry),
    Label(LabelEntry),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FileEntry {
    Session(SessionHeader),
    Message(SessionMessageEntry),
    ThinkingLevelChange(ThinkingLevelChangeEntry),
    ModelChange(ModelChangeEntry),
    Compaction(CompactionEntry),
    BranchSummary(BranchSummaryEntry),
    Custom(CustomEntry),
    CustomMessage(CustomMessageEntry),
    Label(LabelEntry),
}

#[derive(Clone, Debug, PartialEq)]
pub struct SessionTreeNode {
    pub entry: SessionEntry,
    pub children: Vec<SessionTreeNode>,
    pub label: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SessionContext {
    pub messages: Vec<AgentMessage>,
    pub thinking_level: String,
    pub model: Option<ModelRef>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModelRef {
    pub provider: String,
    pub model_id: String,
}

pub fn get_latest_compaction_entry(entries: &[SessionEntry]) -> Option<CompactionEntry> {
    for entry in entries.iter().rev() {
        if let SessionEntry::Compaction(compaction) = entry {
            return Some(compaction.clone());
        }
    }
    None
}

pub fn build_session_context(entries: &[SessionEntry], leaf_id: Option<&str>) -> SessionContext {
    if entries.is_empty() {
        return SessionContext {
            messages: Vec::new(),
            thinking_level: "off".to_string(),
            model: None,
        };
    }

    let mut by_id: HashMap<String, SessionEntry> = HashMap::new();
    for entry in entries {
        by_id.insert(entry.id().to_string(), entry.clone());
    }

    let mut leaf: Option<SessionEntry> = None;
    if let Some(id) = leaf_id {
        leaf = by_id.get(id).cloned();
    }
    if leaf.is_none() {
        leaf = entries.last().cloned();
    }
    let leaf = match leaf {
        Some(entry) => entry,
        None => {
            return SessionContext {
                messages: Vec::new(),
                thinking_level: "off".to_string(),
                model: None,
            };
        }
    };

    let mut path: Vec<SessionEntry> = Vec::new();
    let mut current: Option<SessionEntry> = Some(leaf);
    while let Some(entry) = current {
        path.insert(0, entry.clone());
        let parent_id = entry.parent_id().map(|s| s.to_string());
        current = parent_id.and_then(|id| by_id.get(&id).cloned());
    }

    let mut thinking_level = "off".to_string();
    let mut model: Option<ModelRef> = None;
    let mut compaction: Option<CompactionEntry> = None;

    for entry in &path {
        match entry {
            SessionEntry::ThinkingLevelChange(change) => {
                thinking_level = change.thinking_level.clone();
            }
            SessionEntry::ModelChange(change) => {
                model = Some(ModelRef {
                    provider: change.provider.clone(),
                    model_id: change.model_id.clone(),
                });
            }
            SessionEntry::Message(message) => {
                if let AgentMessage::Assistant(assistant) = &message.message {
                    model = Some(ModelRef {
                        provider: assistant.provider.clone(),
                        model_id: assistant.model.clone(),
                    });
                }
            }
            SessionEntry::Compaction(compaction_entry) => {
                compaction = Some(compaction_entry.clone());
            }
            _ => {}
        }
    }

    let mut messages: Vec<AgentMessage> = Vec::new();

    let append_entry = |entry: &SessionEntry, messages: &mut Vec<AgentMessage>| match entry {
        SessionEntry::Message(message) => {
            messages.push(message.message.clone());
        }
        SessionEntry::CustomMessage(custom_message) => {
            messages.push(create_hook_message(
                &custom_message.custom_type,
                custom_message.content.clone(),
                custom_message.display,
                custom_message.details.clone(),
                &custom_message.timestamp,
            ));
        }
        SessionEntry::BranchSummary(branch_summary) => {
            messages.push(create_branch_summary_message(
                &branch_summary.summary,
                &branch_summary.from_id,
                &branch_summary.timestamp,
            ));
        }
        _ => {}
    };

    if let Some(compaction_entry) = compaction {
        messages.push(create_compaction_summary_message(
            &compaction_entry.summary,
            compaction_entry.tokens_before,
            &compaction_entry.timestamp,
        ));

        let compaction_idx = path.iter().position(
            |entry| matches!(entry, SessionEntry::Compaction(c) if c.id == compaction_entry.id),
        );

        if let Some(compaction_idx) = compaction_idx {
            let mut found_first_kept = false;
            for entry in path.iter().take(compaction_idx) {
                if entry.id() == compaction_entry.first_kept_entry_id {
                    found_first_kept = true;
                }
                if found_first_kept {
                    append_entry(entry, &mut messages);
                }
            }
            for entry in path.iter().skip(compaction_idx + 1) {
                append_entry(entry, &mut messages);
            }
        }
    } else {
        for entry in &path {
            append_entry(entry, &mut messages);
        }
    }

    SessionContext {
        messages,
        thinking_level,
        model,
    }
}

pub fn load_entries_from_file(path: &Path) -> Vec<FileEntry> {
    if !path.exists() {
        return Vec::new();
    }

    let file = match File::open(path) {
        Ok(file) => file,
        Err(_) => return Vec::new(),
    };

    let reader = BufReader::new(file);
    let mut entries: Vec<FileEntry> = Vec::new();

    for line in reader.lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<FileEntry>(&line) {
            entries.push(entry);
        }
    }

    if entries.is_empty() {
        return entries;
    }

    match entries.first() {
        Some(FileEntry::Session(header)) if !header.id.is_empty() => entries,
        _ => Vec::new(),
    }
}

pub fn find_most_recent_session(session_dir: &Path) -> Option<PathBuf> {
    let mut candidates: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();

    let entries = fs::read_dir(session_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }
        if !is_valid_session_file(&path) {
            continue;
        }
        if let Ok(metadata) = fs::metadata(&path) {
            if let Ok(modified) = metadata.modified() {
                candidates.push((path, modified));
            }
        }
    }

    candidates.sort_by_key(|(_, modified)| *modified);
    candidates.last().map(|(path, _)| path.clone())
}

pub fn migrate_session_entries(entries: &mut [FileEntry]) {
    let header_version = entries.iter().find_map(|entry| match entry {
        FileEntry::Session(header) => header.version,
        _ => None,
    });

    if header_version.unwrap_or(1) >= CURRENT_SESSION_VERSION {
        return;
    }

    migrate_v1_to_v2(entries);
}

fn migrate_v1_to_v2(entries: &mut [FileEntry]) {
    let mut ids: HashSet<String> = HashSet::new();
    let mut prev_id: Option<String> = None;

    for entry in entries.iter_mut() {
        match entry {
            FileEntry::Session(header) => {
                header.version = Some(CURRENT_SESSION_VERSION);
            }
            FileEntry::Message(message) => {
                apply_migration_ids(
                    &mut message.id,
                    &mut message.parent_id,
                    &mut prev_id,
                    &mut ids,
                );
            }
            FileEntry::ThinkingLevelChange(change) => {
                apply_migration_ids(
                    &mut change.id,
                    &mut change.parent_id,
                    &mut prev_id,
                    &mut ids,
                );
            }
            FileEntry::ModelChange(change) => {
                apply_migration_ids(
                    &mut change.id,
                    &mut change.parent_id,
                    &mut prev_id,
                    &mut ids,
                );
            }
            FileEntry::Compaction(compaction) => {
                apply_migration_ids(
                    &mut compaction.id,
                    &mut compaction.parent_id,
                    &mut prev_id,
                    &mut ids,
                );
            }
            FileEntry::BranchSummary(summary) => {
                apply_migration_ids(
                    &mut summary.id,
                    &mut summary.parent_id,
                    &mut prev_id,
                    &mut ids,
                );
            }
            FileEntry::Custom(custom) => {
                apply_migration_ids(
                    &mut custom.id,
                    &mut custom.parent_id,
                    &mut prev_id,
                    &mut ids,
                );
            }
            FileEntry::CustomMessage(custom_message) => {
                apply_migration_ids(
                    &mut custom_message.id,
                    &mut custom_message.parent_id,
                    &mut prev_id,
                    &mut ids,
                );
            }
            FileEntry::Label(label) => {
                apply_migration_ids(&mut label.id, &mut label.parent_id, &mut prev_id, &mut ids);
            }
        }
    }

    let id_by_index: Vec<Option<String>> = entries
        .iter()
        .map(|entry| entry.session_id().map(|id| id.to_string()))
        .collect();

    for entry in entries.iter_mut() {
        if let FileEntry::Compaction(compaction) = entry {
            if compaction.first_kept_entry_id.is_empty() {
                if let Some(idx) = compaction.first_kept_entry_index {
                    if let Some(id) = id_by_index.get(idx).and_then(|value| value.clone()) {
                        compaction.first_kept_entry_id = id;
                    }
                }
            }
            compaction.first_kept_entry_index = None;
        }
    }
}

fn apply_migration_ids(
    id: &mut String,
    parent_id: &mut Option<String>,
    prev_id: &mut Option<String>,
    ids: &mut HashSet<String>,
) {
    if id.is_empty() {
        *id = generate_id(ids);
    }
    if parent_id.is_none() {
        *parent_id = prev_id.clone();
    }
    *prev_id = Some(id.clone());
}

fn is_valid_session_file(path: &Path) -> bool {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(_) => return false,
    };
    let mut buf = [0u8; 512];
    let bytes_read = match file.read(&mut buf) {
        Ok(bytes_read) => bytes_read,
        Err(_) => return false,
    };
    let content = String::from_utf8_lossy(&buf[..bytes_read]);
    let first_line = content.lines().next().unwrap_or("");
    if first_line.is_empty() {
        return false;
    }
    let parsed: Result<FileEntry, _> = serde_json::from_str(first_line);
    match parsed {
        Ok(FileEntry::Session(header)) => !header.id.is_empty(),
        _ => false,
    }
}

fn generate_id(existing: &HashSet<String>) -> String {
    for _ in 0..100 {
        let id = Uuid::new_v4().simple().to_string();
        let short = id.chars().take(8).collect::<String>();
        if !existing.contains(&short) {
            return short;
        }
    }
    Uuid::new_v4().simple().to_string()
}

fn build_tree_node(
    id: &str,
    entries: &HashMap<String, SessionEntry>,
    children_map: &HashMap<String, Vec<String>>,
    labels: &HashMap<String, String>,
) -> Option<SessionTreeNode> {
    let entry = entries.get(id)?.clone();
    let mut child_ids = children_map.get(id).cloned().unwrap_or_default();
    child_ids.sort_by_key(|child_id| {
        entries
            .get(child_id)
            .map(|entry| entry.timestamp().to_string())
            .unwrap_or_default()
    });
    let mut children = Vec::new();
    for child_id in child_ids {
        if let Some(child_node) = build_tree_node(&child_id, entries, children_map, labels) {
            children.push(child_node);
        }
    }
    Some(SessionTreeNode {
        entry,
        children,
        label: labels.get(id).cloned(),
    })
}

impl SessionEntry {
    pub fn id(&self) -> &str {
        match self {
            SessionEntry::Message(entry) => &entry.id,
            SessionEntry::ThinkingLevelChange(entry) => &entry.id,
            SessionEntry::ModelChange(entry) => &entry.id,
            SessionEntry::Compaction(entry) => &entry.id,
            SessionEntry::BranchSummary(entry) => &entry.id,
            SessionEntry::Custom(entry) => &entry.id,
            SessionEntry::CustomMessage(entry) => &entry.id,
            SessionEntry::Label(entry) => &entry.id,
        }
    }

    pub fn parent_id(&self) -> Option<&str> {
        match self {
            SessionEntry::Message(entry) => entry.parent_id.as_deref(),
            SessionEntry::ThinkingLevelChange(entry) => entry.parent_id.as_deref(),
            SessionEntry::ModelChange(entry) => entry.parent_id.as_deref(),
            SessionEntry::Compaction(entry) => entry.parent_id.as_deref(),
            SessionEntry::BranchSummary(entry) => entry.parent_id.as_deref(),
            SessionEntry::Custom(entry) => entry.parent_id.as_deref(),
            SessionEntry::CustomMessage(entry) => entry.parent_id.as_deref(),
            SessionEntry::Label(entry) => entry.parent_id.as_deref(),
        }
    }

    pub fn timestamp(&self) -> &str {
        match self {
            SessionEntry::Message(entry) => &entry.timestamp,
            SessionEntry::ThinkingLevelChange(entry) => &entry.timestamp,
            SessionEntry::ModelChange(entry) => &entry.timestamp,
            SessionEntry::Compaction(entry) => &entry.timestamp,
            SessionEntry::BranchSummary(entry) => &entry.timestamp,
            SessionEntry::Custom(entry) => &entry.timestamp,
            SessionEntry::CustomMessage(entry) => &entry.timestamp,
            SessionEntry::Label(entry) => &entry.timestamp,
        }
    }
}

impl FileEntry {
    pub fn session_id(&self) -> Option<&str> {
        match self {
            FileEntry::Message(entry) => Some(&entry.id),
            FileEntry::ThinkingLevelChange(entry) => Some(&entry.id),
            FileEntry::ModelChange(entry) => Some(&entry.id),
            FileEntry::Compaction(entry) => Some(&entry.id),
            FileEntry::BranchSummary(entry) => Some(&entry.id),
            FileEntry::Custom(entry) => Some(&entry.id),
            FileEntry::CustomMessage(entry) => Some(&entry.id),
            FileEntry::Label(entry) => Some(&entry.id),
            FileEntry::Session(_) => None,
        }
    }

    pub fn as_session_entry(&self) -> Option<SessionEntry> {
        match self {
            FileEntry::Message(entry) => Some(SessionEntry::Message(entry.clone())),
            FileEntry::ThinkingLevelChange(entry) => {
                Some(SessionEntry::ThinkingLevelChange(entry.clone()))
            }
            FileEntry::ModelChange(entry) => Some(SessionEntry::ModelChange(entry.clone())),
            FileEntry::Compaction(entry) => Some(SessionEntry::Compaction(entry.clone())),
            FileEntry::BranchSummary(entry) => Some(SessionEntry::BranchSummary(entry.clone())),
            FileEntry::Custom(entry) => Some(SessionEntry::Custom(entry.clone())),
            FileEntry::CustomMessage(entry) => Some(SessionEntry::CustomMessage(entry.clone())),
            FileEntry::Label(entry) => Some(SessionEntry::Label(entry.clone())),
            FileEntry::Session(_) => None,
        }
    }
}

pub struct SessionManager {
    session_id: String,
    session_file: Option<PathBuf>,
    session_dir: PathBuf,
    cwd: PathBuf,
    persist: bool,
    flushed: bool,
    file_entries: Vec<FileEntry>,
    by_id: HashMap<String, SessionEntry>,
    labels_by_id: HashMap<String, String>,
    leaf_id: Option<String>,
}

impl SessionManager {
    pub fn in_memory() -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        SessionManager::new(cwd, PathBuf::new(), None, false)
    }

    pub fn create(cwd: PathBuf) -> Self {
        let session_dir = get_default_session_dir(&cwd);
        SessionManager::new(cwd, session_dir, None, true)
    }

    pub fn create_with_dir(cwd: PathBuf, session_dir: PathBuf) -> Self {
        SessionManager::new(cwd, session_dir, None, true)
    }

    pub fn open(path: PathBuf, session_dir: Option<PathBuf>) -> Self {
        let entries = load_entries_from_file(&path);
        let cwd = entries
            .iter()
            .find_map(|entry| match entry {
                FileEntry::Session(header) => Some(PathBuf::from(header.cwd.clone())),
                _ => None,
            })
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let dir = session_dir.unwrap_or_else(|| {
            path.parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."))
        });
        SessionManager::new(cwd, dir, Some(path), true)
    }

    pub fn continue_recent(cwd: PathBuf, session_dir: Option<PathBuf>) -> Self {
        let dir = session_dir.unwrap_or_else(|| get_default_session_dir(&cwd));
        if let Some(path) = find_most_recent_session(&dir) {
            return SessionManager::new(cwd, dir, Some(path), true);
        }
        SessionManager::new(cwd, dir, None, true)
    }

    fn new(
        cwd: PathBuf,
        session_dir: PathBuf,
        session_file: Option<PathBuf>,
        persist: bool,
    ) -> Self {
        let mut manager = SessionManager {
            session_id: String::new(),
            session_file,
            session_dir,
            cwd,
            persist,
            flushed: false,
            file_entries: Vec::new(),
            by_id: HashMap::new(),
            labels_by_id: HashMap::new(),
            leaf_id: None,
        };
        if manager
            .session_file
            .as_ref()
            .map(|p| p.exists())
            .unwrap_or(false)
        {
            manager.set_session_file(manager.session_file.clone().unwrap());
        } else {
            manager.new_session(None);
        }
        manager
    }

    pub fn new_session(&mut self, parent_session: Option<String>) -> Option<PathBuf> {
        self.session_id = Uuid::new_v4().simple().to_string();
        let timestamp = Utc::now().to_rfc3339();
        let header = SessionHeader {
            id: self.session_id.clone(),
            version: Some(CURRENT_SESSION_VERSION),
            timestamp: timestamp.clone(),
            cwd: self.cwd.to_string_lossy().to_string(),
            parent_session,
        };
        let header_entry = FileEntry::Session(header.clone());
        self.file_entries = vec![header_entry.clone()];
        self.by_id.clear();
        self.labels_by_id.clear();
        self.leaf_id = None;
        self.flushed = false;

        if self.persist && self.session_file.is_none() {
            let file_timestamp = timestamp.replace([':', '.'], "-");
            let filename = format!("{file_timestamp}_{}.jsonl", self.session_id);
            let path = self.get_session_dir().join(filename);
            self.session_file = Some(path);
        }
        if self.persist {
            if let Some(path) = self.session_file.as_ref() {
                if !path.exists() {
                    if let Some(parent) = path.parent() {
                        let _ = fs::create_dir_all(parent);
                    }
                    if let Ok(line) = serde_json::to_string(&header_entry) {
                        let _ = fs::write(path, format!("{line}\n"));
                    }
                }
            }
        }
        self.session_file.clone()
    }

    fn next_id(&self) -> String {
        let existing: HashSet<String> = self.by_id.keys().cloned().collect();
        generate_id(&existing)
    }

    pub fn set_session_file(&mut self, session_file: PathBuf) {
        self.session_file = Some(session_file.clone());
        if session_file.exists() {
            self.file_entries = load_entries_from_file(&session_file);
            let header_id = self
                .file_entries
                .iter()
                .find_map(|entry| match entry {
                    FileEntry::Session(header) => Some(header.id.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| Uuid::new_v4().simple().to_string());
            self.session_id = header_id;

            migrate_session_entries(&mut self.file_entries);
            self.rewrite_file();
            self.build_index();
            self.flushed = true;
        } else {
            self.new_session(None);
        }
    }

    fn build_index(&mut self) {
        self.by_id.clear();
        self.labels_by_id.clear();
        self.leaf_id = None;
        for entry in &self.file_entries {
            if let Some(session_entry) = entry.as_session_entry() {
                self.leaf_id = Some(session_entry.id().to_string());
                if let SessionEntry::Label(label) = &session_entry {
                    if let Some(value) = &label.label {
                        self.labels_by_id
                            .insert(label.target_id.clone(), value.clone());
                    } else {
                        self.labels_by_id.remove(&label.target_id);
                    }
                }
                self.by_id
                    .insert(session_entry.id().to_string(), session_entry);
            }
        }
    }

    fn rewrite_file(&self) {
        if !self.persist {
            return;
        }
        let path = match &self.session_file {
            Some(path) => path,
            None => return,
        };
        let mut content = String::new();
        for entry in &self.file_entries {
            if let Ok(line) = serde_json::to_string(entry) {
                content.push_str(&line);
                content.push('\n');
            }
        }
        let _ = fs::write(path, content);
    }

    fn persist_entry(&mut self, entry: &FileEntry) {
        if !self.persist {
            return;
        }
        let path = match &self.session_file {
            Some(path) => path,
            None => return,
        };
        let has_assistant = self.file_entries.iter().any(|entry| match entry {
            FileEntry::Message(message) => matches!(message.message, AgentMessage::Assistant(_)),
            _ => false,
        });
        if !has_assistant {
            return;
        }

        if !self.flushed {
            let mut content = String::new();
            for entry in &self.file_entries {
                if let Ok(line) = serde_json::to_string(entry) {
                    content.push_str(&line);
                    content.push('\n');
                }
            }
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Ok(mut file) = File::create(path) {
                let _ = file.write_all(content.as_bytes());
            }
            self.flushed = true;
        } else if let Ok(line) = serde_json::to_string(entry) {
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Ok(mut file) = OpenOptions::new().append(true).create(true).open(path) {
                let _ = writeln!(file, "{}", line);
            }
        }
    }

    fn append_entry(&mut self, entry: SessionEntry) -> String {
        let id = entry.id().to_string();
        let file_entry = match &entry {
            SessionEntry::Message(message) => FileEntry::Message(message.clone()),
            SessionEntry::ThinkingLevelChange(change) => {
                FileEntry::ThinkingLevelChange(change.clone())
            }
            SessionEntry::ModelChange(change) => FileEntry::ModelChange(change.clone()),
            SessionEntry::Compaction(compaction) => FileEntry::Compaction(compaction.clone()),
            SessionEntry::BranchSummary(summary) => FileEntry::BranchSummary(summary.clone()),
            SessionEntry::Custom(custom) => FileEntry::Custom(custom.clone()),
            SessionEntry::CustomMessage(custom_message) => {
                FileEntry::CustomMessage(custom_message.clone())
            }
            SessionEntry::Label(label) => FileEntry::Label(label.clone()),
        };
        self.file_entries.push(file_entry.clone());
        self.by_id.insert(id.clone(), entry.clone());
        self.leaf_id = Some(id.clone());

        if let SessionEntry::Label(label) = entry {
            if let Some(value) = label.label {
                self.labels_by_id.insert(label.target_id, value);
            } else {
                self.labels_by_id.remove(&label.target_id);
            }
        }

        self.persist_entry(&file_entry);
        id
    }

    pub fn append_message(&mut self, message: AgentMessage) -> String {
        let entry = SessionMessageEntry {
            id: self.next_id(),
            parent_id: self.leaf_id.clone(),
            timestamp: Utc::now().to_rfc3339(),
            message,
        };
        self.append_entry(SessionEntry::Message(entry))
    }

    pub fn append_thinking_level_change(&mut self, thinking_level: &str) -> String {
        let entry = ThinkingLevelChangeEntry {
            id: self.next_id(),
            parent_id: self.leaf_id.clone(),
            timestamp: Utc::now().to_rfc3339(),
            thinking_level: thinking_level.to_string(),
        };
        self.append_entry(SessionEntry::ThinkingLevelChange(entry))
    }

    pub fn append_model_change(&mut self, provider: &str, model_id: &str) -> String {
        let entry = ModelChangeEntry {
            id: self.next_id(),
            parent_id: self.leaf_id.clone(),
            timestamp: Utc::now().to_rfc3339(),
            provider: provider.to_string(),
            model_id: model_id.to_string(),
        };
        self.append_entry(SessionEntry::ModelChange(entry))
    }

    pub fn append_compaction(
        &mut self,
        summary: &str,
        first_kept_entry_id: &str,
        tokens_before: i64,
    ) -> String {
        let entry = CompactionEntry {
            id: self.next_id(),
            parent_id: self.leaf_id.clone(),
            timestamp: Utc::now().to_rfc3339(),
            summary: summary.to_string(),
            first_kept_entry_id: first_kept_entry_id.to_string(),
            first_kept_entry_index: None,
            tokens_before,
            details: None,
            from_hook: None,
        };
        self.append_entry(SessionEntry::Compaction(entry))
    }

    pub fn append_custom_entry(&mut self, custom_type: &str, data: Value) -> String {
        let entry = CustomEntry {
            id: self.next_id(),
            parent_id: self.leaf_id.clone(),
            timestamp: Utc::now().to_rfc3339(),
            custom_type: custom_type.to_string(),
            data: Some(data),
        };
        self.append_entry(SessionEntry::Custom(entry))
    }

    pub fn append_label_change(
        &mut self,
        target_id: &str,
        label: Option<&str>,
    ) -> Result<String, String> {
        if !self.by_id.contains_key(target_id) {
            return Err(format!("Entry {} not found", target_id));
        }
        let entry = LabelEntry {
            id: self.next_id(),
            parent_id: self.leaf_id.clone(),
            timestamp: Utc::now().to_rfc3339(),
            target_id: target_id.to_string(),
            label: label.map(|value| value.to_string()),
        };
        Ok(self.append_entry(SessionEntry::Label(entry)))
    }

    pub fn get_entries(&self) -> Vec<SessionEntry> {
        self.file_entries
            .iter()
            .filter_map(|entry| entry.as_session_entry())
            .collect()
    }

    pub fn get_header(&self) -> Option<SessionHeader> {
        self.file_entries.iter().find_map(|entry| match entry {
            FileEntry::Session(header) => Some(header.clone()),
            _ => None,
        })
    }

    pub fn get_session_id(&self) -> String {
        self.session_id.clone()
    }

    pub fn get_session_file(&self) -> Option<PathBuf> {
        self.session_file.clone()
    }

    pub fn get_leaf_id(&self) -> Option<String> {
        self.leaf_id.clone()
    }

    pub fn get_leaf_entry(&self) -> Option<SessionEntry> {
        self.leaf_id
            .as_ref()
            .and_then(|id| self.by_id.get(id).cloned())
    }

    pub fn get_entry(&self, id: &str) -> Option<SessionEntry> {
        self.by_id.get(id).cloned()
    }

    pub fn get_label(&self, id: &str) -> Option<String> {
        self.labels_by_id.get(id).cloned()
    }

    pub fn get_branch(&self, from_id: Option<&str>) -> Vec<SessionEntry> {
        let start = from_id
            .map(|id| id.to_string())
            .or_else(|| self.leaf_id.clone());
        let mut path = Vec::new();
        let mut current = start.and_then(|id| self.by_id.get(&id).cloned());
        while let Some(entry) = current {
            path.push(entry.clone());
            current = entry.parent_id().and_then(|id| self.by_id.get(id).cloned());
        }
        path.reverse();
        path
    }

    pub fn get_tree(&self) -> Vec<SessionTreeNode> {
        let entries = self.get_entries();
        let mut entry_map: HashMap<String, SessionEntry> = HashMap::new();
        let mut children_map: HashMap<String, Vec<String>> = HashMap::new();
        let mut roots: Vec<String> = Vec::new();

        for entry in &entries {
            entry_map.insert(entry.id().to_string(), entry.clone());
        }

        for entry in &entries {
            let id = entry.id().to_string();
            let parent_id = entry.parent_id().map(|s| s.to_string());
            if parent_id.as_deref() == Some(entry.id()) || parent_id.is_none() {
                roots.push(id);
            } else if let Some(parent_id) = parent_id {
                if entry_map.contains_key(&parent_id) {
                    children_map.entry(parent_id).or_default().push(id);
                } else {
                    roots.push(id);
                }
            }
        }

        let mut nodes = Vec::new();
        for root_id in roots {
            if let Some(node) =
                build_tree_node(&root_id, &entry_map, &children_map, &self.labels_by_id)
            {
                nodes.push(node);
            }
        }
        nodes
    }

    pub fn build_session_context(&self) -> SessionContext {
        if self.leaf_id.is_none() && !self.by_id.is_empty() {
            return SessionContext {
                messages: Vec::new(),
                thinking_level: "off".to_string(),
                model: None,
            };
        }
        build_session_context(&self.get_entries(), self.leaf_id.as_deref())
    }

    pub fn branch(&mut self, branch_from_id: &str) -> Result<(), String> {
        if !self.by_id.contains_key(branch_from_id) {
            return Err(format!("Entry {} not found", branch_from_id));
        }
        self.leaf_id = Some(branch_from_id.to_string());
        Ok(())
    }

    pub fn reset_leaf(&mut self) {
        self.leaf_id = None;
    }

    pub fn get_children(&self, parent_id: &str) -> Vec<SessionEntry> {
        self.by_id
            .values()
            .filter(|entry| entry.parent_id() == Some(parent_id))
            .cloned()
            .collect()
    }

    pub fn branch_with_summary(
        &mut self,
        branch_from_id: Option<&str>,
        summary: &str,
        details: Option<Value>,
        from_hook: Option<bool>,
    ) -> Result<String, String> {
        if let Some(branch_from_id) = branch_from_id {
            if !self.by_id.contains_key(branch_from_id) {
                return Err(format!("Entry {} not found", branch_from_id));
            }
            self.leaf_id = Some(branch_from_id.to_string());
        } else {
            self.leaf_id = None;
        }
        let entry = BranchSummaryEntry {
            id: self.next_id(),
            parent_id: branch_from_id.map(|value| value.to_string()),
            timestamp: Utc::now().to_rfc3339(),
            from_id: branch_from_id.unwrap_or("root").to_string(),
            summary: summary.to_string(),
            details,
            from_hook,
        };
        Ok(self.append_entry(SessionEntry::BranchSummary(entry)))
    }

    pub fn create_branched_session(&mut self, leaf_id: &str) -> Result<Option<PathBuf>, String> {
        if !self.by_id.contains_key(leaf_id) {
            return Err(format!("Entry {} not found", leaf_id));
        }

        let path = self.get_branch(Some(leaf_id));
        if path.is_empty() {
            return Err(format!("Entry {} not found", leaf_id));
        }

        let path_without_labels: Vec<SessionEntry> = path
            .iter()
            .filter(|entry| !matches!(entry, SessionEntry::Label(_)))
            .cloned()
            .collect();

        let new_session_id = Uuid::new_v4().simple().to_string();
        let timestamp = Utc::now().to_rfc3339();
        let header = SessionHeader {
            id: new_session_id.clone(),
            version: Some(CURRENT_SESSION_VERSION),
            timestamp: timestamp.clone(),
            cwd: self.cwd.to_string_lossy().to_string(),
            parent_session: if self.persist {
                self.session_file
                    .as_ref()
                    .map(|path| path.to_string_lossy().to_string())
            } else {
                None
            },
        };

        let path_entry_ids: HashSet<String> = path_without_labels
            .iter()
            .map(|e| e.id().to_string())
            .collect();
        let mut labels_to_write: Vec<(String, String)> = Vec::new();
        for (target_id, label) in &self.labels_by_id {
            if path_entry_ids.contains(target_id) {
                labels_to_write.push((target_id.clone(), label.clone()));
            }
        }

        if self.persist {
            let mut existing_ids = path_entry_ids.clone();
            let file_timestamp = timestamp.replace([':', '.'], "-");
            let filename = format!("{file_timestamp}_{new_session_id}.jsonl");
            let new_session_file = self.get_session_dir().join(filename);

            if let Ok(mut file) = File::create(&new_session_file) {
                let _ = writeln!(
                    file,
                    "{}",
                    serde_json::to_string(&FileEntry::Session(header.clone())).unwrap()
                );
                for entry in &path_without_labels {
                    let file_entry = entry.to_file_entry();
                    let _ = writeln!(file, "{}", serde_json::to_string(&file_entry).unwrap());
                }
            }

            let mut label_entries = Vec::new();
            let mut parent_id = path_without_labels.last().map(|e| e.id().to_string());
            for (target_id, label) in labels_to_write {
                let id = generate_id(&existing_ids);
                existing_ids.insert(id.clone());
                let entry = LabelEntry {
                    id: id.clone(),
                    parent_id: parent_id.clone(),
                    timestamp: Utc::now().to_rfc3339(),
                    target_id,
                    label: Some(label),
                };
                label_entries.push(entry);
                parent_id = Some(id);
            }

            if let Ok(mut file) = OpenOptions::new().append(true).open(&new_session_file) {
                for entry in &label_entries {
                    let _ = writeln!(
                        file,
                        "{}",
                        serde_json::to_string(&FileEntry::Label(entry.clone())).unwrap()
                    );
                }
            }

            self.file_entries = vec![FileEntry::Session(header)];
            for entry in &path_without_labels {
                self.file_entries.push(entry.to_file_entry());
            }
            for entry in label_entries {
                self.file_entries.push(FileEntry::Label(entry));
            }
            self.session_id = new_session_id;
            self.session_file = Some(new_session_file.clone());
            self.build_index();
            return Ok(Some(new_session_file));
        }

        let mut label_entries: Vec<SessionEntry> = Vec::new();
        let mut parent_id = path_without_labels.last().map(|e| e.id().to_string());
        let mut existing_ids = path_entry_ids.clone();
        for (target_id, label) in labels_to_write {
            let id = generate_id(&existing_ids);
            existing_ids.insert(id.clone());
            let entry = LabelEntry {
                id: id.clone(),
                parent_id: parent_id.clone(),
                timestamp: Utc::now().to_rfc3339(),
                target_id,
                label: Some(label),
            };
            label_entries.push(SessionEntry::Label(entry));
            parent_id = Some(id);
        }

        self.file_entries = vec![FileEntry::Session(header)];
        for entry in &path_without_labels {
            self.file_entries.push(entry.to_file_entry());
        }
        for entry in label_entries {
            self.file_entries.push(entry.to_file_entry());
        }
        self.session_id = new_session_id;
        self.build_index();
        Ok(None)
    }

    pub fn get_session_dir(&self) -> PathBuf {
        if self.session_dir.as_os_str().is_empty() {
            get_default_session_dir(&self.cwd)
        } else {
            self.session_dir.clone()
        }
    }
}

impl SessionEntry {
    fn to_file_entry(&self) -> FileEntry {
        match self {
            SessionEntry::Message(entry) => FileEntry::Message(entry.clone()),
            SessionEntry::ThinkingLevelChange(entry) => {
                FileEntry::ThinkingLevelChange(entry.clone())
            }
            SessionEntry::ModelChange(entry) => FileEntry::ModelChange(entry.clone()),
            SessionEntry::Compaction(entry) => FileEntry::Compaction(entry.clone()),
            SessionEntry::BranchSummary(entry) => FileEntry::BranchSummary(entry.clone()),
            SessionEntry::Custom(entry) => FileEntry::Custom(entry.clone()),
            SessionEntry::CustomMessage(entry) => FileEntry::CustomMessage(entry.clone()),
            SessionEntry::Label(entry) => FileEntry::Label(entry.clone()),
        }
    }
}

fn get_default_session_dir(cwd: &Path) -> PathBuf {
    let safe_path = format!(
        "--{}--",
        cwd.to_string_lossy()
            .trim_start_matches('/')
            .replace(['/', '\\', ':'], "-")
    );
    let agent_dir = resolve_agent_dir();
    let dir = agent_dir.join("sessions").join(safe_path);
    let _ = fs::create_dir_all(&dir);
    dir
}

fn resolve_agent_dir() -> PathBuf {
    if let Ok(dir) = env::var("PI_CODING_AGENT_DIR") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }
    let home = env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".pi").join("agent")
}
