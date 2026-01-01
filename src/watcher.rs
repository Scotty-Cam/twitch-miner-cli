//! Watcher module for simulating stream viewing.
//!
//! Implements the "minute-watched" spade payload and watch pulse mechanism.

use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use regex_lite::Regex;
use serde::Serialize;
use serde_json;
use std::sync::LazyLock;

use crate::auth::AuthState;
use crate::constants::{CLIENT_ANDROID_APP, CLIENT_WEB};
use crate::gql::GqlClient;
use tokio::sync::mpsc::UnboundedSender;
use tokio::time::{sleep, Duration};

// Lazy-compiled regex patterns - compiled once at first use, reused forever
static SPADE_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#""beacon_?url": ?"(https://video-edge-[\.\w\-/]+\.ts(?:\?allow_stream=true)?)""#)
        .expect("Invalid spade pattern regex")
});

static SETTINGS_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"src="(https://[\w\.]+/config/settings\.[0-9a-f]{32}\.js)""#)
        .expect("Invalid settings pattern regex")
});

#[derive(Debug, Clone)]
pub struct MiningStatus {
    pub channel_login: String,
    pub game_name: String,
    pub drop_name: String,
    pub progress_percent: f32,
    pub minutes_watched: i32,
    pub minutes_required: i32,
}

pub enum WatcherEvent {
    Status(MiningStatus),
    /// Transient errors that can be retried (network glitches, rate limiting, token refresh failures)
    TransientError(String),
    /// Fatal errors that require stopping (channel offline, no drops available)
    FatalError(String),
    Claimed(String), // Drop name
    /// All drops in current campaign are complete - stop watching this game
    CampaignComplete(String), // Game name
}

/// The spade payload sent to simulate watching.
#[derive(Debug, Clone, Serialize)]
struct SpadeEvent {
    event: &'static str,
    properties: SpadeProperties,
}

#[derive(Debug, Clone, Serialize)]
struct SpadeProperties {
    broadcast_id: String,
    channel_id: String,
    channel: String,
    hidden: bool,
    live: bool,
    location: &'static str,
    logged_in: bool,
    muted: bool,
    player: &'static str,
    user_id: u64,
}

/// Stream info needed for watching.
#[derive(Debug, Clone)]
pub struct WatchTarget {
    pub channel_id: String,
    pub channel_login: String,
    pub broadcast_id: String,
    pub spade_url: String,
    pub token: String,
    pub sig: String,
}

/// The watcher that sends "minute-watched" pulses.
#[derive(Clone)]
pub struct Watcher {
    client: reqwest::Client,
    auth: AuthState,
    proxy_url: Option<String>,
}

impl Watcher {
    /// Create a new watcher.
    pub fn new(auth: AuthState) -> Self {
        Self::new_with_proxy(auth, None)
    }

    /// Create a new watcher with optional proxy.
    pub fn new_with_proxy(auth: AuthState, proxy_url: Option<String>) -> Self {
        let mut builder = reqwest::Client::builder();

        if let Some(ref url) = proxy_url {
            if let Ok(proxy) = reqwest::Proxy::all(url) {
                builder = builder.proxy(proxy);
                tracing::info!("Watcher using proxy");
            }
        }

        Self {
            client: builder.build().expect("Failed to build HTTP client"),
            auth,
            proxy_url,
        }
    }

    /// Generate the spade payload for a watch target.
    pub fn generate_payload(&self, target: &WatchTarget) -> String {
        let event = SpadeEvent {
            event: "minute-watched",
            properties: SpadeProperties {
                broadcast_id: target.broadcast_id.clone(),
                channel_id: target.channel_id.clone(),
                channel: target.channel_login.clone(),
                hidden: false,
                live: true,
                location: "channel",
                logged_in: true,
                muted: false,
                player: "site",
                user_id: self.auth.user_id,
            },
        };

        let events = vec![event];
        let json = serde_json::to_string(&events).unwrap();
        let encoded = BASE64.encode(json.as_bytes());
        encoded
    }

