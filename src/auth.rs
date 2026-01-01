//! Authentication module for Twitch API access.
//!
//! Implements the Device Code Flow for user authentication.

use std::path::Path;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use tokio::fs;
use tokio::time::sleep;

use crate::constants::{ClientInfo, CLIENT_ANDROID_APP};

// =============================================================================
// Token Storage
// =============================================================================

/// Stored authentication state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthState {
    pub access_token: String,
    pub user_id: u64,
    pub device_id: String,
    pub login: String,
}

impl AuthState {
    /// Save auth state to a JSON file.
    pub async fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json).await?;
        Ok(())
    }

    /// Load auth state from a JSON file.
    pub async fn load(path: impl AsRef<Path>) -> Result<Self> {
        let contents = fs::read_to_string(path).await?;
        let state: Self = serde_json::from_str(&contents)?;
        Ok(state)
    }
}

// =============================================================================
// Device Code Flow
// =============================================================================

/// Response from the device code request.
#[derive(Debug, Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    interval: u64,
}

/// Response from the token request.
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[allow(dead_code)]
    #[serde(default)]
    refresh_token: Option<String>,
}

/// Response from the validate endpoint.
#[derive(Debug, Deserialize)]
struct ValidateResponse {
    user_id: String,
    login: String,
}

/// Authenticator using Device Code Flow.
pub struct DeviceAuthenticator {
    client: reqwest::Client,
    client_info: ClientInfo,
    device_id: String,
    proxy_url: Option<String>,
}

impl DeviceAuthenticator {
    /// Create a new authenticator with Android client (bypasses integrity checks).
    pub fn new() -> Self {
        Self::with_client_info(CLIENT_ANDROID_APP, None)
    }

    /// Create a new authenticator with proxy support.
    pub fn new_with_proxy(proxy_url: Option<String>) -> Self {
        Self::with_client_info(CLIENT_ANDROID_APP, proxy_url)
    }

    /// Create a new authenticator with custom client info.
    pub fn with_client_info(client_info: ClientInfo, proxy_url: Option<String>) -> Self {
        // Generate a placeholder device_id - will be replaced by init()
        let device_id = generate_device_id();

        let mut builder = reqwest::Client::builder();

        if let Some(ref url) = proxy_url {
            if let Ok(proxy) = reqwest::Proxy::all(url) {
                builder = builder.proxy(proxy);
                tracing::info!("Auth using proxy");
            }
        }

        Self {
            client: builder.build().expect("Failed to build HTTP client"),
            client_info,
            device_id,
            proxy_url,
        }
    }

    /// Initialize by fetching unique_id from Twitch.
    /// This MUST be called before authenticate() for proper integrity check handling.
    pub async fn init(&mut self) -> Result<()> {
        // Fetch Twitch page to get unique_id cookie
        let response = self
            .client
            .get(self.client_info.client_url)
            .header("User-Agent", self.client_info.user_agent)
            .header("Accept", "text/html,application/xhtml+xml")
            .send()
            .await
            .map_err(|e| {
                if self.proxy_url.is_some() && (e.is_connect() || e.is_timeout()) {
                    return anyhow!(
                        "Proxy connection failed during auth init. Check settings. Details: {}",
                        e
                    );
                }
                anyhow!("Failed to fetch Twitch page for unique_id: {}", e)
            })?;

        // Look for unique_id in Set-Cookie headers
        for (name, value) in response.headers().iter() {
            if name.as_str().eq_ignore_ascii_case("set-cookie") {
                if let Ok(cookie_str) = value.to_str() {
                    if let Some(stripped) = cookie_str.strip_prefix("unique_id=") {
                        let end = stripped.find(';').unwrap_or(stripped.len());
                        self.device_id = stripped[..end].to_string();
                        tracing::info!("Got unique_id from Twitch: {}", self.device_id);
                        return Ok(());
                    }
                }
            }
        }

        // If no unique_id found, log warning but continue with generated one
        tracing::warn!("Could not get unique_id from Twitch, using generated device_id");
        Ok(())
    }

    /// Perform the Device Code Flow authentication.
    ///
    /// Returns a callback with the user code and verification URI,
    /// then waits for the user to authenticate.
    pub async fn authenticate<F>(&self, on_code: F) -> Result<AuthState>
    where
        F: FnOnce(&str, &str),
    {
        // Step 1: Request device code
        let device_response = self.request_device_code().await?;

        // Step 2: Show code to user
        on_code(
            &device_response.user_code,
            &device_response.verification_uri,
        );

        // Step 3: Poll for token
        let access_token = self
            .poll_for_token(
                &device_response.device_code,
                device_response.interval,
                device_response.expires_in,
            )
            .await?;

        // Step 4: Validate token and get user info
        let validate_response = self.validate_token(&access_token).await?;

        Ok(AuthState {
            access_token,
            user_id: validate_response
                .user_id
                .parse()
                .context("Invalid user_id")?,
            device_id: self.device_id.clone(),
            login: validate_response.login,
        })
    }

    /// Perform the Device Code Flow authentication using async channel.
    ///
    /// Sends the code and URI via the provided channel, then polls for token.
    pub async fn authenticate_async(
        &self,
        tx: tokio::sync::mpsc::Sender<crate::LoginMessage>,
    ) -> Result<AuthState> {
        // Step 1: Request device code
        let device_response = self.request_device_code().await?;

        // Step 2: Send code to UI via channel
        let _ = tx
            .send(crate::LoginMessage::CodeReady {
                code: device_response.user_code.clone(),
                uri: device_response.verification_uri.clone(),
            })
            .await;

        // Step 3: Poll for token
        let access_token = self
            .poll_for_token(
                &device_response.device_code,
                device_response.interval,
                device_response.expires_in,
            )
            .await?;

        // Step 4: Validate token and get user info
        let validate_response = self.validate_token(&access_token).await?;

        Ok(AuthState {
            access_token,
            user_id: validate_response
                .user_id
                .parse()
                .context("Invalid user_id")?,
            device_id: self.device_id.clone(),
            login: validate_response.login,
        })
    }

