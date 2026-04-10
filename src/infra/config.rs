use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::models::AlertConfig;

// ---------------------------------------------------------------------------
// Section: monitor
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct MonitorConfig {
    pub poll_interval_secs: u64,
    pub fd_warn_pct: u8,
    pub fd_elevated_pct: u8,
    pub fd_crit_pct: u8,
    pub inactivity_timeout_mins: u64,
}

impl Default for MonitorConfig {
    fn default() -> Self {
        Self {
            poll_interval_secs: 3,
            fd_warn_pct: 85,
            fd_elevated_pct: 90,
            fd_crit_pct: 95,
            inactivity_timeout_mins: 30,
        }
    }
}

impl MonitorConfig {
    /// Convert the fd thresholds into an `AlertConfig` used by MonitorService.
    pub fn alert_config(&self) -> AlertConfig {
        AlertConfig {
            warn_pct: self.fd_warn_pct,
            elevated_pct: self.fd_elevated_pct,
            crit_pct: self.fd_crit_pct,
        }
    }
}

// ---------------------------------------------------------------------------
// Section: terminal
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct TerminalConfig {
    pub app: String,
    pub tmux_path: String,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            app: "Ghostty".to_string(),
            tmux_path: detect_tmux_path(),
        }
    }
}

/// Search common locations for the tmux binary.
/// Returns the first path that exists, or falls back to "tmux" (relying on PATH).
fn detect_tmux_path() -> String {
    let candidates = [
        "/opt/homebrew/bin/tmux", // Apple Silicon Homebrew
        "/usr/local/bin/tmux",    // Intel Homebrew / manual install
        "/usr/bin/tmux",          // Xcode CLT / system
    ];
    for path in &candidates {
        if std::path::Path::new(path).exists() {
            return path.to_string();
        }
    }
    "tmux".to_string()
}

// ---------------------------------------------------------------------------
// Section: snapshots
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SnapshotsConfig {
    pub dir: String,
}

impl Default for SnapshotsConfig {
    fn default() -> Self {
        Self {
            dir: "~/.config/tmuxbar/snapshots".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Section: general
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GeneralConfig {
    pub launch_at_login: bool,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            launch_at_login: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Root config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct AppConfig {
    pub monitor: MonitorConfig,
    pub terminal: TerminalConfig,
    pub snapshots: SnapshotsConfig,
    pub general: GeneralConfig,
}

impl AppConfig {
    /// Returns the canonical path to the config file:
    /// `~/.config/tmuxbar/config.toml`
    pub fn config_path() -> PathBuf {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        home.join(".config").join("tmuxbar").join("config.toml")
    }

    /// Load config from disk.
    ///
    /// - If the file does not exist, write a default file and return defaults.
    /// - If the file exists but is not valid TOML, return an error.
    /// - On success, tilde-expand `snapshots.dir`.
    pub fn load() -> Result<Self> {
        let path = Self::config_path();

        if !path.exists() {
            // Create parent dirs and write defaults.
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create config dir {}", parent.display()))?;
            }
            let default_cfg = AppConfig::default();
            let toml_str = toml::to_string_pretty(&default_cfg)
                .context("failed to serialize default config")?;
            std::fs::write(&path, &toml_str)
                .with_context(|| format!("failed to write default config to {}", path.display()))?;
            let mut cfg = default_cfg;
            cfg.expand_tilde();
            return Ok(cfg);
        }

        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read config file {}", path.display()))?;

        let mut cfg: AppConfig =
            toml::from_str(&raw).with_context(|| format!("failed to parse config file {}", path.display()))?;

        cfg.expand_tilde();
        Ok(cfg)
    }

    /// Write the current config back to disk (used by settings UI).
    pub fn save(&self) -> Result<()> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create config dir {}", parent.display()))?;
        }
        let toml_str = toml::to_string_pretty(self).context("failed to serialize config")?;
        std::fs::write(&path, &toml_str)
            .with_context(|| format!("failed to write config to {}", path.display()))?;
        Ok(())
    }

    /// Expand a leading `~` in `snapshots.dir` to the real home directory.
    fn expand_tilde(&mut self) {
        let dir = &self.snapshots.dir;
        if dir.starts_with("~/") || dir == "~" {
            if let Some(home) = dirs::home_dir() {
                let without_tilde = dir.trim_start_matches('~');
                self.snapshots.dir = format!("{}{}", home.display(), without_tilde);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    // Helper: deserialize AppConfig from a TOML string without going through
    // the file-system path (avoids creating ~/.config/tmuxbar/config.toml).
    fn from_toml(s: &str) -> Result<AppConfig> {
        let mut cfg: AppConfig = toml::from_str(s)?;
        cfg.expand_tilde();
        Ok(cfg)
    }

    // --- default values -------------------------------------------------------

    #[test]
    fn test_default_monitor() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.monitor.poll_interval_secs, 3);
        assert_eq!(cfg.monitor.fd_warn_pct, 85);
        assert_eq!(cfg.monitor.fd_elevated_pct, 90);
        assert_eq!(cfg.monitor.fd_crit_pct, 95);
        assert_eq!(cfg.monitor.inactivity_timeout_mins, 30);
    }

    #[test]
    fn test_default_terminal() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.terminal.app, "Ghostty");
        assert_eq!(cfg.terminal.tmux_path, "/opt/homebrew/bin/tmux");
    }

