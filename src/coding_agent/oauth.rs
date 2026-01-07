//! OAuth authentication flows for AI providers.
//!
//! Supports:
//! - Anthropic (Claude Pro/Max) - PKCE authorization code flow
//! - GitHub Copilot - Device code flow
//! - OpenAI Codex - PKCE authorization code flow with local callback server

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::AuthCredential;

/// URL encode a string
fn url_encode(s: &str) -> String {
    let mut encoded = String::new();
    for c in s.chars() {
        match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => encoded.push(c),
            _ => {
                for b in c.to_string().as_bytes() {
                    encoded.push_str(&format!("%{:02X}", b));
                }
            }
        }
    }
    encoded
}

/// Build URL query string from parameters
fn build_query_string(params: &[(&str, &str)]) -> String {
    params
        .iter()
        .map(|(k, v)| format!("{}={}", url_encode(k), url_encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Build form-urlencoded body from parameters
fn build_form_body(params: &[(&str, &str)]) -> String {
    build_query_string(params)
}

/// Parse query string into HashMap
fn parse_query_string(s: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for pair in s.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            map.insert(k.to_string(), v.to_string());
        }
    }
    map
}

/// OAuth provider information
#[derive(Clone, Debug)]
pub struct OAuthProviderInfo {
    pub id: String,
    pub name: String,
    pub available: bool,
}

/// Get list of supported OAuth providers
pub fn get_oauth_providers() -> Vec<OAuthProviderInfo> {
    vec![
        OAuthProviderInfo {
            id: "anthropic".to_string(),
            name: "Anthropic (Claude Pro/Max)".to_string(),
            available: true,
        },
        OAuthProviderInfo {
            id: "openai-codex".to_string(),
            name: "ChatGPT Plus/Pro (Codex Subscription)".to_string(),
            available: true,
        },
        OAuthProviderInfo {
            id: "github-copilot".to_string(),
            name: "GitHub Copilot".to_string(),
            available: true,
        },
    ]
}

/// OAuth credentials result
#[derive(Clone, Debug)]
pub struct OAuthCredentials {
    pub access: String,
    pub refresh: String,
    pub expires: i64,
    pub enterprise_url: Option<String>,
    pub project_id: Option<String>,
    pub account_id: Option<String>,
}

impl OAuthCredentials {
    pub fn to_auth_credential(&self) -> AuthCredential {
        AuthCredential::OAuth {
            access: self.access.clone(),
            refresh: Some(self.refresh.clone()),
            expires: Some(self.expires),
            enterprise_url: self.enterprise_url.clone(),
            project_id: self.project_id.clone(),
            email: None,
            account_id: self.account_id.clone(),
        }
    }
}

// ============================================================================
// PKCE Utilities
// ============================================================================

/// Generate PKCE code verifier and challenge
fn generate_pkce() -> (String, String) {
    let mut verifier_bytes = [0u8; 32];
    rand::fill(&mut verifier_bytes);
    let verifier = URL_SAFE_NO_PAD.encode(verifier_bytes);

    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let hash = hasher.finalize();
    let challenge = URL_SAFE_NO_PAD.encode(hash);

    (verifier, challenge)
}

/// Generate random hex string
fn generate_random_hex(len: usize) -> String {
    let mut bytes = vec![0u8; len];
    rand::fill(&mut bytes[..]);
    hex::encode(bytes)
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ============================================================================
// Anthropic OAuth
// ============================================================================

const ANTHROPIC_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const ANTHROPIC_AUTHORIZE_URL: &str = "https://claude.ai/oauth/authorize";
const ANTHROPIC_TOKEN_URL: &str = "https://console.anthropic.com/v1/oauth/token";
const ANTHROPIC_REDIRECT_URI: &str = "https://console.anthropic.com/oauth/code/callback";
const ANTHROPIC_SCOPES: &str = "org:create_api_key user:profile user:inference";

/// Login with Anthropic OAuth
///
/// Returns the authorization URL that should be shown to the user.
/// The user will complete authorization and receive a code in the format `code#state`.
pub fn anthropic_get_auth_url() -> (String, String) {
    let (verifier, challenge) = generate_pkce();

    let params = [
        ("code", "true"),
        ("client_id", ANTHROPIC_CLIENT_ID),
        ("response_type", "code"),
        ("redirect_uri", ANTHROPIC_REDIRECT_URI),
        ("scope", ANTHROPIC_SCOPES),
        ("code_challenge", challenge.as_str()),
        ("code_challenge_method", "S256"),
        ("state", verifier.as_str()),
    ];

    let query = build_query_string(&params);
    let url = format!("{}?{}", ANTHROPIC_AUTHORIZE_URL, query);

    (url, verifier)
}

/// Exchange Anthropic authorization code for tokens
pub fn anthropic_exchange_code(
    auth_code: &str,
    verifier: &str,
) -> Result<OAuthCredentials, String> {
    // Parse the auth code format: code#state
    let (code, state) = if auth_code.contains('#') {
        let parts: Vec<&str> = auth_code.splitn(2, '#').collect();
        (parts[0], parts.get(1).copied())
    } else {
        (auth_code, None)
    };

    let body = serde_json::json!({
        "grant_type": "authorization_code",
        "client_id": ANTHROPIC_CLIENT_ID,
        "code": code,
        "state": state.unwrap_or(""),
        "redirect_uri": ANTHROPIC_REDIRECT_URI,
        "code_verifier": verifier,
    });

    let client = reqwest::blocking::Client::new();
    let response = client
        .post(ANTHROPIC_TOKEN_URL)
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .send()
        .map_err(|e| format!("Token exchange request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().unwrap_or_default();
        return Err(format!("Token exchange failed ({}): {}", status, text));
    }

    let data: serde_json::Value = response
        .json()
        .map_err(|e| format!("Failed to parse token response: {}", e))?;

    let access_token = data["access_token"]
        .as_str()
        .ok_or("Missing access_token")?
        .to_string();
    let refresh_token = data["refresh_token"]
        .as_str()
        .ok_or("Missing refresh_token")?
        .to_string();
    let expires_in = data["expires_in"].as_i64().ok_or("Missing expires_in")?;

    // Calculate expiry time with 5 minute buffer
    let expires_at = now_millis() + expires_in * 1000 - 5 * 60 * 1000;

    Ok(OAuthCredentials {
        access: access_token,
        refresh: refresh_token,
        expires: expires_at,
        enterprise_url: None,
        project_id: None,
        account_id: None,
    })
}

/// Refresh Anthropic OAuth token
pub fn anthropic_refresh_token(refresh_token: &str) -> Result<OAuthCredentials, String> {
    let body = serde_json::json!({
        "grant_type": "refresh_token",
        "client_id": ANTHROPIC_CLIENT_ID,
        "refresh_token": refresh_token,
    });

    let client = reqwest::blocking::Client::new();
    let response = client
        .post(ANTHROPIC_TOKEN_URL)
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .send()
        .map_err(|e| format!("Token refresh request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().unwrap_or_default();
        return Err(format!("Token refresh failed ({}): {}", status, text));
    }

    let data: serde_json::Value = response
        .json()
        .map_err(|e| format!("Failed to parse refresh response: {}", e))?;

    let access_token = data["access_token"]
        .as_str()
        .ok_or("Missing access_token")?
        .to_string();
    let refresh_token = data["refresh_token"]
        .as_str()
        .ok_or("Missing refresh_token")?
        .to_string();
    let expires_in = data["expires_in"].as_i64().ok_or("Missing expires_in")?;

    let expires_at = now_millis() + expires_in * 1000 - 5 * 60 * 1000;

    Ok(OAuthCredentials {
        access: access_token,
        refresh: refresh_token,
        expires: expires_at,
        enterprise_url: None,
        project_id: None,
        account_id: None,
    })
}

// ============================================================================
// GitHub Copilot OAuth (Device Code Flow)
// ============================================================================

const GITHUB_COPILOT_CLIENT_ID: &str = "Iv1.b507a08c87ecfe98";

const COPILOT_HEADERS: [(&str, &str); 4] = [
    ("User-Agent", "GitHubCopilotChat/0.35.0"),
    ("Editor-Version", "vscode/1.107.0"),
    ("Editor-Plugin-Version", "copilot-chat/0.35.0"),
    ("Copilot-Integration-Id", "vscode-chat"),
];

/// Normalize a GitHub domain input
pub fn normalize_github_domain(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    let url_str = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("https://{}", trimmed)
    };
    match url::Url::parse(&url_str) {
        Ok(url) => url.host_str().map(|s| s.to_string()),
        Err(_) => None,
    }
}

fn github_get_urls(domain: &str) -> (String, String, String) {
    (
        format!("https://{}/login/device/code", domain),
        format!("https://{}/login/oauth/access_token", domain),
        format!("https://api.{}/copilot_internal/v2/token", domain),
    )
}

/// Get the base URL for GitHub Copilot API from token
pub fn get_github_copilot_base_url(token: Option<&str>, enterprise_domain: Option<&str>) -> String {
    // If we have a token, try to extract proxy-ep
    if let Some(token) = token {
        if let Some(captures) = regex::Regex::new(r"proxy-ep=([^;]+)")
            .ok()
            .and_then(|re| re.captures(token))
        {
            if let Some(proxy_host) = captures.get(1) {
                let api_host = proxy_host.as_str().replacen("proxy.", "api.", 1);
                return format!("https://{}", api_host);
            }
        }
    }

    // Fallback
    if let Some(domain) = enterprise_domain {
        format!("https://copilot-api.{}", domain)
    } else {
        "https://api.individual.githubcopilot.com".to_string()
    }
}

/// Device code response from GitHub
#[derive(Debug, Clone)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub interval: u64,
    pub expires_in: u64,
}

/// Start GitHub device code flow
pub fn github_start_device_flow(domain: &str) -> Result<DeviceCodeResponse, String> {
    let (device_code_url, _, _) = github_get_urls(domain);

    let body = serde_json::json!({
        "client_id": GITHUB_COPILOT_CLIENT_ID,
        "scope": "read:user",
    });

    let client = reqwest::blocking::Client::new();
    let response = client
        .post(&device_code_url)
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .header("User-Agent", "GitHubCopilotChat/0.35.0")
        .body(body.to_string())
        .send()
        .map_err(|e| format!("Device code request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().unwrap_or_default();
        return Err(format!("Device code request failed ({}): {}", status, text));
    }

    let data: serde_json::Value = response
        .json()
        .map_err(|e| format!("Failed to parse device code response: {}", e))?;

    Ok(DeviceCodeResponse {
        device_code: data["device_code"]
            .as_str()
            .ok_or("Missing device_code")?
            .to_string(),
        user_code: data["user_code"]
            .as_str()
            .ok_or("Missing user_code")?
            .to_string(),
        verification_uri: data["verification_uri"]
            .as_str()
            .ok_or("Missing verification_uri")?
            .to_string(),
        interval: data["interval"].as_u64().unwrap_or(5),
        expires_in: data["expires_in"].as_u64().unwrap_or(900),
    })
}

/// Poll for GitHub access token
pub fn github_poll_for_token(
    domain: &str,
    device_code: &str,
    interval_seconds: u64,
    expires_in: u64,
    cancelled: Arc<AtomicBool>,
) -> Result<String, String> {
    let (_, access_token_url, _) = github_get_urls(domain);
    let deadline = now_millis() + (expires_in as i64 * 1000);
    let mut interval_ms = std::cmp::max(1000, interval_seconds * 1000);

    let client = reqwest::blocking::Client::new();

    while now_millis() < deadline {
        if cancelled.load(Ordering::Relaxed) {
            return Err("Login cancelled".to_string());
        }

        let body = serde_json::json!({
            "client_id": GITHUB_COPILOT_CLIENT_ID,
            "device_code": device_code,
            "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
        });

        let response = client
            .post(&access_token_url)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("User-Agent", "GitHubCopilotChat/0.35.0")
            .body(body.to_string())
            .send();

        if let Ok(resp) = response {
            if let Ok(data) = resp.json::<serde_json::Value>() {
                // Check for success
                if let Some(access_token) = data["access_token"].as_str() {
                    return Ok(access_token.to_string());
                }

                // Check for errors
                if let Some(error) = data["error"].as_str() {
                    match error {
                        "authorization_pending" => {
                            // Keep waiting
                        }
                        "slow_down" => {
                            interval_ms += 5000;
                        }
                        _ => {
                            return Err(format!("Device flow failed: {}", error));
                        }
                    }
                }
            }
        }

        std::thread::sleep(Duration::from_millis(interval_ms));
    }

    Err("Device flow timed out".to_string())
}

/// Refresh GitHub Copilot token
pub fn github_refresh_copilot_token(
    github_access_token: &str,
    enterprise_domain: Option<&str>,
) -> Result<OAuthCredentials, String> {
    let domain = enterprise_domain.unwrap_or("github.com");
    let (_, _, copilot_token_url) = github_get_urls(domain);

    let client = reqwest::blocking::Client::new();
    let mut request = client
        .get(&copilot_token_url)
        .header("Accept", "application/json")
        .header("Authorization", format!("Bearer {}", github_access_token));

    for (key, value) in COPILOT_HEADERS {
        request = request.header(key, value);
    }

    let response = request
        .send()
        .map_err(|e| format!("Copilot token request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().unwrap_or_default();
        return Err(format!(
            "Copilot token request failed ({}): {}",
            status, text
        ));
    }

    let data: serde_json::Value = response
        .json()
        .map_err(|e| format!("Failed to parse Copilot token response: {}", e))?;

    let token = data["token"].as_str().ok_or("Missing token")?.to_string();
    let expires_at = data["expires_at"].as_i64().ok_or("Missing expires_at")?;

    Ok(OAuthCredentials {
        refresh: github_access_token.to_string(),
        access: token,
        expires: expires_at * 1000 - 5 * 60 * 1000,
        enterprise_url: enterprise_domain.map(|s| s.to_string()),
        project_id: None,
        account_id: None,
    })
}

// ============================================================================
// OpenAI Codex OAuth
// ============================================================================

const OPENAI_CODEX_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const OPENAI_CODEX_AUTHORIZE_URL: &str = "https://auth.openai.com/oauth/authorize";
const OPENAI_CODEX_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const OPENAI_CODEX_REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
const OPENAI_CODEX_SCOPE: &str = "openid profile email offline_access";

/// Create OpenAI Codex authorization flow
pub fn openai_codex_get_auth_url() -> (String, String, String) {
    let (verifier, challenge) = generate_pkce();
    let state = generate_random_hex(16);

    let params = [
        ("response_type", "code"),
        ("client_id", OPENAI_CODEX_CLIENT_ID),
        ("redirect_uri", OPENAI_CODEX_REDIRECT_URI),
        ("scope", OPENAI_CODEX_SCOPE),
        ("code_challenge", challenge.as_str()),
        ("code_challenge_method", "S256"),
        ("state", state.as_str()),
        ("id_token_add_organizations", "true"),
        ("codex_cli_simplified_flow", "true"),
        ("originator", "codex_cli_rs"),
    ];

    let query = build_query_string(&params);
    let url = format!("{}?{}", OPENAI_CODEX_AUTHORIZE_URL, query);

    (url, verifier, state)
}

/// Parse authorization code from URL or code#state format
fn parse_openai_authorization_input(input: &str) -> (Option<String>, Option<String>) {
    let value = input.trim();
    if value.is_empty() {
        return (None, None);
    }

    // Try parsing as URL
    if let Ok(url) = url::Url::parse(value) {
        let code = url
            .query_pairs()
            .find(|(k, _)| k == "code")
            .map(|(_, v)| v.to_string());
        let state = url
            .query_pairs()
            .find(|(k, _)| k == "state")
            .map(|(_, v)| v.to_string());
        if code.is_some() {
            return (code, state);
        }
    }

    // Try code#state format
    if value.contains('#') {
        let parts: Vec<&str> = value.splitn(2, '#').collect();
        return (
            Some(parts[0].to_string()),
            parts.get(1).map(|s| s.to_string()),
        );
    }

    // Try code=...&state=... format
    if value.contains("code=") {
        let pairs = parse_query_string(value);
        if !pairs.is_empty() {
            return (pairs.get("code").cloned(), pairs.get("state").cloned());
        }
    }

    // Just the code
    (Some(value.to_string()), None)
}

/// Decode JWT to extract account ID
fn decode_jwt_account_id(token: &str) -> Option<String> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }

    let payload = URL_SAFE_NO_PAD.decode(parts[1]).ok()?;
    let json: serde_json::Value = serde_json::from_slice(&payload).ok()?;

    json["https://api.openai.com/auth"]["chatgpt_account_id"]
        .as_str()
        .map(|s| s.to_string())
}

/// Start local OAuth callback server for OpenAI Codex
pub struct OAuthCallbackServer {
    available: bool,
    code: Arc<Mutex<Option<String>>>,
    cancelled: Arc<AtomicBool>,
}

impl OAuthCallbackServer {
    pub fn start(state: &str) -> Self {
        let code = Arc::new(Mutex::new(None));
        let cancelled = Arc::new(AtomicBool::new(false));

        let listener = TcpListener::bind("127.0.0.1:1455").ok();
        let available = listener.is_some();

        if let Some(listener) = listener {
            // Set non-blocking so we can check for cancellation
            let _ = listener.set_nonblocking(true);

            let code_clone = Arc::clone(&code);
            let cancelled_clone = Arc::clone(&cancelled);
            let state = state.to_string();

            std::thread::spawn(move || {
                Self::run_server(listener, &state, code_clone, cancelled_clone);
            });
        }

        Self {
            available,
            code,
            cancelled,
        }
    }

    fn run_server(
        listener: TcpListener,
        state: &str,
        code: Arc<Mutex<Option<String>>>,
        cancelled: Arc<AtomicBool>,
    ) {
        loop {
            if cancelled.load(Ordering::Relaxed) {
                break;
            }

            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut request_line = String::new();
                    if reader.read_line(&mut request_line).is_ok() {
                        // Parse GET /auth/callback?code=...&state=... HTTP/1.1
                        if let Some(path) = request_line.split_whitespace().nth(1) {
                            if path.starts_with("/auth/callback") {
                                if let Ok(url) =
                                    url::Url::parse(&format!("http://localhost{}", path))
                                {
                                    let params: HashMap<_, _> = url.query_pairs().collect();

                                    // Check state
                                    if params.get("state").map(|s| s.as_ref()) == Some(state) {
                                        if let Some(auth_code) = params.get("code") {
                                            *code.lock().unwrap() = Some(auth_code.to_string());

                                            // Send success response
                                            let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n\
                                                <html><body><p>Authentication successful. Return to your terminal.</p></body></html>";
                                            let _ = stream.write_all(response.as_bytes());
                                            break;
                                        }
                                    }
                                }
                            }
                        }

                        // Send error response
                        let response = "HTTP/1.1 400 Bad Request\r\n\r\n";
                        let _ = stream.write_all(response.as_bytes());
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(_) => break,
            }
        }
    }

    pub fn wait_for_code(&self, timeout_seconds: u64) -> Option<String> {
        let deadline = now_millis() + (timeout_seconds as i64 * 1000);

        while now_millis() < deadline {
            if self.cancelled.load(Ordering::Relaxed) {
                return None;
            }

            if let Some(code) = self.code.lock().unwrap().clone() {
                return Some(code);
            }

            std::thread::sleep(Duration::from_millis(100));
        }

        None
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn is_available(&self) -> bool {
        self.available
    }
}

/// Exchange OpenAI Codex authorization code for tokens
pub fn openai_codex_exchange_code(code: &str, verifier: &str) -> Result<OAuthCredentials, String> {
    let params = [
        ("grant_type", "authorization_code"),
        ("client_id", OPENAI_CODEX_CLIENT_ID),
        ("code", code),
        ("code_verifier", verifier),
        ("redirect_uri", OPENAI_CODEX_REDIRECT_URI),
    ];

    let client = reqwest::blocking::Client::new();
    let response = client
        .post(OPENAI_CODEX_TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(build_form_body(&params))
        .send()
        .map_err(|e| format!("Token exchange request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().unwrap_or_default();
        return Err(format!("Token exchange failed ({}): {}", status, text));
    }

    let data: serde_json::Value = response
        .json()
        .map_err(|e| format!("Failed to parse token response: {}", e))?;

    let access_token = data["access_token"]
        .as_str()
        .ok_or("Missing access_token")?
        .to_string();
    let refresh_token = data["refresh_token"]
        .as_str()
        .ok_or("Missing refresh_token")?
        .to_string();
    let expires_in = data["expires_in"].as_i64().ok_or("Missing expires_in")?;

    let account_id =
        decode_jwt_account_id(&access_token).ok_or("Failed to extract accountId from token")?;

    let expires_at = now_millis() + expires_in * 1000;

    Ok(OAuthCredentials {
        access: access_token,
        refresh: refresh_token,
        expires: expires_at,
        enterprise_url: None,
        project_id: None,
        account_id: Some(account_id),
    })
}

/// Refresh OpenAI Codex token
pub fn openai_codex_refresh_token(refresh_token: &str) -> Result<OAuthCredentials, String> {
    let params = [
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", OPENAI_CODEX_CLIENT_ID),
    ];

    let client = reqwest::blocking::Client::new();
    let response = client
        .post(OPENAI_CODEX_TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(build_form_body(&params))
        .send()
        .map_err(|e| format!("Token refresh request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().unwrap_or_default();
        return Err(format!("Token refresh failed ({}): {}", status, text));
    }

    let data: serde_json::Value = response
        .json()
        .map_err(|e| format!("Failed to parse refresh response: {}", e))?;

    let access_token = data["access_token"]
        .as_str()
        .ok_or("Missing access_token")?
        .to_string();
    let refresh_token = data["refresh_token"]
        .as_str()
        .ok_or("Missing refresh_token")?
        .to_string();
    let expires_in = data["expires_in"].as_i64().ok_or("Missing expires_in")?;

    let account_id =
        decode_jwt_account_id(&access_token).ok_or("Failed to extract accountId from token")?;

    let expires_at = now_millis() + expires_in * 1000;

    Ok(OAuthCredentials {
        access: access_token,
        refresh: refresh_token,
        expires: expires_at,
        enterprise_url: None,
        project_id: None,
        account_id: Some(account_id),
    })
}

/// Login flow helper for OpenAI Codex that handles manual code input
pub fn openai_codex_login_with_input(
    auth_input: &str,
    verifier: &str,
    state: &str,
) -> Result<OAuthCredentials, String> {
    let (code, parsed_state) = parse_openai_authorization_input(auth_input);

    let code = code.ok_or("Missing authorization code")?;

    // Verify state if provided
    if let Some(parsed_state) = parsed_state {
        if parsed_state != state {
            return Err("State mismatch".to_string());
        }
    }

    openai_codex_exchange_code(&code, verifier)
}

/// Open URL in browser (platform-specific)
pub fn open_browser(url: &str) -> bool {
    #[cfg(target_os = "macos")]
    let cmd = "open";
    #[cfg(target_os = "windows")]
    let cmd = "start";
    #[cfg(target_os = "linux")]
    let cmd = "xdg-open";
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    let cmd = "";

    if cmd.is_empty() {
        return false;
    }

    std::process::Command::new(cmd)
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .is_ok()
}
