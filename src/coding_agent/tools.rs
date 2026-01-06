use crate::core::messages::ContentBlock;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const DEFAULT_MAX_LINES: usize = 2000;
const DEFAULT_MAX_BYTES: usize = 50 * 1024;
const GREP_MAX_LINE_LENGTH: usize = 500;

#[derive(Clone, Debug)]
pub struct ToolResult {
    pub content: Vec<ContentBlock>,
    pub details: Option<Value>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TruncationResult {
    pub content: String,
    pub truncated: bool,
    pub truncated_by: Option<TruncatedBy>,
    pub total_lines: usize,
    pub total_bytes: usize,
    pub output_lines: usize,
    pub output_bytes: usize,
    pub last_line_partial: bool,
    pub first_line_exceeds_limit: bool,
    pub max_lines: usize,
    pub max_bytes: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TruncatedBy {
    Lines,
    Bytes,
}

#[derive(Clone, Debug)]
pub struct ReadToolArgs {
    pub path: String,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct WriteToolArgs {
    pub path: String,
    pub content: String,
}

#[derive(Clone, Debug)]
pub struct EditToolArgs {
    pub path: String,
    pub old_text: String,
    pub new_text: String,
}

#[derive(Clone, Debug)]
pub struct BashToolArgs {
    pub command: String,
    pub timeout: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct GrepToolArgs {
    pub pattern: String,
    pub path: Option<String>,
    pub glob: Option<String>,
    pub ignore_case: Option<bool>,
    pub literal: Option<bool>,
    pub context: Option<usize>,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct FindToolArgs {
    pub pattern: String,
    pub path: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct LsToolArgs {
    pub path: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct ReadTool {
    cwd: PathBuf,
}

#[derive(Clone, Debug)]
pub struct WriteTool {
    cwd: PathBuf,
}

#[derive(Clone, Debug)]
pub struct EditTool {
    cwd: PathBuf,
}

#[derive(Clone, Debug)]
pub struct BashTool {
    cwd: PathBuf,
}

#[derive(Clone, Debug)]
pub struct GrepTool {
    cwd: PathBuf,
}

#[derive(Clone, Debug)]
pub struct FindTool {
    cwd: PathBuf,
}

#[derive(Clone, Debug)]
pub struct LsTool {
    cwd: PathBuf,
}

impl ReadTool {
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        Self { cwd: cwd.into() }
    }

    pub fn execute(&self, _call_id: &str, args: ReadToolArgs) -> Result<ToolResult, String> {
        let absolute_path = resolve_path(&args.path, &self.cwd);
        let data = fs::read(&absolute_path).map_err(|err| match err.kind() {
            std::io::ErrorKind::NotFound => format!("File not found: {}", args.path),
            _ => format!("Failed to read {}: {}", args.path, err),
        })?;

        if let Some(mime_type) = detect_image_mime_type(&data) {
            let encoded = base64_encode(&data);
            let note = format!("Read image file [{}]", mime_type);
            return Ok(ToolResult {
                content: vec![
                    ContentBlock::Text {
                        text: note,
                        text_signature: None,
                    },
                    ContentBlock::Image {
                        data: encoded,
                        mime_type: mime_type.to_string(),
                    },
                ],
                details: None,
            });
        }

        let text = String::from_utf8(data)
            .map_err(|err| format!("Failed to read {}: {}", args.path, err))?;
        let all_lines: Vec<&str> = text.split('\n').collect();
        let total_file_lines = all_lines.len();
        let offset_value = args.offset.unwrap_or(1);
        let start_line = offset_value.saturating_sub(1);

        if start_line >= total_file_lines {
            return Err(format!(
                "Offset {} is beyond end of file ({} lines total)",
                offset_value, total_file_lines
            ));
        }

        let (selected_content, user_limited_lines) = match args.limit {
            Some(limit) => {
                let end_line = (start_line + limit).min(total_file_lines);
                (
                    all_lines[start_line..end_line].join("\n"),
                    Some(end_line - start_line),
                )
            }
            None => (all_lines[start_line..].join("\n"), None),
        };

        let truncation = truncate_head(&selected_content, None);
        let start_line_display = start_line + 1;

        let mut details = None;
        let output_text = if truncation.first_line_exceeds_limit {
            details = Some(json!({ "truncation": truncation }));
            format!(
                "[Line {} is {}, exceeds {} limit. Use bash: sed -n '{}p' {} | head -c {}]",
                start_line_display,
                format_size(all_lines[start_line].len()),
                format_size(DEFAULT_MAX_BYTES),
                start_line_display,
                args.path,
                DEFAULT_MAX_BYTES
            )
        } else if truncation.truncated {
            details = Some(json!({ "truncation": truncation.clone() }));
            let end_line_display = start_line_display + truncation.output_lines.saturating_sub(1);
            let next_offset = end_line_display + 1;
            if matches!(truncation.truncated_by, Some(TruncatedBy::Lines)) {
                format!(
                    "{}\n\n[Showing lines {}-{} of {}. Use offset={} to continue]",
                    truncation.content,
                    start_line_display,
                    end_line_display,
                    total_file_lines,
                    next_offset
                )
            } else {
                format!(
                    "{}\n\n[Showing lines {}-{} of {} ({} limit). Use offset={} to continue]",
                    truncation.content,
                    start_line_display,
                    end_line_display,
                    total_file_lines,
                    format_size(DEFAULT_MAX_BYTES),
                    next_offset
                )
            }
        } else if let Some(user_limit) = user_limited_lines {
            if start_line + user_limit < total_file_lines {
                let remaining = total_file_lines - (start_line + user_limit);
                let next_offset = start_line + user_limit + 1;
                format!(
                    "{}\n\n[{} more lines in file. Use offset={} to continue]",
                    truncation.content, remaining, next_offset
                )
            } else {
                truncation.content
            }
        } else {
            truncation.content
        };

        Ok(ToolResult {
            content: vec![ContentBlock::Text {
                text: output_text,
                text_signature: None,
            }],
            details,
        })
    }
}

impl WriteTool {
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        Self { cwd: cwd.into() }
    }

    pub fn execute(&self, _call_id: &str, args: WriteToolArgs) -> Result<ToolResult, String> {
        let absolute_path = resolve_path(&args.path, &self.cwd);
        if let Some(parent) = absolute_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("Failed to create directory for {}: {}", args.path, err))?;
        }
        fs::write(&absolute_path, args.content.as_bytes())
            .map_err(|err| format!("Failed to write {}: {}", args.path, err))?;
        Ok(ToolResult {
            content: vec![ContentBlock::Text {
                text: format!(
                    "Successfully wrote {} bytes to {}",
                    args.content.len(),
                    args.path
                ),
                text_signature: None,
            }],
            details: None,
        })
    }
}

impl EditTool {
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        Self { cwd: cwd.into() }
    }

    pub fn execute(&self, _call_id: &str, args: EditToolArgs) -> Result<ToolResult, String> {
        let absolute_path = resolve_path(&args.path, &self.cwd);
        let raw_content = fs::read_to_string(&absolute_path)
            .map_err(|_| format!("File not found: {}", args.path))?;

        let (bom, content) = strip_bom(&raw_content);
        let original_ending = detect_line_ending(&content);
        let normalized_content = normalize_to_lf(&content);
        let normalized_old = normalize_to_lf(&args.old_text);
        let normalized_new = normalize_to_lf(&args.new_text);

        if !normalized_content.contains(&normalized_old) {
            return Err(format!(
                "Could not find the exact text in {}. The old text must match exactly including all whitespace and newlines.",
                args.path
            ));
        }

        let occurrences = normalized_content.matches(&normalized_old).count();
        if occurrences > 1 {
            return Err(format!(
                "Found {} occurrences of the text in {}. The text must be unique. Please provide more context to make it unique.",
                occurrences, args.path
            ));
        }

        let index = normalized_content
            .find(&normalized_old)
            .ok_or_else(|| "Unexpected failure locating text".to_string())?;
        let mut normalized_new_content = String::with_capacity(
            normalized_content.len() - normalized_old.len() + normalized_new.len(),
        );
        normalized_new_content.push_str(&normalized_content[..index]);
        normalized_new_content.push_str(&normalized_new);
        normalized_new_content.push_str(&normalized_content[index + normalized_old.len()..]);

        if normalized_new_content == normalized_content {
            return Err(format!(
                "No changes made to {}. The replacement produced identical content.",
                args.path
            ));
        }

        let restored = restore_line_endings(&normalized_new_content, original_ending);
        let final_content = format!("{bom}{restored}");
        fs::write(&absolute_path, final_content.as_bytes())
            .map_err(|err| format!("Failed to write {}: {}", args.path, err))?;

        let diff = generate_diff_string(&normalized_content, &normalized_new_content);
        let first_changed_line =
            find_first_changed_line(&normalized_content, &normalized_new_content);

        Ok(ToolResult {
            content: vec![ContentBlock::Text {
                text: format!("Successfully replaced text in {}.", args.path),
                text_signature: None,
            }],
            details: Some(json!({
                "diff": diff,
                "firstChangedLine": first_changed_line,
            })),
        })
    }
}

impl BashTool {
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        Self { cwd: cwd.into() }
    }

    pub fn execute(&self, _call_id: &str, args: BashToolArgs) -> Result<ToolResult, String> {
        let mut child = Command::new("bash")
            .arg("-lc")
            .arg(&args.command)
            .current_dir(&self.cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| format!("Failed to execute bash: {err}"))?;

        let mut stdout = child.stdout.take();
        let mut stderr = child.stderr.take();
        let start = Instant::now();
        let timeout = args.timeout.map(Duration::from_secs);
        let mut exit_status = None;
        let mut timed_out = false;

        loop {
            if let Some(status) = child
                .try_wait()
                .map_err(|err| format!("Failed to execute bash: {err}"))?
            {
                exit_status = Some(status);
                break;
            }
            if let Some(timeout) = timeout {
                if start.elapsed() >= timeout {
                    timed_out = true;
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        let mut output = Vec::new();
        if let Some(mut out) = stdout.take() {
            let _ = out.read_to_end(&mut output);
        }
        if let Some(mut err) = stderr.take() {
            let _ = err.read_to_end(&mut output);
        }
        let combined = String::from_utf8_lossy(&output).to_string();

        if timed_out {
            return Err(format!(
                "{}Command timed out after {} seconds",
                if combined.is_empty() { "" } else { "\n\n" },
                args.timeout.unwrap_or(0)
            ));
        }

        let status = exit_status.ok_or_else(|| "Command did not exit".to_string())?;
        if !status.success() {
            return Err(format!(
                "{}Command failed with code {}",
                if combined.is_empty() { "" } else { "\n\n" },
                status.code().unwrap_or(-1)
            ));
        }

        Ok(ToolResult {
            content: vec![ContentBlock::Text {
                text: if combined.is_empty() {
                    "(no output)".to_string()
                } else {
                    combined
                },
                text_signature: None,
            }],
            details: None,
        })
    }
}

impl GrepTool {
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        Self { cwd: cwd.into() }
    }

    pub fn execute(&self, _call_id: &str, args: GrepToolArgs) -> Result<ToolResult, String> {
        let search_path = resolve_path(args.path.as_deref().unwrap_or("."), &self.cwd);
        let metadata = fs::metadata(&search_path)
            .map_err(|_| format!("Path not found: {}", search_path.display()))?;
        let effective_limit = args.limit.unwrap_or(100).max(1);
        let context = args.context.unwrap_or(0);
        let pattern = if args.ignore_case.unwrap_or(false) {
            args.pattern.to_lowercase()
        } else {
            args.pattern.clone()
        };

        let mut matches_output = Vec::new();
        let mut match_count = 0usize;
        let mut match_limit_reached = false;

        if metadata.is_file() {
            let content = fs::read_to_string(&search_path)
                .map_err(|err| format!("Failed to read {}: {}", search_path.display(), err))?;
            let normalized = normalize_to_lf(&content);
            let lines: Vec<&str> = normalized.split('\n').collect();
            let file_label = search_path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| search_path.display().to_string());

            for (idx, line) in lines.iter().enumerate() {
                let haystack = if args.ignore_case.unwrap_or(false) {
                    line.to_lowercase()
                } else {
                    (*line).to_string()
                };
                if haystack.contains(&pattern) {
                    match_count += 1;
                    let line_number = idx + 1;
                    let start = if context > 0 {
                        line_number.saturating_sub(context)
                    } else {
                        line_number
                    };
                    let end = if context > 0 {
                        (line_number + context).min(lines.len())
                    } else {
                        line_number
                    };

                    for current in start..=end {
                        let text_line = lines.get(current - 1).copied().unwrap_or("");
                        let sanitized = text_line.replace('\r', "");
                        let (trimmed, _was_truncated) =
                            truncate_line(&sanitized, GREP_MAX_LINE_LENGTH);
                        if current == line_number {
                            matches_output.push(format!("{file_label}:{current}: {trimmed}"));
                        } else {
                            matches_output.push(format!("{file_label}-{current}- {trimmed}"));
                        }
                    }

                    if match_count >= effective_limit {
                        match_limit_reached = true;
                        break;
                    }
                }
            }
        } else {
            return Err("Directory search is not implemented yet.".to_string());
        }

        if match_count == 0 {
            return Ok(ToolResult {
                content: vec![ContentBlock::Text {
                    text: "No matches found".to_string(),
                    text_signature: None,
                }],
                details: None,
            });
        }

        let mut output = matches_output.join("\n");
        if match_limit_reached {
            output.push_str(&format!(
                "\n\n[{} matches limit reached. Use limit={} for more, or refine pattern]",
                effective_limit,
                effective_limit * 2
            ));
        }

        Ok(ToolResult {
            content: vec![ContentBlock::Text {
                text: output,
                text_signature: None,
            }],
            details: None,
        })
    }
}

impl FindTool {
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        Self { cwd: cwd.into() }
    }

    pub fn execute(&self, _call_id: &str, args: FindToolArgs) -> Result<ToolResult, String> {
        let search_path = resolve_path(args.path.as_deref().unwrap_or("."), &self.cwd);
        let effective_limit = args.limit.unwrap_or(1000);
        let ignore_set = read_gitignore(&search_path);

        let mut results = Vec::new();
        collect_files(
            &search_path,
            &search_path,
            &args.pattern,
            &ignore_set,
            &mut results,
        );

        if results.is_empty() {
            return Ok(ToolResult {
                content: vec![ContentBlock::Text {
                    text: "No files found matching pattern".to_string(),
                    text_signature: None,
                }],
                details: None,
            });
        }

        results.sort();
        if results.len() > effective_limit {
            results.truncate(effective_limit);
        }

        Ok(ToolResult {
            content: vec![ContentBlock::Text {
                text: results.join("\n"),
                text_signature: None,
            }],
            details: None,
        })
    }
}

impl LsTool {
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        Self { cwd: cwd.into() }
    }