    /// Request a device code from Twitch.
    async fn request_device_code(&self) -> Result<DeviceCodeResponse> {
        let response = self
            .client
            .post("https://id.twitch.tv/oauth2/device")
            .header("Accept", "application/json")
            .header("Accept-Encoding", "gzip")
            .header("Accept-Language", "en-US")
            .header("Cache-Control", "no-cache")
            .header("Client-Id", self.client_info.client_id)
            .header("Host", "id.twitch.tv")
            .header("Origin", self.client_info.client_url)
            .header("Pragma", "no-cache")
            .header("Referer", self.client_info.client_url)
            .header("User-Agent", self.client_info.user_agent)
            .header("X-Device-Id", &self.device_id)
            .form(&[("client_id", self.client_info.client_id), ("scopes", "")])
            .send()
            .await
            .map_err(|e| {
                if self.proxy_url.is_some() && (e.is_connect() || e.is_timeout()) {
                    return anyhow!("Proxy connection failed requesting device code. Check settings. Details: {}", e);
                }
                anyhow!("Failed to request device code: {}", e)
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("Device code request failed: {} - {}", status, body));
        }

        response
            .json()
            .await
            .context("Failed to parse device code response")
    }

    /// Poll for the access token after user authenticates.
    async fn poll_for_token(
        &self,
        device_code: &str,
        interval: u64,
        expires_in: u64,
    ) -> Result<String> {
        let poll_interval = Duration::from_secs(interval);
        let max_attempts = expires_in / interval;

        for attempt in 0..max_attempts {
            sleep(poll_interval).await;

            let response = self
                .client
                .post("https://id.twitch.tv/oauth2/token")
                .header("Accept", "application/json")
                .header("Accept-Encoding", "gzip")
                .header("Client-Id", self.client_info.client_id)
                .header("User-Agent", self.client_info.user_agent)
                .header("X-Device-Id", &self.device_id)
                .form(&[
                    ("client_id", self.client_info.client_id),
                    ("device_code", device_code),
                    (
                        "grant_type",
                        "urn:ietf:params:oauth:grant-type:device_code",
                    ),
                ])
                .send()
                .await
                .map_err(|e| {
                    if self.proxy_url.is_some() && (e.is_connect() || e.is_timeout()) {
                        return anyhow!("Proxy connection failed polling for token. Check settings. Details: {}", e);
                    }
                    anyhow!("Failed to poll for token: {}", e)
                })?;

            if response.status().is_success() {
                let token_response: TokenResponse = response
                    .json()
                    .await
                    .context("Failed to parse token response")?;
                return Ok(token_response.access_token);
            }

            // 400 means user hasn't authenticated yet, continue polling
            if response.status().as_u16() != 400 {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                return Err(anyhow!("Token request failed: {} - {}", status, body));
            }

            tracing::debug!(
                "Waiting for user authentication... (attempt {}/{})",
                attempt + 1,
                max_attempts
            );
        }

        Err(anyhow!("Device code expired before user authenticated"))
    }

    /// Validate an access token and get user info.
    async fn validate_token(&self, access_token: &str) -> Result<ValidateResponse> {
        let response = self
            .client
            .get("https://id.twitch.tv/oauth2/validate")
            .header("Authorization", format!("OAuth {}", access_token))
            .send()
            .await
            .map_err(|e| {
                if self.proxy_url.is_some() && (e.is_connect() || e.is_timeout()) {
                    return anyhow!(
                        "Proxy connection failed validating token. Check settings. Details: {}",
                        e
                    );
                }
                anyhow!("Failed to validate token: {}", e)
            })?;

        if !response.status().is_success() {
            return Err(anyhow!("Token validation failed: {}", response.status()));
        }

        response
            .json()
            .await
            .context("Failed to parse validate response")
    }
}

impl Default for DeviceAuthenticator {
    fn default() -> Self {
        Self::new()
    }
}

/// Generate a random device ID (32 hex characters).
fn generate_device_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{:032x}", timestamp)
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_id_generation() {
        let id1 = generate_device_id();
        let id2 = generate_device_id();

        assert_eq!(id1.len(), 32);
        assert!(id1.chars().all(|c| c.is_ascii_hexdigit()));
        // IDs should be different (time-based)
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_auth_state_serialization() {
        let state = AuthState {
            access_token: "test_token".to_string(),
            user_id: 12345678,
            device_id: "abcdef1234567890".to_string(),
            login: "testuser".to_string(),
        };

        let json = serde_json::to_string(&state).unwrap();
        let parsed: AuthState = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.access_token, "test_token");
        assert_eq!(parsed.user_id, 12345678);
        assert_eq!(parsed.login, "testuser");
    }

    #[tokio::test]
    async fn test_auth_state_save_load() {
        let state = AuthState {
            access_token: "test_token".to_string(),
            user_id: 12345678,
            device_id: "abcdef1234567890".to_string(),
            login: "testuser".to_string(),
        };

        let temp_path = std::env::temp_dir().join("test_auth_state.json");

        state.save(&temp_path).await.unwrap();
        let loaded = AuthState::load(&temp_path).await.unwrap();

        assert_eq!(loaded.access_token, state.access_token);
        assert_eq!(loaded.user_id, state.user_id);

        // Cleanup
        let _ = fs::remove_file(&temp_path).await;
    }
}
