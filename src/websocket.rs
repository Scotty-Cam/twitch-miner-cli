//! WebSocket module for Twitch PubSub connections.
//!
//! Handles real-time events for drops progress, stream status, and notifications.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{mpsc, RwLock};
use tokio::time::{interval, timeout};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::constants::PING_INTERVAL;

const PUBSUB_URL: &str = "wss://pubsub-edge.twitch.tv/v1";
const PING_TIMEOUT: Duration = Duration::from_secs(10);

// =============================================================================
// Message Types
// =============================================================================

/// Outgoing WebSocket message.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
enum OutgoingMessage {
    #[serde(rename = "PING")]
    Ping,
    #[serde(rename = "LISTEN")]
    Listen { data: ListenData },
    #[allow(dead_code)]
    #[serde(rename = "UNLISTEN")]
    Unlisten { data: UnlistenData },
}

#[derive(Debug, Clone, Serialize)]
struct ListenData {
    topics: Vec<String>,
    auth_token: String,
}

#[derive(Debug, Clone, Serialize)]
struct UnlistenData {
    topics: Vec<String>,
}

/// Incoming WebSocket message.
#[derive(Debug, Clone, Deserialize)]
pub struct IncomingMessage {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub data: Option<MessageData>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MessageData {
    pub topic: String,
    pub message: String,
}

/// Parsed PubSub event.
#[derive(Debug, Clone)]
pub enum PubSubEvent {
    /// Drop progress update
    DropProgress {
        drop_id: String,
        current_minutes: i32,
    },
    /// Drop ready to claim
    DropReady { drop_instance_id: String },
    /// Stream went online
    StreamOnline { channel_id: u64 },
    /// Stream went offline
    StreamOffline { channel_id: u64 },
    /// Unknown event
    Unknown(Value),
}

// =============================================================================
// WebSocket Manager
// =============================================================================

/// Manager for PubSub WebSocket connections.
pub struct WebSocketManager {
    access_token: String,
    topics: Arc<RwLock<HashMap<String, bool>>>,
    event_tx: mpsc::Sender<PubSubEvent>,
}

impl WebSocketManager {
    /// Create a new WebSocket manager.
    pub fn new(access_token: String) -> (Self, mpsc::Receiver<PubSubEvent>) {
        let (tx, rx) = mpsc::channel(100);
        (
            Self {
                access_token,
                topics: Arc::new(RwLock::new(HashMap::new())),
                event_tx: tx,
            },
            rx,
        )
    }

    /// Add topics to listen to.
    pub async fn add_topics(&self, topics: Vec<String>) {
        let mut current = self.topics.write().await;
        for topic in topics {
            current.insert(topic, false); // false = not yet subscribed
        }
    }

