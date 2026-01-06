use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum UserContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ContentBlock {
    Text {
        text: String,
        #[serde(default)]
        text_signature: Option<String>,
    },
    Thinking {
        thinking: String,
        #[serde(default)]
        thinking_signature: Option<String>,
    },
    ToolCall {
        id: String,
        name: String,
        #[serde(default)]
        arguments: Value,
        #[serde(default)]
        thought_signature: Option<String>,
    },
    Image {
        data: String,
        mime_type: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cost {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
    pub total: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub input: i64,
    pub output: i64,
    pub cache_read: i64,
    pub cache_write: i64,
    #[serde(default)]
    pub total_tokens: Option<i64>,
    #[serde(default)]
    pub cost: Option<Cost>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserMessage {
    pub content: UserContent,
    pub timestamp: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantMessage {
    pub content: Vec<ContentBlock>,
    pub api: String,
    pub provider: String,
    pub model: String,
    pub usage: Usage,
    pub stop_reason: String,
    #[serde(default)]
    pub error_message: Option<String>,
    pub timestamp: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResultMessage {
    pub tool_call_id: String,
    pub tool_name: String,
    pub content: Vec<ContentBlock>,
    #[serde(default)]
    pub details: Option<Value>,
    pub is_error: bool,
    pub timestamp: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BashExecutionMessage {
    pub command: String,
    pub output: String,
    pub exit_code: Option<i64>,
    pub cancelled: bool,
    pub truncated: bool,
    pub full_output_path: Option<String>,
    pub timestamp: i64,
    #[serde(default)]
    pub exclude_from_context: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookMessage {
    pub custom_type: String,
    pub content: UserContent,
    pub display: bool,
    #[serde(default)]
    pub details: Option<Value>,
    pub timestamp: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchSummaryMessage {
    pub summary: String,
    pub from_id: String,
    pub timestamp: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionSummaryMessage {
    pub summary: String,
    pub tokens_before: i64,
    pub timestamp: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "camelCase")]
pub enum AgentMessage {
    User(UserMessage),
    Assistant(AssistantMessage),
    ToolResult(ToolResultMessage),
    BashExecution(BashExecutionMessage),
    HookMessage(HookMessage),
    BranchSummary(BranchSummaryMessage),
    CompactionSummary(CompactionSummaryMessage),
}

pub fn parse_timestamp_millis(timestamp: &str) -> i64 {
    DateTime::parse_from_rfc3339(timestamp)
        .map(|dt| dt.with_timezone(&Utc).timestamp_millis())
        .unwrap_or(0)
}

pub fn create_branch_summary_message(
    summary: &str,
    from_id: &str,
    timestamp: &str,
) -> AgentMessage {
    AgentMessage::BranchSummary(BranchSummaryMessage {
        summary: summary.to_string(),
        from_id: from_id.to_string(),
        timestamp: parse_timestamp_millis(timestamp),
    })
}

pub fn create_compaction_summary_message(
    summary: &str,
    tokens_before: i64,
    timestamp: &str,
) -> AgentMessage {
    AgentMessage::CompactionSummary(CompactionSummaryMessage {
        summary: summary.to_string(),
        tokens_before,
        timestamp: parse_timestamp_millis(timestamp),
    })
}

pub fn create_hook_message(
    custom_type: &str,
    content: UserContent,
    display: bool,
    details: Option<Value>,
    timestamp: &str,
) -> AgentMessage {
    AgentMessage::HookMessage(HookMessage {
        custom_type: custom_type.to_string(),
        content,
        display,
        details,
        timestamp: parse_timestamp_millis(timestamp),
    })
}
