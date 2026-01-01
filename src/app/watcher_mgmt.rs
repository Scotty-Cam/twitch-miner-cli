use super::{App, AppState, CampaignOps};
use crate::models::{Channel, DropSelfInfo};
use crate::watcher::{Watcher, WatcherEvent};
use crate::websocket::PubSubEvent;
use anyhow::{Context, Result};
use std::time::{Duration, Instant};

#[allow(async_fn_in_trait)]
pub trait WatcherOps {
    async fn start_watching(
        &mut self,
        channel_login: String,
        channel_id: String,
        broadcast_id: String,
        game_name: String,
    ) -> Result<()>;
    fn stop_watching(&mut self);
    async fn check_priority_switch(&mut self) -> Result<bool>;
    async fn try_autostart(&mut self) -> Result<String>;
    fn is_watcher_active(&self) -> bool;
    fn bump_active_drop_second(&mut self);
    async fn claim_unclaimed_drops(&mut self) -> Result<Vec<(String, String)>>;
    fn handle_worker_event(&mut self, event: WatcherEvent) -> Vec<String>;
    fn handle_pubsub_event(&mut self, event: &PubSubEvent);
}

impl WatcherOps for App {
    async fn start_watching(
        &mut self,
        channel_login: String,
        channel_id: String,
        broadcast_id: String,
        game_name: String,
    ) -> Result<()> {
        if self.watcher_rx.is_some() {
            return Ok(());
        }

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        self.watcher_rx = Some(rx);
        self.mining_status = None; // Reset status until first update

        let auth = self.auth.clone().context("Not logged in")?;
        let gql = self.gql.clone().context("GQL not initialized")?;
        let watcher = Watcher::new(auth);

        self.watching_channel = Some(Channel {
            id: channel_id.clone(),
            login: channel_login.clone(),
            display_name: Some(channel_login.clone()),
            profile_image_url: None,
        });

        // Spawn the watcher loop
        tokio::spawn(async move {
            if let Err(e) = crate::watcher::mine_loop(
                gql,
                watcher,
                channel_login,
                channel_id,
                broadcast_id,
                game_name,
                tx,
            )
            .await
            {
                tracing::error!("Watcher loop failed: {}", e);
            }
        });

        self.state = AppState::Watching;
        Ok(())
    }

    fn stop_watching(&mut self) {
        self.watching_channel = None;
        self.watching_target = None;
        self.watcher_rx = None;
        self.has_live_stream = false;
        self.mining_status = None;
        self.current_attempt_game = None; // Clear attempt so we stop bumping
        self.state = AppState::Idle;
    }

    /// Check if a higher priority game now has available streams.
    /// If so, switch to it by stopping current watch (autostart will pick it up).
    async fn check_priority_switch(&mut self) -> Result<bool> {
        // Only check if we're currently watching something
        if !self.has_live_stream || self.mining_status.is_none() {
            return Ok(false);
        }

        let current_game = match &self.mining_status {
            Some(status) => status.game_name.clone(),
            None => return Ok(false),
        };

        // Find current game's priority index
        let current_priority = self
            .config
            .priority_games
            .iter()
            .position(|g| g == &current_game);

        // Check higher priority games (those with lower index)
        let max_check = current_priority.unwrap_or(self.config.priority_games.len());

        for (idx, game_name) in self.config.priority_games.iter().enumerate() {
            if idx >= max_check {
                break; // Only check higher priority games
            }

            // Skip if recently failed
            if let Some(fail_time) = self.failed_game_attempts.get(game_name) {
                if fail_time.elapsed() < Duration::from_secs(300) {
                    continue;
                }
            }

            // Check if game has active campaign WITH unclaimed drops
            // Must have: active campaign + progress < 100% + at least one unclaimed drop
            let has_unclaimed_drops = self.all_campaigns.iter().any(|c| {
                c.game.display_name == *game_name
                    && c.is_active()
                    && c.campaign_progress() < 1.0
                    && c.first_unclaimed_drop().is_some()
            }) || self.campaigns.iter().any(|c| {
                c.game.display_name == *game_name
                    && c.is_active()
                    && c.campaign_progress() < 1.0
                    && c.first_unclaimed_drop().is_some()
            });

            if !has_unclaimed_drops {
                tracing::debug!("Skipping {} - no unclaimed drops", game_name);
                continue;
            }

            // Check if game has live streams
            let slug = game_name.to_lowercase().replace(" ", "-");
            if let Some(gql) = &self.gql {
                if let Ok(resp) = gql.get_game_directory(&slug, 1).await {
                    if let Some(streams) = resp
                        .get("game")
                        .and_then(|g| g.get("streams"))
                        .and_then(|s| s.get("edges"))
                        .and_then(|e| e.as_array())
                    {
                        if !streams.is_empty() {
                            tracing::info!(
                                "Higher priority game {} has streams! Switching from {}",
                                game_name,
                                current_game
                            );
                            self.stop_watching();
                            return Ok(true); // Switched, autostart will pick up the new game
                        }
                    }
                }
            }
        }

        Ok(false)
    }

