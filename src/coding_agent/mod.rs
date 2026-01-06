pub mod export_html;
pub mod fuzzy;
pub mod tools;

pub use fuzzy::{fuzzy_filter, fuzzy_match, FuzzyMatch};
pub mod agent_session;
pub mod auth_storage;
pub mod hooks;
pub mod interactive_mode;
pub mod model_registry;
pub mod model_resolver;
pub mod prompt_templates;
pub mod skills;
pub mod slash_commands;
pub mod system_prompt;

pub use agent_session::{
    AgentSession, AgentSessionConfig, AgentSessionError, AgentSessionEvent, AgentSessionState,
    BashResult, BranchCandidate, BranchResult, CompactionOverrides, ExportResult, ModelCycleResult,
    NavigateTreeOptions, NavigateTreeResult, SessionStats, SettingsManager, SettingsOverrides,
    ThinkingLevelCycleResult, TokenStats,
};
pub use auth_storage::{AuthCredential, AuthStorage};
pub use export_html::{export_from_file, export_session_to_html};
pub use hooks::{
    CompactionHook, CompactionResult, HookAPI, HookContext, SessionBeforeCompactEvent,
    SessionBeforeCompactResult, SessionCompactEvent,
};
pub use interactive_mode::InteractiveMode;
pub use model_registry::{Model, ModelRegistry};
pub use model_resolver::{
    parse_model_pattern, resolve_model_scope, InitialModelResult, ParsedModelResult, ScopedModel,
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
