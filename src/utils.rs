//! Shared utility functions.

/// Mask credentials in proxy URL for display (e.g., http://***:***@host:port)
pub fn mask_proxy_url(url: &str) -> String {
    if let Ok(parsed) = url::Url::parse(url) {
        if parsed.username().is_empty() {
            url.to_string()
        } else {
            let scheme = parsed.scheme();
            let host = parsed.host_str().unwrap_or("unknown");
            let port = parsed.port().map(|p| format!(":{}", p)).unwrap_or_default();
            format!("{}://***:***@{}{}", scheme, host, port)
        }
    } else {
        url.to_string()
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_proxy_url_with_credentials() {
        let url = "http://user:pass@proxy.example.com:8080";
        assert_eq!(mask_proxy_url(url), "http://***:***@proxy.example.com:8080");
    }

    #[test]
    fn test_mask_proxy_url_without_credentials() {
        let url = "http://proxy.example.com:8080";
        assert_eq!(mask_proxy_url(url), url);
    }

    #[test]
    fn test_mask_proxy_url_socks() {
        let url = "socks5://user:pass@proxy.example.com:1080";
        assert_eq!(
            mask_proxy_url(url),
            "socks5://***:***@proxy.example.com:1080"
        );
    }

    #[test]
    fn test_mask_proxy_url_invalid() {
        let url = "not-a-valid-url";
        assert_eq!(mask_proxy_url(url), url);
    }
}
