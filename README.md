# StandGround

A macOS menu bar app that saves and restores window positions per monitor configuration.

When you dock/undock monitors or change display arrangements, StandGround automatically restores your windows to where they belong.

## Features

- **Save/restore window layouts** per display configuration
- **Auto-restore** — automatically restores windows when monitors change
- **Launch at Login** via LaunchAgent
- **Auto-update** from GitHub releases
- Runs as a menu bar app (no Dock icon)

## Requirements

- macOS
- **Accessibility** permission (System Settings > Privacy & Security > Accessibility)
- **Screen Recording** permission (System Settings > Privacy & Security > Screen Recording)

The app will prompt for these permissions on first launch.

## Install

### From DMG

1. Download `StandGround-darwin-arm64.dmg` (Apple Silicon) or `StandGround-darwin-x86_64.dmg` (Intel) from the [latest release](https://github.com/jcquintas/standground/releases/latest)
2. Open the DMG and drag `StandGround.app` to `/Applications`
3. Launch from Applications or Spotlight

### From standalone binary

1. Download `standground-darwin-arm64.tar.gz` or `standground-darwin-x86_64.tar.gz` from the [latest release](https://github.com/jcquintas/standground/releases/latest)
2. Extract and move to your PATH:
   ```sh
   tar xzf standground-darwin-*.tar.gz
   mv standground /usr/local/bin/
   ```

### From source

```sh
git clone https://github.com/jcquintas/standground.git
cd standground
cargo build --release
cp target/release/standground /usr/local/bin/
```

To build as an app bundle instead:

```sh
./scripts/bundle.sh
cp -r target/StandGround.app /Applications/
```

## Usage

```sh
# Run as a background daemon
standground

# Run in foreground (useful for debugging)
standground --foreground

# Debug mode (verbose logging)
standground --debug

# Print version
standground --version
```

Once running, click the menu bar icon to:

- **Save Current Layout** — snapshot all window positions for the current display setup
- **Restore Layout** — move windows back to their saved positions
- **Auto-restore** — toggle automatic restore on display changes
- **Launch at Login** — toggle starting on login
- **Auto-update** — toggle automatic updates
- **Check for Updates** / **Update** — manually check or install updates

## Configuration

Config and layout data are stored in:

```
~/Library/Application Support/com.standground.standground/
├── config.json     # Settings (auto-restore, launch at login, auto-update)
└── layouts.json    # Saved window layouts per display configuration
```

## License

MIT
