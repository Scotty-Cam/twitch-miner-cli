//! Dashboard UI layout.
//!
//! Main TUI layout with navigation, campaigns list, and log window.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};
use std::time::{SystemTime, UNIX_EPOCH};

// Branding constants
const AUTHOR_NAME: &str = "Scotty-Cam";

const PICKAXE: &str = "\u{26CF}\u{FE0E}"; // ⛏︎

use super::{render_about, render_settings};
use crate::app::{App, AppState, DropsFocus, HomeFocus, Page};
use crate::app::{CampaignOps, NavigationOps, StateOps};

/// Render the main application UI.
pub fn render_dashboard(frame: &mut Frame, app: &App, logs: &[String]) {
    // About page gets special layout without logs
    if app.page == Page::About {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Navigation bar (Fixed height)
                Constraint::Fill(1),   // Main content (Flexible - takes log space too)
                Constraint::Length(3), // Status bar (Fixed height)
            ])
            .split(frame.area());

        render_nav_bar(frame, app, chunks[0]);
        render_status_bar(frame, app, chunks[2]);
        render_about(frame, chunks[1], app.about_scroll);
        return;
    }

    // Standard layout with logs
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Navigation bar (Fixed height)
            Constraint::Fill(1),   // Main content (Flexible)
            Constraint::Length(6), // Log window (Fixed 6 lines)
            Constraint::Length(3), // Status bar (Fixed height)
        ])
        .split(frame.area());

    render_nav_bar(frame, app, chunks[0]);

    // Status Bar
    render_status_bar(frame, app, chunks[3]);

    // Route to appropriate page
    match app.page {
        Page::Home => render_home(frame, app, chunks[1]),
        Page::Drops => render_drops_page(frame, app, chunks[1]),
        Page::Settings => render_settings(frame, app, chunks[1]),
        Page::About => unreachable!(), // Handled above
    }

    render_logs(frame, logs, chunks[2]);
}

