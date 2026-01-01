use super::App;
use crate::models::{DropSelfInfo, DropsCampaign, Game, TimedDrop};
use std::collections::HashSet;

pub trait CampaignOps {
    fn active_campaigns(&self) -> Vec<&DropsCampaign>;
    fn prioritized_campaigns(&self) -> Vec<&DropsCampaign>;
    fn subscribed_campaigns_with_progress(&self) -> Vec<&DropsCampaign>;
    fn get_game_display_info(&self, game_name: &str) -> (String, bool);
    fn toggle_game_subscription(&mut self, game_name: String) -> String;
    fn add_priority_game(&mut self, game_name: String) -> String;
    fn mark_drop_claimed(&mut self, game_name: &str, drop_name: &str);
    fn first_unclaimed_drop(&self) -> Option<(&DropsCampaign, &TimedDrop)>;
    fn wanted_games(&self) -> Vec<&Game>;
}

impl CampaignOps for App {
    /// Get the campaigns that can be progressed.
    fn active_campaigns(&self) -> Vec<&DropsCampaign> {
        self.campaigns
            .iter()
            .filter(|c| c.is_active() && !self.config.excluded_games.contains(&c.game.display_name))
            .collect()
    }

    /// Get campaigns sorted by priority.
    fn prioritized_campaigns(&self) -> Vec<&DropsCampaign> {
        let mut active = self.active_campaigns();

        active.sort_by(|a, b| {
            let a_priority = self
                .config
                .priority_games
                .iter()
                .position(|g| g == &a.game.display_name);
            let b_priority = self
                .config
                .priority_games
                .iter()
                .position(|g| g == &b.game.display_name);

            match (a_priority, b_priority) {
                (Some(a_idx), Some(b_idx)) => a_idx.cmp(&b_idx),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => a.ends_at.cmp(&b.ends_at), // Earlier ending first
            }
        });

        active
    }

    /// Get subscribed (priority) campaigns with progress data for home screen display.
    /// Deduplicates campaigns by ID and prioritizes campaigns from inventory (which have progress).
    fn subscribed_campaigns_with_progress(&self) -> Vec<&DropsCampaign> {
        let mut seen_ids = HashSet::new();
        let mut subscribed = Vec::new();

        // First, check all_campaigns (which may have merged progress data from inventory)
        for campaign in &self.all_campaigns {
            if self
                .config
                .priority_games
                .contains(&campaign.game.display_name)
                && seen_ids.insert(&campaign.id)
            {
                subscribed.push(campaign);
            }
        }

        // Then add any from inventory that weren't in all_campaigns (fallback)
        for campaign in &self.campaigns {
            if self
                .config
                .priority_games
                .contains(&campaign.game.display_name)
                && seen_ids.insert(&campaign.id)
            {
                subscribed.push(campaign);
            }
        }

        // Sort by game name for consistent display
        subscribed.sort_by(|a, b| {
            a.game
                .display_name
                .to_lowercase()
                .cmp(&b.game.display_name.to_lowercase())
        });

        subscribed
    }

    /// Get format string for campaigns AND linked status
    fn get_game_display_info(&self, game_name: &str) -> (String, bool) {
        use super::NavigationOps; // Needed for get_game_campaigns_string

        let campaigns_str = self.get_game_campaigns_string(game_name);

        // Check linked status
        let mut is_linked = false;

        // check active campaigns first
        for campaign in &self.campaigns {
            if campaign.game.display_name == game_name {
                if let Some(self_info) = &campaign.self_info {
                    if self_info.is_account_connected {
                        is_linked = true;
                        break;
                    }
                }
            }
        }

        if !is_linked {
            // check all campaigns
            for campaign in &self.all_campaigns {
                if campaign.game.display_name == game_name {
                    if let Some(self_info) = &campaign.self_info {
                        if self_info.is_account_connected {
                            is_linked = true;
                            break;
                        }
                    }
                }
            }
        }

        (campaigns_str, is_linked)
    }

    /// Toggle subscription/priority for a game
    fn toggle_game_subscription(&mut self, game_name: String) -> String {
        let is_subscribed = self.config.priority_games.contains(&game_name);

        if is_subscribed {
            self.config.priority_games.retain(|g| g != &game_name);
            if let Err(e) = self.config.save() {
                tracing::error!("Failed to save config: {}", e);
                return "Failed to save settings".to_string();
            }
            format!("Unsubscribed from {}.", game_name)
        } else {
            self.config.priority_games.push(game_name.clone());
            if let Err(e) = self.config.save() {
                tracing::error!("Failed to save config: {}", e);
                return "Failed to save settings".to_string();
            }
            format!("Subscribed to {}.", game_name)
        }
    }

    /// Add a game to priority list (from Search/Add)
    fn add_priority_game(&mut self, game_name: String) -> String {
        if self.config.priority_games.contains(&game_name) {
            return format!("{} is already in your priority list.", game_name);
        }

        self.config.priority_games.insert(0, game_name.clone()); // Add to top
        if let Err(e) = self.config.save() {
            tracing::error!("Failed to save config: {}", e);
            return "Failed to save settings".to_string();
        }
        format!("Added {} to priority list.", game_name)
    }

    /// Mark a drop as claimed in local state.
    fn mark_drop_claimed(&mut self, game_name: &str, drop_name: &str) {
        let search_targets = vec![&mut self.all_campaigns, &mut self.campaigns];

        for campaigns in search_targets {
            for campaign in campaigns.iter_mut() {
                if campaign.game.display_name == game_name {
                    for drop in &mut campaign.time_based_drops {
                        if drop.name == drop_name {
                            if let Some(ref mut info) = drop.self_info {
                                info.is_claimed = true;
                            } else {
                                // Create self_info if missing (shouldn't happen for claimable drops usually)
                                drop.self_info = Some(DropSelfInfo {
                                    current_minutes_watched: drop.required_minutes,
                                    is_claimed: true,
                                    drop_instance_id: None,
                                });
                            }
                            tracing::info!(
                                "[SYNC] Marked drop '{}' as claimed in campaign data",
                                drop_name
                            );
                            break;
                        }
                    }
                }
            }
        }
    }

    /// Find the first unclaimed drop across all campaigns.
    fn first_unclaimed_drop(&self) -> Option<(&DropsCampaign, &TimedDrop)> {
        for campaign in self.prioritized_campaigns() {
            if let Some(drop) = campaign.first_unclaimed_drop() {
                return Some((campaign, drop));
            }
        }
        None
    }

    /// Get games that have active drops.
    fn wanted_games(&self) -> Vec<&Game> {
        self.prioritized_campaigns()
            .iter()
            .map(|c| &c.game)
            .collect()
    }
}
