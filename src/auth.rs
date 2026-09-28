use crate::storage::CopilotOAuthData;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use serde::Deserialize;
use std::collections::HashMap;
use std::io::{self, Write};
use std::time::Duration;

pub const CLIENT_ID: &str = "Ov23ctDVkRmgkPke0Mmm";
pub const COPILOT_CLI_USER_AGENT: &str = "copilot/1.0.82";
pub const COPILOT_INTEGRATION_ID: &str = "copilot-developer-cli";
pub const COPILOT_API_VERSION: &str = "2026-08-01";

#[derive(Debug, Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    #[serde(default = "default_interval")]
    interval: u64,
    #[serde(default = "default_expires_in")]
    expires_in: u64,
}

fn default_interval() -> u64 {
    5
}

fn default_expires_in() -> u64 {
    900
}

#[derive(Debug, Deserialize)]
struct AccessTokenSuccessResponse {
    access_token: String,
}

#[derive(Debug, Deserialize)]
struct AccessTokenErrorResponse {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
    #[serde(default)]
    interval: Option<u64>,
}

pub fn get_github_copilot_base_url(enterprise_domain: Option<&str>) -> String {
    match enterprise_domain {
        Some(domain)
            if !domain.trim().is_empty()
                && domain != "github.com"
                && domain != "api.github.com" =>
        {
            let trimmed = domain.trim();
            if trimmed.starts_with("copilot-api.") {
                format!("https://{}", trimmed)
            } else {
                format!("https://copilot-api.{}", trimmed)
            }
        }
        _ => "https://api.githubcopilot.com".to_string(),
    }
}

pub fn default_github_api_base_url(enterprise_domain: Option<&str>) -> String {
    match enterprise_domain {
        Some(domain)
            if !domain.trim().is_empty()
                && domain != "github.com"
                && domain != "api.github.com" =>
        {
            let trimmed = domain.trim();
            format!("https://{}/api/v3", trimmed)
        }
        _ => "https://api.github.com".to_string(),
    }
}

pub async fn is_copilot_token_valid(client: &reqwest::Client, creds: &CopilotOAuthData) -> bool {
    let base_url = default_github_api_base_url(creds.enterprise_url.as_deref());
    let url = format!("{}/copilot_internal/user", base_url);

    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    headers.insert(USER_AGENT, HeaderValue::from_static(COPILOT_CLI_USER_AGENT));
    if let Ok(val) = HeaderValue::from_str(&format!("Bearer {}", creds.access)) {
        headers.insert(AUTHORIZATION, val);
    } else {
        return false;
    }

    match client.get(&url).headers(headers).send().await {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}

pub async fn discover_copilot_api_endpoint(
    client: &reqwest::Client,
    token: &str,
    enterprise_domain: Option<&str>,
) -> Option<String> {
    let base_url = default_github_api_base_url(enterprise_domain);
    let url = format!("{}/copilot_internal/user", base_url);

    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    headers.insert(USER_AGENT, HeaderValue::from_static(COPILOT_CLI_USER_AGENT));
    let auth_header = format!("token {}", token);
    if let Ok(val) = HeaderValue::from_str(&auth_header) {
        headers.insert(AUTHORIZATION, val);
    } else {
        return None;
    }

    let resp = client.get(&url).headers(headers).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }

    let json: serde_json::Value = resp.json().await.ok()?;
    let endpoint = json.get("endpoints")?.get("api")?.as_str()?;
    let trimmed = endpoint.trim().trim_end_matches('/');
    if trimmed.starts_with("https://") {
        Some(trimmed.to_string())
    } else {
        None
    }
}

pub async fn login_device_flow(
    client: &reqwest::Client,
    domain: &str,
) -> Result<CopilotOAuthData, String> {
    let clean_domain = if domain.trim().is_empty() {
        "github.com"
    } else {
        domain.trim()
    };

    let device_code_url = format!("https://{}/login/device/code", clean_domain);
    let access_token_url = format!("https://{}/login/oauth/access_token", clean_domain);

    let mut form = HashMap::new();
    form.insert("client_id", CLIENT_ID);
    form.insert("scope", "read:user");

    let resp = client
        .post(&device_code_url)
        .header(ACCEPT, "application/json")
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(USER_AGENT, "copilot-developer-action/0.0.1")
        .form(&form)
        .send()
        .await
        .map_err(|e| format!("Failed to request device code: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Device code request failed ({status}): {body}"));
    }

    let code_resp: DeviceCodeResponse = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse device code response: {}", e))?;

    println!("\nFirst copy your one-time code: {}", code_resp.user_code);
    println!("Then visit: {}", code_resp.verification_uri);
    println!("Waiting for authentication...\n");
    io::stdout().flush().ok();

    let deadline = std::time::Instant::now() + Duration::from_secs(code_resp.expires_in);
    let mut poll_interval = Duration::from_secs(code_resp.interval.max(1));

    let mut poll_form = HashMap::new();
    poll_form.insert("client_id", CLIENT_ID);
    poll_form.insert("device_code", &code_resp.device_code);
    poll_form.insert("grant_type", "urn:ietf:params:oauth:grant-type:device_code");

    while std::time::Instant::now() < deadline {
        tokio::time::sleep(poll_interval).await;

        let poll_resp = client
            .post(&access_token_url)
            .header(ACCEPT, "application/json")
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(USER_AGENT, "copilot-developer-action/0.0.1")
            .form(&poll_form)
            .send()
            .await
            .map_err(|e| format!("Poll access token request failed: {}", e))?;

        let body_bytes = poll_resp
            .bytes()
            .await
            .map_err(|e| format!("Failed to read response body: {}", e))?;

        if let Ok(success) = serde_json::from_slice::<AccessTokenSuccessResponse>(&body_bytes) {
            let enterprise_url = if clean_domain != "github.com" {
                Some(clean_domain.to_string())
            } else {
                None
            };

            let discovered_endpoint = discover_copilot_api_endpoint(
                client,
                &success.access_token,
                enterprise_url.as_deref(),
            )
            .await;

            let api_endpoint = discovered_endpoint
                .unwrap_or_else(|| get_github_copilot_base_url(enterprise_url.as_deref()));

            let far_future = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64
                + 10 * 365 * 24 * 3600 * 1000;

            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;

            return Ok(CopilotOAuthData {
                access: success.access_token.clone(),
                refresh: success.access_token,
                expires: far_future,
                api_endpoint: Some(api_endpoint),
                enterprise_url,
                authorized_at: Some(now_ms),
            });
        }

        if let Ok(err) = serde_json::from_slice::<AccessTokenErrorResponse>(&body_bytes) {
            match err.error.as_str() {
                "authorization_pending" => {
                    continue;
                }
                "slow_down" => {
                    let extra = err.interval.unwrap_or(5);
                    poll_interval += Duration::from_secs(extra);
                    continue;
                }
                "expired_token" => {
                    return Err("Device code expired. Please run the login again.".to_string());
                }
                "access_denied" => {
                    return Err("Login was cancelled or denied on GitHub.".to_string());
                }
                other => {
                    let desc = err.error_description.unwrap_or_default();
                    return Err(format!("Device login failed with {}: {}", other, desc));
                }
            }
        }
    }

    Err("Device flow timed out. Please try again.".to_string())
}
