//! Application state and main logic loop.
//!
//! Coordinates authentication, inventory management, watching, and WebSocket events.

mod campaigns;
mod config;
mod navigation;
mod state;
mod watcher_mgmt; // Add this
pub use campaigns::*;
pub use config::*;
pub use navigation::*;
pub use state::*;
pub use watcher_mgmt::*; // Add this

use crate::auth::AuthState;
use crate::gql::GqlClient;
use crate::models::{Channel, DropsCampaign, TimedDrop};
use crate::watcher::{MiningStatus, WatchTarget, Watcher, WatcherEvent};
use anyhow::Result;
use std::collections::HashMap;
use std::time::Instant;

use tokio::sync::mpsc::UnboundedReceiver;

/// The main application.
pub struct App {
    pub state: AppState,
    pub page: Page,
    pub auth: Option<AuthState>,
    pub config: AppConfig,
    pub campaigns: Vec<DropsCampaign>,
    pub all_campaigns: Vec<DropsCampaign>,

    pub watching_channel: Option<Channel>,
    pub watching_target: Option<WatchTarget>,
    /// Drops Page State
    pub drops_focus: DropsFocus,
    pub drops_all_selected: usize,
    pub drops_subscribed_selected: usize,
    /// Home Page State
    pub home_focus: HomeFocus,
    pub home_watching_selected: usize,
    pub home_inactive_selected: usize,
    /// Pending login code for device code flow
    pub login_code: Option<String>,
    /// Pending login verification URI
    pub login_uri: Option<String>,
    /// Login status message
    pub login_status: Option<String>,
    pub gql: Option<GqlClient>,
    pub watcher: Option<Watcher>,
    pub drops: HashMap<String, TimedDrop>,
    pub mining_status: Option<MiningStatus>,
    pub watcher_rx: Option<UnboundedReceiver<WatcherEvent>>,

    // Failure recovery
    pub failed_game_attempts: HashMap<String, Instant>,
    pub current_attempt_game: Option<String>,
    /// Tracks whether we have an actual connection to a live stream
    pub has_live_stream: bool,

    /// Counter for transient errors - too many consecutive transient errors become fatal
    pub transient_error_count: u32,

    // Settings UI state
    pub settings_focus: SettingsFocus,
    pub settings_selected: SettingsItem,
    /// Whether proxy URL is being edited
    pub proxy_editing: bool,
    /// Current proxy URL input buffer
    pub proxy_input: String,
    /// Scroll position for About page
    pub about_scroll: u16,
}

impl App {
    /// Create a new application instance with default values.
    fn init_default(config: AppConfig) -> Self {
        Self {
            state: AppState::Idle,
            page: Page::Home,
            auth: None,
            config,
            campaigns: Vec::new(),
            all_campaigns: Vec::new(),

            watching_channel: None,
            watching_target: None,
            drops_focus: DropsFocus::AllDrops,
            drops_all_selected: 0,
            drops_subscribed_selected: 0,
            home_focus: HomeFocus::Watching,
            home_watching_selected: 0,
            home_inactive_selected: 0,
            login_code: None,
            login_uri: None,
            login_status: None,
            gql: None,
            watcher: None,
            drops: HashMap::new(),
            mining_status: None,
            watcher_rx: None,
            failed_game_attempts: HashMap::new(),
            current_attempt_game: None,
            has_live_stream: false,

            transient_error_count: 0,
            settings_focus: SettingsFocus::Settings,
            settings_selected: SettingsItem::AccountSettings,
            proxy_editing: false,
            proxy_input: String::new(),
            about_scroll: 0,
        }
    }

    /// Create a new application instance without authentication.
    pub fn new_logged_out(config: AppConfig) -> Self {
        Self::init_default(config)
    }

    /// Create a new application instance with authentication.
    pub fn new(auth: AuthState, config: AppConfig) -> Self {
        let gql = GqlClient::new_with_proxy(auth.clone(), config.proxy_url.clone());
        let watcher = Watcher::new_with_proxy(auth.clone(), config.proxy_url.clone());

        let mut app = Self::init_default(config);
        app.auth = Some(auth);
        app.gql = Some(gql);
        app.watcher = Some(watcher);
        app
    }

