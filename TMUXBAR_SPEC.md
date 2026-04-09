# TmuxBar — Architecture & Implementation Spec

> macOS menu bar app for tmux session management, written in Rust.
> This document is the single source of truth for planning and implementation.

---

## 1. Product requirements

### 1.1 Session management

- Display session list: name, uptime, running process (foreground command).
- Show connected clients (devices) per session.
- **Create session** — opens Ghostty and immediately attaches to the new session.
- **Click existing session** — opens Ghostty and attaches to that session.
- Kill session (individual / kill-server for all).

### 1.2 Monitoring

- Real-time system fd usage tracking (`kern.num_files` / `kern.maxfiles`).
- Menu bar icon colour changes with fd state:
  - **Normal** (green) — below 85 %
  - **Warning** (yellow) — 85 %–94 %
  - **Critical** (red) — 95 %+
- Per-session memory / CPU usage.
- Inactivity detection — warn if a session has had no input for a configurable duration.

### 1.3 fd alert escalation & safe restart

fd alerts **escalate** — the user may be busy and forget the first notification.

| Threshold | Action |
|-----------|--------|
| 85 % | First macOS notification. Icon turns yellow. |
| 90 % | Second notification (elevated urgency). |
| 95 % | Notification every 1 % increment (95, 96, 97, 98, 99). Icon turns red. |

Each notification includes a **"Restart now"** action button.
**Automatic termination never happens.** Timing is always the user's decision.

When the user clicks "Restart now":

1. `SnapshotService.save_all()` — serialise every session to JSON.
2. `tmux kill-server`
3. `tmux start-server`
4. `SnapshotService.restore_all()` — recreate sessions, windows, panes, working directories.
5. Log the entire sequence to `EventLogger`.

### 1.4 Logging

- fd spikes (any time usage crosses a threshold boundary).
- Session create / destroy events.
- Safe-restart events (snapshot save, kill, restore, success/failure).
- Stored in local SQLite (`~/.local/share/tmuxbar/logs.db`).

### 1.5 Settings & configuration

File: `~/.config/tmuxbar/config.toml`

```toml
[monitor]
poll_interval_secs = 3
fd_warn_pct = 85          # first notification
fd_elevated_pct = 90      # elevated warning
fd_crit_pct = 95          # per-percent notifications begin
inactivity_timeout_mins = 30

[terminal]
app = "Ghostty"           # fallback: "Terminal"
tmux_path = "/opt/homebrew/bin/tmux"

[snapshots]
dir = "~/.config/tmuxbar/snapshots"

[general]
launch_at_login = true
```

### 1.6 Edge cases

- **Ghostty not installed** — fall back to `Terminal.app`; show one-time notice.
- **Ghostty security dialog** — `open -na Ghostty.app --args -e ...` triggers a macOS security confirmation ("Allow Ghostty to Execute?") on every invocation. This cannot be disabled (GHSA-q9fg-cpmh-c78x). Users should be informed on first use.
- **tmux not found at configured path** — search `PATH`; if still missing, show error and disable session features.
- **tmux server not running** — `list-sessions` will fail on first launch. Handle gracefully; show empty session list and allow session creation.
- **Notification permission denied** — if `UNUserNotificationCenter` authorization is rejected, fall back to menu bar icon colour changes only. Log a warning.
- **Multiple TmuxBar instances** — use a file lock (`~/.local/share/tmuxbar/tmuxbar.lock`) to prevent concurrent instances. Show notice and exit if lock is held.
- **Snapshot restore partial failure** — restore what we can, log failures, notify user of which sessions could not be restored.
- **SQLite database locked** — use WAL mode for concurrent read/write. Retry with backoff on `SQLITE_BUSY`.

---

## 2. Architecture

