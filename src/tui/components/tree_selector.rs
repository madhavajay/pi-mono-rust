//! Session tree selector component with ASCII art visualization.
//!
//! Provides a navigable tree view of session entries with:
//! - ASCII art connectors (├─, └─, │)
//! - Active path highlighting
//! - Filter modes (default, no-tools, user-only, labeled-only, all)
//! - Search/filtering by text

use crate::core::messages::{AgentMessage, UserContent};
use crate::core::session_manager::{SessionEntry, SessionTreeNode};
use crate::tui::keys::matches_key;
use crate::tui::utils::truncate_to_width;
use std::collections::HashSet;

/// Gutter info: position (indent level) and whether to show │
#[derive(Clone, Debug)]
struct GutterInfo {
    /// Indentation level where the connector was shown
    position: usize,
    /// true = show │, false = show spaces
    show: bool,
}

/// Flattened tree node for navigation
#[derive(Clone, Debug)]
struct FlatNode {
    /// Reference to the original node (by entry ID)
    entry_id: String,
    /// The entry type for filtering
    entry_type: String,
    /// Optional label
    label: Option<String>,
    /// Parent ID for active path calculation
    parent_id: Option<String>,
    /// Indentation level (each level = 3 chars)
    indent: usize,
    /// Whether to show connector (├─ or └─)
    show_connector: bool,
    /// If show_connector, true = last sibling (└─), false = not last (├─)
    is_last: bool,
    /// Gutter info for each ancestor branch point
    gutters: Vec<GutterInfo>,
    /// True if this node is a root under a virtual branching root (multiple roots)
    is_virtual_root_child: bool,
    /// Display text for the entry
    display_text: String,
}

/// Filter mode for tree display
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterMode {
    Default,
    NoTools,
    UserOnly,
    LabeledOnly,
    All,
}

impl FilterMode {
    fn next(self) -> Self {
        match self {
            Self::Default => Self::NoTools,
            Self::NoTools => Self::UserOnly,
            Self::UserOnly => Self::LabeledOnly,
            Self::LabeledOnly => Self::All,
            Self::All => Self::Default,
        }
    }

    fn prev(self) -> Self {
        match self {
            Self::Default => Self::All,
            Self::NoTools => Self::Default,
            Self::UserOnly => Self::NoTools,
            Self::LabeledOnly => Self::UserOnly,
            Self::All => Self::LabeledOnly,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::Default => "",
            Self::NoTools => " [no-tools]",
            Self::UserOnly => " [user]",
            Self::LabeledOnly => " [labeled]",
            Self::All => " [all]",
        }
    }
}

// Type aliases for callback signatures
type SelectCallback = Box<dyn FnMut(&str)>;
type CancelCallback = Box<dyn FnMut()>;
type LabelEditCallback = Box<dyn FnMut(&str, Option<&str>)>;

/// Tree list component with selection and ASCII art visualization
pub struct TreeList {
    /// All flattened nodes
    flat_nodes: Vec<FlatNode>,
    /// Filtered nodes (after applying filter mode and search)
    filtered_nodes: Vec<FlatNode>,
    /// Currently selected index in filtered_nodes
    selected_index: usize,
    /// Current leaf entry ID (the active position)
    current_leaf_id: Option<String>,
    /// Maximum visible lines
    max_visible_lines: usize,
    /// Current filter mode
    filter_mode: FilterMode,
    /// Current search query
    search_query: String,
    /// Whether there are multiple roots
    multiple_roots: bool,
    /// Entry IDs on the active path (from root to current leaf)
    active_path_ids: HashSet<String>,
    /// Callback when an entry is selected
    pub on_select: Option<SelectCallback>,
    /// Callback when selection is cancelled
    pub on_cancel: Option<CancelCallback>,
    /// Callback when label edit is requested
    pub on_label_edit: Option<LabelEditCallback>,
}