/// Render the navigation bar.
fn render_nav_bar(frame: &mut Frame, app: &App, area: Rect) {
    let nav_items = [
        ("H", "ome", Page::Home),
        ("D", "rops", Page::Drops),
        ("S", "ettings", Page::Settings),
        ("A", "bout", Page::About),
    ];

    // 1. Render Outer Block for continuous border
    let block = Block::default()
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::DarkGray));
    let inner_area = block.inner(area);
    frame.render_widget(block, area);

    // 2. Calculate animated branding color
    let title_color = if app.config.logo_animation_enabled {
        // Purple Haze Pulse: Transition between deep purple and electric lavender
        let time_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        let intensity = (time_secs * 3.0).sin() * 0.5 + 0.5;
        let r = (60.0 + 100.0 * intensity) as u8;
        let g = (20.0 * intensity) as u8;
        let b = (120.0 + 135.0 * intensity) as u8;
        Color::Rgb(r, g, b)
    } else {
        // Disabled: Grey
        Color::DarkGray
    };

    let pick_color = if app.config.logo_animation_enabled {
        Color::Rgb(255, 215, 0) // Static Gold
    } else {
        Color::DarkGray
    };

    // 3. Build branding spans: ⛏︎ Twitch Miner CLI by ScottyCam
    let branding_spans = vec![
        Span::styled(
            PICKAXE,
            Style::default().fg(pick_color).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            "Twitch Miner CLI",
            Style::default()
                .fg(title_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" by ", Style::default().fg(Color::White)),
        Span::styled(
            AUTHOR_NAME,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
    ];

    // 4. Build menu spans
    let mut nav_spans: Vec<Span> = nav_items
        .iter()
        .flat_map(|(key, label, page)| {
            let style = if *page == app.page {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            vec![
                Span::styled("[", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    *key,
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("]", Style::default().fg(Color::DarkGray)),
                Span::styled(*label, style),
                Span::raw("  "),
            ]
        })
        .collect();

    // Add Quit option manually
    nav_spans.extend(vec![
        Span::styled("[", Style::default().fg(Color::DarkGray)),
        Span::styled(
            "Q",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("]", Style::default().fg(Color::DarkGray)),
        Span::styled("uit", Style::default().fg(Color::White)),
        Span::raw("  "),
    ]);

    // 5. Simple two-column layout: branding left, menu right
    // Menu width: Previous ~44 + [Q]uit(8) = ~52
    let menu_width = 52u16;

    let layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Fill(1),            // Left: Branding (takes remaining space)
            Constraint::Length(menu_width), // Right: Menu (fixed width)
        ])
        .split(inner_area);

    // 6. Render Branding (left aligned)
    let branding_line = Line::from(branding_spans);
    let branding = Paragraph::new(branding_line);
    frame.render_widget(branding, layout[0]);

    // 7. Render Menu (right aligned within its area)
    let nav_line = Line::from(nav_spans).alignment(ratatui::layout::Alignment::Right);
    let menu = Paragraph::new(nav_line);
    frame.render_widget(menu, layout[1]);
}

/// Render the Home page content.
fn render_home(frame: &mut Frame, app: &App, area: Rect) {
    if !app.is_logged_in() {
        let content = vec![
            Line::from(""),
            Line::from(Span::styled(
                "Welcome to Twitch Miner CLI!",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Please login to start farming drops.",
                Style::default().fg(Color::Gray),
            )),
            Line::from(""),
            Line::from(vec![
                Span::raw("Go to "),
                Span::styled(
                    "[S]",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("ettings to login with Twitch."),
            ]),
        ];

        let paragraph =
            Paragraph::new(content).block(Block::default().borders(Borders::ALL).title("Home"));
        frame.render_widget(paragraph, area);
        return;
    }

    // Logged in - show Watching + Inactive Campaigns (Split view)
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50), // Watching
            Constraint::Percentage(50), // Inactive
        ])
        .split(area);

    render_watching_panel(frame, app, chunks[0]);
    render_inactive_panel(frame, app, chunks[1]);
}

/// Render Watching panel (Left side)
fn render_watching_panel(frame: &mut Frame, app: &App, area: Rect) {
    let focus_style = if app.home_focus == HomeFocus::Watching {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Watching ")
        .style(focus_style);

    let inner_area = block.inner(area);
    frame.render_widget(block, area);

    // Find active game drop
    let campaigns = app.subscribed_campaigns_with_progress();

    // Check if this campaign is currently being watched (or attempted)
    let active_game = app
        .mining_status
        .as_ref()
        .map(|s| s.game_name.as_str())
        .or(app.current_attempt_game.as_deref());

    let watched_campaign = campaigns
        .iter()
        .find(|c| Some(c.game.display_name.as_str()) == active_game)
        .copied();

    if let Some(campaign) = watched_campaign {
        // Render detailed progress view
        // Adjust area to fit content (create a list item effectively)
        let content_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(6), Constraint::Min(0)])
            .split(inner_area);

        render_campaign_progress(frame, app, campaign, content_chunks[0]);

        // If there are multiple watching? (shouldn't happen with current logic, but maybe queue?)
        // For now just show the one active one.
    } else if let Some(msg) = &app.login_status {
        // Show login status if relevant
        let p = Paragraph::new(msg.as_str()).style(Style::default().fg(Color::Yellow));
        frame.render_widget(p, inner_area);
    } else if app.state == AppState::Watching && active_game.is_none() {
        let p = Paragraph::new("Searching for streams...").style(Style::default().fg(Color::Gray));
        frame.render_widget(p, inner_area);
    } else {
        let p =
            Paragraph::new("Not watching any streams.").style(Style::default().fg(Color::DarkGray));
        frame.render_widget(p, inner_area);
    }
}

