//! Twitch Miner CLI
//!
//! A terminal-based application for mining Twitch drops.

pub mod app;
pub mod auth;
pub mod constants;
pub mod gql;
pub mod models;
pub mod notifications;
pub mod ui;
pub mod utils;
pub mod watcher;
pub mod websocket;

use std::io;
use std::time::Duration;

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use tokio::sync::mpsc;

use app::{App, AppConfig, AppState, NavigationOps, Page, StateOps, WatcherOps};
use auth::{AuthState, DeviceAuthenticator};
use ui::render_dashboard;

/// Message types for async login
pub enum LoginMessage {
    CodeReady { code: String, uri: String },
    Success(AuthState),
    Failed(String),
}

/// Run the TUI application.
async fn run_app() -> anyhow::Result<()> {
    // Initialize tracing to a file to avoid corrupting TUI output
    let log_file = std::fs::File::create("twitch_miner.log")?;
    tracing_subscriber::fmt()
        .with_writer(log_file)
        .with_ansi(false)
        .init();

    // Load configuration
    let config = AppConfig::load();

    // Try to load existing auth (optional)
    let app = match AuthState::load(&config.auth_path).await {
        Ok(auth) => App::new(auth, config),
        Err(_) => App::new_logged_out(config),
    };

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Clear terminal and force initial size calculation
    terminal.clear()?;

    // Create app state
    let mut app = app;
    let mut logs: Vec<String> = vec!["Twitch Miner CLI started.".to_string()];

    if app.is_logged_in() {
        logs.push(format!(
            "Logged in as: {}",
            app.username().unwrap_or("Unknown")
        ));
        logs.push("Refreshing all campaign data...".to_string());
        app.change_state(AppState::AllCampaignsFetch);
    } else {
        logs.push("Not logged in. Go to Settings to login.".to_string());
    }

    // Channel for login messages
    let (login_tx, mut login_rx) = mpsc::channel::<LoginMessage>(10);

    // Main loop
    let mut interval = tokio::time::interval(Duration::from_millis(100));
    let mut last_autostart_check = std::time::Instant::now() - Duration::from_secs(10); // Trigger immediately on startup
    let mut last_second_bump = std::time::Instant::now();
    let mut last_api_refresh = std::time::Instant::now();
    let mut last_priority_check = std::time::Instant::now();
    let mut has_done_first_refresh = false; // Track if we've done initial refresh

    let mut last_claim_check = std::time::Instant::now();

    // Event stream for async input handling
    use crossterm::event::EventStream;
    use futures_util::StreamExt;
    let mut reader = EventStream::new();

    loop {
        tokio::select! {
             _ = tokio::signal::ctrl_c() => {
                logs.push("Received shutdown signal...".to_string());
                app.change_state(AppState::Exit);
                break;
            }
            _ = interval.tick() => {
                // App background tasks
                let tick_logs = app.tick();
                logs.extend(tick_logs);

                // Autostart watcher check (when idle, try to start watching)
                if app.state == AppState::Idle
                    && app.is_logged_in()
                    && last_autostart_check.elapsed() > Duration::from_secs(2)
                {
                    last_autostart_check = std::time::Instant::now();
                    match app.try_autostart().await {
                        Ok(msg) => logs.push(format!("▶ {}", msg)),
                        Err(_) => {
                            // Only update log if it's different from the last message
                        }
                    }
                }

                // Local second tracking for smooth UI countdown
                // Timer ticks for ALL subscribed campaigns regardless of watching state
                if last_second_bump.elapsed() >= Duration::from_secs(1) {
                    last_second_bump = std::time::Instant::now();
                    app.bump_active_drop_second();
                }

                // Priority check: switch to higher priority game if available (every 60s)
                if app.state == AppState::Watching
                    && app.is_logged_in()
                    && last_priority_check.elapsed() >= Duration::from_secs(60)
                {
                    last_priority_check = std::time::Instant::now();
                    if let Ok(switched) = app.check_priority_switch().await {
                        if switched {
                            logs.push("Switched to higher priority game.".to_string());
                        }
                    }
                }

                // Periodic drop claim cleanup (every 60s)
                // This runs regardless of state to catch drops that watcher might have missed
                if app.is_logged_in() && last_claim_check.elapsed() >= Duration::from_secs(60) {
                    last_claim_check = std::time::Instant::now();
                    if let Ok(claimed_list) = app.claim_unclaimed_drops().await {
                        for (game_name, drop_name) in claimed_list {
                            logs.push(format!("Drop obtained: {}, {}", drop_name, game_name));
                        }
                    }
                }

                // Auto-refresh from Twitch API (every 60s)
                if app.is_logged_in() && last_api_refresh.elapsed() >= Duration::from_secs(60) {
                    last_api_refresh = std::time::Instant::now();

                    // If Idle, do a full state refresh (shows loading screen)
                    if matches!(app.state, AppState::Idle) {
                        app.change_state(AppState::AllCampaignsFetch);
                    } else {
                        // If Watching or other states, do a background silent refresh
                        // This keeps campaign data synced without interrupting the UI/Watcher
                        let _ = app.refresh_data_background().await;
                    }
                }

                // Draw
                // Note: terminal.draw triggers a redraw. It's fast but we still want to avoid extensive logic in draw closure.
                if let Err(e) = terminal.draw(|frame| {
                    render_dashboard(frame, &app, &logs);
                }) {
                     tracing::error!("Failed to draw: {}", e);
                }

                // Check for login messages (non-blocking)
                while let Ok(msg) = login_rx.try_recv() {
                    match msg {
                        LoginMessage::CodeReady { code, uri } => {
                            app.set_login_pending(code, uri);
                            logs.push("Login code ready - check Settings page".to_string());
                        }
                        LoginMessage::Success(auth) => {
                            auth.save("auth.json").await.ok();
                            let username = auth.login.clone();
                            app.set_auth(auth);
                            app.clear_login_state();
                            logs.push(format!("Login successful! Welcome, {}", username));
                            logs.push("Refreshing all campaign data...".to_string());
                            app.change_state(AppState::AllCampaignsFetch);
                        }
                        LoginMessage::Failed(err) => {
                            app.clear_login_state();
                            app.change_state(AppState::Idle);
                            logs.push(format!("Login failed: {}", err));
                        }
                    }
                }

                // Process state transitions if any (mostly triggered by user input or timers above)
                match app.state {
                    AppState::InventoryFetch => {
                        match app.fetch_inventory().await {
                            Ok(_count) => {
                                // Also fetch detailed progress for subscribed campaigns
                                match app.fetch_subscribed_campaign_details().await {
                                    Ok(updated) => {
                                        if !has_done_first_refresh && updated > 0 {
                                            logs.push(format!(
                                                "Updated progress for {} subscribed campaigns.",
                                                updated
                                            ));
                                        }
                                    }
                                    Err(e) => {
                                        tracing::warn!("Failed to fetch subscribed details: {}", e);
                                    }
                                }
                                // Mark first refresh as done
                                has_done_first_refresh = true;
                                // Restore Watching state if watcher is still active
                                if app.is_watcher_active() {
                                    app.change_state(AppState::Watching);
                                } else {
                                    app.change_state(AppState::Idle);
                                }
                            }
                            Err(e) => {
                                logs.push(format!("Error: {}", e));
                                // Restore Watching state if watcher is still active
                                if app.is_watcher_active() {
                                    app.change_state(AppState::Watching);
                                } else {
                                    app.change_state(AppState::Idle);
                                }
                            }
                        }
                    }
                    AppState::AllCampaignsFetch => {
                        match app.fetch_all_campaigns().await {
                            Ok(count) => {
                                if !has_done_first_refresh {
                                    logs.push(format!("Loaded {} campaigns. Updating inventory...", count));
                                }
                                // After all campaigns, fetch current user inventory too
                                app.change_state(AppState::InventoryFetch);
                            }
                            Err(e) => {
                                logs.push(format!("Error loading campaigns: {}", e));
                                app.change_state(AppState::InventoryFetch); // Try inventory anyway
                            }
                        }
                    }
                    AppState::Exit => {
                         break;
                    }
                    _ => {}
                }

                // Truncate logs
                if logs.len() > 100 {
                    logs.drain(0..50);
                }
            }
            Some(Ok(event)) = reader.next() => {
                 if let Event::Key(key) = event {
                    if key.kind == KeyEventKind::Press {
                        // EXCLUSIVE INPUT MODE: Proxy Editing
                        if app.is_proxy_editing() {
                            match key.code {
                                KeyCode::Enter => {
                                    let msg = app.save_proxy();
                                    logs.push(msg);
                                }
                                KeyCode::Esc => {
                                    app.cancel_proxy_edit();
                                    logs.push("Proxy edit cancelled.".to_string());
                                }
                                KeyCode::Backspace => {
                                    app.proxy_input.pop();
                                }
                                KeyCode::Char(c) => {
                                    app.proxy_input.push(c);
                                }
                                _ => {}
                            }
                        } else {
                            // Standard Application Input
                            match key.code {
                                // Quit
                                KeyCode::Char('q') | KeyCode::Esc => {
                                    app.change_state(AppState::Exit);
                                    break;
                                }

                                // Navigation keys
                                KeyCode::Char('h') | KeyCode::Char('H') => {
                                    app.navigate_to(Page::Home);
                                }
                                KeyCode::Char('d') | KeyCode::Char('D') => {
                                    app.navigate_to(Page::Drops);
                                    if app.is_logged_in() && app.all_campaigns.is_empty() {
                                        logs.push("Fetching all campaigns...".to_string());
                                        app.change_state(AppState::AllCampaignsFetch);
                                    }
                                }
                                KeyCode::Char('s') | KeyCode::Char('S') => {
                                    app.navigate_to(Page::Settings);
                                }
                                KeyCode::Char('a') | KeyCode::Char('A') => {
                                    app.navigate_to(Page::About);
                                }

                                // Settings Page Specific Inputs
                                _ if app.page == Page::Settings => {
                                    match key.code {
                                        KeyCode::Up => {
                                            app.move_settings_selection_up();
                                        }
                                        KeyCode::Down => {
                                            app.move_settings_selection_down();
                                        }
                                        KeyCode::Left | KeyCode::Right => {
                                            app.cycle_settings_focus();
                                        }
                                        KeyCode::Enter => {
                                            // Activate selected setting
                                            match app.settings_selected {
                                                crate::app::SettingsItem::AccountSettings => {
                                                    // No action on Enter for Account Settings
                                                }
                                                crate::app::SettingsItem::Notifications => {
                                                    let msg = app.toggle_notifications();
                                                    logs.push(msg);
                                                }
                                                crate::app::SettingsItem::LogoAnimation => {
                                                    let msg = app.toggle_logo_animation();
                                                    logs.push(msg);
                                                }
                                                crate::app::SettingsItem::ProxySettings => {
                                                    app.start_proxy_edit();
                                                }
                                            }
                                        }
                                        KeyCode::Char('l') | KeyCode::Char('L') => {
                                            // Login/Logout toggle
                                            if !app.is_login_pending() {
                                                if app.is_logged_in() {
                                                    // Logout
                                                    app.logout();
                                                    let _ = std::fs::remove_file("auth.json");
                                                    logs.push("Logged out successfully.".to_string());
                                                } else {
                                                    // Login
                                                    logs.push("Starting login...".to_string());
                                                    app.change_state(AppState::LoginPending);
                                                    let tx = login_tx.clone();
                                                    let proxy_url = app.config.proxy_url.clone();
                                                    tokio::spawn(async move {
                                                        let mut authenticator =
                                                            DeviceAuthenticator::new_with_proxy(
                                                                proxy_url,
                                                            );

                                                        if let Err(e) = authenticator.init().await {
                                                            let _ = tx
                                                                .send(LoginMessage::Failed(
                                                                    e.to_string(),
                                                                ))
                                                                .await;
                                                            return;
                                                        }

                                                        match authenticator
                                                            .authenticate_async(tx.clone())
                                                            .await
                                                        {
                                                            Ok(auth) => {
                                                                let _ = tx
                                                                    .send(LoginMessage::Success(auth))
                                                                    .await;
                                                            }
                                                            Err(e) => {
                                                                let _ = tx
                                                                    .send(LoginMessage::Failed(
                                                                        e.to_string(),
                                                                    ))
                                                                    .await;
                                                            }
                                                        }
                                                    });
                                                }
                                            }
                                        }
                                        _ => {}
                                    }
                                }

                                // Home Page Specific Inputs
                                _ if app.page == Page::Home => match key.code {
                                    KeyCode::Left | KeyCode::Right => {
                                        app.cycle_home_focus();
                                    }
                                    KeyCode::Up => {
                                        app.move_home_selection_up();
                                    }
                                    KeyCode::Down => {
                                        app.move_home_selection_down();
                                    }
                                    _ => {}
                                },

                                // Drops Page Specific Inputs
                                _ if app.page == Page::Drops => {
                                    match key.code {
                                        KeyCode::Up => {
                                            if key.modifiers.contains(event::KeyModifiers::SHIFT) {
                                                // Reorder subscribed game up
                                                if app.drops_focus
                                                    == crate::app::DropsFocus::SubscribedDrops
                                                    && app.move_subscribed_game_up()
                                                {
                                                    // Trigger priority check immediately if order changed
                                                    if let Ok(switched) =
                                                        app.check_priority_switch().await
                                                    {
                                                        if switched {
                                                            logs.push(
                                                                "Switched to higher priority game."
                                                                    .to_string(),
                                                            );
                                                        }
                                                    }
                                                }
                                            } else {
                                                app.move_drops_selection_up();
                                            }
                                        }
                                        KeyCode::Down => {
                                            if key.modifiers.contains(event::KeyModifiers::SHIFT) {
                                                // Reorder subscribed game down
                                                if app.drops_focus
                                                    == crate::app::DropsFocus::SubscribedDrops
                                                    && app.move_subscribed_game_down()
                                                {
                                                    // Trigger priority check immediately if order changed
                                                    if let Ok(switched) =
                                                        app.check_priority_switch().await
                                                    {
                                                        if switched {
                                                            logs.push(
                                                                "Switched to higher priority game."
                                                                    .to_string(),
                                                            );
                                                        }
                                                    }
                                                }
                                            } else {
                                                app.move_drops_selection_down();
                                            }
                                        }
                                        KeyCode::Left | KeyCode::Right => {
                                            app.cycle_drops_focus();
                                        }
                                        KeyCode::Enter => {
                                            match app.toggle_drops_subscription() {
                                                Ok((msg, should_refresh)) => {
                                                    logs.push(msg);
                                                    if should_refresh {
                                                        // Fetch details for the newly subscribed game immediately
                                                        // We prefer background refresh to avoid full UI reload/blocking
                                                        let _ = app.refresh_data_background().await;
                                                        logs.push(
                                                            "Fetching campaign details...".to_string(),
                                                        );
                                                    }
                                                }
                                                Err(e) => logs.push(format!("Error: {}", e)),
                                            }
                                        }
                                        _ => {}
                                    }
                                }

                                // About Page Specific Inputs (Scrolling)
                                _ if app.page == Page::About => {
                                    match key.code {
                                        KeyCode::Up => {
                                            app.scroll_about_up();
                                        }
                                        KeyCode::Down => {
                                            // Calculate visible height: terminal height - nav(3) - status(3) - borders(2)
                                            let visible_height = terminal
                                                .size()
                                                .map(|s| s.height.saturating_sub(8))
                                                .unwrap_or(20);
                                            app.scroll_about_down(visible_height);
                                        }
                                        _ => {}
                                    }
                                }

                                _ => {}
                            }
                        }
                    }
                }
                if let Err(e) = terminal.draw(|frame| {
                    render_dashboard(frame, &app, &logs);
                }) {
                     tracing::error!("Failed to draw: {}", e);
                }
            }
        }
    }

    // Cleanup
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if let Err(e) = run_app().await {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
    Ok(())
}
