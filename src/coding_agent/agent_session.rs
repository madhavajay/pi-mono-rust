use crate::agent::{
    Agent, AgentError, AgentEvent, AgentMessage, AgentTool, AgentToolResult, CustomMessage,
    ThinkingLevel,
};
use crate::coding_agent::export_html::export_session_to_html;
use crate::coding_agent::extension_host::ExtensionHost;
use crate::coding_agent::hooks::{
    CompactionHook, CompactionResult, SessionBeforeCompactEvent, SessionCompactEvent,
};
use crate::coding_agent::prompt_templates::{expand_prompt_template, PromptTemplate};
use crate::coding_agent::ModelRegistry;
use crate::config;
use crate::core::compaction::prepare_compaction;
use crate::core::messages::{
    AgentMessage as CoreAgentMessage, BashExecutionMessage, ContentBlock, UserContent, UserMessage,
};
use crate::core::session_manager::{BranchSummaryEntry, SessionEntry, SessionManager};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cell::{Cell, RefCell};
use std::env;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::rc::Rc;

pub struct AgentSessionConfig {
    pub agent: Agent,
    pub session_manager: SessionManager,
    pub settings_manager: SettingsManager,
    pub model_registry: ModelRegistry,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AgentSessionEvent {
    Agent(Box<AgentEvent>),
    AutoCompactionStart { reason: String },
    AutoCompactionEnd { aborted: bool },
}

pub type AgentSessionEventListener = Box<dyn Fn(&AgentSessionEvent)>;

pub struct AgentSession {
    pub agent: Agent,
    pub session_manager: SessionManager,
    pub settings_manager: SettingsManager,
    pub model_registry: ModelRegistry,
    prompt_templates: Vec<PromptTemplate>,
    branch_summary_aborted: Cell<bool>,
    compaction_hooks: Vec<CompactionHook>,
    extension_host: Option<Rc<RefCell<ExtensionHost>>>,
    tools_wrapped_with_extensions: bool,
    listeners: Rc<RefCell<Vec<(usize, AgentSessionEventListener)>>>,
    next_listener_id: Rc<RefCell<usize>>,
    unsubscribe_agent: Option<Box<dyn FnOnce()>>,
}

impl AgentSession {
    pub fn new(config: AgentSessionConfig) -> Self {
        let listeners = Rc::new(RefCell::new(
            Vec::<(usize, AgentSessionEventListener)>::new(),
        ));
        let next_listener_id = Rc::new(RefCell::new(0));
        let agent = config.agent;
        let session_manager = config.session_manager;
        let settings_manager = config.settings_manager;
        let model_registry = config.model_registry;

        let context = session_manager.build_session_context();
        let messages: Vec<AgentMessage> = context
            .messages
            .iter()
            .filter_map(convert_core_message)
            .collect();
        if !messages.is_empty() {
            agent.replace_messages(messages);
        }
        if let Some(model) = context.model.clone() {
            if let Some(match_model) = model_registry.find(&model.provider, &model.model_id) {
                agent.set_model(crate::agent::Model {
                    id: match_model.id,
                    name: match_model.name,
                    api: match_model.api,
                    provider: match_model.provider,
                });
            }
        }
        if let Some(level) = thinking_level_from_str(&context.thinking_level) {
            agent.set_thinking_level(level);
        }

        let listeners_ref = listeners.clone();
        let unsubscribe = agent.subscribe(move |event| {
            let session_event = AgentSessionEvent::Agent(Box::new(event.clone()));
            for (_, listener) in listeners_ref.borrow().iter() {
                listener(&session_event);
            }
        });

        Self {
            agent,
            session_manager,
            settings_manager,
            model_registry,
            prompt_templates: Vec::new(),
            branch_summary_aborted: Cell::new(false),
            compaction_hooks: Vec::new(),
            extension_host: None,
            tools_wrapped_with_extensions: false,
            listeners,
            next_listener_id,
            unsubscribe_agent: Some(Box::new(unsubscribe)),
        }
    }

    pub fn subscribe<F>(&self, listener: F) -> impl FnOnce()
    where
        F: Fn(&AgentSessionEvent) + 'static,
    {
        let id = {
            let mut next_id = self.next_listener_id.borrow_mut();
            let id = *next_id;
            *next_id += 1;
            id
        };
        self.listeners.borrow_mut().push((id, Box::new(listener)));
        let listeners = self.listeners.clone();
        move || {
            listeners
                .borrow_mut()
                .retain(|(listener_id, _)| *listener_id != id);
        }
    }

    pub fn dispose(&mut self) {
        if let Some(unsubscribe) = self.unsubscribe_agent.take() {
            unsubscribe();
        }
        self.listeners.borrow_mut().clear();
    }

    pub fn is_streaming(&self) -> bool {
        self.agent.state().is_streaming
    }

    pub fn session_file(&self) -> Option<PathBuf> {
        self.session_manager.get_session_file()
    }

    pub fn session_id(&self) -> String {
        self.session_manager.get_session_id()
    }

    pub fn messages(&self) -> Vec<AgentMessage> {
        self.agent.state().messages
    }

    pub fn set_prompt_templates(&mut self, templates: Vec<PromptTemplate>) {
        self.prompt_templates = templates;
    }

    pub fn prompt_templates(&self) -> &[PromptTemplate] {
        &self.prompt_templates
    }

    pub fn pending_message_count(&self) -> usize {
        self.agent.pending_steering_count() + self.agent.pending_follow_up_count()
    }

    pub fn prompt(&mut self, text: &str) -> Result<(), AgentSessionError> {
        if self.is_streaming() {
            return Err(AgentSessionError::AlreadyStreaming);
        }

        let before_len = self.agent.state().messages.len();
        let expanded_text = self.expand_prompt_text(text);
        self.agent
            .prompt(expanded_text.as_str())
            .map_err(AgentSessionError::Agent)?;
        let messages = self.agent.state().messages;
        for message in messages.into_iter().skip(before_len) {
            if let Some(core_message) = convert_message(&message) {
                self.session_manager.append_message(core_message);
            }
        }
        Ok(())
    }

    pub fn prompt_content(&mut self, content: UserContent) -> Result<(), AgentSessionError> {
        if self.is_streaming() {
            return Err(AgentSessionError::AlreadyStreaming);
        }

        let before_len = self.agent.state().messages.len();
        let content = self.expand_user_content(content);
        let message = AgentMessage::User(UserMessage {
            content,
            timestamp: now_millis(),
        });
        self.agent
            .prompt(message)
            .map_err(AgentSessionError::Agent)?;
        let messages = self.agent.state().messages;
        for message in messages.into_iter().skip(before_len) {
            if let Some(core_message) = convert_message(&message) {
                self.session_manager.append_message(core_message);
            }
        }
        Ok(())
    }