```
┌─────────────────────────────────────────────────────────┐
│  UI Layer  (AppKit via objc2 / objc2-app-kit)           │
│  ┌────────────┐ ┌────────────┐ ┌────────┐ ┌──────────┐ │
│  │ MenuBarIcon│ │SessionMenu │ │ Alerts │ │ Settings │ │
│  └────────────┘ └────────────┘ └────────┘ └──────────┘ │
└────────────────────────┬────────────────────────────────┘
                         │  events / callbacks
┌────────────────────────▼────────────────────────────────┐
│  Core Services                                          │
│  ┌─────────────────┐ ┌───────────────┐ ┌─────────────┐ │
│  │ SessionManager  │ │MonitorService │ │SnapshotSvc  │ │
│  │ CRUD, attach    │ │ fd/cpu/mem    │ │ save/restore│ │
│  └─────────────────┘ └───────────────┘ └─────────────┘ │
│  ┌─────────────────┐ ┌───────────────┐ ┌─────────────┐ │
│  │ FdAlertPolicy   │ │InactivityDet. │ │ EventLogger │ │
│  │ escalation logic│ │ per-session   │ │ SQLite sink │ │
│  └─────────────────┘ └───────────────┘ └─────────────┘ │
└────────────────────────┬────────────────────────────────┘
                         │  trait abstractions
┌────────────────────────▼────────────────────────────────┐
│  Infrastructure Adapters                                │
│  ┌───────────┐ ┌──────────┐ ┌────────┐ ┌────────────┐  │
│  │TmuxClient │ │ SysProbe │ │ Config │ │  LogStore  │  │
│  │CLI wrapper│ │sysctl/   │ │  TOML  │ │  SQLite    │  │
│  │           │ │libproc   │ │        │ │            │  │
│  └───────────┘ └──────────┘ └────────┘ └────────────┘  │
└────────────────────────┬────────────────────────────────┘
                         │  process / syscall
┌────────────────────────▼────────────────────────────────┐
│  External                                               │
│  tmux server  ·  Ghostty  ·  macOS kernel  ·  launchd   │
└─────────────────────────────────────────────────────────┘
```

### 2.1 UI layer

| Component | Responsibility | macOS API |
|-----------|---------------|-----------|
| `MenuBarIcon` | Status icon with colour state | `NSStatusItem` |
| `SessionMenu` | Popup listing sessions; create/attach/kill actions | `NSMenu` / `NSMenuItem` |
| `Alerts` | fd warnings, inactivity notices | `UNUserNotificationCenter` |
| `Settings` | Open config file in editor (or future preferences window) | — |

### 2.2 Core services

#### `SessionManager`

```
pub struct SessionManager { tmux: Arc<dyn TmuxAdapter> }

impl SessionManager {
    pub fn list_sessions(&self) -> Result<Vec<Session>>;
    pub fn create_and_attach(&self, name: &str) -> Result<()>;
    pub fn attach(&self, name: &str) -> Result<()>;
    pub fn kill_session(&self, name: &str) -> Result<()>;
    pub fn kill_server(&self) -> Result<()>;
}
```

`create_and_attach` / `attach`:
1. Ensure session exists (create if needed).
2. Spawn terminal with session attach command (terminal-specific):
   - **Ghostty**: `open -na Ghostty.app --args -e <tmux_path> attach -t <name>`
     - `-n` forces a new instance (ensures args are not ignored by existing process).
     - `-e` sets `initial-command`; Ghostty auto-closes when the command exits.
     - Note: macOS shows a security confirmation dialog on each `-e` invocation.
   - **Terminal.app**: `osascript -e 'tell application "Terminal" to do script "<tmux_path> attach -t <name>"'`
     - Always opens a new window.
3. If Ghostty launch fails, retry with `Terminal.app`.

#### `MonitorService`

Runs on a tokio interval timer (configurable, default 3 s).

Each tick:
1. Read system fd via `SysProbe::fd_usage() -> (current, max)` (direct `sysctl` call — `sysinfo` crate does not support `kern.num_files`).
2. Read per-session stats: `tmux list-panes -F '#{pane_pid}'` → `sysinfo::Process` for CPU/RSS. Aggregate per session by **summing** CPU% and RSS across all panes.
3. Emit `MonitorEvent` to a broadcast channel.

Consumers: `FdAlertPolicy`, `InactivityDetector`, UI (icon colour + session list).

#### `FdAlertPolicy`

State machine tracking the **last notified threshold**.

```
pub struct FdAlertPolicy {
    config: AlertConfig,          // warn_pct, crit_pct
    last_notified_pct: Option<u8>,
}

impl FdAlertPolicy {
    /// Returns Some(pct) if a new notification should fire.
    pub fn evaluate(&mut self, current_pct: u8) -> Option<AlertLevel>;
}
```

Notification trigger points: **85, 90, 95, 96, 97, 98, 99**.

Logic:
- `< 85 %` → Normal. Reset `last_notified_pct`.
- Crossing 85 % → first warning notification.
- Crossing 90 % → elevated warning notification.
- `>= 95 %` → notify at every new integer percent (95, 96, 97, 98, 99).
- Never re-notify for the same percentage.
- When usage drops below 85 %, reset all tracking.