    /// Send a watch pulse to the spade URL.
    pub async fn send_pulse(&self, target: &WatchTarget) -> Result<bool> {
        let payload = self.generate_payload(target);
        let body = format!("data={}", payload);

        let response = self
            .client
            .post(&target.spade_url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("User-Agent", CLIENT_ANDROID_APP.user_agent)
            .header("Client-Id", CLIENT_ANDROID_APP.client_id)
            .header("X-Device-Id", &self.auth.device_id)
            .body(body)
            .send()
            .await
            .map_err(|e| {
                if self.proxy_url.is_some() && (e.is_connect() || e.is_timeout()) {
                    return anyhow!(
                        "Proxy connection failed during pulse. Check settings. Details: {}",
                        e
                    );
                }
                anyhow!("Failed to send watch pulse: {}", e)
            })?;

        // 204 No Content is the expected success response
        Ok(response.status().as_u16() == 204)
    }

    /// Extract the spade URL from channel HTML.
    pub async fn fetch_spade_url(&self, channel_login: &str) -> Result<String> {
        let url = format!("https://www.twitch.tv/{}", channel_login);

        let response = self
            .client
            .get(&url)
            .header("User-Agent", CLIENT_WEB.user_agent)
            .send()
            .await
            .map_err(|e| {
                if self.proxy_url.is_some() && (e.is_connect() || e.is_timeout()) {
                    return anyhow!("Proxy connection failed fetching channel page. Check settings. Details: {}", e);
                }
                anyhow!("Failed to fetch channel page: {}", e)
            })?;

        let html = response
            .text()
            .await
            .context("Failed to read channel HTML")?;

        // Try to find spade URL directly (mobile view) - uses lazy-compiled pattern
        if let Some(captures) = SPADE_PATTERN.captures(&html) {
            return Ok(captures.get(1).unwrap().as_str().to_string());
        }

        // Try to find settings.js URL and extract from there - uses lazy-compiled pattern

        if let Some(captures) = SETTINGS_PATTERN.captures(&html) {
            let settings_url = captures.get(1).unwrap().as_str();

            let settings_response = self
                .client
                .get(settings_url)
                .header("User-Agent", CLIENT_WEB.user_agent)
                .send()
                .await
                .context("Failed to fetch settings.js")?;

            let settings_js = settings_response
                .text()
                .await
                .context("Failed to read settings.js")?;

            if let Some(captures) = SPADE_PATTERN.captures(&settings_js) {
                return Ok(captures.get(1).unwrap().as_str().to_string());
            }
        }

        Err(anyhow!("Could not extract spade URL from channel page"))
    }

    /// Fetch the HLS playlist from Usher to simulate viewing.
    pub async fn fetch_hls_playlist(&self, target: &WatchTarget) -> Result<()> {
        let url = format!(
            "https://usher.ttvnw.net/api/channel/hls/{}.m3u8",
            target.channel_login
        );

        let params = [
            ("token", target.token.as_str()),
            ("sig", target.sig.as_str()),
            ("allow_source", "true"),
            ("allow_audio_only", "true"),
            ("fast_bread", "true"),
        ];

        let _ = self
            .client
            .get(&url)
            .query(&params)
            .header("User-Agent", CLIENT_ANDROID_APP.user_agent)
            .send()
            .await
            .map_err(|e| {
                if self.proxy_url.is_some() && (e.is_connect() || e.is_timeout()) {
                    return anyhow!(
                        "Proxy connection failed fetching playlist. Check settings. Details: {}",
                        e
                    );
                }
                anyhow!("Failed to fetch HLS playlist: {}", e)
            })?;

        Ok(())
    }
}

/// The main mining loop.
pub async fn mine_loop(
    gql: GqlClient,
    watcher: Watcher,
    channel_login: String,
    channel_id: String,
    broadcast_id: String, // NEW: Stream ID
    game_name: String,
    tx: UnboundedSender<WatcherEvent>,
) -> Result<()> {
    // Initial fetch of token
    let mut token_val = String::new();
    let mut sig_val = String::new();

    // Persistence state
    let mut consecutive_loop_failures = 0;
    let mut has_mined_once = false;
    let mut last_claimed_drop: Option<String> = None;

    // Fetch spade_url once (it shouldn't change often)
    let spade_url = match watcher.fetch_spade_url(&channel_login).await {
        Ok(s) => s,
        Err(e) => {
            // Fallback or error? Fallback to generic might work for some
            tracing::warn!("Failed to fetch spade URL: {}", e);
            format!("https://video-edge-{}.twitch.tv/hls", channel_login) // weak fallback
        }
    };

    loop {
        // Refresh token with retry logic (up to 3 attempts)
        let mut token_acquired = false;
        for token_retry in 1..=3 {
            match gql.get_playback_token(&channel_login).await {
                Ok(resp) => {
                    if let Some(token_obj) = resp.get("streamPlaybackAccessToken") {
                        if let Some(val) = token_obj.get("value").and_then(|v| v.as_str()) {
                            token_val = val.to_string();
                        }
                        if let Some(sig) = token_obj.get("signature").and_then(|v| v.as_str()) {
                            sig_val = sig.to_string();
                        }
                        if !token_val.is_empty() && !sig_val.is_empty() {
                            token_acquired = true;
                            break;
                        }
                    }
                    // Token structure unexpected - retry
                    tracing::warn!(
                        "[TOKEN_RETRY] Attempt {}/3: Token response structure unexpected",
                        token_retry
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        "[TOKEN_RETRY] Attempt {}/3: GQL request failed: {}",
                        token_retry,
                        e
                    );
                }
            }
            // Exponential backoff before retry (5s, 10s)
            if token_retry < 3 {
                sleep(Duration::from_secs(5 * token_retry as u64)).await;
            }
        }

        // Only report transient error after all retries exhausted
        if !token_acquired && token_val.is_empty() {
            if tx
                .send(WatcherEvent::TransientError(
                    "Token refresh failed after 3 attempts".to_string(),
                ))
                .is_err()
            {
                return Ok(());
            }
            // Continue loop - will retry next cycle
            sleep(Duration::from_secs(60)).await;
            continue;
        }

        if !token_val.is_empty() && !sig_val.is_empty() {
            let target = WatchTarget {
                channel_id: channel_id.clone(),
                channel_login: channel_login.clone(),
                broadcast_id: broadcast_id.clone(),
                spade_url: spade_url.clone(),
                token: token_val.clone(),
                sig: sig_val.clone(),
            };

            // 1. Fetch HLS (Connect to stream first)
            if let Err(e) = watcher.fetch_hls_playlist(&target).await {
                tracing::warn!("HLS Fetch warning: {}", e);
            }

            // Short sleep to simulate buffer time / connection establishment
            // Reduced to 0.5s for speed
            sleep(Duration::from_millis(500)).await;

            // 2. Send Watch Pulse (Spade) - CRITICAL for Drops
            if let Err(e) = watcher.send_pulse(&target).await {
                tracing::warn!("Pulse failed: {}", e);
            }

            // 3. Rapid Retry Drop Check (Wait for propagation)
            let mut drop_found_in_cycle = false;

            // Retries: 5 times spaced 2s. Fast detection.
            for retry in 1..=5 {
                if retry > 1 {
                    tracing::info!("Waiting for drop context update ({}/5)...", retry);
                    sleep(Duration::from_secs(2)).await;
                } else {
                    sleep(Duration::from_millis(1500)).await;
                }

                match gql.get_current_drop(&channel_id, "").await {
                    Ok(resp) => {
                        // Response is ALREADY valid 'data' object (unwrapped by gql.rs)

                        // Android GQL structure: currentUser -> dropCurrentSession
                        let has_context = resp
                            .get("currentUser")
                            .and_then(|u| u.get("dropCurrentSession"))
                            .is_some();

                        // Fallback check (Web structure)
                        let has_context = has_context || resp.get("currentSession").is_some();
                        // Old fallback
                        let has_context = has_context
                            || resp
                                .get("user")
                                .and_then(|u| u.get("dropCurrentSessionContext"))
                                .is_some();

                        tracing::info!("Drop Check #{}: Context Found = {}", retry, has_context);

                        if has_context {
                            if let Some(drop_ctx) = resp
                                .get("currentUser")
                                .and_then(|u| u.get("dropCurrentSession"))
                                .or_else(|| resp.get("currentSession"))
                                .or_else(|| {
                                    resp.get("user")
                                        .and_then(|u| u.get("dropCurrentSessionContext"))
                                })
                            {
                                // Polymorphic Parsing:
                                // Web: drop_ctx -> drop -> self
                                // Android: drop_ctx (flat)

                                let drop_node = drop_ctx.get("drop").unwrap_or(drop_ctx);
                                let self_node = drop_node.get("self").unwrap_or(drop_node);

                                // Check if we have valid data (at least required minutes > 0 or a valid ID)
                                let required = drop_node
                                    .get("requiredMinutesWatched")
                                    .and_then(|v| v.as_i64())
                                    .unwrap_or(0)
                                    as i32;
                                let id_check = drop_node
                                    .get("id")
                                    .or_else(|| self_node.get("dropInstanceID"))
                                    .is_some();

                                if required > 0 || id_check {
                                    // Try multiple ways to get the drop name
                                    let name = drop_node
                                        .get("name")
                                        .and_then(|n| n.as_str())
                                        .or_else(|| {
                                            drop_ctx.get("dropName").and_then(|n| n.as_str())
                                        })
                                        .or_else(|| {
                                            self_node.get("dropName").and_then(|n| n.as_str())
                                        })
                                        .or_else(|| {
                                            drop_node
                                                .get("benefitEdges")
                                                .and_then(|e| e.as_array())
                                                .and_then(|arr| arr.first())
                                                .and_then(|b| b.get("benefit"))
                                                .and_then(|b| b.get("name"))
                                                .and_then(|n| n.as_str())
                                        })
                                        .unwrap_or("Active Drop")
                                        .to_string();

                                    // Debug: log what we're seeing if name is fallback
                                    if name == "Active Drop" {
                                        tracing::debug!(
                                            "[DROP_DEBUG] drop_node keys: {:?}",
                                            drop_node
                                        );
                                    }

                                    let current = self_node
                                        .get("currentMinutesWatched")
                                        .and_then(|v| v.as_i64())
                                        .unwrap_or(0)
                                        as i32;
                                    let is_claimed = self_node
                                        .get("isClaimed")
                                        .and_then(|v| v.as_bool())
                                        .unwrap_or(false);

                                    // Skip already-claimed drops - wait for API to return next drop
                                    if is_claimed {
                                        tracing::info!("[DROP_CLAIMED] {} is already claimed, waiting for next drop context...", name);

                                        // RECOVERY LOGIC: If we were mining this drop and it's now claimed,
                                        // but we missed the claim event (e.g. network failure on claim_drop),
                                        // track it and notify anyway to ensure UI updates and notifications happen.
                                        if has_mined_once
                                            && last_claimed_drop.as_ref() != Some(&name)
                                        {
                                            tracing::info!("[DROP_RECOVERY] Detected claimed status for '{}' without prior event. Sending Claimed event.", name);
                                            if tx.send(WatcherEvent::Claimed(name.clone())).is_err()
                                            {
                                                return Ok(());
                                            }
                                            last_claimed_drop = Some(name.clone());
                                        }

                                        // IMPORTANT: Mark as found so we don't trigger FatalError
                                        // The API is working, just returning cached claimed drop
                                        drop_found_in_cycle = true;
                                        has_mined_once = true; // We've successfully mined before
                                        break; // Exit retry loop - we'll check again next 60s cycle
                                    }

                                    let percent = if required > 0 {
                                        current as f32 / required as f32 * 100.0
                                    } else {
                                        0.0
                                    };

                                    let status = MiningStatus {
                                        channel_login: channel_login.clone(),
                                        game_name: game_name.clone(),
                                        drop_name: name.clone(),
                                        progress_percent: percent,
                                        minutes_watched: current,
                                        minutes_required: required,
                                    };

                                    if tx.send(WatcherEvent::Status(status)).is_err() {
                                        return Ok(());
                                    }
                                    drop_found_in_cycle = true;

                                    // Attempt claim if drop is ready (100% but not yet claimed)
                                    if current >= required && required > 0 {
                                        if let Some(instance_id) = self_node
                                            .get("dropInstanceID")
                                            .or_else(|| drop_node.get("id"))
                                            .and_then(|v| v.as_str())
                                        {
                                            tracing::info!(
                                                "[DROP_CLAIM] Attempting to claim: {} ({})",
                                                name,
                                                instance_id
                                            );
                                            match gql.claim_drop(instance_id).await {
                                                Ok(_) => {
                                                    if tx
                                                        .send(WatcherEvent::Claimed(name.clone()))
                                                        .is_err()
                                                    {
                                                        return Ok(());
                                                    }
                                                    last_claimed_drop = Some(name.clone());
                                                    tracing::info!(
                                                        "[DROP_CLAIM] Successfully claimed: {}",
                                                        name
                                                    );

                                                    // After claiming, wait briefly for API propagation
                                                    // then check if there are more drops
                                                    sleep(Duration::from_secs(3)).await;

                                                    // Quick check for next drop
                                                    match gql
                                                        .get_current_drop(&channel_id, "")
                                                        .await
                                                    {
                                                        Ok(next_resp) => {
                                                            let next_ctx = next_resp
                                                                .get("currentUser")
                                                                .and_then(|u| {
                                                                    u.get("dropCurrentSession")
                                                                })
                                                                .or_else(|| {
                                                                    next_resp.get("currentSession")
                                                                });

                                                            let has_next_unclaimed = next_ctx
                                                                .and_then(|ctx| {
                                                                    let drop_node = ctx.get("drop").unwrap_or(ctx);
                                                                    let self_node = drop_node.get("self").unwrap_or(drop_node);
                                                                    let is_claimed = self_node
                                                                        .get("isClaimed")
                                                                        .and_then(|v| v.as_bool())
                                                                        .unwrap_or(false);
                                                                    let req = drop_node
                                                                        .get("requiredMinutesWatched")
                                                                        .and_then(|v| v.as_i64())
                                                                        .unwrap_or(0);
                                                                    // Has next if: context exists, not claimed, and has required time
                                                                    if !is_claimed && req > 0 {
                                                                        Some(true)
                                                                    } else {
                                                                        None
                                                                    }
                                                                })
                                                                .unwrap_or(false);

                                                            if has_next_unclaimed {
                                                                tracing::info!("[DROP_NEXT] Found next unclaimed drop, continuing...");
                                                                // Continue the retry loop to get the next drop status
                                                                continue;
                                                            } else {
                                                                // No more drops - campaign complete!
                                                                tracing::info!("[CAMPAIGN_COMPLETE] No more unclaimed drops for {}", game_name);
                                                                let _ = tx.send(
                                                                    WatcherEvent::CampaignComplete(
                                                                        game_name.clone(),
                                                                    ),
                                                                );
                                                                return Ok(());
                                                            }
                                                        }
                                                        Err(e) => {
                                                            tracing::warn!("[DROP_NEXT] Failed to check for next drop: {}", e);
                                                            // On error, assume campaign complete to allow transition
                                                            let _ = tx.send(
                                                                WatcherEvent::CampaignComplete(
                                                                    game_name.clone(),
                                                                ),
                                                            );
                                                            return Ok(());
                                                        }
                                                    }
                                                }
                                                Err(e) => {
                                                    tracing::warn!(
                                                        "[DROP_CLAIM] Failed to claim {}: {}",
                                                        name,
                                                        e
                                                    );
                                                }
                                            }
                                        } else {
                                            // Drop at 100% but no instance_id - account may not be linked
                                            tracing::warn!(
                                                "[UNLINKED] Drop '{}' at 100% but no drop_instance_id - account may not be linked to {}!",
                                                name,
                                                game_name
                                            );
                                            // Send CampaignComplete to stop watching this game
                                            // No point continuing if we can't claim
                                            let _ = tx.send(WatcherEvent::CampaignComplete(
                                                game_name.clone(),
                                            ));
                                            return Ok(());
                                        }
                                    }
                                    break;
                                } else {
                                    // Debugging: If 'drop' is missing, what DOES exist?
                                    tracing::warn!(
                                        "Context found but no drop data valid. keys: {:?}",
                                        drop_ctx
                                    );
                                }
                            }
                        } else {
                            // Log warning only if context is truly missing
                            tracing::warn!("Drop context missing. Raw response: {:?}", resp);
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Drop check request error: {}", e);
                    }
                }
            }

            if !drop_found_in_cycle {
                if has_mined_once {
                    consecutive_loop_failures += 1;
                    tracing::warn!(
                        "[DEEP DIVE] Loop Failure #{}. has_mined_once=true",
                        consecutive_loop_failures
                    );
                    if consecutive_loop_failures >= 5 {
                        // After 5 consecutive failures (5+ mins), report as transient - may recover
                        if tx
                            .send(WatcherEvent::TransientError(
                                "Drop context missing for extended period".to_string(),
                            ))
                            .is_err()
                        {
                            return Ok(());
                        }
                    } else {
                        tracing::info!(
                            "Drop context missing (glitch?), holding position ({}/3)...",
                            consecutive_loop_failures
                        );
                    }
                } else {
                    tracing::error!("[FATAL] No drop context on first attempt - channel may not have drops enabled");
                    if tx
                        .send(WatcherEvent::FatalError(
                            "No active drop context found - channel may not have drops".to_string(),
                        ))
                        .is_err()
                    {
                        return Ok(());
                    }
                }
            } else {
                has_mined_once = true;
                consecutive_loop_failures = 0;
            }
        } else {
            // This branch only hit if token_acquired=true but values are empty (edge case)
            if tx
                .send(WatcherEvent::TransientError(
                    "Token values empty despite acquisition".to_string(),
                ))
                .is_err()
            {
                return Ok(());
            }
        }

        sleep(Duration::from_secs(60)).await;
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
            access_token: "test_token".to_string(),
            user_id: 12345678,
            device_id: "test_device".to_string(),
            login: "testuser".to_string(),
        }
    }

    fn mock_target() -> WatchTarget {
        WatchTarget {
            channel_id: "98765".to_string(),
            channel_login: "streamer".to_string(),
            broadcast_id: "broadcast123".to_string(),
            spade_url: "https://video-edge.twitch.tv/test.ts".to_string(),
            token: "test_token".to_string(),
            sig: "test_sig".to_string(),
        }
    }

    #[test]
    fn test_payload_is_base64_encoded() {
        let watcher = Watcher::new(mock_auth());
        let target = mock_target();
        let payload = watcher.generate_payload(&target);

        // Should be valid base64
        let decoded = BASE64.decode(&payload).expect("Should be valid base64");
        let json_str = String::from_utf8(decoded).expect("Should be valid UTF-8");

        // Should be valid JSON
        let parsed: Vec<serde_json::Value> =
            serde_json::from_str(&json_str).expect("Should be valid JSON");

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0]["event"], "minute-watched");
    }

    #[test]
    fn test_payload_contains_correct_fields() {
        let watcher = Watcher::new(mock_auth());
        let target = mock_target();
        let payload = watcher.generate_payload(&target);

        let decoded = BASE64.decode(&payload).unwrap();
        let json_str = String::from_utf8(decoded).unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&json_str).unwrap();

        let props = &parsed[0]["properties"];
        assert_eq!(props["channel_id"], "98765");
        assert_eq!(props["channel"], "streamer");
        assert_eq!(props["broadcast_id"], "broadcast123");
        assert_eq!(props["user_id"], 12345678);
        assert_eq!(props["live"], true);
        assert_eq!(props["logged_in"], true);
        assert_eq!(props["player"], "site");
    }

    #[test]
    fn test_payload_structure_matches_python() {
        // Verify our payload matches the structure from Python's _spade_payload
        let watcher = Watcher::new(mock_auth());
        let target = mock_target();
        let payload = watcher.generate_payload(&target);

        let decoded = BASE64.decode(&payload).unwrap();
        let json_str = String::from_utf8(decoded).unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&json_str).unwrap();

        // Check all required fields exist
        let props = &parsed[0]["properties"];
        assert!(props.get("broadcast_id").is_some());
        assert!(props.get("channel_id").is_some());
        assert!(props.get("channel").is_some());
        assert!(props.get("hidden").is_some());
        assert!(props.get("live").is_some());
        assert!(props.get("location").is_some());
        assert!(props.get("logged_in").is_some());
        assert!(props.get("muted").is_some());
        assert!(props.get("player").is_some());
        assert!(props.get("user_id").is_some());
    }
}