    pub fn execute(&self, _call_id: &str, args: LsToolArgs) -> Result<ToolResult, String> {
        let dir_path = resolve_path(args.path.as_deref().unwrap_or("."), &self.cwd);
        let effective_limit = args.limit.unwrap_or(500);
        let metadata = fs::metadata(&dir_path)
            .map_err(|_| format!("Path not found: {}", dir_path.display()))?;
        if !metadata.is_dir() {
            return Err(format!("Not a directory: {}", dir_path.display()));
        }

        let mut entries = Vec::new();
        for entry in
            fs::read_dir(&dir_path).map_err(|err| format!("Cannot read directory: {}", err))?
        {
            let entry = entry.map_err(|err| format!("Cannot read directory: {}", err))?;
            let file_name = entry.file_name();
            let name = file_name.to_string_lossy().to_string();
            let entry_path = entry.path();
            let suffix = match entry_path.metadata() {
                Ok(stat) if stat.is_dir() => "/",
                _ => "",
            };
            entries.push(format!("{name}{suffix}"));
        }

        entries.sort_by_key(|a| a.to_lowercase());
        if entries.len() > effective_limit {
            entries.truncate(effective_limit);
        }

        let output = if entries.is_empty() {
            "(empty directory)".to_string()
        } else {
            entries.join("\n")
        };

        Ok(ToolResult {
            content: vec![ContentBlock::Text {
                text: output,
                text_signature: None,
            }],
            details: None,
        })
    }
}