    pub fn steer(&self, text: &str) {
        let expanded_text = self.expand_prompt_text(text);
        self.agent.steer(AgentMessage::User(UserMessage {
            content: UserContent::Text(expanded_text),
            timestamp: now_millis(),
        }));
    }

    pub fn follow_up(&self, text: &str) {
        let expanded_text = self.expand_prompt_text(text);
        self.agent.follow_up(AgentMessage::User(UserMessage {
            content: UserContent::Text(expanded_text),
            timestamp: now_millis(),
        }));
    }

    fn expand_prompt_text(&self, text: &str) -> String {
        if self.prompt_templates.is_empty() {
            return text.to_string();
        }
        expand_prompt_template(text, &self.prompt_templates)
    }

    fn expand_user_content(&self, content: UserContent) -> UserContent {
        match content {
            UserContent::Text(text) => UserContent::Text(self.expand_prompt_text(&text)),
            UserContent::Blocks(mut blocks) => {
                let mut expanded = false;
                for block in &mut blocks {
                    if expanded {
                        break;
                    }
                    if let ContentBlock::Text { text, .. } = block {
                        if text.starts_with('/') {
                            *text = self.expand_prompt_text(text);
                        }
                        expanded = true;
                    }
                }
                UserContent::Blocks(blocks)
            }
        }
    }

    pub fn abort(&self) {
        self.agent.abort();
    }

    pub fn set_model(&mut self, model: crate::agent::Model) {
        self.agent.set_model(model.clone());
        self.session_manager
            .append_model_change(&model.provider, &model.id);
    }

    pub fn set_steering_mode(&mut self, mode: crate::agent::QueueMode) {
        self.agent.set_steering_mode(mode);
    }

    pub fn steering_mode(&self) -> crate::agent::QueueMode {
        self.agent.get_steering_mode()
    }

    pub fn set_follow_up_mode(&mut self, mode: crate::agent::QueueMode) {
        self.agent.set_follow_up_mode(mode);
    }

    pub fn follow_up_mode(&self) -> crate::agent::QueueMode {
        self.agent.get_follow_up_mode()
    }

    pub fn set_compaction_hooks(&mut self, hooks: Vec<CompactionHook>) {
        self.compaction_hooks = hooks;
    }

    pub fn set_extension_host(&mut self, host: ExtensionHost) {
        let host = Rc::new(RefCell::new(host));
        let before_host = host.clone();
        let after_host = host.clone();
        let hook = CompactionHook::new(
            Some(Box::new(move |event| {
                before_host
                    .borrow_mut()
                    .emit_before_compact(event)
                    .unwrap_or_default()
            })),
            Some(Box::new(move |event| {
                let _ = after_host.borrow_mut().emit_compact(event);
            })),
        );
        self.compaction_hooks.push(hook);
        self.extension_host = Some(host);
        self.wrap_tools_with_extensions();
    }

    fn wrap_tools_with_extensions(&mut self) {
        if self.tools_wrapped_with_extensions {
            return;
        }
        let Some(host) = self.extension_host.clone() else {
            return;
        };
        let tools = self.agent.state().tools;
        if tools.is_empty() {
            return;
        }
        let wrapped = wrap_tools_with_extension_host(tools, host);
        self.agent.set_tools(wrapped);
        self.tools_wrapped_with_extensions = true;
    }

    pub fn abort_branch_summary(&self) {
        self.branch_summary_aborted.set(true);
    }

    pub fn get_user_messages_for_branching(&self) -> Vec<BranchCandidate> {
        let entries = self.session_manager.get_entries();
        let mut results = Vec::new();

        for entry in entries {
            let SessionEntry::Message(message_entry) = entry else {
                continue;
            };
            let CoreAgentMessage::User(user) = &message_entry.message else {
                continue;
            };
            let text = extract_user_text(&user.content);
            if !text.is_empty() {
                results.push(BranchCandidate {
                    entry_id: message_entry.id.clone(),
                    text,
                });
            }
        }

        results
    }

    pub fn branch(&mut self, entry_id: &str) -> Result<BranchResult, AgentSessionError> {
        let entry = self
            .session_manager
            .get_entry(entry_id)
            .ok_or(AgentSessionError::InvalidBranchEntry)?;

        let SessionEntry::Message(message_entry) = entry else {
            return Err(AgentSessionError::InvalidBranchEntry);
        };

        let CoreAgentMessage::User(user) = &message_entry.message else {
            return Err(AgentSessionError::InvalidBranchEntry);
        };

        let selected_text = extract_user_text(&user.content);
        let parent_id = message_entry.parent_id.clone();

        if let Some(parent_id) = parent_id {
            self.session_manager
                .create_branched_session(&parent_id)
                .map_err(AgentSessionError::Session)?;
        } else {
            self.session_manager.new_session(None);
        }

        let context = self.session_manager.build_session_context();
        let messages = context
            .messages
            .iter()
            .filter_map(convert_core_message)
            .collect();
        self.agent.replace_messages(messages);

        Ok(BranchResult {
            selected_text,
            cancelled: false,
        })
    }

    pub fn navigate_tree(
        &mut self,
        target_id: &str,
        options: NavigateTreeOptions,
    ) -> Result<NavigateTreeResult, AgentSessionError> {
        let old_leaf_id = self.session_manager.get_leaf_id();
        if old_leaf_id.as_deref() == Some(target_id) {
            return Ok(NavigateTreeResult {
                editor_text: None,
                cancelled: false,
                aborted: false,
                summary_entry: None,
            });
        }

        let target_entry = self
            .session_manager
            .get_entry(target_id)
            .ok_or(AgentSessionError::InvalidTreeTarget)?;

        let summarize = options.summarize;
        let was_aborted = self.branch_summary_aborted.get();
        self.branch_summary_aborted.set(false);

        let (entries_to_summarize, _common_ancestor) = collect_entries_for_branch_summary(
            &self.session_manager,
            old_leaf_id.as_deref(),
            target_id,
        );

        if summarize && was_aborted {
            return Ok(NavigateTreeResult {
                editor_text: None,
                cancelled: true,
                aborted: true,
                summary_entry: None,
            });
        }

        let summary_text = if summarize && !entries_to_summarize.is_empty() {
            Some(summarize_entries(
                &entries_to_summarize,
                options.custom_instructions.as_deref(),
            ))
        } else {
            None
        };

        if summarize && self.branch_summary_aborted.get() {
            return Ok(NavigateTreeResult {
                editor_text: None,
                cancelled: true,
                aborted: true,
                summary_entry: None,
            });
        }

        let (new_leaf_id, editor_text) = match &target_entry {
            SessionEntry::Message(message_entry) => match &message_entry.message {
                CoreAgentMessage::User(user) => (
                    message_entry.parent_id.clone(),
                    Some(extract_user_text(&user.content)),
                ),
                _ => (Some(message_entry.id.clone()), None),
            },
            SessionEntry::CustomMessage(custom) => (
                custom.parent_id.clone(),
                Some(extract_user_text(&custom.content)),
            ),
            _ => (Some(target_id.to_string()), None),
        };

        let mut summary_entry = None;
        if let Some(summary_text) = summary_text {
            let summary_id = self
                .session_manager
                .branch_with_summary(new_leaf_id.as_deref(), &summary_text, None, None)
                .map_err(AgentSessionError::Session)?;
            if let Some(SessionEntry::BranchSummary(entry)) =
                self.session_manager.get_entry(&summary_id)
            {
                summary_entry = Some(entry);
            }
        } else if new_leaf_id.is_none() {
            self.session_manager.reset_leaf();
        } else if let Some(new_leaf_id) = new_leaf_id.as_deref() {
            self.session_manager
                .branch(new_leaf_id)
                .map_err(AgentSessionError::Session)?;
        }

        let context = self.session_manager.build_session_context();
        let messages = context
            .messages
            .iter()
            .filter_map(convert_core_message)
            .collect();
        self.agent.replace_messages(messages);

        Ok(NavigateTreeResult {
            editor_text,
            cancelled: false,
            aborted: false,
            summary_entry,
        })
    }

