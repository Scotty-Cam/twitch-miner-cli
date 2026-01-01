//! Channel and Stream models.

use serde::{Deserialize, Serialize};

use super::inventory::Game;

/// A Twitch channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channel {
    pub id: String,
    #[serde(rename = "login")]
    pub login: String,
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
    #[serde(rename = "profileImageURL")]
    pub profile_image_url: Option<String>,
}

impl Channel {
    /// Get the channel's display name, falling back to login.
    pub fn name(&self) -> &str {
        self.display_name.as_deref().unwrap_or(&self.login)
    }

    /// Get the channel URL.
    pub fn url(&self) -> String {
        format!("https://www.twitch.tv/{}", self.login)
    }
}

/// A live stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stream {
    pub id: String,
    #[serde(rename = "broadcaster")]
    pub channel: Channel,
    pub game: Option<Game>,
    #[serde(rename = "viewersCount")]
    pub viewers: i32,
    pub title: Option<String>,
}

impl Stream {
    /// Check if the stream is playing a specific game.
    pub fn is_playing_game(&self, game_name: &str) -> bool {
        self.game
            .as_ref()
            .map(|g| g.display_name.eq_ignore_ascii_case(game_name))
            .unwrap_or(false)
    }
}

/// Stream status from WebSocket events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamStatus {
    Online,
    Offline,
}

/// Parsed channel info from directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryChannel {
    pub id: String,
    pub login: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "viewersCount")]
    pub viewers: i32,
    pub title: Option<String>,
    #[serde(rename = "dropsEnabled")]
    pub drops_enabled: bool,
}

impl DirectoryChannel {
    /// Convert to a Channel.
    pub fn to_channel(&self) -> Channel {
        Channel {
            id: self.id.clone(),
            login: self.login.clone(),
            display_name: Some(self.display_name.clone()),
            profile_image_url: None,
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channel_name() {
        let channel = Channel {
            id: "123".to_string(),
            login: "testuser".to_string(),
            display_name: Some("Test User".to_string()),
            profile_image_url: None,
        };
        assert_eq!(channel.name(), "Test User");

        let channel_no_display = Channel {
            id: "123".to_string(),
            login: "testuser".to_string(),
            display_name: None,
            profile_image_url: None,
        };
        assert_eq!(channel_no_display.name(), "testuser");
    }

    #[test]
    fn test_channel_url() {
        let channel = Channel {
            id: "123".to_string(),
            login: "streamer".to_string(),
            display_name: None,
            profile_image_url: None,
        };
        assert_eq!(channel.url(), "https://www.twitch.tv/streamer");
    }

    #[test]
    fn test_stream_is_playing_game() {
        let stream = Stream {
            id: "stream-1".to_string(),
            channel: Channel {
                id: "123".to_string(),
                login: "streamer".to_string(),
                display_name: None,
                profile_image_url: None,
            },
            game: Some(Game {
                id: "game-1".to_string(),
                display_name: "Fortnite".to_string(),
                box_art_url: None,
                slug: None,
            }),
            viewers: 1000,
            title: Some("Playing Fortnite!".to_string()),
        };

        assert!(stream.is_playing_game("Fortnite"));
        assert!(stream.is_playing_game("fortnite")); // case insensitive
        assert!(!stream.is_playing_game("Minecraft"));
    }

    #[test]
    fn test_directory_channel_parsing() {
        let json = r#"{
            "id": "12345",
            "login": "streamer",
            "displayName": "Cool Streamer",
            "viewersCount": 5000,
            "title": "Playing games!",
            "dropsEnabled": true
        }"#;

        let dir_channel: DirectoryChannel = serde_json::from_str(json).unwrap();
        assert_eq!(dir_channel.login, "streamer");
        assert!(dir_channel.drops_enabled);

        let channel = dir_channel.to_channel();
        assert_eq!(channel.name(), "Cool Streamer");
    }
}
