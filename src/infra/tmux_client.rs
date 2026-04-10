use anyhow::{anyhow, Context, Result};
use std::process::Command;

use crate::models::{RawPane, RawSession, RawWindow, TmuxAdapter};

// ---------------------------------------------------------------------------
// TmuxClient
// ---------------------------------------------------------------------------

/// Wraps the tmux CLI binary. All tmux operations go through this type.
pub struct TmuxClient {
    pub tmux_path: String,
}

impl TmuxClient {
    pub fn new(tmux_path: impl Into<String>) -> Self {
        Self {
            tmux_path: tmux_path.into(),
        }
    }

    /// Run a tmux sub-command and return its trimmed stdout.
    ///
    /// Returns an error when:
    /// - the binary is not found at `tmux_path`
    /// - the process exits with a non-zero status
    fn run_tmux(&self, args: &[&str]) -> Result<String> {
        let output = Command::new(&self.tmux_path)
            .args(args)
            .output()
            .with_context(|| {
                format!(
                    "tmux binary not found or not executable at '{}'",
                    self.tmux_path
                )
            })?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            return Ok(stdout);
        }

        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(anyhow!(
            "tmux {:?} exited with {}: {}",
            args,
            output.status,
            stderr
        ))
    }
}

// ---------------------------------------------------------------------------
// Parsing helpers (pub(crate) so the unit tests can call them directly)
// ---------------------------------------------------------------------------

pub(crate) fn parse_sessions(output: &str) -> Result<Vec<RawSession>> {
    let mut sessions = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(4, '\t').collect();
        if parts.len() < 4 {
            // Skip malformed lines rather than aborting.
            continue;
        }
        let name = parts[0].to_string();
        let created: i64 = match parts[1].parse() {
            Ok(v) => v,
            Err(_) => {
                tracing::warn!("Skipping session '{name}': invalid created value '{}'", parts[1]);
                continue;
            }
        };
        let attached_clients: u32 = match parts[2].parse() {
            Ok(v) => v,
            Err(_) => {
                tracing::warn!("Skipping session '{name}': invalid attached value '{}'", parts[2]);
                continue;
            }
        };
        let activity: i64 = match parts[3].parse() {
            Ok(v) => v,
            Err(_) => {
                tracing::warn!("Skipping session '{name}': invalid activity value '{}'", parts[3]);
                continue;
            }
        };
        sessions.push(RawSession {
            name,
            created,
            attached_clients,
            activity,
        });
    }
    Ok(sessions)
}

pub(crate) fn parse_windows(output: &str) -> Result<Vec<RawWindow>> {
    let mut windows = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        if parts.len() < 3 {
            continue;
        }
        let index: u32 = parts[0]
            .parse()
            .with_context(|| format!("invalid window_index value '{}'", parts[0]))?;
        let name = parts[1].to_string();
        let layout = parts[2].to_string();
        windows.push(RawWindow {
            index,
            name,
            layout,
        });
    }
    Ok(windows)
}

pub(crate) fn parse_panes(output: &str) -> Result<Vec<RawPane>> {
    let mut panes = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(4, '\t').collect();
        if parts.len() < 4 {
            continue;
        }
        let index: u32 = parts[0]
            .parse()
            .with_context(|| format!("invalid pane_index value '{}'", parts[0]))?;
        let pid: u32 = parts[1]
            .parse()
            .with_context(|| format!("invalid pane_pid value '{}'", parts[1]))?;
        let current_dir = parts[2].to_string();
        let current_command = parts[3].to_string();
        panes.push(RawPane {
            index,
            pid,
            current_dir,
            current_command,
        });
    }
    Ok(panes)
}

// ---------------------------------------------------------------------------
// TmuxAdapter implementation
// ---------------------------------------------------------------------------

impl TmuxAdapter for TmuxClient {
    fn list_sessions(&self) -> Result<Vec<RawSession>> {
        let fmt = "#{session_name}\t#{session_created}\t#{session_attached}\t#{session_activity}";
        match self.run_tmux(&["list-sessions", "-F", fmt]) {
            Ok(output) => parse_sessions(&output),
            Err(e) => {
                let msg = e.to_string().to_lowercase();
                // Treat "no server running" as an empty session list — common on first launch.
                if msg.contains("no server running")
                    || msg.contains("no sessions")
                    || msg.contains("error connecting to")
                {
                    Ok(vec![])
                } else {
                    Err(e)
                }
            }
        }
    }

