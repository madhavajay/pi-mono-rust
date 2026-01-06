use crate::coding_agent::tools as agent_tools;
use crate::core::messages::ContentBlock;
use serde_json::{json, Value};
use std::path::PathBuf;

pub struct ToolContext {
    pub cwd: PathBuf,
}

pub struct ToolDefinition {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
    pub execute: fn(&Value, &ToolContext) -> Result<String, String>,
}

pub fn default_tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "read",
            description: "Read the contents of a file.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the file to read (relative or absolute)" },
                    "offset": { "type": "integer", "description": "Line number to start reading from (1-indexed)" },
                    "limit": { "type": "integer", "description": "Maximum number of lines to read" }
                },
                "required": ["path"],
                "additionalProperties": false
            }),
            execute: read_tool,
        },
        ToolDefinition {
            name: "write",
            description: "Write content to a file, creating it if needed.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the file to write (relative or absolute)" },
                    "content": { "type": "string", "description": "File contents" }
                },
                "required": ["path", "content"],
                "additionalProperties": false
            }),
            execute: write_tool,
        },
        ToolDefinition {
            name: "edit",
            description: "Replace exact text in a file.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the file to edit (relative or absolute)" },
                    "oldText": { "type": "string", "description": "Exact text to find and replace" },
                    "newText": { "type": "string", "description": "New text to replace the old text with" }
                },
                "required": ["path", "oldText", "newText"],
                "additionalProperties": false
            }),
            execute: edit_tool,
        },
        ToolDefinition {
            name: "bash",
            description: "Execute a bash command in the current working directory.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "Bash command to execute" },
                    "timeout": { "type": "integer", "description": "Timeout in seconds (optional)" }
                },
                "required": ["command"],
                "additionalProperties": false
            }),
            execute: bash_tool,
        },
        ToolDefinition {
            name: "grep",
            description: "Search file contents for a pattern.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Search pattern (regex or literal string)" },
                    "path": { "type": "string", "description": "Directory or file to search (default: current directory)" },
                    "glob": { "type": "string", "description": "Filter files by glob pattern, e.g. '*.ts'" },
                    "ignoreCase": { "type": "boolean", "description": "Case-insensitive search (default: false)" },
                    "literal": { "type": "boolean", "description": "Treat pattern as literal string instead of regex (default: false)" },
                    "context": { "type": "integer", "description": "Number of lines to show before and after each match (default: 0)" },
                    "limit": { "type": "integer", "description": "Maximum number of matches to return (default: 100)" }
                },
                "required": ["pattern"],
                "additionalProperties": false
            }),
            execute: grep_tool,
        },
        ToolDefinition {
            name: "find",
            description: "Search for files by glob pattern.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Glob pattern to match files, e.g. '*.ts' or '**/*.json'" },
                    "path": { "type": "string", "description": "Directory to search in (default: current directory)" },
                    "limit": { "type": "integer", "description": "Maximum number of results (default: 1000)" }
                },
                "required": ["pattern"],
                "additionalProperties": false
            }),
            execute: find_tool,
        },
        ToolDefinition {
            name: "ls",
            description: "List directory contents.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Directory to list (default: current directory)" },
                    "limit": { "type": "integer", "description": "Maximum number of entries to return (default: 500)" }
                },
                "additionalProperties": false
            }),
            execute: ls_tool,
        },
    ]
}

fn read_tool(args: &Value, ctx: &ToolContext) -> Result<String, String> {
    let path = get_string_arg(args, "path")?;
    let offset = get_optional_usize_arg(args, "offset");
    let limit = get_optional_usize_arg(args, "limit");
    let tool = agent_tools::ReadTool::new(&ctx.cwd);
    let result = tool.execute(
        "tool-call",
        agent_tools::ReadToolArgs {
            path,
            offset,
            limit,
        },
    )?;
    Ok(tool_result_to_text(result))
}

fn write_tool(args: &Value, ctx: &ToolContext) -> Result<String, String> {
    let path = get_string_arg(args, "path")?;
    let content = get_string_arg(args, "content")?;
    let tool = agent_tools::WriteTool::new(&ctx.cwd);
    let result = tool.execute("tool-call", agent_tools::WriteToolArgs { path, content })?;
    Ok(tool_result_to_text(result))
}

fn edit_tool(args: &Value, ctx: &ToolContext) -> Result<String, String> {
    let path = get_string_arg(args, "path")?;
    let old_text = get_string_arg(args, "oldText")?;
    let new_text = get_string_arg(args, "newText")?;
    let tool = agent_tools::EditTool::new(&ctx.cwd);
    let result = tool.execute(
        "tool-call",
        agent_tools::EditToolArgs {
            path,
            old_text,
            new_text,
        },
    )?;
    Ok(tool_result_to_text(result))
}

fn bash_tool(args: &Value, ctx: &ToolContext) -> Result<String, String> {
    let command = get_string_arg(args, "command")?;
    let timeout = get_optional_u64_arg(args, "timeout");
    let tool = agent_tools::BashTool::new(&ctx.cwd);
    let result = tool.execute("tool-call", agent_tools::BashToolArgs { command, timeout })?;
    Ok(tool_result_to_text(result))
}

fn grep_tool(args: &Value, ctx: &ToolContext) -> Result<String, String> {
    let pattern = get_string_arg(args, "pattern")?;
    let tool = agent_tools::GrepTool::new(&ctx.cwd);
    let result = tool.execute(
        "tool-call",
        agent_tools::GrepToolArgs {
            pattern,
            path: get_optional_string_arg(args, "path"),
            glob: get_optional_string_arg(args, "glob"),
            ignore_case: get_optional_bool_arg(args, "ignoreCase"),
            literal: get_optional_bool_arg(args, "literal"),
            context: get_optional_usize_arg(args, "context"),
            limit: get_optional_usize_arg(args, "limit"),
        },
    )?;
    Ok(tool_result_to_text(result))
}

fn find_tool(args: &Value, ctx: &ToolContext) -> Result<String, String> {
    let pattern = get_string_arg(args, "pattern")?;
    let tool = agent_tools::FindTool::new(&ctx.cwd);
    let result = tool.execute(
        "tool-call",
        agent_tools::FindToolArgs {
            pattern,
            path: get_optional_string_arg(args, "path"),
            limit: get_optional_usize_arg(args, "limit"),
        },
    )?;
    Ok(tool_result_to_text(result))
}

fn ls_tool(args: &Value, ctx: &ToolContext) -> Result<String, String> {
    let tool = agent_tools::LsTool::new(&ctx.cwd);
    let result = tool.execute(
        "tool-call",
        agent_tools::LsToolArgs {
            path: get_optional_string_arg(args, "path"),
            limit: get_optional_usize_arg(args, "limit"),
        },
    )?;
    Ok(tool_result_to_text(result))
}

fn get_string_arg(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
        .ok_or_else(|| format!("Missing or invalid \"{}\" argument", key))
}

fn get_optional_string_arg(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
}

fn get_optional_bool_arg(args: &Value, key: &str) -> Option<bool> {
    args.get(key).and_then(|value| value.as_bool())
}

fn get_optional_usize_arg(args: &Value, key: &str) -> Option<usize> {
    args.get(key)
        .and_then(|value| value.as_i64())
        .and_then(|value| {
            if value < 0 {
                None
            } else {
                Some(value as usize)
            }
        })
}

fn get_optional_u64_arg(args: &Value, key: &str) -> Option<u64> {
    args.get(key)
        .and_then(|value| value.as_i64())
        .and_then(|value| if value < 0 { None } else { Some(value as u64) })
}

fn tool_result_to_text(result: agent_tools::ToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}
