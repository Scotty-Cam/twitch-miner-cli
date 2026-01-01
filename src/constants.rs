//! Core constants for the Twitch Miner CLI.
//!
//! This module contains GQL operation definitions, WebSocket topics,
//! and client configuration data ported from the Python reference implementation.

use std::time::Duration;

/// Interval between watch pulses (simulating viewing)
pub const WATCH_INTERVAL: Duration = Duration::from_secs(59);

/// Interval between WebSocket PINGs
pub const PING_INTERVAL: Duration = Duration::from_secs(180); // 3 minutes

/// Maximum WebSocket connections
pub const MAX_WEBSOCKETS: usize = 8;

/// Topics limit per WebSocket
pub const WS_TOPICS_LIMIT: usize = 50;

/// Maximum extra minutes to track locally before forcing a refresh
pub const MAX_EXTRA_MINUTES: i32 = 15;

// =============================================================================
// Client Configuration
// =============================================================================

/// Client type configuration for Twitch API access.
#[derive(Debug, Clone)]
pub struct ClientInfo {
    pub client_url: &'static str,
    pub client_id: &'static str,
    pub user_agent: &'static str,
}

/// Web client configuration (primary)
pub const CLIENT_WEB: ClientInfo = ClientInfo {
    client_url: "https://www.twitch.tv",
    client_id: "kimne78kx3ncx6brgo4mv6wki5h1ko",
    user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/138.0.0.0 Safari/537.36",
};

/// Mobile web client configuration
pub const CLIENT_MOBILE_WEB: ClientInfo = ClientInfo {
    client_url: "https://m.twitch.tv",
    client_id: "r8s4dac0uhzifbpu9sjdiwzctle17ff",
    user_agent: "Mozilla/5.0 (Linux; Android 16) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/138.0.7204.158 Mobile Safari/537.36",
};

/// Android app client configuration - bypasses integrity checks!
/// This is what TwitchDropsMiner uses by default.
pub const CLIENT_ANDROID_APP: ClientInfo = ClientInfo {
    client_url: "https://www.twitch.tv",
    client_id: "kd1unb4b3q4t58fwlpcbzcbnm76a8fp",
    user_agent: "Dalvik/2.1.0 (Linux; U; Android 16; SM-S911B Build/TP1A.220624.014) tv.twitch.android.app/25.3.0/2503006",
};

// =============================================================================
// GQL Operations
// =============================================================================

/// A GraphQL operation with its persisted query hash.
#[derive(Debug, Clone)]
pub struct GqlOperation {
    pub name: &'static str,
    pub sha256: &'static str,
}

impl GqlOperation {
    pub const fn new(name: &'static str, sha256: &'static str) -> Self {
        Self { name, sha256 }
    }
}

/// All GQL operations used by the miner.
/// The SHA256 hashes are required for Twitch's persisted query system.
pub mod gql_operations {
    use super::GqlOperation;

    /// Returns stream information for a particular channel
    pub const GET_STREAM_INFO: GqlOperation = GqlOperation::new(
        "VideoPlayerStreamInfoOverlayChannel",
        "198492e0857f6aedead9665c81c5a06d67b25b58034649687124083ff288597d",
    );

    /// Claim channel points
    pub const CLAIM_COMMUNITY_POINTS: GqlOperation = GqlOperation::new(
        "ClaimCommunityPoints",
        "46aaeebe02c99afdf4fc97c7c0cba964124bf6b0af229395f1f6d1feed05b3d0",
    );

    /// Claim a drop reward
    pub const CLAIM_DROP: GqlOperation = GqlOperation::new(
        "DropsPage_ClaimDropRewards",
        "a455deea71bdc9015b78eb49f4acfbce8baa7ccbedd28e549bb025bd0f751930",
    );

    /// Returns current state of points for a channel
    pub const CHANNEL_POINTS_CONTEXT: GqlOperation = GqlOperation::new(
        "ChannelPointsContext",
        "374314de591e69925fce3ddc2bcf085796f56ebb8cad67a0daa3165c03adc345",
    );

    /// Returns all in-progress campaigns (inventory)
    pub const INVENTORY: GqlOperation = GqlOperation::new(
        "Inventory",
        "d86775d0ef16a63a33ad52e80eaff963b2d5b72fada7c991504a57496e1d8e4b",
    );

