//! Inventory models for drops campaigns and timed drops.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A game on Twitch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Game {
    pub id: String,
    /// The display name - ViewerDropsDashboard uses "displayName", Inventory uses "name"
    #[serde(alias = "displayName", alias = "name", default)]
    pub display_name: String,
    #[serde(rename = "boxArtURL")]
    pub box_art_url: Option<String>,
    pub slug: Option<String>,
}

/// A drops campaign.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropsCampaign {
    pub id: String,
    pub name: String,
    pub game: Game,
    #[serde(rename = "startAt")]
    pub starts_at: DateTime<Utc>,
    #[serde(rename = "endAt")]
    pub ends_at: DateTime<Utc>,
    pub status: String,
    /// Time-based drops - not included in ViewerDropsDashboard, only in detailed view
    #[serde(rename = "timeBasedDrops", default)]
    pub time_based_drops: Vec<TimedDrop>,
    #[serde(rename = "self")]
    pub self_info: Option<CampaignSelfInfo>,
}

impl DropsCampaign {
    /// Check if the campaign is currently active.
    pub fn is_active(&self) -> bool {
        let now = Utc::now();
        self.starts_at <= now && now <= self.ends_at && self.status == "ACTIVE"
    }

    /// Check if the campaign is upcoming.
    pub fn is_upcoming(&self) -> bool {
        Utc::now() < self.starts_at
    }

    /// Check if the campaign has expired.
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.ends_at
    }

    /// Get the total required minutes for all drops.
    pub fn total_required_minutes(&self) -> i32 {
        self.time_based_drops
            .iter()
            .map(|d| d.required_minutes)
            .sum()
    }

    /// Get the first unclaimed drop (prioritizing lowest remaining minutes).
    pub fn first_unclaimed_drop(&self) -> Option<&TimedDrop> {
        self.time_based_drops
            .iter()
            .filter(|d| !d.is_claimed())
            .min_by(|a, b| {
                a.remaining_minutes()
                    .partial_cmp(&b.remaining_minutes())
                    .unwrap()
            })
    }

    /// Get the count of claimed drops.
    pub fn claimed_drops_count(&self) -> usize {
        self.time_based_drops
            .iter()
            .filter(|d| d.is_claimed())
            .count()
    }

    /// Get total number of drops.
    pub fn total_drops_count(&self) -> usize {
        self.time_based_drops.len()
    }

    /// Get overall campaign progress as a percentage (0.0 - 1.0).
    /// TwitchDropsMiner formula: average of ALL drops' individual progress.
    /// Claimed drops = 1.0 (100%), unclaimed drops = their individual progress.
    pub fn campaign_progress(&self) -> f64 {
        if self.time_based_drops.is_empty() {
            return 0.0;
        }

        let total_progress: f64 = self.time_based_drops.iter().map(|d| d.progress()).sum();

        total_progress / self.time_based_drops.len() as f64
    }

    /// Get the total remaining minutes for the campaign (sum of all unclaimed drops).
    pub fn campaign_remaining_minutes(&self) -> f64 {
        self.time_based_drops
            .iter()
            .filter(|d| !d.is_claimed())
            .map(|d| d.remaining_minutes())
            .sum()
    }

    /// Get the time remaining for the campaign as a formatted H:MM:SS string.
    pub fn time_remaining(&self) -> String {
        let remaining_secs = (self.campaign_remaining_minutes() * 60.0).round() as i32;
        let hours = remaining_secs / 3600;
        let mins = (remaining_secs % 3600) / 60;
        let secs = remaining_secs % 60;
        format!("{}:{:02}:{:02} remaining", hours, mins, secs)
    }

    /// Check if the campaign is fully completed (all drops claimed).
    pub fn is_completed(&self) -> bool {
        if self.time_based_drops.is_empty() {
            return false;
        }
        self.claimed_drops_count() == self.total_drops_count()
    }
}

/// Self-referential info about user's campaign status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CampaignSelfInfo {
    #[serde(rename = "isAccountConnected")]
    pub is_account_connected: bool,
}

