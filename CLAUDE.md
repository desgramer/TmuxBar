# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

TmuxBar is a macOS menu bar app for tmux session management, written in Rust. It provides session CRUD, system fd monitoring with escalating alerts, safe server restart with snapshot/restore, and per-session resource tracking. The canonical spec is `TMUXBAR_SPEC.md`.

## Build & Run

```bash
cargo build                    # debug build
cargo build --release          # release build
cargo run                      # run debug (requires macOS with tmux installed)
cargo test                     # all tests
cargo test --lib               # unit tests only
cargo test <test_name>         # single test
cargo clippy                   # lint
cargo fmt -- --check           # format check
```

## Architecture (3-layer)

**App Bootstrap** (`src/app.rs`) — Application startup, Tokio runtime + AppKit bridging, main event loop. `src/models.rs` defines shared data types (sessions, events, config structs).

**UI Layer** (`src/ui/`) — AppKit via `objc2`/`objc2-app-kit`:
- `MenuBarApp` — NSStatusItem (menu bar icon with coloured Unicode glyphs per alert level)
- `SessionMenuBuilder` — NSMenu (session list with uptime/stats), accepts optional `MenuActionHandler`
- `MenuActionHandler` — ObjC class via `define_class!` macro; routes NSMenuItem clicks to `AppCommand` via `tokio::sync::mpsc`. Uses target/action pattern with `menuItemClicked:` selector. Tags 0–999 for sessions, 1000+ for fixed actions.
- `NotificationService` — notifications via `osascript` (graceful degradation)

**Core Services** (`src/core/`) — Business logic, all testable without macOS UI:
- `SessionManager` — CRUD sessions, spawn terminal (Ghostty → Terminal.app fallback). Uses `.status()` to prevent zombie processes.
- `MonitorService` — Tokio interval timer (default 3s). Reads fd usage + per-session CPU/RSS/uptime/foreground command, emits `MonitorEvent` to broadcast channel.
- `FdAlertPolicy` — State machine for escalating fd notifications (85% warn, 90% elevated, 95%+ per-percent alerts). `evaluate()` has side effects, `current_level()` is pure.
- `SnapshotService` — Serializes sessions to JSON (windows, panes, working dirs, layouts). Paths are single-quoted in `cd` commands during restore to handle spaces/special chars.
- `InactivityDetector` — Per-session last-activity tracking via `#{session_activity}`.
- `EventLogger` — Structured events to SQLite (fd spikes, session create/destroy, restart phases).
- `RestartService` — Orchestrates snapshot→kill→start→restore with per-phase logging. Runs via `spawn_blocking` to avoid blocking Tokio single-thread runtime.

**Infrastructure Adapters** (`src/infra/`) — All external deps behind traits for testability:
- `TmuxAdapter` trait / `TmuxClient` — wraps tmux CLI via `std::process::Command`. `parse_sessions()` skips malformed lines instead of failing entirely.
- `SystemProbe` trait / `SysProbe` — `sysctl` FFI for fd stats (kern.num_files/kern.maxfiles) with size validation and negative-value rejection. `sysinfo` crate for per-process CPU/RSS.
- `Config` — TOML with hot-reload via `notify` crate. `detect_tmux_path()` searches `/opt/homebrew/bin`, `/usr/local/bin`, `/usr/bin` and falls back to `"tmux"`. `expand_tilde()` resolves `~` in snapshot dir. `alert_config()` validates threshold ordering (warn < elevated < crit).
- `LogStore` — SQLite WAL at `~/.local/share/tmuxbar/logs.db` with 5s `busy_timeout`
- `ConfigWatcher` — File watcher with 500ms debounce for config hot-reload
- `LaunchAgent` — Manages `~/Library/LaunchAgents/com.tmuxbar.plist` for login launch
- `InstanceLock` — File lock (`flock`) preventing multiple instances

## Runtime Files

```
~/.config/tmuxbar/config.toml          # TOML config (created on first run)
~/.config/tmuxbar/snapshots/           # Session snapshot JSONs
~/.config/tmuxbar/.ghostty_notified    # One-time Ghostty notice marker
~/.local/share/tmuxbar/logs.db         # SQLite event log (WAL mode)
~/Library/LaunchAgents/com.tmuxbar.plist  # Login launch agent
/tmp/tmuxbar.lock                      # Instance lock (flock)
```

## Key Design Decisions

- **Trait abstractions for all external deps** — `TmuxAdapter`, `SystemProbe` traits enable mocking in tests. Core services take `Arc<dyn Trait>`.
- **Automatic termination never happens** — fd alerts escalate notifications but the user always decides when to restart.
- **Safe restart sequence** — snapshot all sessions → kill-server → start-server → restore all sessions. Partial restore failures are logged and reported individually.
- **Tokio + AppKit coexistence** — Tokio `current_thread` runtime on background thread, NSApplication on main thread. Main→Tokio via `tokio::sync::mpsc`, Tokio→Main via `dispatch2::DispatchQueue::main().exec_async()`. Never `block_on` on main thread, never `exec_sync` from async context. Blocking work (RestartService) uses `spawn_blocking`.
- **ObjC interop** — `define_class!` macro creates `MainThreadOnly` classes with ivars. Raw pointers (`StatusItemPtr`, `ActionHandlerPtr`) are sent to GCD closures with `unsafe impl Send/Sync`; only dereferenced on main thread.
- **XDG-style paths** — `~/.config/tmuxbar/` and `~/.local/share/tmuxbar/`, constructed via `dirs::home_dir()` with `unwrap_or_else(|| PathBuf::from("."))` fallback.
- **Graceful degradation** — Mutex poison returns early instead of panicking. Parse errors skip bad lines. Missing HOME falls back to `.`.

## Key Crates

`objc2` + `objc2-app-kit` + `dispatch2` (AppKit/GCD bindings), `tokio` (async), `sysinfo` (process stats), `rusqlite` (SQLite WAL), `serde` + `toml` (config), `notify` (file watcher), `libc` (sysctl FFI + flock), `tracing` (logging), `anyhow`/`thiserror` (errors), `block2` (ObjC blocks), `chrono` (time)

## Testing

151 tests (+ 1 ignored), 0 clippy warnings. All core services are tested with mock adapters (MockTmux, MockSysProbe). UI tests are limited to formatting helpers since AppKit requires main thread + NSApplication event loop.

```bash
cargo test                                    # all tests
cargo test core::fd_alert_policy              # state machine tests (21)
cargo test infra::config                      # config round-trip tests (16)
cargo test core::restart_service              # restart flow tests (5)
```

## Known Limitations

- Snapshot restore assumes tmux `base-index=0`. Non-zero base-index causes window target mismatch during restore.
- Settings menu item is wired but handler is a no-op (logs only).
- "Restart now" button in notifications not possible via osascript — restart is triggered from menu instead.