/// Render Inactive panel (Right side - formerly Subscribed Campaigns)
fn render_inactive_panel(frame: &mut Frame, app: &App, area: Rect) {
    let subscribed = app.subscribed_campaigns_with_progress();

    // Group campaigns by game name
    let mut games: std::collections::HashMap<&str, Vec<&crate::models::DropsCampaign>> =
        std::collections::HashMap::new();
    for campaign in &subscribed {
        if campaign.is_active() {
            games
                .entry(&campaign.game.display_name)
                .or_default()
                .push(*campaign);
        }
    }

    let active_game = app
        .mining_status
        .as_ref()
        .map(|s| s.game_name.as_str())
        .or(app.current_attempt_game.as_deref());

    // Filter out active game
    let mut game_names: Vec<&str> = games
        .keys()
        .copied()
        .filter(|g| Some(*g) != active_game)
        .collect();

    // Sort Logic:
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

        // Incomplete (true) < Complete (false) for ascending sort? No, we want Incomplete first.
        // Ascending: false (0), true (1). Descending: true, false.
        b_has_drops
            .cmp(&a_has_drops) // true first
            .then_with(|| a.to_lowercase().cmp(&b.to_lowercase()))
    });

    let focus_style = if app.home_focus == HomeFocus::Inactive {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Inactive ")
        .style(focus_style);

    let inner_area = block.inner(area);
    frame.render_widget(block, area);

    if game_names.is_empty() {
        let content = Paragraph::new("Add games from the [Drops] screen")
            .style(Style::default().fg(Color::Gray));
        frame.render_widget(content, inner_area);
        return;
    }

    // Build list items
    let mut items = Vec::new();
    let mut current_idx = 0;

    for game_name in &game_names {
        if let Some(campaigns) = games.get(game_name) {
            // Game header
            let is_selected =
                app.home_focus == HomeFocus::Inactive && app.home_inactive_selected == current_idx;

            let arrow_style = if is_selected {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Cyan)
            };

            let name_style = Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD);

            let (_, is_linked) = app.get_game_display_info(game_name);
            let linked_icon = " ∞"; // Leading space
            let linked_color = if is_linked {
                Color::Green
            } else {
                Color::DarkGray
            };

            // Build game header spans
            let header_spans = vec![
                Span::styled("▸ ", arrow_style),
                Span::styled(*game_name, name_style),
                Span::styled(linked_icon, Style::default().fg(linked_color)),
            ];

            items.push(ListItem::new(Line::from(header_spans)));
            current_idx += 1;

            // Add unlinked message on a separate line if account not connected
            if !is_linked {
                // Display raw URL so terminal can auto-detect it.
                // We avoid OSC 8 here because invisible escape codes confuse ratatui's width calculations.
                items.push(ListItem::new(Line::from(vec![
                    Span::styled("    ", Style::default()), // Indent
                    Span::styled("Link your account: ", Style::default().fg(Color::Yellow)),
                    Span::styled(
                        "https://www.twitch.tv/drops/inventory",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::UNDERLINED),
                    ),
                ])));
                current_idx += 1;
            }

            for campaign in campaigns {
                let camp_status = get_campaign_status_line(campaign);
                items.push(ListItem::new(Line::from(vec![
                    Span::styled("    ", Style::default()),
                    Span::styled(&campaign.name, Style::default().fg(Color::Gray)),
                    Span::styled(" - ", Style::default().fg(Color::DarkGray)),
                    camp_status,
                ])));
                current_idx += 1;
            }

            // Gaps
            items.push(ListItem::new(Line::from("")));
            current_idx += 1;
        }
    }

    // Create List widget
    // User requested NO grey background and NO default arrow for selection.
    // We handle selection visualization manually via `is_selected` logic above (Yellow text).
    // So we disable the default highlight style and symbol.
    let list = List::new(items)
        .highlight_style(Style::default()) // No change on selection (transparent)
        .highlight_symbol(""); // No symbol

    let mut state = ratatui::widgets::ListState::default();
    if app.home_focus == HomeFocus::Inactive {
        state.select(Some(app.home_inactive_selected));
    }

    frame.render_stateful_widget(list, inner_area, &mut state);
}