    fn list_windows(&self, session: &str) -> Result<Vec<RawWindow>> {
        let fmt = "#{window_index}\t#{window_name}\t#{window_layout}";
        let output = self.run_tmux(&["list-windows", "-t", session, "-F", fmt])?;
        parse_windows(&output)
    }

    fn list_panes(&self, session: &str, window: &str) -> Result<Vec<RawPane>> {
        let target = format!("{}:{}", session, window);
        let fmt = "#{pane_index}\t#{pane_pid}\t#{pane_current_path}\t#{pane_current_command}";
        let output = self.run_tmux(&["list-panes", "-t", &target, "-F", fmt])?;
        parse_panes(&output)
    }

    fn new_session(&self, name: &str) -> Result<()> {
        self.run_tmux(&["new-session", "-d", "-s", name])?;
        Ok(())
    }

    fn kill_session(&self, name: &str) -> Result<()> {
        self.run_tmux(&["kill-session", "-t", name])?;
        Ok(())
    }

    fn kill_server(&self) -> Result<()> {
        self.run_tmux(&["kill-server"])?;
        Ok(())
    }

    fn start_server(&self) -> Result<()> {
        self.run_tmux(&["start-server"])?;
        Ok(())
    }

    /// Verify the session exists without actually attaching.
    /// Attaching is the terminal launcher's responsibility; this just checks presence.
    fn attach_session(&self, name: &str) -> Result<()> {
        self.run_tmux(&["has-session", "-t", name])
            .with_context(|| format!("session '{}' does not exist or tmux server is not running", name))?;
        Ok(())
    }

    fn session_activity(&self, session: &str) -> Result<i64> {
        let output =
            self.run_tmux(&["display-message", "-t", session, "-p", "#{session_activity}"])?;
        output
            .trim()
            .parse::<i64>()
            .with_context(|| format!("could not parse session_activity as i64: '{}'", output))
    }

    fn new_window(&self, session: &str, name: &str) -> Result<()> {
        self.run_tmux(&["new-window", "-t", session, "-n", name])?;
        Ok(())
    }

    fn split_window(&self, session: &str, window: &str) -> Result<()> {
        let target = format!("{}:{}", session, window);
        self.run_tmux(&["split-window", "-t", &target])?;
        Ok(())
    }

    fn send_keys(&self, target: &str, keys: &str) -> Result<()> {
        self.run_tmux(&["send-keys", "-t", target, keys, "Enter"])?;
        Ok(())
    }

