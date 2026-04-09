# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

TmuxBar is a macOS menu bar app for tmux session management, written in Rust. It provides session CRUD, system fd monitoring with escalating alerts, safe server restart with snapshot/restore, and per-session resource tracking. The canonical spec is `TMUXBAR_SPEC.md`.

## Build & Run

```bash
cargo build                    # debug build
cargo build --release          # release build
cargo run                      # run debug
cargo test                     # all tests
cargo test --lib               # unit tests only
cargo test <test_name>         # single test
cargo clippy                   # lint
cargo fmt -- --check           # format check
```

## Architecture (3-layer)

**UI Layer** (`src/ui/`) — AppKit via `objc2`/`objc2-app-kit`. NSStatusItem (menu bar icon with green/yellow/red states), NSMenu (session list), UNUserNotificationCenter (fd alerts with "Restart now" action).

**Core Services** (`src/core/`) — Business logic, all testable without macOS UI:
- `SessionManager` — CRUD sessions, spawn terminal (Ghostty: `open -na Ghostty.app --args -e`; Terminal.app: `osascript`). Falls back from Ghostty to Terminal.app.
- `MonitorService` — Tokio interval timer (default 3s). Reads fd usage + per-session CPU/RSS, emits `MonitorEvent` to broadcast channel.
- `FdAlertPolicy` — State machine for escalating fd notifications (85% warn, 90% elevated, 95%+ per-percent alerts). Never re-notifies same percentage.
- `SnapshotService` — Serializes sessions to JSON (windows, panes, working dirs, layouts). Used during safe restart.
- `InactivityDetector` — Per-session last-activity tracking via `#{session_activity}`.
- `EventLogger` — Structured events to SQLite.

**Infrastructure Adapters** (`src/infra/`) — All external deps behind traits for testability:
- `TmuxAdapter` trait / `TmuxClient` — wraps tmux CLI via `std::process::Command`
- `SystemProbe` trait / `SysProbe` — `sysctl` FFI for fd stats (kern.num_files/kern.maxfiles) + `sysinfo` crate for per-process CPU/RSS
- `Config` — TOML with hot-reload via `notify` crate. Path: `~/.config/tmuxbar/config.toml`
- `LogStore` — SQLite at `~/.local/share/tmuxbar/logs.db`

## Key Design Decisions

- **Trait abstractions for all external deps** — `TmuxAdapter`, `SystemProbe` traits enable mocking in tests. Core services take `Arc<dyn Trait>`.
- **Automatic termination never happens** — fd alerts escalate notifications but the user always decides when to restart.
- **Safe restart sequence** — snapshot all sessions -> kill-server -> start-server -> restore all sessions. Partial restore failures are logged and reported individually.
- **Tokio + AppKit coexistence** — Tokio runtime on background thread, NSApplication on main thread. Main→Tokio via `tokio::sync::mpsc`, Tokio→Main via `dispatch2::DispatchQueue::main().exec_async()`. Never `block_on` on main thread, never `exec_sync` from async context.
- **XDG-style paths** — `~/.config/tmuxbar/` and `~/.local/share/tmuxbar/`, constructed via `dirs::home_dir()` (not `directories` crate, which ignores XDG on macOS).

## Key Crates

`objc2` + `objc2-app-kit` + `dispatch2` (AppKit/GCD bindings), `tokio` (async), `sysinfo` (process stats), `rusqlite` (SQLite WAL), `serde` + `toml` (config), `notify` (file watcher), `tracing` (logging), `anyhow`/`thiserror` (errors)
