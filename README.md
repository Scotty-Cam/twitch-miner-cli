# Twitch Miner CLI

[![CI](https://github.com/Scotty-Cam/twitch-miner-cli/actions/workflows/ci.yml/badge.svg)](https://github.com/Scotty-Cam/twitch-miner-cli/actions/workflows/ci.yml)
[![Release](https://github.com/Scotty-Cam/twitch-miner-cli/actions/workflows/release.yml/badge.svg)](https://github.com/Scotty-Cam/twitch-miner-cli/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

A fast, lightweight terminal-based Twitch drops miner written in Rust.

![Screenshot](docs/screenshot.png)

## Features

- 🎮 **Automatic Drop Mining** - Watches streams to earn Twitch drops
- 📺 **Multi-Campaign Support** - Tracks all available drop campaigns
- 🔄 **Priority System** - Set game priorities for optimal drop farming
- 🔔 **Desktop Notifications** - Get notified when drops are claimed
- 🌐 **Proxy Support** - HTTP/SOCKS5 proxy configuration
- ⚡ **Lightweight** - ~2MB binary, minimal RAM usage
- 🖥️ **Cross-Platform** - Windows, Linux, macOS

## Installation

### Pre-built Binaries

Download the latest release from the [Releases page](https://github.com/Scotty-Cam/twitch-miner-cli/releases/latest).

### Build from Source

```bash
# Clone the repository
git clone https://github.com/Scotty-Cam/twitch-miner-cli.git
cd twitch-miner-cli

# Build release version
cargo build --release

# Binary will be at: target/release/twitch_miner_cli
```

### macOS Installation

1. Download `twitch_miner_cli-macos-arm64` (for M1/M2/M3) or `x64` (for Intel)
2. Open Terminal and navigate to downloads:
   ```bash
   cd ~/Downloads
   ```
3. Make the binary executable:
   ```bash
   chmod +x twitch_miner_cli-macos-arm64
   ```
4. **Important**: Remove the quarantine attribute (prevents "Malware" warning):
   ```bash
   xattr -d com.apple.quarantine twitch_miner_cli-macos-arm64
   ```
5. Run it:
   ```bash
   ./twitch_miner_cli-macos-arm64
   ```

## Usage

```bash
# Run the application
./twitch_miner_cli

# Or on Windows
twitch_miner_cli.exe
```

### First Run

1. Navigate to **Settings** using arrow keys
2. Select **Account Settings** and press `L` to login
3. Follow the device code flow to authenticate with Twitch
4. Return to **Home** to see available campaigns
5. Add games to your priority list on the **Drops** page

### Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `←` `→` | Navigate between pages |
| `↑` `↓` | Navigate within lists |
| `Enter` | Select/Confirm |
| `L` | Login/Logout |
| `Q` | Quit |
| `Tab` | Switch panel focus |
| `Shift+↑/↓` | Reorder priority |

## Configuration

Settings are stored in `settings.json`:

```json
{
  "auth_path": "auth.json",
  "priority_games": ["Game Name 1", "Game Name 2"],
  "excluded_games": [],
  "notifications_enabled": true,
  "logo_animation_enabled": true,
  "proxy_url": null
}
```

### Proxy Configuration

Set a proxy in Settings or directly in `settings.json`:

```json
{
  "proxy_url": "socks5://user:pass@proxy.example.com:1080"
}
```

Supported formats:
- `http://host:port`
- `http://user:pass@host:port`
- `socks5://host:port`
- `socks5://user:pass@host:port`

## Development

### Prerequisites

- Rust 1.70+ (install via [rustup](https://rustup.rs/))

### Building

```bash
# Debug build
cargo build

# Release build (optimized)
cargo build --release

# Run tests
cargo test

# Run with debug GQL output
cargo run --features debug-gql
```

### Project Structure

```
src/
├── main.rs          # Entry point and event loop
├── app.rs           # Application state and logic
├── auth.rs          # Twitch authentication
├── gql.rs           # GraphQL API client
├── watcher.rs       # Stream watching logic
├── websocket.rs     # PubSub real-time events
├── constants.rs     # API constants
├── notifications.rs # Desktop notifications
├── utils.rs         # Shared utilities
├── models/          # Data structures
│   ├── mod.rs
│   ├── inventory.rs
│   ├── channel.rs
│   └── gql.rs
└── ui/              # TUI rendering
    ├── mod.rs
    ├── dashboard.rs
    ├── settings.rs
    └── about.rs
```

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Acknowledgments

- Inspired by [TwitchDropsMiner](https://github.com/DevilXD/TwitchDropsMiner)
- Built with [Ratatui](https://github.com/ratatui-org/ratatui) TUI framework
- Uses the Twitch GQL API

## Disclaimer

This tool is for educational purposes. Use responsibly and in accordance with Twitch's Terms of Service.
