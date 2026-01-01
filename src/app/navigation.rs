use super::App;
use super::{CampaignOps, WatcherOps};
use crate::models::DropsCampaign;
use anyhow::Result;
use std::collections::HashMap;

/// Navigation pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Home,
    Drops,
    Settings,
    About,
}

/// Focus areas for the Home page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HomeFocus {
    #[default]
    Watching,
    Inactive,
}

/// Focus areas for the Drops page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DropsFocus {
    #[default]
    AllDrops,
    SubscribedDrops,
}

/// Focus areas for the Settings page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsFocus {
    #[default]
    Settings, // Left panel - settings list
    Help, // Right panel - help text (read-only)
}

/// Selectable items in the Settings page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsItem {
    #[default]
    AccountSettings,
    Notifications,
    LogoAnimation,
    ProxySettings,
}

pub trait NavigationOps {
    fn navigate_to(&mut self, page: Page);
    fn cycle_settings_focus(&mut self);
    fn move_settings_selection_up(&mut self);
    fn move_settings_selection_down(&mut self);
    fn toggle_logo_animation(&mut self) -> String;
    fn scroll_about_up(&mut self);
    fn scroll_about_down(&mut self, visible_height: u16);
    fn start_proxy_edit(&mut self);
    fn save_proxy(&mut self) -> String;
    fn cancel_proxy_edit(&mut self);
    fn is_proxy_editing(&self) -> bool;
    fn move_subscribed_game_up(&mut self) -> bool;
    fn move_subscribed_game_down(&mut self) -> bool;
    fn toggle_notifications(&mut self) -> String;
    fn cycle_home_focus(&mut self);
    fn move_home_selection_up(&mut self);
    fn move_home_selection_down(&mut self);
    fn get_inactive_header_indices(&self) -> Vec<usize>;
    fn get_watching_item_count(&self) -> usize;
    fn cycle_drops_focus(&mut self);
    fn move_drops_selection_up(&mut self);
    fn move_drops_selection_down(&mut self);
    fn get_drops_all_games(&self) -> Vec<String>;
    fn get_drops_subscribed_games(&self) -> Vec<String>;
    fn get_game_campaigns_string(&self, game_name: &str) -> String;
    fn toggle_drops_subscription(&mut self) -> Result<(String, bool)>;
}

impl NavigationOps for App {
    /// Navigate to a different page.
    fn navigate_to(&mut self, page: Page) {
        self.page = page;
    }

    /// Toggle focus between Settings and Help panels
    fn cycle_settings_focus(&mut self) {
        self.settings_focus = match self.settings_focus {
            SettingsFocus::Settings => SettingsFocus::Help,
            SettingsFocus::Help => SettingsFocus::Settings,
        };
    }

    /// Move settings selection up
    fn move_settings_selection_up(&mut self) {
        self.settings_selected = match self.settings_selected {
            SettingsItem::AccountSettings => SettingsItem::AccountSettings, // Already at top
            SettingsItem::Notifications => SettingsItem::AccountSettings,
            SettingsItem::LogoAnimation => SettingsItem::Notifications,
            SettingsItem::ProxySettings => SettingsItem::LogoAnimation,
        };
    }

    /// Move settings selection down
    fn move_settings_selection_down(&mut self) {
        self.settings_selected = match self.settings_selected {
            SettingsItem::AccountSettings => SettingsItem::Notifications,
            SettingsItem::Notifications => SettingsItem::LogoAnimation,
            SettingsItem::LogoAnimation => SettingsItem::ProxySettings,
            SettingsItem::ProxySettings => SettingsItem::ProxySettings, // Already at bottom
        };
    }

    /// Toggle logo animation on/off
    fn toggle_logo_animation(&mut self) -> String {
        self.config.logo_animation_enabled = !self.config.logo_animation_enabled;
        let _ = self.config.save();
        if self.config.logo_animation_enabled {
            "Logo animation enabled.".to_string()
        } else {
            "Logo animation disabled.".to_string()
        }
    }

    /// Scroll the About page up
    fn scroll_about_up(&mut self) {
        self.about_scroll = self.about_scroll.saturating_sub(1);
    }