fn resolve_path(path: &str, cwd: &Path) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    }
}

fn detect_image_mime_type(data: &[u8]) -> Option<&'static str> {
    let png_magic: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if data.len() >= png_magic.len() && data[..png_magic.len()] == png_magic {
        return Some("image/png");
    }
    None
}

fn base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(data.len().div_ceil(3) * 4);
    let mut i = 0;
    while i < data.len() {
        let b0 = data[i];
        let b1 = if i + 1 < data.len() { data[i + 1] } else { 0 };
        let b2 = if i + 2 < data.len() { data[i + 2] } else { 0 };
        output.push(TABLE[(b0 >> 2) as usize] as char);
        output.push(TABLE[((b0 & 0x03) << 4 | (b1 >> 4)) as usize] as char);
        if i + 1 < data.len() {
            output.push(TABLE[((b1 & 0x0f) << 2 | (b2 >> 6)) as usize] as char);
        } else {
            output.push('=');
        }
        if i + 2 < data.len() {
            output.push(TABLE[(b2 & 0x3f) as usize] as char);
        } else {
            output.push('=');
        }
        i += 3;
    }
    output
}

fn format_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes}B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1}KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1}MB", bytes as f64 / 1024.0 / 1024.0)
    }
}

fn truncate_head(content: &str, options: Option<(usize, usize)>) -> TruncationResult {
    let (max_lines, max_bytes) = options.unwrap_or((DEFAULT_MAX_LINES, DEFAULT_MAX_BYTES));
    let total_bytes = content.len();
    let lines: Vec<&str> = content.split('\n').collect();
    let total_lines = lines.len();

    if total_lines <= max_lines && total_bytes <= max_bytes {
        return TruncationResult {
            content: content.to_string(),
            truncated: false,
            truncated_by: None,
            total_lines,
            total_bytes,
            output_lines: total_lines,
            output_bytes: total_bytes,
            last_line_partial: false,
            first_line_exceeds_limit: false,
            max_lines,
            max_bytes,
        };
    }

    let first_line_bytes = lines.first().map(|line| line.len()).unwrap_or(0);
    if first_line_bytes > max_bytes {
        return TruncationResult {
            content: String::new(),
            truncated: true,
            truncated_by: Some(TruncatedBy::Bytes),
            total_lines,
            total_bytes,
            output_lines: 0,
            output_bytes: 0,
            last_line_partial: false,
            first_line_exceeds_limit: true,
            max_lines,
            max_bytes,
        };
    }

    let mut output_lines = Vec::new();
    let mut output_bytes = 0usize;
    let mut truncated_by = TruncatedBy::Lines;

    for (idx, line) in lines.iter().enumerate() {
        if idx >= max_lines {
            break;
        }
        let line_bytes = line.len() + if idx > 0 { 1 } else { 0 };
        if output_bytes + line_bytes > max_bytes {
            truncated_by = TruncatedBy::Bytes;
            break;
        }
        output_lines.push(*line);
        output_bytes += line_bytes;
    }

    if output_lines.len() >= max_lines && output_bytes <= max_bytes {
        truncated_by = TruncatedBy::Lines;
    }

    let output_content = output_lines.join("\n");
    let final_output_bytes = output_content.len();

    TruncationResult {
        content: output_content,
        truncated: true,
        truncated_by: Some(truncated_by),
        total_lines,
        total_bytes,
        output_lines: output_lines.len(),
        output_bytes: final_output_bytes,
        last_line_partial: false,
        first_line_exceeds_limit: false,
        max_lines,
        max_bytes,
    }
}

