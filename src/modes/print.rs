use crate::agent::AgentMessage;
use crate::cli::event_json::serialize_session_event;
use crate::cli::file_inputs::FileInputImage;
use crate::coding_agent::AgentSession;
use crate::core::messages::ContentBlock;
use crate::Mode;
use serde_json::Value;
use std::io::{self, Write};

use super::build_user_content_from_files;

pub fn run_print_mode_session(
    mode: Mode,
    session: &mut AgentSession,
    messages: &[String],
    initial_message: Option<String>,
    initial_images: &[FileInputImage],
) -> Result<(), String> {
    if matches!(mode, Mode::Json) {
        let _ = session.subscribe(|event| {
            if let Some(value) = serialize_session_event(event) {
                emit_json(&value);
            }
        });
    }

    let mut sent_any = false;
    if initial_message.is_some() || !initial_images.is_empty() {
        let content = build_user_content_from_files(initial_message.as_deref(), initial_images)?;
        session
            .prompt_content(content)
            .map_err(|err| err.to_string())?;
        sent_any = true;
    }

    for message in messages {
        if message.trim().is_empty() {
            continue;
        }
        session.prompt(message).map_err(|err| err.to_string())?;
        sent_any = true;
    }

    if !sent_any {
        return Err("No messages provided.".to_string());
    }

    if matches!(mode, Mode::Text) {
        print_last_assistant_text(session)?;
    }

    Ok(())
}

fn print_last_assistant_text(session: &AgentSession) -> Result<(), String> {
    let messages = session.messages();
    let assistant = messages.iter().rev().find_map(|message| {
        if let AgentMessage::Assistant(assistant) = message {
            Some(assistant)
        } else {
            None
        }
    });

    let assistant = assistant.ok_or_else(|| "No assistant response.".to_string())?;
    if assistant.stop_reason == "error" || assistant.stop_reason == "aborted" {
        return Err(assistant
            .error_message
            .clone()
            .unwrap_or_else(|| format!("Request {}", assistant.stop_reason)));
    }
    for block in &assistant.content {
        if let ContentBlock::Text { text, .. } = block {
            println!("{text}");
        }
    }
    Ok(())
}

fn emit_json(value: &Value) {
    let output = serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string());
    println!("{output}");
    let _ = io::stdout().flush();
}
