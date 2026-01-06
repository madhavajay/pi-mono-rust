use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::core::messages::{AssistantMessage, ContentBlock, UserContent, UserMessage};

use super::{
    agent_loop, agent_loop_continue, AgentContext, AgentEvent, AgentLoopConfig, AgentMessage,
    AgentTool, ConvertToLlmFn, CustomMessage, ListenerFn, LlmContext, Model, StreamEvents,
    StreamFn, TransformContextFn,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThinkingLevel {
    Off,
    Minimal,
    Low,
    Medium,
    High,
    XHigh,
}

impl ThinkingLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            ThinkingLevel::Off => "off",
            ThinkingLevel::Minimal => "minimal",
            ThinkingLevel::Low => "low",
            ThinkingLevel::Medium => "medium",
            ThinkingLevel::High => "high",
            ThinkingLevel::XHigh => "xhigh",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AgentState {
    pub system_prompt: String,
    pub model: Model,
    pub thinking_level: ThinkingLevel,
    pub tools: Vec<AgentTool>,
    pub messages: Vec<AgentMessage>,
    pub is_streaming: bool,
    pub stream_message: Option<AgentMessage>,
    pub pending_tool_calls: HashSet<String>,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct AgentStateOverride {
    pub system_prompt: Option<String>,
    pub model: Option<Model>,
    pub thinking_level: Option<ThinkingLevel>,
    pub tools: Option<Vec<AgentTool>>,
    pub messages: Option<Vec<AgentMessage>>,
    pub is_streaming: Option<bool>,
    pub stream_message: Option<Option<AgentMessage>>,
    pub pending_tool_calls: Option<HashSet<String>>,
    pub error: Option<Option<String>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueMode {
    All,
    OneAtATime,
}

#[derive(Default)]
pub struct AgentOptions {
    pub initial_state: Option<AgentStateOverride>,
    pub convert_to_llm: Option<Box<ConvertToLlmFn>>,
    pub transform_context: Option<Box<TransformContextFn>>,
    pub steering_mode: Option<QueueMode>,
    pub follow_up_mode: Option<QueueMode>,
    pub stream_fn: Option<Box<StreamFn>>,
    pub abort_flag: Option<Rc<Cell<bool>>>,
}

type ListenerEntry = (usize, Box<ListenerFn>);

pub struct Agent {
    state: Rc<RefCell<AgentState>>,
    listeners: Rc<RefCell<Vec<ListenerEntry>>>,
    next_listener_id: Rc<RefCell<usize>>,
    convert_to_llm: Rc<RefCell<Box<ConvertToLlmFn>>>,
    transform_context: Option<Rc<RefCell<Box<TransformContextFn>>>>,
    steering_queue: Rc<RefCell<Vec<AgentMessage>>>,
    follow_up_queue: Rc<RefCell<Vec<AgentMessage>>>,
    steering_mode: QueueMode,
    follow_up_mode: QueueMode,
    stream_fn: Rc<RefCell<Box<StreamFn>>>,
    aborted: Rc<Cell<bool>>,
}

impl Agent {
    pub fn new(opts: AgentOptions) -> Self {
        let AgentOptions {
            initial_state,
            convert_to_llm,
            transform_context,
            steering_mode,
            follow_up_mode,
            stream_fn,
            abort_flag,
        } = opts;
        let mut state = AgentState {
            system_prompt: String::new(),
            model: default_model(),
            thinking_level: ThinkingLevel::Off,
            tools: Vec::new(),
            messages: Vec::new(),
            is_streaming: false,
            stream_message: None,
            pending_tool_calls: HashSet::new(),
            error: None,
        };

        if let Some(initial) = initial_state {
            if let Some(system_prompt) = initial.system_prompt {
                state.system_prompt = system_prompt;
            }
            if let Some(model) = initial.model {
                state.model = model;
            }
            if let Some(level) = initial.thinking_level {
                state.thinking_level = level;
            }
            if let Some(tools) = initial.tools {
                state.tools = tools;
            }
            if let Some(messages) = initial.messages {
                state.messages = messages;
            }
            if let Some(is_streaming) = initial.is_streaming {
                state.is_streaming = is_streaming;
            }
            if let Some(stream_message) = initial.stream_message {
                state.stream_message = stream_message;
            }
            if let Some(pending) = initial.pending_tool_calls {
                state.pending_tool_calls = pending;
            }
            if let Some(error) = initial.error {
                state.error = error;
            }
        }

        let convert_to_llm = convert_to_llm.unwrap_or_else(|| Box::new(default_convert_to_llm));
        let transform_context = transform_context.map(|transform| Rc::new(RefCell::new(transform)));
        let stream_fn = stream_fn.unwrap_or_else(|| Box::new(default_stream_fn));
        let aborted = abort_flag.unwrap_or_else(|| Rc::new(Cell::new(false)));

        Self {
            state: Rc::new(RefCell::new(state)),
            listeners: Rc::new(RefCell::new(Vec::new())),
            next_listener_id: Rc::new(RefCell::new(0)),
            convert_to_llm: Rc::new(RefCell::new(convert_to_llm)),
            transform_context,
            steering_queue: Rc::new(RefCell::new(Vec::new())),
            follow_up_queue: Rc::new(RefCell::new(Vec::new())),
            steering_mode: steering_mode.unwrap_or(QueueMode::OneAtATime),
            follow_up_mode: follow_up_mode.unwrap_or(QueueMode::OneAtATime),
            stream_fn: Rc::new(RefCell::new(stream_fn)),
            aborted,
        }
    }

    pub fn state(&self) -> AgentState {
        self.state.borrow().clone()
    }

    pub fn subscribe<F>(&self, listener: F) -> impl FnOnce()
    where
        F: Fn(&AgentEvent) + 'static,
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

    pub fn set_system_prompt(&self, value: &str) {
        self.state.borrow_mut().system_prompt = value.to_string();
    }

    pub fn set_model(&self, model: Model) {
        self.state.borrow_mut().model = model;
    }

    pub fn set_thinking_level(&self, level: ThinkingLevel) {
        self.state.borrow_mut().thinking_level = level;
    }

    pub fn set_steering_mode(&mut self, mode: QueueMode) {
        self.steering_mode = mode;
    }

    pub fn get_steering_mode(&self) -> QueueMode {
        self.steering_mode
    }

    pub fn set_follow_up_mode(&mut self, mode: QueueMode) {
        self.follow_up_mode = mode;
    }

    pub fn get_follow_up_mode(&self) -> QueueMode {
        self.follow_up_mode
    }

    pub fn set_tools(&self, tools: Vec<AgentTool>) {
        self.state.borrow_mut().tools = tools;
    }

    pub fn replace_messages(&self, messages: Vec<AgentMessage>) {
        self.state.borrow_mut().messages = messages;
    }

    pub fn append_message(&self, message: AgentMessage) {
        self.state.borrow_mut().messages.push(message);
    }

    pub fn clear_messages(&self) {
        self.state.borrow_mut().messages.clear();
    }

    pub fn steer(&self, message: AgentMessage) {
        self.steering_queue.borrow_mut().push(message);
    }

    pub fn follow_up(&self, message: AgentMessage) {
        self.follow_up_queue.borrow_mut().push(message);
    }

    pub fn clear_steering_queue(&self) {
        self.steering_queue.borrow_mut().clear();
    }

    pub fn clear_follow_up_queue(&self) {
        self.follow_up_queue.borrow_mut().clear();
    }

    pub fn clear_all_queues(&self) {
        self.clear_steering_queue();
        self.clear_follow_up_queue();
    }

    pub fn pending_steering_count(&self) -> usize {
        self.steering_queue.borrow().len()
    }

    pub fn pending_follow_up_count(&self) -> usize {
        self.follow_up_queue.borrow().len()
    }

    pub fn abort(&self) {
        self.aborted.set(true);
        let mut state = self.state.borrow_mut();
        state.is_streaming = false;
        state.stream_message = None;
        state.pending_tool_calls.clear();
    }

    pub fn prompt<T: Into<PromptInput>>(&self, input: T) -> Result<(), AgentError> {
        {
            let mut state = self.state.borrow_mut();
            if state.is_streaming {
                return Err(AgentError::AlreadyStreaming);
            }
            self.aborted.set(false);
            state.is_streaming = true;
            state.stream_message = None;
            state.error = None;
        }

        let messages = build_prompt_messages(input.into());
        let state_snapshot = self.state.borrow().clone();

        let context = AgentContext {
            system_prompt: state_snapshot.system_prompt.clone(),
            messages: state_snapshot.messages.clone(),
            tools: state_snapshot.tools.clone(),
        };

        let config = self.build_loop_config();
        let stream_fn = self.stream_fn.clone();

        let stream = agent_loop(messages, context, config, &mut *stream_fn.borrow_mut());

        apply_events(&self.state, &self.listeners, stream.events());

        let was_aborted = self.aborted.get();
        let keep_streaming = if was_aborted {
            false
        } else {
            should_keep_streaming(&self.state.borrow().messages)
        };

        let mut state = self.state.borrow_mut();
        state.is_streaming = keep_streaming;
        state.stream_message = None;
        if was_aborted {
            let error_message = "Request was aborted".to_string();
            let aborted_message = aborted_assistant_message(&state.model, &error_message);
            state
                .messages
                .push(AgentMessage::Assistant(aborted_message));
            state.error = Some(error_message);
        }

        Ok(())
    }

    pub fn continue_prompt(&self) -> Result<(), AgentError> {
        {
            let state = self.state.borrow();
            if state.is_streaming {
                return Err(AgentError::AlreadyStreamingContinue);
            }
            if state.messages.is_empty() {
                return Err(AgentError::NoMessages);
            }
            if matches!(state.messages.last(), Some(AgentMessage::Assistant(_))) {
                return Err(AgentError::LastMessageAssistant);
            }
        }
        self.aborted.set(false);

        let state_snapshot = self.state.borrow().clone();
        let context = AgentContext {
            system_prompt: state_snapshot.system_prompt.clone(),
            messages: state_snapshot.messages.clone(),
            tools: state_snapshot.tools.clone(),
        };

        let config = self.build_loop_config();
        let stream_fn = self.stream_fn.clone();

        let stream = agent_loop_continue(context, config, &mut *stream_fn.borrow_mut())
            .map_err(|err| AgentError::Loop(err.to_string()))?;

        apply_events(&self.state, &self.listeners, stream.events());

        let was_aborted = self.aborted.get();
        let keep_streaming = if was_aborted {
            false
        } else {
            should_keep_streaming(&self.state.borrow().messages)
        };
        let mut state = self.state.borrow_mut();
        state.is_streaming = keep_streaming;
        state.stream_message = None;
        if was_aborted {
            let error_message = "Request was aborted".to_string();
            let aborted_message = aborted_assistant_message(&state.model, &error_message);
            state
                .messages
                .push(AgentMessage::Assistant(aborted_message));
            state.error = Some(error_message);
        }

        Ok(())
    }

    fn build_loop_config(&self) -> AgentLoopConfig {
        let convert_to_llm = self.convert_to_llm.clone();
        let transform_context = self.transform_context.clone();
        let steering_queue = self.steering_queue.clone();
        let follow_up_queue = self.follow_up_queue.clone();
        let steering_mode = self.steering_mode;
        let follow_up_mode = self.follow_up_mode;
        let model = self.state.borrow().model.clone();

        let convert =
            Box::new(move |messages: &[AgentMessage]| (convert_to_llm.borrow_mut())(messages));

        let transform = transform_context.map(|transform| {
            Box::new(move |messages: &[AgentMessage]| (transform.borrow_mut())(messages))
                as Box<TransformContextFn>
        });

        let steering = Box::new(move || match steering_mode {
            QueueMode::OneAtATime => {
                let mut queue = steering_queue.borrow_mut();
                if queue.is_empty() {
                    Vec::new()
                } else {
                    vec![queue.remove(0)]
                }
            }
            QueueMode::All => {
                let mut queue = steering_queue.borrow_mut();
                let items = queue.clone();
                queue.clear();
                items
            }
        });

        let follow_up = Box::new(move || match follow_up_mode {
            QueueMode::OneAtATime => {
                let mut queue = follow_up_queue.borrow_mut();
                if queue.is_empty() {
                    Vec::new()
                } else {
                    vec![queue.remove(0)]
                }
            }
            QueueMode::All => {
                let mut queue = follow_up_queue.borrow_mut();
                let items = queue.clone();
                queue.clear();
                items
            }
        });

        AgentLoopConfig {
            model,
            convert_to_llm: convert,
            transform_context: transform,
            get_steering_messages: Some(steering),
            get_follow_up_messages: Some(follow_up),
        }
    }
}

#[derive(Debug)]
pub enum AgentError {
    AlreadyStreaming,
    AlreadyStreamingContinue,
    NoMessages,
    LastMessageAssistant,
    Loop(String),
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentError::AlreadyStreaming => write!(
                f,
                "Agent is already processing a prompt. Use steer() or followUp() to queue messages, or wait for completion."
            ),
            AgentError::AlreadyStreamingContinue => write!(
                f,
                "Agent is already processing. Wait for completion before continuing."
            ),
            AgentError::NoMessages => write!(f, "No messages to continue from"),
            AgentError::LastMessageAssistant => {
                write!(f, "Cannot continue from message role: assistant")
            }
            AgentError::Loop(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for AgentError {}

pub enum PromptInput {
    Text(String),
    Message(Box<AgentMessage>),
    Messages(Vec<AgentMessage>),
}

impl From<&str> for PromptInput {
    fn from(value: &str) -> Self {
        PromptInput::Text(value.to_string())
    }
}

impl From<String> for PromptInput {
    fn from(value: String) -> Self {
        PromptInput::Text(value)
    }
}

impl From<AgentMessage> for PromptInput {
    fn from(value: AgentMessage) -> Self {
        PromptInput::Message(Box::new(value))
    }
}

impl From<Vec<AgentMessage>> for PromptInput {
    fn from(value: Vec<AgentMessage>) -> Self {
        PromptInput::Messages(value)
    }
}

fn build_prompt_messages(input: PromptInput) -> Vec<AgentMessage> {
    match input {
        PromptInput::Text(text) => vec![AgentMessage::User(UserMessage {
            content: UserContent::Text(text),
            timestamp: now_millis(),
        })],
        PromptInput::Message(message) => vec![*message],
        PromptInput::Messages(messages) => messages,
    }
}

fn apply_events(
    state: &Rc<RefCell<AgentState>>,
    listeners: &Rc<RefCell<Vec<ListenerEntry>>>,
    events: &[AgentEvent],
) {
    for event in events {
        {
            let mut state = state.borrow_mut();
            match event {
                AgentEvent::MessageStart { message } | AgentEvent::MessageUpdate { message } => {
                    state.stream_message = Some(message.clone());
                }
                AgentEvent::MessageEnd { message } => {
                    state.stream_message = None;
                    state.messages.push(message.clone());
                }
                AgentEvent::ToolExecutionStart { tool_call_id, .. } => {
                    state.pending_tool_calls.insert(tool_call_id.clone());
                }
                AgentEvent::ToolExecutionEnd { tool_call_id, .. } => {
                    state.pending_tool_calls.remove(tool_call_id);
                }
                AgentEvent::TurnEnd {
                    message: AgentMessage::Assistant(assistant),
                    ..
                } => {
                    if assistant.error_message.is_some() {
                        state.error = assistant.error_message.clone();
                    }
                }
                AgentEvent::AgentEnd { .. } => {
                    state.is_streaming = false;
                    state.stream_message = None;
                }
                _ => {}
            }
        }

        for (_, listener) in listeners.borrow().iter() {
            listener(event);
        }
    }
}

fn should_keep_streaming(messages: &[AgentMessage]) -> bool {
    messages.iter().rev().find_map(|message| match message {
        AgentMessage::Assistant(assistant) => Some(assistant.stop_reason.as_str()),
        _ => None,
    }) == Some("streaming")
}

fn default_model() -> Model {
    Model {
        id: "gemini-2.5-flash-lite-preview-06-17".to_string(),
        name: "gemini-2.5-flash-lite-preview-06-17".to_string(),
        api: "google".to_string(),
        provider: "google".to_string(),
    }
}

fn default_stream_fn(
    _model: &Model,
    _context: &LlmContext,
    _events: &mut StreamEvents,
) -> AssistantMessage {
    AssistantMessage {
        content: vec![ContentBlock::Text {
            text: String::new(),
            text_signature: None,
        }],
        api: "openai-responses".to_string(),
        provider: "openai".to_string(),
        model: "mock".to_string(),
        usage: default_usage(),
        stop_reason: "stop".to_string(),
        error_message: None,
        timestamp: now_millis(),
    }
}

fn aborted_assistant_message(model: &Model, error_message: &str) -> AssistantMessage {
    AssistantMessage {
        content: vec![ContentBlock::Text {
            text: String::new(),
            text_signature: None,
        }],
        api: model.api.clone(),
        provider: model.provider.clone(),
        model: model.id.clone(),
        usage: default_usage(),
        stop_reason: "aborted".to_string(),
        error_message: Some(error_message.to_string()),
        timestamp: now_millis(),
    }
}

fn default_usage() -> crate::core::messages::Usage {
    crate::core::messages::Usage {
        input: 0,
        output: 0,
        cache_read: 0,
        cache_write: 0,
        total_tokens: Some(0),
        cost: Some(crate::core::messages::Cost {
            input: 0.0,
            output: 0.0,
            cache_read: 0.0,
            cache_write: 0.0,
            total: 0.0,
        }),
    }
}

fn default_convert_to_llm(messages: &[AgentMessage]) -> Vec<AgentMessage> {
    messages
        .iter()
        .filter(|&message| {
            matches!(
                message,
                AgentMessage::User(_) | AgentMessage::Assistant(_) | AgentMessage::ToolResult(_)
            )
        })
        .cloned()
        .collect()
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

pub fn get_model(provider: &str, id: &str) -> Model {
    Model {
        id: id.to_string(),
        name: id.to_string(),
        api: provider.to_string(),
        provider: provider.to_string(),
    }
}

pub fn custom_message(role: &str, text: &str) -> AgentMessage {
    AgentMessage::Custom(CustomMessage {
        role: role.to_string(),
        text: text.to_string(),
        timestamp: now_millis(),
    })
}