/// A timed drop within a campaign.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimedDrop {
    pub id: String,
    pub name: String,
    #[serde(rename = "requiredMinutesWatched")]
    pub required_minutes: i32,
    #[serde(rename = "startAt")]
    pub starts_at: DateTime<Utc>,
    #[serde(rename = "endAt")]
    pub ends_at: DateTime<Utc>,
    #[serde(rename = "benefitEdges")]
    pub benefit_edges: Vec<BenefitEdge>,
    #[serde(rename = "self")]
    pub self_info: Option<DropSelfInfo>,
    #[serde(skip)]
    pub extra_minutes: i32,
    #[serde(skip)]
    pub extra_seconds: i32,
}

impl TimedDrop {
    /// Get current watched minutes (base + local extra).
    /// Returns float to represent fractional minutes from seconds.
    pub fn current_minutes(&self) -> f64 {
        let base = self
            .self_info
            .as_ref()
            .map(|s| s.current_minutes_watched)
            .unwrap_or(0);
        base as f64 + self.extra_minutes as f64 + (self.extra_seconds as f64 / 60.0)
    }

    /// Get remaining minutes to complete the drop.
    pub fn remaining_minutes(&self) -> f64 {
        (self.required_minutes as f64 - self.current_minutes()).max(0.0)
    }

    /// Get progress as a percentage (0.0 - 1.0).
    pub fn progress(&self) -> f64 {
        if self.required_minutes == 0 {
            return 1.0;
        }
        (self.current_minutes() / self.required_minutes as f64).min(1.0)
    }

    /// Check if the drop has been claimed OR is at 100% (effectively complete).
    /// This is used by first_unclaimed_drop() to skip completed drops.
    pub fn is_claimed(&self) -> bool {
        let explicitly_claimed = self
            .self_info
            .as_ref()
            .map(|s| s.is_claimed)
            .unwrap_or(false);

        // Also consider as "claimed" if we're at 100% progress
        // This handles the case where claim succeeded but local data wasn't updated
        let at_full_progress =
            self.required_minutes > 0 && self.current_minutes() >= self.required_minutes as f64;

        explicitly_claimed || at_full_progress
    }

    /// Check if the drop is ready to be claimed.
    pub fn can_claim(&self) -> bool {
        if let Some(info) = &self.self_info {
            info.current_minutes_watched >= self.required_minutes
                && !info.is_claimed
                && info.drop_instance_id.is_some()
        } else {
            false
        }
    }

    /// Get the drop instance ID for claiming.
    pub fn drop_instance_id(&self) -> Option<&str> {
        self.self_info
            .as_ref()
            .and_then(|s| s.drop_instance_id.as_deref())
    }

    /// Format the remaining time as H:MM:SS like TwitchDropsMiner.
    pub fn time_remaining_display(&self) -> String {
        let remaining_secs = (self.remaining_minutes() * 60.0).round() as i32;
        let hours = remaining_secs / 3600;
        let mins = (remaining_secs % 3600) / 60;
        let secs = remaining_secs % 60;
        format!(
            "{} {:02}:{:02}:{:02} remaining",
            if remaining_secs <= 0 { "Done!" } else { "" },
            hours,
            mins,
            secs
        )
        .trim()
        .to_string()
    }

    /// Total remaining minutes including any precondition drops.
    /// For TDM compatibility - simplified version without precondition chaining.
    pub fn total_remaining_minutes(&self, _campaign: &super::DropsCampaign) -> f64 {
        // Simplified: just return this drop's remaining minutes
        // Full TDM version chains precondition drops' remaining time
        self.remaining_minutes()
    }

    /// Bump extra minutes locally.
    pub fn bump_extra_minute(&mut self) {
        if self.extra_minutes < crate::constants::MAX_EXTRA_MINUTES {
            self.extra_minutes += 1;
        }
    }

    /// Bump extra seconds locally. Integers!
    pub fn bump_extra_second(&mut self) {
        if self.extra_minutes < crate::constants::MAX_EXTRA_MINUTES {
            self.extra_seconds += 1;
            if self.extra_seconds >= 60 {
                self.extra_minutes += 1;
                self.extra_seconds = 0;
            }
        }
    }

    /// Reset extra minutes (e.g. after API refresh).
    pub fn reset_local_tracking(&mut self) {
        self.extra_minutes = 0;
        self.extra_seconds = 0;
    }
}

/// Self-referential info about user's drop progress.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropSelfInfo {
    #[serde(rename = "currentMinutesWatched")]
    pub current_minutes_watched: i32,
    #[serde(rename = "isClaimed")]
    pub is_claimed: bool,
    #[serde(rename = "dropInstanceID")]
    pub drop_instance_id: Option<String>,
}