    fn select_layout(&self, target: &str, layout: &str) -> Result<()> {
        self.run_tmux(&["select-layout", "-t", target, layout])?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Unit tests (parsing only — no actual tmux process)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // parse_sessions
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_sessions_valid() {
        let output = "main\t1700000000\t1\t1700001000\nwork\t1700002000\t0\t1700003000\n";
        let sessions = parse_sessions(output).expect("should parse");
        assert_eq!(sessions.len(), 2);

        assert_eq!(sessions[0].name, "main");
        assert_eq!(sessions[0].created, 1700000000);
        assert_eq!(sessions[0].attached_clients, 1);
        assert_eq!(sessions[0].activity, 1700001000);

        assert_eq!(sessions[1].name, "work");
        assert_eq!(sessions[1].created, 1700002000);
        assert_eq!(sessions[1].attached_clients, 0);
        assert_eq!(sessions[1].activity, 1700003000);
    }

    #[test]
    fn test_parse_sessions_empty() {
        let sessions = parse_sessions("").expect("empty output should return empty vec");
        assert!(sessions.is_empty());
    }

    #[test]
    fn test_parse_sessions_whitespace_only() {
        let sessions = parse_sessions("   \n  \n").expect("whitespace-only should return empty vec");
        assert!(sessions.is_empty());
    }

    #[test]
    fn test_parse_sessions_missing_fields_skipped() {
        // Lines with fewer than 4 tab-separated fields are silently skipped.
        let output = "incomplete\t123\nmain\t1700000000\t1\t1700001000\n";
        let sessions = parse_sessions(output).expect("should parse partial output");
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].name, "main");
    }

    #[test]
    fn test_parse_sessions_invalid_integer_skips_line() {
        let output = "bad\tNOT_A_NUMBER\t1\t1700001000\ngood\t1700000000\t0\t1700001000\n";
        let sessions = parse_sessions(output).expect("should not fail");
        // The malformed line is skipped; only the valid session is returned.
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].name, "good");
    }

    #[test]
    fn test_parse_sessions_session_name_with_special_chars() {
        // Session names can contain hyphens and dots.
        let output = "my-project.dev\t1700000000\t0\t1700001000\n";
        let sessions = parse_sessions(output).expect("should parse");
        assert_eq!(sessions[0].name, "my-project.dev");
    }

    // -----------------------------------------------------------------------
    // parse_windows
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_windows_valid() {
        let output = "0\tbash\t5e53,220x48,0,0\n1\tnvim\teven-horizontal,220x48,0,0\n";
        let windows = parse_windows(output).expect("should parse");
        assert_eq!(windows.len(), 2);

        assert_eq!(windows[0].index, 0);
        assert_eq!(windows[0].name, "bash");
        assert_eq!(windows[0].layout, "5e53,220x48,0,0");

        assert_eq!(windows[1].index, 1);
        assert_eq!(windows[1].name, "nvim");
        assert_eq!(windows[1].layout, "even-horizontal,220x48,0,0");
    }

    #[test]
    fn test_parse_windows_empty() {
        let windows = parse_windows("").expect("empty output should return empty vec");
        assert!(windows.is_empty());
    }

    #[test]
    fn test_parse_windows_missing_fields_skipped() {
        let output = "only-one-field\n0\tbash\t5e53,220x48,0,0\n";
        let windows = parse_windows(output).expect("should parse partial output");
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].name, "bash");
    }

    #[test]
    fn test_parse_windows_layout_contains_tabs() {
        // splitn(3) means the third field absorbs any remaining content.
        let output = "0\tbash\t5e53,220x48,0,0\twhatever\n";
        let windows = parse_windows(output).expect("should parse");
        // layout should include everything after the second tab.
        assert!(windows[0].layout.contains("5e53"));
    }

    // -----------------------------------------------------------------------
    // parse_panes
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_panes_valid() {
        let output =
            "0\t12345\t/home/user/project\tnvim\n1\t12346\t/home/user\tbash\n";
        let panes = parse_panes(output).expect("should parse");
        assert_eq!(panes.len(), 2);

        assert_eq!(panes[0].index, 0);
        assert_eq!(panes[0].pid, 12345);
        assert_eq!(panes[0].current_dir, "/home/user/project");
        assert_eq!(panes[0].current_command, "nvim");

        assert_eq!(panes[1].index, 1);
        assert_eq!(panes[1].pid, 12346);
        assert_eq!(panes[1].current_dir, "/home/user");
        assert_eq!(panes[1].current_command, "bash");
    }

    #[test]
    fn test_parse_panes_empty() {
        let panes = parse_panes("").expect("empty output should return empty vec");
        assert!(panes.is_empty());
    }

    #[test]
    fn test_parse_panes_missing_fields_skipped() {
        let output = "0\t12345\t/home/user\n1\t12346\t/home/user\tbash\n";
        let panes = parse_panes(output).expect("should parse partial output");
        assert_eq!(panes.len(), 1);
        assert_eq!(panes[0].pid, 12346);
    }

    #[test]
    fn test_parse_panes_command_with_spaces() {
        // The 4th field absorbs the rest of the line via splitn(4).
        let output = "0\t99\t/tmp\tpython3 -m http.server\n";
        let panes = parse_panes(output).expect("should parse");
        assert_eq!(panes[0].current_command, "python3 -m http.server");
    }

    #[test]
    fn test_parse_panes_invalid_pid_returns_error() {
        let output = "0\tNOT_A_PID\t/tmp\tbash\n";
        let result = parse_panes(output);
        assert!(result.is_err(), "should fail on invalid pid");
    }
}