    /// Run the WebSocket connection loop.
    pub async fn run(&self) -> Result<()> {
        loop {
            match self.connect_and_handle().await {
                Ok(()) => {
                    tracing::info!("WebSocket connection closed gracefully");
                    break;
                }
                Err(e) => {
                    tracing::warn!("WebSocket error: {}. Reconnecting in 5s...", e);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
        Ok(())
    }

    /// Connect and handle messages.
    async fn connect_and_handle(&self) -> Result<()> {
        let (ws_stream, _) = connect_async(PUBSUB_URL)
            .await
            .context("Failed to connect to PubSub")?;

        tracing::info!("Connected to Twitch PubSub");

        let (mut write, mut read) = ws_stream.split();
        let mut ping_interval = interval(PING_INTERVAL);

        // Subscribe to pending topics
        let topics_to_subscribe: Vec<String> = {
            let mut topics = self.topics.write().await;
            topics
                .iter_mut()
                .filter(|(_, subscribed)| !**subscribed)
                .map(|(topic, subscribed)| {
                    *subscribed = true;
                    topic.clone()
                })
                .collect()
        };

        if !topics_to_subscribe.is_empty() {
            let listen_msg = OutgoingMessage::Listen {
                data: ListenData {
                    topics: topics_to_subscribe.clone(),
                    auth_token: self.access_token.clone(),
                },
            };
            let json = serde_json::to_string(&listen_msg)?;
            write.send(Message::Text(json)).await?;
            tracing::debug!("Subscribed to {} topics", topics_to_subscribe.len());
        }

        loop {
            tokio::select! {
                _ = ping_interval.tick() => {
                    let ping_msg = OutgoingMessage::Ping;
                    let json = serde_json::to_string(&ping_msg)?;
                    write.send(Message::Text(json)).await?;
                    tracing::trace!("Sent PING");
                }
                msg = timeout(PING_TIMEOUT * 2, read.next()) => {
                    match msg {
                        Ok(Some(Ok(Message::Text(text)))) => {
                            self.handle_message(&text).await?;
                        }
                        Ok(Some(Ok(Message::Close(_)))) => {
                            tracing::info!("WebSocket closed by server");
                            return Ok(());
                        }
                        Ok(Some(Err(e))) => {
                            return Err(anyhow!("WebSocket error: {}", e));
                        }
                        Ok(None) => {
                            return Err(anyhow!("WebSocket stream ended"));
                        }
                        Err(_) => {
                            return Err(anyhow!("WebSocket read timeout"));
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    /// Handle an incoming message.
    async fn handle_message(&self, text: &str) -> Result<()> {
        let msg: IncomingMessage = serde_json::from_str(text)?;

        match msg.msg_type.as_str() {
            "PONG" => {
                tracing::trace!("Received PONG");
            }
            "RESPONSE" => {
                if let Some(error) = msg.error {
                    tracing::warn!("PubSub error: {}", error);
                }
            }
            "MESSAGE" => {
                if let Some(data) = msg.data {
                    let event = self.parse_event(&data.topic, &data.message)?;
                    let _ = self.event_tx.send(event).await;
                }
            }
            _ => {
                tracing::debug!("Unknown message type: {}", msg.msg_type);
            }
        }

        Ok(())
    }

    /// Parse a PubSub event from the message data.
    fn parse_event(&self, topic: &str, message: &str) -> Result<PubSubEvent> {
        let value: Value = serde_json::from_str(message)?;

        // Parse based on topic prefix
        if topic.starts_with("user-drop-events") {
            return self.parse_drop_event(&value);
        }

        if topic.starts_with("video-playback-by-id") {
            return self.parse_stream_event(topic, &value);
        }

        Ok(PubSubEvent::Unknown(value))
    }

    /// Parse a drop-related event.
    fn parse_drop_event(&self, value: &Value) -> Result<PubSubEvent> {
        let event_type = value["type"].as_str().unwrap_or("");

        match event_type {
            "drop-progress" => {
                let drop_id = value["data"]["drop_id"].as_str().unwrap_or("").to_string();
                let current_minutes =
                    value["data"]["current_progress_min"].as_i64().unwrap_or(0) as i32;
                Ok(PubSubEvent::DropProgress {
                    drop_id,
                    current_minutes,
                })
            }
            "drop-claim" => {
                let drop_instance_id = value["data"]["drop_instance_id"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                Ok(PubSubEvent::DropReady { drop_instance_id })
            }
            _ => Ok(PubSubEvent::Unknown(value.clone())),
        }
    }

    /// Parse a stream-related event.
    fn parse_stream_event(&self, topic: &str, value: &Value) -> Result<PubSubEvent> {
        // Extract channel_id from topic (video-playback-by-id.{channel_id})
        let channel_id: u64 = topic
            .split('.')
            .next_back()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        let event_type = value["type"].as_str().unwrap_or("");

        match event_type {
            "stream-up" => Ok(PubSubEvent::StreamOnline { channel_id }),
            "stream-down" => Ok(PubSubEvent::StreamOffline { channel_id }),
            _ => Ok(PubSubEvent::Unknown(value.clone())),
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
    fn test_outgoing_ping_serialization() {
        let msg = OutgoingMessage::Ping;
        let json = serde_json::to_string(&msg).unwrap();
        assert_eq!(json, r#"{"type":"PING"}"#);
    }

    #[test]
    fn test_outgoing_listen_serialization() {
        let msg = OutgoingMessage::Listen {
            data: ListenData {
                topics: vec!["user-drop-events.12345".to_string()],
                auth_token: "token123".to_string(),
            },
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["type"], "LISTEN");
        assert_eq!(parsed["data"]["topics"][0], "user-drop-events.12345");
        assert_eq!(parsed["data"]["auth_token"], "token123");
    }

    #[test]
    fn test_incoming_pong_parsing() {
        let json = r#"{"type":"PONG"}"#;
        let msg: IncomingMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.msg_type, "PONG");
    }

    #[test]
    fn test_incoming_message_parsing() {
        let json = r#"{
            "type": "MESSAGE",
            "data": {
                "topic": "user-drop-events.12345",
                "message": "{\"type\":\"drop-progress\",\"data\":{\"drop_id\":\"abc\",\"current_progress_min\":15}}"
            }
        }"#;

        let msg: IncomingMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.msg_type, "MESSAGE");
        assert!(msg.data.is_some());

        let data = msg.data.unwrap();
        assert_eq!(data.topic, "user-drop-events.12345");
    }

    #[test]
    fn test_parse_drop_progress_event() {
        let (manager, _rx) = WebSocketManager::new("token".to_string());
        let value: Value = serde_json::from_str(
            r#"{"type":"drop-progress","data":{"drop_id":"drop123","current_progress_min":30}}"#,
        )
        .unwrap();

        let event = manager.parse_drop_event(&value).unwrap();
        match event {
            PubSubEvent::DropProgress {
                drop_id,
                current_minutes,
            } => {
                assert_eq!(drop_id, "drop123");
                assert_eq!(current_minutes, 30);
            }
            _ => panic!("Expected DropProgress event"),
        }
    }

    #[test]
    fn test_parse_stream_event() {
        let (manager, _rx) = WebSocketManager::new("token".to_string());
        let value: Value = serde_json::from_str(r#"{"type":"stream-up"}"#).unwrap();

        let event = manager
            .parse_stream_event("video-playback-by-id.98765", &value)
            .unwrap();
        match event {
            PubSubEvent::StreamOnline { channel_id } => {
                assert_eq!(channel_id, 98765);
            }
            _ => panic!("Expected StreamOnline event"),
        }
    }
}
