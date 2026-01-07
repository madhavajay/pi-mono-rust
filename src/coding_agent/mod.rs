pub mod export_html;
pub mod extension_host;
pub mod extension_runner;
pub mod extensions;
pub mod fuzzy;
pub mod tools;

pub use fuzzy::{fuzzy_filter, fuzzy_match, FuzzyMatch};
pub mod agent_session;
pub mod auth_storage;
pub mod changelog;
pub mod hooks;
pub mod interactive_mode;
pub mod model_registry;
pub mod model_resolver;
pub mod oauth;
pub mod prompt_templates;
pub mod skills;
pub mod slash_commands;
pub mod system_prompt;
pub mod theme;

pub use agent_session::{
    AgentSession, AgentSessionConfig, AgentSessionError, AgentSessionEvent, AgentSessionState,
    BashResult, BranchCandidate, BranchResult, CompactionOverrides, ExportResult, ModelCycleResult,
    NavigateTreeOptions, NavigateTreeResult, SessionStats, SettingsManager, SettingsOverrides,
    ThinkingLevelCycleResult, TokenStats,
};
pub use auth_storage::{AuthCredential, AuthStorage};
pub use changelog::{get_changelog_path, parse_changelog, ChangelogEntry};
pub use export_html::{export_from_file, export_session_to_html};
pub use extension_host::{
    ExtensionCommand, ExtensionHost, ExtensionManifest, ExtensionUiRequest, ExtensionUiResponse,
};
pub use extension_runner::{
    ExtensionRunner, RegisteredCommand, RegisteredFlag, RegisteredMessageRenderer,
    RegisteredShortcut, RegisteredTool,
};
pub use extensions::discover_extension_paths;
pub use hooks::{
    CompactionHook, CompactionResult, HookAPI, HookContext, SessionBeforeCompactEvent,
    SessionBeforeCompactResult, SessionCompactEvent,
};
pub use interactive_mode::InteractiveMode;
pub use model_registry::{Model, ModelRegistry};
pub use model_resolver::{
    parse_model_pattern, resolve_model_scope, InitialModelResult, ParsedModelResult, ScopedModel,
};
pub use oauth::{
    anthropic_exchange_code, anthropic_get_auth_url, anthropic_refresh_token,
    get_github_copilot_base_url, get_oauth_providers, github_poll_for_token,
    github_refresh_copilot_token, github_start_device_flow, normalize_github_domain, open_browser,
    openai_codex_exchange_code, openai_codex_get_auth_url, openai_codex_login_with_input,
    openai_codex_refresh_token, DeviceCodeResponse, OAuthCallbackServer, OAuthCredentials,
    OAuthProviderInfo,
};
pub use prompt_templates::{
    expand_prompt_template, load_prompt_templates, LoadPromptTemplatesOptions, PromptTemplate,
};
pub use skills::{
    format_skills_for_prompt, load_skills, load_skills_from_dir, LoadSkillsFromDirOptions,
    LoadSkillsOptions, LoadSkillsResult, Skill, SkillWarning,
};
pub use slash_commands::{parse_command_args, substitute_args};
pub use system_prompt::{
    build_system_prompt, load_project_context_files, BuildSystemPromptOptions, ContextFile,
    LoadContextFilesOptions,
};
pub use theme::{
    available_themes, load_theme, load_theme_or_default, set_active_theme, Theme, ThemeBg,
    ThemeColor,
};