    /// Fetch inventory from Twitch GQL API.
    pub async fn fetch_inventory(&mut self) -> Result<usize> {
        let gql = self
            .gql
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Not logged in"))?;

        // Initialize cookies first (required for integrity checks)
        gql.init_cookies().await?;

        let response = gql.fetch_inventory().await?;

        // Parse using the new Inventory struct to get both campaigns and event drops
        let inventory_data = response.get("currentUser").and_then(|u| u.get("inventory"));

        let mut new_campaigns = Vec::new();
        let mut drop_count = 0;

        if let Some(inv_json) = inventory_data {
            if let Ok(inventory) =
                serde_json::from_value::<crate::models::inventory::Inventory>(inv_json.clone())
            {
                // 1. Process active/in-progress campaigns
                if let Some(campaigns) = inventory.drop_campaigns_in_progress {
                    for campaign in campaigns {
                        // Update drops map
                        for drop in &campaign.time_based_drops {
                            self.drops.insert(drop.id.clone(), drop.clone());
                            drop_count += 1;
                        }
                        new_campaigns.push(campaign);
                    }
                }

                // 2. Process game event drops (claimed items) to update subscribed campaigns status
                if let Some(event_drops) = inventory.game_event_drops {
                    for campaign in self.all_campaigns.iter_mut() {
                        // Only check subscribed games that currently have NO drops (and are likely showing as blank/queued)
                        if self
                            .config
                            .priority_games
                            .contains(&campaign.game.display_name)
                            && campaign.time_based_drops.is_empty()
                        {
                            // Check if ANY claimed drop seems to belong to this game
                            // We check if the drop name contains the game name OR if it's Rocket League (special case "RL")
                            let matching_drops: Vec<&crate::models::inventory::GameEventDrop> =
                                event_drops
                                    .iter()
                                    .filter(|d| {
                                        d.name.contains(&campaign.game.display_name)
                                            || (campaign.game.display_name == "Rocket League"
                                                && d.name.contains("RL"))
                                    })
                                    .collect();

                            if !matching_drops.is_empty() {
                                tracing::info!(
                                    "Found {} claimed drops for completed campaign: {}",
                                    matching_drops.len(),
                                    campaign.game.display_name
                                );

                                // Create a TimedDrop for each claimed item count so the UI shows accurate count (e.g. 10/10)
                                let mut completed_drops = Vec::new();
                                for d in matching_drops {
                                    let count = d.total_count.max(1);
                                    for i in 0..count {
                                        completed_drops.push(crate::models::inventory::TimedDrop {
                                            id: format!("{}_{}", d.id, i),
                                            name: d.name.clone(),
                                            required_minutes: 0, // 0 required = 100% progress
                                            starts_at: campaign.starts_at,
                                            ends_at: campaign.ends_at,
                                            benefit_edges: vec![], // Could try to make a fake benefit but not strictly needed for display
                                            self_info: Some(
                                                crate::models::inventory::DropSelfInfo {
                                                    current_minutes_watched: 0,
                                                    is_claimed: true,
                                                    drop_instance_id: Some(d.id.clone()),
                                                },
                                            ),
                                            extra_minutes: 0,
                                            extra_seconds: 0,
                                        });
                                    }
                                }

                                campaign.time_based_drops = completed_drops;
                            }
                        }
                    }
                }
            } else {
                // Fallback to old parsing if struct parsing fails
                let campaigns_data = inv_json
                    .get("dropCampaignsInProgress")
                    .and_then(|c| c.as_array())
                    .cloned()
                    .unwrap_or_default();

                for campaign_json in campaigns_data {
                    if let Ok(campaign) = serde_json::from_value::<DropsCampaign>(campaign_json) {
                        for drop in &campaign.time_based_drops {
                            self.drops.insert(drop.id.clone(), drop.clone());
                            drop_count += 1;
                        }
                        new_campaigns.push(campaign);
                    }
                }
            }
        }

        self.campaigns = new_campaigns;
        // Compact memory - free excess capacity after parsing
        self.campaigns.shrink_to_fit();
        for campaign in &mut self.campaigns {
            campaign.time_based_drops.shrink_to_fit();
        }
        self.drops.shrink_to_fit();

        let campaign_count = self.campaigns.len();

        tracing::info!(
            "Fetched {} campaigns with {} drops",
            campaign_count,
            drop_count
        );

        Ok(campaign_count)
    }

