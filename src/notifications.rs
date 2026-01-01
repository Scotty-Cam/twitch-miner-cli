//! Desktop notifications for drop completion alerts.
//!
//! Provides cross-platform notification support using the `notify-rust` crate.
//! Works on Windows, macOS, and Linux.

use notify_rust::{Notification, Timeout};

/// Send a desktop notification when a drop is obtained.
///
/// # Arguments
/// * `game` - The name of the game
/// * `drop` - The name of the drop that was obtained
///
/// # Returns
/// `Ok(())` if the notification was sent successfully, or an error if it failed.
pub fn send_drop_notification(game: &str, drop: &str) -> Result<(), notify_rust::error::Error> {
    Notification::new()
        .summary("Twitch drop obtained!")
        .body(&format!("{}\nGame: {}", drop, game))
        .sound_name("message-new-instant") // Standard freedesktop sound
        .timeout(Timeout::Milliseconds(10000)) // 10 seconds
        .show()?;

    tracing::info!("Notification sent for drop: {} ({})", drop, game);
    Ok(())
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Test that a notification can be sent.
    /// This test actually shows a notification on your desktop!
    /// Run with: cargo test test_notification_shows -- --ignored --nocapture
    #[test]
    #[ignore] // Ignored by default - run manually to see the notification
    fn test_notification_shows() {
        let result = send_drop_notification("Test Game", "Test Drop Reward");

        match result {
            Ok(()) => println!("✓ Notification sent successfully!"),
            Err(e) => panic!("✗ Notification failed: {}", e),
        }
    }

    /// Interactive test - sends a notification that looks like a real drop claim.
    /// Run with: cargo test test_realistic_drop_notification -- --ignored --nocapture
    #[test]
    #[ignore]
    fn test_realistic_drop_notification() {
        let result = send_drop_notification("World of Tanks", "Premium Tank Bundle");

        assert!(result.is_ok(), "Notification should send without error");
        println!("✓ Notification sent! Check your desktop for the toast.");

        // Give user time to see the notification
        std::thread::sleep(std::time::Duration::from_secs(3));
    }
}