    /// Try to automatically start watching a priority game.
    async fn try_autostart(&mut self) -> Result<String> {
        tracing::info!("Autostart check initiated...");
        if self.mining_status.is_some() || self.watcher_rx.is_some() {
            tracing::debug!("Already mining or watcher active.");
            return Ok("Already watching".to_string());
        }

        // Cleanup old failures
        let now = Instant::now();
        self.failed_game_attempts
            .retain(|_, time| now.duration_since(*time) < Duration::from_secs(300));

        tracing::debug!(
            "Attempting autostart check for {} priority games",
            self.config.priority_games.len()
        );

        let mut found_target = None;
        // Clone to avoid borrow
        let priority_games = self.config.priority_games.clone();

        // 1. Check priority games for active campaigns
        for game_name in &priority_games {
            // Check if game failed recently
            if let Some(fail_time) = self.failed_game_attempts.get(game_name) {
                if fail_time.elapsed() < Duration::from_secs(300) {
                    // 5 min cooldown
                    tracing::info!(
                        "Skipping {} due to recent failure (cooldown active)",
                        game_name
                    );
                    continue;
                }
            }

            // Simple slugify
            let slug = game_name.to_lowercase().replace(" ", "-");

            // Check if we have an active, uncompleted campaign for this game
            // Check if game has active campaign WITH unclaimed drops
            let has_active_campaign = self.all_campaigns.iter().any(|c| {
                c.game.display_name == *game_name
                    && c.is_active()
                    && c.campaign_progress() < 1.0
                    && c.first_unclaimed_drop().is_some()
            }) || self.campaigns.iter().any(|c| {
                c.game.display_name == *game_name
                    && c.is_active()
                    && c.campaign_progress() < 1.0
                    && c.first_unclaimed_drop().is_some()
            });

            if !has_active_campaign {
                // Check inventory campaigns too (backup)
                let has_inventory_campaign = self.campaigns.iter().any(|c| {
                    c.game.display_name == *game_name
                        && c.is_active()
                        && c.campaign_progress() < 1.0
                        && c.first_unclaimed_drop().is_some()
                });

                if !has_inventory_campaign {
                    tracing::debug!(
                        "Skipping {} - no active campaign with unclaimed drops",
                        game_name
                    );
                    continue;
                }
            }

            // Fetch live streams
            let gql_client = if let Some(g) = &self.gql {
                g.clone()
            } else {
                continue;
            };

            tracing::info!("Fetching streams for game: {} (slug: {})", game_name, slug);
            match gql_client.get_game_directory(&slug, 5).await {
                // Limit 5
                Ok(resp) => {
                    // Parse streams
                    if let Some(streams) = resp
                        .get("game")
                        .and_then(|g| g.get("streams"))
                        .and_then(|s| s.get("edges"))
                        .and_then(|e| e.as_array())
                    {
                        tracing::debug!("Found {} streams for {}", streams.len(), game_name);
                        // Find first valid stream
                        for edge in streams {
                            if let Some(node) = edge.get("node") {
                                let login = node
                                    .get("broadcaster")
                                    .and_then(|b| b.get("login"))
                                    .and_then(|s| s.as_str());
                                let channel_id = node
                                    .get("broadcaster")
                                    .and_then(|b| b.get("id"))
                                    .and_then(|s| s.as_str());
                                let broadcast_id = node.get("id").and_then(|s| s.as_str()); // Stream ID
                                let viewers = node
                                    .get("viewersCount")
                                    .and_then(|v| v.as_i64())
                                    .unwrap_or(0);

                                if let (Some(l), Some(cid), Some(bid)) =
                                    (login, channel_id, broadcast_id)
                                {
                                    found_target = Some((
                                        l.to_string(),
                                        cid.to_string(),
                                        bid.to_string(),
                                        game_name.clone(),
                                        viewers,
                                    ));
                                    break;
                                }
                            }
                        }
                    }
                }
                Err(e) => tracing::warn!("Failed to fetch streams for {}: {}", game_name, e),
            }

            if found_target.is_some() {
                break;
            }
        }

        if let Some((login, channel_id, broadcast_id, game, viewers)) = found_target {
            tracing::info!(
                "Autostarting watch for {} ({}) - {} viewers",
                login,
                game,
                viewers
            );
            self.current_attempt_game = Some(game.clone());
            self.start_watching(login.clone(), channel_id, broadcast_id, game.clone())
                .await?;
            return Ok(format!(
                "Started watching {} playing {} ({} viewers)",
                login, game, viewers
            ));
        }

        Err(anyhow::anyhow!("No suitable streams found"))
    }

