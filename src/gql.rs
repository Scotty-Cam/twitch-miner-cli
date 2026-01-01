//! GQL client for Twitch API interactions.

use anyhow::{anyhow, Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::auth::AuthState;
use crate::constants::{gql_operations, ClientInfo, GqlOperation, CLIENT_ANDROID_APP};
use crate::models::{GqlRequest, GqlResponse};
use crate::utils::mask_proxy_url;

const GQL_URL: &str = "https://gql.twitch.tv/gql";

/// A client for making GQL requests to Twitch.
#[derive(Clone)]
pub struct GqlClient {
    client: reqwest::Client,
    client_info: ClientInfo,
    auth: AuthState,
    /// Stored unique_id cookie value
    unique_id: Option<String>,
    /// Cookies initialized flag
    cookies_initialized: bool,
    proxy_url: Option<String>,
}

impl GqlClient {
    /// Create a new GQL client with the given auth state.
    /// Uses Android app client by default to bypass integrity checks.
    pub fn new(auth: AuthState) -> Self {
        Self::with_client_info(auth, CLIENT_ANDROID_APP, None)
    }

    /// Create a new GQL client with the given auth state and proxy.
    pub fn new_with_proxy(auth: AuthState, proxy_url: Option<String>) -> Self {
        Self::with_client_info(auth, CLIENT_ANDROID_APP, proxy_url)
    }

    /// Create a new GQL client with custom client info.
    pub fn with_client_info(
        auth: AuthState,
        client_info: ClientInfo,
        proxy_url: Option<String>,
    ) -> Self {
        let mut builder = reqwest::Client::builder();

        // Add proxy if configured
        if let Some(ref url) = proxy_url {
            if let Ok(proxy) = reqwest::Proxy::all(url) {
                builder = builder.proxy(proxy);
                tracing::info!("GQL client using proxy: {}", mask_proxy_url(url));
            } else {
                tracing::warn!("Invalid proxy URL, ignoring: {}", mask_proxy_url(url));
            }
        }

        let client = builder.build().expect("Failed to build HTTP client");

        Self {
            client,
            client_info,
            auth,
            unique_id: None,
            cookies_initialized: false,
            proxy_url,
        }
    }

    /// Try to extract cookies from Python's cookies.jar file (pickle format).
    /// This is a simple parser that looks for the cookie values in the binary data.
    fn load_cookies_from_jar() -> Option<(String, String)> {
        let jar_paths = [
            "cookies.jar",
            "external_repos/cookies.jar",
            "external_repos/Twitch Drops Miner compiled/cookies.jar",
            "../TwitchDropsMiner/cookies.jar",
        ];

        for path in &jar_paths {
            if let Ok(data) = std::fs::read(path) {
                // Extract unique_id - look for "unique_id" followed by a 32-char value
                let unique_id = Self::extract_32char_value(&data, b"unique_id");
                // Extract auth-token - look for "auth-token" followed by a 30-char value
                let auth_token = Self::extract_auth_token(&data);

                if let (Some(uid), Some(token)) = (unique_id, auth_token) {
                    #[cfg(feature = "debug-gql")]
                    {
                        let _ = std::fs::write(
                            "cookies_debug.txt",
                            format!(
                                "Found from: {}\nunique_id: {}\nauth-token: {}",
                                path, uid, token
                            ),
                        );
                    }
                    return Some((uid, token));
                }
            }
        }
        None
    }

    /// Extract a 32-character alphanumeric value after a key in pickle data.
    fn extract_32char_value(data: &[u8], key: &[u8]) -> Option<String> {
        // Find the key in data
        for i in 0..data.len().saturating_sub(key.len() + 50) {
            if &data[i..i + key.len()] == key {
                // Look for the value after the key - it should be within next 100 bytes
                // Values are often preceded by 0x20 (space) in pickle format
                for j in
                    i + key.len()..std::cmp::min(i + key.len() + 100, data.len().saturating_sub(32))
                {
                    // Skip any non-alphanumeric bytes until we hit the value
                    if data[j] == b' ' && j + 1 < data.len() && j + 33 <= data.len() {
                        // This might be the space before the value
                        let potential_value = &data[j + 1..j + 33];
                        if potential_value.iter().all(|b| b.is_ascii_alphanumeric()) {
                            if let Ok(s) = std::str::from_utf8(potential_value) {
                                return Some(s.to_string());
                            }
                        }
                    }
                    // Also check directly for alphanumeric start
                    if data[j].is_ascii_alphanumeric() && j + 32 <= data.len() {
                        let potential_value = &data[j..j + 32];
                        if potential_value.iter().all(|b| b.is_ascii_alphanumeric()) {
                            if let Ok(s) = std::str::from_utf8(potential_value) {
                                return Some(s.to_string());
                            }
                        }
                    }
                }
            }
        }
        None
    }

    /// Extract auth-token (30-character lowercase alphanumeric).
    fn extract_auth_token(data: &[u8]) -> Option<String> {
        let key = b"auth-token";
        // Find the key in data
        for i in 0..data.len().saturating_sub(key.len() + 60) {
            if &data[i..i + key.len()] == key {
                // Look for the value after the key
                for j in
                    i + key.len()..std::cmp::min(i + key.len() + 100, data.len().saturating_sub(30))
                {
                    // Check for space followed by 30-char token
                    if data[j] == b' ' && j + 1 < data.len() && j + 31 <= data.len() {
                        let potential_value = &data[j + 1..j + 31];
                        if potential_value
                            .iter()
                            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
                        {
                            if let Ok(s) = std::str::from_utf8(potential_value) {
                                return Some(s.to_string());
                            }
                        }
                    }
                    // Also check directly
                    if (data[j].is_ascii_lowercase() || data[j].is_ascii_digit())
                        && j + 30 <= data.len()
                    {
                        let potential_value = &data[j..j + 30];
                        if potential_value
                            .iter()
                            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
                        {
                            if let Ok(s) = std::str::from_utf8(potential_value) {
                                return Some(s.to_string());
                            }
                        }
                    }
                }
            }
        }
        None
    }

    /// Initialize cookies by visiting Twitch (required for integrity checks).
    /// This fetches the unique_id cookie from Twitch and uses it for all subsequent requests.
    pub async fn init_cookies(&mut self) -> Result<()> {
        if self.cookies_initialized {
            return Ok(());
        }

        // First, try to load unique_id from Python's cookies.jar file
        if let Some((unique_id, _auth_token)) = Self::load_cookies_from_jar() {
            // Use the unique_id from Python's session
            self.auth.device_id = unique_id.clone();
            self.unique_id = Some(unique_id.clone());

            #[cfg(feature = "debug-gql")]
            {
                let _ = std::fs::write(
                    "cookies_debug.txt",
                    format!("Loaded unique_id from cookies.jar: {}", unique_id),
                );
            }
        } else {
            // Fall back: fetch unique_id from Twitch by making a request
            let response = self
                .client
                .get(self.client_info.client_url)
                .header(USER_AGENT, self.client_info.user_agent)
                .header("Accept", "text/html")
                .send()
                .await
                .map_err(|e| {
                    if self.proxy_url.is_some() && (e.is_connect() || e.is_timeout()) {
                        return anyhow!("Proxy connection failed during cookie init. Please check your settings. Details: {}", e);
                    }
                    anyhow!("Failed to fetch Twitch page: {}", e)
                })?;

            // Parse Set-Cookie headers manually to find unique_id
            let mut found_unique_id: Option<String> = None;
            for (name, value) in response.headers().iter() {
                if name.as_str().eq_ignore_ascii_case("set-cookie") {
                    if let Ok(cookie_str) = value.to_str() {
                        // Parse "unique_id=VALUE; ..." format
                        if let Some(stripped) = cookie_str.strip_prefix("unique_id=") {
                            if let Some(end) = stripped.find(';') {
                                found_unique_id = Some(stripped[..end].to_string());
                            } else {
                                found_unique_id = Some(stripped.to_string());
                            }
                            break;
                        }
                    }
                }
            }

            if let Some(uid) = found_unique_id {
                self.auth.device_id = uid.clone();
                self.unique_id = Some(uid.clone());
                #[cfg(feature = "debug-gql")]
                {
                    let _ = std::fs::write(
                        "cookies_debug.txt",
                        format!("Fetched unique_id from Twitch: {}", uid),
                    );
                }
            } else {
                // Use device_id as fallback
                self.unique_id = Some(self.auth.device_id.clone());
                #[cfg(feature = "debug-gql")]
                {
                    let _ = std::fs::write(
                        "cookies_debug.txt",
                        "Failed to get unique_id from Twitch - using existing device_id",
                    );
                }
            }
        }

        self.cookies_initialized = true;
        Ok(())
    }

    /// Build the Cookie header value for requests
    fn build_cookie_header(&self) -> String {
        let unique_id = self.unique_id.as_ref().unwrap_or(&self.auth.device_id);
        format!(
            "unique_id={}; auth-token={}",
            unique_id, self.auth.access_token
        )
    }

    /// Build the headers required for GQL requests.
    fn build_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();

        headers.insert("Accept", HeaderValue::from_static("*/*"));
        headers.insert("Accept-Encoding", HeaderValue::from_static("gzip"));
        headers.insert("Accept-Language", HeaderValue::from_static("en-US"));
        headers.insert("Pragma", HeaderValue::from_static("no-cache"));
        headers.insert("Cache-Control", HeaderValue::from_static("no-cache"));

        headers.insert(
            "Client-Id",
            HeaderValue::from_str(self.client_info.client_id).unwrap(),
        );
        headers.insert(
            USER_AGENT,
            HeaderValue::from_str(self.client_info.user_agent).unwrap(),
        );
        headers.insert(
            "X-Device-Id",
            HeaderValue::from_str(&self.auth.device_id).unwrap(),
        );
        // Add Client-Session-Id (required for some queries)
        headers.insert(
            "Client-Session-Id",
            HeaderValue::from_str(&self.auth.device_id[..16]).unwrap(),
        );
        headers.insert(
            "Origin",
            HeaderValue::from_str(self.client_info.client_url).unwrap(),
        );
        headers.insert(
            "Referer",
            HeaderValue::from_str(self.client_info.client_url).unwrap(),
        );
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("OAuth {}", self.auth.access_token)).unwrap(),
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        // Add cookies manually (since we removed the cookies feature)
        if let Ok(cookie_val) = HeaderValue::from_str(&self.build_cookie_header()) {
            headers.insert("Cookie", cookie_val);
        }

        headers
    }

    /// Execute a GQL query and parse the response.
    pub async fn query<T: DeserializeOwned>(
        &self,
        operation: &GqlOperation,
        variables: Option<Value>,
    ) -> Result<T> {
        let request_body = GqlRequest::new(operation, variables);
        let headers = self.build_headers();

        #[cfg(feature = "debug-gql")]
        {
            let request_json = serde_json::to_string_pretty(&request_body).unwrap_or_default();
            let _ = std::fs::write("gql_debug_request.json", &request_json);
        }

        let response = self
            .client
            .post(GQL_URL)
            .headers(headers)
            .json(&request_body)
            .send()
            .await
            .map_err(|e| {
                if self.proxy_url.is_some() && (e.is_connect() || e.is_timeout()) {
                    return anyhow!(
                        "Proxy connection failed. Please check your settings. Details: {}",
                        e
                    );
                }
                anyhow!("Failed to send GQL request: {}", e)
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            #[cfg(feature = "debug-gql")]
            {
                let _ = std::fs::write("gql_debug_error.json", &body);
            }
            return Err(anyhow!("GQL request failed: {} - {}", status, body));
        }

        let response_text = response.text().await.context("Failed to read response")?;
        #[cfg(feature = "debug-gql")]
        {
            let _ = std::fs::write("gql_debug_response.json", &response_text);
        }

        let gql_response: GqlResponse<T> =
            serde_json::from_str(&response_text).context("Failed to parse GQL response")?;

        if gql_response.has_errors() {
            let errors = gql_response.errors.unwrap();
            let error_msgs: Vec<_> = errors.iter().map(|e| e.message.as_str()).collect();
            return Err(anyhow!("GQL errors: {}", error_msgs.join(", ")));
        }

        gql_response
            .data
            .ok_or_else(|| anyhow!("GQL response missing data"))
    }

    /// Execute a raw GQL query and return the JSON value.
    pub async fn query_raw(
        &self,
        operation: &GqlOperation,
        variables: Option<Value>,
    ) -> Result<Value> {
        self.query(operation, variables).await
    }

    // =========================================================================
    // Convenience methods for common operations
    // =========================================================================

    /// Fetch the user's drops inventory (campaigns already opted in).
    pub async fn fetch_inventory(&self) -> Result<Value> {
        let result = self
            .query_raw(
                &gql_operations::INVENTORY,
                Some(serde_json::json!({"fetchRewardCampaigns": true})), // Fetch completed campaigns too
            )
            .await;

        #[cfg(feature = "debug-gql")]
        if let Ok(ref response) = result {
            let _ = std::fs::write(
                "gql_debug_inventory_response.json",
                serde_json::to_string_pretty(response).unwrap_or_default(),
            );
        }

        result
    }

    /// Fetch ALL available campaigns (Viewer Drops Dashboard).
    pub async fn fetch_all_campaigns(&self) -> Result<Value> {
        let result = self
            .query_raw(
                &gql_operations::CAMPAIGNS,
                Some(serde_json::json!({"fetchRewardCampaigns": false})),
            )
            .await;

        #[cfg(feature = "debug-gql")]
        if let Ok(ref response) = result {
            let _ = std::fs::write(
                "gql_debug_campaigns_response.json",
                serde_json::to_string_pretty(response).unwrap_or_default(),
            );
        }

        result
    }

    /// Get current drop progress for a channel.
    pub async fn get_current_drop(&self, channel_id: &str, _channel_login: &str) -> Result<Value> {
        // Use Android client for drops to match Auth Token (bypasses integrity)
        let client = Self::with_client_info(
            self.auth.clone(),
            CLIENT_ANDROID_APP,
            self.proxy_url.clone(),
        );

        client
            .query_raw(
                &gql_operations::CURRENT_DROP,
                Some(serde_json::json!({
                    "channelID": channel_id,
                    "channelLogin": "" // Android client expects empty string here
                })),
            )
            .await
    }

    /// Claim a drop reward.
    pub async fn claim_drop(&self, drop_instance_id: &str) -> Result<Value> {
        self.query_raw(
            &gql_operations::CLAIM_DROP,
            Some(serde_json::json!({
                "input": {
                    "dropInstanceID": drop_instance_id
                }
            })),
        )
        .await
    }

    /// Get playback access token for a channel.
    pub async fn get_playback_token(&self, channel_login: &str) -> Result<Value> {
        // Default using Android client (from new()) is already correct for bypassing integrity
        self.query_raw(
            &gql_operations::PLAYBACK_ACCESS_TOKEN,
            Some(serde_json::json!({
                "isLive": true,
                "isVod": false,
                "login": channel_login,
                "platform": "android",
                "playerType": "channel_home_live",
                "vodID": ""
            })),
        )
        .await
    }

    /// Get live channels for a game.
    pub async fn get_game_directory(&self, game_slug: &str, limit: u32) -> Result<Value> {
        self.query_raw(
            &gql_operations::GAME_DIRECTORY,
            Some(serde_json::json!({
                "limit": limit,
                "slug": game_slug,
                "imageWidth": 50,
                "includeCostreaming": false,
                "options": {
                    "broadcasterLanguages": [],
                    "freeformTags": null,
                    "includeRestricted": ["SUB_ONLY_LIVE"],
                    "recommendationsContext": {"platform": "web"},
                    "sort": "RELEVANCE",
                    "systemFilters": [],
                    "tags": [],
                    "requestID": "JIRA-VXP-2397"
                },
                "sortTypeIsRecency": false
            })),
        )
        .await
    }

    /// Fetch detailed information about a specific campaign (including drops progress).
    pub async fn fetch_campaign_details(
        &self,
        campaign_id: &str,
        channel_login: Option<&str>,
    ) -> Result<Value> {
        let channel_login = channel_login.unwrap_or("");

        // Try using different variable names based on typical Twitch GQL patterns
        let result = self
            .query_raw(
                &gql_operations::CAMPAIGN_DETAILS,
                Some(serde_json::json!({
                    "dropID": campaign_id,
                    "channelLogin": channel_login
                })),
            )
            .await;

        #[cfg(feature = "debug-gql")]
        if let Ok(ref response) = result {
            let sanitized_id = campaign_id.chars().take(8).collect::<String>();
            // Only write if user is null (error case) to avoid spam
            if response.get("user").is_none() || response["user"].is_null() {
                let _ = std::fs::write(
                    format!("debug_campaign_{}_err.json", sanitized_id),
                    serde_json::to_string_pretty(response).unwrap_or_default(),
                );
            }
        }

        result
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_auth() -> AuthState {
        AuthState {
            access_token: "test_token_12345".to_string(),
            user_id: 12345678,
            device_id: "abcdef1234567890abcdef1234567890".to_string(),
            login: "testuser".to_string(),
        }
    }

    #[test]
    fn test_headers_contain_required_fields() {
        let client = GqlClient::new(mock_auth());
        let headers = client.build_headers();

        assert!(headers.contains_key("Client-Id"));
        assert!(headers.contains_key(USER_AGENT));
        assert!(headers.contains_key(AUTHORIZATION));
        assert!(headers.contains_key("X-Device-Id"));
        assert!(headers.contains_key("Origin"));
        assert!(headers.contains_key("Referer"));
    }

    #[test]
    fn test_authorization_header_format() {
        let auth = mock_auth();
        let client = GqlClient::new(auth.clone());
        let headers = client.build_headers();

        let auth_header = headers.get(AUTHORIZATION).unwrap().to_str().unwrap();
        assert_eq!(auth_header, format!("OAuth {}", auth.access_token));
    }

    #[test]
    fn test_client_id_header() {
        use crate::constants::CLIENT_ANDROID_APP;
        let client = GqlClient::new(mock_auth());
        let headers = client.build_headers();

        let client_id = headers.get("Client-Id").unwrap().to_str().unwrap();
        assert_eq!(client_id, CLIENT_ANDROID_APP.client_id);
    }

    #[test]
    fn test_gql_request_body_structure() {
        let request = GqlRequest::new(
            &gql_operations::INVENTORY,
            Some(serde_json::json!({"fetchRewardCampaigns": false})),
        );
        let json = serde_json::to_value(&request).unwrap();

        assert_eq!(json["operationName"], "Inventory");
        assert!(
            json["extensions"]["persistedQuery"]["sha256Hash"]
                .as_str()
                .unwrap()
                .len()
                == 64
        );
        assert_eq!(json["variables"]["fetchRewardCampaigns"], false);
    }
}
