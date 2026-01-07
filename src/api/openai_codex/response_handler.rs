//! Response handling for OpenAI Codex API
//!
//! Handles error parsing, rate limit extraction, and SSE stream parsing.

use reqwest::header::HeaderMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Rate limit information for a single tier
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexRateLimit {
    pub used_percent: Option<f64>,
    pub window_minutes: Option<u64>,
    pub resets_at: Option<u64>,
}

/// Rate limits for primary and secondary tiers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexRateLimits {
    pub primary: Option<CodexRateLimit>,
    pub secondary: Option<CodexRateLimit>,
}

/// Error information from the Codex API
#[derive(Debug, Clone)]
pub struct CodexErrorInfo {
    pub message: String,
    pub status: u16,
    pub friendly_message: Option<String>,
    pub rate_limits: Option<CodexRateLimits>,
    pub raw: Option<String>,
}

/// Parse an error response from the Codex API
pub fn parse_codex_error(status: u16, headers: &HeaderMap, body: &str) -> CodexErrorInfo {
    let mut message = if body.is_empty() {
        "Request failed".to_string()
    } else {
        body.to_string()
    };
    let mut friendly_message: Option<String> = None;
    let mut rate_limits: Option<CodexRateLimits> = None;

    // Try to parse JSON error
    if let Ok(parsed) = serde_json::from_str::<Value>(body) {
        let error = parsed.get("error").unwrap_or(&Value::Null);

        // Extract rate limit headers
        let primary = CodexRateLimit {
            used_percent: parse_header_float(headers, "x-codex-primary-used-percent"),
            window_minutes: parse_header_u64(headers, "x-codex-primary-window-minutes"),
            resets_at: parse_header_u64(headers, "x-codex-primary-reset-at"),
        };
        let secondary = CodexRateLimit {
            used_percent: parse_header_float(headers, "x-codex-secondary-used-percent"),
            window_minutes: parse_header_u64(headers, "x-codex-secondary-window-minutes"),
            resets_at: parse_header_u64(headers, "x-codex-secondary-reset-at"),
        };

        if primary.used_percent.is_some() || secondary.used_percent.is_some() {
            rate_limits = Some(CodexRateLimits {
                primary: Some(primary),
                secondary: Some(secondary),
            });
        }

        // Check for usage limit errors
        let code = error
            .get("code")
            .or_else(|| error.get("type"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let is_rate_limit_error = code.contains("usage_limit_reached")
            || code.contains("usage_not_included")
            || code.contains("rate_limit_exceeded")
            || status == 429;

        if is_rate_limit_error {
            let resets_at = error
                .get("resets_at")
                .and_then(|v| v.as_u64())
                .or_else(|| {
                    rate_limits
                        .as_ref()
                        .and_then(|rl| rl.primary.as_ref())
                        .and_then(|p| p.resets_at)
                })
                .or_else(|| {
                    rate_limits
                        .as_ref()
                        .and_then(|rl| rl.secondary.as_ref())
                        .and_then(|s| s.resets_at)
                });

            let mins = resets_at.map(|reset_at| {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                let reset_ms = reset_at * 1000;
                if reset_ms > now {
                    (reset_ms - now) / 60000
                } else {
                    0
                }
            });

            let plan_type = error.get("plan_type").and_then(|v| v.as_str());
            let plan = plan_type
                .map(|p| format!(" ({} plan)", p.to_lowercase()))
                .unwrap_or_default();
            let when = mins
                .map(|m| format!(" Try again in ~{} min.", m))
                .unwrap_or_default();

            friendly_message = Some(format!(
                "You have hit your ChatGPT usage limit{}.{}",
                plan, when
            ));
        }

        // Extract error message
        if let Some(err_msg) = error.get("message").and_then(|v| v.as_str()) {
            message = err_msg.to_string();
        } else if let Some(ref friendly) = friendly_message {
            message = friendly.clone();
        }
    }

    CodexErrorInfo {
        message,
        status,
        friendly_message,
        rate_limits,
        raw: Some(body.to_string()),
    }
}

fn parse_header_float(headers: &HeaderMap, name: &str) -> Option<f64> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<f64>().ok())
}

fn parse_header_u64(headers: &HeaderMap, name: &str) -> Option<u64> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
}

/// SSE event from the Codex stream
#[derive(Debug, Clone)]
pub struct CodexSseEvent {
    pub event_type: String,
    pub data: Value,
}

/// Parse SSE chunks from the stream
pub fn parse_sse_chunk(chunk: &str) -> Option<CodexSseEvent> {
    let mut data_lines: Vec<&str> = Vec::new();
    let mut event_type: Option<&str> = None;

    for line in chunk.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(rest) = line.strip_prefix("event:") {
            let value = rest.trim();
            if !value.is_empty() {
                event_type = Some(value);
            }
        } else if let Some(rest) = line.strip_prefix("data:") {
            data_lines.push(rest.trim_start());
        }
    }

    if data_lines.is_empty() {
        return None;
    }

    let data_str = data_lines.join("\n").trim().to_string();
    if data_str.is_empty() || data_str == "[DONE]" {
        return None;
    }

    let data: Value = serde_json::from_str(&data_str).ok()?;

    // Get event type from JSON if not in SSE header
    let event_type = event_type
        .map(String::from)
        .or_else(|| data.get("type").and_then(|t| t.as_str()).map(String::from))
        .unwrap_or_default();

    Some(CodexSseEvent { event_type, data })
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::HeaderValue;

    #[test]
    fn test_parse_codex_error_rate_limit() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-codex-primary-used-percent",
            HeaderValue::from_static("99"),
        );
        headers.insert(
            "x-codex-primary-window-minutes",
            HeaderValue::from_static("60"),
        );

        let body = r#"{"error": {"code": "usage_limit_reached", "plan_type": "Plus"}}"#;
        let info = parse_codex_error(429, &headers, body);

        assert!(info.friendly_message.is_some());
        assert!(info
            .friendly_message
            .as_ref()
            .unwrap()
            .to_lowercase()
            .contains("usage limit"));
        assert!(info.rate_limits.is_some());
        assert_eq!(
            info.rate_limits
                .as_ref()
                .unwrap()
                .primary
                .as_ref()
                .unwrap()
                .used_percent,
            Some(99.0)
        );
    }

    #[test]
    fn test_parse_sse_chunk() {
        let chunk = r#"event: response.output_text.delta
data: {"type": "response.output_text.delta", "delta": "Hello"}"#;

        let event = parse_sse_chunk(chunk).unwrap();
        assert_eq!(event.event_type, "response.output_text.delta");
        assert_eq!(
            event.data.get("delta").and_then(|d| d.as_str()),
            Some("Hello")
        );
    }

    #[test]
    fn test_parse_sse_chunk_done() {
        let chunk = "data: [DONE]";
        assert!(parse_sse_chunk(chunk).is_none());
    }
}
