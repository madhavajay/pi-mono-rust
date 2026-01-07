//! Constants for OpenAI Codex (ChatGPT OAuth) backend

/// Base URL for the Codex API
pub const CODEX_BASE_URL: &str = "https://chatgpt.com/backend-api";

/// OpenAI-specific header names (must be lowercase for http crate)
pub mod headers {
    pub const BETA: &str = "openai-beta";
    pub const ACCOUNT_ID: &str = "chatgpt-account-id";
    pub const ORIGINATOR: &str = "originator";
    pub const SESSION_ID: &str = "session_id";
    pub const CONVERSATION_ID: &str = "conversation_id";
}

/// OpenAI-specific header values
pub mod header_values {
    pub const BETA_RESPONSES: &str = "responses=experimental";
    pub const ORIGINATOR_CODEX: &str = "codex_cli_rs";
}

/// URL paths for the API
pub mod url_paths {
    pub const RESPONSES: &str = "/responses";
    pub const CODEX_RESPONSES: &str = "/codex/responses";
}

/// JWT claim path for account ID extraction
pub const JWT_CLAIM_PATH: &str = "https://api.openai.com/auth";