    /// Check if there's an active watcher (watching a stream).
    fn is_watcher_active(&self) -> bool {
        self.watcher_rx.is_some()
    }

    /// Increment extra seconds for ALL subscribed (priority) campaigns.
    fn bump_active_drop_second(&mut self) {
        // Determine active game
        let active_game = if let Some(status) = &self.mining_status {
            Some(status.game_name.clone())
        } else {
            self.current_attempt_game.as_ref().cloned()
        };

        if let Some(game_name) = active_game {
            // Update in all_campaigns
            for campaign in &mut self.all_campaigns {
                if campaign.game.display_name == game_name {
                    if let Some(drop) = campaign
                        .time_based_drops
                        .iter_mut()
                        .find(|d| !d.is_claimed())
                    {
                        drop.bump_extra_second();
                    }
                }
            }

            // Update in inventory campaigns too
            for campaign in &mut self.campaigns {
                if campaign.game.display_name == game_name {
                    if let Some(drop) = campaign
                        .time_based_drops
                        .iter_mut()
                        .find(|d| !d.is_claimed())
                    {
                        drop.bump_extra_second();
                    }
                }
            }
        }
    }

    /// Check for and claim any drops that are ready but unclaimed.
    async fn claim_unclaimed_drops(&mut self) -> Result<Vec<(String, String)>> {
        let gql = if let Some(g) = &self.gql {
            g.clone()
        } else {
            return Ok(Vec::new());
        };

        let mut claims_to_process = Vec::new(); // (Game, Drop, InstanceID)

        // Scan all campaigns for claimable drops
        let campaign_sources = [&self.all_campaigns, &self.campaigns];
        for campaigns in campaign_sources {
            for campaign in campaigns {
                for drop in &campaign.time_based_drops {
                    if drop.can_claim() {
                        if let Some(instance_id) = drop.drop_instance_id() {
                            // Try to get a better name
                            let display_name = drop
                                .benefit_edges
                                .first()
                                .map(|e| e.benefit.name.clone())
                                .unwrap_or(drop.name.clone());

                            claims_to_process.push((
                                campaign.game.display_name.clone(),
                                display_name,
                                instance_id.to_string(),
                            ));
                        }
                    }
                }
            }
        }

        if claims_to_process.is_empty() {
            return Ok(Vec::new());
        }

        let mut claimed_drops = Vec::new();
        let total = claims_to_process.len();
        tracing::info!(
            "Found {} unclaimed drops. Attempting cleanup claim...",
            total
        );

        // Deduplicate claims (same drop in both lists)
        claims_to_process.sort_by(|a, b| a.2.cmp(&b.2));
        claims_to_process.dedup_by(|a, b| a.2 == b.2);

        for (game_name, drop_name, instance_id) in claims_to_process {
            tracing::info!("Cleanup Claim: {} - {}", game_name, drop_name);
            match gql.claim_drop(&instance_id).await {
                Ok(_) => {
                    self.mark_drop_claimed(&game_name, &drop_name);

                    // Send desktop notification
                    if self.config.notifications_enabled {
                        if let Err(e) =
                            crate::notifications::send_drop_notification(&game_name, &drop_name)
                        {
                            tracing::warn!("Failed to send notification: {}", e);
                        }
                    }
                    claimed_drops.push((game_name, drop_name));
                }
                Err(e) => {
                    tracing::warn!("Failed to cleanup claim for {}: {}", drop_name, e);
                }
            }
            // Small delay to be polite
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        if !claimed_drops.is_empty() {
            tracing::info!(
                "Successfully claimed {}/{} drops during cleanup",
                claimed_drops.len(),
                total
            );
        }

        Ok(claimed_drops)
    }

    /// Handle a Watcher Event (from worker)
    fn handle_worker_event(&mut self, event: WatcherEvent) -> Vec<String> {
        let mut logs = Vec::new();
        match event {
            WatcherEvent::Status(status) => {
                // Sync progress to campaign drop data for UI display
                for campaign in &mut self.all_campaigns {
                    if campaign.game.display_name == status.game_name {
                        let drop_idx = campaign
                            .time_based_drops
                            .iter()
                            .position(|d| d.name == status.drop_name && !d.is_claimed())
                            .or_else(|| {
                                campaign
                                    .time_based_drops
                                    .iter()
                                    .position(|d| d.name == status.drop_name)
                            })
                            .or_else(|| {
                                // Fallback: if name is "Active Drop" (default), update first unclaimed
                                if status.drop_name == "Active Drop" {
                                    campaign
                                        .time_based_drops
                                        .iter()
                                        .position(|d| !d.is_claimed())
                                } else {
                                    None
                                }
                            });

                        if let Some(idx) = drop_idx {
                            let drop = &mut campaign.time_based_drops[idx];
                            let local_minutes = drop.current_minutes();
                            let api_minutes = status.minutes_watched as f64;

                            if let Some(ref mut info) = drop.self_info {
                                info.current_minutes_watched = status.minutes_watched;
                            } else {
                                drop.self_info = Some(DropSelfInfo {
                                    current_minutes_watched: status.minutes_watched,
                                    is_claimed: false,
                                    drop_instance_id: None,
                                });
                            }

                            if api_minutes >= local_minutes {
                                drop.extra_minutes = 0;
                                drop.extra_seconds = 0;
                            }
                        }
                        break;
                    }
                }

                self.mining_status = Some(status);
                self.has_live_stream = true;
                self.current_attempt_game = None;
                self.transient_error_count = 0;
            }
            WatcherEvent::TransientError(e) => {
                logs.push(format!("WATCHER: Transient issue: {}", e));
                tracing::warn!("Transient watcher error: {}", e);
                self.transient_error_count += 1;

                if self.transient_error_count >= 10 {
                    logs.push(
                        "WATCHER: Too many transient errors, stopping to try another channel"
                            .to_string(),
                    );
                    tracing::error!(
                        "Too many transient errors ({}), treating as fatal",
                        self.transient_error_count
                    );
                    self.transient_error_count = 0;
                    self.stop_watching();
                }
            }
            WatcherEvent::FatalError(e) => {
                logs.push(format!("WATCHER FATAL: {}", e));
                tracing::error!("Fatal watcher error: {}", e);

                self.has_live_stream = false;

                let failed_game = self
                    .current_attempt_game
                    .take()
                    .or_else(|| self.mining_status.as_ref().map(|s| s.game_name.clone()));

                if let Some(game) = failed_game {
                    logs.push(format!("WATCHER: Recording failure for game: {}", game));
                    self.failed_game_attempts.insert(game, Instant::now());
                }

                self.transient_error_count = 0;
                self.stop_watching();
            }
            WatcherEvent::Claimed(name) => {
                let game_name = self
                    .mining_status
                    .as_ref()
                    .map(|s| s.game_name.clone())
                    .unwrap_or_else(|| "Unknown Game".to_string());

                logs.push(format!(
                    "Twitch drop obtained: {} | Game: {}",
                    name, game_name
                ));
                tracing::info!("Twitch drop obtained: {} | Game: {}", name, game_name);

                if self.config.notifications_enabled {
                    if let Err(e) = crate::notifications::send_drop_notification(&game_name, &name)
                    {
                        tracing::warn!("Failed to send notification: {}", e);
                    }
                }
                self.mark_drop_claimed(&game_name, &name);
            }
            WatcherEvent::CampaignComplete(game_name) => {
                logs.push(format!(
                    "✓ Campaign complete: {} - All drops claimed!",
                    game_name
                ));
                tracing::info!(
                    "[CAMPAIGN_COMPLETE] All drops claimed for {}, stopping watcher",
                    game_name
                );
                self.stop_watching();
                // Autostart will pick up the next game on the next tick (every 2s when Idle)
            }
        }
        logs
    }

    /// Handle a PubSub event.
    fn handle_pubsub_event(&mut self, event: &PubSubEvent) {
        match event {
            PubSubEvent::DropProgress {
                drop_id,
                current_minutes,
            } => {
                if let Some(drop) = self.drops.get_mut(drop_id) {
                    if let Some(ref mut info) = drop.self_info {
                        info.current_minutes_watched = *current_minutes;
                    }
                    tracing::info!(
                        "Drop progress: {} - {}/{} minutes",
                        drop.name,
                        current_minutes,
                        drop.required_minutes
                    );
                }
            }
            PubSubEvent::DropReady { drop_instance_id } => {
                tracing::info!("Drop ready to claim: {}", drop_instance_id);
            }
            PubSubEvent::StreamOnline { channel_id } => {
                tracing::info!("Stream online: {}", channel_id);
            }
            PubSubEvent::StreamOffline { channel_id } => {
                tracing::info!("Stream offline: {}", channel_id);
            }
            PubSubEvent::Unknown(_) => {}
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{App, AppConfig, AppState};
    use crate::auth::AuthState;
    use crate::watcher::MiningStatus;

    fn mock_auth() -> AuthState {
        AuthState {
            access_token: "test_token".to_string(),
            user_id: 12345678,
            device_id: "test_device".to_string(),
            login: "testuser".to_string(),
        }
    }

    #[test]
    fn test_campaign_complete_stops_watching() {
        let mut app = App::new(mock_auth(), AppConfig::default());

        // Simulate watching state
        app.state = AppState::Watching;
        app.has_live_stream = true;
        app.mining_status = Some(MiningStatus {
            game_name: "TestGame".to_string(),
            channel_login: "test_channel".to_string(),
            drop_name: "TestDrop".to_string(),
            progress_percent: 100.0,
            minutes_watched: 60,
            minutes_required: 60,
        });

        // Handle CampaignComplete event
        let logs = app.handle_worker_event(WatcherEvent::CampaignComplete("TestGame".to_string()));

        // Verify state changes
        assert_eq!(
            app.state,
            AppState::Idle,
            "State should be Idle after campaign complete"
        );
        assert!(
            !app.has_live_stream,
            "has_live_stream should be false after stop_watching"
        );
        assert!(
            app.mining_status.is_none(),
            "mining_status should be None after stop_watching"
        );
        assert!(
            logs.iter().any(|l| l.contains("Campaign complete")),
            "Log should contain campaign complete message"
        );
    }

    #[test]
    fn test_claimed_event_marks_drop_claimed() {
        let mut app = App::new(mock_auth(), AppConfig::default());

        // Set up mining status so we know the game name
        app.mining_status = Some(MiningStatus {
            game_name: "TestGame".to_string(),
            channel_login: "test_channel".to_string(),
            drop_name: "TestDrop".to_string(),
            progress_percent: 100.0,
            minutes_watched: 60,
            minutes_required: 60,
        });

        // Handle Claimed event
        let logs = app.handle_worker_event(WatcherEvent::Claimed("TestDrop".to_string()));

        // Verify log output
        assert!(
            logs.iter().any(|l| l.contains("Twitch drop obtained")),
            "Log should contain drop obtained message"
        );
        assert!(
            logs.iter().any(|l| l.contains("TestGame")),
            "Log should mention game name"
        );
    }
}