impl TreeList {
    /// Create a new tree list from session tree nodes
    pub fn new(
        tree: Vec<SessionTreeNode>,
        current_leaf_id: Option<String>,
        max_visible_lines: usize,
    ) -> Self {
        let multiple_roots = tree.len() > 1;
        let flat_nodes = Self::flatten_tree(&tree, current_leaf_id.as_deref(), multiple_roots);

        let mut list = Self {
            flat_nodes,
            filtered_nodes: Vec::new(),
            selected_index: 0,
            current_leaf_id,
            max_visible_lines,
            filter_mode: FilterMode::Default,
            search_query: String::new(),
            multiple_roots,
            active_path_ids: HashSet::new(),
            on_select: None,
            on_cancel: None,
            on_label_edit: None,
        };

        list.build_active_path();
        list.apply_filter();

        // Start with current leaf selected
        if let Some(ref leaf_id) = list.current_leaf_id {
            if let Some(idx) = list
                .filtered_nodes
                .iter()
                .position(|n| &n.entry_id == leaf_id)
            {
                list.selected_index = idx;
            } else {
                list.selected_index = list.filtered_nodes.len().saturating_sub(1);
            }
        }

        list
    }

    /// Build the set of entry IDs on the path from root to current leaf
    fn build_active_path(&mut self) {
        self.active_path_ids.clear();

        let leaf_id = match &self.current_leaf_id {
            Some(id) => id,
            None => return,
        };

        // Build a map of id -> parent_id for path lookup
        let parent_map: std::collections::HashMap<&str, Option<&str>> = self
            .flat_nodes
            .iter()
            .map(|n| (n.entry_id.as_str(), n.parent_id.as_deref()))
            .collect();

        // Walk from leaf to root
        let mut current_id: Option<&str> = Some(leaf_id);
        while let Some(id) = current_id {
            self.active_path_ids.insert(id.to_string());
            current_id = parent_map.get(id).and_then(|p| *p);
        }
    }

    /// Flatten tree into a list suitable for display
    fn flatten_tree(
        roots: &[SessionTreeNode],
        current_leaf_id: Option<&str>,
        multiple_roots: bool,
    ) -> Vec<FlatNode> {
        let mut result = Vec::new();

        // Stack items: (node, indent, just_branched, show_connector, is_last, gutters, is_virtual_root_child)
        type StackItem<'a> = (
            &'a SessionTreeNode,
            usize,
            bool,
            bool,
            bool,
            Vec<GutterInfo>,
            bool,
        );
        let mut stack: Vec<StackItem> = Vec::new();

        // Determine which subtrees contain the active leaf (to sort current branch first)
        let contains_active = Self::build_contains_active_map(roots, current_leaf_id);

        // Add roots in reverse order, prioritizing the one containing the active leaf
        let mut ordered_roots: Vec<&SessionTreeNode> = roots.iter().collect();
        ordered_roots.sort_by_key(|r| !contains_active.contains(r.entry.id()));

        for (i, root) in ordered_roots.iter().enumerate().rev() {
            let is_last = i == ordered_roots.len() - 1;
            stack.push((
                root,
                if multiple_roots { 1 } else { 0 },
                multiple_roots,
                multiple_roots,
                is_last,
                Vec::new(),
                multiple_roots,
            ));
        }

        while let Some((
            node,
            indent,
            just_branched,
            show_connector,
            is_last,
            gutters,
            is_virtual_root_child,
        )) = stack.pop()
        {
            // Get display text for entry
            let display_text = Self::get_entry_display_text(node);
            let entry_type = Self::get_entry_type(&node.entry);

            result.push(FlatNode {
                entry_id: node.entry.id().to_string(),
                entry_type,
                label: node.label.clone(),
                parent_id: node.entry.parent_id().map(String::from),
                indent,
                show_connector,
                is_last,
                gutters: gutters.clone(),
                is_virtual_root_child,
                display_text,
            });

            let children = &node.children;
            let multiple_children = children.len() > 1;

            // Order children so the branch containing the active leaf comes first
            let mut ordered_children: Vec<&SessionTreeNode> = children.iter().collect();
            ordered_children.sort_by_key(|c| !contains_active.contains(c.entry.id()));

            // Calculate child indent
            let child_indent = if multiple_children || (just_branched && indent > 0) {
                indent + 1
            } else {
                indent
            };

            // Build gutters for children
            let connector_displayed = show_connector && !is_virtual_root_child;
            let current_display_indent = if multiple_roots {
                indent.saturating_sub(1)
            } else {
                indent
            };
            let connector_position = current_display_indent.saturating_sub(1);
            let child_gutters: Vec<GutterInfo> = if connector_displayed {
                let mut g = gutters.clone();
                g.push(GutterInfo {
                    position: connector_position,
                    show: !is_last,
                });
                g
            } else {
                gutters.clone()
            };

            // Add children in reverse order
            for (i, child) in ordered_children.iter().enumerate().rev() {
                let child_is_last = i == ordered_children.len() - 1;
                stack.push((
                    child,
                    child_indent,
                    multiple_children,
                    multiple_children,
                    child_is_last,
                    child_gutters.clone(),
                    false,
                ));
            }
        }