#### `SnapshotService`

Snapshot JSON schema per session:

```json
{
  "name": "dev",
  "windows": [
    {
      "name": "editor",
      "layout": "main-vertical",
      "panes": [
        { "working_dir": "/home/user/project", "index": 0 },
        { "working_dir": "/home/user/project/src", "index": 1 }
      ]
    }
  ]
}
```

Save: `tmux list-windows -t <session> -F '...'` + `tmux list-panes ...`
Restore: `tmux new-session` → `tmux new-window` → `tmux split-window` → `tmux send-keys 'cd <dir>' Enter` → `tmux select-layout <layout>`.

#### `InactivityDetector`

Tracks per-session last activity timestamp via `tmux display -p '#{session_activity}'`.
Fires a notification when `now - last_activity > inactivity_timeout`.

#### `EventLogger`

Accepts structured events, writes to `LogStore`.

```rust
pub enum LogEvent {
    FdSpike { pct: u8, timestamp: DateTime<Utc> },
    SessionCreated { name: String },
    SessionDestroyed { name: String },
    SafeRestart { phase: RestartPhase, success: bool },
}
```

### 2.3 Runtime integration (Tokio + AppKit)

AppKit's `NSApplication::run()` blocks the main thread. Tokio runs on a separate background thread. Communication uses typed channels + GCD dispatch.

```
Main thread                     Background thread
─────────────                   ─────────────────
NSApplication::run()            tokio::runtime::Runtime::block_on()
  ├─ NSStatusItem               ├─ MonitorService (interval timer)
  ├─ NSMenu callbacks           ├─ SnapshotService (async I/O)
  └─ UI updates                 └─ EventLogger (SQLite writes)

           ┌─── mpsc::channel<Command> ───┐
  Main ────┤                              ├──── Tokio
           └─ DispatchQueue::main()       ┘
              .exec_async() ◄── AppEvent
```

**Main → Tokio**: `tokio::sync::mpsc` channel. Menu callbacks send `Command` variants (e.g., `CreateSession`, `KillSession`, `RestartServer`).

**Tokio → Main**: `dispatch2::DispatchQueue::main().exec_async(closure)`. The closure runs on the main thread inside the AppKit run loop, safe for UI updates. Always use `exec_async`, never `exec_sync` from async context (deadlock risk).

**Key rules**:
- Never call `tokio::runtime::Runtime::block_on()` on the main thread.
- Never hold `std::sync::Mutex` across `.await` points — use `tokio::sync::Mutex` or scope the lock before await.
- UI objects (`NSStatusItem`, `NSMenu`) must only be accessed on the main thread; `MainThreadMarker` enforces this at compile time.
- On app termination (`applicationWillTerminate`), close the command channel to let the Tokio runtime shut down gracefully.

### 2.4 Infrastructure adapters

All external dependencies are behind traits so core logic is testable with mocks.

```rust
pub trait TmuxAdapter: Send + Sync {
    fn list_sessions(&self) -> Result<Vec<RawSession>>;
    fn list_windows(&self, session: &str) -> Result<Vec<RawWindow>>;
    fn list_panes(&self, session: &str, window: &str) -> Result<Vec<RawPane>>;
    fn new_session(&self, name: &str) -> Result<()>;
    fn kill_session(&self, name: &str) -> Result<()>;
    fn kill_server(&self) -> Result<()>;
    fn start_server(&self) -> Result<()>;
    // ...
}

pub trait SystemProbe: Send + Sync {
    fn fd_usage(&self) -> Result<(u64, u64)>;          // (current, max)
    fn process_stats(&self, pid: u32) -> Result<ProcStats>; // cpu%, rss
}
```

| Adapter | Implementation |
|---------|---------------|
| `TmuxClient` | `std::process::Command` wrapping tmux CLI. Path from Config. |
| `SysProbe` | `sysctl` FFI for fd stats (`kern.num_files`, `kern.maxfiles`). `sysinfo` crate for per-process CPU/RSS. |
| `Config` | `serde` + `toml` crate. Watches file for changes via `notify` crate. |
| `LogStore` | `rusqlite` with WAL mode. Single `events` table with `(id, type, payload_json, timestamp)`. |

### 2.5 Crate dependencies

