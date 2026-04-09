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
    fn session_activity(&self, session: &str) -> anyhow::Result<i64>;
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
    pub created: i64,
    pub attached_clients: u32,
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

#[derive(Debug, Clone)]
pub struct SessionStats {
    pub cpu_percent: f32,
    pub memory_bytes: u64,
}

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