    /// Returns current drop progress for a watched channel
    pub const CURRENT_DROP: GqlOperation = GqlOperation::new(
        "DropCurrentSessionContext",
        "4d06b702d25d652afb9ef835d2a550031f1cf762b193523a92166f40ea3d142b",
    );

    /// Returns all available campaigns
    pub const CAMPAIGNS: GqlOperation = GqlOperation::new(
        "ViewerDropsDashboard",
        "5a4da2ab3d5b47c9f9ce864e727b2cb346af1e3ea8b897fe8f704a97ff017619",
    );

    /// Returns extended information about a campaign
    pub const CAMPAIGN_DETAILS: GqlOperation = GqlOperation::new(
        "DropCampaignDetails",
        "039277bf98f3130929262cc7c6efd9c141ca3749cb6dca442fc8ead9a53f77c1",
    );

    /// Returns drops available for a channel
    pub const AVAILABLE_DROPS: GqlOperation = GqlOperation::new(
        "DropsHighlightService_AvailableDrops",
        "9a62a09bce5b53e26e64a671e530bc599cb6aab1e5ba3cbd5d85966d3940716f",
    );

    /// Returns stream playback access token
    pub const PLAYBACK_ACCESS_TOKEN: GqlOperation = GqlOperation::new(
        "PlaybackAccessToken",
        "ed230aa1e33e07eebb8928504583da78a5173989fadfb1ac94be06a04f3cdbe9",
    );

    /// Returns live channels for a game
    pub const GAME_DIRECTORY: GqlOperation = GqlOperation::new(
        "DirectoryPage_Game",
        "98a996c3c3ebb1ba4fd65d6671c6028d7ee8d615cb540b0731b3db2a911d3649",
    );

    /// Converts game name to game slug
    pub const SLUG_REDIRECT: GqlOperation = GqlOperation::new(
        "DirectoryGameRedirect",
        "1f0300090caceec51f33c5e20647aceff9017f740f223c3c532ba6fa59f6b6cc",
    );
}

// =============================================================================
// WebSocket Topics
// =============================================================================

/// WebSocket topic names for PubSub subscriptions.
pub mod websocket_topics {
    // User topics (use user_id)
    pub const USER_DROPS: &str = "user-drop-events";
    pub const USER_NOTIFICATIONS: &str = "onsite-notifications";
    pub const USER_COMMUNITY_POINTS: &str = "community-points-user-v1";

    // Channel topics (use channel_id)
    pub const CHANNEL_STREAM_STATE: &str = "video-playback-by-id";
    pub const CHANNEL_STREAM_UPDATE: &str = "broadcast-settings-update";
}

/// Format a WebSocket topic string.
pub fn format_topic(topic_name: &str, target_id: u64) -> String {
    format!("{}.{}", topic_name, target_id)
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_info() {
        assert_eq!(CLIENT_WEB.client_id, "kimne78kx3ncx6brgo4mv6wki5h1ko");
        assert!(CLIENT_WEB.client_url.starts_with("https://"));
    }

    #[test]
    fn test_gql_operations_hashes() {
        // Verify critical hashes match the Python implementation exactly
        assert_eq!(
            gql_operations::INVENTORY.sha256,
            "d86775d0ef16a63a33ad52e80eaff963b2d5b72fada7c991504a57496e1d8e4b"
        );
        assert_eq!(
            gql_operations::CLAIM_DROP.sha256,
            "a455deea71bdc9015b78eb49f4acfbce8baa7ccbedd28e549bb025bd0f751930"
        );
        assert_eq!(
            gql_operations::CURRENT_DROP.sha256,
            "4d06b702d25d652afb9ef835d2a550031f1cf762b193523a92166f40ea3d142b"
        );
        assert_eq!(
            gql_operations::PLAYBACK_ACCESS_TOKEN.sha256,
            "ed230aa1e33e07eebb8928504583da78a5173989fadfb1ac94be06a04f3cdbe9"
        );
    }

    #[test]
    fn test_websocket_topic_formatting() {
        let topic = format_topic(websocket_topics::USER_DROPS, 12345678);
        assert_eq!(topic, "user-drop-events.12345678");

        let topic = format_topic(websocket_topics::CHANNEL_STREAM_STATE, 87654321);
        assert_eq!(topic, "video-playback-by-id.87654321");
    }

    #[test]
    fn test_intervals() {
        assert_eq!(WATCH_INTERVAL.as_secs(), 59);
        assert_eq!(PING_INTERVAL.as_secs(), 180);
    }
}