    /// Fetch ALL available campaigns (not just opted-in).
    pub async fn fetch_all_campaigns(&mut self) -> Result<usize> {
        let gql = self
            .gql
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Not logged in"))?;

        // Initialize cookies first (required for integrity checks)
        gql.init_cookies().await?;

        let response = gql.fetch_all_campaigns().await?;

        // Parse campaigns from the response
        let campaigns_data = response
            .get("currentUser")
            .and_then(|u| u.get("dropCampaigns"))
            .and_then(|c| c.as_array())
            .cloned()
            .unwrap_or_default();

        let mut new_campaigns = Vec::new();

        for campaign_json in campaigns_data {
            match serde_json::from_value::<DropsCampaign>(campaign_json.clone()) {
                Ok(mut campaign) => {
                    // Preserve local tracking (extra_minutes/extra_seconds) from old data
                    // This prevents timer from resetting to 0 on API refresh
                    if let Some(old_campaign) =
                        self.all_campaigns.iter().find(|c| c.id == campaign.id)
                    {
                        for new_drop in &mut campaign.time_based_drops {
                            if let Some(old_drop) = old_campaign
                                .time_based_drops
                                .iter()
                                .find(|d| d.id == new_drop.id)
                            {
                                // CARRY OVER local tracking from old drop to preserve countdown
                                new_drop.extra_minutes = old_drop.extra_minutes;
                                new_drop.extra_seconds = old_drop.extra_seconds;
                            }
                        }
                    }
                    new_campaigns.push(campaign);
                }
                Err(e) => {
                    // Try to at least get the name for debugging
                    let name = campaign_json
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("Unknown");
                    tracing::warn!("Failed to parse campaign '{}': {}", name, e);
                }
            }
        }

        self.all_campaigns = new_campaigns;
        // Compact memory - free excess capacity after parsing
        self.all_campaigns.shrink_to_fit();
        for campaign in &mut self.all_campaigns {
            campaign.time_based_drops.shrink_to_fit();
        }

        let count = self.all_campaigns.len();

        tracing::info!("Fetched {} total campaigns", count);

        Ok(count)
    }

    /// Fetch detailed progress for subscribed (priority) campaigns.
    /// This calls the Twitch API to get drop details for each subscribed campaign.
    pub async fn fetch_subscribed_campaign_details(&mut self) -> Result<usize> {
        let mut updated_count = 0;

        // First: Merge drops from inventory (as before)
        let inventory_drops: std::collections::HashMap<String, Vec<crate::models::TimedDrop>> =
            self.campaigns
                .iter()
                .filter(|c| !c.time_based_drops.is_empty())
                .map(|c| (c.id.clone(), c.time_based_drops.clone()))
                .collect();

        for campaign in self.all_campaigns.iter_mut() {
            if self
                .config
                .priority_games
                .contains(&campaign.game.display_name)
            {
                if let Some(inv_drops) = inventory_drops.get(&campaign.id) {
                    if !inv_drops.is_empty() {
                        let old_tracking: std::collections::HashMap<String, (i32, i32)> = campaign
                            .time_based_drops
                            .iter()
                            .map(|d| (d.id.clone(), (d.extra_minutes, d.extra_seconds)))
                            .collect();

                        campaign.time_based_drops = inv_drops.clone();

                        for drop in &mut campaign.time_based_drops {
                            if let Some((mins, secs)) = old_tracking.get(&drop.id) {
                                drop.extra_minutes = *mins;
                                drop.extra_seconds = *secs;
                            }
                        }
                        updated_count += 1;
                    }
                }
            }
        }

        // Second: For subscribed campaigns with EMPTY drops, call the API to get details
        let gql = match self.gql.as_ref() {
            Some(gql) => gql.clone(),
            None => return Ok(updated_count),
        };

        // Collect campaign IDs that need API fetch
        let campaigns_needing_fetch: Vec<(String, String, Option<String>)> = self
            .all_campaigns
            .iter()
            .filter(|c| {
                self.config.priority_games.contains(&c.game.display_name)
                    && c.time_based_drops.is_empty()
                    && c.is_active()
            })
            .map(|c| {
                (
                    c.id.clone(),
                    c.game.display_name.clone(),
                    c.game.slug.clone(),
                )
            })
            .collect();

        for (campaign_id, game_name, game_slug) in campaigns_needing_fetch {
            // Try to find a live channel for context if we have a slug
            let mut channel_login = None;
            if let Some(slug) = game_slug {
                // Fetch top 1 stream for the game to get a valid channel context
                if let Ok(directory) = gql.get_game_directory(&slug, 1).await {
                    // Extract broadcaster login: data.game.streams.edges[0].node.broadcaster.login
                    if let Some(login) = directory
                        .get("data")
                        .and_then(|d| d.get("game"))
                        .and_then(|g| g.get("streams"))
                        .and_then(|e| e.get(0))
                        .and_then(|e| e.get("node"))
                        .and_then(|n| n.get("broadcaster"))
                        .and_then(|b| b.get("login"))
                        .and_then(|l| l.as_str())
                    {
                        channel_login = Some(login.to_string());
                        tracing::debug!("Found context channel for {}: {}", game_name, login);
                    }
                }
            }

            match gql
                .fetch_campaign_details(&campaign_id, channel_login.as_deref())
                .await
            {
                Ok(details) => {
                    // Parse drops from the campaign details response
                    if let Some(drops_array) = details
                        .get("user")
                        .and_then(|u| u.get("dropCampaign"))
                        .and_then(|c| c.get("timeBasedDrops"))
                        .and_then(|d| d.as_array())
                    {
                        let mut parsed_drops = Vec::new();
                        for drop_json in drops_array {
                            match serde_json::from_value::<crate::models::TimedDrop>(
                                drop_json.clone(),
                            ) {
                                Ok(drop) => parsed_drops.push(drop),
                                Err(e) => tracing::debug!("Failed to parse drop: {}", e),
                            }
                        }

                        if !parsed_drops.is_empty() {
                            // Find and update the campaign
                            if let Some(campaign) =
                                self.all_campaigns.iter_mut().find(|c| c.id == campaign_id)
                            {
                                campaign.time_based_drops = parsed_drops;
                                updated_count += 1;
                                tracing::info!(
                                    "Fetched {} drops for {} from API via context {}",
                                    campaign.time_based_drops.len(),
                                    game_name,
                                    channel_login.as_deref().unwrap_or("none")
                                );
                            }
                        }
                    } else {
                        tracing::debug!(
                            "No dropCampaign data found for {} (context: {:?})",
                            game_name,
                            channel_login
                        );
                    }
                }
                Err(e) => {
                    tracing::debug!("Failed to fetch campaign details for {}: {}", game_name, e);
                }
            }
        }

        if updated_count > 0 {
            tracing::info!(
                "Updated drop data for {} subscribed campaigns",
                updated_count
            );
        }

        Ok(updated_count)
    }

