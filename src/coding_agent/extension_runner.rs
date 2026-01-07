use crate::coding_agent::extension_host::{ExtensionHost, ExtensionHostError, ExtensionManifest};
use crate::core::messages::AgentMessage;
use serde_json::Value;
use std::collections::HashMap;

type ErrorListener = Box<dyn Fn(&ExtensionHostError)>;

#[derive(Clone, Debug, PartialEq)]
pub struct RegisteredTool {
    pub name: String,
    pub label: Option<String>,
    pub description: Option<String>,
    pub parameters: Option<Value>,
    pub extension_path: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RegisteredCommand {
    pub name: String,
    pub description: Option<String>,
    pub extension_path: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RegisteredShortcut {
    pub shortcut: String,
    pub description: Option<String>,
    pub extension_path: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RegisteredMessageRenderer {
    pub custom_type: String,
    pub extension_path: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RegisteredFlag {
    pub name: String,
    pub description: Option<String>,
    pub flag_type: Option<String>,
    pub default: Option<Value>,
    pub extension_path: String,
}

pub struct ExtensionRunner {
    host: ExtensionHost,
    manifest: ExtensionManifest,
    error_listeners: Vec<ErrorListener>,
    warnings: Vec<String>,
}

impl ExtensionRunner {
    pub fn new(host: ExtensionHost, manifest: ExtensionManifest) -> Self {
        Self {
            host,
            manifest,
            error_listeners: Vec::new(),
            warnings: Vec::new(),
        }
    }

    pub fn get_all_registered_tools(&self) -> Vec<RegisteredTool> {
        let mut tools = Vec::new();
        for extension in &self.manifest.extensions {
            tools.extend(extension.tools.iter().map(|tool| RegisteredTool {
                name: tool.name.clone(),
                label: tool.label.clone(),
                description: tool.description.clone(),
                parameters: tool.parameters.clone(),
                extension_path: extension.path.clone(),
            }));
        }
        tools
    }

    pub fn get_registered_commands(&self) -> Vec<RegisteredCommand> {
        let mut commands = Vec::new();
        for extension in &self.manifest.extensions {
            commands.extend(extension.commands.iter().map(|command| RegisteredCommand {
                name: command.name.clone(),
                description: command.description.clone(),
                extension_path: extension.path.clone(),
            }));
        }
        commands
    }

    pub fn get_command(&self, name: &str) -> Option<RegisteredCommand> {
        self.manifest.extensions.iter().find_map(|extension| {
            extension.commands.iter().find_map(|command| {
                if command.name == name {
                    Some(RegisteredCommand {
                        name: command.name.clone(),
                        description: command.description.clone(),
                        extension_path: extension.path.clone(),
                    })
                } else {
                    None
                }
            })
        })
    }

    pub fn get_message_renderer(&self, custom_type: &str) -> Option<RegisteredMessageRenderer> {
        self.manifest.extensions.iter().find_map(|extension| {
            extension.message_renderers.iter().find_map(|renderer| {
                if renderer.custom_type == custom_type {
                    Some(RegisteredMessageRenderer {
                        custom_type: renderer.custom_type.clone(),
                        extension_path: extension.path.clone(),
                    })
                } else {
                    None
                }
            })
        })
    }

    pub fn get_flags(&self) -> HashMap<String, RegisteredFlag> {
        let mut flags = HashMap::new();
        for extension in &self.manifest.extensions {
            for flag in &extension.flags {
                flags.insert(
                    flag.name.clone(),
                    RegisteredFlag {
                        name: flag.name.clone(),
                        description: flag.description.clone(),
                        flag_type: flag.flag_type.clone(),
                        default: flag.default.clone(),
                        extension_path: extension.path.clone(),
                    },
                );
            }
        }
        flags
    }

    pub fn set_flag_value(&mut self, name: &str, value: Value) -> Result<(), String> {
        if !self
            .manifest
            .extensions
            .iter()
            .any(|extension| extension.flags.iter().any(|flag| flag.name == name))
        {
            return Ok(());
        }
        let mut flags = HashMap::new();
        flags.insert(name.to_string(), value);
        self.host.set_flag_values(&flags)
    }

    pub fn get_shortcuts(&mut self) -> HashMap<String, RegisteredShortcut> {
        let mut shortcuts: HashMap<String, RegisteredShortcut> = HashMap::new();
        let mut warnings = Vec::new();
        for extension in &self.manifest.extensions {
            for shortcut in &extension.shortcuts {
                let normalized = shortcut.shortcut.to_lowercase();
                if Self::is_reserved_shortcut(&normalized) {
                    warnings.push(format!(
                        "Extension shortcut '{}' from {} conflicts with built-in shortcut. Skipping.",
                        shortcut.shortcut, extension.path
                    ));
                    continue;
                }
                if let Some(existing) = shortcuts.get(&normalized) {
                    warnings.push(format!(
                        "Extension shortcut conflict: '{}' registered by both {} and {}. Using {}.",
                        shortcut.shortcut, existing.extension_path, extension.path, extension.path
                    ));
                }
                shortcuts.insert(
                    normalized,
                    RegisteredShortcut {
                        shortcut: shortcut.shortcut.clone(),
                        description: shortcut.description.clone(),
                        extension_path: extension.path.clone(),
                    },
                );
            }
        }
        for warning in warnings {
            self.warn(warning);
        }
        shortcuts
    }

    pub fn on_error<F>(&mut self, listener: F)
    where
        F: Fn(&ExtensionHostError) + 'static,
    {
        self.error_listeners.push(Box::new(listener));
    }

    pub fn emit_context(&mut self, messages: &[AgentMessage]) -> Result<(), String> {
        let errors = self.host.emit_context(messages)?;
        for error in &errors {
            for listener in &self.error_listeners {
                listener(error);
            }
        }
        Ok(())
    }

    pub fn has_handlers(&self, event: &str) -> bool {
        self.manifest
            .extensions
            .iter()
            .any(|extension| extension.handler_counts.get(event).copied().unwrap_or(0) > 0)
    }

    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    fn warn(&mut self, message: String) {
        eprintln!("{message}");
        self.warnings.push(message);
    }

    fn is_reserved_shortcut(shortcut: &str) -> bool {
        matches!(
            shortcut,
            "ctrl+c"
                | "ctrl+d"
                | "ctrl+z"
                | "ctrl+k"
                | "ctrl+p"
                | "ctrl+l"
                | "ctrl+o"
                | "ctrl+t"
                | "ctrl+g"
                | "shift+tab"
                | "shift+ctrl+p"
                | "alt+enter"
                | "escape"
                | "enter"
        )
    }
}
