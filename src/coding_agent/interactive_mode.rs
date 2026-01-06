use crate::agent::AgentMessage;
use crate::core::messages::{ContentBlock, UserContent};
use crate::tui::{Container, Spacer, Text};
use serde_json::Value;

pub struct InteractiveMode {
    pub chat_container: Container,
    last_status_spacer: Option<usize>,
    last_status_text: Option<usize>,
}

impl InteractiveMode {
    pub fn new() -> Self {
        Self {
            chat_container: Container::new(),
            last_status_spacer: None,
            last_status_text: None,
        }
    }

    pub fn show_status(&mut self, message: &str) {
        let len = self.chat_container.children.len();
        if len >= 2 {
            let last_index = len - 1;
            let second_last_index = len - 2;
            if self.last_status_text == Some(last_index)
                && self.last_status_spacer == Some(second_last_index)
            {
                if let Some(child) = self.chat_container.children.get_mut(last_index) {
                    if let Some(text) = child.as_any_mut().downcast_mut::<Text>() {
                        text.set_text(message);
                    }
                }
                return;
            }
        }

        self.chat_container.add_child(Spacer::new(1));
        self.chat_container.add_child(Text::new(message));
        let len = self.chat_container.children.len();
        self.last_status_spacer = Some(len - 2);
        self.last_status_text = Some(len - 1);
    }
}

pub fn format_message_for_interactive(
    message: &AgentMessage,
    include_user: bool,
) -> Option<String> {
    match message {
        AgentMessage::User(user) => {
            if include_user {
                Some(format!("You:\n{}", format_user_content(&user.content)))
            } else {
                None
            }
        }
        AgentMessage::Assistant(assistant) => Some(format!(
            "Assistant:\n{}",
            format_content_blocks(&assistant.content)
        )),
        AgentMessage::ToolResult(result) => {
            let label = if result.is_error {
                "Tool result (error)"
            } else {
                "Tool result"
            };
            let mut entry = format!(
                "{}: {}\n{}",
                label,
                result.tool_name,
                format_content_blocks(&result.content)
            );
            if let Some(details) = result.details.as_ref().filter(|value| !value.is_null()) {
                entry.push_str("\n\nDetails:\n");
                entry.push_str(&format_json(details));
            }
            Some(entry)
        }
        AgentMessage::Custom(message) => {
            Some(format!("Custom ({})\n{}", message.role, message.text))
        }
    }
}

pub fn format_content_blocks(blocks: &[ContentBlock]) -> String {
    let mut parts = Vec::new();
    for block in blocks {
        match block {
            ContentBlock::Text { text, .. } => {
                if !text.is_empty() {
                    parts.push(text.clone());
                }
            }
            ContentBlock::Thinking { thinking, .. } => {
                if thinking.is_empty() {
                    parts.push("Thinking:\n[empty]".to_string());
                } else {
                    parts.push(format!("Thinking:\n{thinking}"));
                }
            }
            ContentBlock::ToolCall {
                name, arguments, ..
            } => {
                let mut entry = format!("Tool call: {name}");
                let formatted = format_json(arguments);
                if !formatted.is_empty() {
                    entry.push_str("\nArguments:\n");
                    entry.push_str(&formatted);
                }
                parts.push(entry);
            }
            ContentBlock::Image { mime_type, .. } => {
                parts.push(format!("Image attachment ({mime_type})"));
            }
        }
    }
    if parts.is_empty() {
        "[empty message]".to_string()
    } else {
        parts.join("\n\n")
    }
}

fn format_user_content(content: &UserContent) -> String {
    match content {
        UserContent::Text(text) => {
            if text.trim().is_empty() {
                "[empty message]".to_string()
            } else {
                text.clone()
            }
        }
        UserContent::Blocks(blocks) => format_content_blocks(blocks),
    }
}

fn format_json(value: &Value) -> String {
    if value.is_null() {
        return String::new();
    }
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

impl Default for InteractiveMode {
    fn default() -> Self {
        Self::new()
    }
}
