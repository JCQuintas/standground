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
