use crate::core::messages::{
    create_branch_summary_message, create_hook_message, AgentMessage, ContentBlock, Usage,
    UserContent,
};
use crate::core::session_manager::SessionEntry;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq)]
pub struct FileOperations {
    pub read: HashSet<String>,
    pub written: HashSet<String>,
    pub edited: HashSet<String>,
}

impl Default for FileOperations {
    fn default() -> Self {
        Self::new()
    }
}

impl FileOperations {
    pub fn new() -> Self {
        Self {
            read: HashSet::new(),
            written: HashSet::new(),
            edited: HashSet::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionDetails {
    pub read_files: Vec<String>,
    pub modified_files: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CompactionSettings {
    pub enabled: bool,
    pub reserve_tokens: i64,
    pub keep_recent_tokens: i64,
}

pub const DEFAULT_COMPACTION_SETTINGS: CompactionSettings = CompactionSettings {
    enabled: true,
    reserve_tokens: 16_384,
    keep_recent_tokens: 20_000,
};

pub fn calculate_context_tokens(usage: &Usage) -> i64 {
    usage
        .total_tokens
        .unwrap_or(usage.input + usage.output + usage.cache_read + usage.cache_write)
}

fn get_assistant_usage(message: &AgentMessage) -> Option<Usage> {
    match message {
        AgentMessage::Assistant(assistant) => {
            if assistant.stop_reason != "aborted" && assistant.stop_reason != "error" {
                return Some(assistant.usage.clone());
            }
            None
        }
        _ => None,
    }
}

pub fn get_last_assistant_usage(entries: &[SessionEntry]) -> Option<Usage> {
    for entry in entries.iter().rev() {
        if let SessionEntry::Message(message) = entry {
            if let Some(usage) = get_assistant_usage(&message.message) {
                return Some(usage);
            }
        }
    }
    None
}

pub fn should_compact(
    context_tokens: i64,
    context_window: i64,
    settings: CompactionSettings,
) -> bool {
    if !settings.enabled {
        return false;
    }
    context_tokens > context_window - settings.reserve_tokens
}

pub fn estimate_tokens(message: &AgentMessage) -> i64 {
    let mut chars: usize = 0;

    match message {
        AgentMessage::User(user) => match &user.content {
            UserContent::Text(text) => chars += text.len(),
            UserContent::Blocks(blocks) => {
                for block in blocks {
                    if let ContentBlock::Text { text, .. } = block {
                        chars += text.len();
                    }
                }
            }
        },
        AgentMessage::Assistant(assistant) => {
            for block in &assistant.content {
                match block {
                    ContentBlock::Text { text, .. } => chars += text.len(),
                    ContentBlock::Thinking { thinking, .. } => chars += thinking.len(),
                    ContentBlock::ToolCall {
                        name, arguments, ..
                    } => {
                        chars += name.len();
                        chars += serde_json::to_string(arguments).unwrap_or_default().len();
                    }
                    ContentBlock::Image { .. } => {}
                }
            }
        }
        AgentMessage::HookMessage(hook) => match &hook.content {
            UserContent::Text(text) => chars += text.len(),
            UserContent::Blocks(blocks) => {
                for block in blocks {
                    match block {
                        ContentBlock::Text { text, .. } => chars += text.len(),
                        ContentBlock::Image { .. } => chars += 4800,
                        _ => {}
                    }
                }
            }
        },
        AgentMessage::ToolResult(result) => {
            for block in &result.content {
                match block {
                    ContentBlock::Text { text, .. } => chars += text.len(),
                    ContentBlock::Image { .. } => chars += 4800,
                    _ => {}
                }
            }
        }
        AgentMessage::BashExecution(bash) => {
            chars += bash.command.len();
            chars += bash.output.len();
        }
        AgentMessage::BranchSummary(summary) => chars += summary.summary.len(),
        AgentMessage::CompactionSummary(summary) => chars += summary.summary.len(),
    }

    chars.div_ceil(4) as i64
}

fn find_valid_cut_points(
    entries: &[SessionEntry],
    start_index: usize,
    end_index: usize,
) -> Vec<usize> {
    let mut cut_points = Vec::new();
    for (i, entry) in entries.iter().enumerate().take(end_index).skip(start_index) {
        match entry {
            SessionEntry::Message(message) => {
                let is_valid = matches!(
                    message.message,
                    AgentMessage::BashExecution(_)
                        | AgentMessage::HookMessage(_)
                        | AgentMessage::BranchSummary(_)
                        | AgentMessage::CompactionSummary(_)
                        | AgentMessage::User(_)
                        | AgentMessage::Assistant(_)
                );
                if is_valid {
                    cut_points.push(i);
                }
            }
            SessionEntry::BranchSummary(_) | SessionEntry::CustomMessage(_) => {
                cut_points.push(i);
            }
            _ => {}
        }
    }
    cut_points
}

pub fn find_turn_start_index(
    entries: &[SessionEntry],
    entry_index: usize,
    start_index: usize,
) -> Option<usize> {
    for i in (start_index..=entry_index).rev() {
        let entry = &entries[i];
        match entry {
            SessionEntry::BranchSummary(_) | SessionEntry::CustomMessage(_) => return Some(i),
            SessionEntry::Message(message) => match message.message {
                AgentMessage::User(_) | AgentMessage::BashExecution(_) => return Some(i),
                _ => {}
            },
            _ => {}
        }
    }
    None
}

#[derive(Clone, Debug, PartialEq)]
pub struct CutPointResult {
    pub first_kept_entry_index: usize,
    pub turn_start_index: Option<usize>,
    pub is_split_turn: bool,
}

pub fn find_cut_point(
    entries: &[SessionEntry],
    start_index: usize,
    end_index: usize,
    keep_recent_tokens: i64,
) -> CutPointResult {
    let cut_points = find_valid_cut_points(entries, start_index, end_index);

    if cut_points.is_empty() {
        return CutPointResult {
            first_kept_entry_index: start_index,
            turn_start_index: None,
            is_split_turn: false,
        };
    }

    let mut accumulated_tokens = 0_i64;
    let mut cut_index = cut_points[0];

    for i in (start_index..end_index).rev() {
        let entry = &entries[i];
        let message = match entry {
            SessionEntry::Message(message) => &message.message,
            _ => continue,
        };

        let message_tokens = estimate_tokens(message);
        accumulated_tokens += message_tokens;

        if accumulated_tokens >= keep_recent_tokens {
            for cut_point in &cut_points {
                if *cut_point >= i {
                    cut_index = *cut_point;
                    break;
                }
            }
            break;
        }
    }

    while cut_index > start_index {
        let prev_entry = &entries[cut_index - 1];
        if matches!(prev_entry, SessionEntry::Compaction(_)) {
            break;
        }
        if matches!(prev_entry, SessionEntry::Message(_)) {
            break;
        }
        cut_index -= 1;
    }

    let cut_entry = &entries[cut_index];
    let is_user_message = matches!(
        cut_entry,
        SessionEntry::Message(message) if matches!(message.message, AgentMessage::User(_))
    );
    let turn_start_index = if is_user_message {
        None
    } else {
        find_turn_start_index(entries, cut_index, start_index)
    };
    let is_split_turn = !is_user_message && turn_start_index.is_some();

    CutPointResult {
        first_kept_entry_index: cut_index,
        turn_start_index,
        is_split_turn,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompactionPreparation {
    pub first_kept_entry_id: String,
    pub messages_to_summarize: Vec<AgentMessage>,
    pub turn_prefix_messages: Vec<AgentMessage>,
    pub is_split_turn: bool,
    pub tokens_before: i64,
    pub previous_summary: Option<String>,
    pub file_ops: FileOperations,
    pub settings: CompactionSettings,
}

fn get_message_from_entry(entry: &SessionEntry) -> Option<AgentMessage> {
    match entry {
        SessionEntry::Message(message) => Some(message.message.clone()),
        SessionEntry::CustomMessage(custom_message) => Some(create_hook_message(
            &custom_message.custom_type,
            custom_message.content.clone(),
            custom_message.display,
            custom_message.details.clone(),
            &custom_message.timestamp,
        )),
        SessionEntry::BranchSummary(branch_summary) => Some(create_branch_summary_message(
            &branch_summary.summary,
            &branch_summary.from_id,
            &branch_summary.timestamp,
        )),
        _ => None,
    }
}

fn extract_file_ops_from_message(message: &AgentMessage, file_ops: &mut FileOperations) {
    let assistant = match message {
        AgentMessage::Assistant(assistant) => assistant,
        _ => return,
    };
    for block in &assistant.content {
        let (name, arguments) = match block {
            ContentBlock::ToolCall {
                name, arguments, ..
            } => (name.as_str(), arguments),
            _ => continue,
        };
        let path = arguments
            .as_object()
            .and_then(|args| args.get("path"))
            .and_then(|value| value.as_str());
        let path = match path {
            Some(path) => path,
            None => continue,
        };
        match name {
            "read" => {
                file_ops.read.insert(path.to_string());
            }
            "write" => {
                file_ops.written.insert(path.to_string());
            }
            "edit" => {
                file_ops.edited.insert(path.to_string());
            }
            _ => {}
        }
    }
}

fn extract_file_operations(
    messages: &[AgentMessage],
    entries: &[SessionEntry],
    prev_compaction_index: isize,
) -> FileOperations {
    let mut file_ops = FileOperations::new();

    if prev_compaction_index >= 0 {
        let entry = &entries[prev_compaction_index as usize];
        if let SessionEntry::Compaction(compaction) = entry {
            let from_hook = compaction.from_hook.unwrap_or(false);
            if !from_hook {
                if let Some(details) = &compaction.details {
                    if let Ok(details) =
                        serde_json::from_value::<CompactionDetails>(details.clone())
                    {
                        for file in details.read_files {
                            file_ops.read.insert(file);
                        }
                        for file in details.modified_files {
                            file_ops.edited.insert(file);
                        }
                    }
                }
            }
        }
    }

    for message in messages {
        extract_file_ops_from_message(message, &mut file_ops);
    }

    file_ops
}

pub fn prepare_compaction(
    path_entries: &[SessionEntry],
    settings: CompactionSettings,
) -> Option<CompactionPreparation> {
    if matches!(path_entries.last(), Some(SessionEntry::Compaction(_))) {
        return None;
    }

    let mut prev_compaction_index = -1_isize;
    for (index, entry) in path_entries.iter().enumerate().rev() {
        if matches!(entry, SessionEntry::Compaction(_)) {
            prev_compaction_index = index as isize;
            break;
        }
    }

    let boundary_start = (prev_compaction_index + 1) as usize;
    let boundary_end = path_entries.len();

    let last_usage = get_last_assistant_usage(path_entries);
    let tokens_before = last_usage
        .map(|usage| calculate_context_tokens(&usage))
        .unwrap_or(0);

    let cut_point = find_cut_point(
        path_entries,
        boundary_start,
        boundary_end,
        settings.keep_recent_tokens,
    );

    let first_kept_entry = path_entries.get(cut_point.first_kept_entry_index)?;
    let first_kept_entry_id = first_kept_entry.id();
    if first_kept_entry_id.is_empty() {
        return None;
    }

    let history_end = if cut_point.is_split_turn {
        cut_point
            .turn_start_index
            .unwrap_or(cut_point.first_kept_entry_index)
    } else {
        cut_point.first_kept_entry_index
    };

    let mut messages_to_summarize = Vec::new();
    for entry in &path_entries[boundary_start..history_end] {
        if let Some(message) = get_message_from_entry(entry) {
            messages_to_summarize.push(message);
        }
    }

    let mut turn_prefix_messages = Vec::new();
    if cut_point.is_split_turn {
        let start = cut_point
            .turn_start_index
            .unwrap_or(cut_point.first_kept_entry_index);
        for entry in &path_entries[start..cut_point.first_kept_entry_index] {
            if let Some(message) = get_message_from_entry(entry) {
                turn_prefix_messages.push(message);
            }
        }
    }

    let mut previous_summary = None;
    if prev_compaction_index >= 0 {
        if let SessionEntry::Compaction(compaction) = &path_entries[prev_compaction_index as usize]
        {
            previous_summary = Some(compaction.summary.clone());
        }
    }

    let mut file_ops =
        extract_file_operations(&messages_to_summarize, path_entries, prev_compaction_index);
    if cut_point.is_split_turn {
        for message in &turn_prefix_messages {
            extract_file_ops_from_message(message, &mut file_ops);
        }
    }

    Some(CompactionPreparation {
        first_kept_entry_id: first_kept_entry_id.to_string(),
        messages_to_summarize,
        turn_prefix_messages,
        is_split_turn: cut_point.is_split_turn,
        tokens_before,
        previous_summary,
        file_ops,
        settings,
    })
}

pub fn compute_file_lists(file_ops: &FileOperations) -> (Vec<String>, Vec<String>) {
    let modified: HashSet<String> = file_ops
        .edited
        .iter()
        .chain(file_ops.written.iter())
        .cloned()
        .collect();
    let mut read_files: Vec<String> = file_ops
        .read
        .iter()
        .filter(|path| !modified.contains(*path))
        .cloned()
        .collect();
    let mut modified_files: Vec<String> = modified.into_iter().collect();
    read_files.sort();
    modified_files.sort();
    (read_files, modified_files)
}

pub fn format_file_operations(read_files: &[String], modified_files: &[String]) -> String {
    let mut sections: Vec<String> = Vec::new();
    if !read_files.is_empty() {
        sections.push(format!(
            "<read-files>\n{}\n</read-files>",
            read_files.join("\n")
        ));
    }
    if !modified_files.is_empty() {
        sections.push(format!(
            "<modified-files>\n{}\n</modified-files>",
            modified_files.join("\n")
        ));
    }
    if sections.is_empty() {
        return String::new();
    }
    format!("\n\n{}", sections.join("\n\n"))
}