    /// Scroll the About page down (clamped to content length)
    fn scroll_about_down(&mut self, visible_height: u16) {
        use crate::ui::about::ABOUT_CONTENT_LINES;
        // Only scroll if there's content below the visible area
        let max_scroll = ABOUT_CONTENT_LINES.saturating_sub(visible_height);
        if self.about_scroll < max_scroll {
            self.about_scroll = self.about_scroll.saturating_add(1);
        }
    }

    /// Start editing the proxy URL
    fn start_proxy_edit(&mut self) {
        self.proxy_editing = true;
        self.proxy_input = self.config.proxy_url.clone().unwrap_or_default();
    }

    /// Save the proxy URL from input buffer
    fn save_proxy(&mut self) -> String {
        self.proxy_editing = false;
        let input = self.proxy_input.trim().to_string();

        if input.is_empty() {
            self.config.proxy_url = None;
        } else {
            self.config.proxy_url = Some(input);
        }

        if let Err(e) = self.config.save() {
            return format!("Failed to save proxy: {}", e);
        }

        "Proxy saved. Please restart to apply.".to_string()
    }

    /// Cancel editing the proxy URL
    fn cancel_proxy_edit(&mut self) {
        self.proxy_editing = false;
        self.proxy_input.clear();
    }

    /// Check if currently editing proxy
    fn is_proxy_editing(&self) -> bool {
        self.proxy_editing
    }

    /// Move the selected subscribed game up in priority.
    /// Returns true if changed.
    fn move_subscribed_game_up(&mut self) -> bool {
        let max_idx = self.config.priority_games.len().saturating_sub(1);
        if self.drops_subscribed_selected == 0 {
            return false;
        }
        if self.drops_subscribed_selected > max_idx {
            return false;
        }

        let idx = self.drops_subscribed_selected;
        self.config.priority_games.swap(idx, idx - 1);
        self.drops_subscribed_selected -= 1;

        if let Err(e) = self.config.save() {
            tracing::error!("Failed to save config after moving priority: {}", e);
        }
        true
    }

    /// Move the selected subscribed game down in priority.
    /// Returns true if changed.
    fn move_subscribed_game_down(&mut self) -> bool {
        let max_idx = self.config.priority_games.len().saturating_sub(1);
        if self.drops_subscribed_selected >= max_idx {
            return false;
        }

        let idx = self.drops_subscribed_selected;
        self.config.priority_games.swap(idx, idx + 1);
        self.drops_subscribed_selected += 1;

        if let Err(e) = self.config.save() {
            tracing::error!("Failed to save config after moving priority: {}", e);
        }
        true
    }

    /// Toggle notifications on/off
    fn toggle_notifications(&mut self) -> String {
        self.config.notifications_enabled = !self.config.notifications_enabled;
        let _ = self.config.save();
        if self.config.notifications_enabled {
            "Notifications enabled.".to_string()
        } else {
            "Notifications disabled.".to_string()
        }
    }

    /// Toggle focus between Watching and Inactive panels on Home
    fn cycle_home_focus(&mut self) {
        self.home_focus = match self.home_focus {
            HomeFocus::Watching => HomeFocus::Inactive,
            HomeFocus::Inactive => HomeFocus::Watching,
        };
    }

    /// Move Home selection up
    fn move_home_selection_up(&mut self) {
        match self.home_focus {
            HomeFocus::Watching => {
                if self.home_watching_selected > 0 {
                    self.home_watching_selected -= 1;
                }
            }
            HomeFocus::Inactive => {
                // Smart scroll: Jump to previous Game Header
                let headers = self.get_inactive_header_indices();
                if let Some(current_pos) = headers
                    .iter()
                    .rposition(|&x| x < self.home_inactive_selected)
                {
                    self.home_inactive_selected = headers[current_pos];
                } else if !headers.is_empty() {
                    // unexpected, just go to first
                    self.home_inactive_selected = headers[0];
                }
            }
        }
    }