| Crate | Purpose |
|-------|---------|
| `objc2`, `objc2-app-kit`, `objc2-foundation` | AppKit bindings (NSStatusItem, NSMenu, etc.) |
| `dispatch2` | GCD bindings for main-thread dispatch (part of objc2 ecosystem) |
| `block2` | Objective-C block support (required by dispatch2) |
| `tokio` | Async runtime for polling timers |
| `sysinfo` | Per-process CPU/RSS monitoring (uses libproc internally) |
| `serde`, `serde_json`, `toml` | Serialisation |
| `rusqlite` | SQLite for logs (enable WAL mode) |
| `notify` | File-system watcher for config hot-reload |
| `chrono` | Timestamps |
| `anyhow` / `thiserror` | Error handling |
| `tracing`, `tracing-subscriber` | Structured logging (debug) |

**Not using `directories` crate** — it returns Apple-native paths (`~/Library/Application Support/`) on macOS and ignores XDG env vars. Since tmuxbar uses XDG-style paths (`~/.config/`, `~/.local/share/`) consistent with other terminal tools (tmux, neovim, ghostty), paths are constructed manually via `dirs::home_dir()`.

**`sysinfo` limitations** — `kern.num_files` / `kern.maxfiles` are not supported by `sysinfo`. These two values require direct `sysctl` FFI calls in `SysProbe`.

### 2.6 Launch at login

Register a `LaunchAgent` plist at `~/Library/LaunchAgents/com.tmuxbar.plist` pointing to the app binary. Toggle via config `launch_at_login`.

---

## 3. Project structure

```
tmuxbar/
├── Cargo.toml
├── src/
│   ├── main.rs                 # App entry, run loop
│   ├── app.rs                  # App state, wiring
│   ├── ui/
│   │   ├── mod.rs
│   │   ├── menu_bar.rs         # NSStatusItem setup
│   │   ├── session_menu.rs     # NSMenu with session items
│   │   └── notifications.rs    # UNUserNotificationCenter
│   ├── core/
│   │   ├── mod.rs
│   │   ├── session_manager.rs
│   │   ├── monitor_service.rs
│   │   ├── snapshot_service.rs
│   │   ├── fd_alert_policy.rs
│   │   ├── inactivity_detector.rs
│   │   └── event_logger.rs
│   ├── infra/
│   │   ├── mod.rs
│   │   ├── tmux_client.rs      # TmuxAdapter impl
│   │   ├── sys_probe.rs        # SystemProbe impl (sysctl + libproc)
│   │   ├── config.rs           # TOML config with hot-reload
│   │   └── log_store.rs        # SQLite
│   └── models.rs               # Shared types: Session, Window, Pane, etc.
└── resources/
    ├── icon_normal.png
    ├── icon_warning.png
    ├── icon_critical.png
    └── com.tmuxbar.plist        # LaunchAgent template
```

---

## 4. Implementation order

Suggested phased approach. Each phase is independently testable.

### Phase 1 — Skeleton & infra

1. Cargo project setup, workspace layout.
2. `Config` — TOML read/write, defaults.
3. `TmuxClient` — implement `TmuxAdapter` trait against real tmux CLI.
4. `SysProbe` — fd usage via sysctl, process stats via libproc.
5. `LogStore` — SQLite schema, insert/query.
6. Unit tests for all infra adapters.

### Phase 2 — Core services (headless)

7. `SessionManager` — list, create, kill (no UI yet; test via integration tests).
8. `MonitorService` — polling loop emitting events to a channel.
9. `FdAlertPolicy` — state machine with unit tests for every threshold transition.
10. `InactivityDetector` — per-session tracking.
11. `SnapshotService` — save/restore with snapshot JSON files.
12. `EventLogger` — receives events, writes to LogStore.
13. Integration test: run MonitorService → FdAlertPolicy → EventLogger end-to-end.

### Phase 3 — UI

14. `MenuBarIcon` — NSStatusItem with three icon states.
15. `SessionMenu` — dynamic NSMenu populated from SessionManager.
16. Session click → Ghostty attach flow (with Terminal.app fallback).
17. `Notifications` — UNUserNotificationCenter alerts with "Restart now" action.
18. Wire MonitorService events → icon colour updates + alert notifications.

### Phase 4 — Safe restart & polish

19. Full restart flow: user clicks "Restart now" → snapshot → kill → restore.
20. Launch-at-login (LaunchAgent plist registration).
21. Config hot-reload via `notify` watcher.
22. Error handling audit — ensure all failure paths notify the user gracefully.
23. README, build instructions (`cargo build --release`).