    #[test]
    fn test_default_snapshots() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.snapshots.dir, "~/.config/tmuxbar/snapshots");
    }

    #[test]
    fn test_default_general() {
        let cfg = AppConfig::default();
        assert!(cfg.general.launch_at_login);
    }

    // --- alert_config ---------------------------------------------------------

    #[test]
    fn test_alert_config_matches_monitor() {
        let cfg = AppConfig::default();
        let alert = cfg.monitor.alert_config();
        assert_eq!(alert.warn_pct, cfg.monitor.fd_warn_pct);
        assert_eq!(alert.elevated_pct, cfg.monitor.fd_elevated_pct);
        assert_eq!(alert.crit_pct, cfg.monitor.fd_crit_pct);
    }

    // --- round-trip -----------------------------------------------------------

    #[test]
    fn test_round_trip_serialize_deserialize() {
        let original = AppConfig::default();
        let toml_str = toml::to_string_pretty(&original).expect("serialize failed");
        let restored: AppConfig = toml::from_str(&toml_str).expect("deserialize failed");
        assert_eq!(original, restored);
    }

    #[test]
    fn test_round_trip_preserves_custom_values() {
        let mut cfg = AppConfig::default();
        cfg.monitor.poll_interval_secs = 10;
        cfg.terminal.app = "Terminal".to_string();
        cfg.general.launch_at_login = false;

        let toml_str = toml::to_string_pretty(&cfg).expect("serialize failed");
        let restored: AppConfig = toml::from_str(&toml_str).expect("deserialize failed");
        assert_eq!(restored.monitor.poll_interval_secs, 10);
        assert_eq!(restored.terminal.app, "Terminal");
        assert!(!restored.general.launch_at_login);
    }

    // --- partial config (missing sections use defaults) ----------------------

    #[test]
    fn test_partial_config_only_monitor_section() {
        let toml_str = r#"
[monitor]
poll_interval_secs = 5
"#;
        let cfg = from_toml(toml_str).expect("parse failed");
        assert_eq!(cfg.monitor.poll_interval_secs, 5);
        // Unchanged fields keep defaults.
        assert_eq!(cfg.monitor.fd_warn_pct, 85);
        // Missing sections also keep defaults.
        assert_eq!(cfg.terminal.app, "Ghostty");
        // snapshots.dir will have been tilde-expanded by from_toml().
        let home = dirs::home_dir().expect("no home dir");
        assert_eq!(
            cfg.snapshots.dir,
            format!("{}/.config/tmuxbar/snapshots", home.display())
        );
        assert!(cfg.general.launch_at_login);
    }

    #[test]
    fn test_partial_config_only_terminal_section() {
        let toml_str = r#"
[terminal]
app = "iTerm2"
"#;
        let cfg = from_toml(toml_str).expect("parse failed");
        assert_eq!(cfg.terminal.app, "iTerm2");
        assert_eq!(cfg.terminal.tmux_path, "/opt/homebrew/bin/tmux");
        // Other sections are defaults.
        assert_eq!(cfg.monitor.poll_interval_secs, 3);
    }

    #[test]
    fn test_empty_toml_uses_all_defaults() {
        let cfg = from_toml("").expect("parse failed");
        // All scalar fields match defaults.
        let def = AppConfig::default();
        assert_eq!(cfg.monitor, def.monitor);
        assert_eq!(cfg.terminal, def.terminal);
        assert_eq!(cfg.general, def.general);
        // snapshots.dir will have been tilde-expanded; verify expansion is correct.
        let home = dirs::home_dir().expect("no home dir");
        assert_eq!(
            cfg.snapshots.dir,
            format!("{}/.config/tmuxbar/snapshots", home.display())
        );
    }

    // --- tilde expansion ------------------------------------------------------

    #[test]
    fn test_tilde_expansion_in_snapshots_dir() {
        let toml_str = r#"
[snapshots]
dir = "~/.config/tmuxbar/snapshots"
"#;
        let cfg = from_toml(toml_str).expect("parse failed");
        let home = dirs::home_dir().expect("no home dir");
        let expected = format!("{}/.config/tmuxbar/snapshots", home.display());
        assert_eq!(cfg.snapshots.dir, expected);
        assert!(!cfg.snapshots.dir.starts_with('~'));
    }

    #[test]
    fn test_no_tilde_not_expanded() {
        let toml_str = r#"
[snapshots]
dir = "/absolute/path/snapshots"
"#;
        let cfg = from_toml(toml_str).expect("parse failed");
        assert_eq!(cfg.snapshots.dir, "/absolute/path/snapshots");
    }

    #[test]
    fn test_tilde_only_expanded() {
        let mut cfg = AppConfig::default();
        cfg.snapshots.dir = "~".to_string();
        cfg.expand_tilde();
        let home = dirs::home_dir().expect("no home dir");
        assert_eq!(cfg.snapshots.dir, home.display().to_string());
    }

    // --- invalid TOML returns error ------------------------------------------

    #[test]
    fn test_invalid_toml_returns_error() {
        let bad_toml = "this is not = valid toml ][[[";
        let result: Result<AppConfig, _> = toml::from_str(bad_toml);
        assert!(result.is_err(), "expected parse error for invalid TOML");
    }

    // --- load from temp file -------------------------------------------------

    #[test]
    fn test_load_from_temp_file_partial_config() {
        // Write a partial config to a temp file and verify missing sections
        // get defaults when deserialized directly.
        let mut tmp = NamedTempFile::new().expect("failed to create temp file");
        write!(
            tmp,
            r#"
[terminal]
app = "Alacritty"
"#
        )
        .expect("write failed");

        let raw = std::fs::read_to_string(tmp.path()).expect("read failed");
        let mut cfg: AppConfig = toml::from_str(&raw).expect("parse failed");
        cfg.expand_tilde();

        assert_eq!(cfg.terminal.app, "Alacritty");
        assert_eq!(cfg.terminal.tmux_path, "/opt/homebrew/bin/tmux");
        assert_eq!(cfg.monitor.poll_interval_secs, 3);
        assert!(cfg.general.launch_at_login);
    }

    #[test]
    fn test_save_and_reload_roundtrip() {
        let tmp_dir = tempfile::tempdir().expect("failed to create temp dir");
        // We can't easily override config_path(), so we test serialization /
        // deserialization directly to simulate what save() + load() do.
        let mut original = AppConfig::default();
        original.monitor.poll_interval_secs = 7;
        original.terminal.app = "WezTerm".to_string();
        original.snapshots.dir = "/tmp/snaps".to_string();
        original.general.launch_at_login = false;

        let toml_str = toml::to_string_pretty(&original).expect("serialize");
        let config_path = tmp_dir.path().join("config.toml");
        std::fs::write(&config_path, &toml_str).expect("write");

        let raw = std::fs::read_to_string(&config_path).expect("read");
        let restored: AppConfig = toml::from_str(&raw).expect("parse");

        assert_eq!(restored.monitor.poll_interval_secs, 7);
        assert_eq!(restored.terminal.app, "WezTerm");
        assert_eq!(restored.snapshots.dir, "/tmp/snaps");
        assert!(!restored.general.launch_at_login);
    }
}