    /// Move Home selection down
    fn move_home_selection_down(&mut self) {
        match self.home_focus {
            HomeFocus::Watching => {
                let max = self.get_watching_item_count().saturating_sub(1);
                if self.home_watching_selected < max {
                    self.home_watching_selected += 1;
                }
            }
            HomeFocus::Inactive => {
                // Smart scroll: Jump to next Game Header
                let headers = self.get_inactive_header_indices();
                if let Some(current_pos) = headers
                    .iter()
                    .position(|&x| x > self.home_inactive_selected)
                {
                    self.home_inactive_selected = headers[current_pos];
                } else if let Some(last) = headers.last() {
                    // ensure we don't go past last header (or stay there)
                    if self.home_inactive_selected < *last {
                        self.home_inactive_selected = *last;
                    }
                }
            }
        }
    }

    /// Get indices of Game Headers in the Inactive list
    fn get_inactive_header_indices(&self) -> Vec<usize> {
        let subscribed = self.subscribed_campaigns_with_progress();
        let active_game = self
            .mining_status
            .as_ref()
            .map(|s| s.game_name.as_str())
            .or(self.current_attempt_game.as_deref());

        // Group and sort exactly as in dashboard.rs
        let mut games: HashMap<&str, Vec<&DropsCampaign>> = HashMap::new();
        for campaign in &subscribed {
            if campaign.is_active() && Some(campaign.game.display_name.as_str()) != active_game {
                games
                    .entry(&campaign.game.display_name)
                    .or_default()
                    .push(campaign);
            }
        }

        let mut game_names: Vec<&str> = games.keys().copied().collect();
        // Sort Logic: Match dashboard.rs exactly
        // 1. Games not fully complete (have active/unclaimed drops) come FIRST.
        // 2. Alphabetical within groups.
        game_names.sort_by(|a, b| {
            let a_has_drops = games
                .get(a)
                .map(|list| list.iter().any(|c| !c.is_completed()))
                .unwrap_or(false);
            let b_has_drops = games
                .get(b)
                .map(|list| list.iter().any(|c| !c.is_completed()))
                .unwrap_or(false);

            b_has_drops
                .cmp(&a_has_drops)
                .then_with(|| a.to_lowercase().cmp(&b.to_lowercase()))
        });

        let mut indices = Vec::new();
        let mut current_idx = 0;

        for game_name in game_names {
            indices.push(current_idx); // Header is here

            // Calculate size of this group
            // Header (1) + Campaigns (N) + Spacer (1)
            let count = games.get(game_name).map(|v| v.len()).unwrap_or(0);
            current_idx += 1 + count + 1;
        }

        if indices.is_empty() {
            vec![0] // Default to 0 if empty
        } else {
            indices
        }
    }

    /// Calculate item count for Watching list (UI logic replication)
    fn get_watching_item_count(&self) -> usize {
        // Logic should match render_watching_panel
        // Currently we only show ONE campaign detailed view (which is treated as one item or multiple?)
        // If we want to scroll the detailed view... it's just one item per campaign?
        // Let's assume 1 item per active campaign for the active game.
        let subscribed = self.subscribed_campaigns_with_progress();
        let active_game = self
            .mining_status
            .as_ref()
            .map(|s| s.game_name.as_str())
            .or(self.current_attempt_game.as_deref());

        let count = subscribed
            .iter()
            .filter(|c| Some(c.game.display_name.as_str()) == active_game)
            .count();

        // If 0, we might show a message (1 item)
        if count == 0 {
            1
        } else {
            count
        }
        // Note: Render logic currently picks ONE. I should update render logic to show list if I want scrolling.
        // For now, let's say count is 1.
    }

    /// Toggle focus between All Drops and Subscribed Drops
    fn cycle_drops_focus(&mut self) {
        self.drops_focus = match self.drops_focus {
            DropsFocus::AllDrops => DropsFocus::SubscribedDrops,
            DropsFocus::SubscribedDrops => DropsFocus::AllDrops,
        };
    }

    /// Move Drops selection up
    fn move_drops_selection_up(&mut self) {
        match self.drops_focus {
            DropsFocus::AllDrops => {
                if self.drops_all_selected > 0 {
                    self.drops_all_selected -= 1;
                }
            }
            DropsFocus::SubscribedDrops => {
                if self.drops_subscribed_selected > 0 {
                    self.drops_subscribed_selected -= 1;
                }
            }
        }
    }