        result
    }

    /// Build a set of entry IDs for nodes that contain the active leaf in their subtree
    fn build_contains_active_map(
        roots: &[SessionTreeNode],
        current_leaf_id: Option<&str>,
    ) -> HashSet<String> {
        let mut contains_active = HashSet::new();
        let leaf_id = match current_leaf_id {
            Some(id) => id,
            None => return contains_active,
        };

        // Build list in pre-order, then process in reverse for post-order effect
        let mut all_nodes: Vec<&SessionTreeNode> = Vec::new();
        let mut pre_order_stack: Vec<&SessionTreeNode> = roots.iter().collect();

        while let Some(node) = pre_order_stack.pop() {
            all_nodes.push(node);
            for child in node.children.iter().rev() {
                pre_order_stack.push(child);
            }
        }

        // Process in reverse (post-order): children before parents
        for node in all_nodes.iter().rev() {
            let mut has = node.entry.id() == leaf_id;
            for child in &node.children {
                if contains_active.contains(child.entry.id()) {
                    has = true;
                }
            }
            if has {
                contains_active.insert(node.entry.id().to_string());
            }
        }

        contains_active
    }

    /// Get entry type for filtering
    fn get_entry_type(entry: &SessionEntry) -> String {
        match entry {
            SessionEntry::Message(e) => {
                let role = match &e.message {
                    AgentMessage::User(_) => "user",
                    AgentMessage::Assistant(_) => "assistant",
                    AgentMessage::ToolResult(_) => "toolResult",
                    AgentMessage::BashExecution(_) => "bashExecution",
                    AgentMessage::HookMessage(_) => "custom",
                    AgentMessage::BranchSummary(_) => "branchSummary",
                    AgentMessage::CompactionSummary(_) => "compactionSummary",
                };
                format!("message:{}", role)
            }
            SessionEntry::Compaction(_) => "compaction".to_string(),
            SessionEntry::BranchSummary(_) => "branch_summary".to_string(),
            SessionEntry::ModelChange(_) => "model_change".to_string(),
            SessionEntry::ThinkingLevelChange(_) => "thinking_level_change".to_string(),
            SessionEntry::Label(_) => "label".to_string(),
            SessionEntry::Custom(_) => "custom".to_string(),
            SessionEntry::CustomMessage(e) => format!("custom_message:{}", e.custom_type),
        }
    }

    /// Get display text for an entry
    fn get_entry_display_text(node: &SessionTreeNode) -> String {
        let normalize = |s: &str| s.replace(['\n', '\t'], " ").trim().to_string();

        match &node.entry {
            SessionEntry::Message(e) => match &e.message {
                AgentMessage::User(user_msg) => {
                    let content_str = Self::extract_user_content(&user_msg.content);
                    format!("user: {}", normalize(&content_str))
                }
                AgentMessage::Assistant(assistant_msg) => {
                    let content_str = Self::extract_content_blocks(&assistant_msg.content);
                    if content_str.is_empty() {
                        "assistant: (no content)".to_string()
                    } else {
                        format!("assistant: {}", normalize(&content_str))
                    }
                }
                AgentMessage::ToolResult(_) => "[tool result]".to_string(),
                AgentMessage::BashExecution(bash_msg) => {
                    let cmd = bash_msg.command.chars().take(50).collect::<String>();
                    format!("[bash: {}]", normalize(&cmd))
                }
                AgentMessage::HookMessage(hook_msg) => {
                    format!("[hook: {}]", hook_msg.custom_type)
                }
                AgentMessage::BranchSummary(summary_msg) => {
                    format!("[branch summary]: {}", normalize(&summary_msg.summary))
                }
                AgentMessage::CompactionSummary(summary_msg) => {
                    format!("[compaction summary]: {}", normalize(&summary_msg.summary))
                }
            },
            SessionEntry::Compaction(e) => {
                format!("[compaction: {}k tokens]", e.tokens_before / 1000)
            }
            SessionEntry::BranchSummary(e) => {
                format!("[branch summary]: {}", normalize(&e.summary))
            }
            SessionEntry::ModelChange(e) => {
                format!("[model: {}]", e.model_id)
            }
            SessionEntry::ThinkingLevelChange(e) => {
                format!("[thinking: {}]", e.thinking_level)
            }
            SessionEntry::Label(e) => {
                format!("[label: {}]", e.label.as_deref().unwrap_or("(cleared)"))
            }
            SessionEntry::Custom(e) => {
                format!("[custom: {}]", e.custom_type)
            }
            SessionEntry::CustomMessage(e) => {
                let content_str = Self::extract_user_content(&e.content);
                format!("[{}]: {}", e.custom_type, normalize(&content_str))
            }
        }
    }

    /// Extract text from UserContent
    fn extract_user_content(content: &UserContent) -> String {
        const MAX_LEN: usize = 200;
        match content {
            UserContent::Text(text) => text.chars().take(MAX_LEN).collect(),
            UserContent::Blocks(blocks) => {
                let mut result = String::new();
                for block in blocks {
                    if let crate::core::messages::ContentBlock::Text { text, .. } = block {
                        result.push_str(text);
                        if result.len() >= MAX_LEN {
                            return result.chars().take(MAX_LEN).collect();
                        }
                    }
                }
                result
            }
        }
    }

    /// Extract text from content blocks
    fn extract_content_blocks(content: &[crate::core::messages::ContentBlock]) -> String {
        const MAX_LEN: usize = 200;
        let mut result = String::new();
        for block in content {
            if let crate::core::messages::ContentBlock::Text { text, .. } = block {
                result.push_str(text);
                if result.len() >= MAX_LEN {
                    return result.chars().take(MAX_LEN).collect();
                }
            }
        }
        result
    }

    /// Extract text content from JSON value (for compatibility)
    #[cfg(test)]
    fn extract_text_content(content: &serde_json::Value) -> String {
        const MAX_LEN: usize = 200;

        if let Some(s) = content.as_str() {
            return s.chars().take(MAX_LEN).collect();
        }

        if let Some(arr) = content.as_array() {
            let mut result = String::new();
            for item in arr {
                if let Some(obj) = item.as_object() {
                    if obj.get("type").and_then(|t| t.as_str()) == Some("text") {
                        if let Some(text) = obj.get("text").and_then(|t| t.as_str()) {
                            result.push_str(text);
                            if result.len() >= MAX_LEN {
                                return result.chars().take(MAX_LEN).collect();
                            }
                        }
                    }
                }
            }
            return result;
        }

        String::new()
    }

    /// Apply current filter and search to flat_nodes
    fn apply_filter(&mut self) {
        let previous_id = self
            .filtered_nodes
            .get(self.selected_index)
            .map(|n| n.entry_id.clone());

        let search_tokens: Vec<String> = self
            .search_query
            .to_lowercase()
            .split_whitespace()
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();

        self.filtered_nodes = self
            .flat_nodes
            .iter()
            .filter(|node| {
                // Apply filter mode
                let passes_filter = match self.filter_mode {
                    FilterMode::Default => {
                        // Hide settings/bookkeeping entries
                        !matches!(
                            node.entry_type.as_str(),
                            "label" | "custom" | "model_change" | "thinking_level_change"
                        )
                    }
                    FilterMode::NoTools => {
                        // Default minus tool results
                        !matches!(
                            node.entry_type.as_str(),
                            "label"
                                | "custom"
                                | "model_change"
                                | "thinking_level_change"
                                | "message:toolResult"
                        )
                    }
                    FilterMode::UserOnly => node.entry_type == "message:user",
                    FilterMode::LabeledOnly => node.label.is_some(),
                    FilterMode::All => true,
                };

                if !passes_filter {
                    return false;
                }

                // Apply search filter
                if !search_tokens.is_empty() {
                    let node_text = format!(
                        "{} {} {}",
                        node.display_text.to_lowercase(),
                        node.entry_type.to_lowercase(),
                        node.label.as_deref().unwrap_or("").to_lowercase()
                    );
                    return search_tokens.iter().all(|token| node_text.contains(token));
                }

                true
            })
            .cloned()
            .collect();

        // Try to preserve cursor on the same node
        if let Some(prev_id) = previous_id {
            if let Some(idx) = self
                .filtered_nodes
                .iter()
                .position(|n| n.entry_id == prev_id)
            {
                self.selected_index = idx;
                return;
            }
        }

        // Clamp index if out of bounds
        if self.selected_index >= self.filtered_nodes.len() {
            self.selected_index = self.filtered_nodes.len().saturating_sub(1);
        }
    }

    /// Get currently selected node's entry ID
    pub fn selected_entry_id(&self) -> Option<&str> {
        self.filtered_nodes
            .get(self.selected_index)
            .map(|n| n.entry_id.as_str())
    }

    /// Get the current search query
    pub fn search_query(&self) -> &str {
        &self.search_query
    }

    /// Update a node's label
    pub fn update_node_label(&mut self, entry_id: &str, label: Option<String>) {
        for node in &mut self.flat_nodes {
            if node.entry_id == entry_id {
                node.label = label;
                break;
            }
        }
        self.apply_filter();
    }

    /// Render the tree list
    pub fn render(&self, width: usize) -> Vec<String> {
        let mut lines = Vec::new();

        if self.filtered_nodes.is_empty() {
            lines.push(truncate_to_width("  No entries found", width));
            lines.push(truncate_to_width(
                &format!("  (0/0){}", self.filter_mode.label()),
                width,
            ));
            return lines;
        }

        let start_index = self
            .selected_index
            .saturating_sub(self.max_visible_lines / 2)
            .min(
                self.filtered_nodes
                    .len()
                    .saturating_sub(self.max_visible_lines),
            );
        let end_index = (start_index + self.max_visible_lines).min(self.filtered_nodes.len());

        for i in start_index..end_index {
            let node = &self.filtered_nodes[i];
            let is_selected = i == self.selected_index;

            // Build line: cursor + prefix + path marker + label + content
            let cursor = if is_selected { "› " } else { "  " };

            // If multiple roots, shift display (roots at 0, not 1)
            let display_indent = if self.multiple_roots {
                node.indent.saturating_sub(1)
            } else {
                node.indent
            };

            // Build prefix with gutters and connectors
            let prefix = self.build_prefix(node, display_indent);

            // Active path marker
            let path_marker = if self.active_path_ids.contains(&node.entry_id) {
                "• "
            } else {
                ""
            };

            // Label
            let label = node
                .label
                .as_ref()
                .map(|l| format!("[{}] ", l))
                .unwrap_or_default();

            // Build full line
            let line = format!(
                "{}{}{}{}{}",
                cursor, prefix, path_marker, label, node.display_text
            );

            lines.push(truncate_to_width(&line, width));
        }

        // Position indicator
        lines.push(truncate_to_width(
            &format!(
                "  ({}/{}){}",
                self.selected_index + 1,
                self.filtered_nodes.len(),
                self.filter_mode.label()
            ),
            width,
        ));

        lines
    }

    /// Build the ASCII art prefix for a node
    fn build_prefix(&self, node: &FlatNode, display_indent: usize) -> String {
        let connector_position = if node.show_connector && !node.is_virtual_root_child {
            display_indent.saturating_sub(1)
        } else {
            usize::MAX
        };

        let total_chars = display_indent * 3;
        let mut prefix_chars = Vec::with_capacity(total_chars);

        for char_idx in 0..total_chars {
            let level = char_idx / 3;
            let pos_in_level = char_idx % 3;

            // Check if there's a gutter at this level
            let gutter = node.gutters.iter().find(|g| g.position == level);

            if let Some(g) = gutter {
                if pos_in_level == 0 {
                    prefix_chars.push(if g.show { '│' } else { ' ' });
                } else {
                    prefix_chars.push(' ');
                }
            } else if level == connector_position {
                // Connector at this level
                match pos_in_level {
                    0 => prefix_chars.push(if node.is_last { '└' } else { '├' }),
                    1 => prefix_chars.push('─'),
                    _ => prefix_chars.push(' '),
                }
            } else {
                prefix_chars.push(' ');
            }
        }

        prefix_chars.into_iter().collect()
    }

    /// Handle keyboard input
    pub fn handle_input(&mut self, key_data: &str) {
        // Navigation
        if matches_key(key_data, "up") {
            if self.selected_index == 0 {
                self.selected_index = self.filtered_nodes.len().saturating_sub(1);
            } else {
                self.selected_index -= 1;
            }
        } else if matches_key(key_data, "down") {
            if self.selected_index >= self.filtered_nodes.len().saturating_sub(1) {
                self.selected_index = 0;
            } else {
                self.selected_index += 1;
            }
        } else if matches_key(key_data, "left") {
            // Page up
            self.selected_index = self.selected_index.saturating_sub(self.max_visible_lines);
        } else if matches_key(key_data, "right") {
            // Page down
            self.selected_index = (self.selected_index + self.max_visible_lines)
                .min(self.filtered_nodes.len().saturating_sub(1));
        } else if matches_key(key_data, "enter") {
            let entry_id = self.selected_entry_id().map(String::from);
            if let (Some(callback), Some(id)) = (&mut self.on_select, entry_id) {
                callback(&id);
            }
        } else if matches_key(key_data, "escape") {
            if !self.search_query.is_empty() {
                self.search_query.clear();
                self.apply_filter();
            } else if let Some(callback) = &mut self.on_cancel {
                callback();
            }
        } else if matches_key(key_data, "ctrl+shift+o") {
            // Cycle filter backwards
            self.filter_mode = self.filter_mode.prev();
            self.apply_filter();
        } else if matches_key(key_data, "ctrl+o") {
            // Cycle filter forwards
            self.filter_mode = self.filter_mode.next();
            self.apply_filter();
        } else if matches_key(key_data, "backspace") {
            if !self.search_query.is_empty() {
                self.search_query.pop();
                self.apply_filter();
            }
        } else if key_data == "l" && self.search_query.is_empty() {
            // Label edit
            let entry = self.filtered_nodes.get(self.selected_index).cloned();
            if let (Some(callback), Some(node)) = (&mut self.on_label_edit, entry) {
                callback(&node.entry_id, node.label.as_deref());
            }
        } else {
            // Check for printable character (search input)
            let has_control = key_data
                .chars()
                .any(|c| c as u32 <= 31 || c as u32 == 0x7f || (0x80..=0x9f).contains(&(c as u32)));
            if !has_control && !key_data.is_empty() {
                self.search_query.push_str(key_data);
                self.apply_filter();
            }
        }
    }
}