/// Get a compact status line for a campaign.
fn get_campaign_status_line(campaign: &crate::models::DropsCampaign) -> Span<'static> {
    // Check for unlinked account - progress data is not available from Twitch
    let is_account_linked = campaign
        .self_info
        .as_ref()
        .map(|s| s.is_account_connected)
        .unwrap_or(true); // Assume linked if no info (conservative)

    if !is_account_linked && campaign.is_active() {
        // Can't know progress for unlinked - Twitch doesn't report it
        return Span::styled("-".to_string(), Style::default().fg(Color::DarkGray));
    }

    if let Some(drop) = campaign.first_unclaimed_drop() {
        // Has unclaimed drops - show progress
        let pct = (drop.progress() * 100.0) as u8;
        let claimed = campaign.claimed_drops_count();
        let total = campaign.total_drops_count();

        if pct == 0 && drop.current_minutes() < 1.0 {
            // 0% and no progress yet
            Span::styled(
                format!("0% ({}/{}) - Ready", claimed, total),
                Style::default().fg(Color::Yellow),
            )
        } else {
            Span::styled(
                format!("{}% ({}/{})", pct, claimed, total),
                Style::default().fg(Color::Green),
            )
        }
    } else if campaign.time_based_drops.is_empty() {
        // No drops data loaded - could be:
        // 1. Never watched (Queued)
        // 2. All drops claimed (Complete) - not in "in progress" inventory
        // Since we can't distinguish, show a neutral status for active campaigns
        if campaign.is_active() {
            // Could be complete or just not watched yet - show placeholder
            Span::styled("0%", Style::default().fg(Color::DarkGray))
        } else {
            Span::styled("Expired", Style::default().fg(Color::DarkGray))
        }
    } else {
        // All drops claimed
        let total = campaign.total_drops_count();
        Span::styled(
            format!("Complete ({}/{})", total, total),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )
    }
}

