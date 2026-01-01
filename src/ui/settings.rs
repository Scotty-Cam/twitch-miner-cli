//! Settings page UI.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::app::StateOps;
use crate::app::{App, SettingsFocus, SettingsItem};
use crate::utils::mask_proxy_url;

/// Render the settings page with two-panel layout.
pub fn render_settings(frame: &mut Frame, app: &App, area: Rect) {
    // Split area into 2 panels (matching Home/Drops layout)
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50), // Settings
            Constraint::Percentage(50), // Help
        ])
        .split(area);

    render_settings_panel(frame, app, chunks[0]);
    render_help_panel(frame, app, chunks[1]);
}

/// Render the Settings panel (left side).
fn render_settings_panel(frame: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.settings_focus == SettingsFocus::Settings;

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
        .title(Span::styled(" Settings ", title_style))
        .border_style(border_style);

    let inner_area = block.inner(area);
    frame.render_widget(block, area);

    // Determine height for Account Settings based on state
    // Base height is 2 (Header + Status) + 1 for spacing = 3
    // If showing login code/uri, add 2 lines -> 5
    let account_height =
        if app.is_login_pending() && app.login_code.is_some() && app.login_uri.is_some() {
            5
        } else {
            3
        };

    // Layout for settings items - reduced spacing
    let item_chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(account_height), // Account Settings section
            Constraint::Length(2),              // Notifications section
            Constraint::Length(2),              // Logo Animation section
            Constraint::Length(2),              // Proxy Settings section
            Constraint::Min(0),                 // Remaining space
        ])
        .split(inner_area);

    // Account Settings Section
    render_account_settings_item(frame, app, item_chunks[0]);

    // Notifications Section
    render_notifications_item(frame, app, item_chunks[1]);

    // Logo Animation Section
    render_logo_animation_item(frame, app, item_chunks[2]);

    // Proxy Settings Section
    render_proxy_settings_item(frame, app, item_chunks[3]);
}