    pub fn compact(&mut self) -> Result<CompactionResult, AgentSessionError> {
        let branch_entries = self.session_manager.get_branch(None);
        let settings = self.settings_manager.get_compaction_settings();
        let preparation = prepare_compaction(&branch_entries, settings).ok_or_else(|| {
            AgentSessionError::Compaction("Compaction not applicable".to_string())
        })?;

        let before_event = SessionBeforeCompactEvent {
            preparation: preparation.clone(),
            branch_entries: branch_entries.clone(),
        };

        let mut hook_compaction: Option<CompactionResult> = None;
        for hook in &self.compaction_hooks {
            let Some(handler) = &hook.on_before_compact else {
                continue;
            };
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(&before_event)));
            let result = match result {
                Ok(result) => result,
                Err(_) => continue,
            };
            if result.cancel == Some(true) {
                return Err(AgentSessionError::Compaction(
                    "Compaction cancelled".to_string(),
                ));
            }
            if let Some(compaction) = result.compaction {
                hook_compaction = Some(compaction);
            }
        }

        let mut summary = summarize_compaction_messages(&preparation.messages_to_summarize);
        if summary.trim().is_empty() {
            summary = "Summary.".to_string();
        }

        let mut result = CompactionResult {
            summary,
            first_kept_entry_id: preparation.first_kept_entry_id.clone(),
            tokens_before: preparation.tokens_before,
        };

        let mut from_hook = false;
        if let Some(compaction) = hook_compaction {
            result = compaction;
            from_hook = true;
        }

        self.session_manager.append_compaction(
            &result.summary,
            &result.first_kept_entry_id,
            result.tokens_before,
        );

        let compaction_entry = match self.session_manager.get_leaf_entry() {
            Some(SessionEntry::Compaction(entry)) => entry,
            _ => {
                return Err(AgentSessionError::Compaction(
                    "Failed to persist compaction entry".to_string(),
                ))
            }
        };
        let compact_event = SessionCompactEvent {
            compaction_entry,
            from_hook,
        };
        for hook in &self.compaction_hooks {
            let Some(handler) = &hook.on_compact else {
                continue;
            };
            let _ =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(&compact_event)));
        }

        let context = self.session_manager.build_session_context();
        let messages = context
            .messages
            .iter()
            .filter_map(convert_core_message)
            .collect();
        self.agent.replace_messages(messages);

        Ok(result)
    }

    pub fn get_state(&self) -> AgentSessionState {
        let state = self.agent.state();
        AgentSessionState {
            model: state.model,
            thinking_level: state.thinking_level,
            is_streaming: state.is_streaming,
            message_count: state.messages.len(),
        }
    }

    pub fn set_thinking_level(&mut self, level: ThinkingLevel) {
        self.agent.set_thinking_level(level);
        self.session_manager
            .append_thinking_level_change(level.as_str());
    }

    pub fn cycle_thinking_level(&mut self) -> ThinkingLevelCycleResult {
        let levels = [
            ThinkingLevel::Off,
            ThinkingLevel::Minimal,
            ThinkingLevel::Low,
            ThinkingLevel::Medium,
            ThinkingLevel::High,
            ThinkingLevel::XHigh,
        ];
        let current = self.agent.state().thinking_level;
        let index = levels
            .iter()
            .position(|level| *level == current)
            .unwrap_or(0);
        let next = levels[(index + 1) % levels.len()];
        self.set_thinking_level(next);
        ThinkingLevelCycleResult { level: next }
    }

    pub fn set_auto_compaction_enabled(&mut self, enabled: bool) {
        self.settings_manager.set_compaction_enabled(enabled);
    }

    pub fn auto_compaction_enabled(&self) -> bool {
        self.settings_manager.is_compaction_enabled()
    }

    pub fn set_auto_retry_enabled(&mut self, enabled: bool) {
        self.settings_manager.set_retry_enabled(enabled);
    }

    pub fn auto_retry_enabled(&self) -> bool {
        self.settings_manager.retry_enabled()
    }

    pub fn abort_retry(&mut self) {}

    pub fn abort_bash(&mut self) {}

    pub fn cycle_model(&mut self) -> Option<ModelCycleResult> {
        let models = self.model_registry.get_available();
        if models.is_empty() {
            return None;
        }

        let current = self.agent.state().model;
        let current_index = models
            .iter()
            .position(|model| model.provider == current.provider && model.id == current.id);
        let next_index = match current_index {
            Some(index) => (index + 1) % models.len(),
            None => 0,
        };
        let next = models[next_index].clone();
        self.set_model(crate::agent::Model {
            id: next.id.clone(),
            name: next.name.clone(),
            api: next.api.clone(),
            provider: next.provider.clone(),
        });

        Some(ModelCycleResult {
            model: next,
            thinking_level: self.agent.state().thinking_level,
            is_scoped: false,
        })
    }

    pub fn get_available_models(&self) -> Vec<crate::coding_agent::Model> {
        self.model_registry.get_available()
    }

    pub fn get_session_stats(&self) -> SessionStats {
        let messages = self.agent.state().messages;
        let mut user_messages = 0;
        let mut assistant_messages = 0;
        let mut tool_results = 0;
        let mut tool_calls = 0;

        let mut input = 0;
        let mut output = 0;
        let mut cache_read = 0;
        let mut cache_write = 0;
        let mut cost = 0.0;

        for message in &messages {
            match message {
                AgentMessage::User(_) => user_messages += 1,
                AgentMessage::Assistant(assistant) => {
                    assistant_messages += 1;
                    tool_calls += assistant
                        .content
                        .iter()
                        .filter(|block| matches!(block, ContentBlock::ToolCall { .. }))
                        .count();
                    input += assistant.usage.input;
                    output += assistant.usage.output;
                    cache_read += assistant.usage.cache_read;
                    cache_write += assistant.usage.cache_write;
                    if let Some(usage_cost) = &assistant.usage.cost {
                        cost += usage_cost.total;
                    }
                }
                AgentMessage::ToolResult(_) => tool_results += 1,
                _ => {}
            }
        }

        SessionStats {
            session_file: self.session_manager.get_session_file(),
            session_id: self.session_manager.get_session_id(),
            user_messages,
            assistant_messages,
            tool_calls,
            tool_results,
            total_messages: messages.len(),
            tokens: TokenStats {
                input,
                output,
                cache_read,
                cache_write,
                total: input + output + cache_read + cache_write,
            },
            cost,
        }
    }

    pub fn new_session(&mut self) {
        self.session_manager.new_session(None);
        self.agent.abort();
        self.agent.clear_messages();
        self.agent.clear_all_queues();
    }

    pub fn export_to_html_with_path(
        &self,
        output_path: Option<&PathBuf>,
    ) -> Result<ExportResult, AgentSessionError> {
        let state = self.agent.state();
        let output_path = output_path.cloned();
        let path = export_session_to_html(&self.session_manager, Some(&state), output_path)
            .map_err(AgentSessionError::Session)?;
        Ok(ExportResult { path })
    }

    pub fn export_to_html(&self) -> Result<ExportResult, AgentSessionError> {
        self.export_to_html_with_path(None)
    }

    pub fn execute_bash(&mut self, command: &str) -> Result<BashResult, AgentSessionError> {
        let output = Command::new("sh")
            .arg("-c")
            .arg(command)
            .output()
            .map_err(|err| AgentSessionError::Session(err.to_string()))?;
        let mut combined = String::new();
        combined.push_str(&String::from_utf8_lossy(&output.stdout));
        combined.push_str(&String::from_utf8_lossy(&output.stderr));

        let exit_code = output.status.code().map(|code| code as i64);
        let timestamp = now_millis();
        let message = BashExecutionMessage {
            command: command.to_string(),
            output: combined.clone(),
            exit_code,
            cancelled: false,
            truncated: false,
            full_output_path: None,
            timestamp,
            exclude_from_context: None,
        };
        self.session_manager
            .append_message(CoreAgentMessage::BashExecution(message));
        self.agent
            .append_message(AgentMessage::Custom(CustomMessage {
                role: "bashExecution".to_string(),
                text: combined.clone(),
                timestamp,
            }));

        Ok(BashResult {
            output: combined,
            exit_code,
            cancelled: false,
        })
    }

    pub fn get_last_assistant_text(&self) -> Option<String> {
        let messages = self.agent.state().messages;
        for message in messages.iter().rev() {
            if let AgentMessage::Assistant(assistant) = message {
                if assistant.stop_reason == "aborted" && assistant.content.is_empty() {
                    continue;
                }
                let mut text = String::new();
                for block in &assistant.content {
                    if let ContentBlock::Text { text: chunk, .. } = block {
                        text.push_str(chunk);
                    }
                }
                if !text.trim().is_empty() {
                    return Some(text);
                }
            }
        }
        None
    }

    pub fn switch_session(&mut self, session_path: PathBuf) -> Result<bool, AgentSessionError> {
        self.agent.abort();
        self.agent.clear_all_queues();

        self.session_manager.set_session_file(session_path);
        let context = self.session_manager.build_session_context();
        let messages = context
            .messages
            .iter()
            .filter_map(convert_core_message)
            .collect();
        self.agent.replace_messages(messages);

        if let Some(model) = context.model {
            if let Some(match_model) = self.model_registry.find(&model.provider, &model.model_id) {
                self.agent.set_model(crate::agent::Model {
                    id: match_model.id,
                    name: match_model.name,
                    api: match_model.api,
                    provider: match_model.provider,
                });
            }
        }

        if let Some(level) = thinking_level_from_str(&context.thinking_level) {
            self.agent.set_thinking_level(level);
        }

        Ok(true)
    }
}

