use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Trait definitions (infrastructure adapters)
// ---------------------------------------------------------------------------

/// Abstraction over the tmux CLI for session/window/pane operations.
pub trait TmuxAdapter: Send + Sync {
    fn list_sessions(&self) -> anyhow::Result<Vec<RawSession>>;
    fn list_windows(&self, session: &str) -> anyhow::Result<Vec<RawWindow>>;
    fn list_panes(&self, session: &str, window: &str) -> anyhow::Result<Vec<RawPane>>;
    fn new_session(&self, name: &str) -> anyhow::Result<()>;
    fn kill_session(&self, name: &str) -> anyhow::Result<()>;
    fn kill_server(&self) -> anyhow::Result<()>;
    fn start_server(&self) -> anyhow::Result<()>;
    fn attach_session(&self, name: &str) -> anyhow::Result<()>;
    /// Fetch a fresh activity timestamp (Unix epoch seconds) for a single session
    /// without listing all sessions. Used by InactivityDetector for targeted polling.
    fn session_activity(&self, session: &str) -> anyhow::Result<i64>;
    fn new_window(&self, session: &str, name: &str) -> anyhow::Result<()>;
    fn split_window(&self, session: &str, window: &str) -> anyhow::Result<()>;
    fn send_keys(&self, target: &str, keys: &str) -> anyhow::Result<()>;
    fn select_layout(&self, target: &str, layout: &str) -> anyhow::Result<()>;
}

/// Abstraction over OS-level system metrics.
pub trait SystemProbe: Send + Sync {
    /// Returns `(current_fds, max_fds)`.
    fn fd_usage(&self) -> anyhow::Result<(u64, u64)>;
    fn process_stats(&self, pid: u32) -> anyhow::Result<ProcStats>;
}

// ---------------------------------------------------------------------------
// Raw types from tmux CLI output
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct RawSession {
    pub name: String,
    /// Unix epoch seconds — tmux returns this as a raw integer.
    pub created: i64,
    pub attached_clients: u32,
    /// Unix epoch seconds of last session activity.
    pub activity: i64,
}

#[derive(Debug, Clone)]
pub struct RawWindow {
    pub index: u32,
    pub name: String,
    pub layout: String,
}

#[derive(Debug, Clone)]
pub struct RawPane {
    pub index: u32,
    pub pid: u32,
    pub current_dir: String,
    pub current_command: String,
}

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Session {
    pub name: String,
    pub uptime: chrono::Duration,
    pub foreground_command: String,
    pub attached_clients: u32,
    pub stats: Option<SessionStats>,
}

/// Aggregated stats for a tmux session (sum of all pane ProcStats).
#[derive(Debug, Clone)]
pub struct SessionStats {
    pub cpu_percent: f32,
    pub memory_bytes: u64,
}

/// Raw OS stats for a single process. SessionStats aggregates these across all panes in a session.
#[derive(Debug, Clone)]
pub struct ProcStats {
    pub cpu_percent: f32,
    pub memory_bytes: u64,
}

// ---------------------------------------------------------------------------
// Snapshot types (serde Serialize/Deserialize)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub name: String,
    pub windows: Vec<WindowSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowSnapshot {
    pub name: String,
    pub layout: String,
    pub panes: Vec<PaneSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneSnapshot {
    pub working_dir: String,
    pub index: u32,
}

// ---------------------------------------------------------------------------
// Monitoring types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct MonitorEvent {
    pub fd_current: u64,
    pub fd_max: u64,
    pub fd_percent: u8,
    pub sessions: Vec<SessionStatus>,
}

#[derive(Debug, Clone)]
pub struct SessionStatus {
    pub name: String,
    pub stats: SessionStats,
    pub last_activity: i64,
    /// Unix epoch seconds when the session was created — used to compute uptime.
    pub created: i64,
    /// Number of clients currently attached to this session.
    pub attached_clients: u32,
    /// The command running in the first pane of the first window.
    pub foreground_command: String,
}

// ---------------------------------------------------------------------------
// Alert types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AlertLevel {
    Normal,
    Warning,
    Elevated,
    Critical,
}

/// Configuration for alert thresholds (file-descriptor percentages).
#[derive(Debug, Clone)]
pub struct AlertConfig {
    pub warn_pct: u8,
    pub elevated_pct: u8,
    pub crit_pct: u8,
}

impl Default for AlertConfig {
    fn default() -> Self {
        Self {
            warn_pct: 85,
            elevated_pct: 90,
            crit_pct: 95,
        }
    }
}

// ---------------------------------------------------------------------------
// Logging types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogEvent {
    FdSpike {
        pct: u8,
        timestamp: DateTime<Utc>,
    },
    SessionCreated {
        name: String,
    },
    SessionDestroyed {
        name: String,
    },
    SafeRestart {
        phase: RestartPhase,
        success: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RestartPhase {
    SnapshotSave,
    ServerKill,
    ServerStart,
    SnapshotRestore,
}

// ---------------------------------------------------------------------------
// Command types (UI -> Tokio channel)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppCommand {
    CreateSession { name: String },
    AttachSession { name: String },
    KillSession { name: String },
    KillServer,
    RestartServer,
    Quit,
}