/// Session tree selector component
pub struct TreeSelectorComponent {
    tree_list: TreeList,
}

impl TreeSelectorComponent {
    /// Create a new tree selector
    pub fn new(
        tree: Vec<SessionTreeNode>,
        current_leaf_id: Option<String>,
        terminal_height: usize,
    ) -> Self {
        let max_visible_lines = (terminal_height / 2).max(5);
        let tree_list = TreeList::new(tree, current_leaf_id, max_visible_lines);

        Self { tree_list }
    }

    /// Set callback for when an entry is selected
    pub fn on_select<F>(&mut self, callback: F)
    where
        F: FnMut(&str) + 'static,
    {
        self.tree_list.on_select = Some(Box::new(callback));
    }

    /// Set callback for when selection is cancelled
    pub fn on_cancel<F>(&mut self, callback: F)
    where
        F: FnMut() + 'static,
    {
        self.tree_list.on_cancel = Some(Box::new(callback));
    }

    /// Set callback for when label edit is requested
    pub fn on_label_edit<F>(&mut self, callback: F)
    where
        F: FnMut(&str, Option<&str>) + 'static,
    {
        self.tree_list.on_label_edit = Some(Box::new(callback));
    }

    /// Get currently selected entry ID
    pub fn selected_entry_id(&self) -> Option<&str> {
        self.tree_list.selected_entry_id()
    }

