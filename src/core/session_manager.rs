use std::process::Command;
use std::sync::Arc;

use anyhow::Result;
use tracing;

use crate::models::{Session, TmuxAdapter};

/// Sanitize a string for safe interpolation into AppleScript/shell commands.
/// Removes characters that could escape string context.
fn sanitize_for_shell(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_' || *c == '.')
        .collect()
}

/// Manages tmux session CRUD operations and terminal attachment.
///
/// Delegates all tmux interactions to the injected `TmuxAdapter`, and spawns
/// a terminal emulator (Ghostty or Terminal.app) when a user wants to attach.
pub struct SessionManager {
    tmux: Arc<dyn TmuxAdapter>,
    terminal_app: String,
    tmux_path: String,
}

impl SessionManager {
    pub fn new(
        tmux: Arc<dyn TmuxAdapter>,
        terminal_app: impl Into<String>,
        tmux_path: impl Into<String>,
    ) -> Self {
        Self {
            tmux,
            terminal_app: terminal_app.into(),
            tmux_path: tmux_path.into(),
        }
    }

    /// List all tmux sessions, converting raw data into domain `Session` objects.
    ///
    /// For each session the foreground command is resolved by querying the first
    /// pane of the first window. If that lookup fails the command falls back to
    /// `"(unknown)"`.
    pub fn list_sessions(&self) -> Result<Vec<Session>> {
        let raw_sessions = self.tmux.list_sessions()?;
        let now = chrono::Utc::now().timestamp();

        let mut sessions = Vec::with_capacity(raw_sessions.len());
        for raw in &raw_sessions {
            let uptime_secs = now - raw.created;
            let uptime = chrono::Duration::seconds(uptime_secs);

            let foreground_command = self
                .resolve_foreground_command(&raw.name)
                .unwrap_or_else(|| "(unknown)".to_string());

            sessions.push(Session {
                name: raw.name.clone(),
                uptime,
                foreground_command,
                attached_clients: raw.attached_clients,
                stats: None,
            });
        }

        Ok(sessions)
    }

    /// Create a new detached tmux session and open a terminal attached to it.
    pub fn create_and_attach(&self, name: &str) -> Result<()> {
        self.tmux.new_session(name)?;
        self.open_terminal(name)
    }

    /// Attach to an existing tmux session by opening a terminal.
    ///
    /// Verifies the session exists first via `attach_session` (which calls
    /// `has-session` under the hood), then opens the terminal.
    pub fn attach(&self, name: &str) -> Result<()> {
        self.tmux.attach_session(name)?;
        self.open_terminal(name)
    }

    /// Kill a single tmux session by name.
    pub fn kill_session(&self, name: &str) -> Result<()> {
        self.tmux.kill_session(name)
    }