    /// Move Drops selection down
    fn move_drops_selection_down(&mut self) {
        match self.drops_focus {
            DropsFocus::AllDrops => {
                let count = self.get_drops_all_games().len();
                if count > 0 && self.drops_all_selected < count - 1 {
                    self.drops_all_selected += 1;
                }
            }
            DropsFocus::SubscribedDrops => {
                let count = self.get_drops_subscribed_games().len();
                if count > 0 && self.drops_subscribed_selected < count - 1 {
                    self.drops_subscribed_selected += 1;
                }
            }
        }
    }

    /// Get list of consolidated games for "All Drops" panel (Left)
    /// Excludes subscribed games.
    fn get_drops_all_games(&self) -> Vec<String> {
        let mut games = std::collections::HashSet::new();
        // Check active campaigns first
        for campaign in &self.campaigns {
            if campaign.is_active()
                && !self
                    .config
                    .priority_games
                    .contains(&campaign.game.display_name)
            {
                games.insert(campaign.game.display_name.clone());
            }
        }
        // Check all campaigns (some might be inactive but we show them if we have "None" logic?)
        // Requirement: "If a game has campaigns multiple (none) then just show none once"
        // So we should show games that are present in our known list?
        // Let's stick to showing games from 'all_campaigns' as well, if not subscribed.
        for campaign in &self.all_campaigns {
            if !self
                .config
                .priority_games
                .contains(&campaign.game.display_name)
            {
                games.insert(campaign.game.display_name.clone());
            }
        }

        let mut game_list: Vec<String> = games.into_iter().collect();
        game_list.sort_by_key(|a| a.to_lowercase());
        game_list
    }

    /// Get list of consolidated games for "Subscribed Drops" panel (Right)
    /// Only subscribed games. Preserves priority order (first = highest priority).
    fn get_drops_subscribed_games(&self) -> Vec<String> {
        // Return priority_games directly to preserve order for reordering
        self.config.priority_games.clone()
    }

    /// Get format string for campaigns of a specific game
    fn get_game_campaigns_string(&self, game_name: &str) -> String {
        let mut campaign_names = Vec::new();

        // Collect all campaigns for this game
        // Prefer active ones from 'campaigns' inventory first
        for campaign in &self.campaigns {
            if campaign.game.display_name == game_name && campaign.is_active() {
                campaign_names.push(campaign.name.clone());
            }
        }

        // If no active ones found in inventory, check all_campaigns
        if campaign_names.is_empty() {
            for campaign in &self.all_campaigns {
                if campaign.game.display_name == game_name && campaign.is_active() {
                    campaign_names.push(campaign.name.clone());
                }
            }
        }

        if campaign_names.is_empty() {
            // Maybe it's not active anymore?
            return "No Active Campaigns".to_string();
        }

        campaign_names.dedup();
        campaign_names.join(", ")
    }

    /// Toggle subscription for the currently selected Drop (Left or Right panel)
    fn toggle_drops_subscription(&mut self) -> Result<(String, bool)> {
        let game_name = match self.drops_focus {
            DropsFocus::AllDrops => {
                let games = self.get_drops_all_games();
                games.get(self.drops_all_selected).cloned()
            }
            DropsFocus::SubscribedDrops => {
                let games = self.get_drops_subscribed_games();
                games.get(self.drops_subscribed_selected).cloned()
            }
        };

        if let Some(name) = game_name {
            // Use CampaignOps method
            let message = self.toggle_game_subscription(name.clone());

            // Check if we unsubscribed
            if !self.config.priority_games.contains(&name) {
                // Unsubscribed
                let active_game = self
                    .mining_status
                    .as_ref()
                    .map(|s| s.game_name.as_str())
                    .or(self.current_attempt_game.as_deref());

                if Some(name.as_str()) == active_game {
                    self.stop_watching();
                }

                if self.drops_subscribed_selected >= self.config.priority_games.len()
                    && self.drops_subscribed_selected > 0
                {
                    self.drops_subscribed_selected -= 1;
                }
            }

            Ok((message, true))
        } else {
            Ok(("No game selected".to_string(), false))
        }
    }
}