    /// Update a node's label
    pub fn update_node_label(&mut self, entry_id: &str, label: Option<String>) {
        self.tree_list.update_node_label(entry_id, label);
    }

    /// Render the tree selector
    pub fn render(&self, width: usize) -> Vec<String> {
        let mut lines = Vec::new();

        // Header
        lines.push(truncate_to_width(&"─".repeat(width), width));
        lines.push(truncate_to_width("  Session Tree", width));
        lines.push(truncate_to_width(
            "  ↑/↓: move. ←/→: page. l: label. ^O/⇧^O: filter. Type to search",
            width,
        ));

        // Search line
        let query = self.tree_list.search_query();
        if query.is_empty() {
            lines.push(truncate_to_width("  Search:", width));
        } else {
            lines.push(truncate_to_width(&format!("  Search: {}", query), width));
        }

        lines.push(truncate_to_width(&"─".repeat(width), width));
        lines.push(String::new()); // spacer

        // Tree list
        lines.extend(self.tree_list.render(width));

        lines.push(String::new()); // spacer
        lines.push(truncate_to_width(&"─".repeat(width), width));

        lines
    }

    /// Handle keyboard input
    pub fn handle_input(&mut self, key_data: &str) {
        self.tree_list.handle_input(key_data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_tree() {
        let tree_list = TreeList::new(vec![], None, 10);
        assert!(tree_list.filtered_nodes.is_empty());
    }

    #[test]
    fn test_filter_mode_cycling() {
        assert_eq!(FilterMode::Default.next(), FilterMode::NoTools);
        assert_eq!(FilterMode::NoTools.next(), FilterMode::UserOnly);
        assert_eq!(FilterMode::UserOnly.next(), FilterMode::LabeledOnly);
        assert_eq!(FilterMode::LabeledOnly.next(), FilterMode::All);
        assert_eq!(FilterMode::All.next(), FilterMode::Default);

        assert_eq!(FilterMode::Default.prev(), FilterMode::All);
        assert_eq!(FilterMode::All.prev(), FilterMode::LabeledOnly);
    }

    #[test]
    fn test_filter_mode_labels() {
        assert_eq!(FilterMode::Default.label(), "");
        assert_eq!(FilterMode::NoTools.label(), " [no-tools]");
        assert_eq!(FilterMode::UserOnly.label(), " [user]");
        assert_eq!(FilterMode::LabeledOnly.label(), " [labeled]");
        assert_eq!(FilterMode::All.label(), " [all]");
    }

    #[test]
    fn test_extract_text_content_string() {
        let content = serde_json::json!("hello world");
        let result = TreeList::extract_text_content(&content);
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_extract_text_content_array() {
        let content = serde_json::json!([
            {"type": "text", "text": "hello "},
            {"type": "text", "text": "world"}
        ]);
        let result = TreeList::extract_text_content(&content);
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_extract_text_content_empty() {
        let content = serde_json::json!({});
        let result = TreeList::extract_text_content(&content);
        assert_eq!(result, "");
    }

    #[test]
    fn test_extract_text_content_truncation() {
        let long_text = "a".repeat(300);
        let content = serde_json::json!(long_text);
        let result = TreeList::extract_text_content(&content);
        assert_eq!(result.len(), 200);
    }
}