    /// Kill the entire tmux server (all sessions).
    pub fn kill_server(&self) -> Result<()> {
        self.tmux.kill_server()
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    /// Resolve the foreground command of the first pane in the first window.
    fn resolve_foreground_command(&self, session_name: &str) -> Option<String> {
        let windows = self.tmux.list_windows(session_name).ok()?;
        let first_window = windows.first()?;
        let window_index = first_window.index.to_string();
        let panes = self
            .tmux
            .list_panes(session_name, &window_index)
            .ok()?;
        let first_pane = panes.first()?;
        Some(first_pane.current_command.clone())
    }

    /// Open a terminal emulator attached to the given tmux session.
    ///
    /// If the configured terminal is Ghostty, we attempt `open -na Ghostty.app`.
    /// If that spawn fails we fall back to Terminal.app via osascript.
    fn open_terminal(&self, session_name: &str) -> Result<()> {
        let safe_name = sanitize_for_shell(session_name);

        if self.terminal_app.eq_ignore_ascii_case("ghostty") {
            // Ghostty -e takes the command and arguments as separate args.
            let result = Command::new("open")
                .args([
                    "-na", "Ghostty.app", "--args",
                    "-e", &self.tmux_path, "attach", "-t", &safe_name,
                ])
                .status();

            match result {
                Ok(s) if s.success() => return Ok(()),
                Ok(s) => {
                    tracing::warn!(
                        "Ghostty exited with {s}, falling back to Terminal.app"
                    );
                    // Fall through to Terminal.app
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to launch Ghostty, falling back to Terminal.app: {e}"
                    );
                    // Fall through to Terminal.app
                }
            }
        }

        // Terminal.app (default / fallback)
        // Use .status() to wait for osascript to complete and avoid zombie processes.
        let status = Command::new("osascript")
            .args([
                "-e",
                &format!(
                    r#"tell application "Terminal" to do script "{} attach -t {}""#,
                    self.tmux_path, safe_name
                ),
            ])
            .status()?;

        if !status.success() {
            tracing::warn!("osascript exited with {status}");
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{RawPane, RawSession, RawWindow};
    use std::sync::Mutex;

    // -----------------------------------------------------------------------
    // MockTmux — canned data for unit tests
    // -----------------------------------------------------------------------

    /// Records which methods were called and returns pre-configured data.
    struct MockTmux {
        sessions: Vec<RawSession>,
        windows: Vec<RawWindow>,
        panes: Vec<RawPane>,
        /// Track calls to verify delegation.
        calls: Mutex<Vec<String>>,
    }

    impl MockTmux {
        fn new(
            sessions: Vec<RawSession>,
            windows: Vec<RawWindow>,
            panes: Vec<RawPane>,
        ) -> Self {
            Self {
                sessions,
                windows,
                panes,
                calls: Mutex::new(Vec::new()),
            }
        }

        fn record(&self, call: &str) {
            self.calls.lock().unwrap().push(call.to_string());
        }

        fn call_log(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl TmuxAdapter for MockTmux {
        fn list_sessions(&self) -> Result<Vec<RawSession>> {
            self.record("list_sessions");
            Ok(self.sessions.clone())
        }

        fn list_windows(&self, session: &str) -> Result<Vec<RawWindow>> {
            self.record(&format!("list_windows:{session}"));
            Ok(self.windows.clone())
        }

        fn list_panes(&self, session: &str, window: &str) -> Result<Vec<RawPane>> {
            self.record(&format!("list_panes:{session}:{window}"));
            Ok(self.panes.clone())
        }

        fn new_session(&self, name: &str) -> Result<()> {
            self.record(&format!("new_session:{name}"));
            Ok(())
        }

        fn kill_session(&self, name: &str) -> Result<()> {
            self.record(&format!("kill_session:{name}"));
            Ok(())
        }

        fn kill_server(&self) -> Result<()> {
            self.record("kill_server");
            Ok(())
        }

        fn start_server(&self) -> Result<()> {
            self.record("start_server");
            Ok(())
        }

        fn attach_session(&self, name: &str) -> Result<()> {
            self.record(&format!("attach_session:{name}"));
            Ok(())
        }

        fn session_activity(&self, session: &str) -> Result<i64> {
            self.record(&format!("session_activity:{session}"));
            Ok(0)
        }

        fn new_window(&self, _session: &str, _name: &str) -> anyhow::Result<()> { Ok(()) }
        fn split_window(&self, _session: &str, _window: &str) -> anyhow::Result<()> { Ok(()) }
        fn send_keys(&self, _target: &str, _keys: &str) -> anyhow::Result<()> { Ok(()) }
        fn select_layout(&self, _target: &str, _layout: &str) -> anyhow::Result<()> { Ok(()) }
        fn get_global_option(&self, _name: &str) -> anyhow::Result<String> {
            Ok("0".to_string())
        }
    }

    // -----------------------------------------------------------------------
    // Helper: build a SessionManager backed by MockTmux
    // -----------------------------------------------------------------------

    fn make_manager(mock: Arc<MockTmux>) -> SessionManager {
        SessionManager::new(mock, "Terminal", "/opt/homebrew/bin/tmux")
    }

    // -----------------------------------------------------------------------
    // list_sessions tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_list_sessions_converts_raw_to_domain() {
        let now = chrono::Utc::now().timestamp();
        let created = now - 3600; // 1 hour ago

        let mock = Arc::new(MockTmux::new(
            vec![RawSession {
                name: "dev".to_string(),
                created,
                attached_clients: 2,
                activity: now,
            }],
            vec![RawWindow {
                index: 0,
                name: "editor".to_string(),
                layout: "main-vertical".to_string(),
            }],
            vec![RawPane {
                index: 0,
                pid: 1234,
                current_dir: "/home/user".to_string(),
                current_command: "nvim".to_string(),
            }],
        ));

        let mgr = make_manager(mock.clone());
        let sessions = mgr.list_sessions().expect("should succeed");

        assert_eq!(sessions.len(), 1);
        let s = &sessions[0];
        assert_eq!(s.name, "dev");
        assert_eq!(s.foreground_command, "nvim");
        assert_eq!(s.attached_clients, 2);
        assert!(s.stats.is_none());

        // Uptime should be approximately 3600 seconds (allow 2s tolerance for
        // clock drift between `now` computation and the call to Utc::now inside
        // list_sessions).
        let uptime_secs = s.uptime.num_seconds();
        assert!(
            (3598..=3602).contains(&uptime_secs),
            "expected uptime ~3600s, got {uptime_secs}"
        );
    }

    #[test]
    fn test_list_sessions_empty() {
        let mock = Arc::new(MockTmux::new(vec![], vec![], vec![]));
        let mgr = make_manager(mock);
        let sessions = mgr.list_sessions().expect("should succeed");
        assert!(sessions.is_empty());
    }

    #[test]
    fn test_list_sessions_foreground_command_fallback() {
        // When list_windows returns empty, foreground_command should be "(unknown)".
        let mock = Arc::new(MockTmux::new(
            vec![RawSession {
                name: "orphan".to_string(),
                created: chrono::Utc::now().timestamp() - 60,
                attached_clients: 0,
                activity: 0,
            }],
            vec![], // no windows
            vec![],
        ));

        let mgr = make_manager(mock);
        let sessions = mgr.list_sessions().expect("should succeed");
        assert_eq!(sessions[0].foreground_command, "(unknown)");
    }

    #[test]
    fn test_list_sessions_multiple() {
        let now = chrono::Utc::now().timestamp();

        let mock = Arc::new(MockTmux::new(
            vec![
                RawSession {
                    name: "alpha".to_string(),
                    created: now - 120,
                    attached_clients: 1,
                    activity: now,
                },
                RawSession {
                    name: "beta".to_string(),
                    created: now - 7200,
                    attached_clients: 0,
                    activity: now - 3600,
                },
            ],
            vec![RawWindow {
                index: 0,
                name: "shell".to_string(),
                layout: "even-horizontal".to_string(),
            }],
            vec![RawPane {
                index: 0,
                pid: 999,
                current_dir: "/tmp".to_string(),
                current_command: "bash".to_string(),
            }],
        ));

        let mgr = make_manager(mock);
        let sessions = mgr.list_sessions().expect("should succeed");

        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].name, "alpha");
        assert_eq!(sessions[1].name, "beta");
        // Both get the same foreground command from the single mock pane.
        assert_eq!(sessions[0].foreground_command, "bash");
        assert_eq!(sessions[1].foreground_command, "bash");
    }

    // -----------------------------------------------------------------------
    // kill_session tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_kill_session_delegates() {
        let mock = Arc::new(MockTmux::new(vec![], vec![], vec![]));
        let mgr = make_manager(mock.clone());

        mgr.kill_session("doomed").expect("should succeed");
        assert_eq!(mock.call_log(), vec!["kill_session:doomed"]);
    }

    // -----------------------------------------------------------------------
    // kill_server tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_kill_server_delegates() {
        let mock = Arc::new(MockTmux::new(vec![], vec![], vec![]));
        let mgr = make_manager(mock.clone());

        mgr.kill_server().expect("should succeed");
        assert_eq!(mock.call_log(), vec!["kill_server"]);
    }
}
