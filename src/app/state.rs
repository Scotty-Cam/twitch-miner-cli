use super::App;
use crate::auth::AuthState;
use crate::gql::GqlClient;
use crate::watcher::Watcher;

/// Application state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppState {
    Idle,
    InventoryFetch,
    AllCampaignsFetch,
    ChannelSelection,
    Watching,
    /// Login flow in progress
    LoginPending,
    Exit,
}

pub trait StateOps {
    fn is_logged_in(&self) -> bool;
    fn username(&self) -> Option<&str>;
    fn set_auth(&mut self, auth: AuthState);
    fn logout(&mut self);
    fn compact_memory(&mut self);
    fn set_login_pending(&mut self, code: String, uri: String);
    fn clear_login_state(&mut self);
    fn is_login_pending(&self) -> bool;
    fn change_state(&mut self, new_state: AppState);
}

impl StateOps for App {
    /// Check if the user is logged in.
    fn is_logged_in(&self) -> bool {
        self.auth.is_some()
    }

    /// Get the username if logged in.
    fn username(&self) -> Option<&str> {
        self.auth.as_ref().map(|a| a.login.as_str())
    }

    /// Set authentication state (for login).
    fn set_auth(&mut self, auth: AuthState) {
        let gql = GqlClient::new_with_proxy(auth.clone(), self.config.proxy_url.clone());
        let watcher = Watcher::new_with_proxy(auth.clone(), self.config.proxy_url.clone());
        self.auth = Some(auth);
        self.gql = Some(gql);
        self.watcher = Some(watcher);
    }

    /// Clear authentication state (for logout).
    fn logout(&mut self) {
        self.auth = None;
        self.gql = None;
        self.watcher = None;
        self.campaigns.clear();
        self.campaigns.shrink_to_fit();
        self.all_campaigns.clear();
        self.all_campaigns.shrink_to_fit();
        self.drops.clear();
        self.drops.shrink_to_fit();
        self.failed_game_attempts.clear();
        self.failed_game_attempts.shrink_to_fit();
        self.watching_channel = None;
        self.watching_target = None;
        self.clear_login_state();
    }

    /// Compact memory by shrinking all collections to their actual size.
    /// Call this periodically to reduce memory fragmentation.
    fn compact_memory(&mut self) {
        self.campaigns.shrink_to_fit();
        self.all_campaigns.shrink_to_fit();
        self.drops.shrink_to_fit();
        self.failed_game_attempts.shrink_to_fit();
        self.config.priority_games.shrink_to_fit();
        self.config.excluded_games.shrink_to_fit();

        for campaign in &mut self.campaigns {
            campaign.time_based_drops.shrink_to_fit();
        }
        for campaign in &mut self.all_campaigns {
            campaign.time_based_drops.shrink_to_fit();
        }
    }

    /// Set pending login state.
    fn set_login_pending(&mut self, code: String, uri: String) {
        self.login_code = Some(code);
        self.login_status = Some(format!("Waiting for authorization... Go to {}", uri));
        self.login_uri = Some(uri);
    }

    /// Clear login state.
    fn clear_login_state(&mut self) {
        self.login_code = None;
        self.login_uri = None;
        self.login_status = None;
    }

    /// Check if login is pending.
    fn is_login_pending(&self) -> bool {
        self.login_code.is_some()
    }

    /// Change the application state.
    fn change_state(&mut self, new_state: AppState) {
        tracing::info!("State change: {:?} -> {:?}", self.state, new_state);
        self.state = new_state;
    }
}