/// A benefit edge (reward info).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenefitEdge {
    pub benefit: Benefit,
}

/// A benefit (reward).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Benefit {
    pub id: String,
    pub name: String,
    #[serde(rename = "imageAssetURL")]
    pub image_url: Option<String>,
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_campaign_parsing() {
        let json = r#"{
            "id": "campaign-123",
            "name": "Test Campaign",
            "game": {
                "id": "game-456",
                "name": "Test Game"
            },
            "startAt": "2024-01-01T00:00:00Z",
            "endAt": "2024-12-31T23:59:59Z",
            "status": "ACTIVE",
            "timeBasedDrops": []
        }"#;

        let campaign: DropsCampaign = serde_json::from_str(json).unwrap();
        assert_eq!(campaign.id, "campaign-123");
        assert_eq!(campaign.game.display_name, "Test Game");
        assert_eq!(campaign.status, "ACTIVE");
    }

    #[test]
    fn test_timed_drop_progress() {
        let drop = TimedDrop {
            id: "drop-1".to_string(),
            name: "Test Drop".to_string(),
            required_minutes: 60,
            starts_at: Utc::now(),
            ends_at: Utc::now(),
            benefit_edges: vec![],
            self_info: Some(DropSelfInfo {
                current_minutes_watched: 30,
                is_claimed: false,
                drop_instance_id: None,
            }),
            extra_minutes: 0,
            extra_seconds: 0,
        };

        assert_eq!(drop.current_minutes(), 30.0);
        assert_eq!(drop.remaining_minutes(), 30.0);
        assert!((drop.progress() - 0.5).abs() < 0.001);
        assert!(!drop.is_claimed());
        assert!(!drop.can_claim());
    }

    #[test]
    fn test_drop_can_claim() {
        let drop = TimedDrop {
            id: "drop-1".to_string(),
            name: "Test Drop".to_string(),
            required_minutes: 60,
            starts_at: Utc::now(),
            ends_at: Utc::now(),
            benefit_edges: vec![],
            self_info: Some(DropSelfInfo {
                current_minutes_watched: 60,
                is_claimed: false,
                drop_instance_id: Some("instance-123".to_string()),
            }),
            extra_minutes: 0,
            extra_seconds: 0,
        };

        assert!(drop.can_claim());
        assert_eq!(drop.drop_instance_id(), Some("instance-123"));
    }

    #[test]
    fn test_campaign_total_minutes() {
        let campaign = DropsCampaign {
            id: "c1".to_string(),
            name: "Campaign".to_string(),
            game: Game {
                id: "g1".to_string(),
                display_name: "Game".to_string(),
                box_art_url: None,
                slug: None,
            },
            starts_at: Utc::now(),
            ends_at: Utc::now(),
            status: "ACTIVE".to_string(),
            time_based_drops: vec![
                TimedDrop {
                    id: "d1".to_string(),
                    name: "Drop 1".to_string(),
                    required_minutes: 30,
                    starts_at: Utc::now(),
                    ends_at: Utc::now(),
                    benefit_edges: vec![],
                    self_info: None,
                    extra_minutes: 0,
                    extra_seconds: 0,
                },
                TimedDrop {
                    id: "d2".to_string(),
                    name: "Drop 2".to_string(),
                    required_minutes: 60,
                    starts_at: Utc::now(),
                    ends_at: Utc::now(),
                    benefit_edges: vec![],
                    self_info: None,
                    extra_minutes: 0,
                    extra_seconds: 0,
                },
            ],
            self_info: None,
        };

        assert_eq!(campaign.total_required_minutes(), 90);
    }
}

/// A drop reward from game event drops (claimed items).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameEventDrop {
    pub id: String,
    pub name: String,
    #[serde(rename = "lastAwardedAt")]
    pub last_awarded_at: DateTime<Utc>,
    #[serde(rename = "totalCount")]
    pub total_count: i32,
}

/// Inventory containing various campaign types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Inventory {
    #[serde(rename = "dropCampaignsInProgress")]
    pub drop_campaigns_in_progress: Option<Vec<DropsCampaign>>,
    #[serde(rename = "gameEventDrops")]
    pub game_event_drops: Option<Vec<GameEventDrop>>,
}