fn truncate_line(line: &str, max_chars: usize) -> (String, bool) {
    if line.len() <= max_chars {
        (line.to_string(), false)
    } else {
        (format!("{}... [truncated]", &line[..max_chars]), true)
    }
}

fn normalize_to_lf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

#[derive(Clone, Copy)]
enum LineEnding {
    CrLf,
    Lf,
}

fn detect_line_ending(content: &str) -> LineEnding {
    let crlf_idx = content.find("\r\n");
    let lf_idx = content.find('\n');
    match (crlf_idx, lf_idx) {
        (_, None) => LineEnding::Lf,
        (None, Some(_)) => LineEnding::Lf,
        (Some(crlf), Some(lf)) => {
            if crlf < lf {
                LineEnding::CrLf
            } else {
                LineEnding::Lf
            }
        }
    }
}

fn restore_line_endings(text: &str, ending: LineEnding) -> String {
    match ending {
        LineEnding::CrLf => text.replace('\n', "\r\n"),
        LineEnding::Lf => text.to_string(),
    }
}

fn strip_bom(content: &str) -> (String, String) {
    let bom = '\u{feff}';
    if content.starts_with(bom) {
        (bom.to_string(), content[bom.len_utf8()..].to_string())
    } else {
        ("".to_string(), content.to_string())
    }
}