    /// Perform a full data refresh (Campaigns + Inventory) without changing AppState.
    /// This is used for background updates while watching.
    pub async fn refresh_data_background(&mut self) -> Result<()> {
        // 1. Fetch All Campaigns
        match self.fetch_all_campaigns().await {
            Ok(_) => {
                // 2. Fetch Inventory (updates campaigns list)
                if let Err(e) = self.fetch_inventory().await {
                    tracing::warn!("Background refresh: Inventory fetch failed: {}", e);
                }

                // 3. Fetch Subscribed Details
                if let Err(e) = self.fetch_subscribed_campaign_details().await {
                    tracing::warn!("Background refresh: Subscribed details fetch failed: {}", e);
                }
            }
            Err(e) => {
                tracing::warn!("Background refresh: Campaigns fetch failed: {}", e);
                return Err(e);
            }
        }

        // Compact memory after refresh to reduce fragmentation
        self.compact_memory();

        Ok(())
    }

    pub fn tick(&mut self) -> Vec<String> {
        let mut logs = Vec::new();
        let mut events = Vec::new();
        if let Some(rx) = &mut self.watcher_rx {
            while let Ok(event) = rx.try_recv() {
                events.push(event);
            }
        }

        for event in events {
            logs.extend(self.handle_worker_event(event));
        }
        logs
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Game;
    use chrono::Utc;

    fn mock_auth() -> AuthState {
        AuthState {
            access_token: "test_token".to_string(),
            user_id: 12345678,
            device_id: "test_device".to_string(),
            login: "testuser".to_string(),
        }
    }

    fn mock_campaign(game_name: &str, active: bool) -> DropsCampaign {
        let now = Utc::now();
        DropsCampaign {
            id: format!("campaign-{}", game_name),
            name: format!("{} Campaign", game_name),
            game: Game {
                id: format!("game-{}", game_name),
                display_name: game_name.to_string(),
                box_art_url: None,
                slug: None,
            },
            starts_at: if active {
                now - chrono::Duration::days(1)
            } else {
                now + chrono::Duration::days(1)
            },
            ends_at: now + chrono::Duration::days(7),
            status: if active {
                "ACTIVE".to_string()
            } else {
                "UPCOMING".to_string()
            },
            time_based_drops: vec![],
            self_info: None,
        }
    }

    #[test]
    fn test_app_state_change() {
        let mut app = App::new(mock_auth(), AppConfig::default());
        assert_eq!(app.state, AppState::Idle);

        app.change_state(AppState::Watching);
        assert_eq!(app.state, AppState::Watching);
    }

    #[test]
    fn test_active_campaigns_filter() {
        let mut app = App::new(mock_auth(), AppConfig::default());
        app.campaigns = vec![
            mock_campaign("Fortnite", true),
            mock_campaign("Minecraft", false),
            mock_campaign("Valorant", true),
        ];

        let active = app.active_campaigns();
        assert_eq!(active.len(), 2);
        assert!(active.iter().any(|c| c.game.display_name == "Fortnite"));
        assert!(active.iter().any(|c| c.game.display_name == "Valorant"));
    }

    #[test]
    fn test_prioritized_campaigns() {
        let mut app = App::new(
            mock_auth(),
            AppConfig {
                priority_games: vec!["Valorant".to_string()],
                ..Default::default()
            },
        );
        app.campaigns = vec![
            mock_campaign("Fortnite", true),
            mock_campaign("Valorant", true),
        ];

        let prioritized = app.prioritized_campaigns();
        assert_eq!(prioritized.len(), 2);
        assert_eq!(prioritized[0].game.display_name, "Valorant"); // Priority first
    }

    #[test]
    fn test_excluded_games() {
        let mut app = App::new(
            mock_auth(),
            AppConfig {
                excluded_games: vec!["Fortnite".to_string()],
                ..Default::default()
            },
        );
        app.campaigns = vec![
            mock_campaign("Fortnite", true),
            mock_campaign("Valorant", true),
        ];

        let active = app.active_campaigns();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].game.display_name, "Valorant");
    }

    #[test]
    fn test_wanted_games() {
        let mut app = App::new(mock_auth(), AppConfig::default());
        app.campaigns = vec![
            mock_campaign("Fortnite", true),
            mock_campaign("Valorant", true),
        ];

        let games = app.wanted_games();
        assert_eq!(games.len(), 2);
    }

    #[test]
    fn test_bump_only_when_watching_state() {
        let mut app = App::new(mock_auth(), AppConfig::default());
        app.state = AppState::Idle; // Not watching
        app.mining_status = Some(crate::watcher::MiningStatus {
            game_name: "TestGame".to_string(),
            channel_login: "test".to_string(),
            drop_name: "TestDrop".to_string(),
            progress_percent: 0.0,
            minutes_watched: 0,
            minutes_required: 60,
        });

        // Bump subscribed campaigns
        app.bump_active_drop_second();
        // No campaigns set and no priority games, so nothing to bump

        // Add a priority game and test again
        app.config.priority_games.push("TestGame".to_string());
        app.bump_active_drop_second();
        // Still no campaigns, but now the function tries to find matching campaigns
    }

    #[test]
    fn test_stop_watching_resets_state() {
        let mut app = App::new(mock_auth(), AppConfig::default());
        app.has_live_stream = true;
        app.state = AppState::Watching;
        app.mining_status = Some(crate::watcher::MiningStatus {
            game_name: "TestGame".to_string(),
            channel_login: "test".to_string(),
            drop_name: "TestDrop".to_string(),
            progress_percent: 50.0,
            minutes_watched: 30,
            minutes_required: 60,
        });

        app.stop_watching();

        assert_eq!(app.state, AppState::Idle);
        assert!(!app.has_live_stream);
        assert!(app.mining_status.is_none());
        assert!(app.watching_channel.is_none());
    }

    #[test]
    fn test_initial_has_live_stream_is_false() {
        let app = App::new(mock_auth(), AppConfig::default());
        assert!(!app.has_live_stream);
    }

    #[test]
    fn test_valid_proxy_url_formats() {
        // Valid HTTP proxy
        assert!(crate::app::is_valid_proxy_url(
            "http://proxy.example.com:8080"
        ));
        // Valid with credentials
        assert!(crate::app::is_valid_proxy_url(
            "http://user:pass@proxy.example.com:8080"
        ));
        // Valid HTTPS
        assert!(crate::app::is_valid_proxy_url(
            "https://proxy.example.com:443"
        ));
        // Valid SOCKS5
        assert!(crate::app::is_valid_proxy_url("socks5://localhost:1080"));

        // Invalid - empty
        assert!(!crate::app::is_valid_proxy_url(""));
        // Invalid - no scheme
        assert!(!crate::app::is_valid_proxy_url("proxy.example.com:8080"));
        // Invalid - unsupported scheme
        assert!(!crate::app::is_valid_proxy_url("ftp://proxy.example.com"));
    }

    #[test]
    fn test_config_with_proxy_serialization() {
        let config = AppConfig {
            proxy_url: Some("http://user:pass@proxy:8080".to_string()),
            ..AppConfig::default()
        };

        let json = serde_json::to_string(&config).unwrap();
        let loaded: AppConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(
            loaded.proxy_url,
            Some("http://user:pass@proxy:8080".to_string())
        );
    }

    #[test]
    fn test_config_without_proxy_loads_none() {
        let json = r#"{"auth_path":"auth.json","priority_games":[],"excluded_games":[]}"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert!(config.proxy_url.is_none());
    }
}