#[derive(Debug)]
pub enum AgentSessionError {
    AlreadyStreaming,
    Agent(AgentError),
    InvalidBranchEntry,
    InvalidTreeTarget,
    Compaction(String),
    Session(String),
}

impl std::fmt::Display for AgentSessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentSessionError::AlreadyStreaming => write!(
                f,
                "Agent is already processing. Specify streamingBehavior ('steer' or 'followUp') to queue the message."
            ),
            AgentSessionError::Agent(err) => write!(f, "{err}"),
            AgentSessionError::InvalidBranchEntry => write!(f, "Invalid entry ID for branching"),
            AgentSessionError::InvalidTreeTarget => write!(f, "Entry not found for navigation"),
            AgentSessionError::Compaction(err) => write!(f, "{err}"),
            AgentSessionError::Session(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for AgentSessionError {}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsCompaction {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reserve_tokens: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_recent_tokens: Option<i64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsBranchSummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reserve_tokens: Option<i64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsRetry {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_retries: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_delay_ms: Option<i64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSkills {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_codex_user: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_claude_user: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_claude_project: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_pi_user: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_pi_project: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_directories: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignored_skills: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_skills: Option<Vec<String>>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsTerminal {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub show_images: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsImages {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_resize: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_changelog_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_thinking_level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steering_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub follow_up_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compaction: Option<SettingsCompaction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch_summary: Option<SettingsBranchSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry: Option<SettingsRetry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hide_thinking_block: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collapse_changelog: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skills: Option<SettingsSkills>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal: Option<SettingsTerminal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub images: Option<SettingsImages>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled_models: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub double_escape_action: Option<String>,
}

fn merge_settings(base: &Settings, overrides: &Settings) -> Settings {
    Settings {
        last_changelog_version: overrides
            .last_changelog_version
            .clone()
            .or_else(|| base.last_changelog_version.clone()),
        default_provider: overrides
            .default_provider
            .clone()
            .or_else(|| base.default_provider.clone()),
        default_model: overrides
            .default_model
            .clone()
            .or_else(|| base.default_model.clone()),
        default_thinking_level: overrides
            .default_thinking_level
            .clone()
            .or_else(|| base.default_thinking_level.clone()),
        steering_mode: overrides
            .steering_mode
            .clone()
            .or_else(|| base.steering_mode.clone()),
        follow_up_mode: overrides
            .follow_up_mode
            .clone()
            .or_else(|| base.follow_up_mode.clone()),
        theme: overrides.theme.clone().or_else(|| base.theme.clone()),
        compaction: merge_optional_nested(
            base.compaction.as_ref(),
            overrides.compaction.as_ref(),
            merge_compaction,
        ),
        branch_summary: merge_optional_nested(
            base.branch_summary.as_ref(),
            overrides.branch_summary.as_ref(),
            merge_branch_summary,
        ),
        retry: merge_optional_nested(base.retry.as_ref(), overrides.retry.as_ref(), merge_retry),
        hide_thinking_block: overrides.hide_thinking_block.or(base.hide_thinking_block),
        shell_path: overrides
            .shell_path
            .clone()
            .or_else(|| base.shell_path.clone()),
        collapse_changelog: overrides.collapse_changelog.or(base.collapse_changelog),
        extensions: overrides
            .extensions
            .clone()
            .or_else(|| base.extensions.clone()),
        skills: merge_optional_nested(
            base.skills.as_ref(),
            overrides.skills.as_ref(),
            merge_skills,
        ),
        terminal: merge_optional_nested(
            base.terminal.as_ref(),
            overrides.terminal.as_ref(),
            merge_terminal,
        ),
        images: merge_optional_nested(
            base.images.as_ref(),
            overrides.images.as_ref(),
            merge_images,
        ),
        enabled_models: overrides
            .enabled_models
            .clone()
            .or_else(|| base.enabled_models.clone()),
        double_escape_action: overrides
            .double_escape_action
            .clone()
            .or_else(|| base.double_escape_action.clone()),
    }
}

fn merge_optional_nested<T, F>(base: Option<&T>, overrides: Option<&T>, merge: F) -> Option<T>
where
    T: Clone,
    F: Fn(&T, &T) -> T,
{
    match (base, overrides) {
        (Some(base), Some(overrides)) => Some(merge(base, overrides)),
        (None, Some(overrides)) => Some(overrides.clone()),
        (Some(base), None) => Some(base.clone()),
        (None, None) => None,
    }
}

fn merge_compaction(
    base: &SettingsCompaction,
    overrides: &SettingsCompaction,
) -> SettingsCompaction {
    SettingsCompaction {
        enabled: overrides.enabled.or(base.enabled),
        reserve_tokens: overrides.reserve_tokens.or(base.reserve_tokens),
        keep_recent_tokens: overrides.keep_recent_tokens.or(base.keep_recent_tokens),
    }
}

fn merge_branch_summary(
    base: &SettingsBranchSummary,
    overrides: &SettingsBranchSummary,
) -> SettingsBranchSummary {
    SettingsBranchSummary {
        reserve_tokens: overrides.reserve_tokens.or(base.reserve_tokens),
    }
}

fn merge_retry(base: &SettingsRetry, overrides: &SettingsRetry) -> SettingsRetry {
    SettingsRetry {
        enabled: overrides.enabled.or(base.enabled),
        max_retries: overrides.max_retries.or(base.max_retries),
        base_delay_ms: overrides.base_delay_ms.or(base.base_delay_ms),
    }
}

fn merge_skills(base: &SettingsSkills, overrides: &SettingsSkills) -> SettingsSkills {
    SettingsSkills {
        enabled: overrides.enabled.or(base.enabled),
        enable_codex_user: overrides.enable_codex_user.or(base.enable_codex_user),
        enable_claude_user: overrides.enable_claude_user.or(base.enable_claude_user),
        enable_claude_project: overrides
            .enable_claude_project
            .or(base.enable_claude_project),
        enable_pi_user: overrides.enable_pi_user.or(base.enable_pi_user),
        enable_pi_project: overrides.enable_pi_project.or(base.enable_pi_project),
        custom_directories: overrides
            .custom_directories
            .clone()
            .or_else(|| base.custom_directories.clone()),
        ignored_skills: overrides
            .ignored_skills
            .clone()
            .or_else(|| base.ignored_skills.clone()),
        include_skills: overrides
            .include_skills
            .clone()
            .or_else(|| base.include_skills.clone()),
    }
}

fn merge_terminal(base: &SettingsTerminal, overrides: &SettingsTerminal) -> SettingsTerminal {
    SettingsTerminal {
        show_images: overrides.show_images.or(base.show_images),
    }
}

fn merge_images(base: &SettingsImages, overrides: &SettingsImages) -> SettingsImages {
    SettingsImages {
        auto_resize: overrides.auto_resize.or(base.auto_resize),
    }
}

pub struct SettingsManager {
    settings_path: Option<PathBuf>,
    project_settings_path: Option<PathBuf>,
    global_settings: Settings,
    settings: Settings,
    persist: bool,
}

impl SettingsManager {
    pub fn create(cwd: impl Into<String>, agent_dir: impl Into<String>) -> Self {
        let cwd = normalize_cwd(cwd.into());
        let agent_dir = normalize_agent_dir(agent_dir.into());

        let settings_path = agent_dir.as_ref().map(|dir| dir.join("settings.json"));
        let project_settings_path = cwd
            .as_ref()
            .map(|dir| dir.join(config::config_dir_name()).join("settings.json"));
        let global_settings = settings_path
            .as_ref()
            .map(|path| load_settings_from_file(path))
            .unwrap_or_default();

        let mut manager = Self {
            settings_path,
            project_settings_path,
            global_settings,
            settings: Settings::default(),
            persist: true,
        };
        manager.refresh_settings();
        manager
    }

    pub fn in_memory(settings: Settings) -> Self {
        Self {
            settings_path: None,
            project_settings_path: None,
            global_settings: settings.clone(),
            settings,
            persist: false,
        }
    }

    pub fn apply_overrides(&mut self, overrides: SettingsOverrides) {
        let overrides = overrides.into_settings();
        self.settings = merge_settings(&self.settings, &overrides);
    }

    pub fn get_last_changelog_version(&self) -> Option<String> {
        self.settings.last_changelog_version.clone()
    }

    pub fn set_last_changelog_version(&mut self, version: &str) {
        self.global_settings.last_changelog_version = Some(version.to_string());
        self.save();
    }

    pub fn get_default_provider(&self) -> Option<String> {
        self.settings.default_provider.clone()
    }

    pub fn get_default_model(&self) -> Option<String> {
        self.settings.default_model.clone()
    }

    pub fn set_default_provider(&mut self, provider: &str) {
        self.global_settings.default_provider = Some(provider.to_string());
        self.save();
    }

    pub fn set_default_model(&mut self, model_id: &str) {
        self.global_settings.default_model = Some(model_id.to_string());
        self.save();
    }

    pub fn set_default_model_and_provider(&mut self, provider: &str, model_id: &str) {
        self.global_settings.default_provider = Some(provider.to_string());
        self.global_settings.default_model = Some(model_id.to_string());
        self.save();
    }

    pub fn get_steering_mode(&self) -> String {
        self.settings
            .steering_mode
            .clone()
            .unwrap_or_else(|| "one-at-a-time".to_string())
    }

    pub fn set_steering_mode(&mut self, mode: &str) {
        self.global_settings.steering_mode = Some(mode.to_string());
        self.save();
    }

    pub fn get_follow_up_mode(&self) -> String {
        self.settings
            .follow_up_mode
            .clone()
            .unwrap_or_else(|| "one-at-a-time".to_string())
    }

    pub fn set_follow_up_mode(&mut self, mode: &str) {
        self.global_settings.follow_up_mode = Some(mode.to_string());
        self.save();
    }

    pub fn get_theme(&self) -> Option<String> {
        self.settings.theme.clone()
    }

    pub fn set_theme(&mut self, theme: &str) {
        self.global_settings.theme = Some(theme.to_string());
        self.save();
    }

    pub fn get_default_thinking_level(&self) -> Option<String> {
        self.settings.default_thinking_level.clone()
    }

    pub fn set_default_thinking_level(&mut self, level: &str) {
        self.global_settings.default_thinking_level = Some(level.to_string());
        self.save();
    }

    pub fn get_compaction_settings(&self) -> crate::core::compaction::CompactionSettings {
        crate::core::compaction::CompactionSettings {
            enabled: self.get_compaction_enabled(),
            reserve_tokens: self.get_compaction_reserve_tokens(),
            keep_recent_tokens: self.get_compaction_keep_recent_tokens(),
        }
    }

    pub fn get_compaction_enabled(&self) -> bool {
        self.settings
            .compaction
            .as_ref()
            .and_then(|settings| settings.enabled)
            .unwrap_or(true)
    }

    pub fn set_compaction_enabled(&mut self, enabled: bool) {
        let mut compaction = self.global_settings.compaction.clone().unwrap_or_default();
        compaction.enabled = Some(enabled);
        self.global_settings.compaction = Some(compaction);
        self.save();
    }

    pub fn get_compaction_reserve_tokens(&self) -> i64 {
        self.settings
            .compaction
            .as_ref()
            .and_then(|settings| settings.reserve_tokens)
            .unwrap_or(16_384)
    }

    pub fn get_compaction_keep_recent_tokens(&self) -> i64 {
        self.settings
            .compaction
            .as_ref()
            .and_then(|settings| settings.keep_recent_tokens)
            .unwrap_or(20_000)
    }

    pub fn get_branch_summary_settings(&self) -> SettingsBranchSummary {
        SettingsBranchSummary {
            reserve_tokens: self
                .settings
                .branch_summary
                .as_ref()
                .and_then(|settings| settings.reserve_tokens)
                .or(Some(16_384)),
        }
    }

    pub fn get_retry_settings(&self) -> SettingsRetry {
        SettingsRetry {
            enabled: Some(self.get_retry_enabled()),
            max_retries: Some(
                self.settings
                    .retry
                    .as_ref()
                    .and_then(|settings| settings.max_retries)
                    .unwrap_or(3),
            ),
            base_delay_ms: Some(
                self.settings
                    .retry
                    .as_ref()
                    .and_then(|settings| settings.base_delay_ms)
                    .unwrap_or(2000),
            ),
        }
    }

    pub fn get_retry_enabled(&self) -> bool {
        self.settings
            .retry
            .as_ref()
            .and_then(|settings| settings.enabled)
            .unwrap_or(true)
    }

    pub fn set_retry_enabled(&mut self, enabled: bool) {
        let mut retry = self.global_settings.retry.clone().unwrap_or_default();
        retry.enabled = Some(enabled);
        self.global_settings.retry = Some(retry);
        self.save();
    }

    pub fn get_hide_thinking_block(&self) -> bool {
        self.settings.hide_thinking_block.unwrap_or(false)
    }

    pub fn set_hide_thinking_block(&mut self, hide: bool) {
        self.global_settings.hide_thinking_block = Some(hide);
        self.save();
    }

    pub fn get_shell_path(&self) -> Option<String> {
        self.settings.shell_path.clone()
    }

    pub fn set_shell_path(&mut self, path: Option<String>) {
        self.global_settings.shell_path = path;
        self.save();
    }

    pub fn get_collapse_changelog(&self) -> bool {
        self.settings.collapse_changelog.unwrap_or(false)
    }

    pub fn set_collapse_changelog(&mut self, collapse: bool) {
        self.global_settings.collapse_changelog = Some(collapse);
        self.save();
    }

    pub fn get_extension_paths(&self) -> Vec<String> {
        self.settings.extensions.clone().unwrap_or_default()
    }

    pub fn set_extension_paths(&mut self, paths: Vec<String>) {
        self.global_settings.extensions = Some(paths);
        self.save();
    }

    pub fn get_skills_enabled(&self) -> bool {
        self.settings
            .skills
            .as_ref()
            .and_then(|skills| skills.enabled)
            .unwrap_or(true)
    }

    pub fn set_skills_enabled(&mut self, enabled: bool) {
        let mut skills = self.global_settings.skills.clone().unwrap_or_default();
        skills.enabled = Some(enabled);
        self.global_settings.skills = Some(skills);
        self.save();
    }

    pub fn get_skills_settings(&self) -> SettingsSkills {
        let skills = self.settings.skills.clone().unwrap_or_default();
        SettingsSkills {
            enabled: Some(skills.enabled.unwrap_or(true)),
            enable_codex_user: Some(skills.enable_codex_user.unwrap_or(true)),
            enable_claude_user: Some(skills.enable_claude_user.unwrap_or(true)),
            enable_claude_project: Some(skills.enable_claude_project.unwrap_or(true)),
            enable_pi_user: Some(skills.enable_pi_user.unwrap_or(true)),
            enable_pi_project: Some(skills.enable_pi_project.unwrap_or(true)),
            custom_directories: Some(skills.custom_directories.unwrap_or_default()),
            ignored_skills: Some(skills.ignored_skills.unwrap_or_default()),
            include_skills: Some(skills.include_skills.unwrap_or_default()),
        }
    }

    pub fn get_show_images(&self) -> bool {
        self.settings
            .terminal
            .as_ref()
            .and_then(|terminal| terminal.show_images)
            .unwrap_or(true)
    }

    pub fn set_show_images(&mut self, show: bool) {
        let mut terminal = self.global_settings.terminal.clone().unwrap_or_default();
        terminal.show_images = Some(show);
        self.global_settings.terminal = Some(terminal);
        self.save();
    }

    pub fn get_image_auto_resize(&self) -> bool {
        self.settings
            .images
            .as_ref()
            .and_then(|images| images.auto_resize)
            .unwrap_or(true)
    }

    pub fn set_image_auto_resize(&mut self, enabled: bool) {
        let mut images = self.global_settings.images.clone().unwrap_or_default();
        images.auto_resize = Some(enabled);
        self.global_settings.images = Some(images);
        self.save();
    }

    pub fn get_enabled_models(&self) -> Option<Vec<String>> {
        self.settings.enabled_models.clone()
    }

    pub fn get_double_escape_action(&self) -> String {
        self.settings
            .double_escape_action
            .clone()
            .unwrap_or_else(|| "tree".to_string())
    }

    pub fn set_double_escape_action(&mut self, action: &str) {
        self.global_settings.double_escape_action = Some(action.to_string());
        self.save();
    }

    pub fn is_compaction_enabled(&self) -> bool {
        self.get_compaction_enabled()
    }

    pub fn retry_enabled(&self) -> bool {
        self.get_retry_enabled()
    }

    fn refresh_settings(&mut self) {
        let project_settings = self
            .project_settings_path
            .as_ref()
            .map(|path| load_settings_from_file(path))
            .unwrap_or_default();
        self.settings = merge_settings(&self.global_settings, &project_settings);
    }

    fn save(&mut self) {
        if !self.persist {
            return;
        }
        let Some(path) = self.settings_path.as_ref() else {
            return;
        };

        if let Some(parent) = path.parent() {
            if let Err(err) = fs::create_dir_all(parent) {
                eprintln!("Warning: Could not create settings dir: {err}");
                return;
            }
        }

        match serde_json::to_string_pretty(&self.global_settings) {
            Ok(contents) => {
                if let Err(err) = fs::write(path, contents) {
                    eprintln!("Warning: Could not save settings file: {err}");
                    return;
                }
            }
            Err(err) => {
                eprintln!("Warning: Could not serialize settings: {err}");
                return;
            }
        }

        self.refresh_settings();
    }
}

pub struct CompactionOverrides {
    pub enabled: Option<bool>,
    pub reserve_tokens: Option<i64>,
    pub keep_recent_tokens: Option<i64>,
}

pub struct SettingsOverrides {
    pub compaction: Option<CompactionOverrides>,
}

impl SettingsOverrides {
    fn into_settings(self) -> Settings {
        Settings {
            compaction: self.compaction.map(|compaction| SettingsCompaction {
                enabled: compaction.enabled,
                reserve_tokens: compaction.reserve_tokens,
                keep_recent_tokens: compaction.keep_recent_tokens,
            }),
            ..Settings::default()
        }
    }
}

fn normalize_cwd(input: String) -> Option<PathBuf> {
    if !input.trim().is_empty() {
        return Some(PathBuf::from(input));
    }
    env::current_dir().ok()
}

fn normalize_agent_dir(input: String) -> Option<PathBuf> {
    if !input.trim().is_empty() {
        return Some(PathBuf::from(input));
    }
    env::var(config::env_agent_dir_name())
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| Some(config::get_agent_dir()))
}

fn load_settings_from_file(path: &Path) -> Settings {
    if !path.exists() {
        return Settings::default();
    }
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(err) => {
            eprintln!(
                "Warning: Could not read settings file {}: {err}",
                path.display()
            );
            return Settings::default();
        }
    };

    let value: Value = match serde_json::from_str(&content) {
        Ok(value) => value,
        Err(err) => {
            eprintln!(
                "Warning: Could not parse settings file {}: {err}",
                path.display()
            );
            return Settings::default();
        }
    };

    let migrated = migrate_settings_value(value);
    serde_json::from_value(migrated).unwrap_or_else(|err| {
        eprintln!(
            "Warning: Could not decode settings file {}: {err}",
            path.display()
        );
        Settings::default()
    })
}

fn migrate_settings_value(mut value: Value) -> Value {
    if let Value::Object(ref mut map) = value {
        let has_queue = map.contains_key("queueMode");
        let has_steering = map.contains_key("steeringMode");
        if has_queue && !has_steering {
            if let Some(queue_value) = map.get("queueMode").cloned() {
                map.insert("steeringMode".to_string(), queue_value);
            }
        }
        if has_queue {
            map.remove("queueMode");
        }
    }
    value
}

#[derive(Clone, Debug, PartialEq)]
pub struct BranchCandidate {
    pub entry_id: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BranchResult {
    pub selected_text: String,
    pub cancelled: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NavigateTreeOptions {
    pub summarize: bool,
    pub custom_instructions: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NavigateTreeResult {
    pub editor_text: Option<String>,
    pub cancelled: bool,
    pub aborted: bool,
    pub summary_entry: Option<BranchSummaryEntry>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AgentSessionState {
    pub model: crate::agent::Model,
    pub thinking_level: ThinkingLevel,
    pub is_streaming: bool,
    pub message_count: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ThinkingLevelCycleResult {
    pub level: ThinkingLevel,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModelCycleResult {
    pub model: crate::coding_agent::Model,
    pub thinking_level: ThinkingLevel,
    pub is_scoped: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TokenStats {
    pub input: i64,
    pub output: i64,
    pub cache_read: i64,
    pub cache_write: i64,
    pub total: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SessionStats {
    pub session_file: Option<PathBuf>,
    pub session_id: String,
    pub user_messages: usize,
    pub assistant_messages: usize,
    pub tool_calls: usize,
    pub tool_results: usize,
    pub total_messages: usize,
    pub tokens: TokenStats,
    pub cost: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ExportResult {
    pub path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BashResult {
    pub output: String,
    pub exit_code: Option<i64>,
    pub cancelled: bool,
}

fn convert_message(message: &AgentMessage) -> Option<CoreAgentMessage> {
    match message {
        AgentMessage::User(user) => Some(CoreAgentMessage::User(user.clone())),
        AgentMessage::Assistant(assistant) => Some(CoreAgentMessage::Assistant(assistant.clone())),
        AgentMessage::ToolResult(result) => Some(CoreAgentMessage::ToolResult(result.clone())),
        AgentMessage::Custom(custom) => Some(CoreAgentMessage::HookMessage(
            crate::core::messages::HookMessage {
                custom_type: custom.role.clone(),
                content: UserContent::Text(custom.text.clone()),
                display: true,
                details: None,
                timestamp: custom.timestamp,
            },
        )),
    }
}

fn convert_core_message(message: &CoreAgentMessage) -> Option<AgentMessage> {
    match message {
        CoreAgentMessage::User(user) => Some(AgentMessage::User(user.clone())),
        CoreAgentMessage::Assistant(assistant) => Some(AgentMessage::Assistant(assistant.clone())),
        CoreAgentMessage::ToolResult(result) => Some(AgentMessage::ToolResult(result.clone())),
        CoreAgentMessage::HookMessage(hook) => Some(AgentMessage::Custom(CustomMessage {
            role: "hookMessage".to_string(),
            text: extract_user_text(&hook.content),
            timestamp: hook.timestamp,
        })),
        CoreAgentMessage::BranchSummary(summary) => Some(AgentMessage::Custom(CustomMessage {
            role: "branchSummary".to_string(),
            text: summary.summary.clone(),
            timestamp: summary.timestamp,
        })),
        CoreAgentMessage::CompactionSummary(summary) => Some(AgentMessage::Custom(CustomMessage {
            role: "compactionSummary".to_string(),
            text: summary.summary.clone(),
            timestamp: summary.timestamp,
        })),
        CoreAgentMessage::BashExecution(bash) => Some(AgentMessage::Custom(CustomMessage {
            role: "bashExecution".to_string(),
            text: bash.output.clone(),
            timestamp: bash.timestamp,
        })),
    }
}

fn extract_user_text(content: &UserContent) -> String {
    match content {
        UserContent::Text(text) => text.clone(),
        UserContent::Blocks(blocks) => blocks
            .iter()
            .filter_map(|block| match block {
                crate::core::messages::ContentBlock::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<String>>()
            .join(""),
    }
}

fn collect_entries_for_branch_summary(
    session_manager: &SessionManager,
    old_leaf_id: Option<&str>,
    target_id: &str,
) -> (Vec<SessionEntry>, Option<String>) {
    let old_path = old_leaf_id
        .map(|id| session_manager.get_branch(Some(id)))
        .unwrap_or_default();
    if old_path.is_empty() {
        return (Vec::new(), None);
    }

    let target_path = session_manager.get_branch(Some(target_id));

    let mut common_ancestor = None;
    for (old_entry, target_entry) in old_path.iter().zip(target_path.iter()) {
        if old_entry.id() == target_entry.id() {
            common_ancestor = Some(old_entry.id().to_string());
        } else {
            break;
        }
    }

    let start_index = common_ancestor
        .as_ref()
        .and_then(|id| old_path.iter().position(|entry| entry.id() == id))
        .map(|index| index + 1)
        .unwrap_or(0);

    let entries_to_summarize = old_path.into_iter().skip(start_index).collect();
    (entries_to_summarize, common_ancestor)
}

fn summarize_entries(entries: &[SessionEntry], custom_instructions: Option<&str>) -> String {
    let mut parts = Vec::new();
    for entry in entries {
        if let SessionEntry::Message(message_entry) = entry {
            if let CoreAgentMessage::User(user) = &message_entry.message {
                let text = extract_user_text(&user.content);
                if !text.is_empty() {
                    parts.push(text);
                }
            }
        }
    }

    let mut summary = if parts.is_empty() {
        "Summary.".to_string()
    } else {
        let merged = parts.join(" ");
        let clipped = clip_words(&merged, 12);
        format!("Summary: {clipped}")
    };

    if let Some(instructions) = custom_instructions {
        summary.push(' ');
        summary.push_str(&clip_words(instructions, 6));
    }

    summary
}

fn clip_words(text: &str, max_words: usize) -> String {
    let mut words = text.split_whitespace();
    let mut kept = Vec::new();
    for _ in 0..max_words {
        if let Some(word) = words.next() {
            kept.push(word);
        } else {
            break;
        }
    }
    kept.join(" ")
}

fn summarize_compaction_messages(messages: &[CoreAgentMessage]) -> String {
    let mut parts = Vec::new();
    for message in messages {
        match message {
            CoreAgentMessage::User(user) => {
                let text = extract_user_text(&user.content);
                if !text.is_empty() {
                    parts.push(text);
                }
            }
            CoreAgentMessage::Assistant(assistant) => {
                let mut text = String::new();
                for block in &assistant.content {
                    if let crate::core::messages::ContentBlock::Text { text: chunk, .. } = block {
                        text.push_str(chunk);
                    }
                }
                if !text.is_empty() {
                    parts.push(text);
                }
            }
            CoreAgentMessage::ToolResult(result) => {
                let mut text = String::new();
                for block in &result.content {
                    if let crate::core::messages::ContentBlock::Text { text: chunk, .. } = block {
                        text.push_str(chunk);
                    }
                }
                if !text.is_empty() {
                    parts.push(text);
                }
            }
            CoreAgentMessage::HookMessage(hook) => {
                let text = extract_user_text(&hook.content);
                if !text.is_empty() {
                    parts.push(text);
                }
            }
            CoreAgentMessage::BranchSummary(summary) => parts.push(summary.summary.clone()),
            CoreAgentMessage::CompactionSummary(summary) => parts.push(summary.summary.clone()),
            CoreAgentMessage::BashExecution(bash) => {
                if !bash.output.is_empty() {
                    parts.push(bash.output.clone());
                }
            }
        }
    }

    if parts.is_empty() {
        return String::new();
    }

    let merged = parts.join(" ");
    format!("Summary: {}", clip_words(&merged, 32))
}

fn wrap_tools_with_extension_host(
    tools: Vec<AgentTool>,
    host: Rc<RefCell<ExtensionHost>>,
) -> Vec<AgentTool> {
    tools
        .into_iter()
        .map(|tool| {
            let tool_name = tool.name.clone();
            let label = tool.label.clone();
            let description = tool.description.clone();
            let execute = tool.execute.clone();
            let host_ref = host.clone();

            AgentTool {
                name: tool_name.clone(),
                label,
                description,
                execute: Rc::new(move |tool_call_id, args| {
                    let call_result = match host_ref
                        .borrow_mut()
                        .emit_tool_call(&tool_name, tool_call_id, args)
                    {
                        Ok(result) => result,
                        Err(err) => {
                            eprintln!("Warning: Extension tool_call failed: {err}");
                            crate::coding_agent::extension_host::ExtensionToolCallResult::default()
                        }
                    };

                    if call_result.block.unwrap_or(false) {
                        let reason = call_result.reason.unwrap_or_else(|| {
                            "Tool execution was blocked by an extension".to_string()
                        });
                        return Err(reason);
                    }

                    match (execute)(tool_call_id, args) {
                        Ok(result) => {
                            let override_result = match host_ref.borrow_mut().emit_tool_result(
                                &tool_name,
                                tool_call_id,
                                args,
                                &result.content,
                                &result.details,
                                false,
                            ) {
                                Ok(override_result) => override_result,
                                Err(err) => {
                                    eprintln!("Warning: Extension tool_result failed: {err}");
                                    crate::coding_agent::extension_host::ExtensionToolResult::default()
                                }
                            };
                            let content = override_result.content.unwrap_or(result.content);
                            let details = override_result.details.unwrap_or(result.details);
                            Ok(AgentToolResult { content, details })
                        }
                        Err(err) => {
                            let error_content = vec![ContentBlock::Text {
                                text: err.clone(),
                                text_signature: None,
                            }];
                            let _ = host_ref.borrow_mut().emit_tool_result(
                                &tool_name,
                                tool_call_id,
                                args,
                                &error_content,
                                &Value::Null,
                                true,
                            );
                            Err(err)
                        }
                    }
                }),
            }
        })
        .collect()
}

fn now_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

fn thinking_level_from_str(level: &str) -> Option<ThinkingLevel> {
    match level {
        "off" => Some(ThinkingLevel::Off),
        "minimal" => Some(ThinkingLevel::Minimal),
        "low" => Some(ThinkingLevel::Low),
        "medium" => Some(ThinkingLevel::Medium),
        "high" => Some(ThinkingLevel::High),
        "xhigh" => Some(ThinkingLevel::XHigh),
        _ => None,
    }
}