/// Render a single campaign's progress like TwitchDropsMiner UI.
fn render_campaign_progress(
    frame: &mut Frame,
    app: &App,
    campaign: &crate::models::DropsCampaign,
    area: Rect,
) {
    // Layout: 6 lines per campaign
    // 1: Game | Campaign name
    // 2: Campaign progress info (X/N drops, percentage, time remaining)
    // 3: Campaign gauge
    // 4: Drop name
    // 5: Drop progress info (percentage, time remaining)
    // 6: Drop gauge
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Game + Campaign name
            Constraint::Length(1), // Campaign progress text
            Constraint::Length(1), // Campaign gauge
            Constraint::Length(1), // Drop name
            Constraint::Length(1), // Drop progress text
            Constraint::Length(1), // Drop gauge
        ])
        .split(area);

    // Check if this campaign is currently being watched (or attempted)
    let is_watching = app
        .mining_status
        .as_ref()
        .map(|s| s.game_name == campaign.game.display_name)
        .or(app
            .current_attempt_game
            .as_ref()
            .map(|g| *g == campaign.game.display_name))
        .unwrap_or(false);

    // Find the first unclaimed drop for progress display, OR use mining status if watching
    // Priority:
    // 1. If mining this game, try to match the mining status drop
    // 2. Fallback to first unclaimed
    let mut display_drop = campaign.first_unclaimed_drop();

    // If we are mining this game, override display with the drop we are actually mining
    // This fixes the "stale progress bar" issue where UI shows old state until API refresh
    if is_watching {
        if let Some(status) = &app.mining_status {
            if let Some(drop) = campaign
                .time_based_drops
                .iter()
                .find(|d| d.name == status.drop_name)
            {
                display_drop = Some(drop);
            }
        }
    }

    let status_indicator = if is_watching {
        Span::raw("")
    } else if campaign.is_active() {
        Span::styled(" ○ Active", Style::default().fg(Color::Yellow))
    } else {
        Span::styled(" ○ Inactive", Style::default().fg(Color::DarkGray))
    };

    let (_, is_linked) = app.get_game_display_info(&campaign.game.display_name);
    let linked_icon = " ∞";
    let linked_color = if is_linked {
        Color::Green
    } else {
        Color::DarkGray
    };

    // Line 1: Game | Campaign name
    let header_line = Line::from(vec![
        Span::styled("▸ ", Style::default().fg(Color::Cyan)),
        Span::styled(
            &campaign.game.display_name,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(linked_icon, Style::default().fg(linked_color)),
        Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
        Span::styled(&campaign.name, Style::default().fg(Color::Gray)),
        status_indicator,
    ]);
    frame.render_widget(Paragraph::new(header_line), chunks[0]);

    // If we have a stored drop or we are actively watching (fallback to status)
    if let Some(drop) = display_drop {
        let claimed = campaign.claimed_drops_count();
        let total = campaign.total_drops_count();
        let campaign_pct = campaign.campaign_progress() * 100.0;
        let campaign_time = campaign.time_remaining();

        // Line 2: Campaign progress info
        let campaign_info = Line::from(vec![
            Span::styled("  Campaign: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!(
                    "{:.1}% ({}/{})",
                    (campaign_pct * 10.0).floor() / 10.0,
                    claimed,
                    total
                ),
                Style::default().fg(Color::Cyan),
            ),
            Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
            Span::styled(campaign_time, Style::default().fg(Color::Yellow)),
        ]);
        frame.render_widget(Paragraph::new(campaign_info), chunks[1]);

        // Line 3: Campaign gauge
        render_gradient_bar(frame, chunks[2], campaign_pct);

        // Line 4: Drop name (prefer benefit name if available)
        let display_name = if let Some(benefit) = drop.benefit_edges.first().map(|e| &e.benefit) {
            &benefit.name
        } else {
            &drop.name
        };

        let drop_name_line = Line::from(vec![
            Span::styled("  Drop: ", Style::default().fg(Color::DarkGray)),
            Span::styled(display_name, Style::default().fg(Color::White)),
        ]);
        frame.render_widget(Paragraph::new(drop_name_line), chunks[3]);

        // Line 5: Drop progress info
        let drop_pct = drop.progress() * 100.0;
        let drop_time = drop.time_remaining_display();
        let drop_info = Line::from(vec![
            Span::styled("  Progress: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{:.1}%", drop_pct),
                Style::default().fg(Color::Green),
            ),
            Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
            Span::styled(drop_time, Style::default().fg(Color::Yellow)),
        ]);
        frame.render_widget(Paragraph::new(drop_info), chunks[4]);

        // Line 6: Drop gauge
        render_gradient_bar(frame, chunks[5], drop_pct);
    } else if is_watching && app.mining_status.is_some() {
        // Fallback: We are watching but don't have drop details in the campaign object yet.
        // Use mining_status to render what we know.
        let status = app.mining_status.as_ref().unwrap();

        // Line 2: Campaign placeholders (unknown totals)
        let campaign_info = Line::from(vec![
            Span::styled("  Campaign: ", Style::default().fg(Color::DarkGray)),
            Span::styled("Active", Style::default().fg(Color::Cyan)),
        ]);
        frame.render_widget(Paragraph::new(campaign_info), chunks[1]);

        // Line 4: Drop name from Status
        let drop_name_line = Line::from(vec![
            Span::styled("  Drop: ", Style::default().fg(Color::DarkGray)),
            Span::styled(&status.drop_name, Style::default().fg(Color::White)),
        ]);
        frame.render_widget(Paragraph::new(drop_name_line), chunks[3]);

        // Line 5: Drop progress from Status
        let drop_pct = status.progress_percent;
        let remaining_mins = status
            .minutes_required
            .saturating_sub(status.minutes_watched);
        let drop_time = format!("{} min remaining", remaining_mins);

        let drop_info = Line::from(vec![
            Span::styled("  Progress: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{:.1}%", drop_pct),
                Style::default().fg(Color::Green),
            ),
            Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
            Span::styled(drop_time, Style::default().fg(Color::Yellow)),
        ]);
        frame.render_widget(Paragraph::new(drop_info), chunks[4]);

        // Line 6: Gauge
        render_gradient_bar(frame, chunks[5], drop_pct as f64);
    } else if campaign.time_based_drops.is_empty() {
        // No drop data available
        let waiting_line = Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                "Waiting for progress data...",
                Style::default().fg(Color::DarkGray),
            ),
        ]);
        frame.render_widget(Paragraph::new(waiting_line), chunks[1]);
    } else {
        // All drops claimed
        let complete_line = Line::from(vec![
            Span::styled("  All ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{} drops", campaign.total_drops_count()),
                Style::default().fg(Color::Cyan),
            ),
            Span::styled(
                " CLAIMED ✓",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
        ]);
        frame.render_widget(Paragraph::new(complete_line), chunks[1]);
    }
}

