//! About page UI.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

/// ASCII art logo for Twitch Miner CLI
const LOGO: &[&str] = &[
    " ████████╗██╗    ██╗██╗████████╗ ██████╗██╗  ██╗    ███╗   ███╗██╗███╗   ██╗███████╗██████╗ ",
    " ╚══██╔══╝██║    ██║██║╚══██╔══╝██╔════╝██║  ██║    ████╗ ████║██║████╗  ██║██╔════╝██╔══██╗",
    "    ██║   ██║ █╗ ██║██║   ██║   ██║     ███████║    ██╔████╔██║██║██╔██╗ ██║█████╗  ██████╔╝",
    "    ██║   ██║███╗██║██║   ██║   ██║     ██╔══██║    ██║╚██╔╝██║██║██║╚██╗██║██╔══╝  ██╔══██╗",
    "    ██║   ╚███╔███╔╝██║   ██║   ╚██████╗██║  ██║    ██║ ╚═╝ ██║██║██║ ╚████║███████╗██║  ██║",
    "    ╚═╝    ╚══╝╚══╝ ╚═╝   ╚═╝    ╚═════╝╚═╝  ╚═╝    ╚═╝     ╚═╝╚═╝╚═╝  ╚═══╝╚══════╝╚═╝  ╚═╝",
    "  ██████╗██╗     ██╗",
    " ██╔════╝██║     ██║",
    " ██║     ██║     ██║",
    " ██║     ██║     ██║",
    " ╚██████╗███████╗██║",
    "  ╚═════╝╚══════╝╚═╝",
];

/// Apply a vertical purple gradient to a logo line.
/// Transitions from light purple (191, 148, 255) to deep purple (145, 71, 255).
fn get_gradient_color(line_index: usize, total_lines: usize) -> Color {
    let fraction = line_index as f64 / (total_lines.max(2) - 1) as f64;
    let r = (191.0 + (145.0 - 191.0) * fraction) as u8;
    let g = (148.0 + (71.0 - 148.0) * fraction) as u8;
    let b = 255u8; // Blue stays constant
    Color::Rgb(r, g, b)
}

/// Build the about page content with the logo and text.
fn build_about_content() -> Vec<Line<'static>> {
    let mut content = Vec::new();

    // Empty line at top
    content.push(Line::from(""));

    // Add logo lines with gradient
    let logo_len = LOGO.len();
    for (i, line) in LOGO.iter().enumerate() {
        let color = get_gradient_color(i, logo_len);
        content.push(Line::from(Span::styled(
            line.to_string(),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )));
    }

    // Version
    let version_text = format!("v{}", env!("CARGO_PKG_VERSION"));
    // Center roughly within the logo's width (approx 88 chars)
    content.push(Line::from(Span::styled(
        format!("{:^88}", version_text),
        Style::default().fg(Color::DarkGray),
    )));

    // Empty lines after logo
    content.push(Line::from(""));

    // Description
    content.push(Line::from("Twitch Miner CLI allows you to mine for drops on Twitch without having to watch the stream."));
    content.push(Line::from("Its focused on being able to run in your terminal with a very low RAM footprint and being extremely simple to use."));
    content.push(Line::from(""));

    // Key Features
    content.push(Line::from(Span::styled(
        "Key Features",
        Style::default()
            .add_modifier(Modifier::BOLD)
            .fg(Color::Yellow),
    )));
    content.push(Line::from(
        "• Mine for Twitch drops in your terminal without having to watch the stream",
    ));
    content.push(Line::from(
        "• Change the priority order of games being mined",
    ));
    content.push(Line::from("• Notifications for completed drops"));
    content.push(Line::from("• Small filesize (<2 MB) in one executable"));
    content.push(Line::from(
        "• Low RAM footprint when running (<5 MB on Windows 11)",
    ));
    content.push(Line::from("• Compatible with Windows, Mac, Linux"));
    content.push(Line::from(""));

    // Disclaimer
    content.push(Line::from(Span::styled(
        "Only tested on local machines, untested on docker etc",
        Style::default()
            .fg(Color::Gray)
            .add_modifier(Modifier::ITALIC),
    )));
    content.push(Line::from(""));

    // Useful links
    content.push(Line::from(Span::styled(
        "Useful links",
        Style::default()
            .add_modifier(Modifier::BOLD)
            .fg(Color::Yellow),
    )));
    content.push(Line::from(""));
    content.push(Line::from("Download the latest version here:"));
    content.push(Line::from(Span::styled(
        "https://github.com/Scotty-Cam/twitch-miner-cli/releases",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::UNDERLINED),
    )));
    content.push(Line::from(""));
    content.push(Line::from("GitHub link:"));
    content.push(Line::from(Span::styled(
        "https://github.com/Scotty-Cam/twitch-miner-cli",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::UNDERLINED),
    )));
    content.push(Line::from(""));
    content.push(Line::from(vec![
        Span::raw("If you find this useful please Star "),
        Span::raw("⭐"),
        Span::raw(" the repository on GitHub - it costs nothing"),
    ]));
    content.push(Line::from(""));

    // Special Thanks
    content.push(Line::from(""));
    content.push(Line::from(Span::styled(
        "Special Thanks",
        Style::default()
            .add_modifier(Modifier::BOLD)
            .fg(Color::Yellow),
    )));
    content.push(Line::from(""));
    content.push(Line::from(vec![
        Span::raw("Thanks to DevilXD for their Twitch Drops Miner - "),
        Span::styled(
            "https://github.com/DevilXD/TwitchDropsMiner",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::UNDERLINED),
        ),
    ]));

    content
}

/// Total number of lines in the about page content.
/// Used to clamp scroll position.
pub const ABOUT_CONTENT_LINES: u16 = 48;

/// Render the about page with scrolling support.
pub fn render_about(frame: &mut Frame, area: Rect, scroll: u16) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" About ")
        .title_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );

    let content = build_about_content();

    let paragraph = Paragraph::new(content)
        .block(block)
        .wrap(Wrap { trim: false }) // Enable word wrap but preserve whitespace for ASCII art
        .scroll((scroll, 0));

    frame.render_widget(paragraph, area);
}