/// Render the Account Settings item.
fn render_account_settings_item(frame: &mut Frame, app: &App, area: Rect) {
    let is_selected = app.settings_focus == SettingsFocus::Settings
        && app.settings_selected == SettingsItem::AccountSettings;

    let arrow = if is_selected { "▸ " } else { "  " };
    let arrow_style = if is_selected {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Cyan)
    };

    let title_style = if is_selected {
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    let mut lines = vec![Line::from(vec![
        Span::styled(arrow, arrow_style),
        Span::styled("Account Settings", title_style),
    ])];

    // Status line
    if app.is_login_pending() {
        lines.push(Line::from(vec![
            Span::styled("  Status: ", Style::default().fg(Color::Gray)),
            Span::styled(
                "Logging In...",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));

        if let (Some(code), Some(uri)) = (&app.login_code, &app.login_uri) {
            lines.push(Line::from(vec![
                Span::styled("  Code: ", Style::default().fg(Color::Gray)),
                Span::styled(
                    code.as_str(),
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::styled("  Auth URL: ", Style::default().fg(Color::Gray)),
                Span::styled(uri.as_str(), Style::default().fg(Color::Yellow)),
            ]));
        }
    } else if app.is_logged_in() {
        lines.push(Line::from(vec![
            Span::styled("  Status: ", Style::default().fg(Color::Gray)),
            Span::styled(
                "Logged In",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" │ User: ", Style::default().fg(Color::Gray)),
            Span::styled(
                app.username().unwrap_or("Unknown"),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    } else {
        lines.push(Line::from(vec![
            Span::styled("  Status: ", Style::default().fg(Color::Gray)),
            Span::styled(
                "Not Logged In",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
        ]));
    };

    let paragraph = Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

/// Render the Notifications item.
fn render_notifications_item(frame: &mut Frame, app: &App, area: Rect) {
    let is_selected = app.settings_focus == SettingsFocus::Settings
        && app.settings_selected == SettingsItem::Notifications;

    let arrow = if is_selected { "▸ " } else { "  " };
    let arrow_style = if is_selected {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Cyan)
    };

    let title_style = if is_selected {
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    let status = if app.config.notifications_enabled {
        Span::styled(
            "Enabled ✓",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            "Disabled ✗",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )
    };

    let lines = vec![Line::from(vec![
        Span::styled(arrow, arrow_style),
        Span::styled("Notifications: ", title_style),
        status,
    ])];

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, area);
}

/// Render the Logo Animation item.
fn render_logo_animation_item(frame: &mut Frame, app: &App, area: Rect) {
    let is_selected = app.settings_focus == SettingsFocus::Settings
        && app.settings_selected == SettingsItem::LogoAnimation;

    let arrow = if is_selected { "▸ " } else { "  " };
    let arrow_style = if is_selected {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Cyan)
    };

    let title_style = if is_selected {
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    let status = if app.config.logo_animation_enabled {
        Span::styled(
            "Enabled ✓",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            "Disabled ✗",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
    };

    let lines = vec![Line::from(vec![
        Span::styled(arrow, arrow_style),
        Span::styled("Logo Animation: ", title_style),
        status,
    ])];

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, area);
}

/// Render the Proxy Settings item.
fn render_proxy_settings_item(frame: &mut Frame, app: &App, area: Rect) {
    let is_selected = app.settings_focus == SettingsFocus::Settings
        && app.settings_selected == SettingsItem::ProxySettings;

    let arrow = if is_selected { "▸ " } else { "  " };
    let arrow_style = if is_selected {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Cyan)
    };

    let title_style = if is_selected {
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    // Show input field when editing, otherwise show current value
    let status = if app.proxy_editing {
        // Show input buffer with cursor
        let input_display = format!("{}█", app.proxy_input);
        Span::styled(
            input_display,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
    } else if let Some(ref url) = app.config.proxy_url {
        // Mask credentials in display
        let masked = mask_proxy_url(url);
        Span::styled(format!("✓ {}", masked), Style::default().fg(Color::Green))
    } else {
        Span::styled("Not Set", Style::default().fg(Color::DarkGray))
    };

    let lines = vec![Line::from(vec![
        Span::styled(arrow, arrow_style),
        Span::styled("Proxy: ", title_style),
        status,
    ])];

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, area);
}

fn render_help_panel(frame: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.settings_focus == SettingsFocus::Help;

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
        .title(Span::styled(" Help ", title_style))
        .border_style(border_style);

    let inner_area = block.inner(area);
    frame.render_widget(block, area);

    // Show contextual help based on current selection
    let help_text = match app.settings_selected {
        SettingsItem::AccountSettings => {
            if app.is_login_pending() {
                vec![
                    Line::from(Span::styled(
                        "Logging In",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    )),
                    Line::from(""),
                    Line::from("1. Visit the URL shown in the logs"),
                    Line::from("2. Enter the code displayed"),
                    Line::from("3. Authorize the application"),
                    Line::from(""),
                    Line::from(Span::styled(
                        "Waiting for authorization...",
                        Style::default().fg(Color::DarkGray),
                    )),
                ]
            } else if app.is_logged_in() {
                vec![
                    Line::from(Span::styled(
                        "Account Settings",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    )),
                    Line::from(""),
                    Line::from("Your Twitch account is connected."),
                    Line::from("Ready to earn drops from streams."),
                    Line::from(""),
                    Line::from(vec![
                        Span::raw("Press "),
                        Span::styled(
                            "[L]",
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(" to logout."),
                    ]),
                ]
            } else {
                vec![
                    Line::from(Span::styled(
                        "Account Settings",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    )),
                    Line::from(""),
                    Line::from("Connect your Twitch account to start"),
                    Line::from("mining drops automatically."),
                    Line::from(""),
                    Line::from(vec![
                        Span::raw("Press "),
                        Span::styled(
                            "[L]",
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(" to login with Twitch."),
                    ]),
                ]
            }
        }
        SettingsItem::Notifications => {
            vec![
                Line::from(Span::styled(
                    "Notifications",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from("Toggle desktop notifications for drop"),
                Line::from("completions."),
                Line::from(""),
                Line::from("When enabled, you'll receive a desktop"),
                Line::from("notification when a drop is claimed."),
                Line::from(""),
                Line::from(vec![
                    Span::raw("Press "),
                    Span::styled(
                        "[Enter]",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" to toggle."),
                ]),
            ]
        }
        SettingsItem::LogoAnimation => {
            vec![
                Line::from(Span::styled(
                    "Logo Animation",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from("Toggle the animated pulsing purple"),
                Line::from("logo in the header bar."),
                Line::from(""),
                Line::from("When disabled, the logo will be"),
                Line::from("displayed in grey."),
                Line::from(""),
                Line::from(vec![
                    Span::raw("Press "),
                    Span::styled(
                        "[Enter]",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" to toggle."),
                ]),
            ]
        }
        SettingsItem::ProxySettings => {
            vec![
                Line::from(Span::styled(
                    "Proxy Settings",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from("Configure an HTTP/SOCKS5 proxy for"),
                Line::from("all network requests."),
                Line::from(""),
                Line::from(Span::styled("Format:", Style::default().fg(Color::Yellow))),
                Line::from("http://user:pass@host:port"),
                Line::from("socks5://host:port"),
                Line::from(""),
                Line::from(vec![
                    Span::raw("Press "),
                    Span::styled(
                        "[Enter]",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" to edit."),
                ]),
                Line::from(vec![
                    Span::styled(
                        "[Esc]",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" to cancel."),
                ]),
                Line::from(""),
                Line::from(Span::styled(
                    "⚠ Requires restart to apply",
                    Style::default().fg(Color::Yellow),
                )),
            ]
        }
    };

    let paragraph = Paragraph::new(help_text).wrap(ratatui::widgets::Wrap { trim: true });
    frame.render_widget(paragraph, inner_area);
}
