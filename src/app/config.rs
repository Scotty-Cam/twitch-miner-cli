use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Configuration for the application.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_auth_path")]
    pub auth_path: PathBuf,
    #[serde(default)]
    pub priority_games: Vec<String>,
    #[serde(default)]
    pub excluded_games: Vec<String>,
    #[serde(default = "default_true")]
    pub notifications_enabled: bool,
    #[serde(default = "default_true")]
    pub logo_animation_enabled: bool,
    /// Proxy URL in format: http://username:password@address:port
    #[serde(default)]
    pub proxy_url: Option<String>,
}

fn default_auth_path() -> PathBuf {
    PathBuf::from("auth.json")
}

fn default_true() -> bool {
    true
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            auth_path: default_auth_path(),
            priority_games: Vec::new(),
            excluded_games: Vec::new(),
            notifications_enabled: true,
            logo_animation_enabled: true,
            proxy_url: None,
        }
    }
}

/// Validate proxy URL format (http://[user:pass@]host:port)
pub fn is_valid_proxy_url(url: &str) -> bool {
    if url.is_empty() {
        return false;
    }
    // Must start with http:// or https:// or socks5://
    if !url.starts_with("http://") && !url.starts_with("https://") && !url.starts_with("socks5://")
    {
        return false;
    }
    // Try to parse as URL
    url::Url::parse(url).is_ok()
}

impl AppConfig {
    pub fn load() -> Self {
        if let Ok(content) = std::fs::read_to_string("settings.json") {
            if let Ok(config) = serde_json::from_str(&content) {
                return config;
            }
        }
        Self::default()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write("settings.json", content)?;
        Ok(())
    }
}