fn generate_diff_string(old_content: &str, new_content: &str) -> String {
    if old_content == new_content {
        return String::new();
    }
    format!("---\n+++ \n-{}\n+{}", old_content, new_content)
}

fn find_first_changed_line(old_content: &str, new_content: &str) -> Option<usize> {
    let old_lines: Vec<&str> = old_content.split('\n').collect();
    let new_lines: Vec<&str> = new_content.split('\n').collect();
    let min_len = old_lines.len().min(new_lines.len());
    for idx in 0..min_len {
        if old_lines[idx] != new_lines[idx] {
            return Some(idx + 1);
        }
    }
    if old_lines.len() != new_lines.len() {
        return Some(min_len + 1);
    }
    None
}

fn read_gitignore(base: &Path) -> HashSet<String> {
    let mut entries = HashSet::new();
    let path = base.join(".gitignore");
    if let Ok(content) = fs::read_to_string(path) {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            entries.insert(trimmed.to_string());
        }
    }
    entries
}

fn collect_files(
    base: &Path,
    current: &Path,
    pattern: &str,
    ignore_set: &HashSet<String>,
    results: &mut Vec<String>,
) {
    let entries = match fs::read_dir(current) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let rel = path.strip_prefix(base).unwrap_or(&path);
        let rel_string = rel.to_string_lossy().replace('\\', "/");
        if ignore_set.contains(rel_string.as_str()) {
            continue;
        }
        let metadata = match entry.metadata() {
            Ok(meta) => meta,
            Err(_) => continue,
        };
        if metadata.is_dir() {
            collect_files(base, &path, pattern, ignore_set, results);
        } else if metadata.is_file() && matches_pattern(&rel_string, pattern) {
            results.push(rel_string);
        }
    }
}

fn matches_pattern(path: &str, pattern: &str) -> bool {
    if pattern == "**/*.txt" || pattern == "*.txt" {
        return path.ends_with(".txt");
    }
    if let Some(idx) = pattern.rfind('*') {
        let suffix = &pattern[idx + 1..];
        if !suffix.is_empty() {
            return path.ends_with(suffix);
        }
    }
    path == pattern
}
