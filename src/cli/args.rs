#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mode {
    Text,
    Json,
    Rpc,
}

impl Mode {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "text" => Some(Self::Text),
            "json" => Some(Self::Json),
            "rpc" => Some(Self::Rpc),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ThinkingLevel {
    Off,
    Minimal,
    Low,
    Medium,
    High,
    XHigh,
}

impl ThinkingLevel {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "off" => Some(Self::Off),
            "minimal" => Some(Self::Minimal),
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            "xhigh" => Some(Self::XHigh),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ListModels {
    All,
    Pattern(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Args {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub system_prompt: Option<String>,
    pub append_system_prompt: Option<String>,
    pub thinking: Option<ThinkingLevel>,
    pub continue_session: bool,
    pub resume: bool,
    pub help: bool,
    pub version: bool,
    pub mode: Option<Mode>,
    pub no_session: bool,
    pub session: Option<String>,
    pub session_dir: Option<String>,
    pub models: Option<Vec<String>>,
    pub tools: Option<Vec<String>>,
    pub extensions: Option<Vec<String>>,
    pub print: bool,
    pub export: Option<String>,
    pub no_skills: bool,
    pub skills: Option<Vec<String>>,
    pub list_models: Option<ListModels>,
    pub messages: Vec<String>,
    pub file_args: Vec<String>,
}

const VALID_TOOLS: [&str; 7] = ["read", "bash", "edit", "write", "grep", "find", "ls"];

pub fn is_valid_thinking_level(level: &str) -> bool {
    ThinkingLevel::parse(level).is_some()
}

pub fn parse_args(args: &[String]) -> Args {
    let mut result = Args {
        provider: None,
        model: None,
        api_key: None,
        system_prompt: None,
        append_system_prompt: None,
        thinking: None,
        continue_session: false,
        resume: false,
        help: false,
        version: false,
        mode: None,
        no_session: false,
        session: None,
        session_dir: None,
        models: None,
        tools: None,
        extensions: None,
        print: false,
        export: None,
        no_skills: false,
        skills: None,
        list_models: None,
        messages: Vec::new(),
        file_args: Vec::new(),
    };

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();

        match arg {
            "--help" | "-h" => {
                result.help = true;
            }
            "--version" | "-v" => {
                result.version = true;
            }
            "--mode" if i + 1 < args.len() => {
                if let Some(mode) = Mode::parse(&args[i + 1]) {
                    result.mode = Some(mode);
                }
                i += 1;
            }
            "--continue" | "-c" => {
                result.continue_session = true;
            }
            "--resume" | "-r" => {
                result.resume = true;
            }
            "--provider" if i + 1 < args.len() => {
                result.provider = Some(args[i + 1].clone());
                i += 1;
            }
            "--model" if i + 1 < args.len() => {
                result.model = Some(args[i + 1].clone());
                i += 1;
            }
            "--api-key" if i + 1 < args.len() => {
                result.api_key = Some(args[i + 1].clone());
                i += 1;
            }
            "--system-prompt" if i + 1 < args.len() => {
                result.system_prompt = Some(args[i + 1].clone());
                i += 1;
            }
            "--append-system-prompt" if i + 1 < args.len() => {
                result.append_system_prompt = Some(args[i + 1].clone());
                i += 1;
            }
            "--no-session" => {
                result.no_session = true;
            }
            "--session" if i + 1 < args.len() => {
                result.session = Some(args[i + 1].clone());
                i += 1;
            }
            "--session-dir" if i + 1 < args.len() => {
                result.session_dir = Some(args[i + 1].clone());
                i += 1;
            }
            "--models" if i + 1 < args.len() => {
                let models = args[i + 1]
                    .split(',')
                    .map(|value| value.trim().to_string())
                    .collect::<Vec<_>>();
                result.models = Some(models);
                i += 1;
            }
            "--tools" if i + 1 < args.len() => {
                let tool_names = args[i + 1]
                    .split(',')
                    .map(|value| value.trim())
                    .collect::<Vec<_>>();
                let mut valid = Vec::new();
                for name in tool_names {
                    if VALID_TOOLS.contains(&name) {
                        valid.push(name.to_string());
                    } else {
                        eprintln!(
                            "Warning: Unknown tool \"{name}\". Valid tools: {}",
                            VALID_TOOLS.join(", ")
                        );
                    }
                }
                result.tools = Some(valid);
                i += 1;
            }
            "--thinking" if i + 1 < args.len() => {
                let level = &args[i + 1];
                if let Some(parsed) = ThinkingLevel::parse(level) {
                    result.thinking = Some(parsed);
                } else {
                    eprintln!(
						"Warning: Invalid thinking level \"{level}\". Valid values: off, minimal, low, medium, high, xhigh"
					);
                }
                i += 1;
            }
            "--print" | "-p" => {
                result.print = true;
            }
            "--export" if i + 1 < args.len() => {
                result.export = Some(args[i + 1].clone());
                i += 1;
            }
            "--extension" | "-e" if i + 1 < args.len() => {
                result
                    .extensions
                    .get_or_insert_with(Vec::new)
                    .push(args[i + 1].clone());
                i += 1;
            }
            "--no-skills" => {
                result.no_skills = true;
            }
            "--skills" if i + 1 < args.len() => {
                let skills = args[i + 1]
                    .split(',')
                    .map(|value| value.trim().to_string())
                    .collect::<Vec<_>>();
                result.skills = Some(skills);
                i += 1;
            }
            "--list-models" => {
                if i + 1 < args.len()
                    && !args[i + 1].starts_with('-')
                    && !args[i + 1].starts_with('@')
                {
                    result.list_models = Some(ListModels::Pattern(args[i + 1].clone()));
                    i += 1;
                } else {
                    result.list_models = Some(ListModels::All);
                }
            }
            _ if arg.starts_with('@') => {
                result
                    .file_args
                    .push(arg.trim_start_matches('@').to_string());
            }
            _ if !arg.starts_with('-') => {
                result.messages.push(arg.to_string());
            }
            _ => {}
        }

        i += 1;
    }

    result
}