/// Helper function to render a fancy purple gradient progress bar.
fn render_gradient_bar(frame: &mut Frame, area: Rect, percent: f64) {
    let width = area.width as usize;
    if width == 0 {
        return;
    }

    // Ensure percent is 0..100 logic handled cleanly
    let pct_clamped = percent.clamp(0.0, 100.0);
    // Determine how many characters are "filled"
    let filled_len = ((width as f64 * pct_clamped) / 100.0).round() as usize;

    let mut spans = Vec::with_capacity(width);

    for i in 0..width {
        if i < filled_len {
            // Gradient Logic from user's python script:
            // Dark Purple (45, 0, 100) to Light Purple (210, 160, 255)
            // fraction = i / max(1, width - 1)
            let fraction = i as f64 / (width.max(2) - 1) as f64;

            let r = (45.0 + (210.0 - 45.0) * fraction) as u8;
            let g = (0.0 + (160.0 - 0.0) * fraction) as u8;
            let b = (100.0 + (255.0 - 100.0) * fraction) as u8;

            spans.push(Span::styled("█", Style::default().fg(Color::Rgb(r, g, b))));
        } else {
            // Empty part: Dark Grey (40, 40, 40)
            spans.push(Span::styled(
                "░",
                Style::default().fg(Color::Rgb(40, 40, 40)),
            ));
        }
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Render the drops page with split view.
fn render_drops_page(frame: &mut Frame, app: &App, area: Rect) {
    if !app.is_logged_in() {
        let content = Paragraph::new("Please login to view drops.")
            .block(Block::default().borders(Borders::ALL).title("Drops"));
        frame.render_widget(content, area);
        return;
    }

    // Split area into 2 panels
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50), // All Drops
            Constraint::Percentage(50), // Subscribed Drops
        ])
        .split(area);

    render_all_drops_panel(frame, app, chunks[0]);
    render_subscribed_drops_panel(frame, app, chunks[1]);
}

