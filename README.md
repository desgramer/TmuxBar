# TmuxBar

A macOS menu bar app for tmux session management, written in Rust.

## Features

- **Session Management** — Create, attach, rename, and kill tmux sessions from the menu bar
- **System Monitoring** — Real-time file descriptor usage tracking with color-coded menu bar icon (green/yellow/red)
- **fd Alert Escalation** — Escalating notifications at 85%/90%/95%+ thresholds; automatic termination never happens
- **Safe Restart** — Snapshot all sessions → kill-server → start-server → restore all sessions
- **Per-Session Stats** — CPU, memory, uptime, and foreground command per session
- **Inactivity Detection** — Warns when sessions have had no input for a configurable duration
- **Event Logging** — Structured events to SQLite (fd spikes, session lifecycle, restart phases)
- **Hot-Reload Config** — TOML config with file watcher, changes apply without restart
- **i18n** — Korean, English, Japanese, Chinese
- **Login Launch** — Optional LaunchAgent for auto-start at login

## Requirements

- macOS (AppKit-based, no cross-platform support)
- tmux installed (`brew install tmux`)
- Rust toolchain (for building from source)

## Installation

### From Source

```bash
git clone https://github.com/desgramer/TmuxBar.git
cd TmuxBar
cargo build --release
```

The binary will be at `target/release/tmuxbar`.

### Running

```bash
# Run directly
./target/release/tmuxbar

# Or via cargo
cargo run --release
```

## Configuration

TmuxBar creates a default config on first run at `~/.config/tmuxbar/config.toml`.

```toml
[general]
language = "ko"           # ko, en, ja, zh
poll_interval_secs = 3

[alerts]
warn_threshold = 85
elevated_threshold = 90
critical_threshold = 95

[snapshot]
dir = "~/.config/tmuxbar/snapshots"

[launch]
start_at_login = false
```

## Architecture

Three-layer architecture with trait-based dependency injection for testability:

```
┌─────────────────────────────────────┐
│  UI Layer (src/ui/)                 │
│  AppKit via objc2 — NSStatusItem,   │
│  NSMenu, NSAlert, notifications     │
├─────────────────────────────────────┤
│  Core Services (src/core/)          │
│  SessionManager, MonitorService,    │
│  FdAlertPolicy, SnapshotService,    │
│  RestartService, InactivityDetector │
├─────────────────────────────────────┤
│  Infrastructure (src/infra/)        │
│  TmuxClient, SysProbe, Config,     │
│  LogStore, ConfigWatcher,           │
│  LaunchAgent, InstanceLock          │
└─────────────────────────────────────┘
```

- **Tokio + AppKit coexistence** — Tokio `current_thread` runtime on background thread, NSApplication on main thread
- **All external deps behind traits** — `TmuxAdapter`, `SystemProbe` for easy mocking
- **Graceful degradation** — Mutex poison returns early, parse errors skip bad lines

## Runtime Files

| Path | Description |
|------|-------------|
| `~/.config/tmuxbar/config.toml` | Configuration |
| `~/.config/tmuxbar/snapshots/` | Session snapshot JSONs |
| `~/.local/share/tmuxbar/logs.db` | SQLite event log (WAL mode) |
| `~/Library/LaunchAgents/com.tmuxbar.plist` | Login launch agent |
| `/tmp/tmuxbar.lock` | Instance lock |

## Development

```bash
cargo test              # 171 tests
cargo clippy            # lint
cargo fmt -- --check    # format check
```

## License

[MIT](LICENSE)