fn render_all_drops_panel(frame: &mut Frame, app: &App, area: Rect) {
    let games = app.get_drops_all_games();
    let is_focused = app.drops_focus == DropsFocus::AllDrops;

    let border_style = if is_focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let title_style = if is_focused {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(" All Drops ", title_style))
        .border_style(border_style);

    let inner_area = block.inner(area);
    frame.render_widget(block, area);

    if games.is_empty() {
        let p = Paragraph::new("No drops available.").style(Style::default().fg(Color::Gray));
        frame.render_widget(p, inner_area);
        return;
    }

    let items: Vec<ListItem> = games
        .iter()
        .enumerate()
        .map(|(i, game)| {
            let (campaigns_str, is_linked) = app.get_game_display_info(game);
            let linked_color = if is_linked {
                Color::Green
            } else {
                Color::DarkGray
            };

            // Selection highlight handled by List iteration usually, but we want custom content
            let (name_style, arrow) = if is_focused && i == app.drops_all_selected {
                (
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                    "▸ ",
                )
            } else {
                (
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                    "",
                )
            };

            // Layout: [Icon] Game Name
            //         Active: Campaigns

            let lines = vec![
                Line::from(vec![
                    Span::styled(arrow, Style::default().fg(Color::Yellow)),
                    Span::styled(game.as_str(), name_style),
                    Span::styled(" ∞", Style::default().fg(linked_color)),
                ]),
                Line::from(vec![
                    Span::styled("  Active: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(campaigns_str, Style::default().fg(Color::Gray)),
                ]),
                Line::from(""), // Spacer
            ];

            ListItem::new(lines)
        })
        .collect();

    let list = List::new(items)
        .highlight_style(Style::default()) // Manual highlight
        .highlight_symbol("");

    let mut state = ratatui::widgets::ListState::default();
    state.select(Some(app.drops_all_selected));
    frame.render_stateful_widget(list, inner_area, &mut state);
}

fn render_subscribed_drops_panel(frame: &mut Frame, app: &App, area: Rect) {
    let games = app.get_drops_subscribed_games();
    let is_focused = app.drops_focus == DropsFocus::SubscribedDrops;

    let border_style = if is_focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let title_style = if is_focused {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(" Subscribed Drops ", title_style))
        .border_style(border_style);

    let inner_area = block.inner(area);
    frame.render_widget(block, area);

    if games.is_empty() {
        let p = Paragraph::new("Add game from All Drops to start watching streams")
            .style(Style::default().fg(Color::Gray));
        frame.render_widget(p, inner_area);
        return;
    }

    let items: Vec<ListItem> = games
        .iter()
        .enumerate()
        .map(|(i, game)| {
            let (campaigns_str, is_linked) = app.get_game_display_info(game);
            let linked_color = if is_linked {
                Color::Green
            } else {
                Color::DarkGray
            };

            let (name_style, arrow) = if is_focused && i == app.drops_subscribed_selected {
                (
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                    "▸ ",
                )
            } else {
                (
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                    "",
                )
            };

            let lines = vec![
                Line::from(vec![
                    Span::styled(arrow, Style::default().fg(Color::Yellow)),
                    Span::styled(game.as_str(), name_style),
                    Span::styled(" ∞", Style::default().fg(linked_color)),
                ]),
                Line::from(vec![
                    Span::styled("  Active: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(campaigns_str, Style::default().fg(Color::Gray)),
                ]),
                Line::from(""), // Spacer
            ];

            ListItem::new(lines)
        })
        .collect();

    let list = List::new(items)
        .highlight_style(Style::default())
        .highlight_symbol("");

    let mut state = ratatui::widgets::ListState::default();
    state.select(Some(app.drops_subscribed_selected));
    frame.render_stateful_widget(list, inner_area, &mut state);
}

/// Render the log window.
fn render_logs(frame: &mut Frame, logs: &[String], area: Rect) {
    // Determine max visible lines (height - 2 for borders)
    let max_lines = area.height.saturating_sub(2) as usize;
    if max_lines == 0 {
        return;
    }

    let log_lines: Vec<ListItem> = logs
        .iter()
        .rev()
        .take(max_lines)
        .rev()
        .map(|log| {
            ListItem::new(Line::from(Span::styled(
                log.as_str(),
                Style::default().fg(Color::Gray),
            )))
        })
        .collect();

    let logs_list = List::new(log_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Logs ")
            .style(Style::default().fg(Color::DarkGray)),
    );

    frame.render_widget(logs_list, area);
}

/// Render the bottom status bar with shortcuts.
fn render_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let shortcuts = match app.page {
        Page::Drops => {
            let sub_action = match app.drops_focus {
                DropsFocus::AllDrops => "Subscribe",
                DropsFocus::SubscribedDrops => "Unsubscribe",
            };
            if app.drops_focus == DropsFocus::SubscribedDrops {
                format!(
                    "[↑/↓] Scroll | [Shift+↑/↓] Set Priority | [Enter] {} | [←/→] Switch",
                    sub_action
                )
            } else {
                format!("[↑/↓] Scroll | [Enter] {} | [←/→] Switch List", sub_action)
            }
        }
        Page::Settings => {
            if app.is_logged_in() {
                "[↑/↓] Navigate | [←/→] Switch Panel | [Enter] Select".to_string()
            } else {
                "[↑/↓] Navigate | [←/→] Switch Panel".to_string()
            }
        }
        Page::Home => "[←/→] Switch Panel | [↑/↓] Scroll".to_string(),
        Page::About => "[↑/↓] Scroll".to_string(),
    };

    // Build Stats (Count Unique Games for Active/Sub)

    // Active Games (Deduplicate by Display Name to match UI grouping)
    // Filter by is_active() to ensure we don't count expired campaigns still in inventory
    let active_count = app
        .campaigns
        .iter()
        .filter(|c| c.is_active() && app.config.priority_games.contains(&c.game.display_name))
        .map(|c| c.game.display_name.trim())
        .collect::<std::collections::HashSet<_>>()
        .len();

    // All remains as Campaigns count unless requested otherwise, but usually 'All' matches total list size.
    let all_count = app.all_campaigns.len();

    // Subscribed Games (Unique Games by Display Name)
    let mut subscribed_games = std::collections::HashSet::new();
    // Subscribed should check ALL campaigns, but count unique GAMES.
    for campaign in app.campaigns.iter().chain(app.all_campaigns.iter()) {
        if app
            .config
            .priority_games
            .contains(&campaign.game.display_name)
        {
            subscribed_games.insert(&campaign.game.display_name);
        }
    }
    let subscribed_count = subscribed_games.len();

    // Combined Layout - Single Line
    // "Active: X | Sub: Y | All: Z      ......................      Shortcuts..."

    let stats_spans = vec![
        Span::styled("In Progress: ", Style::default().fg(Color::Gray)),
        Span::styled(
            format!("{}", active_count),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" | "),
        Span::styled("Sub: ", Style::default().fg(Color::Gray)),
        Span::styled(
            format!("{}", subscribed_count),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" | "),
        Span::styled("All: ", Style::default().fg(Color::Gray)),
        Span::styled(
            format!("{}", all_count),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    ];

    // We need to layout these beautifully in one bar.
    // Ratatui Paragraph doesn't easily support "Left Text .... Center Text .... Right Text" in one widget.
    // But we can use Layout again with 2 constraints, but NO borders between them, and one surrounding Block.

    let block = Block::default()
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::White));

    let inner_area = block.inner(area);
    frame.render_widget(block, area);

    // Calculate widths with padding
    // Add spaces to Stats and User strings if not present
    let stats_display_width = stats_spans.iter().map(|s| s.content.len()).sum::<usize>() as u16 + 2; // + padding

    // Determine if we can show shortcuts
    let available_width = inner_area.width.saturating_sub(stats_display_width);
    let shortcuts_needed = shortcuts.len() as u16 + 2; // + padding

    let shortcuts_text = if available_width >= shortcuts_needed {
        // Enough space
        shortcuts
    } else if available_width >= 20 {
        // Tight: show simpler shortcuts or truncated
        "[Using Arrows]".to_string()
    } else {
        // No space, hide
        "".to_string()
    };

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(stats_display_width),
            Constraint::Min(0), // Takes remaining space for Shortcuts
        ])
        .split(inner_area);

    let stats_p =
        Paragraph::new(Line::from(stats_spans)).alignment(ratatui::layout::Alignment::Left); // Explicit left
    let shortcuts_p =
        Paragraph::new(Line::from(shortcuts_text)).alignment(ratatui::layout::Alignment::Right); // Aligned Right

    frame.render_widget(stats_p, chunks[0]);
    frame.render_widget(shortcuts_p, chunks[1]);
}
